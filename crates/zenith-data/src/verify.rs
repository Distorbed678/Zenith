//! Parity suite `datagen-ingest`: the cached tables must reproduce every fact in the data generator's reports.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;

use crate::reports::Reports;
use crate::tables::Tables;
use crate::version_dir;

pub use zenith_parity::SuiteResult;

#[derive(Debug, Default)]
pub struct Check {
    pub passed: u64,
    pub total: u64,
    /// The first few failures, for the report.
    pub failures: Vec<String>,
}

impl Check {
    fn record(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if ok {
            self.passed += 1;
        } else if self.failures.len() < 20 {
            self.failures.push(what());
        }
    }
}

/// Compare `tables` against `reports`, one check per block, state, registry, registry entry, packet, and command tree.
pub fn compare(tables: &Tables, reports: &Reports) -> Check {
    let mut c = Check::default();

    for (name, rb) in &reports.blocks {
        let tb = tables.block(name);
        let ok = tb.is_some_and(|tb| {
            let props: BTreeMap<&String, &Vec<String>> =
                tb.properties.iter().map(|p| (&p.name, &p.values)).collect();
            let want: BTreeMap<&String, &Vec<String>> = rb.properties.iter().collect();
            props == want
                && tb.state_count as usize == rb.states.len()
                && rb
                    .states
                    .iter()
                    .any(|s| s.default && s.id == tb.default_state)
        });
        c.record(ok, || format!("block {name}"));

        for s in &rb.states {
            let ok = tables.states.get(s.id as usize).is_some_and(|row| {
                let block = &tables.blocks[row.block as usize];
                block.name == *name
                    && block.properties.len() == row.values.len()
                    && block.properties.iter().zip(&row.values).all(|(p, &vi)| {
                        s.properties
                            .get(&p.name)
                            .is_some_and(|v| p.values.get(vi as usize) == Some(v))
                    })
            });
            c.record(ok, || format!("state {} of {name}", s.id));
        }
    }

    for (name, rr) in &reports.registries {
        let tr = tables.registry(name);
        c.record(
            tr.is_some_and(|tr| tr.protocol_id == rr.protocol_id && tr.default == rr.default),
            || format!("registry {name}"),
        );
        for (entry, id) in &rr.entries {
            let ok = tr.is_some_and(|tr| {
                tr.entries
                    .get(id.protocol_id as usize)
                    .is_some_and(|e| e.name == *entry && e.protocol_id == id.protocol_id)
            });
            c.record(ok, || format!("registry {name} entry {entry}"));
        }
    }

    for (state, dirs) in &reports.packets {
        for (dir, packets) in dirs {
            for (name, id) in packets {
                let ok = tables.packets.iter().any(|p| {
                    p.state == *state
                        && p.direction == *dir
                        && p.name == *name
                        && p.protocol_id == id.protocol_id
                });
                c.record(ok, || format!("packet {state}/{dir}/{name}"));
            }
        }
    }

    c.record(tables.commands_json == reports.commands_json, || {
        "command tree".to_owned()
    });
    c
}

/// Run the suite against the cached tables and reports of `version`. Optionally write the result file.
pub fn run(version: &str, results_dir: Option<&Path>) -> anyhow::Result<(SuiteResult, Check)> {
    let dir = version_dir(version);
    let tables = Tables::load(&dir.join("tables.bin"))
        .context("no tables cached; run `zenith-data ingest` first")?;
    let reports = Reports::load(&dir.join("reports"))?;
    let check = compare(&tables, &reports);
    let result = SuiteResult::new("datagen-ingest", version, check.passed, check.total);
    if let Some(out) = results_dir {
        result.write(out)?;
    }
    Ok((result, check))
}
