//! The sheet system on the text command line (docs/plotting.md, "Sheets on
//! the command line"; include/katana/cad/plotting/sheet_verbs.hpp): the verbs
//! a person, a script, the headless katana_cli and an AI agent type to lay
//! out, edit and inspect a project's sheets.
//!
//! ```text
//! SHEETS [LIST] | JSON [path] | SAVE path | LOAD path | APPEND path | CHECK | PAGESETUP
//! SHEET NEW | REMOVE | MOVE | COPY | RENAME | SET | FIELD | SUGGESTPAPER
//! VIEW ADD | SET | REMOVE | LIST | FIT | BESTROTATE
//! TILE n preset
//! ARRANGE n | ALIGN | DISTRIBUTE | MATCHSCALE
//! GENERATE fit | grid | strips | profile | sections | frames | register
//! TITLEBLOCK [LIST] | field value | REVISION ADD | REVISION REMOVE | LOGO path
//! ```
//!
//! Sheets are named by their number in the set (1, 2 ...) or by their id (s1,
//! s2 ...), views by their id (vp1, vp2 ...). Options are key=value and verbs,
//! keys and words are case-insensitive. Every edit is ONE undoable step
//! through sheet_commands.rs, and a refused edit changes nothing. A reply is
//! one fact per line in the same key=value form the options take, so an
//! option SHEETS LIST prints can be typed back. Stored sheets that cannot be
//! read refuse every verb but SHEETS LOAD, rather than be overwritten.
//!
//! Plotting needs the window, so PLOTSHEETS is the desktop window's verb; it
//! parses its line with parse_plot_sheets below, in the words the others use.
//!
//! What the window adds (SheetVerbContext) reaches the verbs through
//! closures that are handed the Document when they are called: the verbs hold
//! the document mutably while they run, so a closure cannot keep a borrow of
//! it as the C++'s lambdas keep `&document`.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::text::{
    ascii_lower, format_exact_real, parse_finite_double, reply_quoted, unescape_typed,
};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::{EnumName, Model};
use katana_geometry::alignment::{SolvedAlignment, solve_alignment};
use katana_geometry::{Box2, Point2};
use katana_math::{DEG_TO_RAD, RAD_TO_DEG, cpp};

use crate::document::Document;
use crate::layer_overrides::LayerOverrides;
use crate::plot::{PaperSize, PlotColourMode, paper_dimensions, plot_colour_mode_from};
use crate::plotting::arrange::{
    AlignEdge, DistributeAxis, PaperAdvice, align_edge_from, distribute_axis_from, draws_in,
    fit_paper_to_viewport, smart_layout_rotated, viewport_content_in_set,
};
use crate::plotting::arrange_commands::{
    align_viewports, auto_arrange_sheet, choose_paper_for_scale, distribute_viewports, drawn_scale,
    fit_viewport_to_content, main_plan_of, match_scale, rotate_to_best_fit,
};
use crate::plotting::frame::BUILT_IN_FRAME_ID;
use crate::plotting::generators::{
    CrossSectionRequest, GridRequest, LayoutRequest, SheetTemplate, StripRequest, blank_sheet,
    cross_section_sheets, grid_sheets, prepare_for_append, sheets_from_plot_frames, smart_layout,
    strip_sheets,
};
use crate::plotting::layout::{
    TILING_GUTTER_MM, TilingPreset, preset_id, preset_name, tile_viewports, tiling_area,
};
use crate::plotting::page_setup::{
    MAXIMUM_LINE_WEIGHT_SCALE, MAXIMUM_PLOT_DPI, MINIMUM_LINE_WEIGHT_SCALE, MINIMUM_PLOT_DPI,
    PageSetup, parse_sheet_selection, set_page_setup, validate_file_name_pattern,
    validate_page_setup,
};
use crate::plotting::plan_grid::grid_style_from;
use crate::plotting::preflight::{
    Finding, PreflightOptions, check_document_sheets, findings_to_json, summarize, summary_text,
};
use crate::plotting::section_fit::SectionFit;
use crate::plotting::sheet_commands::{
    add_sheet, add_sheets, duplicate_sheet, edit_sheet, edit_sheet_set, field_context_for,
    import_image_asset, import_logo, move_sheet, remove_sheet,
};
use crate::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use crate::plotting::sheet_set::{
    GridStyle, LegendScope, Revision, Sheet, SheetSet, Viewport, ViewportKind, digits_prefix,
    legend_scope_from, next_viewport_id, resolve_fields, sheet_index, viewport_kind_from,
};
use crate::plotting::tables::add_register_sheet;
use crate::section::SectionSurfaceInput;
use crate::selection::drawn_extent;

// An Image view may show a photograph of the site, so it is allowed more
// than a logo; still not so much that every PDF of the set carries a raw
// camera file.
const MAXIMUM_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

// ---- what the front end adds ----------------------------------------------------------

/// Whether anything is drawn in a world box: in the window, everything its
/// plan view draws (planDrawsIn).
pub type DrawsInFn<'a> = dyn Fn(&Document, &Box2) -> bool + 'a;
/// The checks on the sheets at the indices given (every sheet when empty),
/// with what the painter knows (checkSheetsFor).
pub type CheckFn<'a> = dyn Fn(&Document, &[usize]) -> Vec<Finding> + 'a;
/// What a viewport shows, as world points.
pub type ContentFn<'a> = dyn Fn(&Document, &Viewport) -> Vec<Point2> + 'a;
/// The scale and exaggeration an automatic section is drawn at.
pub type FitSectionFn<'a> = dyn Fn(&Document, &Viewport) -> SectionFit + 'a;
/// The set as the painter draws it, every automatic scale decided.
pub type DrawnSetFn<'a> = dyn Fn(&Document, &SheetSet) -> SheetSet + 'a;

/// What the verbs need that the Document does not hold, from the front end
/// that has it. The desktop window supplies them; headless they are left
/// empty and the verbs do without. (The surfaces GENERATE SECTIONS samples,
/// to centre each section on its ground and choose the exaggeration, are the
/// drawing's own shown ones, Document::surfaces, on every front end.)
#[derive(Default)]
pub struct SheetVerbContext<'a> {
    /// What GENERATE FIT and GRID cover without area=: in the window,
    /// everything its plan view draws (imagery and point clouds included), as
    /// the sheet editor's Generate Sheets covers. Not given: the drawing's
    /// entities (drawn_extent).
    pub drawing_extent: Option<Box2>,
    /// Whether anything is drawn in a world box, asked of each tile GRID (and
    /// FIT at a fixed scale) cuts, so that a tile over empty ground is left
    /// out. Not given: what a plan of the drawing draws (draws_in).
    pub draws_in: Option<Box<DrawsInFn<'a>>>,
    /// What SHEETS CHECK runs on the sheets at the indices given. Not given:
    /// check_document_sheets.
    pub check: Option<Box<CheckFn<'a>>>,
    /// What a viewport shows, as world points, for ARRANGE MATCHSCALE, VIEW
    /// FIT, VIEW BESTROTATE and SHEET SUGGESTPAPER: in the window with its
    /// reference layers and meshes, and a key plan's outlines placed as the
    /// painter places them. Not given: viewport_content_in_set over the
    /// drawing.
    pub content: Option<Box<ContentFn<'a>>>,
    /// The scale and exaggeration an automatic section is drawn at, for
    /// ARRANGE MATCHSCALE from one: the painter fits it to the ground it
    /// cuts, which the document does not hold. Not given: its stored ones.
    pub fit_section: Option<Box<FitSectionFn<'a>>>,
    /// The set as the painter draws it, every automatic scale decided, for
    /// SHEET FIELD to print the scale the title block prints. Not given: the
    /// stored scales.
    pub drawn: Option<Box<DrawnSetFn<'a>>>,
}

/// Builds the context; called only by the verbs that need it, so a front end
/// may build it lazily.
pub type SheetVerbContextFn<'a> = dyn Fn(&Document) -> SheetVerbContext<'a> + 'a;
/// The C++ `SheetVerbContextProvider`, empty for none. The provider is
/// borrowed for `'p`; the closures it builds may borrow for `'a`.
pub type SheetVerbContextProvider<'p, 'a> = Option<&'p SheetVerbContextFn<'a>>;

fn context_of<'a>(
    provider: SheetVerbContextProvider<'_, 'a>,
    document: &Document,
) -> SheetVerbContext<'a> {
    match provider {
        Some(provide) => provide(document),
        None => SheetVerbContext::default(),
    }
}

// ---- words ---------------------------------------------------------------------------

// Letters and digits only, lower case: "Project_Line_1", "project line 1" and
// "PROJECTLINE1" are one key, so neither a person nor an agent has to
// remember which separator a name uses.
fn folded(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        let lower = ascii_lower(byte);
        if lower.is_ascii_lowercase() || byte.is_ascii_digit() {
            out.push(char::from(lower));
        }
    }
    out
}

fn uppered(text: &str) -> String {
    text.to_ascii_uppercase()
}

fn joined(words: &[String], from: usize, separator: &str) -> String {
    words
        .get(from..)
        .map(|rest| rest.join(separator))
        .unwrap_or_default()
}

// A typed text read back, as a reply writes it (reply_quoted): core's reader.
fn unescaped(text: &str) -> String {
    unescape_typed(text)
}

fn count_of(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn usage(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

// A path as the C++'s pathToUtf8 says it in a message.
fn path_text(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

// ---- values --------------------------------------------------------------------------

// Locale independent ("1.5" is never "1,5"), finite, and an explicit '+'
// allowed. from_chars takes a '-' but no '+': the one '+' is stepped over,
// and what follows it is read as from_chars reads it ("+-1" is -1, "++1" is
// refused).
fn number(text: &str, what: &str) -> Result<f64> {
    let rest = text.strip_prefix('+').unwrap_or(text);
    let value = if rest.starts_with('+') {
        None
    } else {
        parse_finite_double(rest)
    };
    value.ok_or_else(|| {
        make_error(
            ErrorCode::ParseFailure,
            format!("{what} must be a number"),
            text,
        )
    })
}

fn positive(text: &str, what: &str) -> Result<f64> {
    let value = number(text, what)?;
    if !(value > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{what} must be more than 0"),
            text,
        ));
    }
    Ok(value)
}

fn not_negative(text: &str, what: &str) -> Result<f64> {
    let value = number(text, what)?;
    if value < 0.0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{what} cannot be negative"),
            text,
        ));
    }
    Ok(value)
}

// A whole number from 1.
fn counting(text: &str, what: &str) -> Result<usize> {
    match digits_prefix(text.as_bytes()) {
        (Some(value), taken) if !text.is_empty() && taken == text.len() && value != 0 => Ok(value),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("{what} must be a whole number from 1"),
            text,
        )),
    }
}

fn on_off(text: &str, key: &str) -> Result<bool> {
    let word = folded(text);
    match word.as_str() {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("{key}= is on or off"),
            text,
        )),
    }
}

// "a,b,c": numbers separated by commas; an empty text is an empty list.
fn number_list(text: &str, what: &str) -> Result<Vec<f64>> {
    let mut values = Vec::new();
    if text.is_empty() {
        return Ok(values);
    }
    for part in text.split(',') {
        let Ok(value) = number(part, what) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("{what} must be numbers separated by commas"),
                text,
            ));
        };
        values.push(value);
    }
    Ok(values)
}

fn point_from(text: &str, what: &str) -> Result<Point2> {
    match number_list(text, what) {
        Ok(values) if values.len() == 2 => Ok(Point2::new(values[0], values[1])),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("{what} is x,y"),
            text,
        )),
    }
}

// "x0,y0,x1,y1", either corner first; a rectangle with no area is refused.
fn box_from(text: &str, what: &str) -> Result<Box2> {
    let v = match number_list(text, what) {
        Ok(values) if values.len() == 4 => values,
        _ => {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("{what} is x0,y0,x1,y1"),
                text,
            ));
        }
    };
    let rect = Box2::new(
        Point2::new(cpp::min(v[0], v[2]), cpp::min(v[1], v[3])),
        Point2::new(cpp::max(v[0], v[2]), cpp::max(v[1], v[3])),
    );
    if !(rect.width() > 0.0) || !(rect.height() > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{what} must have a width and a height"),
            text,
        ));
    }
    Ok(rect)
}

// 1 : scale, typed as "500" or "1:500"; None for "auto".
fn scale_from(text: &str) -> Result<Option<f64>> {
    if folded(text) == "auto" {
        return Ok(None);
    }
    let denominator = text.strip_prefix("1:").unwrap_or(text);
    Ok(Some(positive(denominator, "scale=")?))
}

// A number as a reply writes it: to a millionth (a micrometre of paper, a
// micro-degree) in its shortest form, so 25.5 is "25.5" and not
// "25.499999999999996", and -0 is "0".
fn decimal(value: f64) -> String {
    let mut rounded = value;
    if (value * 1e6).is_finite() {
        rounded = (value * 1e6).round() / 1e6;
    }
    if rounded == 0.0 {
        rounded = 0.0;
    }
    format_exact_real(rounded)
}

fn point_text(point: &Point2) -> String {
    format!("{},{}", decimal(point.x), decimal(point.y))
}

fn box_text(rect: &Box2) -> String {
    if rect.empty() {
        return String::from("none");
    }
    format!("{},{}", point_text(&rect.min), point_text(&rect.max))
}

fn on_off_text(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

// ---- options -------------------------------------------------------------------------

// A word of the form key=value. The key is folded; the value is as typed.
struct KeyValue {
    key: String,
    value: String,
    // As typed, for an error's context.
    word: String,
}

fn option_from(word: &str) -> Option<KeyValue> {
    let equals = word.find('=')?;
    if equals == 0 {
        return None;
    }
    Some(KeyValue {
        key: folded(&word[..equals]),
        value: String::from(&word[equals + 1..]),
        word: String::from(word),
    })
}

// Every word from `from` on must be key=value.
fn options_from(words: &[String], from: usize, usage_text: &str) -> Result<Vec<KeyValue>> {
    let mut options = Vec::new();
    for word in words.iter().skip(from) {
        let Some(option) = option_from(word) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("expected option=value; usage: {usage_text}"),
                word.as_str(),
            ));
        };
        options.push(option);
    }
    Ok(options)
}

