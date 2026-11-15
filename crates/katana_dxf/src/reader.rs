//! A DXF file -> entities, layers and linetypes, with no third-party library.
//!
//! WHY A READER OF OUR OWN. DXF used to come in through GDAL's vector driver,
//! which reads a drawing as GIS features and loses what a CAD user exchanges a
//! DXF for: an arc became a 9-vertex polyline, a circle an OPEN polyline, text
//! a point. This module reads the entities themselves, so an ARC is an Arc.
//! It sits where the archive reader does - the entity model below it (and
//! commands, for import_command), no third-party library.
//!
//! WHERE EACH ENTITY GOES.
//!
//! ```text
//!   LINE                a Line. One of zero length, a Point: some programs
//!                       draw a surveyed dot that way, and dropping it loses
//!                       the dot.
//!   ARC, CIRCLE         an Arc, a Circle.
//!   LWPOLYLINE,         a Polyline, closed flag kept. Arc segments (bulges)
//!   POLYLINE (2D, 3D)   are kept as a CurvePolyline2 where the frame keeps a
//!                       circle a circle, else chorded to `curve_tolerance`,
//!                       each one chorded counted. Heights: a 3D polyline's
//!                       per-vertex Z, and a 2D one's elevation, go through
//!                       set_heights.
//!   POINT               a Point; a non-zero Z is its elevation.
//!   TEXT, MTEXT         Text: string, height, rotation. MTEXT formatting
//!                       codes are stripped (plain_mtext) and each paragraph
//!                       becomes a Text of its own, because a Text is one line.
//!   INSERT, MINSERT     the block's entities, expanded (nested blocks too),
//!                       with the insert's attributes as properties of every
//!                       entity it produced. A visible attribute is also a Text.
//!   DIMENSION           its picture block, expanded: lines, arrows and text.
//!   ATTDEF              nothing: it is the template the ATTRIB replaced.
//!   anything else       counted in the tally as read and not imported.
//! ```
//!
//! Paper space entities are counted and not imported: a Katana drawing is the
//! model. Entities in a tilted plane (an extrusion direction that is not +Z or
//! -Z) are projected onto the plan, circles and arcs as chorded polylines, and
//! counted.
//!
//! NEVER FAILS ON CONTENT. An entity that cannot be represented is a warning
//! and a tally, because one bad entity must not cost the user the other ten
//! thousand. It fails only when the input is not a DXF file at all (no
//! SECTION), is a binary DXF, or a group code line is not a number - past that
//! point the pairs cannot be told apart and nothing after it can be trusted.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text::{format_exact_real, parse_finite_double, trimmed};
use katana_core::text_encoding::{TextEncoding, decode_text_as, utf8_or_windows1252};
use katana_entity::entity::{
    ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, heights_of, property_to_string, set_heights,
};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::layer_path::{MAXIMUM_LAYER_DEPTH, validate_layer_path};
use katana_entity::tables::{CONTINUOUS_LINETYPE, DEFAULT_LAYER_NAME};
use katana_entity::{
    Color, Entity, Geometry, Layer, Linetype, LinetypeElement, PointGeometry, PropertyMap,
    PropertyValue, TextGeometry, validate,
};
use katana_geometry::chording::{chord_arc, chord_circle, sagitta_chord_count};
use katana_geometry::{
    Arc2, Box2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2,
    Spline2, Vec2,
};
use katana_math::numerics::{TWO_PI, normalize_angle, tolerance};

use crate::codes::{
    COLOUR_BY_BLOCK, COLOUR_BY_LAYER, RADIANS_PER_DEGREE, indexed_colour, plain_mtext, plain_text,
    true_colour,
};
use crate::tokenizer::{Pair, Tokenizer, to_integer, to_real, trimmed_value};
use katana_math::cpp;

// ---- the public interface -------------------------------------------------------

/// Metadata keys this reader writes and the writer reads back.
pub const META_SOURCE: &str = "source";
/// The block an entity was expanded from, outermost first ("SITE/TREE").
pub const META_BLOCK: &str = "dxf.block";
/// An entity's own linetype and lineweight (hundredths of a millimetre) where
/// they are not ByLayer. Katana has no per-entity field for either yet.
pub const META_LINETYPE: &str = "dxf.linetype";
pub const META_LINEWEIGHT: &str = "dxf.lineweight";
/// The registered application name of the extended data this module writes
/// and reads, for what the format's own groups cannot hold: a layer's full
/// Katana path (a DXF layer name cannot hold a "/"), the heights of an entity
/// known at only some of its vertices (the format has no "no height"), and a
/// colour no index is (R2000 has no true colour). Each follows its word: the
/// path its text, the heights the `elevations` list's text in pieces, the
/// colour "#RRGGBB".
pub const APPLICATION_NAME: &str = "KATANA";
pub const EXTENDED_PATH: &str = "path";
pub const EXTENDED_HEIGHTS: &str = "elevations";
pub const EXTENDED_COLOUR: &str = "colour";

#[derive(Clone, Debug, PartialEq)]
pub struct ImportOptions {
    /// Largest distance a chord may stand off the arc it replaces, in model
    /// units - a polyline's bulges, and a circle or arc in a tilted plane.
    /// Stated rather than hidden: it is a lossy step.
    pub curve_tolerance: f64,
    /// Subtracted from every plan coordinate. See the interop layer's vector
    /// import options.
    pub origin_shift: Option<Vec2>,
    /// Written into every entity's metadata as `source`; empty writes nothing.
    pub source_name: String,
    /// Blocks inside blocks deeper than this are not expanded, and said so. A
    /// block that inserts itself is caught separately, at any depth.
    pub maximum_block_depth: usize,
    /// Stops a hostile or broken file (a MINSERT of a billion copies) from
    /// exhausting memory: expansion ends there, with a warning.
    pub maximum_entities: usize,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            curve_tolerance: 0.001,
            origin_shift: None,
            source_name: String::new(),
            maximum_block_depth: 16,
            maximum_entities: 20_000_000,
        }
    }
}

/// How many of one DXF entity kind the file held, and how many entities the
/// import made from them (an INSERT makes many, a skipped kind none).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityTally {
    pub kind: String,
    pub read: usize,
    pub imported: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfImport {
    pub entities: Vec<Entity>,
    /// The LAYER table in file order, then any layer an entity names that the
    /// table lacks (with the defaults), so the caller creates exactly these.
    pub layers: Vec<Layer>,
    /// The LTYPE table's patterns that Katana can draw, in file order. The
    /// built-in ByLayer, ByBlock and Continuous are not listed.
    pub linetypes: Vec<Linetype>,
    pub bounds: Box2,
    /// $ACADVER ("AC1015") and the release it names ("R2000"); empty when the
    /// file has no header, as a minimal R12 file need not.
    pub version: String,
    pub release: String,
    pub tally: Vec<EntityTally>,
    /// Arc segments of polylines replaced by chords.
    pub arcs_chorded: usize,
    pub warnings: Vec<String>,
}

// ---- the reader -----------------------------------------------------------------

/// The one conversion of a file's degrees; exact_degrees (codes.rs) is chosen
/// against it.
const DEGREES: f64 = RADIANS_PER_DEGREE;
/// A text a person reads is one line; MTEXT's default line pitch is five thirds
/// of the text height.
const LINE_PITCH: f64 = 5.0 / 3.0;
/// The width of a character as a fraction of its height, for the few places a
/// text's extent must be guessed without a font: the entity layer's figure.
const GLYPH_ASPECT: f64 = 0.6;

fn upper(text: &str) -> String {
    text.to_ascii_uppercase()
}

/// The C++'s equalsUpper: `text` with a-z raised is `upper_word`, which is
/// upper case already.
fn equals_upper(text: &str, upper_word: &str) -> bool {
    text.eq_ignore_ascii_case(upper_word)
}

/// The C++ core::trimmed on text: ASCII blanks off both ends.
fn trimmed_text(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r'])
}

#[derive(Clone, Copy, Debug, Default)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

fn cross(a: &Vec3, b: &Vec3) -> Vec3 {
    Vec3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn normalised(v: &Vec3) -> Vec3 {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if !(length > 0.0) || !length.is_finite() {
        return Vec3 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
    }
    Vec3 {
        x: v.x / length,
        y: v.y / length,
        z: v.z / length,
    }
}

/// x' = a x + b y + tx, y' = c x + d y + ty: where an entity's own coordinates
/// land in plan. An object coordinate system, a block insertion, or both.
#[derive(Clone, Copy, Debug)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

impl Affine {
    fn apply(&self, p: Point2) -> Point2 {
        Point2::new(
            self.a * p.x + self.b * p.y + self.tx,
            self.c * p.x + self.d * p.y + self.ty,
        )
    }

    fn linear(&self, v: Vec2) -> Vec2 {
        Vec2::new(self.a * v.x + self.b * v.y, self.c * v.x + self.d * v.y)
    }

    fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    fn is_identity(&self) -> bool {
        self.a == 1.0
            && self.b == 0.0
            && self.c == 0.0
            && self.d == 1.0
            && self.tx == 0.0
            && self.ty == 0.0
    }

    /// Rotation, uniform scale and perhaps a mirror: a circle stays a circle.
    fn is_similarity(&self) -> bool {
        let first = self.a * self.a + self.c * self.c;
        let second = self.b * self.b + self.d * self.d;
        let dot = self.a * self.b + self.c * self.d;
        let scale = cpp::max(first, second);
        scale > 0.0 && (first - second).abs() <= 1e-9 * scale && dot.abs() <= 1e-9 * scale
    }

    fn largest_scale(&self) -> f64 {
        cpp::max(
            self.a * self.a + self.c * self.c,
            self.b * self.b + self.d * self.d,
        )
        .sqrt()
    }

    /// This after `inner`: inner is applied first.
    fn after(&self, inner: &Affine) -> Affine {
        Affine {
            a: self.a * inner.a + self.b * inner.c,
            b: self.a * inner.b + self.b * inner.d,
            c: self.c * inner.a + self.d * inner.c,
            d: self.c * inner.b + self.d * inner.d,
            tx: self.a * inner.tx + self.b * inner.ty + self.tx,
            ty: self.c * inner.tx + self.d * inner.ty + self.ty,
        }
    }
}

/// The object coordinate system of an entity whose extrusion direction is
/// `normal`, as a map onto the plan, by the format's arbitrary axis algorithm.
/// `z` is the entity's elevation in that system, which moves it in plan only
/// when the plane is tilted.
fn object_to_plan(normal_in: &Vec3, z: f64) -> Affine {
    let normal = normalised(normal_in);
    const ARBITRARY_AXIS_LIMIT: f64 = 1.0 / 64.0;
    let x_axis = normalised(&if normal.x.abs() < ARBITRARY_AXIS_LIMIT
        && normal.y.abs() < ARBITRARY_AXIS_LIMIT
    {
        cross(
            &Vec3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            &normal,
        )
    } else {
        cross(
            &Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            &normal,
        )
    });
    let y_axis = normalised(&cross(&normal, &x_axis));
    Affine {
        a: x_axis.x,
        b: y_axis.x,
        c: x_axis.y,
        d: y_axis.y,
        tx: z * normal.x,
        ty: z * normal.y,
    }
}

fn is_plain_extrusion(normal: &Vec3) -> bool {
    normal.x == 0.0 && normal.y == 0.0 && normal.z > 0.0
}

/// An object coordinate system's plane is the plan's, facing up or down, when
/// its map onto the plan keeps shapes (object_to_plan of a tilted plane squashes
/// one axis). Then all of it is at one level, and that level means something.
fn is_level(normal: &Vec3, frame: &Affine) -> bool {
    is_plain_extrusion(normal) || frame.is_similarity()
}

/// The height of elevation `z` in a level object coordinate system: z runs
/// along the extrusion, so its height is z times the extrusion's Z - the same
/// z for the plain (0,0,1), and -z for a system facing down, (0,0,-1), in
/// which the writing program put a circle at height 10 as elevation -10.
fn height_of(normal: &Vec3, z: f64) -> f64 {
    z * normalised(normal).z
}

fn sweep_between(start_degrees: f64, end_degrees: f64) -> f64 {
    let mut sweep = (end_degrees - start_degrees) % 360.0;
    if sweep <= 0.0 {
        sweep += 360.0;
    }
    sweep * DEGREES
}

/// The arc a bulge describes from `from` to `to`: bulge = tan(angle / 4),
/// positive counter-clockwise. None for a straight segment.
fn bulge_arc(from: Point2, to: Point2, bulge: f64) -> Option<Arc2> {
    if bulge == 0.0 || !bulge.is_finite() {
        return None;
    }
    let chord = to - from;
    let length = chord.length();
    if !(length > tolerance::GEOMETRIC) {
        return None;
    }
    // Centre off the chord's midpoint along its left normal by
    // (length / 2) (1 - b^2) / (2 b): at b = 1 (a semicircle) the midpoint,
    // small b far to the left, b > 1 (more than half a turn) to the right.
    let unit = chord / length;
    let left = Vec2::new(-unit.y, unit.x);
    let middle = from + chord * 0.5;
    let centre = middle + left * (length * 0.5 * (1.0 - bulge * bulge) / (2.0 * bulge));
    let radius = length * 0.25 * (1.0 + bulge * bulge) / bulge.abs();
    let start = from - centre;
    Some(Arc2::new(
        centre,
        radius,
        start.y.atan2(start.x),
        4.0 * bulge.atan(),
    ))
}

/// One vertex of a polyline in its own coordinates.
#[derive(Clone, Copy, Debug, Default)]
struct Vertex {
    x: f64,
    y: f64,
    z: f64,
    bulge: f64,
}

/// Everything most entities carry: group 8, 62, 420, 6, 370, 60, 67, 210-230.
struct Common<'a> {
    layer: &'a str,
    colour: Option<i32>,
    true_colour: Option<i64>,
    linetype: &'a str,
    lineweight: Option<i32>,
    invisible: bool,
    #[allow(
        dead_code,
        reason = "the C++ keeps group 67 here too and reads it nowhere: convert decides paper space on the record's pairs"
    )]
    paper_space: bool,
    extrusion: Vec3,
    // Extended data: the application it belongs to, the word of this
    // module's that the values now follow, and what they said - the heights
    // list joined from its pieces (see EXTENDED_HEIGHTS), the colour.
    application: &'a str,
    extended_word: &'a str,
    heights: String,
    exact_colour: Option<Color>,
}

