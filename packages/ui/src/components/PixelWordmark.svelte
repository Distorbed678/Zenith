<script lang="ts">
  /**
   * The Zenith wordmark, drawn from original pixel glyphs (7 px tall).
   * A dedicated wordmark keeps the brand legible: the display font's "Z" reads like a "2" at large sizes.
   */
  interface Props {
    /** Rendered height in CSS pixels; snapped to a whole multiple of the 7-pixel glyph grid. */
    height?: number;
    /** Fill color (defaults to the current text color). */
    color?: string;
    /** Drop-shadow color, offset one glyph pixel down and right. Pass '' to disable. */
    shadow?: string;
    /** Accessible name. */
    label?: string;
    class?: string;
  }

  let {
    height = 28,
    color = 'currentColor',
    shadow = 'var(--zn-wordmark-shadow, rgba(0, 0, 0, 0.45))',
    label = 'Zenith',
    class: className = ''
  }: Props = $props();

  // Original glyphs, 7 rows each. '#' = filled pixel.
  const GLYPHS: Record<string, string[]> = {
    Z: ['#####', '....#', '...#.', '..#..', '.#...', '#....', '#####'],
    e: ['.....', '.....', '.###.', '#...#', '#####', '#....', '.###.'],
    n: ['.....', '.....', '####.', '#...#', '#...#', '#...#', '#...#'],
    i: ['.#.', '...', '##.', '.#.', '.#.', '.#.', '###'],
    t: ['.#...', '.#...', '####.', '.#...', '.#...', '.#..#', '..##.'],
    h: ['#....', '#....', '####.', '#...#', '#...#', '#...#', '#...#']
  };

  const WORD = 'Zenith';
  const GAP = 1;

  const layout = $derived.by(() => {
    const cells: Array<[number, number]> = [];
    let x = 0;
    for (const ch of WORD) {
      const g = GLYPHS[ch];
      g.forEach((row, y) => {
        [...row].forEach((c, dx) => {
          if (c === '#') cells.push([x + dx, y]);
        });
      });
      x += g[0].length + GAP;
    }
    return { cells, width: x - GAP };
  });

  const scale = $derived(Math.max(1, Math.round(height / 7)));
</script>

<svg
  class={className}
  role="img"
  aria-label={label}
  width={(layout.width + 1) * scale}
  height={8 * scale}
  viewBox="0 0 {layout.width + 1} 8"
  shape-rendering="crispEdges"
>
  {#if shadow}
    {#each layout.cells as [x, y] (`s${x}-${y}`)}
      <rect x={x + 1} y={y + 1} width="1" height="1" fill={shadow} />
    {/each}
  {/if}
  {#each layout.cells as [x, y] (`f${x}-${y}`)}
    <rect {x} {y} width="1" height="1" fill={color} />
  {/each}
</svg>

<style>
  svg {
    display: block;
    image-rendering: pixelated;
  }
</style>
