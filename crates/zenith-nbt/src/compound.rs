//! NBT compound map reproducing Java 21 `HashMap<String, Tag>` iteration order.
//!
//! Write order of NBT compounds follows Java's `HashMap` bucket iteration order:
//! - Hash = Java `String.hashCode()` over UTF-16 code units, spread via `h ^ (h >>> 16)`.
//! - Bucket index = `spread_hash & (capacity - 1)`.
//! - Power-of-two capacity starting at 16 (lazily allocated on first insert), load factor 0.75.
//! - On resize, each bucket `j` splits into `j` (`(hash & old_cap) == 0`) and `j + old_cap`
//!   (`(hash & old_cap) != 0`), preserving relative order within each split bucket.
//! - Within a bucket, entries iterate in insertion order.
//! - Cloning a [`Compound`] uses Java 21's `HashMap(Map)` copy-constructor sizing
//!   (`tableSizeFor(ceil(len / 0.75))`), so a clone of a map that grew and shrank can iterate
//!   in a different order than the source, matching vanilla's `CompoundTag.copy()`.
//!
//! ## Treeified bins
//!
//! In Java's `HashMap`, when a bucket exceeds 8 colliding entries and the table capacity is at
//! least 64, that bin is converted into a red-black tree (whose iteration order differs from
//! insertion order). If capacity is below 64, Java resizes the table instead of treeifying.
//! This implementation reproduces the early-resize behavior for capacity < 64, and detects
//! and records when a bin would treeify at capacity >= 64 ([`Compound::has_treeified_bin`]),
//! returning [`Error::UnsupportedTreeifiedBin`] on serialization or [`Compound::try_iter`]
//! rather than silently emitting a wrong order.

use std::fmt;

use crate::error::Error;
use crate::mutf8::{NbtString, java_hash_str};
use crate::tag::Tag;

const DEFAULT_INITIAL_CAPACITY: usize = 16;
const MAXIMUM_CAPACITY: usize = 1 << 30;
const TREEIFY_THRESHOLD: usize = 8;
const MIN_TREEIFY_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
struct Entry {
    spread_hash: u32,
    key: NbtString,
    value: Tag,
}

/// An NBT compound tag mapping [`NbtString`] keys to [`Tag`] values with Java 21 `HashMap`
/// iteration order.
pub struct Compound {
    /// Bucket table; empty until the first insertion (matching Java's lazy allocation).
    buckets: Vec<Vec<Entry>>,
    /// Number of key-value entries currently stored.
    len: usize,
    /// When `buckets` is empty, holds the deferred initial capacity (0 means default 16).
    /// When `buckets` is allocated, holds the resize threshold (`(cap * 3) / 4`).
    threshold: usize,
    /// Records the first `(bucket_len, capacity)` that would have treeified in Java's `HashMap`.
    treeified: Option<(usize, usize)>,
}

#[inline]
const fn spread_hash(java_hash: i32) -> u32 {
    let h = java_hash as u32;
    h ^ (h >> 16)
}

#[inline]
const fn table_size_for(cap: usize) -> usize {
    if cap <= 1 {
        1
    } else if cap >= MAXIMUM_CAPACITY {
        MAXIMUM_CAPACITY
    } else {
        cap.next_power_of_two()
    }
}

#[inline]
const fn threshold_for(cap: usize) -> usize {
    if cap >= MAXIMUM_CAPACITY {
        usize::MAX
    } else {
        // Exact equivalent of `(int) (cap * 0.75f)` for all power-of-two capacities.
        (cap * 3) / 4
    }
}

/// Computes Java 21 `HashMap.calculateHashMapCapacity(num_mappings)` = `ceil(n / 0.75)`
/// rounded up via `tableSizeFor`.
#[inline]
const fn copy_initial_capacity(num_mappings: usize) -> usize {
    if num_mappings == 0 {
        return 0;
    }
    // ceil(num_mappings / 0.75) == ceil(4 * num_mappings / 3) == (4 * num_mappings + 2) / 3
    let Some(scaled) = num_mappings.checked_mul(4) else {
        return MAXIMUM_CAPACITY;
    };
    let Some(numerator) = scaled.checked_add(2) else {
        return MAXIMUM_CAPACITY;
    };
    table_size_for(numerator / 3)
}

