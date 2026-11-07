//! Archive -> Katana's domain model: where each element goes, and what is
//! said about what could not go anywhere (domain.rs).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::PI;

use katana_core::error::{Error, ErrorCode, Result};
use katana_core::text::{format_exact_real, parse_integer, unescape_typed};
use katana_entity::colour_names::{fold_colour_name, resolve_colour};
use katana_entity::entity::set_heights;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::layer_path::validate_layer_path;
use katana_entity::{
    Alignment, Color, Entity, Geometry, Layer, LeaderGeometry, PointGeometry, PropertyMap,
    PropertyValue, Style, TextGeometry, validate,
};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentPi, Arc2, Circle2, HorizontalAlignment, Point2, Point3, Polyline2, ProfilePvi,
    Segment2, SolvedAlignment, Triangle2, Vec2, VerticalAlignment,
};
use katana_terrain::{TinSurface, TinTriangle, combine_surfaces};

use crate::archive::{
    AlignmentSource, ArcString, Archive, AttributeList, AttributeValue, Breakline, CircleString,
    DraftingLeader, DraftingReference, DrainageRecord, DrainageString, Element, Field, FieldList,
    LasCloud, PlotFrame, Segment, SegmentKind, SolvedGeometry, StringHeader, StringKind,
    SuperAlignment, SuperTin, TextString, Tin, Trimesh, Vertex, VertexString, element_keyword,
    to_string,
};
use crate::domain::{
    DomainImport, ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, ElementTally, ImportOptions,
    ImportedCloud, ImportedCloudPoint, ImportedMesh, ImportedSurface, META_BREAKLINE,
    META_CHAINAGE, META_COLOUR, META_COLOUR_RGB, META_ELEMENT, META_EXTRA_PREFIX,
    META_LEADER_PREFIX, META_NAME, META_POINT_IDS, META_SOURCE, META_STRING_STYLE,
    META_SYMBOL_PREFIX, META_TEXT_PREFIX, META_VERTEX, standard_colour,
};
use crate::domain_common::layer_path_for_model;
use crate::plan_geometry::{ChordReport, PlanPoint, arc_tangent_at, chord_plan, solve_arc};
use crate::reader::parse_real;
use crate::text_utilities::{format_real, lower};

/// Never fails on CONTENT - an element that cannot be represented is a
/// warning and a tally of 0 imported, because one bad string must not cost the
/// user the other ten thousand. Fails with InvalidArgument for bad options.
pub fn to_domain(archive: &Archive, options: &ImportOptions) -> Result<DomainImport> {
    if !(options.curve_tolerance > 0.0) || !options.curve_tolerance.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
        ));
    }
    if !(options.alignment_tolerance > 0.0) || !options.alignment_tolerance.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the alignment tolerance must be positive",
        ));
    }
    if !options.layer_prefix.is_empty() {
        validate_layer_path(&options.layer_prefix)?;
    }
    if let Some(shift) = options.origin_shift
        && !(shift.x.is_finite() && shift.y.is_finite())
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the origin shift is not finite",
        ));
    }
    Ok(Importer::new(archive, options).run())
}

// How far short of half its chord a radius may fall and still be taken as the
// semicircle it was rounded from, when an arc is solved for its tangents: a
// millimetre, the precision real exports carry.
const ARC_SOLVE_TOLERANCE: f64 = 0.001;

// Skipped elements named one by one. Past this they are only counted: ten
// thousand identical lines tell the user nothing the count does not.
const MAX_NAMED_PROBLEMS: usize = 20;

const ASYMMETRIC_PARABOLA: &str =
    "an asymmetric vertical parabola is approximated by a symmetric one of the same length";
const CIRCULAR_VERTICAL_CURVE: &str =
    "a circular vertical curve is represented by the parabola with the same tangents";

fn radians(degrees: f64) -> f64 {
    degrees * PI / 180.0
}

// IEEE 754 remainder, as std::remainder computes it: x less the multiple of y
// nearest to it, ties to the even multiple, exactly - no Rust function has it.
// For a y that is finite, normal and far from the overflow edge (360 here) it
// is fdlibm's reduction: x modulo 2y, which is exact, then at most two exact
// subtractions of y (Sterbenz), and the sign of x put back.
fn ieee_remainder(x: f64, y: f64) -> f64 {
    let mut magnitude = (x % (y + y)).abs();
    let half = 0.5 * y;
    if magnitude > half {
        magnitude -= y;
        if magnitude >= half {
            magnitude -= y;
        }
    }
    if x.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

// std::max(worst, value).
fn raise_to(worst: &mut f64, value: f64) {
    if *worst < value {
        *worst = value;
    }
}

// A list item with a blank in it, as `"..."` with `"` and `\` escaped - the
// .12da rule (manual 1.1), so one splitter serves both.
fn quoted_list_item(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        if character == '"' || character == '\\' {
            out.push('\\');
        }
        out.push(character);
    }
    out.push('"');
    out
}

// A field of a pit or a pipe as a typed property: a number when it was
// written as one, text otherwise - "diameter 0.375" must not arrive as the
// text "0.375", or nothing can be computed from it.
fn property_of(field: &Field) -> PropertyValue {
    if !field.quoted {
        if let Some(integer) = parse_integer(&field.value) {
            return PropertyValue::Integer(integer);
        }
        if let Some(real) = parse_real(&field.value) {
            return PropertyValue::Real(real);
        }
    }
    PropertyValue::Text(field.value.clone())
}

fn add_property(properties: &mut PropertyMap, key: String, value: PropertyValue) {
    if key.is_empty() {
        return;
    }
    // Attribute names need not be unique in an archive. A map needs them to
    // be, and the second "Remarks" is not worth less than the first, so a
    // name already taken is given the first free "(2)", "(3)" ...
    let key = match properties.entry(key) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(value);
            return;
        }
        std::collections::btree_map::Entry::Occupied(slot) => slot.key().clone(),
    };
    let mut copy = 2;
    loop {
        let candidate = format!("{key} ({copy})");
        if let std::collections::btree_map::Entry::Vacant(slot) = properties.entry(candidate) {
            slot.insert(value);
            return;
        }
        copy += 1;
    }
}

fn flatten_attributes(attributes: &AttributeList, prefix: &str, properties: &mut PropertyMap) {
    for attribute in attributes {
        let mut key = String::from(prefix);
        if !prefix.is_empty() {
            key.push('/');
        }
        key.push_str(&attribute.name);
        match &attribute.value {
            AttributeValue::Group(group) => flatten_attributes(group, &key, properties),
            AttributeValue::Integer(integer) => {
                add_property(properties, key, PropertyValue::Integer(*integer));
            }
            AttributeValue::Real(real) => add_property(properties, key, PropertyValue::Real(*real)),
            AttributeValue::Text(text) => {
                add_property(properties, key, PropertyValue::Text(text.clone()));
            }
        }
    }
}

// A per-vertex or per-segment list as one text value: the property model has
// no list type (see HEIGHTS in domain.rs). Values with a blank in them are
// quoted so that the list can be split again; META_POINT_IDS keeps its own
// older comma form.
fn join_list(values: &[String]) -> String {
    let mut out = String::new();
    for value in values {
        if !out.is_empty() {
            out.push(' ');
        }
        let plain = !value.is_empty() && !value.contains([' ', '\t', '"']);
        if plain {
            out.push_str(value);
        } else {
            out.push_str(&quoted_list_item(value));
        }
    }
    out
}

fn join_flags(flags: &[bool]) -> String {
    let mut out = String::new();
    for &flag in flags {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push(if flag { '1' } else { '0' });
    }
    out
}

fn join_reals(values: &[f64]) -> String {
    let mut out = String::new();
    for &value in values {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format_exact_real(value));
    }
    out
}

fn meta(entity: &mut Entity, key: impl Into<String>, value: impl Into<PropertyValue>) {
    entity.metadata.insert(key.into(), value.into());
}

// A flag per vertex or segment, or one for them all, into the metadata: the
// entity model has no place for them, so they ride there and are written
// back. True when any of them is off.
fn flags(entity: &mut Entity, key: &str, value: Option<bool>, list: &[bool]) -> bool {
    if let Some(flag) = value {
        meta(entity, key, if flag { "1" } else { "0" });
    } else if !list.is_empty() {
        meta(entity, key, join_flags(list));
    }
    value == Some(false) || list.iter().any(|&flag| !flag)
}

fn describe(header: &StringHeader, kind: &str) -> String {
    format!("{kind} \"{}\" in model \"{}\"", header.name, header.model)
}

// The height of a chorded point: the vertex's own, or for a point chording
// added, interpolated between the vertices either side of it.
fn height_at(point: &PlanPoint, vertices: &[Vertex], constant_z: Option<f64>) -> Option<f64> {
    let height_of = |index: usize| vertices[index % vertices.len()].z.or(constant_z);
    let a = height_of(point.segment);
    if point.fraction == 0.0 {
        return a;
    }
    let b = height_of(point.segment + 1);
    let (Some(a), Some(b)) = (a, b) else {
        return None;
    };
    Some(a + (b - a) * point.fraction)
}

// Where the archive's anchor point puts the LEFT END OF THE BASELINE, which
// is what a Katana TextGeometry's position is.
//
// justify is "top|middle|bottom" and "-left|-centre|-right" (manual
// 1.5.8.4.9); every annotation in the sample archives is "bottom-left", which
// is already Katana's meaning, but the format allows the other eight and a
// text anchored by its centre sits half its width away. The width is
// ESTIMATED at 0.6 of the height per character (per byte, as the C++ counts)
// - there is no font here, and the renderer that will draw it has its own
// metrics; the estimate only has to be better than ignoring justification,
// which is out by the whole width.
fn baseline_left(anchor: Point2, text: &str, height: f64, rotation: f64, justify: &str) -> Point2 {
    const WIDTH_PER_HEIGHT: f64 = 0.6;
    let width = text.len() as f64 * height * WIDTH_PER_HEIGHT;
    let wanted = lower(justify);
    let mut along = 0.0;
    if wanted.contains("centre") || wanted.contains("center") {
        along = -0.5 * width;
    } else if wanted.contains("right") {
        along = -width;
    }
    // "top" and "middle" are measured from the cap line and the middle of it;
    // the baseline is below both.
    let mut across = 0.0;
    if wanted.starts_with("top") {
        across = -height;
    } else if wanted.starts_with("middle") {
        across = -0.5 * height;
    }
    let c = rotation.cos();
    let sn = rotation.sin();
    Point2::new(
        anchor.x + along * c - across * sn,
        anchor.y + along * sn + across * c,
    )
}

