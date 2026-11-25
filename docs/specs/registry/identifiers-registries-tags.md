# Registry: identifiers, registries, block-state IDs, tags and feature flags (26.3)

- Verified by suites: `registry-ids`, `blockstate-ids`
- Sources: public data-pack and protocol documentation; the game's data-generator reports (`reports/registries.json`,
  `reports/blocks.json`, compiled into Zenith's versioned tables in `crates/zenith-data/src/tables.rs`); and local
  reference reading (2026-11-24). Everything below is written in our own words.

## Summary

Every named object in the game—blocks, items, biomes, damage types, tags, pack files—is addressed by a two-part
**identifier** (`namespace:path`). Objects are grouped into **registries** that assign dense, zero-based 32-bit
numeric protocol IDs. Built-in registries are static per game version (ingested by Zenith from the data generator),
while dynamic registries are loaded from data packs and synchronized to clients during the configuration phase.
**Block states** receive a separate dense global ID table derived from block registration order and property
enumeration. **Tags** group registry entries through JSON files merged across data packs with dependency ordering,
optional references, and replacement rules. **Feature flags** gate experimental content and packs without altering
numeric IDs.

---

## 1. Identifiers and key types

### 1.1 Structure and validation

An identifier consists of two ASCII strings: a **namespace** and a **path**, canonically formatted as
`<namespace>:<path>`.

- **Default namespace:** `minecraft`.
- **Separator:** `:` (U+003A).
- **Allowed characters:**
  - Namespace: lowercase ASCII letters `a`–`z`, digits `0`–`9`, underscore `_`, hyphen `-`, and period `.`
    (regex `[a-z0-9_.-]`).
  - Path: all namespace characters plus forward slash `/` (regex `[a-z0-9/._-]`).
  - Uppercase letters, whitespace, colons, and all non-ASCII characters are rejected in both parts.
- **Reserved namespace value:** the exact namespace string `".."` is rejected, even though `.` is an allowed
  character. Other dot-containing namespaces (`"."`, `"..."`, `"a..b"`) pass validation.
- **Empty parts:** character validation checks every character in the string and therefore returns valid for an empty
  string `""`. The observable consequence depends on the entry point:
  - Single-string parsing of `""` or `":"` produces `minecraft:` (namespace `"minecraft"`, path `""`).
  - Single-string parsing of `":foo"` produces `minecraft:foo` (a leading separator selects the default namespace).
  - Direct two-argument construction with an empty namespace `("", "foo")` succeeds and produces `:foo` (empty
    namespace, path `"foo"`), because `""` is not `".."` and contains no invalid characters.
  - Direct two-argument construction with an empty path `("ns", "")` succeeds and produces `ns:`.

### 1.2 Parsing rules (ordered)

1. **Single-string parsing (JSON, NBT, network, general API):**
   1. Search for the **first** occurrence of `:` in the input.
   2. If no `:` is found, use namespace `"minecraft"` and the entire string as the path; validate the path.
   3. If `:` is at index 0, use namespace `"minecraft"` and the substring after `:` as the path; validate the path.
   4. Otherwise, split at the first `:` into `namespace` (before) and `path` (after). Validate `namespace` first, then
      validate `path`. Any second `:` remains inside `path` and fails path validation.
2. **Command-stream parsing:**
   - Greedily consumes contiguous characters from the set `[0-9a-z_:./-]`, stopping at the first character outside that
     set.
   - Feeds the consumed slice into single-string parsing above. If validation fails (for example, a second `:` or a `".."`
     namespace), the reader cursor is rewound to the start and an invalid-identifier syntax error is raised.
   - The **non-empty** command variant additionally rejects zero consumed characters before parsing; the general
     command variant allows zero consumed characters and parses `""` as `minecraft:`.
3. **Filesystem resolution:**
   - Resolving an identifier against a root directory joins `root / namespace / path`, normalizes both paths, and
     rejects the result if the normalized target escapes the normalized root (for example, via `..` segments in the
     path).

### 1.3 Length limits