impl Default for Common<'_> {
    fn default() -> Self {
        Self {
            layer: "0",
            colour: None,
            true_colour: None,
            linetype: "",
            lineweight: None,
            invisible: false,
            paper_space: false,
            extrusion: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            application: "",
            extended_word: "",
            heights: String::new(),
            exact_colour: None,
        }
    }
}

impl<'a> Common<'a> {
    /// The heights this module's writer put beside the entity, for `count`
    /// vertices; empty when there are none, or not that many. One height
    /// alone stands for every vertex, as the `elevation` property does: it is
    /// how a level of exactly 0 goes out (see the writer's finish).
    fn extended_heights(&self, count: usize) -> Vec<Option<f64>> {
        if self.heights.is_empty() {
            return Vec::new();
        }
        let mut list = PropertyMap::new();
        list.insert(
            String::from(ELEVATIONS_PROPERTY),
            PropertyValue::Text(self.heights.clone()),
        );
        if !self.heights.contains(' ')
            && let Some(one) = parse_finite_double(&self.heights)
        {
            list.insert(String::from(ELEVATION_PROPERTY), PropertyValue::Real(one));
        }
        let parsed = heights_of(&list, count);
        if parsed.iter().any(Option::is_some) {
            parsed
        } else {
            Vec::new()
        }
    }

    /// True when the pair was one of these.
    fn take(&mut self, pair: &Pair<'a>) -> bool {
        match pair.code {
            1001 => {
                self.application = trimmed_value(pair.value);
                self.extended_word = "";
                true
            }
            1000 => {
                if self.application == APPLICATION_NAME {
                    let value = trimmed_value(pair.value);
                    if value == EXTENDED_HEIGHTS || value == EXTENDED_COLOUR {
                        self.extended_word = value;
                        if value == EXTENDED_HEIGHTS {
                            self.heights.clear();
                        }
                    } else if self.extended_word == EXTENDED_HEIGHTS {
                        if !self.heights.is_empty() {
                            self.heights.push(' ');
                        }
                        self.heights.push_str(value);
                    } else if self.extended_word == EXTENDED_COLOUR {
                        if let Ok(parsed) = Color::from_hex(value) {
                            self.exact_colour = Some(parsed);
                        }
                        self.extended_word = "";
                    }
                }
                true
            }
            1002 => true,
            8 => {
                self.layer = trimmed_value(pair.value);
                true
            }
            62 => {
                self.colour = to_integer(pair.value)
                    .map(|value| value as i32)
                    .or(self.colour);
                true
            }
            420 => {
                self.true_colour = to_integer(pair.value).or(self.true_colour);
                true
            }
            6 => {
                self.linetype = trimmed_value(pair.value);
                true
            }
            370 => {
                self.lineweight = to_integer(pair.value)
                    .map(|value| value as i32)
                    .or(self.lineweight);
                true
            }
            60 => {
                self.invisible = to_integer(pair.value).unwrap_or(0) == 1;
                true
            }
            67 => {
                self.paper_space = to_integer(pair.value).unwrap_or(0) == 1;
                true
            }
            210 => {
                self.extrusion.x = to_real(pair.value).unwrap_or(0.0);
                true
            }
            220 => {
                self.extrusion.y = to_real(pair.value).unwrap_or(0.0);
                true
            }
            230 => {
                self.extrusion.z = to_real(pair.value).unwrap_or(1.0);
                true
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Record<'a> {
    record_type: &'a str,
    pairs: Vec<Pair<'a>>,
    // VERTEX records of a POLYLINE, ATTRIB records of an INSERT. Only the
    // first child_count are this record's: the vector is reused.
    children: Vec<Record<'a>>,
    child_count: usize,
}

impl<'a> Record<'a> {
    fn clear(&mut self) {
        self.pairs.clear();
        self.child_count = 0;
    }

    fn add_child(&mut self) -> &mut Record<'a> {
        if self.child_count == self.children.len() {
            self.children.push(Record::default());
        }
        let child = &mut self.children[self.child_count];
        self.child_count += 1;
        child.clear();
        child
    }

    fn own_children(&self) -> &[Record<'a>] {
        &self.children[..self.child_count]
    }
}

/// A block's entity, in the block's coordinates, remembering what it takes
/// from the insert that places it.
struct BlockEntity {
    entity: Entity,
    layer_from_insert: bool,  // it is on layer 0
    colour_from_insert: bool, // its colour is ByBlock
    // Drawn at the block's Z 0, so it has no heights of its own - and is at
    // whatever level the insert puts the block's base: a survey symbol is
    // drawn at 0 and inserted at the point's level. False where it has
    // heights, and where no one height means anything (a tilted plane).
    at_block_zero: bool,
}

/// How an insert moves its block's heights: a block Z times `scale`, plus
/// `offset`. None where the insert's plane is tilted, and no one height is
/// right for what it places.
#[derive(Clone, Copy, Debug)]
struct HeightMap {
    scale: f64,
    offset: f64,
}

#[derive(Default)]
struct Block<'a> {
    name: String,
    base: Point2,
    base_z: f64,
    flags: i32,
    records: Vec<Record<'a>>,
    // Filled the first time the block is inserted, then reused: a survey
    // drawing inserts one symbol thousands of times.
    expanded: Vec<BlockEntity>,
    built: bool,
    building: bool,
}

/// Where the entities being converted are going: straight into the drawing,
/// or into a block's expansion, which is then placed insert by insert. The
/// C++ points at the block's own vector; here the sink holds the expansion
/// while it is built, and the block takes it when it is done, so that the
/// block table is free to be read while one of its blocks is building.
#[derive(Default)]
struct Sink<'a> {
    block: Option<Vec<BlockEntity>>, // None: the drawing
    kind: &'a str,                   // the tally the output counts to
    depth: usize,                    // of block nesting
    block_path: String,              // metadata of what it produces
}

struct Reader<'a> {
    tokens: Tokenizer<'a>,
    options: &'a ImportOptions,
    result: DxfImport,
    default_text_height: f64,
    code_page: String,
    // The C++ keeps each layer's DXF name beside it, and reads it nowhere.
    layers: Vec<Layer>,
    layer_index: HashMap<String, usize>, // upper-case DXF name
    last_layer_name: String,
    last_layer: usize,
    have_last_layer: bool,
    linetype_names: HashMap<String, String>, // upper-case -> Katana
    blocks: Vec<Block<'a>>,
    block_index: HashMap<String, usize>, // upper-case name
    tally_index: HashMap<String, usize>,
    warning_counts: BTreeMap<String, usize>,
    full_reported: bool,
}

/// A layer name as a Katana layer path. One that is already a valid path is
/// kept - a "/" in it nests, which is how a file written by an older export
/// brings its tree back. Otherwise it is mended level by level: a control
/// character becomes "_", the blanks around a level go, and a level that is
/// then empty, "." or ".." becomes "_"; past the depth a path may have, the
/// levels are joined with "_" instead. Never an invalid path: "_" at worst.
fn path_for_layer_name(name: &str) -> String {
    if name.is_empty() {
        return String::from(DEFAULT_LAYER_NAME);
    }
    if validate_layer_path(name).is_ok() {
        return name.to_owned();
    }
    let mut levels: Vec<String> = Vec::new();
    for piece in name.split('/') {
        // A control byte (below 0x20, or 0x7F) is a character of its own.
        let mut level: String = trimmed_text(piece)
            .chars()
            .map(|c| if c.is_ascii_control() { '_' } else { c })
            .collect();
        if level.is_empty() || level == "." || level == ".." {
            level = format!("_{level}");
        }
        levels.push(level);
    }
    let joiner = if levels.len() > MAXIMUM_LAYER_DEPTH {
        "_"
    } else {
        "/"
    };
    let fixed = levels.join(joiner);
    if validate_layer_path(&fixed).is_ok() {
        return fixed;
    }
    String::from("_") // too long: nothing of it can be kept safely
}

/// A DXF pattern as Katana's: runs of dashes (or of gaps) merged, the ends
/// joined when they are the same kind, and started on a dash - DXF allows a
/// pattern to begin with a gap or hold two dashes together, Katana's must
/// alternate. Empty for a pattern that is all dash (continuous) or all gap.
fn normalised_pattern(lengths: &[f64]) -> Vec<LinetypeElement> {
    let mut merged: Vec<LinetypeElement> = Vec::new();
    for &length in lengths {
        if !length.is_finite() {
            return Vec::new();
        }
        let down = length >= 0.0;
        if let Some(last) = merged.last_mut()
            && last.is_pen_down() == down
        {
            // Two pen-down runs make a dash (a dot and a dash, a dash); two
            // gaps a longer gap.
            last.length += length;
            continue;
        }
        merged.push(LinetypeElement { length });
    }
    if merged.len() >= 2 && merged[0].is_pen_down() == merged[merged.len() - 1].is_pen_down() {
        let last = merged.pop().map_or(0.0, |element| element.length);
        merged[0].length += last;
    }
    let any_gap = merged.iter().any(LinetypeElement::is_gap);
    let any_dash = merged.iter().any(LinetypeElement::is_pen_down);
    if !any_gap || !any_dash {
        return Vec::new();
    }
    if merged[0].is_gap() {
        merged.rotate_left(1);
    }
    merged
}

/// The curve through `frame` into plan. A similarity keeps it a circle or arc
/// (a mirror reverses the arc's direction); anything else - a tilted plane, a
/// block scaled unevenly - makes it an ellipse, which is chorded.
fn place_curve(arc: &Arc2, full: bool, frame: &Affine, tolerance: f64) -> Geometry {
    if frame.is_identity() {
        return if full {
            Geometry::Circle(Circle2::new(arc.center, arc.radius))
        } else {
            Geometry::Arc(*arc)
        };
    }
    if frame.is_similarity() {
        let scale = frame.determinant().abs().sqrt();
        let centre = frame.apply(arc.center);
        if full {
            return Geometry::Circle(Circle2::new(centre, arc.radius * scale));
        }
        let from = if frame.determinant() < 0.0 {
            arc.end_angle()
        } else {
            arc.start_angle
        };
        let direction = frame.linear(Vec2::new(from.cos(), from.sin()));
        return Geometry::Arc(Arc2::new(
            centre,
            arc.radius * scale,
            direction.y.atan2(direction.x),
            arc.sweep,
        ));
    }
    let scale = frame.largest_scale();
    let local = if scale > 0.0 {
        tolerance / scale
    } else {
        tolerance
    };
    let mut points = if full {
        chord_circle(&Circle2::new(arc.center, arc.radius), local)
    } else {
        chord_arc(arc, local)
    };
    for point in &mut points {
        *point = frame.apply(*point);
    }
    Geometry::Polyline(Polyline2::new(points, full))
}

