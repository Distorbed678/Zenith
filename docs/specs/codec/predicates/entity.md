# Predicates: entity predicate and its 25 sub-predicates (26.3)

- Verified by suites: `predicates`
- Sources: local reference reading of the 26.3 game (2026-11-26); `bounds-and-basics.md`, `block-fluid-location.md`,
  `item-and-components.md`, `damage-and-misc.md`, `docs/specs/codec/loot/conditions.md` (`entity_properties`),
  `docs/specs/registry/identifiers-registries-tags.md`; usage patterns in the user's jar data (read only). Our own
  words. Index and hook catalogue: [README.md](README.md).

## Summary

An **entity predicate** is a JSON object whose keys are ids of the registry `minecraft:entity_sub_predicate_type`
(25 entries) and whose values are the schema of that sub-predicate. It holds iff the entity is non-null and **all**
present sub-predicates hold. There is no `type` / `type_specific` wrapper: type-specific checks are ordinary keys named
`minecraft:type_specific/<kind>`. Evaluation needs a *server level* and an optional *reference position* (a `Vec3`,
"the player position" in advancements and the `origin` loot parameter in loot); nested predicates (vehicle, passenger,
targeted entity, looking-at, lightning struck entities) are evaluated with the same reference position.

Entity state reads are listed as **hooks `E.*`** (README §4); this spec says exactly which are read, in which order
and with which comparisons, so the entity milestone can supply them.

## Inputs and state

- `level`: the server level; `ref`: nullable `Vec3`; `entity`: nullable.
- Hooks: see §7 (the full list with the quantity each returns).

## 1. Decode and combination

### 1.1 JSON

`{ "<sub-predicate id>": <value>, … }`. Keys are resolved by the registry name codec (so `entity_type` and
`minecraft:entity_type` are the same key; the namespace defaults to `minecraft`). An unknown key fails the decode (no
ignoring of unknown keys here: this is a dispatched map, `datapacks-and-codecs.md` §7.4). The empty object `{}` is
valid. Duplicate keys: last wins at the JSON layer. Encode writes the map back (key order unspecified).

### 1.2 Evaluation

`matches(level, ref, entity)`:
- `entity == null` ⇒ **false** (even for `{}`).
- empty map ⇒ true.
- one entry ⇒ that sub-predicate.
- several ⇒ conjunction over the entries sorted **stably** by this rank (ties keep decode order): `entity_type`: −1;
  `vehicle`, `passenger`, `targeted_entity`, `type_specific/lightning`, `type_specific/player`: 1; `nbt`: 2; every
  other key: 0. Short-circuit at the first false. All sub-predicates are pure, so the order is **unobservable**; it is
  stated so the cheap checks run first.

Sub-predicate signature: `matches(entity, level, ref)`.

## 2. Registry `minecraft:entity_sub_predicate_type` (raw ids)

| id | Key | id | Key |
|---|---|---|---|
| 0 | `minecraft:entity_type` | 13 | `minecraft:targeted_entity` |
| 1 | `minecraft:location` | 14 | `minecraft:team` |
| 2 | `minecraft:stepping_on` | 15 | `minecraft:slots` |
| 3 | `minecraft:movement_affected_by` | 16 | `minecraft:components` |
| 4 | `minecraft:distance` | 17 | `minecraft:predicates` |
| 5 | `minecraft:movement` | 18 | `minecraft:entity_tags` |
| 6 | `minecraft:effects` | 19 | `minecraft:type_specific/lightning` |
| 7 | `minecraft:nbt` | 20 | `minecraft:type_specific/fishing_hook` |
| 8 | `minecraft:flags` | 21 | `minecraft:type_specific/player` |
| 9 | `minecraft:equipment` | 22 | `minecraft:type_specific/cube_mob` |
| 10 | `minecraft:periodic_tick` | 23 | `minecraft:type_specific/raider` |
| 11 | `minecraft:vehicle` | 24 | `minecraft:type_specific/sheep` |
| 12 | `minecraft:passenger` | | |

(Ids are registration order; the registry is not network-synchronised in a way that exposes these ids.)