// Only `worldsize` is a height in model units. papersize (mm on the plot) and
// screensize (pixels) have no model-unit equivalent without a plot scale, so
// they fall back to the model's own default height.
fn text_height(annotation: Option<&FieldList>) -> f64 {
    let fallback = TextGeometry::default().height;
    let Some(annotation) = annotation else {
        return fallback;
    };
    match annotation.real("worldsize") {
        Some(world) if world > 0.0 => world,
        _ => fallback,
    }
}

// The direction of travel entering (`at_start`) or leaving a curved segment,
// from the segment's own parameters - so that two curves with no straight
// between them can still be given their tangents.
fn tangent_of(segment: &Segment, a: Point2, b: Point2, at_start: bool) -> Option<f64> {
    if segment.kind == SegmentKind::Spiral {
        let leading = segment.parameters.boolean("leading").unwrap_or(true);
        // (a1) belongs to the segment's first vertex when leading, to its
        // second - and pointing backwards - when trailing. See
        // plan_geometry.rs.
        let first = segment.parameters.real(if leading { "a1" } else { "a2" });
        let second = segment.parameters.real(if leading { "a2" } else { "a1" });
        let angle = if at_start { first } else { second }?;
        return Some(radians(angle) + if leading { 0.0 } else { PI });
    }
    if segment.kind == SegmentKind::Arc {
        let arc = solve_arc(a, b, segment, ARC_SOLVE_TOLERANCE)?;
        return Some(arc_tangent_at(&arc, if at_start { a } else { b }));
    }
    None
}

// PVIs from the solved vertical geometry. Returns the reason on failure.
fn pvis_from_elements(
    data: &SolvedGeometry,
    vertical: &mut VerticalAlignment,
    notes: &mut Vec<String>,
) -> String {
    let count = data.vertices.len();
    if count < 2 {
        return String::from("its vertical geometry has fewer than two vertices");
    }
    let kind_of = |index: usize| {
        data.segments
            .get(index)
            .map_or(SegmentKind::Straight, |segment| segment.kind)
    };
    vertical.pvis.push(ProfilePvi {
        station: data.vertices[0].x,
        elevation: data.vertices[0].y,
        curve_length: 0.0,
    });
    for (i, pair) in data.vertices.windows(2).enumerate() {
        let a = &pair[0];
        let b = &pair[1];
        let length = b.x - a.x;
        if !(length > 0.0) {
            return String::from("its vertical geometry goes backwards in chainage");
        }
        if kind_of(i) == SegmentKind::Straight {
            if i + 2 < count && kind_of(i + 1) == SegmentKind::Straight {
                vertical.pvis.push(ProfilePvi {
                    station: b.x,
                    elevation: b.y,
                    curve_length: 0.0,
                });
            }
            continue;
        }
        let mut pvi = ProfilePvi {
            curve_length: length,
            ..ProfilePvi::default()
        };
        if kind_of(i) == SegmentKind::Parabola {
            let parameters = &data.segments[i].parameters;
            let (Some(chainage), Some(height)) =
                (parameters.real("chainage"), parameters.real("height"))
            else {
                return String::from("a vertical parabola does not record its VIP");
            };
            pvi.station = chainage;
            pvi.elevation = height;
            if (chainage - 0.5 * (a.x + b.x)).abs() > 1e-3 * length {
                notes.push(String::from(ASYMMETRIC_PARABOLA));
                pvi.station = 0.5 * (a.x + b.x);
                // Keep the entering grade: the VIP slides along it.
                let grade = (height - a.y) / (chainage - a.x);
                pvi.elevation = a.y + grade * (pvi.station - a.x);
            }
        } else if kind_of(i) == SegmentKind::Arc {
            // A circular vertical curve. Its end tangents are taken from the
            // circle and a parabola fitted to them: for the grades of a road
            // or railway the two differ by less than a millimetre.
            let start = Point2::new(a.x, a.y);
            let Some(arc) = solve_arc(
                start,
                Point2::new(b.x, b.y),
                &data.segments[i],
                ARC_SOLVE_TOLERANCE,
            ) else {
                return String::from("a vertical arc cannot span its end points");
            };
            // The entering grade is the arc's tangent there; the VIP of the
            // symmetric parabola sits on it, mid-way.
            let grade = arc_tangent_at(&arc, start).tan();
            pvi.station = 0.5 * (a.x + b.x);
            pvi.elevation = a.y + grade * (pvi.station - a.x);
            notes.push(String::from(CIRCULAR_VERTICAL_CURVE));
        } else {
            return String::from(
                "its vertical geometry holds a segment that is not a line, parabola or arc",
            );
        }
        vertical.pvis.push(pvi);
    }
    let last = &data.vertices[count - 1];
    vertical.pvis.push(ProfilePvi {
        station: last.x,
        elevation: last.y,
        curve_length: 0.0,
    });
    String::new()
}

// PVIs from IP-method vertical parts (and so from an old alignment string's
// vipdata).
fn pvis_from_parts(
    alignment: &SuperAlignment,
    vertical: &mut VerticalAlignment,
    notes: &mut Vec<String>,
) -> String {
    let parts = &alignment.vertical_parts;
    for (i, part) in parts.iter().enumerate() {
        let (Some(x), Some(y)) = (part.fields.real("x"), part.fields.real("y")) else {
            return String::from("a VIP has no chainage or height");
        };
        let mut pvi = ProfilePvi {
            station: x,
            elevation: y,
            curve_length: 0.0,
        };
        let interior = i > 0 && i + 1 < parts.len();
        if interior && part.kind != "ip" {
            let previous = &parts[i - 1].fields;
            let following = &parts[i + 1].fields;
            let (Some(px), Some(py), Some(nx), Some(ny)) = (
                previous.real("x"),
                previous.real("y"),
                following.real("x"),
                following.real("y"),
            ) else {
                return String::from("its VIPs are not in increasing chainage");
            };
            if !(x > px) || !(nx > x) {
                return String::from("its VIPs are not in increasing chainage");
            }
            // Grades in percent, as the K value is defined: K is the length of
            // curve per percent of grade change.
            let change = ((ny - y) / (nx - x) - (y - py) / (x - px)).abs() * 100.0;
            match part.kind.as_str() {
                "length" => {
                    let l1 = part.fields.real("l1");
                    let l2 = part.fields.real("l2");
                    pvi.curve_length = part.fields.real("l").unwrap_or(0.0);
                    if let (Some(l1), Some(l2)) = (l1, l2) {
                        pvi.curve_length = l1 + l2;
                        notes.push(String::from(ASYMMETRIC_PARABOLA));
                    }
                }
                "asymmetric" => {
                    pvi.curve_length = part.fields.real("l1").unwrap_or(0.0)
                        + part.fields.real("l2").unwrap_or(0.0);
                    notes.push(String::from(ASYMMETRIC_PARABOLA));
                }
                "kvalue" => {
                    pvi.curve_length = part.fields.real("k").unwrap_or(0.0) * change;
                }
                "radius" | "arc" => {
                    // L = R * |change of grade|, the grade as a ratio.
                    pvi.curve_length = part.fields.real("r").unwrap_or(0.0) * change / 100.0;
                    if part.kind == "arc" {
                        notes.push(String::from(CIRCULAR_VERTICAL_CURVE));
                    }
                }
                _ => {}
            }
            pvi.curve_length = pvi.curve_length.abs();
        }
        vertical.pvis.push(pvi);
    }
    String::new()
}

fn keep_drafting_reference(entity: &mut Entity, reference: &DraftingReference, prefix: &str) {
    for field in reference.fields.fields() {
        meta(
            entity,
            format!("{prefix}.{}", field.key),
            property_of(field),
        );
    }
    for (i, block) in reference.blocks.iter().enumerate() {
        keep_drafting_reference(entity, block, &format!("{prefix}.{}[{i}]", block.name));
    }
}

struct Reconstruction {
    horizontal: HorizontalAlignment,
    // Non-empty: why the PIs could not be had.
    failure: String,
}

impl Reconstruction {
    fn new() -> Self {
        Self {
            horizontal: HorizontalAlignment::default(),
            failure: String::new(),
        }
    }

    fn failed(failure: &str) -> Self {
        Self {
            horizontal: HorizontalAlignment::default(),
            failure: String::from(failure),
        }
    }
}

struct Importer<'a> {
    archive: &'a Archive,
    options: &'a ImportOptions,
    result: DomainImport,
    layer_of_model: BTreeMap<String, String>,
    // Linestyle name -> its place in result.styles_needed; see style_for. Its
    // order is never seen: it is only asked.
    style_index: HashMap<String, usize>,
    alignment_names: BTreeSet<String>,
    noted: BTreeSet<String>,
    chord_report: ChordReport,
    named_problems: usize,
    trimeshes_empty: usize,
    trimesh_faces_dropped: usize,
    partly_invisible: usize,
    symbols_left: usize,
    leader_references: usize,
    annotations_left: usize,
    coloured_tins: usize,
    point_strings_split: usize,
    points_from_strings: usize,
}

impl<'a> Importer<'a> {
    fn new(archive: &'a Archive, options: &'a ImportOptions) -> Self {
        Self {
            archive,
            options,
            result: DomainImport::default(),
            layer_of_model: BTreeMap::new(),
            style_index: HashMap::new(),
            alignment_names: BTreeSet::new(),
            noted: BTreeSet::new(),
            chord_report: ChordReport::default(),
            named_problems: 0,
            trimeshes_empty: 0,
            trimesh_faces_dropped: 0,
            partly_invisible: 0,
            symbols_left: 0,
            leader_references: 0,
            annotations_left: 0,
            coloured_tins: 0,
            point_strings_split: 0,
            points_from_strings: 0,
        }
    }