fn unknown_option(option: &KeyValue, place: &str, known: &str) -> Error {
    make_error(
        ErrorCode::InvalidArgument,
        format!("{place} takes no option {}=; it takes {known}", option.key),
        option.word.as_str(),
    )
}

fn paper_from(text: &str) -> Result<PaperSize> {
    const PAPERS: [(&str, PaperSize); 5] = [
        ("a0", PaperSize::A0),
        ("a1", PaperSize::A1),
        ("a2", PaperSize::A2),
        ("a3", PaperSize::A3),
        ("a4", PaperSize::A4),
    ];
    let word = folded(text);
    PAPERS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, paper)| *paper)
        .ok_or_else(|| {
            make_error(
                ErrorCode::InvalidArgument,
                "paper= is A0, A1, A2, A3 or A4",
                text,
            )
        })
}

fn paper_name(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A0 => "A0",
        PaperSize::A1 => "A1",
        PaperSize::A2 => "A2",
        PaperSize::A3 => "A3",
        PaperSize::A4 => "A4",
    }
}

fn landscape_from(text: &str) -> Result<bool> {
    match folded(text).as_str() {
        "landscape" => Ok(true),
        "portrait" => Ok(false),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            "orientation= is landscape or portrait",
            text,
        )),
    }
}

// on (the built-in frame), off or none (no frame), or a frame's id.
fn frame_from(text: &str) -> Result<String> {
    let word = folded(text);
    if word == "on" || word == "yes" || word == "true" || word == folded(BUILT_IN_FRAME_ID) {
        return Ok(String::from(BUILT_IN_FRAME_ID));
    }
    if word == "off" || word == "none" || word == "no" || word == "false" {
        return Ok(String::new());
    }
    Err(make_error(
        ErrorCode::NotFound,
        format!("no plot frame of that name: frame= is on, off or {BUILT_IN_FRAME_ID}"),
        text,
    ))
}

// The kinds by their stored names, and the words a person reaches for.
fn kind_from(text: &str) -> Result<ViewportKind> {
    if let Some(stored) = viewport_kind_from(text) {
        return Ok(stored);
    }
    const WORDS: [(&str, ViewportKind); 23] = [
        ("plan", ViewportKind::Plan),
        ("longsection", ViewportKind::LongSection),
        ("profile", ViewportKind::LongSection),
        ("crosssections", ViewportKind::CrossSections),
        ("crosssection", ViewportKind::CrossSections),
        ("sections", ViewportKind::CrossSections),
        ("xs", ViewportKind::CrossSections),
        ("model3d", ViewportKind::Model3D),
        ("3d", ViewportKind::Model3D),
        ("snapshot", ViewportKind::Model3D),
        ("legend", ViewportKind::Legend),
        ("notes", ViewportKind::Notes),
        ("note", ViewportKind::Notes),
        ("image", ViewportKind::Image),
        ("keyplan", ViewportKind::KeyPlan),
        ("key", ViewportKind::KeyPlan),
        ("sheetindex", ViewportKind::SheetIndex),
        ("register", ViewportKind::SheetIndex),
        ("drawingregister", ViewportKind::SheetIndex),
        ("index", ViewportKind::SheetIndex),
        ("revisions", ViewportKind::Revisions),
        ("revisiontable", ViewportKind::Revisions),
        ("revision", ViewportKind::Revisions),
    ];
    let word = folded(text);
    WORDS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| {
            make_error(
                ErrorCode::InvalidArgument,
                "a view is plan, key_plan, long_section, cross_sections, model_3d, legend, \
                 notes, image, sheet_index (the drawing register) or revisions",
                text,
            )
        })
}

// The presets in the order the C++ enum declares them.
const ALL_PRESETS: [TilingPreset; 8] = [
    TilingPreset::Full,
    TilingPreset::Columns2,
    TilingPreset::Rows2,
    TilingPreset::Quad,
    TilingPreset::MainRight,
    TilingPreset::MainBelow,
    TilingPreset::MainCorner,
    TilingPreset::MainTwoRight,
];

// A preset by its stored id (cols2), its menu name ("Main and panel right")
// or the name of its value (MainRight), in any case and spacing.
fn preset_from(text: &str) -> Result<TilingPreset> {
    let word = folded(text);
    for preset in ALL_PRESETS {
        if word == folded(preset_id(preset)) || word == folded(preset_name(preset)) {
            return Ok(preset);
        }
    }
    const WORDS: [(&str, TilingPreset); 8] = [
        ("columns2", TilingPreset::Columns2),
        ("columns", TilingPreset::Columns2),
        ("rows", TilingPreset::Rows2),
        ("quarters", TilingPreset::Quad),
        ("mainright", TilingPreset::MainRight),
        ("mainbelow", TilingPreset::MainBelow),
        ("maincorner", TilingPreset::MainCorner),
        ("maintworight", TilingPreset::MainTwoRight),
    ];
    WORDS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, preset)| *preset)
        .ok_or_else(|| {
            make_error(
                ErrorCode::InvalidArgument,
                "a tiling preset is full, cols2, rows2, quad, sectionR, sectionB, sectionBR or \
                 sectionMap3d, or its menu name",
                text,
            )
        })
}

// ---- viewports -----------------------------------------------------------------------

fn drawn_to_scale(kind: ViewportKind) -> bool {
    matches!(
        kind,
        ViewportKind::Plan
            | ViewportKind::KeyPlan
            | ViewportKind::LongSection
            | ViewportKind::CrossSections
    )
}

fn is_section(kind: ViewportKind) -> bool {
    matches!(
        kind,
        ViewportKind::LongSection | ViewportKind::CrossSections
    )
}

fn is_plan_like(kind: ViewportKind) -> bool {
    matches!(kind, ViewportKind::Plan | ViewportKind::KeyPlan)
}

fn only_for(viewport: &Viewport, option: &KeyValue, allowed: bool, kinds: &str) -> Status {
    if allowed {
        return Ok(());
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!(
            "{}= is for {kinds} views, not {}",
            option.key,
            viewport.kind.name()
        ),
        option.word.as_str(),
    ))
}

// The editor's placing of a new view: its size for the kind, centred in the
// tiling area, or the whole area on an empty sheet.
fn free_place(sheet: &Sheet, width: f64, height: f64) -> Box2 {
    let area = tiling_area(sheet);
    if sheet.viewports.is_empty() {
        return area;
    }
    let width = cpp::min(width, area.width());
    let height = cpp::min(height, area.height());
    let c = area.center();
    Box2::new(
        Point2::new(c.x - width / 2.0, c.y - height / 2.0),
        Point2::new(c.x + width / 2.0, c.y + height / 2.0),
    )
}

// The chainage half way along alignment `name`, when it solves.
fn middle_station(model: &Model, name: &str) -> Option<f64> {
    let alignment = model.alignments.find(name)?;
    let solved = solve_alignment(&alignment.horizontal).ok()?;
    Some((solved.start_station() + solved.end_station()) / 2.0)
}

fn alignment_named(model: &Model, name: &str) -> Result<String> {
    if model.alignments.find(name).is_none() {
        return Err(make_error(
            ErrorCode::NotFound,
            "the drawing has no alignment of that name",
            name,
        ));
    }
    Ok(String::from(name))
}

// The alignment a generator runs along: the one named, else the drawing's
// only one.
fn alignment_for(model: &Model, given: &str) -> Result<String> {
    if !given.is_empty() {
        return alignment_named(model, given);
    }
    let names = model.alignments.names();
    if names.len() == 1 {
        return Ok(names[0].clone());
    }
    if names.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            "the drawing has no alignment: define one with ALIGN NEW",
        ));
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!(
            "name the alignment with alignment=: the drawing has {}",
            names.join(", ")
        ),
    ))
}

// Where the viewport `id` is: its sheet's index and its own. An id is matched
// as typed and then in any case (VP3 is vp3).
fn find_viewport(set: &SheetSet, id: &str) -> Option<(usize, usize)> {
    for exact in [true, false] {
        for (s, sheet) in set.sheets.iter().enumerate() {
            for (v, viewport) in sheet.viewports.iter().enumerate() {
                let same = if exact {
                    viewport.id == id
                } else {
                    uppered(&viewport.id) == uppered(id)
                };
                if same {
                    return Some((s, v));
                }
            }
        }
    }
    None
}

fn no_viewport(id: &str) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no view with that id; VIEW LIST shows them",
        id,
    )
}

// A view must land on its paper, or it prints nothing.
fn check_on_paper(sheet: &Sheet, viewport: &Viewport) -> Status {
    let paper = paper_dimensions(sheet.paper, sheet.landscape);
    let page = Box2::new(
        Point2::new(0.0, 0.0),
        Point2::new(paper.width_mm, paper.height_mm),
    );
    if viewport.rect.empty() || page.intersects(viewport.rect) {
        return Ok(());
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!(
            "view {} would be off the paper: {} {} is {} x {} mm",
            viewport.id,
            paper_name(sheet.paper),
            if sheet.landscape {
                "landscape"
            } else {
                "portrait"
            },
            decimal(paper.width_mm),
            decimal(paper.height_mm)
        ),
        format!("rect={}", box_text(&viewport.rect)),
    ))
}

// The short line SHEETS LIST gives a view.
fn viewport_summary(viewport: &Viewport) -> String {
    let mut line = format!("view id={} kind={}", viewport.id, viewport.kind.name());
    if drawn_to_scale(viewport.kind) {
        line.push_str(" scale=");
        line.push_str(&if viewport.auto_scale {
            String::from("auto")
        } else {
            decimal(viewport.scale)
        });
    }
    line.push_str(" rect=");
    line.push_str(&box_text(&viewport.rect));
    line
}

// The sheet's own line, without its views.
fn sheet_line(set: &SheetSet, index: usize) -> String {
    let sheet = &set.sheets[index];
    format!(
        "sheet {} id={} name={} paper={} orientation={} frame={} legendblock={} views={}",
        index + 1,
        sheet.id,
        reply_quoted(&sheet.name),
        paper_name(sheet.paper),
        if sheet.landscape {
            "landscape"
        } else {
            "portrait"
        },
        if sheet.frame.is_empty() {
            "none"
        } else {
            sheet.frame.as_str()
        },
        on_off_text(sheet.frame_legend),
        sheet.viewports.len()
    )
}

fn readable(document: &Document) -> Result<Arc<SheetSet>> {
    if let Err(error) = document.sheet_set_status() {
        return Err(make_error(
            error.code,
            format!("the project's sheets cannot be read: {}", error.message),
            error.context,
        ));
    }
    Ok(document.sheet_set())
}

// ---- the title block -----------------------------------------------------------------

// The name TITLEBLOCK takes and lists, and the frame's field of the same
// value, if any.
const TITLE_FIELDS: [(&str, &str); 22] = [
    ("organisation", "organisation"),
    ("project1", "project_line_1"),
    ("project2", "project_line_2"),
    ("project3", "project_line_3"),
    ("project4", "project_line_4"),
    ("client", "client"),
    ("setnumber", "set_number"),
    ("numbering", ""),
    ("coordsys", "coordinate_system"),
    ("datum", "height_datum"),
    ("model", "model_name"),
    ("notes", "notes"),
    ("locatorname", "locator_name"),
    ("locatordate", "locator_date"),
    ("surveyorname", "surveyor_name"),
    ("surveyordate", "surveyor_date"),
    ("compilername", "compiler_name"),
    ("compilerdate", "compiler_date"),
    ("reviewername", "reviewer_name"),
    ("reviewerdate", "reviewer_date"),
    ("approvername", "approver_name"),
    ("approverdate", "approver_date"),
];

fn title_field_from(field: &str) -> Option<&'static str> {
    let word = folded(field);
    for (name, frame_name) in TITLE_FIELDS {
        if word == folded(name) || (!frame_name.is_empty() && word == folded(frame_name)) {
            return Some(name);
        }
    }
    match word.as_str() {
        "organization" | "org" => Some("organisation"),
        "set" => Some("setnumber"),
        _ => None,
    }
}

// The string field `name` (a TITLE_FIELDS name, not a project line) is kept
// in.
fn title_slot<'s>(set: &'s SheetSet, name: &str) -> Option<&'s String> {
    let d = &set.defaults;
    Some(match name {
        "organisation" => &d.organisation,
        "client" => &d.client,
        "setnumber" => &d.set_number,
        "numbering" => &set.numbering,
        "coordsys" => &d.coordinate_system,
        "datum" => &d.height_datum,
        "model" => &d.model_name,
        "notes" => &d.notes,
        "locatorname" => &d.locator.name,
        "locatordate" => &d.locator.date,
        "surveyorname" => &d.surveyor.name,
        "surveyordate" => &d.surveyor.date,
        "compilername" => &d.compiler.name,
        "compilerdate" => &d.compiler.date,
        "reviewername" => &d.reviewer.name,
        "reviewerdate" => &d.reviewer.date,
        "approvername" => &d.approver.name,
        "approverdate" => &d.approver.date,
        _ => return None,
    })
}

fn title_slot_mut<'s>(set: &'s mut SheetSet, name: &str) -> Option<&'s mut String> {
    let d = &mut set.defaults;
    Some(match name {
        "organisation" => &mut d.organisation,
        "client" => &mut d.client,
        "setnumber" => &mut d.set_number,
        "numbering" => &mut set.numbering,
        "coordsys" => &mut d.coordinate_system,
        "datum" => &mut d.height_datum,
        "model" => &mut d.model_name,
        "notes" => &mut d.notes,
        "locatorname" => &mut d.locator.name,
        "locatordate" => &mut d.locator.date,
        "surveyorname" => &mut d.surveyor.name,
        "surveyordate" => &mut d.surveyor.date,
        "compilername" => &mut d.compiler.name,
        "compilerdate" => &mut d.compiler.date,
        "reviewername" => &mut d.reviewer.name,
        "reviewerdate" => &mut d.reviewer.date,
        "approvername" => &mut d.approver.name,
        "approverdate" => &mut d.approver.date,
        _ => return None,
    })
}

// 1 to 4 for project1..project4, else 0.
fn project_line(name: &str) -> usize {
    let bytes = name.as_bytes();
    if bytes.len() == 8 && name.starts_with("project") && (b'1'..=b'4').contains(&bytes[7]) {
        return usize::from(bytes[7] - b'0');
    }
    0
}

