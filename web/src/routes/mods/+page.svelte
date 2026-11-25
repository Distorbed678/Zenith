<script lang="ts">
  import {
    PixelButton,
    PixelFrame,
    PixelIcon,
    PixelTooltip,
    StatusBadge
  } from '@zenith/ui';
  import type { ModIndexPayload, ModPermission } from './+page';
  import { siteStrings } from '$lib/strings';

  interface Props {
    data: {
      payload: ModIndexPayload;
    };
  }

  let { data }: Props = $props();

  let query = $state('');
  let selectedTag = $state<string>('ALL');
  let selectedPermissions = $state<ModPermission[]>([]);

  const allPermissions: ModPermission[] = ['network', 'filesystem', 'render'];

  const allTags = $derived.by(() => {
    const set = new Set<string>();
    for (const mod of data.payload.mods) {
      for (const tag of mod.tags) set.add(tag);
    }
    return Array.from(set).sort();
  });

  function togglePermission(perm: ModPermission) {
    if (selectedPermissions.includes(perm)) {
      selectedPermissions = selectedPermissions.filter((p) => p !== perm);
    } else {
      selectedPermissions = [...selectedPermissions, perm];
    }
  }

  function clearFilters() {
    query = '';
    selectedTag = 'ALL';
    selectedPermissions = [];
  }

  const filteredMods = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return data.payload.mods.filter((mod) => {
      if (selectedTag !== 'ALL' && !mod.tags.includes(selectedTag)) {
        return false;
      }
      if (
        selectedPermissions.length > 0 &&
        !selectedPermissions.every((perm) => mod.permissions.includes(perm))
      ) {
        return false;
      }
      if (!q) return true;
      return (
        mod.name.toLowerCase().includes(q) ||
        mod.summary.toLowerCase().includes(q) ||
        mod.author.toLowerCase().includes(q) ||
        mod.tags.some((t) => t.toLowerCase().includes(q))
      );
    });
  });
</script>

<svelte:head>
  <title>{siteStrings.mods.metaTitle}</title>
  <meta name="description" content={siteStrings.mods.metaDescription} />
</svelte:head>

