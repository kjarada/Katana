//! BIM > Road Template Manager... (docs/road_design.md, "In the window"): the drawing's road
//! templates - their components out from the centreline, each component's pavement courses - and a
//! drawing of the template across a level road. The port of bim/road_template_manager.
//!
//! NOTHING IS CHANGED HERE. Every button builds the ROADTEMPLATE line a person would type and hands
//! it to the window's one executor (host::Host::run), so it is echoed, kept in the history and one
//! undo step; an agent types the same line. The two grids are buffers, as the Alignment Manager's
//! are: their cells are edited in place and applied by Apply as ONE line for the grid (a group of
//! key=value for each edited row), so an Apply is one undo step and a value the verb refuses leaves
//! the template as it was, with the edits still in the grid; never by a table's own change. What
//! the dialog shows is read from Document::road_templates, and the preview is cad's own section of
//! it (road::road_section::template_section), so the drawing and ROADTEMPLATE SECTION agree. Edits
//! survive a reload that changed nothing the grid shows, and nothing else: not the choice of
//! another template, not an undo or a typed line that changed the component.
//!
//! NON-MODAL, kept by the window between uses; it hears the Document through a DocumentWatcher, so
//! a line typed on the command line, an undo or an opened project reloads it, keeping the chosen
//! template and component.
//!
//! Object names, which the widget tests and the headless steps drive it by:
//!
//! ```text
//! roadTemplateManagerDialog   the dialog (the action bimRoadTemplates' data)
//! roadTemplateList            the templates: name, left, right, courses, description
//! roadTemplateName            the name New, Copy and Rename use
//! roadTemplateNew             ROADTEMPLATE NEW "name"
//! roadTemplateCopy            ROADTEMPLATE NEW "name" FROM <chosen>
//! roadTemplateRename          ROADTEMPLATE RENAME <chosen> "name"
//! roadTemplateDelete          ROADTEMPLATE DELETE <chosen>
//! roadTemplateDefaults        ROADTEMPLATE DEFAULTS
//! roadTemplateFile            the file Export writes and Import reads (a text box: a headless
//!                             run has no file dialog)
//! roadTemplateBrowse          a file dialog for it (none in a headless run)
//! roadTemplateOverwrite       replace what is there: a file on Export, the templates of the same
//!                             name on Import (the line gets OVERWRITE)
//! roadTemplateExport          ROADTEMPLATE EXPORT <chosen> "file" [OVERWRITE]
//! roadTemplateExportAll       ROADTEMPLATE EXPORT ALL "file" [OVERWRITE]
//! roadTemplateImport          ROADTEMPLATE IMPORT "file" [OVERWRITE]
//! roadTemplateComponents      side, name, kind, width, crossfall, layer, faces (a kerb's: inward
//!                             or outward), and what else it is; width, crossfall, layer and a
//!                             kerb's faces edited in place
//! roadTemplateApplyComponents ROADTEMPLATE SET <t> side:name width= crossfall= layer= [faces=]
//!                             [side:name ...], one line for every row edited
//! roadTemplateRevertComponents  the grid as the drawing holds it again
//! roadTemplateAddSide         left, right or both
//! roadTemplateAddKind         lane, shoulder, kerb, footpath, verge, median, drain, batter
//! roadTemplateAdd             ROADTEMPLATE ADD <t> <side> <kind>
//! roadTemplateRemove          ROADTEMPLATE REMOVE <t> side:name (the chosen row)
//! roadTemplateComponentKeys   key=value ... for the chosen component (shape=, cut=, ...)
//! roadTemplateSetKeys         ROADTEMPLATE SET <t> side:name <the keys>
//! roadTemplateCourses         the chosen component's courses, top down: name, thickness,
//!                             material, IFC type - the last three edited
//! roadTemplateApplyCourses    ROADTEMPLATE COURSE <t> side:name SET <course> ... [<course> ...],
//!                             one line for every row edited
//! roadTemplateCourseName, roadTemplateCourseThickness, roadTemplateCourseMaterial
//! roadTemplateCourseAdd       ROADTEMPLATE COURSE <t> side:name ADD <course> thickness=
//!                             material=
//! roadTemplateCourseRemove    ROADTEMPLATE COURSE <t> side:name REMOVE <course>
//! roadTemplatePreview         the template across a level road, its courses coloured
//! roadTemplateStatus          what the last line replied, or why it was refused
//! roadTemplateClose
//! ```
//!
//! Not carried over: the grids' cell colours (a component's side in its colour, a course's name on
//! its colour), which the shared table does not draw; the template's colours are in the preview.

use std::collections::BTreeSet;

use egui::Ui;
use katana_cad::Document;
use katana_cad::road::road_section::{RoadSection, template_section};
use katana_cad::road::road_template::{
    Component, ComponentKind, KerbShape, PavementCourse, RoadTemplate, Side, kerb_width, side_from,
};
use katana_core::text::format_general;
use katana_entity::EnumName;
use katana_math::cpp;

use super::road_dialog_support::{
    Cell, Combo, PreviewRaster, StatusLine, Table, TableEvents, button, cell, editable, field,
    note, option, preview, rows_height, same_ignoring_case, save_file_dialog, word,
};
use crate::canvas::{
    Brush, Canvas, Color, FillRule, HAlign, Path, Pen, PenCap, PenJoin, PenStyle, PointF, RectF,
    RenderHint, TextFlags, VAlign,
};
use crate::command_word::exact_number;
use crate::customisation::document_watcher::DocumentWatcher;
use crate::drive::controls::check;
use crate::format::qt_fixed;
use crate::host::Host;
use crate::shell::dialog::DialogFrame;
use crate::theme::palette;

