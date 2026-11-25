<script lang="ts">
  import { onMount } from 'svelte';
  import {
    PixelButton,
    PixelFrame,
    PixelIcon,
    PixelTabs,
    StatusBadge,
    type TabItem
  } from '@zenith/ui';
  import { siteStrings } from '$lib/strings';

  interface GitHubAsset {
    name: string;
    browser_download_url: string;
    size: number;
  }

  interface GitHubRelease {
    id: number;
    tag_name: string;
    name: string | null;
    html_url: string;
    prerelease: boolean;
    draft: boolean;
    published_at: string;
    assets: GitHubAsset[];
  }

  type Channel = 'release' | 'beta' | 'nightly';

  let channel = $state<Channel>('release');
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let releases = $state<GitHubRelease[]>([]);

  const channelTabs: TabItem[] = [
    { id: 'release', label: siteStrings.downloads.channels.release },
    { id: 'beta', label: siteStrings.downloads.channels.beta },
    { id: 'nightly', label: siteStrings.downloads.channels.nightly }
  ];

  function classifyRelease(rel: GitHubRelease): Channel {
    const tag = `${rel.tag_name} ${rel.name ?? ''}`.toLowerCase();
    if (tag.includes('nightly') || tag.includes('dev') || tag.includes('canary')) {
      return 'nightly';
    }
    if (rel.prerelease || tag.includes('beta') || tag.includes('rc') || tag.includes('alpha')) {
      return 'beta';
    }
    return 'release';
  }

  function matchAssetForPlatform(assets: GitHubAsset[], platformId: string): GitHubAsset | null {
    const patterns: Record<string, RegExp> = {
      'linux-x86_64': /(linux|appimage|x86_64|amd64).*(x86_64|amd64|appimage|tar\.gz|deb|rpm)/i,
      'linux-arm64': /(linux.*(arm64|aarch64)|(arm64|aarch64).*linux)/i,
      'windows-x86_64': /(win|windows|\.msi|\.exe|x86_64.*zip)/i,
      'macos-arm64': /(mac|darwin|apple|aarch64.*\.dmg|arm64.*\.dmg|\.dmg)/i
    };
    const re = patterns[platformId];
    if (!re) return null;
    return assets.find((a) => re.test(a.name)) ?? null;
  }

  function formatBytes(bytes: number): string {
    if (bytes <= 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    const idx = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
    return `${(bytes / Math.pow(1024, idx)).toFixed(idx === 0 ? 0 : 1)} ${units[idx]}`;
  }

  async function fetchReleases() {
    loading = true;
    errorMessage = null;
    try {
      const response = await fetch(siteStrings.brand.releasesApiUrl, {
        headers: { Accept: 'application/vnd.github+json' }
      });
      if (!response.ok) {
        if (response.status === 404) {
          releases = [];
          return;
        }
        throw new Error(`HTTP ${response.status}`);
      }
      const data = (await response.json()) as GitHubRelease[];
      releases = Array.isArray(data) ? data.filter((r) => !r.draft) : [];
    } catch {
      errorMessage = siteStrings.downloads.errorPrefix;
      releases = [];
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    fetchReleases();
  });

  const channelReleases = $derived(releases.filter((r) => classifyRelease(r) === channel));
  const latestRelease = $derived(channelReleases[0] ?? null);
</script>

<svelte:head>
  <title>{siteStrings.downloads.metaTitle}</title>
  <meta name="description" content={siteStrings.downloads.metaDescription} />
</svelte:head>

<div class="downloads-page container">
  <header class="page-header">
    <h1>{siteStrings.downloads.heading}</h1>
    <p class="page-lead">{siteStrings.downloads.lead}</p>
  </header>

  <section class="channel-section" aria-label={siteStrings.downloads.channelLabel}>
    <PixelTabs
      tabs={channelTabs}
      bind:active={channel}
      label={siteStrings.downloads.channelLabel}
    />

    {#if loading}
      <PixelFrame variant="inset" padding="md" class="status-panel">
        <p>{siteStrings.downloads.loading}</p>
      </PixelFrame>
    {:else if errorMessage}
      <PixelFrame variant="inset" padding="md" class="status-panel">
        <p>{errorMessage}</p>
        <div class="panel-actions">
          <PixelButton variant="secondary" size="sm" onclick={fetchReleases}>
            {siteStrings.downloads.retryButton}
          </PixelButton>
          <PixelButton
            variant="ghost"
            size="sm"
            href="{siteStrings.brand.repoUrl}/releases"
            external
          >
            {siteStrings.downloads.viewReleasesButton}
          </PixelButton>
        </div>
      </PixelFrame>
    {:else if !latestRelease}
      <PixelFrame variant="elevated" padding="lg" class="status-panel">
        <div class="empty-head">
          <StatusBadge status="planned" label="Milestone M29" />
          <h2>{siteStrings.downloads.empty}</h2>
        </div>
        <p class="empty-sub">
          Zenith is currently in active foundation work. Once milestone M29 lands, signed builds
          for all four platforms below will appear here automatically.
        </p>

        <div class="platform-grid">
          {#each siteStrings.downloads.platforms as platform (platform.id)}
            <div class="platform-row">
              <div>
                <h3>{platform.name}</h3>
                <p class="platform-format">{platform.format}</p>
              </div>
              <PixelButton variant="ghost" size="sm" disabled>
                {siteStrings.downloads.downloadButton}
              </PixelButton>
            </div>
          {/each}
        </div>
      </PixelFrame>
    {:else}
      <PixelFrame variant="accent" padding="lg" class="release-panel">
        <div class="release-head">
          <div>
            <h2>{latestRelease.name || latestRelease.tag_name}</h2>
            <p class="release-meta z-tabular">
              Tag {latestRelease.tag_name}
              <span class="meta-sep" aria-hidden="true">|</span>
              Published {latestRelease.published_at.slice(0, 10)}
            </p>
          </div>
          <PixelButton variant="ghost" size="sm" href={latestRelease.html_url} external>
            {siteStrings.downloads.viewReleasesButton}
          </PixelButton>
        </div>

        <div class="platform-grid">
          {#each siteStrings.downloads.platforms as platform (platform.id)}
            {@const asset = matchAssetForPlatform(latestRelease.assets, platform.id)}
            <div class="platform-row">
              <div>
                <h3>{platform.name}</h3>
                <p class="platform-format">
                  {#if asset}
                    {asset.name} ({formatBytes(asset.size)})
                  {:else}
                    {siteStrings.downloads.noPlatformAsset}
                  {/if}
                </p>
              </div>
              {#if asset}
                <PixelButton variant="primary" size="sm" href={asset.browser_download_url} external>
                  <PixelIcon name="download" size={14} />
                  <span>{siteStrings.downloads.downloadButton}</span>
                </PixelButton>
              {:else}
                <PixelButton variant="ghost" size="sm" disabled>
                  {siteStrings.downloads.downloadButton}
                </PixelButton>
              {/if}
            </div>
          {/each}
        </div>
      </PixelFrame>
    {/if}
  </section>

  <!-- Requirements -->
  <section class="requirements-section">
    <h2>{siteStrings.downloads.requirementsHeading}</h2>
    <div class="requirements-list">
      {#each siteStrings.downloads.requirements as req (req.title)}
        <div class="requirement-item">
          <h3>{req.title}</h3>
          <p>{req.body}</p>
        </div>
      {/each}
    </div>
  </section>
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
    max-width: 58ch;
    color: var(--z-ink-muted);
  }

  .channel-section {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-5);
    margin-bottom: var(--z-space-7);
  }

  :global(.status-panel) p {
    margin: 0;
    color: var(--z-ink-muted);
  }

  .panel-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-3);
    margin-top: var(--z-space-4);
  }

  .empty-head {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--z-space-3);
    margin-bottom: var(--z-space-3);
  }

  .empty-head h2,
  .release-head h2 {
    font-size: var(--z-text-xl);
  }

  .empty-sub {
    margin: 0 0 var(--z-space-5);
    max-width: 60ch;
    color: var(--z-ink-muted);
  }

  .release-head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--z-space-4);
    margin-bottom: var(--z-space-5);
  }

  .release-meta {
    margin: 0.25rem 0 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
  }

  .meta-sep {
    margin-inline: var(--z-space-2);
    color: var(--z-border-strong);
  }

  .platform-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--z-space-4);
  }

  .platform-row {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: var(--z-space-3);
    padding: var(--z-space-4);
    background-color: var(--z-surface-inset);
    border: 2px solid var(--z-border);
  }

  .platform-row h3 {
    font-size: var(--z-text-base);
    margin-bottom: 0.2rem;
  }

  .platform-format {
    margin: 0;
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  .requirements-section h2 {
    font-size: var(--z-text-xl);
    margin-bottom: var(--z-space-4);
  }

  .requirements-list {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--z-space-6);
    border-top: var(--z-px) solid var(--z-border);
    padding-top: var(--z-space-5);
  }

  .requirement-item h3 {
    font-size: var(--z-text-lg);
    margin-bottom: 0.35rem;
  }

  .requirement-item p {
    margin: 0;
    color: var(--z-ink-muted);
    max-width: 52ch;
    font-size: var(--z-text-sm);
  }

  @media (max-width: 768px) {
    .platform-grid,
    .requirements-list {
      grid-template-columns: 1fr;
    }
  }
</style>