/// Where a text's left baseline is, from a justified anchor: a guess at its
/// width without a font, and so used only when the file gives nothing better.
/// The width counts bytes, as the C++'s std::string::size does.
fn left_baseline_from(
    anchor: Point2,
    text: &str,
    height: f64,
    rotation: f64,
    horizontal: i32,
    vertical: i32,
) -> Point2 {
    let width = GLYPH_ASPECT * height * text.len() as f64;
    let mut dx = 0.0;
    let mut dy = 0.0;
    match horizontal {
        // centre, middle
        1 | 4 => dx = -width / 2.0,
        // right
        2 => dx = -width,
        _ => {}
    }
    if horizontal == 4 {
        dy = -height / 2.0;
    }
    match vertical {
        // middle
        2 => dy = -height / 2.0,
        // top
        3 => dy = -height,
        _ => {}
    }
    anchor + Vec2::new(dx, dy).rotated(rotation)
}

/// Moves heights in Z with the insert: scaled, then raised.
fn place_heights(properties: &mut PropertyMap, vertices: usize, z_scale: f64, z_offset: f64) {
    let mut heights = heights_of(properties, vertices);
    let mut any = false;
    for height in heights.iter_mut().flatten() {
        *height = *height * z_scale + z_offset;
        any = true;
    }
    if any {
        set_heights(properties, &heights);
    }
}

/// The heights an entity's geometry has: one a vertex, two for a line and for
/// an arc (one an end, which differ where it climbs), one for anything else.
fn vertex_count(geometry: &Geometry) -> usize {
    match geometry {
        Geometry::Polyline(polyline) => polyline.vertices.len(),
        Geometry::Line(_) | Geometry::Arc(_) => 2,
        _ => 1,
    }
}

/// The geometry through an insert's transform.
fn place_geometry(geometry: &Geometry, t: &Affine, tolerance: f64) -> Option<Geometry> {
    if t.is_identity() {
        return Some(geometry.clone());
    }
    match geometry {
        Geometry::Point(point) => Some(Geometry::Point(PointGeometry {
            position: t.apply(point.position),
        })),
        Geometry::Line(segment) => Some(Geometry::Line(Segment2::new(
            t.apply(segment.start),
            t.apply(segment.end),
        ))),
        Geometry::Arc(arc) => Some(place_curve(arc, false, t, tolerance)),
        Geometry::Circle(circle) => Some(place_curve(
            &Arc2::new(circle.center, circle.radius, 0.0, TWO_PI),
            true,
            t,
            tolerance,
        )),
        Geometry::Polyline(polyline) => {
            let mut out = polyline.clone();
            for vertex in &mut out.vertices {
                *vertex = t.apply(*vertex);
            }
            Some(Geometry::Polyline(out))
        }
        Geometry::Text(text) => {
            let mut out = text.clone();
            out.position = t.apply(text.position);
            let along = t.linear(Vec2::new(text.rotation.cos(), text.rotation.sin()));
            let up = t.linear(Vec2::new(-text.rotation.sin(), text.rotation.cos()));
            out.rotation = along.y.atan2(along.x);
            out.height = text.height * up.length();
            Some(Geometry::Text(out))
        }
        Geometry::Dimension(dimension) => {
            let mut out = dimension.clone();
            out.start = t.apply(dimension.start);
            out.end = t.apply(dimension.end);
            let scale = t.determinant().abs().sqrt();
            out.offset = dimension.offset * scale * if t.determinant() < 0.0 { -1.0 } else { 1.0 };
            out.vertex = t.apply(dimension.vertex);
            Some(Geometry::Dimension(out))
        }
        // This reader makes neither a label nor a leader, so none is ever in
        // a block; the cases are here for the match to be complete.
        Geometry::Label(_) => None,
        Geometry::Leader(leader) => {
            let mut out = leader.clone();
            for vertex in &mut out.vertices {
                *vertex = t.apply(*vertex);
            }
            Some(Geometry::Leader(out))
        }
        // Under a similarity the arcs stay arcs (a mirror turns them the
        // other way); under anything else they are chorded, and the heights
        // go into the properties as a straight polyline's do.
        Geometry::CurvePolyline(polyline) => {
            if t.is_similarity() {
                let mut out = polyline.clone();
                for vertex in &mut out.vertices {
                    vertex.position = t.apply(vertex.position);
                    if t.determinant() < 0.0 {
                        vertex.bulge = -vertex.bulge + 0.0; // never -0
                    }
                }
                return Some(Geometry::CurvePolyline(out));
            }
            let mut out = polyline.to_polyline(tolerance / cpp::max(t.largest_scale(), 1e-12));
            for vertex in &mut out.vertices {
                *vertex = t.apply(*vertex);
            }
            Some(Geometry::Polyline(out))
        }
        Geometry::Ellipse(ellipse) => {
            if t.is_similarity() {
                let mut out = *ellipse;
                out.center = t.apply(ellipse.center);
                out.major_axis = t.linear(ellipse.major_axis);
                if t.determinant() < 0.0 {
                    out.start_parameter =
                        normalize_angle(-(ellipse.start_parameter + ellipse.sweep));
                }
                return Some(Geometry::Ellipse(out));
            }
            let mut points = ellipse.tessellate(tolerance / cpp::max(t.largest_scale(), 1e-12));
            if ellipse.is_full() && points.len() > 1 {
                points.pop();
            }
            for point in &mut points {
                *point = t.apply(*point);
            }
            Some(Geometry::Polyline(Polyline2::new(
                points,
                ellipse.is_full(),
            )))
        }
        // A B-spline is affine invariant: its control points carry it exactly.
        Geometry::Spline(spline) => {
            let mut out = spline.clone();
            for point in &mut out.control_points {
                *point = t.apply(*point);
            }
            for point in &mut out.fit_points {
                *point = t.apply(*point);
            }
            Some(Geometry::Spline(out))
        }
    }
}

fn translate(geometry: &mut Geometry, shift: Vec2) {
    match geometry {
        Geometry::Point(shape) => shape.position -= shift,
        Geometry::Text(shape) => shape.position -= shift,
        Geometry::Line(shape) => {
            shape.start -= shift;
            shape.end -= shift;
        }
        Geometry::Dimension(shape) => {
            shape.start -= shift;
            shape.end -= shift;
            shape.vertex -= shift;
        }
        Geometry::Arc(shape) => shape.center -= shift,
        Geometry::Circle(shape) => shape.center -= shift,
        Geometry::Label(shape) => {
            shape.anchor -= shift;
            if let Some(position) = &mut shape.position {
                *position -= shift;
            }
        }
        Geometry::CurvePolyline(shape) => {
            for vertex in &mut shape.vertices {
                vertex.position -= shift;
            }
        }
        Geometry::Ellipse(shape) => shape.center -= shift,
        Geometry::Spline(shape) => {
            for p in &mut shape.control_points {
                *p -= shift;
            }
            for p in &mut shape.fit_points {
                *p -= shift;
            }
        }
        Geometry::Polyline(shape) => {
            for vertex in &mut shape.vertices {
                *vertex -= shift;
            }
        }
        Geometry::Leader(shape) => {
            for vertex in &mut shape.vertices {
                *vertex -= shift;
            }
        }
    }
}

fn point_entity(position: Point2) -> Entity {
    Entity {
        geometry: Geometry::Point(PointGeometry { position }),
        ..Entity::default()
    }
}

/// A real out of a pair: zero, and `bad` set, when it is not a number.
fn real_value(value: &str, bad: &mut bool) -> f64 {
    let parsed = to_real(value);
    *bad = *bad || parsed.is_none();
    parsed.unwrap_or(0.0)
}

impl<'a> Reader<'a> {
    fn new(text: &'a str, options: &'a ImportOptions) -> Self {
        Self {
            tokens: Tokenizer::new(text),
            options,
            result: DxfImport::default(),
            default_text_height: 2.5,
            code_page: String::new(),
            layers: Vec::new(),
            layer_index: HashMap::new(),
            last_layer_name: String::new(),
            last_layer: 0,
            have_last_layer: false,
            linetype_names: HashMap::new(),
            blocks: Vec::new(),
            block_index: HashMap::new(),
            tally_index: HashMap::new(),
            warning_counts: BTreeMap::new(),
            full_reported: false,
        }
    }

    // ---- bookkeeping ----

    fn warn(&mut self, category: impl Into<String>) {
        *self.warning_counts.entry(category.into()).or_insert(0) += 1;
    }

    fn full(&self) -> bool {
        self.result.entities.len() >= self.options.maximum_entities
    }

    /// A block's expansion counts against the same limit: blocks nested ten
    /// deep that each insert the one below ten times are ten billion entities
    /// before one reaches the drawing.
    fn sink_full(&self, sink: &Sink<'_>) -> bool {
        match &sink.block {
            Some(block) => block.len() >= self.options.maximum_entities,
            None => self.full(),
        }
    }

    /// True, and said once, when the import has made as many entities as it may.
    fn report_full(&mut self, sink: &Sink<'_>) -> bool {
        if !self.sink_full(sink) {
            return false;
        }
        if !self.full_reported {
            self.full_reported = true;
            self.warn(format!(
                "the import stopped at {} entities, the most one import makes",
                self.options.maximum_entities
            ));
        }
        true
    }

    fn tally(&mut self, kind: &str) -> &mut EntityTally {
        let index = match self.tally_index.get(kind) {
            Some(&index) => index,
            None => {
                let index = self.result.tally.len();
                self.tally_index.insert(kind.to_owned(), index);
                self.result.tally.push(EntityTally {
                    kind: kind.to_owned(),
                    read: 0,
                    imported: 0,
                });
                index
            }
        };
        &mut self.result.tally[index]
    }

    // ---- structure ----

    fn read_body(&mut self, record: &mut Record<'a>) {
        record.pairs.clear();
        while let Some(pair) = self.tokens.next_pair() {
            if pair.code == 0 {
                self.tokens.push_back(pair);
                return;
            }
            record.pairs.push(pair);
        }
    }

    /// A POLYLINE's VERTEX records, or an INSERT's ATTRIB records, up to the
    /// SEQEND that closes them. A missing SEQEND ends the list at the next
    /// other record.
    fn read_children(&mut self, record: &mut Record<'a>) {
        let polyline = record.record_type == "POLYLINE";
        let mut attributes = false;
        if record.record_type == "INSERT" {
            for pair in &record.pairs {
                if pair.code == 66 {
                    attributes = to_integer(pair.value).unwrap_or(0) == 1;
                }
            }
        }
        if !polyline && !attributes {
            return;
        }
        let child_type = if polyline { "VERTEX" } else { "ATTRIB" };
        while let Some(pair) = self.tokens.next_pair() {
            if pair.code != 0 {
                continue;
            }
            let record_type = trimmed_value(pair.value);
            if record_type == child_type {
                let child = record.add_child();
                child.record_type = record_type;
                self.read_body(child);
                continue;
            }
            if record_type == "SEQEND" {
                let mut ignored = Record::default();
                self.read_body(&mut ignored);
                return;
            }
            self.tokens.push_back(pair);
            self.warn(format!(
                "{} without the SEQEND that ends its {child_type} list",
                record.record_type
            ));
            return;
        }
    }

