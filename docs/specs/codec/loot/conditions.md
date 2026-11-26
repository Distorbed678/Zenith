# Codec: loot conditions (predicates) (26.3)

- Verified by suites: `loot-tables`
- Sources: local reference reading of the 26.3 game (2026-11-26); `docs/specs/math/random.md`; written in our own
  words. Entity / item / block / location / damage-source *predicate objects* are a separate (missing) spec — see Open
  questions.

## Summary
A **condition** is a boolean function of the loot context (`test(context)`). Registry `minecraft:predicate` stores
named conditions (data, e.g. `tool/can_silk_touch`); a condition slot accepts an id or an inline object
`{"condition": "<type>", …}` (type registry `minecraft:loot_condition_type`). Conditions appear: on tables' pools and
entries (`condition`), on functions (`condition`), inside number providers (`conditional`, `number_dispatcher`) and as
checks inside other conditions. There is **no** `reference` condition; naming a registry id in a slot is the reference.
Several conditions on one element are written with `all_of`.

Conditions may consume random numbers: `random_chance`, `random_chance_with_enchanted_bonus`, `table_bonus`,
`survives_explosion` (when a radius exists). They are evaluated lazily and in the order the algorithm in
`loot-tables.md` calls them, so draw order is observable.

## Types (registered order)
`inverted, any_of, all_of, random_chance, random_chance_with_enchanted_bonus, entity_properties, killed_by_player,
entity_scores, match_block, match_tool, table_bonus, survives_explosion, damage_source_properties, location_check,
weather_check, time_check, int_value_check, float_value_check, enchantment_active_check, environment_attribute_check`
(20).

### `inverted`
`{term: condition}` → `!term(ctx)`.

### `all_of` / `any_of`
`{terms: [condition…]}` (a holder set: list, single element, or `#tag` of the predicate registry). Evaluated in list
order with **short-circuit**: `all_of` stops at the first false (empty list → true), `any_of` at the first true (empty
list → false). Later terms' draws do not happen after the short circuit.

### `random_chance`
`{chance: float provider}`. `p = chance.getFloat(ctx)` (evaluated first; non-finite → 0.0), then **always** one
`nextFloat()`; true iff `nextFloat() < p`. So `p ≤ 0` is false (but a draw is consumed) and `p ≥ 1` is true (a draw is
consumed; `nextFloat() < 1` always).

### `random_chance_with_enchanted_bonus`
`{unenchanted_chance: float in [0,1], enchanted_chance: LevelBasedValue, enchantment: enchantment holder}`.
`killer = attacking_entity`. If `killer` is a living entity, `level = best enchantment level of enchantment on killer`
(the maximum over the killer's equipment slots that the enchantment's `slots` apply to, taking each item's `enchantments`
component level; 0 if none; see `loot/functions.md` §Enchantment level lookup), else 0. `chance = level > 0 ?
enchanted_chance.calculate(level) : unenchanted_chance` (f32). Then **one** `nextFloat()`; true iff `< chance`.
(No short circuit: the draw always happens.) Referenced parameter: `attacking_entity`.

### `entity_properties`
`{entity: this|attacker|direct_attacker|attacking_player|target_entity|interacting_entity, predicate: entity predicate?}`.
`e = ctx[entity param]` (may be null), `pos = ctx[origin]`. If `predicate` is absent → **true** (even if `e` is
null). Otherwise `predicate.matches(level, pos, e)` (entity-predicate spec; a null entity does not match).

### `killed_by_player`
No fields; true iff the context has the `last_damage_player` parameter.

