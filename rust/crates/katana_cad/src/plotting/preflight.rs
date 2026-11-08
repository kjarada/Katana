//! Preflight: what would go wrong on paper, found before the paper is used
//! (docs/plotting.md, "Preflight checks"; include/katana/cad/plotting/preflight.hpp).
//!
//! check_sheets reads a sheet set, the drawing and the project's field
//! values, and lists what a plot would get wrong: a view off the paper or
//! under the title block, two views on top of each other, a plan over
//! nothing, a section of an alignment that is not there or at a chainage off
//! its end, text too small to read at the view's scale, a title block with
//! blanks in it, a match line leading to a sheet that was removed. Each
//! finding carries a stable code, a severity, where it is (sheet and viewport
//! ids), what is wrong in words, and what to do about it.
//!
//! It changes nothing and plots nothing: the sheet editor lists the findings
//! in its Checks dock and warns before a plot, and an agent reads them as
//! JSON (findings_to_json). A plot is never refused because of them: an
//! error here is paper wasted, not a file that cannot be written.
//!
//! Pure: the same set, drawing and options give the same findings in the
//! same order: the set's own findings first, then sheet by sheet, each
//! sheet's own before its viewports', back to front, each viewport's checks in
//! the order preflight_checks() lists them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use katana_core::civil_date::today_utc;
use katana_core::error::make_error;
use katana_core::json::{Json, JsonResult};
use katana_core::text::{format_exact_real, format_general};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, EnumName, Geometry, Model};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{Box2, Point2, Polyline2, SpatialIndex, Vec2};
use katana_math::cpp;

use crate::dimension_draw::resolve_dimension_style;
use crate::document::Document;
use crate::hatching::resolve_hatch_pattern;
use crate::plot::{SHEET_SCALES, paper_dimensions, sheet_scale_at_least};
use crate::plotting::frame::{Frame, frame_for};
use crate::plotting::key_plan::{KeyPlanOutline, fit_key_plan, key_plan_outlines, model_placer};
use crate::plotting::layout::minimum_size;
use crate::plotting::legend::{
    LegendMetrics, LegendOptions, PlanResolver, compute_legend, layout_legend,
};
use crate::plotting::page_setup::validate_page_setup;
use crate::plotting::plan_grid::{
    GRID_MINIMUM_SPACING_MM, PlanGridOptions, PlanPlacement, plan_grid,
};
use crate::plotting::sheet_commands::{field_context_for, frame_date, logo_path};
use crate::plotting::sheet_set::{
    FieldContext, GridStyle, PlanWindow, Sheet, SheetSet, Viewport, ViewportKind, ViewportSource,
    WorldMarkKind, automatic_title, drawing_area, resolve_fields, sheet_index, std_fixed,
};
use crate::plotting::tables::{estimate_text_width, layout_viewport_table};
use crate::selection::{drawn_extent, is_drawn};
use crate::spatial_query::{for_each_candidate, query_extents};
use crate::terrain::surface_plan::{SurfaceStyleProblem, surface_style_problems};

/// How much a finding matters. An Error wastes the sheet (something is not
/// drawn, is cut off or is covered); a Warning is probably not what was
/// meant; Info is worth knowing. Declared worst first, so sorting by it puts
/// errors at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// "error", "warning", "info": the C++ `toString(Severity)`.
impl EnumName for Severity {
    fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

pub fn severity_from(name: &str) -> Option<Severity> {
    [Severity::Error, Severity::Warning, Severity::Info]
        .into_iter()
        .find(|severity| severity.name() == name)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// Stable: "viewport.outside", "field.empty" ... (preflight_checks lists
    /// them all). Never renamed: an agent and a saved report key on it.
    pub code: String,
    /// The sheet, by position and id; nothing for a finding about the whole
    /// set (a blank title-block value every sheet prints, the logo).
    pub sheet_index: Option<usize>,
    pub sheet_id: String,
    /// The viewport, when the finding is about one.
    pub viewport_id: String,
    /// What else the finding names: the field ("organisation"), the other
    /// viewport of an overlap, the sheet a mark leads to, the alignment.
    pub subject: String,
    /// What is wrong, in words.
    pub message: String,
    /// What to do about it.
    pub fix: String,
}

impl Default for Finding {
    fn default() -> Self {
        Self {
            severity: Severity::Warning,
            code: String::new(),
            sheet_index: None,
            sheet_id: String::new(),
            viewport_id: String::new(),
            subject: String::new(),
            message: String::new(),
            fix: String::new(),
        }
    }
}

/// One line of the catalogue: a check's code, the worst severity it reports,
/// and what it looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckDescription {
    pub code: &'static str,
    pub severity: Severity,
    pub looks_for: &'static str,
}

const fn check(
    code: &'static str,
    severity: Severity,
    looks_for: &'static str,
) -> CheckDescription {
    CheckDescription {
        code,
        severity,
        looks_for,
    }
}

static CHECKS: [CheckDescription; 34] = [
    // The set's own: what every sheet shares.
    check(
        "field.empty",
        Severity::Warning,
        "a value the frame prints that resolves to nothing: the organisation, a sign-off name, \
         the height datum, the coordinate system... listed once for the set",
    ),
    check(
        "logo.missing",
        Severity::Info,
        "no logo in the title block's logo slot",
    ),
    check(
        "logo.unreadable",
        Severity::Warning,
        "a logo named that is not in the project or cannot be read",
    ),
    check(
        "pagesetup.invalid",
        Severity::Error,
        "a page setup the plot refuses: a resolution or line weight scale out of range, or a \
         file-name pattern that cannot be expanded",
    ),
    // Each sheet's own.
    check(
        "sheet.duplicate-id",
        Severity::Error,
        "a sheet with no id, or the id of an earlier sheet",
    ),
    check(
        "sheet.duplicate-name",
        Severity::Warning,
        "a sheet with the name of an earlier sheet",
    ),
    check(
        "frame.unknown",
        Severity::Error,
        "a frame this build does not have: no title block prints",
    ),
    check(
        "portrait.no-frame",
        Severity::Warning,
        "a portrait sheet asking for a frame, which is laid out for landscape paper",
    ),
    check("sheet.empty", Severity::Warning, "a sheet with no views"),
    // Each viewport's, in this order.
    check(
        "viewport.duplicate-id",
        Severity::Error,
        "a viewport with no id, or the id of an earlier one",
    ),
    check(
        "viewport.unplaced",
        Severity::Warning,
        "a viewport not placed on the paper, which does not print",
    ),
    check(
        "viewport.outside",
        Severity::Error,
        "a viewport off the paper or under the title block (an error), or past the drawing area \
         into the margin (a warning)",
    ),
    check(
        "viewport.overlap",
        Severity::Warning,
        "a viewport covering part of an earlier one (a panel set inside a view is an inset: info)",
    ),
    check(
        "viewport.too-small",
        Severity::Info,
        "a viewport smaller than the smallest its kind reads at",
    ),
    check(
        "scale.invalid",
        Severity::Error,
        "a scale that is not a positive number",
    ),
    check(
        "scale.non-standard",
        Severity::Info,
        "a fixed scale that is not on the sheet scale ladder",
    ),
    check(
        "plan.alignment-missing",
        Severity::Warning,
        "a plan following an alignment the drawing does not have",
    ),
    check(
        "plan.empty",
        Severity::Warning,
        "a plan whose window - at its scale, about its centre, turned by its rotation - holds \
         nothing it draws",
    ),
    check(
        "plan.surface-style",
        Severity::Warning,
        "a plan whose window reaches a surface that its style cannot be drawn in whole: its \
         contours are too fine to trace, so they are missing from the plot",
    ),
    check(
        "text.too-small",
        Severity::Warning,
        "drawing text in a plan that prints smaller than the minimum at the plan's scale",
    ),
    check(
        "grid.invalid",
        Severity::Error,
        "a coordinate grid that cannot be drawn: its lines would print closer than the least \
         spacing",
    ),
    check(
        "grid.empty",
        Severity::Info,
        "a coordinate grid whose interval is wider than the plan's window",
    ),
    check(
        "section.alignment-missing",
        Severity::Error,
        "a section with no alignment, or naming one the drawing does not have",
    ),
    check(
        "section.alignment-invalid",
        Severity::Error,
        "a section of an alignment that cannot be solved",
    ),
    check(
        "section.no-stations",
        Severity::Error,
        "cross sections with no chainage to cut at",
    ),
    check(
        "section.station-outside",
        Severity::Error,
        "a cross section's chainage off the alignment (an error), or a long section's range \
         running past its end (a warning)",
    ),
    check(
        "section.no-surface",
        Severity::Error,
        "a section with no surface to cut (nor a design profile)",
    ),
    check(
        "image.missing",
        Severity::Error,
        "an image panel naming no file, or one the project does not have",
    ),
    check("notes.empty", Severity::Info, "a notes panel with no text"),
    check(
        "legend.empty",
        Severity::Info,
        "a legend whose plans show nothing, so it lists nothing",
    ),
    check(
        "legend.overflow",
        Severity::Warning,
        "a legend with more entries than fit, which prints \"+N more\" for the rest",
    ),
    check(
        "revisions.empty",
        Severity::Info,
        "a revision table on a set with no revisions",
    ),
    check(
        "table.overflow",
        Severity::Warning,
        "a drawing register or revision table whose rows do not all fit",
    ),
    check(
        "matchline.dangling",
        Severity::Warning,
        "a match line or key-plan outline leading to a sheet that no longer exists",
    ),
];

