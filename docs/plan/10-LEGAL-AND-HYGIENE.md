# 10: Legal hygiene: clean room, assets, trademarks

> Not legal advice. These are engineering rules chosen to keep the project clearly on the right side of copyright
> and trademark lines, and to keep the public repo from being taken down.

## Why the code isn't a translation of the game

The original request was "decompile and recompile 1:1 in Rust." A line-by-line Rust translation of decompiled game
code would be a derivative work of the game's code. Publishing it would invite a takedown that would end the project.
Zenith therefore reaches 1:1 a different way: it is an **independent reimplementation whose sameness is proven by
tests against the user's own copy.** The goal (identical behavior) is the same; the path is legally sound.

## Clean-room process

```
 Observation side                                  Implementation side
 ─────────────────                                 ───────────────────
 • public format/protocol docs                     • reads ONLY docs/specs/ + public docs
 • data-generator reports (user's jar, local)  ──► • writes original Rust with its own structure
 • black-box oracle experiments                    • verifies with parity suites
 • local reference reading (decompiler/javap
   output in ~/.cache/zenith/reference/, never
   committed) to understand behavior
            │
            ▼
   docs/specs/<area>/*.md: behavior written in our own words:
   rules, state machines, orders of operations, constants and IDs
   (facts needed for interoperability), edge cases and quirks.
   NO code, NO pseudo-code that mirrors the original's structure.
```

Rules:
1. Specs describe **what** happens and in **what order**, never **how the original code is written**. No copied
   identifiers beyond the public, interoperability-relevant names (registry IDs, packet names, NBT keys, JSON fields).
2. The Rust implementation has its own architecture (data-oriented, ECS-style storage, different decomposition). It is
   not a mirror of the original class hierarchy.
3. When an AI agent writes both a spec and an implementation, it must write the spec first, commit it, and implement
   from the spec. Implementation sessions are not given reference-reading output.
4. Decompiler/javap output lives only in `~/.cache/zenith/reference/<version>/`. It is git-ignored by location (outside
   the repo), and `xtask guard` rejects decompiler banners and game-package Java sources anywhere in the repo.
5. Never paste game code into issues, PRs, commits, docs or chat logs that become public.

## Game files: never in the repo

- No `.class`, `.jar`, textures, sounds, models, lang files, shaders, data-pack JSON, structure NBT, data-gen reports,
  region files or screenshots of the game.
- Test fixtures are **generated at test time** from the user's install (or from the CI-downloaded jar), or are
  synthetic data we author ourselves.
- Enforcement: **`cargo xtask guard`** (local pre-commit hook + CI):
  - SHA-1 of every tracked file is checked against a blocklist built from the official jar's entries and the official
    asset index (blobs ≥ 64 bytes);
  - extension rules (`.class`, `.jar`, `.mca`, …; `.java` only in `tools/oracle/`);
  - decompiler banners and game-package Java sources outside the oracle;
  - files over 5 MiB.

## Trademarks

- Product name: **Zenith**. Never combine it with the game's name (e.g. "<game> Zenith", "Zenith for <game>"), and never
  use "MC" in product names, crate names, logos or domains. The guard checks this.
- Allowed descriptive (nominative) use: "compatible with Minecraft: Java Edition", "requires your own copy of
  Minecraft: Java Edition".
- Logo and art are original (pixel sun at its zenith over teal/navy hills; deliberately not grass-block-like).
- Every public surface carries this notice (see `NOTICE.md`):
  "Zenith is an independent project. Not an official Minecraft product. Not approved by or associated with Mojang or
  Microsoft. Minecraft is a trademark of Mojang AB."
- The project never sells anything that depends on the game, and never distributes game content.

## Ownership requirement

Zenith refuses to start without passing local proof (see 06) for the exact version. Content is read from the user's
install at runtime, so a copy of Zenith is useless without a legitimate, launched install.
