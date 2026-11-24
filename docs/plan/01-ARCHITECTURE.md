# 01: Architecture

## Goals that shape the design

1. **Determinism.** Given the same seed, inputs and data packs, Zenith produces bit-identical game state to vanilla.
   This drives every decision about threading, floating point, iteration order and RNG.
2. **Speed without divergence.** Parallelism is used only where it can't change observable order, or where results are
   merged back in vanilla's exact order.
3. **Version isolation.** Version-specific behavior is isolated, so supporting a new version means adding a layer, not
   editing everything.
4. **Runtime-sourced content.** Nothing from the game lives in the repo. Content comes from the user's install, through
   ingestion, into a per-version cache.

## Workspace layout (target)

Crates are added when their milestone starts. Names are `zenith-<area>`.

```
crates/
  zenith-install      M0   find installs, local ownership proof                         ✅ exists
  zenith-data         M1   data-gen ingestion → versioned binary tables (blocks, registries, packets)
  zenith-parity       M1   dump format, differ, suite runner, results → progress/parity
  zenith-nbt          M2   NBT/SNBT, network NBT, modified UTF-8
  zenith-storage      M3   Anvil regions, level.dat, player data, session lock, io backends
  zenith-registry     M4   identifiers, registries, block states, tags, feature flags
  zenith-codec        M5   codec framework, data packs, text components, data components
  zenith-math         M6   Java-exact math, RNGs, noise
  zenith-worldgen     M7-8 density functions, biomes, surface, carvers, features, structures
  zenith-light        M9   light engines
  zenith-world        M10  level, chunks, tickets, ticking, block updates, redstone, fluids
  zenith-entity       M11-12 entities, physics, AI, pathfinding, spawning
  zenith-item         M13  items, inventories, menus, recipes, enchanting
  zenith-command      M14  command graph, arguments, functions, chat
  zenith-rules        M15  loot, advancements, stats, scoreboard
  zenith-server       M16  server loop, player management (lib) + `zenith-server` binary
  zenith-protocol     M16  packets, states, framing, compression, encryption
  zenith-platform     M17  SDL3 window/input, OS integration
  zenith-gpu          M17  ash Vulkan 1.3 device, memory, frame graph, shader pipeline
  zenith-resources    M18  resource packs, models, atlases, fonts, lang, sounds index
  zenith-render       M19  world/entity/particle/sky rendering
  zenith-gui          M20  widgets, screens, HUD, text
  zenith-audio        M21  3D audio, sound events, music
  zenith-client       M22  client game loop, prediction, integrated server glue  + `zenith` binary
  zenith-auth         M23  Microsoft/Xbox/Minecraft-services auth, keyring
  zenith-mod-host     M24  wasmtime host, WIT API, permissions, hot reload
  zenith-sdk          M24  mod-author crate (guest side)
  zenith-mod-index    M25  index client, signatures, PubGrub resolver, lockfiles, .zpack/.mrpack
  zenith-version      M30  version layer: per-version adapters + feature gates
xtask/                     repo automation (progress tracker, guard, later: datagen, diffbot, perf)
launcher/                  M26 Tauri 2 app (Rust backend + Svelte 5 front-end on packages/ui)
packages/ui/               shared Svelte 5 design system (launcher + website)
web/                       SvelteKit static website
tools/oracle/              parity oracle Java agent (our own code; built locally; never ships)
```

### Dependency direction

```
            ┌─────────────── zenith-client (binary) ───────────────┐
            │   zenith-render  zenith-gui  zenith-audio            │
            │   zenith-resources  zenith-gpu  zenith-platform      │
            └──────────────┬───────────────────────────────────────┘
                           │ uses (integrated server)
            ┌──────────────▼──────────────── zenith-server ────────┐
            │  zenith-protocol   zenith-command   zenith-rules     │
            │  zenith-entity  zenith-item  zenith-world  zenith-light│
            │  zenith-worldgen                                       │
            └──────────────┬────────────────────────────────────────┘
                           ▼
     zenith-codec → zenith-registry → zenith-storage → zenith-nbt → zenith-math
                           ▲
               zenith-data (ingested tables)  ◄── zenith-install (where the game files are)
```

Rules: no cycles; the server side never depends on client crates; `zenith-mod-host` depends on the server and client
API crates through narrow traits only.

## Runtime content pipeline