/// Every check, in the order a viewport's findings come in.
pub fn preflight_checks() -> &'static [CheckDescription] {
    &CHECKS
}

#[derive(Clone)]
pub struct PreflightOptions<'a> {
    /// The sheets to check, by index; every sheet when empty. The set's own
    /// checks (the title block's blanks, the logo) count only these sheets.
    pub sheets: Vec<usize>,
    /// Drawing text printing smaller than this, in paper millimetres, is too
    /// small to read: 1.8 mm is the smallest lettering a drafting standard
    /// allows on a sheet that may be copied or reduced.
    pub minimum_text_mm: f64,
    /// Two viewports overlap when they share more than this much on both
    /// axes: edges snapped together meet exactly and are not an overlap.
    pub overlap_tolerance_mm: f64,
    /// How far a viewport may stray past the drawing area before it is
    /// outside it: the millimetre's rounding a hand-drawn rectangle has.
    pub outside_tolerance_mm: f64,
    /// How an automatic plan's scale and centre are decided. The painter's
    /// own rule when the caller has it (the Qt layer passes
    /// resolvePlanViewport, which counts imagery and meshes too); plan_window
    /// otherwise.
    pub resolve_plan: Option<&'a PlanResolver<'a>>,
    /// World boxes of what a plan draws besides entities and alignments
    /// (imagery, point clouds, meshes), so a plan over nothing but an
    /// orthophoto is not called empty.
    pub other_content: Vec<Box2>,
    /// How many surfaces the sections are cut from; nothing when not known,
    /// and then the sections' surfaces are not checked.
    pub section_surfaces: Option<usize>,
    /// The visible surfaces whose styles cannot be drawn in whole, each with
    /// where it lies (surface_style_problems): what a plan whose window
    /// reaches one will be without. Nothing when not known, and then no
    /// surface is checked; empty when every style can be drawn.
    pub surface_problems: Option<Vec<SurfaceStyleProblem>>,
    /// Whether the set's logo file was found and read; nothing when not known.
    pub logo_readable: Option<bool>,
    /// Where an Image viewport's file is looked for; empty: not looked for.
    pub assets: PathBuf,
    /// A spatial index in step with the model, to find a plan's entities
    /// without scanning the drawing; None scans it.
    pub index: Option<&'a SpatialIndex>,
    /// Codes not to report.
    pub skip: BTreeSet<String>,
    /// The set as the painter draws it (every automatic scale decided), for
    /// laying a drawing register out as it plots: its SCALE column is part of
    /// what must fit. None: the set checked, its stored scales. The Qt layer
    /// passes resolvedSheetSet.
    pub drawn_set: Option<&'a SheetSet>,
}

