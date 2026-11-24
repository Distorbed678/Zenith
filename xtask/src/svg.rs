//! Pixel-styled SVG tracker cards for the README. Self-contained (no external fonts or images), so they
//! render inside GitHub's sandboxed <img>. Colors follow the Zenith brand palette.

use std::fmt::Write as _;

use crate::progress::Progress;

const SKY_0: &str = "#0B1020";
const SKY_1: &str = "#121A33";
const SKY_2: &str = "#1B2747";
const NAVY: &str = "#284A72";
const NAVY_DEEP: &str = "#1F3A5E";
const TEAL: &str = "#5EE0CF";
const TEAL_DEEP: &str = "#2EA3A8";
const SUN: &str = "#FFD34D";
const SUN_DEEP: &str = "#FF9E2C";
const INK: &str = "#F4F1E8";
const INK_DIM: &str = "#B9B3A3";
const FONT: &str =
    "'Pixelify Sans', ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";
const LOGO: &str = include_str!("../../docs/assets/brand/logo.svg");

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct Canvas {
    w: u32,
    h: u32,
    body: String,
}

impl Canvas {
    fn new(w: u32, h: u32, title: &str) -> Self {
        let mut c = Canvas {
            w,
            h,
            body: String::new(),
        };
        let _ = write!(c.body, "<title>{}</title>", esc(title));
        c.background();
        c
    }

    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, fill: &str) {
        let _ = write!(
            self.body,
            r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}"/>"#
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: u32, y: u32, size: u32, fill: &str, weight: u32, anchor: &str, s: &str) {
        let _ = write!(
            self.body,
            r#"<text x="{x}" y="{y}" font-family="{FONT}" font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}">{}</text>"#,
            esc(s)
        );
    }

    /// Night sky with a stepped pixel frame and a deterministic starfield.
    fn background(&mut self) {
        let (w, h) = (self.w, self.h);
        // notched outer frame (pixel corners)
        self.rect(4, 0, w - 8, h, NAVY);
        self.rect(0, 4, w, h - 8, NAVY);
        self.rect(8, 4, w - 16, h - 8, SKY_0);
        self.rect(4, 8, w - 8, h - 16, SKY_0);
        // sky bands
        self.rect(8, (h * 2) / 3, w - 16, h / 3 - 8, SKY_1);
        let mut seed: u32 = 0x9E37_79B9 ^ (w * 31 + h);
        for _ in 0..(w / 18) {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = 12 + (seed >> 8) % (w - 24);
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let y = 12 + (seed >> 8) % (h - 24);
            let c = if seed.is_multiple_of(5) { SUN } else { INK_DIM };
            let _ = write!(
                self.body,
                r#"<rect x="{x}" y="{y}" width="2" height="2" fill="{c}" opacity="0.45"/>"#
            );
        }
    }

    /// Segmented pixel bar: `segments` cells, `pct` filled.
    #[allow(clippy::too_many_arguments)]
    fn bar(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        segments: u32,
        pct: f64,
        fill: &str,
        fill_edge: &str,
    ) {
        self.rect(x, y, w, h, NAVY_DEEP);
        let gap = 2;
        let cell = (w - 4 - gap * (segments - 1)) / segments;
        let filled = ((pct / 100.0) * segments as f64).round() as u32;
        let filled = if pct > 0.0 { filled.max(1) } else { 0 };
        for i in 0..segments {
            let cx = x + 2 + i * (cell + gap);
            if i < filled {
                self.rect(cx, y + 2, cell, h - 4, fill);
                self.rect(cx, y + h - 2 - 3, cell, 3, fill_edge);
            } else {
                self.rect(cx, y + 2, cell, h - 4, SKY_2);
            }
        }
    }

    fn logo(&mut self, x: u32, y: u32, scale: u32) {
        let start = LOGO.find("</title>").map(|i| i + 8).unwrap_or(0);
        let end = LOGO.rfind("</svg>").unwrap_or(LOGO.len());
        let _ = write!(
            self.body,
            r#"<g transform="translate({x} {y}) scale({scale})">{}</g>"#,
            &LOGO[start..end]
        );
    }

    fn finish(self) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" shape-rendering="crispEdges">{b}</svg>