fn unknown_title_field(field: &str) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no title-block field of that name; TITLEBLOCK LIST shows them",
        field,
    )
}

fn revision_line(revision: &Revision) -> String {
    format!(
        "revision code={} date={} description={} by={}",
        reply_quoted(&revision.code),
        reply_quoted(&revision.date),
        reply_quoted(&revision.description),
        reply_quoted(&revision.by)
    )
}

// ---- SHEETS --------------------------------------------------------------------------

// The page setup as SHEETS PAGESETUP prints it, in the words it takes.
fn page_setup_line(setup: &PageSetup) -> String {
    format!(
        "pagesetup style={} lineweight={} dpi={} pattern={} filepersheet={}",
        setup.colour_mode.name(),
        decimal(setup.line_weight_scale),
        decimal(setup.dpi),
        reply_quoted(&setup.file_name_pattern),
        on_off_text(setup.file_per_sheet)
    )
}

// The plot style by its stored name or a common other one
// (plot_colour_mode_from).
fn style_from(text: &str) -> Result<PlotColourMode> {
    plot_colour_mode_from(text).ok_or_else(|| {
        make_error(
            ErrorCode::InvalidArgument,
            "style= is colour, grey or mono",
            text,
        )
    })
}

// SHEETS PAGESETUP [style=] [lineweight=] [dpi=] [pattern=] [filepersheet=]:
// the page setup kept with the set, printed, or changed as one step.
fn page_setup_verb(document: &mut Document, args: &[String]) -> Result<String> {
    const USAGE: &str = "SHEETS PAGESETUP [style=colour|grey|mono] [lineweight=f] [dpi=n] \
                         [pattern=text] [filepersheet=on|off]";
    let options = options_from(args, 1, USAGE)?;
    let mut setup = document.sheet_set().page_setup.clone();
    if options.is_empty() {
        return Ok(page_setup_line(&setup));
    }
    for option in &options {
        match option.key.as_str() {
            "style" | "colour" | "color" | "mode" => setup.colour_mode = style_from(&option.value)?,
            "lineweight" | "lineweightscale" => {
                setup.line_weight_scale = number(&option.value, "lineweight=")?;
            }
            "dpi" => setup.dpi = number(&option.value, "dpi=")?,
            "pattern" | "names" => setup.file_name_pattern = unescaped(&option.value),
            "filepersheet" => setup.file_per_sheet = on_off(&option.value, &option.key)?,
            _ => {
                return Err(unknown_option(
                    option,
                    "a page setup",
                    "style=, lineweight=, dpi=, pattern= and filepersheet=",
                ));
            }
        }
    }
    // Checked before the step, so a refused value says which it was.
    validate_page_setup(&setup)?;
    set_page_setup(document, &setup)?;
    Ok(page_setup_line(&document.sheet_set().page_setup))
}

// SHEETS CHECK [sheets=1,3-5] [json]: the preflight checks, a summary line and
// then a finding a line (check_reply_line), or the findings as JSON.
fn check_verb(
    document: &Document,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    const USAGE: &str = "SHEETS CHECK [sheets=1,3-5] [json]";
    let set = document.sheet_set();
    let mut indices: Vec<usize> = Vec::new();
    let mut json = false;
    for word in args.iter().skip(1) {
        if folded(word) == "json" {
            json = true;
            continue;
        }
        let Some(option) = option_from(word) else {
            return Err(usage(USAGE));
        };
        if option.key != "sheets" && option.key != "sheet" {
            return Err(usage(USAGE));
        }
        indices = parse_sheet_selection(&option.value, &set)?;
    }
    let given = context_of(context, document);
    let findings = match &given.check {
        Some(check) => check(document, indices.as_slice()),
        None => check_document_sheets(
            document,
            PreflightOptions {
                sheets: indices.clone(),
                ..PreflightOptions::default()
            },
        ),
    };
    if json {
        return Ok(findings_to_json(&findings));
    }
    let checked = if indices.is_empty() {
        set.sheets.len()
    } else {
        indices.len()
    };
    let mut reply = format!(
        "checked {}: {}",
        count_of(checked, "sheet", "sheets"),
        summary_text(&summarize(&findings))
    );
    for finding in &findings {
        reply.push('\n');
        reply.push_str(&check_reply_line(finding));
    }
    Ok(reply)
}

fn sheets_verb(
    document: &mut Document,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    const USAGE: &str = "SHEETS [LIST] | JSON [path] | SAVE path | LOAD path | APPEND path | \
                         CHECK [sheets=] [json] | PAGESETUP [option=value ...]";
    let action = args
        .first()
        .map_or_else(|| String::from("LIST"), |first| uppered(first));
    match action.as_str() {
        "LIST" => {
            if args.len() > 1 {
                return Err(usage(USAGE));
            }
            let set = readable(document)?;
            let mut reply = count_of(set.sheets.len(), "sheet", "sheets");
            for i in 0..set.sheets.len() {
                reply.push('\n');
                reply.push_str(&describe_sheet(&set, i));
            }
            Ok(reply)
        }
        "JSON" | "SAVE" => {
            let wrong = if action == "SAVE" {
                args.len() != 2
            } else {
                args.len() > 2
            };
            if wrong {
                return Err(usage(USAGE));
            }
            let set = readable(document)?;
            if args.len() == 1 {
                return sheet_set_to_json(&set);
            }
            write_sheet_set_file(&set, &path_from_utf8(&args[1]))?;
            Ok(format!(
                "wrote {} to \"{}\"",
                count_of(set.sheets.len(), "sheet", "sheets"),
                args[1]
            ))
        }
        "CHECK" | "PREFLIGHT" => check_verb(document, args, context),
        "PAGESETUP" | "SETUP" => page_setup_verb(document, args),
        "LOAD" => {
            if args.len() != 2 {
                return Err(usage(USAGE));
            }
            let loaded = read_sheet_set_file(&path_from_utf8(&args[1]))?;
            let count = loaded.sheets.len();
            document.set_sheet_set(&loaded, "LOAD_SHEETS")?;
            Ok(format!(
                "loaded {} from \"{}\"",
                count_of(count, "sheet", "sheets"),
                args[1]
            ))
        }
        "APPEND" => {
            // Another set's sheets after these, as one step: renumbered so
            // they join without clashing (prepare_for_append, which also moves
            // a key plan's references among them). The file's title block,
            // revisions and page setup stay behind: they are the set's, and
            // this set keeps its own.
            if args.len() != 2 {
                return Err(usage(USAGE));
            }
            let loaded = read_sheet_set_file(&path_from_utf8(&args[1]))?;
            let count = loaded.sheets.len();
            if count == 0 {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "the file holds no sheets to append",
                    format!("\"{}\"", args[1]),
                ));
            }
            let first = document.sheet_set().sheets.len();
            add_sheets(document, loaded.sheets, "APPEND_SHEETS")?;
            let mut reply = format!(
                "appended {} from \"{}\": {}",
                count_of(count, "sheet", "sheets"),
                args[1],
                if count == 1 {
                    format!("{}", first + 1)
                } else {
                    format!("{} to {}", first + 1, first + count)
                }
            );
            let now = document.sheet_set();
            for i in first..first + count {
                reply.push('\n');
                reply.push_str(&describe_sheet(&now, i));
            }
            Ok(reply)
        }
        _ => Err(usage(USAGE)),
    }
}

// ---- SHEET ---------------------------------------------------------------------------

// A sheet option of SHEET NEW and SHEET SET, applied to `sheet`.
fn set_sheet_option(sheet: &mut Sheet, option: &KeyValue) -> Status {
    match option.key.as_str() {
        "paper" => sheet.paper = paper_from(&option.value)?,
        "orientation" => sheet.landscape = landscape_from(&option.value)?,
        "frame" => sheet.frame = frame_from(&option.value)?,
        "legendblock" => sheet.frame_legend = on_off(&option.value, &option.key)?,
        "name" => {
            if option.value.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a sheet needs a name",
                    option.word.as_str(),
                ));
            }
            sheet.name = unescaped(&option.value);
        }
        _ => {
            return Err(unknown_option(
                option,
                "a sheet",
                "paper=, orientation=, frame=, legendblock= and name=",
            ));
        }
    }
    Ok(())
}

// The bare words PORTRAIT and LANDSCAPE, as SHEET NEW and SHEET SET take them.
fn orientation_word(word: &str) -> Option<bool> {
    match folded(word).as_str() {
        "portrait" => Some(false),
        "landscape" => Some(true),
        _ => None,
    }
}

fn new_sheet(document: &mut Document, args: &[String]) -> Result<String> {
    let set = document.sheet_set();
    let mut sheet = blank_sheet(&SheetTemplate::default(), String::new());
    let mut name_words: Vec<String> = Vec::new();
    let mut at: Option<usize> = None;
    let mut named = false;
    for word in args.iter().skip(1) {
        let Some(option) = option_from(word) else {
            match orientation_word(word) {
                Some(landscape) => sheet.landscape = landscape,
                None => name_words.push(word.clone()),
            }
            continue;
        };
        if option.key == "at" {
            let position = counting(&option.value, "at=")?;
            if position > set.sheets.len() + 1 {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "at= is 1 to {}: the set has {}",
                        set.sheets.len() + 1,
                        count_of(set.sheets.len(), "sheet", "sheets")
                    ),
                    option.word,
                ));
            }
            at = Some(position - 1);
            continue;
        }
        set_sheet_option(&mut sheet, &option)?;
        named = named || option.key == "name";
    }
    if named && !name_words.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the name is given twice",
            joined(&name_words, 0, " "),
        ));
    }
    if !named {
        sheet.name = if name_words.is_empty() {
            format!("SHEET {}", set.sheets.len() + 1)
        } else {
            unescaped(&joined(&name_words, 0, " "))
        };
    }
    let position = at.unwrap_or(set.sheets.len());
    add_sheet(document, sheet, Some(position))?;
    Ok(format!(
        "added {}",
        sheet_line(&document.sheet_set(), position)
    ))
}

fn sheet_field(
    document: &mut Document,
    index: usize,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    if args.len() < 3 {
        return Err(usage(
            "SHEET FIELD n field [value]   (\"\" clears the sheet's own value)",
        ));
    }
    let set = document.sheet_set();
    let stored: &SheetSet = &set;
    let reading: &Document = document;
    // The automatic values as the title block prints them: with the front
    // end's scales for automatic views when it has them.
    let given = if context.is_some() && args.len() == 3 {
        context_of(context, reading)
    } else {
        SheetVerbContext::default()
    };
    let drawn_set: Option<SheetSet> = given.drawn.as_ref().map(|drawn| drawn(reading, stored));
    let source: &SheetSet = drawn_set.as_ref().unwrap_or(stored);
    let automatic = resolve_fields(source, index, &field_context_for(reading, String::new()));
    let mut field = String::new();
    for name in automatic.keys() {
        if folded(name) == folded(&args[2]) {
            field = name.clone();
        }
    }
    if field.is_empty() {
        let names: Vec<&str> = automatic.keys().map(String::as_str).collect();
        return Err(make_error(
            ErrorCode::NotFound,
            format!(
                "the frame prints no field of that name; it prints {}",
                names.join(" ")
            ),
            args[2].as_str(),
        ));
    }
    let sheet = &set.sheets[index];
    if args.len() == 3 {
        if let Some(own) = sheet.fields.get(&field) {
            return Ok(format!(
                "sheet {} field {}={}",
                index + 1,
                field,
                reply_quoted(own)
            ));
        }
        return Ok(format!(
            "sheet {} field {}={} automatic",
            index + 1,
            field,
            reply_quoted(automatic.get(&field).map_or("", String::as_str))
        ));
    }
    let value = unescaped(&joined(args, 3, " "));
    edit_sheet(
        document,
        index,
        |edited| {
            if value.is_empty() {
                edited.fields.remove(&field);
            } else {
                edited.fields.insert(field.clone(), value.clone());
            }
            Ok(())
        },
        "SET_SHEET_FIELD",
    )?;
    if value.is_empty() {
        return Ok(format!("sheet {} field {} cleared", index + 1, field));
    }
    Ok(format!(
        "sheet {} field {}={}",
        index + 1,
        field,
        reply_quoted(&value)
    ))
}

// What `viewport` shows, as world points: the front end's (with its imagery,
// and a key plan's outlines where the painter puts each plan), else the
// drawing's (viewport_content_in_set, so a key plan's are its live outlines).
fn content_of(
    document: &Document,
    given: &SheetVerbContext<'_>,
    viewport: &Viewport,
) -> Vec<Point2> {
    match &given.content {
        Some(content) => content(document, viewport),
        None => viewport_content_in_set(document.model(), &document.sheet_set(), viewport, None),
    }
}

fn paper_advice_line(advice: &PaperAdvice) -> String {
    format!(
        "paper={} orientation={} frame={} fill={}",
        paper_name(advice.paper),
        if advice.landscape {
            "landscape"
        } else {
            "portrait"
        },
        if advice.frame.is_empty() {
            "off"
        } else {
            advice.frame.as_str()
        },
        decimal((advice.fill * 1000.0).round() / 1000.0)
    )
}

