//! A MicroStation V7 design file -> drawing entities and their layers, with no
//! third-party library (docs/dgn.md).
//!
//! WHAT BECOMES WHAT.
//!
//! ```text
//!   Line                 a Line; one of zero length a Point - how dgnlib and
//!                        MicroStation write a point.
//!   Line String          a Polyline.
//!   Shape                a closed Polyline, its repeated closing vertex dropped.
//!   Complex Chain/Shape  ONE Polyline, or a CurvePolyline when a member is a
//!                        circular arc - each arc its bulge; an elliptical arc
//!                        member is chords, counted.
//!   Arc, Ellipse         an Arc or a Circle when circular, else an Ellipse -
//!                        true curves.
//!   Text                 a Text at its origin, the left end of its baseline.
//!   Text Node            its texts, each a Text.
//!   Cell                 its components, each tagged with the cell's name;
//!                        a hole shape of a cell (MicroStation's grouped hole)
//!                        and the shapes beside it tagged as the hole and its
//!                        area (source.ring, source.part) for the one ring rule.
//!   Curve                a Polyline through its points, counted: MicroStation
//!                        smooths between them.
//! ```
//!
//! A level (1-63) is a layer named "level N": a V7 file holds no level names.
//! An element's colour is its index in the file's colour table (its last one,
//! as dgnlib takes it), else in MicroStation's default table; its line style
//! and weight are kept as metadata (Katana has no per-entity field for
//! either). Heights: a 3D design's Z, where an element has any Z that is not
//! 0, since 0 is what a plan element of a 3D design holds (the DXF reader's
//! rule, docs/dxf.md).
//!
//! Everything else is counted in the tally and said: B-splines, surfaces,
//! solids, cones, point strings, shared cells, tags, reference attachments.
//! Deleted elements, which MicroStation keeps until a design is compressed,
//! are passed over and counted.
//!
//! WHAT IS REFUSED: a file that is no V7 design - empty, a V8 design (an OLE
//! compound document), a cell library, a DXF or a DWG named .dgn - each by
//! name, with what to do instead; and one whose design header is cut short.
//! Past the header, damage costs only the element it is in.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_entity::entity::set_heights;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{
    Entity, Geometry, Layer, PointGeometry, PropertyValue, TextGeometry, validate,
};
use katana_geometry::curves2d::bulge_from_sweep;
use katana_geometry::{
    Arc2, Box2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Vec2,
};
use katana_math::numerics::{DEG_TO_RAD, HALF_PI, TWO_PI, normalize_angle, tolerance};

use crate::codes::*;
use crate::plane::{InPlan, Xyz, quaternion_in_plan, rotation_in_plan};

// ---- the public interface -------------------------------------------------------

/// Written into every entity's metadata as `source`: the file's name.
pub const META_SOURCE: &str = "source";
/// The cell an entity was a component of, outermost first ("TREE/TRUNK").
pub const META_CELL: &str = "dgn.cell";
/// The element's colour index, beside the colour it named.
pub const META_COLOUR: &str = "dgn.colour";
/// The element's line style (0-7) and weight (0-31), where not 0: Katana has
/// no per-entity field for either; the writer puts them back.
pub const META_STYLE: &str = "dgn.style";
pub const META_WEIGHT: &str = "dgn.weight";
/// A text's font number and its justification code, for the writer to put
/// back.
pub const META_FONT: &str = "dgn.font";
pub const META_JUSTIFICATION: &str = "dgn.justification";

/// The ring tags of katana_entity::ring_tags: a hole of an area, and the part
/// both belong to.
const RING_KEY: &str = "source.ring";
const PART_KEY: &str = "source.part";

#[derive(Clone, Debug, PartialEq)]
pub struct ImportOptions {
    /// Largest distance a chord may stand off the curve it replaces, in model
    /// units: an elliptical arc inside a complex chain, which a polyline's
    /// bulges cannot hold.
    pub curve_tolerance: f64,
    /// Subtracted from every plan coordinate, as the DXF reader's is.
    pub origin_shift: Option<Vec2>,
    /// Written into every entity's metadata as `source`; empty writes nothing.
    pub source_name: String,
    /// Stops a hostile or broken file from exhausting memory: the import ends
    /// there, with a warning.
    pub maximum_entities: usize,
    /// Cells inside cells deeper than this are not read, and said so.
    pub maximum_depth: usize,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            curve_tolerance: 0.001,
            origin_shift: None,
            source_name: String::new(),
            maximum_entities: 20_000_000,
            maximum_depth: 16,
        }
    }
}

/// How many elements of one kind the file held and how many entities they
/// became.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DgnTally {
    pub kind: String,
    pub read: usize,
    pub imported: usize,
}

/// A design's working units: what its master and sub-units are called, and
/// how many integer steps - units of resolution - a master unit holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkingUnits {
    pub master: String,
    pub sub: String,
    pub subunits_per_master: i32,
    pub units_per_subunit: i32,
}

impl WorkingUnits {
    /// Units of resolution per master unit; 0 when the header gives none.
    pub fn per_master(&self) -> f64 {
        if self.subunits_per_master > 0 && self.units_per_subunit > 0 {
            f64::from(self.subunits_per_master) * f64::from(self.units_per_subunit)
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DgnImport {
    pub entities: Vec<Entity>,
    /// A layer for every level an entity is on, "level 1" before "level 2".
    pub layers: Vec<Layer>,
    pub bounds: Box2,
    pub three_d: bool,
    pub units: WorkingUnits,
    /// Where the design's 0,0,0 is on the integer plane, in units of
    /// resolution.
    pub global_origin: [f64; 3],
    /// Whether the file has a colour table of its own.
    pub colour_table: bool,
    /// Elements in the file, the design header included.
    pub elements: usize,
    pub tally: Vec<DgnTally>,
    /// Elliptical arcs in complex chains replaced by chords.
    pub arcs_chorded: usize,
    pub warnings: Vec<String>,
}

impl DgnImport {
    /// "DGN V7 2D" or "DGN V7 3D": what the reply calls the file.
    pub fn format(&self) -> &'static str {
        if self.three_d { "DGN V7 3D" } else { "DGN V7 2D" }
    }

    /// The step of a coordinate, in master units; 1 when the header gives no
    /// working units.
    pub fn resolution(&self) -> f64 {
        let per_master = self.units.per_master();
        if per_master > 0.0 { 1.0 / per_master } else { 1.0 }
    }
}

/// What a file's first bytes say it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    /// A V7 design file, its header's first word 08 09 FE 02 or, 3D,
    /// C8 09 FE 02 (dgnopen.cpp:40-44).
    Design,
    /// A V7 cell library: cells to place, not a drawing (dgnopen.cpp:35).
    CellLibrary,
    /// An OLE compound document: a MicroStation V8 design is one.
    CompoundDocument,
    /// A DXF, ASCII or binary.
    Dxf,
    /// A DWG: "AC10" and four digits, its version.
    Dwg,
    Empty,
    Other,
}

/// What `bytes` - a file's first bytes, or all of it - is.
pub fn identify(bytes: &[u8]) -> Content {
    if bytes.is_empty() {
        return Content::Empty;
    }
    if bytes.starts_with(&SIGNATURE_2D) || bytes.starts_with(&SIGNATURE_3D) {
        return Content::Design;
    }
    if bytes.starts_with(&SIGNATURE_CELL_LIBRARY) {
        return Content::CellLibrary;
    }
    if bytes.starts_with(&SIGNATURE_COMPOUND_DOCUMENT) {
        return Content::CompoundDocument;
    }
    if bytes.len() >= 6 && bytes.starts_with(b"AC10") && bytes[4..6].iter().all(u8::is_ascii_digit)
    {
        return Content::Dwg;
    }
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Content::Dxf;
    }
    // An ASCII DXF opens with the group code 0 and SECTION, blanks around.
    let head = &bytes[..bytes.len().min(64)];
    let text = String::from_utf8_lossy(head);
    let mut words = text.split_ascii_whitespace();
    if words.next() == Some("0") && words.next() == Some("SECTION") {
        return Content::Dxf;
    }
    Content::Other
}

