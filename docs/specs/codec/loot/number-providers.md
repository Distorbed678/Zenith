# Codec: loot number providers, range checks and level-based values (26.3)

- Verified by suites: `loot-tables`
- Sources: local reference reading of the 26.3 game (2026-11-26); `docs/specs/math/random.md`;
  `docs/specs/math/java-math-helpers.md` (`Mth.sin/cos`); `docs/specs/codec/environment-attributes.md` (attribute reads).
  Written in our own words.

## Summary
Loot data never contains a bare Java-style `IntProvider`. Every integer or float that varies (count of a stack, number of
rolls, a chance, a level, a limit) is a **context int provider** or **context float provider**. Both are registries
(`minecraft:context_int_provider`, `minecraft:context_float_provider`) of provider *values*, and a provider slot accepts a registry id, an
inline typed object, **or a bare number** (a constant). Providers are evaluated against the loot context and may draw
from its random source. This file also covers `IntRangePredicate` / `FloatRangePredicate` (used by checks) and the
`IntLimit` (used by `limit_count`) and `LevelBasedValue` (enchantment-level formulas).

## 1. Evaluation protocol

- A provider has an **unsafe** evaluation (may throw an arithmetic failure) and safe wrappers:
  - `getInt(ctx)`: unsafe result; on an arithmetic failure → **0**.
  - `getFloat(ctx)`: unsafe result if **finite**, else **0.0**; an arithmetic failure → 0.0.
  - `getFloatOrThrow(ctx)`: as `getFloat` but propagates the failure (used inside `binomial`'s `p`; the failure then
    propagates up to the enclosing `getInt`, giving 0).
- Failures ("arithmetic failures") are exactly those of §3 (division by zero, exact-arithmetic overflow, int→float
  non-finite…). No other exception types are caught (see Edge cases 5).
- Inputs are evaluated **left to right, in the order the fields are listed**, each exactly once, **before** the
  operation; random draws therefore occur in that order.
- `Float` arithmetic is f32 without fused ops; `Math.*` calls that return `double` are cast to f32 afterwards.

## 2. Codec shapes

- **Slot** (`Holder<ContextIntProvider>` / `…FloatProvider`): `Either<constant, typed>` inside a holder: a JSON number
  → `constant`; a JSON string → registry reference; a JSON object → `{"type": "<id>", …fields}` (type ids may omit
  `minecraft:`); the bare-number form exists for ints (any JSON number accepted as an int by the usual coercion) and
  floats.
- **`ResolvableInt` / `ResolvableFloat`** (used by components such as fuel/cooking time): a bare number, or the
  *resource key* of a registered provider; resolved at use time by looking the key up and calling `getInt`/`getFloat`;
  a missing key yields the supplied default.
- Common field shapes: unary `{input}`; binary `{left, right}`; aggregate `{inputs: [..]}` (a holder set; list or `#`-tag
  is meaningless here, in practice a list; ≥ 1 element required by validation); range `{min, max}`; power `{base, exponent}`;
  conditional `{condition, on_true, on_false = 0}`; dispatcher `{cases: [{condition, value}], default = 0}`;
  distribution `{distribution: [{data, weight ≥ 0}, …]}` (non-empty list, at least one non-zero total weight).

## 3. Int providers (registered type ids; registration order)

`abs, avg, binomial, conditional, constant, sub, environment_attribute, from_float, max, min, floor_mod, floor_div, mod,
div, negate, number_dispatcher, pow, mul, score, storage, add, uniform, weighted_list`.

