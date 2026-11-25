<script lang="ts">
  import type { ProgressHistoryPoint } from '$lib/data/progress';
  import { siteStrings } from '$lib/strings';

  interface Props {
    history: ProgressHistoryPoint[];
  }

  let { history }: Props = $props();

  let activeIndex = $state<number | null>(null);

  const width = 480;
  const height = 180;
  const padLeft = 40;
  const padRight = 18;
  const padTop = 18;
  const padBottom = 30;
  const plotW = width - padLeft - padRight;
  const plotH = height - padTop - padBottom;

  const yTicks = [0, 25, 50, 75, 100];

  function xFor(index: number, count: number): number {
    if (count <= 1) return Math.round(padLeft + plotW * 0.5);
    return Math.round(padLeft + (index / (count - 1)) * plotW);
  }

  function yFor(pct: number): number {
    const clamped = Math.max(0, Math.min(100, pct));
    return Math.round(padTop + plotH - (clamped / 100) * plotH);
  }

  // Stepped pixel path between data points (horizontal then vertical steps)
  function buildSteppedPath(points: Array<{ x: number; y: number }>): string {
    if (points.length === 0) return '';
    if (points.length === 1) {
      const p = points[0];
      return `M ${padLeft} ${p.y} H ${p.x}`;
    }
    let d = `M ${points[0].x} ${points[0].y}`;
    for (let i = 1; i < points.length; i++) {
      const prev = points[i - 1];
      const curr = points[i];
      const midX = Math.round((prev.x + curr.x) / 2);
      d += ` H ${midX} V ${curr.y} H ${curr.x}`;
    }
    return d;
  }

  const overallCoords = $derived(
    history.map((pt, i) => ({
      x: xFor(i, history.length),
      y: yFor(pt.percent),
      pt
    }))
  );

  const parityCoords = $derived(
    history.map((pt, i) => ({
      x: xFor(i, history.length),
      y: yFor(pt.parityPercent),
      pt
    }))
  );

  const overallPath = $derived(buildSteppedPath(overallCoords));
  const parityPath = $derived(buildSteppedPath(parityCoords));

  const inspected = $derived(
    activeIndex !== null && history[activeIndex]
      ? history[activeIndex]
      : history[history.length - 1] ?? null
  );
</script>

