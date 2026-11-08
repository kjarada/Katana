//! The plot frame: the sheet border, the title-block strip, its cells, the
//! field slots and the text rules (docs/plotting.md;
//! include/katana/cad/plotting/frame.hpp).
//!
//! The one frame Katana ships is the owner's A3 landscape cross-section frame,
//! measured item by item from their app and committed as data
//! (resources/plot_frames/a3_landscape.json, embedded into the build). The
//! organisation's branding was taken out before it was committed; its three
//! cells are now slots the user fills: their own logo image, their
//! organisation's name, and their notes.
//!
//! PAPER COORDINATES. Everything on a sheet - frame, cells, viewports - is in
//! millimetres from the BOTTOM-LEFT corner of the paper, Y up: the frame's own
//! convention, and the one model space uses, so a renderer flips Y once, at
//! the device, and nothing in the model has two conventions.
//!
//! Other paper sizes use the same frame scaled uniformly (frame_scale_for): A1
//! is the A3 frame doubled, text, line weights and dashes included. A portrait
//! sheet has no frame (docs/plotting.md says why).
//!
//! Everything here is data and arithmetic; the painter lives in the window.

use std::sync::OnceLock;

use katana_core::error::make_error;
use katana_core::json::{Json, JsonError};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::Color;
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::plot::{PaperSize, paper_dimensions};

/// The id a Sheet names the built-in frame by. An empty id means no frame.
pub const BUILT_IN_FRAME_ID: &str = "a3_landscape";

// The frame's JSON, embedded by the compiler: the C++ uses #embed of the same
// committed file, so both programs carry the same bytes.
static EMBEDDED_FRAME: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../resources/plot_frames/a3_landscape.json"
));

// The size the measured frame was drawn for. Only its ratio to the paper is
// used (frame_scale_for); the JSON's own sheet size is what a parse reads.
const MEASURED_WIDTH_MM: f64 = 420.0;
const MEASURED_HEIGHT_MM: f64 = 297.0;

// The removed organisation header's cap height and the removed notes text's
// first-line anchor and cap height, as the frame's cell descriptions record
// them: the slots are laid out exactly where the branding was.
const ORGANISATION_CAP_MM: f64 = 3.738;
const NOTES_ANCHOR: Point2 = Point2::new(220.918, 32.961);
const NOTES_CAP_MM: f64 = 0.85;
// The measured frame's condensing factor: 0.82 of Arial is Arial Narrow's
// width, which is what the notes were set in.
const CONDENSED: f64 = 0.82;
// Line pitch as a multiple of the cap height (the measured rule).
const LINE_PITCH: f64 = 1.5;

/// What an item of the frame is for. A renderer draws them all the same way;
/// the role is what lets it leave the construction guide off the paper, hide
/// the legend block, or find the text a field fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameRole {
    /// The drawing border, the title-block box and its rules.
    Border,
    /// The dashed line a hand-written or field value sits on.
    Underline,
    /// The sheet-edge guide: shown while editing, NEVER plotted.
    Construction,
    /// The utility legend block (optional on a sheet).
    Legend,
    /// Static text: "SCALE", "SHEET No.", "DATE:".
    Label,
    /// Text carrying one or more {field} placeholders.
    Field,
    /// The frame version and plot date under the title block.
    Stamp,
    /// A user slot the branding was removed from: organisation, notes.
    Slot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FramePolyline {
    pub points: Vec<Point2>,
    pub closed: bool,
    pub weight_mm: f64,
    pub colour: Color,
    /// Dash and gap in paper millimetres; both 0 for a solid line.
    pub dash_mm: f64,
    pub gap_mm: f64,
    pub role: FrameRole,
    /// False only for the construction guide, which is drawn on screen and
    /// never on paper.
    pub plots: bool,
}

