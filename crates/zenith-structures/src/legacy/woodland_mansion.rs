//! Woodland Mansion (`minecraft:woodland_mansion`) 11×11 3-floor grid planner and piece placer
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §6).

use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::Identifier;

use crate::geom::{
    BlockPos, Direction, Mirror, Rotation, dir_from_2d_data_value, dir_step,
    get_zero_position_with_transform, shuffle_slice,
};
use crate::legacy::{LegacyGenContext, LegacyPiece, get_lowest_y_in_5by5_box};
use crate::template::TemplateManager;

const CLEAR: i32 = 0;
const CORRIDOR: i32 = 1;
const ROOM: i32 = 2;
const START_ROOM: i32 = 3;
const TEST_ROOM: i32 = 4;
const BLOCKED: i32 = 5;

const ROOM_ID_MASK: i32 = 0xFFFF;
const ROOM_1X1: i32 = 0x10000;
const ROOM_1X2: i32 = 0x20000;
const ROOM_2X2: i32 = 0x40000;
const ROOM_TYPE_MASK: i32 = 0xF0000;
const ROOM_ORIGIN_FLAG: i32 = 0x10_0000;
const ROOM_DOOR_FLAG: i32 = 0x20_0000;
const ROOM_STAIRS_FLAG: i32 = 0x40_0000;
const ROOM_CORRIDOR_FLAG: i32 = 0x80_0000;

/// Generates pieces for `minecraft:woodland_mansion` (§6.1–§6.3).
pub fn generate(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    let block_x = (chunk_x << 4) + 7;
    let block_z = (chunk_z << 4) + 7;
    if !ctx.could_valid_biome_exist_in_terrain_column(block_x, block_z) {
        return None;
    }
    let rotation = Rotation::random(rng);
    let start_pos = get_lowest_y_in_5by5_box(ctx, block_x, block_z, rotation);
    if start_pos.y < 60 {
        return None;
    }
    if !ctx.is_valid_biome(start_pos) {
        return None;
    }
    let mut pieces = Vec::new();
    generate_mansion(
        ctx.template_manager(),
        start_pos,
        rotation,
        &mut pieces,
        rng,
    );
    Some(pieces)
}

#[derive(Debug, Clone)]
struct SimpleGrid {
    width: i32,
    height: i32,
    value_if_outside: i32,
    grid: Vec<i32>,
}

impl SimpleGrid {
    fn new(width: i32, height: i32, value_if_outside: i32) -> Self {
        Self {
            width,
            height,
            value_if_outside,
            grid: vec![0; (width * height) as usize],
        }
    }

    fn get(&self, x: i32, y: i32) -> i32 {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[(x + y * self.width) as usize]
        } else {
            self.value_if_outside
        }
    }

    fn set(&mut self, x: i32, y: i32, val: i32) {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[(x + y * self.width) as usize] = val;
        }
    }

    fn set_range(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, val: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, val);
            }
        }
    }

    fn setif(&mut self, x: i32, y: i32, expected: i32, val: i32) {
        if self.get(x, y) == expected {
            self.set(x, y, val);
        }
    }

    fn edges_to(&self, x: i32, y: i32, val: i32) -> bool {
        self.get(x - 1, y) == val
            || self.get(x + 1, y) == val
            || self.get(x, y + 1) == val
            || self.get(x, y - 1) == val
    }
}

#[inline]
fn is_house(grid: &SimpleGrid, x: i32, y: i32) -> bool {
    let v = grid.get(x, y);
    v == CORRIDOR || v == ROOM || v == START_ROOM || v == TEST_ROOM
}

#[inline]
fn is_room_id(
    grid: &SimpleGrid,
    x: i32,
    y: i32,
    floor_rooms: &[SimpleGrid; 3],
    floor: usize,
    room_id: i32,
) -> bool {
    is_house(grid, x, y) && (floor_rooms[floor].get(x, y) & ROOM_ID_MASK) == room_id
}

struct MansionGrid {
    entrance_x: i32,
    entrance_y: i32,
    base_grid: SimpleGrid,
    third_floor_grid: SimpleGrid,
    floor_rooms: [SimpleGrid; 3],
}

impl MansionGrid {
    fn new(rng: &mut LegacyRandom) -> Self {
        let mut base_grid = SimpleGrid::new(11, 11, BLOCKED);
        let third_floor_grid = SimpleGrid::new(11, 11, BLOCKED);
        let floor_rooms = [
            SimpleGrid::new(11, 11, BLOCKED),
            SimpleGrid::new(11, 11, BLOCKED),
            SimpleGrid::new(11, 11, BLOCKED),
        ];

        // §6.2 step 1
        base_grid.set_range(7, 4, 8, 5, START_ROOM);
        base_grid.set_range(6, 4, 6, 5, ROOM);
        base_grid.set_range(9, 2, 10, 7, BLOCKED);
        base_grid.set_range(8, 2, 8, 3, CORRIDOR);
        base_grid.set_range(8, 6, 8, 7, CORRIDOR);
        base_grid.set(6, 3, CORRIDOR);
        base_grid.set(6, 6, CORRIDOR);
        base_grid.set_range(0, 0, 11, 1, BLOCKED);
        base_grid.set_range(0, 9, 11, 11, BLOCKED);

        // §6.2 step 2: 4 recursive corridors in exact order
        Self::recursive_corridor(&mut base_grid, 7, 2, Direction::West, 6, rng);
        Self::recursive_corridor(&mut base_grid, 7, 7, Direction::West, 6, rng);
        Self::recursive_corridor(&mut base_grid, 5, 3, Direction::West, 3, rng);
        Self::recursive_corridor(&mut base_grid, 5, 6, Direction::West, 3, rng);

        // §6.2 step 3
        while Self::clean_edges(&mut base_grid) {}

        let mut mansion = Self {
            entrance_x: 7,
            entrance_y: 4,
            base_grid,
            third_floor_grid,
            floor_rooms,
        };

        // §6.2 step 4
        let base_copy = mansion.base_grid.clone();
        Self::identify_rooms(&base_copy, &mut mansion.floor_rooms[0], rng);
        Self::identify_rooms(&base_copy, &mut mansion.floor_rooms[1], rng);

        // §6.2 step 5
        mansion.floor_rooms[0].set_range(8, 4, 8, 5, ROOM_CORRIDOR_FLAG);
        mansion.floor_rooms[1].set_range(8, 4, 8, 5, ROOM_CORRIDOR_FLAG);

        // §6.2 step 6
        mansion.setup_third_floor(rng);

        // §6.2 step 7
        let third_copy = mansion.third_floor_grid.clone();
        Self::identify_rooms(&third_copy, &mut mansion.floor_rooms[2], rng);

        mansion
    }