<div class="chart-wrap">
  <div class="chart-meta">
    <div class="chart-legend" aria-label="Chart series">
      <span class="legend-item">
        <span class="legend-swatch swatch-gold" aria-hidden="true"></span>
        <span>{siteStrings.tracker.historyOverallLegend}</span>
      </span>
      <span class="legend-item">
        <span class="legend-swatch swatch-teal" aria-hidden="true"></span>
        <span>{siteStrings.tracker.historyParityLegend}</span>
      </span>
    </div>

    {#if inspected}
      <div class="chart-readout z-tabular" aria-live="polite">
        <span class="readout-date">{inspected.date}</span>
        <span class="readout-gold">{inspected.percent.toFixed(1)}%</span>
        <span class="readout-teal">{inspected.parityPercent.toFixed(1)}%</span>
      </div>
    {/if}
  </div>

  <svg
    viewBox="0 0 {width} {height}"
    class="pixel-chart"
    shape-rendering="crispEdges"
    role="img"
    aria-label="Progress history chart showing overall completion and parity pass percentage over time"
  >
    <!-- Plot background well -->
    <rect
      x={padLeft}
      y={padTop}
      width={plotW}
      height={plotH}
      class="chart-well"
    />

    <!-- Horizontal pixel gridlines -->
    {#each yTicks as tick (tick)}
      {@const y = yFor(tick)}
      <line
        x1={padLeft}
        y1={y}
        x2={padLeft + plotW}
        y2={y}
        class="grid-line"
        stroke-dasharray={tick === 0 ? undefined : '4 4'}
      />
      <text x={padLeft - 6} y={y + 4} text-anchor="end" class="axis-label">
        {tick}%
      </text>
    {/each}

    <!-- Left axis -->
    <line
      x1={padLeft}
      y1={padTop}
      x2={padLeft}
      y2={padTop + plotH}
      class="axis-line"
    />

    <!-- Stepped series lines -->
    {#if parityPath}
      <path d={parityPath} fill="none" stroke="#5EE0CF" stroke-width="3" />
    {/if}
    {#if overallPath}
      <path d={overallPath} fill="none" stroke="#FFD34D" stroke-width="3" />
    {/if}

    <!-- Pixel plot nodes -->
    {#each overallCoords as node, i (node.pt.date + i)}
      {@const parityNode = parityCoords[i]}
      <!-- Vertical hover hit band -->
      <rect
        x={node.x - 12}
        y={padTop}
        width="24"
        height={plotH}
        fill="transparent"
        role="button"
        tabindex="0"
        aria-label="{node.pt.date}: overall {node.pt.percent.toFixed(1)}%, parity {node.pt.parityPercent.toFixed(1)}%"
        onmouseenter={() => (activeIndex = i)}
        onmouseleave={() => (activeIndex = null)}
        onfocus={() => (activeIndex = i)}
        onblur={() => (activeIndex = null)}
      />

      {#if activeIndex === i}
        <line
          x1={node.x}
          y1={padTop}
          x2={node.x}
          y2={padTop + plotH}
          class="cursor-line"
        />
      {/if}

      {#if parityNode}
        <rect
          x={parityNode.x - 4}
          y={parityNode.y - 4}
          width="8"
          height="8"
          fill="#5EE0CF"
          stroke="#0B1020"
          stroke-width="2"
        />
      {/if}

      <rect
        x={node.x - 4}
        y={node.y - 4}
        width="8"
        height="8"
        fill="#FFD34D"
        stroke="#0B1020"
        stroke-width="2"
      />
    {/each}

    <!-- X axis date labels (first, middle, last) -->
    {#if history.length === 1}
      <text
        x={xFor(0, 1)}
        y={height - 8}
        text-anchor="middle"
        class="axis-label"
      >
        {history[0].date}
      </text>
    {:else if history.length > 1}
      <text x={padLeft} y={height - 8} text-anchor="start" class="axis-label">
        {history[0].date}
      </text>
      <text x={padLeft + plotW} y={height - 8} text-anchor="end" class="axis-label">
        {history[history.length - 1].date}
      </text>
    {/if}
  </svg>

  {#if history.length <= 1}
    <p class="chart-caption">{siteStrings.tracker.historyEmpty}</p>
  {/if}
</div>

<style>
  .chart-wrap {
    display: flex;
    flex-direction: column;
    gap: var(--z-space-3);
  }

  .chart-meta {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: var(--z-space-3);
    font-size: var(--z-text-xs);
  }

  .chart-legend {
    display: flex;
    align-items: center;
    gap: var(--z-space-4);
  }

  .legend-item {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--z-ink-muted);
    font-weight: 700;
  }

  .legend-swatch {
    width: 10px;
    height: 10px;
    display: inline-block;
  }

  .swatch-gold {
    background-color: var(--z-sun-core);
  }

  .swatch-teal {
    background-color: var(--z-horizon-bright);
  }

  .chart-readout {
    display: flex;
    align-items: center;
    gap: var(--z-space-3);
    font-weight: 700;
  }

  .readout-date {
    color: var(--z-ink-muted);
  }

  .readout-gold {
    color: var(--z-sun-core);
  }

  :root[data-theme='day'] .readout-gold {
    color: var(--z-accent-shadow);
  }

  .readout-teal {
    color: var(--z-link);
  }

  .pixel-chart {
    width: 100%;
    height: auto;
    display: block;
  }

  .chart-well {
    fill: var(--z-surface-inset);
    stroke: var(--z-border);
    stroke-width: 2;
  }

  .grid-line {
    stroke: var(--z-navy-deep);
    stroke-width: 1;
  }

  .axis-line {
    stroke: var(--z-border-strong);
    stroke-width: 2;
  }

  .cursor-line {
    stroke: var(--z-arc);
    stroke-width: 1;
    stroke-dasharray: 2 2;
  }

  .axis-label {
    fill: var(--z-ink-dim);
    font-family: var(--z-font-body);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .chart-caption {
    margin: 0;
    font-size: var(--z-text-xs);
    color: var(--z-ink-dim);
  }
</style>
