# Math: noise generators (26.3)

- Verified by suites: `noise-samples` (oracle point, derivative, volume, and parity-config samples × property tests)
- Sources: Ken Perlin's improved gradient noise (2002), Stefan Gustavson's simplex noise formulation (2005), data-pack JSON schemas for `minecraft:worldgen/noise` and `minecraft:old_blended_noise`, `docs/specs/math/random.md` for RNG behavior, and local reference reading (2026-11-24) for 26.3 exact precision, ordering, and quirks. Everything below is written in our own words.

## Summary

Version 26.3 uses a unified single-precision (`f32`) output pipeline for all noise generators, while keeping world coordinates and lattice setup in double precision (`f64`). Every generator provides 2D sampling `(f64, f64) -> f32`, 3D sampling `(f64, f64, f64) -> f32`, a conservative `f32` output interval `[min, max]`, and volume accumulation over a 3D regular block grid.

The noise family consists of:
1. **Gradient-noise foundation**: shared origin offsets, 256-byte permutation table, 16-vector gradient table, and coordinate wrapping.
2. **Improved Perlin noise** (single octave): 3D and 2D gradient noise in `f32`, plus analytical partial derivatives.
3. **Smeared Perlin noise** (deprecated single octave): Perlin noise with quantized Y-axis "y-fuzz" displacement used by blended terrain noise.
4. **Simplex noise** (single octave): 2D and 3D simplex gradient noise computed in `f64` and narrowed to `f32`.
5. **Octave stack**: flat sequence of `(noise, frequency: f64, amplitude: f32)` layers accumulated in `f32`.
6. **Legacy FBM initializer** (deprecated): sequential octave-stack builder with 262-draw octave skipping for legacy Nether biome noises.
7. **Double-Perlin (normal) noise**: parameterized specification (JSON-backed in `minecraft:worldgen/noise`) that compiles to an octave stack of paired Perlin octaves, with three normalization modes and a legacy Nether instantiation path.
8. **Blended noise** (`minecraft:old_blended_noise`): 5-parameter density function combining three smeared-Perlin octave stacks via clamped linear interpolation.
9. **Noise utility helpers** (`NoiseUtils`): trigonometric extreme-bias remap and parity configuration formatting.

## Inputs and state

### Arithmetic conventions
- Floating-point values are IEEE-754 binary32 (`f32`) and binary64 (`f64`), round-to-nearest-even, **without** fused multiply-add (every multiply and add rounds independently).
- Addition, subtraction, and multiplication chains associate **left to right** unless parenthesized.
- Integer operations are two's-complement wrapping (`i32` is 32-bit signed, `i8` is 8-bit signed).
- **Floor to `i32`**, written $\lfloor v \rfloor_{32}$: apply IEEE-754 floor (`floor(v)` in `f64` or `f32`), then narrow to `i32` with Java saturating semantics ($\text{NaN} \mapsto 0$, $v \ge 2^{31}-1 \mapsto 2^{31}-1$, $v \le -2^{31} \mapsto -2^{31}$).
- **Random source**: either the legacy 48-bit LCG or xoroshiro128++ source from `docs/specs/math/random.md`. All `nextDouble()`, `nextInt(bound)`, `consumeCount(n)`, positional factory creation, and `fromHashOf(name)` semantics follow that specification.

### Interval bounds
Each generator reports a conservative `f32` closed interval $[a, b]$ (or not-an-interval `NaI` if a bound becomes `NaN`):
- Symmetric interval of radius $r \ge 0$: $[-r, r]$.
- Exact interval of scalar $c$: $[c, c]$.
- Interval addition: $[a_1, b_1] + [a_2, b_2] = [a_1 + a_2, b_1 + b_2]$ in `f32` (`NaI` if either bound is `NaN`).
- Interval multiplication: compute the four corner products using zero-absorbing multiplication ($u \odot v = 0.0_f$ if $u = 0.0_f$ or $v = 0.0_f$, else $u \cdot v$ in `f32`); take $[\min, \max]$ of the four products (`NaI` if either input is `NaI`).

### Volume accumulation grid
A 3D sampling volume has positive dimensions $(S_x, S_y, S_z)$, integer origin $(X_0, Y_0, Z_0)$, and positive integer strides $(\Delta_x, \Delta_y, \Delta_z)$.
- Block coordinates are $x(i_x) = X_0 + i_x \Delta_x$, $y(i_y) = Y_0 + i_y \Delta_y$, $z(i_z) = Z_0 + i_z \Delta_z$.
- Flat buffer layout is Z-major, then X, then Y contiguous: $\text{index}(i_x, i_y, i_z) = i_y + (i_x + i_z S_x) S_y$.
- Default volume accumulation loops $i_z \in [0, S_z)$, $i_x \in [0, S_x)$, $i_y \in [0, S_y)$, scales coordinates as $x(i_x) \cdot s_{xz}$, $y(i_y) \cdot s_y$, $z(i_z) \cdot s_{xz}$ in `f64`, and adds $a \cdot \text{sample}$ into `buffer[index]` in `f32`.

## Behavior (ordered rules)

### 1. Gradient-noise foundation

All single-octave gradient generators (Perlin, smeared Perlin, and simplex) share the same state and initialization procedure.

#### State
- Three origin offsets $(o_x, o_y, o_z)$ in `f64`.
- A 256-entry permutation table $P[0..255]$ of signed bytes (`i8`).

#### Construction from a random source and offset scale $S \in \mathbb{R}$ (`f64`, default $256.0$)
1. Draw three `f64` values in exact order and scale each by $S$:
   - $o_x = \text{nextDouble}() \cdot S$
   - $o_y = \text{nextDouble}() \cdot S$
   - $o_z = \text{nextDouble}() \cdot S$
   Even when $S = 0.0$, all three `nextDouble()` calls are still performed and consume RNG state.
2. Initialize $P[i] = \text{trunc}_{i8}(i)$ for $i = 0, 1, \dots, 255$.
3. Perform a forward Fisher-Yates shuffle for $i = 0, 1, \dots, 255$ **inclusive**:
   - Draw $k = \text{nextInt}(256 - i)$.
   - Swap $P[i]$ and $P[i + k]$.
   Note that the final iteration $i = 255$ calls `nextInt(1)` (always returning $0$, swapping $P[255]$ with itself) and advances the RNG state by one draw.

#### Permutation lookup and gradient table
- **Hash lookup** for any 32-bit integer $n$:
  $$p(n) = \text{zero\_extend}_{i32}(P[n \mathbin{\&} \text{0xFF}] \mathbin{\&} \text{0xFF}) \in [0, 255]$$
- **Gradient table** $G[0..15]$ of integer triples $(g_x, g_y, g_z)$:
  - Entries $0..11$ are the 12 cube-edge vectors; entries $12..15$ repeat four of them so indices $0..15$ can be masked with $\mathbin{\&} 15$ (see **Constants and IDs**).
- **Dot product** for gradient $G[m] = (g_x, g_y, g_z)$:
  - In `f32`: $((g_x \cdot x) + (g_y \cdot y)) + (g_z \cdot z)$ (each integer component cast to `f32`).
  - In `f64`: $((g_x \cdot x) + (g_y \cdot y)) + (g_z \cdot z)$ (each integer component cast to `f64`).
  - Split XZ dot (used in column-cached volume sampling): $(g_x \cdot x) + (g_z \cdot z)$ in `f32`, with $(g_y \cdot y)$ added afterward. Because every vector in $G[0..15]$ has at least one zero component, the split sum produces the same IEEE-754 `f32` bits as the full 3D dot product for all finite inputs.

