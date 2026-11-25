<script lang="ts" module>
  export type BadgeStatus = 'done' | 'in-progress' | 'planned' | 'warning' | 'neutral';
</script>

<script lang="ts">
  import { uiStrings } from '../strings';

  interface Props {
    status: BadgeStatus;
    label?: string;
    class?: string;
  }

  let { status, label, class: className = '' }: Props = $props();

  const text = $derived(label ?? uiStrings.status[status]);
</script>

<span class="z-badge z-badge--{status} {className}">
  <span class="z-badge-pip" aria-hidden="true"></span>
  <span>{text}</span>
</span>

<style>
  .z-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.18rem 0.6rem;
    font-family: var(--z-font-body);
    font-size: var(--z-text-xs);
    font-weight: 700;
    line-height: 1.25;
    white-space: nowrap;
    border: 2px solid currentColor;
  }

  .z-badge-pip {
    width: 6px;
    height: 6px;
    background-color: currentColor;
    flex-shrink: 0;
  }

  .z-badge--done {
    color: var(--z-status-done);
    background-color: var(--z-status-done-bg);
  }

  .z-badge--in-progress {
    color: var(--z-status-progress);
    background-color: var(--z-status-progress-bg);
  }

  .z-badge--planned {
    color: var(--z-status-planned);
    background-color: var(--z-status-planned-bg);
  }

  .z-badge--warning {
    color: var(--z-status-warning);
    background-color: var(--z-status-warning-bg);
  }

  .z-badge--neutral {
    color: var(--z-ink-muted);
    background-color: var(--z-surface-elevated);
    border-color: var(--z-border-strong);
  }
</style>
