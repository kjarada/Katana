//! The preflight checks in the sheet editor (docs/plotting.md, "Preflight checks"): the headless
//! checker (cad's preflight) given what the painter knows, and the Checks dock that lists what it
//! finds. The port of plotting/sheet_checks.
//!
//! The checker needs no window. What it cannot know by itself is what the WINDOW draws from: the
//! imagery, point clouds and meshes a plan shows, the surfaces the sections are cut from, whether
//! the logo decoded, and the painter's own rule for an automatic plan. The painter's SheetSource
//! carries all of it, so preflight_options_for turns one into the other, and what the checks call
//! empty is what the painter would leave empty.
//!
//! The dock checks again on its own a moment after the edits stop (schedule), so a burst of edits
//! costs one check, not one per edit; Check Sheets and a plot run it at once (the editor's
//! check_now). What it finds never stops a plot.
//!
//! THE RUST SHAPE. The C++ dock held the Document and a checker closure and ran on a QTimer. Here
//! the dock holds the findings and the timer's deadline only: the editor, which is lent the host,
//! runs the checker and hands the findings over (check_now_with), and asks at each frame whether a
//! scheduled run is due (due). The editor's own check functions (checkSheets, showFinding) are in
//! this file, as the C++ kept them beside the dock they drive.

use std::time::{Duration, Instant};

use egui::Ui;
use katana_cad::plotting::legend::PlanResolver;
use katana_cad::plotting::preflight::{
    Finding, PreflightOptions, PreflightSummary, Severity, check_sheets, finding_line, summarize,
    summary_text,
};
use katana_cad::plotting::sheet_set::{
    PlanWindow, SheetSet, Viewport, ViewportKind, format_sheet_number, sheet_index,
};
use katana_cad::terrain::surface_plan::surface_style_problems;
use katana_cad::terrain::surface_style::{SurfaceStyle, built_in_surface_styles};
use katana_entity::Model;
use katana_geometry::{Box2, Point2};

use crate::canvas::{
    Brush, Canvas, Color, FillRule, Image, Pen, PenCap, PenJoin, PenStyle, PointF, RasterCanvas,
    RectF, RenderHint,
};
use crate::dialog_layout::{
    TABLE_COLUMN_GAP, TABLE_ROW_GAP, empty_table, table_header, table_row_colour,
};
use crate::drive::controls;
use crate::drive::mailbox::{Act, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::host::Host;
use crate::plotting::sheet_tables::resolved_sheet_set;
use crate::sheet_editor::SheetEditor;
use crate::sheet_painter::{SheetSource, resolve_plan_viewport};

// ---- the checker, given what the painter knows -------------------------------------------------

/// The painter's own rule for an automatic plan (resolve_plan_viewport), as the checker asks it,
/// so the window checked is the one drawn.
pub fn plan_resolver<'s>(source: &'s SheetSource<'s>) -> impl Fn(&Viewport) -> PlanWindow + 's {
    move |viewport: &Viewport| {
        let at = resolve_plan_viewport(viewport, source);
        PlanWindow {
            scale: at.scale,
            centre: at.centre,
        }
    }
}

/// The checker's options for sheets painted from `source`: an automatic plan resolved by
/// `resolver` (plan_resolver), the visible imagery, point clouds and meshes counted as content,
/// the visible surfaces as what sections are cut from, the decoded logo, the assets folder and the
/// spatial index. They borrow `source` and `resolver`, so they cannot outlive them (the C++'s
/// options referred to the source the same way, unchecked).
pub fn preflight_options_for<'a>(
    source: &'a SheetSource<'a>,
    resolver: &'a PlanResolver<'a>,
) -> PreflightOptions<'a> {
    let mut options = PreflightOptions {
        resolve_plan: Some(resolver),
        ..PreflightOptions::default()
    };
    // What a plan draws besides entities and alignments, as plan_drawn_bounds counts it: visible
    // imagery and point clouds, and every shown mesh's footprint.
    if let Some(reference) = source.plan.reference {
        for raster in reference.rasters() {
            if raster.visible {
                options.other_content.push(raster.world_bounds());
            }
        }
        for cloud in reference.point_clouds() {
            if cloud.visible {
                options.other_content.push(cloud.world_bounds());
            }
        }
    }
    if let Some(meshes) = source.plan.meshes {
        for item in meshes {
            let Some(mesh) = item.mesh else {
                continue;
            };
            if !item.visible || mesh.empty() || item.style == SurfaceStyle::Hidden {
                continue;
            }
            let space = mesh.bounds();
            if !space.empty() {
                options.other_content.push(Box2::new(
                    Point2::new(space.min.x, space.min.y),
                    Point2::new(space.max.x, space.max.y),
                ));
            }
        }
    }
    // The painter cuts sections from the visible surfaces only.
    options.section_surfaces = Some(
        source
            .surfaces
            .iter()
            .filter(|surface| surface.visible && surface.surface.is_some())
            .count(),
    );
    // The surfaces the plan painter draws, and whether their styles can be drawn in whole: a plan
    // the painter would draw without a surface's contours is warned about before the plot
    // (plan.surface-style). With no surfaces to draw there is nothing to say, which is known.
    options.surface_problems = Some(match source.plan.surfaces {
        Some(surfaces) => match source.plan.surface_styles {
            Some(styles) => surface_style_problems(surfaces, styles),
            None => surface_style_problems(surfaces, &built_in_surface_styles()),
        },
        None => Vec::new(),
    });
    // The caller loads the logo once for every page; a null image is a file missing or unreadable.
    options.logo_readable = Some(!source.logo.is_null());
    options.assets.clone_from(&source.assets);
    options.index = source.plan.index;
    options
}

