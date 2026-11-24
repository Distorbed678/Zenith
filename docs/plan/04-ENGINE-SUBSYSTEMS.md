# 04: Engine subsystems

Each subsystem: **scope → approach → parity → performance notes**. All behavior is implemented from specs we write in
our own words (`docs/specs/<area>/*.md`), using public format documentation, data-generator reports, the jar's data
files at runtime, and black-box observation via the oracle. See [10-LEGAL-AND-HYGIENE.md](10-LEGAL-AND-HYGIENE.md).

---

## Core data (P1)

### NBT & SNBT: `zenith-nbt` (M2)
- **Scope:** the binary tag format (all tag types), file variant (named root, gzip/zlib/uncompressed), network variant
  (nameless root), Java modified UTF-8, depth limit and accounting limits that match vanilla's, and SNBT parse/print
  matching vanilla's formatting (quoting rules, number suffixes, list/array syntax, pretty printing for `/data get`).
- **Approach:** a zero-copy reader (`&[u8]` → borrowed views) for hot paths plus an owned tree type; serde integration
  for typed structs; a streaming writer.
- **Parity:** `nbt-roundtrip`, `snbt-format`, plus fuzz targets.
- **Perf:** SIMD-friendly bulk arrays (`LongArray` for block states), arena allocation for trees.

### Saves: `zenith-storage` (M3)
- **Scope:** Anvil region files (`.mca`, 4 KiB sectors, location/timestamp tables, oversized `.mcc` externals,
  compression ids for gzip/zlib/none/LZ4/custom), `level.dat`, `playerdata/`, `entities/` and `poi/` regions, data
  storage (`data/*.dat`), session lock, and atomic write-then-rename semantics.
- **Approach:** an async I/O layer with a thread-pool backend everywhere and **io_uring** on Linux (same bytes, faster).
  Chunk serialization lives with the world types (M10), but the container format lives here.
- **Parity:** `anvil-io`: vanilla loads Zenith-written worlds and vice versa, verified with the oracle.

### Registries, identifiers & tags: `zenith-registry` (M4)
- **Scope:** identifiers (namespace:path rules), built-in registries (blocks, items, entity types, …), dynamic
  (data-driven) registries (biomes, dimension types, damage types, enchantments, variants, …), the global block-state ID
  table, tags and their resolution (nested tags, `replace`, optional entries), and feature flags.
- **Approach:** frozen registries after bootstrap; dense `u32` IDs; `const`-generic typed handles; tables from
  `zenith-data`.
- **Parity:** `registry-ids`, `blockstate-ids`.

### Codecs, data packs, text & data components: `zenith-codec` (M5)
- **Scope:** a codec framework equivalent in *behavior* to what vanilla needs (decode/encode across JSON and NBT, with
  lenient/strict modes and error messages that match vanilla's where they're user-visible); data-pack discovery (vanilla
  built-in + world `datapacks/` + feature packs), overlays, `pack.mcmeta` with format ranges, filters, and reload
  ordering; every data registry in the jar; text components (all content types, styles, click/hover events, translation
  with args, fallback); and the **item data component** system (typed components, patches, defaults per item).
- **Parity:** `datapack-load`, `text-component`.

---

## World simulation (P2)

### Math, RNG & noise: `zenith-math` (M6)
- Java-compatible RNGs: the 48-bit LCG, Xoroshiro128++, seed mixing (Stafford variant), positional random factories,
  "legacy" vs "xoroshiro" random sources, and markov-chain-free `nextInt(bound)` rejection sampling.
- Java-exact helpers: the sine lookup table, `floor`/`ceil` variants, `lerp`/`clampedLerp`, `smoothstep`, and fast
  inverse sqrt usage where vanilla uses it. String `hashCode` for seeds.
- Noise: improved Perlin noise, simplex noise, blended noise (legacy terrain), normal noise (double-Perlin with
  amplitude lists), and octave/first-octave semantics.
