# Predicates: block, fluid, light, location, adventure-mode (`can_place_on` / `can_break`), `lock` (26.3)

- Verified by suites: `predicates`
- Sources: local reference reading of the 26.3 game (2026-11-26); `bounds-and-basics.md`, `item-and-components.md`,
  `docs/specs/lighting/chunk-steps.md` §10 (light reads), `docs/specs/registry/identifiers-registries-tags.md`,
  `docs/specs/blocks/state-facts.md`; data-pack data in the user's jar read for usage patterns only. Our own words.
  Index and hooks: [README.md](README.md).

## Summary

- **Block predicate** — block holder set, state properties, block-entity NBT, block-entity data components. Used by
  loot `match_block` (inline fields), location predicates (`block`), the item components `can_place_on` / `can_break`
  and several advancement triggers.
- **Fluid predicate**, **light predicate** — small state / light checks used inside location predicates.
- **Location predicate** — position bounds, dimension, biome, structure, smokey, light, block, fluid, can-see-sky,
  evaluated at a world position; the only predicate that needs the world.
- **Adventure-mode predicate** — the value of `can_place_on` / `can_break`: a list of block predicates with a small
  result cache whose quirks are observable.
- **Lock code** — the value of the `lock` component and of container block entities' `lock` key: an item predicate.

## Inputs and state

World hooks used here (catalogued in README §3; each is "read-only" except where stated):
`W.loaded(pos)`, `W.blockState(pos)`, `W.fluidState(pos)`, `W.blockEntity(pos)`, `W.beFullTag(be)`,
`W.beComponents(be)`, `W.biome(pos)`, `W.structurePiece(pos, set)`, `W.smokey(pos)`, `W.light(pos)`,
`W.canSeeSky(pos)`, `W.dimension()`.

## 1. Block predicate

### 1.1 Schema

Fields are decoded **inline in the parent object** (the loot `match_block` condition, a location predicate's `block`
object and an adventure-mode element all use the same object shape):

| Key | Type | Default | Meaning |
|---|---|---|---|
| `blocks` | block holder set (registry `minecraft:block`) | absent | state's block must be in the set |
| `state` | state-properties predicate (`bounds-and-basics.md` §4) | absent | property matchers |
| `nbt` | NBT predicate (§3 there) | absent | block-entity data must contain this |
| `components` | map component-id → value (`item-and-components.md` §2.1) | `{}` | exact component matches on the block entity |
| `predicates` | map predicate-type / component id → predicate (`item-and-components.md` §2.2) | `{}` | partial component predicates on the block entity |

`components` and `predicates` are the two fields of the shared "data component matchers" object, merged into this
object (they are not nested under a key).

