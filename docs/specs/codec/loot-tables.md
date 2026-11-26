# Codec: loot tables — model, generation algorithm, random sequences, container fill (26.3)

- Verified by suites: `loot-tables` (generated loot: item stacks in order, with the exact random draws, compared with the game)
- Sources: local reference reading of the 26.3 game (2026-11-26); `docs/specs/math/random.md` (random sources);
  `docs/specs/codec/datapacks-and-codecs.md` (codec rules, §7) and `docs/specs/registry/identifiers-registries-tags.md`
  (holders, holder sets, §2.6); `docs/specs/codec/text-and-data-components.md` (item stacks and components). Everything
  below is written in our own words. Vanilla loot tables, predicates, item modifiers and number providers are *data*:
  implementations read them from the user's own jar and must never embed them.
- Split files (all under `docs/specs/codec/loot/`): [`entries.md`](loot/entries.md) (entry types),
  [`functions.md`](loot/functions.md) (item functions), [`conditions.md`](loot/conditions.md) (conditions / predicates),
  [`number-providers.md`](loot/number-providers.md) (int and float providers, range checks), [`contexts.md`](loot/contexts.md)
  (parameters, parameter sets, who generates loot with what, slot sources, NBT / score providers).

## Summary

A **loot table** is a data object that turns a **loot context** (a bag of typed parameters, a luck value and a random
source) into a list of item stacks. A table contains **pools**; a pool rolls a number of times; each roll picks one
**entry** (weighted) among those whose conditions hold; a picked entry produces zero or more stacks; **functions**
("modifiers") rewrite each stack on its way out. The finished stacks are then **split to stack-size** and handed to the
caller (drops, chest fill, fishing, trades, vault ejection, …).

26.3 differs structurally from older versions, and an implementation must follow the 26.3 shapes:
- Conditions, item functions ("item modifiers"), slot sources, **number providers (int and float)** and loot tables are
  all **registries** (`minecraft:predicate`, `minecraft:item_modifier`, `minecraft:slot_source`,
  `minecraft:context_int_provider`, `minecraft:context_float_provider`, `minecraft:loot_table`), and every place that
  holds one accepts either a registry id or an inline definition (a "holder", REG §2.6).
- A pool / entry / function holds **one** optional `condition` and **one** optional `modifier` (not lists). Several are
  expressed with the `all_of` condition and a function list (§3.1).
- There is no `reference` condition or function; a registered predicate is used by naming its id where a condition is
  expected.
- Numbers are not `NumberProvider`/`IntProvider`; they are the *context* providers of `loot/number-providers.md`.

## Inputs and state

- **Loot parameters** (`LootParams`): the level (server world), a typed parameter map, a *luck* float (default 0.0) and
  a map of *dynamic drops* (identifier → callback that emits stacks; used by `dynamic` entries).
- **Parameter set** (`ContextKeySet`, registry `minecraft:context_key_set`): the allowed and required parameters; a
  `LootParams` is *built against* a set and refuses unknown or missing-required parameters (contexts.md).
- **Loot context** (`LootContext`): params + a **random source** + the data lookup (registries) + a *visited* set (used
  only to stop a table that includes itself). The random source is chosen at creation (§5).
- Registries for tables / predicates / modifiers are read-only after load. A table id that is **not in the registry**
  resolves to the *empty table* (no pools, no modifier, empty parameter set) — a missing table silently yields nothing.

## 1. Table codec (`minecraft:loot_table`)

```
{ "type": <context key set id> = "minecraft:generic",   (lenient: an unknown/invalid id falls back to generic)
  "random_sequence": <identifier>,                      (optional)
  "pools": [ <pool>, ... ] = [],
  "modifier": <function>                                (optional; a registered item_modifier id, an inline
                                                         function object, or an inline JSON array of functions) }
```
The table id itself (its registry key) is *not* the random sequence: only the explicit `random_sequence` field
selects one. (Vanilla tables set it to a path equal to their own id; that is data.)
Validation (at load, logged, not blocking except fatal problems such as a table or modifier referencing itself
recursively): parameters used by functions/conditions must be allowed by the table's `type` set; an `alternatives`
entry with an unconditioned non-last child is "unreachable"; composite entries need children; aggregate number
providers need ≥ 1 input. Non-fatal problems are only logged; the table still loads and runs.

