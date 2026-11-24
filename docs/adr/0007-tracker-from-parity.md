# 0007: Progress tracker is generated from milestones + parity results

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: 6.1

## Decision
`progress/milestones.toml` (tasks, weights) + `progress/parity/*.json` (suite results) → `cargo xtask progress` →
`progress.json` (website), `history.json`, and pixel-styled SVG cards for the README. CI checks that outputs are
current. A milestone can't be `done` with open tasks or suites below 100%.

## Consequences
+ The numbers can't be inflated by hand; README and website always agree.
