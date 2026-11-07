//! The contents of an archive file (.12da), as typed values.
//!
//! A .12da is the text interchange format of the archive format's own
//! modelling program: a stream of commands (`model`, `colour`, `style`,
//! `breakline`, `null`) and brace-delimited elements - strings, tins, super
//! tins, trimeshes. This module is the whole vocabulary of the format
//! chapter of the reference manual (V15), element for element; `coverage.rs`
//! lists them and a test holds that list against the reader so the two
//! cannot drift.
//!
//! TWO THINGS SHAPE THIS MODEL.
//!
//! 1. The program that writes the format writes far more than its manual
//!    documents. A real export carries `drawables`, `geometry_modifiers`,
//!    `equalities`, `extrude_value`, `vertex_uid_data`, `time_created`,
//!    `weight` and dozens more, and the parts of a super alignment other than
//!    IPs are declared undocumented by the manual itself. So nothing here
//!    pretends to be closed: a scalar `key value` the reader has no member for
//!    is kept, in order, in the element's `extras` (a FieldList), so nothing
//!    scalar is lost; a BLOCK it has no member for is counted by path in
//!    `Archive::unrecognised`, so an import can say exactly what it did not
//!    take rather than implying it took everything (PLAN.MD section 36).
//!
//! 2. The superseded string types ARE super strings. The manual says of 2d,
//!    3d, 4d, pipe and polyline strings that each "has been superseded by the
//!    super string", and a super string can express every one of them. They
//!    are read into the same `VertexString`, tagged with the kind they came
//!    from, so that everything downstream handles one shape of string, not
//!    eight.
//!
//! Coordinates are kept as written. `z` is optional because the format has a
//! null height - the `null` keyword, or the null value (-999 unless a `null`
//! command says otherwise) - and a null is not a zero.

use std::collections::BTreeMap;

use katana_core::text::{format_exact_real, parse_integer};

use crate::reader::parse_real;
use crate::text_utilities::parse_boolean;

// ---- attributes (manual section 1.3) ---------------------------------------

pub type AttributeList = Vec<Attribute>;

/// The C++ std::variant<std::int64_t, double, std::string, AttributeList>, in
/// its order. A group nests: `group { name "X" attributes { ... } }`.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Group(AttributeList),
}

impl Default for AttributeValue {
    // A default-constructed std::variant holds its first alternative.
    fn default() -> Self {
        Self::Integer(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub value: AttributeValue,
    /// The type keyword as written, when it is not one of the four the manual
    /// documents. Real exports carry `uid "this id" 5292`; keeping the word
    /// means it is written back as it was read.
    pub declared_type: String,
}

// ---- scalar fields ---------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    /// Lower case.
    pub key: String,
    pub value: String,
    /// Written in double quotes.
    pub quoted: bool,
}

/// An ordered `key value` record: a pit, a pipe, a plot frame, the properties
/// of an annotation, and the fields of any element that this model has no
/// member for. Ordered and duplicate-tolerant, because it has to reproduce
/// what was read; lookups take the LAST occurrence, which is what the format's
/// own program does with a repeated key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldList {
    fields: Vec<Field>,
}

