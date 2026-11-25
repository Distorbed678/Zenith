<script lang="ts">
  import { base } from '$app/paths';
  import { PixelButton, PixelFrame, PixelSky } from '@zenith/ui';
  import { siteStrings } from '$lib/strings';
</script>

<svelte:head>
  <title>{siteStrings.notFound.metaTitle}</title>
</svelte:head>

<section class="not-found">
  <div class="sky-backdrop" aria-hidden="true">
    <PixelSky mode="ambient" pixelSize={3} />
  </div>

  <div class="not-found-inner">
    <PixelFrame variant="elevated" padding="lg" class="not-found-panel">
      <p class="code z-tabular">{siteStrings.notFound.code}</p>
      <h1>{siteStrings.notFound.heading}</h1>
      <p class="body">{siteStrings.notFound.body}</p>
      <div class="actions">
        <PixelButton variant="primary" href="{base}/">
          {siteStrings.notFound.homeAction}
        </PixelButton>
        <PixelButton variant="secondary" href="{base}/tracker">
          {siteStrings.notFound.trackerAction}
        </PixelButton>
        <PixelButton variant="ghost" href="{base}/docs">
          {siteStrings.notFound.docsAction}
        </PixelButton>
      </div>
    </PixelFrame>
  </div>
</section>

<style>
  .not-found {
    position: relative;
    min-height: 30rem;
    display: flex;
    align-items: center;
    padding: var(--z-space-7) var(--z-space-4);
    overflow: hidden;
  }

  .sky-backdrop {
    position: absolute;
    inset: 0;
    z-index: var(--z-layer-sky);
    opacity: 0.55;
  }

  .not-found-inner {
    position: relative;
    z-index: var(--z-layer-content);
    max-width: 40rem;
    width: 100%;
    margin: 0 auto;
  }

  .code {
    margin: 0 0 var(--z-space-2);
    font-family: var(--z-font-display);
    font-size: clamp(3rem, 8vw, 4.5rem);
    font-weight: 700;
    line-height: 1;
    color: var(--z-sun-core);
  }

  :root[data-theme='day'] .code {
    color: var(--z-accent-shadow);
  }

  h1 {
    font-size: var(--z-text-2xl);
    margin-bottom: var(--z-space-3);
  }

  .body {
    margin: 0 0 var(--z-space-5);
    color: var(--z-ink-muted);
    max-width: 50ch;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-3);
  }
</style>
