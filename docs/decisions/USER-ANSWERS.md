# User answers log

This is the permanent record of every question asked and every answer the owner gave.
**Rule:** re-read this file before asking anything, and append to it after every question round.
Answers are quoted or paraphrased faithfully. Where an answer changed an earlier one, the later answer wins and the change is noted.

---

## Round 1 — 2026-11-24 — Identity & access

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 1.1 | Project name | Verdigris, Ferrocube, Hematite, Patina | **None.** "These are bad names. Keep it simple. A name like horizon is good but I don't want that name because it's taken." |
| 1.2 | Ownership check method | MS sign-in + local; MS only; local proof only; launcher account + local | "Azure needs me to pay so I don't want that, so I guess just the local proof." → **superseded in 2.3** |
| 1.3 | GitHub account for repo + Pages | Distorbed678, WaystoneWorks | **Distorbed678** |
| 1.4 | License | MIT OR Apache-2.0; GPL-3.0+; MPL-2.0; AGPL-3.0 | **MIT OR Apache-2.0** |

## Round 2 — 2026-11-24 — Name, proof checks, multiplayer, version range

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 2.1 | Name (simple, Horizon-like) | Meridian, Daybreak, Summit, Cairn | **Zenith** (typed by the owner) |
| 2.2 | Which local proof checks are required | Official jar hash; full asset set verified; launch evidence; launcher account present | **Official jar hash + full asset set verified + launch evidence.** (Launcher-account check *not* chosen.) |
| 2.3 | Multiplayer without MS sign-in | Offline/LAN + pluggable auth; same + MS milestone; singleplayer only | "If Azure is actually free then disregard what I said and **include MS sign-in** so multiplayer and verification that you own the game can work." |
| 2.4 | How far back the multi-version architecture should go | 26.x first, design for any; 1.20.5+; 1.13+; everything | **26.x first, design for any version** |

### What these answers settle
- **Ownership = Microsoft sign-in (entitlement check) AND local proof** (jar SHA-1 vs Mojang manifest, full asset set hash-verified, launch evidence).
  - Azure app registration costs nothing. Azure may ask for a card to verify identity, and Mojang must approve the app ID (form at aka.ms/mce-reviewappid) before the Minecraft-services step works.
  - Until approval arrives, Zenith runs in local-proof-only mode (singleplayer/LAN/offline-mode servers). Auth is pluggable, so turning on MS sign-in later needs no rewrite.
- Repo: `github.com/Distorbed678/Zenith`, Pages: `distorbed678.github.io/Zenith`.
- Note: a Minecraft *mod* named "Zenith" already exists (an Apotheosis port). The owner chose the name anyway. Product copy should say "Zenith — an independent game engine", never "Zenith for Minecraft". <!-- guard:allow -->

## Round 3 — 2026-11-24 — Core tech stack

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 3.1 | Launcher tech | Tauri 2 + Svelte 5; Dioxus; native GPU UI (Slint/iced); engine's own UI | **Tauri 2 + Svelte 5** |
| 3.2 | Game renderer graphics stack | Vulkan via ash; wgpu; Vulkan + OpenGL day 1 | **Vulkan via ash** (OpenGL backend may come later) |
| 3.3 | Mod runtime | WASM; native Rust dylibs; WASM + native trusted tier; Luau/Lua | **WASM mods** (wasmtime) |
| 3.4 | Run existing Java mods (Fabric/NeoForge)? | No, native only; research a JVM bridge later | **Research a JVM bridge later** (roadmap research item, not a commitment) |

## Round 4 — 2026-11-24 — Launcher look, mod sources, platforms, launcher scope

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 4.1 | Launcher + website visual direction | Frosted glass + live scenes; celestial; clean dark + glow; original pixel-crafted | **Original pixel-crafted** (all-new pixel-art frames, buttons, bitmap-style font, animated pixel scenery) |
| 4.2 | Mod/content sources | Modrinth; CurseForge; Zenith index (GitHub-based); local files/URLs | **Zenith index (GitHub-based) + local files/URLs.** Modrinth and CurseForge *not* chosen as browse sources. |
| 4.3 | Platforms | Linux x86_64; Windows x86_64; macOS Apple Silicon; Linux ARM64 | **All four** |
| 4.4 | Launcher scope / imports | Also launch real vanilla Java; import Prism/MultiMC; import official launcher; import Modrinth/CF app profiles | **Import from Prism/MultiMC + official launcher + Modrinth/CF app profiles.** Launching real vanilla Java *not* chosen. |

