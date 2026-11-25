<script lang="ts">
  import { onMount } from 'svelte';
  import {
    PixelButton,
    PixelFrame,
    PixelIcon,
    PixelProgress,
    PixelTabs,
    PixelTooltip,
    StatusBadge,
    type TabItem
  } from '@zenith/ui';
  import HistoryChart from '$lib/components/HistoryChart.svelte';
  import {
    isFallbackProgress,
    progressData,
    type MilestoneStatus
  } from '$lib/data/progress';
  import { siteStrings } from '$lib/strings';

  let selectedPhase = $state<string>('ALL');
  let selectedStatus = $state<string>('all');
  let expandedIds = $state<Record<string, boolean>>({});

  // Default expand active in-progress milestones so tasks are immediately legible
  onMount(() => {
    const initial: Record<string, boolean> = {};
    for (const m of progressData.milestones) {
      if (m.status === 'in-progress') {
        initial[m.id] = true;
      }
    }
    expandedIds = initial;

    const params = new URLSearchParams(window.location.search);
    const phaseParam = params.get('phase');
    if (phaseParam && progressData.phases.some((p) => p.id === phaseParam)) {
      selectedPhase = phaseParam;
    }
  });

  function toggleExpand(id: string) {
    expandedIds = { ...expandedIds, [id]: !expandedIds[id] };
  }

  function resetFilters() {
    selectedPhase = 'ALL';
    selectedStatus = 'all';
  }

  const phaseFilteredMilestones = $derived(
    selectedPhase === 'ALL'
      ? progressData.milestones
      : progressData.milestones.filter((m) => m.phase === selectedPhase)
  );

  const statusTabs = $derived<TabItem[]>([
    {
      id: 'all',
      label: siteStrings.tracker.statusTabs.all,
      count: phaseFilteredMilestones.length
    },
    {
      id: 'in-progress',
      label: siteStrings.tracker.statusTabs['in-progress'],
      count: phaseFilteredMilestones.filter((m) => m.status === 'in-progress').length
    },
    {
      id: 'done',
      label: siteStrings.tracker.statusTabs.done,
      count: phaseFilteredMilestones.filter((m) => m.status === 'done').length
    },
    {
      id: 'planned',
      label: siteStrings.tracker.statusTabs.planned,
      count: phaseFilteredMilestones.filter((m) => m.status === 'planned').length
    }
  ]);

  const visibleMilestones = $derived(
    selectedStatus === 'all'
      ? phaseFilteredMilestones
      : phaseFilteredMilestones.filter((m) => m.status === (selectedStatus as MilestoneStatus))
  );

  const formattedDate = $derived(progressData.generatedAt.slice(0, 10));
</script>

<svelte:head>
  <title>{siteStrings.tracker.metaTitle}</title>
  <meta name="description" content={siteStrings.tracker.metaDescription} />
</svelte:head>