    fn recursive_corridor(
        grid: &mut SimpleGrid,
        x: i32,
        y: i32,
        heading: Direction,
        depth: i32,
        rng: &mut LegacyRandom,
    ) {
        if depth <= 0 {
            return;
        }
        grid.set(x, y, CORRIDOR);
        let (hx, _, hz) = dir_step(heading);
        grid.setif(x + hx, y + hz, CLEAR, CORRIDOR);

        for _ in 0..8 {
            let next_dir = dir_from_2d_data_value(rng.next_int_bounded(4));
            if next_dir != heading.opposite()
                && (next_dir != Direction::East || !rng.next_boolean())
            {
                let nx = x + hx;
                let ny = y + hz;
                let (dx, _, dz) = dir_step(next_dir);
                if grid.get(nx + dx, ny + dz) == CLEAR
                    && grid.get(nx + dx * 2, ny + dz * 2) == CLEAR
                {
                    Self::recursive_corridor(grid, nx + dx, ny + dz, next_dir, depth - 1, rng);
                    break;
                }
            }
        }

        let cw = heading.clockwise();
        let ccw = heading.counter_clockwise();
        let (cwx, _, cwz) = dir_step(cw);
        let (ccwx, _, ccwz) = dir_step(ccw);
        grid.setif(x + cwx, y + cwz, CLEAR, ROOM);
        grid.setif(x + ccwx, y + ccwz, CLEAR, ROOM);
        grid.setif(x + hx + cwx, y + hz + cwz, CLEAR, ROOM);
        grid.setif(x + hx + ccwx, y + hz + ccwz, CLEAR, ROOM);
        grid.setif(x + hx * 2, y + hz * 2, CLEAR, ROOM);
        grid.setif(x + cwx * 2, y + cwz * 2, CLEAR, ROOM);
        grid.setif(x + ccwx * 2, y + ccwz * 2, CLEAR, ROOM);
    }

    fn clean_edges(grid: &mut SimpleGrid) -> bool {
        let mut changed = false;
        for y in 0..grid.height {
            for x in 0..grid.width {
                if grid.get(x, y) == CLEAR {
                    let mut cardinal = 0;
                    cardinal += i32::from(is_house(grid, x + 1, y));
                    cardinal += i32::from(is_house(grid, x - 1, y));
                    cardinal += i32::from(is_house(grid, x, y + 1));
                    cardinal += i32::from(is_house(grid, x, y - 1));
                    if cardinal >= 3 {
                        grid.set(x, y, ROOM);
                        changed = true;
                    } else if cardinal == 2 {
                        let mut diag = 0;
                        diag += i32::from(is_house(grid, x + 1, y + 1));
                        diag += i32::from(is_house(grid, x - 1, y + 1));
                        diag += i32::from(is_house(grid, x + 1, y - 1));
                        diag += i32::from(is_house(grid, x - 1, y - 1));
                        if diag <= 1 {
                            grid.set(x, y, ROOM);
                            changed = true;
                        }
                    }
                }
            }
        }
        changed
    }

