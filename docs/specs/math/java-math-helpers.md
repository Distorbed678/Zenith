# Math: Java math, packing, and geometry helpers (26.3)

- Verified by suites: `rng-streams` (helpers), `math-helpers` (new oracle suite)
- Sources: IEEE-754 binary32/binary64 arithmetic, Java SE 25 `java.lang.Math` / `java.lang.Float` / `java.lang.Double` specifications, JOML 1.10.5 `Math.invsqrt`, public MurmurHash3 32-bit finalizer and De Bruijn bit-scan constants, public fast inverse-root Newton-Raphson recurrences, Robert Penner / easings.net formulas, `docs/specs/math/random.md`, `docs/specs/math/noise.md`, and local reference reading (2026-11-24) for 26.3 exact precision, ordering, and quirks. Everything below is written in our own words.

## Summary

Version 26.3 concentrates its scalar math, trigonometric approximations, bit manipulation, color conversion, coordinate packing, and 3D geometry arithmetic across a small family of utility modules:

1. **Lookup-table trigonometry** (`Mth.sin`, `Mth.cos`, `Mth.atan2`): a 65,536-entry `f32` sine table indexed in `f64` and a 257-entry `f64` table-plus-Taylor arctangent built on the fast inverse square root.
2. **Roots and fast inverse roots** (`Mth.sqrt`, `Mth.invSqrt`, `Mth.fastInvSqrt`, `Mth.fastInvCubeRoot`): exact IEEE-754 roots alongside 64-bit and 32-bit bit-level Newton-Raphson approximations.
3. **Rounding, clamping, modular arithmetic, and integer bit-math** (`Mth`, `Divisor`): floor/ceil/frac conversions, float vs integer clamping, Euclidean/ceiling division, power-of-two bit smearing, De Bruijn `log2`/`ceillog2`, lower-bound binary search, and symmetric outward integer stepping.
4. **Interpolation, remapping, splines, and easing** (`Mth`, `Ease`, `EasingType`, `CubicSpline`, `SmoothDouble`, `BinaryAnimator`): linear/bilinear/trilinear/integer/discrete interpolation, quintic smoothstep, Catmull-Rom, triangle wave, 32 named easing curves plus Newton/bisection cubic Bézier solving, and piecewise cubic Hermite splines.
5. **Angle normalization, rotation, and quantization** (`Mth`, `SegmentedAnglePrecision`): half-open `[-180, 180)` degree wrapping, shortest-arc stepping and interpolation in degrees and radians, signed-byte angle packing, configurable $N$-bit binary angles, and quaternion axis projection.
6. **Hashing, mixing, and RNG sampling helpers** (`Mth`, `LinearCongruentialGenerator`, `ChunkPos`, `Vec3i`): MurmurHash3 `fmix32`, quadratic 64-bit LCG mixing, chunk/vector hashes, bounded random draws, coordinate wobble, and version-4 UUID bit-stamping (building on the RNG primitives already specified in `docs/specs/math/random.md`).
7. **Color and light packing** (`ARGB`, `Mth.hsvToRgb`, `Mth.hsvToArgb`, `LightCoordsUtil`): 32-bit ARGB/ABGR packing, 10-bit sRGB–linear lookup tables, integer alpha/linear blending, HSV conversion, and 4-bit/8-bit packed sky/block light coordinates.
8. **64-bit coordinate packing** (`BlockPos`, `SectionPos`, `ChunkPos`, `QuartPos`): signed two's-complement bitfields for block, section, chunk, and quart coordinates.
9. **Vector and bounding-box arithmetic** (`Vec3i`, `BlockPos`, `Vec3`, `Vec2`, `LpVec3`, `AABB`, `Mth.rayIntersectsAABB`): distance, normalization, rotation, compact network vector quantization, slab ray-clipping, SAT ray-AABB intersection, and AABB set/shape operations with observable rounding and boundary quirks.

## Inputs and state

### Numeric and conversion conventions
- **Floating-point arithmetic**: IEEE-754 binary32 (`f32`) and binary64 (`f64`), round-to-nearest-even, **no fused multiply-add** (each multiplication and addition rounds independently). Chains of additions, subtractions, and multiplications associate **left to right** unless parenthesized.
- **Integer arithmetic**: two's-complement 32-bit signed (`i32`), 64-bit signed (`i64`), 16-bit signed (`i16`), and 8-bit signed (`i8`), wrapping on overflow. Arithmetic right shift is written $\gg$; logical (unsigned) right shift is written $\ggg$.
- **Java remainder (`%`)**: for integers, truncated toward zero (result has the sign of the dividend; zero divisor throws). For `f32`/`f64`, IEEE-754 truncated remainder (`frem`/`drem`, **not** round-to-nearest `IEEEremainder`): $a \bmod_{\text{trunc}} b = a - \text{trunc}(a / b) \cdot b$, preserving the sign of $a$ when the remainder is zero, and producing `NaN` if $b = 0$ or $a$ is non-finite.
- **Saturating float-to-integer truncation**, written $\text{trunc}_{i32}(v)$ and $\text{trunc}_{i64}(v)$: finite values truncate toward zero; $\text{NaN} \mapsto 0$; $v \ge T_{\max} \mapsto T_{\max}$; $v \le T_{\min} \mapsto T_{\min}$.
- **Saturating floor and ceil to integer**:
  - $\lfloor v \rfloor_{32} = \text{trunc}_{i32}(\text{floor}_{f64}(\text{f64}(v)))$, $\lfloor v \rfloor_{64} = \text{trunc}_{i64}(\text{floor}_{f64}(v))$.
  - $\lceil v \rceil_{32} = \text{trunc}_{i32}(\text{ceil}_{f64}(\text{f64}(v)))$, $\lceil v \rceil_{64} = \text{trunc}_{i64}(\text{ceil}_{f64}(v))$.
- **Java `Math.round`**:
  - $\text{round}_{i32}(x \in \text{f32})$ and $\text{round}_{i64}(x \in \text{f64})$ round ties toward $+\infty$ (exact $\lfloor x + 0.5 \rfloor$ in real arithmetic, saturating to the target integer type, $\text{NaN} \mapsto 0$; notably $\text{round}(-0.5) = 0$).
- **Java `Math.min` / `Math.max` on floats**: `NaN`-propagating (if either operand is `NaN`, the result is `NaN`) and signed-zero aware ($\min(+0.0, -0.0) = -0.0$, $\max(+0.0, -0.0) = +0.0$).
- **Java `Math.clamp(v, lo, hi)`** (used in `ARGB` and `LpVec3`, distinct from `Mth.clamp`): rejects `lo > hi` or `NaN` bounds; for floats evaluates $\min(\text{hi}, \max(v, \text{lo}))$, propagating `NaN` and ordering $-0.0 < +0.0$.
- **Java `Math.floorDiv(a, b)` / `Math.floorMod(a, b)`**: quotient floored toward $-\infty$ and matching remainder $a - \text{floorDiv}(a, b) \cdot b$ (zero divisor throws; $\text{floorDiv}(T_{\min}, -1) = T_{\min}$ by wrapping).
- **Host transcendental precision**: `Math.sqrt` is IEEE-754 correctly rounded; non-algebraic JVM `Math` functions (`sin`, `cos`, `asin`, `atan2`, `pow`, `log`) may use CPU/HotSpot intrinsics that differ from `StrictMath`/fdlibm by up to 1 ULP on some hardware. Match the reference machine's oracle output and record any platform variance.

### Immutable lookup tables initialized at startup
- `SIN[0..65535]` (`f32[65536]`): sine lookup table in `Mth`.
- `ASIN_TAB[0..256]` and `COS_TAB[0..256]` (`f64[257]` each): arctangent tables in `Mth`.
- `SRGB_TO_LINEAR[0..255]` (`i16[256]`) and `LINEAR_TO_SRGB[0..1023]` (`i8[1024]`, interpreted unsigned): 10-bit color transfer tables in `ARGB`.

## Behavior (ordered rules)

### 1. Lookup-table trigonometry (`Mth`)

#### Sine and cosine table (`Mth.sin`, `Mth.cos`)
Let $N = 65{,}536 = 2^{16}$, mask $M = 65{,}535 = \text{0xFFFF}$, quarter-turn offset $Q = 16{,}384.0$ (`f64`), and scale factor:
$$K = \frac{65{,}536.0}{2 \cdot \pi} = 10{,}430.378350470453 \quad (\text{f64, }\texttt{0x40C45F306DC9C883})$$

1. **Table population**: for each integer $i \in \{0, 1, \dots, 65535\}$, compute in `f64` then narrow to `f32`:
   $$\text{SIN}[i] = \text{f32}\!\left(\sin_{f64}\!\left(\frac{\text{f64}(i)}{K}\right)\right)$$
   Note that the argument is formed by **dividing** by $K$ (not multiplying by $2\pi / 65536$).
2. **Sine lookup** for input $x \in \text{f64}$ (returns `f32`):
   $$\text{sin}(x) = \text{SIN}\!\left[\,\text{trunc}_{i64}(x \cdot K) \mathbin{\&} \text{0xFFFF}\,\right]$$
3. **Cosine lookup** for input $x \in \text{f64}$ (returns `f32`):
   $$\text{cos}(x) = \text{SIN}\!\left[\,\text{trunc}_{i64}(x \cdot K + 16{,}384.0) \mathbin{\&} \text{0xFFFF}\,\right]$$

Both functions accept `f64` in 26.3 and multiply/add in `f64` before **truncating toward zero** to `i64` (not flooring) and masking the low 16 bits.

#### Fast arctangent (`Mth.atan2(y, x)`)
Both inputs and the output are `f64`.

1. **Table population**: for $k \in \{0, 1, \dots, 256\}$, let $v = \text{f64}(k) / 256.0$ and $a = \arcsin_{f64}(v)$; store $\text{ASIN\_TAB}[k] = a$ and $\text{COS\_TAB}[k] = \cos_{f64}(a)$.
2. **Evaluation** for $(y, x) \in \text{f64}^2$:
   1. Compute $d_2 = x \cdot x + y \cdot y$. If $d_2$ is `NaN`, return `NaN`.
   2. Record signs strictly against $0.0$:
      - $\text{negY} = (y < 0.0)$; if true, $y \leftarrow -y$.
      - $\text{negX} = (x < 0.0)$; if true, $x \leftarrow -x$.
      (Negative zero $-0.0$ fails $< 0.0$, so its sign bit is ignored.)
   3. Octant fold: $\text{steep} = (y > x)$; if true, swap $x$ and $y$.
   4. Normalize using **fast inverse square root** (Section 2, not exact $1/\sqrt{d_2}$):
      $$r = \text{fastInvSqrt}(d_2), \qquad x \leftarrow x \cdot r, \qquad y \leftarrow y \cdot r$$
   5. Quantize $y$ to multiples of $2^{-8}$ using the IEEE-754 bias $B = \text{bits\_to\_f64}(\text{0x42B0000000000000}) = 2^{44}$:
      $$y_p = B + y, \qquad \text{idx} = \text{trunc}_{i32}(\text{f64\_to\_raw\_bits}(y_p))$$
      (Takes the low 32 bits of the IEEE-754 representation of $y_p$, which equals $\text{round\_nearest\_even}(256 y)$ for $y \in [0, 1]$.)
   6. Look up $\phi = \text{ASIN\_TAB}[\text{idx}]$ and $c_\phi = \text{COS\_TAB}[\text{idx}]$ (out-of-bounds `idx` outside $[0, 256]$ throws in Java; reachable on $x^2 + y^2 = +\infty$ or extreme subnormal inputs). Reconstruct the quantized sine $s_\phi = y_p - B$.
   7. Compute the residual sine and 3rd-order arcsine correction with $C_{1/6} = 1.0 / 6.0$ (`0x3FC5555555555555`):
      $$s_d = y \cdot c_\phi - x \cdot s_\phi, \qquad d = ((6.0 + s_d \cdot s_d) \cdot s_d) \cdot C_{1/6}, \qquad \theta = \phi + d$$
   8. Unfold in exact order:
      - If $\text{steep}$: $\theta \leftarrow (\pi / 2.0) - \theta$.
      - If $\text{negX}$: $\theta \leftarrow \pi - \theta$.
      - If $\text{negY}$: $\theta \leftarrow -\theta$.
   9. Return $\theta$.

---

### 2. Roots and fast inverse roots (`Mth`)

