# Predicates: index, conventions and hook catalogue (26.3)

- Verified by suites: `predicates`
- Sources: local reference reading of the 26.3 game (2026-11-26) of the advancement-predicate and data-component-predicate
  classes and their callers; `docs/specs/codec/datapacks-and-codecs.md` §7, `docs/specs/codec/text-and-data-components.md`,
  `docs/specs/registry/identifiers-registries-tags.md`, `docs/specs/lighting/chunk-steps.md`. Everything below is
  written in our own words; data-pack data in the user's jar was read for usage patterns only (nothing is copied).

## Summary

"Predicates" are the declarative test objects shared by **loot conditions** (`entity_properties`, `match_tool`,
`match_block`, `location_check`, `damage_source_properties`, `item_filter`), **item components** (`can_place_on`,
`can_break`, `lock`), **advancement triggers**, **entity selectors / commands**, and **block-entity locks**. They split
into pure data tests (bounds, NBT, state properties, item/component matching) and *hooks into world / entity state*
(blocks, fluids, light, biomes, structures, entity flags…). This folder specifies every predicate schema (JSON and
network forms), default values, decode errors, evaluation order and the exact numeric comparisons; the hooks are the
interface to the world and entity milestones.

## 1. Files

| File | Contents |
|---|---|
| [bounds-and-basics.md](bounds-and-basics.md) | `MinMaxBounds` (Ints, Doubles, FloatDegrees; squared twins; command syntax), distance predicate, NBT predicate and the "contains" comparison, state-properties predicate (exact / ranged), tag predicate, collection predicate (`contains` / `count` / `size`) |
| [block-fluid-location.md](block-fluid-location.md) | block predicate (three call forms), fluid, light, location predicate (ordered checks), adventure-mode predicate (`can_place_on` / `can_break`, with its result-cache quirk), lock code (`lock`) |
| [item-and-components.md](item-and-components.md) | item predicate, data-component matcher block (exact `components` + partial `predicates`), the 15-entry `data_component_predicate_type` registry, enchantment predicate, network forms |
| [entity.md](entity.md) | entity predicate and the 25-entry `entity_sub_predicate_type` registry (location / stepping / movement / flags / equipment / slots / components / tags / vehicle / passenger / … / player / lightning / fishing hook / cube mob / raider / sheep), entity-state hooks, slot ranges |
| [damage-and-misc.md](damage-and-misc.md) | advancement damage predicate, damage-source predicate, mob-effects predicate, food / game type / input predicates, slots predicate |

## 2. Conventions used by every file

- **Decode rules** are those of `datapacks-and-codecs.md` §7: unknown keys are ignored in ordinary objects (the only
  dispatched maps — entity predicates and the two component-predicate maps — *do* reject unknown keys); `null` equals
  absent; an optional field that is present but invalid fails the whole object; numbers follow the JSON-to-int/float
  coercions of §7.2; holder sets follow registry spec §2.6.
- `Optional<T>` fields are written "key?" (absent = not checked). `X = ANY` means "default is the unconstrained bounds".
- "ANY" bounds, "point", "swapped": `bounds-and-basics.md` §1.
- **Network forms.** Predicates reach clients only inside item components: `can_place_on` / `can_break` (block predicates
  with a component-matcher block) and `lock` (NBT of the item-predicate codec); everything else is server-only data.
  Bounds, state-properties, NBT, holder sets and the matcher block have explicit stream forms (see each file).
  Component-predicate values travel as NBT-encoded JSON schemas (`item-and-components.md` §2.2).
- **Purity.** Every predicate is a pure function of its inputs and the hooks below, **except**
  `type_specific/player.advancements` (creates a progress entry for an advancement the player never started; see
  `entity.md` §5.3). Short-circuiting is therefore unobservable except through that and through hook cost.
- **Failure is `false`.** Missing world state (unloaded chunk, missing block entity, missing entity) makes the relevant
  test false; no predicate throws on bad world state. Decode failures are load errors of the owning file.

## 3. World-state hooks (`W.*`)

Provided by the world / chunk / block-entity milestones; each is read-only.