- **In memory and JSON/NBT:** no identifier-specific length cap (strings are bounded only by general format limits, such
  as NBT's 65,535-byte modified UTF-8 limit).
- **Network wire encoding:** encoded as a standard protocol UTF-8 string with a maximum length of **32,767 UTF-16 code
  units** (and a maximum wire byte length of `3 × 32,767 = 98,301` bytes; because valid identifiers are pure ASCII,
  both limits are effectively 32,767 ASCII characters). Overlong byte counts, negative lengths, or truncated buffers
  fail decoding before identifier validation runs.

### 1.4 Ordering, equality and formatting

- **Equality:** exact string equality on both `namespace` and `path`.
- **Natural ordering:** compares **`path` first** (UTF-16 / ASCII lexicographical order), and breaks ties by comparing
  **`namespace` second**. For example, `z:a` sorts before `a:b`, and `alpha:foo` sorts before `minecraft:foo`. This
  ordering directly determines numeric ID assignment for data-driven registries (Section 2.4).
- **Formatting forms:**
  - Canonical string: `<namespace>:<path>`.
  - Short string: `<path>` when `namespace == "minecraft"`, otherwise `<namespace>:<path>`.
  - Language key: `<namespace>.<path>` (short form omits `minecraft.`; prefixed/suffixed forms join with `.`).
  - Debug filename: canonical string with every `/` and `:` replaced by `_`.

### 1.5 Error semantics

- Invalid namespace (bad character or `".."`) fails with an identifier error indicating an invalid namespace character
  for the combined `<namespace>:<path>`.
- Invalid path (bad character, including a second `:`) fails with an identifier error indicating an invalid path
  character for `<namespace>:<path>`.
- Identifier error messages escape special and non-ASCII characters using Java-style string escapes so control
  characters and non-ASCII code points never appear raw in diagnostics.
- JSON/codec decoding wraps identifier errors as a decode error stating that the input is not a valid resource location
  along with the underlying message.
- Command parsing maps identifier errors to the translatable invalid-ID command syntax error (`argument.id.invalid`).

### 1.6 Typed keys: resource keys and tag keys

- **Resource key:** a pair `(registry_id, element_id)` identifying one entry within a specific registry.
  - Registry keys themselves are resource keys whose `registry_id` is the root registry `minecraft:root` and whose
    `element_id` is the registry's own identifier (for example, `(minecraft:root, minecraft:block)`).
  - Interned weakly so identical `(registry_id, element_id)` pairs share identity.
  - In JSON and on the network, a resource key in a known-registry context serializes as only its `element_id`
    identifier; the registry identifier is omitted unless the field itself carries a registry key.
  - Note: two distinct logical registries share the identifier `minecraft:dimension`—runtime world levels and
    data-driven level stems (the dimensions layer).
- **Tag key:** a pair `(registry_key, tag_id)` identifying a tag within a specific registry.
  - Interned weakly; registry compatibility checks compare the registry key.
  - Serializes in JSON as `"<tag_id>"` when typed as a bare tag key, or `"#<tag_id>"` when parsed with the hashed tag
    format (used wherever tags and direct elements share a field). In hashed form, a string not starting with `#` fails
    with a "not a tag id" error.
  - On the network, tag keys serialize as bare `tag_id` identifiers (the enclosing registry is sent separately or fixed
    by context).

---

## 2. Registries

### 2.1 Core registry invariants

Every registry maps between four views of its entries:
1. Dense, non-negative 32-bit integer IDs (`0, 1, ..., N − 1`).
2. Element identifiers (`namespace:path`).
3. Typed resource keys `(registry_id, element_id)`.
4. Runtime values (matched by object identity).

Ordered rules:
1. Numeric IDs are assigned **strictly in registration order**, starting at `0` with no gaps.
2. Registering a duplicate identifier or a duplicate value (same object identity) fails immediately.
3. Registration is rejected once the registry is **frozen**.
4. Each registration records **registration metadata**: optional known-pack provenance `(namespace, id, version)` and a
   lifecycle (`stable` or `experimental`). The registry's aggregate lifecycle combines the initial lifecycle with every
   registered entry's lifecycle (any `experimental` entry makes the aggregate non-stable).
5. Iteration order (element enumeration and protocol serialization) is **numeric ID order (`0 .. N − 1`)**.
6. Looking up an unknown value's numeric ID returns `-1` in a non-defaulted registry; looking up an out-of-range
   numeric ID (`< 0` or `>= size`) returns absent.

### 2.2 Defaulted registries

A defaulted registry designates one default entry identifier at creation time. When that identifier is registered, its
reference holder is recorded as the default entry.

Observable fallback behavior compared to a non-defaulted registry:

| Operation | Non-defaulted registry | Defaulted registry |
|-----------|------------------------|--------------------|
| Value lookup by identifier (missing) | absent | default value |
| Optional value lookup by identifier (missing) | empty | **empty** (no fallback) |
| Holder lookup by identifier, resource key, or numeric ID (missing) | empty | **empty** (no fallback) |
| Value lookup by resource key (missing) | absent | **absent** (no fallback) |
| Value lookup by numeric ID (out of range) | absent | default value |
| Numeric ID lookup by value (unregistered / absent) | `-1` | numeric ID of default value |
| Identifier lookup by value (unregistered) | absent | default identifier |
| Any-entry lookup | first registered entry (`id 0`), or empty | default entry |
| Random-entry lookup when empty | empty | default entry |

The 10 built-in defaulted registries in 26.3 and their default identifiers:

| Registry | Default identifier |
|----------|--------------------|
| `minecraft:game_event` | `minecraft:step` |
| `minecraft:fluid` | `minecraft:empty` |
| `minecraft:block` | `minecraft:air` |
| `minecraft:entity_type` | `minecraft:pig` |
| `minecraft:item` | `minecraft:air` |
| `minecraft:chunk_status` | `minecraft:empty` |
| `minecraft:villager_type` | `minecraft:plains` |
| `minecraft:villager_profession` | `minecraft:none` |
| `minecraft:memory_module_type` | `minecraft:dummy` |
| `minecraft:sensor_type` | `minecraft:dummy` |

### 2.3 Built-in registries: bootstrap order, freezing and intrusive holders

All built-in registries are themselves registered into the root meta-registry `minecraft:root` in a fixed declaration
order. Their protocol IDs in `minecraft:root` and every entry's protocol ID within each built-in registry are captured
verbatim by the game's data generator in `reports/registries.json`, which Zenith compiles into `Tables`
(`crates/zenith-data/src/tables.rs`). Zenith relies on those ingested tables for built-in registry IDs and entry IDs
rather than replaying static initialization side effects.

Vanilla bootstrap executes in this exact sequence:
1. **Guard check:** accessing built-in registries before bootstrap starts raises an unbootstrapped-access error.
2. **Pre-registry hooks:** fire flammability table, entity-type sanity check, command selector options, dispenser
   behaviors, and cauldron interactions initialize first (triggering static block/item/entity registration).
3. **Populate contents:** every built-in registry's loader runs in the insertion order of `minecraft:root`. Each entry
   receives built-in metadata (no known-pack provenance, `stable` lifecycle).
4. **Freeze:**
   1. Freeze the root registry `minecraft:root` first.
   2. For each built-in registry in `minecraft:root` order:
      - Bind all tags that were pre-referenced during bootstrap to **empty** element lists.
      - Freeze the registry.
5. **Validate:** for every built-in registry, log an error if it has zero entries, and fail if a defaulted registry
   never had its default key registered.

**Freezing checks (both built-in and dynamic registries):**
1. Freezing an already-frozen registry is an idempotent no-op.
2. Mark the registry frozen and bind each registered value onto its reference holder.
3. Fail if any key in the registry still has an **unbound** reference holder (for example, a placeholder created by a
   forward reference during loading that was never registered). Unbound keys are reported sorted by identifier.
4. If the registry uses **intrusive holders**, fail if any pre-allocated intrusive holder was never registered; then
   discard the unregistered-intrusive-holder tracking table.
5. Fail if the active tag table was already bound before freezing, or if any pre-created named tag remains unbound
   (unbound tags are reported sorted by tag identifier).
6. Build the data-component lookup index, publish the bound tag map as the active tag set, and populate every reference
   holder's reverse tag-membership set (verifying that every tag member is a reference holder belonging to this
   registry, never a direct holder or a foreign-registry holder).