/// The checks (cad's check_sheets) for the sheets at `sheets` of `set` (every sheet when empty),
/// with those options. A source with no drawing is checked against an empty one, as the painter
/// would draw it.
pub fn check_sheets_for(set: &SheetSet, source: &SheetSource<'_>, sheets: &[usize]) -> Vec<Finding> {
    let nothing = Model::default();
    let model = source.plan.model.unwrap_or(&nothing);
    // A register is laid out with the scales it prints, as the painter lays it out.
    let registers = set.sheets.iter().any(|sheet| {
        sheet
            .viewports
            .iter()
            .any(|viewport| viewport.kind == ViewportKind::SheetIndex)
    });
    let drawn = registers.then(|| resolved_sheet_set(set, source, None));
    let resolver = plan_resolver(source);
    let mut options = preflight_options_for(source, &resolver);
    options.sheets = sheets.to_vec();
    if source.plan.model.is_none() {
        // An index of some other drawing.
        options.index = None;
    }
    options.drawn_set = drawn.as_ref();
    check_sheets(set, model, &source.fields, &options)
}

/// What a log gets before a plot: a summary line, then a line for each error (finding_line).
/// Nothing when there are only notes or nothing at all.
pub fn preflight_log(findings: &[Finding]) -> Vec<String> {
    let summary = summarize(findings);
    if summary.errors == 0 && summary.warnings == 0 {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "Sheet checks: {}. The plot goes ahead; Check Sheets in File > Sheets lists them.",
        summary_text(&summary)
    )];
    lines.extend(
        findings
            .iter()
            .filter(|finding| finding.severity == Severity::Error)
            .map(finding_line),
    );
    lines
}

// ---- the marks ---------------------------------------------------------------------------------

/// A mark painted in a 16-unit box at `size` pixels. Painted here rather than taken from the
/// platform's style, so they read the same everywhere.
fn painted_icon(size: u32, paint: impl Fn(&mut dyn Canvas)) -> Image {
    let mut image = Image::new(size, size);
    RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        let unit = f64::from(size) / 16.0;
        canvas.scale(unit, unit);
        paint(canvas);
    });
    image
}

fn stroke(color: Color, width: f64) -> Pen {
    Pen::with(color, width, PenStyle::Solid, PenCap::Round, PenJoin::Round)
}