"#,
            w = self.w,
            h = self.h,
            b = self.body
        )
    }
}

fn status_label(s: &str) -> &'static str {
    match s {
        "done" => "Done",
        "in-progress" => "In progress",
        _ => "Planned",
    }
}

fn overall(p: &Progress) -> String {
    let mut c = Canvas::new(880, 232, "Zenith overall progress");
    c.logo(32, 36, 4);
    c.text(176, 74, 40, INK, 700, "start", &p.project);
    c.text(
        176,
        102,
        16,
        INK_DIM,
        400,
        "start",
        &format!(
            "Rebuilding Java Edition {} in Rust, verified against your own copy",
            p.target_version
        ),
    );
    c.text(
        848,
        74,
        40,
        SUN,
        700,
        "end",
        &format!("{:.1}%", p.overall.percent),
    );
    c.text(
        848,
        98,
        14,
        INK_DIM,
        400,
        "end",
        "overall, weighted by effort",
    );

    c.text(176, 140, 14, INK, 600, "start", "Built");
    c.bar(260, 126, 588, 20, 48, p.overall.percent, SUN, SUN_DEEP);
    c.text(176, 176, 14, INK, 600, "start", "Proven 1:1");
    c.bar(
        260,
        162,
        588,
        20,
        48,
        p.overall.parity_percent,
        TEAL,
        TEAL_DEEP,
    );

    let stats = [
        format!(
            "Milestones done {} of {}",
            p.overall.milestones_done, p.overall.milestones_total
        ),
        format!(
            "Parity suites passing {} of {}",
            p.overall.suites_passing, p.overall.suites_total
        ),
        format!("Protocol {}", p.protocol),
        format!("World version {}", p.world_version),
    ];
    for (x, s) in [32u32, 260, 520, 680].into_iter().zip(&stats) {
        c.text(x, 212, 14, INK_DIM, 400, "start", s);
    }
    c.finish()
}

fn phases(p: &Progress) -> String {
    let row = 44;
    let h = 72 + row * p.phases.len() as u32 + 8;
    let mut c = Canvas::new(880, h, "Zenith progress by phase");
    c.text(32, 46, 22, INK, 700, "start", "Phases");
    c.text(
        848,
        46,
        14,
        INK_DIM,
        400,
        "end",
        "Built first, in this order",
    );
    for (i, ph) in p.phases.iter().enumerate() {
        let y = 72 + row * i as u32;
        c.text(32, y + 20, 15, SUN, 700, "start", &ph.id);
        c.text(72, y + 20, 15, INK, 600, "start", &ph.title);
        c.bar(300, y + 6, 470, 18, 32, ph.percent, SUN, SUN_DEEP);
        c.text(
            848,
            y + 20,
            15,
            INK,
            700,
            "end",
            &format!("{:.1}%", ph.percent),
        );
    }
    c.finish()
}

