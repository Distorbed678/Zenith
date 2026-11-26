//! Leaf-distance propagation (`updateLeaves`) and boundary shape updates
//! (`StructureTemplate.updateShapeAtEdge`), `features-trees.md` §2.4–§2.5.

use zenith_blocks::Direction;

use super::place::Bounds;
use super::shape::{optional_distance, update_shape};
use super::{TREE_UPDATE_FLAGS, TreeCache};
use crate::block_ops::BlockOps;
use crate::java_hash::JavaBlockPosSet;
use crate::region::GenerationRegion;
use crate::rng::BlockPos;

/// Dense 3-D bit set over the tree bounding box (`BitSetDiscreteVoxelShape`).
pub(super) struct VoxelShape {
    size: [i32; 3],
    bits: Vec<bool>,
}

impl VoxelShape {
    fn new(size: [i32; 3]) -> Self {
        let n = (size[0] as usize) * (size[1] as usize) * (size[2] as usize);
        Self {
            size,
            bits: vec![false; n],
        }
    }

    #[inline]
    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        ((x * self.size[1] + y) * self.size[2] + z) as usize
    }

    #[inline]
    fn fill(&mut self, x: i32, y: i32, z: i32) {
        let i = self.index(x, y, z);
        self.bits[i] = true;
    }

    #[inline]
    fn is_full(&self, x: i32, y: i32, z: i32) -> bool {
        self.bits[self.index(x, y, z)]
    }
}

const DISTANCE_NAMES: [&str; 8] = ["0", "1", "2", "3", "4", "5", "6", "7"];

/// `TreeFeature.updateLeaves`.
pub(super) fn update_leaves(
    region: &mut GenerationRegion,
    ops: &BlockOps,
    cache: &TreeCache,
    bounds: &Bounds,
    logs: &JavaBlockPosSet,
    decorations: &JavaBlockPosSet,
    roots: &JavaBlockPosSet,
) -> VoxelShape {
    let size = bounds.span();
    let mut shape = VoxelShape::new(size);
    let local = |p: BlockPos| {
        (
            p.x - bounds.min[0],
            p.y - bounds.min[1],
            p.z - bounds.min[2],
        )
    };
    // `toCheck`: seven `HashSet<BlockPos>`; entries are popped in Java iteration order and a
    // position can sit in several sets at once (a stale entry later overwrites its distance).
    let mut buckets: [JavaBlockPosSet; 7] = std::array::from_fn(|_| JavaBlockPosSet::new());

    for p in decorations.iter().chain(roots.iter()) {
        if bounds.contains(p) {
            let (x, y, z) = local(p);
            shape.fill(x, y, z);
        }
    }
    for p in logs.iter() {
        buckets[0].insert(p);
    }

    let mut smallest: usize = 0;
    loop {
        while smallest < 7 && buckets[smallest].is_empty() {
            smallest += 1;
        }
        if smallest >= 7 {
            return shape;
        }
        let Some(pos) = buckets[smallest].pop_first() else {
            continue;
        };
        if !bounds.contains(pos) {
            continue;
        }
        if smallest != 0 {
            let state = region.get_block(pos);
            if let Some(ns) = ops.set_property(state, "distance", DISTANCE_NAMES[smallest]) {
                region.set_block(pos, ns, TREE_UPDATE_FLAGS);
            }
        }
        let (x, y, z) = local(pos);
        shape.fill(x, y, z);
        for dir in Direction::ALL {
            let n = pos.relative(dir);
            if !bounds.contains(n) {
                continue;
            }
            let (nx, ny, nz) = local(n);
            if shape.is_full(nx, ny, nz) {
                continue;
            }
            let cur = region.get_block(n);
            if let Some(existing) = optional_distance(ops, cache, cur) {
                let new_dist = existing.min(smallest as i32 + 1);
                if new_dist < 7 {
                    let nd = new_dist as usize;
                    buckets[nd].insert(n);
                    smallest = smallest.min(nd);
                }
            }
        }
    }
}

/// `StructureTemplate.updateShapeAtEdge(level, 3, shape, min_x, min_y, min_z)`.
pub(super) fn update_shape_at_edge(
    region: &mut GenerationRegion,
    ops: &BlockOps,
    cache: &TreeCache,
    shape: &VoxelShape,
    bounds: &Bounds,
) {
    let [sx, sy, sz] = shape.size;
    let mut faces: Vec<(Direction, i32, i32, i32)> = Vec::new();
    // Pass 1: Z-axis faces (NORTH / SOUTH).
    for x in 0..sx {
        for y in 0..sy {
            let mut last = false;
            for z in 0..=sz {
                let full = z != sz && shape.is_full(x, y, z);
                if !last && full {
                    faces.push((Direction::North, x, y, z));
                }
                if last && !full {
                    faces.push((Direction::South, x, y, z - 1));
                }
                last = full;
            }
        }
    }
    // Pass 2: Y-axis faces (DOWN / UP), `z` outer, `x` middle.
    for z in 0..sz {
        for x in 0..sx {
            let mut last = false;
            for y in 0..=sy {
                let full = y != sy && shape.is_full(x, y, z);
                if !last && full {
                    faces.push((Direction::Down, x, y, z));
                }
                if last && !full {
                    faces.push((Direction::Up, x, y - 1, z));
                }
                last = full;
            }
        }
    }
    // Pass 3: X-axis faces (WEST / EAST), `y` outer, `z` middle.
    for y in 0..sy {
        for z in 0..sz {
            let mut last = false;
            for x in 0..=sx {
                let full = x != sx && shape.is_full(x, y, z);
                if !last && full {
                    faces.push((Direction::West, x, y, z));
                }
                if last && !full {
                    faces.push((Direction::East, x - 1, y, z));
                }
                last = full;
            }
        }
    }

    for (dir, lx, ly, lz) in faces {
        let pos = BlockPos::new(bounds.min[0] + lx, bounds.min[1] + ly, bounds.min[2] + lz);
        let npos = pos.relative(dir);
        let state = region.get_block(pos);
        let nstate = region.get_block(npos);
        let new_state = update_shape(region, ops, cache, pos, state, dir, npos, nstate);
        if new_state != state {
            region.set_block(pos, new_state, 2);
        }
        let new_nstate = update_shape(
            region,
            ops,
            cache,
            npos,
            nstate,
            dir.opposite(),
            pos,
            new_state,
        );
        if new_nstate != nstate {
            region.set_block(npos, new_nstate, 2);
        }
    }
}
