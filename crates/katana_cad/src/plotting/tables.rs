//! Tables on a sheet: the drawing register and the revision table
//! (docs/plotting.md, "The drawing register and the revision table";
//! include/katana/cad/plotting/tables.hpp).
//!
//! A table is laid out here and only drawn by the painter: its rules, each of
//! its texts and where it goes in paper millimetres, the text size and row
//! height chosen to fit the viewport, the second block of columns the rows
//! continue in, and how many rows did not fit. So the layout is tested without
//! pixels, and what the editor shows, what plots and what an agent reads are
//! the same rows.
//!
//! ```text
//! drawing_register       a row per sheet: its number as the set numbers it now,
//!                        its title, the scale its title block reports, its paper
//!                        and its current revision
//! revision_rows          the set's revisions, newest first; the newest N only
//! layout_table           any table fitted into a rectangle
//! layout_viewport_table  the table a SheetIndex or Revisions viewport shows
//! register_sheet         a cover sheet holding the register and the revisions
//! add_register_sheet     that sheet put first in a document's set, one step
//! ```
//!
//! Text is measured through a TextWidth the caller supplies - the painter
//! passes its font's - or, when none is given, estimated from Arial's advance
//! widths: deterministic, and close enough to lay a table out by.

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::document::Document;
use crate::plotting::frame::HorizontalJustify;
use crate::plotting::generators::{SheetTemplate, blank_sheet, prepare_for_append};
use crate::plotting::layout::{TILING_GUTTER_MM, TilingPreset, tile_viewports};
use crate::plotting::sheet_set::{
    FieldContext, Revision, Sheet, SheetSet, Viewport, ViewportKind, automatic_title,
    format_sheet_number, resolve_fields,
};

/// The width in paper millimetres of one line of `text` (UTF-8) set in Arial
/// at a cap height of `cap_mm`, bold or not, not condensed. The layout assumes
/// a width grows in proportion to the cap height, as a scaled font's does.
pub type TextWidth<'a> = dyn Fn(&str, f64, bool) -> f64 + 'a;

// ---- measuring ------------------------------------------------------------------

// Advance widths of ASCII 32 to 126 in thousandths of an em: Helvetica's
// published metrics, which Arial was drawn to match character for character.
#[rustfmt::skip]
const REGULAR_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // space to /
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556,                               // 0 to 9
    278, 278, 584, 584, 584, 556, 1015,                                             // : to @
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833,                // A to M
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611,                // N to Z
    278, 278, 278, 469, 556, 333,                                                   // [ to `
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833,                // a to m
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500,                // n to z
    334, 260, 334, 584,                                                             // { to ~
];
#[rustfmt::skip]
const BOLD_WIDTHS: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556,
    333, 333, 584, 584, 584, 611, 975,
    722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833,
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611,
    333, 278, 333, 584, 556, 333,
    556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889,
    611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500,
    389, 280, 389, 584,
];
// Arial's cap height in ems (1467 of 2048 units), what a text's size is
// given by here.
const CAP_HEIGHT_EM: f64 = 1467.0 / 2048.0;
// A digit's.
const OTHER_WIDTH: u16 = 556;
// A full em.
const WIDE_WIDTH: u16 = 1000;

// Whether a code point is drawn a full em wide: the East Asian wide blocks.
fn is_wide(c: u32) -> bool {
    (0x1100..=0x115F).contains(&c)
        || (0x2E80..=0xA4CF).contains(&c)
        || (0xAC00..=0xD7A3).contains(&c)
        || (0xF900..=0xFAFF).contains(&c)
        || (0xFE30..=0xFE4F).contains(&c)
        || (0xFF00..=0xFF60).contains(&c)
        || (0xFFE0..=0xFFE6).contains(&c)
        || c >= 0x20000
}

/// A TextWidth from Arial's advance widths (the metrics Arial shares with
/// Helvetica), for when there is no font to ask: deterministic on every
/// machine. A character outside ASCII counts as a digit's width, and one from
/// the wide East Asian blocks as a full em.
pub fn estimate_text_width(text: &str, cap_mm: f64, bold: bool) -> f64 {
    let table = if bold { &BOLD_WIDTHS } else { &REGULAR_WIDTHS };
    let bytes = text.as_bytes();
    let mut units = 0.0;
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];
        if lead < 0x80 {
            if (32..=126).contains(&lead) {
                units += f64::from(table[usize::from(lead - 32)]);
            } else if lead == b'\t' {
                units += f64::from(table[0]);
            }
            i += 1;
            continue;
        }
        // A UTF-8 sequence: its length from the lead byte, its code point
        // from the bytes after it. A stray continuation byte counts as one.
        let (length, mut code) = if lead & 0xE0 == 0xC0 {
            (2, u32::from(lead & 0x1F))
        } else if lead & 0xF0 == 0xE0 {
            (3, u32::from(lead & 0x0F))
        } else if lead & 0xF8 == 0xF0 {
            (4, u32::from(lead & 0x07))
        } else {
            (1, 0)
        };
        let mut k = 1;
        while k < length && i + k < bytes.len() {
            code = (code << 6) | u32::from(bytes[i + k] & 0x3F);
            k += 1;
        }
        units += f64::from(if length > 1 && is_wide(code) {
            WIDE_WIDTH
        } else {
            OTHER_WIDTH
        });
        i += length;
    }
    units / 1000.0 * cap_mm / CAP_HEIGHT_EM
}