/// The toolbar's Check Sheets glyph: a sheet with a tick on it.
pub fn check_sheets_icon(size: u32) -> Image {
    painted_icon(size, |canvas| {
        canvas.set_pen(stroke(Color::rgb(110, 110, 110), 1.0));
        canvas.set_brush(Brush::solid(Color::WHITE));
        canvas.draw_rect(RectF::new(2.5, 1.5, 11.0, 13.0));
        canvas.set_brush(Brush::none());
        canvas.set_pen(stroke(Color::rgb(46, 125, 50), 2.0));
        canvas.draw_polyline(&[
            PointF::new(5.0, 8.5),
            PointF::new(7.3, 11.0),
            PointF::new(11.5, 5.0),
        ]);
    })
}

/// A finding's severity mark at `size` pixels.
pub fn severity_icon(severity: Severity, size: u32) -> Image {
    match severity {
        // A red disc with a white cross.
        Severity::Error => painted_icon(size, |canvas| {
            canvas.set_pen(Pen::none());
            canvas.set_brush(Brush::solid(Color::rgb(211, 47, 47)));
            canvas.draw_ellipse(RectF::new(1.0, 1.0, 14.0, 14.0));
            canvas.set_pen(stroke(Color::WHITE, 2.0));
            canvas.draw_line(PointF::new(5.5, 5.5), PointF::new(10.5, 10.5));
            canvas.draw_line(PointF::new(10.5, 5.5), PointF::new(5.5, 10.5));
        }),
        // An amber triangle with a dark exclamation mark.
        Severity::Warning => painted_icon(size, |canvas| {
            canvas.set_pen(stroke(Color::rgb(245, 166, 35), 1.0));
            canvas.set_brush(Brush::solid(Color::rgb(245, 166, 35)));
            canvas.draw_polygon(
                &[
                    PointF::new(8.0, 1.5),
                    PointF::new(15.0, 14.5),
                    PointF::new(1.0, 14.5),
                ],
                FillRule::OddEven,
            );
            canvas.set_pen(stroke(Color::rgb(60, 40, 0), 2.0));
            canvas.draw_line(PointF::new(8.0, 6.0), PointF::new(8.0, 9.8));
            canvas.draw_point(PointF::new(8.0, 12.3));
        }),
        // A blue disc with a white "i".
        Severity::Info => painted_icon(size, |canvas| {
            canvas.set_pen(Pen::none());
            canvas.set_brush(Brush::solid(Color::rgb(25, 118, 210)));
            canvas.draw_ellipse(RectF::new(1.0, 1.0, 14.0, 14.0));
            canvas.set_pen(stroke(Color::WHITE, 2.0));
            canvas.draw_point(PointF::new(8.0, 4.8));
            canvas.draw_line(PointF::new(8.0, 7.5), PointF::new(8.0, 11.5));
        }),
    }
}

// ---- the dock ----------------------------------------------------------------------------------

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Info => "Note",
    }
}

/// "No problems found." / "2 errors, 1 warning, 3 notes."
fn sentence(summary: &PreflightSummary) -> String {
    let text = summary_text(summary);
    let mut characters = text.chars();
    let mut capital = match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    };
    capital.push('.');
    capital
}

/// The sheet a finding is on, as the list names it: its number and name, or "All sheets" for the
/// set's own.
fn sheet_text(set: &SheetSet, finding: &Finding) -> String {
    let Some(index) = finding.sheet_index else {
        return String::from("All sheets");
    };
    if index >= set.sheets.len() || set.sheets[index].id != finding.sheet_id {
        // The set changed since.
        return finding.sheet_id.clone();
    }
    let number = format_sheet_number(
        &set.numbering,
        index + 1,
        set.sheets.len(),
        &set.defaults.set_number,
    );
    format!("{number}  {}", set.sheets[index].name)
}

/// Whether two findings are about the same thing, however their words changed: the row kept
/// current across a check.
fn same_subject(a: &Finding, b: &Finding) -> bool {
    a.code == b.code
        && a.sheet_id == b.sheet_id
        && a.viewport_id == b.viewport_id
        && a.subject == b.subject
}

