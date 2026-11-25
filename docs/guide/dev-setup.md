# Developer setup

## Toolchain

| Tool | Version | Why |
|------|---------|-----|
| Rust | pinned in `rust-toolchain.toml` (1.99.0) | Engine, xtask, launcher backend |
| Node.js | 22 | Website and launcher front-end |
| pnpm | 10+ | JS workspace |
| Java | 25 (your launcher's bundled runtime works) | Only for data-gen ingestion and the parity oracle |
| Vulkan SDK | 1.3+ | Validation layers while developing the renderer |

## First steps

```bash
git clone https://github.com/Distorbed678/Zenith && cd Zenith
git config core.hooksPath .githooks        # pre-commit guard
cargo build --workspace
cargo test --workspace
cargo run -p zenith-install -- check       # needs your own launched 26.3 install
cargo xtask guard                          # hygiene scan (hash rule needs a local install, or use --fetch)
cargo xtask progress                       # regenerate the tracker after editing progress/milestones.toml
pnpm install && pnpm -C web dev            # website at http://localhost:5173
cargo run -p zenith-data -- ingest         # build game tables from your install (uses your launcher's Java)
cargo xtask oracle run --ticks 100 --out /tmp/vanilla.zpd --accept-eula   # parity oracle (asks for EULA once)
```

## Reference material (local only)

To understand a behavior you may inspect your own jar locally, for example with `javap` or a decompiler. Keep all
output in `~/.cache/zenith/reference/<version>/`, **outside the repo**. Write what you learn as a spec in your own
words under `docs/specs/`, then implement from the spec. Never paste game code anywhere in the repo, issues or PRs.
See [docs/specs/README.md](../specs/README.md).

## Useful environment variables

| Variable | Effect |
|----------|--------|
| `ZENITH_LAUNCHER_ROOT` | Add a launcher directory to search |
| `ZENITH_CACHE_DIR` | Override the cache directory |
