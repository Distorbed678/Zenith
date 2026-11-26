# Codec: loot item functions ("item modifiers") (26.3)

- Verified by suites: `loot-tables`
- Sources: local reference reading of the 26.3 game (2026-11-26); `docs/specs/math/random.md`;
  `docs/specs/codec/text-and-data-components.md` (components, patches, text resolution A.9);
  [`number-providers.md`](number-providers.md). Written in our own words.

## Summary
A **function** rewrites one `ItemStack` in the loot pipeline (`loot-tables.md` §3.1). 42 types (registry
`minecraft:loot_function_type`, discriminator field **`function`**). Every function except the wrapper/identity ones has
the common optional field `condition`; if present and false the stack is returned untouched (the condition is
evaluated **before** any of the function's own draws). Functions mutate the stack **in place** unless noted; callers
treat the returned stack as the result.

Registered ids, in registration order: `set_count, set_item, enchant_with_levels, enchant_randomly, set_enchantments,
set_custom_data, set_components, furnace_smelt, enchanted_count_increase, set_damage, set_attributes, set_name,
exploration_map, set_stew_effect, copy_name, set_contents, modify_contents, filtered, limit_count, apply_bonus,
set_loot_table, explosion_decay, set_lore, fill_player_head, copy_custom_data, copy_state, set_banner_pattern,
set_potion, set_random_dyes, set_random_potion, set_instrument, sequence, copy_components, set_fireworks,
set_firework_explosion, set_book_cover, set_written_book_pages, set_writable_book_pages, toggle_tooltips,
set_ominous_bottle_amplifier, set_custom_model_data, discard`.

Item stack helpers used below (`text-and-data-components.md` B.6): `isEmpty` = air item or count ≤ 0;
`count` may be set to any int (including 0 or above the maximum); `grow(n)` = `count += n`; `limitSize(m)` = if not
empty and `count > m` then `count = m`; `split(n)` removes `min(n, count)` from the stack and returns a copy of that
size; `update(component, default, f)` = `set(component, f(get(component) ?? default))`; `copyWithCount` of an empty stack
is the empty stack.

## Notation
`RNG` is the context random source. "draws" lists random calls in order. `getInt/getFloat` are provider evaluations
that may themselves draw (number-providers.md).

## 1. Wrappers

### `sequence`
`{functions: [function…], condition?}` — applies the listed functions in order (each gets the previous result). An
inline JSON **array** in any `modifier` slot is a condition-less `sequence`. The list elements are item-modifier
holders (id or inline).

### `filtered`
`{item_filter: item predicate, on_pass?: function, on_fail?: function, condition?}`: if the stack matches the item
predicate (item-predicate spec) apply `on_pass`, else `on_fail`; a missing branch leaves the stack unchanged.

### `discard`
Returns the **empty stack**. (The empty stack still travels down the pipeline: `loot-tables.md` §3.5.)

## 2. Count and item

### `set_count`
`{count: int provider, add: bool = false}`: `stack.count = (add ? stack.count : 0) + count.getInt(ctx)` (i32 sum,
wrapping). No clamping; a result ≤ 0 makes the stack empty; counts above the max are split later by the stack
splitter.

### `limit_count`
`{limit: IntLimit}` (number-providers.md §5): `stack.count = limit.clamp(ctx, stack.count)`.

### `set_item`
`{item: item id}`: returns a **new stack** of that item with the same count and the same component *patch*
(`transmuteCopy`); an empty input stays empty.

### `explosion_decay`
No fields. If `explosion_radius` is absent → unchanged. Else `p = 1.0f / radius` and for each of the `n = stack.count`
items one `nextFloat()`; the new count is the number of draws with `nextFloat() <= p`. (`n` draws exactly; none if
`n ≤ 0`.)

### `enchanted_count_increase`
`{enchantment, count: float provider, limit: int = 0, condition?}` — requires `attacking_entity`. If the attacker is a
living entity: `level = best enchantment level` (§Enchantment level lookup); `level == 0` → unchanged. Else
`add = level × count.getFloat(ctx)` (int × f32), `stack.grow(Math.round(add))` (half-up, saturating), then if
`limit > 0` `stack.limitSize(limit)`. A non-living or missing attacker → unchanged.

### `apply_bonus`
`{enchantment, formula: "<id>", parameters?: {...}}` (the pair `formula`/`parameters` is a dispatched optional pair; ids
`minecraft:ore_drops`, `minecraft:uniform_bonus_count`, `minecraft:binomial_with_bonus_count`) — requires `tool`. If no
tool → unchanged. `level` = the tool's level of `enchantment` in its `enchantments` component (0 if none; stored
enchantments of books do **not** count). New count (set directly; `count` = current stack count):
- `uniform_bonus_count {bonusMultiplier: int}`: `count + RNG.nextInt(bonusMultiplier × level + 1)` — **always draws**,
  even at level 0 (`nextInt(1)`); a non-positive bound would crash.
