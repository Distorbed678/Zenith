//! Suite result files (`progress/parity/<suite>.json`), read by `cargo xtask progress`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteResult {
    pub suite: String,
    pub version: String,
    pub passed: u64,
    pub total: u64,
    /// UTC date, `YYYY-MM-DD`.
    pub updated_at: String,
}

impl SuiteResult {
    pub fn new(suite: &str, version: &str, passed: u64, total: u64) -> Self {
        Self {
            suite: suite.to_owned(),
            version: version.to_owned(),
            passed,
            total,
            updated_at: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        }
    }

    /// Write `<dir>/<suite>.json`.
    pub fn write(&self, dir: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(dir)?;
        fs::write(
            dir.join(format!("{}.json", self.suite)),
            serde_json::to_string_pretty(self)? + "\n",
        )?;
        Ok(())
    }
}
