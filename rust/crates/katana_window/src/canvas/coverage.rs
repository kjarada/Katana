//! Exact-area antialiased coverage: how much of each device pixel a filled path covers, worked
//! out from the area of the pixel inside it, as Qt's raster engine does (qgrayraster, FreeType's
//! accumulation rasteriser). The raster canvas fills every antialiased shape through it and
//! leaves only the blending to tiny-skia.
//!
//! Why not tiny-skia's own antialiasing: it samples each pixel on a 4 x 4 grid, so an edge lands
//! on a quarter of a pixel and a coverage is a sixteenth. A pen 1.3 px wide then inks as 1.25 or
//! 1.5, and a painted twin that compares two pens' ink (a mesh footprint's outline at two
//! resolutions, 2 x 1.3 / 0.65 = 4.0 in Qt) read 3.33. Supersampling more finely only moves the
//! error; the area is exact.
//!
//! The method is the signed-area accumulation: each edge of the flattened path adds to the cell
//! it crosses the area of that cell's part to the right of it, and to the next cell the rest, by
//! the height it spans; a running sum along a row is then each pixel's winding, weighted by the
//! area it holds. The fill rule is applied to that sum, as FreeType applies it to a cell's
//! coverage: winding takes its size, at most one; odd-even folds it about every whole winding.
//! Curves are flattened to within a hundredth of a pixel first.

use tiny_skia::{FillRule, Path, PathSegment};

/// The largest distance a flattened curve's chords lie from the curve, in device pixels: a
/// hundredth of a pixel, under what an 8-bit coverage can show (1/255 of a pixel's area along a
/// pixel-long edge is 0.004 px, and a chord's error averages two thirds of its peak).
const TOLERANCE: f64 = 0.01;

/// The most chords a curve is cut into: a curve thousands of pixels long and tightly bent stays
/// within the tolerance with far fewer, and the cap keeps a non-sensical one from stalling a
/// paint.
const MOST_CHORDS: f64 = 4096.0;

/// The rows a band of coverage is worked out in at a time: a fill across a whole plotted sheet
/// (an A1 page at 300 dots an inch is 9933 x 7016 px) would otherwise hold eight bytes a pixel
/// of cells and four of its pixels at once; 64 rows of it are 5 MB.
pub(crate) const BAND_ROWS: u32 = 64;

type Point = (f64, f64);

