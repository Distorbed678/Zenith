# Codec: loot contexts, parameter sets, callers, slot sources and NBT / score providers (26.3)

- Verified by suites: `loot-tables`
- Sources: local reference reading of the 26.3 game (2026-11-26); [`../loot-tables.md`](../loot-tables.md). Written in
  our own words.

## Summary
A `LootParams` holds a typed **parameter map** built against a **parameter set** (`minecraft:context_key_set`): the
builder rejects parameters the set does not allow ("Parameters not allowed in this parameter set") and missing
required ones ("Missing required parameters") — these are programming errors in the game, so Zenith's callers must
pass exactly the right keys. A set also types the data: a table's `type` field names its set; the data validator
checks that every function/condition's *referenced parameters* are allowed by that set (logged problem only).

## 1. Parameters (registered keys, namespace `minecraft`)

| key | value type | meaning |
|---|---|---|
| `this_entity` | Entity | the entity the loot concerns (killed mob, interacting player, fishing hook, …) |
| `interacting_entity` | Entity | player/entity interacting (shearing, block interact, entity interact) |
| `target_entity` | Entity | the entity being targeted/interacted with |
| `last_damage_player` | Player | the player that last damaged the dead entity (present only if "player kill") |
| `damage_source` | DamageSource | |
| `attacking_entity` | Entity | `damage_source.getEntity()` (indirect attacker, e.g. the shooter) |
| `direct_attacking_entity` | Entity | `damage_source.getDirectEntity()` (e.g. the arrow) |
| `origin` | Vec3 | position of the event (block centre `x+.5,y+.5,z+.5` for block events; entity `position()` for entities) |
| `block_state` | BlockState | |
| `block_entity` | BlockEntity | |
| `tool` | ItemInstance | tool / item involved |
| `explosion_radius` | float | present only for explosions that use "destroy with decay" |
| `enchantment_level` | int | level of the enchantment whose effect runs (enchantment contexts) |
| `enchantment_active` | boolean | whether the enchantment is active |
| `additional_cost_component_allowed` | Unit | flag: trade-cost component may be added (villager trades) |
| `container` | SlotProvider | a container/slot provider for slot sources |

**Entity targets** (`entity` fields in conditions/functions): `this`→`this_entity`, `attacker`→`attacking_entity`,
`direct_attacker`→`direct_attacking_entity`, `attacking_player`→`last_damage_player`, `target_entity`,
`interacting_entity`. **Block-entity target:** `block_entity`. **Item target:** `tool`.
`LootContextArg` fields (in `copy_name`, `copy_components`, NBT/ slot providers) accept the relevant subset by these
names; the entity / block entity names share one namespace (`this`…`block_entity`), item adds `tool`, and
`range slot source` also accepts `container`.

## 2. Parameter sets (registered, `minecraft:*`)
`R` = required, `O` = optional. (31 sets; `generic` lists every parameter as required and is the default for tables.)

| id | parameters |
|---|---|
| `empty` | – |
| `generic` | all 16 as R (data-validation default only) |
| `chest` | origin R; this_entity O |
| `command` | origin R; this_entity O |
| `command_slot_source` | origin R; container R; this_entity O |
| `command_compute_default` | origin R; this_entity O |
| `command_compute_position` | origin R; block_state R; this_entity O; block_entity O |
| `command_compute_entity` | origin R; target_entity R; this_entity O |
| `selector` | origin R; this_entity R |
| `villager_trade` | origin R; this_entity R; additional_cost_component_allowed R |
| `fishing` | origin R; tool R; this_entity O |
| `entity` | this_entity R; origin R; damage_source R; attacking_entity O; direct_attacking_entity O; last_damage_player O |
| `equipment` | origin R; this_entity R |
| `archaeology` | origin R; this_entity R; tool R |
| `gift` | origin R; this_entity R |
| `barter` | this_entity R |
| `vault` | origin R; this_entity O; tool O |
| `advancement_reward` | this_entity R; origin R |
| `advancement_entity` | this_entity R; origin R |
| `advancement_location` | this_entity R; origin R; tool R; block_state R; block_entity O |
| `block_use` | this_entity R; origin R; block_state R |
| `block` | block_state R; origin R; tool R; this_entity O; block_entity O; explosion_radius O |
| `shearing` | origin R; this_entity R; tool R |
| `entity_interact` | target_entity R; interacting_entity O; tool R |
| `block_interact` | block_state R; origin R; block_entity O; interacting_entity O; tool O |
| `container_process` | block_entity R; block_state R; container R; origin R |
| `enchanted_damage` | this_entity R; enchantment_level R; origin R; damage_source R; direct_attacking_entity O; attacking_entity O |
| `enchanted_item` | tool R; enchantment_level R |
| `enchanted_location` | this_entity R; enchantment_level R; origin R; enchantment_active R |
| `enchanted_entity` | this_entity R; enchantment_level R; origin R |
| `hit_block` | this_entity R; enchantment_level R; origin R; block_state R |

