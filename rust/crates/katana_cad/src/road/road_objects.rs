//! Road objects (include/katana/cad/road/road_objects.hpp,
//! src/katana_cad/road/road_objects.cpp; docs/road_objects.md): the furniture
//! and structures of a road placed along an alignment or a drawn line - safety
//! barriers, guide posts, signs, light poles, culverts, retaining and noise
//! walls, fences and line markings. Horizontal BIM: what stands beside and
//! crosses a road, never a building.
//!
//! An object is a DEFINITION - a kind, where it is placed, and the parameters
//! a person gave - kept in the drawing as versioned JSON under the metadata key
//! "road_objects" (Document::road_objects), edited by the ROADOBJECT verbs as
//! one undo step each. Its 3D objects (bim::bim_object) are MADE from it every
//! time, by build_road_object and bim::bim_object::placed_objects, from the
//! alignment and its design profile or a surface as they are now: move the
//! alignment and the barrier follows it.
//!
//! Every solid is closed and outward-oriented (road_object_mesh.rs), so the IFC
//! export writes it as a closed face set and its volume is exact for the mesh.
//! (The 3D view draws both faces of every triangle: the winding is for the
//! export and the volume, not the display.)
//!
//! Heights. An object stands on the ground of its placement: along an
//! alignment (Profile), the design profile's level at the station, carried out
//! to the object's offset at `crossfall` percent (negative falls away from the
//! centreline); on a surface (Surface), the named surface of the drawing under
//! each point, draped; on its line (Line), the heights of the line or points it
//! was placed with. What cannot be placed (a station off the alignment, an
//! alignment with no profile, a point off the surface, a vertex with no
//! height) is refused by the verb that places it, and once placed (the
//! alignment shortened since, the surface removed) the object is left out of
//! the model and the problem listed (BimModel::problems), never drawn at a
//! guessed level.
//!
//! The JSON form is road_object_json.rs's (the C++ road_object_json.cpp),
//! re-exported here as the header declares it.

use std::collections::BTreeMap;

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, lowered, parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::colour_names::standard_colour;
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::layer_path::validate_layer_path;
use katana_entity::{Alignment, Color, EntityId};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::mesh::mesh_volume;
use katana_geometry::profile::solve_profile;
use katana_geometry::{Point2, Point3, Polyline2, SolvedAlignment, SolvedProfile, Vec2, Vec3};
use katana_math::cpp;
use katana_math::numerics::PI;
use katana_terrain::TinSurface;

use crate::bim::bim_object::{BimModel, BimObject, BimProblem, Quantity};
use crate::document::Document;
use crate::road::road_object_mesh::{
    PathPoint, add_box, add_extrusion, add_hollow_sweep, add_prism, add_sweep, is_closed_mesh,
    regular_polygon,
};

pub use crate::road::road_object_json::{road_objects_from_json, road_objects_to_json};

// ---- kinds --------------------------------------------------------------------------------

/// Append-only: the JSON stores the word, never the number, but the dialog's
/// pages and the KINDS reply follow this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum RoadObjectKind {
    #[default]
    Barrier,
    GuidePost,
    Sign,
    LightPole,
    Culvert,
    Wall,
    Fence,
    Marking,
}

pub const ROAD_OBJECT_KIND_COUNT: usize = 8;

static KINDS: [RoadObjectKind; ROAD_OBJECT_KIND_COUNT] = [
    RoadObjectKind::Barrier,
    RoadObjectKind::GuidePost,
    RoadObjectKind::Sign,
    RoadObjectKind::LightPole,
    RoadObjectKind::Culvert,
    RoadObjectKind::Wall,
    RoadObjectKind::Fence,
    RoadObjectKind::Marking,
];

/// Every kind, in declaration order.
pub fn road_object_kinds() -> &'static [RoadObjectKind] {
    &KINDS
}

/// "barrier", "guidepost", "sign", "light", "culvert", "wall", "fence",
/// "marking": the word ROADOBJECT ADD takes, the JSON stores and
/// BimObject::kind says.
pub fn kind_word(kind: RoadObjectKind) -> &'static str {
    match kind {
        RoadObjectKind::Barrier => "barrier",
        RoadObjectKind::GuidePost => "guidepost",
        RoadObjectKind::Sign => "sign",
        RoadObjectKind::LightPole => "light",
        RoadObjectKind::Culvert => "culvert",
        RoadObjectKind::Wall => "wall",
        RoadObjectKind::Fence => "fence",
        RoadObjectKind::Marking => "marking",
    }
}

// The C++ `lowered(trimmed(text))`: ASCII blanks off both ends, ASCII letters
// folded. Both keep a UTF-8 text UTF-8.
fn folded(text: &str) -> String {
    String::from_utf8_lossy(&lowered(trimmed(text.as_bytes()))).into_owned()
}

/// The word in any case, or its plural ("barriers", "guideposts", "lights",
/// "markings" ...). NotFound naming the words there are.
pub fn kind_from_word(word: &str) -> Result<RoadObjectKind> {
    let folded = folded(word);
    for &kind in road_object_kinds() {
        let name = kind_word(kind);
        if folded == name || folded == format!("{name}s") {
            return Ok(kind);
        }
    }
    // The words the menu says, run together.
    if folded == "lightpole" || folded == "lightpoles" {
        return Ok(RoadObjectKind::LightPole);
    }
    if folded == "linemarking" || folded == "linemarkings" {
        return Ok(RoadObjectKind::Marking);
    }
    Err(make_error(
        ErrorCode::NotFound,
        "not a road object kind; the kinds are barrier, guidepost, sign, light, culvert, wall, \
         fence and marking",
        word,
    ))
}

/// "Safety barrier", "Guide posts", "Sign", "Light poles", "Culvert", "Wall",
/// "Fence", "Line marking": what a person reads.
pub fn kind_title(kind: RoadObjectKind) -> &'static str {
    match kind {
        RoadObjectKind::Barrier => "Safety barrier",
        RoadObjectKind::GuidePost => "Guide posts",
        RoadObjectKind::Sign => "Sign",
        RoadObjectKind::LightPole => "Light poles",
        RoadObjectKind::Culvert => "Culvert",
        RoadObjectKind::Wall => "Wall",
        RoadObjectKind::Fence => "Fence",
        RoadObjectKind::Marking => "Line marking",
    }
}

/// How a kind takes its placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KindShape {
    /// Runs along it: a barrier, a wall, a fence, a line marking.
    Linear,
    /// Stands at points of it: one at a station, one at each point, or one
    /// every `spacing` along it - guide posts, signs, light poles.
    Point,
    /// Crosses it: a culvert, across the alignment at a station, or from the
    /// first point to the last.
    Crossing,
}

pub fn shape_of(kind: RoadObjectKind) -> KindShape {
    match kind {
        RoadObjectKind::Barrier
        | RoadObjectKind::Wall
        | RoadObjectKind::Fence
        | RoadObjectKind::Marking => KindShape::Linear,
        RoadObjectKind::GuidePost | RoadObjectKind::Sign | RoadObjectKind::LightPole => {
            KindShape::Point
        }
        RoadObjectKind::Culvert => KindShape::Crossing,
    }
}

// ---- parameters ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ParameterType {
    #[default]
    Number,
    Choice,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterSpec {
    pub key: &'static str,
    pub r#type: ParameterType,
    /// "m", "deg", "%", "" for a count or a word.
    pub unit: &'static str,
    /// What it is when not given, as a person reads it: "0.787", "wbeam", "by
    /// type" (it follows another parameter), "auto" (worked out from the
    /// placement).
    pub default_text: &'static str,
    /// A Choice's words, lower case.
    pub choices: &'static [&'static str],
    /// A Number's range, inclusive.
    pub minimum: f64,
    pub maximum: f64,
    pub description: &'static str,
    /// A Number that is a count: a whole number.
    pub whole: bool,
}

/// A parameter's value: a number, or a word or a text.
#[derive(Clone, Debug, PartialEq)]
pub enum ParameterValue {
    Number(f64),
    Text(String),
}

pub type Parameters = BTreeMap<String, ParameterValue>;

// ---- what the kinds are made of: published dimensions, each named ---------------------------

// The W-beam and thrie-beam rails of AASHTO M 180: a W-beam 311 mm (12 1/4 in)
// and a thrie-beam 508 mm (20 in) from edge to edge, both 83 mm (3 1/4 in)
// deep. Drawn as a plain box of that size, the corrugation left out.
const W_BEAM_RAIL_HEIGHT: f64 = 0.311;
const THRIE_BEAM_RAIL_HEIGHT: f64 = 0.508;
const BEAM_DEPTH: f64 = 0.083;
// The steel post a beam barrier stands on: a W6x9 (W150x13.5) section, 150 mm
// deep across the barrier and 100 mm wide along it (AISC shape tables,
// rounded).
const POST_DEPTH: f64 = 0.150;
const POST_FLANGE: f64 = 0.100;
// The F-shape concrete barrier (AASHTO Roadside Design Guide, FHWA): a 76 mm
// (3 in) vertical reveal at the foot, the face at 55 degrees from the
// horizontal up to 254 mm (10 in), then at 84 degrees to the top.
const F_SHAPE_REVEAL: f64 = 0.076;
const F_SHAPE_BREAK: f64 = 0.254;
const F_SHAPE_LOWER_DEGREES: f64 = 55.0;
const F_SHAPE_UPPER_DEGREES: f64 = 84.0;
// Wire rope: 19 mm ropes, 100 mm apart below the top one, on 100 mm posts
// standing 50 mm above it - typical of the proprietary systems, whose own
// drawings govern.
const ROPE_DIAMETER: f64 = 0.019;
const ROPE_SPACING: f64 = 0.10;
const ROPE_POST: f64 = 0.10;
const ROPE_POST_ABOVE: f64 = 0.05;
// A fence's wires are 2.5 to 4 mm; drawn 20 mm so they show at the distance a
// road is looked at from. Their volume is not a quantity.
const FENCE_STRAND: f64 = 0.02;
// A light pole's arm, 80 mm square, and its luminaire, 600 x 300 x 150 mm:
// typical of a road lantern.
const ARM_SECTION: f64 = 0.08;
const LUMINAIRE_LENGTH: f64 = 0.6;
const LUMINAIRE_WIDTH: f64 = 0.3;
const LUMINAIRE_HEIGHT: f64 = 0.15;
// A culvert's headwall: 250 mm thick, standing 300 mm beyond the barrels at
// each side and above them - typical of a precast headwall.
const HEADWALL_THICKNESS: f64 = 0.25;
const HEADWALL_MARGIN: f64 = 0.3;
// A pipe drawn with 24 facets: a facet of a 900 mm pipe is at most
// 0.45 x (1 - cos 7.5 deg) = 3.9 mm inside the true circle.
const PIPE_SIDES: i32 = 24;
// A pole or a round sign with 8 and 24 sides.
const POLE_SIDES: i32 = 8;
const ROUND_SIGN_SIDES: i32 = 24;

// An alignment's objects are sampled every metre between its key stations, so
// a section swept along a curve of radius 50 m is within 1^2 / (8 x 50) =
// 2.5 mm of the true offset curve; a line draped on a surface is sampled as
// often, so it follows the ground.
const PATH_STEP: f64 = 1.0;
// A station this close to an end of the alignment is that end: a station typed
// as the end the reply gave must not fall off it by a rounding.
const STATION_TOLERANCE: f64 = 1e-6;
// Two points of a placed line this close in plan are one: a micrometre, below
// anything set out and above the noise of a coordinate typed twice.
const SAME_POSITION: f64 = 1e-6;
// The solids a mark of a broken line is made of are swept with the same
// millimetre a sweep passes points closer than over (road_object_mesh.rs's
// SAME_POINT): a dash shorter than it has no solid.
const SHORTEST_MARK: f64 = 1e-3;
// The most posts, signs, poles or marks one definition makes. Each is a solid,
// the model is made again after every command, and a spacing typed as 0.001
// for 0.1 would otherwise ask for a hundred times the memory of a run that was
// merely long: a million posts took 5.7 s and 2.3 GB to make. Ten thousand is
// 19 km of barrier at its 1.905 m posts or 120 km of centre line; a longer run
// is placed in lengths.
const MAX_PLACED_PARTS: f64 = 10000.0;

// ---- the parameter tables -----------------------------------------------------------------

static BARRIER_TYPES: [&str; 5] = ["wbeam", "thriebeam", "fshape", "singleslope", "wirerope"];
static SIDES: [&str; 3] = ["auto", "left", "right"];
static SIGN_SHAPES: [&str; 5] = ["rectangle", "diamond", "circle", "octagon", "triangle"];
static CULVERT_SHAPES: [&str; 2] = ["circular", "box"];
static ON_OFF: [&str; 2] = ["on", "off"];
static WALL_TYPES: [&str; 2] = ["retaining", "noise"];
static MARKING_TYPES: [&str; 5] = ["edge", "centre", "lane", "continuity", "solid"];

// A facing, an invert: a level anywhere a project could be. Ten kilometres
// either way of the datum holds every road on Earth.
const ANY_LEVEL: f64 = 10000.0;

#[allow(
    clippy::too_many_arguments,
    reason = "one row of the C++ parameter tables, field for field"
)]
const fn spec(
    key: &'static str,
    r#type: ParameterType,
    unit: &'static str,
    default_text: &'static str,
    choices: &'static [&'static str],
    minimum: f64,
    maximum: f64,
    description: &'static str,
    whole: bool,
) -> ParameterSpec {
    ParameterSpec {
        key,
        r#type,
        unit,
        default_text,
        choices,
        minimum,
        maximum,
        description,
        whole,
    }
}

