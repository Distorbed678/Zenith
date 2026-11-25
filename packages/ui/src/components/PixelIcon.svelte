<script lang="ts" module>
  export type PixelIconName =
    | 'check'
    | 'shield'
    | 'gpu'
    | 'wasm'
    | 'launcher'
    | 'lock'
    | 'pause'
    | 'play'
    | 'sun'
    | 'moon'
    | 'chevron-down'
    | 'chevron-right'
    | 'menu'
    | 'close'
    | 'search'
    | 'download'
    | 'book'
    | 'code'
    | 'warning';
</script>

<script lang="ts">
  interface Props {
    name: PixelIconName;
    size?: number;
    class?: string;
  }

  let { name, size = 16, class: className = '' }: Props = $props();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  viewBox="0 0 16 16"
  width={size}
  height={size}
  fill="currentColor"
  shape-rendering="crispEdges"
  aria-hidden="true"
  class="z-pixel-icon z-pixelated {className}"
>
  {#if name === 'check'}
    <rect x="2" y="8" width="2" height="2" />
    <rect x="4" y="10" width="2" height="2" />
    <rect x="6" y="8" width="2" height="2" />
    <rect x="8" y="6" width="2" height="2" />
    <rect x="10" y="4" width="2" height="2" />
    <rect x="12" y="2" width="2" height="2" />
  {:else if name === 'shield'}
    <rect x="3" y="2" width="10" height="2" />
    <rect x="2" y="3" width="2" height="6" />
    <rect x="12" y="3" width="2" height="6" />
    <rect x="4" y="9" width="2" height="2" />
    <rect x="10" y="9" width="2" height="2" />
    <rect x="6" y="11" width="4" height="2" />
    <rect x="7" y="13" width="2" height="1" />
    <rect x="6" y="6" width="1" height="2" />
    <rect x="7" y="8" width="2" height="1" />
    <rect x="9" y="5" width="2" height="2" />
  {:else if name === 'gpu'}
    <rect x="2" y="4" width="12" height="8" opacity="0.22" />
    <rect x="2" y="4" width="12" height="1" />
    <rect x="2" y="11" width="12" height="1" />
    <rect x="2" y="4" width="1" height="8" />
    <rect x="13" y="4" width="1" height="8" />
    <rect x="5" y="6" width="2" height="4" />
    <rect x="9" y="6" width="2" height="4" />
    <rect x="4" y="2" width="2" height="2" />
    <rect x="10" y="2" width="2" height="2" />
    <rect x="4" y="12" width="2" height="2" />
    <rect x="10" y="12" width="2" height="2" />
  {:else if name === 'wasm'}
    <rect x="3" y="2" width="10" height="2" />
    <rect x="2" y="4" width="2" height="8" />
    <rect x="12" y="4" width="2" height="8" />
    <rect x="3" y="12" width="10" height="2" />
    <rect x="5" y="6" width="2" height="4" />
    <rect x="7" y="8" width="2" height="2" />
    <rect x="9" y="6" width="2" height="4" />
  {:else if name === 'launcher'}
    <rect x="2" y="2" width="12" height="2" />
    <rect x="2" y="4" width="1" height="10" />
    <rect x="13" y="4" width="1" height="10" />
    <rect x="2" y="13" width="12" height="1" />
    <rect x="6" y="6" width="2" height="5" />
    <rect x="8" y="7" width="2" height="3" />
    <rect x="10" y="8" width="1" height="1" />
  {:else if name === 'lock'}
    <rect x="5" y="2" width="6" height="2" />
    <rect x="4" y="4" width="2" height="3" />
    <rect x="10" y="4" width="2" height="3" />
    <rect x="3" y="7" width="10" height="7" />
    <rect x="7" y="9" width="2" height="3" fill="var(--z-bg)" />
  {:else if name === 'pause'}
    <rect x="4" y="3" width="3" height="10" />
    <rect x="9" y="3" width="3" height="10" />
  {:else if name === 'play'}
    <rect x="4" y="3" width="2" height="10" />
    <rect x="6" y="4" width="2" height="8" />
    <rect x="8" y="5" width="2" height="6" />
    <rect x="10" y="6" width="2" height="4" />
    <rect x="12" y="7" width="1" height="2" />
  {:else if name === 'sun'}
    <rect x="7" y="1" width="2" height="2" />
    <rect x="7" y="13" width="2" height="2" />
    <rect x="1" y="7" width="2" height="2" />
    <rect x="13" y="7" width="2" height="2" />
    <rect x="3" y="3" width="2" height="2" />
    <rect x="11" y="3" width="2" height="2" />
    <rect x="3" y="11" width="2" height="2" />
    <rect x="11" y="11" width="2" height="2" />
    <rect x="5" y="5" width="6" height="6" />
  {:else if name === 'moon'}
    <rect x="6" y="2" width="4" height="2" />
    <rect x="4" y="4" width="4" height="2" />
    <rect x="3" y="6" width="4" height="4" />
    <rect x="4" y="10" width="4" height="2" />
    <rect x="6" y="12" width="6" height="2" />
    <rect x="10" y="10" width="3" height="2" />
  {:else if name === 'chevron-down'}
    <rect x="3" y="5" width="2" height="2" />
    <rect x="5" y="7" width="2" height="2" />
    <rect x="7" y="9" width="2" height="2" />
    <rect x="9" y="7" width="2" height="2" />
    <rect x="11" y="5" width="2" height="2" />
  {:else if name === 'chevron-right'}
    <rect x="5" y="3" width="2" height="2" />
    <rect x="7" y="5" width="2" height="2" />
    <rect x="9" y="7" width="2" height="2" />
    <rect x="7" y="9" width="2" height="2" />
    <rect x="5" y="11" width="2" height="2" />
  {:else if name === 'menu'}
    <rect x="2" y="3" width="12" height="2" />
    <rect x="2" y="7" width="12" height="2" />
    <rect x="2" y="11" width="12" height="2" />
  {:else if name === 'close'}
    <rect x="3" y="3" width="2" height="2" />
    <rect x="5" y="5" width="2" height="2" />
    <rect x="7" y="7" width="2" height="2" />
    <rect x="9" y="9" width="2" height="2" />
    <rect x="11" y="11" width="2" height="2" />
    <rect x="11" y="3" width="2" height="2" />
    <rect x="9" y="5" width="2" height="2" />
    <rect x="5" y="9" width="2" height="2" />
    <rect x="3" y="11" width="2" height="2" />
  {:else if name === 'search'}
    <rect x="4" y="2" width="5" height="2" />
    <rect x="2" y="4" width="2" height="5" />
    <rect x="9" y="4" width="2" height="5" />
    <rect x="4" y="9" width="5" height="2" />
    <rect x="10" y="10" width="2" height="2" />
    <rect x="12" y="12" width="2" height="2" />
  {:else if name === 'download'}
    <rect x="7" y="2" width="2" height="7" />
    <rect x="4" y="6" width="2" height="2" />
    <rect x="10" y="6" width="2" height="2" />
    <rect x="6" y="8" width="4" height="2" />
    <rect x="2" y="12" width="12" height="2" />
  {:else if name === 'book'}
    <rect x="2" y="2" width="5" height="2" />
    <rect x="9" y="2" width="5" height="2" />
    <rect x="2" y="4" width="2" height="9" />
    <rect x="7" y="4" width="2" height="9" />
    <rect x="12" y="4" width="2" height="9" />
    <rect x="2" y="12" width="12" height="2" />
  {:else if name === 'code'}
    <rect x="4" y="4" width="2" height="2" />
    <rect x="2" y="6" width="2" height="4" />
    <rect x="4" y="10" width="2" height="2" />
    <rect x="10" y="4" width="2" height="2" />
    <rect x="12" y="6" width="2" height="4" />
    <rect x="10" y="10" width="2" height="2" />
    <rect x="7" y="5" width="2" height="6" />
  {:else if name === 'warning'}
    <rect x="7" y="2" width="2" height="2" />
    <rect x="6" y="4" width="4" height="2" />
    <rect x="5" y="6" width="6" height="2" />
    <rect x="4" y="8" width="8" height="2" />
    <rect x="3" y="10" width="10" height="2" />
    <rect x="2" y="12" width="12" height="2" />
    <rect x="7" y="6" width="2" height="4" fill="var(--z-bg)" />
    <rect x="7" y="11" width="2" height="1" fill="var(--z-bg)" />
  {/if}
</svg>

<style>
  .z-pixel-icon {
    flex-shrink: 0;
  }
</style>