### `entity_scores`
`{entity: <target>, scores: {<objective name>: int range}}`. False if the entity parameter is missing. For every
objective entry (map iteration order: the decoded map's order): the objective must exist and the entity must have a
score in it, and the score must satisfy the int range (`IntRangePredicate`, number-providers.md §5); all must hold.

### `match_block`
Fields = a block predicate (blocks / tag, `state` property predicates, `nbt`), decoded inline in the same object.
True iff the context has `block_state`, the predicate matches that state, and (if the predicate constrains block
entity data) matches the context's `block_entity`.

### `match_tool`
`{predicate: item predicate?}`. `tool = ctx[tool]`: false if absent; true if `predicate` absent; else
`predicate.test(tool)`. (Tools may be `ItemInstance`s such as an entity's held stack.)

### `table_bonus`
`{enchantment, chances: [float] (non-empty)}`. `level` = level of `enchantment` in the **tool's** `enchantments`
component (0 if no tool). `chance = chances[min(level, chances.size − 1)]`. One `nextFloat() < chance`. Always draws.

### `survives_explosion`
No fields. If `explosion_radius` is absent → true, **no draw**. Else `p = 1.0f / radius` (f32; radius 0 → +∞ → true),
true iff `nextFloat() <= p`. (Note `<=`, unlike `random_chance`.)

### `damage_source_properties`
`{predicate: damage-source predicate?}`. False unless **both** `origin` and `damage_source` exist; then true if
`predicate` absent else `predicate.matches(level, origin, source)`.

### `location_check`
`{predicate: location predicate?, offsetX: int = 0, offsetY: int = 0, offsetZ: int = 0}` (the offset fields live in the
same object). False if `origin` absent; true if `predicate` absent; else
`predicate.matches(level, origin.x + offsetX, origin.y + offsetY, origin.z + offsetZ)` (doubles).

### `weather_check`
`{raining: bool?, thundering: bool?}`. `level.isRaining()` is `canHaveWeather && rainLevel(1.0) > 0.2`;
`level.isThundering()` is `canHaveWeather && thunderLevel(1.0) > 0.9` (levels as `environment-attributes.md` §6.5).
Result: (raining absent or equals `isRaining`) and (thundering absent or equals `isThundering`).

### `time_check`
`{clock: world clock id, period: long?, value: int range}`. `t = clock.totalTicks` (a long; the server clock manager,
`environment-attributes.md` §1); if `period` present, `t = t % period` (Java long remainder, sign of dividend; period 0
crashes); then `value.test(ctx, (int)t)` — **narrowing cast of the long to int** (wraps for huge tick counts).

### `int_value_check` / `float_value_check`
`{value: int/float provider, test: range}` where the range is a `IntRangePredicate` / `FloatRangePredicate`
(number-providers.md §5). Evaluation order: if the range has **no bounds at all**, the `value` provider is **not
evaluated** (so no draws) and the result is true; for a bounded range or a point: `value.getInt/getFloat` first, then
the bound providers (min then max).

### `enchantment_active_check`
`{active: bool}`. True iff the context has `enchantment_active` and it equals `active`.

### `environment_attribute_check`
`{attribute: attribute id, value: <value of that attribute>}`. Reads `getValue(ctx, attribute)`
(`environment-attributes.md` §6.3; position = the `origin` parameter for positional attributes, no origin ⇒ default
value) and compares with the given value using **exact equality of the typed value** (floats by `Float.equals`: NaN equals
NaN, `+0.0 ≠ −0.0`; vectors componentwise bit equality; records and enums by value). The JSON `value` uses the
attribute's value codec (range-validated).

## Edge cases and known quirks
1. `random_chance(p)` always draws once even when `p` is constant 0 or 1; but `survives_explosion` without a radius does
   not draw.
2. `any_of` / `all_of` short-circuit, so adding a trailing term changes draws only if earlier terms don't decide.
3. A condition that references a parameter the table's set doesn't allow is a *validation problem* (logged); at run time
   the condition just sees "absent": `match_tool` → false, `killed_by_player` → false, `location_check` → false,
   `damage_source_properties` → false, `entity_properties` with predicate → depends on predicate (null entity),
   `survives_explosion` → true, `table_bonus` → level 0.
4. `entity_properties` with no predicate is a no-op "true" — even for `entity: "attacker"` with no attacker.
5. `time_check` uses the clock's `totalTicks`, **not** day time of a particular dimension; clocks are world-level
   (`environment-attributes.md`).
6. Registered named predicates the game looks up by id: `tool/can_silk_touch` (match_tool: tool has `silk_touch` at
   level ≥ 1 in its `enchantments` component), `tool/can_shear` (match_tool: item is shears), `block/fast_cooking`
   (match_block: block is smoker or blast furnace). Contents are data; this describes their meaning.

## Open questions
- Entity/Item/Block/Location/DamageSource predicate specs (advancement criteria shapes) are needed to implement
  **Now specified** in `codec/predicates/` (2026-11-26): `entity.md`, `item-and-components.md`, `block-fluid-location.md`,
  `damage-and-misc.md`, `bounds-and-basics.md`.
  `entity_properties`, `match_tool`, `match_block`, `location_check`, `damage_source_properties`, and `item_filter`s.
- **[oracle]** `EnchantmentHelper.getEnchantmentLevel(Holder<Enchantment>, LivingEntity)` slot iteration for entities
  whose `getSlotItems` includes body/saddle slots (the lookup uses only slots matching the enchantment's `slots` field).
- **[oracle]** Equality semantics of JOML vectors in `environment_attribute_check` (`Vector3f.equals`).

## Oracle probe points
`LootItemConditionTypes`, `LootItemRandomChanceCondition.test`, `LootItemRandomChanceWithEnchantedBonusCondition.test`,
`BonusLevelTableCondition.test`, `ExplosionCondition.test`, `TimeCheck.test`, `WeatherCheck.test`,
`IntValueCheck.test`, `EnvironmentAttributeCheck.test`.

## Parity status
`loot-tables`: not yet run.
