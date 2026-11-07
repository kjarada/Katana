//! Drawing entities -> IFC, by classification (classification.rs).
//!
//! An entity becomes the class its rule names, or an IfcAnnotation. Its
//! geometry is written as the class expects it:
//!
//! ```text
//! an element's run (kerb, fence, pipe)   Axis Curve3D where every vertex
//!                                        has a height, else FootPrint
//!                                        Curve2D
//! an element at a point (pit, valve)     FootPrint GeometricCurveSet: the
//!                                        point, and the outline when a
//!                                        circle drew it
//! an annotation                          Annotation: Point, Curve2D or
//!                                        Curve3D, or Annotation2D for
//!                                        text, dimensions and leaders
//! ```
//!
//! Arcs are IfcIndexedPolyCurve arcs through three points of the true arc and
//! circles IfcCircle: never chords. A height is written where Katana has one
//! (entity::heights_of) and nowhere else.
//!
//! DRAINAGE from a .12da archive (the line carries its pipes as pipe.<i>.*
//! properties, each pit is a point carrying pit.*) is taken as the network it
//! is: pipe i an IfcPipeSegment from vertex i to vertex i + 1 of the line, its
//! centre at its inverts plus half its diameter, and each pit an
//! IfcDistributionChamberElement from its lowest connected invert to its top,
//! all in one IfcDistributionSystem per string.
//!
//! A SERVICES PLAN drawn from an AS 5488 schedule (UTILITY DRAW,
//! docs/subsurface_utilities.md, "Drawing the services": each run and point
//! carries what the grading found as utility.* properties) is laid out for a
//! plan, a layer per type and quality level. It is written as the service it
//! is: each run the class the schedule's own export gives the service
//! (classify_utility_run, from the same attributes), each point an
//! IfcAnnotation SURVEY, all of a service in one IfcDistributionSystem named
//! by its line, and each classified by its quality level in AS 5488.1-2019.
//!
//! Each entity keeps its layer (IfcPresentationLayerAssignment), its colour
//! (a curve style), its properties (Katana_Attributes) and where it came from
//! (Katana_Provenance). Labels are not written: what a label says and where
//! it stands are worked out for a view by the placer (cad/annotation), which
//! this layer cannot see; they are counted and reported.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use katana_core::text::parse_finite_double;
use katana_entity::entity::{
    Color, DimensionGeometry, DimensionKind, ELEVATION_PROPERTY, Entity, EntityId, EntityType,
    Geometry, PropertyMap, PropertyValue, TextGeometry, TextJustify, heights_of,
    property_to_string, to_string as entity_name,
};
use katana_entity::leader_values::leader_note;
use katana_entity::model::Model;
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveSegment, Ellipse2, Point2, Spline2, Vec2,
};
use katana_math::Vec3;
use katana_math::numerics::{RAD_TO_DEG, normalize_angle};
use katana_survey::subsurface::{
    self as sub, QualityLevel, UtilityAttributes, UtilityType, parse_quality_level,
    parse_utility_type,
};

use crate::builder::{Builder, PropertyList, add_entity_properties};
use crate::classification::{
    ClassificationRule, IfcClass, UtilityClass, classification_text, classify_entity,
    classify_utility_run, contains_word, default_classification_rules, service_system_for,
};
use crate::parts::{quality_level_reference, service_long_name};
use crate::step::step_real;
use crate::step_file::{Args, Id, list_of};
use crate::support::{lower, max_of, min_of, printf_fixed};

/// Two consecutive points closer than the model context's precision are one
/// point to every reader (GEM111), and are written once.
const SAME_POINT: f64 = 1e-5;

fn text(map: &PropertyMap, key: &str) -> String {
    map.get(key).map(property_to_string).unwrap_or_default()
}

fn number(map: &PropertyMap, key: &str) -> Option<f64> {
    match map.get(key)? {
        PropertyValue::Real(real) => Some(*real),
        PropertyValue::Integer(whole) => Some(*whole as f64),
        PropertyValue::Text(words) => parse_finite_double(words),
        PropertyValue::Boolean(_) => None,
    }
}

/// `snprintf(buffer, size, "%.3f", value)`.
fn format_number(value: f64) -> String {
    printf_fixed(value, 3)
}

/// The one height of an entity whose vertices all share it.
fn uniform_height(heights: &[Option<f64>]) -> Option<f64> {
    let first = (*heights.first()?)?;
    for height in heights {
        match height {
            Some(height) if *height == first => {}
            _ => return None,
        }
    }
    Some(first)
}

fn all_heights(heights: &[Option<f64>]) -> bool {
    !heights.is_empty() && heights.iter().all(Option::is_some)
}

/// The IfcDistributionSystemEnum values; anything else a system is called is
/// written USERDEFINED with that name as its object type.
fn is_system_enumeration(value: &str) -> bool {
    const VALUES: [&str; 51] = [
        "AIRCONDITIONING",
        "AUDIOVISUAL",
        "CATENARY_SYSTEM",
        "CHEMICAL",
        "CHILLEDWATER",
        "COMMUNICATION",
        "COMPRESSEDAIR",
        "CONDENSERWATER",
        "CONTROL",
        "CONVEYING",
        "DATA",
        "DISPOSAL",
        "DOMESTICCOLDWATER",
        "DOMESTICHOTWATER",
        "DRAINAGE",
        "EARTHING",
        "ELECTRICAL",
        "ELECTROACOUSTIC",
        "EXHAUST",
        "FIREPROTECTION",
        "FIXEDTRANSMISSIONNETWORK",
        "FUEL",
        "GAS",
        "HAZARDOUS",
        "HEATING",
        "LIGHTING",
        "LIGHTNINGPROTECTION",
        "MOBILENETWORK",
        "MONITORINGSYSTEM",
        "MUNICIPALSOLIDWASTE",
        "OIL",
        "OPERATIONAL",
        "OPERATIONALTELEPHONYSYSTEM",
        "OVERHEAD_CONTACTLINE_SYSTEM",
        "POWERGENERATION",
        "RAINWATER",
        "REFRIGERATION",
        "RETURN_CIRCUIT",
        "SECURITY",
        "SEWAGE",
        "SIGNAL",
        "STORMWATER",
        "TELEPHONE",
        "TV",
        "VACUUM",
        "VENT",
        "VENTILATION",
        "WASTEWATER",
        "WATERSUPPLY",
        "USERDEFINED",
        "NOTDEFINED",
    ];
    VALUES.contains(&value)
}

/// A .12da pit type ("1050 dia Access Chamber", "Headwall") as a chamber type.
fn pit_class(pit_type: &str) -> IfcClass {
    // Each byte that is not an ASCII letter or digit a blank, as the C
    // locale's isalnum has it.
    let mut words = String::from(" ");
    for &c in pit_type.as_bytes() {
        words.push(if c.is_ascii_alphanumeric() {
            char::from(c.to_ascii_uppercase())
        } else {
            ' '
        });
    }
    words.push(' ');
    let has = |word: &str| words.contains(&format!(" {word} "));
    if has("MANHOLE") || has("MH") {
        return IfcClass::new("IfcDistributionChamberElement", "MANHOLE", "");
    }
    if has("SUMP") {
        return IfcClass::new("IfcDistributionChamberElement", "SUMP", "");
    }
    if has("CHAMBER") {
        return IfcClass::new("IfcDistributionChamberElement", "INSPECTIONCHAMBER", "");
    }
    if has("PIT") || has("GULLY") || has("INLET") || has("SIP") || has("KIP") || has("GRATE") {
        return IfcClass::new("IfcDistributionChamberElement", "INSPECTIONPIT", "");
    }
    if has("HEADWALL") {
        // The structure a pipe discharges through at its outfall: a
        // drainage structure IFC has no type of chamber for.
        return IfcClass::new("IfcDistributionChamberElement", "USERDEFINED", "HEADWALL");
    }
    IfcClass::new("IfcDistributionChamberElement", "NOTDEFINED", "")
}

/// IfcBoxAlignment's words for a Katana justification.
fn box_alignment(justify: TextJustify) -> &'static str {
    match justify {
        TextJustify::BottomLeft => "bottom-left",
        TextJustify::BottomCentre => "bottom-middle",
        TextJustify::BottomRight => "bottom-right",
        TextJustify::MiddleLeft => "middle-left",
        TextJustify::MiddleCentre => "center",
        TextJustify::MiddleRight => "middle-right",
        TextJustify::TopLeft => "top-left",
        TextJustify::TopCentre => "top-middle",
        TextJustify::TopRight => "top-right",
    }
}

