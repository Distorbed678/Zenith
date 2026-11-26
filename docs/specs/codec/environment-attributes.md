# Codec: environment attributes, timelines and the attribute-resolution system (26.3)

- Verified by suites: `environment-attributes` (attribute values resolved at positions / times, per attribute, against the game)
- Sources: local reference reading of the 26.3 game (2026-11-26) for the registry, the layer system, modifiers,
  timelines, keyframe sampling and every gameplay reader; `docs/specs/math/java-math-helpers.md` (easing curves, `Mth`
  helpers); `docs/specs/math/random.md`; `docs/specs/ticks/server-tick.md`; `docs/specs/lighting/chunk-steps.md`
  (sky darkening). Everything below is written in our own words. Vanilla *data* (dimension types, biomes, timelines,
  world clocks) is **not** reproduced here: implementations read it from the user's own jar.

## Summary

An **environment attribute** is a named, typed value that can differ by dimension, biome, time of day and weather.
Sky colors, fog, ambient sounds and many gameplay rules (does a bed work, does water evaporate, may a raid start, how
likely does a turtle egg progress) are attributes. 26.3 replaced several hard-coded dimension fields with them.

Each attribute is registered (registry `minecraft:environment_attribute`) with a **value type**, a **default**, an
optional **value range**, and four flags. A **world** (per `Level`) owns one *attribute system*. For every attribute
the system holds an ordered list of **layers**; the value at a position and time is the default folded through the
layers in order, then sanitized to the range. Layers come from the dimension type, the biomes, the dimension's
**timelines** (keyframed data tied to a world clock) and, for dimensions that can have weather, a built-in weather
layer.

The server resolves attributes in two ways: `getValue(attribute, position)` (positional) and
`getDimensionValue(attribute)` (position-free). Dimension values are cached for the duration of one server tick.

## Inputs and state

- Registry `minecraft:attribute_type` (the value kinds, §2), registry `minecraft:environment_attribute` (§3).
- Per dimension: `DimensionType.attributes` (an attribute map, §5), `DimensionType.timelines` (a holder set of
  timeline registry entries, ordered), `DimensionType.default_clock` (optional world clock).
- Per biome: `Biome.attributes` (an attribute map restricted to *positional* attributes).
- Registry `minecraft:timeline` (§7) and registry `minecraft:world_clock`. A clock instance exposes
  `totalTicks` (a non-negative 64-bit tick counter). The server clock manager advances **every** clock by one tick per
  server tick while the `advance_time` game rule is on (otherwise no clock moves), and command/sleep changes
  invalidate every level's attribute cache immediately.
- Weather: the level's rain level and thunder level (`rainLevel`, `thunderLevel` floats in [0, 1]); the layer reads
  them sampled at partial tick 1.0.
- A biome source (for the biome layer, §6.2).
- A **tick id** counter and a **cached dimension value** per attribute (§6.4).

Arithmetic note: every `Float`-typed attribute computation is IEEE-754 binary32 with no fused multiply-add unless
stated. Integer attribute arithmetic would wrap, but no 26.3 attribute uses the `integer` type (the type exists and
is registered).

---

## 1. Definitions

- A **modifier** (a.k.a. operation) maps `(current value, argument) → new value`. Each attribute type has a fixed
  library of modifiers (§4). The universal modifier is **`override`**: result = argument.
- A **layer** is one of three kinds:
  - **Constant** layer: `value' = f(value)` with no inputs besides the value.
  - **Time-based** layer: `value' = f(value, tickId)`; reads a world clock or weather.
  - **Positional** layer: `value' = f(value, position, optional spatial interpolator)`.
- An **attribute map** is a map attribute → *entry* `(modifier, argument)` (§5).

## 2. Attribute types (registry `minecraft:attribute_type`)

Registered ids (all in the `minecraft` namespace) with value kind, JSON value codec, modifier library, and the four
interpolation behaviours. Interpolation function names: **lerp** = `p0 + α·(p1 − p0)` in f32; **step(t)** = `α ≥ t ? to : from`;
**degrees(m)** = `d = wrapDegrees(to − from)`; `|d| ≥ m ? to : from + α·d`; **srgb** = component-wise f32 lerp on the
float vector (despite the name there is no gamma conversion); **lerpInt** = `p0 + floor(α·(p1 − p0))` (f32 product,
floor to int); **cross-fade** (lists, §2.1).

| type id | value | JSON value | modifier library (besides `override`) | keyframe | state-change | spatial | partial-tick |
|---|---|---|---|---|---|---|---|
| `boolean` | bool | JSON boolean | and, nand, or, nor, xor, xnor | step(1.0) | step(0.0) | step(0.5) | step(0.0) |
| `tri_state` | TriState | bool, or string `"default"` (also `"true"`/`"false"` strings) | none | step(1) | step(0) | step(0.5) | step(0) |
| `float` | f32 | number | alpha_blend, add, subtract, multiply, minimum, maximum | lerp | lerp | lerp | lerp |
| `angle_degrees` | f32 | number | same as `float` | lerp | lerp | lerp | degrees(90) |
| `rgb_color` | 3 × f32 (r,g,b in 0..1) | color (§2.2) | alpha_blend, add, subtract, multiply, blend_to_gray | srgb | srgb | srgb | srgb |
| `argb_color` | 4 × f32 (r,g,b,a) | color (§2.2) | alpha_blend, add, subtract, multiply, blend_to_gray | srgb | srgb | srgb | srgb |
| `integer` | i32 | number | add, subtract, multiply, minimum, maximum | lerpInt | lerpInt | lerpInt | lerpInt |
| `moon_phase` | enum | string: `full_moon`, `waning_gibbous`, `third_quarter`, `waning_crescent`, `new_moon`, `waxing_crescent`, `first_quarter`, `waxing_gibbous` (indices 0..7 in this order) | none | step(1) | step(0) | step(0.5) | step(0) |
| `activity` | villager `Activity` | registry name of an `minecraft:activity` entry | none | step(1) | step(0) | step(0.5) | step(0) |
| `bed_rule` | BedRule | object (§2.3) | none | step(1) | step(0) | step(0.5) | step(0) |
| `particle` | particle options | the particle codec (see the particle spec) | none | step(1) | step(0) | step(0.5) | step(0) |
| `ambient_particles` | list of `{particle, probability}` | list of objects (§2.1) | append | cross-fade | cross-fade | cross-fade | cross-fade |
| `background_music` | BackgroundMusic | object (§2.4) | none | step(1) | step(0) | step(0.5) | step(0) |
| `ambient_sounds` | AmbientSounds | object (§2.5) | none | step(1) | step(0) | step(0.5) | step(0) |
| `mob_spawn_settings` | MobSpawnSettings | object `{spawns_by_category, spawn_costs}` (the biome spawn-settings codec) | overlay | step(1) | step(0) | step(0.5) | step(0) |