use ParameterType::{Choice, Number, Text};

const NONE: &[&str] = &[];

static BARRIER_PARAMETERS: [ParameterSpec; 7] = [
    spec(
        "type",
        Choice,
        "",
        "wbeam",
        &BARRIER_TYPES,
        0.0,
        0.0,
        "wbeam, thriebeam (steel beams on posts), fshape, singleslope (concrete), wirerope",
        false,
    ),
    spec(
        "height",
        Number,
        "m",
        "by type",
        NONE,
        0.3,
        2.0,
        "top of the barrier above the ground",
        false,
    ),
    spec(
        "postspacing",
        Number,
        "m",
        "by type",
        NONE,
        0.5,
        10.0,
        "between the posts of a beam or wire rope barrier",
        false,
    ),
    spec(
        "width",
        Number,
        "m",
        "by type",
        NONE,
        0.2,
        1.5,
        "base width of a concrete barrier",
        false,
    ),
    spec(
        "topwidth",
        Number,
        "m",
        "by type",
        NONE,
        0.05,
        1.0,
        "top width of a single slope barrier",
        false,
    ),
    spec(
        "ropes",
        Number,
        "",
        "by type",
        NONE,
        2.0,
        6.0,
        "a wire rope barrier's ropes",
        true,
    ),
    spec(
        "side",
        Choice,
        "",
        "auto",
        &SIDES,
        0.0,
        0.0,
        "the side of its line the traffic is on, looking along it; auto: towards the centreline",
        false,
    ),
];

static GUIDE_POST_PARAMETERS: [ParameterSpec; 2] = [
    spec(
        "height",
        Number,
        "m",
        "1.0",
        NONE,
        0.3,
        3.0,
        "above the ground",
        false,
    ),
    spec(
        "size",
        Number,
        "m",
        "0.1",
        NONE,
        0.02,
        0.5,
        "the post's square section",
        false,
    ),
];

static SIGN_PARAMETERS: [ParameterSpec; 10] = [
    spec(
        "shape",
        Choice,
        "",
        "rectangle",
        &SIGN_SHAPES,
        0.0,
        0.0,
        "the plate's outline",
        false,
    ),
    spec(
        "width",
        Number,
        "m",
        "0.9",
        NONE,
        0.1,
        10.0,
        "the plate's width; a diamond's diagonal, a circle's or an octagon's across",
        false,
    ),
    spec(
        "plateheight",
        Number,
        "m",
        "0.6",
        NONE,
        0.1,
        10.0,
        "a rectangle's height; the other shapes are as tall as their outline",
        false,
    ),
    spec(
        "clearance",
        Number,
        "m",
        "1.5",
        NONE,
        0.0,
        10.0,
        "from the ground to the plate's lower edge",
        false,
    ),
    spec(
        "posts",
        Number,
        "",
        "1",
        NONE,
        1.0,
        2.0,
        "one post, or two",
        true,
    ),
    spec(
        "postsize",
        Number,
        "m",
        "0.075",
        NONE,
        0.02,
        0.5,
        "a post's square section",
        false,
    ),
    spec(
        "thickness",
        Number,
        "m",
        "0.003",
        NONE,
        0.001,
        0.1,
        "the plate's",
        false,
    ),
    spec(
        "facing",
        Number,
        "deg",
        "auto",
        NONE,
        -360.0,
        360.0,
        "the way the face looks, counter-clockwise from east; auto: towards the traffic",
        false,
    ),
    spec(
        "code",
        Text,
        "",
        "",
        NONE,
        0.0,
        0.0,
        "the sign's code in its standard",
        false,
    ),
    spec("text", Text, "", "", NONE, 0.0, 0.0, "its legend", false),
];

static LIGHT_PARAMETERS: [ParameterSpec; 4] = [
    spec(
        "height",
        Number,
        "m",
        "10",
        NONE,
        2.0,
        40.0,
        "mounting height above the ground",
        false,
    ),
    spec(
        "outreach",
        Number,
        "m",
        "1.5",
        NONE,
        0.0,
        10.0,
        "the arm, from the pole to the lantern",
        false,
    ),
    spec(
        "polesize",
        Number,
        "m",
        "0.2",
        NONE,
        0.05,
        1.0,
        "the pole across its flats",
        false,
    ),
    spec(
        "facing",
        Number,
        "deg",
        "auto",
        NONE,
        -360.0,
        360.0,
        "the way the arm points, counter-clockwise from east; auto: towards the centreline",
        false,
    ),
];

static CULVERT_PARAMETERS: [ParameterSpec; 13] = [
    spec(
        "shape",
        Choice,
        "",
        "circular",
        &CULVERT_SHAPES,
        0.0,
        0.0,
        "a pipe, or a box",
        false,
    ),
    spec(
        "cells",
        Number,
        "",
        "1",
        NONE,
        1.0,
        20.0,
        "barrels side by side",
        true,
    ),
    spec(
        "diameter",
        Number,
        "m",
        "0.9",
        NONE,
        0.1,
        5.0,
        "a pipe's inside diameter",
        false,
    ),
    spec(
        "span",
        Number,
        "m",
        "1.2",
        NONE,
        0.3,
        10.0,
        "a box's inside width",
        false,
    ),
    spec(
        "rise",
        Number,
        "m",
        "0.9",
        NONE,
        0.3,
        10.0,
        "a box's inside height",
        false,
    ),
    spec(
        "wall",
        Number,
        "m",
        "by type",
        NONE,
        0.02,
        1.0,
        "the barrel's wall",
        false,
    ),
    spec(
        "cellgap",
        Number,
        "m",
        "by type",
        NONE,
        0.0,
        5.0,
        "between the barrels",
        false,
    ),
    spec(
        "length",
        Number,
        "m",
        "20",
        NONE,
        1.0,
        500.0,
        "across an alignment, centred on the station; along a line or between points, the line's",
        false,
    ),
    spec(
        "skew",
        Number,
        "deg",
        "0",
        NONE,
        -80.0,
        80.0,
        "from square to the alignment, counter-clockwise",
        false,
    ),
    spec(
        "invert",
        Number,
        "m",
        "auto",
        NONE,
        -ANY_LEVEL,
        ANY_LEVEL,
        "the inlet's invert level; auto: from the cover",
        false,
    ),
    spec(
        "invert2",
        Number,
        "m",
        "auto",
        NONE,
        -ANY_LEVEL,
        ANY_LEVEL,
        "the outlet's; auto: the inlet's when invert is given, else from the cover",
        false,
    ),
    spec(
        "cover",
        Number,
        "m",
        "0.6",
        NONE,
        0.0,
        50.0,
        "from the ground to the barrel's top",
        false,
    ),
    spec(
        "headwalls",
        Choice,
        "",
        "on",
        &ON_OFF,
        0.0,
        0.0,
        "a headwall at each end",
        false,
    ),
];

static WALL_PARAMETERS: [ParameterSpec; 5] = [
    spec(
        "type",
        Choice,
        "",
        "retaining",
        &WALL_TYPES,
        0.0,
        0.0,
        "retaining, or noise",
        false,
    ),
    spec(
        "height",
        Number,
        "m",
        "by type",
        NONE,
        0.2,
        20.0,
        "above the ground",
        false,
    ),
    spec(
        "thickness",
        Number,
        "m",
        "by type",
        NONE,
        0.05,
        3.0,
        "the stem's",
        false,
    ),
    spec(
        "footing",
        Number,
        "m",
        "by type",
        NONE,
        0.0,
        10.0,
        "the strip footing's width; 0: none",
        false,
    ),
    spec(
        "footingdepth",
        Number,
        "m",
        "by type",
        NONE,
        0.1,
        5.0,
        "below the ground",
        false,
    ),
];

static FENCE_PARAMETERS: [ParameterSpec; 4] = [
    spec(
        "height",
        Number,
        "m",
        "1.2",
        NONE,
        0.3,
        5.0,
        "above the ground",
        false,
    ),
    spec(
        "postspacing",
        Number,
        "m",
        "3",
        NONE,
        0.5,
        20.0,
        "between the posts",
        false,
    ),
    spec(
        "postsize",
        Number,
        "m",
        "0.1",
        NONE,
        0.02,
        0.5,
        "a post's square section",
        false,
    ),
    spec(
        "rails",
        Number,
        "",
        "3",
        NONE,
        1.0,
        10.0,
        "wires or rails",
        true,
    ),
];

static MARKING_PARAMETERS: [ParameterSpec; 5] = [
    spec(
        "type",
        Choice,
        "",
        "edge",
        &MARKING_TYPES,
        0.0,
        0.0,
        "edge, centre (dividing), lane, continuity, solid",
        false,
    ),
    spec(
        "width",
        Number,
        "m",
        "by type",
        NONE,
        0.05,
        1.0,
        "the line's",
        false,
    ),
    spec(
        "dash",
        Number,
        "m",
        "by type",
        NONE,
        0.0,
        50.0,
        "a mark's length; 0: continuous",
        false,
    ),
    spec(
        "gap",
        Number,
        "m",
        "by type",
        NONE,
        0.0,
        50.0,
        "between marks",
        false,
    ),
    spec(
        "thickness",
        Number,
        "m",
        "0.003",
        NONE,
        0.0005,
        0.05,
        "the material's",
        false,
    ),
];

// The defaults that follow a type= (ParameterSpec::default_text "by type").
struct TypeDefault {
    kind: RoadObjectKind,
    r#type: &'static str,
    key: &'static str,
    value: f64,
}

const fn by_type(
    kind: RoadObjectKind,
    r#type: &'static str,
    key: &'static str,
    value: f64,
) -> TypeDefault {
    TypeDefault {
        kind,
        r#type,
        key,
        value,
    }
}

static TYPE_DEFAULTS: [TypeDefault; 39] = [
    // Tops of rail: 787 mm (31 in) for the MASH-tested Midwest Guardrail
    // System's W-beam; 864 mm (34 in) for thrie-beam. Posts at 1.905 m (6 ft 3
    // in), the standard beam barrier spacing.
    by_type(RoadObjectKind::Barrier, "wbeam", "height", 0.787),
    by_type(RoadObjectKind::Barrier, "wbeam", "postspacing", 1.905),
    by_type(RoadObjectKind::Barrier, "thriebeam", "height", 0.864),
    by_type(RoadObjectKind::Barrier, "thriebeam", "postspacing", 1.905),
    // The F-shape: 813 mm (32 in) on a 610 mm (24 in) base.
    by_type(RoadObjectKind::Barrier, "fshape", "height", 0.813),
    by_type(RoadObjectKind::Barrier, "fshape", "width", 0.610),
    // The single slope: 1067 mm (42 in), a 610 mm base and a 203 mm (8 in)
    // top - its faces 10.8 degrees from the vertical.
    by_type(RoadObjectKind::Barrier, "singleslope", "height", 1.067),
    by_type(RoadObjectKind::Barrier, "singleslope", "width", 0.610),
    by_type(RoadObjectKind::Barrier, "singleslope", "topwidth", 0.203),
    by_type(RoadObjectKind::Barrier, "wirerope", "height", 0.80),
    by_type(RoadObjectKind::Barrier, "wirerope", "postspacing", 3.0),
    by_type(RoadObjectKind::Barrier, "wirerope", "ropes", 4.0),
    // A pipe's wall: 100 mm, a typical reinforced concrete pipe of 900 mm; a
    // box's 150 mm. Pipes 300 mm apart; boxes side by side.
    by_type(RoadObjectKind::Culvert, "circular", "wall", 0.10),
    by_type(RoadObjectKind::Culvert, "circular", "cellgap", 0.3),
    by_type(RoadObjectKind::Culvert, "box", "wall", 0.15),
    by_type(RoadObjectKind::Culvert, "box", "cellgap", 0.0),
    by_type(RoadObjectKind::Wall, "retaining", "height", 2.0),
    by_type(RoadObjectKind::Wall, "retaining", "thickness", 0.25),
    by_type(RoadObjectKind::Wall, "retaining", "footing", 1.2),
    by_type(RoadObjectKind::Wall, "retaining", "footingdepth", 0.4),
    by_type(RoadObjectKind::Wall, "noise", "height", 4.0),
    by_type(RoadObjectKind::Wall, "noise", "thickness", 0.2),
    by_type(RoadObjectKind::Wall, "noise", "footing", 0.0),
    by_type(RoadObjectKind::Wall, "noise", "footingdepth", 0.4),
    // AS 1742.2's longitudinal lines: an edge line 150 mm, a dividing or lane
    // line 100 mm broken 3 m line and 9 m gap, a continuity line 1 m and 3 m.
    by_type(RoadObjectKind::Marking, "edge", "width", 0.15),
    by_type(RoadObjectKind::Marking, "edge", "dash", 0.0),
    by_type(RoadObjectKind::Marking, "edge", "gap", 0.0),
    by_type(RoadObjectKind::Marking, "centre", "width", 0.10),
    by_type(RoadObjectKind::Marking, "centre", "dash", 3.0),
    by_type(RoadObjectKind::Marking, "centre", "gap", 9.0),
    by_type(RoadObjectKind::Marking, "lane", "width", 0.10),
    by_type(RoadObjectKind::Marking, "lane", "dash", 3.0),
    by_type(RoadObjectKind::Marking, "lane", "gap", 9.0),
    by_type(RoadObjectKind::Marking, "continuity", "width", 0.15),
    by_type(RoadObjectKind::Marking, "continuity", "dash", 1.0),
    by_type(RoadObjectKind::Marking, "continuity", "gap", 3.0),
    by_type(RoadObjectKind::Marking, "solid", "width", 0.10),
    by_type(RoadObjectKind::Marking, "solid", "dash", 0.0),
    by_type(RoadObjectKind::Marking, "solid", "gap", 0.0),
];