/// The components grid's columns.
pub const C_SIDE: usize = 0;
pub const C_NAME: usize = 1;
pub const C_KIND: usize = 2;
pub const C_WIDTH: usize = 3;
pub const C_CROSSFALL: usize = 4;
pub const C_LAYER: usize = 5;
pub const C_FACES: usize = 6;
pub const C_DETAIL: usize = 7;

/// The courses grid's columns.
pub const K_NAME: usize = 0;
pub const K_THICKNESS: usize = 1;
pub const K_MATERIAL: usize = 2;
pub const K_IFC: usize = 3;

/// Rows of each grid the dialog always leaves room for. Left to the layout, a template's twelve
/// components and a lane's three courses were each shown through a single row at the dialog's
/// opening size: the template list, the preview and six rows of controls took the room. A dialog
/// taller than the screen is fitted to it by the dialog frame (shell::dialog), which scrolls what
/// does not fit, so asking for this is safe on a laptop.
pub const GRID_ROWS: f32 = 4.0;

/// The room a grid keeps: its header, GRID_ROWS rows, and half a row more, which says that the
/// grid scrolls (heightForRows).
pub fn grid_height(style: &egui::Style) -> f32 {
    rows_height(style, 1.0 + GRID_ROWS + 0.5)
}

/// QString::number(value, 'f', 3).
fn fixed3(value: f64) -> String {
    qt_fixed(value, 3)
}

/// QString::arg(double): printf's %g at six digits.
fn arg(value: f64) -> String {
    format_general(value, 6)
}

fn colour_of(colour: &katana_entity::Color) -> Color {
    Color::rgba(colour.r, colour.g, colour.b, colour.a)
}

/// What else a component is, in a word or two, for the grid's last column.
pub fn detail_of(component: &Component) -> String {
    match component.kind {
        ComponentKind::Kerb => format!(
            "{}, {} m high, {} m wide",
            component.shape.name(),
            fixed3(component.height),
            fixed3(kerb_width(component))
        ),
        ComponentKind::Drain => format!("invert {} m down", fixed3(component.depth)),
        ComponentKind::Batter => {
            let mut text = format!(
                "cut {}:1, fill {}:1, to {} m",
                arg(component.cut_slope),
                arg(component.fill_slope),
                arg(component.maximum_width)
            );
            if component.bench_height > 0.0 {
                text.push_str(&format!(
                    ", benches {} m every {} m",
                    arg(component.bench_width),
                    arg(component.bench_height)
                ));
            }
            text
        }
        _ => format!(
            "{} courses{}",
            component.courses.len(),
            if component.superelevated {
                ", superelevated"
            } else {
                ""
            }
        ),
    }
}

/// The template across a level road (road::road_section::template_section), drawn as a person
/// reads a typical section: the courses and kerbs as coloured solids, the finished surface over
/// them, a batter as a dashed stub falling at its fill slope. The vertical is exaggerated, as a
/// section's is, so a 50 mm course can be seen at all; the factor is printed. `rect` is the
/// preview's, in the canvas's coordinates (its paintEvent).
pub fn paint_template_preview(
    canvas: &mut dyn Canvas,
    rect: RectF,
    road_template: Option<&RoadTemplate>,
    section: Option<&RoadSection>,
) {
    canvas.fill_rect(rect, &Brush::solid(palette::panel()));
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    let (Some(road_template), Some(section)) = (road_template, section) else {
        canvas.set_pen(Pen::new(palette::text_muted()));
        let said = if road_template.is_some() {
            "No components yet: Add one below"
        } else {
            "No template chosen"
        };
        canvas.draw_text_in(rect, TextFlags::CENTER, said);
        return;
    };
    if section.top.len() < 2 {
        canvas.set_pen(Pen::new(palette::text_muted()));
        canvas.draw_text_in(rect, TextFlags::CENTER, "No components yet: Add one below");
        return;
    }
    let mut left: f64 = 0.0;
    let mut right: f64 = 0.0;
    let mut low: f64 = 0.0;
    let mut high: f64 = 0.0;
    for point in section.top.iter().chain(section.formation.iter()) {
        left = cpp::max(left, point.offset);
        right = cpp::min(right, point.offset);
        low = cpp::min(low, point.level);
        high = cpp::max(high, point.level);
    }
    // Room for a batter's stub each side.
    left += 2.0;
    right -= 2.0;
    low -= 0.5;
    high += 0.1;
    let frame = rect.adjusted(12.0, 12.0, -12.0, -24.0);
    let scale = frame.width / cpp::max(left - right, 1e-6);
    let exaggeration = cpp::clamp(
        frame.height / cpp::max((high - low) * scale, 1e-9),
        1.0,
        10.0,
    );
    let centre_y = frame.center().y;
    let middle = 0.5 * (low + high);
    // Offsets grow to the left: the left side is drawn on the left.
    let at = |offset: f64, level: f64| {
        PointF::new(
            frame.left() + (left - offset) * scale,
            centre_y - (level - middle) * scale * exaggeration,
        )
    };
    for solid in &section.solids {
        let Some(component) = road_template.components.get(solid.component) else {
            continue;
        };
        let fill = match solid
            .course
            .and_then(|course| component.courses.get(course))
        {
            Some(course) => colour_of(&course.colour),
            None => colour_of(&component.colour),
        };
        let mut polygon: Vec<PointF> = solid
            .columns
            .iter()
            .map(|column| at(column.offset, column.top))
            .collect();
        polygon.extend(
            solid
                .columns
                .iter()
                .rev()
                .map(|column| at(column.offset, column.bottom)),
        );
        canvas.set_pen(Pen::with(
            palette::border(),
            0.8,
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Bevel,
        ));
        canvas.set_brush(Brush::solid(fill));
        canvas.draw_polygon(&polygon, FillRule::OddEven);
    }
    canvas.set_brush(Brush::none());
    let mut top = Path::new();
    let first = &section.top[0];
    top.move_to(at(first.offset, first.level));
    for point in &section.top {
        top.line_to(at(point.offset, point.level));
    }
    canvas.set_pen(Pen::with(
        palette::accent(),
        2.0,
        PenStyle::Solid,
        PenCap::Square,
        PenJoin::Bevel,
    ));
    canvas.draw_path(&top);
    // A batter's stub: 2 m out at the side's fill slope.
    for side in [Side::Left, Side::Right] {
        let components = road_template.side(side);
        let Some(last) = components.last() else {
            continue;
        };
        if last.kind != ComponentKind::Batter {
            continue;
        }
        let hinge = if side == Side::Left {
            &section.top[0]
        } else {
            &section.top[section.top.len() - 1]
        };
        let sign = if side == Side::Left { 1.0 } else { -1.0 };
        canvas.set_pen(Pen::with(
            palette::text_muted(),
            1.0,
            PenStyle::Dash,
            PenCap::Square,
            PenJoin::Bevel,
        ));
        canvas.draw_line(
            at(hinge.offset, hinge.level),
            at(
                hinge.offset + sign * 2.0,
                hinge.level - 2.0 / last.fill_slope,
            ),
        );
    }
    canvas.set_pen(Pen::new(palette::text_muted()));
    canvas.draw_text_in(
        rect.adjusted(8.0, 0.0, -8.0, -4.0),
        TextFlags::aligned(HAlign::Right, VAlign::Bottom),
        &format!("vertical x{}", qt_fixed(exaggeration, 1)),
    );
}

