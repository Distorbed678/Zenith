# Codec: data packs, resource packs, and the codec framework (26.3)

- Verified by suites: `datapack-load`
- Sources: public data-pack and resource-pack documentation (`pack.mcmeta`, overlays, filters); the public
  documentation and behavior of the two libraries the game bundles for this: Gson **2.14.0** (JSON text) and
  DataFixerUpper **10.0.21** (the codec library, MIT-licensed); `version.json` inside the user's 26.3 client jar;
  local reference reading (2026-11-24); and local experiments (2026-11-24) that ran the game's own bundled libraries
  and, where noted, the game's own pack-metadata codecs from the user's jar against hand-written inputs. Experiment ids
  are cited as `[X:<id>]`. They are `json-strict`, `codec-coerce`, `codec-record`, `pack-format`, `overlays`,
  `filter`, `world-config` and `description`. Facts without an id come from reference reading. Everything below is
  written in our own words.
- Related specs: identifiers, registries, tags, feature flags, known packs and registry ID order are in
  [`registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) (called **REG** below).
  `level.dat` storage is in [`storage/region-and-level.md`](../storage/region-and-level.md).

## Summary

A **pack** is a directory or a `.zip` file with a `pack.mcmeta` at its root. Data packs keep their content under
`data/<namespace>/…`. Resource packs keep it under `assets/<namespace>/…`. The game finds packs from a fixed set of
sources and reads each pack's `pack.mcmeta`. A pack whose metadata is unusable is **dropped**. A pack with a valid but
mismatched version range is still **listed, and loaded if selected** (flagged as incompatible only in the client UI).
The game then picks an ordered **selection**: lowest priority first, the built-in `vanilla` pack normally at the bottom.

The selected packs form a **stack**. For any resource path, the topmost pack that has the file wins. Inside a pack,
active **overlays** sit above the pack's base content, and later-listed overlays win. A pack's `filter` hides matching
files from the packs **below** it. `.mcmeta` sidecar files attach metadata to the resource they shadow.

Reloading runs in a fixed order: static-registry tags, then the world registries, then dimensions, then the reloadable
registries (loot tables, predicates, item modifiers, advancements and recipes are registries in 26.3), then component
initializers and commands, then functions. Every JSON data file goes through **strict JSON** parsing and then a
**codec** that maps JSON to values. Unknown fields are ignored. Integers come from any JSON number by truncation and
wrap-around. There is no string↔number or boolean↔number coercion, and JSON `null` counts as an absent field. In 26.3
any decode error in a registry element aborts the whole load, recipes and advancements included. Tag files and
functions fail one file at a time.

---

## 1. Basics

### 1.1 Pack types

| Pack type | Content root | Current format in 26.3 | "Last pre-minor" major |
|---|---|---|---|
| Data pack (server data) | `data/` | **121.0** | **81** |
| Resource pack (client resources) | `assets/` | **97.1** | **64** |

The current formats come from the `pack_version` object in the jar's `version.json` (`data_major`/`data_minor`,
`resource_major`/`resource_minor`). The "last pre-minor" major is the newest format that came before minor versions
existed. It drives the legacy-field rules in §3.3 and §3.5.

### 1.2 Pack-format values

- A format value is a pair **(major, minor)** of 32-bit integers. Values compare by major first, then minor.
- When displayed, a minor of 2,147,483,647 shows as `<major>.*` (a wildcard meaning any minor). Any other minor shows
  as `<major>.<minor>`.
- A "range" is an inclusive `[min, max]` of format values with `min ≤ max`.

### 1.3 Pack identity

Each discovered pack has an **id** (a string, unique within its repository), a title, a description (a text
component), a **source kind**, optional **known-pack info** `(namespace, id, version)` (see REG §2.5), a set of
**requested feature flags**, a list of **active overlay directories**, a compatibility class (§3.4), and a selection
config (`required`, default position `TOP`/`BOTTOM`, `fixed position`).

| Source kind | Added automatically when new? | Used for |
|---|---|---|
| built-in | yes | `vanilla` data and resource pack, built-in resource packs |
| feature | **no** | built-in experimental data packs |
| world | yes | the world's `datapacks/` folder |
| server | yes | server-downloaded resource packs |
| default | yes | the client `resourcepacks/` folder |

---

## 2. Pack discovery

### 2.1 Data-pack sources (in order)

1. **Built-in source.** First the `vanilla` pack, then the feature packs (§2.3).
2. **World folder source.** `<world>/datapacks/` (§2.4).

The packs from all sources go into one map keyed by pack id. If an id repeats, the source processed later wins.
Ordinary ids never collide, because folder packs are prefixed `file/`. The map iterates in **ascending id order**
(Java `String` order: UTF-16 code units, so `file/B` < `file/a` < `vanilla`). That order is used wherever §4 says
"available order".

### 2.2 The `vanilla` data pack

| Property | Value |
|---|---|
| id | `vanilla` |
| source | built-in |
| known pack | `minecraft:core:26.3` |
| selection config | not required, default position BOTTOM, not fixed |
| namespaces | only `minecraft` (other directories under the jar's `data/` are not exposed) |
| metadata | **synthetic** (the jar has no root `pack.mcmeta`): description is the translatable `dataPack.vanilla.description`, range `[121.0, 121.*]`, requested features `{minecraft:vanilla}` |
| content | the jar's `data/minecraft/…` tree |

If a root `pack.mcmeta` exists in the game's resource roots (never in 26.3), its sections are preferred. Any section
it lacks, or fails to parse, falls back to the synthetic metadata.

### 2.3 Feature packs

- They are found inside the vanilla pack's content at `data/minecraft/datapacks/<id>/`. In 26.3 there are three:
  `minecart_improvements`, `redstone_experiments` and `trade_rebalance`.
- Detection follows the same entry rules as the world folder (§2.4). The pack id is the entry name with a trailing
  `.zip` removed. A `.zip` found inside the jar is refused, because archives are only opened from the real file
  system.
- Properties: source **feature** (never auto-added), known pack `minecraft:<id>:26.3`, title = the literal id, not
  required, default position TOP, not fixed. Their metadata is read from their own `pack.mcmeta`, which declares
  `features.enabled` and `min_format = max_format = 121`.

### 2.4 The world `datapacks/` folder

1. Create `<world>/datapacks/` if it is missing (resolving an existing symlinked path first).
2. Enumerate the folder's direct entries (no recursion; directory-listing order does not matter because of the sorted
   map). For each entry:
   1. Read its attributes **without following links**. If it vanished, skip it silently.
   2. If it is a **symlink**: check the link target against the world's symlink allow-list (`allowed_symlinks.txt`,
      default: nothing allowed). If not allowed, log "ignoring potential pack entry" and skip. Otherwise continue
      with the link target. A relative target is used as-is (see Open questions).
   3. If it is a **directory**: walk its whole tree. Any symlink inside that is not allowed makes the entire pack
      ignored. Otherwise it is a pack **only if `pack.mcmeta` is a regular file directly inside it**. If not, log
      "non-pack entry" and skip.
   4. If it is a **regular file whose name ends in `.zip`** (case-sensitive), it is a zip pack candidate. Its contents
      are not checked yet.
   5. Anything else (other files, `.ZIP`, sockets) is a non-pack entry and is skipped.
   6. An I/O error while reading one entry's properties logs a warning and skips only that entry.
3. Each candidate gets id **`file/<entry name>`**, keeping any `.zip` (for example `file/my_pack.zip` or
   `file/my_pack`). Title = the literal entry name. Source = world. No known-pack info. Not required, default position
   TOP, not fixed.
4. Read the candidate's metadata (§3). If it is unusable, the candidate is dropped and is not available.

A zip that cannot be opened (corrupt, unreadable) logs an error once. From then on it behaves as an **empty pack**: no
root files, no namespaces. Its metadata is therefore "missing" and the pack is dropped.

### 2.5 Resource-pack sources (client)

In order: (1) built-in: the client `vanilla` pack, then built-in packs; (2) server-downloaded packs; (3) the client
`resourcepacks/` folder.

- Client `vanilla`: id `vanilla`, known pack `minecraft:core:26.3`, **required**, BOTTOM, not fixed. Namespaces
  `minecraft` and `realms`. Two layers: the jar's `assets/` tree, then the hashed asset index as a lower layer. The
  synthetic range is `[97.1, 97.*]` and there is no features section.
- Built-in client packs are discovered under `minecraft:resourcepacks/` in either layer. In 26.3 these are
  `high_contrast.zip` and `programmer_art.zip` from the asset index. Archives *are* allowed here because the asset
  index is a virtual file system. Ids `high_contrast` and `programmer_art` (with `.zip` removed); translated titles;
  source built-in; known pack `minecraft:<id>:26.3`; not required; TOP.
- Server-downloaded packs: id `server/<8-digit uppercase hex serial>/<id>`, source server, **required, TOP, fixed**.
- Folder packs: same entry rules as §2.4, under `resourcepacks/`. Source default. Id `file/<entry name>`.

---

## 3. `pack.mcmeta`

### 3.1 How the file is read

- It is located at the pack root: a regular file inside a folder pack, or the zip entry named exactly `pack.mcmeta`.
- Bytes are decoded as UTF-8; malformed sequences become U+FFFD. JSON tokens are read in **strict** mode (§7.1), with
  three differences from data files: the root **must be a JSON object**; reading **stops after the root object**, so
  trailing text, comments or a second object after it are ignored; and an empty file is an error. `[X:json-strict]`
- The file is parsed once per pack-resources instance, the first time any section is needed. Each **section** is the
  value of one top-level key. Sections are decoded independently, and only when asked for. Unknown top-level keys are
  ignored.
- A decode error in a section is a hard error for that request. There are no partial results.

Data packs use four sections: `pack`, `features`, `overlays`, `filter`.

### 3.2 Reading order and outcomes (per discovered pack)

1. Decode `pack` with the strict schema (§3.3).
2. If step 1 raised an error (malformed JSON *or* a schema error), log a warning and decode `pack` again with the
   **fallback schema**: only `description` is read, and the range becomes `[2147483647.0, 2147483647.0]`, i.e.
   unknown. If the file itself is malformed JSON, this fails again.
3. If `pack.mcmeta` is absent, or has no `pack` key, log "missing metadata" and **drop the pack**.
4. Decode `features` (§3.6). If absent, the requested set is empty.
5. Classify compatibility (§3.4).
6. Decode `overlays` (§3.5) and keep the entries active for the current format.
7. **Any error raised in steps 2–6 drops the pack** with a "failed to read pack metadata" warning.

| Situation | Result |
|---|---|
| no `pack.mcmeta` / no `pack` key | dropped |
| malformed JSON (anywhere before the end of the root object) | dropped |
| `pack` fails the strict schema but has a valid `description` | **kept**, compatibility `UNKNOWN` |
| `pack` fails the strict schema and `description` is missing or invalid | dropped |
| `features` invalid (any unknown flag id, not a list, …) | dropped |
| `overlays` invalid (any entry invalid) | dropped |
| `filter` invalid | kept, filter ignored (error logged when the stack is built, §5.3) |
| valid but `TOO_OLD` / `TOO_NEW` | kept, loaded if selected (§4.6) |

### 3.3 The `pack` section

| Field | Type | Meaning |
|---|---|---|
| `description` | text component, **required** | A JSON string, a **non-empty** array of components, or an object with a content key (`text`, `translate`, …). Numbers, booleans, `{}`, `[]` and `null` are rejected. An object with an invalid style value (for example an unknown `color`) is rejected. `[X:description]` |
| `min_format` | format value, optional | "min" encoding (below) |
| `max_format` | format value, optional | "max" encoding (below) |
| `pack_format` | integer, optional | legacy single format. Any integer, including negatives. If present it must decode, or `pack` fails |
| `supported_formats` | integer range, optional | legacy range of majors |

**Format value encoding** (`min_format`, `max_format`, and the overlay keys of the same names):
- A JSON number `n` (non-negative, truncated to an integer) means `n.0` for a minimum and `n.*` for a maximum.
- A JSON array of **1 to 256** non-negative integers: `[a]` means `a.0` (minimum) or `a.*` (maximum). `[a, b, …]`
  means `a.b`, and elements after the second are ignored.
- Anything else, including negative numbers, `[]`, strings or arrays longer than 256, is a decode error.
  `[X:pack-format]`

**Integer-range encoding** (`supported_formats`, overlay `formats`): a single number `n` means `[n, n]`; an array of
**exactly two** numbers `[a, b]` with `a ≤ b`; or an object `{"min_inclusive": a, "max_inclusive": b}` with `a ≤ b`.

**Validation.** LPM is the last pre-minor major (81 for data, 64 for resources). M and X are the decoded min and max
formats; P is `pack_format`; S is `supported_formats`. Rules are tested in this order; the first one that applies
decides:

1. Exactly one of M and X is present: **error** (both must be declared).
2. M and X are both present:
   1. M > X (full major.minor comparison): **error**.
   2. If `M.major > LPM`:
      - S present: **error** (`supported_formats` is deprecated from LPM+1).
      - P present and outside `[M.major, X.major]`: **error**.
      - Otherwise the range is **[M, X]**.
   3. Otherwise (`M.major ≤ LPM`, a pack that also targets old game versions):
      - S absent: **error** (old versions need `supported_formats`).
      - `S.min ≠ M.major`: **error**.
      - `S.max ≠ X.major` **and** `S.max ≠ LPM`: **error**.
      - P absent: **error**.
      - P outside `[M.major, X.major]`, or `P < 15`: **error**.
      - Otherwise the range is **[M, X]**.
3. Only S is present (among M, X and S):
   - `S.max > LPM`: **error** (newer versions require `min_format`/`max_format`).
   - P absent: **error**.
   - P outside `[S.min, S.max]`, or `P < 15`: **error**.
   - Otherwise the range is **[S.min.0, S.max.0]**. The maximum minor is 0, not `*`.
4. Only P is present: if `P > LPM`, **error**. Otherwise the range is **[P.0, P.0]**.
5. Nothing is present: **error** (no format information).

Examples for data packs, current 121.0 `[X:pack-format]`:

| Input (besides `description`) | Range | Class |
|---|---|---|
| `"min_format":121,"max_format":121` | `[121.0, 121.*]` | COMPATIBLE |
| `"min_format":[121,1],"max_format":121` | `[121.1, 121.*]` | TOO_NEW |
| `"min_format":[121,2147483647],"max_format":121` | `[121.*, 121.*]` | TOO_NEW |
| `"min_format":121.7,"max_format":121.2` | `[121.0, 121.*]` | COMPATIBLE |
| `"min_format":121,"max_format":130,"pack_format":125` | `[121.0, 130.*]` | COMPATIBLE |
| `"min_format":80,"max_format":121,"supported_formats":[80,81],"pack_format":80` | `[80.0, 121.*]` | COMPATIBLE |
| `"min_format":80,"max_format":121,"supported_formats":[80,121],"pack_format":80` | `[80.0, 121.*]` | COMPATIBLE |
| `"pack_format":48` | `[48.0, 48.0]` | TOO_OLD |
| `"pack_format":-3` | `[-3.0, -3.0]` | TOO_OLD |
| `"supported_formats":{"min_inclusive":40,"max_inclusive":81},"pack_format":40` | `[40.0, 81.0]` | TOO_OLD |
| `"pack_format":121` | error, so fallback | UNKNOWN |
| `"min_format":121,"max_format":121,"supported_formats":[121,121]` | error, so fallback | UNKNOWN |
| `"min_format":121,"max_format":121,"pack_format":5` | error, so fallback | UNKNOWN |
| `"min_format":80,"max_format":121` | error, so fallback | UNKNOWN |
| `"pack_format":10,"supported_formats":[10,20]` | error (P < 15), so fallback | UNKNOWN |

### 3.4 Compatibility classification

Let C be the current format (121.0 for data) and `[lo, hi]` the pack's range:
1. If `lo.major = 2147483647`, the class is `UNKNOWN`. This happens only for the fallback, because 2147483647 is too
   large to pass rule 4 of §3.3 and any other route.
2. Else if `hi < C`: `TOO_OLD`.
3. Else if `C < lo`: `TOO_NEW`.
4. Else: `COMPATIBLE`.

Only `COMPATIBLE` counts as compatible. Translation keys for UI text: `pack.incompatible.old|new|unknown`, with
confirmations under `pack.incompatible.confirm.*`.

### 3.5 The `overlays` section

Shape: `{"entries": [ <entry>, … ]}`. `entries` is required; a missing `entries` is an error. Each entry has:

| Field | Required | Meaning |
|---|---|---|
| `directory` | yes | Overlay folder name, relative to the pack root. Must match `[-_a-zA-Z0-9.]+` entirely. Uppercase letters and dots are allowed; slashes are not. |
| `min_format`, `max_format` | optional | Format values, same encodings as §3.3 |
| `formats` | optional | Legacy integer range of majors |

Validation of the list:
1. Compute **minVersion**: for each entry, its effective minimum major is the smaller of `min_format.major` and
   `formats.min` when both are present, whichever is present otherwise, and +∞ when neither is. minVersion is the
   smallest of these over all entries. Set **requireOld = (minVersion ≤ LPM)**.
2. Go through the entries in order:
   1. If the entry has none of `min_format`, `max_format`, `formats`, log "unknown or broken overlay entry" and
      **skip it**. This is not an error.
   2. Otherwise validate it like §3.3, but with no `pack_format` checks:
      - exactly one of M and X present: error;
      - requireOld and `formats` absent: error. This rule applies to **every** entry once any single entry targets
        LPM or older;
      - M and X present: if M > X, error. If `M.major > LPM` and not requireOld: `formats` present is an error
        (deprecated); otherwise the range is [M, X]. Otherwise `formats.min` must equal `M.major` and `formats.max`
        must equal `X.major` or LPM, giving [M, X];
      - only `formats`: if `formats.max > LPM`, error; otherwise the range is `[formats.min.0, formats.max.0]`.
   3. **Any entry error fails the whole section**, so the pack is dropped (§3.2).
3. **Active overlays** are the entries whose range contains the current format, kept **in listed order**. This is
   decided once, when the pack is discovered.

Example `[X:overlays]`: entries `a` `[120, 121]`, `b` min 121 / max `[121,0]`, `c` `[122, 130]` give active overlays
`[a, b]`. An entry `{"directory":"x","formats":5}` is valid (`[5.0, 5.0]`) but inactive. A list with one entry using
`formats:[80,81]` and another using only `min_format:121,max_format:121` is **invalid**, because the second entry
lacks `formats`.

### 3.6 The `features` section

Shape: `{"enabled": [ <feature-flag id>, … ]}`. `enabled` is required and must be a list of identifiers (an omitted
namespace means `minecraft`). Any unknown flag id makes the section an error, so the **pack is dropped**. A
non-identifier string is also an error. `[]` is valid and means no flags. Flag semantics are in REG §5.

### 3.7 The `filter` section

Shape: `{"block": [ {"namespace": <regex>?, "path": <regex>?}, … ]}`. `block` is required. Each regex must compile
under **Java regular-expression syntax**; an invalid regex fails the whole section. Semantics are in §5.3.

---

## 4. Pack selection and ordering

The **selection** is an ordered list of pack ids, **lowest priority first**. The stack is built in exactly this order.

### 4.1 World configuration in `level.dat`

Inside `Data`:
- `DataPacks`: compound `{Enabled: [string…], Disabled: [string…]}`. **Both** lists are required inside it. The key is
  read **leniently**: if it is missing *or fails to decode for any reason* (for example `Disabled` is missing, or a
  non-string is in a list), the whole value silently becomes the default `{Enabled: ["vanilla"], Disabled: []}`.
  `[X:world-config]`
- `enabled_features`: list of flag ids, read **leniently** with default `[minecraft:vanilla]`. An unknown flag id
  makes the whole list fall back to the default, so other valid flags such as `minecraft:trade_rebalance` are lost
  too. `[]` is valid and stays empty. `[X:world-config]`

### 4.2 Configuring the repository (world load)

Inputs: the stored `DataPacks` (E = Enabled, D = Disabled), and a mode.

| Mode | forced features | allowed features |
|---|---|---|
| new world (init) | ∅ | all known flags |
| existing world | stored `enabled_features` | stored `enabled_features` |
| safe mode | (as for the world's mode) | (not used) |

Steps:
1. Rediscover all packs (§2). The previous selection is kept by id across rediscovery.
2. **Safe mode:** the selection is exactly `["vanilla"]`. Go to step 6, with *disable-inactive* off.
3. Start an insertion-ordered set S. For each id in E, in order: if available, add it to S (duplicates collapse to the
   first occurrence); otherwise log "missing data pack" and skip it.
4. For each available pack in available order (§2.1) whose id is **not** in D:
   1. If it is not in S, its source is added automatically (anything but *feature*), and its requested features ⊆
      allowed, append it to S and log "found new data pack, loading it automatically". If the features are not
      allowed, log that and skip it.
   2. If it **was** already in S (from E) and its requested features ⊄ allowed, remove it from S and log a warning.
   3. Packs whose id is in D are skipped entirely, **including the feature check of sub-step 2**.
5. If S is empty, add `vanilla`.
6. Apply S as the selection. Then **force feature packs**:
   1. missing = forced − (union of the selected packs' requested features).
   2. If missing is non-empty: go through the available packs in available order, stopping once missing is empty. A
      pack qualifies if its source is *feature*, its requested set is non-empty, it intersects missing, and it is a
      subset of forced. Append each qualifying pack to the selection, log it, and remove its flags from missing.
      Forcing a pack that is already selected is an internal error that cannot happen in practice.
7. Result:
   - **Enabled** = the final selection, in order.
   - **Disabled** = every available id not selected, in available order. When disable-inactive is off (safe mode),
     Disabled is **empty**.
   - **enabled_features** = union of every selected pack's requested features, plus forced.

### 4.3 Dedicated-server new worlds

With no `level.dat`, `server.properties` supplies E = `initial-enabled-packs` (default `vanilla`) and
D = `initial-disabled-packs` (default empty). Each is split on `,` with surrounding whitespace trimmed and empty items
kept, so an empty property gives a single `""` id, which then logs "missing data pack". The world is configured in
new-world mode, starting from features `[minecraft:vanilla]`. Feature packs (never auto-added) become active only if
listed in `initial-enabled-packs`. A server started with `--safeMode` uses safe mode.

### 4.4 `/reload`

1. Rediscover packs.
2. New selection = the current selection in order, then every available id that is in neither the current selection
   nor the world's stored **Disabled** list, appended in available order. **There is no source check and no feature
   check here.** A world pack dropped into `datapacks/` while running, or one requesting flags the world lacks, is
   enabled by `/reload` and its content is loaded.
3. Open the packs in that order and run the reload (§6.2). The world's feature set is **not** changed by a reload.
4. On success, the selection is stored. Enabled = the selection; Disabled = every other available id; features
   unchanged.
5. The "reload success" message is sent **before** the reload runs. If the reload then fails, a second "reload
   failure" message follows, and the previous resources, selection and stored configuration stay in effect.

The `/datapack enable|disable` command changes the selection and then triggers the same reload. Its argument rules are
for the command spec. Enabling only offers packs whose requested features ⊆ the world's enabled features.

### 4.5 Required and fixed-position packs

After any selection is applied, each available pack marked **required** that is missing from the selection is inserted
by its default position:
- **BOTTOM**: before the first pack that is not (fixed-position with default BOTTOM). In other words, just above the
  leading run of fixed bottom packs.
- **TOP**: after the last pack that is not (fixed-position with default TOP). In other words, just below the trailing
  run of fixed top packs.

Required packs are inserted in available order. No data pack is required in 26.3, so this matters only for resource
packs (client `vanilla`, server-downloaded packs). Selections may contain each pack only once in practice. The world
path dedupes through the set in §4.2, and the client path through its own set.

### 4.6 Incompatible packs load

The server **never** filters by compatibility. A selected `TOO_OLD`, `TOO_NEW` or `UNKNOWN` data pack is opened and its
content is loaded like any other. Only lists printed for crash reports mark such packs with ` (incompatible)`. The
client pack screen asks for confirmation before enabling one.

### 4.7 Resource-pack selection (client `options.txt`)

The keys are `resourcePacks` and `incompatibleResourcePacks`, both JSON arrays of ids. On startup, for each id in
`resourcePacks`, in order:
1. Look the id up. If it is absent and does not start with `file/`, try `file/<id>` (legacy names).
2. If it is still absent, remove it from the options and log.
3. Else if the pack is not compatible and the *original* id is not in `incompatibleResourcePacks`, remove it from the
   options.
4. Else if the pack is compatible and the original id **is** in `incompatibleResourcePacks`, remove it from that list.
   Quirk: the pack is **not** selected in this pass, though it stays in `resourcePacks` for the next start.
5. Otherwise select the pack (by its real id).

Required packs are then inserted (§4.5). When saving, fixed-position packs are left out of `resourcePacks`, and every
saved pack that is incompatible is also written to `incompatibleResourcePacks`.

---

## 5. Resource lookup across the pack stack

### 5.1 Layers

From the bottom up: the selected packs in selection order (index 0 is the lowest priority). Inside each pack: its base
content at the bottom, then its **active overlays in listed order**. The last-listed active overlay has the highest
priority within the pack. Overlays contribute files and namespaces only. Metadata (`pack.mcmeta`, `filter`) always
comes from the pack's base root, and an overlay's own `pack.mcmeta` is ignored. Overlays of a folder pack live at
`<pack>/<directory>/data/…`; overlays of a zip live under entries `<directory>/data/…`. A missing overlay directory
contributes nothing.

### 5.2 Paths inside one pack

A resource identifier `ns:path` maps to `data/<ns>/<path>` (or `assets/…`).

| Aspect | Folder pack | Zip pack |
|---|---|---|
| Direct lookup | Every `/`-separated segment of `path` must match `[-._a-z0-9]+` and must not be `.` or `..`, and the path must have no empty segment. A violation logs "invalid path" and returns *not found*. The file must exist. | The literal entry name `data/<ns>/<path>` is looked up, with **no segment validation** and no normalization. Names are case-sensitive. |
| Namespaces | Direct sub-**directories** of `data/` whose names are valid namespaces. Plain files there log a warning and are rejected. Invalid names log and are ignored. | Any entry name `data/<x>` or `data/<x>/…` with a non-empty `<x>` that is a valid namespace, **including a plain file named `data/<x>`**. Invalid names log and are ignored. |
| Listing `ns` + directory `d` | Recursively walk every **regular file** under `data/<ns>/<d>/`, at any depth. Symlinks are not followed and are not regular files. A missing directory gives nothing. | Every non-directory entry whose name starts with `data/<ns>/<d>/`. |
| Listed names | The relative path, joined with `/`, is turned into an identifier. A name with characters outside `[a-z0-9/._-]` (for example uppercase) logs and is **skipped**. | Same. |

Listing directories must not end with `/`; asking for one is an internal error.

### 5.3 Namespace routing and filters

Building the stack:
1. All namespaces = the union of every selected pack's namespaces, as in §5.2, including overlays.
2. For each namespace N, a per-namespace stack is built in pack order. For each pack, decode its `filter` section. If
   that fails, log an error and treat the pack as having no filter. Then:
   - **filter active for N**: some `block` entry's namespace regex **finds a match anywhere** in N (unanchored
     search). An entry with no `namespace` matches every N.
   - The pack has N and its filter is active: push the pack with its path filter.
   - The pack has N and its filter is not active: push the pack with **no** filter.
   - The pack lacks N but its filter is active: push a **filter-only** layer.
   - Otherwise the pack takes no part in N.
3. **Path filter**: a full resource path (for example `recipe/stone.json`, including the category directory and the
   extension, and also applied to sidecar paths such as `…/x.png.mcmeta`) is blocked when **some** `block` entry's
   path regex finds a match anywhere in it. An entry with no `path` matches every path.
4. Namespace and path are tested **independently across entries** (a cross product). With
   `[{namespace:"a"}, {path:"x"}]` the filter is active for every namespace (the second entry has no namespace) and
   blocks every path (the first entry has no path), so it hides **everything** below. `[{}]` also hides everything.
   `[]` hides nothing. `[X:filter]`
5. A filter hides files from **lower** layers only, never from its own pack or higher packs.

Lookups for an identifier in a namespace that no pack provides find nothing.

### 5.4 Single-resource lookup (one winner)

Go through the namespace's layers from the **top** down. At each layer: if the pack has the file, **return it**.
Otherwise, if the layer's filter blocks the path, stop and return *not found*, logging "not found, but was filtered by
pack X". Its sidecar is the topmost `<path>.mcmeta` found by a separate top-down search from the top layer down to
**and including** the winner's layer. That search stops at the first layer whose filter blocks the sidecar path. So a
higher pack's sidecar can attach to a lower pack's file, even if the higher pack lacks the file.

### 5.5 Resource-stack lookup (all versions)

Go through the layers from the top down. Collect every layer that has the file. Stop **after** the first layer whose
filter blocks the path: that layer's own file is kept, and lower ones are excluded. Each collected version uses only
**its own pack's** sidecar, or none if a higher layer's filter blocked the sidecar path. Return the versions
**bottom-up** (lowest pack first).

### 5.6 Directory listing (one winner per file)

Used by registry loading and functions (§5.9). For directory `d` and a path predicate (usually "ends with `.json`"):
1. Go through the layers from the **bottom** up. Before each layer, remove from the results everything its filter
   blocks: files that are already found, and sidecars, by their own paths. Then add the layer's listed files, so that
   higher layers overwrite lower ones.
   - A listed name ending in **`.mcmeta` is always treated as a sidecar** and never as a resource. The predicate is
     tested on the name with `.mcmeta` removed.
2. Each surviving file gets the surviving sidecar only if that sidecar's layer is the **same or higher** than the
   file's layer.
3. The output is ordered by identifier natural order (path first, then namespace; REG §1.4).

### 5.7 Directory listing (stacks)

Used by tags. As in §5.6, go bottom-up. Before adding a layer's files, its filter **clears the accumulated versions**
of every file whose path it blocks, and clears accumulated sidecars it blocks. Then append the layer's versions. A file
whose version list ends up empty is omitted. Each version uses its own pack's sidecar. Versions are kept lowest pack
first, and the output is ordered by identifier.

### 5.8 Sidecars in data packs

No 26.3 server-side data loader reads sidecar metadata. The rules matter for resource packs (texture animation and
similar). For data packs the only visible effect is that **files ending in `.mcmeta` are never listed as data**.

### 5.9 Which consumer uses which mode

| Consumer | Location | Mode |
|---|---|---|
| Dynamic registries (world, dimension, reloadable) | `data/<ns>/<registry path>/**.json` | listing, one winner (§5.6) |
| Tags (all registries, including function tags) | `data/<ns>/tags/<registry path>/**.json` | listing, stacks (§5.7), merged per REG §4.2 |
| Functions | `data/<ns>/function/**.mcfunction` | listing, one winner |
| Structure templates | `data/<ns>/structure/**.nbt` | looked up on demand per id, cache cleared on reload (own spec, later) |

Registry paths are the singular registry ids (`recipe`, `loot_table`, `tags/block`, `tags/function`, …). Legacy plural
folders (`recipes/`, `loot_tables/`, `tags/blocks/`, `functions/`) are not read at all: there is no error, and the
files are simply invisible. The element id is the file path with the category prefix and extension removed, so a file
named exactly `.json` gives an element with an **empty path** (`ns:`).

---

## 6. Reload pipeline

### 6.1 World load (server start, or opening a singleplayer world)

1. Configure the pack repository (§4.2). Open the selected packs in order and build the stack (§5).
2. Load **tags for all static (built-in) registries** from the stack. They are held as *pending* and are not applied
   yet. A registry with zero loaded tags gets no pending reload (REG §4.5).
3. Load the **world-layer** registries (REG §2.4; 51 registries). Their element codecs see the static registries with
   the pending tags. Each registry lists, decodes, sorts, registers, and loads its own tags. Then all of them are
   frozen and validated. **Any error anywhere aborts** (§7.6).
4. Load the **dimension layer** (`dimension`, the level stems) with the static and world registries visible. Abort on
   any error.
5. Read or create the world data (`level.dat`, world-gen settings) using the loaded dimensions.
6. Load the **reloadable layer** (`loot_table`, `predicate`, `context_float_provider`, `context_int_provider`,
   `item_modifier`, `slot_source`, `advancement`, `recipe`) with everything above visible. Abort on any error.
7. **Validate loot data** across loot tables, predicates and modifiers. Problems are logged as warnings. Any *fatal*
   problem aborts.
8. Build data-component initializers.
9. Build the command tree (filtered by the world's enabled features), the recipe manager and the advancement tree from
   the loaded registries.
10. Run reload listeners. On the server there is exactly one: the **function library** (§6.3). It loads function tags
    and all `.mcfunction` files.
11. On the main thread, apply the pending static-registry tags and the data components.
12. If any step fails, close the opened packs and report the failure (§7.7).

### 6.2 `/reload`

1. Pick the selection (§4.4), open those packs, and build a new stack.
2. Load tags for **every existing registry**: static, world, dimension and reloadable. They are held as pending.
3. Do steps 6–10 of §6.1 again against the existing static, world and dimension registries.
   **World-layer and dimension-layer elements are not reloaded** (biomes, dimension types, enchantments, …). **Their
   tags are reloaded.**
4. On success, on the main thread: close the old stack and swap in the new one; store the selection and configuration
   (§4.4); apply the pending tags and components; finalize recipes for the enabled features; save every player;
   resend the reloaded data to connected players (tags, recipes, advancements, commands, …); replace the function
   library; clear the structure-template cache.
5. On failure: the old state stays (§4.4, step 5).

### 6.3 Reload-listener scheduling

This applies to the function library and the client resource reload. All listeners **prepare in parallel**. A
listener's **apply** step starts only after *every* listener has finished preparing *and* the previous listener's apply
step has finished. So applies run strictly in **registration order**. The first failure fails the whole reload.

Function library details:
- Files are read as UTF-8 (malformed bytes become U+FFFD) and split into lines on `\n`, `\r\n` or `\r`.
- Each function compiles independently. A failure logs "failed to load function" and that function is **omitted**.
  The reload continues.
- Function tags are built after the functions; a reference to a missing function counts as a missing element (REG §4).

Client resource reload listeners, in registration order (informative; details belong to the client and render specs):
language, textures, shaders, sounds, atlases, fonts, grass, foliage and dry-foliage colormaps, models, equipment
assets, paletted textures, entity renderers, block-entity renderers, particles, game renderer, level extractor,
clouds, GPU warn-list, regional compliance, and then the GUI's own listeners (splashes, then the HUD).

---

## 7. Codec framework: how data files decode

### 7.1 JSON text

Data files (registry elements, tags) are read as UTF-8 (malformed bytes become U+FFFD) and parsed in **strict** mode.
`pack.mcmeta` and sidecars use the same token rules with the differences in §3.1. `[X:json-strict]`

| Input | Data files | `pack.mcmeta` / sidecars |
|---|---|---|
| leading UTF-8 BOM | accepted (skipped) | accepted |
| `//` or `/* */` comments | rejected | rejected inside the object; ignored after it |
| trailing comma `[1,2,]` / `{"a":1,}` / `[1,,2]` | rejected | rejected |
| single-quoted strings, unquoted keys | rejected | rejected |
| `NaN`, `Infinity`, leading zeros `01`, `1.` | rejected | rejected |
| unescaped control characters (U+0000–U+001F) inside strings | rejected | rejected |
| unknown escapes such as `\'` | rejected | rejected |
| `-0`, `1e400` (out-of-range numbers) | accepted as JSON | accepted |
| duplicate keys | **last value wins** | last value wins |
| top-level primitive or array | accepted (the codec then decides) | rejected (must be an object) |
| content after the root value | rejected | ignored |
| empty file / whitespace only | parses as JSON `null` (the codec then fails) | rejected |

### 7.2 JSON values to primitive values

`[X:codec-coerce]`

| Target | Accepts | Rule |
|---|---|---|
| boolean | JSON `true`/`false` only | Numbers and strings are rejected ("not a boolean") |
| string | JSON strings only | Numbers, booleans and arrays are rejected ("not a string") |
| 32-bit int | any JSON number | Take the decimal text. If it fits a 32-bit int, use it. Else if it is an integer that fits 64 bits, keep the **low 32 bits** (`3000000000` gives `-1294967296`). Else treat it as an exact decimal, **truncate toward zero**, and keep the low 32 bits (`1.9` gives `1`, `-1.9` gives `-1`, `1e2` gives `100`, `1e20` gives `1661992960`, `1e400` gives `0`). A decimal exponent outside about ±10000 is a **hard failure**: an exception, not a decode error with a message. Strings and booleans are rejected ("not a number") |
| 8/16-bit int | any JSON number | as 32-bit int, then keep the low 8 or 16 bits (`300` gives `44` as a byte) |
| 64-bit int | any JSON number | as above, at 64 bits (`1e3` gives `1000`; 2^63 wraps to -2^63; `1.5` gives `1`) |
| float | any JSON number | the decimal text is rounded **directly to the nearest float**, not by way of double; overflow gives ±∞ (`1e40` gives ∞) |
| double | any JSON number | nearest double; overflow gives ±∞ |
| any | JSON `null` | rejected as a value; as an object field it means "absent" (§7.3) |

Range-checked numbers (for example "non-negative", "within [a; b]") are checked **after** this conversion, so `-0.5`
passes a non-negative check as `0`.

### 7.3 Objects (records)

`[X:codec-record]`
- The input must be a JSON object, otherwise "not a JSON object".
- **Unknown keys are ignored** everywhere. There is no strict-keys mode.
- A field whose value is JSON **`null` behaves exactly as if absent.**
- **Required field**: if absent or null, the error is "no key `<name>`".
- **Optional field with a default** (the usual kind): if absent or null, use the default. If present but invalid, the
  record **fails**. The partial result uses the default for that field.
- **Lenient optional field** (used in a few places, such as world configuration): if present but invalid, use the
  default silently, with no error. A partially valid list there becomes the default, not the valid part.
- **Optional without a default**: absent or null gives "empty". Invalid gives an error.
- On encode, a field equal to its default is **omitted**, and an empty optional is omitted. This matters when values
  are re-encoded (for example registry data sent to clients as NBT; REG §2.5).
- All field errors of one object are collected and joined with `"; "`. Every field is always checked; checking does
  not stop at the first failure.

### 7.4 Composite shapes

- **List**: the input must be a JSON array. Each element decodes independently. If any element fails, the list fails;
  the partial result is the list of the elements that succeeded, in order. A size-bounded list rejects too few or too
  many elements; when too long, its partial result is cut to the maximum.
- **Map keyed by strings** (identifiers, names): the input must be a JSON object. Keys and values decode independently.
  A failed entry makes the map fail, and the partial result keeps the good entries. Duplicate keys cannot occur (§7.1).
- **Either / alternatives**: try the first form. If it fails, try the second. The first success wins, so input that
  fits both forms decodes as the first. If both fail, the message names both failures. Common shapes built this way:
  "one element or a list" (a lone element is treated as a one-element list), "id string or inline object",
  "number or [min, max] or {min_inclusive, max_inclusive}".
- **Type-dispatched objects** (`"type": "<id>"` plus the type's own fields in the **same** object): a missing `type`
  is "no key type". An unknown type id is an error. The remaining keys are decoded by the chosen type's schema, and
  extra keys are ignored. **On encode the chosen type's own fields are written first and the discriminator key
  (`type`, or `action` for text events) last**, e.g. `{"title":"T","type":"minecraft:notice"}`. This holds for every
  dispatched object built with the codec library's key-dispatch helper. *(Settled by the `text-component` suite,
  2026-11-25.)*
- **Identifiers, resource keys, holders and tags**: REG §1 and REG §2.6. A referenced id that is missing from a frozen
  registry is a decode error. A reference into a registry that is still loading creates a placeholder, and the failure
  shows up when that registry is frozen.
- **Validated values** (ranges, non-empty strings, regexes, enums by name): the base value decodes first, then the
  check runs. A failed check is a decode error. An enum name that does not exist is an error, with no case folding.

### 7.5 Decode results

Every decode yields exactly one of these:
- **success** (a value);
- **error with partial**: a message plus a best-effort value (good list elements, good map entries, defaults
  substituted for bad optional fields, recognized feature flags);
- **error without partial**.

Consumers either **demand success** (a partial result counts as failure), **accept partial** (log the message, keep
the partial value), or **fall back** to a default. The table in §7.6 says which.

### 7.6 Error handling per consumer

| Consumer | On a decode or parse error |
|---|---|
| `pack.mcmeta` sections | demand success; see §3.2 for drop or fallback |
| world `DataPacks` / `enabled_features` | lenient; fall back to defaults silently (§4.1). The rest of the world config is accepted from partial results, with errors logged |
| dynamic-registry element (world, dimension, reloadable: biomes … loot tables, recipes, advancements) | demand success. Malformed JSON, a schema error, *or a partial result* records an error for that element. After the whole batch finishes, **any recorded error aborts the entire load**, and every error in the batch is reported together |
| registry freeze / validators | same batch-wide abort (REG §2.4) |
| loot validation | warnings logged; a fatal problem aborts |
| tag file | that pack's file is skipped with an error log; the rest continues (REG §4.2) |
| function file | that function is omitted; the rest continues |

### 7.7 What the user sees

The exact message text is not part of parity; the content and grouping are.
- **Registry load failure.** The log carries one block grouped by registry, sorted by registry id. Inside each, the
  elements are sorted by element id, and each shows its full error chain. An element's error says *which element
  failed in which pack* (`Failed to parse <element id> from pack <pack id>` in spirit), wrapping the codec message. A
  crash report titled "Registry Loading" lists `<registry>/<element>: <message>` lines sorted by registry, then
  element.
- **Dedicated server.** It logs that data packs failed to load and that the server cannot continue, suggesting either
  fixing the packs or restarting with `--safeMode`. Then it stops.
- **Singleplayer.** A "data pack failed to load" screen offers going back or **safe mode** (vanilla only, §4.2). If
  safe mode also fails, a second alert says so, and the world is not opened.
- **`/reload`.** The reload-success message comes immediately. If the reload fails, a reload-failure message follows
  and the details are in the log.
- **Codec message meanings.** These cover: a missing key; a wrong JSON type ("not a number / boolean / string /
  JSON object / json array"); both alternatives failed (each one named); a list too short or too long, with its
  allowed range; a value outside a range; an invalid identifier; unknown feature ids (listed); a failed map entry (the
  message names the offending key/value pair); and the `pack.mcmeta` format rules in §3.3 (each rule's message states
  the fix, such as "add `supported_formats`" or "remove `supported_formats`"). Several messages for one object are
  joined with `"; "`.

---

## Constants and IDs

| Constant | Value |
|---|---|
| Data / resource content roots | `data/`, `assets/` |
| Metadata file / sidecar suffix | `pack.mcmeta`, `.mcmeta` |
| Current data-pack format (26.3) | `121.0` (`version.json` → `pack_version.data_major/minor`) |
| Current resource-pack format (26.3) | `97.1` |
| Last pre-minor major (data / resource) | `81` / `64` |
| Minimum legacy multi-version `pack_format` | `15` |
| Wildcard / unknown format component | `2147483647` (shown as `*`) |
| Format-array length bounds | `1`..`256` |
| Overlay directory pattern | `[-_a-zA-Z0-9.]+` (whole string) |
| Path segment pattern (folder lookups) | `[-._a-z0-9]+`, not `.` or `..` |
| Pack ids | `vanilla`; feature packs `minecart_improvements`, `redstone_experiments`, `trade_rebalance`; folder `file/<name>`; client built-ins `high_contrast`, `programmer_art`; downloaded `server/%08X/<id>` |
| Known packs | `minecraft:core:26.3` (both vanillas); `minecraft:<id>:26.3` (built-in feature and resource packs) |
| `level.dat` keys | `Data.DataPacks.Enabled`, `Data.DataPacks.Disabled`, `Data.enabled_features` |
| Default world data config | Enabled `["vanilla"]`, Disabled `[]`, features `[minecraft:vanilla]` |
| `server.properties` keys | `initial-enabled-packs` (default `vanilla`), `initial-disabled-packs` (default empty) |
| `options.txt` keys | `resourcePacks`, `incompatibleResourcePacks` |
| Feature-pack folder in vanilla | `data/minecraft/datapacks/` |
| Built-in resource-pack folder | `minecraft:resourcepacks/` (asset index) |
| Function files / function tags | `function/**.mcfunction`, `tags/function/**.json` |
| Bundled libraries | Gson `2.14.0`, DataFixerUpper `10.0.21` |

---

## Edge cases a naive implementation gets wrong

1. **Strict JSON but BOM-tolerant, duplicate keys last-wins:** a UTF-8 BOM is accepted, while comments, trailing commas,
   `NaN`, leading zeros, raw control characters in strings and `\'` are all rejected. `{"a":1,"a":2}` silently means
   `a = 2`. `[X:json-strict]`
2. **`pack.mcmeta` ignores everything after its root object:** `{…} // comment` or `{…} garbage` is a valid pack, while
   the same trailing text makes a data file invalid. An empty data file parses as `null`, which then fails its codec.
3. **Integer decoding truncates and wraps instead of rejecting:** `1.9` reads as `1`, `-1.9` as `-1`, `3000000000`
   as `-1294967296`, `1e20` as `1661992960`, `300` as the byte `44`. Floats parse directly from decimal to float.
   Absurd exponents throw an exception instead of giving a decode error. `[X:codec-coerce]`
4. **No type coercion:** `"5"` is not a number, `1` is not a boolean, `5` is not a string, and `5` is not a valid
   pack `description`.
5. **`null` counts as absent:** a required field set to `null` reports "no key". An optional field set to `null` takes
   its default. A `null` list element or map value is an error.
6. **Strict vs lenient optional fields:** a present-but-invalid optional field fails the whole object, apart from the
   few lenient fields, which silently take their default.
7. **One bad registry file kills the whole load:** in 26.3 recipes, advancements, loot tables, predicates and item
   modifiers are registries. Any malformed or partially invalid element aborts world load or `/reload`, with every
   error reported together. Tags and functions fail one file at a time.
8. **Dropped vs incompatible packs:** a missing `pack.mcmeta`, malformed JSON, an unknown feature flag, an invalid
   overlay entry or an invalid `description` **drops** the pack. A pack with an invalid format declaration but a valid
   `description` is **kept** as `UNKNOWN`. `TOO_OLD`, `TOO_NEW` and `UNKNOWN` packs are loaded on the server when
   selected.
9. **Format edge cases:** `pack_format` alone above 81 is an error. `pack_format` alone at 81 or below (even negative)
   is a valid single-format range. `[121,1]` as `min_format` is `TOO_NEW` on 121.0. A bare number as `max_format`
   means `n.*`, and in `min_format` it means `n.0`. Decimals truncate (`121.7` becomes 121). A legacy
   `supported_formats` range ends at minor `0`, not `*`. `[X:pack-format]`
10. **Mixed legacy/new declarations:** `min_format ≤ 81` requires `supported_formats` starting exactly at
    `min_format.major` and ending at `max_format.major` **or** 81, plus a `pack_format` within
    `[min.major, max.major]` and ≥ 15. With `min_format > 81`, `supported_formats` is forbidden.
11. **Overlay all-or-nothing legacy rule:** if any one overlay entry reaches format 81 or older, *every* entry must
    carry `formats`. Entries with no version fields are skipped with a warning, not rejected. Any invalid entry drops
    the whole pack. Active overlays keep their listed order, and the last one wins. `[X:overlays]`
12. **Overlay directory names:** uppercase letters and dots are allowed, slashes are not. `.` resolves to the pack root
    itself, and for folder packs `..` resolves to the pack's parent folder (see Open questions).
13. **Filter cross product:** namespace and path patterns are matched independently across `block` entries, as
    unanchored regex *searches*. `[{namespace:"a"},{path:"x"}]` and `[{}]` both hide every lower-pack file in every
    namespace. Paths are full paths such as `recipe/x.json`, not element ids. An invalid `filter` section is ignored,
    and the pack still loads. `[X:filter]`
14. **Filters never hide the declaring pack's own files**, nor those of higher packs. In single lookups a filter
    returns "not found" immediately; in stack lookups the filtering layer's own version is kept.
15. **Sidecar ownership differs by lookup mode:** a single lookup or one-winner listing may attach a sidecar from a
    *higher* pack to a lower pack's file, but never one from a lower pack. Stack lookups use only each version's own
    pack's sidecar. Files ending in `.mcmeta` are never listed as data.
16. **Folder vs zip differences:** a zip lookup does not validate path segments; a zip counts a plain file `data/foo`
    as namespace `foo`; folder packs need a `pack.mcmeta` regular file to be detected at all, while any `*.zip`
    regular file is a candidate (`.ZIP` is not). World-folder ids are `file/<entry name>` verbatim, so a zip keeps its
    extension (`file/x.zip`). Built-in feature-pack ids drop a trailing `.zip`.
17. **Uppercase file names are silently skipped:** a listed file whose relative path is not a valid identifier (for
    example `recipe/Foo.json`) logs and is ignored. Legacy plural folders (`recipes/`, `tags/blocks/`) are simply not
    read.
18. **`level.dat` lenient fallbacks lose data:** `DataPacks` with a missing `Disabled` list (or any bad value) resets
    to `Enabled:["vanilla"]`. An `enabled_features` list containing one unknown flag resets to `[minecraft:vanilla]`,
    dropping valid experimental flags. `[]` stays empty. `[X:world-config]`
19. **Selection order:** `Enabled` order is kept (first = lowest priority, duplicates collapse). Auto-added packs are
    appended in UTF-16 id order (`file/B` before `file/a` before `vanilla`), and forced feature packs come last.
    Feature packs are never auto-added; world, built-in and server packs are.
20. **Pack in both `Enabled` and `Disabled`:** it stays enabled and **skips the feature check**. Its requested flags
    are then *added* to the world's enabled features.
21. **Safe mode is not sticky:** a safe-mode load stores `Enabled` = vanilla (plus forced feature packs) and an
    **empty** `Disabled`. On the next normal load, every world pack (but no feature pack) is auto-enabled again,
    including ones that were disabled before.
22. **`/reload` has no feature or source check:** it appends every available pack that is not in `Disabled`. It does
    not reload biomes, dimension types or other world-layer elements, but does reload their tags. Its success message
    is sent before the reload runs, and a failure keeps the old data.
23. **Empty `initial-enabled-packs`:** splitting `""` yields one empty id (a "missing data pack" warning), not an
    empty list.
24. **Client options quirk:** a pack listed in `incompatibleResourcePacks` that has become compatible is removed from
    that list and is *not* selected in that startup pass. Legacy ids without `file/` are resolved by adding it.

## Open questions

1. **Java regex dialect for `filter`:** patterns follow Java `Pattern` syntax with unanchored search. Rust's `regex`
   crate lacks look-around, backreferences and possessive quantifiers, and differs on some classes. Should Zenith embed
   a Java-compatible engine (for example `fancy-regex` plus a translation layer), or reject unsupported constructs and
   log them like an invalid regex? An oracle fixture should check `(?i)`, `\Q…\E`, look-ahead and `^`/`$` anchoring.
2. **Relative symlink targets in `datapacks/`:** a symlinked entry with an allowed relative target is re-read using
   the target path as given, which may resolve against the process working directory rather than the link's folder.
   Confirm with the oracle whether a relative pack symlink works at all before matching it.
3. **Overlay `..` / `.` directory names:** `..` passes the directory-name check and, for folder packs, would read
   overlay content from the parent `datapacks/` folder. Should Zenith replicate this, or treat it as a
   security-relevant bug that is not reproduced? It is observable, but it is a path-escape.
4. **Case-insensitive file systems:** direct lookups use the OS file system, so on Windows/macOS a request for
   `recipe/foo.json` finds `Recipe/Foo.json`, while listings skip it. Zenith targets Linux first; decide whether to
   emulate host case-folding on other platforms.
5. **Server-downloaded resource-pack order:** the required/fixed TOP insertion rule (§4.5) places later-inserted
   server packs *below* earlier ones. The downloaded-pack source may reorder them on its own; verify with a
   multi-pack server oracle scenario.
6. **Hard numeric failure threshold:** the exact exponent limit at which integer decoding throws (about ±10000 in Gson
   2.14.0) and how it surfaces through each consumer (it is caught per element in registry and tag loading) should be
   pinned by a `datapack-load` fixture.
7. **Feature-flag partial in other consumers:** `features` in `pack.mcmeta` demands success, and the world config is
   lenient. Other places that decode flag lists (for example dimension or world-preset data) may accept the partial
   set; this needs checking per consumer when those specs are written.
8. **(implementer) Literal case in JSON keywords:** the spec table says nothing about `TRUE`, `True` or `nULL`. The
   implementation accepts only the exact lowercase literals `true`, `false` and `null` (conservative). Check with a
   `json-strict` fixture whether the game's strict reader also accepts other letter cases.
9. **(implementer) Lone surrogate escapes:** `"\ud83d"` (an unpaired surrogate) is a valid JSON string for the game's
   reader, and the string then carries a lone UTF-16 unit. Rust strings cannot, so the implementation substitutes
   U+FFFD. Identifiers and pack ids never contain surrogates, so only free-text fields could differ.
10. **(implementer) Exact exponent limit:** the integer-read "hard failure" uses `|exponent| > 10000` on the exponent
    **as written in the text** (so `1e10000` still reads as `0`, `1e10001` fails). Spec §7.2 says "about ±10000"; the
    exact bound and whether it applies to the exponent or to the resulting decimal scale needs a fixture (see 6).
11. **(implementer) Nesting limit:** the game's JSON reader has no explicit depth limit; the implementation reports a
    parse error beyond 1024 nested levels instead of overflowing its own stack (a crash, not replicated).
12. **(implementer) `null` as a section value:** `pack.mcmeta` sections are read when the **key is present**, so
    `"features": null`, `"overlays": null` or `"pack": null` is treated as a present, invalid section (pack dropped for
    `pack`/`features`/`overlays`; ignored for `filter`) rather than as an absent key. The spec only states this for
    record fields. Verify with an oracle fixture.
13. **(implementer) Text-component validation depth:** §3.3 describes only the broad shapes. The implementation
    accepts strings, non-empty arrays and objects with a content key (`text`, `translate`, `score`, `selector`,
    `keybind`, `nbt`, `object`, or an explicit `type`), checks `extra`, `color` (the 16 names or `#RRGGBB`), the five
    style booleans, `font` and `insertion`, and does not validate click/hover events or `shadow_color`. A fixture
    corpus for descriptions would pin the remaining rules.
14. **(implementer) `allowed_symlinks.txt` format and relative targets:** the spec only says the list defaults to
    "nothing allowed". The implementation reads one rule per line (`#` comments), each optionally typed `[prefix]`
    (default), `[regex]` or `[glob]`, and matches it against the **canonical target** of the symlink. For allowed
    symlinked entries it continues with the canonical target (the working behavior), not with a possibly
    CWD-relative path (Open question 2).
15. **(implementer) Overlay directory `.` for zip packs:** `.` resolves to the pack root for folder packs. For zip packs
    the overlay prefix is the literal entry-name prefix `./`, which matches nothing in a normal archive, so a `.`
    overlay contributes nothing there. Confirm with an oracle fixture whether the game normalizes it.
16. **(implementer) `initial-disabled-packs` default:** it is "empty", and splitting an empty value gives `[""]` like
    `initial-enabled-packs`. The empty id never matches a pack, so this is not observable in the stored lists, but the
    exact in-memory value differs from `[]`.
17. **(implementer) Path-segment checks only on direct lookups:** folder packs validate path segments on direct
    lookups only. Listing directories are checked for `.`/`..`/empty segments by the implementation (a listing with
    `..` would escape the pack folder); the spec says nothing about this because the game's callers never pass such
    directories.

## Reader review (2026-11-25)
Facts in this spec were checked by the reader with local experiments that ran the game's own bundled JSON and codec
libraries and the game's pack-metadata codecs on test inputs. The experiment sources stayed in a local scratchpad and
are not in the repo. Engineering decisions for the open questions:
- **Filter regexes:** Java regex syntax with unanchored search. Zenith uses `fancy-regex` (backreferences and
  lookaround like Java) and covers the remaining syntax differences in tests.
- **Case-insensitive file systems:** Zenith uses the OS's own file lookups, exactly as vanilla does, so behavior
  matches per platform.
- **Overlay `..` escape:** owner decision 11.1: **not replicated.** The overlay is refused with a warning (security
  deviation #1 in `docs/security-deviations.md`). Everything else about overlays is replicated exactly.

## Parity status (2026-11-25): `datapack-load`, stage 1 (JSON layer)
**15,964/15,964**: all 15,698 JSON files in the 26.3 jar parse to identical trees (number text, key order, duplicate
keys), and so do the synthetic edge cases and every primitive number read through the game's bundled codecs. Facts
settled by the oracle on the way:
- **Nesting:** opening a 256th nested container fails; 255 is accepted. Only containers count.
- **Exponents:** integer reads reject `|exponent| ≥ 10000`.
- **No coercion:** `true`, `false` and `"5"` are not numbers.
- **`pack.mcmeta` and `.mcmeta` sidecars** are read with a parser that requires a JSON *object* root and ignores
  trailing content. This is not the game's separate lenient parser, which accepts comments, `NaN`, leading zeros and
  trailing commas.
- **Unpaired surrogates:** a JSON string escape such as `"\ud800"` keeps the raw UTF-16 unit, in values and in
  object keys. Zenith's JSON strings are lossless (`JsonString`, UTF-8 until a lone surrogate appears) and
  re-encode such units as `\uXXXX` escapes. This closed the last 2 cases.