    /// The next record of a section into `record`: false at the end of the
    /// text. `end_of_section` is set at ENDSEC (or at an EOF met before one).
    fn read_record(&mut self, record: &mut Record<'a>, end_of_section: &mut bool) -> bool {
        while let Some(pair) = self.tokens.next_pair() {
            if pair.code != 0 {
                continue; // a stray pair between records: nothing can own it
            }
            let record_type = trimmed_value(pair.value);
            if record_type == "ENDSEC" || record_type == "EOF" {
                if record_type == "EOF" {
                    self.tokens.push_back(pair);
                }
                *end_of_section = true;
                return true;
            }
            record.clear();
            record.record_type = record_type;
            self.read_body(record);
            self.read_children(record);
            return true;
        }
        false
    }

    fn read_header(&mut self) {
        let mut variable = "";
        while let Some(pair) = self.tokens.next_pair() {
            if pair.code == 0 {
                if trimmed_value(pair.value) != "ENDSEC" {
                    self.tokens.push_back(pair);
                }
                return;
            }
            if pair.code == 9 {
                variable = trimmed_value(pair.value);
                continue;
            }
            if variable == "$ACADVER" && pair.code == 1 {
                self.result.version = trimmed_value(pair.value).to_owned();
                self.result.release = release_name(&self.result.version).to_owned();
            } else if variable == "$DWGCODEPAGE" && pair.code == 3 {
                self.code_page = trimmed_value(pair.value).to_owned();
            } else if variable == "$TEXTSIZE"
                && pair.code == 40
                && let Some(size) = to_real(pair.value)
                && size > 0.0
            {
                self.default_text_height = size;
            }
        }
    }

    fn read_tables(&mut self) {
        let mut record = Record::default();
        let mut end = false;
        while !end && self.read_record(&mut record, &mut end) {
            if end {
                break;
            }
            if record.record_type == "LAYER" {
                self.read_layer(&record);
            } else if record.record_type == "LTYPE" {
                self.read_linetype(&record);
            }
        }
    }

    fn read_layer(&mut self, record: &Record<'a>) {
        let mut name = "";
        let mut colour: Option<i32> = None;
        let mut rgb: Option<i64> = None;
        let mut flags = 0;
        let mut linetype = "";
        let mut weight: Option<i32> = None;
        let mut xdata_application = "";
        let mut xdata_word = "";
        let mut katana_path: Option<String> = None;
        let mut exact_colour: Option<Color> = None;
        for pair in &record.pairs {
            match pair.code {
                2 => name = trimmed_value(pair.value),
                62 => colour = to_integer(pair.value).map(|value| value as i32).or(colour),
                420 => rgb = to_integer(pair.value).or(rgb),
                70 => flags = to_integer(pair.value).unwrap_or(0) as i32,
                6 => linetype = trimmed_value(pair.value),
                370 => weight = to_integer(pair.value).map(|value| value as i32).or(weight),
                1001 => {
                    xdata_application = trimmed_value(pair.value);
                    xdata_word = "";
                }
                1000 if xdata_application == APPLICATION_NAME => {
                    let value = trimmed_value(pair.value);
                    if xdata_word.is_empty() && (value == EXTENDED_PATH || value == EXTENDED_COLOUR)
                    {
                        xdata_word = value;
                    } else if xdata_word == EXTENDED_PATH {
                        let path = plain_text(pair.value);
                        if validate_layer_path(&path).is_ok() {
                            katana_path = Some(path);
                        }
                        xdata_word = "";
                    } else if xdata_word == EXTENDED_COLOUR {
                        if let Ok(parsed) = Color::from_hex(value) {
                            exact_colour = Some(parsed);
                        }
                        xdata_word = "";
                    }
                }
                _ => {}
            }
        }
        if name.is_empty() {
            self.warn("LAYER table entry with no name");
            return;
        }
        let key = upper(name);
        if self.layer_index.contains_key(&key) {
            return; // the first definition stands
        }
        let mut layer = Layer {
            name: katana_path.unwrap_or_else(|| path_for_layer_name(&plain_text(name))),
            ..Layer::default()
        };
        // A negative colour is the layer switched off; frozen (flag 1) hides it too.
        if let Some(colour) = colour {
            layer.visible = colour >= 0 && (flags & 1) == 0;
            if let Some(indexed) = indexed_colour(colour.wrapping_abs()) {
                layer.color = indexed;
            }
        } else {
            layer.visible = (flags & 1) == 0;
        }
        if let Some(rgb) = rgb {
            layer.color = true_colour(rgb);
        }
        if let Some(exact) = exact_colour {
            layer.color = exact;
        }
        layer.locked = (flags & 4) != 0;
        if !linetype.is_empty()
            && !equals_upper(linetype, "CONTINUOUS")
            && !equals_upper(linetype, "BYLAYER")
            && !equals_upper(linetype, "BYBLOCK")
        {
            // Resolved against the LTYPE table once both are read.
            layer.linetype = linetype.to_owned();
        }
        if let Some(weight) = weight
            && weight >= 0
        {
            layer.line_weight = f64::from(weight) / 100.0;
        }
        self.layer_index.insert(key, self.layers.len());
        self.layers.push(layer);
    }

    fn read_linetype(&mut self, record: &Record<'a>) {
        let mut name = "";
        let mut description = "";
        let mut lengths: Vec<f64> = Vec::new();
        for pair in &record.pairs {
            match pair.code {
                2 => name = trimmed_value(pair.value),
                3 => description = pair.value,
                49 => lengths.push(to_real(pair.value).unwrap_or(0.0)),
                _ => {}
            }
        }
        if name.is_empty()
            || equals_upper(name, "BYLAYER")
            || equals_upper(name, "BYBLOCK")
            || equals_upper(name, "CONTINUOUS")
        {
            return;
        }
        let key = upper(name);
        if self.linetype_names.contains_key(&key) {
            return;
        }
        let mut linetype = Linetype {
            name: plain_text(name),
            description: plain_text(description),
            pattern: Vec::new(),
        };
        if !lengths.is_empty() {
            linetype.pattern = normalised_pattern(&lengths);
            if linetype.pattern.is_empty() {
                self.warn("linetype pattern that is all dash or all gap, read as continuous");
            }
        }
        if validate(&linetype).is_err() {
            self.warn("linetype Katana cannot draw, read as continuous");
            linetype.pattern.clear();
        }
        // (The C++ also refused a name that is not UTF-8, which a String
        // decoded from the file cannot be.)
        if validate(&linetype).is_err() {
            return;
        }
        self.linetype_names.insert(key, linetype.name.clone());
        self.result.linetypes.push(linetype);
    }

    fn read_blocks(&mut self) {
        let mut record = Record::default();
        let mut end = false;
        let mut current: Option<usize> = None;
        while !end && self.read_record(&mut record, &mut end) {
            if end {
                break;
            }
            if record.record_type == "BLOCK" {
                let mut block = Block::default();
                for pair in &record.pairs {
                    match pair.code {
                        2 => block.name = trimmed_value(pair.value).to_owned(),
                        10 => block.base.x = to_real(pair.value).unwrap_or(0.0),
                        20 => block.base.y = to_real(pair.value).unwrap_or(0.0),
                        30 => block.base_z = to_real(pair.value).unwrap_or(0.0),
                        70 => block.flags = to_integer(pair.value).unwrap_or(0) as i32,
                        _ => {}
                    }
                }
                let key = upper(&block.name);
                if block.name.is_empty() || self.block_index.contains_key(&key) {
                    current = None; // nameless, or a repeat: its content is not reachable
                    continue;
                }
                current = Some(self.blocks.len());
                self.block_index.insert(key, self.blocks.len());
                self.blocks.push(block);
                continue;
            }
            if record.record_type == "ENDBLK" {
                current = None;
                continue;
            }
            if let Some(index) = current {
                let stored = Record {
                    record_type: record.record_type,
                    pairs: record.pairs.clone(),
                    children: record
                        .own_children()
                        .iter()
                        .map(|child| Record {
                            record_type: child.record_type,
                            pairs: child.pairs.clone(),
                            children: Vec::new(),
                            child_count: 0,
                        })
                        .collect(),
                    child_count: record.child_count,
                };
                self.blocks[index].records.push(stored);
            }
        }
    }

    fn read_entities(&mut self) {
        let mut record = Record::default();
        let mut end = false;
        let mut drawing = Sink::default();
        while !end && self.read_record(&mut record, &mut end) {
            if end {
                break;
            }
            drawing.kind = record.record_type;
            self.convert(&record, &mut drawing);
        }
    }

    // ---- tables ----

    fn layer_path(&mut self, dxf_name: &str) -> String {
        let dxf_name = if dxf_name.is_empty() {
            DEFAULT_LAYER_NAME
        } else {
            dxf_name
        };
        if self.have_last_layer && dxf_name == self.last_layer_name {
            return self.layers[self.last_layer].name.clone();
        }
        let key = upper(dxf_name);
        let index = match self.layer_index.get(&key) {
            Some(&index) => index,
            None => {
                // Named by an entity and missing from the table: the defaults.
                let layer = Layer {
                    name: path_for_layer_name(&plain_text(dxf_name)),
                    ..Layer::default()
                };
                let index = self.layers.len();
                self.layer_index.insert(key, index);
                self.layers.push(layer);
                index
            }
        };
        dxf_name.clone_into(&mut self.last_layer_name);
        self.last_layer = index;
        self.have_last_layer = true;
        self.layers[index].name.clone()
    }

    fn colour_of(&self, common: &Common<'_>) -> Option<Color> {
        if common.exact_colour.is_some() {
            return common.exact_colour; // this module's own record of it
        }
        if let Some(value) = common.true_colour {
            return Some(true_colour(value));
        }
        if let Some(colour) = common.colour {
            return indexed_colour(colour.wrapping_abs());
        }
        None
    }

    // ---- conversion ----

    /// The entity, validated, onto the sink: into the drawing with its layer
    /// and colour resolved, or into a block expansion remembering what it
    /// inherits. `level`: without `heights`, the entity lies at Z 0 in its
    /// plane, rather than in a tilted plane where it has no one height.
    fn emit(
        &mut self,
        mut entity: Entity,
        common: &Common<'_>,
        sink: &mut Sink<'a>,
        heights: Option<&[Option<f64>]>,
        level: bool,
    ) {
        if let Err(error) = validate(&entity.geometry) {
            self.warn(format!("{} not imported: {}", sink.kind, error.message));
            return;
        }
        if self.report_full(sink) {
            return;
        }
        if let Some(heights) = heights {
            set_heights(&mut entity.properties, heights);
        }
        let from_insert = common.layer.is_empty() || common.layer == "0";
        let by_block = common.true_colour.is_none()
            && common.exact_colour.is_none()
            && common.colour == Some(COLOUR_BY_BLOCK);
        if !by_block && (common.exact_colour.is_some() || common.colour != Some(COLOUR_BY_LAYER)) {
            entity.color = self.colour_of(common);
        }
        entity.visible = !common.invisible;
        if !common.linetype.is_empty() && !equals_upper(common.linetype, "BYLAYER") {
            let name = self
                .linetype_names
                .get(&upper(common.linetype))
                .cloned()
                .unwrap_or_else(|| plain_text(common.linetype));
            entity
                .metadata
                .insert(String::from(META_LINETYPE), PropertyValue::Text(name));
        }
        if let Some(weight) = common.lineweight
            && weight >= 0
        {
            entity.metadata.insert(
                String::from(META_LINEWEIGHT),
                PropertyValue::Integer(i64::from(weight)),
            );
        }
        if !sink.block_path.is_empty() {
            entity.metadata.insert(
                String::from(META_BLOCK),
                PropertyValue::Text(sink.block_path.clone()),
            );
        }
        if let Some(block) = &mut sink.block {
            entity.layer = String::from(if common.layer.is_empty() {
                "0"
            } else {
                common.layer
            });
            block.push(BlockEntity {
                entity,
                layer_from_insert: from_insert,
                colour_from_insert: by_block,
                at_block_zero: heights.is_none() && level,
            });
            return;
        }
        entity.layer = self.layer_path(common.layer);
        if !self.options.source_name.is_empty() {
            entity.metadata.insert(
                String::from(META_SOURCE),
                PropertyValue::Text(self.options.source_name.clone()),
            );
        }
        self.result.entities.push(entity);
        self.tally(sink.kind).imported += 1;
    }

