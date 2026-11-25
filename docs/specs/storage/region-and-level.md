# Storage: Region files, world layout, and level persistence (26.3)

- Verified by suites: `anvil-io`
- Sources: public Anvil region format, NBT, RFC 1950 (zlib), RFC 1952 (gzip), `lz4-java` block-stream format, and OS file-locking documentation; compatibility facts, 26.3 directory layout, error-recovery rules, and exact write/flush semantics confirmed by local reference reading (2026-11-24). Everything below is written in our own words.

## Summary
A world save is a directory containing one exclusive session lock, top-level metadata (`level.dat` and its rotated backup `level.dat_old`), per-player NBT files, global and per-dimension auxiliary `.dat` files, and per-dimension Anvil region stores split across three subdirectories (`region/`, `entities/`, and `poi/`). Every multi-byte integer in `.mca` region headers and chunk length prefixes is **big-endian**; the only little-endian fields are inside compression ID 4 (`LZ4Block`) block headers and RFC 1952 gzip headers. Zenith must read every valid or partially recoverable file vanilla reads, reject or recover identically on corruption or truncation, and emit byte-compatible files and directory trees.

---

## World directory layout (26.3)

In 26.3, every dimension lives under `<world>/dimensions/<namespace>/<path>/`. The legacy pre-26.3 layout (overworld directly in `<world>/`, Nether in `DIM-1/`, End in `DIM1/`, players in `playerdata/`) is migrated before world load by the file-fixer and is not the active 26.3 write layout.

| Path relative to `<world>/` | Purpose |
|---|---|
| `session.lock` | Exclusive session lock file (3 bytes: UTF-8 snowman) |
| `level.dat` | Active gzip-compressed world metadata NBT (`{ "Data": { ... } }`) |
| `level.dat_old` | Previous `level.dat` backup rotated on every save |
| `level.dat_corrupted_<YYYY-MM-DD_HH-mm-ss>` | Quarantine name when a broken `level.dat` is moved aside during fallback recovery |
| `icon.png` | World icon image |
| `data/<namespace>/<path>.dat` | World-scoped saved data (world-gen settings, game rules, scoreboard, maps, command storage, etc.) |
| `players/data/<uuid>.dat` | Active per-player gzip-compressed NBT (`<uuid>` is lowercase canonical hyphenated UUID) |
| `players/data/<uuid>.dat_old` | Previous per-player backup rotated on save |
| `players/data/<uuid>_corrupted_<YYYY-MM-DD_HH-mm-ss>.dat` | Quarantine copy when `<uuid>.dat` fails to load |
| `players/advancements/` | Per-player advancement progress |
| `players/stats/` | Per-player statistics |
| `datapacks/` | World data packs |
| `generated/` | Generated structure templates |
| `resourcepacks/resources.zip` | Bundled world resource pack |
| `dimensions/<ns>/<path>/region/` | Terrain, biome, heightmap, light, block-entity, structure, and proto-chunk entity `.mca`/`.mcc` files |
| `dimensions/<ns>/<path>/entities/` | Full-chunk entity `.mca`/`.mcc` files |
| `dimensions/<ns>/<path>/poi/` | Point-of-interest section `.mca`/`.mcc` files |
| `dimensions/<ns>/<path>/data/<ns>/<path>.dat` | Dimension-scoped saved data (chunk tickets, raids, world border, etc.) |

Built-in dimension paths:
- Overworld (`minecraft:overworld`): `dimensions/minecraft/overworld/`
- Nether (`minecraft:the_nether`): `dimensions/minecraft/the_nether/`
- End (`minecraft:the_end`): `dimensions/minecraft/the_end/`

Resolving a `<namespace>:<path>` identifier against a root directory joins `<root>/<namespace>/<path>`, normalizes both paths, and rejects any result that escapes `<root>`.

---

## Region file container (`.mca` and `.mcc`)

### Coordinates and file naming
- One region covers a 32 × 32 grid of chunks (1,024 slots).
- From signed 32-bit global chunk coordinates `(x, z)`:
  - Region coordinates: `rx = x >> 5` (arithmetic right shift, equivalent to `floor(x / 32)`), `rz = z >> 5`.
  - Region-local coordinates (`0..31`): `lx = x & 31`, `lz = z & 31`.
  - Header slot index (`0..1023`): `index = lx + lz * 32`.
- Region filename: `r.<rx>.<rz>.mca` (matched by `^r\.(-?[0-9]+)\.(-?[0-9]+)\.mca$` when enumerating region files).
- External oversized-chunk filename: `c.<x>.<z>.mcc` (uses **global** chunk coordinates, stored in the **same directory** as the `.mca` file).

### File geometry and 8,192-byte header
A region file is divided into fixed **4,096-byte sectors**. Sectors `0` and `1` (bytes `0..8191`) form the header; chunk payloads occupy sectors `>= 2`.

| Byte range | Contents |
|---|---|
| `0..4095` | 1,024 big-endian 32-bit **location entries**, indexed by `lx + lz * 32` |
| `4096..8191` | 1,024 big-endian 32-bit **timestamp entries**, indexed by `lx + lz * 32` |
| `8192..` | Data sectors starting at `sectorNumber * 4096` |

#### Location entry (32-bit big-endian)
- Value `0`: chunk is absent.
- Non-zero value: `(sectorNumber << 8) | sectorCount`
  - `sectorNumber = (entry >> 8) & 0x00FFFFFF` — 24-bit unsigned starting sector index (byte offset = `sectorNumber * 4096`).
  - `sectorCount = entry & 0xFF` — 8-bit unsigned sector count (`1..255`).

#### Timestamp entry (32-bit big-endian)
- Unix epoch time in whole seconds, truncated to signed 32-bit integer (`floor(epochMillis / 1000)`).
- Updated to the current epoch seconds both when a chunk is **written** and when an existing chunk is **cleared** (clearing zeroes the location entry while setting the timestamp to the deletion time; it does not zero the timestamp).