// The parameter that is the type, for the kinds that have one.
fn type_key(kind: RoadObjectKind) -> &'static str {
    match kind {
        RoadObjectKind::Barrier | RoadObjectKind::Wall | RoadObjectKind::Marking => "type",
        RoadObjectKind::Culvert | RoadObjectKind::Sign => "shape",
        RoadObjectKind::GuidePost | RoadObjectKind::LightPole | RoadObjectKind::Fence => "",
    }
}

fn text_of<'a>(parameters: &'a Parameters, key: &str) -> &'a str {
    match parameters.get(key) {
        Some(ParameterValue::Text(text)) => text,
        _ => "",
    }
}

// The value a parameter not given takes, for the object's type; None for an
// "auto" one, a text, or a "by type" one this type has no use for.
fn default_of(
    kind: RoadObjectKind,
    kind_type: &str,
    spec: &ParameterSpec,
) -> Option<ParameterValue> {
    if spec.r#type == ParameterType::Choice {
        return Some(ParameterValue::Text(String::from(spec.default_text)));
    }
    if spec.r#type == ParameterType::Text {
        return None;
    }
    if spec.default_text == "by type" || spec.default_text == "auto" || spec.default_text.is_empty()
    {
        return TYPE_DEFAULTS
            .iter()
            .find(|row| row.kind == kind && row.r#type == kind_type && row.key == spec.key)
            .map(|row| ParameterValue::Number(row.value));
    }
    parse_finite_double(spec.default_text).map(ParameterValue::Number)
}

// ---- formatting -----------------------------------------------------------------------------

fn real(value: f64) -> String {
    format_exact_real(value)
}

fn point_text(point: Point3) -> String {
    format!("{},{},{}", real(point.x), real(point.y), real(point.z))
}

fn radians(degrees: f64) -> f64 {
    degrees * PI / 180.0
}

fn unit_at(radians_value: f64) -> Vec2 {
    Vec2::new(radians_value.cos(), radians_value.sin())
}

// ---- making: what cannot be made is answered here and listed at the edge ---------------------

// What the C++ throws from deep inside the making and catches at its edge: a
// sentence of why the object could not be made.
struct Unplaceable {
    message: String,
}

type Made<T> = std::result::Result<T, Unplaceable>;

fn fail<T>(message: impl Into<String>) -> Made<T> {
    Err(Unplaceable {
        message: message.into(),
    })
}

// A parameter's value as made: given, else the default (effective_parameters).
struct Values<'a> {
    object: &'a RoadObject,
    values: Parameters,
}

impl<'a> Values<'a> {
    fn new(object: &'a RoadObject) -> Self {
        Self {
            object,
            values: effective_parameters(object),
        }
    }

    fn number(&self, key: &str) -> Made<f64> {
        if let Some(ParameterValue::Number(value)) = self.values.get(key) {
            return Ok(*value);
        }
        fail(format!("{} has no value for {key}", self.object.id))
    }

    fn optional_number(&self, key: &str) -> Option<f64> {
        match self.values.get(key) {
            Some(ParameterValue::Number(value)) => Some(*value),
            _ => None,
        }
    }

    fn text(&self, key: &str) -> String {
        String::from(text_of(&self.values, key))
    }

    fn all(&self) -> &Parameters {
        &self.values
    }
}

// What a path gives at a distance along it: the point, its level interpolated
// by length, and the direction of the segment it is on.
#[derive(Clone, Copy)]
struct Sample {
    position: Point2,
    z: f64,
    direction: Vec2,
}

// A path in plan with a level at each point, and how far along each point is.
struct Path {
    points: Vec<PathPoint>,
    along: Vec<f64>,
}

impl Path {
    fn new(points: Vec<PathPoint>) -> Self {
        let mut kept: Vec<PathPoint> = Vec::new();
        let mut along: Vec<f64> = Vec::new();
        for point in points {
            if let Some(last) = kept.last()
                && last.position.distance_to(point.position) < SAME_POSITION
            {
                continue;
            }
            let step = kept
                .last()
                .map_or(0.0, |last| last.position.distance_to(point.position));
            along.push(along.last().map_or(0.0, |before| before + step));
            kept.push(point);
        }
        Self {
            points: kept,
            along,
        }
    }

    fn points(&self) -> &[PathPoint] {
        &self.points
    }

    fn length(&self) -> f64 {
        self.along.last().copied().unwrap_or(0.0)
    }

    fn size(&self) -> usize {
        self.points.len()
    }

    // The point `distance` along, its level interpolated by length and the
    // direction of the segment it is on.
    fn at(&self, distance: f64) -> Sample {
        let segment = self.segment_at(distance);
        let a = self.points[segment];
        let b = self.points[segment + 1];
        let span = self.along[segment + 1] - self.along[segment];
        let t = if span > 0.0 {
            cpp::clamp((distance - self.along[segment]) / span, 0.0, 1.0)
        } else {
            0.0
        };
        Sample {
            position: a.position + (b.position - a.position) * t,
            z: a.z + (b.z - a.z) * t,
            direction: (b.position - a.position).normalized(),
        }
    }

    // The part between `from` and `to`, its ends interpolated.
    fn between(&self, from: f64, to: f64) -> Vec<PathPoint> {
        let mut out = Vec::new();
        let start = self.at(from);
        out.push(PathPoint::new(start.position, start.z));
        for (point, &along) in self.points.iter().zip(&self.along) {
            if along > from && along < to {
                out.push(*point);
            }
        }
        let end = self.at(to);
        out.push(PathPoint::new(end.position, end.z));
        out
    }

    fn plan(&self) -> Polyline2 {
        Polyline2::new(
            self.points.iter().map(|point| point.position).collect(),
            false,
        )
    }

    fn segment_at(&self, distance: f64) -> usize {
        // std::upper_bound: the first point farther along than `distance`.
        let upper = self.along.partition_point(|&along| !(distance < along));
        upper.saturating_sub(1).min(self.points.len() - 2)
    }
}

// How many whole spacings fit in `length`, refused when a definition would
// make more than MAX_PLACED_PARTS of something at that spacing: the check
// comes before anything is allocated, so an absurd spacing is a refusal naming
// it and not an out-of-memory.
fn whole_spacings(length: f64, spacing: f64, of: &str) -> Made<usize> {
    let whole = (length / spacing + 1e-9).floor();
    if !(whole + 1.0 <= MAX_PLACED_PARTS) {
        return fail(format!(
            "{of} every {} m along {} m would make more than the {} a definition takes; place the \
             run in lengths, or give a larger spacing",
            real(spacing),
            real(length),
            MAX_PLACED_PARTS as usize
        ));
    }
    Ok(whole as usize)
}

// Distances from 0 to `length` every `spacing`, the end always among them: the
// posts of a barrier or a fence, the end post where the spacing does not come
// out even.
fn post_distances(length: f64, spacing: f64) -> Made<Vec<f64>> {
    let whole = whole_spacings(length, spacing, "posts")?;
    let mut out: Vec<f64> = (0..=whole)
        .map(|k| cpp::min(k as f64 * spacing, length))
        .collect();
    if length - out[out.len() - 1] > SAME_POSITION {
        out.push(length);
    }
    Ok(out)
}

// Distances from 0 every `spacing` up to `length`, and no more: a run of guide
// posts or light poles is not given an extra one at its end, as a barrier is
// given an end post.
fn every_spacing(length: f64, spacing: f64) -> Made<Vec<f64>> {
    let whole = whole_spacings(length, spacing, "objects")?;
    Ok((0..=whole).map(|k| k as f64 * spacing).collect())
}

// True when the path comes back to where it started: a closed polyline, or a
// line drawn out and back. Whatever stands at its end then stands on what
// stands at its start.
fn returns_to_start(path: &Path) -> bool {
    let points = path.points();
    points.len() > 1
        && points[0]
            .position
            .distance_to(points[points.len() - 1].position)
            < SAME_POSITION
}

// The posts of a barrier or a fence along `path`: post_distances, less the end
// post of a path that comes back to its start, which would stand where the
// first one does.
fn posts_along(path: &Path, spacing: f64) -> Made<Vec<f64>> {
    let mut out = post_distances(path.length(), spacing)?;
    if out.len() > 1 && returns_to_start(path) {
        out.pop();
    }
    Ok(out)
}

// The vertices of a placed line that stand for a corner of it, by index. A
// vertex on the one before it is that corner again - a node edited twice
// leaves one - and has no direction of its own; the closing vertex of a line
// that comes back to its start is its first corner again, and a point object
// stood on it would be the first one's twin. Both would make an empty solid
// (no direction to stand it) or a doubled count (docs/road_objects.md,
// "Placing").
fn corners_of(vertices: &[PlacedVertex]) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for (i, vertex) in vertices.iter().enumerate() {
        if let Some(&last) = kept.last()
            && vertices[last].position.distance_to(vertex.position) < SAME_POSITION
        {
            continue;
        }
        kept.push(i);
    }
    if kept.len() > 2
        && vertices[kept[0]]
            .position
            .distance_to(vertices[kept[kept.len() - 1]].position)
            < SAME_POSITION
    {
        kept.pop();
    }
    kept
}

// One object stood on its ground: what a placement resolves to before any
// solid is made.
#[derive(Clone, Copy)]
struct Spot {
    position: Point2,
    z: f64,
    // Along the placement: the alignment's direction, the line's.
    forward: Vec2,
    // The offset from the centreline, along an alignment.
    offset: f64,
}

struct Placement<'a> {
    object: &'a RoadObject,
    source: HeightSource,
    alignment: Option<SolvedAlignment>,
    profile: Option<SolvedProfile>,
    surface: Option<&'a TinSurface>,
}

impl<'a> Placement<'a> {
    fn new<'c: 'a>(context: &PlacementContext<'c>, object: &'a RoadObject) -> Made<Self> {
        let placement = &object.placement;
        let source = height_source_of(placement);
        let mut made = Placement {
            object,
            source,
            alignment: None,
            profile: None,
            surface: None,
        };
        if placement.kind == PlacementKind::Alignment {
            let found = context
                .alignment
                .as_ref()
                .and_then(|lookup| lookup(placement.alignment.as_str()));
            let Some(found) = found else {
                return fail(format!("alignment {} does not exist", placement.alignment));
            };
            match solve_alignment(&found.horizontal) {
                Ok(solved) => made.alignment = Some(solved),
                Err(error) => {
                    return fail(format!(
                        "alignment {} does not solve: {}",
                        placement.alignment,
                        error.describe()
                    ));
                }
            }
            if source == HeightSource::Line {
                return fail(
                    "heights=line takes the heights of a line or points; along an alignment an \
                     object stands on its profile or a surface",
                );
            }
            if source == HeightSource::Profile {
                let Some(vertical) = &found.vertical else {
                    return fail(format!(
                        "alignment {} has no design profile: give it one, or give \
                         surface=<name> to stand the object on a surface",
                        placement.alignment
                    ));
                };
                match solve_profile(vertical) {
                    Ok(profile) => made.profile = Some(profile),
                    Err(error) => {
                        return fail(format!(
                            "the profile of {} does not solve: {}",
                            placement.alignment,
                            error.describe()
                        ));
                    }
                }
            }
        } else {
            if source == HeightSource::Profile {
                return fail(
                    "heights=profile needs an alignment: give ALIGNMENT <name>, or the heights of \
                     the line or a surface",
                );
            }
            if placement.vertices.is_empty() {
                return fail("the placement has no points");
            }
        }
        if source == HeightSource::Surface {
            if placement.surface.is_empty() {
                return fail("heights=surface needs surface=<name>");
            }
            made.surface = context
                .surface
                .as_ref()
                .and_then(|lookup| lookup(placement.surface.as_str()));
            if made.surface.is_none() {
                return fail(format!("surface {} does not exist", placement.surface));
            }
        }
        // Only the profile is carried out to an offset; a crossfall given to an
        // object that stands on anything else would be echoed and ignored.
        if placement.crossfall != 0.0 && source != HeightSource::Profile {
            return fail(format!(
                "crossfall= carries the design profile out to the offset, and this object stands \
                 on {}; leave crossfall= out, or stand it on the profile",
                if source == HeightSource::Surface {
                    "a surface"
                } else {
                    "the heights of its line or points"
                }
            ));
        }
        Ok(made)
    }

    fn placement(&self) -> &RoadObjectPlacement {
        &self.object.placement
    }

    // The solved alignment, the profile and the surface are there whenever
    // they are asked for: new() refuses a placement along an alignment that
    // does not solve, one on the profile with no profile, and one on a
    // surface the drawing lacks (the C++ dereferences them as they are).
    fn solved(&self) -> &SolvedAlignment {
        self.alignment
            .as_ref()
            .expect("a placement along an alignment has its alignment solved when it is made")
    }

    fn profile(&self) -> &SolvedProfile {
        self.profile
            .as_ref()
            .expect("a placement on the profile has its profile solved when it is made")
    }