Notes:
- "state-change" is what the weather layer uses (§6.5). "spatial" is used by the client-only smooth biome blending
  (§6.2). "partial-tick" is client-only render interpolation. A step(0) function always returns `to`.
- `toFloat` is identity for `float` / `angle_degrees`; `toInt` is identity for `integer`; other types have neither
  (used only by the loot number providers, §9).
- The `Either` shape of a value: an attribute map entry (§5) accepts either the bare value (an **override**) or the
  full form.

### 2.1 Ambient particles
`{particle: <particle options>, probability: float in [0,1]}`. Spawning decision at runtime: spawn when
`random.nextFloat() ≤ probability`. Cross-fade of two lists with factor α: if α == 0.0 → `from`; if α == 1.0 → `to`;
else the concatenation of every `from` element with probability × (1 − α) followed by every `to` element with
probability × α (f32 products). `append(subject, argument)`: if the argument is empty → subject; else if the subject
is empty → argument; else subject followed by argument.

### 2.2 Color value codecs
- `rgb_color` value: string `"#RRGGBB"` (exactly 6 hex digits, leading `#`; opaque), **or** an integer interpreted as a
  packed `0xRRGGBB` (alpha ignored), **or** a list of 3 floats `[r,g,b]`. The channel float is `byte / 255.0f`.
- `argb_color` value: string `"#AARRGGBB"` (exactly 8 digits), **or** an integer (packed ARGB), **or** a list of 4
  floats `[r,g,b,a]`.
- Encoding writes the hex string form; floats are converted to bytes by `floor(v × 255.0f)` (truncating, not
  rounding).
- Hex-string errors: missing `#` ("must begin with #"), wrong digit count, non-hex, out-of-range.
- Color **arguments** of modifiers use the same codecs (RGB-vector for `add`/`subtract`/`multiply` on RGB, ARGB-vector
  where §4 says so).

### 2.3 `BedRule`
`{can_sleep: Rule, can_set_spawn: Rule, destroy_on_use: bool = false, destroy_on_leave: bool = false,
error_message: text component (optional)}`, where **Rule** ∈ {`always`, `when_dark`, `never`}:
- `always` → true; `never` → false; `when_dark` → `level.isDarkOutside()` (§8.1).
Built-in named instances used as defaults (not data): *can-sleep-when-dark* (`can_sleep = when_dark`,
`can_set_spawn = always`, no destroy flags, error message translation `block.minecraft.bed.no_sleep`);
*destroy-on-use* (`never`, `never`, `destroy_on_use = true`, no message); *destroy-on-leave* (`when_dark`, `never`,
`destroy_on_leave = true`, message `block.minecraft.bed.no_sleep`).

### 2.4 `BackgroundMusic`
`{default: Music?, creative: Music?, underwater: Music?}` (all optional). Selection for a listener: if underwater
and an underwater entry exists → it; else if creative mode and a creative entry exists → it; else the default entry.
Client-only.

### 2.5 `AmbientSounds`
`{loop: sound event?, mood: {sound, tick_delay: int, block_search_extent: int, offset: double}?,
additions: compact list of {sound, tick_chance: double}}` (`additions` default empty; a single object may stand for
a one-element list). Client-only. A built-in "legacy cave" mood: sound `ambient.cave`, tick_delay 6000,
block_search_extent 8, offset 2.0.

## 3. The attribute registry (26.3)

Every id is in the `minecraft` namespace. "Range" is the value range the attribute enforces: values are **validated
on decode** (decode error if outside) and **sanitized (clamped) at resolution time**. Flags: **S** = syncable (sent
to the client in the dimension-type / biome / timeline network forms), **P** = positional (default; `N` = not
positional: resolved with no position), **I** = spatially interpolated (client blends biome values by Gaussian
weights; the server ignores this), **F** = full-resolution biome (positional layer asks the exact per-block biome
instead of the 4×4×4 noise biome). Unlisted flags are off. "unit" = [0, 1]; "unit-ε" = [0, 0.9999999f]; "≥0" =
[0, +∞).