/// The Katana layer a level is: "level 7". A V7 level is a number, 1-63,
/// and the file names none of them.
pub fn level_layer_name(level: u8) -> String {
    format!("level {level}")
}

/// The level a layer named as level_layer_name names it is; None for any
/// other name.
pub fn level_of_layer_name(name: &str) -> Option<u8> {
    let digits = name.strip_prefix("level ")?;
    if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let level: u8 = digits.parse().ok()?;
    (level <= 63).then_some(level)
}

/// True for a path ending in .dgn, in any letter case: the one test the front
/// ends route by, so that the window and the command line agree on it.
pub fn is_dgn_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"dgn"))
}

/// `bytes` is the whole file.
pub fn read_dgn(bytes: &[u8], options: &ImportOptions) -> Result<DgnImport> {
    if !(options.curve_tolerance.is_finite() && options.curve_tolerance > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
        ));
    }
    if let Some(shift) = options.origin_shift
        && !shift.is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the origin shift is not finite",
        ));
    }
    refuse_what_is_no_design(bytes)?;
    Reader::new(bytes, options)?.run()
}

pub fn read_dgn_file(path: &Path, options: &ImportOptions) -> Result<DgnImport> {
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
    read_dgn(&bytes, options).map_err(|mut error| {
        error.context = context();
        error
    })
}

/// Each kind of file that is no V7 design, refused by name with what to do.
fn refuse_what_is_no_design(bytes: &[u8]) -> Result<()> {
    let (code, message) = match identify(bytes) {
        Content::Design => return Ok(()),
        Content::Empty => (
            ErrorCode::ParseFailure,
            "the file is empty: a DGN design file begins with a 1536-byte design header",
        ),
        Content::CompoundDocument => (
            ErrorCode::Unsupported,
            "this is a MicroStation V8 DGN (an OLE compound document), and Katana reads V7 \
             design files only, since no open library reads V8 here: save it as V7 in \
             MicroStation (File > Save As, type V7), or export it there as DXF and import \
             the DXF",
        ),
        Content::CellLibrary => (
            ErrorCode::Unsupported,
            "this is a MicroStation cell library (.cel), cells to place rather than a \
             drawing: place its cells in a design file and import that",
        ),
        Content::Dxf => (
            ErrorCode::Unsupported,
            "this is a DXF file named .dgn: rename it .dxf and import it as DXF",
        ),
        Content::Dwg => (
            ErrorCode::Unsupported,
            "this is a DWG file named .dgn: rename it .dwg",
        ),
        Content::Other => (
            ErrorCode::ParseFailure,
            "not a DGN file: a V7 design file begins with its design header (a type 9 \
             element of 768 words), and this one does not",
        ),
    };
    Err(Error::new(code, message))
}

// ---- the design -----------------------------------------------------------------

/// One element as it is in the file: what the reader reads, and what the
/// writer copies of a seed.
#[derive(Clone, Copy)]
pub(crate) struct Element<'a> {
    pub(crate) offset: usize,
    pub(crate) kind: u8,
    pub(crate) level: u8,
    pub(crate) complex: bool,
    pub(crate) deleted: bool,
    pub(crate) bytes: &'a [u8],
}

impl Element<'_> {
    fn properties(&self) -> u16 {
        word(self.bytes, 32).unwrap_or(0)
    }

    /// Where the group this header begins ends: its total length counts the
    /// words after the 38 bytes before it (dgnwrite.cpp:1920), the header's
    /// own and its members'. None when the field is not there or is 0.
    fn group_end(&self) -> Option<usize> {
        let total = usize::from(word(self.bytes, 36)?);
        (total > 0).then_some(self.offset + 38 + 2 * total)
    }

    /// The 0xA9 0x51 attribute linkage of sub-unit corrections dgnlib applies
    /// to a line's or a line string's vertices (dgnread.cpp:416-431): the
    /// attribute bytes, and where the corrections begin in them.
    fn precision_deltas(&self) -> Option<(&[u8], usize)> {
        if self.properties() & PROPERTY_ATTRIBUTES == 0 {
            return None;
        }
        let index = usize::from(word(self.bytes, 30)?);
        let attributes = self.bytes.get(32 + 2 * index..)?;
        let found = attributes
            .windows(4)
            .position(|four| four[0] == 0xA9 && four[1] == 0x51)?;
        let length = 2 * usize::from(u16::from_le_bytes([attributes[found + 2], attributes[found + 3]]));
        (length > 0).then_some((attributes, found + 6))
    }
}

/// The display header's symbology (dgnread.cpp:1284-1291).
#[derive(Clone, Copy, Default)]
struct Symbology {
    style: u8,
    weight: u8,
    colour: u8,
    properties: u16,
}

fn symbology_of(element: &Element<'_>) -> Symbology {
    if element.bytes.len() < 36 {
        return Symbology::default();
    }
    Symbology {
        style: element.bytes[34] & 0x07,
        weight: (element.bytes[34] & 0xF8) >> 3,
        colour: element.bytes[35],
        properties: element.properties(),
    }
}

