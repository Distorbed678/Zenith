# Worldgen: biomes and climate (26.3)

- Verified by suites: `golden-terrain`
- Sources: data-pack JSON schemas for `minecraft:worldgen/biome_source` and `minecraft:worldgen/multi_noise_biome_source_parameter_list`, the game's `reports/biome_parameters/` data-generator report format, Guava's public `Hashing.sha256().hashLong(long).asLong()` little-endian specification, IEEE-754 binary32/binary64 arithmetic, and local reference reading (2026-11-25) for 26.3 exact quantization, R-tree construction/tie-breaking, preset build rules, chunk biome sampling order, and Voronoi biome zoom. Everything below is written in our own words.
- Related specs:
  - [`docs/specs/math/noise.md`](../math/noise.md) (noise stacks, `f32` volume vs point sampling associativity, End island simplex noise, legacy Nether biome noises)
  - [`docs/specs/math/random.md`](../math/random.md) (legacy LCG and xoroshiro128++ RNGs)
  - [`docs/specs/math/java-math-helpers.md`](../math/java-math-helpers.md) (`Mth` helpers, `LinearCongruentialGenerator`, `QuartPos`, `SectionPos`, `ChunkPos`)
  - [`docs/specs/codec/datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md) (strict JSON, codec coercion, `either` / `mapEither`, type dispatch)
  - [`docs/specs/registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) (registries, holders, holder sets, world-layer dynamic registries)
  - [`docs/specs/storage/region-and-level.md`](../storage/region-and-level.md) (Anvil chunk layout and section persistence)
  - [`docs/specs/worldgen/density-functions.md`](./density-functions.md) (density functions, `NoiseRouter` fields, `DensityVolume` / `DensityBuffer` sampling, `SamplerContext`)

---

## Summary

Biomes in 26.3 are assigned at **quart resolution** ($4 \times 4 \times 4$ blocks per cell) during the `BIOMES` chunk generation step and stored in each chunk section's $4 \times 4 \times 4$ paletted biome container. When gameplay, surface rules, carvers, or features query the biome at a **block** coordinate, a seeded 3D Voronoi zoom (`BiomeManager`) maps the block coordinate to one of the 8 surrounding quart cells using an obfuscated copy of the world seed.

