//! Sheets: a project's drawings on paper (docs/plotting.md;
//! include/katana/cad/plotting/sheet_set.hpp).
//!
//! A SheetSet is every sheet of a project: the title-block values they share,
//! how they are numbered, the revisions table, and the sheets themselves. A
//! Sheet is one piece of paper - its size, its frame, the few title-block
//! values it overrides, and its viewports. A Viewport is a window onto the
//! drawing (a plan, a long section, cross sections, a 3D snapshot) or a panel
//! of paper furniture (a legend, notes, an image, a key plan) at a rectangle on
//! the paper.
//!
//! All of it is plain values: saved as versioned JSON under the project's
//! "sheets" metadata key (sheet_json.rs), changed through undoable commands
//! (sheet_commands.rs), and painted by the window. Paper rectangles are
//! millimetres from the bottom-left corner of the paper, Y up (frame.rs).
//!
//! Title-block FIELDS resolve automatically - sheet number and count, scale,
//! dates, project, coordinate system, file name - and a value the user typed
//! always wins (resolve_fields).
//!
//! LegendScope's names live here with the enum (the C++ declares them in
//! legend.hpp), so the JSON can read and write them without the legend.

use std::collections::BTreeMap;

use katana_entity::EnumName;
use katana_geometry::{Box2, Point2};

use crate::layer_overrides::LayerOverrides;
use crate::plot::{PaperSize, paper_dimensions};
use crate::plotting::frame::{BUILT_IN_FRAME_ID, built_in_frame, frame_scale_for};
use crate::plotting::layout::tiling_rank;
use crate::plotting::page_setup::PageSetup;

/// What a viewport shows. Every kind has a tiling rank (layout.rs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewportKind {
    /// The drawing in plan, at a scale, rotated to suit.
    #[default]
    Plan,
    /// A profile along an alignment.
    LongSection,
    /// One or more cross sections of an alignment.
    CrossSections,
    /// A snapshot of the 3D view.
    Model3D,
    /// The styles and layers drawn on the sheet.
    Legend,
    /// Free text.
    Notes,
    /// An image from the project's assets.
    Image,
    /// Where the other sheets are: their outlines, numbered.
    KeyPlan,
    /// The drawing register: every sheet's number, title, scale, paper and
    /// revision (tables.rs).
    SheetIndex,
    /// The revision table, newest first (tables.rs).
    Revisions,
}

const KIND_NAMES: [(ViewportKind, &str); 10] = [
    (ViewportKind::Plan, "plan"),
    (ViewportKind::LongSection, "long_section"),
    (ViewportKind::CrossSections, "cross_sections"),
    (ViewportKind::Model3D, "model_3d"),
    (ViewportKind::Legend, "legend"),
    (ViewportKind::Notes, "notes"),
    (ViewportKind::Image, "image"),
    (ViewportKind::KeyPlan, "key_plan"),
    (ViewportKind::SheetIndex, "sheet_index"),
    (ViewportKind::Revisions, "revisions"),
];

/// The C++ `toString(ViewportKind)`.
impl EnumName for ViewportKind {
    fn name(self) -> &'static str {
        for (value, name) in KIND_NAMES {
            if value == self {
                return name;
            }
        }
        "plan"
    }
}

pub fn viewport_kind_from(name: &str) -> Option<ViewportKind> {
    KIND_NAMES
        .iter()
        .find(|(_, known)| *known == name)
        .map(|(value, _)| *value)
}

/// What a section or strip viewport is cut from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewportSource {
    /// The alignment's name; empty for a plain plan.
    pub alignment: String,
    /// The chainage range shown (LongSection, strips).
    pub chainage_from: f64,
    pub chainage_to: f64,
    /// CrossSections: every so many metres, when > 0.
    pub section_interval: f64,
    /// CrossSections: these chainages, when given.
    pub stations: Vec<f64>,
    pub section_half_width: f64,
}

