//! A graded utility schedule as drawing (docs/subsurface_utilities.md,
//! "Drawing the services"; include/katana/cad/utilities/utility_drawing.hpp,
//! src/katana_cad/utilities/utility_drawing.cpp): what UTILITY DRAW adds to a
//! document.
//!
//! The survey library grades located services by AS 5488 quality level
//! (katana_survey::subsurface); this turns the grading into layers, linetypes
//! and entities a person can see, plot and query. It is split in two so that
//! each half can be tested on its own:
//!
//! ```text
//! draw_utilities        the schedule -> a UtilityDrawing: every layer,
//!                       linetype and entity the drawing needs, as values.
//!                       Pure: it sees no document.
//! utility_draw_command  the drawing -> ONE command for a model: what the
//!                       model lacks is created, what it has is reused.
//! ```
//!
//! A service is drawn as one polyline per maximal run of consecutive segments
//! graded at the same level, so the plan shows exactly where the level
//! changes, on "<prefix>/<type>/QL-A" .. "QL-D", and one point per located
//! vertex on "<prefix>/<type>/points". The quality level is in the layer's
//! linetype - QL-A continuous, then dashed, dash-dot and dotted as the
//! evidence weakens - so it survives a monochrome plot; the kind of service is
//! in the layer's colour. Everything the grading found is on the entities as
//! "utility.*" properties, so an agent or the property panel can ask why a
//! stretch is QL-C without re-running the report.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{CommandPtr, Transaction, create_entities, create_layer, create_linetype};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::layer_path::{join_layer_path, layer_ancestors, validate_layer_path};
use katana_entity::tables::CONTINUOUS_LINETYPE;
use katana_entity::{
    Color, Entity, Geometry, Layer, Linetype, LinetypeElement, Model, PointGeometry, PropertyMap,
    PropertyValue, validate,
};
use katana_geometry::{Box2, Point2, Polyline2};
use katana_survey::subsurface::{
    self as sub, CoverResult, GradedSegment, GradedVertex, GradingSettings, LevelReference,
    QualityLevel, UtilityLine, UtilityStatus, UtilityType, UtilityVertex,
};

use crate::scope_verbs::record_value;
use crate::survey_tools::c_fixed;

/// The layer every drawing goes under unless LAYER names another.
pub const DEFAULT_UTILITY_LAYER_PREFIX: &str = "utilities";

/// The undo step's name: what Edit > Undo and the history say.
pub const UTILITY_DRAW_STEP: &str = "UTILITY DRAW";
/// And UTILITY REGRADE's (utility_data.rs).
pub const UTILITY_REGRADE_STEP: &str = "UTILITY REGRADE";

/// The first record of UTILITY DRAW's reply: format_utility_drawing's `record`
/// as the C++ defaults it.
pub const UTILITIES_DRAWN_RECORD: &str = "utilities drawn";

