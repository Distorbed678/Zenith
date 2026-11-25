# Block static facts dump (`ZBF1`)

Specification for `dev.zenith.oracle.BlockFacts` (`tools/oracle/src/dev/zenith/oracle/BlockFacts.java`), the
exhaustive per-block-state parity oracle described in [ADR 0009](../../adr/0009-block-static-facts.md).

After bootstrapping the game's static registries (`SharedConstants.tryDetectVersion()`, `Bootstrap.bootStrap()`)
and binding built-in server-data tags onto static registries (matching `TerrainProbe`'s `column` mode so
tag-backed predicates such as `#minecraft:blocks_motion_in_heightmap`,
`#minecraft:blocks_motion_in_heightmap_no_leaves`, `#minecraft:causes_suffocation`, `#minecraft:shulker_boxes`,
and `#minecraft:water` match a running server), `BlockFacts` streams the static facts of every `BlockState` in
`Block.BLOCK_STATE_REGISTRY` global-id order (`0..total_states`) with bounded memory.

---

## Invocation

```text
java -Xmx2G -cp <game_classpath>:<zenith-oracle.jar> dev.zenith.oracle.BlockFacts <output_file> [start_id end_id_exclusive]
```

- Run from a cache working directory (e.g. `~/.cache/zenith/parity/cwd`) so the game logger does not write `logs/` into the repo.
- With 1 argument (`<output_file>`), dumps all block states `[0, total_states)`.
- With 3 arguments (`<output_file> <start_id> <end_id_exclusive>`), dumps only global IDs in `[start_id, end_id_exclusive)` for debugging.
- Exits with non-zero status and a message on `stderr` if arguments are invalid, the range is out of bounds, or bootstrap/reflection fails.

---

## Primitive encodings

All multi-byte integers and IEEE-754 bit patterns are **big-endian** (matching `java.io.DataOutputStream`, `ZPD1`, `ZTP1`, and `ZTC1`).

| Type | Size | Encoding |
|---|---|---|
| `u8` | 1 byte | Unsigned 8-bit integer (`0..255`) |
| `u16` | 2 bytes | Unsigned 16-bit big-endian integer (`0..65535`) |
| `i32` | 4 bytes | Signed 32-bit two's-complement big-endian integer |
| `u32` | 4 bytes | Unsigned 32-bit big-endian integer |
| `f32bits` | 4 bytes | Raw IEEE-754 binary32 bits as big-endian `u32` (`Float.floatToRawIntBits` / `f32::to_bits`) |
| `f64bits` | 8 bytes | Raw IEEE-754 binary64 bits as big-endian `u64` (`Double.doubleToRawLongBits` / `f64::to_bits`) |
| `str` | `2 + len` bytes | `u16 byte_len` followed by `byte_len` bytes of standard UTF-8 |
| `box` | 48 bytes | 6 × `f64bits` in order: `min_x`, `min_y`, `min_z`, `max_x`, `max_y`, `max_z` |

### `shape` encoding

A `VoxelShape` is encoded as:

```text
box_count   : u16
// Present iff box_count > 0:
bounds      : box              // VoxelShape#bounds()
boxes       : box[box_count]   // VoxelShape#toAabbs(), which collects VoxelShape#forAllBoxes in yield order
```

When `shape.isEmpty()` is `true`, `box_count = 0` and neither `bounds` nor `boxes` is written (`VoxelShape#bounds()` throws `UnsupportedOperationException` on empty shapes).

---

## File layout

### Header (102 bytes fixed)

```text
magic           : [u8; 4] = b"ZBF1"     // 0x5A 0x42 0x46 0x31
version         : u16     = 1
total_states    : u32                   // Block.BLOCK_STATE_REGISTRY.size()
start_id        : u32                   // first global block-state id in this file
state_count     : u32                   // number of records that follow (end_id - start_id)
probe_positions : [i32; 3][3]           // 3 probe BlockPos (x, y, z):
                                        //   P0 = (0, 0, 0)
                                        //   P1 = (1, 0, 3)
                                        //   P2 = (-7, 64, 13)
view_probe_aabb : box                   // AABB passed to isViewBlocking at P0:
                                        //   (0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
```

Followed immediately by `state_count` state records in strictly ascending `global_id` order (`start_id .. start_id + state_count`), with no trailing bytes.

---

## Per-state record layout

Every position- or level-taking query is evaluated against `EmptyBlockGetter.INSTANCE` (where `getBlockState` returns `minecraft:air`, `getFluidState` returns `minecraft:empty`, and `getBlockEntity` returns `null`) and `CollisionContext.empty()` where a collision context is required.

