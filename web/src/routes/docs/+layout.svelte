<script lang="ts">
  import type { Snippet } from 'svelte';
  import { page } from '$app/state';
  import { base } from '$app/paths';
  import { PixelIcon } from '@zenith/ui';
  import type { DocGroup } from '$lib/server/docs';
  import { siteStrings } from '$lib/strings';

  interface Props {
    data: {
      groups: DocGroup[];
    };
    children: Snippet;
  }

  let { data, children }: Props = $props();
  let mobileSidebarOpen = $state(false);

  function isDocActive(targetPath: string): boolean {
    const current = page.url.pathname;
    const expected = `${base}${targetPath}`;
    return current === expected || current === `${expected}/`;
  }
</script>

<div class="docs-shell container">
  <div class="mobile-bar">
    <button
      type="button"
      class="sidebar-toggle"
      aria-expanded={mobileSidebarOpen}
      aria-controls="docs-sidebar-nav"
      onclick={() => (mobileSidebarOpen = !mobileSidebarOpen)}
    >
      <PixelIcon name={mobileSidebarOpen ? 'chevron-down' : 'chevron-right'} size={14} />
      <span>{siteStrings.docs.sidebarLabel}</span>
    </button>
  </div>

  <aside
    id="docs-sidebar-nav"
    class="docs-sidebar"
    class:is-open={mobileSidebarOpen}
    aria-label={siteStrings.docs.sidebarLabel}
  >
    <div class="sidebar-sticky">
      <a
        href="{base}/docs"
        class="sidebar-overview"
        class:is-active={isDocActive('/docs')}
        onclick={() => (mobileSidebarOpen = false)}
      >
        <PixelIcon name="book" size={14} />
        <span>{siteStrings.docs.heading}</span>
      </a>

      {#each data.groups as group (group.key)}
        <div class="sidebar-group">
          <h2 class="sidebar-group-title">{group.label}</h2>
          {#if group.items.length === 0}
            <p class="sidebar-empty">None yet</p>
          {:else}
            <ul class="sidebar-list">
              {#each group.items as item (item.slug)}
                {@const active = isDocActive(`/docs/${item.slug}`)}
                <li>
                  <a
                    href="{base}/docs/{item.slug}"
                    class="sidebar-link"
                    class:is-active={active}
                    aria-current={active ? 'page' : undefined}
                    onclick={() => (mobileSidebarOpen = false)}
                  >
                    {item.title}
                  </a>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/each}

      <div class="sidebar-group">
        <h2 class="sidebar-group-title">{siteStrings.docs.apiLinkTitle}</h2>
        <ul class="sidebar-list">
          <li>
            <a
              href="{base}/docs/api"
              class="sidebar-link"
              class:is-active={isDocActive('/docs/api')}
              aria-current={isDocActive('/docs/api') ? 'page' : undefined}
              onclick={() => (mobileSidebarOpen = false)}
            >
              Rust crate docs (rustdoc)
            </a>
          </li>
        </ul>
      </div>
    </div>
  </aside>

  <div class="docs-main">
    {@render children()}
  </div>
</div>

<style>
  .container {
    max-width: var(--z-container);
    margin: 0 auto;
    padding: var(--z-space-6) var(--z-space-4) 0;
  }

  .docs-shell {
    display: grid;
    grid-template-columns: 15.5rem minmax(0, 1fr);
    gap: var(--z-space-6);
    align-items: start;
  }

  .mobile-bar {
    display: none;
  }

  .sidebar-toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--z-space-2);
    width: 100%;
    padding: 0.55rem 0.85rem;
    font-family: var(--z-font-display);
    font-size: var(--z-text-sm);
    font-weight: 600;
    color: var(--z-ink);
    background-color: var(--z-surface);
    border: var(--z-px) solid var(--z-border);
    cursor: pointer;
  }

  .docs-sidebar {
    border-right: var(--z-px) solid var(--z-navy-deep);
    padding-right: var(--z-space-4);
  }

  .sidebar-sticky {
    position: sticky;
    top: 5rem;
    max-height: calc(100dvh - 6rem);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--z-space-4);
  }

  .sidebar-overview {
    display: inline-flex;
    align-items: center;
    gap: var(--z-space-2);
    padding: 0.45rem 0.6rem;
    font-family: var(--z-font-display);
    font-size: var(--z-text-base);
    font-weight: 700;
    color: var(--z-ink);
    text-decoration: none;
    border-left: var(--z-px) solid transparent;
  }

  .sidebar-overview:hover {
    background-color: var(--z-surface);
  }

  .sidebar-overview.is-active {
    background-color: var(--z-surface);
    border-left-color: var(--z-sun-core);
    color: var(--z-sun-core);
  }

  :root[data-theme='day'] .sidebar-overview.is-active {
    color: var(--z-accent-shadow);
  }

  .sidebar-group-title {
    font-size: var(--z-text-sm);
    color: var(--z-ink);
    margin-bottom: 0.35rem;
    padding-left: 0.6rem;
  }

  .sidebar-empty {
    margin: 0;
    padding-left: 0.6rem;
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  .sidebar-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .sidebar-link {
    display: block;
    padding: 0.35rem 0.6rem;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
    text-decoration: none;
    border-left: var(--z-px) solid transparent;
    line-height: 1.35;
  }

  .sidebar-link:hover {
    color: var(--z-ink);
    background-color: var(--z-surface);
  }

  .sidebar-link.is-active {
    color: var(--z-link);
    background-color: var(--z-surface);
    border-left-color: var(--z-horizon-bright);
    font-weight: 700;
  }

  .docs-main {
    min-width: 0;
  }

  @media (max-width: 900px) {
    .docs-shell {
      grid-template-columns: 1fr;
      gap: var(--z-space-4);
    }

    .mobile-bar {
      display: block;
    }

    .docs-sidebar {
      display: none;
      border-right: 0;
      border: var(--z-px) solid var(--z-border);
      background-color: var(--z-surface);
      padding: var(--z-space-4);
    }

    .docs-sidebar.is-open {
      display: block;
    }

    .sidebar-sticky {
      position: static;
      max-height: none;
    }
  }
</style>
