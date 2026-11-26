//! Packed block / section coordinates and direction helpers (`data-layers.md` §1).
//!
//! A *block node* packs `x` (26 bits, top), `z` (26 bits) and `y` (12 bits, low) into an `i64`; a *section node* packs
//! `x` (22 bits), `z` (22 bits) and `y` (20 bits). Every field is two's-complement truncated, and stepping a node
//! is unpack → add → repack, so a coordinate that leaves its field range wraps.

use zenith_blocks::Direction;

/// Packs block coordinates into a block node.
#[inline]
pub const fn block_node(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & 0x3FF_FFFF) << 38) | ((z as i64 & 0x3FF_FFFF) << 12) | (y as i64 & 0xFFF)
}

/// `x` of a block node.
#[inline]
pub const fn block_x(node: i64) -> i32 {
    (node >> 38) as i32
}

/// `y` of a block node.
#[inline]
pub const fn block_y(node: i64) -> i32 {
    ((node << 52) >> 52) as i32
}

/// `z` of a block node.
#[inline]
pub const fn block_z(node: i64) -> i32 {
    ((node << 26) >> 38) as i32
}

/// The block node one step from `node` towards `d`.
#[inline]
pub fn block_offset(node: i64, d: Direction) -> i64 {
    let (dx, dy, dz) = step(d);
    block_node(
        block_x(node).wrapping_add(dx),
        block_y(node).wrapping_add(dy),
        block_z(node).wrapping_add(dz),
    )
}

/// The unit step of a direction.
#[inline]
pub const fn step(d: Direction) -> (i32, i32, i32) {
    match d {
        Direction::Down => (0, -1, 0),
        Direction::Up => (0, 1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::West => (-1, 0, 0),
        Direction::East => (1, 0, 0),
    }
}

/// Packs section coordinates into a section node.
#[inline]
pub const fn section_node(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & 0x3F_FFFF) << 42) | ((z as i64 & 0x3F_FFFF) << 20) | (y as i64 & 0xF_FFFF)
}

/// `x` of a section node.
#[inline]
pub const fn section_x(node: i64) -> i32 {
    (node >> 42) as i32
}

/// `y` of a section node.
#[inline]
pub const fn section_y(node: i64) -> i32 {
    ((node << 44) >> 44) as i32
}

/// `z` of a section node.
#[inline]
pub const fn section_z(node: i64) -> i32 {
    ((node << 22) >> 42) as i32
}

/// The section node containing a block node (`blockToSection`).
#[inline]
pub const fn block_to_section(node: i64) -> i64 {
    section_node(block_x(node) >> 4, block_y(node) >> 4, block_z(node) >> 4)
}

/// Section node `node + (dx, dy, dz)` (unpack, add, repack).
#[inline]
pub const fn section_offset(node: i64, dx: i32, dy: i32, dz: i32) -> i64 {
    section_node(
        section_x(node).wrapping_add(dx),
        section_y(node).wrapping_add(dy),
        section_z(node).wrapping_add(dz),
    )
}

/// The *zero node* of a section node: the node with `y` cleared; identifies a section column.
#[inline]
pub const fn section_zero(node: i64) -> i64 {
    node & !0xF_FFFF
}

/// The zero node of the column of chunk `(cx, cz)`.
#[inline]
pub const fn chunk_zero(cx: i32, cz: i32) -> i64 {
    section_node(cx, 0, cz)
}

/// Section-local nibble index of `(x, y, z)` (each `0..16`): `(y << 8) | (z << 4) | x`.
#[inline]
pub const fn layer_index(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

/// The flat index of a block node: `y` rounded down to a multiple of 16 (the section-local `y = 0`).
#[inline]
pub const fn block_flat_index(node: i64) -> i64 {
    node & !15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_node_round_trips() {
        for &(x, y, z) in &[
            (0, 0, 0),
            (-1, -64, -1),
            (30_000_000, 319, -30_000_000),
            (-33_554_432, -2048, 33_554_431),
            (5, 2047, -7),
        ] {
            let n = block_node(x, y, z);
            assert_eq!((block_x(n), block_y(n), block_z(n)), (x, y, z));
        }
    }

    #[test]
    fn section_node_round_trips() {
        for &(x, y, z) in &[
            (0, 0, 0),
            (-1, -5, -1),
            (1_875_000, 20, -1_875_000),
            (3, -4, 9),
        ] {
            let n = section_node(x, y, z);
            assert_eq!((section_x(n), section_y(n), section_z(n)), (x, y, z));
        }
    }

    #[test]
    fn block_to_section_floors() {
        let n = block_node(-1, -1, 16);
        let s = block_to_section(n);
        assert_eq!((section_x(s), section_y(s), section_z(s)), (-1, -1, 1));
        assert_eq!(section_zero(s), section_node(-1, 0, 1));
    }

    #[test]
    fn offset_wraps_in_field() {
        let n = block_node(0, 2047, 0);
        assert_eq!(block_y(block_offset(n, Direction::Up)), -2048);
        let n = block_node(3, 4, 5);
        assert_eq!(block_offset(n, Direction::West), block_node(2, 4, 5));
        assert_eq!(block_offset(n, Direction::South), block_node(3, 4, 6));
    }

    #[test]
    fn flat_index_rounds_y() {
        let n = block_node(7, 37, 9);
        assert_eq!(block_y(block_flat_index(n)), 32);
        assert_eq!(block_x(block_flat_index(n)), 7);
        let n = block_node(7, -3, 9);
        assert_eq!(block_y(block_flat_index(n)), -16);
    }
}