impl Compound {
    /// Creates an empty `Compound` (equivalent to `new HashMap<>()`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buckets: Vec::new(),
            len: 0,
            threshold: 0,
            treeified: None,
        }
    }

    /// Creates an empty `Compound` with a deferred initial capacity matching
    /// `new HashMap<>(initial_capacity)`.
    #[must_use]
    pub const fn with_capacity(initial_capacity: usize) -> Self {
        let threshold = if initial_capacity == 0 {
            0
        } else {
            table_size_for(initial_capacity)
        };
        Self {
            buckets: Vec::new(),
            len: 0,
            threshold,
            treeified: None,
        }
    }

    /// Creates a `Compound` sized for `expected_entries` using Java 21's
    /// `HashMap.newHashMap(expected_entries)` / `HashMap(Map)` sizing formula.
    #[must_use]
    pub const fn with_expected_entries(expected_entries: usize) -> Self {
        Self {
            buckets: Vec::new(),
            len: 0,
            threshold: copy_initial_capacity(expected_entries),
            treeified: None,
        }
    }

    /// Returns the number of entries in the compound.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the compound contains no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the current allocated bucket-table capacity (`0` before the first insertion).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buckets.len()
    }

    /// Returns `true` if any bin has crossed Java's treeification threshold
    /// (`> 8` colliding entries at `capacity >= 64`).
    #[must_use]
    pub const fn has_treeified_bin(&self) -> bool {
        self.treeified.is_some()
    }

    /// Ensures the map has not entered an unsupported treeified-bin state.
    pub fn check_order_supported(&self) -> Result<(), Error> {
        if let Some((bucket_len, capacity)) = self.treeified {
            return Err(Error::UnsupportedTreeifiedBin {
                bucket_len,
                capacity,
            });
        }
        Ok(())
    }

    fn allocate_initial(&mut self) {
        let cap = if self.threshold > 0 {
            self.threshold
        } else {
            DEFAULT_INITIAL_CAPACITY
        };
        self.buckets = (0..cap).map(|_| Vec::new()).collect();
        self.threshold = threshold_for(cap);
    }

    fn resize(&mut self) {
        if self.buckets.is_empty() {
            self.allocate_initial();
            return;
        }
        let old_cap = self.buckets.len();
        if old_cap >= MAXIMUM_CAPACITY {
            self.threshold = usize::MAX;
            return;
        }
        let new_cap = old_cap << 1;
        let new_threshold = if old_cap >= DEFAULT_INITIAL_CAPACITY {
            self.threshold.saturating_mul(2)
        } else {
            threshold_for(new_cap)
        };

        let mut new_buckets: Vec<Vec<Entry>> = (0..new_cap).map(|_| Vec::new()).collect();
        let old_buckets = std::mem::take(&mut self.buckets);
        let bit = old_cap as u32;

        for (j, bucket) in old_buckets.into_iter().enumerate() {
            for entry in bucket {
                if (entry.spread_hash & bit) == 0 {
                    new_buckets[j].push(entry);
                } else {
                    new_buckets[j + old_cap].push(entry);
                }
            }
        }

        self.buckets = new_buckets;
        self.threshold = new_threshold;
    }

    /// Inserts a key-value pair into the compound, returning the previous value if the key
    /// was already present.
    ///
    /// Replacing an existing key preserves its position in iteration order and does not
    /// trigger a resize.
    pub fn insert(&mut self, key: impl Into<NbtString>, value: Tag) -> Option<Tag> {
        let key = key.into();
        if self.buckets.is_empty() {
            self.allocate_initial();
        }
        let hash = spread_hash(key.java_hash_code());
        let cap = self.buckets.len();
        let idx = (hash as usize) & (cap - 1);

        let bucket = &mut self.buckets[idx];
        for entry in bucket.iter_mut() {
            if entry.spread_hash == hash && entry.key == key {
                return Some(std::mem::replace(&mut entry.value, value));
            }
        }

        // Java `HashMap.putVal`: `binCount` counts existing nodes traversed before appending
        // the new node. `if (binCount >= TREEIFY_THRESHOLD - 1)` triggers when appending the
        // 9th node (existing len >= 8, so new len >= 9).
        let existing_len = bucket.len();
        bucket.push(Entry {
            spread_hash: hash,
            key,
            value,
        });
        let new_bucket_len = existing_len + 1;

        if existing_len >= TREEIFY_THRESHOLD {
            if cap < MIN_TREEIFY_CAPACITY {
                self.resize();
            } else if self.treeified.is_none() {
                self.treeified = Some((new_bucket_len, cap));
            }
        }

        self.len += 1;
        if self.len > self.threshold {
            self.resize();
        }

        None
    }

    /// Looks up a value by `&str` key without allocating.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Tag> {
        if self.buckets.is_empty() {
            return None;
        }
        let hash = spread_hash(java_hash_str(key));
        let idx = (hash as usize) & (self.buckets.len() - 1);
        self.buckets[idx]
            .iter()
            .find(|e| e.spread_hash == hash && e.key == key)
            .map(|e| &e.value)
    }

    /// Looks up a mutable reference to a value by `&str` key without allocating.
    #[must_use]
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Tag> {
        if self.buckets.is_empty() {
            return None;
        }
        let hash = spread_hash(java_hash_str(key));
        let idx = (hash as usize) & (self.buckets.len() - 1);
        self.buckets[idx]
            .iter_mut()
            .find(|e| e.spread_hash == hash && e.key == key)
            .map(|e| &mut e.value)
    }

    /// Looks up a value by [`NbtString`] key (supports keys containing lone surrogates).
    #[must_use]
    pub fn get_nbt(&self, key: &NbtString) -> Option<&Tag> {
        if self.buckets.is_empty() {
            return None;
        }
        let hash = spread_hash(key.java_hash_code());
        let idx = (hash as usize) & (self.buckets.len() - 1);
        self.buckets[idx]
            .iter()
            .find(|e| e.spread_hash == hash && e.key == *key)
            .map(|e| &e.value)
    }

    /// Returns `true` if the compound contains `key`.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Returns `true` if the compound contains the given [`NbtString`] key.
    #[must_use]
    pub fn contains_nbt_key(&self, key: &NbtString) -> bool {
        self.get_nbt(key).is_some()
    }

    /// Removes `key` from the compound, returning its value if present.
    ///
    /// Preserves the relative order of remaining entries in the bucket and does not shrink
    /// the bucket table (matching Java's `HashMap.remove`).
    pub fn remove(&mut self, key: &str) -> Option<Tag> {
        if self.buckets.is_empty() {
            return None;
        }
        let hash = spread_hash(java_hash_str(key));
        let idx = (hash as usize) & (self.buckets.len() - 1);
        let bucket = &mut self.buckets[idx];
        let pos = bucket
            .iter()
            .position(|e| e.spread_hash == hash && e.key == key)?;
        let entry = bucket.remove(pos);
        self.len -= 1;
        Some(entry.value)
    }

    /// Removes an [`NbtString`] key from the compound, returning its value if present.
    pub fn remove_nbt(&mut self, key: &NbtString) -> Option<Tag> {
        if self.buckets.is_empty() {
            return None;
        }
        let hash = spread_hash(key.java_hash_code());
        let idx = (hash as usize) & (self.buckets.len() - 1);
        let bucket = &mut self.buckets[idx];
        let pos = bucket
            .iter()
            .position(|e| e.spread_hash == hash && e.key == *key)?;
        let entry = bucket.remove(pos);
        self.len -= 1;
        Some(entry.value)
    }

    /// Removes all entries while preserving the current bucket table capacity
    /// (matching Java's `HashMap.clear`).
    pub fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.len = 0;
        self.treeified = None;
    }

    /// Iterates over `(&NbtString, &Tag)` entries in Java `HashMap` bucket order.
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            buckets: self.buckets.iter(),
            current: [].iter(),
            remaining: self.len,
        }
    }

    /// Iterates over entries in Java `HashMap` order, failing with
    /// [`Error::UnsupportedTreeifiedBin`] if a bin would have treeified.
    pub fn try_iter(&self) -> Result<Iter<'_>, Error> {
        self.check_order_supported()?;
        Ok(self.iter())
    }

    /// Iterates over keys in Java `HashMap` bucket order.
    pub fn keys(&self) -> impl Iterator<Item = &NbtString> {
        self.iter().map(|(k, _)| k)
    }

    /// Iterates over values in Java `HashMap` bucket order.
    pub fn values(&self) -> impl Iterator<Item = &Tag> {
        self.iter().map(|(_, v)| v)
    }

    /// Returns `true` if `self` and `other` have identical entries in the exact same iteration order.
    #[must_use]
    pub fn order_eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }

    /// Returns `true` if this compound has the heterogeneous-list wrapper shape:
    /// exactly one entry whose key is the empty string `""`.
    #[must_use]
    pub fn is_list_wrapper(&self) -> bool {
        self.len == 1 && self.contains_key("")
    }

    /// Unwraps a wrapper-shaped compound (1 entry with key `""`) into its inner value,
    /// returning `Err(self)` if it does not have the wrapper shape.
    pub fn unwrap_list_wrapper(mut self) -> Result<Tag, Self> {
        if self.is_list_wrapper() {
            Ok(self
                .remove("")
                .expect("is_list_wrapper verified empty key exists"))
        } else {
            Err(self)
        }
    }

    /// Wraps `tag` in a single-entry compound `{ "": tag }`.
    #[must_use]
    pub fn wrap_list_element(tag: Tag) -> Self {
        let mut c = Self::new();
        c.insert("", tag);
        c
    }
}