1. **`Mth.sqrt(x: f32) -> f32`**: widens $x$ to `f64`, computes exact $\sqrt{\text{f64}(x)}$, narrows to `f32`.
2. **`Mth.invSqrt(x: f32) -> f32`** (delegates to JOML `invsqrt(float)`):
   $$\text{invSqrt}_{f32}(x) = \frac{1.0_f}{\text{f32}\!\left(\sqrt{\text{f64}(x)}\right)}$$
   The square root is narrowed to `f32` **before** dividing `1.0f` in `f32` (not `f32(1.0 / sqrt(f64(x)))`).
3. **`Mth.invSqrt(x: f64) -> f64`** (delegates to JOML `invsqrt(double)`): $1.0 / \sqrt{x}$ in `f64`.
4. **`Mth.fastInvSqrt(x: f64) -> f64`** (64-bit bit-hack + one Newton-Raphson step):
   1. $h = 0.5 \cdot x$ (`f64`).
   2. $i = \text{f64\_to\_raw\_bits}(x)$ (`i64`; preserves raw `NaN` payloads without canonicalization).
   3. $i \leftarrow \text{0x5FE6EB50C7B537AA}_{i64} - (i \gg 1)$ (wrapping 64-bit subtraction, **arithmetic** right shift by 1).
   4. $y = \text{bits\_to\_f64}(i)$.
   5. Return $y \cdot (1.5 - ((h \cdot y) \cdot y))$ in `f64` (left-to-right inside parentheses).
5. **`Mth.fastInvCubeRoot(x: f32) -> f32`** (32-bit bit-hack + two Newton-Raphson steps):
   1. $i = \text{f32\_to\_bits}(x)$ (`i32`; uses `Float.floatToIntBits`, which **canonicalizes `NaN`** to `0x7FC00000`).
   2. $i \leftarrow \text{0x54A2FA8C}_{i32} - (i /_{\text{trunc}} 3)$ (wrapping 32-bit subtraction, signed integer division truncating toward zero; magic constant $1{,}419{,}967{,}116$).
   3. $y_0 = \text{bits\_to\_f32}(i)$.
   4. Let $C_{2/3} = 0.6666667_f$ (`0x3F2AAAAB`). Perform two iterations in `f32`:
      $$y_{k+1} = (C_{2/3} \cdot y_k) + \frac{1.0_f}{(((3.0_f \cdot y_k) \cdot y_k) \cdot x)} \qquad (k = 0, 1)$$
   5. Return $y_2$.

---

### 3. Rounding, clamping, modular arithmetic, and integer helpers (`Mth`, `Divisor`)

#### Floor, ceil, and fractional part
- `Mth.floor(f32) -> i32`, `Mth.floor(f64) -> i32`: $\lfloor v \rfloor_{32}$.
- `Mth.lfloor(f64) -> i64`: $\lfloor v \rfloor_{64}$.
- `Mth.ceil(f32) -> i32`, `Mth.ceil(f64) -> i32`: $\lceil v \rceil_{32}$.
- `Mth.ceilLong(f64) -> i64`: $\lceil v \rceil_{64}$.
- `Mth.frac(f32) -> f32`: $v - \text{f32}(\lfloor v \rfloor_{32})$.
- `Mth.frac(f64) -> f64`: $v - \text{f64}(\lfloor v \rfloor_{64})$.
  Both `frac` overloads subtract the **integer-floored** value converted back to float, so values beyond integer range saturate and large in-range values lose low bits when converted back to float.

#### Clamping (`Mth.clamp`)
- **Integer** (`i32`, `i64`): $\min(\max(v, \text{lo}), \text{hi})$. If $\text{lo} > \text{hi}$, returns $\text{hi}$.
- **Floating-point** (`f32`, `f64`): if $v < \text{lo}$ return $\text{lo}$; otherwise return $\min_{\text{Java}}(v, \text{hi})$.
  - If $v$ is `NaN`, $v < \text{lo}$ is false and $\min(\text{NaN}, \text{hi})$ returns `NaN`.
  - If $\text{lo} > \text{hi}$ and $v < \text{lo}$, returns $\text{lo}$ (opposite of integer `clamp`).
  - If $v = -0.0$, $\text{lo} = +0.0$, $\text{hi} \ge 0.0$, $-0.0 < +0.0$ is false so $\min(-0.0, \text{hi})$ returns **$-0.0$**.

#### Sign, magnitude, tolerance equality, powers, and lengths
- `Mth.abs(i32)` / `Mth.abs(f32)`: standard Java `Math.abs` (`abs(i32::MIN) == i32::MIN`; `abs(-0.0f) == +0.0f`).
- `Mth.absMax(a, b)` (`i32`, `f32`, `f64`): $\max(|a|, |b|)$.
- `Mth.chessboardDistance(x0, z0, x1, z1)`: $\text{absMax}(x_1 -_{32} x_0,\; z_1 -_{32} z_0)$.
- `Mth.sign(x: f64) -> i32`: $0$ if $x = 0.0$ (both $\pm 0.0$); $+1$ if $x > 0.0$; **$-1$ otherwise (including when $x$ is `NaN`)**.
- `Mth.equal(a, b)` (`f32` and `f64`): $|b - a| < \varepsilon$, where $\varepsilon$ is the **`f32` constant `1.0E-5F`** (`0x3727C5AC`), widened to `f64` (`9.999999747378752e-6`, `0x3EE4F8B580000000`) in the `f64` overload.
- `Mth.square(x)` (`f32`, `f64`, `i32`, `i64`): $x \cdot x$ (wrapping for integers); `Mth.cube(x: f32)`: $(x \cdot x) \cdot x$.
- `Mth.lengthSquared` / `Mth.length` (2D and 3D, `f32` and `f64`): sums $(x^2 + y^2)$ or $((x^2 + y^2) + z^2)$ left-to-right in the input type, then applies `Math.sqrt` (narrowed to `f32` for `f32` overloads).

#### Modulo, division, multiple rounding, quantization, and `Divisor`
- `Mth.positiveModulo(a: i32, m: i32)`: $\text{floorMod}(a, m)$.
- `Mth.positiveModulo(a, m)` (`f32`, `f64`): $((a \bmod_{\text{trunc}} m) + m) \bmod_{\text{trunc}} m$.
- `Mth.isMultipleOf(a: i32, b: i32)`: $(a \bmod_{\text{trunc}} b) = 0$.
- `Mth.floorDiv(a: i32, b: i32)`: $\text{floorDiv}(a, b)$.
- `Mth.positiveCeilDiv(a, b)` (`i32`, `i64`): $-\text{floorDiv}(-a, b)$ (wrapping negation).
- `Mth.roundToward(a, m)` (`i32`, `i64`): $\text{positiveCeilDiv}(a, m) \cdot m$ (wrapping multiply).
- `Mth.quantize(v: f64, res: i32) -> i32`: $\lfloor v / \text{f64}(\text{res}) \rfloor_{32} \cdot \text{res}$ (if $\text{res} = 0$, float division gives $\pm\infty$ or `NaN`, floor saturates, multiplying by $0$ yields $0$ without throwing).
- `Mth.mulAndTruncate(fraction, factor) -> i32`: $(\text{numerator} \cdot_{32} \text{factor}) /_{\text{trunc}} \text{denominator}$ (32-bit wrapping product before division).
- `com.mojang.math.Divisor(numerator: i32, denominator: i32)`:
  - Yields zero elements if $\text{denominator} \le 0$.
  - Otherwise sets $q = \text{numerator} /_{\text{trunc}} \text{denominator}$, $m = \text{numerator} \bmod_{\text{trunc}} \text{denominator}$, and running remainder $r = 0$.
  - For each of `denominator` steps: $r \leftarrow r + m$; if $r \ge \text{denominator}$, set $r \leftarrow r - \text{denominator}$ and yield $q + 1$; else yield $q$. (For negative `numerator`, $m \le 0$ so the increment branch never fires and every step yields $q$.)

#### Power-of-two, bit-width, logarithms, search, and outward stepping
- `Mth.isPowerOfTwo(n)` (`i32`, `i64`): $n \ne 0 \land (n \mathbin{\&} (n - 1)) = 0$. Returns **`true`** for $T_{\min}$ (the sign-bit value `0x80000000` / `0x8000000000000000`).
- `Mth.smallestEncompassingPowerOfTwo(n: i32) -> i32`:
  - Let $r = n -_{32} 1$. Sequentially apply $r \leftarrow r \mid (r \gg k)$ for $k = 1, 2, 4, 8, 16$ (**arithmetic** right shifts), then return $r +_{32} 1$.
  - For $n \in [1, 2^{30}]$, returns the smallest power of two $\ge n$. For $n \in (2^{30}, 2^{31}-1]$ or $n = -2^{31}$, wraps to $-2^{31}$. For $n \in [-2^{31}+1, 0]$, returns $0$.
- `Mth.ceillog2(n: i32) -> i32`:
  - Let $p = n$ if $\text{isPowerOfTwo}(n)$ else $\text{smallestEncompassingPowerOfTwo}(n)$.
  - Sign-extend $p$ to `i64`, multiply by `125613361` (`0x077CB531`), arithmetic right shift by 27, mask with `& 31`, and return `MULTIPLY_DE_BRUIJN_BIT_POSITION[idx]` (see **Constants and IDs**).
  - For $n \le 0$ (except $n = -2^{31}$), $p = 0 \implies \text{result} = 0$. For $n = -2^{31}$, $\text{isPowerOfTwo}$ is true, giving index $16 \implies \text{result} = 31$.
- `Mth.log2(n: i32) -> i32`: $\text{ceillog2}(n) - (0 \text{ if } \text{isPowerOfTwo}(n) \text{ else } 1)$. (For $n = 0$, returns $-1$; for $n = -2^{31}$, returns $31$.)
- `Mth.smallestSquareSide(count: i32) -> i32`: throws if $\text{count} < 0$; else $\lceil \sqrt{\text{f64}(\text{count})} \rceil_{32}$.
- `Mth.binarySearch(from: i32, to: i32, predicate) -> i32`:
  - Let $\text{len} = \text{to} -_{32} \text{from}$. While $\text{len} > 0$: $\text{half} = \text{len} / 2$, $\text{mid} = \text{from} +_{32} \text{half}$; if $\text{predicate}(\text{mid})$ is true set $\text{len} = \text{half}$, else set $\text{from} = \text{mid} +_{32} 1$ and $\text{len} \leftarrow \text{len} - (\text{half} + 1)$. Return `from`.
- `Mth.outFromOrigin(origin, lower, upper, stepSize)` (default `stepSize = 1`):
  - Rejects `lower > upper` or `stepSize < 1`.
  - Clamps $o = \text{clamp}(\text{origin}, \text{lower}, \text{upper})$ and yields $o$ first.
  - At each step from cursor $c$ with $d = |o - c|$: terminates once both $o - d < \text{lower}$ and $o + d > \text{upper}$. Otherwise, letting $\text{prevNeg} = (c \le o)$ and $\text{canPos} = (o + d + \text{stepSize} \le \text{upper})$, if $\neg \text{prevNeg} \lor \neg \text{canPos}$, attempts $a = o - d - (\text{stepSize if prevNeg else } 0)$ and moves to $a$ if $a \ge \text{lower}$; otherwise moves to $o + d + \text{stepSize}$. Thus at each distance $d = k \cdot \text{stepSize}$, the positive offset $o + d$ is visited before $o - d$.

---

### 4. Interpolation, remapping, smoothstep, splines, and easing

#### Basic interpolation and remapping (`Mth`)
- **`lerp(α, p0, p1)`** (`f32` and `f64`): $p_0 + \alpha \cdot (p_1 - p_0)$ (no FMA; at $\alpha = 1$, $p_0 + (p_1 - p_0)$ can differ from $p_1$ by 1 ULP).
- **`lerp2`**, **`lerp3`** (`f32` and `f64`): nested `lerp` (innermost along $\alpha_1$, middle along $\alpha_2$, outermost along $\alpha_3$; see also `docs/specs/math/noise.md`).
- **`lerp(α: f64, a: Vec3, b: Vec3)`**: component-wise `f64` `lerp`.
- **`clampedLerp(α, lo, hi)`** (`f32` and `f64`): if $\alpha < 0$ return `lo`; else if $\alpha > 1$ return `hi`; else return $\text{lerp}(\alpha, \text{lo}, \text{hi})$. (Does **not** short-circuit at $\alpha = 0$ or $\alpha = 1$; returns `NaN` when $\alpha$ is `NaN`.)
- **`lerpInt(α: f32, p0: i32, p1: i32) -> i32`**: $p_0 +_{32} \lfloor \alpha \cdot \text{f32}(p_1 -_{32} p_0) \rfloor_{32}$.
- **`lerpDiscrete(α: f32, p0: i32, p1: i32) -> i32`**: letting $\delta = p_1 -_{32} p_0$, returns $p_0 +_{32} \lfloor \alpha \cdot \text{f32}(\delta -_{32} 1) \rfloor_{32} +_{32} (1 \text{ if } \alpha > 0.0_f \text{ else } 0)$.
- **`inverseLerp(v, lo, hi)`** (`f32` and `f64`): $(v - \text{lo}) / (\text{hi} - \text{lo})$.
- **`map(v, f0, f1, t0, t1)`** / **`clampedMap(v, f0, f1, t0, t1)`** (`f32` and `f64`): $\text{lerp}(\text{inverseLerp}(v, f_0, f_1), t_0, t_1)$ and $\text{clampedLerp}(\text{inverseLerp}(v, f_0, f_1), t_0, t_1)$.

