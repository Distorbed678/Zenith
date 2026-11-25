export const siteStrings = {
  brand: {
    name: 'Zenith',
    tagline: 'Your copy of the game, rebuilt 1:1 in Rust.',
    subline:
      'An independent engine compatible with Minecraft: Java Edition. Requires your own already-launched copy and ships none of the game files.',
    repoUrl: 'https://github.com/Distorbed678/Zenith',
    releasesApiUrl: 'https://api.github.com/repos/Distorbed678/Zenith/releases',
    license: 'MIT OR Apache-2.0',
    trademarkNotice:
      'Zenith is an independent project. Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft. Minecraft is a trademark of Mojang AB.'
  },
  nav: {
    skipToContent: 'Skip to main content',
    primaryLabel: 'Primary navigation',
    openMenu: 'Open navigation menu',
    closeMenu: 'Close navigation menu',
    items: [
      { id: 'home', label: 'Home', path: '/' },
      { id: 'tracker', label: 'Tracker', path: '/tracker' },
      { id: 'docs', label: 'Docs', path: '/docs' },
      { id: 'mods', label: 'Mods', path: '/mods' },
      { id: 'downloads', label: 'Downloads', path: '/downloads' },
      { id: 'changelog', label: 'Changelog', path: '/changelog' }
    ]
  },
  footer: {
    linksHeading: 'Explore',
    projectHeading: 'Project',
    apiDocsLabel: 'Rust API reference',
    githubLabel: 'GitHub repository',
    licenseLabel: 'License'
  },
  home: {
    metaTitle: 'Zenith — Independent Rust engine compatible with Minecraft: Java Edition',
    metaDescription:
      'Zenith is an independent open-source Rust game engine aiming for 1:1 parity with Minecraft: Java Edition using your own copy.',
    ctaTracker: 'Open progress tracker',
    ctaDocs: 'Read the docs',
    ctaGithub: 'View on GitHub',
    horizonOverallLabel: 'Overall rebuild progress',
    horizonParityLabel: 'Parity suites passing',
    horizonTargetLabel: 'Target version',
    horizonProtocolLabel: 'Protocol',
    differentHeading: 'Built for 1:1 parity, verified in the open',
    differentLead:
      'Zenith reimplements the client, server, and tooling in Rust from clean-room specifications. Internals are free to be fast, while observable gameplay and visuals stay identical to vanilla.',
    pillars: [
      {
        id: 'parity',
        icon: 'shield' as const,
        title: 'Parity proven against your own copy',
        body: 'Every subsystem ships with automated parity suites comparing world generation, ticking, packets, and pixels directly against your local Java Edition installation.'
      },
      {
        id: 'vulkan',
        icon: 'gpu' as const,
        title: 'Vulkan 1.3 renderer via ash',
        body: 'A modern GPU-driven Vulkan 1.3 renderer built for frame-time stability while matching vanilla lighting, culling, atlases, and core shaders pixel for pixel.'
      },
      {
        id: 'wasm',
        icon: 'wasm' as const,
        title: 'Sandboxed WebAssembly mods',
        body: 'Mods run inside Wasmtime with explicit capability permissions for network, filesystem, and rendering. Base gameplay stays pure vanilla; extras live in mods.'
      },
      {
        id: 'launcher',
        icon: 'launcher' as const,
        title: 'Dedicated pixel-crafted launcher',
        body: 'A Tauri 2 desktop launcher sharing this exact interface imports profiles from Prism, MultiMC, the official launcher, and Modrinth or CurseForge apps.'
      },
      {
        id: 'cleanroom',
        icon: 'lock' as const,
        title: 'Nothing from the game is shipped',
        body: 'Zenith distributes zero textures, sounds, data packs, or decompiled code. It reads assets at runtime only after local ownership checks pass on your launched copy.'
      }
    ],
    roadmapHeading: 'Phase roadmap',
    roadmapLead:
      'Development moves headless core first: data formats and world simulation before networking, Vulkan rendering, and the launcher.',
    roadmapAction: 'Inspect all milestones'
  },
  tracker: {
    metaTitle: 'Progress tracker — Zenith',
    metaDescription:
      'Live milestone completion and parity test pass rates for the Zenith 1:1 Rust rebuild.',
    heading: 'Live progress tracker',
    lead: 'Tracked from milestones and automated parity suites. A subsystem counts as done only when every linked parity suite reaches 100%.',
    overallLabel: 'Overall completion',
    parityLabel: 'Parity suites',
    milestonesLabel: 'Milestones complete',
    suitesLabel: 'Suites passing',
    targetLabel: 'Target',
    protocolLabel: 'Protocol',
    worldVersionLabel: 'World version',
    updatedLabel: 'Generated',
    fallbackNotice:
      'Showing baseline tracker data. Live numbers update automatically whenever progress/progress.json is generated.',
    historyHeading: 'Progress over time',
    historyOverallLegend: 'Overall %',
    historyParityLegend: 'Parity %',
    historyEmpty: 'More history points will appear as milestones advance.',
    phasesHeading: 'Phases P0 to P7',
    allPhasesLabel: 'All phases',
    statusFilterLabel: 'Filter milestones by status',
    statusTabs: {
      all: 'All',
      'in-progress': 'In progress',
      done: 'Done',
      planned: 'Planned'
    },
    weightLabel: 'Weight',
    tasksLabel: 'Tasks',
    suitesSectionLabel: 'Parity suites',
    noSuitesText: 'No parity suites linked to this milestone.',
    expandTasks: 'Show tasks and suites',
    collapseTasks: 'Hide tasks and suites',
    emptyFilter: 'No milestones match the selected phase and status filters.',
    clearFilters: 'Reset filters'
  },
  docs: {
    metaTitle: 'Documentation — Zenith',
    metaDescription:
      'Architecture plans, guides, design records, and clean-room specifications for Zenith.',
    heading: 'Documentation',
    lead: 'Plans, architecture decision records, guides, and clean-room behavior specifications read directly from the repository.',
    sidebarLabel: 'Documentation navigation',
    tocHeading: 'On this page',
    apiLinkTitle: 'Rust API reference',
    apiLinkDescription: 'Generated rustdoc documentation for every zenith-* workspace crate.',
    apiPageHeading: 'Rust API reference',
    apiPageBody:
      'Crate-level documentation for the Zenith Rust workspace is generated by rustdoc and published alongside the static site.',
    apiOpenButton: 'Open rustdoc at /Zenith/api/',
    apiLocalNote:
      'If rustdoc has not been published for the current milestone yet, generate it locally with cargo doc --workspace --no-deps --open.',
    emptyGroup: 'Documents in this section will appear automatically when added to the repository.',
    groups: {
      plan: 'Plan',
      guide: 'Guides',
      adr: 'Architecture decisions',
      specs: 'Specifications',
      decisions: 'Owner decisions'
    }
  },
  downloads: {
    metaTitle: 'Downloads — Zenith',
    metaDescription:
      'Release, beta, and nightly builds of Zenith for Linux, Windows, and macOS Apple Silicon.',
    heading: 'Downloads',
    lead: 'Signed desktop builds for Linux, Windows, and macOS Apple Silicon fetched directly from GitHub Releases.',
    channelLabel: 'Release channel',
    channels: {
      release: 'Release',
      beta: 'Beta',
      nightly: 'Nightly'
    },
    platforms: [
      { id: 'linux-x86_64', name: 'Linux x86_64', format: 'AppImage, Flatpak, AUR' },
      { id: 'linux-arm64', name: 'Linux ARM64', format: 'AppImage, tarball' },
      { id: 'windows-x86_64', name: 'Windows x86_64', format: 'MSI installer, portable zip' },
      { id: 'macos-arm64', name: 'macOS Apple Silicon', format: 'DMG bundle' }
    ],
    loading: 'Checking GitHub Releases for available builds…',
    empty: 'No builds yet. The first nightly ships with milestone M29.',
    errorPrefix: 'Could not reach GitHub Releases right now.',
    retryButton: 'Check again',
    viewReleasesButton: 'View releases on GitHub',
    downloadButton: 'Download',
    noPlatformAsset: 'No package for this platform in this release.',
    requirementsHeading: 'Before you launch Zenith',
    requirements: [
      {
        title: 'Your own already-launched copy',
        body: 'Zenith requires a legitimately owned copy of Minecraft: Java Edition matching the target version (26.3), launched at least once so official files and assets are present locally.'
      },
      {
        title: 'Vulkan 1.3 capable GPU',
        body: 'The client renderer targets Vulkan 1.3 via ash. Make sure your graphics drivers expose Vulkan 1.3 support.'
      }
    ]
  },
  changelog: {
    metaTitle: 'Changelog — Zenith',
    metaDescription: 'Release notes and development milestones for Zenith.',
    heading: 'Changelog',
    lead: 'Generated from conventional commits and milestone releases.',
    emptyTitle: 'No changelog entries published yet',
    emptyBody:
      'Zenith is currently in early foundation work (milestone M0). Release notes will appear here automatically once CHANGELOG.md is created in the repository root.',
    emptyActionTracker: 'Check the live tracker',
    emptyActionGithub: 'Browse commits on GitHub'
  },
  mods: {
    metaTitle: 'Mod index — Zenith',
    metaDescription:
      'Browse sandboxed WebAssembly mods for Zenith with explicit capability permissions.',
    heading: 'Mod index',
    lead: 'Explore WebAssembly mods built for the Zenith SDK. Every mod declares the capabilities it needs before the launcher installs it.',
    milestoneNotice:
      'The signed GitHub-based mod index launches with milestone M25. Entries below are sample placeholders showing how search, tags, permissions, and launcher links work.',
    searchLabel: 'Search mods',
    searchPlaceholder: 'Search by name, summary, or tag…',
    tagsFilterLabel: 'Filter by tag',
    permissionsFilterLabel: 'Filter by permission',
    allTagsLabel: 'All tags',
    exampleBadge: 'Example entry',
    openInLauncher: 'Open in launcher',
    permissionsHeading: 'Capabilities',
    permissionLabels: {
      network: 'Network',
      filesystem: 'Filesystem',
      render: 'Render'
    },
    permissionDescriptions: {
      network: 'Can open sockets or talk to local or remote endpoints.',
      filesystem: 'Can read or write files inside its scoped instance directory.',
      render: 'Can register custom HUD overlays or rendering hooks.'
    },
    emptyResults: 'No sample mods match your search and filter selection.',
    clearFilters: 'Clear filters'
  },
  notFound: {
    metaTitle: 'Page not found — Zenith',
    code: '404',
    heading: 'Below the horizon',
    body: 'That page does not exist or moved to another coordinate. Head back to the home page, check the live tracker, or browse the docs.',
    homeAction: 'Return home',
    trackerAction: 'Open progress tracker',
    docsAction: 'Read the docs'
  }
} as const;