/// One row of the list: its severity, sheet, view, what is wrong and what to do (the columns), and
/// the tooltip every cell shows.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckRow {
    pub finding: usize,
    pub severity: Severity,
    pub cells: [String; 5],
    pub tip: String,
}

/// The Checks dock: every finding of the last run, one row each - errors first, then warnings,
/// then notes, each in the checker's order. A double-click (or Enter) on a row goes to it.
#[derive(Debug)]
pub struct SheetChecksDock {
    findings: Vec<Finding>,
    rows: Vec<CheckRow>,
    current: Option<usize>,
    summary: String,
    title: String,
    runs: i32,
    /// A change came while the window was hidden.
    stale: bool,
    delay: Duration,
    /// When the scheduled run is due (the C++'s single-shot timer, running).
    due: Option<Instant>,
    open: bool,
    /// A row activated in the last frame, for the editor to go to.
    activated: Option<usize>,
}

impl Default for SheetChecksDock {
    fn default() -> Self {
        Self::new()
    }
}

impl SheetChecksDock {
    /// How long after the last change the checks run again: long enough for a burst of edits to
    /// settle, short enough to feel live.
    pub const RECHECK_DELAY_MS: u64 = 400;

    pub fn new() -> Self {
        let mut dock = Self {
            findings: Vec::new(),
            rows: Vec::new(),
            current: None,
            summary: String::new(),
            title: String::new(),
            runs: 0,
            stale: false,
            delay: Duration::from_millis(Self::RECHECK_DELAY_MS),
            due: None,
            open: true,
            activated: None,
        };
        dock.rebuild(&SheetSet::default());
        dock
    }

    /// The findings of a run made now, listed; a scheduled run is dropped, since this one is
    /// newer. The editor runs the checker (SheetEditor::check_now) and hands them here.
    pub fn check_now_with(&mut self, findings: Vec<Finding>, set: &SheetSet) -> &[Finding] {
        self.due = None;
        let current = self
            .current
            .and_then(|row| self.rows.get(row))
            .and_then(|row| self.findings.get(row.finding))
            .cloned();
        self.findings = findings;
        self.runs += 1;
        self.rebuild(set);
        // The row that was current, found again by what it is about.
        self.current = current.and_then(|current| {
            self.findings
                .iter()
                .position(|finding| same_subject(finding, &current))
                .and_then(|index| self.row_of(index))
        });
        &self.findings
    }

    /// Runs the checks a delay after the last call. While the window the dock is in is hidden,
    /// only marks the findings out of date: they are checked again when it is shown.
    pub fn schedule(&mut self, window_visible: bool) {
        // The editor lives on while it is closed, and the document keeps changing under it:
        // checking every sheet after each edit nobody can see the result of would only slow the
        // drawing down. Marked out of date, and checked once the editor is shown again.
        if !window_visible {
            self.stale = true;
            self.due = None;
            return;
        }
        self.due = Some(Instant::now() + self.delay);
    }

    /// The window is shown again (the C++'s showEvent): a run missed while hidden is scheduled.
    pub fn shown_again(&mut self) {
        if self.stale {
            self.stale = false;
            self.due = Some(Instant::now() + self.delay);
        }
    }

    pub fn pending(&self) -> bool {
        self.due.is_some()
    }

    /// Whether the scheduled run is due now.
    pub fn due(&self) -> bool {
        self.due.is_some_and(|due| Instant::now() >= due)
    }

    /// How long until the scheduled run, for the frame that should come then.
    pub fn time_to_run(&self) -> Option<Duration> {
        self.due
            .map(|due| due.saturating_duration_since(Instant::now()))
    }