    fn run(mut self) -> DomainImport {
        // Most elements become one entity, and none becomes more than a
        // handful, so the element count is the right order for the first
        // allocation.
        let archive = self.archive;
        self.result.entities.reserve(archive.elements.len());
        for element in &archive.elements {
            let keyword = element_keyword(element);
            let imported = match element {
                Element::VertexString(string) => self.import_vertex_string(string),
                Element::ArcString(arc) => self.import_arc(arc),
                Element::CircleString(circle) => self.import_circle(circle),
                Element::TextString(text) => self.import_text(text),
                Element::PlotFrame(frame) => self.import_plot_frame(frame),
                Element::DrainageString(drainage) => self.import_drainage(drainage),
                Element::SuperAlignment(alignment) => self.import_alignment(alignment),
                Element::LasCloud(cloud) => self.import_cloud(cloud),
                Element::Tin(tin) => self.import_tin(tin),
                Element::SuperTin(super_tin) => self.import_super_tin(super_tin),
                Element::Trimesh(trimesh) => self.import_trimesh(trimesh),
                Element::DraftingLeader(leader) => self.import_leader(leader),
            };
            let tally = self.tally_for(&keyword);
            self.result.tally[tally].read += 1;
            if imported != 0 {
                self.result.tally[tally].imported += 1;
            }
        }
        self.build_super_tins();
        self.finish_warnings();
        self.result
    }

    // ---- bookkeeping --------------------------------------------------------------

    fn tally_for(&mut self, keyword: &str) -> usize {
        if let Some(index) = self
            .result
            .tally
            .iter()
            .position(|tally| tally.keyword == keyword)
        {
            return index;
        }
        self.result.tally.push(ElementTally {
            keyword: String::from(keyword),
            read: 0,
            imported: 0,
        });
        self.result.tally.len() - 1
    }

    fn problem(&mut self, message: String) {
        if self.named_problems < MAX_NAMED_PROBLEMS {
            self.result.warnings.push(message);
        }
        self.named_problems += 1;
    }

    fn plan(&self, x: f64, y: f64) -> Point2 {
        let shift = self.options.origin_shift.unwrap_or_default();
        Point2::new(x - shift.x, y - shift.y)
    }

    fn plan_vertex(&self, vertex: &Vertex) -> Point2 {
        self.plan(vertex.x, vertex.y)
    }

    fn colour_for(&mut self, name: &str) -> Option<Color> {
        let rgb = resolve_colour(&self.options.colours, name);
        let folded = fold_colour_name(name);
        if rgb.is_none()
            && !folded.is_empty()
            && folded != "no colour"
            && self.noted.insert(format!("colour:{folded}"))
        {
            self.problem(format!(
                "colour \"{name}\" is not in the standard or active customisation palette; its name is kept and the inherited colour is used"
            ));
        }
        rgb
    }

    fn layer_for(&mut self, model: &str, colour: &str) -> String {
        if let Some(found) = self.layer_of_model.get(model) {
            return found.clone();
        }
        let mut layer = Layer {
            name: layer_path_for_model(model, &self.options.layer_prefix),
            ..Layer::default()
        };
        if let Some(rgb) = self.colour_for(colour) {
            layer.color = rgb;
        }
        let known = self
            .result
            .layers_needed
            .iter()
            .any(|other| other.name == layer.name);
        let name = layer.name.clone();
        if !known {
            self.result.layers_needed.push(layer);
        }
        self.layer_of_model
            .insert(String::from(model), name.clone());
        name
    }

    // The Katana style for a library linestyle name, created once per name;
    // its place in styles_needed, whose name is `linestyle`. A name Katana
    // cannot hold (empty) means ByLayer, and None. A style named by a symbol
    // block gets the symbol at the size the first such block gave; met as a
    // line first, it stays a line style until a symbol block names it.
    fn style_for(&mut self, linestyle: &str, symbol_block: Option<&FieldList>) -> Option<usize> {
        if linestyle.is_empty() {
            return None;
        }
        // Found through an index, NOT by searching styles_needed: this is
        // asked once for every entity, and the search it replaces was
        // O(entities x styles). The vector's first-use order is the contract
        // (domain.rs); the index only says where in it each name landed.
        let index = match self.style_index.get(linestyle) {
            Some(&index) => index,
            None => {
                // The LINESTYLE NAME, kept as the name to draw the line with.
                // A loaded linestyle library is looked up by this, and a
                // project with no library falls back to a plain line.
                self.result.styles_needed.push(Style {
                    name: String::from(linestyle),
                    linetype: String::from(linestyle),
                    description: String::from("library linestyle"),
                    ..Style::default()
                });
                let index = self.result.styles_needed.len() - 1;
                self.style_index.insert(String::from(linestyle), index);
                index
            }
        };
        let known = &mut self.result.styles_needed[index];
        if let Some(block) = symbol_block
            && known.symbol.is_empty()
        {
            // The REAL symbol name, not one of the sixteen shapes Katana draws
            // without a library: a loaded symbol library is matched against
            // it. The guess happens when it is drawn, only if nothing defines
            // the name (katana_entity::tables::built_in_symbol_for).
            known.symbol = String::from(linestyle);
            let size = block.real("size").unwrap_or(0.0);
            known.symbol_size = if 0.0 < size { size } else { 0.0 };
            known.description = String::from("library symbol");
        }
        Some(index)
    }

    fn make_entity(&mut self, header: &StringHeader, keyword: &str) -> Entity {
        let mut entity = Entity {
            layer: self.layer_for(&header.model, &header.colour),
            ..Entity::default()
        };
        self.style_for(&header.style, None);
        entity.style = header.style.clone();
        // A string has a colour of its own; there is no ByLayer in an
        // archive. A name Katana has no RGB for is left to the layer, and
        // kept below.
        entity.color = self.colour_for(&header.colour);
        if self.options.attributes_as_properties {
            flatten_attributes(&header.attributes, "", &mut entity.properties);
        }
        meta(&mut entity, META_ELEMENT, keyword);
        if !header.name.is_empty() {
            meta(&mut entity, META_NAME, header.name.clone());
        }
        if !header.colour.is_empty() {
            meta(&mut entity, META_COLOUR, header.colour.clone());
            if let Some(color) = entity.color
                && standard_colour(&header.colour).is_none()
            {
                meta(&mut entity, META_COLOUR_RGB, color.to_hex());
            }
        }
        if header.chainage != 0.0 {
            meta(&mut entity, META_CHAINAGE, header.chainage);
        }
        meta(
            &mut entity,
            META_BREAKLINE,
            if header.breakline == Breakline::Point {
                "point"
            } else {
                "line"
            },
        );
        if !self.options.source_name.is_empty() {
            meta(&mut entity, META_SOURCE, self.options.source_name.clone());
        }
        // Every scalar the string carried that Katana has no field for -
        // weight, time_created, a face's hatching, a plot frame's margins -
        // travels as `12d.x.<key>`, and export writes it back. Nothing scalar
        // is lost between a file and the file exported from it.
        for field in header.extras.fields() {
            if field.key != "null_value" {
                meta(
                    &mut entity,
                    format!("{META_EXTRA_PREFIX}{}", field.key),
                    field.value.clone(),
                );
            }
        }
        entity
    }

    // Adds `entity` if its geometry is one the model will accept. It must be
    // checked HERE: the caller creates every entity in one atomic command, so
    // a single zero-length string would otherwise fail the whole import.
    fn add(&mut self, entity: Entity, what: &str) -> bool {
        if let Err(error) = validate(&entity.geometry) {
            self.problem(format!("{what} skipped: {}", error.message));
            return false;
        }
        self.result
            .bounds
            .expand_box(bounding_box(&entity.geometry));
        self.result.entities.push(entity);
        true
    }

    // ---- strings ----------------------------------------------------------------------

    // Vertices and segments -> a Point, a Line or a Polyline on `entity`.
    fn set_string_geometry(
        &mut self,
        entity: &mut Entity,
        vertices: &[Vertex],
        segments: &[Segment],
        closed: bool,
        constant_z: Option<f64>,
    ) -> bool {
        if vertices.is_empty() {
            return false;
        }
        let mut report = ChordReport::default();
        let points = chord_plan(
            vertices,
            segments,
            closed,
            self.options.curve_tolerance,
            &mut report,
        );
        self.chord_report.merge(&report);

        // Exactly one of each per chorded point, so the size is known.
        let mut heights = Vec::with_capacity(points.len());
        let mut polyline =
            Polyline2::new(Vec::with_capacity(points.len()), closed && points.len() > 2);
        for point in &points {
            polyline
                .vertices
                .push(self.plan(point.point.x, point.point.y));
            heights.push(height_at(point, vertices, constant_z));
        }

        if polyline.vertices.len() == 1 || !(polyline.length() > 0.0) {
            // One vertex, or several in the same place: a point.
            entity.geometry = Geometry::Point(PointGeometry {
                position: polyline.vertices[0],
            });
            heights.truncate(1);
        } else if polyline.vertices.len() == 2 && !polyline.closed {
            // A Line rather than a two-vertex polyline, as importVector makes
            // it, so that it can still be filleted and extended.
            entity.geometry =
                Geometry::Line(Segment2::new(polyline.vertices[0], polyline.vertices[1]));
        } else {
            entity.geometry = Geometry::Polyline(polyline);
        }
        set_heights(&mut entity.properties, &heights);
        true
    }