    fn identify_rooms(from_grid: &SimpleGrid, room_grid: &mut SimpleGrid, rng: &mut LegacyRandom) {
        let mut cells = Vec::new();
        for y in 0..from_grid.height {
            for x in 0..from_grid.width {
                if from_grid.get(x, y) == ROOM {
                    cells.push((x, y));
                }
            }
        }
        shuffle_slice(&mut cells, rng);

        let mut room_id = 10_i32;
        for (x, y) in cells {
            if room_grid.get(x, y) != 0 {
                continue;
            }
            let mut x0 = x;
            let mut x1 = x;
            let mut y0 = y;
            let mut y1 = y;
            let mut room_type = ROOM_1X1;

            if room_grid.get(x + 1, y) == 0
                && room_grid.get(x, y + 1) == 0
                && room_grid.get(x + 1, y + 1) == 0
                && from_grid.get(x + 1, y) == ROOM
                && from_grid.get(x, y + 1) == ROOM
                && from_grid.get(x + 1, y + 1) == ROOM
            {
                x1 += 1;
                y1 += 1;
                room_type = ROOM_2X2;
            } else if room_grid.get(x - 1, y) == 0
                && room_grid.get(x, y + 1) == 0
                && room_grid.get(x - 1, y + 1) == 0
                && from_grid.get(x - 1, y) == ROOM
                && from_grid.get(x, y + 1) == ROOM
                && from_grid.get(x - 1, y + 1) == ROOM
            {
                x0 -= 1;
                y1 += 1;
                room_type = ROOM_2X2;
            } else if room_grid.get(x - 1, y) == 0
                && room_grid.get(x, y - 1) == 0
                && room_grid.get(x - 1, y - 1) == 0
                && from_grid.get(x - 1, y) == ROOM
                && from_grid.get(x, y - 1) == ROOM
                && from_grid.get(x - 1, y - 1) == ROOM
            {
                x0 -= 1;
                y0 -= 1;
                room_type = ROOM_2X2;
            } else if room_grid.get(x + 1, y) == 0 && from_grid.get(x + 1, y) == ROOM {
                x1 += 1;
                room_type = ROOM_1X2;
            } else if room_grid.get(x, y + 1) == 0 && from_grid.get(x, y + 1) == ROOM {
                y1 += 1;
                room_type = ROOM_1X2;
            } else if room_grid.get(x - 1, y) == 0 && from_grid.get(x - 1, y) == ROOM {
                x0 -= 1;
                room_type = ROOM_1X2;
            } else if room_grid.get(x, y - 1) == 0 && from_grid.get(x, y - 1) == ROOM {
                y0 -= 1;
                room_type = ROOM_1X2;
            }

            let mut door_x = if rng.next_boolean() { x0 } else { x1 };
            let mut door_y = if rng.next_boolean() { y0 } else { y1 };
            let mut door_flag = ROOM_DOOR_FLAG;
            if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
                door_x = if door_x == x0 { x1 } else { x0 };
                door_y = if door_y == y0 { y1 } else { y0 };
                if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
                    door_y = if door_y == y0 { y1 } else { y0 };
                    if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
                        door_x = if door_x == x0 { x1 } else { x0 };
                        door_y = if door_y == y0 { y1 } else { y0 };
                        if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
                            door_flag = 0;
                            door_x = x0;
                            door_y = y0;
                        }
                    }
                }
            }

            for ry in y0..=y1 {
                for rx in x0..=x1 {
                    if rx == door_x && ry == door_y {
                        room_grid.set(rx, ry, ROOM_ORIGIN_FLAG | door_flag | room_type | room_id);
                    } else {
                        room_grid.set(rx, ry, room_type | room_id);
                    }
                }
            }
            room_id += 1;
        }
    }

    fn get_1x2_room_direction(
        &self,
        grid: &SimpleGrid,
        x: i32,
        y: i32,
        floor: usize,
        room_id: i32,
    ) -> Option<Direction> {
        for dir in Direction::HORIZONTAL {
            let (dx, _, dz) = dir_step(dir);
            if is_room_id(grid, x + dx, y + dz, &self.floor_rooms, floor, room_id) {
                return Some(dir);
            }
        }
        None
    }

    fn setup_third_floor(&mut self, rng: &mut LegacyRandom) {
        let mut potential_rooms = Vec::new();
        for y in 0..self.floor_rooms[1].height {
            for x in 0..self.floor_rooms[1].width {
                let d = self.floor_rooms[1].get(x, y);
                let ty = d & ROOM_TYPE_MASK;
                if ty == ROOM_1X2 && (d & ROOM_DOOR_FLAG) == ROOM_DOOR_FLAG {
                    potential_rooms.push((x, y));
                }
            }
        }
        if potential_rooms.is_empty() {
            self.third_floor_grid.set_range(
                0,
                0,
                self.third_floor_grid.width,
                self.third_floor_grid.height,
                BLOCKED,
            );
            return;
        }
        let (rx, ry) = potential_rooms[rng.next_int_bounded(potential_rooms.len() as i32) as usize];
        let val = self.floor_rooms[1].get(rx, ry);
        self.floor_rooms[1].set(rx, ry, val | ROOM_STAIRS_FLAG);
        let Some(room_dir) =
            self.get_1x2_room_direction(&self.base_grid, rx, ry, 1, val & ROOM_ID_MASK)
        else {
            return;
        };
        let (dx, _, dz) = dir_step(room_dir);
        let end_x = rx + dx;
        let end_y = ry + dz;

        for y in 0..self.third_floor_grid.height {
            for x in 0..self.third_floor_grid.width {
                if !is_house(&self.base_grid, x, y) {
                    self.third_floor_grid.set(x, y, BLOCKED);
                } else if x == rx && y == ry {
                    self.third_floor_grid.set(x, y, START_ROOM);
                } else if x == end_x && y == end_y {
                    self.third_floor_grid.set(x, y, START_ROOM);
                    self.floor_rooms[2].set(x, y, ROOM_CORRIDOR_FLAG);
                }
            }
        }

        let mut potential_corridors = Vec::new();
        for dir in Direction::HORIZONTAL {
            let (cx, _, cz) = dir_step(dir);
            if self.third_floor_grid.get(end_x + cx, end_y + cz) == CLEAR {
                potential_corridors.push(dir);
            }
        }

        if potential_corridors.is_empty() {
            self.third_floor_grid.set_range(
                0,
                0,
                self.third_floor_grid.width,
                self.third_floor_grid.height,
                BLOCKED,
            );
            let cur = self.floor_rooms[1].get(rx, ry);
            self.floor_rooms[1].set(rx, ry, cur & !ROOM_STAIRS_FLAG);
            return;
        }

        let corr_dir =
            potential_corridors[rng.next_int_bounded(potential_corridors.len() as i32) as usize];
        let (cx, _, cz) = dir_step(corr_dir);
        Self::recursive_corridor(
            &mut self.third_floor_grid,
            end_x + cx,
            end_y + cz,
            corr_dir,
            4,
            rng,
        );
        while Self::clean_edges(&mut self.third_floor_grid) {}
    }
}

