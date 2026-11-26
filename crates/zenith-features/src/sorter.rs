//! Global topological feature sorting and cycle detection (`FeatureSorter.buildFeaturesPerStep`,
//! `docs/specs/worldgen/features.md` §1.3).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use thiserror::Error;

use crate::placement::{PlacedFeature, PlacedFeatureId};

/// Sorted features and identity-to-index lookup for a single decoration step (`StepFeatureData`).
#[derive(Debug, Clone)]
pub struct StepFeatureData {
    /// Topologically sorted placed features for this decoration step.
    pub features: Vec<Arc<PlacedFeature>>,
    /// Maps each [`PlacedFeatureId`] (reference identity) to its 0-based index in `features`.
    pub index_mapping: HashMap<PlacedFeatureId, i32>,
}

impl StepFeatureData {
    /// Looks up the global index of `id` in this step.
    #[inline]
    pub fn index_of(&self, id: PlacedFeatureId) -> Option<i32> {
        self.index_mapping.get(&id).copied()
    }
}

/// Error returned when a feature-ordering cycle is detected across biomes.
#[derive(Debug, Clone, Error)]
#[error("Feature order cycle found, involved sources: {involved_sources:?}")]
pub struct FeatureCycleError {
    /// Indices into the input `sources` list belonging to the minimized cycle.
    pub involved_sources: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FeatureNode {
    step: usize,
    feature_index: i32,
    id: PlacedFeatureId,
}

/// Runs `FeatureSorter.buildFeaturesPerStep` (`features.md` §1.3).
///
/// Preserves the exact iteration order of `sources` for `featureIndex` tie-breaking, builds
/// cross-step directed edges, performs ascending `(step, feature_index)` DFS topological sort,
/// and greedily minimizes involved sources when a cycle is detected.
pub fn build_features_per_step<'a, T, F>(
    sources: &'a [T],
    mut feature_getter: F,
    try_reducing_error: bool,
) -> Result<Vec<StepFeatureData>, FeatureCycleError>
where
    F: FnMut(&'a T) -> &'a [Vec<Arc<PlacedFeature>>],
{
    let indices: Vec<usize> = (0..sources.len()).collect();
    match build_for_indices(sources, &indices, &mut feature_getter) {
        Ok(steps) => Ok(steps),
        Err(()) => {
            if !try_reducing_error {
                return Err(FeatureCycleError {
                    involved_sources: indices,
                });
            }
            let mut reduced = indices;
            loop {
                let mut removed_any = false;
                let mut i = 0;
                while i < reduced.len() {
                    let mut candidate = reduced.clone();
                    candidate.remove(i);
                    if build_for_indices(sources, &candidate, &mut feature_getter).is_err() {
                        reduced = candidate;
                        removed_any = true;
                        break;
                    }
                    i += 1;
                }
                if !removed_any {
                    break;
                }
            }
            Err(FeatureCycleError {
                involved_sources: reduced,
            })
        }
    }
}

fn build_for_indices<'a, T, F>(
    sources: &'a [T],
    active_indices: &[usize],
    feature_getter: &mut F,
) -> Result<Vec<StepFeatureData>, ()>
where
    F: FnMut(&'a T) -> &'a [Vec<Arc<PlacedFeature>>],
{
    let mut feature_index: HashMap<PlacedFeatureId, i32> = HashMap::new();
    let mut id_to_arc: HashMap<PlacedFeatureId, Arc<PlacedFeature>> = HashMap::new();
    let mut next_feature_index: i32 = 0;
    let mut edges: BTreeMap<FeatureNode, BTreeSet<FeatureNode>> = BTreeMap::new();
    let mut max_step: usize = 0;

    for &src_idx in active_indices {
        let features_for_step = feature_getter(&sources[src_idx]);
        max_step = max_step.max(features_for_step.len());
        let mut flat: Vec<FeatureNode> = Vec::new();
        for (step, step_features) in features_for_step.iter().enumerate() {
            for pf in step_features {
                let idx = *feature_index.entry(pf.id).or_insert_with(|| {
                    let assigned = next_feature_index;
                    next_feature_index += 1;
                    assigned
                });
                id_to_arc.entry(pf.id).or_insert_with(|| Arc::clone(pf));
                flat.push(FeatureNode {
                    step,
                    feature_index: idx,
                    id: pf.id,
                });
            }
        }
        for i in 0..flat.len() {
            let entry = edges.entry(flat[i]).or_default();
            if i + 1 < flat.len() {
                entry.insert(flat[i + 1]);
            }
        }
    }

    let mut discovered: BTreeSet<FeatureNode> = BTreeSet::new();
    let mut currently_visiting: BTreeSet<FeatureNode> = BTreeSet::new();
    let mut sorted_features: Vec<FeatureNode> = Vec::with_capacity(edges.len());

    for &v in edges.keys() {
        if !discovered.contains(&v)
            && dfs_cycle(
                v,
                &edges,
                &mut discovered,
                &mut currently_visiting,
                &mut sorted_features,
            )
        {
            return Err(());
        }
    }

    sorted_features.reverse();

    let mut result = Vec::with_capacity(max_step);
    for step in 0..max_step {
        let mut features = Vec::new();
        let mut index_mapping = HashMap::new();
        for node in &sorted_features {
            if node.step == step {
                let idx = features.len() as i32;
                index_mapping.insert(node.id, idx);
                features.push(Arc::clone(&id_to_arc[&node.id]));
            }
        }
        result.push(StepFeatureData {
            features,
            index_mapping,
        });
    }

    Ok(result)
}

fn dfs_cycle(
    v: FeatureNode,
    edges: &BTreeMap<FeatureNode, BTreeSet<FeatureNode>>,
    discovered: &mut BTreeSet<FeatureNode>,
    currently_visiting: &mut BTreeSet<FeatureNode>,
    sorted: &mut Vec<FeatureNode>,
) -> bool {
    if discovered.contains(&v) {
        return false;
    }
    if currently_visiting.contains(&v) {
        return true;
    }
    currently_visiting.insert(v);
    if let Some(neighbors) = edges.get(&v) {
        for &next in neighbors {
            if dfs_cycle(next, edges, discovered, currently_visiting, sorted) {
                return true;
            }
        }
    }
    currently_visiting.remove(&v);
    discovered.insert(v);
    sorted.push(v);
    false
}