**Intrusive holders:** five built-in registries (`minecraft:fluid`, `minecraft:block`, `minecraft:entity_type`,
`minecraft:item`, `minecraft:block_entity_type`) pre-allocate a reference holder on each value at construction time
before its key is registered. Observable rules:
- Registering a value without a pre-allocated intrusive holder fails.
- Re-binding an intrusive holder to a different key or different value fails.
- Registries with intrusive holders reject requests to create stand-alone placeholder holders for unregistered keys.

### 2.4 Dynamic (data-driven) registries: layers and numeric ID assignment

Data-driven registries are organized into immutable **layers** composed into a single composite lookup (duplicate
registry keys across layers are forbidden):

- **Server layers (bottom to top):**
  1. **Static layer** — all built-in registries from `minecraft:root`.
  2. **World layer** — 51 world/data-driven registries (biomes, dimension types, damage types, enchantments, worldgen
     registries, entity variants, painting variants, dialogs, clocks, timelines, villager trades, etc.).
  3. **Dimensions layer** — `minecraft:dimension` (level stems), loaded with visibility of static and world layers.
  4. **Reloadable layer** — 8 reloadable registries (`minecraft:loot_table`, `minecraft:predicate`,
     `minecraft:context_float_provider`, `minecraft:context_int_provider`, `minecraft:item_modifier`,
     `minecraft:slot_source`, `minecraft:advancement`, `minecraft:recipe`), loaded with visibility of static (with
     pending reloaded static tags), world, and dimensions layers.
- **Client layers (bottom to top):**
  1. **Static layer** — all built-in registries.
  2. **Remote layer** — the 32 synchronized dynamic registries received from the server.

**Exact loading and numeric ID assignment order for data-pack registries:**

1. Before loading the world layer, tags for all static registries are loaded from the resource manager as pending
   reloads and overlaid into the read-only lookup context seen by world-layer codecs (they are not mutated onto the
   global static registries until the entire reload succeeds).
2. For each registry in the batch (world, then dimensions, then reloadable):
   1. Discover element files under `data/<namespace>/<registry_path>/` ending in `.json`, where `<registry_path>` is
      the registry identifier's path (for example, `worldgen/biome`, `damage_type`, `trial_spawner`).
   2. Across multiple data packs (lowest priority first, highest priority last), a higher-priority pack's file replaces
      any lower-priority pack's file at the same identifier, and a higher-priority pack's `filter` section removes
      matching lower-priority files.
   3. Strip `<registry_path>/` from the front and `.json` from the end of each file path to obtain the element
      identifier.
   4. Parse every file in parallel using strict JSON (RFC 8259: no comments, no trailing commas, no trailing tokens)
      and the registry's element schema. Schemas may reference elements in the same batch; referencing an entry in a
      currently loading registry creates a stand-alone placeholder holder if it is not yet registered. Parse failures
      are recorded per element key rather than aborting immediately.
   5. **Deterministic numeric ID assignment:** sort all parsed and failed entries by their **full file identifier**
      (`<namespace>:<registry_path>/<element_path>.json`) using identifier natural ordering (**full file path first,
      then namespace**), and register successful entries sequentially in that sorted order, assigning IDs
      `0, 1, 2, ...`.
      - **Subtle ordering quirk:** sorting happens on the **file identifier with `.json` still attached**, not on the
        stripped element identifier. Because `-` (ASCII 45) sorts before `.` (ASCII 46), and `.` followed by `a`–`i`
        sorts before `.json`, a path like `foo-bar` (`.../foo-bar.json`) or `foo.a` (`.../foo.a.json`) sorts **before**
        `foo` (`.../foo.json`), whereas `foo/bar` (`.../foo/bar.json`) and `foo_bar` (`.../foo_bar.json`) sort
        **after** `foo` (`/` is 47, `_` is 95).
   6. Registration metadata: if the winning resource came from a known pack whose namespace is `"minecraft"`, its
      lifecycle is `stable` (and it retains its known-pack provenance); otherwise its lifecycle is `experimental` (and
      any non-vanilla known-pack provenance is still stored if present).
   7. Load and bind the registry's own tags (Section 4) while the registry is still writable.
3. After all registries in the batch complete step 2:
   1. Freeze every registry in the batch; any freeze failure (such as an unbound forward reference) is recorded under
      that registry's key.
   2. If **any** parse or freeze error occurred anywhere in the batch, log all errors grouped by registry and element
      (sorted by identifier) and abort loading.
   3. Run per-registry validators on every frozen registry:
      - Non-empty validator (must contain at least 1 entry): `wolf_variant`, `wolf_sound_variant`, `pig_variant`,
        `pig_sound_variant`, `frog_variant`, `cat_variant`, `cat_sound_variant`, `cow_variant`, `cow_sound_variant`,
        `chicken_variant`, `chicken_sound_variant`, `zombie_nautilus_variant`, `painting_variant`.
      - Timeline validator (`minecraft:timeline`): fails if two timelines define the same time-marker key on the same
        `world_clock`.
   4. If any validator recorded errors, log and abort loading.

### 2.5 Registry synchronization to clients (configuration phase)

During the configuration phase, the server synchronizes dynamic registries and tags to the client in this order:

1. **Enabled features:** server sends `update_enabled_features` carrying the set of active feature-flag identifiers.
2. **Known-pack negotiation:** server collects known-pack metadata (`namespace`, `id`, `version`) from all active data
   packs and sends `select_known_packs`.
   - **Exact-match rule:** when the client replies with its accepted known-pack list, the server checks whether the
     client's list is **equal** (same elements in the same order) to the requested list. If equal, all requested packs
     are negotiated; **if not equal (even if the client returned a strict subset)**, the negotiated set becomes
     **empty** and the server transmits full NBT payloads for every entry.
3. **Synchronized registry data:** server iterates the fixed list of **32 synchronized registries** in order:
   1. `worldgen/biome` *(network codec)*
   2. `chat_type`
   3. `trim_pattern`
   4. `trim_material`
   5. `wolf_variant` *(network codec, non-empty)*
   6. `wolf_sound_variant` *(network codec, non-empty)*
   7. `pig_variant` *(network codec, non-empty)*
   8. `pig_sound_variant` *(network codec, non-empty)*
   9. `frog_variant` *(network codec, non-empty)*
   10. `cat_variant` *(network codec, non-empty)*
   11. `cat_sound_variant` *(network codec, non-empty)*
   12. `cow_sound_variant` *(direct codec, non-empty)*
   13. `cow_variant` *(network codec, non-empty)*
   14. `chicken_sound_variant` *(direct codec, non-empty)*
   15. `chicken_variant` *(network codec, non-empty)*
   16. `zombie_nautilus_variant` *(network codec, non-empty)*
   17. `painting_variant` *(direct codec, non-empty)*
   18. `sulfur_cube_archetype`
   19. `dimension_type` *(network codec)*
   20. `damage_type`
   21. `banner_pattern`
   22. `enchantment`
   23. `jukebox_song`
   24. `instrument`
   25. `test_environment`
   26. `test_instance`
   27. `dialog`
   28. `world_clock`
   29. `timeline` *(network codec)*
   30. `decorated_pot_pattern`
   31. `block_transformer`
   32. `worldgen/block_state_provider`
   - For each present registry, the server sends a `registry_data` packet containing `(registry_id, entries)` where
     `entries` are in the server's **numeric ID order (`0 .. N − 1`)**.
   - Each entry is `(element_id, optional_nbt)`: if the entry's known-pack provenance is in the negotiated set,
     `optional_nbt` is omitted (`absent`); otherwise the value is encoded to unnamed NBT with the registry's
     network/direct schema (read on the client under the default NBT byte quota).
4. **Tag sync:** immediately after all `registry_data` packets, the server sends `update_tags` covering every static
   registry and every synchronized dynamic registry whose tag map is non-empty (Section 4.5).
5. **Client assembly and client numeric IDs:**
   - The client collects `registry_data` packets per registry (if multiple packets arrive for the same registry, their
     entry lists are appended in arrival order) and `update_tags` payloads (a later tag payload for the same registry
     replaces an earlier one).
   - At finish-configuration, if any `registry_data` arrived, the client constructs the remote layer:
     - Static-registry tags from `update_tags` are resolved into pending reloads (skipped on local integrated-server
       memory connections so shared static registries are not overwritten twice) and exposed to dynamic schemas during
       decoding.
     - For each synchronized registry that received data, entries are decoded in parallel—from the entry's NBT payload
       when present, or from the client's local known-pack JSON file (`data/<ns>/<registry_path>/<path>.json`) when
       omitted—and registered in the **exact order received from the server**, preserving 1:1 numeric IDs without
       re-sorting.
     - Network-loaded entries receive empty known-pack provenance and `experimental` lifecycle.
     - Dynamic-registry tags from `update_tags` are bound before freezing.
     - Registries are frozen and validated (same non-empty validators as the server; a synchronized registry that
       received no `registry_data` packet stays empty and fails validation if it has a non-empty validator).
     - Pending static-registry tags and registry data components are applied last.

### 2.6 Holder and holder-set observable behavior

- **Holder kinds:**
  - **Reference holder:** represents a registered entry belonging to a specific registry owner. Carries a resource key,
    bound value, bound tag set, and bound data components.
    - Accessing key or value before binding fails.
    - Querying tag membership before tags are bound fails.
    - Matching against an identifier compares the key's identifier; matching against a resource key compares interned
      key identity; matching against a tag key checks the bound tag set; matching against another holder delegates to
      whether the other holder matches this holder's resource key.
    - Can only be serialized in a registry context whose owner matches its own registry.
  - **Direct holder:** wraps an inline, unregistered value directly.
    - Always bound; has no resource key (unwrapping its key yields empty; display name is `"[unregistered]"`).
    - Never matches any identifier, resource key, key predicate, or tag key (always false); matching against another
      holder compares wrapped values by value equality.
    - Can serialize in any registry context, except in fixed-reference schemas (which reject direct holders) and tags
      (which reject direct holders).
- **Holder-set kinds:**
  - **Named holder set:** backed by a tag key and registry owner, bound to an immutable list of reference holders.
    - Accessing contents before binding fails.
    - Membership checks whether the queried holder belongs to that tag key (rather than scanning the element list).
  - **Direct holder set:** backed by an explicit immutable list of holders (always bound, no tag key).
    - Membership checks against a lazily built set of the list's elements.
- **JSON and wire encoding:**
  - **JSON holder (inline-allowed):** a valid identifier string looks up a reference holder in the registry (failing if
    missing); any non-identifier-string input (or when no registry context is present) decodes via the inline element
    schema into a direct holder. If inline definitions are disallowed, non-identifier input fails.
  - **JSON holder (fixed-reference):** requires registry context; only accepts identifier strings and rejects direct
    holders on encode.
  - **JSON holder set:** accepts `"#<tag_id>"` (resolved to a named holder set via registry context; missing tag is an
    error), a single element (when compact form is enabled), or a JSON array of elements (decoded as a direct set).
  - **Network holder (registry-only):** `VarInt(registry_id)` (out-of-range fails).
  - **Network holder (with inline fallback):** `VarInt(0)` followed by inline element payload for direct;
    `VarInt(registry_id + 1)` for reference.
  - **Network holder set:** `VarInt(0)` followed by tag identifier for named (resolved in the registry; missing tag
    fails); `VarInt(count + 1)` followed by `count` registry-only holder IDs for direct.