## 2. Pool codec

```
{ "rolls": <int provider>,                              (required)
  "bonus_rolls": <float provider> = constant 0.0,
  "entries": [ <entry>, ... ],                          (required list; may be empty)
  "condition": <condition>,                             (optional)
  "modifier": <function> }                              (optional)
```
(`rolls` and `bonus_rolls` accept a bare number, an object with a provider `type`, or a registry id of a stored
provider, per number-providers.md.)

## 3. Generation algorithm

### 3.1 Function application model
- A "function" is a transformation `(stack, context) → stack` (it may return the same mutated stack, a different
  stack, or the empty stack). Every function in the data model is a **conditional function** with an optional own
  `condition` field: when present and false, the stack passes through untouched.
- The `modifier` field accepts: a registered item modifier id; an inline function object (`"function": "<type>"` —
  the discriminator key is `function`); or an inline **array** of functions, which is a `sequence` (functions applied in
  list order, each receiving the previous result). An inline array may not itself carry a condition; use the explicit
  `sequence` function (fields `condition`, `functions`) for that.
- Decoration composes outward: a stack leaving an entry passes **entry modifier → (enclosing composite entries'
  modifiers, innermost first) → pool modifier → table modifier**, then the stack splitter (§3.5).

### 3.2 `getRandomItemsRaw(table, context, output)`
1. Push the table onto the context's visited set. If it was already present (a table that includes itself, directly or
   through `loot_table` entries): log a warning, produce **nothing**, and return (the visited set is unchanged).
2. For each pool, in order: run the pool (§3.3). All output stacks pass through the table `modifier` first (if any).
3. Pop the table from the visited set.

### 3.3 Running a pool (`addRandomItems`)
1. If the pool has a `condition` and it evaluates false → nothing (no draws except those the condition itself makes).
2. `count = rolls.getInt(context) + floor(bonus_rolls.getFloat(context) × luck)` — evaluated in that order, left to
   right; `rolls` first (it may draw random numbers). `luck` is the `LootParams` luck (f32); the product is f32; `floor`
   is toward −∞ to i32. `count` is a 32-bit signed integer sum (wrapping). A non-positive count runs no rolls.
3. Repeat `count` times (**one roll**):
   a. *Collect*: iterate the pool's entries in order, calling `expand(context, collector)` on each (§3.4). Every
      expanded leaf entry yields a weight `w = max(floor(weight + quality × luck), 0)` computed in f32 (`quality × luck`
      in f32; the integer `weight` is converted to f32 before the add; `floor` to i32; clamp below at 0). Leaves with
      `w ≤ 0` are **dropped** (their conditions were still evaluated). Keep the surviving leaves in order and the sum
      `W` of their weights (32-bit int, wrapping on overflow).
   b. If there are no survivors or `W == 0` → nothing happens for this roll.
   c. If exactly **one** survivor → use it **without drawing a random number**.
   d. Otherwise draw `r = random.nextInt(W)` (exactly one draw; `W` must be > 0 — a wrapped-negative `W` would throw) and walk the survivors in order, subtracting each weight
      from `r`; the first one making `r < 0` is chosen.
   e. Run the chosen leaf's `createItemStack(output, context)` (this is where functions and nested draws happen).
4. Weights and conditions are re-evaluated for **every** roll (luck is constant within a table run).

### 3.4 Entry expansion (`expand`)
Every entry, regardless of type, first evaluates its own `condition` (absent = true). If false it contributes nothing
and `expand` returns **false**. Otherwise it expands (type-specific, `entries.md`) and returns whether it "succeeded".
If the entry has a `modifier`, each leaf entry it produces is wrapped so that, when chosen, the modifier is applied to
its stacks (§3.1). `expand` of composite entries decides which children are expanded:
- `alternatives`: children in order; stop at the first child whose `expand` returned true.
- `group`: expand every child; always true.
- `sequence`: expand children in order; stop at the first that returns false; true iff all returned true.