/// One line of the drawing register.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegisterRow {
    pub sheet_id: String,
    /// As the sheet's title block prints each: the number the set gives the
    /// sheet now (or the one typed on it), its name, the scale it reports
    /// ("1:500", "AS SHOWN", "N.T.S."), its paper, and its current revision -
    /// typed on the sheet, else the set's latest.
    pub number: String,
    pub title: String,
    pub scale: String,
    pub paper: String,
    pub revision: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableColumn {
    pub header: String,
    pub justify: HorizontalJustify,
    /// Takes the width the other columns leave (at most one column does).
    pub stretch: bool,
    /// Wraps onto as many lines as it needs, and starts a new line at each
    /// '\n'. A cell of any other column is one line - a '\n' in it prints as
    /// a space - and one too wide is squeezed to fit, as the frame's fields
    /// are. Blanks and line breaks at either end of any cell are dropped.
    pub wrap: bool,
}

/// What a table holds, before it is fitted anywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSpec {
    /// One line across the top in bold; empty for none.
    pub heading: String,
    pub columns: Vec<TableColumn>,
    /// A text per column.
    pub rows: Vec<Vec<String>>,
    /// A row shaded lightly.
    pub highlight: Option<usize>,
    /// How many blocks of columns, side by side, the rows may continue in
    /// before the rest is left out and counted ("+12 more").
    pub max_blocks: usize,
    pub overflow_word: String,
}

