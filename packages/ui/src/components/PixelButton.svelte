<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    variant?: 'primary' | 'secondary' | 'ghost';
    size?: 'sm' | 'md' | 'lg';
    href?: string;
    external?: boolean;
    type?: 'button' | 'submit' | 'reset';
    disabled?: boolean;
    pressed?: boolean;
    ariaLabel?: string;
    onclick?: (event: MouseEvent) => void;
    class?: string;
    children: Snippet;
  }

  let {
    variant = 'primary',
    size = 'md',
    href,
    external = false,
    type = 'button',
    disabled = false,
    pressed,
    ariaLabel,
    onclick,
    class: className = '',
    children
  }: Props = $props();
</script>

{#if href && !disabled}
  <a
    {href}
    target={external ? '_blank' : undefined}
    rel={external ? 'noopener noreferrer' : undefined}
    aria-label={ariaLabel}
    {onclick}
    class="z-btn z-btn--{variant} z-btn--{size} {className}"
  >
    {@render children()}
  </a>
{:else}
  <button
    {type}
    {disabled}
    aria-pressed={pressed}
    aria-label={ariaLabel}
    {onclick}
    class="z-btn z-btn--{variant} z-btn--{size} {className}"
  >
    {@render children()}
  </button>
{/if}

<style>
  .z-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--z-space-2);
    font-family: var(--z-font-display);
    font-weight: 600;
    line-height: 1.15;
    text-decoration: none;
    cursor: pointer;
    user-select: none;
    white-space: nowrap;
    border: var(--z-px) solid transparent;
    transition:
      transform var(--z-dur-press) var(--z-ease-step),
      background-color var(--z-dur-ui) linear,
      border-color var(--z-dur-ui) linear,
      box-shadow var(--z-dur-press) var(--z-ease-step);
  }

  .z-btn--sm {
    font-size: var(--z-text-sm);
    padding: 0.4rem 0.75rem;
    min-height: 2.25rem;
  }

  .z-btn--md {
    font-size: var(--z-text-base);
    padding: 0.55rem 1.15rem;
    min-height: 2.75rem;
  }

  .z-btn--lg {
    font-size: var(--z-text-lg);
    padding: 0.7rem 1.4rem;
    min-height: 3.125rem;
  }

  /* Primary: Zenith Gold with overhead catch-light and solid amber bottom lip */
  .z-btn--primary {
    background-color: var(--z-accent);
    color: #0b1020;
    border-color: var(--z-accent-edge);
    box-shadow:
      inset 0 var(--z-px) 0 0 #fff1a6,
      inset 0 calc(var(--z-px) * -1) 0 0 var(--z-accent-edge),
      0 var(--z-px) 0 0 var(--z-accent-shadow);
  }

  .z-btn--primary:hover:not(:disabled) {
    background-color: var(--z-accent-hover);
  }

  .z-btn--primary:active:not(:disabled) {
    transform: translateY(var(--z-px));
    box-shadow:
      inset 0 var(--z-px) 0 0 var(--z-accent-edge),
      0 0 0 0 var(--z-accent-shadow);
  }

  /* Secondary: Observatory surface with stepped steel bevel */
  .z-btn--secondary {
    background-color: var(--z-surface-elevated);
    color: var(--z-ink);
    border-color: var(--z-border-strong);
    box-shadow:
      inset 0 var(--z-px) 0 0 var(--z-bevel-light),
      inset 0 calc(var(--z-px) * -1) 0 0 var(--z-bevel-dark),
      var(--z-shadow-button);
  }

  .z-btn--secondary:hover:not(:disabled) {
    border-color: var(--z-horizon-bright);
    color: var(--z-horizon-bright);
  }

  .z-btn--secondary:active:not(:disabled),
  .z-btn--secondary[aria-pressed='true'] {
    transform: translateY(var(--z-px));
    box-shadow: inset 0 var(--z-px) 0 0 var(--z-bevel-dark);
  }

  /* Ghost: Quiet stepped outline */
  .z-btn--ghost {
    background-color: transparent;
    color: var(--z-ink);
    border-color: var(--z-border);
    box-shadow: none;
  }

  .z-btn--ghost:hover:not(:disabled) {
    background-color: var(--z-surface);
    border-color: var(--z-horizon-mid);
    color: var(--z-horizon-bright);
  }

  .z-btn--ghost:active:not(:disabled),
  .z-btn--ghost[aria-pressed='true'] {
    transform: translateY(var(--z-px));
    background-color: var(--z-surface-inset);
  }

  .z-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