// SHEET SUGGESTPAPER n [scale=n] [apply=on]: the smallest paper that holds
// what the sheet's main plan shows at a scale (its own, or scale=), as the
// editor's Arrange > Choose Paper for This Scale works it out; apply=on puts
// the sheet on it as one step.
fn suggest_paper_verb(
    document: &mut Document,
    index: usize,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    const USAGE: &str = "SHEET SUGGESTPAPER n [scale=n] [apply=on|off]";
    let options = options_from(args, 2, USAGE)?;
    let mut scale: Option<f64> = None;
    let mut apply = false;
    for option in &options {
        match option.key.as_str() {
            // auto: the scale it is drawn at.
            "scale" => scale = scale_from(&option.value)?,
            "apply" => apply = on_off(&option.value, &option.key)?,
            _ => {
                return Err(unknown_option(
                    option,
                    "SHEET SUGGESTPAPER",
                    "scale= and apply=",
                ));
            }
        }
    }
    let set = document.sheet_set();
    let id = main_plan_of(&set, index);
    if id.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the sheet has no plan or key plan to choose paper for",
            format!("{}", index + 1),
        ));
    }
    let Some((s, v)) = find_viewport(&set, &id) else {
        return Err(no_viewport(&id));
    };
    let plan = &set.sheets[s].viewports[v];
    let given = context_of(context, document);
    let content = content_of(document, &given, plan);
    if content.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the plan shows nothing to choose paper for",
            id,
        ));
    }
    let scale_used = scale.unwrap_or_else(|| drawn_scale(plan, &content));
    if apply {
        let change = choose_paper_for_scale(document, &id, &content, Some(scale_used))?;
        return Ok(format!(
            "sheet {} view={} scale={} {}\n{}",
            index + 1,
            id,
            decimal(scale_used),
            paper_advice_line(&change.advice),
            describe_sheet(&document.sheet_set(), index)
        ));
    }
    // Worked out on a copy of the sheet: the same answer apply=on gives.
    let mut trial = set.sheets[index].clone();
    let change = fit_paper_to_viewport(&mut trial, &id, &content, scale_used)?;
    Ok(format!(
        "sheet {} view={} scale={} {}",
        index + 1,
        id,
        decimal(scale_used),
        paper_advice_line(&change.advice)
    ))
}

fn sheet_verb(
    document: &mut Document,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    const USAGE: &str = "SHEET NEW [name] [option=value ...] | REMOVE n | MOVE n to | COPY n | \
                         RENAME n name | SET n option=value ... | FIELD n field [value] | \
                         SUGGESTPAPER n [scale=n] [apply=on]";
    let Some(first) = args.first() else {
        return Err(usage(USAGE));
    };
    let action = uppered(first);
    if action == "NEW" || action == "ADD" {
        return new_sheet(document, args);
    }
    if args.len() < 2 {
        return Err(usage(USAGE));
    }
    let set = document.sheet_set();
    let index = sheet_index_from(&set, &args[1])?;

    match action.as_str() {
        "REMOVE" | "DELETE" => {
            if args.len() != 2 {
                return Err(usage("SHEET REMOVE n"));
            }
            let gone = format!(
                "removed sheet {} id={} name={}",
                index + 1,
                set.sheets[index].id,
                reply_quoted(&set.sheets[index].name)
            );
            remove_sheet(document, index)?;
            Ok(gone)
        }
        "MOVE" => {
            if args.len() != 3 {
                return Err(usage("SHEET MOVE n to   (to: the number it becomes)"));
            }
            let to = counting(&args[2], "the position to move to")?;
            if to > set.sheets.len() {
                return Err(make_error(
                    ErrorCode::NotFound,
                    format!(
                        "no position {to}: the set has {}",
                        count_of(set.sheets.len(), "sheet", "sheets")
                    ),
                    args[2].as_str(),
                ));
            }
            let id = set.sheets[index].id.clone();
            move_sheet(document, index, to - 1)?;
            Ok(format!("moved sheet id={id} from {} to {to}", index + 1))
        }
        "COPY" | "DUPLICATE" => {
            if args.len() != 2 {
                return Err(usage("SHEET COPY n"));
            }
            duplicate_sheet(document, index)?;
            Ok(format!(
                "copied sheet {} to {}",
                index + 1,
                sheet_line(&document.sheet_set(), index + 1)
            ))
        }
        "RENAME" => {
            let name = unescaped(&joined(args, 2, " "));
            if name.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a sheet needs a name",
                    "SHEET RENAME n name",
                ));
            }
            edit_sheet(
                document,
                index,
                |edited| {
                    edited.name = name;
                    Ok(())
                },
                "RENAME_SHEET",
            )?;
            Ok(format!(
                "renamed {}",
                sheet_line(&document.sheet_set(), index)
            ))
        }
        "SET" => {
            if args.len() < 3 {
                return Err(usage(
                    "SHEET SET n [paper=A0..A4] [orientation=landscape|portrait] \
                     [frame=on|off] [legendblock=on|off] [name=text]",
                ));
            }
            edit_sheet(
                document,
                index,
                |edited| {
                    for word in &args[2..] {
                        if let Some(landscape) = orientation_word(word) {
                            edited.landscape = landscape;
                            continue;
                        }
                        let Some(option) = option_from(word) else {
                            return Err(make_error(
                                ErrorCode::InvalidArgument,
                                "expected option=value, or portrait or landscape",
                                word.as_str(),
                            ));
                        };
                        set_sheet_option(edited, &option)?;
                    }
                    Ok(())
                },
                "EDIT_SHEET",
            )?;
            Ok(format!("set {}", sheet_line(&document.sheet_set(), index)))
        }
        "FIELD" => sheet_field(document, index, args, context),
        "SUGGESTPAPER" | "PAPER" => suggest_paper_verb(document, index, args, context),
        _ => Err(usage(USAGE)),
    }
}

// ---- VIEW ----------------------------------------------------------------------------

// file=path for an Image view: the image copied into the project's assets.
fn image_option(document: &Document, viewport: &Viewport, option: &KeyValue) -> Result<String> {
    if viewport.kind != ViewportKind::Image {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("file= is for image views, not {}", viewport.kind.name()),
            option.word.as_str(),
        ));
    }
    import_image_asset(
        document,
        &path_from_utf8(&option.value),
        MAXIMUM_IMAGE_BYTES,
        "image",
    )
}

// The options of VIEW ADD and VIEW SET applied to `viewport` on `sheet`:
// file= as `imported` (already copied), the rest by set_viewport_option.
fn apply_view_options(
    viewport: &mut Viewport,
    sheet: &Sheet,
    options: &[KeyValue],
    imported: Option<&str>,
    model: &Model,
) -> Status {
    for option in options {
        if option.key == "file" {
            if let Some(name) = imported {
                viewport.text = String::from(name);
            }
            continue;
        }
        set_viewport_option(viewport, &option.key, &option.value, model)?;
    }
    check_on_paper(sheet, viewport)
}

fn has_option(options: &[KeyValue], key: &str) -> bool {
    options.iter().any(|option| option.key == key)
}

fn view_add(document: &mut Document, args: &[String]) -> Result<String> {
    const USAGE: &str = "VIEW ADD n kind [option=value ...]";
    if args.len() < 3 {
        return Err(usage(USAGE));
    }
    let set = document.sheet_set();
    let index = sheet_index_from(&set, &args[1])?;
    let kind = kind_from(&args[2])?;
    let options = options_from(args, 3, USAGE)?;
    let model = document.model();
    let mut viewport = default_viewport(&set, index, kind, model);
    // Everything but the file first, so a refused option copies nothing.
    apply_view_options(&mut viewport, &set.sheets[index], &options, None, model)?;
    // Cross sections moved to another alignment are cut half way along it, as
    // they are cut half way along the first one by default.
    if viewport.kind == ViewportKind::CrossSections
        && has_option(&options, "alignment")
        && !has_option(&options, "stations")
        && !has_option(&options, "interval")
    {
        viewport.source.stations.clear();
        if let Some(middle) = middle_station(model, &viewport.source.alignment) {
            viewport.source.stations = vec![middle];
        }
    }
    for option in &options {
        if option.key == "file" {
            viewport.text = image_option(document, &viewport, option)?;
        }
    }
    if viewport.kind == ViewportKind::Image && viewport.text.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "an image view needs file=path (copied into the project's assets) or text=name of a \
             file already in them",
        ));
    }
    let added = viewport.clone();
    edit_sheet(
        document,
        index,
        |edited| {
            edited.viewports.push(added);
            Ok(())
        },
        "ADD_VIEWPORT",
    )?;
    Ok(format!(
        "added view {} to sheet {}\n{}",
        viewport.id,
        index + 1,
        describe_viewport(&viewport)
    ))
}

fn view_set(document: &mut Document, args: &[String]) -> Result<String> {
    const USAGE: &str = "VIEW SET id option=value ...";
    if args.len() < 3 {
        return Err(usage(USAGE));
    }
    let set = document.sheet_set();
    let Some((sheet_at, view_at)) = find_viewport(&set, &args[1]) else {
        return Err(no_viewport(&args[1]));
    };
    let options = options_from(args, 2, USAGE)?;
    let model = document.model();
    let sheet = &set.sheets[sheet_at];
    // Tried on a copy first, so a refused option copies no file.
    let mut trial = sheet.viewports[view_at].clone();
    apply_view_options(&mut trial, sheet, &options, None, model)?;
    let mut imported: Option<String> = None;
    for option in &options {
        if option.key == "file" {
            imported = Some(image_option(document, &trial, option)?);
        }
    }
    // The C++ applies the options again inside the step, to the sheet the
    // step copies; that sheet is this one (nothing has changed it since), so
    // they are applied here, where the model can be read, and the step stores
    // the result.
    let mut edited_view = sheet.viewports[view_at].clone();
    apply_view_options(
        &mut edited_view,
        sheet,
        &options,
        imported.as_deref(),
        model,
    )?;
    edit_sheet(
        document,
        sheet_at,
        |edited| {
            edited.viewports[view_at] = edited_view;
            Ok(())
        },
        "EDIT_VIEWPORT",
    )?;
    Ok(describe_viewport(
        &document.sheet_set().sheets[sheet_at].viewports[view_at],
    ))
}

// VIEW FIT id and VIEW BESTROTATE id: a plan's rectangle fitted to what it
// shows, or the plan turned to the rotation it shows most of at, each one step
// (arrange_commands.rs).
fn view_fit_verb(
    document: &mut Document,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
    rotate: bool,
) -> Result<String> {
    if args.len() != 2 {
        return Err(usage(if rotate {
            "VIEW BESTROTATE id"
        } else {
            "VIEW FIT id"
        }));
    }
    let Some((s, v)) = find_viewport(&document.sheet_set(), &args[1]) else {
        return Err(no_viewport(&args[1]));
    };
    let viewport = document.sheet_set().sheets[s].viewports[v].clone();
    if !is_plan_like(viewport.kind) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{} is for plans and key plans, not {}",
                if rotate {
                    "VIEW BESTROTATE"
                } else {
                    "VIEW FIT"
                },
                viewport.kind.name()
            ),
            viewport.id.as_str(),
        ));
    }
    let given = context_of(context, document);
    let content = content_of(document, &given, &viewport);
    if content.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the view shows nothing to fit to",
            viewport.id.as_str(),
        ));
    }
    if rotate {
        let fit = rotate_to_best_fit(document, &viewport.id, &content)?;
        return Ok(format!(
            "rotated {} rotation={} scale={}\n{}",
            viewport.id,
            decimal(fit.rotation * RAD_TO_DEG),
            decimal(fit.standard_scale),
            describe_viewport(&document.sheet_set().sheets[s].viewports[v])
        ));
    }
    fit_viewport_to_content(document, &viewport.id, &content, None)?;
    Ok(format!(
        "fitted {}\n{}",
        viewport.id,
        describe_viewport(&document.sheet_set().sheets[s].viewports[v])
    ))
}

fn view_verb(
    document: &mut Document,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    const USAGE: &str = "VIEW ADD n kind [option=value ...] | SET id option=value ... | REMOVE \
                         id | LIST [n] | FIT id | BESTROTATE id";
    let Some(first) = args.first() else {
        return Err(usage(USAGE));
    };
    match uppered(first).as_str() {
        "ADD" | "NEW" => view_add(document, args),
        "SET" => view_set(document, args),
        "FIT" => view_fit_verb(document, args, context, false),
        "BESTROTATE" | "ROTATE" => view_fit_verb(document, args, context, true),
        "REMOVE" | "DELETE" => {
            if args.len() != 2 {
                return Err(usage("VIEW REMOVE id"));
            }
            let set = document.sheet_set();
            let Some((sheet_at, view_at)) = find_viewport(&set, &args[1]) else {
                return Err(no_viewport(&args[1]));
            };
            // The id as stored: the one typed may differ in case (VP2 is vp2).
            let id = set.sheets[sheet_at].viewports[view_at].id.clone();
            edit_sheet(
                document,
                sheet_at,
                |edited| {
                    edited.viewports.remove(view_at);
                    Ok(())
                },
                "REMOVE_VIEWPORT",
            )?;
            Ok(format!("removed view {id} from sheet {}", sheet_at + 1))
        }
        "LIST" => {
            if args.len() > 2 {
                return Err(usage("VIEW LIST [n]"));
            }
            let set = readable(document)?;
            let mut first_sheet = 0;
            let mut last_sheet = set.sheets.len();
            if args.len() == 2 {
                let found = sheet_index_from(&set, &args[1])?;
                first_sheet = found;
                last_sheet = found + 1;
            }
            if first_sheet == last_sheet {
                return Ok(String::from("0 sheets"));
            }
            let mut reply = String::new();
            for i in first_sheet..last_sheet {
                let sheet = &set.sheets[i];
                if !reply.is_empty() {
                    reply.push('\n');
                }
                reply.push_str(&format!(
                    "sheet {} id={} views={}",
                    i + 1,
                    sheet.id,
                    sheet.viewports.len()
                ));
                for viewport in &sheet.viewports {
                    reply.push('\n');
                    reply.push_str(&describe_viewport(viewport));
                }
            }
            Ok(reply)
        }
        _ => Err(usage(USAGE)),
    }
}

// ---- TILE ----------------------------------------------------------------------------

fn tile_verb(document: &mut Document, args: &[String]) -> Result<String> {
    if args.len() < 2 {
        return Err(usage(
            "TILE n preset   (full cols2 rows2 quad sectionR sectionB sectionBR sectionMap3d, \
             or the menu's name)",
        ));
    }
    let index = sheet_index_from(&document.sheet_set(), &args[0])?;
    // The rest of the line, so a menu name needs no quotes.
    let preset = preset_from(&joined(args, 1, " "))?;
    let mut placed = 0;
    edit_sheet(
        document,
        index,
        |edited| {
            placed = tile_viewports(edited, preset, TILING_GUTTER_MM);
            Ok(())
        },
        "TILE_VIEWPORTS",
    )?;
    let mut reply = format!(
        "tiled {} on sheet {} preset={}",
        count_of(placed, "view", "views"),
        index + 1,
        preset_id(preset)
    );
    for viewport in &document.sheet_set().sheets[index].viewports {
        reply.push('\n');
        reply.push_str(&viewport_summary(viewport));
    }
    Ok(reply)
}