/// How a plan draws its coordinate grid (plan_grid.rs): not at all, ticks in
/// from its border, a small cross at every intersection, or light lines right
/// across. Every style but None labels the eastings and northings at the edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridStyle {
    #[default]
    None,
    Ticks,
    Crosses,
    Lines,
}

/// What a WorldMark is: the C++ `WorldMark::Kind`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WorldMarkKind {
    #[default]
    MatchLine,
    SheetOutline,
}

/// A line or outline drawn over a viewport in world coordinates: the match
/// line where the next sheet takes over, or a key plan's sheet outlines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldMark {
    pub kind: WorldMarkKind,
    pub points: Vec<Point2>,
    /// "MATCH LINE CH 250.000"; the sheet it leads to is added when it is
    /// drawn (mark_label), so reordering the sheets cannot make it lie.
    pub label: String,
    /// The id of the sheet this mark refers to; empty for none.
    pub sheet: String,
}

/// What a Legend viewport lists (legend.rs): what the plans of its own sheet
/// show, what the plans of every sheet show, or everything drawn. Stored by
/// name ("this_sheet", "whole_set", "whole_drawing"), never renamed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LegendScope {
    #[default]
    ThisSheet,
    WholeSet,
    WholeDrawing,
}

/// The C++ `toString(LegendScope)` (legend.cpp).
impl EnumName for LegendScope {
    fn name(self) -> &'static str {
        match self {
            LegendScope::ThisSheet => "this_sheet",
            LegendScope::WholeSet => "whole_set",
            LegendScope::WholeDrawing => "whole_drawing",
        }
    }
}

/// The C++ `legendScopeFrom` (legend.cpp).
pub fn legend_scope_from(name: &str) -> Option<LegendScope> {
    [
        LegendScope::ThisSheet,
        LegendScope::WholeSet,
        LegendScope::WholeDrawing,
    ]
    .into_iter()
    .find(|scope| scope.name() == name)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    /// Unique within the sheet set; "vp1", "vp2" ...
    pub id: String,
    pub kind: ViewportKind,
    /// On the paper, millimetres.
    pub rect: Box2,
    /// 1 : scale. With auto_scale the painter chooses the largest standard
    /// scale (SHEET_SCALES) at which the content fits, and this is the last
    /// choice.
    pub scale: f64,
    pub auto_scale: bool,
    /// The world point at the rectangle's centre. For a section it is (offset
    /// or chainage, elevation); with auto_centre the painter centres the
    /// content, keeping the scale - a section whose ground is not sampled yet.
    pub centre: Point2,
    pub auto_centre: bool,
    /// Radians counter-clockwise: the world direction that runs left to right
    /// across the paper. A strip along an alignment is rotated to its bearing.
    pub rotation: f64,
    /// Sections only.
    pub vertical_exaggeration: f64,
    /// Model3D: the camera's height angle.
    pub tilt_degrees: f64,
    pub source: ViewportSource,
    /// Hidden in this viewport, as a view hides them.
    pub hidden_layers: LayerOverrides,
    /// Empty: automatic_title().
    pub title: String,
    pub north_arrow: bool,
    pub scale_bar: bool,
    /// Tiling and snapping leave a locked viewport alone.
    pub locked: bool,
    /// Notes: the text; Image: the asset's file name.
    pub text: String,
    pub marks: Vec<WorldMark>,
    /// Revisions: only the newest so many; 0 shows every revision.
    pub revision_limit: usize,
    /// Legend only.
    pub legend_scope: LegendScope,
    /// A plan's coordinate grid, and its spacing in metres; 0 is automatic: the
    /// round spacing about 50 mm apart on the paper (plan_grid.rs).
    pub grid_style: GridStyle,
    pub grid_interval: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: ViewportKind::Plan,
            rect: Box2::default(),
            scale: 500.0,
            auto_scale: false,
            centre: Point2::default(),
            auto_centre: false,
            rotation: 0.0,
            vertical_exaggeration: 1.0,
            tilt_degrees: 30.0,
            source: ViewportSource::default(),
            hidden_layers: LayerOverrides::default(),
            title: String::new(),
            north_arrow: false,
            scale_bar: false,
            locked: false,
            text: String::new(),
            marks: Vec::new(),
            revision_limit: 0,
            legend_scope: LegendScope::ThisSheet,
            grid_style: GridStyle::None,
            grid_interval: 0.0,
        }
    }
}