impl Default for PreflightOptions<'_> {
    fn default() -> Self {
        Self {
            sheets: Vec::new(),
            minimum_text_mm: 1.8,
            overlap_tolerance_mm: 0.5,
            outside_tolerance_mm: 0.05,
            resolve_plan: None,
            other_content: Vec::new(),
            section_surfaces: None,
            surface_problems: None,
            logo_readable: None,
            assets: PathBuf::new(),
            index: None,
            skip: BTreeSet::new(),
            drawn_set: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreflightSummary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

// A viewport as a person finds it: its title as printed, and its id.
fn label_of(viewport: &Viewport) -> String {
    let mut title = if viewport.title.is_empty() {
        automatic_title(viewport)
    } else {
        viewport.title.clone()
    };
    if title.is_empty() {
        title = String::from("IMAGE");
    }
    format!("{title} ({})", viewport.id)
}

fn mm(value: f64) -> String {
    format!("{} mm", std_fixed(value, 1))
}

fn chainage(value: f64) -> String {
    format!("CH {}", std_fixed(value, 3))
}

fn scale_name(denominator: f64) -> String {
    format!("1:{}", std_fixed(denominator, 0))
}

// The C++ find_first_not_of(" \t\r\n"): the blanks a typed value may carry.
fn is_blank_byte(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\r' | '\n')
}

fn trimmed_upper(text: &str) -> String {
    text.trim_matches(is_blank_byte).to_ascii_uppercase()
}

fn is_blank(text: &str) -> bool {
    text.chars().all(is_blank_byte)
}

fn is_scaled(kind: ViewportKind) -> bool {
    matches!(
        kind,
        ViewportKind::Plan
            | ViewportKind::KeyPlan
            | ViewportKind::LongSection
            | ViewportKind::CrossSections
    )
}

// A panel set inside a view on purpose (a legend or key plan in a plan's
// corner) is an inset, not a mistake.
fn is_panel(kind: ViewportKind) -> bool {
    matches!(
        kind,
        ViewportKind::Legend
            | ViewportKind::Notes
            | ViewportKind::Image
            | ViewportKind::KeyPlan
            | ViewportKind::SheetIndex
            | ViewportKind::Revisions
    )
}

// std::max({a, b, ...}): the first largest, by `<`, as std::max_element finds it.
fn largest(values: &[f64]) -> f64 {
    values[1..]
        .iter()
        .fold(values[0], |kept, &value| cpp::max(kept, value))
}

// ---- the title block's blanks -----------------------------------------------------------

// What each field the frame prints is called on the Title Block, and how much
// a blank matters. Nothing: a blank is expected (the optional project lines,
// the client, the notes). The order is the order findings come in.
struct FieldRule {
    field: &'static str,
    label: &'static str,
    severity: Option<Severity>,
    fix: &'static str,
}

const PROJECT_TAB: &str = "Fill it in on Title Block > Project";
const SIGN_OFF_TAB: &str = "Fill it in on Title Block > Sign-offs";

const fn rule(
    field: &'static str,
    label: &'static str,
    severity: Option<Severity>,
    fix: &'static str,
) -> FieldRule {
    FieldRule {
        field,
        label,
        severity,
        fix,
    }
}

static FIELD_RULES: [FieldRule; 27] = [
    rule(
        "organisation",
        "Organisation",
        Some(Severity::Warning),
        PROJECT_TAB,
    ),
    rule(
        "project_line_1",
        "Project line 1",
        Some(Severity::Warning),
        "Fill it in on Title Block > Project, or give the project a name",
    ),
    rule(
        "project_line_2",
        "Project line 2",
        Some(Severity::Info),
        "Fill it in on Title Block > Project, or give the project a description",
    ),
    rule("project_line_3", "Project line 3", None, PROJECT_TAB),
    rule("project_line_4", "Project line 4", None, PROJECT_TAB),
    rule("client", "Client", None, PROJECT_TAB),
    rule(
        "set_number",
        "Drawing set number",
        Some(Severity::Info),
        PROJECT_TAB,
    ),
    rule(
        "coordinate_system",
        "Coordinate system",
        Some(Severity::Warning),
        "Fill it in on Title Block > Project, or set the project's coordinate system",
    ),
    rule(
        "height_datum",
        "Height datum",
        Some(Severity::Warning),
        PROJECT_TAB,
    ),
    rule(
        "model_name",
        "Model name",
        Some(Severity::Info),
        PROJECT_TAB,
    ),
    rule(
        "locator_name",
        "Utilities located by",
        Some(Severity::Warning),
        SIGN_OFF_TAB,
    ),
    rule(
        "surveyor_name",
        "Surveyed by",
        Some(Severity::Warning),
        SIGN_OFF_TAB,
    ),
    rule(
        "compiler_name",
        "Compiled by",
        Some(Severity::Warning),
        SIGN_OFF_TAB,
    ),
    rule(
        "reviewer_name",
        "Reviewed by",
        Some(Severity::Warning),
        SIGN_OFF_TAB,
    ),
    rule(
        "approver_name",
        "Approved by",
        Some(Severity::Warning),
        SIGN_OFF_TAB,
    ),
    // A sign-off with no date of its own prints the plot date, so these are
    // blank only when that is, and plot_date says so once.
    rule("locator_date", "Utilities located on", None, SIGN_OFF_TAB),
    rule("surveyor_date", "Surveyed on", None, SIGN_OFF_TAB),
    rule("compiler_date", "Compiled on", None, SIGN_OFF_TAB),
    rule("reviewer_date", "Reviewed on", None, SIGN_OFF_TAB),
    rule("approver_date", "Approved on", None, SIGN_OFF_TAB),
    rule("notes", "Notes", None, "Fill it in on Title Block > Notes"),
    rule(
        "file_name",
        "File name",
        Some(Severity::Warning),
        "Save the drawing as a project, or give it a name: the file name is the project folder's",
    ),
    rule(
        "sheet_number",
        "Sheet number",
        Some(Severity::Warning),
        "Set the numbering on Title Block > Project > Sheet numbering",
    ),
    rule(
        "sheet_count",
        "Sheet count",
        Some(Severity::Warning),
        "Clear the sheet's own value for it",
    ),
    rule(
        "scale",
        "Scale",
        Some(Severity::Warning),
        "Clear the sheet's own value for it",
    ),
    rule(
        "plot_date",
        "Plot date",
        Some(Severity::Warning),
        "Plot with a date",
    ),
    rule(
        "revision",
        "Revision",
        Some(Severity::Info),
        "Add a revision on Title Block > Revisions",
    ),
];

fn rule_for(field: &str) -> Option<&'static FieldRule> {
    FIELD_RULES.iter().find(|rule| rule.field == field)
}

// ---- a plan's window on the world -------------------------------------------------------

// The world rectangle a plan viewport shows, in its own axes: `u` runs across
// the paper, `v` up it, each half-size in world units.
struct Window {
    centre: Point2,
    u: Vec2,
    v: Vec2,
    half_width: f64,
    half_height: f64,
}

impl Window {
    fn local(&self, world: Point2) -> Point2 {
        let d = world - self.centre;
        Point2::new(d.dot(self.u), d.dot(self.v))
    }

    fn contains_local(&self, q: Point2) -> bool {
        q.x.abs() <= self.half_width && q.y.abs() <= self.half_height
    }

    fn corners(&self) -> [Point2; 4] {
        let a = self.u * self.half_width;
        let b = self.v * self.half_height;
        [
            self.centre - a - b,
            self.centre + a - b,
            self.centre + a + b,
            self.centre - a + b,
        ]
    }

    fn reach(&self) -> Box2 {
        let mut reach = Box2::default();
        for corner in self.corners() {
            reach.expand(corner);
        }
        reach
    }
}

fn window_of(viewport: &Viewport, at: &PlanWindow) -> Window {
    let u = Vec2::new(viewport.rotation.cos(), viewport.rotation.sin());
    Window {
        centre: at.centre,
        u,
        v: Vec2::new(-u.y, u.x),
        half_width: viewport.rect.width() * at.scale / 2000.0,
        half_height: viewport.rect.height() * at.scale / 2000.0,
    }
}

// Whether the segment a-b passes through the window: clipped against it in
// the window's axes (Liang-Barsky).
fn segment_meets(window: &Window, a: Point2, b: Point2) -> bool {
    let p = window.local(a);
    let q = window.local(b);
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let d = q - p;
    let planes = [
        (-d.x, p.x + window.half_width),
        (d.x, window.half_width - p.x),
        (-d.y, p.y + window.half_height),
        (d.y, window.half_height - p.y),
    ];
    for (denominator, distance) in planes {
        if denominator == 0.0 {
            if distance < 0.0 {
                return false;
            }
            continue;
        }
        let t = distance / denominator;
        if denominator < 0.0 {
            t0 = cpp::max(t0, t);
        } else {
            t1 = cpp::min(t1, t);
        }
        if t0 > t1 {
            return false;
        }
    }
    true
}

// Whether a world box and the window share any point: the separating-axis
// test on the two rectangles' four axes.
fn box_meets(window: &Window, area: &Box2) -> bool {
    if area.empty() {
        return false;
    }
    if !area.intersects(window.reach()) {
        return false;
    }
    let mut local = Box2::default();
    for corner in [
        area.min,
        area.max,
        Point2::new(area.min.x, area.max.y),
        Point2::new(area.max.x, area.min.y),
    ] {
        local.expand(window.local(corner));
    }
    local.intersects(Box2::new(
        Point2::new(-window.half_width, -window.half_height),
        Point2::new(window.half_width, window.half_height),
    ))
}

// `filled`: the outline is hatched, so a window wholly inside it looks at its
// fill; an outline that is not shows nothing inside it.
fn polyline_meets(window: &Window, line: &Polyline2, filled: bool) -> bool {
    let vertices = &line.vertices;
    if vertices.len() == 1 {
        return window.contains_local(window.local(vertices[0]));
    }
    for pair in vertices.windows(2) {
        if segment_meets(window, pair[0], pair[1]) {
            return true;
        }
    }
    if line.closed && vertices.len() > 2 {
        if segment_meets(window, vertices[vertices.len() - 1], vertices[0]) {
            return true;
        }
        // A window wholly inside a hatched outline is looking at its fill:
        // counted as shown, so a hatched area is never called empty. Inside a
        // bare outline (a boundary, a buffer) there is nothing to see.
        if !filled {
            return false;
        }
        let mut inside = false;
        let c = window.centre;
        let mut j = vertices.len() - 1;
        for (i, &a) in vertices.iter().enumerate() {
            let b = vertices[j];
            if (a.y > c.y) != (b.y > c.y) && c.x < (b.x - a.x) * (c.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
            j = i;
        }
        return inside;
    }
    false
}

// Whether an entity's drawing reaches into the window. Exact for points,
// lines and polylines; a closed outline or a circle counts when it crosses
// the window, or when the window is inside it and it is hatched (its fill);
// a dimension by everything it draws (its label and arrows reach well past
// the points it measures: query_extents); arcs, text and anything else by
// their box.
fn entity_meets(window: &Window, model: &Model, entity: &Entity) -> bool {
    match &entity.geometry {
        Geometry::Point(point) => window.contains_local(window.local(point.position)),
        Geometry::Line(segment) => segment_meets(window, segment.start, segment.end),
        Geometry::Polyline(line) => polyline_meets(
            window,
            line,
            line.closed && resolve_hatch_pattern(model, entity).is_some(),
        ),
        Geometry::Circle(circle) => {
            let c = window.local(circle.center);
            let dx = cpp::max(c.x.abs() - window.half_width, 0.0);
            let dy = cpp::max(c.y.abs() - window.half_height, 0.0);
            if dx.hypot(dy) > circle.radius {
                return false; // the window is wholly outside the circle
            }
            // The farthest corner of the window from the centre: inside the
            // circle, the window sees only its fill.
            let fx = c.x.abs() + window.half_width;
            let fy = c.y.abs() + window.half_height;
            fx.hypot(fy) >= circle.radius || resolve_hatch_pattern(model, entity).is_some()
        }
        Geometry::Dimension(_) => box_meets(window, &query_extents(model, entity)),
        geometry => box_meets(window, &bounding_box(geometry)),
    }
}

// ---- everything else --------------------------------------------------------------------

// Where a finding is: the sheet by position and id, and the viewport; the
// set's own findings have none of them.
#[derive(Clone, Copy)]
struct Place<'p> {
    sheet_index: Option<usize>,
    sheet_id: &'p str,
    viewport_id: &'p str,
}

const THE_SET: Place<'static> = Place {
    sheet_index: None,
    sheet_id: "",
    viewport_id: "",
};

// The findings, and the codes not to report.
struct Report<'o> {
    skip: &'o BTreeSet<String>,
    findings: Vec<Finding>,
}

impl Report<'_> {
    fn add(
        &mut self,
        place: Place<'_>,
        severity: Severity,
        code: &str,
        subject: String,
        message: String,
        fix: String,
    ) {
        if self.skip.contains(code) {
            return;
        }
        self.findings.push(Finding {
            severity,
            code: String::from(code),
            sheet_index: place.sheet_index,
            sheet_id: String::from(place.sheet_id),
            viewport_id: String::from(place.viewport_id),
            subject,
            message,
            fix,
        });
    }
}

// The sheets a check covers: those asked for that exist, in the set's order,
// each once.
fn sheets_to_check(set: &SheetSet, options: &PreflightOptions<'_>) -> Vec<usize> {
    if options.sheets.is_empty() {
        return (0..set.sheets.len()).collect();
    }
    let mut indices: Vec<usize> = options
        .sheets
        .iter()
        .copied()
        .filter(|&index| index < set.sheets.len())
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
}

type Frames = BTreeMap<String, Result<Frame>>;

// The frame a sheet prints with, when it prints one.
fn printed_frame<'f>(sheet: &Sheet, frames: &'f mut Frames) -> Option<&'f Frame> {
    if sheet.frame.is_empty() || !sheet.landscape {
        return None;
    }
    // Any key that tells the frame and the paper apart: the C++ writes the
    // paper's enumerator as a number.
    let key = format!("{}|{:?}", sheet.frame, sheet.paper);
    frames
        .entry(key)
        .or_insert_with(|| frame_for(&sheet.frame, sheet.paper, true))
        .as_ref()
        .ok()
}

fn emit_blank(
    report: &mut Report<'_>,
    framed: usize,
    blank: (&str, &str),
    severity: Severity,
    fix: &str,
    count: usize,
) {
    let (field, label) = blank;
    let place = if count == framed {
        String::from(if framed == 1 {
            "the sheet"
        } else {
            "every sheet"
        })
    } else {
        format!("{count} of {framed} sheets")
    };
    report.add(
        THE_SET,
        severity,
        "field.empty",
        String::from(field),
        format!("{label} is blank in the title block of {place}"),
        String::from(fix),
    );
}

