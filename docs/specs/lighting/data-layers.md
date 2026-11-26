# Lighting: data layers, section bookkeeping and the visible / updating split (26.3)

- Verified by suites: `light-engine` (section `data-layers`: per-section nibble dumps, `getDebugSectionType`, top-section
  bookkeeping, saved chunk NBT light arrays, light packet payloads)
- Sources: local reference reading of the 26.3 decompile (2026-11-25); everything below is written in our own words.
  Hash-set order was checked against a JDK + the public `fastutil` 8.5.18 jar with a throw-away fixture (not in the repo).
- Related specs: [`light-engine.md`](light-engine.md) (queues, block and sky propagation), [`sky-light-sources.md`](sky-light-sources.md)
  (per-column "lowest source" heightmap), [`chunk-steps.md`](chunk-steps.md) (`initialize_light`, `light`, load / unload,
  live updates, client), `../storage/region-and-level.md` (chunk NBT envelope), `../java/hash-collections.md` (the JDK
  `HashSet` order; this document needs the **fastutil** open-addressing order instead, see §8).

## Summary

Light is stored as 4-bit values in **data layers**: one 2048-byte nibble array per 16×16×16 section, for two independent
engines (block light, sky light). A layer exists for a section when the section **has data** (it holds a non-air block) or
**any of its 26 neighbouring sections has data** (this is the "light needs the one-section border" rule; it also creates
layers one section above and below the world and in sections of chunks that are not loaded). Sections are addressed by
packed 64-bit section nodes, blocks by packed 64-bit block nodes.

Each engine keeps two views of its layer map: an **updating** map the propagation code reads and writes, and a **visible**
map (a snapshot taken at the end of every engine run) that game logic, saving and networking read. Layers are
copy-on-write between the two. Section creation and removal are driven by `updateSectionStatus`; removal is deferred to
the end of the engine run. Chunk NBT loading **queues** layers per section; queued layers are applied (or kept waiting) by
the end-of-run "inconsistency" pass. Sky light additionally tracks, per section column, one past the top-most section that
has a layer ("top section"), and an engine-wide "lowest section ever stored"; these decide the sky value of sections that
have no layer and the initial contents of new layers.

## Inputs and state

### 1. Packed coordinates (needed for ordering and for equality of keys)

- **Block node** (`BlockPos.asLong`): 64-bit value with `x` in the top 26 bits (bits 38..63), `z` in bits 12..37 and `y`
  in the low 12 bits (bits 0..11); each field is two's-complement truncated (`x & 0x3FFFFFF`, `z & 0x3FFFFFF`,
  `y & 0xFFF`). Unpack by shifting up and arithmetic-shifting down (sign-extended). Stepping a node by a direction is
  unpack → add the step → repack (so a coordinate that leaves the field range wraps).
- **Section node** (`SectionPos.asLong`): `x` in bits 42..63 (22 bits), `z` in bits 20..41 (22 bits), `y` in bits 0..19
  (20 bits), all two's-complement truncated. `blockToSection(blockNode)` = pack(x>>4, y>>4, z>>4) (arithmetic shifts).
  **Zero node** of a section node = the node with `y` cleared (`node & !0xFFFFF`); it identifies a section **column**.
- **Flat index** of a block node (`node & !15`): clears the low four bits, i.e. rounds `y` down to a multiple of 16 (for
  the 12-bit field this is the section-local `y = 0` of the same section).
- `sectionRelative(c) = c & 15`. Section-local layer index of `(x, y, z)` (each 0..15) = `(y << 8) | (z << 4) | x`.
- Direction ordinals (used for queue bits and for loop order, **never reorder**): DOWN 0, UP 1, NORTH 2, SOUTH 3,
  WEST 4, EAST 5. Steps: DOWN (0,−1,0), UP (0,+1,0), NORTH (0,0,−1), SOUTH (0,0,+1), WEST (−1,0,0), EAST (+1,0,0).
  Opposite pairs: DOWN↔UP, NORTH↔SOUTH, WEST↔EAST.
- Light section range of a level: `minLightSection = minSectionY − 1`; `lightSectionCount = sectionsCount + 2`;
  `maxLightSection = minLightSection + lightSectionCount` (**exclusive**, = `maxSectionY + 2`). The Overworld
  (`minY = −64`, 384 tall) has world sections −4..19 and light sections −5..20 (inclusive).