impl FieldList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>, quoted: bool) {
        self.fields.push(Field {
            key: key.into(),
            value: value.into(),
            quoted,
        });
    }

    /// Shortest text that reads back as the same double: a field that has been
    /// through a FieldList must not come out a different number.
    pub fn set_real(&mut self, key: &str, value: f64) {
        self.add(key, format_exact_real(value), false);
    }

    pub fn set_integer(&mut self, key: &str, value: i64) {
        self.add(key, format!("{value}"), false);
    }

    pub fn set_text(&mut self, key: &str, value: impl Into<String>) {
        self.add(key, value, true);
    }

    /// Removes every occurrence of `key` and returns the last, which is how a
    /// reader moves a field it DOES have a member for out of the catch-all.
    pub fn take(&mut self, key: &str) -> Option<Field> {
        let mut taken = None;
        let mut index = 0;
        while index < self.fields.len() {
            if self.fields[index].key == key {
                taken = Some(self.fields.remove(index));
            } else {
                index += 1;
            }
        }
        taken
    }

    pub fn find(&self, key: &str) -> Option<&Field> {
        self.fields.iter().rev().find(|field| field.key == key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.find(key).is_some()
    }

    /// None when absent, null, or not a number. A quoted value is still taken
    /// as a number when it reads as one: `xstart "500005"` is a coordinate
    /// somebody quoted, and refusing it would place the arc nowhere. The
    /// `null` keyword, quoted or not, is no number.
    pub fn real(&self, key: &str) -> Option<f64> {
        parse_real(&self.find(key)?.value)
    }

    pub fn integer(&self, key: &str) -> Option<i64> {
        parse_integer(&self.find(key)?.value)
    }

    pub fn boolean(&self, key: &str) -> Option<bool> {
        parse_boolean(&self.find(key)?.value)
    }

    pub fn text(&self, key: &str, fallback: &str) -> String {
        match self.find(key) {
            Some(field) => field.value.clone(),
            None => String::from(fallback),
        }
    }

    pub fn empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn size(&self) -> usize {
        self.fields.len()
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
}

// ---- geometry --------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    /// None: a null height.
    pub z: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SegmentKind {
    #[default]
    Straight,
    Arc,
    Spiral,
    Curve,
    Parabola,
}

/// The geometry between two consecutive vertices (manual 1.5.8.2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Segment {
    pub kind: SegmentKind,
    /// Arc. SIGNED: a positive radius turns to the RIGHT travelling from the
    /// first vertex to the second, a negative one to the left. The manual's
    /// wording ("+ve is above the line connecting the vertices") is ambiguous
    /// about which way is up; this was settled against the writing program's
    /// own output, where all 18 arcs that follow a straight in
    /// `test Super Alignment.12da` agree - 10 positive all turning right, 8
    /// negative all turning left.
    pub radius: f64,
    /// Arc: take the larger of the two arcs of this radius between the
    /// vertices.
    pub major: bool,
    /// Spiral: type leading l1 r1 a1 l2 r2 a2. Curve: type leading xorigin
    /// yorigin radius length start end angle offset mvalue. Parabola (vertical
    /// geometry only): chainage height, the VIP of the curve.
    pub parameters: FieldList,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Breakline {
    Point,
    #[default]
    Line,
}

/// What every string carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StringHeader {
    pub name: String,
    /// Resolved: the string's own, else the current `model` command.
    pub model: String,
    /// Resolved likewise; a NAME - the RGB lives in the writing program's
    /// project.
    pub colour: String,
    pub style: String,
    pub chainage: f64,
    pub breakline: Breakline,
    pub attributes: AttributeList,
    /// Scalar fields with no member above: weight, time_created,
    /// time_updated, and whatever else the writing program wrote.
    pub extras: FieldList,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StringKind {
    #[default]
    Super,
    TwoD,
    ThreeD,
    FourD,
    Pipe,
    Polyline,
    Face,
    Interface,
}

pub fn to_string(kind: StringKind) -> &'static str {
    match kind {
        StringKind::Super => "super",
        StringKind::TwoD => "2d",
        StringKind::ThreeD => "3d",
        StringKind::FourD => "4d",
        StringKind::Pipe => "pipe",
        StringKind::Polyline => "polyline",
        StringKind::Face => "face",
        StringKind::Interface => "interface",
    }
}