### 1. Identity

```text
global_id         : u32                 // global id in Block.BLOCK_STATE_REGISTRY
block_id          : str                 // BuiltInRegistries.BLOCK.getKey(block).toString()
prop_count        : u8                  // number of properties on this block state
props[prop_count] : {
    name          : str                 // Property#getName()
    value         : str                 // Property.Value#valueName()
}                                       // in StateHolder#getValues() order
```

### 2. Flags (`u32` bitfield)

```text
flags : u32
```

| Bit | Mask | Field | Game source (evaluated at `EmptyBlockGetter.INSTANCE`, `P0` where applicable) |
|---:|---|---|---|
| 0 | `0x0000_0001` | `is_default_state` | `state == block.defaultBlockState()` |
| 1 | `0x0000_0002` | `is_air` | `state.isAir()` |
| 2 | `0x0000_0004` | `can_occlude` | `state.canOcclude()` |
| 3 | `0x0000_0008` | `solid_render` | `state.isSolidRender()` |
| 4 | `0x0000_0010` | `propagates_skylight_down` | `state.propagatesSkylightDown()` |
| 5 | `0x0000_0020` | `use_shape_for_light_occlusion` | `state.useShapeForLightOcclusion()` |
| 6 | `0x0000_0040` | `collision_shape_full_block` | `state.isCollisionShapeFullBlock(EmptyBlockGetter.INSTANCE, P0)` |
| 7 | `0x0000_0080` | `has_large_collision_shape` | `state.hasLargeCollisionShape()` |
| 8 | `0x0000_0100` | `has_dynamic_shape` | `block.hasDynamicShape()` |
| 9 | `0x0000_0200` | `is_randomly_ticking` | `state.isRandomlyTicking()` |
| 10 | `0x0000_0400` | `replaceable` | `state.canBeReplaced()` |
| 11 | `0x0000_0800` | `liquid` | `state.liquid()` (deprecated) |
| 12 | `0x0000_1000` | `legacy_solid` | `state.isSolid()` (deprecated) |
| 13 | `0x0000_2000` | `blocks_motion_in_heightmap` | `state.is(BlockTags.BLOCKS_MOTION_IN_HEIGHTMAP)` — 26.3 heightmap motion predicate (`OCEAN_FLOOR`, `OCEAN_FLOOR_WG`, `MOTION_BLOCKING`) |
| 14 | `0x0000_4000` | `blocks_motion_in_heightmap_no_leaves` | `state.is(BlockTags.BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES)` — 26.3 heightmap motion-no-leaves predicate (`MOTION_BLOCKING_NO_LEAVES`) |
| 15 | `0x0000_8000` | `ignited_by_lava` | `state.ignitedByLava()` |
| 16 | `0x0001_0000` | `requires_correct_tool_for_drops` | `state.requiresCorrectToolForDrops()` |
| 17 | `0x0002_0000` | `is_redstone_conductor` | `state.isRedstoneConductor(EmptyBlockGetter.INSTANCE, P0)` |
| 18 | `0x0004_0000` | `is_suffocating` | `state.isSuffocating(EmptyBlockGetter.INSTANCE, P0)` |
| 19 | `0x0008_0000` | `is_view_blocking` | `state.isViewBlocking(EmptyBlockGetter.INSTANCE, P0, view_probe_aabb)` |
| 20 | `0x0010_0000` | `emissive_rendering` | `state.emissiveRendering()` (no position parameter in 26.3) |
| 21 | `0x0020_0000` | `is_signal_source` | `state.isSignalSource()` |
| 22 | `0x0040_0000` | `has_analog_output_signal` | `state.hasAnalogOutputSignal()` |
| 23 | `0x0080_0000` | `has_block_entity` | `state.hasBlockEntity()` |
| 24 | `0x0100_0000` | `spawn_terrain_particles` | `state.shouldSpawnTerrainParticles()` |
| 25 | `0x0200_0000` | `is_light_permeable` | `state.isLightPermeable()` |
| 26 | `0x0400_0000` | `pathfindable_land` | `state.isPathfindable(PathComputationType.LAND)` |
| 27 | `0x0800_0000` | `pathfindable_water` | `state.isPathfindable(PathComputationType.WATER)` |
| 28 | `0x1000_0000` | `pathfindable_air` | `state.isPathfindable(PathComputationType.AIR)` |
| 29 | `0x2000_0000` | `possible_to_respawn_in` | `block.isPossibleToRespawnInThis(state)` |
| 30 | `0x4000_0000` | `exception_for_connection` | `Block.isExceptionForConnection(state)` |
| 31 | `0x8000_0000` | `can_be_replaced_by_fluid` | `state.canBeReplaced(Fluids.WATER)` |