- `ore_drops` (no parameters): if `level > 0`: `bonus = RNG.nextInt(level + 2) − 1`, `bonus = max(bonus, 0)`, result
  `count × (bonus + 1)`; if `level ≤ 0` no draw, count unchanged.
- `binomial_with_bonus_count {extra: int, probability: float}`: for `i` in `0 … level + extra − 1`: if
  `RNG.nextFloat() < probability` then `count++`; (no draws when `level + extra ≤ 0`).

## 3. Enchantments

### Enchantment level lookup (shared)
- *Item level* `L_item(e, stack)`: level of enchantment `e` in the stack's `enchantments` component (0 if absent).
  (Enchanted books use `stored_enchantments`, not counted here.)
- *Entity level* `L_ent(e, entity)` (for living entities): the **maximum** of `L_item(e, stack)` over the entity's
  non-empty equipment-slot stacks, over only those slots matching one of `e`'s `slots` groups, in slot-enum order.

### `enchant_with_levels`
`{levels: int provider, options?: enchantment holder-set, include_additional_cost_component: bool = false, condition?}`.
`cost = levels.getInt(ctx)`. Then the vanilla "enchanting-table selection" (`selectEnchantment`) on the stack with the
random source — **draw order**:
1. If the stack has no `enchantable` component → no enchantments selected, **no draws** (go to step 8).
2. `v` = the `enchantable.value` (int). `cost += 1 + nextInt(v/4 + 1) + nextInt(v/4 + 1)` (two draws, left to right; `v/4`
   is integer division).
3. `span = (nextFloat() + nextFloat() − 1.0f) × 0.15f` (two draws).
4. `cost = clamp(Math.round(cost + cost × span), 1, Integer.MAX_VALUE)` (`cost + cost×span` is f32: int + int×float;
   `Math.round` half up).
5. Candidate list `A`: iterate the source (`options` if present, else **every enchantment in the registry in registry
   order**) and keep those with `isPrimaryItem(stack)` (supported by the stack's item **and** — if the enchantment has
   `primary_items` — also in that set) **or** the stack is exactly the book item. For each kept enchantment, take the
   **highest** level `l` from `maxLevel` down to 1 with `minCost(l) ≤ cost ≤ maxCost(l)` (cost formula
   `base + per_level_above_first × (l − 1)`); if none, the enchantment is not a candidate. Candidate = `(enchantment, l)`.