/// Whether elements of `kind` carry the display header (dgnread.cpp:1243
/// DGNElemTypeHasDispHdr): the drawing's elements do, the design's control
/// elements do not.
fn has_display_header(kind: u8) -> bool {
    !matches!(
        kind,
        0 | TYPE_DESIGN_HEADER
            | TYPE_CELL_LIBRARY
            | TYPE_LEVEL_SYMBOLOGY
            | 32
            | 44
            | 48..=51
            | 57
            | 60..=63
    )
}

/// Control elements: the design's own records, never drawn and not tallied.
fn is_control(kind: u8) -> bool {
    !has_display_header(kind)
        || matches!(
            kind,
            TYPE_GROUP_DATA | TYPE_DIGITIZER_SETUP | TYPE_APPLICATION | TYPE_CELL_LIBRARY
        )
}

/// The elements of the file in order, from the header on, up to the end of
/// the design; what was cut short is said in `warnings`.
pub(crate) fn split_elements<'a>(bytes: &'a [u8], warnings: &mut Vec<String>) -> Vec<Element<'a>> {
    let mut elements = Vec::new();
    let mut at = 0;
    loop {
        if at >= bytes.len() {
            warnings.push(String::from(
                "the file ends without the end-of-design word (FF FF): it may have been cut \
                 short at the end of an element",
            ));
            break;
        }
        if bytes[at..].starts_with(&END_OF_DESIGN) {
            break;
        }
        let Some(words) = word(bytes, at + 2) else {
            warnings.push(format!(
                "the file ends inside the header of element {} at byte {at}: it is cut short, \
                 and what was read before it is imported",
                elements.len()
            ));
            break;
        };
        let size = 4 + 2 * usize::from(words);
        let kind = bytes[at + 1] & 0x7F;
        if at + size > bytes.len() {
            warnings.push(format!(
                "element {} ({}) at byte {at} runs past the end of the file: the file is cut \
                 short, and what was read before it is imported",
                elements.len(),
                element_name(kind)
            ));
            break;
        }
        elements.push(Element {
            offset: at,
            kind,
            level: bytes[at] & 0x3F,
            complex: bytes[at] & 0x80 != 0,
            deleted: bytes[at + 1] & 0x80 != 0,
            bytes: &bytes[at..at + size],
        });
        at += size;
    }
    elements
}

/// How the integers and reals of the file become drawing coordinates.
struct Design {
    three_d: bool,
    /// Units of resolution per master unit; 1 when the header gives none.
    per_master: f64,
    origin: [f64; 3],
    shift: Vec2,
}

impl Design {
    fn point_size(&self) -> usize {
        if self.three_d { 12 } else { 8 }
    }

    /// A point of the integer plane, sub-unit corrections added, in master
    /// units: (uor - origin) / per master (dgnread.cpp:1664 DGNTransformPoint,
    /// written as one division so that a coordinate on a decimal step comes
    /// back as the nearest double to it).
    fn world(&self, uor: [f64; 3]) -> Xyz {
        Xyz::new(
            (uor[0] - self.origin[0]) / self.per_master - self.shift.x,
            (uor[1] - self.origin[1]) / self.per_master - self.shift.y,
            if self.three_d {
                (uor[2] - self.origin[2]) / self.per_master
            } else {
                0.0
            },
        )
    }

    fn length(&self, uor: f64) -> f64 {
        uor / self.per_master
    }

    /// The integer point at `at`: two coordinates, or three in a 3D design.
    fn integer_point(&self, bytes: &[u8], at: usize) -> Option<[f64; 3]> {
        let x = f64::from(int32(bytes, at)?);
        let y = f64::from(int32(bytes, at + 4)?);
        let z = if self.three_d {
            f64::from(int32(bytes, at + 8)?)
        } else {
            0.0
        };
        Some([x, y, z])
    }

    /// The point of VAX reals at `at` (an arc's or ellipse's origin).
    fn real_point(&self, bytes: &[u8], at: usize) -> Option<[f64; 3]> {
        let x = vax_double(bytes, at)?;
        let y = vax_double(bytes, at + 8)?;
        let z = if self.three_d {
            vax_double(bytes, at + 16)?
        } else {
            0.0
        };
        Some([x, y, z])
    }

    /// An element's orientation: its 2D rotation, or its 3D quaternion at
    /// `at`; None when tilted out of the plan or not there.
    fn in_plan(&self, bytes: &[u8], at: usize) -> Option<Option<InPlan>> {
        if self.three_d {
            let mut quaternion = [0_i32; 4];
            for (index, value) in quaternion.iter_mut().enumerate() {
                *value = int32(bytes, at + 4 * index)?;
            }
            Some(quaternion_in_plan(quaternion))
        } else {
            Some(Some(rotation_in_plan(int32(bytes, at)?)))
        }
    }
}

fn plan(point: Xyz) -> Point2 {
    Point2::new(point.x, point.y)
}

/// An arc or an ellipse as the file holds it, in master units and radians:
/// its centre, its primary axis's half-length along its own x axis and its
/// secondary's along y, and the parametric start and sweep.
struct Conic {
    centre: Xyz,
    primary: f64,
    secondary: f64,
    in_plan: InPlan,
    start: f64,
    sweep: f64,
}

impl Conic {
    fn circular(&self) -> bool {
        (self.primary - self.secondary).abs() <= 1e-12 * self.primary.max(self.secondary)
    }

    /// The start and sweep on the plan: a plane seen from below runs its
    /// angles clockwise, so they are negated (plane.rs).
    fn plan_angles(&self) -> (f64, f64) {
        if self.in_plan.mirrored {
            (-self.start, -self.sweep)
        } else {
            (self.start, self.sweep)
        }
    }

    fn is_full(&self) -> bool {
        self.sweep.abs() >= TWO_PI - tolerance::ANGULAR
    }

    fn as_arc(&self) -> Arc2 {
        let (start, sweep) = self.plan_angles();
        Arc2::new(
            plan(self.centre),
            self.primary,
            normalize_angle(self.in_plan.angle + start),
            sweep,
        )
    }