    fn convert(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let record_type = record.record_type;
        if sink.block.is_none() {
            self.tally(record_type).read += 1;
            if record
                .pairs
                .iter()
                .any(|pair| pair.code == 67 && to_integer(pair.value).unwrap_or(0) == 1)
            {
                self.warn("paper space entities not imported (a Katana drawing is the model)");
                return;
            }
        }
        match record_type {
            "LINE" => self.convert_line(record, sink),
            "POINT" => self.convert_point(record, sink),
            "CIRCLE" => self.convert_circle(record, sink, false),
            "ARC" => self.convert_circle(record, sink, true),
            "ELLIPSE" => self.convert_ellipse(record, sink),
            "SPLINE" => self.convert_spline(record, sink),
            "LWPOLYLINE" => self.convert_lw_polyline(record, sink),
            "POLYLINE" => self.convert_polyline(record, sink),
            "TEXT" => self.convert_text(record, sink, false, None),
            "MTEXT" => self.convert_mtext(record, sink),
            "INSERT" => self.convert_insert(record, sink),
            "DIMENSION" => self.convert_dimension(record, sink),
            // ATTDEF is the template an ATTRIB replaced; a viewport is paper
            // space furniture. Neither is drawing content.
            "ATTDEF" | "SEQEND" | "ENDBLK" | "VIEWPORT" => {}
            _ if sink.block.is_none() => self.warn(format!(
                "{record_type} entities not imported (Katana has no such entity)"
            )),
            _ => {}
        }
    }