6. If `A` is non-empty: pick one weighted by the enchantment's `weight` (`random_item`: `total = Σ weight`; if `total
   == 0` no pick and no draw; else `r = nextInt(total)` and walk). Append to `results`.
7. Loop: `while nextInt(50) ≤ cost` (the draw happens on **every** loop test, including the failing last test):
   if `results` is non-empty, remove from `A` every candidate incompatible with the **last** result (`!areCompatible`:
   the same enchantment, or either's `exclusive_set` contains the other); if `A` is empty → `break`; pick one more by
   weight (`nextInt(total)`), append; `cost = cost / 2` (integer division).
8. If the stack is the plain book item, the result stack becomes a **fresh** `enchanted_book` stack (count 1, default
   components only); then for each selected enchantment, `stack.enchant(e, l)` (see below).
9. If `include_additional_cost_component` and the context has `additional_cost_component_allowed` and the result stack
   is non-empty and the **original `levels` value** `> 0`: set component `additional_trade_cost = levels value`.
`enchant(e, l)` = on the stack's enchantments component (`stored_enchantments` for an enchanted book): if `l > 0`,
`level(e) = max(existing, min(l, 255))`. **If the stack lacks that component entirely, nothing happens** (silently).

### `enchant_randomly`
`{options?: holder-set, only_compatible: bool = true, include_additional_cost_component: bool = false, condition?}`:
1. `check = !stack.is(book) && only_compatible`. Candidates = (`options`, else the whole enchantment registry in
   registry order) filtered, if `check`, by `canEnchant(stack)` (the stack's item ∈ `supported_items`).
2. If none: log a warning, return unchanged (no draws). Else `e = candidates[RNG.nextInt(size)]` (**always draws**, even for size 1).
3. `level = Mth.nextInt(RNG, 1, e.maxLevel)` (draws `nextInt(maxLevel)` only if `maxLevel > 1`).
4. If the stack is the book: replace with a fresh `enchanted_book` (count 1). `stack.enchant(e, level)`.
5. If `include_additional_cost_component` and `additional_cost_component_allowed` present:
   `additional_trade_cost = 2 + RNG.nextInt(5 + level × 10) + 3 × level` (one draw).

### `set_enchantments`
`{enchantments: {<enchantment id>: int provider} = {}, add: bool = false}`: if the stack is the book, first
`transmuteCopy` to `enchanted_book` (keeping count and patch). Then for each map entry **in the map's iteration order**:
`add` ⇒ `level = clamp(current + provider.getInt, 0, 255)`, else `level = clamp(provider.getInt, 0, 255)`; set
(`level ≤ 0` removes the entry; max 255). If the stack lacks the enchantment component nothing happens. The decoded
map is an unordered immutable map in the game: with **two or more random** providers the draw order is
**[oracle]-unstable** (not reproducible across runs); vanilla data uses constants.

## 4. Damage

### `set_damage`
`{damage: float provider, add: bool = false}`: only for damageable stacks (`max_damage` and `damage` components present,
no `unbreakable`); else log a warning and return unchanged. `base = add ? 1.0f − (f32)damageValue / (f32)maxDamage : 0.0f`;
`pct = 1.0f − clamp(damage.getFloat + base, 0.0f, 1.0f)`; `damage = floor(pct × maxDamage)` (f32 product, clamped into
`[0, maxDamage]` when stored). So `damage` provider = **remaining durability fraction** (1 = undamaged).

## 5. Components and text

### `set_components`
`{components: <component patch>}`: apply the patch (`text-and-data-components.md` B.4 syntax, `!` removals allowed) to
the stack, then **validate strictly**: components must be valid for the item and `count ≤ maxStackSize` (using the new
components); on any failure the **entire patch is rolled back** (log error). Note validation includes the current
count: a stack already above its max size fails.

### `set_custom_data`
`{tag: SNBT compound or JSON object}` (lenient parsing): merges `tag` **recursively** into `custom_data` (creating it
if absent). Compound merge: existing keys that are compounds in both are merged key by key; otherwise the new value
replaces.

### `copy_custom_data`
`{source: NBT provider, ops: [{source: nbt path, target: nbt path, op: replace|append|merge}]}`. `src = source.get(ctx)`;
absent → unchanged. For each op in order: `sourceTags = path.get(src)`; if empty skip; the working target is a copy of
the stack's `custom_data` compound (created lazily on first need); `replace`: set the target path to the **last**
source tag; `append`: for each list tag found/created at the target path (created as a list) append a copy of every
source tag; `merge`: for each compound at the target path (created as compound) merge every compound source tag.
Errors in a path (syntax at run time) are swallowed per op. After all ops, if the target copy was created, write it back
to `custom_data`. NBT providers: contexts.md §6.

### `copy_name`
`{source: this|attacker|direct_attacker|attacking_player|target_entity|interacting_entity|block_entity}` — if the
source object is a nameable (entity or block entity), `custom_name := source.customName` (a null name **removes** the
custom name).

### `copy_components`
`{source: <entity target>|block_entity|tool, include?: [component types], exclude?: [component types]}`: if the source is
a component map (an item stack): copy components that pass the predicate (`not in exclude` and `in include` when each is
present) onto the stack (overwriting). For entities / block entities (component getters): iterate `include` (default:
**every registered component type in registry order**) skipping `exclude`d types, and set each type the source has.

### `copy_state`
`{block: block id, properties: [string]}` — requires `block_state`. Unknown property names are dropped at decode. For
each listed property (any order; they are independent) the state value is written into the item's `block_state`
component if the context's block state has that property.

### `set_name`
`{name?: text component, entity?: <entity target>, target: custom_name|item_name = custom_name}`: if `name` present,
resolve it (text-and-data-components A.9 with the `entity` as the command source at permission level GAMEMASTER; resolution
failure ⇒ the unresolved text is used) and set the chosen component. No `name` ⇒ unchanged.

### `set_lore`
`{lore: [text component] (≤ 256), mode: {replace_all|replace_section|insert|append (+offset/size)}, entity?}` — resolve
each line like `set_name`; combine with the stack's current `lore` per the **list operation** (§ListOperation, max 256
lines); store as `lore`. If the combined list would exceed 256 lines the operation is rejected (original kept, logged).

### `toggle_tooltips`
`{toggles: {<component type>: bool}}`: for each entry in map order: `tooltip_display.hidden_components` gets the type
removed when the value is `true` (shown) or added when `false` (hidden).

### `set_custom_model_data`
`{floats?, flags?, strings?, colors?}`, each a list-operation block `{values, mode…}` (`ListOperation.StandAlone`,
unbounded). Floats and colors are **providers** evaluated (in list order) when applied: floats via `getFloat`, colors via
`getInt` (a colour may also be written as an int/`#rgb` literal). Each present block updates the corresponding list of
the stack's `custom_model_data` (default empty) using its operation; absent blocks leave the list unchanged. Evaluation
order: floats, then flags, strings, colors.