<div class="mods-page container">
  <header class="page-header">
    <h1>{siteStrings.mods.heading}</h1>
    <p class="page-lead">{siteStrings.mods.lead}</p>
  </header>

  <PixelFrame variant="accent" padding="sm" class="milestone-notice">
    <div class="notice-row">
      <StatusBadge status="planned" label="Milestone M25" />
      <p>{siteStrings.mods.milestoneNotice}</p>
    </div>
  </PixelFrame>

  <!-- Search & filters -->
  <PixelFrame variant="inset" padding="md" as="section" class="filter-panel">
    <div class="search-row">
      <label class="search-label" for="mod-search">{siteStrings.mods.searchLabel}</label>
      <div class="search-input-wrap">
        <PixelIcon name="search" size={16} />
        <input
          id="mod-search"
          type="search"
          bind:value={query}
          placeholder={siteStrings.mods.searchPlaceholder}
          class="search-input"
        />
      </div>
    </div>

    <div class="chip-groups">
      <div class="chip-group">
        <span class="chip-group-label">{siteStrings.mods.tagsFilterLabel}</span>
        <div class="chip-row">
          <button
            type="button"
            class="filter-chip"
            class:is-active={selectedTag === 'ALL'}
            aria-pressed={selectedTag === 'ALL'}
            onclick={() => (selectedTag = 'ALL')}
          >
            {siteStrings.mods.allTagsLabel}
          </button>
          {#each allTags as tag (tag)}
            <button
              type="button"
              class="filter-chip"
              class:is-active={selectedTag === tag}
              aria-pressed={selectedTag === tag}
              onclick={() => (selectedTag = selectedTag === tag ? 'ALL' : tag)}
            >
              {tag}
            </button>
          {/each}
        </div>
      </div>

      <div class="chip-group">
        <span class="chip-group-label">{siteStrings.mods.permissionsFilterLabel}</span>
        <div class="chip-row">
          {#each allPermissions as perm (perm)}
            {@const active = selectedPermissions.includes(perm)}
            <button
              type="button"
              class="filter-chip perm-filter"
              class:is-active={active}
              aria-pressed={active}
              onclick={() => togglePermission(perm)}
            >
              {siteStrings.mods.permissionLabels[perm]}
            </button>
          {/each}
        </div>
      </div>
    </div>
  </PixelFrame>

  <!-- Mod results -->
  {#if filteredMods.length === 0}
    <PixelFrame variant="elevated" padding="md" class="empty-mods">
      <p>{siteStrings.mods.emptyResults}</p>
      <PixelButton variant="secondary" size="sm" onclick={clearFilters}>
        {siteStrings.mods.clearFilters}
      </PixelButton>
    </PixelFrame>
  {:else}
    <div class="mod-list">
      {#each filteredMods as mod (mod.id)}
        <PixelFrame variant="elevated" padding="md" as="article" class="mod-entry">
          <div class="mod-main">
            <div class="mod-top">
              <h2>{mod.name}</h2>
              {#if mod.example}
                <StatusBadge status="warning" label={siteStrings.mods.exampleBadge} />
              {/if}
            </div>

            <p class="mod-meta z-tabular">
              <span>Version {mod.version}</span>
              <span>Target {mod.targetVersion}</span>
              <span>By {mod.author}</span>
            </p>

            <p class="mod-summary">{mod.summary}</p>

            <div class="mod-chips">
              <div class="chip-inline">
                {#each mod.tags as tag (tag)}
                  <span class="tag-pill">{tag}</span>
                {/each}
              </div>

              <div class="chip-inline" aria-label={siteStrings.mods.permissionsHeading}>
                {#each mod.permissions as perm (perm)}
                  <PixelTooltip text={siteStrings.mods.permissionDescriptions[perm]}>
                    <span class="perm-pill perm-pill--{perm}">
                      <PixelIcon name="lock" size={12} />
                      <span>{siteStrings.mods.permissionLabels[perm]}</span>
                    </span>
                  </PixelTooltip>
                {/each}
              </div>
            </div>
          </div>

          <div class="mod-action">
            <PixelButton variant="primary" size="sm" href="zenith://install/{mod.id}">
              <PixelIcon name="launcher" size={14} />
              <span>{siteStrings.mods.openInLauncher}</span>
            </PixelButton>
            <code class="deep-link">zenith://install/{mod.id}</code>
          </div>
        </PixelFrame>
      {/each}
    </div>
  {/if}
</div>

<style>
  .container {
    max-width: var(--z-container);
    margin: 0 auto;
    padding: var(--z-space-6) var(--z-space-4) 0;
  }

  .page-header {
    padding-bottom: var(--z-space-5);
    border-bottom: var(--z-px) solid var(--z-border);
    margin-bottom: var(--z-space-5);
  }

  .page-header h1 {
    font-size: var(--z-text-2xl);
    margin-bottom: var(--z-space-2);
  }

  .page-lead {
    margin: 0;
    max-width: 60ch;
    color: var(--z-ink-muted);
  }

  :global(.milestone-notice) {
    margin-bottom: var(--z-space-5);
  }

  .notice-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--z-space-3);
  }

  .notice-row p {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink);
  }

  :global(.filter-panel) {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-4);
    margin-bottom: var(--z-space-6);
  }

  .search-row {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .search-label,
  .chip-group-label {
    font-size: var(--z-text-sm);
    font-weight: 700;
    color: var(--z-ink-muted);
  }

  .search-input-wrap {
    display: flex;
    align-items: center;
    gap: var(--z-space-2);
    padding: 0.45rem 0.75rem;
    background-color: var(--z-surface);
    border: var(--z-px) solid var(--z-border);
    color: var(--z-ink-dim);
  }

  .search-input-wrap:focus-within {
    border-color: var(--z-horizon-bright);
  }

  .search-input {
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--z-ink);
    font-family: var(--z-font-body);
    font-size: var(--z-text-base);
    outline: none;
  }

  .chip-groups {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-5);
  }

  .chip-group {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .chip-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.45rem;
  }

  .filter-chip {
    padding: 0.25rem 0.65rem;
    font-family: var(--z-font-body);
    font-size: var(--z-text-xs);
    font-weight: 700;
    color: var(--z-ink-muted);
    background-color: var(--z-surface);
    border: 2px solid var(--z-border);
    cursor: pointer;
  }

  .filter-chip:hover {
    color: var(--z-ink);
    border-color: var(--z-horizon-bright);
  }

  .filter-chip.is-active {
    color: #0b1020;
    background-color: var(--z-sun-core);
    border-color: var(--z-sun-rim);
  }

  .perm-filter.is-active {
    color: #0b1020;
    background-color: var(--z-horizon-bright);
    border-color: var(--z-horizon-mid);
  }

  .mod-list {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-4);
  }

  :global(.mod-entry) {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: var(--z-space-5);
    align-items: center;
  }

  .mod-top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--z-space-3);
    margin-bottom: 0.3rem;
  }

  .mod-top h2 {
    font-size: var(--z-text-xl);
  }

  .mod-meta {
    margin: 0 0 var(--z-space-3);
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-4);
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  .mod-summary {
    margin: 0 0 var(--z-space-4);
    color: var(--z-ink-muted);
    max-width: 60ch;
  }

  .mod-chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--z-space-4);
  }

  .chip-inline {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .tag-pill {
    padding: 0.15rem 0.5rem;
    font-size: var(--z-text-xs);
    color: var(--z-ink-muted);
    background-color: var(--z-surface-inset);
    border: 1px solid var(--z-border);
  }

  .perm-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.15rem 0.55rem;
    font-size: var(--z-text-xs);
    font-weight: 700;
    border: 2px solid currentColor;
  }

  .perm-pill--network {
    color: var(--z-status-warning);
    background-color: var(--z-status-warning-bg);
  }

  .perm-pill--filesystem {
    color: var(--z-status-progress);
    background-color: var(--z-status-progress-bg);
  }

  .perm-pill--render {
    color: var(--z-status-done);
    background-color: var(--z-status-done-bg);
  }

  .mod-action {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 0.45rem;
  }

  .deep-link {
    font-family: var(--z-font-mono);
    font-size: 0.75rem;
    color: var(--z-ink-dim);
  }

  :global(.empty-mods) {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--z-space-3);
  }

  :global(.empty-mods) p {
    margin: 0;
    color: var(--z-ink-muted);
  }

  @media (max-width: 820px) {
    :global(.mod-entry) {
      grid-template-columns: 1fr;
    }

    .mod-action {
      align-items: flex-start;
    }
  }
</style>
