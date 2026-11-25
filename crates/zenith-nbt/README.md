# `zenith-nbt`

Clean-room NBT (Named Binary Tag) and SNBT (Stringified NBT) library for Zenith, compatible with Minecraft: Java Edition (26.3).

## Features

- **All 13 NBT tag types** (`Tag`, `TagType`), with exact IEEE-754 bit-pattern preservation for `Float` and `Double` (including NaN payloads and `-0.0`).
- **Java modified UTF-8 (`NbtString`)**: encodes `U+0000` as `0xC0 0x80`, astral characters as paired 3-byte surrogates, preserves unpaired surrogates losslessly, and enforces the 65,535-byte wire limit without truncation.
- **Java 21 `HashMap` compound order (`Compound`)**: reproduces Java `String.hashCode()` over UTF-16 units, `h ^ (h >>> 16)` spreading, power-of-two capacity (default 16, load factor 0.75), resize split order, insertion order within buckets, and copy-constructor sizing on `Clone`. Treeified bins (`> 8` colliding entries at capacity `>= 64`) are detected and reported via `Error::UnsupportedTreeifiedBin` rather than silently mis-ordered.
- **Heterogeneous `List`**: holds mixed tag types in memory and applies binary wrapper-compound encoding/decoding (`{ "": elem }`) on write and read.
- **Binary I/O**: named-root (file/region), unnamed-root (network, where `End` maps to `None`), gzip, and zlib streams.
- **Read limits (`Accounter`)**: byte-quota accounting (`DEFAULT_QUOTA = 2_097_152`, `UNCOMPRESSED_QUOTA = 104_857_600`, or unlimited) and nesting-depth enforcement (`512`).
- **SNBT**: plain single-line printer (with formatting constants isolated in `snbt::style` for parity pinning) and a recursive-descent parser for the 1.21.5+ SNBT grammar (`\x`, `\u`, `\U`, `\N{name}`, hex/binary literals, underscores, signed/unsigned suffixes, heterogeneous lists, typed arrays, `bool()`, `uuid()`).
- **Optional `serde` feature**: `to_tag` / `from_tag` for typed Rust structs and enums.

## Testing

```bash
cargo test -p zenith-nbt
cargo test -p zenith-nbt --features serde
```

Java `HashMap` iteration-order fixtures can be regenerated with JDK 21:

```bash
java tools/javamap-fixtures/GenOrder.java
```

## Fuzzing

Fuzz targets live in `crates/zenith-nbt/fuzz/` (as an isolated workspace):

- `read_named`
- `read_network`
- `snbt_parse`
- `roundtrip`