Registration order is the order listed (`empty, generic, chest, command, command_slot_source, command_compute_default,
command_compute_position, command_compute_entity, selector, villager_trade, fishing, entity, equipment, archaeology,
gift, barter, vault, advancement_reward, advancement_entity, advancement_location, block_use, block, shearing,
entity_interact, block_interact, container_process, enchanted_damage, enchanted_item, enchanted_location,
enchanted_entity, hit_block`); the `type` field's decode is lenient: an unknown name falls back to `generic`.

## 3. Callers: what they pass (loot generation, server side)

| caller | table | set | parameters / luck / seed |
|---|---|---|---|
| mob death (`LivingEntity.dropFromLootTable`) | the entity type's death table | `entity` | this_entity; origin = entity position; damage_source; attacking_entity, direct_attacking_entity (optional); **only if player-kill**: last_damage_player and luck = that player's luck; seed = the mob's `DeathLootTableSeed` (0 normally ⇒ sequence/level random) |
| block break (`Block.getDrops(state, level, pos, blockEntity, breaker, tool)`) | block's `drops` table (the table key is a block property) | `block` | origin = block centre; tool; this_entity (optional breaker); block_entity (optional); block_state; **no luck** (0), no seed. Blocks with no drops table produce nothing |
| block drop without a tool (`Block.getDrops(state, level, pos, blockEntity)`) | | `block` | tool = empty stack |
| explosion destroying a block | block's table | `block` | origin; tool = empty; block_entity; this_entity = explosion's *direct* source; explosion_radius = the explosion radius **only for destroy-with-decay explosions** |
| chest / barrel / container open (`RandomizableContainer.unpackLootTable`) | container's `LootTable` | `chest` | origin = block centre; with a player: this_entity and luck = player luck; `fill` with the container's seed |
| chest vehicle | | `chest` | likewise with vehicle position |
| fishing (`FishingHook.retrieve`) | `gameplay/fishing` | `fishing` | origin = hook position; tool = the rod; this_entity = hook; luck = hook luck + player luck |
| archaeology (`BrushableBlockEntity`) | the block entity's table | `archaeology` | origin = block centre; this_entity = brusher; tool = brush; luck; the block entity's seed |
| vault ejection | vault config table | `vault` | origin; luck; this_entity = player; tool = inserted key |
| vault display item | config table | `vault` | origin only |
| mob equipment (`Mob.equip`) | equipment table | `equipment` | origin; this_entity |
| gifts, shearing, entity interaction, piglin barter, composter, … | respective tables | `gift`, `shearing`, `entity_interact`, `barter`, `block_interact` | per the sets in §2 |
| `/loot` command | per source | `entity`, `chest`, `fishing`, `command*`, `block` | per command |

"Block tables" for each block and "entity tables" for each entity type are named in data/code; **which table a block
or entity uses** is part of each block/entity definition, not of this spec. The `randomSequence` of a table in the data
selects the stream (`loot-tables.md` §5).

