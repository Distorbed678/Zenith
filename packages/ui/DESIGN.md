# Zenith UI Design System (`@zenith/ui`)

This document records the visual design plan for **Zenith** — shared by the GitHub Pages website (`web/`) and the future Tauri 2 + Svelte 5 launcher (`launcher/`). It follows the two-pass process in Anthropic's `frontend-design` skill: an initial design plan, followed by a critical review against the brief and generic AI defaults, and the final specification implemented in `tokens.css` and `packages/ui/src/`.

---

## Pass 1: Initial design plan

### Subject & audience
Zenith is an independent, open-source Rust game engine that rebuilds a block-building sandbox game 1:1 on top of the player's own, already-launched copy of Minecraft: Java Edition. It ships zero game assets. Its audience is players, modders, and Rust contributors who care about verifiable parity, speed, and clean-room engineering.

The primary job of the UI is two-fold:
1. Show honestly how far the 1:1 rebuild has progressed (live milestone + parity test tracking).
2. Present technical documentation, downloads, and the mod index with calm clarity — in an **original pixel-crafted** visual language shared with the desktop launcher.

### Aesthetic direction: Celestial Observatory in Crisp Pixel Art
Rather than retro CRT neon or cliché block-game grass textures, Zenith draws directly from its name and emblem (`docs/assets/brand/logo.svg`): **the sun at its zenith over stepped teal-and-navy voxel hills under a deep night sky**.

Every frame, button, and gauge behaves like a physical pixel-crafted observatory instrument lit from directly overhead (zenith light): bright top pixel edges, deep bottom drop edges, stepped notched corners on a strict `3px` pixel grid (`--px: 3px`), and zero blurry drop shadows.

### Core base palette (6 named hex colors)
| Token role | Name | Hex | Purpose |
| :--- | :--- | :--- | :--- |
| Canvas | Midnight Sky | `#0B1020` | Primary page background (night sky default) |
| Surface | Observatory Indigo | `#121A33` | Primary panel surface (`#1B2747` elevated, `#0E152A` recessed) |
| Structure | Stratosphere Navy | `#284A72` | Stepped borders, bevels, hill ridges (`#1F3A5E` deep edge) |
| Ink | Arc Parchment | `#F4F1E8` | Primary readable text (`#F6E7B8` dotted zenith arc) |
| Primary signal | Zenith Gold | `#FFD34D` | Sun core, active zenith peak, primary button highlight (`#FF9E2C` rim, `#FFB938` rays) |
| Secondary signal | Horizon Teal | `#5EE0CF` | Verified parity fill, links, keyboard focus rings, ridge crest (`#2EA3A8` mid-ridge) |

Extended semantic tokens:
- Muted ink: `#9BB0CE` (contrast > 6.5:1 on `#121A33`, > 7.8:1 on `#0B1020`)
- Dim / planned ink: `#7A93B8` (contrast > 4.6:1 on `#121A33`)
- Warning / solar amber: `#FF9E2C`
- Error / pixel coral: `#F26D7D`
- Optional Day Theme (`data-theme="day"`): morning sky canvas `#D8ECF4`, elevated `#EEF7FA`, recessed `#C5E1EC`, deep navy ink `#101B33`, muted `#355072`, deepened gold `#C86D00` and deepened teal `#126E74` to maintain WCAG AA contrast.

### Typography & type scale
Two clearly distinct families plus a dedicated code face:
- **Display & structural headings — Pixelify Sans** (`@fontsource/pixelify-sans`, 400–700, OFL): Pinned by the brief. Crisp mixed-case pixel letterforms with authentic stepped geometry. Used for page titles, section headings, primary button labels, and milestone/phase identifiers. Never set in all-caps.
- **Body & UI prose — Atkinson Hyperlegible** (`@fontsource/atkinson-hyperlegible`, 400 / 700 / italic, OFL): Deliberately chosen over generic sans defaults (Inter, Roboto, Space Grotesk). Engineered by the Braille Institute for unmistakable glyph differentiation (`0`/`O`, `1`/`l`/`I`), open counters that stay crisp on dark navy surfaces, and sturdy humanist geometry that pairs cleanly with pixel headings without imitating them. Used with `font-variant-numeric: tabular-nums` for metrics, percentages, and tables.
- **Code & literal identifiers — IBM Plex Mono** (`@fontsource/ibm-plex-mono`, 400 / 500, OFL): Strictly confined to fenced code blocks, inline code, CLI commands, and file paths — never used as a decorative small-caps data label.