impl Default for FramePolyline {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
            weight_mm: 0.25,
            colour: Color::new(0, 0, 0, 255),
            dash_mm: 0.0,
            gap_mm: 0.0,
            role: FrameRole::Border,
            plots: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HorizontalJustify {
    #[default]
    Left,
    Centre,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VerticalJustify {
    #[default]
    Bottom,
    Middle,
    Top,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameText {
    /// Where the text is drawn: the measured position with the text's offset
    /// and raise already added (six texts carry one). For a multi-line text it
    /// is the FIRST line's anchor; later lines step DOWN by line_spacing_mm
    /// whatever the justification - the measured rule.
    pub anchor: Point2,
    /// The height of a capital, not the em.
    pub cap_height_mm: f64,
    /// Horizontal stretch about the anchor: nearly every label is 0.82, which
    /// on Arial is exactly Arial Narrow's width. A renderer applies it as a
    /// painter scale, never as a font stretch (which measures 89%, not 82%).
    pub x_factor: f64,
    pub horizontal: HorizontalJustify,
    pub vertical: VerticalJustify,
    /// Counter-clockwise.
    pub angle_degrees: f64,
    /// 1.5 x the cap height, as measured.
    pub line_spacing_mm: f64,
    pub font_face: String,
    pub bold: bool,
    pub colour: Color,
    /// Static text with {field} placeholders (expand_template in sheet_set.rs
    /// fills them). Lines are separated by '\n'.
    pub content: String,
    /// The fields this text shows, in order of appearance; empty for a label.
    pub fields: Vec<String>,
    pub role: FrameRole,
    /// The cell a derived value is centred in rather than drawn from its
    /// anchor (scale, sheet number, sheet count); empty for every other text.
    pub centred_in: String,
}

impl Default for FrameText {
    fn default() -> Self {
        Self {
            anchor: Point2::default(),
            cap_height_mm: 1.0,
            x_factor: 1.0,
            horizontal: HorizontalJustify::Left,
            vertical: VerticalJustify::Bottom,
            angle_degrees: 0.0,
            line_spacing_mm: 1.5,
            font_face: String::from("Arial"),
            bold: false,
            colour: Color::new(0, 0, 0, 255),
            content: String::new(),
            fields: Vec::new(),
            role: FrameRole::Label,
            centred_in: String::new(),
        }
    }
}

/// A drawing symbol of the legend block, by the name of its style.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSymbol {
    pub style: String,
    pub at: Point2,
    pub size_mm: f64,
    pub rotation_degrees: f64,
    pub colour: Color,
}

impl Default for FrameSymbol {
    fn default() -> Self {
        Self {
            style: String::new(),
            at: Point2::default(),
            size_mm: 1.0,
            rotation_degrees: 0.0,
            colour: Color::new(0, 0, 0, 255),
        }
    }
}

/// A named rectangle of the title block: "scale", "logo", "organisation",
/// "notes", "sheet_number" ... docs/plotting.md lists all sixteen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameCell {
    pub id: String,
    pub rect: Box2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameMargins {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: String,
    pub width_mm: f64,
    pub height_mm: f64,
    /// The factor this frame was scaled by from the measured one: 1 for A3.
    pub scale: f64,
    pub margins: FrameMargins,
    /// Where viewports go: the paper inside the margins.
    pub drawing_area: Box2,
    /// The drawn border around the drawing area and the title-block box.
    pub border: Box2,
    pub title_block: Box2,
    /// The construction guide at the sheet edge (never plotted).
    pub sheet_edge: Box2,
    pub polylines: Vec<FramePolyline>,
    pub texts: Vec<FrameText>,
    pub symbols: Vec<FrameSymbol>,
    pub cells: Vec<FrameCell>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            id: String::new(),
            width_mm: 0.0,
            height_mm: 0.0,
            scale: 1.0,
            margins: FrameMargins::default(),
            drawing_area: Box2::default(),
            border: Box2::default(),
            title_block: Box2::default(),
            sheet_edge: Box2::default(),
            polylines: Vec::new(),
            texts: Vec::new(),
            symbols: Vec::new(),
            cells: Vec::new(),
        }
    }
}

