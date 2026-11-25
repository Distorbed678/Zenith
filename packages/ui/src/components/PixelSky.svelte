<script lang="ts">
  import { onMount } from 'svelte';
  import { motionState } from '../motion.svelte';
  import { themeState } from '../theme.svelte';

  interface Props {
    mode?: 'hero' | 'ambient';
    pixelSize?: number;
    paused?: boolean;
    showHills?: boolean;
    class?: string;
  }

  let {
    mode = 'hero',
    pixelSize = 3,
    paused = false,
    showHills = true,
    class: className = ''
  }: Props = $props();

  let canvas: HTMLCanvasElement | null = $state(null);

  const isPaused = $derived(paused || motionState.paused);

  // Deterministic pseudo-random helper for crisp repeatable stars and hill silhouettes
  function hash(n: number): number {
    const x = Math.sin(n * 127.1 + 311.7) * 43758.5453123;
    return x - Math.floor(x);
  }

  interface Star {
    nx: number;
    ny: number;
    phase: number;
    bright: boolean;
  }

  interface Cloud {
    x: number;
    ny: number;
    w: number;
    speed: number;
    layer: 0 | 1;
  }

  const stars: Star[] = Array.from({ length: 46 }, (_, i) => ({
    nx: hash(i * 3 + 1),
    ny: 0.06 + hash(i * 3 + 2) * 0.52,
    phase: Math.floor(hash(i * 3 + 3) * 12),
    bright: i % 5 === 0
  }));

  const clouds: Cloud[] = [
    { x: 0.12, ny: 0.24, w: 18, speed: 0.0022, layer: 0 },
    { x: 0.56, ny: 0.18, w: 22, speed: 0.0017, layer: 0 },
    { x: 0.82, ny: 0.34, w: 16, speed: 0.0026, layer: 1 },
    { x: 0.34, ny: 0.42, w: 14, speed: 0.0031, layer: 1 }
  ];

  // Smoothstep quantized to discrete pixel steps for the hero sunrise-to-zenith climb
  function steppedEaseOut(t: number, steps: number): number {
    const clamped = Math.max(0, Math.min(1, t));
    const q = Math.floor(clamped * steps) / steps;
    return 1 - Math.pow(1 - q, 2);
  }

  function getLocalArcProgress(): { t: number; isDayOrb: boolean } {
    const now = new Date();
    const hour = now.getHours() + now.getMinutes() / 60;
    if (hour >= 6 && hour <= 18) {
      return { t: (hour - 6) / 12, isDayOrb: true };
    }
    const nightHour = hour > 18 ? hour - 18 : hour + 6;
    return { t: nightHour / 12, isDayOrb: false };
  }

  onMount(() => {
    motionState.init();
    themeState.init();

    const el = canvas;
    if (!el) return;
    const ctx = el.getContext('2d', { alpha: false });
    if (!ctx) return;

    let rafId = 0;
    let lastDraw = 0;
    let startTime = performance.now();
    let hidden = document.hidden;
    let cssW = 0;
    let cssH = 0;
    let dpr = 1;
    let stepScale = pixelSize;

    // Target ~16 fps (well below 30 fps) for authentic stepped pixel motion
    const FRAME_MS = 62;

    function syncSize() {
      if (!el) return;
      const rect = el.getBoundingClientRect();
      cssW = Math.max(1, Math.round(rect.width));
      cssH = Math.max(1, Math.round(rect.height));
      dpr = Math.min(2, window.devicePixelRatio || 1);
      stepScale = Math.max(2, Math.round(pixelSize * dpr));
      const targetW = Math.round(cssW * dpr);
      const targetH = Math.round(cssH * dpr);
      if (el.width !== targetW || el.height !== targetH) {
        el.width = targetW;
        el.height = targetH;
      }
      ctx!.imageSmoothingEnabled = false;
    }

    function pxRect(vx: number, vy: number, vw: number, vh: number, color: string) {
      ctx!.fillStyle = color;
      ctx!.fillRect(
        Math.round(vx) * stepScale,
        Math.round(vy) * stepScale,
        Math.round(vw) * stepScale,
        Math.round(vh) * stepScale
      );
    }

    function drawSun(cx: number, cy: number, rayPulse: boolean, isDayTheme: boolean) {
      const core = '#FFD34D';
      const rim = '#FF9E2C';
      const ray = isDayTheme ? '#F09A1F' : '#FFB938';

      // Rays (pixel-snapped cross + diagonals)
      const rayLen = rayPulse ? 3 : 2;
      pxRect(cx, cy - 6 - rayLen, 1, rayLen, ray);
      pxRect(cx, cy + 7, 1, rayLen, ray);
      pxRect(cx - 6 - rayLen, cy, rayLen, 1, ray);
      pxRect(cx + 7, cy, rayLen, 1, ray);

      pxRect(cx - 5, cy - 5, 1, 1, ray);
      pxRect(cx + 5, cy - 5, 1, 1, ray);
      pxRect(cx - 5, cy + 5, 1, 1, ray);
      pxRect(cx + 5, cy + 5, 1, 1, ray);

      // Outer rim
      pxRect(cx - 2, cy - 4, 5, 9, rim);
      pxRect(cx - 3, cy - 3, 7, 7, rim);
      pxRect(cx - 4, cy - 2, 9, 5, rim);

      // Core
      pxRect(cx - 2, cy - 3, 5, 7, core);
      pxRect(cx - 3, cy - 2, 7, 5, core);

      // Zenith catch-light pixels at top of sun core
      pxRect(cx - 1, cy - 2, 3, 1, '#FFF1A6');
    }

    function drawMoon(cx: number, cy: number, skyBg: string) {
      pxRect(cx - 2, cy - 3, 5, 7, '#F6E7B8');
      pxRect(cx - 3, cy - 2, 7, 5, '#F6E7B8');
      pxRect(cx, cy - 2, 3, 4, skyBg);
      pxRect(cx + 1, cy - 3, 2, 5, skyBg);
    }

    function drawCloud(x: number, y: number, w: number, fill: string, shade: string) {
      const topW = Math.max(4, Math.floor(w * 0.55));
      const topX = x + Math.floor((w - topW) / 2);
      pxRect(topX, y - 2, topW, 2, fill);
      pxRect(x + 1, y, w - 2, 2, fill);
      pxRect(x, y + 1, w, 2, shade);
    }

    function hillHeight(col: number, period: number, amp: number, seed: number): number {
      const a = Math.sin((col / period) * Math.PI * 2 + seed);
      const b = Math.sin((col / (period * 0.43)) * Math.PI * 2 + seed * 1.9) * 0.45;
      const c = Math.cos((col / (period * 0.21)) * Math.PI * 2 + seed * 0.7) * 0.22;
      const raw = (a + b + c + 1.67) / 3.34;
      // Quantize in stepped terraces so hills look voxel/pixel-crafted
      return Math.max(1, Math.round(raw * amp));
    }

    function drawFrame(now: number) {
      syncSize();
      const vw = Math.ceil(el!.width / stepScale);
      const vh = Math.ceil(el!.height / stepScale);
      const elapsedSec = isPaused ? 0 : (now - startTime) / 1000;
      const isDayTheme = themeState.current === 'day';

      // 1. Stepped sky bands
      const skyBands = isDayTheme
        ? ['#7FC5DF', '#9FD5EA', '#BEE4F2', '#D8ECF4']
        : ['#0B1020', '#0F162C', '#121A33', '#1B2747'];
      const bandH = Math.ceil(vh / skyBands.length);
      for (let i = 0; i < skyBands.length; i++) {
        pxRect(0, i * bandH, vw, bandH + 1, skyBands[i]);
      }

      // 2. Stars (night sky)
      if (!isDayTheme) {
        const tick = Math.floor(elapsedSec * 2.2);
        for (const s of stars) {
          const sx = Math.floor(s.nx * vw);
          const sy = Math.floor(s.ny * vh);
          const pulse = (tick + s.phase) % 10;
          if (s.bright && pulse === 0) {
            pxRect(sx, sy, 1, 1, '#F6E7B8');
            pxRect(sx - 1, sy, 1, 1, '#5A76A1');
            pxRect(sx + 1, sy, 1, 1, '#5A76A1');
            pxRect(sx, sy - 1, 1, 1, '#5A76A1');
            pxRect(sx, sy + 1, 1, 1, '#5A76A1');
          } else {
            const col = pulse < 3 ? '#F4F1E8' : pulse < 6 ? '#7A93B8' : '#3C547D';
            pxRect(sx, sy, 1, 1, col);
          }
        }
      }

      // 3. Dotted zenith arc & Sun/Moon
      const horizonY = showHills ? Math.floor(vh * 0.8) : Math.floor(vh * 0.9);
      const isWideHero = mode === 'hero' && cssW >= 900;
      const arcCenterX = isWideHero ? Math.floor(vw * 0.72) : Math.floor(vw * 0.5);
      const arcRx = isWideHero
        ? Math.min(Math.floor(vw * 0.26), Math.floor(vh * 0.95))
        : Math.min(Math.floor(vw * 0.42), Math.floor(vh * 0.92));
      const arcRy = isWideHero
        ? Math.max(22, Math.floor(horizonY * 0.68))
        : Math.max(18, Math.floor(horizonY * 0.74));
      const arcBaseY = isWideHero ? horizonY + 2 : Math.floor(horizonY * 0.86);

      let orbT = 0.5;
      let isSun = true;
      let rayPulse = false;

      if (mode === 'hero') {
        if (isPaused) {
          orbT = 0.5;
        } else {
          const climbDur = 1.45;
          const p = Math.min(1, elapsedSec / climbDur);
          // Rise from eastern slope (0.18) to zenith apex (0.50) in 16 discrete steps
          orbT = 0.18 + steppedEaseOut(p, 16) * 0.32;
          // Single subtle ray pulse right after arriving at zenith
          rayPulse = elapsedSec >= climbDur && elapsedSec < climbDur + 0.55;
        }
      } else {
        const local = getLocalArcProgress();
        orbT = 0.12 + local.t * 0.76;
        isSun = local.isDayOrb || isDayTheme;
      }

      const dotCount = Math.max(18, Math.min(40, Math.floor(arcRx / 3)));
      const arcColorActive = isDayTheme ? '#FFF5CC' : '#F6E7B8';
      const arcColorMuted = isDayTheme ? '#8CB4C7' : '#324B73';

      for (let i = 0; i <= dotCount; i++) {
        const t = i / dotCount;
        const angle = Math.PI * (1 - t);
        const dx = Math.round(arcCenterX + Math.cos(angle) * arcRx);
        const dy = Math.round(arcBaseY - Math.sin(angle) * arcRy);
        if (dy < 3 || dy >= horizonY + 2) continue;
        // Leave clean space immediately around the sun/moon
        const orbAngle = Math.PI * (1 - orbT);
        const ox = Math.round(arcCenterX + Math.cos(orbAngle) * arcRx);
        const oy = Math.round(arcBaseY - Math.sin(orbAngle) * arcRy);
        if (Math.hypot(dx - ox, dy - oy) < 9) continue;

        const lit = mode === 'hero' ? t <= orbT : true;
        pxRect(dx, dy, 1, 1, lit ? arcColorActive : arcColorMuted);
      }

      const orbAngle = Math.PI * (1 - orbT);
      const orbX = Math.round(arcCenterX + Math.cos(orbAngle) * arcRx);
      const orbY = Math.round(arcBaseY - Math.sin(orbAngle) * arcRy);
      if (isSun) {
        drawSun(orbX, orbY, rayPulse, isDayTheme);
      } else {
        drawMoon(orbX, orbY, skyBands[0]);
      }

      // 4. Slow drifting pixel clouds
      const cloudFill = isDayTheme ? '#F5FBFD' : '#1E2E52';
      const cloudShade = isDayTheme ? '#C9E3EE' : '#15213D';
      for (const c of clouds) {
        const drift = isPaused ? 0 : (elapsedSec * c.speed) % 1;
        const rawX = ((c.x + drift) % 1.25) - 0.12;
        const cx = Math.floor(rawX * vw);
        const cy = Math.floor(c.ny * horizonY);
        drawCloud(cx, cy, c.w, cloudFill, cloudShade);
      }

      // 5. Stepped parallax teal/navy hills (echoing logo.svg, zero grass-block colors)
      if (showHills) {
        const farShift = isPaused ? 0 : Math.floor(elapsedSec * 0.45);
        const nearShift = isPaused ? 0 : Math.floor(elapsedSec * 0.9);
        const farCol = isDayTheme ? '#4A769D' : '#182847';
        const farRim = isDayTheme ? '#6A9AB8' : '#233A63';

        // Far ridge
        for (let x = 0; x < vw; x += 2) {
          const h = hillHeight(x + farShift, 78, 12, 0.8);
          const topY = vh - h - 7;
          pxRect(x, topY, 2, 1, farRim);
          pxRect(x, topY + 1, 2, vh - topY, farCol);
        }

        // Near ridge: bright teal crest (#5EE0CF) -> mid teal (#2EA3A8) -> navy (#284A72 / #1F3A5E)
        for (let x = 0; x < vw; x += 2) {
          const h = hillHeight(x + nearShift, 52, 10, 2.4);
          const topY = vh - h - 2;
          pxRect(x, topY, 2, 1, '#5EE0CF');
          pxRect(x, topY + 1, 2, 2, '#2EA3A8');
          pxRect(x, topY + 3, 2, vh - topY, '#284A72');
          // Subtle diagonal pixel texture inside the navy body (matching logo.svg)
          for (let y = topY + 4; y < vh; y += 3) {
            if ((x + y) % 5 === 0) {
              pxRect(x, y, 1, 1, '#1F3A5E');
            }
          }
        }
      }
    }

    function loop(now: number) {
      if (hidden || isPaused) {
        rafId = 0;
        return;
      }
      if (now - lastDraw >= FRAME_MS) {
        lastDraw = now;
        drawFrame(now);
      }
      rafId = requestAnimationFrame(loop);
    }

    function startOrDrawStatic() {
      if (rafId) {
        cancelAnimationFrame(rafId);
        rafId = 0;
      }
      drawFrame(performance.now());
      if (!hidden && !isPaused) {
        rafId = requestAnimationFrame(loop);
      }
    }

    const onVisibilityChange = () => {
      hidden = document.hidden;
      if (!hidden) {
        lastDraw = 0;
        startOrDrawStatic();
      } else if (rafId) {
        cancelAnimationFrame(rafId);
        rafId = 0;
      }
    };

    const onRedraw = () => {
      lastDraw = 0;
      startOrDrawStatic();
    };

    const resizeObserver = new ResizeObserver(() => {
      drawFrame(performance.now());
    });
    resizeObserver.observe(el);
    document.addEventListener('visibilitychange', onVisibilityChange);
    el.addEventListener('zenith-redraw', onRedraw);

    startOrDrawStatic();

    return () => {
      if (rafId) cancelAnimationFrame(rafId);
      resizeObserver.disconnect();
      document.removeEventListener('visibilitychange', onVisibilityChange);
      el.removeEventListener('zenith-redraw', onRedraw);
    };
  });

  // React to pause / theme toggles cleanly
  $effect(() => {
    void isPaused;
    void themeState.current;
    // Triggered reactively when user toggles motion or theme; handled by canvas observer on next paint
    if (canvas) {
      canvas.dispatchEvent(new Event('zenith-redraw'));
    }
  });
</script>

<canvas
  bind:this={canvas}
  class="z-pixel-sky z-pixelated {className}"
  aria-hidden="true"
></canvas>

<style>
  .z-pixel-sky {
    display: block;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
</style>