    fn surface(&self) -> &TinSurface {
        self.surface
            .expect("a placement on a surface has the surface when it is made")
    }

    // A station on the alignment, an end typed as the reply gave it kept on.
    fn checked_station(&self, station: f64) -> Made<f64> {
        let solved = self.solved();
        if station < solved.start_station() - STATION_TOLERANCE
            || station > solved.end_station() + STATION_TOLERANCE
        {
            return fail(format!(
                "station {} is off alignment {} ({} to {})",
                real(station),
                self.placement().alignment,
                real(solved.start_station()),
                real(solved.end_station())
            ));
        }
        Ok(cpp::clamp(
            station,
            solved.start_station(),
            solved.end_station(),
        ))
    }

    // The level at `position`, `station` and `offset` along the alignment.
    fn level_on_alignment(&self, station: f64, offset: f64, position: Point2) -> Made<f64> {
        if self.source == HeightSource::Profile {
            let Some(level) = self.profile().elevation_at(station) else {
                return fail(format!(
                    "station {} is outside the design profile of {}",
                    real(station),
                    self.placement().alignment
                ));
            };
            return Ok(level
                + offset.abs() * self.placement().crossfall / 100.0
                + self.placement().dz);
        }
        Ok(self.surface_level(position)? + self.placement().dz)
    }

    fn surface_level(&self, position: Point2) -> Made<f64> {
        let Some(level) = self.surface().elevation_at(position) else {
            return fail(format!(
                "the point {},{} is off surface {}",
                real(position.x),
                real(position.y),
                self.placement().surface
            ));
        };
        Ok(level)
    }

    // A vertex's level: its own height, or the surface's under it.
    fn vertex_level(&self, index: usize) -> Made<f64> {
        let vertex = &self.placement().vertices[index];
        if self.source == HeightSource::Surface {
            return Ok(self.surface_level(vertex.position)? + self.placement().dz);
        }
        let Some(z) = vertex.z else {
            return fail(format!(
                "point {} of the placement has no height: give surface=<name> to stand it on a \
                 surface",
                index + 1
            ));
        };
        Ok(z + self.placement().dz)
    }

    fn alignment_spot(&self, station: f64) -> Made<Spot> {
        let at = self.checked_station(station)?;
        let offset = self.placement().offset;
        let solved = self.solved();
        let (Some(point), Some(direction)) = (
            solved.point_at_station_offset(at, offset),
            solved.direction_at_station(at),
        ) else {
            return fail(format!(
                "station {} is off alignment {}",
                real(station),
                self.placement().alignment
            ));
        };
        Ok(Spot {
            position: point,
            z: self.level_on_alignment(at, offset, point)?,
            forward: unit_at(direction),
            offset,
        })
    }

    // The path a linear object runs along, with its ground levels.
    fn linear_path(&self) -> Made<Path> {
        let placement = self.placement();
        let kind = kind_word(self.object.kind);
        let mut points: Vec<PathPoint> = Vec::new();
        if placement.kind == PlacementKind::Alignment {
            let Some(to_station) = placement.to else {
                return fail(format!(
                    "a {kind} along an alignment runs from station= to to="
                ));
            };
            let from = self.checked_station(placement.station)?;
            let to = self.checked_station(to_station)?;
            if !(to > from) {
                return fail(format!(
                    "to= ({}) must be past station= ({})",
                    real(to_station),
                    real(placement.station)
                ));
            }
            let mut stations: Vec<f64> = Vec::new();
            let steps = (((to - from) / PATH_STEP).ceil() as usize).max(1);
            for i in 0..=steps {
                stations.push(from + (to - from) * i as f64 / steps as f64);
            }
            for key in self.solved().key_stations() {
                if key > from && key < to {
                    stations.push(key);
                }
            }
            stations.sort_by(f64::total_cmp);
            stations.dedup_by(|b, a| (*a - *b).abs() < 1e-6);
            for station in stations {
                let spot = self.alignment_spot(station)?;
                points.push(PathPoint::new(spot.position, spot.z));
            }
        } else {
            if placement.vertices.len() < 2 {
                return fail(format!("a {kind} runs along two points at least"));
            }
            for (i, vertex) in placement.vertices.iter().enumerate() {
                if self.source == HeightSource::Surface && i > 0 {
                    // Draped: the ground between two vertices sampled too.
                    let start = placement.vertices[i - 1].position;
                    let span = start.distance_to(vertex.position);
                    let steps = (span / PATH_STEP).ceil() as usize;
                    for s in 1..steps {
                        let between = start + (vertex.position - start) * (s as f64 / steps as f64);
                        points.push(PathPoint::new(
                            between,
                            self.surface_level(between)? + placement.dz,
                        ));
                    }
                }
                points.push(PathPoint::new(vertex.position, self.vertex_level(i)?));
            }
        }
        let path = Path::new(points);
        if path.size() < 2 || path.length() < 1e-3 {
            return fail(format!("the {kind} has no length"));
        }
        Ok(path)
    }

    // Where the objects of a point kind stand.
    #[allow(
        clippy::needless_range_loop,
        reason = "the index names a corner and the one after it, as the C++ loop does"
    )]
    fn spots(&self) -> Made<Vec<Spot>> {
        let placement = self.placement();
        let mut out = Vec::new();
        if placement.spacing.is_some_and(|spacing| !(spacing > 0.0)) {
            return fail("spacing= must be more than 0");
        }
        if placement.kind == PlacementKind::Alignment {
            if placement.to.is_some() && placement.spacing.is_none() {
                return fail(format!(
                    "to= needs spacing=: a {} every so many metres from station= to to=",
                    kind_word(self.object.kind)
                ));
            }
            if placement.spacing.is_some() && placement.to.is_none() {
                return fail("spacing= needs to=: where the run ends");
            }
            let (Some(to), Some(spacing)) = (placement.to, placement.spacing) else {
                out.push(self.alignment_spot(placement.station)?);
                return Ok(out);
            };
            if !(to >= placement.station) {
                return fail(format!(
                    "to= ({}) must be past station= ({})",
                    real(to),
                    real(placement.station)
                ));
            }
            for along in every_spacing(to - placement.station, spacing)? {
                out.push(self.alignment_spot(placement.station + along)?);
            }
            return Ok(out);
        }
        if placement.vertices.len() > MAX_PLACED_PARTS as usize {
            return fail(format!(
                "{} points is more than the {} a definition takes; place them in groups",
                placement.vertices.len(),
                MAX_PLACED_PARTS as usize
            ));
        }
        if placement.kind == PlacementKind::Points {
            for (i, vertex) in placement.vertices.iter().enumerate() {
                out.push(Spot {
                    position: vertex.position,
                    z: self.vertex_level(i)?,
                    // At points nothing says which way is along: north, so a
                    // sign faces south, down a drawing.
                    forward: Vec2::new(0.0, 1.0),
                    offset: 0.0,
                });
            }
            return Ok(out);
        }
        let Some(spacing) = placement.spacing else {
            // One at each corner of the line. A repeated vertex is the corner
            // it repeats, and a line that comes back to its start stands one
            // object there, not two (corners_of).
            let corners = corners_of(&placement.vertices);
            for k in 0..corners.len() {
                let forward = if corners.len() < 2 {
                    Vec2::new(0.0, 1.0)
                } else {
                    // Along the segment it starts, or the last one at the
                    // end: between corners that differ, so never a zero
                    // direction.
                    let from = if k + 1 < corners.len() { k } else { k - 1 };
                    (placement.vertices[corners[from + 1]].position
                        - placement.vertices[corners[from]].position)
                        .normalized()
                };
                out.push(Spot {
                    position: placement.vertices[corners[k]].position,
                    z: self.vertex_level(corners[k])?,
                    forward,
                    offset: 0.0,
                });
            }
            return Ok(out);
        };
        // Along a line, every `spacing` from its start; on a line that comes
        // back to its start, the distance that is the whole line is the start
        // again.
        let path = self.linear_path()?;
        let mut distances = every_spacing(path.length(), spacing)?;
        if distances.len() > 1
            && returns_to_start(&path)
            && path.length() - distances[distances.len() - 1] < SAME_POSITION
        {
            distances.pop();
        }
        for along in distances {
            let sample = path.at(along);
            out.push(Spot {
                position: sample.position,
                z: sample.z,
                forward: sample.direction,
                offset: 0.0,
            });
        }
        Ok(out)
    }
}

// ---- the 3D objects -----------------------------------------------------------------------

struct Classification {
    ifc_class: &'static str,
    predefined_type: &'static str,
    part: &'static str,
}

const fn classified(
    ifc_class: &'static str,
    predefined_type: &'static str,
    part: &'static str,
) -> Classification {
    Classification {
        ifc_class,
        predefined_type,
        part,
    }
}

fn property(key: &str, value: impl Into<String>) -> (String, String) {
    (String::from(key), value.into())
}

fn skeleton(
    object: &RoadObject,
    values: &Values<'_>,
    suffix: &str,
    name_suffix: &str,
    ifc: &Classification,
) -> BimObject {
    let mut made = BimObject {
        id: format!("object/{}{suffix}", object.id),
        name: format!("{}{name_suffix}", display_name(object)),
        kind: String::from(kind_word(object.kind)),
        facility: object.facility.clone(),
        // The part of the road it belongs to; none for an object of the site,
        // which belongs to no road.
        part: if object.facility.is_empty() {
            String::new()
        } else {
            String::from(ifc.part)
        },
        ifc_class: String::from(ifc.ifc_class),
        predefined_type: String::from(ifc.predefined_type),
        material: material_of(object),
        colour: colour_of(object),
        layer: layer_of(object),
        ..BimObject::default()
    };
    let placement = &object.placement;
    made.properties.push(property("Object", object.id.as_str()));
    made.properties
        .push(property("Kind", kind_word(object.kind)));
    made.properties
        .push(property("Placement", placement_word(placement.kind)));
    if placement.kind == PlacementKind::Alignment {
        made.properties
            .push(property("Alignment", placement.alignment.as_str()));
        made.properties
            .push(property("Station", real(placement.station)));
        if let Some(to) = placement.to {
            made.properties.push(property("To", real(to)));
        }
        made.properties
            .push(property("Offset", real(placement.offset)));
    } else if placement.source != INVALID_ENTITY_ID {
        made.properties
            .push(property("Source", format!("#{}", placement.source)));
    }
    made.properties.push(property(
        "Heights",
        height_word(height_source_of(placement)),
    ));
    if height_source_of(placement) == HeightSource::Surface {
        made.properties
            .push(property("Surface", placement.surface.as_str()));
    }
    for (key, value) in values.all() {
        made.properties.push(property(key, parameter_text(value)));
    }
    made
}

fn add_quantity(object: &mut BimObject, name: &str, value: f64, unit: &str) {
    object.quantities.push(Quantity {
        name: String::from(name),
        value,
        unit: String::from(unit),
    });
}

// Which way the traffic is, as the sign of u: +1 left of the line, -1 right.
fn traffic_side(object: &RoadObject, values: &Values<'_>) -> f64 {
    let side = values.text("side");
    if side == "left" {
        return 1.0;
    }
    if side == "right" {
        return -1.0;
    }
    // Along an alignment, the traffic is towards the centreline: on the right
    // of a barrier left of it, on the left of one right of it.
    if object.placement.kind == PlacementKind::Alignment && object.placement.offset < 0.0 {
        return 1.0;
    }
    -1.0
}

fn rectangle_profile(u0: f64, u1: f64, v0: f64, v1: f64) -> Vec<Point2> {
    vec![
        Point2::new(cpp::min(u0, u1), v0),
        Point2::new(cpp::max(u0, u1), v0),
        Point2::new(cpp::max(u0, u1), v1),
        Point2::new(cpp::min(u0, u1), v1),
    ]
}

fn base_of(sample: &Sample) -> Point3 {
    Point3::new(sample.position.x, sample.position.y, sample.z)
}

