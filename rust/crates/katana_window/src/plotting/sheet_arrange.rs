//! The sheet editor's Arrange commands (docs/plotting.md, "Arranging a sheet"): the Arrange menu on
//! the toolbar and in the canvas's context menu, and the "Choose paper for this scale" button of
//! the sheet's properties. The port of plotting/sheet_arrange.
//!
//! Each command is a thin front end over cad's arrange_commands: it gathers the ids it acts on and
//! what a view shows, makes ONE undoable step, and says what it did - the same step whether a
//! person clicked it, a test called it or an agent ran the command-line verb. What the editor adds
//! is what only the window knows: the reference layers and meshes it shows, and the scale the
//! painter drew an automatic view at.
//!
//! The ids come from arrange_targets, the one place the canvas's selection is read, so a multiple
//! selection plugs in there and every command follows.
//!
//! The commands are the editor's methods here (the C++'s free functions took the document and the
//! editor): each is lent the window, whose drawing it edits.

use std::rc::Rc;

use katana_cad::document::Document;
use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::plot::SHEET_SCALES;
use katana_cad::plotting::arrange::{
    AlignEdge, DistributeAxis, drawn_outline, has_scale, viewport_content as content_of_viewport,
    viewport_content_in_set,
};
use katana_cad::plotting::arrange_commands::{
    align_viewports, auto_arrange_sheet, distribute_viewports, fit_viewport_to_content,
    main_plan_of, match_scale, rotate_to_best_fit,
};
use katana_cad::plotting::plan_grid::PlanPlacement;
use katana_cad::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind};
use katana_cad::plotting::sheet_verbs::run_sheet_verb;
use katana_cad::terrain::surface_style::SurfaceStyle;
use katana_cad::CommandInterpreter;
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{ErrorCode, Result};
use katana_entity::EnumName;
use katana_geometry::polygon::convex_hull;
use katana_geometry::{Box2, Point2};
use katana_math::numerics::RAD_TO_DEG;

use crate::drive::action::{Action, Menu};
use crate::drive::controls::{self, Choice};
use crate::host::Host;
use crate::sheet_editor::{SheetEditor, sheet_verb_context_for};
use crate::sheet_painter::{
    SheetPaintCache, SheetSource, resolve_plan_viewport, resolve_plan_viewport_on_sheet,
    resolve_section_viewport,
};
use crate::shell::DialogFrame;

fn is_section_view(kind: ViewportKind) -> bool {
    kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections
}

fn is_plan(kind: ViewportKind) -> bool {
    kind == ViewportKind::Plan || kind == ViewportKind::KeyPlan
}

/// A box's four corners, for a hull.
fn add_corners(points: &mut Vec<Point2>, area: &Box2) {
    if !area.empty() {
        points.extend([
            area.min,
            Point2::new(area.max.x, area.min.y),
            area.max,
            Point2::new(area.min.x, area.max.y),
        ]);
    }
}

fn find_viewport(set: &SheetSet, id: &str) -> Option<Viewport> {
    set.sheets
        .iter()
        .flat_map(|sheet| &sheet.viewports)
        .find(|viewport| viewport.id == id)
        .cloned()
}

fn scale_words(scale: f64) -> String {
    if scale == scale.floor() {
        format!("1:{scale:.0}")
    } else {
        format!("1:{scale:.2}")
    }
}

fn count_words(count: usize, one: &str) -> String {
    format!("{count} {one}{}", if count == 1 { "" } else { "s" })
}

fn list_words(ids: &[String]) -> String {
    ids.join(", ")
}

fn edge_words(edge: AlignEdge) -> &'static str {
    match edge {
        AlignEdge::Left => "on their left edges",
        AlignEdge::Right => "on their right edges",
        AlignEdge::Top => "on their top edges",
        AlignEdge::Bottom => "on their bottom edges",
        AlignEdge::HorizontalCentre => "on one vertical line through their centres",
        AlignEdge::VerticalCentre => "on one horizontal line through their centres",
    }
}

/// Choose Paper's first scale: the one the plan is drawn at now.
const AS_DRAWN: &str = "As drawn";

