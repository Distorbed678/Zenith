# Ticks: iteration orders of the collections that decide tick order (26.3)

- Verified by suites: `server-tick`, `block-updates`
- Sources: local reference reading of the 26.3 client (2026-11-26, reader session) to find every collection whose
  iteration or tie-break order reaches world state during ticking; black-box fixtures (throw-away, outside the repo)
  run against the public `fastutil` 8.5.18 jar that the launcher metadata lists for 26.3: **2,900 random scenarios on
  `Long2LongOpenHashMap` (default constructor; about 740,000 order checks, including iterator removal) and 1,500 on
  `Long2ByteOpenHashMap` (382,139 checks), and 3,000 on `Object2ObjectOpenHashMap` with controlled `hashCode`
  values (329,288 checks) — 0 differences** against the rules written below; the public OpenJDK `PriorityQueue`
  algorithm. Everything is in our own words.
- Related specs: [`../java/hash-collections.md`](../java/hash-collections.md) (JDK `HashMap`/`HashSet` order; the
  fastutil rules for `LongOpenHashSet(512, 0.5)` are also in [`../lighting/data-layers.md`](../lighting/data-layers.md)
  §8), [`server-tick.md`](server-tick.md), [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md),
  [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md).

## Summary

Several ordered facts of the tick pipeline are decided not by game logic but by the iteration order of a hash
collection or by the tie behaviour of a binary heap. This file states those orders once, so the other tick specs can
refer to them. Zenith may use any internal data structure as long as the *observable order* below is reproduced.

## Inputs and state

None of its own. The collections concerned are listed in the table of Behavior §1.

## Behavior (ordered rules)

### 1. Which collections matter