#### Coordinate wrapping
Used by Perlin and smeared Perlin (**not** by simplex noise). Let $R = 33{,}554{,}432.0 = 2^{25}$ and $H = \text{nextDown}_{f64}(16{,}777{,}216.0) = 2^{24} - 2^{-29}$ (`0x416FFFFFFFFFFFFF`).
- Under normal operation (farlands debug flag disabled):
  $$\text{wrap}(x) = \begin{cases} x & \text{if } -H \le x < H \\ x - \lfloor x / R + 0.5 \rfloor \cdot R & \text{otherwise} \end{cases}$$
  where $\lfloor \cdot \rfloor$ is `f64` floor (not truncated to `i32`).
- At $x = H$, the condition $x < H$ is false, so $x$ enters the wrapping branch; in `f64`, $H / 2^{25} + 0.5$ rounds to $1.0$, so $\text{wrap}(H) = -16{,}777{,}216.0$. At $x = -H$, $-H \le x$ is true, so $\text{wrap}(-H) = -H$.

#### Shared `f32` interpolation primitives
- **Fade (smoothstep)** for $t \in \text{f32}$:
  $$s(t) = ((t \cdot t) \cdot t) \cdot (t \cdot (t \cdot 6.0_f - 15.0_f) + 10.0_f)$$
- **Fade derivative** for $t \in \text{f32}$:
  $$s'(t) = (((30.0_f \cdot t) \cdot t) \cdot (t - 1.0_f)) \cdot (t - 1.0_f)$$
- **Linear interpolation** in `f32`:
  $$\text{lerp}(\alpha, a, b) = a + \alpha \cdot (b - a)$$
- **Bilinear interpolation** in `f32`:
  $$\text{lerp}_2(\alpha_1, \alpha_2, c_{00}, c_{10}, c_{01}, c_{11}) = \text{lerp}(\alpha_2, \text{lerp}(\alpha_1, c_{00}, c_{10}), \text{lerp}(\alpha_1, c_{01}, c_{11}))$$
- **Trilinear interpolation** in `f32`:
  $$\text{lerp}_3(\alpha_1, \alpha_2, \alpha_3, c_{000}, c_{100}, c_{010}, c_{110}, c_{001}, c_{101}, c_{011}, c_{111}) = \text{lerp}(\alpha_3, \text{lerp}_2(\alpha_1, \alpha_2, c_{000}, c_{100}, c_{010}, c_{110}), \text{lerp}_2(\alpha_1, \alpha_2, c_{001}, c_{101}, c_{011}, c_{111}))$$

---

### 2. Improved Perlin noise (single octave)

- **Construction**: gradient-noise initialization with $S = 256.0$.
- **Range**: $[-2.0_f, 2.0_f]$.
- **Standard deviation constant**: $\sigma_{\text{Perlin}} = 0.2702247831245211$ (`0x3FD14B5CE385AA29`).

#### 3D sampling at $(x_{\text{in}}, y_{\text{in}}, z_{\text{in}}) \in \text{f64}^3$
1. Shift into lattice space in `f64`:
   $$x = \text{wrap}(x_{\text{in}}) + o_x, \quad y = \text{wrap}(y_{\text{in}}) + o_y, \quad z = \text{wrap}(z_{\text{in}}) + o_z$$
2. Split into integer cell origin and `f32` fractional offset:
   $$X = \lfloor x \rfloor_{32}, \quad Y = \lfloor y \rfloor_{32}, \quad Z = \lfloor z \rfloor_{32}$$
   $$x_r = \text{f32}(x - \text{f64}(X)), \quad y_r = \text{f32}(y - \text{f64}(Y)), \quad z_r = \text{f32}(z - \text{f64}(Z))$$
3. Compute the six intermediate permutation hashes (32-bit wrapping additions):
   - $h_{x0} = p(X)$, $h_{x1} = p(X + 1)$
   - $h_{00} = p(h_{x0} + Y)$, $h_{01} = p(h_{x0} + Y + 1)$
   - $h_{10} = p(h_{x1} + Y)$, $h_{11} = p(h_{x1} + Y + 1)$
4. For each of the 8 cell corners $(c_x, c_y, c_z) \in \{0, 1\}^3$, look up gradient $g_{c_x c_y c_z} = G[p(h_{c_x c_y} + Z + c_z) \mathbin{\&} 15]$ and evaluate its `f32` dot product with $(x_r - c_x, y_r - c_y, z_r - c_z)$:
   $$d_{c_x c_y c_z} = g_{c_x c_y c_z} \cdot (x_r - c_x, y_r - c_y, z_r - c_z)$$
   (where $c_i = 0$ passes the coordinate unmodified and $c_i = 1$ subtracts $1.0_f$ in `f32`).
5. Compute fade weights $\alpha_x = s(x_r)$, $\alpha_y = s(y_r)$, $\alpha_z = s(z_r)$ in `f32`.
6. Return $\text{lerp}_3(\alpha_x, \alpha_y, \alpha_z, d_{000}, d_{100}, d_{010}, d_{110}, d_{001}, d_{101}, d_{011}, d_{111})$ in `f32`.

#### 2D sampling at $(x_{\text{in}}, y_{\text{in}}) \in \text{f64}^2$
Evaluates the 3D sampler at $(\text{wrap}(x_{\text{in}}), 0.0, \text{wrap}(y_{\text{in}}))$.
- The second 2D coordinate maps to the **Z** axis, while the Y axis is fixed to $0.0$ (which still receives $+ o_y$).
- `wrap` is invoked before forwarding and again inside the 3D sampler (idempotent on all finite values).

#### Sampling with analytical derivatives
Given $(x_{\text{in}}, y_{\text{in}}, z_{\text{in}})$ and a 3-element `f32` accumulator $(D_x, D_y, D_z)$:
1. Perform steps 1–5 of 3D sampling to obtain the 8 gradients $g_{c_x c_y c_z}$, 8 dot products $d_{c_x c_y c_z}$, and fade weights $(\alpha_x, \alpha_y, \alpha_z)$.
2. Interpolate the three gradient components in `f32`:
   - $u_x = \text{lerp}_3(\alpha_x, \alpha_y, \alpha_z, g_{000,x}, g_{100,x}, g_{010,x}, g_{110,x}, g_{001,x}, g_{101,x}, g_{011,x}, g_{111,x})$
   - $u_y = \text{lerp}_3(\alpha_x, \alpha_y, \alpha_z, g_{000,y}, g_{100,y}, g_{010,y}, g_{110,y}, g_{001,y}, g_{101,y}, g_{011,y}, g_{111,y})$
   - $u_z = \text{lerp}_3(\alpha_x, \alpha_y, \alpha_z, g_{000,z}, g_{100,z}, g_{010,z}, g_{110,z}, g_{001,z}, g_{101,z}, g_{011,z}, g_{111,z})$
3. Interpolate corner differences in `f32` (exact argument order matters):
   - $v_x = \text{lerp}_2(\alpha_y, \alpha_z, d_{100} - d_{000}, d_{110} - d_{010}, d_{101} - d_{001}, d_{111} - d_{011})$
   - $v_y = \text{lerp}_2(\alpha_z, \alpha_x, d_{010} - d_{000}, d_{011} - d_{001}, d_{110} - d_{100}, d_{111} - d_{101})$
   - $v_z = \text{lerp}_2(\alpha_x, \alpha_y, d_{001} - d_{000}, d_{101} - d_{100}, d_{011} - d_{010}, d_{111} - d_{110})$
