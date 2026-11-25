# Terrain chunk dump (`ZTC1`)

Format of the full-chunk dump written by the oracle agent (`tools/oracle/src/dev/zenith/oracle/ChunkDump.java`) for
the `golden-terrain` suite. The user's own vanilla dedicated server generates each requested chunk through its
real chunk pipeline up to a requested status, and the agent writes what the chunk holds at that point. The Rust
side is `xtask/src/oracle.rs` (`dump_chunks`, `parse_chunk_dump`); `cargo xtask oracle chunks` is a self-check that
prints per-chunk statistics.

## Usage

The caller creates a fresh world folder per world configuration and starts the server with:

| server.properties key | Value |
|---|---|
| `level-seed` | the world seed (decimal `i64`) |
| `level-type` | world preset id: `minecraft:normal`, `minecraft:large_biomes`, `minecraft:amplified` (also `minecraft:flat`, `minecraft:single_biome_surface`, `minecraft:debug_all_block_states`) |
| `generate-structures` | `false` for M7 terrain-only parity; `true` once structure terrain adaptation exists (M8) |

Agent arguments (comma-separated, after `-javaagent:<jar>=`): the usual `out=<zpd file>,ticks=1,seed=…,version=…`,
plus `chunks=<request file>` and `chunks_out=<output file>`. When `chunks` is present the agent generates and dumps
every requested chunk on the first server tick (on the server thread, through the level's chunk source with
"load or generate" on), then stops the server at the end of that tick.

Choosing chunks:
- 26.3 does not force-generate a spawn area at startup (only previously saved chunk tickets are reactivated), but
  finding the initial spawn point generates chunks near the world origin. Request chunks at least ~20 chunks away
  from the origin in each dimension the server opens (overworld, nether, end) unless the test is about the origin.
- Generating a chunk to `terrain` only advances its neighbours to the earlier statuses the pipeline needs, never to
  `features`, so requested chunks may be adjacent.
- Nether and end chunks are requested with dimension ids `minecraft:the_nether` and `minecraft:the_end`.

## Request file

Big-endian. `str` = `u16` byte length + UTF-8 bytes.

```text
count               : u32
requests[count]     : {
    dimension       : str     // e.g. "minecraft:overworld"
    chunk_x         : i32
    chunk_z         : i32
    status          : str     // chunk status name: "empty", "structure_starts", "structure_references",
                              // "biomes", "terrain", "features", ..., "full"
}
```

## Output file

```text
magic               : [u8; 4] = b"ZTC1"
count               : u32                    // = request count, same order
chunks[count]       : {
    dimension       : str
    chunk_x         : i32
    chunk_z         : i32
    requested       : str                    // status name as requested
    reached         : str                    // the chunk's persisted status after generation (registered id)
    min_y           : i32
    height          : i32
    section_count   : i32                    // = height / 16
    blocks[section_count * 4096] : i32       // global block-state id of every block:
                                             // sections bottom-up; inside a section index (y * 16 + z) * 16 + x
    biome_table_len : u16
    biome_table[biome_table_len] : str       // biome ids in first-seen order
    biomes[section_count * 64]   : u16       // index into biome_table; per section, (qy * 4 + qz) * 4 + qx
    heightmap_count : u8
    heightmaps[heightmap_count]  : {         // sorted by name
        name        : str                    // serialization name, e.g. "OCEAN_FLOOR_WG", "WORLD_SURFACE_WG"
        heights[256] : i32                   // first available y per column, index z * 16 + x
    }
    mark_count      : i32
    marks[mark_count] : {                    // post-processing marks, per section in list order
        section     : i32                    // section index (0 = lowest section)
        packed      : u16                    // packed local offset as the game stores it
    }
    block_ticks     : i32                    // scheduled block ticks held by the chunk (-1: not countable)
    fluid_ticks     : i32                    // scheduled fluid ticks held by the chunk (-1: not countable)
}
```

Notes:
- Biome values are what the chunk *stores* (after the biomes step), read per section at quart resolution.
- At the `terrain` status 26.3 chunks hold all six heightmap types (`MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`,
  `OCEAN_FLOOR`, `OCEAN_FLOOR_WG`, `WORLD_SURFACE`, `WORLD_SURFACE_WG`).
- The world folder (game-generated data) is deleted by the Rust side after reading; nothing is kept in the repo.

## Self-check (2026-11-25, seed 12345, structures off, `terrain`)

8 chunks in 9 s. Overworld chunks (200,200), (201,200), (−300,450), (1000,−1000): bedrock floor, deepslate below
stone, dirt/grass surface, lava pockets at depth, water post-processing marks (0–27 per chunk), biomes such as
`snowy_plains`, `snowy_taiga` + `dripstone_caves`, `snowy_beach`. Nether (100,100), (−50,80): netherrack, soul sand /
soul soil, lava, bedrock roof and floor. End (0,0), (100,100): end stone only, no bedrock. No tree or other feature
blocks, as expected before the features step.
