//! Entities, layers and linetypes -> a DXF file (R2000, ASCII), with no
//! third-party library.
//!
//! WHY A WRITER OF OUR OWN. GDAL's DXF driver took 116 s to write a 28 000
//! entity survey drawing (about 2 ms a feature, CPU-bound), wrote every closed
//! polyline and circle as a SOLID HATCH - a parcel arrived in the client's CAD
//! program as a filled black shape - chorded every arc, and skipped text.
//!
//! WHAT IS WRITTEN.
//!
//! ```text
//!   Point                     POINT, with its elevation as Z.
//!   Line                      LINE, heights as the two Z values.
//!   Arc, Circle               ARC, CIRCLE: true curves, never chords.
//!   Polyline                  LWPOLYLINE with the closed flag - NEVER a HATCH.
//!                             One whose vertices differ in height is a 3D
//!                             POLYLINE instead, since an LWPOLYLINE has one
//!                             elevation; one height becomes its elevation.
//!   Text                      TEXT, left of the baseline, height, rotation.
//!                             A Text holding line breaks is one TEXT a line.
//!   Dimension                 exploded: extension lines, the dimension line,
//!                             two ticks and the measurement as TEXT.
//! ```
//!
//! Layers go into the LAYER table with their colour, visibility, lock,
//! linetype and lineweight; linetypes with patterns into LTYPE. Colours, the
//! layers' and the entities', are the nearest of the 255 indexed colours:
//! R2000 has no true colour.
//!
//! WHAT THE FORMAT CANNOT HOLD goes beside the entity or layer as extended
//! data under the registered application KATANA, which other programs pass by
//! and this module's reader takes back (reader.rs, EXTENDED_PATH and after):
//! a layer's path - "/" is one of the characters the format refuses in a
//! symbol name, so "survey/kerb" is written "survey$kerb", the separator bound
//! references use (see layer_name_for); a colour no index is exactly; and the
//! heights of an entity known at only some of its vertices, which is written
//! in plan, since the format has no "no height" and a Z of 0 would be a false
//! level.
//!
//! Properties are not written: DXF has no place for them that other programs
//! read.
//!
//! Strings are ASCII: a character outside it is written \U+XXXX, which every
//! release from R2000 reads, and the degree, plus-minus and diameter signs as
//! the %%d %%p %%c every release reads.
//!
//! Every real is written as the C++'s std::to_chars writes it, which is
//! katana_core's format_exact_real: the shortest text that reads back as the
//! same double.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use katana_core::error::{Error, ErrorCode, Result, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text::format_exact_real;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_entity::annotation::{DEFAULT_TEXT_STYLE_NAME, annotation_model_size};
use katana_entity::dimension_text::dimension_label;
use katana_entity::entity::{heights_of, property_to_string};
use katana_entity::leader_values::leader_note;
use katana_entity::tables::{DEFAULT_DIMENSION_STYLE_NAME, DEFAULT_LAYER_NAME};
use katana_entity::text_block::estimated_text_corners;
use katana_entity::{
    Color, DimensionGeometry, DimensionKind, Entity, EntityId, Geometry, Layer, LeaderContext,
    LeaderGeometry, Linetype, Model, PropertyValue, TextGeometry, TextJustify,
};
use katana_geometry::{
    Box2, CurvePolyline2, CurveSegment, Ellipse2, Point2, Polyline2, Spline2, Vec2,
};
use katana_math::numerics::{PI, normalize_angle};

use crate::codes::{
    COLOUR_FOREGROUND, encode_text, exact_degrees, indexed_colour, nearest_indexed_colour,
    nearest_lineweight,
};
use crate::reader::{
    APPLICATION_NAME, EXTENDED_COLOUR, EXTENDED_HEIGHTS, EXTENDED_PATH, META_LINETYPE,
    META_LINEWEIGHT,
};
use katana_math::cpp;

// ---- the public interface -------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions<'a> {
    /// Empty exports the whole model; otherwise only these entities.
    pub entities: Vec<EntityId>,
    /// Empty exports every layer; otherwise only entities on these layers.
    pub layers: Vec<String>,
    /// Added back to every plan coordinate, undoing an import's origin_shift.
    pub origin_shift: Option<Vec2>,
    /// The scale paper-sized annotation is written at (docs/annotation.md): a
    /// 2.5 mm text is 2.5 x scale / 1000 model units in the file, since a DXF
    /// TEXT has one height. The application passes its annotation scale.
    pub annotation_scale: f64,
    /// Annotation the caller has drawn out for this export, by entity: an
    /// entity found here is written as these shapes - lines, polylines and
    /// single-line texts, on its layer and in its colour - instead of as
    /// itself. It is how a label (whose words and place are worked out for a
    /// view by the cad layer's placer, which this module may not see), a
    /// dimension of a kind other than aligned and a leader's arrowhead and
    /// callout frame reach the file: the cad layer's export annotation draws
    /// them, at annotation_scale, and the front ends pass them. An entry with
    /// no shapes writes nothing (a label with no room at the scale). None
    /// writes those kinds as before: labels and the other dimension kinds
    /// skipped with a warning, a leader as its line, landing and note.
    pub drawn: Option<&'a BTreeMap<EntityId, Vec<Geometry>>>,
}

impl Default for ExportOptions<'_> {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            layers: Vec::new(),
            origin_shift: None,
            annotation_scale: 1000.0,
            drawn: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DxfExport {
    /// The file (write_dxf); empty from write_dxf_file.
    pub text: String,
    /// Katana entities written.
    pub entities_written: usize,
    /// DXF entities: a Dimension is several.
    pub dxf_entities_written: usize,
    pub entities_skipped: usize,
    pub layers_written: usize,
    pub linetypes_written: usize,
    pub bytes_written: usize,
    pub warnings: Vec<String>,
}

// ---- the writer -----------------------------------------------------------------

const DEGREES_PER_RADIAN: f64 = 180.0 / PI;
const LINE_PITCH: f64 = 5.0 / 3.0; // as the reader: MTEXT's default pitch

// Handles of the objects every file has. A table's records, the layers, the
// linetypes and the entities take handles from FIRST_FREE_HANDLE on.
const BLOCK_RECORD_TABLE: u64 = 0x1;
const LAYER_TABLE: u64 = 0x2;
const STYLE_TABLE: u64 = 0x3;
const LINETYPE_TABLE: u64 = 0x5;
const VIEW_TABLE: u64 = 0x6;
const UCS_TABLE: u64 = 0x7;
const VIEWPORT_TABLE: u64 = 0x8;
const APPLICATION_TABLE: u64 = 0x9;
const DIMENSION_STYLE_TABLE: u64 = 0xA;
const ROOT_DICTIONARY: u64 = 0xC;
const GROUP_DICTIONARY: u64 = 0xD;
const PLOT_STYLE_DICTIONARY: u64 = 0xE;
const PLOT_STYLE_PLACEHOLDER: u64 = 0xF;
const LAYOUT_DICTIONARY: u64 = 0x10;
const MULTILINE_STYLE_DICTIONARY: u64 = 0x11;
const MULTILINE_STYLE: u64 = 0x12;
const ACTIVE_VIEWPORT: u64 = 0x13;
const LINETYPE_BY_BLOCK: u64 = 0x14;
const LINETYPE_BY_LAYER: u64 = 0x15;
const LINETYPE_CONTINUOUS: u64 = 0x16;
const STANDARD_STYLE: u64 = 0x17;
const APPLICATION_ACAD: u64 = 0x18;
const APPLICATION_KATANA: u64 = 0x19;
const STANDARD_DIMENSION_STYLE: u64 = 0x1A;
const MODEL_SPACE_RECORD: u64 = 0x1B;
const PAPER_SPACE_RECORD: u64 = 0x1C;
const MODEL_SPACE_BLOCK: u64 = 0x1D;
const MODEL_SPACE_END: u64 = 0x1E;
const PAPER_SPACE_BLOCK: u64 = 0x1F;
const PAPER_SPACE_END: u64 = 0x20;
const MODEL_LAYOUT: u64 = 0x21;
const PAPER_LAYOUT: u64 = 0x22;
const FIRST_FREE_HANDLE: u64 = 0x30;