4. **Add** (accumulate, do not overwrite) into the output array in `f32`:
   - $D_x \leftarrow D_x + (u_x + s'(x_r) \cdot v_x)$
   - $D_y \leftarrow D_y + (u_y + s'(y_r) \cdot v_y)$
   - $D_z \leftarrow D_z + (u_z + s'(z_r) \cdot v_z)$
5. Return the standard 3D noise value $\text{lerp}_3(\alpha_x, \alpha_y, \alpha_z, d_{000}, \dots, d_{111})$.

#### Volume accumulation optimization
Iterates $i_z$ (outer), $i_x$ (middle), $i_y$ (inner), computing $(Z, z_r, \alpha_z)$ once per $i_z$ and $(X, x_r, \alpha_x, h_{x0}, h_{x1})$ once per $(i_z, i_x)$. Within each column, it tracks the previous integer Y cell $Y_{\text{prev}}$ (initialized to $-2^{31}$), recomputing the 8 XZ partial dots and $g_y$ components only when $Y \ne Y_{\text{prev}}$. For each voxel, it evaluates $\text{lerp}_3$ using $d_{c_x c_y c_z, xz} + g_{c_x c_y c_z, y} \cdot (y_r - c_y)$ and adds $\text{amplitude} \cdot \text{value}$ to the buffer in `f32`.

---

### 3. Smeared Perlin noise ("y-fuzz" variant, deprecated)

Parameterized by a single `f64` Y-smear step size $q_y$.

- **Construction**: identical to single-octave Perlin noise (draws 3 `nextDouble() * 256.0` and 256 Fisher-Yates swaps), plus stores $q_y$.
- **Range**: symmetric interval of radius $\text{f32}(|q_y| + 2.0)$.

#### 3D sampling at $(x_{\text{in}}, y_{\text{in}}, z_{\text{in}}) \in \text{f64}^3$
1. Shift into lattice space in `f64`:
   $$x = \text{wrap}(x_{\text{in}}) + o_x, \quad y = \text{wrap}(y_{\text{in}}) + o_y, \quad z = \text{wrap}(z_{\text{in}}) + o_z$$
2. Split into cell origin and offsets, keeping $y_r$ in **`f64`**:
   $$X = \lfloor x \rfloor_{32}, \quad Y = \lfloor y \rfloor_{32}, \quad Z = \lfloor z \rfloor_{32}$$
   $$x_r = \text{f32}(x - \text{f64}(X)), \quad y_r = y - \text{f64}(Y) \text{ (in f64)}, \quad z_r = \text{f32}(z - \text{f64}(Z))$$
3. Compute the Y displacement in `f64` using the **unwrapped, unshifted** input $y_{\text{in}}$:
   - Clamp limit:
     $$L = \begin{cases} y_{\text{in}} & \text{if } y_{\text{in}} \ge 0.0 \text{ and } y_{\text{in}} < y_r \\ y_r & \text{otherwise} \end{cases}$$
   - Let $\varepsilon = \text{f64}(1.0\times 10^{-7}_f)$ (the **`f32`** constant `0x33D6BF95`, exact value $1.0000000116860974 \times 10^{-7}$, widened to `f64`).
   - Quantize with 32-bit integer floor:
     $$\delta_y = \text{f64}\!\left(\left\lfloor \frac{L}{q_y} + \varepsilon \right\rfloor_{32}\right) \cdot q_y$$
   - Displaced fractional Y, narrowed to `f32`:
     $$y_{\text{smear}} = \text{f32}(y_r - \delta_y)$$
4. Evaluate Perlin steps 3–6 with a **split Y coordinate**:
   - The 8 corner dot products use $y_{\text{smear}}$ and $(y_{\text{smear}} - 1.0_f)$.
   - The Y fade weight uses the **undisplaced** coordinate: $\alpha_y = s(\text{f32}(y_r))$.

#### 2D sampling and volume accumulation
- 2D sampling inherits Perlin's delegation to 3D at $(\text{wrap}(x_{\text{in}}), 0.0, \text{wrap}(y_{\text{in}}))$, dispatching to the smeared 3D formula with $y_{\text{in}} = 0.0$.
- Volume accumulation mirrors Perlin's column-cached loop, using $y_{\text{in}} = y(i_y) \cdot s_y$ (before wrapping or adding $o_y$) for the clamp limit $L$, undisplaced $\alpha_y = s(\text{f32}(y_r))$, and displaced $y_{\text{smear}}$ in the per-Y dot product additions.

---

### 4. Simplex noise

Extends the gradient-noise foundation. **Does not wrap coordinates** (`wrap` is never called). All internal arithmetic is `f64`, with only the final scaled result narrowed to `f32`.

- **Construction**: gradient-noise initialization with $S = 256.0$ by default, or $S = 0.0$ when constructed with origin offsets discarded. (In zero-offset mode, the three `nextDouble()` calls are still consumed and multiplied by $0.0$, yielding $o_x = o_y = o_z = 0.0$.)
- **Range**: $[-2.0_f, 2.0_f]$.
- **Standard deviation constant**: $\sigma_{\text{Simplex}} = 0.42544$ (`0x3FDB3A68B19A415F`).

#### Shared corner contribution
Given gradient index $m \in \{0, \dots, 11\}$, displacement $(x, y, z) \in \text{f64}^3$, and falloff base $B \in \{0.5, 0.6\}$:
1. Compute in `f64` left-to-right:
   $$t = ((B - x \cdot x) - y \cdot y) - z \cdot z$$
2. If $t < 0.0$, the contribution is $0.0$.
3. Otherwise let $t_2 = t \cdot t$, and return $(t_2 \cdot t_2) \cdot (G[m] \cdot (x, y, z))$ in `f64`.

#### 2D sampling at $(x_{\text{in}}, y_{\text{in}}) \in \text{f64}^2$
Let $\sqrt{3} = \text{sqrt}_{f64}(3.0)$, $F_2 = 0.5 \cdot (\sqrt{3} - 1.0)$, and $G_2 = (3.0 - \sqrt{3}) / 6.0$.
1. Apply offsets: $x = x_{\text{in}} + o_x$, $y = y_{\text{in}} + o_y$.
2. Skew to simplex cell origin:
   $$s = (x + y) \cdot F_2, \quad i = \lfloor x + s \rfloor_{32}, \quad j = \lfloor y + s \rfloor_{32}$$
3. Unskew back to input space (note that $i +_{32} j$ is **32-bit wrapping integer addition** before widening to `f64`):
   $$t = \text{f64}(i +_{32} j) \cdot G_2, \quad X_0 = \text{f64}(i) - t, \quad Y_0 = \text{f64}(j) - t$$
   $$x_0 = x - X_0, \quad y_0 = y - Y_0$$
4. Choose middle triangle vertex:
   - If $x_0 > y_0$: $(i_1, j_1) = (1, 0)$
   - Else ($x_0 \le y_0$): $(i_1, j_1) = (0, 1)$
5. Compute remaining corner displacements (left-to-right):
   - $x_1 = (x_0 - i_1) + G_2$, $y_1 = (y_0 - j_1) + G_2$
   - $x_2 = (x_0 - 1.0) + (2.0 \cdot G_2)$, $y_2 = (y_0 - 1.0) + (2.0 \cdot G_2)$
6. Hash the three corners modulo **12** (not `& 15`), with $i_m = i \mathbin{\&} \text{0xFF}$, $j_m = j \mathbin{\&} \text{0xFF}$:
   - $g_0 = p(i_m + p(j_m)) \bmod 12$
   - $g_1 = p(i_m + i_1 + p(j_m + j_1)) \bmod 12$
   - $g_2 = p(i_m + 1 + p(j_m + 1)) \bmod 12$
7. Evaluate corner contributions $n_0, n_1, n_2$ with $z = 0.0$ and $B = 0.5$.
8. Return $\text{f32}(70.0 \cdot ((n_0 + n_1) + n_2))$.

#### 3D sampling at $(x_{\text{in}}, y_{\text{in}}, z_{\text{in}}) \in \text{f64}^3$
Let $F_3 = 1.0 / 3.0 = 0.3333333333333333$ and $G_3 = 1.0 / 6.0 = 0.16666666666666666$.
1. Apply offsets: $x = x_{\text{in}} + o_x$, $y = y_{\text{in}} + o_y$, $z = z_{\text{in}} + o_z$.
2. Skew to simplex cell origin:
   $$s = ((x + y) + z) \cdot F_3, \quad i = \lfloor x + s \rfloor_{32}, \quad j = \lfloor y + s \rfloor_{32}, \quad k = \lfloor z + s \rfloor_{32}$$
3. Unskew (note $(i +_{32} j) +_{32} k$ is 32-bit wrapping integer addition):
   $$t = \text{f64}(i +_{32} j +_{32} k) \cdot G_3$$
   $$x_0 = x - (\text{f64}(i) - t), \quad y_0 = y - (\text{f64}(j) - t), \quad z_0 = z - (\text{f64}(k) - t)$$
4. Select the tetrahedron traversal $(i_1, j_1, k_1)$ and $(i_2, j_2, k_2)$ with exact tie-breaking:

   | Condition | $(i_1, j_1, k_1)$ | $(i_2, j_2, k_2)$ |
   |---|---|---|
   | $x_0 \ge y_0$ and $y_0 \ge z_0$ | $(1, 0, 0)$ | $(1, 1, 0)$ |
   | $x_0 \ge y_0$ and $y_0 < z_0$ and $x_0 \ge z_0$ | $(1, 0, 0)$ | $(1, 0, 1)$ |
   | $x_0 \ge y_0$ and $y_0 < z_0$ and $x_0 < z_0$ | $(0, 0, 1)$ | $(1, 0, 1)$ |
   | $x_0 < y_0$ and $y_0 < z_0$ | $(0, 0, 1)$ | $(0, 1, 1)$ |
   | $x_0 < y_0$ and $y_0 \ge z_0$ and $x_0 < z_0$ | $(0, 1, 0)$ | $(0, 1, 1)$ |
   | $x_0 < y_0$ and $y_0 \ge z_0$ and $x_0 \ge z_0$ | $(0, 1, 0)$ | $(1, 1, 0)$ |

5. Compute remaining corner displacements (exact left-to-right order):
   - $x_1 = (x_0 - i_1) + G_3$, $y_1 = (y_0 - j_1) + G_3$, $z_1 = (z_0 - k_1) + G_3$
   - $x_2 = (x_0 - i_2) + F_3$, $y_2 = (y_0 - j_2) + F_3$, $z_2 = (z_0 - k_2) + F_3$
   - $x_3 = (x_0 - 1.0) + 0.5$, $y_3 = (y_0 - 1.0) + 0.5$, $z_3 = (z_0 - 1.0) + 0.5$
   (Note: $(x_0 - 1.0) + 0.5$ is evaluated in two steps, not simplified to $x_0 - 0.5$.)
6. Hash the four corners modulo **12**, with $i_m = i \mathbin{\&} \text{0xFF}$, $j_m = j \mathbin{\&} \text{0xFF}$, $k_m = k \mathbin{\&} \text{0xFF}$:
   - $g_0 = p(i_m + p(j_m + p(k_m))) \bmod 12$
   - $g_1 = p(i_m + i_1 + p(j_m + j_1 + p(k_m + k_1))) \bmod 12$
   - $g_2 = p(i_m + i_2 + p(j_m + j_2 + p(k_m + k_2))) \bmod 12$
   - $g_3 = p(i_m + 1 + p(j_m + 1 + p(k_m + 1))) \bmod 12$
7. Evaluate corner contributions $n_0, n_1, n_2, n_3$ with $B = 0.6$.
8. Return $\text{f32}(32.0 \cdot (((n_0 + n_1) + n_2) + n_3))$.

---

### 5. Octave stack

An octave stack stores an ordered list of layers $L_0, L_1, \dots, L_{m-1}$, where each layer holds:
- a child noise generator $N_k$,
- a `f64` frequency multiplier $\phi_k$,
- a `f32` amplitude weight $a_k$.

#### Stack composition
- Adding a single layer appends $(N, \phi, a)$.
- Adding an existing sub-stack with outer frequency $\Phi \in \text{f64}$ and outer amplitude $A \in \text{f32}$ **flattens** every layer $(N_k, \phi_k, a_k)$ of the sub-stack in order as $(N_k, \phi_k \cdot \Phi, a_k \cdot A)$ (multiplying frequency in `f64` and amplitude in `f32`).

#### Range
Start with $I = [0.0_f, 0.0_f]$. For $k = 0, \dots, m - 1$ in order:
$$I \leftarrow I + (\text{range}(N_k) \times [a_k, a_k])$$
using interval multiplication and addition in `f32`.

#### Point sampling
- **3D**: initialize $v = 0.0_f$. For $k = 0, \dots, m - 1$ in order:
  $$v \leftarrow v + a_k \cdot N_k(x \cdot \phi_k, y \cdot \phi_k, z \cdot \phi_k)$$
  All products $a_k \cdot N_k(\dots)$ and additions into $v$ are in **`f32`**.
- **2D**: identical, evaluating $N_k(x \cdot \phi_k, y \cdot \phi_k)$.

#### Volume accumulation
Given outer scales $(s_{xz}, s_y) \in \text{f64}^2$ and outer amplitude $A \in \text{f32}$, iterate $k = 0, \dots, m - 1$ in order and run layer $N_k$'s volume accumulation with scales $(s_{xz} \cdot \phi_k, s_y \cdot \phi_k)$ in `f64` and amplitude $(A \cdot a_k)$ in `f32`.
- **Precision subtlety**: Point evaluation via a scaled density sampler computes $(x_{\text{block}} \cdot s_{xz}) \cdot \phi_k$, whereas volume accumulation computes $x_{\text{block}} \cdot (s_{xz} \cdot \phi_k)$. When neither $s_{xz}$ nor $\phi_k$ is a power of two, these two `f64` products can differ by 1 ULP.

#### How octave stacks are seeded across generators
In 26.3 there is **no** separate "legacy vs xoroshiro" branch inside the octave stack itself; seeding depends on which builder creates the stack:
1. **Modern double-Perlin noises** (both legacy and xoroshiro world RNGs): see Section 7. Always forks two positional random factories and calls `fromHashOf("octave_<k>")` for each active octave exponent $k$.
2. **Legacy Nether biome noises**: see Section 6 and Section 7. Uses sequential creation with 262-draw skips on a legacy LCG source.
3. **Blended terrain noise**: see Section 8. Uses sequential high-to-low smeared-Perlin creation with no skipped draws.
4. **Frozen-ocean biome temperature stack**: see **Constants and IDs** for the fixed-seed 3-layer simplex stack.

---

### 6. Legacy FBM initializer (deprecated)

Used exclusively by the legacy Nether biome instantiation path of double-Perlin noise.

#### Inputs
- A random source $R$,
- First octave exponent $o_0 \in \text{i32}$ (must satisfy $o_0 + n - 1 \le 0$, i.e., no positive octaves),
- Amplitude list $w_0, \dots, w_{n-1} \in \text{f64}$ ($n$ octaves).

Let $z = -o_0$ be the index corresponding to octave $0$. Allocate $n$ empty octave slots $O[0..n-1]$.

#### Ordered RNG consumption and octave creation
1. **Unconditionally** construct a single-octave Perlin generator $P_0$ from $R$ (consuming 3 `nextDouble()` + 256 `nextInt()` draws).
2. If $0 \le z < n$ and $w_z \ne 0.0$, place $P_0$ in $O[z]$; otherwise **discard** $P_0$ (its RNG draws remain consumed).
3. Loop $i$ **downward** from $z - 1$ down to $0$ inclusive:
   - If $i < n$ and $w_i \ne 0.0$: construct a Perlin octave from $R$ and store it in $O[i]$.
   - Otherwise (either $i \ge n$ or $w_i = 0.0$): skip an octave by calling $R.\text{consumeCount}(262)$.
     - On a legacy 48-bit LCG source, `consumeCount(262)` advances the LCG by 262 `nextInt()` calls (matching the nominal $3 \times 2 + 256 = 262$ LCG steps of constructing one Perlin octave without rejection retries).
     - On xoroshiro128++, `consumeCount(262)` advances by 262 `nextLong()` steps.
4. Validate:
   - The count of populated slots in $O$ must equal the count of non-zero entries in $w$ (error otherwise).
   - If $z < n - 1$ (i.e., $o_0 + n - 1 > 0$), fail with an error (positive octaves are rejected).

#### Octave stack assembly
1. Initialize in `f64`:
   $$\phi = 2.0^{-z} = 2.0^{o_0}, \qquad \nu = \frac{2.0^{n - 1}}{2.0^{n} - 1.0}$$
2. Loop $i$ **upward** from $0$ to $n - 1$:
   - If $O[i]$ is populated, append layer $(O[i], \phi, \text{f32}(\nu \cdot w_i))$ to the octave stack.
   - Always update $\phi \leftarrow \phi \cdot 2.0$ and $\nu \leftarrow \nu / 2.0$ (even if $O[i]$ is empty).
3. Build and return the octave stack.

Notice the asymmetry: octaves are **seeded** from highest exponent ($0$, then downward to $o_0$), whereas the stack **accumulates** them from lowest exponent ($o_0$, upward to $o_0 + n - 1$).

---

### 7. Double-Perlin (normal) noise

In 26.3, a normal-noise registry entry (`minecraft:worldgen/noise`) is an immutable parameter object that precomputes octave metadata, normalization factor, and conservative range, and instantiates an octave stack from a random source.

#### JSON fields and validation (`minecraft:worldgen/noise`)

| JSON field | Type / valid range | Default | Notes |
|---|---|---|---|
| `base_amplitude` | `f64` in $[\text{f64}(1.0\times 10^{-5}_f),\; 1{,}000{,}000.0]$ | `1.0` | Lower bound is the `f32` literal `1.0E-5F` (`0x3727C5AC`) widened to `f64` (`9.999999747378752e-6`) |
| `base_octave` | `i32` in $[-32, 32]$ | *(required)* | Exponent of the lowest octave |
| `octave_count` | `i32` in $[1, 32]$ | `1` | Total number of octaves $n$ |
| `normalize` | `true` (enabled), `false` (disabled), or `"legacy"` (legacy) | `true` | Three-mode normalization setting |
| `amplitude_modifiers` | list of $0..32$ `f64` in $[0.0, 1{,}000{,}000.0]$ | `[]` | If non-empty, length must equal `octave_count`; if empty, every octave modifier is $1.0$ |

#### Precomputation of active octaves, normalization factor, and range
Given `base_amplitude` $A_0$, `base_octave` $o_0$, `octave_count` $n$, `normalize` mode, and `amplitude_modifiers` $M$:
1. **Build active octaves** in `f64`:
   - $\phi = 2.0^{o_0}$, $a = A_0$.
   - If `normalize` is enabled or `"legacy"` (i.e., not disabled):
     $$a \leftarrow a \cdot \frac{0.5^{-(n - 1)}}{0.5^{-n} - 1.0}$$
   - For $i = 0, \dots, n - 1$:
     - $m_i = 1.0$ if $M$ is empty, else $M[i]$.
     - If $m_i \ne 0.0$, append active octave record $(k_i = o_0 + i,\; \phi,\; a_i = a \cdot m_i)$.
     - Update $\phi \leftarrow \phi \cdot 2.0$ and $a \leftarrow a \cdot 0.5$.
2. **Target amplitude** in `f64`:
   $$T = \sum_{o \in \text{active}} |o.a|$$
   (left-to-right sum in ascending octave order; $0.0$ if no active octaves).
3. **Standard normalization factor** $N_{\text{std}}$ in `f64`:
   - Sum per-octave variances left-to-right:
     $$V = \sum_{o \in \text{active}} (\sigma_{\text{Perlin}} \cdot |o.a|)^2, \qquad \sigma_{\text{Perlin}} = 0.2702247831245211$$
   - Let $d_{\text{in}} = \sqrt{V}$.
   - If $d_{\text{in}} = 0.0$, then $N_{\text{std}} = 0.0$.
   - Otherwise:
     $$N_{\text{std}} = \frac{T \cdot (1.0 / 3.0)}{d_{\text{in}} \cdot \sqrt{2.0}}$$
     where $1.0 / 3.0 = 0.3333333333333333$ (`0x3FD5555555555555`).
   - Note that `normalize: false` only skips the $0.5^{-(n-1)}/(0.5^{-n}-1)$ scaling in step 1; it **still** computes and applies $N_{\text{std}}$.
4. **Legacy normalization override** (only when `normalize` is `"legacy"` and $N_{\text{std}} \ne 0.0$):
   - Find the minimum and maximum indices $i_{\min}, i_{\max} \in [0, n - 1]$ with $m_i \ne 0.0$, and let $\text{span} = i_{\max} - i_{\min}$.
   - Legacy expected deviation:
     $$E(\text{span}) = 0.1 \cdot \left(1.0 + \frac{1.0}{\text{f64}(\text{span} + 1)}\right)$$
   - Legacy normalization factor (left-to-right):
     $$N_{\text{leg}} = \frac{(A_0 \cdot 0.5) \cdot 0.3333333333333333}{E(\text{span})}$$
   - Update $T \leftarrow T \cdot (N_{\text{leg}} / N_{\text{std}})$ and set final normalization factor $N = N_{\text{leg}}$.
   - For enabled and disabled modes, set $N = N_{\text{std}}$.
5. **Conservative range**:
   $$\text{range} = \left[-r,\; r\right], \qquad r = \text{f32}\!\left((T \cdot 0.3333333333333333) \cdot 6.0\right)$$
   (evaluated left-to-right in `f64`, then cast to `f32`).

#### Parity constructor (used for built-in worldgen noise definitions)
All built-in vanilla noises defined by `(firstOctave, amplitudes[0..n-1])` (with $n \ge 1$) are constructed with `normalize = true`, `base_octave = firstOctave`, `octave_count = n`, `amplitude_modifiers` left empty if every entry equals $1.0$ (otherwise populated with the entries), and:
- Run steps 1–3 above with temporary $A_0 = 1.0$ and `normalize = true` to obtain $N_{\text{new}}$.
- If $N_{\text{new}} = 0.0$, set `base_amplitude` $= 1.0$.
- Otherwise compute $N_{\text{old}}$ via the legacy formula with $A_0 = 1.0$ and set `base_amplitude` $= N_{\text{old}} / N_{\text{new}}$.
- Then construct the normal-noise specification using that `base_amplitude` (repeating steps 1–5; rounding in $N_{\text{old}} / N_{\text{new}}$ is preserved).

#### Standard instantiation from a random source
In worldgen, the noise's random source $R$ is derived from the world's root positional factory via `fromHashOf("<namespace>:<path>")` (e.g. `"minecraft:temperature"`, `"minecraft:offset"`). Then:
1. Fork two positional random factories from $R$ in exact order:
   - $F_1$ = first positional factory forked from $R$,
   - $F_2$ = second positional factory forked from $R$.
2. Initialize an empty octave stack.
3. For each active octave $(k, \phi, a)$ in ascending $k$ order (omitting any octave whose modifier was $0.0$):
   - Form the ASCII seed name `"octave_" + decimal(k)` (e.g. `"octave_-10"`, `"octave_0"`, `"octave_3"`).
   - Construct Perlin octave $P_1$ from $F_1.\text{fromHashOf}(\text{seedName})$.
   - Construct Perlin octave $P_2$ from $F_2.\text{fromHashOf}(\text{seedName})$.
   - Layer amplitude: $w = \text{f32}(N \cdot a)$ (product in `f64`, cast to `f32`).
   - Append layer $(P_1,\; \phi,\; w)$.
   - Append layer $(P_2,\; \phi \cdot \kappa,\; w)$, where $\kappa = 337.0 / 331.0 = 1.0181268882175227$ (`0x3FF04A3F6B8128FE`).
4. Build and return the octave stack.

Key properties:
- Layers are **interleaved** ($P_{1,0}, P_{2,0}, P_{1,1}, P_{2,1}, \dots$), affecting `f32` summation order.
- Both legacy and xoroshiro world RNGs follow this identical procedure; they differ only in how positional factories and `fromHashOf(String)` behave (see `docs/specs/math/random.md`).

#### Legacy Nether biome instantiation (deprecated)
Used by worldgen exclusively for `minecraft:nether/temperature` (seeded with legacy LCG `worldSeed + 0`) and `minecraft:nether/vegetation` (seeded with legacy LCG `worldSeed + 1`), regardless of the dimension's normal RNG algorithm:
1. If `amplitude_modifiers` is empty, construct a list of `octave_count` copies of $1.0$; otherwise use `amplitude_modifiers`.
2. Build stack $S_1$ using the **Legacy FBM initializer** with `(R, base_octave, amplitudes)`.
3. Build stack $S_2$ using the **Legacy FBM initializer** with `(R, base_octave, amplitudes)` on the **same** random source $R$ (continuing from where $S_1$ stopped).
4. Compute outer scale $v = \text{f32}(N \cdot A_0)$ in `f64`, narrowed to `f32`.
5. Build a combined octave stack by flattening $S_1$ with `(frequency = 1.0, amplitude = v)` followed by flattening $S_2$ with `(frequency = 1.0181268882175227, amplitude = v)`.
- Unlike standard instantiation, the layers are **non-interleaved** (all of $S_1$, then all of $S_2$), and each layer's amplitude undergoes **two `f32` roundings** (once inside the legacy FBM initializer, then multiplied by $v$ in `f32` during flattening).

---

### 8. Blended noise (`minecraft:old_blended_noise`)

A density function parameterized by five `f64` fields:

| JSON field | Valid range |
|---|---|
| `xz_scale` ($s_{xz}$) | $[0.001, 1000.0]$ |
| `y_scale` ($s_y$) | $[0.001, 1000.0]$ |
| `xz_factor` ($f_{xz}$) | $[0.001, 1000.0]$ |
| `y_factor` ($f_y$) | $[0.001, 1000.0]$ |
| `smear_scale_multiplier` ($m_s$) | $[1.0, 8.0]$ |

#### Seeding in worldgen
- Seed identifier: `"minecraft:terrain"`.
- If the dimension uses the legacy random source, it instantiates a legacy LCG seeded with `worldSeed` directly; otherwise it derives the RNG from the world positional factory via `fromHashOf("minecraft:terrain")`.

#### Sub-noise stack construction
Let $B = 684.412$, $M_{xz} = B \cdot s_{xz}$, $M_y = B \cdot s_y$, $Y_{\text{lim}} = M_y \cdot m_s$, $Y_{\text{main}} = Y_{\text{lim}} / f_y$.
Let $C_{\text{lim}} = \text{f64}(0.99998474_f) = 65535.0 / 65536.0 = 0.9999847412109375$ (`0x3F7FFF00` widened to `f64`), and $C_{\text{main}} = 12.75$.

Define the smeared-FBM builder for `(random, firstOctave, smearScaleY, valueFactor)` (requires $\text{firstOctave} \le 0$):
1. Let $n = -\text{firstOctave} + 1$.
2. Initialize $\phi = 1.0$ and $\nu = \text{valueFactor} / (2.0^n - 1.0)$ in `f64`.
   - For limit noises ($n = 16$, $2^{16}-1 = 65535$): $\nu$ starts at exact $2^{-16} = 1 / 65536$.
   - For main noise ($n = 8$, $2^8 - 1 = 255$): $\nu$ starts at $12.75 / 255.0 = 0.05$.
3. Repeat $n$ times (highest frequency down to lowest frequency):
   - Construct a **Smeared Perlin noise** from `random` with Y-smear step size $q_y = \text{smearScaleY} \cdot \phi$.
   - Append layer `(noise, frequency = \phi, amplitude = \text{f32}(\nu))` to the octave stack.
   - Update $\phi \leftarrow \phi / 2.0$ and $\nu \leftarrow \nu \cdot 2.0$.
4. Build and return the octave stack.

Construct three stacks sequentially from the **same** random source in exact order:
1. Lower-limit stack: smeared FBM with $\text{firstOctave} = -15$ (16 octaves), $\text{smearScaleY} = Y_{\text{lim}}$, $\text{valueFactor} = C_{\text{lim}}$.
2. Upper-limit stack: smeared FBM with $\text{firstOctave} = -15$ (16 octaves), $\text{smearScaleY} = Y_{\text{lim}}$, $\text{valueFactor} = C_{\text{lim}}$.
3. Main selector stack: smeared FBM with $\text{firstOctave} = -7$ (8 octaves), $\text{smearScaleY} = Y_{\text{main}}$, $\text{valueFactor} = C_{\text{main}}$.

#### Sampling at block coordinate $(b_x, b_y, b_z) \in \text{i32}^3$
All combining arithmetic is in `f32`; coordinate scaling is in `f64`:
1. Evaluate the main selector stack at $(b_x \cdot (M_{xz} / f_{xz}),\; b_y \cdot (M_y / f_y),\; b_z \cdot (M_{xz} / f_{xz}))$ to get $m \in \text{f32}$.
2. Compute selector $\alpha = \text{clamp}_{f32}(m + 0.5_f,\; 0.0_f,\; 1.0_f)$ (where $\text{clamp}(v, 0, 1)$ is $0.0_f$ if $v < 0.0_f$, else $\min(v, 1.0_f)$).
3. Evaluate blended result with **exact endpoint short-circuiting**:
   - If $\alpha = 0.0_f$: return the lower-limit stack sampled at $(b_x \cdot M_{xz}, b_y \cdot M_y, b_z \cdot M_{xz})$.
   - If $\alpha = 1.0_f$: return the upper-limit stack sampled at $(b_x \cdot M_{xz}, b_y \cdot M_y, b_z \cdot M_{xz})$.
   - Otherwise: evaluate both $a = \text{lower-limit}(\dots)$ and $b = \text{upper-limit}(\dots)$ and return $\text{lerp}(\alpha, a, b) = a + \alpha \cdot (b - a)$ in `f32`.
   (Volume evaluation fills buffers for $\alpha$, lower-limit, and upper-limit via volume accumulation with outer amplitude $1.0_f$, then applies the exact same $\alpha = 0.0_f$ / $\alpha = 1.0_f$ / $\text{lerp}$ selection per voxel.)

#### Range
Computed by running the same 16-iteration high-to-low loop as the lower-limit stack ($\text{firstOctave} = -15$, $\text{smearScaleY} = Y_{\text{lim}}$, $\text{valueFactor} = C_{\text{lim}}$) and accumulating $[0.0_f, 0.0_f] + \sum (\text{range}_{\text{smear}}(\text{smearScaleY} \cdot \phi) \times [\text{f32}(\nu), \text{f32}(\nu)])$ in `f32` interval arithmetic.

---

### 9. Noise utility helpers (`NoiseUtils`)

1. **Extreme-bias remap** for $(x, k) \in \text{f64}^2$:
   $$\text{bias}(x, k) = x + \frac{\sin(\pi \cdot x) \cdot k}{\pi}$$
   Exact `f64` evaluation order: `x + ((sin(PI * x) * k) / PI)` with $\pi = \text{0x400921FB54442D18}$.
   - Uses JVM `Math.sin` (not `StrictMath.sin`); like `Math.log` in `random.md`, platform intrinsics may differ from fdlibm by up to 1 ULP on some CPUs.
2. **Parity octave config formatting**:
   Given offsets $(o_x, o_y, o_z) \in \text{f64}^3$ and permutation array $P$ (`i8[256]` or `i32[256]`):
   Formats in US/root locale as:
   `xo=%.3f, yo=%.3f, zo=%.3f, p0=%d, p255=%d`
   with $o_x, o_y, o_z$ **cast to `f32`** before `%.3f` formatting, and $P[0], P[255]$ formatted as **signed** integers (`-128..127` when backed by the `i8` permutation table).

---

## Edge cases and known quirks

Every item below can cause bit-level divergence if implemented naively (all to be verified by oracle suite `noise-samples`):

1. **Final Fisher-Yates draw `nextInt(1)`**: Gradient-noise initialization loops $i = 0 \dots 255$ inclusive, calling `nextInt(1)` at $i = 255$. Skipping the no-op last swap desynchronizes every subsequent noise initialized from the same random source (critical in blended noise, the legacy FBM initializer, and biome simplex stacks).
2. **Three `nextDouble()` draws even when offset scale is `0.0`**: Constructing simplex noise with discarded offsets ($S = 0.0$) still calls `nextDouble()` three times and multiplies each result by `0.0`. Omitting those calls desynchronizes the permutation shuffle.
3. **Asymmetric `wrap` boundary at `nextDown(2^24)`**: The pass-through range is $[-H, H)$ with $H = \text{nextDown}_{f64}(16{,}777{,}216.0)$ (`0x416FFFFFFFFFFFFF`), not $\pm 2^{24}$. At $x = +H$, `wrap(x)` enters the modular branch and produces $-16{,}777{,}216.0$; at $x = -H$, `wrap(x)` returns $-H$ unchanged.
4. **Simplex noise does NOT wrap coordinates**: Unlike Perlin and smeared Perlin, simplex noise passes raw `f64` coordinates directly to $+ o$ and skewing.
5. **Simplex 32-bit integer overflow in unskew sum**: In simplex 2D and 3D, $(i + j)$ and $(i + j + k)$ are summed in **32-bit wrapping `i32`** before converting to `f64` and multiplying by $G_2$ or $G_3$. Large coordinates near $\pm 2^{30}$ wrap differently than `f64` or `i64` addition.
6. **Simplex 3D fourth-corner offset order**: $(x_0 - 1.0) + 0.5$ is evaluated in two `f64` steps, not simplified to $x_0 - 0.5$; near $x_0 = 0$ the intermediate rounding differs.
7. **Simplex modulo 12 vs Perlin mask `& 15`**: Simplex hashes reduce $p(\dots) \bmod 12$ (using only the first 12 cube-edge gradients), whereas Perlin masks $p(\dots) \mathbin{\&} 15$ (including the 4 duplicated gradients at indices 12..15).
8. **2D Perlin samples the XZ plane, not XY**: 2D Perlin delegates to 3D at $(\text{wrap}(x), 0.0, \text{wrap}(y))$, so the second coordinate is Z and $o_y$ still shifts the slice away from $y = 0$.
9. **`f64` lattice + `f32` fractional/interpolation split in Perlin**: Wrapping, adding offsets, and subtracting floor happen in `f64`; the resulting fractional coordinates $(x_r, y_r, z_r)$ are narrowed to `f32`, and all dot products, smoothstep, trilinear lerp, derivatives, and octave-stack accumulations happen in `f32`. Note that $\text{f32}(x - \lfloor x \rfloor)$ can round up to $1.0_f$ when $x - \lfloor x \rfloor \in [1 - 2^{-25}, 1)$.
10. **Smeared Perlin uses the UNWRAPPED input `y` for its clamp limit**: The test $0.0 \le y_{\text{in}} < y_r$ inspects the raw input $y_{\text{in}}$ before `wrap` and before $+ o_y$.
11. **Smeared Perlin `1.0E-7F` epsilon is `f32` widened to `f64`**: Using `f64` `1e-7` instead of `0x33D6BF95` widened to `f64` ($1.0000000116860974 \times 10^{-7}$) shifts the quantization boundary near integer multiples of $q_y$.
12. **Smeared Perlin quantizes with 32-bit saturating floor and splits fade vs dot Y**: $\lfloor L / q_y + \varepsilon \rfloor_{32}$ saturates to `i32`, and only the dot products use the displaced $y_{\text{smear}}$ while smoothstep uses the **undisplaced** $\text{f32}(y_r)$.
13. **Octave accumulation order in `f32`**: Because octave stacks accumulate in `f32`, layer ordering changes rounding:
    - Standard double-Perlin instantiation **interleaves** `[first_0, second_0, first_1, second_1, ...]` from lowest to highest frequency.
    - Legacy Nether double-Perlin instantiation is **non-interleaved** (`[all first_i, all second_i]`) from lowest to highest frequency.
    - Blended noise accumulates **highest frequency (smallest amplitude) to lowest frequency (largest amplitude)**.
14. **Volume vs point multiplication associativity**: Point evaluation computes `(blockCoord * scale) * layerFreq`, whereas volume accumulation computes `blockCoord * (scale * layerFreq)`. When neither factor is a power of two (e.g., double-Perlin's second octave with $\kappa = 337/331$ combined with a non-dyadic `xz_scale`), point and volume paths can differ by 1 ULP.
15. **Perlin column-cache `-2^31` sentinel**: In column-cached volume sampling, the previous-cell sentinel is $-2^{31}$; if the first sampled voxel had $Y = -2^{31}$ (only reachable if farlands wrapping bypass is enabled), gradient initialization would be skipped.
16. **Legacy FBM unconditional octave-0 draw and `consumeCount(262)`**: The legacy FBM initializer always builds octave 0 first (discarding it if out of bounds or zero amplitude), then walks downward calling `consumeCount(262)` for every out-of-bounds or zero-amplitude slot. For Nether noises (`base_octave = -7`, 2 octaves), it discards octave 0 and skips 5 octaves ($5 \times 262 = 1310$ LCG draws) before building octaves $-6$ and $-7$.
17. **Double-Perlin `normalize: false` still applies standard-deviation normalization**: Setting `normalize` to `false` only omits the $0.5^{-(n-1)} / (0.5^{-n} - 1)$ amplitude scaling; it still divides by $\sqrt{2} \cdot \sigma$ and multiplies by $T / 3$.
18. **Double-Perlin parity constructor double-pass rounding and `base_amplitude` lower bound**: Built-in noises compute `base_amplitude` $= N_{\text{old}} / N_{\text{new}}$ and re-run normalization, preserving rounding error; JSON validation uses $\text{f64}(1.0\times 10^{-5}_f)$ (`9.999999747378752e-6`), not `1e-5`, as the minimum `base_amplitude`.
19. **Legacy Nether double `f32` rounding**: Legacy Nether double-Perlin instantiation narrows per-octave amplitudes to `f32` inside the legacy FBM initializer, narrows $N \cdot A_0$ to `f32`, and multiplies those two `f32` values when flattening the stack.
20. **Blended noise limit factor is a `f32` literal**: $0.99998474_f$ widened to `f64` equals exact $65535 / 65536 = 0.9999847412109375$ (so dividing by $2^{16}-1 = 65535$ yields exact $2^{-16}$); using `0.99998474` as a `f64` literal produces the wrong weights.
21. **Blended noise exact `0.0_f` / `1.0_f` lerp short-circuit**: When $\alpha = 0.0_f$ or $\alpha = 1.0_f$, the output is the lower-limit or upper-limit sample directly (not $a + 1.0_f \cdot (b - a)$, which can differ from $b$ by 1 ULP in `f32` when $|a| \gg |b|$).
22. **Perlin analytical derivative accumulates (`+=`) and orders the Y cross-term as `(az, ax)`**: It adds into the 3-element output array rather than overwriting, and evaluates the Y difference term as $\text{lerp}_2(\alpha_z, \alpha_x, \dots)$ (Z inner, X outer).
23. **`NoiseUtils` parity string casts offsets to `f32` and prints signed `i8` permutations**: `xo`/`yo`/`zo` narrow to `f32` before `%.3f`, and `p0`/`p255` print signed `-128..127` for byte tables.
24. **JVM `Math.sin` in `NoiseUtils` extreme-bias helper**: May differ by up to 1 ULP from `StrictMath.sin` / fdlibm depending on CPU/JVM intrinsic; match the host platform's oracle samples.

---

## Constants and IDs

### 16-entry gradient table $G[0..15]$

| Index | $(g_x, g_y, g_z)$ | Index | $(g_x, g_y, g_z)$ |
|---|---|---|---|
| 0 | $(+1, +1, 0)$ | 8 | $(0, +1, +1)$ |
| 1 | $(-1, +1, 0)$ | 9 | $(0, -1, +1)$ |
| 2 | $(+1, -1, 0)$ | 10 | $(0, +1, -1)$ |
| 3 | $(-1, -1, 0)$ | 11 | $(0, -1, -1)$ |
| 4 | $(+1, 0, +1)$ | 12 | $(+1, +1, 0)$ |
| 5 | $(-1, 0, +1)$ | 13 | $(0, -1, +1)$ |
| 6 | $(+1, 0, -1)$ | 14 | $(-1, +1, 0)$ |
| 7 | $(-1, 0, -1)$ | 15 | $(0, -1, -1)$ |

### Exact floating-point constants

| Constant | Type | Decimal / formula | IEEE-754 hex |
|---|---|---|---|
| Coordinate wrap period $R$ | `f64` | $33{,}554{,}432.0 = 2^{25}$ | `0x4180000000000000` |
| Coordinate wrap half-bound $H$ | `f64` | $\text{nextDown}(2^{24}) = 16{,}777{,}215.999999998$ | `0x416FFFFFFFFFFFFF` |
| Default origin offset scale | `f64` | $256.0$ | `0x4070000000000000` |
| Perlin standard deviation | `f64` | $0.2702247831245211$ | `0x3FD14B5CE385AA29` |
| Smeared Perlin shift epsilon | `f32` | $1.0 \times 10^{-7}_f$ (`1.0000000116860974e-7` as `f64`) | `0x33D6BF95` |
| Simplex standard deviation | `f64` | $0.42544$ | `0x3FDB3A68B19A415F` |
| Simplex $\sqrt{3}$ | `f64` | $1.7320508075688772$ | `0x3FFBB67AE8584CAA` |
| Simplex $F_2 = 0.5(\sqrt{3}-1)$ | `f64` | $0.3660254037844386$ | `0x3FD76CF5D0B09954` |
| Simplex $G_2 = (3-\sqrt{3})/6$ | `f64` | $0.21132486540518713$ | `0x3FCB0CB174DF99C8` |
| Simplex $F_3 = 1/3$ | `f64` | $0.3333333333333333$ | `0x3FD5555555555555` |
| Simplex $G_3 = 1/6$ | `f64` | $0.16666666666666666$ | `0x3FC5555555555555` |
| Simplex 2D falloff / scale | `f64` | $0.5$ / $70.0$ | — |
| Simplex 3D falloff / scale | `f64` | $0.6$ / $32.0$ | — |
| Normal noise input factor $\kappa$ | `f64` | $337.0 / 331.0 = 1.0181268882175227$ | `0x3FF04A3F6B8128FE` |
| Normal noise target deviation | `f64` | $1.0 / 3.0 = 0.3333333333333333$ | `0x3FD5555555555555` |
| Normal noise `base_amplitude` min | `f32` | $1.0 \times 10^{-5}_f$ (`9.999999747378752e-6` as `f64`) | `0x3727C5AC` |
| Normal noise max octaves / max amp | `i32` / `f64` | $32$ / $1{,}000{,}000.0$ | — |
| Blended noise base scale $B$ | `f64` | $684.412$ | `0x4085634BC6A7EF9E` |
| Blended noise limit factor | `f32` | $65535 / 65536 = 0.9999847412109375$ | `0x3F7FFF00` |
| Blended noise main factor | `f64` | $12.75$ | `0x4029800000000000` |
| Blended noise first octaves | `i32` | limit: $-15$ (16 octaves), main: $-7$ (8 octaves) | — |
| Legacy FBM skip count | `i32` | $262$ | — |

### Interoperability IDs and fixed seeds
- Density function type ID for blended noise: `minecraft:old_blended_noise`.
- Blended noise RNG hash identifier: `minecraft:terrain`.
- Normal noise octave hash names: `"octave_<k>"` (e.g. `"octave_-7"`, `"octave_0"`).
- Legacy Nether biome noise registry keys: `minecraft:nether/temperature` (seed `worldSeed + 0`), `minecraft:nether/vegetation` (seed `worldSeed + 1`).
- Built-in blended-noise dimension presets (`xz_scale`, `y_scale`, `xz_factor`, `y_factor`, `smear_scale_multiplier`):
  - Overworld: `(0.25, 0.125, 80.0, 160.0, 8.0)`
  - Nether: `(0.25, 0.375, 80.0, 60.0, 8.0)`
  - End: `(0.25, 0.25, 80.0, 160.0, 4.0)`
- Fixed-seed biome simplex noises (all constructed with zero offset scale $S = 0.0$ on a legacy LCG source):
  - Temperature: seed `1234`, single simplex.
  - Biome info: seed `2345`, single simplex.
  - Frozen temperature: seed `3456`, 3-layer octave stack with `(freq=1.0, amp=0.14285715_f)`, `(0.5, 0.2857143_f)`, `(0.25, 0.5714286_f)` (sequentially created from the same RNG).
- End island simplex noise: legacy LCG seeded with `worldSeed`, `consumeCount(17292)`, then single simplex with zero offset scale $S = 0.0$.

---

## Open questions

1. **Per-platform `Math.sin` parity**: Verify via oracle `noise-samples` whether HotSpot's `Math.sin` intrinsic diverges by 1 ULP from fdlibm `sin` on any CI architecture for the `NoiseUtils` extreme-bias helper, and record any per-platform quirk if found.
2. **Non-positive Y-smear step size ($q_y \le 0$) in smeared Perlin**: Vanilla callers only pass strictly positive $q_y$ ($684.412 \cdot s_y \cdot m_s \cdot 2^{-k} > 0$). Confirm whether any data-pack or test exercises $q_y = 0.0$ (where $L / 0.0$ saturates $\lfloor \cdot \rfloor_{32}$ to $2^{31}-1$ or $0$ before multiplying by $0.0$).
3. **`NormalNoise` range queried by downstream density functions (`NormalNoiseParameters::range()` vs `OctaveStack::range()`)**: `NormalNoiseParameters` precomputes `[-r, r]` with $r = \text{f32}((T \cdot \frac{1}{3}) \cdot 6.0)$ (Section 7 step 5), whereas its instantiated `OctaveStack` computes a wider layer-summed interval $\sum ([-2, 2] \times [w_k, w_k])$ (Section 5). Confirm which interval downstream density functions (`minecraft:noise`, `minecraft:shifted_noise`, etc.) report as their min/max bounds; both are exposed (`NormalNoiseParameters::range()` / `NormalNoise::parameter_range()` and `OctaveStack::range()` / `NormalNoise::range()`).
   **Answered (reader, 2026-11-25):** the noise-backed density functions report the precomputed parameter range
   `[-r, r]`, not the layer-summed stack interval. See `worldgen/density-functions.md` §2.1.

## Parity status (2026-11-25)
`noise-samples`: **48,300/48,300** floats bit-identical to vanilla 26.3: Perlin (3D, 2D, value + derivatives),
simplex (2D, 3D), smeared Perlin (3 smear scales), normal noise (4 parity configurations; standard and legacy-Nether
instantiation, including matching rejection of a positive legacy octave), legacy FBM and smeared FBM stacks. Each was
built from both random generators with 5 seeds and sampled at 210 points, including the 2²⁴ wrap boundary and ±0. The
top-level blended-noise sampler (selector + limit lerp) is verified with the density functions in M7.
