//! The shape of every block family (`docs/specs/blocks/shapes-catalogue.md`).

use crate::BuildError;
use crate::shape::{Direction, Transform, VoxelShape};
use crate::table::BlockInfo;

/// One block state's properties plus its table row.
pub(crate) struct Ctx<'a> {
    pub info: &'a BlockInfo,
    pub props: &'a [(String, String)],
}

impl Ctx<'_> {
    fn raw(&self, name: &str) -> Result<&str, BuildError> {
        self.props
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .ok_or_else(|| BuildError::MissingProperty {
                block: self.info.id.clone(),
                property: name.to_owned(),
            })
    }

    pub fn has(&self, name: &str) -> bool {
        self.props.iter().any(|(k, _)| k == name)
    }

    pub fn str(&self, name: &str) -> Result<&str, BuildError> {
        self.raw(name)
    }

    pub fn bool(&self, name: &str) -> Result<bool, BuildError> {
        Ok(self.raw(name)? == "true")
    }

    pub fn int(&self, name: &str) -> Result<i32, BuildError> {
        let v = self.raw(name)?;
        v.parse().map_err(|_| BuildError::BadValue {
            block: self.info.id.clone(),
            property: name.to_owned(),
            value: v.to_owned(),
        })
    }

    pub fn dir(&self, name: &str) -> Result<Direction, BuildError> {
        let v = self.raw(name)?;
        Direction::from_name(v).ok_or_else(|| BuildError::BadValue {
            block: self.info.id.clone(),
            property: name.to_owned(),
            value: v.to_owned(),
        })
    }

    pub fn facing(&self) -> Result<Direction, BuildError> {
        self.dir("facing")
    }

    fn bad(&self, name: &str) -> BuildError {
        BuildError::BadValue {
            block: self.info.id.clone(),
            property: name.to_owned(),
            value: self.raw(name).unwrap_or("").to_owned(),
        }
    }
}

/// The shapes a family defines for one state (before the context-independent defaults are applied).
pub(crate) struct FamilyShapes {
    pub outline: VoxelShape,
    pub collision: Option<VoxelShape>,
    pub visual: Option<VoxelShape>,
    pub support: Option<VoxelShape>,
    pub interaction: Option<VoxelShape>,
    pub occlusion: Option<VoxelShape>,
    /// The shapes move with the block's random position offset.
    pub offset: bool,
}

impl FamilyShapes {
    fn new(outline: VoxelShape) -> Self {
        Self {
            outline,
            collision: None,
            visual: None,
            support: None,
            interaction: None,
            occlusion: None,
            offset: false,
        }
    }
    fn collision(mut self, s: VoxelShape) -> Self {
        self.collision = Some(s);
        self
    }
    fn visual(mut self, s: VoxelShape) -> Self {
        self.visual = Some(s);
        self
    }
    fn support(mut self, s: VoxelShape) -> Self {
        self.support = Some(s);
        self
    }
    fn interaction(mut self, s: VoxelShape) -> Self {
        self.interaction = Some(s);
        self
    }
    fn occlusion(mut self, s: VoxelShape) -> Self {
        self.occlusion = Some(s);
        self
    }
    fn offset(mut self) -> Self {
        self.offset = true;
        self
    }
}

type P = [f64; 6];

fn px(b: P) -> VoxelShape {
    VoxelShape::px(b[0], b[1], b[2], b[3], b[4], b[5])
}

fn union(boxes: &[P]) -> VoxelShape {
    VoxelShape::union_all(boxes.iter().map(|b| px(*b)))
}

fn nesw<T: Copy>(d: Direction, n: T, e: T, s: T, w: T) -> T {
    match d {
        Direction::East => e,
        Direction::South => s,
        Direction::West => w,
        _ => n,
    }
}

fn rotated(north: &VoxelShape, d: Direction) -> VoxelShape {
    north.rotate(Transform::horizontal(d))
}

/// Shape of a thin pane attached to the side opposite `d` (doors, trapdoors, ladders).
fn thin_side(d: Direction) -> VoxelShape {
    px(nesw(
        d,
        [0.0, 0.0, 13.0, 16.0, 16.0, 16.0],
        [0.0, 0.0, 0.0, 3.0, 16.0, 16.0],
        [0.0, 0.0, 0.0, 16.0, 16.0, 3.0],
        [13.0, 0.0, 0.0, 16.0, 16.0, 16.0],
    ))
}

fn quarter(q: &str, y1: f64, y2: f64) -> VoxelShape {
    match q {
        "NW" => px([0.0, y1, 0.0, 8.0, y2, 8.0]),
        "NE" => px([8.0, y1, 0.0, 16.0, y2, 8.0]),
        "SE" => px([8.0, y1, 8.0, 16.0, y2, 16.0]),
        _ => px([0.0, y1, 8.0, 8.0, y2, 16.0]),
    }
}

fn stairs(ctx: &Ctx) -> Result<VoxelShape, BuildError> {
    let facing = ctx.facing()?;
    let top = match ctx.str("half")? {
        "top" => true,
        "bottom" => false,
        _ => return Err(ctx.bad("half")),
    };
    let f = match facing {
        Direction::North => 0,
        Direction::East => 1,
        Direction::South => 2,
        Direction::West => 3,
        _ => return Err(ctx.bad("facing")),
    };
    let quarters: &[&str] = match ctx.str("shape")? {
        "straight" => [
            &["NW", "NE"][..],
            &["NE", "SE"],
            &["SE", "SW"],
            &["SW", "NW"],
        ][f],
        "outer_left" => [&["NW"][..], &["NE"], &["SE"], &["SW"]][f],
        "outer_right" => [&["NE"][..], &["SE"], &["SW"], &["NW"]][f],
        "inner_right" => [
            &["NW", "NE", "SE"][..],
            &["NE", "SE", "SW"],
            &["SE", "SW", "NW"],
            &["SW", "NW", "NE"],
        ][f],
        "inner_left" => [
            &["SW", "NW", "NE"][..],
            &["NW", "NE", "SE"],
            &["NE", "SE", "SW"],
            &["SE", "SW", "NW"],
        ][f],
        _ => return Err(ctx.bad("shape")),
    };
    let (slab, qy) = if top {
        (px([0.0, 8.0, 0.0, 16.0, 16.0, 16.0]), (0.0, 8.0))
    } else {
        (px([0.0, 0.0, 0.0, 16.0, 8.0, 16.0]), (8.0, 16.0))
    };
    let mut s = slab;
    for q in quarters {
        s = s.or(&quarter(q, qy.0, qy.1));
    }
    Ok(s)
}

fn fence_arms(ctx: &Ctx, post: P, arms: [P; 4]) -> Result<VoxelShape, BuildError> {
    let mut s = px(post);
    for (name, arm) in ["north", "east", "south", "west"].into_iter().zip(arms) {
        if ctx.bool(name)? {
            s = s.or(&px(arm));
        }
    }
    Ok(s)
}

fn wall_shape(ctx: &Ctx, y_low: f64, y_tall: f64, post_top: f64) -> Result<VoxelShape, BuildError> {
    let mut parts: Vec<VoxelShape> = Vec::new();
    if ctx.bool("up")? {
        parts.push(px([4.0, 0.0, 4.0, 12.0, post_top, 12.0]));
    }
    for name in ["north", "east", "south", "west"] {
        let h = match ctx.str(name)? {
            "none" => continue,
            "low" => y_low,
            "tall" => y_tall,
            _ => return Err(ctx.bad(name)),
        };
        let b = match name {
            "north" => [5.0, 0.0, 0.0, 11.0, h, 11.0],
            "east" => [5.0, 0.0, 5.0, 16.0, h, 11.0],
            "south" => [5.0, 0.0, 5.0, 11.0, h, 16.0],
            _ => [0.0, 0.0, 5.0, 11.0, h, 11.0],
        };
        parts.push(px(b));
    }
    Ok(VoxelShape::union_all(parts))
}

