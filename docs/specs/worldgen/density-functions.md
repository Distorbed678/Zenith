# Worldgen: density functions, noise routing, and terrain fill (26.3)

- Verified by suites: `golden-terrain`
- Sources: data-pack JSON schemas for `minecraft:worldgen/density_function`, `minecraft:worldgen/noise_settings`, and `minecraft:worldgen/noise`, public cubic Hermite spline and trilinear interpolation definitions, and local reference reading (2026-11-25) for 26.3 exact types, precision, compiler rewrites, volume vs. point evaluation differences, seeding, and terrain fill order. Everything below is written in our own words.
- Related specs:
  - [`math/noise.md`](../math/noise.md) — gradient/Perlin/smeared-Perlin/simplex/octave-stack/normal-noise/blended-noise math and seeding
  - [`math/random.md`](../math/random.md) — legacy 48-bit LCG, xoroshiro128++, positional random factories, `fromHashOf`, position hash
  - [`math/java-math-helpers.md`](../math/java-math-helpers.md) — `Mth` helpers (`lerp`, `lerp3`, `clamp`, `sqrt`, `fastInvSqrt`, `binarySearch`, `floorDiv`, `positiveModulo`, `BlockPos.asLong`, `ChunkPos.pack`, `QuartPos`)
  - [`codec/datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md) — strict JSON parsing, record/either/dispatch codec semantics, integer truncation
  - [`registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) — identifiers, holders, registries, block-state IDs

---

## Summary

Version 26.3 replaces the legacy double-precision per-point density-function pipeline with a compiled, **single-precision (`f32`)** volume-and-point sampler architecture:

1. **AST and codecs (`DensityFunction`, 44 registered types)**: Data-pack JSON defines an immutable DAG/tree of `DensityFunction` nodes (plus numeric literal shorthands for constants and identifier references into `minecraft:worldgen/density_function`). Each node reports a conservative `f32` `Interval` range and a 3-bit `domainAxes` mask (`X`, `Y`, `Z`).
2. **Compiler and rewrite passes (`DensityFunctionCompiler`)**: Before sampling, every root density function is rewritten to inline registry references, deduplicate `cache` subtrees into indexed cache cells, and insert `slice` wrappers wherever a node is invariant along one or more axes (`SliceUniformAxes`), then compiled into a tree of `DensitySampler` implementations.
3. **Dual evaluation paths (`sampleVolume` vs. `sampleValue`)**: Every sampler evaluates either over a regular 3D strided grid (`DensityVolume` into a flat Z-X-Y `f32` `DensityBuffer`) or at a single `(blockX, blockY, blockZ)` point. Several nodes (notably `interpolated`, `noise`, `mul`, `div`, `min`, `max`, `shift_b`) have subtle floating-point or short-circuit differences between volume and point paths; caches bridge the two paths.
4. **Seeding (`RandomState`)**: A dimension's `RandomState` derives all noise samplers, blended-noise RNGs, end-island RNGs, and named positional random factories from `(worldSeed, useLegacyRandomSource)`.
5. **Noise routing and settings (`NoiseRouter`, `NoiseSettings`, `NoiseGeneratorSettings`)**: In 26.3, `NoiseRouter` carries 8 density functions (6 climate fields, `chunk_surface_level`, and `final_density`); `NoiseSettings` carries only `(min_y, height)` because interpolation cell sizes live on `interpolated` nodes; aquifers live in an optional `aquifers` config on `NoiseGeneratorSettings`, and ore veins live inside `material_rule`.
6. **Chunk terrain generation (`NoiseChunk`, `NoiseBasedChunkGenerator.buildTerrain`)**: One `NoiseChunk` owns the shared per-chunk `SamplerContext` (caches, buffer pool, `Beardifier`, `Blender`) and `Aquifer` across the three sub-stages of the `TERRAIN` chunk status: `doFill` (evaluating `final_density` over the `16 × height × 16` chunk volume and writing non-air blocks + heightmaps), `buildSurface` (material rules), and `generateCarvers`.
7. **Structure adaptation (`Beardifier`) and old-chunk blending (`Blender` / `BlendingData`)**: Context-bound density contributors injected into `SamplerContext` for each chunk.

---

## Inputs and state

### Arithmetic and type conventions
- All density values, `Interval` bounds, `DensityBuffer` entries, and `CubicSpline` evaluations are **IEEE-754 binary32 (`f32`)**, round-to-nearest-even, **no fused multiply-add** (each operation rounds independently).
- World block coordinates `(blockX, blockY, blockZ)` are signed 32-bit integers (`i32`), wrapping on overflow. Noise input coordinates (`blockCoord * scale + shift`) are computed in **`f64`**, widening `f32` shifts to `f64` before adding.
- Java `Math.floor(double)`, `Math.ceil(double)`, `Math.pow(double, double)`, and `Math.log(double)` widen `f32` inputs to `f64`, compute in `f64`, and narrow back to `f32`.
- Java `Math.round(float)` rounds ties toward $+\infty$ into a saturating **`i32`** (`NaN -> 0`, `>= 2^31-1 -> 2^31-1`, `<= -2^31 -> -2^31`), then widens to `f32` when stored in a `float`.
- Java `Math.min`/`Math.max` propagate `NaN` and order `-0.0f < +0.0f`; `Mth.clamp(v, lo, hi)` returns `lo` if `v < lo`, else `Math.min(v, hi)` (see `java-math-helpers.md`).
- Java `Math.floorDiv(a, b)` / `Math.floorMod(a, b)` floor toward $-\infty$; plain `/` and `%` on integers truncate toward zero.
- `QuartPos.fromBlock(b) = b >> 2`, `QuartPos.toBlock(q) = q << 2`, `QuartPos.fromSection(s) = s << 2`, `QuartPos.toSection(q) = q >> 2` (arithmetic shifts).

### Axis bitmask (`@Axes`)
Every `DensityFunction` declares which coordinate axes its output may depend on:
- `AXIS_X = 1` (`0b001`)
- `AXIS_Y = 2` (`0b010`)
- `AXIS_Z = 4` (`0b100`)
- `NO_AXES = 0` (`0b000`)
- `ALL_AXES = 7` (`0b111`)

### Conservative `f32` interval arithmetic (`Interval`)
An `Interval` is either the singleton not-an-interval `NaI` (`[NaN, NaN]`) or a closed `f32` interval `[min, max]` with `min <= max` (infinities allowed; `INFINITE = [-Inf, +Inf]`). Construction with `max < min` or `NaN` (other than `NaI`) is an error.

Let $u \odot v = 0.0_f$ if $u = 0.0_f$ or $v = 0.0_f$, else $u \cdot v$ in `f32` (zero-absorbing bound product, preventing $0 \times \pm\infty = \text{NaN}$):

| Operation | Exact rule |
|---|---|
| `ofExact(v)` | `[v, v]` |
| `ofSymmetric(r)` | `[-r, r]` |
| `encapsulating(list)` | Start `min = +Inf, max = -Inf`; ignore `NaI` elements; update `min = min(min, e.min)`, `max = max(max, e.max)`; if `max < min` return `NaI`, else `[min, max]`. |
| `encapsulating(a: f32, b: f32)` | Both `NaN` $\to$ `NaI`; one `NaN` $\to$ `ofExact(other)`; else `[min(a,b), max(a,b)]`. |
| `add(A, B)` | `lo = A.min + B.min`, `hi = A.max + B.max`; `NaI` if either is `NaN`, else `[lo, hi]`. |
| `sub(A, B)` | `lo = A.min - B.max`, `hi = A.max - B.min`; `NaI` if either is `NaN`, else `[lo, hi]`. |
| `mul(A, B)` | If either `NaI` $\to$ `NaI`; compute the 4 products $A.\text{min} \odot B.\text{min}$, $A.\text{min} \odot B.\text{max}$, $A.\text{max} \odot B.\text{min}$, $A.\text{max} \odot B.\text{max}$; return `[min of 4, max of 4]`. |
| `reciprocal(A)` | If `NaI` or `A == [0, 0]` $\to$ `NaI`. If `0` not in `A` $\to$ `[1/A.max, 1/A.min]`. Else if `A.max == 0` $\to$ `[-Inf, 1/A.min]`. Else if `A.min == 0` $\to$ `[1/A.max, +Inf]`. Else `INFINITE`. |
| `div(A, B)` | `mul(A, reciprocal(B))` |
| `min(A, B)` | `NaI` if either `NaI`; else `[min(A.min, B.min), min(A.max, B.max)]`. |
| `max(A, B)` | `NaI` if either `NaI`; else `[max(A.min, B.min), max(A.max, B.max)]`. |
| `clamp(A, lo, hi)` | Rejects `lo > hi`. `NaI` if `A` is `NaI`. If `A.min >= hi` $\to$ `[hi, hi]`. Else if `A.max <= lo` $\to$ `[lo, lo]`. Else `[max(A.min, lo), min(A.max, hi)]`. |
| `abs(A)` | `NaI` if `NaI`; `hi = max(|A.min|, |A.max|)`; if `0 ∈ A` $\to$ `[0, hi]`, else `[min(|A.min|, |A.max|), hi]`. |
| `square(A)` | `NaI` if `NaI`; `hi = max(A.min², A.max²)`; if `0 ∈ A` $\to$ `[0, hi]`, else `[min(A.min², A.max²), hi]`. |
| `mapMonotonic(A, f)` | `NaI` if `NaI`; compute `u = f(A.min)`, `v = f(A.max)`; error if either is `NaN`; return `[min(u, v), max(u, v)]`. |
| `log(A)` | If `A.max < 0.0f` $\to$ `NaI`; else `mapMonotonic(max(A, [0, 0]), x -> (float) Math.log(x))`. (Note: if `A` is `NaI`, `A.max < 0` is false, `max(NaI, [0,0])` is `NaI`, returns `NaI`.) |
| `sign(A)` | `NaI` if `NaI`. If `A.min == A.max` $\to$ `ofExact(signum(A.min))`. If `0 ∈ A`: `A.min == 0` $\to$ `[0, 1]`, `A.max == 0` $\to$ `[-1, 0]`, else `[-1, 1]`. Else `ofExact(A.min > 0 ? 1 : -1)`. |
| `pow(B, E)` | `NaI` if either `NaI`. If `B.min == B.max`, evaluate scalar-base `powScalar(B.min, E)`. Else encapsulate `powScalar(B.min, E)` and `powScalar(B.max, E)`; if `0 ∈ B`, also encapsulate `powScalar(+0.0f, E)` when `B.max > 0` and `powScalar(-0.0f, E)` when `B.min < 0`. See details below for `powScalar`. |
| `lerp(T, A, B)` | `NaI` if any `NaI`; encapsulate `lerpScalar(T, A.min, B.min)`, `lerpScalar(T, A.max, B.min)`, `lerpScalar(T, A.min, B.max)`, `lerpScalar(T, A.max, B.max)`. For finite `(a, b)`, `lerpScalar` encapsulates `a + (t.min ⊙ (b - a))` and `a + (t.max ⊙ (b - a))`. For infinite `(a, b)`: if `a == b` $\to$ `[a, a]`; else evaluate `p = ((1 - t) ⊙ a) + (t ⊙ b)` at `t.min` and `t.max` (when both terms are infinite: `t <= 0` $\to$ `b > a ? -Inf : +Inf`, `t >= 1` $\to$ `b > a ? +Inf : -Inf`, else `NaN` $\to$ `NaI`). |

Scalar-base `powScalar(b: f32, E)`:
- If `b` is `NaN` or `E` is `NaI` $\to$ `NaI`.
- If `E.min == E.max`: `v = f32(pow_f64(b, E.min))`; `NaI` if `NaN`, else `[v, v]`.
- Else if `b == 0.0f`: let $Z$ be: if `0 ∈ E` (`E.max == 0` $\to$ `[1, +Inf]`, `E.min == 0` $\to$ `[0, 1]`, else `[0, +Inf]`), else (`E.max < 0` $\to$ `[+Inf, +Inf]`, else `[0, 0]`); return `mul(Z, ofExact(copySign(1.0f, b)))`.
- Else if `b == 1.0f` $\to$ `[1, 1]`.
- Else if `b > 0.0f`: if both `E.min` and `E.max` finite $\to$ `encapsulating(f32(pow(b, E.min)), f32(pow(b, E.max)))`; else if both infinite $\to$ `[0, +Inf]`; else if `E.min` infinite $\to$ (`b < 1` $\to$ `[f32(pow(b, E.max)), +Inf]`, else `[0, f32(pow(b, E.max))]`); else (`E.max` infinite) $\to$ (`b < 1` $\to$ `[0, f32(pow(b, E.min))]`, else `[f32(pow(b, E.min)), +Inf]`).
- Else (`b < 0.0f`): let `lo = f32(ceil(E.min))`, `hi = f32(floor(E.max))`; if `hi < lo` $\to$ `NaI`. Start `R = encapsulating(f32(pow(b, lo)), f32(pow(b, hi)))`. If `lo` infinite, encapsulate `-f32(pow(b, lo))`; else if `lo + 1 < hi`, encapsulate `f32(pow(b, lo + 1))`. If `hi` infinite, encapsulate `-f32(pow(b, hi))`; else if `hi - 1 > lo`, encapsulate `f32(pow(b, hi - 1))`. Return `R`.

### Sampling grid (`DensityVolume`) and flat buffer (`DensityBuffer`)
A `DensityVolume` is a 9-tuple `(sizeX, sizeY, sizeZ, minBlockX, minBlockY, minBlockZ, stepBlockX, stepBlockY, stepBlockZ)` with all `size* > 0` and all `stepBlock* > 0` (unit-step shorthand sets `stepBlock* = 1`).
- Block coordinates: `blockX(ix) = minBlockX + ix * stepBlockX`, `blockY(iy) = minBlockY + iy * stepBlockY`, `blockZ(iz) = minBlockZ + iz * stepBlockZ`.
- Inclusive max block coordinates: `maxBlockX = minBlockX + sizeX * stepBlockX - 1` (likewise Y, Z).
  - Note: for `step > 1`, `maxBlockX` is `step - 1` past the last sampled coordinate `blockX(sizeX - 1)`. This matters for `intersects(BoundingBox)` and `InterpolatedFunction`.
