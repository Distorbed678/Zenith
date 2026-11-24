# 09: Website and progress tracker

Owner decisions: **SvelteKit static** (5.4), the same pixel-crafted UI as the launcher (4.1) via the shared
`packages/ui`, docs **in repo + on the website + rustdoc** (6.2), tracker numbers from **parity tests + milestones**
(6.1), and sections **tracker, docs + API, downloads + changelog, mod index browser** (9.4). Built following the
**frontend-design skill** (S.1).

URL: **https://distorbed678.github.io/Zenith/** (base path `/Zenith`).

## Site map

| Route | Content | Data source |
|-------|---------|-------------|
| `/` | Hero (sunrise-to-zenith moment), what makes Zenith different, roadmap phases | `progress/progress.json` |
| `/tracker` | Live dashboard: overall + parity %, per-phase bars, milestone cards with task checklists and suites, history chart | `progress/progress.json` |
| `/docs/...` | Rendered `docs/plan`, `docs/guide`, `docs/adr`, `docs/specs`, and the decisions log | repo markdown at build time |
| `/docs/api` | Link to rustdoc at `/Zenith/api/` | CI `cargo doc` output |
| `/downloads` | Release / Beta / Nightly × 4 platforms; requirements | GitHub Releases API (client-side) |
| `/changelog` | `CHANGELOG.md` | release-please |
| `/mods` | Mod index browser with permission chips and `zenith://` deep links | index JSON (sample until M25) |

Every page footer carries the trademark notice from `NOTICE.md`.

## Tracker data model

```
progress/milestones.toml        hand-maintained: phases, milestones, weights, tasks, linked suites
progress/parity/<suite>.json    written by parity runs: {suite, version, passed, total, updatedAt}
        │
        └── cargo xtask progress
              ├── progress/progress.json     (schema v1; read by the website)
              ├── progress/history.json      (overall % and parity % over time)
              └── docs/assets/tracker/*.svg  (README visual tracker: overall, phases, milestones, history)
```

Rules:
- Milestone % = task completion, or the mean of task completion and suite pass ratio when suites are linked.
- A milestone can't be marked `done` unless every task is done and every suite is at 100% (xtask refuses).
- Overall % = effort-weighted mean (weights in milestones.toml), so the number stays honest about the size of the work.
- "Proven 1:1" % = mean pass ratio across all suites; suites with no results count as 0.
- `cargo xtask progress --check` runs in CI, so committed tracker files can't go stale.

## README visual tracker

The README embeds the four generated SVG cards (pixel-styled, self-contained, no external fonts, so they render in
GitHub's sandboxed `<img>`), plus a collapsible milestone table. The same numbers drive the website, so they always
agree.

## Docs pipeline

- Source of truth: markdown in `docs/`. The site collects it with `import.meta.glob`, renders it with mdsvex/GFM +
  shiki at build time, rewrites relative links, and builds the sidebar from folder structure. New docs appear
  automatically.
- rustdoc: `cargo doc --workspace --no-deps` in the Pages workflow → `/api/`.

## Deployment

`.github/workflows/pages.yml`: on pushes to `main` that touch `web/`, `packages/`, `docs/`, `progress/` or
`CHANGELOG.md` → pnpm install → `BASE_PATH=/Zenith pnpm -C web build` → upload → deploy-pages.