fn check_fields(
    set: &SheetSet,
    context: &FieldContext,
    indices: &[usize],
    frames: &mut Frames,
    report: &mut Report<'_>,
) {
    // Per field: on how many framed sheets it prints blank.
    let mut blanks: BTreeMap<String, usize> = BTreeMap::new();
    let mut framed = 0;
    for &index in indices {
        let sheet = &set.sheets[index];
        let Some(frame) = printed_frame(sheet, frames) else {
            continue;
        };
        framed += 1;
        let mut printed: BTreeSet<String> = BTreeSet::new();
        for text in &frame.texts {
            printed.extend(text.fields.iter().cloned());
        }
        let values = resolve_fields(set, index, context);
        for field in &printed {
            // A value the sheet sets itself, even an empty one, is the user's
            // word on that sheet: a blank there is on purpose.
            if sheet.fields.contains_key(field) {
                continue;
            }
            if values.get(field).is_none_or(|value| is_blank(value)) {
                *blanks.entry(field.clone()).or_insert(0) += 1;
            }
        }
    }
    // The known fields in the Title Block's order, then any other the frame
    // prints, by name.
    for rule in &FIELD_RULES {
        if let (Some(&count), Some(severity)) = (blanks.get(rule.field), rule.severity) {
            emit_blank(
                report,
                framed,
                (rule.field, rule.label),
                severity,
                rule.fix,
                count,
            );
        }
    }
    for (field, &count) in &blanks {
        if rule_for(field).is_none() {
            emit_blank(
                report,
                framed,
                (field, &format!("The field {field}")),
                Severity::Warning,
                "Fill it in on Title Block, or on the sheet's own title-block values",
                count,
            );
        }
    }
}

fn check_logo(
    set: &SheetSet,
    indices: &[usize],
    frames: &mut Frames,
    options: &PreflightOptions<'_>,
    report: &mut Report<'_>,
) {
    let framed = indices.iter().any(|&index| {
        printed_frame(&set.sheets[index], frames).is_some_and(|frame| frame.cell("logo").is_some())
    });
    if !framed {
        return;
    }
    let logo = &set.defaults.logo_asset;
    if logo.is_empty() {
        report.add(
            THE_SET,
            Severity::Info,
            "logo.missing",
            String::new(),
            String::from("The title block's logo slot is empty"),
            String::from(
                "Choose your organisation's logo on Title Block > Logo (save the project first: \
                 the logo is kept in it)",
            ),
        );
    } else if options.logo_readable == Some(false) {
        report.add(
            THE_SET,
            Severity::Warning,
            "logo.unreadable",
            logo.clone(),
            format!(
                "The logo {logo} is not in the project's assets folder or cannot be read: its \
                 slot prints empty"
            ),
            String::from("Choose the logo again on Title Block > Logo"),
        );
    }
}

fn check_sheet_itself(set: &SheetSet, index: usize, frames: &mut Frames, report: &mut Report<'_>) {
    let sheet = &set.sheets[index];
    let place = Place {
        sheet_index: Some(index),
        sheet_id: &sheet.id,
        viewport_id: "",
    };

    if sheet.id.is_empty() {
        report.add(
            place,
            Severity::Error,
            "sheet.duplicate-id",
            String::new(),
            String::from("The sheet has no id: nothing can lead to it"),
            String::from(
                "Remove the sheet and add it again, or duplicate it: a new sheet gets an id",
            ),
        );
    } else if let Some(other) = set.sheets[..index]
        .iter()
        .position(|other| other.id == sheet.id)
    {
        report.add(
            place,
            Severity::Error,
            "sheet.duplicate-id",
            set.sheets[other].id.clone(),
            format!(
                "The sheet has the same id, {}, as sheet {}: match lines lead to the first of \
                 them",
                sheet.id,
                other + 1
            ),
            String::from("Duplicate the sheet and remove this one: the copy gets an id of its own"),
        );
    }

    let name = trimmed_upper(&sheet.name);
    if !name.is_empty()
        && let Some(other) = set.sheets[..index]
            .iter()
            .position(|other| trimmed_upper(&other.name) == name)
    {
        report.add(
            place,
            Severity::Warning,
            "sheet.duplicate-name",
            set.sheets[other].id.clone(),
            format!(
                "The sheet has the same name as sheet {}: {}",
                other + 1,
                sheet.name
            ),
            String::from("Rename one of them: a sheet is found by its name"),
        );
    }

    if !sheet.frame.is_empty() && sheet.landscape && printed_frame(sheet, frames).is_none() {
        report.add(
            place,
            Severity::Error,
            "frame.unknown",
            sheet.frame.clone(),
            format!(
                "The sheet asks for the frame {}, which this build does not have: it prints \
                 without a title block",
                sheet.frame
            ),
            String::from("Choose the built-in frame in the sheet's properties"),
        );
    }
    if !sheet.frame.is_empty() && !sheet.landscape {
        report.add(
            place,
            Severity::Warning,
            "portrait.no-frame",
            sheet.frame.clone(),
            String::from(
                "The sheet is portrait, so it prints without its title block: the frame is laid \
                 out for landscape paper",
            ),
            String::from("Make the sheet landscape, or turn its frame off"),
        );
    }
    if sheet.viewports.is_empty() {
        report.add(
            place,
            Severity::Warning,
            "sheet.empty",
            String::new(),
            String::from(if sheet.frame.is_empty() || !sheet.landscape {
                "The sheet has no views: it prints a blank page"
            } else {
                "The sheet has no views: it prints an empty frame"
            }),
            String::from("Add a view, or remove the sheet"),
        );
    }
}

// What a plan viewport shows once its window is known: whether anything it
// draws is in the window, and its text too small to read.
#[derive(Default)]
struct PlanContent {
    shown: bool,
    small_texts: usize,
    // Model units.
    smallest_model: f64,
}

fn plan_content(
    viewport: &Viewport,
    window: &Window,
    scale: f64,
    model: &Model,
    options: &PreflightOptions<'_>,
) -> PlanContent {
    let mut content = PlanContent::default();
    let minimum_model = options.minimum_text_mm * scale / 1000.0;
    let mut scratch = Vec::new();
    for_each_candidate(model, options.index, window.reach(), &mut scratch, |e| {
        if !is_drawn(model, e, &viewport.hidden_layers) || !entity_meets(window, model, e) {
            return;
        }
        content.shown = true;
        let height = match &e.geometry {
            Geometry::Text(text) => {
                if is_blank(&text.text) {
                    return;
                }
                text.height
            }
            Geometry::Dimension(_) => resolve_dimension_style(model, e).text_height,
            _ => return,
        };
        if height < minimum_model {
            content.smallest_model = if content.small_texts == 0 {
                height
            } else {
                cpp::min(content.smallest_model, height)
            };
            content.small_texts += 1;
        }
    });
    if !content.shown {
        model.alignments.for_each(|alignment| {
            if content.shown {
                return;
            }
            if let Ok(solved) = solve_alignment(&alignment.horizontal) {
                content.shown = polyline_meets(window, &solved.to_polyline(0.01), false);
            }
        });
    }
    if !content.shown {
        content.shown = options
            .other_content
            .iter()
            .any(|area| box_meets(window, area));
    }
    // A plan's match lines are drawn too.
    if !content.shown {
        content.shown = viewport.marks.iter().any(|mark| {
            // A key plan's stored outlines are stale copies the painter no
            // longer draws; its live ones are counted by check_plan.
            if viewport.kind == ViewportKind::KeyPlan && mark.kind == WorldMarkKind::SheetOutline {
                return false;
            }
            // Its lines only: an outline is not filled.
            let mut line = Polyline2::new(mark.points.clone(), false);
            if mark.kind == WorldMarkKind::SheetOutline && line.vertices.len() > 2 {
                line.vertices.push(line.vertices[0]);
            }
            !line.vertices.is_empty() && polyline_meets(window, &line, false)
        });
    }
    content
}