- Total element count: `size = sizeX * sizeY * sizeZ`.
- Flat index layout (**Z outer, X middle, Y contiguous inner**):
  $$\text{index}(i_x, i_y, i_z) = i_y + (i_x + i_z \cdot \text{sizeX}) \cdot \text{sizeY}$$
- Reverse lookup `indexOfBlock(bx, by, bz)`: let `rx = bx - minBlockX`, `ry = by - minBlockY`, `rz = bz - minBlockZ`.
  - If all steps are `1`: if `0 <= rx < sizeX` and `0 <= ry < sizeY` and `0 <= rz < sizeZ`, return `index(rx, ry, rz)`; else `-1`.
  - Otherwise: if `0 <= rx < sizeX * stepX`, `0 <= ry < sizeY * stepY`, `0 <= rz < sizeZ * stepZ`, and `floorMod(rx, stepX) == 0`, `floorMod(ry, stepY) == 0`, `floorMod(rz, stepZ) == 0`, return `index(floorDiv(rx, stepX), floorDiv(ry, stepY), floorDiv(rz, stepZ))`; else `-1`.
- Default naive volume loop (`sampleVolumeNaive`): loops `z ∈ [0, sizeZ)`, `x ∈ [0, sizeX)`, `y ∈ [0, sizeY)`, writing `sampleValue(context, blockX(x), blockY(y), blockZ(z))` sequentially at `index = 0, 1, ...`.

### Sampling context (`SamplerContext`)
Each sampling session carries a `SamplerContext` holding:
- An immutable map of context fields keyed by `ContextKey` (used for `Beardifier.CONTEXT_KEY`, `Blender.CONTEXT_KEY`, `Blender.ALPHA_KEY`, `Blender.OFFSET_KEY`).
- A temporary `f32` buffer arena (`DensityBufferArena`; in chunk generation a per-`NoiseChunk` `DensityBufferPool` rounded up to multiples of 16, pooled up to 16 pools per `RandomState` with max buffer age 20 GC ticks; pool reuse has no effect on output values because every consumer overwrites or fills acquired buffers).
- Optional per-context cache table (`enableCaches` true inside `NoiseChunk` and `NoiseSpawnFinder`; false for `SamplerContext.EMPTY_UNCACHED`).

---

## Behavior (ordered rules)

### 1. Compilation, optimization, and caching (`DensityFunctionCompiler`)

`RandomState` owns one `DensityFunctionCompiler`. Calling `getSampler(function)` memoizes by structural equality of `function`. On a cache miss (serialized under a lock so cache IDs are deterministic):

#### Step 1A — Reference inlining and `cache` deduplication (pass 1)
Walk the AST top-down:
1. If the current node is a `HolderHolder` (a named reference into `minecraft:worldgen/density_function`), unwrap it to its referenced `DensityFunction` value.
2. If the node is `CacheFunction(input)`:
   - Look up `input` (the **un-rewritten** child `DensityFunction`, compared by structural equality) in the compiler's `preparedCaches` table.
   - If absent:
     1. Assign `id = nextCacheId++` (starting from `0`).
     2. Run the **entire** two-pass `optimizerRule` (pass 1 followed by pass 2) on `input` to produce `optInput`.
     3. Compile `optInput` into a `DensitySampler` and wrap it in `CachingDensitySampler(id, compiledInput)`.
     4. Record `PreparedCache(id, optInput.range(), optInput.domainAxes(), cachingSampler)` under key `input`.
   - Return the `PreparedCache` node (its `rewriteChildren` returns `this`, so pass 2 will not descend inside it, though pass 2 may still wrap it in `SliceFunction`).
3. Otherwise, recursively rewrite every child `DensityFunction` with pass 1 (for `HolderHolder` wrapping another `HolderHolder`, `HolderHolder.rewriteChildren` unwraps and recurses).

#### Step 1B — Uniform-axis slicing (`SliceUniformAxes`, pass 2)
Walk the pass-1 AST top-down carrying `parentDomainAxes` (initialized to `ALL_AXES = 7`):
1. If the node is `ConstantFunction` or `GradientFunction`, return it unchanged (do not wrap in `SliceFunction`).
2. Let `axes = node.domainAxes()`.
3. If `axes == parentDomainAxes`, recursively rewrite children with `parentDomainAxes` and return.
4. Otherwise:
   - Recursively rewrite children with `newParentDomainAxes = axes`.
   - Compute `removedAxes = parentDomainAxes & ~axes`.
   - Collect `existingRemovedAxes` by peeling any consecutive outermost `SliceFunction` nodes already on the rewritten node and ORing their axis bits.
   - Let `filteredAxes = removedAxes & ~existingRemovedAxes`.
   - Wrap the rewritten node in `SliceFunction(axis, 0, ...)` for each bit set in `filteredAxes`, in exact order from innermost to outermost:
     1. If `AXIS_X` set: wrap in `SliceFunction(X, 0, node)`.
     2. If `AXIS_Z` set: wrap in `SliceFunction(Z, 0, node)`.
     3. If `AXIS_Y` set: wrap in `SliceFunction(Y, 0, node)`.

#### Step 1C — Compilation to `DensitySampler`
Call `compileSampler(compileContext)` on the rewritten AST. Each node compiles its children and returns a specialized `DensitySampler` (detailed in Section 2).

#### Step 1D — Runtime cache semantics (`CachingDensitySampler` & `SamplerContext`)
Each cache cell `cacheCells[cacheId]` stores four mutable fields: `volume: Option<DensityVolume>` (initially none), `buffer: Option<Buffer>` (initially none), `valueKey: i64` (initially `0`), `value: f32` (initially `NaN`).

- **When `enableCaches == false`**: pass straight through to `input.sampleVolume` / `input.sampleValue`.
- **`sampleVolumeCached(cacheId, input, outputBuffer, volume)`**:
  1. If `cell.buffer` is empty OR `!volume.equals(cell.volume)`:
     - Release any previous `cell.buffer`.
     - Store `cell.volume = volume`, acquire a new buffer `cell.buffer`, and evaluate `input.sampleVolume(context, cell.buffer, volume)`.
  2. Copy all `volume.size()` floats from `cell.buffer` into `outputBuffer`.
  - Note: a single cache cell holds only the **most recent** `DensityVolume`; sampling a different volume overwrites it. (In particular, `cell.valueKey`/`cell.value` are NOT cleared when the volume changes.)
- **`sampleValueCached(cacheId, input, blockX, blockY, blockZ)`**:
  1. Compute `cacheKey = BlockPos.asLong(blockX, blockY, blockZ)` (26-bit X, 26-bit Z, 12-bit Y; see `java-math-helpers.md`).
  2. If `cell.valueKey == cacheKey && !isNaN(cell.value)`: return `cell.value`.
  3. Else if `cell.buffer` and `cell.volume` are present: compute `idx = cell.volume.indexOfBlock(blockX, blockY, blockZ)`. If `idx != -1`, return `cell.buffer.get(idx)` (without updating `cell.valueKey`/`cell.value`).
  4. Otherwise evaluate `v = input.sampleValue(context, blockX, blockY, blockZ)`, store `cell.valueKey = cacheKey`, `cell.value = v`, and return `v`.

**Critical parity rule**: Because step 3 of `sampleValueCached` reads directly from the cached volume buffer, any subtle difference between `sampleVolume` and `sampleValue` (such as `NoiseFunction` octave coordinate associativity or `InterpolatedFunction` incremental Y stepping) becomes visible to subsequent point queries (e.g., `Aquifer`, `MaterialSystem`, `FindTopSurfaceFunction`) whenever a `cache` node was populated by an earlier `sampleVolume` call covering that coordinate!

---

### 2. All 44 density function types (26.3)

#### Top-level `DensityFunction` JSON codec
A `DensityFunction` value in JSON accepts three forms (checked in order):
1. **Identifier string** (registry reference): looks up a `Holder.Reference<DensityFunction>` in dynamic registry `minecraft:worldgen/density_function`, wrapped as `HolderHolder`.
2. **JSON number** in `[-1000000.0f, 1000000.0f]` (`NOISE_VALUE_CODEC`): decodes directly as `ConstantFunction(value)`.
3. **JSON object** with `"type": "<id>"` dispatched through built-in registry `minecraft:worldgen/density_function_type` (44 entries below).

> **Note on 26.3 schema changes from pre-26.3 versions**:
> - Removed/replaced types: `shifted_noise` (merged into `noise` via optional `shift_x`/`shift_y`/`shift_z`), `y_clamped_gradient` (merged into `gradient`), `weird_scaled_sampler` (replaced by `interval_select` + `noise` + `mul` + `abs`), `flat_cache` / `cache_2d` / `cache_once` / `cache_all_in_cell` (unified into `cache` + `slice` + `SliceUniformAxes`), `invert` (renamed `reciprocal`), `end_islands` (renamed `end_outer_islands`, with main End island built using `distance_to_point`).
> - Field renames: unary ops use `"input"` (not `"argument"`); binary ops use `"left"` and `"right"` (not `"argument1"` / `"argument2"`); `interpolated` uses `"input"`, `"cell_size_xz"`, `"cell_size_y"`; `find_top_surface` uses `"cell_height"`.
> - All numeric density fields use `f32` (`NOISE_VALUE_CODEC` = `f32` in `[-1000000.0, 1000000.0]`), except `noise` scales and `old_blended_noise` parameters which remain `f64`.

#### 2.1 Generator functions (11 types)

##### `minecraft:constant` (`ConstantFunction`)
- **JSON fields**: `value` (`f32` in `[-1e6, 1e6]`, required). Also decoded from a bare JSON number.
- **`domainAxes`**: `0` (`NO_AXES`). **`range`**: `[value, value]`.
- **Point / Volume**: returns `value` / fills buffer with `value`.

##### `minecraft:blend_alpha`, `minecraft:blend_offset`, `minecraft:beardifier` (`SimpleDensityFunction`)
- **JSON fields**: none (empty object `{"type": "..."}`).
- **`domainAxes`**, **`range`**, and **context-bound sampler**:
  - `minecraft:blend_alpha`: `domainAxes = 5` (X|Z), `range = [0.0f, 1.0f]`, reads `Blender.ALPHA_KEY` from `SamplerContext`, fallback constant `1.0f`.
  - `minecraft:blend_offset`: `domainAxes = 5` (X|Z), `range = [-Inf, +Inf]`, reads `Blender.OFFSET_KEY` from `SamplerContext`, fallback constant `0.0f`.
  - `minecraft:beardifier`: `domainAxes = 7` (X|Y|Z), `range = [-Inf, +Inf]`, reads `Beardifier.CONTEXT_KEY` from `SamplerContext`, fallback constant `0.0f`.

##### `minecraft:noise` (`NoiseFunction`)
- **JSON fields**:
  - `noise`: `Holder<NormalNoise>` (registry identifier in `minecraft:worldgen/noise` or inline `NormalNoise` object; note `RandomState` requires a registered key — see Section 4).
  - `xz_scale`: `f64` (required, unconstrained).
  - `y_scale`: `f64` (required, unconstrained).
  - `shift_x`, `shift_y`, `shift_z`: `DensityFunction` (each optional, default `ConstantFunction(0.0f)`).
- **`domainAxes`**: start with `7`; clear `AXIS_Y` if `y_scale == 0.0`; clear `AXIS_X | AXIS_Z` if `xz_scale == 0.0`; then bitwise-OR `shift_x.domainAxes() | shift_y.domainAxes() | shift_z.domainAxes()`.
- **`range`**: `noise.value().range()` — i.e., `NormalNoise`'s precomputed parameter range `[-r, r]` where $r = \text{f32}((T \cdot \frac{1}{3}) \cdot 6.0)$ (see `docs/specs/math/noise.md` §7 step 5, NOT the wider `OctaveStack` range).
- **Compilation & evaluation** (after `CompileContext.createNoiseSampler(noise)`):
  1. **Unshifted** (`shift_x == ZERO && shift_y == ZERO && shift_z == ZERO`, structural equality against `ConstantFunction(+0.0f)`):
     - Point: `noise.get(blockX * xz_scale, blockY * y_scale, blockZ * xz_scale)`.
     - Volume: `outputBuffer.fill(0.0f)` then `noise.addToVolume(outputBuffer, volume, xz_scale, y_scale, 1.0f)` (see `noise.md` §5 for the `blockCoord * (scale * layerFreq)` vs. `(blockCoord * scale) * layerFreq` 1-ULP distinction).
  2. **XZ-shifted** (`shift_y == ZERO`, at least one of `shift_x`/`shift_z` not `ZERO`):
     - Point: `x = blockX * xz_scale + (f64) shift_x(bx, by, bz)`, `y = blockY * y_scale`, `z = blockZ * xz_scale + (f64) shift_z(bx, by, bz)`; return `noise.get(x, y, z)`.
     - Volume: sample `shift_x` into `outputBuffer`, sample `shift_z` into a temp buffer, then loop `z`, `x`, `y` computing `baseX = blockX(x) * xz_scale`, `baseZ = blockZ(z) * xz_scale`, `noise.get(baseX + (f64) shiftX[i], blockY(y) * y_scale, baseZ + (f64) shiftZ[i])`.
  3. **XYZ-shifted** (`shift_y != ZERO`):
     - Same as XZ-shifted, adding `+(f64) shift_y` to `blockY * y_scale`.

