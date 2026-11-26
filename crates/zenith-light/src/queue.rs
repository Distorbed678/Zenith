//! Queue entries: the 12-bit encoding of one logical `(blockNode, entryData)` pair (`light-engine.md` Inputs C).

use std::collections::VecDeque;

use zenith_blocks::Direction;

/// Bit of a "propagate in direction" flag: `4 + ordinal(d)`.
#[inline]
pub const fn dir_bit(d: Direction) -> u16 {
    1 << (4 + d.ordinal())
}

/// All six direction flags.
pub const ALL_DIRECTIONS: u16 = 1008;
/// `fromEmptyShape` flag.
pub const FROM_EMPTY_SHAPE: u16 = 1024;
/// `increaseFromEmission` flag.
pub const INCREASE_FROM_EMISSION: u16 = 2048;

/// `PULL_LIGHT_IN_ENTRY = decreaseAllDirections(1)`.
pub const PULL_LIGHT_IN_ENTRY: u16 = ALL_DIRECTIONS | 1;
/// Sky: `decreaseAllDirections(15)`.
pub const REMOVE_TOP_SKY_SOURCE_ENTRY: u16 = ALL_DIRECTIONS | 15;
/// Sky: `decreaseSkipOneDirection(15, UP)`.
pub const REMOVE_SKY_SOURCE_ENTRY: u16 = (ALL_DIRECTIONS - 32) | 15;
/// Sky: `increaseSkipOneDirection(15, false, UP)`.
pub const ADD_SKY_SOURCE_ENTRY: u16 = (ALL_DIRECTIONS - 32) | 15;

/// The level stored in an entry.
#[inline]
pub const fn level(data: u16) -> u8 {
    (data & 15) as u8
}

/// Whether an entry propagates in direction `d`.
#[inline]
pub const fn has_direction(data: u16, d: Direction) -> bool {
    data & dir_bit(d) != 0
}

/// `decreaseAllDirections(l)`.
pub const fn decrease_all_directions(l: u8) -> u16 {
    ALL_DIRECTIONS | (l & 15) as u16
}

/// `decreaseSkipOneDirection(l, d)`.
pub const fn decrease_skip_one_direction(l: u8, d: Direction) -> u16 {
    (ALL_DIRECTIONS - dir_bit(d)) | (l & 15) as u16
}

/// `increaseLightFromEmission(l, fromEmpty)`.
pub const fn increase_light_from_emission(l: u8, from_empty: bool) -> u16 {
    ALL_DIRECTIONS
        | INCREASE_FROM_EMISSION
        | if from_empty { FROM_EMPTY_SHAPE } else { 0 }
        | (l & 15) as u16
}

/// `increaseSkipOneDirection(l, fromEmpty, d)`.
pub const fn increase_skip_one_direction(l: u8, from_empty: bool, d: Direction) -> u16 {
    (ALL_DIRECTIONS - dir_bit(d)) | if from_empty { FROM_EMPTY_SHAPE } else { 0 } | (l & 15) as u16
}

/// `increaseOnlyOneDirection(l, fromEmpty, d)`.
pub const fn increase_only_one_direction(l: u8, from_empty: bool, d: Direction) -> u16 {
    dir_bit(d) | if from_empty { FROM_EMPTY_SHAPE } else { 0 } | (l & 15) as u16
}

/// `increaseSkySourceInDirections(down, north, south, west, east)`.
pub const fn increase_sky_source_in_directions(
    down: bool,
    north: bool,
    south: bool,
    west: bool,
    east: bool,
) -> u16 {
    15 | if down { dir_bit(Direction::Down) } else { 0 }
        | if north { dir_bit(Direction::North) } else { 0 }
        | if south { dir_bit(Direction::South) } else { 0 }
        | if west { dir_bit(Direction::West) } else { 0 }
        | if east { dir_bit(Direction::East) } else { 0 }
}

/// Which queue a traced entry went into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueKind {
    /// The decrease queue.
    Decrease,
    /// The increase queue.
    Increase,
}

/// One enqueued logical entry, in enqueue order (for parity traces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEntry {
    /// The queue.
    pub kind: QueueKind,
    /// The block node.
    pub node: i64,
    /// The entry data.
    pub data: u16,
}

/// The two FIFO queues plus the optional enqueue trace.
#[derive(Debug, Default, Clone)]
pub struct Queues {
    decrease: VecDeque<(i64, u16)>,
    increase: VecDeque<(i64, u16)>,
    trace: Option<Vec<TraceEntry>>,
}

impl Queues {
    /// Enqueues a decrease entry.
    #[inline]
    pub fn enqueue_decrease(&mut self, node: i64, data: u16) {
        self.decrease.push_back((node, data));
        if let Some(t) = &mut self.trace {
            t.push(TraceEntry {
                kind: QueueKind::Decrease,
                node,
                data,
            });
        }
    }

    /// Enqueues an increase entry.
    #[inline]
    pub fn enqueue_increase(&mut self, node: i64, data: u16) {
        self.increase.push_back((node, data));
        if let Some(t) = &mut self.trace {
            t.push(TraceEntry {
                kind: QueueKind::Increase,
                node,
                data,
            });
        }
    }

    /// Dequeues the next decrease entry.
    #[inline]
    pub fn pop_decrease(&mut self) -> Option<(i64, u16)> {
        self.decrease.pop_front()
    }

    /// Dequeues the next increase entry.
    #[inline]
    pub fn pop_increase(&mut self) -> Option<(i64, u16)> {
        self.increase.pop_front()
    }

    /// Whether either queue holds entries.
    pub fn is_pending(&self) -> bool {
        !self.decrease.is_empty() || !self.increase.is_empty()
    }

    /// Starts recording every enqueue.
    pub fn start_trace(&mut self) {
        self.trace = Some(Vec::new());
    }

    /// Returns and clears the recorded enqueues (tracing stays on).
    pub fn take_trace(&mut self) -> Vec<TraceEntry> {
        self.trace.as_mut().map(std::mem::take).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_constants() {
        assert_eq!(PULL_LIGHT_IN_ENTRY, 1009);
        assert_eq!(REMOVE_TOP_SKY_SOURCE_ENTRY, 1023);
        assert_eq!(REMOVE_SKY_SOURCE_ENTRY, 991);
        assert_eq!(ADD_SKY_SOURCE_ENTRY, 991);
        assert_eq!(increase_light_from_emission(14, true), 4094);
        assert_eq!(increase_light_from_emission(15, true), 4095);
        assert_eq!(dir_bit(Direction::Down), 16);
        assert_eq!(dir_bit(Direction::East), 512);
    }

    #[test]
    fn skip_and_only_one() {
        assert_eq!(decrease_skip_one_direction(7, Direction::Up), 1008 - 32 + 7);
        assert_eq!(
            increase_only_one_direction(3, false, Direction::North),
            64 | 3
        );
        assert_eq!(
            increase_skip_one_direction(9, true, Direction::West),
            (1008 - 256) | 1024 | 9
        );
        assert_eq!(
            increase_sky_source_in_directions(true, false, true, false, false),
            15 | 16 | 128
        );
    }
}