/// A Choose Paper scale as SHEET SUGGESTPAPER takes it: "auto" for As drawn, else the denominator
/// of "1:500" or "500". Nothing for any other text.
fn scale_option(text: &str) -> Option<String> {
    let mut typed = text.trim();
    if typed.is_empty() || typed.eq_ignore_ascii_case(AS_DRAWN) {
        return Some(String::from("auto"));
    }
    if let Some(rest) = typed.strip_prefix("1:") {
        typed = rest.trim();
    }
    let denominator = parse_finite_double(typed)?;
    (denominator > 0.0).then(|| String::from(typed))
}

/// SHEET SUGGESTPAPER's reply, "sheet 1 view=vp1 scale=500 paper=A1 orientation=landscape
/// frame=a3_landscape fill=0.834", as a person reads it: "A1 landscape, 83% full at 1:500".
fn advice_words(reply: &str) -> String {
    let mut values = std::collections::BTreeMap::new();
    for word in reply.split(' ') {
        if let Some((key, value)) = word.split_once('=') {
            values.insert(key, value);
        }
    }
    let number = |key: &str| {
        values
            .get(key)
            .and_then(|value| parse_finite_double(value))
            .unwrap_or(0.0)
    };
    let text = |key: &str| values.get(key).copied().unwrap_or_default();
    format!(
        "{} {}, {}% full at {}",
        text("paper"),
        text("orientation"),
        (number("fill") * 100.0).round() as i64,
        scale_words(number("scale"))
    )
}

/// Everything a plan with `layers` hidden draws, as world points: the drawing's outline (cad's
/// drawn_outline) and the corners of the shown reference layers and meshes - what
/// plan_drawn_bounds frames, as points, so a rotation can be fitted to it. The hull,
/// counter-clockwise.
pub fn drawing_content(source: &SheetSource<'_>, layers: &LayerOverrides) -> Vec<Point2> {
    let Some(model) = source.plan.model else {
        return Vec::new();
    };
    let mut points = drawn_outline(model, layers);
    // What plan_drawn_bounds counts besides the drawing: the shown imagery and point clouds, and
    // each shown mesh's footprint.
    if let Some(reference) = source.plan.reference {
        for raster in reference.rasters() {
            if raster.visible {
                add_corners(&mut points, &raster.world_bounds());
            }
        }
        for cloud in reference.point_clouds() {
            if cloud.visible {
                add_corners(&mut points, &cloud.world_bounds());
            }
        }
    }
    if let Some(meshes) = source.plan.meshes {
        for item in meshes {
            if let Some(mesh) = item.mesh
                && item.visible
                && !mesh.empty()
                && item.style != SurfaceStyle::Hidden
            {
                let space = mesh.bounds();
                if !space.empty() {
                    add_corners(
                        &mut points,
                        &Box2::new(
                            Point2::new(space.min.x, space.min.y),
                            Point2::new(space.max.x, space.max.y),
                        ),
                    );
                }
            }
        }
    }
    convex_hull(points)
}

/// What a plan or key plan viewport of `set` shows, as world points: its stretch of its
/// alignment, or the drawing as its hidden layers leave it (drawing_content); a key plan shows the
/// outlines of the sheets' plans, live, each automatic one placed as the painter places it, and
/// the drawing only when there is none. Empty for any other kind, or with nothing to show.
pub fn viewport_content(viewport: &Viewport, source: &SheetSource<'_>, set: &SheetSet) -> Vec<Point2> {
    let Some(model) = source.plan.model else {
        return Vec::new();
    };
    if !is_plan(viewport.kind) {
        return Vec::new();
    }
    if viewport.kind == ViewportKind::KeyPlan {
        // The outlines, each automatic plan where the painter puts it.
        let place = |plan: &Viewport| {
            let at = resolve_plan_viewport(plan, source);
            PlanPlacement {
                scale: at.scale,
                centre: at.centre,
            }
        };
        let outlines = viewport_content_in_set(model, set, viewport, Some(&place));
        let none = !set.sheets.iter().any(|sheet| {
            sheet
                .viewports
                .iter()
                .any(|v| v.kind == ViewportKind::Plan && !v.rect.empty())
        });
        if !none {
            return outlines;
        }
    }
    let mut points = content_of_viewport(model, viewport);
    // A plan of the drawing also shows the window's reference layers and meshes; a strip shows
    // its stretch of the alignment only.
    let strip = !viewport.source.alignment.is_empty()
        && model.alignments.find(&viewport.source.alignment).is_some();
    if !strip {
        points.extend(drawing_content(source, &viewport.hidden_layers));
    }
    convex_hull(points)
}