##### `minecraft:shift`, `minecraft:shift_a`, `minecraft:shift_b` (`ShiftNoiseFunction`)
- **JSON fields**: `noise` (`Holder<NormalNoise>`, required).
- **`range`**: `Interval.mul(noise.value().range(), [4.0f, 4.0f])`.
- **`domainAxes` & evaluation**:
  - `minecraft:shift`: `domainAxes = 7`. Compiles to `ConstMulSampler(NoiseFunction.Sampler(noise, 0.25, 0.25), 4.0f)`. Point: `noise.get(bx * 0.25, by * 0.25, bz * 0.25) * 4.0f`; volume: unshifted `NoiseFunction` volume then multiply every entry by `4.0f`.
  - `minecraft:shift_a`: `domainAxes = 5` (X|Z). Compiles to `ConstMulSampler(NoiseFunction.Sampler(noise, 0.25, 0.0), 4.0f)`. Point: `noise.get(bx * 0.25, 0.0, bz * 0.25) * 4.0f`.
  - `minecraft:shift_b`: `domainAxes = 5` (X|Z).
    - Point: `noise.get(bz * 0.25, bx * 0.25, 0.0) * 4.0f` (note: Z maps to noise X, X maps to noise Y, noise Z is `0.0`).
    - Volume: constructs transposed volume `(sizeZ, sizeX, 1, minBlockZ, minBlockX, 0, stepZ, stepX, 1)`, fills temp buffer with `0.0f`, calls `noise.addToVolume(temp, transposedVolume, 0.25, 0.25, 4.0f)`, then for each `(z, x)` broadcasts `temp[index(z, x, 0)]` across all `sizeY` entries of column `(x, z)` in `outputBuffer`.

##### `minecraft:end_outer_islands` (`EndIslandFunction`)
- **JSON fields**: none.
- **`domainAxes`**: `5` (X|Z). **`range`**: `[-0.84375f, 0.5625f]`.
- **Compilation**: calls `context.createEndIslandRandom()` (always a legacy 48-bit LCG seeded with `worldSeed`), calls `consumeCount(17292)`, and builds a single 2D `SimplexNoise` with zero origin offsets ($S = 0.0$, still consuming 3 `nextDouble()` + 256 Fisher-Yates draws; see `noise.md` §4).
- **Point evaluation at `(bx, by, bz)`**:
  1. Compute `sectionX = bx / 8`, `sectionZ = bz / 8` (**truncating** integer division toward zero, NOT `floorDiv`).
  2. Compute `chunkX = sectionX / 2`, `chunkZ = sectionZ / 2`, `subX = sectionX % 2`, `subZ = sectionZ % 2` (all truncating toward zero).
  3. Initialize `doffs = -100.0f`.
  4. Loop `xo` from `-12` to `12` inclusive, inner `zo` from `-12` to `12` inclusive:
     - `tx = (i64)(chunkX + xo)`, `tz = (i64)(chunkZ + zo)`.
     - If `tx * tx + tz * tz > 4096L` and `islandNoise.get((f64) tx, (f64) tz) < -0.9f`:
       - `islandSize = (abs((f32) tx) * 3439.0f + abs((f32) tz) * 147.0f) % 13.0f + 9.0f` (IEEE-754 `f32` truncated remainder `%`).
       - `xd = (f32)(subX - xo * 2)`, `zd = (f32)(subZ - zo * 2)`.
       - `newDoffs = Mth.clamp(100.0f - Mth.sqrt(xd * xd + zd * zd) * islandSize, -100.0f, 80.0f)`.
       - `doffs = Math.max(doffs, newDoffs)`.
  5. Return `(doffs - 8.0f) / 128.0f` in `f32`.
- **Volume evaluation**: loops `z`, `x`, computes point value at `(blockX(x), 0, blockZ(z))`, and broadcasts across all `sizeY` entries of that column.
- *(Note: the central End main island is no longer hard-coded inside this function; vanilla `minecraft:end/islands` combines `end_outer_islands` via `max` with `slice(y=0, (clamp(100 - distance_to_point([0,0,0], euclidean), -100, 80) - 8) * 0.0078125)`.)*

##### `minecraft:distance_to_point` (`DistanceToPointFunction`)
- **JSON fields**:
  - `point`: `[x, y, z]` array of 3 `i32` (`Vec3i.CODEC`, required).
  - `metric`: enum string (required): `"euclidean"`, `"euclidean_squared"`, `"manhattan"`, or `"chebyshev"`.
- **`domainAxes`**: `7`. **`range`**: `[0.0f, +Inf]`.
- **Evaluation**: compute 32-bit wrapping differences `dx = (f32)(point.x - bx)`, `dy = (f32)(point.y - by)`, `dz = (f32)(point.z - bz)`:
  - `euclidean`: `Mth.sqrt((dx*dx + dy*dy) + dz*dz)` (`f32` left-to-right sum, `f64` sqrt narrowed to `f32`).
  - `euclidean_squared`: `(dx*dx + dy*dy) + dz*dz` (`f32`).
  - `manhattan`: `(|dx| + |dy|) + |dz|` (`f32`).
  - `chebyshev`: `Math.max(Math.max(|dx|, |dy|), |dz|)` (`f32`).
- **Volume**: standard Z-X-Y loop calling point evaluation.

##### `minecraft:gradient` (`GradientFunction`)
- **JSON fields**:
  - `axis`: `"x"` | `"y"` | `"z"` (required).
  - `tiling`: `"clamp_to_edge"` | `"repeat"` | `"mirrored_repeat"` (optional, default `"clamp_to_edge"`).
  - `from_coordinate`, `to_coordinate`: `i32` (required; codec validation rejects `from_coordinate == to_coordinate`).
  - `from_value`, `to_value`: `f32` in `[-1e6, 1e6]` (required).
- **`domainAxes`**: single bit for `axis`. **`range`**: `Interval.encapsulating(from_value, to_value)`.
- **Compilation**: precompute `range = to_coordinate - from_coordinate` (`i32`, may be negative) and `factor = (to_value - from_value) / (f32) range` (`f32`).
- **Point evaluation** on `c = choose(axis, bx, by, bz)`:
  - `clamp_to_edge`: `lo = min(from, to)`, `hi = max(from, to)`; return `from_value + (f32)(Mth.clamp(c, lo, hi) - from_coordinate) * factor`.
  - `repeat`: return `from_value + (f32) Math.floorMod(c - from_coordinate, range) * factor`.
  - `mirrored_repeat`: `rel = c - from_coordinate`, `tile = Math.floorDiv(rel, range)`, `local = rel - tile * range`; if `(tile & 1) == 0` return `from_value + (f32) local * factor`, else `from_value + (f32)(range - local) * factor`.
- **Volume evaluation**: computes once per slice along `axis` and fills the orthogonal plane/columns with that value (identical values to point evaluation).

##### `minecraft:old_blended_noise` (`BlendedNoise`)
- **JSON fields**: `xz_scale`, `y_scale`, `xz_factor`, `y_factor` (`f64` in `[0.001, 1000.0]`), `smear_scale_multiplier` (`f64` in `[1.0, 8.0]`), all required.
- **`domainAxes`**: `7`. **`range`**: computed via the 16-octave smeared-Perlin interval sum specified in `docs/specs/math/noise.md` §8.
- **Compilation**: obtains RNG via `context.createRandom("minecraft:terrain")` (see Section 4), builds `(minLimit, maxLimit, main)` smeared-Perlin octave stacks per `noise.md` §8, wraps them as unshifted `NoiseFunction.Sampler`s with scales `(Mxz, My)`, `(Mxz, My)`, and `(Mxz / xz_factor, My / y_factor)`, and composes:
  `LerpFunction.Sampler(ClampFunction.Sampler(ConstAddSampler(main, 0.5f), 0.0f, 1.0f), minLimit, maxLimit)`.
  (Exact evaluation matches `noise.md` §8, including `0.0f` / `1.0f` lerp short-circuiting and volume vs. point `NoiseStack` associativity.)

---

#### 2.2 Unary functions (11 types, `UnaryFunction`)

All 11 types share JSON schema `{"type": "<id>", "input": <DensityFunction>}`, `domainAxes = input.domainAxes()`, and apply the operation element-wise in both point and volume paths (volume evaluates `input` into `outputBuffer` and mutates in place):

| Type ID | Formula on `x: f32` | `range()` on `I = input.range()` |
|---|---|---|
| `minecraft:abs` | `Math.abs(x)` | `Interval.abs(I)` |
| `minecraft:square` | `x * x` | `Interval.square(I)` |
| `minecraft:cube` | `(x * x) * x` | `Interval.mapMonotonic(I, cube)` |
| `minecraft:sqrt` | `(f32) Math.sqrt((f64) x)` | `Interval.pow(I, [0.5f, 0.5f])` |
| `minecraft:half_negative` | `x > 0.0f ? x : x * 0.5f` | `Interval.mapMonotonic(I, halfNeg)` |
| `minecraft:quarter_negative` | `x > 0.0f ? x : x * 0.25f` | `Interval.mapMonotonic(I, quarterNeg)` |
| `minecraft:reciprocal` | `1.0f / x` | `Interval.reciprocal(I)` |
| `minecraft:negate` | `-x` | `Interval.sub([0.0f, 0.0f], I)` |
| `minecraft:squeeze` | `c = Mth.clamp(x, -1.0f, 1.0f); (c / 2.0f) - (((c * c) * c) / 24.0f)` | `Interval.mapMonotonic(I, squeeze)` |
| `minecraft:log` | `(f32) Math.log((f64) x)` | `Interval.log(I)` |
| `minecraft:sign` | `Math.signum(x)` (`±0.0 -> ±0.0`, `NaN -> NaN`) | `Interval.sign(I)` |

---

#### 2.3 Rounding functions (4 types, `RoundFunction`)

All 4 types share JSON schema `{"type": "<id>", "input": <DensityFunction>, "multiple": <DensityFunction, optional, default 1.0>}`:
- **`domainAxes`**: `input.domainAxes() | multiple.domainAxes()`.
- **`range`**: `Interval.mul(Interval.mapMonotonic(Interval.div(input.range(), mRange), r), mRange)` where `mRange = multiple.range()`.
- Primitive `roundToInt(v: f32)` per type:
  - `minecraft:floor`: `(f32) Math.floor((f64) v)`
  - `minecraft:round`: `(f32) Math.round(v)` (**saturating `i32`**, ties toward $+\infty$, `NaN -> 0`)
  - `minecraft:ceil`: `(f32) Math.ceil((f64) v)`
  - `minecraft:truncate`: `v > 0.0f ? (f32) Math.floor((f64) v) : (f32) Math.ceil((f64) v)`
- **Compilation & evaluation**:
  - If `multiple` is `ConstantFunction(1.0f)`: evaluates `roundToInt(input)` directly.
  - Otherwise: if `m == 0.0f`, returns `input` unchanged; else returns `roundToInt(input / m) * m` in `f32`.

---

#### 2.4 Binary arithmetic & min/max functions (6 types, `BinaryFunction`)

All 6 types share JSON schema `{"type": "<id>", "left": <DensityFunction>, "right": <DensityFunction>}`, `domainAxes = left.domainAxes() | right.domainAxes()`, and `range = Interval.<op>(left.range(), right.range())`.

**Compiler specializations and volume-vs-point rules** (exact order of checks matters for `f32` rounding and short-circuiting):
1. `minecraft:add`:
   - `left` is `ConstantFunction(c)` $\to$ `ConstAdd(right, c)`: `right + c`.
   - Else `right` is `ConstantFunction(c)` $\to$ `ConstAdd(left, c)`: `left + c`.
   - Else `Add`: `left + right`.
2. `minecraft:sub`:
   - `left` is `ConstantFunction(c)` $\to$ `ConstSub(c, right)`: `c - right`.
   - Else `right` is `ConstantFunction(c)` $\to$ `ConstAdd(left, -c)`: `left + (-c)`.
   - Else `Sub`: point computes `left - right`; volume computes `left + (-right)`.
3. `minecraft:mul`:
   - `left` is `ConstantFunction(c)` $\to$ `ConstMul(right, c)`: `right * c`.
   - Else `right` is `ConstantFunction(c)` $\to$ `ConstMul(left, c)`: `left * c`.
   - Else `Mul`:
     - **Point**: evaluate `l = left`; if `l == 0.0f` return `0.0f` (**without** evaluating `right`, returning `+0.0f` even if `l == -0.0f` or `right` would be `±Inf`/`NaN`); else `l * right`.
     - **Volume**: evaluates both buffers and computes `left[i] * right[i]` with **no** zero short-circuit.
4. `minecraft:div`:
   - `left` is `ConstantFunction(c)` $\to$ `ConstDiv(c, right)`: `c / right`.
   - Else `right` is `ConstantFunction(c)` $\to$ **`ConstMul(left, 1.0f / c)`** (multiplies by precomputed `f32` reciprocal, NOT direct division!).
   - Else `Div`:
     - **Point**: evaluate `l = left`; if `l == 0.0f` return `0.0f` (without evaluating `right`); else `l / right`.
     - **Volume**: evaluates both buffers and computes `left[i] / right[i]` with **no** zero short-circuit.
5. `minecraft:min`:
   - **Non-overlapping interval pruning**: if `left.range().max() < right.range().min()`, emit warning and compile to `left` alone; else if `right.range().max() < left.range().min()`, emit warning and compile to `right` alone.
   - Else if `left` is `ConstantFunction(c)` $\to$ `ConstMin(right, c)`; else if `right` is `ConstantFunction(c)` $\to$ `ConstMin(left, c)`; else `Min(left, right, right.range().min())`.
   - **Point**: `ConstMin` uses `Math.min(x, c)`; `Min` evaluates `l = left`, short-circuits `l <= rightMin ? l : Math.min(l, right)`.
   - **Volume**: evaluates both (for `Min`), overwrites `output[i] = r` iff `r < output[i]` (strict `<`, so `NaN` on the right is ignored and signed zero keeps `output[i]`).
6. `minecraft:max`:
   - **Non-overlapping interval pruning**: if `left.range().min() > right.range().max()`, compile to `left`; else if `right.range().min() > left.range().max()`, compile to `right`.
   - Else `ConstMax` (checking `left` then `right`) or `Max(left, right, right.range().max())`.
   - **Point**: `ConstMax` uses `Math.max(x, c)`; `Max` short-circuits `l >= rightMax ? l : Math.max(l, right)`.
   - **Volume**: overwrites `output[i] = r` iff `r > output[i]`.