/// What the courses grid shows: of which template, of which component of it, and those courses as
/// the drawing held them. The component's reference alone (left:lane) is the same in every template
/// that has one, so it cannot tell a grid filled from another template's from one still current.
#[derive(Clone, Debug, PartialEq)]
struct FilledCourses {
    template_name: String,
    reference: String,
    courses: Vec<PavementCourse>,
}

/// The template a reference side:name names in `road_template`.
fn component_of<'a>(road_template: &'a RoadTemplate, reference: &str) -> Option<&'a Component> {
    let (side, name) = reference.split_once(':')?;
    let side = side_from(side)?;
    let index = road_template.find(side, name)?;
    road_template.components.get(index)
}

struct Body {
    watcher: DocumentWatcher,
    executor: bool,
    list: Table,
    name: String,
    file: String,
    overwrite: bool,
    components: Table,
    add_side: Combo,
    add_kind: Combo,
    keys: String,
    courses: Table,
    course_name: String,
    course_thickness: String,
    course_material: String,
    /// What the preview shows: the template and its section across a level road.
    preview_template: Option<RoadTemplate>,
    preview_section: Option<RoadSection>,
    raster: Option<PreviewRaster>,
    texture: Option<egui::TextureHandle>,
    status: StatusLine,
    /// True while a grid is refilled, when its change is the dialog's own doing and not an edit.
    loading: bool,
    /// The rows edited since each grid was filled, and what it was filled from: a reload that
    /// changed nothing it shows keeps the edits.
    edited_components: BTreeSet<usize>,
    edited_courses: BTreeSet<usize>,
    filled_from: Option<RoadTemplate>,
    filled_courses: Option<FilledCourses>,
    close: bool,
}

impl Body {
    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    fn current_template(&self) -> String {
        self.list
            .current_row()
            .map(|row| self.list.text(row, 0))
            .unwrap_or_default()
    }

    fn current(&self, document: &Document) -> Option<RoadTemplate> {
        if !self.alive() {
            return None;
        }
        let chosen = self.current_template();
        if chosen.is_empty() {
            return None;
        }
        document.road_templates().find(&chosen).cloned()
    }

    /// The chosen component's reference, side:name; empty for none.
    fn chosen_component(&self) -> String {
        match self.components.current_row() {
            Some(row) if self.components.cell(row, C_SIDE).is_some() => format!(
                "{}:{}",
                self.components.text(row, C_SIDE),
                self.components.text(row, C_NAME)
            ),
            _ => String::new(),
        }
    }

    fn template_word(&mut self) -> Option<String> {
        let chosen = self.current_template();
        if chosen.is_empty() {
            self.status
                .set("Choose a template first, or make one with New.", true);
            return None;
        }
        word(&mut self.status, &chosen, "the template's name")
    }

    fn run(&mut self, host: &mut dyn Host, line: &str) -> bool {
        if !self.alive() {
            self.status
                .set("The drawing this dialog edited is gone.", true);
            return false;
        }
        let ok = super::road_dialog_support::run(host, self.executor, &mut self.status, line);
        self.reload(host.document());
        ok
    }

    /// The file field as the line takes it; None, said on the status line, when it is empty or
    /// cannot be written in a line.
    fn file_word(&mut self) -> Option<String> {
        let text = String::from(self.file.trim());
        if text.is_empty() {
            self.status.set("Type the file, or Browse for it.", true);
            return None;
        }
        word(&mut self.status, &text, "the file")
    }

    /// ROADTEMPLATE EXPORT <what> "file" [OVERWRITE].
    fn export_to(&mut self, host: &mut dyn Host, what: &str) {
        if let Some(path) = self.file_word() {
            let overwrite = if self.overwrite { " OVERWRITE" } else { "" };
            self.run(
                host,
                &format!("ROADTEMPLATE EXPORT {what} {path}{overwrite}"),
            );
        }
    }

    /// The template `name` chosen (ignoring case), its handler run unless loading.
    fn select_named(&mut self, document: &Document, name: &str) -> bool {
        let Some(row) = (0..self.list.row_count())
            .find(|&row| same_ignoring_case(&self.list.text(row, 0), name))
        else {
            return false;
        };
        if self.list.select_row(row) {
            self.list_selection_changed(document);
        }
        true
    }

