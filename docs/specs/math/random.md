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
  - `nextDouble()` = ((next(26) << 27) + next(27)) × D, where **D is the single-precision constant written
    1.110223e-16, widened to double**. D is *not* exactly 2⁻⁵³. Using 2⁻⁵³ (as `java.util.Random` does) gives
    different low bits. This is a key parity edge case.
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
  - `nextDouble()` = (step >>> 11) × D (the top 53 bits; same D quirk as above).
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
1. `nextDouble` low bits vs a 2⁻⁵³ implementation (must differ where the float constant matters).
2. Position hash with large |x| (32-bit wrap), negative coordinates, and y extremes.
3. `nextInt(bound)` for powers of two, bound = 1, bound = 2³¹ − 1, and bounds that force rejection loops.
4. Xoroshiro all-zero seed replacement; positional `fromSeed` with s = 0.
5. Gaussian cache behavior across `setSeed`, and names with non-ASCII characters (MD5 over UTF-8).
