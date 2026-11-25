<script lang="ts">
  import { base } from '$app/paths';
  import {
    PixelButton,
    PixelIcon,
    PixelProgress,
    PixelSky,
    PixelWordmark,
    SunLogo
  } from '@zenith/ui';
  import { progressData } from '$lib/data/progress';
  import { siteStrings } from '$lib/strings';
</script>

<svelte:head>
  <title>{siteStrings.home.metaTitle}</title>
  <meta name="description" content={siteStrings.home.metaDescription} />
</svelte:head>

<!-- Hero: open pixel sky stage with single sunrise-to-zenith moment -->
<section class="hero">
  <div class="hero-sky" aria-hidden="true">
    <PixelSky mode="hero" pixelSize={4} showHills />
  </div>

  <div class="hero-inner">
    <div class="hero-copy">
      <div class="hero-mark">
        <SunLogo size={52} decorative />
        <h1 class="hero-title"><PixelWordmark height={63} label={siteStrings.brand.name} /></h1>
      </div>
      <p class="hero-tagline">{siteStrings.brand.tagline}</p>
      <p class="hero-subline">{siteStrings.brand.subline}</p>

      <div class="hero-actions">
        <PixelButton variant="primary" size="md" href="{base}/tracker">
          {siteStrings.home.ctaTracker}
        </PixelButton>
        <PixelButton variant="secondary" size="md" href="{base}/docs">
          {siteStrings.home.ctaDocs}
        </PixelButton>
        <PixelButton
          variant="ghost"
          size="md"
          href={siteStrings.brand.repoUrl}
          external
        >
          {siteStrings.home.ctaGithub}
        </PixelButton>
      </div>
    </div>
  </div>
</section>

<!-- Horizon sill: live overall progress anchored directly below the stepped hills -->
<section class="horizon-sill" aria-label={siteStrings.home.horizonOverallLabel}>
  <div class="container sill-grid">
    <div class="sill-meter">
      <PixelProgress
        value={progressData.overall.percent}
        segments={28}
        tone="gold"
        size="md"
        label={siteStrings.home.horizonOverallLabel}
        showValue
      />
    </div>

    <dl class="sill-facts z-tabular">
      <div class="sill-fact">
        <dt>{siteStrings.home.horizonParityLabel}</dt>
        <dd>{progressData.overall.suitesPassing} / {progressData.overall.suitesTotal}</dd>
      </div>
      <div class="sill-fact">
        <dt>{siteStrings.home.horizonTargetLabel}</dt>
        <dd>{progressData.targetVersion}</dd>
      </div>
      <div class="sill-fact">
        <dt>{siteStrings.home.horizonProtocolLabel}</dt>
        <dd>{progressData.protocol}</dd>
      </div>
    </dl>
  </div>
</section>