    /// Ellipse2 has its major axis first and a sweep above 0: a longer
    /// secondary axis is the major one, a quarter turn on, and a sweep below
    /// 0 is the same points from the other end.
    fn as_ellipse(&self) -> Ellipse2 {
        let (mut start, mut sweep) = self.plan_angles();
        let along = Vec2::new(self.in_plan.angle.cos(), self.in_plan.angle.sin());
        let (major_axis, ratio) = if self.primary >= self.secondary {
            (along * self.primary, self.secondary / self.primary)
        } else {
            start -= HALF_PI;
            (along.perpendicular() * self.secondary, self.primary / self.secondary)
        };
        if sweep < 0.0 {
            start += sweep;
            sweep = -sweep;
        }
        let full = sweep >= TWO_PI - tolerance::ANGULAR;
        Ellipse2 {
            center: plan(self.centre),
            major_axis,
            ratio,
            start_parameter: if full { 0.0 } else { normalize_angle(start) },
            sweep: if full { TWO_PI } else { sweep },
        }
    }
}

/// A piece of a complex chain or shape.
enum Piece {
    Points(Vec<Xyz>),
    Arc(Conic),
}

/// A cell being read: its name, where its components end, and the closed
/// shapes it holds so far, for the hole rule.
struct Cell {
    name: String,
    end: usize,
    part: String,
    shapes: Vec<usize>,
    holes: usize,
}