## 3. Generic sub-predicates (ids 0–18)

Each is `matches(entity, level, ref)`.

### 3.1 `entity_type`
Value: an entity-type holder set (`#tag`, id, id array). True iff `E.typeHolder ∈ set`.

### 3.2 `location`
Value: a location predicate (`block-fluid-location.md` §4). Evaluated at `(E.x, E.y, E.z)` (the entity position; not the
eye, not the block).

### 3.3 `stepping_on`
Value: a location predicate. `E.onGround` false ⇒ false. Otherwise `p = center(E.onPos())` = the supporting block
`E.onPos()` (§7.4 with offset `1.0e-5f`) plus `(0.5, 0.5, 0.5)`, and the location predicate is evaluated at `p`
(so `position.y` bounds are compared against `blockY + 0.5`).

### 3.4 `movement_affected_by`
Value: a location predicate evaluated at `center(E.blockPosBelowThatAffectsMyMovement())` = `E.onPos(0.500001f)` plus
`(0.5,0.5,0.5)`. No `onGround` requirement.

### 3.5 `distance`
Value: a distance predicate (`bounds-and-basics.md` §2). `ref == null` ⇒ false. Else
`distance.matches(ref.x, ref.y, ref.z, E.x, E.y, E.z)`.

### 3.6 `movement`
Value: object, all `Doubles = ANY`: `x`, `y`, `z`, `speed`, `horizontal_speed`, `vertical_speed`, `fall_distance`.
Let `v = E.knownMovement × 20.0` (componentwise `f64` multiply; blocks per second) and `fd = E.fallDistance` (`f64`).
Holds iff, in order: `x.matches(v.x)`, `y.matches(v.y)`, `z.matches(v.z)` (signed); `speed.matchesSqr(vx*vx + vy*vy + vz*vz)`
(`f64`, left to right); `horizontal_speed.matchesSqr(vx*vx + vz*vz)`; `vertical_speed.matches(|vy|)`;
`fall_distance.matches(fd)`.

### 3.7 `effects`
Value: the mob-effects predicate *itself* (a map `effect id → {amplifier, duration, ambient, visible}`;
`item-and-components.md` §3.3). Entity must be a **living entity** (else false). For each map entry (map order):
`instance = E.activeEffects[effect]`; the instance predicate must hold for it — an absent instance fails. An empty map ⇒
true for living entities, false otherwise. Instance predicate: `amplifier.matches(inst.amplifier)`,
`duration.matches(inst.duration)` (infinite = −1), `ambient`, `visible` (equalities if present).

### 3.8 `nbt`
Value: an NBT predicate. Holds iff the NBT contains-compare holds against the entity comparison tag (§7.5;
`bounds-and-basics.md` §3.3).

### 3.9 `flags`
Value: object with optional bools `is_on_ground`, `is_on_fire`, `is_sneaking`, `is_sprinting`, `is_swimming`, `is_flying`,
`is_baby`, `is_in_water`, `is_fall_flying`. Evaluate in this order; first mismatch ⇒ false:
1. `is_on_ground` == `E.onGround`.
2. `is_on_fire` == `E.isOnFire`.
3. `is_sneaking` == `E.isCrouching` (the *crouching pose*, not the sneak-key flag).
4. `is_sprinting` == `E.isSprinting`.
5. `is_swimming` == `E.isSwimming`.
6. `is_flying`: the entity's value is `E` is living **and** (`E.isFallFlying` **or** (`E` is a player and its abilities
   `flying` is true)); must equal the given bool. For a non-living entity the value is false (so `is_flying: false`
   holds for it, `true` fails).
7. `is_in_water` == `E.isInWater`.
8. `is_fall_flying` and `is_baby`, jointly: if `is_fall_flying` is present **and** `E` is living **and**
   `E.isFallFlying` differs ⇒ false. Otherwise, if `is_baby` is present **and** `E` is living: it must equal
   `E.isBaby`. **For a non-living entity both constraints are ignored (treated as satisfied)**, unlike `is_flying`.
   (`is_fall_flying` is checked before `is_baby`.)

