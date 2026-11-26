# Worldgen: tree feature (`minecraft:tree`) (26.3)

- Verified by suites: `golden-features`
- Sources: data-pack JSON schemas for `minecraft:tree` and the five tree sub-registries (`worldgen/trunk_placer_type`, `worldgen/foliage_placer_type`, `worldgen/root_placer_type`, `worldgen/tree_decorator_type`, `worldgen/feature_size_type`), public documentation of tree generation and sapling growth, and local reference reading (2026-11-25) for 26.3 exact schemas, placement order, RNG call sequences, leaf-distance propagation, and quirks. Everything below is written in our own words.
- Related specs: [`math/random.md`](../math/random.md) (RNG primitives), [`registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) (§3.3 block-state codec, §2.6 holders/holder sets, §4 tags), [`codec/datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md) (JSON/codec coercion and record rules), and [`worldgen/features.md`](./features.md) (feature dispatch, block state providers, block predicates, int providers, placement modifiers, and non-tree features such as `fallen_tree`, `huge_fungus`, and huge mushrooms).

---

## Summary

In 26.3, `minecraft:tree` is a self-contained feature record registered in `minecraft:worldgen/feature_type` (feature configuration fields are serialized directly on the feature object rather than under a nested `config` wrapper, per `worldgen/features.md`). A tree feature composes four pluggable geometry/decoration stages plus a minimum-clearance profile:

1. **Feature size** (`minimum_size`, 2 built-in types): defines the horizontal free-space radius required at each height offset during the pre-placement clearance scan and an optional minimum clipped height.
2. **Root placer** (`root_placer`, optional, 1 built-in type): shifts the trunk origin vertically and places underground/surface roots before the trunk is built.
3. **Trunk placer** (`trunk_placer`, 10 built-in types): selects total tree height, optionally replaces blocks beneath the trunk via `below_trunk_provider`, places log columns and branches, and emits an ordered list of **foliage attachment points**.
4. **Foliage placer** (`foliage_placer`, 12 built-in types): selects crown height and leaf radius, then generates leaf clusters around each foliage attachment point with waterlogging and persistent-leaf guards.
5. **Tree decorators** (`decorators`, 11 built-in types): post-process the placed root, log, and leaf position sets in configured order (vines, hanging pale moss, creaking hearts, cocoa pods, shelf mushrooms, bee nests with bee occupants, ground alteration, leaf/log attachments, and ground-scatter blocks).

After all four stages finish, `minecraft:tree` runs a bounded multi-source shortest-path pass to assign accurate `distance` (`1..7`) values to leaves within the tree's bounding box, followed by a boundary shape-update pass over the resulting filled voxel shape. In-game sapling, mangrove propagule, and azalea growth (`TreeGrower`) selects a configured `minecraft:tree` feature (including 2×2 mega-tree detection and nearby-flower detection) and invokes the exact same placement entry point.

---

## Inputs and state

### Execution inputs
A `minecraft:tree` invocation receives:
- `level`: a mutable world view (`WorldGenRegion` during chunk generation or `ServerLevel` during live sapling growth) providing block-state queries/writes, fluid-state queries, block-entity access, heightmaps (`MOTION_BLOCKING_NO_LEAVES`), vertical build bounds (`min_y` inclusive, `max_y` inclusive), and registry access.
- `chunk_generator`: the active chunk generator (forwarded only if a decorator invokes a sub-feature; note that `PaleMossDecorator` reads the generator from `level.getLevel()` rather than the feature argument).
- `random`: a `RandomSource` following [`math/random.md`](../math/random.md). All `nextInt(bound)`, `nextIntBetweenInclusive(min, max) = nextInt(max - min + 1) + min`, `nextFloat()`, and `nextBoolean()` draws are consumed sequentially in the exact order specified below.
- `origin`: integer block position `(x, y, z)` where the tree is anchored (the base of the trunk when no `root_placer` is present, or the root origin when one is present).

### Arithmetic and lookup conventions
- Integer division (`a / b`) is Java signed truncating division toward zero; remainder (`a % b`) preserves the dividend's sign.
- `floor(v)` maps `f32` or `f64` via IEEE-754 floor followed by Java saturating narrowing to `i32`.
- `Mth.sin(d)` and `Mth.cos(d)` use the game's 65,536-entry `f32` sine table (see **Constants and IDs**), **not** `f64` `Math.sin`/`Math.cos`. Where a placer uses JVM `Math.sin`/`Math.cos`/`Math.sqrt`/`Math.pow` instead, this spec states so explicitly.
- **Manhattan distance** `distManhattan(a, b)`: converts `|b.x - a.x|`, `|b.y - a.y|`, `|b.z - a.z|` to `f32`, sums them in `f32`, and truncates to `i32` (for all tree-scale offsets this is exact integer $|dx| + |dy| + |dz|$).
- **Direction orderings** (exact enumeration order matters for iteration and RNG selection):
  - `Direction.values()` (6 directions): `[DOWN, UP, NORTH, SOUTH, WEST, EAST]`.
  - `Direction.Plane.HORIZONTAL` (4 directions): `[NORTH, EAST, SOUTH, WEST]`.
  - `Direction.getClockWise()` on horizontal directions: `NORTH → EAST → SOUTH → WEST → NORTH`.
  - `Direction.Plane.HORIZONTAL.getRandomDirection(random)` draws `random.nextInt(4)` and indexes `[NORTH, EAST, SOUTH, WEST]`.
  - `Util.getRandom(list, random)` draws `random.nextInt(list.size())` and returns `list[index]`.
  - `Util.shuffle(list, random)` is a backward Fisher-Yates shuffle: for `i = list.size()` down to `2` inclusive, draw `k = random.nextInt(i)` and swap `list[i - 1]` with `list[k]`.
  - `Util.shuffledCopy(source, random)` copies `source` into a new list in iteration order and runs `Util.shuffle` on the copy.
  - `Direction.allShuffled(random)` calls `Util.shuffledCopy([DOWN, UP, NORTH, SOUTH, WEST, EAST], random)` — always consuming 5 draws: `nextInt(6), nextInt(5), nextInt(4), nextInt(3), nextInt(2)`.

### Shared helpers from `worldgen/features.md`
The following building blocks are specified in [`worldgen/features.md`](./features.md) and referenced here by name:
- **Block state providers** (`Holder<BlockStateProvider>`): registry reference (`"<ns>:<path>"` into `minecraft:worldgen/block_state_provider`) or inline provider (`BlockState` shorthand or `{"type": ...}`). Two sampling methods matter specifically for trees:
  - `getState(level, random, pos) -> BlockState`: always returns a non-null state (for `rule_based_state_provider` and `random_block_provider`, falls back to the current block at `pos` when no rule matches or the block set is empty).
  - `getOptionalState(level, random, pos) -> Option<BlockState>`: returns `None` when a `rule_based_state_provider` matches no rule and has no fallback (or its matched `then` / `fallback` returns `None`), or when a `random_block_provider` has an empty block set; all other providers delegate to `getState` and return `Some`.
- **Int providers** (`IntProvider`): bare integer shorthand or typed object `{"type": ...}`, with range validation `[min, max]` checking both `minInclusive() >= min` and `maxInclusive() <= max`.
- **Holder sets of blocks** (`HolderSet<Block>`): `"#<tag_id>"`, single block ID string, or JSON array of block IDs (see `identifiers-registries-tags.md` §2.6).

### Mutable state tracked during one tree placement
Each call to `TreeFeature::place` allocates four mutable hash sets of immutable `BlockPos`:
1. `root_positions`: every position passed to `root_setter`.
2. `trunks`: every position passed to `trunk_setter` (this **includes** blocks placed by `below_trunk_provider`!).
3. `foliage`: every position passed to `foliage_setter`.
4. `decorations`: every position passed to `decoration_setter`.

All four setters write to the world with block update flags **`19`** (`UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE` = `1 | 2 | 16`) and unconditionally insert the immutable `BlockPos` into their respective set — even if `level.setBlock` returns `false` (for example, outside a `WorldGenRegion` write zone).

---

## Behavior (ordered rules)

### 1. `minecraft:tree` configuration schema

Registered in `minecraft:worldgen/feature_type` as `minecraft:tree`. Notice that 26.3 replaced the older `dirt_provider` + `force_dirt` pair with a single required `below_trunk_provider` (typically a `rule_based_state_provider` reference such as `minecraft:soil_beneath_tree`, or a direct block state such as `minecraft:rooted_dirt`).

| JSON field | Type | Required | Default | Notes |
|---|---|---|---|---|
| `trunk_provider` | `Holder<BlockStateProvider>` | yes | — | Supplies log states for trunk and branches |
| `trunk_placer` | `TrunkPlacer` (dispatched by `type`) | yes | — | One of the 10 types in §4 |
| `foliage_provider` | `Holder<BlockStateProvider>` | yes | — | Supplies leaf states for crowns |
| `foliage_placer` | `FoliagePlacer` (dispatched by `type`) | yes | — | One of the 12 types in §5 |
| `root_placer` | `RootPlacer` (dispatched by `type`) | no | absent | Optional; only `minecraft:mangrove_root_placer` exists in 26.3 (§6) |
| `minimum_size` | `FeatureSize` (dispatched by `type`) | yes | — | One of the 2 types in §3 |
| `decorators` | array of `TreeDecorator` | yes | — | Ordered list of decorators from §7 (may be `[]`) |
| `ignore_vines` | `bool` | no | `false` | If `false`, `minecraft:vine` blocks fail the free-space clearance scan |
| `below_trunk_provider` | `Holder<BlockStateProvider>` | yes | — | Sampled via `getOptionalState` beneath trunk bases |

---

### 2. Core tree placement algorithm

#### 2.1 Shared position predicates and block placement helpers
- **`is_vine(level, pos)`**: true iff the block at `pos` is `minecraft:vine`.
- **`is_air_or_leaves(level, pos)`**: true iff the block state at `pos` has `isAir()` (`air`, `cave_air`, `void_air`) or belongs to `#minecraft:leaves`.
- **`valid_tree_pos(level, pos)`**: true iff the block state at `pos` has `isAir()` or belongs to `#minecraft:replaceable_by_trees`.
- **`TrunkPlacer::valid_tree_pos(level, pos)`**: defaults to `valid_tree_pos(level, pos)`. Overridden only by `upwards_branching_trunk_placer` to also accept blocks matching its `can_grow_through` holder set.
- **`TrunkPlacer::is_free(level, pos)`**: true iff `trunk_placer.valid_tree_pos(level, pos)` OR the block state at `pos` belongs to `#minecraft:logs`.
- **`place_below_trunk_block(level, trunk_setter, random, pos, tree)`**:
  1. Call `state = tree.below_trunk_provider.getOptionalState(level, random, pos)`.
  2. If `state` is `Some(s)`, invoke `trunk_setter(pos, s)` (inserts `pos` into `trunks` and sets block with flags `19`). If `None`, do nothing.
- **`TrunkPlacer::place_log(level, trunk_setter, random, pos, tree, modifier)`** (default `modifier = identity`):
  1. If `trunk_placer.valid_tree_pos(level, pos)` is false, return `false` without calling `trunk_provider`.
  2. Call `raw = tree.trunk_provider.getState(level, random, pos)`, apply `modifier(raw)`, invoke `trunk_setter(pos, modified)`, and return `true`.
- **`TrunkPlacer::place_log_if_free(level, trunk_setter, random, pos, tree)`**:
  1. If `trunk_placer.is_free(level, pos)`, call `place_log(level, trunk_setter, random, pos, tree, identity)`.
  *(Note: because `place_log` still checks `valid_tree_pos`, a pre-existing log that is not in `#minecraft:replaceable_by_trees` passes `is_free` and then returns `false` in `place_log` without consuming RNG or overwriting the block.)*
- **`FoliagePlacer::try_place_leaf(level, foliage_setter, random, tree, pos)`**:
  1. Check `is_persistent`: true iff the existing block state at `pos` has the boolean property `persistent` with value `true` (default `false` when the property is absent).
  2. If `is_persistent` is true OR `valid_tree_pos(level, pos)` is false, return `false` without calling `foliage_provider`.
  3. Call `leaf_state = tree.foliage_provider.getState(level, random, pos)`.
  4. If `leaf_state` has the boolean property `waterlogged`, set `waterlogged` to `level.getFluidState(pos).isSourceOfType(minecraft:water)` (**source water only**; flowing water yields `false`).
  5. Note: `distance` and `persistent` are **not** modified here; standard leaf blocks default to `distance=7, persistent=false` until the post-placement pass (§2.4).
  6. Invoke `foliage_setter.set(pos, leaf_state)` (inserts `pos` into `foliage` and sets block with flags `19`) and return `true`.

#### 2.2 Main placement sequence (`TreeFeature::place` and `doPlace`)
Execute the following steps in exact order:

1. **Allocate tracking sets**: `root_positions`, `trunks`, `foliage`, `decorations` (all empty `HashSet<BlockPos>`).
2. **Sample tree dimensions and trunk origin** (exact RNG order):
   1. `tree_height = trunk_placer.get_tree_height(random)` = `base_height + random.nextInt(height_rand_a + 1) + random.nextInt(height_rand_b + 1)`.
   2. `foliage_height = foliage_placer.foliage_height(random, tree_height, tree)`.
   3. `trunk_height = tree_height - foliage_height`.
   4. `leaf_radius = foliage_placer.foliage_radius(random, trunk_height)`.
   5. `trunk_origin`: if `root_placer` is present, call `root_placer.get_trunk_origin(origin, random)` = `origin.above(trunk_offset_y.sample(random))`; otherwise `origin`.
3. **Vertical build-height check**:
   - Let `min_y = min(origin.y, trunk_origin.y)` and `max_y = max(origin.y, trunk_origin.y) + tree_height + 1`.
   - Require `min_y >= level.getMinY() + 1` **and** `max_y <= level.getMaxY() + 1` (where `level.getMaxY()` is the inclusive top block Y, e.g. `319` in the Overworld, so `max_y <= 320`, leaving 1 block below `min_y` for the below-trunk block).
   - If violated, return `false` immediately.
4. **Free-space clearance scan (`get_max_free_tree_height`)**:
   - Loop `y` from `0` to `tree_height + 1` **inclusive**:
     - `r = minimum_size.get_size_at_height(tree_height, y)`.
     - Loop `x` from `-r` to `+r` inclusive:
       - Loop `z` from `-r` to `+r` inclusive:
         - Let `p = trunk_origin + (x, y, z)`.
         - If `!trunk_placer.is_free(level, p)` OR `(!ignore_vines && is_vine(level, p))`, stop and return `clipped_tree_height = y - 2`.
   - If the entire scan succeeds, `clipped_tree_height = tree_height`.