/// The scale and centre a plan viewport is drawn at once "auto" is decided:
/// what the legend reads its window from (legend.rs) and what preflight
/// checks (preflight.rs), both resolved by the painter's rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanWindow {
    pub scale: f64,
    pub centre: Point2,
}

impl Default for PlanWindow {
    fn default() -> Self {
        Self {
            scale: 500.0,
            centre: Point2::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    /// Stable for the sheet's life - "s1", "s2" ... - so a match line or key
    /// plan can refer to it however the sheets are reordered or renamed; and
    /// not given to a new sheet while a mark still names it (new_sheet_ids).
    pub id: String,
    pub name: String,
    pub paper: PaperSize,
    pub landscape: bool,
    /// Empty: no frame.
    pub frame: String,
    /// The frame's utility legend block.
    pub frame_legend: bool,
    /// Title-block values this sheet overrides, by field name. A value here
    /// always wins over the set's defaults and over anything automatic.
    pub fields: BTreeMap<String, String>,
    /// Back to front.
    pub viewports: Vec<Viewport>,
}

impl Default for Sheet {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            paper: PaperSize::A3,
            landscape: true,
            frame: String::from(BUILT_IN_FRAME_ID),
            frame_legend: true,
            fields: BTreeMap::new(),
            viewports: Vec::new(),
        }
    }
}

/// A sign-off row: who, and when. An empty date prints the plot date.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignOff {
    pub name: String,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Revision {
    /// "A", "B", "0" ...
    pub code: String,
    pub date: String,
    pub description: String,
    pub by: String,
}

/// The title-block values every sheet shares. An empty value falls back to
/// what the project knows (docs/plotting.md lists which).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetDefaults {
    pub organisation: String,
    /// Up to four; the first two default to the project's name and
    /// description.
    pub project_lines: Vec<String>,
    pub client: String,
    /// UTILITIES.
    pub locator: SignOff,
    /// SURVEYED.
    pub surveyor: SignOff,
    /// COMPILED.
    pub compiler: SignOff,
    /// REVIEWED.
    pub reviewer: SignOff,
    /// The manager's row under the notes.
    pub approver: SignOff,
    pub notes: String,
    pub height_datum: String,
    /// Empty: the project's coordinate system.
    pub coordinate_system: String,
    pub model_name: String,
    pub set_number: String,
    /// A file in the project's assets/ directory.
    pub logo_asset: String,
}

pub const SHEET_SET_VERSION: i32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct SheetSet {
    pub defaults: SheetDefaults,
    /// How a sheet number is written: {n} is the sheet's position from 1,
    /// {n:02} the same padded to two digits, {N} the count, {set} the set
    /// number. "{n}" by default, as the frame's SHEET No. cell expects.
    pub numbering: String,
    /// Oldest first, in the order issued: the last is the current revision,
    /// whatever its date or code says (resolve_fields, tables.rs).
    pub revisions: Vec<Revision>,
    pub sheets: Vec<Sheet>,
    /// How the set is plotted: the plot style, the resolution, the file
    /// naming (page_setup.rs).
    pub page_setup: PageSetup,
}

impl Default for SheetSet {
    fn default() -> Self {
        Self {
            defaults: SheetDefaults::default(),
            numbering: String::from("{n}"),
            revisions: Vec::new(),
            sheets: Vec::new(),
            page_setup: PageSetup::default(),
        }
    }
}

/// What the project contributes to the fields; filled by the caller so this
/// stays a pure function (field_context_for in sheet_commands.rs builds one
/// from a Document).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldContext {
    pub project_name: String,
    pub project_description: String,
    pub coordinate_system: String,
    pub file_name: String,
    /// As it is printed: the frame writes dd/mm/yy.
    pub plot_date: String,
}