### 3.5 Stack splitting and the public entry points
- `getRandomItems(...)` wraps the output with the **stack splitter** before returning: for each produced stack:
  - a stack whose item is **not enabled** in the world's feature flags is dropped (the empty stack is always enabled);
  - if `count < maxStackSize` (of that stack): pass as is. An **empty stack** (count ≤ 0, or air) also satisfies
    `count < max` and is passed through **as an empty stack** (so callers must skip empties);
  - else repeatedly emit copies with `min(maxStackSize, remaining)` until `remaining ≤ 0` (so a 70-count of a
    64-stack gives 64 then 6; a count of exactly `maxStackSize` also splits into one full copy).
  `getRandomItemsRaw` (used by nested `loot_table` entries and `sequence` / container functions) does **not** split;
  the caller's splitter splits once at the end.
- Entry points and their random source (§5): `getRandomItems(params)` (no seed), `getRandomItems(params, seed)`,
  `getRandomItems(params, randomSource)`, and `fill(container, params, seed)` (§6).

## 4. Luck
`luck` is a float carried in `LootParams`. Callers set it from the interacting player's `luck` attribute (plus the
hook's own luck for fishing). It affects entry weights (`quality`, §3.3), `bonus_rolls`, and nothing else directly;
enchantment-driven chances (`looting` …) read enchantment *levels* from the context instead. With no player the luck
is 0.0.

## 5. Random sources and random sequences

### 5.1 Choosing the context's random source
`LootContext.Builder.create(randomSequenceKey)` picks, in priority order:
1. an **explicit random source** passed by the caller (`getRandomItems(params, randomSource)`);
2. an **explicit seed** `s ≠ 0` → a **legacy 48-bit LCG** source seeded with `s` (`RandomSource.create(s)`;
   see `math/random.md`). A seed of `0` means "no explicit seed" and is ignored;
3. the table's `random_sequence` id (if present) → the server's **random sequence** of that id (§5.2);
4. otherwise the **level's own random source** (`ServerLevel.getRandom()`), a legacy source seeded from a
   process-unique value at startup — **not reproducible**; parity tests must avoid it or capture it through the oracle.

Explicit seeds come from: entity death loot seeds (`DeathLootTableSeed` on mobs; 0 unless set by `/summon` or spawn
data), container `LootTableSeed`, brushable-block seeds. Block drops, fishing, vault ejection and gifts pass no seed
(sequence or level random).

### 5.2 `RandomSequences` (world-saved)
Saved data `random_sequences` in the overworld data folder: `{salt: int, include_world_seed: bool = true,
include_sequence_id: bool = true, sequences: { <identifier>: { "source": [lo:long, hi:long] } } }`. Defaults on a new
world: salt `0`, both flags `true`, no sequences.

`server.getRandomSequence(id)`: look up (or create on first use) the sequence for `id`, and return a random source
backed by that **single persistent xoroshiro128++ generator** (every draw advances stored state and marks the saved
data dirty; successive table runs continue the stream). Creation of the generator for `id` with the current defaults:
1. `s = (include_world_seed ? worldSeed : 0) XOR sign_extend(salt)` — `worldSeed` is the **world's seed** from the
   world-gen settings (one value for the whole server, not per dimension), `salt` is the 32-bit integer.
2. 128-bit seed (unmixed upgrade, `random.md`): `lo = s XOR S`, `hi = lo + G` (wrapping; G, S the golden/silver
   constants of random.md) — **no Mix13 yet**.
3. If `include_sequence_id`: `(lo, hi) ← (lo XOR md5lo, hi XOR md5hi)` where `(md5lo, md5hi)` are the two big-endian
   64-bit halves of the MD5 of the UTF-8 string `"namespace:path"` of `id` (the full identifier text, namespace always
   written).
4. Apply **Mix13** (random.md) to `lo` and `hi` separately; construct xoroshiro128++ with exactly `(lo, hi)` (all-zero
   would become `(G, S)`).
Commands (`/random reset`, `/random reset * <salt> …`) re-create sequences with the arguments given; the `include_*`
flags and `salt` used at creation are not stored per sequence, only the resulting generator state.

The wrapper forwards `nextInt()`, `nextInt(bound)`, `nextLong()`, `nextBoolean()`, `nextFloat()`, `nextDouble()`,
`nextGaussian()`, `fork`, `forkPositional`, `setSeed` to the same generator.

