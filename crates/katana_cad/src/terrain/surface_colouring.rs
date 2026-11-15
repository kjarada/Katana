//! How a surface's faces are coloured, resolved once from its display style
//! (surface_style.rs) and the values it spans, and asked by every picture of
//! it - the plan view (surface_plan.rs), the 3D scene (scene.rs), the legends
//! and the SURFACE LEGEND verb - so that no two of them can disagree about
//! what colour a height, a slope or an aspect is (docs/surface_display.md;
//! include/katana/cad/terrain/surface_colouring.hpp,
//! src/katana_cad/terrain/surface_colouring.cpp and surface_ramps.cpp).
//!
//! The value a face is coloured by is its ELEVATION in drawing units, or its
//! SLOPE IN PERCENT (rise over run times 100: 100 is 45 degrees), or the
//! direction it faces (aspect, in eight sectors). The ramps are the surface
//! styles' view of katana_terrain's one ramp table (colour_ramps), which the
//! 3D scene's own elevation and slope colours are read from too.

use std::cmp::Ordering;
use std::f64::consts::PI;
use std::sync::OnceLock;

use katana_entity::Color;
use katana_geometry::Point3;
use katana_math::cpp;
use katana_math::numerics::tolerance;
use katana_terrain::TinSurface;
use katana_terrain::colour_ramps::{COLOUR_RAMP_STOPS, colour_ramps, find_colour_ramp};

use crate::terrain::surface_style::{
    ColourStop, SurfaceDisplayStyle, SurfaceFaces, surface_ramp_names,
};

// ---- the ramps -------------------------------------------------------------------------

/// Every ramp is five colours evenly spaced over 0 to 1, blended linearly
/// between them: the form the 3D scene's vector kernels read (four segments a
/// channel, simd/scene_kernels.rs), so a surface coloured by ANY ramp takes
/// the fast path, and a ramp is a row of five colours a person can read in the
/// table.
pub const SURFACE_RAMP_STOPS: usize = 5;
pub type SurfaceRampStops = [Color; SURFACE_RAMP_STOPS];

const _: () = assert!(COLOUR_RAMP_STOPS == SURFACE_RAMP_STOPS);

/// The table's ramps as the drawing's colours, in the table's order, built
/// once and never changed: the scene keeps a reference to a ramp's stops
/// (`surface_ramp_stops(name)`), which a table that grew would move.
fn drawing_ramps() -> &'static [SurfaceRampStops] {
    static RAMPS: OnceLock<Vec<SurfaceRampStops>> = OnceLock::new();
    RAMPS.get_or_init(|| {
        colour_ramps()
            .iter()
            .map(|ramp| {
                std::array::from_fn(|i| {
                    let stop = ramp.stops[i];
                    Color::new(stop.r, stop.g, stop.b, 255)
                })
            })
            .collect()
    })
}

/// The stops of `ramp` (case-insensitive); an unknown name is the first
/// ramp's, as surface_ramp_colour says. One object a ramp, whatever the case
/// of the name.
pub fn surface_ramp_stops(ramp: &str) -> &'static SurfaceRampStops {
    // An unknown name is the first ramp's, as the header says.
    let index = find_colour_ramp(ramp)
        .and_then(|found| {
            colour_ramps()
                .iter()
                .position(|candidate| std::ptr::eq(candidate, found))
        })
        .unwrap_or(0);
    &drawing_ramps()[index]
}

/// Whether `ramp` names one of surface_ramp_names(), in any case.
pub fn is_surface_ramp(ramp: &str) -> bool {
    find_colour_ramp(ramp).is_some()
}

