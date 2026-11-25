<script lang="ts">
  import type { Snippet } from 'svelte';
  import '@zenith/ui/fonts.css';
  import '@zenith/ui/tokens.css';
  import '$lib/styles/prose.css';

  import SiteHeader from '$lib/components/SiteHeader.svelte';
  import SiteFooter from '$lib/components/SiteFooter.svelte';
  import { siteStrings } from '$lib/strings';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();
</script>

<a class="skip-link" href="#main-content">{siteStrings.nav.skipToContent}</a>

<div class="app-shell">
  <SiteHeader />
  <main id="main-content" class="app-main" tabindex="-1">
    {@render children()}
  </main>
  <SiteFooter />
</div>

<style>
  .skip-link {
    position: absolute;
    left: -9999px;
    top: 0;
    z-index: var(--z-layer-modal);
    padding: 0.65rem 1rem;
    background-color: var(--z-sun-core);
    color: #0b1020;
    font-family: var(--z-font-display);
    font-weight: 700;
    text-decoration: none;
  }

  .skip-link:focus {
    left: var(--z-space-3);
    top: var(--z-space-3);
  }

  .app-shell {
    min-height: 100dvh;
    display: flex;
    flex-direction: column;
  }

  .app-main {
    flex: 1 0 auto;
    outline: none;
  }
</style>