Biome assignment has six distinct subsystems:
1. **Climate parameter space**: a 7-dimensional quantized integer space (6 router channels + offset) with closed intervals and squared $L_2$ distance.
2. **Multi-noise biome source (`minecraft:multi_noise`)**: selects the closest biome in 7D climate space via an in-memory 19-way R-tree with a per-thread warm-start cache (`lastResult`) whose strict-inequality updates govern tie-breaking.
3. **Other biome sources**: `minecraft:fixed`, `minecraft:checkerboard`, and `minecraft:the_end` (the End's 64-chunk central island test and erosion-channel threshold rule).
4. **Chunk biome sampling and spawn finding**: how `BIOMES` samples a chunk's quart grid (volume sampling for multi-noise, exact section/X/Y/Z fill order, blending/retrogen wrappers, cross-chunk fallback) and how `NoiseSpawnFinder` locates the world origin chunk from `spawn_target` density functions.
5. **Voronoi biome zoom (`BiomeManager`)**: SHA-256 little-endian seed obfuscation and exact 8-corner quadratic-LCG jittered distance lookup.
6. **Vanilla multi-noise presets (`minecraft:overworld` and `minecraft:nether`)**: the jar ships only preset-name stubs, not the parameter tables; Zenith regenerates the exact ordered entry lists from the procedural build rules in Section 6 (or ingests the user's own data-generator reports at runtime without committing game data).

---

## Inputs and state

### Numeric and conversion conventions
- Floating-point arithmetic is IEEE-754 binary32 (`f32`) and binary64 (`f64`), round-to-nearest-even, **without** fused multiply-add. Chains associate left-to-right unless parenthesized.
- Integer arithmetic is Java two's-complement wrapping (`i32` signed 32-bit, `i64` signed 64-bit). Arithmetic right shift is $\gg$.
- Saturating float-to-integer truncation toward zero is written $\text{trunc}_{i32}(v)$ and $\text{trunc}_{i64}(v)$ ($\text{NaN} \mapsto 0$, out-of-range saturates to $T_{\min}$ / $T_{\max}$).
- Integer division written $/_{\text{trunc}}$ truncates toward zero (e.g., $(-1) /_{\text{trunc}} 2 = 0$, not $-1$).
- `square_i64(v)` is $v \times_{64} v$ (wrapping 64-bit multiply).
- `floorMod(a, m)` is Euclidean modulo toward $-\infty$ (for $m > 0$, result is in $[0, m - 1]$).

### Registries and world state
- **Built-in biome-source codec registry**: `minecraft:worldgen/biome_source` containing `minecraft:fixed`, `minecraft:multi_noise`, `minecraft:checkerboard`, and `minecraft:the_end`.
- **Dynamic world-layer registries** (loaded from data packs per `docs/specs/registry/identifiers-registries-tags.md` §2.4):
  - `minecraft:worldgen/biome` — biome definitions (referenced by holders and holder sets).
  - `minecraft:worldgen/multi_noise_biome_source_parameter_list` — named parameter-list presets.
- **Per-dimension runtime inputs**:
  - `worldSeed: i64` and its obfuscated `biomeZoomSeed: i64`.
  - `BiomeSource` (from the dimension's `ChunkGenerator`).
  - `NoiseRouter` (from `NoiseGeneratorSettings`, see `docs/specs/worldgen/density-functions.md`) bound into a `Climate.Sampler` with six `DensitySampler.Bound` channels:
    - `temperature` $\leftarrow$ router `temperature`
    - `humidity` $\leftarrow$ router `vegetation`
    - `continentalness` $\leftarrow$ router `continents`
    - `erosion` $\leftarrow$ router `erosion`
    - `depth` $\leftarrow$ router `depth`
    - `weirdness` $\leftarrow$ router `ridges`
  - Chunk vertical bounds: `minY`, `height` (multiples of 16), `minSectionY = minY >> 4`, `maxSectionY = ((minY + height - 1) >> 4)`.

---

## Behavior (ordered rules)

### 1. Climate parameter space, quantization, and distance metric

#### Parameter coordinates and quantization
All climate matching operates in a **7-dimensional signed 64-bit integer space** with fixed dimension indices:

| Index | Name | Router field | JSON field | Target value |
|---|---|---|---|---|
| `0` | Temperature | `temperature` | `temperature` | quantized router sample |
| `1` | Humidity | `vegetation` | `humidity` | quantized router sample |
| `2` | Continentalness | `continents` | `continentalness` | quantized router sample |
| `3` | Erosion | `erosion` | `erosion` | quantized router sample |
| `4` | Depth | `depth` | `depth` | quantized router sample |
| `5` | Weirdness | `ridges` | `weirdness` | quantized router sample |
| `6` | Offset | *(none)* | `offset` | always `0` |

1. **Quantization (`f32 -> i64`)**:
   $$\text{quantize}(x) = \text{trunc}_{i64}(x \cdot_{f32} 10{,}000.0_f)$$
   - The multiplication by $10{,}000.0_f$ is performed in **`f32`** before saturating truncation toward zero to `i64`. Do **not** promote to `f64` and do **not** round to nearest or floor.
2. **Unquantization (`i64 -> f32`)**:
   $$\text{unquantize}(q) = \text{f32}(q) /_{f32} 10{,}000.0_f$$
3. **Target point construction**: given six `f32` router values $(t, h, c, e, d, w)$, construct `TargetPoint` by applying $\text{quantize}$ to each value independently. As a 7-element `i64` vector for R-tree search, the 7th slot (`index 6`) is always `0`.

#### Parameter intervals (`Parameter`)
A `Parameter` is a closed quantized interval $[\text{min}, \text{max}]$ of `i64` with $\text{min} \le \text{max}$:
- `point(v: f32)`: $[\text{quantize}(v),\; \text{quantize}(v)]$.
- `span(lo: f32, hi: f32)`: fails if $\text{lo} > \text{hi}$ (in `f32`), else $[\text{quantize}(\text{lo}),\; \text{quantize}(\text{hi})]$.
- `span(a: Parameter, b: Parameter)`: fails if $a.\text{min} > b.\text{max}$, else $[a.\text{min},\; b.\text{max}]$.
- Union `span(a: Parameter, b: Option<Parameter>)`: if $b$ is absent return $a$, else $[\min(a.\text{min}, b.\text{min}),\; \max(a.\text{max}, b.\text{max})]$.

**1D distance from an interval to a target coordinate $t \in \text{i64}$**:
1. Compute $\text{above} = t -_{64} \text{max}$ and $\text{below} = \text{min} -_{64} t$.
2. If $\text{above} > 0$, return $\text{above}$; otherwise return $\max(\text{below},\; 0)$.
(Returns `0` when $\text{min} \le t \le \text{max}$, and the positive gap otherwise.)

#### Parameter point (`ParameterPoint`) and distance metric
A `ParameterPoint` holds six `Parameter` intervals `(temperature, humidity, continentalness, erosion, depth, weirdness)` plus a scalar `offset: i64` (quantized from `f32`). Its 7-element bounding box appends `[offset, offset]` at index `6`.

**7D squared distance (fitness)** between any 7-element bounding box $B[0..6]$ (leaf or internal R-tree node) and target vector $T[0..6]$ (with $T[6] = 0$):
$$D(B, T) = \sum_{i=0}^{6} \text{square}_{i64}\!\big(B[i].\text{distance}(T[i])\big)$$
evaluated left-to-right for $i = 0, 1, 2, 3, 4, 5, 6$ in wrapping 64-bit integer arithmetic. For a leaf (`ParameterPoint`), index `6` has interval $[\text{offset}, \text{offset}]$ and $T[6] = 0$, contributing $\text{square}_{i64}(\text{offset})$ (matching `ParameterPoint::fitness`).

#### JSON schemas for `Parameter` and `ParameterPoint`
- **`Parameter` codec** (`ExtraCodecs.intervalCodec` over `f32` in $[-2.0_f, 2.0_f]$ inclusive):
  Tries forms in order (per `docs/specs/codec/datapacks-and-codecs.md` §7.4):
  1. A single JSON number $v \in [-2.0_f, 2.0_f]$ $\mapsto$ $[\text{quantize}(v), \text{quantize}(v)]$.
  2. A JSON array of **exact length 2** `[min, max]`, each in $[-2.0_f, 2.0_f]$.
  3. A JSON object `{"min": <f32>, "max": <f32>}`, each in $[-2.0_f, 2.0_f]$.
  - Rejects any interval where Java `Float.compare(min, max) > 0` (note: `-0.0f` compares less than `+0.0f`, so `[+0.0, -0.0]` is rejected).
  - On encode: if `min == max`, emits a single float `unquantize(min)`; otherwise emits the 2-element array `[unquantize(min), unquantize(max)]`.
- **`ParameterPoint` codec**: JSON object with seven required fields:
  - `temperature`, `humidity`, `continentalness`, `erosion`, `depth`, `weirdness`: each a `Parameter` (range $[-2.0_f, 2.0_f]$).
  - `offset`: `f32` in $[0.0_f, 1.0_f]$ inclusive, quantized via $\text{quantize}$.

---

### 2. Multi-noise biome source (`minecraft:multi_noise`)

#### Data-pack schema
Type-dispatched in `minecraft:worldgen/biome_source` with `"type": "minecraft:multi_noise"`. The remaining fields are decoded via **`mapEither(DIRECT, PRESET)`** (inline `"biomes"` is tried **first**; if valid, `"preset"` is ignored):
1. **Inline form**:
   - `"biomes"`: **non-empty** JSON array of objects, each with:
     - `"parameters"`: `ParameterPoint` (required)
     - `"biome"`: `Holder<Biome>` (required; reference ID string or inline biome definition per `docs/specs/registry/identifiers-registries-tags.md` §2.6).
2. **Preset form**:
   - `"preset"`: `Holder<MultiNoiseBiomeSourceParameterList>` (required; either a registry ID string in `minecraft:worldgen/multi_noise_biome_source_parameter_list`, such as `"minecraft:overworld"` or `"minecraft:nether"`, or an inline object `{"preset": "<builtin_preset_id>"}`).
- **Parameter-list registry (`minecraft:worldgen/multi_noise_biome_source_parameter_list`)**:
  - World-layer dynamic registry loaded from `data/<ns>/worldgen/multi_noise_biome_source_parameter_list/*.json`.
  - Schema: `{"preset": "<id>"}` where `<id>` must be one of the two built-in preset identifiers: `minecraft:overworld` or `minecraft:nether`. Any other identifier is a decode error.
  - At load time, the preset invokes its code-defined builder (Section 6) and resolves each biome key via `getOrThrow` against `minecraft:worldgen/biome` (a missing biome fails loading).

#### Search structure: 19-way 7D R-tree construction
Every `ParameterList<T>` compiles its ordered list of $N \ge 1$ `(ParameterPoint, T)` entries into an immutable 7D R-tree with branching factor $K = 19$ (`CHILDREN_PER_NODE = 19` in 26.3):

1. **Validation and leaf creation**:
   - If $N = 0$, fail with an error.
   - Convert each entry $i \in [0, N)$ into a `Leaf` holding its 7-element `Parameter[]` array `[T, H, C, E, D, W, [offset, offset]]` and its value $T$, preserving input list order.
2. **Recursive `build(nodes)`**:
   - If `nodes.len == 0`: fail.
   - If `nodes.len == 1`: return `nodes[0]` directly (do **not** wrap in a `SubTree`).
   - **Leaf-level / small-group base case (`nodes.len <= 19`)**:
     - Perform a **stable sort** (TimSort semantics: equal keys preserve existing relative order) ascending by:
       $$M(\text{node}) = \sum_{d=0}^{6} \left| (\text{node}.\text{space}[d].\text{min} +_{64} \text{node}.\text{space}[d].\text{max}) /_{\text{trunc}} 2 \right|$$
     - Wrap the sorted list in a `SubTree` whose bounding box on each axis $d \in [0, 6]$ is the union span $[\min_j \text{min}_{j,d},\; \max_j \text{max}_{j,d}]$ over its children. Return it.
   - **Split case (`nodes.len > 19`)**:
     - Initialize `bestCost = i64::MAX`, `bestDim = -1`, `bestBuckets = none`.
     - For each axis $d = 0, 1, 2, 3, 4, 5, 6$ in ascending order:
       1. **In-place stable sort** `nodes` using the cyclic 7-axis **signed midpoint** comparator starting at $d$:
          - Compare axes in order $k = d,\; (d+1) \bmod 7,\; \dots,\; (d+6) \bmod 7$.
          - Key for node on axis $k$: $c_k = (\text{min}_k +_{64} \text{max}_k) /_{\text{trunc}} 2$ (signed `i64`).
       2. **Bucketize** the sorted `nodes` with $K = 19$:
          - Compute target bucket size in `f64` then truncate to `i32`:
            $$B = \text{trunc}_{i32}\!\left(\text{pow}_{f64}\!\left(19.0,\; \left\lfloor \frac{\ln(\text{f64}(\text{nodes.len}) - 0.01)}{\ln(19.0)} \right\rfloor\right)\right)$$
          - Partition `nodes` sequentially into contiguous groups of size $B$, placing any final remainder ($1 \dots B - 1$) into a final bucket. Wrap each group into a temporary `SubTree` (copying its children array immediately so subsequent iterations' in-place sorts do not mutate it) and compute each bucket's 7D bounding box.
       3. **Evaluate split cost**:
          $$\text{totalCost} = \sum_{b \in \text{buckets}} \sum_{a=0}^{6} \left| b.\text{space}[a].\text{max} -_{64} b.\text{space}[a].\text{min} \right|$$
          (sum of bucket L1 semi-perimeters in wrapping 64-bit signed arithmetic).
       4. If `bestCost > totalCost` (**strict `>`**): set `bestCost = totalCost`, `bestDim = d`, `bestBuckets = buckets`. (Ties keep the **smaller** axis index $d$.)
     - **Stable sort** `bestBuckets` using the cyclic 7-axis **absolute midpoint** comparator starting at `bestDim`:
       - Compare axes in order $k = \text{bestDim},\; (\text{bestDim}+1) \bmod 7,\; \dots,\; (\text{bestDim}+6) \bmod 7$.
       - Key for bucket on axis $k$: $|(\text{min}_k +_{64} \text{max}_k) /_{\text{trunc}} 2|$.
     - Recursively invoke `build` on each bucket's children list (in the exact order captured when `bestBuckets` was created at iteration `bestDim`, which determines tie-breaking for any leaf-level `totalMagnitude` stable sort) and return a `SubTree` over the resulting child nodes in `bestBuckets` order.

#### Exact R-tree lookup and tie-breaking
Each `RTree` maintains a **thread-local warm-start cache** `lastResult: Option<Leaf>` initialized to `None` on each thread.

Given `target: TargetPoint`:
1. Form `targetArray = [T, H, C, E, D, W, 0]`.
2. Call `leaf = root.search(targetArray, lastResult)`.
3. Store `lastResult = Some(leaf)` on the current thread.
4. Return `leaf.value`.

**Node search rules**:
- **`Leaf::search(target, candidate)`**: returns `self` unconditionally (ignores `candidate`).
- **`SubTree::search(target, candidate)`**:
  1. Set `minDist = i64::MAX` if `candidate` is `None`, else `D(candidate.space, target)`.
  2. Set `closest = candidate`.
  3. For each `child` in `this.children` in index order `0 .. children.len - 1`:
     1. Compute `childDist = D(child.space, target)`.
     2. If `minDist > childDist` (**strict inequality**):
        - Recurse: `leaf = child.search(target, closest)` (passing the current best `closest` down!).
        - Compute `leafDist`: if `child` is a `Leaf` (so `leaf` is `child`), reuse `childDist`; otherwise compute `D(leaf.space, target)`.
        - If `minDist > leafDist` (**strict inequality**): set `minDist = leafDist`, `closest = Some(leaf)`.
  4. Return `closest`.

**Tie-breaking guarantees (critical for bit-exactness)**:
- Both pruning (`minDist > childDist`) and replacement (`minDist > leafDist`) are **strict**.
- Consequently, if two or more leaves achieve the exact same minimum `i64` squared distance:
  1. If `lastResult` is one of the tied optimal leaves, **`lastResult` wins**, regardless of where it sits in the tree.
  2. Otherwise, the **first** optimal leaf visited in depth-first traversal of the exact R-tree built above wins.
- A brute-force scan in list order does **not** guarantee identical tie-breaking to the R-tree; Zenith must match the R-tree's structure and warm-start search semantics. **[oracle]** (See Open questions #1 regarding cross-chunk persistence of `lastResult`.)

#### Point resolver vs chunk volume resolver
- **Single-point resolver (`createResolver(sampler)`)**: used for uncached lookups, carver biomes, structure biome checks, `/locate biome`, and debug info. For `(quartX, quartY, quartZ)`, computes `blockX = quartX << 2`, `blockY = quartY << 2`, `blockZ = quartZ << 2`, samples the 6 router functions via `sampleValue(blockX, blockY, blockZ)` in order `(temperature, humidity, continentalness, erosion, depth, weirdness)`, quantizes into `TargetPoint`, and searches the R-tree.
- **Chunk volume resolver (`createResolverForChunk`)**: used during the `BIOMES` chunk step (Section 4). Given `(minQuartX, minQuartY, minQuartZ, quartSizeX, quartSizeY, quartSizeZ)`:
  1. Construct a `DensityVolume` with dimensions `(quartSizeX, quartSizeY, quartSizeZ)`, origin `(minQuartX << 2, minQuartY << 2, minQuartZ << 2)`, and strides `(4, 4, 4)`.
  2. Allocate 6 unpooled `DensityBuffer`s of length `quartSizeX * quartSizeY * quartSizeZ`.
  3. Fill them via **`sampleVolume`** in exact order: `temperature`, `humidity` (`vegetation`), `continentalness` (`continents`), `erosion`, `depth`, `weirdness` (`ridges`). Buffer indexing is Z-major, X-middle, Y-contiguous: `index(ix, iy, iz) = iy + (ix + iz * quartSizeX) * quartSizeY`.
  4. Return a resolver that, for `(quartX, quartY, quartZ)`, reads all 6 floats at `index(quartX - minQuartX, quartY - minQuartY, quartZ - minQuartZ)`, quantizes them via `Climate::target`, and searches the R-tree.
  - **Precision note**: `sampleVolume` can differ by up to 1 ULP from `sampleValue` for non-dyadic frequency × scale products (see `docs/specs/math/noise.md` §5). Chunk generation uses `sampleVolume`, whereas uncached queries use `sampleValue`. **[oracle]** (See Open questions #2.)

---

### 3. All other biome source types in 26.3

Every biome source codec is registered in `minecraft:worldgen/biome_source` (4 types total in 26.3). The three non-multi-noise types do not override `createResolverForChunk` and therefore use their point `createResolver` during chunk generation as well.

#### 3.1 `minecraft:fixed` (`FixedBiomeSource`)
- **Schema**: `{"type": "minecraft:fixed", "biome": <Holder<Biome>>}`.
- **Possible biomes**: `{biome}`.
- **Sampling rule**: returns `biome` for every `(quartX, quartY, quartZ)` without evaluating the climate sampler.
- Used directly by `single_biome_surface` (`minecraft:plains`), `FlatLevelSource` (configured flat-world biome), and `DebugLevelSource` (`minecraft:plains`).

#### 3.2 `minecraft:checkerboard` (`CheckerboardColumnBiomeSource`)
- **Schema**:
  - `"biomes"`: `HolderSet<Biome>` (required; tag `"#ns:id"`, single biome, or list of biomes).
  - `"scale"`: `i32` in `[0, 62]` inclusive (optional, default `2`).
- **Derived shift**: `bitShift = scale + 2`. (With default `scale = 2`, `bitShift = 4`, giving 16-quart = 64-block = 4-chunk tiles; with `scale = 0`, `bitShift = 2`, giving 1-chunk tiles.)
- **Sampling rule**: ignores `quartY`. Let $n = \text{biomes.size()}$. Return:
  $$\text{biomes}\!\Big[\,\text{floorMod}\big((\text{quartX} \gg_{32} \text{bitShift}) +_{32} (\text{quartZ} \gg_{32} \text{bitShift}),\; n\big)\,\Big]$$
  - **Java 32-bit shift quirk**: because `quartX` and `quartZ` are `i32`, Java masks the shift distance with `bitShift & 31`. For `scale` in `30..62` (`bitShift` in `32..64`), the effective shift wraps modulo 32 (e.g., `scale = 30` shifts by `0`, `scale = 31` shifts by `1`). The sum uses wrapping 32-bit addition before `Math.floorMod`. (An empty `biomes` set causes division by zero at query time.)

#### 3.3 `minecraft:the_end` (`TheEndBiomeSource`)
- **Schema**: `{"type": "minecraft:the_end"}` (no JSON fields). Automatically retrieves 5 fixed entries from `minecraft:worldgen/biome` via registry lookup:
  - `minecraft:the_end`
  - `minecraft:end_highlands`
  - `minecraft:end_midlands`
  - `minecraft:small_end_islands`
  - `minecraft:end_barrens`
- **Sampling rule** for `(quartX, quartY, quartZ)`:
  1. Convert to block and chunk (section) coordinates:
     - `blockX = quartX << 2`, `blockY = quartY << 2`, `blockZ = quartZ << 2`
     - `chunkX = blockX >> 4` (equivalent to `quartX >> 2`), `chunkZ = blockZ >> 4` (equivalent to `quartZ >> 2`)
  2. **Central island check** (64-bit signed arithmetic):
     - Compute $d^2 = \text{i64}(\text{chunkX}) \times \text{i64}(\text{chunkX}) + \text{i64}(\text{chunkZ}) \times \text{i64}(\text{chunkZ})$.
     - If $d^2 \le 4096$ (i.e., Euclidean chunk distance $\le 64$, or $1{,}024$ blocks), return **`minecraft:the_end`** without sampling noise.
  3. **Outer islands sampling**:
     - Compute chunk-center sample coordinates (32-bit wrapping):
       $$\text{sampleX} = (\text{chunkX} \cdot 2 + 1) \cdot 8, \qquad \text{sampleZ} = (\text{chunkZ} \cdot 2 + 1) \cdot 8$$
       (Every quart inside chunk `(chunkX, chunkZ)` samples the exact same `(sampleX, sampleZ) = (chunkX * 16 + 8, chunkZ * 16 + 8)`, so outer End biomes are uniform across each 16×16 chunk column when the erosion density function is Y-independent.)
     - Evaluate the router's **`erosion`** channel at `(sampleX, blockY, sampleZ)` and widen `f32 -> f64`:
       $$h = \text{f64}\big(\text{sampler.erosion().sampleValue}(\text{sampleX}, \text{blockY}, \text{sampleZ})\big)$$
       (In vanilla `minecraft:end` noise settings, the router's `erosion` field is `minecraft:end_islands`, built from the End island simplex noise seeded with legacy LCG `worldSeed` + `consumeCount(17292)` and $S = 0.0$ per `docs/specs/math/noise.md`; see `docs/specs/worldgen/density-functions.md` for `minecraft:end_islands`.)
  4. **Threshold classification** (exact ordered comparisons; all three constants are exact in binary32 and binary64):
     - If $h > 0.25$: return **`minecraft:end_highlands`**.
     - Else if $h \ge -0.0625$: return **`minecraft:end_midlands`**.
     - Else if $h < -0.21875$: return **`minecraft:small_end_islands`**.
     - Else ($-0.21875 \le h < -0.0625$, or $h$ is `NaN`): return **`minecraft:end_barrens`**.

---

### 4. Where biome sampling happens: quart grid, `BIOMES` step, and spawn finding

#### Quart coordinates and chunk pyramid placement
- Quart conversions (`QuartPos`, see `docs/specs/math/java-math-helpers.md` §8):
  - `fromBlock(b) = b >> 2`, `toBlock(q) = q << 2`, `quartLocal(b) = b & 3`
  - `fromSection(s) = s << 2`, `toSection(q) = q >> 2`
- In 26.3's `ChunkPyramid.GENERATION_PYRAMID`, chunk steps execute in order:
  `EMPTY` $\to$ `STRUCTURE_STARTS` $\to$ `STRUCTURE_REFERENCES` $\to$ **`BIOMES`** $\to$ `TERRAIN` (fills noise, builds surface, runs carvers) $\to$ `FEATURES` $\to$ `INITIALIZE_LIGHT` $\to$ `LIGHT` $\to$ `SPAWN` $\to$ `FULL`.
  - `BIOMES` depends on `STRUCTURE_STARTS` in radius 8.
  - `TERRAIN` and `SPAWN` each require `BIOMES` in radius **1** ($3 \times 3$ chunks), guaranteeing that `BiomeManager::getBiome` (which reads up to $\pm 1$ quart across chunk edges) hits stored chunk biomes during surface building.

#### Exact execution of the `BIOMES` step
For a chunk at `(chunkX, chunkZ)` with height bounds `(minY, height)`:
1. Acquire a pooled `DensityBufferPool` from `RandomState` and create a `Climate.Sampler` using `SamplerContext` with caches enabled and the buffer pool attached.
2. Compute chunk quart origin and extent:
   - `minQuartX = (chunkX << 4) >> 2 = chunkX << 2`, `quartSizeX = 4`
   - `minQuartY = minY >> 2`, `quartSizeY = height >> 2`
   - `minQuartZ = (chunkZ << 4) >> 2 = chunkZ << 2`, `quartSizeZ = 4`
3. Obtain `resolver = biomeSource.createResolverForChunk(climateSampler, minQuartX, minQuartY, minQuartZ, 4, height >> 2, 4)`.
4. Decorate `resolver` via `ChunkGenerator::decorateBiomeResolver`:
   - `NoiseBasedChunkGenerator`: wraps first with `Blender::getBiomeResolver` (for chunks adjacent to pre-26.3 blended chunks, selects nearest old-chunk biome within blending radius in XZ; no-op for fresh worlds), then with `BelowZeroRetrogen::getBiomeResolver` (for upgrading pre-1.18 chunks, keeps only retained cave biomes and copies `quartY = 0` otherwise; no-op when not upgrading).
   - `FlatLevelSource` / `DebugLevelSource`: no decoration.
5. Fill sections in **ascending `sectionY` order** from `minSectionY` to `maxSectionY` inclusive (for vanilla Overworld: sections `-4..=19`, `quartY` in `-16..=79`; Nether & End: sections `0..=15`, `quartY` in `0..=63`):
   - Let `quartMinY = sectionY << 2`.
   - Recreate the section's biome `PalettedContainer` (initialized to its previous palette entry 0).
   - Populate all $4 \times 4 \times 4 = 64$ entries in exact **X outer, Y middle, Z inner** loop order:
     - For `x` in `0..4`:
       - For `y` in `0..4`:
         - For `z` in `0..4`:
           - Query `biome = resolver.getNoiseBiome(minQuartX + x, quartMinY + y, minQuartZ + z)`.
           - Write `biome` into the new container at local quart `(x, y, z)`.
   - Replace the section's biome container with the newly filled container.
6. Release the `DensityBufferPool`.

#### Section storage layout and stored-biome reads
- Each section's biome `PalettedContainer` uses `Strategy::createForBiomes` (`bitsPerAxis = 2`, 64 entries):
  - Flat index for local quart `(x, y, z)` in `0..4`:
    $$\text{index}(x, y, z) = ((y \ll 2) \mid z) \ll 2 \mid x = 16 y + 4 z + x$$
    (Y-major, Z-middle, X-contiguous — identical axis order to block states.)
  - In-memory palette tiers: `0` bits (single-value), `1..3` bits (linear palette), $\ge 4$ bits (global palette with `ceillog2(biomeRegistry.size())` bits). On disk (Anvil NBT `biomes`), repacks to `ceillog2(distinctBiomes)` bits (`0` omits `"data"`).
- **Reading from a chunk (`ChunkAccess::getNoiseBiome(quartX, quartY, quartZ)`)**:
  - Clamps `quartY` to `[minY >> 2,\; (minY >> 2) + (height >> 2) - 1]`.
  - Computes `sectionIndex = ((clampedQuartY << 2) >> 4) - minSectionY`.
  - Returns `sections[sectionIndex].biomes.get(quartX & 3, clampedQuartY & 3, quartZ & 3)`.
- **Reading from a `LevelReader` / `WorldGenRegion`**:
  - Looks up chunk `(quartX >> 2, quartZ >> 2)` at status `ChunkStatus.BIOMES` (without generating).
  - If present, delegates to `chunk.getNoiseBiome(quartX, quartY, quartZ)`.
  - Otherwise falls back to `getUncachedNoiseBiome`, which evaluates `biomeSource.createUncachedResolver(randomState)` (point evaluation, no caches).
- **Carver biome queries (`NoiseBasedChunkGenerator::generateCarvers`)**:
  - During `TERRAIN`, carver selection and carving-mask top-material placement do **not** read stored chunk biomes; they create a fresh uncached resolver (`biomeSource.createUncachedResolver(randomState)`), sample `(sourceChunkX << 2, 0, sourceChunkZ << 2)` for each chunk's carver list, and wrap `biomeManager.withDifferentSource(uncachedResolver)` for block-position biome lookups during `applyCarvingMask`.

#### Spawn finding (`NoiseSpawnFinder` and `SpawnTargetPoint`)
In 26.3, world-origin chunk selection (`ChunkGenerator::getOrigin`) does **not** use `Climate.Sampler` or `BiomeSource`; instead `NoiseBasedChunkGenerator::getOrigin` uses `NoiseGeneratorSettings.spawn_target`:
- **Schema**: `"spawn_target"` is a list of `SpawnTargetPoint` objects; each `SpawnTargetPoint` is a JSON map `{ "<density_function_id>": <Climate.Parameter>, ... }`.
- If `spawn_target` is empty (Nether, End, Caves, Floating Islands, Flat, Debug), `getOrigin` returns chunk `(0, 0)`.
- **Vanilla Overworld `spawn_target`**: 2 target points built by `OverworldBiomeBuilder::spawnTarget` using the dimension's Overworld function set (`temperature`, `vegetation`, `continents`, `erosion` — normal, large-biomes, or amplified variants) plus `minecraft:overworld/ridges`:
  - Point 1: `temperature`, `vegetation`, `erosion` in `[-1.0, 1.0]`, `continents` in `[-0.11, 1.0]`, `ridges` in `[-1.0, -0.16]`.
  - Point 2: identical except `ridges` in `[0.16, 1.0]`.
- **Search algorithm (`NoiseSpawnFinder`)** using a caching `DensitySamplerSet`:
  1. Define `eval(blockX, blockZ)`:
     - Snap to quart-aligned block coordinates: `qx = (blockX >> 2) << 2`, `qz = (blockZ >> 2) << 2`.
     - For each `SpawnTargetPoint`, sum `square_i64(param.distance(quantize(sampleValue(fn, qx, 0, qz))))` over its map entries. Take `minFitness = min` across target points (initialized to `i64::MAX`).
     - Compute `totalFitness = minFitness * 4_194_304 + square_i64(i64(blockX)) + square_i64(i64(blockZ))` (where $2048^2 = 4{,}194{,}304$; wrapping 64-bit).
     - Return `(BlockPos(blockX, 0, blockZ), totalFitness)`.
  2. Initialize `best = eval(0, 0)`.
  3. Run `radialSearch(maxRadius = 2048.0f, step = 512.0f)`, then `radialSearch(maxRadius = 512.0f, step = 32.0f)`:
     - Fix `origin = best.pos`, `angle = 0.0f` (`f32`), `radius = step` (`f32`).
     - While `radius <= maxRadius`:
       - `x = origin.x + trunc_i32(Math.sin(f64(angle)) * f64(radius))`
       - `z = origin.z + trunc_i32(Math.cos(f64(angle)) * f64(radius))`
       - `cand = eval(x, z)`; if `cand.fitness < best.fitness` (strict `<`), set `best = cand`.
       - `angle = angle + (step / radius)` (in `f32`).
       - If `f64(angle) > 2.0 * Math.PI`: set `angle = 0.0f`, `radius = radius + step` (`f32`).
  4. Return `ChunkPos.containing(best.pos)`.
- **Relevance to chunk output**: `NoiseSpawnFinder` does **not** alter terrain blocks or stored biomes of any individual chunk `(cx, cz)`, so it does not affect per-chunk biome parity; it determines the world's spawn chunk and the origin passed to `ChunkGeneratorStructureState`. **[oracle]** (See Open questions #3.)

---

### 5. Biome zoom / Voronoi fuzzing (`BiomeManager`)

Used whenever the engine resolves a biome at a **block** coordinate `(x, y, z)` (surface rules in `MaterialSystem`, carver top-material replacement, feature placement, mob spawns, gameplay checks, client colors).

#### Seed obfuscation (`BiomeManager::obfuscateSeed`)
Given the 64-bit signed `worldSeed`:
1. Serialize `worldSeed` into 8 bytes in **little-endian** order (`to_le_bytes`).
2. Hash those 8 bytes with **SHA-256** to obtain a 32-byte digest `H[0..31]`.
3. Interpret the first 8 bytes `H[0..7]` as a signed 64-bit integer in **little-endian** order (`i64::from_le_bytes`).

(Matches Guava's `Hashing.sha256().hashLong(seed).asLong()`.) Both `ServerLevel` and `WorldGenRegion` construct their `BiomeManager` with `biomeZoomSeed = obfuscateSeed(worldSeed)`.

#### Exact 8-corner Voronoi lookup at block `(x, y, z) ∈ i32^3`
1. Shift by $-2$ in wrapping 32-bit arithmetic:
   $$\text{absX} = x -_{32} 2, \qquad \text{absY} = y -_{32} 2, \qquad \text{absZ} = z -_{32} 2$$
2. Compute base quart coordinates (arithmetic shift right by 2) and exact fractional offsets in `f64` ($\in \{0.0, 0.25, 0.5, 0.75\}$):
   $$\text{parentX} = \text{absX} \gg 2, \quad \text{parentY} = \text{absY} \gg 2, \quad \text{parentZ} = \text{absZ} \gg 2$$
   $$\text{fractX} = \text{f64}(\text{absX} \mathbin{\&} 3) / 4.0, \quad \text{fractY} = \text{f64}(\text{absY} \mathbin{\&} 3) / 4.0, \quad \text{fractZ} = \text{f64}(\text{absZ} \mathbin{\&} 3) / 4.0$$
3. Initialize `minI = 0` and `minDist = +Infinity` (`f64`).
4. For `i = 0, 1, 2, 3, 4, 5, 6, 7`:
   - `xEven = (i & 4) == 0`, `yEven = (i & 2) == 0`, `zEven = (i & 1) == 0`.
   - Corner quart coordinates (wrapping 32-bit `+ 1`):
     - `cornerX = xEven ? parentX : parentX + 1`
     - `cornerY = yEven ? parentY : parentY + 1`
     - `cornerZ = zEven ? parentZ : parentZ + 1`
   - Relative displacements in `f64`:
     - `dx = xEven ? fractX : fractX - 1.0`
     - `dy = yEven ? fractY : fractY - 1.0`
     - `dz = zEven ? fractZ : fractZ - 1.0`
   - Compute `d = fiddledDistance(biomeZoomSeed, cornerX, cornerY, cornerZ, dx, dy, dz)`.
   - If `minDist > d` (**strict `>`**, so ties keep the smaller index `i`): set `minI = i`, `minDist = d`.
5. Compute the winning quart `(biomeX, biomeY, biomeZ)` from `minI` using the same bit masks (`(minI & 4) == 0 ? parentX : parentX + 1`, etc.) and return `noiseBiomeSource.getNoiseBiome(biomeX, biomeY, biomeZ)`.

#### Fiddled distance and jitter function
Uses the quadratic 64-bit mixer from `docs/specs/math/java-math-helpers.md` §6:
$$\text{lcg}(r, c) = r \times_{64} (r \times_{64} 6364136223846793005 +_{64} 1442695040888963407) +_{64} c$$
and the jitter helper `fiddle(r: i64) -> f64`:
$$\text{fiddle}(r) = \left(\frac{\text{f64}(\text{floorMod}(r \gg 24,\; 1024))}{1024.0} - 0.5\right) \cdot 0.9$$
(Because $1024 = 2^{10}$, `floorMod(r >> 24, 1024)` is identical to `(r >> 24) & 1023`, yielding an integer in `[0, 1023]` and jitter in $[-0.45,\; +0.44912109375]$.)

Given `(seed: i64, cx: i32, cy: i32, cz: i32, dx: f64, dy: f64, dz: f64)` (sign-extending `cx, cy, cz` to `i64`):
1. `r = seed`
2. `r = lcg(r, cx)`, `r = lcg(r, cy)`, `r = lcg(r, cz)`
3. `r = lcg(r, cx)`, `r = lcg(r, cy)`, `r = lcg(r, cz)`
4. `fx = fiddle(r)`
5. `r = lcg(r, seed)`, `fy = fiddle(r)`
6. `r = lcg(r, seed)`, `fz = fiddle(r)`
7. Return in **exact Z, then Y, then X** left-to-right `f64` summation order (no FMA):
   $$((dz + fz)^2 + (dy + fy)^2) + (dx + fx)^2$$

---

### 6. Overworld and Nether parameter-list presets (data vs code & build rules)

#### Availability in the jar vs code vs data generator
- Inside the 26.3 jar, `data/minecraft/worldgen/multi_noise_biome_source_parameter_list/overworld.json` contains only `{"preset": "minecraft:overworld"}` and `nether.json` contains only `{"preset": "minecraft:nether"}`. **The actual parameter lists are NOT stored as data in the jar**; they are built procedurally in code.
- Running the game's data generator with `--reports` (`BiomeParametersDumpReport`) dumps both expanded lists to `reports/biome_parameters/minecraft/overworld.json` and `reports/biome_parameters/minecraft/nether.json` (every endpoint in both presets round-trips `unquantize` $\to$ `quantize` to the exact same `i64`).
- Zenith **must not** ship copied vanilla parameter JSON files in the repository. Instead, implement the exact procedural preset builders specified below (which also satisfies any data pack referencing `"preset": "minecraft:overworld"` or `"minecraft:nether"` without requiring a separate report dump), and cross-check them against `reports/biome_parameters/` during parity verification.

#### 6.1 Nether preset (`minecraft:nether`, 5 entries)
Emit the following 5 entries in exact order (every noise axis is a point interval `Parameter::point(v)`):

| Order | Biome ID | `T` | `H` | `C` | `E` | `D` | `W` | `offset` |
|---|---|---|---|---|---|---|---|---|
| 0 | `minecraft:nether_wastes` | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) |
| 1 | `minecraft:soul_sand_valley` | `0.0` (`0`) | `-0.5` (`-5000`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) |
| 2 | `minecraft:crimson_forest` | `0.4` (`4000`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) |
| 3 | `minecraft:warped_forest` | `0.0` (`0`) | `0.5` (`5000`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.375` (`3750`) |
| 4 | `minecraft:basalt_deltas` | `-0.5` (`-5000`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.0` (`0`) | `0.175` (`1750`) |

(Because $N = 5 \le 19$, the Nether R-tree builds a single root `SubTree` stably sorted by `totalMagnitude`: `nether_wastes` ($0$), `crimson_forest` ($4000$), `soul_sand_valley` ($5000$), `basalt_deltas` ($6750$), `warped_forest` ($8750$).)

#### 6.2 Overworld preset (`minecraft:overworld`, 7,594 entries)

All intervals below are built from `f32` literals via `Parameter::span` (which applies `quantize(x) = trunc_i64(x * 10000.0f)`). Note the non-obvious `f32` truncations in the table below (especially `-0.78f -> -7799` and `±0.26666668f / ±0.56666666f / ±0.7666667f -> ±2666 / ±5666 / ±7666`).

##### Named axis intervals

- **Full range**: `FULL = span(-1.0f, 1.0f)` = `[-10000, 10000]`.
- **Temperatures `T[0..4]`**:
  - `T[0]` (`FROZEN`): `span(-1.0f, -0.45f)` = `[-10000, -4500]`
  - `T[1]`: `span(-0.45f, -0.15f)` = `[-4500, -1500]`
  - `T[2]`: `span(-0.15f, 0.2f)` = `[-1500, 2000]`
  - `T[3]`: `span(0.2f, 0.55f)` = `[2000, 5500]`
  - `T[4]`: `span(0.55f, 1.0f)` = `[5500, 10000]`
  - `UNFROZEN = span(T[1], T[4])` = `[-4500, 10000]`
- **Humidities `H[0..4]`**:
  - `H[0]`: `span(-1.0f, -0.35f)` = `[-10000, -3500]`
  - `H[1]`: `span(-0.35f, -0.1f)` = `[-3500, -1000]`
  - `H[2]`: `span(-0.1f, 0.1f)` = `[-1000, 1000]`
  - `H[3]`: `span(0.1f, 0.3f)` = `[1000, 3000]`
  - `H[4]`: `span(0.3f, 1.0f)` = `[3000, 10000]`
- **Erosions `E[0..6]`**:
  - `E[0]`: `span(-1.0f, -0.78f)` = `[-10000, -7799]` *(note `-7799`, not `-7800`!)*
  - `E[1]`: `span(-0.78f, -0.375f)` = `[-7799, -3750]`
  - `E[2]`: `span(-0.375f, -0.2225f)` = `[-3750, -2225]`
  - `E[3]`: `span(-0.2225f, 0.05f)` = `[-2225, 500]`
  - `E[4]`: `span(0.05f, 0.45f)` = `[500, 4500]`
  - `E[5]`: `span(0.45f, 0.55f)` = `[4500, 5500]`
  - `E[6]`: `span(0.55f, 1.0f)` = `[5500, 10000]`
- **Continentalness bands**:
  - `C_MUSHROOM`: `span(-1.2f, -1.05f)` = `[-12000, -10500]`
  - `C_DEEP_OCEAN`: `span(-1.05f, -0.455f)` = `[-10500, -4550]`
  - `C_OCEAN`: `span(-0.455f, -0.19f)` = `[-4550, -1900]`
  - `C_COAST`: `span(-0.19f, -0.11f)` = `[-1900, -1100]`
  - `C_INLAND`: `span(-0.11f, 0.55f)` = `[-1100, 5500]`
  - `C_NEAR`: `span(-0.11f, 0.03f)` = `[-1100, 300]`
  - `C_MID`: `span(0.03f, 0.3f)` = `[300, 3000]`
  - `C_FAR`: `span(0.3f, 1.0f)` = `[3000, 10000]`
- Notation: `span(A, B)` denotes `[A.min, B.max]`.

##### Biome lookup matrices (all IDs in `minecraft:` namespace; `—` means absent / fallback)

**`OCEANS[depthRow][T]`** (`depthRow = 0` deep, `1` normal; `T = 0..4`):
- Row 0: `deep_frozen_ocean`, `deep_cold_ocean`, `deep_ocean`, `deep_lukewarm_ocean`, `warm_ocean`
- Row 1: `frozen_ocean`, `cold_ocean`, `ocean`, `lukewarm_ocean`, `warm_ocean`

**`MIDDLE[T][H]`** (`T = 0..4` rows, `H = 0..4` columns):

| `T \ H` | `0` | `1` | `2` | `3` | `4` |
|---|---|---|---|---|---|
| `0` | `snowy_plains` | `snowy_plains` | `snowy_plains` | `snowy_taiga` | `taiga` |
| `1` | `plains` | `plains` | `forest` | `taiga` | `old_growth_spruce_taiga` |
| `2` | `flower_forest` | `plains` | `forest` | `birch_forest` | `dark_forest` |
| `3` | `savanna` | `savanna` | `forest` | `jungle` | `jungle` |
| `4` | `desert` | `desert` | `desert` | `desert` | `desert` |

**`MIDDLE_VARIANT[T][H]`**:

| `T \ H` | `0` | `1` | `2` | `3` | `4` |
|---|---|---|---|---|---|
| `0` | `ice_spikes` | — | `snowy_taiga` | — | — |
| `1` | `dappled_forest` | — | — | — | `old_growth_pine_taiga` |
| `2` | `sunflower_plains` | — | — | `old_growth_birch_forest` | — |
| `3` | — | — | `plains` | `sparse_jungle` | `bamboo_jungle` |
| `4` | — | — | — | — | — |

*(Note: `minecraft:dappled_forest` at `[1][0]` is a 26.3 Overworld biome.)*

**`PLATEAU[T][H]`**:

| `T \ H` | `0` | `1` | `2` | `3` | `4` |
|---|---|---|---|---|---|
| `0` | `snowy_plains` | `snowy_plains` | `snowy_plains` | `snowy_taiga` | `snowy_taiga` |
| `1` | `meadow` | `meadow` | `forest` | `taiga` | `old_growth_spruce_taiga` |
| `2` | `meadow` | `meadow` | `meadow` | `meadow` | `pale_garden` |
| `3` | `savanna_plateau` | `savanna_plateau` | `forest` | `forest` | `jungle` |
| `4` | `badlands` | `badlands` | `badlands` | `wooded_badlands` | `wooded_badlands` |

**`PLATEAU_VARIANT[T][H]`**:

| `T \ H` | `0` | `1` | `2` | `3` | `4` |
|---|---|---|---|---|---|
| `0` | `ice_spikes` | — | — | — | — |
| `1` | `cherry_grove` | — | `meadow` | `meadow` | `old_growth_pine_taiga` |
| `2` | `cherry_grove` | `cherry_grove` | `forest` | `birch_forest` | — |
| `3` | — | — | — | — | — |
| `4` | `eroded_badlands` | `eroded_badlands` | — | — | — |

**`SHATTERED[T][H]`**:

| `T \ H` | `0` | `1` | `2` | `3` | `4` |
|---|---|---|---|---|---|
| `0` | `windswept_gravelly_hills` | `windswept_gravelly_hills` | `windswept_hills` | `windswept_forest` | `windswept_forest` |
| `1` | `windswept_gravelly_hills` | `windswept_gravelly_hills` | `windswept_hills` | `windswept_forest` | `windswept_forest` |
| `2` | `windswept_hills` | `windswept_hills` | `windswept_hills` | `windswept_forest` | `windswept_forest` |
| `3` | — | — | — | — | — |
| `4` | — | — | — | — | — |

##### Selection helper functions
Given `(t: 0..4, h: 0..4, w: Parameter)`, let `negW = (w.max < 0)`:
1. `pickMiddle(t, h, w)`: if `negW` return `MIDDLE[t][h]`; else return `MIDDLE_VARIANT[t][h]` if present, else `MIDDLE[t][h]`.
2. `pickBadlands(h, w)`: if `h < 2` return (`negW ? badlands : eroded_badlands`); else if `h < 3` return `badlands`; else return `wooded_badlands`.
3. `pickMiddleOrBadlands(t, h, w)`: if `t == 4` return `pickBadlands(h, w)`; else `pickMiddle(t, h, w)`.
4. `pickPlateau(t, h, w)`: if `!negW` and `PLATEAU_VARIANT[t][h]` is present, return it; else return `PLATEAU[t][h]`.
5. `pickSlope(t, h, w)`: if `t >= 3` return `pickPlateau(t, h, w)`; else return (`h <= 1 ? snowy_slopes : grove`).
6. `pickMiddleOrBadlandsOrSlope(t, h, w)`: if `t == 0` return `pickSlope(t, h, w)`; else `pickMiddleOrBadlands(t, h, w)`.
7. `pickPeak(t, h, w)`: if `t <= 2` return (`negW ? jagged_peaks : frozen_peaks`); else if `t == 3` return `stony_peaks`; else `pickBadlands(h, w)`.
8. `pickShattered(t, h, w)`: return `SHATTERED[t][h]` if present, else `pickMiddle(t, h, w)`.
9. `maybeWindsweptSavanna(t, h, w, base)`: if `t > 1 && h < 4 && !negW` return `windswept_savanna`; else return `base`.
10. `pickBeach(t, h)`: if `t == 0` return `snowy_beach`; else if `t == 4` return `desert`; else return `beach`.
11. `pickShatteredCoast(t, h, w)`: let `base = (!negW) ? pickMiddle(t, h, w) : pickBeach(t, h)`; return `maybeWindsweptSavanna(t, h, w, base)`.

##### Emission primitives
Every emission uses `offset = 0.0f` (`0`):
- **`emitSurface(T, H, C, E, W, biome)`**: appends **two** entries in order:
  1. `(T, H, C, E, point(0.0f) [0, 0], W, 0) -> biome`
  2. `(T, H, C, E, point(1.0f) [10000, 10000], W, 0) -> biome`
- **`emitUnderground(T, H, C, E, W, biome)`**: appends **one** entry with `D = span(0.2f, 0.9f)` (`[2000, 9000]`).
- **`emitBottom(T, H, C, E, W, biome)`**: appends **one** entry with `D = point(1.1f)` (`[11000, 11000]`).

##### Exact ordered emission sequence (3 phases)

**Phase 1 — Off-coast biomes (11 `emitSurface` = 22 entries)**:
1. `emitSurface(FULL, FULL, C_MUSHROOM, FULL, FULL, mushroom_fields)`.
2. For `t = 0, 1, 2, 3, 4`:
   1. `emitSurface(T[t], FULL, C_DEEP_OCEAN, FULL, FULL, OCEANS[0][t])`
   2. `emitSurface(T[t], FULL, C_OCEAN, FULL, FULL, OCEANS[1][t])`

**Phase 2 — Inland biomes across 13 ordered weirdness slices (7,568 entries)**:
Execute the slice builders in this exact order:

| # | Slice builder | `W` (`f32` span) | Quantized `W` (`i64`) | `negW` (`W.max < 0`) | Entries |
|---|---|---|---|---|---|
| 1 | Mid | `[-1.0, -0.93333334]` | `[-10000, -9333]` | `true` | 716 |
| 2 | High | `[-0.93333334, -0.7666667]` | `[-9333, -7666]` | `true` | 650 |
| 3 | Peaks | `[-0.7666667, -0.56666666]` | `[-7666, -5666]` | `true` | 550 |
| 4 | High | `[-0.56666666, -0.4]` | `[-5666, -4000]` | `true` | 650 |
| 5 | Mid | `[-0.4, -0.26666668]` | `[-4000, -2666]` | `true` | 716 |
| 6 | Low | `[-0.26666668, -0.05]` | `[-2666, -500]` | `true` | 516 |
| 7 | Valleys | `[-0.05, 0.05]` | `[-500, 500]` | `false` | 72 |
| 8 | Low | `[0.05, 0.26666668]` | `[500, 2666]` | `false` | 516 |
| 9 | Mid | `[0.26666668, 0.4]` | `[2666, 4000]` | `false` | 666 |
| 10 | High | `[0.4, 0.56666666]` | `[4000, 5666]` | `false` | 650 |
| 11 | Peaks | `[0.56666666, 0.7666667]` | `[5666, 7666]` | `false` | 550 |
| 12 | High | `[0.7666667, 0.93333334]` | `[7666, 9333]` | `false` | 650 |
| 13 | Mid | `[0.93333334, 1.0]` | `[9333, 10000]` | `false` | 666 |

Each slice builder emits via `emitSurface` with the slice's weirdness `w`:

- **Peaks slice**: for `t = 0..4`, for `h = 0..4`, emit 11 calls with `(T[t], H[h], C, E, w, biome)`:
  1. `span(C_COAST, C_FAR)`, `E[0]` $\to$ `pickPeak`
  2. `span(C_COAST, C_NEAR)`, `E[1]` $\to$ `pickMiddleOrBadlandsOrSlope`
  3. `span(C_MID, C_FAR)`, `E[1]` $\to$ `pickPeak`
  4. `span(C_COAST, C_NEAR)`, `span(E[2], E[3])` $\to$ `pickMiddle`
  5. `span(C_MID, C_FAR)`, `E[2]` $\to$ `pickPlateau`
  6. `C_MID`, `E[3]` $\to$ `pickMiddleOrBadlands`
  7. `C_FAR`, `E[3]` $\to$ `pickPlateau`
  8. `span(C_COAST, C_FAR)`, `E[4]` $\to$ `pickMiddle`
  9. `span(C_COAST, C_NEAR)`, `E[5]` $\to$ `maybeWindsweptSavanna(pickShattered)`
  10. `span(C_MID, C_FAR)`, `E[5]` $\to$ `pickShattered`
  11. `span(C_COAST, C_FAR)`, `E[6]` $\to$ `pickMiddle`

- **High slice**: for `t = 0..4`, for `h = 0..4`, emit 13 calls with `(T[t], H[h], C, E, w, biome)`:
  1. `C_COAST`, `span(E[0], E[1])` $\to$ `pickMiddle`
  2. `C_NEAR`, `E[0]` $\to$ `pickSlope`
  3. `span(C_MID, C_FAR)`, `E[0]` $\to$ `pickPeak`
  4. `C_NEAR`, `E[1]` $\to$ `pickMiddleOrBadlandsOrSlope`
  5. `span(C_MID, C_FAR)`, `E[1]` $\to$ `pickSlope`
  6. `span(C_COAST, C_NEAR)`, `span(E[2], E[3])` $\to$ `pickMiddle`
  7. `span(C_MID, C_FAR)`, `E[2]` $\to$ `pickPlateau`
  8. `C_MID`, `E[3]` $\to$ `pickMiddleOrBadlands`
  9. `C_FAR`, `E[3]` $\to$ `pickPlateau`
  10. `span(C_COAST, C_FAR)`, `E[4]` $\to$ `pickMiddle`
  11. `span(C_COAST, C_NEAR)`, `E[5]` $\to$ `maybeWindsweptSavanna(pickMiddle)`
  12. `span(C_MID, C_FAR)`, `E[5]` $\to$ `pickShattered`
  13. `span(C_COAST, C_FAR)`, `E[6]` $\to$ `pickMiddle`

- **Mid slice**:
  - Pre-loop (3 calls):
    1. `FULL`, `FULL`, `C_COAST`, `span(E[0], E[2])` $\to$ `stony_shore`
    2. `span(T[1], T[2])`, `FULL`, `span(C_NEAR, C_FAR)`, `E[6]` $\to$ `swamp`
    3. `span(T[3], T[4])`, `FULL`, `span(C_NEAR, C_FAR)`, `E[6]` $\to$ `mangrove_swamp`
  - Loop `t = 0..4`, `h = 0..4`, with `(T[t], H[h], C, E, w, biome)`:
    1. `span(C_NEAR, C_FAR)`, `E[0]` $\to$ `pickSlope`
    2. `span(C_NEAR, C_MID)`, `E[1]` $\to$ `pickMiddleOrBadlandsOrSlope`
    3. `C_FAR`, `E[1]` $\to$ (`t == 0 ? pickSlope : pickPlateau`)
    4. `C_NEAR`, `E[2]` $\to$ `pickMiddle`
    5. `C_MID`, `E[2]` $\to$ `pickMiddleOrBadlands`
    6. `C_FAR`, `E[2]` $\to$ `pickPlateau`
    7. `span(C_COAST, C_NEAR)`, `E[3]` $\to$ `pickMiddle`
    8. `span(C_MID, C_FAR)`, `E[3]` $\to$ `pickMiddleOrBadlands`
    9. If `negW`: emit `C_COAST`, `E[4]` $\to$ `pickBeach`, then `span(C_NEAR, C_FAR)`, `E[4]` $\to$ `pickMiddle`. Else (`!negW`): emit single `span(C_COAST, C_FAR)`, `E[4]` $\to$ `pickMiddle`.
    10. `C_COAST`, `E[5]` $\to$ `pickShatteredCoast`
    11. `C_NEAR`, `E[5]` $\to$ `maybeWindsweptSavanna(pickMiddle)`
    12. `span(C_MID, C_FAR)`, `E[5]` $\to$ `pickShattered`
    13. `C_COAST`, `E[6]` $\to$ (`negW ? pickBeach : pickMiddle`)
    14. If `t == 0`: `span(C_NEAR, C_FAR)`, `E[6]` $\to$ `pickMiddle`

- **Low slice**:
  - Pre-loop (3 calls):
    1. `FULL`, `FULL`, `C_COAST`, `span(E[0], E[2])` $\to$ `stony_shore`
    2. `span(T[1], T[2])`, `FULL`, `span(C_NEAR, C_FAR)`, `E[6]` $\to$ `swamp`
    3. `span(T[3], T[4])`, `FULL`, `span(C_NEAR, C_FAR)`, `E[6]` $\to$ `mangrove_swamp`
  - Loop `t = 0..4`, `h = 0..4`, with `(T[t], H[h], C, E, w, biome)`:
    1. `C_NEAR`, `span(E[0], E[1])` $\to$ `pickMiddleOrBadlands`
    2. `span(C_MID, C_FAR)`, `span(E[0], E[1])` $\to$ `pickMiddleOrBadlandsOrSlope`
    3. `C_NEAR`, `span(E[2], E[3])` $\to$ `pickMiddle`
    4. `span(C_MID, C_FAR)`, `span(E[2], E[3])` $\to$ `pickMiddleOrBadlands`
    5. `C_COAST`, `span(E[3], E[4])` $\to$ `pickBeach`
    6. `span(C_NEAR, C_FAR)`, `E[4]` $\to$ `pickMiddle`
    7. `C_COAST`, `E[5]` $\to$ `pickShatteredCoast`
    8. `C_NEAR`, `E[5]` $\to$ `maybeWindsweptSavanna(pickMiddle)`
    9. `span(C_MID, C_FAR)`, `E[5]` $\to$ `pickMiddle`
    10. `C_COAST`, `E[6]` $\to$ `pickBeach`
    11. If `t == 0`: `span(C_NEAR, C_FAR)`, `E[6]` $\to$ `pickMiddle`

- **Valleys slice**:
  - Pre-loop (11 calls):
    1. `FROZEN`, `FULL`, `C_COAST`, `span(E[0], E[1])` $\to$ (`negW ? stony_shore : frozen_river`)
    2. `UNFROZEN`, `FULL`, `C_COAST`, `span(E[0], E[1])` $\to$ (`negW ? stony_shore : river`)
    3. `FROZEN`, `FULL`, `C_NEAR`, `span(E[0], E[1])` $\to$ `frozen_river`
    4. `UNFROZEN`, `FULL`, `C_NEAR`, `span(E[0], E[1])` $\to$ `river`
    5. `FROZEN`, `FULL`, `span(C_COAST, C_FAR)`, `span(E[2], E[5])` $\to$ `frozen_river`
    6. `UNFROZEN`, `FULL`, `span(C_COAST, C_FAR)`, `span(E[2], E[5])` $\to$ `river`
    7. `FROZEN`, `FULL`, `C_COAST`, `E[6]` $\to$ `frozen_river`
    8. `UNFROZEN`, `FULL`, `C_COAST`, `E[6]` $\to$ `river`
    9. `span(T[1], T[2])`, `FULL`, `span(C_INLAND, C_FAR)`, `E[6]` $\to$ `swamp`
    10. `span(T[3], T[4])`, `FULL`, `span(C_INLAND, C_FAR)`, `E[6]` $\to$ `mangrove_swamp`
    11. `FROZEN`, `FULL`, `span(C_INLAND, C_FAR)`, `E[6]` $\to$ `frozen_river`
  - Loop `t = 0..4`, `h = 0..4`:
    1. `T[t]`, `H[h]`, `span(C_MID, C_FAR)`, `span(E[0], E[1])` $\to$ `pickMiddleOrBadlands`

**Phase 3 — Underground biomes (4 entries)**:
1. `emitUnderground(FULL, FULL, span(0.8f, 1.0f), FULL, FULL, dripstone_caves)` (`C = [8000, 10000]`, `D = [2000, 9000]`)
2. `emitUnderground(FULL, span(0.7f, 1.0f), FULL, FULL, FULL, lush_caves)` (`H = [7000, 10000]`, `D = [2000, 9000]`)
3. `emitUnderground(FULL, FULL, span(C_COAST, C_INLAND), span(E[5], E[6]), span(-1.1f, -0.85f), sulfur_caves)` (`C = [-1900, 5500]`, `E = [4500, 10000]`, `W = [-11000, -8500]`, `D = [2000, 9000]`) *(new 26.3 underground biome)*
4. `emitBottom(FULL, FULL, FULL, span(E[0], E[1]), FULL, deep_dark)` (`E = [-10000, -3750]`, `D = [11000, 11000]`)

Total Overworld entries: $22 + 7{,}568 + 4 = 7{,}594$.

---

## Edge cases and known quirks

1. **`f32` quantization truncation (`-0.78f -> -7799`)**: `quantize(x)` multiplies in `f32` and truncates toward zero. Consequently `-0.78f * 10000.0f = -7799.9995f` truncates to `-7799` (not `-7800`), and `±0.26666668f`, `±0.56666666f`, `±0.7666667f` truncate to `±2666`, `±5666`, `±7666` (not `±2667`, `±5667`, `±7667`). Using `f64` or rounding breaks Overworld boundary parity.
2. **26.3 R-tree branching factor is `19`**: `CHILDREN_PER_NODE = 19` (changed from 6/10 in earlier versions). Both the leaf-group cutoff (`size <= 19`) and `bucketize`'s `pow(19, floor(log(n - 0.01) / log(19)))` depend on `19`.
3. **R-tree `build` in-place stable sort + `totalMagnitude` tie preservation**: During split selection, `nodes` is sorted in place by cyclic signed midpoint (`(min + max) /_trunc 2`, truncating toward zero) for `d = 0..6`. Each `buckets` snapshot copies slices before `d + 1` re-sorts `nodes`. When `build` later reaches `len <= 19` and stably sorts by `totalMagnitude`, ties preserve the order from the parent's `bestDim` sort (or original list order when $N \le 19$ at the root).
4. **R-tree `lastResult` warm-start wins distance ties**: `SubTree::search` uses strict `>` (`minDist > childDist` and `minDist > leafDist`), so `lastResult` beats any equally distant leaf regardless of tree order, and among other ties the first leaf in depth-first tree order wins.
5. **Chunk fill loop order is X → Y → Z while container storage is Y → Z → X**: `LevelChunkSection::fillBiomesFromNoise` queries `x` outer, `y` middle, `z` inner (which determines `lastResult` progression across the 64 quarts), while `PalettedContainer` stores entries at `((y << 2) | z) << 2 | x`.
6. **Multi-noise `createResolverForChunk` uses `sampleVolume`, not `sampleValue`**: Chunk biome generation uses `DensitySampler.Bound::sampleVolume` over a stride-4 `DensityVolume`, whereas uncached queries and carvers use `sampleValue`. Non-dyadic noise frequencies can differ by 1 ULP between volume and point paths (see `docs/specs/math/noise.md` §5).
7. **Multi-noise `mapEither` prefers `"biomes"` over `"preset"`**: If a JSON object specifies both `"biomes"` and `"preset"`, `"biomes"` decodes first and `"preset"` is ignored.
8. **`BiomeManager::obfuscateSeed` is little-endian on BOTH input and output**: The 64-bit world seed is encoded as 8 little-endian bytes before SHA-256, and the first 8 digest bytes are decoded as a little-endian signed `i64`.
9. **`BiomeManager` distance sums in Z → Y → X order and breaks ties toward smaller corner index**: `getFiddledDistance` evaluates `((dz + fz)^2 + (dy + fy)^2) + (dx + fx)^2` left-to-right in `f64`, and `getBiome` updates `minI` only on strict `minDist > d`, so exact ties pick the lowest corner bitmask `i ∈ 0..7`.
10. **`TheEndBiomeSource` thresholds and `NaN` fallback**: `heightValue > 0.25` (strict) is Highlands, `>= -0.0625` (inclusive) is Midlands, `< -0.21875` (strict) is Small Islands, and the `else` branch (`[-0.21875, -0.0625)` as well as `NaN`) is Barrens. Outer End biomes evaluate `erosion` at `(chunkX * 16 + 8, quartY * 4, chunkZ * 16 + 8)`, so all 16 horizontal quarts in a chunk share the same `(sampleX, sampleZ)`.
11. **`CheckerboardColumnBiomeSource` 32-bit shift wrapping**: `scale` accepts `[0, 62]` and `bitShift = scale + 2`, so for `scale >= 30`, Java's 32-bit `quart >> bitShift` masks `bitShift` by `& 31`.
12. **26.3 new Overworld biomes**: `OverworldBiomeBuilder` includes `minecraft:dappled_forest` (`MIDDLE_VARIANT[1][0]`), `minecraft:pale_garden` (`PLATEAU[2][4]`), and `minecraft:sulfur_caves` (underground phase).
13. **`ChunkAccess::getNoiseBiome` clamps `quartY`, never fails vertically**: Out-of-bounds `quartY` (such as `BiomeManager` reading `minQuartY - 1` at `blockY = minY` or `maxQuartY + 1` at `blockY = maxY`) clamps silently to `[minQuartY, maxQuartY]`.

---

## Constants and IDs

### Numeric constants
| Constant | Type | Value | Meaning |
|---|---|---|---|
| `QUANTIZATION_FACTOR` | `f32` | `10000.0f` (`0x461C4000`) | Climate coordinate quantization scale |
| `PARAMETER_COUNT` | `i32` | `7` | Climate R-tree dimensions (`T, H, C, E, D, W, offset`) |
| `CHILDREN_PER_NODE` | `i32` | `19` | R-tree branching factor in 26.3 |
| Bucket-size epsilon | `f64` | `0.01` | Subtracted from `nodes.size()` in `log(n - 0.01) / log(19)` |
| Parameter JSON bounds | `f32` | `[-2.0f, 2.0f]` | Allowed interval endpoints in JSON |
| Offset JSON bounds | `f32` | `[0.0f, 1.0f]` | Allowed `offset` range in JSON |
| End central island $R^2$ | `i64` | `4096` | Max `chunkX^2 + chunkZ^2` for `minecraft:the_end` |
| End Highlands threshold | `f64` | `0.25` (`>`) | `erosion` cutoff for `minecraft:end_highlands` |
| End Midlands threshold | `f64` | `-0.0625` (`>=`) | `erosion` cutoff for `minecraft:end_midlands` |
| End Small Islands threshold | `f64` | `-0.21875` (`<`) | `erosion` cutoff for `minecraft:small_end_islands` |
| Voronoi zoom offset / divisor | `i32` / `f64` | `-2` / `4.0` | Block-to-Voronoi cell shift and fractional scale |
| Voronoi LCG multiplier | `i64` | `6364136223846793005` (`0x5851F42D4C957F2D`) | Quadratic mixer multiplier |
| Voronoi LCG increment | `i64` | `1442695040888963407` (`0x14057B7EF767814F`) | Quadratic mixer increment |
| Voronoi fiddle resolution / scale | `i32` / `f64` | `1024` / `0.9` | `((floorMod(r >> 24, 1024) / 1024.0) - 0.5) * 0.9` |
| `NoiseSpawnFinder` distance weight | `i64` | `4194304` (`2048^2`) | Multiplier on climate fitness vs origin distance squared |
| Overworld / Nether preset entry counts | `usize` | `7594` / `5` | Exact entry counts produced by Section 6 rules |

### `BiomeManager::obfuscateSeed` reference test vectors
| Input `worldSeed` (`i64`) | Output `biomeZoomSeed` (`i64`) | Hex (64-bit) |
|---|---|---|
| `0` | `8794265229978523055` | `0x7A0B81A1F57055AF` |
| `1` | `-6467378160175308932` | `0xA63F41D436A19F7C` |
| `-1` | `6759447113877070610` | `0x5DCE615644AEA312` |
| `1234567890123456789` | `7890956078138091352` | `0x6D824EBEB7A16758` |

### Registry and type identifiers
- Built-in biome-source codec registry: `minecraft:worldgen/biome_source`
  - `minecraft:fixed`, `minecraft:multi_noise`, `minecraft:checkerboard`, `minecraft:the_end`
- Dynamic biome registry: `minecraft:worldgen/biome`
- Dynamic parameter-list registry: `minecraft:worldgen/multi_noise_biome_source_parameter_list`
  - Built-in preset IDs: `minecraft:overworld`, `minecraft:nether`
- Data-generator report paths: `reports/biome_parameters/minecraft/overworld.json`, `reports/biome_parameters/minecraft/nether.json`

---

## Oracle probe points

Public class and method entry points in 26.3 for black-box parity dumping:

1. **Router climate float values at positions**:
   - Point path: `net.minecraft.world.level.levelgen.RandomState#createClimateSampler(SamplerContext)` $\to$ `Climate.Sampler#temperature()` / `#humidity()` / `#continentalness()` / `#erosion()` / `#depth()` / `#weirdness()`, each calling `DensitySampler.Bound#sampleValue(int blockX, int blockY, int blockZ)`.
   - Volume path (chunk generation): `DensitySampler.Bound#sampleVolume(DensityBuffer, DensityVolume)` with a stride-4 `DensityVolume(4, height >> 2, 4, chunkMinBlockX, minY, chunkMinBlockZ, 4, 4, 4)`.
2. **Quantized target point**:
   - `net.minecraft.world.level.biome.Climate#quantizeCoord(float)` and `Climate#unquantizeCoord(long)`.
   - `net.minecraft.world.level.biome.Climate.Sampler#sample(int quartX, int quartY, int quartZ)` $\to$ `Climate.TargetPoint` (`#temperature()`, `#humidity()`, `#continentalness()`, `#erosion()`, `#depth()`, `#weirdness()`).
3. **Chosen biome per quart position**:
   - Direct multi-noise lookup: `net.minecraft.world.level.biome.MultiNoiseBiomeSource#getNoiseBiome(Climate.TargetPoint)` and `Climate.ParameterList#findValue(Climate.TargetPoint)` (plus `#findValueBruteForce(Climate.TargetPoint)` for tie diagnostics).
   - Point resolver (all biome sources): `net.minecraft.world.level.biome.BiomeSource#createUncachedResolver(RandomState)` or `#createCachingResolver(RandomState)` $\to$ `BiomeResolver#getNoiseBiome(int quartX, int quartY, int quartZ)`.
   - Chunk resolver (used by `BIOMES` step): `BiomeSource#createResolverForChunk(Climate.Sampler, int minQuartX, int minQuartY, int minQuartZ, int quartSizeX, int quartSizeY, int quartSizeZ)` $\to$ `BiomeResolver#getNoiseBiome(int, int, int)`.
   - Stored chunk result after `BIOMES` step: `ChunkAccess#getNoiseBiome(int quartX, int quartY, int quartZ)` and `LevelChunkSection#getNoiseBiome(int localQuartX, int localQuartY, int localQuartZ)`.
4. **Zoomed biome per block position & seed obfuscation**:
   - `net.minecraft.world.level.biome.BiomeManager#obfuscateSeed(long)`.
   - `net.minecraft.world.level.biome.BiomeManager#getBiome(int blockX, int blockY, int blockZ)` (and `#getBiome(BlockPos)`), backed by either a generated chunk/region (`LevelReader#getBiome(BlockPos)`) or a deterministic mock `BiomeResolver`.
5. **Preset tables and spawn finding**:
   - `net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList#knownPresets()` (and `reports/biome_parameters/minecraft/{overworld,nether}.json`).
   - `net.minecraft.world.level.levelgen.NoiseSpawnFinder#findSpawnPosition(List<SpawnTargetPoint>, DensitySamplerSet)` and `NoiseBasedChunkGenerator#getOrigin(RandomState)`.

---

## Open questions

1. **[oracle] Cross-chunk `RTree` `lastResult` tie-breaking sensitivity**: `Climate.RTree` keeps `lastResult` in a `ThreadLocal` that persists across chunks processed on the same worker thread. Because `SubTree::search` uses strict `>`, if the first quart of a chunk (`(0, minQuartY, 0)`) has an exact distance tie between two distinct biomes, the winner depends on the previous chunk's final leaf on that thread (or `null` on a fresh thread).
   - *Probe*: Sample $100+$ Overworld and Nether chunks across 5+ seeds via `MultiNoiseBiomeSource#createResolverForChunk` comparing a fresh-thread (`lastResult = null` at chunk start) run against multi-chunk sequential runs and `ParameterList#findValueBruteForce`, recording whether any distance tie between distinct biomes occurs in practice.
2. **[oracle] Volume vs point 1-ULP quantization flips in `MultiNoiseBiomeSource`**: `createResolverForChunk` uses `sampleVolume` while `createResolver` uses `sampleValue`. Because `sampleVolume` multiplies `scale * layerFreq` differently from `(block * scale) * layerFreq`, a 1-ULP float difference on an exact integer boundary of `x * 10000.0f` could shift a quantized coordinate by 1.
   - *Probe*: For 50+ Overworld and Nether chunks across multiple seeds, compare the 6 climate floats and resulting `TargetPoint`s from `createResolverForChunk` (`sampleVolume`) against `createCachingResolver` / `createUncachedResolver` (`sampleValue`) and verify Zenith matches the volume path for chunk biomes and the point path for uncached lookups.
3. **[oracle] Host `Math.sin` / `Math.cos` parity in `NoiseSpawnFinder`**: `NoiseSpawnFinder::radialSearch` calls JVM `Math.sin` and `Math.cos` (not `StrictMath`), which may use platform intrinsics differing by 1 ULP from fdlibm on some CPUs.
   - *Probe*: Run `NoiseSpawnFinder#findSpawnPosition` across 100 seeds on each CI platform and compare against Zenith's `sin`/`cos` implementation; record any per-platform divergence.
4. **[oracle] Overworld R-tree build parity**: Confirm that Zenith's procedural Overworld (7,594 entries) and Nether (5 entries) builders match `reports/biome_parameters/minecraft/{overworld,nether}.json` entry-for-entry (including exact quantized `i64` bounds and order) and that Zenith's 19-way R-tree produces bit-identical `findValue` results to vanilla's `Climate.RTree` across 100,000+ synthetic boundary/tie `TargetPoint`s (including exact bucket-size boundaries around $19^k$).

---

## Parity status

not yet run
