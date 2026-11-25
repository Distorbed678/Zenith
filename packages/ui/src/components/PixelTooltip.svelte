<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    text: string;
    position?: 'top' | 'bottom';
    class?: string;
    children: Snippet;
  }

  let { text, position = 'top', class: className = '', children }: Props = $props();

  let open = $state(false);
  const tooltipId = `z-tip-${Math.random().toString(36).slice(2, 9)}`;

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      open = false;
    }
  }
</script>

<svelte:window onkeydown={open ? handleKeydown : undefined} />

<span
  role="group"
  class="z-tooltip-host {className}"
  onmouseenter={() => (open = true)}
  onmouseleave={() => (open = false)}
  onfocusin={() => (open = true)}
  onfocusout={() => (open = false)}
>
  <span class="z-tooltip-trigger" aria-describedby={open ? tooltipId : undefined}>
    {@render children()}
  </span>

  {#if open}
    <span
      id={tooltipId}
      role="tooltip"
      class="z-tooltip z-tooltip--{position}"
    >
      {text}
    </span>
  {/if}
</span>

<style>
  .z-tooltip-host {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .z-tooltip-trigger {
    display: inline-flex;
    align-items: center;
  }

  .z-tooltip {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    z-index: var(--z-layer-popover);
    width: max-content;
    max-width: 18rem;
    padding: 0.4rem 0.65rem;
    font-family: var(--z-font-body);
    font-size: var(--z-text-xs);
    line-height: 1.4;
    color: var(--z-ink);
    background-color: var(--z-surface-elevated);
    border: var(--z-px) solid var(--z-border-strong);
    box-shadow: var(--z-shadow-popover);
    pointer-events: none;
    text-align: left;
  }

  .z-tooltip--top {
    bottom: calc(100% + 8px);
  }

  .z-tooltip--bottom {
    top: calc(100% + 8px);
  }
</style>