struct Reader<'a> {
    options: &'a ImportOptions,
    elements: Vec<Element<'a>>,
    design: Design,
    colours: [[u8; 3]; 256],
    result: DgnImport,
    tally_index: HashMap<String, usize>,
    levels: BTreeSet<u8>,
    cells: Vec<Cell>,
    stopped: bool,
    // What is said at the end, counted.
    deleted: usize,
    curves: usize,
    tilted: usize,
    invalid: usize,
    short: usize,
    references: usize,
    double_byte: usize,
    texts_cut: usize,
    members_not_read: usize,
    too_deep: usize,
    not_read: BTreeMap<String, usize>,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], options: &'a ImportOptions) -> Result<Self> {
        if bytes.len() < HEADER_BYTES {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                format!(
                    "the file ends inside its design header ({} of {HEADER_BYTES} bytes): it is \
                     cut short or damaged",
                    bytes.len()
                ),
            ));
        }
        let mut result = DgnImport::default();
        let elements = split_elements(bytes, &mut result.warnings);
        let header = elements.first().map(|element| element.bytes).unwrap_or_default();
        if header.len() < HEADER_BYTES {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "the design header is cut short: the file is damaged",
            ));
        }
        result.three_d = header[HEADER_DIMENSION] & HEADER_THREE_D != 0;
        result.units = WorkingUnits {
            master: unit_name(&header[HEADER_MASTER_NAME..HEADER_MASTER_NAME + 2]),
            sub: unit_name(&header[HEADER_SUB_NAME..HEADER_SUB_NAME + 2]),
            subunits_per_master: int32(header, HEADER_SUBUNITS_PER_MASTER).unwrap_or(0),
            units_per_subunit: int32(header, HEADER_UNITS_PER_SUBUNIT).unwrap_or(0),
        };
        let mut per_master = result.units.per_master();
        if per_master <= 0.0 {
            result.warnings.push(format!(
                "the design header gives no working units ({} sub-units per master unit, {} \
                 units of resolution per sub-unit): coordinates are read as units of resolution",
                result.units.subunits_per_master, result.units.units_per_subunit
            ));
            per_master = 1.0;
        }
        for (axis, origin) in result.global_origin.iter_mut().enumerate() {
            match vax_double(header, HEADER_GLOBAL_ORIGIN + 8 * axis) {
                Some(value) => *origin = value,
                None => result.warnings.push(String::from(
                    "the design header's global origin is not a number: it is taken as 0,0,0",
                )),
            }
        }
        let design = Design {
            three_d: result.three_d,
            per_master,
            origin: result.global_origin,
            shift: options.origin_shift.unwrap_or(Vec2::new(0.0, 0.0)),
        };
        // The colour table: the file's last one, as dgnlib takes it
        // (dgnread.cpp:1345-1351), for every element before it too.
        let mut colours = DEFAULT_COLOURS;
        for element in &elements {
            if element.kind == TYPE_GROUP_DATA
                && element.level == LEVEL_COLOUR_TABLE
                && !element.deleted
                && let Some(table) = colour_table(element.bytes)
            {
                colours = table;
                result.colour_table = true;
            }
        }
        result.elements = elements.len();
        Ok(Self {
            options,
            elements,
            design,
            colours,
            result,
            tally_index: HashMap::new(),
            levels: BTreeSet::new(),
            cells: Vec::new(),
            stopped: false,
            deleted: 0,
            curves: 0,
            tilted: 0,
            invalid: 0,
            short: 0,
            references: 0,
            double_byte: 0,
            texts_cut: 0,
            members_not_read: 0,
            too_deep: 0,
            not_read: BTreeMap::new(),
        })
    }

    fn tally(&mut self, kind: u8) -> &mut DgnTally {
        let name = element_name(kind);
        let index = match self.tally_index.get(&name) {
            Some(&index) => index,
            None => {
                let index = self.result.tally.len();
                self.tally_index.insert(name.clone(), index);
                self.result.tally.push(DgnTally {
                    kind: name,
                    read: 0,
                    imported: 0,
                });
                index
            }
        };
        &mut self.result.tally[index]
    }

    /// Leaves the cells whose components `element` is not one of: past their
    /// stated length, or not a component at all. A cell holding a hole tags
    /// its other closed shapes as the area the hole is in.
    fn leave_cells(&mut self, element: &Element<'_>) {
        while let Some(cell) = self.cells.last() {
            if element.offset < cell.end && element.complex {
                break;
            }
            if let Some(cell) = self.cells.pop()
                && cell.holes > 0
            {
                for &index in &cell.shapes {
                    let metadata = &mut self.result.entities[index].metadata;
                    metadata
                        .entry(String::from(RING_KEY))
                        .or_insert_with(|| PropertyValue::from("exterior"));
                    metadata.insert(String::from(PART_KEY), PropertyValue::from(cell.part.clone()));
                }
            }
        }
    }

    fn run(mut self) -> Result<DgnImport> {
        let mut index = 1; // the design header is element 0
        while index < self.elements.len() && !self.stopped {
            let element = self.elements[index];
            index += 1;
            self.leave_cells(&element);
            if element.deleted {
                self.deleted += 1;
                continue;
            }
            match element.kind {
                TYPE_GROUP_DATA if element.level == LEVEL_REFERENCE_FILE => self.references += 1,
                TYPE_CELL_HEADER => self.enter_cell(&element),
                TYPE_TEXT_NODE => {
                    // Its texts follow, each a component, and are read as texts.
                    let tally = self.tally(element.kind);
                    tally.read += 1;
                    tally.imported += 1;
                }
                TYPE_COMPLEX_CHAIN | TYPE_COMPLEX_SHAPE => {
                    index = self.read_complex(&element, index);
                }
                TYPE_LINE | TYPE_LINE_STRING | TYPE_SHAPE | TYPE_CURVE | TYPE_ELLIPSE
                | TYPE_ARC | TYPE_TEXT => {
                    self.tally(element.kind).read += 1;
                    if let Some((geometry, heights)) = self.read_simple(&element) {
                        self.keep(&element, geometry, &heights);
                    }
                }
                kind if is_control(kind) => {}
                kind => {
                    self.tally(kind).read += 1;
                    *self.not_read.entry(element_name(kind)).or_insert(0) += 1;
                }
            }
        }
        // The cells still open end with the design.
        let end = Element {
            offset: usize::MAX,
            kind: 0,
            level: 0,
            complex: false,
            deleted: false,
            bytes: &[],
        };
        self.leave_cells(&end);
        Ok(self.finish())
    }

    fn enter_cell(&mut self, element: &Element<'_>) {
        self.tally(element.kind).read += 1;
        if self.cells.len() >= self.options.maximum_depth {
            self.too_deep += 1;
            return;
        }
        let name = match (word(element.bytes, 38), word(element.bytes, 40)) {
            (Some(first), Some(second)) => {
                format!("{}{}", radix50_text(first), radix50_text(second))
                    .trim()
                    .to_owned()
            }
            _ => String::new(),
        };
        let end = element.group_end().unwrap_or(usize::MAX);
        self.cells.push(Cell {
            name,
            end,
            part: format!("cell {}", element.offset),
            shapes: Vec::new(),
            holes: 0,
        });
        self.tally(element.kind).imported += 1;
    }

    /// The cells an entity is inside, outermost first.
    fn cell_path(&self) -> String {
        let names: Vec<&str> = self
            .cells
            .iter()
            .map(|cell| cell.name.as_str())
            .filter(|name| !name.is_empty())
            .collect();
        names.join("/")
    }

    /// Heights of a 3D design's points: each Z, unless every one is 0, which
    /// is a plan element of a 3D design (the module comment).
    fn heights(&self, zs: &[f64]) -> Vec<Option<f64>> {
        if !self.design.three_d || zs.iter().all(|z| *z == 0.0) {
            return vec![None; zs.len()];
        }
        zs.iter().map(|z| Some(*z)).collect()
    }

    /// The entity `geometry` is, on `element`'s level and in its colour (a
    /// complex chain's header's, which its members share), kept when the
    /// drawing can hold it.
    fn keep(&mut self, element: &Element<'_>, geometry: Geometry, heights: &[Option<f64>]) {
        let symbology = symbology_of(element);
        let mut entity = Entity {
            geometry,
            layer: level_layer_name(element.level),
            color: Some(colour_at(&self.colours, symbology.colour)),
            ..Entity::default()
        };
        if heights.iter().any(Option::is_some) {
            set_heights(&mut entity.properties, heights);
        }
        let metadata = &mut entity.metadata;
        if !self.options.source_name.is_empty() {
            metadata.insert(
                String::from(META_SOURCE),
                PropertyValue::from(self.options.source_name.clone()),
            );
        }
        metadata.insert(
            String::from(META_COLOUR),
            PropertyValue::Integer(i64::from(symbology.colour)),
        );
        if symbology.style != 0 {
            metadata.insert(
                String::from(META_STYLE),
                PropertyValue::Integer(i64::from(symbology.style)),
            );
        }
        if symbology.weight != 0 {
            metadata.insert(
                String::from(META_WEIGHT),
                PropertyValue::Integer(i64::from(symbology.weight)),
            );
        }
        let cells = self.cell_path();
        if !cells.is_empty() {
            metadata.insert(String::from(META_CELL), PropertyValue::from(cells));
        }
        if let Geometry::Text(_) = &entity.geometry {
            if let Some(&font) = element.bytes.get(36) {
                metadata.insert(String::from(META_FONT), PropertyValue::Integer(i64::from(font)));
            }
            if let Some(&justification) = element.bytes.get(37)
                && justification != JUSTIFY_LEFT_BOTTOM
            {
                metadata.insert(
                    String::from(META_JUSTIFICATION),
                    PropertyValue::Integer(i64::from(justification)),
                );
            }
        }
        if validate(&entity.geometry).is_err() {
            self.invalid += 1;
            return;
        }
        let bounds = bounding_box(&entity.geometry);
        if !bounds.empty() {
            self.result.bounds.expand_box(bounds);
        }
        // A closed shape inside a cell: a hole of the cell's area when it
        // says so, the area itself when the cell holds a hole (leave_cells).
        let closed = match &entity.geometry {
            Geometry::Polyline(polyline) => polyline.closed,
            Geometry::CurvePolyline(polyline) => polyline.closed,
            _ => false,
        };
        let index = self.result.entities.len();
        if closed && let Some(cell) = self.cells.last_mut() {
            if symbology.properties & PROPERTY_HOLE != 0 {
                cell.holes += 1;
                entity
                    .metadata
                    .insert(String::from(RING_KEY), PropertyValue::from("hole"));
            }
            cell.shapes.push(index);
        }
        self.levels.insert(element.level);
        self.result.entities.push(entity);
        self.tally(element.kind).imported += 1;
        if self.result.entities.len() >= self.options.maximum_entities {
            self.stopped = true;
            self.result.warnings.push(format!(
                "the import stopped at {} entities, the most one import takes; the rest of the \
                 file was not read",
                self.options.maximum_entities
            ));
        }
    }

    // ---- the elements ---------------------------------------------------------

    /// A line's, line string's, shape's or curve's points, sub-unit
    /// corrections added (dgnread.cpp:452-464, 539-548).
    fn points(&mut self, element: &Element<'_>) -> Option<Vec<Xyz>> {
        let size = self.design.point_size();
        let (first, count) = if element.kind == TYPE_LINE {
            (36, 2)
        } else {
            let count = usize::from(word(element.bytes, 36)?);
            (38, count)
        };
        let available = element.bytes.len().saturating_sub(first) / size;
        let count = if count > available {
            self.short += 1;
            available
        } else {
            count
        };
        let deltas = element.precision_deltas();
        let mut points = Vec::with_capacity(count);
        for index in 0..count {
            let mut uor = self.design.integer_point(element.bytes, first + index * size)?;
            if let Some((attributes, start)) = deltas {
                let at = start + index * 4;
                if let Some(pair) = attributes.get(at..at + 4) {
                    uor[0] += f64::from(i16::from_le_bytes([pair[0], pair[1]])) / 32767.0;
                    uor[1] += f64::from(i16::from_le_bytes([pair[2], pair[3]])) / 32767.0;
                }
            }
            points.push(self.design.world(uor));
        }
        Some(points)
    }

    /// An arc's or an ellipse's parameters (dgnread.cpp:609-728), or None
    /// with the reason counted.
    fn conic(&mut self, element: &Element<'_>) -> Option<Conic> {
        let bytes = element.bytes;
        let (start, sweep, axes, orientation, origin) = if element.kind == TYPE_ARC {
            let start = f64::from(int32(bytes, 36)?) / ANGLE_STEPS_PER_DEGREE * DEG_TO_RAD;
            // The sweep is a sign and a magnitude, the sign in the high
            // word's top bit (dgnread.cpp:674-685); 0 is a whole turn.
            let mut raw = [0_u8; 4];
            raw.copy_from_slice(bytes.get(40..44)?);
            let negative = raw[1] & 0x80 != 0;
            raw[1] &= 0x7F;
            let magnitude = f64::from(int32(&raw, 0)?);
            let steps = if negative { -magnitude } else { magnitude };
            let sweep = if steps == 0.0 {
                TWO_PI
            } else {
                steps / ANGLE_STEPS_PER_DEGREE * DEG_TO_RAD
            };
            let origin = if self.design.three_d { 76 } else { 64 };
            (start, sweep, 44, 60, origin)
        } else {
            let origin = if self.design.three_d { 68 } else { 56 };
            (0.0, TWO_PI, 36, 52, origin)
        };
        let primary = self.design.length(vax_double(bytes, axes)?);
        let secondary = self.design.length(vax_double(bytes, axes + 8)?);
        let Some(in_plan) = self.design.in_plan(bytes, orientation)? else {
            self.tilted += 1;
            return None;
        };
        let centre = self.design.world(self.design.real_point(bytes, origin)?);
        if !(primary.is_finite() && secondary.is_finite() && primary > 0.0 && secondary > 0.0) {
            self.invalid += 1;
            return None;
        }
        Some(Conic {
            centre,
            primary,
            secondary,
            in_plan,
            start,
            sweep: sweep.clamp(-TWO_PI, TWO_PI),
        })
    }

    /// A line, line string, shape, curve, arc, ellipse or text, or None with
    /// the reason counted: an element shorter than its kind needs when no
    /// other reason was.
    fn read_simple(&mut self, element: &Element<'_>) -> Option<(Geometry, Vec<Option<f64>>)> {
        let before = self.tilted + self.invalid + self.short;
        let read = match element.kind {
            TYPE_LINE | TYPE_LINE_STRING | TYPE_SHAPE | TYPE_CURVE => self.read_linear(element),
            TYPE_ARC | TYPE_ELLIPSE => self.read_conic(element),
            _ => self.read_text(element),
        };
        if read.is_none() && self.tilted + self.invalid + self.short == before {
            self.short += 1;
        }
        read
    }

    fn read_linear(&mut self, element: &Element<'_>) -> Option<(Geometry, Vec<Option<f64>>)> {
        let mut points = self.points(element)?;
        if points.is_empty() {
            self.invalid += 1;
            return None;
        }
        if element.kind == TYPE_LINE {
            let heights = self.heights(&z_values(&points));
            let (first, last) = (points[0], points[points.len() - 1]);
            let geometry = line_or_point(plan(first), plan(last));
            let heights = if let Geometry::Point(_) = geometry {
                vec![heights[0]]
            } else {
                vec![heights[0], heights[heights.len() - 1]]
            };
            return Some((geometry, heights));
        }
        if element.kind == TYPE_CURVE {
            // The first two and last two points of a curve set its end
            // slopes; it passes through the rest (dgnstroke.cpp
            // DGNStrokeCurve).
            if points.len() < 6 {
                self.invalid += 1;
                return None;
            }
            self.curves += 1;
            points = points[2..points.len() - 2].to_vec();
        }
        let closed = element.kind == TYPE_SHAPE;
        if closed && points.len() > 2 && points.first() == points.last() {
            points.pop();
        }
        Some(linear_geometry(&points, closed, self.heights(&z_values(&points))))
    }

    fn read_conic(&mut self, element: &Element<'_>) -> Option<(Geometry, Vec<Option<f64>>)> {
        let conic = self.conic(element)?;
        let z = self.heights(&[conic.centre.z])[0];
        if conic.circular() {
            if conic.is_full() {
                let circle = Circle2::new(plan(conic.centre), conic.primary);
                return Some((Geometry::Circle(circle), vec![z]));
            }
            return Some((Geometry::Arc(conic.as_arc()), vec![z, z]));
        }
        Some((Geometry::Ellipse(conic.as_ellipse()), vec![z]))
    }

    /// A text (dgnread.cpp:730-805): its origin is the left end of its
    /// baseline, whatever its justification, which is kept for the writer.
    fn read_text(&mut self, element: &Element<'_>) -> Option<(Geometry, Vec<Option<f64>>)> {
        let bytes = element.bytes;
        let three_d = self.design.three_d;
        let height_steps = f64::from(int32(bytes, 42)?);
        let (orientation, origin, count_at) = if three_d { (46, 62, 74) } else { (46, 50, 58) };
        let in_plan = match self.design.in_plan(bytes, orientation)? {
            Some(in_plan) => in_plan,
            None => {
                // Read on the plan, turned as its baseline's projection is.
                self.tilted += 1;
                let mut quaternion = [0_i32; 4];
                for (index, value) in quaternion.iter_mut().enumerate() {
                    *value = int32(bytes, orientation + 4 * index)?;
                }
                tilted_baseline(quaternion)
            }
        };
        if in_plan.mirrored {
            self.tilted += 1;
        }
        let at = self.design.world(self.design.integer_point(bytes, origin)?);
        let count = usize::from(*bytes.get(count_at)?);
        let first = count_at + 2;
        let available = bytes.len().saturating_sub(first);
        if count > available {
            self.texts_cut += 1;
        }
        let raw = bytes.get(first..first + count.min(available))?;
        let text = self.decode_text(raw);
        let height = self.design.length(height_steps * TEXT_SIZE_STEP);
        let geometry = TextGeometry::new(plan(at), text, height, in_plan.angle);
        let z = self.heights(&[at.z]);
        Some((Geometry::Text(geometry), z))
    }

    /// A text's bytes: 8-bit text in the writer's code page - UTF-8 where the
    /// bytes are, else Windows-1252, MicroStation's for Western text - or,
    /// after the 0xFF 0xFD mark, 16-bit codes (dgnread.cpp:776-798), of which
    /// those below 256 are Latin-1 and the others a double-byte code page's,
    /// which is not read.
    fn decode_text(&mut self, raw: &[u8]) -> String {
        if raw.starts_with(&[0xFF, 0xFD]) {
            let mut text = String::new();
            let mut lost = false;
            for pair in raw[2..].as_chunks::<2>().0 {
                let code = u16::from_le_bytes([pair[0], pair[1]]);
                if code == 0 {
                    break;
                }
                if code < 256 {
                    text.push(char::from(code as u8));
                } else {
                    text.push(char::REPLACEMENT_CHARACTER);
                    lost = true;
                }
            }
            if lost {
                self.double_byte += 1;
            }
            return text;
        }
        let end = raw.iter().rposition(|&byte| byte != 0).map_or(0, |last| last + 1);
        utf8_or_windows1252(&raw[..end])
    }

    /// A complex chain or shape and its members: ONE polyline. Returns the
    /// index of the element after its members.
    fn read_complex(&mut self, header: &Element<'_>, next: usize) -> usize {
        self.tally(header.kind).read += 1;
        let (members, after) = self.collect_members(header, next, 0);
        let mut pieces = Vec::with_capacity(members.len());
        for member in members {
            let element = self.elements[member];
            match element.kind {
                TYPE_LINE | TYPE_LINE_STRING => match self.points(&element) {
                    Some(points) => pieces.push(Piece::Points(points)),
                    None => self.members_not_read += 1,
                },
                TYPE_CURVE => match self.points(&element) {
                    Some(points) if points.len() >= 6 => {
                        self.curves += 1;
                        pieces.push(Piece::Points(points[2..points.len() - 2].to_vec()));
                    }
                    _ => self.members_not_read += 1,
                },
                TYPE_ARC => match self.conic(&element) {
                    Some(conic) if conic.circular() => pieces.push(Piece::Arc(conic)),
                    Some(conic) => {
                        // An elliptical arc has no bulge: chords, counted.
                        let ellipse = conic.as_ellipse();
                        let mut points: Vec<Xyz> = ellipse
                            .tessellate(self.options.curve_tolerance)
                            .into_iter()
                            .map(|point| Xyz::new(point.x, point.y, conic.centre.z))
                            .collect();
                        // Tessellated in the ellipse's own direction; the
                        // chain runs the arc's.
                        if conic.plan_angles().1 < 0.0 {
                            points.reverse();
                        }
                        self.result.arcs_chorded += 1;
                        pieces.push(Piece::Points(points));
                    }
                    None => self.members_not_read += 1,
                },
                _ => self.members_not_read += 1,
            }
        }
        let closed = header.kind == TYPE_COMPLEX_SHAPE;
        if let Some((geometry, heights)) = self.assemble(pieces, closed) {
            self.keep(header, geometry, &heights);
        } else {
            self.invalid += 1;
        }
        after
    }

    /// The members of the complex header at `header`, from `next`: as many as
    /// it says it has, each a component, within its total length. A complex
    /// header among them (MicroStation nests chains) gives its own members in
    /// its place.
    fn collect_members(
        &mut self,
        header: &Element<'_>,
        next: usize,
        depth: usize,
    ) -> (Vec<usize>, usize) {
        let count = usize::from(word(header.bytes, 38).unwrap_or(0));
        let end = header.group_end().unwrap_or(usize::MAX);
        let mut members = Vec::new();
        let mut index = next;
        let mut taken = 0;
        while taken < count && index < self.elements.len() {
            let element = self.elements[index];
            if !element.complex || element.offset >= end {
                break;
            }
            index += 1;
            taken += 1;
            if element.deleted {
                continue;
            }
            if matches!(element.kind, TYPE_COMPLEX_CHAIN | TYPE_COMPLEX_SHAPE) {
                if depth >= self.options.maximum_depth {
                    self.too_deep += 1;
                    continue;
                }
                let (inner, after) = self.collect_members(&element, index, depth + 1);
                members.extend(inner);
                index = after;
            } else {
                members.push(index - 1);
            }
        }
        (members, index)
    }

    /// The pieces joined end to end: a piece that starts where the last ended
    /// shares its vertex (the integer one, where a line string's meets an
    /// arc's computed end), an arc gives its start vertex its bulge, and a
    /// closed one drops a last vertex that is its first.
    fn assemble(&self, pieces: Vec<Piece>, closed: bool) -> Option<(Geometry, Vec<Option<f64>>)> {
        // Two units of resolution: a line string's vertex is the integer
        // nearest the arc's end that MicroStation computed. An arc's end is
        // further from it by its angles' step: 1/360000 degree, 4.85e-8 rad,
        // half of it at most when rounded, so 1e-7 of its radius holds it
        // with room.
        let resolution = 2.0 / self.design.per_master;
        let same = |a: Point2, b: Point2, join: f64| {
            (a.x - b.x).abs() <= join && (a.y - b.y).abs() <= join
        };
        let mut vertices: Vec<CurveVertex> = Vec::new();
        let mut zs: Vec<f64> = Vec::new();
        // How far the last vertex may be from the next piece's start.
        let mut join = resolution;
        for piece in pieces {
            match piece {
                Piece::Points(points) => {
                    for (index, point) in points.iter().enumerate() {
                        let position = plan(*point);
                        if index == 0
                            && let Some(last) = vertices.last_mut()
                            && same(last.position, position, join)
                        {
                            last.position = position;
                            continue;
                        }
                        vertices.push(CurveVertex {
                            position,
                            bulge: 0.0,
                            height: None,
                        });
                        zs.push(point.z);
                    }
                    join = resolution;
                }
                Piece::Arc(conic) => {
                    let arc = conic.as_arc();
                    let from = arc.start_point();
                    let to = arc.end_point();
                    join = join.max(1e-7 * conic.primary);
                    let joined = vertices
                        .last()
                        .is_some_and(|last| same(last.position, from, join));
                    if !joined {
                        vertices.push(CurveVertex {
                            position: from,
                            bulge: 0.0,
                            height: None,
                        });
                        zs.push(conic.centre.z);
                    }
                    if let Some(last) = vertices.last_mut() {
                        last.bulge = bulge_from_sweep(arc.sweep);
                    }
                    vertices.push(CurveVertex {
                        position: to,
                        bulge: 0.0,
                        height: None,
                    });
                    zs.push(conic.centre.z);
                    join = resolution.max(1e-7 * conic.primary);
                }
            }
        }
        let ends_meet = vertices.len() > 2
            && same(
                vertices[0].position,
                vertices[vertices.len() - 1].position,
                join.max(resolution),
            );
        if closed && ends_meet {
            vertices.pop();
            zs.pop();
        }
        if vertices.len() < 2 {
            return None;
        }
        let heights = self.heights(&zs);
        if vertices.iter().any(|vertex| vertex.bulge != 0.0) {
            for (vertex, height) in vertices.iter_mut().zip(&heights) {
                vertex.height = *height;
            }
            return Some((
                Geometry::CurvePolyline(CurvePolyline2 { vertices, closed }),
                Vec::new(),
            ));
        }
        let points: Vec<Xyz> = vertices
            .iter()
            .zip(&zs)
            .map(|(vertex, z)| Xyz::new(vertex.position.x, vertex.position.y, *z))
            .collect();
        Some(linear_geometry(&points, closed, heights))
    }

    fn finish(mut self) -> DgnImport {
        self.result.layers = self
            .levels
            .iter()
            .map(|&level| Layer {
                name: level_layer_name(level),
                ..Layer::default()
            })
            .collect();
        let warnings = &mut self.result.warnings;
        if self.deleted != 0 {
            self.result.tally.push(DgnTally {
                kind: String::from("Deleted"),
                read: self.deleted,
                imported: 0,
            });
        }
        for (kind, count) in &self.not_read {
            warnings.push(format!("{count} {kind} elements are not read"));
        }
        if self.references != 0 {
            warnings.push(format!(
                "the design attaches {} reference files, which are not read: import each of \
                 them as well",
                self.references
            ));
        }
        if self.curves != 0 {
            warnings.push(format!(
                "{} curve elements were drawn as straight lines through their points, where \
                 MicroStation draws a smooth curve through them",
                self.curves
            ));
        }
        if self.result.arcs_chorded != 0 {
            warnings.push(format!(
                "{} elliptical arcs in complex chains were replaced by chords within {}",
                self.result.arcs_chorded,
                katana_core::text::format_exact_real(self.options.curve_tolerance)
            ));
        }
        if self.tilted != 0 {
            warnings.push(format!(
                "{} arcs, ellipses or texts lie in a plane tilted or turned over out of the \
                 plan: the arcs and ellipses were not read, the texts were placed on the plan",
                self.tilted
            ));
        }
        if self.short != 0 {
            warnings.push(format!(
                "{} elements were shorter than their kind needs and were read in part or not \
                 at all",
                self.short
            ));
        }
        if self.invalid != 0 {
            warnings.push(format!(
                "{} elements had geometry a drawing cannot hold - no length, no size, no text \
                 - and were left out",
                self.invalid
            ));
        }
        if self.members_not_read != 0 {
            warnings.push(format!(
                "{} members of complex chains or shapes are of a kind that is not read, or \
                 damaged, and were left out of their chain",
                self.members_not_read
            ));
        }
        if self.too_deep != 0 {
            warnings.push(format!(
                "{} cells or chains nested more than {} deep were not read",
                self.too_deep, self.options.maximum_depth
            ));
        }
        if self.texts_cut != 0 {
            warnings.push(format!(
                "{} texts were cut short by the end of their element",
                self.texts_cut
            ));
        }
        if self.double_byte != 0 {
            warnings.push(format!(
                "{} texts hold double-byte characters of a code page that is not read; each \
                 is shown as U+FFFD",
                self.double_byte
            ));
        }
        self.result
    }
}

