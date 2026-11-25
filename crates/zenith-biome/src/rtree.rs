//! 19-way 7D R-tree (`ParameterList<T>`) and explicit warm-start [`SearchState`].
//!
//! Implements Section 2 ("Search structure: 19-way 7D R-tree construction" and
//! "Exact R-tree lookup and tie-breaking") of `docs/specs/worldgen/biomes-and-climate.md`.

use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use thiserror::Error;
use zenith_math::helpers::intrinsics::{java_math_log, java_math_pow};

use crate::climate::{PARAMETER_COUNT, Parameter, ParameterPoint, TargetPoint, squared_distance};

/// Maximum children per internal R-tree node in 26.3 (`Climate.RTree.CHILDREN_PER_NODE = 19`).
pub const CHILDREN_PER_NODE: usize = 19;

/// Epsilon subtracted from `nodes.len()` before `log(n - 0.01) / log(19)` during bucketization.
const BUCKET_EPSILON: f64 = 0.01_f64;

static NEXT_TREE_ID: AtomicU64 = AtomicU64::new(1);

/// Errors raised when building a [`ParameterList`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParameterListError {
    /// Attempted to construct a [`ParameterList`] with zero entries.
    #[error("parameter list must contain at least one entry")]
    Empty,
}

/// Explicit warm-start state for [`ParameterList::find_value`].
///
/// In vanilla, each `Climate.RTree` keeps its `lastResult` leaf in a `ThreadLocal`. Because
/// `SubTree::search` uses strict inequalities (`minDist > childDist` and `minDist > leafDist`),
/// `lastResult` wins any exact distance tie against other leaves regardless of tree order.
///
/// Zenith exposes this state explicitly instead of using a hidden thread-local:
/// - To reproduce vanilla's per-thread warm-start across chunks on a worker thread, keep one
///   `SearchState` per `(ParameterList, worker thread)` and pass it into every lookup on that
///   thread (following the exact ascending-section, X-outer / Y-middle / Z-inner chunk fill loop).
/// - To simulate a fresh worker thread at the start of a chunk or independent query, start from
///   [`SearchState::default()`] (or call [`SearchState::reset`]).
///
/// Each [`ParameterList`] has an internal unique ID, so reusing a `SearchState` across two
/// different `ParameterList` instances safely treats the warm-start as empty for the other tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchState {
    tree_id: u64,
    last_leaf: Option<usize>,
}

impl SearchState {
    /// Creates a fresh empty warm-start state (`lastResult = None`).
    #[inline]
    pub const fn new() -> Self {
        Self {
            tree_id: 0,
            last_leaf: None,
        }
    }

    /// Clears the cached warm-start leaf (`lastResult = None`).
    #[inline]
    pub fn reset(&mut self) {
        self.tree_id = 0;
        self.last_leaf = None;
    }