// ---- ARRANGE -------------------------------------------------------------------------

// Views by their ids, all on one sheet: that sheet, and the ids as stored.
fn views_on_one_sheet(
    set: &SheetSet,
    args: &[String],
    from: usize,
) -> Result<(usize, Vec<String>)> {
    let mut sheet: Option<usize> = None;
    let mut ids: Vec<String> = Vec::new();
    for word in args.iter().skip(from) {
        let Some((s, v)) = find_viewport(set, word) else {
            return Err(no_viewport(word));
        };
        if let Some(kept) = sheet
            && kept != s
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the views are on sheets {} and {}: they are arranged a sheet at a time",
                    kept + 1,
                    s + 1
                ),
                word.as_str(),
            ));
        }
        sheet = Some(s);
        ids.push(set.sheets[s].viewports[v].id.clone());
    }
    let Some(sheet) = sheet else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "name the views by their ids (vp1 vp2 ...)",
        ));
    };
    Ok((sheet, ids))
}

fn id_list(ids: &[String]) -> String {
    ids.join(",")
}

// ARRANGE n | ALIGN edge id ... | DISTRIBUTE across|up id ... | MATCHSCALE from
// id ...: the editor's Arrange menu, each one step (arrange_commands.rs).
fn arrange_verb(
    document: &mut Document,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    const USAGE: &str = "ARRANGE n | ALIGN left|right|top|bottom|hcentre|vcentre id id ... | \
                         DISTRIBUTE across|up id id id ... | MATCHSCALE from id id ...";
    let Some(first) = args.first() else {
        return Err(usage(USAGE));
    };
    let set = document.sheet_set();
    let action = uppered(first);
    if action == "ALIGN" {
        if args.len() < 3 {
            return Err(usage(
                "ARRANGE ALIGN left|right|top|bottom|hcentre|vcentre id id ...",
            ));
        }
        let word = folded(&args[1]);
        let edge = match align_edge_from(&word) {
            Some(edge) => edge,
            None if word == "centre" || word == "center" => AlignEdge::HorizontalCentre,
            None if word == "middle" => AlignEdge::VerticalCentre,
            None => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "an edge is left, right, top, bottom, hcentre or vcentre",
                    args[1].as_str(),
                ));
            }
        };
        let (sheet, ids) = views_on_one_sheet(&set, args, 2)?;
        let moved = align_viewports(document, sheet, &ids, edge)?;
        return Ok(format!(
            "aligned {} on sheet {} edge={} moved={}",
            id_list(&ids),
            sheet + 1,
            edge.name(),
            id_list(&moved)
        ));
    }
    if action == "DISTRIBUTE" {
        if args.len() < 3 {
            return Err(usage("ARRANGE DISTRIBUTE across|up id id id ..."));
        }
        let word = folded(&args[1]);
        let axis = match distribute_axis_from(&word) {
            Some(axis) => axis,
            None if word == "across" || word == "x" => DistributeAxis::Horizontal,
            None if word == "up" || word == "y" => DistributeAxis::Vertical,
            None => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "distribute across or up",
                    args[1].as_str(),
                ));
            }
        };
        let (sheet, ids) = views_on_one_sheet(&set, args, 2)?;
        let moved = distribute_viewports(document, sheet, &ids, axis)?;
        return Ok(format!(
            "distributed {} on sheet {} axis={} moved={}",
            id_list(&ids),
            sheet + 1,
            axis.name(),
            id_list(&moved)
        ));
    }
    if action == "MATCHSCALE" || action == "MATCH" {
        if args.len() < 3 {
            return Err(usage(
                "ARRANGE MATCHSCALE from id id ...   (from: the view whose scale the others take)",
            ));
        }
        let Some((fs, fv)) = find_viewport(&set, &args[1]) else {
            return Err(no_viewport(&args[1]));
        };
        let source = set.sheets[fs].viewports[fv].clone();
        let mut ids: Vec<String> = Vec::new();
        for word in &args[2..] {
            let Some((s, v)) = find_viewport(&set, word) else {
                return Err(no_viewport(word));
            };
            ids.push(set.sheets[s].viewports[v].id.clone());
        }
        // An automatic plan is matched at the scale it is drawn at, with what
        // the front end's plan shows; an automatic section at the scale and
        // exaggeration the front end fits it to.
        let mut scale: Option<f64> = None;
        let mut exaggeration: Option<f64> = None;
        if source.auto_scale {
            let reading: &Document = document;
            let given = context_of(context, reading);
            if is_plan_like(source.kind) {
                scale = Some(drawn_scale(&source, &content_of(reading, &given, &source)));
            } else if is_section(source.kind)
                && let Some(fit_section) = &given.fit_section
            {
                let fit = fit_section(reading, &source);
                scale = Some(fit.scale);
                exaggeration = Some(fit.exaggeration);
            }
        }
        let changed = match_scale(document, &ids, &source.id, scale, exaggeration)?;
        return Ok(format!(
            "matched {} to {} scale={} changed={}",
            id_list(&ids),
            source.id,
            decimal(scale.unwrap_or(source.scale)),
            id_list(&changed)
        ));
    }
    if args.len() != 1 {
        return Err(usage(USAGE));
    }
    let index = sheet_index_from(&set, first)?;
    let arranged = auto_arrange_sheet(document, index)?;
    Ok(format!(
        "arranged sheet {} moved={} overlapping={} unplaced={} mainshrunk={}\n{}",
        index + 1,
        id_list(&arranged.moved),
        id_list(&arranged.overlapping),
        id_list(&arranged.unplaced),
        on_off_text(arranged.main_shrunk),
        describe_sheet(&document.sheet_set(), index)
    ))
}

// ---- GENERATE ------------------------------------------------------------------------

// What GENERATE was asked, parsed; None is "not given".
#[derive(Default)]
struct GenerateOptions {
    paper: SheetTemplate,
    // Given: a denominator, or auto (the inner None).
    scale: Option<Option<f64>>,
    alignment: String,
    area: Option<Box2>,
    overlap: Option<f64>,
    interval: Option<f64>,
    half_width: Option<f64>,
    from: Option<f64>,
    to: Option<f64>,
    // 0: auto.
    exaggeration: Option<f64>,
    stations: Vec<f64>,
    rows: Option<usize>,
    columns: Option<usize>,
    key_plan: Option<bool>,
    model3d: Option<bool>,
    legend: Option<bool>,
    rotate: Option<bool>,
    // on: tiles with nothing drawn in them are kept.
    empty: Option<bool>,
    replace: bool,
}

// The options each kind of GENERATE takes, beyond replace=.
fn generate_keys(kind: &str) -> &'static str {
    match kind {
        "fit" => {
            "paper orientation frame area alignment scale model3d legend interval halfwidth \
             rotate empty"
        }
        "grid" => "paper orientation frame area scale overlap keyplan empty",
        "strips" => "paper orientation frame alignment scale overlap from to keyplan",
        "profile" => "paper orientation frame alignment scale interval halfwidth model3d legend",
        "sections" => {
            "paper orientation frame alignment scale interval stations halfwidth rows columns ve"
        }
        "frames" => "frame",
        "register" => "paper orientation frame",
        _ => "",
    }
}

fn takes_key(keys: &str, key: &str) -> bool {
    !keys.is_empty() && keys.split(' ').any(|word| word == key)
}

fn generate_options(kind: &str, args: &[String]) -> Result<GenerateOptions> {
    let keys = generate_keys(kind);
    let mut o = GenerateOptions::default();
    for word in args.iter().skip(1) {
        if let Some(landscape) = orientation_word(word)
            && takes_key(keys, "orientation")
        {
            o.paper.landscape = landscape;
            continue;
        }
        let Some(option) = option_from(word) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "expected option=value; GENERATE {} takes {keys} replace",
                    uppered(kind)
                ),
                word.as_str(),
            ));
        };
        let key = option.key.as_str();
        let value = option.value.as_str();
        if key == "replace" {
            o.replace = on_off(value, key)?;
            continue;
        }
        if !takes_key(keys, key) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "GENERATE {} takes no option {key}=; it takes {keys} replace",
                    uppered(kind)
                ),
                option.word.as_str(),
            ));
        }
        match key {
            "paper" => o.paper.paper = paper_from(value)?,
            "orientation" => o.paper.landscape = landscape_from(value)?,
            "frame" => o.paper.frame = frame_from(value)?,
            "scale" => o.scale = Some(scale_from(value)?),
            "alignment" => o.alignment = String::from(value),
            "area" => o.area = Some(box_from(value, "area=")?),
            "overlap" => o.overlap = Some(not_negative(value, "overlap=")?),
            "interval" => o.interval = Some(positive(value, "interval=")?),
            "halfwidth" => o.half_width = Some(positive(value, "halfwidth=")?),
            "from" => o.from = Some(number(value, "from=")?),
            "to" => o.to = Some(number(value, "to=")?),
            "ve" => {
                o.exaggeration = Some(if folded(value) == "auto" {
                    0.0
                } else {
                    positive(value, "ve=")?
                });
            }
            "stations" => o.stations = number_list(value, "stations=")?,
            "rows" => o.rows = Some(counting(value, "rows=")?),
            "columns" => o.columns = Some(counting(value, "columns=")?),
            "keyplan" => o.key_plan = Some(on_off(value, key)?),
            "model3d" => o.model3d = Some(on_off(value, key)?),
            "legend" => o.legend = Some(on_off(value, key)?),
            "rotate" => o.rotate = Some(on_off(value, key)?),
            "empty" => o.empty = Some(on_off(value, key)?),
            _ => {}
        }
    }
    Ok(o)
}

fn solved_alignment(model: &Model, name: &str) -> Result<SolvedAlignment> {
    let Some(alignment) = model.alignments.find(name) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "the drawing has no alignment of that name",
            name,
        ));
    };
    solve_alignment(&alignment.horizontal)
}

// What FIT and GRID cover: area=, else the front end's drawing extent, else
// the drawing's entities.
fn generate_area(
    o: &GenerateOptions,
    model: &Model,
    drawing_extent: impl FnOnce() -> Option<Box2>,
) -> Result<Box2> {
    if let Some(area) = o.area {
        return Ok(area);
    }
    let mut extent = drawing_extent().unwrap_or_default();
    if extent.empty() {
        extent = drawn_extent(model, &LayerOverrides::default());
    }
    if extent.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the drawing is empty, so there is nothing to lay out; draw something or give \
             area=x0,y0,x1,y1",
        ));
    }
    Ok(extent)
}

fn corners_of(area: &Box2) -> Vec<Point2> {
    vec![
        area.min,
        Point2::new(area.max.x, area.min.y),
        area.max,
        Point2::new(area.min.x, area.max.y),
    ]
}

// Tiles of which not one has anything drawn in it are refused by the
// generator (NotFound); the refusal says how to have them all the same.
fn say_how_to_keep_empty_tiles(sheets: Result<Vec<Sheet>>) -> Result<Vec<Sheet>> {
    sheets.map_err(|error| {
        if error.code == ErrorCode::NotFound {
            make_error(
                ErrorCode::NotFound,
                format!("{}; empty=on keeps every tile", error.message),
                error.context,
            )
        } else {
            error
        }
    })
}