### 2. `DataLayer` (one section, one engine)

- Fields: `data` (nullable 2048-byte array) and `defaultValue` (0..15). Constructors: empty (`data = null`, default 0);
  filled with a value (`data = null`, default = value); from a byte array (must be exactly 2048 bytes, default 0).
- **Nibble addressing:** index `i = (y << 8) | (z << 4) | x`; byte `i >> 1`; the **low nibble** holds even `i` (even `x`),
  the high nibble odd `i`. `get`: if `data == null` return `defaultValue`, else the nibble. `set(…, v)`: materialises the
  array first (see below), then replaces only that nibble with `v & 15`.
- **Materialising** (`getData`): if `data == null`, allocate 2048 zero bytes; if `defaultValue != 0` fill every byte with
  `defaultValue | (defaultValue << 4)`. The array is then kept forever (it is never nulled again except by `fill`).
- `fill(v)`: `defaultValue = v; data = null`.
- `copy()`: `data == null` → new layer with the same default and no array (stays lazy); else a clone of the array
  (default 0).
- Predicates: `isEmpty` = `data == null && defaultValue == 0`; `isDefinitelyHomogenous` = `data == null`;
  `isDefinitelyFilledWith(v)` = `data == null && defaultValue == v`.
- **Why null-ness matters for parity:** the chunk saver and the light packets omit layers for which `isEmpty` is true and
  write every other layer through `getData` (2048 bytes). A layer that was written with only zeros but has an array is
  **not** empty and **is** saved. Operations that materialise: any `set` (including `set(…, 0)`), `getData`, the
  clone of a materialised layer, `repeatFirstLayer` of a materialised layer. Operations that keep it lazy: `copy` of a lazy
  layer, `fill`, construction.

### 3. Per-engine storage state (`LayerLightSectionStorage`; one instance per engine)

| Field | Content |
| --- | --- |
| `sectionStates` | map section node → byte; default 0. Bits 0..4 = count of **neighbouring sections that have data** (0..26); bit 5 (value 32) = "this section has data". `0` means no entry. |
| `columnsWithSources` | set of **zero nodes** whose chunk has light enabled ("lightOn"). |
| `updatingSectionData` / `visibleSectionData` | the two maps section node → `DataLayer` (visible is a map copy; layer objects are shared until copy-on-write; sky maps also carry `topSections` and `currentLowestY`, see §6). |
| `changedSections` | set of section nodes whose updating layer is private to the current run (copy-on-write marker) or whose presence changed. |
| `sectionsAffectedByLightUpdates` | set of section nodes to report to the chunk source on the next swap. |
| `queuedSections` | map section node → `DataLayer` of data supplied from outside (chunk NBT, client packet) that is not yet part of the updating map. |
| `columnsToRetainQueuedDataFor` | set of zero nodes ("retain") — see §5. |
| `toRemove` | set of section nodes whose state dropped to 0 and whose layer will be removed at the end of the run. |
| `hasInconsistencies` | flag: some section was added / removed / queued since the last end-of-run pass. |

Hash-set iteration orders of `columnsWithSources`, `changedSections`, `sectionsAffectedByLightUpdates`, `queuedSections`
and `toRemove` never change a stored light value; only `sectionsAffectedByLightUpdates` changes the **order of the
change notifications** (§7). Their orders need not be reproduced bit-for-bit (see Edge cases 9).

## Behavior (ordered rules)

### 4. Which sections have a layer (`updateSectionStatus`)

`updateSectionStatus(sectionNode, sectionEmpty)` (called by chunk code, §`chunk-steps.md`):

1. `state = sectionStates[node]` (0 if absent). `newState` = `state` with bit 5 set when `!sectionEmpty`, cleared when
   `sectionEmpty`. If `newState == state` stop.
2. `putSectionState(node, newState)` (rule 4a).
3. `inc = sectionEmpty ? −1 : +1`. For `dx` in −1..1 (outer), `dy` in −1..1 (middle), `dz` in −1..1 (inner), skipping
   (0,0,0): `n = node + (dx,dy,dz)`; `ns = sectionStates[n]` (0 if absent); `putSectionState(n, ns with its neighbour
   count replaced by count(ns) + inc)`. A resulting count outside 0..26 is a fatal error (cannot happen with consistent
   calls). The nested order **X, then Y, then Z** is observable for sky light (rule 12: creation order decides the
   initial contents of layers).

