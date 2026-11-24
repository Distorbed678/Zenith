# 0008: Build the headless core first

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: 9.1, 6.4

## Decision
Milestones run P0 → P1 (core data) → P2 (simulation) → P3 (protocol) → P4 (client) → P5 (platform) → P6 → P7,
autonomously. The renderer bring-up (M17) may run in parallel, because it doesn't need the simulation.

## Consequences
+ The hardest parity work is tested without a GPU in the loop.
− Nothing visual to play with for a long time (the website tracker shows the progress).