| Hook | Meaning |
|---|---|
| `W.loaded(pos)` | `pos` is within the level's valid bounds (inside the build height; chunk coordinate within the valid chunk range) **and** its chunk is loaded at status `FULL` (a chunk holder exists whose ticket level is at least the `FULL` level) |
| `W.blockState(pos)`, `W.fluidState(pos)` | state at `pos` (void air / empty fluid outside valid bounds) |
| `W.blockEntity(pos)` | the block entity at `pos` or null |
| `W.beFullTag(be)` | `saveWithFullMetadata`: the block entity's own saved data, then `components`, then `id`, `x`, `y`, `z` (registry-aware) |
| `W.beComponents(be)` | the block entity's stored components plus its implicit components (`collectComponents`) |
| `W.biome(pos)` | biome holder after the biome-zoom lookup (`worldgen/biomes-and-climate.md` §5) |
| `W.structurePiece(pos, set)` | a valid structure start in `pos`'s chunk references whose structure ∈ `set` and a piece box contains `pos` |
| `W.smokey(pos)` | campfire smoke scan (`block-fluid-location.md` §4.2 step 6) |
| `W.light(pos)` | `getMaxLocalRawBrightness` (`lighting/chunk-steps.md` §10) |
| `W.canSeeSky(pos)` | sky light ≥ 15 at `pos` |
| `W.dimension()` | the level's dimension key |
| `W.raidAt(pos)` | the active raid whose area contains `pos`, or none (raider predicate) |
| `W.advProgress(player, adv)` | get-or-create advancement progress (side effect) |
| `W.damageSource` | `DamageSource`: type holder, direct entity, causing entity |

## 4. Entity-state hooks (`E.*`)

Catalogued with exact quantities in `entity.md` §7 (position, flags, movement, tick count, tags, team, vehicle,
passengers, target, effects, equipment, slots, comparison tag, components, supporting-block position, type-specific
fields, player fields) and `entity.md` §8 (slot ids).

## 5. Callers (who evaluates what)

| Caller | Predicate | Reference position |
|---|---|---|
| loot `entity_properties` | entity predicate | loot `origin` (may be null) |
| loot `match_tool` / `item_filter` | item predicate | — |
| loot `match_block` | block predicate (state + block entity incl. components) | — |
| loot `location_check` | location predicate at `origin + offset` | — |
| loot `damage_source_properties` | damage-source predicate | loot `origin` |
| item components `can_place_on` / `can_break` | adventure-mode predicate (block form 2) | — |
| `lock` component / container locks | item predicate | — |
| advancement triggers | any of the above (entity predicates as contextual predicates; distance predicate in distance-type triggers) | player position |
| `/execute if items`, `/clear`, … | item predicate built by the command's component-predicate parser | — |

Contextual advancement wrappers and trigger-specific fields belong to the advancement spec, which calls into
`matches(...)` exactly as documented here.

## 6. Decode-error catalogue (what the owning file reports)

| Case | Where |
|---|---|
| bounds `min > max` (Ints, Doubles) | `bounds-and-basics.md` §1.2 |
| NBT predicate string is not SNBT or not a compound | `bounds-and-basics.md` §3.1 |
| unknown component id in `components` / transient component | `item-and-components.md` §2.1 |
| key in `predicates` that is neither a predicate type nor a component type | `item-and-components.md` §2.2 |
| empty `can_place_on` / `can_break` array | `block-fluid-location.md` §5.1 |
| unknown entity sub-predicate key | `entity.md` §1.1 |
| non-positive `periodic_tick` | `entity.md` §3.11 |
| `team`, `slots`, `stats` unknown names | `entity.md` §3.15–3.16, §5.2 |
| missing tag in a holder set | registry spec §2.6 |

## 7. Consolidated oracle list

Every `[oracle]` item is listed in the "Open questions" of its file; the probes are listed in each file's "Oracle probe
points". Distinct items: bounds-and-basics 4 · block-fluid-location 4 · item-and-components 5 · entity 6 ·
damage-and-misc 3 = **22** (a few are also flagged inline where the rule is stated). Additional non-oracle open
items: block-fluid-location 1, entity 1.

## 8. Known gaps and cross-spec updates needed

- `codec/loot/conditions.md` "Open questions" and `codec/text-and-data-components.md` open question 15 and the
  `can_place_on` / `can_break` / `lock` rows should now point here; the implementation should replace the raw maps with
  typed predicates per `block-fluid-location.md` §5–§6.
- The command-argument parsers for item/block predicates (`ItemPredicateArgument`, `BlockPredicateArgument`,
  `ComponentPredicateParser`) are command-spec territory; they build these same predicates. The **worldgen** block
  predicates (`minecraft:matching_blocks`, `solid`, `would_survive`, …) are a different family (`worldgen/features.md`).

## Parity status

`predicates`: not yet run.