fn name_of(entity: &Entity) -> String {
    for candidate in [
        text(&entity.metadata, "12d.name"),
        text(&entity.properties, "point"),
        text(&entity.properties, "code"),
    ] {
        if !candidate.is_empty() {
            return candidate;
        }
    }
    format!("{} {}", entity_name(entity.r#type()), entity.id)
}

/// Whether `entity` is part of a plan UTILITY DRAW made: the names of the
/// utility.* properties are the drawing's documented record
/// (docs/subsurface_utilities.md, "What the grading found is on the
/// entities"; cad's utility drawing, which this layer cannot see). A run is a
/// polyline (a curve polyline when it has arcs), or a line an EXPLODE made of
/// one, which keeps its properties; a located point is a point.
fn is_drawn_run(entity: &Entity) -> bool {
    matches!(
        entity.geometry,
        Geometry::Polyline(_) | Geometry::CurvePolyline(_) | Geometry::Line(_)
    ) && !text(&entity.properties, "utility.line").is_empty()
        && !text(&entity.properties, "utility.type").is_empty()
}

fn is_drawn_point(entity: &Entity) -> bool {
    matches!(entity.geometry, Geometry::Point(_))
        && !text(&entity.properties, "utility.line").is_empty()
        && !text(&entity.properties, "utility.type").is_empty()
}

fn point_of(entity: &Entity) -> Point2 {
    match &entity.geometry {
        Geometry::Point(point) => point.position,
        _ => Point2::default(),
    }
}

/// The service's attributes, as UTILITY DRAW put them on a run.
fn attributes_of(run: &Entity) -> UtilityAttributes {
    let properties = &run.properties;
    let mut service = UtilityAttributes {
        r#type: parse_utility_type(&text(properties, "utility.type"))
            .unwrap_or(UtilityType::Unknown),
        owner: text(properties, "utility.owner"),
        material: text(properties, "utility.material"),
        configuration: text(properties, "utility.configuration"),
        description: text(properties, "utility.description"),
        ..UtilityAttributes::default()
    };
    for (name, value) in properties {
        if let Some(field) = name.strip_prefix("utility.field.") {
            service
                .fields
                .insert(String::from(field), property_to_string(value));
        }
    }
    service
}

/// What says one service from another among runs of one line id: two
/// schedules drawn under one prefix may both have an "E1", and E1 from each
/// is its own service, classed by its own attributes.
fn fingerprint_of(run: &Entity) -> String {
    let mut out = String::new();
    for (name, value) in &run.properties {
        if name == "utility.type"
            || name == "utility.owner"
            || name == "utility.material"
            || name == "utility.configuration"
            || name == "utility.description"
            || name.starts_with("utility.field.")
        {
            out.push_str(name);
            out.push('\u{1f}');
            out.push_str(&property_to_string(value));
            out.push('\u{1e}');
        }
    }
    out
}

/// Whether a vertex of `run` is at `at`, to the centimetre write_drainage
/// gives a pit to its string with.
fn passes_through(run: &Entity, at: Point2) -> bool {
    let near = |vertex: Point2| (vertex - at).length() < 0.01;
    match &run.geometry {
        Geometry::Polyline(polyline) => polyline.vertices.iter().any(|&vertex| near(vertex)),
        Geometry::CurvePolyline(curve) => curve.vertices.iter().any(|vertex| near(vertex.position)),
        Geometry::Line(segment) => near(segment.start) || near(segment.end),
        _ => false,
    }
}

/// The <prefix>/<type> a run was drawn under: the layer above its QL layer.
fn drawn_on_of(layer: &str) -> String {
    match layer.rfind('/') {
        Some(slash) => String::from(&layer[..slash]),
        None => String::new(),
    }
}

#[derive(Clone, Debug, Default)]
struct Drawn {
    items: Vec<Id>,
    /// the RepresentationType
    kind: String,
    in_3d: bool,
}

struct DrawnService<'m> {
    /// the <prefix>/<type> its runs were drawn under
    drawn_on: String,
    /// utility.line
    line: String,
    fingerprint: String,
    runs: Vec<&'m Entity>,
    points: Vec<&'m Entity>,
}

struct DrawingWriter<'b, 'a, 'm> {
    b: &'b mut Builder<'a>,
    model: &'m Model,
    rules: &'m [ClassificationRule],
    project_rules: Vec<ClassificationRule>,
    handled: BTreeSet<EntityId>,
    systems: BTreeMap<(String, String), Vec<Id>>,
    labels_skipped: usize,
}

impl<'b, 'a, 'm> DrawingWriter<'b, 'a, 'm> {
    fn new(
        builder: &'b mut Builder<'a>,
        model: &'m Model,
        rules: &'m [ClassificationRule],
    ) -> Self {
        // A project's own rules: what the caller gave that is not one of the
        // defaults (a RULES file's, which the front ends put before them).
        let defaults = default_classification_rules();
        let project_rules = rules
            .iter()
            .filter(|rule| !defaults.contains(rule))
            .cloned()
            .collect();
        Self {
            b: builder,
            model,
            rules,
            project_rules,
            handled: BTreeSet::new(),
            systems: BTreeMap::new(),
            labels_skipped: 0,
        }
    }

    fn write(&mut self) {
        let model = self.model;
        let mut chosen: Vec<&'m Entity> = Vec::new();
        for id in model.entities.ids() {
            if let Some(entity) = model.entities.find(id)
                && self.selected(entity)
            {
                chosen.push(entity);
            }
        }
        self.write_drawn_services(&chosen);
        self.write_drainage(&chosen);
        for &entity in &chosen {
            if self.handled.contains(&entity.id) {
                continue;
            }
            self.write_entity(entity);
        }
        self.write_systems();
        if self.labels_skipped > 0 {
            let count = self.labels_skipped;
            self.b.warn(format!(
                "{count}{} not written: a label's words and place are worked out for a view",
                if count == 1 {
                    " label was"
                } else {
                    " labels were"
                }
            ));
        }
    }

    fn selected(&self, entity: &Entity) -> bool {
        let options = self.b.options();
        if !options.entities.is_empty() && !options.entities.contains(&entity.id) {
            return false;
        }
        if !options.layers.is_empty() && !options.layers.contains(&entity.layer) {
            return false;
        }
        true
    }

    fn colour_of(&self, entity: &Entity) -> Color {
        if let Some(colour) = entity.color {
            return colour;
        }
        self.model
            .layers
            .find(&entity.layer)
            .map_or_else(Color::default, |layer| layer.color)
    }

    // ---- geometry ---------------------------------------------------------------

    /// A polyline through `plan` at `heights` (all present, or ignored),
    /// consecutive repeats dropped; closed by referring back to its first
    /// point, as GEM111 asks. 0 when fewer than two points remain (the points
    /// already written stay in the file, as the C++ leaves them).
    fn curve(
        &mut self,
        plan: &[Point2],
        heights: &[Option<f64>],
        mut closed: bool,
        in_3d: bool,
    ) -> Id {
        let mut points = Vec::new();
        let mut previous: Option<Vec3> = None;
        let mut first: Option<Vec3> = None;
        for (i, p) in plan.iter().enumerate() {
            let z = if in_3d {
                heights.get(i).copied().flatten().unwrap_or(0.0)
            } else {
                0.0
            };
            let at = Vec3::new(p.x, p.y, z);
            if let Some(previous) = previous
                && (at - previous).length() < SAME_POINT
            {
                continue;
            }
            previous = Some(at);
            first.get_or_insert(at);
            points.push(if in_3d {
                self.b.point_3d(at)
            } else {
                self.b.point(Vec2::new(at.x, at.y))
            });
        }
        // A last point on the first is the closing point, whether or not the
        // polyline says it is closed.
        if points.len() > 2
            && let (Some(first), Some(previous)) = (first, previous)
            && (previous - first).length() < SAME_POINT
        {
            points.pop();
            closed = true;
        }
        if points.len() < 2 {
            return 0;
        }
        if closed && points.len() > 2 {
            points.push(points[0]);
        }
        self.b.file().add("IfcPolyline", Args::new().refs(&points))
    }

    /// One point of a point list, in the file's local coordinates.
    fn coordinate(&self, p: Point2, height: Option<f64>) -> String {
        match height {
            Some(z) => {
                let at = self.b.local_3d(Vec3::new(p.x, p.y, z));
                list_of(&[step_real(at.x), step_real(at.y), step_real(at.z)])
            }
            None => {
                let at = self.b.local(p);
                list_of(&[step_real(at.x), step_real(at.y)])
            }
        }
    }

