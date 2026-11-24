# 07: Launcher

Owner decisions: **Tauri 2 + Svelte 5** (3.1), **original pixel-crafted** look (4.1), **Anthropic's frontend-design
skill** guides all UI work (S.1). Imports from **Prism/MultiMC, the official launcher, and Modrinth/CurseForge app
profiles** (4.4). Launching real vanilla Java is *not* in scope. All four launcher extras were accepted (7.4), plus
reduced motion/contrast, localization and a controller UI (8.3).

## Architecture

```
launcher/
  src-tauri/          Rust backend (commands, state, downloads, process management)
    uses: zenith-install, zenith-auth, zenith-mod-index, zenith-storage (snapshots)
  src/                Svelte 5 front-end, built on packages/ui (shared with the website)
```
- Tauri 2 with a strict CSP; the front-end talks to Rust only through typed commands (`tauri-specta` bindings).
- The launcher starts Zenith as a separate process, with arguments for the instance dir, version and auth handle.
  Tokens are passed through a short-lived local IPC handle, never on the command line.

## Features

| Feature | Notes |
|---------|-------|
| First-run wizard | Finds installs (zenith-install) → shows the three proof checks with friendly fixes ("Launch 26.3 once in Prism, then click Re-check") → optional Microsoft sign-in |
| Instances | Create, clone, edit; per-instance game version, mods, packs, Java-free; playtime; icons (original pixel set) |
| Imports | Prism/MultiMC (`instances/*`, mmc-pack.json), official launcher (`launcher_profiles.json`, game dirs), Modrinth App profiles, CurseForge App instances. Worlds, resource packs, data packs, shader packs and options carry over. Java mods are listed as "not compatible" (until R1) |
| Mods | Browse the Zenith index + install local files/URLs; enable/disable; updates; permission review (08); dependency resolution with clear conflict messages |
| Modpacks | Create/export `.zpack`; import `.zpack` and `.mrpack`; lockfile view; one-click update with a diff |
| Instance snapshots | Content-addressed, deduplicated backups (worlds, config, mods) before updates; restore points; retention policy |
| Crash safe-mode | Detect a crash on start → next launch offers Safe Mode (mods off), points to the likely culprit from the crash report, one-click disable |
| 3D skin/cape viewer | Animated preview of your profile skin (needs MS sign-in); drawn with WebGL in the launcher |
| Signed auto-updates | Tauri updater with minisign signatures; Release / Beta / Nightly channels; rollback |
| Localization | Fluent (`.ftl`) for launcher strings; English first; community-translatable |
| Controller / Steam Deck | Gamepad navigation, focus rings, a big-picture layout at 1280×800 |
| Accessibility | `prefers-reduced-motion` and `prefers-contrast` respected; a toggle to pause animated backgrounds |
| Crash reports | Opt-in, local only: view report → "Open GitHub issue" with a pre-filled template |

## Visual design

The design system lives in `packages/ui` (tokens, pixel frames, buttons, meters, `PixelSky` animated background,
`SunLogo`), documented in `packages/ui/DESIGN.md`. It is produced by following the frontend-design skill's two-pass
process. The website uses the exact same package, so the two can't drift apart.

- The **launcher** may additionally show panoramas rendered by Zenith from the user's own assets (legal, because the
  assets are the user's copy). The website never shows game assets.
- The fonts are OFL pixel fonts for v1. An original "Zenith Pixel" bitmap font is a stretch goal.

## Downloads (game files)

The launcher never downloads game files on its own. Ownership requires that the user's own launcher has downloaded and
launched the version. The first-run wizard explains this in plain language and links to the launcher the user already
has.