fn check_plan(
    set: &SheetSet,
    viewport: &Viewport,
    place: Place<'_>,
    model: &Model,
    options: &PreflightOptions<'_>,
    report: &mut Report<'_>,
) {
    let label = label_of(viewport);
    let alignment = &viewport.source.alignment;
    if !alignment.is_empty() && model.alignments.find(alignment).is_none() {
        report.add(
            place,
            Severity::Warning,
            "plan.alignment-missing",
            alignment.clone(),
            format!("{label} follows the alignment {alignment}, which the drawing does not have"),
            String::from(
                "Generate the sheet again from an alignment the drawing has, or clear the view's \
                 alignment",
            ),
        );
    }
    let mut at = match options.resolve_plan {
        Some(resolve) => resolve(viewport),
        None => plan_window(viewport, model, &options.other_content),
    };
    // A key plan is drawn live (key_plan.rs): the sheets' plans outlined where
    // they are now, each automatic one placed as the painter places it, and
    // an automatic key plan fitted to those outlines rather than to the
    // drawing, so it is checked at the window it is drawn at, and what it
    // shows includes the outlines.
    let mut outlines: Vec<KeyPlanOutline> = Vec::new();
    if viewport.kind == ViewportKind::KeyPlan {
        let model_place = model_placer(model);
        let placer = |plan: &Viewport| -> PlanPlacement {
            match options.resolve_plan {
                Some(resolve) => {
                    let placed = resolve(plan);
                    PlanPlacement {
                        scale: placed.scale,
                        centre: placed.centre,
                    }
                }
                None => model_place(plan),
            }
        };
        outlines = key_plan_outlines(set, place.sheet_index.unwrap_or(0), Some(&placer));
        if !outlines.is_empty() && (viewport.auto_scale || viewport.auto_centre) {
            let fitted = fit_key_plan(viewport, &outlines, &Box2::default());
            at = PlanWindow {
                scale: fitted.scale,
                centre: fitted.centre,
            };
        }
    }
    if !(at.scale > 0.0)
        || !at.scale.is_finite()
        || !at.centre.x.is_finite()
        || !at.centre.y.is_finite()
    {
        // A fixed scale that cannot be used has had scale.invalid. An
        // automatic one falls back to the stored scale and centre when there
        // is nothing to fit, and those cannot be used either: say so here,
        // since scale.invalid looks only at fixed scales.
        if viewport.auto_scale {
            report.add(
                place,
                Severity::Error,
                "scale.invalid",
                String::new(),
                format!(
                    "{label} has nothing to fit its automatic scale to, and its stored scale ({}) \
                     cannot be used",
                    format_exact_real(at.scale)
                ),
                String::from("Give it a scale, or draw something for it to show"),
            );
        }
        return;
    }
    let window = window_of(viewport, &at);
    let mut content = plan_content(viewport, &window, at.scale, model, options);
    if !content.shown {
        content.shown = outlines.iter().any(|outline| {
            let mut ring = Polyline2::new(outline.corners.clone(), false);
            if !ring.vertices.is_empty() {
                ring.vertices.push(ring.vertices[0]);
            }
            ring.vertices.len() > 1 && polyline_meets(&window, &ring, false)
        });
    }
    if !content.shown {
        report.add(
            place,
            Severity::Warning,
            "plan.empty",
            String::new(),
            format!(
                "{label} shows nothing: nothing it draws lies in its {} x {} m window at {} about \
                 ({}, {})",
                std_fixed(2.0 * window.half_width, 1),
                std_fixed(2.0 * window.half_height, 1),
                scale_name(at.scale),
                std_fixed(at.centre.x, 3),
                std_fixed(at.centre.y, 3)
            ),
            String::from(
                "Pan the drawing into it (Shift-drag), set its scale and centre to Auto, or show \
                 the layers it hides",
            ),
        );
    }
    // A surface in the window whose style the painter cannot draw in whole:
    // it paints the plan without those contours and says so only in the
    // sheet's problems, so the plot is warned about here, before the paper.
    // The window's bounding box against the surface's, as the painter culls
    // a surface to the box the frame shows (a surface the box misses draws,
    // and says, nothing).
    if let Some(problems) = &options.surface_problems {
        let reach = window.reach();
        for found in problems {
            if !found.bounds.intersects(reach) {
                continue;
            }
            report.add(
                place,
                Severity::Warning,
                "plan.surface-style",
                found.problem.surface.clone(),
                format!(
                    "{label} shows the surface {} (style {}) without part of its style: {}",
                    found.problem.surface, found.problem.style, found.problem.reason
                ),
                format!(
                    "Make the style's contour interval larger (SURFACESTYLE SET \"{}\" \
                     interval=..., or Terrain > Surface Styles), or give the surface another \
                     style (SURFACE STYLE \"{}\" <style>)",
                    found.problem.style, found.problem.surface
                ),
            );
        }
    }
    // The coordinate grid, worked out as the painter works it out at this
    // window (plan_grid.rs).
    if viewport.grid_style != GridStyle::None {
        let placement = PlanPlacement {
            scale: at.scale,
            centre: at.centre,
        };
        match plan_grid(viewport, &placement, &PlanGridOptions::default()) {
            Err(error) => report.add(
                place,
                Severity::Error,
                "grid.invalid",
                String::new(),
                format!("{label}'s coordinate grid is not drawn: {}", error.message),
                format!(
                    "Set its grid interval to Auto, or to at least {} m at {}",
                    format_general(GRID_MINIMUM_SPACING_MM * at.scale / 1000.0, 3),
                    scale_name(at.scale)
                ),
            ),
            Ok(grid) if grid.lines.is_empty() => report.add(
                place,
                Severity::Info,
                "grid.empty",
                String::new(),
                format!(
                    "{label}'s coordinate grid, every {} m, has no line in its window",
                    format_general(grid.interval, 6)
                ),
                String::from("Set its grid interval to Auto, or make it smaller"),
            ),
            Ok(_) => {}
        }
    }
    // A key plan is a small-scale map of where the sheets are: its drawing is
    // faded and its own text is not meant to be read.
    if content.small_texts > 0 && viewport.kind != ViewportKind::KeyPlan {
        let smallest_mm = content.smallest_model * 1000.0 / at.scale;
        // The largest standard scale at which the smallest text reaches the
        // minimum, and the text height that reads at this one.
        let ceiling = content.smallest_model * 1000.0 / options.minimum_text_mm;
        let mut readable: Option<f64> = None;
        for standard in SHEET_SCALES {
            if standard <= ceiling {
                readable = Some(standard);
            }
        }
        let height = format!(
            "{} m",
            format_general(options.minimum_text_mm * at.scale / 1000.0, 3)
        );
        let fix = match readable {
            Some(readable) => format!(
                "Plot the view at {} or larger, or make the text at least {height} high",
                scale_name(readable)
            ),
            None => format!("Make the text at least {height} high"),
        };
        report.add(
            place,
            Severity::Warning,
            "text.too-small",
            String::new(),
            format!(
                "{label} has {} text{} printing smaller than {} at {}, the smallest {} mm",
                content.small_texts,
                if content.small_texts == 1 { "" } else { "s" },
                mm(options.minimum_text_mm),
                scale_name(at.scale),
                std_fixed(smallest_mm, 2)
            ),
            fix,
        );
    }
}

// The chainages a cross-section viewport cuts, as the painter lists them.
fn cross_stations(from: &ViewportSource) -> Vec<f64> {
    let mut stations = from.stations.clone();
    if stations.is_empty() && from.section_interval > 0.0 && from.chainage_to > from.chainage_from {
        let mut at = from.chainage_from;
        while at <= from.chainage_to + 1e-9 && stations.len() < 64 {
            stations.push(at);
            at += from.section_interval;
        }
    }
    stations
}

fn check_section(
    viewport: &Viewport,
    place: Place<'_>,
    model: &Model,
    options: &PreflightOptions<'_>,
    report: &mut Report<'_>,
) {
    let label = label_of(viewport);
    let from = &viewport.source;
    let cross = viewport.kind == ViewportKind::CrossSections;
    if from.alignment.is_empty() {
        report.add(
            place,
            Severity::Error,
            "section.alignment-missing",
            String::new(),
            format!("{label} has no alignment: it prints empty"),
            String::from("Choose its alignment in the view's properties"),
        );
        return;
    }
    let Some(alignment) = model.alignments.find(&from.alignment) else {
        report.add(
            place,
            Severity::Error,
            "section.alignment-missing",
            from.alignment.clone(),
            format!(
                "{label} cuts the alignment {}, which the drawing does not have: it prints empty",
                from.alignment
            ),
            String::from("Choose another alignment in the view's properties, or remove the view"),
        );
        return;
    };
    let solved = match solve_alignment(&alignment.horizontal) {
        Ok(solved) => solved,
        Err(error) => {
            report.add(
                place,
                Severity::Error,
                "section.alignment-invalid",
                from.alignment.clone(),
                format!(
                    "{label} cuts the alignment {}, which cannot be solved: {}",
                    from.alignment,
                    error.describe()
                ),
                String::from("Mend the alignment's geometry"),
            );
            return;
        }
    };
    let runs = format!(
        "{} runs {} to {}",
        from.alignment,
        chainage(solved.start_station()),
        chainage(solved.end_station())
    );
    if cross {
        let stations = cross_stations(from);
        if stations.is_empty() {
            report.add(
                place,
                Severity::Error,
                "section.no-stations",
                from.alignment.clone(),
                format!("{label} has no chainage to cut a section at: it prints empty"),
                String::from(
                    "Give it chainages, or an interval and a range, in the view's properties",
                ),
            );
        } else {
            let half_width = if from.section_half_width > 0.0 {
                from.section_half_width
            } else {
                20.0
            };
            let off: Vec<f64> = stations
                .iter()
                .copied()
                .filter(|&station| {
                    solved
                        .point_at_station_offset(station, half_width)
                        .is_none()
                        || solved
                            .point_at_station_offset(station, -half_width)
                            .is_none()
                })
                .collect();
            if !off.is_empty() {
                let mut listed = String::new();
                for (i, &station) in off.iter().take(3).enumerate() {
                    if i > 0 {
                        listed.push_str(", ");
                    }
                    listed.push_str(&chainage(station));
                }
                if off.len() > 3 {
                    listed.push_str(&format!(" and {} more", off.len() - 3));
                }
                report.add(
                    place,
                    Severity::Error,
                    "section.station-outside",
                    from.alignment.clone(),
                    format!(
                        "{label} cuts {} of its {} section{} off the alignment ({}): they print \
                         blank. {}",
                        off.len(),
                        stations.len(),
                        if stations.len() == 1 { "" } else { "s" },
                        listed,
                        runs
                    ),
                    format!(
                        "Cut them between {} and {}",
                        chainage(solved.start_station()),
                        chainage(solved.end_station())
                    ),
                );
            }
        }
    } else if from.chainage_to > from.chainage_from {
        let start = solved.start_station();
        let end = solved.end_station();
        const SLACK: f64 = 1e-6;
        if from.chainage_from < start - SLACK || from.chainage_to > end + SLACK {
            let none = from.chainage_to <= start || from.chainage_from >= end;
            report.add(
                place,
                if none {
                    Severity::Error
                } else {
                    Severity::Warning
                },
                "section.station-outside",
                from.alignment.clone(),
                format!(
                    "{label} shows {} to {}, {}: {}{}",
                    chainage(from.chainage_from),
                    chainage(from.chainage_to),
                    if none {
                        "all of it off the alignment"
                    } else {
                        "running past its end"
                    },
                    runs,
                    if none {
                        "; it prints empty"
                    } else {
                        "; the rest is blank"
                    }
                ),
                format!(
                    "Set its range within {} to {}",
                    chainage(start),
                    chainage(end)
                ),
            );
        }
    }
    if options.section_surfaces == Some(0) && (cross || alignment.vertical.is_none()) {
        report.add(
            place,
            Severity::Error,
            "section.no-surface",
            from.alignment.clone(),
            if cross {
                format!("{label} has no surface to cut: it prints empty")
            } else {
                format!(
                    "{label} has no surface to cut, and {} has no design profile: it prints empty",
                    from.alignment
                )
            },
            String::from("Build a surface (Terrain > Surface From ...), or show a hidden one"),
        );
    }
}