impl Default for Compound {
    fn default() -> Self {
        Self::new()
    }
}

/// Clones the compound using Java 21's `new HashMap<>(other)` copy-constructor sizing,
/// reproducing vanilla's `CompoundTag.copy()` iteration order.
impl Clone for Compound {
    fn clone(&self) -> Self {
        if self.is_empty() {
            return Self::new();
        }
        let mut copy = Self::with_expected_entries(self.len);
        for (k, v) in self.iter() {
            copy.insert(k.clone(), v.clone());
        }
        copy
    }
}

/// Map equality (independent of bucket capacity / iteration order), matching `HashMap.equals`.
impl PartialEq for Compound {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        self.iter()
            .all(|(k, v)| other.get_nbt(k).is_some_and(|ov| ov == v))
    }
}

impl Eq for Compound {}

impl fmt::Debug for Compound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// Borrowing iterator over entries of a [`Compound`] in Java `HashMap` order.
#[derive(Debug, Clone)]
pub struct Iter<'a> {
    buckets: std::slice::Iter<'a, Vec<Entry>>,
    current: std::slice::Iter<'a, Entry>,
    remaining: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a NbtString, &'a Tag);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(entry) = self.current.next() {
                self.remaining -= 1;
                return Some((&entry.key, &entry.value));
            }
            let bucket = self.buckets.next()?;
            self.current = bucket.iter();
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Iter<'_> {
    fn len(&self) -> usize {
        self.remaining
    }
}

/// Owning iterator over entries of a [`Compound`] in Java `HashMap` order.
#[derive(Debug)]
pub struct IntoIter {
    buckets: std::vec::IntoIter<Vec<Entry>>,
    current: std::vec::IntoIter<Entry>,
    remaining: usize,
}

impl Iterator for IntoIter {
    type Item = (NbtString, Tag);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(entry) = self.current.next() {
                self.remaining -= 1;
                return Some((entry.key, entry.value));
            }
            let bucket = self.buckets.next()?;
            self.current = bucket.into_iter();
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for IntoIter {
    fn len(&self) -> usize {
        self.remaining
    }
}

impl IntoIterator for Compound {
    type Item = (NbtString, Tag);
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            buckets: self.buckets.into_iter(),
            current: Vec::new().into_iter(),
            remaining: self.len,
        }
    }
}

impl<'a> IntoIterator for &'a Compound {
    type Item = (&'a NbtString, &'a Tag);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<K: Into<NbtString>> FromIterator<(K, Tag)> for Compound {
    fn from_iter<T: IntoIterator<Item = (K, Tag)>>(iter: T) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}