### `fill_player_head`
`{entity: <target>}`: if the stack is `player_head` and the target entity is a player → `profile` := that player's
resolved game profile (name, UUID, properties). Otherwise unchanged.

## 6. Lists: `ListOperation`
Field `mode` (required in this embedding): `replace_all` (result = new values), `replace_section {offset: int ≥ 0 = 0,
size?: int ≥ 0}`, `insert {offset ≥ 0 = 0}`, `append`. For `original`, `replacement`, `maxSize`:
- `append`: `original ++ replacement`; if the total exceeds `maxSize` → original (error logged).
- `insert(offset)`: `offset > size(original)` → original (error); total > maxSize → original (error); else
  `original[0..offset) ++ replacement ++ original[offset..)`.
- `replace_all`: `replacement` (no size check at run time).
- `replace_section(offset, size?)`: `offset > size(original)` → original (error); else
  `original[0..offset) ++ replacement ++ original[offset + (size ?? replacement.size)..)` (if the resume index ≥ original
  size nothing follows); result larger than `maxSize` → original (error).
Decode check: `replace_section.size` must be ≤ `maxSize` of the embedding (256 for lore, 100 for writable-book pages, unlimited
elsewhere).

## 7. Specific item data

### `set_potion`
`{id: potion id}`: `potion_contents := existing (or empty) with potion = id`.

### `set_random_potion`
`{options?: potion holder-set}`: one random element: if `options` present `getRandomElement` (empty set ⇒ none);
else the whole potion registry in registry id order. One `nextInt(n)` (only if `n > 0`). Sets `potion_contents.potion`.