/// The properties a drawn service carries. One place for the names, since the
/// window, an agent and the tests all read them. A property is absent when its
/// value was not recorded or cannot be computed: absent is not zero. Two are
/// always there when their entity is: the type, which names the layer
/// ("unknown" when not recorded), and a level's reference, which the level is
/// read on (see LEVEL_REFERENCE).
pub mod keys {
    // On both the polylines and the points.
    /// The schedule's line id.
    pub const LINE: &str = "utility.line";
    /// utility_type_word.
    pub const TYPE: &str = "utility.type";
    /// The level the evidence supports: the run's level on a polyline, the
    /// vertex's own on a point. "QL-A" .. "QL-D".
    pub const QUALITY_LEVEL: &str = "utility.quality_level";
    // The service, on its polylines and on each of its points.
    pub const OWNER: &str = "utility.owner";
    pub const MATERIAL: &str = "utility.material";
    /// Metres.
    pub const DIAMETER: &str = "utility.diameter";
    pub const DIAMETER_INSIDE: &str = "utility.diameter_inside";
    pub const CONFIGURATION: &str = "utility.configuration";
    pub const DESCRIPTION: &str = "utility.description";
    /// "in service", "disused", "abandoned", "proposed"; absent when the
    /// schedule gives none, which the report lists as a missing attribute.
    pub const STATUS: &str = "utility.status";
    // On a polyline: the run.
    /// Why the run is below the better of its segments' ends, each distinct
    /// reason once, joined by "; ".
    pub const LIMITED_BY: &str = "utility.limited_by";
    /// The run's plan length, metres.
    pub const LENGTH: &str = "utility.length";
    /// The run's first vertex id.
    pub const FROM: &str = "utility.from";
    /// And its last.
    pub const TO: &str = "utility.to";
    // On a point: the vertex. A point carries the whole schedule row it was
    // drawn from - its line's attributes above as well as its own below - so
    // the points alone are the schedule, and the drawing can be graded,
    // reported, checked and written out again (utility_data.rs). The runs are
    // derived: UTILITY REGRADE draws them again from the points.
    pub const VERTEX: &str = "utility.vertex";
    /// The vertex's place along its line, from 1. Read as a number, so a point
    /// put in between two may take 2.5; two points of one line at one place
    /// are refused by name.
    pub const ORDER: &str = "utility.order";
    pub const METHOD: &str = "utility.method";
    pub const CLAIMED: &str = "utility.claimed";
    pub const OVER_CLAIM: &str = "utility.over_claim";
    /// The level as recorded, and the depth as recorded: what the schedule
    /// gave. SERVICE_LEVEL is what the grading made of them (the level, or the
    /// surface less the depth).
    pub const LEVEL: &str = "utility.level";
    pub const DEPTH: &str = "utility.depth";
    pub const HORIZONTAL_UNCERTAINTY: &str = "utility.h_unc";
    pub const VERTICAL_UNCERTAINTY: &str = "utility.v_unc";
    /// What is known of the service from this vertex to the next: "detected",
    /// "exposed" or "assumed". On every point but the last when the schedule
    /// gave a path for any segment of the line, else on none (every segment
    /// detected).
    pub const PATH: &str = "utility.path";
    pub const SERVICE_LEVEL: &str = "utility.service_level";
    /// With a level or a depth, always, and whenever it is not "top": the
    /// part of the service it is on, "top", "centre", "invert" or "unknown" -
    /// the reference the grading and the cover took it on. A level given with
    /// no reference is read as on the top (parse_utility_csv), so "top" is also
    /// what an unrecorded one reads as: the parsed schedule does not keep the
    /// difference.
    pub const LEVEL_REFERENCE: &str = "utility.level_ref";
    pub const LEVEL_QUALIFIED: &str = "utility.level_qualified";
    pub const SURFACE_LEVEL: &str = "utility.surface_level";
    pub const COVER: &str = "utility.cover";
    pub const COVER_NOTE: &str = "utility.cover_note";
    pub const COVER_BELOW_MINIMUM: &str = "utility.cover_below_minimum";
    pub const VERIFIES: &str = "utility.verifies";
    /// The settings the line was graded with, on each of its points, so that
    /// UTILITY REGRADE grades it again as it was drawn unless told otherwise:
    /// SPACING (GradingSettings::maximum_detected_spacing, metres; absent when
    /// it is infinite, the rule off) and MINCOVER (metres; absent when none
    /// was given). The points of one line must agree on them, as on its
    /// attributes.
    pub const SPACING: &str = "utility.spacing";
    pub const MINIMUM_COVER: &str = "utility.min_cover";
    /// A delivery schema's attributes that nothing interprets, kept by their
    /// own name after this prefix: "utility.field.AssetStatus". A polyline
    /// carries its service's; a point its service's and its own. Which are
    /// which is the schedule format's to say (utility_csv_columns, CarriedOn),
    /// as it said when the schedule was read.
    pub const FIELD_PREFIX: &str = "utility.field.";
    /// A cell the schedule reader reads as "not recorded", or reads only in
    /// part, as the schedule wrote it (UtilityAttributes::recorded and
    /// UtilityVertex::recorded), by the reader's column name after this
    /// prefix: "utility.recorded.size" = "Not Applicable". The line's (type,
    /// status, size) on its polylines and points; a vertex's (ql, level_ref)
    /// on its point. Nothing is graded from them: they are what UTILITY
    /// SCHEDULE and CHECK of the drawing write back, while they still read as
    /// the value held.
    pub const RECORDED_PREFIX: &str = "utility.recorded.";
    /// On a point UTILITY DRAW made from geometry in the drawing
    /// (utility_data.rs, read_geometry_services): the entity its line was read
    /// from, "#<entity id>". What says that entity is drawn already, so a draw
    /// of the same scope again draws only what is new. Provenance, not the
    /// schedule: no schedule column carries it, and REGRADE keeps it on the
    /// point as it keeps a person's own.
    pub const SOURCE: &str = "utility.source";
    /// On geometry a service is drawn from, never on a drawn point: what the
    /// heights of a line entity (every vertex) or a point (its vertex) are the
    /// heights of - "surface", "service" or "none" (GeometryHeights). A string
    /// shot on the ground marks whose potholes were shot on the pipe says it
    /// on those points.
    pub const HEIGHTS: &str = "utility.heights";
}