impl Frame {
    /// None when the frame has no cell of that id.
    pub fn cell(&self, cell_id: &str) -> Option<&FrameCell> {
        self.cells.iter().find(|cell| cell.id == cell_id)
    }

    /// Indices into `texts` of every text that shows `field`.
    pub fn texts_showing(&self, field: &str) -> Vec<usize> {
        self.texts
            .iter()
            .enumerate()
            .filter(|(_, text)| text.fields.iter().any(|shown| shown.as_str() == field))
            .map(|(index, _)| index)
            .collect()
    }
}

// What stops a parse: a nlohmann exception (an accessor that throws, which the
// C++ catches at parse_frame's edge) or a refusal of the frame's own.
enum Fault {
    Json(JsonError),
    Refused(Error),
}

impl From<JsonError> for Fault {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}

impl From<Error> for Fault {
    fn from(error: Error) -> Self {
        Self::Refused(error)
    }
}

type Parsed<T> = std::result::Result<T, Fault>;

// The frame's numbered text fields, by the meaning each has in the measured
// frame, under the general names a sheet set uses (docs/plotting.md). 8 and 9
// (the coordinate system and zone) are replaced as a whole text below.
fn user_text_field(number: i32) -> Option<&'static str> {
    Some(match number {
        1 => "sheet_count",
        2 => "approver_name",
        3 => "locator_name",
        4 => "surveyor_name",
        5 => "compiler_name",
        6 => "reviewer_name",
        7 => "model_name",
        10 => "project_line_1",
        11 => "project_line_2",
        12 => "project_line_3",
        13 => "project_line_4",
        14 => "set_number",
        17 => "scale",
        19 => "file_name",
        _ => return None,
    })
}

// The measured frame feeds ONE date to all six date texts, so a sheet
// reviewed a week after it was surveyed said otherwise. Each sign-off row gets
// its own date field here, by the entity that draws it (the sign-off rows
// from the top: utilities/locator, surveyed, compiled, reviewed; then the
// approver's row, and the stamp under the title block).
fn date_field_for(entity_index: usize) -> &'static str {
    match entity_index {
        122 => "approver_date",
        123 => "locator_date",
        124 => "surveyor_date",
        125 => "compiler_date",
        126 => "reviewer_date",
        _ => "plot_date",
    }
}

fn colour_of(item: &Json) -> Parsed<Color> {
    Ok(Color::from_hex(&item.at("colour_hex")?.get::<String>()?)?)
}

fn point_of(pair: &Json) -> Parsed<Point2> {
    let x = pair.at_index(0)?.get::<f64>()?;
    let y = pair.at_index(1)?.get::<f64>()?;
    Ok(Point2::new(x, y))
}