/// The scale a viewport of `set` is drawn at: an automatic plan's (or key plan's) as the painter
/// resolves it, an automatic section's as it is fitted, else its own.
pub fn drawn_scale_of(viewport: &Viewport, source: &SheetSource<'_>, set: &SheetSet) -> f64 {
    if is_section_view(viewport.kind) && viewport.auto_scale {
        // The scale the section is fitted to, as the painter fits it.
        let mut cache = SheetPaintCache::new();
        return resolve_section_viewport(viewport, source, &mut cache).scale;
    }
    if is_plan(viewport.kind) && viewport.auto_scale {
        // With the set and its sheet, so an automatic key plan is at the scale it is drawn at:
        // fitted to its outlines, not to the drawing.
        let on_sheet = set
            .sheets
            .iter()
            .position(|sheet| sheet.viewports.iter().any(|v| v.id == viewport.id));
        return match on_sheet {
            Some(index) => resolve_plan_viewport_on_sheet(viewport, source, set, index).scale,
            None => resolve_plan_viewport(viewport, source).scale,
        };
    }
    viewport.scale
}

// ---- the menus -----------------------------------------------------------------------------------

/// The Align commands: the edge, the menu's text, the object name.
const ALIGNS: [(AlignEdge, &str, &str); 6] = [
    (AlignEdge::Left, "Align Left", "sheetAlignLeft"),
    (AlignEdge::Right, "Align Right", "sheetAlignRight"),
    (AlignEdge::Top, "Align Top", "sheetAlignTop"),
    (AlignEdge::Bottom, "Align Bottom", "sheetAlignBottom"),
    (
        AlignEdge::HorizontalCentre,
        "Align Centres Horizontally",
        "sheetAlignHCentre",
    ),
    (
        AlignEdge::VerticalCentre,
        "Align Centres Vertically",
        "sheetAlignVCentre",
    ),
];

/// The dialogs the Arrange commands open: Choose Paper.
#[derive(Default)]
pub struct ArrangeDialogs {
    suggest_paper: Option<SuggestPaperDialog>,
}

/// Choose Paper for the editor's current sheet (sheetSuggestPaperDialog): the scale its main plan
/// is to be drawn at - As drawn, a standard scale or one typed (sheetSuggestPaperScale) - and the
/// paper that holds it at that scale, worked out as the scale changes (sheetSuggestPaperAdvice,
/// "A1 landscape, 83% full at 1:500") by SHEET SUGGESTPAPER itself, which changes nothing without
/// apply=on. Apply (sheetSuggestPaperApply) runs "SHEET SUGGESTPAPER n scale=... apply=on" through
/// the editor (run_line): one step. Cancel is sheetSuggestPaperCancel.
pub struct SuggestPaperDialog {
    frame: DialogFrame,
    sheet: String,
    note: String,
    pub scale: Choice,
    advice: String,
    apply_enabled: bool,
    /// The scale text the advice was worked out for.
    advised_for: Option<String>,
}

impl SuggestPaperDialog {
    fn items() -> Vec<String> {
        let mut items = vec![String::from(AS_DRAWN)];
        items.extend(SHEET_SCALES.iter().map(|scale| scale_words(*scale)));
        items
    }

    /// "SHEET SUGGESTPAPER n scale=..." for the choice; nothing for a scale that cannot be read.
    fn line(&self) -> Option<String> {
        let words = scale_option(&self.scale.current_text(&Self::items()))?;
        Some(format!("SHEET SUGGESTPAPER {} scale={words}", self.sheet))
    }

    /// What the advice says (sheetSuggestPaperAdvice).
    pub fn advice(&self) -> &str {
        &self.advice
    }

    pub fn apply_enabled(&self) -> bool {
        self.apply_enabled
    }

    /// What the scale box shows.
    pub fn scale_text(&self) -> String {
        self.scale.current_text(&Self::items())
    }