<!-- What makes Zenith different: asymmetric open editorial ledger (not identical cards) -->
<section class="section container">
  <div class="editorial-split">
    <div class="editorial-lead">
      <h2>{siteStrings.home.differentHeading}</h2>
      <p>{siteStrings.home.differentLead}</p>
    </div>

    <ul class="pillar-list">
      {#each siteStrings.home.pillars as pillar (pillar.id)}
        <li class="pillar-item">
          <div class="pillar-icon">
            <PixelIcon name={pillar.icon} size={20} />
          </div>
          <div class="pillar-body">
            <h3>{pillar.title}</h3>
            <p>{pillar.body}</p>
          </div>
        </li>
      {/each}
    </ul>
  </div>
</section>

<!-- Roadmap P0-P7: true sequential phases -->
<section class="section container roadmap-section">
  <div class="roadmap-header">
    <div>
      <h2>{siteStrings.home.roadmapHeading}</h2>
      <p class="section-lead">{siteStrings.home.roadmapLead}</p>
    </div>
    <PixelButton variant="secondary" href="{base}/tracker">
      {siteStrings.home.roadmapAction}
    </PixelButton>
  </div>

  <ol class="roadmap-list">
    {#each progressData.phases as phase (phase.id)}
      <li class="roadmap-row">
        <a href="{base}/tracker?phase={phase.id}" class="roadmap-link">
          <span class="phase-code">{phase.id}</span>
          <div class="phase-info">
            <span class="phase-title">{phase.title}</span>
            <span class="phase-blurb">{phase.blurb}</span>
          </div>
          <div class="phase-progress">
            <PixelProgress
              value={phase.percent}
              segments={12}
              tone={phase.percent >= 100 ? 'teal' : 'gold'}
              size="sm"
            />
          </div>
          <div class="phase-numbers z-tabular">
            <span class="phase-pct">{phase.percent.toFixed(0)}%</span>
            <span class="phase-count">
              {phase.milestones.length}
              {phase.milestones.length === 1 ? 'milestone' : 'milestones'}
            </span>
          </div>
        </a>
      </li>
    {/each}
  </ol>
</section>

<style>
  .container {
    max-width: var(--z-container);
    margin: 0 auto;
    padding-inline: var(--z-space-4);
  }

  /* Hero */
  .hero {
    position: relative;
    min-height: clamp(29rem, 58vh, 36rem);
    display: flex;
    align-items: center;
    overflow: hidden;
    background-color: var(--z-sky-deep);
  }

  .hero-sky {
    position: absolute;
    inset: 0;
    z-index: var(--z-layer-sky);
  }

  .hero-inner {
    position: relative;
    z-index: var(--z-layer-content);
    width: 100%;
    max-width: var(--z-container);
    margin: 0 auto;
    padding: var(--z-space-7) var(--z-space-4) clamp(5.5rem, 12vh, 7.5rem);
  }

  .hero-copy {
    max-width: 39rem;
    padding: var(--z-space-4) var(--z-space-5) var(--z-space-4) 0;
    text-shadow: 0 2px 0 rgba(11, 16, 32, 0.9);
  }

  :root[data-theme='day'] .hero-copy {
    text-shadow: 0 1px 0 rgba(234, 245, 250, 0.85);
  }

  .hero-mark {
    display: flex;
    align-items: center;
    gap: var(--z-space-3);
    margin-bottom: var(--z-space-3);
  }

  .hero-title {
    font-size: var(--z-text-3xl);
    line-height: 1;
    color: var(--z-ink);
  }

  .hero-tagline {
    margin: 0 0 var(--z-space-3);
    font-family: var(--z-font-display);
    font-size: clamp(1.3rem, 2.5vw, 1.7rem);
    line-height: 1.25;
    color: var(--z-arc);
  }

  :root[data-theme='day'] .hero-tagline {
    color: var(--z-navy-deep);
  }

  .hero-subline {
    margin: 0 0 var(--z-space-5);
    color: var(--z-ink);
    opacity: 0.88;
    max-width: 52ch;
  }

  .hero-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-3);
  }

  /* Horizon sill */
  .horizon-sill {
    background-color: var(--z-surface);
    border-top: var(--z-px) solid var(--z-horizon-mid);
    border-bottom: var(--z-px) solid var(--z-border);
    padding: var(--z-space-4) 0;
  }

  .sill-grid {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr);
    gap: var(--z-space-5);
    align-items: center;
  }

  .sill-facts {
    margin: 0;
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--z-space-3);
  }

  .sill-fact {
    padding-left: var(--z-space-3);
    border-left: var(--z-px) solid var(--z-border);
  }

  .sill-fact dt {
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
    margin-bottom: 0.15rem;
  }

  .sill-fact dd {
    margin: 0;
    font-family: var(--z-font-body);
    font-size: var(--z-text-lg);
    font-weight: 700;
    color: var(--z-ink);
  }

  /* Sections */
  .section {
    padding-top: var(--z-space-8);
  }

  .section h2 {
    font-size: var(--z-text-2xl);
    line-height: 1.15;
    margin-bottom: var(--z-space-3);
  }

  .section-lead {
    margin: 0;
    max-width: 58ch;
    color: var(--z-ink-muted);
  }

  /* Asymmetric editorial ledger */
  .editorial-split {
    display: grid;
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.5fr);
    gap: var(--z-space-7);
    align-items: start;
  }

  .editorial-lead p {
    margin: 0;
    color: var(--z-ink-muted);
    max-width: 42ch;
  }

  .pillar-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--z-space-5) var(--z-space-6);
  }

  .pillar-item {
    display: flex;
    align-items: flex-start;
    gap: var(--z-space-3);
    padding-bottom: var(--z-space-5);
    border-bottom: var(--z-px) solid var(--z-navy-deep);
  }

  .pillar-item:last-child {
    grid-column: 1 / -1;
    border-bottom-color: var(--z-horizon-mid);
  }

  .pillar-icon {
    flex-shrink: 0;
    width: 2.25rem;
    height: 2.25rem;
    display: grid;
    place-items: center;
    background-color: var(--z-surface);
    border: 2px solid var(--z-border);
    color: var(--z-horizon-bright);
  }

  :root[data-theme='day'] .pillar-icon {
    color: var(--z-link);
  }

  .pillar-body h3 {
    font-size: var(--z-text-lg);
    margin-bottom: 0.35rem;
  }

  .pillar-body p {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
    line-height: 1.6;
  }

  /* Roadmap sequence */
  .roadmap-header {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: flex-end;
    gap: var(--z-space-4);
    margin-bottom: var(--z-space-5);
  }

  .roadmap-list {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: var(--z-px) solid var(--z-border);
  }

  .roadmap-row {
    border-bottom: 2px solid var(--z-navy-deep);
  }

  .roadmap-link {
    display: grid;
    grid-template-columns: 3.5rem minmax(0, 1.8fr) minmax(8rem, 0.9fr) 7.5rem;
    gap: var(--z-space-4);
    align-items: center;
    padding: var(--z-space-4) var(--z-space-3);
    text-decoration: none;
    color: var(--z-ink);
    transition: background-color var(--z-dur-ui) linear;
  }

  .roadmap-link:hover {
    background-color: var(--z-surface);
  }

  .phase-code {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 3rem;
    height: 2.25rem;
    font-family: var(--z-font-display);
    font-size: var(--z-text-base);
    font-weight: 700;
    background-color: var(--z-surface-elevated);
    border: 2px solid var(--z-border-strong);
    color: var(--z-sun-core);
  }

  :root[data-theme='day'] .phase-code {
    color: var(--z-accent-shadow);
  }

  .phase-info {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    min-width: 0;
  }

  .phase-title {
    font-family: var(--z-font-display);
    font-size: var(--z-text-lg);
    font-weight: 600;
    color: var(--z-ink);
  }

  .phase-blurb {
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
  }

  .phase-numbers {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 0.1rem;
  }

  .phase-pct {
    font-family: var(--z-font-display);
    font-size: var(--z-text-lg);
    font-weight: 700;
    color: var(--z-ink);
  }

  .phase-count {
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  @media (max-width: 960px) {
    .sill-grid,
    .editorial-split {
      grid-template-columns: 1fr;
    }

    .roadmap-link {
      grid-template-columns: 3.25rem minmax(0, 1fr);
      gap: var(--z-space-3);
    }

    .phase-progress {
      grid-column: 2;
    }

    .phase-numbers {
      grid-column: 2;
      flex-direction: row;
      justify-content: space-between;
      align-items: baseline;
    }
  }

  @media (max-width: 900px) {
    .hero {
      align-items: flex-end;
      min-height: 33rem;
    }

    .hero-inner {
      padding: 11.5rem var(--z-space-4) 5rem;
    }

    .hero-copy {
      padding: var(--z-space-4);
      background-color: rgba(11, 16, 32, 0.82);
      border: var(--z-px) solid var(--z-border);
    }

    :root[data-theme='day'] .hero-copy {
      background-color: rgba(234, 245, 250, 0.88);
    }
  }

  @media (max-width: 640px) {
    .hero-copy {
      padding: var(--z-space-4);
    }

    .sill-facts {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--z-space-3);
    }

    .sill-fact {
      padding-left: var(--z-space-2);
    }

    .pillar-list {
      grid-template-columns: 1fr;
    }

    .pillar-item:last-child {
      grid-column: auto;
    }
  }
</style>