4a. `putSectionState(node, state)`:
 - `state != 0`: store it; if the previous value was 0 → `initializeSection(node)`.
 - `state == 0`: remove the entry; if the previous value was non-zero → `removeSection(node)`.

4b. `initializeSection(node)`: if `node` is in `toRemove`, **remove it from `toRemove` and do nothing else** (the layer was
   never removed; its data is kept). Otherwise, in this order: `updating[node] = createDataLayer(node)`;
   `changedSections += node`; `onNodeAdded(node)` (sky only, §6); mark `node` **and its 26 neighbours** as affected
   (add the 27 section nodes to `sectionsAffectedByLightUpdates`); `hasInconsistencies = true`.
   Note `createDataLayer` runs **before** `onNodeAdded`, so it does not see the node itself in the top-section map.

4c. `removeSection(node)`: add to `toRemove`; `hasInconsistencies = true`. The layer stays readable until the end-of-run
   pass (rule 7).

A section "**stores light**" (`storingLightForSection`) iff the **updating** map has a layer for it. Debug type
(`getDebugSectionType`): state 0 → EMPTY (display `"2"`), bit 5 clear but non-zero → LIGHT_ONLY (`"1"`), bit 5 set →
LIGHT_AND_DATA (`"0"`).

### 5. Queued data, retain flags and light-enabled columns

- `queueSectionData(node, layer)`: non-null → `queuedSections[node] = layer` and `hasInconsistencies = true`;
  null → remove `node` from `queuedSections` (**no** flag change).
- `retainData(zeroNode, retain)`: add / remove `zeroNode` in `columnsToRetainQueuedDataFor`.
- `setLightEnabled(zeroNode, enable)` (storage level): add / remove `zeroNode` in `columnsWithSources`.
  `lightOnInSection(sectionNode)` = `columnsWithSources` contains the zero node of the section's column.
- `createDataLayer(node)` (block engine): the queued layer for `node` (**the same object**, not a copy) if present, else
  a new empty layer. Sky variant: rule 12.

### 6. Sky-only bookkeeping (`SkyLightSectionStorage`)

State held in the sky engine's **updating** map (and cloned into every visible snapshot):
- `currentLowestY` — initial value `i32::MAX`. The lowest section Y for which any layer was ever added; it only ever
  **decreases** (removal does not raise it). It is also the **default return value** of `topSections`.
- `topSections` — map zero node → one past the highest section Y that has a layer in that column. A missing entry reads
  as `currentLowestY` ("column has no layers").

`onNodeAdded(node)`: if `y(node) < currentLowestY`, set `currentLowestY = y(node)` (and the map default with it). Then, for
the column, if `topSections[zero] < y + 1` (the missing value `currentLowestY` counts as the old value) set it to `y + 1`.

