# 0001: Clean-room reimplementation verified by parity tests

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: original request; CLAUDE.md hard rules

## Context
The request was to "decompile and recompile 1:1 in Rust". A line-by-line translation of decompiled game code would be a
derivative work. In a public repository, it would very likely be taken down and end the project.

## Decision
Zenith is an independent reimplementation. Behavior is captured in specs written in our own words (`docs/specs/`),
then implemented in original Rust. Sameness is *proven* by the parity toolchain (oracle agent + differ + suites)
running against the user's own copy. Decompiler output may be read locally to understand behavior, but it lives only
in `~/.cache/zenith/reference/`, is never committed, and is never transliterated.

## Consequences
+ Legally defensible; still reaches 1:1 (measured, not claimed).
+ Forces good architecture (data-oriented, parallel-ready) instead of mirroring Java class hierarchies.
− More up-front spec work per subsystem.