fn bell_body() -> VoxelShape {
    union(&[
        [5.0, 6.0, 5.0, 11.0, 13.0, 11.0],
        [4.0, 4.0, 4.0, 12.0, 6.0, 12.0],
    ])
}

fn segmented(ctx: &Ctx, amount_prop: &str, h: f64) -> Result<VoxelShape, BuildError> {
    let facing = ctx.facing()?;
    let count = ctx.int(amount_prop)?;
    // Quarters taken counter-clockwise starting at the facing's own quarter.
    let order = [
        Direction::North,
        Direction::West,
        Direction::South,
        Direction::East,
    ];
    let start = order
        .iter()
        .position(|d| *d == facing)
        .ok_or_else(|| ctx.bad("facing"))?;
    let q = |d: Direction| match d {
        Direction::North => [0.0, 0.0, 8.0, 8.0],
        Direction::West => [0.0, 8.0, 8.0, 16.0],
        Direction::South => [8.0, 8.0, 16.0, 16.0],
        _ => [8.0, 0.0, 16.0, 8.0],
    };
    let mut lo = [f64::MAX, f64::MAX];
    let mut hi = [f64::MIN, f64::MIN];
    for n in 0..usize::try_from(count.clamp(0, 4)).unwrap_or(0) {
        let r = q(order[(start + n) % 4]);
        lo[0] = lo[0].min(r[0]);
        lo[1] = lo[1].min(r[1]);
        hi[0] = hi[0].max(r[2]);
        hi[1] = hi[1].max(r[3]);
    }
    if count < 1 {
        return Ok(VoxelShape::empty());
    }
    Ok(px([lo[0], 0.0, lo[1], hi[0], h, hi[1]]))
}

fn skull_standing(variant: &str) -> FamilyShapes {
    match variant {
        "piglin" => FamilyShapes::new(px([3.0, 0.0, 3.0, 13.0, 8.0, 13.0])),
        "dragon" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 8.5, 12.0]))
            .collision(px([4.0, 0.0, 4.0, 12.0, 8.0, 12.0])),
        _ => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 8.0, 12.0])),
    }
}

fn lectern_north() -> VoxelShape {
    union(&[
        [0.0, 10.0, 1.0, 16.0, 14.0, 5.333_333],
        [0.0, 12.0, 5.333_333, 16.0, 16.0, 9.666_667],
        [0.0, 14.0, 9.666_667, 16.0, 18.0, 14.0],
        [0.0, 0.0, 0.0, 16.0, 2.0, 16.0],
        [4.0, 2.0, 4.0, 12.0, 14.0, 12.0],
    ])
}

fn scaffold_stable() -> VoxelShape {
    union(&[
        [0.0, 14.0, 0.0, 16.0, 16.0, 16.0],
        [0.0, 0.0, 0.0, 2.0, 16.0, 2.0],
        [14.0, 0.0, 0.0, 16.0, 16.0, 2.0],
        [14.0, 0.0, 14.0, 16.0, 16.0, 16.0],
        [0.0, 0.0, 14.0, 2.0, 16.0, 16.0],
    ])
}

fn hopper(ctx: &Ctx) -> Result<FamilyShapes, BuildError> {
    let bowl = union(&[
        [0.0, 10.0, 0.0, 16.0, 16.0, 16.0],
        [4.0, 4.0, 4.0, 12.0, 10.0, 12.0],
    ])
    .subtract(&px([2.0, 11.0, 2.0, 14.0, 16.0, 14.0]));
    let facing = ctx.facing()?;
    // The spout is modelled for NORTH about the pivot (8, 6, 8) px; the result is clipped to the block.
    let spout = match facing {
        Direction::Down => [6.0, -2.0, 6.0, 10.0, 6.0, 10.0],
        Direction::North => [6.0, 4.0, 0.0, 10.0, 8.0, 8.0],
        Direction::East => [8.0, 4.0, 6.0, 16.0, 8.0, 10.0],
        Direction::South => [6.0, 4.0, 8.0, 10.0, 8.0, 16.0],
        Direction::West => [0.0, 4.0, 6.0, 8.0, 8.0, 10.0],
        Direction::Up => return Err(ctx.bad("facing")),
    };
    let clipped = px(spout).and(&VoxelShape::block());
    let outline = bowl.or(&clipped);
    let inside = px([2.0, 11.0, 2.0, 14.0, 16.0, 14.0]);
    let interaction = if facing == Direction::Down {
        inside
    } else {
        inside.or(&px(nesw(
            facing,
            [6.0, 8.0, 0.0, 10.0, 10.0, 4.0],
            [12.0, 8.0, 6.0, 16.0, 10.0, 10.0],
            [6.0, 8.0, 12.0, 10.0, 10.0, 16.0],
            [0.0, 8.0, 6.0, 4.0, 10.0, 10.0],
        )))
    };
    Ok(FamilyShapes::new(outline).interaction(interaction))
}

fn cauldron() -> FamilyShapes {
    let cutout = union(&[
        [0.0, 0.0, 4.0, 16.0, 3.0, 12.0],
        [4.0, 0.0, 0.0, 12.0, 3.0, 16.0],
        [2.0, 0.0, 2.0, 14.0, 3.0, 14.0],
        [2.0, 4.0, 2.0, 14.0, 16.0, 14.0],
    ]);
    FamilyShapes::new(VoxelShape::block().subtract(&cutout))
        .interaction(px([2.0, 4.0, 2.0, 14.0, 16.0, 14.0]))
}

fn piston_head(ctx: &Ctx) -> Result<VoxelShape, BuildError> {
    let short = ctx.bool("short")?;
    let (platform, arm_short, arm_long): (P, P, P) = match ctx.facing()? {
        Direction::North => (
            [0.0, 0.0, 0.0, 16.0, 16.0, 4.0],
            [6.0, 6.0, 4.0, 10.0, 10.0, 16.0],
            [6.0, 6.0, 4.0, 10.0, 10.0, 20.0],
        ),
        Direction::East => (
            [12.0, 0.0, 0.0, 16.0, 16.0, 16.0],
            [0.0, 6.0, 6.0, 12.0, 10.0, 10.0],
            [-4.0, 6.0, 6.0, 12.0, 10.0, 10.0],
        ),
        Direction::South => (
            [0.0, 0.0, 12.0, 16.0, 16.0, 16.0],
            [6.0, 6.0, 0.0, 10.0, 10.0, 12.0],
            [6.0, 6.0, -4.0, 10.0, 10.0, 12.0],
        ),
        Direction::West => (
            [0.0, 0.0, 0.0, 4.0, 16.0, 16.0],
            [4.0, 6.0, 6.0, 16.0, 10.0, 10.0],
            [4.0, 6.0, 6.0, 20.0, 10.0, 10.0],
        ),
        Direction::Up => (
            [0.0, 12.0, 0.0, 16.0, 16.0, 16.0],
            [6.0, 0.0, 6.0, 10.0, 12.0, 10.0],
            [6.0, -4.0, 6.0, 10.0, 12.0, 10.0],
        ),
        Direction::Down => (
            [0.0, 0.0, 0.0, 16.0, 4.0, 16.0],
            [6.0, 4.0, 6.0, 10.0, 16.0, 10.0],
            [6.0, 4.0, 6.0, 10.0, 20.0, 10.0],
        ),
    };
    Ok(px(platform).or(&px(if short { arm_short } else { arm_long })))
}

