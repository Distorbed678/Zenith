# Predicates: damage, damage source, mob effects, food, game type, input, slots (26.3)

- Verified by suites: `predicates`
- Sources: local reference reading of the 26.3 game (2026-11-26); `entity.md`, `item-and-components.md`,
  `bounds-and-basics.md`, `docs/specs/codec/loot/conditions.md` (`damage_source_properties`); usage patterns in the
  user's jar data (read only). Written in our own words. Index: [README.md](README.md).

## Summary

The remaining predicate objects that are not entity / item / block / location:

1. **Damage predicate** (advancement triggers on hurt events) — numbers of damage plus a damage-source predicate.
2. **Damage-source predicate** (also used directly by loot `damage_source_properties`) — damage-type tags and the
   direct / causing entities.
3. **Mob-effects predicate** — effect → amplifier / duration / flags.
4. **Food**, **game type**, **input** predicates (player sub-predicate building blocks).
5. **Slots predicate** — command-slot ranges mapped to item predicates.
6. **Enchantment predicate** and **tag predicate** cross-references.

## Inputs and state

- A **damage source** `S` exposes: `S.typeHolder` (a `minecraft:damage_type` holder, a data-driven registry),
  `S.directEntity` (the entity that physically caused the damage, e.g. an arrow; nullable), `S.causingEntity`
  (the "source entity": the responsible entity, e.g. the shooter; nullable). `S.isDirect` = **reference identity**
  `causingEntity == directEntity` — true when both are null, or the same entity.
