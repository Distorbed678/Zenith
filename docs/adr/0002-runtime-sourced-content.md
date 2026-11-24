# 0002: All game content is read at runtime from the user's install

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: original request ("don't ship any assets", "require a legitimate copy launched once"), 2.2

## Decision
Assets, data packs, shaders, structure files and data-generator outputs are read or produced at runtime from the
user's install and cached per jar hash in the user's cache dir. The repo holds none of them; `xtask guard` enforces
this with a hash blocklist built from the official jar and asset index.

## Consequences
+ The project distributes no game content; the ownership requirement is natural.
− First launch per version runs an ingestion step (needs the launcher-provided Java once).
