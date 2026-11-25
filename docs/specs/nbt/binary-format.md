# NBT: binary format, limits and ordering (26.3)

- Verified by suites: `nbt-roundtrip`, fuzz targets
- Sources: public NBT format documentation; Java SE `DataInput`/`DataOutput` documentation (modified UTF-8);
  facts confirmed by local reference reading (2026-11-24); exact error text is captured by the oracle, not copied here.

## Summary
NBT is a big-endian tree of typed values. Zenith must read everything vanilla reads, reject everything vanilla
rejects (same limits, same failure points), and write byte-identical output wherever vanilla's output is
deterministic.

## Tag types
| ID | Type | Payload |
|----|------|---------|
| 0 | End | none (marks the end of a compound; also the element type of an empty list) |
| 1 | Byte | 1 byte, signed |
| 2 | Short | 2 bytes, signed |
| 3 | Int | 4 bytes, signed |
| 4 | Long | 8 bytes, signed |
| 5 | Float | 4 bytes, IEEE-754. Read: bits kept, except **−0.0 → +0.0**. **Write: any NaN becomes the canonical `0x7FC00000`** |
| 6 | Double | 8 bytes, IEEE-754. Read: bits kept, except **−0.0 → +0.0**. **Write: any NaN becomes the canonical `0x7FF8000000000000`** |
| 7 | Byte array | signed 32-bit length, then that many bytes |
| 8 | String | unsigned 16-bit byte length, then Java *modified* UTF-8 |
| 9 | List | element type ID (1 byte), signed 32-bit count, then the elements' payloads |
| 10 | Compound | a sequence of (type ID, key string, payload) entries, ended by a single End type byte |
| 11 | Int array | signed 32-bit length, then 32-bit ints |
| 12 | Long array | signed 32-bit length, then 64-bit longs |

## Strings
- Encoding is Java's modified UTF-8: U+0000 is written as the two bytes C0 80; characters outside the BMP are
  written as two 3-byte encodings of their UTF-16 surrogates; 4-byte UTF-8 sequences are never produced.
- The 16-bit prefix counts **encoded bytes**. A string whose encoding would exceed 65,535 bytes cannot be written:
  vanilla's write fails with an I/O error. It never truncates.
- Lengths in Zenith's in-memory model are measured in UTF-16 code units wherever vanilla measures string length
  (used by the byte accounting below).

## Root forms
- **Named root** (files, region chunks): type ID, a key string (usually empty), then the payload.
- **Unnamed root** (network, since the configuration-phase protocol): type ID, then the payload, with no name.
  An End type here means "no value".
- Whole-file compression: gzip for `.dat` files. Region files are covered in the storage spec.

## Compounds
- A compound is an **unordered map** from key to value, backed by Java's `HashMap`.
- **Write order is the map's iteration order**, so byte-identical output requires reproducing Java `HashMap`
  iteration order for String keys:
  - String hash = Java `String.hashCode` over UTF-16 code units; bucket hash = `h XOR (h >>> 16)`;
    bucket index = hash AND (capacity − 1);
  - capacity is a power of two (default 16, load factor 0.75), doubling when the size exceeds capacity × 0.75;
    on resize, entries keep their relative order within each split bucket;
  - within a bucket, entries iterate in insertion order (bins of 8+ entries become trees once capacity ≥ 64,
    which changes in-bin order; this is reproducible but practically never reached by vanilla data);
  - **copying** a compound builds a fresh map sized from the entry count, so a copy can iterate in a different
    order than its source (for example when the source grew and then had entries removed). Zenith copies must
    behave the same way.
- Reading: a repeated key replaces the earlier value, and the replacement is not charged as a new entry (below).

## Lists
- The written element type is derived from the contents at write time:
  - empty list → End (0);
  - all elements of one type → that type;
  - mixed types → Compound (10), with **wrapping**: each non-compound element is written as a compound with exactly
    one entry whose key is the empty string. A compound element is written unchanged, *unless* it already has that
    shape (exactly one entry, empty key), in which case it is wrapped again so it survives the round trip.
- Reading a list of compounds unwraps every element that has the wrapper shape. A list read from bytes can
  therefore become heterogeneous in memory, and it re-serializes identically.

## Limits (read side)
- **Depth:** at most 512 nested compounds/lists. Entering the 513th level fails the read.
- **Byte quota:** every read charges an estimated *in-memory* size against a quota:
  - default quota 2,097,152 (network payloads and most reads);
  - 104,857,600 for uncompressed file reads;
  - unlimited for specific trusted internal paths (documented per caller in later specs).
