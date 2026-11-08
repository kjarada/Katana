//! Merging of coincident plan positions.
//!
//! PointMerger hands out one index per distinct position: a position within
//! `tolerance` (Euclidean, inclusive) of an already kept position gets that
//! position's index, otherwise it is kept and gets the next index. When several
//! kept positions qualify the earliest wins, so the result depends only on the
//! order of the add() calls. Coincidence is not transitive; "compare against the
//! kept positions" is the rule that makes the outcome well defined.
//!
//! Implementation: open-addressing hash grid sized once for the expected number
//! of positions (8 bytes per position, no per-position allocation). Expected O(1)
//! per add().

use katana_geometry::Point2;
use katana_math::cpp;
use katana_math::numerics::tolerance;

const EMPTY_SLOT: u32 = 0xFFFF_FFFF;

/// Cells are this many tolerances wide. A position then lies within reach of a
/// neighbouring cell only rarely, so most add() calls probe one cell instead of
/// nine. A performance parameter: it has no influence on the result.
const CELL_WIDTH_IN_TOLERANCES: f64 = 1024.0;

/// Cell coordinates are clamped to this magnitude so that the conversion to
/// i64 and the +-1 neighbour step cannot overflow. Positions beyond it share
/// cells, which costs speed but not correctness: the distance test decides.
const MAX_CELL_COORDINATE: f64 = 4.0e18;

fn cell_coordinate(scaled: f64) -> i64 {
    cpp::clamp(scaled.floor(), -MAX_CELL_COORDINATE, MAX_CELL_COORDINATE) as i64
}

fn table_size_for(capacity: usize) -> usize {
    let mut size = 16;
    while size < 2 * capacity {
        size *= 2;
    }
    size
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Entry {
    pub index: u32,
    /// false: merged into an earlier position
    pub inserted: bool,
}

#[derive(Clone, Copy)]
struct Cell {
    x: i64,
    y: i64,
}

pub(crate) struct PointMerger {
    tolerance: f64,
    cell_size: f64,
    mask: usize,
    slots: Vec<u32>,
    positions: Vec<Point2>,
}

impl PointMerger {
    /// `capacity` is an upper bound on the number of add() calls. `tolerance` is
    /// in model units; 0 merges exactly equal positions only.
    pub fn new(capacity: usize, tolerance: f64) -> Self {
        let kept_tolerance = cpp::max(tolerance, 0.0);
        let mask = table_size_for(capacity) - 1;
        Self {
            tolerance: kept_tolerance,
            cell_size: CELL_WIDTH_IN_TOLERANCES * cpp::max(kept_tolerance, tolerance::GEOMETRIC),
            mask,
            slots: vec![EMPTY_SLOT; mask + 1],
            positions: Vec::with_capacity(capacity),
        }
    }

    pub fn positions(&self) -> &Vec<Point2> {
        &self.positions
    }

    fn slot_of(&self, cell: Cell) -> usize {
        // splitmix64 finaliser over the combined cell coordinates.
        let mut h = (cell.x as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(cell.y as u64);
        h ^= h >> 30;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 27;
        h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
        h ^= h >> 31;
        (h as usize) & self.mask
    }

    /// Earliest kept position within tolerance found in the probe sequence of `cell`.
    fn find_near(&self, cell: Cell, position: Point2) -> u32 {
        let mut best = EMPTY_SLOT;
        let mut slot = self.slot_of(cell);
        while self.slots[slot] != EMPTY_SLOT {
            let candidate = self.slots[slot];
            let kept = self.positions[candidate as usize];
            // Cheap rejection before the hypot in distance_to().
            let far = (kept.x - position.x).abs() > self.tolerance
                || (kept.y - position.y).abs() > self.tolerance;
            if !far && kept.distance_to(position) <= self.tolerance {
                best = best.min(candidate);
            }
            slot = (slot + 1) & self.mask;
        }
        best
    }

    pub fn add(&mut self, position: Point2) -> Entry {
        let scaled_x = position.x / self.cell_size;
        let scaled_y = position.y / self.cell_size;
        let home = Cell {
            x: cell_coordinate(scaled_x),
            y: cell_coordinate(scaled_y),
        };

        let mut found = self.find_near(home, position);
        if self.tolerance > 0.0 {
            // A neighbouring cell can hold a match only when the position is within
            // `tolerance` of the shared border; twice that is tested to absorb the
            // rounding of the scaled coordinates.
            let reach = 2.0 * self.tolerance / self.cell_size;
            let fraction_x = scaled_x - scaled_x.floor();
            let fraction_y = scaled_y - scaled_y.floor();
            let low_x = fraction_x <= reach;
            let high_x = 1.0 - fraction_x <= reach;
            let low_y = fraction_y <= reach;
            let high_y = 1.0 - fraction_y <= reach;
            for dy in -1i64..=1 {
                if (dy < 0 && !low_y) || (dy > 0 && !high_y) {
                    continue;
                }
                for dx in -1i64..=1 {
                    if (dx == 0 && dy == 0) || (dx < 0 && !low_x) || (dx > 0 && !high_x) {
                        continue;
                    }
                    found = found.min(self.find_near(
                        Cell {
                            x: home.x + dx,
                            y: home.y + dy,
                        },
                        position,
                    ));
                }
            }
        }
        if found != EMPTY_SLOT {
            return Entry {
                index: found,
                inserted: false,
            };
        }

        // Keep the load factor at or below one half even if the caller's capacity
        // estimate was too low.
        if 2 * (self.positions.len() + 1) > self.slots.len() {
            self.mask = 2 * self.slots.len() - 1;
            self.slots = vec![EMPTY_SLOT; self.mask + 1];
            for i in 0..self.positions.len() {
                let cell = Cell {
                    x: cell_coordinate(self.positions[i].x / self.cell_size),
                    y: cell_coordinate(self.positions[i].y / self.cell_size),
                };
                let mut slot = self.slot_of(cell);
                while self.slots[slot] != EMPTY_SLOT {
                    slot = (slot + 1) & self.mask;
                }
                self.slots[slot] = i as u32;
            }
        }

        let index = self.positions.len() as u32;
        self.positions.push(position);
        let mut slot = self.slot_of(home);
        while self.slots[slot] != EMPTY_SLOT {
            slot = (slot + 1) & self.mask;
        }
        self.slots[slot] = index;
        Entry {
            index,
            inserted: true,
        }
    }
}