### `set_random_dyes`
`{number_of_dyes: int provider}`: `n = getInt`; `n ≤ 0` → unchanged. Draw `n` times `nextInt(16)` selecting a colour from
the 16-colour list in id order (white … black, below). The result is a **copy with count 1** (dye mixing) with
`dyed_color` set: let `current` = existing `dyed_color` (if any), and the list of chosen texture-diffuse colours
`d_i`. With integer sums over `current` (if present) and all `d_i`: `R, G, B` channel sums; `I` = sum of
`max(r, g, b)` per colour; `k` = number of colours. `r = R/k`, `g = G/k`, `b = B/k` (integer division);
`avgI = (f32)I / k`; `resI = (f32)max(r, g, b)`; `r' = (int)(r × avgI / resI)` (f32 operations in the order
`(r × avgI) / resI`, then truncation toward zero), same for g, b. Stored colour `0x00RRGGBB`.
Texture-diffuse colours by id order (white, orange, magenta, light_blue, yellow, lime, pink, gray, light_gray, cyan,
purple, blue, brown, green, red, black):
`F9FFFE, F9801D, C74EBD, 3AB3DA, FED83D, 80C71F, F38BAA, 474F52, 9D9D97, 169C9C, 8932B8, 3C44AA, 835432, 5E7C16, B02E26, 1D1D21`.
(If `resI == 0` the division is by zero in f32 → NaN/∞ → cast gives 0 or saturates — edge case for all-black.)

### `set_stew_effect`
`{effects: [{type: mob effect id, duration: int provider}] = []}` (no duplicate effects). Only if the stack is
`suspicious_stew` and the list is non-empty: `entry = effects[RNG.nextInt(size)]` (always draws), `duration =
entry.duration.getInt`, if the effect is **not instantaneous** `duration ×= 20`; append `{effect, duration}` to
`suspicious_stew_effects` (appended; no merge with an existing entry for the same effect).

### `set_banner_pattern`
`{patterns: [{pattern: banner pattern id, color: dye}], append: bool}`: `append` ⇒ `banner_patterns` := existing layers ++
patterns; else replaced.

### `set_fireworks`
`{explosions?: list-operation block {values (≤256 explosions), mode…}, flight_duration?: 0..255}`: on `fireworks`
(default `{flight_duration 0, explosions []}`): flight duration replaced if given; explosions updated by the operation
(max 256).

### `set_firework_explosion`
`{shape?, colors?, fade_colors?, trail?, twinkle?}`: on `firework_explosion` (default: small_ball, no colours, no
trail/twinkle) replace each present field.

### `set_book_cover`
`{title?: string (≤ 32 chars, may be filterable), author?: string, generation?: 0..3}`: on `written_book_content`
(default empty) replace the present fields; pages kept.

### `set_written_book_pages`
`{pages: [text (filterable)], mode…}`: pages updated by the operation (unbounded); content re-validated by the
component.

### `set_writable_book_pages`
`{pages: [string (filterable)], mode…}`: operation with **max 100** pages.

### `set_instrument`
`{options: instrument holder-set}`: if non-empty, `instrument := options[nextInt(size)]` (draws only if the set is
non-empty).

### `set_ominous_bottle_amplifier`
`{amplifier: int provider}`: `ominous_bottle_amplifier := clamp(getInt, 0, 4)`.

### `set_attributes`
`{modifiers: [{id: identifier, attribute: attribute id, operation: add_value|add_multiplied_base|add_multiplied_total,
amount: float provider, slot: slot group or list of groups (non-empty)}], replace: bool = true}`. Base item attribute
modifiers (`attribute_modifiers` component) are replaced from empty if `replace`, else updated in place. For each
modifier **in list order**: `slotGroup = slot[RNG.nextInt(slot.size)]` (**always draws**, even for one slot), *then*
`amount.getFloat` (f32 widened to f64); the entry is added with `withModifierAdded`: any existing entry with the same
attribute **and** modifier id is removed first, then the new entry is appended at the end.

### `set_contents` / `modify_contents` / `set_loot_table`
- `set_contents {component: container|bundle_contents|charged_projectiles, entries: [entry…]}`: if the stack is empty
  → unchanged. Else **every** entry is expanded (conditions respected, composites per their rules) and **every produced
  leaf is created immediately — no weighted selection and no weight draw**; each produced stack passes through the stack
  splitter (feature-flag filter, split at max size). The collected stacks (in order) become the component's contents
  (`copyWithContents`: container: in slots from 0; bundle: appended; projectiles likewise) replacing previous contents.