- **Parity:** `rng-streams`, `noise-samples`. **Perf:** SIMD (`std::simd` when stable, otherwise `wide`) batch sampling
  for density functions; results bit-identical, since each lane does the same IEEE ops in the same order.

### Terrain generation: `zenith-worldgen` (M7)
- Density functions (every type in the router, including caching markers `cache_2d`, `flat_cache`, `interpolated`,
  `blend_*`), the noise router per dimension, cell interpolation, aquifers (fluid levels, barriers, lava), ore veins,
  noodle caves, the multi-noise biome source with a climate parameter search tree, surface rules (conditions +
  sequences), and carvers (cave/canyon variants).
- All configuration comes from the jar's worldgen JSON at runtime. Zenith implements the *machinery*.
- **Parity:** `golden-terrain`. **Perf:** density evaluation compiled into a flat instruction tape per noise router
  (JIT-free "bytecode"), evaluated over 4×8×4 cells with SIMD; chunk-parallel with dependency-radius scheduling.

### Features & structures (M8)
- Configured/placed features, placement modifiers, and the 11 decoration steps in vanilla order per biome. Feature RNG
  seeding per (chunk, step, index). Tree/foliage placers, vegetation patches, geodes, fossils, dungeons, etc.
- Structures: structure sets, placement types (random spread, concentric rings), exclusion zones, structure starts,
  jigsaw assembly (pools, processors, terrain adaptation), and templates loaded from the jar's structure NBT at runtime.
- **Parity:** `golden-features`, `golden-structures`.

### Lighting: `zenith-light` (M9)
- Block light and sky light engines with vanilla's queue semantics (increase/decrease passes, section-level skylight
  sources, data layers), and lighting during the chunk pipeline. **Parity:** `light-engine`. **Perf:** parallel per
  region with deterministic merge (same final values; vanilla's are order-independent at the end of a batch, which
  the suite verifies).

### Server tick, block updates, redstone, fluids: `zenith-world` (M10)
- Chunk tickets/levels and the status pipeline, entity sections, the per-tick phase order (world border, weather/time,
  scheduled block ticks, fluid ticks, raids, chunk random ticks, block events, entity ticks, block entity ticks), game
  rules, neighbor updates and shape updates (with the update-order quirks redstone players rely on), redstone
  components (dust, repeaters, comparators, observers, pistons incl. quasi-connectivity and BUD behavior, rails,
  hoppers), and fluids.
- **Parity:** `tick-oracle`, `redstone-contraptions`.

### Entities (M11), mobs (M12)
- Movement and collision (AABB sweep, step-up, block shape caching), drag/gravity per type, riding/vehicles,
  projectiles, item/XP entities, damage + damage types + effects + attributes; entity tracking for networking.
- Mob AI: goal-selector AI, brain-based AI (memories, sensors, activities, behaviors), pathfinding (node evaluators
  per mob type, path following), spawning (natural spawning with mob caps, structure spawns, spawners, trial
  spawners), villagers/trading/gossip, raids, and variants.
- **Parity:** `entity-physics`, `mob-ai-oracle`, `spawning`.

### Items, inventories, crafting, menus (M13)
- Item behavior per data component, containers and menus (every menu type), click types (pickup, quick move, swap,
  clone, throw, quick craft/drag, pickup-all), recipe matching (shaped/shapeless/transmute/special), smelting,
  smithing, stonecutting, brewing, enchanting tables, anvils and grindstones. **Parity:** `menus`, `recipes`.

### Commands, functions, chat (M14)
- A command graph with Brigadier-compatible behavior (parsing, redirects, forks, suggestions, error messages), every
  vanilla command, entity selectors, argument types, `mcfunction` execution (the command queue model, macros, `return`,
  `execute` forking), and chat (chat types, decoration, signing when signed in). **Parity:** `command-tree`, `functions`.

### Progression & rules (M15)
- Loot tables, predicates, item modifiers, advancements and criteria triggers, statistics, the scoreboard
  (objectives, criteria, display slots, teams), boss bars, and the recipe book. **Parity:** `loot`, `advancements`.