Notes on 26.3 predicates:
- `blocks_motion_in_heightmap` / `blocks_motion_in_heightmap_no_leaves`: in 26.3, `Heightmap.Types` replaced the legacy `blocksMotion()` flag with the two block tags `#minecraft:blocks_motion_in_heightmap` and `#minecraft:blocks_motion_in_heightmap_no_leaves`. Together with `is_air` and `fluid_is_empty`, these determine all six heightmap predicates.
- `is_view_blocking`: in 26.3, `BlockStateBase#isViewBlocking` takes a third `AABB nearPlaneBox` argument (used by stairs and slabs to test intersection against their occlusion boxes, and by snow layers to test `layers >= 8`; default falls back to `isSuffocating`). Evaluated with `view_probe_aabb = [0, 0, 0]..[1, 1, 1]` at `P0 = (0, 0, 0)`, `is_view_blocking` differs from `is_suffocating` only for stairs, slabs and one snow-layer state in 26.3 (all three are view-blocking without being suffocating).

### 3. Scalars, enums, offsets, and post-process

```text
light_block              : u8           // state.getLightDampening() (0..15)
light_emission           : u8           // state.getLightEmission() (0..15)
map_color_id             : u8           // state.getMapColor(EmptyBlockGetter.INSTANCE, P0).id (0..63)
map_color_rgb            : u32          // state.getMapColor(EmptyBlockGetter.INSTANCE, P0).col (0x00RRGGBB)
push_reaction            : str          // PushReaction#name() — in 26.3: "PUSH_PULL", "PUSH", "POPPED",
                                        //   "IMMOVEABLE", or "IGNORE_ENTITY"
instrument               : str          // state.instrument().getSerializedName() (e.g. "harp", "basedrum")
render_shape             : u8           // RenderShape#ordinal(): 0 = INVISIBLE, 1 = MODEL
offset_type              : u8           // 0 = NONE, 1 = XZ, 2 = XYZ
// Present iff offset_type != 0:
offsets                  : [f64bits; 3][3]
                                        // state.getOffset(P0), state.getOffset(P1), state.getOffset(P2)
                                        // each as (x, y, z)
post_process_present     : u8           // 0 = getPostProcessPos returned null; 1 = non-null
// Present iff post_process_present == 1:
post_process_dx          : i32          // returned BlockPos.getX() - P0.x
post_process_dy          : i32          // returned BlockPos.getY() - P0.y
post_process_dz          : i32          // returned BlockPos.getZ() - P0.z
destroy_speed            : f32bits      // state.getDestroySpeed(EmptyBlockGetter.INSTANCE, P0) (hardness; -1.0 = indestructible)
explosion_resistance     : f32bits      // block.getExplosionResistance()
friction                 : f32bits      // block.getFriction()
speed_factor             : f32bits      // block.getSpeedFactor()
jump_factor              : f32bits      // block.getJumpFactor()
bounce_restitution       : f32bits      // block.getBounceRestitution()
fall_distance_reduction  : f32bits      // block.getFallDistanceReduction()
shade_brightness         : f32bits      // state.getShadeBrightness(EmptyBlockGetter.INSTANCE, P0)
```

Notes:
- `offset_type` is classified via public API: `NONE` (`0`) if `!state.hasOffsetFunction()`; otherwise `XYZ` (`2`) if `state.getOffset(pos).y != 0.0` at any probe position or `(x, 0, 0)` for `x in 0..16`, else `XZ` (`1`).
- `post_process_*`: in 26.3, `BlockStateBase#getPostProcessPos(BlockGetter, BlockPos)` returns `@Nullable BlockPos` rather than a boolean (`null`, `pos` for self, or `pos.above()` for blocks such as magma / soul sand). Storing the delta relative to `P0` preserves the exact target offset.

### 4. Sound type

```text
sound_volume             : f32bits      // soundType.getVolume()
sound_pitch              : f32bits      // soundType.getPitch()
sound_break              : str          // soundType.getBreakSound().location().toString()
sound_step               : str          // soundType.getStepSound().location().toString()
sound_place              : str          // soundType.getPlaceSound().location().toString()
sound_hit                : str          // soundType.getHitSound().location().toString()
sound_fall               : str          // soundType.getFallSound().location().toString()
```

### 5. Fluid state

