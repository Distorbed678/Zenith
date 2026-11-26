//! Iteration order of a fastutil `LongOpenHashSet` (default capacity, load factor `0.75`).
//!
//! Vanilla keeps the start chunks that reference a chunk for one structure in such a set and
//! visits them in its iteration order, which decides the order in which several starts of the
//! same structure place their pieces (and so which one wins where they overlap, and how the
//! shared decoration random advances).

/// `HashCommon.LONG_PHI`.
const LONG_PHI: i64 = 0x9E37_79B9_7F4A_7C15_u64 as i64;

/// `HashCommon.mix(long)` as an `int`.
#[inline]
fn mix(x: i64) -> i32 {
    let mut h = x.wrapping_mul(LONG_PHI);
    h ^= ((h as u64) >> 32) as i64;
    (h ^ (((h as u64) >> 16) as i64)) as i32
}

/// `HashCommon.arraySize(expected, 0.75)`.
fn array_size(expected: usize) -> usize {
    let want = ((expected as f64) / 0.75).ceil() as usize;
    want.max(2).next_power_of_two()
}

/// `HashCommon.maxFill(n, 0.75)`.
fn max_fill(n: usize) -> usize {
    (((n as f64) * 0.75).ceil() as usize).min(n - 1)
}

/// The elements of a `LongOpenHashSet` filled by `add` calls in `insertion` order (duplicates
/// ignored), in the order its iterator returns them: the zero key first, then the table from
/// the last slot down to the first.
pub fn long_open_hash_set_order(insertion: &[i64]) -> Vec<i64> {
    let mut n = array_size(16);
    let mut keys: Vec<i64> = vec![0; n];
    let mut contains_zero = false;
    let mut size = 0usize;
    let mut fill = max_fill(n);
    for &k in insertion {
        if k == 0 {
            if contains_zero {
                continue;
            }
            contains_zero = true;
        } else {
            let mask = n - 1;
            let mut pos = (mix(k) as usize) & mask;
            let mut found = false;
            while keys[pos] != 0 {
                if keys[pos] == k {
                    found = true;
                    break;
                }
                pos = (pos + 1) & mask;
            }
            if found {
                continue;
            }
            keys[pos] = k;
        }
        let before = size;
        size += 1;
        if before >= fill {
            // rehash(arraySize(size + 1, f))
            let new_n = array_size(size + 1);
            let mask = new_n - 1;
            let mut new_keys = vec![0i64; new_n];
            for i in (0..n).rev() {
                let key = keys[i];
                if key != 0 {
                    let mut pos = (mix(key) as usize) & mask;
                    while new_keys[pos] != 0 {
                        pos = (pos + 1) & mask;
                    }
                    new_keys[pos] = key;
                }
            }
            n = new_n;
            keys = new_keys;
            fill = max_fill(n);
        }
    }
    let mut out = Vec::with_capacity(size);
    if contains_zero {
        out.push(0);
    }
    for i in (0..n).rev() {
        if keys[i] != 0 {
            out.push(keys[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_every_element_once() {
        let v = [5_i64, 1 << 32, -3, 5, 0, 77, 1 << 40];
        let mut o = long_open_hash_set_order(&v);
        assert_eq!(o.len(), 6);
        o.sort_unstable();
        assert_eq!(o, vec![-3, 0, 5, 77, 1 << 32, 1 << 40]);
    }

    #[test]
    fn growth_keeps_elements() {
        let v: Vec<i64> = (1..200).map(|i| i * 7919).collect();
        let mut o = long_open_hash_set_order(&v);
        o.sort_unstable();
        assert_eq!(o, v);
    }
}
