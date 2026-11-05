use crate::scalar::max;
use crate::{Box2, Point2};
use katana_core::task_pool::TaskPool;
use std::sync::OnceLock;

pub const SPLAT_TILE_POINTS: usize = 1024;
pub const SPLAT_POINT_BUDGET: usize = 4_000_000;
pub const OFF_IMAGE: i32 = i32::MIN;
const PROJECT_MINIMUM: usize = 4;
const BLOCK: usize = 512;
const PARALLEL_MINIMUM: usize = 32_768;
const MINIMUM_BAND_ROWS: i32 = 16;
const CHUNK: usize = 1 << 16;
const CHUNKS: usize = 64;
const MAXIMUM_LEVEL: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplatTile {
    pub begin: u32,
    pub count: u32,
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SplatCloud {
    pub origin: Point2,
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub colours: Vec<u32>,
    pub tiles: Vec<SplatTile>,
    pub source_count: usize,
}
impl SplatCloud {
    pub fn size(&self) -> usize {
        self.xs.len()
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplatView {
    pub centre: Point2,
    pub scale: f64,
    pub width: i32,
    pub height: i32,
    pub radius: i32,
    pub point_budget: usize,
}
impl Default for SplatView {
    fn default() -> Self {
        Self {
            centre: Point2::default(),
            scale: 1.0,
            width: 0,
            height: 0,
            radius: 0,
            point_budget: SPLAT_POINT_BUDGET,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplatStats {
    pub points_in_view: usize,
    pub points_drawn: usize,
    pub row_begin: i32,
    pub row_end: i32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelProjection {
    pub half_width: f64,
    pub half_height: f64,
    pub centre_x: f64,
    pub centre_y: f64,
    pub scale: f64,
}
impl Default for PixelProjection {
    fn default() -> Self {
        Self {
            half_width: 0.0,
            half_height: 0.0,
            centre_x: 0.0,
            centre_y: 0.0,
            scale: 1.0,
        }
    }
}
impl PixelProjection {
    fn data(&self) -> [f64; 5] {
        [
            self.half_width,
            self.half_height,
            self.centre_x,
            self.centre_y,
            self.scale,
        ]
    }
}
fn project(projection: &[f64; 5], xs: &[f32], ys: &[f32], px: &mut [i32], py: &mut [i32]) {
    if xs.len() >= PROJECT_MINIMUM && crate::simd::project_to_pixels(projection, xs, ys, px, py) {
        return;
    }
    crate::simd::project_scalar(projection, xs, ys, px, py);
}
pub fn project_to_pixels(
    projection: &PixelProjection,
    xs: &[f32],
    ys: &[f32],
    px: &mut [i32],
    py: &mut [i32],
) {
    let count = xs.len().min(ys.len()).min(px.len()).min(py.len());
    project(
        &projection.data(),
        &xs[..count],
        &ys[..count],
        &mut px[..count],
        &mut py[..count],
    );
}
fn map_chunks<Value: Send + Sync>(
    pool: &TaskPool,
    count: usize,
    body: impl Fn(usize) -> Value + Sync,
) -> Vec<Value> {
    let slots: Vec<OnceLock<Value>> = (0..count).map(|_| OnceLock::new()).collect();
    pool.parallel_for(0, count, |index| {
        assert!(slots[index].set(body(index)).is_ok());
    });
    slots
        .into_iter()
        .map(|slot| slot.into_inner().expect("every chunk was visited"))
        .collect()
}
fn grid_index(offset: f64, extent: f64, cells: i32) -> i32 {
    let parameter = if extent > 0.0 { offset / extent } else { 0.0 };
    ((parameter * cells as f64) as i32).clamp(0, cells - 1)
}
fn order_by_level(
    xs: &[f32],
    ys: &[f32],
    origin_x: f64,
    origin_y: f64,
    width: f64,
    height: f64,
) -> Vec<u32> {
    let count = xs.len();
    let mut order: Vec<u32> = (0..count).map(|index| index as u32).collect();
    if count <= 1 {
        return order;
    }
    let mut deepest = 0;
    while deepest < MAXIMUM_LEVEL && (1_usize << (2 * deepest)) < count {
        deepest += 1;
    }
    let cells = 1_i32 << deepest;
    let cell_x: Vec<u32> = xs
        .iter()
        .map(|&offset| grid_index(offset as f64 - origin_x, width, cells) as u32)
        .collect();
    let cell_y: Vec<u32> = ys
        .iter()
        .map(|&offset| grid_index(offset as f64 - origin_y, height, cells) as u32)
        .collect();
    let mut levels = vec![(deepest + 1) as u8; count];
    let mut taken = Vec::new();
    let mut left = count;
    for level in 0..=deepest {
        if left == 0 {
            break;
        }
        let shift = deepest - level;
        taken.clear();
        taken.resize(1_usize << (2 * level), 0_u8);
        for index in 0..count {
            if levels[index] as usize <= deepest {
                continue;
            }
            let cell = &mut taken[(((cell_y[index] >> shift) as usize) << level)
                + (cell_x[index] >> shift) as usize];
            if *cell == 0 {
                *cell = 1;
                levels[index] = level as u8;
                left -= 1;
            }
        }
    }
    let mut start = [0_u32; MAXIMUM_LEVEL + 3];
    for &level in &levels {
        start[level as usize + 1] += 1;
    }
    for level in 1..start.len() {
        start[level] += start[level - 1];
    }
    for (index, &level) in levels.iter().enumerate() {
        order[start[level as usize] as usize] = index as u32;
        start[level as usize] += 1;
    }
    order
}
#[derive(Clone, Copy)]
struct FloatBox {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    points: usize,
}
impl Default for FloatBox {
    fn default() -> Self {
        Self {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
            points: 0,
        }
    }
}
impl FloatBox {
    fn expand(&mut self, east: f32, north: f32) {
        if east < self.min_x {
            self.min_x = east;
        }
        if north < self.min_y {
            self.min_y = north;
        }
        if self.max_x < east {
            self.max_x = east;
        }
        if self.max_y < north {
            self.max_y = north;
        }
        self.points += 1;
    }
    fn merge(&mut self, other: Self) {
        if other.min_x < self.min_x {
            self.min_x = other.min_x;
        }
        if other.min_y < self.min_y {
            self.min_y = other.min_y;
        }
        if self.max_x < other.max_x {
            self.max_x = other.max_x;
        }
        if self.max_y < other.max_y {
            self.max_y = other.max_y;
        }
        self.points += other.points;
    }
}
struct FloatChunk {
    xs: Vec<f32>,
    ys: Vec<f32>,
    bounds: FloatBox,
}
struct CountChunk {
    tile_of_point: Vec<u32>,
    counts: Vec<u32>,
}
struct TileOutput {
    tile: SplatTile,
    xs: Vec<f32>,
    ys: Vec<f32>,
    colours: Vec<u32>,
}

pub fn build_splat_cloud(
    xs: &[u8],
    ys: &[u8],
    stride_bytes: usize,
    count: usize,
    colours: &[u32],
    pool: &TaskPool,
) -> SplatCloud {
    let required = if count == 0 {
        0
    } else {
        (count - 1)
            .checked_mul(stride_bytes)
            .and_then(|offset| offset.checked_add(8))
            .expect("strided source size overflow")
    };
    assert!(xs.len() >= required && ys.len() >= required && colours.len() >= count);
    let coordinate = |bytes: &[u8], index: usize| {
        let offset = index * stride_bytes;
        f64::from_ne_bytes(bytes[offset..offset + 8].try_into().unwrap())
    };
    let mut cloud = SplatCloud {
        source_count: count,
        ..Default::default()
    };
    let chunk_size = CHUNK.max(count.div_ceil(CHUNKS));
    let chunks = count.div_ceil(chunk_size);
    let chunk_bounds = map_chunks(pool, chunks, |chunk| {
        let mut bounds = Box2::default();
        let low = chunk * chunk_size;
        let high = count.min((chunk + 1) * chunk_size);
        for index in low..high {
            let east = coordinate(xs, index);
            let north = coordinate(ys, index);
            if east.is_finite() && north.is_finite() {
                bounds.expand(Point2::new(east, north));
            }
        }
        bounds
    });
    let mut bounds = Box2::default();
    for chunk in chunk_bounds {
        if !chunk.empty() {
            bounds.expand(chunk.min);
            bounds.expand(chunk.max);
        }
    }
    if bounds.empty() {
        return cloud;
    }
    cloud.origin = Point2::new(
        0.5 * (bounds.min.x + bounds.max.x),
        0.5 * (bounds.min.y + bounds.max.y),
    );
    let float_chunks = map_chunks(pool, chunks, |chunk| {
        let low = chunk * chunk_size;
        let high = count.min((chunk + 1) * chunk_size);
        let mut result = FloatChunk {
            xs: Vec::with_capacity(high - low),
            ys: Vec::with_capacity(high - low),
            bounds: FloatBox::default(),
        };
        for index in low..high {
            let east = (coordinate(xs, index) - cloud.origin.x) as f32;
            let north = (coordinate(ys, index) - cloud.origin.y) as f32;
            if !east.is_finite() || !north.is_finite() {
                result.xs.push(f32::NAN);
                result.ys.push(f32::NAN);
                continue;
            }
            result.xs.push(east);
            result.ys.push(north);
            result.bounds.expand(east, north);
        }
        result
    });
    let mut all = FloatBox::default();
    let mut float_xs = Vec::with_capacity(count);
    let mut float_ys = Vec::with_capacity(count);
    for chunk in float_chunks {
        all.merge(chunk.bounds);
        float_xs.extend(chunk.xs);
        float_ys.extend(chunk.ys);
    }
    let drawable = all.points;
    if drawable == 0 {
        return cloud;
    }
    let min_x = all.min_x;
    let min_y = all.min_y;
    let width = all.max_x as f64 - min_x as f64;
    let height = all.max_y as f64 - min_y as f64;
    let wanted = max(1.0, drawable as f64 / SPLAT_TILE_POINTS as f64);
    let mut columns = 1;
    let mut rows = 1;
    if width > 0.0 && height > 0.0 {
        columns = ((wanted * width / height).sqrt().round() as i32).clamp(1, 4096);
        rows = ((wanted / columns as f64).round() as i32).clamp(1, 4096);
    } else if width > 0.0 {
        columns = (wanted.round() as i32).clamp(1, 4096);
    } else if height > 0.0 {
        rows = (wanted.round() as i32).clamp(1, 4096);
    }
    let tile_width = width / columns as f64;
    let tile_height = height / rows as f64;
    let tile_count = columns as usize * rows as usize;
    let mut count_chunks = map_chunks(pool, chunks, |chunk| {
        let low = chunk * chunk_size;
        let high = count.min((chunk + 1) * chunk_size);
        let mut result = CountChunk {
            tile_of_point: Vec::with_capacity(high - low),
            counts: vec![0; tile_count],
        };
        for index in low..high {
            if float_xs[index].is_nan() {
                result.tile_of_point.push(u32::MAX);
                continue;
            }
            let column = grid_index(float_xs[index] as f64 - min_x as f64, width, columns);
            let row = rows - 1 - grid_index(float_ys[index] as f64 - min_y as f64, height, rows);
            let tile = (row * columns + column) as u32;
            result.tile_of_point.push(tile);
            result.counts[tile as usize] += 1;
        }
        result
    });
    let mut start = vec![0_u32; tile_count + 1];
    let mut at = 0_u32;
    for (tile, tile_start) in start.iter_mut().enumerate().take(tile_count) {
        *tile_start = at;
        for chunk in &mut count_chunks {
            let number = chunk.counts[tile];
            chunk.counts[tile] = at;
            at = at.wrapping_add(number);
        }
    }
    start[tile_count] = at;
    let gathered = map_chunks(pool, chunks, |chunk| {
        let low = chunk * chunk_size;
        let mut cursors = count_chunks[chunk].counts.clone();
        let mut output = Vec::new();
        for (local, &tile) in count_chunks[chunk].tile_of_point.iter().enumerate() {
            if tile == u32::MAX {
                continue;
            }
            let index = low + local;
            let at = cursors[tile as usize];
            cursors[tile as usize] += 1;
            output.push((at as usize, float_xs[index], float_ys[index], index as u32));
        }
        output
    });
    let mut gathered_xs = vec![0.0_f32; drawable];
    let mut gathered_ys = vec![0.0_f32; drawable];
    let mut gathered_source = vec![0_u32; drawable];
    for chunk in gathered {
        for (at, east, north, source) in chunk {
            gathered_xs[at] = east;
            gathered_ys[at] = north;
            gathered_source[at] = source;
        }
    }
    let tiles: Vec<OnceLock<TileOutput>> = (0..tile_count).map(|_| OnceLock::new()).collect();
    pool.parallel_ranges(0, tile_count, 16, |first_tile, last_tile| {
        for tile_index in first_tile..last_tile {
            let begin = start[tile_index];
            let end = start[tile_index + 1];
            let count = end - begin;
            let mut output = TileOutput {
                tile: SplatTile {
                    begin,
                    count,
                    ..Default::default()
                },
                xs: Vec::with_capacity(count as usize),
                ys: Vec::with_capacity(count as usize),
                colours: Vec::with_capacity(count as usize),
            };
            if begin != end {
                let row = (tile_index / columns as usize) as i32;
                let column = (tile_index % columns as usize) as i32;
                let order = order_by_level(
                    &gathered_xs[begin as usize..end as usize],
                    &gathered_ys[begin as usize..end as usize],
                    min_x as f64 + column as f64 * tile_width,
                    min_y as f64 + (rows - 1 - row) as f64 * tile_height,
                    tile_width,
                    tile_height,
                );
                let mut tile_bounds = FloatBox::default();
                for offset in order {
                    let from = (begin + offset) as usize;
                    let east = gathered_xs[from];
                    let north = gathered_ys[from];
                    output.xs.push(east);
                    output.ys.push(north);
                    output.colours.push(colours[gathered_source[from] as usize]);
                    tile_bounds.expand(east, north);
                }
                output.tile.min_x = tile_bounds.min_x;
                output.tile.min_y = tile_bounds.min_y;
                output.tile.max_x = tile_bounds.max_x;
                output.tile.max_y = tile_bounds.max_y;
            }
            assert!(tiles[tile_index].set(output).is_ok());
        }
    });
    cloud.xs.reserve(drawable);
    cloud.ys.reserve(drawable);
    cloud.colours.reserve(drawable);
    for slot in tiles {
        let output = slot.into_inner().expect("every tile was visited");
        cloud.xs.extend(output.xs);
        cloud.ys.extend(output.ys);
        cloud.colours.extend(output.colours);
        if output.tile.count > 0 {
            cloud.tiles.push(output.tile);
        }
    }
    cloud
}

struct Visible {
    tile: u32,
    draw: u32,
    row_low: i32,
    row_high: i32,
}
struct DrawContext<'a> {
    cloud: &'a SplatCloud,
    visible: &'a [Visible],
    projection: [f64; 5],
    width: i32,
    height: i32,
    radius: i32,
}
impl DrawContext<'_> {
    fn band(&self, low: i32, high: i32, pixels: &mut [u32], stride: usize) {
        for row in 0..(high - low) as usize {
            pixels[row * stride..row * stride + self.width as usize].fill(0);
        }
        let mut pixel_x = [0_i32; BLOCK];
        let mut pixel_y = [0_i32; BLOCK];
        for visible in self.visible {
            if visible.row_high < low || visible.row_low >= high {
                continue;
            }
            let tile = self.cloud.tiles[visible.tile as usize];
            for first in (0..visible.draw as usize).step_by(BLOCK) {
                let count = BLOCK.min(visible.draw as usize - first);
                let base = tile.begin as usize + first;
                project(
                    &self.projection,
                    &self.cloud.xs[base..base + count],
                    &self.cloud.ys[base..base + count],
                    &mut pixel_x[..count],
                    &mut pixel_y[..count],
                );
                if self.radius == 0 {
                    for index in 0..count {
                        let row = pixel_y[index];
                        let column = pixel_x[index];
                        if row >= low && row < high && column >= 0 && column < self.width {
                            pixels[(row - low) as usize * stride + column as usize] =
                                self.cloud.colours[base + index];
                        }
                    }
                    continue;
                }
                for index in 0..count {
                    let row = pixel_y[index];
                    let column = pixel_x[index];
                    if column < 0
                        || column >= self.width
                        || row < 0
                        || row >= self.height
                        || row + self.radius < low
                        || row - self.radius >= high
                    {
                        continue;
                    }
                    let row_start = low.max(row - self.radius);
                    let row_end = (high - 1).min(row + self.radius);
                    let column_start = 0.max(column - self.radius);
                    let column_end = (self.width - 1).min(column + self.radius);
                    for target_row in row_start..=row_end {
                        for target_column in column_start..=column_end {
                            pixels[(target_row - low) as usize * stride + target_column as usize] =
                                self.cloud.colours[base + index];
                        }
                    }
                }
            }
        }
    }
}
pub fn splat_cloud(
    cloud: &SplatCloud,
    view: &SplatView,
    pixels: &mut [u32],
    stride: usize,
    pool: &TaskPool,
) -> SplatStats {
    let mut stats = SplatStats::default();
    if cloud.size() == 0
        || view.width <= 0
        || view.height <= 0
        || !(view.scale > 0.0)
        || !view.scale.is_finite()
    {
        return stats;
    }
    let projection = PixelProjection {
        half_width: 0.5 * view.width as f64,
        half_height: 0.5 * view.height as f64,
        centre_x: view.centre.x - cloud.origin.x,
        centre_y: view.centre.y - cloud.origin.y,
        scale: view.scale,
    };
    let radius = view.radius.max(0);
    let width = view.width;
    let height = view.height;
    let mut visible = Vec::new();
    let mut row_begin = height;
    let mut row_end = 0;
    for (index, tile) in cloud.tiles.iter().enumerate() {
        let screen_x_low =
            projection.half_width + (tile.min_x as f64 - projection.centre_x) * projection.scale;
        let screen_x_high =
            projection.half_width + (tile.max_x as f64 - projection.centre_x) * projection.scale;
        let screen_y_low =
            projection.half_height - (tile.max_y as f64 - projection.centre_y) * projection.scale;
        let screen_y_high =
            projection.half_height - (tile.min_y as f64 - projection.centre_y) * projection.scale;
        if !(screen_x_high > -1.0
            && screen_x_low < width as f64
            && screen_y_high > -1.0
            && screen_y_low < height as f64)
        {
            continue;
        }
        let pixel_low = if screen_y_low <= 0.0 {
            0
        } else {
            screen_y_low as i32
        };
        let pixel_high = if screen_y_high >= height as f64 {
            height - 1
        } else {
            screen_y_high as i32
        };
        let row_low = 0.max(pixel_low - radius);
        let row_high = (height - 1).min(pixel_high + radius);
        visible.push(Visible {
            tile: index as u32,
            draw: tile.count,
            row_low,
            row_high,
        });
        stats.points_in_view += tile.count as usize;
        row_begin = row_begin.min(row_low);
        row_end = row_end.max(row_high + 1);
    }
    if visible.is_empty() {
        return stats;
    }
    if stats.points_in_view > view.point_budget {
        let budget = view.point_budget as u64;
        let total = stats.points_in_view as u64;
        for entry in &mut visible {
            entry.draw = ((entry.draw as u64)
                .wrapping_mul(budget)
                .wrapping_add(total - 1)
                / total) as u32;
        }
    }
    for entry in &visible {
        stats.points_drawn += entry.draw as usize;
    }
    stats.row_begin = row_begin;
    stats.row_end = row_end;
    assert!(stride >= width as usize);
    let required = (row_end as usize - 1)
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(width as usize))
        .expect("image size overflow");
    assert!(pixels.len() >= required);
    let context = DrawContext {
        cloud,
        visible: &visible,
        projection: projection.data(),
        width,
        height,
        radius,
    };
    let rows = row_end - row_begin;
    let threads = if stats.points_drawn < PARALLEL_MINIMUM {
        1
    } else {
        pool.concurrency()
    };
    let bands = 1.max((threads as i32).min(rows / MINIMUM_BAND_ROWS)) as usize;
    if bands == 1 {
        context.band(
            row_begin,
            row_end,
            &mut pixels[row_begin as usize * stride..required],
            stride,
        );
        return stats;
    }
    let rendered = map_chunks(pool, bands, |band| {
        let low = row_begin + (rows as usize * band / bands) as i32;
        let high = row_begin + (rows as usize * (band + 1) / bands) as i32;
        let mut image = vec![0; (high - low) as usize * width as usize];
        context.band(low, high, &mut image, width as usize);
        (low, high, image)
    });
    for (low, high, image) in rendered {
        for row in low..high {
            let source = (row - low) as usize * width as usize;
            let destination = row as usize * stride;
            pixels[destination..destination + width as usize]
                .copy_from_slice(&image[source..source + width as usize]);
        }
    }
    stats
}
