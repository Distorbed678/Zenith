<script lang="ts">
  import { PixelButton, PixelFrame } from '@zenith/ui';
  import { siteStrings } from '$lib/strings';
</script>

<svelte:head>
  <title>{siteStrings.docs.apiPageHeading} — Zenith</title>
</svelte:head>

<article class="api-page z-prose">
  <h1>{siteStrings.docs.apiPageHeading}</h1>
  <p>{siteStrings.docs.apiPageBody}</p>

  <PixelFrame variant="elevated" padding="md" class="api-box">
    <p class="api-url">
      Published target: <code>/Zenith/api/</code>
    </p>
    <div class="api-actions">
      <PixelButton variant="primary" href="/Zenith/api/">
        {siteStrings.docs.apiOpenButton}
      </PixelButton>
    </div>
  </PixelFrame>

  <h2>Build API docs locally</h2>
  <p>{siteStrings.docs.apiLocalNote}</p>
  <pre class="local-cmd"><code>cargo doc --workspace --no-deps --open</code></pre>
</article>

<style>
  :global(.api-box) {
    margin: var(--z-space-5) 0;
    display: flex;
    flex-direction: column;
    gap: var(--z-space-3);
  }

  .api-url {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
  }

  .api-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-3);
  }

  .local-cmd {
    margin: var(--z-space-3) 0 0;
    padding: var(--z-space-4);
    background-color: var(--z-surface-inset);
    border: var(--z-px) solid var(--z-border);
    font-family: var(--z-font-mono);
    font-size: 0.875rem;
    overflow-x: auto;
  }
</style>