fn box_of(rect: &Json) -> Parsed<Box2> {
    let (x0, y0, x1, y1) = if rect.is_array() {
        (
            rect.at_index(0)?.get::<f64>()?,
            rect.at_index(1)?.get::<f64>()?,
            rect.at_index(2)?.get::<f64>()?,
            rect.at_index(3)?.get::<f64>()?,
        )
    } else {
        (
            rect.at("x0")?.get::<f64>()?,
            rect.at("y0")?.get::<f64>()?,
            rect.at("x1")?.get::<f64>()?,
            rect.at("y1")?.get::<f64>()?,
        )
    };
    Ok(Box2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

fn role_of(role: &str) -> FrameRole {
    match role {
        "field-underline" => FrameRole::Underline,
        "construction" => FrameRole::Construction,
        "legend" => FrameRole::Legend,
        "field" => FrameRole::Field,
        "stamp" => FrameRole::Stamp,
        "label" => FrameRole::Label,
        _ => FrameRole::Border,
    }
}

// "vertical-horizontal", e.g. "bottom-left" or "middle-centre".
fn justify_of(justify: &str) -> (VerticalJustify, HorizontalJustify) {
    let vertical = if justify.starts_with("middle") {
        VerticalJustify::Middle
    } else if justify.starts_with("top") {
        VerticalJustify::Top
    } else {
        VerticalJustify::Bottom
    };
    let horizontal = if justify.ends_with("centre") || justify.ends_with("center") {
        HorizontalJustify::Centre
    } else if justify.ends_with("right") {
        HorizontalJustify::Right
    } else {
        HorizontalJustify::Left
    };
    (vertical, horizontal)
}

fn find_byte(text: &[u8], byte: u8, from: usize) -> Option<usize> {
    text.get(from..)?
        .iter()
        .position(|&found| found == byte)
        .map(|offset| offset + from)
}

// The field name of a {name} placeholder starting at `at` - lower-case
// letters, digits and underscores, as resolve_fields names them - or empty
// text when there is none there.
fn placeholder_at(text: &[u8], at: usize) -> &[u8] {
    if at >= text.len() || text[at] != b'{' {
        return &[];
    }
    let Some(close) = find_byte(text, b'}', at + 1) else {
        return &[];
    };
    if close == at + 1 {
        return &[];
    }
    let name = &text[at + 1..close];
    let named = name
        .iter()
        .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_');
    if named { name } else { &[] }
}

// `std::from_chars` into an int over `text`: the value of the number it
// starts with (a minus sign and digits) when there is one that fits, and
// whether it took all of `text`. The C++ leaves its variable unchanged when
// there is no number or it does not fit.
fn int_prefix(text: &[u8]) -> (Option<i32>, bool) {
    let negative = text.first() == Some(&b'-');
    let start = usize::from(negative);
    let digits = text[start..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return (None, false);
    }
    let mut value: i64 = 0;
    for &byte in &text[start..start + digits] {
        value = value * 10 + i64::from(byte - b'0');
        if value > i64::from(i32::MAX) + 1 {
            return (None, start + digits == text.len());
        }
    }
    let value = if negative { -value } else { value };
    match i32::try_from(value) {
        Ok(number) => (Some(number), start + digits == text.len()),
        Err(_) => (None, start + digits == text.len()),
    }
}

// The measured text with its tokens turned into {field} placeholders, and the
// fields it shows. Fails on a numbered field this build has no name for, so
// a changed frame is caught by the tests rather than printing a raw token. A
// placeholder already in that form, as in a frame written for Katana, is a
// field too.
fn template_of(raw: &str, entity_index: usize, fields: &mut Vec<String>) -> Result<String> {
    // Regional wording turned into fields: the coordinate system (with its
    // zone) and the height datum were hard-coded for one country's grid.
    if raw.starts_with("CO-ORD SYSTEM:") {
        *fields = vec![String::from("coordinate_system")];
        return Ok(String::from("CO-ORD SYSTEM: {coordinate_system}"));
    }
    if raw.starts_with("HEIGHT DATUM:") {
        *fields = vec![String::from("height_datum")];
        return Ok(String::from("HEIGHT DATUM: {height_datum}"));
    }
    let mut text = String::from(raw);
    // The project block's first line carried a region-specific label ahead
    // of its field; the lines are the user's, so the label goes with it.
    if text.starts_with("LGA:")
        && let Some(field) = text.find("$user_text<10,")
    {
        text.replace_range(..field, "");
    }
    // The stamp names the paper; on a scaled frame that is no longer A3.
    if text.starts_with("A3 Border") {
        text.replace_range(0..2, "{paper}");
        fields.push(String::from("paper"));
    }
    let bytes = text.as_bytes();
    // Bytes of `text` and ASCII placed only where `text` has an ASCII byte, so
    // what is built is UTF-8 as `text` is.
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        if rest.starts_with(b"$user_text<") {
            let (comma, close) = match (find_byte(bytes, b',', at), find_byte(bytes, b'>', at)) {
                (Some(comma), Some(close)) if comma <= close => (comma, close),
                _ => {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "a frame field token is not closed",
                        format!("entity {entity_index}"),
                    ));
                }
            };
            let (parsed, whole) = int_prefix(&bytes[at + 11..comma]);
            let number = parsed.unwrap_or(0);
            let found = if parsed.is_some() && whole {
                user_text_field(number)
            } else {
                None
            };
            let Some(name) = found else {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "the frame uses a numbered field this build has no name for",
                    format!("user_text {number} in entity {entity_index}"),
                ));
            };
            out.push(b'{');
            out.extend_from_slice(name.as_bytes());
            out.push(b'}');
            fields.push(String::from(name));
            at = close + 1;
        } else if rest.starts_with(b"$drawing_number") {
            out.extend_from_slice(b"{sheet_number}");
            fields.push(String::from("sheet_number"));
            at += 15;
        } else if rest.starts_with(b"$time") {
            let key = date_field_for(entity_index);
            out.push(b'{');
            out.extend_from_slice(key.as_bytes());
            out.push(b'}');
            fields.push(String::from(key));
            at += 5;
            if at < bytes.len() && bytes[at] == b'<' {
                at = match find_byte(bytes, b'>', at) {
                    Some(close) => close + 1,
                    None => bytes.len(),
                };
            }
        } else if rest.starts_with(b"{{") {
            // A literal brace, as expand_template reads one: not a field.
            out.extend_from_slice(b"{{");
            at += 2;
        } else {
            let name = placeholder_at(bytes, at);
            if name.is_empty() {
                out.push(bytes[at]);
                at += 1;
            } else {
                // A field named as a sheet names it, "REV {revision}": a frame
                // written for Katana shows it as it is, and the painter fills
                // it.
                out.push(b'{');
                out.extend_from_slice(name);
                out.push(b'}');
                fields.push(String::from_utf8_lossy(name).into_owned());
                at += name.len() + 2;
            }
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

// The cell a derived value is centred in (the measured app re-centres the
// same three).
fn centred_cell_for(fields: &[String]) -> String {
    if let [field] = fields
        && (field == "scale" || field == "sheet_number" || field == "sheet_count")
    {
        return field.clone();
    }
    String::new()
}

fn read_polyline(item: &Json, frame: &mut Frame) -> Parsed<()> {
    let mut line = FramePolyline::default();
    for point in item.at("points_tbf")?.iter() {
        line.points.push(point_of(point)?);
    }
    line.closed = item.at("closed")?.get::<bool>()?;
    // The data repeats a closed ring's first point at its end; a closed
    // polyline here does not, so a renderer does not draw that edge twice.
    if line.closed && line.points.len() > 1 && line.points.first() == line.points.last() {
        line.points.pop();
    }
    line.weight_mm = item.at("weight_mm")?.get::<f64>()?;
    line.colour = colour_of(item)?;
    let dash = item.at("dash_mm")?;
    if dash.is_array() {
        line.dash_mm = dash.at_index(0)?.get::<f64>()?;
        line.gap_mm = dash.at_index(1)?.get::<f64>()?;
    }
    line.role = role_of(&item.at("role")?.get::<String>()?);
    line.plots = line.role != FrameRole::Construction;
    frame.polylines.push(line);
    Ok(())
}

fn read_text(item: &Json, frame: &mut Frame) -> Parsed<()> {
    let mut text = FrameText {
        anchor: point_of(item.at("anchor_tbf")?)?,
        cap_height_mm: item.at("cap_height_mm")?.get::<f64>()?,
        x_factor: item.at("x_factor")?.get::<f64>()?,
        ..FrameText::default()
    };
    let (vertical, horizontal) = justify_of(&item.at("justify")?.get::<String>()?);
    text.vertical = vertical;
    text.horizontal = horizontal;
    text.angle_degrees = item.at("angle_deg")?.get::<f64>()?;
    text.line_spacing_mm = LINE_PITCH * text.cap_height_mm;
    text.font_face = item.at("font_face")?.get::<String>()?;
    text.bold = item.at("bold")?.get::<bool>()?;
    text.colour = colour_of(item)?;
    let index = item.at("index")?.get::<usize>()?;
    let raw = item.at("raw_text")?.get::<String>()?;
    text.content = template_of(&raw, index, &mut text.fields)?;
    text.role = role_of(&item.at("role")?.get::<String>()?);
    if text.role == FrameRole::Label && !text.fields.is_empty() {
        text.role = FrameRole::Field; // the height datum
    }
    text.centred_in = centred_cell_for(&text.fields);
    frame.texts.push(text);
    Ok(())
}

fn read_symbol(item: &Json, frame: &mut Frame) -> Parsed<()> {
    let style = item.at("symbol_style")?.get::<String>()?;
    let at = point_of(item.at("at_tbf")?)?;
    let size_mm = item.at("size_mm")?.get::<f64>()?;
    let rotation_degrees = item.at("rotation_deg")?.get::<f64>()?;
    let colour = colour_of(item)?;
    frame.symbols.push(FrameSymbol {
        style,
        at,
        size_mm,
        rotation_degrees,
        colour,
    });
    Ok(())
}

// The two slots whose text was removed with the branding, laid out where it
// was: the organisation's name centred in its cell, and the notes from the
// removed text's first-line anchor.
fn add_slots(frame: &mut Frame) {
    if let Some(centre) = frame.cell("organisation").map(|cell| cell.rect.center()) {
        frame.texts.push(FrameText {
            anchor: centre,
            cap_height_mm: ORGANISATION_CAP_MM,
            x_factor: CONDENSED,
            vertical: VerticalJustify::Middle,
            horizontal: HorizontalJustify::Centre,
            line_spacing_mm: LINE_PITCH * ORGANISATION_CAP_MM,
            content: String::from("{organisation}"),
            fields: vec![String::from("organisation")],
            role: FrameRole::Slot,
            ..FrameText::default()
        });
    }
    if frame.cell("notes").is_some() {
        frame.texts.push(FrameText {
            anchor: NOTES_ANCHOR,
            cap_height_mm: NOTES_CAP_MM,
            x_factor: CONDENSED,
            line_spacing_mm: LINE_PITCH * NOTES_CAP_MM,
            content: String::from("{notes}"),
            fields: vec![String::from("notes")],
            role: FrameRole::Slot,
            ..FrameText::default()
        });
    }
}

fn parse(root: &Json) -> Parsed<Frame> {
    let mut frame = Frame {
        id: String::from(BUILT_IN_FRAME_ID),
        ..Frame::default()
    };
    let sheet = root.at("sheet")?;
    frame.width_mm = sheet.at("width_mm")?.get::<f64>()?;
    frame.height_mm = sheet.at("height_mm")?.get::<f64>()?;
    let margins = sheet.at("margins_mm")?;
    frame.margins = FrameMargins {
        left: margins.at("left")?.get::<f64>()?,
        right: margins.at("right")?.get::<f64>()?,
        top: margins.at("top")?.get::<f64>()?,
        bottom: margins.at("bottom")?.get::<f64>()?,
    };
    if !(frame.width_mm > 0.0)
        || !(frame.height_mm > 0.0)
        || !(frame.margins.left + frame.margins.right < frame.width_mm)
        || !(frame.margins.top + frame.margins.bottom < frame.height_mm)
    {
        return Err(Fault::Refused(Error::new(
            ErrorCode::InvalidArgument,
            "the frame's size and margins leave no sheet",
        )));
    }
    frame.drawing_area = box_of(sheet.at("viewport_tbf")?)?;
    frame.border = box_of(sheet.at("drawing_border_tbf")?)?;
    frame.title_block = box_of(sheet.at("title_block_tbf")?)?;
    frame.sheet_edge = box_of(sheet.at("sheet_edge_tbf")?)?;
    for cell in root.at("cells")?.iter() {
        let mut id = cell.at("id")?.get::<String>()?;
        // The measured frame's disclaimer cell is the user's notes slot now.
        if id == "disclaimer" {
            id = String::from("notes");
        }
        let rect = box_of(cell.at("tbf")?)?;
        frame.cells.push(FrameCell { id, rect });
    }
    for item in root.at("entities")?.iter() {
        let kind = item.at("kind")?.get::<String>()?;
        match kind.as_str() {
            "poly" => read_polyline(item, &mut frame)?,
            "text" => read_text(item, &mut frame)?,
            "symbol" => read_symbol(item, &mut frame)?,
            _ => {
                return Err(Fault::Refused(make_error(
                    ErrorCode::ParseFailure,
                    "a frame item of an unknown kind",
                    kind.clone(),
                )));
            }
        }
    }
    add_slots(&mut frame);
    Ok(frame)
}

fn parse_bytes(json: &[u8]) -> Result<Frame> {
    let parsed = Json::parse(json)
        .map_err(Fault::Json)
        .and_then(|root| parse(&root));
    match parsed {
        Ok(frame) => Ok(frame),
        Err(Fault::Json(error)) => Err(make_error(
            ErrorCode::ParseFailure,
            "the plot frame is not in the expected format",
            error.what,
        )),
        Err(Fault::Refused(error)) => Err(error),
    }
}

/// Parses a frame description in the committed format (see the JSON's own
/// "_about"). Besides the measured frame's tokens, a text may name a field as
/// a sheet names it, "REV {revision}", and it is filled like the others.
/// ParseFailure for text that is not JSON or lacks a required member;
/// InvalidArgument for a size or margin that makes no sheet.
pub fn parse_frame(json: &str) -> Result<Frame> {
    parse_bytes(json.as_bytes())
}

/// The built-in A3 landscape frame, parsed once on first use from the copy
/// embedded in the build. A failure here is a defect in the committed data,
/// and is reported rather than papered over.
pub fn built_in_frame() -> &'static Result<Frame> {
    static FRAME: OnceLock<Result<Frame>> = OnceLock::new();
    FRAME.get_or_init(|| parse_bytes(EMBEDDED_FRAME))
}

