<script lang="ts">
  import { base } from '$app/paths';
  import { SunLogo } from '@zenith/ui';
  import { siteStrings } from '$lib/strings';
</script>

<footer class="site-footer">
  <div class="footer-horizon" aria-hidden="true"></div>
  <div class="footer-inner">
    <div class="footer-legal">
      <div class="footer-brand">
        <SunLogo size={24} decorative />
        <span class="footer-name">{siteStrings.brand.name}</span>
      </div>
      <p class="trademark">{siteStrings.brand.trademarkNotice}</p>
      <p class="license">
        <span>{siteStrings.footer.licenseLabel}:</span>
        <strong>{siteStrings.brand.license}</strong>
      </p>
    </div>

    <div class="footer-columns">
      <div class="footer-col">
        <h2 class="footer-heading">{siteStrings.footer.linksHeading}</h2>
        <ul class="footer-list">
          <li><a href="{base}/tracker">Progress tracker</a></li>
          <li><a href="{base}/docs">Documentation</a></li>
          <li><a href="{base}/docs/api">{siteStrings.footer.apiDocsLabel}</a></li>
          <li><a href="{base}/mods">Mod index</a></li>
        </ul>
      </div>

      <div class="footer-col">
        <h2 class="footer-heading">{siteStrings.footer.projectHeading}</h2>
        <ul class="footer-list">
          <li><a href="{base}/downloads">Downloads</a></li>
          <li><a href="{base}/changelog">Changelog</a></li>
          <li>
            <a href={siteStrings.brand.repoUrl} target="_blank" rel="noopener noreferrer">
              {siteStrings.footer.githubLabel}
            </a>
          </li>
        </ul>
      </div>
    </div>
  </div>
</footer>

<style>
  .site-footer {
    margin-top: var(--z-space-8);
    background-color: var(--z-surface);
    border-top: var(--z-px) solid var(--z-border);
  }

  .footer-horizon {
    height: var(--z-px);
    background-image: repeating-linear-gradient(
      to right,
      var(--z-horizon-bright) 0 12px,
      var(--z-horizon-mid) 12px 24px,
      var(--z-navy-mid) 24px 48px
    );
  }

  .footer-inner {
    max-width: var(--z-container);
    margin: 0 auto;
    padding: var(--z-space-6) var(--z-space-4);
    display: grid;
    grid-template-columns: minmax(0, 1.6fr) minmax(0, 1fr);
    gap: var(--z-space-6);
  }

  .footer-brand {
    display: inline-flex;
    align-items: center;
    gap: var(--z-space-2);
    margin-bottom: var(--z-space-3);
  }

  .footer-name {
    font-family: var(--z-font-display);
    font-size: var(--z-text-lg);
    font-weight: 700;
  }

  .trademark {
    margin: 0 0 var(--z-space-3);
    max-width: 56ch;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
    line-height: 1.55;
  }

  .license {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-dim);
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
  }

  .license strong {
    color: var(--z-ink);
    font-weight: 700;
  }

  .footer-columns {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--z-space-5);
  }

  .footer-heading {
    font-size: var(--z-text-base);
    margin-bottom: var(--z-space-3);
    color: var(--z-ink);
  }

  .footer-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .footer-list a {
    color: var(--z-ink-muted);
    font-size: var(--z-text-sm);
    text-decoration: none;
  }

  .footer-list a:hover {
    color: var(--z-link);
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  @media (max-width: 768px) {
    .footer-inner {
      grid-template-columns: 1fr;
      gap: var(--z-space-5);
    }
  }
</style>
