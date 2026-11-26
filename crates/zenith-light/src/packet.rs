//! Light packet payloads (`chunk-steps.md` §8, `ClientboundLightUpdatePacketData`).

use crate::layer::{DataLayer, LAYER_BYTES};
use crate::level::LevelLightEngine;
use crate::pos::section_node;
use crate::storage::LightLayer;

/// A growable bit set (`java.util.BitSet`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    /// An empty set.
    pub const fn new() -> Self {
        Self { words: Vec::new() }
    }

    /// Sets bit `i`; returns `true` when it was clear.
    pub fn set(&mut self, i: usize) -> bool {
        let w = i / 64;
        if self.words.len() <= w {
            self.words.resize(w + 1, 0);
        }
        let bit = 1u64 << (i % 64);
        let was = self.words[w] & bit != 0;
        self.words[w] |= bit;
        !was
    }

    /// Reads bit `i`.
    pub fn get(&self, i: usize) -> bool {
        self.words
            .get(i / 64)
            .is_some_and(|w| w & (1u64 << (i % 64)) != 0)
    }

    /// Whether no bit is set.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// Clears every bit.
    pub fn clear(&mut self) {
        self.words.clear();
    }

    /// `toLongArray()`: trailing zero words trimmed.
    pub fn to_longs(&self) -> Vec<u64> {
        let mut v = self.words.clone();
        while v.last() == Some(&0) {
            v.pop();
        }
        v
    }

    /// Builds a set from `BitSet.valueOf(long[])`.
    pub fn from_longs(words: Vec<u64>) -> Self {
        Self { words }
    }

    /// The set bits ascending.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.words.len() * 64).filter(|&i| self.get(i))
    }
}

/// The light payload of a chunk or light-update packet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LightUpdatePacketData {
    /// Light sections (index = section − min light section) with sky data.
    pub sky_y_mask: BitSet,
    /// … with block data.
    pub block_y_mask: BitSet,
    /// … whose sky layer is known to be empty.
    pub empty_sky_y_mask: BitSet,
    /// … whose block layer is known to be empty.
    pub empty_block_y_mask: BitSet,
    /// The sky arrays, one per set bit of `sky_y_mask`, ascending.
    pub sky_updates: Vec<Vec<u8>>,
    /// The block arrays.
    pub block_updates: Vec<Vec<u8>>,
}

/// Error decoding a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PacketError {
    /// The buffer ended early or a length was out of range.
    #[error("malformed light packet data")]
    Malformed,
}

impl LightUpdatePacketData {
    /// Reads the layers of the chunk at `(cx, cz)`; `None` filters mean "all sections".
    pub fn from_engine(
        engine: &LevelLightEngine,
        cx: i32,
        cz: i32,
        sky_filter: Option<&BitSet>,
        block_filter: Option<&BitSet>,
    ) -> Self {
        let mut out = Self::default();
        let min = engine.min_light_section();
        for index in 0..engine.light_section_count() as usize {
            let node = section_node(cx, min + index as i32, cz);
            for (layer, filter) in [
                (LightLayer::Sky, sky_filter),
                (LightLayer::Block, block_filter),
            ] {
                if filter.is_some_and(|f| !f.get(index)) {
                    continue;
                }
                let Some(data) = engine.data_layer_data(layer, node) else {
                    continue;
                };
                let (mask, empty, updates) = match layer {
                    LightLayer::Sky => (
                        &mut out.sky_y_mask,
                        &mut out.empty_sky_y_mask,
                        &mut out.sky_updates,
                    ),
                    LightLayer::Block => (
                        &mut out.block_y_mask,
                        &mut out.empty_block_y_mask,
                        &mut out.block_updates,
                    ),
                };
                if data.is_empty() {
                    empty.set(index);
                } else {
                    mask.set(index);
                    updates.push(data.to_bytes().to_vec());
                }
            }
        }
        out
    }

    /// The layers a client queues: `(layer, index, Some(data) | empty)`, sky first, ascending index.
    pub fn layers(&self) -> Vec<(LightLayer, usize, DataLayer)> {
        let mut out = Vec::new();
        for (layer, mask, empty, updates) in [
            (
                LightLayer::Sky,
                &self.sky_y_mask,
                &self.empty_sky_y_mask,
                &self.sky_updates,
            ),
            (
                LightLayer::Block,
                &self.block_y_mask,
                &self.empty_block_y_mask,
                &self.block_updates,
            ),
        ] {
            let mut next = 0usize;
            let upper = (mask.words.len().max(empty.words.len())) * 64;
            for index in 0..upper {
                if mask.get(index) {
                    if let Some(bytes) = updates.get(next)
                        && let Ok(l) = DataLayer::from_bytes(bytes)
                    {
                        out.push((layer, index, l));
                    }
                    next += 1;
                } else if empty.get(index) {
                    out.push((layer, index, DataLayer::new()));
                }
            }
        }
        out
    }