    fn list_selection_changed(&mut self, document: &Document) {
        if !self.loading {
            // Another template: the edits made to the last one's grids are not edits of this one.
            self.edited_components.clear();
            self.edited_courses.clear();
            self.fill_components(document, true);
        }
    }

    fn components_selection_changed(&mut self, document: &Document) {
        if !self.loading {
            self.fill_courses(document);
        }
    }

    fn set_preview(&mut self, road_template: Option<RoadTemplate>) {
        self.preview_section = road_template
            .as_ref()
            .and_then(|each| template_section(each).ok());
        self.preview_template = road_template;
    }

    fn reload(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let chosen = self.current_template();
        let set = document.road_templates();
        self.loading = true;
        self.list.clear();
        for each in &set.templates {
            let course_count: usize = each
                .components
                .iter()
                .map(|component| component.courses.len())
                .sum();
            self.list.push_row(vec![
                cell(each.name.as_str()),
                cell(format!("{}", each.side(Side::Left).len())),
                cell(format!("{}", each.side(Side::Right).len())),
                cell(format!("{course_count}")),
                cell(each.description.as_str()),
            ]);
        }
        // Chosen again quietly: a reload is not a choice, and keeps the edits.
        if !chosen.is_empty() {
            self.select_named(document, &chosen);
        }
        self.loading = false;
        self.fill_components(document, false);
        if let Err(error) = document.road_templates_status() {
            self.status.set(error.describe(), true);
        }
    }

    fn fill_components(&mut self, document: &Document, force: bool) {
        let road_template = self.current(document);
        self.set_preview(road_template.clone());
        // Unapplied edits survive a reload that did not change the template.
        if !force
            && !self.edited_components.is_empty()
            && road_template.is_some()
            && self.filled_from == road_template
        {
            return;
        }
        let keep = self.chosen_component();
        self.loading = true;
        self.components.clear();
        self.edited_components.clear();
        self.filled_from = None;
        if let Some(road_template) = &road_template {
            self.filled_from = Some(road_template.clone());
            for side in [Side::Left, Side::Right] {
                for each in road_template.side(side) {
                    let has_width = each.kind != ComponentKind::Batter;
                    let has_crossfall = each.kind != ComponentKind::Batter
                        && (each.kind != ComponentKind::Kerb || each.shape == KerbShape::EdgeStrip);
                    let has_faces =
                        each.kind == ComponentKind::Kerb && each.shape != KerbShape::EdgeStrip;
                    let width = if has_width {
                        exact_number(each.width)
                    } else {
                        String::new()
                    };
                    let crossfall = if has_crossfall {
                        exact_number(each.crossfall)
                    } else {
                        String::new()
                    };
                    let faces = if has_faces {
                        String::from(each.faces.name())
                    } else {
                        String::new()
                    };
                    self.components.push_row(vec![
                        cell(side.name()),
                        cell(each.name.as_str()),
                        cell(each.kind.name()),
                        Cell::new(width, has_width),
                        Cell::new(crossfall, has_crossfall),
                        editable(each.layer.as_str()),
                        Cell::new(faces, has_faces),
                        cell(detail_of(each)),
                    ]);
                }
            }
        }
        self.loading = false;
        let found = (0..self.components.row_count()).find(|&row| {
            format!(
                "{}:{}",
                self.components.text(row, C_SIDE),
                self.components.text(row, C_NAME)
            ) == keep
        });
        if let Some(row) = found
            && self.components.select_row(row)
        {
            self.components_selection_changed(document);
        }
        self.fill_courses(document);
    }

    fn fill_courses(&mut self, document: &Document) {
        let road_template = self.current(document);
        let reference = self.chosen_component();
        let chosen = road_template
            .as_ref()
            .and_then(|each| component_of(each, &reference))
            .cloned();
        // Unapplied edits survive only a reload that changed nothing the grid shows: the same
        // template, the same component and the same courses. An undo, a typed line or an opened
        // project that changed them shows what the drawing holds, as the components grid does.
        if !self.edited_courses.is_empty()
            && let (Some(road_template), Some(chosen), Some(filled)) =
                (&road_template, &chosen, &self.filled_courses)
            && filled.template_name == road_template.name
            && filled.reference == reference
            && filled.courses == chosen.courses
        {
            return;
        }
        self.loading = true;
        self.courses.clear();
        self.edited_courses.clear();
        self.filled_courses = None;
        if let (Some(road_template), Some(chosen)) = (&road_template, &chosen) {
            self.filled_courses = Some(FilledCourses {
                template_name: road_template.name.clone(),
                reference: reference.clone(),
                courses: chosen.courses.clone(),
            });
            for course in &chosen.courses {
                self.courses.push_row(vec![
                    cell(course.name.as_str()),
                    editable(exact_number(course.thickness)),
                    editable(course.material.as_str()),
                    editable(course.ifc_type.as_str()),
                ]);
            }
        }
        self.loading = false;
    }