/// A Point when every one of `points` (at least one) is the same on the
/// plan, else a Polyline: a line string of two points stays a polyline, as
/// it was drawn.
fn linear_geometry(
    points: &[Xyz],
    closed: bool,
    heights: Vec<Option<f64>>,
) -> (Geometry, Vec<Option<f64>>) {
    let first = points.first().copied().unwrap_or_default();
    let all_one = points
        .iter()
        .all(|point| plan(*point).distance_to(plan(first)) <= tolerance::GEOMETRIC);
    if all_one {
        let height = heights.first().copied().flatten();
        return (
            Geometry::Point(PointGeometry {
                position: plan(first),
            }),
            vec![height],
        );
    }
    let vertices: Vec<Point2> = points.iter().map(|point| plan(*point)).collect();
    (Geometry::Polyline(Polyline2::new(vertices, closed)), heights)
}

fn z_values(points: &[Xyz]) -> Vec<f64> {
    points.iter().map(|point| point.z).collect()
}

/// A text in a tilted plane, read on the plan: the angle of its baseline's
/// projection (the element's x axis, plane.rs), seen from above.
fn tilted_baseline(quaternion: [i32; 4]) -> InPlan {
    let [w, x, y, z] = quaternion.map(f64::from);
    let length = (w * w + x * x + y * y + z * z).sqrt().max(f64::MIN_POSITIVE);
    let (w, x, y, z) = (w / length, x / length, y / length, z / length);
    let along_x = 1.0 - 2.0 * (y * y + z * z);
    let along_y = 2.0 * (x * y - w * z);
    InPlan {
        angle: along_y.atan2(along_x),
        mirrored: false,
    }
}