    /// An arc as an IfcIndexedPolyCurve through its start, middle and end.
    fn arc(&mut self, shape: &Arc2, height: Option<f64>) -> Id {
        let points = [shape.start_point(), shape.midpoint(), shape.end_point()];
        let coordinates: Vec<String> = points.iter().map(|&p| self.coordinate(p, height)).collect();
        let list = self.b.file().add(
            if height.is_some() {
                "IfcCartesianPointList3D"
            } else {
                "IfcCartesianPointList2D"
            },
            Args::new().raw(&list_of(&coordinates)).null(),
        );
        self.b.file().add(
            "IfcIndexedPolyCurve",
            Args::new()
                .reference(list)
                .raw("(IFCARCINDEX((1,2,3)))")
                .boolean(false),
        )
    }

    /// A curve polyline as ONE IfcIndexedPolyCurve: an IfcLineIndex per
    /// straight segment and an IfcArcIndex through the true midpoint of each
    /// arc, so its arcs stay arcs. The midpoints follow the vertices in the
    /// point list; a midpoint's height is its ends' mean, the height halfway
    /// along the segment by the rule the 3D view uses (height_at_station).
    /// 0 with fewer than two vertices.
    fn curve_polyline(&mut self, shape: &CurvePolyline2, in_3d: bool) -> Id {
        if shape.vertices.len() < 2 {
            return 0;
        }
        let height_of = |vertex: usize| -> f64 {
            if in_3d {
                shape.vertices[vertex].height.unwrap_or(0.0)
            } else {
                0.0
            }
        };
        let mut coordinates: Vec<String> = Vec::new();
        // IFC indices count from 1.
        let mut add = |this: &Self, p: Point2, z: f64| -> String {
            coordinates.push(this.coordinate(p, in_3d.then_some(z)));
            format!("{}", coordinates.len())
        };
        let mut vertex_index = Vec::with_capacity(shape.vertices.len());
        for (i, vertex) in shape.vertices.iter().enumerate() {
            vertex_index.push(add(self, vertex.position, height_of(i)));
        }
        let mut segments = Vec::new();
        for (i, start) in vertex_index.iter().enumerate().take(shape.segment_count()) {
            let end = shape.segment_end(i);
            match shape.segment(i) {
                CurveSegment::Arc(arc_piece) => {
                    let middle = add(
                        self,
                        arc_piece.midpoint(),
                        (height_of(i) + height_of(end)) * 0.5,
                    );
                    segments.push(format!(
                        "IFCARCINDEX(({start},{middle},{}))",
                        vertex_index[end]
                    ));
                }
                CurveSegment::Segment(_) => {
                    segments.push(format!("IFCLINEINDEX(({start},{}))", vertex_index[end]));
                }
            }
        }
        let list = self.b.file().add(
            if in_3d {
                "IfcCartesianPointList3D"
            } else {
                "IfcCartesianPointList2D"
            },
            Args::new().raw(&list_of(&coordinates)).null(),
        );
        self.b.file().add(
            "IfcIndexedPolyCurve",
            Args::new()
                .reference(list)
                .raw(&list_of(&segments))
                .boolean(false),
        )
    }

    /// An ellipse as IfcEllipse, its placement's x axis along the major axis;
    /// an elliptical arc as that ellipse trimmed by parameter. IFC's conic
    /// parameter is the eccentric anomaly in the file's plane angle unit
    /// (radians, builder.rs), which is exactly Ellipse2's start_parameter and
    /// sweep, so the trim needs no conversion.
    fn ellipse(&mut self, shape: &Ellipse2, height: Option<f64>) -> Id {
        let length = shape.major_axis.length();
        let ux = shape.major_axis.x / length;
        let uy = shape.major_axis.y / length;
        let placement = match height {
            Some(z) => {
                let centre = self
                    .b
                    .point_3d(Vec3::new(shape.center.x, shape.center.y, z));
                let axis = self.b.direction_3d(ux, uy, 0.0);
                self.b.file().add(
                    "IfcAxis2Placement3D",
                    Args::new().reference(centre).null().reference(axis),
                )
            }
            None => {
                let centre = self.b.point(Vec2::new(shape.center.x, shape.center.y));
                let axis = self.b.direction(ux, uy);
                self.b.file().add(
                    "IfcAxis2Placement2D",
                    Args::new().reference(centre).reference(axis),
                )
            }
        };
        let basis = self.b.file().add(
            "IfcEllipse",
            Args::new()
                .reference(placement)
                .real(shape.major_radius())
                .real(shape.minor_radius()),
        );
        if shape.is_full() {
            return basis;
        }
        self.b.file().add(
            "IfcTrimmedCurve",
            Args::new()
                .reference(basis)
                // Each in [0, 2 pi): with the sense true, an end below the
                // start runs through the parameter 0.
                .raw(&format!(
                    "(IFCPARAMETERVALUE({}))",
                    step_real(normalize_angle(shape.start_parameter))
                ))
                .raw(&format!(
                    "(IFCPARAMETERVALUE({}))",
                    step_real(normalize_angle(shape.end_parameter()))
                ))
                .boolean(true)
                .enumeration("PARAMETER"),
        )
    }

    /// A spline as IfcBSplineCurveWithKnots - IfcRationalBSplineCurveWithKnots
    /// when it has weights - from its control points and knots, the knots
    /// written as IFC asks: each distinct value once, with its multiplicity.
    /// Its fit points are how it was drawn, not what it is, and IFC has no
    /// place for them.
    fn spline(&mut self, shape: &Spline2) -> Id {
        let mut points = Vec::with_capacity(shape.control_points.len());
        for p in &shape.control_points {
            points.push(self.b.point(Vec2::new(p.x, p.y)));
        }
        let mut multiplicities = Vec::new();
        let mut knots = Vec::new();
        let mut i = 0;
        while i < shape.knots.len() {
            let mut j = i;
            while j < shape.knots.len() && shape.knots[j] == shape.knots[i] {
                j += 1;
            }
            multiplicities.push(format!("{}", j - i));
            knots.push(shape.knots[i]);
            i = j;
        }
        let mut args = Args::new();
        args.integer(i64::from(shape.degree))
            .refs(&points)
            .enumeration("UNSPECIFIED")
            .raw(".F.")
            .raw(".U.")
            .raw(&list_of(&multiplicities))
            .reals(&knots)
            .enumeration("UNSPECIFIED");
        if shape.weights.is_empty() {
            return self.b.file().add("IfcBSplineCurveWithKnots", &args);
        }
        args.reals(&shape.weights);
        self.b.file().add("IfcRationalBSplineCurveWithKnots", &args)
    }

    fn circle(&mut self, shape: &Circle2, height: Option<f64>) -> Id {
        let placement = match height {
            Some(z) => {
                let centre = self
                    .b
                    .point_3d(Vec3::new(shape.center.x, shape.center.y, z));
                self.b.file().add(
                    "IfcAxis2Placement3D",
                    Args::new().reference(centre).null().null(),
                )
            }
            None => {
                let centre = self.b.point(Vec2::new(shape.center.x, shape.center.y));
                self.b
                    .file()
                    .add("IfcAxis2Placement2D", Args::new().reference(centre).null())
            }
        };
        self.b.file().add(
            "IfcCircle",
            Args::new().reference(placement).real(shape.radius),
        )
    }

    /// Text, as IfcTextLiteralWithExtent: IFC 4.3 deprecates IfcTextLiteral
    /// (IFC102), and the extent is what the current one asks for. There is no
    /// font here, so the box is ESTIMATED as the .12da import estimates a
    /// text's width - 0.6 of the height per character of its longest line -
    /// and a line's height per line; the corner the box alignment names sits
    /// on the text's justification point, as Katana places it. Written in
    /// plan: an Annotation2D's text is two-dimensional.
    fn text_literal(&mut self, shape: &TextGeometry) -> Id {
        let mut lines: usize = 1;
        let mut widest: usize = 0;
        let mut current: usize = 0;
        // Each byte, then the end of the text, which ends the last line.
        let bytes = shape.text.bytes().map(Some).chain(std::iter::once(None));
        for byte in bytes {
            match byte {
                None | Some(b'\n') => {
                    widest = widest.max(current);
                    current = 0;
                    if byte.is_some() {
                        lines += 1;
                    }
                }
                // a character, not a UTF-8 continuation byte
                Some(c) if (c & 0xC0) != 0x80 => current += 1,
                Some(_) => {}
            }
        }
        let location = self.b.point(Vec2::new(shape.position.x, shape.position.y));
        let direction = self.b.direction(shape.rotation.cos(), shape.rotation.sin());
        let placement = self.b.file().add(
            "IfcAxis2Placement2D",
            Args::new().reference(location).reference(direction),
        );
        let extent = self.b.file().add(
            "IfcPlanarExtent",
            Args::new()
                .real(0.6 * shape.height * widest.max(1) as f64)
                .real(shape.height * lines as f64),
        );
        self.b.file().add(
            "IfcTextLiteralWithExtent",
            Args::new()
                .string(&shape.text)
                .reference(placement)
                .enumeration("RIGHT")
                .reference(extent)
                .string(box_alignment(shape.justify)),
        )
    }

