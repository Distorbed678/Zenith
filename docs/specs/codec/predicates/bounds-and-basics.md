# Predicates: numeric bounds, distance, NBT, state properties, tags, collections (26.3)

- Verified by suites: `predicates`
- Sources: local reference reading of the 26.3 game (2026-11-26); `docs/specs/codec/datapacks-and-codecs.md` §7 (codec
  framework), `docs/specs/registry/identifiers-registries-tags.md` §2.6 (holder sets), `docs/specs/nbt/snbt.md`,
  `docs/specs/math/java-math-helpers.md`; data-pack data in the user's own jar was read for usage patterns only.
  Written in our own words. Index and shared hook catalogue: [README.md](README.md).

## Summary

The small building blocks every other predicate is made of:

1. **Bounds** — an optional `min` and optional `max` over `int`, `double` or `float`, with a *squared twin* used for
   squared-distance comparisons (§1).
2. **Distance predicate** — per-axis, horizontal and absolute distance between two points, computed in **single
   precision** (§2).
3. **NBT predicate** — "the target's NBT contains this partial NBT" (§3).
4. **State-properties predicate** — exact or ranged matches over block-state / fluid-state properties (§4).
5. **Tag predicate** — "the holder is / is not in this holder set" (§5).
6. **Collection predicate** — `contains` / `count` / `size` over a sequence of elements (§6).

All predicates here are **pure** over their inputs, so the order of the conjuncts is unobservable. The order is still
given wherever the game has one, because an implementation may share it with other call sites.

## Inputs and state

- Registry context for holder sets: the registry spec §2.6. A holder-set field in JSON is `"#namespace:tag"` (named
  set, fails if the tag is missing), a single element id (compact form) or an array of ids (direct set). It is checked
  against the field's registry. Tags used by data-pack data are resolved at load time (`codec/datapacks-and-codecs.md`).
- Optional-field rules are those of `datapacks-and-codecs.md` §7.3: *absent or `null` ⇒ default; present but invalid ⇒
  the whole object fails* (there is no lenient field in this folder unless stated).
- "ANY" below means "no constraint": a bounds with neither `min` nor `max`.

## 1. Numeric bounds (`MinMaxBounds`)

### 1.1 Shape

A bounds value holds `min: Optional<N>` and `max: Optional<N>`. Three numeric flavours exist:

| Flavour | N | Extra data kept | Swapped check on decode |
|---|---|---|---|
| **Ints** | `i32` | squared twin `i64` (`min*min`, `max*max` computed in `i64`) | yes |
| **Doubles** | `f64` | squared twin `f64` (`min*min`, `max*max` in `f64`) | yes |
| **FloatDegrees** | `f32` | none | **no** (a range may wrap, e.g. 350..10 degrees) |

`isAny` = both bounds absent. The squared twin is computed once at construction from the (already validated) bounds, one
squared value per present bound, **independently of sign**: a bounds `{min: -5, max: -2}` has squared twin
`{min: 25, max: 4}` (still decodes; the swapped check looks only at the original bounds).

### 1.2 JSON form (decode)

`either( {min?, max?} , number )`, tried in this order:

1. **Object form.** `min` and `max` are optional keys, each a number of the flavour's type (the usual JSON-number
   coercions of §7.2 of the datapack spec: `Ints` accept `1.9` as `1`, `3000000000` wraps to low 32 bits, …). `{}` is
   valid and means ANY. A present but invalid key makes this alternative fail.
2. **Number form.** A bare number `n` means `{min: n, max: n}` ("exactly n").