### 3.10 `equipment`
Value: object of optional item predicates `head`, `chest`, `legs`, `feet`, `body`, `mainhand`, `offhand`. **Entity must
be living or the result is false** — also for `{}`. Check order: head, chest, legs, feet, body, mainhand, offhand; each
present predicate is applied to `E.itemBySlot(slot)` (the empty stack when nothing is equipped).

### 3.11 `periodic_tick`
Value: a positive integer `n` (`≥ 1`, else "Value must be positive"). Holds iff `E.tickCount % n == 0` (Java `int`
remainder; `tickCount ≥ 0`).

### 3.12 `vehicle`
Value: an entity predicate. `E.vehicle` null ⇒ false; else `value.matches(level, ref, E.vehicle)`.

### 3.13 `passenger`
Value: an entity predicate. True iff **any** entry of `E.passengers` (direct passengers, list order) matches (an empty
passenger list ⇒ false).

### 3.14 `targeted_entity`
Value: an entity predicate. The target is `E.target` if `E` is a **mob** (has an AI attack target), else null; the nested
predicate sees a null entity as non-match, so non-mobs and mobs without target ⇒ false.

### 3.15 `team`
Value: a string. True iff `E.team` is non-null and its name equals the string (case-sensitive).

### 3.16 `slots`
Value: a map `slot-range name → item predicate`. For each entry (map order): the entry holds iff **some** slot id in the
range's id list (in the list's order) is a slot the entity has (`E.getSlot(id) != null`) and the item predicate holds
for that slot's current stack. All entries must hold; empty map ⇒ true. The slot range names and their id lists are in
§8. An unknown range name fails the decode (the name codec enumerates only §8's names).

### 3.17 `components`
Value: the exact-component map (`item-and-components.md` §2.1). Holds iff `DataComponentExactPredicate.test(E)`: for each
`(type, value)` the entity's `get(type)` (§7.6) is non-null and equal to `value`. Empty map ⇒ true.

### 3.18 `predicates`
Value: the partial-predicate map (`item-and-components.md` §2.2). Holds iff every predicate holds against the entity as
a component getter (§7.6). Empty map ⇒ true. (Keys that are not predicate types become any-value checks on entity
components such as `minecraft:cat/variant`.)

### 3.19 `entity_tags`
Value: object `{any_of?: [string], all_of?: [string], none_of?: [string]}` against `E.entityTags` (the scoreboard tags,
a set of strings, i.e. the `Tags` list). Order: `any_of` present ⇒ at least one listed tag must be in the set (an empty
list fails); `none_of` present ⇒ none of the listed tags may be in the set; `all_of` present ⇒ every listed tag must be
in the set. Absent keys are skipped.

## 4. Type-specific sub-predicates (ids 19–24)

### 4.1 `type_specific/lightning`
`{blocks_set_on_fire: Ints = ANY, entity_struck?: entity predicate}`. `E` must be a **lightning bolt**. Holds iff
`blocks_set_on_fire.matches(bolt.blocksSetOnFire)` **and** (no `entity_struck`, or some entity among
`bolt.hitEntities` that is **still alive** matches `entity_struck` with the same `ref`). `hitEntities` is the list of
entities the bolt damaged/struck, in the order recorded.

### 4.2 `type_specific/fishing_hook`
`{in_open_water?: bool}`. If absent ⇒ **true for every entity** (the entity type is not checked). Otherwise `E` must be a
fishing hook and `hook.isOpenWaterFishing == value`.

### 4.3 `type_specific/player`
§5.

### 4.4 `type_specific/cube_mob`
`{size: Ints = ANY}`. `E` must be a **cube mob** (slimes, magma cubes, and the other "cube" mobs deriving from the same
base) else false; `size.matches(E.size)` where `size` is the synched size value (slime size 1, 2, 4 …; **not** the saved
`Size` NBT, which is `size − 1`).

### 4.5 `type_specific/raider`
`{has_raid: bool = false, is_captain: bool = false}`. `E` must be a **raider** else false; holds iff
`raider.hasRaid() == has_raid && raider.isCaptain() == is_captain`. Note the defaults: `{}` means "a raider that is
**not** in a raid and **not** a captain". `hasRaid` = on a server level, has a current raid **or** a raid exists at the
raider's block position (`W.raidAt`); `isCaptain` = wears (head slot) the ominous banner (same components as the
reference banner, `ItemStack.matches`) **and** is the patrol leader.