/// A DXF name: at most 255 characters, none the format refuses, and unique
/// without regard to letter case, which is how a CAD program compares them.
#[derive(Default)]
struct SymbolNames {
    used: HashSet<String>,
}

impl SymbolNames {
    /// `name` is ASCII (symbol_name writes it through encode_text), so the
    /// cut at 240 bytes is a cut at 240 characters.
    fn unique(&mut self, mut name: String) -> String {
        if name.len() > 240 {
            name.truncate(240);
        }
        let mut candidate = name.clone();
        let mut copy = 2;
        while !self.used.insert(candidate.to_ascii_uppercase()) {
            candidate = format!("{name}~{copy}");
            copy += 1;
        }
        candidate
    }

    fn reserve(&mut self, name: &str) {
        self.used.insert(name.to_ascii_uppercase());
    }
}

fn symbol_name(name: &str, nested: bool) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c == '/' && nested {
                '$'
            } else if c.is_ascii_control()
                || matches!(
                    c,
                    '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | '=' | '`'
                )
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    if out.is_empty() {
        out = String::from("_");
    }
    encode_text(&out)
}

/// Every vertex known, at exactly 0: a level the plain groups write as the Z 0
/// that means "in plan", so it needs saying beside the entity (Writer::finish).
fn is_zero_level(heights: &[Option<f64>]) -> bool {
    !heights.is_empty() && heights.iter().all(|z| *z == Some(0.0))
}

/// The shortest text that reads back as exactly `value` (std::to_chars), with
/// a decimal point where it would have none, since some readers want one in a
/// real.
fn real_text(value: f64) -> String {
    let mut text = format_exact_real(value);
    if !text.contains(['.', 'e', 'E', 'n', 'i']) {
        text.push_str(".0");
    }
    text
}

/// std::error_code::message() is the system's text alone; io::Error's Display
/// adds " (os error N)" after it.
fn os_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error() {
        let suffix = format!(" (os error {code})");
        if let Some(message) = text.strip_suffix(&suffix) {
            return message.to_owned();
        }
    }
    text
}

struct Writer<'m> {
    model: &'m Model,
    options: &'m ExportOptions<'m>,
    shift: Vec2,
    body: String,
    handle: u64,
    extents: Box2,
    result: DxfExport,
    layer_names: HashMap<String, String>, // Katana path -> DXF
    linetype_names: HashMap<String, String>, // Katana -> DXF
    // The table records in the order they are written, with their DXF names.
    linetypes: Vec<(&'m Linetype, String)>,
    layers: Vec<(&'m Layer, String)>,
    partial_heights: usize,
    // Labels drawn out by the front end that drew nothing (no room at the
    // scale), counted for the warning.
    drawn_nothing: usize,
    // Labels, and dimensions of a kind other than aligned, not written
    // (write_geometry says why), for the export's warning.
    labels_skipped: usize,
    dimensions_skipped: usize,
    // The entity begun last has a colour no index is exactly.
    exact_colour: Option<Color>,
    last_colour_key: usize,
    last_colour_index: i32,
    have_last_colour: bool,
}

impl<'m> Writer<'m> {
    fn new(model: &'m Model, options: &'m ExportOptions<'m>) -> Self {
        Self {
            model,
            options,
            shift: options.origin_shift.unwrap_or(Vec2::new(0.0, 0.0)),
            body: String::new(),
            handle: FIRST_FREE_HANDLE,
            extents: Box2::default(),
            result: DxfExport::default(),
            layer_names: HashMap::new(),
            linetype_names: HashMap::new(),
            linetypes: Vec::new(),
            layers: Vec::new(),
            partial_heights: 0,
            drawn_nothing: 0,
            labels_skipped: 0,
            dimensions_skipped: 0,
            exact_colour: None,
            last_colour_key: 0,
            last_colour_index: COLOUR_FOREGROUND,
            have_last_colour: false,
        }
    }

    // ---- pairs ----

    fn code(&mut self, group_code: i32) {
        // Right-aligned in three columns, as the format's own writers do.
        let _ = writeln!(self.body, "{group_code:>3}");
    }

    fn text(&mut self, group_code: i32, value: &str) {
        self.code(group_code);
        self.body.push_str(value);
        self.body.push('\n');
    }

    fn integer(&mut self, group_code: i32, value: i64) {
        self.code(group_code);
        let _ = writeln!(self.body, "{value}");
    }

    /// The shortest text that reads back as exactly `value`, with a decimal
    /// point where it would have none, since some readers want one in a real.
    fn real(&mut self, group_code: i32, value: f64) {
        self.code(group_code);
        if value == 0.0 {
            self.body.push_str("0.0\n"); // and never "-0"
            return;
        }
        let written = real_text(value);
        self.body.push_str(&written);
        self.body.push('\n');
    }

    fn handle(&mut self, group_code: i32, value: u64) {
        self.code(group_code);
        let _ = writeln!(self.body, "{value:X}");
    }

    fn point(&mut self, group_code: i32, p: Point2, z: f64) {
        self.real(group_code, p.x + self.shift.x);
        self.real(group_code + 10, p.y + self.shift.y);
        self.real(group_code + 20, z);
        self.extents
            .expand(Point2::new(p.x + self.shift.x, p.y + self.shift.y));
    }

    fn next_handle(&mut self) -> u64 {
        let handle = self.handle;
        self.handle += 1;
        handle
    }

    // ---- sections ----

    /// Written last, into a string of its own, because $HANDSEED - the next
    /// free handle - is known only once everything else has one.
    fn header(&self) -> String {
        let mut out = String::new();
        let mut group = |group_code: &str, value: &str| {
            out.push_str(group_code);
            out.push('\n');
            out.push_str(value);
            out.push('\n');
        };
        let none = self.extents.empty();
        let low = if none {
            Point2::new(0.0, 0.0)
        } else {
            self.extents.min
        };
        let high = if none {
            Point2::new(0.0, 0.0)
        } else {
            self.extents.max
        };
        let handseed = format!("{:X}", self.handle);
        group("  0", "SECTION");
        group("  2", "HEADER");
        group("  9", "$ACADVER");
        group("  1", "AC1015");
        group("  9", "$ACADMAINTVER");
        group(" 70", "6");
        group("  9", "$DWGCODEPAGE");
        group("  3", "ANSI_1252");
        group("  9", "$INSBASE");
        group(" 10", "0.0");
        group(" 20", "0.0");
        group(" 30", "0.0");
        group("  9", "$EXTMIN");
        group(" 10", real_text(low.x).as_str());
        group(" 20", real_text(low.y).as_str());
        group(" 30", "0.0");
        group("  9", "$EXTMAX");
        group(" 10", real_text(high.x).as_str());
        group(" 20", real_text(high.y).as_str());
        group(" 30", "0.0");
        group("  9", "$LIMMIN");
        group(" 10", real_text(low.x).as_str());
        group(" 20", real_text(low.y).as_str());
        group("  9", "$LIMMAX");
        group(" 10", real_text(high.x).as_str());
        group(" 20", real_text(high.y).as_str());
        group("  9", "$LTSCALE");
        group(" 40", "1.0");
        group("  9", "$TEXTSIZE");
        group(" 40", "2.5");
        group("  9", "$TEXTSTYLE");
        group("  7", "Standard");
        group("  9", "$CLAYER");
        group("  8", "0");
        group("  9", "$CELTYPE");
        group("  6", "ByLayer");
        group("  9", "$CECOLOR");
        group(" 62", "256");
        group("  9", "$DIMSTYLE");
        group("  2", "Standard");
        group("  9", "$HANDSEED");
        group("  5", handseed.as_str());
        group("  9", "$MEASUREMENT");
        group(" 70", "1");
        // Metres: survey coordinates are, and a drawing with no unit set is
        // scaled by nothing when it is inserted into one that has.
        group("  9", "$INSUNITS");
        group(" 70", "6");
        group("  9", "$LWDISPLAY");
        group("290", "1");
        group("  0", "ENDSEC");
        // CLASSES: the object classes the OBJECTS section below uses.
        group("  0", "SECTION");
        group("  2", "CLASSES");
        for (dxf_name, class_name) in [
            ("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault"),
            ("ACDBPLACEHOLDER", "AcDbPlaceHolder"),
            ("LAYOUT", "AcDbLayout"),
        ] {
            group("  0", "CLASS");
            group("  1", dxf_name);
            group("  2", class_name);
            group("  3", "ObjectDBX Classes");
            group(" 90", "0");
            group("280", "0");
            group("281", "0");
        }
        group("  0", "ENDSEC");
        out
    }

    /// The DXF name of every linetype and layer, before anything is written:
    /// the entities, written first, name their layers, and the tables, written
    /// after them, list the same names under the same handles.
    fn assign_names(&mut self) {
        let model = self.model;
        let options = self.options;
        let mut linetype_symbols = SymbolNames::default();
        linetype_symbols.reserve("ByBlock");
        linetype_symbols.reserve("ByLayer");
        linetype_symbols.reserve("Continuous");
        for name in model.linetypes.names() {
            let Some(linetype) = model.linetypes.find(&name) else {
                continue;
            };
            if linetype.is_continuous() {
                continue;
            }
            let dxf_name = linetype_symbols.unique(symbol_name(&linetype.name, false));
            self.linetype_names
                .entry(linetype.name.clone())
                .or_insert_with(|| dxf_name.clone());
            self.linetypes.push((linetype, dxf_name));
        }
        // Every layer the drawing has, used or not - an empty layer is still part
        // of how the drawing is organised. Layer 0 first, under its own name.
        let only: BTreeSet<&str> = options.layers.iter().map(String::as_str).collect();
        let mut layer_symbols = SymbolNames::default();
        layer_symbols.reserve("0");
        for path in model.layers.names() {
            let zero = path == DEFAULT_LAYER_NAME;
            if !only.is_empty() && !zero && !only.contains(path.as_str()) {
                continue;
            }
            let Some(layer) = model.layers.find(&path) else {
                continue;
            };
            let dxf_name = if zero {
                String::from("0")
            } else {
                layer_symbols.unique(layer_name_for(&path))
            };
            self.layer_names
                .entry(path)
                .or_insert_with(|| dxf_name.clone());
            self.layers.push((layer, dxf_name));
        }
    }

    fn table_head(&mut self, name: &str, own: u64, count: i64) {
        self.text(0, "TABLE");
        self.text(2, name);
        self.handle(5, own);
        self.handle(330, 0);
        self.text(100, "AcDbSymbolTable");
        self.integer(70, count);
    }

    fn record(&mut self, record_type: &str, own: u64, table: u64, subclass: &str) {
        self.text(0, record_type);
        self.handle(5, own);
        self.handle(330, table);
        self.text(100, "AcDbSymbolTableRecord");
        self.text(100, subclass);
    }

    fn built_in_linetype(&mut self, name: &str, own: u64, text3: &str) {
        self.record("LTYPE", own, LINETYPE_TABLE, "AcDbLinetypeTableRecord");
        self.text(2, name);
        self.integer(70, 0);
        self.text(3, text3);
        self.integer(72, 65);
        self.integer(73, 0);
        self.real(40, 0.0);
    }

    fn write_tables(&mut self) {
        self.text(0, "SECTION");
        self.text(2, "TABLES");

        // VPORT: the one active viewport, looking at the extents.
        self.table_head("VPORT", VIEWPORT_TABLE, 1);
        self.record(
            "VPORT",
            ACTIVE_VIEWPORT,
            VIEWPORT_TABLE,
            "AcDbViewportTableRecord",
        );
        self.text(2, "*Active");
        self.integer(70, 0);
        self.real(10, 0.0);
        self.real(20, 0.0);
        self.real(11, 1.0);
        self.real(21, 1.0);
        let centre = if self.extents.empty() {
            Point2::new(0.0, 0.0)
        } else {
            self.extents.center()
        };
        self.real(12, centre.x);
        self.real(22, centre.y);
        self.real(13, 0.0);
        self.real(23, 0.0);
        self.real(14, 10.0);
        self.real(24, 10.0);
        self.real(15, 10.0);
        self.real(25, 10.0);
        self.real(16, 0.0);
        self.real(26, 0.0);
        self.real(36, 1.0);
        self.real(17, 0.0);
        self.real(27, 0.0);
        self.real(37, 0.0);
        let view_height = if self.extents.empty() {
            100.0
        } else {
            cpp::max(1.0, self.extents.height() * 1.1)
        };
        self.real(40, view_height);
        let aspect = if self.extents.empty() {
            1.5
        } else {
            (self.extents.width() / cpp::max(self.extents.height(), 1e-9)).clamp(0.1, 10.0)
        };
        self.real(41, aspect);
        self.real(42, 50.0);
        self.real(43, 0.0);
        self.real(44, 0.0);
        self.real(50, 0.0);
        self.real(51, 0.0);
        self.integer(71, 0);
        self.integer(72, 1000);
        self.integer(73, 1);
        self.integer(74, 3);
        self.integer(75, 0);
        self.integer(76, 0);
        self.integer(77, 0);
        self.integer(78, 0);
        self.text(0, "ENDTAB");

        // LTYPE: the three every file has, then the drawing's patterns.
        let linetypes = std::mem::take(&mut self.linetypes);
        self.table_head("LTYPE", LINETYPE_TABLE, 3 + linetypes.len() as i64);
        self.built_in_linetype("ByBlock", LINETYPE_BY_BLOCK, "");
        self.built_in_linetype("ByLayer", LINETYPE_BY_LAYER, "");
        self.built_in_linetype("Continuous", LINETYPE_CONTINUOUS, "Solid line");
        for (linetype, name) in &linetypes {
            let own = self.next_handle();
            self.record("LTYPE", own, LINETYPE_TABLE, "AcDbLinetypeTableRecord");
            self.text(2, name);
            self.integer(70, 0);
            self.text(3, &encode_text(&linetype.description));
            self.integer(72, 65);
            self.integer(73, linetype.pattern.len() as i64);
            self.real(40, linetype.pattern_length());
            for element in &linetype.pattern {
                self.real(49, element.length);
                self.integer(74, 0);
            }
            self.result.linetypes_written += 1;
        }
        self.text(0, "ENDTAB");

        let layers = std::mem::take(&mut self.layers);
        self.table_head("LAYER", LAYER_TABLE, layers.len() as i64);
        for (layer, name) in &layers {
            let own = self.next_handle();
            self.record("LAYER", own, LAYER_TABLE, "AcDbLayerTableRecord");
            self.text(2, name);
            self.integer(70, if layer.locked { 4 } else { 0 });
            let colour = nearest_indexed_colour(layer.color);
            self.integer(62, i64::from(if layer.visible { colour } else { -colour }));
            let linetype = self
                .linetype_names
                .get(&layer.linetype)
                .cloned()
                .unwrap_or_else(|| String::from("Continuous"));
            self.text(6, &linetype);
            self.integer(370, i64::from(nearest_lineweight(layer.line_weight)));
            self.handle(390, PLOT_STYLE_PLACEHOLDER);
            // The path, where the name could not hold it, and the colour, where
            // no index is it, for this reader to take back: other programs pass
            // extended data by without reading it.
            let exact = indexed_colour(colour) == Some(layer.color);
            if *name != layer.name || !exact {
                self.text(1001, APPLICATION_NAME);
                if *name != layer.name {
                    self.text(1000, EXTENDED_PATH);
                    self.text(1000, &encode_text(&layer.name));
                }
                if !exact {
                    self.text(1000, EXTENDED_COLOUR);
                    self.text(1000, &layer.color.to_hex());
                }
            }
            self.result.layers_written += 1;
        }
        self.text(0, "ENDTAB");

        self.table_head("STYLE", STYLE_TABLE, 1);
        self.record(
            "STYLE",
            STANDARD_STYLE,
            STYLE_TABLE,
            "AcDbTextStyleTableRecord",
        );
        self.text(2, "Standard");
        self.integer(70, 0);
        self.real(40, 0.0);
        self.real(41, 1.0);
        self.real(50, 0.0);
        self.integer(71, 0);
        self.real(42, 2.5);
        self.text(3, "txt");
        self.text(4, "");
        self.text(0, "ENDTAB");

        self.table_head("VIEW", VIEW_TABLE, 0);
        self.text(0, "ENDTAB");
        self.table_head("UCS", UCS_TABLE, 0);
        self.text(0, "ENDTAB");

        self.table_head("APPID", APPLICATION_TABLE, 2);
        self.record(
            "APPID",
            APPLICATION_ACAD,
            APPLICATION_TABLE,
            "AcDbRegAppTableRecord",
        );
        self.text(2, "ACAD");
        self.integer(70, 0);
        self.record(
            "APPID",
            APPLICATION_KATANA,
            APPLICATION_TABLE,
            "AcDbRegAppTableRecord",
        );
        self.text(2, APPLICATION_NAME);
        self.integer(70, 0);
        self.text(0, "ENDTAB");

        self.table_head("DIMSTYLE", DIMENSION_STYLE_TABLE, 1);
        self.text(100, "AcDbDimStyleTable");
        self.text(0, "DIMSTYLE");
        self.handle(105, STANDARD_DIMENSION_STYLE); // a dimension style's handle is 105, not 5
        self.handle(330, DIMENSION_STYLE_TABLE);
        self.text(100, "AcDbSymbolTableRecord");
        self.text(100, "AcDbDimStyleTableRecord");
        self.text(2, "Standard");
        self.integer(70, 0);
        self.real(40, 1.0);
        self.real(41, 2.5);
        self.real(42, 0.625);
        self.real(43, 3.75);
        self.real(44, 1.25);
        self.real(140, 2.5);
        self.real(141, 2.5);
        self.real(147, 0.625);
        self.integer(77, 1);
        self.integer(271, 3);
        self.text(0, "ENDTAB");

        self.table_head("BLOCK_RECORD", BLOCK_RECORD_TABLE, 2);
        self.record(
            "BLOCK_RECORD",
            MODEL_SPACE_RECORD,
            BLOCK_RECORD_TABLE,
            "AcDbBlockTableRecord",
        );
        self.text(2, "*Model_Space");
        self.handle(340, MODEL_LAYOUT);
        self.record(
            "BLOCK_RECORD",
            PAPER_SPACE_RECORD,
            BLOCK_RECORD_TABLE,
            "AcDbBlockTableRecord",
        );
        self.text(2, "*Paper_Space");
        self.handle(340, PAPER_LAYOUT);
        self.text(0, "ENDTAB");
        self.text(0, "ENDSEC");
    }

    fn block(&mut self, name: &str, begin: u64, end: u64, owner: u64) {
        self.text(0, "BLOCK");
        self.handle(5, begin);
        self.handle(330, owner);
        self.text(100, "AcDbEntity");
        self.text(8, "0");
        self.text(100, "AcDbBlockBegin");
        self.text(2, name);
        self.integer(70, 0);
        self.real(10, 0.0);
        self.real(20, 0.0);
        self.real(30, 0.0);
        self.text(3, name);
        self.text(1, "");
        self.text(0, "ENDBLK");
        self.handle(5, end);
        self.handle(330, owner);
        self.text(100, "AcDbEntity");
        self.text(8, "0");
        self.text(100, "AcDbBlockEnd");
    }

    fn write_blocks(&mut self) {
        self.text(0, "SECTION");
        self.text(2, "BLOCKS");
        self.block(
            "*Model_Space",
            MODEL_SPACE_BLOCK,
            MODEL_SPACE_END,
            MODEL_SPACE_RECORD,
        );
        self.block(
            "*Paper_Space",
            PAPER_SPACE_BLOCK,
            PAPER_SPACE_END,
            PAPER_SPACE_RECORD,
        );
        self.text(0, "ENDSEC");
    }

    fn dictionary(&mut self, own: u64, owner: u64) {
        self.text(0, "DICTIONARY");
        self.handle(5, own);
        self.handle(330, owner);
        self.text(100, "AcDbDictionary");
        self.integer(281, 1);
    }

    fn layout(&mut self, own: u64, name: &str, flags: i64, order: i64, record: u64) {
        self.text(0, "LAYOUT");
        self.handle(5, own);
        self.handle(330, LAYOUT_DICTIONARY);
        self.text(100, "AcDbPlotSettings");
        self.text(1, "");
        self.text(4, "A3");
        self.text(6, "");
        self.real(40, 7.5);
        self.real(41, 20.0);
        self.real(42, 7.5);
        self.real(43, 20.0);
        self.real(44, 420.0);
        self.real(45, 297.0);
        self.real(46, 0.0);
        self.real(47, 0.0);
        self.real(48, 0.0);
        self.real(49, 0.0);
        self.real(140, 0.0);
        self.real(141, 0.0);
        self.real(142, 1.0);
        self.real(143, 1.0);
        self.integer(70, flags);
        self.integer(72, 1);
        self.integer(73, 0);
        self.integer(74, 5);
        self.text(7, "");
        self.integer(75, 16);
        self.integer(76, 0);
        self.integer(77, 2);
        self.integer(78, 300);
        self.real(147, 1.0);
        self.real(148, 0.0);
        self.real(149, 0.0);
        self.text(100, "AcDbLayout");
        self.text(1, name);
        self.integer(70, 1);
        self.integer(71, order);
        self.real(10, 0.0);
        self.real(20, 0.0);
        self.real(11, 420.0);
        self.real(21, 297.0);
        self.real(12, 0.0);
        self.real(22, 0.0);
        self.real(32, 0.0);
        self.real(14, 1e20);
        self.real(24, 1e20);
        self.real(34, 1e20);
        self.real(15, -1e20);
        self.real(25, -1e20);
        self.real(35, -1e20);
        self.real(146, 0.0);
        self.real(13, 0.0);
        self.real(23, 0.0);
        self.real(33, 0.0);
        self.real(16, 1.0);
        self.real(26, 0.0);
        self.real(36, 0.0);
        self.real(17, 0.0);
        self.real(27, 1.0);
        self.real(37, 0.0);
        self.integer(76, 1);
        self.handle(330, record);
    }

    fn write_objects(&mut self) {
        self.text(0, "SECTION");
        self.text(2, "OBJECTS");
        self.dictionary(ROOT_DICTIONARY, 0);
        self.text(3, "ACAD_GROUP");
        self.handle(350, GROUP_DICTIONARY);
        self.text(3, "ACAD_LAYOUT");
        self.handle(350, LAYOUT_DICTIONARY);
        self.text(3, "ACAD_MLINESTYLE");
        self.handle(350, MULTILINE_STYLE_DICTIONARY);
        self.text(3, "ACAD_PLOTSTYLENAME");
        self.handle(350, PLOT_STYLE_DICTIONARY);
        self.dictionary(GROUP_DICTIONARY, ROOT_DICTIONARY);
        self.dictionary(LAYOUT_DICTIONARY, ROOT_DICTIONARY);
        self.text(3, "Layout1");
        self.handle(350, PAPER_LAYOUT);
        self.text(3, "Model");
        self.handle(350, MODEL_LAYOUT);
        self.dictionary(MULTILINE_STYLE_DICTIONARY, ROOT_DICTIONARY);
        self.text(3, "Standard");
        self.handle(350, MULTILINE_STYLE);
        self.text(0, "ACDBDICTIONARYWDFLT");
        self.handle(5, PLOT_STYLE_DICTIONARY);
        self.handle(330, ROOT_DICTIONARY);
        self.text(100, "AcDbDictionary");
        self.integer(281, 1);
        self.text(3, "Normal");
        self.handle(350, PLOT_STYLE_PLACEHOLDER);
        self.text(100, "AcDbDictionaryWithDefault");
        self.handle(340, PLOT_STYLE_PLACEHOLDER);
        self.text(0, "ACDBPLACEHOLDER");
        self.handle(5, PLOT_STYLE_PLACEHOLDER);
        self.handle(330, PLOT_STYLE_DICTIONARY);
        self.text(0, "MLINESTYLE");
        self.handle(5, MULTILINE_STYLE);
        self.handle(330, MULTILINE_STYLE_DICTIONARY);
        self.text(100, "AcDbMlineStyle");
        self.text(2, "Standard");
        self.integer(70, 0);
        self.text(3, "");
        self.integer(62, 256);
        self.real(51, 90.0);
        self.real(52, 90.0);
        self.integer(71, 2);
        self.real(49, 0.5);
        self.integer(62, 256);
        self.text(6, "BYLAYER");
        self.real(49, -0.5);
        self.integer(62, 256);
        self.text(6, "BYLAYER");
        self.layout(MODEL_LAYOUT, "Model", 1024, 0, MODEL_SPACE_RECORD);
        self.layout(PAPER_LAYOUT, "Layout1", 0, 1, PAPER_SPACE_RECORD);
        self.text(0, "ENDSEC");
    }

    // ---- entities ----

    fn layer_of(&mut self, entity: &Entity) -> String {
        if let Some(found) = self.layer_names.get(&entity.layer) {
            return found.clone();
        }
        // A layer the table does not have (the model would not allow one, but a
        // name is cheap to make safe): the name, made legal.
        let name = layer_name_for(&entity.layer);
        self.layer_names.insert(entity.layer.clone(), name.clone());
        name
    }

    fn begin(&mut self, entity_type: &str, entity: &Entity, subclass: &str, owner: u64) -> u64 {
        self.exact_colour = None;
        let own = self.next_handle();
        self.text(0, entity_type);
        self.handle(5, own);
        self.handle(330, owner);
        self.text(100, "AcDbEntity");
        let layer = self.layer_of(entity);
        self.text(8, &layer);
        if let Some(colour) = entity.color {
            let key = (usize::from(colour.r) << 16)
                | (usize::from(colour.g) << 8)
                | usize::from(colour.b);
            if !self.have_last_colour || key != self.last_colour_key {
                self.last_colour_index = nearest_indexed_colour(colour);
                self.last_colour_key = key;
                self.have_last_colour = true;
            }
            self.integer(62, i64::from(self.last_colour_index));
            if indexed_colour(self.last_colour_index) != Some(colour) {
                self.exact_colour = Some(colour);
            }
        }
        let linetype = entity
            .metadata
            .get(META_LINETYPE)
            .and_then(|value| self.linetype_names.get(&property_to_string(value)).cloned());
        if let Some(name) = linetype {
            self.text(6, &name);
        }
        if let Some(PropertyValue::Integer(value)) = entity.metadata.get(META_LINEWEIGHT) {
            self.integer(370, i64::from(nearest_lineweight(*value as f64 / 100.0)));
        }
        if !entity.visible {
            self.integer(60, 1);
        }
        self.text(100, subclass);
        self.result.dxf_entities_written += 1;
        own
    }

    fn heights_of(&self, entity: &Entity, count: usize) -> Vec<Option<f64>> {
        if entity.properties.is_empty() {
            return vec![None; count];
        }
        heights_of(&entity.properties, count)
    }

    fn line(&mut self, entity: &Entity, from: Point2, to: Point2) {
        self.begin("LINE", entity, "AcDbLine", MODEL_SPACE_RECORD);
        self.point(10, from, 0.0);
        self.point(11, to, 0.0);
        self.finish(None);
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the signature mirrors the C++ textLine"
    )]
    fn text_line(
        &mut self,
        entity: &Entity,
        position: Point2,
        line_text: &str,
        height: f64,
        rotation: f64,
        z: f64,
        centre: Option<Point2>,
        heights: Option<&[Option<f64>]>,
    ) {
        self.begin("TEXT", entity, "AcDbText", MODEL_SPACE_RECORD);
        self.point(10, position, z);
        self.real(40, height);
        self.text(1, &encode_text(line_text));
        // The degrees that read back as exactly this rotation, where it is a
        // turn or less, as every drawn text's is.
        let mut degrees = exact_degrees(rotation);
        // (The C++'s `degrees < 0 || degrees >= 360`; a NaN, which that skips
        // and this does not, comes out of the fmod a NaN either way.)
        if !(0.0..360.0).contains(&degrees) {
            degrees %= 360.0;
            if degrees < 0.0 {
                degrees += 360.0;
            }
        }
        if degrees != 0.0 {
            self.real(50, degrees);
        }
        if let Some(centre) = centre {
            self.integer(72, 1);
            self.point(11, centre, z);
        }
        self.text(100, "AcDbText");
        self.finish(heights);
    }

    fn write_text(&mut self, entity: &Entity, geometry: &TextGeometry) {
        let heights = self.heights_of(entity, 1);
        let z = heights[0].unwrap_or(0.0);
        let besides = if is_zero_level(&heights) {
            Some(heights.as_slice())
        } else {
            None
        };
        // One TEXT a line: TEXT holds one, and a Text holding a break is lines.
        let height = self.text_height_of(geometry);
        let down =
            Vec2::new(geometry.rotation.sin(), -geometry.rotation.cos()) * (LINE_PITCH * height);
        let mut rest: &str = &geometry.text;
        let mut position = geometry.position;
        if geometry.justify != TextJustify::BottomLeft {
            // The block's first baseline-left, by the estimate the model's box
            // uses (entity/text_block.rs): TEXT's own alignment points would
            // need the reader's font to agree with ours.
            let mut sized = geometry.clone();
            sized.height = height;
            let corners = estimated_text_corners(&sized);
            position =
                corners[3] + Vec2::new(geometry.rotation.sin(), -geometry.rotation.cos()) * height;
        }
        loop {
            let end = rest.find('\n');
            let mut line_text = match end {
                Some(end) => &rest[..end],
                None => rest,
            };
            if let Some(stripped) = line_text.strip_suffix('\r') {
                line_text = stripped;
            }
            if !line_text.is_empty() {
                self.text_line(
                    entity,
                    position,
                    line_text,
                    height,
                    geometry.rotation,
                    z,
                    None,
                    besides,
                );
            }
            let Some(end) = end else {
                break;
            };
            rest = &rest[end + 1..];
            position += down;
        }
    }

    /// This module's extended data, under its registered application: what the
    /// entity's own groups cannot hold, for this reader to take back. Other
    /// programs pass it by.
    ///
    /// HEIGHTS the entity's own groups cannot hold:
    ///
    /// ```text
    ///  - known at some vertices and not others. The entity itself goes in plan,
    ///    because the format has no "no height" and a vertex written at Z 0 would
    ///    be a false level - a surface built from the file would dive to the datum
    ///    there.
    ///  - at the two ends of an arc that climbs: ARC has one Z. In plan too, for
    ///    the same reason.
    ///  - a level of exactly 0 at every vertex. The groups say Z 0, which is how
    ///    every program writes "in plan", and so how this reader takes it; a
    ///    surveyed level of 0 would come back as no level at all.
    /// ```
    ///
    /// The list is the `elevations` property's text ("31.25 null 32.5"), in
    /// pieces of at most 250 characters, since R2000 holds an extended data
    /// string to 255. One height at every vertex is that one height alone,
    /// which the reader takes for every vertex, as the `elevation` property is.
    /// Only the first two kinds lose anything in another program, and the
    /// export's warning counts them.
    ///
    /// A COLOUR that no index is: R2000 has no true colour, so the entity
    /// carries the nearest index for everyone else and its own colour here.
    fn finish(&mut self, heights: Option<&[Option<f64>]>) {
        if heights.is_none() && self.exact_colour.is_none() {
            return;
        }
        self.text(1001, APPLICATION_NAME);
        if let Some(heights) = heights {
            let one = !heights.is_empty()
                && heights
                    .iter()
                    .all(|height| height.is_some() && *height == heights[0]);
            let count = if one { 1 } else { heights.len() };
            self.text(1000, EXTENDED_HEIGHTS);
            self.text(1002, "{");
            let mut piece = String::new();
            for height in &heights[..count] {
                let token = match height {
                    Some(height) => format_exact_real(*height),
                    None => String::from("null"),
                };
                if !piece.is_empty() && piece.len() + 1 + token.len() > 250 {
                    self.text(1000, &piece);
                    piece.clear();
                }
                if !piece.is_empty() {
                    piece.push(' ');
                }
                piece.push_str(&token);
            }
            if !piece.is_empty() {
                self.text(1000, &piece);
            }
            self.text(1002, "}");
            if !one {
                self.partial_heights += 1; // written in plan: other programs see no height
            }
        }
        if let Some(colour) = self.exact_colour.take() {
            self.text(1000, EXTENDED_COLOUR);
            self.text(1000, &colour.to_hex());
        }
    }

    fn write_polyline(&mut self, entity: &Entity, polyline: &Polyline2) {
        let count = polyline.vertices.len();
        let heights = self.heights_of(entity, count);
        let mut any_height = false;
        let mut every_height = true;
        let mut one_height = true;
        for height in &heights {
            any_height = any_height || height.is_some();
            every_height = every_height && height.is_some();
            one_height =
                one_height && height.is_some_and(|value| value == heights[0].unwrap_or(0.0));
        }
        if !any_height || one_height || !every_height {
            // Every vertex at one height, or none: an LWPOLYLINE, with the closed
            // flag. Never a HATCH - a closed boundary is a line, not a fill.
            self.begin("LWPOLYLINE", entity, "AcDbPolyline", MODEL_SPACE_RECORD);
            self.integer(90, count as i64);
            self.integer(70, if polyline.closed { 1 } else { 0 });
            let zero_level = one_height && any_height && heights[0] == Some(0.0);
            if one_height && any_height && !zero_level {
                self.real(38, heights[0].unwrap_or(0.0));
            }
            for vertex in &polyline.vertices {
                self.real(10, vertex.x + self.shift.x);
                self.real(20, vertex.y + self.shift.y);
                self.extents.expand(Point2::new(
                    vertex.x + self.shift.x,
                    vertex.y + self.shift.y,
                ));
            }
            self.finish(if (any_height && !every_height) || zero_level {
                Some(heights.as_slice())
            } else {
                None
            });
            return;
        }
        // Heights that differ: a 3D polyline, the only kind with a Z per vertex.
        let owner = self.begin("POLYLINE", entity, "AcDb3dPolyline", MODEL_SPACE_RECORD);
        self.integer(66, 1);
        self.real(10, 0.0);
        self.real(20, 0.0);
        self.real(30, 0.0);
        self.integer(70, 8 | if polyline.closed { 1 } else { 0 });
        self.finish(None); // on the POLYLINE, before its vertices
        for (&vertex, height) in polyline.vertices.iter().zip(&heights) {
            self.begin("VERTEX", entity, "AcDbVertex", owner);
            self.text(100, "AcDb3dPolylineVertex");
            self.point(10, vertex, height.unwrap_or(0.0));
            self.integer(70, 32);
        }
        let end = self.next_handle();
        self.text(0, "SEQEND");
        self.handle(5, end);
        self.handle(330, owner);
        self.text(100, "AcDbEntity");
        let layer = self.layer_of(entity);
        self.text(8, &layer);
    }

    /// A polyline with arcs: an LWPOLYLINE with each arc's bulge (group 42), the
    /// format's own way of saying it. Heights one and the same at every vertex
    /// go in its elevation; any other heights go beside it as this module's
    /// extended data, as a straight polyline's partial heights do, since the
    /// LWPOLYLINE has one elevation and the 3D POLYLINE, which has a Z per
    /// vertex, has no arcs.
    fn write_curve_polyline(&mut self, entity: &Entity, polyline: &CurvePolyline2) {
        let count = polyline.vertices.len();
        let heights = polyline.heights();
        let any_height = polyline.has_heights();
        let mut one_height = any_height;
        for height in &heights {
            one_height = one_height && height.is_some() && *height == heights[0];
        }
        self.begin("LWPOLYLINE", entity, "AcDbPolyline", MODEL_SPACE_RECORD);
        self.integer(90, count as i64);
        self.integer(70, if polyline.closed { 1 } else { 0 });
        let zero_level = one_height && heights[0] == Some(0.0);
        if one_height && !zero_level {
            self.real(38, heights[0].unwrap_or(0.0));
        }
        for (i, vertex) in polyline.vertices.iter().enumerate() {
            self.real(10, vertex.position.x + self.shift.x);
            self.real(20, vertex.position.y + self.shift.y);
            let starts_segment = polyline.closed || i + 1 < count;
            if starts_segment && vertex.bulge != 0.0 {
                self.real(42, vertex.bulge);
            }
            self.extents.expand(Point2::new(
                vertex.position.x + self.shift.x,
                vertex.position.y + self.shift.y,
            ));
        }
        for piece in polyline.segments() {
            if let CurveSegment::Arc(arc) = piece {
                let bounds = arc.bounding_box();
                self.extents.expand(Point2::new(
                    bounds.min.x + self.shift.x,
                    bounds.min.y + self.shift.y,
                ));
                self.extents.expand(Point2::new(
                    bounds.max.x + self.shift.x,
                    bounds.max.y + self.shift.y,
                ));
            }
        }
        self.finish(if (any_height && !one_height) || zero_level {
            Some(heights.as_slice())
        } else {
            None
        });
    }

    /// ELLIPSE: centre, the major axis's end relative to it, the ratio, and the
    /// start and end parameters in radians - Ellipse2's own fields.
    fn write_ellipse(&mut self, entity: &Entity, ellipse: &Ellipse2) {
        let heights = self.heights_of(entity, 1);
        self.begin("ELLIPSE", entity, "AcDbEllipse", MODEL_SPACE_RECORD);
        self.point(10, ellipse.center, heights[0].unwrap_or(0.0));
        self.real(11, ellipse.major_axis.x);
        self.real(21, ellipse.major_axis.y);
        self.real(31, 0.0);
        self.real(40, cpp::min(ellipse.ratio, 1.0));
        // Each in [0, 2 pi), as a DXF reader expects them; an end below the start
        // is an arc through the parameter 0, which is how the format says it.
        let start = normalize_angle(ellipse.start_parameter);
        self.real(41, start);
        self.real(
            42,
            if ellipse.is_full() {
                start + 2.0 * PI
            } else {
                normalize_angle(ellipse.end_parameter())
            },
        );
        self.finish(if is_zero_level(&heights) {
            Some(heights.as_slice())
        } else {
            None
        });
        let bounds = ellipse.bounding_box();
        self.extents.expand(Point2::new(
            bounds.min.x + self.shift.x,
            bounds.min.y + self.shift.y,
        ));
        self.extents.expand(Point2::new(
            bounds.max.x + self.shift.x,
            bounds.max.y + self.shift.y,
        ));
    }

    /// SPLINE, planar: degree, knots, weights when rational, control points, and
    /// the fit points it was drawn through when it has them.
    fn write_spline(&mut self, entity: &Entity, spline: &Spline2) {
        self.begin("SPLINE", entity, "AcDbSpline", MODEL_SPACE_RECORD);
        self.real(210, 0.0);
        self.real(220, 0.0);
        self.real(230, 1.0);
        self.integer(70, 8 | if spline.is_rational() { 4 } else { 0 });
        self.integer(71, i64::from(spline.degree));
        self.integer(72, spline.knots.len() as i64);
        self.integer(73, spline.control_points.len() as i64);
        self.integer(74, spline.fit_points.len() as i64);
        self.real(42, 1e-10);
        self.real(43, 1e-10);
        if !spline.fit_points.is_empty() {
            self.real(44, 1e-10);
        }
        for &knot in &spline.knots {
            self.real(40, knot);
        }
        for &weight in &spline.weights {
            self.real(41, weight);
        }
        for &p in &spline.control_points {
            self.point(10, p, 0.0);
        }
        for &p in &spline.fit_points {
            self.point(11, p, 0.0);
        }
        self.finish(None);
    }

    /// A text's height in the file: its paper height, or its style's, at the
    /// export's annotation scale; else its model height.
    fn text_height_of(&self, text: &TextGeometry) -> f64 {
        let mut paper = text.paper_height;
        if !(paper > 0.0)
            && !text.style.is_empty()
            && let Some(style) = self.model.text_styles.find(&text.style)
        {
            paper = style.paper_height;
        }
        if paper > 0.0 {
            annotation_model_size(paper, self.options.annotation_scale)
        } else {
            text.height
        }
    }

    /// A leader as the lines and text it draws at the export's annotation scale:
    /// the line through its vertices, a landing, and its note's lines beside the
    /// landing. No arrowhead - a filled one would be a SOLID with nothing to say
    /// it belongs to the line - and no callout frame.
    fn write_leader(&mut self, entity: &Entity, leader: &LeaderGeometry) {
        let v = &leader.vertices;
        for pair in v.windows(2) {
            self.line(entity, pair[0], pair[1]);
        }
        let scale = self.options.annotation_scale;
        // A model leader has two vertices at least (entity validation); the
        // C++ reads before the vector where it has fewer.
        let last = v[v.len() - 1] - v[v.len() - 2];
        let sign = if last.x < 0.0 { -1.0 } else { 1.0 };
        let mut end = v[v.len() - 1];
        if leader.landing > 0.0 {
            end += Vec2::new(sign * annotation_model_size(leader.landing, scale), 0.0);
            self.line(entity, v[v.len() - 1], end);
        }
        // A smart leader's note is read off its target (docs/annotation.md,
        // "Smart leaders"). This layer cannot see survey coding, so a {code}
        // field is absent here; a front end hands the drawn leader instead
        // (ExportOptions::drawn), which has it.
        let text = leader_note(self.model, leader, &[], &LeaderContext::default());
        if text.is_empty() {
            return;
        }
        let mut paper = leader.paper_height;
        if !(paper > 0.0) {
            let name = if leader.style.is_empty() {
                DEFAULT_TEXT_STYLE_NAME
            } else {
                leader.style.as_str()
            };
            paper = match self.model.text_styles.find(name) {
                Some(style) if style.paper_height > 0.0 => style.paper_height,
                _ => 2.5,
            };
        }
        let height = annotation_model_size(paper, scale);
        let note = TextGeometry {
            text,
            height,
            position: end + Vec2::new(sign * 0.5 * height, 0.0),
            justify: if sign > 0.0 {
                TextJustify::MiddleLeft
            } else {
                TextJustify::MiddleRight
            },
            ..TextGeometry::default()
        };
        self.write_text(entity, &note);
    }

    /// A dimension as the lines and text it draws: the format's DIMENSION needs
    /// a picture block beside it that every reader draws instead of the entity,
    /// so writing the picture is writing what the reader will see, without the
    /// half that some readers get wrong.
    fn write_dimension(&mut self, entity: &Entity, dimension: &DimensionGeometry) {
        if dimension.kind != DimensionKind::Aligned {
            self.dimensions_skipped += 1;
            self.result.entities_skipped += 1;
            return;
        }
        let style_name = match self.model.layers.find(&entity.layer) {
            Some(layer) if !layer.dimension_style.is_empty() => layer.dimension_style.clone(),
            _ => String::from(DEFAULT_DIMENSION_STYLE_NAME),
        };
        let mut style = self
            .model
            .dimension_styles
            .find(&style_name)
            .cloned()
            .unwrap_or_default();
        if style.paper_sized {
            let scale = self.options.annotation_scale;
            style.text_height = annotation_model_size(style.text_height, scale);
            style.text_gap = annotation_model_size(style.text_gap, scale);
            style.extension_offset = annotation_model_size(style.extension_offset, scale);
            style.extension_beyond = annotation_model_size(style.extension_beyond, scale);
            style.arrow_size = annotation_model_size(style.arrow_size, scale);
        }
        let length = dimension.measurement();
        let along = (dimension.end - dimension.start) / length;
        let left = Vec2::new(-along.y, along.x);
        let side = if dimension.offset >= 0.0 { 1.0 } else { -1.0 };
        let a = dimension.start + left * dimension.offset;
        let b = dimension.end + left * dimension.offset;
        if dimension.offset.abs() > style.extension_offset {
            self.line(
                entity,
                dimension.start + left * (side * style.extension_offset),
                a + left * (side * style.extension_beyond),
            );
            self.line(
                entity,
                dimension.end + left * (side * style.extension_offset),
                b + left * (side * style.extension_beyond),
            );
        }
        self.line(entity, a, b);
        // Architectural ticks: a slash of the arrow size across each end.
        let tick = (along + left) * (style.arrow_size * 0.5 / 2.0_f64.sqrt());
        self.line(entity, a - tick, a + tick);
        self.line(entity, b - tick, b + tick);
        let label = dimension_label(length, &dimension.text_override, &style);
        if !label.is_empty() {
            // Read from below or from the right, never upside down.
            let mut rotation = along.y.atan2(along.x);
            let mut up = left;
            let mut forward = along;
            if rotation > PI / 2.0 + 1e-9 || rotation <= -PI / 2.0 {
                rotation += PI;
                // The C++'s `up * -1.0`: the same bits as the negation.
                up = -up;
                forward = -forward;
            }
            let middle = (a + b) * 0.5 + up * style.text_gap;
            // Bytes, as the C++'s std::string::size counts them.
            let width = 0.6 * style.text_height * label.len() as f64;
            let start = middle - forward * (width / 2.0);
            self.text_line(
                entity,
                start,
                &label,
                style.text_height,
                rotation,
                0.0,
                Some(middle),
                None,
            );
        }
    }

    fn write_entity(&mut self, entity: &Entity) {
        // Annotation the front end drew out for this file (ExportOptions::drawn):
        // written as its shapes, on its layer and in its colour.
        if let Some(drawn) = self.options.drawn
            && let Some(shapes) = drawn.get(&entity.id)
        {
            if shapes.is_empty() {
                self.drawn_nothing += 1;
            }
            let mut shape = entity.clone();
            shape.properties.clear(); // an annotation's shapes have no surveyed heights
            for geometry in shapes {
                shape.geometry = geometry.clone();
                self.write_geometry(&shape);
            }
            return;
        }
        self.write_geometry(entity);
    }

    /// The entity as its own geometry, with no drawn shapes looked up: what
    /// write_entity writes each drawn shape with, since a shape keeps its
    /// entity's id.
    fn write_geometry(&mut self, entity: &Entity) {
        match &entity.geometry {
            Geometry::Point(shape) => {
                let heights = self.heights_of(entity, 1);
                self.begin("POINT", entity, "AcDbPoint", MODEL_SPACE_RECORD);
                self.point(10, shape.position, heights[0].unwrap_or(0.0));
                self.finish(if is_zero_level(&heights) {
                    Some(heights.as_slice())
                } else {
                    None
                });
            }
            Geometry::Line(shape) => {
                let heights = self.heights_of(entity, 2);
                // One end's height without the other's: in plan, with the
                // heights beside it (finish says why).
                let both = heights[0].is_some() && heights[1].is_some();
                self.begin("LINE", entity, "AcDbLine", MODEL_SPACE_RECORD);
                self.point(
                    10,
                    shape.start,
                    if both { heights[0].unwrap_or(0.0) } else { 0.0 },
                );
                self.point(
                    11,
                    shape.end,
                    if both { heights[1].unwrap_or(0.0) } else { 0.0 },
                );
                let partial = !both && (heights[0].is_some() || heights[1].is_some());
                self.finish(if partial || is_zero_level(&heights) {
                    Some(heights.as_slice())
                } else {
                    None
                });
            }
            Geometry::Arc(shape) => {
                let full = shape.sweep.abs() >= 2.0 * PI - 1e-12;
                // Every arc in the file runs counter-clockwise: a clockwise
                // one is the same curve from its other end, and its heights
                // are in that order too.
                let from = if shape.sweep >= 0.0 {
                    shape.start_angle
                } else {
                    shape.start_angle + shape.sweep
                };
                // A height at each end, as the model keeps them for an arc
                // on a grade. One Z holds them only when they are the same.
                let mut heights = self.heights_of(entity, 2);
                if shape.sweep < 0.0 {
                    heights.swap(0, 1);
                }
                let one_level = heights[0].is_some() && heights[0] == heights[1];
                let z = if one_level {
                    heights[0].unwrap_or(0.0)
                } else {
                    0.0
                };
                self.begin(
                    if full { "CIRCLE" } else { "ARC" },
                    entity,
                    "AcDbCircle",
                    MODEL_SPACE_RECORD,
                );
                self.point(10, shape.center, z);
                self.real(40, shape.radius);
                if !full {
                    self.text(100, "AcDbArc");
                    let mut start = (from * DEGREES_PER_RADIAN) % 360.0;
                    if start < 0.0 {
                        start += 360.0;
                    }
                    let end = (start + shape.sweep.abs() * DEGREES_PER_RADIAN) % 360.0;
                    self.real(50, start);
                    self.real(51, end);
                }
                let any = heights[0].is_some() || heights[1].is_some();
                self.finish(if any && (!one_level || z == 0.0) {
                    Some(heights.as_slice())
                } else {
                    None
                });
                let r = shape.radius;
                self.extents.expand(Point2::new(
                    shape.center.x + self.shift.x - r,
                    shape.center.y + self.shift.y - r,
                ));
                self.extents.expand(Point2::new(
                    shape.center.x + self.shift.x + r,
                    shape.center.y + self.shift.y + r,
                ));
            }
            Geometry::Circle(shape) => {
                let heights = self.heights_of(entity, 1);
                self.begin("CIRCLE", entity, "AcDbCircle", MODEL_SPACE_RECORD);
                self.point(10, shape.center, heights[0].unwrap_or(0.0));
                self.real(40, shape.radius);
                self.finish(if is_zero_level(&heights) {
                    Some(heights.as_slice())
                } else {
                    None
                });
                let r = shape.radius;
                self.extents.expand(Point2::new(
                    shape.center.x + self.shift.x - r,
                    shape.center.y + self.shift.y - r,
                ));
                self.extents.expand(Point2::new(
                    shape.center.x + self.shift.x + r,
                    shape.center.y + self.shift.y + r,
                ));
            }
            Geometry::Polyline(shape) => self.write_polyline(entity, shape),
            Geometry::Text(shape) => self.write_text(entity, shape),
            Geometry::Dimension(shape) => self.write_dimension(entity, shape),
            Geometry::Leader(shape) => self.write_leader(entity, shape),
            Geometry::CurvePolyline(shape) => self.write_curve_polyline(entity, shape),
            Geometry::Ellipse(shape) => self.write_ellipse(entity, shape),
            Geometry::Spline(shape) => self.write_spline(entity, shape),
            Geometry::Label(_) => {
                // A label's words and place are worked out for a view by the
                // placer, which this module cannot see; counted and said, never
                // written as something it is not.
                self.labels_skipped += 1;
                self.result.entities_skipped += 1;
            }
        }
    }

    fn write_entities(&mut self) {
        self.text(0, "SECTION");
        self.text(2, "ENTITIES");
        let model = self.model;
        let options = self.options;
        let chosen: HashSet<EntityId> = options.entities.iter().copied().collect();
        let only: BTreeSet<&str> = options.layers.iter().map(String::as_str).collect();
        model.entities.for_each(|entity| {
            if !chosen.is_empty() && !chosen.contains(&entity.id) {
                return;
            }
            if !only.is_empty() && !only.contains(entity.layer.as_str()) {
                return;
            }
            // A label or a dimension the file cannot hold is counted skipped,
            // not written as well.
            let skipped = self.result.entities_skipped;
            self.write_entity(entity);
            if self.result.entities_skipped == skipped {
                self.result.entities_written += 1;
            }
        });
        self.text(0, "ENDSEC");
    }

    fn run(mut self) -> Result<DxfExport> {
        if let Some(shift) = self.options.origin_shift
            && (!shift.x.is_finite() || !shift.y.is_finite())
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the origin shift is not finite",
            ));
        }
        self.assign_names();
        // Entities first, into a buffer of their own, so that the extents the
        // header and the viewport state are known when those are written - ahead
        // of them in the file. The table records keep the handles before them.
        self.handle = FIRST_FREE_HANDLE + self.linetypes.len() as u64 + self.layers.len() as u64;
        self.body = String::with_capacity(64 * 1024 + self.model.entities.size() * 256);
        self.write_entities();
        let entities = std::mem::take(&mut self.body);
        let after_entities = self.handle;
        self.handle = FIRST_FREE_HANDLE;
        self.write_tables();
        let tables = std::mem::take(&mut self.body);
        self.handle = after_entities;
        self.write_blocks();
        let blocks = std::mem::take(&mut self.body);
        self.write_objects();
        let objects = std::mem::take(&mut self.body);

        if self.labels_skipped != 0 {
            self.result.warnings.push(format!(
                "{} labels were not written: their text and place are worked out for a view, \
                 which the file has no form for",
                self.labels_skipped
            ));
        }
        if self.drawn_nothing != 0 {
            self.result.warnings.push(format!(
                "{} labels had no room at 1:{} and were not written, as the plan view leaves \
                 them out at that scale",
                self.drawn_nothing,
                format_exact_real(self.options.annotation_scale)
            ));
        }
        if self.dimensions_skipped != 0 {
            self.result.warnings.push(format!(
                "{} linear, angular, radial and ordinate dimensions were not written: only \
                 aligned dimensions are written, as the lines and text they draw",
                self.dimensions_skipped
            ));
        }
        if self.partial_heights != 0 {
            self.result.warnings.push(format!(
                "{} entities with heights the format cannot hold - at only some vertices, or \
                 different at the two ends of an arc - were written in plan, since the format \
                 has no \"no height\" and a Z of 0 would be a false level; their heights go \
                 beside them as extended data, which Katana reads back",
                self.partial_heights
            ));
        }
        let mut out = self.header();
        out.reserve(tables.len() + blocks.len() + entities.len() + objects.len() + 8);
        out.push_str(&tables);
        out.push_str(&blocks);
        out.push_str(&entities);
        out.push_str(&objects);
        out.push_str("  0\nEOF\n");
        self.result.bytes_written = out.len();
        self.result.text = out;
        Ok(self.result)
    }
}