/// `string super`, and every string type that is a list of vertices.
///
/// Per-vertex lists hold one entry per vertex; per-segment lists one per
/// segment - n-1 for an open string of n vertices, n for a closed one, and the
/// manual allows n for an open string too (the last is kept for when the
/// string is closed). A list that is empty means "not given", not "all
/// default".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VertexString {
    pub kind: StringKind,
    pub header: StringHeader,
    pub closed: bool,
    /// The one height of a `data_2d` string (`z value`) or a 2d string.
    pub constant_z: Option<f64>,
    pub vertices: Vec<Vertex>,
    /// Empty: every segment is straight.
    pub segments: Vec<Segment>,

    /// Vertex ids; alphanumeric.
    pub point_ids: Vec<String>,
    pub vertex_text_value: Option<String>,
    pub vertex_text: Vec<String>,
    pub segment_text_value: Option<String>,
    pub segment_text: Vec<String>,
    pub segment_colours: Vec<String>,

    /// Pipes and culverts: one size for the string, or one per segment. The
    /// manual says a string has one or the other; the writing program writes
    /// both on a culvert, and both are kept.
    pub diameter: Option<f64>,
    pub diameters: Vec<f64>,
    /// Width, height.
    pub culvert: Option<[f64; 2]>,
    pub culverts: Vec<[f64; 2]>,
    /// bottom|invert|top|obvert|centre; empty: not given.
    pub justify: String,

    pub vertex_tinable_value: Option<bool>,
    pub vertex_tinable: Vec<bool>,
    pub segment_tinable_value: Option<bool>,
    pub segment_tinable: Vec<bool>,
    pub vertex_visible_value: Option<bool>,
    pub vertex_visible: Vec<bool>,
    pub segment_visible_value: Option<bool>,
    pub segment_visible: Vec<bool>,

    /// How vertex and segment text is drawn: angle offset raise textstyle
    /// slant xfactor worldsize|papersize|screensize justify colour. A 4d
    /// string's string-level text properties arrive as `vertex_annotation`.
    pub vertex_annotation: Option<FieldList>,
    pub vertex_annotations: Vec<FieldList>,
    pub segment_annotation: Option<FieldList>,
    pub segment_annotations: Vec<FieldList>,
    /// style colour size rotation offset raise
    pub symbol: Option<FieldList>,
    pub symbols: Vec<FieldList>,

    pub vertex_attributes: Vec<AttributeList>,
    pub segment_attributes: Vec<AttributeList>,
    /// chord_arc distance
    pub interval: Option<FieldList>,

    /// Interface strings: -1 cut, 0 on the surface, 1 fill, per vertex.
    pub interface_modes: Vec<i32>,
}

impl VertexString {
    pub fn segment_count(&self) -> usize {
        if self.vertices.len() < 2 {
            return 0;
        }
        if self.closed {
            self.vertices.len()
        } else {
            self.vertices.len() - 1
        }
    }
}

/// `string arc`: a helix in general - the ends may differ in height.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArcString {
    pub header: StringHeader,
    /// Signed as Segment::radius is.
    pub radius: f64,
    pub centre: Vertex,
    pub start: Vertex,
    pub end: Vertex,
}