`onNodeRemoved(node)` (called from the end-of-run pass, rule 7 step 4): if `topSections[zero] == y + 1` (the removed
section was the column's top): `n = node, yy = y`; while `n` has no layer in the updating map **and** `yy >= currentLowestY`:
`n = n − one section down; yy −= 1`. Then if `n` has a layer set `topSections[zero] = yy + 1`, else remove the entry.

Helpers: `hasLightDataAtOrBelow(sectionY)` = `sectionY >= currentLowestY`; `isAboveData(node)` = the column's top equals
`currentLowestY` (no layers) **or** `y(node) >= top`; `getTopSectionY(zero)` = `topSections[zero]` (default
`currentLowestY`); `getBottomSectionY()` = `currentLowestY`.

### 7. The end-of-run pass: `markNewInconsistencies` and `swapSectionMap`

Called by the engine at the end of every `runLightUpdates` (see `light-engine.md`, Behavior rule 2), in this order:

**`markNewInconsistencies`** (does nothing unless `hasInconsistencies`; it first clears the flag):
1. For each node in `toRemove` (any order): `queued = queuedSections.remove(node)`; `stored = updating.remove(node)`.
   If `columnsToRetainQueuedDataFor` contains the node's zero node: put `queued` back into `queuedSections` if it was
   non-null, else put `stored` (the removed layer) there if non-null.
2. Clear the 2-entry layer cache (semantically transparent).
3. For each node in `toRemove`: `onNodeRemoved(node)` (sky), `changedSections += node`.
4. Clear `toRemove`.
5. For each `(node, layer)` in `queuedSections`: if the **updating** map has a layer for `node` (stores light): if that
   layer is not the very same object as `layer`, replace it (`updating[node] = layer`, `changedSections += node`); then
   **remove the entry from `queuedSections`** (also when it was the same object). Entries for sections without a layer
   stay queued (they are consumed later by `createDataLayer`, rule 4b/12, or dropped by `queueSectionData(node, null)`).
6. Clear the cache again.

**`swapSectionMap`:**
1. If `changedSections` is non-empty: `visible = copy of updating` (map copy; layer objects shared; sky also copies
   `topSections` and `currentLowestY`); clear `changedSections`. (If it is empty the visible map is **not** refreshed.)
2. If `sectionsAffectedByLightUpdates` is non-empty: for each node (set iteration order, §8) call
   `chunkSource.onLightUpdate(layer, sectionPos)` with this engine's layer (SKY or BLOCK); clear the set.

**Copy-on-write (inside a run):** the first write to a section in a run (`setStoredLevel`): if `changedSections.add(node)`
returned true, the updating map receives a **copy** of the layer (so the visible snapshot keeps the old object) and the
write goes to the copy; otherwise the current updating layer is written in place. `getDataLayerToWrite(node)` (used by the
sky engine's bulk writes) does the same but returns null when there is no layer. Consequence: a layer object that was
placed in the updating map by `initializeSection` or by rule 7.5 (e.g. a layer parsed from NBT) is mutated **in place**
during the run that placed it, because its node is already in `changedSections`.

`setStoredLevel(blockNode, level)`: obtain the writable layer as above, store the nibble, then add the section nodes
given by `aroundAndAtBlockPos(blockNode)` to `sectionsAffectedByLightUpdates`: for the block's
`x-1..x+1`, `y-1..y+1`, `z-1..z+1` ranges convert the extreme coordinates to section coordinates; if all three ranges are a
single section add that one node, else add every node of the box (loops X, Y, Z ascending). `getStoredLevel(blockNode)`
reads the **updating** layer (a missing layer is a fatal error; callers always test `storingLightForSection` first).

### 8. Reading light values (what the game sees)

All game reads use the **visible** map (and so lag the engine until `swapSectionMap`):

- **Block engine:** layer of the block's section in the visible map; none → 0; else the nibble.
- **Sky engine** (`getLightValue(blockNode)` with `updating = false`): `top = visible.topSections[zero]`.
  - If `top != visible.currentLowestY` **and** `sectionY < top`: take the layer of the block's section. If it is missing,
    set the block node to its flat index (local `y = 0`) and walk upward one section at a time: `sectionY += 1`; if
    `sectionY >= top` return **15**; the first section that has a layer supplies the nibble at `(x, 0, z)` (local y 0).
    If the layer was present, read the nibble at the real local coordinates.
  - Otherwise (column has no layer, or the section is at/above the top): **15** (in the visible variant the light-enabled
    flag is not consulted). A hypothetical `updating = true` read returns 0 instead of 15 when the column is not
    light-enabled; it is not used by any caller in 26.3.
- A level without sky light (dimension type `has_skylight = false`) has **no sky engine**: its sky listener is a dummy that
  returns 0 and no data layers.
- `getDataLayerData(sectionNode)` (used by save and network): the **queued** layer if one exists, else the visible layer,
  else null.

### 9. The sky layer-creation rule (`createDataLayer`, sky)

For a new section `node` (rule 4b), in order:
1. A queued layer for `node` → use it (same object).
2. `top = updating.topSections[zero]` (before this node is registered). If `top != currentLowestY` **and** `y(node) < top`:
   search upward `node+1, node+2, …` for the first layer in the updating map (it exists because `top − 1` has one) and
   return `repeatFirstLayer(thatLayer)`: if it is lazy (`data == null`) → `copy()` (lazy, same default); else a new
   2048-byte array filled by copying the source layer's **first 128 bytes (its local-y 0 slice)** into all sixteen 128-byte
   slices.
3. Otherwise: `lightOnInSection(node)` (the chunk is light-enabled) → a layer **filled with 15** (lazy, default 15); else
   an empty layer.

Sky layers created in rule 2 therefore start as "the light of the bottom face of the section above, extended downward"; the
creation **order** of the sections (rule 4 loops) decides which layer a new one copies.

## Edge cases and known quirks

1. **Layers exist for sections of chunks that are not loaded** (neighbour counts cross chunk borders) and for the two padding
   sections of every world column that contains data. Their data is real engine state (and is saved with the owning chunk
   if that chunk is later saved, since the saver walks `minLightSection .. maxLightSection − 1` of the chunk being saved).
2. **A removed chunk's border sections can survive.** Unloading a chunk calls `updateSectionStatus(…, true)` for its
   sections; a section whose state stays non-zero because a neighbour chunk still has data **keeps its layer and its
   old data** (it is not removed, not zeroed, not re-queued).
3. **`toRemove` cancellation:** if a section is scheduled for removal and re-added before the pass runs (same batch), its
   old layer, contents and queued data are kept untouched.
4. **Queued data for sections without a layer is retained indefinitely** (until `queueSectionData(node, null)` or until the
   section appears). Retain flags only control whether data of a *removed* layer is put back into the queue (rule 7.1).
5. The sky `currentLowestY` never rises; after all layers are gone it stays at its minimum while `topSections` entries are
   removed per column.
6. `getDataLayerData` returns the **queued** layer first: right after a chunk load (before the engine run) the saved arrays
   are what a save or a packet would see.
7. `visible` is only refreshed when `changedSections` is non-empty; a run that only touched `sectionsAffectedByLightUpdates`
   still notifies.
8. **Deferred removal means the first run after a status flip still sees the layer** (reads of the updating map in that
   run succeed; the visible map keeps it until the swap that follows the removal pass).
9. The iteration orders of `columnsWithSources`, `changedSections`, `queuedSections`, `toRemove` and `sectionsAffectedByLightUpdates`
   do not affect stored values. `sectionsAffectedByLightUpdates` order only decides the order of `onLightUpdate` calls; on the
   server these feed an identity-hash set of chunk holders (unspecified order), on the client a dirty-section set, so a
   deterministic order (ascending insertion order of first occurrence is sufficient) is acceptable.
10. `DataLayer` byte arrays handed to the engine by the loader are used **directly** (not cloned) as layers; the engine
    may mutate them in place (rule 7 copy-on-write note).

### Hash-set iteration order used by the engine (`LongOpenHashSet`, fastutil 8.5.x)

Only `blockNodesToCheck` (the pending block-check set, `light-engine.md` Inputs B) needs its order exactly; it is a
fastutil `LongOpenHashSet(512, 0.5)`. Its order is **not** the JDK `HashSet` order of `../java/hash-collections.md`.

- Table length `n` is a power of two; slots are `0..n−1` plus one extra "null slot" for key 0. Constructed with
  expected 512 and load factor 0.5: `n = 1024`, `maxFill = 512` (`min(ceil(n·0.5), n−1)`), `mask = n − 1`.
- **Hash** of a key `k` (64-bit pattern, treat as unsigned for the multiply): `h = k · 0x9E3779B97F4A7C15 (mod 2^64)`;
  `h ^= h >>> 32`; `h ^= h >>> 16`; the **slot** is `(int)h & mask` (low 32 bits first, then the mask).
- **Add:** key 0 → set a `containsNull` flag (no slot). Otherwise probe linearly from the slot upward (wrapping at `n`)
  until an empty slot (key 0 means empty) or an equal key (then nothing happens); store the key. Then, **if the size before
  this insertion was `>= maxFill`**, grow after incrementing the size: new `n' = nextPowerOfTwo(ceil((size + 1) / 0.5))`
  with `size` already incremented (size 513 → 2048 slots), `maxFill` recomputed.
- **Rehash** (grow): walk the old slots from the **highest index down to 0**, skipping empty slots (the null slot is not
  moved), inserting each key into the new table by the same probe rule.
- **Iteration:** the null key first (if present), then slots from index `n − 1` **down to 0**, skipping empty slots.
- `clear` empties the table and the null flag; the engine then calls `trim(512)`, which shrinks the table back to
  1024 slots when it had grown (it never shrinks below 1024 for this construction), so every run starts with the 1024-slot
  table and an empty set.
- Fixture status: the rules above reproduce the real iteration order for 30/30 random sets (5 to ~4200 keys, including key 0
  and growth to 4096+ slots) generated with fastutil 8.5.18.

## Constants and IDs

`DataLayer` size 2048 bytes (4096 nibbles); layer slice 128 bytes; light range padding 1 section each side; max neighbour
count 26; section state bit 5 = has data (value 32), bits 0..4 = neighbour count; `currentLowestY` initial `i32::MAX`;
`blockNodesToCheck` initial capacity 512, load factor 0.5.

NBT keys per section tag: `Y` (byte), `BlockLight` (byte array 2048), `SkyLight` (byte array 2048); chunk tag key
`isLightOn` (byte; written only when true).

## Oracle probe points

(Public API of 26.3, usable from a Java agent or a server-side probe; none of these are required by the Rust build.)

- `LevelLightEngine#getLayerListener(LightLayer)` → `LayerLightEventListener#getDataLayerData(SectionPos)` (queued-or-
  visible layer), `#getLightValue(BlockPos)` (visible value); `LevelLightEngine#getRawBrightness(BlockPos, int)`,
  `getMinLightSection()`, `getMaxLightSection()`, `getLightSectionCount()`, `lightOnInColumn(long)`.
- `LevelLightEngine#getDebugSectionType(LightLayer, SectionPos)` → state 0/1/2 per section (dumps the section-state map).
- `DataLayer#getData()`, `#get(int,int,int)`, `#isEmpty()`, `#isDefinitelyHomogenous()` (dump null-ness separately from
  content: call `isDefinitelyHomogenous` **before** `getData`, which materialises the array).
- `SerializableChunkData.copyOf(ServerLevel, ChunkAccess)` then `write()` → exact saved section tags (which layers are
  present, and their bytes); `ClientboundLightUpdatePacketData(ChunkPos, LevelLightEngine, null, null)` for the packet form.
- `ServerChunkCache#getLightEngine()` / `ServerLevel#getLightEngine()` / `ClientChunkCache#getLightEngine()`.
- Internal state (agent-only): `LayerLightSectionStorage.sectionStates`, `updatingSectionData`, `queuedSections`,
  `toRemove`, `SkyLightSectionStorage.SkyDataLayerStorageMap.topSections/currentLowestY`.

## Open questions

1. **[oracle] fastutil version.** The order rules were verified against 8.5.18, the version Prism's metadata lists for the
   recent releases. Probe: print `LongOpenHashSet.class.getProtectionDomain().getCodeSource().getLocation()` from the
   running 26.3 client and compare the jar's version; if different, re-run the 30-set fixture (insert N packed block nodes
   into `new LongOpenHashSet(512, 0.5F)`, iterate, compare).
   **Answered (main thread, 2026-11-26):** the 26.3 version metadata the launcher installs from lists
   `it.unimi.dsi:fastutil:8.5.18` — the version the fixture was checked against. Zenith's `light-engine` suite should
   still replay this order against the user's own fastutil jar (like `java-hashset` does with the JDK).
2. **[oracle] `hasInconsistencies` and "lazy vs materialised" layers after the very first load.** Probe: load a freshly generated
   chunk, `copyOf(...).write()`, and record for every light section (−5..20) whether `BlockLight` / `SkyLight` is
   present; check that sections never written by `setStoredLevel` or `set` (lazy empty) are absent.
3. **[oracle] Sky layer creation order** (rule 9 step 2). Probe: call `updateSectionStatus` for a single column in
   ascending then descending order on a scratch engine and dump both layers' bytes; confirm the replicated-bottom-slice
   rule and its dependence on order.
4. **(implementer) Adopted queued layers.** `createDataLayer` hands the queued object to the updating map and the queue
   entry stays until the end-of-run pass (rule 7.5). Zenith keeps layers as copy-on-write shared handles, so the entry is
   replaced by an "adopted" marker at adoption: `getDataLayerData` then answers with the updating layer (the same object in
   the game), and rule 7.1 puts that layer back into the queue for retained columns. Equivalent as long as nothing reads
   `getDataLayerData` between adoption and the swap in a way that differs; probe: client first-chunk order (chunk-steps Q6).
5. **(implementer) Order of `sectionsAffectedByLightUpdates`.** Stored in insertion order of first occurrence (spec Edge 9
   allows it); `initializeSection` marks its 27 sections with the offsets in X, Y, Z ascending order. Only the order of
   change notifications could differ from the game.
6. **(implementer) `DataLayer.getData` on shared layers.** Packets and saves read layers through a non-materialising
   `to_bytes` (a lazy layer yields its fill bytes). The game's `getData` materialises the shared layer, which can only
   change null-ness of a layer that is then *not empty* anyway (empty layers are never read); confirm with the packet probe.
## Parity status

Not yet run.