fn button(ctx: &Ctx) -> Result<VoxelShape, BuildError> {
    let facing = ctx.facing()?;
    let pressed = ctx.bool("powered")?;
    let d = if pressed { 1.0 } else { 2.0 };
    let b: P = match ctx.str("face")? {
        "wall" => nesw(
            facing,
            [5.0, 6.0, 16.0 - d, 11.0, 10.0, 16.0],
            [0.0, 6.0, 5.0, d, 10.0, 11.0],
            [5.0, 6.0, 0.0, 11.0, 10.0, d],
            [16.0 - d, 6.0, 5.0, 16.0, 10.0, 11.0],
        ),
        "floor" => {
            if matches!(facing, Direction::North | Direction::South) {
                [5.0, 0.0, 6.0, 11.0, d, 10.0]
            } else {
                [6.0, 0.0, 5.0, 10.0, d, 11.0]
            }
        }
        "ceiling" => {
            if matches!(facing, Direction::North | Direction::South) {
                [5.0, 16.0 - d, 6.0, 11.0, 16.0, 10.0]
            } else {
                [6.0, 16.0 - d, 5.0, 10.0, 16.0, 11.0]
            }
        }
        _ => return Err(ctx.bad("face")),
    };
    Ok(px(b))
}

fn lever(ctx: &Ctx) -> Result<VoxelShape, BuildError> {
    let facing = ctx.facing()?;
    let ns = matches!(facing, Direction::North | Direction::South);
    let b: P = match ctx.str("face")? {
        "wall" => nesw(
            facing,
            [5.0, 4.0, 10.0, 11.0, 12.0, 16.0],
            [0.0, 4.0, 5.0, 6.0, 12.0, 11.0],
            [5.0, 4.0, 0.0, 11.0, 12.0, 6.0],
            [10.0, 4.0, 5.0, 16.0, 12.0, 11.0],
        ),
        "floor" => {
            if ns {
                [5.0, 0.0, 4.0, 11.0, 6.0, 12.0]
            } else {
                [4.0, 0.0, 5.0, 12.0, 6.0, 11.0]
            }
        }
        "ceiling" => {
            if ns {
                [5.0, 10.0, 4.0, 11.0, 16.0, 12.0]
            } else {
                [4.0, 10.0, 5.0, 12.0, 16.0, 11.0]
            }
        }
        _ => return Err(ctx.bad("face")),
    };
    Ok(px(b))
}

fn grindstone(ctx: &Ctx) -> Result<VoxelShape, BuildError> {
    let north = union(&[
        [4.0, 2.0, 0.0, 12.0, 14.0, 12.0],
        [2.0, 6.0, 7.0, 4.0, 10.0, 16.0],
        [2.0, 5.0, 3.0, 4.0, 11.0, 9.0],
        [12.0, 6.0, 7.0, 14.0, 10.0, 16.0],
        [12.0, 5.0, 3.0, 14.0, 11.0, 9.0],
    ]);
    let facing = ctx.facing()?;
    let t = match ctx.str("face")? {
        "wall" => Transform::horizontal(facing),
        "floor" => Transform::horizontal(facing).after(Transform::X270),
        "ceiling" => Transform::horizontal(facing).after(Transform::Y180.after(Transform::X90)),
        _ => return Err(ctx.bad("face")),
    };
    Ok(north.rotate(t))
}

fn amethyst(ctx: &Ctx, h: f64, w: f64) -> Result<VoxelShape, BuildError> {
    let (a, b) = (8.0 - w / 2.0, 8.0 + w / 2.0);
    Ok(px(match ctx.facing()? {
        Direction::Up => [a, 0.0, a, b, h, b],
        Direction::Down => [a, 16.0 - h, a, b, 16.0, b],
        Direction::North => [a, a, 16.0 - h, b, b, 16.0],
        Direction::South => [a, a, 0.0, b, b, h],
        Direction::East => [0.0, a, a, h, b, b],
        Direction::West => [16.0 - h, a, a, 16.0, b, b],
    }))
}

fn variant_nums(v: &str) -> (f64, f64) {
    let mut h = 0.0;
    let mut w = 0.0;
    for part in v.split(',') {
        if let Some(x) = part.strip_prefix("h=") {
            h = x.parse().unwrap_or(0.0);
        } else if let Some(x) = part.strip_prefix("w=") {
            w = x.parse().unwrap_or(0.0);
        }
    }
    (h, w)
}

fn multiface(ctx: &Ctx, with_down: bool) -> Result<VoxelShape, BuildError> {
    let mut parts: Vec<VoxelShape> = Vec::new();
    let sheets: [(&str, P); 6] = [
        ("down", [0.0, 0.0, 0.0, 16.0, 1.0, 16.0]),
        ("up", [0.0, 15.0, 0.0, 16.0, 16.0, 16.0]),
        ("north", [0.0, 0.0, 0.0, 16.0, 16.0, 1.0]),
        ("east", [15.0, 0.0, 0.0, 16.0, 16.0, 16.0]),
        ("south", [0.0, 0.0, 15.0, 16.0, 16.0, 16.0]),
        ("west", [0.0, 0.0, 0.0, 1.0, 16.0, 16.0]),
    ];
    for (name, b) in sheets {
        if name == "down" && !with_down {
            continue;
        }
        if ctx.bool(name)? {
            parts.push(px(b));
        }
    }
    if parts.is_empty() {
        return Ok(VoxelShape::block());
    }
    // Union order: DOWN, UP, N, E, S, W as listed in the catalogue.
    Ok(VoxelShape::union_all(parts))
}