| type | fields | result (all `int` = 32-bit signed; "exact" ops throw on overflow) |
|---|---|---|
| `constant` | `value: int` | `value` |
| `abs` | `input` | `Math.absExact(x)` (fails for `Integer.MIN_VALUE`) |
| `negate` | `input` | `Math.negateExact(x)` |
| `sub` | `left`, `right` | `Math.subtractExact(l, r)` |
| `add` | `inputs` | sum as 64-bit; failure if the sum is outside i32 |
| `mul` | `inputs` | product as 64-bit starting at 1; wrapping in i64 (silent), failure if outside i32 |
| `avg` | `inputs` | `(i64 sum) / (i64 count)` truncating toward zero; failure if outside i32; zero inputs ⇒ division by zero failure |
| `max` / `min` | `inputs` | fold with `Math.max/min` starting from `Integer.MIN_VALUE` / `MAX_VALUE` |
| `div` | `left`, `right` | `l / r` truncating (failure if `r == 0`; `MIN/−1` wraps silently) |
| `mod` | `left`, `right` | `l % r` (sign of dividend; failure if `r == 0`) |
| `floor_div` | `left`, `right` | `Math.floorDivExact(l, r)` (failure on `r == 0` or `MIN/−1`) |
| `floor_mod` | `left`, `right` | `Math.floorMod(l, r)` (failure on `r == 0`) |
| `pow` | `base`, `exponent` | failure if `base == 0 && exponent == 0`; else `Math.powExact(base, exponent)` (failure on overflow; behaviour for a **negative exponent** is **[oracle]**: Java's exact power may throw an `ArithmeticException` (→ 0) or an `IllegalArgumentException` (crash)) |
| `from_float` | `input` (float provider) | float must be finite else failure; truncate toward zero to 64-bit, failure if not representable in i32 |
| `uniform` | `min`, `max` (int providers) | `min ≥ max ? min : nextInt(max − min + 1) + min`; evaluates `min` then `max` first; **no draw when `min ≥ max`** |
| `binomial` | `n` (int), `p` (float) | evaluate `n` then `p` (via `getFloatOrThrow`); `count = 0; for i in 0..n−1: if nextFloat() < p then count++`; `n ≤ 0` draws nothing |
| `weighted_list` | `distribution` | one `nextInt(totalWeight)` draw, walk the list subtracting weights (selects the first element with cumulative weight > draw), then evaluates the chosen element |
| `conditional` | `condition`, `on_true`, `on_false = 0` | evaluates the condition (may draw), then only the selected branch |
| `number_dispatcher` | `cases`, `default = 0` | cases in order, first whose condition is true wins (later conditions not evaluated); then evaluates only the winner |
| `score` | `target` (score-holder provider, §contexts), `score: string` (objective name), `fallback = 0` | the holder's value in that objective, or `fallback` when the holder can't be resolved, the objective doesn't exist, or the holder has no score |
| `storage` | `storage: id`, `path: NBT path`, `fallback = 0` | the **single** numeric tag selected by `path` in command storage `storage` (`.intValue()` — truncation toward zero for floats/doubles, wrapping for long) else `fallback`; selecting zero or ≥ 2 tags, or a non-numeric tag, ⇒ `fallback` |
| `environment_attribute` | `attribute` | `attribute.type.toInt(value)`; the codec only accepts attributes whose type has an int view (**in 26.3 only the `integer` type, and no registered attribute uses it**, so no valid attribute exists); value via `getValue(ctx, attribute)` of `environment-attributes.md` §6.3 |

`binomial` quirk: `n` and `p` are both evaluated before the first draw; a non-finite `p` fails the whole provider → 0 via
`getInt`.

## 4. Float providers (registered type ids; registration order)

`abs, avg, ceil, conditional, constant, cos, sub, enchantment_level, environment_attribute, floor, from_int, length, max,
min, mod, negate, number_dispatcher, pow, mul, div, round, sin, sqrt, storage, add, truncate, uniform, weighted_list`.

| type | fields | result (f32) |
|---|---|---|
| `constant` | `value: float` | `value` |
| `abs` / `negate` | `input` | `|x|` / `−x` |
| `sub` | `left`, `right` | `l − r` |
| `add` | `inputs` | sum starting at 0.0, left to right |
| `mul` | `inputs` | product starting at 1.0 |
| `avg` | `inputs` | `sum / (f32)count` (sum from 0.0, left to right) |
| `max` / `min` | `inputs` | fold with `Math.max/min` from `−MAX_VALUE` / `+MAX_VALUE` (NaN propagates) |
| `div` | `left`, `right` | `l / r` (IEEE; result non-finite ⇒ final `getFloat` = 0.0) |
| `mod` | `left`, `right` | evaluates `right` **first**: `0.0 → NaN`; else evaluates `left` and returns `l % r` (IEEE remainder-by-truncation, `fmod`) |
| `pow` | `base`, `exponent` | `0^0 → NaN`; else `(float)Math.pow(b, e)` (b, e widened to f64) |
| `length` | `inputs` | `sqrt(Σ x²)` — sum of squares in f32 from 0.0, then `(float)Math.sqrt` |
| `sqrt` | `input` | `(float)Math.sqrt(x)` (negative ⇒ NaN ⇒ 0.0 via `getFloat`) |
| `sin` / `cos` | `input` | `Mth.sin` / `Mth.cos` (the 65 536-entry table, math/java-math-helpers.md) — **not** `Math.sin` |
| `floor` | `input` | `(float)Math.floor(x)` |
| `ceil` | `input` | `Mth.ceil(x)` as a Java int (saturating), converted back to f32 |
| `round` | `input` | `Math.round(x)` → Java int (half up: `floor(x + 0.5)`, saturating, NaN → 0), converted to f32 |
| `truncate` | `input` | `x > 0 ? floor(x) : ceil(x)` as f32 |
| `from_int` | `input` (int provider) | the int converted to f32 (rounds to nearest above 2²⁴) |
| `uniform` | `min`, `max` | evaluates `min`, `max`; `min ≥ max ? min : nextFloat() × (max − min) + min` (f32); **no draw when `min ≥ max`** |
| `weighted_list` | `distribution` | as the int version |
| `conditional` / `number_dispatcher` | as int; default 0.0 | |
| `storage` | `storage`, `path`, `fallback = 0.0` | single numeric tag's `.floatValue()` else `fallback` |
| `enchantment_level` | `amount: LevelBasedValue` | `amount.calculate(level)` with `level` = the context's `enchantment_level` parameter, or 0 if absent |
| `environment_attribute` | `attribute` | `attribute.type.toFloat(value)`; allowed attributes: those of type `float` or `angle_degrees` (e.g. `gameplay/sky_light_level`, the chances); value via `getValue(ctx, attribute)` |

## 5. Range predicates and limits

### `IntRangePredicate` (checks, time checks)
A bare value (any int slot form) = **point**: true iff `input == provider.getInt(ctx)`. An object `{min?, max?}` = **line**:
- both absent → always true;
- only `min` → `input ≥ min.getInt(ctx)`; only `max` → `input ≤ max.getInt(ctx)`;
- both → `input ≥ min.getInt && input ≤ max.getInt` (**`max` is not evaluated when the `min` test fails**).
Bounds are inclusive and evaluated per test (so random providers re-draw each time).

### `FloatRangePredicate`
Same shape with float providers (`getFloat`, so non-finite → 0.0); point test is `input == value` in f32.

### `IntLimit` (`limit_count`)
`{min?, max?}` int providers: neither → input unchanged; only max → `min(max, input)`; only min → `max(min, input)`;
both → `min(max(input, min), max)` (evaluate `min` then `max`; if `min > max` the result is `max`).

### `LevelBasedValue` (enchantment-dependent number; also used by `enchantment_level` and `random_chance_with_enchanted_bonus`)
Codec: a bare float (`constant`) or `{"type": <id>, …}` with ids `clamped, fraction, levels_squared, linear, exponent,
lookup`. `calculate(level: int) → f32`:
- `constant{value}` → `value`.
- `linear{base, per_level_above_first}` → `base + per_level_above_first × (f32)(level − 1)`.
- `levels_squared{added}` → `(f32)(level × level) + added` (the square is an int, `Mth.square(int)`).
- `fraction{numerator, denominator}` → `denominator == 0.0 ? 0.0 : numerator / denominator` (denominator evaluated first).
- `exponent{base, power}` → `(float)Math.pow(base, power)`.
- `clamped{value, min, max}` → `Mth.clamp(value, min, max)` (`value < min ? min : Math.min(value, max)`); decode error unless `max > min`.
- `lookup{values: [float], fallback}` → `level ≤ values.size ? values[level−1] : fallback.calculate(level)`
  (`level ≤ 0` indexes out of range → crash; vanilla never passes level ≤ 0 except `enchantment_level` with no level,
  which passes 0 — a lookup there would crash; **[oracle]** unverified in 26.3 data).

## Edge cases and known quirks
1. `getInt` of a provider that fails returns 0 *after* any random numbers already drawn were consumed (no rollback).
2. `uniform` with `min == max` or `min > max` returns `min` without drawing — different draw count from `min < max`.
3. `weighted_list` weights of zero are legal; a total weight of zero fails at decode ("must contain at least one entry
   with non-zero weight" is the message for an empty list; a non-empty list of all-zero weights throws at use).
4. `from_float` truncates toward zero; `round` rounds half up; `ceil`/`floor` on ints saturate.
5. Only `ArithmeticException`-class failures are swallowed. `Mth.nextInt` with `max − min + 1` overflowing to ≤ 0
   (e.g. `uniform` from i32 min to i32 max) raises a *different* exception (illegal bound) and crashes the loot run in
   the game; not required for parity.
6. Providers are shared objects: a registered provider referenced from several places is evaluated afresh at each use
   (no memoisation).
7. `sin`/`cos` use the lookup table version (`Mth.sin`), unlike `Math.sin`.

## Registered stored providers looked up by game code
The jar's data defines providers under these ids (values are data): int `compostable/{low,low_medium,medium,
medium_high,always_add_one}`, `cooking/time_*`, `cooking/{normal,fast}_burn_time_reduction_factor`, `brewing/uses_default`;
float `cooking/{speed_default,normal_speed_multiplier,fast_speed_multiplier}`, `brewing/speed_default`. Game systems
(composters, furnaces, brewing stands) resolve them through `ResolvableInt/Float` with a context built from the block
(parameter set `block_interact`/`command_*` family, contexts.md). Their definitions use `number_dispatcher`/`conditional`
and the predicate `block/fast_cooking` / `match_block` (data).

## Open questions
- **[oracle]** `Math.sqrt` is IEEE-exact; `Math.pow` may differ by 1 ulp between platforms (HotSpot intrinsic). Probe `Power.getFloatUnsafe` over sampled `(b, e)`.
- **[oracle]** `Math.powExact(int, int)` with a negative exponent, and with `base ∈ {0, 1, −1}`: exception type and
  result. Probe `Power.getIntUnsafe` through `ContextIntProvider.getInt` with exponent −1.

## Oracle probe points
`net.minecraft.world.level.storage.loot.providers.number.ints.*`, `…floats.*`, `ContextIntProvider.getInt`,
`ContextFloatProvider.getFloat`, `IntRangePredicate.test`, `FloatRangePredicate.test`, `IntLimit.clamp`,
`net.minecraft.world.item.enchantment.LevelBasedValue.calculate`.

## Parity status
`loot-tables`: not yet run.