Type scale (1.25 major third with comfortable reading rhythm, body prose capped at `68ch` < 80 characters):
- `--text-xs`: `0.8125rem` (13px) — fine print, table captions, secondary badges
- `--text-sm`: `0.9375rem` (15px) — nav items, metadata, filter chips, button `sm`
- `--text-base`: `1.0625rem` (17px, line-height `1.65`) — body copy and docs prose
- `--text-lg`: `1.25rem` (20px, line-height `1.45`) — lead copy, `h3` headings
- `--text-xl`: `1.625rem` (26px, line-height `1.25`) — `h2` section headings (Pixelify Sans)
- `--text-2xl`: `2.25rem` (36px, line-height `1.12`) — page titles (Pixelify Sans)
- `--text-3xl`: `clamp(2.5rem, 5vw, 3.75rem)` (40–60px, line-height `1.05`) — hero display headline (Pixelify Sans)

### Layout concept & ASCII wireframes

**Alignment guidance:** Left-aligned reading axis (`max-width: 72rem`, centered viewport container) across all prose, tables, roadmaps, and documentation. Only the pixel sun's orbital arc in the hero sky centers horizontally across the upper stage, echoing the symmetric `logo.svg` crest while keeping text left-aligned for fast scanning.

**Home (`/`) — Open sky hero + horizon gauge + editorial ledger + sequence roadmap:**
```
+----------------------------------------------------------------------+
| [Sun] Zenith    Tracker  Docs  Mods  Downloads  Changelog   [||][☾]  |
+----------------------------------------------------------------------+
|  *      .   *       . . . [SUN] . . .       *     .      *           |
|      .          .                       .           [cloud]          |
|   [cloud]    .                             .                         |
|                                                                      |
|  Zenith                                                              |
|  Your copy of the game, rebuilt 1:1 in Rust.                         |
|  [Open progress tracker]  [Read the docs]  [View on GitHub]          |
|     ___/\_____/\____/\_________/\____/\____   (stepped teal hills)   |
|  ==[||||||.................................] 1.4% overall  0% parity |
+----------------------------------------------------------------------+
|  What makes Zenith different                                         |
|  Left lead column (1/3)  |  Right 2-col open spec list (2/3)         |
|  Plain explanation of    |  [icon] Parity proven by tests            |
|  clean-room rebuild      |  [icon] Vulkan 1.3 renderer               |
|  and zero shipped files. |  [icon] Sandboxed WASM mods               |
|                          |  [icon] Shared pixel-crafted launcher     |
|                          |  [icon] Nothing from the game is shipped  |
+----------------------------------------------------------------------+
|  Roadmap (P0 to P7 — true sequence)                                  |
|  [P0] Foundation          [|||||||||||||....] 70%   1 milestone      |
|  [P1] Core data           [.................]  0%   5 milestones     |
|  ...                                                                 |
+----------------------------------------------------------------------+
|  Trademark notice · MIT OR Apache-2.0 · Links                        |
+----------------------------------------------------------------------+
```

**Tracker (`/tracker`) — Telemetry header + SVG pixel history + filterable milestone list:**
```
+----------------------------------------------------------------------+
|  Progress tracker                     Target 26.3  Protocol 777      |
|  +---------------------------+  +----------------------------------+ |
|  | Overall 1.4% [||........] |  | Pixel-stepped SVG history chart  | |
|  | Parity  0.0% [..........] |  | (overall gold vs parity teal)    | |
|  | 0/33 milestones  0/40     |  |                                  | |
|  +---------------------------+  +----------------------------------+ |
|  Phase progress strip (P0..P7 clickable filter tiles)                |
|  Status filter tabs: [All] [In progress] [Done] [Planned]            |
|  +-----------------------------------------------------------------+ |
|  | M0  Foundation & tooling   [In progress]  Weight 2  [|||||..] 70%| |
|  | v 7 of 10 tasks  |  0 parity suites                              | |
|  |   [x] Owner Q&A rounds logged ...                                | |
|  +-----------------------------------------------------------------+ |
+----------------------------------------------------------------------+
```

**Docs (`/docs/[...slug]`) — Three-column observatory reader (2-col on tablet, 1-col on mobile):**
```
+----------------------------------------------------------------------+
| Sidebar (15rem)  | Article prose (max 68ch, <80 chars)  | TOC (13rem)|
|  Plan            |  Document title                      | On this    |
|  Guides          |  Clean left-aligned prose, GFM       | page       |
|  ADRs            |  tables in stepped frames, shiki     |  - Section |
|  Specs           |  code blocks in recessed wells.      |  - Section |
|  Owner decisions |                                      |            |
|  API reference   |                                      |            |
+----------------------------------------------------------------------+
```

