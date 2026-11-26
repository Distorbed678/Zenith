//! Per-state light inputs read from `zenith-blocks` (`light-engine.md` Inputs A).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zenith_blocks::{BlockFacts, BoolOp, Direction, ShapeId, ShapeKind, VoxelShape, flag};
use zenith_registry::{BlockStateTable, Identifier};

const EPS: f64 = 1.0e-7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShapeClass {
    Empty,
    Block,
    Other,
}

/// Dense per-state light tables plus the shape-pair occlusion cache.
pub struct LightFacts {
    facts: Arc<BlockFacts>,
    /// `dampening` per state.
    dampening: Vec<u8>,
    emission: Vec<u8>,
    is_air: Vec<bool>,
    empty_shape: Vec<bool>,
    use_shape: Vec<bool>,
    /// `lightShape(s, d)` ids, `state * 6 + d`.
    light_shape: Vec<ShapeId>,
    class: Vec<ShapeClass>,
    pair_cache: Mutex<HashMap<(u32, u32), bool>>,
    air_state: u32,
    bedrock_state: u32,
}

impl std::fmt::Debug for LightFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LightFacts")
            .field("states", &self.dampening.len())
            .field("air_state", &self.air_state)
            .field("bedrock_state", &self.bedrock_state)
            .finish()
    }
}

impl LightFacts {
    /// Builds the tables. `air_state` is the plain air default state, `bedrock_state` the state read for missing
    /// chunks.
    pub fn new(facts: Arc<BlockFacts>, air_state: u32, bedrock_state: u32) -> Self {
        let n = facts.len();
        let mut dampening = Vec::with_capacity(n);
        let mut emission = Vec::with_capacity(n);
        let mut is_air = Vec::with_capacity(n);
        let mut empty_shape = Vec::with_capacity(n);
        let mut use_shape = Vec::with_capacity(n);
        let mut light_shape = Vec::with_capacity(n * 6);
        let empty_id = (0..facts.shape_count() as u32)
            .map(ShapeId)
            .find(|&id| facts.shape(id).is_empty())
            .unwrap_or(ShapeId(0));
        for s in facts.states() {
            dampening.push(s.light_block);
            emission.push(s.light_emission);
            is_air.push(s.has(flag::IS_AIR));
            let uses = s.has(flag::USE_SHAPE_FOR_LIGHT_OCCLUSION);
            use_shape.push(uses);
            let empty = !s.has(flag::CAN_OCCLUDE) || !uses;
            empty_shape.push(empty);
            for d in 0..6 {
                light_shape.push(if empty { empty_id } else { s.face_occlusion[d] });
            }
        }
        let class = (0..facts.shape_count() as u32)
            .map(|i| {
                let sh = facts.shape(ShapeId(i));
                if sh.is_empty() {
                    ShapeClass::Empty
                } else if sh.is_block() {
                    ShapeClass::Block
                } else {
                    ShapeClass::Other
                }
            })
            .collect();
        Self {
            facts,
            dampening,
            emission,
            is_air,
            empty_shape,
            use_shape,
            light_shape,
            class,
            pair_cache: Mutex::new(HashMap::new()),
            air_state,
            bedrock_state,
        }
    }

    /// Builds the tables, looking the air and bedrock default states up in `table`.
    ///
    /// Returns `None` when `minecraft:air` or `minecraft:bedrock` is missing.
    pub fn from_table(facts: Arc<BlockFacts>, table: &BlockStateTable) -> Option<Self> {
        let air = table
            .block(&Identifier::parse("minecraft:air").ok()?)?
            .default_state;
        let bedrock = table
            .block(&Identifier::parse("minecraft:bedrock").ok()?)?
            .default_state;
        Some(Self::new(facts, air, bedrock))
    }

    /// The underlying block facts.
    pub fn block_facts(&self) -> &Arc<BlockFacts> {
        &self.facts
    }

    /// The plain air state id.
    pub const fn air_state(&self) -> u32 {
        self.air_state
    }

    /// The state read for positions in missing chunks (bedrock).
    pub const fn bedrock_state(&self) -> u32 {
        self.bedrock_state
    }

    /// `getLightDampening`.
    #[inline]
    pub fn dampening(&self, s: u32) -> u8 {
        self.dampening[s as usize]
    }

    /// `max(1, dampening)`: the light lost entering this block.
    #[inline]
    pub fn opacity(&self, s: u32) -> u8 {
        self.dampening[s as usize].max(1)
    }