/// "water", "electricity", "telecommunications", "gas", "recycled-water",
/// "fire-service", "sewer", "stormwater", "fuel", "its", "other", "unknown":
/// one lower-case word per kind, for a layer name and a reply, where the
/// report's "recycled water" would need quoting.
pub fn utility_type_word(r#type: UtilityType) -> &'static str {
    match r#type {
        UtilityType::Unknown => "unknown",
        UtilityType::Electricity => "electricity",
        UtilityType::Telecommunications => "telecommunications",
        UtilityType::Gas => "gas",
        UtilityType::Water => "water",
        UtilityType::RecycledWater => "recycled-water",
        UtilityType::FireService => "fire-service",
        UtilityType::Sewer => "sewer",
        UtilityType::Stormwater => "stormwater",
        UtilityType::Fuel => "fuel",
        UtilityType::IntelligentTransport => "its",
        UtilityType::Other => "other",
    }
}

const fn hex(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

/// The colour a kind of service's layers are made in: AS 5488's. The
/// standard's primary code and line colour table (AS 5488.1, Appendix B) names
/// one for each service - water blue, electricity orange, communications
/// white, gas yellow, sewer cream, stormwater green, recycled water purple,
/// fire service red, unidentified services pink - and gives no sRGB, so each
/// value is Katana's rendering of the name. Petroleum (fuel) brown is the
/// least certain row: three of the four guides that state the table name it
/// and the fourth's chart does not, so if the standard has no colour for fuel,
/// brown is Katana's choice. The standard leaves the colours of a plan to
/// whoever draws it (clause 1.2.2 of the 2013 edition), so Katana follows its
/// table and is not required to. The Katana Standard survey library draws a
/// coded service in the same values, and a test holds the two equal. ITS and
/// "other" have no AS 5488 colour and are Katana's choice. Each reads at least
/// 3:1 against the plan view's ground (#1E2329). On white paper each does too,
/// except yellow gas and cream sewer, which cannot and still be yellow and
/// cream: they print light. Communications is white, which the plot prints
/// black (PlotSettings::white_to_black). docs/subsurface_utilities.md,
/// "Colours", has the sources and the table. Only a layer the drawing creates
/// takes it; a person's own colour on an existing layer stays.
pub fn utility_type_colour(r#type: UtilityType) -> Color {
    match r#type {
        UtilityType::Water => hex(0x2F, 0x80, 0xED), // blue
        UtilityType::Electricity => hex(0xE0, 0x70, 0x00), // orange
        // white: prints black (PlotSettings::white_to_black)
        UtilityType::Telecommunications => hex(0xFF, 0xFF, 0xFF),
        // yellow: 1.42:1 on white, prints light
        UtilityType::Gas => hex(0xFF, 0xD5, 0x00),
        UtilityType::RecycledWater => hex(0x9B, 0x59, 0xD0), // purple
        UtilityType::FireService => hex(0xE5, 0x39, 0x35),   // red
        // cream: 1.30:1 on white, prints light
        UtilityType::Sewer => hex(0xF0, 0xE2, 0xB0),
        UtilityType::Stormwater => hex(0x00, 0xA8, 0x00), // green
        // brown (petroleum): in three published guides, not a fourth's chart
        UtilityType::Fuel => hex(0xA3, 0x62, 0x15),
        // no AS 5488 colour: Katana's choice
        UtilityType::IntelligentTransport => hex(0x0B, 0x9B, 0xA8),
        // no AS 5488 colour: Katana's choice
        UtilityType::Other => hex(0x94, 0x94, 0x94),
        UtilityType::Unknown => hex(0xF2, 0x55, 0x9B), // pink (unidentified services)
    }
}

