//! What a smart leader's note can say about the entity its tip is on
//! (docs/annotation.md, "Smart leaders").
//!
//! A leader whose tip names an entity (LeaderGeometry::tip_ref) can have a
//! note that is a TEMPLATE - its own (`fields`) or a label style's
//! (`label_style`) - in the label template language (label_text.rs). Its
//! fields are the values of that entity AT THE TIP. They are worked out
//! whenever the leader is drawn, so the note cannot disagree with the
//! geometry or the attributes however either was edited - and, as a label, a
//! line naming a value the target lacks is dropped rather than printed as
//! nothing (absent is not zero).
//!
//! A note may also read the drawing's SURFACES and ALIGNMENTS at its tip,
//! which this layer cannot see. They come in through a LeaderContext, two
//! functions and two lists, which the cad layer makes from a Document. The
//! values are named KIND.NAME, NAME the surface's or the alignment's name as
//! the drawing has it:
//!
//!   surface.NAME    the level of surface NAME at the tip
//!   dsurface.NAME   the target's level (z) minus it: positive ABOVE the surface
//!   cut.NAME        how far the target is BELOW the surface, present only there
//!   fill.NAME       how far it is above, present only there (or exactly on the
//!                   surface, a fill of nothing)
//!   station.NAME    the station of the tip along alignment NAME
//!   offset.NAME     its offset from the centreline: positive LEFT looking
//!                   along increasing station
//!   profile.NAME    the alignment's design profile level at that station
//!   dprofile.NAME   the target's level minus it: positive above the design

use std::fmt;
use std::sync::Arc;

use katana_core::error::{Error, ErrorCode, Status, make_error};
use katana_core::text::equals_ignoring_case;
use katana_geometry::{CurveSegment, Point2};
use katana_math::numerics::{PI, lengths_equal, tolerance};

use crate::anchor::{nearest_anchor, resolve_anchor};
use crate::entity::{
    AnchorPoint, AnchorRef, DimensionKind, Entity, Geometry, LeaderGeometry, heights_of, to_string,
};
use crate::label_text::{
    LabelQuantity, LabelValue, LabelValues, check_template, format_fixed, format_label,
    template_fields,
};
use crate::model::Model;
use crate::scalar;
use crate::value_parts::{add_arc_values, add_common_values, add_segment_values};

/// Where a point is along an alignment, as a context reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AlignmentPlace {
    pub station: f64,
    /// Positive LEFT looking along increasing station.
    pub offset: f64,
    /// The design profile's level at the station; absent with no profile, or
    /// at a station it does not reach.
    pub profile: Option<f64>,
}

/// The level of surface `name` at `at`; None when the drawing has no surface
/// of that name or `at` is off it. Shared and thread-safe, as a copy of the
/// context goes to a worker with a snapshot of the model.
pub type SurfaceLevel = Arc<dyn Fn(&str, Point2) -> Option<f64> + Send + Sync>;

/// Where `at` is along alignment `name`; None when the drawing has no such
/// alignment or `at` is beyond its ends.
pub type AlignmentPlaceFunction = Arc<dyn Fn(&str, Point2) -> Option<AlignmentPlace> + Send + Sync>;

#[derive(Clone, Default)]
pub struct LeaderContext {
    /// Unset: no surface is read.
    pub surface_level: Option<SurfaceLevel>,
    /// Unset: no alignment is read.
    pub alignment_place: Option<AlignmentPlaceFunction>,
    /// The drawing's surfaces and alignments by name, in its order: what
    /// LEADER VALUES and the Leaders manager offer values of, and what a
    /// refusal checks a name against.
    pub surfaces: Vec<String>,
    pub alignments: Vec<String>,
}

impl fmt::Debug for LeaderContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeaderContext")
            .field("surface_level", &self.surface_level.is_some())
            .field("alignment_place", &self.alignment_place.is_some())
            .field("surfaces", &self.surfaces)
            .field("alignments", &self.alignments)
            .finish()
    }
}