// What two rectangles share, across and up.
fn shared(a: &Box2, b: &Box2) -> (f64, f64) {
    (
        cpp::min(a.max.x, b.max.x) - cpp::max(a.min.x, b.min.x),
        cpp::min(a.max.y, b.max.y) - cpp::max(a.min.y, b.min.y),
    )
}

// How far `inner` reaches past `outer` on its worst side, or 0.
fn beyond(inner: &Box2, outer: &Box2) -> f64 {
    largest(&[
        outer.min.x - inner.min.x,
        inner.max.x - outer.max.x,
        outer.min.y - inner.min.y,
        inner.max.y - outer.max.y,
        0.0,
    ])
}

fn check_table(
    set: &SheetSet,
    viewport: &Viewport,
    place: Place<'_>,
    options: &PreflightOptions<'_>,
    report: &mut Report<'_>,
) {
    // A table is laid out as the painter lays it out (tables.rs), so the rows
    // it leaves out are the rows the plot leaves out: a register that says
    // "+3 more" hands over a set whose last sheets it never lists.
    let label = label_of(viewport);
    let revisions = viewport.kind == ViewportKind::Revisions;
    if revisions && set.revisions.is_empty() {
        report.add(
            place,
            Severity::Info,
            "revisions.empty",
            String::new(),
            format!("{label} has no revisions to list: it prints its headings alone"),
            String::from("Add a revision on the Title Block, or remove the table"),
        );
        return;
    }
    let table = layout_viewport_table(
        options.drawn_set.unwrap_or(set),
        place.sheet_index.unwrap_or(0),
        viewport,
        None,
    );
    if table.rows_hidden > 0 {
        report.add(
            place,
            Severity::Warning,
            "table.overflow",
            String::new(),
            format!(
                "{label} shows {} of its {} rows: {} do not fit and print as \"+{} {}\"",
                table.rows_shown,
                table.rows_shown + table.rows_hidden,
                table.rows_hidden,
                table.rows_hidden,
                if revisions { "earlier" } else { "more" }
            ),
            String::from(if revisions {
                "Make it taller, or show only the newest revisions"
            } else {
                "Make it larger, or give it a sheet of its own"
            }),
        );
    }
}

fn check_legend(
    set: &SheetSet,
    viewport: &Viewport,
    place: Place<'_>,
    model: &Model,
    options: &PreflightOptions<'_>,
    report: &mut Report<'_>,
) {
    // Gathered and flowed as the painter gathers and flows it (legend.rs),
    // the labels measured by Arial's widths as the tables are.
    let label = label_of(viewport);
    let gather = LegendOptions {
        index: options.index,
        window: options.resolve_plan,
        ..LegendOptions::default()
    };
    let Ok(legend) = compute_legend(
        model,
        set,
        place.sheet_index.unwrap_or(0),
        viewport.legend_scope,
        &gather,
    ) else {
        return;
    };
    if legend.entries.is_empty() {
        report.add(
            place,
            Severity::Info,
            "legend.empty",
            String::from(viewport.legend_scope.name()),
            format!(
                "{label} lists nothing: the plans it reads ({}) show nothing",
                viewport.legend_scope.name()
            ),
            String::from("Widen its scope to the whole set or drawing, or remove it"),
        );
        return;
    }
    let widths: Vec<f64> = legend
        .entries
        .iter()
        .map(|entry| estimate_text_width(&entry.label.to_ascii_uppercase(), 1.8, false) * 0.9)
        .collect();
    let flowed = layout_legend(&viewport.rect, &widths, &LegendMetrics::default());
    if flowed.more > 0 {
        report.add(
            place,
            Severity::Warning,
            "legend.overflow",
            String::new(),
            format!(
                "{label} lists {} of its {} entries: {} do not fit and print as \"+{} more\"",
                legend.entries.len() - flowed.more,
                legend.entries.len(),
                flowed.more,
                flowed.more
            ),
            String::from("Make it larger, or narrow its scope to this sheet"),
        );
    }
}

fn check_image(
    viewport: &Viewport,
    place: Place<'_>,
    options: &PreflightOptions<'_>,
    report: &mut Report<'_>,
) {
    let label = label_of(viewport);
    if viewport.text.is_empty() {
        report.add(
            place,
            Severity::Error,
            "image.missing",
            String::new(),
            format!("{label} names no image: it prints empty"),
            String::from("Choose its image in the view's properties"),
        );
    } else if !options.assets.as_os_str().is_empty()
        // Where the painter looks for it.
        && !options.assets.join(&viewport.text).is_file()
    {
        report.add(
            place,
            Severity::Error,
            "image.missing",
            viewport.text.clone(),
            format!(
                "{label} shows {}, which is not in the project's assets folder: it prints empty",
                viewport.text
            ),
            String::from("Choose its image again in the view's properties"),
        );
    }
}

// Marks leading to sheets that were removed.
fn check_marks(set: &SheetSet, viewport: &Viewport, place: Place<'_>, report: &mut Report<'_>) {
    let mut lines = 0_usize;
    let mut outlines = 0_usize;
    let mut gone: Vec<&str> = Vec::new();
    for mark in &viewport.marks {
        if mark.sheet.is_empty() || sheet_index(set, &mark.sheet).is_some() {
            continue;
        }
        if mark.kind == WorldMarkKind::MatchLine {
            lines += 1;
        } else {
            outlines += 1;
        }
        if !gone.contains(&mark.sheet.as_str()) {
            gone.push(&mark.sheet);
        }
    }
    if gone.is_empty() {
        return;
    }
    let ids = gone.join(", ");
    let mut what = String::new();
    if lines > 0 {
        what = format!("{lines} match line{}", if lines == 1 { "" } else { "s" });
    }
    if outlines > 0 {
        what.push_str(&format!(
            "{}{outlines} key-plan outline{}",
            if what.is_empty() { "" } else { " and " },
            if outlines == 1 { "" } else { "s" }
        ));
    }
    report.add(
        place,
        Severity::Warning,
        "matchline.dangling",
        ids.clone(),
        format!(
            "{} has {what} leading to sheets that no longer exist ({ids}): they print their label \
             alone",
            label_of(viewport)
        ),
        String::from("Generate the sheets again, or remove the marks"),
    );
}

type FirstIds = BTreeMap<String, (usize, usize)>;