/// The uniform factor the A3 frame is scaled by to sit on `paper`: the smaller
/// of width / 420 and height / 297, so it always fits. A1 is 2 exactly
/// (594 / 297); A0 2.8310 (1189 / 420); A2 1.4141 (420 / 297); A4 0.7071
/// (210 / 297). The slack of a millimetre or two the ISO roundings leave goes
/// to the right and top margins. 0 for a portrait sheet, which has no frame.
pub fn frame_scale_for(paper: PaperSize, landscape: bool) -> f64 {
    if !landscape {
        return 0.0;
    }
    let size = paper_dimensions(paper, true);
    cpp::min(
        size.width_mm / MEASURED_WIDTH_MM,
        size.height_mm / MEASURED_HEIGHT_MM,
    )
}

fn scaled_point(point: Point2, factor: f64) -> Point2 {
    Point2::new(point.x * factor, point.y * factor)
}

fn scaled_box(rect: &Box2, factor: f64) -> Box2 {
    Box2::new(
        scaled_point(rect.min, factor),
        scaled_point(rect.max, factor),
    )
}

/// `frame` scaled by `factor` about the paper origin: every coordinate, text
/// height, line spacing, line weight, dash and symbol size. The paper becomes
/// the given size, and the slack goes to the right and top margins.
pub fn scaled_frame(
    frame: &Frame,
    factor: f64,
    paper_width_mm: f64,
    paper_height_mm: f64,
) -> Frame {
    let mut out = frame.clone();
    out.scale = frame.scale * factor;
    out.width_mm = paper_width_mm;
    out.height_mm = paper_height_mm;
    out.drawing_area = scaled_box(&frame.drawing_area, factor);
    out.border = scaled_box(&frame.border, factor);
    out.title_block = scaled_box(&frame.title_block, factor);
    out.sheet_edge = Box2::new(
        Point2::new(0.0, 0.0),
        Point2::new(paper_width_mm, paper_height_mm),
    );
    // The slack the ISO roundings leave goes to the right and top.
    out.margins = FrameMargins {
        left: frame.margins.left * factor,
        right: paper_width_mm - out.drawing_area.max.x,
        top: paper_height_mm - out.drawing_area.max.y,
        bottom: frame.margins.bottom * factor,
    };
    // The construction guide marks the paper's edge, not the scaled frame's.
    let edge_x = paper_width_mm / frame.width_mm;
    let edge_y = paper_height_mm / frame.height_mm;
    for line in &mut out.polylines {
        let construction = line.role == FrameRole::Construction;
        for point in &mut line.points {
            *point = if construction {
                Point2::new(point.x * edge_x, point.y * edge_y)
            } else {
                scaled_point(*point, factor)
            };
        }
        line.weight_mm *= factor;
        line.dash_mm *= factor;
        line.gap_mm *= factor;
    }
    for text in &mut out.texts {
        text.anchor = scaled_point(text.anchor, factor);
        text.cap_height_mm *= factor;
        text.line_spacing_mm *= factor;
    }
    for symbol in &mut out.symbols {
        symbol.at = scaled_point(symbol.at, factor);
        symbol.size_mm *= factor;
    }
    for cell in &mut out.cells {
        cell.rect = scaled_box(&cell.rect, factor);
    }
    out
}

