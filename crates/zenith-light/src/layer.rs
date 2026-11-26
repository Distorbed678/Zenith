//! `DataLayer`: one 16×16×16 nibble array of one light engine (`data-layers.md` §2).
//!
//! The lazy / materialised distinction is observable (saving and packets omit layers that are *empty*, i.e. lazy with
//! default 0), so it is modelled exactly: `data == None` means "every nibble reads `default`".

use std::fmt;

/// Size of a layer in bytes (4096 nibbles).
pub const LAYER_BYTES: usize = 2048;
/// Size of one local-`y` slice in bytes.
pub const SLICE_BYTES: usize = 128;

/// Error for a byte array that is not exactly [`LAYER_BYTES`] long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadLayerSize(pub usize);

impl fmt::Display for BadLayerSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a light layer needs exactly 2048 bytes, got {}", self.0)
    }
}

impl std::error::Error for BadLayerSize {}

/// A nibble array with an optional lazily-materialised backing store.
#[derive(Clone, PartialEq, Eq)]
pub struct DataLayer {
    data: Option<Box<[u8; LAYER_BYTES]>>,
    default: u8,
}

impl fmt::Debug for DataLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataLayer")
            .field("materialised", &self.data.is_some())
            .field("default", &self.default)
            .finish()
    }
}

impl Default for DataLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl DataLayer {
    /// An empty layer: no array, default 0.
    pub const fn new() -> Self {
        Self {
            data: None,
            default: 0,
        }
    }

    /// A lazy layer whose every nibble reads `value` (no array).
    pub const fn filled(value: u8) -> Self {
        Self {
            data: None,
            default: value,
        }
    }

