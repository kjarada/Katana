//! Walking a path and emitting its pen-down spans (include/katana/cad/dashing.hpp).
//!
//! THE RULE THIS FILE EXISTS TO ENFORCE: a dash length is a MODEL length. A
//! 0.5 m dash is half a metre of ground at every zoom, and the number of
//! dashes on a 10 km boundary is twenty thousand times the number on a 0.5 m
//! one.
//!
//! The easy implementation is the wrong one: hand the renderer a fixed pattern
//! in pixels. That looks correct in a screenshot and is exactly backwards - the
//! dashes stay the same size as you zoom, so a 10 m fence and a 10 km boundary
//! get identical dashes. Nothing in this pipeline may express a pattern in
//! pixels except the single final conversion in the 2D viewport, which is where
//! the painter's units force it.
//!
//! PHASE starts at the first vertex of the entity's own path and is measured
//! along that path. It is a property of the entity, not of the view, so dashes
//! do not crawl while panning and do not redistribute while zooming. Both
//! renderers must therefore traverse the vertices in the same order and close a
//! closed path the same way.
//!
//! WHEN NOT TO DASH. Below roughly one device pixel per element, every dash and
//! gap falls inside a single pixel and the antialiased result is a uniformly
//! paler line: the user sees the wrong COLOUR, not a dash pattern. That is a
//! display decision, not a geometric tolerance, so it is NOT in
//! math::tolerance.

use katana_entity::Linetype;
use katana_geometry::Point2;
use katana_math::cpp;

/// Everything that decides how a pattern is laid down, in one place, so that
/// LTSCALE and a per-entity scale become parameters here rather than a hunt
/// through two renderers. Both are pinned at 1 today.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DashOptions {
    /// Multiplies every element length. DXF $LTSCALE times the entity's own
    /// scale (group code 48); the product, not either factor.
    pub pattern_scale: f64,
    /// Pixels per model unit, from the view transform. Used only to decide
    /// whether the pattern is resolvable, never to size a dash.
    pub view_scale: f64,
    /// A pattern whose shortest element falls below this many device pixels
    /// is drawn solid instead. One pixel is the point below which dashes stop
    /// being visible as dashes and start being a paler line.
    pub minimum_element_pixels: f64,
    /// Upper bound on emitted spans for one path. A dashed 10 km boundary at a
    /// 0.1 m pattern is 100 000 spans; beyond the budget the caller is told to
    /// draw the whole path solid rather than being handed a truncated one.
    pub maximum_spans: usize,
}

impl Default for DashOptions {
    fn default() -> Self {
        Self {
            pattern_scale: 1.0,
            view_scale: 1.0,
            minimum_element_pixels: 1.0,
            maximum_spans: 20_000,
        }
    }
}

fn effective_scale(options: &DashOptions) -> f64 {
    // One place where the scale factors are combined, so that adding a
    // per-entity scale later is a change here and nowhere else.
    if options.pattern_scale.is_finite() && options.pattern_scale > 0.0 {
        options.pattern_scale
    } else {
        1.0
    }
}

/// Whether a pattern should be laid down at all at this scale.
pub fn should_dash(linetype: &Linetype, options: &DashOptions) -> bool {
    if linetype.is_continuous() {
        return false;
    }
    let scale = effective_scale(options);
    if !(linetype.pattern_length() * scale > 0.0) {
        return false;
    }
    if !options.view_scale.is_finite() || options.view_scale <= 0.0 {
        return false;
    }
    let shortest = linetype.shortest_element();
    if !(shortest > 0.0) {
        // Every element is a dot: there is no length to resolve, so the dots
        // are laid out by pattern length alone and are always drawable.
        return linetype.pattern_length() * scale * options.view_scale
            >= options.minimum_element_pixels;
    }
    shortest * scale * options.view_scale >= options.minimum_element_pixels
}