| id | type | default | range | flags |
|---|---|---|---|---|
| `visual/fog_color` | rgb_color | (0,0,0) | any | S I |
| `visual/fog_start_distance` | float | 0.0 | any | S I |
| `visual/fog_end_distance` | float | 1024.0 | ≥0 | S I |
| `visual/sky_fog_end_distance` | float | 512.0 | ≥0 | S I |
| `visual/cloud_fog_end_distance` | float | 2048.0 | ≥0 | S I |
| `visual/water_fog_color` | rgb_color | `0x050533` = (5,5,51)/255 | any | S I |
| `visual/water_fog_start_distance` | float | −8.0 | any | S I |
| `visual/water_fog_end_distance` | float | 96.0 | ≥0 | S I |
| `visual/sky_color` | rgb_color | (0,0,0) | any | S I |
| `visual/sunrise_sunset_color` | argb_color | (0,0,0,0) | any | S I |
| `visual/cloud_color` | argb_color | (0,0,0,0) | any | S I |
| `visual/cloud_height` | float | 192.33 | any | S I |
| `visual/sun_angle` | angle_degrees | 0.0 | any | S I |
| `visual/moon_angle` | angle_degrees | 0.0 | any | S I |
| `visual/star_angle` | angle_degrees | 0.0 | any | S I |
| `visual/moon_phase` | moon_phase | `full_moon` | – | S |
| `visual/star_brightness` | float | 0.0 | unit | S I |
| `visual/block_light_tint` | rgb_color | `0xFFD88C` = (255,216,140)/255 | any | S I |
| `visual/sky_light_color` | rgb_color | (1,1,1) | any | S I |
| `visual/sky_light_factor` | float | 1.0 | unit | S I |
| `visual/night_vision_color` | rgb_color | `0x999999` = (153,153,153)/255 | any | S I |
| `visual/ambient_light_color` | rgb_color | (0,0,0) | any | S I |
| `visual/default_dripstone_particle` | particle | `dripping_dripstone_water` | – | S |
| `visual/ambient_particles` | ambient_particles | `[]` | – | S |
| `audio/background_music` | background_music | empty (no music) | – | S |
| `audio/music_volume` | float | 1.0 | unit | S |
| `audio/ambient_sounds` | ambient_sounds | empty | – | S |
| `audio/firefly_bush_sounds` | boolean | false | – | S |
| `gameplay/sky_light_level` | float | 15.0 | [0, 15] | S **N** |
| `gameplay/can_start_raid` | boolean | true | – | – |
| `gameplay/water_evaporates` | boolean | false | – | S |
| `gameplay/bed_rule` | bed_rule | *can-sleep-when-dark* | – | – |
| `gameplay/straw_bed_rule` | bed_rule | *destroy-on-leave* | – | – |
| `gameplay/respawn_anchor_works` | boolean | false | – | – |
| `gameplay/nether_portal_spawns_piglin` | boolean | false | – | – |
| `gameplay/fast_lava` | boolean | false | – | S **N** |
| `gameplay/increased_fire_burnout` | boolean | false | – | – |
| `gameplay/eyeblossom_open` | tri_state | `default` | – | – |
| `gameplay/turtle_egg_hatch_chance` | float | 0.002 | unit | – |
| `gameplay/piglins_zombify` | boolean | true | – | S |
| `gameplay/snow_golem_melts` | boolean | false | – | – |
| `gameplay/creaking_active` | boolean | false | – | S |
| `gameplay/surface_slime_spawn_chance` | float | 0.0 | unit | – |
| `gameplay/cat_waking_up_gift_chance` | float | 0.0 | unit | – |
| `gameplay/bees_stay_in_hive` | boolean | false | – | – |
| `gameplay/monsters_burn` | boolean | false | – | – |
| `gameplay/can_pillager_patrol_spawn` | boolean | true | – | – |
| `gameplay/natural_mob_spawns` | mob_spawn_settings | empty settings | – | **F** |
| `gameplay/creature_world_gen_spawn_probability` | float | 0.1 | unit-ε | – |
| `gameplay/villager_activity` | activity | `idle` | – | – |
| `gameplay/baby_villager_activity` | activity | `idle` | – | – |

That is **51** attributes. The registry order (used for network ids) is the order of this table. A float default is
the f32 nearest the decimal shown (e.g. 192.33f, 0.002f, 0.1f).

Quirks:
- `visual/moon_phase` and `gameplay/sky_light_level`'s range apply only to the types shown. `AttributeRange` for
  floats: valid iff `min ≤ v ≤ max` (NaN is invalid); sanitize: in-range values unchanged, otherwise
  `v < min ? min : min(v, max)` (so NaN stays NaN).
- A non-positional attribute (`N`) cannot appear in a biome's attribute map (§5.3).
- The attribute id string is also the JSON key in attribute maps and timeline `tracks`.

## 4. Modifiers (operations)

Operation ids (codec `minecraft`-less strings): `override, alpha_blend, add, subtract, multiply, blend_to_gray,
minimum, maximum, and, nand, or, nor, xor, xnor, append, overlay`. A modifier's **argument** has a codec and a
**keyframe interpolation** (how timeline keyframe arguments are blended). `override` is always allowed for every
attribute; other operations only if in the attribute type's library (§2); an unknown or disallowed operation is a
decode error.

### 4.1 Boolean (`s` = current, `a` = argument)
`and: a ∧ s`; `nand: ¬a ∨ ¬s`; `or: a ∨ s`; `nor: ¬a ∧ ¬s`; `xor: a ≠ s`; `xnor: a = s`. Argument: JSON boolean;
keyframe interpolation: step(1.0) (so a boolean track flips exactly at the keyframe tick).

### 4.2 Float
- `add: s + a`, `subtract: s − a`, `multiply: s × a`, `minimum: Math.min(s, a)`, `maximum: Math.max(s, a)` (Java
  semantics: NaN propagates; `−0.0 < +0.0`). Argument: number; keyframe: lerp.
- `alpha_blend`: argument is `FloatWithAlpha{value, alpha}`; result = `lerp(alpha, s, value)` = `s + alpha·(value − s)`.
  Codec: a bare number (= `{value: n, alpha: 1.0}`), or an object `{value: float, alpha: float in [0,1] = 1.0}`;
  encoding uses the bare form when alpha == 1.0. Keyframe interpolation lerps both `value` and `alpha`.
- Integer type: `add`, `subtract`, `multiply` (wrapping), `minimum`, `maximum`; argument int; keyframe lerpInt.