---

#### 2.5 Power, interpolation, clamping, selection, and structural operators (12 types)

##### `minecraft:pow` (`PowFunction`)
- **JSON fields**: `base`, `exponent` (`DensityFunction`, required).
- **`domainAxes`**: `base.domainAxes() | exponent.domainAxes()`. **`range`**: `Interval.pow(base.range(), exponent.range())`.
- **Compilation** (exact priority):
  1. If `base` is `ConstantFunction(b)` $\to$ `(f32) Math.pow((f64) b, (f64) exponent)`.
  2. Else if `exponent` is `ConstantFunction(e)`:
     - Let `a = Math.abs(e)`. Specialize `a == 0.5f` $\to$ `Sqrt(base)`, `a == 1.0f` $\to$ `base`, `a == 2.0f` $\to$ `Square(base)`, `a == 3.0f` $\to$ `Cube(base)`. If `e < 0.0f` (strictly negative), wrap that special sampler in `Reciprocal` (`1.0f / special`).
     - Any other constant `e` $\to$ `(f32) Math.pow((f64) base, (f64) e)`.
  3. Else $\to$ `(f32) Math.pow((f64) base, (f64) exponent)`.

##### `minecraft:lerp` (`LerpFunction`)
- **JSON fields**: `alpha`, `first`, `second` (`DensityFunction`, required).
- **`domainAxes`**: `alpha | first | second`. **`range`**: `Interval.lerp(alpha.range(), first.range(), second.range())`.
- **Compilation**: specializes `first` constant (checked first), else `second` constant, else general.
- **Evaluation** (both volume and point):
  - If `alpha == 0.0f` (including `-0.0f`): return `first` (point path skips `second`).
  - Else if `alpha == 1.0f`: return `second` (point path skips `first`).
  - Else: `Mth.lerp(alpha, first, second) = first + alpha * (second - first)` in `f32` (unclamped).

##### `minecraft:clamp` (`ClampFunction`)
- **JSON fields**: `input` (`DensityFunction`), `min`, `max` (`f32` in `[-1e6, 1e6]`); codec rejects `max < min`.
- **`domainAxes`**: `input.domainAxes()`. **`range`**: `Interval.clamp(input.range(), min, max)`.
- **Evaluation**: `Mth.clamp(input, min, max)` in `f32`.

##### `minecraft:range_choice` (`RangeChoiceFunction`)
- **JSON fields**: `input`, `when_in_range`, `when_out_of_range` (`DensityFunction`), `min_inclusive`, `max_exclusive` (`f32` in `[-1e6, 1e6]`; codec does **not** require `min_inclusive <= max_exclusive`).
- **`domainAxes`**: `input | when_in_range | when_out_of_range`. **`range`**: `Interval.encapsulating(when_in_range.range(), when_out_of_range.range())`.
- **Evaluation**:
  - Condition: `v >= min_inclusive && v < max_exclusive` (false on `NaN`).
  - Constant specialization when both branches are `ConstantFunction`.
  - Volume (non-const): fills `outputBuffer` with `when_in_range`, samples `input` and `when_out_of_range` into temp buffers, and replaces elements failing the condition. Point: evaluates `input` then only the chosen branch.

##### `minecraft:interval_select` (`IntervalSelectFunction`)
- **JSON fields**:
  - `input`: `DensityFunction`.
  - `thresholds`: list of `f32` in `[-1e6, 1e6]`.
  - `functions`: list of `DensityFunction`, size $\ge 2$.
  - Codec validation: `thresholds.size() == functions.size() - 1`, and `thresholds` must be non-decreasing under `Float::compare` (`<=` allowed).
- **`domainAxes`**: OR of `input` and all `functions`. **`range`**: `Interval.encapsulating` of all `functions`'s ranges.
- **Selection**: smallest index `i` with `input < thresholds[i]`; otherwise `functions.size() - 1` (including `NaN`).
- **Evaluation**: single-threshold specialization when `thresholds.size() == 1`. Volume evaluates `input` and all `functions` over the volume then picks per voxel; point evaluates `input` then only the selected function.

##### `minecraft:spline` (`SplineFunction`)
- **JSON fields**: `spline` (`CubicSpline<Coordinate>`). Detailed in Section 3.

##### `minecraft:cache` (`CacheFunction`)
- **JSON fields**: `input` (`DensityFunction`).
- **`domainAxes`**: `input.domainAxes()`. **`range`**: `input.range()`.
- Must be rewritten by `DensityFunctionCompiler` into `PreparedCache` before compilation (compiling raw `CacheFunction` directly throws). Semantics in Section 1D.

##### `minecraft:blend_density` (`BlendDensityFunction`)
- **JSON fields**: `input` (`DensityFunction`).
- **`domainAxes`**: `input.domainAxes()`. **`range`**: `input.range()`.
- **Evaluation**: evaluates `input`; looks up `Blender.CONTEXT_KEY` in `SamplerContext`. If present and `!blender.isEmpty()`, replaces each value `v` with `blender.blendDensity(blockX, blockY, blockZ, v)` (Section 9). Otherwise passes `input` through unchanged.

##### `minecraft:interpolated` (`InterpolatedFunction`)
- **JSON fields**: `input` (`DensityFunction`), `cell_size_xz` (`i32 >= 1`), `cell_size_y` (`i32 >= 1`).
- **`domainAxes`**: `input.domainAxes()`. **`range`**: `input.range()`.
- Precomputes `invXZ = 1.0f / (f32) cell_size_xz`, `invY = 1.0f / (f32) cell_size_y`.
- **Exact evaluation (volume vs. point — CRITICAL for parity)** is specified in Section 6.2.

##### `minecraft:slice` (`SliceFunction`)
- **JSON fields**: `axis` (`"x"` | `"y"` | `"z"`), `coordinate` (`i32`), `input` (`DensityFunction`).
- **`domainAxes`**: `input.domainAxes() & ~axesFrom(axis)`. **`range`**: `input.range()`.
- **Compilation**: if `input` is also a `SliceFunction` on the other horizontal axis (`X` wrapping `Z` or `Z` wrapping `X`), fuses into `XzSampler(innerInput, x, z)`; otherwise compiles to `XSampler`, `YSampler`, or `ZSampler`.
- **Point**: replaces the sliced coordinate(s) with `coordinate` and calls `input.sampleValue`.
- **Volume**: if the requested volume already has `size == 1` and `minBlock == coordinate` on the sliced axis(es), delegates directly to `input.sampleVolume`. Otherwise constructs a reduced `DensityVolume` with `size = 1`, `minBlock = coordinate` along the sliced axis(es) (retaining original `stepBlock*`), samples `input` into a temp buffer, and broadcasts across the sliced axis(es) into `outputBuffer`.

##### `minecraft:find_top_surface` (`FindTopSurfaceFunction`)
- **JSON fields**:
  - `density`, `upper_bound`: `DensityFunction`.
  - `lower_bound`: `i32` in `[DimensionType.MIN_Y * 2, DimensionType.MAX_Y * 2]` = `[-4064, 4062]`.
  - `cell_height`: `i32 >= 1`.
- **`domainAxes`**: `(density.domainAxes() | upper_bound.domainAxes()) & ~AXIS_Y`.
- **`range`**: `Interval.of((f32) lower_bound, Math.max((f32) lower_bound, upper_bound.range().max()))`.
- **Compilation**: wraps its sampler in `SliceFunction.YSampler(..., 0)`, so Y is always pinned to `0` when querying `upper_bound` and broadcast in volumes.
- **Evaluation at `(bx, bz)`**:
  1. Sample `ub = upper_bound(bx, 0, bz)` (in volume mode, `upper_bound` is sampled as a `sizeY = 1, minBlockY = 0` volume first).
  2. Compute `topY = Mth.floor(ub / (f32) cell_height) * cell_height` (`f32` divide, saturating `i32` floor, wrapping `i32` multiply).
  3. If `topY <= lower_bound`, return `(f32) lower_bound`.
  4. Loop `probeY = topY; probeY >= lower_bound; probeY -= cell_height`:
     - If `density.sampleValue(context, bx, probeY, bz) > 0.0f`, return `(f32) probeY`.
  5. Return `(f32) lower_bound`.

---

### 3. Cubic splines (`CubicSpline` & `SplineFunction`)

#### JSON schema
A `CubicSpline` JSON value is recursive:
- Either a **JSON number** (`f32`, unconstrained `Codec.FLOAT` — NOT `NOISE_VALUE_CODEC`) $\to$ `Constant(value)`.
- Or a **JSON object** (`Multipoint`):
  - `coordinate`: `DensityFunction` (required).
  - `points`: non-empty JSON array of point objects, each with:
    - `location`: `f32` (`Codec.FLOAT`, required)
    - `value`: nested `CubicSpline` (required)
    - `derivative`: `f32` (`Codec.FLOAT`, required)
  - Note: JSON decoding checks that `points` is non-empty, but does **not** enforce strictly ascending `location` order (unlike the programmatic `Builder`). Valid data packs always supply strictly ascending `location`s.

#### Coordinate deduplication & compilation (`SplineFunction`)
- `domainAxes()` is the bitwise OR of `domainAxes()` across every `coordinate` function in the spline tree.
- At compile time, `SplineFunction.Sampler` walks the spline tree and assigns each unique `coordinate` `DensityFunction` (deduplicated by structural equality in a `HashMap`) an index `0 .. coordinateCount - 1`, compiling each unique coordinate once.
- **Point evaluation**: allocates `cachedValues[0..coordinateCount-1]` filled with `NaN`. Whenever a spline node requests coordinate `k`, returns `cachedValues[k]` if `!isNaN(cachedValues[k])`, otherwise evaluates `coordSampler[k].sampleValue(context, bx, by, bz)`, caches it, and returns it.
- **Volume evaluation**: allocates lazy `buffers[0..coordinateCount-1]` (initially empty). Loops `i = 0 .. outputBuffer.size() - 1` evaluating the spline; the first time any voxel touches coordinate `k`, it acquires a buffer and runs `coordSampler[k].sampleVolume(context, buf, volume)`, then reads `buf[i]`.

#### Exact `f32` evaluation (`CubicSpline.Multipoint.sample`)
Given multipoint node `(coord, locations[0..n-1], values[0..n-1], derivatives[0..n-1])` with `last = n - 1`:
1. Evaluate `u = coord(input)` in `f32`.
2. Locate segment `start = Mth.binarySearch(0, n, i -> u < locations[i]) - 1` (exact lower-bound search per `java-math-helpers.md` §3; note if `u` is `NaN`, `u < locations[i]` is always false so `start = last`).
3. **Left extrapolation** (`start < 0`): evaluate `y0 = sample(values[0])`; if `derivatives[0] == 0.0f` return `y0`, else `y0 + derivatives[0] * (u - locations[0])`.
4. **Right extrapolation** (`start == last`): evaluate `yL = sample(values[last])`; if `derivatives[last] == 0.0f` return `yL`, else `yL + derivatives[last] * (u - locations[last])`.
5. **Interior cubic Hermite segment** (`0 <= start < last`):
   - `x1 = locations[start]`, `x2 = locations[start + 1]`
   - `t = (u - x1) / (x2 - x1)`
   - `y1 = sample(values[start])`, `y2 = sample(values[start + 1])` (evaluated in this order!)
   - `d1 = derivatives[start]`, `d2 = derivatives[start + 1]`
   - `a = d1 * (x2 - x1) - (y2 - y1)`
   - `b = (-d2) * (x2 - x1) + (y2 - y1)`
   - Return `Mth.lerp(t, y1, y2) + (t * (1.0f - t)) * Mth.lerp(t, a, b)` (all `f32`, left-to-right).

#### Range (`CubicSpline.range()`)
- `Constant(v)`: `[v, v]`.
- `Multipoint`: exact conservative Hermite bound algorithm as specified in `docs/specs/math/java-math-helpers.md` §4 (returns `NaI` if `coord.range()` is `NaI`; otherwise accumulates linear extensions at `inputRange.min()` if `< loc[0]` and `inputRange.max()` if `> loc[last]`, all child ranges, and the `0.25f` overshoot terms for every segment with `d1 != 0 || d2 != 0`).

---

### 4. Seeding and `RandomState` initialization

Given `(noiseRegistry: HolderGetter<NormalNoise>, worldSeed: i64, useLegacyRandom: bool, defaultBlock, seaLevel, noiseRouter)`:

1. **Root positional factory (`random`)**:
   - Select algorithm: `LEGACY` (48-bit LCG) if `useLegacyRandom` is true, else `XOROSHIRO` (xoroshiro128++).
   - Construct `rootRandom = algorithm.newInstance(worldSeed).forkPositional()`.
   - (For `LEGACY`: LCG seeded with `worldSeed`, then `forkPositional()` draws one `nextLong()` as the factory salt $P$. For `XOROSHIRO`: upgraded 128-bit xoroshiro seeded with `worldSeed`, then `forkPositional()` draws `(L, H) = (nextLong(), nextLong())`. See `docs/specs/math/random.md`.)