    // Everything an archive annotation says about a piece of text, applied to
    // the entity Katana will draw. See META_TEXT_PREFIX for what is kept
    // rather than applied, and why.
    fn apply_annotation(
        &mut self,
        entity: &mut Entity,
        geometry: &mut TextGeometry,
        annotation: Option<&FieldList>,
    ) {
        let Some(annotation) = annotation else {
            return;
        };
        geometry.rotation = radians(annotation.real("angle").unwrap_or(0.0));
        // The text's own colour, which is not the string's: "no_colour" means
        // take the string's, which the entity already has.
        let colour = annotation.text("colour", &annotation.text("text_colour", ""));
        if !colour.is_empty() && colour != "no_colour" {
            if let Some(rgb) = self.colour_for(&colour) {
                entity.color = Some(rgb);
            }
            meta(entity, format!("{META_TEXT_PREFIX}colour"), colour);
        }
        let style = annotation.text("textstyle", "");
        if !style.is_empty() {
            self.style_for(&style, None);
            entity.style = style;
        }
        // An offset is in the same units as the SIZE, and only worldsize is
        // model units; a papersize offset is millimetres on a plot, which has
        // no model-unit meaning without a plot scale - the same reason
        // text_height falls back. The direction the manual does not give: it
        // is taken as perpendicular to the text, to the left, which is where
        // the format's program puts an offset annotation above a line.
        let world_units = annotation.real("worldsize").unwrap_or(0.0) > 0.0;
        for key in [
            "offset",
            "raise",
            "slant",
            "x_factor",
            "xfactor",
            "justify",
            "papersize",
            "screensize",
        ] {
            if let Some(field) = annotation.find(key) {
                meta(
                    entity,
                    format!("{META_TEXT_PREFIX}{key}"),
                    field.value.clone(),
                );
            }
        }
        let offset = annotation.real("offset").unwrap_or(0.0);
        if world_units && offset != 0.0 {
            let c = geometry.rotation.cos();
            let sn = geometry.rotation.sin();
            geometry.position = Point2::new(
                geometry.position.x - offset * sn,
                geometry.position.y + offset * c,
            );
        }
        // `raise` raises the LEVEL: it is a height, not a plan displacement,
        // so it goes onto the elevation the text already carries.
        let raised = annotation.real("raise").unwrap_or(0.0);
        if raised != 0.0
            && let Some(PropertyValue::Real(z)) = entity.properties.get_mut(ELEVATION_PROPERTY)
        {
            *z += raised;
        }
        geometry.position = baseline_left(
            geometry.position,
            &geometry.text,
            geometry.height,
            geometry.rotation,
            &annotation.text("justify", ""),
        );
    }

    fn add_vertex_text(&mut self, string: &VertexString) -> usize {
        let mut added = 0;
        for (i, vertex) in string.vertices.iter().enumerate() {
            let text = string
                .vertex_text
                .get(i)
                .or(string.vertex_text_value.as_ref());
            let Some(text) = text.filter(|text| !text.is_empty()) else {
                continue;
            };
            let annotation = string
                .vertex_annotations
                .get(i)
                .or(string.vertex_annotation.as_ref());
            let mut entity = self.make_entity(&string.header, "vertex text");
            entity.properties.clear(); // the string's attributes belong to the string
            let mut geometry = TextGeometry {
                position: self.plan_vertex(vertex),
                text: text.clone(),
                height: text_height(annotation),
                ..TextGeometry::default()
            };
            if let Some(z) = vertex.z.or(string.constant_z) {
                entity
                    .properties
                    .insert(String::from(ELEVATION_PROPERTY), PropertyValue::Real(z));
            }
            self.apply_annotation(&mut entity, &mut geometry, annotation);
            entity.geometry = Geometry::Text(geometry);
            let what = format!("vertex text of {}", describe(&string.header, "string"));
            if self.add(entity, &what) {
                added += 1;
            }
        }
        added
    }

    fn import_vertex_string(&mut self, string: &VertexString) -> usize {
        let keyword = format!("string {}", to_string(string.kind));
        let mut entity = self.make_entity(&string.header, &keyword);
        // A face string is a filled region (manual 1.5.4: fill_mode, hatch);
        // it has no `closed` field because it always is.
        let closed = string.closed || string.kind == StringKind::Face;
        if !self.set_string_geometry(
            &mut entity,
            &string.vertices,
            &string.segments,
            closed,
            string.constant_z,
        ) {
            self.problem(format!(
                "{} skipped: it has no vertices",
                describe(&string.header, &keyword)
            ));
            return 0;
        }
        if !string.point_ids.is_empty() {
            let mut ids = String::new();
            for id in &string.point_ids {
                if !ids.is_empty() {
                    ids.push(',');
                }
                ids.push_str(id);
            }
            meta(&mut entity, META_POINT_IDS, ids);
        }
        // Pipe and culvert sizes: one for the string, or one per segment. A
        // per-segment list whose entries are all the same IS one for the
        // string, which is how version 15 of the format's program writes a
        // uniform pipe.
        let uniform =
            |values: &[f64]| !values.is_empty() && values.iter().all(|&value| value == values[0]);
        if let Some(diameter) = string.diameter {
            meta(&mut entity, "12d.diameter", diameter);
        } else if uniform(string.diameters.as_slice()) {
            meta(&mut entity, "12d.diameter", string.diameters[0]);
        } else if !string.diameters.is_empty() {
            meta(&mut entity, "12d.diameters", join_reals(&string.diameters));
        }
        if let Some(culvert) = string.culvert {
            meta(&mut entity, "12d.culvert_width", culvert[0]);
            meta(&mut entity, "12d.culvert_height", culvert[1]);
        } else if !string.culverts.is_empty() {
            let mut sizes = Vec::with_capacity(2 * string.culverts.len());
            for culvert in &string.culverts {
                sizes.push(culvert[0]);
                sizes.push(culvert[1]);
            }
            meta(&mut entity, "12d.culverts", join_reals(&sizes));
        }
        if !string.justify.is_empty() {
            meta(&mut entity, "12d.justify", string.justify.clone());
        }
        // A string with an invisible or untinable part is also COUNTED, since
        // the drawing shows the whole string.
        if flags(
            &mut entity,
            "12d.vertex_visible",
            string.vertex_visible_value,
            &string.vertex_visible,
        ) || flags(
            &mut entity,
            "12d.segment_visible",
            string.segment_visible_value,
            &string.segment_visible,
        ) {
            self.partly_invisible += 1;
        }
        flags(
            &mut entity,
            "12d.vertex_tinable",
            string.vertex_tinable_value,
            &string.vertex_tinable,
        );
        flags(
            &mut entity,
            "12d.segment_tinable",
            string.segment_tinable_value,
            &string.segment_tinable,
        );
        if !string.segment_colours.is_empty() {
            meta(
                &mut entity,
                "12d.segment_colours",
                join_list(&string.segment_colours),
            );
        }
        if !string.interface_modes.is_empty() {
            let modes: Vec<String> = string
                .interface_modes
                .iter()
                .map(|mode| format!("{mode}"))
                .collect();
            meta(&mut entity, "12d.interface_modes", join_list(&modes));
        }
        // Attributes on vertices and segments become properties keyed by
        // position - `vertex/3/QualityLevel` - the way a group flattens; a
        // one-vertex string's vertex attributes are simply its own.
        if self.options.attributes_as_properties {
            if string.vertex_attributes.len() == 1 && string.vertices.len() == 1 {
                flatten_attributes(&string.vertex_attributes[0], "", &mut entity.properties);
            } else {
                for (i, attributes) in string.vertex_attributes.iter().enumerate() {
                    flatten_attributes(
                        attributes,
                        &format!("vertex/{}", i + 1),
                        &mut entity.properties,
                    );
                }
            }
            for (i, attributes) in string.segment_attributes.iter().enumerate() {
                flatten_attributes(
                    attributes,
                    &format!("segment/{}", i + 1),
                    &mut entity.properties,
                );
            }
        }
        self.import_symbols(string, &mut entity);
        // Per-vertex annotation properties have no Katana form at all; they
        // are counted, so that the import can say what it left.
        if !string.vertex_annotations.is_empty()
            || !string.segment_annotations.is_empty()
            || (string.segment_annotation.is_some()
                && string.segment_text.is_empty()
                && string.segment_text_value.is_none())
        {
            self.annotations_left += 1;
        }
        let what = describe(&string.header, &keyword);
        let mut added = self.add_or_split(entity, &string.header, &what);
        added += self.add_vertex_text(string);
        added += self.add_segment_text(string);
        added
    }

    // `breakline point` means the string's vertices are POINTS - separate
    // survey shots kept in one string for convenience - and not a line
    // through them. Katana has no multi-point geometry, so each vertex becomes
    // its own point entity carrying the string's layer, style, colour,
    // metadata and properties.
    //
    // Without this they were drawn as a polyline: the vertices were joined
    // up, and a model of standing infrastructure came out as a scribble
    // through it. Measured on a 15 MB production archive: 13,326 point strings
    // have a single vertex and were always right; nine have several, and
    // those nine held 649 points.
    fn add_or_split(&mut self, entity: Entity, header: &StringHeader, what: &str) -> usize {
        let places = match (&entity.geometry, header.breakline) {
            (Geometry::Line(segment), Breakline::Point) => Some(vec![segment.start, segment.end]),
            (Geometry::Polyline(polyline), Breakline::Point) => Some(polyline.vertices.clone()),
            _ => None,
        };
        let Some(places) = places else {
            return usize::from(self.add(entity, what));
        };
        // The heights the string carried, one per vertex, so each point keeps
        // its own rather than the list of all of them.
        let mut heights: Vec<String> = Vec::new();
        if let Some(PropertyValue::Text(list)) = entity.properties.get(ELEVATIONS_PROPERTY) {
            heights = list.split(' ').map(String::from).collect();
        }

        let mut added = 0;
        for (i, &place) in places.iter().enumerate() {
            let mut point = entity.clone();
            point.geometry = Geometry::Point(PointGeometry { position: place });
            point.properties.remove(ELEVATIONS_PROPERTY);
            if let Some(height) = heights.get(i)
                && height != "null"
                && !height.is_empty()
                && let Some(value) = parse_real(height)
            {
                point
                    .properties
                    .insert(String::from(ELEVATION_PROPERTY), PropertyValue::Real(value));
            }
            // Which vertex of the string this was, so the points can be told
            // apart and put back in order.
            meta(&mut point, META_VERTEX, (i + 1) as i64);
            if self.add(point, what) {
                added += 1;
            }
        }
        if added != 0 {
            self.point_strings_split += 1;
            self.points_from_strings += added;
        }
        added
    }