#### Smoothstep, Catmull-Rom, and triangle wave (`Mth`)
- **`smoothstep(x: f32)`**: $(((x \cdot x) \cdot x) \cdot (x \cdot (x \cdot 6.0_f - 15.0_f) + 10.0_f))$ (unclamped).
- **`smoothstepDerivative(x: f32)`**: $((((30.0_f \cdot x) \cdot x) \cdot (x - 1.0_f)) \cdot (x - 1.0_f))$ (unclamped).
- **`catmullrom(α, p0, p1, p2, p3)`** (`f32`, exact left-to-right parenthesization):
  $$0.5_f \cdot \Big(\big((2.0_f \cdot p_1 + (p_2 - p_0) \cdot \alpha) + (((2.0_f \cdot p_0 - 5.0_f \cdot p_1) + 4.0_f \cdot p_2) - p_3) \cdot \alpha \cdot \alpha\big) + ((((3.0_f \cdot p_1 - p_0) - 3.0_f \cdot p_2) + p_3) \cdot \alpha \cdot \alpha \cdot \alpha)\Big)$$
- **`triangleWave(index: f32, period: f32) -> f32`**:
  $$\frac{|(\text{index} \bmod_{\text{trunc}} \text{period}) - \text{period} \cdot 0.5_f| - \text{period} \cdot 0.25_f}{\text{period} \cdot 0.25_f}$$

#### Easing curves (`Ease` and `EasingType`)
All simple easings map `f32 -> f32` (with IDs registered in `EasingType`: `constant` $\mapsto 0.0_f$, `linear` $\mapsto x$, plus the 30 `Ease` curves below). Exact `f32` vs `f64` promotion and lookup-table vs `Math.*` usage are critical:

| Easing ID | Exact formula (`x` is `f32`; `xd` denotes `f64(x)`) |
|---|---|
| `in_quad` | $x \cdot x$ (`f32`) |
| `out_quad` | $1.0_f - (1.0_f - x)^2$ (`f32`) |
| `in_out_quad` | If $x < 0.5_f$: $2.0_f \cdot (x^2)$; else $\text{f32}(1.0 - \text{pow}_{f64}(-2.0 \cdot x_d + 2.0,\; 2.0) / 2.0)$ |
| `in_cubic` | $(x \cdot x) \cdot x$ (`f32`) |
| `out_cubic` | $1.0_f - (1.0_f - x)^3$ (`f32`) |
| `in_out_cubic` | If $x < 0.5_f$: $4.0_f \cdot x^3$; else $\text{f32}(1.0 - \text{pow}_{f64}(-2.0 \cdot x_d + 2.0,\; 3.0) / 2.0)$ |
| `in_quart` | $(x^2)^2$ (`f32`) |
| `out_quart` | $1.0_f - ((1.0_f - x)^2)^2$ (`f32`) |
| `in_out_quart` | If $x < 0.5_f$: $8.0_f \cdot (x^2)^2$; else $\text{f32}(1.0 - \text{pow}_{f64}(-2.0 \cdot x_d + 2.0,\; 4.0) / 2.0)$ |
| `in_quint` | $(x^2)^2 \cdot x$ (`f32`) |
| `out_quint` | $1.0_f - \text{f32}(\text{pow}_{f64}(1.0 - x_d,\; 5.0))$ |
| `in_out_quint` | If $x_d < 0.5$: $((((16.0_f \cdot x) \cdot x) \cdot x) \cdot x) \cdot x$; else $\text{f32}(1.0 - \text{pow}_{f64}(-2.0 \cdot x_d + 2.0,\; 5.0) / 2.0)$ |
| `in_sine` | $1.0_f - \text{Mth.cos}(\text{f64}(x \cdot \text{HALF\_PI}_{f32}))$ |
| `out_sine` | $\text{Mth.sin}(\text{f64}(x \cdot \text{HALF\_PI}_{f32}))$ |
| `in_out_sine` | $-(\text{Mth.cos}(\text{f64}(\text{PI}_{f32} \cdot x)) - 1.0_f) / 2.0_f$ |
| `in_expo` | If $x = 0.0_f$: $0.0_f$; else $\text{f32}(\text{pow}_{f64}(2.0,\; 10.0 \cdot x_d - 10.0))$ |
| `out_expo` | If $x = 1.0_f$: $1.0_f$; else $1.0_f - \text{f32}(\text{pow}_{f64}(2.0,\; -10.0 \cdot x_d))$ |
| `in_out_expo` | If $x < 0.5_f$: ($0.0_f$ if $x = 0.0_f$, else $\text{f32}(\text{pow}_{f64}(2.0,\; 20.0 \cdot x_d - 10.0) / 2.0)$); else ($1.0_f$ if $x = 1.0_f$, else $\text{f32}((2.0 - \text{pow}_{f64}(2.0,\; -20.0 \cdot x_d + 10.0)) / 2.0)$) |
| `in_circ` | $\text{f32}(-\sqrt{\text{f64}(1.0_f - x \cdot x)}) + 1.0_f$ |
| `out_circ` | $\text{f32}(\sqrt{\text{f64}(1.0_f - (x - 1.0_f)^2)})$ |
| `in_out_circ` | If $x < 0.5_f$: $\text{f32}((1.0 - \sqrt{1.0 - \text{pow}_{f64}(2.0 \cdot x_d,\; 2.0)}) / 2.0)$; else $\text{f32}((\sqrt{1.0 - \text{pow}_{f64}(-2.0 \cdot x_d + 2.0,\; 2.0)} + 1.0) / 2.0)$ |
| `in_back` | $x^2 \cdot (2.70158_f \cdot x - 1.70158_f)$ (`f32`) |
| `out_back` | $(1.0_f + 2.70158_f \cdot (x - 1.0_f)^3) + 1.70158_f \cdot (x - 1.0_f)^2$ (`f32`) |
| `in_out_back` | If $x < 0.5_f$: $(((4.0_f \cdot x) \cdot x) \cdot (7.189819_f \cdot x - 2.5949094_f)) / 2.0_f$; else with $d = 2.0_f \cdot x - 2.0_f$: $((d \cdot d) \cdot (3.5949094_f \cdot d + 2.5949094_f) + 2.0_f) / 2.0_f$ |
| `in_elastic` | $0.0_f$ if $x=0$; $1.0_f$ if $x=1$; else $\text{f32}(-\text{pow}_{f64}(2.0,\; 10.0 \cdot x_d - 10.0) \cdot \sin_{f64}((x_d \cdot 10.0 - 10.75) \cdot C_4))$ |
| `out_elastic` | $0.0_f$ if $x=0$; $1.0_f$ if $x=1$; else $\text{f32}(\text{pow}_{f64}(2.0,\; -10.0 \cdot x_d) \cdot \sin_{f64}((x_d \cdot 10.0 - 0.75) \cdot C_4) + 1.0)$ |
| `in_out_elastic` | $0.0_f$ if $x=0$; $1.0_f$ if $x=1$; else $s = \sin_{f64}((20.0 \cdot x_d - 11.125) \cdot C_5)$; if $x < 0.5_f$: $\text{f32}(-(\text{pow}_{f64}(2.0,\; 20.0 \cdot x_d - 10.0) \cdot s) / 2.0)$; else $\text{f32}((\text{pow}_{f64}(2.0,\; -20.0 \cdot x_d + 10.0) \cdot s) / 2.0 + 1.0)$ |
| `out_bounce` | If $x < 0.36363637_f$: $7.5625_f \cdot x^2$; else if $x < 0.72727275_f$: $7.5625_f \cdot (x - 0.54545456_f)^2 + 0.75_f$; else if $x_d < 0.9090909090909091$ (**`f64` comparison**): $7.5625_f \cdot (x - 0.8181818_f)^2 + 0.9375_f$; else $7.5625_f \cdot (x - 0.95454544_f)^2 + 0.984375_f$ |
| `in_bounce` | $1.0_f - \text{out\_bounce}(1.0_f - x)$ |
| `in_out_bounce` | If $x < 0.5_f$: $(1.0_f - \text{out\_bounce}(1.0_f - 2.0_f \cdot x)) / 2.0_f$; else $(1.0_f + \text{out\_bounce}(2.0_f \cdot x - 1.0_f)) / 2.0_f$ |

Here $C_4 = \text{f64}(\text{f32}(2\pi / 3))$ (`0x4000C15240000000`) and $C_5 = \text{f64}((\text{PI}_{f32} \cdot 4.0_f) / 9.0_f)$ (`0x3FF6571860000000`) — both are constant-folded in **`f32`** before widening to `f64`!

#### Cubic Bézier easing (`EasingType.CubicBezier`)
Parameterized by `(x1, y1, x2, y2)` in `f32` (JSON validates $x_1, x_2 \in [0, 1]$; symmetric helper sets $x_2 = 1 - x_1$, $y_2 = 1 - y_1$):
1. Precompute two `f32` cubic polynomials $(a, b, c)$ for X from $(x_1, x_2)$ and for Y from $(y_1, y_2)$:
   $$a = (3.0_f \cdot v_1 - 3.0_f \cdot v_2) + 1.0_f, \quad b = (-6.0_f \cdot v_1) + 3.0_f \cdot v_2, \quad c = 3.0_f \cdot v_1$$
   with Horner evaluation $P(t) = ((a \cdot t + b) \cdot t + c) \cdot t$ and derivative $P'(t) = ((3.0_f \cdot a) \cdot t + 2.0_f \cdot b) \cdot t + c$.
2. Solve $P_x(t) = x$ in `f32`:
   - Start at $t = x$. Run up to **4** Newton-Raphson iterations: compute $e = P_x(t) - x$; if $|e| < 10^{-5}_f$ return $t$; compute $g = P'_x(t)$; if $g < 10^{-5}_f$ exit loop to bisection; update $t \leftarrow t - \text{clamp}_{f32}(e / g,\; -0.25_f,\; 0.25_f)$.
   - **Bisection fallback**: initialize $t_0 = 0.0_f$, $t_1 = 1.0_f$, and initial probe $t = t_{\text{NR}}$ (the final Newton iterate, **not** $0.5_f$). While $t_0 < t_1$: evaluate $e = P_x(t) - x$; if $|e| < 10^{-5}_f$ return $t$; if $e < 0.0_f$ set $t_0 = t$ else set $t_1 = t$; then update $t = (t_1 + t_0) / 2.0_f$. Return $t$ on loop exit.
3. Return $P_y(t)$.

#### Piecewise cubic Hermite spline (`CubicSpline`)
A spline node is either `Constant(v: f32)` (returns $v$, interval $[v, v]$) or `Multipoint(coord, locations[0..n-1], values[0..n-1], derivatives[0..n-1])` with strictly ascending `locations` ($n \ge 1$):
1. **Sampling**: evaluate $u = \text{coord}(c) \in \text{f32}$ and locate segment $k = \text{binarySearch}(0, n,\; i \mapsto u < \text{loc}[i]) - 1$.
   - If $k < 0$: evaluate $y_0 = \text{sample}(\text{values}[0], c)$ and return $y_0$ if $d_0 = 0.0_f$ else $y_0 + d_0 \cdot (u - \text{loc}[0])$.
   - If $k = n - 1$: evaluate $y_{\text{last}} = \text{sample}(\text{values}[n-1], c)$ and return $y_{\text{last}}$ if $d_{\text{last}} = 0.0_f$ else $y_{\text{last}} + d_{\text{last}} \cdot (u - \text{loc}[n-1])$.
   - Otherwise ($0 \le k < n - 1$): let $x_1 = \text{loc}[k]$, $x_2 = \text{loc}[k+1]$, $t = (u - x_1) / (x_2 - x_1)$, $y_1 = \text{sample}(\text{values}[k], c)$, $y_2 = \text{sample}(\text{values}[k+1], c)$, $d_1 = \text{deriv}[k]$, $d_2 = \text{deriv}[k+1]$,
     $$a = d_1 \cdot (x_2 - x_1) - (y_2 - y_1), \qquad b = (-d_2) \cdot (x_2 - x_1) + (y_2 - y_1)$$
     $$\text{result} = \text{lerp}_{f32}(t, y_1, y_2) + (t \cdot (1.0_f - t)) \cdot \text{lerp}_{f32}(t, a, b)$$