/// The linetype a quality level's layers are drawn in: "continuous" for QL-A,
/// "utility-ql-b", "utility-ql-c" and "utility-ql-d" for the others.
pub fn quality_level_linetype_name(level: QualityLevel) -> &'static str {
    match level {
        QualityLevel::A => CONTINUOUS_LINETYPE,
        QualityLevel::B => "utility-ql-b",
        QualityLevel::C => "utility-ql-c",
        QualityLevel::D => "utility-ql-d",
    }
}

/// That linetype's definition, in model metres for plans at 1:200 to 1:500:
/// QL-B dashed (1.5 dash, 0.75 gap), QL-C dash-dot (1.5 dash, 0.5 gap, dot,
/// 0.5 gap), QL-D dotted (dot, 0.6 gap). At 1:500 a QL-B dash is 3 mm on
/// paper; at 1:200, 7.5 mm. QL-A's is the built-in continuous line.
pub fn quality_level_linetype(level: QualityLevel) -> Linetype {
    let element = |length: f64| LinetypeElement { length };
    let (description, pattern) = match level {
        QualityLevel::A => ("", Vec::new()),
        QualityLevel::B => ("AS 5488 QL-B  __ __ __", vec![element(1.5), element(-0.75)]),
        QualityLevel::C => (
            "AS 5488 QL-C  __ . __ .",
            vec![element(1.5), element(-0.5), element(0.0), element(-0.5)],
        ),
        QualityLevel::D => ("AS 5488 QL-D  . . . .", vec![element(0.0), element(-0.6)]),
    };
    Linetype {
        name: String::from(quality_level_linetype_name(level)),
        description: String::from(description),
        pattern,
    }
}