---

## 3. Block-state (and fluid-state) global ID assignment

Global block-state IDs live in a dense zero-based 32-bit ID table separate from `minecraft:block`, used in chunk
palettes, block-update packets, and particle/entity data.

### 3.1 Property validation and ordering (per block)

1. **Name constraints:** every property name and every serialized property value name must match `^[a-z0-9_]+$`.
2. **Cardinality constraint:** every property must have **at least 2** possible values (`size > 1`), and property names
   within one block must be unique.
3. **Property order:** a block's properties are sorted **lexicographically by property name** (ASCII order), regardless
   of the order in which the block declared them.
4. **Value order within each property:**
   - **Boolean property:** `[true, false]` — index `0` is `true`, index `1` is `false`.
   - **Integer property `[min, max]`** (`min >= 0`, `max > min`): ascending integer order `min, min + 1, ..., max`.
   - **Enum property:** the exact order of enum values supplied when the property is defined (by default, the enum's
     declaration order filtered by any predicate).

### 3.2 State enumeration and global ID assignment

1. **Per-block states:**
   - A block with 0 properties has exactly 1 singleton state.
   - A block with `k >= 1` properties enumerates the Cartesian product of its sorted properties' value lists in
     **row-major (odometer) order**: the **last** property (alphabetically) cycles fastest and the **first** property
     cycles slowest.
   - Example: for properties `a ∈ [true, false]` and `b ∈ [0, 1, 2]`, the 6 states are enumerated as
     `(true,0), (true,1), (true,2), (false,0), (false,1), (false,2)`.
2. **Global IDs:**
   - After all blocks are registered in `minecraft:block`, iterate `minecraft:block` in **block protocol ID order**
     (`0, 1, ...`), and for each block iterate its enumerated states in order, assigning consecutive IDs `0, 1, 2, ...`.
   - Every block's states therefore occupy a single contiguous range `[first_state, first_state + state_count)`.
   - `minecraft:air` is block ID `0` with 1 state, so global block-state ID `0` is always `minecraft:air`.
3. **Fallback behavior:**
   - Querying the global ID of an absent or unregistered state returns `0`.
   - Querying the state for an out-of-range global ID (`< 0` or `>= total_states`) returns `minecraft:air`'s default
     state.
4. **Zenith ingestion:** `reports/blocks.json` records each block's properties (in sorted property order and value
   order), every state's exact global ID, and the unique default state. `crates/zenith-data/src/tables.rs` validates
   contiguity, uniqueness, density from `0`, and exactly one default state per block.
5. **Fluid states:** fluid states use an identical separate global ID table, iterating `minecraft:fluid` in
   registration order (`empty`, `flowing_water`, `water`, `flowing_lava`, `lava`) and enumerating each fluid's states
   in the same Cartesian order.

---

## 4. Tags

### 4.1 Directory layout and JSON file format

- **Path:** `data/<namespace>/tags/<registry_path>/<tag_path>.json`, where `<registry_path>` is the registry
  identifier's path (singular, matching the registry key directly—for example `tags/block`, `tags/item`,
  `tags/entity_type`, `tags/worldgen/biome`, `tags/trial_spawner`). The tag's identifier is `<namespace>:<tag_path>`.
- **JSON parsing:** uses strict JSON (RFC 8259: no comments, no unquoted keys, no trailing commas, no trailing content
  after the root value).
- **Schema:**

| Field | Type | Required | Default | Meaning |
|-------|------|----------|---------|---------|
| `values` | array of tag entries | yes | — | Ordered list of element or nested-tag references |
| `replace` | boolean | no | `false` | If `true`, discards all entries accumulated from lower-priority packs |

- **Tag entry forms** (each element of `values`):
  1. **String shorthand:** `"<id>"` (required element reference) or `"#<id>"` (required nested tag reference).
  2. **Object form:** `{ "id": "<id>" | "#<id>", "required": <boolean> }`, where `"required"` is optional and defaults
     to `true`.
  - Both forms parse `<id>` (after stripping any leading `#`) via standard identifier parsing, so omitted namespaces
    default to `minecraft`.

### 4.2 Multi-pack merging (ordered rules)

Data packs are stacked from **lowest priority** (index 0, typically `vanilla`) to **highest priority** (last).

1. Collect the resource stack for every `.json` file under `tags/<registry_path>`:
   - Packs are walked from lowest priority to highest priority.
   - If a higher-priority pack's `filter` metadata section matches a tag file's namespace and path, all accumulated file
     entries from earlier packs for that tag file are cleared before the higher-priority pack's own files are added.
   - The resulting stack for each tag file preserves lowest-to-highest pack order.
2. For each tag file stack, process each pack's file in lowest-to-highest order:
   1. Open and parse the file as strict JSON. If I/O or JSON syntax fails, log an error and skip this file (continue
      with the next pack in the stack).
   2. Look up or create the mutable entry list for `tag_id` in the tag-builder map.
      - **Quirk:** this empty list is inserted into the map *after* JSON syntax succeeds and *before* tag-file schema
        decoding runs. Consequently, a syntactically valid JSON file that fails tag schema validation (for example, `{}`
        missing `"values"`) leaves behind an empty tag (if no other pack supplies valid entries), whereas malformed JSON
        does not create an entry at all.
   3. Decode the JSON against the tag-file schema (`values`, optional `replace`). If decoding fails, log an error and
      skip this file; any entries already accumulated from lower-priority packs remain intact (even if the broken file
      set `"replace": true`).
   4. If `replace` is `true`, clear all previously accumulated entries for `tag_id`.
   5. Append all entries from `values` in order, recording each entry's source pack ID (used only in error logs).