/// The colour at `t` along `stops`: t clamped to 0 to 1 (not finite: 0), and
/// each channel a + (b - a) f rounded half up - the arithmetic of the scene's
/// elevation ramp before the table, to the bit, which its kernels equal.
pub fn surface_ramp_colour(stops: &SurfaceRampStops, t: f64) -> Color {
    // The scene's elevation_ramp_color before the table, operation for
    // operation: the kernels (simd/scene_kernels.rs) are held to it to the
    // bit, so it is not to be rewritten as an "equivalent" expression.
    const STOPS: i32 = SURFACE_RAMP_STOPS as i32;
    let clamped = cpp::clamp(if t.is_finite() { t } else { 0.0 }, 0.0, 1.0) * f64::from(STOPS - 1);
    let lower = std::cmp::min(clamped as i32, STOPS - 2);
    let f = clamped - f64::from(lower);
    let blend = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * f + 0.5) as u8;
    let a = stops[lower as usize];
    let b = stops[(lower + 1) as usize];
    Color::new(blend(a.r, b.r), blend(a.g, b.g), blend(a.b, b.b), 255)
}

/// The colour at `t` (clamped to 0 to 1) along ramp `ramp`; an unknown name is
/// the first ramp's. The styles' ramp table: the 3D scene and the plan view
/// both colour through it. The C++ `surfaceRampColour(std::string_view, double)`
/// of surface_style.hpp, under a name of its own beside the stops' form.
pub fn surface_ramp_colour_of(ramp: &str, t: f64) -> Color {
    surface_ramp_colour(surface_ramp_stops(ramp), t)
}

// ---- aspect ----------------------------------------------------------------------------

/// A face's aspect is one of eight sectors of 45 degrees, centred on north,
/// north-east ... north-west, as a compass rose is read.
pub const ASPECT_SECTORS: i32 = 8;

/// The hue wheel the aspect sectors are coloured round: red at north, then
/// orange, yellow, green, cyan, azure, blue and magenta clockwise - full
/// saturation, so neighbouring sectors are as far apart as eight hues allow,
/// and opposite slopes (north and south) are complementary.
const ASPECT_COLOURS: [Color; ASPECT_SECTORS as usize] = [
    Color::new(230, 40, 40, 255),  // N
    Color::new(240, 150, 30, 255), // NE
    Color::new(235, 225, 40, 255), // E
    Color::new(60, 190, 60, 255),  // SE
    Color::new(40, 200, 210, 255), // S
    Color::new(40, 140, 230, 255), // SW
    Color::new(50, 60, 220, 255),  // W
    Color::new(200, 50, 200, 255), // NW
];

const ASPECT_NAMES: [&str; ASPECT_SECTORS as usize] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

/// The sector of an aspect in radians clockwise from north (TinSurface's
/// SlopeAspect::aspect): 0 north, from 337.5 to 22.5 degrees, then clockwise;
/// a sector's lower edge belongs to it. Not finite: 0.
pub fn aspect_sector(aspect_radians: f64) -> i32 {
    if !aspect_radians.is_finite() {
        return 0;
    }
    // Shift by half a sector so north's sector starts at 0, then whole
    // sectors of 45 degrees.
    let sector = PI / 4.0;
    let shifted = (aspect_radians + sector / 2.0) % (2.0 * PI);
    let wrapped = if shifted < 0.0 {
        shifted + 2.0 * PI
    } else {
        shifted
    };
    ((wrapped / sector).floor() as i32).clamp(0, ASPECT_SECTORS - 1)
}

/// Red at north round a hue wheel in 45-degree steps (docs/surface_display.md,
/// "Aspect"); an out-of-range sector is taken modulo eight.
pub fn aspect_sector_colour(sector: i32) -> Color {
    ASPECT_COLOURS[sector.rem_euclid(ASPECT_SECTORS) as usize]
}

/// "N", "NE", "E", "SE", "S", "SW", "W", "NW".
pub fn aspect_sector_name(sector: i32) -> &'static str {
    ASPECT_NAMES[sector.rem_euclid(ASPECT_SECTORS) as usize]
}

// ---- what a surface spans --------------------------------------------------------------