fn generate_sheets(
    kind: &str,
    o: &GenerateOptions,
    document: &Document,
    asked: SheetVerbContextProvider<'_, '_>,
    skipped: &mut Vec<String>,
) -> Result<Vec<Sheet>> {
    let model = document.model();
    let automatic = 0.0;
    // The front end's context, built once however many choices ask for it:
    // the window's gathers everything its plan view draws.
    let built = OnceCell::new();
    let context = || built.get_or_init(|| context_of(asked, document));
    // What a tile of GRID, or of FIT at a fixed scale, must have drawn in it
    // to be kept: what the front end's plan view draws there (with its
    // imagery), else what a plan of the drawing draws (draws_in, through the
    // document's index). Nothing (every tile kept) for empty=on.
    let given_test = |area: &Box2| {
        context()
            .draws_in
            .as_ref()
            .is_some_and(|draws| draws(document, area))
    };
    let own_test = |area: &Box2| {
        draws_in(
            model,
            &LayerOverrides::default(),
            area,
            Some(document.spatial_index()),
        )
    };
    let given_test: &dyn Fn(&Box2) -> bool = &given_test;
    let own_test: &dyn Fn(&Box2) -> bool = &own_test;
    let tile_test = || {
        if o.empty.unwrap_or(false) {
            None
        } else if context().draws_in.is_some() {
            Some(given_test)
        } else {
            Some(own_test)
        }
    };
    if kind == "fit" {
        let mut request = LayoutRequest {
            paper: o.paper.clone(),
            scale: o.scale.flatten().unwrap_or(automatic),
            model3d: o.model3d.unwrap_or(false),
            legend: o.legend.unwrap_or(false),
            ..LayoutRequest::default()
        };
        if o.alignment.is_empty() {
            request.plan_area = Some(generate_area(o, model, || context().drawing_extent)?);
            request.drawn = tile_test();
        } else {
            if o.area.is_some() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "GENERATE FIT lays out area= or alignment=, not both",
                ));
            }
            if o.empty.is_some() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "empty= is for the tiles of an area; a plan along alignment= is never tiled",
                ));
            }
            request.alignment = alignment_named(model, &o.alignment)?;
            request.plan_along_alignment = true;
        }
        if let Some(interval) = o.interval {
            request.alignment = alignment_for(model, &o.alignment)?;
            request.cross_section_interval = interval;
            request.cross_section_half_width =
                o.half_width.unwrap_or(request.cross_section_half_width);
        }
        if o.rotate.unwrap_or(false) {
            // smart_layout_rotated refuses a plan along an alignment and
            // sections after the plan itself, saying why. What it turns: the
            // corners of area=, which a turn of a quarter can still fit better
            // on the paper's other way, else what a plan of the drawing shows
            // (the front end's, with its imagery, else the drawing's outline),
            // as the editor's Rotate to Best Fit measures a plan.
            let content = match &o.area {
                Some(area) => corners_of(area),
                None => {
                    let plan = Viewport {
                        kind: ViewportKind::Plan,
                        ..Viewport::default()
                    };
                    content_of(document, context(), &plan)
                }
            };
            return say_how_to_keep_empty_tiles(smart_layout_rotated(
                model,
                &request,
                &content,
                Some(skipped),
            ));
        }
        return say_how_to_keep_empty_tiles(smart_layout(model, &request, Some(skipped)));
    }
    if kind == "grid" {
        if matches!(o.scale, Some(None)) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "GENERATE GRID needs a scale: tiles are cut at a fixed one (500 when none is \
                 given)",
            ));
        }
        let area = generate_area(o, model, || context().drawing_extent)?;
        let mut request = GridRequest {
            area,
            paper: o.paper.clone(),
            ..GridRequest::default()
        };
        if let Some(Some(scale)) = o.scale {
            request.scale = scale;
        }
        request.overlap_m = o.overlap.unwrap_or(request.overlap_m);
        request.key_plan = o.key_plan.unwrap_or(request.key_plan);
        request.drawn = tile_test();
        return say_how_to_keep_empty_tiles(grid_sheets(&request, Some(skipped)));
    }
    if kind == "strips" || kind == "profile" {
        let name = alignment_for(model, &o.alignment)?;
        let fixed = o.scale.flatten();
        if kind == "strips"
            && let Some(scale) = fixed
        {
            let solved = solved_alignment(model, &name)?;
            let defaults = StripRequest::default();
            let request = StripRequest {
                paper: o.paper.clone(),
                scale,
                overlap_m: o.overlap.unwrap_or(defaults.overlap_m),
                from_chainage: o.from,
                to_chainage: o.to,
                key_plan: o.key_plan.unwrap_or(defaults.key_plan),
                ..defaults
            };
            return strip_sheets(&solved, &name, &request);
        }
        if kind == "strips"
            && (o.overlap.is_some() || o.from.is_some() || o.to.is_some() || o.key_plan.is_some())
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "overlap=, from=, to= and keyplan= need a fixed scale: at scale=auto the whole \
                 alignment is fitted on one sheet",
            ));
        }
        let defaults = LayoutRequest::default();
        let request = LayoutRequest {
            paper: o.paper.clone(),
            alignment: name,
            plan_along_alignment: true,
            long_section: kind == "profile",
            scale: fixed.unwrap_or(automatic),
            model3d: o.model3d.unwrap_or(false),
            legend: o.legend.unwrap_or(false),
            cross_section_interval: o.interval.unwrap_or(0.0),
            cross_section_half_width: o.half_width.unwrap_or(defaults.cross_section_half_width),
            ..defaults
        };
        return smart_layout(model, &request, None);
    }
    if kind == "sections" {
        if o.interval.is_some() && !o.stations.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "GENERATE SECTIONS cuts every interval= or at stations=, not both",
            ));
        }
        let name = alignment_for(model, &o.alignment)?;
        let solved = solved_alignment(model, &name)?;
        let defaults = CrossSectionRequest::default();
        let mut request = CrossSectionRequest {
            paper: o.paper.clone(),
            interval: o.interval.unwrap_or(defaults.interval),
            stations: o.stations.clone(),
            half_width: o.half_width.unwrap_or(defaults.half_width),
            rows: o.rows.unwrap_or(defaults.rows),
            columns: o.columns.unwrap_or(defaults.columns),
            scale: o.scale.flatten().unwrap_or(automatic),
            exaggeration: o.exaggeration.unwrap_or(automatic),
            ..defaults
        };
        // The drawing's shown surfaces: a section sheet is a picture, and
        // hiding a surface takes it out of every picture. None: each section
        // is centred when it is drawn, at an exaggeration of 1.
        for named in document.surfaces().all() {
            if named.visible
                && let Some(surface) = &named.surface
            {
                request.surfaces.push(SectionSurfaceInput {
                    name: &named.name,
                    surface: Some(&**surface),
                });
            }
        }
        return cross_section_sheets(&solved, &name, &request);
    }
    // frames
    let sheets = sheets_from_plot_frames(model, &o.paper, Some(&mut *skipped))?;
    if sheets.is_empty() {
        let mut why = String::from("the drawing has no plot frame that can be read");
        for reason in skipped.iter() {
            why.push_str("\n  ");
            why.push_str(reason);
        }
        return Err(Error::new(ErrorCode::NotFound, why));
    }
    Ok(sheets)
}

fn generate_verb(
    document: &mut Document,
    args: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    let Some(first) = args.first() else {
        return Err(usage(
            "GENERATE fit|grid|strips|profile|sections|frames [option=value ...]   (HELP \
             SHEETS lists the options)",
        ));
    };
    let mut kind = folded(first);
    match kind.as_str() {
        "tiles" => kind = String::from("grid"),
        "strip" => kind = String::from("strips"),
        "section" | "crosssections" => kind = String::from("sections"),
        "planprofile" => kind = String::from("profile"),
        "plotframes" => kind = String::from("frames"),
        "drawingregister" | "sheetindex" | "index" => kind = String::from("register"),
        _ => {}
    }
    if generate_keys(&kind).is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "GENERATE makes fit, grid, strips, profile, sections, frames or register",
            first.as_str(),
        ));
    }
    let options = generate_options(&kind, args)?;
    if kind == "register" {
        // A cover sheet, first in the set: the drawing register and the
        // revision table (tables.rs). It adds to the set; replacing every
        // sheet with a register of none would list nothing.
        if options.replace {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "GENERATE register puts a cover first; it replaces no sheets",
                "replace=on",
            ));
        }
        let id = add_register_sheet(document, &options.paper)?;
        let now = document.sheet_set();
        let at = sheet_index(&now, &id).unwrap_or(0);
        return Ok(format!(
            "generated 1 sheet: {} (the drawing register)\n{}",
            at + 1,
            describe_sheet(&now, at)
        ));
    }
    let mut skipped: Vec<String> = Vec::new();
    let mut sheets = generate_sheets(&kind, &options, document, context, &mut skipped)?;
    if sheets.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "nothing to lay out",
            first.as_str(),
        ));
    }
    let count = sheets.len();
    let first_new = if options.replace {
        0
    } else {
        document.sheet_set().sheets.len()
    };
    if options.replace {
        edit_sheet_set(
            document,
            |set| {
                set.sheets.clear();
                prepare_for_append(set, &mut sheets);
                set.sheets = sheets;
                Ok(())
            },
            "GENERATE_SHEETS",
        )?;
    } else {
        add_sheets(document, sheets, "GENERATE_SHEETS")?;
    }
    let set = document.sheet_set();
    let mut reply = if count == 1 {
        format!("generated 1 sheet: {}", first_new + 1)
    } else {
        format!(
            "generated {count} sheets: {} to {}",
            first_new + 1,
            first_new + count
        )
    };
    if options.replace {
        reply.push_str(" (the set was replaced)");
    }
    for i in first_new..(first_new + count).min(set.sheets.len()) {
        reply.push('\n');
        reply.push_str(&describe_sheet(&set, i));
    }
    for reason in &skipped {
        reply.push_str("\nskipped ");
        reply.push_str(reason);
    }
    Ok(reply)
}

// ---- TITLEBLOCK ----------------------------------------------------------------------

fn title_block_list(document: &Document) -> Result<String> {
    let set = readable(document)?;
    let mut reply = String::new();
    for field in title_block_fields() {
        if !reply.is_empty() {
            reply.push('\n');
        }
        reply.push_str(&format!(
            "{field}={}",
            reply_quoted(&title_block_value(&set, field)?)
        ));
    }
    reply.push_str(&format!(
        "\nlogo={}",
        reply_quoted(&set.defaults.logo_asset)
    ));
    reply.push('\n');
    reply.push_str(&count_of(set.revisions.len(), "revision", "revisions"));
    for revision in &set.revisions {
        reply.push('\n');
        reply.push_str(&revision_line(revision));
    }
    Ok(reply)
}

fn revision_verb(document: &mut Document, args: &[String]) -> Result<String> {
    const USAGE: &str = "TITLEBLOCK REVISION [LIST] | REVISION ADD code date description [by] | \
                         REVISION REMOVE code";
    let action = if args.len() < 2 {
        String::from("LIST")
    } else {
        uppered(&args[1])
    };
    match action.as_str() {
        "LIST" => {
            if args.len() > 2 {
                return Err(usage(USAGE));
            }
            let set = readable(document)?;
            let mut reply = count_of(set.revisions.len(), "revision", "revisions");
            for revision in &set.revisions {
                reply.push('\n');
                reply.push_str(&revision_line(revision));
            }
            Ok(reply)
        }
        "ADD" => {
            if args.len() < 5 || args.len() > 6 {
                return Err(usage(
                    "TITLEBLOCK REVISION ADD code date \"description\" [by]",
                ));
            }
            let revision = Revision {
                code: args[2].clone(),
                date: args[3].clone(),
                description: unescaped(&args[4]),
                by: args.get(5).cloned().unwrap_or_default(),
            };
            if revision.code.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a revision needs a code",
                ));
            }
            edit_sheet_set(
                document,
                |set| {
                    if set
                        .revisions
                        .iter()
                        .any(|existing| existing.code == revision.code)
                    {
                        return Err(make_error(
                            ErrorCode::AlreadyExists,
                            "there is a revision with that code already; REVISION REMOVE it first",
                            revision.code.as_str(),
                        ));
                    }
                    set.revisions.push(revision.clone());
                    Ok(())
                },
                "ADD_REVISION",
            )?;
            Ok(format!("added {}", revision_line(&revision)))
        }
        "REMOVE" | "DELETE" => {
            if args.len() != 3 {
                return Err(usage("TITLEBLOCK REVISION REMOVE code"));
            }
            let code = &args[2];
            edit_sheet_set(
                document,
                |set| {
                    let before = set.revisions.len();
                    set.revisions.retain(|revision| revision.code != *code);
                    if set.revisions.len() == before {
                        return Err(make_error(
                            ErrorCode::NotFound,
                            "no revision with that code",
                            code.as_str(),
                        ));
                    }
                    Ok(())
                },
                "REMOVE_REVISION",
            )?;
            Ok(format!("removed revision code={}", reply_quoted(code)))
        }
        _ => Err(usage(USAGE)),
    }
}

fn title_block_verb(document: &mut Document, args: &[String]) -> Result<String> {
    if args.is_empty() || (args.len() == 1 && uppered(&args[0]) == "LIST") {
        return title_block_list(document);
    }
    let action = uppered(&args[0]);
    if action == "REVISION" || action == "REVISIONS" {
        return revision_verb(document, args);
    }
    if action == "LOGO" {
        if args.len() != 2 {
            return Err(usage("TITLEBLOCK LOGO path   (\"\" removes the logo)"));
        }
        if args[1].is_empty() {
            edit_sheet_set(
                document,
                |set| {
                    set.defaults.logo_asset.clear();
                    Ok(())
                },
                "SET_LOGO",
            )?;
            return Ok(String::from("logo=\"\""));
        }
        let name = import_logo(document, &path_from_utf8(&args[1]))?;
        return Ok(format!("logo={}", reply_quoted(&name)));
    }
    let Some(field) = title_field_from(&args[0]) else {
        return Err(unknown_title_field(&args[0]));
    };
    if args.len() == 1 {
        return Ok(format!(
            "{field}={}",
            reply_quoted(&title_block_value(&document.sheet_set(), field)?)
        ));
    }
    let value = unescaped(&joined(args, 1, " "));
    edit_sheet_set(
        document,
        |set| set_title_block_value(set, field, value),
        "EDIT_TITLE_BLOCK",
    )?;
    Ok(format!(
        "{field}={}",
        reply_quoted(&title_block_value(&document.sheet_set(), field)?)
    ))
}

// ---- the public pieces ---------------------------------------------------------------

/// True for SHEETS, SHEET, VIEW (and VIEWPORT), TILE, ARRANGE, GENERATE and
/// TITLEBLOCK, in any case.
pub fn is_sheet_verb(verb: &str) -> bool {
    matches!(
        uppered(verb).as_str(),
        "SHEETS" | "SHEET" | "VIEW" | "VIEWPORT" | "TILE" | "ARRANGE" | "GENERATE" | "TITLEBLOCK"
    )
}

/// Runs one sheet verb: `tokens` is the whole line split into words with the
/// quotes removed (as CommandInterpreter::tokenize splits it), the verb first.
/// Returns the reply, or an error saying specifically what was refused and
/// why. The C++'s default context is None.
pub fn run_sheet_verb(
    document: &mut Document,
    tokens: &[String],
    context: SheetVerbContextProvider<'_, '_>,
) -> Result<String> {
    let Some(first) = tokens.first().filter(|verb| is_sheet_verb(verb)) else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "not a sheet command; type HELP SHEETS",
            tokens.first().cloned().unwrap_or_default(),
        ));
    };
    let verb = uppered(first);
    let args = &tokens[1..];
    // Stored sheets that cannot be read are not an empty set: an edit made on
    // the empty set the document falls back to would replace them all as one
    // step. So every verb is refused while they cannot be read, but SHEETS
    // LOAD, which replaces them on purpose (and can be undone).
    let loads = verb == "SHEETS" && args.first().is_some_and(|action| uppered(action) == "LOAD");
    if !loads {
        readable(document)?;
    }
    match verb.as_str() {
        "SHEETS" => sheets_verb(document, args, context),
        "SHEET" => sheet_verb(document, args, context),
        "VIEW" | "VIEWPORT" => view_verb(document, args, context),
        "TILE" => tile_verb(document, args),
        "ARRANGE" => arrange_verb(document, args, context),
        "GENERATE" => generate_verb(document, args, context),
        _ => title_block_verb(document, args),
    }
}

