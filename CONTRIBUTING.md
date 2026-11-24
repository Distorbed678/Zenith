# Contributing to Zenith

Thanks for helping! Please read these first. They are mandatory:

1. [Legal hygiene](docs/plan/10-LEGAL-AND-HYGIENE.md): **never** commit game code, assets or data; clean-room
   specs before code; no decompiled code anywhere; trademark-safe naming.
2. [Plan overview](docs/plan/00-OVERVIEW.md) and [roadmap](docs/plan/11-ROADMAP.md).

## Workflow
- Enable the hooks: `git config core.hooksPath .githooks`.
- Write or extend the spec in `docs/specs/<area>/` before implementing behavior.
- Every behavior change comes with parity coverage. "Looks right" is not enough.
- Conventional commits: `feat(worldgen): add aquifer barrier noise`.
- Run `cargo fmt`, `cargo clippy --workspace --all-targets`, `cargo test --workspace` and `cargo xtask guard`.
- If you move a milestone, update `progress/milestones.toml` and run `cargo xtask progress`.

## Fidelity rules
Zenith must be pixel-exact and gameplay-exact with vanilla. PRs that add non-vanilla behavior or options to the base
game will be declined. Extras belong in mods.

## License
By contributing, you agree that your contributions are licensed under MIT OR Apache-2.0.
