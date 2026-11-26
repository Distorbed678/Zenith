# Codec: loot entries (26.3)

- Verified by suites: `loot-tables`
- Sources: local reference reading of the 26.3 game (2026-11-26); see [`../loot-tables.md`](../loot-tables.md) for the
  generation algorithm that drives these. Written in our own words.

## Summary
An **entry** sits in a pool's `entries` list or inside a composite entry's `children`. There are nine entry types
(registry `minecraft:loot_pool_entry_type`, discriminator field `type`): leaf entries produce stacks; composite entries
choose among children; two "expandable" entries (`tag`, `loot_table`) may turn into many leaves.

## Common fields
Every entry has (all optional): `condition` (one condition holder) and `modifier` (one function holder, §
`loot-tables.md` 3.1). Entries that can be *picked by weight* ("uniform" entries: every type except the three
composites) also have `weight: int = 1` and `quality: int = 0` (any int, negatives allowed).
The effective leaf weight at roll time is `max(floor(weight + quality × luck), 0)` (f32 arithmetic, see
`loot-tables.md` 3.3a).

## Types

| id | fields (besides common) | leaf behaviour when picked |
|---|---|---|
| `empty` | – | produces nothing (still consumes a share of the weight) |
| `item` | `name`: item id (fixed registry reference, no inline) | one stack `ItemStack(item, count 1)` with the item's default components |
| `loot_table` | `value`: loot table(s) — a holder set (table id, inline table, array of those, or `#tag`; compact single form accepted); `expand: bool = false` | see below |
| `dynamic` | `name`: identifier | asks the context's dynamic-drop map for `name`; if a callback is registered it emits its stacks, else nothing |
| `tag` | `items`: item holder set (`#tag`, single id, or id list); `expand: bool = false` | see below |
| `slots` | `slot_source`: slot-source holder (contexts.md §5) | emits a **copy** of every non-empty item currently in the source's slots, in slot-source order |
| `alternatives` | `children: [entry] = []` | composite (below) |
| `sequence` | `children: [entry] = []` | composite |
| `group` | `children: [entry] = []` | composite |

### `item`
`createItemStack` pushes `new ItemStack(item)` — count 1, default components (component patch empty). Everything else
(count, enchantments, …) comes from functions.

### `tag` (`expand`)
- `expand = false` (default): expands to **one** leaf (this entry's weight/quality). When picked it emits one
  `ItemStack(item)` of **every** item of the set, in the set's iteration order.
- `expand = true`: expands to **one leaf per item** in the set's iteration order; each leaf has the entry's own
  weight and quality, and when picked emits one stack of its item. (So the pool's choice is spread over the items;
  total weight grows with the set size.)
- The item order is the order of the resolved holder set: for a tag, the tag's flattened content order (REG tag
  rules); for an explicit list, list order. **[oracle]** see `loot-tables.md` Open question 1.
- Both modes always "succeed" (return true), even if the set is empty (zero leaves).

### `loot_table` (`expand`)
- `expand = false`: one leaf; when picked it runs **every** table of the holder set in order via
  `getRandomItemsRaw` on the *same context* (same random, same luck, same params); stacks are not split yet.
- `expand = true`: one leaf per table (each with the entry's weight/quality); when picked, runs only that table.
- Each table run pushes/pops the visited set (self-inclusion is silently ignored, `loot-tables.md` 3.2). A table
  reference that is an *id* not present in the registry resolves to nothing at decode time (decode error for
  unknown ids in a holder-set of fixed references; inline tables are fine).
- The nested table's own `modifier` and pools apply; the **entry's** `modifier` applies afterwards on top.

### `alternatives`
`expand` tries children in order, returns true at the first child that returns true (that child's leaves are
added), false if none. Useful as "first matching branch". A child "returns true" if its condition holds and its own
expansion succeeded (a `sequence` child that fails midway still leaves already-added leaves from its earlier
children in the output — see quirk 2).

### `sequence`
Children expanded in order; stops at the first whose expand returned false; returns true iff all returned true.
Leaves from children before the failing one **have already been emitted** (and stay).

### `group`
Expands every child in order; always returns true (children whose condition fails simply contribute nothing).

### Composite `modifier` and `condition`
Composites carry `condition` and `modifier` only (no weight/quality of their own). The composite's modifier applies
to the stacks of every leaf beneath it, after that leaf's own modifier (`loot-tables.md` 3.1).

## Edge cases and known quirks
1. Weight-0 leaves (from `weight: 0`, or a negative result of `weight + quality·luck`) are discarded during collection
   but their **conditions were evaluated** — a condition that draws random numbers (`random_chance`) shifts later draws.
2. `sequence` returning false still keeps leaves it added before failing; `alternatives` then continues to its next
   child, so those leaves coexist with the next child's leaves (rarely used by vanilla).
3. `tag`/`loot_table` with `expand = true` and an empty set produce no leaves: with no other leaves the roll does
   nothing (and draws nothing).
4. `item` entries ignore the item's `enabled` status here; the stack splitter drops stacks whose item is disabled by
   feature flags at the end.
5. `dynamic` with an unregistered name is a silent no-op; the callback registered by a block entity (e.g. decorated pots,
   shulker boxes: "contents") emits stacks in its own order.
6. Validation rules (non-blocking): composite `children` empty → problem "Empty children list"; `alternatives`: every
   non-last child must have a condition ("Unreachable entry!").

## Constants and IDs
Entry type ids: `empty, item, loot_table, dynamic, tag, slots, alternatives, sequence, group` (registered in this
order). Defaults: weight 1, quality 0, expand false.

## Open questions
- **[oracle]** `HolderSet<Item>` iteration order for `tag` entries (tag flattening order). Probe
  `HolderSet.stream()` for `#minecraft:*` item tags used in vanilla tables.

## Oracle probe points
`LootPoolEntryContainer.expand`, `AlternativesEntry/EntryGroup/SequentialEntry.compose`, `TagEntry.addExpandedEntries`,
`NestedLootTable.addExpandedEntries`, `UniformContainerBase.EntryBase.getWeight`.

## Parity status
`loot-tables`: not yet run.