// The margin all round a sheet with no frame: the single-page plot's default
// (PlotSettings::margin_mm), so a frameless sheet and a plot agree.
const FRAMELESS_MARGIN_MM: f64 = 10.0;

fn is_section(kind: ViewportKind) -> bool {
    kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections
}

// Whether a viewport of this kind is drawn to a scale a title block reports.
fn is_scaled(kind: ViewportKind) -> bool {
    kind == ViewportKind::Plan || is_section(kind)
}

// A denominator as a scale rule carries it: whole. 1 : 31.25 (a 1 : 250
// section at 8 times) prints as 1 : 31, as the owner's app rounds it. The
// C++'s llround is undefined for a denominator past the range of a long long
// (and for a NaN); the cast here saturates, and no guard was there to port.
fn denominator_text(denominator: f64) -> String {
    format!("{}", denominator.round() as i64)
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

fn or_default(value: &str, fallback: &str) -> String {
    String::from(if value.is_empty() { fallback } else { value })
}

/// `std::from_chars` into a `std::size_t` over `text`: the value of the run of
/// digits it starts with, and how many bytes that run is. The value is None
/// when there is no digit or the number does not fit; the C++ then leaves its
/// variable as it was.
pub(crate) fn digits_prefix(text: &[u8]) -> (Option<usize>, usize) {
    let digits = text.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return (None, 0);
    }
    let mut value: usize = 0;
    for &byte in &text[..digits] {
        match value
            .checked_mul(10)
            .and_then(|tens| tens.checked_add(usize::from(byte - b'0')))
        {
            Some(next) => value = next,
            None => return (None, digits),
        }
    }
    (Some(value), digits)
}

/// The C++'s `std::format("{:.Nf}", value)`. Rust writes the same digits for
/// every finite value and an infinity; unlike katana_core's format_fixed,
/// std::format keeps the sign of a value that rounds to zero ("-0.000"), and it
/// spells a NaN "nan" or "-nan" where Rust writes "NaN" (the same helper as
/// katana_survey's utility report).
pub(crate) fn std_fixed(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        return String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        });
    }
    format!("{value:.decimals$}")
}

// The number after `prefix` in `id`, or 0 for an id of another shape -
// anything but digits after the prefix, or digits too many for a count, which
// no id given out here can equal.
fn number_after(id: &str, prefix: &str) -> usize {
    let Some(rest) = id.strip_prefix(prefix) else {
        return 0;
    };
    match digits_prefix(rest.as_bytes()) {
        (Some(number), taken) if taken == rest.len() => number,
        _ => 0,
    }
}

// `count` numbers, in order, that `used` does not hold: those after the
// highest; or, when the highest is too near the largest count to have
// `count` after it - only an id typed into the stored JSON can be - the
// lowest free ones. Ids are compared as numbers - "vp01" counts as 1, and 1
// is not given out beside it - and 0 is never given out.
fn fresh_numbers(mut used: Vec<usize>, count: usize) -> Vec<usize> {
    used.sort_unstable();
    let highest = used.last().copied().unwrap_or(0);
    let mut fresh = Vec::with_capacity(count);
    if count <= usize::MAX - highest {
        for i in 1..=count {
            fresh.push(highest + i);
        }
        return fresh;
    }
    let mut taken = 0;
    let mut candidate: usize = 1;
    while fresh.len() < count {
        while taken < used.len() && used[taken] < candidate {
            taken += 1;
        }
        if taken == used.len() || used[taken] != candidate {
            fresh.push(candidate);
        }
        candidate += 1;
    }
    fresh
}

fn ids_from(prefix: &str, numbers: &[usize]) -> Vec<String> {
    numbers
        .iter()
        .map(|number| format!("{prefix}{number}"))
        .collect()
}