/// `string circle`, and `string feature` - a circle with a height at its
/// centre, which is what the format calls a feature.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CircleString {
    pub header: StringHeader,
    pub feature: bool,
    pub radius: f64,
    pub centre: Vertex,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextString {
    pub header: StringHeader,
    pub text: String,
    pub position: Vertex,
    /// angle offset raise textstyle slant xfactor
    /// worldsize|papersize|screensize justify, and the undocumented
    /// text_colour, border_style, whiteout.
    pub annotation: FieldList,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftingReference {
    pub name: String,
    pub fields: FieldList,
    pub blocks: Vec<DraftingReference>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftingLeader {
    pub header: StringHeader,
    pub arrow: Vertex,
    pub hook: Vertex,
    pub fields: FieldList,
    pub overrides: FieldList,
    pub arrow_reference: Option<DraftingReference>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlotFrame {
    pub header: StringHeader,
    /// title_file border viewport title_1 title_2 plot_file text_size
    /// sheet_code width height scale rotation xorigin yorigin *_margin plotter
    /// textstyle
    pub fields: FieldList,
}

/// One pit, pipe, property control or house connection of a drainage string.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrainageRecord {
    pub fields: FieldList,
    pub attributes: AttributeList,
    /// A property control has geometry of its own.
    pub vertices: Vec<Vertex>,
    pub segments: Vec<Segment>,
}

/// `string drainage` - the manual's water string.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrainageString {
    pub header: StringHeader,
    pub outfall: Option<f64>,
    /// 0: the string runs from downstream to upstream.
    pub flow_direction: Option<i32>,
    pub vertices: Vec<Vertex>,
    pub segments: Vec<Segment>,
    /// In order along the string.
    pub pits: Vec<DrainageRecord>,
    /// Written as pit_v2.
    pub pits_are_version2: bool,
    /// Pipe i joins pit i to pit i + 1.
    pub pipes: Vec<DrainageRecord>,
    pub property_controls: Vec<DrainageRecord>,
    pub house_connections: Vec<DrainageRecord>,
}

/// One construction step of a super alignment. For the IP method - the only
/// one the manual documents - `kind` is ip|arc|spiral horizontally and
/// ip|kvalue|length|radius|asymmetric|arc vertically. Any other kind is one of
/// the writing program's undocumented methods (computator,
/// floating_arc_end_radius_length ...): its scalar fields are kept and its
/// presence means the alignment is not defined by IPs alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlignmentPart {
    pub kind: String,
    pub fields: FieldList,
    pub attributes: AttributeList,
}

/// The solved geometry of a super alignment: horizontally (x, y) vertices,
/// vertically (chainage, height), with the segments between them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolvedGeometry {
    /// name chainage breakline colour style closed ...
    pub header: FieldList,
    pub closed: bool,
    pub vertices: Vec<Vertex>,
    /// Empty: every segment is straight.
    pub segments: Vec<Segment>,
    /// chord_arc distance
    pub interval: Option<FieldList>,
    /// The writing program colours the elements of a solved alignment by kind
    /// (straights one colour, arcs another) with the super string's
    /// per-segment block.
    pub segment_colours: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignmentSource {
    #[default]
    SuperAlignment,
    Alignment,
    Pipeline,
}

/// `string super_alignment`, and the superseded `alignment` and `pipeline`
/// strings, whose hipdata and vipdata ARE an IP-method definition and are read
/// into `horizontal_parts` and `vertical_parts` as one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SuperAlignment {
    pub source: AlignmentSource,
    pub header: StringHeader,
    pub closed: bool,
    /// Empty: not given, which the format reads as clothoid.
    pub spiral_type: String,
    pub valid_horizontal: Option<bool>,
    pub valid_vertical: Option<bool>,
    pub horizontal_parts: Vec<AlignmentPart>,
    pub vertical_parts: Vec<AlignmentPart>,
    pub horizontal_data: Option<SolvedGeometry>,
    pub vertical_data: Option<SolvedGeometry>,
    /// Pipeline strings only.
    pub diameter: Option<f64>,
    pub pipe_length: Option<f64>,
}

fn is_ip_part(part: &AlignmentPart, vertical: bool) -> bool {
    let kind = part.kind.as_str();
    if kind == "ip" || kind == "arc" {
        return true;
    }
    if vertical {
        return kind == "kvalue" || kind == "length" || kind == "radius" || kind == "asymmetric";
    }
    kind == "spiral"
}

impl SuperAlignment {
    /// True when every part is one of the documented IP-method kinds.
    pub fn horizontal_is_ip_only(&self) -> bool {
        !self.horizontal_parts.is_empty()
            && self
                .horizontal_parts
                .iter()
                .all(|part| is_ip_part(part, false))
    }

    pub fn vertical_is_ip_only(&self) -> bool {
        !self.vertical_parts.is_empty()
            && self
                .vertical_parts
                .iter()
                .all(|part| is_ip_part(part, true))
    }
}