fn make_barrier(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let path = placement.linear_path()?;
    let barrier_type = values.text("type");
    let height = values.number("height")?;
    let concrete = barrier_type == "fshape" || barrier_type == "singleslope";
    let mut made = skeleton(
        object,
        &values,
        "",
        "",
        &classified("IfcRailing", "GUARDRAIL", "ROADSIDE"),
    );
    let mut posts: usize = 0;
    if barrier_type == "wbeam" || barrier_type == "thriebeam" {
        let rail = if barrier_type == "wbeam" {
            W_BEAM_RAIL_HEIGHT
        } else {
            THRIE_BEAM_RAIL_HEIGHT
        };
        if height < rail {
            return fail(format!(
                "height= ({}) is less than the rail ({} m)",
                real(height),
                real(rail)
            ));
        }
        for along in posts_along(&path, values.number("postspacing")?)? {
            let sample = path.at(along);
            add_box(
                &mut made.mesh,
                base_of(&sample),
                sample.direction,
                POST_FLANGE,
                POST_DEPTH,
                height,
            );
            posts += 1;
        }
        // The rail on the traffic face of the posts.
        let side = traffic_side(object, &values);
        let profile = rectangle_profile(
            side * POST_DEPTH / 2.0,
            side * (POST_DEPTH / 2.0 + BEAM_DEPTH),
            height - rail,
            height,
        );
        add_sweep(&mut made.mesh, path.points(), &profile);
    } else if barrier_type == "wirerope" {
        let ropes = values.number("ropes")? as usize;
        let lowest = height - ROPE_SPACING * ropes.wrapping_sub(1) as f64;
        if lowest < ROPE_DIAMETER {
            return fail(format!(
                "{ropes} ropes {} m apart do not fit under a top rope at {} m",
                real(ROPE_SPACING),
                real(height)
            ));
        }
        for along in posts_along(&path, values.number("postspacing")?)? {
            let sample = path.at(along);
            add_box(
                &mut made.mesh,
                base_of(&sample),
                sample.direction,
                ROPE_POST,
                ROPE_POST,
                height + ROPE_POST_ABOVE,
            );
            posts += 1;
        }
        for k in 0..ropes {
            let centre = height - ROPE_SPACING * k as f64;
            add_sweep(
                &mut made.mesh,
                path.points(),
                &rectangle_profile(
                    -ROPE_DIAMETER / 2.0,
                    ROPE_DIAMETER / 2.0,
                    centre - ROPE_DIAMETER / 2.0,
                    centre + ROPE_DIAMETER / 2.0,
                ),
            );
        }
    } else {
        let width = values.number("width")?;
        let profile = if barrier_type == "fshape" {
            if height <= F_SHAPE_BREAK {
                return fail(format!(
                    "an F-shape barrier is taller than its {} m slope break",
                    real(F_SHAPE_BREAK)
                ));
            }
            let base = width / 2.0;
            let knee =
                base - (F_SHAPE_BREAK - F_SHAPE_REVEAL) / radians(F_SHAPE_LOWER_DEGREES).tan();
            let top = knee - (height - F_SHAPE_BREAK) / radians(F_SHAPE_UPPER_DEGREES).tan();
            if top < 0.025 {
                return fail(format!(
                    "an F-shape {} m wide is too narrow for {} m: its faces meet",
                    real(width),
                    real(height)
                ));
            }
            vec![
                Point2::new(-base, 0.0),
                Point2::new(base, 0.0),
                Point2::new(base, F_SHAPE_REVEAL),
                Point2::new(knee, F_SHAPE_BREAK),
                Point2::new(top, height),
                Point2::new(-top, height),
                Point2::new(-knee, F_SHAPE_BREAK),
                Point2::new(-base, F_SHAPE_REVEAL),
            ]
        } else {
            let top = values.number("topwidth")?;
            if top > width {
                return fail("a single slope barrier's top is no wider than its base");
            }
            vec![
                Point2::new(-width / 2.0, 0.0),
                Point2::new(width / 2.0, 0.0),
                Point2::new(top / 2.0, height),
                Point2::new(-top / 2.0, height),
            ]
        };
        add_sweep(&mut made.mesh, path.points(), &profile);
    }
    made.plan.push(path.plan());
    add_quantity(&mut made, "Length", path.length(), "m");
    add_quantity(&mut made, "Height", height, "m");
    if posts > 0 {
        add_quantity(&mut made, "Count", posts as f64, "");
    }
    if concrete {
        let volume = mesh_volume(&made.mesh);
        add_quantity(&mut made, "GrossVolume", volume, "m3");
    }
    Ok(vec![made])
}

fn make_wall(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let path = placement.linear_path()?;
    let retaining = values.text("type") == "retaining";
    let height = values.number("height")?;
    let thickness = values.number("thickness")?;
    let footing = values.number("footing")?;
    let depth = values.number("footingdepth")?;
    // The stem: a retaining wall is IFC 4.3's RETAININGWALL; a noise wall has
    // no type of its own there, so it is USERDEFINED with its ObjectType said.
    let mut wall = skeleton(
        object,
        &values,
        "",
        "",
        &classified(
            "IfcWall",
            if retaining {
                "RETAININGWALL"
            } else {
                "USERDEFINED"
            },
            "ROADSIDE",
        ),
    );
    if !retaining {
        wall.properties.push(property("ObjectType", "NOISEBARRIER"));
    }
    wall.properties.push(property(
        "WallType",
        if retaining { "retaining" } else { "noise" },
    ));
    add_sweep(
        &mut wall.mesh,
        path.points(),
        &rectangle_profile(-thickness / 2.0, thickness / 2.0, 0.0, height),
    );
    wall.plan.push(path.plan());
    add_quantity(&mut wall, "Length", path.length(), "m");
    add_quantity(&mut wall, "Height", height, "m");
    add_quantity(&mut wall, "GrossArea", path.length() * height, "m2");
    let volume = mesh_volume(&wall.mesh);
    add_quantity(&mut wall, "GrossVolume", volume, "m3");
    let mut made = vec![wall];
    if footing > 0.0 {
        // The strip footing it stands on, below the ground: an object of its
        // own, as IFC keeps a foundation apart from what it carries.
        let mut base = skeleton(
            object,
            &values,
            "/footing",
            " footing",
            &classified("IfcFooting", "STRIP_FOOTING", "ROADSIDE"),
        );
        add_sweep(
            &mut base.mesh,
            path.points(),
            &rectangle_profile(-footing / 2.0, footing / 2.0, -depth, 0.0),
        );
        base.plan.push(path.plan());
        add_quantity(&mut base, "Length", path.length(), "m");
        add_quantity(&mut base, "Width", footing, "m");
        add_quantity(&mut base, "Depth", depth, "m");
        let volume = mesh_volume(&base.mesh);
        add_quantity(&mut base, "GrossVolume", volume, "m3");
        made.push(base);
    }
    Ok(made)
}

fn make_fence(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let path = placement.linear_path()?;
    let height = values.number("height")?;
    let post = values.number("postsize")?;
    let rails = values.number("rails")? as usize;
    let mut made = skeleton(
        object,
        &values,
        "",
        "",
        &classified("IfcRailing", "FENCE", "ROADSIDE"),
    );
    let mut posts: usize = 0;
    for along in posts_along(&path, values.number("postspacing")?)? {
        let sample = path.at(along);
        add_box(
            &mut made.mesh,
            base_of(&sample),
            sample.direction,
            post,
            post,
            height,
        );
        posts += 1;
    }
    // The wires evenly up the posts, the top one level with their tops.
    for k in 1..=rails {
        let top = height * k as f64 / rails as f64;
        add_sweep(
            &mut made.mesh,
            path.points(),
            &rectangle_profile(
                -FENCE_STRAND / 2.0,
                FENCE_STRAND / 2.0,
                top - FENCE_STRAND,
                top,
            ),
        );
    }
    made.plan.push(path.plan());
    add_quantity(&mut made, "Length", path.length(), "m");
    add_quantity(&mut made, "Height", height, "m");
    add_quantity(&mut made, "Count", posts as f64, "");
    Ok(vec![made])
}

fn make_marking(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let path = placement.linear_path()?;
    let width = values.number("width")?;
    let dash = values.number("dash")?;
    let gap = values.number("gap")?;
    let thickness = values.number("thickness")?;
    if dash > 0.0 && !(gap > 0.0) {
        return fail("a broken line needs gap= as well as dash=");
    }
    if dash > 0.0 {
        // A mark shorter than the millimetre a sweep passes points closer than
        // has no solid, and would still be counted and measured.
        if dash < SHORTEST_MARK {
            return fail(format!(
                "dash= ({} m) is shorter than the {} m a mark is made to; give 0 for a \
                 continuous line",
                real(dash),
                real(SHORTEST_MARK)
            ));
        }
        let marks = (path.length() / (dash + gap)).ceil();
        if !(marks <= MAX_PLACED_PARTS) {
            return fail(format!(
                "dash= {} m and gap= {} m along {} m would make {} marks, more than the {} a \
                 definition takes; place the line in lengths, or give a longer dash and gap",
                real(dash),
                real(gap),
                real(path.length()),
                real(marks),
                MAX_PLACED_PARTS as usize
            ));
        }
    }
    let mut made = skeleton(
        object,
        &values,
        "",
        "",
        &classified("IfcSurfaceFeature", "LINEMARKING", "CARRIAGEWAY"),
    );
    let profile = rectangle_profile(-width / 2.0, width / 2.0, 0.0, thickness);
    let mut marked = 0.0;
    let mut marks: usize = 0;
    let mut mark = |from: f64, to: f64| {
        if to - from < SHORTEST_MARK {
            return; // what is left of the line after its last whole mark
        }
        let piece = path.between(from, to);
        add_sweep(&mut made.mesh, &piece, &profile);
        made.plan.push(Polyline2::new(
            piece.iter().map(|point| point.position).collect(),
            false,
        ));
        marked += to - from;
        marks += 1;
    };
    if dash <= 0.0 {
        mark(0.0, path.length());
    } else {
        // The pattern starts with a mark at the start of the line.
        let mut from = 0.0;
        while from < path.length() - 1e-6 {
            mark(from, cpp::min(from + dash, path.length()));
            from += dash + gap;
        }
    }
    made.properties
        .push(property("MarkingType", values.text("type")));
    add_quantity(&mut made, "Length", marked, "m");
    add_quantity(&mut made, "GrossArea", marked * width, "m2");
    add_quantity(&mut made, "Count", marks as f64, "");
    Ok(vec![made])
}

// A post's outline in plan: a square of `size` standing on `spot`, along its
// forward direction.
fn square_outline(position: Point2, along: Vec2, length: f64, width: f64) -> Polyline2 {
    let a = along * (length / 2.0);
    let l = along.perpendicular() * (width / 2.0);
    Polyline2::new(
        vec![
            position - a - l,
            position + a - l,
            position + a + l,
            position - a + l,
        ],
        true,
    )
}

fn make_guide_posts(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let height = values.number("height")?;
    let size = values.number("size")?;
    let mut out = Vec::new();
    let mut n: usize = 0;
    for spot in placement.spots()? {
        n += 1;
        let mut made = skeleton(
            object,
            &values,
            &format!("/{n}"),
            &format!(" #{n}"),
            &classified("IfcSign", "MARKER", "ROADSIDE"),
        );
        add_box(
            &mut made.mesh,
            Point3::new(spot.position.x, spot.position.y, spot.z),
            spot.forward,
            size,
            size,
            height,
        );
        made.plan
            .push(square_outline(spot.position, spot.forward, size, size));
        add_quantity(&mut made, "Count", 1.0, "");
        add_quantity(&mut made, "Height", height, "m");
        out.push(made);
    }
    Ok(out)
}

// A sign's plate in (u across its face, seen from the front, v up), its lower
// edge at `bottom`.
fn sign_plate(shape: &str, width: f64, plate_height: f64, bottom: f64) -> Vec<Point2> {
    if shape == "diamond" {
        let half = width / 2.0;
        return vec![
            Point2::new(0.0, bottom),
            Point2::new(half, bottom + half),
            Point2::new(0.0, bottom + width),
            Point2::new(-half, bottom + half),
        ];
    }
    if shape == "circle" {
        return regular_polygon(
            Point2::new(0.0, bottom + width / 2.0),
            width / 2.0,
            ROUND_SIGN_SIDES,
            -PI / 2.0,
        );
    }
    if shape == "octagon" {
        // `width` across the flats: the circumradius is half of it over
        // cos 22.5 degrees, a flat at the bottom.
        let radius = width / 2.0 / (PI / 8.0).cos();
        return regular_polygon(Point2::new(0.0, bottom + width / 2.0), radius, 8, PI / 8.0);
    }
    if shape == "triangle" {
        let tall = width * 3.0_f64.sqrt() / 2.0;
        return vec![
            Point2::new(-width / 2.0, bottom),
            Point2::new(width / 2.0, bottom),
            Point2::new(0.0, bottom + tall),
        ];
    }
    vec![
        Point2::new(-width / 2.0, bottom),
        Point2::new(width / 2.0, bottom),
        Point2::new(width / 2.0, bottom + plate_height),
        Point2::new(-width / 2.0, bottom + plate_height),
    ]
}

fn polygon_area(polygon: &[Point2]) -> f64 {
    Polyline2::new(polygon.to_vec(), true).area()
}

fn make_signs(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let width = values.number("width")?;
    let clearance = values.number("clearance")?;
    let post = values.number("postsize")?;
    let thickness = values.number("thickness")?;
    let posts = values.number("posts")? as usize;
    let plate = sign_plate(
        &values.text("shape"),
        width,
        values.number("plateheight")?,
        clearance,
    );
    let mut top = clearance;
    for corner in &plate {
        top = cpp::max(top, corner.y);
    }
    let mut out = Vec::new();
    let mut n: usize = 0;
    for spot in placement.spots()? {
        n += 1;
        let mut made = skeleton(
            object,
            &values,
            &format!("/{n}"),
            &format!(" #{n}"),
            &classified("IfcSign", "PICTORAL", "ROADSIDE"),
        );
        // The face looks back along the placement, at the traffic coming up
        // it, unless facing= says otherwise.
        let face = match values.optional_number("facing") {
            Some(facing) => unit_at(radians(facing)),
            None => -spot.forward,
        };
        // Across the face, to the right of someone reading it: up x face.
        let across = Vec2::new(-face.y, face.x);
        let base = Point3::new(spot.position.x, spot.position.y, spot.z);
        // The posts on the spot, the plate on their front.
        let post_at: Vec<f64> = if posts == 2 {
            vec![-width / 4.0, width / 4.0]
        } else {
            vec![0.0]
        };
        for u in post_at {
            let at = spot.position + across * u;
            add_box(
                &mut made.mesh,
                Point3::new(at.x, at.y, spot.z),
                face,
                post,
                post,
                top,
            );
        }
        let u = Vec3::new(across.x, across.y, 0.0);
        let v = Vec3::new(0.0, 0.0, 1.0);
        let back = base + Vec3::new(face.x, face.y, 0.0) * (post / 2.0);
        add_extrusion(&mut made.mesh, &plate, back, u, v, thickness);
        made.plan.push(Polyline2::new(
            vec![
                Point2::new(back.x, back.y) - across * (width / 2.0),
                Point2::new(back.x, back.y) + across * (width / 2.0),
            ],
            false,
        ));
        if !values.text("code").is_empty() {
            made.properties
                .push(property("SignCode", values.text("code")));
        }
        if !values.text("text").is_empty() {
            made.properties
                .push(property("SignText", values.text("text")));
        }
        add_quantity(&mut made, "Count", 1.0, "");
        add_quantity(&mut made, "Height", top, "m");
        add_quantity(&mut made, "GrossArea", polygon_area(&plate), "m2");
        out.push(made);
    }
    Ok(out)
}

