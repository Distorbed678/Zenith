<script lang="ts">
  import { page } from '$app/state';
  import { base } from '$app/paths';
  import { SunLogo, PixelWordmark, MotionToggle, ThemeToggle, PixelIcon } from '@zenith/ui';
  import { siteStrings } from '$lib/strings';

  let mobileOpen = $state(false);

  function routeHref(path: string): string {
    return path === '/' ? `${base}/` : `${base}${path}`;
  }

  function isCurrent(path: string): boolean {
    const currentPath = page.url.pathname;
    const normalizedBase = base || '';
    const stripped = normalizedBase && currentPath.startsWith(normalizedBase)
      ? currentPath.slice(normalizedBase.length) || '/'
      : currentPath;
    if (path === '/') {
      return stripped === '/' || stripped === '';
    }
    return stripped === path || stripped.startsWith(`${path}/`);
  }

  function closeMobile() {
    mobileOpen = false;
  }
</script>

<header class="site-header">
  <div class="header-inner">
    <a href="{base}/" class="brand-link" onclick={closeMobile}>
      <SunLogo size={32} decorative />
      <span class="brand-name"><PixelWordmark height={21} label={siteStrings.brand.name} /></span>
    </a>

    <nav class="desktop-nav" aria-label={siteStrings.nav.primaryLabel}>
      {#each siteStrings.nav.items as item (item.id)}
        <a
          href={routeHref(item.path)}
          class="nav-link"
          class:is-active={isCurrent(item.path)}
          aria-current={isCurrent(item.path) ? 'page' : undefined}
        >
          {item.label}
        </a>
      {/each}
    </nav>

    <div class="header-controls">
      <div class="desktop-toggles">
        <MotionToggle />
        <ThemeToggle compact />
      </div>
      <div class="mobile-toggles">
        <MotionToggle compact />
      </div>
      <button
        type="button"
        class="mobile-menu-btn"
        aria-expanded={mobileOpen}
        aria-controls="mobile-nav-drawer"
        aria-label={mobileOpen ? siteStrings.nav.closeMenu : siteStrings.nav.openMenu}
        onclick={() => (mobileOpen = !mobileOpen)}
      >
        <PixelIcon name={mobileOpen ? 'close' : 'menu'} size={18} />
      </button>
    </div>
  </div>

  {#if mobileOpen}
    <div id="mobile-nav-drawer" class="mobile-drawer">
      <nav class="mobile-nav" aria-label={siteStrings.nav.primaryLabel}>
        {#each siteStrings.nav.items as item (item.id)}
          <a
            href={routeHref(item.path)}
            class="mobile-nav-link"
            class:is-active={isCurrent(item.path)}
            aria-current={isCurrent(item.path) ? 'page' : undefined}
            onclick={closeMobile}
          >
            {item.label}
          </a>
        {/each}
      </nav>
      <div class="mobile-controls">
        <MotionToggle />
        <ThemeToggle />
      </div>
    </div>
  {/if}
</header>

<style>
  .site-header {
    position: sticky;
    top: 0;
    z-index: var(--z-layer-header);
    background-color: var(--z-surface-header);
    border-bottom: var(--z-px) solid var(--z-border);
  }

  .header-inner {
    max-width: var(--z-container);
    margin: 0 auto;
    padding: 0.65rem var(--z-space-4);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--z-space-4);
  }

  .brand-link {
    display: inline-flex;
    align-items: center;
    gap: 0.65rem;
    text-decoration: none;
    color: var(--z-ink);
  }

  .brand-name {
    font-family: var(--z-font-display);
    font-size: 1.45rem;
    font-weight: 700;
    letter-spacing: 0.02em;
    line-height: 1;
  }

  .desktop-nav {
    display: flex;
    align-items: center;
    gap: var(--z-space-1);
  }

  .nav-link {
    padding: 0.4rem 0.7rem;
    font-family: var(--z-font-display);
    font-size: var(--z-text-sm);
    font-weight: 600;
    color: var(--z-ink-muted);
    text-decoration: none;
    border: 2px solid transparent;
    transition: color var(--z-dur-ui) linear, border-color var(--z-dur-ui) linear;
  }

  .nav-link:hover {
    color: var(--z-ink);
    background-color: var(--z-surface);
  }

  .nav-link.is-active {
    color: var(--z-sun-core);
    border-bottom-color: var(--z-sun-core);
  }

  :root[data-theme='day'] .nav-link.is-active {
    color: var(--z-accent-shadow);
    border-bottom-color: var(--z-accent-edge);
  }

  .header-controls {
    display: flex;
    align-items: center;
    gap: var(--z-space-2);
  }

  .desktop-toggles {
    display: flex;
    align-items: center;
    gap: var(--z-space-2);
  }

  .mobile-toggles {
    display: none;
    align-items: center;
    gap: var(--z-space-2);
  }

  .mobile-menu-btn {
    display: none;
    align-items: center;
    justify-content: center;
    width: 2.25rem;
    height: 2.25rem;
    background-color: var(--z-surface);
    color: var(--z-ink);
    border: 2px solid var(--z-border);
    cursor: pointer;
  }

  .mobile-drawer {
    border-top: var(--z-px) solid var(--z-border);
    background-color: var(--z-surface);
    padding: var(--z-space-4);
    display: flex;
    flex-direction: column;
    gap: var(--z-space-4);
  }

  .mobile-nav {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-1);
  }

  .mobile-nav-link {
    padding: 0.6rem 0.75rem;
    font-family: var(--z-font-display);
    font-size: var(--z-text-base);
    font-weight: 600;
    color: var(--z-ink);
    text-decoration: none;
    border-left: var(--z-px) solid transparent;
  }

  .mobile-nav-link.is-active {
    background-color: var(--z-surface-elevated);
    border-left-color: var(--z-sun-core);
    color: var(--z-sun-core);
  }

  :root[data-theme='day'] .mobile-nav-link.is-active {
    color: var(--z-accent-shadow);
  }

  .mobile-controls {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-2);
    padding-top: var(--z-space-3);
    border-top: 2px solid var(--z-border);
  }

  @media (max-width: 900px) {
    .desktop-nav,
    .desktop-toggles {
      display: none;
    }

    .mobile-toggles {
      display: flex;
    }

    .mobile-menu-btn {
      display: inline-flex;
    }
  }
</style>