### 4.3 Colors (`RGB` = `rgb_color`, `ARGB` = `argb_color`)
- `alpha_blend` argument is an **ARGB** color (4 floats), keyframe srgb lerp.
  - On RGB: if `a.w == 0.0` → s; otherwise `s + (a.rgb − s)·a.w` per channel (a JOML-style lerp; see Open question
    **[oracle]** on fused multiply-add).
  - On ARGB: if `a.w == 1.0` → `a`; if `a.w == 0.0` → s; otherwise
    `α = a.w + s.w·(1 − a.w)` and each rgb channel `(a.c·a.w + s.c·(α − a.w)) / α`; result alpha = α.
- `add` argument is an **RGB** color: result rgb channel = `min(s.c + a.c, 1.0)`; on ARGB the alpha is kept.
- `subtract` argument RGB: `max(s.c − a.c, 0.0)`; alpha kept on ARGB.
- `multiply`: on RGB the argument is RGB: component product. On ARGB the argument is ARGB: component product
  *including alpha*. (If either operand is exactly all-ones the other is returned unchanged; same result.)
- `blend_to_gray`: argument `{brightness: float [0,1], factor: float [0,1]}` (both required; keyframes lerp both).
  `grey = s.r·0.3f + s.g·0.59f + s.b·0.11f` (f32, left to right); `g = clamp(grey × brightness, 0, 1)` for each
  channel (alpha, on ARGB, is kept); result = componentwise `s + (g − s)·factor`.
- The default weather multipliers etc. in §6.5 use these.

### 4.4 `append` (ambient particles) — §2.1.
### 4.5 `overlay` (mob spawn settings) — `overlay(first = current, second = argument)`:
1. If `first` has no defined categories **and** no spawn costs → `second`.
2. Else if `second` has no defined categories and no spawn costs → `first`.
3. Else if `second`'s defined categories ⊇ `first`'s and `second`'s costed entity types ⊇ `first`'s → `second`.
4. Else build a new settings object: for each mob category in enum order, if `second` defines that category use
   *only* `second`'s weighted list for it, otherwise `first`'s list (if defined); then costs = `first`'s costs with
   `second`'s costs put over them. A category that is *defined but empty* in `second` therefore wipes `first`'s list.

## 5. Attribute maps

### 5.1 Codec
An attribute map is a JSON object whose keys are attribute ids (an unknown id is a decode error; a map-level dispatch)
and whose values are **entries**. An entry is decoded as `Either`: **first** the attribute's bare value codec
(= an `override` entry; value validated against the range), **else** the full form
`{"modifier": <operation id>, "argument": <argument codec of that operation for this attribute>}`.
Encoding writes an override entry as the bare value, anything else in full form. Order caution: because the bare form is
tried first, an object value that also happens to satisfy the value codec wins (only possible for object-valued
types with all-optional fields; those types have no non-override operations).
- Different codec entry points: **full** (`DimensionType` direct codec, biome direct codec positional-only),
  **network** (removes entries of non-syncable attributes both ways).
- Biome attribute maps use the **positional-only** validation: error "The following attributes cannot be positional: …"
  if any listed attribute is not positional (i.e. `gameplay/sky_light_level`, `gameplay/fast_lava`).
- An attribute map's `get(attribute)` returns the entry or none; `applyModifier(attribute, base)` returns
  `entry.modifier.apply(base, entry.argument)` or `base` if absent.

### 5.2 Builders
`modify(attribute, modifier, argument)` rejects a modifier not allowed for the attribute type; `set(attribute, v)` =
`modify` with override; `putAll` merges (later wins). Not behaviour-relevant for decoding.

## 6. Resolution

### 6.1 Building a level's attribute system
A **server level** builds its system once at construction (`addDefaultLayers`):
1. **Static layers**:
   a. For every attribute listed in the dimension type's `attributes`, a **constant layer** that applies that entry's
      modifier with that argument. (One layer per attribute, in the map's key order; at most one entry per
      attribute, so order within the dimension layer is immaterial.)
   b. For every attribute that **any biome in the biome registry** defines, one **positional layer** (§6.2). The set of
      such attributes is the distinct union over all registry biomes; layer exists even if only one biome defines it.
2. **Dynamic layers** (added after the static ones, so applied later):
   a. For each timeline in `DimensionType.timelines`, **in the holder-set order** (a tag's order, or the listed
      order), for each attribute that timeline has a track for, one **time-based layer** (the track sampler, §7.3).
   b. If the level can have weather (`canHaveWeather` = the dimension type `has_skylight` **and not** `has_ceiling`
      **and** the dimension is not the End), the **weather layers** (§6.5), one per attribute that rain or thunder
      touches, added after all timeline layers.

Per attribute, the layer list is the concatenation, in that order: dimension constant, biome positional, timeline
time-based layers (timeline by timeline), weather time-based. A world-generation region builds only the *static*
layers (no timelines, no weather).

**Baking:** per attribute, take the layer list; starting from the attribute **default**, fold the leading run of
**constant** layers into a single *base value* and drop them; the remaining layers keep their order. The attribute
is *position-affected* iff any remaining layer is positional. An attribute with no layers at all always resolves to
its default (and is clamped like any other value only if layers exist: with no sampler, the *unsanitized* default is
returned).

### 6.2 The biome layer (positional)
`applyPositional(value, pos, interpolator)`:
- If an interpolator is supplied **and** the attribute is spatially interpolated (flag I) → blend the biomes'
  entries by weight (client probe, §6.6; the server never supplies one).
- Otherwise find the biome and return `biome.attributes.applyModifier(attribute, value)`:
  - for **F** attributes (`natural_mob_spawns`): `BiomeManager.getBiome(floor(x), floor(y), floor(z))`, the
    fuzzy per-block biome lookup (§ worldgen/biomes-and-climate.md);
  - otherwise `BiomeManager.getNoiseBiomeAtPosition(x, y, z)`: noise biome at quart coordinates
    `floor(c) >> 2` for each axis (no fuzz).
  A biome with no entry for the attribute leaves the value unchanged.