**Network** (used when the predicate is sent inside `can_place_on` / `can_break`): in this order —
`optional(holderSet<block>)`, `optional(state-properties stream)`, `optional(NBT stream)` (each a boolean then the
payload), then the matcher block: exact component list (VarInt count, each entry = data-component-type registry id
VarInt + that component's network value) followed by the partial-predicate list (`item-and-components.md` §4). Holder
set wire form: registry spec §2.6 (`0` + tag id for a named set, `count+1` + `count` holder ids for a direct set).

### 1.2 Evaluation

**Predicate state test** `matchesState(state)`: false if `blocks` is present and the state's block is not in it;
otherwise true iff `state` is absent or matches (`bounds-and-basics.md` §4.2).

**`willMatchBlockEntity`** = `nbt` present **or** the matcher block is non-empty (either map non-empty). `requiresNbt` =
`nbt` present.

**Block-entity test** `matchesBlockEntity(level, be)`:
1. if `nbt` present: `be` must exist and `nbt` must *contain-match* the block entity's **full-metadata save** tag
   (`W.beFullTag`: the block entity's own data, then the `components` compound, then `id`, `x`, `y`, `z`), else false;
2. if the matcher block is non-empty: `be` must exist and the matcher block must hold against `W.beComponents(be)`
   (the block entity's stored components plus its implicit components); else false;
3. otherwise true. A predicate with neither does **not** require a block entity (a missing block entity is fine).

**Three call forms** (they differ — port them as three functions):

1. `matches(serverLevel, pos)` (location predicates, some triggers): false if `!W.loaded(pos)`; false if
   `!matchesState(W.blockState(pos))`; if `willMatchBlockEntity`: `matchesBlockEntity(level, W.blockEntity(pos))`
   must hold; else true.
2. `matches(blockInWorld)` (adventure mode): `matchesState(blockInWorld.state)`, and — only when `nbt` is present —
   `blockInWorld.entity` must exist and the **NBT part only** must match. **The `components`/`predicates` matchers are
   not consulted in this form.** `blockInWorld.state` is read lazily; when the position's chunk is not loaded and the
   wrapper was created without chunk loading, the state is *null*; a null state with a `blocks` set present is a
   program error (crash) in the game — treat as unreachable (`can_place_on` checks run on loaded positions).
3. Loot `match_block`: `matchesState(state)` then `matchesBlockEntity(level, blockEntity)` where the state and block
   entity come from the loot parameters (see `loot/conditions.md`); the *components matcher is consulted here* (unlike
   form 2).

## 2. Fluid predicate

JSON: `{fluids?: fluid holder set (registry minecraft:fluid), state?: state-properties predicate}`.
`matches(serverLevel, pos)`: false if `!W.loaded(pos)`; let `f = W.fluidState(pos)` (this is the fluid *state*: the
empty fluid when there is none, with its own properties `level`, `falling` …); false if `fluids` present and `f`'s fluid
is not in it; otherwise true iff `state` is absent or matches `f` (property lookup uses the fluid type's definition).

## 3. Light predicate

JSON: `{light: Ints bounds (default ANY)}`. `matches(serverLevel, pos)` = `W.loaded(pos)` **and**
`light.matches(maxLocalRawBrightness(pos))` where `maxLocalRawBrightness` = `lighting/chunk-steps.md` §10
(`getMaxLocalRawBrightness`: 15 when `x` or `z` is outside `[-30000000, 30000000)`, otherwise
`max(blockLight(pos), skyLight(pos) − skyDarken)` with `skyDarken` the level's cached `int` (`codec/environment-attributes.md`
§8.1) and the sky term 0 for a dimension without a sky engine). The value is *not* clamped below zero by the predicate
(the `max` with the block light already keeps it ≥ 0).

## 4. Location predicate

### 4.1 Schema

All keys optional:

| Key | Type | Meaning |
|---|---|---|
| `position` | `{x?, y?, z?}` each `Doubles` bounds (default ANY) | bounds on the (double) coordinates |
| `biomes` | biome holder set | biome at the block |
| `structures` | structure holder set | any piece of a listed structure covers the block |
| `dimension` | dimension resource key (`ResourceKey<Level>`, an identifier) | must equal the level's dimension |
| `smokey` | bool | campfire-smoke status |
| `light` | light predicate | §3 |
| `block` | block predicate | §1 (`matches(serverLevel,pos)` form) |
| `fluid` | fluid predicate | §2 |
| `can_see_sky` | bool | sky exposure |

**Network:** none. Note `position` is itself an object of three optional bounds (not a bounds); a `position` object
whose three bounds are ANY is legal and behaves as "present but always true".

### 4.2 Evaluation (`matches(level, x, y, z)`, doubles)

Evaluate in this order; the first failure returns false:

1. `position` present: `x.matches(x) && y.matches(y) && z.matches(z)` on the **unrounded** doubles. Else false.
2. `dimension` present: must equal `W.dimension()`.
3. `pos = BlockPos.containing(x, y, z)` = `(floor x, floor y, floor z)` with the saturating `i32` floor of
   `math/java-math-helpers.md` (NaN → 0). `loaded = W.loaded(pos)` (a position outside the valid world bounds is not
   loaded).
4. `biomes` present: requires `loaded` and `biomes ∋ W.biome(pos)` (the biome-zoomed biome at that block,
   `worldgen/biomes-and-climate.md` §5); else false. (An unloaded block never matches a biome / structure / smokey
   constraint.)
5. `structures` present: requires `loaded` and some `StructureStart` among the structures whose *start* is referenced
   by `pos`'s chunk that (a) belongs to a structure in the set, (b) is valid (has pieces), and (c) has a piece whose
   bounding box **contains the block** (inclusive box test on integer coordinates). Referenced starts are those listed
   in the chunk's structure references (`worldgen/structures-placement-and-jigsaw.md`). Else false.
6. `smokey` present: requires `loaded` and `smokey == W.smokey(pos)` where **smokey position** is: scan `i = 1..5`
   downward: at `pos.below(i)` if the state is a *lit campfire* (has `lit` property, is in the campfire tag, `lit=true`)
   ⇒ true; otherwise if the block's collision shape at that position (queried with `pos` as the reference position and
   the empty collision context) intersects the campfire's "virtual post" shape (a 4×16×4-pixel column centred in the block,
   i.e. x,z ∈ [6/16, 10/16], y ∈ [0, 1]) ⇒ return whether the state one block *below that* is a lit campfire; if
   neither, continue; after five steps ⇒ false.
7. `light` present: the light predicate §3 must match at `pos`, else false.
8. `block` present: block predicate form 1 at `pos`, else false.
9. `fluid` present: fluid predicate at `pos`, else false.
10. `can_see_sky` present: result is `(can_see_sky == W.canSeeSky(pos))`, where `canSeeSky` = sky light value at `pos`
    `≥ 15` (`lighting/chunk-steps.md` §10). **No `loaded` check is made here**; for an unloaded position the light
    engine answers `0`, so the value is false. **[oracle]** confirm this for a position inside an unloaded chunk.
11. Everything held ⇒ true. A predicate with no keys matches every position (even unloaded ones).

Callers pass doubles: loot `location_check` passes the origin plus the integer offsets; the entity sub-predicates pass
the entity position, the centre of the supporting block, etc. (`entity.md`).

## 5. Adventure-mode predicate (`can_place_on`, `can_break`)

### 5.1 Schema

- **JSON:** `either(non-empty list of block predicates, single block predicate)`: an array must be non-empty
  ("List must have contents" otherwise); a bare object is one predicate. Encode writes a single object when exactly one
  predicate, else the array. Predicates here decode as in §1.1 *including* `components` / `predicates` keys (even though
  evaluation ignores them).
- **Network:** a VarInt-prefixed list of block predicates in the §1.1 network form.
- Component ids: `minecraft:can_place_on`, `minecraft:can_break` (both persistent + network-synchronised).

### 5.2 Use

`ItemStack.canPlaceOnBlockInAdventureMode(blockInWorld)` / `canBreakBlockInAdventureMode` return **false when the
component is absent** and otherwise `component.test(blockInWorld)`. Callers: item use on a block and block breaking
when the player's abilities forbid building (adventure mode); the creative/spectator flows and tooltips are other
specs. The test result is true iff any predicate in list order satisfies form 2 of §1.2.

### 5.3 Result cache (observable quirk — port it)

Each adventure-mode predicate **object** carries a cache: the last checked `BlockInWorld`, the last result, and a flag
`checksBlockEntity` (initially false). `test(block)`:

1. **Cache hit** when the cached block exists, its block *state* is the identical state object as the new block's
   state (state identity, not position), and — only if `checksBlockEntity` is set — the block entities agree:
   both absent, or both present with equal `saveWithId` compounds (`saveWithoutMetadata` + `id`, no `x/y/z`); one absent
   and one present is a miss. On a hit return the cached result.
2. Otherwise store `block` as the cached block, set `checksBlockEntity = false`, then walk the predicates in order:
   the first one that matches sets `checksBlockEntity |= predicate.requiresNbt`, `lastResult = true`, returns true.
   If none match: `lastResult = false`, return false (the flag stays false).

Consequences: a **negative** result is cached by state identity alone — a predicate list such as `[{blocks: chest,
nbt: …}]` that failed on chest A (same block state) is returned again for chest B with different block-entity data
until a different state is checked in between. Positive results cached from predicates with `nbt` re-validate the
block entity. The cache lives on the predicate object, which is **shared by copies of the stack** (component values are
shared on copy) and recreated by decoding. **[oracle]** to pin when a decode creates a fresh object (login, hotbar sync)
versus sharing; probe: place/break sequences with an nbt-constrained `can_break`.

### 5.4 Tooltip (client; for completeness)

If any predicate lacks `blocks` the tooltip is the single "unknown use" line; otherwise the distinct blocks across all
predicates' sets (in order of first appearance, `HolderSet` iteration order) rendered by block name in dark gray.
Headers: `item.canBreak` / `item.canPlace` in gray.

## 6. Lock code (`lock` component, container block entities)

- **Schema:** the value is exactly an **item predicate** (`item-and-components.md` §1): JSON/NBT/network form of the
  item predicate. Component id `minecraft:lock`; it has **no dedicated network codec**, so on the wire it is the NBT
  encoding (registry-aware ops) of the item-predicate codec.
- `NO_LOCK` is the empty item predicate (all fields default; it matches every stack including the empty stack).
- `unlocksWith(stack)` = predicate.test(stack). `canUnlock(player)` = player is a spectator **or**
  `unlocksWith(main-hand stack)`.
- **Block-entity persistence:** key `lock` read leniently (missing or invalid ⇒ `NO_LOCK`); written only when the value
  is not the `NO_LOCK` singleton (identity, so an explicitly decoded empty predicate **is** written back as `{}`).
- Old string locks (`Lock: "name"`, non-empty) are converted by the data fixer to the item predicate
  `{components: {"minecraft:custom_name": "<name as a quoted JSON string>"}}` (quote and backslash escaped); an empty or
  non-string old value is dropped (datafixer spec).

## Edge cases and known quirks

1. Form 2 ignores the components matcher; loot and location forms honour it.
2. `matchesBlockEntity` accepts a missing block entity when the predicate has neither `nbt` nor matchers; it rejects it
   (false) when either is present.
3. The block-entity NBT compared by `nbt` contains `x`, `y`, `z`, `id` and a `components` compound; data written by
   hand for `nbt:` therefore may include those keys.
4. A loaded-chunk gate applies to biome, structure and smokey, but **not** to `can_see_sky`.
5. `structures` uses piece bounding boxes, not the structure's overall bounding box.
6. The adventure-mode cache can return a stale `false` (see §5.3).
7. A location predicate position uses unrounded doubles for `position` bounds but floors for everything else.
8. An empty `blocks` list is a valid empty set: the predicate never matches any state.
9. The campfire smoke scan queries collision shapes with `pos` (the scan origin), not `pos.below(i)`, as the reference
   position (matters only for position-dependent collision shapes such as bamboo/pointed-dripstone offset blocks).

## Constants and IDs

| Name | Value |
|---|---|
| Component ids | `minecraft:can_place_on`, `minecraft:can_break`, `minecraft:lock` |
| Block-entity NBT keys present for `nbt` | `id`, `x`, `y`, `z`, `components` (+ the type's own keys) |
| Smokey scan depth | 5 blocks down (`i = 1…5`) |
| Light sentinel | 15 outside `±30,000,000` in X/Z |
| Valid world bounds for `W.loaded` | Y within the dimension's build height; chunk coordinates `|cx|,|cz| ≤ MAX_CHUNK_COORD` (= the section coordinate of `2^25 − 1` minus a safety margin; **[oracle]** exact value) |
| Campfire tag | `minecraft:campfires` (block tag) |
| `NO_LOCK` | empty item predicate |

## Open questions

1. **[oracle]** `MAX_CHUNK_COORD` exact integer (`ChunkPyramid.MAX_CHUNK_COORDINATE_VALUE`): formula is
   `((2^25 − 1) >> 4) − 2·(32 + n + 1)` where `n` is the number of accumulated dependencies of the `FULL` step; probe
   the constant directly.
2. **[oracle]** `canSeeSky` for an unloaded chunk (§4.2 step 10).
3. **[oracle]** Adventure-mode cache sharing semantics (§5.3).
4. **[oracle]** The "virtual post" shape of the campfire smoke check (§4.2 step 6): confirm the box is exactly
   `[6,10]×[0,16]×[6,10]` pixels (probe `CampfireBlock.SHAPE_VIRTUAL_POST`).
5. The structure reference lookup (§4.2 step 5) reads chunk structure references at the `STRUCTURE_REFERENCES` status of
   the block's chunk; confirm no case where the call generates/loads a chunk (the `loaded` gate should prevent it).

## Oracle probe points

`BlockPredicate#matches(ServerLevel, BlockPos)`, `#matches(BlockInWorld)`, `#matchesState`, `#matchesBlockEntity`,
`#willMatchBlockEntity`, `FluidPredicate#matches`, `LightPredicate#matches`, `LocationPredicate#matches`,
`CampfireBlock#isSmokeyPos`, `StructureManager#getStructureWithPieceAt(BlockPos, HolderSet)`,
`AdventureModePredicate#test`, `ItemStack#canPlaceOnBlockInAdventureMode`, `ItemStack#canBreakBlockInAdventureMode`,
`LockCode#unlocksWith`, `LockCode#canUnlock`, `MatchBlock#test`, `Level#isLoaded`.

## Parity status

`predicates`: not yet run.
