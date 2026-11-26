# Predicates: item predicate, data-component matchers and the data-component predicate registry (26.3)

- Verified by suites: `predicates`
- Sources: local reference reading of the 26.3 game (2026-11-26); `docs/specs/codec/text-and-data-components.md`
  (component values, item stacks, B.7 catalogue), `bounds-and-basics.md` (bounds, NBT, collections),
  `docs/specs/registry/identifiers-registries-tags.md` §2.6; usage patterns in the user's jar data (read only).
  Written in our own words. Index: [README.md](README.md).

## Summary

An **item predicate** tests an item *instance* (a stack or a stack template): item in a holder set, count in a range,
and a **data-component matcher block**. The matcher block has two parts: *exact* component values and *partial*
predicates drawn from a small registry (`minecraft:data_component_predicate_type`, 15 types). The same matcher block is
reused by block predicates (block entities) and by entity predicates (entity "components"). Item predicates appear in
loot (`match_tool`, `item_filter`), advancement triggers, entity equipment / slots, container-`lock`, commands, and as
element predicates inside container / bundle predicates.

## Inputs and state

- **Item instance** `I`: either an `ItemStack` or an `ItemStackTemplate` (a stack without runtime fields: item, count,
  component patch; used by `container` and `bundle_contents`). Reads: `typeHolder()`, `count()`, `get(componentType)`.
  - `ItemStack` that is *empty* (the empty singleton, item `air`, or count ≤ 0): type = **`minecraft:air`**, count = **0**,
    and `get(type)` is **always null** (its component map is empty — the air item's default components are *not*
    visible).
  - Non-empty stack: `count()` is the stack count; `get(type)` is the patched value (prototype + patch, removals
    applied).
  - Template: `typeHolder` = its item, `count` = its count (never 0), `get(type)` = patch value if set, null if the
    patch removes it, else the item prototype's value.
- **Component getter** `G`: anything exposing `get(type)`: stacks, templates, **block entities** (`W.beComponents`), and
  **entities** (`E.components`, table in `entity.md` §6).
- Holder-set fields follow `bounds-and-basics.md` Inputs; their registries are given per field.
- Equality of component values (exact matching, §2.1) is the game's value equality: `bounds-and-basics.md` §3.3 rule 5
  applies for numbers (Float/Double compare), lists are ordered, maps unordered, text components compare structurally
  (contents, siblings, style).

## 1. Item predicate

### 1.1 Schema (JSON, one object; no network form)

| Key | Type | Default |
|---|---|---|
| `items` | item holder set (`minecraft:item`) | absent |
| `count` | `Ints` bounds | ANY |
| `components` | exact matcher map (§2.1) | `{}` |
| `predicates` | partial matcher map (§2.2) | `{}` |

(`components` and `predicates` are the two matcher-block keys, living in the same object.)

### 1.2 Evaluation `test(I)`

1. `items` present and `I.typeHolder ∉ items` ⇒ false.
2. `count.matches(I.count())` fails ⇒ false.
3. matcher block (§2.3) against `I`.

The all-default predicate (the empty object, also the `NO_LOCK` lock) matches **every** instance including the empty
stack. `{"items": "minecraft:air"}` matches empty stacks (and real air). A `count` lower bound of 1 or more rejects the
empty stack.

## 2. Data-component matcher block (`DataComponentMatchers`)

Two keys, both optional with default `{}`:

### 2.1 `components` — exact matches

JSON: object mapping a **component type id** to a value decoded with that type's **persistent** codec. Rules:
- The key must name a registered data-component type that has a persistent form; a *transient* type fails the decode
  ("Encountered transient component …"); an unknown id fails. The value is decoded with the component's normal JSON
  schema (`text-and-data-components.md` B.7), so the same value-level errors apply.
- Decode keeps the entries in key order. Encode drops transient entries.
- **Network:** VarInt-prefixed list of `(data-component-type registry id VarInt, that component's network value)`.
  Unbounded count. Network values are the types' own stream forms (a type with no dedicated stream form travels as its
  NBT-encoded persistent form; `text-and-data-components.md` B.1).

Evaluation: for each expected `(type, value)` in order: `actual = G.get(type)`; the entry holds iff `actual` is
non-null and equal to `value` (value equality above); a missing component is a mismatch. An empty map is true.

### 2.2 `predicates` — partial predicates

JSON: object mapping a key to a predicate value. The **key** is resolved with `either(predicate-type id, component-type
id)`: the id is looked up in the `minecraft:data_component_predicate_type` registry **first**; only if absent there is
it looked up as a data-component type. Consequently:
- a key that is a registered predicate type (§3) selects that type's codec for the value;
- any other registered component type (persistent or not) is an **"any value" predicate**: the value is a unit —
  written `{}` (any JSON object decodes; **[oracle]** whether a non-object value is rejected), and it matches iff
  `G.get(type) != null`;