    // See META_SYMBOL_PREFIX for the two forms a symbol takes. A string with
    // ONE block takes that symbol as its style however many vertices it has:
    // a vertex symbol goes on EVERY vertex, which is what `mode vertex` means
    // in a library and where cad::styleDrawing puts it.
    //
    // A per-vertex list of MORE than one block - which the format allows and
    // its program does not write - is still kept whole and counted, because
    // there is no single symbol for the string to be drawn with.
    fn import_symbols(&mut self, string: &VertexString, entity: &mut Entity) {
        let mut blocks: Vec<&FieldList> = Vec::new();
        if let Some(symbol) = &string.symbol {
            blocks.push(symbol);
        }
        blocks.extend(string.symbols.iter());
        let Some(&first) = blocks.first() else {
            return;
        };
        let symbol_style = first.text("style", "");
        if blocks.len() == 1 && !symbol_style.is_empty() {
            let style = self.style_for(&symbol_style, Some(first));
            entity.style = symbol_style.clone();
            if string.header.style != symbol_style {
                meta(entity, META_STRING_STYLE, string.header.style.clone());
            }
            // What the string LOOKS like is its symbol, so the symbol's colour
            // is the entity's. The string's own colour name stays in
            // 12d.colour: the two are the same name in every sample archive,
            // but the format allows two and neither may be lost.
            let symbol_colour = first.text("colour", "");
            if let Some(rgb) = self.colour_for(&symbol_colour) {
                entity.color = Some(rgb);
            }
            let style_size =
                style.map_or(0.0, |index| self.result.styles_needed[index].symbol_size);
            for field in first.fields() {
                // The style carries the shape and the size, the entity the
                // colour where it agrees with the string's; the rest of the
                // block is kept only where it says something - the format's
                // program writes rotation, offset and raise as 0 on tens of
                // thousands of points, and metadata is not free.
                let on_style = field.key == "style"
                    || (field.key == "size" && first.real("size").unwrap_or(-1.0) == style_size);
                let on_entity = field.key == "colour" && symbol_colour == string.header.colour;
                let is_default =
                    (field.key == "rotation" || field.key == "offset" || field.key == "raise")
                        && first.real(&field.key) == Some(0.0);
                if !on_style && !on_entity && !is_default {
                    meta(
                        entity,
                        format!("{META_SYMBOL_PREFIX}{}", field.key),
                        field.value.clone(),
                    );
                }
            }
            return;
        }
        // One list per key, a value per block, in the order the keys first
        // appear; a key a block lacks holds an empty item so the columns stay
        // aligned.
        let mut keys: Vec<&str> = Vec::new();
        for block in &blocks {
            for field in block.fields() {
                if !keys.contains(&field.key.as_str()) {
                    keys.push(&field.key);
                }
            }
        }
        for key in &keys {
            let values: Vec<String> = blocks.iter().map(|block| block.text(key, "")).collect();
            meta(
                entity,
                format!("{META_SYMBOL_PREFIX}{key}"),
                join_list(&values),
            );
        }
        if blocks
            .iter()
            .any(|block| !block.text("style", "").is_empty())
        {
            self.symbols_left += 1;
        }
    }

    // Text on a segment is drawn at its middle (manual 1.5.8.4.9 gives it an
    // offset and a raise, which need a paper scale Katana does not have here).
    fn add_segment_text(&mut self, string: &VertexString) -> usize {
        let segments = string.segment_count();
        let mut added = 0;
        for (i, a) in string.vertices.iter().enumerate().take(segments) {
            let text = string
                .segment_text
                .get(i)
                .or(string.segment_text_value.as_ref());
            let Some(text) = text.filter(|text| !text.is_empty()) else {
                continue;
            };
            let annotation = string
                .segment_annotations
                .get(i)
                .or(string.segment_annotation.as_ref());
            let b = &string.vertices[(i + 1) % string.vertices.len()];
            let mut entity = self.make_entity(&string.header, "segment text");
            entity.properties.clear();
            let mut geometry = TextGeometry {
                position: self.plan(0.5 * (a.x + b.x), 0.5 * (a.y + b.y)),
                text: text.clone(),
                height: text_height(annotation),
                ..TextGeometry::default()
            };
            self.apply_annotation(&mut entity, &mut geometry, annotation);
            entity.geometry = Geometry::Text(geometry);
            let what = format!("segment text of {}", describe(&string.header, "string"));
            if self.add(entity, &what) {
                added += 1;
            }
        }
        added
    }

    fn import_arc(&mut self, arc: &ArcString) -> usize {
        let mut entity = self.make_entity(&arc.header, "string arc");
        let centre = self.plan_vertex(&arc.centre);
        let start = self.plan_vertex(&arc.start);
        let end = self.plan_vertex(&arc.end);
        let radius = if arc.radius != 0.0 {
            arc.radius.abs()
        } else {
            centre.distance_to(start)
        };
        let start_angle = (start.y - centre.y).atan2(start.x - centre.x);
        let end_angle = (end.y - centre.y).atan2(end.x - centre.x);
        // The manual gives `string arc` a radius and says nothing of its sign.
        // It is read as every other radius in the format is
        // (plan_geometry.rs): positive runs clockwise from start to end. A
        // negative radius, the only way the format has to say otherwise, runs
        // counter-clockwise.
        let clockwise = arc.radius >= 0.0;
        let sweep = if clockwise {
            -((start_angle - end_angle + 4.0 * PI) % (2.0 * PI))
        } else {
            (end_angle - start_angle + 4.0 * PI) % (2.0 * PI)
        };
        entity.geometry = if sweep == 0.0 {
            Geometry::Circle(Circle2::new(centre, radius))
        } else {
            Geometry::Arc(Arc2::new(centre, radius, start_angle, sweep))
        };
        set_heights(&mut entity.properties, &[arc.start.z, arc.end.z]);
        usize::from(self.add(entity, &describe(&arc.header, "string arc")))
    }

    fn import_circle(&mut self, circle: &CircleString) -> usize {
        let keyword = if circle.feature {
            "string feature"
        } else {
            "string circle"
        };
        let mut entity = self.make_entity(&circle.header, keyword);
        entity.geometry = Geometry::Circle(Circle2::new(
            self.plan_vertex(&circle.centre),
            circle.radius.abs(),
        ));
        set_heights(&mut entity.properties, &[circle.centre.z]);
        usize::from(self.add(entity, &describe(&circle.header, keyword)))
    }

    fn import_text(&mut self, text: &TextString) -> usize {
        let mut entity = self.make_entity(&text.header, "string text");
        let mut geometry = TextGeometry {
            position: self.plan_vertex(&text.position),
            text: text.text.clone(),
            height: text_height(Some(&text.annotation)),
            ..TextGeometry::default()
        };
        set_heights(&mut entity.properties, &[text.position.z]);
        self.apply_annotation(&mut entity, &mut geometry, Some(&text.annotation));
        entity.geometry = Geometry::Text(geometry);
        usize::from(self.add(entity, &describe(&text.header, "string text")))
    }

    fn import_leader(&mut self, leader: &DraftingLeader) -> usize {
        let what = describe(&leader.header, "drafting_element/leader");
        let mut geometry = LeaderGeometry {
            vertices: vec![
                self.plan_vertex(&leader.arrow),
                self.plan_vertex(&leader.hook),
            ],
            ..LeaderGeometry::default()
        };
        let length = geometry.vertices[0].distance_to(geometry.vertices[1]);
        if !length.is_finite() {
            self.problem(format!(
                "{what} skipped: its arrow-to-hook distance is too large"
            ));
            return 0;
        }
        // Paragraph escapes belong to drafting notes, not to file paths in
        // general tokens.
        geometry.text = unescape_typed(
            &leader
                .fields
                .text("display_text", &leader.fields.text("format_text", "")),
        );
        for key in ["arrow_size", "hook_length", "text_size"] {
            let fields = if leader.overrides.find(key).is_some() {
                &leader.overrides
            } else {
                &leader.fields
            };
            if fields.find(key).is_none() {
                continue;
            }
            let size = match fields.real(key) {
                Some(size) if size.is_finite() && !(size < 0.0) => size,
                _ => {
                    self.problem(format!(
                        "{what} skipped: {key} must be finite and non-negative"
                    ));
                    return 0;
                }
            };
            match key {
                "arrow_size" => geometry.arrow_size = size,
                "hook_length" => geometry.landing = size,
                _ => geometry.paper_height = size,
            }
        }
        let angle = if leader.fields.find("hook_angle").is_none() {
            Some(0.0)
        } else {
            leader.fields.real("hook_angle")
        };
        let Some(angle) = angle.filter(|angle| angle.is_finite()) else {
            self.problem(format!("{what} skipped: hook_angle must be finite"));
            return 0;
        };
        let justify = lower(&leader.fields.text("justify_x", "Left_Justify"));
        if justify != "left_justify" && justify != "right_justify" {
            self.problem(format!(
                "{what} skipped: justify_x must be Left_Justify or Right_Justify"
            ));
            return 0;
        }
        geometry.landing_angle = Some(
            radians(ieee_remainder(angle, 360.0))
                + if justify == "right_justify" { PI } else { 0.0 },
        );
        let mut entity = self.make_entity(&leader.header, "drafting_element/leader");
        entity.style.clear();
        for field in leader.fields.fields() {
            meta(
                &mut entity,
                format!("{META_LEADER_PREFIX}{}", field.key),
                property_of(field),
            );
        }
        for field in leader.overrides.fields() {
            meta(
                &mut entity,
                format!("{META_LEADER_PREFIX}overrides.{}", field.key),
                property_of(field),
            );
        }
        if let Some(reference) = &leader.arrow_reference {
            keep_drafting_reference(
                &mut entity,
                reference,
                &format!("{META_LEADER_PREFIX}arrow"),
            );
            self.leader_references += 1;
        }
        let named_style = leader.fields.text("style_name", "");
        if !named_style.is_empty() && self.noted.insert(format!("drafting-style:{named_style}")) {
            self.problem(format!(
                "drafting style \"{named_style}\" is not defined in the archive; leaders use the default text appearance with their explicit size overrides"
            ));
        }
        entity.geometry = Geometry::Leader(geometry);
        usize::from(self.add(entity, &what))
    }