    fn apply_components(&mut self, host: &mut dyn Host) {
        let Some(template_word) = self.template_word() else {
            return;
        };
        if self.edited_components.is_empty() {
            self.status.set("Nothing is edited to apply.", false);
            return;
        }
        // ONE line for every edited row, a group each (ROADTEMPLATE SET <t> <c> key=value ...
        // <c> key=value ...): one undo step, and a value the verb refuses - a width of 0 - changes
        // none of the rows, where a line a row left the rows before it applied. A cell that is not
        // one number is refused here, before the line is built, naming it.
        let mut line = format!("ROADTEMPLATE SET {template_word}");
        let mut any = false;
        let rows: Vec<usize> = self.edited_components.iter().copied().collect();
        for row in rows {
            if self.components.cell(row, C_SIDE).is_none() {
                continue;
            }
            let reference = format!(
                "{}:{}",
                self.components.text(row, C_SIDE),
                self.components.text(row, C_NAME)
            );
            let mut group = reference.clone();
            for (column, key) in [(C_WIDTH, "width"), (C_CROSSFALL, "crossfall")] {
                let Some(item) = self.components.cell(row, column) else {
                    continue;
                };
                if !item.editable {
                    continue;
                }
                let text = item.text.clone();
                if super::road_dialog_support::number(&text).is_none() {
                    self.status.set(
                        format!(
                            "{reference}: the {key} '{text}' is not a number; type one number, \
                             with a decimal point"
                        ),
                        true,
                    );
                    return;
                }
                group.push_str(&format!(" {key}={}", text.trim()));
            }
            // A kerb's facing, where it has one (a flush edge strip has no face): inward or
            // outward, which the verb checks and names if it is neither.
            if let Some(facing) = self.components.cell(row, C_FACES)
                && facing.editable
            {
                let facing = String::from(facing.text.trim());
                let Some(faces) = option(&mut self.status, "faces", &facing) else {
                    return;
                };
                group.push(' ');
                group.push_str(&faces);
            }
            let layer_text = String::from(self.components.text(row, C_LAYER).trim());
            let Some(layer) = option(&mut self.status, "layer", &layer_text) else {
                return;
            };
            line.push_str(&format!(" {group} {layer}"));
            any = true;
        }
        if !any {
            self.status.set("Nothing is edited to apply.", false);
            return;
        }
        // Refused: the template is as it was, so the reload keeps the edits for the person to
        // correct.
        if !super::road_dialog_support::run(host, self.executor, &mut self.status, &line) {
            self.reload(host.document());
            return;
        }
        self.edited_components.clear();
        self.reload(host.document());
    }

    fn apply_courses(&mut self, host: &mut dyn Host) {
        let template_word = self.template_word();
        let reference = self.chosen_component();
        let Some(template_word) = template_word else {
            return;
        };
        if reference.is_empty() || self.edited_courses.is_empty() {
            self.status.set("Nothing is edited to apply.", false);
            return;
        }
        // One line for every edited course, as apply_components does: ROADTEMPLATE COURSE <t> <c>
        // SET <course> key=value ... <course> key=value ...
        let mut line = format!("ROADTEMPLATE COURSE {template_word} {reference} SET");
        let mut any = false;
        let rows: Vec<usize> = self.edited_courses.iter().copied().collect();
        for row in rows {
            if self.courses.cell(row, K_NAME).is_none() {
                continue;
            }
            let name = self.courses.text(row, K_NAME);
            let thickness = String::from(self.courses.text(row, K_THICKNESS).trim());
            if super::road_dialog_support::number(&thickness).is_none() {
                self.status.set(
                    format!("{name}: the thickness '{thickness}' is not a number"),
                    true,
                );
                return;
            }
            let material_text = String::from(self.courses.text(row, K_MATERIAL).trim());
            let ifc_text = String::from(self.courses.text(row, K_IFC).trim());
            let material = option(&mut self.status, "material", &material_text);
            let ifc = option(&mut self.status, "ifc", &ifc_text);
            let (Some(material), Some(ifc)) = (material, ifc) else {
                return;
            };
            line.push_str(&format!(" {name} thickness={thickness} {material} {ifc}"));
            any = true;
        }
        if !any {
            self.status.set("Nothing is edited to apply.", false);
            return;
        }
        if !super::road_dialog_support::run(host, self.executor, &mut self.status, &line) {
            // Refused, so nothing changed: the edits stay for the person to correct.
            self.reload(host.document());
            return;
        }
        self.edited_courses.clear();
        self.reload(host.document());
    }

    // ---- the buttons ----------------------------------------------------------------------------

    fn press_new(&mut self, host: &mut dyn Host) {
        let typed = String::from(self.name.trim());
        if let Some(new_word) = word(&mut self.status, &typed, "the new name")
            && self.run(host, &format!("ROADTEMPLATE NEW {new_word}"))
        {
            let named = String::from(self.name.trim());
            self.select_named(host.document(), &named);
        }
    }

    fn press_copy(&mut self, host: &mut dyn Host) {
        let from = self.template_word();
        let typed = String::from(self.name.trim());
        let new_word = word(&mut self.status, &typed, "the new name");
        if let (Some(from), Some(new_word)) = (from, new_word)
            && self.run(host, &format!("ROADTEMPLATE NEW {new_word} FROM {from}"))
        {
            let named = String::from(self.name.trim());
            self.select_named(host.document(), &named);
        }
    }

    fn press_rename(&mut self, host: &mut dyn Host) {
        let from = self.template_word();
        let typed = String::from(self.name.trim());
        let new_word = word(&mut self.status, &typed, "the new name");
        if let (Some(from), Some(new_word)) = (from, new_word)
            && self.run(host, &format!("ROADTEMPLATE RENAME {from} {new_word}"))
        {
            let named = String::from(self.name.trim());
            self.select_named(host.document(), &named);
        }
    }

    fn press_delete(&mut self, host: &mut dyn Host) {
        if let Some(template_word) = self.template_word() {
            self.run(host, &format!("ROADTEMPLATE DELETE {template_word}"));
        }
    }

