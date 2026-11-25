# Math: random number sources (26.3)

- Verified by suites: `rng-streams` (oracle samples × property tests)
- Sources: JDK documentation of `java.util.Random` (48-bit LCG), the public xoroshiro128++ algorithm (Blackman &
  Vigna), the public SplitMix/Stafford "Mix13" finalizer constants, and local reference reading (2026-11-25) for how
  the game composes them. Everything below is written in our own words.

Integer arithmetic is Java's: 32-bit `int` and 64-bit `long`, two's complement, wrapping on overflow; `>>` is an
arithmetic shift and `>>>` a logical shift. Floats are IEEE-754.

## Seed helpers
- **Golden ratio constant** G = 0x9E3779B97F4A7C15 and **silver ratio constant** S = 0x6A09E667F3BCC909.
- **Mix13** (Stafford variant 13): z ← (z ⊕ (z >>> 30)) × 0xBF58476D1CE4E5B9; z ← (z ⊕ (z >>> 27)) × 0x94D049BB133111EB;
  result z ⊕ (z >>> 31). Wrapping multiplication.
- **Upgrade a 64-bit seed to 128 bits:** lo = seed ⊕ S; hi = lo + G (wrapping); then apply Mix13 to each half.
  (An "unmixed" variant skips the Mix13 step; it exists but is used only where noted by later specs.)
- **Seed from a name:** MD5 of the name's UTF-8 bytes; the first 8 digest bytes (big-endian) are `lo`, the last 8
  (big-endian) are `hi`.
- **Position hash** h(x, y, z) (x, y, z are 32-bit):
  1. a = x × 3129871 computed in **32-bit** arithmetic (it wraps), then sign-extended to 64 bits;
  2. b = z × 116129781 computed in 64-bit arithmetic;
  3. s = a ⊕ b ⊕ y (y sign-extended);
  4. s = s × s × 42317861 + s × 11 (64-bit, wrapping);
  5. result = s >> 16 (arithmetic shift).
  The 32-bit wrap in step 1 is a deliberate parity edge case. Test x values whose product overflows 32 bits.

## Legacy source (48-bit LCG, same as `java.util.Random`)
- State: 48 bits. `setSeed(s)`: state = (s ⊕ 0x5DEECE66D) AND (2⁴⁸ − 1). Setting the seed also clears the Gaussian
  cache.
- `next(bits)`: state = (state × 0x5DEECE66D + 0xB) AND (2⁴⁸ − 1); return the top `bits` of the 48, as a signed
  32-bit value (state >> (48 − bits), truncated to int).