/// The steepest face, in percent; 0 for an empty or flat surface. A triangle
/// vertical in plan (no slope) is not counted.
pub fn steepest_slope_percent(surface: &TinSurface) -> f64 {
    let mut steepest = 0.0;
    for t in 0..surface.triangle_count() {
        if let Some(slope) = surface.triangle_slope_aspect(t)
            && slope.slope.is_finite()
        {
            steepest = cpp::max(steepest, slope.slope * 100.0);
        }
    }
    steepest
}

// ---- the colouring ---------------------------------------------------------------------

/// What colours one surface's faces in one picture: the style's rule with its
/// range resolved. Plain data, cheap to copy.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceColouring {
    pub faces: SurfaceFaces,
    /// Elevation and Slope: the ramp, stretched over [low, high] - or, when
    /// `stops` is not empty, the stops, which override it and are absolute.
    pub ramp: SurfaceRampStops,
    pub ramp_name: String,
    pub stops: Vec<ColourStop>,
    pub banded: bool,
    pub bands: i32,
    pub low: f64,
    pub high: f64,
    /// The data runs past an end of the range the style set: min= above the
    /// lowest value (open_below), max= below the highest (open_above). A
    /// banded ramp's end band then takes in everything past its edge, and
    /// says so (band_low, band_high) rather than naming a closed range its
    /// colour also covers. surface_colouring sets them.
    pub open_below: bool,
    pub open_above: bool,
    /// Flat faces, a flat (level) face under Aspect, and a value that is not
    /// finite.
    pub flat: Color,
}

impl Default for SurfaceColouring {
    fn default() -> Self {
        Self {
            faces: SurfaceFaces::None,
            ramp: *surface_ramp_stops("terrain"),
            ramp_name: String::from("terrain"),
            stops: Vec::new(),
            banded: false,
            bands: 10,
            low: 0.0,
            high: 0.0,
            open_below: false,
            open_above: false,
            flat: Color::new(170, 170, 160, 255),
        }
    }
}

/// A blend of two colours, rounded half up, alpha included.
fn mix(a: Color, b: Color, f: f64) -> Color {
    let channel =
        |lo: u8, hi: u8| (f64::from(lo) + (f64::from(hi) - f64::from(lo)) * f + 0.5) as u8;
    Color::new(
        channel(a.r, b.r),
        channel(a.g, b.g),
        channel(a.b, b.b),
        channel(a.a, b.a),
    )
}

/// Which side of a collapsed range's point a value is on: -1 below, +1 above,
/// 0 at it (within tolerance::GEOMETRIC).
fn side_of_point(colouring: &SurfaceColouring, value: f64) -> i32 {
    if value < colouring.low - tolerance::GEOMETRIC {
        return -1;
    }
    if value > colouring.high + tolerance::GEOMETRIC {
        return 1;
    }
    0
}

/// Where along the ramp, 0 to 1, a value on that side of a collapsed range
/// lies. At the point a level surface is the ramp's middle; slope's point is a
/// face with no steepness, the ramp's start.
fn collapsed_position(colouring: &SurfaceColouring, side: i32) -> f64 {
    if side != 0 {
        return if side < 0 { 0.0 } else { 1.0 };
    }
    if colouring.faces == SurfaceFaces::Slope {
        0.0
    } else {
        0.5
    }
}

fn collapsed_band(colouring: &SurfaceColouring, side: i32) -> usize {
    let count = colouring.band_count();
    let position = collapsed_position(colouring, side);
    if position < 0.5 {
        0
    } else if position > 0.5 {
        count - 1
    } else {
        count / 2
    }
}

/// The index of the first stop whose value is above `value`: std::upper_bound
/// over the stops by value.
fn first_stop_above(stops: &[ColourStop], value: f64) -> usize {
    stops.partition_point(|stop| !(value < stop.value))
}

impl SurfaceColouring {
    pub fn colours_faces(&self) -> bool {
        self.faces != SurfaceFaces::None
    }

