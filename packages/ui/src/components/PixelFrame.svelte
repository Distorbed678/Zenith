<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    variant?: 'elevated' | 'flat' | 'inset' | 'accent';
    padding?: 'none' | 'sm' | 'md' | 'lg';
    as?: 'div' | 'section' | 'article' | 'aside';
    id?: string;
    class?: string;
    children: Snippet;
  }

  let {
    variant = 'elevated',
    padding = 'md',
    as = 'div',
    id,
    class: className = '',
    children
  }: Props = $props();
</script>

<svelte:element
  this={as}
  {id}
  class="z-frame z-frame--{variant} z-frame--pad-{padding} {className}"
>
  {@render children()}
</svelte:element>

<style>
  .z-frame {
    position: relative;
    background-color: var(--z-surface);
    border: var(--z-px) solid var(--z-border);
    box-shadow:
      inset 0 var(--z-px) 0 0 var(--z-bevel-light),
      inset 0 calc(var(--z-px) * -1) 0 0 var(--z-bevel-dark),
      var(--z-shadow-frame);
    /* Stepped 1-pixel-unit corner notches */
    clip-path: polygon(
      var(--z-px) 0,
      calc(100% - var(--z-px)) 0,
      calc(100% - var(--z-px)) var(--z-px),
      100% var(--z-px),
      100% calc(100% - var(--z-px)),
      calc(100% - var(--z-px)) calc(100% - var(--z-px)),
      calc(100% - var(--z-px)) 100%,
      var(--z-px) 100%,
      var(--z-px) calc(100% - var(--z-px)),
      0 calc(100% - var(--z-px)),
      0 var(--z-px),
      var(--z-px) var(--z-px)
    );
  }

  .z-frame--elevated {
    background-color: var(--z-surface);
  }

  .z-frame--flat {
    background-color: var(--z-surface);
    box-shadow: inset 0 var(--z-px) 0 0 var(--z-navy-deep);
  }

  .z-frame--inset {
    background-color: var(--z-surface-inset);
    border-color: var(--z-navy-deep);
    box-shadow:
      inset 0 var(--z-px) 0 0 var(--z-bevel-dark),
      inset var(--z-px) 0 0 0 var(--z-bevel-dark),
      inset 0 calc(var(--z-px) * -1) 0 0 var(--z-border);
  }

  .z-frame--accent {
    background-color: var(--z-surface-elevated);
    border-color: var(--z-horizon-mid);
    box-shadow:
      inset 0 var(--z-px) 0 0 var(--z-horizon-bright),
      inset 0 calc(var(--z-px) * -1) 0 0 var(--z-bevel-dark),
      var(--z-shadow-frame);
  }

  .z-frame--pad-none {
    padding: 0;
  }

  .z-frame--pad-sm {
    padding: var(--z-space-3);
  }

  .z-frame--pad-md {
    padding: var(--z-space-5);
  }

  .z-frame--pad-lg {
    padding: var(--z-space-6);
  }

  @media (max-width: 640px) {
    .z-frame--pad-lg {
      padding: var(--z-space-4);
    }

    .z-frame--pad-md {
      padding: var(--z-space-4);
    }
  }
</style>