2. **Conservative interval range**: returns `NaI` if `coord.range()` is `NaI`. Otherwise accumulates `[min, max]` over: linear extensions at `inputRange.min()` (if $< \text{loc}[0]$) and `inputRange.max()` (if $> \text{loc}[n-1]$) against the first/last child ranges, all child ranges, and for every segment with $d_1 \ne 0 \lor d_2 \ne 0$, the Hermite overshoot bound $\min(m_1, m_2) + 0.25_f \cdot \min(p_1 - M_2 + m_1,\; -p_2 + m_2 - M_1)$ and $\max(M_1, M_2) + 0.25_f \cdot \max(p_1 - m_2 + M_1,\; -p_2 + M_2 - m_1)$ where $p_j = d_j \cdot (x_2 - x_1)$.

#### Stateful interpolation helpers (`SmoothDouble`, `BinaryAnimator`)
- **`SmoothDouble`** (three `f64` fields initialized to `0.0`: `target`, `remaining`, `lastAmount`): on `(targetDelta, time)`, updates `target += targetDelta`; `delta = target - remaining`; `newLast = lerp(0.5, lastAmount, delta)`; `s = signum(delta)`; if `s * delta > s * lastAmount` sets `delta = newLast`; sets `lastAmount = newLast`; `remaining += delta * time`; returns `delta * time`.
- **`BinaryAnimator(length, easing)`**: maintains `ticks`, `ticksOld` in `[0, length]`. On tick: `ticksOld = ticks`, then increments `ticks` if active and `< length`, else decrements if `> 0`. Factor at `partialTicks` is `easing(lerp(partialTicks, f32(ticksOld), f32(ticks)) / f32(length))`.

---

### 5. Angle wrapping, stepping, packing, and rotation (`Mth`, `SegmentedAnglePrecision`)

#### Degree wrapping (`Mth.wrapDegrees`, `Mth.wrapDegrees90`)
- **`wrapDegrees`** overloads (`i32 -> i32`, `i64 -> f32`, `f32 -> f32`, `f64 -> f64`):
  1. Compute $r = \text{angle} \bmod_{\text{trunc}} 360$ (for `i64`, compute `angle % 360L` first, then convert to `f32`).
  2. If $r \ge 180$, $r \leftarrow r - 360$.
  3. If $r < -180$, $r \leftarrow r + 360$.
  4. Return $r \in [-180, 180)$ (half-open: $+180 \mapsto -180$, $-180 \mapsto -180$; for floating-point inputs, $-0.0$ stays $-0.0$ and $\pm\infty / \text{NaN} \mapsto \text{NaN}$).
- **`wrapDegrees90(f32) -> f32`**: identical using modulus $90.0_f$ and half-open range $[-45.0_f, 45.0_f)$.

#### Angle differences, stepping, and rotational interpolation
- `degreesDifference(from, to)` (`f32`): $\text{wrapDegrees}(\text{to} - \text{from})$.
- `degreesDifferenceAbs(a, b)` (`f32`): $|\text{degreesDifference}(a, b)|$.
- `rotateIfNecessary(base, target, maxDiff)` (`f32`): $\text{target} - \text{clamp}_{f32}(\text{degreesDifference}(\text{base}, \text{target}), -\text{maxDiff}, \text{maxDiff})$.
- `approach(current, target, increment)` (`f32`): let $s = |\text{increment}|$; if $\text{current} < \text{target}$ return $\text{clamp}(\text{current} + s, \text{current}, \text{target})$, else return $\text{clamp}(\text{current} - s, \text{target}, \text{current})$.
- `approachDegrees(current, target, increment)` (`f32`): $\text{approach}(\text{current}, \text{current} + \text{degreesDifference}(\text{current}, \text{target}), \text{increment})$.
- `rotLerp(α, from, to)` (`f32` and `f64`): $\text{from} + \alpha \cdot \text{wrapDegrees}(\text{to} - \text{from})$.
- `rotLerpRad(α, from, to)` (`f32`): computes $d = \text{to} - \text{from}$; adds $\text{TWO\_PI}_{f32}$ (`0x40C90FDB`) repeatedly while $d < -\text{PI}_{f32}$ (`0xC0490FDB`), subtracts $\text{TWO\_PI}_{f32}$ repeatedly while $d \ge \text{PI}_{f32}$ (`0x40490FDB`), then returns $\text{from} + \alpha \cdot d$. (Looping accumulates `f32` rounding error for large multi-turn differences and does not terminate on $\pm\infty$.)

#### Byte-packed degrees (`Mth.packDegrees`, `Mth.unpackDegrees`)
- **`packDegrees(angle: f32) -> i8`**: $\text{trunc}_{i8}(\lfloor (\text{angle} \cdot 256.0_f) / 360.0_f \rfloor_{32})$ (left-to-right `f32` multiply then divide, floor to `i32`, keep low 8 bits signed).
- **`unpackDegrees(rot: i8) -> f32`**: $\text{f32}(\text{sign\_extend}_{i32}(\text{rot}) \cdot 360) / 256.0_f$ (signed range $[-180.0_f, 178.59375_f]$, exact in `f32`).

#### Configurable binary angles (`SegmentedAnglePrecision`)
For integer `bitPrecision` $b \in [2, 30]$ (throws outside $[2, 30]$): let $T = 2^b$, $\text{mask} = T - 1$, $s_{\text{to}} = \text{f32}(T) / 360.0_f$, $s_{\text{from}} = 360.0_f / \text{f32}(T)$.
- `normalize(a)`: $a \mathbin{\&} \text{mask}$.
- `fromDegreesWithTurns(deg)`: $\text{round}_{i32}(\text{deg} \cdot s_{\text{to}})$.
- `fromDegrees(deg)`: $\text{normalize}(\text{fromDegreesWithTurns}(\text{deg}))$.
- `toDegreesWithTurns(a)`: $\text{f32}(a) \cdot s_{\text{from}}$.
- `toDegrees(a)`: $d = \text{toDegreesWithTurns}(\text{normalize}(a))$; if $d \ge 180.0_f$ return $d - 360.0_f$, else $d$.
- `isSameAxis(a, b)`: $(a \mathbin{\&} (\text{mask} \gg 1)) = (b \mathbin{\&} (\text{mask} \gg 1))$.
- `fromDirection(dir)`: $0$ for vertical (`UP`/`DOWN`), else $\text{dir.2DDataValue} \ll (b - 2)$ (where South $= 0$, West $= 1$, North $= 2$, East $= 3$).

#### Quaternion twist projection (`Mth.rotationAroundAxis`)
Given `axis` $(a_x, a_y, a_z)$ and quaternion $q = (q_x, q_y, q_z, q_w)$ in `f32`: computes $p = (a_x q_x + a_y q_y) + a_z q_z$, forms $r = (a_x p,\; a_y p,\; a_z p,\; q_w)$, and normalizes $r$ by multiplying each component by $\text{invSqrt}_{f32}(((r_x^2 + r_y^2) + r_z^2) + r_w^2)$ (JOML `Quaternionf.normalize`).

---

### 6. Hashing, mixing, and random sampling helpers (`Mth`, `LinearCongruentialGenerator`, `ChunkPos`, `Vec3i`)

The core RNG engines (48-bit legacy LCG, xoroshiro128++, Mix13, MD5 name hashing, Marsaglia polar Gaussian, and the `Mth.getSeed(x, y, z)` position hash with its 32-bit wrapping $x \cdot 3129871$ term) are specified in `docs/specs/math/random.md`. Additional helpers:

1. **`Mth.murmurHash3Mixer(h: i32) -> i32`** (MurmurHash3 `fmix32`, wrapping 32-bit):
   - $h \leftarrow h \oplus (h \ggg 16)$
   - $h \leftarrow h \times \text{0x85EBCA6B}_{i32}$ ($-2{,}048{,}144{,}789$)
   - $h \leftarrow h \oplus (h \ggg 13)$
   - $h \leftarrow h \times \text{0xC2B2AE35}_{i32}$ ($-1{,}028{,}477{,}387$)
   - return $h \oplus (h \ggg 16)$
2. **`LinearCongruentialGenerator.next(r: i64, c: i64) -> i64`** (quadratic biome/seed mixer, wrapping 64-bit):
   $$\text{next}(r, c) = r \times (r \times 6364136223846793005 + 1442695040888963407) + c$$
   (Note the outer multiplication by $r$: quadratic in $r$, not a linear LCG step.)
3. **`ChunkPos.hash(x: i32, z: i32) -> i32`** (wrapping 32-bit):
   $$(1664525 \cdot x + 1013904223) \oplus \big(1664525 \cdot (z \oplus \text{0xDEADBEEF}_{i32}) + 1013904223\big)$$
4. **`Vec3i.hashCode()`** (wrapping 32-bit): $((y + z \cdot 31) \cdot 31) + x$.
5. **Bounded random helpers (`Mth`)**:
   - `nextInt(rng, min, max)`: if $\text{min} \ge \text{max}$, returns $\text{min}$ **without calling `rng`**; else returns $\text{rng.nextInt}(\text{max} -_{32} \text{min} +_{32} 1) +_{32} \text{min}$.
   - `nextFloat(rng, min, max)` / `nextDouble(rng, min, max)`: if $\text{min} \ge \text{max}$, returns $\text{min}$ **without calling `rng`**; else returns $\text{rng.next}() \cdot (\text{max} - \text{min}) + \text{min}$.
   - `randomBetweenInclusive(rng, min, max)`: **unconditionally** calls $\text{rng.nextInt}(\text{max} -_{32} \text{min} +_{32} 1) +_{32} \text{min}$ (consumes one `nextInt(1)` draw even when $\text{min} = \text{max}$).
   - `randomBetween(rng, min, max)` (`f32`): **unconditionally** calls $\text{rng.nextFloat}() \cdot (\text{max} - \text{min}) + \text{min}$.
   - `normal(rng, mean: f32, dev: f32) -> f32`: $\text{mean} + \text{f32}(\text{rng.nextGaussian}()) \cdot \text{dev}$ (narrows Gaussian to `f32` before multiplying by `dev`).
6. **`Mth.wobble(coord: f64) -> f64`**:
   - Seeds a fresh **legacy 48-bit LCG** (`SingleThreadedRandomSource`, see `docs/specs/math/random.md`) with $\text{sign\_extend}_{i64}(\lfloor \text{coord} \cdot 3000.0 \rfloor_{32})$, draws one `nextDouble()` $u$, and returns:
     $$\text{coord} + \frac{((2.0 \cdot u - 1.0) \cdot 1.0\times 10^{-7})}{2.0}$$
7. **`Mth.createInsecureUUID(rng)`**:
   - Draws `m = rng.nextLong()`, then `l = rng.nextLong()` in that order.
   - `most = (m & ~0xF000L) | 0x4000L` (sets version nibble to 4); `least = (l & 0x3FFFFFFFFFFFFFFFL) | 0x8000000000000000L` (sets RFC 4122 variant bits `10`).

---

### 7. Color and light packing (`ARGB`, `Mth`, `LightCoordsUtil`)