Only when both alternatives fail is the error reported (naming both). Then, for **Ints and Doubles only**, the swapped
check runs: if both bounds are present and `min.compareTo(max) > 0` the decode fails ("Swapped bounds in range: min is
higher than max"). Comparison is the *Java box comparison*: for doubles `compareTo` treats `-0.0 < +0.0` and NaN as
greater than everything, so `{min: 0.0, max: -0.0}` is **rejected** while `{min: -0.0, max: 0.0}` is accepted
(JSON cannot spell NaN). `min == max` is fine.

### 1.3 JSON form (encode)

If `min` and `max` are both present and equal (`Optional.equals`, i.e. `Double.equals`/`Integer.equals`) the value is
written as the bare number. Otherwise the object form is written with only the present keys. ANY writes `{}`. (A
"point" requires both bounds present; two empty optionals also compare equal but are ANY and write `{}`.)

### 1.4 Network form

One flag byte followed by the present values: `flags = (min present ? 1 : 0) | (max present ? 2 : 0)`; then `min`
(if bit 0), then `max` (if bit 1). The element codec is big-endian `i32` (Ints), big-endian IEEE `f64` (Doubles) or
big-endian IEEE `f32` (FloatDegrees). Decode does **not** validate swapping and reads only bits 0 and 1 (other bits are
ignored).

### 1.5 Matching

- **Ints `matches(v: i32)`**: false if `min` present and `min > v`; otherwise true iff `max` absent or `max >= v`.
- **Ints `matchesSqr(v: i64)`**: the same two comparisons against the squared twin.
- **Doubles `matches(v: f64)`**: false if `min` present and `min > v`; otherwise true iff `max` absent or **not**
  `(max < v)`. The negated form is deliberate: with a NaN `v` both comparisons are false, so **a NaN value matches every
  Doubles bounds** (ANY or not). With a NaN *bound* (impossible from JSON) the same logic applies.
- **Doubles `matchesSqr(v: f64)`**: the same against the squared twin.
- `-0.0` equals `0.0` in these primitive comparisons (only the decode-time swapped check distinguishes them).
- FloatDegrees has no matching function in this folder's callers (it is used by commands/triggers on rotations).

### 1.6 Command-argument syntax (shared reader)

Commands (`RangeArgument`, selector `distance=`/`level=`/`x_rotation=` …) parse bounds from text. The reader:

1. If no characters remain → error `argument.range.empty`.
2. Read a number token: characters are consumed while they are an ASCII digit, `-`, or `.` — but a `.` is **not**
   consumed when it is immediately followed by another `.` (that pair is the separator). There is no `+`, no exponent,
   no whitespace. An empty token is "no number".
3. A non-empty token is converted with Java's `Integer.parseInt` / `Double.parseDouble` / `Float.parseFloat`; a format
   failure raises the reader's invalid-int / invalid-double / invalid-float error carrying the token.
4. If at least two characters remain and they are `..`, skip them and read a second number token the same way
   (the max); otherwise `max = min`.
5. Both absent → `argument.range.empty`. Any error resets the cursor to where reading began.
6. Ints and Doubles then reject `min > max` with `argument.range.swapped` (cursor reset); FloatDegrees does not.

So `5` = exactly 5, `5..` = at least 5, `..5` = at most 5, `2..8` = between. `1e3`, `+5`, `1..2..3` are not valid
(the token stops early and the rest of the line is left unread for the caller).

## 2. Distance predicate

Fields (all `Doubles`, default ANY): `x`, `y`, `z`, `horizontal`, `absolute`. JSON: an object with those optional keys.
It is an *object of bounds*, not a bounds itself. (Default for the whole value when it is itself optional is the
all-ANY value.)

`matches(x0, y0, z0, x1, y1, z1)` — all six inputs `f64`; the first point is the *reference* (origin / player), the
second the *subject*:

1. `xd = (f32)(x0 − x1)`, `yd = (f32)(y0 − y1)`, `zd = (f32)(z0 − z1)`: the subtraction is in **`f64`**, then each
   difference is **narrowed to `f32`** (round to nearest even; overflow gives ±∞).
2. `x.matches(|xd|)`, `y.matches(|yd|)`, `z.matches(|zd|)` — the `f32` absolute values widened to `f64`. Any failure ⇒
   false. (These use the *plain* bounds.)
3. `horizontal.matchesSqr(f64(xd*xd + zd*zd))` with the sum evaluated entirely in **`f32`**, left to right (no fused
   multiply-add). Failure ⇒ false.
4. `absolute.matchesSqr(f64((xd*xd + yd*yd) + zd*zd))` in `f32`, left to right.

The squared comparisons use the squared twin of the bounds (so `horizontal: {min: 3}` compares the `f32` squared
distance against `9.0`). Because the arithmetic is `f32`, distances of a few thousand blocks lose integer precision;
an implementation using `f64` throughout will disagree at boundaries.

Callers: the entity sub-predicate `distance` (reference = the evaluation position, subject = the entity; see
`entity.md`) and advancement distance triggers (their own spec).

## 3. NBT predicate

### 3.1 Schema

An NBT predicate wraps one compound tag ("the expected tag").

- **JSON decode (lenient codec):** try, in order, (a) a **string** containing SNBT, parsed with the standard SNBT
  grammar (`nbt/snbt.md`) which must yield a compound (a non-compound root is an error "Expected compound tag"), then
  (b) a JSON **object** converted into a compound tag. The first alternative that succeeds wins; if both fail the error
  names both.
  - For (b) the JSON → NBT conversion types numbers by the JSON-to-NBT conversion of the platform library:
    **[oracle]** best-known rule: an integral number becomes the narrowest of byte / short / int / long that holds it;
    a non-integral number becomes a float if it round-trips through `f32` exactly, else a double; `true`/`false` become
    byte `1`/`0`; strings stay strings; arrays become lists. Because the matcher compares tag *classes* exactly (§3.3),
    this typing is observable. Vanilla data uses the SNBT-string form.
- **JSON encode:** the compound written back as an object (the lenient decoder is decode-only).
- **Network:** a nameless compound tag (NBT type byte `10`, then the compound payload; `nbt/binary-format.md`), default
  quota 2,097,152 bytes, depth limit 512. A non-compound root fails.

### 3.2 Where it applies

- **Item / component:** `matches(components)` reads the `custom_data` component (default: the empty compound when the
  component is absent) and runs the compare below with `partialLists = true`. This is the predicate type
  `minecraft:custom_data` and the NBT used by block-entity matching of components.
- **Entity:** `matches(entity)` first builds the entity's *comparison tag* (§3.4).
- **Block entity:** `matches(tag)` against the block entity's full-metadata save (see `block-fluid-location.md` §1).
- A `null` actual tag ⇒ false.

### 3.3 The "contains" comparison (`expected` vs `actual`, partial lists enabled)

`compare(expected, actual)`:

1. Same object identity ⇒ true. `expected` null ⇒ true. `actual` null (and expected not null) ⇒ false.
2. If the two tags are **not the same tag class** (e.g. int vs long, byte vs short, list vs compound) ⇒ false.
   There is no numeric widening and no byte-boolean coercion.
3. **Compound:** if `actual.size < expected.size` ⇒ false (cheap reject). Then for every entry of `expected`, in
   `expected`'s iteration order: `compare(expectedValue, actual[key])` must be true (a missing key yields a null actual ⇒
   false). Extra keys in `actual` are ignored. Recursive.