    /// The value of a dimension, or a leader's note, at the entity model's
    /// default text height (TextGeometry::height): a dimension's own text
    /// size is on paper, and this layer has no plot scale to turn it into
    /// model units.
    fn text_at(&mut self, at: Point2, rotation: f64, words: &str) -> Id {
        let shape = TextGeometry {
            position: at,
            rotation,
            text: String::from(words),
            ..TextGeometry::default()
        };
        self.text_literal(&shape)
    }

    /// The geometry of `entity` as IFC representation items.
    fn draw(&mut self, entity: &Entity) -> Drawn {
        let mut drawn = Drawn::default();
        match &entity.geometry {
            Geometry::Point(shape) => {
                let heights = heights_of(&entity.properties, 1);
                drawn.in_3d = heights[0].is_some();
                let item = match heights[0] {
                    Some(z) => self
                        .b
                        .point_3d(Vec3::new(shape.position.x, shape.position.y, z)),
                    None => self.b.point(Vec2::new(shape.position.x, shape.position.y)),
                };
                drawn.items.push(item);
                drawn.kind = String::from("Point");
            }
            Geometry::Line(shape) => {
                let heights = heights_of(&entity.properties, 2);
                drawn.in_3d = all_heights(&heights);
                let id = self.curve(&[shape.start, shape.end], &heights, false, drawn.in_3d);
                if id != 0 {
                    drawn.items.push(id);
                }
            }
            Geometry::Polyline(shape) => {
                let heights = heights_of(&entity.properties, shape.vertices.len());
                drawn.in_3d = all_heights(&heights);
                let id = self.curve(&shape.vertices, &heights, shape.closed, drawn.in_3d);
                if id != 0 {
                    drawn.items.push(id);
                }
            }
            Geometry::Arc(shape) => {
                let height = uniform_height(&heights_of(&entity.properties, 2));
                drawn.in_3d = height.is_some();
                let id = self.arc(shape, height);
                drawn.items.push(id);
            }
            Geometry::Circle(shape) => {
                let height = uniform_height(&heights_of(&entity.properties, 1));
                drawn.in_3d = height.is_some();
                let id = self.circle(shape, height);
                drawn.items.push(id);
            }
            Geometry::Text(shape) => {
                let id = self.text_literal(shape);
                drawn.items.push(id);
                drawn.kind = String::from("Annotation2D");
            }
            Geometry::Dimension(shape) => {
                self.draw_dimension(shape, &mut drawn);
                drawn.kind = String::from("Annotation2D");
            }
            Geometry::Leader(shape) => {
                let id = self.curve(&shape.vertices, &[], false, false);
                if id != 0 {
                    drawn.items.push(id);
                }
                // The note the leader SAYS, not shape.text: for a smart
                // leader that is its template ("RL {rl:.3f}"), and for one
                // made from a label style it is empty. The plan, LIST and the
                // DXF all write the note (entity::leader_note).
                let options = self.b.options();
                let note = leader_note(
                    self.model,
                    shape,
                    &options.code_properties,
                    &options.leader_context,
                );
                // A leader is validated to have at least two vertices; the
                // C++ reads the last unchecked.
                if !note.is_empty()
                    && let Some(&tip) = shape.vertices.last()
                {
                    let id = self.text_at(tip, 0.0, &note);
                    drawn.items.push(id);
                }
                drawn.kind = String::from("Annotation2D");
            }
            Geometry::CurvePolyline(shape) => {
                // Its heights are its vertices' own, not the elevation
                // properties a Polyline2 keeps them in.
                drawn.in_3d = !shape.vertices.is_empty()
                    && shape.vertices.iter().all(|vertex| vertex.height.is_some());
                let id = self.curve_polyline(shape, drawn.in_3d);
                if id != 0 {
                    drawn.items.push(id);
                }
            }
            Geometry::Ellipse(shape) => {
                let height = uniform_height(&heights_of(&entity.properties, 1));
                drawn.in_3d = height.is_some();
                let id = self.ellipse(shape, height);
                drawn.items.push(id);
            }
            Geometry::Spline(shape) => {
                let id = self.spline(shape);
                drawn.items.push(id);
            }
            Geometry::Label(_) => {}
        }
        if drawn.kind.is_empty() && !drawn.items.is_empty() {
            drawn.kind = String::from(if drawn.in_3d { "Curve3D" } else { "Curve2D" });
        }
        drawn
    }

    fn draw_dimension(&mut self, shape: &DimensionGeometry, drawn: &mut Drawn) {
        let measured = shape.measurement();
        let words = if shape.text_override.is_empty() {
            if shape.kind == DimensionKind::Angular {
                format!("{}\u{b0}", format_number(measured * RAD_TO_DEG))
            } else {
                format_number(measured)
            }
        } else {
            shape.text_override.clone()
        };
        if shape.kind == DimensionKind::Aligned {
            let along = shape.end - shape.start;
            let length = along.length();
            if length > 0.0 {
                let left = Vec2::new(-along.y, along.x) * (shape.offset / length);
                let a = shape.start + left;
                let b = shape.end + left;
                for line in [[shape.start, a], [shape.end, b], [a, b]] {
                    let id = self.curve(&line, &[], false, false);
                    if id != 0 {
                        drawn.items.push(id);
                    }
                }
                let id = self.text_at((a + b) * 0.5, along.y.atan2(along.x), &words);
                drawn.items.push(id);
                return;
            }
        }
        // The other kinds as the points they measure between, and the value.
        let mut points = vec![shape.start];
        if shape.uses_vertex() {
            points.push(shape.vertex);
        }
        points.push(shape.end);
        let id = self.curve(&points, &[], false, false);
        if id != 0 {
            drawn.items.push(id);
        }
        let id = self.text_at(shape.end, 0.0, &words);
        drawn.items.push(id);
    }

    // ---- entities -----------------------------------------------------------------