### Design principles
1. **Pixel-snapped honesty:** Every border, corner notch, progress segment, and canvas star locks to the `3px` pixel grid. No smooth blur shadows, no frosted glass, no decorative gradients behind text.
2. **One orchestrated motion moment:** On `/` load, the pixel sun rises along the dotted arc from the eastern horizon to the zenith in discrete pixel steps. Everything else stays quiet: slow low-fps sky ambience (pausable via `MotionToggle` and `prefers-reduced-motion`) and crisp press/expand responses to user actions.
3. **Hierarchy through surface, not uniform cards:** Reserve stepped `PixelFrame` boxes for live telemetry, interactive filters, and code wells. Keep narrative features and roadmap lists open on the sky canvas with crisp pixel divider rules.
4. **Zenith lighting:** Light comes from straight above — top edges catch highlight pixels, bottom edges cast crisp solid pixel lips, and pressing a button physically depresses it down by one pixel unit (`3px`).
5. **Launcher-ready modularity:** `@zenith/ui` has zero SvelteKit dependencies, externalizes UI strings in `strings.ts`, supports `data-theme`, `prefers-reduced-motion`, and `prefers-contrast: more`.

---

## Pass 2: Critical review & revisions before building

Reviewing Pass 1 against the brief and the `frontend-design` anti-default checklist revealed four places to sharpen:

1. **Footer separators (caught generic tell):** The initial wireframe sketched `Trademark notice · MIT OR Apache-2.0 · Links` with middle dots (`·`). The brief and skill explicitly forbid middle-dot meta strings. **Revision:** Replace middle dots with clean layout spacing, vertical pixel ticks, or distinct footer columns (legal block left, license and navigation links right).
2. **Hero progress treatment (caught KPI-box default):** Putting multiple separate stat boxes in the hero risks looking like a generic dashboard banner. **Revision:** Integrate overall progress into a single full-width **horizon sill gauge** directly beneath the stepped teal hills of `PixelSky`, visually grounding the sky stage into the page body without separate KPI cards.
3. **Hero text legibility over animated sky:** A centered sun arc spanning the hero could collide with left-aligned text on narrow viewports (360px–640px). **Revision:** Give the hero sky stage dedicated vertical breathing room above the hills (`min-height` with the sun arc occupying the upper zenith band), while keeping the headline, tagline, and action buttons cleanly anchored in the lower-left foreground with a subtle solid-stepped sky backdrop behind text only where needed, ensuring WCAG AA contrast at every viewport width from 360px upward.
4. **Milestone & feature structure (anti-card-grid check):** Avoid wrapping each roadmap phase or each "What makes Zenith different" point in identical bordered cards. **Revision:** Render "What makes Zenith different" as an open asymmetric 2-column ledger with 16×16 original pixel icons and 3px bottom rules; render the P0–P7 roadmap on `/` as a compact stepped timeline table/list; reserve full `PixelFrame` panels for the interactive milestone drawers on `/tracker` and release/mod entries where bordered grouping encodes interactive boundaries.
5. **Typography restraint:** Double-check every label across all routes: zero all-caps eyebrows, zero `→` arrows appended to buttons, zero single-word color highlights inside headlines, zero monospace labels for non-code UI stats.

---

## Pass 3: Post-build screenshot critique & refinements

Inspecting headless Firefox renders at `1280px` desktop and `360px` mobile surfaced four concrete issues that were fixed in code:

1. **Hero sun unobstructed composition:** In the first render, a centered sun arc collided with the left-aligned hero text container during its rise, and wrapping the hero copy inside a bordered box contradicted the open-sky principle. **Fix:** Shifted the hero `PixelSky` arc to the open right sky band on desktop (`>= 900px`) with `pixelSize={4}`, removed the heavy rectangle around the desktop hero copy so it sits cleanly over the night sky, moved the mobile (`< 900px`) sun arc above the hero text band, and sized hero buttons to `md` so all three actions sit on one clean row on desktop.
2. **Numeric legibility at small sizes:** Small tabular metadata (`26.3`, `777`, `5023`, `2026-11-24`, `0 / 33`) rendered in `Pixelify Sans` looked overly blocky compared to Atkinson Hyperlegible. **Fix:** Switched tabular numeric readouts on `/` and `/tracker` to bold `Atkinson Hyperlegible` with `tabular-nums`, keeping `Pixelify Sans` for headlines, phase/milestone badges (`P0`, `M0`), and buttons.
3. **Always-visible mobile motion toggle:** On viewports `<= 900px`, `MotionToggle` was previously tucked inside the mobile drawer. **Fix:** Kept a compact `MotionToggle` button visible right beside the mobile menu trigger in the sticky header at every viewport width down to `360px`.
4. **Horizon sill mobile spacing & inline linked code:** Adjusted `.sill-facts` to a 2-column wrap below `640px` so labels never cramp at `360px`, and styled `a > code` in `.z-prose` with a teal border and `white-space: nowrap` for clean wrapping inside blockquotes.

