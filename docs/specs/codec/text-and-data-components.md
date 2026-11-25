# Codec: text components and item data components (26.3)

- Verified by suites: `text-component`, `datapack-load`
- Sources: public data-pack / text-component documentation (the 1.21.5+ snake_case format); the game's data-generator
  report `reports/minecraft/components/item/*.json` (per-item default components, ingested by Zenith);
  `reports/registries.json` (`minecraft:data_component_type`, 122 entries); behaviour of the public, open-source
  serialization library the game bundles (DataFixerUpper 10.0.21, "DFU") and of Gson 2.14.0; local reference reading
  (2026-11-25). Everything below is written in our own words. Facts marked **[oracle]** must be pinned by an
  oracle-captured fixture before an implementation is called done.
- Related specs (not repeated here): `nbt/binary-format.md` (heterogeneous-list wrapping, NBT quotas and depth),
  `nbt/snbt.md` (SNBT grammar used by commands and string-encoded custom data),
  `registry/identifiers-registries-tags.md` (identifier parsing, holder / holder-set JSON and wire forms, registry IDs).

## Summary

A **text component** is a tree. Every node has exactly one *content* (literal text, a translation, a key binding, a
scoreboard score, an entity selector, an NBT query, or an inline sprite "object"), a *style* (tri-state formatting
flags, colour, shadow colour, font, insertion text, click and hover events) and an ordered list of child components
called *siblings* (`extra`). Styles inherit down the tree field by field. Components are stored in NBT (item
components, block entities, entities, `level.dat`), read from JSON (data packs, server-list status, login disconnect)
and SNBT (commands), and sent on the network as NBT. Some contents (score, selector, nbt, object) only produce their
final text after a server-side **resolution** pass.

An **item data component** is a typed, registry-identified value attached to an item stack. Each item type has a
*prototype* map of default components; a stack stores only a sanitized *patch* against that prototype (added /
changed values and explicit removals). Patches serialize as a map keyed by component id with a `!` prefix for
removals, and on the network as counted lists of typed values. 26.3 defines 122 component types; 119 are persistent
(saved) and 3 are transient (network only).

---

# Part A — Text components

## A.1 Model and inputs

- **Content** — one of the seven kinds in A.2.
- **Style** — eleven optional fields (A.3). An entirely unset style is the *empty style*; decoding always canonicalizes
  an all-unset style to the empty style (this matters for the bare-string encoding rule in A.6).
- **Siblings** — ordered list of child components. Siblings render after the node's own content and inherit the
  node's *resolved* style (A.3.2).
- **External state used when rendering:** the active language table (client: selected language layered over
  `en_us`; dedicated server: built-in `en_us` only), the client key-binding table (keybind contents), and, for
  resolution only, a command source (level, executing entity, position, permission) and the scoreboard / command
  storage.
- **Registry context used when decoding** (A.6.4): which registries are available to check references by id.

## A.2 Content types

### A.2.1 Selection when decoding a map

1. If the map contains a `type` key, its value must be one of `text`, `translatable`, `keybind`, `score`, `selector`,
   `nbt`, `object` (case-sensitive). Only that kind's fields are decoded; if they fail, the component fails. There is
   no fallback to guessing.
2. Otherwise the kinds are tried **in the fixed order** `text`, `translatable`, `keybind`, `score`, `selector`, `nbt`,
   `object`, and the first kind whose fields decode *successfully* wins. Keys belonging to other kinds are then simply
   ignored. If no kind matches, decoding fails ("no matching codec").
3. Encoding never writes `type`.
4. The same "explicit discriminator or ordered guess" scheme is reused one level down: the NBT data source (key
   `source`: `entity`, `block`, `storage`, tried in that order) and the object info (key `object`: `atlas`, `player`,
   tried in that order).

### A.2.2 Kinds, fields and behaviour

| Kind | Fields (required **bold**) | Unresolved plain text | Resolution (A.9) |
|---|---|---|---|
| `text` | **`text`**: string | the string | unchanged |
| `translatable` | **`translate`**: key string; `fallback`: string (lenient — a wrong type is ignored); `with`: list of arguments (strict) | formatted template (A.7) | each component argument is resolved; primitives kept |
| `keybind` | **`keybind`**: key-mapping name, e.g. `key.jump` | server: the name itself; client: the bound key's display name, or, for an unknown name, the name looked up as a translation key | unchanged |
| `score` | **`score`**: object with **`name`** (string) and **`objective`** (string) | *nothing* (empty) | becomes the formatted score text, or empty |
| `selector` | **`selector`**: entity selector string; `separator`: component (strict) | the selector source text, verbatim | becomes the list of matched entity display names |
| `nbt` | **`nbt`**: NBT path string; `interpret`: bool (lenient, default false); `plain`: bool (lenient, default false); `separator`: component (lenient); **one data source** | *nothing* (empty) | becomes the rendered NBT values |
| `object` | object info (below); `fallback`: component (strict) | the fallback's text, else the default fallback (below) | validated against the context's object filter |

Per-kind validation and details:

- **`with` arguments.** Each list element is decoded first as a *primitive* (a number, a boolean or a string) and,
  only if that fails, as a component. A component argument that is plain text with no style and no siblings is
  stored as a plain string. Encoding writes primitives in their native type and component arguments as components.
  An empty `with` list is accepted and is equivalent to no arguments; encoding omits `with` when there are none.
  Lists and maps are never primitives (they always go to the component path), so `["a","b"]` as an argument is a
  component with a sibling.
- **Score `name`.** First tried as an entity selector (full selector grammar, selectors allowed, the whole string must
  be consumed); if that fails it is kept as a plain name. So player names up to 16 characters and UUIDs parse as
  selectors; `*`, names longer than 16 characters, and strings with spaces become plain names. Both forms re-encode as
  the original string.
- **`selector` / `entity` source** — must parse completely with the entity-selector grammar (selectors allowed), else
  the component fails with an "invalid selector" error.
- **`nbt` path** — must parse completely with the command NBT-path grammar. `interpret` and `plain` both true is a
  decode error. Parsing reads nodes until the end of the string or a space; anything left (a space and whatever
  follows) is a "trailing data" error. So the **empty path `""` is valid**: it has zero nodes and selects the source's
  root tag itself, while `" a"` and `"a b"` fail. Data sources: `entity` (selector string), `block` (block-position argument string, relative and local
  coordinates allowed), `storage` (identifier).
- **`object` info.**
  - `atlas`: `atlas` identifier, default `minecraft:blocks` (omitted on encode when equal to the default); **`sprite`**
    identifier. Encoding writes `atlas` (when present) before `sprite`. Default fallback text: `[<sprite>]` when the atlas is `minecraft:blocks`, else
    `[<sprite>@<atlas>]`, where each identifier is printed without its namespace when the namespace is `minecraft`.
  - `player`: **`player`** profile (same shape as the `profile` item component, B.7); `hat` bool, default true
    (omitted when true). Default fallback text: `[<name> head]`, or `[unknown player head]` when the profile has no
    name.
  - **Styled rendering** of an object emits a single U+FFFC character whose style's font is replaced by a special
    sprite font (atlas sprite or player head). Plain-text flattening uses the fallback instead (A.8).

