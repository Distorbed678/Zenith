//! The compiled, versioned tables and their on-disk format.
//!
//! File layout: `b"ZNTB"`, a little-endian `u32` schema version, then the postcard-encoded [`Tables`].

use std::fs;
use std::path::Path;

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::reports::Reports;

/// Bump whenever the [`Tables`] shape changes; old caches are then rebuilt automatically.
pub const SCHEMA: u32 = 1;
const MAGIC: &[u8; 4] = b"ZNTB";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TablesMeta {
    pub version: String,
    pub jar_sha1: String,
    pub world_version: i32,
    pub protocol: i32,
    pub ingested_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub name: String,
    /// Properties in the order that enumerates the block's states.
    pub properties: Vec<Property>,
    pub first_state: u32,
    pub state_count: u32,
    pub default_state: u32,
}

/// One global block state: its block (index into [`Tables::blocks`]) and the value index of each property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateRow {
    pub block: u32,
    pub values: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub name: String,
    pub protocol_id: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Registry {
    pub name: String,
    pub protocol_id: u32,
    pub default: Option<String>,
    /// Sorted by protocol id (dense from 0).
    pub entries: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Packet {
    pub state: String,
    pub direction: String,
    pub name: String,
    pub protocol_id: u32,
}

/// Everything Zenith needs from the data generator for one game version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tables {
    pub meta: TablesMeta,
    /// Sorted by `first_state`.
    pub blocks: Vec<Block>,
    /// Indexed by global block-state id.
    pub states: Vec<StateRow>,
    /// Sorted by registry protocol id.
    pub registries: Vec<Registry>,
    /// Sorted by (state, direction, protocol id).
    pub packets: Vec<Packet>,
    /// The command tree report, kept verbatim until the command milestone (M14) gives it a typed form.
    pub commands_json: String,
}

impl Tables {
    /// Compile reports into tables, checking the invariants the rest of Zenith relies on.
    pub fn compile(reports: &Reports, meta: TablesMeta) -> anyhow::Result<Self> {
        let mut blocks = Vec::with_capacity(reports.blocks.len());
        for (name, b) in &reports.blocks {
            let order = reports
                .property_order
                .get(name)
                .cloned()
                .unwrap_or_default();
            ensure!(
                order.len() == b.properties.len(),
                "{name}: property order mismatch"
            );
            let properties: Vec<Property> = order
                .iter()
                .map(|p| Property {
                    name: p.clone(),
                    values: b.properties[p].clone(),
                })
                .collect();
            let first = b
                .states
                .iter()
                .map(|s| s.id)
                .min()
                .with_context(|| format!("{name}: no states"))?;
            let count = b.states.len() as u32;
            let mut ids: Vec<u32> = b.states.iter().map(|s| s.id).collect();
            ids.sort_unstable();
            ensure!(
                ids == (first..first + count).collect::<Vec<_>>(),
                "{name}: states are not contiguous"
            );
            let defaults: Vec<u32> = b
                .states
                .iter()
                .filter(|s| s.default)
                .map(|s| s.id)
                .collect();
            ensure!(
                defaults.len() == 1,
                "{name}: expected exactly one default state, found {}",
                defaults.len()
            );
            blocks.push(Block {
                name: name.clone(),
                properties,
                first_state: first,
                state_count: count,
                default_state: defaults[0],
            });
        }
        blocks.sort_by_key(|b| b.first_state);

        let total: u32 = blocks.iter().map(|b| b.state_count).sum();
        let mut states: Vec<Option<StateRow>> = vec![None; total as usize];
        for (bi, block) in blocks.iter().enumerate() {
            for s in &reports.blocks[&block.name].states {
                let values = block
                    .properties
                    .iter()
                    .map(|p| {
                        let v = s.properties.get(&p.name).with_context(|| {
                            format!("{}: state {} lacks {}", block.name, s.id, p.name)
                        })?;
                        let idx = p
                            .values
                            .iter()
                            .position(|x| x == v)
                            .with_context(|| format!("{}: bad value {v}", block.name))?;
                        u8::try_from(idx).context("property has more than 256 values")
                    })
                    .collect::<anyhow::Result<Vec<u8>>>()?;
                let slot = states
                    .get_mut(s.id as usize)
                    .with_context(|| format!("state id {} out of range", s.id))?;
                ensure!(slot.is_none(), "state id {} assigned twice", s.id);
                *slot = Some(StateRow {
                    block: bi as u32,
                    values,
                });
            }
        }
        let states = states
            .into_iter()
            .enumerate()
            .map(|(i, s)| s.with_context(|| format!("state id {i} is missing (ids must be dense)")))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let mut registries: Vec<Registry> = reports
            .registries
            .iter()
            .map(|(name, r)| {
                let mut entries: Vec<RegistryEntry> = r
                    .entries
                    .iter()
                    .map(|(n, e)| RegistryEntry {
                        name: n.clone(),
                        protocol_id: e.protocol_id,
                    })
                    .collect();
                entries.sort_by_key(|e| e.protocol_id);
                Registry {
                    name: name.clone(),
                    protocol_id: r.protocol_id,
                    default: r.default.clone(),
                    entries,
                }
            })
            .collect();
        registries.sort_by_key(|r| r.protocol_id);

        let mut packets: Vec<Packet> = reports
            .packets
            .iter()
            .flat_map(|(state, dirs)| {
                dirs.iter().flat_map(move |(dir, ps)| {
                    ps.iter().map(move |(name, id)| Packet {
                        state: state.clone(),
                        direction: dir.clone(),
                        name: name.clone(),
                        protocol_id: id.protocol_id,
                    })
                })
            })
            .collect();
        packets.sort_by(|a, b| {
            (&a.state, &a.direction, a.protocol_id).cmp(&(&b.state, &b.direction, b.protocol_id))
        });

        Ok(Self {
            meta,
            blocks,
            states,
            registries,
            packets,
            commands_json: reports.commands_json.clone(),
        })
    }

    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(1 << 20);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&SCHEMA.to_le_bytes());
        out.extend_from_slice(&postcard::to_stdvec(self)?);
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() < 8 || &bytes[..4] != MAGIC {
            bail!("not a Zenith tables file");
        }
        let schema = u32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
        if schema != SCHEMA {
            bail!("tables schema {schema} is not the supported {SCHEMA}; re-run ingestion");
        }
        Ok(postcard::from_bytes(&bytes[8..])?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("bin.tmp");
        fs::write(&tmp, self.to_bytes()?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Self::from_bytes(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
    }

    /// Look up a block by id.
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.name == name)
    }

    /// Look up a registry by id.
    pub fn registry(&self, name: &str) -> Option<&Registry> {
        self.registries.iter().find(|r| r.name == name)
    }
}
