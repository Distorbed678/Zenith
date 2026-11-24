# 0005: Mods are sandboxed WASM components

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: 3.3, 3.4, 8.1

## Decision
Zenith mods are WebAssembly components (wasmtime, WIT-defined API), with capability permissions, fuel/epoch limits,
hot reload and a Rust SDK. Java mods are a research item only (R1).

## Consequences
+ Safe to download from the in-game browser; one artifact runs on every OS; any language.
− A new ecosystem; existing Java mods don't run.
