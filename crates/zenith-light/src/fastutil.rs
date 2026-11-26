//! `LongOpenHashSet` with the exact iteration order of fastutil 8.5.x (`data-layers.md`, "Hash-set iteration order").
//!
//! Open addressing with linear probing over a power-of-two table; key `0` lives in a separate flag. Only the order
//! matters for parity (the engine's pending-check set is iterated), so only the operations the engine needs exist.
//! Written from the public fastutil semantics (`HashCommon.mix`, `arraySize`, `maxFill`, `rehash`, `trim`).

const LONG_PHI: u64 = 0x9E37_79B9_7F4A_7C15;

/// `HashCommon.mix(long)` of fastutil: golden-ratio multiply, then two xor-shifts, truncated to 32 bits.
#[inline]
pub fn mix(x: i64) -> i32 {
    let mut h = (x as u64).wrapping_mul(LONG_PHI);
    h ^= h >> 32;
    h ^= h >> 16;
    h as i32
}

fn next_power_of_two(x: u64) -> u64 {
    x.max(1).next_power_of_two()
}

/// `HashCommon.arraySize(expected, f)`.
fn array_size(expected: u64, f: f64) -> usize {
    let s = next_power_of_two((expected as f64 / f).ceil() as u64).max(2);
    assert!(s <= 1 << 30, "too large a hash set");
    s as usize
}

/// `HashCommon.maxFill(n, f)`.
fn max_fill(n: usize, f: f64) -> usize {
    ((n as f64 * f).ceil() as usize).min(n - 1)
}

/// A set of `i64` with fastutil's `LongOpenHashSet` layout and iteration order.
#[derive(Debug, Clone)]
pub struct LongOpenHashSet {
    key: Vec<i64>,
    mask: usize,
    contains_null: bool,
    n: usize,
    max_fill: usize,
    size: usize,
    f: f64,
}

impl LongOpenHashSet {
    /// `new LongOpenHashSet(expected, f)`.
    pub fn with_expected(expected: usize, f: f64) -> Self {
        let n = array_size(expected as u64, f);
        Self {
            key: vec![0; n],
            mask: n - 1,
            contains_null: false,
            n,
            max_fill: max_fill(n, f),
            size: 0,
            f,
        }
    }

    /// The set the engine uses for pending block checks: `new LongOpenHashSet(512, 0.5F)`.
    pub fn for_checks() -> Self {
        Self::with_expected(512, 0.5)
    }

    /// Number of elements.
    pub const fn len(&self) -> usize {
        self.size
    }

    /// `true` when empty.
    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Current table length (without the null slot).
    pub const fn table_len(&self) -> usize {
        self.n
    }

    /// Adds `k`; returns `true` when it was absent.
    pub fn insert(&mut self, k: i64) -> bool {
        if k == 0 {
            if self.contains_null {
                return false;
            }
            self.contains_null = true;
        } else {
            let mut pos = mix(k) as usize & self.mask;
            loop {
                let cur = self.key[pos];
                if cur == 0 {
                    break;
                }
                if cur == k {
                    return false;
                }
                pos = (pos + 1) & self.mask;
            }
            self.key[pos] = k;
        }
        let before = self.size;
        self.size += 1;
        if before >= self.max_fill {
            self.rehash(array_size(self.size as u64 + 1, self.f));
        }
        true
    }

    /// Membership test.
    pub fn contains(&self, k: i64) -> bool {
        if k == 0 {
            return self.contains_null;
        }
        let mut pos = mix(k) as usize & self.mask;
        loop {
            let cur = self.key[pos];
            if cur == 0 {
                return false;
            }
            if cur == k {
                return true;
            }
            pos = (pos + 1) & self.mask;
        }
    }

    fn rehash(&mut self, new_n: usize) {
        let mask = new_n - 1;
        let mut new_key = vec![0i64; new_n];
        let mut i = self.n;
        let mut left = self.size - usize::from(self.contains_null);
        while left > 0 {
            left -= 1;
            loop {
                i -= 1;
                if self.key[i] != 0 {
                    break;
                }
            }
            let k = self.key[i];
            let mut pos = mix(k) as usize & mask;
            while new_key[pos] != 0 {
                pos = (pos + 1) & mask;
            }
            new_key[pos] = k;
        }
        self.n = new_n;
        self.mask = mask;
        self.max_fill = max_fill(new_n, self.f);
        self.key = new_key;
    }

    /// Removes every element (`clear()`), keeping the table.
    pub fn clear(&mut self) {
        if self.size == 0 {
            return;
        }
        self.size = 0;
        self.contains_null = false;
        self.key.fill(0);
    }

    /// `trim(n)`: shrinks the table to the size fitting `n` elements when that is smaller than the current one.
    pub fn trim(&mut self, n: usize) {
        let l = next_power_of_two((n as f64 / self.f).ceil() as u64) as usize;
        if l >= self.n || self.size > max_fill(l, self.f) {
            return;
        }
        self.rehash(l);
    }

    /// Elements in fastutil iteration order: the null key first, then slots from the highest index down.
    pub fn iter(&self) -> impl Iterator<Item = i64> + '_ {
        self.contains_null
            .then_some(0i64)
            .into_iter()
            .chain(self.key.iter().rev().copied().filter(|&k| k != 0))
    }

    /// The elements in iteration order, collected.
    pub fn to_vec(&self) -> Vec<i64> {
        self.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_matches_spec() {
        let s = LongOpenHashSet::for_checks();
        assert_eq!(s.table_len(), 1024);
        assert_eq!(s.max_fill, 512);
    }

    #[test]
    fn grows_at_513_to_2048() {
        let mut s = LongOpenHashSet::for_checks();
        for k in 1..=512i64 {
            s.insert(k * 7919);
        }
        assert_eq!(s.table_len(), 1024);
        s.insert(999_999_937);
        assert_eq!(s.table_len(), 2048);
        assert_eq!(s.len(), 513);
        s.clear();
        s.trim(512);
        assert_eq!(s.table_len(), 1024);
        assert!(s.is_empty());
    }

    #[test]
    fn null_key_iterates_first_and_is_deduplicated() {
        let mut s = LongOpenHashSet::for_checks();
        assert!(s.insert(5));
        assert!(s.insert(0));
        assert!(!s.insert(0));
        assert!(!s.insert(5));
        assert_eq!(s.iter().next(), Some(0));
        assert_eq!(s.len(), 2);
        assert!(s.contains(0) && s.contains(5) && !s.contains(6));
    }

    #[test]
    fn iteration_is_descending_slot_order() {
        let mut s = LongOpenHashSet::for_checks();
        let keys = [3i64, 1, 4, 1, 5, 9, 2, 6];
        for &k in &keys {
            s.insert(k);
        }
        let slots: Vec<usize> = s
            .iter()
            .map(|k| {
                let mut pos = mix(k) as usize & s.mask;
                while s.key[pos] != k {
                    pos = (pos + 1) & s.mask;
                }
                pos
            })
            .collect();
        assert!(slots.windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn probing_wraps_and_all_keys_stay_reachable() {
        let mut s = LongOpenHashSet::with_expected(4, 0.5);
        for k in 1..=40i64 {
            s.insert(k << 20);
        }
        for k in 1..=40i64 {
            assert!(s.contains(k << 20));
        }
        assert_eq!(s.len(), 40);
    }
}