impl Default for TableSpec {
    fn default() -> Self {
        Self {
            heading: String::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            highlight: None,
            max_blocks: 2,
            overflow_word: String::from("more"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableStyle {
    /// The text is the largest size from max_cap_mm down, in steps of
    /// step_mm, at which every row fits; never below min_cap_mm, the smallest
    /// text a plotted table is read at.
    pub max_cap_mm: f64,
    pub min_cap_mm: f64,
    pub step_mm: f64,
    /// A one-line row, in cap heights.
    pub row_pitch: f64,
    /// Each further line of a wrapped cell (the frame's rule).
    pub line_pitch: f64,
    /// Each side of a cell's text, in cap heights.
    pub padding_ratio: f64,
    /// The heading's cap height over the body's.
    pub heading_ratio: f64,
    /// Condensing, as the legend and notes are set.
    pub x_factor: f64,
    /// The box, the heading's and headers' rules, blocks.
    pub outer_weight_mm: f64,
    /// Between rows and columns.
    pub inner_weight_mm: f64,
    /// A layout that squeezes no line below this is preferred to one with
    /// larger text that does: a long title set smaller reads better than one
    /// pressed to half its width.
    pub least_squeeze: f64,
}

impl Default for TableStyle {
    fn default() -> Self {
        Self {
            max_cap_mm: 2.5,
            min_cap_mm: 1.8,
            step_mm: 0.1,
            row_pitch: 2.0,
            line_pitch: 1.5,
            padding_ratio: 0.5,
            heading_ratio: 1.25,
            x_factor: 0.9,
            outer_weight_mm: 0.25,
            inner_weight_mm: 0.13,
            least_squeeze: 0.8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableRule {
    pub from: Point2,
    pub to: Point2,
    pub weight_mm: f64,
}

impl Default for TableRule {
    fn default() -> Self {
        Self {
            from: Point2::default(),
            to: Point2::default(),
            weight_mm: 0.13,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableText {
    pub text: String,
    /// On the baseline: the text's left end, middle or right end, as
    /// `horizontal` says.
    pub anchor: Point2,
    pub cap_mm: f64,
    pub x_factor: f64,
    /// A further horizontal squeeze for a line wider than its cell; 1 for none.
    pub squeeze: f64,
    pub bold: bool,
    /// "+12 more": printed grey.
    pub muted: bool,
    pub horizontal: HorizontalJustify,
}

impl Default for TableText {
    fn default() -> Self {
        Self {
            text: String::new(),
            anchor: Point2::default(),
            cap_mm: 1.8,
            x_factor: 1.0,
            squeeze: 1.0,
            bold: false,
            muted: false,
            horizontal: HorizontalJustify::Left,
        }
    }
}

/// A table fitted into a rectangle, in paper millimetres. The painter fills
/// `shaded`, then draws `rules`, then `texts`; nothing here needs clipping.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableLayout {
    pub shaded: Vec<Box2>,
    pub rules: Vec<TableRule>,
    pub texts: Vec<TableText>,
    /// The body's cap height.
    pub cap_mm: f64,
    /// A one-line row.
    pub row_height_mm: f64,
    /// Blocks of columns side by side.
    pub blocks: usize,
    /// The rows shown are the first rows_shown, in order; row_boxes[i] is row
    /// i's rectangle. rows_hidden did not fit and are counted in the last line.
    pub rows_shown: usize,
    pub rows_hidden: usize,
    pub row_boxes: Vec<Box2>,
}

// ---- laying out -----------------------------------------------------------------

// `text` on one line: each line break a space.
fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

// `text` without the spaces, tabs and line breaks at either end: a
// description typed with a trailing line break would otherwise make its row
// a line taller, and a centred code with a trailing space would sit off
// centre.
fn trimmed(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\r', '\n'])
}

// A word of a wrapping cell and its width at a cap height of 1 mm.
struct Word<'a> {
    text: &'a str,
    width: f64,
}

type Paragraph<'a> = Vec<Word<'a>>;

// A cell measured once at a cap height of 1 mm, condensed: at a cap height of
// c every width is c times these (TextWidth's promise).
#[derive(Default)]
struct Cell<'a> {
    // The widest line.
    width: f64,
    // A wrapping column's only.
    widest_word: f64,
    // A wrapping column's only.
    paragraphs: Vec<Paragraph<'a>>,
}

struct Measured<'a> {
    heading: f64,
    headers: Vec<f64>,
    // [row][column]
    cells: Vec<Vec<Cell<'a>>>,
    space: f64,
}

// The lines a wrapping cell breaks into, `room` wide at a cap height of 1 mm,
// with their widths. Words are never split: one wider than the room has a
// line of its own and is squeezed. An empty paragraph is an empty line.
fn wrap_lines(cell: &Cell<'_>, room: f64, space: f64) -> Vec<(String, f64)> {
    let mut lines = Vec::new();
    for paragraph in &cell.paragraphs {
        let mut line = String::new();
        let mut width = 0.0;
        let mut started = false;
        for word in paragraph {
            if started && width + space + word.width > room {
                lines.push((std::mem::take(&mut line), width));
                width = 0.0;
                started = false;
            }
            if started {
                line.push(' ');
                width += space;
            }
            line.push_str(word.text);
            width += word.width;
            started = true;
        }
        lines.push((line, width));
    }
    lines
}

// One way of fitting the table: a text size, a number of blocks, the column
// widths that gives, and each row's number of lines.
struct Candidate {
    cap: f64,
    blocks: usize,
    block_width: f64,
    widths: Vec<f64>,
    width_fits: bool,
    // No line is squeezed below TableStyle::least_squeeze to fit its cell.
    comfortable: bool,
    // Each row's.
    heights: Vec<f64>,
    heading_height: f64,
    header_height: f64,
    // For rows in each block, under the heading and header.
    available: f64,
}

// Where the placed rows go: each row's block and its distance below the
// block's header.
#[derive(Clone, Copy)]
struct Placement {
    block: usize,
    offset: f64,
}

const SLACK: f64 = 1e-9;

fn evaluate(
    spec: &TableSpec,
    measured: &Measured<'_>,
    rect: &Box2,
    cap: f64,
    blocks: usize,
    style: &TableStyle,
) -> Candidate {
    let block_width = rect.width() / blocks as f64;
    let pad = style.padding_ratio * cap;
    let columns = spec.columns.len();

    // Every column but the stretching one as wide as its widest text; the
    // stretching one what is left, at least as wide as its header.
    let mut stretch: Option<usize> = None;
    let mut natural = vec![0.0; columns];
    for j in 0..columns {
        let mut widest = measured.headers[j];
        if spec.columns[j].stretch && stretch.is_none() {
            stretch = Some(j);
        } else {
            for row in &measured.cells {
                widest = cpp::max(widest, row[j].width);
            }
        }
        natural[j] = cap * widest + 2.0 * pad;
    }
    let mut fixed = 0.0;
    for (j, &width) in natural.iter().enumerate() {
        if Some(j) != stretch {
            fixed += width;
        }
    }
    let mut widths = natural.clone();
    let width_fits = match stretch {
        Some(stretch) => {
            widths[stretch] = block_width - fixed;
            widths[stretch] + SLACK >= natural[stretch]
        }
        None => {
            // No column stretches: the room left over is shared in proportion.
            let fits = fixed <= block_width + SLACK;
            for width in &mut widths {
                *width *= if fixed > 0.0 {
                    block_width / fixed
                } else {
                    0.0
                };
            }
            fits
        }
    };
    if !width_fits {
        // Too narrow: every column in proportion to what it needs, so the
        // table still stays inside its block and a last resort still draws.
        let needed = fixed + stretch.map_or(0.0, |stretch| natural[stretch]);
        for (width, &wanted) in widths.iter_mut().zip(&natural) {
            *width = if needed > 0.0 {
                wanted * block_width / needed
            } else {
                0.0
            };
        }
    }

    // Row heights: one line, or as many as the longest wrapped cell. And
    // whether any line - a whole cell, or a wrapped cell's longest word -
    // would be squeezed hard to fit.
    let mut comfortable = width_fits;
    let mut heights = Vec::with_capacity(measured.cells.len());
    for row in &measured.cells {
        let mut lines: usize = 1;
        for j in 0..columns {
            let room = cpp::max(widths[j] - 2.0 * pad, 0.0) / cap;
            let longest = if spec.columns[j].wrap {
                row[j].widest_word
            } else {
                row[j].width
            };
            if longest * style.least_squeeze > room {
                comfortable = false;
            }
            if spec.columns[j].wrap {
                lines = lines.max(wrap_lines(&row[j], room, measured.space).len());
            }
        }
        heights.push(cap * (style.row_pitch + style.line_pitch * (lines - 1) as f64));
    }
    let heading_height = if spec.heading.is_empty() {
        0.0
    } else {
        style.heading_ratio * cap * style.row_pitch
    };
    let header_height = cap * style.row_pitch;
    Candidate {
        cap,
        blocks,
        block_width,
        widths,
        width_fits,
        comfortable,
        heights,
        heading_height,
        header_height,
        available: rect.height() - heading_height - header_height,
    }
}

// The rows of `heights`, in order, into `blocks` blocks each `available`
// high: a row that does not fit what is left of a block starts the next. It
// stops at the first row that fits nowhere.
fn place(heights: &[f64], blocks: usize, available: f64) -> Vec<Placement> {
    let mut placed = Vec::new();
    let mut block = 0;
    let mut used = 0.0;
    for &height in heights {
        if used + height > available + SLACK {
            block += 1;
            used = 0.0;
        }
        if block >= blocks || height > available + SLACK {
            break;
        }
        placed.push(Placement {
            block,
            offset: used,
        });
        used += height;
    }
    placed
}

// The cap heights tried, largest first: every step from the maximum down,
// and the minimum itself.
fn caps_to_try(style: &TableStyle) -> Vec<f64> {
    let mut caps = Vec::new();
    let step = if style.step_mm > 0.0 {
        style.step_mm
    } else {
        style.max_cap_mm - style.min_cap_mm
    };
    let mut k: i32 = 0;
    while step > 0.0 && k < 1000 {
        let cap = style.max_cap_mm - f64::from(k) * step;
        if cap <= style.min_cap_mm + 1e-9 {
            break;
        }
        caps.push(cap);
        k += 1;
    }
    caps.push(style.min_cap_mm);
    caps
}

// A line of text in a cell `width` wide starting at `left`, squeezed when
// it is wider than the cell less its padding.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ cellText's arguments, one for one."
)]
fn cell_text(
    text: String,
    width_at_one: f64,
    left: f64,
    width: f64,
    baseline: f64,
    cap: f64,
    pad: f64,
    justify: HorizontalJustify,
    bold: bool,
    style: &TableStyle,
) -> TableText {
    let room = width - 2.0 * pad;
    let natural = width_at_one * cap;
    let squeeze = if natural > room && natural > 0.0 {
        cpp::max(room, 0.0) / natural
    } else {
        1.0
    };
    let x = match justify {
        HorizontalJustify::Centre => left + width / 2.0,
        HorizontalJustify::Right => left + width - pad,
        HorizontalJustify::Left => left + pad,
    };
    TableText {
        text,
        anchor: Point2::new(x, baseline),
        cap_mm: cap,
        x_factor: style.x_factor,
        squeeze,
        bold,
        muted: false,
        horizontal: justify,
    }
}

// The number a sheet prints: typed on it, else the set's for its position.
fn number_of(set: &SheetSet, index: usize) -> String {
    let sheet = &set.sheets[index];
    if let Some(typed) = sheet.fields.get("sheet_number") {
        return typed.clone();
    }
    format_sheet_number(
        &set.numbering,
        index + 1,
        set.sheets.len(),
        &set.defaults.set_number,
    )
}

// The first line's baseline in a row whose top is `row_top`: the cap height
// centred in a one-line row.
fn baseline(row_top: f64, cap_mm: f64, row_height: f64) -> f64 {
    row_top - (row_height + cap_mm) / 2.0
}

fn push_rule(rules: &mut Vec<TableRule>, from: Point2, to: Point2, weight_mm: f64) {
    rules.push(TableRule {
        from,
        to,
        weight_mm,
    });
}

/// A row per sheet, in the set's order, from the same fields the title blocks
/// print (resolve_fields), so the register and the sheets cannot disagree. An
/// automatic plan scale reads as its last choice; the painter decides them
/// first, as it does for a title block.
pub fn drawing_register(set: &SheetSet) -> Vec<RegisterRow> {
    // The five fields read here owe nothing to the project, so no context.
    let none = FieldContext::default();
    (0..set.sheets.len())
        .map(|i| {
            let fields = resolve_fields(set, i, &none);
            let field = |name: &str| fields.get(name).cloned().unwrap_or_default();
            RegisterRow {
                sheet_id: set.sheets[i].id.clone(),
                number: field("sheet_number"),
                title: field("sheet_name"),
                scale: field("scale"),
                paper: field("paper"),
                revision: field("revision"),
            }
        })
        .collect()
}

/// The set's revisions newest first - they are stored oldest first, in the
/// order they were issued - and only the newest `limit` of them when `limit`
/// is not 0 (the C++'s default).
pub fn revision_rows(set: &SheetSet, limit: usize) -> Vec<Revision> {
    let mut rows: Vec<Revision> = set.revisions.iter().rev().cloned().collect();
    if limit > 0 && rows.len() > limit {
        rows.truncate(limit);
    }
    rows
}

/// `spec` fitted into `rect`: the largest text (from style.max_cap_mm down to
/// style.min_cap_mm) at which every row fits one block; else the largest at
/// which they fit two blocks side by side (up to spec.max_blocks), each with
/// its own header - first among the layouts that squeeze no line below
/// style.least_squeeze, then among any; else the minimum text in as many
/// blocks as are wide enough, with as many rows as fit and "+N more" for the
/// rest. A block is wide enough when every column but the stretching one has
/// its widest text and the stretching one its header. Rules run the box's
/// full height, so a short register is a ruled form with room below. An
/// empty rectangle, or no columns, lays out nothing and counts every row
/// hidden. `measure` defaults to estimate_text_width, `style` (in the C++) to
/// TableStyle's defaults.
pub fn layout_table(
    spec: &TableSpec,
    rect: &Box2,
    measure: Option<&TextWidth<'_>>,
    style: &TableStyle,
) -> TableLayout {
    let mut out = TableLayout::default();
    let row_count = spec.rows.len();
    let columns = spec.columns.len();
    if columns == 0
        || rect.empty()
        || !(rect.width() > 0.0)
        || !(rect.height() > 0.0)
        || !(style.min_cap_mm > 0.0)
        || !(style.max_cap_mm >= style.min_cap_mm)
    {
        out.rows_hidden = row_count;
        return out;
    }
    let width: &TextWidth<'_> = measure.unwrap_or(&estimate_text_width);
    let at_one = |text: &str, bold: bool| {
        if text.is_empty() {
            0.0
        } else {
            width(text, 1.0, bold) * style.x_factor
        }
    };

    // What each cell prints: a line break in a cell that does not wrap, or
    // in the heading, is a space - the row is one line high, and a painter
    // would step a second line down over the row below. Blanks at either
    // end of a text are dropped (trimmed).
    let heading = one_line(trimmed(&spec.heading));
    let mut texts: Vec<Vec<String>> = vec![vec![String::new(); columns]; row_count];
    for (row, given) in texts.iter_mut().zip(&spec.rows) {
        for (j, (cell, text)) in row.iter_mut().zip(given).enumerate() {
            let text = trimmed(text);
            *cell = if spec.columns[j].wrap {
                String::from(text)
            } else {
                one_line(text)
            };
        }
    }

    // Everything measured once, at a cap height of 1 mm.
    let mut measured = Measured {
        heading: at_one(&heading, true),
        headers: spec
            .columns
            .iter()
            .map(|column| at_one(&column.header, true))
            .collect(),
        cells: Vec::with_capacity(row_count),
        space: at_one(" ", false),
    };
    for row in &texts {
        let mut cells = Vec::with_capacity(columns);
        for (j, text) in row.iter().enumerate() {
            let mut cell = Cell::default();
            if !spec.columns[j].wrap {
                cell.width = at_one(text, false);
                cells.push(cell);
                continue;
            }
            // Paragraphs on '\n', words on spaces (a tab, or the '\r' of a
            // "\r\n" typed on another system, is a space too).
            for line in text.split('\n') {
                let paragraph: Paragraph<'_> = line
                    .split([' ', '\t', '\r'])
                    .filter(|word| !word.is_empty())
                    .map(|word| Word {
                        text: word,
                        width: at_one(word, false),
                    })
                    .collect();
                cell.paragraphs.push(paragraph);
            }
            for paragraph in &cell.paragraphs {
                let mut line_width = 0.0;
                for word in paragraph {
                    line_width += (if line_width > 0.0 {
                        measured.space
                    } else {
                        0.0
                    }) + word.width;
                    cell.widest_word = cpp::max(cell.widest_word, word.width);
                }
                cell.width = cpp::max(cell.width, line_width);
            }
            cells.push(cell);
        }
        measured.cells.push(cells);
    }

    // The largest text at which every row fits one block, else two (up to
    // max_blocks) - first among the layouts that squeeze no line hard, then
    // among any - else the smallest text with as many rows as fit.
    let max_blocks = spec.max_blocks.max(1);
    let caps = caps_to_try(style);
    let mut chosen: Option<Candidate> = None;
    let mut placement: Vec<Placement> = Vec::new();
    for comfort in [true, false] {
        let mut blocks = 1;
        while blocks <= max_blocks && chosen.is_none() {
            for &cap in &caps {
                let candidate = evaluate(spec, &measured, rect, cap, blocks, style);
                if !candidate.width_fits
                    || (comfort && !candidate.comfortable)
                    || candidate.available < 0.0
                {
                    continue;
                }
                let rows = place(&candidate.heights, blocks, candidate.available);
                if rows.len() == row_count {
                    chosen = Some(candidate);
                    placement = rows;
                    break;
                }
            }
            blocks += 1;
        }
    }
    let mut hidden = 0;
    let mut overflow: Option<Placement> = None;
    let c = match chosen {
        Some(candidate) => candidate,
        None => {
            // Nothing fits: the smallest text, in as many blocks as are wide
            // enough for it, and a last line counting the rows left out.
            let mut blocks = 1;
            let mut b = max_blocks;
            while b > 1 {
                if evaluate(spec, &measured, rect, style.min_cap_mm, b, style).width_fits {
                    blocks = b;
                    break;
                }
                b -= 1;
            }
            let fallback = evaluate(spec, &measured, rect, style.min_cap_mm, blocks, style);
            if fallback.available < 0.0 {
                // Not even the heading and the header fit: no table, every
                // row counted as hidden.
                out.rows_hidden = row_count;
                out.cap_mm = style.min_cap_mm;
                out.row_height_mm = style.min_cap_mm * style.row_pitch;
                return out;
            }
            placement = place(&fallback.heights, blocks, fallback.available);
            if placement.len() < row_count {
                // The count's line goes after the last row shown; rows give
                // way to it until it fits.
                let line = fallback.cap * style.row_pitch;
                let mut shown = placement.len();
                loop {
                    let mut heights = fallback.heights[..shown].to_vec();
                    heights.push(line);
                    let mut trial = place(&heights, blocks, fallback.available);
                    if trial.len() == heights.len() {
                        overflow = trial.pop();
                        placement = trial;
                        break;
                    }
                    if shown == 0 {
                        placement.clear();
                        break;
                    }
                    shown -= 1;
                }
            }
            hidden = row_count - placement.len();
            fallback
        }
    };

    let cap = c.cap;
    let pad = style.padding_ratio * cap;
    let top = rect.max.y;
    let header_top = top - c.heading_height;
    let body_top = header_top - c.header_height;
    out.cap_mm = cap;
    out.row_height_mm = cap * style.row_pitch;
    out.blocks = c.blocks;
    out.rows_shown = placement.len();
    out.rows_hidden = hidden;

    // The box.
    push_rule(
        &mut out.rules,
        rect.min,
        Point2::new(rect.max.x, rect.min.y),
        style.outer_weight_mm,
    );
    push_rule(
        &mut out.rules,
        Point2::new(rect.max.x, rect.min.y),
        rect.max,
        style.outer_weight_mm,
    );
    push_rule(
        &mut out.rules,
        rect.max,
        Point2::new(rect.min.x, rect.max.y),
        style.outer_weight_mm,
    );
    push_rule(
        &mut out.rules,
        Point2::new(rect.min.x, rect.max.y),
        rect.min,
        style.outer_weight_mm,
    );

    // The heading, across the top.
    if !heading.is_empty() {
        let heading_cap = style.heading_ratio * cap;
        out.texts.push(cell_text(
            heading,
            measured.heading,
            rect.min.x,
            rect.width(),
            baseline(top, heading_cap, c.heading_height),
            heading_cap,
            style.padding_ratio * heading_cap,
            HorizontalJustify::Left,
            true,
            style,
        ));
        push_rule(
            &mut out.rules,
            Point2::new(rect.min.x, header_top),
            Point2::new(rect.max.x, header_top),
            style.outer_weight_mm,
        );
    }

    // Each block: its header, its rules down to the foot of the box - or, in
    // the block that ends with the count of the rows left out, down to that
    // line, which is one cell across the block.
    let block_left = |block: usize| rect.min.x + block as f64 * c.block_width;
    for b in 0..c.blocks {
        let left = block_left(b);
        if b > 0 {
            push_rule(
                &mut out.rules,
                Point2::new(left, header_top),
                Point2::new(left, rect.min.y),
                style.outer_weight_mm,
            );
        }
        let foot = match overflow {
            Some(overflow) if overflow.block == b => body_top - overflow.offset,
            _ => rect.min.y,
        };
        let mut x = left;
        for (j, column) in spec.columns.iter().enumerate() {
            if j > 0 {
                push_rule(
                    &mut out.rules,
                    Point2::new(x, header_top),
                    Point2::new(x, foot),
                    style.inner_weight_mm,
                );
            }
            if !column.header.is_empty() {
                out.texts.push(cell_text(
                    column.header.clone(),
                    measured.headers[j],
                    x,
                    c.widths[j],
                    baseline(header_top, cap, c.header_height),
                    cap,
                    pad,
                    column.justify,
                    true,
                    style,
                ));
            }
            x += c.widths[j];
        }
        push_rule(
            &mut out.rules,
            Point2::new(left, body_top),
            Point2::new(left + c.block_width, body_top),
            style.outer_weight_mm,
        );
    }

    // The rows.
    for (i, placed) in placement.iter().enumerate() {
        let left = block_left(placed.block);
        let row_top = body_top - placed.offset;
        let row_bottom = row_top - c.heights[i];
        let row_box = Box2::new(
            Point2::new(left, row_bottom),
            Point2::new(left + c.block_width, row_top),
        );
        out.row_boxes.push(row_box);
        if spec.highlight == Some(i) {
            out.shaded.push(row_box);
        }
        let first = baseline(row_top, cap, out.row_height_mm);
        let mut x = left;
        for (j, column) in spec.columns.iter().enumerate() {
            let cell = &measured.cells[i][j];
            if column.wrap {
                let room = cpp::max(c.widths[j] - 2.0 * pad, 0.0) / cap;
                let mut y = first;
                for (line, line_width) in wrap_lines(cell, room, measured.space) {
                    if !line.is_empty() {
                        out.texts.push(cell_text(
                            line,
                            line_width,
                            x,
                            c.widths[j],
                            y,
                            cap,
                            pad,
                            column.justify,
                            false,
                            style,
                        ));
                    }
                    y -= style.line_pitch * cap;
                }
            } else if !texts[i][j].is_empty() {
                out.texts.push(cell_text(
                    texts[i][j].clone(),
                    cell.width,
                    x,
                    c.widths[j],
                    first,
                    cap,
                    pad,
                    column.justify,
                    false,
                    style,
                ));
            }
            x += c.widths[j];
        }
        push_rule(
            &mut out.rules,
            Point2::new(left, row_bottom),
            Point2::new(left + c.block_width, row_bottom),
            style.inner_weight_mm,
        );
    }

    // What did not fit, counted where the next row would have gone.
    if let Some(overflow) = overflow
        && hidden > 0
    {
        let left = block_left(overflow.block);
        let row_top = body_top - overflow.offset;
        let text = format!("+{hidden} {}", spec.overflow_word);
        let width_at_one = at_one(&text, false);
        let mut count = cell_text(
            text,
            width_at_one,
            left,
            c.block_width,
            baseline(row_top, cap, out.row_height_mm),
            cap,
            pad,
            HorizontalJustify::Right,
            false,
            style,
        );
        count.muted = true;
        out.texts.push(count);
    }
    out
}

/// What a SheetIndex or Revisions viewport on sheet `sheet_index` shows:
///
/// ```text
/// SheetIndex  SHEET No. | TITLE | SCALE | PAPER | REV, a row per sheet
///             (drawing_register), sheet `sheet_index` highlighted, continuing
///             in a second block;
/// Revisions   REV | DATE | DESCRIPTION | BY, newest first, the viewport's
///             revision_limit newest only, the description wrapped, in one
///             block ("+N earlier" for what does not fit).
/// ```
///
/// The heading is the viewport's title, else automatic_title. A spec with no
/// columns for any other kind.
pub fn table_spec_for(set: &SheetSet, sheet_index: usize, viewport: &Viewport) -> TableSpec {
    let mut spec = TableSpec::default();
    let heading = if viewport.title.is_empty() {
        automatic_title(viewport)
    } else {
        viewport.title.clone()
    };
    let column =
        |header: &str, justify: HorizontalJustify, stretch: bool, wrap: bool| TableColumn {
            header: String::from(header),
            justify,
            stretch,
            wrap,
        };
    if viewport.kind == ViewportKind::SheetIndex {
        spec.heading = heading;
        spec.columns = vec![
            column("SHEET No.", HorizontalJustify::Centre, false, false),
            column("TITLE", HorizontalJustify::Left, true, false),
            column("SCALE", HorizontalJustify::Centre, false, false),
            column("PAPER", HorizontalJustify::Centre, false, false),
            column("REV", HorizontalJustify::Centre, false, false),
        ];
        for row in drawing_register(set) {
            spec.rows.push(vec![
                row.number,
                row.title,
                row.scale,
                row.paper,
                row.revision,
            ]);
        }
        if sheet_index < set.sheets.len() {
            spec.highlight = Some(sheet_index);
        }
        spec.max_blocks = 2;
        spec.overflow_word = String::from("more");
    } else if viewport.kind == ViewportKind::Revisions {
        spec.heading = heading;
        spec.columns = vec![
            column("REV", HorizontalJustify::Centre, false, false),
            column("DATE", HorizontalJustify::Centre, false, false),
            column("DESCRIPTION", HorizontalJustify::Left, true, true),
            column("BY", HorizontalJustify::Centre, false, false),
        ];
        for revision in revision_rows(set, viewport.revision_limit) {
            spec.rows.push(vec![
                revision.code,
                revision.date,
                revision.description,
                revision.by,
            ]);
        }
        // A revision table reads down from the newest; a second block would
        // put older revisions level with newer ones.
        spec.max_blocks = 1;
        spec.overflow_word = String::from("earlier");
    }
    spec
}

/// layout_table(table_spec_for(...), viewport.rect, measure) at the default
/// style.
pub fn layout_viewport_table(
    set: &SheetSet,
    sheet_index: usize,
    viewport: &Viewport,
    measure: Option<&TextWidth<'_>>,
) -> TableLayout {
    layout_table(
        &table_spec_for(set, sheet_index, viewport),
        &viewport.rect,
        measure,
        &TableStyle::default(),
    )
}

/// The name register_sheet gives its sheet.
pub const REGISTER_SHEET_NAME: &str = "DRAWING REGISTER";

/// A cover sheet named DRAWING REGISTER: the register (SheetIndex) in the big
/// cell of "Main and panel right" and the revision table (Revisions) in the
/// panel beside it, without the frame's utility legend, which says nothing on
/// a cover. Ids s1, vp1 and vp2, as a generator's; prepare_for_append
/// renumbers them. AlreadyExists when a sheet of `set` already has a register:
/// two registers would be two lists to keep in step.
pub fn register_sheet(set: &SheetSet, paper: &SheetTemplate) -> Result<Sheet> {
    for (i, sheet) in set.sheets.iter().enumerate() {
        if sheet
            .viewports
            .iter()
            .any(|viewport| viewport.kind == ViewportKind::SheetIndex)
        {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "the set already has a drawing register",
                format!("sheet {} ({})", number_of(set, i), sheet.name),
            ));
        }
    }
    let mut sheet = blank_sheet(paper, String::from(REGISTER_SHEET_NAME));
    sheet.id = String::from("s1");
    // The frame's utility legend explains symbols a cover does not draw.
    sheet.frame_legend = false;
    sheet.viewports = vec![
        Viewport {
            id: String::from("vp1"),
            kind: ViewportKind::SheetIndex,
            ..Viewport::default()
        },
        Viewport {
            id: String::from("vp2"),
            kind: ViewportKind::Revisions,
            ..Viewport::default()
        },
    ];
    // The register ranks first, so it takes the big cell (tiling_rank).
    tile_viewports(&mut sheet, TilingPreset::MainRight, TILING_GUTTER_MM);
    Ok(sheet)
}

/// Puts register_sheet(the document's set, paper) FIRST in the set - a cover
/// comes before what it lists - with fresh ids, as ONE undoable step, and
/// returns the new sheet's id. The other sheets' numbers move up by one; their
/// marks follow them by id.
pub fn add_register_sheet(document: &mut Document, paper: &SheetTemplate) -> Result<String> {
    let mut set = SheetSet::clone(&document.sheet_set());
    let sheet = register_sheet(&set, paper)?;
    let mut one = vec![sheet];
    prepare_for_append(&set, &mut one);
    let cover = one.remove(0);
    let id = cover.id.clone();
    set.sheets.insert(0, cover);
    document.set_sheet_set(&set, "ADD_REGISTER_SHEET")?;
    Ok(id)
}