struct PlacementData {
    rotation: Rotation,
    position: BlockPos,
    wall_type: &'static str,
}

fn emit_wmp(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    name: &str,
    pos: BlockPos,
    rot: Rotation,
    mirror: Mirror,
) {
    let Ok(id) = Identifier::with_default_namespace(format!("woodland_mansion/{name}")) else {
        return;
    };
    let Ok(tpl) = tm.get(&id) else {
        return;
    };
    let bb = tpl.bounding_box(pos, rot, BlockPos::ZERO, mirror);
    pieces.push(LegacyPiece::new(
        "minecraft:wmp",
        bb,
        Some(Direction::North),
        0,
    ));
}

fn generate_mansion(
    tm: &TemplateManager,
    origin: BlockPos,
    rotation: Rotation,
    pieces: &mut Vec<LegacyPiece>,
    rng: &mut LegacyRandom,
) {
    let mansion = MansionGrid::new(rng);
    let start_x = mansion.entrance_x + 1;
    let start_y = mansion.entrance_y + 1;

    // 1. Entrance
    emit_wmp(
        tm,
        pieces,
        "entrance",
        origin.relative(rotation.rotate_dir(Direction::West), 9),
        rotation,
        Mirror::None,
    );

    let mut data = PlacementData {
        rotation,
        position: origin.relative(rotation.rotate_dir(Direction::South), 16),
        wall_type: "wall_flat",
    };

    // 2. Outer walls (floors 1 & 2)
    traverse_outer_walls(
        tm,
        pieces,
        &mut data,
        &mansion.base_grid,
        Direction::South,
        start_x,
        start_y,
        start_x,
        start_y - 1,
    );

    let mut second_data = PlacementData {
        rotation,
        position: origin
            .relative(rotation.rotate_dir(Direction::South), 16)
            .above(8),
        wall_type: "wall_window",
    };
    traverse_outer_walls(
        tm,
        pieces,
        &mut second_data,
        &mansion.base_grid,
        Direction::South,
        start_x,
        start_y,
        start_x,
        start_y - 1,
    );

    // 3. Outer walls (floor 3)
    let mut third_data = PlacementData {
        rotation,
        position: origin
            .relative(rotation.rotate_dir(Direction::South), 16)
            .above(19),
        wall_type: "wall_window",
    };
    'outer_third: for y in 0..mansion.third_floor_grid.height {
        for x in (0..mansion.third_floor_grid.width).rev() {
            if is_house(&mansion.third_floor_grid, x, y) {
                third_data.position = origin
                    .above(19)
                    .relative(rotation.rotate_dir(Direction::South), 8 + (y - start_y) * 8)
                    .relative(rotation.rotate_dir(Direction::East), (x - start_x) * 8);
                traverse_wall_piece(tm, pieces, &mut third_data);
                traverse_outer_walls(
                    tm,
                    pieces,
                    &mut third_data,
                    &mansion.third_floor_grid,
                    Direction::South,
                    x,
                    y,
                    x,
                    y,
                );
                break 'outer_third;
            }
        }
    }

    // 4. Roofs
    create_roof(
        tm,
        pieces,
        origin.above(16),
        rotation,
        &mansion.base_grid,
        Some(&mansion.third_floor_grid),
        start_x,
        start_y,
    );
    create_roof(
        tm,
        pieces,
        origin.above(27),
        rotation,
        &mansion.third_floor_grid,
        None,
        start_x,
        start_y,
    );

    // 5. Interior floors (0, 1, 2)
    for floor_num in 0..3usize {
        let floor_origin =
            origin.above(8 * (floor_num as i32) + if floor_num == 2 { 3 } else { 0 });
        let grid = if floor_num == 2 {
            &mansion.third_floor_grid
        } else {
            &mansion.base_grid
        };
        let rooms_grid = &mansion.floor_rooms[floor_num];
        let south_carpet = if floor_num == 0 {
            "carpet_south_1"
        } else {
            "carpet_south_2"
        };
        let west_carpet = if floor_num == 0 {
            "carpet_west_1"
        } else {
            "carpet_west_2"
        };

        // Corridors
        for y in 0..grid.height {
            for x in 0..grid.width {
                if grid.get(x, y) == CORRIDOR {
                    let pos = floor_origin
                        .relative(rotation.rotate_dir(Direction::South), 8 + (y - start_y) * 8)
                        .relative(rotation.rotate_dir(Direction::East), (x - start_x) * 8);
                    emit_wmp(tm, pieces, "corridor_floor", pos, rotation, Mirror::None);

                    if grid.get(x, y - 1) == CORRIDOR
                        || (rooms_grid.get(x, y - 1) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        emit_wmp(
                            tm,
                            pieces,
                            "carpet_north",
                            pos.relative(rotation.rotate_dir(Direction::East), 1)
                                .above(1),
                            rotation,
                            Mirror::None,
                        );
                    }
                    if grid.get(x + 1, y) == CORRIDOR
                        || (rooms_grid.get(x + 1, y) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        emit_wmp(
                            tm,
                            pieces,
                            "carpet_east",
                            pos.relative(rotation.rotate_dir(Direction::South), 1)
                                .relative(rotation.rotate_dir(Direction::East), 5)
                                .above(1),
                            rotation,
                            Mirror::None,
                        );
                    }
                    if grid.get(x, y + 1) == CORRIDOR
                        || (rooms_grid.get(x, y + 1) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        emit_wmp(
                            tm,
                            pieces,
                            south_carpet,
                            pos.relative(rotation.rotate_dir(Direction::South), 5)
                                .relative(rotation.rotate_dir(Direction::West), 1),
                            rotation,
                            Mirror::None,
                        );
                    }
                    if grid.get(x - 1, y) == CORRIDOR
                        || (rooms_grid.get(x - 1, y) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        emit_wmp(
                            tm,
                            pieces,
                            west_carpet,
                            pos.relative(rotation.rotate_dir(Direction::West), 1)
                                .relative(rotation.rotate_dir(Direction::North), 1),
                            rotation,
                            Mirror::None,
                        );
                    }
                }
            }
        }

        // Rooms & interior walls/doors
        let wall_piece = if floor_num == 0 {
            "indoors_wall_1"
        } else {
            "indoors_wall_2"
        };
        let door_piece = if floor_num == 0 {
            "indoors_door_1"
        } else {
            "indoors_door_2"
        };

        for y in 0..grid.height {
            for x in 0..grid.width {
                let third_start = floor_num == 2 && grid.get(x, y) == START_ROOM;
                if grid.get(x, y) != ROOM && !third_start {
                    continue;
                }
                let room_data = rooms_grid.get(x, y);
                let room_id = room_data & ROOM_ID_MASK;
                let room_type = room_data & ROOM_TYPE_MASK;

                let door_dir: Option<Direction> = if (room_data & ROOM_DOOR_FLAG) == ROOM_DOOR_FLAG
                {
                    let mut dirs = Vec::new();
                    for dir in Direction::HORIZONTAL {
                        let (dx, _, dz) = dir_step(dir);
                        if grid.get(x + dx, y + dz) == CORRIDOR {
                            dirs.push(dir);
                        }
                    }
                    if !dirs.is_empty() {
                        Some(dirs[rng.next_int_bounded(dirs.len() as i32) as usize])
                    } else if (room_data & ROOM_ORIGIN_FLAG) == ROOM_ORIGIN_FLAG {
                        Some(Direction::Up)
                    } else {
                        None
                    }
                } else if (room_data & ROOM_ORIGIN_FLAG) == ROOM_ORIGIN_FLAG {
                    Some(Direction::Up)
                } else {
                    None
                };

                let room_pos = floor_origin
                    .relative(rotation.rotate_dir(Direction::South), 8 + (y - start_y) * 8)
                    .relative(rotation.rotate_dir(Direction::East), -1 + (x - start_x) * 8);

                let suppress_third_corridor =
                    third_start && (room_data & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG;

                // x - 1
                if is_house(grid, x - 1, y)
                    && !is_room_id(grid, x - 1, y, &mansion.floor_rooms, floor_num, room_id)
                {
                    let name = if door_dir == Some(Direction::West) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    emit_wmp(tm, pieces, name, room_pos, rotation, Mirror::None);
                }
                // x + 1
                if grid.get(x + 1, y) == CORRIDOR && !suppress_third_corridor {
                    let name = if door_dir == Some(Direction::East) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    emit_wmp(
                        tm,
                        pieces,
                        name,
                        room_pos.relative(rotation.rotate_dir(Direction::East), 8),
                        rotation,
                        Mirror::None,
                    );
                }
                // y + 1
                if is_house(grid, x, y + 1)
                    && !is_room_id(grid, x, y + 1, &mansion.floor_rooms, floor_num, room_id)
                {
                    let name = if door_dir == Some(Direction::South) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    emit_wmp(
                        tm,
                        pieces,
                        name,
                        room_pos
                            .relative(rotation.rotate_dir(Direction::South), 7)
                            .relative(rotation.rotate_dir(Direction::East), 7),
                        rotation.get_rotated(Rotation::Clockwise90),
                        Mirror::None,
                    );
                }
                // y - 1
                if grid.get(x, y - 1) == CORRIDOR && !suppress_third_corridor {
                    let name = if door_dir == Some(Direction::North) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    emit_wmp(
                        tm,
                        pieces,
                        name,
                        room_pos
                            .relative(rotation.rotate_dir(Direction::North), 1)
                            .relative(rotation.rotate_dir(Direction::East), 7),
                        rotation.get_rotated(Rotation::Clockwise90),
                        Mirror::None,
                    );
                }

                // Room template emission
                if room_type == ROOM_1X1 {
                    if let Some(dd) = door_dir {
                        add_room_1x1(tm, pieces, room_pos, rotation, dd, floor_num, rng);
                    }
                } else if room_type == ROOM_1X2 {
                    if let Some(dd) = door_dir
                        && let Some(rdir) =
                            mansion.get_1x2_room_direction(grid, x, y, floor_num, room_id)
                    {
                        let is_stairs = (room_data & ROOM_STAIRS_FLAG) == ROOM_STAIRS_FLAG;
                        add_room_1x2(
                            tm, pieces, room_pos, rotation, rdir, dd, floor_num, is_stairs, rng,
                        );
                    }
                } else if room_type == ROOM_2X2
                    && let Some(dd) = door_dir
                {
                    if dd == Direction::Up {
                        add_room_2x2_secret(tm, pieces, room_pos, rotation);
                    } else {
                        let mut rdir = dd.clockwise();
                        let (rdx, _, rdz) = dir_step(rdir);
                        if !is_room_id(
                            grid,
                            x + rdx,
                            y + rdz,
                            &mansion.floor_rooms,
                            floor_num,
                            room_id,
                        ) {
                            rdir = rdir.opposite();
                        }
                        add_room_2x2(tm, pieces, room_pos, rotation, rdir, dd, floor_num, rng);
                    }
                }
            }
        }
    }
}