    fn write_entity(&mut self, entity: &Entity) {
        let layer_source = format!("layer {}", entity.layer);
        if entity.r#type() == EntityType::Label {
            self.labels_skipped += 1;
            self.b.report().entities_skipped += 1;
            self.b.tally(
                &layer_source,
                &IfcClass::default(),
                "",
                "a label: labels are not exported",
            );
            return;
        }
        let rules = self.rules;
        let classified = classify_entity(entity, rules);
        let annotation = classified.ifc_class.entity == "IfcAnnotation";
        let drawn = self.draw(entity);
        if drawn.items.is_empty() {
            self.b.report().entities_skipped += 1;
            self.b.warn(format!(
                "{} {} has no extent to write and is not written",
                entity_name(entity.r#type()),
                entity.id
            ));
            self.b
                .tally(&layer_source, &IfcClass::default(), "", "nothing to draw");
            return;
        }

        let representation = self.representation_of(entity, &drawn, annotation);

        let key = format!("entity/{}", entity.id);
        let shape = self.b.product_shape(&[representation]);
        let product = self.b.product(
            &classified.ifc_class,
            &key,
            &name_of(entity),
            &classified.rule,
            shape,
            &format!("{}", entity.id),
        );
        self.b.report().entities_written += 1;
        let why = if classified.rule.is_empty() {
            format!("no rule: a {}", lower(entity_name(entity.r#type())))
        } else {
            format!("rule {}", classified.rule)
        };
        self.b.tally(
            &layer_source,
            &classified.ifc_class,
            &classified.system,
            &why,
        );

        if !classified.system.is_empty() {
            self.systems
                .entry((entity.layer.clone(), classified.system.clone()))
                .or_default()
                .push(product);
        }
        let classified_by = if classified.rule.is_empty() {
            String::from("kind")
        } else {
            format!("rule {}", classified.rule)
        };
        self.write_entity_properties(entity, &key, product, &classified_by, &classified.ifc_class);
    }

    /// The entity's shape as its class expects it, styled in its colour and
    /// on its layer.
    fn representation_of(&mut self, entity: &Entity, drawn: &Drawn, annotation: bool) -> Id {
        let representation = if annotation {
            let context = self.b.annotation_context();
            self.b
                .shape(context, "Annotation", &drawn.kind, &drawn.items)
        } else if drawn.kind == "Point" || entity.r#type() == EntityType::Circle {
            // An element at a point, or outlined by a circle: its footprint.
            let mut items = drawn.items.clone();
            if drawn.kind == "Point" && drawn.in_3d {
                let position = point_of(entity);
                items = vec![self.b.point(Vec2::new(position.x, position.y))];
            } else if drawn.in_3d
                && let Geometry::Circle(circle) = &entity.geometry
            {
                items = vec![self.circle(circle, None)];
            }
            let set = self
                .b
                .file()
                .add("IfcGeometricCurveSet", Args::new().refs(&items));
            let context = self.b.foot_print_context();
            self.b
                .shape(context, "FootPrint", "GeometricCurveSet", &[set])
        } else if drawn.in_3d {
            let context = self.b.axis_context();
            self.b.shape(context, "Axis", "Curve3D", &drawn.items)
        } else {
            let context = self.b.foot_print_context();
            self.b.shape(context, "FootPrint", "Curve2D", &drawn.items)
        };
        if drawn.kind != "Point" {
            let colour = self.colour_of(entity);
            for &item in &drawn.items {
                self.b.colour_curve(item, &colour);
            }
        }
        self.b.layer(&entity.layer, representation);
        representation
    }

    /// Its properties, and where it came from: `classified_by` says what chose
    /// `ifc_class`.
    fn write_entity_properties(
        &mut self,
        entity: &Entity,
        key: &str,
        product: Id,
        classified_by: &str,
        ifc_class: &IfcClass,
    ) {
        let mut attributes = PropertyList::new();
        add_entity_properties(&mut attributes, &entity.properties);
        let set = self.b.property_set(
            &format!("{key}/attributes"),
            "Katana_Attributes",
            &attributes,
        );
        self.b
            .defines(&format!("{key}/attributes"), set, &[product]);

        let mut provenance = PropertyList::new();
        provenance.integer("EntityId", Some(entity.id as i64));
        provenance.label("Layer", &entity.layer);
        provenance.label("Style", &entity.style);
        provenance.label("ClassifiedBy", classified_by);
        for (name, value) in &entity.metadata {
            // What the archive knew and Katana keeps only to write back
            // (12d.x.*, symbols, text formatting) is not the entity's
            // provenance.
            if name.starts_with("12d.x.")
                || name.starts_with("12d.symbol.")
                || name.starts_with("12d.text.")
            {
                continue;
            }
            provenance.label(name, &property_to_string(value));
        }
        let set = self.b.property_set(
            &format!("{key}/provenance"),
            "Katana_Provenance",
            &provenance,
        );
        self.b
            .defines(&format!("{key}/provenance"), set, &[product]);

        if ifc_class.predefined_type == "CONTOURLINE" {
            let mut contour = PropertyList::new();
            // A curve polyline's heights are its vertices' own.
            let heights = match &entity.geometry {
                Geometry::CurvePolyline(curve) => curve.heights(),
                Geometry::Polyline(polyline) => {
                    heights_of(&entity.properties, polyline.vertices.len())
                }
                _ => heights_of(&entity.properties, 2),
            };
            contour.length("ContourValue", uniform_height(&heights));
            let set = self.b.property_set(
                &format!("{key}/contour"),
                "Pset_AnnotationContourLine",
                &contour,
            );
            self.b.defines(&format!("{key}/contour"), set, &[product]);
        }
    }

    /// One IfcDistributionSystem per layer and system, for the service
    /// elements the drawing's rules found.
    fn write_systems(&mut self) {
        let systems = std::mem::take(&mut self.systems);
        for ((layer, system), members) in &systems {
            let listed = is_system_enumeration(system);
            let key = format!("system/{layer}/{system}");
            let guid = self.b.guid(&key);
            let id = self.b.file().add(
                "IfcDistributionSystem",
                Args::new()
                    .string(&guid)
                    .null()
                    .string(layer)
                    .null()
                    .string_or_null(if listed { "" } else { system.as_str() })
                    .null()
                    .enumeration(if listed {
                        system.as_str()
                    } else {
                        "USERDEFINED"
                    }),
            );
            *self
                .b
                .report()
                .classes
                .entry(String::from("IfcDistributionSystem"))
                .or_insert(0) += 1;
            self.b.reference_in_site(id);
            self.b.group(id, &key, members);
        }
        self.systems = systems;
    }

    // ---- services drawn from an AS 5488 schedule -------------------------------------

    /// The services of a drawn plan, then each written. A service is the runs
    /// of one line id drawn under one <prefix>/<type> (the layer above their
    /// QL layer) with the same attributes; each point goes to the service it
    /// stands on - a vertex of one of its runs, as a pit goes to its drainage
    /// string - whatever layer it has since been moved to, and only when it
    /// stands on none to the one service of its line and type, or to its own
    /// layer's.
    fn write_drawn_services(&mut self, chosen: &[&'m Entity]) {
        let mut services: Vec<DrawnService<'m>> = Vec::new();
        for &entity in chosen {
            if !is_drawn_run(entity) {
                continue;
            }
            let drawn_on = drawn_on_of(&entity.layer);
            let line = text(&entity.properties, "utility.line");
            let fingerprint = fingerprint_of(entity);
            let found = services.iter().position(|service| {
                service.drawn_on == drawn_on
                    && service.line == line
                    && service.fingerprint == fingerprint
            });
            let index = match found {
                Some(index) => index,
                None => {
                    services.push(DrawnService {
                        drawn_on,
                        line,
                        fingerprint,
                        runs: Vec::new(),
                        points: Vec::new(),
                    });
                    services.len() - 1
                }
            };
            services[index].runs.push(entity);
        }
        for &entity in chosen {
            if !is_drawn_point(entity) {
                continue;
            }
            let line = text(&entity.properties, "utility.line");
            let utility = text(&entity.properties, "utility.type");
            let at = point_of(entity);
            // A service that has no run (one a point began) is not of the
            // line: the C++ reads its first run unchecked there.
            let same_line = |service: &DrawnService<'_>| {
                service.line == line
                    && service
                        .runs
                        .first()
                        .is_some_and(|run| text(&run.properties, "utility.type") == utility)
            };
            let stands_on = |service: &DrawnService<'_>| {
                same_line(service) && service.runs.iter().any(|run| passes_through(run, at))
            };
            let mut owner = services.iter().position(stands_on);
            if owner.is_none() && services.iter().filter(|service| same_line(service)).count() == 1
            {
                owner = services.iter().position(same_line);
            }
            let owner = match owner {
                Some(owner) => owner,
                None => {
                    let drawn_on = drawn_on_of(&entity.layer);
                    match services.iter().position(|service| {
                        service.runs.is_empty()
                            && service.drawn_on == drawn_on
                            && service.line == line
                    }) {
                        Some(owner) => owner,
                        None => {
                            services.push(DrawnService {
                                drawn_on,
                                line: line.clone(),
                                fingerprint: String::new(),
                                runs: Vec::new(),
                                points: Vec::new(),
                            });
                            services.len() - 1
                        }
                    }
                }
            };
            services[owner].points.push(entity);
        }

        // A line id drawn from services that differ is said, and each
        // written as its own system: "E1" and "E1 (2)" in the report.
        let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
        for service in &services {
            let nth = {
                let count = seen
                    .entry((service.drawn_on.clone(), service.line.clone()))
                    .or_insert(0);
                *count += 1;
                *count
            };
            if nth == 2 {
                self.b.warn(format!(
                    "line {} under {} was drawn from services with different attributes (two \
                     schedules with one line id?): each is written as its own system",
                    service.line, service.drawn_on
                ));
            }
            self.write_drawn_service(service, nth);
        }
    }

    fn write_drawn_service(&mut self, drawn: &DrawnService<'m>, nth: usize) {
        // Every run of the service carries its attributes alike (they are its
        // fingerprint); a service drawn as points alone has only its type.
        let Some(&first) = drawn.runs.first().or_else(|| drawn.points.first()) else {
            return;
        };
        let service = attributes_of(first);
        let run_class = classify_utility_run(&service);
        let system_name = if run_class.system == "USERDEFINED" {
            run_class.system_object_type.clone()
        } else {
            run_class.system.clone()
        };
        let numbered = if nth == 1 {
            drawn.line.clone()
        } else {
            format!("{} ({nth})", drawn.line)
        };
        let key = format!("drawn service/{}/{numbered}", drawn.drawn_on);
        // Named apart from the schedule's "service W1" rows, and with where it
        // was drawn, so the report names each system the file holds once.
        let source = if drawn.drawn_on.is_empty() {
            format!("drawn service {numbered}")
        } else {
            format!("drawn service {numbered} in {}", drawn.drawn_on)
        };
        if self.b.is_schedule_service(&format!(
            "{}/{}",
            drawn.line,
            sub::to_string(service.r#type)
        )) {
            self.b.warn(format!(
                "{source} is also in the schedule given with UTILITIES: the file holds it twice, \
                 drawn and graded; leave out one (NOENTITIES, or no UTILITIES)"
            ));
        }

        let mut written = Vec::new();
        for &entity in &drawn.runs {
            written.push(self.write_drawn_member(
                entity,
                false,
                &drawn.line,
                &run_class,
                &system_name,
                &source,
            ));
        }
        for &entity in &drawn.points {
            written.push(self.write_drawn_member(
                entity,
                true,
                &drawn.line,
                &run_class,
                &system_name,
                &source,
            ));
        }
        written.retain(|&id| id != 0);
        if written.is_empty() {
            return; // nothing of the service was written: no system of nothing
        }
        // As the schedule's export writes a service's system (utilities.rs).
        let guid = self.b.guid(&key);
        let name = self.b.label(&numbered, &source);
        let long_name = service_long_name(&service);
        let id = self.b.file().add(
            "IfcDistributionSystem",
            Args::new()
                .string(&guid)
                .null()
                .string(&name)
                .string_or_null(&service.description)
                .string_or_null(&run_class.system_object_type)
                .string_or_null(&long_name)
                .enumeration(&run_class.system),
        );
        *self
            .b
            .report()
            .classes
            .entry(String::from("IfcDistributionSystem"))
            .or_insert(0) += 1;
        self.b.reference_in_site(id);
        self.b.group(id, &key, &written);
    }

    /// One run or located point of a drawn service; 0 when it has nothing to
    /// draw, which is said.
    #[allow(
        clippy::too_many_arguments,
        reason = "The parameters of the C++'s writeDrawnMember, in its order."
    )]
    fn write_drawn_member(
        &mut self,
        entity: &Entity,
        point: bool,
        line: &str,
        run_class: &UtilityClass,
        system_name: &str,
        source: &str,
    ) -> Id {
        self.handled.insert(entity.id);
        // A project's own rules still name the class of what it drew (a
        // "gas flexible" rule makes a gas run a FLEXIBLESEGMENT); only the
        // defaults give way to the service's own class.
        let ruled = classify_entity(entity, &self.project_rules);
        let by_rule = !ruled.rule.is_empty();
        let ifc_class = if by_rule {
            ruled.ifc_class.clone()
        } else if point {
            IfcClass::new("IfcAnnotation", "SURVEY", "")
        } else {
            run_class.element.clone()
        };
        let system = if by_rule && !ruled.system.is_empty() {
            ruled.system.clone()
        } else {
            String::from(system_name)
        };

        // A located point at its level where the drawing kept one (the
        // level the grading took it at, as the schedule's export places its
        // points); in plan where it did not.
        let level = if point {
            number(&entity.properties, "utility.service_level")
        } else {
            None
        };
        let shape = match level {
            Some(level) => {
                let at = point_of(entity);
                Drawn {
                    items: vec![self.b.point_3d(Vec3::new(at.x, at.y, level))],
                    kind: String::from("Point"),
                    in_3d: true,
                }
            }
            None => self.draw(entity),
        };
        if shape.items.is_empty() {
            self.b.report().entities_skipped += 1;
            self.b.warn(format!(
                "{source}: {} {} has no extent to write and is not written",
                entity_name(entity.r#type()),
                entity.id
            ));
            self.b
                .tally(source, &IfcClass::default(), "", "nothing to draw");
            return 0;
        }
        let annotation = ifc_class.entity == "IfcAnnotation";
        let representation = self.representation_of(entity, &shape, annotation);

        // The level the drawing recorded, read as AS 5488's or not at all: a
        // level the standard does not have is said, and written unclassified.
        let recorded = text(&entity.properties, "utility.quality_level");
        let graded = parse_quality_level(&recorded);
        if graded.is_none() {
            let said = if recorded.is_empty() {
                String::from(" has no quality level")
            } else {
                format!(
                    " has quality level \"{recorded}\", which is not one of AS 5488.1-2019's \
                     QL-A to QL-D"
                )
            };
            self.b.warn(format!(
                "{source}: {} {}{said}: it is written unclassified",
                entity_name(entity.r#type()),
                entity.id
            ));
        }
        let entity_key = format!("entity/{}", entity.id);
        let name = if point {
            text(&entity.properties, "utility.vertex")
        } else {
            format!(
                "{line} {} to {}",
                text(&entity.properties, "utility.from"),
                text(&entity.properties, "utility.to")
            )
        };
        let description = if point {
            format!("Located point of {line}")
        } else {
            match graded {
                Some(graded) => format!("AS 5488.1-2019 {} run of {line}", sub::to_string(graded)),
                None => format!("Run of {line}, its quality level not one of AS 5488's"),
            }
        };
        let product_shape = self.b.product_shape(&[representation]);
        let product = self.b.product(
            &ifc_class,
            &entity_key,
            if name.is_empty() { line } else { name.as_str() },
            &description,
            product_shape,
            &format!("{}", entity.id),
        );
        self.b.report().entities_written += 1;
        let why = if by_rule {
            format!("rule {}", ruled.rule)
        } else if point {
            String::from("a drawn located point")
        } else {
            format!("a drawn run: {}", run_class.reason)
        };
        self.b.tally(source, &ifc_class, &system, &why);
        if let Some(graded) = graded {
            let reference = quality_level_reference(self.b, graded);
            self.b.associate(reference, product);
        }
        self.write_drawn_grade(entity, &entity_key, product, point, graded);
        let classified_by = if by_rule {
            format!("rule {}", ruled.rule)
        } else {
            String::from("a drawn AS 5488 service")
        };
        self.write_entity_properties(entity, &entity_key, product, &classified_by, &ifc_class);
        product
    }

    /// What the grading found, in the property sets the schedule's own
    /// export writes (utilities.rs), from what the drawing kept of it: a
    /// run's AS5488_QualityLevel, a point's AS5488_LocatedPoint. What the
    /// drawing does not keep - a run's claimed level, its path evidence, a
    /// point's recorded level and depth - is absent, not guessed; the level
    /// the grading took a point at, which the drawing keeps, is ServiceLevel,
    /// since the schedule's Level is only a recorded one.
    fn write_drawn_grade(
        &mut self,
        entity: &Entity,
        key: &str,
        product: Id,
        point: bool,
        graded: Option<QualityLevel>,
    ) {
        let p = &entity.properties;
        let flag = |name: &str| -> Option<bool> {
            match p.get(name)? {
                PropertyValue::Boolean(value) => Some(*value),
                _ => None,
            }
        };
        let mut grade = PropertyList::new();
        if let Some(graded) = graded {
            grade.label_specified(
                "QualityLevel",
                sub::to_string(graded),
                if point {
                    "The AS 5488.1-2019 quality level the point's evidence supports"
                } else {
                    "The AS 5488.1-2019 quality level the run's evidence supports"
                },
            );
        }
        if point {
            grade.label("LocateMethod", &text(p, "utility.method"));
            grade.label("QualityLevelClaimed", &text(p, "utility.claimed"));
            grade.text("OverClaim", &text(p, "utility.over_claim"));
            grade.label("LevelReference", &text(p, "utility.level_ref"));
            grade.length("ServiceLevel", number(p, "utility.service_level"));
            grade.boolean("LevelQualified", flag("utility.level_qualified"));
            grade.length("SurfaceLevel", number(p, "utility.surface_level"));
            grade.length("DepthOfCover", number(p, "utility.cover"));
            grade.text("CoverNote", &text(p, "utility.cover_note"));
            grade.boolean("CoverBelowMinimum", flag("utility.cover_below_minimum"));
            grade.identifier("Verifies", &text(p, "utility.verifies"));
        } else {
            grade.text("LimitedBy", &text(p, "utility.limited_by"));
            grade.identifier("StartPoint", &text(p, "utility.from"));
            grade.identifier("EndPoint", &text(p, "utility.to"));
            grade.length("PlanLength", number(p, "utility.length"));
        }
        grade.label("Standard", "AS 5488.1-2019");
        let name = if point {
            "AS5488_LocatedPoint"
        } else {
            "AS5488_QualityLevel"
        };
        let set = self.b.property_set(&format!("{key}/grade"), name, &grade);
        self.b.defines(&format!("{key}/grade"), set, &[product]);
    }

    // ---- drainage from a .12da archive -------------------------------------------

    fn write_drainage(&mut self, chosen: &[&'m Entity]) {
        // The strings, and their pits and house connections, by the string
        // they came from: the archive gives them one header (layer and name).
        let mut parts: BTreeMap<(String, String), Vec<&'m Entity>> = BTreeMap::new();
        let mut lines: Vec<&'m Entity> = Vec::new();
        for &entity in chosen {
            if self.handled.contains(&entity.id) {
                continue; // already written, as a drawn service
            }
            let element = text(&entity.metadata, "12d.element");
            if element == "string drainage" && matches!(entity.geometry, Geometry::Polyline(_)) {
                lines.push(entity);
            } else if element == "drainage pit" || element == "drainage house_connection" {
                parts
                    .entry((entity.layer.clone(), text(&entity.metadata, "12d.name")))
                    .or_default()
                    .push(entity);
            }
        }
        // Each pit and connection goes to ONE string. The archive names a
        // string's parts by its header, and two strings may share a name - or
        // have none - so a part goes to the string of its name it stands on (a
        // vertex of it), and to the first of its name only when it stands on
        // none; otherwise every pit of the name would be written once per
        // string.
        let mut parts_of: HashMap<EntityId, Vec<&'m Entity>> = HashMap::new();
        for ((layer, name), members) in &parts {
            let named: Vec<&'m Entity> = lines
                .iter()
                .copied()
                .filter(|line| line.layer == *layer && text(&line.metadata, "12d.name") == *name)
                .collect();
            let Some(&first) = named.first() else {
                continue; // no string of its name: written as the entity it is
            };
            for &part in members {
                let mut owner = first;
                if let Geometry::Point(point) = &part.geometry {
                    let stands_on = |line: &&Entity| match &line.geometry {
                        Geometry::Polyline(polyline) => polyline
                            .vertices
                            .iter()
                            .any(|&v| (v - point.position).length() < 0.01),
                        _ => false,
                    };
                    if let Some(&found) = named.iter().find(|line| stands_on(line)) {
                        owner = found;
                    }
                }
                parts_of.entry(owner.id).or_default().push(part);
            }
        }
        for &line in &lines {
            let members = parts_of.get(&line.id).cloned().unwrap_or_default();
            self.write_drainage_string(line, &members);
        }
    }

    fn write_drainage_string(&mut self, line: &Entity, parts: &[&Entity]) {
        let Geometry::Polyline(shape) = &line.geometry else {
            return;
        };
        let name = name_of(line);
        let key = format!("drainage/{}", line.id);
        let mut system = service_system_for(&classification_text(line));
        if system.is_empty() {
            // an archive's drainage string with nothing else said drains
            // stormwater
            system = String::from("STORMWATER");
        }
        let heights = heights_of(&line.properties, shape.vertices.len());
        let flow = number(&line.properties, "flow_direction");

        // The pipes, pipe i from vertex i to vertex i + 1.
        let mut pipes: usize = 0;
        while line
            .properties
            .contains_key(&format!("pipe.{}.diameter", pipes + 1))
            || line
                .properties
                .contains_key(&format!("pipe.{}.name", pipes + 1))
        {
            pipes += 1;
        }
        let mut members = Vec::new();
        // The lowest invert at each vertex, for the pits' depths.
        let mut invert_at: Vec<Option<f64>> = vec![None; shape.vertices.len()];
        let layer_source = format!("layer {}", line.layer);
        if pipes == 0 || pipes + 1 != shape.vertices.len() {
            let drawn = self.draw(line);
            if drawn.items.is_empty() {
                // Nothing to draw: not written, and accounted for as not.
                self.b.warn(format!(
                    "drainage string \"{name}\" has {pipes} pipes for {} vertices and no extent \
                     to write; it is not written",
                    shape.vertices.len()
                ));
                self.b.report().entities_skipped += 1;
                self.b
                    .tally(&layer_source, &IfcClass::default(), "", "nothing to draw");
            } else {
                self.b.warn(format!(
                    "drainage string \"{name}\" has {pipes} pipes for {} vertices; it is written \
                     as one pipe along its line",
                    shape.vertices.len()
                ));
                let representation = if drawn.in_3d {
                    let context = self.b.axis_context();
                    self.b.shape(context, "Axis", "Curve3D", &drawn.items)
                } else {
                    let context = self.b.foot_print_context();
                    self.b.shape(context, "FootPrint", "Curve2D", &drawn.items)
                };
                self.b.layer(&line.layer, representation);
                let pipe_class = IfcClass::new("IfcPipeSegment", "RIGIDSEGMENT", "");
                let product_shape = self.b.product_shape(&[representation]);
                members.push(self.b.product(
                    &pipe_class,
                    &format!("{key}/line"),
                    &name,
                    "drainage string",
                    product_shape,
                    &format!("{}", line.id),
                ));
                self.b.report().entities_written += 1;
                self.b.tally(
                    &layer_source,
                    &pipe_class,
                    &system,
                    "drainage string, its pipes not given: one pipe along it",
                );
            }
        } else {
            for i in 0..pipes {
                members.push(self.write_pipe(
                    line,
                    i,
                    &shape.vertices,
                    &heights,
                    flow,
                    &mut invert_at,
                    &key,
                    &system,
                ));
            }
            self.b.report().entities_written += 1;
        }
        self.handled.insert(line.id);

        for &part in parts {
            if !matches!(part.geometry, Geometry::Point(_)) {
                continue;
            }
            members.push(self.write_pit(part, &shape.vertices, &invert_at, &system));
            self.handled.insert(part.id);
            self.b.report().entities_written += 1;
        }
        if members.is_empty() {
            return; // nothing of the string was written: no system of nothing
        }
        let listed = is_system_enumeration(&system);
        let guid = self.b.guid(&key);
        let id = self.b.file().add(
            "IfcDistributionSystem",
            Args::new()
                .string(&guid)
                .null()
                .string(&name)
                .string("drainage string")
                .string_or_null(if listed { "" } else { system.as_str() })
                .null()
                .enumeration(if listed {
                    system.as_str()
                } else {
                    "USERDEFINED"
                }),
        );
        *self
            .b
            .report()
            .classes
            .entry(String::from("IfcDistributionSystem"))
            .or_insert(0) += 1;
        self.b.reference_in_site(id);
        self.b.group(id, &key, &members);
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The parameters of the C++'s writePipe, in its order."
    )]
    fn write_pipe(
        &mut self,
        line: &Entity,
        index: usize,
        vertices: &[Point2],
        heights: &[Option<f64>],
        flow: Option<f64>,
        invert_at: &mut [Option<f64>],
        key: &str,
        system: &str,
    ) -> Id {
        let prefix = format!("pipe.{}.", index + 1);
        let diameter = number(&line.properties, &format!("{prefix}diameter"));
        let upstream = number(&line.properties, &format!("{prefix}us_level"));
        let downstream = number(&line.properties, &format!("{prefix}ds_level"));
        let a = vertices[index];
        let b = vertices[index + 1];

        // Which end is upstream. The string's own heights at its vertices
        // say it where there are any; otherwise a flow_direction of 1 puts
        // it at the string's start, as every .12da file seen writes it. Any
        // other case is not guessed at.
        let mut inverts: Option<(f64, f64)> = None; // at a, at b
        let mut how = String::new();
        if let (Some(upstream), Some(downstream)) = (upstream, downstream) {
            if let (Some(height_a), Some(height_b)) = (heights[index], heights[index + 1]) {
                let forward = (upstream - height_a).abs() + (downstream - height_b).abs();
                let backward = (upstream - height_b).abs() + (downstream - height_a).abs();
                inverts = Some(if forward <= backward {
                    (upstream, downstream)
                } else {
                    (downstream, upstream)
                });
                how = String::from("upstream end by the string's vertex levels");
            } else if flow == Some(1.0) {
                inverts = Some((upstream, downstream));
                how = String::from("upstream end at the string's start (flow_direction 1)");
            } else {
                how = String::from("the upstream end is not known: drawn in plan");
            }
        }
        if let Some((at_a, at_b)) = inverts {
            invert_at[index] = Some(invert_at[index].map_or(at_a, |known| min_of(known, at_a)));
            invert_at[index + 1] =
                Some(invert_at[index + 1].map_or(at_b, |known| min_of(known, at_b)));
        }

        let mut representations = Vec::new();
        match (inverts, diameter) {
            (Some((at_a, at_b)), Some(diameter)) if diameter > 0.0 => {
                let r = diameter / 2.0;
                let axis = self.b.polyline_3d(
                    &[Vec3::new(a.x, a.y, at_a + r), Vec3::new(b.x, b.y, at_b + r)],
                    false,
                );
                let context = self.b.axis_context();
                representations.push(self.b.shape(context, "Axis", "Curve3D", &[axis]));
                let body = self.b.file().add(
                    "IfcSweptDiskSolid",
                    Args::new().reference(axis).real(r).null().null().null(),
                );
                let context = self.b.body_context();
                representations.push(self.b.shape(context, "Body", "AdvancedSweptSolid", &[body]));
            }
            _ => {
                let footprint = self.b.polyline(&[a, b], false);
                let context = self.b.foot_print_context();
                representations.push(self.b.shape(context, "FootPrint", "Curve2D", &[footprint]));
            }
        }
        for &representation in &representations {
            self.b.layer(&line.layer, representation);
        }
        let pipe_name = text(&line.properties, &format!("{prefix}name"));
        let pipe_type = text(&line.properties, &format!("{prefix}type"));
        let mut pipe_class = IfcClass::new("IfcPipeSegment", "RIGIDSEGMENT", "");
        // The type is lowered here, as the C++ has it, so a culvert named only
        // by its pipe type is never found: the needle is upper case.
        if contains_word(
            &format!("{} {}", classification_text(line), lower(&pipe_type)),
            "CULVERT*",
        ) {
            pipe_class.predefined_type = String::from("CULVERT");
        }
        let pipe_key = format!("{key}/pipe/{}", index + 1);
        let product_shape = self.b.product_shape(&representations);
        let shown = if pipe_name.is_empty() {
            format!("{} pipe {}", name_of(line), index + 1)
        } else {
            pipe_name.clone()
        };
        let pipe = self.b.product(
            &pipe_class,
            &pipe_key,
            &shown,
            &pipe_type,
            product_shape,
            &pipe_name,
        );
        self.b.tally(
            &format!("layer {}", line.layer),
            &pipe_class,
            system,
            "drainage pipe",
        );

        let mut common = PropertyList::new();
        common.identifier("Reference", &pipe_name);
        common.positive_length("NominalDiameter", diameter);
        common.positive_length("Length", Some((b - a).length()));
        let set = self.b.property_set(
            &format!("{pipe_key}/common"),
            "Pset_PipeSegmentTypeCommon",
            &common,
        );
        self.b.defines(&format!("{pipe_key}/common"), set, &[pipe]);
        if let Some((at_a, at_b)) = inverts {
            let mut occurrence = PropertyList::new();
            occurrence.length("InvertElevation", Some(max_of(at_a, at_b)));
            let run = (b - a).length();
            if run > 0.0 {
                occurrence.positive_ratio("Gradient", Some((at_a - at_b).abs() / run));
            }
            let set = self.b.property_set(
                &format!("{pipe_key}/occurrence"),
                "Pset_PipeSegmentOccurrence",
                &occurrence,
            );
            self.b
                .defines(&format!("{pipe_key}/occurrence"), set, &[pipe]);
        }
        let mut drainage = PropertyList::new();
        let mut own = PropertyMap::new();
        for (name, value) in &line.properties {
            if let Some(field) = name.strip_prefix(prefix.as_str()) {
                own.entry(String::from(field))
                    .or_insert_with(|| value.clone());
            }
        }
        add_entity_properties(&mut drainage, &own);
        drainage.label("LevelsFrom", &how);
        let set = self.b.property_set(
            &format!("{pipe_key}/drainage"),
            "Katana_DrainagePipe",
            &drainage,
        );
        self.b
            .defines(&format!("{pipe_key}/drainage"), set, &[pipe]);
        pipe
    }

    fn write_pit(
        &mut self,
        part: &Entity,
        vertices: &[Point2],
        invert_at: &[Option<f64>],
        system: &str,
    ) -> Id {
        let pit = text(&part.metadata, "12d.element") == "drainage pit";
        let prefix = if pit { "pit." } else { "house_connection." };
        let at = point_of(part);
        let top = number(&part.properties, ELEVATION_PROPERTY);
        let diameter = number(&part.properties, &format!("{prefix}diameter"));

        // The pit's invert: the lowest pipe invert at the vertex it stands on.
        let mut invert: Option<f64> = None;
        for (vertex, known) in vertices.iter().zip(invert_at) {
            if (*vertex - at).length() < 0.01 && known.is_some() {
                invert = *known;
            }
        }
        let mut representations = Vec::new();
        if pit
            && let (Some(top), Some(invert), Some(diameter)) = (top, invert, diameter)
            && diameter > 0.0
            && top > invert
        {
            let axis = self.b.polyline_3d(
                &[Vec3::new(at.x, at.y, invert), Vec3::new(at.x, at.y, top)],
                false,
            );
            let body = self.b.file().add(
                "IfcSweptDiskSolid",
                Args::new()
                    .reference(axis)
                    .real(diameter / 2.0)
                    .null()
                    .null()
                    .null(),
            );
            let context = self.b.body_context();
            representations.push(self.b.shape(context, "Body", "AdvancedSweptSolid", &[body]));
        }
        let mut items = vec![self.b.point(at)];
        if let Some(diameter) = diameter.filter(|diameter| *diameter > 0.0) {
            let centre = self.b.point(at);
            let placement = self
                .b
                .file()
                .add("IfcAxis2Placement2D", Args::new().reference(centre).null());
            items.push(self.b.file().add(
                "IfcCircle",
                Args::new().reference(placement).real(diameter / 2.0),
            ));
        }
        let set = self
            .b
            .file()
            .add("IfcGeometricCurveSet", Args::new().refs(&items));
        let context = self.b.foot_print_context();
        representations.push(
            self.b
                .shape(context, "FootPrint", "GeometricCurveSet", &[set]),
        );
        for &representation in &representations {
            self.b.layer(&part.layer, representation);
        }
        let pit_type = text(&part.properties, &format!("{prefix}type"));
        let name = text(&part.properties, &format!("{prefix}name"));
        let ifc_class = if pit {
            pit_class(&pit_type)
        } else {
            IfcClass::new("IfcPipeFitting", "JUNCTION", "")
        };
        let key = format!("entity/{}", part.id);
        let product_shape = self.b.product_shape(&representations);
        let shown = if name.is_empty() {
            name_of(part)
        } else {
            name.clone()
        };
        let product = self
            .b
            .product(&ifc_class, &key, &shown, &pit_type, product_shape, &name);
        self.b.tally(
            &format!("layer {}", part.layer),
            &ifc_class,
            system,
            if pit {
                "drainage pit"
            } else {
                "drainage house connection"
            },
        );
        if pit {
            let mut common = PropertyList::new();
            common.identifier("Reference", &name);
            let set = self.b.property_set(
                &format!("{key}/common"),
                "Pset_DistributionChamberElementCommon",
                &common,
            );
            self.b.defines(&format!("{key}/common"), set, &[product]);
            let mut size = PropertyList::new();
            size.positive_length("NominalLength", diameter)
                .positive_length("NominalWidth", diameter);
            if let (Some(top), Some(invert)) = (top, invert)
                && top > invert
            {
                size.positive_length("NominalHeight", Some(top - invert));
            }
            let set = self
                .b
                .property_set(&format!("{key}/size"), "Pset_ElementSize", &size);
            self.b.defines(&format!("{key}/size"), set, &[product]);
        }
        let mut drainage = PropertyList::new();
        let mut own = PropertyMap::new();
        for (field, value) in &part.properties {
            let name = field.strip_prefix(prefix).unwrap_or(field);
            own.entry(String::from(name))
                .or_insert_with(|| value.clone());
        }
        add_entity_properties(&mut drainage, &own);
        drainage.length("InvertLevel", invert);
        let set = self.b.property_set(
            &format!("{key}/drainage"),
            if pit {
                "Katana_DrainagePit"
            } else {
                "Katana_DrainageConnection"
            },
            &drainage,
        );
        self.b.defines(&format!("{key}/drainage"), set, &[product]);
        product
    }
}

pub(crate) fn export_entities(
    builder: &mut Builder<'_>,
    model: &Model,
    rules: &[ClassificationRule],
) {
    DrawingWriter::new(builder, model, rules).write();
}