/// Walks `points` (plus the closing segment when `closed`) in MODEL space and
/// calls `emit(from, to)` once per pen-down span. A dot is emitted as a
/// zero-length span (from == to) and the caller decides how to draw one.
///
/// Returns false when the pattern is continuous, is not resolvable at this
/// scale, or would exceed `maximum_spans`. In every one of those cases NOTHING
/// has been emitted and the caller must draw the path solid.
///
/// The all-or-nothing contract is deliberate. An earlier design truncated at
/// the budget, which silently shortened a long dashed line - a 900 m fence
/// drawn as 300 m, with no error anywhere.
#[allow(
    clippy::needless_range_loop,
    reason = "the index is the segment's number: it names both of its ends, the closing one wrapping"
)]
pub fn for_each_dash(
    points: &[Point2],
    closed: bool,
    linetype: &Linetype,
    options: &DashOptions,
    mut emit: impl FnMut(Point2, Point2),
) -> bool {
    if points.len() < 2 || !should_dash(linetype, options) {
        return false;
    }
    let scale = effective_scale(options);
    let period = linetype.pattern_length() * scale;
    if !(period > 0.0) {
        return false;
    }

    // Total path length first, so the span count can be bounded BEFORE
    // anything is emitted. The contract is all-or-nothing: a caller told
    // "false" draws the whole path solid, and must never receive a truncated
    // one.
    let mut path_length = 0.0;
    for pair in points.windows(2) {
        path_length += pair[0].distance_to(pair[1]);
    }
    if closed {
        path_length += points[points.len() - 1].distance_to(points[0]);
    }
    if !(path_length > 0.0) {
        return false;
    }
    let mut pen_down_per_period: usize = 0;
    for element in &linetype.pattern {
        if element.is_pen_down() {
            pen_down_per_period += 1;
        }
    }
    let periods = path_length / period;
    // +1 per period for the span a segment boundary can split.
    if periods * (pen_down_per_period + 1) as f64 > options.maximum_spans as f64 {
        return false;
    }

    // Walk the path once, carrying the pattern cursor across vertices, so the
    // pattern is continuous along the whole path rather than restarting at
    // every vertex (DXF LWPOLYLINE group 70 bit 128, the $PLINEGEN default).
    let pattern = &linetype.pattern;
    let mut element: usize = 0;
    let mut remaining_in_element = pattern[0].length.abs() * scale;
    let mut pen_down = pattern[0].is_pen_down();
    let starts_with_dot = pattern[0].is_dot();
    if starts_with_dot {
        emit(points[0], points[0]);
        // A dot consumes no length, so step straight to the next element.
        element = 1 % pattern.len();
        remaining_in_element = pattern[element].length.abs() * scale;
        pen_down = pattern[element].is_pen_down();
    }

    let segment_count = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    for s in 0..segment_count {
        let a = points[s];
        let b = points[(s + 1) % points.len()];
        let segment_length = a.distance_to(b);
        if !(segment_length > 0.0) {
            continue;
        }
        let point_at = |distance: f64| {
            let t = cpp::clamp(distance / segment_length, 0.0, 1.0);
            a + (b - a) * t
        };

        let mut consumed = 0.0;
        while consumed < segment_length {
            let take = cpp::min(remaining_in_element, segment_length - consumed);
            if pen_down && take > 0.0 {
                emit(point_at(consumed), point_at(consumed + take));
            }
            consumed += take;
            remaining_in_element -= take;

            if remaining_in_element > 0.0 {
                break; // the element continues into the next segment
            }
            // Advance, skipping over dots by emitting them in place.
            loop {
                element = (element + 1) % pattern.len();
                let next = pattern[element];
                pen_down = next.is_pen_down();
                remaining_in_element = next.length.abs() * scale;
                if next.is_dot() {
                    emit(point_at(consumed), point_at(consumed));
                }
                if !(remaining_in_element <= 0.0 && consumed < segment_length) {
                    break;
                }
            }

            if remaining_in_element <= 0.0 {
                break; // every remaining element is a dot; stop to avoid spinning
            }
        }
    }
    true
}

/// The painter's dash array (Qt's QPen in the C++ window), in units of PEN
/// WIDTH - not pixels, which is the trap: the array is multiplied by the pen
/// width when it is drawn, so a 2 px pen doubles every entry. Alternates
/// on/off starting with on, as the pen requires.
///
/// Empty when the pattern should not be dashed at this scale (see
/// should_dash), in which case the caller uses a solid pen.
///
/// A dot becomes a short on-span rather than a zero, because a zero-length
/// on-span draws nothing at all with a flat cap.
pub fn qt_dash_pattern(
    linetype: &Linetype,
    options: &DashOptions,
    pen_width_pixels: f64,
) -> Vec<f64> {
    if !should_dash(linetype, options) {
        return Vec::new();
    }
    // The pen multiplies the array by its width when it draws, so the array
    // is in units of pen width and the conversion has to divide by it.
    // Forgetting this renders perfectly and makes every pattern as many times
    // too long as the pen is wide - the viewport draws at 1.5 and 2 px, so
    // 1.5x and 2x.
    let width = if pen_width_pixels.is_finite() && pen_width_pixels > 0.0 {
        pen_width_pixels
    } else {
        1.0 // a cosmetic pen is drawn one pixel wide
    };
    let scale = effective_scale(options) * options.view_scale / width;

    let mut qt = Vec::with_capacity(linetype.pattern.len());
    for element in &linetype.pattern {
        // A dot has no length; a zero-length on-span draws nothing under a
        // flat cap, so it becomes the thinnest mark the pen will render.
        let length = if element.is_dot() {
            width * 0.5
        } else {
            element.length.abs() * scale
        };
        qt.push(cpp::max(length, 0.01));
    }
    // The pen requires an even count: the array alternates on/off starting
    // with on, so an odd one would invert the meaning of every later entry
    // when it repeats. validate() already requires the pattern to end with a
    // gap, which makes it even, but a project loaded from elsewhere is not
    // this program's to trust.
    if !qt.len().is_multiple_of(2) {
        qt.push(0.01);
    }
    qt
}