- Hooks: `W.damageSource` (the game's `DamageSource`), `E.*` (`entity.md` §7).
- Entity predicates are evaluated by `entity.md` §1.2; the *reference position* passed down is stated per predicate below.

## 1. Damage predicate (advancement "damage" object)

(Not the item-component `damage` predicate of `item-and-components.md` §3.1.)

### 1.1 Schema (JSON)

All keys optional:

| Key | Type | Default |
|---|---|---|
| `dealt` | `Doubles` | ANY — the damage **before** armour/enchantment reductions |
| `taken` | `Doubles` | ANY — the damage actually applied |
| `source_entity` | entity predicate | absent |
| `blocked` | bool | absent — "the hit was blocked (shield)" |
| `type` | damage-source predicate (§2) | absent |

### 1.2 Evaluation `matches(player, source, originalDamage, actualDamage, blocked)`

In order, first failure ⇒ false:
1. `dealt.matches((f64) originalDamage)` (`f32` widened).
2. `taken.matches((f64) actualDamage)`.
3. `source_entity` present: `source_entity.matches(player.level, player.position, S.causingEntity)` — **the causing entity**,
   with the *player's position* as reference; a null causing entity is a non-match.
4. `blocked` present: must equal the `blocked` flag.
5. `type` present: `type.matches(player.level, player.position, S)`.
Used by the hurt-related advancement triggers (their specs).

## 2. Damage-source predicate

### 2.1 Schema (JSON)

| Key | Type | Default |
|---|---|---|
| `tags` | array of tag predicates | `[]` |
| `direct_entity` | entity predicate | absent |
| `source_entity` | entity predicate | absent |
| `is_direct` | bool | absent |

A **tag predicate** is `{id: damage-type holder set, expected: bool}` (`bounds-and-basics.md` §5): `id` is typically a
`#tag` (e.g. a damage-type tag like fire / projectile / explosion kinds) but may be an id or id array; both keys
required. Encode writes a one-element `id` back as the compact form.

### 2.2 Evaluation `matches(level, position, S)`

1. Every tag predicate (list order): `(S.typeHolder ∈ id) == expected`, else false.
2. `direct_entity` present: `direct_entity.matches(level, position, S.directEntity)` (null ⇒ non-match).
3. `source_entity` present: `source_entity.matches(level, position, S.causingEntity)`.
4. `is_direct` present: must equal `S.isDirect`.
The `position` is the caller's: the player position for triggers; the loot context `origin` for
`damage_source_properties` (loot spec: the condition is false unless both `origin` and `damage_source` exist; the
predicate is then evaluated with `level` = the loot level).
Because entity predicates return false for null, `source_entity: {}` on a source with no causing entity is **false**,
whereas omitting the key is true.

## 3. Mob-effects predicate

### 3.1 Schema

JSON: an object mapping a **mob-effect holder** (registry id, `minecraft:mob_effect`; references only, no inline) to an
**instance predicate**:

| Key | Type | Default |
|---|---|---|
| `amplifier` | `Ints` | ANY |
| `duration` | `Ints` | ANY (**infinite effects have duration `−1`**) |
| `ambient` | bool | absent |
| `visible` | bool | absent |

The map is decoded in key order. **Network** (defined, but not part of the data-component predicate path): VarInt count of
entries, each `(mob-effect registry id VarInt, instance)`, instance = `Ints` stream, `Ints` stream, `optional(bool)`,
`optional(bool)` (`bounds-and-basics.md` §1.4 for the `Ints` stream form).

### 3.2 Evaluation

- **Instance predicate** `matches(instance)`: null ⇒ false; `amplifier.matches(instance.amplifier)`;
  `duration.matches(instance.duration)`; `ambient == instance.isAmbient` (if present); `visible == instance.isVisible`
  (if present).
- **As an entity test** (entity sub-predicate `effects`): the entity must be **living**; for each map entry (any order),
  `instance = entity.activeEffects[effect]` and the instance predicate must hold for it (a missing effect fails); all
  entries must hold; an empty map is true.
- **As an element predicate** (potion `effects` collection): an effect instance `x` satisfies it iff the map has an entry
  keyed by `x.effect` and that entry's instance predicate holds for `x`.

## 4. Food, game-type and input predicates

- **Food:** `{level: Ints = ANY, saturation: Doubles = ANY}`. `matches(foodData)` = `level.matches(foodLevel)` then
  `saturation.matches((f64) saturationLevel)` (the saturation is a `float`).
- **Game type:** JSON **array** of mode names from `survival`, `creative`, `adventure`, `spectator` (no single-string
  shorthand). `matches(mode)` = list contains `mode`. The absent default is all four. (Order in the list is irrelevant;
  duplicates are allowed.)
- **Input:** `{forward?, backward?, left?, right?, jump?, sneak?, sprint?}` all optional bool. `matches(input)` = each
  present bool equals the corresponding flag, where `sneak` reads the input's **shift** flag. The input is the player's
  last received client input (`E.lastClientInput`).

## 5. Slots predicate

JSON: an object mapping a **slot-range name** to an **item predicate**. Names are the closed set of `entity.md` §8; an
unknown name fails the decode. `matches(provider)`:
for each entry (map order) the entry holds iff there exists a slot id `i` in the range's id list (list order) such that
`provider.getSlot(i)` is non-null **and** `itemPredicate.test(slot.get())`; every entry must hold; empty ⇒ true. Missing
slots (no such slot on this entity) are skipped, not errors. (Only entities provide slots in 26.3 callers.) The same
slot-name set is used by the command argument for slots (`/item`, `/loot` targets).

## 6. Cross-references

- **Enchantment predicate** `{enchantments?, levels = ANY}` — `item-and-components.md` §3.2 (it is only a building
  block of the `enchantments` / `stored_enchantments` predicates and triggers).
- **Tag predicate** — `bounds-and-basics.md` §5.
- **Collection predicate** — `bounds-and-basics.md` §6.
- **Item predicate** — `item-and-components.md` §1.

## Edge cases and known quirks

1. `damage.source_entity` uses the *causing* entity, while a damage-source predicate has **both** `direct_entity` and
   `source_entity` — in the latter, `source_entity` is also the causing entity.
2. `is_direct` is identity-based; two distinct entity objects are never "direct" even if they are the same owner.
3. A damage predicate with `blocked: false` fails when the hit was blocked and vice versa; absent skips the test.
4. The mob-effects predicate map is keyed by *holder*, so an effect id from another registry layer cannot appear;
   unknown ids fail the decode.
5. `gamemode` / `Game type` is an array in JSON; a string value is a decode error.
6. Infinite effect durations never satisfy a `duration` upper bound: `−1 ≤ any max`, but a `min ≥ 0` fails.

## Constants and IDs

| Name | Value |
|---|---|
| Damage-type registry | `minecraft:damage_type` (data-driven) |
| Infinite effect duration | −1 |
| Game mode names | `survival, creative, adventure, spectator` |
| Input flag names (JSON) | `forward, backward, left, right, jump, sneak, sprint` |

## Open questions

1. **[oracle]** The exact `DamageSource.isDirect` semantics for sources created with `null` direct and non-null causing
   entities (e.g. thorns, indirect magic): probe `DamageSource#isDirect` for the common damage types.
2. **[oracle]** The `blocked` flag value passed by each trigger and the `originalDamage` / `actualDamage` pair (trigger spec).
3. **[oracle]** Registry raw id of `minecraft:mob_effect` entries (only needed if the mob-effects network stream form is
   used; see `registry` specs).

## Oracle probe points

`DamagePredicate#matches` (advancements), `DamageSourcePredicate#matches`, `TagPredicate#matches`,
`MobEffectsPredicate#matches`, `MobEffectsPredicate#test`, `MobEffectsPredicate$MobEffectInstancePredicate#matches`,
`FoodPredicate#matches`, `GameTypePredicate#matches`, `InputPredicate#matches`, `SlotsPredicate#matches`,
`DamageSource#isDirect`.

## Parity status

`predicates`: not yet run.