    fn import_plot_frame(&mut self, frame: &PlotFrame) -> usize {
        // The sheet on the ground: paper millimetres times the plot scale.
        let width = frame.fields.real("width");
        let height = frame.fields.real("height");
        let scale = frame.fields.real("scale").unwrap_or(1000.0);
        let (Some(width), Some(height)) = (width, height) else {
            self.problem(format!(
                "{} skipped: it has no width, height or scale",
                describe(&frame.header, "string plot_frame")
            ));
            return 0;
        };
        if !(width > 0.0) || !(height > 0.0) || !(scale > 0.0) {
            self.problem(format!(
                "{} skipped: it has no width, height or scale",
                describe(&frame.header, "string plot_frame")
            ));
            return 0;
        }
        let origin = self.plan(
            frame.fields.real("xorigin").unwrap_or(0.0),
            frame.fields.real("yorigin").unwrap_or(0.0),
        );
        let w = width * scale / 1000.0;
        let h = height * scale / 1000.0;
        let rotation = radians(frame.fields.real("rotation").unwrap_or(0.0));
        let along = Vec2::new(rotation.cos(), rotation.sin());
        let up = Vec2::new(-along.y, along.x);
        let sheet = Polyline2::new(
            vec![
                origin,
                origin + along * w,
                origin + along * w + up * h,
                origin + up * h,
            ],
            true,
        );

        let mut entity = self.make_entity(&frame.header, "string plot_frame");
        entity.geometry = Geometry::Polyline(sheet);
        for field in frame.fields.fields() {
            add_property(
                &mut entity.properties,
                format!("plot_frame.{}", field.key),
                property_of(field),
            );
        }
        usize::from(self.add(entity, &describe(&frame.header, "string plot_frame")))
    }

    fn add_record_point(
        &mut self,
        header: &StringHeader,
        record: &DrainageRecord,
        kind: &str,
        added: &mut usize,
    ) {
        let (Some(x), Some(y)) = (record.fields.real("x"), record.fields.real("y")) else {
            self.problem(format!(
                "{kind} \"{}\" of {} skipped: it has no x and y",
                record.fields.text("name", ""),
                describe(header, "string drainage")
            ));
            return;
        };
        let mut entity = self.make_entity(header, &format!("drainage {kind}"));
        entity.properties.clear();
        entity.geometry = Geometry::Point(PointGeometry {
            position: self.plan(x, y),
        });
        for field in record.fields.fields() {
            if field.key != "x" && field.key != "y" && field.key != "z" {
                add_property(
                    &mut entity.properties,
                    format!("{kind}.{}", field.key),
                    property_of(field),
                );
            }
        }
        if self.options.attributes_as_properties {
            flatten_attributes(&record.attributes, "", &mut entity.properties);
        }
        // Manual 1.5.3: `z` is "the z-value of top of node" for a pit, and
        // "internal use only" for a house connection or property control -
        // the format's program writes 0 there, 30 m below the job. A house
        // connection's height is its adopted level, or its level.
        let elevation = if kind == "pit" {
            record.fields.real("z")
        } else if kind == "house_connection" {
            record
                .fields
                .real("adopted_level")
                .or_else(|| record.fields.real("level"))
        } else {
            None
        };
        if let Some(elevation) = elevation
            && elevation != self.archive.null_value
        {
            entity.properties.insert(
                String::from(ELEVATION_PROPERTY),
                PropertyValue::Real(elevation),
            );
        }
        let what = format!("{kind} of {}", describe(header, "string drainage"));
        if self.add(entity, &what) {
            *added += 1;
        }
    }

    fn import_drainage(&mut self, drainage: &DrainageString) -> usize {
        let mut added = 0;
        let mut line = self.make_entity(&drainage.header, "string drainage");
        if self.set_string_geometry(
            &mut line,
            &drainage.vertices,
            &drainage.segments,
            false,
            None,
        ) {
            if let Some(outfall) = drainage.outfall {
                add_property(
                    &mut line.properties,
                    String::from("outfall"),
                    PropertyValue::Real(outfall),
                );
            }
            if let Some(direction) = drainage.flow_direction {
                add_property(
                    &mut line.properties,
                    String::from("flow_direction"),
                    PropertyValue::Integer(i64::from(direction)),
                );
            }
            // Pipe i joins pit i to pit i + 1. It has no position of its own,
            // so it travels with the line.
            for (i, pipe) in drainage.pipes.iter().enumerate() {
                let prefix = format!("pipe.{}.", i + 1);
                for field in pipe.fields.fields() {
                    add_property(
                        &mut line.properties,
                        format!("{prefix}{}", field.key),
                        property_of(field),
                    );
                }
                if self.options.attributes_as_properties {
                    flatten_attributes(
                        &pipe.attributes,
                        &format!("{prefix}attributes"),
                        &mut line.properties,
                    );
                }
            }
            if self.add(line, &describe(&drainage.header, "string drainage")) {
                added += 1;
            }
        }
        for pit in &drainage.pits {
            self.add_record_point(&drainage.header, pit, "pit", &mut added);
        }
        for connection in &drainage.house_connections {
            self.add_record_point(&drainage.header, connection, "house_connection", &mut added);
        }
        for control in &drainage.property_controls {
            if control.vertices.is_empty() {
                self.add_record_point(&drainage.header, control, "property_control", &mut added);
                continue;
            }
            let mut entity = self.make_entity(&drainage.header, "drainage property_control");
            entity.properties.clear();
            if self.set_string_geometry(
                &mut entity,
                &control.vertices,
                &control.segments,
                false,
                None,
            ) {
                for field in control.fields.fields() {
                    add_property(
                        &mut entity.properties,
                        format!("property_control.{}", field.key),
                        property_of(field),
                    );
                }
                if self.options.attributes_as_properties {
                    flatten_attributes(&control.attributes, "", &mut entity.properties);
                }
                let what = format!(
                    "property control of {}",
                    describe(&drainage.header, "string drainage")
                );
                if self.add(entity, &what) {
                    added += 1;
                }
            }
        }
        added
    }

    // ---- alignments ---------------------------------------------------------------------

    fn pis_from_parts(&self, alignment: &SuperAlignment) -> Reconstruction {
        let mut out = Reconstruction::new();
        for part in &alignment.horizontal_parts {
            let (Some(x), Some(y)) = (part.fields.real("x"), part.fields.real("y")) else {
                out.failure = String::from("an IP has no coordinates");
                return out;
            };
            let mut pi = AlignmentPi {
                point: self.plan(x, y),
                radius: part.fields.real("r").unwrap_or(0.0).abs(),
                ..AlignmentPi::default()
            };
            if part.kind == "spiral" {
                pi.spiral_in = part.fields.real("l1").unwrap_or(0.0).abs();
                pi.spiral_out = part.fields.real("l2").unwrap_or(0.0).abs();
            }
            out.horizontal.pis.push(pi);
        }
        out
    }

    fn pis_from_elements(&self, data: &SolvedGeometry) -> Reconstruction {
        let vertex_count = data.vertices.len();
        if vertex_count < 2 {
            return Reconstruction::failed("its solved geometry has fewer than two vertices");
        }
        let vertex = |index: usize| self.plan_vertex(&data.vertices[index]);
        let kind_of = |index: usize| {
            data.segments
                .get(index)
                .map_or(SegmentKind::Straight, |segment| segment.kind)
        };
        let length_of = |index: usize| {
            let p = &data.segments[index].parameters;
            (p.real("l2").unwrap_or(0.0) - p.real("l1").unwrap_or(0.0)).abs()
        };
        let is_clothoid = |index: usize| {
            let kind = lower(&data.segments[index].parameters.text("type", "clothoid"));
            kind == "clothoid" || kind == "natural clothoid"
        };
        let transition_type = |index: usize| {
            format!(
                "its transitions are of type \"{}\" and Katana's are clothoids",
                data.segments[index].parameters.text("type", "")
            )
        };
        let mut out = Reconstruction::new();
        let segment_count = vertex_count - 1;
        out.horizontal.pis.push(AlignmentPi {
            point: vertex(0),
            ..AlignmentPi::default()
        });

        let mut i = 0;
        while i < segment_count {
            if kind_of(i) == SegmentKind::Straight {
                if i + 1 < segment_count && kind_of(i + 1) == SegmentKind::Straight {
                    // two straights: a kink
                    out.horizontal.pis.push(AlignmentPi {
                        point: vertex(i + 1),
                        ..AlignmentPi::default()
                    });
                }
                i += 1;
                continue;
            }
            // One curve: [transition] arc [transition].
            let mut k = i;
            let mut spiral_in = 0.0;
            let mut spiral_out = 0.0;
            if kind_of(k) == SegmentKind::Curve {
                out.failure = String::from("it contains an offset transition");
                return out;
            }
            if kind_of(k) == SegmentKind::Spiral {
                if !is_clothoid(k) {
                    out.failure = transition_type(k);
                    return out;
                }
                spiral_in = length_of(k);
                k += 1;
            }
            if k >= segment_count || kind_of(k) != SegmentKind::Arc {
                out.failure = String::from("a transition is not followed by an arc");
                return out;
            }
            let radius = data.segments[k].radius.abs();
            k += 1;
            if k < segment_count && kind_of(k) == SegmentKind::Spiral {
                if !is_clothoid(k) {
                    out.failure = transition_type(k);
                    return out;
                }
                spiral_out = length_of(k);
                k += 1;
            }
            if i == 0 {
                out.failure = String::from("it begins on a curve");
                return out;
            }
            if k == segment_count {
                out.failure = String::from("it ends on a curve");
                return out;
            }
            let entering = tangent_of(&data.segments[i], vertex(i), vertex(i + 1), true);
            let away = tangent_of(&data.segments[k - 1], vertex(k - 1), vertex(k), false);
            let (Some(entering), Some(away)) = (entering, away) else {
                out.failure = String::from("a curve does not record its tangents");
                return out;
            };
            // PI: where the tangent entering the curve meets the one leaving
            // it.
            let d1 = Vec2::new(entering.cos(), entering.sin());
            let d2 = Vec2::new(away.cos(), away.sin());
            let cross = d1.x * d2.y - d1.y * d2.x;
            if cross.abs() < 1e-9 {
                out.failure = String::from("a curve turns through 180 degrees or not at all");
                return out;
            }
            let gap = vertex(k) - vertex(i);
            let t = (gap.x * d2.y - gap.y * d2.x) / cross;
            out.horizontal.pis.push(AlignmentPi {
                point: vertex(i) + d1 * t,
                radius,
                spiral_in,
                spiral_out,
            });
            i = k;
        }
        out.horizontal.pis.push(AlignmentPi {
            point: vertex(vertex_count - 1),
            ..AlignmentPi::default()
        });
        out
    }