/// The frame a sheet of this paper plots with: the built-in frame scaled to it.
/// NotFound for an unknown id; InvalidArgument for a portrait sheet, which has
/// no frame. Call only with a non-empty id - an empty one means "no frame".
pub fn frame_for(frame_id: &str, paper: PaperSize, landscape: bool) -> Result<Frame> {
    if frame_id != BUILT_IN_FRAME_ID {
        return Err(make_error(
            ErrorCode::NotFound,
            "no plot frame of that name",
            frame_id,
        ));
    }
    if !landscape {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a portrait sheet has no frame: the title block is laid out for landscape paper",
        ));
    }
    let base = match built_in_frame() {
        Ok(base) => base,
        Err(error) => return Err(error.clone()),
    };
    let size = paper_dimensions(paper, true);
    if size.width_mm == base.width_mm && size.height_mm == base.height_mm {
        return Ok(base.clone());
    }
    Ok(scaled_frame(
        base,
        frame_scale_for(paper, true),
        size.width_mm,
        size.height_mm,
    ))
}

/// Where an image of `image_width` x `image_height` (any unit, only the ratio
/// matters) is drawn in `cell`: as large as fits inside the cell less
/// `padding_mm` all round, its aspect ratio kept, centred. Empty when the image
/// or the padded cell has no size. The logo slot is 52.96 x 12.0 mm; with a
/// 1 mm padding (the C++'s default) a 4:1 logo is drawn 40.0 x 10.0 mm.
pub fn fit_image(cell: &Box2, image_width: f64, image_height: f64, padding_mm: f64) -> Box2 {
    let inner = cell.inflated(-padding_mm);
    if !(image_width > 0.0)
        || !(image_height > 0.0)
        || !image_width.is_finite()
        || !image_height.is_finite()
        || inner.empty()
        || !(inner.width() > 0.0)
        || !(inner.height() > 0.0)
    {
        return Box2::default();
    }
    let factor = cpp::min(inner.width() / image_width, inner.height() / image_height);
    let width = image_width * factor;
    let height = image_height * factor;
    let centre = inner.center();
    Box2::new(
        Point2::new(centre.x - width / 2.0, centre.y - height / 2.0),
        Point2::new(centre.x + width / 2.0, centre.y + height / 2.0),
    )
}
