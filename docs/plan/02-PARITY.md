# 02: Parity: proving 1:1

## Definition of 1:1 (owner decisions 5.1, 5.2)

| Aspect | Requirement |
|--------|-------------|
| Gameplay | Identical game state tick by tick: blocks, entities, items, RNG streams, scheduled ticks, scores. |
| Visuals | Pixel-exact: the same settings give the same frame. Tolerance ≤ 1 step per channel, only where GPU rasterization rules legitimately differ between vendors; the reference is vanilla running on the same machine. |
| Bugs | Every observable behavior, including gameplay bugs (update order quirks, quasi-connectivity, item duplication mechanics, etc.). **Not** replicated: crashes, memory leaks, pure performance bugs. |
| Formats | Saves, packets, options and data/resource packs are interchangeable with vanilla in both directions. |
| Extras | None in the base game. Extras come from mods. |
| Internals | Free to differ (threading, data layout, GPU techniques) as long as the above holds. |

A subsystem is **done** only when its parity suites pass at 100% on the reference corpus.

## The parity toolchain

```
             ┌───────────────── vanilla (user's own jar, Java 25) ─────────────────┐
 scenario ──►│  + tools/oracle Java agent: hooks tick/worldgen/network, writes dumps │──► vanilla dump
  (seed,     └──────────────────────────────────────────────────────────────────────┘        │
  inputs,                                                                                   ▼
  packs)     ┌──────────────────────── Zenith (same scenario) ──────────────────────┐    zenith-parity differ
        ───► │  built-in dump hooks (feature `parity-dump`), same dump schema        │──► zenith dump ──► report
             └──────────────────────────────────────────────────────────────────────┘     │
                                                                                         ▼
                                                     progress/parity/<suite>.json  →  tracker
```

### 1. Parity oracle (Java agent): `tools/oracle/`

- Our own Java code, built locally against the user's jar **as a compile-time-only dependency**. It is never
  committed with game code, and never distributed. The guard allows `.java` only in this folder.
- Loaded with `-javaagent:` into the vanilla client or server started from the user's install. It instruments chosen
  methods with the JDK's own class-file API (`java.lang.classfile`, Java 24+; no third-party libraries) to emit
  **state dumps**:
  - per tick: RNG states (world random, per-entity randoms), block changes, scheduled tick queues, entity
    positions/velocities/AI goals/memories, light updates, scoreboard;
  - worldgen: per-chunk block palettes, heightmaps, biomes, structure starts, feature placement log;
  - network: every packet (state, ID, bytes) in both directions;
  - render: deterministic screenshot capture at fixed camera poses (client only).
- The dump format is a versioned, length-prefixed binary stream with zstd compression (`zenith-parity::dump`). The same
  writer API exists on the Zenith side behind the `parity-dump` cargo feature.
- Hook points are found from the unobfuscated jar's class and method signatures (javap). Hook descriptors live in
  `tools/oracle/hooks/<version>.properties`. These are names and signatures only, which are facts needed for
  interoperability. The agent compiles without the game on its classpath and reads state through public methods
  reflectively. **Status (M1):** built and verified. It runs the user's own 26.3 dedicated server for N ticks; two runs
  with the same seed produce identical dumps (the determinism baseline).

### 2. Differ: `zenith-parity`

- Streams both dumps, aligns them by (tick, phase, subject), and reports the **first divergence** with full context:
  the field path, both values, the preceding N events, and the RNG call counts.
- Supports tolerance rules **only** for the screenshot suite (per-pixel channel delta ≤ 1 where it's a rasterization
  difference).
- Writes `progress/parity/<suite>.json`: `{suite, version, passed, total, updatedAt}`. The tracker turns those into
  percentages.

## Suites (all owner-approved in rounds 7–8)