### Chunk record layout inside a data sector
Starting at byte offset `sectorNumber * 4096`:

| Offset | Size | Encoding | Meaning |
|---|---|---|---|
| `0` | 4 bytes | Big-endian signed 32-bit int | `length = 1 + compressedPayloadBytes` (counts the version byte plus the payload bytes) |
| `4` | 1 byte | Unsigned 8-bit | `versionByte`: bit 7 (`0x80`) = external-file flag; bits `0..6` (`versionByte & 0x7F`) = compression ID |
| `5` | `length - 1` bytes | Compressed NBT stream | Named-root NBT compound (root name `""`), present only when bit 7 of `versionByte` is clear |

Total record size in bytes: `dataSize = 5 + compressedPayloadBytes = 4 + length`. Unused trailing bytes inside the last allocated 4,096-byte sector are not explicitly zeroed during a write (see sector padding on close below).

### Compression versions and exact stream settings
Write compression is configured globally by `region-file-compression` in `server.properties` (default `"deflate"`). Invalid values log an error and keep the previous selection. Readers always dispatch per chunk according to that chunk's stored `versionByte`.

| ID | External ID (`ID \| 0x80`) | `server.properties` value | Format | Exact writer settings & buffering | Exact reader buffering & decoding |
|---|---|---|---|---|---|
| `1` | `129` (`0x81`) | *(none — cannot be selected for writing)* | GZip (RFC 1952) | 8,192-byte output buffer → gzip stream (512-byte internal buffer, default zlib level `-1` = level 6, `syncFlush = false`, mtime `0`, OS byte `0xFF`) | 8,192-byte read buffer over gzip decoder (512-byte buffer) |
| `2` | `130` (`0x82`) | `"deflate"` *(default)* | Zlib / Deflate (RFC 1950) | 8,192-byte output buffer → zlib stream (512-byte internal buffer, default zlib level `-1` = level 6, `syncFlush = false`, 2-byte zlib header + Adler-32 trailer) | 8,192-byte read buffer over zlib decoder (512-byte buffer) |
| `3` | `131` (`0x83`) | `"none"` | Raw uncompressed NBT | 8,192-byte output buffer directly into chunk buffer | 8,192-byte read buffer directly over slice/file |
| `4` | `132` (`0x84`) | `"lz4"` | `lz4-java` `LZ4Block` stream *(NOT standard LZ4 Frame Format)* | 8,192-byte output buffer → `LZ4Block` stream (65,536-byte block size, fast LZ4 block compressor, 28-bit-masked xxHash32 seed `0x9747B28C`, `syncFlush = false`) | 8,192-byte read buffer over `LZ4Block` decoder |
| `127` | `255` (`0xFF`) | *(none)* | Custom compression sentinel | Unsupported for writing (error) | Intercepted before decoding: reads one modified-UTF-8 string (`u16` byte length + bytes), checks `namespace:path` identifier syntax, logs an error either way, and treats chunk as missing |

#### Exact wire format for compression ID 4 (`lz4-java` `LZ4Block`)
Compression ID 4 uses `lz4-java`'s block-stream framing, **not** the official LZ4 Frame format (`0x184D2204`). A stream consists of zero or more data blocks followed by one 21-byte end-of-stream terminator block. Every block begins with a **21-byte header**:

| Offset | Size | Encoding | Field |
|---|---|---|---|
| `0` | 8 bytes | ASCII | Magic `"LZ4Block"` (`4C 5A 34 42 6C 6F 63 6B`) |
| `8` | 1 byte | Bitfield | `token = method \| compressionLevel` |
| `9` | 4 bytes | Little-endian `u32` | `compressedLength` |
| `13` | 4 bytes | Little-endian `u32` | `decompressedLength` |
| `17` | 4 bytes | Little-endian `u32` | `checksum` |

- `compressionLevel = max(0, ceil(log2(blockSize)) - 10)`. For the default 65,536-byte (`2^16`) block size, `compressionLevel = 6`.
- `method`: `0x10` (raw/uncompressed) or `0x20` (LZ4 raw block compressed). Default block tokens are therefore `0x16` (raw) and `0x26` (compressed).
- For each buffered block of `n` uncompressed bytes (`1 <= n <= 65,536`):
  - Compute `checksum = xxHash32(uncompressedBytes, seed = 0x9747B28C) & 0x0FFFFFFF`. **The top 4 bits are cleared** (28-bit checksum).
  - Run fast LZ4 block compression. If the compressed output length is `>= n` (no size reduction), write a raw block (`method = 0x10`, `compressedLength = n`, uncompressed bytes verbatim). Otherwise write an LZ4 block (`method = 0x20`, compressed bytes).
- Terminator block (emitted on stream close): 21-byte header with magic `"LZ4Block"`, `token = 0x10 | compressionLevel` (`0x16`), `compressedLength = 0`, `decompressedLength = 0`, `checksum = 0`, and 0 payload bytes.
- On read, the decoder accepts any valid `compressionLevel` (`0..15`, implying max block size `1 << (compressionLevel + 10)`), checks the magic, token, length invariants (non-negative, raw lengths equal, zero-length only when both lengths are zero, decompressed length within max block size), verifies the 28-bit masked xxHash32 checksum on every non-empty block, and stops cleanly on the empty terminator block.

### Sector allocation, rewrite, clear, and close rules