### 4.3 Dependency ordering and resolution (ordered rules)

Given the merged map of `tag_id -> list of entries`:

1. **Build the cycle-free dependency graph:**
   - Initialize an empty directed dependency graph (`tag_id -> set of dependency tag_ids`).
   - **Pass 1 (required nested tags):** for every tag `A` and every required `#B` entry in `A`, test whether `A` is
     already reachable from `B` in the graph. If not, add edge `A -> B`.
   - **Pass 2 (optional nested tags):** for every tag `A` and every optional `#B` (`required: false`) entry in `A`,
     test whether `A` is already reachable from `B` in the graph. If not, add edge `A -> B`.
   - Because Pass 1 completes before Pass 2, a required dependency `A -> B` always takes precedence over a reverse
     optional dependency `B -> A?`, guaranteeing that `B` is resolved before `A`.
2. **Depth-first post-order traversal:**
   - Maintain a `visited` set and a `resolved` map (`tag_id -> immutable list of elements`).
   - For every `tag_id` in the builder map, visit it if not yet in `visited`:
     1. Mark `tag_id` as visited immediately.
     2. Recursively visit all of `tag_id`'s outgoing dependency targets in the graph.
     3. Resolve `tag_id`'s entry list (step 3) and, if resolution succeeds, insert `tag_id -> elements` into `resolved`.
3. **Resolving one tag's entry list:**
   - Initialize an empty **insertion-ordered deduplicating set** (`values`) and an empty `missing` list.
   - For each entry in order:
     - **Nested tag `#T`:** look up `T` in `resolved`.
       - If found, append all elements of `T` into `values` in order (duplicates are ignored, keeping first-seen
         position).
       - If not found (undefined, failed to load, or unresolved due to a cycle): if `required`, append to `missing`; if
         optional (`required: false`), ignore and continue.
     - **Element `E`:** look up `E` via the registry's element lookup (see Section 4.4 for the critical frozen vs
       loading distinction).
       - If found, insert into `values` (duplicates ignored, keeping first-seen position).
       - If not found: if `required`, append to `missing`; if optional (`required: false`), ignore and continue.
   - Notice that **all** entries are checked even after an earlier entry fails, so every missing reference is reported
     together.
   - If `missing` is non-empty: log an error listing all missing entries with their source packs, and **do not insert
     `tag_id` into `resolved`**. Any downstream tag that requires `#tag_id` will therefore also fail to load.
   - If `missing` is empty: freeze `values` as an immutable list in first-seen order and store it in `resolved`.

### 4.4 Cycle, missing-reference and registry-state edge cases

- **Required self-reference (`A` contains `"#A"`):** when adding `A -> A`, `A` has no outgoing edges yet, so the
  reachability check returns false and `A -> A` is added. During depth-first traversal, `A` is marked visited before
  recursing on `A`, which immediately returns; `A` then resolves while `A` is not yet in `resolved`, so `#A` is missing
  and `A` fails to load.
- **Optional self-reference (`A` contains `{ "id": "#A", "required": false }`):** same traversal trace; `#A` is not in
  `resolved`, so the optional entry is silently skipped and `A` loads with its remaining entries.
- **Mutual required cycle (`A` requires `#B` and `B` requires `#A`):** the first edge added (in hash-map iteration
  order) succeeds; the reverse edge is dropped by the cycle check. Whichever tag resolves first fails because the other
  is not yet in `resolved`; the second then also fails because the first was omitted. **Both tags fail to load.**
- **Mixed required/optional cycle (`A` requires `#B`, `B` optionally references `#A`):** Pass 1 adds `A -> B`; Pass 2
  drops `B -> A`. `B` always resolves first (skipping optional `#A`), then `A` resolves and includes all of `B`.
- **Mutual optional cycle (`A` has `#B?`, `B` has `#A?`):** whichever edge is inserted first in hash-map iteration order
  determines which tag resolves first (without the other) and which resolves second (including the first). Both tags
  succeed, but their final contents depend on hash-map order.
- **Missing required element in a frozen (static/built-in) registry:** the lookup returns empty; only that tag (and
  tags requiring it) fails to load with an error log, and world loading continues.
- **Missing required element in a dynamic (loading) registry:** during data-pack registry loading, a **required**
  element lookup queries the writable registration lookup, which **creates an unbound placeholder holder** instead of
  returning empty. The tag itself appears to build, and then **registry freezing fails** with an unbound-value error,
  aborting world load. By contrast, an **optional** (`required: false`) element lookup queries the read-only view of
  already-registered elements and silently skips missing IDs without creating a placeholder.
- **Tags referencing disabled feature-flag elements:** because disabled feature elements remain registered in built-in
  registries, tag resolution succeeds and includes them in the tag's holder list. Callers are responsible for checking
  feature enablement if needed.

### 4.5 Tag reloading and network serialization

- **Static registry tag reload:**
  - When a static registry has at least one loaded tag, reload preparation reuses existing named holder-set instances
    for any tag keys that existed at bootstrap/freeze time (creating new instances for newly introduced tags), and
    exposes the pending tag map in a read-only overlay lookup for subsequent loading stages.
  - Applying the pending reload binds the new element lists into each named set, replaces the registry's active tag
    map, and recomputes every holder's reverse tag-membership set.
  - **Empty-reload quirk:** if a static registry has **zero** loaded tags in the data packs, no pending tag reload is
    created for it; its previously bound tags (such as empty bootstrap tags) are left untouched.
