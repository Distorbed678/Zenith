# 12: Infrastructure, CI and releases

Accepted (round 8.2/8.4): asset/trademark guard (CI + hook), ADRs, conventional commits + release-please, nightly
builds, signed mod index, reproducible builds, keyring token storage. (cargo-deny/audit was *not* chosen; dependency
updates are reviewed manually.)

## Repository conventions

- `main` is protected: PRs need CI to pass. Small autonomous commits go to feature branches → PR → squash-merge.
- **Conventional commits** (`feat(worldgen): …`, `fix(nbt): …`, `docs(plan): …`). **release-please** maintains
  `CHANGELOG.md` and the version bumps, and drafts GitHub Releases.
- **ADRs** in `docs/adr/NNNN-title.md` for every significant decision (template in `docs/adr/0000-template.md`).
- Git hooks in `.githooks/` (enable with `git config core.hooksPath .githooks`):
  `pre-commit` → `cargo xtask guard --staged`, plus `cargo xtask progress` when `progress/` is staged.

## CI workflows (`.github/workflows/`)

| Workflow | Trigger | Jobs |
|----------|---------|------|
| `ci.yml` | push/PR | fmt, clippy (`-D warnings`), test (Linux/Windows/macOS), `xtask guard --fetch`, `xtask progress --check` |
| `pages.yml` | push to main (site/docs/progress paths) | build SvelteKit site + rustdoc → GitHub Pages |
| `parity.yml` | nightly + manual | download the official jar/assets from Mojang inside the job (SHA-1 verified, never uploaded), Java 25, run parity suites, commit `progress/parity/*.json` via a bot PR |
| `perf.yml` | nightly (self-hosted runner later) | bench scenarios → `progress/perf/` |
| `version-watch.yml` | every 6h | version diff bot (03) |
| `nightly.yml` | nightly | build launcher + game for 4 targets, sign, publish the `nightly` pre-release |
| `release-please.yml` | push to main | release PRs; on merge: build, sign, publish |

Game files in CI: downloaded from Mojang's official URLs at job time, verified against the manifest, kept in the job's
temp dir, and **never** included in caches that leave the job, artifacts or releases.

## Build matrix & packaging

| Target | Runner | Packages |
|--------|--------|----------|
| `x86_64-unknown-linux-gnu` | ubuntu-24.04 | AppImage, Flatpak (Flathub later), AUR `zenith-bin`, tar.gz |
| `aarch64-unknown-linux-gnu` | ubuntu-24.04-arm | AppImage, tar.gz |
| `x86_64-pc-windows-msvc` | windows-2025 | MSI (WiX) + NSIS, portable zip |
| `aarch64-apple-darwin` | macos-15 | DMG; notarization needs an Apple Developer account ($99/yr). Until then, unsigned with instructions (open question Q2) |

## Reproducible builds

- Pinned toolchain (`rust-toolchain.toml`), committed `Cargo.lock`/`pnpm-lock.yaml`, `SOURCE_DATE_EPOCH`,
  `--remap-path-prefix`, deterministic archive ordering.
- A `reproduce.yml` job rebuilds the last release on a fresh runner and compares hashes.
- SLSA provenance attestations (`actions/attest-build-provenance`) on release artifacts.

## Signing

- Updates: minisign keypair. The private key lives in GitHub Actions secrets (the owner creates it; open question Q4).
- Mod index: author keys pinned in the index; Sigstore keyless as an option.
- Windows Authenticode / macOS notarization: later (cost decision).