4. **List (partial matching on):**
   - `expected` empty ⇒ true **iff `actual` is empty** (an expected `[]` does not match a non-empty list).
   - `actual.size < expected.size` ⇒ false.
   - Otherwise every element of `expected` must be matched by **some** element of `actual` (`compare` recursively;
     first success per expected element). Matching is *not* injective: two identical expected elements may both be
     satisfied by one actual element; order and position are irrelevant. Elements of different tag classes simply fail
     rule 2.
5. **Anything else** (numeric tags, strings, byte/int/long arrays): exact equality of the two tags:
   - integer tags: equal values; `float`/`double` tags: equality by the *record/box* rule — `Float.compare`/
     `Double.compare` equal, i.e. **NaN equals NaN, `+0.0` does not equal `-0.0`**;
   - strings: equal code-unit sequences;
   - byte/int/long arrays: same length and equal elements.

Note rule 4's size shortcut counts duplicates in `expected`: `expected = [1,1]` against `actual = [1]` is false even
though each element would be found.

### 3.4 Entity comparison tag (hook, entity milestone)

For the entity predicate the *actual* tag is: the entity's NBT written **without** the `id` key (the "save without id"
form: position, motion, rotation, fall distance, fire, air, on-ground, invulnerable, passengers excluded as per the
entity save routine, plus the type's own data, `Tags`, `CustomName`, etc.), and — **only for a player whose selected
hotbar item is non-empty** — an extra key `SelectedItem` holding that stack in its normal item-stack NBT form. The
exact key set is the entity save format (entity milestone); the registry-aware output context is used (components /
holders are written with registry ids).