/// In the order HELP and LEADER VALUES list them: what every target has, then
/// the tip's level and a text's words, then the linework's, then the areas',
/// then a dimension's.
const LEADER_VALUES: [&str; 32] = [
    "id",
    "layer",
    "type",
    "code",
    "point",
    "description",
    "x",
    "y",
    "easting",
    "northing",
    "z",
    "rl",
    "text",
    "bearing",
    "distance",
    "length",
    "dx",
    "dy",
    "dz",
    "grade",
    "segment",
    "chainage",
    "vertices",
    "radius",
    "diameter",
    "chord",
    "delta",
    "tangent",
    "area",
    "perimeter",
    "measurement",
    "angle",
];

/// The context's kinds, a surface's first, then an alignment's.
const SURFACE_KINDS: [&str; 4] = ["surface", "dsurface", "cut", "fill"];
const ALIGNMENT_KINDS: [&str; 4] = ["station", "offset", "profile", "dprofile"];

fn is_surface_kind(kind: &str) -> bool {
    SURFACE_KINDS.contains(&kind)
}

fn put(values: &mut LabelValues, name: &str, quantity: LabelQuantity, number: f64) {
    values.insert(name.to_owned(), LabelValue::of(quantity, number));
}

fn put_level(values: &mut LabelValues, z: Option<f64>) {
    if let Some(z) = z {
        put(values, "z", LabelQuantity::Number, z);
        put(values, "rl", LabelQuantity::Number, z);
    }
}

/// The height `t` of the way from a vertex of height `a` to one of height
/// `b`: either end's own height AT that end, linear between two known heights,
/// and absent otherwise - a level is never made up from one end alone.
fn height_between(a: Option<f64>, b: Option<f64>, t: f64) -> Option<f64> {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    match (a, b) {
        (Some(a), Some(b)) => Some(a + (b - a) * t),
        _ => None,
    }
}

fn clamped_parameter(place: &AnchorRef) -> f64 {
    if place.parameter.is_finite() {
        scalar::clamp(place.parameter, 0.0, 1.0)
    } else {
        0.0
    }
}

/// How far along a line or an arc `place` is, as a fraction from its start;
/// None for a place that is not on it (an arc's centre).
fn fraction_along(place: &AnchorRef) -> Option<f64> {
    match place.point {
        AnchorPoint::Start => Some(0.0),
        AnchorPoint::End => Some(1.0),
        AnchorPoint::Mid => Some(0.5),
        AnchorPoint::Along => Some(clamped_parameter(place)),
        _ => None,
    }
}

/// The segment of a polyline `place` is on and how far along it, None for a
/// place not on the line (Inside). The last vertex of an open polyline is the
/// END of its last segment; every other vertex starts the segment after it.
/// By the vertex count and closure alone, so a curve polyline's segments are
/// found by the same rule.
fn segment_along(n: usize, closed: bool, place: &AnchorRef) -> Option<(usize, f64)> {
    if n < 2 {
        return None;
    }
    let segments = if closed { n } else { n - 1 };
    let at_vertex = |i: usize| -> Option<(usize, f64)> {
        if i >= n {
            return None;
        }
        Some(if i < segments {
            (i, 0.0)
        } else {
            (segments - 1, 1.0)
        })
    };
    match place.point {
        AnchorPoint::Start => at_vertex(0),
        AnchorPoint::End => at_vertex(n - 1),
        AnchorPoint::Vertex => at_vertex(place.index as usize),
        AnchorPoint::SegmentMid | AnchorPoint::Along => {
            if place.index as usize >= segments {
                return None;
            }
            let t = if place.point == AnchorPoint::SegmentMid {
                0.5
            } else {
                clamped_parameter(place)
            };
            Some((place.index as usize, t))
        }
        _ => None,
    }
}

/// Every value name a leader's template may use besides prop.NAME, in the
/// order LEADER VALUES and HELP list them. `length` is the WHOLE entity's - a
/// pipe's length, not the segment the tip landed on, which is `distance`.
/// `chainage` is the length along the entity from its start to the tip.
pub fn leader_value_names() -> Vec<&'static str> {
    LEADER_VALUES.to_vec()
}

/// The KINDs of the context, in the order LEADER VALUES lists them.
pub fn leader_context_kinds() -> Vec<&'static str> {
    SURFACE_KINDS
        .iter()
        .chain(ALIGNMENT_KINDS.iter())
        .copied()
        .collect()
}