## A.3 Style

### A.3.1 Fields

| Key | Value | Decode rules | Encode |
|---|---|---|---|
| `color` | string | one of the 16 names in *Constants* (case-sensitive), or `#` followed by a hexadecimal number in `0`…`FFFFFF` | name if the colour was created from a name, else `#RRGGBB` (6 upper-case digits) |
| `shadow_color` | ARGB integer, or a list of exactly four floats `[r, g, b, a]` | integer used as-is (signed 32-bit). Floats: each channel becomes floor(value × 255) masked to 8 bits, so out-of-range values wrap rather than clamp | always the integer form |
| `bold`, `italic`, `underlined`, `strikethrough`, `obfuscated` | boolean | tri-state: unset / true / false | written only when set; an explicit `false` is preserved |
| `click_event` | object (A.4) | | |
| `hover_event` | object (A.5) | | |
| `insertion` | string | text inserted into the chat box on shift-click | |
| `font` | identifier | any identifier; unset means `minecraft:default` when rendering | the identifier (sprite fonts from `object` contents are render-time only and never serialized) |

Hex colour parsing details (the number is read as a signed base-16 integer):
- A leading `+` is accepted (`#+FF` is `#0000FF`); a leading `-` yields a negative number, rejected as out of range.
- Any digit count is accepted as long as the value fits: `#F` → `#00000F`; `#0000FF00` → `#00FF00`; `#1000000` is out
  of range; values beyond the signed 32-bit range are "invalid colour value".
- Lower-case digits are accepted. Unicode decimal digits and full-width Latin `A`–`F` also count as digits
  (Java digit rules) **[oracle]**.
- Colour equality compares only the RGB value: `"red"` and `"#FF5555"` make equal styles, but re-encode differently.

### A.3.2 Inheritance and merging

- When a component is visited with a parent style, its effective style takes each field from its own style if set,
  else from the parent's (all eleven fields, including click, hover, insertion, font and shadow colour). Siblings and
  translation arguments receive the effective style as their parent. The root's parent is the empty style unless the
  caller supplies one (e.g. item lore lines are rendered over dark purple + italic; custom item names are made italic,
  see B.6).
- Unset booleans render as false; unset colour renders in the context's default colour.

## A.4 Click events (`click_event`)

The object has an `action` key plus action-specific keys (the pre-1.21.5 `clickEvent`/`value` keys are **not**
recognised — they are unknown keys and are silently ignored, leaving no click event).

| `action` | Fields | Validation |
|---|---|---|
| `open_url` | **`url`** | must parse as a URI with a scheme, and the scheme (case-insensitive) must be `http` or `https` |
| `open_file` | **`path`** | **never accepted in serialized data**: decoding and encoding both fail ("not allowed"). Exists only in-memory on the client (e.g. screenshot messages) |
| `run_command` | **`command`** | every UTF-16 unit must be a chat-safe character: not `§` (U+00A7), not below U+0020, not U+007F |
| `suggest_command` | **`command`** | same as `run_command` |
| `show_dialog` | **`dialog`** | an inline dialog definition (decodes in any context; its schema, defaults and key order belong to the future dialog spec, so inline dialogs are a **known gap** of the `text-component` suite, 2 cases), or a dialog identifier, which must name an entry of the dialog registry and so needs registry context (A.6.4); without it, an identifier always fails |
| `change_page` | **`page`** | integer ≥ 1 |
| `copy_to_clipboard` | **`value`** | any string |
| `custom` | **`id`** identifier; `payload` any NBT value (optional) | in JSON the payload is converted to NBT |

Action names are case-sensitive.

## A.5 Hover events (`hover_event`)

| `action` | Fields | Notes |
|---|---|---|
| `show_text` | **`value`**: component | resolved during server-side resolution (A.9) |
| `show_item` | **`id`**: a registered item id (not `minecraft:air`; an unknown id fails); `count`: 1…99, default 1 (omitted when 1); `components`: component patch (B.4) | the item fields are inline in the hover object |
| `show_entity` | **`id`**: a registered entity type id (an unknown id fails); **`uuid`**: four-integer array or hyphenated UUID string; `name`: component | tooltip lines: the name (if any), then `gui.entity_tooltip.type` with the type's name, then the UUID in lower-case hyphenated form |

The old `contents` key is not recognised.

## A.6 Encodings

### A.6.1 Decoding (JSON and NBT, same rules)

1. **String** → literal component with that text (the empty string gives the empty literal).
2. **Non-empty list** → each element is decoded as a component; the result is a copy of the first element with every
   later element appended as an extra sibling (after the first element's own siblings). Consequence: the first
   element's style is inherited by all later elements (`[{"text":"a","color":"red"},"b"]` renders both red;
   `["", …]` is the usual way to avoid that).
3. **Empty list** → decode error.
4. **Map** → content (A.2.1) + `extra` + style fields, all read from the same map. `extra`, if present, must be a
   **non-empty** list of components; `extra: []` is a decode error that fails the whole component.
5. Any other value (number, boolean, null) → decode error.
6. Unknown keys anywhere are ignored.
7. **Strict vs lenient optional fields.** An optional field that is *absent* takes its default. An optional field that
   is *present but invalid* fails the whole component, except for the lenient fields: translatable `fallback`, and nbt
   `interpret`, `plain`, `separator` — those are treated as absent.
8. **NBT specifics.** Strings must be string tags (a number never decodes as a string). Booleans accept *any numeric
   tag* and are true iff the value is non-zero when read as a double (`1b`, `5`, `0.5f` are true; `0`, `0.0d` false;
   a string `"true"` fails). Lists may be heterogeneous (strings mixed with compounds) per `nbt/binary-format.md`.
9. **JSON specifics.** JSON numbers are not accepted where a string is expected. Where a boolean is expected only
   JSON `true` / `false` are accepted; **a JSON number is rejected** (`"bold": 1` fails the component). This differs
   from NBT (rule 8), where numbers are the only booleans. *(Settled by the `text-component` suite, 2026-11-25.)*
   JSON documents nested deeper than the parser's limit fail to parse (opening a 256th nested container fails, see
   `datapacks-and-codecs.md`).

### A.6.2 Encoding

1. If the content is literal text, there are no siblings and the style is empty → a **bare string**.
2. Otherwise a **map**: the content's own fields (no `type`), then `extra` (only if there are siblings), then each
   *set* style field. Optional fields equal to their default are omitted (A.2.2, A.3.1).