/// A path flattened to its edges, in device pixels, and the window of the device it lies over:
/// what its coverage is worked out from, a band of rows at a time.
pub(crate) struct Edges {
    lines: Vec<(Point, Point)>,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The edges of `path`, in device pixels, on a device `device_width` by `device_height`: the
/// window is the path's bounds within the device. None when the path touches no pixel of it.
pub(crate) fn edges(path: &Path, device_width: u32, device_height: u32) -> Option<Edges> {
    let lines = flattened(path);
    let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(start, end) in &lines {
        for (x, y) in [start, end] {
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    if lines.is_empty() || !(left.is_finite() && right.is_finite()) {
        return None;
    }
    let x0 = left.floor().max(0.0);
    let y0 = top.floor().max(0.0);
    let x1 = right.ceil().min(f64::from(device_width));
    let y1 = bottom.ceil().min(f64::from(device_height));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(Edges {
        lines,
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    })
}

impl Edges {
    /// The coverage, filled by `rule`, of the window's rows `first .. first + rows` (counted from
    /// the window's top), row by row.
    pub(crate) fn band(&self, rule: FillRule, first: u32, rows: u32) -> Vec<u8> {
        let rows = rows.min(self.height.saturating_sub(first)) as usize;
        let width = self.width as usize;
        // Two spare cells a row: an edge at the window's right side adds to the cell past it and
        // the one after that, which the row's sum never reads.
        let stride = width + 2;
        let mut cells = vec![0.0_f64; stride * rows];
        let (left, top) = (f64::from(self.x), f64::from(self.y) + f64::from(first));
        for &((ax, ay), (bx, by)) in &self.lines {
            add_edge(
                &mut cells,
                stride,
                width as f64,
                rows as f64,
                (ax - left, ay - top),
                (bx - left, by - top),
            );
        }
        let mut alpha = vec![0_u8; width * rows];
        for row in 0..rows {
            let mut winding = 0.0;
            for column in 0..width {
                winding += cells[row * stride + column];
                let covered = match rule {
                    FillRule::Winding => winding.abs().min(1.0),
                    FillRule::EvenOdd => {
                        let folded = winding.abs() % 2.0;
                        if folded > 1.0 { 2.0 - folded } else { folded }
                    }
                };
                alpha[row * width + column] = (covered * 255.0).round() as u8;
            }
        }
        alpha
    }
}

/// The path as straight edges, every subpath closed (a fill closes it), curves cut into chords
/// within TOLERANCE of them.
fn flattened(path: &Path) -> Vec<(Point, Point)> {
    let mut lines = Vec::new();
    let mut start: Point = (0.0, 0.0);
    let mut at: Point = (0.0, 0.0);
    let point = |p: tiny_skia::Point| (f64::from(p.x), f64::from(p.y));
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(to) => {
                if at != start {
                    lines.push((at, start));
                }
                start = point(to);
                at = start;
            }
            PathSegment::LineTo(to) => {
                let to = point(to);
                lines.push((at, to));
                at = to;
            }
            PathSegment::QuadTo(control, to) => {
                let (c, to) = (point(control), point(to));
                // A quadratic's chords of a parameter step 1/n lie within |p0 - 2 p1 + p2| / (4
                // n^2) of it (its second derivative is 2 (p0 - 2 p1 + p2), and a chord's error is
                // an eighth of that over the step squared).
                let bend = length((at.0 - 2.0 * c.0 + to.0, at.1 - 2.0 * c.1 + to.1));
                let chords = chords_for(bend / (4.0 * TOLERANCE));
                let from = at;
                for i in 1..=chords {
                    let t = i as f64 / chords as f64;
                    let u = 1.0 - t;
                    let next = (
                        u * u * from.0 + 2.0 * u * t * c.0 + t * t * to.0,
                        u * u * from.1 + 2.0 * u * t * c.1 + t * t * to.1,
                    );
                    lines.push((at, if i == chords { to } else { next }));
                    at = if i == chords { to } else { next };
                }
            }
            PathSegment::CubicTo(c1, c2, to) => {
                let (c1, c2, to) = (point(c1), point(c2), point(to));
                // A cubic's second derivative is at most 6 max(|p0 - 2 p1 + p2|, |p1 - 2 p2 +
                // p3|), so its chords of step 1/n lie within 3/4 of that bend over n^2.
                let bend = length((at.0 - 2.0 * c1.0 + c2.0, at.1 - 2.0 * c1.1 + c2.1))
                    .max(length((c1.0 - 2.0 * c2.0 + to.0, c1.1 - 2.0 * c2.1 + to.1)));
                let chords = chords_for(0.75 * bend / TOLERANCE);
                let from = at;
                for i in 1..=chords {
                    let t = i as f64 / chords as f64;
                    let u = 1.0 - t;
                    let next = if i == chords {
                        to
                    } else {
                        (
                            u * u * u * from.0
                                + 3.0 * u * u * t * c1.0
                                + 3.0 * u * t * t * c2.0
                                + t * t * t * to.0,
                            u * u * u * from.1
                                + 3.0 * u * u * t * c1.1
                                + 3.0 * u * t * t * c2.1
                                + t * t * t * to.1,
                        )
                    };
                    lines.push((at, next));
                    at = next;
                }
            }
            PathSegment::Close => {
                if at != start {
                    lines.push((at, start));
                }
                at = start;
            }
        }
    }
    if at != start {
        lines.push((at, start));
    }
    lines
}

fn length((x, y): Point) -> f64 {
    x.hypot(y)
}

/// The chords a curve needs for its chords to lie within the tolerance: the square root of
/// `squared` (the step count whose square brings the error under it), at least one.
fn chords_for(squared: f64) -> usize {
    let chords = squared.sqrt().ceil();
    if chords.is_finite() && chords > 1.0 {
        chords.min(MOST_CHORDS) as usize
    } else {
        1
    }
}

/// Adds the edge from `a` to `b`, in the window's coordinates, to the window's cells: cut at the
/// window's top and bottom (rows outside it are not kept), and at its left and right sides,
/// where the part beyond is laid along the side. A part left of the window covers every pixel
/// of its rows to the right of it, as one along the left side does; a part right of it covers
/// none, as one along the right side does.
fn add_edge(cells: &mut [f64], stride: usize, width: f64, height: f64, a: Point, b: Point) {
    if a.1 == b.1 {
        return;
    }
    // The part within the rows.
    let (t_top, t_bottom) = ((0.0 - a.1) / (b.1 - a.1), (height - a.1) / (b.1 - a.1));
    let (t_in, t_out) = (t_top.min(t_bottom).max(0.0), t_top.max(t_bottom).min(1.0));
    if t_in >= t_out {
        return;
    }
    let along = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    // Cut where it crosses the left and right sides.
    let mut cuts = vec![t_in, t_out];
    if a.0 != b.0 {
        for side in [0.0, width] {
            let t = (side - a.0) / (b.0 - a.0);
            if t > t_in && t < t_out {
                cuts.push(t);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    for pair in cuts.windows(2) {
        let (from, to) = (along(pair[0]), along(pair[1]));
        let clamp = |(x, y): Point| (x.clamp(0.0, width), y.clamp(0.0, height));
        add_line(cells, stride, clamp(from), clamp(to));
    }
}

/// Adds a line lying within the window to its cells: in each row it spans, the signed height it
/// covers there (downward positive), shared between the cells it crosses by the area of each to
/// the line's right.
fn add_line(cells: &mut [f64], stride: usize, a: Point, b: Point) {
    if a.1 == b.1 {
        return;
    }
    let (direction, top, bottom) = if a.1 < b.1 { (1.0, a, b) } else { (-1.0, b, a) };
    let slope = (bottom.0 - top.0) / (bottom.1 - top.1);
    let x_at = |y: f64| {
        if y >= bottom.1 {
            bottom.0
        } else {
            top.0 + slope * (y - top.1)
        }
    };
    let first = top.1.floor() as usize;
    let last = bottom.1.ceil() as usize;
    for row in first..last {
        let enter = (row as f64).max(top.1);
        let leave = ((row + 1) as f64).min(bottom.1);
        if leave <= enter {
            continue;
        }
        let height = (leave - enter) * direction;
        let (x_enter, x_leave) = (x_at(enter), x_at(leave));
        let (low, high) = if x_enter < x_leave {
            (x_enter, x_leave)
        } else {
            (x_leave, x_enter)
        };
        let base = row * stride;
        let low_floor = low.floor();
        let low_cell = low_floor as usize;
        let high_cell = high.ceil() as usize;
        if high_cell <= low_cell + 1 {
            // Within one column: the part of the cell right of the line's mean position.
            let mean = 0.5 * (x_enter + x_leave) - low_floor;
            cells[base + low_cell] += height * (1.0 - mean);
            cells[base + low_cell + 1] += height * mean;
        } else {
            // Across several: the line's height in each column is in proportion to its width
            // there, and each column's area right of it a triangle in the first and last.
            let per_column = 1.0 / (high - low);
            let low_part = low - low_floor;
            let first_area = 0.5 * per_column * (1.0 - low_part) * (1.0 - low_part);
            let high_part = high - high.ceil() + 1.0;
            let last_area = 0.5 * per_column * high_part * high_part;
            cells[base + low_cell] += height * first_area;
            if high_cell == low_cell + 2 {
                cells[base + low_cell + 1] += height * (1.0 - first_area - last_area);
            } else {
                let second = per_column * (1.5 - low_part);
                cells[base + low_cell + 1] += height * (second - first_area);
                for cell in low_cell + 2..high_cell - 1 {
                    cells[base + cell] += height * per_column;
                }
                let before_last = second + (high_cell - low_cell - 3) as f64 * per_column;
                cells[base + high_cell - 1] += height * (1.0 - before_last - last_area);
            }
            cells[base + high_cell] += height * last_area;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{PathBuilder, Rect};

    /// The coverage of a window of the device: pixels `x .. x + width` by `y .. y + height`,
    /// each 0 (untouched) to 255 (covered), row by row.
    struct Coverage {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        alpha: Vec<u8>,
    }

    /// The coverage of `path` over its whole window at once; None when it touches no pixel of
    /// the device.
    fn coverage(
        path: &Path,
        rule: FillRule,
        device_width: u32,
        device_height: u32,
    ) -> Option<Coverage> {
        let edges = edges(path, device_width, device_height)?;
        let alpha = edges.band(rule, 0, edges.height);
        Some(Coverage {
            x: edges.x,
            y: edges.y,
            width: edges.width,
            height: edges.height,
            alpha,
        })
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Path {
        PathBuilder::from_rect(Rect::from_xywh(x, y, width, height).expect("a rectangle"))
    }

    fn alpha_at(coverage: &Coverage, x: u32, y: u32) -> u8 {
        if x < coverage.x
            || y < coverage.y
            || x >= coverage.x + coverage.width
            || y >= coverage.y + coverage.height
        {
            return 0;
        }
        coverage.alpha[((y - coverage.y) * coverage.width + (x - coverage.x)) as usize]
    }

    #[test]
    fn a_square_at_whole_coordinates_covers_its_one_pixel_and_nothing_beside_it() {
        let covered =
            coverage(&rect(2.0, 3.0, 1.0, 1.0), FillRule::Winding, 8, 8).expect("on the device");
        assert_eq!(
            (covered.x, covered.y, covered.width, covered.height),
            (2, 3, 1, 1)
        );
        assert_eq!(covered.alpha, vec![255]);
    }

    // Worked by hand: a rectangle from x 0.2 to 1.6 over one row covers 0.8 of its first pixel
    // (204 of 255) and 0.6 of its second (153); tiny-skia's quarter-pixel edges would give 0.75
    // (191) and 0.5 or 0.75.
    #[test]
    fn an_edge_between_quarters_of_a_pixel_covers_its_exact_area() {
        let covered =
            coverage(&rect(0.2, 0.0, 1.4, 1.0), FillRule::Winding, 4, 1).expect("on the device");
        assert_eq!(alpha_at(&covered, 0, 0), 204);
        assert_eq!(alpha_at(&covered, 1, 0), 153);
        assert_eq!(alpha_at(&covered, 2, 0), 0);
    }

    // Worked by hand: a sloped edge across three columns. The triangle (0,0) (3,0) (0,1) covers,
    // in its one row, 1 - 1/6 of the first pixel (the part right of x = 3 (1 - y) between
    // x 0 and 1 is the triangle of legs 1 and 1/3), 1/2 of the second (symmetric) and 1/6 of
    // the third: 212.5, 127.5 and 42.5 of 255, each of which a rounding may take either way.
    // tiny-skia's sixteenths would give 13/16 or 14/16 (207, 223) and 3/16 (48).
    #[test]
    fn a_sloped_edge_shares_its_row_between_the_columns_it_crosses_by_area() {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, 0.0);
        builder.line_to(3.0, 0.0);
        builder.line_to(0.0, 1.0);
        builder.close();
        let path = builder.finish().expect("a triangle");
        let covered = coverage(&path, FillRule::Winding, 4, 1).expect("on the device");
        let expected = [5.0 / 6.0, 0.5, 1.0 / 6.0];
        for (column, area) in expected.iter().enumerate() {
            let want = area * 255.0;
            let got = f64::from(alpha_at(&covered, column as u32, 0));
            assert!(
                (got - want).abs() <= 0.5,
                "column {column}: {got} for {want}"
            );
        }
    }

    /// A regular polygon of `sides` round (`x`, `y`) with its corners at `radius`, the first at
    /// angle 0: a circle whose area is known exactly (tiny-skia's own circle is quadratics that
    /// bulge out by up to 0.03 px at radius 10, so its area is not pi r^2).
    fn polygon(builder: &mut PathBuilder, x: f64, y: f64, radius: f64, sides: u32) {
        for side in 0..sides {
            let angle = std::f64::consts::TAU * f64::from(side) / f64::from(sides);
            let (px, py) = (
                (x + radius * angle.cos()) as f32,
                (y + radius * angle.sin()) as f32,
            );
            if side == 0 {
                builder.move_to(px, py);
            } else {
                builder.line_to(px, py);
            }
        }
        builder.close();
    }

    fn sum_of(covered: &Coverage) -> f64 {
        covered.alpha.iter().map(|&a| f64::from(a) / 255.0).sum()
    }

    // Worked by hand: the part of the pixel [3,4] x [3,4] (from a ring's centre) outside radius 5
    // is 1 - (integral from 3 to 4 of sqrt(25 - x^2) dx - 3) = 1 - 0.54743 = 0.45257, and all of
    // it is inside radius 7: 115.4 of 255. A ring of radii 5 and 7 round (10, 10), as polygons
    // of 256 sides (each side within 5 (1 - cos(pi / 256)) = 0.0004 px of its circle), covers
    // the pixel (13, 13) that much, within the one 255th a rounding can move it.
    #[test]
    fn a_rings_edge_pixel_is_covered_by_the_area_of_it_inside_the_ring() {
        let mut builder = PathBuilder::new();
        polygon(&mut builder, 10.0, 10.0, 7.0, 256);
        polygon(&mut builder, 10.0, 10.0, 5.0, 256);
        let ring = builder.finish().expect("a ring");
        let covered = coverage(&ring, FillRule::EvenOdd, 20, 20).expect("on the device");
        let pixel = i32::from(alpha_at(&covered, 13, 13));
        assert!((pixel - 115).abs() <= 1, "{pixel}");
        // The hole is clear and the band between the circles covered: the pixel [5,6] x [0,1]
        // from the centre lies from 5 to 6.08 px out.
        assert_eq!(alpha_at(&covered, 10, 10), 0);
        assert_eq!(alpha_at(&covered, 15, 10), 255);
    }

    // Worked by hand: a regular polygon of 64 sides with its corners 10 px out has an area of
    // 64 / 2 x 10^2 x sin(2 pi / 64) = 313.655 px; the coverage summed over the window is that
    // within the rounding of each edge pixel to a 255th (at most half of one each, about 80 edge
    // pixels: 0.16).
    #[test]
    fn a_polygons_coverage_adds_up_to_its_area() {
        let mut builder = PathBuilder::new();
        polygon(&mut builder, 16.0, 16.0, 10.0, 64);
        let disc = builder.finish().expect("a polygon");
        let covered = coverage(&disc, FillRule::Winding, 32, 32).expect("on the device");
        let area = sum_of(&covered);
        let exact = 32.0 * 100.0 * (std::f64::consts::TAU / 64.0).sin();
        assert!((area - exact).abs() < 0.2, "{area} for {exact}");
    }

    // Worked by hand (Archimedes): the region between a quadratic curve and its chord is two
    // thirds of the triangle of its three points. From (2, 28) bent by (16, 0) to (30, 28), the
    // triangle is 28 x 28 / 2 = 392 px, the region 261.333. Flattened into n chords of equal
    // parameter steps the region loses 1 / n^2 of itself (each halving of the steps keeps three
    // quarters of what is left): n = 38 for this bend, 0.181 px; with the rounding (0.15 at
    // most), within 0.4.
    #[test]
    fn a_curves_coverage_adds_up_to_the_area_under_it() {
        let mut builder = PathBuilder::new();
        builder.move_to(2.0, 28.0);
        builder.quad_to(16.0, 0.0, 30.0, 28.0);
        builder.close();
        let segment = builder.finish().expect("a curve");
        let covered = coverage(&segment, FillRule::Winding, 32, 32).expect("on the device");
        let area = sum_of(&covered);
        assert!((area - 2.0 / 3.0 * 392.0).abs() < 0.4, "{area}");
    }

    // Worked a band at a time, a shape is covered exactly as it is all at once: a ring's 32 rows
    // in bands of 3 (the last of 2), its edges cut at every band's top and bottom.
    #[test]
    fn a_shape_worked_in_bands_is_covered_as_it_is_all_at_once() {
        let mut builder = PathBuilder::new();
        builder.push_circle(16.0, 16.0, 15.0);
        builder.push_circle(16.0, 16.0, 6.3);
        let ring = builder.finish().expect("a ring");
        let edges = edges(&ring, 32, 32).expect("on the device");
        let whole = edges.band(FillRule::EvenOdd, 0, edges.height);
        let mut banded = Vec::new();
        let mut first = 0;
        while first < edges.height {
            banded.extend(edges.band(FillRule::EvenOdd, first, 3));
            first += 3;
        }
        assert_eq!(banded.len(), whole.len());
        let differing = banded.iter().zip(&whole).filter(|(a, b)| a != b).count();
        assert_eq!(differing, 0);
    }

    // Two squares turning the same way over one pixel: winding 2 there, filled by the winding
    // rule and a hole by the odd-even one.
    #[test]
    fn an_overlap_is_filled_by_winding_and_clear_by_odd_even() {
        let mut builder = PathBuilder::new();
        builder.push_rect(Rect::from_xywh(0.0, 0.0, 2.0, 1.0).expect("a rectangle"));
        builder.push_rect(Rect::from_xywh(1.0, 0.0, 2.0, 1.0).expect("a rectangle"));
        let path = builder.finish().expect("two rectangles");
        let winding = coverage(&path, FillRule::Winding, 3, 1).expect("on the device");
        let odd_even = coverage(&path, FillRule::EvenOdd, 3, 1).expect("on the device");
        assert_eq!(winding.alpha, vec![255, 255, 255]);
        assert_eq!(odd_even.alpha, vec![255, 0, 255]);
    }

    // A shape running off the device: the part on it is covered as it would be on a larger one,
    // the part off it not kept. Left: from x -5 to 1.5, so the first pixel whole and the second
    // half (127.5, rounding to 128). Right: from 1.5 to 10.
    #[test]
    fn a_shape_off_the_devices_sides_covers_only_the_part_on_it() {
        let left =
            coverage(&rect(-5.0, 0.0, 6.5, 1.0), FillRule::Winding, 3, 1).expect("on the device");
        assert_eq!((left.x, left.width), (0, 2));
        assert_eq!(left.alpha, vec![255, 128]);
        let right =
            coverage(&rect(1.5, 0.0, 8.5, 1.0), FillRule::Winding, 3, 1).expect("on the device");
        assert_eq!((right.x, right.width), (1, 2));
        assert_eq!(right.alpha, vec![128, 255]);
        let above =
            coverage(&rect(0.0, -4.0, 1.0, 4.5), FillRule::Winding, 1, 2).expect("on the device");
        assert_eq!((above.y, above.height), (0, 1));
        assert_eq!(above.alpha, vec![128]);
    }

    #[test]
    fn a_shape_wholly_off_the_device_or_of_no_area_covers_nothing() {
        assert!(coverage(&rect(10.0, 0.0, 2.0, 2.0), FillRule::Winding, 4, 4).is_none());
        assert!(coverage(&rect(0.0, -3.0, 2.0, 2.0), FillRule::Winding, 4, 4).is_none());
        let mut builder = PathBuilder::new();
        builder.move_to(0.5, 0.5);
        builder.line_to(3.5, 3.5);
        let line = builder.finish().expect("a line");
        // A line closed on itself encloses nothing: whatever window it has, nothing in it.
        if let Some(covered) = coverage(&line, FillRule::Winding, 4, 4) {
            assert!(covered.alpha.iter().all(|&a| a == 0));
        }
    }
}