#[allow(
    clippy::too_many_arguments,
    reason = "The C++ checkViewport takes the set, the two positions, the first ids, the frames, \
              the model, the options and the report one by one."
)]
fn check_viewport(
    set: &SheetSet,
    index: usize,
    position: usize,
    first_ids: &FirstIds,
    frames: &mut Frames,
    model: &Model,
    options: &PreflightOptions<'_>,
    report: &mut Report<'_>,
) {
    let sheet = &set.sheets[index];
    let viewport = &sheet.viewports[position];
    let place = Place {
        sheet_index: Some(index),
        sheet_id: &sheet.id,
        viewport_id: &viewport.id,
    };
    let label = label_of(viewport);

    if viewport.id.is_empty() {
        report.add(
            place,
            Severity::Error,
            "viewport.duplicate-id",
            String::new(),
            String::from("A viewport has no id: it cannot be edited"),
            String::from("Remove it and add it again: a new view gets an id"),
        );
    } else if let Some(&first) = first_ids.get(&viewport.id)
        && first != (index, position)
    {
        report.add(
            place,
            Severity::Error,
            "viewport.duplicate-id",
            viewport.id.clone(),
            format!(
                "{label} has the same id as a view on sheet {}: an edit of one may change the \
                 other",
                first.0 + 1
            ),
            String::from(
                "Duplicate its sheet and remove this one: the copy's views get ids of their own",
            ),
        );
    }

    if viewport.rect.empty() {
        report.add(
            place,
            Severity::Warning,
            "viewport.unplaced",
            String::new(),
            format!("{label} is not placed on the paper, so it does not print"),
            String::from("Tile the sheet, or give the view a rectangle"),
        );
        return;
    }

    // On the paper, inside the drawing area, clear of the title block.
    let paper_size = paper_dimensions(sheet.paper, sheet.landscape);
    let paper = Box2::new(
        Point2::new(0.0, 0.0),
        Point2::new(paper_size.width_mm, paper_size.height_mm),
    );
    let rect = &viewport.rect;
    let area = drawing_area(sheet);
    let tolerance = options.outside_tolerance_mm;
    let (under_width, under_height) = match printed_frame(sheet, frames) {
        Some(frame) => shared(rect, &frame.title_block),
        None => (0.0, 0.0),
    };
    let back = "Drag it back inside the drawing area, or tile the sheet";
    if !rect.intersects(paper) {
        report.add(
            place,
            Severity::Error,
            "viewport.outside",
            String::new(),
            format!("{label} is off the paper: it does not print"),
            String::from(back),
        );
    } else if beyond(rect, &paper) > tolerance {
        report.add(
            place,
            Severity::Error,
            "viewport.outside",
            String::new(),
            format!(
                "{label} runs {} off the paper: that part is cut off",
                mm(beyond(rect, &paper))
            ),
            String::from(back),
        );
    } else if under_width > tolerance && under_height > tolerance {
        report.add(
            place,
            Severity::Error,
            "viewport.outside",
            String::new(),
            format!(
                "{label} runs {} under the title block: the title block covers it",
                mm(under_height)
            ),
            String::from(back),
        );
    } else if beyond(rect, &area) > tolerance {
        report.add(
            place,
            Severity::Warning,
            "viewport.outside",
            String::new(),
            format!(
                "{label} runs {} outside the drawing area, into the margin",
                mm(beyond(rect, &area))
            ),
            String::from(back),
        );
    }

    // What it covers of the views behind it.
    for under in &sheet.viewports[..position] {
        if under.rect.empty() {
            continue;
        }
        let (w, h) = shared(rect, &under.rect);
        if w <= options.overlap_tolerance_mm || h <= options.overlap_tolerance_mm {
            continue;
        }
        let inset =
            is_panel(viewport.kind) && !is_panel(under.kind) && under.rect.contains_box(*rect);
        report.add(
            place,
            if inset {
                Severity::Info
            } else {
                Severity::Warning
            },
            "viewport.overlap",
            under.id.clone(),
            if inset {
                format!(
                    "{label} is set inside {}: the drawing under it does not print",
                    label_of(under)
                )
            } else {
                format!(
                    "{label} covers {} x {} of {}: what is under it does not print",
                    std_fixed(w, 1),
                    mm(h),
                    label_of(under)
                )
            },
            String::from(if inset {
                "Move it where the drawing under it is not needed"
            } else {
                "Move or resize one of them, or tile the sheet"
            }),
        );
    }

    let least = minimum_size(viewport.kind);
    if rect.width() < least.width - tolerance || rect.height() < least.height - tolerance {
        report.add(
            place,
            Severity::Info,
            "viewport.too-small",
            String::new(),
            format!(
                "{label} is {} x {}, smaller than {} x {} mm, the least a {} view reads at",
                std_fixed(rect.width(), 1),
                mm(rect.height()),
                std_fixed(least.width, 0),
                std_fixed(least.height, 0),
                viewport.kind.name()
            ),
            String::from("Make it larger"),
        );
    }

    if is_scaled(viewport.kind) && !viewport.auto_scale {
        let scale = viewport.scale;
        if !(scale > 0.0) || !scale.is_finite() {
            report.add(
                place,
                Severity::Error,
                "scale.invalid",
                String::new(),
                format!("{label} has no usable scale ({})", format_exact_real(scale)),
                String::from("Give it a scale, or set it to Auto"),
            );
        } else if !SHEET_SCALES
            .iter()
            .any(|&standard| (standard - scale).abs() <= 1e-9 * standard)
        {
            let above = SHEET_SCALES.iter().position(|&standard| standard > scale);
            let fix = match above {
                Some(0) => format!("Use {}", scale_name(SHEET_SCALES[0])),
                None => format!(
                    "Use {} if it fits",
                    scale_name(SHEET_SCALES[SHEET_SCALES.len() - 1])
                ),
                Some(at) => format!(
                    "Use {} (it shows a little more) or {}",
                    scale_name(SHEET_SCALES[at]),
                    scale_name(SHEET_SCALES[at - 1])
                ),
            };
            report.add(
                place,
                Severity::Info,
                "scale.non-standard",
                String::new(),
                format!(
                    "{label} is at 1:{}, which is not a standard scale",
                    format_general(scale, 6)
                ),
                fix,
            );
        }
    }

    match viewport.kind {
        // A key plan is drawn as a plan is, so checked as one.
        ViewportKind::Plan | ViewportKind::KeyPlan => {
            check_plan(set, viewport, place, model, options, report);
        }
        ViewportKind::LongSection | ViewportKind::CrossSections => {
            check_section(viewport, place, model, options, report);
        }
        ViewportKind::Image => check_image(viewport, place, options, report),
        ViewportKind::Notes => {
            if is_blank(&viewport.text) {
                report.add(
                    place,
                    Severity::Info,
                    "notes.empty",
                    String::new(),
                    format!("{label} has no text: it prints an empty box"),
                    String::from("Type its text in the view's properties, or remove it"),
                );
            }
        }
        ViewportKind::SheetIndex | ViewportKind::Revisions => {
            check_table(set, viewport, place, options, report);
        }
        ViewportKind::Legend => check_legend(set, viewport, place, model, options, report),
        ViewportKind::Model3D => {}
    }

    check_marks(set, viewport, place, report);
}

/// A plan viewport's scale and centre, "auto" decided the painter's way from
/// the model alone: the viewport's stretch of its alignment, else what the
/// viewport draws (every entity its layers show, every alignment, and
/// `other_content`), fitted with 4% to spare at the first of SHEET_SCALES.
/// The C++'s default `other_content` is empty.
pub fn plan_window(viewport: &Viewport, model: &Model, other_content: &[Box2]) -> PlanWindow {
    // The painter's rule (resolvePlanViewport), from the model alone.
    let mut at = PlanWindow {
        scale: viewport.scale,
        centre: viewport.centre,
    };
    if (!viewport.auto_scale && !viewport.auto_centre) || viewport.rect.empty() {
        return at;
    }
    let mut points: Vec<Point2> = Vec::new();
    let from = &viewport.source;
    if !from.alignment.is_empty()
        && let Some(alignment) = model.alignments.find(&from.alignment)
        && let Ok(solved) = solve_alignment(&alignment.horizontal)
    {
        let mut a = from.chainage_from;
        let mut b = from.chainage_to;
        if !(b > a) {
            a = solved.start_station();
            b = solved.end_station();
        }
        for i in 0..=64 {
            if let Some(p) = solved.point_at_station(a + (b - a) * f64::from(i) / 64.0) {
                points.push(p);
            }
        }
    }
    if points.is_empty() {
        let mut extent = drawn_extent(model, &viewport.hidden_layers);
        model.alignments.for_each(|alignment| {
            if let Ok(solved) = solve_alignment(&alignment.horizontal) {
                for &vertex in &solved.to_polyline(1.0).vertices {
                    extent.expand(vertex);
                }
            }
        });
        for other in other_content {
            extent.expand_box(*other);
        }
        if extent.empty() {
            return at;
        }
        points = vec![
            extent.min,
            extent.max,
            Point2::new(extent.min.x, extent.max.y),
            Point2::new(extent.max.x, extent.min.y),
        ];
    }
    if viewport.auto_centre {
        let mut extent = Box2::default();
        for &p in &points {
            extent.expand(p.rotated(-viewport.rotation));
        }
        at.centre = extent.center().rotated(viewport.rotation);
    }
    if viewport.auto_scale {
        let mut half_w = 0.0;
        let mut half_h = 0.0;
        for &p in &points {
            let d = (p - at.centre).rotated(-viewport.rotation);
            half_w = cpp::max(half_w, d.x.abs());
            half_h = cpp::max(half_h, d.y.abs());
        }
        let needed = cpp::max(
            2.0 * half_w * 1000.0 / viewport.rect.width(),
            2.0 * half_h * 1000.0 / viewport.rect.height(),
        ) * 1.04;
        if needed > 0.0
            && let Ok(scale) = sheet_scale_at_least(needed)
        {
            at.scale = scale;
        }
    }
    at
}

/// The world corners of what a plan viewport shows at `window`, counter-
/// clockwise from its bottom-left on the paper: its rectangle at the scale,
/// about the centre, turned by the viewport's rotation.
pub fn plan_window_corners(viewport: &Viewport, window: &PlanWindow) -> [Point2; 4] {
    window_of(viewport, window).corners()
}