fn traverse_wall_piece(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    data: &mut PlacementData,
) {
    emit_wmp(
        tm,
        pieces,
        data.wall_type,
        data.position
            .relative(data.rotation.rotate_dir(Direction::East), 7),
        data.rotation,
        Mirror::None,
    );
    data.position = data
        .position
        .relative(data.rotation.rotate_dir(Direction::South), 8);
}

#[allow(clippy::too_many_arguments)]
fn traverse_outer_walls(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    data: &mut PlacementData,
    grid: &SimpleGrid,
    start_dir: Direction,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
) {
    let mut gx = start_x;
    let mut gy = start_y;
    let mut dir = start_dir;

    loop {
        let (dx, _, dz) = dir_step(dir);
        let (ccwx, _, ccwz) = dir_step(dir.counter_clockwise());
        if !is_house(grid, gx + dx, gy + dz) {
            // Outer corner: mutate data.position by -1*SOUTH before emitting wall_corner,
            // then shift by -7*SOUTH - 6*WEST (net -8*SOUTH - 6*WEST).
            data.position = data
                .position
                .relative(data.rotation.rotate_dir(Direction::South), -1);
            emit_wmp(
                tm,
                pieces,
                "wall_corner",
                data.position,
                data.rotation,
                Mirror::None,
            );
            data.position = data
                .position
                .relative(data.rotation.rotate_dir(Direction::South), -7)
                .relative(data.rotation.rotate_dir(Direction::West), -6);
            data.rotation = data.rotation.get_rotated(Rotation::Clockwise90);
            dir = dir.clockwise();
            if gx != end_x || gy != end_y || dir != start_dir {
                traverse_wall_piece(tm, pieces, data);
            }
        } else if is_house(grid, gx + dx, gy + dz) && is_house(grid, gx + dx + ccwx, gy + dz + ccwz)
        {
            // Inner corner
            data.position = data
                .position
                .relative(data.rotation.rotate_dir(Direction::South), 6)
                .relative(data.rotation.rotate_dir(Direction::East), 8);
            data.rotation = data.rotation.get_rotated(Rotation::Counterclockwise90);
            gx += dx;
            gy += dz;
            dir = dir.counter_clockwise();
        } else {
            // Straight
            gx += dx;
            gy += dz;
            if gx != end_x || gy != end_y || dir != start_dir {
                traverse_wall_piece(tm, pieces, data);
            }
        }
        if gx == end_x && gy == end_y && dir == start_dir {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_roof(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    origin: BlockPos,
    rotation: Rotation,
    grid: &SimpleGrid,
    above_grid: Option<&SimpleGrid>,
    start_x: i32,
    start_y: i32,
) {
    let cell_pos = |x: i32, y: i32| {
        origin
            .relative(rotation.rotate_dir(Direction::South), 8 + (y - start_y) * 8)
            .relative(rotation.rotate_dir(Direction::East), (x - start_x) * 8)
    };
    let covered = |x: i32, y: i32| above_grid.is_some_and(|ag| is_house(ag, x, y));

    // Pass 1
    for y in 0..grid.height {
        for x in 0..grid.width {
            if is_house(grid, x, y) && !covered(x, y) {
                let pos = cell_pos(x, y);
                emit_wmp(tm, pieces, "roof", pos.above(3), rotation, Mirror::None);
                if !is_house(grid, x + 1, y) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_front",
                        pos.relative(rotation.rotate_dir(Direction::East), 6),
                        rotation,
                        Mirror::None,
                    );
                }
                if !is_house(grid, x - 1, y) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_front",
                        pos.relative(rotation.rotate_dir(Direction::South), 7),
                        rotation.get_rotated(Rotation::Clockwise180),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y - 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_front",
                        pos.relative(rotation.rotate_dir(Direction::West), 1),
                        rotation.get_rotated(Rotation::Counterclockwise90),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y + 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_front",
                        pos.relative(rotation.rotate_dir(Direction::East), 6)
                            .relative(rotation.rotate_dir(Direction::South), 6),
                        rotation.get_rotated(Rotation::Clockwise90),
                        Mirror::None,
                    );
                }
            }
        }
    }

    // Pass 2 (small walls where above_grid also has house)
    if let Some(ag) = above_grid {
        for y in 0..grid.height {
            for x in 0..grid.width {
                if is_house(grid, x, y) && is_house(ag, x, y) {
                    let pos = cell_pos(x, y);
                    if !is_house(grid, x + 1, y) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall",
                            pos.relative(rotation.rotate_dir(Direction::East), 7),
                            rotation,
                            Mirror::None,
                        );
                    }
                    if !is_house(grid, x - 1, y) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall",
                            pos.relative(rotation.rotate_dir(Direction::West), 1)
                                .relative(rotation.rotate_dir(Direction::South), 6),
                            rotation.get_rotated(Rotation::Clockwise180),
                            Mirror::None,
                        );
                    }
                    if !is_house(grid, x, y - 1) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall",
                            pos.relative(rotation.rotate_dir(Direction::North), 1),
                            rotation.get_rotated(Rotation::Counterclockwise90),
                            Mirror::None,
                        );
                    }
                    if !is_house(grid, x, y + 1) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall",
                            pos.relative(rotation.rotate_dir(Direction::East), 6)
                                .relative(rotation.rotate_dir(Direction::South), 7),
                            rotation.get_rotated(Rotation::Clockwise90),
                            Mirror::None,
                        );
                    }
                    // Outer corners
                    if !is_house(grid, x + 1, y) && !is_house(grid, x, y - 1) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall_corner",
                            pos.relative(rotation.rotate_dir(Direction::East), 7)
                                .relative(rotation.rotate_dir(Direction::North), 2),
                            rotation,
                            Mirror::None,
                        );
                    }
                    if !is_house(grid, x + 1, y) && !is_house(grid, x, y + 1) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall_corner",
                            pos.relative(rotation.rotate_dir(Direction::East), 8)
                                .relative(rotation.rotate_dir(Direction::South), 7),
                            rotation.get_rotated(Rotation::Clockwise90),
                            Mirror::None,
                        );
                    }
                    if !is_house(grid, x - 1, y) && !is_house(grid, x, y - 1) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall_corner",
                            pos.relative(rotation.rotate_dir(Direction::West), 2)
                                .relative(rotation.rotate_dir(Direction::North), 1),
                            rotation.get_rotated(Rotation::Counterclockwise90),
                            Mirror::None,
                        );
                    }
                    if !is_house(grid, x - 1, y) && !is_house(grid, x, y + 1) {
                        emit_wmp(
                            tm,
                            pieces,
                            "small_wall_corner",
                            pos.relative(rotation.rotate_dir(Direction::West), 1)
                                .relative(rotation.rotate_dir(Direction::South), 8),
                            rotation.get_rotated(Rotation::Clockwise180),
                            Mirror::None,
                        );
                    }
                }
            }
        }
    }

    // Pass 3 (roof corners and inner corners)
    for y in 0..grid.height {
        for x in 0..grid.width {
            if !is_house(grid, x, y) || covered(x, y) {
                continue;
            }
            let pos = cell_pos(x, y);
            if !is_house(grid, x + 1, y) {
                if !is_house(grid, x, y + 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_corner",
                        pos.relative(rotation.rotate_dir(Direction::East), 6)
                            .relative(rotation.rotate_dir(Direction::South), 6),
                        rotation,
                        Mirror::None,
                    );
                } else if is_house(grid, x + 1, y + 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_inner_corner",
                        pos.relative(rotation.rotate_dir(Direction::East), 6)
                            .relative(rotation.rotate_dir(Direction::South), 5),
                        rotation,
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y - 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_corner",
                        pos.relative(rotation.rotate_dir(Direction::East), 6),
                        rotation.get_rotated(Rotation::Counterclockwise90),
                        Mirror::None,
                    );
                } else if is_house(grid, x + 1, y - 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_inner_corner",
                        pos.relative(rotation.rotate_dir(Direction::East), 9)
                            .relative(rotation.rotate_dir(Direction::North), 2),
                        rotation.get_rotated(Rotation::Clockwise90),
                        Mirror::None,
                    );
                }
            }
            if !is_house(grid, x - 1, y) {
                if !is_house(grid, x, y + 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_corner",
                        pos.relative(rotation.rotate_dir(Direction::South), 6),
                        rotation.get_rotated(Rotation::Clockwise90),
                        Mirror::None,
                    );
                } else if is_house(grid, x - 1, y + 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_inner_corner",
                        pos.relative(rotation.rotate_dir(Direction::South), 8)
                            .relative(rotation.rotate_dir(Direction::West), 3),
                        rotation.get_rotated(Rotation::Counterclockwise90),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y - 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_corner",
                        pos,
                        rotation.get_rotated(Rotation::Clockwise180),
                        Mirror::None,
                    );
                } else if is_house(grid, x - 1, y - 1) {
                    emit_wmp(
                        tm,
                        pieces,
                        "roof_inner_corner",
                        pos.relative(rotation.rotate_dir(Direction::South), 1),
                        rotation.get_rotated(Rotation::Clockwise180),
                        Mirror::None,
                    );
                }
            }
        }
    }
}

