<script lang="ts">
  import { onMount } from 'svelte';
  import { motionState } from '../motion.svelte';
  import { uiStrings } from '../strings';
  import PixelIcon from './PixelIcon.svelte';

  interface Props {
    compact?: boolean;
    class?: string;
  }

  let { compact = false, class: className = '' }: Props = $props();

  onMount(() => {
    motionState.init();
  });

  const label = $derived(
    motionState.systemReduced
      ? uiStrings.motion.systemReducedHint
      : motionState.paused
        ? uiStrings.motion.resumeLabel
        : uiStrings.motion.pauseLabel
  );

  const shortText = $derived(
    motionState.paused ? uiStrings.motion.pausedShort : uiStrings.motion.runningShort
  );
</script>

<button
  type="button"
  class="z-motion-toggle {className}"
  class:is-paused={motionState.paused}
  aria-pressed={motionState.paused}
  aria-label={label}
  title={label}
  disabled={motionState.systemReduced}
  onclick={() => motionState.toggle()}
>
  <PixelIcon name={motionState.paused ? 'play' : 'pause'} size={14} />
  {#if !compact}
    <span class="z-motion-text">{shortText}</span>
  {/if}
</button>

<style>
  .z-motion-toggle {
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

  .z-motion-toggle:hover:not(:disabled) {
    color: var(--z-ink);
    border-color: var(--z-horizon-bright);
  }

  .z-motion-toggle:active:not(:disabled) {
    transform: translateY(2px);
  }

  .z-motion-toggle.is-paused {
    color: var(--z-sun-core);
    border-color: var(--z-sun-rim);
  }

  .z-motion-toggle:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .z-motion-text {
    white-space: nowrap;
  }
</style>
