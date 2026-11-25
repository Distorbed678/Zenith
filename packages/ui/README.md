# `@zenith/ui`

Shared pixel-crafted Svelte 5 component library and CSS design tokens for **Zenith** — used by both the static SvelteKit website (`web/`) and the future Tauri 2 desktop launcher (`launcher/`).

This package has **no SvelteKit-only imports** so it runs cleanly in plain Vite + Tauri apps as well as SvelteKit.

See [DESIGN.md](./DESIGN.md) for the palette, typography, layout principles, and two-pass design review.

## Setup

Import the fonts and design tokens once in your app entry point:

```ts
import '@zenith/ui/fonts.css';
import '@zenith/ui/tokens.css';
```

## Components

### `PixelFrame`
Stepped-corner pixel container with overhead zenith bevel lighting.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `variant` | `'elevated' \| 'flat' \| 'inset' \| 'accent'` | `'elevated'` | Surface elevation and border treatment. |
| `padding` | `'none' \| 'sm' \| 'md' \| 'lg'` | `'md'` | Internal padding scale. |
| `as` | `'div' \| 'section' \| 'article' \| 'aside'` | `'div'` | Rendered semantic HTML element. |
| `id` | `string` | — | Optional element id. |
| `class` | `string` | `''` | Extra CSS classes. |
| `children` | `Snippet` | required | Frame content. |

### `PixelButton`
Tactile button or link with a crisp 1-pixel-unit (`3px`) press-down offset.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `variant` | `'primary' \| 'secondary' \| 'ghost'` | `'primary'` | Visual prominence. |
| `size` | `'sm' \| 'md' \| 'lg'` | `'md'` | Button dimensions. |
| `href` | `string` | — | When provided (and not disabled), renders an `<a>` instead of `<button>`. |
| `external` | `boolean` | `false` | Adds `target="_blank" rel="noopener noreferrer"` for links. |
| `type` | `'button' \| 'submit' \| 'reset'` | `'button'` | Button type when rendered as `<button>`. |
| `disabled` | `boolean` | `false` | Disables interaction. |
| `pressed` | `boolean` | — | Sets `aria-pressed` for toggle buttons. |
| `ariaLabel` | `string` | — | Accessible label override. |
| `onclick` | `(event: MouseEvent) => void` | — | Click handler. |
| `children` | `Snippet` | required | Button label/content. |

### `PixelProgress`
Segmented pixel progress meter with `role="progressbar"`.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `value` | `number` | required | Current progress value. |
| `max` | `number` | `100` | Maximum value. |
| `segments` | `number` | `20` | Number of discrete pixel blocks. |
| `tone` | `'gold' \| 'teal'` | `'gold'` | Fill color (`gold` for overall, `teal` for parity). |
| `size` | `'sm' \| 'md' \| 'lg'` | `'md'` | Bar height. |
| `label` | `string` | — | Optional visible label (also used as `aria-label`). |
| `showValue` | `boolean` | `false` | Shows formatted percentage beside the label. |

### `StatusBadge`
Sentence-case status pill with a pixel pip indicator.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `status` | `'done' \| 'in-progress' \| 'planned' \| 'warning' \| 'neutral'` | required | Visual status tone. |
| `label` | `string` | localized default | Optional custom text label. |

### `PixelTabs`
Accessible horizontal tablist with Left/Right/Home/End keyboard navigation.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `tabs` | `Array<{ id: string; label: string; count?: number }>` | required | Tab definitions. |
| `active` | `string` (bindable) | required | Currently selected tab `id`. |
| `label` | `string` | required | Accessible `aria-label` for the tablist. |
| `onchange` | `(id: string) => void` | — | Callback fired when active tab changes. |

### `PixelTooltip`
Hover and keyboard-focus tooltip rendered in a stepped pixel popover. Dismisses on `Escape`.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `text` | `string` | required | Tooltip text. |
| `position` | `'top' \| 'bottom'` | `'top'` | Placement relative to trigger. |
| `children` | `Snippet` | required | Trigger element. |

### `SunLogo`
Original 32×32 pixel emblem of the sun at its zenith over stepped teal/navy hills (`shape-rendering="crispEdges"`).

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `size` | `number` | `32` | Rendered pixel width and height. |
| `decorative` | `boolean` | `false` | Sets `aria-hidden="true"` when used alongside visible text. |
| `title` | `string` | `'Zenith'` | Accessible `<title>` when non-decorative. |

### `PixelSky`
DPR-aware, pixel-snapped HTML5 canvas rendering stars, drifting clouds, dotted zenith arc, sun/moon, and stepped parallax teal/navy hills. Capped at ~16 fps (≤30 fps) and automatically pauses when `document.hidden`, `prefers-reduced-motion: reduce`, or `motionState.paused` is true.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `mode` | `'hero' \| 'ambient'` | `'hero'` | `'hero'` plays the one-shot sunrise-to-zenith climb on load; `'ambient'` tracks local time on the arc. |
| `pixelSize` | `number` | `3` | Base CSS pixels per virtual canvas pixel. |
| `paused` | `boolean` | `false` | Forces static rendering independent of global `motionState`. |
| `showHills` | `boolean` | `true` | Draws the stepped teal/navy parallax hills along the horizon. |

### `MotionToggle` & `motionState`
Visible toggle button to pause or resume animated backgrounds. Persists the preference in `localStorage` inside `try/catch` and respects `prefers-reduced-motion: reduce`.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `compact` | `boolean` | `false` | Icon-only mode for tight toolbars. |

### `ThemeToggle` & `themeState`
Optional toggle between the default Night Sky (`data-theme="night"`) and Day Sky (`data-theme="day"`), persisted in `localStorage` inside `try/catch`.

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `compact` | `boolean` | `false` | Icon-only mode. |

### `PixelIcon`
Original 16×16 pixel-art SVG icon set (`check`, `shield`, `gpu`, `wasm`, `launcher`, `lock`, `pause`, `play`, `sun`, `moon`, `chevron-down`, `chevron-right`, `menu`, `close`, `search`, `download`, `book`, `code`, `warning`).

| Prop | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `name` | `PixelIconName` | required | Icon identifier. |
| `size` | `number` | `16` | Width and height in pixels. |