## Round 5 — 2026-11-24 — Fidelity rules, modpacks, website stack

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 5.1 | How strict "1:1" is on screen | Pixel-exact, internals free; perceptually identical; pixel-exact + opt-in extras | Typed: "**Pixel-exact, gameplay-exact. Internals can be different but it can't change the gameplay or visuals. Extras should come from mods.**" → the base game has NO built-in non-vanilla visual or gameplay options. Anything extra is a mod. |
| 5.2 | Bug-for-bug parity | All observable minus crashes; absolutely everything; gameplay-affecting only | **All observable behavior incl. gameplay bugs, minus crashes / memory leaks / pure-perf bugs** |
| 5.3 | Modpack format | Own .zpack + .mrpack import; .zpack only; .mrpack native | **Own .zpack + .mrpack import** |
| 5.4 | Website stack | SvelteKit static; Astro + Svelte islands; plain HTML | **SvelteKit static**, sharing `packages/ui` with the launcher |

## Round 6 — 2026-11-24 — Tracker, docs, telemetry, work cadence

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 6.1 | Tracker data source | Parity tests + milestones; manual progress.toml | **Parity tests + milestones** (CI-generated progress.json → README SVGs + website dashboard) |
| 6.2 | Docs location | In repo + on website; mdBook; repo markdown only | **In repo (`docs/`) + rendered on the website (mdsvex) + rustdoc under /api** |
| 6.3 | Crash reports / telemetry | Opt-in local crash reports; opt-in upload; nothing | **Opt-in local crash reports** (symbolicated report saved locally + pre-filled GitHub issue offer; no telemetry ever) |
| 6.4 | Working cadence after setup | Milestone runs + check-ins; fully autonomous; small steps | **Fully autonomous.** Keep going milestone after milestone; only stop when truly blocked on a decision that belongs to the owner. |

## Round 7 — 2026-11-24 — Suggestions A (multi-select; everything listed was offered)

| # | Group | Offered | Accepted |
|---|-------|---------|----------|
| 7.1 | Parity & verification | Parity oracle (Java agent); golden worldgen diff; packet record/replay; screenshot diff suite | **All four** |
| 7.2 | Versioning | Version diff bot; data-gen ingestion; deterministic input replay; fuzzing | **All four** |
| 7.3 | Performance | Tracy + perf CI dashboard; PGO + LTO + CPU-tier builds; deterministic parallel pipeline; GPU-driven rendering | **All four** |
| 7.4 | Launcher extras | Signed auto-updates; crash safe-mode; 3D skin/cape viewer; instance snapshots | **All four** |

## Round 8 — 2026-11-24 — Suggestions B

| # | Group | Offered | Accepted |
|---|-------|---------|----------|
| 8.1 | Modding platform | Mod SDK + CLI + hot reload; capability permissions; lockfiles + PubGrub solver; server→client mod sync | **All four** |
| 8.2 | Repo & process | Asset/trademark guard (CI + hook); ADRs; conventional commits + release-please; nightly builds | **All four** |
| 8.3 | Accessibility & UX | Reduced motion + contrast; screen-reader launcher; launcher localization; controller/Steam Deck UI | **Reduced motion + contrast, launcher localization (Fluent), controller/Steam Deck UI.** Screen-reader launcher *not* chosen (still use semantic HTML as baseline good practice). |
| 8.4 | Distribution & security | Signed mod index; reproducible builds; keyring token storage; cargo-deny/audit in CI | **Signed mod index, reproducible builds, keyring token storage.** cargo-deny/audit *not* chosen. |