/// Whether a context KIND reads a surface (surface, dsurface, cut, fill) and
/// not an alignment: what tells a rename of a surface from a rename of an
/// alignment.
pub fn is_surface_context_kind(kind: &str) -> bool {
    is_surface_kind(kind)
}

/// "dsurface.ground" -> {"dsurface", "ground"}.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextValueName<'a> {
    pub kind: &'a str,
    pub name: &'a str,
}

/// None for a name that is not KIND.NAME with a known kind and a NAME.
pub fn context_value_name(value: &str) -> Option<ContextValueName<'_>> {
    let dot = value.find('.')?;
    if dot + 1 >= value.len() {
        return None;
    }
    let kind = &value[..dot];
    if !leader_context_kinds().contains(&kind) {
        return None;
    }
    Some(ContextValueName {
        kind,
        name: &value[dot + 1..],
    })
}

/// What `name` is, None when a leader has no such value (prop.NAME included:
/// a property's quantity is its own). A KIND.NAME of the context is known
/// whatever NAME is: whether the drawing has it is the context's to say.
pub fn leader_value_quantity(name: &str) -> Option<LabelQuantity> {
    if let Some(context) = context_value_name(name) {
        // A station is a chainage, so {station.MC01:ch} writes 0+120.000; an
        // offset a length; the rest are levels and differences of level.
        return Some(match context.kind {
            "station" => LabelQuantity::Chainage,
            "offset" => LabelQuantity::Length,
            _ => LabelQuantity::Number,
        });
    }
    if !LEADER_VALUES.contains(&name) {
        return None;
    }
    Some(match name {
        "id" | "segment" | "vertices" => LabelQuantity::Integer,
        "layer" | "type" | "code" | "point" | "description" | "text" => LabelQuantity::Text,
        "z" | "rl" | "dz" | "grade" => LabelQuantity::Number,
        "bearing" => LabelQuantity::Bearing,
        "delta" | "angle" => LabelQuantity::Angle,
        "area" => LabelQuantity::Area,
        "chainage" => LabelQuantity::Chainage,
        _ => LabelQuantity::Length,
    })
}

/// Checks a leader's template: braces balanced, every field one of
/// leader_value_names(), a KIND.NAME of the context or a prop.NAME, every
/// step applicable. Refused with the first problem, named, as a label style's
/// template is.
pub fn check_leader_template(template_text: &str) -> Status {
    check_template(template_text, &leader_value_quantity, &|name: &str| {
        format!("a leader has no value '{name}' (LEADER VALUES lists them)")
    })
}