Block-position queries use the **block centre** `(x + 0.5, y + 0.5, z + 0.5)`; entity queries use the entity's exact
double position. Thus a block-position query resolves the biome at `floor(x + 0.5) >> 2`, i.e. at the block itself
(`x` integer ⇒ the same block).

### 6.3 `getValue` / `getDimensionValue`
- `getValue(attribute, pos)`: if no sampler exists for the attribute (no layers) → the default. If the attribute is
  not position-affected → `getDimensionValue`. Else compute positionally (**not cached**): `v = base`; for each
  remaining layer in order apply it (constant: `f(v)`; time-based: `f(v, tickId)`; positional: `f(v, pos, interpolator)`);
  return `sanitize(v)`.
- `getDimensionValue(attribute)`: if no sampler → default; else if a cached value exists for this tick → it; else
  compute as above **skipping positional layers** (they leave `v` unchanged), sanitize, **cache** and return. (A
  positional attribute asked through `getDimensionValue` silently ignores its biome layer in the shipped game; a
  development flag makes it an error.)
- `getValue(lootContext, attribute)` (loot, §9): if the attribute is positional, use the context's `origin` parameter
  as the position (default value if the context has no origin); otherwise `getDimensionValue`.
- `getValue(attribute, blockPos)` = `getValue(attribute, Vec3.atCenterOf(blockPos))`.

### 6.4 Tick caches
- `invalidateTickCache()` clears every sampler's cached dimension value and **increments the sampler's tick id**.
- Called: at the **start of every `ServerLevel.tick`** (before weather and everything else), and for every level
  whenever a clock is modified (time set / added / paused / rate-changed / moved to a time marker), and on the client
  when a time/clock packet arrives or the client level ticks.
- Time-based layers cache their sampled argument per tick id: the argument is re-sampled from the clock only when the
  tick id differs from the cached one. Positional (non-cached) computations therefore still see a per-tick-stable
  time argument.
- Consequence: within one server tick the *dimension* value is frozen at whichever moment of the tick first asked for
  it; and the time-based **weather** layer itself is not cached but the dimension value built from it is. The sky
  darkening refresh (`Level.updateSkyBrightness`, §8.1) runs after the tick's weather step. **[oracle]** whether the
  first read in a tick can precede the weather step (see Open question 2).

### 6.5 The built-in weather layers
Hard-coded (not data). Two attribute maps, **RAIN** and **THUNDER**, each is an attribute map of entries:

| attribute | RAIN | THUNDER |
|---|---|---|
| `visual/sky_color` | `blend_to_gray{brightness 0.6, factor 0.75}` | `blend_to_gray{0.24, 0.94}` |
| `visual/fog_color` | `multiply` by (127,127,153)/255 | `multiply` by (63,63,76)/255 |
| `visual/cloud_color` | `blend_to_gray{0.24, 0.5}` | `blend_to_gray{0.095, 0.94}` |
| `gameplay/sky_light_level` | `alpha_blend{value 4.0, alpha 0.3125}` | `alpha_blend{4.0, 0.52734375}` |
| `visual/sky_light_color` | `alpha_blend` by ARGB (122,122,255, alpha 79)/255 | `alpha_blend` by (122,122,255, alpha 134)/255 |
| `visual/sky_light_factor` | `alpha_blend{0.24, 0.3125}` | `alpha_blend{0.24, 0.52734375}` |
| `visual/star_brightness` | `override` 0.0 | `override` 0.0 |
| `visual/sunrise_sunset_color` | `multiply` ARGB by (127,127,153,255)/255 | `multiply` by (63,63,76,255)/255 |
| `gameplay/bees_stay_in_hive` | `override` true | `override` true |

(The byte values are `floor(f × 255.0f)` of the float literals 0.5/0.6, 0.25/0.3, 0.48/1.0, 0.3125, 0.52734375.
`0.3125 = 5/16` and `0.52734375 = 135/256`.)

Per attribute in the union of the two maps, a time-based layer: let `T = level.getThunderLevel(1.0)` and
`R = level.getRainLevel(1.0) − T` (f32). `getRainLevel(1.0)` is `lerp(1.0, previousRain, rain)` and
`getThunderLevel(1.0)` is `lerp(1.0, previousThunder, thunder) × getRainLevel(1.0)` (all f32; note `lerp(1.0, a, b)` is
`a + 1.0·(b − a)`, which can differ from `b` by one ulp). Then:
1. if the RAIN entry exists and `R > 0`: `rainValue = RAINentry(value)`; `value = stateChangeLerp(R, value, rainValue)`;
2. if the THUNDER entry exists and `T > 0`: `thunderValue = THUNDERentry(value)`; `value = stateChangeLerp(T, value, thunderValue)`.
The `stateChangeLerp` of each type is in §2 (so `bees_stay_in_hive` becomes true as soon as rain level > 0; a float
attribute fades in proportionally).

This layer exists only when the level can have weather. It is evaluated on every access that includes time-based layers
(it has no own cache; the per-tick dimension cache of §6.4 hides that).

### 6.6 Client probe (informational)
The client resolves per-frame values: it samples the **6×6×6 Gaussian weights** of noise biomes around the camera
(positions `p × 0.25 − 0.5` split into integral and fractional parts; per axis weight for index `i` ∈ 0..5 is
`lerp(frac, K[i+1], K[i])` with `K = [0,1,4,6,4,1,0]`; sample coordinate `floor − 2 + i`; the weight of a cell is the
product of the three axis weights), accumulates `weight → biome attribute map` in a reference-keyed map (first-seen
order), and for each spatially-interpolated attribute combines them: with one source it applies it; otherwise it
applies each source's map to the *base value coming from earlier layers*, and folds the results left to right with
`spatialLerp(w_i / W_i, acc, v_i)` where `W_i` is the running weight sum and `w_i` the source weight (`float` ratio
from doubles). Not needed for server parity; included so a future client mirrors it.