#### Opening a region file
1. Verify the external-file directory exists and is a directory; otherwise fail.
2. Open the `.mca` file for read/write, creating it if absent. When synchronous chunk writes are enabled (`sync-chunk-writes=true`, the dedicated-server default), open with `O_DSYNC` (synchronous data writes).
3. Zero-initialize the in-memory 8,192-byte header and mark sectors `0` and `1` as occupied in the sector bitmap.
4. Read up to 8,192 bytes starting at file offset `0`.
5. If EOF immediately (brand-new 0-byte file), skip header scanning and **do not write an 8,192-byte header yet**.
6. If fewer than 8,192 bytes were read, log a warning (truncated header) and still scan all 1,024 slots (unread trailing bytes stay zero).
7. Query current file length `fileSize`, then for each slot `i` in `0..1023` with a non-zero location entry:
   - Extract `sectorNumber` and `numSectors`.
   - If `sectorNumber < 2`: log warning (overlaps header) and set `offsets[i] = 0` in memory.
   - Else if `numSectors == 0`: log warning (zero sector count) and set `offsets[i] = 0` in memory.
   - Else if `sectorNumber * 4096 > fileSize` (64-bit product, strictly greater): log warning (out of bounds) and set `offsets[i] = 0` in memory.
   - Else: mark sectors `[sectorNumber, sectorNumber + numSectors)` as occupied in the bitmap.
   - Header repairs are **in-memory only** until a subsequent write or clear flushes the header to disk. Overlapping sector ranges between two distinct slots are not detected or repaired; both set their bits in the bitmap.

#### Sector allocation strategy (first-fit, allocate-before-free)
- A bitset tracks occupied sector indices from `0` upward (`0` and `1` are permanently set).
- Allocating `n` sectors scans upward from sector `0` (effectively from sector `2`) for the **lowest-index** contiguous run of at least `n` clear bits (or the first clear bit beyond the highest occupied sector, extending the file), marks those `n` bits occupied immediately, and returns the starting sector index.

#### Writing a chunk (copy-on-write — never in-place)
Compressed NBT bytes are buffered in memory after a 5-byte prefix (`length` placeholder + `versionId`). When the chunk output stream closes, `length = dataSize - 4` (= `1 + compressedPayloadBytes`) is written into bytes `0..3`, and the region write proceeds exclusively:

1. Compute `sectorsNeeded = (dataSize + 4095) / 4096` (integer ceiling division by 4,096).
2. Read the chunk's existing `(oldSectorNumber, oldSectorCount)` from `offsets[index]`.
3. **Oversized-chunk branch (`sectorsNeeded >= 256`, i.e., `dataSize >= 1,044,481` bytes or `compressedPayloadBytes >= 1,044,476` bytes):**
   - **Oversized (external `.mcc`):**
     1. Log a warning.
     2. Set `sectorsNeeded = 1` and allocate `newSectorNumber` (1 sector) while the chunk's old sectors remain marked occupied.
     3. Create a temporary file in the region directory with prefix `"tmp"` and default suffix `".tmp"`, write **only** bytes `5..dataSize` (the raw compressed stream, with no 4-byte length and no 1-byte version), and close it. This file is opened for create/write **without** `O_DSYNC` or explicit `fsync`.
     4. Build a 5-byte external stub: big-endian `int32(1)` followed by `versionId | 0x80`.
     5. Write the 5-byte stub to the `.mca` file at `newSectorNumber * 4096`.
     6. Deferred commit step: rename/move the temporary file onto `c.<x>.<z>.mcc`, replacing any existing target.
   - **Normal (`sectorsNeeded <= 255`, i.e., `dataSize <= 1,044,480` bytes):**
     1. Allocate `newSectorNumber` (`sectorsNeeded` sectors) while the chunk's old sectors remain marked occupied.
     2. Write all `dataSize` bytes (5-byte chunk header + compressed payload) to the `.mca` file at `newSectorNumber * 4096`.
     3. Deferred commit step: delete `c.<x>.<z>.mcc` if it exists (cleans up a previously oversized chunk that shrank).
4. Set `offsets[index] = (newSectorNumber << 8) | sectorsNeeded` and `timestamps[index] = currentEpochSeconds` in memory.
5. Write the entire 8,192-byte header to offset `0` of the `.mca` file.
6. Execute the deferred commit step (replace `c.<x>.<z>.mcc`, or delete any stale `.mcc`).
7. If `oldSectorNumber != 0`, clear `[oldSectorNumber, oldSectorNumber + oldSectorCount)` in the sector bitmap.

Key invariants of this sequence:
- A rewritten chunk **always moves** to newly allocated sectors, even when the new size equals or is smaller than the old size.
- The previous sectors remain untouched until after the new header is written.
- The `.mca` file is **never shrunk or truncated** when trailing sectors are freed.
- For oversized chunks, the `.mca` header referencing the external stub is written *before* the temporary file is moved onto `c.<x>.<z>.mcc`.

#### Clearing (deleting) a chunk
1. Read `offsets[index]`. If `0`, do nothing (no header write, no timestamp update).
2. Set `offsets[index] = 0` and `timestamps[index] = currentEpochSeconds`.
3. Write the entire 8,192-byte header to offset `0`.
4. Delete `c.<x>.<z>.mcc` if it exists.
5. Free `[oldSectorNumber, oldSectorNumber + oldSectorCount)` in the sector bitmap.

#### Flushing and closing a region file
- **Flush:** `fsync` file data and metadata.
- **Close:**
  1. **Sector-boundary padding:** query `fileSize` and compute `paddedSize = ((fileSize + 4095) / 4096) * 4096`. If `fileSize != paddedSize`, write one zero byte (`0x00`) at byte offset `paddedSize - 1` so the file ends on an exact 4,096-byte boundary. If `fileSize == 0`, `paddedSize == 0`, so an untouched file stays 0 bytes.
  2. Always `fsync` data and metadata (even if padding fails).
  3. Always close the file descriptor (even if `fsync` fails).

### Reading a chunk and error recovery

#### Full chunk read
Under the region file lock:
1. If `offsets[index] == 0`, return missing.
2. Allocate a buffer of `numSectors * 4096` bytes and read from byte offset `sectorNumber * 4096`.
3. If fewer than 5 bytes were read, log an error (truncated chunk header) and return missing.
4. Read `length` (big-endian signed 32-bit int) and `versionByte` (unsigned 8-bit).
5. If `length == 0`, log a warning (allocated, stream missing) and return missing.
6. Let `streamLength = length - 1`.
7. If `(versionByte & 0x80) != 0` (external chunk):
   - If `streamLength != 0`, log a warning ("both internal and external streams"), and **still continue** to open the external file (even if `streamLength < 0`).
   - Let `versionId = versionByte & 0x7F`.
   - Check that `c.<x>.<z>.mcc` exists and is a regular file; if not, log an error and return missing.
   - Open `c.<x>.<z>.mcc` and pass it to version dispatch (step 9).