### 4.6 `type_specific/sheep`
`{sheared?: bool}`. `E` must be a sheep else false; if present, `sheep.isSheared == value` (bit `0x10` of the wool data
byte).

## 5. Player sub-predicate (`type_specific/player`)

### 5.1 Schema

All keys optional:

| Key | Type | Default |
|---|---|---|
| `level` | `Ints` | ANY (compared with `experienceLevel`, an `int`) |
| `food` | `{level: Ints = ANY, saturation: Doubles = ANY}` | ANY/ANY |
| `gamemode` | **array** of game-mode names (`survival`, `creative`, `adventure`, `spectator`) | all four |
| `stats` | array of stat matchers (§5.2) | `[]` |
| `recipes` | map recipe id → bool | `{}` |
| `advancements` | map advancement id → (bool **or** map criterion name → bool) | `{}` |
| `looking_at` | entity predicate | absent |
| `input` | `{forward?, backward?, left?, right?, jump?, sneak?, sprint?}` all bool | absent |

`gamemode` must be an array (a bare string is not accepted); `advancements` values are tried as bool first, then as the
criteria map. Keys of `recipes` / `advancements` are identifiers (recipe keys are not checked against any registry).

### 5.2 Stat matcher

`{type: <stat type id>, stat: <id in that type's registry>, value: Ints = ANY}` — a type-dispatched object where `type` is
resolved in `minecraft:stat_type` and `stat` is a **registry reference** of the registry that type counts:

| stat type | `stat` registry | |
|---|---|---|
| `minecraft:mined` | `minecraft:block` | |
| `minecraft:crafted`, `used`, `broken`, `picked_up`, `dropped` | `minecraft:item` | |
| `minecraft:killed`, `killed_by` | `minecraft:entity_type` | |
| `minecraft:custom` | `minecraft:custom_stat` (identifiers, e.g. `minecraft:jump`) | |

(Registration order: mined, crafted, used, broken, picked_up, dropped, killed, killed_by, custom.) Unknown stat id ⇒ decode
error. Holds iff `value.matches(playerStats.get(stat))` (an `int`, 0 when never set).

### 5.3 Evaluation order (`matches(entity, level, ref)`)

1. `E` is not a **server-side player** ⇒ false.
2. `level.matches(P.experienceLevel)`.
3. `food.level.matches(P.food.foodLevel)` then `food.saturation.matches(P.food.saturation)` (the saturation `float` widened
   to `double`).
4. `gamemode ∋ P.gameMode`.
5. each stat matcher, list order.
6. each `recipes` entry (map order): `P.recipeBook.contains(recipeKey) == value`.
7. `advancements`, only if non-empty: for each entry (map order): look the advancement up in the server advancement
   manager — **absent ⇒ false**; take `progress = P.advancements.getOrStartProgress(advancement)` (**hook with a side
   effect: it creates an empty progress entry if none exists**, `W.advProgress`); then
   - bool form: `progress.isDone == value`;
   - criteria form: for every `(criterion, expected)` (map order): the criterion must exist in the progress (**else
     false**) and `criterionProgress.isDone == expected`.
8. `looking_at` (§5.4).
9. `input`: `matches(P.lastClientInput)` — each present bool must equal the corresponding flag of the last input the
   client sent (`forward, backward, left, right, jump, shift(=sneak), sprint`).

### 5.4 `looking_at`

Requires `P`'s view. Let `from = P.eyePosition`, `dir = P.viewVector(1.0f)`, `to = from + dir × 100.0` (each component
`dir.c * 100.0 + from.c` in `f64`). Find the **nearest entity hit** by a ray from `from` to `to`:
- candidate entities: all entities except `P` itself that intersect the search box `AABB(from, to).inflate(1.0)` and are
  **not spectators**;