## Round 9 — 2026-11-24 — Build order, GPU baseline, brand, site sections

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 9.1 | Milestone order | Headless core first; see it early; launcher + site first | **Headless core first** (NBT → registries/data-gen → worldgen → server tick → protocol → client render → launcher) |
| 9.2 | Minimum GPU feature level | Vulkan 1.3; 1.2; 1.1 + fallbacks | **Vulkan 1.3** |
| 9.3 | Logo / brand | Pixel sun at zenith; pixel Z monogram; wordmark only; owner provides | **Pixel sun at zenith** (original pixel sun atop an arc over a stepped voxel horizon) |
| 9.4 | Website sections | Live progress tracker; docs + API; downloads + changelog; mod index browser | **All four** |

---

## Verified facts gathered during the session (2026-11-24)
- Install: Prism Launcher (Flatpak) at `~/.var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/`.
- 26.3 client jar: `libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar`, SHA-1 `e877b6a07acd633fb3bb475002175cec036e7b87` — **matches Mojang's piston-meta manifest**.
- `version.json` in jar: world_version 5023, protocol 777, resource pack 97.1, data pack 121.0, Java 25 (`java-runtime-epsilon`), build 2026-09-15.
- Jar is **not obfuscated** (readable `net.minecraft.*` names). 11,383 classes; contains `net/minecraft/data/Main` (data generator) and `net/minecraft/gametest/Main`.
- Rendering: new `com.mojang.renderpearl` layer with **OpenGL and Vulkan backends**; SDL windowing (lwjgl-sdl); shaderc + SPIRV-Cross; 64 core shaders. The owner's run used OpenGL 3.3 on an RTX 4080.
- Asset index `34` (~488 MB of objects) present; instance `26.3` has been launched (`logs/latest.log`, lastLaunchTime set).
- Mojang manifest on 2026-11-24: latest release 26.3, latest snapshot 26.4-pre-1.
- Dev machine: CachyOS, RTX 4080 (Vulkan 1.4), 16 threads, 62 GB RAM, Rust 1.99, system Java 21 (Prism ships Java 25), Node 22, pnpm.

## Standing instructions given mid-session (2026-11-24)

| # | Instruction | Where it's enforced |
|---|-------------|---------------------|
| S.1 | "Use Anthropic's frontend design skill for the UI." The skill is installed at `~/.claude/plugins/marketplaces/claude-plugins-official/plugins/frontend-design/skills/frontend-design/SKILL.md`. Applies to the launcher + website. The in-game UI stays vanilla-styled per the original request. | CLAUDE.md rule 6; every UI delegate brief |
| S.2 | Use the question tool when told to ask questions or give suggestions; log answers here. | CLAUDE.md rule 1 |
| S.3 | Subagents OK with the right model; Gemini subagents only **Gemini 4 Neon** (≈ Opus 5.5) to save Claude usage. | CLAUDE.md rule 2 + memory |
| S.4 | No gameplay suggestions. | CLAUDE.md rule 1 |
| S.5 | No Mojang/Minecraft assets in the repo; trademark hygiene; requires a legitimate, launched copy of the matching version. | CLAUDE.md hard rules + `cargo xtask guard` |

## Round 10 — 2026-11-24 — Follow-ups after M0

| # | Question | Options offered | Answer |
|---|----------|-----------------|--------|
| 10.1 | (Owner asked) Can Claude still read every line of the original game to catch very specific edge cases (e.g. elytra landing) for true 1:1, without copying? | — | **Yes.** The jar is decompiled locally into `~/.cache/zenith/reference/26.3/` (outside the repo; Vineflower 1.12.0, checksum-verified). It's read to write behavior specs in our own words; implementation comes from specs; the oracle verifies. The owner then said "don't do it now" to the elytra demo, so it was not carried out. |
| 10.2 | Accept the Minecraft EULA for the oracle's headless vanilla server runs? | Yes: local + CI; local only; no | **Yes: local + CI runs.** Zenith writes `eula=true` only in the oracle's temporary server folders, on the owner's machine and in the nightly parity CI job of the owner's repo. |
| 10.3 | "Continue the plan" | — | Proceed with M1 (parity harness + data-gen ingestion), autonomous cadence. |

### Clean-room working split (applied from M1 on)
- **Reader** (main thread): reads local reference code when needed, writes specs in `docs/specs/` in its own words.
- **Implementer** (delegate or separate session): implements only from specs + public docs. It never reads `~/.cache/zenith/reference/`.