    /// Light emission.
    #[inline]
    pub fn emission(&self, s: u32) -> u8 {
        self.emission[s as usize]
    }

    /// Whether the state is any air (`isAir`: air, cave air, void air).
    #[inline]
    pub fn is_air(&self, s: u32) -> bool {
        self.is_air[s as usize]
    }

    /// `!canOcclude || !useShapeForLightOcclusion`.
    #[inline]
    pub fn empty_shape(&self, s: u32) -> bool {
        self.empty_shape[s as usize]
    }

    /// `useShapeForLightOcclusion`.
    #[inline]
    pub fn use_shape_for_light_occlusion(&self, s: u32) -> bool {
        self.use_shape[s as usize]
    }

    #[inline]
    fn light_shape_id(&self, s: u32, d: Direction) -> ShapeId {
        self.light_shape[s as usize * 6 + d.ordinal()]
    }

    /// `shapeOccludes(from, to, d)`: do the light shapes of the two blocks facing each other across `d` close the
    /// gap? (`faceShapeOccludes(lightShape(from, d), lightShape(to, opposite(d)))`.)
    #[inline]
    pub fn shape_occludes(&self, from: u32, to: u32, d: Direction) -> bool {
        if self.empty_shape[from as usize] && self.empty_shape[to as usize] {
            return false;
        }
        self.face_shape_occludes(
            self.light_shape_id(from, d),
            self.light_shape_id(to, d.opposite()),
        )
    }

    fn face_shape_occludes(&self, a: ShapeId, b: ShapeId) -> bool {
        let (ca, cb) = (self.class[a.0 as usize], self.class[b.0 as usize]);
        if ca == ShapeClass::Block || cb == ShapeClass::Block {
            return true;
        }
        if ca == ShapeClass::Empty && cb == ShapeClass::Empty {
            return false;
        }
        let key = (a.0.min(b.0), a.0.max(b.0));
        let mut cache = self.pair_cache.lock().unwrap_or_else(|e| e.into_inner());
        *cache.entry(key).or_insert_with(|| {
            let union = self.facts.shape(a).or(self.facts.shape(b));
            !VoxelShape::block().join_is_not_empty(&union, BoolOp::OnlyFirst)
        })
    }

    /// `hasDifferentLightProperties(old, new)`: the only gate for "a block change affects light".
    pub fn has_different_light_properties(&self, old: u32, new: u32) -> bool {
        if old == new {
            return false;
        }
        self.dampening(old) != self.dampening(new)
            || self.emission(old) != self.emission(new)
            || self.use_shape_for_light_occlusion(new)
            || self.use_shape_for_light_occlusion(old)
    }

    /// `getLightDampeningInto(from, to, d, simple)`: `simple`, or 16 when the whole occlusion shapes merge to
    /// close the face.
    pub fn light_dampening_into(&self, from: u32, to: u32, d: Direction, simple: u8) -> u8 {
        if self.empty_shape(from) && self.empty_shape(to) {
            return simple;
        }
        let shape_of = |s: u32| -> VoxelShape {
            if self.empty_shape(s) {
                VoxelShape::empty()
            } else {
                self.facts.shape_at_origin(s, ShapeKind::Occlusion).clone()
            }
        };
        if merged_face_occludes(&shape_of(from), &shape_of(to), d) {
            16
        } else {
            simple
        }
    }
}

/// `mergedFaceOccludes(S, O, d)` (`shapes.md` §4.4).
pub fn merged_face_occludes(s: &VoxelShape, o: &VoxelShape, d: Direction) -> bool {
    if s.is_block() || o.is_block() {
        return true;
    }
    let (first, second) = if d.is_positive() { (s, o) } else { (o, s) };
    let a = d.axis();
    let first = if first.is_empty() || first.max(a) < 1.0 - EPS {
        VoxelShape::empty()
    } else {
        first.clone()
    };
    let second = if second.is_empty() || second.min(a) > EPS {
        VoxelShape::empty()
    } else {
        second.clone()
    };
    if first.is_empty() && second.is_empty() {
        return false;
    }
    let (pos, neg) = match a {
        0 => (Direction::East, Direction::West),
        1 => (Direction::Up, Direction::Down),
        _ => (Direction::South, Direction::North),
    };
    let top = first.face_shape(pos);
    let bottom = second.face_shape(neg);
    let union = top.or(&bottom);
    !VoxelShape::block().join_is_not_empty(&union, BoolOp::OnlyFirst)
}