    /// One frame: Some(true) for Apply, Some(false) for Cancel.
    fn ui(&mut self, ctx: &egui::Context) -> Option<bool> {
        let mut frame = self.frame.clone();
        let mut answer = None;
        let shown = frame.show(
            ctx,
            &mut (&mut *self, &mut answer),
            |ui, (dialog, _)| {
                ui.label(egui::RichText::new(dialog.note.as_str()));
                egui::Grid::new("sheetSuggestPaperForm").num_columns(2).show(ui, |ui| {
                    ui.label("Scale");
                    let items = SuggestPaperDialog::items();
                    controls::choice(ui, "sheetSuggestPaperScale", &items, &mut dialog.scale);
                    ui.end_row();
                    ui.label("Paper");
                    controls::label_as(
                        ui,
                        "sheetSuggestPaperAdvice",
                        &dialog.advice,
                        egui::Label::new(dialog.advice.as_str()).wrap(),
                    );
                    ui.end_row();
                });
            },
            |ui, (dialog, answer)| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled_ui(dialog.apply_enabled, |ui| {
                            controls::button(ui, "sheetSuggestPaperApply", "Apply")
                        })
                        .inner
                        .clicked()
                    {
                        **answer = Some(true);
                    }
                    if controls::button(ui, "sheetSuggestPaperCancel", "Cancel").clicked() {
                        **answer = Some(false);
                    }
                });
            },
        );
        self.frame = frame;
        if shown.closed {
            answer = Some(false);
        }
        if self.advised_for.as_deref() != Some(self.scale_text().as_str()) {
            // The advice is worked out once the frame's drawing is done, and drawn at the next.
            ctx.request_repaint();
        }
        answer
    }
}

/// The sheet properties' "Choose paper for this scale" button (sheetChoosePaper), disabled on a
/// sheet with no plan: true when it is clicked.
pub fn choose_paper_button(ui: &mut egui::Ui, set: &SheetSet, index: usize) -> bool {
    let id = main_plan_of(set, index);
    let tip = if id.is_empty() {
        String::from("The sheet has no plan to choose paper for")
    } else {
        format!(
            "The smallest paper that holds {id} at the scale it is drawn at, the views kept in \
             proportion"
        )
    };
    ui.add_enabled_ui(!id.is_empty(), |ui| {
        controls::button(ui, "sheetChoosePaper", "Choose paper for this scale")
    })
    .inner
    .on_hover_text(tip)
    .clicked()
}

impl SheetEditor {
    fn current_of(&self, set: &SheetSet) -> Option<Sheet> {
        set.sheets.get(self.current_sheet()).cloned()
    }

    /// The sheet must exist for a command on it.
    fn sheet_to_arrange(&self, host: &dyn Host) -> Result<Sheet> {
        self.current_of(&host.document().sheet_set()).ok_or_else(|| {
            katana_core::Error::new(ErrorCode::InvalidState, "there is no sheet to arrange")
        })
    }