/// Every sheet verb and option, for HELP SHEETS.
pub fn sheet_verb_help() -> String {
    String::from(
        r#"Sheets on the command line (docs/plotting.md). A sheet is its number (1, 2 ...) or its
id (s1 ...); a view is its id (vp1 ...). Options are key=value, a switch on|off, and "\n" in a
text is a line break. Every edit is one UNDO step. A reply is one fact per line, in the form the
options take.

SHEETS [LIST]                   every sheet and its views
SHEETS JSON [path]              the set as JSON, printed or written to a file
SHEETS SAVE path | LOAD path    write the set to a JSON file | replace the set from one
SHEETS APPEND path              another set's sheets from its JSON file, after these
SHEETS CHECK [sheets=1,3-5] [json]   the preflight checks: a summary, then a finding a line,
                                "severity code sheet view message=... fix=..." (- for none)
SHEETS PAGESETUP [style=colour|grey|mono] [lineweight=f] [dpi=n] [pattern=text]
                 [filepersheet=on|off]   how the set plots; no option prints it
SHEET NEW [name] [paper=A3] [portrait|landscape] [frame=on|off] [legendblock=on|off] [at=n]
SHEET REMOVE n | MOVE n to | COPY n | RENAME n name
SHEET SET n [paper=A0..A4] [orientation=landscape|portrait] [frame=on|off] [legendblock=on|off]
          [name=text]
SHEET FIELD n field [value]     the sheet's own value for a field its frame prints, such as
                                sheet_number or scale; "" clears it, no value prints it
SHEET SUGGESTPAPER n [scale=n|auto] [apply=on]   the smallest paper for the sheet's main plan
                                at a scale; apply=on puts the sheet on it
VIEW ADD n kind [option=value ...]   kinds: plan key_plan long_section cross_sections model_3d
                                     legend notes image sheet_index (register) revisions
VIEW SET id option=value ... | VIEW REMOVE id | VIEW LIST [n]
VIEW FIT id                     a plan's rectangle fitted to what it shows
VIEW BESTROTATE id              a plan turned to show most, and scaled to fit
  options  rect=x0,y0,x1,y1 (paper mm)  scale=500|1:500|auto (plans and sections)
           centre=x,y|auto  rotation=deg  alignment=name  from=ch  to=ch  stations=a,b,c
           interval=m  halfwidth=m  ve=n  tilt=deg  north=on|off  scalebar=on|off
           grid=none|ticks|crosses|lines  gridinterval=m|auto (plans and key plans)
           legend=this_sheet|whole_set|whole_drawing (legends)  revisions=n|all (revision
           tables)  locked=on|off  title=text  text=text  hide=layer  show=layer
           hidden=layer,layer  file=path (an image view's picture, copied into the project)
TILE n preset                   full cols2 rows2 quad sectionR sectionB sectionBR sectionMap3d,
                                or the menu's name (Main and panel right)
ARRANGE n                       no two views overlapping, the main view first and largest
ARRANGE ALIGN left|right|top|bottom|hcentre|vcentre id id ...
ARRANGE DISTRIBUTE across|up id id id ...   equal gaps, the first and last staying
ARRANGE MATCHSCALE from id id ...   the views take the scale view `from` is drawn at
GENERATE kind [option=value ...] [replace=on]   adds the sheets, or replaces every sheet
  fit       the drawing, area=x0,y0,x1,y1 or alignment=name on as few sheets as it takes:
            scale=auto|n model3d=on legend=on interval=m halfwidth=m; rotate=on turns the plan
            of the drawing or area= to fill the sheet; tiles at a fixed scale as grid's, empty=
  grid      tiles over the drawing or area=: scale=500 overlap=m keyplan=on|off; a tile with
            nothing drawn in it is left out, and empty=on keeps it
  strips    plans along alignment=: scale=auto|n; at a fixed scale overlap=m from=ch to=ch
            keyplan=on|off
  profile   plan and profile along alignment=: scale=auto|n interval=m halfwidth=m
  sections  cross sections of alignment=: interval=20|stations=a,b,c halfwidth=20 rows=4
            columns=2 scale=auto|n ve=auto|n
  frames    a sheet per imported plot frame: frame=on|off
  register  a cover sheet put first: the drawing register and the revision table
  and, but for frames, paper=A0..A4 portrait|landscape frame=on|off; alignment= may be left
  out when the drawing has one alignment
TITLEBLOCK [LIST]               the shared title-block values, the logo and the revisions
TITLEBLOCK field [value]        organisation project1..project4 client setnumber numbering
                                coordsys datum model notes, and <role>name <role>date for the
                                roles locator surveyor compiler reviewer approver
TITLEBLOCK REVISION [LIST]      the revisions
TITLEBLOCK REVISION ADD code date "description" [by] | TITLEBLOCK REVISION REMOVE code
TITLEBLOCK LOGO path            the logo, copied into the project ("" removes it)
PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono] [sheets=1,3-5] [dpi=n]
           [lineweight=f] [folder=path] [pattern=text]   plots in the desktop application, the
                                page setup filling in what is not given; a file a line after.
                                Headless: katana project --plot-sheets out.pdf"#,
    )
}

/// A finding as SHEETS CHECK prints it, one line: its severity, its code, the
/// sheet's number (- for the set), the view's id (- for none), then
/// message="..." and, when there is one, fix="..." and subject="..." - the
/// words quoted as every reply quotes them, so a line splits on spaces:
///
/// ```text
/// warning plan.empty 2 vp3 message="PLAN 1:500 (vp3) shows nothing ..." fix="..."
/// ```
///
/// (preflight.rs's finding_line is the sentence a log shows.)
pub fn check_reply_line(finding: &Finding) -> String {
    let mut line = format!(
        "{} {} {} {} message={}",
        finding.severity.name(),
        finding.code,
        finding
            .sheet_index
            .map_or_else(|| String::from("-"), |index| format!("{}", index + 1)),
        if finding.viewport_id.is_empty() {
            "-"
        } else {
            finding.viewport_id.as_str()
        },
        reply_quoted(&finding.message)
    );
    if !finding.fix.is_empty() {
        line.push_str(" fix=");
        line.push_str(&reply_quoted(&finding.fix));
    }
    if !finding.subject.is_empty() {
        line.push_str(" subject=");
        line.push_str(&reply_quoted(&finding.subject));
    }
    line
}

/// The sheet `text` names in `set`: its number from 1, or its id. NotFound
/// for a number or id the set does not have; ParseFailure for anything else.
pub fn sheet_index_from(set: &SheetSet, text: &str) -> Result<usize> {
    let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if digits {
        return match digits_prefix(text.as_bytes()).0 {
            Some(number) if number != 0 && number <= set.sheets.len() => Ok(number - 1),
            _ => Err(make_error(
                ErrorCode::NotFound,
                format!(
                    "no sheet {text}: the set has {}",
                    count_of(set.sheets.len(), "sheet", "sheets")
                ),
                text,
            )),
        };
    }
    if text.is_empty() {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            "a sheet is its number (1, 2 ...) or its id (s1, s2 ...)",
        ));
    }
    if let Some(index) = set.sheets.iter().position(|sheet| sheet.id == text) {
        return Ok(index);
    }
    let wanted = uppered(text);
    if let Some(index) = set
        .sheets
        .iter()
        .position(|sheet| uppered(&sheet.id) == wanted)
    {
        return Ok(index);
    }
    if text.starts_with(['s', 'S']) {
        return Err(make_error(
            ErrorCode::NotFound,
            "no sheet with that id; SHEETS LIST shows them",
            text,
        ));
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "a sheet is its number (1, 2 ...) or its id (s1, s2 ...)",
        text,
    ))
}

/// The words after the verb of
///
/// ```text
/// PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono]
///            [sheets=1,3-5] [dpi=n] [lineweight=f] [folder=path] [pattern=text]
/// ```
///
/// What is not given comes from the set's page setup,
/// as the Plot dialog and --plot-sheets take it; the format is one PDF unless
/// format= says otherwise, or unless only folder= is given, which is a PDF a
/// sheet. The path is the PDF of the one-PDF format, or the folder of a file
/// a sheet (folder= says the same). The sheets are the selection
/// parse_sheet_selection (page_setup.rs) reads, as every plot takes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlotSheetsRequest {
    pub path: PathBuf,
    pub sheets: String,
    /// "pdf", "pdfs", "png" or "tiff"; None: "pdf", or "pdfs" for folder=.
    pub format: Option<String>,
    pub colour_mode: Option<PlotColourMode>,
    pub dpi: Option<f64>,
    pub line_weight_scale: Option<f64>,
    pub file_name_pattern: Option<String>,
}

pub fn parse_plot_sheets(set: &SheetSet, args: &[String]) -> Result<PlotSheetsRequest> {
    const USAGE: &str = "PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono] \
                         [sheets=1,3-5] [dpi=n] [lineweight=f] [folder=path] [pattern=text]";
    let mut request = PlotSheetsRequest::default();
    let mut folder: Option<PathBuf> = None;
    for (i, word) in args.iter().enumerate() {
        let Some(option) = option_from(word) else {
            if i != 0 || word.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("expected option=value; usage: {USAGE}"),
                    word.as_str(),
                ));
            }
            request.path = path_from_utf8(word);
            continue;
        };
        match option.key.as_str() {
            "sheets" | "sheet" => {
                // Read as every plot reads it, so a refusal says what it says
                // there.
                parse_sheet_selection(&option.value, set)?;
                request.sheets = option.value.clone();
            }
            "format" => {
                let format = folded(&option.value);
                if !matches!(format.as_str(), "pdf" | "pdfs" | "png" | "tiff" | "tif") {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "format= is pdf, pdfs, png or tiff",
                        option.word,
                    ));
                }
                request.format = Some(if format == "tif" {
                    String::from("tiff")
                } else {
                    format
                });
            }
            "style" | "colour" | "color" | "mode" => {
                request.colour_mode = Some(style_from(&option.value)?);
            }
            "dpi" => {
                let dpi = number(&option.value, "dpi=")?;
                // number() gives a finite value, so this is the C++'s `dpi < min || dpi > max`.
                if !(MINIMUM_PLOT_DPI..=MAXIMUM_PLOT_DPI).contains(&dpi) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        format!(
                            "dpi= is {} to {}",
                            format_exact_real(MINIMUM_PLOT_DPI),
                            format_exact_real(MAXIMUM_PLOT_DPI)
                        ),
                        option.word,
                    ));
                }
                request.dpi = Some(dpi);
            }
            "lineweight" | "lineweightscale" => {
                let factor = number(&option.value, "lineweight=")?;
                if !(MINIMUM_LINE_WEIGHT_SCALE..=MAXIMUM_LINE_WEIGHT_SCALE).contains(&factor) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        format!(
                            "lineweight= is {} to {}",
                            format_exact_real(MINIMUM_LINE_WEIGHT_SCALE),
                            format_exact_real(MAXIMUM_LINE_WEIGHT_SCALE)
                        ),
                        option.word,
                    ));
                }
                request.line_weight_scale = Some(factor);
            }
            "folder" => {
                if option.value.is_empty() {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "folder= names a folder",
                        option.word,
                    ));
                }
                folder = Some(path_from_utf8(&option.value));
            }
            "pattern" | "names" => {
                let pattern = unescaped(&option.value);
                validate_file_name_pattern(&pattern)?;
                request.file_name_pattern = Some(pattern);
            }
            _ => {
                return Err(unknown_option(
                    &option,
                    "PLOTSHEETS",
                    "format=, style=, sheets=, dpi=, lineweight=, folder= and pattern=",
                ));
            }
        }
    }
    if let Some(folder) = folder {
        if !request.path.as_os_str().is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "give the PDF's path or folder=, not both: folder= is where a file a sheet goes",
                path_text(&request.path),
            ));
        }
        if request.format.as_deref() == Some("pdf") {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "format=pdf is one file: give its path rather than folder=",
            ));
        }
        request.path = folder;
        if request.format.is_none() {
            request.format = Some(String::from("pdfs"));
        }
    }
    if request.path.as_os_str().is_empty() {
        return Err(usage(USAGE));
    }
    Ok(request)
}