#### 32-bit ARGB layout and basic channel operations (`ARGB`)
Bit layout of packed `i32` color: `[31:24]` alpha, `[23:16]` red, `[15:8]` green, `[7:0]` blue.
- **Extraction**: `alpha = c >>> 24`, `red = (c >> 16) & 0xFF`, `green = (c >> 8) & 0xFF`, `blue = c & 0xFF`. Float accessors divide the extracted `i32` by `255.0f`.
- **Packing**:
  - `color(a, r, g, b)` masks all four channels with `& 0xFF` before shifting; `color(r, g, b)` uses `a = 255`.
  - `color(a: i32, rgb: i32)`, `white(a: i32)`, `black(a: i32)` shift `a << 24` without masking (low 8 bits of `a` land in `[31:24]`), masking `rgb & 0xFFFFFF`.
  - `as8BitChannel(v: f32) -> i32`: $\lfloor v \cdot 255.0_f \rfloor_{32}$ (**unclamped**; wraps modulo 256 when masked or shifted `<< 24`).
  - `opaque(c) = c | 0xFF000000`, `transparent(c) = c & 0x00FFFFFF`.
  - `toABGR(c)` / `fromABGR(c)`: `(c & 0xFF00FF00) | ((c & 0x00FF0000) >> 16) | ((c & 0x000000FF) << 16)`.

#### 10-bit sRGB / linear transfer LUTs (`ARGB`)
1. **`SRGB_TO_LINEAR[0..255]`** (`i16[256]`): for $i \in [0, 255]$, let $c = \text{f32}(i) / 255.0_f$:
   $$t = \begin{cases} \text{f32}\!\left(\text{pow}_{f64}\!\left(\dfrac{\text{f64}(c) + 0.055}{1.055},\; 2.4\right)\right) & \text{if } c \ge 0.04045_f \\ c / 12.92_f & \text{otherwise} \end{cases}$$
   Store $\text{trunc}_{i16}(\text{round}_{i32}(t \cdot 1023.0_f))$.
2. **`LINEAR_TO_SRGB[0..1023]`** (`u8` stored in `i8[1024]`): for $i \in [0, 1023]$, let $c = \text{f32}(i) / 1023.0_f$:
   $$t = \begin{cases} \text{f32}\!\left(1.055 \cdot \text{pow}_{f64}(\text{f64}(c),\; C_{1/2.4}) - 0.055\right) & \text{if } c \ge 0.0031308_f \\ 12.92_f \cdot c & \text{otherwise} \end{cases}$$
   where $C_{1/2.4} = 0.4166666666666667$ (`0x3FDAAAAAAAAAAAAB`). Store $\text{trunc}_{u8}(\text{round}_{i32}(t \cdot 255.0_f))$.
3. **Channel helpers**:
   - `srgbToLinearChannel(srgb) = f32(SRGB_TO_LINEAR[srgb]) / 1023.0f`.
   - `linearToSrgbChannel(linear) = LINEAR_TO_SRGB[floor_i32(linear * 1023.0f)] & 0xFF` (uses **`floor`**, not `round`).
   - `meanLinear(c1, c2, c3, c4)`: alpha is $(a_1 + a_2 + a_3 + a_4) / 4$; each RGB channel averages its four `SRGB_TO_LINEAR` entries via integer `/ 4` and indexes `LINEAR_TO_SRGB`.
   - `linearLerp(α, p0, p1)`: alpha uses `Mth.lerpInt(α, a0, a1)`; each RGB channel applies `Mth.lerpInt(α, SRGB_TO_LINEAR[c0], SRGB_TO_LINEAR[c1])` and maps back via `LINEAR_TO_SRGB`.
   - `srgbLerp(α, p0, p1)`: `Mth.lerpInt` directly on each of A, R, G, B.

#### ARGB arithmetic, blending, and HSV conversion
- `multiply(lhs, rhs)`: if either equals `-1` (`0xFFFFFFFF`), returns the other; else per channel $(c_1 \cdot c_2) / 255$.
- `addRgb` / `subtractRgb`: preserves `alpha(lhs)`; clamps each RGB sum to $\le 255$ / difference to $\ge 0$.
- `multiplyAlpha(c, m: f32)`: returns `0` (clearing RGB as well!) if `c == 0 || m <= 0.0f`; returns `c` if `m >= 1.0f`; else `color(alphaFloat(c) * m, c)`.
- `scaleRGB(c, sr, sg, sb)`: keeps alpha; each RGB channel is `Math.clamp(trunc_i32(f32(ch) * scale), 0, 255)` (truncates toward zero, not floor).
- `scaleRGB(c, scale: i32)`: keeps alpha; each RGB channel is `Math.clamp((i64(ch) * i64(scale)) / 255L, 0, 255)`.
- `greyscale(c)`: $g = \text{trunc}_{i32}((\text{f32}(r) \cdot 0.3_f + \text{f32}(g) \cdot 0.59_f) + \text{f32}(b) \cdot 0.11_f)$, preserves alpha.
- `alphaBlend(dst, src)`: if $a_s = 255$ return `src`; if $a_s = 0$ return `dst`; otherwise $a = a_s + (a_d \cdot (255 - a_s)) / 255$, and for each RGB channel:
  $$c_{\text{out}} = \frac{c_s \cdot a_s + c_d \cdot (a - a_s)}{a}$$
  (all integer truncation toward zero; `a >= 1` because $a_s \ge 1$).
- `average(lhs, rhs)`: per channel $(c_1 + c_2) / 2$.
- `setBrightness(color, brightness: f32)`: converts RGB to HSV in `f32` ($M = \max(r, g, b)$, $m = \min(r, g, b)$, $\Delta = \text{f32}(M - m)$, $S = \Delta / \text{f32}(M)$ if $M \ne 0$ else $0.0_f$; standard 6-sector hue $H \in [0, 1)$), then if $S = 0.0_f$ sets $r = g = b = \text{round}_{i32}(\text{brightness} \cdot 255.0_f)$; otherwise computes $k = (H - \text{f32}(\text{floor}_{f64}(H))) \cdot 6.0_f$, $f = k - \text{f32}(\text{floor}_{f64}(k))$, $p = V(1 - S)$, $q = V(1 - Sf)$, $t = V(1 - S(1 - f))$, selects the RGB permutation from $\text{trunc}_{i32}(k) \in \{0..5\}$, and converts each channel with $\text{round}_{i32}(\text{ch} \cdot 255.0_f)$ (preserving original alpha).
- **`Mth.hsvToRgb(h, s, v)` / `Mth.hsvToArgb(h, s, v, alpha)`** (`f32, f32, f32, i32 -> i32`):
  - `hsvToRgb` passes **`alpha = 0`** (not 255!).
  - Sector index $h_i = \text{trunc}_{i32}(h \cdot 6.0_f) \bmod_{\text{trunc}} 6$; throws `RuntimeException` if $h_i \notin \{0, 1, 2, 3, 4, 5\}$ (which occurs for negative $h \le -1/6$).
  - Fractional offset $f = h \cdot 6.0_f - \text{f32}(h_i)$ (**note**: if $h \ge 1.0_f$, $h_i$ wraps modulo 6 while $h \cdot 6.0_f$ does not, making $f \ge 6.0_f$!).
  - $p = v \cdot (1.0_f - s)$, $q = v \cdot (1.0_f - f \cdot s)$, $t = v \cdot (1.0_f - (1.0_f - f) \cdot s)$.
  - Standard 6-sector assignment $(r, g, b)$: $0 \mapsto (v, t, p)$, $1 \mapsto (q, v, p)$, $2 \mapsto (p, v, t)$, $3 \mapsto (p, q, v)$, $4 \mapsto (t, p, v)$, $5 \mapsto (v, p, q)$.
  - Packs via `ARGB.color(alpha, clamp(trunc_i32(r * 255.0f), 0, 255), ...)` (truncation toward zero, not `round` or `floor`).

#### Packed light coordinates (`LightCoordsUtil`)
- **4-bit packed** (`pack(block, sky)`): `(block << 4) | (sky << 20)`. Unpack: `block = (p >> 4) & 15`, `sky = (p >> 20) & 15`. Constants: `FULL_BRIGHT = 0x00F000F0` (`15728880`), `FULL_SKY = 0x00F00000` (`15728640`). `withBlock(p, b) = (p & 0xFF0000) | (b << 4)`.
- **8-bit smooth packed** (`smoothPack(block, sky)`): `(block & 0xFF) | ((sky & 0xFF) << 16)`. Unpack: `smoothBlock = p & 0xFF`, `smoothSky = (p >> 16) & 0xFF` (range `0..240`).
- **`smoothBlend(n1, n2, n3, center)`**: if `sky(center) > 2 || block(center) > 2`, for each neighbor $n_k$: if $n_k = 0$ set $n_k = \text{center}$, else if $\text{sky}(n_k) = 0$ set $n_k \leftarrow n_k \mid (\text{center} \mathbin{\&} \text{0xFF0000})$. Return $((n_1 + n_2 + n_3 + \text{center}) \gg 2) \mathbin{\&} \text{0x00FF00FF}$.
- **`smoothWeightedBlend`**: weights `smoothSky` and `smoothBlock` in `f32` left-to-right, truncates each sum to `i32`, packs via `smoothPack`.

---

### 8. Position packing to 64-bit longs (`BlockPos`, `SectionPos`, `ChunkPos`, `QuartPos`)

#### `BlockPos` 64-bit packing
Bit widths are derived from the horizontal world bound $30{,}000{,}000$:
- `PACKED_HORIZONTAL_LENGTH` $= 1 + \text{log2}(\text{smallestEncompassingPowerOfTwo}(30{,}000{,}000)) = 1 + \text{log2}(2^{25}) = \mathbf{26}$ bits for X and Z.
- `PACKED_Y_LENGTH` $= 64 - 2 \times 26 = \mathbf{12}$ bits for Y.
- `MAX_HORIZONTAL_COORDINATE` $= 2^{25} - 1 = 33{,}554{,}431$.

| Field | Bit range (MSB:LSB) | Width | Offset | Mask | Signed range |
|---|---|---|---|---|---|
| `X` | `[63:38]` | 26 | 38 | `0x3FFFFFF` | $[-33{,}554{,}432,\; +33{,}554{,}431]$ |
| `Z` | `[37:12]` | 26 | 12 | `0x3FFFFFF` | $[-33{,}554{,}432,\; +33{,}554{,}431]$ |
| `Y` | `[11:0]` | 12 | 0 | `0xFFF` | $[-2{,}048,\; +2{,}047]$ |

- **Pack** `asLong(x, y, z)`:
  $$p = ((\text{i64}(x) \mathbin{\&} \text{0x3FFFFFF}) \ll 38) \mid ((\text{i64}(z) \mathbin{\&} \text{0x3FFFFFF}) \ll 12) \mid (\text{i64}(y) \mathbin{\&} \text{0xFFF})$$
  Values outside the signed range silently wrap to their low 26 or 12 two's-complement bits.
- **Unpack** (sign-extended via 64-bit arithmetic shifts, narrowed to `i32`):
  - `getX(p)` $= \text{trunc}_{i32}(p \gg 38)$
  - `getZ(p)` $= \text{trunc}_{i32}((p \ll 26) \gg 38)$
  - `getY(p)` $= \text{trunc}_{i32}((p \ll 52) \gg 52)$
- **`offset(p, dx, dy, dz)`**: unpacks `(x, y, z)`, adds `(dx, dy, dz)` in wrapping `i32`, and repacks (so overflowing Y wraps within 12 bits and never carries into Z).
- **`getFlatIndex(p)`**: $p \mathbin{\&} \text{0xFFFFFFFFFFFFFFF0}$ (`p & -16L`) — clears the low 4 bits of Y (aligning Y down to a 16-block section floor).

#### `SectionPos` 64-bit packing and 12-bit section-relative packing

| Field | Bit range (MSB:LSB) | Width | Offset | Mask | Signed range |
|---|---|---|---|---|---|
| `X` | `[63:42]` | 22 | 42 | `0x3FFFFF` | $[-2{,}097{,}152,\; +2{,}097{,}151]$ |
| `Z` | `[41:20]` | 22 | 20 | `0x3FFFFF` | $[-2{,}097{,}152,\; +2{,}097{,}151]$ |
| `Y` | `[19:0]` | 20 | 0 | `0xFFFFF` | $[-524{,}288,\; +524{,}287]$ |

- **Unpack**: `x = (int)(p >> 42)`, `z = (int)((p << 22) >> 42)`, `y = (int)((p << 44) >> 44)`.
- **`getZeroNode(p)`**: $p \mathbin{\&} \text{0xFFFFFFFFFFF00000}$ (`p & -1048576L`, zeroes the 20-bit Y field).
- **Block $\leftrightarrow$ section**: `blockToSectionCoord(b) = b >> 4` (arithmetic shift), `sectionRelative(b) = b & 15`, `sectionToBlockCoord(s) = s << 4`.
- **12-bit `sectionRelativePos`** (`i16`): `((x & 15) << 8) | ((z & 15) << 4) | (y & 15)` (XZY order; unpacked via logical shifts `(r >>> 8) & 15`, `(r >>> 4) & 15`, `r & 15`).