/// The sheet number as `pattern` writes it for sheet `number` of `count`.
pub fn format_sheet_number(pattern: &str, number: usize, count: usize, set_number: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    while at < pattern.len() {
        let Some(open) = pattern[at..].find('{').map(|found| found + at) else {
            out.push_str(&pattern[at..]);
            break;
        };
        out.push_str(&pattern[at..open]);
        let Some(close) = pattern[open..].find('}').map(|found| found + open) else {
            out.push_str(&pattern[open..]);
            break;
        };
        let token = &pattern[open + 1..close];
        let (name, width) = match token.find(':') {
            Some(colon) => (
                &token[..colon],
                digits_prefix(&token.as_bytes()[colon + 1..]).0.unwrap_or(0),
            ),
            None => (token, 0),
        };
        let mut value = match name {
            "n" => format!("{number}"),
            "N" => format!("{count}"),
            "set" => String::from(set_number),
            _ => String::from(&pattern[open..=close]),
        };
        if value.len() < width {
            value.insert_str(0, &"0".repeat(width - value.len()));
        }
        out.push_str(&value);
        at = close + 1;
    }
    out
}

/// `text` with each {name} replaced by fields[name]; an unknown name becomes
/// empty text. "{{" is a literal brace.
pub fn expand_template(text: &str, fields: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if rest.starts_with("{{") {
            out.push('{');
            at += 2;
            continue;
        }
        if rest.starts_with('{') {
            let Some(close) = rest.find('}') else {
                out.push_str(rest);
                break;
            };
            if let Some(value) = fields.get(&rest[1..close]) {
                out.push_str(value);
            }
            at += close + 1;
            continue;
        }
        // Up to the next brace, copied as it is.
        let run = rest.find('{').unwrap_or(rest.len());
        out.push_str(&rest[..run]);
        at += run;
    }
    out
}

/// How a scale is written: "1:500"; a section says both, "H 1:500 V 1:50",
/// even when they are equal - a lone 1:500 leaves the reader to assume the
/// vertical, and that is the assumption that goes wrong.
pub fn scale_text(viewport: &Viewport) -> String {
    if is_section(viewport.kind) {
        let vertical = if viewport.vertical_exaggeration > 0.0 {
            viewport.scale / viewport.vertical_exaggeration
        } else {
            viewport.scale
        };
        return format!(
            "H 1:{} V 1:{}",
            denominator_text(viewport.scale),
            denominator_text(vertical)
        );
    }
    format!("1:{}", denominator_text(viewport.scale))
}

/// "PLAN 1:500", "LONG SECTION H 1:500 V 1:50", "CROSS SECTION CH 120.000"...
pub fn automatic_title(viewport: &Viewport) -> String {
    match viewport.kind {
        ViewportKind::Plan => format!("PLAN {}", scale_text(viewport)),
        ViewportKind::LongSection => format!("LONG SECTION {}", scale_text(viewport)),
        ViewportKind::CrossSections => {
            if viewport.source.stations.len() == 1 {
                return format!(
                    "CROSS SECTION CH {}",
                    std_fixed(viewport.source.stations[0], 3)
                );
            }
            format!("CROSS SECTIONS {}", scale_text(viewport))
        }
        ViewportKind::Model3D => String::from("3D VIEW"),
        ViewportKind::Legend => String::from("LEGEND"),
        ViewportKind::Notes => String::from("NOTES"),
        ViewportKind::Image => String::new(),
        ViewportKind::KeyPlan => String::from("KEY PLAN"),
        ViewportKind::SheetIndex => String::from("DRAWING REGISTER"),
        ViewportKind::Revisions => String::from("REVISIONS"),
    }
}

// The scale text of the main viewport among those `counts` takes (the lowest
// tiling rank), "AS SHOWN" when those disagree, empty when there is none.
fn reported_scale(sheet: &Sheet, counts: impl Fn(ViewportKind) -> bool) -> String {
    let mut main: Option<&Viewport> = None;
    for viewport in &sheet.viewports {
        if counts(viewport.kind)
            && main.is_none_or(|chosen| tiling_rank(viewport.kind) < tiling_rank(chosen.kind))
        {
            main = Some(viewport);
        }
    }
    let Some(main) = main else {
        return String::new();
    };
    let text = scale_text(main);
    for viewport in &sheet.viewports {
        if counts(viewport.kind) && scale_text(viewport) != text {
            return String::from("AS SHOWN");
        }
    }
    text
}

