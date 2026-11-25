//! 64-bit coordinate packing (`BlockPos`, `SectionPos`, `ChunkPos`) and `QuartPos` shifts.
//!
//! Implements Section 8 of `docs/specs/math/java-math-helpers.md`.

use crate::java::ushr_i32;

// ---------------------------------------------------------------------------
// BlockPos 64-bit packing (X: 26b [63:38], Z: 26b [37:12], Y: 12b [11:0])
// ---------------------------------------------------------------------------

/// Bit width of packed horizontal block coordinates (`26`).
pub const BLOCK_PACKED_HORIZONTAL_LENGTH: u32 = 26;

/// Bit width of packed vertical block coordinate (`12`).
pub const BLOCK_PACKED_Y_LENGTH: u32 = 12;

/// Maximum signed positive horizontal block coordinate (`2^25 - 1 = 33,554,431`).
pub const MAX_HORIZONTAL_COORDINATE: i32 = (1 << 25) - 1;

/// Minimum signed horizontal block coordinate (`-2^25 = -33,554,432`).
pub const MIN_HORIZONTAL_COORDINATE: i32 = -(1 << 25);

/// Maximum signed vertical block coordinate (`2^11 - 1 = 2,047`).
pub const MAX_PACKED_BLOCK_Y: i32 = (1 << 11) - 1;

/// Minimum signed vertical block coordinate (`-2^11 = -2,048`).
pub const MIN_PACKED_BLOCK_Y: i32 = -(1 << 11);

/// 26-bit mask for packed block X/Z (`0x3FFFFFF`).
pub const BLOCK_PACKED_X_MASK: i64 = 0x3FF_FFFF;

/// 26-bit mask for packed block Z (`0x3FFFFFF`).
pub const BLOCK_PACKED_Z_MASK: i64 = 0x3FF_FFFF;

/// 12-bit mask for packed block Y (`0xFFF`).
pub const BLOCK_PACKED_Y_MASK: i64 = 0xFFF;

/// Bit offset of packed block X (`38`).
pub const BLOCK_X_OFFSET: u32 = 38;

/// Bit offset of packed block Z (`12`).
pub const BLOCK_Z_OFFSET: u32 = 12;

/// `BlockPos.asLong(x, y, z)`: packs X (26b), Z (26b), Y (12b) into a 64-bit integer.
///
/// Out-of-range coordinates silently wrap to their low 26 or 12 two's-complement bits.
#[inline]
pub const fn block_as_long(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & BLOCK_PACKED_X_MASK) << BLOCK_X_OFFSET)
        | ((z as i64 & BLOCK_PACKED_Z_MASK) << BLOCK_Z_OFFSET)
        | (y as i64 & BLOCK_PACKED_Y_MASK)
}

/// `BlockPos.getX(long)`: sign-extended 26-bit X coordinate.
#[inline]
pub const fn block_get_x(packed: i64) -> i32 {
    (packed >> 38) as i32
}

/// `BlockPos.getZ(long)`: sign-extended 26-bit Z coordinate.
#[inline]
pub const fn block_get_z(packed: i64) -> i32 {
    ((packed << 26) >> 38) as i32
}

/// `BlockPos.getY(long)`: sign-extended 12-bit Y coordinate.
#[inline]
pub const fn block_get_y(packed: i64) -> i32 {
    ((packed << 52) >> 52) as i32
}

/// `BlockPos.offset(long, dx, dy, dz)`: unpacks, adds in wrapping `i32`, and repacks so Y
/// overflow wraps within 12 bits and never bleeds into Z.
#[inline]
pub const fn block_offset(packed: i64, dx: i32, dy: i32, dz: i32) -> i64 {
    block_as_long(
        block_get_x(packed).wrapping_add(dx),
        block_get_y(packed).wrapping_add(dy),
        block_get_z(packed).wrapping_add(dz),
    )
}

/// `BlockPos.getFlatIndex(long)`: clears the low 4 bits of Y (`packed & -16L`).
#[inline]
pub const fn block_get_flat_index(packed: i64) -> i64 {
    packed & -16_i64
}

// ---------------------------------------------------------------------------
// SectionPos 64-bit packing (X: 22b [63:42], Z: 22b [41:20], Y: 20b [19:0])
// ---------------------------------------------------------------------------

/// 22-bit mask for packed section X/Z (`0x3FFFFF`).
pub const SECTION_PACKED_XZ_MASK: i64 = 0x3F_FFFF;

/// 20-bit mask for packed section Y (`0xFFFFF`).
pub const SECTION_PACKED_Y_MASK: i64 = 0xF_FFFF;

/// Maximum signed section X/Z (`2^21 - 1 = 2,097,151`).
pub const MAX_PACKED_SECTION_XZ: i32 = (1 << 21) - 1;

/// Minimum signed section X/Z (`-2^21 = -2,097,152`).
pub const MIN_PACKED_SECTION_XZ: i32 = -(1 << 21);

/// Maximum signed section Y (`2^19 - 1 = 524,287`).
pub const MAX_PACKED_SECTION_Y: i32 = (1 << 19) - 1;

/// Minimum signed section Y (`-2^19 = -524,288`).
pub const MIN_PACKED_SECTION_Y: i32 = -(1 << 19);