fn make_lights(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let height = values.number("height")?;
    let outreach = values.number("outreach")?;
    let size = values.number("polesize")?;
    let facing = values.optional_number("facing");
    let mut out = Vec::new();
    let mut n: usize = 0;
    for spot in placement.spots()? {
        n += 1;
        // The arm reaches over the road: towards the centreline along an
        // alignment, to the left of a line, south at a point.
        let mut arm = spot.forward.perpendicular();
        if let Some(facing) = facing {
            arm = unit_at(radians(facing));
        } else if object.placement.kind == PlacementKind::Alignment && spot.offset > 0.0 {
            arm = -arm;
        } else if object.placement.kind == PlacementKind::Points {
            arm = -spot.forward;
        }
        let item = format!("/{n}");
        let mut pole = skeleton(
            object,
            &values,
            &format!("{item}/pole"),
            &format!(" #{n} pole"),
            &classified("IfcMember", "POST", "ROADSIDE"),
        );
        let radius = size / 2.0 / (PI / f64::from(POLE_SIDES)).cos();
        let section = regular_polygon(
            spot.position,
            radius,
            POLE_SIDES,
            PI / f64::from(POLE_SIDES),
        );
        add_prism(&mut pole.mesh, &section, spot.z, spot.z + height);
        pole.plan.push(Polyline2::new(section, true));
        let mut lantern_base = spot.z + height;
        let mut lantern_at = spot.position;
        if outreach > 0.0 {
            let middle = spot.position + arm * (outreach / 2.0);
            add_box(
                &mut pole.mesh,
                Point3::new(middle.x, middle.y, spot.z + height - ARM_SECTION),
                arm,
                outreach,
                ARM_SECTION,
                ARM_SECTION,
            );
            lantern_at = spot.position + arm * outreach;
            lantern_base = spot.z + height - ARM_SECTION - LUMINAIRE_HEIGHT;
            pole.plan
                .push(Polyline2::new(vec![spot.position, lantern_at], false));
        }
        add_quantity(&mut pole, "Count", 1.0, "");
        add_quantity(&mut pole, "Height", height, "m");
        let mut lantern = skeleton(
            object,
            &values,
            &format!("{item}/luminaire"),
            &format!(" #{n} luminaire"),
            &classified("IfcLightFixture", "POINTSOURCE", "ROADSIDE"),
        );
        add_box(
            &mut lantern.mesh,
            Point3::new(lantern_at.x, lantern_at.y, lantern_base),
            arm,
            LUMINAIRE_LENGTH,
            LUMINAIRE_WIDTH,
            LUMINAIRE_HEIGHT,
        );
        add_quantity(&mut lantern, "Count", 1.0, "");
        out.push(pole);
        out.push(lantern);
    }
    Ok(out)
}

// The barrel's inside and outside, in (u across, v up from the invert).
struct BarrelSection {
    outer_width: f64,
    outer_height: f64,
    // From the invert up to the barrel's outside top.
    above_invert: f64,
}

fn barrel_section(values: &Values<'_>) -> Made<BarrelSection> {
    let wall = values.number("wall")?;
    if values.text("shape") == "box" {
        let span = values.number("span")?;
        let rise = values.number("rise")?;
        return Ok(BarrelSection {
            outer_width: span + 2.0 * wall,
            outer_height: rise + 2.0 * wall,
            above_invert: rise + wall,
        });
    }
    let diameter = values.number("diameter")?;
    Ok(BarrelSection {
        outer_width: diameter + 2.0 * wall,
        outer_height: diameter + 2.0 * wall,
        above_invert: diameter + wall,
    })
}

fn resolve_culvert(object: &RoadObject, placement: &Placement<'_>) -> Made<CulvertEnds> {
    let values = Values::new(object);
    let place = &object.placement;
    let section = barrel_section(&values)?;
    let mut ground_in: Option<f64> = None;
    let mut ground_out: Option<f64> = None;
    let invert = values.optional_number("invert");
    let invert2 = values.optional_number("invert2");
    let need_ground = invert.is_none() || invert2.is_none();
    let (inlet, outlet) = if place.kind == PlacementKind::Alignment {
        if place.to.is_some() {
            return fail(
                "a culvert crosses the alignment at one station: give length= and skew=, not to=",
            );
        }
        let centre = placement.alignment_spot(place.station)?;
        let forward = centre.forward;
        let left = forward.perpendicular();
        let skew = radians(values.number("skew")?);
        let length = values.number("length")?;
        // Square across the alignment, turned `skew` counter-clockwise; the
        // inlet on the right.
        let axis = left * skew.cos() - forward * skew.sin();
        let inlet = centre.position - axis * (length / 2.0);
        let outlet = centre.position + axis * (length / 2.0);
        if need_ground {
            // Each end's own station and offset, for the profile: exact on a
            // straight, the end's frame at the culvert's station on a curve.
            // The station matters to the profile alone, so it is checked
            // there: an end of a skewed culvert near the alignment's start may
            // fall before it and still stand on a surface that covers it.
            let along = skew.sin() * length / 2.0;
            let across = skew.cos() * length / 2.0;
            let level = |station: f64, offset: f64, at: Point2| -> Made<f64> {
                if placement.source == HeightSource::Surface {
                    return Ok(placement.surface_level(at)? + place.dz);
                }
                placement.level_on_alignment(placement.checked_station(station)?, offset, at)
            };
            ground_in = Some(level(place.station + along, place.offset - across, inlet)?);
            ground_out = Some(level(place.station - along, place.offset + across, outlet)?);
        }
        (inlet, outlet)
    } else {
        if place.kind == PlacementKind::Points && place.vertices.len() != 2 {
            return fail("a culvert AT points takes two: its inlet, then its outlet");
        }
        if place.vertices.len() < 2 {
            return fail("a culvert along a line needs its two ends");
        }
        let inlet = place.vertices[0].position;
        let outlet = place.vertices[place.vertices.len() - 1].position;
        if placement.source == HeightSource::Line {
            // A culvert's line is drawn at its invert, as a drainage line is;
            // an invert given wins, and a vertex it makes unneeded may have no
            // height.
            let last = place.vertices.len() - 1;
            let at_inlet = match invert {
                Some(level) => level,
                None => placement.vertex_level(0)?,
            };
            let at_outlet = match (invert2, invert) {
                (Some(level), _) | (None, Some(level)) => level,
                (None, None) => placement.vertex_level(last)?,
            };
            if inlet.distance_to(outlet) < 0.1 {
                return fail("the culvert's ends are in one place");
            }
            return Ok(CulvertEnds {
                inlet: Point3::new(inlet.x, inlet.y, at_inlet),
                outlet: Point3::new(outlet.x, outlet.y, at_outlet),
            });
        }
        if need_ground {
            ground_in = Some(placement.surface_level(inlet)? + place.dz);
            ground_out = Some(placement.surface_level(outlet)? + place.dz);
        }
        (inlet, outlet)
    };
    if inlet.distance_to(outlet) < 0.1 {
        return fail("the culvert's ends are in one place");
    }
    let cover = values.number("cover")?;
    // A ground level is found whenever an invert is not given (need_ground),
    // so the NaN below is never reached.
    let from_cover =
        |ground: Option<f64>| ground.map_or(f64::NAN, |level| level - cover - section.above_invert);
    let inlet_invert = invert.unwrap_or_else(|| from_cover(ground_in));
    let outlet_invert = match (invert2, invert) {
        (Some(level), _) | (None, Some(level)) => level,
        (None, None) => from_cover(ground_out),
    };
    Ok(CulvertEnds {
        inlet: Point3::new(inlet.x, inlet.y, inlet_invert),
        outlet: Point3::new(outlet.x, outlet.y, outlet_invert),
    })
}

fn make_culvert(object: &RoadObject, placement: &Placement<'_>) -> Made<Vec<BimObject>> {
    let values = Values::new(object);
    let ends = resolve_culvert(object, placement)?;
    let section = barrel_section(&values)?;
    let cells = values.number("cells")? as usize;
    let gap = values.number("cellgap")?;
    let wall = values.number("wall")?;
    let is_box = values.text("shape") == "box";
    let pitch = section.outer_width + gap;
    let total = section.outer_width * cells as f64 + gap * cells.wrapping_sub(1) as f64;
    let path = vec![
        PathPoint::new(Point2::new(ends.inlet.x, ends.inlet.y), ends.inlet.z),
        PathPoint::new(Point2::new(ends.outlet.x, ends.outlet.y), ends.outlet.z),
    ];
    let mut barrel = skeleton(
        object,
        &values,
        "/barrel",
        " barrel",
        &classified("IfcPipeSegment", "CULVERT", "ROADSEGMENT"),
    );
    for k in 0..cells {
        let u = (k as f64 - cells.wrapping_sub(1) as f64 / 2.0) * pitch;
        if is_box {
            let span = values.number("span")?;
            let rise = values.number("rise")?;
            add_hollow_sweep(
                &mut barrel.mesh,
                &path,
                &rectangle_profile(
                    u - span / 2.0 - wall,
                    u + span / 2.0 + wall,
                    -wall,
                    rise + wall,
                ),
                &rectangle_profile(u - span / 2.0, u + span / 2.0, 0.0, rise),
            );
        } else {
            let diameter = values.number("diameter")?;
            let centre = Point2::new(u, diameter / 2.0);
            add_hollow_sweep(
                &mut barrel.mesh,
                &path,
                &regular_polygon(centre, diameter / 2.0 + wall, PIPE_SIDES, 0.0),
                &regular_polygon(centre, diameter / 2.0, PIPE_SIDES, 0.0),
            );
        }
    }
    let inlet = Point2::new(ends.inlet.x, ends.inlet.y);
    let outlet = Point2::new(ends.outlet.x, ends.outlet.y);
    let axis = (outlet - inlet).normalized();
    let left = axis.perpendicular();
    barrel.plan.push(Polyline2::new(
        vec![
            inlet - left * (total / 2.0),
            outlet - left * (total / 2.0),
            outlet + left * (total / 2.0),
            inlet + left * (total / 2.0),
        ],
        true,
    ));
    barrel
        .properties
        .push(property("Inlet", point_text(ends.inlet)));
    barrel
        .properties
        .push(property("Outlet", point_text(ends.outlet)));
    add_quantity(
        &mut barrel,
        "Length",
        (ends.outlet - ends.inlet).length(),
        "m",
    );
    add_quantity(&mut barrel, "Count", cells as f64, "");
    let volume = mesh_volume(&barrel.mesh);
    add_quantity(&mut barrel, "GrossVolume", volume, "m3");
    let mut made = vec![barrel];
    if values.text("headwalls") == "on" {
        let width = total + 2.0 * HEADWALL_MARGIN;
        let tall = section.outer_height + HEADWALL_MARGIN;
        for at_inlet in [true, false] {
            let end = if at_inlet { ends.inlet } else { ends.outlet };
            let outward = if at_inlet { -axis } else { axis };
            let centre = Point2::new(end.x, end.y) + outward * (HEADWALL_THICKNESS / 2.0);
            let mut headwall = skeleton(
                object,
                &values,
                if at_inlet {
                    "/headwall/inlet"
                } else {
                    "/headwall/outlet"
                },
                if at_inlet {
                    " inlet headwall"
                } else {
                    " outlet headwall"
                },
                &classified("IfcWall", "RETAININGWALL", "ROADSEGMENT"),
            );
            // Standing on the barrel's outside bottom, outside its end.
            add_box(
                &mut headwall.mesh,
                Point3::new(centre.x, centre.y, end.z - wall),
                axis,
                HEADWALL_THICKNESS,
                width,
                tall,
            );
            headwall
                .plan
                .push(square_outline(centre, axis, HEADWALL_THICKNESS, width));
            add_quantity(&mut headwall, "Width", width, "m");
            let volume = mesh_volume(&headwall.mesh);
            add_quantity(&mut headwall, "GrossVolume", volume, "m3");
            made.push(headwall);
        }
    }
    Ok(made)
}

fn same_word(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

// ---- parameters -----------------------------------------------------------------------------

/// The parameters `kind` takes beyond the ones every object takes (apply_key),
/// in the order the dialog shows them and KINDS lists them.
pub fn parameters_of(kind: RoadObjectKind) -> &'static [ParameterSpec] {
    match kind {
        RoadObjectKind::Barrier => &BARRIER_PARAMETERS,
        RoadObjectKind::GuidePost => &GUIDE_POST_PARAMETERS,
        RoadObjectKind::Sign => &SIGN_PARAMETERS,
        RoadObjectKind::LightPole => &LIGHT_PARAMETERS,
        RoadObjectKind::Culvert => &CULVERT_PARAMETERS,
        RoadObjectKind::Wall => &WALL_PARAMETERS,
        RoadObjectKind::Fence => &FENCE_PARAMETERS,
        RoadObjectKind::Marking => &MARKING_PARAMETERS,
    }
}