    /// Elevation or Slope: a value decides the colour.
    pub fn by_value(&self) -> bool {
        self.faces == SurfaceFaces::Elevation || self.faces == SurfaceFaces::Slope
    }

    /// Banded only. The bands:
    ///
    /// ```text
    /// a ramp   `bands` (at least one) of equal width over [low, high];
    ///          below low is the first, above high the last. Band k is the
    ///          ramp at k / (bands - 1), so the first and last bands are the
    ///          ramp's two ends (one band: its middle). A collapsed range is
    ///          one point, and takes the same sides: below it the first
    ///          band, above it the last, at it the middle (slope: the first).
    /// stops    one a stop: band k runs from stop k's value up to the next
    ///          stop's, in stop k's colour; below the first stop is the
    ///          first band, from the last stop up is the last.
    /// ```
    pub fn band_count(&self) -> usize {
        if !self.stops.is_empty() {
            return self.stops.len();
        }
        std::cmp::max(self.bands, 1) as usize
    }

    /// A ramp (no stops) whose range is narrower than tolerance::GEOMETRIC: a
    /// point, not something to stretch a ramp or cut bands over.
    pub fn collapsed(&self) -> bool {
        self.stops.is_empty() && !(self.high - self.low > tolerance::GEOMETRIC)
    }

    pub fn band_of(&self, value: f64) -> usize {
        let count = self.band_count();
        if !value.is_finite() || count <= 1 {
            return 0;
        }
        if !self.stops.is_empty() {
            // The last stop at or below the value; below them all, the first.
            let index = first_stop_above(&self.stops, value);
            return if index == 0 { 0 } else { index - 1 };
        }
        let span = self.high - self.low;
        if !(span > tolerance::GEOMETRIC) {
            return collapsed_band(self, side_of_point(self, value));
        }
        let scaled = (value - self.low) / span * count as f64;
        if !(scaled > 0.0) {
            return 0;
        }
        std::cmp::min(scaled as usize, count - 1)
    }

    pub fn band_colour(&self, band: usize) -> Color {
        let count = self.band_count();
        let k = std::cmp::min(band, count - 1);
        if !self.stops.is_empty() {
            return self.stops[k].colour;
        }
        if count <= 1 {
            return surface_ramp_colour(&self.ramp, 0.5);
        }
        surface_ramp_colour(&self.ramp, k as f64 / (count - 1) as f64)
    }

    /// Where band k starts; absent at an open end: the first band of stops
    /// starts nowhere below (one stop is open both ways), and a ramp's first
    /// band is open where the data runs past the range (open_below). Where it
    /// does not it is closed at low.
    pub fn band_low(&self, band: usize) -> Option<f64> {
        let count = self.band_count();
        let k = std::cmp::min(band, count - 1);
        if !self.stops.is_empty() {
            return if k == 0 {
                None
            } else {
                Some(self.stops[k].value)
            };
        }
        if k == 0 && self.open_below {
            return None;
        }
        Some(self.low + (self.high - self.low) * k as f64 / count as f64)
    }

    /// Where band k ends; absent at an open end: the last band of stops ends
    /// nowhere above, and a ramp's last band is open where the data runs past
    /// the range (open_above). Where it does not it is closed at high.
    pub fn band_high(&self, band: usize) -> Option<f64> {
        let count = self.band_count();
        let k = std::cmp::min(band, count - 1);
        if !self.stops.is_empty() {
            return if k + 1 >= count {
                None
            } else {
                Some(self.stops[k + 1].value)
            };
        }
        // The last band's top is `high` itself, not low + span * count /
        // count, which rounding could leave a hair short of it.
        if k + 1 == count {
            return if self.open_above {
                None
            } else {
                Some(self.high)
            };
        }
        Some(self.low + (self.high - self.low) * (k + 1) as f64 / count as f64)
    }