/// The DXF layer name a Katana layer path is written under: "/" becomes "$",
/// and each other character the format refuses in a symbol name
/// (< > \ " : ; ? * | = `) becomes "_". Never empty.
pub fn layer_name_for(layer_path: &str) -> String {
    symbol_name(layer_path, true)
}

pub fn write_dxf(model: &Model, options: &ExportOptions<'_>) -> Result<DxfExport> {
    Writer::new(model, options).run()
}

/// Writes to `path`, replacing it. Fails with FileExportFailure when the file
/// cannot be written; a failed write leaves no half-written file behind it.
pub fn write_dxf_file(
    model: &Model,
    path: &Path,
    options: &ExportOptions<'_>,
) -> Result<DxfExport> {
    let mut written = write_dxf(model, options)?;
    let context = || utf8_or_windows1252(path_to_utf8(path));
    // Beside the target first, then over it: a write that fails part way
    // leaves the previous file, not half of a new one.
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".partial");
    let temporary = PathBuf::from(temporary);
    if std::fs::write(temporary.as_path(), written.text.as_bytes()).is_err() {
        let _ = std::fs::remove_file(temporary.as_path());
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            context(),
        ));
    }
    if let Err(error) = std::fs::rename(temporary.as_path(), path) {
        let _ = std::fs::remove_file(temporary.as_path());
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!("the file could not be replaced: {}", os_message(&error)),
            context(),
        ));
    }
    written.text = String::new();
    Ok(written)
}
