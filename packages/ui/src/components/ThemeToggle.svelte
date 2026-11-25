<script lang="ts">
  import { onMount } from 'svelte';
  import { themeState } from '../theme.svelte';
  import { uiStrings } from '../strings';
  import PixelIcon from './PixelIcon.svelte';

  interface Props {
    compact?: boolean;
    class?: string;
  }

  let { compact = false, class: className = '' }: Props = $props();

  onMount(() => {
    themeState.init();
  });

  const isDay = $derived(themeState.current === 'day');
  const label = $derived(isDay ? uiStrings.theme.switchToNight : uiStrings.theme.switchToDay);
  const shortText = $derived(isDay ? uiStrings.theme.dayShort : uiStrings.theme.nightShort);
</script>

<button
  type="button"
  class="z-theme-toggle {className}"
  aria-pressed={isDay}
  aria-label={label}
  title={label}
  onclick={() => themeState.toggle()}
>
  <PixelIcon name={isDay ? 'sun' : 'moon'} size={14} />
  {#if !compact}
    <span>{shortText}</span>
  {/if}
</button>

<style>
  .z-theme-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 2.125rem;
    padding: 0.3rem 0.65rem;
    font-family: var(--z-font-body);
    font-size: var(--z-text-xs);
    font-weight: 700;
    color: var(--z-ink-muted);
    background-color: var(--z-surface);
    border: 2px solid var(--z-border);
    cursor: pointer;
    transition:
      color var(--z-dur-ui) linear,
      border-color var(--z-dur-ui) linear,
      transform var(--z-dur-press) var(--z-ease-step);
  }

  .z-theme-toggle:hover {
    color: var(--z-ink);
    border-color: var(--z-horizon-bright);
  }

  .z-theme-toggle:active {
    transform: translateY(2px);
  }
</style>