## 4. State-properties predicate

### 4.1 Schema

A JSON **object** mapping *property name* → *value matcher*. Decoded to an ordered list of `(name, matcher)` in the
object's key order (duplicate keys: last wins at the JSON layer). A value matcher is `either(exact, ranged)`:

- **Exact:** a JSON **string** — the property value's *name* (`"true"`, `"north"`, `"3"`). A number or boolean in
  the JSON is **not** accepted (strict string).
- **Ranged:** an object `{min?: string, max?: string}`; each is a value-name string. `{}` is valid.

Encode: the list is written back as a map (so key order of the output follows the platform map, not the list).
**Network:** a VarInt-prefixed list of entries `(UTF-8 name ≤ 32767, bool isExact, payload)` where `isExact = true` is
followed by the UTF-8 value string and `isExact = false` by `optional(string) min`, `optional(string) max` (each a
boolean presence byte then the string).

### 4.2 Evaluation

`matches(state)` = every `(name, matcher)` in list order must hold (empty list ⇒ true). For one entry:

1. Look up the property by `name` in the **state definition of the state's owner** (the block's definition for a block
   state, the fluid type's for a fluid state). **Unknown name ⇒ false** (it is not an error at load: the game has a
   validation helper for unknown names but nothing in 26.3 calls it, so unknown properties silently never match).
2. **Exact:** parse the string with the property's `getValue(name)`:
   - integer property: Java `Integer.parseInt` (accepts a leading `+` or `-`, leading zeros; rejects blanks and
     non-digits), then must lie in the property's `[min, max]`; otherwise "no value";
   - boolean property: exactly `"true"` or `"false"`;
   - enum property: exactly the value's serialized name (case-sensitive).
   "No value" ⇒ false. Otherwise true iff the state's current value `compareTo`s equal (0) to the parsed value.
3. **Ranged:** let `v` = the state's current value.
   - if `min` present: parse it (as above); "no value" ⇒ false; `v.compareTo(min) < 0` ⇒ false.
   - if `max` present: parse; "no value" ⇒ false; `v.compareTo(max) > 0` ⇒ false.
   - otherwise true. (`{}` matches any state that *has the property*.)

**Comparison order of values** (`compareTo`): integers numerically; booleans `false < true`; **enum values by their
declaration (ordinal) order in the game's enum, not by any property-value list order** (e.g. `facing` ranges follow the
`Direction` ordinal order; `shape` follows the enum's declaration order). The enum orders are facts of the block
tables (`blocks/state-facts.md`) and the implementation must expose each enum's ordinal order.

## 5. Tag predicate (`TagPredicate`)

JSON object `{id: holder set (required), expected: bool (required)}` where `id` is a holder set of some registry
(`#tag`, single id, or id array). `matches(holder)` = `(holder ∈ set) == expected`. Used for damage types
(`damage-and-misc.md`). Membership follows registry spec §2.6 (named set: tag membership of the holder; direct set:
element equality of holders; a *direct* holder never belongs to a named set).

## 6. Collection predicates

These are generic over an element type and an element predicate `P` (an item predicate, an effect predicate, an
explosion predicate, …). The JSON element codec is that predicate's own codec.

### 6.1 `CollectionPredicate` (object, all three keys optional, no defaults — absent = not checked)

| Key | Value | Meaning |
|---|---|---|
| `contains` | list of `P` | see §6.2 |
| `count` | list of `{test: P, count: Ints}` | see §6.3 |
| `size` | `Ints` bounds | `size.matches(number of elements in the collection)` |

Evaluation order: `contains`, then `count`, then `size`; any present check that fails ⇒ false. An all-absent object
matches every collection. (`contains` and `count` lists are decoded as JSON arrays only — there is no single-element
shorthand.)

