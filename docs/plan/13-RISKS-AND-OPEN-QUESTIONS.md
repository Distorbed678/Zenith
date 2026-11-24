# 13: Risks and open questions

## Risks

| # | Risk | Impact | Mitigation |
|---|------|--------|------------|
| R-1 | **Sheer scale.** The target has ~11k classes; a full 1:1 reimplementation is a multi-year effort | Very high | Headless-first ordering; effort-weighted honest tracker; delegates for parallel tracks; parity suites keep work from regressing |
| R-2 | Legal challenge (copyright/trademark) | Project-ending | Clean-room process, guard, no game files, nominative trademark use, ownership requirement (10) |
| R-3 | Java FP/collection-order subtleties cause tiny divergences | High | `zenith-math` Java-exact helpers; Java-compatible hash map where order leaks; oracle finds the *first* divergence |
| R-4 | Pixel parity across GPU vendors | Medium | Reference = vanilla on the same machine; ≤1 LSB tolerance only for rasterization rules; vendor matrix in nightly |
| R-5 | Mojang changes formats fast (several releases per year) | Medium | Version diff bot + data-driven ingestion; most changes are data |
| R-6 | Mojang denies or delays Azure app approval | Medium | Local proof works without it; only online-mode servers wait |
| R-7 | The data generator needs a JVM | Low | Use the launcher-installed Java; JVM-free fallback for plain data; tables cached per jar hash |
| R-8 | WebKitGTK performance for launcher animations on Linux/NVIDIA | Low–medium | Canvas animations capped at 30 fps, pause when hidden, reduced-motion path; profile early |
| R-9 | WASM boundary overhead for heavy mods | Medium | Bulk APIs; shared-memory views for block arrays; benchmarks in the SDK |
| R-10 | Name collision: an existing mod is called "Zenith" | Low | Always present as "Zenith, an independent game engine"; distinct logo/brand |

## Open questions for the owner (ask with the question tool when relevant)

| # | Question | Needed by |
|---|----------|-----------|
| Q1 | Will you register the Azure app now (free) so Mojang's approval clock starts early? | M23 (earlier is better) |
| Q2 | Pay for an Apple Developer account (notarized macOS builds), or ship unsigned with instructions? | M29 |
| Q3 | First *older* version target after 26.4 (e.g. 1.21.11, which you also have installed)? | M31 |
| Q4 | Create the minisign update-signing key yourself, or let CI generate it and store it as a secret? | First nightly |
| Q5 | Font rasterization for TTF glyph providers: exact FreeType-compatible output is needed for pixel parity. OK to link FreeType (FTL/GPL dual), or require a pure-Rust reimplementation? | M18 |
| Q6 | Self-hosted perf runner (your machine) for stable benchmarks, or cloud runners with noise? | M28 |
| Q7 | Original "Zenith Pixel" font: commission/design it, or keep OFL pixel fonts? | Anytime |