8. Else (internal chunk):
   - If `streamLength > actualBytesRead - 5`, log an error (truncated stream) and return missing.
   - If `streamLength < 0`, log an error (negative declared size) and return missing.
   - Slice exactly `streamLength` bytes starting at byte 5 and pass to version dispatch (step 9).
9. **Version dispatch:**
   - Look up `versionId` in `{1, 2, 3, 4, 127}`.
   - If `versionId == 127`: read one modified-UTF-8 string from the stream (an I/O error here propagates), check whether it is a syntactically valid `namespace:path` identifier, log an error either way, and return missing.
   - If unknown: log an error and return missing.
   - Otherwise wrap the stream with the version's decompressor.

#### Lightweight existence probe (used by region upgraders)
Reads only 5 bytes at `sectorNumber * 4096` (returning `false` if `offset == 0`, fewer than 5 bytes read, or any I/O error):
- External (`(versionByte & 0x80) != 0`): returns `true` iff `(versionByte & 0x7F)` is in `{1, 2, 3, 4, 127}` AND `c.<x>.<z>.mcc` is a regular file. Does **not** check `length == 1` or even `length != 0`.
- Internal: returns `true` iff `versionByte` is in `{1, 2, 3, 4, 127}`, `length != 0`, and `0 <= (length - 1) <= 4096 * numSectors`. (Upper bound is `4096 * numSectors`, not `4096 * numSectors - 5`, and file length past byte 5 is not checked.)

#### NBT encoding and decoding on chunk streams
- **Read and streaming scan:** uses **unlimited heap byte quota** (the 512 nesting-depth limit still applies). The root tag must be a named Compound; its root name is skipped without accounting.
- **Write:** serializes a named Compound with empty root name `""` through a **string-fallback wrapper**: if any string's modified-UTF-8 encoding exceeds 65,535 bytes, the write logs the error and writes an empty string `""` instead of failing the write. This same string-fallback wrapper applies to all NBT file writes (chunks, `level.dat`, player files, and auxiliary saved data).

---

## Region cache and IO worker ordering/flush semantics