- **Charges:**

| Item | Charge |
|------|--------|
| End | 8 |
| Byte | 9 |
| Short | 10 |
| Int | 12 |
| Float | 12 |
| Long | 16 |
| Double | 16 |
| String value | 36 + 2 × (UTF-16 length) |
| Byte array | 24 + 1 × length |
| Int array | 24 + 4 × length |
| Long array | 24 + 8 × length |
| List | 36 + 4 × count, plus its elements |
| Compound | 48, plus per key 28 + 2 × (key UTF-16 length), plus 36 per *new* key (not charged when a key repeats) |

- Exceeding the quota or the depth fails the read with an accounting error. Unknown type IDs fail with a format
  error. Negative array or list lengths fail. Exact error text comes from oracle fixtures.

## Edge cases to cover in tests
1. Mixed-type lists, lists containing wrapper-shaped compounds, and empty lists.
2. Strings with U+0000, lone surrogates, astral characters, and exactly 65,535 / 65,536 encoded bytes.
3. Compounds grown past 12, 24 and 48 entries (resize points), and copies of compounds after removals.
4. Depth 512 vs 513; quota boundaries hit by both a single large array and many small tags.
5. **Negative zero becomes +0.0 on read** (the game creates float/double tags through a factory that shares one zero
   instance for any value equal to zero); NaN payloads and signs are kept in memory after a read but
   **canonicalized on write** (correction 2026-11-25, verified with the oracle: signaling, negative and payload NaNs
   all come back canonical from vanilla's writer).

## Open questions
1. **Root name accounting in named-root reads:** Does vanilla's `read_named` charge the `Accounter` for the root tag's name string (e.g. `28 + 2 × utf16_len`), or skip/read it uncharged? Current implementation does not charge the root name (matching the table, which only lists the 13 tag types and compound keys).
2. **All-compound lists containing a wrapper-shaped element:** When a `List` contains only `Compound` elements and one of them already has the wrapper shape (`{ "": value }`), does vanilla wrap that element to `{ "": { "": value } }` on write so unwrapping on read restores the original compound? Current implementation wraps wrapper-shaped compounds whenever the wire list element type is `Compound` (both mixed lists and all-compound lists) to preserve round-trip fidelity.
3. **Accounting charges for wrapper compounds in binary lists:** When reading a list with wire element type `Compound`, does vanilla charge the full `Compound` overhead (`48 + 28 + 36` + inner payload) and count `+1` nesting depth for each wrapper compound before unwrapping it? Current implementation reads each element as a normal `Compound` (charging and pushing depth) before unwrapping.
4. **Terminating `End` byte inside compounds:** Confirmed assumption to verify against oracle: the terminating `0` byte at the end of a compound does not charge the standalone `End` (`8`) fee; `End` (`8`) is only charged when `End` appears as a root tag.
5. **Treeified-bin iteration order:** When a compound bin exceeds 8 colliding entries at capacity ≥ 64, vanilla converts the bin to a red-black tree (`HashMap.TreeNode`), changing in-bin iteration order. Current implementation detects and reports this state (`Error::UnsupportedTreeifiedBin`) rather than silently mis-ordering. Confirm whether parity ever requires reproducing `TreeNode` iteration order.


## Answers from the reader (2026-11-25, local reference reading)
- **Q1 root name accounting:** the root tag's name is skipped without being charged. Only the payload is charged.
- **Q2 all-compound lists:** yes. Whenever the written element type is Compound (mixed lists *and* lists made only of
  compounds), any element that has the wrapper shape is wrapped again on write.
- **Q3 wrapper accounting:** a wrapper is read as an ordinary compound (base charge, key charge, entry charge, one
  extra depth level), and only then unwrapped. So the charges and depth for wrappers are real.
- **Q4 compound terminator:** the End byte that closes a compound is not charged. The End charge (8) applies only
  when an End tag is read as a value.
- **Q5 treeified bins:** not reachable from vanilla-generated data, but reachable from crafted network input. Keep
  detection plus a clear error for now. Exact tree-bin order becomes required only if a parity scenario exercises it.
- **New rule:** a list whose element type is End but whose count is greater than zero is a format error. An End-typed
  list with count 0 is the normal empty list.