- for each candidate (iteration order of the level's entity lookup for that box) with box `bb = candidate.boundingBox`
  inflated by the margin `0.0f`: if `bb.clip(from, to)` hits at point `q`, `d = distanceSqr(from, q)`; the candidate
  becomes the current best iff `d < best` (strict; initial best = `f64::MAX`), so ties go to the first in iteration order.
- no hit ⇒ false. Then the hit entity must satisfy `looking_at.matches(level, P.position(), hit)` (the reference position is
  the **player's position**, not `ref`) **and** `P.hasLineOfSight(hit)`: both on the same level, centre-to-centre segment
  from `(P.x, P.eyeY, P.z)` to `(hit.x, hit.eyeY, hit.z)` of length ≤ 128.0 (longer ⇒ false), and a block raycast
  (collider shapes, no fluids) along it reports **no hit**.
Block occlusion is deliberately only checked by this second test; the first ray ignores blocks.

## 6. Where entity predicates are used

Loot `entity_properties` (`loot/conditions.md`), `damage_source_properties` (via damage-source predicates), advancement
trigger conditions (player/entity fields), equipment-based triggers, and nested inside the predicates above. Contextual
wrappers (`ContextAwarePredicate`) belong to the advancement spec; they ultimately call `matches(level, ref, entity)`.

## 7. Entity-state hooks (milestone "entity")

| Hook | Meaning / value |
|---|---|
| `E.typeHolder` | entity-type holder |
| `E.x,y,z` | position, `f64` |
| `E.onGround`, `E.isOnFire`, `E.isCrouching` (pose = crouching), `E.isSprinting`, `E.isSwimming`, `E.isInWater`, `E.isFallFlying`, `E.isBaby` (living), `E.abilities.flying` (player) | booleans as the entity defines them |
| `E.knownMovement` | if the entity's controlling passenger is a player and the entity is alive: that player's known movement; else the entity's delta movement (`Vec3`, blocks/tick) |
| `E.fallDistance` | `f64` field |
| `E.tickCount` | `i32` age in ticks |
| `E.entityTags` | scoreboard tag set |
| `E.team` | the scoreboard team (name) or null |
| `E.vehicle`, `E.passengers` | direct vehicle / ordered direct passengers |
| `E.target` | mob attack target |
| `E.activeEffects` | map effect → instance for living entities |
| `E.itemBySlot(slot)` | living equipment stack (`mainhand, offhand, feet, legs, chest, head, body, saddle`) |
| `E.getSlot(id)` | slot access by command-slot id, §8 |
| `E.comparisonTag` | §7.5 |
| `E.components` | §7.6 |
| `E.onPos(offset)` | §7.4 |
| Lightning: `blocksSetOnFire`, `hitEntities` | |
| Fishing hook: `openWater` | |
| Raider: `hasRaid`, `isCaptain` | |
| Sheep: `isSheared` | |
| Cube mob: `size` | |
| Player: `experienceLevel`, food data (`foodLevel` int, `saturation` float), `gameMode`, stats counter, recipe book (`contains`), advancement progress, `lastClientInput`, `eyePosition`, `viewVector(1.0)` | |

### 7.4 `E.onPos(offset)` (supporting block position)

The three offsets used: `1.0e-5f` (`onPos`), `0.500001f` (`blockPosBelowThatAffectsMyMovement`), `0.2f` (legacy; not used
here). Let `y' = floor(E.y − offset)` using the `f32` offset widened to `f64`.
- If the entity has a recorded **main supporting block position** `s`: if `offset ≤ 1.0e-5f` ⇒ `s`. Else let `B` = block at
  `s`: ⇒ `s` itself if (`offset ≤ 0.5` and `B ∈ #minecraft:fences`) or `B ∈ #minecraft:walls` or `B` is a fence gate;
  otherwise `s` with its Y replaced by `y'`.
- No supporting block: `(floor E.x, y', floor E.z)`.
The supporting-block bookkeeping is the entity movement milestone.

### 7.5 Entity comparison tag
The entity written as NBT **without** its `id`; for a player holding a non-empty selected item, additionally key
`SelectedItem` = that stack (`bounds-and-basics.md` §3.4). The save format is the entity milestone's.

### 7.6 Entities as component getters
`E.get(type)`: `custom_name` ⇒ the custom name component (nullable); `custom_data` ⇒ the entity's stored custom data
(`CustomData`); otherwise the entity *type's* default component map (usually empty) — then these type-specific
overrides apply **before** falling back to the base rule:

| Entity | `get` answers |
|---|---|
| area effect cloud | `potion_contents`, `potion_duration_scale` |
| salmon | `salmon/size` |
| cat | `cat/variant`, `cat/sound_variant`, `cat/collar` |
| llama | `llama/variant` |
| tropical fish | `tropical_fish/pattern`, `tropical_fish/base_color`, `tropical_fish/pattern_color` |
| chicken | `chicken/variant`, `chicken/sound_variant` |
| mooshroom | `mooshroom/variant` |
| fox | `fox/variant` |
| cow | `cow/variant`, `cow/sound_variant` |
| zombie nautilus | `zombie_nautilus/variant` |
| horse | `horse/variant` |
| parrot | `parrot/variant` |
| rabbit | `rabbit/variant` |
| frog | `frog/variant` |
| pig | `pig/variant`, `pig/sound_variant` |
| sheep | `sheep/color` |
| cushion | `cushion/color` |
| shulker | `shulker/color` |
| wolf | `wolf/variant`, `wolf/sound_variant`, `wolf/collar` |
| painting | `painting/variant` |
| mannequin | `profile` |
| axolotl | `axolotl/variant` |
| sulfur cube | `sulfur_cube_content` (derived from the body-armour stack) |
| villager, zombie villager | `villager/variant` (the type of its villager data) |

(Each answers with the entity's current value of that variant/colour field; the component ids are in
`text-and-data-components.md` B.7.) The apply-side (setting components when spawning from an item) is not used by
predicates.

## 8. Slot ranges (`minecraft:slots` keys, command slot names)

Name → ordered id list. `*` ranges list their ids in numeric order.

| Name(s) | Ids |
|---|---|
| `contents` | 0 |
| `container.0` … `container.53`; `container.*` | 0…53 |
| `hotbar.0` … `hotbar.8`; `hotbar.*` | 0…8 |
| `inventory.0` … `inventory.26`; `inventory.*` | 9…35 |
| `enderchest.0` … `enderchest.26`; `enderchest.*` | 200…226 |
| `mob.inventory.0` … `mob.inventory.7`; `mob.inventory.*` | 300…307 |
| `horse.0` … `horse.14`; `horse.*` | 500…514 |
| `weapon`, `weapon.mainhand` | 98 |
| `weapon.offhand` | 99 |
| `weapon.*` | [98, 99] |
| `armor.head` / `chest` / `legs` / `feet` / `body` | 103 / 102 / 101 / 100 / 105 |
| `armor.*` | [103, 102, 101, 100, 105] |
| `saddle` | 106 |
| `horse.chest`, `player.cursor` | 499 |
| `player.crafting.0` … `player.crafting.3`; `player.crafting.*` | 500…503 |

Slot ids by entity: **living entities**: 98 mainhand, 99 offhand, 100–103 feet, legs, chest, head, 105 body, 106 saddle.
**Player**: 499 = the carried (cursor) stack; 500–503 = crafting grid input slots; `0 ≤ id < 36` = inventory
(non-equipment items: 0–8 hotbar, 9–35 main); 200–226 = ender chest; then living-entity slots. **Horses/nautilus**: 500+ =
their inventory. **Chested horses**: 499 = a "chest" if carrying one. **Villagers, pillagers, piglins**: 300+ = inventory.
**Item entity, item frame, fireball, arrow, item display**: slot 0 = the held item. **Chest boats / container minecarts**:
chest slots. Unlisted ids ⇒ no slot (`getSlot` null).

## Edge cases and known quirks

1. `entity_type` etc. keys are plain registry ids with the `minecraft:` default; the type-specific ones contain a slash.
2. `{}` as an entity predicate is true for any non-null entity; as an `equipment` value it is **false** for non-living
   entities; as `type_specific/raider` it matches only raiders outside raids and non-captains.
3. `type_specific/fishing_hook: {}` matches **everything**.
4. `flags.is_baby` / `is_fall_flying` are ignored for non-living entities, but `is_flying` is evaluated (false).
5. `stepping_on` uses the block *centre*; bounds on `position.y` see `y + 0.5`.
6. `looking_at` ignores blocks in the ray and checks line of sight only afterwards; the nested predicate is evaluated
   against the **player's** position, not the outer `ref`.
7. The `advancements` check can create progress records for advancements the player has not started.
8. `periodic_tick` uses the entity's own age, so every entity spawned in the same tick shares phase only if spawned together.
9. Nested entity predicates `vehicle` / `passenger` / `targeted_entity` re-run the whole conjunction on the other entity.

## Constants and IDs

| Name | Value |
|---|---|
| Registry | `minecraft:entity_sub_predicate_type`, 25 entries (§2) |
| `looking_at` range | 100.0 blocks; search box inflate 1.0; ray margin 0.0; line of sight max 128.0 |
| `movement` velocity scale | ×20.0 |
| Supporting-block offsets | `1.0e-5f`, `0.500001f` |
| Stat types | `mined, crafted, used, broken, picked_up, dropped, killed, killed_by, custom` |
| Game modes | `survival, creative, adventure, spectator` |

## Open questions

1. **[oracle]** Whether `E.knownMovement` for a **player** returns server-side tracked movement (the last client-reported
   motion) rather than `deltaMovement`; probe the vanilla `MovementPredicate` on a walking player.
2. **[oracle]** Iteration order of the level's entity lookup used by `looking_at` ties (`Level.getEntities(except, box,
   predicate)`); relevant only when two candidates hit at the same squared distance.
3. **[oracle]** `Raid`/`captain` details (`Raider.isCaptain` banner comparison uses `ItemStack.matches` on the
   ominous-banner template: confirm components compared).
4. **[oracle]** The set of entity classes that count as `cube mob` and as `raider` (class hierarchy facts for the entity
   milestone): probe `AbstractCubeMob`/`Raider` subclasses via the entity type registry.
5. **[oracle]** `E.isOnFire` definition (fire ticks `> 0` or fire flag; includes lava?), `E.isCrouching` pose edge cases.
6. **[oracle]** Per-type `E.get(type)` answers: confirm one probe for each row of §7.6, including the value *types*
   (holder vs enum vs byte).
7. Entity-slot answers for every entity class in §8 (the entity milestone owns completeness).

## Oracle probe points

`EntityPredicate#matches`, `EntityPredicate$` combination (`#combine`), `EntitySubPredicates#bootstrap` (registration
order), `EntityTypePredicate#matches`, `EntityLocationPredicate#matches`, `SteppingOnPredicate#matches`,
`MovementAffectedByPredicate#matches`, `DistanceToPlayerPredicate#matches`, `MovementPredicate#matches`,
`EntityEffectsPredicate#matches`, `EntityNbtPredicate#matches`, `EntityFlagsPredicate#matches`,
`EntityEquipmentPredicate#matches`, `PeriodicEntityTickPredicate#matches`, `VehiclePredicate#matches`,
`PassengerPredicate#matches`, `TargetedEntityPredicate#matches`, `TeamPredicate#matches`, `EntitySlotsPredicate#matches`,
`SlotsPredicate#matches`, `EntityExactDataComponentsPredicate#matches`, `EntityPartialComponentsPredicate#matches`,
`EntityTagPredicate#matches`, `LightningBoltPredicate#matches`, `FishingHookPredicate#matches`, `PlayerPredicate#matches`,
`CubeMobPredicate#matches`, `RaiderPredicate#matches`, `SheepPredicate#matches`, `Entity#getOnPos(float)`,
`Entity#getKnownMovement`, `Entity#get(DataComponentType)`, `ProjectileUtil#getEntityHitResult(Level, Entity, Vec3, Vec3, AABB, Predicate, float)`,
`LivingEntity#hasLineOfSight(Entity)`, `SlotRanges`.

## Parity status

`predicates`: not yet run.