    pub fn set_delay(&mut self, milliseconds: i64) {
        self.delay = Duration::from_millis(u64::try_from(milliseconds.max(0)).unwrap_or(0));
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    pub fn rows(&self) -> &[CheckRow] {
        &self.rows
    }

    /// The summary line (sheetChecksSummary).
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The dock's title: "Checks", or "Checks (1 error, 2 warnings)".
    pub fn window_title(&self) -> &str {
        &self.title
    }

    /// How many times the checks have run, for tests of the debounce.
    pub fn runs(&self) -> i32 {
        self.runs
    }

    /// The list's row of findings()[finding]; None for none (the C++'s -1).
    pub fn row_of(&self, finding: usize) -> Option<usize> {
        self.rows.iter().position(|row| row.finding == finding)
    }

    pub fn current_row(&self) -> Option<usize> {
        self.current
    }

    /// Goes to findings()[finding], as a double-click on its row does: its row made current, and
    /// a copy of it handed back for the editor to go to (going there may edit the sheets and so
    /// check them again).
    pub fn activate(&mut self, finding: usize) -> Option<Finding> {
        let copy = self.findings.get(finding)?.clone();
        if let Some(row) = self.row_of(finding) {
            self.current = Some(row);
        }
        Some(copy)
    }

    /// QDockWidget::close and show: whether the dock is shown.
    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn is_visible(&self) -> bool {
        self.open
    }

    fn rebuild(&mut self, set: &SheetSet) {
        // Errors first, then warnings, then notes; the checker's order within each, so the list
        // reads sheet by sheet.
        self.rows.clear();
        for severity in [Severity::Error, Severity::Warning, Severity::Info] {
            for (index, finding) in self.findings.iter().enumerate() {
                if finding.severity != severity {
                    continue;
                }
                self.rows.push(CheckRow {
                    finding: index,
                    severity,
                    cells: [
                        String::from(severity_name(severity)),
                        sheet_text(set, finding),
                        finding.viewport_id.clone(),
                        finding.message.clone(),
                        finding.fix.clone(),
                    ],
                    tip: format!(
                        "{}\n\nFix: {}\n\n{}",
                        finding.message, finding.fix, finding.code
                    ),
                });
            }
        }
        if self.current.is_some_and(|row| row >= self.rows.len()) {
            self.current = None;
        }
        let summary = summarize(&self.findings);
        self.summary = if self.runs == 0 {
            String::from("Not checked yet.")
        } else {
            sentence(&summary)
        };
        self.title = if summary.errors + summary.warnings == 0 {
            String::from("Checks")
        } else {
            format!(
                "Checks ({})",
                summary_text(&PreflightSummary {
                    errors: summary.errors,
                    warnings: summary.warnings,
                    infos: 0,
                })
            )
        };
    }

    /// A row activated in a frame since the last call, by its finding.
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// Draws the dock's content: the summary (sheetChecksSummary) and the list (sheetChecksList),
    /// a double-click or Enter on a row going to it. A fill of the list makes the row whose
    /// severity cell is the text current, as a click does.
    pub fn ui(&mut self, ui: &mut Ui) {
        controls::label(ui, "sheetChecksSummary", &self.summary);
        let name = "sheetChecksList";
        let taken = take(ui, name);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => match self.rows.iter().position(|row| row.cells[0] == *text) {
                    Some(at) => {
                        self.current = Some(at);
                        settle(ui, taken, Ok(()));
                    }
                    None => settle(ui, taken, Err(lists_no(name, text))),
                },
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let mut activated = None;
        // The window's one table look (dialog_layout): the raised header across the pane, faint
        // stripes, the chosen row lit; and with nothing found, a line saying so where a header
        // floated over an empty pane.
        let chosen: Vec<bool> = (0..self.rows.len())
            .map(|at| self.current == Some(at))
            .collect();
        let empty = self.rows.is_empty();
        let response = egui::ScrollArea::vertical()
            .id_salt(name)
            .max_height(180.0)
            .show(ui, |ui| {
                egui::Grid::new(name)
                    .num_columns(5)
                    .spacing([TABLE_COLUMN_GAP, TABLE_ROW_GAP])
                    .min_row_height(ui.spacing().interact_size.y - TABLE_ROW_GAP)
                    .with_row_color(move |row, style| table_row_colour(row, &chosen, style))
                    .show(ui, |ui| {
                    table_header(ui, &["", "Sheet", "View", "Problem", "Fix"], true);
                    ui.end_row();
                    for (at, row) in self.rows.iter().enumerate() {
                        let colour = match row.severity {
                            Severity::Error => Color::rgb(211, 47, 47),
                            Severity::Warning => Color::rgb(245, 166, 35),
                            Severity::Info => Color::rgb(25, 118, 210),
                        };
                        let chosen = self.current == Some(at);
                        let mut first = ui.selectable_label(
                            chosen,
                            egui::RichText::new(row.cells[0].as_str()).color(colour.to_egui()),
                        );
                        for cell in &row.cells[1..] {
                            first = first.union(ui.label(cell.as_str()));
                        }
                        let first = first.on_hover_text(row.tip.as_str());
                        if first.clicked() {
                            self.current = Some(at);
                        }
                        if first.double_clicked()
                            || (chosen && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                        {
                            activated = Some(row.finding);
                        }
                        ui.end_row();
                    }
                });
                if empty {
                    empty_table(
                        ui,
                        "Nothing to list: Check Sheets lists what a plot would get wrong",
                    );
                }
            })
            .inner_rect;
        let response = ui.interact(response, ui.id().with(name), egui::Sense::hover());
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            settle(ui, taken, Ok(()));
        }
        let mut record = ControlRecord::new(name, ControlKind::Rows);
        record.items = self.rows.iter().map(|row| row.cells.join(" | ")).collect();
        record.value = self
            .current
            .and_then(|at| self.rows.get(at))
            .map(|row| row.cells[0].clone())
            .unwrap_or_default();
        controls::custom(ui, &response, record);
        if let Some(finding) = activated {
            if let Some(row) = self.row_of(finding) {
                self.current = Some(row);
            }
            self.activated = Some(finding);
        }
    }
}

// ---- the editor's side -------------------------------------------------------------------------
// SheetEditor's checks are defined here, beside the dock they drive.

impl SheetEditor {
    /// Runs the checks on every sheet now and lists them in the dock (the dock's checkNow, with
    /// the painter's knowledge of the drawing).
    pub fn check_now(&mut self, host: &dyn Host) -> &[Finding] {
        let document = host.document();
        let set = document.sheet_set();
        let findings = {
            let source = (self.source)(document);
            check_sheets_for(&set, &source, &[])
        };
        self.checks.check_now_with(findings, &set)
    }

