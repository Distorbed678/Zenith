<script lang="ts">
  import type { LoadedDoc } from '$lib/server/docs';
  import { siteStrings } from '$lib/strings';

  interface Props {
    data: {
      doc: LoadedDoc;
    };
  }

  let { data }: Props = $props();
</script>

<svelte:head>
  <title>{data.doc.title} — Zenith Docs</title>
</svelte:head>

<div class="doc-grid">
  <article class="doc-article">
    <div class="doc-source-line">
      <span>{siteStrings.docs.groups[data.doc.group]}</span>
      <code>{data.doc.repoRelPath}</code>
    </div>

    <div class="z-prose">
      {@html data.doc.html}
    </div>
  </article>

  {#if data.doc.headings.length > 0}
    <aside class="doc-toc" aria-label={siteStrings.docs.tocHeading}>
      <div class="toc-sticky">
        <h2>{siteStrings.docs.tocHeading}</h2>
        <ul class="toc-list">
          {#each data.doc.headings as heading (heading.id)}
            <li class:is-sub={heading.depth === 3}>
              <a href="#{heading.id}">{heading.text}</a>
            </li>
          {/each}
        </ul>
      </div>
    </aside>
  {/if}
</div>

<style>
  .doc-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 13rem;
    gap: var(--z-space-6);
    align-items: start;
  }

  .doc-article {
    min-width: 0;
  }

  .doc-source-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--z-space-3);
    margin-bottom: var(--z-space-3);
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  .doc-source-line code {
    font-family: var(--z-font-mono);
    color: var(--z-ink-muted);
    background-color: var(--z-surface);
    border: 1px solid var(--z-border);
    padding: 0.1rem 0.4rem;
  }

  .doc-toc {
    border-left: 2px solid var(--z-navy-deep);
    padding-left: var(--z-space-4);
  }

  .toc-sticky {
    position: sticky;
    top: 5rem;
    max-height: calc(100dvh - 6rem);
    overflow-y: auto;
  }

  .doc-toc h2 {
    font-size: var(--z-text-sm);
    margin-bottom: var(--z-space-3);
    color: var(--z-ink);
  }

  .toc-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .toc-list li.is-sub {
    padding-left: var(--z-space-3);
  }

  .toc-list a {
    display: block;
    font-size: var(--z-text-xs);
    color: var(--z-ink-muted);
    text-decoration: none;
    line-height: 1.4;
  }

  .toc-list a:hover {
    color: var(--z-link);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  @media (max-width: 1120px) {
    .doc-grid {
      grid-template-columns: 1fr;
    }

    .doc-toc {
      order: -1;
      border-left: 0;
      border: 2px solid var(--z-border);
      background-color: var(--z-surface);
      padding: var(--z-space-3) var(--z-space-4);
      margin-bottom: var(--z-space-4);
    }

    .toc-sticky {
      position: static;
      max-height: none;
    }
  }
</style>
