# NBT: SNBT text form (26.3)

- Verified by suites: `snbt-format` (oracle-captured printer outputs + parse round trips), fuzz targets
- Sources: public SNBT documentation (the 1.21.5+ syntax), local reference reading for the rules below

## Printer (the plain, single-line form)
- **Keys** print bare when they start with a letter, `.` or `_`, continue with letters, digits, `.`, `_`, `+`, `-`,
  and are not `true` or `false` (case-insensitive). Every other key is quoted using the string rule below.
- **Strings:** the quote character is chosen from the content. If the first quote-like character in the string is
  `"`, single quotes are used; if it is `'`, double quotes are used; if there is none, double quotes are used.
  Backslashes are always escaped. The chosen quote character is escaped wherever it appears, and the other one is
  left alone.
- **Control characters:** backspace → `\b`, tab → `\t`, line feed → `\n`, form feed → `\f`, carriage return →
  `\r`, and any other character below U+0020 → `\x` plus two hex digits.
- **Typed arrays** start with `[B;`, `[I;` and `[L;` respectively.
- Separator spacing, number suffixes and float formatting are pinned by **oracle-captured fixtures** (generated at
  test time from the user's game), not transcribed here. There is also a pretty-printed multi-line form used by
  some commands; it is pinned the same way.

## Parser (1.21.5+ grammar)
Implement from the public SNBT documentation: unquoted strings, single- and double-quoted strings with escapes
(including `\x`, `\u`, `\U`, `\N{name}`), booleans as bytes, integer suffixes with signed/unsigned variants,
hexadecimal and binary literals, underscores in numbers, scientific notation, typed arrays, heterogeneous lists,
and the built-in operations such as `bool(...)` and `uuid(...)`. Every accepted and rejected input class must be
covered by the parity fuzzer, which compares Zenith's accept/reject decision and resulting tree with vanilla's
(through the oracle).

## Open questions
1. **Compound key order in plain single-line printer:** Does vanilla's single-line SNBT printer iterate compound entries in `HashMap` bucket order (as implemented here) or sorted key order?
2. **Unquoted keys starting with `.` followed by digits (e.g. `.5`):** The spec states keys print bare when starting with a letter, `.` or `_`. Confirm whether vanilla quotes keys like `.5` (which look like floats in value position) or prints them bare.
3. **Lone UTF-16 surrogates in SNBT:** How should the SNBT printer format `NbtString` values containing unpaired UTF-16 surrogates (currently escaped as `\uXXXX`), and does the 1.21.5+ SNBT parser reject lone surrogates in `\uXXXX` / `\UXXXXXXXX` escapes (currently rejected as invalid Unicode scalar values)?
4. **Boolean keyword case sensitivity in parser:** In the 1.21.5+ parser, are `True` / `FALSE` parsed as unquoted strings (as implemented here — only lowercase `true`/`false` are booleans) or as case-insensitive booleans?
5. **Signed prefixes on hex/binary and unsigned literals:** Confirm whether vanilla rejects negative signs on default-unsigned hex/binary literals (e.g. `-0x1`, `-0b1`) unless an explicit signed suffix (`-0x1si`) is given, and whether `+` is allowed on unsigned literals (e.g. `+5ub`, currently allowed) and `-0ub` is rejected (currently rejected).
6. **Additional built-in operations & unsupported corners:** Current parser implements `bool(...)` and `uuid(...)`. Confirm whether 26.3 defines any additional SNBT built-in operations, whether non-hyphenated UUIDs are accepted by `uuid(...)`, whether `bool(NaN)` / non-finite floats are possible via operations, and whether whitespace inside typed-array headers (`[ B ; 1b ]`) is accepted by vanilla.
7. **Unicode name aliases in `\N{...}`:** Current parser resolves `\N{name}` via `unicode_names2` (case-insensitive official Unicode character names). Confirm whether vanilla also accepts Java `Character.codePointOf` aliases/control-character names (e.g. Unicode NameAliases like `BEL` or `LINE FEED`) beyond primary Unicode names.


## Answers from the reader (2026-11-25, local reference reading)
- **Q1 key order in the single-line printer:** compound entries are printed **sorted by key**, using Java's natural
  string order, which compares **UTF-16 code units**. This differs from Rust's `str` ordering for characters above
  U+FFFF, which are encoded as surrogates (0xD800–0xDFFF) and therefore sort *before* U+E000–U+FFFF. Sort by
  UTF-16 units. (The binary format still writes in hash-map order. Only the text form sorts.)
- **Multi-line pretty printer:** it also sorts keys, except that for a few well-known nested paths (e.g. structure
  template layouts) it lists a fixed set of preferred keys first, in a fixed order, and then the remaining keys sorted.
  That path table is pinned by oracle fixtures in the M2 parity phase.
- **Q2 keys like `.5`:** printed bare. The bare-key rule only requires the first character to be a letter, `.` or
  `_`.
- **Q3 lone surrogates:** the printer does not escape them; they are emitted as the raw UTF-16 unit. Parser
  acceptance of lone-surrogate escapes is settled by the oracle differential fuzzer (below).
- **Q4 booleans:** `true` and `false` are recognised **case-insensitively** by the parser (`TRUE`, `False`, … are
  booleans, i.e. bytes 1/0). The printer quotes any key that equals either word case-insensitively.
- **Q5–Q7 (numeric sign rules, extra operations, whitespace in array headers, `\N{}` aliases):** resolved
  empirically. The M2 parity phase adds an oracle **differential SNBT fuzzer**: random inputs go to both vanilla's
  parser (through the oracle) and Zenith's, and every accept/reject decision and resulting tree must agree. That is
  more reliable than transcribing the grammar.