/// Computes the family shapes of one state.
pub(crate) fn shapes(ctx: &Ctx) -> Result<FamilyShapes, BuildError> {
    let info = ctx.info;
    let fam = info.family.as_str();
    let var = info.variant.as_str();
    let full = VoxelShape::block;
    let empty = VoxelShape::empty;
    Ok(match fam {
        // ── A. cubes and invisible blocks ────────────────────────────────────────────────────────────────────────
        "cube" | "cube_plant" => FamilyShapes::new(full()),
        "cube_glass" => FamilyShapes::new(full()).visual(empty()),
        "leaves" => FamilyShapes::new(full()).support(empty()),
        "empty" => FamilyShapes::new(empty()).collision(empty()),
        "fluid" => FamilyShapes::new(empty())
            .collision(empty())
            .visual(empty())
            .support(empty()),
        "moving_piston" => FamilyShapes::new(empty())
            .collision(empty())
            .visual(empty())
            .support(empty()),
        "light" => FamilyShapes::new(empty()).collision(empty()),
        "structure_void" => FamilyShapes::new(VoxelShape::cube(6.0)),
        "shulker_box" => FamilyShapes::new(full()).collision(full()).support(full()),

        // ── B. building families ─────────────────────────────────────────────────────────────────────────────────
        "slab" => FamilyShapes::new(match ctx.str("type")? {
            "bottom" => px([0.0, 0.0, 0.0, 16.0, 8.0, 16.0]),
            "top" => px([0.0, 8.0, 0.0, 16.0, 16.0, 16.0]),
            "double" => full(),
            _ => return Err(ctx.bad("type")),
        }),
        "stairs" => FamilyShapes::new(stairs(ctx)?),
        "fence" => {
            let outline = fence_arms(
                ctx,
                [6.0, 0.0, 6.0, 10.0, 16.0, 10.0],
                [
                    [6.0, 0.0, 0.0, 10.0, 16.0, 8.0],
                    [8.0, 0.0, 6.0, 16.0, 16.0, 10.0],
                    [6.0, 0.0, 8.0, 10.0, 16.0, 16.0],
                    [0.0, 0.0, 6.0, 8.0, 16.0, 10.0],
                ],
            )?;
            let collision = fence_arms(
                ctx,
                [6.0, 0.0, 6.0, 10.0, 24.0, 10.0],
                [
                    [6.0, 0.0, 0.0, 10.0, 24.0, 8.0],
                    [8.0, 0.0, 6.0, 16.0, 24.0, 10.0],
                    [6.0, 0.0, 8.0, 10.0, 24.0, 16.0],
                    [0.0, 0.0, 6.0, 8.0, 24.0, 10.0],
                ],
            )?;
            let occlusion = fence_arms(
                ctx,
                [6.0, 0.0, 6.0, 10.0, 16.0, 10.0],
                [
                    [7.0, 6.0, 0.0, 9.0, 15.0, 8.0],
                    [8.0, 6.0, 7.0, 16.0, 15.0, 9.0],
                    [7.0, 6.0, 8.0, 9.0, 15.0, 16.0],
                    [0.0, 6.0, 7.0, 8.0, 15.0, 9.0],
                ],
            )?;
            FamilyShapes::new(outline.clone())
                .collision(collision)
                .visual(outline)
                .occlusion(occlusion)
        }
        "pane" => {
            let s = fence_arms(
                ctx,
                [7.0, 0.0, 7.0, 9.0, 16.0, 9.0],
                [
                    [7.0, 0.0, 0.0, 9.0, 16.0, 8.0],
                    [8.0, 0.0, 7.0, 16.0, 16.0, 9.0],
                    [7.0, 0.0, 8.0, 9.0, 16.0, 16.0],
                    [0.0, 0.0, 7.0, 8.0, 16.0, 9.0],
                ],
            )?;
            FamilyShapes::new(s).visual(empty())
        }
        "wall" => {
            let outline = wall_shape(ctx, 14.0, 16.0, 16.0)?;
            let collision = wall_shape(ctx, 24.0, 24.0, 24.0)?;
            FamilyShapes::new(outline).collision(collision)
        }
        "fence_gate" => {
            let facing = ctx.facing()?;
            let z_axis = matches!(facing, Direction::North | Direction::South);
            let in_wall = ctx.bool("in_wall")?;
            let open = ctx.bool("open")?;
            let top = if in_wall { 13.0 } else { 16.0 };
            let outline = px(if z_axis {
                [0.0, 0.0, 6.0, 16.0, top, 10.0]
            } else {
                [6.0, 0.0, 0.0, 10.0, top, 16.0]
            });
            let (collision, support) = if open {
                (empty(), empty())
            } else if z_axis {
                (
                    px([0.0, 0.0, 6.0, 16.0, 24.0, 10.0]),
                    px([0.0, 5.0, 6.0, 16.0, 24.0, 10.0]),
                )
            } else {
                (
                    px([6.0, 0.0, 0.0, 10.0, 24.0, 16.0]),
                    px([6.0, 5.0, 0.0, 10.0, 24.0, 16.0]),
                )
            };
            let (y1, y2) = if in_wall { (2.0, 13.0) } else { (5.0, 16.0) };
            let occlusion = if z_axis {
                union(&[[0.0, y1, 7.0, 2.0, y2, 9.0], [14.0, y1, 7.0, 16.0, y2, 9.0]])
            } else {
                union(&[[7.0, y1, 0.0, 9.0, y2, 2.0], [7.0, y1, 14.0, 9.0, y2, 16.0]])
            };
            FamilyShapes::new(outline)
                .collision(collision)
                .support(support)
                .occlusion(occlusion)
        }
        "door" => {
            let facing = ctx.facing()?;
            let d = if ctx.bool("open")? {
                match ctx.str("hinge")? {
                    "right" => facing.counter_clockwise(),
                    _ => facing.clockwise(),
                }
            } else {
                facing
            };
            FamilyShapes::new(thin_side(d))
        }
        "trapdoor" => {
            if ctx.bool("open")? {
                FamilyShapes::new(thin_side(ctx.facing()?))
            } else if ctx.str("half")? == "top" {
                FamilyShapes::new(px([0.0, 13.0, 0.0, 16.0, 16.0, 16.0]))
            } else {
                FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 3.0, 16.0]))
            }
        }
        "ladder" => FamilyShapes::new(thin_side(ctx.facing()?)),
        "chain" => FamilyShapes::new(px(match ctx.str("axis")? {
            "y" => [6.5, 0.0, 6.5, 9.5, 16.0, 9.5],
            "x" => [0.0, 6.5, 6.5, 16.0, 9.5, 9.5],
            _ => [6.5, 6.5, 0.0, 9.5, 9.5, 16.0],
        })),
        "rod" => FamilyShapes::new(px(match ctx.facing()?.axis() {
            1 => [6.0, 0.0, 6.0, 10.0, 16.0, 10.0],
            0 => [0.0, 6.0, 6.0, 16.0, 10.0, 10.0],
            _ => [6.0, 6.0, 0.0, 10.0, 10.0, 16.0],
        })),
        "carpet" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 1.0, 16.0])),
        "pale_moss_carpet" => {
            let mut parts: Vec<VoxelShape> = Vec::new();
            if ctx.bool("bottom")? {
                parts.push(px([0.0, 0.0, 0.0, 16.0, 1.0, 16.0]));
            }
            for (name, sheet) in [
                ("north", [0.0, 0.0, 0.0, 16.0, 0.0, 1.0]),
                ("east", [15.0, 0.0, 0.0, 16.0, 0.0, 16.0]),
                ("south", [0.0, 0.0, 15.0, 16.0, 0.0, 16.0]),
                ("west", [0.0, 0.0, 0.0, 1.0, 0.0, 16.0]),
            ] {
                let h = match ctx.str(name)? {
                    "none" => continue,
                    "low" => 10.0,
                    "tall" => 16.0,
                    _ => return Err(ctx.bad(name)),
                };
                let mut b = sheet;
                b[4] = h;
                parts.push(px(b));
            }
            let outline = {
                let u = VoxelShape::union_all(parts);
                if u.is_empty() { full() } else { u }
            };
            let floor = px([0.0, 0.0, 0.0, 16.0, 1.0, 16.0]);
            let collision = if ctx.bool("bottom")? { floor } else { empty() };
            FamilyShapes::new(outline).collision(collision)
        }
        "snow_layer" => {
            let l = f64::from(ctx.int("layers")?);
            let outline = px([0.0, 0.0, 0.0, 16.0, 2.0 * l, 16.0]);
            let collision = if l <= 1.0 {
                empty()
            } else {
                px([0.0, 0.0, 0.0, 16.0, 2.0 * (l - 1.0), 16.0])
            };
            FamilyShapes::new(outline.clone())
                .collision(collision)
                .visual(outline.clone())
                .support(outline)
        }

        // ── C. furniture, containers, signs ──────────────────────────────────────────────────────────────────────
        "bed" => {
            let facing = ctx.facing()?;
            let d = if ctx.str("part")? == "head" {
                facing
            } else {
                facing.opposite()
            };
            let legs: [P; 2] = nesw(
                d,
                [
                    [0.0, 0.0, 0.0, 3.0, 3.0, 3.0],
                    [13.0, 0.0, 0.0, 16.0, 3.0, 3.0],
                ],
                [
                    [13.0, 0.0, 0.0, 16.0, 3.0, 3.0],
                    [13.0, 0.0, 13.0, 16.0, 3.0, 16.0],
                ],
                [
                    [13.0, 0.0, 13.0, 16.0, 3.0, 16.0],
                    [0.0, 0.0, 13.0, 3.0, 3.0, 16.0],
                ],
                [
                    [0.0, 0.0, 0.0, 3.0, 3.0, 3.0],
                    [0.0, 0.0, 13.0, 3.0, 3.0, 16.0],
                ],
            );
            let mut s = px([0.0, 3.0, 0.0, 16.0, 9.0, 16.0]);
            for l in legs {
                s = s.or(&px(l));
            }
            FamilyShapes::new(s)
        }
        "straw_bed" => {
            let mut s = px([0.0, 0.0, 0.0, 16.0, 4.0, 16.0]);
            if ctx.str("part")? == "head" {
                s = s.or(&px(nesw(
                    ctx.facing()?,
                    [0.0, 0.0, 0.0, 16.0, 5.0, 8.0],
                    [8.0, 0.0, 0.0, 16.0, 5.0, 16.0],
                    [0.0, 0.0, 8.0, 16.0, 5.0, 16.0],
                    [0.0, 0.0, 0.0, 8.0, 5.0, 16.0],
                )));
            }
            FamilyShapes::new(s)
        }
        "chest" => {
            let facing = ctx.facing()?;
            let c = match ctx.str("type")? {
                "single" => None,
                "left" => Some(facing.clockwise()),
                "right" => Some(facing.counter_clockwise()),
                _ => return Err(ctx.bad("type")),
            };
            FamilyShapes::new(px(match c {
                None => [1.0, 0.0, 1.0, 15.0, 14.0, 15.0],
                Some(c) => nesw(
                    c,
                    [1.0, 0.0, 0.0, 15.0, 14.0, 15.0],
                    [1.0, 0.0, 1.0, 16.0, 14.0, 15.0],
                    [1.0, 0.0, 1.0, 15.0, 14.0, 16.0],
                    [0.0, 0.0, 1.0, 15.0, 14.0, 15.0],
                ),
            }))
        }
        "ender_chest" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 14.0, 15.0])),
        "sign" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 16.0, 12.0])),
        "wall_sign" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [0.0, 4.5, 14.0, 16.0, 12.5, 16.0],
            [0.0, 4.5, 0.0, 2.0, 12.5, 16.0],
            [0.0, 4.5, 0.0, 16.0, 12.5, 2.0],
            [14.0, 4.5, 0.0, 16.0, 12.5, 16.0],
        ))),
        "ceiling_hanging_sign" => {
            let s = px(match ctx.int("rotation")? {
                0 | 8 => [1.0, 0.0, 7.0, 15.0, 10.0, 9.0],
                4 | 12 => [7.0, 0.0, 1.0, 9.0, 10.0, 15.0],
                _ => [3.0, 0.0, 3.0, 13.0, 16.0, 13.0],
            });
            FamilyShapes::new(s.clone()).support(s)
        }
        "wall_hanging_sign" => {
            let z_axis = matches!(ctx.facing()?, Direction::North | Direction::South);
            let (plank, board): (P, P) = if z_axis {
                (
                    [0.0, 14.0, 6.0, 16.0, 16.0, 10.0],
                    [1.0, 0.0, 7.0, 15.0, 10.0, 9.0],
                )
            } else {
                (
                    [6.0, 14.0, 0.0, 10.0, 16.0, 16.0],
                    [7.0, 0.0, 1.0, 9.0, 10.0, 15.0],
                )
            };
            let outline = union(&[plank, board]);
            FamilyShapes::new(outline.clone())
                .collision(px(plank))
                .support(outline)
        }
        "banner" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 16.0, 12.0])),
        "wall_banner" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [0.0, 0.0, 14.0, 16.0, 12.5, 16.0],
            [0.0, 0.0, 0.0, 2.0, 12.5, 16.0],
            [0.0, 0.0, 0.0, 16.0, 12.5, 2.0],
            [14.0, 0.0, 0.0, 16.0, 12.5, 16.0],
        ))),
        "shelf" => {
            let north = union(&[
                [0.0, 12.0, 11.0, 16.0, 16.0, 13.0],
                [0.0, 0.0, 13.0, 16.0, 16.0, 16.0],
                [0.0, 0.0, 11.0, 16.0, 4.0, 13.0],
            ]);
            FamilyShapes::new(rotated(&north, ctx.facing()?))
        }
        "lectern" => {
            let base_post = union(&[
                [0.0, 0.0, 0.0, 16.0, 2.0, 16.0],
                [4.0, 2.0, 4.0, 12.0, 14.0, 12.0],
            ]);
            FamilyShapes::new(rotated(&lectern_north(), ctx.facing()?))
                .collision(base_post.clone())
                .visual(base_post.clone())
                .occlusion(base_post)
        }
        "bell" => {
            let facing = ctx.facing()?;
            let z_axis = matches!(facing, Direction::North | Direction::South);
            let s = match ctx.str("attachment")? {
                "floor" => px(if z_axis {
                    [0.0, 0.0, 4.0, 16.0, 16.0, 12.0]
                } else {
                    [4.0, 0.0, 0.0, 12.0, 16.0, 16.0]
                }),
                "ceiling" => bell_body().or(&px([7.0, 13.0, 7.0, 9.0, 16.0, 9.0])),
                "single_wall" => bell_body().or(&px(nesw(
                    facing,
                    [7.0, 13.0, 0.0, 9.0, 15.0, 13.0],
                    [3.0, 13.0, 7.0, 16.0, 15.0, 9.0],
                    [7.0, 13.0, 3.0, 9.0, 15.0, 16.0],
                    [0.0, 13.0, 7.0, 13.0, 15.0, 9.0],
                ))),
                "double_wall" => bell_body().or(&px(if z_axis {
                    [7.0, 13.0, 0.0, 9.0, 15.0, 16.0]
                } else {
                    [0.0, 13.0, 7.0, 16.0, 15.0, 9.0]
                })),
                _ => return Err(ctx.bad("attachment")),
            };
            FamilyShapes::new(s)
        }
        "cauldron" => cauldron(),
        "composter" => {
            let level = usize::try_from(ctx.int("level")?.clamp(0, 8)).unwrap_or(0);
            let h = [2.0, 3.0, 5.0, 7.0, 9.0, 11.0, 13.0, 15.0, 15.0][level];
            let shape = |h: f64| full().subtract(&px([2.0, h, 2.0, 14.0, 16.0, 14.0]));
            FamilyShapes::new(shape(h))
                .collision(shape(2.0))
                .interaction(full())
        }
        "hopper" => hopper(ctx)?,
        "decorated_pot" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 16.0, 15.0])),
        "heavy_core" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 8.0, 12.0])),
        "copper_golem_statue" => FamilyShapes::new(px([3.0, 0.0, 3.0, 13.0, 14.0, 13.0])),
        "conduit" => FamilyShapes::new(px([5.0, 5.0, 5.0, 11.0, 11.0, 11.0])),
        "skull" => skull_standing(var),
        "wall_skull" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [4.0, 4.0, 8.0, 12.0, 12.0, 16.0],
            [0.0, 4.0, 4.0, 8.0, 12.0, 12.0],
            [4.0, 4.0, 0.0, 12.0, 12.0, 8.0],
            [8.0, 4.0, 4.0, 16.0, 12.0, 12.0],
        ))),
        "piglin_wall_head" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [3.0, 4.0, 8.0, 13.0, 12.0, 16.0],
            [0.0, 4.0, 3.0, 8.0, 12.0, 13.0],
            [3.0, 4.0, 0.0, 13.0, 12.0, 8.0],
            [8.0, 4.0, 3.0, 16.0, 12.0, 13.0],
        ))),
        "anvil" => {
            let z_axis = matches!(ctx.facing()?, Direction::North | Direction::South);
            FamilyShapes::new(if z_axis {
                union(&[
                    [2.0, 0.0, 2.0, 14.0, 4.0, 14.0],
                    [4.0, 4.0, 3.0, 12.0, 5.0, 13.0],
                    [6.0, 5.0, 4.0, 10.0, 10.0, 12.0],
                    [3.0, 10.0, 0.0, 13.0, 16.0, 16.0],
                ])
            } else {
                union(&[
                    [2.0, 0.0, 2.0, 14.0, 4.0, 14.0],
                    [3.0, 4.0, 4.0, 13.0, 5.0, 12.0],
                    [4.0, 5.0, 6.0, 12.0, 10.0, 10.0],
                    [0.0, 10.0, 3.0, 16.0, 16.0, 13.0],
                ])
            })
        }
        "grindstone" => FamilyShapes::new(grindstone(ctx)?),
        "stonecutter" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 9.0, 16.0])),
        "enchanting_table" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 12.0, 16.0])),
        "campfire" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 7.0, 16.0])),
        "daylight_detector" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 6.0, 16.0])),
        "sculk_sensor" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 8.0, 16.0])),
        "diode" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 2.0, 16.0])),
        "sculk_shrieker" => {
            let s = px([0.0, 0.0, 0.0, 16.0, 8.0, 16.0]);
            FamilyShapes::new(full())
                .collision(s.clone())
                .visual(s.clone())
                .support(s.clone())
                .occlusion(s)
        }
        "end_portal_frame" => {
            let base = px([0.0, 0.0, 0.0, 16.0, 13.0, 16.0]);
            FamilyShapes::new(if ctx.bool("eye")? {
                base.or(&px([4.0, 13.0, 4.0, 12.0, 16.0, 12.0]))
            } else {
                base
            })
        }
        "brewing_stand" => FamilyShapes::new(union(&[
            [1.0, 0.0, 1.0, 15.0, 2.0, 15.0],
            [7.0, 2.0, 7.0, 9.0, 14.0, 9.0],
        ])),
        "cake" => {
            let b = f64::from(ctx.int("bites")?);
            FamilyShapes::new(px([1.0 + 2.0 * b, 0.0, 1.0, 15.0, 8.0, 15.0]))
        }
        "candle_cake" => FamilyShapes::new(union(&[
            [1.0, 0.0, 1.0, 15.0, 8.0, 15.0],
            [7.0, 8.0, 7.0, 9.0, 14.0, 9.0],
        ])),
        "candle" => FamilyShapes::new(px(match ctx.int("candles")? {
            1 => [7.0, 0.0, 7.0, 9.0, 6.0, 9.0],
            2 => [5.0, 0.0, 6.0, 11.0, 6.0, 9.0],
            3 => [5.0, 0.0, 6.0, 10.0, 6.0, 11.0],
            _ => [5.0, 0.0, 5.0, 11.0, 6.0, 10.0],
        })),
        "lantern" => {
            let y = if ctx.bool("hanging")? { 1.0 } else { 0.0 };
            FamilyShapes::new(union(&[
                [5.0, y, 5.0, 11.0, 7.0 + y, 11.0],
                [6.0, 7.0 + y, 6.0, 10.0, 9.0 + y, 10.0],
            ]))
        }
        "dragon_egg" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 16.0, 15.0])),
        "scaffolding" => {
            let stable = scaffold_stable();
            let unstable = stable.or(&px([0.0, 0.0, 0.0, 16.0, 2.0, 16.0]));
            let outline = if ctx.bool("bottom")? {
                unstable
            } else {
                stable.clone()
            };
            FamilyShapes::new(outline)
                .collision(stable.clone())
                .visual(stable.clone())
                .support(stable)
                .interaction(full())
        }
        "piston" => {
            if ctx.bool("extended")? {
                FamilyShapes::new(px(match ctx.facing()? {
                    Direction::North => [0.0, 0.0, 4.0, 16.0, 16.0, 16.0],
                    Direction::East => [0.0, 0.0, 0.0, 12.0, 16.0, 16.0],
                    Direction::South => [0.0, 0.0, 0.0, 16.0, 16.0, 12.0],
                    Direction::West => [4.0, 0.0, 0.0, 16.0, 16.0, 16.0],
                    Direction::Up => [0.0, 0.0, 0.0, 16.0, 12.0, 16.0],
                    Direction::Down => [0.0, 4.0, 0.0, 16.0, 16.0, 16.0],
                }))
            } else {
                FamilyShapes::new(full())
            }
        }
        "piston_head" => FamilyShapes::new(piston_head(ctx)?),

        // ── D. thin redstone and floor pieces ────────────────────────────────────────────────────────────────────
        "button" => FamilyShapes::new(button(ctx)?),
        "lever" => FamilyShapes::new(lever(ctx)?),
        "pressure_plate" => {
            let signal = if ctx.has("power") {
                ctx.int("power")? > 0
            } else {
                ctx.bool("powered")?
            };
            FamilyShapes::new(px([
                1.0,
                0.0,
                1.0,
                15.0,
                if signal { 0.5 } else { 1.0 },
                15.0,
            ]))
        }
        "redstone_wire" => {
            let mut parts = vec![px([3.0, 0.0, 3.0, 13.0, 1.0, 13.0])];
            let sides: [(&str, P, P); 4] = [
                (
                    "north",
                    [3.0, 0.0, 0.0, 13.0, 1.0, 8.0],
                    [3.0, 0.0, 0.0, 13.0, 16.0, 1.0],
                ),
                (
                    "east",
                    [8.0, 0.0, 3.0, 16.0, 1.0, 13.0],
                    [15.0, 0.0, 3.0, 16.0, 16.0, 13.0],
                ),
                (
                    "south",
                    [3.0, 0.0, 8.0, 13.0, 1.0, 16.0],
                    [3.0, 0.0, 15.0, 13.0, 16.0, 16.0],
                ),
                (
                    "west",
                    [0.0, 0.0, 3.0, 8.0, 1.0, 13.0],
                    [0.0, 0.0, 3.0, 1.0, 16.0, 13.0],
                ),
            ];
            for (name, floor, wall) in sides {
                match ctx.str(name)? {
                    "none" => {}
                    "side" => parts.push(px(floor)),
                    "up" => {
                        parts.push(px(floor));
                        parts.push(px(wall));
                    }
                    _ => return Err(ctx.bad(name)),
                }
            }
            FamilyShapes::new(VoxelShape::union_all(parts))
        }
        "rail" => {
            let ascending = ctx.str("shape")?.starts_with("ascending");
            FamilyShapes::new(px([
                0.0,
                0.0,
                0.0,
                16.0,
                if ascending { 8.0 } else { 2.0 },
                16.0,
            ]))
        }
        "tripwire" => FamilyShapes::new(px(if ctx.bool("attached")? {
            [0.0, 1.0, 0.0, 16.0, 2.5, 16.0]
        } else {
            [0.0, 0.0, 0.0, 16.0, 8.0, 16.0]
        })),
        "tripwire_hook" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [5.0, 0.0, 10.0, 11.0, 10.0, 16.0],
            [0.0, 0.0, 5.0, 6.0, 10.0, 11.0],
            [5.0, 0.0, 0.0, 11.0, 10.0, 6.0],
            [10.0, 0.0, 5.0, 16.0, 10.0, 11.0],
        ))),
        "torch" => FamilyShapes::new(px([6.0, 0.0, 6.0, 10.0, 10.0, 10.0])),
        "wall_torch" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [5.5, 3.0, 11.0, 10.5, 13.0, 16.0],
            [0.0, 3.0, 5.5, 5.0, 13.0, 10.5],
            [5.5, 3.0, 0.0, 10.5, 13.0, 5.0],
            [11.0, 3.0, 5.5, 16.0, 13.0, 10.5],
        ))),
        "fire" => {
            let mut parts: Vec<VoxelShape> = Vec::new();
            for (name, b) in [
                ("north", [0.0, 0.0, 0.0, 16.0, 16.0, 1.0]),
                ("east", [15.0, 0.0, 0.0, 16.0, 16.0, 16.0]),
                ("south", [0.0, 0.0, 15.0, 16.0, 16.0, 16.0]),
                ("west", [0.0, 0.0, 0.0, 1.0, 16.0, 16.0]),
                ("up", [0.0, 15.0, 0.0, 16.0, 16.0, 16.0]),
            ] {
                if ctx.bool(name)? {
                    parts.push(px(b));
                }
            }
            let u = VoxelShape::union_all(parts);
            FamilyShapes::new(if u.is_empty() {
                px([0.0, 0.0, 0.0, 16.0, 1.0, 16.0])
            } else {
                u
            })
        }
        "soul_fire" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 1.0, 16.0])),
        "nether_portal" => FamilyShapes::new(px(if ctx.str("axis")? == "x" {
            [0.0, 0.0, 6.0, 16.0, 16.0, 10.0]
        } else {
            [6.0, 0.0, 0.0, 10.0, 16.0, 16.0]
        })),
        "end_portal" => FamilyShapes::new(px([0.0, 6.0, 0.0, 16.0, 12.0, 16.0])),
        "farmland" | "dirt_path" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 15.0, 16.0])),
        "sinking_block" => FamilyShapes::new(full())
            .collision(px([0.0, 0.0, 0.0, 16.0, 14.0, 16.0]))
            .visual(full())
            .support(full()),
        "honey_block" => FamilyShapes::new(full()).collision(px([1.0, 0.0, 1.0, 15.0, 15.0, 15.0])),
        "powder_snow" => FamilyShapes::new(full())
            .collision(empty())
            .visual(empty())
            .support(empty()),
        "flower_pot" => FamilyShapes::new(px([5.0, 0.0, 5.0, 11.0, 6.0, 11.0])),

        // ── E. plants ────────────────────────────────────────────────────────────────────────────────────────────
        "sapling" | "seagrass" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 12.0, 14.0])),
        "short_plant" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 13.0, 14.0])),
        "short_dry_grass" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 10.0, 14.0])),
        "tall_dry_grass" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 16.0, 15.0])),
        "bush" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 13.0, 16.0])),
        "tall_seagrass" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 16.0, 14.0])),
        "mangrove_propagule" => {
            let a = if ctx.bool("hanging")? {
                ctx.int("age")?
            } else {
                4
            };
            let m = [13.0, 10.0, 7.0, 3.0, 0.0][usize::try_from(a.clamp(0, 4)).unwrap_or(0)];
            FamilyShapes::new(px([7.0, m, 7.0, 9.0, 16.0, 9.0])).offset()
        }
        "flower" => FamilyShapes::new(px([5.0, 0.0, 5.0, 11.0, 10.0, 11.0])).offset(),
        "mushroom" => FamilyShapes::new(px([5.0, 0.0, 5.0, 11.0, 6.0, 11.0])),
        "fungus" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 9.0, 12.0])),
        "nether_sprouts" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 3.0, 14.0])),
        "cactus_flower" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 12.0, 15.0])),
        "crop" => FamilyShapes::new(px([
            0.0,
            0.0,
            0.0,
            16.0,
            2.0 + 2.0 * f64::from(ctx.int("age")?),
            16.0,
        ])),
        "crop_small" => FamilyShapes::new(px([
            0.0,
            0.0,
            0.0,
            16.0,
            2.0 + f64::from(ctx.int("age")?),
            16.0,
        ])),
        "beetroot" => FamilyShapes::new(px([
            0.0,
            0.0,
            0.0,
            16.0,
            2.0 + 2.0 * f64::from(ctx.int("age")?),
            16.0,
        ])),
        "torchflower_crop" => FamilyShapes::new(px([
            5.0,
            0.0,
            5.0,
            11.0,
            6.0 + 4.0 * f64::from(ctx.int("age")?),
            11.0,
        ])),
        "pitcher_crop" => {
            let age = ctx.int("age")?;
            let lower = ctx.str("half")? == "lower";
            let (outline, collision) = if lower {
                if age == 0 {
                    let s = px([5.0, -1.0, 5.0, 11.0, 3.0, 11.0]);
                    (s.clone(), s)
                } else {
                    let o = if age == 1 {
                        px([3.0, -1.0, 3.0, 13.0, 14.0, 13.0])
                    } else {
                        px([3.0, -1.0, 3.0, 13.0, 16.0, 13.0])
                    };
                    (o, px([3.0, -1.0, 3.0, 13.0, 5.0, 13.0]))
                }
            } else {
                let o = match age {
                    3 => px([3.0, 0.0, 3.0, 13.0, 11.0, 13.0]),
                    4 => px([3.0, 0.0, 3.0, 13.0, 15.0, 13.0]),
                    _ => empty(),
                };
                (o, empty())
            };
            FamilyShapes::new(outline).collision(collision)
        }
        "stem" => FamilyShapes::new(px([
            7.0,
            0.0,
            7.0,
            9.0,
            2.0 + 2.0 * f64::from(ctx.int("age")?),
            9.0,
        ])),
        "attached_stem" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [6.0, 0.0, 0.0, 10.0, 10.0, 10.0],
            [6.0, 0.0, 6.0, 16.0, 10.0, 10.0],
            [6.0, 0.0, 6.0, 10.0, 10.0, 16.0],
            [0.0, 0.0, 6.0, 10.0, 10.0, 10.0],
        ))),
        "cocoa" => {
            let facing = ctx.facing()?;
            let age = ctx.int("age")?;
            let rows: [[P; 4]; 3] = [
                [
                    [6.0, 7.0, 1.0, 10.0, 12.0, 5.0],
                    [11.0, 7.0, 6.0, 15.0, 12.0, 10.0],
                    [6.0, 7.0, 11.0, 10.0, 12.0, 15.0],
                    [1.0, 7.0, 6.0, 5.0, 12.0, 10.0],
                ],
                [
                    [5.0, 5.0, 1.0, 11.0, 12.0, 7.0],
                    [9.0, 5.0, 5.0, 15.0, 12.0, 11.0],
                    [5.0, 5.0, 9.0, 11.0, 12.0, 15.0],
                    [1.0, 5.0, 5.0, 7.0, 12.0, 11.0],
                ],
                [
                    [4.0, 3.0, 1.0, 12.0, 12.0, 9.0],
                    [7.0, 3.0, 4.0, 15.0, 12.0, 12.0],
                    [4.0, 3.0, 7.0, 12.0, 12.0, 15.0],
                    [1.0, 3.0, 4.0, 9.0, 12.0, 12.0],
                ],
            ];
            let row = rows[usize::try_from(age.clamp(0, 2)).unwrap_or(0)];
            FamilyShapes::new(px(nesw(facing, row[0], row[1], row[2], row[3])))
        }
        "nether_wart" => FamilyShapes::new(px([
            0.0,
            0.0,
            0.0,
            16.0,
            5.0 + 3.0 * f64::from(ctx.int("age")?),
            16.0,
        ])),
        "sweet_berry_bush" => FamilyShapes::new(match ctx.int("age")? {
            0 => px([3.0, 0.0, 3.0, 13.0, 8.0, 13.0]),
            3 => full(),
            _ => px([1.0, 0.0, 1.0, 15.0, 16.0, 15.0]),
        }),
        "sugar_cane" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 16.0, 14.0])),
        "cactus" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 16.0, 15.0]))
            .collision(px([1.0, 0.0, 1.0, 15.0, 15.0, 15.0])),
        "lily_pad" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 1.5, 15.0])),
        "kelp" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 9.0, 16.0])),
        "kelp_plant" => FamilyShapes::new(full()),
        "weeping_vines" => FamilyShapes::new(px([4.0, 9.0, 4.0, 12.0, 16.0, 12.0])),
        "weeping_vines_plant" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 16.0, 15.0])),
        "twisting_vines" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 15.0, 12.0])),
        "twisting_vines_plant" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 16.0, 12.0])),
        "cave_vines" => FamilyShapes::new(px([1.0, 0.0, 1.0, 15.0, 16.0, 15.0])),
        "hanging_roots" => FamilyShapes::new(px([2.0, 10.0, 2.0, 14.0, 16.0, 14.0])),
        "spore_blossom" => FamilyShapes::new(px([2.0, 13.0, 2.0, 14.0, 16.0, 14.0])),
        "hanging_moss" => FamilyShapes::new(px(if ctx.bool("tip")? {
            [1.0, 2.0, 1.0, 15.0, 16.0, 15.0]
        } else {
            [1.0, 0.0, 1.0, 15.0, 16.0, 15.0]
        })),
        "vine" => FamilyShapes::new(multiface(ctx, false)?),
        "multiface" => FamilyShapes::new(multiface(ctx, true)?),
        "azalea" => FamilyShapes::new(union(&[
            [0.0, 8.0, 0.0, 16.0, 16.0, 16.0],
            [6.0, 0.0, 6.0, 10.0, 8.0, 10.0],
        ])),
        "segmented" => {
            let h = if var == "h=1" { 1.0 } else { 3.0 };
            let prop = if ctx.has("flower_amount") {
                "flower_amount"
            } else {
                "segment_amount"
            };
            FamilyShapes::new(segmented(ctx, prop, h)?)
        }
        "big_dripleaf" => {
            let facing = ctx.facing()?;
            let leaf = match ctx.str("tilt")? {
                "none" | "unstable" => px([0.0, 11.0, 0.0, 16.0, 15.0, 16.0]),
                "partial" => px([0.0, 11.0, 0.0, 16.0, 13.0, 16.0]),
                _ => empty(),
            };
            let stem = px(nesw(
                facing,
                [5.0, 0.0, 9.0, 11.0, 13.0, 15.0],
                [1.0, 0.0, 5.0, 7.0, 13.0, 11.0],
                [5.0, 0.0, 1.0, 11.0, 13.0, 7.0],
                [9.0, 0.0, 5.0, 15.0, 13.0, 11.0],
            ));
            FamilyShapes::new(leaf.or(&stem)).collision(leaf)
        }
        "big_dripleaf_stem" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [5.0, 0.0, 9.0, 11.0, 16.0, 15.0],
            [1.0, 0.0, 5.0, 7.0, 16.0, 11.0],
            [5.0, 0.0, 1.0, 11.0, 16.0, 7.0],
            [9.0, 0.0, 5.0, 15.0, 16.0, 11.0],
        ))),
        "small_dripleaf" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 13.0, 14.0])),
        "bamboo_sapling" => FamilyShapes::new(px([4.0, 0.0, 4.0, 12.0, 12.0, 12.0])).offset(),
        "bamboo" => {
            let outline = if ctx.str("leaves")? == "large" {
                px([3.0, 0.0, 3.0, 13.0, 16.0, 13.0])
            } else {
                px([5.0, 0.0, 5.0, 11.0, 16.0, 11.0])
            };
            FamilyShapes::new(outline)
                .collision(px([6.5, 0.0, 6.5, 9.5, 16.0, 9.5]))
                .offset()
        }
        "speleothem" => {
            let s = match ctx.str("thickness")? {
                "tip_merge" => [5.0, 0.0, 5.0, 11.0, 16.0, 11.0],
                "tip" => {
                    if ctx.str("vertical_direction")? == "down" {
                        [5.0, 5.0, 5.0, 11.0, 16.0, 11.0]
                    } else {
                        [5.0, 0.0, 5.0, 11.0, 11.0, 11.0]
                    }
                }
                "frustum" => [4.0, 0.0, 4.0, 12.0, 16.0, 12.0],
                "middle" => [3.0, 0.0, 3.0, 13.0, 16.0, 13.0],
                _ => [2.0, 0.0, 2.0, 14.0, 16.0, 14.0],
            };
            FamilyShapes::new(px(s)).offset()
        }
        "coral_plant" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 15.0, 14.0])),
        "coral_fan" => FamilyShapes::new(px([2.0, 0.0, 2.0, 14.0, 4.0, 14.0])),
        "coral_wall_fan" => FamilyShapes::new(px(nesw(
            ctx.facing()?,
            [0.0, 4.0, 5.0, 16.0, 12.0, 16.0],
            [0.0, 4.0, 0.0, 11.0, 12.0, 16.0],
            [0.0, 4.0, 0.0, 16.0, 12.0, 11.0],
            [5.0, 4.0, 0.0, 16.0, 12.0, 16.0],
        ))),
        "sea_pickle" => FamilyShapes::new(px(match ctx.int("pickles")? {
            1 => [6.0, 0.0, 6.0, 10.0, 6.0, 10.0],
            2 => [3.0, 0.0, 3.0, 13.0, 6.0, 13.0],
            3 => [2.0, 0.0, 2.0, 14.0, 6.0, 14.0],
            _ => [2.0, 0.0, 2.0, 14.0, 7.0, 14.0],
        })),
        "turtle_egg" => FamilyShapes::new(px(if ctx.int("eggs")? == 1 {
            [3.0, 0.0, 3.0, 12.0, 7.0, 12.0]
        } else {
            [1.0, 0.0, 1.0, 15.0, 7.0, 15.0]
        })),
        "sniffer_egg" => FamilyShapes::new(px([1.0, 0.0, 2.0, 15.0, 16.0, 14.0])),
        "dried_ghast" => FamilyShapes::new(px([3.0, 0.0, 3.0, 13.0, 10.0, 13.0])),
        "frogspawn" => FamilyShapes::new(px([0.0, 0.0, 0.0, 16.0, 1.5, 16.0])),
        "amethyst_cluster" => {
            let (h, w) = variant_nums(var);
            FamilyShapes::new(amethyst(ctx, h, w)?)
        }
        "shelf_mushroom" => {
            let north = if ctx.int("age")? == 0 {
                union(&[
                    [3.0, 9.0, 9.0, 13.0, 11.0, 16.0],
                    [5.0, 8.0, 12.0, 11.0, 9.0, 16.0],
                ])
            } else {
                union(&[
                    [1.0, 8.0, 6.0, 15.0, 11.0, 16.0],
                    [4.0, 6.0, 10.0, 12.0, 8.0, 16.0],
                ])
            };
            FamilyShapes::new(rotated(&north, ctx.facing()?))
        }
        "chorus_plant" => {
            let mut s = px([3.0, 3.0, 3.0, 13.0, 13.0, 13.0]);
            for (name, b) in [
                ("north", [3.0, 3.0, 0.0, 13.0, 13.0, 8.0]),
                ("south", [3.0, 3.0, 8.0, 13.0, 13.0, 16.0]),
                ("east", [8.0, 3.0, 3.0, 16.0, 13.0, 13.0]),
                ("west", [0.0, 3.0, 3.0, 8.0, 13.0, 13.0]),
                ("up", [3.0, 8.0, 3.0, 13.0, 16.0, 13.0]),
                ("down", [3.0, 0.0, 3.0, 13.0, 8.0, 13.0]),
            ] {
                if ctx.bool(name)? {
                    s = s.or(&px(b));
                }
            }
            FamilyShapes::new(s)
        }
        "chorus_flower" => FamilyShapes::new(full()).support(VoxelShape::column(14.0, 0.0, 15.0)),
        other => {
            return Err(BuildError::UnknownFamily {
                block: info.id.clone(),
                family: other.to_owned(),
            });
        }
    })
}
