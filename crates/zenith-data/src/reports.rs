//! Typed views of the data generator's report files.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

/// `blocks.json`: block id → properties + states.
#[derive(Debug, Deserialize)]
pub struct BlockReport {
    #[serde(default)]
    pub properties: BTreeMap<String, Vec<String>>,
    pub states: Vec<StateReport>,
}

#[derive(Debug, Deserialize)]
pub struct StateReport {
    pub id: u32,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

/// `registries.json`: registry id → entries.
#[derive(Debug, Deserialize)]
pub struct RegistryReport {
    pub protocol_id: u32,
    #[serde(default)]
    pub default: Option<String>,
    pub entries: BTreeMap<String, ProtocolId>,
}

#[derive(Debug, Deserialize)]
pub struct ProtocolId {
    pub protocol_id: u32,
}

/// `packets.json`: state → direction → packet id → protocol id.
pub type PacketReport = BTreeMap<String, BTreeMap<String, BTreeMap<String, ProtocolId>>>;

/// The report set Zenith consumes.
#[derive(Debug)]
pub struct Reports {
    /// Block reports keyed by block id. The *property order* is taken from the raw JSON (see `property_order`).
    pub blocks: BTreeMap<String, BlockReport>,
    /// Property names per block in their original JSON order (it defines how state IDs enumerate).
    pub property_order: BTreeMap<String, Vec<String>>,
    pub registries: BTreeMap<String, RegistryReport>,
    pub packets: PacketReport,
    pub commands_json: String,
}

fn read(dir: &Path, name: &str) -> anyhow::Result<Vec<u8>> {
    let p = dir.join(name);
    fs::read(&p).with_context(|| format!("read report {}", p.display()))
}

impl Reports {
    /// Load the reports from a data generator `reports/` directory.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let blocks_raw = read(dir, "blocks.json")?;
        let blocks: BTreeMap<String, BlockReport> =
            serde_json::from_slice(&blocks_raw).context("parse blocks.json")?;
        // serde_json::Value keeps key order only with preserve_order; re-scan the raw JSON for property order instead.
        let property_order = property_order(&blocks_raw)?;
        let registries = serde_json::from_slice(&read(dir, "registries.json")?)
            .context("parse registries.json")?;
        let packets =
            serde_json::from_slice(&read(dir, "packets.json")?).context("parse packets.json")?;
        let commands_json =
            String::from_utf8(read(dir, "commands.json")?).context("commands.json is not UTF-8")?;
        Ok(Self {
            blocks,
            property_order,
            registries,
            packets,
            commands_json,
        })
    }
}

/// Property names per block, in the order they appear in the raw `blocks.json`.
fn property_order(raw: &[u8]) -> anyhow::Result<BTreeMap<String, Vec<String>>> {
    #[derive(Deserialize)]
    struct Ordered {
        #[serde(default)]
        properties: Option<OrderedKeys>,
    }
    struct OrderedKeys(Vec<String>);
    impl<'de> Deserialize<'de> for OrderedKeys {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = OrderedKeys;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a property map")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut m: A,
                ) -> Result<OrderedKeys, A::Error> {
                    let mut keys = Vec::new();
                    while let Some((k, _)) = m.next_entry::<String, serde::de::IgnoredAny>()? {
                        keys.push(k);
                    }
                    Ok(OrderedKeys(keys))
                }
            }
            d.deserialize_map(V)
        }
    }
    let parsed: BTreeMap<String, Ordered> =
        serde_json::from_slice(raw).context("parse blocks.json (order)")?;
    Ok(parsed
        .into_iter()
        .map(|(k, v)| (k, v.properties.map(|p| p.0).unwrap_or_default()))
        .collect())
}