    fn import_alignment(&mut self, alignment: &SuperAlignment) -> usize {
        let keyword = match alignment.source {
            AlignmentSource::Alignment => "string alignment",
            AlignmentSource::Pipeline => "string pipeline",
            AlignmentSource::SuperAlignment => "string super_alignment",
        };
        let what = describe(&alignment.header, keyword);
        let mut added = 0;

        // The definition Katana can hold, if there is one.
        let mut reconstruction = if alignment.horizontal_is_ip_only() {
            self.pis_from_parts(alignment)
        } else if let Some(data) = &alignment.horizontal_data {
            self.pis_from_elements(data)
        } else {
            Reconstruction::failed("it has neither IPs nor solved geometry")
        };
        let spiral = lower(&alignment.spiral_type);
        let uses_spirals = reconstruction
            .horizontal
            .pis
            .iter()
            .any(|pi| pi.spiral_in > 0.0 || pi.spiral_out > 0.0);
        let mut approximation = String::new();
        if reconstruction.failure.is_empty()
            && alignment.horizontal_is_ip_only()
            && uses_spirals
            && !spiral.is_empty()
            && spiral != "clothoid"
            && spiral != "natural clothoid"
        {
            // Defined by IPs, with a transition Katana does not have. Where
            // the archive also holds the solved geometry (below) that is
            // checked and the alignment is refused if it differs; here there
            // is nothing to check against, and an alignment with clothoids of
            // the same lengths - out by centimetres on a tight railway curve -
            // is worth far more than no alignment. It is said, per alignment.
            approximation = format!(
                " its transitions are of type \"{}\" and are represented by clothoids of the same length, which differ from them by up to a few centimetres",
                alignment.spiral_type
            );
        }
        reconstruction.horizontal.start_station = alignment.header.chainage;

        let mut solved: Option<SolvedAlignment> = None;
        if reconstruction.failure.is_empty() {
            match solve_alignment(&reconstruction.horizontal) {
                Err(error) => reconstruction.failure = error.message,
                Ok(attempt) => solved = Some(attempt),
            }
        }
        // The check that makes the reconstruction trustworthy: every vertex
        // the archive recorded must lie on what Katana solved.
        let departure = match (&solved, &alignment.horizontal_data) {
            (Some(solution), Some(data)) => {
                let line = solution.to_polyline(self.options.alignment_tolerance * 0.1);
                let mut worst = 0.0;
                for vertex in &data.vertices {
                    raise_to(
                        &mut worst,
                        line.distance_to(self.plan_vertex(vertex)).unwrap_or(0.0),
                    );
                }
                Some(worst)
            }
            _ => None,
        };
        if let Some(worst) = departure
            && worst > self.options.alignment_tolerance
        {
            reconstruction.failure = format!(
                "the alignment Katana solves from its PIs departs from the archive's by {}",
                format_real(worst, 3)
            );
            solved = None;
        }

        // The centreline as a polyline - always, so that an alignment Katana
        // cannot define is still on the drawing.
        let mut entity = self.make_entity(&alignment.header, keyword);
        let mut have_line = false;
        if let Some(data) = &alignment.horizontal_data {
            have_line = self.set_string_geometry(
                &mut entity,
                &data.vertices,
                &data.segments,
                data.closed,
                None,
            );
        } else if let Some(solution) = &solved {
            entity.geometry =
                Geometry::Polyline(solution.to_polyline(self.options.curve_tolerance));
            have_line = true;
        }
        if have_line {
            if let Some(diameter) = alignment.diameter {
                meta(&mut entity, "12d.diameter", diameter);
            }
            if self.add(entity, &what) {
                added += 1;
            }
        }

        if solved.is_none() {
            // Nothing solved and nothing recorded: the tangent polygon through
            // the IPs is still worth drawing, and is what the designer placed.
            if !have_line && !reconstruction.horizontal.pis.is_empty() {
                let vertices: Vec<Point2> = reconstruction
                    .horizontal
                    .pis
                    .iter()
                    .map(|pi| pi.point)
                    .collect();
                let mut fallback = self.make_entity(&alignment.header, keyword);
                fallback.geometry = if vertices.len() == 1 {
                    Geometry::Point(PointGeometry {
                        position: vertices[0],
                    })
                } else {
                    Geometry::Polyline(Polyline2::new(vertices, false))
                };
                let drawn = self.add(fallback, &what);
                self.problem(format!(
                    "{what}{}{}",
                    if drawn {
                        " is imported as the polygon through its IPs only: "
                    } else {
                        " skipped: "
                    },
                    reconstruction.failure
                ));
                return added + usize::from(drawn);
            }
            self.problem(format!(
                "{what}{}{}",
                if have_line {
                    " is imported as a polyline only: "
                } else {
                    " skipped: "
                },
                reconstruction.failure
            ));
            return added;
        }
        if !approximation.is_empty() {
            self.problem(format!("{what}:{approximation}"));
        }

        let mut record = Alignment {
            name: self.unique_alignment_name(&alignment.header.name),
            description: format!("Imported from archive model \"{}\"", alignment.header.model),
            horizontal: reconstruction.horizontal,
            vertical: None,
        };

        let mut notes = Vec::new();
        let mut vertical = VerticalAlignment::default();
        let mut vertical_failure = String::new();
        if let Some(data) = &alignment.vertical_data
            && data.vertices.len() >= 2
        {
            vertical_failure = pvis_from_elements(data, &mut vertical, &mut notes);
        } else if alignment.vertical_is_ip_only() && alignment.vertical_parts.len() >= 2 {
            vertical_failure = pvis_from_parts(alignment, &mut vertical, &mut notes);
        }
        if vertical_failure.is_empty() && vertical.pvis.len() >= 2 {
            match solve_profile(&vertical) {
                Err(error) => vertical_failure = error.message,
                Ok(profile) => {
                    if let Some(data) = &alignment.vertical_data {
                        // The same check the plan geometry gets: every vertex
                        // the archive recorded must lie on the grade line
                        // Katana solves.
                        let mut worst = 0.0;
                        for vertex in &data.vertices {
                            let deviation = match profile.elevation_at(vertex.x) {
                                Some(height) => (height - vertex.y).abs(),
                                None => self.options.alignment_tolerance * 2.0,
                            };
                            raise_to(&mut worst, deviation);
                        }
                        if worst > self.options.alignment_tolerance {
                            vertical_failure = format!(
                                "the grade line Katana solves from its VIPs departs from the archive's by {}",
                                format_real(worst, 3)
                            );
                        }
                    }
                }
            }
            if vertical_failure.is_empty() {
                record.vertical = Some(vertical);
            }
        }
        if !vertical_failure.is_empty() {
            self.problem(format!(
                "{what}: its vertical geometry was not imported: {vertical_failure}"
            ));
        }
        for note in notes {
            if record.vertical.is_some() && self.noted.insert(note.clone()) {
                self.result.warnings.push(note);
            }
        }
        if let Err(error) = validate(&record) {
            self.problem(format!(
                "{what} is imported as a polyline only: {}",
                error.message
            ));
            return added;
        }
        self.result.alignments.push(record);
        added + 1
    }

    fn unique_alignment_name(&mut self, wanted: &str) -> String {
        let base = if wanted.is_empty() {
            "Alignment"
        } else {
            wanted
        };
        let mut name = String::from(base);
        let mut copy = 2;
        while !self.alignment_names.insert(lower(&name)) {
            name = format!("{base} ({copy})");
            copy += 1;
        }
        name
    }

    // ---- surfaces, clouds, meshes -----------------------------------------------------------

    fn import_tin(&mut self, tin: &Tin) -> usize {
        let what = format!(
            "{} \"{}\"",
            if tin.full { "full_tin" } else { "tin" },
            tin.name
        );
        let mut surface = ImportedSurface {
            name: if tin.name.is_empty() {
                String::from("tin")
            } else {
                tin.name.clone()
            },
            colour: tin.colour.clone(),
            triangles_in_file: tin.triangles.len(),
            ..ImportedSurface::default()
        };

        const UNUSED: u32 = 0xFFFF_FFFF;
        let mut remap = vec![UNUSED; tin.points.len()];
        let mut vertices: Vec<Point3> = Vec::new();
        let mut triangles: Vec<TinTriangle> = Vec::new();
        for (i, triangle) in tin.triangles.iter().enumerate() {
            let corners = [
                &tin.points[triangle[0] as usize],
                &tin.points[triangle[1] as usize],
                &tin.points[triangle[2] as usize],
            ];
            let has_heights = corners.iter().all(|corner| corner.z.is_some());
            let distinct = triangle[0] != triangle[1]
                && triangle[1] != triangle[2]
                && triangle[2] != triangle[0];
            // A triangle with a null height has no surface to give: the
            // format's program treats it as null too.
            if !tin.is_surface_triangle(i) || !has_heights || !distinct {
                surface.triangles_nulled += 1;
                continue;
            }
            let shape = Triangle2::new(
                self.plan_vertex(corners[0]),
                self.plan_vertex(corners[1]),
                self.plan_vertex(corners[2]),
            );
            // Nor has one with no area in PLAN: there is no ground under it to
            // interpolate over. Worse, which way round such a triangle was
            // listed is noise rather than information - its doubled area is
            // below the rounding error of computing it - so keeping it would
            // hand TinSurface an edge direction chosen by the last bits of a
            // subtraction, and one inside-out triangle refuses the whole
            // surface. Real tins carry them: `plot_PW_example_data.12da` has
            // seven, the smallest with a doubled area of 4.9e-15 m^2 over
            // coordinates of 6.2e6 m.
            if shape.is_degenerate() {
                surface.triangles_nulled += 1;
                continue;
            }
            let mut mapped: TinTriangle = [0; 3];
            for (slot, (&index, corner)) in mapped.iter_mut().zip(triangle.iter().zip(corners)) {
                let remapped = &mut remap[index as usize];
                if *remapped == UNUSED {
                    *remapped = vertices.len() as u32;
                    let p = self.plan_vertex(corner);
                    vertices.push(Point3::new(p.x, p.y, corner.z.unwrap_or(0.0)));
                }
                *slot = *remapped;
            }
            // The format lists a triangle clockwise seen from above;
            // TinSurface wants it counter-clockwise. The sign is MEASURED
            // rather than assumed, because other writers get this wrong, and
            // a surface built inside out fails as a whole.
            if shape.signed_area() < 0.0 {
                mapped.swap(1, 2);
            }
            triangles.push(mapped);
        }
        if triangles.is_empty() {
            self.problem(format!("{what} skipped: it has no visible triangles"));
            return 0;
        }
        match TinSurface::create(vertices, triangles, Vec::new()) {
            Err(error) => {
                self.problem(format!("{what} skipped: {}", error.describe()));
                0
            }
            Ok(built) => {
                surface.surface = built;
                self.result.bounds.expand_box(*surface.surface.bounds());
                self.result.surfaces.push(surface);
                if !tin.colours.is_empty() {
                    self.coloured_tins += 1;
                }
                1
            }
        }
    }