/// One LAS point record. Fields the record format does not carry are zero;
/// `format` on the cloud says which are meaningful.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LasPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub intensity: u16,
    pub return_number: u8,
    pub return_count: u8,
    /// Formats 6+.
    pub classification_flags: u8,
    /// Formats 6+.
    pub scanner_channel: u8,
    pub scan_direction: u8,
    pub flight_line_edge: u8,
    pub classification: u8,
    pub scan_angle: i16,
    pub user_data: u8,
    pub point_source_id: u16,
    pub gps_time: f64,
    /// LAS colour packed by the format into 64 bits.
    pub colour: u64,
    pub near_infrared: u8,
}

/// `string las_cloud_data`: points inline, or a reference to a LAS file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LasCloud {
    pub header: StringHeader,
    /// v10_p0 ... v14_p10; empty for a reference.
    pub format: String,
    /// The Y of vX_pY.
    pub point_format: i32,
    pub categories: Vec<bool>,
    /// xmin xmax ymin ymax
    pub range: FieldList,
    pub points: Vec<LasPoint>,
    /// ref_data: file_name
    pub reference_file: String,
}

/// `tin` and `full_tin` (manual 1.4.7).
///
/// Indices are ZERO-BASED here; the file's are one-based. A `full_tin` begins
/// with four construction points (the corners of a rectangle around the data)
/// and lists every triangle including the nulled ones, with `nulling` saying
/// which are visible. A `tin` lists visible triangles only and has neither.
/// Triangles are CLOCKWISE seen from above, the opposite of
/// katana_terrain::TinSurface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tin {
    pub full: bool,
    pub name: String,
    pub colour: String,
    pub attributes: AttributeList,
    pub extras: FieldList,
    pub points: Vec<Vertex>,
    pub triangles: Vec<[u32; 3]>,
    /// full_tin only; 0 in the file (no neighbour) is NO_NEIGHBOUR here.
    pub neighbours: Vec<[u32; 3]>,
    /// full_tin only, one per triangle: true when the triangle is VISIBLE (the
    /// file's 2). Anything else in the file - 1, and the 0 the writing
    /// program's API reports for construction triangles - is not.
    pub visible: Vec<bool>,
    /// Per-triangle colour names; "-1" means the base colour.
    pub colours: Vec<String>,
    /// preserve_strings remove_bubbles weed_tin ...
    pub input: FieldList,
    pub input_models: Vec<String>,
}

impl Tin {
    pub const NO_NEIGHBOUR: u32 = 0xFFFF_FFFF;

    /// Whether triangle `index` is part of the surface: visible, and not a
    /// construction triangle of a full_tin (one touching its first four
    /// points).
    pub fn is_surface_triangle(&self, index: usize) -> bool {
        if index >= self.triangles.len() {
            return false;
        }
        if index < self.visible.len() && !self.visible[index] {
            return false;
        }
        if self.full {
            // Manual 1.4.7.1: "any triangle that contains any of the first four
            // points is a construction triangle". Only a full_tin has them; a
            // `tin` lists visible triangles and its first four points are data.
            let triangle = self.triangles[index];
            if triangle[0] < 4 || triangle[1] < 4 || triangle[2] < 4 {
                return false;
            }
        }
        true
    }
}

/// `super_tin`: an ordered list of tins by name. Later members override
/// earlier ones where they overlap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SuperTin {
    pub name: String,
    pub colour: String,
    pub attributes: AttributeList,
    pub extras: FieldList,
    pub tins: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrimeshInfo {
    pub flag: i64,
    pub key: i64,
    pub colour: String,
    pub name: String,
}

/// `primitive_3d { trimesh_3d { ... } }`: a closed or open mesh of triangles,
/// counter-clockwise seen from OUTSIDE. Indices zero-based here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trimesh {
    pub name: String,
    pub model: String,
    pub colour: String,
    pub attributes: AttributeList,
    pub extras: FieldList,
    pub vertices: Vec<Vertex>,
    pub faces: Vec<[u32; 3]>,
    pub edges: Vec<[u32; 2]>,
    /// flag key colour name
    pub info: FieldList,
    pub blend: Option<f64>,
    /// Optional per-element presentation: a table of infos, and for each
    /// vertex, edge or face a ONE-based index into it, 0 for none (the
    /// manual's example, 1.4.9: two infos, flags 2 0 1 2 0).
    pub vertex_infos: Vec<TrimeshInfo>,
    pub vertex_flags: Vec<u32>,
    pub edge_infos: Vec<TrimeshInfo>,
    pub edge_flags: Vec<u32>,
    pub face_infos: Vec<TrimeshInfo>,
    pub face_flags: Vec<u32>,
}