- **all 15 predicate-type ids of §3 are also data-component ids** (same name), and the predicate type wins: so
  "has a `minecraft:damage` component with any value" cannot be written with the any-value form for those 15 ids
  (writing `"minecraft:damage": {}` runs the damage predicate, which needs a `damage` component and ANY bounds).
- Unknown id in both registries ⇒ decode error. Keys are unique (JSON object).

Evaluation: every predicate must hold (`matches(G)`); order unobservable. Empty map ⇒ true.

**Network:** a list of entries, **at most 64** (more is a decode error), each `(bool, VarInt id, value)`:
`bool = true` then the *predicate-type* registry raw id; `bool = false` then the *data-component-type* registry raw id
(any-value predicates). The value is the **NBT encoding** of the type's JSON codec (registry-aware NBT ops, so holders
and holder sets are written as identifier strings, `#tag` strings or lists, bounds as ints or `{min,max}`, booleans as
bytes), written as a nameless root tag of whatever tag type the codec produces (the end tag is illegal), default NBT
quota (2,097,152 bytes, depth 512). Any-value predicates write an empty compound. Decoding failure disconnects. (A
potion-specific stream form exists in the game's source but is not used by this path.)

### 2.3 Combined test `test(G)`

Exact predicate first, then each partial predicate. `isEmpty` = both maps empty (used by block predicates to decide
whether a block entity is required).

## 3. Registry `minecraft:data_component_predicate_type` (raw ids in registration order)

| id | Type | Component read | JSON value |
|---|---|---|---|
| 0 | `minecraft:damage` | `damage`, `max_damage` | object `{durability?, damage?}` |
| 1 | `minecraft:enchantments` | `enchantments` | **array** of enchantment predicates |
| 2 | `minecraft:stored_enchantments` | `stored_enchantments` | **array** of enchantment predicates |
| 3 | `minecraft:potion_contents` | `potion_contents` | object `{potions?, effects?}` |
| 4 | `minecraft:custom_data` | `custom_data` | NBT predicate (SNBT string or object) |
| 5 | `minecraft:container` | `container` | object `{items?}` |
| 6 | `minecraft:bundle_contents` | `bundle_contents` | object `{items?}` |
| 7 | `minecraft:firework_explosion` | `firework_explosion` | object `{shape?, has_twinkle?, has_trail?}` |
| 8 | `minecraft:fireworks` | `fireworks` | object `{explosions?, flight_duration?}` |
| 9 | `minecraft:writable_book_content` | `writable_book_content` | object `{pages?}` |
| 10 | `minecraft:written_book_content` | `written_book_content` | object `{pages?, author?, title?, generation?, resolved?}` |
| 11 | `minecraft:attribute_modifiers` | `attribute_modifiers` | object `{modifiers?}` |
| 12 | `minecraft:trim` | `trim` | object `{material?, pattern?}` |
| 13 | `minecraft:jukebox_playable` | `jukebox_playable` | object `{song?}` |
| 14 | `minecraft:villager/variant` | `villager/variant` | villager-type holder set (the value *is* the set) |

(The registry is not defaulted; ids are assigned 0…14 in the order above. The component ids in the third column are the
data-component types of the same name.)

**"Single-component" rule** (every type except 0 and 4): the component is read with `G.get(component)`; **a null
component ⇒ false**; otherwise the type's value test below runs. For types 0 and 4 the exceptions are described in
their rows.

### 3.1 `damage`

`{durability: Ints = ANY, damage: Ints = ANY}`. Reads `damage = G.get(damage)`; **null ⇒ false** (items without the
component, e.g. non-damageable ones whose prototype has no `damage`, never match even with ANY bounds).
`maxDamage = G.getOrDefault(max_damage, 0)`. Holds iff `durability.matches(maxDamage − damage)` (i32 subtraction,
wrapping) **and then** `damage.matches(damage)`. No clamping (unlike the stack's own damage getter).
Not to be confused with the advancement `damage` predicate (`damage-and-misc.md` §1).

### 3.2 `enchantments`, `stored_enchantments`

Value: JSON array of **enchantment predicates** `{enchantments?: enchantment holder set, levels: Ints = ANY}`
(encode: array). The type holds iff **every** element `containedIn(E)` where `E` is the item's `ItemEnchantments`
(holder → level ≥ 1). An empty array ⇒ true for any `G` that has the component (e.g. `[]` matches any non-empty stack
whose component exists, which is every item via its default prototype; it does not match empty stacks or entities).

`containedIn(E)` for one element:
- `enchantments` present: true iff some holder `h` in the set has `E.level(h) = L ≠ 0` and `levels.matches(L)` (when
  `levels` is ANY this is just `L ≠ 0`). An empty set ⇒ false. (Set iteration order is irrelevant.)
- else `levels` constrained: true iff some entry of `E` has `levels.matches(entryLevel)`.
- else (neither): true iff `E` is non-empty.
(`ANY` is tested by object identity in the game, so a decoded explicit `"levels": {}` takes the second branch; the
result is identical because ANY matches every level, so the distinction is unobservable.)

### 3.3 `potion_contents`

`{potions?: potion holder set, effects?: collection predicate of effect predicates}`. Component value: optional potion
holder + custom effects (+ colour/name, unused here).
1. If `potions` present: the stack must have a potion **and** it must be in the set, else false. (A stack with only
   custom effects never satisfies a `potions` constraint.)
2. If `effects` present: `effects.test(allEffects)` where `allEffects` = the potion's effects (if a potion is present)
   **followed by** the custom effects, as a sequence of effect instances.
3. Otherwise true.

The element predicate of `effects` is the **mob-effects predicate** `{<effect id>: {amplifier: Ints = ANY, duration:
Ints = ANY, ambient?: bool, visible?: bool}, …}` (map of effect holders — registry references only). An *instance*
satisfies it iff the map has an entry for the instance's own effect and that entry's instance predicate holds:
`amplifier.matches(instance.amplifier)`, `duration.matches(instance.duration)` (**infinite duration is `−1`**),
`ambient == instance.ambient` (if present), `visible == instance.visible` (if present), in that order. An instance
whose effect has no entry fails. (`damage-and-misc.md` §3 reuses this predicate with the entity-effects form.)
Collection semantics: `bounds-and-basics.md` §6 (`contains` with several tests lets one instance satisfy several
tests).

### 3.4 `custom_data`

Value is an NBT predicate. **Exception to the null rule:** a missing `custom_data` component is treated as the *empty
compound*, so the predicate holds iff the expected compound is empty (`{}` matches every stack; anything with a key does
not match a stack without custom data). With the component: NBT contains-compare (`bounds-and-basics.md` §3.3).
Items and entities both expose this component.

### 3.5 `container`, `bundle_contents`

`{items?: collection predicate of item predicates}`. `items` absent ⇒ true (component must exist). Otherwise
`items.test(sequence)`:
- `container`: the sequence is the **non-empty** slots' stack templates in slot order (empty slots are skipped, so
  `size` counts only filled slots);
- `bundle_contents`: all stack templates of the bundle in order.
Each element is tested with the item predicate of §1 (count is the template's count).

### 3.6 `firework_explosion`

`{shape?: small_ball|large_ball|star|creeper|burst, has_twinkle?: bool, has_trail?: bool}` — the object *is* the value.
Holds iff each present key equals the explosion's `shape`, `hasTwinkle`, `hasTrail` (checked in that order).

### 3.7 `fireworks`

`{explosions?: collection predicate of §3.6 objects, flight_duration: Ints = ANY}`. `explosions` present and failing ⇒
false; then `flight_duration.matches(fireworks.flightDuration)`.

### 3.8 `writable_book_content`

`{pages?: collection predicate of strings}`. Element predicate: the page's **raw** text (the unfiltered string) equals the
given string exactly. `pages` absent ⇒ true.

### 3.9 `written_book_content`

`{pages?: collection predicate of text components, author?: string, title?: string, generation: Ints = ANY,
resolved?: bool}`. Order: `author == content.author`; `title == content.title.raw`; `generation.matches(content.generation)`;
`resolved == content.resolved`; `pages.test(content.pages)`; any failure ⇒ false. Page elements are text-component JSON
(`text-and-data-components.md` A.*); an element matches iff the page's **raw** component equals it structurally. A plain
string element is a literal component. **[oracle]** confirm that literal-vs-string component equality behaves as
described when the stored page was created from a different but equivalent form (e.g. `{"text":"x"}` vs `"x"`).

### 3.10 `attribute_modifiers`

`{modifiers?: collection predicate of entry predicates}`. Entry predicate:
`{attribute?: attribute holder set, id?: identifier, amount: Doubles = ANY, operation?: add_value | add_multiplied_base |
add_multiplied_total, slot?: any|mainhand|offhand|hand|feet|legs|chest|head|armor|body|saddle}`. Order: `attribute`
contains the entry's attribute holder; `id` equals the modifier id; `amount.matches(modifier.amount)`; `operation` equal;
`slot` equal to the entry's slot group **exactly** (a `hand` constraint matches only entries written with slot `hand`,
not `mainhand`). `modifiers` absent ⇒ true. The sequence is the component's entry list in order.

### 3.11 `trim`

`{material?: trim-material holder set, pattern?: trim-pattern holder set}` — both are **data-driven registries**
(`minecraft:trim_material`, `minecraft:trim_pattern`). Holds iff each present set contains the trim's material /
pattern holder (checked material first).

### 3.12 `jukebox_playable`

`{song?: jukebox-song holder set}` (`minecraft:jukebox_song`, data-driven). `song` absent ⇒ true. Otherwise true iff
there is a holder in the set whose **resource key** equals the resource key of the item's song holder. A song written
inline in the item (a direct holder with no key) never matches, and neither do direct holders in the set.

### 3.13 `villager/variant`

JSON value = a villager-type holder set (`minecraft:villager_type`: an id, an id array or `#tag`); holds iff the set
contains the component value (`Holder<VillagerType>`). Used with entity `predicates` for villagers and zombie villagers
(entity `get(villager/variant)` = the villager data's type).

## 4. Network summary (where item predicates travel)

| Where | Form |
|---|---|
| `can_place_on`, `can_break` components | VarInt list of block predicates (`block-fluid-location.md` §1.1); each block predicate carries a matcher block (§2) |
| `lock` component | NBT of the item-predicate JSON codec (§1.1) |
| Any matcher block (§2) | exact list + ≤ 64 partial entries as in §2.1/§2.2 |

Item, entity, and loot predicates are otherwise server-only data (no stream form).

## Edge cases and known quirks

1. An empty stack never carries components: `exact`/partial constraints on the empty stack all fail, `{}` constraints
   (`custom_data` aside) fail too because `G.get` is null.
2. `custom_data: {}`-style predicates are the only partial predicate that passes without the component.
3. `enchantments: []` is "has the component" — true for every real stack, false for entities/air.
4. `potion_contents.potions` requires a *potion*, not just effects; custom-effect-only potions fail it.
5. `container` ignores empty slots; `bundle_contents` never has them.
6. Attribute slot matching is exact group equality, not "applies to".
7. Predicate-type ids shadow same-named component ids in `predicates`, so `"minecraft:damage": {}` is the damage
   *predicate* (needs a `damage` component), not any-value.
8. `jukebox_playable.song` compares resource keys, so tag sets work and inline songs never match.
9. The component-less `custom_data` default makes `{}` expected NBT true everywhere but an expected non-empty compound false.

## Constants and IDs

| Name | Value |
|---|---|
| Predicate-type registry | `minecraft:data_component_predicate_type`, 15 entries, ids 0–14 (§3) |
| Max partial predicates on the wire | 64 |
| Infinite effect duration | −1 |
| Firework shapes | `small_ball`(0), `large_ball`(1), `star`(2), `creeper`(3), `burst`(4) |
| Operation names | `add_value`, `add_multiplied_base`, `add_multiplied_total` |
| Slot group names | `any`, `mainhand`, `offhand`, `hand`, `feet`, `legs`, `chest`, `head`, `armor`, `body`, `saddle` |

## Open questions

1. **[oracle]** Any-value predicate (`{}`) decoding: does a non-object value (`1`, `"x"`, `[]`) decode or fail? Probe:
   decode `{"predicates": {"minecraft:map_id": 1}}`.
2. **[oracle]** Value equality of exact components for types whose value class overrides equality oddly (stacks inside
   `charged_projectiles`, `Component` pages): probe a few `components` exacts against equal-but-differently-built values.
3. **[oracle]** Registry raw id assignment of `minecraft:data_component_predicate_type` and of the data-component-type
   registry (needed for the network form): expected = registration order of §3; probe `Registry.getId` for each.
4. **[oracle]** Written-book page equality (§3.9).
5. **[oracle]** NBT-encoding details of each predicate's `encodeStart` under NbtOps (only needed to *send* predicate
   components to clients): probe the encoded tag of one predicate per type.
6. Command item predicates (`/clear`, `/execute if items`) use a separate text parser for `item[component=value,~tag]`;
   it builds these same predicates (command spec).

## Oracle probe points

`ItemPredicate#test`, `DataComponentMatchers#test`, `DataComponentExactPredicate#test`,
`DataComponentPredicate$Type#CODEC`, `DataComponentPredicates` (registration order), `AnyValue#matches`,
`DamagePredicate#matches` (component), `EnchantmentPredicate#containedIn`, `EnchantmentsPredicate#matches`,
`PotionsPredicate#matches`, `PotionContents#getAllEffects`, `MobEffectsPredicate#test`,
`MobEffectsPredicate$MobEffectInstancePredicate#matches`, `CustomDataPredicate#matches`, `ContainerPredicate#matches`,
`BundlePredicate#matches`, `FireworkExplosionPredicate$FireworkPredicate#test`, `FireworksPredicate#matches`,
`WritableBookPredicate#matches`, `WrittenBookPredicate#matches`, `AttributeModifiersPredicate$EntryPredicate#test`,
`TrimPredicate#matches`, `JukeboxPlayablePredicate#matches`, `VillagerTypePredicate#matches`.

## Parity status

`predicates`: not yet run.
