# 05: Performance program

Owner direction: **as fast as possible while still 1:1.** Every optimization lives behind the parity suites: if a
suite goes red, the optimization is wrong.

## Accepted items (round 7.3, all four)

### Tracy + perf CI dashboard
- `tracing` spans in every subsystem; `tracing-tracy` in `profiling` builds; GPU zones via Vulkan timestamp queries.
- `zenith-bench` scenarios (deterministic, via input replay): chunk generation throughput (chunks/s, cold and warm),
  server MSPT on fixed worlds (idle, redstone-heavy, mob-heavy), client frame time at fixed camera paths, and memory
  footprint.
- A CI job on a dedicated runner writes `progress/perf/<date>.json`. The website charts it on the tracker page. A
  regression over 3% on any benchmark fails the PR (with a manual override label).

### PGO + LTO + CPU-tier builds
- Release profile: fat LTO, `codegen-units = 1`, `panic = "abort"`.
- PGO: instrumented build → run the bench scenarios → `llvm-profdata merge` → optimized build. BOLT post-link on Linux.
- CPU tiers: x86-64-v2 (baseline), v3 (AVX2) and v4 (AVX-512), shipped as one launcher-selected binary per tier, or a
  fat binary with `multiversion`-style dispatch for hot kernels (noise, meshing). Apple Silicon and ARM64 Linux get
  NEON-tuned kernels.
- Allocator: mimalloc globally, plus arenas for per-tick scratch.

### Deterministic parallel pipeline
- Worldgen statuses on a work-stealing pool, with per-status neighbor-radius dependencies as in vanilla.
- Lighting parallel per region with deterministic merge.
- Meshing fully parallel (pure function of chunk + neighbors + resource state).
- Pathfinding requests computed in parallel *before* the tick phase that consumes them, but only when the inputs
  can't be changed by earlier steps in the same phase. Otherwise they run inline.
- Every parallel site names its covering parity suite in a code comment, and CI fails if the comment is missing (a
  custom lint in `xtask`).

### GPU-driven rendering
- Section geometry in large shared buffers; `vkCmdDrawIndexedIndirectCount` per render layer.
- Compute culling (frustum + hierarchical-Z occlusion). It must never cull anything vanilla would draw: occlusion
  rules are conservative, and the screenshot suite guards it.
- Bindless texture arrays (descriptor indexing, core in Vulkan 1.3) for atlases and entity textures.
- Persistent mapped staging rings, async transfer queue, and pipeline cache persisted per driver.

## Other techniques (no owner decision needed; all output-neutral)

- Data-oriented chunk storage, with paletted containers unpacked to dense arrays for hot loops.
- SIMD noise sampling and density-function tapes.
- io_uring region I/O; zstd for internal caches only (never for vanilla-format files).
- Interned identifiers and pre-resolved registry handles.
- Lock-free entity tracking queues.
- Zero-copy packet encoding into pooled buffers; batched network writes.

## Targets (initial; refined after M19)

| Metric | Target vs vanilla on the same machine |
|--------|----------------------------------------|
| Chunk gen throughput | ≥ 3× |
| Server MSPT (typical survival world) | ≤ 0.5× |
| Client frame time (render distance 16) | ≤ 0.5× |
| Cold start to title screen | ≤ 0.3× |
| RAM at render distance 16 | ≤ 0.6× |

The owner's machine (RTX 4080, 16 threads) is the reference rig. The Steam Deck is the low-end reference.