### Region file LRU cache
Each region directory (`region/`, `entities/`, `poi/`) maintains its own open-file LRU cache:
- Key: packed 64-bit `(rx as u32 as u64) | ((rz as u32 as u64) << 32)`.
- Capacity: **256** entries.
- **Negative caching:** a read or scan (`create = false`) for an absent `.mca` file stores an *empty* sentinel in the LRU cache so repeated reads avoid filesystem probes. A subsequent write (`create = true`) bypasses a cached empty sentinel, creates the directory safely, opens the `.mca` file, and replaces the entry at the most-recently-used position.
- **Eviction:** when inserting exceeds 256 entries, the least-recently-used entry is evicted; if it holds an open region file, that file is closed immediately (padded, `fsync`'d, closed).
- **Flush:** `fsync`s every open region file currently in the cache.
- **Close:** attempts to close every open region file in the cache even if an earlier close fails, collecting all I/O errors and rethrowing the first with any later errors attached as suppressed.

### Storage IO worker ordering, coalescing, and flushing
Each region store runs a single serialized worker backed by a **3-level strict priority queue** (always pops from the lowest-numbered non-empty queue, FIFO within each priority):

| Priority | Tier | Operations |
|---|---|---|
| `0` (highest) | Foreground | Enqueuing chunk stores, chunk reads, streaming chunk scans, synchronization barriers, cache flushes |
| `1` | Background | Writing one buffered chunk at a time from the pending-write map to the region cache |
| `2` (lowest) | Shutdown | Final drain barrier awaited on close |

Ordered rules:
1. **Pending-write map:** an insertion-ordered map from `ChunkPos` to `(nullable CompoundTag, completionHandle)`.
2. **Store:** executes at priority `0`. Evaluates the tag supplier (which may return `null` to request chunk deletion/clearing). If `ChunkPos` is already in the pending-write map, overwrites its tag **in place** without changing its position in the insertion order and returns the **existing** completion handle; otherwise appends a new entry at the tail. Schedules a priority-`1` drain step.
3. **Read:** executes at priority `0`. If `ChunkPos` is in the pending-write map, returns a **deep copy** of the buffered tag (or empty if a pending deletion) without touching disk. Otherwise reads from the region cache. Priority `0` ensures reads never stall behind queued background writes and always observe the latest write immediately.
4. **Streaming scan:** executes at priority `0`. If `ChunkPos` is in the pending-write map, drives the visitor over the in-memory tag (or skips if pending delete); otherwise streams directly from disk with early-termination support.
5. **Background write drain:** each priority-`1` step pops the **oldest** entry from the pending-write map, writes or clears it in the region cache, completes its handle (normally or with the error), and—if more entries remain—schedules the next priority-`1` step.
6. **Synchronize (`flush` boolean):**
   - Submits a priority-`0` task that captures a join over the completion handles of all entries currently in the pending-write map.
   - Once all those writes complete: if `flush == true`, submits a priority-`0` task calling cache flush (`fsync` on all open cached `.mca` files); if `flush == false`, submits a no-op priority-`0` barrier task.
7. **Shutdown (close):**
   - Idempotent via an atomic shutdown flag. Once set, any still-queued priority-`0` tasks become no-ops (leaving their callers' handles incomplete), while priority-`1` background drains continue until empty.
   - Submits a priority-`2` barrier and blocks until it executes—guaranteeing every already-buffered write has been written to disk before closing.
   - Stops the executor and closes the region cache (logging and swallowing any close error at the worker boundary).

### Chunk scanning for world blending
To test whether any chunk within Chebyshev square `[x - range .. x + range] × [z - range .. z + range]` is an "old chunk":
- Maintains an LRU cache of up to **1,024** per-region 1,024-bit bitsets (bit index `lz * 32 + lx`).
- On cache miss, asynchronously scans all 1,024 chunks of that region via streaming scan, collecting only top-level `Int("DataVersion")` and `Compound("blending_data")`.
- A chunk's bit is set iff `DataVersion` (defaulting to `0` if absent) `< 4997` OR `"blending_data"` is present. Per-chunk scan errors are logged as warnings and leave that bit clear.

---

## Terrain, entity, and POI storage split

Each dimension operates three independent region stores sharing `sync-chunk-writes` and `region-file-compression`:

| Store | Subdirectory | Default `DataVersion` if absent | Root NBT & error/save semantics |
|---|---|---|---|
| Terrain chunks | `region/` | `-1` (data-fixing temporarily inserts `"__context": { "dimension": ..., "generator": ... }` and removes it afterward) | Full chunk schema (`xPos`, `yPos`, `zPos`, `Status`, `sections`, `block_entities`, `structures`, etc.). Full chunks omit `"entities"`; proto-chunks still embed `"entities"` inline. On load, any non-fatal exception (I/O, NBT, or parse failure) is reported and falls back to a fresh replaceable empty proto-chunk; mismatched `(xPos, zPos)` logs/reports a misplaced chunk and relocates it. Proto-chunks never overwrite an existing full chunk on disk, and empty-status proto-chunks with no valid structure starts are not written. |
| Entities | `entities/` | `-1` | `{ "DataVersion": i32, "Position": int_array[x, z], "Entities": [ ... ] }`. Maintains an in-memory set of known-empty chunks: storing an empty entity list for a chunk not in the set writes a deletion (`null`, clearing its `.mca` slot and deleting any `.mcc`) and adds it to the set; repeat empty stores are skipped. Loading an absent chunk adds it to the empty set. Mismatched `"Position"` logs/reports a misplaced chunk but still loads entities. |
| POI sections | `poi/` | `1945` | `{ "DataVersion": i32, "Sections": { "<sectionY>": { ... }, ... } }`. Dirty chunks flush incrementally per tick within time budget, explicitly right before their terrain chunk saves, and all at once on full save. Never issues `null`/clear writes; a dirtied empty chunk writes an empty `"Sections"` compound. I/O read errors are logged/reported and treated as an empty chunk; other exceptions propagate. |

Save and close ordering per dimension:
- Full save with flush: save terrain chunks (each flushing its POI chunk first) → flush all remaining dirty POI chunks → process chunk unloads → synchronize terrain worker with `flush = true`.
- Shutdown: close worldgen and light dispatchers → close POI store → close terrain store → close entity store.

---

## Session lock (`session.lock`)

### Acquisition
1. Safely create `<world>/` if missing (resolving an existing path through symlinks before directory creation).
2. Open `<world>/session.lock` for create + write (do **not** truncate).
3. Write the 3 UTF-8 bytes of U+2603 SNOWMAN (`☃`): `E2 98 83` at offset `0`.
4. `fsync` data and metadata **before** acquiring the lock.
5. Attempt a non-blocking exclusive OS file lock on the whole file.
6. If the lock is already held, close the file handle and fail with a lock error.

### Non-destructive lock probe (world list)
1. Open `<world>/session.lock` for write only (**do not create**).
2. File not found → unlocked (`false`).
3. Access denied (e.g., Windows mandatory lock held by another process) → locked (`true`).
4. Attempt non-blocking exclusive lock: if unavailable → locked (`true`); if acquired → immediately release and close, returning unlocked (`false`).

### Release and lifecycle
- World storage operations verify that the held lock is still valid and fail with an illegal-state error if it was invalidated.
- Closing world access releases the lock and closes the handle, **leaving `session.lock` on disk**.
- World zip backups skip `session.lock`.
- World deletion retries up to **5 attempts** with **500 ms** sleep between failures, skipping `session.lock` while deleting files and only closing and deleting `session.lock` during post-visit of `<world>/` itself.

---

## Safe file replacement protocol

Used for `level.dat` saves, `level.dat_old` restoration, and player `.dat` saves. Parameters: `(targetPath, newPath, backupPath, noRollback)`.

- Each phase retries up to **10 times** in an immediate tight loop (no sleep between retries). Within one attempt, steps run sequentially; if any step returns failure, the rest of that attempt aborts and the next retry restarts from step 1 of the phase.
- Moves use plain rename/move with **no overwrite or atomic-move flags**, relying on explicit deletion first for cross-platform (Windows) compatibility.

Ordered phases:
1. **Create backup** *(only if `targetPath` currently exists)* — up to 10 retries:
   1. Delete `backupPath` if it exists.
   2. Move `targetPath` → `backupPath`.
   3. Verify `backupPath` is a regular file.
   - If all 10 attempts fail, abort and return failure.
2. **Ensure target is removed** — up to 10 retries:
   1. Delete `targetPath` if it exists.
   2. Verify `targetPath` does not exist.
   - If all 10 attempts fail, abort and return failure.
3. **Install new file** — up to 10 retries:
   1. Move `newPath` → `targetPath`.
   2. Verify `targetPath` is a regular file.
   - If all 10 attempts fail:
     - If `noRollback == false`: attempt rollback (up to 10 retries: move `backupPath` → `targetPath`, verify `targetPath` is a regular file), then return failure.
     - If `noRollback == true`: skip rollback and return failure.
4. Return success.

Because phase 1 moves `targetPath` away before phase 3 moves `newPath` in, there is a brief window where `targetPath` does not exist on disk (only `backupPath` and `newPath` exist). Parent directories are not `fsync`'d; instead, temporary `.dat` files are written with `O_SYNC` (`SYNC | WRITE | CREATE | TRUNCATE_EXISTING`) before replacement starts.

---

## `level.dat` and `level.dat_old` handling

### Format and versioning
- Gzip-compressed named-root NBT compound (root name `""`) with one top-level key `"Data"` (Compound).
- `Data.version`: `19133` (Anvil). World summaries accept `19132` (McRegion, marked as requiring manual conversion) and `19133`; any other value fails with an NBT format error.
- `Data.DataVersion` and `Data.Version.Id`: `5023` in 26.3 (`Name`, `Snapshot`, `Series = "main"`). `Data.version_history` (List of Int) appends the current data version if empty or different from its last element.
- `Data.LastPlayed`: epoch milliseconds (`i64`).
- In 26.3, `level.dat` writes `"singleplayer_uuid"` (4-int UUID array) rather than an inline `"Player"` compound, and world-gen settings / game rules live in separate files under `<world>/data/`.

### Write sequence
1. Create a temporary file in `<world>/` with prefix `"level"` and suffix `".dat"` (e.g., `level<random>.dat`).
2. Write gzip-compressed `{ "Data": ... }` opened with `O_SYNC | WRITE | CREATE | TRUNCATE_EXISTING` (8,192-byte output buffer → gzip stream → 8,192-byte output buffer).
3. Run safe file replacement with `(target = level.dat, new = tempFile, backup = level.dat_old, noRollback = false)`.
4. Any exception is caught and logged as an error (not rethrown).

### Read and fallback sequence
- **Quota:** all `level.dat`, `level.dat_old`, and world-summary reads enforce the **104,857,600-byte (100 MiB) uncompressed NBT quota** plus the 512 nesting-depth limit.
- **Direct read:** checks session lock, reads `level.dat`, extracts `root["Data"]` (empty compound if absent).
- **Automatic fallback read (dedicated server startup & world optimizer):**
  1. Attempt direct read of `level.dat`.
  2. If it fails with an I/O or NBT error:
     1. Log warning for `level.dat` and info for fallback `level.dat_old`.
     2. Read `level.dat_old` (same 100 MiB quota). If this also fails, propagate the error.
     3. Restore `level.dat` from `level.dat_old` using safe file replacement with `(target = level.dat, new = level.dat_old, backup = level.dat_corrupted_<YYYY-MM-DD_HH-mm-ss>, noRollback = true)` in local wall-clock time (`uuuu-MM-dd_HH-mm-ss`).
     4. Because `level.dat_old` is **moved** onto `level.dat`, `level.dat_old` does not exist after restoration until the next save.
- **World discovery & summary scanning:**
  - A world directory is listed as a candidate iff **either** `level.dat` or `level.dat_old` exists as a regular file.
  - Summary reading inspects **only `level.dat`** (never falls back to `level.dat_old`). If `level.dat` is missing, unreadable, has a non-compound root, or fails parsing, it yields a corrupted-world summary with timestamp `mtime(level.dat)`, else `mtime(level.dat_old)`, else `-1`.
  - Summary scanning uses a streaming skip visitor that skips `Data.Player` and `Data.WorldGenSettings` compounds.
  - If `level.dat` is a symlink, it is validated against `allowed_symlinks.txt` (default: disallow all symlinks); disallowed symlinks yield a symlink-warning summary.
  - World summaries sort descending by `LastPlayed`, tie-broken ascending lexicographically by directory name.

---

## Player data files (`players/data/`)

- Directory `<world>/players/data/` is created when player storage starts.
- **Write:**
  1. Create a temporary file in `players/data/` with prefix `"<uuid>-"` (trailing hyphen) and suffix `".dat"`.
  2. Write gzip-compressed player NBT opened with `O_SYNC | WRITE | CREATE | TRUNCATE_EXISTING`.
  3. Run safe file replacement with `(target = <uuid>.dat, new = tmpFile, backup = <uuid>.dat_old, noRollback = false)`.
  4. Any exception is caught and logged as a warning.
- **Read:**
  1. If `<uuid>.dat` exists and is a regular file, read it gzip-compressed with **unlimited heap quota**. On any exception, log a warning and treat as missing.
  2. If step 1 returned no tag (missing OR corrupt):
     - If `<uuid>.dat` is a regular file, **copy** it (overwriting existing, copying file attributes) to `<uuid>_corrupted_<YYYY-MM-DD_HH-mm-ss>.dat` (note `.dat` is at the **end**, unlike `level.dat_corrupted_<ts>`). Copy errors are logged as warnings.
     - Attempt to read `<uuid>.dat_old` with unlimited heap quota. Unlike `level.dat` fallback, this does **not** replace `<uuid>.dat` with `<uuid>.dat_old`, and does **not** back up a corrupt `<uuid>.dat_old`.
  3. If loaded from either file, run player data-fixing up to the current data version.

---

## Auxiliary saved data (`data/<namespace>/<path>.dat`)

Used for both world-wide `<world>/data/` and per-dimension `<dimension>/data/`:
- **Read:** peeks the first 2 bytes via a 2-byte pushback buffer. If `(byte[1] << 8) | byte[0] == 0x8B1F` (magic bytes `1F 8B`), reads gzip-compressed NBT; otherwise reads uncompressed named-root NBT. Uses **unlimited heap quota**. Missing `DataVersion` defaults to `1343`.
- **Write:** wraps payload in `{ "data": <encoded>, "DataVersion": 5023 }`, creates parent directories, and writes directly to the target path with `O_SYNC | WRITE | CREATE | TRUNCATE_EXISTING` — **without** temporary files or `.dat_old` backups.

---

## Constants and IDs

| Constant | Value | Meaning |
|---|---|---|
| Sector size | `4096` (`0x1000`) | Bytes per region sector |
| Region header size | `8192` (`0x2000`) | 2 sectors: 1,024 offsets + 1,024 timestamps |
| Chunks per region | `1024` (`32 × 32`) | Slot index `lx + lz * 32` |
| Chunk header size | `5` bytes | 4-byte BE length + 1-byte version |
| External chunk threshold | `256` sectors | Triggered at `dataSize >= 1,044,481` B (`payload >= 1,044,476` B) |
| External stream flag | `0x80` (`128`) | Bit 7 of chunk version byte |
| Compression IDs | `1` GZip, `2` Zlib (default), `3` None, `4` LZ4, `127` Custom | Bits `0..6` of chunk version byte |
| `LZ4Block` magic | `4C 5A 34 42 6C 6F 63 6B` | ASCII `"LZ4Block"` |
| `LZ4Block` block size / level | `65536` / `6` | Default tokens `0x16` (raw) and `0x26` (compressed) |
| `LZ4Block` xxHash32 seed / mask | `0x9747B28C` / `0x0FFFFFFF` | 28-bit masked checksum |
| Region LRU cache limit | `256` | Open + negatively cached `.mca` entries per store |
| Blender scan region cache limit | `1024` | Cached 1,024-bit old-chunk bitsets per worker |
| Blender old-chunk cutoff | `DataVersion < 4997` | Or presence of `"blending_data"` |
| `session.lock` contents | `E2 98 83` | UTF-8 U+2603 (`☃`) |
| `level.dat` storage version | `19133` (Anvil); `19132` (McRegion) | `Data.version` |
| 26.3 `DataVersion` | `5023` (`"main"`) | `Data.DataVersion` / `Data.Version.Id` |
| `level.dat` NBT byte quota | `104,857,600` (100 MiB) | Uncompressed NBT quota on read |
| Safe-replace retries | `10` per phase | Immediate retries, no delay |
| Delete-world retries | `5` attempts | `500 ms` sleep between failures |
| Low disk space threshold | `67,108,864` (64 MiB) | Usable space warning threshold |
| Quarantine timestamp format | `uuuu-MM-dd_HH-mm-ss` | Local time zone, root locale |

---

## Edge cases and known quirks

1. **26.3 unified dimension and player directories:** Overworld, Nether, and End all live under `dimensions/minecraft/{overworld,the_nether,the_end}/` (not root / `DIM-1` / `DIM1`), and player saves live in `players/data/` (not `playerdata/`).
2. **Negative chunk coordinates:** Region indices use floor division (`x >> 5`), and local coordinates use bitmasking (`x & 31`, always `0..31`). Chunk `(-1, -1)` maps to `r.-1.-1.mca` slot `1023` (`lx = 31, lz = 31`), and its external file uses global coordinates: `c.-1.-1.mcc`.
3. **Never-in-place sector rewrite:** Rewriting a chunk—even with identical or smaller sector count—always allocates new sectors *before* freeing the chunk's previous sectors, so the chunk moves to a different sector range and frees its old range only after the header write and external commit complete.
4. **First-fit gap allocation and no truncation:** Allocation picks the lowest-index contiguous run of free sectors starting from sector 2. Freeing sectors at the end of the file never truncates or shrinks the `.mca` file.
5. **Exact external-file cutoff (`1,044,480` vs `1,044,481` bytes):** The 256-sector threshold is computed on `dataSize = 5 + compressedPayloadBytes`. A 1,044,480-byte record (`payload = 1,044,475`, `length = 1,044,476`) takes 255 sectors and stays inside `.mca`; a 1,044,481-byte record (`payload = 1,044,476`, `length = 1,044,477`) spills to `c.<x>.<z>.mcc`.
6. **External `.mcc` omits the 5-byte header and omits `O_DSYNC`:** The `.mcc` file holds raw compressed stream bytes only (starting at byte 5 of the chunk record), while the 1-sector `.mca` stub holds `length = 1` and `versionId | 0x80`. Shrinking an oversized chunk back to `<= 255` sectors deletes its stale `.mcc` file after updating the `.mca` header.
7. **External write crash ordering:** For oversized chunks, the `.mca` header pointing to the external stub is written *before* the temporary file is moved onto `c.<x>.<z>.mcc`.
8. **External stub with unexpected length:** When bit 7 (`0x80`) is set and `length != 0`, any non-zero `length - 1` (even negative) logs a warning but still opens and reads `c.<x>.<z>.mcc`. Conversely, `length == 0` is rejected *before* checking bit 7, returning missing even with bit 7 set.
9. **Compression ID 4 is `lz4-java` `LZ4Block` with 28-bit masked xxHash32:** It is NOT standard LZ4 Frame Format. Each block has a 21-byte header (`"LZ4Block"`, token `0x16`/`0x26`, little-endian lengths and checksum) whose checksum is `xxHash32(seed = 0x9747B28C) & 0x0FFFFFFF` (top 4 bits zeroed), incompressible blocks fall back to raw (`0x16`), and the stream ends with a 21-byte zero-length raw block.
10. **Compression ID 1 (`gzip`) and ID 127 (`custom`) asymmetry:** ID 1 is supported for reading only (not selectable in `server.properties`). ID 127 is registered in the valid-version table—so lightweight existence probes return `true` for version `127` (and `255` with an existing `.mcc`)—but full reads intercept it, read a modified-UTF-8 string, log an error, and always return missing.
11. **Zero-byte `.mca` file on untouched open or clear-of-missing:** Opening a new `.mca` file does not write the 8,192-byte header until a chunk is written or an existing chunk is cleared. Clearing a non-existent chunk in a non-existent region leaves a 0-byte `.mca` file on disk because padding of size `0` is `0`.
12. **Truncated and out-of-bounds header recovery on open:** Headers shorter than 8,192 bytes log a warning and still scan all 1,024 slots (unread bytes default to zero). Slots with `sectorNumber < 2`, `numSectors == 0`, or `sectorNumber * 4096 > fileSize` are zeroed in memory only (persisted only on a future write/clear). A slot whose start is `<= fileSize` but whose end exceeds `fileSize` passes open-time validation and is only caught on chunk read. Overlapping slots between two chunks are not detected on open.
13. **32-bit signed byte-offset limit in vanilla `.mca` I/O:** Open-time bounds checking uses 64-bit `sectorNumber * 4096`, whereas chunk read/write/probe offsets compute `sectorNumber * 4096` in signed 32-bit arithmetic and close-time padding casts file size to signed 32-bit int, capping vanilla `.mca` files at 2 GiB (`sectorNumber < 524,288`).
14. **Clearing a chunk updates its timestamp:** Clearing an existing chunk sets its location entry to `0` and writes the current epoch seconds into its timestamp slot (rather than zeroing the timestamp), deletes any `.mcc`, and frees the old sectors.
15. **Sector-boundary padding happens only on close:** Writing a chunk at EOF leaves the file unaligned to 4,096 bytes until close writes a single `0x00` byte at `paddedSize - 1`.
16. **Existence-probe upper-bound slack:** For internal chunks, the lightweight existence probe accepts `length - 1 <= 4096 * numSectors` (5 bytes more than actually fit in the allocated sectors, which a full read rejects) and does not verify that the file extends through all `length - 1` bytes.
17. **Negative caching in the region cache:** Reading or scanning an absent `.mca` caches an empty sentinel in the 256-slot LRU to avoid repeated `stat` calls; a later write must replace that empty sentinel instead of treating the region as absent.
18. **IO worker read-after-write copy, coalescing, and shutdown draining:** Foreground reads return a **deep copy** of any pending write (isolating mutation and potentially changing NBT compound map iteration order on re-serialization). Coalescing a write for an already-queued chunk replaces its payload in place without changing queue order. Shutdown drains all already-queued writes via priority `2` while dropping unexecuted priority-`0` tasks.
19. **Oversized NBT strings silently replaced with `""` on write:** All NBT file writers (chunks, `level.dat`, player files, saved data) catch modified-UTF-8 string-length overflow (> 65,535 encoded bytes) and emit `""` instead of failing the save.
20. **NBT quota asymmetry across files:** `level.dat`, `level.dat_old`, and world-summary reads enforce the 100 MiB (`104,857,600` B) uncompressed quota; chunks, entities, POI, player `.dat`, and `data/*.dat` use unlimited heap quota (all still enforce the 512 nesting-depth limit).
21. **`level.dat_old` fallback moves instead of copying, and world summaries never fall back:** Automatic fallback moves the broken `level.dat` to `level.dat_corrupted_<timestamp>` and moves `level.dat_old` onto `level.dat` with `noRollback = true` (leaving no `level.dat_old` until the next save). World-list summary loading never falls back to `level.dat_old` and instead reports a corrupted world.
22. **Player `.dat` corruption backup copies and keeps `.dat` at the end:** Unlike `level.dat_corrupted_<ts>`, player fallback **copies** `<uuid>.dat` (preserving attributes) to `<uuid>_corrupted_<ts>.dat`, leaves `<uuid>.dat` in place, reads `<uuid>.dat_old` without promoting it, and creates no backup if `<uuid>.dat_old` also fails. Temp prefixes also differ (`"level"` + `".dat"` vs `"<uuid>-"` + `".dat"`).
23. **Safe file replacement is delete-then-move, not POSIX atomic rename:** It deletes the backup, moves target → backup, ensures target is deleted, then moves new → target (10 immediate retries per phase, no overwrite/atomic flags, no directory `fsync`), rolling back backup → target only when rollback is enabled.
24. **`session.lock` `fsync`-before-lock and persistence on close:** Acquisition writes `E2 98 83` (without truncating), `fsync`s data and metadata *before* locking, and leaves `session.lock` on disk when closed. Lock probing opens write-only without creating and maps access-denied to locked and file-not-found to unlocked.
25. **Auxiliary `data/*.dat` dual-format read vs. direct non-atomic write:** Files under `data/` auto-detect gzip (`1F 8B`) vs. uncompressed NBT on read (default `DataVersion = 1343`), but always overwrite the target file directly in gzip format without temporary files or `.dat_old` rotation.

---

## Open questions
1. **2 GiB `.mca` offset behavior:** Should Zenith use 64-bit byte offsets `(sectorNumber as u64) * 4096` (supporting the full 24-bit sector range up to 64 GiB while remaining byte-identical for all files `< 2 GiB`), or cap/reject `sectorNumber >= 524,288` to mirror vanilla's 32-bit multiplication overflow?
2. **Zlib / GZip compressed-byte identity:** Rust compression backends (`miniz_oxide`, `zlib-ng`, system `zlib`) produce valid RFC 1950/1952 streams readable by vanilla, but compressed byte output can differ slightly from HotSpot's bundled zlib at level 6 (and gzip OS byte `0xFF`). Confirm whether `anvil-io` verifies decoded NBT + exact container/sector layout or requires identical compressed bytes.
3. **Transient temporary filenames:** Vanilla uses OS/JDK random stems (`tmp<rand>.tmp`, `level<rand>.dat`, `<uuid>-<rand>.dat`) that are renamed away before return. Confirm that only prefix, suffix, and same-directory placement matter for interoperability.
4. **Cross-platform safe replacement vs. POSIX `rename(2)`:** Confirm whether Zenith should mirror the exact 3-phase backup-delete-move sequence on all platforms (preserving identical `.dat_old` rotation and failure/rollback behavior) or use atomic overwrite rename where supported after rotating the backup.
5. **Pre-26.3 world layout scope:** Confirm whether `anvil-io` targets only the active 26.3 directory layout (`dimensions/<ns>/<path>/`, `players/data/`) or also needs a reader/migrator for pre-26.3 layouts (`region/`, `DIM-1/`, `DIM1/`, `playerdata/`).

## Reader review (2026-11-25)
Checked against a world freshly generated by vanilla 26.3 through the oracle: the unified layout
(`dimensions/minecraft/{overworld,the_nether,the_end}/`, `players/data/`), `session.lock` = `E2 98 83`, and the
presence of `level.dat_old` all match this spec.