---

## Networking (P3)

### Protocol 777: `zenith-protocol` + `zenith-server` (M16)
- Framing (VarInt length), compression threshold, AES/CFB8 encryption, every connection state (handshake, status,
  login, configuration, play), registry sync and known packs, chunk and light serialization, entity metadata, bundle
  packets, and keep-alive/ping semantics.
- Packet IDs come from ingestion. Packet codecs are written once per shape. A **packet record/replay** corpus is built
  from the oracle (suggestion 7.1).
- `zenith-server`: a dedicated server binary with vanilla's `server.properties`, whitelist/ops/bans JSON formats,
  RCON and query.
- **Parity:** `packet-codec`, `packet-replay`, plus a vanilla client connecting to the Zenith server.

---

## Client (P4)

### Platform & GPU (M17)
- **SDL3** for windowing/input (vanilla 26.3 also uses SDL), including gamepad and IME. **ash** Vulkan 1.3 (owner
  decisions 3.2, 9.2): dynamic rendering, synchronization2, descriptor indexing, VMA-style allocation (`gpu-allocator`),
  timeline semaphores, and a frame graph with automatic barriers.
- Shaders: vanilla's GLSL core shaders are read from the jar (or from resource packs, which can override them) and
  compiled with **shaderc** to SPIR-V at runtime, with the same preprocessing (`#moj_import`, defines) and uniform
  layout. Compiled SPIR-V is cached per pack-stack hash.
- macOS via MoltenVK (Vulkan 1.3 profile on Apple Silicon).

### Resources (M18)
- Resource pack stack with overlays and format ranges; blockstates and models (variants, multipart, element rotations,
  UV lock), atlases (sources, stitching algorithm and layout identical, because UVs feed pixel parity), fonts (bitmap,
  TTF via FreeType-equivalent rasterization (open question Q5), unihex), lang, sounds.json, splashes, and texts.
- All of it comes from the user's jar and asset objects (owner rule). **Parity:** `resource-load`.

### World & entity rendering (M19)
- Chunk meshing with vanilla's face culling, ambient occlusion and smooth lighting math, biome blending (radius from
  options), render types (solid/cutout/translucent with sorting), block entities, entity models (geometry is defined in
  code in vanilla; Zenith reimplements model *definitions* from specs), the sky/sun/moon/stars/clouds/weather/fog
  pipeline, particles, item rendering, and post effects (from the jar's post-effect JSON).
- **GPU-driven** internals (suggestion 7.3): indirect multi-draw per section, compute frustum/occlusion culling,
  persistent mapped buffers, and bindless textures. The output must stay pixel-exact. **Parity:** `screenshot-diff`.

### GUI, HUD & text (M20)
- Widget toolkit with vanilla layout behavior, every screen (title, options, world select/create, multiplayer,
  inventories, advancements, statistics, accessibility, etc.), the HUD (hotbar, hearts, hunger, XP, boss bars, chat,
  subtitles, debug screen), toasts, tooltips, and text rendering (shadow, bidi, glyph providers, obfuscated text).
  GUI sprites come from the user's assets. **Parity:** `screenshot-diff-gui`.
- The **in-game mod manager** (M27) is built from these widgets, so it looks like vanilla UI.

### Audio (M21)
- 3D positional audio that matches vanilla's attenuation, pitch and volume behavior and channel limits (vanilla uses an
  OpenAL implementation; Zenith uses a Rust mixer on cpal plus its own HRTF-free panning model that matches vanilla's
  output). Ogg Vorbis decoding (lewton or a pure-Rust decoder), sound events/variants, music manager, and subtitles.

### Client logic & singleplayer (M22)
- Integrated server, the singleplayer flow, client-side prediction and interpolation, `options.txt` compatibility, and
  **deterministic input record/replay** (suggestion 7.2), which is also used for bug reports and benchmarks.
  **Parity:** `input-replay`.
