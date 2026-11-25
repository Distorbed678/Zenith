<script lang="ts">
  import { base } from '$app/paths';
  import { PixelButton, PixelFrame } from '@zenith/ui';
  import { siteStrings } from '$lib/strings';

  interface Props {
    data: {
      changelog: {
        exists: boolean;
        html: string;
      };
    };
  }

  let { data }: Props = $props();
</script>

<svelte:head>
  <title>{siteStrings.changelog.metaTitle}</title>
  <meta name="description" content={siteStrings.changelog.metaDescription} />
</svelte:head>

<div class="changelog-page container">
  <header class="page-header">
    <h1>{siteStrings.changelog.heading}</h1>
    <p class="page-lead">{siteStrings.changelog.lead}</p>
  </header>

  {#if data.changelog.exists}
    <article class="z-prose">
      {@html data.changelog.html}
    </article>
  {:else}
    <PixelFrame variant="elevated" padding="lg" class="empty-changelog">
      <h2>{siteStrings.changelog.emptyTitle}</h2>
      <p>{siteStrings.changelog.emptyBody}</p>
      <div class="empty-actions">
        <PixelButton variant="primary" href="{base}/tracker">
          {siteStrings.changelog.emptyActionTracker}
        </PixelButton>
        <PixelButton
          variant="secondary"
          href="{siteStrings.brand.repoUrl}/commits/main"
          external
        >
          {siteStrings.changelog.emptyActionGithub}
        </PixelButton>
      </div>
    </PixelFrame>
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
    margin-bottom: var(--z-space-6);
  }

  .page-header h1 {
    font-size: var(--z-text-2xl);
    margin-bottom: var(--z-space-2);
  }

  .page-lead {
    margin: 0;
    color: var(--z-ink-muted);
    max-width: 58ch;
  }

  :global(.empty-changelog) {
    max-width: 46rem;
  }

  :global(.empty-changelog) h2 {
    font-size: var(--z-text-xl);
    margin-bottom: var(--z-space-3);
  }

  :global(.empty-changelog) p {
    margin: 0 0 var(--z-space-5);
    color: var(--z-ink-muted);
    max-width: 54ch;
  }

  .empty-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-3);
  }
</style>
