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