#### `ChunkPos` 64-bit packing and `QuartPos` shifts
- **`ChunkPos.pack(x, z)`**: $(\text{i64}(x) \mathbin{\&} \text{0xFFFFFFFF}) \mid ((\text{i64}(z) \mathbin{\&} \text{0xFFFFFFFF}) \ll 32)$ (`X` in low 32 bits, `Z` in high 32 bits). `INVALID_CHUNK_POS = pack(1875066, 1875066)` (`0x001C9C7A001C9C7A`).
- **Region conversions**: `region = chunk >> 5`, `local = chunk & 31`, `minChunk = region << 5`.
- **`QuartPos`**: `fromBlock(b) = b >> 2`, `quartLocal(b) = b & 3`, `toBlock(q) = q << 2`, `fromSection(s) = s << 2`, `toSection(q) = q >> 2`.

---

### 9. Vector and bounding-box arithmetic (`Vec3i`, `BlockPos`, `Vec3`, `Vec2`, `LpVec3`, `AABB`)

#### `Vec3i` and `BlockPos`
- **`compareTo`**: compares **Y, then Z, then X** via wrapping 32-bit subtraction (`this.y - other.y`, etc.).
- **`distManhattan(other)`**: converts each `Math.abs(other.c - this.c)` to **`f32`** before summing and truncating back to `i32`: $\text{trunc}_{i32}((\text{f32}(|\Delta x|) + \text{f32}(|\Delta y|)) + \text{f32}(|\Delta z|))$. Coordinates differing by $> 2^{24}$ lose low bits in `f32`, and a difference of $-2^{31}$ stays negative under `Math.abs`.
- **`distChessboard(other)`**: $\max(\max(|x_1 - x_2|, |y_1 - y_2|), |z_1 - z_2|)$ in `i32`.
- **`distToCenterSqr(x, y, z)`**: lets $d_c = (\text{f64}(\text{this.}c) + 0.5) - c$ and returns $(d_x^2 + d_y^2) + d_z^2$ in `f64`.
- **`BlockPos.clampLocationWithin(Vec3)`** — **asymmetric `f32`/`f64` bounds**:
  - Lower bound per axis $c \in \{x, y, z\}$: $\text{f64}(\text{f32}(c) + 1.0\times 10^{-5}_f)$ (computed in **`f32`**, then widened to `f64`).
  - Upper bound per axis: $(\text{f64}(c) + 1.0) - \text{f64}(1.0\times 10^{-5}_f)$ (computed in **`f64`** using `9.999999747378752e-6`).
  - Applies `Mth.clamp(loc.c, lo, hi)` in `f64`. For $|c| \ge 2^{20}$, `f32(c) + 1e-5f == f32(c)` (zero lower margin), and for $|c| > 2^{24}$, `f32(c)` rounds odd integers away from `c`.

#### `Vec3` (`f64`, `f64`, `f64`) and `Vec2` (`f32`, `f32`)
- **`Vec3.normalize()`**: computes $d = \sqrt{(x^2 + y^2) + z^2}$ in `f64`; if $d < \text{f64}(1.0\times 10^{-5}_f)$ (`0x3EE4F8B580000000`), returns `ZERO`; else $(x/d, y/d, z/d)$. (If $d$ is `NaN`, the comparison is false and it returns `(NaN, NaN, NaN)`.)
- **`Vec3.subtract(x, y, z)`** delegates to `add(-x, -y, -z)`; **`reverse()`** delegates to `scale(-1.0)` (turning `+0.0` into `-0.0`).
- **`Vec3.xRot` / `yRot` / `zRot(radians: f32)`**: evaluates $c = \text{Mth.cos}(\text{f64}(\text{radians}))$ and $s = \text{Mth.sin}(\text{f64}(\text{radians}))$ (`f32` table values), widens $c, s$ to `f64`, and returns:
  - `xRot`: $(x,\; y \cdot c + z \cdot s,\; z \cdot c - y \cdot s)$
  - `yRot`: $(x \cdot c + z \cdot s,\; y,\; z \cdot c - x \cdot s)$
  - `zRot`: $(x \cdot c + y \cdot s,\; y \cdot c - x \cdot s,\; z)$
- **`Vec3.directionFromRotation(rotX: f32, rotY: f32)`** (pitch, yaw in degrees):
  - Computes in `f32`: $\theta_y = (-\text{rotY}) \cdot \text{DEG\_TO\_RAD}_{f32} - \text{PI}_{f32}$ and $\theta_x = (-\text{rotX}) \cdot \text{DEG\_TO\_RAD}_{f32}$.
  - Looks up $c_y = \text{Mth.cos}(\text{f64}(\theta_y))$, $s_y = \text{Mth.sin}(\text{f64}(\theta_y))$, $c_x = -\text{Mth.cos}(\text{f64}(\theta_x))$, $s_x = \text{Mth.sin}(\text{f64}(\theta_x))$ (all `f32`).
  - Multiplies $s_y \cdot c_x$ and $c_y \cdot c_x$ in **`f32`** before widening $(s_y \cdot c_x,\; s_x,\; c_y \cdot c_x)$ to `f64`.
- **`Vec3.rotation() -> Vec2(pitch, yaw)`**: uses JVM **`Math.atan2`** and **`Math.asin`** (not `Mth.atan2`), narrowing each to `f32` before multiplying by `Mth.RAD_TO_DEG` (`0x42652EE0`):
  $$\text{yaw} = \text{f32}(\text{atan2}_{f64}(-x, z)) \cdot \text{RAD\_TO\_DEG}_{f32}, \qquad \text{pitch} = \text{f32}\!\left(\arcsin_{f64}\!\left(\frac{-y}{\sqrt{(x^2 + y^2) + z^2}}\right)\right) \cdot \text{RAD\_TO\_DEG}_{f32}$$