- Derived operations (shared by every "bit source"):
  - `nextInt()` = next(32).
  - `nextInt(bound)`: bound ≤ 0 is an error. If bound is a power of two: (bound × next(31)) >> 31 in 64-bit.
    Otherwise draw r = next(31), m = r mod bound, and redraw while r − m + (bound − 1) overflows 32-bit (is negative).
  - `nextLong()` = (next(32) << 32) + next(32), where the **second value is sign-extended before the add**.
  - `nextBoolean()` = next(1) ≠ 0.
  - `nextFloat()` = next(24) × F, where F is the single-precision constant written 5.9604645e-8 (exactly 2⁻²⁴).
  - `nextDouble()` = ((next(26) << 27) + next(27)) × D, where D is the single-precision constant written
    1.110223e-16, widened to double. That float rounds to **exactly 2⁻⁵³**, so D = 2⁻⁵³ and the result equals
    `java.util.Random`'s. Implement D as the widened float literal anyway, so the value's origin stays obvious.
    *(Correction 2026-11-25: an earlier revision of this spec claimed D ≠ 2⁻⁵³. That was a reader arithmetic error,
    caught by the implementer's open question and confirmed numerically.)*
- `fork()` = new legacy source seeded with this.nextLong().
- Positional factory (seeded with one 64-bit value P, from this.nextLong()):
  - at(x, y, z) → legacy source seeded with h(x, y, z) ⊕ P;
  - from a name → legacy source seeded with (Java `String.hashCode` of the name, sign-extended) ⊕ P;
  - from a seed s → legacy source seeded with s (P is **not** mixed in).

## Xoroshiro source (xoroshiro128++)
- State (lo, hi). If both are zero at construction they become (G, S) instead.
- Step: result = rotl(lo + hi, 17) + lo; hi' = hi ⊕ lo; lo = rotl(lo, 49) ⊕ hi' ⊕ (hi' << 21); hi = rotl(hi', 28).
  All wrapping 64-bit.
- Construction from a 64-bit seed uses the 128-bit upgrade above (with Mix13). Construction from (lo, hi) uses them
  directly. `setSeed(s)` re-runs the upgrade and clears the Gaussian cache.
- Operations:
  - `nextLong()` = one step.
  - `nextInt()` = low 32 bits of a step.
  - `nextInt(bound)` (bound ≤ 0 is an error), using **multiply-and-reject** (not the legacy algorithm): take the
    32-bit `nextInt()` as unsigned u; p = u × bound (64-bit); frac = low 32 bits of p. If frac < bound, compute
    t = (2³² − bound) mod bound (unsigned), and while frac < t redraw u and recompute p and frac. Result = p >> 32.
  - `nextBoolean()` = lowest bit of a step.
  - `nextFloat()` = (step >>> 40) × F (the top 24 bits).
  - `nextDouble()` = (step >>> 11) × D (the top 53 bits; D = 2⁻⁵³ as above).
  - `consumeCount(n)` = n steps, results discarded.
- `fork()` = new xoroshiro source from (nextLong(), nextLong()), in that order.
- Positional factory (seeded with (L, H) = (nextLong(), nextLong())):
  - at(x, y, z) → xoroshiro from (h(x, y, z) ⊕ L, H), raw (no upgrade);
  - from a name → xoroshiro from (name-hash.lo ⊕ L, name-hash.hi ⊕ H);
  - from a seed s → xoroshiro from (s ⊕ L, s ⊕ H).

## Gaussian (both sources)
Marsaglia polar method with a one-value cache: draw x = 2·nextDouble() − 1 and y = 2·nextDouble() − 1 until
0 < r = x² + y² < 1; m = sqrt(−2·ln(r) / r); cache y·m and return x·m. A later call returns the cached value
first. Setting the seed clears the cache.
- **Edge case:** the game uses `Math.log`, not `StrictMath.log`. On HotSpot, `Math.log` can use a platform
  intrinsic, so results may differ from fdlibm by an ulp on some CPUs. Zenith must match vanilla *on the same
  machine*. Implement fdlibm `log` first, then verify against oracle-captured samples on every CI platform. If a
  platform differs, record it as a per-platform quirk.

## Edge cases to test (oracle `rng-streams`)
1. `nextDouble` equals top-bits × 2⁻⁵³ exactly (D is the widened float constant, which is exactly 2⁻⁵³).
2. Position hash with large |x| (32-bit wrap), negative coordinates, and y extremes.
3. `nextInt(bound)` for powers of two, bound = 1, bound = 2³¹ − 1, and bounds that force rejection loops.
4. Xoroshiro all-zero seed replacement; positional `fromSeed` with s = 0.
5. Gaussian cache behavior across `setSeed`, and names with non-ASCII characters (MD5 over UTF-8).

## Open questions
- **Constant `D` exact IEEE-754 bit pattern:** The spec states both that `D` is "the single-precision constant written 1.110223e-16, widened to double" AND that "`D` is *not* exactly 2⁻⁵³" (causing different low bits from `java.util.Random`). However, in IEEE-754 binary32, `2⁻⁵³` is exact (`0x25000000`), and `1.110223e-16` is within `0.37` binary32 ULPs of `2⁻⁵³` (`Float.toString((float) 0x1.0p-53)` is `"1.110223E-16"`). Consequently `(double) 1.110223E-16F` is bit-identical to `2⁻⁵³` (`0x3CA0000000000000`), whereas the **double-precision** literal `1.110223E-16D` is `0x3C9FFFFFF4178DA3` (`!= 2⁻⁵³`). Please verify the exact constant-pool entry (`Float` `0x25000000` + `f2d` == `2⁻⁵³`, vs `Double` `0x3C9FFFFFF4178DA3`, vs another literal) in the reference bytecode / oracle. Implemented conservatively as `1.110223e-16_f64` (`0x3C9FFFFFF4178DA3`) so `D != 2⁻⁵³` and Edge Case 1 hold until confirmed.
- `LegacyRandom::consumeCount(n)`: `consumeCount(n)` is specified for Xoroshiro (`n` steps), but not explicitly listed under Legacy. Implemented conservatively as `n` calls to `nextInt()` (1 LCG step per count, matching Xoroshiro's 1 step per count, without clearing the Gaussian cache). Confirm whether vanilla's `LegacyRandomSource` inherits `RandomSource.consumeCount` as `n` calls to `nextInt()`.
- `XoroshiroPositionalRandomFactory` zero base state: if a positional factory is ever constructed directly with `(L, H) = (0, 0)`, does the factory itself keep `(0, 0)` and rely on `XoroshiroRandom` construction to apply `(0, 0) -> (G, S)` (our conservative choice), or does the factory normalize `(0, 0)` at factory construction?


## Answers from the reader (2026-11-25)
- **Q1 (D):** the implementer is right. The float literal rounds to exactly 2⁻⁵³, so D = 2⁻⁵³ (`0x3CA0000000000000`).
  The spec text above has been corrected.
- **Q2 (legacy `consumeCount`):** n × `nextInt()`, i.e. n LCG steps, and the Gaussian cache is untouched. (This is the
  shared default for every source; xoroshiro's own version does n raw steps.)
- **Q3 (positional factory zero state):** the factory stores (L, H) as given. The all-zero replacement happens only when
  a generator is constructed, so `at`/`fromSeed` results that come out as (0, 0) become (G, S) at that point.

## Parity status (2026-11-25)
`rng-streams`: **38,594/38,594** values identical to vanilla 26.3 on the reference machine (Linux x86_64, HotSpot
25). This includes 3,488 Gaussian samples, so on this platform `Math.log` and fdlibm-style `ln` agree bit for bit.
CI repeats the check on the other platforms.