## 4. Dynamic drops
`LootParams.withDynamicDrop(id, callback)`: one callback per id (duplicate registration is an error). The `dynamic`
entry looks up its `name` here. Which ids exist is decided by the registering block entities (data/code), not by this spec.

## 5. Slot sources (registry `minecraft:slot_source`)
Used by `slots` entries and command code. A **slot collection** is an ordered list of slot accessors plus stream
operations. Types (field `type`): `group, filtered, limit_slots, slot_range, contents, empty`.
- `empty` → no slots.
- `group {terms: [slot source…]}` (also decodable from a bare array): concatenation of the terms' collections in order.
- `filtered {slot_source, item_filter: item predicate}`: only slots whose item copy matches.
- `limit_slots {slot_source, limit: int ≥ 1}`: at most the first `limit` slots (nested limits take the minimum).
- `slot_range {source: <entity target>|block_entity|container = container, slots: <slot range name>}`: if the context
  object is a slot provider, its slots in that named range (e.g. `container.*`, `weapon.mainhand`), else none.
- `contents {slot_source, component: container|bundle_contents|charged_projectiles}`: for every item of the inner
  collection, the item's contents slots (a flat map: the concatenation of each item's content slots).
The `slots` loot entry emits `itemCopies()` filtered to non-empty stacks, in order (a copy; the source slots are not
modified). Slot-range names are the shared slot-name table (commands spec).

## 6. NBT providers (registry `loot_nbt_provider_type`)
Slot `source`/`provider` in `copy_custom_data`: a bare string = `context` target, or `{type, …}`:
- `context {target: <entity target>|block_entity}`: for an entity, the entity's "tag to compare" (its full saved data
  minus a few transient fields, as used by NBT predicates); for a block entity, its saved data **with full metadata**
  (id, position, components). The tag is a snapshot (not a live view); none → `null`.
- `storage {source: id}`: the whole command-storage compound for that id (empty compound if absent).

## 7. Score-holder providers (registry `loot_score_provider_type`)
Slot `target` of `score` int provider: a bare entity-target string = `context`, or `{type: "context", target}` or
`{type: "fixed", name: string}` (a name-only score holder). `context` yields the target entity (a `ScoreHolder`) or
nothing when the entity param is missing.

## Edge cases and known quirks
1. Block drops use luck 0 unless a code path sets luck; **fortune/looting are enchantment levels on `tool`/attacker**,
   read through `apply_bonus`, `table_bonus`, `enchanted_count_increase`, `random_chance_with_enchanted_bonus`.
2. `last_damage_player` and non-zero luck are set **together and only for player kills**; a mob killed by a wolf of a
   player but not "player killed" has neither.
3. `attacking_entity` is the *owner/shooter* (indirect), `direct_attacking_entity` the projectile.
4. The `tool` of a block break by explosion is the **empty stack** (not null): `match_tool` true/false depends on its
   predicate; `apply_bonus` finds level 0.
5. A table's declared `type` only matters for validation; callers choose which param set they build with.
6. The `chest` set has no `tool`; a table using `match_tool` there would never match (always false).
7. Missing optional parameters leave the corresponding conditions false (`loot/conditions.md` quirk 3).

## Constants and IDs
Parameter ids and set ids above (all in `minecraft:`). Default `type`: `generic`.

## Open questions
- **[oracle]** the full list of vanilla tables' `random_sequence` values and `type` sets (data; read from jar).
- **[oracle]** the exact entity "tag to compare" field set (`NbtPredicate.getEntityTagToCompare`) for `copy_custom_data`
  from entities.
- Slot-range names and slot-provider behaviour belong in the commands/inventory spec.

## Oracle probe points
`LootContextParamSets`, `LootContextParams`, `LootParams.Builder.create`, `LootContext.EntityTarget`,
`ContextMap.Builder.buildAndValidate`, `SlotSources`, `ContextNbtProvider.get`.

## Parity status
`loot-tables`: not yet run.
