use crate::{Box2, Point2};
use std::collections::HashMap;

pub type SpatialId = u64;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpatialEntry {
    pub id: SpatialId,
    pub box_value: Box2,
}
#[derive(Clone, Debug)]
pub struct SpatialIndex {
    cell_size: f64,
    cells: HashMap<u64, Vec<SpatialId>>,
    boxes: HashMap<SpatialId, Box2>,
    oversized: Vec<SpatialId>,
    bounds: Box2,
}
impl Default for SpatialIndex {
    fn default() -> Self {
        Self {
            cell_size: 1.0,
            cells: HashMap::new(),
            boxes: HashMap::new(),
            oversized: Vec::new(),
            bounds: Box2::default(),
        }
    }
}
const MAXIMUM_CELLS_PER_BOX: i64 = 1024;
fn usable(bounds: Box2) -> bool {
    !bounds.empty()
        && bounds.min.x.is_finite()
        && bounds.min.y.is_finite()
        && bounds.max.x.is_finite()
        && bounds.max.y.is_finite()
}
fn key_of(east: i32, north: i32) -> u64 {
    ((east as u32 as u64) << 32) | north as u32 as u64
}
impl SpatialIndex {
    fn cell_of(&self, coordinate: f64) -> i32 {
        if !coordinate.is_finite() {
            return 0;
        }
        (coordinate / self.cell_size).floor().clamp(-2.0e9, 2.0e9) as i32
    }
    fn cell_range(&self, bounds: Box2) -> (i32, i32, i32, i32, bool) {
        let east_min = self.cell_of(bounds.min.x);
        let north_min = self.cell_of(bounds.min.y);
        let east_max = self.cell_of(bounds.max.x);
        let north_max = self.cell_of(bounds.max.y);
        let columns = east_max as i64 - east_min as i64 + 1;
        let rows = north_max as i64 - north_min as i64 + 1;
        (
            east_min,
            north_min,
            east_max,
            north_max,
            columns.wrapping_mul(rows) <= MAXIMUM_CELLS_PER_BOX,
        )
    }
    pub fn clear(&mut self) {
        for ids in self.cells.values_mut() {
            ids.clear();
        }
        self.boxes.clear();
        self.oversized.clear();
        self.bounds = Box2::default();
    }
    pub fn contains(&self, id: SpatialId) -> bool {
        self.boxes.contains_key(&id)
    }
    pub fn size(&self) -> usize {
        self.boxes.len()
    }
    pub fn empty(&self) -> bool {
        self.boxes.is_empty()
    }
    pub fn cell_size(&self) -> f64 {
        self.cell_size
    }
    pub fn oversized_count(&self) -> usize {
        self.oversized.len()
    }
    pub fn bucket_count(&self) -> usize {
        self.cells.len()
    }
    pub fn bounds(&self) -> Box2 {
        self.bounds
    }
    fn insert_into_cells(&mut self, id: SpatialId, bounds: Box2) {
        let (east_min, north_min, east_max, north_max, fits) = self.cell_range(bounds);
        if !fits {
            self.oversized.push(id);
            return;
        }
        for north in north_min..=north_max {
            for east in east_min..=east_max {
                self.cells.entry(key_of(east, north)).or_default().push(id);
            }
        }
    }
    fn remove_from_cells(&mut self, id: SpatialId, bounds: Box2) {
        let (east_min, north_min, east_max, north_max, fits) = self.cell_range(bounds);
        if !fits {
            if let Some(found) = self.oversized.iter().position(|&entry| entry == id) {
                self.oversized.remove(found);
            }
            return;
        }
        for north in north_min..=north_max {
            for east in east_min..=east_max {
                if let Some(ids) = self.cells.get_mut(&key_of(east, north))
                    && let Some(found) = ids.iter().position(|&entry| entry == id)
                {
                    ids.swap_remove(found);
                }
            }
        }
    }
    pub fn insert(&mut self, id: SpatialId, bounds: Box2) {
        if let Some(&existing) = self.boxes.get(&id) {
            if existing == bounds {
                return;
            }
            self.remove_from_cells(id, existing);
            self.boxes.remove(&id);
        }
        if !usable(bounds) {
            return;
        }
        self.boxes.insert(id, bounds);
        self.insert_into_cells(id, bounds);
        self.bounds.expand_box(bounds);
    }
    pub fn remove(&mut self, id: SpatialId) -> bool {
        let Some(&found) = self.boxes.get(&id) else {
            return false;
        };
        self.remove_from_cells(id, found);
        self.boxes.remove(&id);
        true
    }
    fn choose_cell_size(&mut self, entries: &[SpatialEntry]) {
        let mut total = 0.0;
        let mut counted = 0;
        for entry in entries {
            if !usable(entry.box_value) {
                continue;
            }
            total += entry.box_value.width() + entry.box_value.height();
            counted += 1;
        }
        if counted == 0 {
            self.cell_size = 1.0;
            return;
        }
        let mean = total / (2.0 * counted as f64);
        self.cell_size = if mean > 1.0e-9 { mean * 2.0 } else { 1.0 };
    }
    pub fn rebuild(&mut self, entries: &[SpatialEntry]) {
        self.cells.clear();
        self.boxes.clear();
        self.oversized.clear();
        self.bounds = Box2::default();
        self.choose_cell_size(entries);
        self.boxes.reserve(entries.len());
        self.cells.reserve(entries.len());
        for entry in entries {
            if !usable(entry.box_value) {
                continue;
            }
            self.boxes.entry(entry.id).or_insert(entry.box_value);
            self.insert_into_cells(entry.id, entry.box_value);
            self.bounds.expand_box(entry.box_value);
        }
    }
    pub fn query_into(&self, bounds: Box2, output: &mut Vec<SpatialId>) {
        output.clear();
        if !usable(bounds) || self.boxes.is_empty() {
            return;
        }
        let (east_min, north_min, east_max, north_max, fits) = self.cell_range(bounds);
        if !fits {
            output.reserve(self.boxes.len());
            for (&id, stored) in &self.boxes {
                if stored.intersects(bounds) {
                    output.push(id);
                }
            }
        } else {
            for north in north_min..=north_max {
                for east in east_min..=east_max {
                    if let Some(ids) = self.cells.get(&key_of(east, north)) {
                        for &id in ids {
                            if self
                                .boxes
                                .get(&id)
                                .is_some_and(|stored| stored.intersects(bounds))
                            {
                                output.push(id);
                            }
                        }
                    }
                }
            }
            for &id in &self.oversized {
                if self
                    .boxes
                    .get(&id)
                    .is_some_and(|stored| stored.intersects(bounds))
                {
                    output.push(id);
                }
            }
        }
        output.sort();
        output.dedup();
    }
    pub fn query_radius(&self, point: Point2, radius: f64, output: &mut Vec<SpatialId>) {
        if !radius.is_finite() || radius < 0.0 {
            output.clear();
            return;
        }
        self.query_into(
            Box2::new(
                Point2::new(point.x - radius, point.y - radius),
                Point2::new(point.x + radius, point.y + radius),
            ),
            output,
        );
    }
    pub fn query(&self, bounds: Box2) -> Vec<SpatialId> {
        let mut output = Vec::new();
        self.query_into(bounds, &mut output);
        output
    }
}