<div class="tracker-page container">
  <header class="page-header">
    <div>
      <h1>{siteStrings.tracker.heading}</h1>
      <p class="page-lead">{siteStrings.tracker.lead}</p>
    </div>

    <dl class="meta-strip z-tabular">
      <div>
        <dt>{siteStrings.tracker.targetLabel}</dt>
        <dd>{progressData.targetVersion}</dd>
      </div>
      <div>
        <dt>{siteStrings.tracker.protocolLabel}</dt>
        <dd>{progressData.protocol}</dd>
      </div>
      <div>
        <dt>{siteStrings.tracker.worldVersionLabel}</dt>
        <dd>{progressData.worldVersion}</dd>
      </div>
      <div>
        <dt>{siteStrings.tracker.updatedLabel}</dt>
        <dd>{formattedDate}</dd>
      </div>
    </dl>
  </header>

  {#if isFallbackProgress}
    <PixelFrame variant="inset" padding="sm" class="fallback-banner">
      <p>{siteStrings.tracker.fallbackNotice}</p>
    </PixelFrame>
  {/if}

  <!-- Summary & SVG History Chart -->
  <section class="overview-grid" aria-label="Overall progress summary">
    <PixelFrame variant="elevated" padding="md" class="summary-card">
      <div class="summary-meters">
        <PixelProgress
          value={progressData.overall.percent}
          segments={24}
          tone="gold"
          size="lg"
          label={siteStrings.tracker.overallLabel}
          showValue
        />

        <PixelProgress
          value={progressData.overall.parityPercent}
          segments={24}
          tone="teal"
          size="lg"
          label={siteStrings.tracker.parityLabel}
          showValue
        />
      </div>

      <dl class="summary-counts z-tabular">
        <div>
          <dt>{siteStrings.tracker.milestonesLabel}</dt>
          <dd>
            {progressData.overall.milestonesDone} / {progressData.overall.milestonesTotal}
          </dd>
        </div>
        <div>
          <dt>{siteStrings.tracker.suitesLabel}</dt>
          <dd>
            {progressData.overall.suitesPassing} / {progressData.overall.suitesTotal}
          </dd>
        </div>
      </dl>
    </PixelFrame>

    <PixelFrame variant="elevated" padding="md">
      <h2 class="panel-heading">{siteStrings.tracker.historyHeading}</h2>
      <HistoryChart history={progressData.history} />
    </PixelFrame>
  </section>

  <!-- Phase filter strip (P0-P7) -->
  <section class="phases-section" aria-label={siteStrings.tracker.phasesHeading}>
    <div class="section-bar">
      <h2>{siteStrings.tracker.phasesHeading}</h2>
      {#if selectedPhase !== 'ALL'}
        <PixelButton variant="ghost" size="sm" onclick={() => (selectedPhase = 'ALL')}>
          {siteStrings.tracker.allPhasesLabel}
        </PixelButton>
      {/if}
    </div>

    <div class="phase-strip">
      {#each progressData.phases as phase (phase.id)}
        {@const active = selectedPhase === phase.id}
        <button
          type="button"
          class="phase-tile"
          class:is-active={active}
          aria-pressed={active}
          onclick={() => (selectedPhase = active ? 'ALL' : phase.id)}
        >
          <div class="phase-tile-top">
            <span class="phase-id">{phase.id}</span>
            <span class="phase-pct z-tabular">{phase.percent.toFixed(0)}%</span>
          </div>
          <span class="phase-name">{phase.title}</span>
          <PixelProgress
            value={phase.percent}
            segments={8}
            tone={phase.percent >= 100 ? 'teal' : 'gold'}
            size="sm"
          />
        </button>
      {/each}
    </div>
  </section>

  <!-- Milestones filter + list -->
  <section class="milestones-section" aria-label="Milestones">
    <div class="filter-bar">
      <PixelTabs
        tabs={statusTabs}
        bind:active={selectedStatus}
        label={siteStrings.tracker.statusFilterLabel}
      />
    </div>

    {#if visibleMilestones.length === 0}
      <PixelFrame variant="inset" padding="md" class="empty-state">
        <p>{siteStrings.tracker.emptyFilter}</p>
        <PixelButton variant="secondary" size="sm" onclick={resetFilters}>
          {siteStrings.tracker.clearFilters}
        </PixelButton>
      </PixelFrame>
    {:else}
      <div class="milestone-list">
        {#each visibleMilestones as milestone (milestone.id)}
          {@const isOpen = !!expandedIds[milestone.id]}
          {@const compact = milestone.status === 'planned' && !isOpen}
          <PixelFrame
            variant={milestone.status === 'in-progress' ? 'accent' : compact ? 'flat' : 'elevated'}
            padding={compact ? 'sm' : 'md'}
            as="article"
            id={milestone.id}
          >
            <div class="milestone-body" class:compact>
            <div class="milestone-head">
              <div class="milestone-id-group">
                <span class="milestone-id">{milestone.id}</span>
                <span class="milestone-phase">{milestone.phase}</span>
                <h3>{milestone.title}</h3>
              </div>

              <div class="milestone-badges">
                <StatusBadge status={milestone.status} />
                <PixelTooltip text="Relative effort weight used to compute overall progress">
                  <span class="weight-pill z-tabular">
                    {siteStrings.tracker.weightLabel} {milestone.weight}
                  </span>
                </PixelTooltip>
              </div>
            </div>

            {#if !compact}
            <div class="milestone-meter-row">
              <div class="meter-grow">
                <PixelProgress
                  value={milestone.percent}
                  segments={20}
                  tone={milestone.status === 'done' ? 'teal' : 'gold'}
                  size="sm"
                />
              </div>
              <span class="milestone-pct z-tabular">{milestone.percent.toFixed(1)}%</span>
            </div>
            {/if}

            <div class="milestone-footer-bar">
              <div class="milestone-summary z-tabular">
                <span>
                  {siteStrings.tracker.tasksLabel}: {milestone.tasksDone}/{milestone.tasksTotal}
                </span>
                <span>
                  {siteStrings.tracker.suitesSectionLabel}: {milestone.suites.length}
                </span>
              </div>

              <button
                type="button"
                class="expand-btn"
                aria-expanded={isOpen}
                aria-controls="details-{milestone.id}"
                onclick={() => toggleExpand(milestone.id)}
              >
                <PixelIcon name={isOpen ? 'chevron-down' : 'chevron-right'} size={14} />
                <span>
                  {isOpen ? siteStrings.tracker.collapseTasks : siteStrings.tracker.expandTasks}
                </span>
              </button>
            </div>

            {#if isOpen}
              <div id="details-{milestone.id}" class="milestone-details">
                <div class="detail-col">
                  <h4>{siteStrings.tracker.tasksLabel}</h4>
                  <ul class="task-list">
                    {#each milestone.tasks as task (task.title)}
                      <li class="task-item" class:is-done={task.done}>
                        <span class="task-box" aria-hidden="true">
                          {#if task.done}
                            <PixelIcon name="check" size={12} />
                          {/if}
                        </span>
                        <span>{task.title}</span>
                      </li>
                    {/each}
                  </ul>
                </div>

                <div class="detail-col">
                  <h4>{siteStrings.tracker.suitesSectionLabel}</h4>
                  {#if milestone.suites.length === 0}
                    <p class="no-suites">{siteStrings.tracker.noSuitesText}</p>
                  {:else}
                    <ul class="suite-list">
                      {#each milestone.suites as suite (suite.id)}
                        <li class="suite-item">
                          <div class="suite-top">
                            <code class="suite-id">{suite.id}</code>
                            <span class="suite-ratio z-tabular">
                              {suite.passed}/{suite.total} ({suite.percent.toFixed(0)}%)
                            </span>
                          </div>
                          <PixelProgress
                            value={suite.percent}
                            segments={10}
                            tone="teal"
                            size="sm"
                          />
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              </div>
            {/if}
            </div>
          </PixelFrame>
        {/each}
      </div>
    {/if}
  </section>
</div>

<style>
  .container {
    max-width: var(--z-container);
    margin: 0 auto;
    padding: var(--z-space-6) var(--z-space-4) 0;
  }

  .page-header {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: flex-end;
    gap: var(--z-space-5);
    padding-bottom: var(--z-space-5);
    border-bottom: var(--z-px) solid var(--z-border);
    margin-bottom: var(--z-space-5);
  }

  .page-header h1 {
    font-size: var(--z-text-2xl);
    margin-bottom: var(--z-space-2);
  }

  .page-lead {
    margin: 0;
    max-width: 56ch;
    color: var(--z-ink-muted);
  }

  .meta-strip {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-4);
  }

  .meta-strip > div {
    padding-left: var(--z-space-3);
    border-left: var(--z-px) solid var(--z-border);
  }

  .meta-strip dt {
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  .meta-strip dd {
    margin: 0;
    font-family: var(--z-font-body);
    font-size: var(--z-text-base);
    font-weight: 700;
  }

  :global(.fallback-banner) {
    margin-bottom: var(--z-space-5);
  }

  :global(.fallback-banner) p {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
  }

  .overview-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr);
    gap: var(--z-space-5);
    margin-bottom: var(--z-space-7);
  }

  .panel-heading {
    font-size: var(--z-text-lg);
    margin-bottom: var(--z-space-3);
  }

  .summary-meters {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-4);
    margin-bottom: var(--z-space-5);
  }

  .summary-counts {
    margin: 0;
    padding-top: var(--z-space-4);
    border-top: 2px solid var(--z-navy-deep);
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--z-space-4);
  }

  .summary-counts dt {
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }

  .summary-counts dd {
    margin: 0.15rem 0 0;
    font-family: var(--z-font-body);
    font-size: var(--z-text-xl);
    font-weight: 700;
  }

  /* Phases strip */
  .phases-section {
    margin-bottom: var(--z-space-7);
  }

  .section-bar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--z-space-3);
    margin-bottom: var(--z-space-4);
  }

  .section-bar h2 {
    font-size: var(--z-text-xl);
  }

  .phase-strip {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--z-space-3);
  }

  .phase-tile {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    padding: var(--z-space-3);
    text-align: left;
    color: var(--z-ink);
    background-color: var(--z-surface);
    border: var(--z-px) solid var(--z-border);
    cursor: pointer;
    transition:
      border-color var(--z-dur-ui) linear,
      transform var(--z-dur-press) var(--z-ease-step);
  }

  .phase-tile:hover {
    border-color: var(--z-horizon-bright);
  }

  .phase-tile:active {
    transform: translateY(2px);
  }

  .phase-tile.is-active {
    background-color: var(--z-surface-elevated);
    border-color: var(--z-sun-core);
    box-shadow: inset 0 var(--z-px) 0 0 var(--z-sun-core);
  }

  .phase-tile-top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--z-space-2);
  }

  .phase-id {
    font-family: var(--z-font-display);
    font-size: var(--z-text-base);
    font-weight: 700;
    color: var(--z-sun-core);
  }

  :root[data-theme='day'] .phase-id {
    color: var(--z-accent-shadow);
  }

  .phase-pct {
    font-size: var(--z-text-sm);
    font-weight: 700;
    color: var(--z-ink-muted);
  }

  .phase-name {
    font-size: var(--z-text-sm);
    font-weight: 700;
    color: var(--z-ink);
  }

  /* Milestones */
  .filter-bar {
    margin-bottom: var(--z-space-5);
  }

  .milestone-list {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-4);
  }

  /* Planned milestones collapse to one quiet row; the full card appears on expand or once work starts. */
  .milestone-body.compact {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--z-space-2) var(--z-space-4);
  }

  .milestone-body.compact .milestone-head {
    flex: 1 1 32rem;
    margin-bottom: 0;
  }

  .milestone-body.compact .milestone-footer-bar {
    margin: 0;
    padding: 0;
    border: 0;
  }

  .milestone-head {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: var(--z-space-3);
    margin-bottom: var(--z-space-3);
  }

  .milestone-id-group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--z-space-2);
  }

  .milestone-id {
    padding: 0.15rem 0.5rem;
    font-family: var(--z-font-display);
    font-weight: 700;
    font-size: var(--z-text-sm);
    background-color: var(--z-surface-inset);
    border: 2px solid var(--z-border-strong);
    color: var(--z-sun-core);
  }

  :root[data-theme='day'] .milestone-id {
    color: var(--z-accent-shadow);
  }

  .milestone-phase {
    font-family: var(--z-font-display);
    font-size: var(--z-text-sm);
    color: var(--z-ink-dim);
  }

  .milestone-id-group h3 {
    font-size: var(--z-text-lg);
  }

  .milestone-badges {
    display: flex;
    align-items: center;
    gap: var(--z-space-2);
  }

  .weight-pill {
    padding: 0.18rem 0.55rem;
    font-size: var(--z-text-xs);
    font-weight: 700;
    color: var(--z-ink-muted);
    background-color: var(--z-surface-inset);
    border: 2px solid var(--z-border);
  }

  .milestone-meter-row {
    display: flex;
    align-items: center;
    gap: var(--z-space-3);
    margin-bottom: var(--z-space-3);
  }

  .meter-grow {
    flex: 1 1 auto;
  }

  .milestone-pct {
    min-width: 3.5rem;
    text-align: right;
    font-family: var(--z-font-body);
    font-weight: 700;
    font-size: var(--z-text-base);
  }

  .milestone-footer-bar {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: var(--z-space-3);
  }

  .milestone-summary {
    display: flex;
    flex-wrap: wrap;
    gap: var(--z-space-4);
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
  }

  .expand-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.35rem 0.65rem;
    font-family: var(--z-font-body);
    font-size: var(--z-text-sm);
    font-weight: 700;
    color: var(--z-link);
    background-color: transparent;
    border: 2px solid var(--z-border);
    cursor: pointer;
  }

  .expand-btn:hover {
    background-color: var(--z-surface-inset);
    border-color: var(--z-horizon-bright);
  }

  .milestone-details {
    margin-top: var(--z-space-4);
    padding: var(--z-space-4);
    background-color: var(--z-surface-inset);
    border: 2px solid var(--z-navy-deep);
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr);
    gap: var(--z-space-5);
  }

  .detail-col h4 {
    font-size: var(--z-text-base);
    margin-bottom: var(--z-space-3);
    color: var(--z-ink);
  }

  .task-list,
  .suite-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }

  .task-item {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    font-size: var(--z-text-sm);
    color: var(--z-ink-muted);
  }

  .task-item.is-done {
    color: var(--z-ink);
  }

  .task-box {
    flex-shrink: 0;
    margin-top: 0.2rem;
    width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
    border: 2px solid var(--z-border-strong);
    background-color: var(--z-surface);
    color: var(--z-status-done);
  }

  .task-item.is-done .task-box {
    border-color: var(--z-status-done);
    background-color: var(--z-status-done-bg);
  }

  .no-suites {
    margin: 0;
    font-size: var(--z-text-sm);
    color: var(--z-ink-dim);
  }

  .suite-item {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.55rem;
    background-color: var(--z-surface);
    border: 2px solid var(--z-border);
  }

  .suite-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--z-space-2);
    font-size: var(--z-text-xs);
  }

  .suite-id {
    font-family: var(--z-font-mono);
    color: var(--z-ink);
  }

  .suite-ratio {
    color: var(--z-ink-muted);
    font-weight: 700;
  }

  :global(.empty-state) {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--z-space-3);
  }

  :global(.empty-state) p {
    margin: 0;
    color: var(--z-ink-muted);
  }

  @media (max-width: 960px) {
    .overview-grid,
    .milestone-details {
      grid-template-columns: 1fr;
    }

    .phase-strip {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 480px) {
    .phase-strip {
      grid-template-columns: 1fr;
    }
  }
</style>
