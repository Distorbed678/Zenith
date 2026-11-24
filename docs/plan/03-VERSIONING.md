# 03: Versioning: future and older versions

Owner decision 2.4: **26.x first, designed so that any version can be added.** The next targets are picked after 26.3
reaches parity.

## What changes between versions

| Kind of change | Example | Where Zenith absorbs it |
|----------------|---------|-------------------------|
| Pure data | new recipes, loot, biomes, worldgen JSON, tags | Read from the user's jar at runtime; no code change |
| Registry contents | new blocks/items/entities, new state IDs | Data-gen ingestion → `zenith-data` tables (automatic) |
| Packet IDs / ordering | IDs shuffle every release | Ingested `packets` report (automatic) |
| Packet shapes | a field is added to a packet | Per-version codec module in `zenith-protocol::v<proto>` |
| Codec / data format | a JSON field is renamed | `zenith-codec` schema versions, plus data fixers for saves |
| Behavior | a mob's AI changes, a redstone fix | `zenith-version` feature gates + per-version behavior modules |
| Rendering | shader/uniform layout change, new effect | `zenith-render` versioned pipeline descriptors (shaders themselves come from the jar) |
| Save format | new chunk fields | `zenith-storage` + data fixers |

## Data-gen ingestion (suggestion accepted, 7.2)

1. `zenith-install` finds the jar, plus a Java runtime from the launcher (Prism's `java/java-runtime-*`, or the official
   launcher's `runtime/` directory). It checks that the runtime's major version matches the jar's `java_version`.
2. Zenith runs the game's built-in data generator from the user's own jar, into a temp directory:
   reports (blocks with properties and state IDs, registries, packets, commands, items, data components, biome
   parameters) and the generated data/resource trees.
3. `zenith-data` compiles the outputs into `tables.bin`: a compact, versioned, mmap-able format with a header that
   records the jar SHA-1, the Zenith table schema version and the build timestamp. It is invalidated automatically when
   the jar hash changes.
4. Nothing produced here is ever committed or uploaded.

## Version layer: `zenith-version`

- `VersionId { name: "26.3", protocol: 777, world: 5023, data_pack: (121,0), resource_pack: (97,1) }`.
- `Features`: a set of boolean/enum gates (e.g. `ItemComponents`, `ConfigurationPhase`, `NewRenderer2026`) derived from
  the version. Code branches on features, never on version numbers directly. This keeps the code readable and
  lets versions share code paths.
- Larger divergences get versioned modules (`entity::ai::villager::v26_3`), selected by enum dispatch at world load.
  Hot paths are monomorphized per version family to avoid dynamic dispatch in inner loops.
- One running process serves one game version. Multi-version *servers* (a la ViaVersion) are a non-goal for v1.

## The version diff bot (suggestion accepted, 7.2)

A scheduled GitHub Action (`version-watch.yml`) runs every 6 hours:

1. Poll Mojang's version manifest. On a new release or snapshot:
2. Download the jar and asset index (inside CI only; never stored in artifacts or the repo).
3. Run ingestion for the old and new versions, plus a **signature diff** of the class/method/field signature lists
   (from javap: names and types only).
4. Produce a structured diff: registries added/removed/changed, packet ID moves, packet shape changes (from the
   `packets` report and the signature diff), data-pack format changes, shader file changes (path + hash only),
   class/method churn per package.
5. Open **one GitHub issue per change**, labeled by subsystem (`area:worldgen`, `area:protocol`, …), linked to a
   tracking issue `Port <version>`. Each issue holds only facts (names, IDs, hashes); never code or assets.
6. Append the new version's facts (client SHA-1, asset index ID/SHA-1, world/protocol numbers) to
   `crates/zenith-install/data/known_versions.toml` through a PR.

## Older versions (M31)

- Choose a first older target with the owner, once 26.3 reaches 1.0 (open question Q3 in 13).
- Requirements before any older version: **data fixers**. These upgrade older saves to newer formats (vanilla's fixer
  chain covers thousands of schema steps). Zenith implements its own fixer framework (`zenith-storage::fixers`),
  verified by upgrading oracle-generated old-version worlds and diffing them against vanilla's upgrade of the same worlds.
- Big eras and what changes: the **Flattening** (1.13: numeric IDs → namespaced states), the **component era**
  (1.20.5: NBT item tags → data components), **configuration phase** (1.20.2), the 1.18 height change, and the 1.9
  combat change. Each era becomes a feature family in `zenith-version`.

## Release cadence handling

- A Mojang release triggers the bot → issues → the version port milestone (like M30 for 26.4).
- The Zenith release for a new game version ships only when every parity suite is green for that version.
- Old Zenith versions keep working with their game version. The launcher installs the Zenith build that matches each
  instance's game version.