/// The C++ std::variant of the twelve element types, in its order.
#[allow(
    clippy::large_enum_variant,
    reason = "The C++ std::variant holds every element inline; an archive is read and written in place."
)]
#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    VertexString(VertexString),
    ArcString(ArcString),
    CircleString(CircleString),
    TextString(TextString),
    PlotFrame(PlotFrame),
    DrainageString(DrainageString),
    SuperAlignment(SuperAlignment),
    LasCloud(LasCloud),
    Tin(Tin),
    SuperTin(SuperTin),
    Trimesh(Trimesh),
    DraftingLeader(DraftingLeader),
}

macro_rules! element_from {
    ($($variant:ident),+ $(,)?) => {
        $(impl From<$variant> for Element {
            fn from(value: $variant) -> Self {
                Self::$variant(value)
            }
        })+
    };
}

element_from!(
    VertexString,
    ArcString,
    CircleString,
    TextString,
    PlotFrame,
    DrainageString,
    SuperAlignment,
    LasCloud,
    Tin,
    SuperTin,
    Trimesh,
    DraftingLeader,
);

/// The keyword an element is written under: "string super", "full_tin" ...
pub fn element_keyword(element: &Element) -> String {
    match element {
        Element::VertexString(string) => format!("string {}", to_string(string.kind)),
        Element::ArcString(_) => String::from("string arc"),
        Element::CircleString(circle) => String::from(if circle.feature {
            "string feature"
        } else {
            "string circle"
        }),
        Element::TextString(_) => String::from("string text"),
        Element::DraftingLeader(_) => String::from("drafting_element/leader"),
        Element::PlotFrame(_) => String::from("string plot_frame"),
        Element::DrainageString(_) => String::from("string drainage"),
        Element::SuperAlignment(alignment) => String::from(match alignment.source {
            AlignmentSource::Alignment => "string alignment",
            AlignmentSource::Pipeline => "string pipeline",
            AlignmentSource::SuperAlignment => "string super_alignment",
        }),
        Element::LasCloud(_) => String::from("string las_cloud_data"),
        Element::Tin(tin) => String::from(if tin.full { "full_tin" } else { "tin" }),
        Element::SuperTin(_) => String::from("super_tin"),
        Element::Trimesh(_) => String::from("primitive_3d"),
    }
}

/// A model that was declared with a block of its own (`model { name
/// attributes }`), which is how the format attaches attributes to a model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelRecord {
    pub name: String,
    pub attributes: AttributeList,
    pub extras: FieldList,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Archive {
    /// The `// key value` settings the writing program puts at the head of a
    /// file as comments: archive_version, decimal_places, output_hex_floats
    /// ... They are comments, so nothing depends on them, but a reader who
    /// wants to know which version wrote a file should not have to open it in
    /// an editor.
    pub header_settings: Vec<Field>,
    pub project_attributes: AttributeList,
    pub models: Vec<ModelRecord>,
    /// Every model named by a command or an element, in first-use order.
    pub model_names: Vec<String>,
    pub elements: Vec<Element>,
    pub null_value: f64,

    /// Blocks the reader has no member for, by path ("string super_alignment/
    /// drawables"), with how many times each was seen.
    pub unrecognised: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
}

impl Default for Archive {
    fn default() -> Self {
        Self {
            header_settings: Vec::new(),
            project_attributes: Vec::new(),
            models: Vec::new(),
            model_names: Vec::new(),
            elements: Vec::new(),
            null_value: -999.0,
            unrecognised: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }
}