    fn press_browse(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.status.set(
                "A headless session opens no file dialog: type the path in the field",
                true,
            );
            return;
        }
        // A save dialog, for the same box chooses a file to read. (Qt's was told not to ask
        // before it took a file that is there; the system's dialog may ask.)
        if let Some(chosen) =
            save_file_dialog("Road Template File", &self.file, "Road templates", "json")
        {
            self.file = chosen;
        }
    }

    fn press_export(&mut self, host: &mut dyn Host) {
        let Some(template_word) = self.template_word() else {
            return;
        };
        // The word ALL is every template to the verb, in any case: a template of that name cannot
        // be exported by name from here.
        if same_ignoring_case(&self.current_template(), "ALL") {
            self.status.set(
                "A template called ALL cannot be exported alone: Export All exports every \
                 template. Rename it first.",
                true,
            );
            return;
        }
        self.export_to(host, &template_word);
    }

    fn press_import(&mut self, host: &mut dyn Host) {
        if let Some(path) = self.file_word() {
            let overwrite = if self.overwrite { " OVERWRITE" } else { "" };
            self.run(host, &format!("ROADTEMPLATE IMPORT {path}{overwrite}"));
        }
    }

    fn press_add(&mut self, host: &mut dyn Host) {
        if let Some(template_word) = self.template_word() {
            let line = format!(
                "ROADTEMPLATE ADD {template_word} {} {}",
                self.add_side.current_text(),
                self.add_kind.current_text()
            );
            self.run(host, &line);
        }
    }

    fn press_remove(&mut self, host: &mut dyn Host) {
        let template_word = self.template_word();
        let reference = self.chosen_component();
        let Some(template_word) = template_word else {
            return;
        };
        if reference.is_empty() {
            self.status.set("Choose a component to remove.", true);
            return;
        }
        self.run(
            host,
            &format!("ROADTEMPLATE REMOVE {template_word} {reference}"),
        );
    }

    fn press_set_keys(&mut self, host: &mut dyn Host) {
        let template_word = self.template_word();
        let reference = self.chosen_component();
        let Some(template_word) = template_word else {
            return;
        };
        let keys = String::from(self.keys.trim());
        if reference.is_empty() || keys.is_empty() {
            self.status
                .set("Choose a component and type key=value to set.", true);
            return;
        }
        if self.run(
            host,
            &format!("ROADTEMPLATE SET {template_word} {reference} {keys}"),
        ) {
            self.keys.clear();
        }
    }

    fn press_course_add(&mut self, host: &mut dyn Host) {
        let template_word = self.template_word();
        let reference = self.chosen_component();
        let Some(template_word) = template_word else {
            return;
        };
        if reference.is_empty() {
            self.status
                .set("Choose the component the course is under.", true);
            return;
        }
        let course_name = String::from(self.course_name.trim());
        let material_text = String::from(self.course_material.trim());
        let course_word = word(&mut self.status, &course_name, "the course");
        let material = option(&mut self.status, "material", &material_text);
        let (Some(course_word), Some(material)) = (course_word, material) else {
            return;
        };
        let mut line = format!(
            "ROADTEMPLATE COURSE {template_word} {reference} ADD {course_word} thickness={}",
            self.course_thickness.trim()
        );
        if !material_text.is_empty() {
            line.push(' ');
            line.push_str(&material);
        }
        if self.run(host, &line) {
            self.course_name.clear();
            self.course_thickness.clear();
            self.course_material.clear();
        }
    }

    fn press_course_remove(&mut self, host: &mut dyn Host) {
        let template_word = self.template_word();
        let reference = self.chosen_component();
        let row = self.courses.current_row();
        let Some(template_word) = template_word else {
            return;
        };
        let (false, Some(row)) = (reference.is_empty(), row) else {
            self.status.set("Choose the course to remove.", true);
            return;
        };
        let course = self.courses.text(row, K_NAME);
        self.run(
            host,
            &format!("ROADTEMPLATE COURSE {template_word} {reference} REMOVE {course}"),
        );
    }

    /// What a frame did to the components grid, its handlers run.
    fn components_events(&mut self, events: TableEvents, document: &Document) {
        for (row, _) in events.edited {
            if !self.loading {
                self.edited_components.insert(row);
            }
        }
        if events.selection_changed {
            self.components_selection_changed(document);
        }
    }

    fn courses_events(&mut self, events: TableEvents) {
        for (row, _) in events.edited {
            if !self.loading {
                self.edited_courses.insert(row);
            }
        }
    }

    // ---- drawing --------------------------------------------------------------------------------

    fn body_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        // ---- the templates ----
        if self.list.ui(ui).selection_changed {
            self.list_selection_changed(host.document());
        }
        ui.horizontal(|ui| {
            field(
                ui,
                "roadTemplateName",
                &mut self.name,
                "name",
                "The name New, Copy and Rename give a template",
            );
            if button(
                ui,
                "roadTemplateNew",
                "New",
                "ROADTEMPLATE NEW: an empty template",
            )
            .clicked()
            {
                self.press_new(host);
            }
            if button(
                ui,
                "roadTemplateCopy",
                "Copy",
                "ROADTEMPLATE NEW ... FROM: a copy of the chosen template",
            )
            .clicked()
            {
                self.press_copy(host);
            }
            if button(
                ui,
                "roadTemplateRename",
                "Rename",
                "ROADTEMPLATE RENAME: the chosen template, and the corridors that use it",
            )
            .clicked()
            {
                self.press_rename(host);
            }
            if button(
                ui,
                "roadTemplateDelete",
                "Delete",
                "ROADTEMPLATE DELETE: refused while a corridor uses it",
            )
            .clicked()
            {
                self.press_delete(host);
            }
            if button(
                ui,
                "roadTemplateDefaults",
                "Add Standard Templates",
                "ROADTEMPLATE DEFAULTS: rural two-lane, urban street, divided road",
            )
            .clicked()
            {
                self.run(host, "ROADTEMPLATE DEFAULTS");
            }
        });

        // ---- templates in a file ----
        ui.horizontal(|ui| {
            field(
                ui,
                "roadTemplateFile",
                &mut self.file,
                "templates.json",
                "The file Export writes and Import reads",
            );
            if button(
                ui,
                "roadTemplateBrowse",
                "Browse...",
                "Choose the file (no file dialog in a headless run)",
            )
            .clicked()
            {
                self.press_browse(host);
            }
            if button(
                ui,
                "roadTemplateExport",
                "Export",
                "ROADTEMPLATE EXPORT: the chosen template, as JSON",
            )
            .clicked()
            {
                self.press_export(host);
            }
            if button(
                ui,
                "roadTemplateExportAll",
                "Export All",
                "ROADTEMPLATE EXPORT ALL: every template",
            )
            .clicked()
            {
                self.export_to(host, "ALL");
            }
            if button(
                ui,
                "roadTemplateImport",
                "Import",
                "ROADTEMPLATE IMPORT: the file's templates, as one undo step; a name the drawing \
                 has is refused unless Replace is ticked",
            )
            .clicked()
            {
                self.press_import(host);
            }
            check(
                ui,
                "roadTemplateOverwrite",
                "Replace (OVERWRITE)",
                &mut self.overwrite,
            )
            .on_hover_text(
                "Export: replace a file already there. Import: replace the templates of the same \
                 name where they stand",
            );
        });

        // ---- components and courses, above the preview ----
        let height = grid_height(ui.style());
        self.components.set_fixed_height(height);
        self.courses.set_fixed_height(height);
        note(
            ui,
            "Components out from the centreline, each side in order. Width, crossfall (percent, \
             outward: -3 falls), layer and a kerb's faces (inward, or outward for a median's or \
             island's) are edited in place and applied together.",
        );
        let events = self.components.ui(ui);
        self.components_events(events, host.document());
        ui.horizontal(|ui| {
            if button(
                ui,
                "roadTemplateApplyComponents",
                "Apply",
                "ROADTEMPLATE SET for each edited row: its width, crossfall and layer",
            )
            .clicked()
            {
                self.apply_components(host);
            }
            if button(
                ui,
                "roadTemplateRevertComponents",
                "Revert",
                "The grid as the drawing holds it again",
            )
            .clicked()
            {
                self.edited_components.clear();
                self.fill_components(host.document(), true);
            }
            self.add_side.ui(ui, "roadTemplateAddSide");
            self.add_kind.ui(ui, "roadTemplateAddKind");
            if button(
                ui,
                "roadTemplateAdd",
                "Add",
                "ROADTEMPLATE ADD: a component of this kind on this side, before the batter",
            )
            .clicked()
            {
                self.press_add(host);
            }
            if button(
                ui,
                "roadTemplateRemove",
                "Remove",
                "ROADTEMPLATE REMOVE: the chosen component",
            )
            .clicked()
            {
                self.press_remove(host);
            }
        });
        ui.horizontal(|ui| {
            field(
                ui,
                "roadTemplateComponentKeys",
                &mut self.keys,
                "shape=kerbgutter height=0.15  |  cut=2 fill=4 benchheight=",
                "key=value for the chosen component: a kerb's shape faces (inward, or outward for \
                 a median's kerb) height depth gutter gutterfall material, a drain's depth, a \
                 batter's cut fill maxwidth benchheight benchwidth, colour=, \
                 superelevated=on|off, name=",
            );
            if button(
                ui,
                "roadTemplateSetKeys",
                "Set",
                "ROADTEMPLATE SET with these keys",
            )
            .clicked()
            {
                self.press_set_keys(host);
            }
        });
        note(
            ui,
            "The chosen component's courses, top down. Thickness, material and IFC type are \
             edited in place and applied together.",
        );
        let events = self.courses.ui(ui);
        self.courses_events(events);
        ui.horizontal(|ui| {
            if button(
                ui,
                "roadTemplateApplyCourses",
                "Apply",
                "ROADTEMPLATE COURSE ... SET for each edited row",
            )
            .clicked()
            {
                self.apply_courses(host);
            }
            field(
                ui,
                "roadTemplateCourseName",
                &mut self.course_name,
                "course",
                "A new course's name (a word)",
            );
            field(
                ui,
                "roadTemplateCourseThickness",
                &mut self.course_thickness,
                "thickness m",
                "A new course's thickness, metres",
            );
            field(
                ui,
                "roadTemplateCourseMaterial",
                &mut self.course_material,
                "material",
                "A new course's material: asphalt, concrete, crushed rock ...",
            );
            if button(
                ui,
                "roadTemplateCourseAdd",
                "Add Course",
                "ROADTEMPLATE COURSE ... ADD: under the chosen component's courses",
            )
            .clicked()
            {
                self.press_course_add(host);
            }
            if button(
                ui,
                "roadTemplateCourseRemove",
                "Remove Course",
                "ROADTEMPLATE COURSE ... REMOVE: the chosen course",
            )
            .clicked()
            {
                self.press_course_remove(host);
            }
        });

        // A drawing of a typical section reads at this height; the grids above it are the
        // dialog's purpose and get the room.
        let width = ui.available_width().max(220.0);
        let road_template = self.preview_template.as_ref();
        let section = self.preview_section.as_ref();
        preview(
            ui,
            "roadTemplatePreview",
            egui::vec2(width, 110.0),
            self.raster.as_ref(),
            &mut self.texture,
            &mut |canvas: &mut dyn Canvas| {
                let size = egui::vec2(width, 110.0);
                paint_template_preview(
                    canvas,
                    RectF::new(0.0, 0.0, f64::from(size.x), f64::from(size.y)),
                    road_template,
                    section,
                );
            },
        );
        self.status.ui(ui, "roadTemplateStatus");
    }

    fn footer_ui(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, "roadTemplateClose", "Close", "Close the manager").clicked() {
                self.close = true;
            }
        });
    }
}

