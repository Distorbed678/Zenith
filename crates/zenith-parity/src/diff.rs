//! Find the first divergence between two dumps.

use std::collections::VecDeque;
use std::path::Path;

use crate::dump::{DumpReader, Record, Value};

/// How many preceding records to keep for context.
const CONTEXT: usize = 5;

#[derive(Debug, Clone)]
pub struct Divergence {
    /// Index of the first differing record.
    pub index: u64,
    pub what: String,
    pub left: Option<Record>,
    pub right: Option<Record>,
    /// Up to `CONTEXT` identical records before the divergence.
    pub context: Vec<Record>,
}

#[derive(Debug, Clone)]
pub struct DiffReport {
    pub compared: u64,
    pub divergence: Option<Divergence>,
}

fn describe(a: &Record, b: &Record) -> Option<String> {
    if a.tick != b.tick {
        return Some(format!("tick {} vs {}", a.tick, b.tick));
    }
    if a.phase != b.phase {
        return Some(format!("phase {} vs {}", a.phase.name(), b.phase.name()));
    }
    if a.subject != b.subject {
        return Some(format!("subject {:?} vs {:?}", a.subject, b.subject));
    }
    for (i, (ka, va)) in a.fields.iter().enumerate() {
        match b.fields.get(i) {
            Some((kb, _)) if kb != ka => return Some(format!("field #{i} is {ka:?} vs {kb:?}")),
            Some((_, vb)) if vb != va => return Some(format!("{ka}: {va} vs {vb}")),
            None => return Some(format!("field {ka:?} missing on the right")),
            _ => {}
        }
    }
    (b.fields.len() > a.fields.len())
        .then(|| format!("extra field {:?} on the right", b.fields[a.fields.len()].0))
}

/// Compare two record streams.
pub fn diff_records(
    left: impl IntoIterator<Item = anyhow::Result<Record>>,
    right: impl IntoIterator<Item = anyhow::Result<Record>>,
) -> anyhow::Result<DiffReport> {
    let (mut l, mut r) = (left.into_iter(), right.into_iter());
    let mut ctx: VecDeque<Record> = VecDeque::with_capacity(CONTEXT);
    let mut index = 0u64;
    loop {
        let (a, b) = (l.next().transpose()?, r.next().transpose()?);
        let what = match (&a, &b) {
            (None, None) => {
                return Ok(DiffReport {
                    compared: index,
                    divergence: None,
                });
            }
            (Some(_), None) => Some("right dump ends early".to_owned()),
            (None, Some(_)) => Some("left dump ends early".to_owned()),
            (Some(a), Some(b)) => describe(a, b),
        };
        if let Some(what) = what {
            return Ok(DiffReport {
                compared: index,
                divergence: Some(Divergence {
                    index,
                    what,
                    left: a,
                    right: b,
                    context: ctx.into(),
                }),
            });
        }
        if ctx.len() == CONTEXT {
            ctx.pop_front();
        }
        ctx.push_back(a.expect("both present"));
        index += 1;
    }
}

/// Compare two dump files (headers may differ in `source`; everything else must match).
pub fn diff_files(left: &Path, right: &Path) -> anyhow::Result<DiffReport> {
    let (a, b) = (DumpReader::open(left)?, DumpReader::open(right)?);
    anyhow::ensure!(
        a.header.version == b.header.version,
        "dumps are for different game versions"
    );
    anyhow::ensure!(a.header.seed == b.header.seed, "dumps use different seeds");
    diff_records(a, b)
}

/// Human-readable rendering of a record (for reports).
pub fn render(r: &Record) -> String {
    let fields: Vec<String> = r.fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!(
        "t{} {} {} {{{}}}",
        r.tick,
        r.phase.name(),
        r.subject,
        fields.join(", ")
    )
}

/// Convenience for tests.
pub fn same(a: &Value, b: &Value) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dump::Phase;

    fn rec(tick: u64, v: i64) -> anyhow::Result<Record> {
        Ok(Record::new(tick, Phase::TICK_END, "server").with("tickCount", Value::I64(v)))
    }

    #[test]
    fn identical_streams() {
        let r = diff_records(
            (0..10).map(|t| rec(t, t as i64)),
            (0..10).map(|t| rec(t, t as i64)),
        )
        .unwrap();
        assert_eq!(r.compared, 10);
        assert!(r.divergence.is_none());
    }

    #[test]
    fn reports_first_divergence_with_context() {
        let left = (0..10).map(|t| rec(t, t as i64));
        let right = (0..10).map(|t| rec(t, if t >= 6 { 99 } else { t as i64 }));
        let d = diff_records(left, right).unwrap().divergence.unwrap();
        assert_eq!(d.index, 6);
        assert!(d.what.contains("tickCount"), "{}", d.what);
        assert_eq!(d.context.len(), 5);
    }

    #[test]
    fn length_mismatch() {
        let d = diff_records((0..3).map(|t| rec(t, 0)), (0..2).map(|t| rec(t, 0)))
            .unwrap()
            .divergence
            .unwrap();
        assert_eq!(d.index, 2);
        assert!(d.what.contains("right dump ends early"));
    }
}