/// One viewport option, `key` = `value`, as VIEW ADD and VIEW SET take it
/// (sheet_verb_help lists them). `model` checks an alignment= name. The
/// viewport is left as it was when the option is refused.
pub fn set_viewport_option(
    viewport: &mut Viewport,
    key: &str,
    value: &str,
    model: &Model,
) -> Status {
    let option = KeyValue {
        key: folded(key),
        value: String::from(value),
        word: format!("{key}={value}"),
    };
    let name = option.key.as_str();
    // Changed on a copy, so a refusal leaves the viewport as it was.
    let mut edited = viewport.clone();
    let scaled_kinds = "plan, key_plan, long_section and cross_sections";
    match name {
        "rect" => edited.rect = box_from(value, "rect=")?,
        "scale" => {
            only_for(
                viewport,
                &option,
                drawn_to_scale(viewport.kind),
                scaled_kinds,
            )?;
            let scale = scale_from(value)?;
            edited.auto_scale = scale.is_none();
            if let Some(scale) = scale {
                edited.scale = scale;
            }
        }
        "centre" | "center" => {
            only_for(
                viewport,
                &option,
                drawn_to_scale(viewport.kind),
                scaled_kinds,
            )?;
            if folded(value) == "auto" {
                edited.auto_centre = true;
            } else {
                edited.centre = point_from(value, "centre=")?;
                edited.auto_centre = false;
            }
        }
        "rotation" => {
            only_for(
                viewport,
                &option,
                is_plan_like(viewport.kind) || viewport.kind == ViewportKind::Model3D,
                "plan, key_plan and model_3d",
            )?;
            edited.rotation = number(value, "rotation=")? * DEG_TO_RAD;
        }
        "alignment" => {
            only_for(
                viewport,
                &option,
                is_plan_like(viewport.kind) || is_section(viewport.kind),
                scaled_kinds,
            )?;
            if value.is_empty() {
                edited.source.alignment.clear();
            } else {
                edited.source.alignment = alignment_named(model, value)?;
            }
        }
        "from" | "to" => {
            only_for(
                viewport,
                &option,
                is_plan_like(viewport.kind) || is_section(viewport.kind),
                scaled_kinds,
            )?;
            let chainage = number(value, &format!("{name}="))?;
            if name == "from" {
                edited.source.chainage_from = chainage;
            } else {
                edited.source.chainage_to = chainage;
            }
        }
        "stations" | "interval" | "halfwidth" => {
            only_for(
                viewport,
                &option,
                viewport.kind == ViewportKind::CrossSections,
                "cross_sections",
            )?;
            match name {
                "stations" => edited.source.stations = number_list(value, "stations=")?,
                "interval" => edited.source.section_interval = not_negative(value, "interval=")?,
                _ => edited.source.section_half_width = positive(value, "halfwidth=")?,
            }
        }
        "ve" | "exaggeration" => {
            only_for(
                viewport,
                &option,
                is_section(viewport.kind),
                "long_section and cross_sections",
            )?;
            edited.vertical_exaggeration = positive(value, "ve=")?;
        }
        "tilt" => {
            only_for(
                viewport,
                &option,
                viewport.kind == ViewportKind::Model3D,
                "model_3d",
            )?;
            let tilt = number(value, "tilt=")?;
            // number() gives a finite value, so this is the C++'s `tilt < 1 || tilt > 89`.
            if !(1.0..=89.0).contains(&tilt) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "tilt= is the camera's height angle, 1 to 89 degrees",
                    option.word,
                ));
            }
            edited.tilt_degrees = tilt;
        }
        "north" | "northarrow" | "scalebar" => {
            only_for(
                viewport,
                &option,
                is_plan_like(viewport.kind),
                "plan and key_plan",
            )?;
            let on = on_off(value, name)?;
            if name == "scalebar" {
                edited.scale_bar = on;
            } else {
                edited.north_arrow = on;
            }
        }
        "locked" | "lock" => edited.locked = on_off(value, name)?,
        "title" => edited.title = unescaped(value),
        "text" => {
            only_for(
                viewport,
                &option,
                matches!(viewport.kind, ViewportKind::Notes | ViewportKind::Image),
                "notes and image",
            )?;
            edited.text = unescaped(value);
        }
        "hide" | "show" => {
            if value.is_empty() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{name}= names a layer"),
                    option.word,
                ));
            }
            if name == "hide" {
                edited.hidden_layers.hide(value);
            } else {
                edited.hidden_layers.show(value);
            }
        }
        "hidden" => {
            edited.hidden_layers.clear();
            for layer in value.split(',') {
                if !layer.is_empty() {
                    edited.hidden_layers.hide(layer);
                }
            }
        }
        "legend" | "scope" => {
            only_for(
                viewport,
                &option,
                viewport.kind == ViewportKind::Legend,
                "legend",
            )?;
            // The stored names, and the words a person reaches for.
            let word = folded(value);
            let scope = match legend_scope_from(value) {
                Some(scope) => scope,
                None if word == "thissheet" || word == "sheet" => LegendScope::ThisSheet,
                None if word == "wholeset" || word == "set" => LegendScope::WholeSet,
                None if word == "wholedrawing" || word == "drawing" || word == "all" => {
                    LegendScope::WholeDrawing
                }
                None => {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "legend= is this_sheet, whole_set or whole_drawing",
                        option.word,
                    ));
                }
            };
            edited.legend_scope = scope;
        }
        "grid" => {
            only_for(
                viewport,
                &option,
                is_plan_like(viewport.kind),
                "plan and key_plan",
            )?;
            let style = match grid_style_from(&folded(value)) {
                Some(style) => style,
                None if folded(value) == "off" => GridStyle::None,
                None => {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "grid= is none, ticks, crosses or lines",
                        option.word,
                    ));
                }
            };
            edited.grid_style = style;
        }
        "gridinterval" => {
            only_for(
                viewport,
                &option,
                is_plan_like(viewport.kind),
                "plan and key_plan",
            )?;
            edited.grid_interval = if folded(value) == "auto" {
                0.0
            } else {
                positive(value, "gridinterval=")?
            };
        }
        "revisions" | "limit" => {
            only_for(
                viewport,
                &option,
                viewport.kind == ViewportKind::Revisions,
                "revisions",
            )?;
            edited.revision_limit = if folded(value) == "all" {
                0
            } else {
                counting(value, "revisions=")?
            };
        }
        "kind" => {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a view's kind is fixed: VIEW REMOVE it and VIEW ADD another",
                option.word,
            ));
        }
        _ => {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "no view option of that name; it is rect, scale, centre, rotation, alignment, \
                 from, to, stations, interval, halfwidth, ve, tilt, north, scalebar, grid, \
                 gridinterval, legend, revisions, locked, title, text, hide, show, hidden or file",
                option.word,
            ));
        }
    }
    *viewport = edited;
    Ok(())
}

/// A new viewport of `kind` for sheet `sheet_index`, set up as the sheet
/// editor's Add View sets one up: its size for the kind, centred in the
/// tiling area (or filling it on an empty sheet), a plan scaled and centred
/// automatically with a north arrow and scale bar, sections on the drawing's
/// first alignment, a key plan fitted to the sheets' plans, which it outlines
/// live (key_plan.rs), as the editor's Add View > Key Plan does. Its id is
/// the set's next.
pub fn default_viewport(
    set: &SheetSet,
    sheet_index: usize,
    kind: ViewportKind,
    model: &Model,
) -> Viewport {
    let mut viewport = Viewport {
        id: next_viewport_id(set),
        kind,
        ..Viewport::default()
    };
    let sheet = set.sheets.get(sheet_index);
    let place = |width: f64, height: f64| {
        sheet
            .map(|sheet| free_place(sheet, width, height))
            .unwrap_or_default()
    };
    let alignments = model.alignments.names();
    match kind {
        ViewportKind::Plan => {
            viewport.rect = place(200.0, 140.0);
            viewport.auto_scale = true;
            viewport.auto_centre = true;
            viewport.north_arrow = true;
            viewport.scale_bar = true;
        }
        ViewportKind::KeyPlan => {
            viewport.rect = place(90.0, 70.0);
            viewport.auto_scale = true;
            viewport.auto_centre = true;
            viewport.north_arrow = true;
            // No outlines are stored: a key plan draws the sheets' plans where
            // they are when it is drawn (key_plan.rs), numbered as the set
            // numbers them then, and fits itself to them.
        }
        ViewportKind::LongSection => {
            viewport.rect = place(380.0, 110.0);
            viewport.scale = 500.0;
            viewport.vertical_exaggeration = 10.0;
            viewport.auto_centre = true;
            if let Some(first) = alignments.first() {
                viewport.source.alignment = first.clone();
            }
        }
        ViewportKind::CrossSections => {
            viewport.rect = place(180.0, 110.0);
            viewport.scale = 200.0;
            viewport.auto_centre = true;
            viewport.source.section_half_width = 20.0;
            if let Some(first) = alignments.first() {
                viewport.source.alignment = first.clone();
                if let Some(middle) = middle_station(model, first) {
                    viewport.source.stations = vec![middle];
                }
            }
        }
        ViewportKind::Model3D => viewport.rect = place(160.0, 110.0),
        ViewportKind::Legend => viewport.rect = place(80.0, 100.0),
        ViewportKind::Notes => {
            viewport.rect = place(90.0, 80.0);
            viewport.text = String::from("1. ALL DIMENSIONS ARE IN METRES UNLESS NOTED OTHERWISE.");
        }
        // Room for its five columns and a dozen rows at the table's largest
        // text; a longer set continues in a second block, then shrinks.
        ViewportKind::SheetIndex => viewport.rect = place(200.0, 120.0),
        ViewportKind::Revisions => viewport.rect = place(150.0, 50.0),
        ViewportKind::Image => viewport.rect = place(100.0, 80.0),
    }
    viewport
}

/// The fields title_block_value takes, in the order TITLEBLOCK LIST prints
/// them.
pub fn title_block_fields() -> Vec<&'static str> {
    TITLE_FIELDS.iter().map(|(name, _)| *name).collect()
}

/// The shared title-block value `field` names (organisation, project1..4,
/// client, setnumber, numbering, coordsys, datum, model, notes, and
/// `<role>name` / `<role>date` for locator, surveyor, compiler, reviewer and
/// approver; the frame's own field names are accepted too). NotFound for a
/// field that is not one of them.
pub fn title_block_value(set: &SheetSet, field: &str) -> Result<String> {
    let Some(name) = title_field_from(field) else {
        return Err(unknown_title_field(field));
    };
    let line = project_line(name);
    if line != 0 {
        return Ok(set
            .defaults
            .project_lines
            .get(line - 1)
            .cloned()
            .unwrap_or_default());
    }
    Ok(title_slot(set, name).cloned().unwrap_or_default())
}

/// Sets the value title_block_value reads. NotFound for a field that is not
/// one of them; InvalidArgument for a numbering pattern without {n}.
pub fn set_title_block_value(set: &mut SheetSet, field: &str, mut value: String) -> Status {
    let Some(name) = title_field_from(field) else {
        return Err(unknown_title_field(field));
    };
    let line = project_line(name);
    if line != 0 {
        let lines = &mut set.defaults.project_lines;
        if lines.len() < line {
            lines.resize(line, String::new());
        }
        lines[line - 1] = value;
        // Trailing empty lines are no lines: a cleared set stays the default.
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        return Ok(());
    }
    if name == "numbering" {
        if value.is_empty() {
            value = SheetSet::default().numbering;
        } else if !value.contains("{n") {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a numbering pattern needs {n} (or {n:02}), or every sheet gets the same number",
                value,
            ));
        }
    }
    if let Some(slot) = title_slot_mut(set, name) {
        *slot = value;
    }
    Ok(())
}

/// The set written to a JSON file (sheet_json.rs), with a line break after.
pub fn write_sheet_set_file(set: &SheetSet, path: &Path) -> Status {
    let mut json = sheet_set_to_json(set)?;
    json.push('\n');
    if std::fs::write(path, json).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file cannot be written",
            path_text(path),
        ));
    }
    Ok(())
}

/// The set read from a JSON file (sheet_json.rs). NotFound for a file that
/// cannot be opened; the reader's errors with the path before their context.
pub fn read_sheet_set_file(path: &Path) -> Result<SheetSet> {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "the file cannot be opened",
            path_text(path),
        ));
    };
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the file cannot be read",
            path_text(path),
        ));
    }
    let read = match String::from_utf8(bytes) {
        Ok(text) => sheet_set_from_json(&text),
        // The C++ hands the bytes to nlohmann, which refuses a byte that is
        // not UTF-8 with a parse error; read them so, to say what it says.
        Err(invalid) => Err(make_error(
            ErrorCode::ParseFailure,
            "the sheet set is not in the expected format",
            match Json::parse(invalid.as_bytes()) {
                Err(error) => error.what,
                Ok(_) => String::from("the file is not UTF-8 text"),
            },
        )),
    };
    read.map_err(|error| {
        let mut context = path_text(path);
        if !error.context.is_empty() {
            context.push_str(": ");
            context.push_str(&error.context);
        }
        make_error(error.code, error.message, context)
    })
}

/// The lines SHEETS LIST prints for sheet `index`; empty for an index past
/// the end.
pub fn describe_sheet(set: &SheetSet, index: usize) -> String {
    let Some(sheet) = set.sheets.get(index) else {
        return String::new();
    };
    let mut text = sheet_line(set, index);
    for (field, value) in &sheet.fields {
        text.push_str(&format!("\n  field {field}={}", reply_quoted(value)));
    }
    for viewport in &sheet.viewports {
        text.push_str("\n  ");
        text.push_str(&viewport_summary(viewport));
    }
    text
}

/// The line VIEW LIST prints for a view.
pub fn describe_viewport(viewport: &Viewport) -> String {
    let kind = viewport.kind;
    let mut line = format!(
        "view id={} kind={} rect={}",
        viewport.id,
        kind.name(),
        box_text(&viewport.rect)
    );
    if drawn_to_scale(kind) {
        line.push_str(" scale=");
        line.push_str(&if viewport.auto_scale {
            String::from("auto")
        } else {
            decimal(viewport.scale)
        });
        line.push_str(" centre=");
        line.push_str(&if viewport.auto_centre {
            String::from("auto")
        } else {
            point_text(&viewport.centre)
        });
    }
    if is_plan_like(kind) || kind == ViewportKind::Model3D {
        line.push_str(" rotation=");
        line.push_str(&decimal(viewport.rotation * RAD_TO_DEG));
    }
    let source = &viewport.source;
    if is_section(kind) || !source.alignment.is_empty() {
        line.push_str(" alignment=");
        line.push_str(&reply_quoted(&source.alignment));
    }
    if is_section(kind) || source.chainage_from != 0.0 || source.chainage_to != 0.0 {
        line.push_str(&format!(
            " from={} to={}",
            decimal(source.chainage_from),
            decimal(source.chainage_to)
        ));
    }
    if kind == ViewportKind::CrossSections {
        let stations: Vec<String> = source
            .stations
            .iter()
            .map(|&station| decimal(station))
            .collect();
        line.push_str(&format!(
            " interval={} stations={} halfwidth={}",
            decimal(source.section_interval),
            stations.join(","),
            decimal(source.section_half_width)
        ));
    }
    if is_section(kind) {
        line.push_str(" ve=");
        line.push_str(&decimal(viewport.vertical_exaggeration));
    }
    if kind == ViewportKind::Model3D {
        line.push_str(" tilt=");
        line.push_str(&decimal(viewport.tilt_degrees));
    }
    if is_plan_like(kind) {
        line.push_str(&format!(
            " north={} scalebar={} grid={} gridinterval={}",
            on_off_text(viewport.north_arrow),
            on_off_text(viewport.scale_bar),
            viewport.grid_style.name(),
            if viewport.grid_interval > 0.0 {
                decimal(viewport.grid_interval)
            } else {
                String::from("auto")
            }
        ));
    }
    if kind == ViewportKind::Legend {
        line.push_str(&format!(" legend={}", viewport.legend_scope.name()));
    }
    if kind == ViewportKind::Revisions {
        line.push_str(" revisions=");
        line.push_str(&if viewport.revision_limit == 0 {
            String::from("all")
        } else {
            format!("{}", viewport.revision_limit)
        });
    }
    line.push_str(&format!(" locked={}", on_off_text(viewport.locked)));
    if !viewport.hidden_layers.empty() {
        let hidden: Vec<&str> = viewport
            .hidden_layers
            .hidden()
            .iter()
            .map(String::as_str)
            .collect();
        line.push_str(" hidden=");
        line.push_str(&reply_quoted(&hidden.join(",")));
    }
    if !viewport.title.is_empty() {
        line.push_str(" title=");
        line.push_str(&reply_quoted(&viewport.title));
    }
    if !viewport.text.is_empty() {
        line.push_str(" text=");
        line.push_str(&reply_quoted(&viewport.text));
    }
    if !viewport.marks.is_empty() {
        line.push_str(&format!(" marks={}", viewport.marks.len()));
    }
    line
}
