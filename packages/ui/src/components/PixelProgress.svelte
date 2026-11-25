<script lang="ts">
  interface Props {
    value: number;
    max?: number;
    segments?: number;
    tone?: 'gold' | 'teal';
    size?: 'sm' | 'md' | 'lg';
    label?: string;
    showValue?: boolean;
    class?: string;
  }

  let {
    value,
    max = 100,
    segments = 20,
    tone = 'gold',
    size = 'md',
    label,
    showValue = false,
    class: className = ''
  }: Props = $props();

  const clampedPct = $derived.by(() => {
    if (max <= 0) return 0;
    const ratio = (value / max) * 100;
    return Math.max(0, Math.min(100, ratio));
  });

  const filledSegments = $derived.by(() => {
    if (clampedPct <= 0) return 0;
    if (clampedPct >= 100) return segments;
    return Math.max(1, Math.round((clampedPct / 100) * segments));
  });

  const formattedPct = $derived(`${clampedPct.toFixed(1)}%`);
</script>

<div class="z-progress-wrap {className}">
  {#if label || showValue}
    <div class="z-progress-header">
      {#if label}
        <span class="z-progress-label">{label}</span>
      {/if}
      {#if showValue}
        <span class="z-progress-value z-tabular">{formattedPct}</span>
      {/if}
    </div>
  {/if}

  <div
    class="z-progress z-progress--{tone} z-progress--{size}"
    role="progressbar"
    aria-label={label ?? 'Progress'}
    aria-valuemin={0}
    aria-valuemax={max}
    aria-valuenow={Number(value.toFixed(1))}
    aria-valuetext={formattedPct}
    style:--z-segments={segments}
  >
    {#each Array.from({ length: segments }) as _, i (i)}
      <span class="z-progress-seg" class:is-filled={i < filledSegments}></span>
    {/each}
  </div>
</div>

<style>
  .z-progress-wrap {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    width: 100%;
  }

  .z-progress-header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--z-space-3);
    font-size: var(--z-text-sm);
  }

  .z-progress-label {
    color: var(--z-ink-muted);
  }

  .z-progress-value {
    color: var(--z-ink);
    font-weight: 700;
  }

  .z-progress {
    display: grid;
    grid-template-columns: repeat(var(--z-segments, 20), minmax(0, 1fr));
    gap: var(--z-px);
    padding: var(--z-px);
    background-color: var(--z-surface-inset);
    border: var(--z-px) solid var(--z-border);
    box-shadow: inset 0 var(--z-px) 0 0 var(--z-bevel-dark);
  }

  .z-progress--sm {
    height: 18px;
  }

  .z-progress--md {
    height: 24px;
  }

  .z-progress--lg {
    height: 30px;
  }

  .z-progress-seg {
    background-color: var(--z-surface);
    opacity: 0.45;
  }

  .z-progress-seg.is-filled {
    opacity: 1;
  }

  .z-progress--gold .z-progress-seg.is-filled {
    background-color: var(--z-sun-core);
    box-shadow:
      inset 0 var(--z-px) 0 0 #fff1a6,
      inset 0 calc(var(--z-px) * -1) 0 0 var(--z-sun-rim);
  }

  .z-progress--teal .z-progress-seg.is-filled {
    background-color: var(--z-horizon-bright);
    box-shadow:
      inset 0 var(--z-px) 0 0 #b7fbf2,
      inset 0 calc(var(--z-px) * -1) 0 0 var(--z-horizon-mid);
  }
</style>
