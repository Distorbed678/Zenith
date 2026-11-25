# Terrain probe binary format (`ZTP1`)

Specification for `dev.zenith.oracle.TerrainProbe` (`tools/oracle/src/dev/zenith/oracle/TerrainProbe.java`), the
parity oracle for milestone M7 (terrain generation: density functions, noise routing, climate quantization, biome
selection and zoom, and base terrain columns).

The Rust parity harness writes an input file, invokes `TerrainProbe` on the user's game classpath from a cache
working directory (so the game's logger does not write `logs/` into the repo), and compares the resulting `ZTP1`
output file bit-for-bit against Zenith's own implementation.

---

## Invocation

```text
java -Xmx4G -cp <game_classpath>:<zenith-oracle.jar> dev.zenith.oracle.TerrainProbe <input_file> <output_file> <mode>
```

Valid `<mode>` values:

| Mode string | `mode_id` | Purpose |
|---|---|---|
| `router` | `1` | Uncached point samples of all 8 `NoiseRouter` fields and (if present) all 6 `Aquifer.Config` fields |
| `router-volume` | `2` | Per-chunk caching volume samples of `final_density` (`16×H×16`) and all 6 climate channels (`4×(H/4)×4`) |
| `climate` | `3` | Quantized `Climate.TargetPoint` (`6 × i64`) per quart position via the uncached climate sampler |
| `biome` | `4` | Uncached quart biomes, obfuscated zoom seed, and Voronoi-zoomed block biomes |
| `column` | `5` | Base heightmaps (`4 × i32`) and full vertical base column block-state global IDs per `(x, z)` |

The tool exits with a non-zero status and an error message on `stderr` if `<mode>` is unknown, any identifier or
biome selector is unknown, JSON decoding fails, a count is negative/overflowing, or the input file is truncated or
has unexpected trailing bytes.

---

## Primitive encodings

All multi-byte values are **big-endian** (matching `java.io.DataInputStream` / `DataOutputStream` and `ZPD1` in
[`dump-format.md`](./dump-format.md)).

| Type | Size | Encoding |
|---|---|---|
| `u8` | 1 byte | Unsigned 8-bit integer (`0..255`) |
| `u16` | 2 bytes | Unsigned 16-bit big-endian integer (`0..65535`) |
| `i32` | 4 bytes | Signed 32-bit two's-complement big-endian integer |
| `u32` | 4 bytes | Unsigned 32-bit big-endian integer (counts are restricted to `0..2^31 - 1`) |
| `i64` | 8 bytes | Signed 64-bit two's-complement big-endian integer |
| `f32bits` | 4 bytes | Raw IEEE-754 binary32 bit pattern as big-endian `u32` (`Float.floatToRawIntBits` / `f32::to_bits`); preserves `-0.0` and NaN payloads |
| `str` | `2 + len` bytes | `u16 byte_len` followed by `byte_len` bytes of standard UTF-8 (not Java modified UTF-8) |
| `bytes` | `4 + len` bytes | `u32 byte_len` followed by `byte_len` raw bytes (standard UTF-8 when carrying JSON text) |

### Common input header

Every input file starts with:

```text
seed      : i64   // world seed
selector  : str   // noise_settings identifier (modes 1, 2, 3, 5) or biome selector (mode 4)
```

For `noise_settings` identifiers (`modes 1, 2, 3, 5`), `Identifier.tryParse` is used, so both canonical
(`"minecraft:overworld"`) and default-namespace shorthand (`"overworld"`) resolve against
`minecraft:worldgen/noise_settings`. Built-in 26.3 IDs:

- `minecraft:overworld`
- `minecraft:large_biomes`
- `minecraft:amplified`
- `minecraft:nether`
- `minecraft:end`
- `minecraft:caves`
- `minecraft:floating_islands`

### Common output prefix

Every output file starts with 5 bytes:

```text
magic    : [u8; 4] = b"ZTP1"   // 0x5A 0x54 0x50 0x31
mode_id  : u8                  // 1..5 matching the table above
```

All outputs are streamed record-by-record in input order with bounded memory.

---

## Mode 1: `router` (`mode_id = 1`)

Evaluates the 8 `NoiseRouter` density functions and, if `settings.aquifers()` is present, the 6 `Aquifer.Config`
density functions at each input block coordinate `(x, y, z)` via
`RandomState#sampleBlockValueUncached(DensityFunction, int, int, int)` (which compiles each function once and
samples with `SamplerContext.EMPTY_UNCACHED`).

### Input layout

```text
seed                  : i64
noise_settings_id     : str
point_count           : u32
points[point_count]   : {
    x : i32
    y : i32
    z : i32
}
```

### Output layout

```text
magic                 : [u8; 4] = b"ZTP1"
mode_id               : u8 = 1
has_aquifers          : u8        // 0 = settings.aquifers() absent; 1 = present
point_count           : u32
records[point_count]  : {
    // 8 NoiseRouter fields (always present, 32 bytes):
    temperature                       : f32bits
    vegetation                        : f32bits
    continents                        : f32bits
    erosion                           : f32bits
    depth                             : f32bits
    ridges                            : f32bits
    chunk_surface_level               : f32bits
    final_density                     : f32bits

    // 6 Aquifer.Config fields (present iff has_aquifers == 1, +24 bytes = 56 bytes/record):
    aquifer_barrier                   : f32bits
    aquifer_fluid_level_floodedness   : f32bits
    aquifer_fluid_level_spread        : f32bits
    aquifer_lava                      : f32bits
    aquifer_exclusion                 : f32bits
    aquifer_surface_level             : f32bits
}
```

---

## Mode 2: `router-volume` (`mode_id = 2`)

Evaluates per-chunk volumes through the caching volume path (`SamplerContext.builder().enableCaches().build()`)
matching both chunk generation volume shapes in 26.3:

1. **`final_density`** (matches `NoiseBasedChunkGenerator#doFill`):
   - Fresh caching `SamplerContext` per chunk.
   - `DensityVolume(16, height, 16, chunk_x << 4, min_y, chunk_z << 4)` (steps `1, 1, 1`).
   - Sampled via `randomState.getSampler(router.finalDensity()).sampleVolume(ctx, buf, volume)`.
   - Element count: `final_len = 16 * height * 16`.
2. **6 climate fields** (matches `MultiNoiseBiomeSource#createResolverForChunk`):
   - Fresh caching `SamplerContext` per chunk, wrapped in `randomState.createClimateSampler(ctx)`.
   - `DensityVolume(4, height / 4, 4, chunk_x << 4, min_y, chunk_z << 4, 4, 4, 4)` (steps `4, 4, 4`).
   - Sampled via `DensitySampler.Bound#sampleVolume(buf, climateVolume)` in exact order:
     `temperature`, `humidity` (`vegetation`), `continentalness` (`continents`), `erosion`, `depth`, `weirdness`
     (`ridges`).
   - Element count per channel: `climate_len = 4 * (height / 4) * 4`.

### Buffer index order

Every dumped volume array uses `DensityVolume`'s flat layout — **Z outer, X middle, Y inner (contiguous)**:

```text
index(ix, iy, iz) = iy + (ix + iz * size_x) * size_y
```

- For `final_density`: `ix ∈ [0, 16)`, `iy ∈ [0, height)`, `iz ∈ [0, 16)` at block coordinate
  `(chunk_x * 16 + ix, min_y + iy, chunk_z * 16 + iz)`.
- For each climate channel: `ix ∈ [0, 4)`, `iy ∈ [0, height / 4)`, `iz ∈ [0, 4)` at block coordinate
  `(chunk_x * 16 + ix * 4, min_y + iy * 4, chunk_z * 16 + iz * 4)`.

### Input layout

```text
seed                  : i64
noise_settings_id     : str
chunk_count           : u32
chunks[chunk_count]   : {
    chunk_x : i32
    chunk_z : i32
}
```

### Output layout

```text
magic                 : [u8; 4] = b"ZTP1"
mode_id               : u8 = 2
min_y                 : i32       // NoiseSettings.minY()
height                : i32       // NoiseSettings.height() (positive multiple of 16)
final_size_x          : u32       // 16
final_size_y          : u32       // height
final_size_z          : u32       // 16
climate_size_x        : u32       // 4
climate_size_y        : u32       // height / 4
climate_size_z        : u32       // 4
chunk_count           : u32
chunks[chunk_count]   : {
    final_density[final_size_x * final_size_y * final_size_z]       : f32bits[]
    temperature[climate_size_x * climate_size_y * climate_size_z]   : f32bits[]
    vegetation[climate_size_x * climate_size_y * climate_size_z]    : f32bits[]
    continents[climate_size_x * climate_size_y * climate_size_z]    : f32bits[]
    erosion[climate_size_x * climate_size_y * climate_size_z]       : f32bits[]
    depth[climate_size_x * climate_size_y * climate_size_z]         : f32bits[]
    ridges[climate_size_x * climate_size_y * climate_size_z]        : f32bits[]
}
```

---

## Mode 3: `climate` (`mode_id = 3`)

Evaluates quantized climate `TargetPoint`s at input quart coordinates `(qx, qy, qz)` via
`randomState.createClimateSampler(SamplerContext.EMPTY_UNCACHED).sample(qx, qy, qz)`.

### Input layout

```text
seed                  : i64
noise_settings_id     : str
quart_count           : u32
quarts[quart_count]   : {
    qx : i32
    qy : i32
    qz : i32
}
```

### Output layout

```text
magic                 : [u8; 4] = b"ZTP1"
mode_id               : u8 = 3
quart_count           : u32
records[quart_count]  : {
    temperature      : i64
    humidity         : i64
    continentalness  : i64
    erosion          : i64
    depth            : i64
    weirdness        : i64
}
```

---

## Mode 4: `biome` (`mode_id = 4`)

Resolves uncached quart biomes via `BiomeSource#createUncachedResolver(RandomState)` and Voronoi-zoomed block
biomes via `new BiomeManager(resolver, BiomeManager.obfuscateSeed(seed)).getBiome(bx, by, bz)`.

### Biome-source selectors

| `selector` value | Aliases | `BiomeSource` | `NoiseGeneratorSettings` |
|---|---|---|---|
| `overworld` | `minecraft:overworld`, `normal`, `minecraft:normal` | `WorldPreset(minecraft:normal)` → `LevelStem.OVERWORLD` | `minecraft:overworld` |
| `large_biomes` | `minecraft:large_biomes` | `WorldPreset(minecraft:large_biomes)` → `LevelStem.OVERWORLD` | `minecraft:large_biomes` |
| `amplified` | `minecraft:amplified` | `WorldPreset(minecraft:amplified)` → `LevelStem.OVERWORLD` | `minecraft:amplified` |
| `nether` | `minecraft:nether`, `the_nether`, `minecraft:the_nether` | `WorldPreset(minecraft:normal)` → `LevelStem.NETHER` | `minecraft:nether` |
| `end` | `minecraft:end`, `the_end`, `minecraft:the_end` | `WorldPreset(minecraft:normal)` → `LevelStem.END` | `minecraft:end` |
| `json` | — | Decoded from `biome_source_json` via `BiomeSource.CODEC` | `minecraft:overworld` |
| `json:<id>` | — | Decoded from `biome_source_json` via `BiomeSource.CODEC` | `<id>` |

JSON selectors use `StrictJsonParser` and `BiomeSource.CODEC` with the world lookup's `JsonOps` serialization
context (supporting `minecraft:fixed`, `minecraft:checkerboard`, `minecraft:multi_noise`, and `minecraft:the_end`).

> **Note on `Climate.RTree` warm-start state**: Section 1 (`quarts`) and Section 2 (`blocks`) run sequentially on
> the same thread over the same `resolver` instance. Any tie-breaking that depends on `RTree`'s thread-local
> `lastResult` therefore carries across Section 1 into Section 2; pass `quart_count = 0` to probe Section 2 from a
> fresh `lastResult = null`.

### Input layout

```text
seed                  : i64
selector              : str
biome_source_json     : bytes     // PRESENT IFF selector == "json" OR selector.starts_with("json:")
quart_count           : u32
quarts[quart_count]   : {
    qx : i32
    qy : i32
    qz : i32
}
block_count           : u32
blocks[block_count]   : {
    bx : i32
    by : i32
    bz : i32
}
```

### Output layout

```text
magic                     : [u8; 4] = b"ZTP1"
mode_id                   : u8 = 4
obfuscated_seed           : i64       // BiomeManager.obfuscateSeed(seed)
quart_count               : u32
quart_biomes[quart_count] : str       // Holder.getRegisteredName(), e.g. "minecraft:plains"
block_count               : u32
block_biomes[block_count] : str       // zoomed biome at (bx, by, bz)
```

---

## Mode 5: `column` (`mode_id = 5`)

Builds a `NoiseBasedChunkGenerator` with the requested `NoiseGeneratorSettings` and its matching dimension biome
source (`minecraft:nether` → Nether multi-noise, `minecraft:end` → End, all others → Overworld multi-noise), binds
built-in block tags from the vanilla server data pack so heightmap tag predicates match a real server, and for
each `(x, z)` dumps:

1. `NoiseBasedChunkGenerator#getBaseHeight(x, z, type, heightAccessor, randomState)` for 4 `Heightmap.Types` in
   fixed order:
   - Index 0: `OCEAN_FLOOR_WG`
   - Index 1: `WORLD_SURFACE_WG`
   - Index 2: `MOTION_BLOCKING`
   - Index 3: `MOTION_BLOCKING_NO_LEAVES`
2. `NoiseBasedChunkGenerator#getBaseColumn(x, z, heightAccessor, randomState)` as `Block.getId(col.getBlock(y))`
   for `y = min_y` up to `min_y + height - 1` inclusive (ascending Y).

Here `heightAccessor = LevelHeightAccessor.create(min_y, height)`. Note that `getBaseColumn` / `getBaseHeight`
reflect pre-surface `iterateNoiseColumn` output (terrain density + aquifer + default block/fluid; bedrock and
surface material replacement are applied later by `buildSurface`).

### Input layout

```text
seed                    : i64
noise_settings_id       : str
column_count            : u32
columns[column_count]   : {
    x : i32
    z : i32
}
```

### Output layout

```text
magic                     : [u8; 4] = b"ZTP1"
mode_id                   : u8 = 5
min_y                     : i32       // NoiseSettings.minY()
height                    : i32       // NoiseSettings.height()
heightmap_count           : u8 = 4
column_count              : u32
records[column_count]     : {
    height_ocean_floor_wg            : i32
    height_world_surface_wg          : i32
    height_motion_blocking           : i32
    height_motion_blocking_no_leaves : i32
    blocks[height]                   : i32[]   // Block.getId at y = min_y + 0 .. min_y + height - 1
}
```
