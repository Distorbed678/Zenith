# 08: Modding platform

Owner decisions: **WASM mods** (3.3), Java mods only as a **research item** (3.4), sources = **Zenith index +
local files/URLs** (4.2), modpacks = **`.zpack` + `.mrpack` import** (5.3); all four platform suggestions were accepted
(8.1); signed index (8.4); extras beyond vanilla come from mods (5.1).

## Content types

| Type | Format | Notes |
|------|--------|-------|
| Resource packs | vanilla format | Day one; full overlay/format-range support |
| Data packs | vanilla format | Day one |
| Core-shader packs | vanilla resource-pack shaders | Day one (vanilla GLSL → SPIR-V) |
| **Zenith mods** | `.zmod` (zip: `zenith.mod.toml` + `mod.wasm` + assets) | Sandboxed WASM components |

## Runtime: `zenith-mod-host` (M24)

- **wasmtime** with the **Component Model**. The API is defined in WIT (`wit/zenith.wit`), versioned with semver.
  Guests can be written in Rust (first-class SDK), or any language that targets components (C/C++, Go, Zig,
  AssemblyScript…).
- **Execution model:** mods run inside the tick at defined hook points (events), with fuel/epoch limits per call so a
  broken mod can't freeze the game. Bulk APIs (e.g. "set these 10,000 blocks") minimize boundary crossings.
- **Determinism:** mod RNG is derived from the world seed + mod id. Mods run in a stable order (load order from the
  lockfile), so modded worlds replay deterministically too.
- **API surface (grows over time):** registries (add blocks/items/entities via data + behavior callbacks), events
  (tick, block/entity/player events, commands), world access (read/write blocks and entities), networking (custom
  payload channels), client rendering hooks (custom render layers, HUD elements, screens built from vanilla-style
  widgets), and config.
- **Capability permissions** (suggestion 8.1): `zenith.mod.toml` declares capabilities, e.g.
  `["world.write", "net.custom-payload", "fs.config", "render.hooks", "net.http:api.example.com"]`. The user approves
  them at install. Undeclared capabilities are simply not linked into the component.
- **Hot reload** in dev instances: rebuild → the host swaps the component at a tick boundary, migrating state via an
  optional `migrate` export.

## SDK & CLI: `zenith-sdk` + `zenith mod` (M24)

```
zenith mod new my-mod          # cargo-generate template with WIT bindings
zenith mod build               # cargo component build --release → target/my-mod.zmod
zenith mod run                 # launch a dev instance with hot reload
zenith mod publish             # sign + open a PR to the index repo
```
Docs, a cookbook and example mods are published on the website.

## Zenith mod index (M25)

- A public GitHub repo `Distorbed678/zenith-index` (like crates.io-index): one TOML manifest per mod version
  (`id`, `version`, `authors`, `license`, `capabilities`, `dependencies`, `game_versions`, `download` URL (GitHub
  release asset), `sha256`, `signature`).
- **Signing:** authors sign with minisign (or Sigstore keyless via GitHub OIDC). Index entries pin author keys, and the
  client verifies signature + hash before loading anything.
- CI on the index repo validates manifests, rejects capability escalations without a version bump note, and checks
  that downloads match their hashes.
- The website's `/mods` page browses the index; deep links `zenith://install/<id>` open the launcher.

## Dependencies & lockfiles

- A **PubGrub** solver (`pubgrub` crate) over semver ranges, with `conflicts` and `breaks` declarations and clear
  explanations of why something can't be installed.
- `zenith.lock` pins exact versions + sha256 per instance and per modpack, so installs are reproducible.

## Modpacks

- **`.zpack`**: `zenith.pack.toml` (name, version, game version, mods by id+version, overrides) + `zenith.lock` +
  `overrides/` (configs, resource/data packs) + an optional signature.
- **`.mrpack` import**: read `modrinth.index.json`; resource packs, data packs and shader files carry over (hash-verified
  downloads); Java mods are listed as unsupported (R1 research); overrides are copied.

## Server → client mod sync (suggestion 8.1)

When a client joins a Zenith server that has mods, the configuration phase includes a custom payload listing the
required mods (id, version, hash, capabilities). The client shows a vanilla-style screen: "This server needs 3 mods"
→ review permissions → install into a temporary profile → join. Nothing is installed without the user's consent.

## In-game mod manager (M27)

Built from the vanilla-style widget toolkit (M20), drawn with the GUI sprites from the user's own assets, so it looks
like a native vanilla screen: browse/search the index, install/update/toggle, see permissions, and restart when needed.

## Research R1: JVM bridge for Java mods

A feasibility study only: embed a JVM, load Fabric/NeoForge mods against a shim that maps their API onto Zenith.
Expected to be very hard and never 100% compatible. The deliverable is a written report + prototype, followed by an
owner decision.