    /// Checks every sheet now, lists the findings in the dock and shows it (Check Sheets).
    pub fn check_sheets(&mut self, host: &mut dyn Host) -> Vec<Finding> {
        let findings = self.check_now(&*host).to_vec();
        self.checks.show();
        let sheets = host.document().sheet_set().sheets.len();
        self.report(
            host,
            &format!(
                "Checked {sheets} sheet{}: {}",
                if sheets == 1 { "" } else { "s" },
                sentence(&summarize(&findings))
            ),
            false,
        );
        findings
    }

    /// Goes to where a finding is: its sheet made current and its viewport selected, what to do
    /// about it on the status bar.
    pub fn show_finding(&mut self, host: &mut dyn Host, finding: &Finding) {
        self.observe(&*host);
        let set = host.document().sheet_set();
        // By its position while that is still the sheet it was found on (two sheets may share an
        // id: that is a finding too), else by its id.
        let at = match finding.sheet_index {
            Some(index)
                if index < set.sheets.len() && set.sheets[index].id == finding.sheet_id =>
            {
                Some(index)
            }
            _ if !finding.sheet_id.is_empty() => sheet_index(&set, &finding.sheet_id),
            _ => None,
        };
        if let Some(at) = at {
            self.set_current_sheet(&*host, at);
            self.canvas.select(&finding.viewport_id);
            self.focus_canvas = true;
        }
        let mut text = format!("{}.", finding.message);
        if !finding.fix.is_empty() {
            text.push_str(&format!(" {}.", finding.fix));
        }
        self.status = text;
    }

    /// The dock's row `row` activated, as a double-click on it does: the editor goes to its
    /// finding.
    pub fn activate_finding(&mut self, host: &mut dyn Host, finding: usize) {
        if let Some(copy) = self.checks.activate(finding) {
            self.show_finding(host, &copy);
        }
    }
}