/// None for a key `kind` does not take.
pub fn find_parameter(kind: RoadObjectKind, key: &str) -> Option<&'static ParameterSpec> {
    parameters_of(kind)
        .iter()
        .find(|spec| same_word(spec.key, key))
}

/// Where the kind's defaults come from: the standard, or "typical", said.
pub fn kind_source(kind: RoadObjectKind) -> &'static str {
    match kind {
        RoadObjectKind::Barrier => {
            "Beam rails of AASHTO M 180 (W-beam 311 mm, thrie-beam 508 mm, 83 mm deep) on 150 x \
             100 mm steel posts at 1.905 m; tops of rail 787 mm (the MASH-tested Midwest Guardrail \
             System) and 864 mm; the F-shape's 76 mm reveal, 55 degrees to 254 mm and 84 degrees \
             above on a 610 mm base, 813 mm tall, and the 1067 mm single slope (AASHTO Roadside \
             Design Guide); wire rope typical of the proprietary systems. The job's standard \
             drawings govern."
        }
        RoadObjectKind::GuidePost => {
            "Guide posts as AS 1742.2 sets them out; 1.0 m above the ground and 100 mm square is \
             typical. The job's standard drawing governs."
        }
        RoadObjectKind::Sign => {
            "The plate's lower edge 1.5 m above the ground: the rural minimum of the MUTCD (2009), \
             section 2A.18 (5 ft; 7 ft, 2.1 m, where there are pedestrians). A 900 x 600 mm plate \
             on a 75 mm post is typical; the sign's standard governs."
        }
        RoadObjectKind::LightPole => {
            "A 10 m mounting height and a 1.5 m outreach, typical of road lighting to AS/NZS 1158; \
             the lighting design governs."
        }
        RoadObjectKind::Culvert => {
            "A 900 mm reinforced concrete pipe with a 100 mm wall, typical of the AS/NZS 4058 \
             sizes; 600 mm of cover, a typical minimum under a pavement (AS/NZS 3725 governs the \
             loads); headwalls typical of precast units."
        }
        RoadObjectKind::Wall => {
            "A 2.0 m retaining wall with a 250 mm stem on a 1.2 x 0.4 m strip footing, and a 4.0 m \
             noise wall of 200 mm panels: typical; the structural design governs."
        }
        RoadObjectKind::Fence => {
            "1.2 m posts at 3 m with three wires: a typical rural boundary fence."
        }
        RoadObjectKind::Marking => {
            "AS 1742.2's longitudinal lines: an edge line 150 mm; dividing and lane lines 100 mm, \
             broken 3 m line and 9 m gap; continuity lines 1 m and 3 m; 3 mm of thermoplastic. The \
             job's line marking plan governs."
        }
    }
}

/// `text` read as `key` of `kind`: a number in range, a choice in the list
/// (folded to lower case), a text as it is. NotFound for a key the kind does
/// not take; ParseFailure or InvalidArgument naming the key and the value.
pub fn parse_parameter(kind: RoadObjectKind, key: &str, text: &str) -> Result<ParameterValue> {
    let Some(spec) = find_parameter(kind, key) else {
        let keys = parameters_of(kind)
            .iter()
            .map(|each| each.key)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(make_error(
            ErrorCode::NotFound,
            format!(
                "a {} takes no {key}=; it takes {keys}, and the keys every object takes",
                kind_word(kind)
            ),
            key,
        ));
    };
    match spec.r#type {
        ParameterType::Number => {
            let Some(value) = parse_finite_double(trimmed(text.as_bytes())) else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    format!("{}= must be a number", spec.key),
                    text,
                ));
            };
            if spec.whole && value != value.floor() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{}= is a whole number", spec.key),
                    text,
                ));
            }
            if value < spec.minimum || value > spec.maximum {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "{}= must be from {} to {}{}",
                        spec.key,
                        real(spec.minimum),
                        real(spec.maximum),
                        if spec.unit.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", spec.unit)
                        }
                    ),
                    text,
                ));
            }
            Ok(ParameterValue::Number(value))
        }
        ParameterType::Choice => {
            let folded = folded(text);
            if spec.choices.contains(&folded.as_str()) {
                return Ok(ParameterValue::Text(folded));
            }
            Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{}= is one of {}", spec.key, spec.choices.join(", ")),
                text,
            ))
        }
        ParameterType::Text => Ok(ParameterValue::Text(String::from(text))),
    }
}

/// A value as a reply or a line says it back: a number exactly, a word as it
/// is.
pub fn parameter_text(value: &ParameterValue) -> String {
    match value {
        ParameterValue::Number(number) => real(*number),
        ParameterValue::Text(text) => text.clone(),
    }
}

// ---- the definition -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PlacementKind {
    /// ALIGNMENT <name> station= [to=] offset= [spacing=]
    #[default]
    Alignment,
    /// ALONG a drawn line, its vertices copied when it was placed
    Line,
    /// AT points
    Points,
}

/// "alignment", "line", "points".
pub fn placement_word(kind: PlacementKind) -> &'static str {
    match kind {
        PlacementKind::Alignment => "alignment",
        PlacementKind::Line => "line",
        PlacementKind::Points => "points",
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HeightSource {
    #[default]
    Profile,
    Surface,
    Line,
}

/// "profile", "surface", "line".
pub fn height_word(source: HeightSource) -> &'static str {
    match source {
        HeightSource::Profile => "profile",
        HeightSource::Surface => "surface",
        HeightSource::Line => "line",
    }
}

pub fn height_from_word(word: &str) -> Result<HeightSource> {
    let folded = folded(word);
    for source in [
        HeightSource::Profile,
        HeightSource::Surface,
        HeightSource::Line,
    ] {
        if folded == height_word(source) {
            return Ok(source);
        }
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        "heights= is profile, surface or line",
        word,
    ))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlacedVertex {
    pub position: Point2,
    /// Absent where the line or the point had no height: absent is not zero.
    pub z: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadObjectPlacement {
    pub kind: PlacementKind,
    /// Alignment: by name, looked up each time the object is made.
    pub alignment: String,
    pub station: f64,
    /// The end station of a linear object, or of a run of point objects.
    pub to: Option<f64>,
    /// Left of the centreline is positive, looking along increasing station.
    pub offset: f64,
    /// Line, Points: the vertices, copied in when the object was placed - a
    /// line deleted or edited afterwards leaves the object where it was.
    pub vertices: Vec<PlacedVertex>,
    /// Line: the entity they were copied from, for a person; 0 for none.
    pub source: EntityId,
    /// A point kind: one every `spacing` from the start to `to` (or along the
    /// line). Not given: one at the station, or one at each vertex.
    pub spacing: Option<f64>,
    /// Not given: Profile along an alignment and Line otherwise - Surface when
    /// a surface is named (height_source_of).
    pub heights: Option<HeightSource>,
    pub surface: String,
    /// Percent, from the centreline out: the profile's level at |offset| is
    /// the level at the centreline plus |offset| x crossfall / 100.
    pub crossfall: f64,
    /// Raised (or lowered, negative) from the ground the heights give.
    pub dz: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadObject {
    /// "barrier-3": the kind's word and the set's next number, never reused,
    /// so a reply, the IFC GlobalId and a person's notes name it alike.
    pub id: String,
    pub kind: RoadObjectKind,
    /// Empty: the kind's title and the id.
    pub name: String,
    /// Empty: road/objects/<kind word>, made in the same undo step when the
    /// drawing lacks it.
    pub layer: String,
    /// Absent: the kind's colour.
    pub colour: Option<Color>,
    /// Empty: the kind's (and its type's) material.
    pub material: String,
    /// The road it belongs to - written as the IfcRoad it is part of. Empty:
    /// an object of the site.
    pub facility: String,
    pub placement: RoadObjectPlacement,
    /// Only what was given; the rest are the kind's defaults when it is made,
    /// so changing a barrier's type= gives the new type's dimensions rather
    /// than keeping the old one's.
    pub parameters: Parameters,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoadObjectSet {
    pub objects: Vec<RoadObject>,
    /// The number the next object's id takes.
    pub next_number: u64,
}

impl Default for RoadObjectSet {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            next_number: 1,
        }
    }
}

impl RoadObjectSet {
    /// Case-insensitive; None when there is none.
    pub fn find(&self, id: &str) -> Option<&RoadObject> {
        self.objects.iter().find(|object| same_word(&object.id, id))
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut RoadObject> {
        self.objects
            .iter_mut()
            .find(|object| same_word(&object.id, id))
    }

    /// A new id for `kind`, taking the next number.
    pub fn take_id(&mut self, kind: RoadObjectKind) -> String {
        // Past any id a file edited by hand already holds.
        loop {
            let id = format!("{}-{}", kind_word(kind), self.next_number);
            self.next_number = self.next_number.wrapping_add(1);
            if self.find(&id).is_none() {
                return id;
            }
        }
    }
}

/// The source the heights of `placement` come from.
pub fn height_source_of(placement: &RoadObjectPlacement) -> HeightSource {
    if let Some(heights) = placement.heights {
        return heights;
    }
    if !placement.surface.is_empty() {
        return HeightSource::Surface;
    }
    if placement.kind == PlacementKind::Alignment {
        HeightSource::Profile
    } else {
        HeightSource::Line
    }
}

/// The layer `object` is made on.
pub fn layer_of(object: &RoadObject) -> String {
    if !object.layer.is_empty() {
        return object.layer.clone();
    }
    format!("road/objects/{}", kind_word(object.kind))
}

fn concrete_barrier(object: &RoadObject) -> bool {
    if object.kind != RoadObjectKind::Barrier {
        return false;
    }
    let barrier_type = text_of(&object.parameters, "type");
    barrier_type == "fshape" || barrier_type == "singleslope"
}

/// The material `object` is made of.
pub fn material_of(object: &RoadObject) -> String {
    if !object.material.is_empty() {
        return object.material.clone();
    }
    String::from(match object.kind {
        RoadObjectKind::Barrier => {
            if concrete_barrier(object) {
                "concrete"
            } else {
                "steel"
            }
        }
        RoadObjectKind::GuidePost => "plastic",
        RoadObjectKind::Sign => "aluminium",
        RoadObjectKind::LightPole | RoadObjectKind::Fence => "steel",
        RoadObjectKind::Culvert | RoadObjectKind::Wall => "concrete",
        RoadObjectKind::Marking => "thermoplastic",
    })
}

/// The colour `object` is drawn in.
pub fn colour_of(object: &RoadObject) -> Color {
    if let Some(colour) = object.colour {
        return colour;
    }
    // Colours a person tells apart in the 3D view: galvanised steel, grey
    // concrete, a white post, a green guide sign, line-marking white.
    let rgb = |r: u8, g: u8, b: u8| Color::new(r, g, b, 255);
    match object.kind {
        RoadObjectKind::Barrier => {
            if concrete_barrier(object) {
                rgb(189, 184, 174)
            } else {
                rgb(167, 173, 179)
            }
        }
        RoadObjectKind::GuidePost => rgb(244, 244, 244),
        RoadObjectKind::Sign => rgb(46, 125, 50),
        RoadObjectKind::LightPole => rgb(141, 148, 155),
        RoadObjectKind::Culvert => rgb(158, 154, 146),
        RoadObjectKind::Wall => rgb(181, 176, 166),
        RoadObjectKind::Fence => rgb(139, 115, 85),
        RoadObjectKind::Marking => rgb(250, 250, 250),
    }
}

/// The name `object` is shown by.
pub fn display_name(object: &RoadObject) -> String {
    if !object.name.is_empty() {
        return object.name.clone();
    }
    format!("{} {}", kind_title(object.kind), object.id)
}

/// Every parameter of the kind with the value it is made with: the given one,
/// else the default for the object's kind and type. "auto" ones are absent.
pub fn effective_parameters(object: &RoadObject) -> Parameters {
    let mut out = Parameters::new();
    let mut kind_type = String::new();
    let key = type_key(object.kind);
    if !key.is_empty() {
        kind_type = String::from(text_of(&object.parameters, key));
        if kind_type.is_empty()
            && let Some(spec) = find_parameter(object.kind, key)
        {
            kind_type = String::from(spec.default_text);
        }
    }
    for spec in parameters_of(object.kind) {
        if let Some(given) = object.parameters.get(spec.key) {
            out.insert(String::from(spec.key), given.clone());
            continue;
        }
        if let Some(value) = default_of(object.kind, &kind_type, spec) {
            out.insert(String::from(spec.key), value);
        }
    }
    out
}

// The refusal of a key that is only for an object placed along an alignment.
fn along_alignment(object: &RoadObject, key: &str) -> Status {
    if object.placement.kind != PlacementKind::Alignment {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{key}= is for an object along an alignment; this one is placed {}",
                if object.placement.kind == PlacementKind::Line {
                    "along a line"
                } else {
                    "at points"
                }
            ),
            object.id.as_str(),
        ));
    }
    Ok(())
}