/// The scale the title block reports: the main viewport's (the lowest tiling
/// rank among plans and sections), "AS SHOWN" when those disagree. A key plan
/// beside a plan has a scale of its own and does not make the sheet "AS
/// SHOWN"; on a sheet with no plan or section - a key-plan sheet - the key
/// plan's scale is the sheet's. "N.T.S." when nothing is drawn to scale.
pub fn sheet_scale_text(sheet: &Sheet) -> String {
    // Plans and sections first; a key plan only when there is neither - an
    // inset key plan is at its own, smaller scale by design, and a key-plan
    // sheet is still drawn to one.
    let text = reported_scale(sheet, is_scaled);
    if !text.is_empty() {
        return text;
    }
    let text = reported_scale(sheet, |kind| kind == ViewportKind::KeyPlan);
    if !text.is_empty() {
        return text;
    }
    String::from("N.T.S.")
}

/// Every field of sheet `sheet_index`, resolved: an override on the sheet,
/// else the set's value, else the automatic one. Empty map for an index past
/// the end. The names are those frame texts use: sheet_number, sheet_count,
/// sheet_name, scale, paper, plot_date, <role>_name and <role>_date for the
/// five sign-offs, organisation, client, project_line_1..4, set_number,
/// coordinate_system, height_datum, model_name, file_name, notes, revision.
pub fn resolve_fields(
    set: &SheetSet,
    sheet_index: usize,
    context: &FieldContext,
) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let Some(sheet) = set.sheets.get(sheet_index) else {
        return fields;
    };
    let d = &set.defaults;
    let count = set.sheets.len();
    let mut put = |name: &str, value: String| {
        fields.insert(String::from(name), value);
    };
    // The count is the set's size - the owner's app left it as typed, so a
    // set that grew to twelve sheets still said "of 1".
    put(
        "sheet_number",
        format_sheet_number(&set.numbering, sheet_index + 1, count, &d.set_number),
    );
    put("sheet_count", format!("{count}"));
    put("sheet_name", sheet.name.clone());
    put("scale", sheet_scale_text(sheet));
    put("paper", String::from(paper_name(sheet.paper)));
    put("plot_date", context.plot_date.clone());
    let sign_offs = [
        ("locator", &d.locator),
        ("surveyor", &d.surveyor),
        ("compiler", &d.compiler),
        ("reviewer", &d.reviewer),
        ("approver", &d.approver),
    ];
    // Each sign-off keeps its own date; one not given is the plot's.
    for (role, sign_off) in sign_offs {
        put(&format!("{role}_name"), sign_off.name.clone());
        put(
            &format!("{role}_date"),
            or_default(&sign_off.date, &context.plot_date),
        );
    }
    put("organisation", d.organisation.clone());
    put("client", d.client.clone());
    put("notes", d.notes.clone());
    put("height_datum", d.height_datum.clone());
    put(
        "coordinate_system",
        or_default(&d.coordinate_system, &context.coordinate_system),
    );
    put("model_name", d.model_name.clone());
    put("set_number", d.set_number.clone());
    let line = |index: usize| d.project_lines.get(index).cloned().unwrap_or_default();
    put(
        "project_line_1",
        or_default(&line(0), &context.project_name),
    );
    put(
        "project_line_2",
        or_default(&line(1), &context.project_description),
    );
    put("project_line_3", line(2));
    put("project_line_4", line(3));
    put("file_name", context.file_name.clone());
    put(
        "revision",
        set.revisions
            .last()
            .map(|revision| revision.code.clone())
            .unwrap_or_default(),
    );
    // What the user typed on the sheet wins over everything above.
    for (name, value) in &sheet.fields {
        put(name, value.clone());
    }
    fields
}