### 5.3 Which operations draw
All draws are from the **single context random source**, in program order. The numbers drawn by the building blocks:
`nextInt(bound)` (weights, uniform int, weighted-list, bonus formulas, enchantment selection, container fill),
`nextFloat()` (random chance, binomial, float uniform, explosion survival, decay, bonus-binomial), `nextBoolean()`
(container fill splitting). The exact order for each function and condition is in the split files.

## 6. Container filling (`fill`)

Used by chests / barrels / hoppers / any `RandomizableContainer` when first opened and by chest-vehicle entities.
Other consumers (`/loot`, drops, fishing, vaults, trades) use `getRandomItems`, not `fill`.

`fill(container, params, optionalSeed)`:
1. Build the context with `optionalSeed` and the table's `random_sequence` (§5.1). Let `random` be its source.
2. `stacks = getRandomItems(context)` — all pools, with the stack splitter (§3.5). May contain empty stacks.
3. `slots` = the indices `0 … size−1` whose current item is **empty**, ascending; then **shuffle** `slots` with the
   in-place shuffle below, using `random`. (Occupied slots are never touched.)
4. **shuffleAndSplit(stacks, slotCount = slots.size, random)**:
   a. Remove every empty stack. Move every stack with `count > 1` out of `stacks` into a separate **splittable** list
      (order preserved); single-count stacks stay in `stacks` in order.
   b. While `slotCount − stacks.size − splittable.size > 0` **and** `splittable` is non-empty:
      - pick index `i = Mth.nextInt(random, 0, splittable.size−1)` (draws `nextInt(size)` only when `size > 1`, else
        index 0 with no draw); remove that stack `A`;
      - `k = Mth.nextInt(random, 1, A.count / 2)` (integer division; draws `nextInt(A.count/2)` only when
        `A.count/2 > 1`, else `k = 1` with no draw); `B = A.split(k)` (B takes `k` items, A keeps the rest);
      - for `A`: if `A.count > 1` **and** `random.nextBoolean()` (drawn only if `A.count > 1`) → append `A` to
        `splittable`, else append `A` to `stacks`;
      - same rule for `B` (using `B.count`).
   c. Append all remaining `splittable` stacks to `stacks` (in list order).
   d. **Shuffle** `stacks` (in-place shuffle below).
5. For each stack of `stacks` in order: if no slot is left, log "over-fill" and **stop** (the remaining stacks are
   lost); otherwise remove the **last** element of `slots` and set that container slot to the stack.
6. The table's `LootTable` key on the container is cleared by the caller *before* generation
   (`unpackLootTable`), so a container never re-rolls.

**In-place shuffle** (`Util.shuffle`): for `i = size` down to `2`: `j = random.nextInt(i)`; swap elements `i−1` and
`j`.

### 6.1 How containers call it (`unpackLootTable(player)`)
Fires the `generate_loot` advancement trigger for server players, clears the container's table key, builds params
with `origin = block-centre of the container` (`x+0.5, y+0.5, z+0.5` of the block position), and if a player is
present adds `luck = player.luck` and `this_entity = player`; parameter set `chest`; seed = the container's stored
`LootTableSeed` (0 if none). Chest vehicles use the same shape with the vehicle position.

## 7. Data shapes shared by everything

- **Holder shapes** (REG §2.6): wherever a table says "condition", "function", "number provider" a value may be a
  string id (registry reference, error if unknown) or an inline object. Number providers additionally accept a bare
  JSON number (constant).
- **Type discriminators:** entries `"type"`, functions `"function"`, conditions `"condition"`, number providers
  `"type"`, slot sources `"type"`, NBT providers `"type"`, score providers `"type"`. All ids are `minecraft:` unless a
  data pack registers more; unknown ids are decode errors. Dispatch codec rules (field order on write) follow
  `datapacks-and-codecs.md` §7.4.
- **Registered type ids** are listed in each split file.

## Edge cases and known quirks

1. A pool with exactly one surviving entry consumes **no** random number for the choice (`entries.md`/§3.3c); with
   two or more it consumes exactly one `nextInt`. A table of single-entry pools is therefore draw-free apart from
   functions and conditions.
2. Entry weight uses `floor(weight + quality·luck)` in f32 then clamps at 0, per leaf, per roll.
3. `rolls` and `bonus_rolls` are evaluated per pool run (not per roll); the count may be 0 or negative → no rolls.
4. Empty stacks (from `discard`, count-0 results, missing items) propagate to the output list; `fill` removes them,
   most callers ignore them.