    /// Wire form: four bit sets (sky data, block data, sky empty, block empty), then the sky and block array
    /// lists, each array length-prefixed (var-ints, big-endian longs).
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        for set in [
            &self.sky_y_mask,
            &self.block_y_mask,
            &self.empty_sky_y_mask,
            &self.empty_block_y_mask,
        ] {
            let longs = set.to_longs();
            write_varint(&mut b, longs.len() as u32);
            for l in longs {
                b.extend_from_slice(&l.to_be_bytes());
            }
        }
        for list in [&self.sky_updates, &self.block_updates] {
            write_varint(&mut b, list.len() as u32);
            for a in list {
                write_varint(&mut b, a.len() as u32);
                b.extend_from_slice(a);
            }
        }
        b
    }

    /// Parses [`Self::encode`] output.
    pub fn decode(mut buf: &[u8]) -> Result<Self, PacketError> {
        let mut sets = Vec::new();
        for _ in 0..4 {
            let n = read_varint(&mut buf)? as usize;
            let mut longs = Vec::with_capacity(n.min(1024));
            for _ in 0..n {
                let (head, rest) = buf.split_first_chunk::<8>().ok_or(PacketError::Malformed)?;
                longs.push(u64::from_be_bytes(*head));
                buf = rest;
            }
            sets.push(BitSet::from_longs(longs));
        }
        let mut lists = Vec::new();
        for _ in 0..2 {
            let n = read_varint(&mut buf)? as usize;
            let mut list = Vec::with_capacity(n.min(64));
            for _ in 0..n {
                let len = read_varint(&mut buf)? as usize;
                if len > LAYER_BYTES || buf.len() < len {
                    return Err(PacketError::Malformed);
                }
                list.push(buf[..len].to_vec());
                buf = &buf[len..];
            }
            lists.push(list);
        }
        let mut s = sets.into_iter();
        let mut l = lists.into_iter();
        Ok(Self {
            sky_y_mask: s.next().unwrap_or_default(),
            block_y_mask: s.next().unwrap_or_default(),
            empty_sky_y_mask: s.next().unwrap_or_default(),
            empty_block_y_mask: s.next().unwrap_or_default(),
            sky_updates: l.next().unwrap_or_default(),
            block_updates: l.next().unwrap_or_default(),
        })
    }
}

fn write_varint(out: &mut Vec<u8>, mut v: u32) {
    loop {
        if v & !0x7F == 0 {
            out.push(v as u8);
            return;
        }
        out.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
}

fn read_varint(buf: &mut &[u8]) -> Result<u32, PacketError> {
    let mut v = 0u32;
    for shift in (0..35).step_by(7) {
        let (&b, rest) = buf.split_first().ok_or(PacketError::Malformed)?;
        *buf = rest;
        v |= u32::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(PacketError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitset_long_array_trims_trailing_zeros() {
        let mut b = BitSet::new();
        b.set(70);
        b.set(3);
        assert_eq!(b.to_longs(), vec![1 << 3, 1 << 6]);
        let mut c = BitSet::new();
        c.set(200);
        c.words[3] = 0;
        assert!(c.to_longs().is_empty());
    }

    #[test]
    fn varint_round_trip() {
        for v in [0u32, 1, 127, 128, 2048, 300_000] {
            let mut b = Vec::new();
            write_varint(&mut b, v);
            let mut s: &[u8] = &b;
            assert_eq!(read_varint(&mut s).unwrap(), v);
            assert!(s.is_empty());
        }
    }

    #[test]
    fn packet_round_trip() {
        let mut p = LightUpdatePacketData::default();
        p.sky_y_mask.set(5);
        p.empty_block_y_mask.set(2);
        p.sky_updates.push(vec![0x11; 2048]);
        let d = LightUpdatePacketData::decode(&p.encode()).unwrap();
        assert_eq!(d, p);
        let layers = d.layers();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].0, LightLayer::Sky);
        assert_eq!(layers[0].1, 5);
        assert_eq!(layers[0].2.get(0, 0, 0), 1);
        assert_eq!(layers[1].0, LightLayer::Block);
        assert!(layers[1].2.is_empty());
    }
}