fn get_1x1(floor: usize, rng: &mut LegacyRandom) -> String {
    let prefix = if floor == 0 { "1x1_a" } else { "1x1_b" };
    format!("{prefix}{}", rng.next_int_bounded(5) + 1)
}

fn get_1x1_secret(rng: &mut LegacyRandom) -> String {
    format!("1x1_as{}", rng.next_int_bounded(4) + 1)
}

fn get_1x2_side(floor: usize, is_stairs: bool, rng: &mut LegacyRandom) -> String {
    if floor == 0 {
        format!("1x2_a{}", rng.next_int_bounded(9) + 1)
    } else if is_stairs {
        "1x2_c_stairs".to_owned()
    } else {
        format!("1x2_c{}", rng.next_int_bounded(4) + 1)
    }
}

fn get_1x2_front(floor: usize, is_stairs: bool, rng: &mut LegacyRandom) -> String {
    if floor == 0 {
        format!("1x2_b{}", rng.next_int_bounded(5) + 1)
    } else if is_stairs {
        "1x2_d_stairs".to_owned()
    } else {
        format!("1x2_d{}", rng.next_int_bounded(5) + 1)
    }
}

fn get_1x2_secret(floor: usize, rng: &mut LegacyRandom) -> String {
    if floor == 0 {
        format!("1x2_s{}", rng.next_int_bounded(2) + 1)
    } else {
        // Spec edge case #9: still calls nextInt(1)!
        format!("1x2_se{}", rng.next_int_bounded(1) + 1)
    }
}