    /// A layer adopting `bytes` (exactly 2048; default 0).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BadLayerSize> {
        let arr: Box<[u8; LAYER_BYTES]> = bytes
            .to_vec()
            .into_boxed_slice()
            .try_into()
            .map_err(|_| BadLayerSize(bytes.len()))?;
        Ok(Self {
            data: Some(arr),
            default: 0,
        })
    }

    /// The nibble at section-local `(x, y, z)`.
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        self.get_index(crate::pos::layer_index(x, y, z))
    }

    /// The nibble at `index = (y << 8) | (z << 4) | x`.
    #[inline]
    pub fn get_index(&self, index: usize) -> u8 {
        match &self.data {
            None => self.default,
            Some(d) => {
                let b = d[index >> 1];
                if index & 1 == 0 { b & 15 } else { b >> 4 }
            }
        }
    }

    /// Stores `value & 15` at section-local `(x, y, z)`, materialising the array first.
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, value: u8) {
        self.set_index(crate::pos::layer_index(x, y, z), value);
    }

    /// Stores `value & 15` at `index`, materialising the array first.
    #[inline]
    pub fn set_index(&mut self, index: usize, value: u8) {
        let v = value & 15;
        let d = self.materialise();
        let slot = &mut d[index >> 1];
        *slot = if index & 1 == 0 {
            (*slot & 0xF0) | v
        } else {
            (*slot & 0x0F) | (v << 4)
        };
    }

    fn materialise(&mut self) -> &mut [u8; LAYER_BYTES] {
        let default = self.default;
        self.data.get_or_insert_with(|| {
            let fill = if default == 0 {
                0
            } else {
                default | (default << 4)
            };
            Box::new([fill; LAYER_BYTES])
        })
    }

    /// `getData()`: materialises the array and returns it (mutating, as in the game).
    pub fn data_mut(&mut self) -> &[u8; LAYER_BYTES] {
        self.materialise()
    }

    /// The 2048 bytes of this layer without materialising it (a lazy layer yields its filled form).
    pub fn to_bytes(&self) -> [u8; LAYER_BYTES] {
        match &self.data {
            Some(d) => **d,
            None => {
                let fill = if self.default == 0 {
                    0
                } else {
                    self.default | (self.default << 4)
                };
                [fill; LAYER_BYTES]
            }
        }
    }

    /// `fill(v)`: default `v`, array dropped.
    pub fn fill(&mut self, value: u8) {
        self.default = value;
        self.data = None;
    }

    /// `copy()`: a lazy layer stays lazy with the same default; a materialised one clones its array (default 0).
    pub fn copy(&self) -> Self {
        match &self.data {
            None => Self {
                data: None,
                default: self.default,
            },
            Some(d) => Self {
                data: Some(d.clone()),
                default: 0,
            },
        }
    }

    /// `isEmpty`: no array and default 0.
    pub const fn is_empty(&self) -> bool {
        self.data.is_none() && self.default == 0
    }

    /// `isDefinitelyHomogenous`: no array is held.
    pub const fn is_definitely_homogenous(&self) -> bool {
        self.data.is_none()
    }

    /// `isDefinitelyFilledWith(v)`: no array and default `v`.
    pub const fn is_definitely_filled_with(&self, value: u8) -> bool {
        self.data.is_none() && self.default == value
    }

    /// Whether an array has been materialised.
    pub const fn is_materialised(&self) -> bool {
        self.data.is_some()
    }

    /// The lazy default value.
    pub const fn default_value(&self) -> u8 {
        self.default
    }

    /// `repeatFirstLayer`: a lazy layer copies lazily; otherwise a new array holds the first 128 bytes (the local
    /// `y = 0` slice) in all sixteen slices.
    pub fn repeat_first_layer(&self) -> Self {
        match &self.data {
            None => self.copy(),
            Some(d) => {
                let mut out = Box::new([0u8; LAYER_BYTES]);
                for slice in out.as_chunks_mut::<SLICE_BYTES>().0 {
                    slice.copy_from_slice(&d[..SLICE_BYTES]);
                }
                Self {
                    data: Some(out),
                    default: 0,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_layer_reads_default_and_stays_empty() {
        let l = DataLayer::new();
        assert!(l.is_empty() && l.is_definitely_homogenous());
        assert_eq!(l.get(3, 4, 5), 0);
        let f = DataLayer::filled(15);
        assert!(!f.is_empty() && f.is_definitely_filled_with(15));
        assert_eq!(f.get(15, 15, 15), 15);
    }

    #[test]
    fn set_materialises_even_for_zero() {
        let mut l = DataLayer::new();
        l.set(1, 2, 3, 0);
        assert!(!l.is_empty());
        assert!(l.is_materialised());
    }

    #[test]
    fn nibble_addressing_low_nibble_is_even_index() {
        let mut l = DataLayer::new();
        l.set(0, 0, 0, 7);
        l.set(1, 0, 0, 9);
        let b = l.to_bytes();
        assert_eq!(b[0], 0x97);
        l.set(2, 0, 0, 3);
        assert_eq!(l.to_bytes()[1] & 15, 3);
        // index (y<<8)|(z<<4)|x
        l.set(5, 2, 7, 11);
        assert_eq!(l.get_index((2 << 8) | (7 << 4) | 5), 11);
    }

    #[test]
    fn materialising_a_filled_layer_fills_bytes() {
        let mut l = DataLayer::filled(15);
        l.set(0, 0, 0, 1);
        assert_eq!(l.get(1, 0, 0), 15);
        assert_eq!(l.to_bytes()[1], 0xFF);
        assert_eq!(l.to_bytes()[0], 0xF1);
    }

    #[test]
    fn copy_keeps_laziness_and_resets_default_of_arrays() {
        let l = DataLayer::filled(15);
        let c = l.copy();
        assert!(c.is_definitely_filled_with(15));
        let mut m = DataLayer::filled(15);
        m.set(0, 0, 0, 2);
        let c = m.copy();
        assert!(c.is_materialised());
        assert_eq!(c.default_value(), 0);
        assert_eq!(c.get(3, 3, 3), 15);
    }

    #[test]
    fn repeat_first_layer_replicates_bottom_slice() {
        let mut l = DataLayer::new();
        l.set(4, 0, 9, 12);
        l.set(4, 5, 9, 3);
        let r = l.repeat_first_layer();
        for y in 0..16 {
            assert_eq!(r.get(4, y, 9), 12);
        }
        let lazy = DataLayer::filled(15).repeat_first_layer();
        assert!(lazy.is_definitely_filled_with(15));
    }

    #[test]
    fn from_bytes_requires_exact_size() {
        assert!(DataLayer::from_bytes(&[0; 2047]).is_err());
        let l = DataLayer::from_bytes(&[0x21; 2048]).unwrap();
        assert_eq!(l.get(0, 0, 0), 1);
        assert_eq!(l.get(1, 0, 0), 2);
    }

    #[test]
    fn fill_drops_the_array() {
        let mut l = DataLayer::new();
        l.set(0, 0, 0, 5);
        l.fill(15);
        assert!(l.is_definitely_filled_with(15));
    }
}