/// "<prefix>/<type>/QL-B".
pub fn quality_level_layer_name(prefix: &str, r#type: UtilityType, level: QualityLevel) -> String {
    format!(
        "{prefix}/{}/{}",
        utility_type_word(r#type),
        sub::to_string(level)
    )
}

/// "<prefix>/<type>/points".
pub fn points_layer_name(prefix: &str, r#type: UtilityType) -> String {
    format!("{prefix}/{}/points", utility_type_word(r#type))
}

#[derive(Clone, Debug, PartialEq)]
pub struct UtilityDrawOptions {
    /// The grading, as REPORT grades: SPACING sets maximum_detected_spacing.
    pub grading: GradingSettings,
    pub layer_prefix: String,
    /// When given, each point whose cover is computed says whether it is below
    /// this (keys::COVER_BELOW_MINIMUM).
    pub minimum_cover: Option<f64>,
}

impl Default for UtilityDrawOptions {
    fn default() -> Self {
        Self {
            grading: GradingSettings::default(),
            layer_prefix: String::from(DEFAULT_UTILITY_LAYER_PREFIX),
            minimum_cover: None,
        }
    }
}

/// One service as drawn, for the reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawnUtilityLine {
    pub id: String,
    pub r#type: UtilityType,
    pub length: f64,
    /// Plan length at each level, indexed by `QualityLevel as usize`.
    pub length_at: [f64; 4],
    pub polylines: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UtilityDrawing {
    /// Every layer the entities are on and every ancestor of one, parents
    /// first, as the drawing would create them.
    pub layers: Vec<Layer>,
    /// The linetypes those layers name, other than "continuous".
    pub linetypes: Vec<Linetype>,
    /// Per service in schedule order: its runs, then its points. Ids unset.
    pub entities: Vec<Entity>,
    pub lines: Vec<DrawnUtilityLine>,
    pub vertices: usize,
    pub segments: usize,
    /// How many distinct layers hold an entity: created or reused, not the
    /// ancestors that only group them.
    pub drawn_layers: usize,
    /// Every located vertex.
    pub bounds: Box2,
}

// A plan point from a survey coordinate: x is the easting.
fn plan_point(vertex: &UtilityVertex) -> Point2 {
    Point2::new(vertex.position.easting, vertex.position.northing)
}

fn text(value: &str) -> PropertyValue {
    PropertyValue::Text(String::from(value))
}

fn set_text(properties: &mut PropertyMap, key: &str, value: &str) {
    if !value.is_empty() {
        properties.insert(String::from(key), text(value));
    }
}

fn set_real(properties: &mut PropertyMap, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        properties.insert(String::from(key), PropertyValue::Real(value));
    }
}

fn set_fields(properties: &mut PropertyMap, fields: &BTreeMap<String, String>, prefix: &str) {
    for (name, value) in fields {
        set_text(properties, &format!("{prefix}{name}"), value);
    }
}

// What a polyline of a service carries whatever its level.
fn line_properties(line: &UtilityLine) -> PropertyMap {
    let attributes = &line.attributes;
    let mut properties = PropertyMap::new();
    properties.insert(String::from(keys::LINE), text(&line.id));
    properties.insert(
        String::from(keys::TYPE),
        text(utility_type_word(attributes.r#type)),
    );
    set_text(&mut properties, keys::OWNER, &attributes.owner);
    set_text(&mut properties, keys::MATERIAL, &attributes.material);
    if attributes.diameter > 0.0 {
        properties.insert(
            String::from(keys::DIAMETER),
            PropertyValue::Real(attributes.diameter),
        );
        properties.insert(
            String::from(keys::DIAMETER_INSIDE),
            PropertyValue::Boolean(attributes.diameter_is_inside),
        );
    }
    set_text(
        &mut properties,
        keys::CONFIGURATION,
        &attributes.configuration,
    );
    set_text(&mut properties, keys::DESCRIPTION, &attributes.description);
    // Unknown is what a schedule without a status reads as, and what the
    // report lists as missing: not recorded, so not there.
    if attributes.status != UtilityStatus::Unknown {
        properties.insert(
            String::from(keys::STATUS),
            text(sub::to_string(attributes.status)),
        );
    }
    set_fields(&mut properties, &attributes.fields, keys::FIELD_PREFIX);
    set_fields(&mut properties, &attributes.recorded, keys::RECORDED_PREFIX);
    properties
}

// Each distinct reason once, in the order the segments give them: a run of
// three segments all limited by one weak vertex says so once.
fn limited_by(segments: &[GradedSegment]) -> String {
    let mut reasons: Vec<&str> = Vec::new();
    for segment in segments {
        let reason = segment.limited_by.as_str();
        if !reason.is_empty() && !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    reasons.join("; ")
}

// A point carries its line's attributes (`shared`) and everything its row of
// the schedule said, so that the points alone are the schedule
// (utility_data.rs reads them back); then what the grading found there.
fn vertex_point(
    line: &UtilityLine,
    index: usize,
    graded: &GradedVertex,
    cover: &CoverResult,
    options: &UtilityDrawOptions,
    layer: &str,
    shared: &PropertyMap,
) -> Entity {
    let vertex = &line.vertices[index];
    let mut properties = shared.clone();
    set_text(&mut properties, keys::VERTEX, &vertex.id);
    properties.insert(
        String::from(keys::ORDER),
        PropertyValue::Integer((index + 1) as i64),
    );
    properties.insert(
        String::from(keys::METHOD),
        text(sub::to_string(vertex.evidence.method)),
    );
    set_real(&mut properties, keys::LEVEL, vertex.level);
    set_real(&mut properties, keys::DEPTH, vertex.depth);
    set_real(
        &mut properties,
        keys::HORIZONTAL_UNCERTAINTY,
        vertex.evidence.horizontal_uncertainty,
    );
    set_real(
        &mut properties,
        keys::VERTICAL_UNCERTAINTY,
        vertex.evidence.vertical_uncertainty,
    );
    if !line.path_evidence.is_empty() && index + 1 < line.vertices.len() {
        properties.insert(
            String::from(keys::PATH),
            text(sub::to_string(line.path_evidence[index])),
        );
    }
    // Wherever it bears on a level or a cover, and wherever it is not what an
    // unrecorded reference reads as.
    if sub::has_vertical_measurement(vertex) || vertex.level_reference != LevelReference::Top {
        properties.insert(
            String::from(keys::LEVEL_REFERENCE),
            text(sub::to_string(vertex.level_reference)),
        );
    }
    properties.insert(
        String::from(keys::QUALITY_LEVEL),
        text(sub::to_string(graded.classification.level)),
    );
    if let Some(claimed) = vertex.claimed {
        properties.insert(String::from(keys::CLAIMED), text(sub::to_string(claimed)));
    }
    set_text(&mut properties, keys::OVER_CLAIM, &graded.over_claim);
    set_real(
        &mut properties,
        keys::SERVICE_LEVEL,
        sub::service_level(vertex),
    );
    if sub::has_vertical_measurement(vertex) {
        properties.insert(
            String::from(keys::LEVEL_QUALIFIED),
            PropertyValue::Boolean(graded.classification.level_qualified),
        );
    }
    set_real(&mut properties, keys::SURFACE_LEVEL, vertex.surface_level);
    set_real(&mut properties, keys::COVER, cover.cover);
    set_text(&mut properties, keys::COVER_NOTE, &cover.note);
    if cover.cover.is_some() && options.minimum_cover.is_some() {
        properties.insert(
            String::from(keys::COVER_BELOW_MINIMUM),
            PropertyValue::Boolean(cover.below_minimum),
        );
    }
    set_text(&mut properties, keys::VERIFIES, &vertex.verifies);
    if options.grading.maximum_detected_spacing.is_finite() {
        properties.insert(
            String::from(keys::SPACING),
            PropertyValue::Real(options.grading.maximum_detected_spacing),
        );
    }
    set_real(&mut properties, keys::MINIMUM_COVER, options.minimum_cover);
    set_fields(&mut properties, &vertex.fields, keys::FIELD_PREFIX);
    set_fields(&mut properties, &vertex.recorded, keys::RECORDED_PREFIX);
    Entity {
        geometry: Geometry::Point(PointGeometry {
            position: plan_point(vertex),
        }),
        layer: String::from(layer),
        properties,
        ..Entity::default()
    }
}

// The layer `name` for a service of `type`, and every ancestor it lacks, into
// `layers` by name - a map, so parents sort before their children. The
// ancestors take the defaults but for the kind's own group,
// "<prefix>/<type>", which takes its colour, so the layer tree reads as the
// plan does.
//
// The name is checked here, as the model will check it: a prefix that is a
// layer path can still be too deep or too long once two levels are put under
// it, and a drawing with such a layer in it would be refused only when its
// command ran - after its linetypes and layers had been made and taken back,
// and naming the depth rather than the prefix that caused it.
fn need_layer(
    layers: &mut BTreeMap<String, Layer>,
    prefix: &str,
    r#type: UtilityType,
    name: &str,
    linetype: &str,
) -> Status {
    if let Err(why) = validate_layer_path(name) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the layer prefix leaves no room for the layers drawn under it: {name} - {}{}",
                why.message,
                if why.context.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", why.context)
                }
            ),
            prefix,
        ));
    }
    let group = join_layer_path(prefix, utility_type_word(r#type));
    for ancestor in layer_ancestors(name) {
        if let Entry::Vacant(slot) = layers.entry(ancestor) {
            let mut parent = Layer {
                name: slot.key().clone(),
                ..Layer::default()
            };
            if *slot.key() == group {
                parent.color = utility_type_colour(r#type);
            }
            slot.insert(parent);
        }
    }
    layers.insert(
        String::from(name),
        Layer {
            name: String::from(name),
            color: utility_type_colour(r#type),
            linetype: String::from(linetype),
            ..Layer::default()
        },
    );
    Ok(())
}

fn length_at(line: &DrawnUtilityLine, level: QualityLevel) -> f64 {
    line.length_at[level as usize]
}

/// The drawing of `lines`, graded with `options.grading` (the C++ default
/// options are UtilityDrawOptions::default()). All or nothing: InvalidArgument
/// naming the line when any line cannot be graded (fewer than two vertices, a
/// non-finite coordinate); naming the prefix when it is not a layer path, or
/// leaves a layer drawn under it too deep or too long for the model; and when
/// there are no lines at all. A run of no plan length to the model's tolerance
/// (two records at one place, or a rounding error apart, graded unlike their
/// neighbours) has no polyline to draw; its vertices are still drawn as
/// points and its segments still counted. So a drawing this returns holds
/// nothing utility_draw_command's model would refuse as geometry or as a layer
/// name.
pub fn draw_utilities(
    lines: &[UtilityLine],
    options: &UtilityDrawOptions,
) -> Result<UtilityDrawing> {
    if let Err(status) = validate_layer_path(&options.layer_prefix) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("the layer prefix is not a layer path: {}", status.message),
            options.layer_prefix.as_str(),
        ));
    }
    if lines.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the schedule has no lines to draw",
        ));
    }

    let mut drawing = UtilityDrawing::default();
    let mut layers: BTreeMap<String, Layer> = BTreeMap::new();
    let mut drawn_on: BTreeSet<String> = BTreeSet::new();
    let mut levels_used: BTreeSet<QualityLevel> = BTreeSet::new();
    for line in lines {
        // A line that cannot be graded refuses the whole draw, by its id: a
        // drawing missing one service would look complete.
        let refuse = |error: Error| {
            make_error(
                error.code,
                format!(
                    "line {} cannot be graded: {}; nothing was drawn",
                    line.id, error.message
                ),
                error.context,
            )
        };
        let graded = sub::grade_line(line, &options.grading).map_err(refuse)?;
        let cover =
            sub::depth_of_cover(line, options.minimum_cover, &options.grading).map_err(refuse)?;

        let r#type = line.attributes.r#type;
        let shared = line_properties(line);
        let mut drawn = DrawnUtilityLine {
            id: line.id.clone(),
            r#type,
            length_at: graded.length_at,
            length: graded.length(),
            polylines: 0,
        };

        // One polyline per maximal run of segments at one level.
        let segments = &graded.segments;
        let mut first = 0;
        while first < segments.len() {
            let mut last = first;
            let mut length = segments[first].length;
            while last + 1 < segments.len() && segments[last + 1].level == segments[first].level {
                last += 1;
                length += segments[last].length;
            }
            let level = segments[first].level;
            let vertices: Vec<Point2> = line.vertices
                [segments[first].from..=segments[last].from + 1]
                .iter()
                .map(plan_point)
                .collect();
            let geometry = Geometry::Polyline(Polyline2::new(vertices, false));
            // A run with no plan length to the model's tolerance - two records
            // at one place, or a rounding error apart - has nothing to draw.
            // Asked of the model's own check, so the drawing never holds an
            // entity its command would then refuse, refusing the whole draw.
            if validate(&geometry).is_ok() {
                let layer = quality_level_layer_name(&options.layer_prefix, r#type, level);
                need_layer(
                    &mut layers,
                    &options.layer_prefix,
                    r#type,
                    &layer,
                    quality_level_linetype_name(level),
                )?;
                drawn_on.insert(layer.clone());
                levels_used.insert(level);
                let mut properties = shared.clone();
                properties.insert(
                    String::from(keys::QUALITY_LEVEL),
                    text(sub::to_string(level)),
                );
                set_text(
                    &mut properties,
                    keys::LIMITED_BY,
                    &limited_by(&segments[first..=last]),
                );
                properties.insert(String::from(keys::LENGTH), PropertyValue::Real(length));
                set_text(
                    &mut properties,
                    keys::FROM,
                    &line.vertices[segments[first].from].id,
                );
                set_text(
                    &mut properties,
                    keys::TO,
                    &line.vertices[segments[last].from + 1].id,
                );
                drawing.entities.push(Entity {
                    geometry,
                    layer,
                    properties,
                    ..Entity::default()
                });
                drawn.polylines += 1;
            }
            first = last + 1;
        }

        let points_layer = points_layer_name(&options.layer_prefix, r#type);
        need_layer(
            &mut layers,
            &options.layer_prefix,
            r#type,
            &points_layer,
            CONTINUOUS_LINETYPE,
        )?;
        drawn_on.insert(points_layer.clone());
        for (i, vertex) in line.vertices.iter().enumerate() {
            drawing.entities.push(vertex_point(
                line,
                i,
                &graded.vertices[i],
                &cover[i],
                options,
                &points_layer,
                &shared,
            ));
            drawing.bounds.expand(plan_point(vertex));
        }
        drawing.vertices += line.vertices.len();
        drawing.segments += segments.len();
        drawing.lines.push(drawn);
    }

    drawing.layers = layers.into_values().collect();
    // Best evidence first, as the report lists the levels.
    for level in [QualityLevel::B, QualityLevel::C, QualityLevel::D] {
        if levels_used.contains(&level) {
            drawing.linetypes.push(quality_level_linetype(level));
        }
    }
    drawing.drawn_layers = drawn_on.len();
    Ok(drawing)
}

/// `drawing` added to `model` as ONE command named UTILITY_DRAW_STEP: the
/// linetypes the model lacks that a layer to be created names, the layers it
/// lacks (parents first, so undo takes every one back), then the entities. A
/// layer or linetype the model already has is reused as it is - its colour,
/// linetype and lock are the person's - so an existing locked layer refuses
/// the whole step when it runs.
pub fn utility_draw_command(model: &Model, drawing: &UtilityDrawing) -> CommandPtr {
    let mut transaction = Transaction::new(UTILITY_DRAW_STEP);
    // A linetype only for a layer that will be made: a layer that is reused
    // keeps its own, and a linetype nothing names is clutter.
    let named: BTreeSet<&str> = drawing
        .layers
        .iter()
        .filter(|layer| !model.layers.contains(&layer.name))
        .map(|layer| layer.linetype.as_str())
        .collect();
    for linetype in &drawing.linetypes {
        if named.contains(linetype.name.as_str()) && !model.linetypes.contains(&linetype.name) {
            transaction.add(create_linetype(linetype.clone()));
        }
    }
    // Parents first, each its own step, so that undo removes each: a layer
    // created with its ancestors missing would leave them behind on undo.
    for layer in &drawing.layers {
        if !model.layers.contains(&layer.name) {
            transaction.add(create_layer(layer.clone()));
        }
    }
    transaction.add(create_entities(drawing.entities.clone()));
    Box::new(transaction)
}

/// The reply UTILITY DRAW gives, one record a line:
///
/// ```text
/// utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=x0,y0,x1,y1
/// line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 ql_d=0.000
/// ```
///
/// Metres and coordinates to three decimals; an id with a blank, '"' or '='
/// in quotes. UTILITY REGRADE replies the same with `record` "utilities
/// regraded", so a front end frames what it drew the same way;
/// UTILITIES_DRAWN_RECORD is the C++ default.
pub fn format_utility_drawing(drawing: &UtilityDrawing, record: &str) -> String {
    let bounds = &drawing.bounds;
    let mut reply = format!(
        "{record} lines={} vertices={} segments={} entities={} layers={} bounds={},{},{},{}",
        drawing.lines.len(),
        drawing.vertices,
        drawing.segments,
        drawing.entities.len(),
        drawing.drawn_layers,
        c_fixed(bounds.min.x, 3),
        c_fixed(bounds.min.y, 3),
        c_fixed(bounds.max.x, 3),
        c_fixed(bounds.max.y, 3)
    );
    for line in &drawing.lines {
        reply += &format!(
            "\nline id={} type={} length={} ql_a={} ql_b={} ql_c={} ql_d={}",
            record_value(&line.id),
            utility_type_word(line.r#type),
            c_fixed(line.length, 3),
            c_fixed(length_at(line, QualityLevel::A), 3),
            c_fixed(length_at(line, QualityLevel::B), 3),
            c_fixed(length_at(line, QualityLevel::C), 3),
            c_fixed(length_at(line, QualityLevel::D), 3)
        );
    }
    reply
}
