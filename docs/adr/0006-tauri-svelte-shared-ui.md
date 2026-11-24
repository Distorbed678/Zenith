# 0006: Launcher on Tauri 2 + Svelte 5; website shares packages/ui

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: 3.1, 4.1, 5.4, S.1

## Decision
The launcher is Tauri 2 with a Svelte 5 front-end. The website is SvelteKit (static) on GitHub Pages. Both consume
`packages/ui` (design tokens + components), designed with Anthropic's frontend-design skill in an original
pixel-crafted style.

## Consequences
+ The launcher and website can't drift apart visually.
− Tauri uses WebKitGTK on Linux; animations need care (fps caps, pause when hidden).