fn get_2x2(floor: usize, rng: &mut LegacyRandom) -> String {
    if floor == 0 {
        format!("2x2_a{}", rng.next_int_bounded(4) + 1)
    } else {
        format!("2x2_b{}", rng.next_int_bounded(5) + 1)
    }
}

fn add_room_1x1(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    room_pos: BlockPos,
    rot: Rotation,
    door_dir: Direction,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    // Always draws get1x1 first (§6.3 & edge case #9)!
    let mut name = get_1x1(floor, rng);
    let piece_rot = match door_dir {
        Direction::East => Rotation::None,
        Direction::North => Rotation::Counterclockwise90,
        Direction::West => Rotation::Clockwise180,
        Direction::South => Rotation::Clockwise90,
        _ => {
            name = get_1x1_secret(rng);
            Rotation::None
        }
    };
    let off = rot.rotate_pos(get_zero_position_with_transform(
        BlockPos::new(1, 0, 0),
        Mirror::None,
        piece_rot,
        7,
        7,
    ));
    let pos = room_pos.offset(off.x, 0, off.z);
    emit_wmp(
        tm,
        pieces,
        &name,
        pos,
        piece_rot.get_rotated(rot),
        Mirror::None,
    );
}

#[allow(clippy::too_many_arguments)]
fn add_room_1x2(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    room_pos: BlockPos,
    rot: Rotation,
    room_dir: Direction,
    door_dir: Direction,
    floor: usize,
    is_stairs: bool,
    rng: &mut LegacyRandom,
) {
    let rel = |e: i32, s: i32| {
        room_pos
            .relative(rot.rotate_dir(Direction::East), e)
            .relative(rot.rotate_dir(Direction::South), s)
    };
    match (door_dir, room_dir) {
        // 8 side-entrance combinations
        (Direction::East, Direction::South) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(tm, pieces, &n, rel(1, 0), rot, Mirror::None);
        }
        (Direction::East, Direction::North) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(tm, pieces, &n, rel(1, 6), rot, Mirror::LeftRight);
        }
        (Direction::West, Direction::North) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(7, 6),
                rot.get_rotated(Rotation::Clockwise180),
                Mirror::None,
            );
        }
        (Direction::West, Direction::South) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(tm, pieces, &n, rel(7, 0), rot, Mirror::FrontBack);
        }
        (Direction::South, Direction::East) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(1, 0),
                rot.get_rotated(Rotation::Clockwise90),
                Mirror::LeftRight,
            );
        }
        (Direction::South, Direction::West) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(7, 0),
                rot.get_rotated(Rotation::Clockwise90),
                Mirror::None,
            );
        }
        (Direction::North, Direction::West) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(7, 6),
                rot.get_rotated(Rotation::Clockwise90),
                Mirror::FrontBack,
            );
        }
        (Direction::North, Direction::East) => {
            let n = get_1x2_side(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(1, 6),
                rot.get_rotated(Rotation::Counterclockwise90),
                Mirror::None,
            );
        }
        // 4 front-entrance combinations
        (Direction::South, Direction::North) => {
            let n = get_1x2_front(floor, is_stairs, rng);
            emit_wmp(tm, pieces, &n, rel(1, -8), rot, Mirror::None);
        }
        (Direction::North, Direction::South) => {
            let n = get_1x2_front(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(7, 14),
                rot.get_rotated(Rotation::Clockwise180),
                Mirror::None,
            );
        }
        (Direction::West, Direction::East) => {
            let n = get_1x2_front(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(15, 0),
                rot.get_rotated(Rotation::Clockwise90),
                Mirror::None,
            );
        }
        (Direction::East, Direction::West) => {
            let n = get_1x2_front(floor, is_stairs, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(-7, 6),
                rot.get_rotated(Rotation::Counterclockwise90),
                Mirror::None,
            );
        }
        // 2 secret combinations
        (Direction::Up, Direction::East) => {
            let n = get_1x2_secret(floor, rng);
            emit_wmp(
                tm,
                pieces,
                &n,
                rel(15, 0),
                rot.get_rotated(Rotation::Clockwise90),
                Mirror::None,
            );
        }
        (Direction::Up, Direction::South) => {
            let n = get_1x2_secret(floor, rng);
            emit_wmp(tm, pieces, &n, rel(1, 0), rot, Mirror::None);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn add_room_2x2(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    room_pos: BlockPos,
    rot: Rotation,
    room_dir: Direction,
    door_dir: Direction,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    let (e, s, rdelta, mirror) = match (door_dir, room_dir) {
        (Direction::East, Direction::South) => (-7, 0, Rotation::None, Mirror::None),
        (Direction::East, Direction::North) => (-7, 6, Rotation::None, Mirror::LeftRight),
        (Direction::North, Direction::East) => (1, 14, Rotation::Counterclockwise90, Mirror::None),
        (Direction::North, Direction::West) => {
            (7, 14, Rotation::Counterclockwise90, Mirror::LeftRight)
        }
        (Direction::South, Direction::West) => (7, -8, Rotation::Clockwise90, Mirror::None),
        (Direction::South, Direction::East) => (1, -8, Rotation::Clockwise90, Mirror::LeftRight),
        (Direction::West, Direction::North) => (15, 6, Rotation::Clockwise180, Mirror::None),
        (Direction::West, Direction::South) => (15, 0, Rotation::None, Mirror::FrontBack),
        _ => return,
    };
    let name = get_2x2(floor, rng);
    let pos = room_pos
        .relative(rot.rotate_dir(Direction::East), e)
        .relative(rot.rotate_dir(Direction::South), s);
    emit_wmp(tm, pieces, &name, pos, rot.get_rotated(rdelta), mirror);
}

fn add_room_2x2_secret(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    room_pos: BlockPos,
    rot: Rotation,
) {
    emit_wmp(
        tm,
        pieces,
        "2x2_s1",
        room_pos.relative(rot.rotate_dir(Direction::East), 1),
        rot,
        Mirror::None,
    );
}