/// The Road Template Manager (see the module comment).
pub struct RoadTemplateManagerDialog {
    frame: DialogFrame,
    open: bool,
    body: Body,
}

impl RoadTemplateManagerDialog {
    /// The manager over `document`, shown, its first template chosen.
    pub fn new(document: &Document) -> Self {
        let mut add_side = Combo::new();
        for each in ["both", "left", "right"] {
            add_side.add_item(each);
        }
        let mut add_kind = Combo::new();
        for kind in [
            ComponentKind::Lane,
            ComponentKind::Shoulder,
            ComponentKind::Kerb,
            ComponentKind::Footpath,
            ComponentKind::Verge,
            ComponentKind::Median,
            ComponentKind::Drain,
            ComponentKind::Batter,
        ] {
            add_kind.add_item(kind.name());
        }
        let mut body = Body {
            watcher: DocumentWatcher::new(document),
            executor: true,
            list: Table::new(
                "roadTemplateList",
                &["Template", "Left", "Right", "Courses", "Description"],
            )
            .max_height(150.0),
            name: String::new(),
            file: String::new(),
            overwrite: false,
            components: Table::new(
                "roadTemplateComponents",
                &[
                    "Side",
                    "Name",
                    "Kind",
                    "Width (m)",
                    "Crossfall (%)",
                    "Layer",
                    "Faces",
                    "What else",
                ],
            )
            .editing(),
            add_side,
            add_kind,
            keys: String::new(),
            courses: Table::new(
                "roadTemplateCourses",
                &["Course", "Thickness (m)", "Material", "IFC type"],
            )
            .editing(),
            course_name: String::new(),
            course_thickness: String::new(),
            course_material: String::new(),
            preview_template: None,
            preview_section: None,
            raster: None,
            texture: None,
            status: StatusLine::default(),
            loading: false,
            edited_components: BTreeSet::new(),
            edited_courses: BTreeSet::new(),
            filled_from: None,
            filled_courses: None,
            close: false,
        };
        body.reload(document);
        if body.list.row_count() > 0 && body.list.select_row(0) {
            body.list_selection_changed(document);
        }
        Self {
            frame: DialogFrame::new("roadTemplateManagerDialog", "Road Template Manager"),
            open: true,
            body,
        }
    }