| # | Collection (where) | Key / element | Order matters? | Rule |
|---|---|---|---|---|
| 1 | `DistanceManager` simulation tracker `chunks` (fastutil `Long2ByteOpenHashMap`, default ctor) | packed chunk pos | **yes**: the order in which block-ticking chunks are random-ticked | §2 fastutil order; fed by the update order of [`server-tick.md`](server-tick.md) §9 |
| 2 | `DistanceManager` natural-spawn counter `chunks` (`Long2ByteOpenHashMap`) `keySet()` | packed chunk pos | **yes** (before a shuffle: the shuffle's input order) | §2 |
| 3 | `LevelTicks.nextTickForContainer` (`Long2LongOpenHashMap`, default ctor) | packed chunk pos → next trigger tick | **yes**: insertion order into the container heap (ties) | §2, §3 |
| 4 | `LevelTicks.containersToTick` (JDK `PriorityQueue`) | per-chunk tick container | **yes** (ties on equal head priority and sub-tick) | §3 |
| 5 | `LevelChunkTicks.tickQueue` (JDK `PriorityQueue`, order = trigger, priority, sub-tick) | scheduled tick | no (total order: sub-ticks are unique within one chunk) | any ordered structure |
| 6 | `ServerLevel.blockEvents` (`ObjectLinkedOpenHashSet`) | block event record | **yes** | §4 |
| 7 | `ChunkAccess.blockEntities` (`Object2ObjectOpenHashMap<BlockPos, BlockEntity>`) | block position | **yes**: ticker registration order when a chunk is loaded | §2 (object-key variant) |
| 8 | `ChunkAccess.pendingBlockEntities` (JDK `HashMap<BlockPos, CompoundTag>`) | block position | **yes**: promotion order at chunk activation | [`../java/hash-collections.md`](../java/hash-collections.md) |
| 9 | `Level.blockEntityTickers` (`ArrayList`) | ticker | **yes** (it *is* the ticking order) | [`server-tick.md`](server-tick.md) §8 |
| 10 | `LeveledPriorityQueue` levels (`LongLinkedOpenHashSet`) of the chunk trackers | packed chunk pos | **yes** (update order → #1, #2) | §4 (FIFO set) |
| 11 | `TicketStorage.tickets` (`Long2ObjectOpenHashMap<List<Ticket>>`) iteration in ticket purge | packed chunk pos | only through #1 | §2 |
| 12 | `DistanceManager.chunksToUpdateFutures` (`ReferenceOpenHashSet<ChunkHolder>`, identity hash) | chunk holder | order is **not reproducible** (identity hashes); affects only chunk-status promotion order, never a tick result by itself | treat as nondeterministic |
| 13 | `LevelTicks.allContainers`, `toRunThisTickSet`, `LevelChunkTicks.ticksPerPosition` | | no (membership / lookup by key only) | any set / map |
| 14 | `LevelChunk.tickersInLevel` (JDK `HashMap`) | block position → ticker wrapper | no (lookup only) | any map |
| 15 | `ChunkHolder.changedBlocksPerSection` (`ShortOpenHashSet`), light filters | | no (network only) | |

### 2. fastutil open-addressing hash maps and sets (8.5.x), default constructor

Applies to `Long2ByteOpenHashMap`, `Long2LongOpenHashMap`, `Long2ObjectOpenHashMap`, `LongOpenHashSet` and (with the
object hash of rule 2.2) `Object2ObjectOpenHashMap`, `ObjectOpenHashSet` and `ObjectOpenCustomHashSet`.

1. **Table.** Capacity `n` is a power of two; there are `n` normal slots plus one extra "null-key slot" (used by
   `long` key 0). The default constructor asks for 16 expected entries at load factor 0.75, which gives
   **`n = 32`** (next power of two of `ceil(16 / 0.75) = 22`) and `maxFill = min(ceil(n · 0.75), n − 1) = 24`. The
   initial `n` is remembered as `minN`.
2. **Slot of a key.** `long` key `k`: `h = k · 0x9E3779B97F4A7C15` (wrapping 64-bit), `h ^= h >>> 32`,
   `slot = (int)(h ^ (h >>> 16)) & (n − 1)`. Object key with 32-bit hash code `c`: `h = c · 0x9E3779B9` (wrapping
   32-bit), `slot = (h ^ (h >>> 16)) & (n − 1)`. `ChunkPos` keys are the packed long of `docs/specs/math/java-math-helpers.md`
   (x in the low 32 bits, z in the high 32 bits); `BlockPos` hash is the Vec3i hash (`(y + z·31)·31 + x`, wrapping).
   A `long` key equal to 0 is stored in the null-key slot, not in the table.
3. **Insert.** Probe from `slot` upward one slot at a time (wrapping from `n − 1` to 0) until an empty slot or an equal
   key. An equal key only replaces the value (the entry **keeps its slot**). A new key goes into the empty slot. After
   inserting, if the size *before* the insert was `>= maxFill`, grow: new `n' = nextPowerOfTwo(ceil((size + 1) / 0.75))`
   with `size` already incremented (25 entries → 64 slots), `maxFill` recomputed.
4. **Grow / rehash** (also used for the shrink in 2.6): walk the **old** slots from index `n − 1` **down to 0**,
   skipping empty ones, and insert each key into the new table by rule 2.3's probe; the null-key entry moves as is.
5. **Remove** (backward-shift deletion). Find the key. Clear its slot; call that slot the *hole*. Scan the slots that
   follow the hole one by one (wrapping): stop at the first empty slot (hole stays empty). For an occupied slot `p`
   with home slot `s` (rule 2.2): the entry may move into the hole iff walking forward from `s` reaches `p` only
   after passing the hole (cyclically `s ∉ (hole, p]`); when it may, move it into the hole, the vacated slot `p`
   becomes the new hole, and the scan restarts after `p`. Then `size −= 1`.
6. **Shrink.** After a *map-level* removal (not an iterator removal): if `n > minN` and `size < maxFill / 4`
   (integer division) and `n > 16`, rehash to `n / 2`. The table never shrinks below its initial 32.
7. **Iteration order.** The null-key entry first (if present), then slots **`n − 1` down to 0**, skipping empty ones.
   (`keySet()`, `values()` and the "fast" entry iterators all use this same order.)
8. **Removal through an iterator** (`it.remove()` while iterating, e.g. `LevelTicks` container sorting): the removal
   is rule 2.5 applied at the slot just returned, with two differences: (a) no shrink check; (b) when an entry is
   moved from a *lower* slot than the hole (it wrapped around the end of the table), the iterator saves a copy of
   that entry in a "wrapped" list; after slot 0 has been visited, the iterator visits the wrapped entries in the
   order they were saved. Removing a wrapped entry later removes it by key with the map-level remove (so rule 2.6's
   shrink check applies).
9. `ObjectLinkedOpenHashSet` / `LongLinkedOpenHashSet` keep insertion order (see §4) and do not follow rules 2.7–2.8.

### 3. `java.util.PriorityQueue` tie behaviour (the container heap in `LevelTicks`)

`containersToTick` orders chunk containers by the drain order of their head tick: priority ascending (lower
`TickPriority` value first), then sub-tick order ascending (trigger time is **not** compared here). Two containers
can tie (equal priority and sub-tick, typical after a reload, see
[`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §5). The heap is the standard binary
heap in an array:

1. **offer / add**: put the element at index `size`, then sift up: while the index `k > 0`, let `p = (k − 1) >> 1`;
   if the element compares `>= 0` against `queue[p]` stop; otherwise move `queue[p]` down to `k` and continue at `p`.
2. **poll**: take `queue[0]`; let `x = queue[size − 1]`, shrink the size; if elements remain, sift `x` down from index 0:
   while `k < size / 2`: `c = 2k + 1`; if `c + 1 < size` and `queue[c]` compares `> 0` against `queue[c + 1]` then
   `c += 1`; if `x` compares `<= 0` against `queue[c]` stop; else move `queue[c]` up to `k`, `k = c`. Place `x` at `k`.
3. **peek**: `queue[0]`. **iteration** (used when leftover containers are re-registered): array index order.
4. **iterator remove / `removeIf`** (used by `clearArea`): removing index `i` moves the last element into `i` and sifts
   it down, and if it did not move, sifts it up (the standard `removeAt`).
5. Ties therefore resolve by heap position, which depends on the insertion order, which for `LevelTicks` is the
   iteration order of `nextTickForContainer` (rule 2.7–2.8).

### 4. Insertion-ordered sets

`ObjectLinkedOpenHashSet` and `LongLinkedOpenHashSet`: iteration and `removeFirst` follow **first-insertion order**;
adding an element that is already present changes nothing (it keeps its place); removing and re-adding puts it at the
end. Used for the block-event queue and for every level queue of the chunk trackers.

## Edge cases and known quirks

1. `Object2ObjectOpenHashMap<BlockPos, …>` with several positions of equal hash code resolves them in insertion order
   by rule 2.3 (linear probing), so *which of two colliding block entities registers first* depends on the NBT list
   order of the chunk (see [`server-tick.md`](server-tick.md) §8).
2. The maps here are *not* iterated in insertion order: a freshly loaded world and a world that reached the same
   chunk set by a different history can iterate the same keys in different orders (collisions and growth history).
3. fastutil version drift: the 8.5.x line has used these rules; if the user's install ever ships another fastutil, re-run
   the fixtures (Open questions 1).

## Constants and IDs

fastutil: default expected size 16, load factor 0.75, initial table 32, max fill 24; long hash multiplier
`0x9E3779B97F4A7C15`, int hash multiplier `0x9E3779B9`. `PriorityQueue`: binary heap, parent `(k − 1) >> 1`.

## Oracle probe points

- Dump the real iteration order through public API: `ChunkMap#forEachBlockTickingChunk(Consumer<LevelChunk>)`,
  `DistanceManager#forEachEntityTickingChunk(LongConsumer)`, `ChunkMap#collectSpawningChunks(List<LevelChunk>)` (before
  the shuffle), `Level#tickBlockEntities()` plus the protected `Level#blockEntityTickers` list (agent reflection) read
  through `TickingBlockEntity#getPos()`.
- `LevelTicks` internals by reflection: `nextTickForContainer` (iterate its `long2LongEntrySet()` fast iterator),
  `containersToTick` (array order), after each `LevelTicks#tick`.
- Library check: print `Long2LongOpenHashMap.class.getProtectionDomain().getCodeSource().getLocation()` from the
  running game.

## Open questions

1. **[oracle] fastutil version of the running game.** Verified against 8.5.18. Probe: print the code-source of any
   fastutil class from the running 26.3 client and compare the jar version; if different, re-run the three fixtures
   (random `put` / `remove` / iterator-remove scenarios, compare iteration order after every operation).
2. **[oracle] `Long2LongMaps.fastIterator` removal while other puts occur between `it.next()` calls** is not used by the
   game (only removals happen during `LevelTicks` sorting), so it is not covered by the fixtures.

## Parity status

Not yet run (the fixtures above are the reader's own checks of the rules; the Zenith-side suites `server-tick` and
`block-updates` have not run).
