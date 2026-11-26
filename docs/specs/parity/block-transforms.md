# Block state transform dump (`ZBT1`)

Specification for `dev.zenith.oracle.BlockTransforms` (`tools/oracle/src/dev/zenith/oracle/BlockTransforms.java`), the
exhaustive per-block-state oracle for rotation and mirror behaviour (see ADR 0009 and the rules in
[`docs/specs/blocks/state-transforms.md`](../blocks/state-transforms.md)).

After bootstrapping the game's static registries (`SharedConstants.tryDetectVersion()`, `Bootstrap.bootStrap()`; no
tag binding is needed, no rule reads a tag), `BlockTransforms` streams, for every `BlockState` in
`Block.BLOCK_STATE_REGISTRY` global-id order (`0..total_states`), the global id of the state obtained by each of the 12
(mirror, rotation) combinations, with bounded memory.

---

## Invocation

```text
java -Xmx2G -cp <game_classpath>:<zenith-oracle.jar> dev.zenith.oracle.BlockTransforms <output_file> [start_id end_id_exclusive]
```

- Run from a cache working directory (e.g. `~/.cache/zenith/parity/cwd`) so the game logger does not write `logs/` into
  the repo.
- With 1 argument (`<output_file>`), dumps all block states `[0, total_states)`.
- With 3 arguments (`<output_file> <start_id> <end_id_exclusive>`), dumps only global ids in
  `[start_id, end_id_exclusive)` for debugging. Result ids are always global ids and may lie outside the range.
- Exits with status 2 on a usage error, with status 1 and a message on `stderr` if the range is out of bounds,
  bootstrap or reflection fails, a transform returns a state of a different block, or a transform returns an
  unregistered state.
- On success prints one line to `stdout`:
  `[zenith-oracle] block transforms: <records>/<total> states, identity mismatches <n>, states changed: <MIRROR>+<ROTATION>=<count> … in <ms> ms`
  (the 11 non-identity combinations, counts of states whose result id differs from their own id; `identity mismatches`
  counts states whose identity combination did not return the state itself and is expected to be `0`).

---

## Primitive encodings

All multi-byte integers are **big-endian** (matching `java.io.DataOutputStream`, `ZPD1`, `ZTP1`, `ZTC1`, `ZBF1`).

| Type | Size | Encoding |
|---|---|---|
| `u16` | 2 bytes | Unsigned 16-bit big-endian integer |
| `u32` | 4 bytes | Unsigned 32-bit big-endian integer (every value written is non-negative and below `total_states`) |

---

## File layout

### Header (18 bytes fixed)

```text
magic        : [u8; 4] = b"ZBT1"       // 0x5A 0x42 0x54 0x31
version      : u16     = 1
total_states : u32                     // Block.BLOCK_STATE_REGISTRY.size()  (35,723 in 26.3)
start_id     : u32                     // first global block-state id in this file
state_count  : u32                     // number of records that follow (end_id - start_id)
```

Followed immediately by `state_count` records in strictly ascending `global_id` order
(`start_id .. start_id + state_count`), with no trailing bytes. The file size is exactly
`18 + 52 × state_count` bytes (1,857,614 bytes for the full 26.3 table).

### Record (52 bytes fixed)

```text
global_id : u32                        // the state's id in Block.BLOCK_STATE_REGISTRY
result    : [u32; 12]                  // result[m * 4 + r], m in 0..3, r in 0..4
```

`result[m * 4 + r]` is the global id of `state.mirror(M[m]).rotate(R[r])` — **mirror applied first, then rotation**,
the order structure placement uses (spec §5) — where:

| Index | `m` | Mirror | | Index | `r` | Rotation |
|---:|---:|---|---|---:|---:|---|
| 0 | 0 | `NONE` | | 0 | 0 | `NONE` |
| 1 | 1 | `LEFT_RIGHT` | | 1 | 1 | `CLOCKWISE_90` |
| 2 | 2 | `FRONT_BACK` | | 2 | 2 | `CLOCKWISE_180` |
| | | | | 3 | 3 | `COUNTERCLOCKWISE_90` |

So `result[0]` is the identity (must equal `global_id`), `result[1..3]` are the three rotations alone,
`result[4]` and `result[8]` are the two mirrors alone, and the other six entries are the combined mirror-then-rotate
results. The mirror is evaluated once per `(state, m)` and each of the four rotations is applied to that same mirrored
state.

Both operations are the game's own `BlockState#mirror(Mirror)` and `BlockState#rotate(Rotation)`; the dump contains
no names, properties or flags, only ids. A reader that wants property-level information joins on the id with
`reports/blocks.json` (or the `block-facts` dump's identity section).

### Per-record validity checks the oracle enforces

- every result state is the same block as the record's state;
- every result id is a registered global id (`0 <= id < total_states`);
- the identity entry equals `global_id` (violations are counted and reported in the summary line, not an error).

---

## Size, timing and self-checks (26.3)

- `total_states` is 35,723 and equals the data generator's `blocks.json` state total. The full file is 1,857,614
  bytes.
- Wall time is about 4.5 s including the game bootstrap; the output is deterministic: two runs are
  byte-identical.
- Counts of states whose result differs from their own id (the numbers also printed by the summary line):
  `NONE+CLOCKWISE_90` 30,206; `NONE+CLOCKWISE_180` 28,876; `NONE+COUNTERCLOCKWISE_90` 30,206; `LEFT_RIGHT+NONE` 18,668;
  `LEFT_RIGHT+CLOCKWISE_90` 28,858; `LEFT_RIGHT+CLOCKWISE_180` 21,784; `LEFT_RIGHT+COUNTERCLOCKWISE_90` 28,858;
  `FRONT_BACK+NONE` 18,668; `FRONT_BACK+CLOCKWISE_90` 28,858; `FRONT_BACK+CLOCKWISE_180` 20,232;
  `FRONT_BACK+COUNTERCLOCKWISE_90` 28,858; identity mismatches 0.
- A decoder reading this document must consume the file exactly (header + records, no trailing bytes); a Rust reader
  should assert that.
- Laws that hold for every state in 26.3 and make good decoder self-checks: rotation columns compose
  (`result[r1]` then rotating by `r2` gives `result[(r1 + r2) mod 4]`), each mirror is an involution
  (`result[4 * m]` applied twice returns the state), every combined entry equals the rotation-only column of the
  mirrored state, and the table is closed (all ids are valid states of the same block).