    /// A manager with no executor (the C++'s empty CommandRunner): it says it cannot run a line.
    pub fn without_executor(mut self) -> Self {
        self.body.executor = false;
        self
    }

    /// The window's raster canvas, for the preview to be painted on screen.
    pub fn set_raster(&mut self, raster: PreviewRaster) {
        self.body.raster = Some(raster);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn hide(&mut self) {
        self.open = false;
    }

    /// Chooses the template `name` in the list (ignoring case); false when there is none.
    pub fn select_template(&mut self, document: &Document, name: &str) -> bool {
        self.body.select_named(document, name)
    }

    /// The chosen template's name; empty when there is none.
    pub fn current_template(&self) -> String {
        self.body.current_template()
    }

    /// The tables, by their object names: roadTemplateList, roadTemplateComponents,
    /// roadTemplateCourses.
    pub fn table(&self, name: &str) -> Option<&Table> {
        [&self.body.list, &self.body.components, &self.body.courses]
            .into_iter()
            .find(|table| table.name() == name)
    }

    /// A cell of the grid `table` typed into, as a person commits an edit
    /// (QTableWidgetItem::setText): its change is the grid's, marked for Apply, and runs nothing.
    /// False when there is no such cell.
    pub fn edit_cell(&mut self, table: &str, row: usize, column: usize, text: &str) -> bool {
        let body = &mut self.body;
        let grid = match table {
            "roadTemplateComponents" => &mut body.components,
            "roadTemplateCourses" => &mut body.courses,
            "roadTemplateList" => &mut body.list,
            _ => return false,
        };
        if grid.cell(row, column).is_none() {
            return false;
        }
        if grid.set_text(row, column, text) && !body.loading {
            match table {
                "roadTemplateComponents" => {
                    body.edited_components.insert(row);
                }
                "roadTemplateCourses" => {
                    body.edited_courses.insert(row);
                }
                _ => {}
            }
        }
        true
    }

    /// The row `row` of the grid `table` chosen (QTableView::selectRow), its handler run as the
    /// selection's signal ran it. False when there is no such row.
    pub fn select_row(&mut self, document: &Document, table: &str, row: usize) -> bool {
        let body = &mut self.body;
        match table {
            "roadTemplateList" if row < body.list.row_count() => {
                if body.list.select_row(row) {
                    body.list_selection_changed(document);
                }
                true
            }
            "roadTemplateComponents" if row < body.components.row_count() => {
                if body.components.select_row(row) {
                    body.components_selection_changed(document);
                }
                true
            }
            "roadTemplateCourses" if row < body.courses.row_count() => {
                body.courses.select_row(row);
                true
            }
            _ => false,
        }
    }

    /// The preview painted into `canvas` over `rect`, as it is on screen.
    pub fn paint_preview(&self, canvas: &mut dyn Canvas, rect: RectF) {
        paint_template_preview(
            canvas,
            rect,
            self.body.preview_template.as_ref(),
            self.body.preview_section.as_ref(),
        );
    }

    /// Draws the manager for this frame, when it is shown; then the watcher's delivery, as the
    /// C++'s event loop delivered it after a click (corridor_dialog's `ui` says why).
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.open {
            let ctx = ui.ctx().clone();
            let mut lent = (&mut self.body, &mut *host);
            let shown = self.frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body_ui(ui, &mut *lent.1),
                |ui, lent| lent.0.footer_ui(ui),
            );
            if shown.closed || std::mem::take(&mut self.body.close) {
                self.open = false;
            }
        }
        if let Some(changes) = self.body.watcher.deliver(host.document())
            && changes.model
        {
            self.body.reload(host.document());
            ui.ctx().request_repaint();
        }
    }
}