### 6.2 `contains`

Let `tests` be the decoded list of `P`.
- 0 tests ⇒ true.
- 1 test ⇒ true iff **some** element satisfies it.
- ≥ 2 tests ⇒ scan the elements once, in order, keeping a working set of the not-yet-satisfied tests; **each element
  satisfies every remaining test it matches** (one element can satisfy several tests; tests are not paired to distinct
  elements); the result is true as soon as the working set is empty, false if the scan ends first (so an empty
  collection with ≥ 2 tests is false).

### 6.3 `count`

A list of entries `{test, count}` (both keys required). Each entry counts the elements satisfying `test` and requires
`count.matches(that number)`. All entries must hold (empty list ⇒ true). Elements are counted independently per entry.

### 6.4 Network

`CollectionPredicate` appears in network forms only inside predicates that travel as NBT (the data-component
predicates; `item-and-components.md` §3), so the JSON codec is the only schema.

## Edge cases and known quirks

1. `Doubles.matches(NaN)` is true for every bounds; `Ints` cannot be NaN.
2. `{min: 0.0, max: -0.0}` fails to decode; `{min: -0.0, max: 0.0}` decodes; both behave identically afterwards.
3. Squared twins are signless: negative bounds produce a squared interval that no longer corresponds to the original
   (documented in §1.1); real data never uses negative squared bounds.
4. The distance predicate rounds the *difference* to `f32` before squaring — it is not `(f32)` of the exact distance.
5. NBT `contains` for lists is existence-based and non-injective; for compounds it is subset; an expected empty list
   needs an empty actual list; tag classes must match exactly (`1b` ≠ `1`).
6. Exact state values are strings: `{"age": 3}` fails to decode; `{"age": "3"}` is right; `"03"` and `"+3"` also match 3.
7. Ranged enum comparisons use the enum's ordinal order.
8. Unknown property names evaluate to false instead of failing the load.

## Constants and IDs

| Name | Value |
|---|---|
| Bounds network flag bits | `0x01` min present, `0x02` max present |
| Command range errors | `argument.range.empty`, `argument.range.swapped` |
| Network NBT quota / depth | 2,097,152 bytes / 512 |
| State property name limit (network) | 32767 UTF-8 chars |
| `SelectedItem` | key added to a player's comparison tag |

## Open questions

1. **[oracle]** JSON-object form of the NBT predicate: confirm the numeric typing rule of §3.1 (byte/short/int/long
   narrowing, float-if-exact else double). Probe: decode `{"a":1,"b":300,"c":70000,"d":3000000000,"e":1.5,"f":0.1}` with
   the lenient codec and report each tag's class.
2. **[oracle]** Whether an NBT-predicate string that is valid SNBT but followed by trailing text fails (the platform's
   "parse fully" is expected to reject it). Probe: `"{a:1} x"`.
3. **[oracle]** Order of the compound `expected` iteration (affects only the time of rejection, not the result); no
   probe needed unless a parity case shows a divergence.
4. **[oracle]** The exact (ordinal) declaration order of every enum used by state properties — captured from
   `blocks/state-facts.md` data; the probe is `Property.getPossibleValues()` vs `Enum.ordinal()` for each enum property
   (they differ only if the game lists values in a non-declaration order).

## Oracle probe points

`MinMaxBounds$Doubles#matches`, `#matchesSqr`, `MinMaxBounds$Ints#matches`, `#matchesSqr`,
`MinMaxBounds$Bounds#createCodec`, `#createStreamCodec`, `#fromReader`, `DistancePredicate#matches`,
`NbtUtils#compareNbt`, `NbtPredicate#matches`, `NbtPredicate#getEntityTagToCompare`,
`StatePropertiesPredicate#matches`, `StatePropertiesPredicate$ExactMatcher#match`,
`StatePropertiesPredicate$RangedMatcher#match`, `TagPredicate#matches`, `CollectionPredicate#test`,
`CollectionContentsPredicate$Multiple#test`, `CollectionCountsPredicate$Entry#test`.

## Parity status

`predicates`: not yet run.
