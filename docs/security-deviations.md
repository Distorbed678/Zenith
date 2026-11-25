# Security deviations from vanilla

Owner decision 11.1: Zenith replicates every observable vanilla behavior, including gameplay bugs, **except security
weaknesses**. Those are fixed: the unsafe action is refused, and Zenith logs a warning. Normal content behaves exactly
like vanilla, and only malicious or broken input differs. Every deviation is listed here with the condition that
triggers it.

| # | Area | Vanilla behavior | Zenith behavior | Triggered only by |
|---|------|------------------|-----------------|-------------------|
| 1 | Data/resource packs: overlays | In a folder pack, an overlay directory named `..` resolves to the pack's parent folder, so the pack can read files outside itself | The overlay is refused (treated as missing) and a warning names the pack | A `pack.mcmeta` overlay entry whose directory is `..` (or escapes the pack root) |