5. **Clipped-height acceptance**:
   - Require `clipped_tree_height >= tree_height` OR (`minimum_size.min_clipped_height` is present AND `clipped_tree_height >= minimum_size.min_clipped_height`).
   - Otherwise return `false`.
6. **Stage 1 — Roots** *(runs before trunk!)*:
   - If `root_placer` is present, call `ok = root_placer.place_roots(level, root_setter, random, origin, trunk_origin, tree)`.
   - If `!ok`, return `false`.
7. **Stage 2 — Trunk**:
   - Call `attachments = trunk_placer.place_trunk(level, trunk_setter, random, clipped_tree_height, trunk_origin, tree)`.
   - Note that `place_trunk` receives **`clipped_tree_height`**, whereas `foliage_height` and `leaf_radius` in step 2 were computed from the unclipped `tree_height`.
8. **Stage 3 — Foliage**:
   - For each `attachment` in `attachments` in exact list order:
     - Call `foliage_placer.create_foliage(level, foliage_setter, random, tree, clipped_tree_height, attachment, foliage_height, leaf_radius)`.
     - The base `create_foliage` wrapper always samples `offset = foliage_placer.offset.sample(random)` once per attachment before calling the subclass implementation.
9. **Non-empty placement guard**:
   - If `trunks.is_empty() && foliage.is_empty()`, return `false` (decorators, leaf-distance update, and edge shape updates are skipped even if `root_positions` is non-empty).
10. **Stage 4 — Decorators**:
    - If `!decorators.is_empty()`:
      - Build `TreeDecorator.Context` (§2.3) from `(level, decoration_setter, random, trunks, foliage, root_positions)`.
      - For each `decorator` in `decorators` in list order, invoke `decorator.place(context)`.
11. **Bounding box & post-placement passes**:
    - Compute the axis-aligned inclusive `BoundingBox` encapsulating all positions in `root_positions ∪ trunks ∪ foliage ∪ decorations`. (If empty, return `false`.)
    - Run `shape = update_leaves(level, bounds, trunks, decorations, root_positions)` (§2.4).
    - Run `StructureTemplate.updateShapeAtEdge(level, 3, shape, bounds.min_x, bounds.min_y, bounds.min_z)` (§2.5).
    - Return `true`.

#### 2.3 Decorator context construction and ordering
`TreeDecorator.Context` snapshots the three sets placed before decorators run (decorations added by earlier decorators are **not** added to `logs`, `leaves`, or `roots`, though they are visible via `level` queries):
1. Copy `root_positions` into list `roots`, `trunks` into list `logs`, and `foliage` into list `leaves` in `HashSet` iteration order.
2. Sort `logs`, `leaves`, and `roots` each by ascending `y` (`Comparator.comparingInt(Vec3i::getY)`) using a **stable** sort (Java TimSort).
   - **Critical parity detail [oracle]**: positions with equal `y` retain the iteration order of Java's `HashSet<BlockPos>` (see Edge Cases §1 and Open Questions).
