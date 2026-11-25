<script lang="ts">
  import { base } from '$app/paths';
  import { PixelButton, PixelFrame } from '@zenith/ui';
  import type { DocGroup } from '$lib/server/docs';
  import { siteStrings } from '$lib/strings';

  interface Props {
    data: {
      groups: DocGroup[];
    };
  }

  let { data }: Props = $props();
</script>

<svelte:head>
  <title>{siteStrings.docs.metaTitle}</title>
  <meta name="description" content={siteStrings.docs.metaDescription} />
</svelte:head>

<div class="docs-index">
  <header class="docs-index-head">
    <h1>{siteStrings.docs.heading}</h1>
    <p>{siteStrings.docs.lead}</p>
  </header>

  <div class="group-sections">
    {#each data.groups as group (group.key)}
      <section class="group-block">
        <h2>{group.label}</h2>
        {#if group.items.length === 0}
          <p class="group-empty">{siteStrings.docs.emptyGroup}</p>
        {:else}
          <ul class="doc-list">
            {#each group.items as doc (doc.slug)}
              <li>
                <a href="{base}/docs/{doc.slug}" class="doc-entry">
                  <span class="doc-entry-title">{doc.title}</span>
                  <code class="doc-entry-path">{doc.repoRelPath}</code>
                </a>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/each}
  </div>

  <PixelFrame variant="accent" padding="md" as="section" class="api-callout">
    <div class="api-callout-body">
      <h2>{siteStrings.docs.apiLinkTitle}</h2>
      <p>{siteStrings.docs.apiLinkDescription}</p>
    </div>
    <PixelButton variant="secondary" href="{base}/docs/api">
      Open API reference
    </PixelButton>
  </PixelFrame>
</div>

<style>
  .docs-index {
    max-width: var(--z-prose);
  }

  .docs-index-head {
    padding-bottom: var(--z-space-4);
    border-bottom: var(--z-px) solid var(--z-border);
    margin-bottom: var(--z-space-6);
  }

  .docs-index-head h1 {
    font-size: var(--z-text-2xl);
    margin-bottom: var(--z-space-2);
  }

  .docs-index-head p {
    margin: 0;
    color: var(--z-ink-muted);
  }

  .group-sections {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-6);
    margin-bottom: var(--z-space-7);
  }

  .group-block h2 {
    font-size: var(--z-text-xl);
    margin-bottom: var(--z-space-3);
  }

  .group-empty {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-dim);
  }

  .doc-list {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 2px solid var(--z-navy-deep);
  }

  .doc-list li {
    border-bottom: 2px solid var(--z-navy-deep);
  }

  .doc-entry {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--z-space-2);
    padding: 0.7rem 0.5rem;
    text-decoration: none;
    color: var(--z-ink);
  }

  .doc-entry:hover {
    background-color: var(--z-surface);
  }

  .doc-entry-title {
    font-family: var(--z-font-display);
    font-size: var(--z-text-base);
    font-weight: 600;
    color: var(--z-link);
  }

  .doc-entry-path {
    font-family: var(--z-font-mono);
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  :global(.api-callout) {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: var(--z-space-4);
  }

  .api-callout-body h2 {
    font-size: var(--z-text-lg);
    margin-bottom: 0.25rem;
  }

  .api-callout-body p {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
    max-width: 44ch;
  }
</style>