/// `SectionPos.asLong(x, y, z)`: packs X (22b), Z (22b), Y (20b) into a 64-bit integer.
#[inline]
pub const fn section_as_long(x: i32, y: i32, z: i32) -> i64 {
    ((x as i64 & SECTION_PACKED_XZ_MASK) << 42)
        | ((z as i64 & SECTION_PACKED_XZ_MASK) << 20)
        | (y as i64 & SECTION_PACKED_Y_MASK)
}

/// `SectionPos.x(long)`: sign-extended 22-bit X coordinate.
#[inline]
pub const fn section_get_x(packed: i64) -> i32 {
    (packed >> 42) as i32
}

/// `SectionPos.z(long)`: sign-extended 22-bit Z coordinate.
#[inline]
pub const fn section_get_z(packed: i64) -> i32 {
    ((packed << 22) >> 42) as i32
}

/// `SectionPos.y(long)`: sign-extended 20-bit Y coordinate.
#[inline]
pub const fn section_get_y(packed: i64) -> i32 {
    ((packed << 44) >> 44) as i32
}

/// `SectionPos.getZeroNode(long)`: zeroes the 20-bit Y field (`packed & -1048576L`).
#[inline]
pub const fn section_get_zero_node(packed: i64) -> i64 {
    packed & -1_048_576_i64
}

/// `SectionPos.blockToSectionCoord(int)`: arithmetic shift `b >> 4`.
#[inline]
pub const fn block_to_section_coord(block: i32) -> i32 {
    block >> 4
}

/// `SectionPos.sectionRelative(int)`: `b & 15`.
#[inline]
pub const fn section_relative(block: i32) -> i32 {
    block & 15
}

/// `SectionPos.sectionToBlockCoord(int)`: `s << 4`.
#[inline]
pub const fn section_to_block_coord(section: i32) -> i32 {
    section << 4
}

/// `SectionPos.sectionRelativePos(x, y, z) -> short`: 12-bit XZY relative block index.
#[inline]
pub const fn section_relative_pos(x: i32, y: i32, z: i32) -> i16 {
    (((x & 15) << 8) | ((z & 15) << 4) | (y & 15)) as i16
}

/// `SectionPos.sectionRelativeX(short)`: `(r >>> 8) & 15`.
#[inline]
pub const fn section_relative_x(rel: i16) -> i32 {
    ushr_i32(rel as i32, 8) & 15
}

/// `SectionPos.sectionRelativeZ(short)`: `(r >>> 4) & 15`.
#[inline]
pub const fn section_relative_z(rel: i16) -> i32 {
    ushr_i32(rel as i32, 4) & 15
}

/// `SectionPos.sectionRelativeY(short)`: `r & 15`.
#[inline]
pub const fn section_relative_y(rel: i16) -> i32 {
    (rel as i32) & 15
}

// ---------------------------------------------------------------------------
// ChunkPos 64-bit packing and region conversions
// ---------------------------------------------------------------------------

/// `ChunkPos.INVALID_CHUNK_POS = pack(1875066, 1875066)` (`0x001C9C7A001C9C7A`).
pub const INVALID_CHUNK_POS: i64 = chunk_pack(1_875_066, 1_875_066);

/// `ChunkPos.asLong(x, z)` / `ChunkPos.pack(x, z)`: X in low 32 bits, Z in high 32 bits.
#[inline]
pub const fn chunk_pack(x: i32, z: i32) -> i64 {
    (x as i64 & 0xFFFF_FFFF) | ((z as i64 & 0xFFFF_FFFF) << 32)
}

/// `ChunkPos.getX(long)`: low 32 bits as signed `i32`.
#[inline]
pub const fn chunk_get_x(packed: i64) -> i32 {
    packed as i32
}

/// `ChunkPos.getZ(long)`: high 32 bits as signed `i32`.
#[inline]
pub const fn chunk_get_z(packed: i64) -> i32 {
    (packed >> 32) as i32
}

/// `ChunkPos.getRegionX/Z`: arithmetic shift `chunk >> 5`.
#[inline]
pub const fn chunk_to_region(chunk: i32) -> i32 {
    chunk >> 5
}

/// `ChunkPos.getRegionLocalX/Z`: `chunk & 31`.
#[inline]
pub const fn chunk_region_local(chunk: i32) -> i32 {
    chunk & 31
}

/// Minimum chunk coordinate in a 32x32 region: `region << 5`.
#[inline]
pub const fn region_to_min_chunk(region: i32) -> i32 {
    region << 5
}

// ---------------------------------------------------------------------------
// QuartPos (4-block biome quart shifts)
// ---------------------------------------------------------------------------

/// `QuartPos.fromBlock(int)`: `b >> 2`.
#[inline]
pub const fn quart_from_block(block: i32) -> i32 {
    block >> 2
}

/// `QuartPos.quartLocal(int)`: `b & 3`.
#[inline]
pub const fn quart_local(block: i32) -> i32 {
    block & 3
}

/// `QuartPos.toBlock(int)`: `q << 2`.
#[inline]
pub const fn quart_to_block(quart: i32) -> i32 {
    quart << 2
}

/// `QuartPos.fromSection(int)`: `s << 2`.
#[inline]
pub const fn quart_from_section(section: i32) -> i32 {
    section << 2
}

/// `QuartPos.toSection(int)`: `q >> 2`.
#[inline]
pub const fn quart_to_section(quart: i32) -> i32 {
    quart >> 2
}