- **Wire format (`update_tags`):**
  - Serializes a map of `registry_id -> (map of tag_id -> list of VarInt element numeric IDs)` for every static
    registry and every synchronized dynamic registry whose tag map is non-empty (a registry with zero tags is omitted
    from the packet, whereas a tag whose element list is empty *is* sent as an empty list).
  - Serializing a tag containing a direct (unregistered) holder fails.
  - On the client, each numeric ID is resolved against the target registry; **any out-of-range numeric ID is silently
    dropped** rather than failing decoding.

---

## 5. Feature flags

### 5.1 Universe and bitset model

- Each feature flag belongs to a named **universe** and occupies one bit index (`0..63`), giving a hard limit of **64
  flags per universe**. Duplicate flag identifiers or a 65th flag in a universe fail at registry construction.
- A **feature flag set** is a `(universe, u64 mask)` pair. The empty set has `universe = null` and `mask = 0`.
- Set operations:
  - Membership (`flag ∈ S`): false if universes differ; otherwise `(S.mask & flag.mask) != 0`.
  - Subset (`A ⊆ B`): always `true` if `A` is empty; otherwise requires equal universes and `(A.mask & ~B.mask) == 0`.
  - Intersection (`A ∩ B ≠ ∅`): requires both non-empty and equal universes, and `(A.mask & B.mask) != 0`.
  - Union (`A ∪ B`): returns the other if one is empty; otherwise requires equal universes (failing on mismatch) and
    returns `A.mask | B.mask`.
  - Difference (`A \ B`): returns `A` if either is empty; otherwise requires equal universes (failing on mismatch) and
    returns the empty set if `A.mask & ~B.mask == 0`, else the masked set.
- **Experimental check:** a set is experimental iff it is **not** a subset of `{minecraft:vanilla}`.

### 5.2 Built-in flags in 26.3 (`main` universe)

| Bit | Identifier | Role |
|-----|------------|------|
| 0 | `minecraft:vanilla` | Default feature set |
| 1 | `minecraft:trade_rebalance` | Experimental villager trade rebalance |
| 2 | `minecraft:redstone_experiments` | Experimental redstone behavior |
| 3 | `minecraft:minecart_improvements` | Experimental minecart physics |

### 5.3 Serialization and unknown flags

- **JSON/NBT schema:** encodes a feature flag set as a list of identifiers; decodes a list of identifiers into a set.
  If any identifier is unknown, decoding returns a **partial error** (an error result carrying the set of recognized
  flags as its partial value).
- **Lenient name lookup:** logs a warning for each unknown identifier and returns the set of recognized flags.
- **World config (`level.dat`):** `enabled_features` is a lenient optional field defaulting to `[minecraft:vanilla]`.
- **Network (`update_enabled_features`):** encoded as a VarInt-prefixed set of identifiers, sent by the server in the
  configuration phase before registry synchronization.

### 5.4 Element gating and data-pack gating

- **Feature-filtered registries (7 built-in registries):**
  1. `minecraft:item`
  2. `minecraft:block`
  3. `minecraft:entity_type`
  4. `minecraft:game_rule`
  5. `minecraft:menu`
  6. `minecraft:potion`
  7. `minecraft:mob_effect`
- An element in these registries declares a required-features set and is enabled iff
  `required_features ⊆ enabled_features`.
- **No ID shifting:** disabled feature elements remain fully registered in their built-in registries with their normal
  protocol IDs and block-state IDs. Filtering is purely a lookup-view and runtime check: a feature-filtered registry
  view hides disabled entries from single-key lookups and element iteration, while leaving tag lookups and tag listings
  unfiltered.
- **Data-pack gating (`pack.mcmeta`):**
  - A pack may declare `"features": { "enabled": [ "<flag_id>", ... ] }`. Packs without this section request the empty
    flag set (the built-in `vanilla` server pack declares `["minecraft:vanilla"]`).
  - During world pack configuration:
    - **Safe mode:** selects only `"vanilla"`, plus any built-in feature packs needed to cover forced features.
    - **New-world init mode:** forced features = `∅`, allowed features = all flags. All auto-enabled and user-selected
      packs may activate their declared flags.
    - **Existing-world load:** forced features = allowed features = persisted `enabled_features`.
      - Any available auto-add pack is automatically selected only if its requested features are a subset of allowed
        features.
      - Any previously selected pack whose requested features are **not** a subset of allowed features is logged and
        **automatically disabled**.
      - If no packs remain selected, `"vanilla"` is forced selected.
      - Any flags in forced features not yet covered by selected packs cause matching built-in feature-source packs
        (whose requested flags intersect the missing flags and are a subset of forced features) to be automatically
        forced enabled.
    - The resulting world `enabled_features` is the union of all selected packs' requested features and forced features.

---

## Constants and IDs

- Root meta-registry: `minecraft:root`
- Default namespace: `minecraft`
- Network identifier string limit: `32,767` UTF-16 code units (`98,301` max UTF-8 bytes)
- Maximum feature flags per universe: `64`
- Defaulted built-in registries: 10 (table in Section 2.2)
- Intrusive-holder built-in registries: 5 (`fluid`, `block`, `entity_type`, `item`, `block_entity_type`)
- Feature-filtered built-in registries: 7 (`item`, `block`, `entity_type`, `game_rule`, `menu`, `potion`, `mob_effect`)
- Synchronized dynamic registries: 32 (ordered list in Section 2.5)
- Reloadable registries: 8 (`loot_table`, `predicate`, `context_float_provider`, `context_int_provider`,
  `item_modifier`, `slot_source`, `advancement`, `recipe`)
- Built-in registry and block-state protocol IDs for 26.3: ingested at runtime from `reports/registries.json` and
  `reports/blocks.json` (not duplicated here).

---

## Edge cases to cover in tests

1. **Identifier empty parts and colon handling:** `""`, `":"`, and `":foo"` parse to `minecraft:` and `minecraft:foo`;
   two-argument construction `("", "foo")` succeeds with an empty namespace; `"a:b:c"` fails path validation because
   splitting occurs on the first `:`.
