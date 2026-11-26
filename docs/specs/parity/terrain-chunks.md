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
    start_count     : i32
    starts[start_count] : {                  // structure starts held by this chunk, sorted by structure id
        structure   : str                    // e.g. "minecraft:mineshaft"
        start_chunk_x : i32
        start_chunk_z : i32
        references  : i32                    // the start's reference counter
        valid       : u8                     // 1 = a real start
        bounds[6]   : i32                    // min x, y, z, max x, y, z (inclusive)
        piece_count : i32
        pieces[piece_count] : {              // in the start's own list order
            kind    : str                    // structure piece type id, e.g. "minecraft:msroom"
            bounds[6] : i32
            orientation : str                // "north", "south", "west", "east", ... or "" when none
            depth   : i32                    // generation depth
        }
    }
    reference_count : i32
    references[reference_count] : {          // sorted by structure id
        structure   : str
        count       : i32
        starts[count] : i64                  // packed start-chunk positions, ascending: x in the low 32 bits,
                                             // z in the high 32 bits
    }
}
```

Notes:
- Biome values are what the chunk *stores* (after the biomes step), read per section at quart resolution.
- At the `terrain` status 26.3 chunks hold all six heightmap types (`MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`,
  `OCEAN_FLOOR`, `OCEAN_FLOOR_WG`, `WORLD_SURFACE`, `WORLD_SURFACE_WG`).
- The world folder (game-generated data) is deleted by the Rust side after reading; nothing is kept in the repo.

## Optional sections (`ZTC2`)

Agent argument `chunks_extra=<bitmask>` (xtask `oracle::dump_chunks_with`, `cargo xtask oracle chunks --extras N`)
adds sections; with any bit set the file starts with `ZTC2` and a `u8` copy of the bitmask follows the chunk count.
Each chunk record then ends, after the references section, with the selected sections in bit order:

```text
// bit 1 — light (useful at status "light" / "full"):
light_section_count : i32                 // from the level's min to max light section (one beyond the chunk each side)
light_sections[...] : {
    y        : i32                        // section y
    block    : u8 present, [u8; 2048] if present   // raw nibble array (low nibble = even index)
    sky      : u8 present, [u8; 2048] if present
}
// bit 2 — block entities, sorted by (y, z, x):
count       : i32
entries[count] : { x: i32, y: i32, z: i32, nbt_len: i32 (-1 = none), nbt[nbt_len] }   // network NBT (type byte + payload)
// bit 4 — entities held by a proto chunk, in list order:
count       : i32                         // -1: the chunk keeps none (a full chunk; entities live in the level)
entries[count] : { nbt_len: i32, nbt[nbt_len] }
```

Self-check (2026-11-26, seed 12345, structures on): full overworld chunks report light sections −5…20 with block light up
to 15; a nether chunk has block layers and no sky layers; a mineshaft area at `features` holds spawner block entities
and chest-minecart entities.

## Structure starts and references

Every chunk record ends with the chunk's structure starts and references, so structure placement can be verified on
its own (request `structure_starts` / `structure_references`). With `generate-structures=false` both lists are empty.
Self-check (seed 12345, structures on): chunk (104,104) at `structure_references` references `minecraft:mineshaft`
from (108,105); chunk (108,105) at `structure_starts` holds that mineshaft start (box 1675,−10,1609 … 1816,17,1737,
157 pieces, the first an `msroom` with no orientation at depth 0).

## Self-check (2026-11-25, seed 12345, structures off, `terrain`)

8 chunks in 9 s. Overworld chunks (200,200), (201,200), (−300,450), (1000,−1000): bedrock floor, deepslate below
stone, dirt/grass surface, lava pockets at depth, water post-processing marks (0–27 per chunk), biomes such as
`snowy_plains`, `snowy_taiga` + `dripstone_caves`, `snowy_beach`. Nether (100,100), (−50,80): netherrack, soul sand /
soul soil, lava, bedrock roof and floor. End (0,0), (100,100): end stone only, no bedrock. No tree or other feature
blocks, as expected before the features step.