/// One key=value onto `object`: the keys every object takes - name, layer,
/// colour (#RRGGBB or a standard colour name; empty for the kind's),
/// material, facility - the placement's - alignment (an alignment
/// placement's), station, to, offset, spacing, heights, surface, crossfall, dz
/// (to=, spacing= and heights= empty clear them; offset=, crossfall= and dz=
/// empty go back to 0) - and the kind's own (parameters_of; empty clears it
/// back to the default). Refused, naming the key: a key neither takes, a value
/// that does not read, station=/to=/offset=/alignment= on a placement that is
/// not along an alignment, spacing= on a kind that is not a point kind or a
/// placement at points, crossfall= on a placement that is not along an
/// alignment, and station= or alignment= given empty (a placement along an
/// alignment has to say where).
pub fn apply_key(object: &mut RoadObject, key: &str, value: &str) -> Status {
    let key_word = String::from_utf8_lossy(&lowered(key)).into_owned();
    let number = |low: f64, high: f64| -> Result<f64> {
        let Some(parsed) = parse_finite_double(trimmed(value.as_bytes())) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("{key_word}= must be a number"),
                value,
            ));
        };
        if parsed < low || parsed > high {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("{key_word}= must be from {} to {}", real(low), real(high)),
                value,
            ));
        }
        Ok(parsed)
    };
    // A station, an offset: anywhere on a project, as a level is.
    const FAR: f64 = 1e7;
    match key_word.as_str() {
        "name" => {
            object.name = String::from(value);
            return Ok(());
        }
        "layer" => {
            if !value.is_empty() {
                validate_layer_path(value)?;
            }
            object.layer = String::from(value);
            return Ok(());
        }
        "colour" | "color" => {
            if value.is_empty() {
                object.colour = None;
                return Ok(());
            }
            if value.starts_with('#') {
                object.colour = Some(Color::from_hex(value)?);
                return Ok(());
            }
            let Some(named) = standard_colour(value) else {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "colour= is #RRGGBB or a standard colour name (red, dark green ...)",
                    value,
                ));
            };
            object.colour = Some(named);
            return Ok(());
        }
        "material" => {
            object.material = String::from(value);
            return Ok(());
        }
        "facility" => {
            object.facility = String::from(value);
            return Ok(());
        }
        "alignment" => {
            along_alignment(object, &key_word)?;
            if value.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "alignment= needs a name",
                ));
            }
            object.placement.alignment = String::from(value);
            return Ok(());
        }
        "station" | "offset" => {
            along_alignment(object, &key_word)?;
            let station = key_word == "station";
            if value.is_empty() {
                // An offset goes back to the centreline; a station has no
                // default to go back to - where along the alignment is the
                // whole point of the placement.
                if station {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "station= needs a number",
                        object.id.as_str(),
                    ));
                }
                object.placement.offset = RoadObjectPlacement::default().offset;
                return Ok(());
            }
            let parsed = number(-FAR, FAR)?;
            if station {
                object.placement.station = parsed;
            } else {
                object.placement.offset = parsed;
            }
            return Ok(());
        }
        "to" => {
            along_alignment(object, &key_word)?;
            if value.is_empty() {
                object.placement.to = None;
                return Ok(());
            }
            object.placement.to = Some(number(-FAR, FAR)?);
            return Ok(());
        }
        "spacing" => {
            if value.is_empty() {
                object.placement.spacing = None;
                return Ok(());
            }
            // Read by the point kinds placed along an alignment or a line and
            // by nothing else; a barrier's and a fence's posts have
            // postspacing=. Accepted anywhere else it would be echoed in the
            // reply and ignored.
            if shape_of(object.kind) != KindShape::Point {
                let posts =
                    object.kind == RoadObjectKind::Barrier || object.kind == RoadObjectKind::Fence;
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "spacing= is for guide posts, signs and light poles; a {} {}",
                        kind_word(object.kind),
                        if posts {
                            "takes postspacing= for its posts"
                        } else {
                            "has nothing to space"
                        }
                    ),
                    object.id.as_str(),
                ));
            }
            if object.placement.kind == PlacementKind::Points {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "spacing= places a run along an alignment or a line; at points one stands at \
                     each point",
                    object.id.as_str(),
                ));
            }
            let parsed = number(0.0, FAR)?;
            if !(parsed > 0.0) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "spacing= must be more than 0",
                    value,
                ));
            }
            object.placement.spacing = Some(parsed);
            return Ok(());
        }
        "heights" => {
            if value.is_empty() {
                object.placement.heights = None;
                return Ok(());
            }
            object.placement.heights = Some(height_from_word(value)?);
            return Ok(());
        }
        "surface" => {
            object.placement.surface = String::from(value);
            return Ok(());
        }
        "crossfall" => {
            if value.is_empty() {
                object.placement.crossfall = RoadObjectPlacement::default().crossfall;
                return Ok(());
            }
            // Percent: a road's crossfall is a few percent, a verge's up to a
            // batter's 50 (1 on 2).
            let parsed = number(-50.0, 50.0)?;
            // The profile is only along an alignment; an object placed along a
            // line or at points never reads it. (An alignment object that
            // stands on a surface is refused where it is made, Placement.)
            if parsed != 0.0 && along_alignment(object, &key_word).is_err() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "crossfall= carries the design profile out to the offset; this object has no \
                     profile to carry",
                    object.id.as_str(),
                ));
            }
            object.placement.crossfall = parsed;
            return Ok(());
        }
        "dz" => {
            if value.is_empty() {
                object.placement.dz = RoadObjectPlacement::default().dz;
                return Ok(());
            }
            object.placement.dz = number(-1000.0, 1000.0)?;
            return Ok(());
        }
        _ => {}
    }
    let spec = find_parameter(object.kind, &key_word);
    if let Some(spec) = spec
        && value.is_empty()
    {
        object.parameters.remove(spec.key);
        return Ok(());
    }
    let parsed = parse_parameter(object.kind, &key_word, value)?;
    if let Some(spec) = spec {
        object.parameters.insert(String::from(spec.key), parsed);
    }
    Ok(())
}

// ---- making the objects ---------------------------------------------------------------------

/// What making an object looks up an alignment by.
pub type AlignmentLookup<'a> = Box<dyn Fn(&str) -> Option<&'a Alignment> + 'a>;
/// What making an object looks up a surface by.
pub type SurfaceLookup<'a> = Box<dyn Fn(&str) -> Option<&'a TinSurface> + 'a>;

/// What making an object looks up: an alignment by name, a surface by name.
/// Either may answer None, and either may be missing (the C++'s empty
/// std::function), which answers None for every name.
#[derive(Default)]
pub struct PlacementContext<'a> {
    pub alignment: Option<AlignmentLookup<'a>>,
    pub surface: Option<SurfaceLookup<'a>>,
}

/// The document's alignments and surfaces (Document::surfaces).
pub fn placement_context(document: &Document) -> PlacementContext<'_> {
    PlacementContext {
        alignment: Some(Box::new(move |name: &str| {
            document.model().alignments.find(name)
        })),
        surface: Some(Box::new(move |name: &str| {
            document
                .surfaces()
                .find(name)
                .and_then(|found| found.surface.as_deref())
        })),
    }
}

// The objects of `object`, or why it could not be made.
fn make_objects(context: &PlacementContext<'_>, object: &RoadObject) -> Made<Vec<BimObject>> {
    let placement = Placement::new(context, object)?;
    let objects = match object.kind {
        RoadObjectKind::Barrier => make_barrier(object, &placement)?,
        RoadObjectKind::GuidePost => make_guide_posts(object, &placement)?,
        RoadObjectKind::Sign => make_signs(object, &placement)?,
        RoadObjectKind::LightPole => make_lights(object, &placement)?,
        RoadObjectKind::Culvert => make_culvert(object, &placement)?,
        RoadObjectKind::Wall => make_wall(object, &placement)?,
        RoadObjectKind::Fence => make_fence(object, &placement)?,
        RoadObjectKind::Marking => make_marking(object, &placement)?,
    };
    if let Some(why) = shape_problem(&objects) {
        return fail(why);
    }
    Ok(objects)
}

/// The 3D objects of one road object, in order, or nothing and the problem
/// that stopped it (source "object <id>"). Deterministic: the same definition
/// in the same drawing gives the same objects, ids and all.
pub fn build_road_object(context: &PlacementContext<'_>, object: &RoadObject) -> BimModel {
    let mut model = BimModel::default();
    match make_objects(context, object) {
        Ok(objects) => model.objects = objects,
        Err(problem) => model.problems.push(BimProblem {
            source: format!("object {}", object.id),
            message: problem.message,
        }),
    }
    model
}

/// Why `parts` - the solids a maker gave for one object - are not an object,
/// as the sentence build_road_object lists it with; None when there is at
/// least one part and every part has faces. No part at all is a placement
/// that gives nowhere to stand the object; a part with no faces is a section
/// that did not triangulate or a sweep of a single point, which would be
/// counted and written to IFC as an element with no shape. A function of plain
/// data, so a test gives it the empty part that no maker makes any more and
/// sees the sentence.
pub fn shape_problem(parts: &[BimObject]) -> Option<String> {
    if parts.is_empty() {
        return Some(String::from("its placement gives nowhere to stand it"));
    }
    // A solid with no faces is one that could not be made - a section that
    // does not triangulate, a sweep of a single point - and would be counted
    // and written to IFC as an element with no shape.
    parts
        .iter()
        .find(|part| part.mesh.faces.is_empty())
        .map(|part| {
            format!(
                "{} could not be made into a solid: its placement or its dimensions give it no \
                 shape",
                part.id
            )
        })
}

/// What an edit asks of `built`, build_road_object's answer for `object`,
/// before it stores the definition: a definition the model would only list as
/// a problem is never stored. InvalidArgument naming the first problem it was
/// left with (and, for an object placed along a line, the line: "barrier-3
/// (#12)"), or the part that is not a closed solid (is_closed_mesh,
/// road_object_mesh.rs) - the IFC export writes each solid as a closed face
/// set and its volume is read off the triangles, so one that does not close is
/// refused here, once per edit, and not checked on every rebuild of the model.
/// Also plain data in, so a test hands it the open mesh no maker makes any
/// more.
pub fn check_built(object: &RoadObject, built: &BimModel) -> Status {
    if let Some(first) = built.problems.first() {
        let mut place = object.id.clone();
        if object.placement.source != INVALID_ENTITY_ID {
            place = format!("{place} (#{})", object.placement.source);
        }
        return Err(make_error(
            ErrorCode::InvalidArgument,
            first.message.as_str(),
            place,
        ));
    }
    for part in &built.objects {
        if !is_closed_mesh(&part.mesh) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "{} is not a closed solid; its placement or its dimensions make a shape that \
                     does not close",
                    part.id
                ),
                object.id.as_str(),
            ));
        }
    }
    Ok(())
}

// A part that stands under or hangs from the one its object is named for: a
// wall's footing runs the wall's whole length, a light pole's luminaire is the
// pole's own. Each says its own Length or Count (IFC reads them off the
// element), but they measure the same run, or the same pole, a second time.
fn is_supporting_part(part: &BimObject) -> bool {
    part.ifc_class == "IfcFooting" || part.ifc_class == "IfcLightFixture"
}

// The quantities that count a thing once however many parts carry it.
fn counted_once(name: &str) -> bool {
    name == "Length" || name == "Count" || name == "GrossArea"
}

/// The quantities of `objects` together, by name in the order first met: the
/// volumes and weights summed, a height, width or depth the largest. A Length,
/// a Count and a GrossArea are those of the objects themselves, not of what
/// stands under or hangs from them: a wall's footing (IfcFooting) and a light
/// pole's luminaire (IfcLightFixture) say their own, as IFC reads them off the
/// element, but add nothing here - a 50 m wall is 50 m long however deep its
/// footing, and three poles are three however many lanterns they carry.
pub fn total_quantities(objects: &[BimObject]) -> Vec<Quantity> {
    let mut totals: Vec<Quantity> = Vec::new();
    for object in objects {
        for quantity in &object.quantities {
            if is_supporting_part(object) && counted_once(&quantity.name) {
                continue;
            }
            let Some(at) = totals.iter().position(|total| total.name == quantity.name) else {
                totals.push(quantity.clone());
                continue;
            };
            let found = &mut totals[at];
            // A height, a width or a depth is the largest; the rest add up.
            if quantity.name == "Height" || quantity.name == "Width" || quantity.name == "Depth" {
                found.value = cpp::max(found.value, quantity.value);
            } else {
                found.value += quantity.value;
            }
        }
    }
    totals
}

/// A culvert's two ends, inlet first, at its inverts: where build_road_object
/// puts the barrel, for a reply and a test.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CulvertEnds {
    pub inlet: Point3,
    pub outlet: Point3,
}

/// InvalidArgument for any kind but a culvert; InvalidGeometry with whatever
/// the placement cannot give otherwise.
pub fn culvert_ends(context: &PlacementContext<'_>, object: &RoadObject) -> Result<CulvertEnds> {
    if object.kind != RoadObjectKind::Culvert {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "not a culvert",
            object.id.as_str(),
        ));
    }
    Placement::new(context, object)
        .and_then(|placement| resolve_culvert(object, &placement))
        .map_err(|problem| {
            make_error(
                ErrorCode::InvalidGeometry,
                problem.message,
                object.id.as_str(),
            )
        })
}

// ---- JSON (the drawing's "road_objects" metadata, ROADOBJECT LIST JSON) ---------------------

/// Version 1. A newer version is refused with Unsupported, never read by
/// guesswork.
pub const ROAD_OBJECTS_VERSION: i32 = 1;