- `modify_contents {component, modifier: function}`: if the stack is empty or lacks the component → unchanged. Else each
  non-empty content item (copy) has the modifier applied, then is clamped to its max stack size (`limitSize`), then the
  component is rebuilt from the results; empty items are skipped unchanged.
- `set_loot_table {loot_table_id: table reference (id, no inline), seed: long = 0}`: if non-empty, sets component
  `container_loot := {loot_table: id, seed}`.

### `furnace_smelt`
`{use_input_count: bool = true}`: if the stack is empty → unchanged. Looks up the first smelting recipe for the single
input (recipe spec). If found with a non-empty result: `n = (use_input_count ? stack.count : 1) × result.count`; returns a
**new stack** of the result item with `min(n, resultMaxStackSize)`. Else logs and returns the stack unchanged.

### `exploration_map` (hook)
`{destination: structure holder-set, decoration: map decoration type = woodland_mansion, zoom: byte = 2, search_radius:
int = 50, skip_existing_chunks: bool = true}` — requires `origin`. Empty stack / no origin → unchanged. Else if
`skip_existing_chunks`, the structure start at the origin block (of the destination set) is "claimed" (a structure
reference is added); the nearest matching structure within `search_radius` chunks is located (`findNearestMapStructure`,
honouring `skip_existing_chunks`); if none found → unchanged; else the stack becomes a map centred on that structure at
`zoom` (initialised with tracking and unlimited tracking, a biome preview rendered) with a target decoration of
`decoration` and a label key `"+"`. The structure search is a worldgen hook (not specified here).

## Edge cases and known quirks
1. `set_attributes`, `set_stew_effect`, `enchant_randomly` consume a draw even with a single option (`nextInt(1)`
   still advances the generator); `set_random_potion`/`set_instrument` draw only when the candidate set is non-empty.
2. `enchant_with_levels` consumes **two `nextFloat()` and two or more `nextInt` draws** before choosing; the
   `while nextInt(50) ≤ cost` loop always performs at least one draw when candidates exist.
3. `set_item` and book→enchanted-book conversions keep/drop component patches differently (`set_item`,
   `set_enchantments` keep; `enchant_*` fresh).
4. A stack above its max size is legal inside the pipeline; only `set_components` strict validation (and the final
   splitter) care.
5. `set_damage`'s `add: true` treats the *current* durability fraction as a baseline; result is clamped to `[0, max]`.
6. Items whose `enchantable`/enchantment component is missing are untouched by `enchant_*`/`set_enchantments`.
7. `explosion_decay` and the `survives_explosion` condition both use `1.0f / radius` but compare with `<=`.
8. `set_random_dyes` always returns a stack of count 1 (the input count is discarded).

## Open questions
- **[oracle]** `set_enchantments` with ≥ 2 random providers: map iteration order (immutable map salt).
- **[oracle]** Registry orders (enchantments, potions, component types) — `loot-tables.md` Open question 1.
- **[oracle]** `ItemLore` / written-book component validation limits when `set_lore`/`set_written_book_pages` exceed
  component limits.
- Text resolution (`set_name`, `set_lore`) of selectors/scores requires the command-source model (A.9).
- `furnace_smelt` and `exploration_map` depend on the recipe and structure-search specs (hooks).

## Oracle probe points
`LootItemFunctions`, `EnchantmentHelper.selectEnchantment / enchantItem / getAvailableEnchantmentResults`,
`EnchantRandomlyFunction.run`, `ApplyBonusCount.*.calculateNewCount`, `SetItemDamageFunction.run`,
`SetAttributesFunction.updateModifiers`, `DyedItemColor.applyDyes`, `ListOperation.apply`,
`LootTable.shuffleAndSplitItems`.

## Parity status
`loot-tables`: not yet run.