```text
fluid_id                 : str          // BuiltInRegistries.FLUID.getKey(fluidState.getType()).toString()
fluid_amount             : u8           // fluidState.getAmount() (0..8)
fluid_flags              : u8           // bit 0 (0x01): falling (FlowingFluid.FALLING if present, else 0)
                                        // bit 1 (0x02): is_source (fluidState.isSource())
                                        // bit 2 (0x04): is_empty (fluidState.isEmpty())
fluid_own_height         : f32bits      // fluidState.getOwnHeight()
```

### 6. Shapes

```text
occlusion_shape          : shape        // state.getOcclusionShape() (position-independent in 26.3)
collision_shape_p0       : shape        // state.getCollisionShape(EmptyBlockGetter.INSTANCE, P0, CollisionContext.empty())
outline_shape_p0         : shape        // state.getShape(EmptyBlockGetter.INSTANCE, P0, CollisionContext.empty())
visual_shape_p0          : shape        // state.getVisualShape(EmptyBlockGetter.INSTANCE, P0, CollisionContext.empty())
interaction_shape_p0     : shape        // state.getInteractionShape(EmptyBlockGetter.INSTANCE, P0)
block_support_shape_p0   : shape        // state.getBlockSupportShape(EmptyBlockGetter.INSTANCE, P0)
shape_pos_diff_mask      : u8           // bitmask of which of the 5 position-evaluated shapes differed
                                        // across P0, P1, P2 (exact IEEE-754 bit comparison of bounds & boxes):
                                        //   bit 0 (0x01): collision_shape
                                        //   bit 1 (0x02): outline_shape
                                        //   bit 2 (0x04): visual_shape
                                        //   bit 3 (0x08): interaction_shape
                                        //   bit 4 (0x10): block_support_shape
// Present iff shape_pos_diff_mask != 0:
collision_shape_p1       : shape
outline_shape_p1         : shape
visual_shape_p1          : shape
interaction_shape_p1     : shape
block_support_shape_p1   : shape
collision_shape_p2       : shape
outline_shape_p2         : shape
visual_shape_p2          : shape
interaction_shape_p2     : shape
block_support_shape_p2   : shape
```

When `shape_pos_diff_mask == 0`, all five position-evaluated shapes at `P1` and `P2` are bit-identical to `P0` and are omitted from the stream.

### 7. Face facts

Directions follow `Direction` 3D-data / enum ordinal order:

| Bit / Index | Direction |
|---:|---|
| 0 | `down` (`-Y`) |
| 1 | `up` (`+Y`) |
| 2 | `north` (`-Z`) |
| 3 | `south` (`+Z`) |
| 4 | `west` (`-X`) |
| 5 | `east` (`+X`) |

```text
face_sturdy_full_mask    : u8           // bit d = state.isFaceSturdy(EmptyBlockGetter.INSTANCE, P0, dir[d], SupportType.FULL)
face_sturdy_center_mask  : u8           // bit d = state.isFaceSturdy(EmptyBlockGetter.INSTANCE, P0, dir[d], SupportType.CENTER)
face_sturdy_rigid_mask   : u8           // bit d = state.isFaceSturdy(EmptyBlockGetter.INSTANCE, P0, dir[d], SupportType.RIGID)
collision_face_full_mask : u8           // bit d = Block.isFaceFull(collision_shape_p0, dir[d])
face_occlusion_shapes[6] : shape        // state.getFaceOcclusionShape(dir[d]) for d = 0..6
```

---

## Size, timing and self-checks (26.3)

- `total_states` is 35,723 and equals the data generator's `blocks.json` state total; 110 states (22 plant-like blocks:
  flowers, bamboo, mangrove propagule, pointed dripstone and similar) have `shape_pos_diff_mask != 0` (only the outline
  shape for most, collision + outline + others for bamboo and dripstone). The full file is about 52 MB.
- Wall time is about 18 s, of which about 14 s is the game bootstrap and tag binding; the 35,723 records stream in
  about 4 s with bounded memory (one `DataOutputStream`, 64 KiB buffer, no per-state retention).
- The output is deterministic: two runs are byte-identical.
- A decoder reading this document must consume the file exactly (header + records, no trailing bytes); a Rust reader
  should assert that.
- Offset classification note: of the blocks with an offset function, 5 are `XYZ` and the rest `XZ`; the offset is
  evaluated at `P0`, `P1`, `P2` only, so a position-dependent shape needs the (x, y, z) offset formula implemented and
  proven against these samples and the `shape_pos_diff_mask` shapes.