5. The stack splitter splits at `count ≥ maxStackSize`; the **max stack size comes from the stack's components**
   (`max_stack_size`), so a function that changes components before splitting changes the split.
6. `getInt`/`getFloat` swallow arithmetic failures (division by zero, overflow, non-finite float): the value becomes 0
   / 0.0 and **no exception** escapes (number-providers.md §1).
7. A table that includes itself (directly or via `loot_table` entries) outputs nothing for the inner reference, and
   the warning is not an error.
8. A missing table id → empty table. A table whose `type` set lacks parameters its functions need still *runs*: a
   missing parameter at run time makes the individual function/condition behave as documented (usually no effect /
   false), never an error — except functions marked "requires" below.
9. `enchant_*` functions silently do nothing to items that carry no enchantment component (functions.md).
10. Random sequences persist: generating loot from a table with a `random_sequence` advances the stored generator;
    two tables with the same `random_sequence` id share one stream.

## Constants and IDs

- Registries: `minecraft:loot_table`, `minecraft:predicate`, `minecraft:item_modifier`, `minecraft:slot_source`,
  `minecraft:context_int_provider`, `minecraft:context_float_provider`, `minecraft:context_key_set`,
  type registries: `loot_pool_entry_type` (9), `loot_function_type` (42), `loot_condition_type` (20),
  `context_int_provider_type` (23), `context_float_provider_type` (28), `slot_source_type` (6),
  `loot_nbt_provider_type` (2), `loot_score_provider_type` (2).
- Saved data id `random_sequences`. Default parameter set `generic`.
- Weight default 1, quality default 0; stack split uses the stack's own max size.
- Built-in predicate keys the *game code* looks up by id (data supplied by the jar): `tool/can_silk_touch`,
  `tool/can_shear`, `block/fast_cooking`.

## Open questions

1. **[oracle]** Registry iteration order for `enchant_randomly` / `enchant_with_levels` when no `options` set is given
   (all enchantments) and for tag-derived `HolderSet` order (`tag` entries, `options` tags, `set_random_potion`).
   Probe `HolderLookup.listElements()` for `minecraft:enchantment` / `minecraft:potion` and print `HolderSet.stream()`
   order of the tag `#minecraft:on_random_loot`.
2. **[oracle]** `Level.getRandom()` of a server level for tables without `random_sequence` — verify non-reproducible.
3. **[oracle]** `RandomSequences.get` for a fresh world: print `(lo, hi)` for ids
   `minecraft:chests/simple_dungeon` with several seeds/salts; compare with §5.2.
4. Item/entity/block/location/damage-source **predicates** (`item_filter`, `match_tool`, `entity_properties`,
   `location_check`, `damage_source_properties`, `match_block`) and data-component *predicates* are shared with
   advancements and are **not specified here**: see `codec/predicates/` (README indexes every predicate type).
5. Recipes (`furnace_smelt`), structure search (`exploration_map`), text-component resolution (`set_name`/`set_lore`
   with `entity`), command storage (`storage` providers), scoreboard (`score`, `entity_scores`) depend on other specs.
6. Float edge cases: `Math.round(float)`, `Mth.sin/cos`, `Math.pow` use host precision; see `math/java-math-helpers.md`.

## Oracle probe points
`net.minecraft.world.level.storage.loot.LootTable` (`getRandomItems(LootParams, long, Consumer)`, `getRandomItemsRaw`,
`fill(Container, LootParams, long)`, `createStackSplitter`), `LootPool.addRandomItems`, `LootContext.Builder.create`,
`net.minecraft.world.RandomSequences.get`, `net.minecraft.server.MinecraftServer.getRandomSequence`,
`net.minecraft.world.level.storage.loot.entries.LootPoolEntryContainer.expand`,
`UniformContainerBase.EntryBase.getWeight`. Parity suite plan: for a selection of vanilla tables × luck {0, 1, 3} × tool
enchant levels × 200 seeds (explicit non-zero seeds), compare the ordered stack list (item, count, component patch); and
container fills into empty 27-slot containers, comparing slot contents.

## Parity status
`loot-tables`: not yet run.