/// Everything that would go wrong plotting `set` (docs/plotting.md lists each
/// check and why). `context` is the project's contribution to the title block
/// (field_context_for builds it); `model` is what the views show. The C++'s
/// default options are PreflightOptions::default().
pub fn check_sheets(
    set: &SheetSet,
    model: &Model,
    context: &FieldContext,
    options: &PreflightOptions<'_>,
) -> Vec<Finding> {
    let mut report = Report {
        skip: &options.skip,
        findings: Vec::new(),
    };
    let indices = sheets_to_check(set, options);
    let mut frames: Frames = BTreeMap::new();

    // The set's own findings first: they are about every sheet.
    check_fields(set, context, &indices, &mut frames, &mut report);
    check_logo(set, &indices, &mut frames, options, &mut report);
    if let Err(error) = validate_page_setup(&set.page_setup) {
        report.add(
            THE_SET,
            Severity::Error,
            "pagesetup.invalid",
            String::new(),
            format!("The page setup cannot be plotted: {}", error.message),
            String::from("Correct it in the Plot dialog, or with SHEETS PAGESETUP"),
        );
    }

    // Where each viewport id is first used, over the whole set.
    let mut first_ids: FirstIds = BTreeMap::new();
    for (s, sheet) in set.sheets.iter().enumerate() {
        for (v, viewport) in sheet.viewports.iter().enumerate() {
            first_ids.entry(viewport.id.clone()).or_insert((s, v));
        }
    }
    for &index in &indices {
        check_sheet_itself(set, index, &mut frames, &mut report);
        for position in 0..set.sheets[index].viewports.len() {
            check_viewport(
                set,
                index,
                position,
                &first_ids,
                &mut frames,
                model,
                options,
                &mut report,
            );
        }
    }
    report.findings
}

/// The same for a document's own sheets: its drawing, its spatial index, its
/// project fields, whether its logo file is there, where its images are and
/// whether its surfaces' styles can be drawn. Options the caller set are
/// kept. "Today" is the plot date the fields are given: any date will do, the
/// findings ask only whether a date prints.
pub fn check_document_sheets<'a>(
    document: &'a Document,
    mut options: PreflightOptions<'a>,
) -> Vec<Finding> {
    let set = document.sheet_set();
    if options.index.is_none() {
        options.index = Some(document.spatial_index());
    }
    if options.logo_readable.is_none() && !set.defaults.logo_asset.is_empty() {
        options.logo_readable = Some(logo_path(document).is_some_and(|path| path.is_file()));
    }
    if options.surface_problems.is_none() {
        options.surface_problems = Some(surface_style_problems(
            document.surfaces(),
            document.surface_styles(),
        ));
    }
    if options.assets.as_os_str().is_empty()
        && let Some(directory) = document.project_directory()
    {
        options.assets = directory.join("assets");
    }
    // Any date will do: the findings ask only whether a date prints.
    let context = field_context_for(document, frame_date(today_utc()));
    check_sheets(&set, document.model(), &context, &options)
}

/// The findings about the sheets at `indices` (their own, and the set's) in
/// their order; every finding when `indices` is empty. What a plot of just
/// those sheets is warned about.
pub fn findings_on_sheets(findings: &[Finding], indices: &[usize]) -> Vec<Finding> {
    findings
        .iter()
        .filter(|finding| {
            indices.is_empty()
                || finding
                    .sheet_index
                    .is_none_or(|index| indices.contains(&index))
        })
        .cloned()
        .collect()
}

pub fn summarize(findings: &[Finding]) -> PreflightSummary {
    let mut summary = PreflightSummary::default();
    for finding in findings {
        match finding.severity {
            Severity::Error => summary.errors += 1,
            Severity::Warning => summary.warnings += 1,
            Severity::Info => summary.infos += 1,
        }
    }
    summary
}

/// "2 errors, 1 warning, 3 notes"; "no problems found" for none.
pub fn summary_text(summary: &PreflightSummary) -> String {
    let mut text = String::new();
    let mut part = |count: usize, one: &str, many: &str| {
        if count > 0 {
            if !text.is_empty() {
                text.push_str(", ");
            }
            text.push_str(&format!("{count} {}", if count == 1 { one } else { many }));
        }
    };
    part(summary.errors, "error", "errors");
    part(summary.warnings, "warning", "warnings");
    part(summary.infos, "note", "notes");
    if text.is_empty() {
        String::from("no problems found")
    } else {
        text
    }
}

/// One line for a log: "ERROR viewport.outside [s2/vp3] PLAN 1:500 runs
/// 12.0 mm under the title block. Fix: ..."
pub fn finding_line(finding: &Finding) -> String {
    let severity = finding.severity.name().to_ascii_uppercase();
    let mut place = String::new();
    if !finding.sheet_id.is_empty() || !finding.viewport_id.is_empty() {
        place = format!(" [{}", finding.sheet_id);
        if !finding.viewport_id.is_empty() {
            place.push('/');
            place.push_str(&finding.viewport_id);
        }
        place.push(']');
    } else if let Some(index) = finding.sheet_index {
        place = format!(" [sheet {}]", index + 1);
    }
    let mut line = format!("{severity} {}{place} {}.", finding.code, finding.message);
    if !finding.fix.is_empty() {
        line.push_str(&format!(" Fix: {}.", finding.fix));
    }
    line
}

/// The findings as JSON, for an agent: {"format": "katana-sheet-checks",
/// "version": 1, "summary": {...}, "findings": [{...}]}. Empty members are
/// left out; findings_from_json reads it back to the same findings.
pub fn findings_to_json(findings: &[Finding]) -> String {
    let summary = summarize(findings);
    let mut list: Vec<Json> = Vec::new();
    for finding in findings {
        let mut item = Json::object();
        item["severity"] = Json::from(finding.severity.name());
        item["code"] = Json::from(&finding.code);
        if let Some(index) = finding.sheet_index {
            item["sheet_index"] = Json::from(index);
        }
        let texts = [
            ("sheet", &finding.sheet_id),
            ("viewport", &finding.viewport_id),
            ("subject", &finding.subject),
            ("message", &finding.message),
            ("fix", &finding.fix),
        ];
        for (key, value) in texts {
            if !value.is_empty() {
                item[key] = Json::from(value);
            }
        }
        list.push(item);
    }
    let mut json = Json::object();
    json["format"] = Json::from("katana-sheet-checks");
    json["version"] = Json::from(1_i32);
    json["summary"] = Json::object_of([
        ("errors", Json::from(summary.errors)),
        ("warnings", Json::from(summary.warnings)),
        ("info", Json::from(summary.infos)),
    ]);
    json["findings"] = Json::from(list);
    json.dump()
}

// What findings_from_json reads, nlohmann's exceptions apart: the C++ catches
// every one of them and says the report cannot be read.
fn read_findings(text: &str) -> JsonResult<Result<Vec<Finding>>> {
    let json = Json::parse(text)?;
    if !json.is_object() || json.value("format", String::new())? != "katana-sheet-checks" {
        return Ok(Err(Error::new(
            ErrorCode::ParseFailure,
            "not a sheet-checks report",
        )));
    }
    if json.value("version", 0_i32)? != 1 {
        return Ok(Err(make_error(
            ErrorCode::Unsupported,
            "a sheet-checks report of another version",
            format!("{}", json.value("version", 0_i32)?),
        )));
    }
    let list = json.at("findings")?;
    let Some(items) = list.as_array() else {
        return Ok(Err(Error::new(
            ErrorCode::ParseFailure,
            "a sheet-checks report whose findings are not a list",
        )));
    };
    let mut findings = Vec::new();
    for item in items {
        let severity_name = item.at("severity")?.get::<String>()?;
        let Some(severity) = severity_from(&severity_name) else {
            return Ok(Err(make_error(
                ErrorCode::ParseFailure,
                "an unknown severity",
                severity_name,
            )));
        };
        let code = item.at("code")?.get::<String>()?;
        let mut sheet_index = None;
        if item.contains("sheet_index") {
            // A position: a negative or fractional one is not read as a huge
            // or rounded one.
            let index = item.at("sheet_index")?;
            if !index.is_number_unsigned() {
                return Ok(Err(make_error(
                    ErrorCode::ParseFailure,
                    "a sheet index that is not a position",
                    index.dump(),
                )));
            }
            sheet_index = Some(index.get::<usize>()?);
        }
        findings.push(Finding {
            severity,
            code,
            sheet_index,
            sheet_id: item.value("sheet", String::new())?,
            viewport_id: item.value("viewport", String::new())?,
            subject: item.value("subject", String::new())?,
            message: item.value("message", String::new())?,
            fix: item.value("fix", String::new())?,
        });
    }
    Ok(Ok(findings))
}

/// The findings a findings_to_json text holds. ParseFailure for text that is
/// not a sheet-checks report; Unsupported for a report of another version.
pub fn findings_from_json(text: &str) -> Result<Vec<Finding>> {
    match read_findings(text) {
        Ok(read) => read,
        Err(error) => Err(make_error(
            ErrorCode::ParseFailure,
            "a sheet-checks report that cannot be read",
            error.what,
        )),
    }
}
