# 11: Roadmap

Order (owner decision 9.1): **headless core first**. Task checklists live in
[`progress/milestones.toml`](../../progress/milestones.toml) (the tracker's source of truth). This file holds goals,
dependencies and acceptance criteria. Weights are rough relative effort (1 ≈ a small milestone).

Cadence (owner decision 6.4): **fully autonomous**. Work milestones in order, update the tracker after each, and stop
only for owner-only decisions (marked ⛔).

| ID | Milestone | Depends on | Weight | Acceptance |
|----|-----------|-----------|--------|------------|
| **P0** | **Foundation** | | | |
| M0 | Foundation & tooling | — | 2 | Plan, ADRs, guard, local proof passing on the owner's install, tracker pipeline, website live, public repo + CI green |
| **P1** | **Core data** | | | |
| M1 | Parity harness & data-gen ingestion | M0 | 4 | Ingestion produces tables for 26.3 from the owner's jar; oracle agent can dump one tick of a server; `datagen-ingest` 100% |
| M2 | NBT & SNBT | M0 | 2 | `nbt-roundtrip` + `snbt-format` 100%; fuzzers run 1h clean |
| M3 | Saves | M2 | 3 | `anvil-io` 100%; vanilla opens Zenith-written worlds |
| M4 | Registries, identifiers, tags | M1 | 3 | `registry-ids`, `blockstate-ids` 100% |
| M5 | Codecs, data packs, text, components | M4 | 5 | `datapack-load`, `text-component` 100% |
| **P2** | **World simulation** | | | |
| M6 | Math, RNG, noise | M0 | 2 | `rng-streams`, `noise-samples` 100% |
| M7 | Terrain generation | M5, M6 | 8 | `golden-terrain` 100% over 50 seeds in every dimension and world type (done 2026-11-25: 14,798 chunks, 147,980 checks; structures off — structure terrain adaptation is verified in M8) |
| M8 | Features & structures | M7 | 7 | `golden-features`, `golden-structures` 100% |
| M9 | Lighting | M7 | 3 | `light-engine` 100% |
| M10 | Server tick, updates, redstone, fluids | M8, M9, M3 | 8 | `tick-oracle`, `redstone-contraptions` 100% |
| M11 | Entities: physics & core | M10 | 6 | `entity-physics` 100% |
| M12 | Mobs: AI, pathfinding, spawning | M11 | 8 | `mob-ai-oracle`, `spawning` 100% |
| M13 | Items, inventories, crafting, menus | M11 | 6 | `menus`, `recipes` 100% |
| M14 | Commands, functions, chat | M10 | 4 | `command-tree`, `functions` 100% |
| M15 | Progression & rules | M13, M14 | 4 | `loot`, `advancements` 100% |
| **P3** | **Networking** | | | |
| M16 | Protocol 777 & dedicated server | M10–M15 | 6 | `packet-codec`, `packet-replay` 100%; a vanilla client plays on the Zenith server |
| **P4** | **Client** | | | |
| M17 | Window, input, Vulkan 1.3 | M0 | 4 | A triangle → a frame graph running on Linux/Windows/macOS; shaderc pipeline compiles every vanilla core shader |
| M18 | Resources | M17, M5 | 4 | `resource-load` 100% |
| M19 | World & entity rendering | M18, M16 | 10 | `screenshot-diff` 100% |
| M20 | GUI, HUD, text | M18 | 5 | `screenshot-diff-gui` 100% |
| M21 | Audio | M18 | 3 | Sound event parity checks (positions, pitch, volume curves) |
| M22 | Client logic & singleplayer | M19, M20, M21 | 5 | `input-replay` 100%; full survival session playable |
| **P5** | **Platform** | | | |
| M23 | Microsoft sign-in | M0 | 2 | ⛔ The owner registers an Azure app and gets Mojang's approval; then: sign-in, entitlement, keyring, online-mode join |
| M24 | WASM mod runtime & SDK | M22 | 6 | Example mods (block, item, command, HUD) work, with hot reload |
| M25 | Mod index, signing, modpacks | M24 | 3 | Index live; signed install; `.zpack` round-trip; `.mrpack` import |
| M26 | Launcher | M25, M23 | 5 | All features in 07 |
| M27 | In-game mod manager | M24, M20 | 2 | Vanilla-style screens work end to end |
| **P6** | **Speed & release** | | | |
| M28 | Performance program | M22 | 4 | Targets in 05 met or documented |
| M29 | 1.0 for 26.3 | all above | 2 | Every suite 100%; signed, reproducible builds for all 4 platforms |
| **P7** | **More versions** | | | |
| M30 | Next release port (26.4) | M29 | 3 | All suites 100% on 26.4 too |
| M31 | Older versions + data fixers | M30 | 6 | ⛔ The owner picks the first older target |
| R1 | Research: JVM bridge | M24 | 1 | A report + prototype; ⛔ the owner decides go/no-go |

## Parallel tracks

Some milestones are independent and can run in parallel (including via delegates): M2/M6 alongside M1; M17 alongside
P1/P2 (the renderer bring-up doesn't need simulation); website/launcher design-system polish at any time.

## Definition of done (every milestone)
1. Specs written in `docs/specs/<area>/` (clean-room rule).
2. Code + unit tests + its parity suites at 100% (where linked).
3. `cargo clippy` and `cargo xtask guard` are clean; docs updated.
4. `progress/milestones.toml` updated → `cargo xtask progress` → committed.
5. A short entry in `CHANGELOG.md` via conventional commits.