    fn convert_line(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let (mut x1, mut y1, mut z1, mut x2, mut y2, mut z2) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            let target = match pair.code {
                10 => &mut x1,
                20 => &mut y1,
                30 => &mut z1,
                11 => &mut x2,
                21 => &mut y2,
                31 => &mut z2,
                _ => continue,
            };
            *target = real_value(pair.value, &mut bad);
        }
        if bad {
            self.warn("LINE not imported: a coordinate is not a number");
            return;
        }
        let start = Point2::new(x1, y1);
        let end = Point2::new(x2, y2);
        if start.distance_to(end) <= tolerance::GEOMETRIC {
            // A dot drawn as a line of no length is still a surveyed dot.
            let entity = point_entity(start);
            if z1 != 0.0 {
                let heights = [Some(z1)];
                self.emit(entity, &common, sink, Some(heights.as_slice()), true);
            } else {
                self.emit(entity, &common, sink, None, true);
            }
            return;
        }
        let entity = Entity {
            geometry: Geometry::Line(Segment2::new(start, end)),
            ..Entity::default()
        };
        let extended = common.extended_heights(2);
        if !extended.is_empty() {
            self.emit(entity, &common, sink, Some(extended.as_slice()), true);
        } else if z1 != 0.0 || z2 != 0.0 {
            let heights = [Some(z1), Some(z2)];
            self.emit(entity, &common, sink, Some(heights.as_slice()), true);
        } else {
            self.emit(entity, &common, sink, None, true);
        }
    }

    fn convert_point(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            match pair.code {
                10 => x = real_value(pair.value, &mut bad),
                20 => y = real_value(pair.value, &mut bad),
                30 => z = real_value(pair.value, &mut bad),
                _ => {}
            }
        }
        if bad {
            self.warn("POINT not imported: a coordinate is not a number");
            return;
        }
        let entity = point_entity(Point2::new(x, y));
        // Z 0 is the plan, as every program writes it - unless this module's
        // writer said beside it that 0 is the point's surveyed level.
        let extended = common.extended_heights(1);
        if !extended.is_empty() {
            self.emit(entity, &common, sink, Some(extended.as_slice()), true);
        } else if z != 0.0 {
            let heights = [Some(z)];
            self.emit(entity, &common, sink, Some(heights.as_slice()), true);
        } else {
            self.emit(entity, &common, sink, None, true);
        }
    }

    fn convert_circle(&mut self, record: &Record<'a>, sink: &mut Sink<'a>, arc: bool) {
        let mut common = Common::default();
        let (mut x, mut y, mut z, mut radius, mut start, mut end) =
            (0.0, 0.0, 0.0, 0.0, 0.0, 360.0);
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            let target = match pair.code {
                10 => &mut x,
                20 => &mut y,
                30 => &mut z,
                40 => &mut radius,
                50 => &mut start,
                51 => &mut end,
                _ => continue,
            };
            *target = real_value(pair.value, &mut bad);
        }
        let kind = if arc { "ARC" } else { "CIRCLE" };
        if bad {
            self.warn(format!("{kind} not imported: a value is not a number"));
            return;
        }
        let plain = is_plain_extrusion(&common.extrusion);
        let frame = if plain {
            Affine::default()
        } else {
            object_to_plan(&common.extrusion, z)
        };
        let level = is_level(&common.extrusion, &frame);
        if !level {
            self.warn(format!(
                "{kind} in a tilted plane, projected onto the plan as a chorded polyline"
            ));
        }
        // An arc that starts where it ends is read as the full turn it draws.
        let full = !arc || (end - start).abs() % 360.0 == 0.0;
        let curve = Arc2::new(
            Point2::new(x, y),
            radius,
            if full { 0.0 } else { start * DEGREES },
            if full {
                TWO_PI
            } else {
                sweep_between(start, end)
            },
        );
        if !(radius > 0.0) {
            self.warn(format!("{kind} not imported: its radius is not positive"));
            return;
        }
        let entity = Entity {
            geometry: place_curve(&curve, full, &frame, self.options.curve_tolerance),
            ..Entity::default()
        };
        // An arc has a height at each end, which differ where it climbs, as an
        // imported survey arc on a grade does. This module's writer puts two that
        // differ beside the ARC, whose one Z cannot hold them, in the file's
        // order from the start angle; a mirror reverses that order with the arc.
        let two_ends = arc && !full;
        let mut extended = common.extended_heights(if two_ends { 2 } else { 1 });
        if !extended.is_empty() {
            if two_ends && frame.determinant() < 0.0 {
                extended.swap(0, 1);
            }
            self.emit(entity, &common, sink, Some(extended.as_slice()), true);
        } else if z != 0.0 && level {
            // The elevation of a level circle; a tilted one has none that means
            // anything.
            let heights = [Some(height_of(&common.extrusion, z))];
            self.emit(entity, &common, sink, Some(heights.as_slice()), true);
        } else {
            self.emit(entity, &common, sink, None, level);
        }
    }

    fn convert_ellipse(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let (mut cx, mut cy, mut cz) = (0.0, 0.0, 0.0);
        let (mut mx, mut my, mut mz) = (1.0, 0.0, 0.0);
        let (mut ratio, mut start, mut end) = (1.0, 0.0, TWO_PI);
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            let target = match pair.code {
                10 => &mut cx,
                20 => &mut cy,
                30 => &mut cz,
                11 => &mut mx,
                21 => &mut my,
                31 => &mut mz,
                40 => &mut ratio,
                41 => &mut start,
                42 => &mut end,
                _ => continue,
            };
            *target = real_value(pair.value, &mut bad);
        }
        if bad || !(ratio > 0.0) {
            self.warn("ELLIPSE not imported: a value is not a number");
            return;
        }
        // The ellipse is in world coordinates: centre, major axis, and the minor
        // axis `ratio` times as long, a quarter turn about the extrusion.
        let normal = normalised(&common.extrusion);
        let major = Vec3 {
            x: mx,
            y: my,
            z: mz,
        };
        let minor_direction = cross(&normal, &major);
        let major_axis = Vec2::new(mx, my);
        let minor_axis = Vec2::new(minor_direction.x * ratio, minor_direction.y * ratio);
        let major_length = (mx * mx + my * my + mz * mz).sqrt();
        let mut sweep = (end - start) % TWO_PI;
        if sweep <= 0.0 {
            sweep += TWO_PI;
        }
        let full = (sweep - TWO_PI).abs() <= 1e-9;
        // An ellipse in the plan (extrusion straight up or straight down) is kept
        // as one (docs/drawing.md); straight down, the minor axis points the
        // other way, which Ellipse2 says as the same arc walked from its end.
        if normal.x.abs() <= 1e-12
            && normal.y.abs() <= 1e-12
            && major_length > 0.0
            && mz.abs() <= 1e-12 * major_length
        {
            let exact = Ellipse2 {
                center: Point2::new(cx, cy),
                major_axis,
                ratio: cpp::min(ratio, 1.0),
                // In [0, 2 pi), as the file's own parameters are: a mirror's
                // -(start + sweep) is the same place a turn on.
                start_parameter: normalize_angle(if normal.z > 0.0 {
                    start
                } else {
                    -(start + sweep)
                }),
                sweep: if full { TWO_PI } else { sweep },
            };
            let entity = Entity {
                geometry: Geometry::Ellipse(exact),
                ..Entity::default()
            };
            if cz != 0.0 {
                let heights = [Some(cz)];
                self.emit(entity, &common, sink, Some(heights.as_slice()), true);
            } else {
                self.emit(entity, &common, sink, None, true);
            }
            return;
        }
        // Chorded as the circle of the major radius would be: the flatter an
        // ellipse the less a chord stands off it, so this is never too coarse.
        let count = sagitta_chord_count(major_length, sweep, self.options.curve_tolerance);
        let mut points = Vec::with_capacity(count + 1);
        let last = if full { count - 1 } else { count };
        for i in 0..=last {
            let t = start + sweep * i as f64 / count as f64;
            points.push(Point2::new(cx, cy) + major_axis * t.cos() + minor_axis * t.sin());
        }
        let vertices = points.len();
        let entity = Entity {
            geometry: Geometry::Polyline(Polyline2::new(points, full)),
            ..Entity::default()
        };
        // The centre is in world coordinates, so its Z is the height of an
        // ellipse that lies level; a tilted one has none.
        let level = is_level(&normal, &object_to_plan(&normal, 0.0));
        if level && cz != 0.0 {
            let heights = vec![Some(cz); vertices];
            self.emit(entity, &common, sink, Some(heights.as_slice()), true);
        } else {
            self.emit(entity, &common, sink, None, level);
        }
    }

    /// SPLINE: degree, knots, weights, control points and fit points, taken as
    /// the file has them into a Spline2 (docs/drawing.md). A spline with fit
    /// points and no control points (some writers leave the solve to the
    /// reader) is solved through them. Heights are dropped with a warning:
    /// Spline2 is a plan curve.
    fn convert_spline(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let mut spline = Spline2::default();
        let mut flags = 0;
        let mut bad = false;
        let mut any_z = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            match pair.code {
                70 => flags = to_integer(pair.value).unwrap_or(0) as i32,
                71 => spline.degree = to_integer(pair.value).unwrap_or(3) as i32,
                40 => spline.knots.push(real_value(pair.value, &mut bad)),
                41 => spline.weights.push(real_value(pair.value, &mut bad)),
                10 => spline
                    .control_points
                    .push(Point2::new(real_value(pair.value, &mut bad), 0.0)),
                20 if !spline.control_points.is_empty() => {
                    let y = real_value(pair.value, &mut bad);
                    if let Some(last) = spline.control_points.last_mut() {
                        last.y = y;
                    }
                }
                11 => spline
                    .fit_points
                    .push(Point2::new(real_value(pair.value, &mut bad), 0.0)),
                21 if !spline.fit_points.is_empty() => {
                    let y = real_value(pair.value, &mut bad);
                    if let Some(last) = spline.fit_points.last_mut() {
                        last.y = y;
                    }
                }
                // Once a height is seen the rest are not read, nor checked.
                30 | 31 => any_z = any_z || real_value(pair.value, &mut bad) != 0.0,
                _ => {}
            }
        }
        if bad {
            self.warn("SPLINE not imported: a value is not a number");
            return;
        }
        if (flags & 4) == 0
            || spline.weights.len() != spline.control_points.len()
            || spline.weights.iter().all(|&w| w == 1.0)
        {
            spline.weights.clear();
        }
        if spline.control_points.is_empty() && spline.fit_points.len() >= 2 {
            match Spline2::through_points(spline.fit_points.clone(), spline.degree) {
                Ok(solved) => spline = solved,
                Err(error) => {
                    self.warn(format!("SPLINE not imported: {}", error.message));
                    return;
                }
            }
        }
        if any_z {
            self.warn("SPLINE heights not imported: a spline is drawn in plan");
        }
        let entity = Entity {
            geometry: Geometry::Spline(spline),
            ..Entity::default()
        };
        self.emit(entity, &common, sink, None, true);
    }

    /// The vertices' z are world heights already; `level` is false where the
    /// polyline lies in a tilted plane, and they are then all 0.
    #[allow(
        clippy::too_many_arguments,
        reason = "the signature mirrors the C++ emitPolyline"
    )]
    fn emit_polyline(
        &mut self,
        vertices: Vec<Vertex>,
        closed: bool,
        three_d: bool,
        frame: &Affine,
        level: bool,
        common: &Common<'_>,
        sink: &mut Sink<'a>,
    ) {
        let kind = sink.kind;
        if vertices.len() == 1 {
            // One vertex draws a dot; that is what it is kept as.
            let entity = point_entity(frame.apply(Point2::new(vertices[0].x, vertices[0].y)));
            if vertices[0].z != 0.0 {
                let heights = [Some(vertices[0].z)];
                self.emit(entity, common, sink, Some(heights.as_slice()), true);
            } else {
                self.emit(entity, common, sink, None, level);
            }
            return;
        }
        if vertices.is_empty() {
            self.warn(format!("{kind} not imported: it has no vertices"));
            return;
        }
        // A 2D polyline with arc segments is kept with its arcs, as a
        // CurvePolyline2 whose heights are its own (docs/drawing.md), wherever
        // the frame keeps a circle a circle; under a non-uniform scale an arc is
        // not one any more, and is chorded below as it always was.
        let vertex_total = vertices.len();
        let starts = &vertices[..vertex_total - usize::from(!closed)];
        let any_bulge = starts.iter().any(|v| v.bulge != 0.0 && v.bulge.is_finite());
        if !three_d && any_bulge && frame.is_similarity() {
            let extended = common.extended_heights(vertex_total);
            let mirrored = frame.determinant() < 0.0;
            let any_z = vertices.iter().any(|v| v.z != 0.0);
            let mut curved = CurvePolyline2 {
                vertices: Vec::with_capacity(vertex_total),
                closed,
            };
            for (i, v) in vertices.iter().enumerate() {
                let height = if !extended.is_empty() {
                    extended[i]
                } else if any_z {
                    Some(v.z)
                } else {
                    None
                };
                curved.vertices.push(CurveVertex {
                    position: frame.apply(Point2::new(v.x, v.y)),
                    // + 0.0: a straight segment's 0 mirrored stays 0, not -0,
                    // which VERTEX LIST would print as a bulge.
                    bulge: if v.bulge.is_finite() {
                        if mirrored { -v.bulge + 0.0 } else { v.bulge }
                    } else {
                        0.0
                    },
                    height,
                });
            }
            let entity = Entity {
                geometry: Geometry::CurvePolyline(curved),
                ..Entity::default()
            };
            self.emit(entity, common, sink, None, level);
            return;
        }
        // The vertices are kept as the file has them - a closed polyline whose
        // last vertex repeats its first included - so that what was written is
        // what comes back: the real drawing this was measured on has one such,
        // and dropping the repeat made it the only entity of 27 886 to differ.
        let count = vertices.len();
        let mut points: Vec<Point2> = Vec::with_capacity(count);
        let mut heights: Vec<Option<f64>> = Vec::with_capacity(count);
        let mut any_height = false;
        let segments = if closed { count } else { count - 1 };
        let scale = frame.largest_scale();
        let tolerance = if scale > 0.0 {
            self.options.curve_tolerance / scale
        } else {
            self.options.curve_tolerance
        };
        for (i, vertex) in vertices.iter().enumerate() {
            points.push(Point2::new(vertex.x, vertex.y));
            heights.push(Some(vertex.z));
            any_height = any_height || vertex.z != 0.0;
            if i >= segments || three_d {
                continue;
            }
            let next = vertices[(i + 1) % count];
            let Some(arc) = bulge_arc(
                Point2::new(vertex.x, vertex.y),
                Point2::new(next.x, next.y),
                vertex.bulge,
            ) else {
                continue;
            };
            self.result.arcs_chorded += 1;
            let chords = chord_arc(&arc, tolerance);
            // The ends are the vertices themselves, exactly; only what lies
            // between them is added. Heights along an arc are the vertex's own,
            // since a 2D polyline has one elevation.
            for &chord in chords.get(1..chords.len().saturating_sub(1)).unwrap_or(&[]) {
                points.push(chord);
                heights.push(Some(vertex.z));
            }
        }
        if !frame.is_identity() {
            for point in &mut points {
                *point = frame.apply(*point);
            }
        }
        // Heights this module's writer kept beside a polyline it had to write in
        // plan, since some of its vertices had none - one a vertex, so only
        // where no arc added any.
        let extended = common.extended_heights(count);
        let use_extended = !extended.is_empty() && points.len() == count;
        let entity = Entity {
            geometry: Geometry::Polyline(Polyline2::new(points, closed)),
            ..Entity::default()
        };
        // A 3D polyline's heights are data even where they are zero; a 2D one's
        // elevation of zero is only the plan.
        if use_extended {
            self.emit(entity, common, sink, Some(extended.as_slice()), true);
        } else if three_d || any_height {
            self.emit(entity, common, sink, Some(heights.as_slice()), true);
        } else {
            self.emit(entity, common, sink, None, level);
        }
    }

    fn convert_lw_polyline(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut elevation = 0.0;
        let mut flags = 0;
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            match pair.code {
                90 => {
                    // A count is a hint, and a hostile one could ask for any amount
                    // of memory: never more than the pairs there are to fill it.
                    let count = to_integer(pair.value).unwrap_or(0);
                    if count > 0 {
                        vertices.reserve((count as usize).min(record.pairs.len()));
                    }
                }
                70 => flags = to_integer(pair.value).unwrap_or(0) as i32,
                38 => elevation = to_real(pair.value).unwrap_or(0.0),
                10 => vertices.push(Vertex {
                    x: real_value(pair.value, &mut bad),
                    ..Vertex::default()
                }),
                20 if !vertices.is_empty() => {
                    let y = real_value(pair.value, &mut bad);
                    if let Some(last) = vertices.last_mut() {
                        last.y = y;
                    }
                }
                42 if !vertices.is_empty() => {
                    let bulge = to_real(pair.value).unwrap_or(0.0);
                    if let Some(last) = vertices.last_mut() {
                        last.bulge = bulge;
                    }
                }
                _ => {}
            }
        }
        if bad {
            self.warn("LWPOLYLINE not imported: a coordinate is not a number");
            return;
        }
        let plain = is_plain_extrusion(&common.extrusion);
        let frame = if plain {
            Affine::default()
        } else {
            object_to_plan(&common.extrusion, elevation)
        };
        let level = is_level(&common.extrusion, &frame);
        if !level {
            self.warn("LWPOLYLINE in a tilted plane, projected onto the plan");
        }
        let height = if level {
            height_of(&common.extrusion, elevation)
        } else {
            0.0
        };
        for vertex in &mut vertices {
            vertex.z = height;
        }
        self.emit_polyline(
            vertices,
            (flags & 1) != 0,
            false,
            &frame,
            level,
            &common,
            sink,
        );
    }

    fn convert_polyline(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let mut flags = 0;
        let mut elevation = 0.0;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            if pair.code == 70 {
                flags = to_integer(pair.value).unwrap_or(0) as i32;
            } else if pair.code == 30 {
                elevation = to_real(pair.value).unwrap_or(0.0);
            }
        }
        if (flags & (16 | 64)) != 0 {
            self.warn("POLYLINE meshes not imported (Katana has no mesh entity in the drawing)");
            return;
        }
        let three_d = (flags & 8) != 0;
        // A 3D polyline's vertices are world coordinates; a 2D one's are in its
        // object coordinate system, at its elevation.
        let plain = three_d || is_plain_extrusion(&common.extrusion);
        let frame = if plain {
            Affine::default()
        } else {
            object_to_plan(&common.extrusion, elevation)
        };
        let level = plain || frame.is_similarity();
        let height = if level && !three_d {
            height_of(&common.extrusion, elevation)
        } else {
            0.0
        };
        let mut vertices: Vec<Vertex> = Vec::with_capacity(record.child_count);
        let mut bad = false;
        for child in record.own_children() {
            let mut vertex = Vertex {
                z: height,
                ..Vertex::default()
            };
            let mut vertex_flags = 0;
            for pair in &child.pairs {
                match pair.code {
                    10 | 20 | 30 => {
                        let value = real_value(pair.value, &mut bad);
                        if pair.code == 10 {
                            vertex.x = value;
                        } else if pair.code == 20 {
                            vertex.y = value;
                        } else if three_d {
                            vertex.z = value;
                        }
                    }
                    42 => vertex.bulge = to_real(pair.value).unwrap_or(0.0),
                    70 => vertex_flags = to_integer(pair.value).unwrap_or(0) as i32,
                    _ => {}
                }
            }
            // A spline's frame (control) points are not on the curve; its
            // fitted vertices are.
            if (vertex_flags & 16) != 0 {
                continue;
            }
            vertices.push(vertex);
        }
        if bad {
            self.warn("POLYLINE not imported: a coordinate is not a number");
            return;
        }
        if !level {
            self.warn("POLYLINE in a tilted plane, projected onto the plan");
        }
        self.emit_polyline(
            vertices,
            (flags & 1) != 0,
            three_d,
            &frame,
            level,
            &common,
            sink,
        );
    }

    fn convert_text(
        &mut self,
        record: &Record<'a>,
        sink: &mut Sink<'a>,
        attribute: bool,
        properties: Option<&PropertyMap>,
    ) {
        let mut common = Common::default();
        let mut raw: &str = "";
        let (mut x, mut y, mut ax, mut ay) = (0.0, 0.0, 0.0, 0.0);
        let (mut height, mut rotation, mut z) = (0.0, 0.0, 0.0);
        let mut have_first = false;
        let mut have_second = false;
        let mut horizontal = 0;
        let mut vertical = 0;
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            match pair.code {
                1 => raw = pair.value,
                10 | 20 | 11 | 21 | 30 | 40 | 50 => {
                    let v = real_value(pair.value, &mut bad);
                    match pair.code {
                        10 => {
                            x = v;
                            have_first = true;
                        }
                        20 => y = v,
                        11 => {
                            ax = v;
                            have_second = true;
                        }
                        21 => ay = v,
                        30 => z = v,
                        40 => height = v,
                        _ => rotation = v,
                    }
                }
                72 => horizontal = to_integer(pair.value).unwrap_or(0) as i32,
                73 if !attribute => vertical = to_integer(pair.value).unwrap_or(0) as i32,
                74 if attribute => vertical = to_integer(pair.value).unwrap_or(0) as i32,
                _ => {}
            }
        }
        let kind = if attribute { "ATTRIB" } else { "TEXT" };
        if bad {
            self.warn(format!("{kind} not imported: a value is not a number"));
            return;
        }
        let text = plain_text(raw);
        if text.is_empty() {
            return; // nothing to read, and Katana refuses an empty text
        }
        if !(height > 0.0) {
            height = self.default_text_height;
        }
        let radians = rotation * DEGREES;
        let mut position = Point2::new(x, y);
        // Justified text is anchored at the second point. The first is then the
        // left of the baseline as the writing program computed it, which is
        // better than any guess - unless the program did not compute it, which
        // leaves it at the origin or missing.
        let justified = (horizontal != 0 && horizontal != 3 && horizontal != 5) || vertical != 0;
        if justified && have_second && (!have_first || (x == 0.0 && y == 0.0)) {
            position = left_baseline_from(
                Point2::new(ax, ay),
                &text,
                height,
                radians,
                horizontal,
                vertical,
            );
        }
        let plain = is_plain_extrusion(&common.extrusion);
        let frame = if plain {
            Affine::default()
        } else {
            object_to_plan(&common.extrusion, z)
        };
        let level = is_level(&common.extrusion, &frame);
        let mut final_rotation = radians;
        if !plain {
            position = frame.apply(position);
            let direction = frame.linear(Vec2::new(radians.cos(), radians.sin()));
            final_rotation = direction.y.atan2(direction.x);
        }
        let mut entity = Entity {
            geometry: Geometry::Text(TextGeometry::new(position, text, height, final_rotation)),
            ..Entity::default()
        };
        if let Some(properties) = properties {
            entity.properties = properties.clone();
        }
        let extended = common.extended_heights(1);
        if !extended.is_empty() {
            self.emit(entity, &common, sink, Some(extended.as_slice()), true);
        } else if z != 0.0 && level {
            let heights = [Some(height_of(&common.extrusion, z))];
            self.emit(entity, &common, sink, Some(heights.as_slice()), true);
        } else {
            self.emit(entity, &common, sink, None, level);
        }
    }

    /// A line of MTEXT whose bytes are not UTF-8 (plain_mtext says how one
    /// can be): what the C++'s validate says of it, in its order of checks -
    /// the position, the rotation, then the text - since a String cannot be
    /// made of it to validate.
    fn refuse_text(&mut self, sink: &Sink<'a>, position: Point2, rotation: f64) {
        let message = if !position.is_finite() {
            "text position is not finite"
        } else if !rotation.is_finite() {
            "text rotation is not finite"
        } else {
            "text is not valid UTF-8"
        };
        self.warn(format!("{} not imported: {message}", sink.kind));
    }

    fn convert_mtext(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let mut raw = String::new();
        let mut last: &str = "";
        let (mut x, mut y, mut z, mut height, mut rotation) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let (mut dx, mut dy) = (0.0, 0.0);
        let mut spacing = 1.0;
        let mut have_direction = false;
        let mut attachment = 1;
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            match pair.code {
                3 => raw.push_str(pair.value), // the text in pieces, before the last
                1 => last = pair.value,
                10 | 20 | 30 | 40 | 50 | 11 | 21 | 44 => {
                    let v = real_value(pair.value, &mut bad);
                    match pair.code {
                        10 => x = v,
                        20 => y = v,
                        30 => z = v,
                        40 => height = v,
                        50 => rotation = v,
                        11 => {
                            dx = v;
                            have_direction = true;
                        }
                        21 => {
                            dy = v;
                            have_direction = true;
                        }
                        _ => spacing = if v > 0.0 { v } else { 1.0 },
                    }
                }
                71 => attachment = to_integer(pair.value).unwrap_or(1) as i32,
                _ => {}
            }
        }
        if bad {
            self.warn("MTEXT not imported: a value is not a number");
            return;
        }
        raw.push_str(last);
        let plain = plain_mtext(&raw);
        if !(height > 0.0) {
            height = self.default_text_height;
        }
        // The direction vector, where there is one, wins over the angle: it is
        // what the writing program keeps, and the angle is derived from it.
        let mut radians = rotation * DEGREES;
        let directed = have_direction && (dx != 0.0 || dy != 0.0);
        if directed {
            radians = dy.atan2(dx);
        }
        let lines: Vec<&[u8]> = plain.split(|&byte| byte == b'\n').collect();
        let pitch = LINE_PITCH * height * spacing;
        let block = height + pitch * (lines.len() - 1) as f64;
        // Attachment 1-9: top, middle, bottom rows of left, centre, right. The
        // first baseline sits one text height below a top attachment.
        let row = (attachment.clamp(1, 9) - 1) / 3;
        let column = (attachment.clamp(1, 9) - 1) % 3;
        let mut first_baseline = -height;
        if row == 1 {
            first_baseline = block / 2.0 - height;
        } else if row == 2 {
            first_baseline = block - height;
        }
        // MTEXT is not an object coordinate system entity: its insertion point
        // and its direction vector are world coordinates, and the extrusion says
        // only which way its plane faces. (Put through the object system, a text
        // facing down landed at the mirror of where it stands, turned half a
        // turn.) An angle given alone is measured in that plane, and its axes
        // turn it into the world, as TEXT's is.
        let plain_extrusion = is_plain_extrusion(&common.extrusion);
        let frame = if plain_extrusion {
            Affine::default()
        } else {
            object_to_plan(&common.extrusion, 0.0)
        };
        let level = is_level(&common.extrusion, &frame);
        let mut along = Vec2::new(radians.cos(), radians.sin());
        let mut up = Vec2::new(-along.y, along.x);
        let mut final_rotation = radians;
        if !plain_extrusion {
            if directed {
                // The text's up is the extrusion across its direction: to the
                // left of it seen from above, to the right in a plane facing down.
                if normalised(&common.extrusion).z < 0.0 {
                    up = Vec2::new(along.y, -along.x);
                }
            } else {
                up = frame.linear(up);
                along = frame.linear(along);
                final_rotation = along.y.atan2(along.x);
            }
        }
        // The insertion point's Z is a world height too, where the plane is level.
        let mut heights = common.extended_heights(1);
        if heights.is_empty() && z != 0.0 && level {
            heights = vec![Some(z)];
        }
        for (i, &line) in lines.iter().enumerate() {
            if trimmed(line).is_empty() {
                continue;
            }
            // Bytes, as the C++'s std::string::size counts them.
            let width = GLYPH_ASPECT * height * line.len() as f64;
            let shift = if column == 1 {
                -width / 2.0
            } else if column == 2 {
                -width
            } else {
                0.0
            };
            let position =
                Point2::new(x, y) + along * shift + up * (first_baseline - pitch * i as f64);
            let Ok(words) = std::str::from_utf8(line) else {
                self.refuse_text(sink, position, final_rotation);
                continue;
            };
            let entity = Entity {
                geometry: Geometry::Text(TextGeometry::new(
                    position,
                    words,
                    height,
                    final_rotation,
                )),
                ..Entity::default()
            };
            let given = if heights.is_empty() {
                None
            } else {
                Some(heights.as_slice())
            };
            self.emit(entity, &common, sink, given, level);
        }
    }

    /// Converts a block's records once, into the block's own coordinates.
    fn build_block(&mut self, name: &str, depth: usize, insert_kind: &str) -> Option<usize> {
        let Some(&index) = self.block_index.get(&upper(name)) else {
            self.warn(format!("{insert_kind} of a block the file does not define"));
            return None;
        };
        if self.blocks[index].built {
            return Some(index);
        }
        if self.blocks[index].building {
            self.warn(format!(
                "{insert_kind} of a block that contains itself, not expanded"
            ));
            return None;
        }
        if depth > self.options.maximum_block_depth {
            self.warn(format!(
                "{insert_kind} nested deeper than {} blocks, not expanded",
                self.options.maximum_block_depth
            ));
            return None;
        }
        if (self.blocks[index].flags & 4) != 0 {
            self.warn(format!(
                "{insert_kind} of an external reference, whose drawing is not read"
            ));
        }
        self.blocks[index].building = true;
        // The records are lent out while they are converted, so that a block
        // they insert can be built meanwhile; this block, being built, is
        // refused if they reach it again.
        let records = std::mem::take(&mut self.blocks[index].records);
        let mut inner = Sink {
            block: Some(Vec::new()),
            kind: "",
            depth,
            block_path: String::new(),
        };
        for record in &records {
            inner.kind = record.record_type;
            self.convert(record, &mut inner);
        }
        let block = &mut self.blocks[index];
        block.records = records;
        block.expanded = inner.block.unwrap_or_default();
        block.building = false;
        block.built = true;
        Some(index)
    }

    fn place(
        &mut self,
        block_index: usize,
        transform: &Affine,
        height_map: Option<HeightMap>,
        insert: &Common<'_>,
        attributes: &PropertyMap,
        sink: &mut Sink<'a>,
    ) {
        // Lent out while it is placed: nothing placing reaches the block table.
        let expanded = std::mem::take(&mut self.blocks[block_index].expanded);
        let name = self.blocks[block_index].name.clone();
        for item in &expanded {
            if self.report_full(sink) {
                break;
            }
            let Some(geometry) = place_geometry(
                &item.entity.geometry,
                transform,
                self.options.curve_tolerance,
            ) else {
                continue;
            };
            let mut entity = item.entity.clone();
            entity.geometry = geometry;
            for (key, value) in attributes {
                // the entity's own value wins
                entity
                    .properties
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
            // Its heights with the insert: an entity drawn at the block's Z 0
            // is at the insert's level (block Z 0, scaled, plus the offset); one
            // with heights has them scaled and raised; under a tilted insert
            // none is right. At an offset of 0 a Z-0 entity stays one, for the
            // insert that places this block in turn.
            let vertices = vertex_count(&entity.geometry);
            let mut at_block_zero = false;
            match height_map {
                None => set_heights(&mut entity.properties, &[]),
                Some(map) if item.at_block_zero => {
                    if map.offset != 0.0 {
                        set_heights(&mut entity.properties, &vec![Some(map.offset); vertices]);
                    } else {
                        at_block_zero = true;
                    }
                }
                Some(map) if map.scale != 1.0 || map.offset != 0.0 => {
                    place_heights(&mut entity.properties, vertices, map.scale, map.offset);
                }
                Some(_) => {}
            }
            // A curve polyline holds its heights itself: they move with the insert
            // there, by the same rule.
            if let Geometry::CurvePolyline(curved) = &mut entity.geometry {
                for vertex in &mut curved.vertices {
                    match height_map {
                        None => vertex.height = None,
                        Some(map) => {
                            if let Some(height) = vertex.height {
                                vertex.height = Some(height * map.scale + map.offset);
                            } else if item.at_block_zero && map.offset != 0.0 {
                                vertex.height = Some(map.offset);
                            }
                        }
                    }
                }
            } else if matches!(entity.geometry, Geometry::Polyline(_))
                && matches!(item.entity.geometry, Geometry::CurvePolyline(_))
            {
                // Chorded by a non-uniform insert, which no arc survives: its
                // heights went with the geometry that held them, and none is
                // invented for the chords.
                set_heights(&mut entity.properties, &[]);
            }
            // A mirror reverses an arc, and the heights at its two ends with it.
            if transform.determinant() < 0.0 && matches!(entity.geometry, Geometry::Arc(_)) {
                let mut ends = heights_of(&entity.properties, 2);
                if ends[0] != ends[1] {
                    ends.swap(0, 1);
                    set_heights(&mut entity.properties, &ends);
                }
            }
            // What the entity inherits from this insert: its layer when it is on
            // layer 0, its colour when that is ByBlock.
            let layer: &str = if item.layer_from_insert {
                insert.layer
            } else {
                &item.entity.layer
            };
            let mut block_path = if sink.block_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{name}", sink.block_path)
            };
            if let Some(nested) = item.entity.metadata.get(META_BLOCK) {
                block_path.push('/');
                block_path.push_str(&property_to_string(nested));
            }
            entity
                .metadata
                .insert(String::from(META_BLOCK), PropertyValue::Text(block_path));
            let insert_by_block =
                insert.true_colour.is_none() && insert.colour == Some(COLOUR_BY_BLOCK);
            if sink.block.is_some() {
                entity.layer = String::from(if layer.is_empty() { "0" } else { layer });
                if item.colour_from_insert {
                    entity.color = self.colour_of(insert);
                    if insert.colour == Some(COLOUR_BY_LAYER) {
                        entity.color = None;
                    }
                }
                let placed = BlockEntity {
                    entity,
                    layer_from_insert: item.layer_from_insert
                        && (insert.layer.is_empty() || insert.layer == "0"),
                    colour_from_insert: item.colour_from_insert && insert_by_block,
                    at_block_zero,
                };
                if let Some(block) = &mut sink.block {
                    block.push(placed);
                }
                continue;
            }
            entity.layer = self.layer_path(layer);
            if item.colour_from_insert {
                let insert_by_layer = insert.true_colour.is_none()
                    && (insert.colour.is_none()
                        || insert.colour == Some(COLOUR_BY_LAYER)
                        || insert.colour == Some(COLOUR_BY_BLOCK));
                if insert_by_layer {
                    // ByBlock under a ByLayer insert is the INSERT's layer colour,
                    // which is not this entity's layer's unless it is on it.
                    let insert_layer = self.layer_path(insert.layer);
                    if insert_layer != entity.layer {
                        entity.color = Some(self.layers[self.last_layer].color);
                    } else {
                        entity.color = None;
                    }
                } else {
                    entity.color = self.colour_of(insert);
                }
            }
            if !self.options.source_name.is_empty() {
                entity.metadata.insert(
                    String::from(META_SOURCE),
                    PropertyValue::Text(self.options.source_name.clone()),
                );
            }
            self.result.entities.push(entity);
            self.tally(sink.kind).imported += 1;
        }
        self.blocks[block_index].expanded = expanded;
    }

    fn convert_insert(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let mut name = "";
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        let (mut sx, mut sy, mut sz) = (1.0, 1.0, 1.0);
        let mut rotation = 0.0;
        let (mut column_spacing, mut row_spacing) = (0.0, 0.0);
        let (mut columns, mut rows): (i64, i64) = (1, 1);
        let mut bad = false;
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            match pair.code {
                2 => name = trimmed_value(pair.value),
                10 | 20 | 30 | 41 | 42 | 43 | 50 | 44 | 45 => {
                    let v = real_value(pair.value, &mut bad);
                    match pair.code {
                        10 => x = v,
                        20 => y = v,
                        30 => z = v,
                        41 => sx = v,
                        42 => sy = v,
                        43 => sz = v,
                        50 => rotation = v,
                        44 => column_spacing = v,
                        _ => row_spacing = v,
                    }
                }
                70 => columns = to_integer(pair.value).unwrap_or(1).max(1),
                71 => rows = to_integer(pair.value).unwrap_or(1).max(1),
                _ => {}
            }
        }
        if bad {
            self.warn("INSERT not imported: a value is not a number");
            return;
        }
        // The attributes: every one is a property of what the insert makes; a
        // visible one is also a Text, where the writing program put it.
        let mut attributes = PropertyMap::new();
        for child in record.own_children() {
            let mut tag = String::new();
            let mut value = String::new();
            for pair in &child.pairs {
                if pair.code == 2 {
                    tag = plain_text(trimmed_value(pair.value));
                } else if pair.code == 1 {
                    value = plain_text(pair.value);
                }
            }
            if !tag.is_empty() {
                attributes.insert(tag, PropertyValue::Text(value));
            }
        }
        for child in record.own_children() {
            let mut invisible = false;
            for pair in &child.pairs {
                if pair.code == 70 {
                    invisible = (to_integer(pair.value).unwrap_or(0) & 1) != 0;
                }
            }
            if !invisible {
                // The C++ hands a copy of the sink, which shares its block.
                self.convert_text(child, sink, true, Some(&attributes));
            }
        }
        let block = self.build_block(name, sink.depth + 1, "INSERT");
        let plain = is_plain_extrusion(&common.extrusion);
        let frame = if plain {
            Affine::default()
        } else {
            object_to_plan(&common.extrusion, z)
        };
        // The block's Z runs along the insert's extrusion: a block Z of zb is at
        // height (z + sz (zb - base Z)) times the extrusion's Z - up for the
        // plain one, down in an insert facing down.
        let level = is_level(&common.extrusion, &frame);
        let base_z = block.map_or(0.0, |index| self.blocks[index].base_z);
        let height_map = if level {
            let upward = normalised(&common.extrusion).z;
            Some(HeightMap {
                scale: sz * upward,
                offset: upward * (z - sz * base_z),
            })
        } else {
            self.warn("INSERT in a tilted plane, projected onto the plan without heights");
            None
        };
        let radians = rotation * DEGREES;
        let cosine = radians.cos();
        let sine = radians.sin();
        let nothing_drawn = block.is_none_or(|index| self.blocks[index].expanded.is_empty());
        if columns > 100_000 || rows > 100_000 || columns * rows > 100_000 {
            self.warn("MINSERT of more than 100 000 copies, only the first placed");
            columns = 1;
            rows = 1;
        }
        let base = block.map_or(Point2::new(0.0, 0.0), |index| self.blocks[index].base);
        for row in 0..rows {
            for column in 0..columns {
                // insertion + rotation * (scale * (p - base) + array offset)
                let offset = Vec2::new(column as f64 * column_spacing, row as f64 * row_spacing);
                let moved = Vec2::new(-sx * base.x + offset.x, -sy * base.y + offset.y);
                let local = Affine {
                    a: cosine * sx,
                    b: -sine * sy,
                    c: sine * sx,
                    d: cosine * sy,
                    tx: x + cosine * moved.x - sine * moved.y,
                    ty: y + sine * moved.x + cosine * moved.y,
                };
                let transform = frame.after(&local);
                if nothing_drawn {
                    if !attributes.is_empty() && row == 0 && column == 0 {
                        // The data of an insert that draws nothing - a block of
                        // attributes only - stays in the drawing on a point.
                        let mut entity = point_entity(transform.apply(base));
                        entity.properties = attributes.clone();
                        if z != 0.0 && level {
                            let heights = [Some(height_of(&common.extrusion, z))];
                            self.emit(entity, &common, sink, Some(heights.as_slice()), true);
                        } else {
                            self.emit(entity, &common, sink, None, level);
                        }
                    }
                    continue;
                }
                if let Some(index) = block {
                    self.place(index, &transform, height_map, &common, &attributes, sink);
                }
            }
        }
    }

    fn convert_dimension(&mut self, record: &Record<'a>, sink: &mut Sink<'a>) {
        let mut common = Common::default();
        let mut name = "";
        for pair in &record.pairs {
            if common.take(pair) {
                continue;
            }
            if pair.code == 2 {
                name = trimmed_value(pair.value);
            }
        }
        if name.is_empty() {
            self.warn("DIMENSION with no picture block, not imported");
            return;
        }
        // The picture block is drawn where it stands: its entities are already
        // in world coordinates.
        let Some(block) = self.build_block(name, sink.depth + 1, "DIMENSION") else {
            return;
        };
        let base = self.blocks[block].base;
        let transform = Affine {
            tx: -base.x,
            ty: -base.y,
            ..Affine::default()
        };
        self.place(
            block,
            &transform,
            Some(HeightMap {
                scale: 1.0,
                offset: 0.0,
            }),
            &common,
            &PropertyMap::new(),
            sink,
        );
    }

    fn run(mut self) -> Result<DxfImport> {
        let mut saw_section = false;
        let mut saw_eof = false;
        while let Some(pair) = self.tokens.next_pair() {
            if pair.code != 0 {
                continue;
            }
            let word = trimmed_value(pair.value);
            if word == "EOF" {
                saw_eof = true;
                break;
            }
            if word != "SECTION" {
                continue;
            }
            saw_section = true;
            let Some(pair) = self.tokens.next_pair() else {
                break;
            };
            let section = if pair.code == 2 {
                trimmed_value(pair.value)
            } else {
                ""
            };
            match section {
                "HEADER" => self.read_header(),
                "TABLES" => self.read_tables(),
                "BLOCKS" => self.read_blocks(),
                "ENTITIES" => self.read_entities(),
                _ => {
                    // CLASSES, OBJECTS, THUMBNAILIMAGE, ACDSDATA: nothing drawn.
                    let mut ignored = Record::default();
                    let mut end = false;
                    while !end && self.read_record(&mut ignored, &mut end) {}
                }
            }
        }
        if self.tokens.failed() {
            if !saw_section {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    format!("not a DXF file: {}", self.tokens.failure()),
                ));
            }
            return Err(Error::new(
                ErrorCode::ParseFailure,
                format!(
                    "the DXF file is damaged: {} - nothing after it can be read reliably",
                    self.tokens.failure()
                ),
            ));
        }
        if !saw_section {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "not a DXF file: it has no SECTION",
            ));
        }
        if !saw_eof {
            self.result
                .warnings
                .push(String::from(if self.tokens.truncated() {
                    "the file ends in the middle of a group: it is truncated, and what was read \
                 before the end is imported"
                } else {
                    "the file has no EOF marker: it may be truncated, and what was read is imported"
                }));
        }

        // Layers: the linetype each names resolved against LTYPE (continuous
        // where the table does not have it), in file order. Two DXF names can
        // become one path - " Kerb" and "Kerb" once the blank is trimmed - and
        // one layer is what they then are: the first stands.
        let mut listed: HashSet<String> = HashSet::new();
        for mut layer in std::mem::take(&mut self.layers) {
            if !listed.insert(layer.name.clone()) {
                continue;
            }
            if layer.linetype != CONTINUOUS_LINETYPE {
                let found = self.linetype_names.get(&upper(&layer.linetype)).cloned();
                match found {
                    Some(found) => layer.linetype = found,
                    None => {
                        self.warn(
                            "layer linetype the LTYPE table does not define, drawn continuous",
                        );
                        layer.linetype = String::from(CONTINUOUS_LINETYPE);
                    }
                }
            }
            self.result.layers.push(layer);
        }
        if let Some(shift) = self.options.origin_shift {
            for entity in &mut self.result.entities {
                translate(&mut entity.geometry, shift);
            }
        }
        for entity in &self.result.entities {
            self.result
                .bounds
                .expand_box(bounding_box(&entity.geometry));
        }
        if self.result.arcs_chorded != 0 {
            let tolerance = format_exact_real(self.options.curve_tolerance);
            let warning = format!(
                "{} polyline arc segments chorded to within {tolerance} (Katana polylines have \
                 no arc segments yet)",
                self.result.arcs_chorded
            );
            self.result.warnings.push(warning);
        }
        if !self.code_page.is_empty()
            && self.code_page != "ANSI_1252"
            && !self.code_page.starts_with("UTF")
        {
            // Recorded for the caller; the decoding decision was made on the bytes.
            let warning = format!(
                "the file declares code page {}; text that is not UTF-8 was read as Windows-1252",
                self.code_page
            );
            self.result.warnings.push(warning);
        }
        for (category, count) in &self.warning_counts {
            self.result.warnings.push(format!("{count} x {category}"));
        }
        Ok(self.result)
    }
}