    /// The colour of `value` (an elevation, or a slope in percent):
    ///
    /// ```text
    /// smooth ramp   the ramp at (value - low) / (high - low), clamped. A
    ///               range narrower than tolerance::GEOMETRIC is a point
    ///               (collapsed): below it is the ramp's start and above it
    ///               its end, as for any range; at it the ramp's middle, as
    ///               the scene's one ramp always was for a level surface -
    ///               or, for slope, its start, because the point is a face
    ///               with no steepness;
    /// smooth stops  blended between the two stops either side, the first
    ///               below them all and the last above;
    /// banded        its band's colour (band_of, band_colour).
    /// ```
    ///
    /// Not finite: `flat`.
    pub fn colour_at(&self, value: f64) -> Color {
        if !value.is_finite() {
            return self.flat;
        }
        if self.banded {
            return self.band_colour(self.band_of(value));
        }
        if let (Some(front), Some(back)) = (self.stops.first(), self.stops.last()) {
            if value <= front.value {
                return front.colour;
            }
            if value >= back.value {
                return back.colour;
            }
            let above = first_stop_above(&self.stops, value);
            let b = self.stops[above];
            let a = self.stops[above - 1];
            let width = b.value - a.value;
            return if width > 0.0 {
                mix(a.colour, b.colour, (value - a.value) / width)
            } else {
                b.colour
            };
        }
        let span = self.high - self.low;
        if !(span > tolerance::GEOMETRIC) {
            return surface_ramp_colour(
                &self.ramp,
                collapsed_position(self, side_of_point(self, value)),
            );
        }
        surface_ramp_colour(&self.ramp, (value - self.low) / span)
    }
}