/// Where a sheet's viewports go: the frame's drawing area, or the paper less
/// 10 mm all round when the sheet has no frame (or cannot have one).
pub fn drawing_area(sheet: &Sheet) -> Box2 {
    if !sheet.frame.is_empty()
        && sheet.landscape
        && sheet.frame == BUILT_IN_FRAME_ID
        && let Ok(frame) = built_in_frame()
    {
        let factor = frame_scale_for(sheet.paper, true);
        let area = &frame.drawing_area;
        return Box2::new(
            Point2::new(area.min.x * factor, area.min.y * factor),
            Point2::new(area.max.x * factor, area.max.y * factor),
        );
    }
    let paper = paper_dimensions(sheet.paper, sheet.landscape);
    Box2::new(
        Point2::new(FRAMELESS_MARGIN_MM, FRAMELESS_MARGIN_MM),
        Point2::new(
            paper.width_mm - FRAMELESS_MARGIN_MM,
            paper.height_mm - FRAMELESS_MARGIN_MM,
        ),
    )
}

/// `count` viewport ids, in order, that no viewport in `set` has: "vp" and the
/// numbers after the highest in use. Ids come from stored JSON unchecked, so
/// one too near the largest count to have `count` numbers after it gives the
/// lowest free numbers instead; nothing here fails or repeats an id.
pub fn new_viewport_ids(set: &SheetSet, count: usize) -> Vec<String> {
    let used: Vec<usize> = set
        .sheets
        .iter()
        .flat_map(|sheet| sheet.viewports.iter())
        .map(|viewport| number_after(&viewport.id, "vp"))
        .collect();
    ids_from("vp", &fresh_numbers(used, count))
}

/// `count` sheet ids, in order, that no sheet in `set` has and no mark in it
/// names: a sheet removed while a match line or a key-plan outline still leads
/// to it keeps its id from the sheets added after it, so the mark leads
/// nowhere (mark_label prints its label alone) rather than to a stranger.
pub fn new_sheet_ids(set: &SheetSet, count: usize) -> Vec<String> {
    // The ids marks name count as used as well as the sheets' own: when a
    // sheet is removed, the match lines and key-plan outlines that led to it
    // still name it, and a new sheet given its id would be led to by them.
    let mut used = Vec::new();
    for sheet in &set.sheets {
        used.push(number_after(&sheet.id, "s"));
        for viewport in &sheet.viewports {
            for mark in &viewport.marks {
                used.push(number_after(&mark.sheet, "s"));
            }
        }
    }
    ids_from("s", &fresh_numbers(used, count))
}

/// One viewport id, as new_viewport_ids gives it.
pub fn next_viewport_id(set: &SheetSet) -> String {
    new_viewport_ids(set, 1).remove(0)
}

/// One sheet id, as new_sheet_ids gives it.
pub fn next_sheet_id(set: &SheetSet) -> String {
    new_sheet_ids(set, 1).remove(0)
}

/// The index of the sheet with id `id`, if there is one.
pub fn sheet_index(set: &SheetSet, id: &str) -> Option<usize> {
    set.sheets
        .iter()
        .position(|sheet| !id.is_empty() && sheet.id == id)
}

/// What a mark says on paper: its label, and for a mark that refers to a sheet
/// " - SEE SHEET " and that sheet's number as the set numbers it now. A key
/// plan's outline is labelled with the number alone.
pub fn mark_label(set: &SheetSet, mark: &WorldMark) -> String {
    let Some(index) = sheet_index(set, &mark.sheet) else {
        return mark.label.clone();
    };
    let target = &set.sheets[index];
    let number = match target.fields.get("sheet_number") {
        Some(overridden) => overridden.clone(),
        None => format_sheet_number(
            &set.numbering,
            index + 1,
            set.sheets.len(),
            &set.defaults.set_number,
        ),
    };
    if mark.kind == WorldMarkKind::SheetOutline {
        return if mark.label.is_empty() {
            number
        } else {
            format!("{} {number}", mark.label)
        };
    }
    if mark.label.is_empty() {
        format!("SEE SHEET {number}")
    } else {
        format!("{} - SEE SHEET {number}", mark.label)
    }
}
