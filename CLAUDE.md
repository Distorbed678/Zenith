# Zenith — agent instructions

Zenith is an independent, open-source game engine written in Rust. It runs Minecraft: Java Edition content
(26.3 first, more versions later) **1:1**, using the player's **own, legitimately owned and already-launched copy**.
The repo also holds a Tauri launcher, a WASM modding platform, and a SvelteKit website on GitHub Pages.

The full plan lives in `docs/plan/` (start at `docs/plan/00-OVERVIEW.md`). Every owner decision is recorded in
`docs/decisions/USER-ANSWERS.md`. Architecture decisions are in `docs/adr/`.

## Owner instructions (verbatim intent, do not drop)

1. **Question tool rule.** When the owner says "ask questions" or "give suggestions", use the **AskUserQuestion tool**
   (multiSelect for suggestion batches), never plain chat. After every round, append the questions, the options and the
   answers to `docs/decisions/USER-ANSWERS.md`. Re-read that file first, so nothing already settled is asked again.
   **No gameplay suggestions, ever.**
2. **Subagents.** Use subagents when they help, with the right model for the task. In T3 Code, Gemini subagents can be
   started with `delegate_task` (provider `antigravity`, model **`gemini-4-neon`**). The owner allows **Gemini 4 Neon only**
   (never another Gemini model). The owner considers it as capable as Opus 5.5, so use it to save Claude usage on large,
   self-contained work. Keep architecture and cross-cutting decisions in the main thread.
3. **Cadence: fully autonomous.** Work milestone after milestone (`docs/plan/11-ROADMAP.md`). Stop only when blocked on a
   decision that belongs to the owner. Update the tracker (`progress/milestones.toml` → `cargo xtask progress`) whenever a
   milestone moves.
4. **Fidelity.** Pixel-exact and gameplay-exact with vanilla. Internals may differ for speed, but must never change
   gameplay or visuals. **No built-in non-vanilla options: extras come from mods.** Bug parity covers every observable
   behavior, including gameplay bugs. Crashes, memory leaks and pure-performance bugs are not replicated.
5. **Speed.** As fast as possible, but only through changes that keep output 1:1 (proven by the parity suites).
6. **UI design skill.** All launcher and website UI work follows Anthropic's **frontend-design** skill
   (`~/.claude/plugins/marketplaces/claude-plugins-official/plugins/frontend-design/skills/frontend-design/SKILL.md`).
   For Claude this is the `frontend-design` skill; delegates are pointed at that file. Work in two passes: a design plan
   (tokens/type/layout/principles) reviewed against the brief, then the build, then a screenshot critique. The design plan
   lives in `packages/ui/DESIGN.md`. The **in-game** UI (e.g. the mod manager) is the exception: it must look like the
   vanilla game UI, drawn with the GUI sprites from the user's own install.

## Hard rules (legal hygiene — CI enforces most of these)

- **Never commit Mojang/Minecraft code, assets or data.** No `.class` or `.java` files, decompiled output, textures,
  sounds, models, lang files, shaders, data-pack JSON, or data-generator reports. All of it is read **at runtime** from
  the user's install. `cargo xtask guard` and `.githooks/pre-commit` check this.
- **Clean-room discipline.** The Rust code is an *independent reimplementation*. Write behavior specs in our own words
  (`docs/specs/`), then implement from the specs. Never transliterate decompiled code line by line, and never paste
  decompiled code into the repo, issues, PRs or docs. Decompiler/javap output stays in
  `~/.cache/zenith/reference/<version>/` (outside the repo) and is used only to understand behavior and to place parity-
  oracle hooks. Facts needed for interoperability are fine: protocol IDs, field orders, numeric constants, RNG call order.
- **Trademark hygiene.** The product name is "Zenith". Say "compatible with Minecraft: Java Edition" or "runs your own
  copy of Minecraft: Java Edition". Never "Zenith Minecraft", "Minecraft Zenith", or "MC" in product names, logos, <!-- guard:allow -->
  crate names or the domain. Every public page carries the notice in `NOTICE.md`. Logos and art are original.
- **Ownership.** The game only starts when local proof passes (official jar SHA-1 matches Mojang's manifest, the full
  asset set is hash-verified, there is launch evidence). Microsoft sign-in is also checked once an approved Azure app ID
  is configured (`docs/plan/06-AUTH-OWNERSHIP.md`).
- **Privacy.** Never put the owner's email, name or tokens in commits, issues, public pages or external requests.
  Tokens live only in the OS keyring.

## Layout

```
crates/        Rust engine crates (zenith-*)    xtask/       repo automation (progress, guard)
progress/      milestones.toml + generated progress.json
docs/          plan/, specs/, adr/, guide/, decisions/, assets/
packages/ui/   shared Svelte 5 components + design tokens (launcher + website)
web/           SvelteKit static site → GitHub Pages     launcher/   Tauri 2 app (milestone M12)
tools/         parity oracle (Java agent, local only), scripts
```

## Commands

```bash
cargo build --workspace              # build everything
cargo test --workspace               # unit + parity tests that need no game files
cargo run -p zenith-install -- check # run local proof against the installed 26.3
cargo xtask progress                 # regenerate progress.json + README SVG tracker
cargo xtask guard                    # asset/trademark/decompile hygiene scan
pnpm -C web dev                      # website dev server
git config core.hooksPath .githooks  # enable the pre-commit guard (once per clone)
```

## Owner's environment

- Prism Launcher (Flatpak): `~/.var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/`
  - jar: `libraries/com/mojang/minecraft/26.3/minecraft-26.3-client.jar`; assets: `assets/` (index `34`)
  - Java 25 runtime: `java/java-runtime-epsilon/bin/java` (needed only for one-time data-gen ingestion + the oracle)
- GitHub: `Distorbed678/Zenith` (public). Pages: https://distorbed678.github.io/Zenith/
- CachyOS, RTX 4080, Vulkan 1.4, 16 threads, 62 GB RAM, Rust 1.99, Node 22, pnpm.

## Conventions

- Conventional commits (`feat(nbt): …`), release-please changelogs. ADR for every major decision.
- Crates are named `zenith-<area>`. The MSRV is pinned in `rust-toolchain.toml`.
- Every engine subsystem ships with a parity suite. A subsystem counts as "done" only at 100% parity.