/// `style`'s rule for `faces` (its plan_faces or its faces3d), the ramp
/// stretched over the style's range - each end the style gives - else over
/// [default_low, default_high]. A range given back to front is taken as the
/// point at its low end (collapsed). An unknown ramp name is the first ramp,
/// as surface_ramp_colour takes it.
pub fn surface_colouring(
    style: &SurfaceDisplayStyle,
    faces: SurfaceFaces,
    default_low: f64,
    default_high: f64,
) -> SurfaceColouring {
    let mut colouring = SurfaceColouring {
        faces,
        ..SurfaceColouring::default()
    };
    colouring.ramp_name = if is_surface_ramp(&style.ramp) {
        style.ramp.clone()
    } else {
        String::from(surface_ramp_names()[0])
    };
    colouring.ramp = *surface_ramp_stops(&style.ramp);
    colouring.stops = style.stops.clone();
    // check_surface_style refuses stops out of order; a set that came in
    // around it is put in order rather than read wrongly. Stable, by `<` on
    // the values, as std::ranges::stable_sort orders them.
    colouring.stops.sort_by(|a, b| {
        if a.value < b.value {
            Ordering::Less
        } else if b.value < a.value {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    colouring.banded = style.banded;
    colouring.bands = std::cmp::max(style.bands, 1);
    colouring.flat = style.flat_colour;
    colouring.low = style.range_min.unwrap_or(default_low);
    colouring.high = style.range_max.unwrap_or(default_high);
    if !colouring.low.is_finite() {
        colouring.low = 0.0;
    }
    if !colouring.high.is_finite() || colouring.high < colouring.low {
        colouring.high = colouring.low;
    }
    // The data runs past an end the style fixed: its end band takes it in.
    colouring.open_below = style
        .range_min
        .is_some_and(|minimum| default_low.is_finite() && minimum > default_low);
    colouring.open_above = style
        .range_max
        .is_some_and(|maximum| default_high.is_finite() && maximum < default_high);
    colouring
}

/// The same, with the defaults a surface gives on its own: its lowest and
/// highest vertex for Elevation, 0 to its steepest face for Slope. The C++
/// overload `surfaceColouring(style, faces, surface)`.
pub fn surface_colouring_for(
    style: &SurfaceDisplayStyle,
    faces: SurfaceFaces,
    surface: &TinSurface,
) -> SurfaceColouring {
    if faces == SurfaceFaces::Slope {
        // The steepest face is a pass over every triangle: only when the
        // style leaves the top of the range open.
        let top = match style.range_max {
            Some(maximum) => maximum,
            None => steepest_slope_percent(surface),
        };
        return surface_colouring(style, faces, 0.0, top);
    }
    surface_colouring(
        style,
        faces,
        surface.min_elevation(),
        surface.max_elevation(),
    )
}

/// The parts of a triangle in each band of a BANDED colouring by elevation:
/// convex polygons of three to five corners on the triangle's own plane, each
/// wholly in one band, covering the triangle exactly - a band edge is a
/// contour across it, and a corner on one has the edge's level exactly. The
/// 3D scene draws banded faces so, a band's colour stopping at its contour
/// rather than smeared across a triangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BandPiece {
    pub band: usize,
    pub corners: [Point3; 5],
    pub count: usize,
}

/// The corners of `polygon` on the `keep_above` side of the level `level`
/// (above: z >= level; below: z <= level), Sutherland and Hodgman against one
/// horizontal plane. A corner where an edge crosses is put on the level
/// exactly, so a piece's band edge is the contour itself.
fn clip_to_level(polygon: &mut [Point3; 5], count: &mut usize, level: f64, keep_above: bool) {
    let mut out = [Point3::default(); 5];
    let mut kept = 0;
    let inside = |p: &Point3| {
        if keep_above {
            p.z >= level
        } else {
            p.z <= level
        }
    };
    for i in 0..*count {
        let p = polygon[i];
        let q = polygon[(i + 1) % *count];
        let p_in = inside(&p);
        let q_in = inside(&q);
        if p_in && kept < out.len() {
            out[kept] = p;
            kept += 1;
        }
        if p_in != q_in && kept < out.len() {
            let t = (level - p.z) / (q.z - p.z);
            out[kept] = Point3::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t, level);
            kept += 1;
        }
    }
    *polygon = out;
    *count = kept;
}

/// Appends `a`, `b`, `c`'s pieces to `out`, lowest band first. A colouring
/// that is not banded by value gives the whole triangle in band 0. A banded
/// style has at most MAXIMUM_SURFACE_BANDS bands (check_surface_style), so a
/// triangle is cut into that many pieces at the most.
pub fn band_pieces(
    colouring: &SurfaceColouring,
    a: Point3,
    b: Point3,
    c: Point3,
    out: &mut Vec<BandPiece>,
) {
    let mut whole = BandPiece {
        band: 0,
        corners: [a, b, c, Point3::default(), Point3::default()],
        count: 3,
    };
    if !colouring.banded || colouring.faces != SurfaceFaces::Elevation {
        out.push(whole);
        return;
    }
    let lowest = cpp::min3(a.z, b.z, c.z);
    let highest = cpp::max3(a.z, b.z, c.z);
    let first = colouring.band_of(lowest);
    let last = colouring.band_of(highest);
    if first == last || !lowest.is_finite() || !highest.is_finite() {
        whole.band = first;
        out.push(whole);
        return;
    }
    for k in first..=last {
        let mut piece = whole;
        piece.band = k;
        // Cut at the band's own edges, except where the triangle does not
        // reach past one: the first piece keeps what lies below its band
        // (none, by band_of), the last what lies above.
        if k > first
            && let Some(bottom) = colouring.band_low(k)
        {
            clip_to_level(&mut piece.corners, &mut piece.count, bottom, true);
        }
        if k < last
            && piece.count >= 3
            && let Some(top) = colouring.band_high(k)
        {
            clip_to_level(&mut piece.corners, &mut piece.count, top, false);
        }
        if piece.count >= 3 {
            out.push(piece);
        }
    }
}

// ---- the legend ------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceLegendEntry {
    pub colour: Color,
    /// The values it stands for; an open end is absent. A gradient's entries
    /// are points (from == to).
    pub from: Option<f64>,
    pub to: Option<f64>,
    /// What a person reads beside the swatch: "24.892 - 26.285", "< 2",
    /// "33 +", "NE", "level". Empty on a gradient's inner samples.
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceLegend {
    pub faces: SurfaceFaces,
    /// "Elevation", "Slope %", "Aspect", "Flat".
    pub title: String,
    /// A smooth ramp or smooth stops: `entries` are colours from the lowest
    /// value to the highest, to be blended into one bar, labelled at its ends
    /// (each at its own value: `from`, which need not be evenly spaced).
    /// Otherwise one swatch an entry, lowest first. A collapsed range is not
    /// a gradient: it has the swatches below, at and above its point.
    pub gradient: bool,
    pub entries: Vec<SurfaceLegendEntry>,
}

/// A legend and whose it is - the surface and the style it is drawn in: what
/// a view stacks in its corner, one for each surface it draws in colours that
/// mean something. The plan view's paint and the 3D scene both produce them,
/// and both views draw them through one stack (docs/surface_display.md, "The
/// legend").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceLegendCard {
    pub surface: String,
    pub style: String,
    pub legend: SurfaceLegend,
}