3. The list form is never produced by encoding.
4. **Key order in JSON output** (settled by the `text-component` suite, 2026-11-25): fields are written in the order
   listed for each type in this spec, with two rules that override it:
   - **Dispatched objects write their own fields first and the discriminator key last.** `click_event` and
     `hover_event` objects end with `action`: `{"url":"…","action":"open_url"}`,
     `{"id":"minecraft:stone","count":5,"action":"show_item"}`, `{"value":…,"action":"show_text"}`. (The content
     kind's `type` key and the `source` / `object` sub-discriminators are never written, A.2.1.)
   - The `atlas` object writes `atlas` before `sprite` (A.2.2).
   NBT compounds carry no meaningful order (SNBT printing sorts keys, see `nbt/snbt.md`), but the network/binary NBT
   form writes entries in insertion order, so the same order applies there.

### A.6.3 Where each encoding is used

| Context | Format | Limits |
|---|---|---|
| Item components, block entities, entities, saved data | NBT | NBT depth / quota rules |
| Commands (`tellraw`, `title`, item/component arguments, …) | SNBT parsed to NBT, then decoded as NBT | — |
| Data packs (advancements, dialogs, item modifiers, …) | strict JSON | — |
| Network (chat, titles, item names, …) | NBT, nameless root tag, decoded with registry access | default quota (2,097,152 bytes, depth 512) for untrusted paths; some server→client paths use the unlimited quota |
| Login disconnect reason | lenient JSON string, at most 262,144 characters | — |
| Written book pages | NBT / network, plus a size rule | a page is rejected on decode if its compact JSON encoding exceeds **32,767 characters** (the check converts the page to JSON even when decoding NBT) |

### A.6.4 Registry context

Decoding takes a **registry context** that answers, per registry, whether it is available and whether an id is
registered in it.
1. **Static registries** (fixed by the game version: items, entity types, blocks, …) are always available.
2. **Dynamic registries** (loaded from data packs: dialogs, enchantments, …) are available only when the caller
   decodes with registry access. In the game that is every real path (network, commands, saved data, data packs);
   a context without it exists only in tools.
3. A reference by id to an entry that is not registered fails. A reference by id into a registry that is not
   available fails ("registry does not exist"). An inline definition (where the field allows one, e.g. an inline
   dialog) decodes in any context.
4. The `text-component` parity suite decodes with full registry access: the game's built-in static registries plus
   the built-in world registries of the version (the same entries the version's own data files describe).

## A.7 Translation lookup and argument formatting

### A.7.1 Lookup

1. Template = the language table's value for the key; if missing, the `fallback` string if one is set, else the key
   itself.
2. **Lang-file rewrite at load time:** in every lang-file value, each occurrence of `%`, an optional `N$`, any run of
   digits and dots, then `d` or `f` is rewritten to `%s` (or `%N$s`). So `%d`, `%.2f`, `%2$d` work in lang files.
   This rewrite does *not* apply to fallback strings or to a key used as its own template.
3. The parsed template is cached per language instance and re-parsed after a language change.
4. **The built-in default table** (dedicated server; also the client's base layer before resource packs) is built
   from two files in the game jar, in this order:
   1. `assets/minecraft/lang/en_us.json`: every entry, with rule 2's rewrite applied.
   2. `assets/minecraft/lang/deprecated.json`, an object with `removed` (list of keys) and `renamed` (object mapping
      old key → new key). First every `removed` key is deleted. Then each `renamed` pair is applied **in document
      order**: the value under the old key is taken out; if there was one it is stored under the new key
      (replacing any value there), otherwise the new key is deleted as well.
   A missing `deprecated.json` means nothing is removed or renamed. In 26.3 this deletes 423 keys and renames 148,
   changing 708 entries in all, so e.g. `{"translate":"addServer.add"}` flattens to the key text, not `Done`. *(Settled by the `text-component`
   suite, 2026-11-25.)*

### A.7.2 Template parsing (ordered rules)

1. A **placeholder** is a `%`, then optionally a run of decimal digits followed by `$`, then either one ASCII letter, a
   `%`, or the end of the string.
2. Text between placeholders is emitted verbatim; but if that text contains any `%` (a `%` that did not start a valid
   placeholder, e.g. `% ` or `%1s`), the template is **malformed**.
3. `%%` (exactly two characters) emits one `%`. `%N$%` is malformed.
4. `%s` takes the next *sequential* argument. The sequential counter starts at the first argument and advances only
   on non-positional `%s`.
5. `%N$s` takes argument N (1-based) and does not advance the counter. `%s %1$s %s` → arguments 1, 1, 2.
6. Any other letter (`%d` in a fallback, `%S`, `%x`), a `%` at the very end, index 0, an index beyond the argument
   count, or an index too large for a 32-bit integer → malformed.
7. **Malformed templates render as the raw template text** (after lookup), with no argument substitution at all.
8. Extra, unused arguments are ignored.

### A.7.3 Argument rendering

- Component argument → rendered as a component (its own style over the translatable's effective style).
- String → verbatim. Boolean → `true` / `false`. A null argument (only possible in-memory) → `null`.
- Numbers → the platform's canonical decimal text for the stored numeric type: integers in plain decimal; floating
  values in the shortest round-trip form with at least one fractional digit, using `E` notation outside
  [10⁻³, 10⁷) (`1.5`, `1.0E10`, `1.0E-5`, `Infinity`).
- **Numeric type depends on the source format.** From NBT the tag type is kept (`1b` → `1`; `1.5f` → `1.5`; SNBT `true`
  is a byte → renders `1`). From JSON the library narrows each number using its **exact decimal value** (not a
  double approximation):
  1. If the decimal value is an integer that fits in a signed 64-bit long, it becomes the smallest of
     byte/short/int/long that holds it. This includes `2.0` → `2`, `1e2` → `100`, `-0.0` → `0` (an integer zero, not a
     negative zero) and `9007199254740993.0` → `9007199254740993` (exact, even though no double holds it).
  2. Otherwise (a fractional value such as `1.0000000000000000001`, or an integer outside the long range such as
     `1e20`) it becomes the nearest double; if that double converts to a float and back unchanged it is stored as a
     float, else as a double. So `1.5` → float `1.5`, `0.1` → double `0.1`, `1.0000000000000000001` → float `1.0`
     (renders `1.0`), `1e20` → double (renders `1.0E20`), `1e-400` → float `0.0`.
  3. JSON `true` stays a boolean → `true`.
  The narrowed type also decides the NBT tag type when the component is encoded to NBT (`-0.0` → byte `0`).
  *(Rules 1–2 settled by the `text-component` suite, 2026-11-25.)*

## A.8 Flattening to plain text

1. **Plain string** (the default flattening): depth-first concatenation of each node's content text followed by its
   siblings' text, recursively; translation arguments are flattened in place. Per kind: literal → text; translatable →
   formatted template; keybind → as in A.2.2; score → nothing; selector → selector source text; nbt → nothing; object →
   fallback text (or default fallback). `§` formatting codes are kept verbatim.
2. **Truncated plain string:** same walk, stopping once the limit (UTF-16 units) is reached; the segment that crosses
   the limit is cut.
3. **Legacy formatting codes during rendering:** when a segment's text is turned into styled characters, a `§` followed
   by a code character (case-insensitive) changes the style for the rest of *that segment only*: `0`–`9`, `a`–`f` set
   the colour and clear bold/italic/underlined/strikethrough/obfuscated; `k` obfuscated, `l` bold, `m`
   strikethrough, `n` underlined, `o` italic; `r` resets to the segment's own effective style. The `§` and the code
   character are both consumed, even for an unknown code; a trailing lone `§` is dropped. A "plain text with codes
   stripped" helper exists that applies these same rules and keeps only the characters.
4. **Surrogates:** when converting to characters, an unpaired UTF-16 surrogate becomes U+FFFD.
5. **Flat list:** a component can be flattened to a list of single-literal components, one per non-empty text segment,
   each carrying its effective style.

## A.9 Server-side resolution

Resolution turns a component into a new component in which score, selector, nbt and object contents are replaced by
their values. It runs for `tellraw` / `title` / boss-bar names and other command arguments, sign text, text displays,
written books when first opened, the `set_name` loot function, and (client-side, without a source) the server-list
MOTD.

### A.9.1 Ordered rules

1. The root is resolved at depth 0. If the current depth exceeds the context's depth limit, stop: either return a
   copy of the remaining subtree unresolved (default behaviour) or return the literal `...` (MOTD behaviour).
   Translation arguments, separators and fallbacks of a node at depth d are resolved at depth d + 1.
2. Otherwise resolve the node's content at depth + 1, then append each sibling resolved at depth + 1, then apply the
   node's original style *over* the resolved content's style (original fields win, unset fields fall back to the
   resolved content's style). If the style has a `show_text` hover, its text is resolved too (depth + 1).
3. Per content kind:
   - **text, keybind** → unchanged copy.
   - **translatable** → same key and fallback; component arguments resolved, primitives kept.
   - **score** → empty if there is no command source. Holder: a selector name that matches exactly one entity uses
     that entity; more than one is a resolution error (the "only one entity" command error); zero uses the selector's
     *source text* as a plain name; a plain name is used as-is. If the holder is `*` and the context has a default
     scoreboard entity, that entity is used. Missing objective or missing score → empty; otherwise the score
     formatted with the score's own number format, else the objective's, else unstyled.
   - **selector** → empty without a source; otherwise the matched entities' display names joined by the resolved
     separator (default: `, ` in gray). Zero matches → empty; one match → that name alone.
   - **nbt** → empty without a source, or when the context's resolved-nbt counter (starting at 0, incremented by
     every nbt content that passes this check) is already above **65,536** — so the first 65,537 nbt contents of one
     resolution are resolved and later ones are empty. Values: every compound from the data source (all matched entities; the block
     entity at a loaded position; the storage compound, which exists even if empty), each queried with the path
     (path errors yield no values). With `interpret`: each value tag is decoded *directly as an NBT component* (a
     string tag becomes literal text — it is **not** parsed as JSON), failures are skipped with a warning, successes
     are resolved recursively. Without `interpret`: each value is rendered as SNBT-like text with syntax colours
     (strings appear **quoted**), or the same text uncoloured when `plain` is true. Values are joined with the
     resolved separator (default unstyled `, `); no values → empty.
   - **object** → the fallback is resolved; if the context's object filter rejects the object, the result is the
     resolved fallback, or a literal of the default fallback; otherwise the object is kept with the resolved fallback.

### A.9.2 Contexts

| Caller | Depth limit | Limit behaviour | Source / notes |
|---|---|---|---|
| Commands, signs, text displays, books, loot `set_name` | 100 | copy remaining subtree | source present; the score `*` entity is the source entity, or an explicit override (`title`, `tellraw` target, `set_name` entity) |
| Boss-bar names | 100 | copy | override entity explicitly none |
| Client server-list MOTD sanitizing | 16 | replace with `...` | no source (score/selector/nbt become empty); `player` objects rejected → fallback; resolution errors → empty MOTD |

Written books: on first open, every page is resolved; a page that throws keeps its original; if any resolved page
encodes to more than 32,767 JSON characters, no page changes and the book is just marked `resolved`.

---

# Part B — Item data components

## B.1 Concepts

- **Component type** — an entry of the built-in registry `minecraft:data_component_type` (122 entries, protocol IDs
  from `reports/registries.json`). Each type has an optional *persistent* codec (JSON/NBT) and a *network* codec. A type
  with no persistent codec is **transient**: never saved, rejected in serialized patches, but still sent on the
  network. A type with no dedicated network codec sends its value as an NBT tag produced by the persistent codec
  (default quota on decode).
- **Component map** — an immutable map from type to value.
- **Prototype** — the item type's default component map (exactly what the data-generator `components` report lists).
- **Patch** — map from type to either a value (set/override) or a *removed* marker.
- **Patched map** — a prototype plus a patch: what an item stack holds.

## B.2 Prototypes (default components)

1. Every item starts from a common base: `max_stack_size` 64, `lore` empty, `enchantments` empty, `repair_cost` 0,
   `use_effects` default, `attribute_modifiers` empty, `rarity` common, `break_sound` `entity.item.break`,
   `tooltip_display` default, `attack_animation` and `interact_animation` default; then item-specific components
   (including `item_name`, a translation of the item's or block's description key such as `block.minecraft.stone`,
   and `item_model`).
2. Prototypes are **bound after registries load** (some defaults reference data-pack registries such as enchantments,
   instruments or trim materials), and again on every reload. Decoding an item stack whose item has no bound
   prototype yet fails ("does not have components yet").
3. Zenith takes prototypes from the data-generator report rather than re-deriving them. The report prints the default
   values with the persistent codecs, so default-valued optional fields are omitted (e.g. `"minecraft:consumable": {}`).

## B.3 Patched-map semantics (ordered rules)

1. **Get(type):** if the patch has an entry → its value, or *absent* for a removed marker; else the prototype's value
   (possibly absent).
2. **Set(type, value):** if the value equals the prototype's value, the patch entry is dropped; otherwise the patch
   stores the value. Setting "nothing" is the same as Remove.
3. **Remove(type):** if the prototype has the type, the patch stores a removed marker; otherwise any patch entry is
   dropped.
4. **Applying a patch** applies each entry by rules 2–3 (a set equal to the default becomes no entry; a removal of
   a type the prototype lacks becomes no entry). A stack built from a patch is therefore always *sanitized*: its patch
   never contains default-equal values or no-op removals.
5. **Key set / size / iteration:** prototype types, plus patched-in types, minus removed types. Iteration lists
   patched values first (patch order), then untouched prototype entries.
6. **Equality** of two stacks' components = same prototype and equal patches. Because patches are sanitized, two stacks
   that read the same are equal.
7. Applying a patch *with validation* (used by the `set_components` loot function) restores the old patch if the
   result fails the stack validation in B.6.

## B.4 Patch encoding (JSON and NBT)

1. A patch is a map. Each key is a component id, optionally prefixed by `!`:
   - `"<id>": <value>` — set; the value uses the type's persistent codec.
   - `"!<id>": {}` — remove. The value must be a map (an object / compound); its contents are ignored.
2. The id after the optional `!` is parsed as an identifier (default namespace `minecraft`). An unparsable or unknown
   id fails ("no component with type"), and a transient type fails ("is not a persistent component").
3. Encoding writes every non-transient entry (sets as `id`, removals as `!id` with `{}`); an empty patch is `{}`.
4. Both `"x"` and `"!x"` in one input map are two distinct keys; both are kept and the one processed **last** wins.
   Processing order is the input map's iteration order (JSON object order; NBT compounds have no defined order)
   **[oracle]**.
5. An entry that fails to decode makes the patch fail; whether a *partial* patch (the good entries) survives depends on
   the caller (B.6) and on the library's partial-result rules **[oracle]**.

A **full component map** (not a patch; used e.g. by item predicates' exact `components` match and some data-pack
fields) uses the same keys without the `!` form.

## B.5 Network encoding

- **Patch:** VarInt count of set entries, VarInt count of removed entries; then each set entry as (VarInt component
  type protocol id, value in the type's network codec); then each removed type's VarInt id. Set entries are written
  before removals. Zero/zero decodes to the empty patch.
- **Delimited patch** (used only for untrusted client→server creative-slot items): identical, but every set value is
  prefixed with a VarInt byte length and must decode exactly within it.
- **Optional item stack:** VarInt count; `count ≤ 0` means the empty stack and nothing follows; otherwise VarInt item
  protocol id, then the patch. The non-optional variant rejects the empty stack in both directions.
- **Creative-slot packet:** the decoded stack must additionally encode with the persistent item-stack codec, or the
  packet is rejected.
- **Hashed stacks** (client→server container clicks): an optional (item id, count, map of type → 32-bit hash of the
  persistent encoding, set of removed types; each at most 256 entries). The hash function is specified separately
  (protocol spec); a stack matches when removed sets are equal and every set component's hash matches.
- Components without a dedicated network codec (value sent as NBT): `custom_data`, `intangible_projectile`,
  `map_decorations`, `recipes`, `debug_stick_state`, `lock`, `container_loot`.

## B.6 Item stacks and item templates

| | Item stack (`ItemStack` form) | Item template (inside other components / hover) |
|---|---|---|
| `id` | item id; not `minecraft:air`; prototype must be bound | item id; not `minecraft:air` |
| `count` | 1…99, **always written** (default 1 when absent) | 1…99, default 1, **omitted when 1** |
| `components` | patch (B.4), default empty, omitted when empty | same |
| short form | — | a bare item-id string is accepted on decode (`"minecraft:stone"`); encoding always uses the object form |
| empty | "optional" fields encode the empty stack as `{}` and decode `{}` as empty | cannot be empty |

**Stack validation** (applied by command item arguments, by turning an item template into a stack — a failing
template yields the empty stack with a warning — and by validated patch application; *not* by plain NBT/JSON decoding
of a stack, and not by the creative-slot packet, which only checks encodability):
1. A stack cannot have both `max_damage` and a `max_stack_size` above 1.
2. Items inside `container`, `bundle_contents` and `charged_projectiles` must not exceed their own max stack size.
3. Bundle total weight must be computable: each item weighs 1/max-stack-size per unit, a non-empty `bees` item weighs
   1, a nested bundle weighs its own content weight + 1/16; arithmetic overflow is an error.
4. The stack's count must not exceed its max stack size.

**Loading from saves:** when an item field inside a block entity / entity / container fails to decode, the problem is
reported and the library's *partial result* is used if one exists **[oracle]**; there is no all-or-nothing guarantee.

**Display name:** hover name = `custom_name` if set, else the written-book title (if non-blank), else `item_name`
(item default). The styled hover name gets the rarity colour, and italic only when `custom_name` is set. Lore lines
render over dark purple + italic.

## B.7 Component catalog (26.3)

Notation: *unit* = an empty map `{}` (any map decodes). *holder* = registry reference as in the registry spec: "id"
means id only; "id/inline" also accepts an inline definition. *holder set* = `#tag`, single id, or list of ids.
Every holder given by id, every id in a holder set, and every registry-keyed map key (e.g. the enchantment ids in
`enchantments`) is checked against the registry context (A.6.4): an unregistered id, or an id into an unavailable
registry, fails the component (so `{"minecraft:enchantments":{"minecraft:not_real":2}}` fails, which fails a
`show_item` hover event and so the whole text component). Tags (`#…`) follow the registry spec (a missing tag fails).
The `text-component` suite does not test tags yet: its built-in lookup has no data-pack tags loaded.
*item template* = B.6. *dye* = one of the 16 dye names (see Constants). *Transient* rows have no saved form.

| Id | Value shape (defaults in parentheses) | Validation |
|---|---|---|
| `custom_data` | compound, or an SNBT string that parses to a compound (encodes as compound) | — |
| `max_stack_size` | int | 1…99 |
| `max_damage` | int | ≥ 1 |
| `damage` | int | ≥ 0 |
| `unbreakable` | unit | |
| `use_effects` | `can_sprint` (false), `interact_vibrations` (true), `speed_multiplier` (0.2) | multiplier 0…1 |
| `custom_name` | text component | |
| `minimum_attack_charge` | float | 0…1 |
| `damage_type` | damage type holder (id) | |
| `item_name` | text component | |
| `item_model` | identifier | |
| `lore` | list of text components | ≤ 256 lines |
| `rarity` | `common` / `uncommon` / `rare` / `epic` | |
| `enchantments`, `stored_enchantments` | map enchantment id → level | level 1…255 |
| `can_place_on`, `can_break` | one block predicate, or a non-empty list of them | |
| `attribute_modifiers` | list of `{type: attribute id, id, amount: double, operation: add_value / add_multiplied_base / add_multiplied_total, slot (any), display ({type: default} / {type: hidden} / {type: override, value: component})}` | slot ∈ any, mainhand, offhand, hand, feet, legs, chest, head, armor, body, saddle |
| `custom_model_data` | `floats`, `flags`, `strings`, `colors` lists (all empty) | colours: int or `[r,g,b]` floats |
| `tooltip_display` | `hide_tooltip` (false), `hidden_components`: list of component ids (empty) | |
| `repair_cost` | int | ≥ 0 |
| `creative_slot_lock` | *transient*, unit | |
| `enchantment_glint_override` | boolean | |
| `intangible_projectile` | unit | |
| `food` | `nutrition`, `saturation`, `can_always_eat` (false) | nutrition ≥ 0 |
| `consumable` | `consume_seconds` (1.6), `animation` (eat), `sound` (generic eat), `has_consume_particles` (true), `on_consume_effects` ([]) | seconds ≥ 0; animation ∈ none, eat, drink, block, bow, trident, crossbow, spyglass, toot_horn, brush, bundle, spear; effect types apply_effects, remove_effects, clear_all_effects, teleport_randomly, play_sound |
| `use_remainder` | item template | |
| `use_cooldown` | `seconds`, `cooldown_group` (optional id) | seconds > 0 |
| `damage_resistant` | `types`: damage-type holder set | |
| `tool` | `rules`: list of `{blocks: block holder set, speed?, correct_for_drops?}`, `default_mining_speed` (1.0), `damage_per_block` (1), `can_destroy_blocks_in_creative` (true) | rule speed > 0; damage ≥ 0 |
| `weapon` | `item_damage_per_attack` (1), `disable_blocking_for_seconds` (0) | both ≥ 0 |
| `attack_range` | `min_reach` (0), `max_reach` (3), `min_creative_reach` (0), `max_creative_reach` (5), `hitbox_margin` (0.3), `mob_factor` (1) | reaches 0…64; margin 0…1; factor 0…2 |
| `enchantable` | `value` | ≥ 1 |
| `equippable` | `slot`, `equip_sound` (generic armor equip), `asset_id`?, `camera_overlay`?, `allowed_entities`? (entity holder set), `dispensable` (true), `swappable` (true), `damage_on_hurt` (true), `equip_on_interact` (false), `can_be_sheared` (false), `shearing_sound` (shears snip) | slot ∈ mainhand, offhand, feet, legs, chest, head, body, saddle |
| `repairable` | `items`: item holder set | |
| `glider` | unit | |
| `tooltip_style` | identifier | |
| `death_protection` | `death_effects` ([]) | |
| `blocks_attacks` | `block_delay_seconds` (0), `disable_cooldown_scale` (1), `damage_reductions` (one entry: angle 90, base 0, factor 1), `item_damage` (default function), `bypassed_by`?, `block_sound`?, `disabled_sound`? | reduction `horizontal_blocking_angle` > 0 (90), `type`?, `base`, `factor`; item damage `threshold` ≥ 0, `base`, `factor` |
| `piercing_weapon` | `deals_knockback` (true), `dismounts` (false), `sound`?, `hit_sound`? | |
| `kinetic_weapon` | `contact_cooldown_ticks` (10), `delay_ticks` (0), `dismount_conditions`?, `knockback_conditions`?, `damage_conditions`?, `forward_movement` (0), `damage_multiplier` (1), `sound`?, `hit_sound`? | condition: `max_duration_ticks` ≥ 0, `min_speed` (0), `min_relative_speed` (0) |
| `attack_animation`, `interact_animation` | `type` (whack), `duration` (6) | type ∈ none, whack, stab; duration ≥ 0 |
| `additional_trade_cost` | *transient*, int | |
| `block_transformer` | block-transformer holder (id) | |
| `villager_food` | `nutrition` | ≥ 1 |
| `dye`, `base_color` | dye | |
| `dyed_color` | RGB int, or `[r,g,b]` floats (encodes as int) | |
| `map_id` | int | |
| `map_decorations` | map string → `{type: map decoration type id, x: double, z: double, rotation: float}` | |
| `map_post_processing` | *transient* | |
| `charged_projectiles` | list of item templates | ≤ 1024 |
| `bundle_contents` | list of item templates | weight rule (B.6) |
| `potion_contents` | potion id string, or `{potion?, custom_color?: int, custom_effects ([]), custom_name?: string}`; encodes as the object | effect: `id`, `amplifier` 0…255 (0), `duration` (0), `ambient` (false), `show_particles` (true), `show_icon`?, `hidden_effect`? (nested) |
| `potion_duration_scale` | float | ≥ 0 |
| `suspicious_stew_effects` | list of `{id: effect, duration (160, lenient)}` | |
| `writable_book_content` | `pages` ([]): list of *filterable* strings | ≤ 100 pages, each 0…1024 chars |
| `written_book_content` | `title`: filterable string, `author`: string, `generation` (0), `pages` ([]): filterable components, `resolved` (false) | title ≤ 32 chars; generation 0…3; page ≤ 32,767 JSON chars |
| `trim` | `material`: trim material (id/inline), `pattern`: trim pattern (id/inline) | |
| `debug_stick_state` | map block id → property name | property must exist on that block |
| `entity_data`, `block_entity_data` | compound (or SNBT string) that must contain `id` (entity type / block-entity type); the rest is the payload | missing `id` fails |
| `bucket_entity_data` | as `custom_data` | |
| `instrument` | instrument (id/inline) | |
| `provides_trim_material` | trim material (id/inline) | |
| `ominous_bottle_amplifier` | int | 0…4 |
| `jukebox_playable` | jukebox song (id) | |
| `provides_banner_patterns` | banner-pattern holder set | |
| `recipes` | list of recipe ids | |
| `lodestone_tracker` | `target`? `{dimension, pos}`, `tracked` (true) | |
| `firework_explosion` | `shape`, `colors` ([] ints), `fade_colors` ([]), `has_trail` (false), `has_twinkle` (false) | shape ∈ small_ball, large_ball, star, creeper, burst |
| `fireworks` | `flight_duration` (0), `explosions` ([]) | duration 0…255; ≤ 256 explosions |
| `profile` | player name string, or `{name?, id? (4-int UUID), properties?, texture?, cape?, elytra?, model?}` | name ≤ 16 chars, no char ≤ U+0020 or ≥ U+007F; ≤ 16 properties (name ≤ 64, value ≤ 32,767, signature ≤ 1024); a bare name, or an object with exactly one of name / id and no properties, is resolved later by profile lookup |
| `note_block_sound` | identifier | |
| `banner_patterns` | list of `{pattern: banner pattern (id/inline), color: dye}` | |
| `pot_decorations` | `{back?, left?, right?, front?}` item templates | |
| `container` | list of `{slot, item: item template}`; missing slots are empty, later duplicates overwrite earlier ones | slot 0…255; ≤ 256 entries |
| `block_state` | map property name → value string | |
| `bees` | list of `{entity_data, ticks_in_hive, min_ticks_in_hive}` | |
| `sulfur_cube_content` | item template | |
| `lock` | item predicate | |
| `container_loot` | `loot_table` id, `seed` (0, long) | |
| `break_sound` | sound event (id/inline) | |
| `compostable` | `layers`: int or context-int-provider id | |
| `cooking_fuel` | `burn_time`: int or provider id, `speed_multiplier`: float or provider id | |
| `brewing_fuel` | `uses`, `speed_multiplier` (same forms) | |
| `mob_visibility` | `targeting_entity_types`: entity holder set, `visibility` | 0…10 |
| `villager/variant`, `wolf/variant`, `wolf/sound_variant`, `pig/variant`, `pig/sound_variant`, `cow/variant`, `cow/sound_variant`, `chicken/variant`, `chicken/sound_variant`, `zombie_nautilus/variant`, `frog/variant`, `painting/variant`, `cat/variant`, `cat/sound_variant` | registry id (id only) | |
| `fox/variant`, `salmon/size`, `parrot/variant`, `tropical_fish/pattern`, `mooshroom/variant`, `rabbit/variant`, `horse/variant`, `llama/variant`, `axolotl/variant` | fixed enum name | |
| `wolf/collar`, `cat/collar`, `sheep/color`, `shulker/color`, `tropical_fish/base_color`, `tropical_fish/pattern_color`, `cushion/color` | dye | |
| `provides_pottery_pattern` | decorated pot pattern (id) | |
| `sign_text_front`, `sign_text_back` | `messages`: exactly 4 components; `filtered_messages`: exactly 4 (lenient); `color` (black) and `has_glowing_text` (false) **always written** | |
| `waxed` | unit | |

*Filterable* values accept either the bare value or `{raw, filtered?}`, and encode as the object form.

---

## Edge cases and known quirks

1. `"hello"`, `{"text":"hello"}` and `["hello"]` decode to equal components; all three re-encode as the bare string
   `"hello"`.
2. `[]` and `{"text":"a","extra":[]}` are decode errors (the second fails the whole component, not just `extra`).
3. `[{"text":"a","color":"red"},"b"]` renders `b` red too; list form merges into the first element.
4. `{"text":"a","translate":"b"}` is literal `a` (kind order); `{"translate":"b","keybind":"c"}` is a translation;
   `{"type":"keybind","text":"x"}` fails (explicit type, missing `keybind`).
5. `{"type":"Text","text":"a"}` fails (type names are case-sensitive and there is no fallback once `type` exists).
6. `{"text":"a","bold":false}` stays a map on re-encode; `bold:false` is not the same as unset.
7. `{"text":"a","clickEvent":{...}}` / `hoverEvent` (old camelCase) decode as plain `a` with no event.
8. `click_event` with `action: open_file` fails to decode; `open_url` with `ftp://…`, `javascript:…` or a scheme-less
   URL fails; `HTTPS://…` is accepted.
9. `run_command` containing `§`, a newline, a tab or U+007F fails; `copy_to_clipboard` accepts any string.
10. `change_page` with page 0 or negative fails.
11. `color` values: `"Red"` fails; `"#ff5555"` is accepted and re-encodes `"#FF5555"`; `"#+FF"` is accepted (blue);
    `"#"`, `"#GG0000"`, `"#-1"`, `"#1000000"` fail; `"#F"` is `#00000F`.
12. `"red"` and `"#FF5555"` styles compare equal but encode differently.
13. `shadow_color: [2.0, 0, 0, 1]` gives red channel 254 (floor(510) masked), `[-0.5, …]` gives 128; result is
    re-encoded as an integer.
14. NBT booleans: `bold:1b`, `bold:5`, `bold:0.5f` are true; `bold:"true"` fails. JSON `"bold": 1`, `0`, `256` and
    `0.5` all **fail** the component (JSON booleans must be `true` / `false`).
15. In NBT, `{text:5}` fails (numbers are not strings); same in JSON.
16. Lenient fields: `{"translate":"k","fallback":5}` keeps the component and drops the fallback; `{"nbt":"a",
    "entity":"@s","interpret":"yes"}` behaves as `interpret:false`; but `{"selector":"@p","separator":5}` fails.
17. `nbt` with both `interpret:true` and `plain:true` fails; `{"nbt":"a"}` with no source fails ("no matching codec").
18. `nbt` source precedence without `source`: entity before block before storage.
19. `nbt` interpret of a string tag `"{\"text\":\"x\"}"` yields the literal text `{"text":"x"}` (no JSON parsing).
20. `nbt` without interpret on a string value renders with quotes (`"x"`), coloured unless `plain`.
21. Unresolved flattening: score → empty, nbt → empty, selector → its source text (e.g. `@p`), keybind on a dedicated
    server → the raw key name.
22. `score` with name `@a` matching two players → resolution error; matching nobody → score of the literal holder
    name `@a`.
23. `score` name `*` resolves to the context entity (command source or explicit override).
24. Translation `%s %1$s %s` with args (A, B) → `A A B`.
25. `%%` renders `%`; `100%` (trailing `%`), `50% off`, `%d` in a fallback, `%0$s`, `%3$s` with two args, and
    `%1$%` all render the raw template with no substitution.
26. Lang-file values with `%d` / `%.1f` / `%2$d` work (rewritten at load), but the same in a `fallback` string makes
    it malformed.
27. Missing key: with fallback → fallback (formatted); without → the key text itself (formatted, so a key containing
    `%s` substitutes arguments).
28. Translation argument numbers: SNBT `with:[true]` renders `1`, JSON `[true]` renders `true`; JSON `[2.0]` renders
    `2`; NBT `[2.0d]` renders `2.0`; JSON `[1e20]` renders `1.0E20` **[oracle]**.
29. A `with` element `{"text":"x"}` becomes the plain string `x` (re-encodes as `"x"`), while `{"text":"x","bold":true}`
    stays a component.
30. `object` without `type`/`object`: `{"sprite":"item/apple"}` is an atlas object on `minecraft:blocks`; plain text
    `[item/apple]`; with a non-default atlas `minecraft:items` → `[item/apple@items]`; player object without name →
    `[unknown player head]`.
31. MOTD sanitizing: player-head objects become their fallback; nesting deeper than 16 becomes `...`; score/selector/
    nbt become empty.
32. Server resolution depth (limit 100): in a chain of nested siblings, nodes at depths 0…100 are resolved; the node
    at depth 101 and everything below it are copied unresolved (a selector there still shows its source text).
33. nbt-content cap: in one resolution the first 65,537 nbt contents resolve; the 65,538th and later become empty.
34. `§` codes: `§cRed§rX` renders `X` in the segment's own style; `§z` is consumed silently; a trailing `§` is dropped;
    codes do not leak into the next sibling; plain-string flattening (A.8 rule 1) keeps the codes.
35. Unpaired surrogates render as U+FFFD; plain-string flattening keeps them as-is.
36. Written book page over 32,767 compact-JSON characters fails to decode; on resolution, one oversized page cancels
    the resolution of the whole book (book still marked resolved).
37. Patch `{"minecraft:max_stack_size": 64}` on an item whose default is 64 produces an empty patch once the stack is
    built (and re-encodes without `components`); `{"!minecraft:food": {}}` on a non-food item is a no-op.
38. Patch `{"!minecraft:damage": 0}` (value not a map) fails; `{"!minecraft:damage": {"x":1}}` is a valid removal.
    Likewise unit components need a map: `"minecraft:unbreakable": {}` works, `"minecraft:unbreakable": true` fails.
39. Patch key `minecraft:creative_slot_lock` (transient) fails; `"Minecraft:damage"` (invalid identifier) and unknown
    ids fail.
40. Patch containing both `minecraft:damage` and `!minecraft:damage`: last processed wins (JSON: document order) **[oracle]**.
41. Item stack JSON/NBT always contains `count`; templates omit `count` when 1; a template may be a bare id string;
    `count: 0`, `100` and id `minecraft:air` fail.
42. `{id:"minecraft:diamond_sword", components:{"minecraft:max_stack_size":64}}` decodes from NBT but fails stack
    validation (damageable and stackable) where validation applies (command item arguments, templates, the
    `set_components` loot function); the creative-slot packet does not run this check.
43. `container` with entries for slots 0 and 5 → six slots, 1–4 empty; duplicate slot numbers → last wins; slot 256
    fails.
44. `potion_contents: "minecraft:swiftness"` decodes; it re-encodes as `{potion: "minecraft:swiftness"}`.
45. `profile: "Steve"` decodes as a to-be-resolved profile; `profile: "Bad Name"` (space) and 17-character names fail.
46. Sign text with 3 messages fails; sign text always serializes `color` and `has_glowing_text` even at defaults.
47. Network patch: set entries always precede removals regardless of in-memory order; the empty patch is two zero
    VarInts; an item count ≤ 0 on the wire is the empty stack.

## Constants and IDs

- Content kind names (fallback order): `text`, `translatable`, `keybind`, `score`, `selector`, `nbt`, `object`.
- Data source names (order): `entity`, `block`, `storage`. Object kinds (order): `atlas`, `player`.
- Click actions: `open_url`, `open_file` (never serialized), `run_command`, `suggest_command`, `show_dialog`,
  `change_page`, `copy_to_clipboard`, `custom`. Hover actions: `show_text`, `show_item`, `show_entity`.
- Named colours (legacy code, RGB): black `0` 000000, dark_blue `1` 0000AA, dark_green `2` 00AA00, dark_aqua `3`
  00AAAA, dark_red `4` AA0000, dark_purple `5` AA00AA, gold `6` FFAA00, gray `7` AAAAAA, dark_gray `8` 555555, blue `9`
  5555FF, green `a` 55FF55, aqua `b` 55FFFF, red `c` FF5555, light_purple `d` FF55FF, yellow `e` FFFF55, white `f`
  FFFFFF. Format codes: `k` obfuscated, `l` bold, `m` strikethrough, `n` underlined, `o` italic, `r` reset.
- Dye names: white, orange, magenta, light_blue, yellow, lime, pink, gray, light_gray, cyan, purple, blue, brown,
  green, red, black.
- Default font `minecraft:default`; default sprite atlas `minecraft:blocks`; object placeholder character U+FFFC.
- Default separators: selector `, ` (gray); nbt `, ` (unstyled).
- Resolution: depth limit 100 (MOTD 16); resolved-nbt cap 65,536.
- Limits: book page 32,767 JSON chars; book title 32 chars; writable page 1024 chars / 100 pages; lore 256 lines;
  container 256 slots; charged projectiles 1024; firework explosions 256; item count 1…99; login disconnect JSON
  262,144 chars; network NBT default quota 2,097,152 bytes, depth 512.
- Component-type protocol IDs: `reports/registries.json` → `minecraft:data_component_type` (122 entries, `custom_data`
  = 0 … `cushion/color` = 121). Transient: `creative_slot_lock` (20), `additional_trade_cost` (42),
  `map_post_processing` (50).
- Oracle hooks: the text-component codec entry point (`ComponentSerialization.CODEC`, both JSON and NBT ops), the
  translation decomposition (`TranslatableContents`), `ComponentUtils.resolve`, and the component-patch codec
  (`DataComponentPatch.CODEC` / `STREAM_CODEC`).

## Open questions

1. **Partial results (DFU 10.0.21).** When part of a list, map or record fails, does the library return a partial value
   (e.g. a component list with the bad element dropped, a patch without the bad entry), and how does it propagate
   through nested records? World loading uses partial values, so this decides what survives a corrupt item. Pin with
   oracle decode fixtures over every shape in B.7.
2. **JSON number → boolean** and **JSON number narrowing** for translation arguments: confirm the exact library rules
   (low-byte test vs non-zero; integral-value narrowing; float-vs-double choice; huge values → `Infinity`).
3. **JSON key order** of encoded components and patches (record field order vs reverse vs sorted) for byte-exact JSON
   output (server status, data generator). Pin with fixtures.
4. **Fallback order storage.** The kind / source / object guess order is the registration order as preserved by the
   bundled Guava bidirectional hash map (believed insertion-ordered). Confirm with the fixture of edge case 4.
5. **Duplicate set/remove keys in one NBT patch** (edge case 40): which wins when the compound's iteration order is
   hash-based.
6. **Hex digits beyond ASCII** (full-width digits and letters) in `#` colours: confirm acceptance by fixture.
7. **Lenient UUID strings** in `show_entity` (`1-1-1-1-1`-style short groups): confirm what the platform UUID parser
   accepts.
8. **Gson nesting limit** (255 by default in Gson ≥ 2.11): confirm it applies to data-pack and status JSON, and the
   exact boundary.
9. **NBT-to-text rendering** used by non-interpreted `nbt` contents (spacing, folding of long lists, colour tokens):
   specify in the command-output spec and reuse here.
10. **Hashed-stack hash function** (B.5): needs its own protocol spec (hashing of every persistent value shape).
11. **Client keybind display names** for keybind contents (key names per keyboard layout / mouse buttons) belong to the
    client input spec.
12. **(implementer) Name-only `profile` re-encoding:** whether a `profile` decoded from a bare player-name string
    (`"Steve"`) re-encodes as the object form `{"name":"Steve"}` (current implementation) or collapses back to a bare
    string.
    **Answered (reader, 2026-11-25):** object form. The profile codec tries the full record form first and accepts a
    bare name only as an alternative on decode; encoding always uses the full form, so `"Steve"` re-encodes as
    `{"name":"Steve"}`. The current implementation is correct.
13. **(implementer) Dedicated per-component network codecs (B.5):** the 7 NBT-backed components and 3 transient
    components have their wire layouts pinned; the exact per-field binary wire layouts of the remaining complex
    persistent component types (such as `consumable`, `tool`, `equippable`, `blocks_attacks`, `written_book_content`)
    belong to the protocol spec and currently round-trip via network NBT.
14. **(implementer) Sub-discriminator omission on encode:** A.2.1 states encoding never writes `type`; the
    implementation likewise omits `source` (on `nbt`) and `object` (on `object`) when encoding. Confirm with oracle
    fixtures whether vanilla omits all three discriminator keys.
    **Answered (reader, 2026-11-25):** yes, all three. `type`, `source` and `object` are built by the same
    discriminator helper, whose encoder always writes the compact (keyless) form; the key is only consulted on decode.
    Exception: with map-compressing ops (none on any 26.3 text path) the keyed form is used for both directions.
    The current implementation is correct.
15. **(implementer) Registry-context checks for resource-key-like fields in B.7:** B.7 describes `recipes` ("list of
    recipe ids"), `container_loot.loot_table` ("`loot_table` id"), `lodestone_tracker.target.dimension` (`dimension`),
    and `compostable` / `cooking_fuel` / `brewing_fuel` provider ids ("context-int-provider id" / "provider id") as ids
    rather than holders, and block/item predicates (`can_place_on`, `can_break`, `lock`) are not yet decoded past raw
    maps. Confirm whether any of those id fields are validated against a registry on decode (e.g. `minecraft:recipe`,
    `minecraft:loot_table`, `minecraft:dimension`, `minecraft:context_int_provider`,
    `minecraft:context_float_provider`) or remain unchecked resource keys; they are currently left unchecked.

## Reader review (2026-11-25)
Twelve rules marked **[oracle]** come from the game's bundled serialization and JSON libraries rather than its own
code. They become settled facts only once the `text-component` suite captures vanilla's actual output for them. The
implementation reuses the JSON layer from `zenith-codec` (part A), so number, `null` and key-order handling are shared.