## 7. Timelines

### 7.1 Registry `minecraft:timeline` and codec
```
{ "clock":        <world_clock registry id>,           (required)
  "period_ticks": int ≥ 1,                              (optional; absent = non-looping)
  "tracks":       { <attribute id>: <track>, ... },     (optional, default empty)
  "time_markers": { <marker id>: int≥0 | {"ticks": int≥0, "show_in_commands": bool=false}, ... } (optional) }
```
Track: `{"modifier": <operation id> = "override", "keyframes": [ {"ticks": int≥0, "value": <argument> }, ... ], "ease": <easing> = "linear"}`.
`<argument>` uses the argument codec of the track's modifier for that attribute (for `override`: the attribute's
value codec, range-validated).
Keyframe list rules (decode errors): non-empty; non-decreasing `ticks`; at most **three** consecutive entries share
one tick value (the check counts a run of equal ticks starting from the *last* keyframe's tick as initial state, so a
list whose first keyframe shares the last keyframe's tick counts that run too; the error message says "more than 2").
The network form drops tracks whose attribute is not syncable.
Validation when `period_ticks` is present: each time marker's ticks must be in `[0, period)`; each keyframe's ticks
must be in `[0, period]` (inclusive upper bound). Registry-level validation: a marker id must not be defined by two
timelines of the same clock.
Time markers are consumed by the clock/time commands and sleep ("wake up" marker) — see the clock spec; marker
semantics: `occursAt(total)` = (period absent ? `ticks == total` : `ticks == total mod period`); the next occurrence
is computed by `resolveTimeToMoveTo`.

### 7.2 Easing
`ease` is either a string naming one of 32 curves (`constant`, `linear`, `in_/out_/in_out_` × `back, bounce, circ,
cubic, elastic, expo, quad, quart, quint, sine`) or `{"cubic_bezier": [x1, y1, x2, y2]}` (4 floats, `x1`, `x2` in
[0,1]). Curve definitions and the Bézier Newton/bisection solver are in
[`math/java-math-helpers.md`](../math/java-math-helpers.md). `constant(x) = 0` for all x (so a `constant` track
holds the *from* value until the segment end). A symmetric Bézier `(a, b)` means controls `(a, b, 1−a, 1−b)`.

### 7.3 Sampling a track (the `AttributeTrackSampler`)
Baking (per attribute, per timeline): the track's keyframes become **segments**:
- One keyframe → one constant segment (`constant` easing, from = to = value, ticks 0..0).
- Otherwise, if the timeline has a `period_ticks = P`: segment 0 goes from `(last.ticks − P, last.value)` to
  `(first.ticks, first.value)`; then one segment per consecutive keyframe pair (keyframe i → i+1, using the track's
  easing); then a final segment `(last.ticks, last.value) → (first.ticks + P, first.value)`. Without a period: only
  the consecutive pairs. (The wrap segments always use the *track's* easing.)

Sampling at clock total ticks `t` (a signed 64-bit): `t' = floorMod(t, P)` if looping else `t`. Pick the **first**
segment with `t' < to.ticks`, else the **last** segment. Then:
1. if `t' ≤ from.ticks` → `from.value`;
2. else if `t' ≥ to.ticks` → `to.value`;
3. else `α = (f32)(t' − from.ticks) / (f32)(to.ticks − from.ticks)` (long difference converted to f32, divided in f32),
   `α' = easing(α)`, result = the argument keyframe-lerp `(α', from.value, to.value)`.
Zero-length segments (two keyframes on one tick) are skipped by the "first segment with `t' < to`" rule, so at that tick
the value is the *later* keyframe's: a step. Before the first keyframe of a non-looping track the value is the
first keyframe's, after the last keyframe it is the last keyframe's.

The layer's `applyTimeBased(v, tickId)`: if there is no cached argument or `tickId` differs from the cached tick id,
set the cached argument = sample at the clock's `totalTicks()` (long); then return `modifier.apply(v, argument)`.

### 7.4 What the vanilla timelines look like (structure only; read values from the jar)
The bundled data defines four timelines (`day`, `moon`, `villager_schedule`, `early_game`) bound to the overworld
clock: the day timeline loops every 24 000 ticks and carries tracks for sky/fog/cloud/light colors and factors,
the sun/moon/star angles, star brightness, sunrise-sunset color, sky light level, firefly sounds, eyeblossom
openness, creaking activity, turtle egg hatch chance, cat gift chance, bee and monster-burn flags; the moon timeline
loops every 8 × 24 000 ticks (moon phase and surface slime chance); the villager schedule loops every 24 000
ticks (activity tracks); the early-game timeline is non-looping (pillager patrols suppressed until a tick
threshold). Tracks mostly use multiplicative / max / or-style modifiers so that dimension and biome values are
modulated rather than replaced. Zenith must treat these as data (jar), not constants.

## 8. How gameplay code reads attributes (server side)

"Pos" = block centre unless stated "entity position". `dim` = `getDimensionValue`. Client-only readers are listed in
§8.3. The reading helper `getValue(attribute, pos)` is unambiguous.

### 8.1 Sky light and "dark outside"
- `skyDarken` (int) = truncating cast of `15.0f − dim(gameplay/sky_light_level)`. Refreshed at level construction and
  once per `ServerLevel.tick` after the sleep/weather block (see `ticks/server-tick.md` 6.5 and
  `lighting/chunk-steps.md`). `LevelReader.getSkyDarken()` returns it.
- `isBrightOutside()` = `!dimensionType.hasFixedTime && skyDarken < 4`; `isDarkOutside()` =
  `!hasFixedTime && !isBrightOutside()` (so a fixed-time dimension is neither). Used by bed `when_dark`, villager and
  mob goals (fox, drowned, restrict-sun, flee-sun, village siege, wandering trader, enderman).
- Phantoms: spawn check uses `skyDarken ≥ 5`.

### 8.2 Table of readers
| attribute | reader | rule |
|---|---|---|
| `gameplay/bed_rule`, `gameplay/straw_bed_rule` | `AbstractBedBlock.getBedRule(level, pos)` (`BedBlock` uses `bed_rule`, `StrawBedBlock` uses `straw_bed_rule`) | On use (server): if `destroy_on_use` → send the error message (overlay) to the player and run the block's destroy-on-use; else if occupied → try to wake a sleeping villager out, else "occupied" message; else start sleeping with the rule. Player sleeping checks `canSleep(level)`; `ServerPlayer` spawn-point setting checks `canSetSpawn(level)`. After waking, `destroy_on_leave` removes the bed. `Player` also re-checks `canSleep` for the bed under the player. |
| `gameplay/respawn_anchor_works` | `RespawnAnchorBlock.canSetSpawn` and anchor use | when false anchors explode instead of setting spawn |
| `gameplay/nether_portal_spawns_piglin` | `NetherPortalBlock.randomTick` | one gate of the portal piglin spawn (with `spawn monsters`, non-peaceful, `random.nextInt(2000) < difficulty id`, a player nearby) |
| `gameplay/fast_lava` | `LavaFluid` and `Entity` lava current | `dim` read. Lava slope-find distance 4 (else 2), drop-off 1 (else 2), tick delay 10 (else 30); entity lava push scale 0.007 (else 0.0023333333333333335) |
| `gameplay/increased_fire_burnout` | `FireBlock.tick` | the six neighbour burn-out chances lose 50 (300→250 for E/W/N/S, 250→200 for up/down), **and** every spread `odds` value in the 3×6×3 spread loop is halved (integer `/ 2`) — see `ticks/blocks/tick-fire.md` |
| `gameplay/water_evaporates` | `BucketItem` (placing water), `IceBlock` melt and destroy, `WetSpongeBlock.onPlace`, `PointedDripstoneBlock` (mud above) | water bucket evaporates (smoke particles, fizz sound); ice is removed instead of becoming water; wet sponge dries to sponge; mud no longer drips water |
| `gameplay/eyeblossom_open` | `EyeblossomBlock.tick` / `FlowerPotBlock.randomTick` | `shouldBeOpen = value.toBoolean(currentlyOpen)`; `default` keeps the current state |
| `gameplay/turtle_egg_hatch_chance` | `TurtleEggBlock` hatch step | `chance > 0 && random.nextFloat() < chance` |
| `gameplay/piglins_zombify` | `AbstractPiglin.isConverting`, `Hoglin.isConverting` | conversion only when the mob is not immune, not no-AI, and the value is true (entity position) |
| `gameplay/snow_golem_melts` | `SnowGolem.aiStep` | when true the golem takes 1 point of on-fire damage every tick |
| `gameplay/creaking_active` | `CreakingHeartBlock/BlockEntity` | heart state AWAKE vs DORMANT; protector removal when not active (and not persistent) |
| `gameplay/surface_slime_spawn_chance` | `Slime.checkSlimeSpawnRules` | in biomes tagged for surface slimes and `50 < y < 70`: `random.nextFloat() < chance` then a light check |
| `gameplay/cat_waking_up_gift_chance` | cat `CatLieOnBed` stop | when the owner slept ≥ 100 ticks: `random.nextFloat() < chance` → morning gift |
| `gameplay/bees_stay_in_hive` | `Bee.wantsToEnterHive` (entity pos), `BeehiveBlockEntity.releaseOccupant` | bees enter/stay unless the release is an emergency |
| `gameplay/monsters_burn` | `Mob.isSunBurnTick` (entity pos, server only) | sun-burn only when true (then light-level, rain, powder snow, `canSeeSky` rules apply) |
| `gameplay/can_pillager_patrol_spawn` | `PatrolSpawner` | gate for the spawn position |
| `gameplay/can_start_raid` | `Raids.createOrExtendRaid` | no raid when false |
| `gameplay/natural_mob_spawns` | `NaturalSpawner`, `ChunkGenerator.getMobsAt` | per-category weighted spawn lists and spawn costs at a position (full-resolution biome) |
| `gameplay/creature_world_gen_spawn_probability` | `NaturalSpawner.spawnMobsForChunkGeneration` | probability gate for worldgen animal spawning |
| `gameplay/villager_activity`, `gameplay/baby_villager_activity` | `Brain.updateActivityFromSchedule` | at most every 20 game ticks; scheduled activity = attribute at the entity position (idle if the brain has no schedule) |
| `visual/moon_phase` | `ServerLevel.getMoonBrightness(pos)` and the `moon_brightness` spawn-variant condition | brightness table by phase index `[1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75]` |
| `visual/sun_angle` | `DaylightDetectorBlock.updateSignalStrength` | sun angle in degrees × π/180 (f32) feeds the detector formula |
| `visual/cloud_color`, `visual/cloud_height` | `Entity.isInClouds` | in clouds iff cloud alpha ≠ 0 and the entity's box top reaches the cloud layer |
| `visual/default_dripstone_particle` | `PointedDripstoneBlock.getDripParticle` | used when there is no fluid above |
| `audio/firefly_bush_sounds` | `FireflyBushBlock.animateTick` | client ambient |
| `gameplay/sky_light_level` | `Level.updateSkyBrightness` | §8.1 |

### 8.3 Client-only readers (informational)
Fog (`fog_*`, `sky_*`, `water_fog_*`), sky rendering (`sky_color`, `sun/moon/star_angle`, `star_brightness`,
`sunrise_sunset_color`, `moon_phase`), the light map (`block_light_tint`, `sky_light_color`, `sky_light_factor`,
`ambient_light_color`, `night_vision_color`), clouds, music and ambient sounds, ambient particles
(`EnvironmentAttributeProbe`).

### 8.4 Loot tables
Loot conditions/number providers read attributes through §6.3's `getValue(lootContext, attribute)` — see
[`loot/number-providers.md`](loot/number-providers.md) and [`loot/conditions.md`](loot/conditions.md).

## Edge cases and known quirks

1. **No sampler ⇒ unsanitized default.** If neither the dimension, any biome, any timeline nor weather defines
   an attribute, the resolved value is exactly the registered default (never clamped).
2. **Positional attribute through `getDimensionValue`** skips biome layers (no error in the shipped game).
3. **Constant-prefix folding** changes nothing observable: the base is the default folded through the leading constants.
   A *biome* layer placed before any constant layer cannot occur (dimension layer is added first).
4. **Sanitize only at the end:** intermediate values may leave the range (e.g. a float `multiply` above 1 then a
   later `minimum`).
5. **Float `alpha_blend` uses `lerp(alpha, s, value)`** — at `alpha = 1` the result is `s + (value − s)`, which can
   differ from `value` by an ulp in f32.
6. **Keyframe boolean tracks** (`step(1.0)`) switch exactly at the keyframe tick; `constant`-eased float tracks hold
   the earlier value until `to.ticks` (the `α ≥ 1` branch returns `to.value` at the segment end).
7. **Looping tracks use `floorMod`** (negative clock times loop upwards). A track with one keyframe is constant even
   when the timeline loops.
8. **Moon phase is data driven**: `visual/moon_phase` comes from the moon timeline (8 phases of 24 000 ticks); the old
   day-counter arithmetic is not used by 26.3 code paths listed above.
9. **Per-tick cache vs commands:** `/time` etc. invalidate every level's cache immediately, so a command-driven change
   is visible in the same tick.
10. **Attribute maps with a weather/timeline layer for a non-syncable attribute** are fine on the server; only the
    network form filters.

## Constants and IDs

- Registry ids: `minecraft:environment_attribute` (51 entries, §3), `minecraft:attribute_type` (15 entries, §2, in
  the order of the table), `minecraft:timeline`, `minecraft:world_clock`.
- Operation ids: see §4. `moon_brightness_per_phase = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75]` (f32).
- Spatial interpolation kernel `[0, 1, 4, 6, 4, 1, 0]`, radius 2, 6 samples per axis.
- Weather constants §6.5. Greyscale weights `0.3f, 0.59f, 0.11f`.
- Hex color digit counts: RGB 6, ARGB 8.
- Time-marker keys are resource ids under the `clock_time_marker` root; period 24 000 ticks/day (data).

## Open questions

1. **[oracle] JOML lerp fusion.** The vector lerp used by `srgbLerp`, `alpha_blend` on RGB, and spatial blending comes
   from the JOML library (not in the decompiled source). It may compute `x + (y − x)·t` as a fused multiply-add. Probe
   `ARGB.srgbLerp(float, Vector3fc, Vector3fc)` / `ARGB.alphaBlend(Vector3fc, Vector4fc)` with random float vectors and
   compare fused vs unfused results bit by bit.
2. **[oracle] Cache staleness inside a tick.** Confirm whether any server code reads `gameplay/sky_light_level` (or
   another weather- or time-affected dimension attribute) between `invalidateTickCache` at the start of
   `ServerLevel.tick` and the weather step. Probe: set rain level to 1.0 mid-run and read `skyDarken` at tick N and N+1
   through `ServerLevel.environmentAttributes().getDimensionValue`.
3. **[oracle] Layer order across several timelines** that track the same attribute (`DimensionType.timelines` holder-set
   order for a tag or list). Probe: print `DimensionType.timelines()` iteration order for each vanilla dimension type.
4. `canHaveWeather` is `hasSkyLight && !hasCeiling && dimension != END`; which vanilla dimension types satisfy `has_skylight`/`has_ceiling` is jar data.
5. `Either` order of attribute entry decode for object-valued types (value form wins): confirmed by reading, not run.
6. Interplay of `Mth.sin`/`cos` table with `inSine/outSine` easing: covered by `math/java-math-helpers.md`.
7. Dimension-type and biome *data* (which attributes each sets) is not in this spec: read from the jar.

## Oracle probe points
`net.minecraft.world.attribute.EnvironmentAttributeSystem` (`getValue(EnvironmentAttribute, Vec3, SpatialAttributeInterpolator)`,
`getDimensionValue`, `invalidateTickCache`, package-private `getConstantBaseValue` / `isAffectedByPosition`),
`EnvironmentAttributeSystem.Builder.addDefaultLayers(Level)`,
`net.minecraft.world.attribute.WeatherAttributes.addBuiltinLayers`,
`net.minecraft.world.timeline.AttributeTrackSampler.applyTimeBased`,
`net.minecraft.util.KeyframeTrackSampler.sample(long)`, `net.minecraft.util.EasingType.apply(float)`,
`net.minecraft.util.ARGB.srgbLerp / alphaBlend / multiply / greyscale / scaleRGB`,
`net.minecraft.world.level.Level.updateSkyBrightness / getSkyDarken / getRainLevel / getThunderLevel`,
`net.minecraft.world.level.dimension.DimensionType.timelines()` / `attributes()`,
`net.minecraft.world.attribute.EnvironmentAttributeMap.CODEC` (round-trip of hand-written entries).
Parity suite sampling plan: for every attribute, a dimension (overworld/nether/end), 10 positions over varied biomes,
and clock ticks 0, 1, 11 999, 12 000, 13 000, 23 999, 24 000, 192 000 × rain/thunder levels {0, 0.3, 1}.

## Parity status
`environment-attributes`: not yet run.