/// The fewest decimals, at most three, that write every one of `values`
/// exactly to a part in a million of a unit: a legend's labels, so bands of
/// whole metres read "25 - 26" and the sample terrain's tenths of its range
/// read to the millimetre.
pub fn legend_decimals(values: &[f64]) -> i32 {
    let mut decimals = 0;
    for &value in values {
        if !value.is_finite() {
            continue;
        }
        // Within a millionth of a unit of a value written with d decimals:
        // 26.284699999999997, a tenth of the sample terrain's range added to
        // its lowest point, is not 26.28 but is 26.285.
        let d = (0..3)
            .find(|&d| {
                let power = 10.0_f64.powi(d);
                let scaled = value * power;
                (scaled - scaled.round()).abs() <= 1.0e-6 * power
            })
            .unwrap_or(3);
        decimals = std::cmp::max(decimals, d);
    }
    decimals
}

/// `value` with `decimals` decimals (clamped to 0 to 6), locale-independent
/// ("." always). std::to_chars' fixed form: a negative zero is written as a
/// zero, a negative value rounding to zero keeps its sign, and a value whose
/// digits would not fit the C++'s 64-character buffer is "?".
pub fn format_legend_value(value: f64, decimals: i32) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    let decimals = decimals.clamp(0, 6) as usize;
    let text = if value.is_nan() {
        String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        })
    } else if value.is_infinite() {
        String::from(if value < 0.0 { "-inf" } else { "inf" })
    } else {
        format!("{value:.decimals$}")
    };
    if text.len() > 64 {
        return String::from("?");
    }
    text
}

/// A range too narrow to cut into bands or stretch a ramp over is one point,
/// and a value is below it, at it or above it. Entries for the colours those
/// three really are - not N swatches of a "40 - 40" nobody can tell apart.
/// Slope is never below its point, which is 0 or a steepness the surface did
/// not reach.
fn collapsed_entries(colouring: &SurfaceColouring, legend: &mut SurfaceLegend) {
    let point = colouring.low;
    let text = format_legend_value(point, legend_decimals(&[point]));
    let colour_on = |side: i32| {
        if colouring.banded {
            colouring.band_colour(collapsed_band(colouring, side))
        } else {
            surface_ramp_colour(&colouring.ramp, collapsed_position(colouring, side))
        }
    };
    if !(colouring.faces == SurfaceFaces::Slope && point <= 0.0) {
        legend.entries.push(SurfaceLegendEntry {
            colour: colour_on(-1),
            from: None,
            to: Some(point),
            label: format!("< {text}"),
        });
    }
    legend.entries.push(SurfaceLegendEntry {
        colour: colour_on(0),
        from: Some(point),
        to: Some(point),
        label: text.clone(),
    });
    legend.entries.push(SurfaceLegendEntry {
        colour: colour_on(1),
        from: Some(point),
        to: None,
        label: format!("> {text}"),
    });
}