2. **Namespace `".."` rejection vs other dots:** `"..:foo"` fails namespace validation, whereas `".:foo"`,
   `"...:foo"`, and `"a..b:foo"` succeed; path traversal `"minecraft:../secret"` passes identifier validation but is
   rejected by filesystem root-resolution.
3. **Identifier uppercase / non-ASCII rejection and escaped error messages:** uppercase ASCII (`"Minecraft:stone"`) and
   non-ASCII characters fail, and error messages escape control/non-ASCII characters.
4. **Identifier natural ordering:** path is compared before namespace (`z:a < a:b`, `alpha:foo < minecraft:foo`).
5. **Data-driven numeric ID ordering with `.json` suffix attached:** elements `foo`, `foo-bar`, `foo.a`, `foo/bar`,
   `foo_bar` in the same namespace receive IDs in order `foo-bar` (`-` = 45), `foo.a` (`.` then `a` < `j`), `foo`
   (`.json`), `foo/bar` (`/` = 47), `foo_bar` (`_` = 95) — different from sorting stripped element IDs.
6. **Defaulted registry selective fallbacks:** missing identifier returns the default value via direct value lookup and
   the default ID via numeric ID lookup, but returns empty via optional identifier lookup, holder lookup, and value
   lookup by resource key.
7. **Built-in vs dynamic tag missing required element:** missing required element in a frozen static registry drops
   only the affected tag (and tags requiring it), whereas in a loading dynamic registry it creates an unbound holder
   and aborts the entire registry load at freeze time.
8. **Tag `replace: true` across multiple data packs and parse failures:** a higher-priority pack with `replace: true`
   clears lower-priority entries; a higher-priority pack with invalid JSON or invalid tag schema does **not** clear
   lower-priority entries; valid JSON with invalid tag schema still inserts an empty tag entry if no other pack
   contributed, while malformed JSON does not.
9. **Tag deduplication and first-seen order:** repeated elements (direct or via nested `#tag` expansion) appear once at
   their first-seen position.
10. **Tag cycles (all four combinations):** required self-reference fails; optional self-reference succeeds (ignored);
    mutual required cycle drops both tags; required `A -> #B` plus optional `B -> #A?` deterministically resolves `B`
    before `A`.
11. **Tag network deserialization with out-of-range numeric IDs:** invalid numeric IDs in `update_tags` are silently
    filtered out rather than failing decode, while a missing tag in a named holder-set wire payload fails.
12. **Known-pack exact-list negotiation:** if a client accepts a strict subset of the server's `select_known_packs`
    list, the server treats negotiated packs as empty and sends full NBT for all synchronized entries; client numeric
    IDs follow exact packet arrival order (including across multiple `registry_data` packets for one registry).
13. **Block-state enumeration order:** properties are sorted alphabetically by name, boolean properties order `true`
    before `false`, and the last property varies fastest; `minecraft:air` is always global state ID `0`, and absent /
    out-of-range lookups fall back to `0` / `minecraft:air`.
14. **Feature flags do not shift IDs and do not filter tag contents:** disabling experimental flags leaves all
    built-in registry IDs and block-state IDs unchanged; feature-filtered registry views hide disabled entries from
    single-key lookups and element listings only, not tag membership lists; an existing world automatically disables
    any data pack requesting unenabled flags.
15. **Holder and holder-set wire encoding off-by-one:** inline-capable holder encoding uses `0` for direct and `id + 1`
    for reference; holder-set encoding uses `0` + tag ID for named and `count + 1` + `count` holder IDs for direct.

## Open questions

1. **Multi-pack `filter` cross-product behavior:** a pack's `filter` section stores a list of `{namespace, path}` regex
   entries, and resource-manager setup checks namespace match and path match independently across the list (meaning two
   separate entries `{namespace: "a"}` and `{path: "tags/.*"}` can interact across namespaces). Confirm whether any
   vanilla or test data pack relies on this cross-product quirk when implementing `filter` support for data-pack
   loading.
2. **Command-stream empty identifier parsing:** the general command-stream identifier reader accepts zero consumed
   characters and returns `minecraft:`, whereas the non-empty variant rejects empty input. Verify per command argument
   type (in the command milestone) which callers use which variant.
3. **Mutual optional tag cycle determinism:** when two tags both hold `required: false` references to each other,
   vanilla's resolution order depends on Java `HashMap` iteration order over identifier keys. Confirm whether Zenith
   should reproduce exact `HashMap` iteration order for tag dependency sorting or treat mutual optional cycles as
   order-arbitrary unless an oracle fixture tests one.
4. **Empty static tag reload preservation:** when all data packs supply zero tags for a given static registry, vanilla
   skips pending tag reload creation for that registry rather than clearing previously applied tags. Confirm whether
   `/reload` removing the last custom tag from a static registry that has no vanilla tags exhibits this stale-tag
   retention in the oracle.
5. **Filesystem root-resolution scope for `..` path segments:** Section 1.2 states that resolving an identifier joins
   `root / namespace / path` and rejects targets escaping `root`, whereas Edge Case 2 states `"minecraft:../secret"` is
   rejected even though `root / minecraft / ../secret` normalizes to `root / secret` (escaping `root / minecraft`, not
   `root`). Confirm whether vanilla checks containment against `root / namespace`, rejects all `..` path segments
   (including non-escaping `minecraft:a/../b`), or rejects empty/`.` namespaces during pack path resolution. Implemented
   conservatively by enforcing containment within both `root` and `root / namespace` and rejecting leading `/` in paths.

## Reader review (2026-11-25)
The block-state enumeration rule above (alphabetical properties, `true` before `false`, last property fastest,
sequential IDs in block registration order) was checked independently against the 26.3 data-generator report: the
rule reproduces **35,723/35,723** global state IDs, with `minecraft:air` = 0.