/// The target's own values at the place `reference` names (anchor_values),
/// and that place: what the context is read at.
fn target_values(
    target: &Entity,
    reference: &AnchorRef,
    tip: Point2,
    code_properties: &[String],
) -> (LabelValues, Point2) {
    let mut values = LabelValues::new();
    add_common_values(target, code_properties, &mut values);
    values.insert(
        String::from("type"),
        LabelValue::of_text(to_string(target.r#type())),
    );

    // The place the tip is at: the one `reference` names, or - when the
    // target no longer has it - the place on the target nearest the tip.
    let mut place = *reference;
    let mut at = resolve_anchor(target, reference);
    if at.is_none()
        && let Some(nearest) = nearest_anchor(target, tip)
    {
        place = nearest;
        at = resolve_anchor(target, &place);
    }
    let point = at.unwrap_or(tip);
    put(&mut values, "x", LabelQuantity::Length, point.x);
    put(&mut values, "easting", LabelQuantity::Length, point.x);
    put(&mut values, "y", LabelQuantity::Length, point.y);
    put(&mut values, "northing", LabelQuantity::Length, point.y);

    match &target.geometry {
        Geometry::Point(_) => put_level(&mut values, heights_of(&target.properties, 1)[0]),
        Geometry::Text(text) => {
            values.insert(String::from("text"), LabelValue::of_text(text.text.clone()));
            put_level(&mut values, heights_of(&target.properties, 1)[0]);
        }
        Geometry::Line(line) => {
            let z = heights_of(&target.properties, 2);
            add_segment_values(&mut values, line.start, line.end, 0, z[0], z[1]);
            if let Some(t) = fraction_along(&place) {
                put(
                    &mut values,
                    "chainage",
                    LabelQuantity::Chainage,
                    t * line.length(),
                );
                put_level(&mut values, height_between(z[0], z[1], t));
            }
        }
        Geometry::Arc(arc) => {
            add_arc_values(&mut values, arc);
            put(
                &mut values,
                "diameter",
                LabelQuantity::Length,
                2.0 * arc.radius,
            );
            let z = heights_of(&target.properties, 2);
            if let Some(t) = fraction_along(&place) {
                put(
                    &mut values,
                    "chainage",
                    LabelQuantity::Chainage,
                    t * arc.length(),
                );
                put_level(&mut values, height_between(z[0], z[1], t));
            }
        }
        Geometry::Circle(circle) => {
            put(&mut values, "radius", LabelQuantity::Length, circle.radius);
            put(
                &mut values,
                "diameter",
                LabelQuantity::Length,
                2.0 * circle.radius,
            );
            put(
                &mut values,
                "length",
                LabelQuantity::Length,
                circle.perimeter(),
            );
            put(
                &mut values,
                "perimeter",
                LabelQuantity::Length,
                circle.perimeter(),
            );
            put(&mut values, "area", LabelQuantity::Area, circle.area());
            put_level(&mut values, heights_of(&target.properties, 1)[0]);
        }
        Geometry::Polyline(polyline) => {
            let v = &polyline.vertices;
            let z = heights_of(&target.properties, v.len());
            if let Some((i, t)) = segment_along(v.len(), polyline.closed, &place) {
                let j = (i + 1) % v.len();
                add_segment_values(&mut values, v[i], v[j], i, z[i], z[j]);
                let mut before = 0.0;
                for k in 0..i {
                    before += v[k].distance_to(v[k + 1]);
                }
                put(
                    &mut values,
                    "chainage",
                    LabelQuantity::Chainage,
                    before + t * v[i].distance_to(v[j]),
                );
                put_level(&mut values, height_between(z[i], z[j], t));
            } else if !z.is_empty()
                && z.iter()
                    .all(|h| h.is_some() && z[0].is_some() && *h == z[0])
            {
                // Inside a figure every vertex of which is at one level - a
                // building pad - the tip is at that level too.
                put_level(&mut values, z[0]);
            }
            // The whole line's, over the segment's that add_segment_values set.
            put(
                &mut values,
                "length",
                LabelQuantity::Length,
                polyline.length(),
            );
            put(
                &mut values,
                "vertices",
                LabelQuantity::Integer,
                v.len() as f64,
            );
            if polyline.closed && v.len() >= 3 {
                put(&mut values, "area", LabelQuantity::Area, polyline.area());
                put(
                    &mut values,
                    "perimeter",
                    LabelQuantity::Length,
                    polyline.perimeter(),
                );
            }
        }
        Geometry::CurvePolyline(curve) => {
            // As a polyline, its heights its vertices' own; an arc segment
            // gives an arc's values, a straight one a segment's.
            let v = &curve.vertices;
            if let Some((i, t)) = segment_along(v.len(), curve.closed, &place) {
                let j = curve.segment_end(i);
                match curve.segment(i) {
                    CurveSegment::Arc(arc) => {
                        add_arc_values(&mut values, &arc);
                        put(
                            &mut values,
                            "segment",
                            LabelQuantity::Integer,
                            (i + 1) as f64,
                        );
                    }
                    CurveSegment::Segment(_) => add_segment_values(
                        &mut values,
                        v[i].position,
                        v[j].position,
                        i,
                        v[i].height,
                        v[j].height,
                    ),
                }
                let station = curve.station_of_vertex(i) + t * curve.segment_length(i);
                put(&mut values, "chainage", LabelQuantity::Chainage, station);
                put_level(&mut values, height_between(v[i].height, v[j].height, t));
            } else if !v.is_empty()
                && v.iter().all(|vertex| {
                    vertex.height.is_some() && v[0].height.is_some() && vertex.height == v[0].height
                })
            {
                put_level(&mut values, v[0].height);
            }
            put(&mut values, "length", LabelQuantity::Length, curve.length());
            put(
                &mut values,
                "vertices",
                LabelQuantity::Integer,
                v.len() as f64,
            );
            if curve.closed && v.len() >= 3 {
                put(&mut values, "area", LabelQuantity::Area, curve.area());
                put(
                    &mut values,
                    "perimeter",
                    LabelQuantity::Length,
                    curve.length(),
                );
            }
        }
        Geometry::Ellipse(ellipse) => {
            put(
                &mut values,
                "length",
                LabelQuantity::Length,
                ellipse.length(),
            );
            if ellipse.is_full() {
                put(
                    &mut values,
                    "perimeter",
                    LabelQuantity::Length,
                    ellipse.length(),
                );
                put(
                    &mut values,
                    "area",
                    LabelQuantity::Area,
                    PI * ellipse.major_radius() * ellipse.minor_radius(),
                );
            }
            put_level(&mut values, heights_of(&target.properties, 1)[0]);
        }
        Geometry::Spline(spline) => {
            if spline.check_structure().is_ok() {
                put(
                    &mut values,
                    "length",
                    LabelQuantity::Length,
                    spline.length(),
                );
            }
        }
        Geometry::Dimension(dimension) => {
            if dimension.kind == DimensionKind::Angular {
                put(
                    &mut values,
                    "angle",
                    LabelQuantity::Angle,
                    dimension.measurement(),
                );
            } else {
                put(
                    &mut values,
                    "measurement",
                    LabelQuantity::Length,
                    dimension.measurement(),
                );
            }
        }
        Geometry::Label(_) | Geometry::Leader(_) => {}
    }
    (values, point)
}

/// Every KIND.NAME of each surface and alignment `context` lists.
fn every_context_value(context: &LeaderContext) -> Vec<String> {
    let mut names = Vec::new();
    for surface in &context.surfaces {
        for kind in SURFACE_KINDS {
            names.push(format!("{kind}.{surface}"));
        }
    }
    for alignment in &context.alignments {
        for kind in ALIGNMENT_KINDS {
            names.push(format!("{kind}.{alignment}"));
        }
    }
    names
}

/// A leader's target's values at its tip, with the context values `names`
/// names (every one the context lists when None), and where they were read.
fn read_leader(
    model: &Model,
    leader: &LeaderGeometry,
    code_properties: &[String],
    context: &LeaderContext,
    names: Option<&[String]>,
) -> Option<(LabelValues, Point2)> {
    if !leader.tip_ref.associated() {
        return None;
    }
    let tip = *leader.vertices.first()?;
    let target = model.entities.find(leader.tip_ref.entity)?;
    let (mut values, at) = target_values(target, &leader.tip_ref, tip, code_properties);
    match names {
        Some(names) => add_context_values(&mut values, at, context, names),
        None => add_context_values(&mut values, at, context, &every_context_value(context)),
    }
    Some((values, at))
}

/// The values `target` offers at the place `reference` names on it
/// (reference.entity is not checked against target.id). `tip` is where the
/// tip is now: the place when `reference` names none the target still has (a
/// vertex since removed), in which case the nearest place on the target
/// stands for it. `code_properties` are where a survey code is looked for, as
/// for a label. With a context, also every KIND.NAME of each surface and
/// alignment it lists, read at that place.
pub fn anchor_values(
    target: &Entity,
    reference: &AnchorRef,
    tip: Point2,
    code_properties: &[String],
    context: &LeaderContext,
) -> LabelValues {
    let (mut values, at) = target_values(target, reference, tip, code_properties);
    add_context_values(&mut values, at, context, &every_context_value(context));
    values
}

/// The context values `names` name (others are passed over), read at `at` for
/// a target whose own values are `values` - its level is values' "z" - added
/// to `values`.
pub fn add_context_values(
    values: &mut LabelValues,
    at: Point2,
    context: &LeaderContext,
    names: &[String],
) {
    // The target's own level, which a difference is taken from.
    let level = values.get("z").map(|found| found.number);
    for value in names {
        let Some(parsed) = context_value_name(value) else {
            continue;
        };
        if values.contains_key(value.as_str()) {
            continue;
        }
        if is_surface_kind(parsed.kind) {
            let Some(ground) = context
                .surface_level
                .as_ref()
                .and_then(|surface_level| surface_level(parsed.name, at))
            else {
                continue;
            };
            if parsed.kind == "surface" {
                put(values, value, LabelQuantity::Number, ground);
                continue;
            }
            let Some(level) = level else {
                continue;
            };
            // On the surface is a fill of nothing, and "on" is within the
            // geometric tolerance: the surface's level is interpolated, so a
            // point exactly on it differs from it in the last bits, and some
            // of them would read as a cut of 4e-15 (docs/annotation.md).
            let above = if lengths_equal(level, ground, tolerance::GEOMETRIC) {
                0.0
            } else {
                level - ground
            };
            if parsed.kind == "dsurface" {
                put(values, value, LabelQuantity::Number, above);
            } else if parsed.kind == "cut" && above < 0.0 {
                put(values, value, LabelQuantity::Number, -above);
            } else if parsed.kind == "fill" && above >= 0.0 {
                put(values, value, LabelQuantity::Number, above);
            }
            continue;
        }
        let Some(place) = context
            .alignment_place
            .as_ref()
            .and_then(|alignment_place| alignment_place(parsed.name, at))
        else {
            continue;
        };
        if parsed.kind == "station" {
            put(values, value, LabelQuantity::Chainage, place.station);
        } else if parsed.kind == "offset" {
            put(values, value, LabelQuantity::Length, place.offset);
        } else if parsed.kind == "profile"
            && let Some(profile) = place.profile
        {
            put(values, value, LabelQuantity::Number, profile);
        } else if parsed.kind == "dprofile"
            && let (Some(profile), Some(level)) = (place.profile, level)
        {
            put(values, value, LabelQuantity::Number, level - profile);
        }
    }
}

/// Why the context value `name` is not in `values` read at `at`, in words -
/// "the tip is off surface ground", "the drawing has no alignment MC01" - or
/// empty when it is there or is no context value.
pub fn why_absent(name: &str, values: &LabelValues, at: Point2, context: &LeaderContext) -> String {
    let Some(parsed) = context_value_name(name) else {
        return String::new();
    };
    if values.contains_key(name) {
        return String::new();
    }
    let what = parsed.name;
    let has_level = values.contains_key("z");
    if is_surface_kind(parsed.kind) {
        let Some(surface_level) = &context.surface_level else {
            return String::from("no surfaces were given to read here");
        };
        // The matcher the surface store finds a surface by, so a refusal never
        // says there is no "Ground" when the store finds "ground".
        if !context
            .surfaces
            .iter()
            .any(|surface| equals_ignoring_case(surface.as_bytes(), what.as_bytes()))
        {
            return format!("the drawing has no surface {what}");
        }
        if surface_level(parsed.name, at).is_none() {
            return format!("the tip is off surface {what}");
        }
        if parsed.kind != "surface" && !has_level {
            return String::from("the target has no level");
        }
        if parsed.kind == "cut" {
            return format!("the target is not below surface {what}");
        }
        if parsed.kind == "fill" {
            return format!("the target is below surface {what}");
        }
        return String::new();
    }
    let Some(alignment_place) = &context.alignment_place else {
        return String::from("no alignments were given to read here");
    };
    if !context.alignments.iter().any(|alignment| alignment == what) {
        return format!("the drawing has no alignment {what}");
    }
    let Some(place) = alignment_place(parsed.name, at) else {
        return format!("the tip is beyond the ends of alignment {what}");
    };
    if (parsed.kind == "profile" || parsed.kind == "dprofile") && place.profile.is_none() {
        return format!(
            "alignment {what} has no design profile at station {}",
            format_fixed(place.station, 3)
        );
    }
    if parsed.kind == "dprofile" && !has_level {
        return String::from("the target has no level");
    }
    String::new()
}

/// Whether a leader's note is read off its target.
pub fn is_smart(leader: &LeaderGeometry) -> bool {
    leader.fields || !leader.label_style.is_empty()
}

/// The template a smart leader's note is made from: its own text, or its
/// label style's; None for a plain leader and for one whose label style is
/// not in the model.
pub fn leader_template(model: &Model, leader: &LeaderGeometry) -> Option<String> {
    if !leader.label_style.is_empty() {
        return model
            .label_styles
            .find(&leader.label_style)
            .map(|style| style.text.clone());
    }
    leader.fields.then(|| leader.text.clone())
}

/// The values of a leader's target at its tip, with every value of the
/// context (anchor_values); None when the tip names no entity or that entity
/// is gone.
pub fn leader_values(
    model: &Model,
    leader: &LeaderGeometry,
    code_properties: &[String],
    context: &LeaderContext,
) -> Option<LabelValues> {
    read_leader(model, leader, code_properties, context, None).map(|(values, _)| values)
}

/// The note as it is drawn: a plain leader's text, verbatim; a smart leader's
/// template filled from its target's values and the context values it names,
/// a line naming a value that is absent dropped - empty when every line was.
pub fn leader_note(
    model: &Model,
    leader: &LeaderGeometry,
    code_properties: &[String],
    context: &LeaderContext,
) -> String {
    if !is_smart(leader) {
        return leader.text.clone();
    }
    let Some(template_text) = leader_template(model, leader) else {
        return String::new();
    };
    // Only what the template names is read of the context: a note is made on
    // every paint, and a drawing may hold many surfaces.
    let names = template_fields(&template_text);
    let read = read_leader(model, leader, code_properties, context, Some(&names));
    match read {
        Some((values, _)) => format_label(&template_text, &values),
        None => format_label(&template_text, &LabelValues::new()),
    }
}

/// The value names `template_text` uses that `values` lacks, in the order the
/// template names them: what a refusal to make a leader that would say
/// nothing lists.
pub fn missing_values(template_text: &str, values: &LabelValues) -> Vec<String> {
    template_fields(template_text)
        .into_iter()
        .filter(|name| !values.contains_key(name.as_str()))
        .collect()
}

/// Refuses a smart leader that could not say anything: one whose tip names no
/// entity, or names one that is gone, or whose note would be empty - with the
/// values its template names that the target lacks, and for a value of the
/// context why it is absent (why_absent). A plain leader always passes.
pub fn check_leader_says_something(
    model: &Model,
    leader: &LeaderGeometry,
    code_properties: &[String],
    context: &LeaderContext,
) -> Status {
    if !is_smart(leader) {
        return Ok(());
    }
    if !leader.tip_ref.associated() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a smart leader reads the entity its tip is on: put the tip on one \
             (#id, #id.end, #id.inside or #id@x,y on the command line)",
        ));
    }
    let Some(target) = model.entities.find(leader.tip_ref.entity) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "entity does not exist",
            format!("id={}", leader.tip_ref.entity),
        ));
    };
    if leader_template(model, leader).is_none() {
        return Err(make_error(
            ErrorCode::NotFound,
            "label style does not exist",
            leader.label_style.as_str(),
        ));
    }
    if !leader_note(model, leader, code_properties, context).is_empty() {
        return Ok(());
    }
    let template_text = leader_template(model, leader).unwrap_or_default();
    let names = template_fields(&template_text);
    let read = read_leader(model, leader, code_properties, context, Some(&names));
    let empty = LabelValues::new();
    let values = read.as_ref().map_or(&empty, |(values, _)| values);
    let mut missing = String::new();
    for name in missing_values(&template_text, values) {
        // A value of the context says why it is not there: a tip off the
        // surface reads very like a misspelt surface otherwise.
        let why = read
            .as_ref()
            .map(|(values, at)| why_absent(&name, values, *at, context))
            .unwrap_or_default();
        let said = if why.is_empty() {
            name
        } else {
            format!("{name} ({why})")
        };
        if !missing.is_empty() {
            missing.push_str(", ");
        }
        missing.push_str(&said);
    }
    let what = if missing.is_empty() {
        String::from("none of the values its template names")
    } else {
        format!("no {missing}")
    };
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!(
            "the leader would say nothing: its target has {what} (LEADER VALUES lists what it has)"
        ),
        format!("id={} type={}", target.id, to_string(target.r#type())),
    ))
}