/// What `colouring` means, to be read off a picture. Empty entries for faces
/// that are not coloured (None).
pub fn surface_legend(colouring: &SurfaceColouring) -> SurfaceLegend {
    let mut legend = SurfaceLegend {
        faces: colouring.faces,
        ..SurfaceLegend::default()
    };
    match colouring.faces {
        SurfaceFaces::None => return legend,
        SurfaceFaces::Flat => {
            legend.title = String::from("Flat");
            legend.entries.push(SurfaceLegendEntry {
                colour: colouring.flat,
                from: None,
                to: None,
                label: String::from("surface"),
            });
            return legend;
        }
        SurfaceFaces::Aspect => {
            legend.title = String::from("Aspect");
            for sector in 0..ASPECT_SECTORS {
                legend.entries.push(SurfaceLegendEntry {
                    colour: aspect_sector_colour(sector),
                    from: None,
                    to: None,
                    label: String::from(aspect_sector_name(sector)),
                });
            }
            legend.entries.push(SurfaceLegendEntry {
                colour: colouring.flat,
                from: None,
                to: None,
                label: String::from("level"),
            });
            return legend;
        }
        SurfaceFaces::Elevation | SurfaceFaces::Slope => {}
    }
    legend.title = String::from(if colouring.faces == SurfaceFaces::Slope {
        "Slope %"
    } else {
        "Elevation"
    });

    if colouring.collapsed() {
        collapsed_entries(colouring, &mut legend);
        return legend;
    }
    if colouring.banded {
        let count = colouring.band_count();
        let mut edges = Vec::new();
        for k in 0..count {
            for edge in [colouring.band_low(k), colouring.band_high(k)]
                .into_iter()
                .flatten()
            {
                edges.push(edge);
            }
        }
        let decimals = legend_decimals(&edges);
        for k in 0..count {
            let from = colouring.band_low(k);
            let to = colouring.band_high(k);
            let label = match (from, to) {
                (Some(from), Some(to)) => format!(
                    "{} - {}",
                    format_legend_value(from, decimals),
                    format_legend_value(to, decimals)
                ),
                (None, Some(to)) => format!("< {}", format_legend_value(to, decimals)),
                (Some(from), None) => format!("{} +", format_legend_value(from, decimals)),
                // One stop: every value is in it.
                (None, None) => String::from("all"),
            };
            legend.entries.push(SurfaceLegendEntry {
                colour: colouring.band_colour(k),
                from,
                to,
                label,
            });
        }
        return legend;
    }

    legend.gradient = true;
    if !colouring.stops.is_empty() {
        let values: Vec<f64> = colouring.stops.iter().map(|stop| stop.value).collect();
        let decimals = legend_decimals(&values);
        for stop in &colouring.stops {
            legend.entries.push(SurfaceLegendEntry {
                colour: stop.colour,
                from: Some(stop.value),
                to: Some(stop.value),
                label: format_legend_value(stop.value, decimals),
            });
        }
        return legend;
    }
    // The ramp at its five stops and halfway between each: nine samples,
    // enough for a bar blended between them to show every stop's colour.
    const SAMPLES: i32 = 9;
    let decimals = legend_decimals(&[colouring.low, colouring.high]);
    for i in 0..SAMPLES {
        let t = f64::from(i) / f64::from(SAMPLES - 1);
        let value = colouring.low + (colouring.high - colouring.low) * t;
        let label = if i == 0 || i == SAMPLES - 1 {
            format_legend_value(
                if i == 0 {
                    colouring.low
                } else {
                    colouring.high
                },
                decimals,
            )
        } else {
            String::new()
        };
        legend.entries.push(SurfaceLegendEntry {
            colour: colouring.colour_at(value),
            from: Some(value),
            to: Some(value),
            label,
        });
    }
    legend
}

// The SURFACE word of each face kind, as the SURFACESTYLE verbs write it, is
// surface_faces_word (surface_style.rs).
