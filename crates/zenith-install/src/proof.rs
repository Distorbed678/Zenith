//! Combine every ownership check into one report.

use serde::Serialize;

use crate::assets::verify_assets;
use crate::discover::{GameInstall, discover_installs};
use crate::evidence::{self, Signal};
use crate::jar::{JarVersionInfo, read_jar_version, sha1_file};
use crate::manifest::version_facts;

#[derive(Debug, Clone)]
pub struct ProofOptions {
    pub version: String,
    /// Never touch the network: use cached or built-in manifest facts.
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CheckStatus {
    Pass,
    Fail,
    /// Not active yet (e.g. Microsoft sign-in before an Azure app is approved). Never counts as a pass or a fail.
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub id: &'static str,
    pub title: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnershipReport {
    pub version: String,
    pub install: Option<GameInstall>,
    pub jar: Option<JarVersionInfo>,
    pub checks: Vec<CheckResult>,
    /// True only when every non-skipped check passed.
    pub passed: bool,
}

fn check(
    id: &'static str,
    title: &'static str,
    status: CheckStatus,
    detail: impl Into<String>,
) -> CheckResult {
    CheckResult {
        id,
        title,
        status,
        detail: detail.into(),
    }
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn prove_one(
    install: &GameInstall,
    opts: &ProofOptions,
) -> (Option<JarVersionInfo>, Vec<CheckResult>) {
    use CheckStatus::*;
    let mut checks = vec![check(
        "install",
        "Game install found",
        Pass,
        format!(
            "{} at {}",
            install.launcher.label(),
            install.client_jar.display()
        ),
    )];

    let facts = match version_facts(&opts.version, opts.offline) {
        Ok(f) => {
            checks.push(check(
                "facts",
                "Official version facts",
                Pass,
                format!("source: {:?}", f.source),
            ));
            Some(f)
        }
        Err(e) => {
            checks.push(check(
                "facts",
                "Official version facts",
                Fail,
                format!("{e:#}"),
            ));
            None
        }
    };

    // 1. Official jar hash
    match (&facts, sha1_file(&install.client_jar)) {
        (Some(f), Ok(h)) if h == f.client_sha1 => checks.push(check(
            "jar-hash",
            "Official jar hash",
            Pass,
            format!("SHA-1 {h} matches Mojang's manifest"),
        )),
        (Some(f), Ok(h)) => checks.push(check(
            "jar-hash",
            "Official jar hash",
            Fail,
            format!("SHA-1 {h} does not match the official {}", f.client_sha1),
        )),
        (_, Err(e)) => checks.push(check(
            "jar-hash",
            "Official jar hash",
            Fail,
            format!("cannot read jar: {e}"),
        )),
        (None, Ok(_)) => checks.push(check(
            "jar-hash",
            "Official jar hash",
            Fail,
            "no official hash to compare against",
        )),
    }

    let jar = match read_jar_version(&install.client_jar) {
        Ok(j) if j.id == opts.version => {
            checks.push(check(
                "jar-version",
                "Jar reports the right version",
                Pass,
                format!(
                    "{} (world {}, protocol {})",
                    j.id, j.world_version, j.protocol_version
                ),
            ));
            Some(j)
        }
        Ok(j) => {
            checks.push(check(
                "jar-version",
                "Jar reports the right version",
                Fail,
                format!("jar says {}", j.id),
            ));
            None
        }
        Err(e) => {
            checks.push(check(
                "jar-version",
                "Jar reports the right version",
                Fail,
                format!("{e:#}"),
            ));
            None
        }
    };

    // 2. Full asset set verified
    match &facts {
        Some(f) => match verify_assets(&install.assets_dir, &f.asset_index_id, &f.asset_index_sha1) {
            Ok(r) if r.ok() => checks.push(check(
                "assets",
                "Full asset set verified",
                Pass,
                format!("index {}: {} objects, {:.0} MiB, all hashes match", r.index_id, r.unique_blobs, mib(r.total_bytes)),
            )),
            Ok(r) => checks.push(check(
                "assets",
                "Full asset set verified",
                Fail,
                format!(
                    "index {}: {} missing, {} corrupt (e.g. {}); launch this version once in your launcher",
                    r.index_id,
                    r.missing,
                    r.corrupt,
                    r.examples.join(", ")
                ),
            )),
            Err(e) => checks.push(check("assets", "Full asset set verified", Fail, format!("{e:#}"))),
        },
        None => checks.push(check("assets", "Full asset set verified", Fail, "no official asset index to compare against")),
    }

    // 3. Launch evidence
    match &jar {
        Some(j) => {
            let ev = evidence::collect(&install.game_dirs, &opts.version, j.world_version);
            match ev.iter().find(|e| e.is_sufficient()) {
                Some(e) => {
                    let kinds: Vec<&str> = e
                        .signals
                        .iter()
                        .map(|s| match s {
                            Signal::OptionsDataVersion { .. } => "options.txt data version",
                            Signal::LogMentionsVersion { .. } => "game log",
                            Signal::LauncherLastLaunch { .. } => "launcher last-launch time",
                        })
                        .collect();
                    checks.push(check(
                        "launch-evidence",
                        "Launched at least once",
                        Pass,
                        format!("{} ({})", e.game_dir.display(), kinds.join(", ")),
                    ));
                }
                None => checks.push(check(
                    "launch-evidence",
                    "Launched at least once",
                    Fail,
                    format!(
                        "no instance of {} shows it was started ({} checked); launch it once from your launcher",
                        opts.version,
                        install.game_dirs.len()
                    ),
                )),
            }
        }
        None => checks.push(check(
            "launch-evidence",
            "Launched at least once",
            Fail,
            "jar version unknown",
        )),
    }

    // 4. Microsoft account (pluggable; activates with an approved Azure app ID, milestone M23)
    checks.push(check(
        "microsoft",
        "Microsoft account owns the game",
        Skipped,
        "activates once an approved Azure app ID is configured (milestone M23)",
    ));

    (jar, checks)
}

fn all_passed(checks: &[CheckResult]) -> bool {
    checks.iter().all(|c| c.status != CheckStatus::Fail)
}

/// Run every check against each install of the version, and return the first one that passes (or the best attempt).
pub fn prove_ownership(opts: &ProofOptions) -> OwnershipReport {
    let installs = discover_installs(Some(&opts.version));
    if installs.is_empty() {
        return OwnershipReport {
            version: opts.version.clone(),
            install: None,
            jar: None,
            checks: vec![check(
                "install",
                "Game install found",
                CheckStatus::Fail,
                format!(
                    "no launcher install of {} found; install and launch it once with Prism, MultiMC or the official \
                     launcher (or set ZENITH_LAUNCHER_ROOT)",
                    opts.version
                ),
            )],
            passed: false,
        };
    }
    let mut best: Option<OwnershipReport> = None;
    for install in installs {
        let (jar, checks) = prove_one(&install, opts);
        let passed = all_passed(&checks);
        let fails = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .count();
        let report = OwnershipReport {
            version: opts.version.clone(),
            install: Some(install),
            jar,
            checks,
            passed,
        };
        if passed {
            return report;
        }
        let better = best.as_ref().is_none_or(|b| {
            fails
                < b.checks
                    .iter()
                    .filter(|c| c.status == CheckStatus::Fail)
                    .count()
        });
        if better {
            best = Some(report);
        }
    }
    best.expect("at least one install")
}
