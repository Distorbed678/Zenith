<script lang="ts" module>
  export interface TabItem {
    id: string;
    label: string;
    count?: number;
  }
</script>

<script lang="ts">
  interface Props {
    tabs: TabItem[];
    active: string;
    label: string;
    onchange?: (id: string) => void;
    class?: string;
  }

  let {
    tabs,
    active = $bindable(),
    label,
    onchange,
    class: className = ''
  }: Props = $props();

  let tabButtons: Array<HTMLButtonElement | null> = $state([]);

  function selectTab(id: string) {
    active = id;
    onchange?.(id);
  }

  function handleKeydown(event: KeyboardEvent, index: number) {
    let nextIndex: number | null = null;
    if (event.key === 'ArrowRight') {
      nextIndex = (index + 1) % tabs.length;
    } else if (event.key === 'ArrowLeft') {
      nextIndex = (index - 1 + tabs.length) % tabs.length;
    } else if (event.key === 'Home') {
      nextIndex = 0;
    } else if (event.key === 'End') {
      nextIndex = tabs.length - 1;
    }

    if (nextIndex !== null) {
      event.preventDefault();
      const target = tabs[nextIndex];
      if (target) {
        selectTab(target.id);
        tabButtons[nextIndex]?.focus();
      }
    }
  }
</script>

<div class="z-tabs {className}" role="tablist" aria-label={label}>
  {#each tabs as tab, index (tab.id)}
    {@const isSelected = active === tab.id}
    <button
      bind:this={tabButtons[index]}
      type="button"
      role="tab"
      aria-selected={isSelected}
      tabindex={isSelected ? 0 : -1}
      class="z-tab"
      class:is-active={isSelected}
      onclick={() => selectTab(tab.id)}
      onkeydown={(event) => handleKeydown(event, index)}
    >
      <span>{tab.label}</span>
      {#if tab.count !== undefined}
        <span class="z-tab-count z-tabular">{tab.count}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .z-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-2);
    border-bottom: var(--z-px) solid var(--z-border);
    padding-bottom: 0;
  }

  .z-tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--z-space-2);
    padding: 0.5rem 0.9rem;
    margin-bottom: calc(var(--z-px) * -1);
    font-family: var(--z-font-display);
    font-size: var(--z-text-sm);
    font-weight: 600;
    color: var(--z-ink-muted);
    background-color: transparent;
    border: var(--z-px) solid transparent;
    border-bottom: var(--z-px) solid var(--z-border);
    cursor: pointer;
    transition:
      color var(--z-dur-ui) linear,
      background-color var(--z-dur-ui) linear;
  }

  .z-tab:hover {
    color: var(--z-ink);
    background-color: var(--z-surface);
  }

  .z-tab.is-active {
    color: var(--z-ink);
    background-color: var(--z-surface);
    border-color: var(--z-border);
    border-top-color: var(--z-sun-core);
    border-bottom-color: var(--z-surface);
    box-shadow: inset 0 var(--z-px) 0 0 var(--z-sun-core);
  }

  .z-tab-count {
    font-family: var(--z-font-body);
    font-size: var(--z-text-xs);
    font-weight: 700;
    color: var(--z-ink-dim);
    padding: 0.05rem 0.4rem;
    background-color: var(--z-surface-inset);
    border: 1px solid var(--z-border);
  }

  .z-tab.is-active .z-tab-count {
    color: var(--z-sun-core);
    border-color: var(--z-sun-rim);
  }
</style>