/// A unit name, two bytes, blanks and NULs trimmed.
fn unit_name(bytes: &[u8]) -> String {
    let end = bytes.iter().rposition(|&byte| byte != 0).map_or(0, |last| last + 1);
    utf8_or_windows1252(&bytes[..end]).trim().to_owned()
}

/// A colour table element (dgnread.cpp:1330-1354): the background's three
/// bytes at 38, then colours 0-254 from 41. None when it is cut short.
fn colour_table(bytes: &[u8]) -> Option<[[u8; 3]; 256]> {
    let raw = bytes.get(38..38 + 768)?;
    let mut table = [[0_u8; 3]; 256];
    table[BACKGROUND_INDEX].copy_from_slice(&raw[0..3]);
    for (index, entry) in table.iter_mut().enumerate().take(BACKGROUND_INDEX) {
        entry.copy_from_slice(&raw[3 + 3 * index..6 + 3 * index]);
    }
    Some(table)
}

/// A line element: a Line, or a Point when it has no length - how dgnlib's
/// writer and MicroStation put a point (ogrdgnlayer.cpp:1268, "a degenerate
/// line").
fn line_or_point(a: Point2, b: Point2) -> Geometry {
    if a.distance_to(b) <= tolerance::GEOMETRIC {
        Geometry::Point(PointGeometry { position: a })
    } else {
        Geometry::Line(Segment2::new(a, b))
    }
}