| Suite id | Milestone | What it checks | Corpus |
|----------|-----------|----------------|--------|
| `datagen-ingest` | M1 | Ingested tables equal data-gen reports | Every report file |
| `nbt-roundtrip` | M2 | Zenith's read→write of each blob equals **vanilla's own read→write of the same bytes** (not the original file: build-pipeline files were written in a different key order). Rejections must match too | Jar structure templates + oracle worlds + local saves (≈47k blobs, read at run time) |
| `snbt-format` | M2 | Zenith's single-line SNBT text equals vanilla's `toString()` for every corpus blob (parser: oracle differential fuzzer, pending) | Same corpus as `nbt-roundtrip` |
| `anvil-io` | M3 | Zenith copies real region files; vanilla reads them back with equal chunks; sector layouts and bytes match vanilla's own copy (same machine: same zlib, write granularity and deflate call pattern) | 48 region files from oracle worlds + local saves |
| `registry-ids` | M4 | Every registry entry has the same numeric ID | All registries |
| `blockstate-ids` | M4 | Every block state ↔ global ID | All states |
| `datapack-load` | M5 | All vanilla data parses; decoded→re-encoded matches | Whole data/ tree from the jar |
| `text-component` | M5 | Decode status, JSON + network-NBT re-encode, NBT→JSON decode, plain + truncated flattening (`TextCheck`, full registry access) | Hand-made edge cases + every translation key (bare and with arguments) + every component-like value in the jar's data files |
| `rng-streams` | M6 | RNG outputs identical for seeds × call patterns | Property-based |
| `noise-samples` | M6 | Noise samplers identical at random points | 1M samples / noise |
| `worldgen-router` | M7 | Every noise-router field and aquifer-config function at points (uncached path), final-density and climate chunk volumes (caching path), raw `f32` bits (`TerrainProbe`) | 7 noise settings × 3 seeds × 134 points; 2 seeds × 3 chunks |
| `worldgen-biomes` | M7 | Quantized climate target points; chosen biome per quart and zoomed biome per block for every built-in source and hand-made fixed / checkerboard / tied multi-noise sources; seed obfuscation (`TerrainProbe`) | 7 settings × 3 seeds × 317 quarts; 9 sources × 4 seeds × (317 quarts + 314 blocks) |
| `worldgen-columns` | M7 | The 4 base heights and every block-state id of the base column (density + aquifer + default block/fluid, before surface and carvers) per (x, z) (`TerrainProbe` column mode) | 7 noise settings × 3 seeds × 61 columns |
| `block-facts` | M8 | Every block state's static facts vs the game (`BlockFacts`): identity, 32 flags, numbers, sound, fluid state, six shape kinds (box lists in order + bounds), position-dependent shapes, face sturdiness / fullness / occlusion | All 35,723 states × 15 field groups |
| `golden-terrain` | M7 | Blocks, biomes, heightmaps per chunk | 10k chunks × 50 seeds × 3 dimensions |
| `golden-features` | M8 | Feature placement incl. block entities | Same corpus |
| `golden-structures` | M8 | Structure starts/pieces/contents | Same corpus + forced structure seeds |
| `light-engine` | M9 | Light values after gen and after edits | Corpus + scripted edits |
| `tick-oracle` | M10 | Per-tick world state over scripted scenarios | Scenario library |
| `redstone-contraptions` | M10 | Known-quirky machines produce identical timing traces | Community-style test worlds we build ourselves |
| `entity-physics` | M11 | Positions/velocities per tick | Scenario library |
| `mob-ai-oracle` | M12 | AI decisions, path choices, memories | Scenario library |
| `spawning` | M12 | Spawn attempts and results | Seeds × biomes |
| `menus` | M13 | Click/drag/shift semantics per container | Generated click sequences |
| `recipes` | M13 | Every recipe's matching + output | All recipes |
| `command-tree` | M14 | The command graph and suggestions are identical | Full tree dump |
| `functions` | M14 | Function execution traces | Scenario library |
| `loot` | M15 | Loot rolls given RNG state | All loot tables × contexts |
| `advancements` | M15 | Trigger evaluation | Scenario library |
| `packet-codec` | M16 | Each packet encodes/decodes identically | Recorded corpus |
| `packet-replay` | M16 | Zenith server/client response streams equal vanilla's | Recorded sessions |
| `resource-load` | M18 | Models, atlases, fonts load identically (atlas layout incl.) | Default resources + test packs |
| `screenshot-diff` | M19 | Frames at fixed poses | Camera path library |
| `screenshot-diff-gui` | M20 | Every screen at multiple GUI scales | Screen list |
| `input-replay` | M22 | Recorded input sessions replay to the same state + frames | Session library |

Plus **fuzzing** (cargo-fuzz) for every parser: NBT, SNBT, region files, packets, JSON codecs, resource-pack loaders.
Fuzzing finds crashes; parity finds divergences.

## Scenario library

`tests/scenarios/` holds scenario definitions that we write ourselves (TOML + our own small test worlds, built by
scripts from vanilla commands, so no game files are committed):

```toml
[scenario]
id = "redstone/qc-piston-basic"
seed = 12345
world = "flat"              # generated at run time
ticks = 200
setup = ["setblock ~ ~ ~ ...", "..."]   # commands run in both engines
inputs = "inputs/none.zin"   # recorded input stream (client scenarios)
dump = ["blocks", "scheduled", "entities"]
```

## CI vs local

- The parity suites need the game files. Locally, they use the developer's install. **CI downloads the official jar
  and asset index directly from Mojang's servers at run time** (public URLs from the version manifest, verified by
  SHA-1), uses them only inside the CI job, and never caches them into artifacts that get published.
- The Java oracle needs Java 25 in CI (`actions/setup-java`, Temurin 25).
- Heavy corpora (golden worldgen across 50 seeds) run nightly; PRs run a smaller smoke corpus.

## Determinism rules for Zenith code (enforced in review + lints)

1. No `HashMap` iteration where order is observable. Use `IndexMap`/sorted maps, or match vanilla's hash iteration
   order exactly where vanilla iterates a hash set (some vanilla behavior depends on Java's `HashMap` bucket order, so
   `zenith-math` provides a Java-compatible hash map for those spots).
2. Every RNG call happens in the same order and with the same bounds as vanilla. RNG objects are passed explicitly.
3. FP rules from [01](01-ARCHITECTURE.md#floating-point-and-integer-semantics).
4. Parallel work must merge in vanilla order. Each parallel site carries a comment naming the parity suite that covers
   it.
