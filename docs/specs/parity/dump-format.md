# Parity dump format `ZPD1`

Zenith's own format (not a game format). Both sides write it: the oracle agent inside vanilla (Java) and Zenith
(Rust, the `parity-dump` feature). All integers are big-endian.

```
file    := "ZPD1" header record*
header  := str source | str version | str scenario | u64 seed
record  := u32 body_length | body
body    := u64 tick | u16 phase | str subject | u16 field_count | field*
field   := str key | u8 type | value
type    := 0 i64 (8 bytes) | 1 f64 (8 bytes, raw IEEE bits) | 2 bool (1 byte) | 3 str | 4 bytes (u32 length + data)
str     := u16 byte length | UTF-8 (standard UTF-8, not Java's modified form)
```

## Phases
| Value | Phase | Written |
|-------|-------|---------|
| 0 | `tick_start` | before a server tick |
| 1 | `tick_end` | after a server tick |
| 10 | `chunk_generated` | when a chunk reaches full status |
| 20 | `packet_out` / 21 `packet_in` | network bytes |
| 30 | `entity_state` | per-entity state snapshot |

## Comparison rules
- Records are compared in order. The first mismatch in tick, phase, subject, field set or value is **the** divergence.
- Floats compare by raw bits (so −0.0 ≠ 0.0, and NaN payloads matter), because 1:1 means bit-identical.
- Records may only be skipped through an explicit, documented filter (e.g. ignoring wall-clock fields).