- **`Vec3.projectedOn(onto)`**: if $\text{onto.lengthSqr}() = 0.0$ returns `onto`; else evaluates $(\text{onto} \cdot (\text{this} \cdot \text{onto})) \cdot (1.0 / \text{onto.lengthSqr}())$ in **two separate scalar multiplications** (not multiplying by $\text{dot} / \text{lengthSqr}$).
- **`Vec3.offsetRandom(rng, offset: f32)`** / **`offsetRandomXZ`**: draws `(rng.nextFloat() - 0.5f) * offset` in `f32` per active axis (order X, Y, Z or X, Z), widens to `f64`, and adds.
- **`Vec3.align(axes)`**: replaces each selected axis with $\text{f64}(\lfloor c \rfloor_{32})$.
- **`Vec3.equals` / `hashCode`**: uses `Double.compare` (`-0.0 != +0.0`, `NaN == NaN`) and `Double.doubleToLongBits` with polynomial multiplier 31.
- **`Vec2.normalized()`**: threshold is **`1.0E-4F`** (`0x38D1B717`, ten times `Vec3`'s threshold). **`Vec2.equals`** uses `==` (`-0.0f == +0.0f`, `NaN != NaN`). **`Vec2.rotate(rad: f64)`** uses `Mth.cos`/`Mth.sin` (`f32`), returning $(x c - y s,\; y c + x s)$ in `f32`.

#### Low-precision network vector quantization (`LpVec3`)
Constants: $V_{\max} = 1.7179869183 \times 10^{10} = 2^{34} - 1$ (`0x420FFFFFFFF80000`), $V_{\min} = 1.0 / 32766.0 = 3.051944088384301 \times 10^{-5}$ (`0x3F00004001000400`).
- **Encode**: sanitize each axis (`NaN -> 0.0`, `Math.clamp(v, -V_max, V_max)`). Let $L = \text{absMax}(x, \text{absMax}(y, z))$. If $L < V_{\min}$, write a single `0` byte. Otherwise let $S = \lceil L \rceil_{64}$, $\text{partial} = ((S \mathbin{\&} 3) \ne S)$, $\text{markers} = (S \mathbin{\&} 3) \mid (4 \text{ if partial else } 0)$, quantize each axis $q_c = \text{round}_{i64}(((c / \text{f64}(S)) \cdot 0.5 + 0.5) \cdot 32766.0)$, pack $\text{buf} = \text{markers} \mid (q_x \ll 3) \mid (q_y \ll 18) \mid (q_z \ll 33)$ into 6 bytes written as `[byte(buf), byte(buf >> 8), be_i32(buf >> 16)]`, followed by `VarInt(i32(S >> 2))` if `partial`.
- **Decode**: if first byte $b_0 = 0$, return `Vec3.ZERO`. Otherwise read $b_1$ (u8) and $w$ (big-endian u32), assemble $\text{buf} = (w \ll 16) \mid (b_1 \ll 8) \mid b_0$, extract $S = (b_0 \mathbin{\&} 3) \mid ((\text{u32}(\text{VarInt}) \ll 2) \text{ if } (b_0 \mathbin{\&} 4) \ne 0)$, and unpack each 15-bit lane at shifts 3, 18, 33 via $(\min(q \mathbin{\&} \text{0x7FFF},\; 32766.0) \cdot 2.0 / 32766.0 - 1.0) \cdot \text{f64}(S)$.

#### `AABB` (`minX, minY, minZ, maxX, maxY, maxZ` in `f64`) and ray tests
- **Canonicalizing constructor**: sets $\text{min}_c = \min_{\text{Java}}(a_c, b_c)$ and $\text{max}_c = \max_{\text{Java}}(a_c, b_c)$ per axis. Any operation that delegates through the constructor (`contract`, `expandTowards`, `inflate`, `deflate`, `intersect`, `nextDeflated`, `ofSize`, `setMin*`/`setMax*`) automatically re-sorts inverted bounds!
  - **`AABB(BlockPos)`**, **`of(BoundingBox)`**, **`encapsulatingFullBlocks`**: add `+ 1` in **32-bit wrapping `i32`** before widening to `f64`.
  - **`intersect(other)`**: passes `max(min1, min2)` and `min(max1, max2)` to the constructor — for disjoint boxes, the constructor swaps min and max and yields a box spanning the **gap** between them.
  - **`nextDeflated()`**: passes `(Math.nextUp(min), Math.nextDown(max))` to the constructor — for a zero-thickness axis (`min == max`), `nextUp > nextDown` swaps to `[nextDown, nextUp]` (inflating by 2 ULPs instead of deflating).
- **Predicates**:
  - `intersects(other)`: **strict** inequalities on all 3 axes ($\text{min}_c < \text{other.max}_c \land \text{max}_c > \text{other.min}_c$).
  - `contains(x, y, z)`: **half-open** $[\text{min}_c, \text{max}_c)$ on all 3 axes.
- **Centers and size**:
  - `getCenter()` / `getBottomCenter()`: uses `Mth.lerp(0.5, min, max)` $= \text{min} + 0.5 \cdot (\text{max} - \text{min})$ (not `(min + max) * 0.5`).
  - `getSize()`: $((\text{maxX} - \text{minX}) + (\text{maxY} - \text{minY}) + (\text{maxZ} - \text{minZ})) / 3.0$.
  - `distanceToSqr(Vec3)` / `distanceToSqr(AABB)`: per axis $d_c = \max(\max(\text{min}_c - p_c,\; p_c - \text{max}_c),\; 0.0)$ (or $\max(\max(\text{min}_c - b.\text{max}_c,\; b.\text{min}_c - \text{max}_c),\; 0.0)$), returns $(d_x^2 + d_y^2) + d_z^2$.
- **Segment clipping (`AABB.clip(from, to)`)**:
  - Constants: $\epsilon = 1.0 \times 10^{-7}$ (`0x3E7AD7F29ABCAF48`), initial best parameter $s_{\text{best}} = 1.0$, $(d_x, d_y, d_z) = \text{to} - \text{from}$.
  - Tests faces in fixed order **X, then Y, then Z**:
    - X: if $d_x > \epsilon$ test `minX` (`WEST`); else if $d_x < -\epsilon$ test `maxX` (`EAST`).
    - Y: if $d_y > \epsilon$ test `minY` (`DOWN`); else if $d_y < -\epsilon$ test `maxY` (`UP`).
    - Z: if $d_z > \epsilon$ test `minZ` (`NORTH`); else if $d_z < -\epsilon$ test `maxZ` (`SOUTH`).
  - For each tested face at plane $P$ on primary axis $A$ with transverse axes $(B, C)$: compute $s = (P - \text{from}_A) / d_A$, $p_B = \text{from}_B + s \cdot d_B$, $p_C = \text{from}_C + s \cdot d_C$. Accept and update $s_{\text{best}} = s$ strictly if:
    $$0.0 < s < s_{\text{best}} \;\land\; \text{min}_B - \epsilon < p_B < \text{max}_B + \epsilon \;\land\; \text{min}_C - \epsilon < p_C < \text{max}_C + \epsilon$$
    Notice that $s = 0.0$ and $s = 1.0$ are **strictly excluded**, and face bounds are padded outward by $\epsilon = 10^{-7}$.
  - On hit, returns $\text{from} + (s_{\text{best}} \cdot d_x,\; s_{\text{best}} \cdot d_y,\; s_{\text{best}} \cdot d_z)$. Multi-box `clip(aabbs, from, to, pos)` shares a single $s_{\text{best}}$ across `aabb.move(pos)` in iteration order.
- **`collidedAlongVector(vec, aabbs)`**: sets $\text{from} = \text{getCenter}()$, $\text{to} = \text{from} + \text{vec}$; for each shape box, inflates it by $(s_x \cdot 0.5 - 10^{-7},\; s_y \cdot 0.5 - 10^{-7},\; s_z \cdot 0.5 - 10^{-7})$ and returns `true` if `inflated.contains(to) || inflated.contains(from) || inflated.clip(from, to).isPresent()`.
- **`AABB.Builder`**: accumulates bounds in **`f32`** (from `Vector3fc`, starting at $\pm\infty$), then widens all 6 `f32` bounds to `f64` on `build()`.
- **`Mth.rayIntersectsAABB(start, dir, aabb)`** (SAT / Woo ray test):
  1. For each axis $c \in \{x, y, z\}$: $m_c = (\text{min}_c + \text{max}_c) \cdot 0.5$, $e_c = (\text{max}_c - \text{min}_c) \cdot 0.5$, $\delta_c = \text{start}_c - m_c$. Reject (`false`) if $|\delta_c| > e_c \land \delta_c \cdot \text{dir}_c \ge 0.0$.
  2. Let $a_x = |\text{dir}_x|$, $a_y = |\text{dir}_y|$, $a_z = |\text{dir}_z|$.
  3. X-cross test: reject if $|\text{dir}_y \delta_z - \text{dir}_z \delta_y| > e_y a_z + e_z a_y$.
  4. Y-cross test: reject if $|\text{dir}_z \delta_x - \text{dir}_x \delta_z| > e_x a_z + e_z a_x$.
  5. Z-cross test: return $|\text{dir}_x \delta_y - \text{dir}_y \delta_x| < e_x a_y + e_y a_x$ (**strict `<`**, unlike `>` in steps 3–4!). Consequently, any ray with $\text{dir}_x = \text{dir}_y = 0$ (a pure Z-axis ray) evaluates $0.0 < 0.0$ and returns **`false`** even when aiming directly through the box center.

---

## Edge cases and known quirks

Every item below can produce bit-level or behavioral divergence if implemented naively (verified by `rng-streams` for random helpers and `math-helpers` for all remaining helpers):

1. **`Mth.sin` / `Mth.cos` `f64` indexing uses truncation toward zero, not floor**: In 26.3, inputs are `f64` multiplied by $K = 10{,}430.378350470453$ (`0x40C45F306DC9C883`), converted via saturating `d2l` (truncating toward zero), and masked `& 0xFFFF`. Small negative values in $(-1/K, 0)$ map to index `0` rather than `65535`; values overflowing `i64` saturate to `i64::MAX` (`& 0xFFFF = 65535`) or `i64::MIN` (`& 0xFFFF = 0`); `NaN` maps to index `0` (`0.0f`).
2. **`SIN` table uses division by $K$ and JVM `Math.sin`**: Entries are $\text{f32}(\sin_{f64}(i / K))$, not $\text{f32}(\sin_{f64}(i \cdot 2\pi / 65536))$; e.g., `SIN[32768] = 1.2246469e-16f` (`0x250D3132`), `SIN[65535] = -9.58738e-5f` (`0xB8C90FDB`).
3. **`Mth.RAD_TO_DEG` is 1 ULP below `(float)(180.0 / Math.PI)`**: Defined as `180.0f / (float)Math.PI` = `0x42652EE0` (`57.2957763671875`), not `0x42652EE1` (`57.295780181884766`).
4. **`Mth.invSqrt(f32)` narrows `sqrt` before dividing**: Delegates to JOML's `1.0f / (float)Math.sqrt((double)x)`, differing by 1 ULP from `(float)(1.0 / Math.sqrt(x))` on many inputs.
5. **`Mth.fastInvSqrt(0.0)` is finite, and shifts signed `i64` arithmetically**: Uses `doubleToRawLongBits` (no NaN canonicalization) and arithmetic `>> 1`. At `x = +0.0`, it returns `1.4404511704572167e+154` (`0x5FF1307C95C7E9C0`), not $+\infty$, which is why `Mth.atan2(0.0, 0.0)` returns `0.0` rather than `NaN`.
6. **`Mth.atan2` ignores `-0.0`, uses `fastInvSqrt`, and can index out of bounds on overflow**: Signed zeros fail `< 0.0` (so `atan2(-0.0, -0.0) = +0.0`, unlike `Math.atan2` which returns $-\pi$). Inputs whose $x^2 + y^2$ overflows to $+\infty$ or underflows subnormally can produce `idx` outside `[0, 256]`.
7. **`Mth.fastInvCubeRoot` canonicalizes `NaN` and uses signed truncating division by 3**: Calls `Float.floatToIntBits` (unlike `fastInvSqrt`'s raw bits), divides signed `i32` by 3 toward zero, and runs exactly two Newton steps with `0.6666667f` (`0x3F2AAAAB`).
8. **`Mth.sign(NaN)` returns `-1`**: Checks `== 0.0`, then `> 0.0`, falling through to `-1` for both negative numbers and `NaN`.
9. **`Mth.equal(f64, f64)` compares against widened `f32` `1.0E-5F`**: Threshold is `9.999999747378752e-6` (`0x3EE4F8B580000000`), not `1.0e-5` (`0x3EE4F8B588E368F1`).
10. **`Mth.clamp` float quirks vs integer `clamp` and `Math.clamp`**: For floats, `clamp(-0.0, +0.0, 1.0)` returns `-0.0`; `clamp(v, lo, hi)` with `lo > hi` returns `lo` when `v < lo` (while integer `clamp` always returns `hi`); `NaN` input propagates `NaN`.
11. **`Mth.frac` uses integer `floor`/`lfloor`**: For `f32` with $|v| \ge 2^{31}$ (or `f64` with $|v| \ge 2^{63}$), floor saturates and `frac(v)` does not return `0.0`; for `f32` in $[2^{24}, 2^{31})$, converting `i32` back to `f32` introduces rounding error.
12. **`Mth.isPowerOfTwo` returns `true` for `MIN_VALUE`, and `smallestEncompassingPowerOfTwo` / `ceillog2` / `log2` quirks**: `isPowerOfTwo(i32::MIN)` and `isPowerOfTwo(i64::MIN)` are `true`; `ceillog2(i32::MIN) = log2(i32::MIN) = 31`; `smallestEncompassingPowerOfTwo(n)` returns `0` for $n \in [-2^{31}+1, 0]$ and wraps to `-2^31` for $n > 2^{30}$ or $n = -2^{31}$; `log2(0) = -1` while `ceillog2(0) = 0`.
13. **`Mth.positiveCeilDiv(MIN_VALUE, d)` wraps on negation**: `-i32::MIN == i32::MIN`, flipping the sign of the quotient; `Mth.quantize(v, 0)` returns `0` instead of throwing.
14. **`Mth.lerp` and `clampedLerp` at endpoints**: `lerp(1.0, a, b) = a + (b - a)` can differ from `b` by 1 ULP; `clampedLerp(1.0, a, b)` does **not** short-circuit to `b` (only `factor > 1.0` returns `b`), and `clampedLerp(NaN, a, b)` returns `NaN`.
15. **`Mth.wrapDegrees` half-open `[-180, 180)` vs `rotLerpRad` additive loop**: `wrapDegrees(180) = -180`, `wrapDegrees(-180) = -180`, `wrapDegrees(-0.0) = -0.0`. `rotLerpRad` loops `+= TWO_PI` / `-= TWO_PI` in `f32`, accumulating drift on large differences and looping infinitely on $\pm\infty$.
16. **`Ease` mixed `f32`/`f64` precision and lookup-table sine**:
    - `inSine`/`outSine`/`inOutSine` use `Mth.sin`/`Mth.cos` (quantized table), while `inElastic`/`outElastic`/`inOutElastic` use JVM `Math.sin` with `f32`-folded constants $C_4$ (`0x4000C15240000000`) and $C_5$ (`0x3FF6571860000000`) widened to `f64`.
    - `outBounce` compares its first two thresholds in `f32` (`0.36363637f`, `0.72727275f`) but its third threshold (`0.9090909090909091`) in **`f64`**.
    - `inCirc` narrows `-sqrt(...)` to `f32` before adding `1.0f`.
17. **`EasingType.CubicBezier` bisection initializes `t` from the Newton exit value**: The first bisection check tests the final Newton iterate (not `(0 + 1)/2`), and if $t_0, t_1$ become adjacent `f32` values without meeting $|e| < 10^{-5}_f$, `(t1 + t0)/2.0f` can fail to narrow the bracket.
18. **`Mth.nextInt` vs `Mth.randomBetweenInclusive` RNG consumption when `min == max`**: `Mth.nextInt(rng, k, k)` returns `k` without advancing `rng`; `Mth.randomBetweenInclusive(rng, k, k)` calls `rng.nextInt(1)` and advances `rng` by one draw.
19. **`LinearCongruentialGenerator.next` is quadratic, not linear**: Computes $r \cdot (r \cdot A + B) + c$ (wrapping 64-bit).
20. **`Mth.hsvToRgb` sets alpha to `0`, throws on negative sector, and does not wrap `f` for `hue >= 1`**: Unlike `ARGB.color(r, g, b)` (alpha 255), `hsvToRgb` outputs alpha `0`; `hue <= -1/6` throws `RuntimeException`; `hue >= 1.0f` wraps `h_i` modulo 6 while leaving `f = hue * 6 - h_i >= 6.0f`.
21. **`ARGB` truncation vs floor vs round across methods**: `as8BitChannel` and `linearToSrgbChannel` use `Mth.floor` (unclamped in `as8BitChannel`); `scaleRGB`, `greyscale`, and `Mth.hsvToArgb` use `trunc_i32`; `SRGB_TO_LINEAR`, `LINEAR_TO_SRGB`, and `setBrightness` use `Math.round`. `multiplyAlpha(c, 0.0f)` returns `0` (zeroing RGB, not preserving RGB with alpha 0).
22. **`BlockPos` / `SectionPos` packed-long signed ranges and silent wrapping**: `BlockPos` packs X (26b), Z (26b), Y (12b) — Y wraps modulo 4096 into $[-2048, 2047]$ and X/Z wrap into $[-33{,}554{,}432, 33{,}554{,}431]$; `SectionPos` packs X (22b), Z (22b), Y (20b). `BlockPos.offset(long, ...)` unpacks and repacks so Y overflow never bleeds into Z.
23. **`BlockPos.clampLocationWithin` asymmetric `f32`/`f64` clamping**: Lower bound `(double)((float)coord + 1.0E-5F)` loses precision in `f32` (zero margin for $|c| \ge 2^{20}$; rounds odd integers for $|c| > 2^{24}$), while upper bound `((double)coord + 1.0) - (double)1.0E-5F` stays in `f64`.
24. **`Vec3i.distManhattan` sums in `f32` and `compareTo` subtracts with wrapping**: `distManhattan` loses low bits above $2^{24}$ and can go negative on `i32::MIN` delta; `compareTo` orders Y, Z, X using wrapping subtraction (not `Integer.compare`).
25. **`Vec3` normalization, projection, rotation, and `equals` quirks**: `normalize()` compares against `f64(1.0E-5F)` (not `1e-5`) and returns `(NaN, NaN, NaN)` on `NaN`; `directionFromRotation` computes trig arguments and products in `f32` using `Mth.sin`/`Mth.cos` before widening to `f64`; `rotation()` uses JVM `Math.atan2`/`Math.asin` narrowed to `f32` then multiplied by `Mth.RAD_TO_DEG`; `projectedOn` performs two separate scalings (`scale(dot).scale(1.0 / lenSq)`); `Vec3.equals` uses `Double.compare` (`-0.0 != +0.0`, `NaN == NaN`) whereas `Vec2.equals` uses `==` and `Vec2.normalized` uses threshold `1.0E-4F`.
26. **`AABB` canonicalizing constructor flips disjoint `intersect`, over-`deflate`, and zero-thickness `nextDeflated`**: Any inverted `[min, max]` pair is silently swapped by the constructor (making disjoint `intersect` return the gap box and zero-thickness `nextDeflated` expand by 2 ULPs).
27. **`AABB.clip` strict `(0, 1)` parameter bounds and $10^{-7}$ face pad**: Rejects $s = 0.0$ and $s = 1.0$ strictly, ignores axes with $|d_c| \le 10^{-7}$, checks axes in fixed X, Y, Z order, and widens transverse bounds by $\pm 10^{-7}$.
28. **`Mth.rayIntersectsAABB` strict `<` on the Z-cross test**: Fails (`false`) for any ray parallel to the Z axis ($\text{dir}_x = \text{dir}_y = 0$), because the final test evaluates $0.0 < 0.0$.

---

## Constants and IDs

### Scalar floating-point constants

| Constant | Type | Formula / decimal | IEEE-754 hex |
|---|---|---|---|
| `Mth.PI` | `f32` | $\text{f32}(\pi) = 3.1415927_f$ | `0x40490FDB` |
| `Mth.HALF_PI` | `f32` | $\text{f32}(\pi / 2) = 1.5707964_f$ | `0x3FC90FDB` |
| `Mth.TWO_PI` | `f32` | $\text{f32}(2\pi) = 6.2831855_f$ | `0x40C90FDB` |
| `Mth.DEG_TO_RAD` | `f32` | $\text{f32}(\pi / 180) = 0.017453292_f$ | `0x3C8EFA35` |
| `Mth.RAD_TO_DEG` | `f32` | $180.0_f / \text{f32}(\pi) = 57.295776_f$ | `0x42652EE0` |
| `Mth.EPSILON` | `f32` | $1.0\times 10^{-5}_f$ | `0x3727C5AC` |
| `Mth.EPSILON` widened to `f64` | `f64` | $9.999999747378752\times 10^{-6}$ | `0x3EE4F8B580000000` |
| `Mth.SQRT_OF_TWO` | `f32` | $\text{f32}(\sqrt{2.0}) = 1.4142135_f$ | `0x3FB504F3` |
| `com.mojang.math.Constants.EPSILON` | `f32` | $1.0\times 10^{-6}_f$ | `0x358637BD` |
| `Vec2` normalize threshold | `f32` | $1.0\times 10^{-4}_f$ | `0x38D1B717` |
| `Mth` sine table scale $K$ | `f64` | $65536.0 / (2\pi) = 10430.378350470453$ | `0x40C45F306DC9C883` |
| `Mth` atan2 bias `FRAC_BIAS` | `f64` | $2^{44} = 17592186044416.0$ | `0x42B0000000000000` |
| `Mth` atan2 `ONE_SIXTH` | `f64` | $1.0 / 6.0 = 0.16666666666666666$ | `0x3FC5555555555555` |
| `Mth` `fastInvCubeRoot` $C_{2/3}$ | `f32` | $0.6666667_f$ | `0x3F2AAAAB` |
| `AABB.EPSILON` | `f64` | $1.0\times 10^{-7}$ | `0x3E7AD7F29ABCAF48` |
| `LpVec3.ABS_MAX_VALUE` | `f64` | $2^{34} - 1 = 17179869183.0$ | `0x420FFFFFFFF80000` |
| `LpVec3.ABS_MIN_VALUE` | `f64` | $1.0 / 32766.0 = 3.051944088384301\times 10^{-5}$ | `0x3F00004001000400` |
| `ARGB` sRGB-to-linear threshold | `f32` | $0.04045_f$ | `0x3D25AEE6` |
| `ARGB` linear-to-sRGB threshold | `f32` | $0.0031308_f$ | `0x3B4D2E1C` |
| `ARGB` linear-to-sRGB exponent | `f64` | $1.0 / 2.4 = 0.4166666666666667$ | `0x3FDAAAAAAAAAAAAB` |
| `ARGB` greyscale weights $(R, G, B)$ | `f32` | $(0.3_f,\; 0.59_f,\; 0.11_f)$ | `0x3E99999A`, `0x3F170A3D`, `0x3DE147AE` |
| `Ease` elastic $C_4$ (widened from `f32`) | `f64` | $\text{f64}(\text{f32}(2\pi / 3)) = 2.094395160675049$ | `0x4000C15240000000` |
| `Ease` elastic $C_5$ (widened from `f32`) | `f64` | $\text{f64}((\text{PI}_{f32} \cdot 4.0_f) / 9.0_f) = 1.3962634801864624$ | `0x3FF6571860000000` |
| `Ease` back constants $(c_1, c_3, c_2, c_2+1, 2c_2+2)$ | `f32` | $1.70158_f,\; 2.70158_f,\; 2.5949094_f,\; 3.5949094_f,\; 7.189819_f$ | `0x3FD9CD60`, `0x402CE6B0`, `0x402612FF`, `0x406612FF`, `0x40E612FF` |
| `Ease` bounce thresholds | `f32`, `f32`, `f64` | $0.36363637_f$, $0.72727275_f$, $0.9090909090909091$ | `0x3EBA2E8C`, `0x3F3A2E8C`, `0x3FED1745D1745D17` |
| `Ease` bounce offsets | `f32` | $0.54545456_f$, $0.8181818_f$, $0.95454544_f$ | `0x3F0BA2E9`, `0x3F51745D`, `0x3F745D17` |

### Integer bit-manipulation, hashing, and packing constants

| Constant | Type | Decimal | Hex |
|---|---|---|---|
| `Mth.fastInvSqrt` magic | `i64` | `6910469410427058090` | `0x5FE6EB50C7B537AA` |
| `Mth.fastInvCubeRoot` magic | `i32` | `1419967116` | `0x54A2FA8C` |
| `Mth.ceillog2` De Bruijn multiplier | `i64` | `125613361` | `0x077CB531` |
| `Mth.murmurHash3Mixer` $m_1, m_2$ | `i32` | `-2048144789`, `-1028477387` | `0x85EBCA6B`, `0xC2B2AE35` |
| `LinearCongruentialGenerator` $A, B$ | `i64` | `6364136223846793005`, `1442695040888963407` | `0x5851F42D4C957F2D`, `0x14057B7EF767814F` |
| `ChunkPos.hash` $A, C, Z_{\text{xor}}$ | `i32` | `1664525`, `1013904223`, `-559038737` | `0x0019660D`, `0x3C6EF35F`, `0xDEADBEEF` |
| `ChunkPos.INVALID_CHUNK_POS` | `i64` | `pack(1875066, 1875066)` | `0x001C9C7A001C9C7A` |
| `LightCoordsUtil.FULL_BRIGHT`, `FULL_SKY` | `i32` | `15728880`, `15728640` | `0x00F000F0`, `0x00F00000` |

### `MULTIPLY_DE_BRUIJN_BIT_POSITION[0..31]`

| Index | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Value | 0 | 1 | 28 | 2 | 29 | 14 | 24 | 3 | 30 | 22 | 20 | 15 | 25 | 17 | 4 | 8 |

| Index | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 | 30 | 31 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Value | 31 | 27 | 13 | 23 | 21 | 19 | 16 | 7 | 26 | 12 | 18 | 6 | 11 | 5 | 10 | 9 |

---

## Open questions

1. **JVM `Math.sin`/`Math.cos`/`Math.asin`/`Math.pow` vs fdlibm on `SIN`, `ASIN_TAB`, `COS_TAB`, and `ARGB` tables**: Do any of the 65,536 `SIN` entries, 257 `ASIN_TAB`/`COS_TAB` entries, or 256/1024 `ARGB` sRGB–linear table entries differ between Rust's `libm`/fdlibm and HotSpot 25's `Math.*` intrinsics across Linux x86_64, macOS arm64, and Windows x86_64? Verify exhaustively in the `math-helpers` oracle suite.
2. **Out-of-bounds behavior in `Mth.atan2` and `Mth.hsvToArgb`**: On inputs where Java throws `ArrayIndexOutOfBoundsException` (`atan2` overflow/extreme subnormal) or `RuntimeException` (`hsvToArgb` with `hue <= -1/6`), should Zenith mirror a recoverable error/panic or clamp, given that no valid gameplay state triggers these paths?
3. **`EasingType.CubicBezier` bisection termination guard**: Should the Rust implementation add an iteration cap (or midpoint stagnation check `t == t0 || t == t1`) to prevent infinite loops on pathological out-of-range inputs where two adjacent `f32` endpoints never satisfy $|P_x(t) - x| < 10^{-5}_f$?
4. **Scope of `math-helpers` oracle coverage for `ARGB` JOML vector overloads**: `ARGB` also exposes `Vector3fc`/`Vector4fc` overloads (`multiply`, `addRgb`, `subtractRgb`, `scaleRGB`, `greyscale`, `alphaBlend`, `srgbLerp`). Confirm whether the `math-helpers` suite should cover only the packed `i32` color helpers or both `i32` and `f32` vector overloads.
5. **`ARGB.setBrightness` RGB-to-HSV hue formula (`(g - b) / delta` vs `bc - gc`)**: Section 7 specifies `setBrightness` uses "standard 6-sector hue $H \in [0, 1)$" in `f32`, which can be formulated directly (`(g - b) / delta`, `2 + (b - r) / delta`, `4 + (r - g) / delta`) or via `java.awt.Color.RGBtoHSB` (`rc = (M - r) / delta`, `gc = (M - g) / delta`, `bc = (M - b) / delta`, then `bc - gc`, `2 + rc - bc`, `4 + gc - rc`). These can differ by 1 ULP in `f32` on some RGB triples; confirm which formulation vanilla uses in `math-helpers` (the clean-room implementation currently uses the direct `(g - b) / delta` form).
6. **`LightCoordsUtil.smoothBlend` sky-zero field width and `smoothWeightedBlend` arity**: Confirm whether `smoothBlend(n1, n2, n3, center)` checks the 4-bit fields `(p >> 20) & 15` / `(p >> 4) & 15` (as written) or 8-bit smooth fields `(p >> 16) & 0xFF` / `p & 0xFF` for `sky(center) > 2 || block(center) > 2` and `sky(n_k) == 0`, and confirm that `smoothWeightedBlend` takes 4 light coordinates and 4 `f32` weights `(c0, c1, c2, c3, w0, w1, w2, w3)` accumulated left-to-right.
7. **Consolidating JVM-intrinsic wrappers into `crate::java`**: `zenith_math::java` currently exposes only `java_math_log` (with `java_math_sin` in `zenith_math::noise::utils`), so `zenith_math::helpers::intrinsics` centralizes `java_math_sin`, `java_math_cos`, `java_math_asin`, `java_math_atan2`, `java_math_log`, `java_math_exp`, and `java_math_pow` (delegating `log` to `crate::java` and `sin` to `crate::noise`). Confirm whether all seven should be consolidated into `crate::java`.

## Reader review (2026-11-25)
Method names in this spec are kept on purpose: the `math-helpers` oracle suite calls exactly these public helpers in
the user's own game, so the names are interoperability facts. Three constants were checked numerically: the
degrees-conversion constant is one ULP below `(float)(180/π)` (`0x42652EE0`), sine-table entry 32768 is `0x250D3132`,
and the widened `1.0E-5F` threshold is `0x3EE4F8B580000000`.

## Parity status (2026-11-25)
`math-helpers`: **3,552/3,552** values identical to vanilla 26.3 over 32 probes (table sin/cos, atan2, fast inverse
roots, floor/ceil/frac variants, wrap-degrees variants, the lerp family, smoothstep, positive modulo, power-of-two/log
helpers, HSV, sign, block-position packing), on specials (±0, NaN, ±∞, near 2³¹/2⁶³) and a pseudo-random sweep.

Engineering rule found on the way: Rust does not guarantee the sign/payload of NaN results, and LLVM may legally
rewrite `x / -1.0` into `-x` when the divisor is a compile-time constant (which flips a NaN's sign bit). The running
game follows the hardware's rules. So Zenith code must never depend on a NaN's sign or payload, except where the game
canonicalizes (e.g. NBT writes) or deliberately reads raw bits. In that case, compute with runtime values and cover the
case in a parity suite.