    /// The plan a Fit or Rotate acts on: the selected view, which must be a plan or key plan, or
    /// with nothing selected the sheet's main plan.
    fn plan_to_fit(&self, host: &dyn Host) -> Result<Viewport> {
        self.sheet_to_arrange(host)?;
        let set = host.document().sheet_set();
        let selected = self.canvas.selected();
        let id = if selected.is_empty() {
            main_plan_of(&set, self.current_sheet())
        } else {
            String::from(selected)
        };
        let Some(viewport) = find_viewport(&set, &id) else {
            return Err(katana_core::Error::new(
                ErrorCode::InvalidArgument,
                "the sheet has no plan: select one, or add one",
            ));
        };
        if !is_plan(viewport.kind) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "select a plan or key plan: only a view of the drawing is fitted and turned",
                viewport.id,
            ));
        }
        Ok(viewport)
    }

    /// The viewports the commands act on: the canvas's selection, or with nothing selected every
    /// placed viewport on the current sheet, in the sheet's order.
    pub fn arrange_targets(&self, host: &dyn Host) -> Vec<String> {
        let Some(sheet) = self.current_of(&host.document().sheet_set()) else {
            return Vec::new();
        };
        // The whole selection: a group chosen on the canvas is arranged as a group, whichever of
        // it is the primary.
        let selected = self.canvas.selected_ids();
        if !selected.is_empty() {
            return selected;
        }
        sheet
            .viewports
            .iter()
            .filter(|viewport| !viewport.rect.empty())
            .map(|viewport| viewport.id.clone())
            .collect()
    }

    // ---- the commands -------------------------------------------------------------------------

    pub fn arrange_sheet(&mut self, host: &mut dyn Host) -> Result<String> {
        self.sheet_to_arrange(&*host)?;
        let result = auto_arrange_sheet(host.document_mut(), self.current_sheet())?;
        let mut text = if result.moved.is_empty() {
            String::from("Nothing overlaps: the sheet is arranged already.")
        } else {
            format!(
                "Arranged the sheet: moved {}.",
                count_words(result.moved.len(), "view")
            )
        };
        if result.main_shrunk {
            text.push_str(" The main view was made smaller to make room.");
        }
        if !result.overlapping.is_empty() {
            text.push_str(&format!(
                " No room for {}: still overlapping.",
                list_words(&result.overlapping)
            ));
        }
        if !result.unplaced.is_empty() {
            text.push_str(&format!(
                " No room for {}: not placed.",
                list_words(&result.unplaced)
            ));
        }
        Ok(text)
    }

    pub fn align_selection(&mut self, host: &mut dyn Host, edge: AlignEdge) -> Result<String> {
        self.sheet_to_arrange(&*host)?;
        let ids = self.arrange_targets(&*host);
        let moved = align_viewports(host.document_mut(), self.current_sheet(), &ids, edge)?;
        if moved.is_empty() {
            return Ok(String::from("Nothing moved: the views are aligned already."));
        }
        Ok(format!(
            "Aligned {} {}.",
            count_words(moved.len(), "view"),
            edge_words(edge)
        ))
    }

    pub fn distribute_selection(
        &mut self,
        host: &mut dyn Host,
        axis: DistributeAxis,
    ) -> Result<String> {
        let sheet = self.sheet_to_arrange(&*host)?;
        let mut ids = self.arrange_targets(&*host);
        // A chosen group is spaced as it is; with fewer than three chosen there is nothing between
        // to space, and the views not chosen are not moved.
        let chosen = self.canvas.selected_ids().len();
        if chosen > 0 && chosen < 3 {
            return Err(katana_core::Error::new(
                ErrorCode::InvalidArgument,
                "choose three views or more to space evenly, or none for every view",
            ));
        }
        if ids.len() < 3 {
            ids = sheet
                .viewports
                .iter()
                .filter(|viewport| !viewport.rect.empty())
                .map(|viewport| viewport.id.clone())
                .collect();
        }
        let moved = distribute_viewports(host.document_mut(), self.current_sheet(), &ids, axis)?;
        if moved.is_empty() {
            return Ok(String::from(
                "Nothing moved: the views are evenly spaced already.",
            ));
        }
        Ok(format!(
            "Spaced {} evenly {} the sheet.",
            count_words(ids.len(), "view"),
            if axis == DistributeAxis::Horizontal {
                "across"
            } else {
                "up"
            }
        ))
    }

    /// The selection (every view drawn to a scale on the sheet when nothing is selected) takes the
    /// scale `from_id` is drawn at.
    pub fn match_selection_scale(&mut self, host: &mut dyn Host, from_id: &str) -> Result<String> {
        self.sheet_to_arrange(&*host)?;
        let set = host.document().sheet_set();
        let Some(from) = find_viewport(&set, from_id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no viewport of that id to take the scale from",
                from_id,
            ));
        };
        let (scale, exaggeration) = {
            let source = self.source(host.document());
            let scale = drawn_scale_of(&from, &source, &set);
            // An automatic section is matched at the exaggeration it is drawn at too.
            let exaggeration = (is_section_view(from.kind) && from.auto_scale).then(|| {
                let mut cache = SheetPaintCache::new();
                resolve_section_viewport(&from, &source, &mut cache).exaggeration
            });
            (scale, exaggeration)
        };
        let ids = self.arrange_targets(&*host);
        let changed = match_scale(host.document_mut(), &ids, from_id, Some(scale), exaggeration)?;
        if changed.is_empty() {
            return Ok(format!(
                "Nothing changed: the views are at {} already, or have no scale.",
                scale_words(scale)
            ));
        }
        Ok(format!(
            "Gave {} the scale of {from_id}, {}.",
            list_words(&changed),
            scale_words(scale)
        ))
    }

    /// The selected plan or key plan, else the sheet's main plan, sized so what it shows just fits
    /// at its scale.
    pub fn fit_selection_to_content(&mut self, host: &mut dyn Host) -> Result<String> {
        let viewport = self.plan_to_fit(&*host)?;
        let set = host.document().sheet_set();
        let (content, scale) = {
            let source = self.source(host.document());
            (
                viewport_content(&viewport, &source, &set),
                drawn_scale_of(&viewport, &source, &set),
            )
        };
        if content.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the view shows nothing to fit to",
                viewport.id,
            ));
        }
        fit_viewport_to_content(host.document_mut(), &viewport.id, &content, Some(scale))?;
        let fitted = find_viewport(&host.document().sheet_set(), &viewport.id).unwrap_or(viewport);
        Ok(format!(
            "Sized {} to what it shows: {:.1} x {:.1} mm at {}.",
            fitted.id,
            fitted.rect.width(),
            fitted.rect.height(),
            scale_words(scale)
        ))
    }

    /// The selected plan or key plan, else the sheet's main plan, turned as far as it takes to
    /// show the drawing at the largest scale.
    pub fn rotate_selection_to_best_fit(&mut self, host: &mut dyn Host) -> Result<String> {
        let plan = self.plan_to_fit(&*host)?;
        let set = host.document().sheet_set();
        let content = viewport_content(&plan, &self.source(host.document()), &set);
        if content.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the view shows nothing to fit to",
                plan.id,
            ));
        }
        let fit = rotate_to_best_fit(host.document_mut(), &plan.id, &content)?;
        if fit.rotation == 0.0 {
            return Ok(format!(
                "{} shows most square to the paper: north up at {}.",
                plan.id,
                scale_words(fit.standard_scale)
            ));
        }
        Ok(format!(
            "Turned {} to {:.2} degrees, at {}.",
            plan.id,
            fit.rotation * RAD_TO_DEG,
            scale_words(fit.standard_scale)
        ))
    }

    /// What a command did, on the status bar and in the log.
    fn announce(&mut self, host: &mut dyn Host, outcome: Result<String>) {
        match outcome {
            Ok(text) => self.report(host, &text, false),
            Err(error) => self.report(host, &error.describe(), true),
        }
        self.observe(&*host);
    }

    // ---- the menus ----------------------------------------------------------------------------

    /// The Arrange menu (sheetArrangeMenu): the commands by their stable names - sheetArrangeAuto,
    /// sheetAlignLeft, sheetAlignRight, sheetAlignTop, sheetAlignBottom, sheetAlignHCentre,
    /// sheetAlignVCentre, sheetDistributeHorizontal, sheetDistributeVertical, sheetMatchScaleMenu
    /// (its items sheetMatchScale_<viewport id>, or sheetMatchScaleNone, listed as the menu is
    /// made), sheetFitToContent and sheetRotateToBestFit.
    pub fn arrange_menu(&self, host: &dyn Host) -> Menu {
        let mut menu = Menu::new("sheetArrangeMenu", "Arrange").action(
            Action::new("sheetArrangeAuto", "Auto Arrange")
                .tip("Move the views apart so none overlaps, the main view keeping its place"),
        );
        menu = menu.separator();
        for (_, text, name) in ALIGNS {
            menu = menu.action(Action::new(name, text).tip(
                "The selected view to the tiling area, or with nothing selected every view to the \
                 others",
            ));
        }
        menu = menu
            .separator()
            .action(
                Action::new("sheetDistributeHorizontal", "Distribute Horizontally")
                    .tip("Equal gaps across the sheet between the views"),
            )
            .action(
                Action::new("sheetDistributeVertical", "Distribute Vertically")
                    .tip("Equal gaps up the sheet between the views"),
            )
            .separator()
            .submenu(self.match_scale_menu(host))
            .action(
                Action::new("sheetFitToContent", "Fit View to Content").tip(
                    "Size the selected plan (or the main plan) so what it shows just fits at its \
                     scale",
                ),
            )
            .action(
                Action::new("sheetRotateToBestFit", "Rotate to Best Fit").tip(
                    "Turn the selected plan (or the main plan) as far as it takes to show the \
                     drawing at the largest scale",
                ),
            );
        menu
    }

    /// The Match Scale To list (sheetMatchScaleMenu): every view drawn to a scale in the set but
    /// the selected one, by sheet, with the scale it is drawn at.
    pub fn match_scale_menu(&self, host: &dyn Host) -> Menu {
        let mut menu = Menu::new("sheetMatchScaleMenu", "Match Scale To...");
        let document: &Document = host.document();
        let set = document.sheet_set();
        let selected = self.canvas.selected();
        let source = self.source(document);
        for (s, sheet) in set.sheets.iter().enumerate() {
            for viewport in &sheet.viewports {
                if viewport.id == selected || !has_scale(viewport.kind) {
                    continue;
                }
                let title = if viewport.title.is_empty() {
                    String::from(viewport.kind.name())
                } else {
                    viewport.title.clone()
                };
                let on = if sheet.name.is_empty() {
                    format!("sheet {}", s + 1)
                } else {
                    sheet.name.clone()
                };
                let text = format!(
                    "{}  {title}  ({on}, {})",
                    viewport.id,
                    scale_words(drawn_scale_of(viewport, &source, &set))
                );
                menu = menu.action(Action::new(&format!("sheetMatchScale_{}", viewport.id), &text));
            }
        }
        if menu.items.is_empty() {
            menu = menu.action(
                Action::new("sheetMatchScaleNone", "(no other view is drawn to a scale)")
                    .enabled(false),
            );
        }
        menu
    }

    /// The Arrange commands triggered by name; false for a name that is none of them.
    pub(crate) fn trigger_arrange(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let outcome = if let Some(id) = name.strip_prefix("sheetMatchScale_") {
            self.match_selection_scale(host, id)
        } else if let Some((edge, _, _)) = ALIGNS.iter().find(|(_, _, known)| *known == name) {
            self.align_selection(host, *edge)
        } else {
            match name {
                "sheetArrangeAuto" => self.arrange_sheet(host),
                "sheetDistributeHorizontal" => {
                    self.distribute_selection(host, DistributeAxis::Horizontal)
                }
                "sheetDistributeVertical" => {
                    self.distribute_selection(host, DistributeAxis::Vertical)
                }
                "sheetFitToContent" => self.fit_selection_to_content(host),
                "sheetRotateToBestFit" => self.rotate_selection_to_best_fit(host),
                _ => return false,
            }
        };
        self.announce(host, outcome);
        true
    }

    // ---- Choose Paper -------------------------------------------------------------------------

    /// Choose Paper for the current sheet, opened and not waited on (sheetSuggestPaperDialog).
    pub fn open_suggest_paper_dialog(&mut self, host: &mut dyn Host) -> &mut SuggestPaperDialog {
        self.observe(&*host);
        let set = host.document().sheet_set();
        let id = main_plan_of(&set, self.current_sheet());
        let mut dialog = SuggestPaperDialog {
            frame: DialogFrame::new("sheetSuggestPaperDialog", "Choose Paper for a Scale"),
            sheet: (self.current_sheet() + 1).to_string(),
            note: format!(
                "The smallest paper that holds {} at the scale, the other views kept in \
                 proportion.",
                if id.is_empty() { "the plan" } else { id.as_str() }
            ),
            scale: Choice {
                index: Some(0),
                text: String::from(AS_DRAWN),
                editable: true,
            },
            advice: String::new(),
            apply_enabled: false,
            advised_for: None,
        };
        self.advise(host, &mut dialog);
        self.arrange.suggest_paper.insert(dialog)
    }

    pub fn suggest_paper_dialog(&mut self) -> Option<&mut SuggestPaperDialog> {
        self.arrange.suggest_paper.as_mut()
    }

    /// What the verb advises without apply=on - the answer Apply gives, worked out by the same
    /// code on the same content - and nothing changed.
    fn advise(&mut self, host: &mut dyn Host, dialog: &mut SuggestPaperDialog) {
        dialog.advised_for = Some(dialog.scale_text());
        let Some(line) = dialog.line() else {
            dialog.advice = String::from("A scale is 1:500, 500 or As drawn.");
            dialog.apply_enabled = false;
            return;
        };
        let source = Rc::clone(&self.source);
        let provider = move |document: &Document| sheet_verb_context_for(document, &source);
        let reply = CommandInterpreter::tokenize(&line)
            .and_then(|tokens| run_sheet_verb(host.document_mut(), &tokens, Some(&provider)));
        match reply {
            Ok(reply) => {
                dialog.advice = advice_words(&reply);
                dialog.apply_enabled = true;
            }
            Err(error) => {
                dialog.advice = error.message;
                dialog.apply_enabled = false;
            }
        }
    }

    /// The Arrange commands' dialogs, a frame each.
    pub(crate) fn arrange_dialogs_ui(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        let Some(mut dialog) = self.arrange.suggest_paper.take() else {
            return;
        };
        let answer = dialog.ui(ctx);
        if dialog.advised_for.as_deref() != Some(dialog.scale_text().as_str()) {
            self.advise(host, &mut dialog);
        }
        match answer {
            None => self.arrange.suggest_paper = Some(dialog),
            Some(false) => {}
            Some(true) => {
                if let Some(line) = dialog.line() {
                    let _ = self.run_line(host, &format!("{line} apply=on"));
                }
            }
        }
    }
}