fn milestones(p: &Progress) -> String {
    let cols = 11u32;
    let (tw, th, gap) = (68u32, 60u32, 8u32);
    let rows = (p.milestones.len() as u32).div_ceil(cols);
    let h = 84 + rows * (th + gap) + 40;
    let mut c = Canvas::new(880, h, "Zenith milestones");
    c.text(32, 46, 22, INK, 700, "start", "Milestones");
    c.text(
        848,
        46,
        14,
        INK_DIM,
        400,
        "end",
        "Each tile fills as its tasks and parity suites pass",
    );
    for (i, m) in p.milestones.iter().enumerate() {
        let (col, row) = (i as u32 % cols, i as u32 / cols);
        let x = 32 + col * (tw + gap);
        let y = 68 + row * (th + gap);
        let (edge, base) = match m.status.as_str() {
            "done" => (TEAL, TEAL_DEEP),
            "in-progress" => (SUN, SKY_2),
            _ => (NAVY, SKY_2),
        };
        c.rect(x + 2, y, tw - 4, th, edge);
        c.rect(x, y + 2, tw, th - 4, edge);
        c.rect(x + 2, y + 2, tw - 4, th - 4, base);
        if m.status != "done" && m.percent > 0.0 {
            let fh = (((th - 4) as f64) * m.percent / 100.0).round().max(2.0) as u32;
            c.rect(
                x + 2,
                y + th - 2 - fh,
                tw - 4,
                fh,
                if m.status == "in-progress" {
                    SUN_DEEP
                } else {
                    NAVY
                },
            );
        }
        let label = if m.status == "in-progress" || m.status == "done" {
            INK
        } else {
            INK_DIM
        };
        c.text(x + 8, y + 22, 15, label, 700, "start", &m.id);
        c.text(
            x + tw - 8,
            y + th - 10,
            12,
            label,
            600,
            "end",
            &format!("{:.0}%", m.percent),
        );
        let _ = write!(
            c.body,
            "<title>{}</title>",
            esc(&format!(
                "{}: {} ({})",
                m.id,
                m.title,
                status_label(&m.status)
            ))
        );
    }
    let ly = h - 28;
    for (j, (col, name)) in [(SUN, "In progress"), (TEAL, "Done"), (NAVY, "Planned")]
        .iter()
        .enumerate()
    {
        let x = 32 + j as u32 * 150;
        c.rect(x, ly - 12, 14, 14, col);
        c.text(x + 22, ly, 13, INK_DIM, 400, "start", name);
    }
    c.finish()
}

fn history(p: &Progress) -> String {
    let (w, h) = (880u32, 220u32);
    let mut c = Canvas::new(w, h, "Zenith progress over time");
    c.text(32, 46, 22, INK, 700, "start", "Over time");
    let (x0, x1, y0, y1) = (72u32, 848u32, 180u32, 68u32);
    for pct in [0, 50, 100] {
        let y = y0 - (y0 - y1) * pct / 100;
        c.rect(x0, y, x1 - x0, 2, SKY_2);
        c.text(x0 - 8, y + 5, 12, INK_DIM, 400, "end", &format!("{pct}%"));
    }
    let n = p.history.len().max(1) as u32;
    let px = |i: u32| {
        if n == 1 {
            (x0 + x1) / 2
        } else {
            x0 + (x1 - x0) * i / (n - 1)
        }
    };
    let py = |v: f64| y0 - ((y0 - y1) as f64 * v.clamp(0.0, 100.0) / 100.0).round() as u32;
    for (series, col) in [(0usize, SUN), (1usize, TEAL)] {
        let mut prev: Option<(u32, u32)> = None;
        for (i, hp) in p.history.iter().enumerate() {
            let v = if series == 0 {
                hp.percent
            } else {
                hp.parity_percent
            };
            let (x, y) = (px(i as u32), py(v));
            if let Some((pxv, pyv)) = prev {
                c.rect(pxv, pyv - 1, x - pxv, 3, col); // step: horizontal, then vertical
                let (top, bot) = (y.min(pyv), y.max(pyv));
                c.rect(x - 1, top - 1, 3, bot - top + 3, col);
            }
            c.rect(x - 4, y - 4, 8, 8, col);
            prev = Some((x, y));
        }
    }
    if let (Some(first), Some(last)) = (p.history.first(), p.history.last()) {
        c.text(x0, h - 14, 12, INK_DIM, 400, "start", &first.date);
        if p.history.len() > 1 {
            c.text(x1, h - 14, 12, INK_DIM, 400, "end", &last.date);
        }
    }
    c.rect(560, 34, 12, 12, SUN);
    c.text(578, 45, 13, INK_DIM, 400, "start", "Built");
    c.rect(660, 34, 12, 12, TEAL);
    c.text(678, 45, 13, INK_DIM, 400, "start", "Proven 1:1");
    c.finish()
}

/// (file name, svg body) for every tracker card.
pub fn render_all(p: &Progress) -> Vec<(&'static str, String)> {
    vec![
        ("overall.svg", overall(p)),
        ("phases.svg", phases(p)),
        ("milestones.svg", milestones(p)),
        ("history.svg", history(p)),
    ]
}
