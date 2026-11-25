# 0009: Per-block-state static facts are specified and reimplemented, proven against every state

- Status: accepted
- Date: 2026-11-25
- Follows from: ADR 0001 (clean-room reimplementation verified by parity), ADR 0002 (runtime-sourced content)

## Context
World generation (features, structures) and later physics, lighting and rendering depend on facts that every block
state has but that no data file lists: collision / outline / occlusion shapes, which faces are sturdy for each support
type, whether it can support something at its centre, solid-render and opacity flags, light emission, replaceability,
the fluid state it holds, and similar. In the game these come from code (per-block classes and their property-driven
shape logic), not from the data generator's reports. The M8 reader specs mark several of these as needed
(`structures-legacy-pieces.md` [oracle] 5, `features.md` §3).

Two ways were considered:
1. **Extract at ingest.** Run an extraction tool against the user's jar and store the facts in the per-version cache,
   like the data-generator ingestion of ADR 0002.
2. **Specify and reimplement.** A reader writes the rules (shape construction per block family, how sturdiness is
   derived from shapes, the flags), an implementer builds them in Rust, and an oracle suite compares Zenith's facts
   with the game's for every block state of the version.

## Decision
Option 2. The data generator is the game's own export feature and its outputs are data; shape logic is behaviour,
which ADR 0001 says Zenith reimplements. Collision shapes are needed for entity movement anyway, so they must exist in
Rust regardless. A `block-facts` parity suite dumps the game's facts for **every block state** (about 29,700 in 26.3)
and must be 100% before anything depends on them. Extraction stays a debugging aid in the oracle only, never a
runtime input.

## Consequences
+ Consistent with the clean-room model; one implementation serves world generation, physics, lighting and rendering.
+ Cheap, exhaustive verification: the state space is finite, so the suite checks all of it.
− A large spec (block families, shape helpers, per-block exceptions) before M8 features and structures can be exact.