/// The release an $ACADVER names: "AC1009" -> "R12", "AC1015" -> "R2000",
/// "AC1032" -> "R2018". Empty for one this does not know.
pub fn release_name(acadver: &str) -> &'static str {
    const RELEASES: [(&str, &str); 10] = [
        ("AC1006", "R10"),
        ("AC1009", "R12"),
        ("AC1012", "R13"),
        ("AC1014", "R14"),
        ("AC1015", "R2000"),
        ("AC1018", "R2004"),
        ("AC1021", "R2007"),
        ("AC1024", "R2010"),
        ("AC1027", "R2013"),
        ("AC1032", "R2018"),
    ];
    for (code, name) in RELEASES {
        if acadver == code {
            return name;
        }
    }
    ""
}

/// True for a path ending in .dxf, in any letter case: the one test the front
/// ends route by, so that the window and the command line agree on it.
pub fn is_dxf_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"dxf"))
}

/// `bytes` is the whole file. UTF-8 is used as it is; anything else is read as
/// Windows-1252, which every Western code page a $DWGCODEPAGE names is near
/// enough to. \U+XXXX escapes become the characters they name.
pub fn read_dxf(bytes: impl AsRef<[u8]>, options: &ImportOptions) -> Result<DxfImport> {
    let mut bytes = bytes.as_ref();
    if !(options.curve_tolerance > 0.0) || !options.curve_tolerance.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
        ));
    }
    // The sentinel every binary DXF begins with: the format's own bytes.
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "binary DXF is not read; save the drawing as ASCII DXF",
        ));
    }
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        bytes = rest;
    }
    // UTF-8 as it is (every release from R2007 writes it, and ASCII is UTF-8);
    // anything else is an older release in its code page.
    let decoded: String;
    let (text, recoded) = match std::str::from_utf8(bytes) {
        Ok(text) => (text, false),
        Err(_) => {
            decoded = decode_text_as(bytes, TextEncoding::Windows1252)?.text;
            (decoded.as_str(), true)
        }
    };
    let mut result = Reader::new(text, options).run();
    if recoded && let Ok(imported) = &mut result {
        imported.warnings.insert(
            0,
            String::from("the file is not UTF-8; it was read as Windows-1252"),
        );
    }
    result
}

pub fn read_dxf_file(path: &Path, options: &ImportOptions) -> Result<DxfImport> {
    let context = || utf8_or_windows1252(path_to_utf8(path));
    let mut file = File::open(path).map_err(|_| {
        make_error(
            ErrorCode::FileImportFailure,
            "cannot open the file",
            context(),
        )
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| {
        make_error(
            ErrorCode::FileImportFailure,
            "cannot read the file",
            context(),
        )
    })?;
    read_dxf(bytes.as_slice(), options).map_err(|mut error| {
        error.context = context();
        error
    })
}