```
user's install ──(zenith-install: locate + prove ownership)──► jar + assets + Java runtime path
     │
     ├─ first run per version: run the vanilla data generator from the user's jar with the user's launcher-provided
     │  Java 25 → reports (blocks, registries, packets, commands, items, data-pack JSON) in ~/.cache/zenith/<ver>/reports
     │
     ├─ zenith-data compiles the reports into compact, versioned binary tables (mmap-friendly) in
     │  ~/.cache/zenith/<ver>/tables.bin, keyed by jar SHA-1, so a changed jar invalidates the cache
     │
     └─ at runtime: data packs, resource packs, assets and lang are read directly from the jar and the asset objects
        (zip reader + content-addressed object store), with an in-memory index
```

Java is needed **only** for the one-time ingestion step and for the (developer-only) parity oracle. Players never need
a JVM to play: Zenith finds the Java runtime the launcher already installed (e.g. Prism's `java-runtime-epsilon`).
Fallback: a JVM-free extraction path for everything that is plain data in the jar (most data packs/assets), with
the code-defined tables (block properties, packet IDs) cached after the first successful ingestion.

## Threading model

Vanilla runs a server thread (20 TPS) plus a render thread, with worker pools for chunk generation, lighting and
meshing. Zenith keeps the **same logical schedule**, because observable order matters, but executes it faster:

- **Server tick:** single logical thread for every step whose order is observable (block updates, entity ticks,
  scheduled ticks, redstone). Inside a step, work that is provably independent (e.g. per-section random-tick sampling
  with pre-drawn RNG values) can fan out. Results are applied in vanilla order.
- **Chunk pipeline:** chunk statuses (structure starts → biomes → noise → surface → carvers → features → light → full)
  run on a work-stealing pool. Vanilla is also parallel here, but its final results are deterministic per chunk, given
  neighbor-dependency rules. Zenith mirrors the dependency radius per status, so results match.
- **Client:** main thread (input/UI), render thread (Vulkan submission), and a mesh worker pool. Meshes are built off
  thread and uploaded through persistent staging. Frame pacing mirrors vanilla's tick/partial-tick interpolation.
- **Determinism tooling:** a `--deterministic-schedule` debug mode serializes everything for oracle diffs.

## Data-oriented core

- Block states are `u32` global IDs (identical to vanilla's palette IDs), with per-state property tables in
  `zenith-data`.
- Chunk sections use **paletted containers** with the same bit-packing as vanilla, because the network format and
  save format require them. Internally, hot paths may unpack to dense arrays.
- Entities: a hybrid. Vanilla semantics are inheritance-heavy, but Zenith uses an ECS-like storage (archetype tables)
  with per-type behavior tables. Iteration order must match vanilla's entity tick order, so each world keeps an
  explicit ordered entity list alongside the component storage.

## Floating point and integer semantics

Java's `float`/`double` are IEEE-754 with strict semantics (since Java 17, all FP is strict). Rust's `f32`/`f64` match
when we:

- avoid fused multiply-add unless Java would use it (`Math.fma` only where vanilla calls it); never let LLVM contract
  (Rust doesn't contract by default, so we keep it that way and lint against `mul_add`);
- reimplement Java's `Math`/`StrictMath` functions where vanilla relies on exact results (`sin`/`cos` tables, `atan2`,
  `sqrt` is exact IEEE in both, `pow`/`exp`/`log` must match `StrictMath` (fdlibm) when vanilla uses StrictMath);
- match Java integer overflow semantics (wrapping) and `/` and `%` truncation, plus `floorDiv`/`floorMod` helpers;
- match `(int)` casts of floats (saturating, NaN → 0; Rust `as` matches this since 1.45).

`zenith-math` owns all of this, with exhaustive property tests against the oracle.

## Version layer (summary; full design in 03)

- `zenith-data` tables are versioned by jar SHA-1.
- Behavior differences between versions live in `zenith-version` adapters, selected at startup, as trait objects or
  enum dispatch over the narrow set of things that change.
- Protocol handling per version: packet ID maps come from ingestion; packet *shapes* that change get per-version codec
  modules.

## Configuration and files

- Player-facing files keep vanilla's names and formats (`options.txt`, `servers.dat`, worlds), so instances are
  interchangeable with vanilla.
- Zenith-specific settings live in `zenith.toml` in the instance directory (mods, launcher integration only; never
  gameplay or visual changes).
- Cache: `~/.cache/zenith/` (Linux), `%LOCALAPPDATA%\zenith\cache` (Windows), `~/Library/Caches/zenith` (macOS).

## Error handling and observability

- Libraries use `thiserror`; binaries use `anyhow`. No panics across mod boundaries.
- `tracing` everywhere, with Tracy as a subscriber in profiling builds.
- Crash handler: symbolicated backtrace + system info → local crash report file, plus an offer to open a pre-filled
  GitHub issue (owner decision 6.3). No telemetry.