    fn import_super_tin(&mut self, super_tin: &SuperTin) -> usize {
        // Nothing to build: a super tin is a ranking of tins, and the tins
        // themselves are what carries the surface.
        self.result.super_tins.push(super_tin.clone());
        1
    }

    // A trimesh becomes a mesh in the session, beside the surfaces. Its
    // vertices come across whole - the file's indices are one-based and the
    // reader has already subtracted - and a face that names a vertex which
    // does not exist is dropped and counted rather than costing the mesh.
    fn import_trimesh(&mut self, trimesh: &Trimesh) -> usize {
        let mut imported = ImportedMesh {
            name: if trimesh.name.is_empty() {
                String::from("trimesh")
            } else {
                trimesh.name.clone()
            },
            layer: layer_path_for_model(&trimesh.model, &self.options.layer_prefix),
            colour_name: trimesh.colour.clone(),
            edges_in_file: trimesh.edges.len(),
            vertex_infos_in_file: trimesh.vertex_infos.len(),
            edge_infos_in_file: trimesh.edge_infos.len(),
            ..ImportedMesh::default()
        };
        imported.color = self.colour_for(&trimesh.colour);
        if self.options.attributes_as_properties {
            flatten_attributes(&trimesh.attributes, "", &mut imported.properties);
        }

        imported.mesh.vertices.reserve(trimesh.vertices.len());
        for vertex in &trimesh.vertices {
            // A mesh vertex with no height is not a vertex: a face that used
            // it is dropped below, because it names a point in space.
            imported
                .mesh
                .vertices
                .push(Point3::new(vertex.x, vertex.y, vertex.z.unwrap_or(0.0)));
        }
        let mut dropped = 0;
        let count = trimesh.vertices.len() as u32;
        for (i, face) in trimesh.faces.iter().enumerate() {
            let named = face[0] < count && face[1] < count && face[2] < count;
            let distinct = face[0] != face[1] && face[1] != face[2] && face[2] != face[0];
            let solid = named
                && face
                    .iter()
                    .all(|&index| trimesh.vertices[index as usize].z.is_some());
            if !named || !distinct || !solid {
                dropped += 1;
                continue;
            }
            imported.mesh.faces.push(*face);
            // face_flags are ONE-based into face_infos, 0 meaning none (manual
            // 1.4.9); a flag out of range is treated as none.
            let mut colour = String::new();
            if let Some(&flag) = trimesh.face_flags.get(i)
                && flag != 0
                && (flag as usize) <= trimesh.face_infos.len()
            {
                colour = trimesh.face_infos[flag as usize - 1].colour.clone();
            }
            let color = if colour.is_empty() {
                None
            } else {
                self.colour_for(&colour)
            };
            imported.face_colour_names.push(colour);
            imported.face_colors.push(color);
        }
        // All or nothing: a table of face colours with a gap in it would be
        // read by position, so if no face was coloured it is no table.
        if imported
            .face_colour_names
            .iter()
            .all(|colour| colour.is_empty())
        {
            imported.face_colour_names.clear();
            imported.face_colors.clear();
        }
        if dropped != 0 {
            self.trimesh_faces_dropped += dropped;
        }
        if imported.mesh.faces.is_empty() {
            self.trimeshes_empty += 1;
            return 0;
        }
        self.result.meshes.push(imported);
        1
    }

    fn import_cloud(&mut self, cloud: &LasCloud) -> usize {
        let mut imported = ImportedCloud {
            name: if cloud.header.name.is_empty() {
                String::from("archive cloud")
            } else {
                cloud.header.name.clone()
            },
            reference_file: cloud.reference_file.clone(),
            ..ImportedCloud::default()
        };
        imported.points.reserve(cloud.points.len());
        for point in &cloud.points {
            let p = self.plan(point.x, point.y);
            imported.points.push(ImportedCloudPoint {
                x: p.x,
                y: p.y,
                z: point.z,
                intensity: point.intensity,
                classification: point.classification,
            });
            self.result.bounds.expand(p);
        }
        if imported.points.is_empty() && imported.reference_file.is_empty() {
            self.problem(format!(
                "{} skipped: it has no points",
                describe(&cloud.header, "string las_cloud_data")
            ));
            return 0;
        }
        self.result.clouds.push(imported);
        1
    }

    // A super tin is a ranking of tins, and the tins are read as separate
    // surfaces - so this can only be done once every element has been, and it
    // is: the list may name a tin that appears later in the file.
    fn build_super_tins(&mut self) {
        // The members stay in the list as well. A tin and a super tin are
        // separate objects in an archive, and a user who asked for both should
        // get both rather than have the parts swallowed by the whole.
        let members = self.result.surfaces.len();
        let super_tins = self.result.super_tins.clone();
        for super_tin in &super_tins {
            let mut ranked = Vec::new();
            let mut missing = Vec::new();
            for name in &super_tin.tins {
                // Names are compared without case (manual 1.4.8).
                let wanted = lower(name);
                match self.result.surfaces[..members]
                    .iter()
                    .position(|surface| lower(&surface.name) == wanted)
                {
                    Some(found) => ranked.push(found),
                    None => missing.push(name.clone()),
                }
            }
            if !missing.is_empty() {
                self.problem(format!(
                    "super tin \"{}\" names {} tins the archive does not carry ({}); it is built from the rest",
                    super_tin.name,
                    missing.len(),
                    missing.join(", ")
                ));
            }
            if ranked.is_empty() {
                continue;
            }
            let members_in_rank_order: Vec<Option<&TinSurface>> = ranked
                .iter()
                .map(|&found| Some(&self.result.surfaces[found].surface))
                .collect();
            let combined = combine_surfaces(&members_in_rank_order);
            match combined {
                Err(error) => {
                    self.problem(format!(
                        "super tin \"{}\" could not be built: {}",
                        super_tin.name,
                        error.describe()
                    ));
                }
                Ok(combined) => {
                    let surface = ImportedSurface {
                        name: if super_tin.name.is_empty() {
                            String::from("super tin")
                        } else {
                            super_tin.name.clone()
                        },
                        colour: super_tin.colour.clone(),
                        triangles_in_file: combined.triangle_count(),
                        surface: combined,
                        triangles_nulled: 0,
                    };
                    self.result.bounds.expand_box(*surface.surface.bounds());
                    self.result.surfaces.push(surface);
                }
            }
        }
    }

    fn finish_warnings(&mut self) {
        if self.named_problems > MAX_NAMED_PROBLEMS {
            let further = self.named_problems - MAX_NAMED_PROBLEMS;
            self.result.warnings.push(format!(
                "{further} further elements had problems and are not listed"
            ));
        }
        if self.trimeshes_empty != 0 {
            self.result.warnings.push(format!(
                "{} trimeshes have no triangle that names three distinct vertices with heights, and are not imported",
                self.trimeshes_empty
            ));
        }
        if self.trimesh_faces_dropped != 0 {
            self.result.warnings.push(format!(
                "{} mesh faces were dropped: they name a vertex that does not exist, repeat one, or have no height",
                self.trimesh_faces_dropped
            ));
        }
        if self.partly_invisible != 0 {
            self.result.warnings.push(format!(
                "{} strings have invisible vertices or segments, which are drawn (the flags are kept in the entity metadata)",
                self.partly_invisible
            ));
        }
        if self.symbols_left != 0 {
            self.result.warnings.push(format!(
                "{} strings carry a DIFFERENT symbol per vertex, which are kept and written back but not drawn: a string is drawn with one symbol, from its style, and there is no one symbol here",
                self.symbols_left
            ));
        }
        if self.annotations_left != 0 {
            self.result.warnings.push(format!(
                "{} strings carry per-vertex or per-segment annotation settings beyond the text height and angle that were taken",
                self.annotations_left
            ));
        }
        if self.point_strings_split != 0 {
            self.problem(format!(
                "{} strings marked `breakline point` held more than one vertex and became {} points, which is what that flag means",
                self.point_strings_split, self.points_from_strings
            ));
        }
        if self.coloured_tins != 0 {
            self.result.warnings.push(format!(
                "{} surfaces carry per-triangle colours, which a surface here does not have",
                self.coloured_tins
            ));
        }
        if self.chord_report.arcs + self.chord_report.transitions != 0 {
            self.result.warnings.push(format!(
                "{} arcs and {} transitions were chorded to within {} of the curve",
                self.chord_report.arcs,
                self.chord_report.transitions,
                format_real(self.options.curve_tolerance, 6)
            ));
        }
        for (kind, &adjustment) in &self.chord_report.adjustment_by_type {
            self.result.warnings.push(format!(
                "transitions of type \"{kind}\" are drawn as clothoids and moved by up to {} to meet the end points the archive recorded",
                format_real(adjustment, 3)
            ));
        }
        if self.chord_report.straightened != 0 {
            self.result.warnings.push(format!(
                "{} curved segments could not be reproduced from their parameters and are drawn as straight lines",
                self.chord_report.straightened
            ));
        }
        let archive = self.archive;
        for (path, count) in &archive.unrecognised {
            self.result
                .warnings
                .push(format!("not read: {path} ({count})"));
        }
        if self.leader_references != 0 {
            self.result.warnings.push(format!(
                "{} leaders carry external arrow references, retained as metadata; their recorded arrow coordinates are used without guessing a target entity",
                self.leader_references
            ));
        }
        let mut warnings = archive.warnings.clone();
        warnings.append(&mut self.result.warnings);
        self.result.warnings = warnings;
    }
}
