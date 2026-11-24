# Zenith master plan: overview

> Status: living document. Owner decisions are logged in [`../decisions/USER-ANSWERS.md`](../decisions/USER-ANSWERS.md).
> If this plan and that log ever disagree, the log wins, and this plan gets fixed.

## What Zenith is

Zenith is an **independent game engine written in Rust** that runs Minecraft: Java Edition **1:1**: same world
generation, same simulation (including gameplay bugs), same network protocol, same save files, same pixels. It uses the
player's **own, legitimately owned copy** of the matching version, which must have been **launched at least once**.
Zenith ships no game code, assets or data. Everything the game needs is read at runtime from the player's install.

On top of the engine, Zenith provides:

- a **launcher** (Tauri 2 + Svelte 5) in an original pixel-crafted style, with animations and animated backgrounds;
- a **modding platform**: sandboxed **WASM** mods, a mod SDK and CLI, a signed GitHub-based mod index, modpacks
  (`.zpack`, plus `.mrpack` import), and an in-game mod manager that looks like the vanilla UI;
- a **website** on GitHub Pages in the same style as the launcher, with a live progress tracker, docs, downloads and a
  mod index browser;
- tooling that proves the 1:1 claim: a **parity oracle** that runs the real game and Zenith side by side and diffs them.

The first target is **26.3** (protocol 777, world version 5023). The architecture is built so that later releases (26.4+)
and older versions can be added without rewrites.

## Principles (in priority order)

1. **Legal hygiene first.** No game code, assets or data in the repo, ever. Clean-room reimplementation from behavior
   specs. Trademark-safe naming. See [10-LEGAL-AND-HYGIENE.md](10-LEGAL-AND-HYGIENE.md).
2. **Proven 1:1.** "Same as vanilla" is a test result, not a claim. Every subsystem has a parity suite run against the
   real game. A subsystem is done only at 100% parity. See [02-PARITY.md](02-PARITY.md).
3. **Pixel-exact and gameplay-exact.** Internals may differ for speed, but must never change gameplay or visuals. The
   base game has **no** non-vanilla options; extras come from mods. Bug parity covers every observable behavior,
   except crashes, memory leaks and pure-performance bugs.
4. **As fast as possible, within rule 3.** Multithreading, GPU-driven rendering, PGO/LTO, and data-oriented design, all
   gated by the parity suites. See [05-PERFORMANCE.md](05-PERFORMANCE.md).
5. **Version-agnostic core.** Version-specific behavior lives behind a version layer that is fed by data-generator
   ingestion and the version diff bot. See [03-VERSIONING.md](03-VERSIONING.md).
6. **Safe extensibility.** Mods are sandboxed and permissioned; downloads are signed and hash-pinned.

## Scope at a glance

| Area | In scope (v1 for 26.3) | Later |
|------|------------------------|-------|
| Engine | Client + integrated server + dedicated server, all of vanilla 26.3 | 26.4+, older versions, OpenGL backend |
| Parity | Oracle agent, golden worldgen, packet replay, screenshot diff, input replay, fuzzing | Continuous nightly corpora growth |
| Ownership | Local proof (jar hash, full asset set, launch evidence) + Microsoft sign-in | — |
| Network | Protocol 777: LAN, offline-mode, and (after MS sign-in) online-mode servers, plus the Zenith server | Realms |
| Launcher | Instances, imports (Prism/MultiMC/official/Modrinth App/CF App), mods, modpacks, snapshots, safe mode, skin viewer, signed updates, localization, controller UI | — |
| Modding | WASM runtime, SDK/CLI/hot reload, permissions, signed index, PubGrub lockfiles, `.zpack`, `.mrpack` import, server→client sync, in-game manager | JVM bridge for Java mods (research R1) |
| Platforms | Linux x86_64, Windows x86_64, macOS Apple Silicon, Linux ARM64 | — |
| Website | Tracker, docs + API, downloads + changelog, mod index browser | — |

Explicitly **out of scope**: gameplay changes of any kind in the base game; built-in "enhancements"; Modrinth or
CurseForge as in-app browse sources (owner decision 4.2; their *profiles* can still be imported, see 07).

## Plan map

| File | Contents |
|------|----------|
| [01-ARCHITECTURE.md](01-ARCHITECTURE.md) | Crates, layering, threading model, data flow, version layer |
| [02-PARITY.md](02-PARITY.md) | What 1:1 means, the oracle, every parity suite, scoring |
| [03-VERSIONING.md](03-VERSIONING.md) | Multi-version design, data-gen ingestion, version diff bot, data fixers |
| [04-ENGINE-SUBSYSTEMS.md](04-ENGINE-SUBSYSTEMS.md) | Every engine subsystem: scope, approach, parity strategy |
| [05-PERFORMANCE.md](05-PERFORMANCE.md) | Performance program and its 1:1 guardrails |
| [06-AUTH-OWNERSHIP.md](06-AUTH-OWNERSHIP.md) | Local proof + Microsoft sign-in, Azure/Mojang approval steps |
| [07-LAUNCHER.md](07-LAUNCHER.md) | Launcher features, UX, design system, imports, updates |
| [08-MODDING.md](08-MODDING.md) | WASM API, SDK, permissions, index, modpacks, in-game manager, JVM bridge research |
| [09-WEBSITE-AND-TRACKER.md](09-WEBSITE-AND-TRACKER.md) | Website, docs pipeline, tracker data model and visuals |
| [10-LEGAL-AND-HYGIENE.md](10-LEGAL-AND-HYGIENE.md) | Clean-room process, the guard, trademark rules |
| [11-ROADMAP.md](11-ROADMAP.md) | Milestones M0–M31 + R1 with goals, dependencies, acceptance criteria |
| [12-INFRA-CI-RELEASE.md](12-INFRA-CI-RELEASE.md) | CI, builds, packaging, releases, nightly, reproducibility |
| [13-RISKS-AND-OPEN-QUESTIONS.md](13-RISKS-AND-OPEN-QUESTIONS.md) | Risks, mitigations, open questions for the owner |

## Facts about the target (verified 2026-11-24 on the owner's machine)

- Client jar SHA-1 `e877b6a07acd633fb3bb475002175cec036e7b87` (matches Mojang's manifest). Asset index `34`.
- `version.json`: world version 5023, protocol 777, resource pack format 97.1, data pack format 121.0, Java 25.
- The jar is **not obfuscated** (readable class names). It has 11,383 classes and contains the data generator
  (`net.minecraft.data.Main`) and the game-test runner.
- Rendering: a new render abstraction with **OpenGL and Vulkan backends**, SDL windowing, and GLSL compiled with shaderc
  to SPIR-V (SPIRV-Cross for GL). 64 core shaders + 18 includes + 10 post shaders live in the jar's assets.
- Libraries the game uses (behavior we must match, never code we copy): Netty (networking), Brigadier (commands),
  DataFixerUpper (codecs/fixers), LWJGL 3.4.3 (SDL, Vulkan, OpenGL, OpenAL, stb, FreeType, shaderc), JOML, fastutil,
  Guava, Gson, ICU4J, LZ4, jorbis (Ogg Vorbis), Tracy.

## How the work proceeds

The owner chose **fully autonomous** cadence: milestone after milestone in the order of [11-ROADMAP.md](11-ROADMAP.md)
(headless core first), stopping only for decisions that belong to the owner. After every milestone:
`progress/milestones.toml` is updated → `cargo xtask progress` → the README and website trackers update.
