//! milestones.toml + parity results → progress.json, history.json and SVG tracker.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::repo_root;
use crate::svg;

// ── input ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Source {
    meta: Meta,
    phase: Vec<PhaseIn>,
    milestone: Vec<MilestoneIn>,
}

#[derive(Deserialize)]
struct Meta {
    project: String,
    target_version: String,
    protocol: u32,
    world_version: u32,
}

#[derive(Deserialize)]
struct PhaseIn {
    id: String,
    title: String,
    blurb: String,
}

#[derive(Deserialize)]
struct MilestoneIn {
    id: String,
    phase: String,
    title: String,
    weight: f64,
    status: String,
    #[serde(default)]
    suites: Vec<String>,
    tasks: Vec<TaskIn>,
}

#[derive(Deserialize)]
struct TaskIn {
    t: String,
    done: bool,
}

/// One parity suite's latest result, written by the parity runners to progress/parity/<suite>.json.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuiteResult {
    suite: String,
    passed: u64,
    total: u64,
    updated_at: Option<String>,
}

// ── output (schema v1, consumed by the website) ──────────────────────────────

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub schema_version: u32,
    pub generated_at: String,
    pub project: String,
    pub target_version: String,
    pub protocol: u32,
    pub world_version: u32,
    pub overall: Overall,
    pub phases: Vec<Phase>,
    pub milestones: Vec<Milestone>,
    pub history: Vec<HistoryPoint>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Overall {
    pub percent: f64,
    pub parity_percent: f64,
    pub milestones_done: usize,
    pub milestones_total: usize,
    pub suites_passing: usize,
    pub suites_total: usize,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    pub id: String,
    pub title: String,
    pub blurb: String,
    pub percent: f64,
    pub weight: f64,
    pub milestones: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub id: String,
    pub phase: String,
    pub title: String,
    pub status: String,
    pub weight: f64,
    pub percent: f64,
    pub tasks_done: usize,
    pub tasks_total: usize,
    pub tasks: Vec<Task>,
    pub suites: Vec<Suite>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Task {
    pub title: String,
    pub done: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Suite {
    pub id: String,
    pub passed: u64,
    pub total: u64,
    pub percent: f64,
    pub updated_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub date: String,
    pub percent: f64,
    pub parity_percent: f64,
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn weighted_mean(items: impl Iterator<Item = (f64, f64)>) -> f64 {
    let (sum, w) = items.fold((0.0, 0.0), |(s, w), (p, wt)| (s + p * wt, w + wt));
    if w == 0.0 { 0.0 } else { sum / w }
}

fn load_suites(dir: &Path) -> anyhow::Result<BTreeMap<String, SuiteResult>> {
    let mut out = BTreeMap::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return Ok(out);
    };
    for e in rd {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "json") {
            let r: SuiteResult = serde_json::from_slice(&fs::read(&p)?)
                .with_context(|| format!("parse {}", p.display()))?;
            out.insert(r.suite.clone(), r);
        }
    }
    Ok(out)
}

fn build(
    src: Source,
    results: &BTreeMap<String, SuiteResult>,
    prior_history: Vec<HistoryPoint>,
) -> anyhow::Result<Progress> {
    let mut milestones = Vec::new();
    for m in src.milestone {
        if !src.phase.iter().any(|p| p.id == m.phase) {
            bail!("milestone {} references unknown phase {}", m.id, m.phase);
        }
        if !["planned", "in-progress", "done"].contains(&m.status.as_str()) {
            bail!("milestone {} has invalid status {:?}", m.id, m.status);
        }
        let tasks_done = m.tasks.iter().filter(|t| t.done).count();
        let task_ratio = if m.tasks.is_empty() {
            0.0
        } else {
            tasks_done as f64 / m.tasks.len() as f64
        };
        let suites: Vec<Suite> = m
            .suites
            .iter()
            .map(|id| {
                let r = results.get(id);
                let (passed, total) = r.map_or((0, 0), |r| (r.passed, r.total));
                Suite {
                    id: id.clone(),
                    passed,
                    total,
                    percent: if total == 0 {
                        0.0
                    } else {
                        round1(100.0 * passed as f64 / total as f64)
                    },
                    updated_at: r.and_then(|r| r.updated_at.clone()),
                }
            })
            .collect();
        let suite_ratio = if suites.is_empty() {
            None
        } else {
            Some(suites.iter().map(|s| s.percent / 100.0).sum::<f64>() / suites.len() as f64)
        };
        let ratio = suite_ratio.map_or(task_ratio, |s| (task_ratio + s) / 2.0);
        if m.status == "done"
            && (tasks_done != m.tasks.len() || suite_ratio.is_some_and(|s| s < 1.0))
        {
            bail!(
                "milestone {} is marked done but has open tasks or suites below 100%",
                m.id
            );
        }
        milestones.push(Milestone {
            id: m.id,
            phase: m.phase,
            title: m.title,
            status: m.status,
            weight: m.weight,
            percent: round1(100.0 * ratio),
            tasks_done,
            tasks_total: m.tasks.len(),
            tasks: m
                .tasks
                .into_iter()
                .map(|t| Task {
                    title: t.t,
                    done: t.done,
                })
                .collect(),
            suites,
        });
    }

    let phases = src
        .phase
        .into_iter()
        .map(|p| {
            let ms: Vec<&Milestone> = milestones.iter().filter(|m| m.phase == p.id).collect();
            Phase {
                percent: round1(weighted_mean(ms.iter().map(|m| (m.percent, m.weight)))),
                weight: ms.iter().map(|m| m.weight).sum(),
                milestones: ms.iter().map(|m| m.id.clone()).collect(),
                id: p.id,
                title: p.title,
                blurb: p.blurb,
            }
        })
        .collect();

    let all_suites: Vec<&Suite> = milestones.iter().flat_map(|m| &m.suites).collect();
    let overall = Overall {
        percent: round1(weighted_mean(
            milestones.iter().map(|m| (m.percent, m.weight)),
        )),
        parity_percent: if all_suites.is_empty() {
            0.0
        } else {
            round1(all_suites.iter().map(|s| s.percent).sum::<f64>() / all_suites.len() as f64)
        },
        milestones_done: milestones.iter().filter(|m| m.status == "done").count(),
        milestones_total: milestones.len(),
        suites_passing: all_suites
            .iter()
            .filter(|s| s.total > 0 && s.passed == s.total)
            .count(),
        suites_total: all_suites.len(),
    };

    let mut history = prior_history;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let point = HistoryPoint {
        date: today.clone(),
        percent: overall.percent,
        parity_percent: overall.parity_percent,
    };
    match history.last_mut() {
        Some(last) if last.date == today => *last = point,
        Some(last)
            if last.percent == point.percent && last.parity_percent == point.parity_percent => {}
        _ => history.push(point),
    }

    Ok(Progress {
        schema_version: 1,
        generated_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        project: src.meta.project,
        target_version: src.meta.target_version,
        protocol: src.meta.protocol,
        world_version: src.meta.world_version,
        overall,
        phases,
        milestones,
        history,
    })
}

const TABLE_START: &str = "<!-- tracker:start -->";
const TABLE_END: &str = "<!-- tracker:end -->";

/// Replace the generated milestone table between the tracker markers in README.md.
fn readme_with_table(readme: &str, p: &Progress) -> anyhow::Result<String> {
    let (Some(a), Some(b)) = (readme.find(TABLE_START), readme.find(TABLE_END)) else {
        bail!("README.md is missing the {TABLE_START} / {TABLE_END} markers");
    };
    let bar = |pct: f64| {
        let filled = ((pct / 10.0).round() as usize).min(10);
        format!("{}{}", "▰".repeat(filled), "▱".repeat(10 - filled))
    };
    let mut t = String::new();
    t.push_str(TABLE_START);
    t.push_str("\n\n_Generated by `cargo xtask progress`. Don't edit by hand._\n");
    for ph in &p.phases {
        t.push_str(&format!(
            "\n#### {} {}: {:.1}%\n\n{}\n\n",
            ph.id, ph.title, ph.percent, ph.blurb
        ));
        t.push_str(
            "| Milestone | Status | Progress | Tasks | Parity suites |\n|---|---|---|---|---|\n",
        );
        for m in p.milestones.iter().filter(|m| m.phase == ph.id) {
            let status = match m.status.as_str() {
                "done" => "✅ Done",
                "in-progress" => "🟡 In progress",
                _ => "⬜ Planned",
            };
            let suites = if m.suites.is_empty() {
                "—".to_owned()
            } else {
                m.suites
                    .iter()
                    .map(|s| format!("`{}` {}/{}", s.id, s.passed, s.total))
                    .collect::<Vec<_>>()
                    .join("<br>")
            };
            let tasks = m
                .tasks
                .iter()
                .map(|t| {
                    format!(
                        "{} {}",
                        if t.done { "☑" } else { "☐" },
                        t.title.replace('|', "\\|")
                    )
                })
                .collect::<Vec<_>>()
                .join("<br>");
            t.push_str(&format!(
                "| **{}** {} | {} | `{}` {:.0}% | {} | {} |\n",
                m.id,
                m.title.replace('|', "\\|"),
                status,
                bar(m.percent),
                m.percent,
                tasks,
                suites
            ));
        }
    }
    t.push('\n');
    t.push_str(TABLE_END);
    Ok(format!(
        "{}{}{}",
        &readme[..a],
        t,
        &readme[b + TABLE_END.len()..]
    ))
}

/// Returns Ok(true) on success; in check mode Ok(false) means the outputs are stale.
pub fn run(check: bool) -> anyhow::Result<bool> {
    let root = repo_root();
    let src: Source = toml::from_str(&fs::read_to_string(root.join("progress/milestones.toml"))?)
        .context("parse progress/milestones.toml")?;
    let results = load_suites(&root.join("progress/parity"))?;
    let history_path = root.join("progress/history.json");
    let prior: Vec<HistoryPoint> = match fs::read(&history_path) {
        Ok(b) => serde_json::from_slice(&b).context("parse progress/history.json")?,
        Err(_) => Vec::new(),
    };
    let progress = build(src, &results, prior)?;

    let mut outputs: Vec<(std::path::PathBuf, String)> = vec![
        (
            root.join("progress/progress.json"),
            serde_json::to_string_pretty(&progress)? + "\n",
        ),
        (
            history_path,
            serde_json::to_string_pretty(&progress.history)? + "\n",
        ),
    ];
    for (name, body) in svg::render_all(&progress) {
        outputs.push((root.join("docs/assets/tracker").join(name), body));
    }
    let readme_path = root.join("README.md");
    if let Ok(readme) = fs::read_to_string(&readme_path) {
        outputs.push((readme_path, readme_with_table(&readme, &progress)?));
    }

    if check {
        let mut stale = Vec::new();
        for (path, body) in &outputs {
            let current = fs::read_to_string(path).unwrap_or_default();
            let same = if path.ends_with("progress.json") {
                let strip = |s: &str| -> Option<Progress> {
                    let mut p: Progress = serde_json::from_str(s).ok()?;
                    p.generated_at.clear();
                    Some(p)
                };
                strip(&current).is_some() && strip(&current) == strip(body)
            } else {
                current == *body
            };
            if !same {
                stale.push(
                    path.strip_prefix(&root)
                        .unwrap_or(path)
                        .display()
                        .to_string(),
                );
            }
        }
        if stale.is_empty() {
            println!("progress outputs are up to date");
            return Ok(true);
        }
        eprintln!(
            "stale progress outputs (run `cargo xtask progress`):\n  {}",
            stale.join("\n  ")
        );
        return Ok(false);
    }

    for (path, body) in outputs {
        if let Some(d) = path.parent() {
            fs::create_dir_all(d)?;
        }
        fs::write(&path, body)?;
    }
    println!(
        "overall {:.1}%  parity {:.1}%  milestones {}/{}  suites {}/{}",
        progress.overall.percent,
        progress.overall.parity_percent,
        progress.overall.milestones_done,
        progress.overall.milestones_total,
        progress.overall.suites_passing,
        progress.overall.suites_total
    );
    Ok(true)
}