3. Context helper predicates:
   - `is_air(pos)`: `state.isAir()`.
   - `is_replaceable(pos)`: `state.canBeReplaced()` (the block's `replaceable` property, e.g. air, water, lava, tall grass, snow layers, vines, dead bush, etc.).
   - `is_water_or_water_nearby(pos)`: true iff `pos`, `pos.east()`, `pos.west()`, `pos.north()`, or `pos.south()` has block `minecraft:water` (checks block identity `Blocks.WATER`, not waterlogged blocks or fluid tag).
   - `place_vine(pos, prop)`: `decoration_setter(pos, minecraft:vine[up=false,north=false,east=false,south=false,west=false, <prop>=true])`.
4. Shared lowest-base helper (`TreeFeature.getLowestTrunkOrRootOfTree(context)`):
   - If `context.roots` is empty: returns a copy of `context.logs`.
   - Else if `!context.logs.is_empty()` AND `context.roots[0].y == context.logs[0].y`: returns `context.logs` followed by `context.roots`.
   - Else: returns a copy of `context.roots`.
   *(Because `logs` and `roots` are sorted by ascending `y`, index 0 holds each list's minimum Y. Note that `logs` includes `below_trunk_provider` blocks placed at `origin.y - 1`!)*

#### 2.4 Leaf-distance propagation pass (`updateLeaves`)
Given `(level, bounds, logs, decorations, root_positions)`:
1. Allocate a 3D bitset voxel shape `shape` of dimensions `(bounds.x_span, bounds.y_span, bounds.z_span)`, all initially empty (`0`), where local coordinate `(lx, ly, lz) = (pos.x - bounds.min_x, pos.y - bounds.min_y, pos.z - bounds.min_z)`.
2. Allocate 7 distance buckets `to_check[0..6]`, each a `HashSet<BlockPos>`.
3. For every `pos` in `decorations ∪ root_positions`: if `bounds.is_inside(pos)`, mark `shape` filled at `pos - bounds.min`.
4. Insert all `pos` in `logs` into `to_check[0]`. Set `smallest_distance = 0`.
5. Loop:
   - While `smallest_distance < 7` and `to_check[smallest_distance]` is empty, increment `smallest_distance`.
   - If `smallest_distance >= 7`, stop and return `shape`.
   - Pop one `pos` from `to_check[smallest_distance]` and remove it from that bucket.
   - If `!bounds.is_inside(pos)`, continue.
   - If `smallest_distance != 0`:
     - Read `state = level.getBlockState(pos)`.
     - Write `state.setValue(distance, smallest_distance)` to `level` with update flags **`19`**.
   - Mark `shape` filled at `pos - bounds.min`.
   - For each `dir` in `Direction.values()` (`DOWN, UP, NORTH, SOUTH, WEST, EAST`):
     - `n = pos + dir`.
     - If `bounds.is_inside(n)` and `!shape.is_full(n - bounds.min)`:
       - Inspect `current_state = level.getBlockState(n)` via `LeavesBlock.getOptionalDistanceAt(current_state)`:
         - If `current_state` is in `#minecraft:prevents_nearby_leaf_decay` (logs, etc.): yields `Some(0)`.
         - Else if `current_state` has the `distance` integer property (`1..7`): yields `Some(current_state.distance)`.
         - Else: yields `None`.
       - If `Some(existing_dist)`:
         - `new_dist = min(existing_dist, smallest_distance + 1)`.
         - If `new_dist < 7`:
           - Add `n` to `to_check[new_dist]`.
           - Set `smallest_distance = min(smallest_distance, new_dist)`.

Key consequences of this algorithm:
- Only positions that are in `decorations`, `root_positions`, `logs`, or reached with distance `0..6` are marked filled in `shape`. Leaves whose final distance remains `7` (disconnected from all decay-preventing blocks within `bounds`) are left at `distance=7` and are **not** marked in `shape`.
- Any block inside `bounds` matching `#minecraft:prevents_nearby_leaf_decay` (including nearby pre-existing logs, or branch logs placed by `PoplarFoliagePlacer` into `foliage` instead of `trunks`) acts as a distance-`0` source if reached by BFS from `logs`, dropping `smallest_distance` back to `0` without attempting to set a `distance` property on it.
- Pre-existing persistent leaves inside `bounds` that are adjacent to the tree still have their `distance` property updated (their `persistent` property is unchanged).
- If a decorator (e.g., `CreakingHeartDecorator`, `BeehiveDecorator`) overwrote a log position in `logs`, that position is already filled in `shape` by step 3, yet it is still queued in `to_check[0]` by step 4 and still propagates distance `1` to adjacent leaves at `smallest_distance == 0`!

#### 2.5 Boundary shape-update pass (`StructureTemplate.updateShapeAtEdge`)
Called with `update_mode = 3` (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`), `shape`, and `(bounds.min_x, bounds.min_y, bounds.min_z)`:
1. Walk all boundary faces of `shape` via `DiscreteVoxelShape.forAllFaces` in three exact axis passes (`AxisCycle.NONE`, `AxisCycle.FORWARD`, `AxisCycle.BACKWARD`):
   - **Pass 1 (`NONE`, Z-axis faces `NORTH` / `SOUTH`)**: for `x = 0 ..< x_span`, `y = 0 ..< y_span`, scan `z = 0 ..= z_span` (treating `z == z_span` as empty):
     - Transition empty→full at `z`: emit face `(NORTH, x, y, z)`.
     - Transition full→empty at `z`: emit face `(SOUTH, x, y, z - 1)`.
   - **Pass 2 (`FORWARD`, Y-axis faces `DOWN` / `UP`)**: for `z = 0 ..< z_span`, `x = 0 ..< x_span`, scan `y = 0 ..= y_span`:
     - Transition empty→full at `y`: emit face `(DOWN, x, y, z)`.
     - Transition full→empty at `y`: emit face `(UP, x, y - 1, z)`.
   - **Pass 3 (`BACKWARD`, X-axis faces `WEST` / `EAST`)**: for `y = 0 ..< y_span`, `z = 0 ..< z_span`, scan `x = 0 ..= x_span`:
     - Transition empty→full at `x`: emit face `(WEST, x, y, z)`.
     - Transition full→empty at `x`: emit face `(EAST, x - 1, y, z)`.
2. For each emitted `(dir, lx, ly, lz)`:
   - `pos = (bounds.min_x + lx, bounds.min_y + ly, bounds.min_z + lz)`, `npos = pos + dir`.
   - Read `state = level.getBlockState(pos)` and `nstate = level.getBlockState(npos)`.
   - Compute `new_state = state.updateShape(level, level, pos, dir, npos, nstate, level.getRandom())`.
   - If `new_state != state`, call `level.setBlock(pos, new_state, update_mode & ~1)` (i.e. flags **`2`**, `UPDATE_CLIENTS` without `UPDATE_NEIGHBORS` and without `UPDATE_KNOWN_SHAPE`, so `WorldGenRegion` marks post-processing if needed).
   - Compute `new_nstate = nstate.updateShape(level, level, npos, dir.opposite(), pos, new_state, level.getRandom())`.
   - If `new_nstate != nstate`, call `level.setBlock(npos, new_nstate, 2)`.

Note that `updateShape` receives `level.getRandom()`, **not** the feature's `random` stream. For `LeavesBlock`, `updateShape` schedules a water tick if `waterlogged` and schedules a 1-tick leaf update whenever `getDistanceAt(neighbor) + 1 != 1 || state.distance != 1` (leaving `state` unchanged immediately).

---

### 3. Feature sizes (`minecraft:worldgen/feature_size_type`)

Dispatched by `"type"`. Both registered types share one optional field:
- `min_clipped_height`: optional integer in `[0, 80]`; default absent.

#### 3.1 `minecraft:two_layers_feature_size`
| Field | Type / range | Default |
|---|---|---|
| `limit` | `i32` in `[0, 81]` | `1` |
| `lower_size` | `i32` in `[0, 16]` | `0` |
| `upper_size` | `i32` in `[0, 16]` | `1` |
| `min_clipped_height` | optional `i32` in `[0, 80]` | absent |

- `get_size_at_height(tree_height, yo)`: returns `lower_size` if `yo < limit`, else `upper_size`.

#### 3.2 `minecraft:three_layers_feature_size`
| Field | Type / range | Default |
|---|---|---|
| `limit` | `i32` in `[0, 80]` | `1` |
| `upper_limit` | `i32` in `[0, 80]` | `1` |
| `lower_size` | `i32` in `[0, 16]` | `0` |
| `middle_size` | `i32` in `[0, 16]` | `1` |
| `upper_size` | `i32` in `[0, 16]` | `1` |
| `min_clipped_height` | optional `i32` in `[0, 80]` | absent |

- `get_size_at_height(tree_height, yo)`:
  1. If `yo < limit`: return `lower_size`.
  2. Else if `yo >= tree_height - upper_limit`: return `upper_size`.
  3. Else: return `middle_size`.

---

### 4. Trunk placers (`minecraft:worldgen/trunk_placer_type`)

Dispatched by `"type"`. All 10 registered trunk placers share three required base fields:

| Field | Type / range | Meaning |
|---|---|---|
| `base_height` | `i32` in `[0, 32]` | Base trunk height |
| `height_rand_a` | `i32` in `[0, 24]` | First uniform height addend bound |
| `height_rand_b` | `i32` in `[0, 24]` | Second uniform height addend bound |

- Shared `get_tree_height(random)`: `base_height + random.nextInt(height_rand_a + 1) + random.nextInt(height_rand_b + 1)` (always consumes **two** `nextInt` calls, even when `height_rand_a == 0` or `height_rand_b == 0`, calling `nextInt(1)`).
- Every `place_trunk` returns a `List<FoliageAttachment>`, where `FoliageAttachment` is the 5-tuple `(pos: BlockPos, radius_offset_xz: i32, foliage_height_offset: i32, size_x: i32, size_z: i32)`. Convenience constructor `(pos, radius_offset_xz, double_trunk: bool)` sets `foliage_height_offset = 0` and `size_x = size_z = (2 if double_trunk else 1)`; `double_trunk()` returns `size_x == 2 && size_z == 2`. All 10 built-in trunk placers emit `foliage_height_offset = 0` and symmetric `size_x == size_z ∈ {1, 2}`.
- Sideways log orientation uses `trySetValue(axis, dir.axis)` (no-op if the block state lacks the `axis` property).

#### 4.1 `minecraft:straight_trunk_placer`
- **Fields**: base fields only.
- **Placement (`tree_height`, `origin`)**:
  1. `place_below_trunk_block` at `origin.below()`.
  2. For `y = 0` to `tree_height - 1`: `place_log` at `origin.above(y)`.
  3. Return `[(origin.above(tree_height), radius_offset=0, double_trunk=false)]`.

#### 4.2 `minecraft:forking_trunk_placer`
- **Fields**: base fields only.
- **Placement (`tree_height`, `origin`)**:
  1. `place_below_trunk_block` at `origin.below()`.
  2. Draw `lean_dir = HORIZONTAL.getRandomDirection(random)`.
  3. Draw `lean_height = tree_height - random.nextInt(4) - 1`.
  4. Draw `lean_steps = 3 - random.nextInt(3)`.
  5. Set `tx = origin.x`, `tz = origin.z`, `ey = None`.
  6. For `yo = 0` to `tree_height - 1`:
     - `yy = origin.y + yo`.
     - If `yo >= lean_height && lean_steps > 0`: `tx += lean_dir.dx`, `tz += lean_dir.dz`, `lean_steps -= 1`.
     - If `place_log((tx, yy, tz))` succeeds: set `ey = Some(yy + 1)`.
  7. If `ey == Some(v)`: append attachment `((tx, v, tz), radius_offset=1, double_trunk=false)`.
  8. Reset `tx = origin.x`, `tz = origin.z`.
  9. Draw `branch_dir = HORIZONTAL.getRandomDirection(random)`.
  10. If `branch_dir != lean_dir`:
      - Draw `branch_pos = lean_height - random.nextInt(2) - 1`.
      - Draw `branch_steps = 1 + random.nextInt(3)`.
      - Set `ey = None`.
      - Loop with `yo = branch_pos` while `yo < tree_height && branch_steps > 0` (at end of each iteration: `yo += 1`, `branch_steps -= 1`):
        - If `yo >= 1`:
          - `yy = origin.y + yo`, `tx += branch_dir.dx`, `tz += branch_dir.dz`.
          - If `place_log((tx, yy, tz))` succeeds: set `ey = Some(yy + 1)`.
      - If `ey == Some(v)`: append attachment `((tx, v, tz), radius_offset=0, double_trunk=false)`.
  11. Return attachments. *(Note: no logs are rotated horizontally; all keep default state.)*

#### 4.3 `minecraft:giant_trunk_placer`
- **Fields**: base fields only.
- **Placement (`tree_height`, `origin`)**:
  1. Let `b = origin.below()`. Call `place_below_trunk_block` in exact order: `b`, `b.east()`, `b.south()`, `b.south().east()`.
  2. For `hh = 0` to `tree_height - 1`:
     - `place_log_if_free` at `origin + (0, hh, 0)`.
     - If `hh < tree_height - 1`:
       - `place_log_if_free` at `origin + (1, hh, 0)`.
       - `place_log_if_free` at `origin + (1, hh, 1)`.
       - `place_log_if_free` at `origin + (0, hh, 1)`.
     *(Notice the top layer `hh == tree_height - 1` places only the `(0, hh, 0)` log!)*
  3. Return `[(origin.above(tree_height), radius_offset=0, double_trunk=true)]`.

#### 4.4 `minecraft:mega_jungle_trunk_placer`
- **Fields**: base fields only.
- **Placement (`tree_height`, `origin`)**:
  1. Run `GiantTrunkPlacer::place_trunk` and seed `attachments` with its returned single attachment.
  2. Initialize `branch_height = tree_height - 2 - random.nextInt(4)`.
  3. While `branch_height > tree_height / 2` (signed integer division):
     - Draw `angle: f32 = random.nextFloat() * 6.2831855_f32` (`(float)(Math.PI * 2)` = `0x40C90FDB`).
     - Set `bx = 0`, `bz = 0`.
     - For `b = 0` to `4` inclusive:
       - `bx = trunc_i32(1.5_f32 + Mth.cos(f64(angle)) * f32(b))` (addition and multiplication in `f32`, then Java `(int)` truncation toward zero).
       - `bz = trunc_i32(1.5_f32 + Mth.sin(f64(angle)) * f32(b))`.
       - `place_log` at `origin + (bx, branch_height - 3 + b / 2, bz)`.
     - Append attachment `(origin + (bx, branch_height, bz), radius_offset=-2, double_trunk=false)`.
     - Update `branch_height -= 2 + random.nextInt(4)`.
  4. Return `attachments`.

#### 4.5 `minecraft:dark_oak_trunk_placer`
- **Fields**: base fields only.
- **Placement (`tree_height`, `origin`)**:
  1. Let `b = origin.below()`. Call `place_below_trunk_block` in exact order: `b`, `b.east()`, `b.south()`, `b.south().east()`.
  2. Draw `lean_dir = HORIZONTAL.getRandomDirection(random)`.
  3. Draw `lean_height = tree_height - random.nextInt(4)`.
  4. Draw `lean_steps = 2 - random.nextInt(3)`.
  5. Set `(x, y, z) = origin`, `tx = x`, `tz = z`, `ey = y + tree_height - 1`.
  6. For `dy = 0` to `tree_height - 1`:
     - If `dy >= lean_height && lean_steps > 0`: `tx += lean_dir.dx`, `tz += lean_dir.dz`, `lean_steps -= 1`.
     - Let `p = (tx, y + dy, tz)`.
     - If `is_air_or_leaves(level, p)` (checks **only** the `(0, 0)` corner, using `is_air_or_leaves`, not `valid_tree_pos`):
       - Call `place_log` in exact order: `p`, `p.east()`, `p.south()`, `p.east().south()`.
  7. Append main attachment `((tx, ey, tz), radius_offset=0, double_trunk=true)`.
  8. Side branches: loop `ox` from `-1` to `2` inclusive (outer), `oz` from `-1` to `2` inclusive (inner):
     - If `ox < 0 || ox > 1 || oz < 0 || oz > 1` (the 12 perimeter cells around the un-leaned base):
       - Draw `random.nextInt(3)`; if `<= 0` (i.e. `== 0`):
         - Draw `length = random.nextInt(3) + 2`.
         - For `branch_y = 0` to `length - 1`: `place_log` at `(x + ox, ey - branch_y - 1, z + oz)`.
         - Append attachment `((x + ox, ey, z + oz), radius_offset=0, double_trunk=false)`.
  9. Return `attachments`.

#### 4.6 `minecraft:fancy_trunk_placer`
- **Fields**: base fields only.
- **Constants**: `TRUNK_HEIGHT_SCALE = 0.618`, `CLUSTER_DENSITY_MAGIC = 1.382`, `BRANCH_SLOPE = 0.381`, `BRANCH_LENGTH_MAGIC = 0.328`, `ASSUMED_FOLIAGE_HEIGHT = 5`.
- **Helper `tree_shape(height, y) -> f32`**:
  1. If `f32(y) < f32(height) * 0.3_f32`: return `-1.0_f32`.
  2. `radius = f32(height) / 2.0_f32`, `adjacent = radius - f32(y)`.
  3. `distance = f32(Math.sqrt(f64(radius * radius - adjacent * adjacent)))`.
  4. If `adjacent == 0.0_f32`: `distance = radius`; else if `abs(adjacent) >= radius`: return `0.0_f32`.
  5. Return `distance * 0.5_f32`.
- **Helper `get_log_axis(start, pos)`**:
  - `xdiff = |pos.x - start.x|`, `zdiff = |pos.z - start.z|`, `maxdiff = max(xdiff, zdiff)`.
  - If `maxdiff > 0`: return `X` if `xdiff == maxdiff` else `Z`; otherwise return `Y`.
- **Helper `make_limb(start, end, do_place) -> bool`**:
  1. If `!do_place && start == end`: return `true`.
  2. `delta = end - start`, `steps = max(|delta.x|, max(|delta.y|, |delta.z|))`.
  3. `dx = f32(delta.x) / f32(steps)`, `dy = f32(delta.y) / f32(steps)`, `dz = f32(delta.z) / f32(steps)`. *(If `do_place` is true and `start == end`, `steps == 0`, producing `NaN` step deltas whose `floor(0.5 + NaN) = 0` places one log at `start`.)*
  4. For `i = 0` to `steps` **inclusive**:
     - `p = start + (floor(0.5_f32 + f32(i) * dx), floor(0.5_f32 + f32(i) * dy), floor(0.5_f32 + f32(i) * dz))`.
     - If `do_place`: `place_log` at `p` with modifier `trySetValue(axis, get_log_axis(start, p))`.
     - Else if `!is_free(level, p)`: return `false`.
  5. Return `true`.
- **Helper `trim_branches(height, local_y) -> bool`**: `f64(local_y) >= f64(height) * 0.2`.
- **Placement (`tree_height`, `origin`)**:
  1. `height = tree_height + 2`, `trunk_height = floor(f64(height) * 0.618)`.
  2. `place_below_trunk_block` at `origin.below()`.
  3. `clusters_per_y = min(1, floor(1.382 + Math.pow(f64(height) / 13.0, 2.0)))` — always evaluates to **`1`** for all real `height`.
  4. `trunk_top = origin.y + trunk_height`, `relative_y = height - 5`.
  5. Initialize list `foliage_coords = [(origin.above(relative_y), branch_base = trunk_top)]`.
  6. Loop `relative_y` downward while `relative_y >= 0`:
     - `shape = tree_shape(height, relative_y)`.
     - If `shape >= 0.0_f32`:
       - For `i = 0` to `clusters_per_y - 1`:
         - `radius: f64 = f64(shape) * (f64(random.nextFloat()) + 0.328)`.
         - `angle: f64 = f64(random.nextFloat() * 2.0_f32) * Math.PI` *(note `nextFloat() * 2.0_f32` in `f32`, widened to `f64` before multiplying by `Math.PI`)*.
         - `x = radius * Math.sin(angle) + 0.5`, `z = radius * Math.cos(angle) + 0.5` *(JVM `Math.sin`/`Math.cos`, **[oracle]**)*.
         - `check_start = origin + (floor(x), relative_y - 1, floor(z))`, `check_end = check_start.above(5)`.
         - If `make_limb(check_start, check_end, do_place=false)`:
           - `dx = origin.x - check_start.x`, `dz = origin.z - check_start.z`.
           - `branch_height = f64(check_start.y) - Math.sqrt(f64(dx * dx + dz * dz)) * 0.381`.
           - `branch_top = trunk_top` if `branch_height > f64(trunk_top)` else `trunc_i32(branch_height)` (Java `(int)` truncation toward zero).
           - `branch_base_pos = (origin.x, branch_top, origin.z)`.
           - If `make_limb(branch_base_pos, check_start, do_place=false)`:
             - Append `(check_start, branch_base = branch_top)` to `foliage_coords`.
  7. Place main trunk: `make_limb(origin, origin.above(trunk_height), do_place=true)` (places `trunk_height + 1` vertical logs from `origin.y` through `origin.y + trunk_height`).
  8. Place branches: for each `(end_pos, branch_base)` in `foliage_coords`:
     - Let `base_pos = (origin.x, branch_base, origin.z)`.
     - If `base_pos != end_pos && trim_branches(height, branch_base - origin.y)`:
       - `make_limb(base_pos, end_pos, do_place=true)`.
  9. Emit attachments: for each `(end_pos, branch_base)` in `foliage_coords`:
     - If `trim_branches(height, branch_base - origin.y)`:
       - Append `(end_pos, radius_offset=0, double_trunk=false)`.
  10. Return attachments.

#### 4.7 `minecraft:bending_trunk_placer`
- **Extra fields**:
  - `min_height_for_leaves`: optional positive `i32` (`>= 1`), default `1`.
  - `bend_length`: required `IntProvider` in `[1, 64]`.
- **Placement (`tree_height`, `origin`)**:
  1. Draw `direction = HORIZONTAL.getRandomDirection(random)` *(before `place_below_trunk_block`!)*.
  2. Set `log_height = tree_height - 1`, `pos = origin` (mutable).
  3. `place_below_trunk_block` at `origin.below()`.
  4. Vertical segment: for `i = 0` to `log_height` **inclusive**:
     - Draw `r = random.nextInt(2)` (unconditionally on every iteration).
     - If `i + 1 >= log_height + r`: move `pos` by `direction`.
     - If `valid_tree_pos(level, pos)`: `place_log` at `pos` (no axis rotation).
     - If `i >= min_height_for_leaves`: append `(pos.immutable(), radius_offset=0, double_trunk=false)`.
     - Move `pos` by `UP`.
  5. Sample `dir_length = bend_length.sample(random)`.
  6. Horizontal segment: for `i = 0` to `dir_length` **inclusive** (`dir_length + 1` blocks):
     - If `valid_tree_pos(level, pos)`: `place_log` at `pos` (no axis rotation).
     - Append `(pos.immutable(), radius_offset=0, double_trunk=false)`.
     - Move `pos` by `direction`.
  7. Return attachments.

#### 4.8 `minecraft:upwards_branching_trunk_placer`
- **Extra fields**:
  - `extra_branch_steps`: required positive `IntProvider` (`>= 1`).
  - `place_branch_per_log_probability`: required `f32` in `[0.0, 1.0]`.
  - `extra_branch_length`: required non-negative `IntProvider` (`>= 0`).
  - `can_grow_through`: required `HolderSet<Block>`.
- **Overrides `valid_tree_pos(level, pos)`**: `TreeFeature.valid_tree_pos(level, pos) || state.is(can_grow_through)`. (This also extends `is_free`.)
- **Placement (`tree_height`, `origin`)**:
  - Does **not** call `place_below_trunk_block`.
  - Loop `height_pos = 0` to `tree_height - 1`:
    1. `current_height = origin.y + height_pos`.
    2. Call `placed = place_log((origin.x, current_height, origin.z))`.
    3. If `placed && height_pos < tree_height - 1 && random.nextFloat() < place_branch_per_log_probability`:
       - `branch_dir = HORIZONTAL.getRandomDirection(random)`.
       - `branch_len = extra_branch_length.sample(random)`.
       - `branch_pos = max(0, branch_len - extra_branch_length.sample(random) - 1)` *(samples `extra_branch_length` twice)*.
       - `branch_steps = extra_branch_steps.sample(random)`.
       - Run `place_branch`:
         - `height_along_branch = current_height + branch_pos`, `log_x = origin.x`, `log_z = origin.z`, `idx = branch_pos`.
         - While `idx < tree_height && branch_steps > 0`:
           - If `idx >= 1`:
             - `placement_y = current_height + idx`.
             - `log_x += branch_dir.dx`, `log_z += branch_dir.dz`.
             - `height_along_branch = placement_y`.
             - If `place_log((log_x, placement_y, log_z))` succeeds: `height_along_branch += 1`.
             - Unconditionally append `((log_x, placement_y, log_z), radius_offset=0, double_trunk=false)`.
           - `idx += 1`, `branch_steps -= 1`.
         - If `height_along_branch - current_height > 1`:
           - Let `fp = (log_x, height_along_branch, log_z)`.
           - Append `(fp, 0, false)` and `(fp.below(2), 0, false)`.
    4. If `height_pos == tree_height - 1`:
       - Append `((origin.x, current_height + 1, origin.z), radius_offset=0, double_trunk=false)`.
  - Return attachments.

#### 4.9 `minecraft:cherry_trunk_placer`
- **Extra fields**:
  - `branch_count`: required `IntProvider` in `[1, 3]`.
  - `branch_horizontal_length`: required `IntProvider` in `[2, 16]`.
  - `branch_start_offset_from_top`: required **bare `UniformInt` object** (`{"min_inclusive": a, "max_inclusive": b}`, no `"type"` key) with `-16 <= a`, `b <= 0`, and `b - a >= 1` (at least 2 values of variation). At construction, derives `second_branch_start_offset_from_top = UniformInt(a, b - 1)`.
  - `branch_end_offset_from_top`: required `IntProvider` in `[-16, 16]`.
- **Placement (`tree_height`, `origin`)**:
  1. `place_below_trunk_block` at `origin.below()`.
  2. `first_off = max(0, tree_height - 1 + branch_start_offset_from_top.sample(random))`.
  3. `second_off = max(0, tree_height - 1 + second_branch_start_offset_from_top.sample(random))`.
  4. If `second_off >= first_off`: `second_off += 1`.
  5. `count = branch_count.sample(random)`.
  6. `has_middle = (count == 3)`, `has_both_sides = (count >= 2)`.
  7. `trunk_height`: `tree_height` if `has_middle`, else `max(first_off, second_off) + 1` if `has_both_sides`, else `first_off + 1`.
  8. For `y = 0` to `trunk_height - 1`: `place_log` at `origin.above(y)`.
  9. If `has_middle`: append `(origin.above(trunk_height), 0, false)`.
  10. Draw `tree_dir = HORIZONTAL.getRandomDirection(random)`. Let `sideways` be `trySetValue(axis, tree_dir.axis)`.
  11. Append `generate_branch(tree_dir, first_off, first_off < trunk_height - 1)`.
  12. If `has_both_sides`: append `generate_branch(tree_dir.opposite(), second_off, second_off < trunk_height - 1)`.
  13. Return attachments.
- **`generate_branch(branch_dir, offset_from_origin, middle_continues_upwards)`**:
  1. `log_pos = origin.above(offset_from_origin)`.
  2. `end_off = tree_height - 1 + branch_end_offset_from_top.sample(random)`.
  3. `extend = middle_continues_upwards || end_off < offset_from_origin`.
  4. `dist = branch_horizontal_length.sample(random) + (1 if extend else 0)`.
  5. `branch_end = origin.relative(branch_dir, dist).above(end_off)`.
  6. `horiz_steps = 2 if extend else 1`.
  7. Repeat `horiz_steps` times: move `log_pos` by `branch_dir`, call `place_log` with `sideways`.
  8. `vert_dir = UP if branch_end.y > log_pos.y else DOWN`.
  9. Loop:
     - `manhattan = distManhattan(log_pos, branch_end)`.
     - If `manhattan == 0`: return `(branch_end.above(), 0, false)`.
     - `vert_chance = f32(|branch_end.y - log_pos.y|) / f32(manhattan)`.
     - `grow_vert = random.nextFloat() < vert_chance`.
     - Move `log_pos` by `vert_dir` if `grow_vert` else `branch_dir`.
     - Call `place_log` at `log_pos` with `identity` if `grow_vert` else `sideways`.

#### 4.10 `minecraft:poplar_trunk_placer`
- **Extra fields**:
  - `trunk_height_above_branches`: required `IntProvider` in `[0, 8]`.
  - `branch_amount`: required `IntProvider` in `[1, 4]`.
- **Placement (`tree_height`, `origin`)**:
  1. `place_below_trunk_block` at `origin.below()`.
  2. `branch_top_y = tree_height - trunk_height_above_branches.sample(random)`.
  3. For `y = 0` to `tree_height - 1`:
     - `place_log` at `origin.above(y)`.
     - **Unconditionally on every `y` iteration**, call `Direction.allShuffled(random)` (5 `nextInt` draws) and filter out vertical directions (`UP`, `DOWN`), preserving the shuffled order of the 4 horizontal directions in `dirs`.
     - If `branch_top_y - 1 == y`:
       - Sample `branches = branch_amount.sample(random)`.
       - For `k = 0` to `branches - 1`:
         - `d = dirs[k]`.
         - `place_log` at `origin.above(y).relative(d, 1)` with `trySetValue(axis, d.axis)`.
  4. Return `[(origin.above(branch_top_y), radius_offset=0, double_trunk=false)]`.

---

### 5. Foliage placers (`minecraft:worldgen/foliage_placer_type`)

Dispatched by `"type"`. All 12 registered foliage placers share two required base fields:

| Field | Type / range | Meaning |
|---|---|---|
| `radius` | `IntProvider` in `[0, 16]` | Base leaf radius provider |
| `offset` | `IntProvider` in `[0, 16]` | Vertical crown offset provider (sampled once per attachment) |

#### Shared foliage helpers
- **Default `foliage_radius(random, trunk_height)`**: `radius.sample(random)` (overridden only by `pine_foliage_placer`).
- **Outer `create_foliage` wrapper**: always computes `offset = this.offset.sample(random)` first, then delegates to the subtype's placement routine (even if the subtype ignores `offset`, as `random_spread_foliage_placer` does).
- **`should_skip_location_signed(random, dx, y, dz, current_radius, double_trunk)`**:
  - If `double_trunk`: `ax = min(|dx|, |dx - 1|)`, `az = min(|dz|, |dz - 1|)`.
  - Else: `ax = |dx|`, `az = |dz|`.
  - Delegates to `should_skip_location(random, ax, y, az, current_radius, double_trunk)`.
- **`place_leaves_row(origin, current_radius, y, double_trunk)`**:
  - Let `ext = 1 if double_trunk else 0`.
  - Loop `dx = -current_radius` to `current_radius + ext` inclusive (outer), `dz = -current_radius` to `current_radius + ext` inclusive (inner):
    - If `!should_skip_location_signed(random, dx, y, dz, current_radius, double_trunk)`:
      - `try_place_leaf(level, foliage_setter, random, tree, origin + (dx, y, dz))`.
  *(If `current_radius + ext < -current_radius`, the loop runs zero times.)*
- **`place_leaves_row_with_hanging_leaves_below(origin, current_radius, y, double_trunk, hang_chance, ext_chance)`**:
  1. Call `place_leaves_row(origin, current_radius, y, double_trunk)`.
  2. Let `ext = 1 if double_trunk else 0`, `log_pos = origin.below()`.
  3. For each `along_edge` in `[NORTH, EAST, SOUTH, WEST]`:
     - `to_edge = along_edge.getClockWise()`.
     - `off_to_edge = (current_radius + ext)` if `to_edge` has positive axis direction (`EAST`, `SOUTH`) else `current_radius`.
     - Start `pos = origin + (0, y - 1, 0) + to_edge * off_to_edge + along_edge * (-current_radius)`.
     - Loop `i = -current_radius` while `i < current_radius + ext` (strict `<`):
       - Check `leaves_above = foliage_setter.is_set(pos.above())` (tests the `foliage` set of this tree).
       - If `leaves_above` AND `try_place_extension(hang_chance, log_pos, pos)`:
         - Call `try_place_extension(ext_chance, log_pos, pos.below())`.
       - Advance `pos` by `along_edge` and increment `i`.
  - **`try_place_extension(chance, log_pos, pos)`**:
    - If `distManhattan(pos, log_pos) >= 7`: return `false` (no RNG draw).
    - If `random.nextFloat() > chance`: return `false` (strict `>`; `<=` succeeds).
    - Return `try_place_leaf(level, foliage_setter, random, tree, pos)`.

In the subtype rules below, let `H = foliage_height + attachment.foliage_height_offset`, `R_off = attachment.radius_offset_xz`, and `dbl = attachment.double_trunk()`.

#### 5.1 `minecraft:blob_foliage_placer`
- **Extra field**: `height`: required `i32` in `[0, 16]`.
- **`foliage_height`**: returns `height` (no RNG).
- **`create_foliage`**: for `yo = offset` down to `offset - H` inclusive:
  - `r = max(leaf_radius + R_off - 1 - yo / 2, 0)` (Java signed truncating division `yo / 2`).
  - `place_leaves_row(attachment.pos, r, yo, dbl)`.
- **`should_skip_location`**: `dx == r && dz == r && (random.nextInt(2) == 0 || y == 0)`.
  - **RNG quirk**: when `dx == r && dz == r`, `random.nextInt(2)` is evaluated **before** checking `y == 0`, so corners on layer `y == 0` still consume one `nextInt(2)` draw.

#### 5.2 `minecraft:spruce_foliage_placer`
- **Extra field**: `trunk_height`: required `IntProvider` in `[0, 24]`.
- **`foliage_height`**: `max(4, tree_height - trunk_height.sample(random))`.
- **`create_foliage`**:
  1. `cur_r = random.nextInt(2)`, `max_r = 1`, `min_r = 0`.
  2. For `yo = offset` down to `-H` inclusive *(lower bound is `-H`, NOT `offset - H`!)*:
     - `place_leaves_row(attachment.pos, cur_r, yo, dbl)`.
     - If `cur_r >= max_r`: `cur_r = min_r`, `min_r = 1`, `max_r = min(max_r + 1, leaf_radius + R_off)`.
     - Else: `cur_r += 1`.
- **`should_skip_location`**: `dx == r && dz == r && r > 0`.

#### 5.3 `minecraft:pine_foliage_placer`
- **Extra field**: `height`: required `IntProvider` in `[0, 24]`.
- **`foliage_height`**: `height.sample(random)`.
- **`foliage_radius(random, trunk_height)`**: `radius.sample(random) + random.nextInt(max(trunk_height + 1, 1))`.
- **`create_foliage`**:
  1. `cur_r = 0`.
  2. For `yo = offset` down to `offset - H` inclusive:
     - `place_leaves_row(attachment.pos, cur_r, yo, dbl)`.
     - If `cur_r >= 1 && yo == offset - H + 1`: `cur_r -= 1`.
     - Else if `cur_r < leaf_radius + R_off`: `cur_r += 1`.
- **`should_skip_location`**: `dx == r && dz == r && r > 0`.

#### 5.4 `minecraft:acacia_foliage_placer`
- **Extra fields**: none.
- **`foliage_height`**: returns `0` (no RNG).
- **`create_foliage`**: let `p = attachment.pos.above(offset)`. Place three rows in order:
  1. `place_leaves_row(p, leaf_radius + R_off, -1 - H, dbl)`.
  2. `place_leaves_row(p, leaf_radius - 1, -H, dbl)` *(does not add `R_off`)*.
  3. `place_leaves_row(p, leaf_radius + R_off - 1, 0, dbl)`.
- **`should_skip_location`**:
  - If `y == 0`: `(dx > 1 || dz > 1) && dx != 0 && dz != 0`.
  - Else: `dx == r && dz == r && r > 0`.

#### 5.5 `minecraft:bush_foliage_placer`
- **Extra field**: `height`: required `i32` in `[0, 16]`.
- **`foliage_height`**: returns `height` (no RNG).
- **`create_foliage`**: for `yo = offset` down to `offset - H` inclusive:
  - `r = leaf_radius + R_off - 1 - yo` *(unclamped; if negative, `place_leaves_row` does nothing)*.
  - `place_leaves_row(attachment.pos, r, yo, dbl)`.
- **`should_skip_location`**: `dx == r && dz == r && random.nextInt(2) == 0`.

#### 5.6 `minecraft:fancy_foliage_placer`
- **Extra field**: `height`: required `i32` in `[0, 16]`.
- **`foliage_height`**: returns `height` (no RNG).
- **`create_foliage`**: for `yo = offset` down to `offset - foliage_height` inclusive *(ignores `foliage_height_offset` and `R_off`)*:
  - `r = leaf_radius + (1 if yo != offset && yo != offset - foliage_height else 0)`.
  - `place_leaves_row(attachment.pos, r, yo, dbl)`.
- **`should_skip_location`**: `(f32(dx) + 0.5_f32)^2 + (f32(dz) + 0.5_f32)^2 > f32(r * r)` (evaluated in `f32`).

#### 5.7 `minecraft:jungle_foliage_placer` (`MegaJungleFoliagePlacer`)
*(Note the registry path is `jungle_foliage_placer`, not `mega_jungle_foliage_placer`.)*
- **Extra field**: `height`: required `i32` in `[0, 16]`.
- **`foliage_height`**: returns `height` (no RNG).
- **`create_foliage`**:
  1. `leaf_h = (foliage_height if dbl else 1 + random.nextInt(2)) + attachment.foliage_height_offset`.
  2. For `yo = offset` down to `offset - leaf_h` inclusive:
     - `r = leaf_radius + R_off + 1 - yo`.
     - `place_leaves_row(attachment.pos, r, yo, dbl)`.
- **`should_skip_location`**: `dx + dz >= 7 || dx * dx + dz * dz > r * r`.

#### 5.8 `minecraft:mega_pine_foliage_placer`
- **Extra field**: `crown_height`: required `IntProvider` in `[0, 24]`.
- **`foliage_height`**: `crown_height.sample(random)`.
- **`create_foliage`**:
  1. `prev_r = 0`, `fp = attachment.pos`.
  2. Loop **upward** `yy = fp.y - H + offset` to `fp.y + offset` inclusive:
     - `yo = fp.y - yy`.
     - `smooth_r = leaf_radius + R_off + floor((f32(yo) / f32(H)) * 3.5_f32)` *(in `f32`, left-to-right)*.
     - `jagged_r = smooth_r + 1` if `yo > 0 && smooth_r == prev_r && (yy & 1) == 0` else `smooth_r`.
     - `place_leaves_row((fp.x, yy, fp.z), jagged_r, 0, dbl)`.
     - `prev_r = smooth_r`.
- **`should_skip_location`**: `dx + dz >= 7 || dx * dx + dz * dz > r * r`.

#### 5.9 `minecraft:dark_oak_foliage_placer`
- **Extra fields**: none.
- **`foliage_height`**: returns `4` (no RNG).
- **`create_foliage`**: let `p = attachment.pos.above(offset)` *(ignores `R_off` and `foliage_height_offset`)*.
  - If `dbl`:
    1. `place_leaves_row(p, leaf_radius + 2, -1, true)`.
    2. `place_leaves_row(p, leaf_radius + 3, 0, true)`.
    3. `place_leaves_row(p, leaf_radius + 2, 1, true)`.
    4. If `random.nextBoolean()`: `place_leaves_row(p, leaf_radius, 2, true)`.
  - Else:
    1. `place_leaves_row(p, leaf_radius + 2, -1, false)`.
    2. `place_leaves_row(p, leaf_radius + 1, 0, false)`.
- **`should_skip_location_signed` override**:
  - If `y == 0 && dbl && (dx == -r || dx >= r) && (dz == -r || dz >= r)`: return `true`. *(On `dx, dz ∈ [-r, r + 1]`, skips `dx ∈ {-r, r, r + 1}` crossed with `dz ∈ {-r, r, r + 1}`.)*
  - Otherwise delegate to default `should_skip_location_signed`.
- **`should_skip_location`**:
  - If `y == -1 && !dbl`: `dx == r && dz == r`.
  - Else if `y == 1`: `dx + dz > r * 2 - 2`.
  - Else: `false`.

#### 5.10 `minecraft:random_spread_foliage_placer`
- **Extra fields**:
  - `foliage_height`: required `IntProvider` in `[1, 512]`.
  - `leaf_placement_attempts`: required `i32` in `[0, 256]`.
- **`foliage_height`**: `foliage_height.sample(random)`.
- **`create_foliage`**: *(ignores `offset`, `R_off`, `foliage_height_offset`, `dbl`; note `offset.sample(random)` was still consumed by the outer wrapper)*:
  - Let `p = attachment.pos`.
  - Repeat `leaf_placement_attempts` times:
    - `dx = random.nextInt(leaf_radius) - random.nextInt(leaf_radius)`.
    - `dy = random.nextInt(foliage_height) - random.nextInt(foliage_height)`.
    - `dz = random.nextInt(leaf_radius) - random.nextInt(leaf_radius)`.
    - `try_place_leaf(level, foliage_setter, random, tree, p + (dx, dy, dz))`.

#### 5.11 `minecraft:cherry_foliage_placer`
- **Extra fields**:
  - `height`: required `IntProvider` in `[4, 16]`.
  - `wide_bottom_layer_hole_chance`: required `f32` in `[0.0, 1.0]`.
  - `corner_hole_chance`: required `f32` in `[0.0, 1.0]`. *(Codec encode quirk: re-encoding serializes `wide_bottom_layer_hole_chance` into the `corner_hole_chance` field; decoding reads both fields accurately.)*
  - `hanging_leaves_chance`: required `f32` in `[0.0, 1.0]`.
  - `hanging_leaves_extension_chance`: required `f32` in `[0.0, 1.0]`.
- **`foliage_height`**: `height.sample(random)`.
- **`create_foliage`**:
  - Let `p = attachment.pos.above(offset)`, `r = leaf_radius + R_off - 1`.
  1. `place_leaves_row(p, r - 2, H - 3, dbl)`.
  2. `place_leaves_row(p, r - 1, H - 4, dbl)`.
  3. For `y = H - 5` down to `0` inclusive: `place_leaves_row(p, r, y, dbl)`.
  4. `place_leaves_row_with_hanging_leaves_below(p, r, -1, dbl, hanging_leaves_chance, hanging_leaves_extension_chance)`.
  5. `place_leaves_row_with_hanging_leaves_below(p, r - 1, -2, dbl, hanging_leaves_chance, hanging_leaves_extension_chance)`.
- **`should_skip_location`**:
  1. If `y == -1 && (dx == r || dz == r) && random.nextFloat() < wide_bottom_layer_hole_chance`: return `true`.
  2. Let `corner = (dx == r && dz == r)`.
  3. If `r > 2`: return `corner || (dx + dz > r * 2 - 2 && random.nextFloat() < corner_hole_chance)`.
  4. Else: return `corner && random.nextFloat() < corner_hole_chance`.

#### 5.12 `minecraft:poplar_foliage_placer`
- **Extra fields**:
  - `height`: required `IntProvider` in `[5, 16]`.
  - `side_hole_chance`: required `f32` in `[0.0, 1.0]`.
- **`foliage_height`**: `height.sample(random)`.
- **Rhombus helpers** for row `(r, y, H, flip)`:
  - `partial = (y == H - 1 || y == H - 2)`.
  - `is_small_corner(dx, dz, flip)`:
    - If `flip`: `(dx > 0 && dz > 0) || (dz < 0 && dx < 0)`.
    - Else: `(dx > 0 && dz < 0) || (dz > 0 && dx < 0)`.
  - `corner_cut(dx, dz, r, partial, flip)`: `r - 1` if `is_small_corner`, else (`r + 1` if `partial` else `r`).
  - `within_rhombus(r, abs_dx, abs_dz, cut, extra)`: `abs_dx + abs_dz <= r * 2 - (cut + extra)`.
- **Custom row placement** `(origin, r, y, dbl, H, flip)`:
  - Let `ext = 1 if dbl else 0`. Loop `dx = -r ..= r + ext` (outer), `dz = -r ..= r + ext` (inner) — does **not** apply `should_skip_location_signed`'s double-trunk shift:
    - `partial = (y == H - 1 || y == H - 2)`, `cut = corner_cut(dx, dz, r, partial, flip)`.
    - `abs_dx = |dx|`, `abs_dz = |dz|`.
    - If `partial && (abs_dx == r || abs_dz == r)`: skip (**no RNG draw**).
    - Otherwise draw `extra = 1 if random.nextFloat() <= side_hole_chance else 0` *(inclusive `<=`)*.
    - If `within_rhombus(r, abs_dx, abs_dz, cut, extra)`: `try_place_leaf(level, foliage_setter, random, tree, origin + (dx, y, dz))`.
- **Internal branch log replacement** `replace_leaves_with_log(origin, r, y, dbl, H, flip)`:
  - Let `ext = 1 if dbl else 0`. Loop `dx = -r ..= r + ext` (outer), `dz = -r ..= r + ext` (inner):
    - `abs_dz = |dz|`, `abs_dx = |dx|`, `cut = corner_cut(dx, dz, r, partial, flip)`.
    - If `within_rhombus(r, abs_dx, abs_dz, cut, 2)` AND `((abs_dz == 0 && r - abs_dx >= 4) || (abs_dx == 0 && r - abs_dz >= 4))`:
      - Let `pos = origin + (dx, y, dz)`, `axis = X if abs_dz == 0 else Z`.
      - Sample `expected_leaf = tree.foliage_provider.getState(level, random, pos)` and test exact equality against `level.getBlockState(pos)`. *(Consumes `foliage_provider` RNG even when the block does not match!)*
      - If equal, call `foliage_setter.set(pos, tree.trunk_provider.getState(level, random, pos).trySetValue(axis, axis))`. *(Placed via `foliage_setter`, so it is recorded in `foliage`, not `trunks`!)*
- **`create_foliage`**:
  - Let `p = attachment.pos.above(offset)`, `r = leaf_radius + R_off - 1`.
  - Draw `flip = random.nextBoolean()`.
  1. Custom row `(p, r - 2, H - 1, dbl, H, flip)`.
  2. Custom row `(p, r - 1, H - 2, dbl, H, flip)`.
  3. Custom row `(p, r - 1, H - 3, dbl, H, flip)`.
  4. For `y = H - 4` down to `1` inclusive: custom row `(p, r, y, dbl, H, flip)`.
  5. `replace_leaves_with_log(p, r, H - 4, dbl, H, flip)`.
  6. Custom row `(p, r - 1, 0, dbl, H, flip)`.
  7. Custom row `(p, clamp(r - 2, 1, 2), -1, dbl, H, flip)`.

---

### 6. Root placers (`minecraft:worldgen/root_placer_type`)

Dispatched by `"type"`. Only one type is registered in 26.3 (`minecraft:mangrove_root_placer`).

#### Shared `RootPlacer` fields and helpers
| Field | Type | Required | Notes |
|---|---|---|---|
| `trunk_offset_y` | `IntProvider` (unbounded) | yes | Added to `origin.y` to obtain `trunk_origin` |
| `root_provider` | `Holder<BlockStateProvider>` | yes | Supplies default root block state |
| `above_root_placement` | `AboveRootPlacement` object | no | Optional block placed above each normal root |

`AboveRootPlacement` object fields:
- `above_root_provider`: required `Holder<BlockStateProvider>`.
- `above_root_placement_chance`: required `f32` in `[0.0, 1.0]`.

Shared helpers:
- **`get_potentially_waterlogged_state(level, pos, state)`**: if `state` has property `waterlogged`, sets it to `level.getFluidState(pos).is(#minecraft:water)` (note: matches the **`#minecraft:water` fluid tag** — both source and flowing water — unlike `FoliagePlacer.try_place_leaf` which checks source water only).
- **Base `can_place_root(level, pos)`**: `TreeFeature.valid_tree_pos(level, pos)`.
- **Base `place_root(level, root_setter, random, pos, tree)`**:
  1. If `!can_place_root(level, pos)`: return.
  2. Sample `s = root_provider.getState(level, random, pos)` and call `root_setter(pos, get_potentially_waterlogged_state(level, pos, s))`.
  3. If `above_root_placement` is present:
     - Let `above = pos.above()`.
     - Draw `random.nextFloat()`; if `< above_root_placement_chance` AND `level.getBlockState(above).isAir()`:
       - Sample `a = above_root_provider.getState(level, random, above)` and call `root_setter(above, get_potentially_waterlogged_state(level, above, a))`.
     *(Note short-circuit order: `nextFloat()` is drawn **before** testing `isAir(above)`!)*

#### 6.1 `minecraft:mangrove_root_placer`
- **Extra field**: `mangrove_root_placement` (required object):
  | Field | Type / range |
  |---|---|
  | `can_grow_through` | `HolderSet<Block>` |
  | `muddy_roots_in` | `HolderSet<Block>` |
  | `muddy_roots_provider` | `Holder<BlockStateProvider>` |
  | `max_root_width` | `i32` in `[1, 12]` |
  | `max_root_length` | `i32` in `[1, 64]` |
  | `random_skew_chance` | `f32` in `[0.0, 1.0]` |
- **Overrides `can_place_root(level, pos)`**: `TreeFeature.valid_tree_pos(level, pos) || state.is(can_grow_through)`.
- **Overrides `place_root(level, root_setter, random, pos, tree)`**:
  - If `level.getBlockState(pos).is(muddy_roots_in)`:
    - Sample `s = muddy_roots_provider.getState(level, random, pos)` and call `root_setter(pos, get_potentially_waterlogged_state(level, pos, s))`. *(Skips `can_place_root` re-check and skips `above_root_placement` — no RNG draw for above-root!)*
  - Else: call base `place_root`.
- **`place_roots(level, root_setter, random, origin, trunk_origin, tree) -> bool`**:
  1. **Column clearance check**: walk `p` from `origin` upward while `p.y < trunk_origin.y`. If `!can_place_root(level, p)`, return `false` immediately.
  2. Initialize `root_positions = [trunk_origin.below()]`.
  3. **Simulate 4 directional root arms**: for each `dir` in `[NORTH, EAST, SOUTH, WEST]`:
     - Let `start = trunk_origin.relative(dir)`, `arm = []`.
     - If `!simulate_roots(level, random, start, dir, trunk_origin, arm, layer=0)`: return `false` immediately (no blocks have been placed yet).
     - Append all entries of `arm` to `root_positions`, then append `start` (`trunk_origin.relative(dir)`).
  4. **Place all roots**: for each `rp` in `root_positions` in list order, call `place_root(level, root_setter, random, rp, tree)`.
  5. Return `true`.
- **`simulate_roots(level, random, root_pos, dir, root_origin, arm, layer) -> bool`**:
  1. If `layer == max_root_length || arm.size() > max_root_length`: return `false`.
  2. Compute `candidates = potential_root_positions(root_pos, dir, random, root_origin)`:
     - Let `below = root_pos.below()`, `next_to = root_pos.relative(dir)`, `width = distManhattan(root_pos, root_origin)`.
     - If `width > max_root_width - 3 && width <= max_root_width`:
       - `[below, next_to.below()]` if `random.nextFloat() < random_skew_chance` else `[below]`.
     - Else if `width > max_root_width`:
       - `[below]` (no RNG draw).
     - Else if `random.nextFloat() < random_skew_chance`:
       - `[below]`.
     - Else:
       - `[next_to]` if `random.nextBoolean()` else `[below]`.
  3. For each `cand` in `candidates` in order:
     - If `can_place_root(level, cand)`:
       - Append `cand` to `arm`.
       - If `!simulate_roots(level, random, cand, dir, root_origin, arm, layer + 1)`: return `false`.
  4. Return `true`.

---

### 7. Tree decorators (`minecraft:worldgen/tree_decorator_type`)

Dispatched by `"type"`. All 11 registered types in registry order:

#### 7.1 `minecraft:trunk_vine`
- **Fields**: none (`MapCodec.unit`).
- **Behavior**: for each `pos` in `context.logs` in order, test 4 sides in exact order **(WEST, EAST, NORTH, SOUTH)**:
  1. If `random.nextInt(3) > 0` and `context.is_air(pos.west())`: `place_vine(pos.west(), east)`.
  2. If `random.nextInt(3) > 0` and `context.is_air(pos.east())`: `place_vine(pos.east(), west)`.
  3. If `random.nextInt(3) > 0` and `context.is_air(pos.north())`: `place_vine(pos.north(), south)`.
  4. If `random.nextInt(3) > 0` and `context.is_air(pos.south())`: `place_vine(pos.south(), north)`.
  *(Note: each `nextInt(3)` is drawn unconditionally before checking `is_air`.)*

#### 7.2 `minecraft:leave_vine`
- **Fields**: `probability`: required `f32` in `[0.0, 1.0]`.
- **Behavior**: for each `pos` in `context.leaves` in order, test 4 sides in exact order **(WEST, EAST, NORTH, SOUTH)**:
  - For each `(offset_dir, vine_prop)` in `[(WEST, east), (EAST, west), (NORTH, south), (SOUTH, north)]`:
    - If `random.nextFloat() < probability` and `context.is_air(pos + offset_dir)`:
      - `add_hanging_vine(pos + offset_dir, vine_prop)`: place vine at `p`, then up to 4 additional blocks downward (`p.below()`, ..., while `is_air` and remaining steps `> 0`, so up to **5** vine blocks total).

#### 7.3 `minecraft:pale_moss`
- **Fields**:
  - `leaves_probability`: required `f32` in `[0.0, 1.0]`.
  - `trunk_probability`: required `f32` in `[0.0, 1.0]`.
  - `ground_probability`: required `f32` in `[0.0, 1.0]`.
- **Behavior**:
  1. `shuffled_logs = Util.shuffledCopy(context.logs, random)` *(consumed even if `logs` is empty? If empty, `shuffle` does 0 draws)*.
  2. If `shuffled_logs` is empty, return.
  3. `origin = min(shuffled_logs, by y)` *(first element with minimum `y` in the **shuffled** list — so when multiple logs share the minimum `y`, e.g. a 2×2 pale oak trunk base, `origin` is chosen randomly among them!)*.
  4. If `random.nextFloat() < ground_probability`:
     - Look up feature `minecraft:pale_moss_patch` in `level.registryAccess()` (`Registries.FEATURE`); if present, invoke its `place(level, level.getLevel().getChunkSource().getGenerator(), random, origin.above())`.
  5. For each `pos` in **`context.logs`** (the unshuffled Y-sorted list):
     - If `random.nextFloat() < trunk_probability` and `context.is_air(pos.below())`: `add_moss_hanger(pos.below())`.
  6. For each `pos` in **`context.leaves`** (the unshuffled Y-sorted list):
     - If `random.nextFloat() < leaves_probability` and `context.is_air(pos.below())`: `add_moss_hanger(pos.below())`.
- **`add_moss_hanger(pos)`**:
  - While `context.is_air(pos.below()) && !(random.nextFloat() < 0.5_f64)` *(note: `is_air` is checked **before** `nextFloat()`; `0.5` is a `f64` literal, equivalent to `0.5_f32`)*:
    - `context.set_block(pos, minecraft:pale_hanging_moss[tip=false])`.
    - `pos = pos.below()`.
  - `context.set_block(pos, minecraft:pale_hanging_moss[tip=true])`.

#### 7.4 `minecraft:creaking_heart`
- **Fields**: `probability`: required `f32` in `[0.0, 1.0]`.
- **Behavior**:
  1. If `context.logs` is empty, return (no RNG draw).
  2. If `random.nextFloat() >= probability`, return.
  3. Copy `context.logs` into a mutable list and call `Util.shuffle(list, random)`.
  4. Scan the shuffled list for the first `pos` such that **all 6** orthogonal neighbors (`Direction.values()`) match `#minecraft:logs`.
  5. If found, call `context.set_block(pos, minecraft:creaking_heart[axis=y, creaking_heart_state=dormant, natural=true])`.

#### 7.5 `minecraft:cocoa`
- **Fields**: `probability`: required `f32` in `[0.0, 1.0]`.
- **Behavior**:
  1. If `random.nextFloat() >= probability`, return *(drawn **before** checking `logs.isEmpty()`)*.
  2. If `context.logs` is empty, return.
  3. `tree_y = context.logs[0].y`.
  4. For each `pos` in `context.logs` with `pos.y - tree_y <= 2`:
     - For each `dir` in `[NORTH, EAST, SOUTH, WEST]`:
       - If `random.nextFloat() <= 0.25_f32` *(inclusive `<=`)*:
         - `opp = dir.opposite()`, `cocoa_pos = pos + (opp.dx, 0, opp.dz)`.
         - If `context.is_air(cocoa_pos)`:
           - `age = random.nextInt(3)`.
           - `context.set_block(cocoa_pos, minecraft:cocoa[age=<age>, facing=<dir>])`.

#### 7.6 `minecraft:shelf_mushroom`
- **Fields**: `probability`: required `f32` in `[0.0, 1.0]`.
- **Behavior**:
  1. If `random.nextFloat() >= probability`, return *(drawn before `logs.isEmpty()`)*.
  2. If `context.logs` is empty, return.
  3. Detect fallen log: `is_fallen = (context.logs[0].y == context.logs[last].y)`.
  4. **Standing tree (`!is_fallen`)**:
     - Draw `first = HORIZONTAL.getRandomDirection(random)`, set `dirs = [first, first.getClockWise()]`.
     - `base_y = context.logs[0].y`.
     - For each `log_pos` in `context.logs`:
       - Let `dy = log_pos.y - base_y`. If `1 <= dy <= 4`:
         - For each `facing` in `dirs`:
           - If `random.nextFloat() <= 0.25_f32` *(inclusive `<=`)*:
             - `mpos = log_pos + (facing.dx, 0, facing.dz)`.
             - If `context.is_replaceable(mpos) && !context.is_water_or_water_nearby(mpos)` AND block at `mpos.below()` is NOT `minecraft:shelf_mushroom`:
               - Draw `age = random.nextInt(2)`.
               - `context.set_block(mpos, minecraft:shelf_mushroom[age=<age>, facing=<facing>])`.
               - **`break`** out of the `dirs` loop for this `log_pos` (at most one shelf mushroom per log block).
  5. **Fallen log (`is_fallen`)**:
     - `log_axis = X if context.logs[0].x != context.logs[last].x else Z`.
     - `dirs = [NORTH, SOUTH]` if `log_axis == X` else `[EAST, WEST]`. *(No RNG draw.)*
     - For each `log_pos` in `context.logs`:
       - For each `facing` in `dirs`:
         - If `random.nextFloat() <= 0.25_f32`:
           - `mpos = log_pos + (facing.dx, 0, facing.dz)`.
           - If `context.is_replaceable(mpos) && !context.is_water_or_water_nearby(mpos)` AND no horizontal neighbor of `mpos` is `minecraft:shelf_mushroom` AND no horizontal neighbor of `log_pos` is `minecraft:shelf_mushroom`:
             - Draw `age = random.nextInt(2)`.
             - `context.set_block(mpos, minecraft:shelf_mushroom[age=<age>, facing=<facing>])`.

#### 7.7 `minecraft:beehive`
- **Fields**: `probability`: required `f32` in `[0.0, 1.0]`.
- **Constants**: `WORLDGEN_FACING = SOUTH`; `SPAWN_DIRECTIONS = [EAST, SOUTH, WEST]` (`HORIZONTAL` minus `NORTH`).
- **Behavior**:
  1. If `context.logs` is empty, return (no RNG draw).
  2. If `random.nextFloat() >= probability`, return.
  3. Compute `hive_y`:
     - If `!context.leaves.is_empty()`: `max(context.leaves[0].y - 1, context.logs[0].y + 1)` (no RNG).
     - Else: `min(context.logs[0].y + 1 + random.nextInt(3), context.logs[last].y)`.
  4. Build `candidates`: for each `log_pos` in `context.logs` with `log_pos.y == hive_y`, append `log_pos.east()`, `log_pos.south()`, `log_pos.west()` in that order.
  5. If `candidates` is empty, return.
  6. Call `Util.shuffle(candidates, random)`.
  7. Find the first `pos` in `candidates` such that `context.is_air(pos) && context.is_air(pos.south())`.
  8. If found:
     - `context.set_block(pos, minecraft:bee_nest[facing=south, honey_level=0])`.
     - Look up the `minecraft:beehive` block entity at `pos` (`level.getBlockEntity(pos, BEEHIVE)` — in `WorldGenRegion`, `setBlock` wrote a `"DUMMY"` block-entity tag to the `ProtoChunk` and `getBlockEntity` instantiates the live `BeehiveBlockEntity` and attaches it to the chunk):
       - If present:
         - Draw `num_bees = 2 + random.nextInt(2)` (2 or 3).
         - Repeat `num_bees` times:
           - Draw `ticks_in_hive = random.nextInt(599)` (`0..598`).
           - Store occupant `(entity_data={id: "minecraft:bee"}, ticks_in_hive, min_ticks_in_hive=600)`.

#### 7.8 `minecraft:alter_ground`
- **Fields**: `provider`: required `Holder<BlockStateProvider>`.
- **Behavior**:
  1. Let `base_list = TreeFeature.getLowestTrunkOrRootOfTree(context)` (§2.3). If empty, return.
  2. `min_y = base_list[0].y`.
  3. For each `pos` in `base_list` with `pos.y == min_y`:
     - Place 4 fixed circles in order:
       1. `place_circle(pos.west().north())` (`(-1, 0, -1)`)
       2. `place_circle(pos.east(2).north())` (`(+2, 0, -1)`)
       3. `place_circle(pos.west().south(2))` (`(-1, 0, +2)`)
       4. `place_circle(pos.east(2).south(2))` (`(+2, 0, +2)`)
     - Repeat 5 times:
       - Draw `v = random.nextInt(64)`, `xx = v % 8`, `zz = v / 8`.
       - If `xx == 0 || xx == 7 || zz == 0 || zz == 7` (border of an 8×8 square):
         - `place_circle(pos + (-3 + xx, 0, -3 + zz))`.
- **`place_circle(center)`**:
  - Loop `xx = -2 ..= 2` (outer), `zz = -2 ..= 2` (inner):
    - If `|xx| != 2 || |zz| != 2` (21-block rounded 5×5):
      - `place_block_at(center + (xx, 0, zz))`:
        - For `dy = 2` down to `-3` inclusive:
          - `cursor = target.above(dy)`.
          - `s = provider.getOptionalState(level, random, cursor)`.
          - If `s == Some(state)`: `context.set_block(cursor, state)` and `break`.
          - If `!context.is_air(cursor) && dy < 0`: `break`.

#### 7.9 `minecraft:attached_to_leaves`
- **Fields**:
  - `probability`: required `f32` in `[0.0, 1.0]`.
  - `exclusion_radius_xz`: required `i32` in `[0, 16]`.
  - `exclusion_radius_y`: required `i32` in `[0, 16]`.
  - `block_provider`: required `Holder<BlockStateProvider>`.
  - `required_empty_blocks`: required `i32` in `[1, 16]`.
  - `directions`: required non-empty array of `Direction` strings (`down`, `up`, `north`, `south`, `west`, `east`).
- **Behavior**:
  1. Initialize empty `blacklist: HashSet<BlockPos>`.
  2. For each `leaf_pos` in `Util.shuffledCopy(context.leaves, random)`:
     - Draw `dir = Util.getRandom(directions, random)` *(unconditionally, even if blacklisted!)*.
     - `place_pos = leaf_pos.relative(dir)`.
     - If `!blacklist.contains(place_pos)` AND `random.nextFloat() < probability` AND `is_air(leaf_pos.relative(dir, i))` for all `i = 1 ..= required_empty_blocks`:
       - Add every block in the inclusive box `[place_pos + (-exclusion_radius_xz, -exclusion_radius_y, -exclusion_radius_xz), place_pos + (+exclusion_radius_xz, +exclusion_radius_y, +exclusion_radius_xz)]` to `blacklist`.
       - `context.set_block(place_pos, block_provider.getState(level, random, place_pos))`.
     *(Note short-circuit order: if `place_pos` is already blacklisted, `nextFloat()` is **not** drawn.)*

#### 7.10 `minecraft:place_on_ground`
- **Fields**:
  - `tries`: optional positive `i32` (`>= 1`), default `128`.
  - `radius`: optional non-negative `i32` (`>= 0`), default `2`.
  - `height`: optional non-negative `i32` (`>= 0`), default `1`.
  - `block_state_provider`: required `Holder<BlockStateProvider>`.
- **Behavior**:
  1. Let `base_list = TreeFeature.getLowestTrunkOrRootOfTree(context)`. If empty, return.
  2. Let `min_y = base_list[0].y`. Compute `min_x, max_x, min_z, max_z` across all `p` in `base_list` with `p.y == min_y`.
  3. Inflate the box `(min_x, min_y, min_z) ..= (max_x, min_y, max_z)` by `(radius, height, radius)` to get `bb = (min_x - radius, min_y - height, min_z - radius) ..= (max_x + radius, min_y + height, max_z + radius)`.
  4. Repeat `tries` times:
     - Draw `x = random.nextIntBetweenInclusive(bb.min_x, bb.max_x)`.
     - Draw `y = random.nextIntBetweenInclusive(bb.min_y, bb.max_y)`.
     - Draw `z = random.nextIntBetweenInclusive(bb.min_z, bb.max_z)`.
     - Let `pos = (x, y, z)`, `above = pos.above()`.
     - If (`level.getBlockState(above)` is air OR `minecraft:vine`) AND `level.getBlockState(pos).isSolidRender()` AND `level.getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).y <= above.y`:
       - `context.set_block(above, block_state_provider.getState(level, random, above))`.

#### 7.11 `minecraft:attached_to_logs`
- **Fields**:
  - `probability`: required `f32` in `[0.0, 1.0]`.
  - `block_provider`: required `Holder<BlockStateProvider>`.
  - `directions`: required non-empty array of `Direction` strings.
- **Behavior**:
  1. For each `log_pos` in `Util.shuffledCopy(context.logs, random)`:
     - Draw `dir = Util.getRandom(directions, random)`.
     - `place_pos = log_pos.relative(dir)`.
     - If `random.nextFloat() <= probability` *(inclusive `<=`)* AND `context.is_air(place_pos)`:
       - `context.set_block(place_pos, block_provider.getState(level, random, place_pos))`.

---

### 8. Sapling growth integration (`TreeGrower`)

Saplings, planted mangrove propagules, and azalea bushes grow by invoking `TreeGrower::growTree(server_level, chunk_generator, pos, sapling_state, random)`, which resolves a configured `minecraft:tree` feature from `Registries.FEATURE` and calls `Feature::place`.

#### 8.1 Built-in `TreeGrower` registry and block bindings (26.3)
Each `TreeGrower` holds three `WeightedList<ResourceKey<Feature>>` tables (`trees`, `mega_trees`, `flower_trees`) and an optional `shortest_tree_type` (used only by bonemeal build-height validation). A non-empty `WeightedList` selects an entry via `random.nextInt(total_weight)` (consuming 1 draw even when `total_weight == 1`, i.e., `nextInt(1)`), iterating entries in listed order; an empty `WeightedList` returns `None` **without** drawing RNG.

| Grower name | Blocks using it | `trees` (weight) | `mega_trees` (weight) | `flower_trees` (weight) | `shortest_tree_type` |
|---|---|---|---|---|---|
| `oak` | `oak_sapling` | `oak` (9), `fancy_oak` (1) | — | `oak_bees_005` (9), `fancy_oak_bees_005` (1) | `oak` |
| `spruce` | `spruce_sapling` | `spruce` (1) | `mega_spruce` (1), `mega_pine` (1) | — | `spruce` |
| `mangrove` | `mangrove_propagule` | `mangrove` (15), `tall_mangrove` (85) | — | — | `mangrove` |
| `azalea` | `azalea`, `flowering_azalea` | `azalea_tree` (1) | — | — | `azalea_tree` |
| `birch` | `birch_sapling` | `birch` (1) | — | `birch_bees_005` (1) | `birch` |
| `jungle` | `jungle_sapling` | `jungle_tree_no_vine` (1) | `mega_jungle_tree` (1) | — | `jungle_tree_no_vine` |
| `acacia` | `acacia_sapling` | `acacia` (1) | — | — | `acacia` |
| `cherry` | `cherry_sapling` | `cherry` (1) | — | `cherry_bees_005` (1) | `cherry` |
| `dark_oak` | `dark_oak_sapling` | — | `dark_oak` (1) | — | none |
| `pale_oak` | `pale_oak_sapling` | — | `pale_oak_bonemeal` (1) | — | none |
| `poplar` | `poplar_sapling` | `red_poplar` (1), `orange_poplar` (1), `yellow_poplar` (1) | — | — | `red_poplar` |

*(All feature keys above are in the `minecraft` namespace.)*

#### 8.2 `TreeGrower::growTree` ordered algorithm
1. **Mega-tree attempt** *(evaluated first!)*:
   1. Sample `mega_key = mega_trees.getRandom(random)` (consumes RNG whenever `mega_trees` is non-empty — **even if the sapling is not in a 2×2 square!**).
   2. If `mega_key` is present, look up `feature` in `Registries.FEATURE`.
   3. If found, run the **2×2 sapling check** `find_two_by_two(level, sapling_state, pos)`:
      - Loop `dx = 0` down to `-1` (outer), `dz = 0` down to `-1` (inner) — testing NW-corner candidates in order `(0,0), (0,-1), (-1,0), (-1,-1)`:
        - Read the 4 block states at `(pos + (dx, 0, dz))`, `(pos + (dx + 1, 0, dz))`, `(pos + (dx, 0, dz + 1))`, `(pos + (dx + 1, 0, dz + 1))`.
        - If all 4 have the **same `Block`** as `sapling_state` (regardless of `stage` property), return `Some((dx, dz, saved_4_states))`.
   4. If a 2×2 match was found:
      - **Remove all 4 saplings** in the order above: replace each position with `level.getFluidState(p).createLegacyBlock()` (air or water) using update flags **`818`** (`UPDATE_CLIENTS | UPDATE_SKIP_ALL_SIDEEFFECTS` = `2 | 816`, where `816 = 16 | 32 | 256 | 512`).
      - Call `feature.place(level, chunk_generator, random, pos + (dx, 0, dz))`.
      - If `place` returns `true`, return `true`.
      - Otherwise **restore all 4 saved block states** using update flags **`260`** (`UPDATE_NONE` = `4 | 256`) and return `false` (does **not** fall through to a single tree!).
2. **Single-tree attempt**:
   1. Check `has_flowers(level, pos)`: scan the inclusive box `[pos + (-2, -1, -2), pos + (+2, +1, +2)]` for any block matching `#minecraft:flowers`.
   2. Sample `key = flower_trees.getRandom(random)` if `has_flowers && !flower_trees.is_empty()`, else `trees.getRandom(random)`.
   3. If `key` is absent or not found in `Registries.FEATURE`, return `false`.
   4. Remove the single sapling at `pos` with `level.getFluidState(pos).createLegacyBlock()` and flags **`818`**.
   5. Call `feature.place(level, chunk_generator, random, pos)`.
   6. If `place` returns `true`, return `true`.
   7. Otherwise restore `sapling_state` at `pos` with flags **`260`** and return `false`.

#### 8.3 Sapling / propagule / azalea growth entry points
- **`SaplingBlock`**:
  - `randomTick`: if `max_local_raw_brightness(pos.above()) >= 9` AND `random.nextInt(7) == 0`, call `advanceTree`.
  - `advanceTree`: if `stage == 0`, set `stage = 1` with flags `260`; else call `tree_grower.growTree(...)`.
  - `isValidBonemealTarget`: requires `tree_grower.canGrow(level, pos, state)` AND `level.isInsideBuildHeight(pos.above(min_height))`, where `min_height` is `shortest_tree_type.trunk_placer.base_height` (or `0` if absent). Note that `canGrow` calls `getConfiguredFeature(level.getRandom(), hasFlowers)` and `getConfiguredMegaFeature(level.getRandom())` (consuming `level.getRandom()`!), returning `find_two_by_two(...).isPresent()` when `feature_key == null && mega_feature_key != null`, else `true`.
  - `isBonemealSuccess`: `level.getRandom().nextFloat() < 0.45_f64`.
- **`MangrovePropaguleBlock`** (subclass of `SaplingBlock`, waterloggable):
  - When `hanging == false`, `randomTick` skips the light check and calls `advanceTree` on `random.nextInt(7) == 0`; `isValidBonemealTarget` returns `true` (skipping `canGrow` and build-height checks); `isBonemealSuccess` inherits `< 0.45`.
  - When `hanging == true`, `randomTick` and bonemeal only advance `age` (`0 → 4`), never growing a tree.
- **`AzaleaBlock`** (`azalea` and `flowering_azalea`):
  - No random-tick growth; bonemeal only (`isValidBonemealTarget` checks `level.isInsideBuildHeight(pos.above(min_height + 2))` and `level.getFluidState(pos.above()).isEmpty()`; `isBonemealSuccess` `< 0.45`; `performBonemeal` directly calls `TreeGrower.AZALEA.growTree` with no two-stage `STAGE` cycle).

---

## Edge cases and known quirks

1. **`HashSet<BlockPos>` iteration order controls within-Y decorator ordering [oracle]**: `TreeFeature::place` stores `root_positions`, `trunks`, and `foliage` in `HashSet<BlockPos>`, and `TreeDecorator.Context` sorts copies only by `y`. Because Java TimSort is stable, ties at the same `y` stay in `HashSet` bucket order (based on `Vec3i.hashCode() = (y + z * 31) * 31 + x`, Java `HashMap` spread `h ^ (h >>> 16)`, initial capacity 16, load factor 0.75). Every decorator that iterates or shuffles `logs`, `leaves`, or `roots` (`trunk_vine`, `leave_vine`, `pale_moss`, `creaking_heart`, `cocoa`, `shelf_mushroom`, `beehive`, `alter_ground`, `attached_to_leaves`, `attached_to_logs`) depends on this tie-breaking order for exact RNG parity on multi-block layers.
2. **Roots run *before* trunk, and `below_trunk_provider` blocks are inserted into `trunks`**: `place_below_trunk_block` writes via `trunk_setter`, so dirt/rooted-dirt blocks placed under the trunk are present in `context.logs()`! This affects `getLowestTrunkOrRootOfTree`, `BeehiveDecorator` (`logs[0].y + 1`), `CocoaDecorator` (`pos.y - logs[0].y <= 2`), `ShelfMushroomDecorator` (`1 <= dy <= 4`), `PaleMossDecorator` (`origin = min y` log, placing `pale_moss_patch` at `origin.above()`), `TrunkVineDecorator` (can place vines adjacent to the dirt block under the trunk if exposed to air!), and `updateLeaves` (seeds dirt in bucket 0, making leaves adjacent to that dirt `distance=1`!).
3. **Unclipped vs clipped height split**: `foliage_height` and `leaf_radius` are sampled from the unclipped `tree_height` *before* `get_max_free_tree_height`, whereas `place_trunk` and `create_foliage` receive `clipped_tree_height`.
4. **Always two RNG draws in `get_tree_height`**: `TrunkPlacer.getTreeHeight` always calls `nextInt(height_rand_a + 1)` and `nextInt(height_rand_b + 1)` even when either is `0` (`nextInt(1)` still advances the RNG).
5. **`PoplarTrunkPlacer` shuffles directions on every Y level**: `Direction.allShuffled(random)` (5 `nextInt` draws) is invoked unconditionally inside the `for y = 0 ..< tree_height` loop, even on Y levels that do not place branches.
6. **`PoplarFoliagePlacer` places internal branch logs via `foliage_setter` and re-samples `foliage_provider`**: `tryPlaceLog` compares the world state to a fresh `foliage_provider.getState(level, random, pos)` call and writes the log into `foliage` (not `trunks`). Those logs still act as distance-`0` sources in `updateLeaves` if BFS reaches them via `#minecraft:prevents_nearby_leaf_decay`.
7. **`CherryFoliagePlacer` codec encode bug**: on serialization, `corner_hole_chance` uses the getter for `wide_bottom_layer_hole_chance`. Decoding reads both fields cleanly.
8. **`FancyTrunkPlacer` `clustersPerY = Math.min(1, ...)`**: always clamps cluster attempts per Y to `1` regardless of tree height; uses JVM `Math.sin`/`Math.cos`/`Math.sqrt`/`Math.pow` **[oracle]** and `(int)` truncation toward zero for `branchTop`.
9. **`BlobFoliagePlacer` consumes `nextInt(2)` on `y == 0` corners**: the `||` order evaluates `random.nextInt(2) == 0` before `y == 0`.
10. **`DarkOakTrunkPlacer` checks only the `(0, 0)` corner with `is_air_or_leaves`**: when a 2×2 trunk layer's `(tx, yy, tz)` is neither air nor leaves, none of the 4 logs on that layer are placed, even if the other 3 corners are air. Conversely, side branches use the un-leaned `(origin.x, origin.z)`.
11. **`DarkOakFoliagePlacer` asymmetric `y == 0` corner cutout**: for `double_trunk`, skipping `dx ∈ {-r, r, r + 1}` × `dz ∈ {-r, r, r + 1}` cuts 1 block at `(-r, -r)` and 2×2 blocks at `(+r..+r+1, +r..+r+1)`.
12. **`SpruceFoliagePlacer` loop lower bound is `-H`, not `offset - H`**: a positive `offset` adds extra top layers without shifting the bottom upward.
13. **`RandomSpreadFoliagePlacer` still consumes `offset.sample(random)`**: performed by the base `FoliagePlacer.createFoliage` wrapper even though the subtype ignores `offset`.
14. **Waterlogging mismatch between leaves and roots**: `FoliagePlacer.try_place_leaf` sets `waterlogged=true` only for **source** `minecraft:water` (`isSourceOfType(Fluids.WATER)`), whereas `RootPlacer.get_potentially_waterlogged_state` checks the `#minecraft:water` fluid tag (both source and flowing water).
15. **`PaleMossDecorator` ignores `TreeFeature`'s `chunk_generator` argument**: it calls `level.getLevel().getChunkSource().getGenerator()`, and picks `origin` as the first minimum-Y element in the **shuffled** copy of `logs`.
16. **Inequality direction differences across decorators and placers**:
    - Strict `<` (`nextFloat() < p`): `upwards_branching` branch chance, `cherry` vertical step, `cherry` foliage holes, `mangrove` root skew & above-root chance, `leave_vine`, `pale_moss` (ground/trunk/leaves and hanger `< 0.5`), `creaking_heart` (`!(>=)`), `cocoa` top-level (`!(>=)`), `shelf_mushroom` top-level (`!(>=)`), `beehive` (`!(>=)`), `attached_to_leaves`.
    - Inclusive `<=` (`nextFloat() <= p` or `!(> p)`): `FoliagePlacer.tryPlaceExtension` (`!(> chance)`), `poplar` foliage `side_hole_chance`, `cocoa` per-side `<= 0.25F`, `shelf_mushroom` per-side `!(> 0.25F)`, `attached_to_logs`.
17. **Write-zone clipping still records positions [oracle]**: in `WorldGenRegion`, if `setBlock` returns `false` outside the 3×3 chunk write zone, all four tree setters still add `pos` to their sets and return success from `place_log` / `try_place_leaf`.
18. **Mega-tree `WeightedList` draw happens before the 2×2 check**: in `TreeGrower::growTree`, a lone spruce, jungle, dark oak, or pale oak sapling still draws from `mega_trees` before failing `find_two_by_two`.
19. **Block-update flag differences**: tree blocks use `19` (`1 | 2 | 16`), edge shape updates use `2` (`3 & ~1`), sapling removal uses `818` (`2 | 816`), and sapling rollback / stage increment uses `260` (`4 | 256`).

---

## Constants and IDs

### Built-in registry IDs (26.3, exact registration order)
- **Feature type** (`minecraft:worldgen/feature_type`): `minecraft:tree`.
- **Feature size types** (`minecraft:worldgen/feature_size_type`, 2):
  `minecraft:two_layers_feature_size`, `minecraft:three_layers_feature_size`.
- **Trunk placer types** (`minecraft:worldgen/trunk_placer_type`, 10):
  `minecraft:straight_trunk_placer`, `minecraft:forking_trunk_placer`, `minecraft:giant_trunk_placer`, `minecraft:mega_jungle_trunk_placer`, `minecraft:dark_oak_trunk_placer`, `minecraft:fancy_trunk_placer`, `minecraft:bending_trunk_placer`, `minecraft:upwards_branching_trunk_placer`, `minecraft:cherry_trunk_placer`, `minecraft:poplar_trunk_placer`.
- **Foliage placer types** (`minecraft:worldgen/foliage_placer_type`, 12):
  `minecraft:blob_foliage_placer`, `minecraft:spruce_foliage_placer`, `minecraft:pine_foliage_placer`, `minecraft:acacia_foliage_placer`, `minecraft:bush_foliage_placer`, `minecraft:fancy_foliage_placer`, `minecraft:jungle_foliage_placer`, `minecraft:mega_pine_foliage_placer`, `minecraft:dark_oak_foliage_placer`, `minecraft:random_spread_foliage_placer`, `minecraft:cherry_foliage_placer`, `minecraft:poplar_foliage_placer`.
- **Root placer types** (`minecraft:worldgen/root_placer_type`, 1):
  `minecraft:mangrove_root_placer`.
- **Tree decorator types** (`minecraft:worldgen/tree_decorator_type`, 11):
  `minecraft:trunk_vine`, `minecraft:leave_vine`, `minecraft:pale_moss`, `minecraft:creaking_heart`, `minecraft:cocoa`, `minecraft:shelf_mushroom`, `minecraft:beehive`, `minecraft:alter_ground`, `minecraft:attached_to_leaves`, `minecraft:place_on_ground`, `minecraft:attached_to_logs`.

### Referenced block/fluid tags and hardcoded IDs
- **Block tags**: `#minecraft:leaves`, `#minecraft:logs`, `#minecraft:replaceable_by_trees`, `#minecraft:prevents_nearby_leaf_decay`, `#minecraft:flowers`.
- **Fluid tags / fluids**: `#minecraft:water` (root waterlogging), `minecraft:water` source fluid (leaf waterlogging).
- **Hardcoded blocks & block entities**: `minecraft:vine`, `minecraft:water`, `minecraft:pale_hanging_moss`, `minecraft:creaking_heart`, `minecraft:cocoa`, `minecraft:shelf_mushroom`, `minecraft:bee_nest` (block entity `minecraft:beehive`, occupant entity `minecraft:bee`).
- **Hardcoded feature key**: `minecraft:pale_moss_patch` (invoked by `minecraft:pale_moss`).

### Numeric constants
- Block update flag masks:
  - `BLOCK_UPDATE_FLAGS` (tree placement & leaf distance update): `19` (`UPDATE_NEIGHBORS=1 | UPDATE_CLIENTS=2 | UPDATE_KNOWN_SHAPE=16`).
  - `StructureTemplate.updateShapeAtEdge` mode: `3` (`UPDATE_NEIGHBORS=1 | UPDATE_CLIENTS=2`), masked with `& ~1` to `2` on `setBlock`.
  - Sapling removal: `818` (`2 | 16 | 32 | 256 | 512`).
  - Sapling restore / stage advance: `260` (`4 | 256`).
- Leaf decay distance: `DECAY_DISTANCE = 7` (valid `distance` property values `1..7`).
- `Mth` sine LUT (used by `MegaJungleTrunkPlacer`): 65,536-entry `f32` table `SIN[i] = f32(Math.sin(f64(i) / 10430.378350470453))`; `Mth.sin(d) = SIN[(i64(d * 10430.378350470453)) & 65535]`, `Mth.cos(d) = SIN[(i64(d * 10430.378350470453 + 16384.0)) & 65535]`.
- `MegaJungleTrunkPlacer` `TWO_PI`: `6.2831855_f32` (`0x40C90FDB`).
- `FancyTrunkPlacer`: `0.618`, `1.382`, `0.381`, `0.328`, `0.3_f32`, `0.2`, `0.5_f32`, `2.0_f32`, `Math.PI = 0x400921FB54442D18`.
- `Vec3i.hashCode()`: `((y + z * 31) * 31 + x)` (32-bit wrapping `i32`).
- `BeehiveDecorator`: occupant `ticks_in_hive = nextInt(599)`, `min_ticks_in_hive = 600`, `num_bees = 2 + nextInt(2)`.

---

## Oracle probe points

Public classes and methods in 26.3 suitable for black-box oracle instrumentation (`golden-features`):

- `net.minecraft.world.level.levelgen.feature.TreeFeature`:
  - `place(WorldGenLevel, ChunkGenerator, RandomSource, BlockPos)`
  - `validTreePos(LevelSimulatedReader, BlockPos)`, `isAirOrLeaves(LevelSimulatedReader, BlockPos)`, `isVine(LevelSimulatedReader, BlockPos)`
  - `getLowestTrunkOrRootOfTree(TreeDecorator.Context)`
- `net.minecraft.world.level.levelgen.feature.featuresize.FeatureSize` (`TwoLayersFeatureSize`, `ThreeLayersFeatureSize`):
  - `getSizeAtHeight(int, int)`, `minClippedHeight()`
- `net.minecraft.world.level.levelgen.feature.trunkplacers.TrunkPlacer` and all 10 subclasses (`StraightTrunkPlacer`, `ForkingTrunkPlacer`, `GiantTrunkPlacer`, `MegaJungleTrunkPlacer`, `DarkOakTrunkPlacer`, `FancyTrunkPlacer`, `BendingTrunkPlacer`, `UpwardsBranchingTrunkPlacer`, `CherryTrunkPlacer`, `PoplarTrunkPlacer`):
  - `getTreeHeight(RandomSource)`, `placeTrunk(WorldGenLevel, BiConsumer, RandomSource, int, BlockPos, TreeFeature)`, `isFree(WorldGenLevel, BlockPos)`
- `net.minecraft.world.level.levelgen.feature.foliageplacers.FoliagePlacer` and all 12 subclasses (`BlobFoliagePlacer`, `SpruceFoliagePlacer`, `PineFoliagePlacer`, `AcaciaFoliagePlacer`, `BushFoliagePlacer`, `FancyFoliagePlacer`, `MegaJungleFoliagePlacer`, `MegaPineFoliagePlacer`, `DarkOakFoliagePlacer`, `RandomSpreadFoliagePlacer`, `CherryFoliagePlacer`, `PoplarFoliagePlacer`):
  - `foliageHeight(RandomSource, int, TreeFeature)`, `foliageRadius(RandomSource, int)`, `createFoliage(WorldGenLevel, FoliageSetter, RandomSource, TreeFeature, int, FoliageAttachment, int, int)`
- `net.minecraft.world.level.levelgen.feature.rootplacers.RootPlacer` (`MangroveRootPlacer`):
  - `getTrunkOrigin(BlockPos, RandomSource)`, `placeRoots(WorldGenLevel, BiConsumer, RandomSource, BlockPos, BlockPos, TreeFeature)`
- `net.minecraft.world.level.levelgen.feature.treedecorators.TreeDecorator` and all 11 subclasses (`TrunkVineDecorator`, `LeaveVineDecorator`, `PaleMossDecorator`, `CreakingHeartDecorator`, `CocoaDecorator`, `ShelfMushroomDecorator`, `BeehiveDecorator`, `AlterGroundDecorator`, `AttachedToLeavesDecorator`, `PlaceOnGroundDecorator`, `AttachedToLogsDecorator`):
  - `place(TreeDecorator.Context)`
- `net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate`:
  - `updateShapeAtEdge(LevelAccessor, int, DiscreteVoxelShape, int, int, int)`
- `net.minecraft.world.level.block.LeavesBlock`:
  - `getOptionalDistanceAt(BlockState)`
- `net.minecraft.world.level.block.grower.TreeGrower`:
  - `growTree(ServerLevel, ChunkGenerator, BlockPos, BlockState, RandomSource)`, `canGrow(ServerLevel, BlockPos, BlockState)`, `getMinimumHeight(ServerLevel)`
- `net.minecraft.world.level.block.SaplingBlock`, `MangrovePropaguleBlock`, `AzaleaBlock`:
  - `advanceTree(ServerLevel, BlockPos, BlockState, RandomSource)`, `performBonemeal(ServerLevel, RandomSource, BlockPos, BlockState, BonemealSource)`

---

## Open questions

1. **[oracle] Java `HashSet<BlockPos>` iteration order in `TreeDecorator.Context`**: `TreeDecorator.Context` sorts `logs`, `leaves`, and `roots` by `y` only, leaving same-`y` positions in JDK `HashMap` bucket order (capacity doubling from 16 at load factor `0.75`, hash `(h = Vec3i.hashCode) ^ (h >>> 16)`, bin treeification at 8 collisions with capacity `>= 64`).
   - **Concrete black-box probe**: Invoke `TreeFeature.place` through the oracle on multi-column and decorated trees (`minecraft:dark_oak`, `minecraft:pale_oak_creaking`, `minecraft:mega_jungle_tree`, `minecraft:mega_spruce`, `minecraft:swamp_oak`, `minecraft:mangrove`, `minecraft:red_poplar`) across 20 seeds and origins (including negative coordinates where `Vec3i.hashCode()` is negative) and capture the ordered `context.logs()`, `context.leaves()`, and `context.roots()` lists plus final chunk block states, verifying that replicating JDK `HashSet` bucket ordering matches 100% block-for-block (and whether bin treeification is ever reached in vanilla trees).
2. **[oracle] JVM `Math.sin`/`Math.cos`/`Math.pow`/`Math.sqrt` parity in `FancyTrunkPlacer`**: `FancyTrunkPlacer` uses JVM `Math` intrinsics (not `Mth` LUT or `StrictMath`) for branch polar coordinates and branch-base slopes.
   - **Concrete black-box probe**: Run `minecraft:fancy_oak` across 200 seeds on the oracle host and compare the emitted `FoliageAttachment` list and placed log coordinates against Rust `f64::sin`/`cos`/`powf`/`sqrt` to detect any 1-ULP floor boundary flip.
3. **[oracle] `WorldGenRegion` write-zone boundary clipping**: When a wide tree (`minecraft:red_poplar`, `minecraft:tall_mangrove`, `minecraft:mega_jungle_tree`) is anchored near the edge of a `WorldGenRegion` 3×3 chunk write zone, `setBlock` returns `false` while the tree's setters still record the clipped positions.
   - **Concrete black-box probe**: Place wide trees at chunk-edge offsets (`local x/z ∈ {0, 15}` on a 3×3 `WorldGenRegion` whose outer border rejects writes) via the oracle and verify whether `BoundingBox`, `updateLeaves`, decorators (`leave_vine`, `attached_to_leaves`, `shelf_mushroom`), and `updateShapeAtEdge` produce identical blocks inside the writable chunks when clipped positions remain in the tracking sets.
4. **[oracle] `StructureTemplate.updateShapeAtEdge` side effects during worldgen vs sapling growth**: Edge updates run with flags `2` (`UPDATE_CLIENTS`, no `UPDATE_NEIGHBORS`) and call `BlockState.updateShape` on both sides of every outer face of `shape`.
   - **Concrete black-box probe**: Place trees adjacent to water, vines, fences/walls, snowy grass blocks, and pre-existing leaves in both a `WorldGenRegion` and a live `ServerLevel` (via `TreeGrower.growTree`), then diff all block states and scheduled ticks within `bounds.inflatedBy(1)` immediately after `place` returns.

**Implementer findings (zenith-features, `golden-features`, 2026-11-25).** Observed against the vanilla 26.3 server
(forest-type Overworld chunks, 3 golden seeds plus 8 extra local seeds; one open mismatch, item 8):

1. **HashSet iteration order — answered, and it matters more than §2.4 states.** Zenith's `JavaBlockPosSet` matches a real
   JDK `HashSet` (insert, iteration order, and `iterator().next()` + `iterator.remove()`); it is checked against a
   JDK-generated fixture in `java_hash.rs` and against pale-oak (2×2 trunk, `pale_moss` shuffles) and jungle chunks.
   Two details of `updateLeaves` that §2.4 only implies: (a) each of the seven `toCheck` buckets is itself a
   `HashSet<BlockPos>` and the "pop" is `iterator().next()` + remove, i.e. **JDK iteration order within the bucket**; (b) the
   popped position is **not** re-checked against `shape.is_full`, so a position that was queued in bucket `d` and again
   in a higher bucket `d' > d` (found by a different parent before it was popped) is popped twice and the **second pop
   overwrites `distance` with `d'`**. Ordinary leaves therefore end up with the *largest* bucket they were queued in,
   not the BFS minimum (e.g. leaves directly next to a trunk log can read `distance=2`). Treeified bins (≥8 collisions at
   capacity ≥64) were never reached in the corpus.
2. **JVM `Math.sin`/`cos`/`sqrt`/`pow` in `fancy_trunk_placer` — answered.** Using `f64` `sin`/`cos` from
   `zenith_math::helpers::intrinsics` (the same routine the sine table is built from) and `f64::sqrt`/`x*x` for `pow(x, 2)`,
   `fancy_oak` (with and without `beehive` / `place_on_ground`) matched in every sampled chunk.
3. **Write-zone clipping — still open.** No corpus tree reached the 3×3 border, so the clipped-position behaviour is not
   exercised (Zenith records clipped positions exactly as §2.2 states).
4. **`updateShapeAtEdge` side effects — answered for the blocks that occur in forest-type chunks** (all observable
   through block states and the scheduled-tick counts of the chunk dump; every item was needed for parity):
   - Leaves schedule a block tick (`pos`, leaf block) iff `getDistanceAt(neighbour) + 1 != 1 || state.distance != 1 + …`
     exactly as §2.5 says, plus a water fluid tick when `waterlogged`; ticks are de-duplicated per `(pos, block)`.
   - `FallingBlock` family (sand, gravel, concrete powder, anvil) **schedules a block tick on every shape update**
     (seen with gravel/sand beside mangrove/birch trees: +2..+5 ticks per chunk).
   - `pale_hanging_moss` is **not** removed by `updateShape`: it schedules a 1-tick block tick when its upper neighbour
     cannot hold it, and for `dir == DOWN` sets `tip = !neighbour.is(pale_hanging_moss)`. Its support test accepts any
     full **collision** face (so leaves hold it), not only sturdy faces.
   - `pale_moss_carpet` (`MossyCarpetBlock`) recomputes its side values with `getUpdatedState`; a `bottom=true` carpet
     is treated as `createSides = true` (every supported horizontal side becomes `low`; `tall` if the carpet above has that
     side and is not `bottom`; a non-bottom carpet drops a side whose lower carpet has none).
   - Vines use `getUpdatedState` (no change for `DOWN`; air if no face remains); snowy grass/podzol/mycelium set `snowy`
     from the block above only for `dir == UP`; unsupported plants/carpets/snow layers/double plants become air;
     liquids schedule a fluid tick when either side is a source.
   Everything else touched in the corpus (logs, dirt, mud, roots, stone, moss blocks, …) has no observable shape update.
5. **Sapling growth (§8)** is not implemented (only the shared entry point `place_tree`), so it is unverified.
6. **`pale_moss` decorator with a 2×2 trunk — answered.** The `pale_moss_patch` origin is `min-y` of the *shuffled* log
   copy (the four `below_trunk` dirt blocks are the minimum), and the log / leaf hanger loops walk the `y`-sorted lists
   in HashSet tie order; with that every pale-garden / dark-forest chunk of the corpus matches.
7. **`creaking_heart` is only partly exercised**: a log whose six neighbours are all logs is never found in a plain 2×2
   trunk, so the decorator consumed its shuffle and placed nothing in the corpus; the placement branch is unverified.
8. **Open — lush-caves moss patch, one block (not tree related).** Seed `-8472910384721`, overworld chunk `(1469, -435)`
   (biome `lush_caves`, `y = 6`, chunk-local `x = 15`): block `(23519, 6, -6948)` is `minecraft:short_grass` in vanilla and
   `minecraft:moss_carpet` in Zenith (floor `moss_block` in both). It reproduces with tree placement disabled. The
   east neighbour chunk `(1470, -435)` (not part of the suite, compared via a debug mode) shows 11 more
   `moss_vegetation` picks that differ right after that position (same positions, swapped / shifted `short_grass`,
   `moss_carpet`, `tall_grass` choices), i.e. the RNG stream of that single `moss_patch` placement diverges at that visit,
   with every earlier visible pick identical. Not explained by `features.md` §4.55; probably an invisible
   difference in the `vegetation_patch` surface set or its RNG draws before that position.

---

## Parity status
Verified through `golden-features` (**1,800/1,800**, 2026-11-25): trees of every family (oak, birch, spruce/pine,
jungle incl. mega, acacia, dark oak, mangrove with roots, cherry, pale oak with creaking hearts not exercised, azalea,
fancy, fallen trees) and their decorators, including the leaf-distance pass in Java HashSet order (see
`java/hash-collections.md`, `java-hashset`). Sapling growth shares the entry point but is not exercised.