2. **Normal noise instantiation (`CompileContext.createNoiseSampler(Holder<NormalNoise>)`)**:
   - If the holder's key is **`minecraft:nether/temperature`**: instantiate via **legacy Nether biome** path (`noise.md` §7) with a fresh `LegacyRandomSource(worldSeed + 0)` (not cached in `noiseInstances`).
   - Else if the holder's key is **`minecraft:nether/vegetation`**: instantiate via **legacy Nether biome** path (`noise.md` §7) with a fresh `LegacyRandomSource(worldSeed + 1)` (not cached in `noiseInstances`).
   - Else: unwrap the holder's `ResourceKey<NormalNoise>` (throws if inline/unregistered!) and look up / create in `noiseInstances`:
     - Derive RNG `r = rootRandom.fromHashOf("<namespace>:<path>")` (using the canonical identifier string of the noise's registry key).
     - Instantiate via standard double-Perlin instantiation (`noise.md` §7) on `r`.
3. **Named RNG instantiation (`CompileContext.createRandom(Identifier id)`)**:
   - Used by `minecraft:old_blended_noise` with `id = "minecraft:terrain"`.
   - If `useLegacyRandom && id.equals("minecraft:terrain")`: return a fresh **`LegacyRandomSource(worldSeed)`** directly (bypassing `rootRandom`!).
   - Otherwise: return `rootRandom.fromHashOf(id.toString())`.
4. **End island RNG (`CompileContext.createEndIslandRandom()`)**:
   - Always returns a fresh **`LegacyRandomSource(worldSeed)`** regardless of `useLegacyRandom`.
5. **Named positional factories (`getOrCreateRandomFactory(Identifier name)`)**:
   - Memoized map: `rootRandom.fromHashOf(name.toString()).forkPositional()`.
   - Used for aquifers (`"minecraft:aquifer"`) and material rules / ore veins (detailed in `aquifers-ores-surface-carvers.md`).
6. **`MaterialSystem` initialization**:
   - Eagerly instantiates surface/clay-band/badlands/iceberg noises via `getOrCreateNoise(...)` and clay bands from `rootRandom.fromHashOf("minecraft:clay_bands")` (these are independent positional hashes, so order does not affect density-function noises).

---

### 5. `NoiseRouter`, `NoiseSettings`, and `NoiseGeneratorSettings`

#### 5.1 `NoiseRouter` (8 fields in 26.3)
JSON object inside `noise_router` (all 8 fields required `DensityFunction`s):

| JSON field | Downstream consumers |
|---|---|
| `temperature` | `Climate.Sampler` (multi-noise biome selection, sampled at block coord `quart << 2`, quantized via `(i64)(v * 10000.0f)`) |
| `vegetation` | `Climate.Sampler` (humidity axis for biome selection) |
| `continents` | `Climate.Sampler` (continentalness axis for biome selection) |
| `erosion` | `Climate.Sampler` (erosion axis for biome selection; in End, holds `end/islands`) |
| `depth` | `Climate.Sampler` (depth axis for biome selection) |
| `ridges` | `Climate.Sampler` (weirdness axis for biome selection) |
| `chunk_surface_level` | Passed to `MaterialSystem` as the preliminary surface level density function for surface-rule height checks |
| `final_density` | Evaluated over the chunk volume in `NoiseBasedChunkGenerator.doFill` (and `iterateNoiseColumn` for `getBaseHeight`/`getBaseColumn`) to determine solid vs. non-solid blocks |

> **26.3 structural changes**: `barrier`, `fluid_level_floodedness`, `fluid_level_spread`, and `lava` moved from `NoiseRouter` into `NoiseGeneratorSettings.aquifers` (`Aquifer.Config`). `initial_density_without_jaggedness` was replaced by `chunk_surface_level` (on `NoiseRouter`) and `surface_level` (on `Aquifer.Config`). `vein_toggle`, `vein_ridged`, and `vein_gap` moved out of `NoiseRouter` into registered density functions (`overworld/ore_vein/*`) referenced by `OreVeinRule` inside `material_rule`.

#### 5.2 `NoiseSettings` (2 fields in 26.3)
JSON object inside `noise`:

| JSON field | Type / valid range | Validation rules |
|---|---|---|
| `min_y` | `i32` in `[-2032, 2031]` (`DimensionType.MIN_Y..MAX_Y`) | Must be a multiple of 16 (`min_y % 16 == 0`). |
| `height` | `i32` in `[0, 4064]` (`0..DimensionType.Y_SIZE`) | Must be a multiple of 16 (`height % 16 == 0`), and `min_y + height <= 2032` (`MAX_Y + 1`). |

Height clamping against a `LevelHeightAccessor` (`clampToHeightAccessor`):
- `newMinY = max(this.minY, accessor.getMinY())`
- `newHeight = min(this.minY + this.height, accessor.getMaxY() + 1) - newMinY`
(Note: `newHeight` can be `<= 0` if the ranges do not overlap; callers skip generation when `newHeight <= 0`.)

#### 5.3 `NoiseGeneratorSettings` (`minecraft:worldgen/noise_settings`)
Dynamic registry element schema (codec `DIRECT_CODEC`; holders accept registry reference or inline object):

| JSON field | Type | Required / Default | Purpose |
|---|---|---|---|
| `noise` | `NoiseSettings` | required | Generation vertical bounds `(min_y, height)` |
| `default_block` | `BlockState` | required | Fallback solid block when `Aquifer.computeSubstance` returns `null` (i.e. `density > 0.0`) |
| `default_fluid` | `BlockState` | required | Sea-level fluid block for `globalFluidPicker` |
| `noise_router` | `NoiseRouter` | required | The 8 routed density functions |
| `material_rule` | `Holder<MaterialRule>` | required | Surface, bedrock, and ore-vein replacement rule (replaces `surface_rule`) |
| `spawn_target` | `List<SpawnTargetPoint>` | required | Target parameter boxes for `NoiseSpawnFinder` (may be empty `[]`) |
| `sea_level` | `i32` | required | Sea level Y (fluid occupies `y < sea_level`) |
| `disable_mob_generation` | `bool` | required | Disables initial chunk mob spawning |
| `aquifers` | `Aquifer.Config` | optional (absent = disabled) | If present, enables `NoiseBasedAquifer`; if absent, uses `Aquifer.createDisabled` |
| `legacy_random_source` | `bool` | required | `true` $\to$ 48-bit LCG `RandomState` + legacy `BlendedNoise` seeding; `false` $\to$ xoroshiro128++ |
| `debug_functions` | `List<{label: String, function: DensityFunction}>` | optional, default `[]` | F3 debug screen readout entries (no gameplay/terrain effect) |

**`Aquifer.Config` JSON schema** (all 6 fields required `DensityFunction`s):
- `barrier`, `fluid_level_floodedness`, `fluid_level_spread`, `lava`, `exclusion`, `surface_level`.

**`SpawnTargetPoint` JSON schema**:
- JSON map from `Holder<DensityFunction>` (reference identifier in `minecraft:worldgen/density_function`) to `Climate.Parameter` (a single `f32`, 2-element array `[min, max]`, or `{"min": f32, "max": f32}`, each `f32` in `[-2.0, 2.0]` with `min <= max`, quantized via `(i64)(v * 10000.0f)`).
- Fitness at `(bx, by, bz)`: sum of `Mth.square(param.distance(quantizeCoord(sampler.sampleValue(bx, by, bz))))` across entries. Used by `NoiseSpawnFinder` (radial search at radii `[512..2048]` step `512` then `[32..512]` step `32` with distance bias `x² + z²`, fitness `minFitness * 2048² + (x² + z²)`).

---

### 6. `NoiseChunk`, trilinear interpolation, and block state hooks

#### 6.1 `NoiseChunk` construction and lifecycle
Constructed per chunk (or per 1×H×1 column in `iterateNoiseColumn`) with `(randomState, beardifier, settings, globalFluidPicker, blender, volume)`:
1. Acquire a `DensityBufferPool` from `randomState`.
2. Build `SamplerContext` user fields:
   - Always bind `Beardifier.CONTEXT_KEY -> beardifier` (if `null`, `getFieldOrDefault` falls back to constant `0.0f`).
   - If `!blender.isEmpty()`: precompute `blendBuffer = blender.blendOffsetAndFactor(volume)` and bind `Blender.CONTEXT_KEY -> blender`, `Blender.ALPHA_KEY -> blendBuffer.createAlphaSampler()`, `Blender.OFFSET_KEY -> blendBuffer.createOffsetSampler()`.
3. Create `cachingSamplers = randomState.samplersWithContext(SamplerContext.builder().setUserFields(...).useBufferArena(bufferPool).enableCaches().build())`.
4. Create `aquifer`:
   - If `settings.aquifers().isPresent()`: instantiate `NoiseBasedAquifer(cachingSamplers, config, randomState.getOrCreateRandomFactory("minecraft:aquifer"), volume, globalFluidPicker)`. (Note: `NoiseBasedAquifer`'s constructor immediately evaluates `config.surfaceLevel` over a quart-stepped 2D `DensityVolume` covering the aquifer grid to compute `skipSamplingAboveY` and warm `surfaceLevelCache`.)
   - Else: instantiate `Aquifer.createDisabled(globalFluidPicker)`.
5. On `close()`: return the `DensityBufferPool` to `randomState`.

#### 6.2 Trilinear interpolation (`InterpolatedFunction`)
In 26.3, cell dimensions `(cellSizeXz, cellSizeY)` are parameters of each `minecraft:interpolated` node (vanilla Overworld/Nether/Caves use `(4, 8)`, End/Floating Islands use `(8, 4)`, `chunk_surface_level` uses `(16, 1)`). Precomputed reciprocals are `invXZ = 1.0f / (f32) cellSizeXz` and `invY = 1.0f / (f32) cellSizeY`.

##### Volume path (`sampleVolume`)
Three branches:
1. **Aligned cell-grid passthrough**: if `(stepX == cellSizeXz || sizeX == 1)` AND `(stepY == cellSizeY || sizeY == 1)` AND `(stepZ == cellSizeXz || sizeZ == 1)` AND `floorMod(minBlockX, cellSizeXz) == 0` AND `floorMod(minBlockY, cellSizeY) == 0` AND `floorMod(minBlockZ, cellSizeXz) == 0`:
   - Delegate directly to `input.sampleVolume(context, outputBuffer, volume)` (no interpolation needed because every sample point is a cell corner).
2. **Unit block step** (`stepX == 1 && stepY == 1 && stepZ == 1`):
   - Compute covering cell bounds:
     - `minCellX = floorDiv(minBlockX, cellSizeXz)`, `maxCellX = floorDiv(maxBlockX, cellSizeXz)`, `cellCountX = maxCellX - minCellX + 1` (likewise Y with `cellSizeY`, Z with `cellSizeXz`).
   - Build cell-corner volume `cellVolume`:
     - `sizeX = (floorMod(maxBlockX, cellSizeXz) == 0) ? cellCountX : cellCountX + 1` (likewise Y, Z).
     - `minBlock = (minCellX * cellSizeXz, minCellY * cellSizeY, minCellZ * cellSizeXz)`.
     - `step = (cellSizeXz, cellSizeY, cellSizeXz)`.
   - Sample `input.sampleVolume(context, cellBuffer, cellVolume)`.
   - Walk cells: `cellZ ∈ [0, cellCountZ)`, `cellX ∈ [0, cellCountX)`, `cellY ∈ [0, cellCountY)`. For each cell, clamp `nextCell* = min(cell* + 1, cellVolume.size* - 1)` and fetch the 8 corner floats `v000, v100, v010, v110, v001, v101, v011, v111` (where bit order is `v<x><y><z>`).
   - **Exact `fillCell` interpolation order (Z $\to$ X $\to$ incremental Y, all `f32`)**:
     - Local clip bounds within the cell:
       - `offX = cellVolume.blockX(cellX) - volume.minBlockX()` (likewise Y, Z).
       - `x0 = max(0, -offX)`, `x1 = min(cellSizeXz, volume.sizeX() - offX) - 1` (likewise Y, Z).
     - Outer loop `z = z0 .. z1`:
       - `alphaZ = (f32) z * invXZ`
       - `v00_ = Mth.lerp(alphaZ, v000, v001)`
       - `v01_ = Mth.lerp(alphaZ, v010, v011)`
       - `v10_ = Mth.lerp(alphaZ, v100, v101)`
       - `v11_ = Mth.lerp(alphaZ, v110, v111)`
     - Middle loop `x = x0 .. x1`:
       - `alphaX = (f32) x * invXZ`
       - `v_0_ = Mth.lerp(alphaX, v00_, v10_)`
       - `v_1_ = Mth.lerp(alphaX, v01_, v11_)`
       - `valueStep = (v_1_ - v_0_) * invY`
       - `value = v_0_ + valueStep * (f32) y0`
     - Inner loop `y = y0 .. y1`:
       - Write `outputBuffer[index(offX + x, offY + y, offZ + z)] = value`.
       - Update **`value += valueStep`** (repeated `f32` addition, NOT `Mth.lerp(y * invY, v_0_, v_1_)`!).
3. **Non-unit step** (neither passthrough nor unit step):
   - Allocates a full unit-step `blockVolume` of size `(sizeX * stepX, sizeY * stepY, sizeZ * stepZ)` at `(minBlockX, minBlockY, minBlockZ)`, runs branch 2 into it, and subsamples `(x * stepX, y * stepY, z * stepZ)` into `outputBuffer`.

##### Point path (`sampleValue`)
1. `xIn = floorMod(bx, cellSizeXz)`, `yIn = floorMod(by, cellSizeY)`, `zIn = floorMod(bz, cellSizeXz)`.
2. If `xIn == 0 && yIn == 0 && zIn == 0`: return `input.sampleValue(context, bx, by, bz)`.
3. Otherwise build a `2 × 2 × 2` corner volume at `(bx - xIn, by - yIn, bz - zIn)` with steps `(cellSizeXz, cellSizeY, cellSizeXz)`, evaluate `input.sampleVolume` into a temp buffer, and return:
   $$\text{Mth.lerp3}\!\left(\frac{\text{f32}(x_{\text{in}})}{\text{f32}(\text{cellSizeXz})},\; \frac{\text{f32}(y_{\text{in}})}{\text{f32}(\text{cellSizeY})},\; \frac{\text{f32}(z_{\text{in}})}{\text{f32}(\text{cellSizeXz})},\; c_{000}, c_{100}, c_{010}, c_{110}, c_{001}, c_{101}, c_{011}, c_{111}\right)$$
   - Note two differences vs. `fillCell`: (a) `Mth.lerp3` interpolates **X innermost, Y middle, Z outermost**, whereas `fillCell` interpolates **Z $\to$ X $\to$ incremental Y**; (b) `sampleValue` divides `(f32) xIn / cellSizeXz`, whereas `fillCell` multiplies `(f32) x * (1.0f / cellSizeXz)` (differs for non-power-of-two cell sizes).

#### 6.3 Block-state resolution & downstream interfaces
During `doFill`, each voxel's `f32` `final_density` is widened to `f64` and passed to `Aquifer.computeSubstance(blockX, blockY, blockZ, (f64) density) -> Option<BlockState>`:
- **`globalFluidPicker`** (built once per `NoiseBasedChunkGenerator`):
  - `lavaStatus = FluidStatus(-54, minecraft:lava)`
  - `seaStatus = FluidStatus(settings.sea_level, settings.default_fluid)`
  - At `(x, y, z)`: if `y < min(-54, settings.sea_level)` return `lavaStatus`, else `seaStatus`. (When `DEBUG_DISABLE_FLUID_GENERATION` is true, returns `FluidStatus(-4064, minecraft:air)`; off in normal gameplay.)
  - `FluidStatus.at(blockY)` returns `fluidType` if `blockY < fluidLevel`, else `minecraft:air`.
- **Disabled aquifer** (`settings.aquifers` absent):
  - `computeSubstance`: if `density > 0.0` return `null` (solid); else return `globalFluidPicker.computeFluid(bx, by, bz).at(by)`.
  - `shouldScheduleFluidUpdate()`: always `false`.
- **Enabled aquifer** (`NoiseBasedAquifer`, internals specified in `aquifers-ores-surface-carvers.md`):
  - If `density > 0.0`: sets `shouldScheduleFluidUpdate = false` and returns `null`.
  - Otherwise computes fluid/air/barrier block state and sets `shouldScheduleFluidUpdate`.
- **Fallback**: when `computeSubstance` returns `null`, `doFill` uses `settings.default_block`.
- **Ore veins & surface replacement**: NOT executed inside `doFill`. In 26.3, ore veins (`OreVeinRule`) are part of `MaterialRule`, evaluated in `buildSurface` immediately after `doFill` using the same `NoiseChunk` (sharing `cachingSamplers`, `volume` narrowed vertically to the highest non-empty section `maxSectionY * 16 + 15`, and `DensityBufferPool`). Carvers (`generateCarvers`) run third, also sharing the same `NoiseChunk` (`aquifer` and `cachingSamplers`).

---

### 7. Terrain fill (`ChunkStatus.TERRAIN` / `doFill`)

In `ChunkPyramid.GENERATION_PYRAMID`, the `TERRAIN` step requires `STRUCTURE_STARTS` (radius 8) and `BIOMES` (radius 1) and executes `ChunkStatusTasks.buildTerrain`:

1. Collect `possibleBiomes` from the `3 × 3` chunk neighborhood (radius 1) around the target chunk.
2. Call `NoiseBasedChunkGenerator.buildTerrain(chunk, Blender.of(region), randomState, structureManager, biomeManager, region, possibleBiomes)`:
   1. Clamp `noiseSettings = settings.noiseSettings().clampToHeightAccessor(chunk.getHeightAccessorForGeneration())`.
   2. If `noiseSettings.height() <= 0` (or debug void terrain flag enabled), return `chunk` immediately without modifying blocks.
   3. Create chunk `DensityVolume(16, noiseSettings.height(), 16, chunkPos.getMinBlockX(), noiseSettings.minY(), chunkPos.getMinBlockZ())` (unit steps).
   4. Build `Beardifier.forStructuresInChunk(structureManager, chunk.getPos())` and open `NoiseChunk`.
   5. Acquire chunk sections from `topSectionIndex = chunk.getSectionIndex(volume.maxBlockY())` down to `bottomSectionIndex = chunk.getSectionIndex(volume.minBlockY())` inclusive.
   6. **Execute `doFill(noiseChunk, chunk)`**:
      1. Obtain unprimed heightmaps `OCEAN_FLOOR_WG` (opaque predicate: `#minecraft:blocks_motion_in_heightmap`) and `WORLD_SURFACE_WG` (opaque predicate: `!state.isAir()`).
      2. Evaluate `finalDensity = noiseChunk.cachingSamplers().get(settings.noiseRouter().finalDensity()).sampleVolume(volume)` for the entire `16 × height × 16` chunk at once.
      3. Walk every block in exact order:
         - Outer: `z = 0 .. 15` (`blockZ = minBlockZ + z`)
         - Middle: `x = 0 .. 15` (`blockX = minBlockX + x`)
         - Inner: **`y = volume.sizeY() - 1` down to `0`** (top-to-bottom! `blockY = minY + y`)
      4. For each `(x, y, z)`:
         - Read `density = densityBuffer.get(volume.indexUnchecked(x, y, z))`.
         - `state = aquifer.computeSubstance(blockX, blockY, blockZ, (f64) density)`.
         - If `state == null`, `state = settings.defaultBlock()`.
         - If `state != Blocks.AIR.defaultBlockState()` (skip default `minecraft:air` only):
           - Write `section.setBlockState(x, blockY & 15, z, state, false)` into section `chunk.getSectionIndex(blockY)`.
           - Call `oceanFloor.update(x, blockY, z, state)`.
           - Call `worldSurface.update(x, blockY, z, state)`.
           - If `aquifer.shouldScheduleFluidUpdate() && !state.getFluidState().isEmpty()`: mark `(blockX, blockY, blockZ)` for chunk post-processing.
   7. Release chunk sections.
   8. Run `buildSurface(chunk, noiseChunk, randomState, biomeManager, possibleBiomes, settings.materialRule().value())`.
   9. Run `generateCarvers(chunk, blender, noiseChunk, randomState, biomeManager, carverBiomeRegion, materialRule)`.
   10. Close `NoiseChunk`.
3. Post-`buildTerrain` completion (in `ChunkStatusTasks.buildTerrain`):
   - If the chunk is a `ProtoChunk` with `BelowZeroRetrogen` (upgrading pre-1.18 chunks only): replace old bedrock and apply bedrock hole mask.
   - Prime heightmaps `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`, and `WORLD_SURFACE`.

#### Single-column queries (`getBaseHeight`, `getBaseColumn`)
`iterateNoiseColumn(heightAccessor, randomState, blockX, blockZ, ...)` clamps `noiseSettings`, constructs a `1 × height × 1` `DensityVolume(1, height, 1, blockX, minY, blockZ)`, opens a `NoiseChunk` with `beardifier = null` and `blender = Blender.empty()`, samples `final_density` over that column volume, and scans `y = height - 1` down to `0` calling `aquifer.computeSubstance` (defaulting `null` to `defaultBlock`). `getBaseHeight` returns `blockY + 1` for the highest block matching `heightmapType.isOpaque()`, or `heightAccessor.getMinY()` if none matches.

---

### 8. Structure terrain adaptation (`Beardifier`)

`Beardifier` is a `DensitySampler` injected under `Beardifier.CONTEXT_KEY` (read by `minecraft:beardifier`).

#### Precomputed 24³ kernel (`BEARD_KERNEL`)
Static `f32[13824]` (`24 × 24 × 24`), indexed as `zi * 576 + xi * 24 + yi` for `xi, yi, zi ∈ [0, 23]`:
- Let `dx = xi - 12`, `dy = (f64)(yi - 12) + 0.5`, `dz = zi - 12`.
- `distSq = (f64)(dx * dx) + dy * dy + (f64)(dz * dz)` (left-to-right `f64`).
- `BEARD_KERNEL[idx] = (f32) Math.pow(Math.E, -distSq / 16.0)` (uses `Math.pow(Math.E, ...)`, NOT `Math.exp(...)`).

#### Piece and junction collection (`forStructuresInChunk(structureManager, chunkPos)`)
1. Query all `StructureStart`s overlapping `(chunkX, chunkZ)` whose `terrainAdaptation() != NONE`. If empty, return `Beardifier.EMPTY`.
2. Let `cx = chunkPos.getMinBlockX()`, `cz = chunkPos.getMinBlockZ()`. Maintain lists `rigids`, `junctions`, and optional `anyPieceBox`.
3. For each `StructureStart` and each `StructurePiece` in `start.getPieces()`:
   - Skip piece unless its bounding box intersects `[cx - 12, cz - 12, cx + 27, cz + 27]` (inclusive on all edges).
   - If the piece is a `PoolElementStructurePiece`:
     - If `projection == RIGID`: add `Rigid(piece.boundingBox, terrainAdjustment, piece.groundLevelDelta)` and expand `anyPieceBox` to encapsulate `piece.boundingBox`.
     - For every `JigsawJunction` on the piece: if `jx > cx - 12 && jz > cz - 12 && jx < cx + 27 && jz < cz + 27` (**strict** inequalities), add `junction` and expand `anyPieceBox` to include the 1×1×1 box `(jx, sourceGroundY, jz)`.
   - Else (non-pool piece): add `Rigid(piece.boundingBox, terrainAdjustment, 0)` and expand `anyPieceBox` with `piece.boundingBox`.
4. If `anyPieceBox` is null, return `Beardifier.EMPTY`. Otherwise set `affectedBox = anyPieceBox.inflatedBy(24)` (each min `- 24`, each max `+ 24`).

#### Evaluation
- **Point (`sampleValue`)**: if `affectedBox == null` or `(bx, by, bz)` is outside `affectedBox`, return `0.0f`; else `sampleValueUnchecked(bx, by, bz)`.
- **Volume (`sampleVolume`)**: fill `outputBuffer` with `0.0f`. If `affectedBox != null` and `volume.intersects(affectedBox)`:
  - `minX = floorDiv(max(0, affectedBox.minX - volume.minBlockX), stepX)` (likewise Y, Z).
  - `maxX = min(volume.sizeX - 1, floorDiv(affectedBox.maxX - volume.minBlockX, stepX))` (likewise Y, Z).
  - Loop `z = minZ..maxZ`, `x = minX..maxX`, `y = minY..maxY`, setting `outputBuffer[index(x, y, z)] = sampleValueUnchecked(blockX(x), blockY(y), blockZ(z))`.
- **`sampleValueUnchecked(bx, by, bz)`**: start `total = 0.0f` (`f32`):
  1. For each `Rigid(box, adjustment, groundLevelDelta)` in insertion order:
     - `dx = max(0, max(box.minX - bx, bx - box.maxX))`
     - `dz = max(0, max(box.minZ - bz, bz - box.maxZ))`
     - `groundY = box.minY + groundLevelDelta`, `dyToGround = by - groundY`
     - `dy`: `0` for `NONE`; `dyToGround` for `BURY`/`BEARD_THIN`; `max(0, max(groundY - by, by - box.maxY))` for `BEARD_BOX`; `max(0, max(box.minY - by, by - box.maxY))` for `ENCAPSULATE`.
     - Add to `total` (`f32`):
       - `NONE`: `0.0f`
       - `BURY`: `bury(dx, dy / 2.0f, dz)`
       - `BEARD_THIN` or `BEARD_BOX`: `beard(dx, dy, dz, dyToGround) * 0.8f`
       - `ENCAPSULATE`: `bury(dx / 2.0f, dy / 2.0f, dz / 2.0f) * 0.8f`
  2. For each `JigsawJunction` in insertion order:
     - `dx = bx - junction.sourceX`, `dy = by - junction.sourceGroundY`, `dz = bz - junction.sourceZ`.
     - Add `beard(dx, dy, dz, dy) * 0.4f` to `total`.
  3. Return `total`.

Helper formulas (all `f32` unless noted):
- **`bury(dx, dy, dz)`**: `d2 = (dx*dx + dy*dy) + dz*dz`; if `d2 >= 36.0f` return `0.0f`; else `1.0f - Mth.sqrt(d2) / 6.0f`.
- **`beard(dx: i32, dy: i32, dz: i32, yToGround: i32)`**:
  - `xi = dx + 12`, `yi = dy + 12`, `zi = dz + 12`.
  - If any of `xi, yi, zi` is outside `[0, 23]` (so valid signed offset range is `[-12, +11]`), return `0.0f`.
  - `dyOff = (f32) yToGround + 0.5f`.
  - `d2 = ((f32) dx * (f32) dx + dyOff * dyOff) + (f32) dz * (f32) dz`.
  - `val = (-dyOff * (f32) Mth.fastInvSqrt((f64)(d2 / 2.0f))) / 2.0f` (uses 64-bit `Mth.fastInvSqrt` from `java-math-helpers.md`).
  - Return `val * BEARD_KERNEL[zi * 576 + xi * 24 + yi]`.

---

### 9. Old-chunk terrain blending (`Blender` & `BlendingData`)

Blending **only activates when generating new chunks adjacent to pre-existing old-format chunks** carrying `BlendingData` (chunks with `isOldChunkAround(centerPos, 7)` true). For freshly generated worlds, `Blender.of(region)` returns `Blender.EMPTY`, so `blend_alpha = 1.0f`, `blend_offset = 0.0f`, and `blend_density(x) = x`.

#### Constants and shift noise
- `HEIGHT_BLENDING_RANGE_CELLS = 27` (`7 * 4 - 1` quarts), `HEIGHT_BLENDING_RANGE_CHUNKS = 7`.
- `DENSITY_BLENDING_RANGE_CELLS = 2`, `DENSITY_BLENDING_RANGE_CHUNKS = 1`.
- `SHIFT_NOISE`: `NormalNoise.createParity(-3, 1.0, 1.0, 1.0, 0.0)` instantiated with `new XoroshiroRandomSource(42L)`.

#### `Blender.of(region)` neighborhood gathering
- If `!region.isOldChunkAround(centerPos, 7)`, return `EMPTY`.
- Iterate `dx ∈ [-7, 7]`, `dz ∈ [-7, 7]` with `dx² + dz² <= 64` (`(7 + 1)²`):
  - Call `BlendingData.getOrUpdateBlendingData(region, centerX + dx, centerZ + dz)`.
  - If non-null, store in `heightAndBiomeData[chunkPos]`, and if `|dx| <= 1 && |dz| <= 1`, also store in `densityData[chunkPos]`.
- Build optional `carvingFilter` from the center chunk and its 8 neighbors (`Direction8`) that have `BlendingData`. If all three are empty/null, return `EMPTY`.

#### `BlendingData` border-column layout and extraction
Each old chunk stores `CELL_COLUMN_COUNT = 16` border quart-columns (`7` "inside" along north/west edges `x == 0 || z == 0`, indexed `3 - x + z` for `x, z ∈ [0, 3]`; `9` "outside" along east/south edges `x == 4 || z == 4`, indexed `7 + x + 4 - z` for `x, z ∈ [0, 4]`):
- Reverse index decoding for `i ∈ [0, 15]`:
  - If `i < 7`: `x = max(0, 3 - i)`, `z = max(0, i - 3)`.
  - Else (`j = i - 7`): `x = 4 - max(0, 4 - j)`, `z = 4 - max(0, j - 4)`.
- When `calculateData(chunk, newSides)` runs once (for neighbor sides where `isOldNoiseGeneration() == false`):
  - Populates inside `(0,0)` (block `0,0`) if N, W, or NW is new; `(1..3, 0)` (blocks `4i, 0`) if N; `(0, 1..3)` (blocks `0, 4i`) if W; outside `(4, 1..3)` (blocks `15, 4i`) if E; `(0..3, 4)` (blocks `4i, 15`) if S; `(4, 0)` (block `15, 0`) if E and NE; `(4, 4)` (block `15, 15`) if E, S, and SE.
- **Height extraction (`heights[idx]`)**: if unset (`Float.MAX_VALUE`), scan downward from `min(WORLD_SURFACE_WG(bx, bz), oldMaxY)` (or `oldMaxY` if unprimed) down to `oldMinY + 1`, returning the first Y whose block is in `{podzol, gravel, grass_block, stone, coarse_dirt, sand, red_sand, mycelium, snow_block, terracotta, dirt}`, else `oldMinY`.
- **Density column (`densities[idx]`, length `sectionsCount * 2`, vertical cell size `8`)**:
  - Fill with `-1.0f`. Start cursor `pos = (bx, oldMaxY + 1, bz)` and step downward (`isGround`: not air, not `#leaves`, not `#logs`, not brown/red mushroom block, and non-empty collision shape $\to +1.0_f$, else $-1.0_f$).
  - Read 7 blocks (`last7`), then for `cellIndex = len - 2` down to `0`: read 1 (`one`) and 7 (`curr7`), set `densities[cellIndex] = (last7 + one + curr7) / 15.0f`, `last7 = curr7`.
  - Surface cell sharpening: let `sIdx = floorDiv(floor(height), 8) - (minSectionY * 2 + 1)`. If `0 <= sIdx < len - 1`:
    - `inCell = ((height + 0.5f) % 8.0f) / 8.0f` (**truncated** float `%`, NOT positive modulo!)
    - `amp = (1.0f - inCell) / inCell`, `m = max(amp, 1.0f) * 0.25f`
    - `densities[sIdx + 1] = -amp / m`, `densities[sIdx] = 1.0f / m`.
  - Stored values are scaled by `0.1f` on read (`getDensity` / `iterateDensities`). At `cellY == minSectionY * 2`, `getDensity` returns `0.1f`.
  - Note: `iterateDensities(minCellX, minCellZ, fromCellY, toCellY)` maps `minYIndex = max(0, fromCellY - columnMinY)` and `maxYIndex = min(len, toCellY - columnMinY)` with loop `yIndex < maxYIndex` (**exclusive** of `toCellY`, so `blendDensity` passing `(cellY - 1, cellY + 1)` only visits `cellY - 1` and `cellY`).

#### `blend_alpha` and `blend_offset`
- `NoiseChunk` precomputes a 2D quart grid covering `minCellX = volume.minBlockX >> 2` through `maxCellX = (volume.maxBlockX >> 2) + 1` inclusive (likewise Z), evaluating `blendOffsetAndFactor(cellX << 2, cellZ << 2)` for each quart. Both `ALPHA_KEY` and `OFFSET_KEY` samplers look up `(blockX >> 2, blockZ >> 2)` in this grid (falling back to live evaluation if out of bounds).
- Exact lookup (`getBlendingDataValue(cellX, cellY, cellZ, getter)`): queries `heightAndBiomeData` at `(chunkX, chunkZ) = (cellX >> 2, cellZ >> 2)`; if `Float.MAX_VALUE` and `(cellX & 3) == 0` / `(cellZ & 3) == 0`, checks `(chunkX - 1, chunkZ - 1)` (when both), then `(chunkX - 1, chunkZ)`, then `(chunkX, chunkZ - 1)`.
- **`blendOffsetAndFactor(blockX, blockZ)`**:
  1. `cellX = blockX >> 2`, `cellZ = blockZ >> 2`.
  2. If exact height `h != Float.MAX_VALUE`: return `(alpha = 0.0f, offset = (f32) heightToOffset(h))`.
  3. Otherwise iterate all stored height cells across `heightAndBiomeData`: let `d = sqrt((cellX - tx)² + (cellZ - tz)²)` in `f64`. Skip `d > 27.0`. Track `closest = min(closest, d)`, accumulate `w = 1.0 / (d*d*d*d)`, `weightedHeights += h * w`, `totalWeight += w` in `f64`.
  4. If `closest == +Inf`, return `(1.0f, 0.0f)`.
  5. `avgH = weightedHeights / totalWeight`, `a = Mth.clamp(closest / 28.0, 0.0, 1.0)`, `alpha = 3.0 * a * a - 2.0 * a * a * a`; return `((f32) alpha, (f32) heightToOffset(avgH))`.
- **`heightToOffset(height: f64) -> f64`**:
  - `targetY = height + 0.5`, `m = Mth.positiveModulo(targetY, 8.0)`.
  - Return `(32.0 * (targetY - 128.0) - 3.0 * (targetY - 120.0) * m + 3.0 * m * m) / (128.0 * (32.0 - 3.0 * m))`.

#### `blend_density(blockX, blockY, blockZ, noiseValue)`
1. `cellX = blockX >> 2`, `cellY = blockY / 8` (**truncating** integer division toward zero!), `cellZ = blockZ >> 2`.
2. If exact density `fd = getBlendingDataValue(cellX, cellY, cellZ, getDensity)` (note: queries `heightAndBiomeData`, not `densityData`!) `!= Float.MAX_VALUE`: return `fd`.
3. Otherwise iterate `densityData` with `iterateDensities(..., cellY - 1, cellY + 1)`: compute `d = sqrt((cellX - tx)² + ((cellY - ty) * 2)² + (cellZ - tz)²)` in `f64`. Skip `d > 2.0`. Track `closest`, accumulate `w = 1.0 / (d*d*d*d)`, `weightedDensities += dens * w`, `totalWeight += w`.
4. If `closest == +Inf`, return `noiseValue`.
5. `avgD = weightedDensities / totalWeight`, `alpha = Mth.clamp(closest / 3.0, 0.0, 1.0)`.
6. Return `(f32) Mth.lerp(alpha, avgD, (f64) noiseValue)`.

#### Biome blending & carving filter (summary)
- `blendBiome(qx, qy, qz)`: finds closest biome cell in `heightAndBiomeData` at `qy` within distance `<= 27.0f` (`f32`); computes `shift = SHIFT_NOISE.get(qx, 0.0, qz) * 12.0f`, `alpha = Mth.clamp((closest + shift) / 28.0f, 0.0f, 1.0f)`; returns old biome if `alpha <= 0.5f`, else `null` (use new biome).
- `carvingFilter(x, y, z)`: shifts `(x + 0.5, y + 0.5, z + 0.5)` by `SHIFT_NOISE.get(x,y,z)*4`, `SHIFT_NOISE.get(y,z,x)*4`, `SHIFT_NOISE.get(z,x,y)*4`; allows carving iff minimum Euclidean distance to any old chunk's 16×oldHeight×16 box is `< 4.0`.

---

## Edge cases and known quirks

1. **Volume vs. point trilinear interpolation in `interpolated`**: `fillCell` (volume path) lerps **Z first, then X, then incremental `value += valueStep` along Y** with `alpha = i * (1.0f / cellSize)`, whereas `sampleValue` (point path) calls `Mth.lerp3` (**X first, Y second, Z third**) with `(f32) i / cellSize`. Repeated `value += valueStep` accumulates `f32` rounding drift across the 8-block Y cell!
2. **Volume cache leaks volume-path rounding into point queries**: `SamplerContext.sampleValueCached` returns `cell.buffer.get(idx)` whenever a `cache` cell already holds a `DensityVolume` containing `(bx, by, bz)`. Therefore, evaluating `final_density` as a chunk volume populates `cache` cells with volume-path values that `Aquifer` and `MaterialSystem` later read via `sampleValue`.
3. **Single-point cache key wraps via `BlockPos.asLong` and skips `NaN`**: `sampleValueCached` keys its 1-entry point cache on 26/12/26-bit packed coordinates and treats `NaN` as uncached.
4. **Compiler `SliceUniformAxes` changes cached `DensityVolume` shapes**: Wrapping 2D/1D nodes in `SliceFunction(..., 0)` shrinks their sampled volumes to `size = 1, minBlock = 0` along uniform axes, both inside and outside `PreparedCache`.
5. **`div` by constant compiles to multiply by reciprocal**: `BinaryFunction(DIV, left, ConstantFunction(c))` compiles to `ConstMulSampler(left, 1.0f / c)` — differs by up to 1 ULP from `left / c` for non-powers-of-two.
6. **`mul` and `div` short-circuit on `left == 0.0f` ONLY in `sampleValue`**: Point evaluation returns `+0.0f` when `left == 0.0f` (even if `left == -0.0f` or `right` is `0`/`±Inf`/`NaN`), whereas `sampleVolume` computes `left * right` / `left / right` unconditionally.
7. **`min` and `max` volume comparisons ignore `NaN` on the right**: Volume loops use `if (right < left) out = right` / `if (right > left) out = right`, whereas `ConstMin`/`ConstMax`/`Min`/`Max` point paths call Java `Math.min`/`Math.max` (which propagate `NaN` and order `-0.0f < +0.0f`).
8. **`min` and `max` prune non-overlapping intervals at compile time**: If `left.range()` and `right.range()` are strictly disjoint (`max < min`), the dominated child is discarded completely.
9. **`pow` with constant exponents `±0.5, ±1, ±2, ±3` bypasses `Math.pow`**: Compiles to `f32` `Sqrt`/`identity`/`Square`/`Cube` (plus `1.0f / ...` for negative exponents), NOT `f64` `Math.pow`. When BOTH `base` and `exponent` are constant, the `base`-constant check wins first and uses `Math.pow`.
10. **`round` saturates to `i32`**: `RoundFunction(ROUND)` uses `Math.round(float) -> int`, saturating values outside `[-2^31, 2^31 - 1]` and mapping `NaN -> 0.0f`, whereas `floor`/`ceil`/`truncate` use `f64` `Math.floor`/`Math.ceil`.
11. **`NoiseFunction` inline `NormalNoise` rejection in `RandomState`**: Although `NormalNoise.CODEC` accepts inline noise objects, `RandomState`'s `CompileContext.createNoiseSampler` calls `parameters.unwrapKey().orElseThrow()` for all non-Nether noises, so unregistered inline noise definitions fail at compile time.
12. **`NoiseFunction` zero-shift equality check**: `shift_x == ConstantFunction(0.0f)` uses record equality (`Float.compare`), so `-0.0f` does not trigger the unshifted optimization (forcing the `Shifted*Sampler` point-wise `noise.get` loop instead of `addToVolume`).
13. **`shift_b` volume vs. point `4.0f` placement**: Volume mode passes amplitude `4.0f` into `noise.addToVolume` (multiplying per-octave amplitudes), whereas point mode multiplies the summed noise by `4.0f`.
14. **`end_outer_islands` uses truncating `/` and `%` on negative coordinates**: `blockX / 8`, `sectionX / 2`, and `sectionX % 2` truncate toward zero, creating symmetry artifacts around `x = 0` and `z = 0`.
15. **`end_outer_islands` always uses Legacy RNG**: `createEndIslandRandom()` always instantiates `LegacyRandomSource(worldSeed)` even when `legacy_random_source` is `false`.
16. **`old_blended_noise` legacy seeding bypasses `rootRandom`**: When `legacy_random_source` is `true`, `createRandom("minecraft:terrain")` returns `LegacyRandomSource(worldSeed)` directly instead of `rootRandom.fromHashOf("minecraft:terrain")`.
17. **Legacy Nether noises ignore `legacy_random_source` and bypass `noiseInstances` cache**: `minecraft:nether/temperature` and `minecraft:nether/vegetation` always instantiate via `createForLegacyNetherBiome` with `LegacyRandomSource(worldSeed + 0)` and `LegacyRandomSource(worldSeed + 1)`.
18. **`Beardifier` asymmetric kernel range `[-12, +11]` and `Math.pow(Math.E, ...)`**: The 24³ kernel covers offsets `-12..+11` (not `+12`), uses `Math.pow(Math.E, -d²/16)` instead of `Math.exp`, uses 64-bit `Mth.fastInvSqrt`, and filters `JigsawJunction`s with strict `<` / `>` bounds while filtering `StructurePiece`s with inclusive `intersects`.
19. **`Blender` truncating division and remainder quirks**: `blendDensity` computes `cellY = blockY / 8` (truncating toward zero, wrong for negative Y!), `BlendingData.getDensityColumn` computes `(height + 0.5f) % 8.0f` (truncating `%`, negative for negative `height`), `iterateDensities` excludes `toCellY`, and `blendDensity`'s exact-cell lookup reads `heightAndBiomeData` instead of `densityData`.
20. **`Blender` hash-map iteration order** **[oracle]**: `blendOffsetAndFactor` and `blendDensity` accumulate `f64` weighted sums in `Long2ObjectOpenHashMap` bucket order, which depends on fastutil's hash mixing and table resize history.
21. **JVM transcendental intrinsics** **[oracle]**: `UnaryFunction(LOG)`, `PowFunction` (`Math.pow`), `Beardifier` (`Math.pow(Math.E, ...)`), and `NoiseSpawnFinder` (`Math.sin`/`Math.cos`) call `java.lang.Math` (not `StrictMath`); confirm zero divergence vs. fdlibm/libm on target CPUs via oracle probes.
22. **`doFill` top-to-bottom Y order and `AIR` identity skip**: `doFill` walks `z` outer, `x` middle, `y` **descending** (critical because `Aquifer` caches grid anchors lazily and `Heightmap.update` expects top-down or updates incrementally), and skips writing `Blocks.AIR.defaultBlockState()` (avoiding unnecessary palette writes and heightmap calls for air).

---

## Constants and IDs

### Registry keys
- Built-in density function type registry: `minecraft:worldgen/density_function_type` (44 entries)
- Dynamic density function registry: `minecraft:worldgen/density_function`
- Dynamic noise settings registry: `minecraft:worldgen/noise_settings`
- Dynamic normal noise registry: `minecraft:worldgen/noise`

### Complete list of 44 `worldgen/density_function_type` IDs
`constant`, `blend_alpha`, `blend_offset`, `beardifier`, `noise`, `end_outer_islands`, `distance_to_point`, `gradient`, `shift_a`, `shift_b`, `shift`, `abs`, `square`, `cube`, `sqrt`, `half_negative`, `quarter_negative`, `reciprocal`, `negate`, `squeeze`, `log`, `sign`, `floor`, `round`, `ceil`, `truncate`, `add`, `sub`, `mul`, `div`, `min`, `max`, `pow`, `spline`, `lerp`, `clamp`, `range_choice`, `interval_select`, `cache`, `blend_density`, `interpolated`, `slice`, `find_top_surface`, `old_blended_noise`.

### Exact numeric constants

| Constant | Type | Value | IEEE-754 hex / notes |
|---|---|---|---|
| `MAX_REASONABLE_NOISE_VALUE` | `f32` | `1000000.0` | `0x49742400` (`NOISE_VALUE_CODEC` bounds `[-1e6, 1e6]`) |
| `DimensionType.Y_SIZE` | `i32` | `4064` | `(1 << 12) - 32` |
| `DimensionType.MIN_Y` / `MAX_Y` | `i32` | `-2032` / `2031` | `find_top_surface.lower_bound` range `[-4064, 4062]` |
| `Density.SURFACE` / `UNRECOVERABLY_DENSE` / `UNRECOVERABLY_THIN` | `f32` | `0.0` / `64.0` / `-64.0` | — |
| `ShiftNoiseFunction` coordinate / value scale | `f64` / `f32` | `0.25` / `4.0` | Exact powers of two |
| `EndIslandFunction` threshold / RNG skip | `f32` / `i32` | `-0.9` (`0xBF666666`) / `17292` | `ISLAND_CHUNK_DISTANCE_SQR = 4096L` |
| `Climate.QUANTIZATION_FACTOR` | `f32` | `10000.0` | `0x461C4000` |
| `NoiseSpawnFinder.MAX_RADIUS` | `i64` | `2048` | Radii `(2048, step 512)` then `(512, step 32)` |
| `NoiseBasedChunkGenerator` deep lava level | `i32` | `-54` | `y < min(-54, sea_level)` |
| `Beardifier` kernel size / radius / bury max dist | `i32` / `i32` / `f32` | `24` (13,824 entries) / `12` / `6.0` | `affectedBox` inflation = `24`; rigid scale `0.8f`, junction scale `0.4f` |
| `Blender` height / density cell & chunk ranges | `i32` | `27` cells (`7` chunks) / `2` cells (`1` chunk) | Shift noise seed `42L` (xoroshiro) |
| `BlendingData` cell width / height / column count / factor | `i32` / `f32` | `4` / `8` / `16` / `0.1f` (`0x3DCCCCCD`) | `NO_VALUE = Float.MAX_VALUE` (`0x7F7FFFFF`) |
| `NoiseRouterData.GLOBAL_OFFSET` | `f32` | `-0.50375` | `0xBF00F5C3` |
| `NoiseRouterData.NOISE_ZERO` / `SURFACE_DENSITY_THRESHOLD` / `CHEESE_NOISE_TARGET` | `f32` | `0.390625` / `1.5625` / `-0.703125` | Exact dyadic rationals (`25/64`, `25/16`, `-45/64`) |

### Built-in `NoiseSettings` vertical dimensions & interpolation cell sizes (vanilla presets)

| Preset (`minecraft:worldgen/noise_settings`) | `min_y` | `height` | `sea_level` | `legacy_random_source` | `aquifers` | Terrain `interpolated` `(cell_xz, cell_y)` |
|---|---|---|---|---|---|---|
| `overworld` / `large_biomes` / `amplified` | `-64` | `384` | `63` | `false` | present | `(4, 8)` (`chunk_surface_level`: `(16, 1)`) |
| `nether` | `0` | `128` | `32` | `true` | absent | `(4, 8)` |
| `end` | `0` | `128` | `0` | `true` | absent | `(8, 4)` |
| `caves` | `-64` | `192` | `32` | `true` | absent | `(4, 8)` |
| `floating_islands` | `0` | `256` | `-64` | `true` | absent | `(8, 4)` |

### Context keys and special RNG identifiers
- `SamplerContext` keys: `minecraft:beardifier`, `minecraft:blender`, `minecraft:blender/alpha`, `minecraft:blender/offset`.
- Special RNG seed identifiers: `minecraft:terrain` (`BlendedNoise`), `minecraft:aquifer` (`Aquifer`), `minecraft:clay_bands` (`MaterialSystem`).
- Legacy Nether noise keys: `minecraft:nether/temperature` (`seed + 0`), `minecraft:nether/vegetation` (`seed + 1`).

---

## Oracle probe points

A black-box Java parity agent/probe for `golden-terrain` should dump outputs at five stages using these public classes and methods:

1. **Compiled density function point & volume parity**:
   - `net.minecraft.world.level.levelgen.RandomState.create(HolderGetter<NormalNoise>, long, NoiseGeneratorSettings)`
   - `RandomState.sampleBlockValueUncached(DensityFunction, int, int, int)` — dump raw `f32` bits (`Float.floatToRawIntBits`) for every one of the 8 `NoiseRouter` fields (`temperature`, `vegetation`, `continents`, `erosion`, `depth`, `ridges`, `chunkSurfaceLevel`, `finalDensity`) and each of the 6 `Aquifer.Config` functions at fixed `(x, y, z)` coordinates (including negative coordinates, `0`, multiples and non-multiples of 4/8/16, and large coordinates near `±2^24`).
   - `RandomState.getSampler(DensityFunction).sampleVolume(SamplerContext, DensityBuffer, DensityVolume)` — dump the full `DensityBuffer` (`float[]` raw bits) for a `16 × height × 16` chunk `DensityVolume` with `SamplerContext.builder().enableCaches().build()` to verify `SliceUniformAxes`, `CachingDensitySampler`, and `InterpolatedFunction.fillCell` (Z $\to$ X $\to$ incremental Y) against point queries.
2. **Individual density function micro-tests**:
   - `DensityFunctionCompiler.getSampler(DensityFunction)` across synthetic instances of all 44 types (`InterpolatedFunction`, `SplineFunction`, `PowFunction` with special vs. general exponents, `BinaryFunction` `div`-by-const and `mul`/`div` zero short-circuit, `RoundFunction` `ROUND` saturation, `GradientFunction` all 3 tiling modes with negative `coordinateRange`, `IntervalSelectFunction`, `RangeChoiceFunction`, `FindTopSurfaceFunction`, `EndIslandFunction` at negative coords, `DistanceToPointFunction` all 4 metrics, `ShiftNoiseFunction.ShiftB` volume vs. point, `BlendedNoise`).
3. **Beardifier & Blender isolated probes**:
   - `net.minecraft.world.level.levelgen.Beardifier` public constructor `Beardifier(List<Beardifier.Rigid>, List<JigsawJunction>, BoundingBox)` + `sampleValue` and `sampleVolume` across all 5 `TerrainAdjustment` modes and offsets `[-13..+13]`.
   - `net.minecraft.world.level.levelgen.blending.Blender.of(WorldGenRegion)`, `blendOffsetAndFactor(DensityVolume)`, and `blendDensity(int, int, int, float)` on synthetic `BlendingData` chunks (including negative Y).
4. **Column height & column block parity (pre-surface, no structures/blending)**:
   - `net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator.getBaseHeight(int, int, Heightmap.Types, LevelHeightAccessor, RandomState)` and `getBaseColumn(int, int, LevelHeightAccessor, RandomState)` across seeds and dimensions (Overworld, Nether, End, Amplified, Large Biomes).
5. **Full `doFill` / `TERRAIN` chunk block parity**:
   - Hook `NoiseBasedChunkGenerator.doFill(NoiseChunk, ChunkAccess)` (post-`doFill`, pre-`buildSurface`) and `NoiseBasedChunkGenerator.buildTerrain(...)` (post-`TERRAIN`) to dump every block-state global ID (`Block.getId(state)`), `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG` heightmap arrays (`Heightmap.getRawData()`), and post-processing fluid marks across a grid of chunks for multiple seeds and dimensions.

---

## Open questions

1. **[oracle] `Blender` hash-map iteration order (`Long2ObjectOpenHashMap`)**: When blending across multiple old chunks, `Blender.blendOffsetAndFactor` and `Blender.blendDensity` accumulate `f64` weighted sums in `Long2ObjectOpenHashMap` bucket order. Probe: construct a `Blender` with $\ge 3$ surrounding `BlendingData` chunks at varying distances and observe whether any quart `(blockX, blockZ)` or `(blockX, blockY, blockZ)` differs in the `f32` output between sorted-chunk iteration and fastutil's `Long2ObjectOpenHashMap` iteration order (inputs: 4 old chunks around `(0, 0)` with non-trivial heights/densities $\to$ observe raw `f32` bits of `blendOffsetAndFactor` and `blendDensity`).
2. **[oracle] JVM `Math.log` / `Math.pow` / `Math.sin` / `Math.cos` intrinsic parity**: `UnaryFunction(LOG)`, `PowFunction`, `Beardifier` (`BEARD_KERNEL` via `Math.pow(Math.E, ...)`), and `NoiseSpawnFinder` (`Math.sin`/`Math.cos`) call `java.lang.Math` rather than `StrictMath`. Probe: dump all 13,824 `f32` bits of `Beardifier.BEARD_KERNEL` and sample `PowFunction` / `LogSampler` at 1,000 non-dyadic `f32` inputs on each CI host architecture $\to$ observe whether HotSpot intrinsics match Rust `f64::powf`/`ln`/`sin`/`cos` (or fdlibm) bit-for-bit.
3. **[oracle] `NoiseSpawnFinder` tie-breaking and `Math.sin`/`Math.cos` coordinates**: `NoiseSpawnFinder.radialSearch` computes `(int)(Math.sin(angle) * radius)` with `f32` `angle` widened to `f64` and `angle += radiusIncrement / radius` in `f32`, comparing `angle > Math.PI * 2` in `f64`. Probe: run `NoiseBasedChunkGenerator.getOrigin(RandomState)` across 20 Overworld seeds $\to$ observe the resulting `ChunkPos` and confirm exact match against `NoiseSpawnFinder`.
4. **[oracle] `CubicSpline` unsorted JSON points**: `CubicSpline.Multipoint.codec` does not reject unsorted `locations` on JSON decode, whereas `Builder` does. Probe: decode a hand-crafted `minecraft:spline` JSON with non-ascending `locations` via `DensityFunction.CODEC` $\to$ observe whether decoding succeeds and what `binarySearch` returns (vanilla data packs are always sorted, so this only matters for malformed third-party data packs).
5. **(implementer) `BlockState` JSON schema in `NoiseGeneratorSettings`**: §5.3 specifies `default_block` and `default_fluid` as `BlockState` without listing the accepted JSON forms. In 26.3 vanilla `worldgen/noise_settings/*.json`, these fields use bare block identifier strings (e.g. `"minecraft:stone"`, `"minecraft:water"`). Implemented `BlockStateSpecCodec` to accept either a bare `Identifier` string (default block state with empty properties) or an object `{"Name": <Identifier>, "Properties"?: {<String>: <String>}}`. Please confirm whether `BlockState.CODEC` in 26.3 accepts any other form or validates property names/values at decode time.
   **Answered (reader, 2026-11-25):** see `registry/identifiers-registries-tags.md` §3.3. The accepted forms are a
   bare block id (default state) or `{"id": …, "properties": {…}}` (lower-case keys); `{"Name", "Properties"}` is the
   pre-5006 form and must be **rejected** by this codec.

---

## Parity status
`worldgen-router`: **30,063/30,063** values bit-identical to vanilla 26.3. All 8 noise-router fields at 134 points
(edge cases: cell and chunk boundaries, negatives, ±2²⁴, the world border; plus random points in three distance
bands) and the 6 aquifer-config functions where present, for all 7 built-in noise settings × 3 seeds; and whole
final-density (16×H×16) and climate (stride 4) chunk volumes through the caching path for 2 seeds × 3 chunks each.
`worldgen-columns`: **2,562/2,562** identical: the 4 base heights and every block-state id of the base column (§6–7 with
the aquifer of `aquifers-and-ore-veins.md`) at 61 columns for all 7 noise settings × 3 seeds. §8 (beardifier) is verified
with real structures by `golden-terrain-structures` (**101,830/101,830**, 12 seeds); §9 (old-chunk blending) is still
an interface only. Full chunks are verified by `golden-terrain`.