    /// Returns the cached leaf index (if any) for diagnostics/testing.
    #[inline]
    pub const fn last_leaf_index(&self) -> Option<usize> {
        self.last_leaf
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeRef {
    Leaf(usize),
    SubTree(usize),
}

#[derive(Debug, Clone, Copy)]
struct LeafNode {
    space: [Parameter; PARAMETER_COUNT],
    entry_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct SubTreeNode {
    space: [Parameter; PARAMETER_COUNT],
    children_start: usize,
    children_end: usize,
}

/// Temporary recursive build node used during R-tree construction before flattening into arenas.
#[derive(Clone)]
struct BuildNode {
    space: [Parameter; PARAMETER_COUNT],
    kind: BuildKind,
}

#[derive(Clone)]
enum BuildKind {
    Leaf(usize),
    SubTree(Vec<BuildNode>),
}

/// Ordered climate parameter list compiled into an immutable 19-way 7D R-tree.
#[derive(Debug, Clone)]
pub struct ParameterList<T> {
    tree_id: u64,
    entries: Vec<(ParameterPoint, T)>,
    leaves: Vec<LeafNode>,
    subtrees: Vec<SubTreeNode>,
    children: Vec<NodeRef>,
    root: NodeRef,
}

impl<T> ParameterList<T> {
    /// Builds a [`ParameterList`] and compiles its 19-way 7D R-tree from `entries`.
    ///
    /// Preserves `entries` in their exact input order. Fails with [`ParameterListError::Empty`]
    /// if `entries` is empty.
    pub fn new(entries: Vec<(ParameterPoint, T)>) -> Result<Self, ParameterListError> {
        if entries.is_empty() {
            return Err(ParameterListError::Empty);
        }

        let mut leaves = Vec::with_capacity(entries.len());
        let mut build_nodes = Vec::with_capacity(entries.len());
        for (entry_index, (point, _)) in entries.iter().enumerate() {
            let space = point.parameter_space();
            leaves.push(LeafNode { space, entry_index });
            build_nodes.push(BuildNode {
                space,
                kind: BuildKind::Leaf(entry_index),
            });
        }

        let built_root = build_tree(build_nodes);

        let mut subtrees = Vec::new();
        let mut children = Vec::new();
        let root = flatten_node(built_root, &mut subtrees, &mut children);
        let tree_id = NEXT_TREE_ID.fetch_add(1, AtomicOrdering::Relaxed);

        Ok(Self {
            tree_id,
            entries,
            leaves,
            subtrees,
            children,
            root,
        })
    }

    /// Number of entries in this parameter list (always `>= 1`).
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Always `false` because a [`ParameterList`] is guaranteed non-empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the original ordered `(ParameterPoint, T)` entries.
    #[inline]
    pub fn entries(&self) -> &[(ParameterPoint, T)] {
        &self.entries
    }

    /// Finds the closest entry to `target` using the exact 19-way R-tree and updates `state`
    /// with the winning leaf for subsequent warm-started queries.
    #[inline]
    pub fn find_value(&self, target: TargetPoint, state: &mut SearchState) -> &T {
        let leaf_idx = self.find_leaf_index(target, state);
        &self.entries[self.leaves[leaf_idx].entry_index].1
    }

    /// Finds the closest entry index (into [`ParameterList::entries`]) using the R-tree and
    /// updates `state`.
    #[inline]
    pub fn find_entry_index(&self, target: TargetPoint, state: &mut SearchState) -> usize {
        let leaf_idx = self.find_leaf_index(target, state);
        self.leaves[leaf_idx].entry_index
    }

    /// Convenience search with a cold warm-start (`lastResult = None`).
    #[inline]
    pub fn find_value_cold(&self, target: TargetPoint) -> &T {
        let mut state = SearchState::new();
        self.find_value(target, &mut state)
    }

    /// Linear scan over [`ParameterList::entries`] in original list order (diagnostic helper
    /// matching `Climate.ParameterList#findValueBruteForce`).
    ///
    /// Note that brute-force tie-breaking can differ from [`ParameterList::find_value`] when
    /// multiple entries share the exact minimum squared distance.
    pub fn find_value_brute_force(&self, target: TargetPoint) -> &T {
        let target_arr = target.to_parameter_array();
        let mut best_dist = i64::MAX;
        let mut best_idx = 0usize;
        for (i, leaf) in self.leaves.iter().enumerate() {
            let d = squared_distance(&leaf.space, &target_arr);
            if d < best_dist {
                best_dist = d;
                best_idx = i;
            }
        }
        &self.entries[self.leaves[best_idx].entry_index].1
    }

    #[inline]
    fn find_leaf_index(&self, target: TargetPoint, state: &mut SearchState) -> usize {
        let target_arr = target.to_parameter_array();
        let candidate = if state.tree_id == self.tree_id {
            state.last_leaf
        } else {
            None
        };
        let leaf_idx = self.search_node(self.root, &target_arr, candidate);
        state.tree_id = self.tree_id;
        state.last_leaf = Some(leaf_idx);
        leaf_idx
    }

    fn search_node(
        &self,
        node: NodeRef,
        target: &[i64; PARAMETER_COUNT],
        candidate: Option<usize>,
    ) -> usize {
        match node {
            NodeRef::Leaf(idx) => idx,
            NodeRef::SubTree(st_idx) => {
                let mut min_dist = match candidate {
                    Some(leaf_idx) => squared_distance(&self.leaves[leaf_idx].space, target),
                    None => i64::MAX,
                };
                let mut closest = candidate;
                let st = self.subtrees[st_idx];
                let children = &self.children[st.children_start..st.children_end];
                for &child in children {
                    let child_dist = match child {
                        NodeRef::Leaf(l) => squared_distance(&self.leaves[l].space, target),
                        NodeRef::SubTree(s) => squared_distance(&self.subtrees[s].space, target),
                    };
                    if min_dist > child_dist {
                        match child {
                            NodeRef::Leaf(l) => {
                                // Leaf::search returns itself, so leaf_dist == child_dist < min_dist.
                                min_dist = child_dist;
                                closest = Some(l);
                            }
                            NodeRef::SubTree(_) => {
                                let leaf = self.search_node(child, target, closest);
                                let leaf_dist = squared_distance(&self.leaves[leaf].space, target);
                                if min_dist > leaf_dist {
                                    min_dist = leaf_dist;
                                    closest = Some(leaf);
                                }
                            }
                        }
                    }
                }
                closest.expect("non-empty R-tree search must find a leaf")
            }
        }
    }
}

/// Computes `M(node) = sum_{d=0..6} |(min_d +_64 max_d) /_trunc 2|` in wrapping 64-bit arithmetic.
#[inline]
fn total_magnitude(space: &[Parameter; PARAMETER_COUNT]) -> i64 {
    let mut sum: i64 = 0;
    for p in space {
        sum = sum.wrapping_add(p.midpoint().wrapping_abs());
    }
    sum
}

/// Computes the 7D union bounding box of a non-empty slice of nodes.
fn bounding_box(nodes: &[BuildNode]) -> [Parameter; PARAMETER_COUNT] {
    let mut space = nodes[0].space;
    for node in &nodes[1..] {
        for (acc, p) in space.iter_mut().zip(&node.space) {
            if p.min < acc.min {
                acc.min = p.min;
            }
            if p.max > acc.max {
                acc.max = p.max;
            }
        }
    }
    space
}

/// Cyclic 7-axis comparator starting at `start_dim`, comparing either signed midpoints or
/// absolute midpoints.
#[inline]
fn compare_cyclic(
    a: &[Parameter; PARAMETER_COUNT],
    b: &[Parameter; PARAMETER_COUNT],
    start_dim: usize,
    abs: bool,
) -> Ordering {
    for offset in 0..PARAMETER_COUNT {
        let k = (start_dim + offset) % PARAMETER_COUNT;
        let ka = if abs {
            a[k].midpoint().wrapping_abs()
        } else {
            a[k].midpoint()
        };
        let kb = if abs {
            b[k].midpoint().wrapping_abs()
        } else {
            b[k].midpoint()
        };
        let ord = ka.cmp(&kb);
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

/// L1 semi-perimeter sum across all buckets in wrapping 64-bit signed arithmetic.
fn split_cost(buckets: &[([Parameter; PARAMETER_COUNT], Vec<BuildNode>)]) -> i64 {
    let mut total: i64 = 0;
    for (space, _) in buckets {
        for p in space {
            total = total.wrapping_add(p.max.wrapping_sub(p.min).wrapping_abs());
        }
    }
    total
}

/// Exact recursive 19-way R-tree build matching spec §2.
fn build_tree(mut nodes: Vec<BuildNode>) -> BuildNode {
    debug_assert!(!nodes.is_empty());

    if nodes.len() == 1 {
        return nodes.pop().unwrap();
    }

    if nodes.len() <= CHILDREN_PER_NODE {
        // Stable sort ascending by totalMagnitude (preserves relative order of ties).
        nodes.sort_by_key(|n| total_magnitude(&n.space));
        let space = bounding_box(&nodes);
        return BuildNode {
            space,
            kind: BuildKind::SubTree(nodes),
        };
    }

    // Split case: nodes.len() > 19.
    let exp =
        (java_math_log(nodes.len() as f64 - BUCKET_EPSILON) / java_math_log(19.0_f64)).floor();
    let bucket_size = (java_math_pow(19.0_f64, exp) as i32) as usize;
    debug_assert!(bucket_size >= 1);

    let mut best_cost = i64::MAX;
    let mut best_dim: usize = 0;
    let mut best_buckets: Option<Vec<([Parameter; PARAMETER_COUNT], Vec<BuildNode>)>> = None;

    for d in 0..PARAMETER_COUNT {
        // In-place stable sort across iterations d = 0..6 (exact spec rule).
        nodes.sort_by(|a, b| compare_cyclic(&a.space, &b.space, d, false));

        // Bucketize sequentially into contiguous groups of `bucket_size`, copying slices so
        // subsequent iterations' in-place sorts do not mutate captured buckets.
        let mut buckets = Vec::with_capacity(nodes.len().div_ceil(bucket_size));
        for chunk in nodes.chunks(bucket_size) {
            let children = chunk.to_vec();
            let space = bounding_box(&children);
            buckets.push((space, children));
        }

        let cost = split_cost(&buckets);
        if best_cost > cost {
            best_cost = cost;
            best_dim = d;
            best_buckets = Some(buckets);
        }
    }

    let mut buckets = best_buckets.expect("at least one split dimension must be chosen");
    // Stable sort best buckets by cyclic absolute midpoint starting at best_dim.
    buckets.sort_by(|a, b| compare_cyclic(&a.0, &b.0, best_dim, true));

    let built_children: Vec<BuildNode> = buckets
        .into_iter()
        .map(|(_, group)| build_tree(group))
        .collect();
    let space = bounding_box(&built_children);
    BuildNode {
        space,
        kind: BuildKind::SubTree(built_children),
    }
}

fn flatten_node(
    node: BuildNode,
    subtrees: &mut Vec<SubTreeNode>,
    children_arena: &mut Vec<NodeRef>,
) -> NodeRef {
    match node.kind {
        BuildKind::Leaf(idx) => NodeRef::Leaf(idx),
        BuildKind::SubTree(child_nodes) => {
            let child_refs: Vec<NodeRef> = child_nodes
                .into_iter()
                .map(|c| flatten_node(c, subtrees, children_arena))
                .collect();
            let children_start = children_arena.len();
            children_arena.extend(child_refs);
            let children_end = children_arena.len();
            let st_idx = subtrees.len();
            subtrees.push(SubTreeNode {
                space: node.space,
                children_start,
                children_end,
            });
            NodeRef::SubTree(st_idx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt_1d(t: f32) -> ParameterPoint {
        ParameterPoint::new(
            Parameter::point(t),
            Parameter::point(0.0),
            Parameter::point(0.0),
            Parameter::point(0.0),
            Parameter::point(0.0),
            Parameter::point(0.0),
            0.0,
        )
    }

    #[test]
    fn empty_list_is_rejected() {
        let res: Result<ParameterList<u32>, _> = ParameterList::new(Vec::new());
        assert_eq!(res.unwrap_err(), ParameterListError::Empty);
    }

    #[test]
    fn single_entry_is_not_wrapped_in_subtree() {
        let list = ParameterList::new(vec![(pt_1d(0.25), "only")]).unwrap();
        assert!(matches!(list.root, NodeRef::Leaf(0)));
        assert!(list.subtrees.is_empty());
        let mut state = SearchState::new();
        assert_eq!(
            *list.find_value(
                TargetPoint::from_floats(1.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                &mut state
            ),
            "only"
        );
    }

    #[test]
    fn small_list_total_magnitude_tie_breaking_and_warm_start() {
        // Two leaves at identical distance from target T=0: -0.5 (mag 5000) and +0.5 (mag 5000).
        // Input order places "neg" before "pos"; totalMagnitude is equal (5000), so stable sort
        // keeps "neg" first in the single SubTree.
        let list = ParameterList::new(vec![(pt_1d(-0.5), "neg"), (pt_1d(0.5), "pos")]).unwrap();
        let origin = TargetPoint::from_floats(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        // Cold search at origin: tie between "neg" and "pos" -> first in tree ("neg") wins.
        let mut state = SearchState::new();
        assert_eq!(*list.find_value(origin, &mut state), "neg");

        // Warm-start on "pos", then query origin again: strict `>` means `lastResult` ("pos") wins!
        let near_pos = TargetPoint::from_floats(0.6, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(*list.find_value(near_pos, &mut state), "pos");
        assert_eq!(*list.find_value(origin, &mut state), "pos");

        // Resetting state restores cold tie-breaking ("neg").
        state.reset();
        assert_eq!(*list.find_value(origin, &mut state), "neg");
    }

    #[test]
    fn rtree_matches_brute_force_across_19_way_boundaries() {
        // Test exact boundary sizes around 19 and 19^2 = 361 (spec open question #4).
        for count in [19usize, 20, 38, 361, 362] {
            let entries: Vec<(ParameterPoint, usize)> = (0..count)
                .map(|i| {
                    let v = -1.0_f32 + (i as f32) * (2.0_f32 / count as f32);
                    let h = ((i * 7) % count) as f32 / count as f32 - 0.5_f32;
                    let pt = ParameterPoint::new(
                        Parameter::point(v),
                        Parameter::point(h),
                        Parameter::point(0.0),
                        Parameter::point(0.0),
                        Parameter::point(0.0),
                        Parameter::point(0.0),
                        0.0,
                    );
                    (pt, i)
                })
                .collect();
            let list = ParameterList::new(entries).unwrap();
            let mut state = SearchState::new();
            for step in -20..=20 {
                let t = step as f32 * 0.05;
                let h = ((step * 3) as f32 * 0.02).clamp(-1.0, 1.0);
                let target = TargetPoint::from_floats(t, h, 0.0, 0.0, 0.0, 0.0);
                let rtree_idx = *list.find_value(target, &mut state);
                let brute_idx = *list.find_value_brute_force(target);
                let d_rtree = list.entries()[rtree_idx].0.fitness(&target);
                let d_brute = list.entries()[brute_idx].0.fitness(&target);
                assert_eq!(
                    d_rtree, d_brute,
                    "count={count}, step={step}: R-tree distance {d_rtree} != brute {d_brute}"
                );
            }
        }
    }

    #[test]
    fn reusing_search_state_across_different_trees_is_safe() {
        let big = ParameterList::new(
            (0..50)
                .map(|i| (pt_1d(-1.0 + i as f32 * 0.04), i))
                .collect(),
        )
        .unwrap();
        let small = ParameterList::new(vec![(pt_1d(0.0), "small")]).unwrap();
        let mut state = SearchState::new();
        let _ = big.find_value(
            TargetPoint::from_floats(0.95, 0.0, 0.0, 0.0, 0.0, 0.0),
            &mut state,
        );
        // Searching `small` with `big`'s state must not panic even if `last_leaf >= small.len()`.
        assert_eq!(
            *small.find_value(
                TargetPoint::from_floats(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                &mut state
            ),
            "small"
        );
    }

    #[test]
    fn nether_rtree_root_order_matches_spec_section_6_1() {
        // Spec §6.1: N = 5 <= 19 produces a single root SubTree sorted stably by totalMagnitude:
        // nether_wastes (0), crimson_forest (4000), soul_sand_valley (5000), basalt_deltas (6750), warped_forest (8750).
        let list = crate::presets::nether_parameter_list();
        let NodeRef::SubTree(root_idx) = list.root else {
            panic!("expected SubTree root for 5 entries");
        };
        let st = list.subtrees[root_idx];
        let ordered_paths: Vec<&str> = list.children[st.children_start..st.children_end]
            .iter()
            .map(|c| match *c {
                NodeRef::Leaf(l) => list.entries[list.leaves[l].entry_index].1.path(),
                NodeRef::SubTree(_) => panic!("expected only Leaf children"),
            })
            .collect();
        assert_eq!(
            ordered_paths,
            vec![
                "nether_wastes",
                "crimson_forest",
                "soul_sand_valley",
                "basalt_deltas",
                "warped_forest",
            ]
        );
    }

    #[test]
    fn overworld_rtree_matches_brute_force_optimal_distance() {
        let list = crate::presets::overworld_parameter_list();
        let mut state = SearchState::new();
        // Deterministic pseudo-random sampling across full Overworld climate space + boundaries.
        let mut rng: u64 = 0xDEAD_BEEF_CAFE_BABE;
        let mut next_coord = || -> i64 {
            rng = rng
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((rng >> 33) as i64) % 24_001 - 12_000
        };
        for i in 0..500 {
            let target = TargetPoint::new(
                next_coord(),
                next_coord(),
                next_coord(),
                next_coord(),
                next_coord(),
                next_coord(),
            );
            let rtree_entry = list.find_entry_index(target, &mut state);
            let d_rtree = list.entries()[rtree_entry].0.fitness(&target);
            let mut min_brute = i64::MAX;
            for (pt, _) in list.entries() {
                let d = pt.fitness(&target);
                if d < min_brute {
                    min_brute = d;
                }
            }
            assert_eq!(
                d_rtree, min_brute,
                "overworld sample {i}: R-tree distance {d_rtree} != minimum {min_brute}"
            );
        }
    }

    #[test]
    fn negative_midpoint_truncates_toward_zero() {
        // (-1 + 0) /_trunc 2 == 0, matching Java integer division (not Euclidean floor -1).
        let p = Parameter::from_quantized(-1, 0).unwrap();
        assert_eq!(p.midpoint(), 0);
        let p2 = Parameter::from_quantized(-3, 0).unwrap();
        assert_eq!(p2.midpoint(), -1);
    }
}
