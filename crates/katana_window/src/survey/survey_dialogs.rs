//! The dialogs of the Survey menu: Inverse, Forward Point, the Angle and Bearing Calculator,
//! Traverse, Level Book and the Coordinate Converter (survey_dialogs.hpp/.cpp).
//!
//! NOTHING SURVEY-SPECIFIC IS COMPUTED HERE. A dialog gathers text, hands it to the functions of
//! katana_cad::survey_tools and shows the report those return - the same report the command line
//! prints for INVERSE, FORWARD and AREA - in its own result pane and in the command log. A number
//! that appeared only in a dialog would be a second answer nobody tests.
//!
//! Every dialog is NON-MODAL and kept by the survey workbench between uses, so it can stay open
//! beside the drawing while the selection changes: "Use Selection" reads the selected point
//! entities when it is pressed, never before.
//!
//! As the C++ does, Add Point and Add to Drawing execute their cad command on the drawing
//! themselves (one undo step each) rather than handing a verb line to the executor: the C++
//! dialogs call katana_cad::survey_tools directly, and the port keeps that (docs/rust.md, "Not
//! done": the survey tool dialogs are the window's dialogs that do not go through the one
//! executor).
//!
//! The object names below - of each dialog, its fields and its verb buttons - are an interface:
//! the headless --dialog switch fills fields and presses buttons by these names.
//!
//! ```text
//! dialog (action + "Dialog")       fields                         buttons
//! surveyInverseDialog              from to                        useSelection compute
//! surveyForwardDialog              from direction distance        useSelection compute
//!                                  heightDifference name          addPoint
//! surveyAngleCalculatorDialog      angle unit                     convert
//! surveyTraverseDialog             name kind start startMark      compute addToDrawing
//!                                  startAzimuth legs endStation
//!                                  end closingAngle closingMark
//!                                  closingAzimuth method
//!                                  balanceAngles angleSigma
//!                                  distanceSigma distancePpm
//! surveyLevelBookDialog            name startLevel closingLevel   reduce
//!                                  book adjustment allowance
//! surveyCoordinateConverterDialog  source target points           useSelection convert
//! ```
//!
//! Every dialog has a Close button (`close`), a line for what is wrong (`message`) and a
//! read-only result pane (`result`).

use std::path::PathBuf;

use egui::{Color32, Label, Rect, Response, RichText, Ui, UiBuilder, Vec2};
use katana_cad::Document;
use katana_cad::survey_tools::{
    AngleInputUnit, ForwardInput, LevelBookSpec, SurveyPosition, THIRD_ORDER_LEVEL_ALLOWANCE,
    TraverseMethod, TraverseOrientation, TraverseSpec, compute_forward, compute_inverse,
    compute_level_book, compute_traverse_tool, conversion_lines_for_selection, convert_angle,
    convert_coordinates, default_traverse_precision, ends_of_line, format_angle_conversion,
    format_coordinate_conversion, format_forward_report, format_inverse_report,
    format_level_book_report, format_traverse_report, forward_point_command, parse_level_book,
    parse_survey_angle, parse_survey_direction, parse_survey_number, parse_survey_position,
    parse_traverse_legs, selected_point_positions, survey_angle_formats, traverse_command,
};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, format_general};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_survey::angles::{arc_seconds_to_radians, radians_to_arc_seconds};
use katana_survey::{LevelAdjustment, TraverseKind, TraverseStochasticModel};

use crate::drive::{self, Choice, LineOptions, TextOptions};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme::palette;

// ---- what every survey dialog shares -------------------------------------------------------

/// A label in the muted colour, wrapped: the note at the top of a dialog. No object name, as the
/// C++'s mutedLabel has none.
pub(crate) fn muted(ui: &mut Ui, text: &str) -> Response {
    ui.add(Label::new(RichText::new(text).color(palette::text_muted().to_egui())).wrap())
}

/// A named label in `colour`, wrapped: a message line, a summary.
pub(crate) fn coloured_label(ui: &mut Ui, name: &str, text: &str, colour: Color32) -> Response {
    drive::label_as(
        ui,
        name,
        text,
        Label::new(RichText::new(text).color(colour)).wrap(),
    )
}

/// The muted colour or the error colour, as a message line is shown.
pub(crate) fn message_colour(is_error: bool) -> Color32 {
    if is_error {
        palette::error().to_egui()
    } else {
        palette::text_muted().to_egui()
    }
}

/// What a widget its owner hid (QWidget::setVisible(false)) is: drawn invisible, so it is still
/// there to be filled - as Qt's setText fills a hidden field - and a press of it is refused as
/// hidden; it takes no room, as a hidden Qt widget takes none.
pub(crate) fn hidden<R>(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().id_salt(("katanaHidden", salt)).invisible());
    add(&mut child)
}

/// What a widget on a page that is not in front is (a QStackedWidget's or a QTabWidget's other
/// pages): not hidden itself, so a fill and a press still reach it as they reach it in Qt, but
/// neither painted nor reachable by the pointer, and taking no room.
pub(crate) fn offstage<R>(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().id_salt(("katanaOffstage", salt)));
    let corner = child.max_rect().min;
    child.set_clip_rect(Rect::from_min_size(corner, Vec2::ZERO));
    add(&mut child)
}

/// A form row: the label, then what `add` draws beside it (QFormLayout::addRow).
pub(crate) fn form_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        if !label.is_empty() {
            ui.label(label);
        }
        add(ui)
    })
    .inner
}

/// A one-line field with its placeholder and tooltip.
pub(crate) fn field(
    ui: &mut Ui,
    name: &str,
    value: &mut String,
    placeholder: &str,
    tip: &str,
) -> Response {
    let response = drive::line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder,
            ..LineOptions::default()
        },
    );
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

/// A field that is drawn disabled when `enabled` is false (QWidget::setEnabled).
pub(crate) fn field_if(
    ui: &mut Ui,
    enabled: bool,
    name: &str,
    value: &mut String,
    placeholder: &str,
    tip: &str,
) -> Response {
    ui.add_enabled_ui(enabled, |ui| field(ui, name, value, placeholder, tip))
        .inner
}

/// A choice over `items`.
pub(crate) fn choice_of(ui: &mut Ui, name: &str, items: &[&str], chosen: &mut Choice) -> Response {
    let items: Vec<String> = items.iter().map(|item| String::from(*item)).collect();
    drive::choice(ui, name, &items, chosen)
}

/// A file dialog to save to, on the desktop only: a headless session opens none, and the
/// callers refuse before they get here (the plan's section 4.1, "No hidden modality").
pub(crate) fn save_file_dialog(
    title: &str,
    suggested: &str,
    filters: &[(&str, &[&str])],
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(suggested);
    for (name, extensions) in filters {
        dialog = dialog.add_filter(*name, extensions);
    }
    dialog.save_file()
}

/// A file dialog to open, on the desktop only (see save_file_dialog).
pub(crate) fn open_file_dialog(title: &str, filters: &[(&str, &[&str])]) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    for (name, extensions) in filters {
        dialog = dialog.add_filter(*name, extensions);
    }
    dialog.pick_file()
}

/// The refusal of a file dialog in a headless session.
pub(crate) fn no_file_dialog() -> Error {
    make_error(
        ErrorCode::InvalidState,
        "a headless session opens no file dialog",
        "type the file's path in its field",
    )
}

/// Frames every view on the drawing after an operation added to it.
pub(crate) fn frame_views(host: &mut dyn Host) {
    if let Some(views) = host.views() {
        views.zoom_extents_all();
    }
}

/// A field's text as a verb reads it: trimmed (QLineEdit::text().trimmed()).
fn text(value: &str) -> &str {
    value.trim()
}

/// Names the field an error came from, so "a position is E,N ..." says WHICH position. The
/// library's message and context are kept as they are.
fn in_field<T>(field: &str, result: Result<T>) -> Result<T> {
    result.map_err(|mut error| {
        error.message = format!("{field}: {}", error.message);
        error
    })
}

/// A field that may be left empty: None when it is, the parse otherwise - so an empty field is
/// "not given", never zero.
fn optional_number(value: &str, what: &str) -> Result<Option<f64>> {
    let value = text(value);
    if value.is_empty() {
        return Ok(None);
    }
    in_field(what, parse_survey_number(value, what)).map(Some)
}

/// A position as the fields take it back: exact, so what Use Selection filled in is the entity's
/// coordinate to the last bit and not a rounding of it.
fn position_text(position: &SurveyPosition) -> String {
    let mut value = format!(
        "{},{}",
        format_exact_real(position.point.x),
        format_exact_real(position.point.y)
    );
    if let Some(elevation) = position.elevation {
        value.push(',');
        value.push_str(&format_exact_real(elevation));
    }
    value
}

/// A point entity's id as a field takes it.
fn entity_text(position: &SurveyPosition) -> String {
    position
        .entity
        .map(|entity| format!("{entity}"))
        .unwrap_or_default()
}

fn selection_summary(document: &Document) -> String {
    format!(
        "{} selected, {} of them point entities",
        document.selection().size(),
        selected_point_positions(document).len()
    )
}

/// The library's one sentence on angle text (survey_angle_formats), as a sentence of the note:
/// capitalised and stopped.
fn sentence(text: &str) -> String {
    let mut characters = text.chars();
    let mut result = match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    };
    result.push('.');
    result
}

/// A button of a survey dialog's row: its object name and its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolButton {
    pub name: &'static str,
    pub text: &'static str,
}

impl ToolButton {
    pub const fn new(name: &'static str, text: &'static str) -> Self {
        Self { name, text }
    }
}

const USE_SELECTION: ToolButton = ToolButton::new("useSelection", "Use Selection");
const COMPUTE: ToolButton = ToolButton::new("compute", "Compute");

/// What every survey dialog shows besides its form: the muted note saying what the tool does, a
/// line for what is wrong (`message`), and the read-only result pane (`result`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolPane {
    pub note: String,
    pub message: String,
    pub message_is_error: bool,
    pub result: String,
}

/// The frame every survey dialog shares (the C++ SurveyToolDialog): the note, a form, the
/// message line, the result pane in a fixed-width font (the reports are tables), and a row of
/// buttons - the fillers, which read the drawing into the form, then the verbs, then Close.
pub struct SurveyToolFrame {
    pub frame: DialogFrame,
    pub pane: ToolPane,
    pub open: bool,
}

impl SurveyToolFrame {
    /// Not modal: kept open beside the drawing while the selection changes.
    pub fn new(name: &str, title: &str, note: &str) -> Self {
        Self {
            frame: DialogFrame::new(name, title),
            pane: ToolPane {
                note: String::from(note),
                ..ToolPane::default()
            },
            open: false,
        }
    }

    pub fn title(&self) -> &str {
        self.frame.title()
    }

    /// Shows `text` on the message line in the muted colour (not an error).
    pub fn set_note(&mut self, text: &str) {
        self.pane.message = String::from(text);
        self.pane.message_is_error = false;
    }

    /// Shows `error` on the message line in the error colour, and logs it as an error.
    pub fn show_error(&mut self, host: &mut dyn Host, error: &Error) {
        let mut shown = error.message.clone();
        if !error.context.is_empty() {
            shown.push_str(" - ");
            shown.push_str(&error.context);
        }
        self.pane.message = shown;
        self.pane.message_is_error = true;
        host.log(
            &format!("{}: {}", self.frame.title(), error.describe()),
            true,
        );
    }

    /// A verb's end: its report shown in the pane and logged; its error shown on the message
    /// line and logged as an error.
    pub fn verb_done(&mut self, host: &mut dyn Host, report: Result<String>) {
        match report {
            Err(error) => self.show_error(host, &error),
            Ok(report) => {
                self.pane.message.clear();
                self.pane.result.clone_from(&report);
                host.log(&report, false);
            }
        }
    }

    /// A filler's end: its error is shown, its success is silent (what it filled is on screen).
    /// The message line was cleared before it ran.
    pub fn filler_done(&mut self, host: &mut dyn Host, status: Status) {
        if let Err(error) = status {
            self.show_error(host, &error);
        }
    }

    /// Draws the dialog, when it is open: `draw` draws the form over `form`; the row has the
    /// fillers (in their order, first leftmost), the verbs and Close. Answers the button pressed,
    /// Close and the title bar's close having hidden it already.
    pub fn show<T>(
        &mut self,
        ui: &mut Ui,
        form: &mut T,
        draw: impl FnOnce(&mut Ui, &mut T),
        fillers: &[ToolButton],
        verbs: &[ToolButton],
    ) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        let ctx = ui.ctx().clone();
        let mut pressed: Option<&'static str> = None;
        let mut parts = (&mut self.pane, form, &mut pressed);
        let shown = self.frame.show(
            &ctx,
            &mut parts,
            |ui, parts| {
                let (pane, form, _) = parts;
                muted(ui, &pane.note);
                draw(ui, &mut **form);
                coloured_label(
                    ui,
                    "message",
                    &pane.message,
                    message_colour(pane.message_is_error),
                );
                drive::text_with(
                    ui,
                    "result",
                    &mut pane.result,
                    TextOptions {
                        read_only: true,
                        rows: 10,
                        monospace: true,
                    },
                );
            },
            |ui, parts| {
                let (_, _, pressed) = parts;
                ui.horizontal(|ui| {
                    // Left of the stretch: fillers read the drawing, verbs act on the form.
                    for filler in fillers {
                        if drive::button(ui, filler.name, filler.text).clicked() {
                            **pressed = Some(filler.name);
                        }
                    }
                    ui.add_space(24.0);
                    for verb in verbs {
                        if drive::button(ui, verb.name, verb.text).clicked() {
                            **pressed = Some(verb.name);
                        }
                    }
                    if drive::button(ui, "close", "Close").clicked() {
                        **pressed = Some("close");
                    }
                });
            },
        );
        if shown.closed || pressed == Some("close") {
            self.open = false;
        }
        pressed
    }
}

// ---- inverse ----------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct InverseForm {
    from: String,
    to: String,
}

/// Survey > Inverse... (surveyInverseDialog).
pub struct SurveyInverseDialog {
    pub tool: SurveyToolFrame,
    form: InverseForm,
}

impl Default for SurveyInverseDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyInverseDialog {
    pub fn new() -> Self {
        Self {
            tool: SurveyToolFrame::new(
                "surveyInverseDialog",
                "Inverse",
                "Horizontal distance, azimuth and bearing between two positions - and the \
                 height difference, slope distance and grade when both have an elevation (a \
                 missing one is never taken as zero). Plane grid arithmetic: no scale factor is \
                 applied. Use Selection takes two selected points, or the two ends of one \
                 selected line or open polyline.",
            ),
            form: InverseForm::default(),
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let tip = "E,N or E,N,Z - easting first - or the id of a point entity";
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            |ui, form| {
                form_row(ui, "From:", |ui| {
                    field(ui, "from", &mut form.from, "E,N[,Z] or point id", tip)
                });
                form_row(ui, "To:", |ui| {
                    field(ui, "to", &mut form.to, "E,N[,Z] or point id", tip)
                });
            },
            &[USE_SELECTION],
            &[COMPUTE],
        );
        match pressed {
            Some("useSelection") => {
                self.tool.pane.message.clear();
                let status = self.use_selection(host.document());
                self.tool.filler_done(host, status);
            }
            Some("compute") => {
                let report = self.compute(host.document());
                self.tool.verb_done(host, report);
            }
            _ => {}
        }
    }

    fn compute(&self, document: &Document) -> Result<String> {
        let from = in_field(
            "From",
            parse_survey_position(document, text(&self.form.from)),
        )?;
        let to = in_field("To", parse_survey_position(document, text(&self.form.to)))?;
        let result = compute_inverse(&from, &to)?;
        Ok(format_inverse_report(&result))
    }

    fn use_selection(&mut self, document: &Document) -> Status {
        let ids = document.selection().ids();
        let points = selected_point_positions(document);
        if ids.len() == 2 && points.len() == 2 {
            self.form.from = entity_text(&points[0]);
            self.form.to = entity_text(&points[1]);
            return Ok(());
        }
        if ids.len() == 1 {
            let (start, end) = ends_of_line(document, ids[0])?;
            // The fields take points by id but not a line's ends, so the ends go in as their
            // coordinates - with the heights the line carries.
            self.form.from = position_text(&start);
            self.form.to = position_text(&end);
            // The source is "line 5 start": the note names the line, not its end.
            let kept = start.source.chars().count().saturating_sub(6);
            let line: String = start.source.chars().take(kept).collect();
            self.tool
                .set_note(&format!("The ends of {line}, as coordinates."));
            return Ok(());
        }
        Err(make_error(
            ErrorCode::InvalidState,
            "select two point entities, or one line or open polyline",
            selection_summary(document),
        ))
    }
}

// ---- forward ----------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct ForwardForm {
    from: String,
    direction: String,
    distance: String,
    height_difference: String,
    name: String,
}

/// Survey > Forward Point... (surveyForwardDialog).
pub struct SurveyForwardDialog {
    pub tool: SurveyToolFrame,
    form: ForwardForm,
}

impl Default for SurveyForwardDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyForwardDialog {
    pub fn new() -> Self {
        let note = format!(
            "A new point from a position, a direction and a horizontal distance (a radiation). \
             With a height difference the new point's height is the start's plus it - refused \
             when the start has none. Add Point puts it on the current layer as one undoable \
             step. {}",
            sentence(survey_angle_formats())
        );
        Self {
            tool: SurveyToolFrame::new("surveyForwardDialog", "Forward Point", &note),
            form: ForwardForm::default(),
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            |ui, form| {
                form_row(ui, "From:", |ui| {
                    field(
                        ui,
                        "from",
                        &mut form.from,
                        "E,N[,Z] or point id",
                        "E,N or E,N,Z - easting first - or the id of a point entity",
                    )
                });
                form_row(ui, "Direction:", |ui| {
                    field(
                        ui,
                        "direction",
                        &mut form.direction,
                        "azimuth or bearing, e.g. N 36d52m11.63s E",
                        "An azimuth clockwise from grid north, or a quadrant bearing",
                    )
                });
                form_row(ui, "Horizontal distance:", |ui| {
                    field(ui, "distance", &mut form.distance, "drawing units", "")
                });
                form_row(ui, "Height difference:", |ui| {
                    field(
                        ui,
                        "heightDifference",
                        &mut form.height_difference,
                        "optional",
                        "Leave empty for a point with no height",
                    )
                });
                form_row(ui, "Point name:", |ui| {
                    field(
                        ui,
                        "name",
                        &mut form.name,
                        "optional",
                        "Written as the new point's name",
                    )
                });
            },
            &[USE_SELECTION],
            &[COMPUTE, ToolButton::new("addPoint", "Add Point")],
        );
        match pressed {
            Some("useSelection") => {
                self.tool.pane.message.clear();
                let status = self.use_selection(host.document());
                self.tool.filler_done(host, status);
            }
            Some("compute") => {
                let report = self.compute(host, false);
                self.tool.verb_done(host, report);
            }
            Some("addPoint") => {
                let report = self.compute(host, true);
                self.tool.verb_done(host, report);
            }
            _ => {}
        }
    }

    fn use_selection(&mut self, document: &Document) -> Status {
        let points = selected_point_positions(document);
        if points.len() != 1 || document.selection().size() != 1 {
            return Err(make_error(
                ErrorCode::InvalidState,
                "select the one point entity to start from",
                selection_summary(document),
            ));
        }
        self.form.from = entity_text(&points[0]);
        Ok(())
    }

    fn compute(&self, host: &mut dyn Host, add: bool) -> Result<String> {
        let input = {
            let document = host.document();
            let from = in_field(
                "From",
                parse_survey_position(document, text(&self.form.from)),
            )?;
            let azimuth = in_field(
                "Direction",
                parse_survey_direction(text(&self.form.direction)),
            )?;
            let distance =
                parse_survey_number(text(&self.form.distance), "the horizontal distance")?;
            let height_difference =
                optional_number(&self.form.height_difference, "the height difference")?;
            ForwardInput {
                from,
                azimuth,
                distance,
                height_difference,
                name: String::from(text(&self.form.name)),
            }
        };
        let result = compute_forward(&input)?;
        let report = format_forward_report(&result);
        if !add {
            return Ok(report);
        }
        let command = forward_point_command(host.document(), &result)?;
        host.document_mut().execute(command)?;
        frame_views(host);
        Ok(format!(
            "{report}\n  Created on layer {}",
            host.document().current_layer()
        ))
    }
}

// ---- angle calculator -------------------------------------------------------------------------

/// The units of the Entered In choice, in the order of AngleInputUnit.
const ANGLE_UNITS: [(&str, AngleInputUnit); 4] = [
    ("Degrees", AngleInputUnit::Degrees),
    ("Gons", AngleInputUnit::Gons),
    ("Radians", AngleInputUnit::Radians),
    ("Quadrant bearing", AngleInputUnit::Bearing),
];

#[derive(Clone, Debug, PartialEq)]
struct AngleForm {
    angle: String,
    unit: Choice,
}

/// Survey > Angle and Bearing Calculator... (surveyAngleCalculatorDialog).
pub struct SurveyAngleDialog {
    pub tool: SurveyToolFrame,
    form: AngleForm,
}

impl Default for SurveyAngleDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyAngleDialog {
    pub fn new() -> Self {
        Self {
            tool: SurveyToolFrame::new(
                "surveyAngleCalculatorDialog",
                "Angle and Bearing Calculator",
                "Converts an angle between degrees, minutes and seconds, decimal degrees, gons \
                 (400 to the circle) and radians, and reads it as a direction: its azimuth, \
                 quadrant bearing and reverse. Degrees are typed with a mark after each field - \
                 36d52m11.63s, 36:52:11.63, 36-52-11.63 - or as decimals; there is no DDD.MMSS \
                 form, which would make 36.5211 mean two angles.",
            ),
            form: AngleForm {
                angle: String::new(),
                unit: Choice::at(0),
            },
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            |ui, form| {
                form_row(ui, "Angle:", |ui| {
                    field(
                        ui,
                        "angle",
                        &mut form.angle,
                        "e.g. 36d52m11.63s, 100, 0.5, S 45 W",
                        "",
                    )
                });
                let units: Vec<&str> = ANGLE_UNITS.iter().map(|(label, _)| *label).collect();
                form_row(ui, "Entered in:", |ui| {
                    choice_of(ui, "unit", &units, &mut form.unit)
                });
            },
            &[],
            &[ToolButton::new("convert", "Convert")],
        );
        if pressed == Some("convert") {
            let unit = ANGLE_UNITS[self.form.unit.index.unwrap_or(0).min(ANGLE_UNITS.len() - 1)].1;
            let report = convert_angle(text(&self.form.angle), unit)
                .map(|conversion| format_angle_conversion(&conversion));
            self.tool.verb_done(host, report);
        }
    }
}

// ---- traverse ---------------------------------------------------------------------------------

/// In the order of the Kind choice.
const KINDS: [(&str, TraverseKind); 3] = [
    ("Closed loop", TraverseKind::ClosedLoop),
    ("Link", TraverseKind::Link),
    ("Open", TraverseKind::Open),
];

/// In the order of the Method choice.
const TRAVERSE_METHODS: [(&str, TraverseMethod); 4] = [
    ("Bowditch (compass)", TraverseMethod::Compass),
    ("Transit", TraverseMethod::Transit),
    ("Least squares", TraverseMethod::LeastSquares),
    ("None", TraverseMethod::None),
];

#[derive(Clone, Debug, PartialEq)]
struct TraverseForm {
    name: String,
    kind: Choice,
    start: String,
    start_mark: String,
    start_azimuth: String,
    legs: String,
    end_station: String,
    end: String,
    closing_angle: String,
    closing_mark: String,
    closing_azimuth: String,
    method: Choice,
    balance_angles: bool,
    angle_sigma: String,
    distance_sigma: String,
    distance_ppm: String,
}

impl TraverseForm {
    fn kind(&self) -> TraverseKind {
        KINDS[self.kind.index.unwrap_or(0).min(KINDS.len() - 1)].1
    }

    fn method(&self) -> TraverseMethod {
        TRAVERSE_METHODS[self
            .method
            .index
            .unwrap_or(0)
            .min(TRAVERSE_METHODS.len() - 1)]
        .1
    }
}

/// A reference mark (a position) or an azimuth; the mark wins, as TraverseOrientation says.
/// Neither is refused rather than taken as north: an orientation nobody gave would turn the whole
/// traverse.
fn orientation(
    document: &Document,
    mark: &str,
    azimuth: &str,
    what: &str,
) -> Result<TraverseOrientation> {
    if !text(mark).is_empty() {
        let position = in_field(what, parse_survey_position(document, text(mark)))?;
        return Ok(TraverseOrientation {
            reference_mark: Some(position.point),
            ..TraverseOrientation::default()
        });
    }
    if !text(azimuth).is_empty() {
        let direction = in_field(what, parse_survey_direction(text(azimuth)))?;
        return Ok(TraverseOrientation {
            azimuth: direction,
            ..TraverseOrientation::default()
        });
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!("{what}: give a reference mark or an azimuth"),
    ))
}

/// Survey > Traverse... (surveyTraverseDialog).
pub struct SurveyTraverseDialog {
    pub tool: SurveyToolFrame,
    form: TraverseForm,
}

impl Default for SurveyTraverseDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyTraverseDialog {
    pub fn new() -> Self {
        // The weights least squares uses, filled with the library's defaults (a common 5"
        // construction instrument) to be replaced with the instrument's.
        let defaults = default_traverse_precision();
        Self {
            tool: SurveyToolFrame::new(
                "surveyTraverseDialog",
                "Traverse",
                "A traverse from known control: the angles turned clockwise from the back \
                 station and the horizontal distances, one leg per line (station angle \
                 distance). A closed loop returns to its start; a link closes on a second known \
                 station (and on a reference direction there, if a closing angle is given); an \
                 open traverse has no check at all. Positions are E,N or a point id. Add to \
                 Drawing puts the computed stations and the traverse line on the current layer \
                 as one undoable step.",
            ),
            form: TraverseForm {
                name: String::new(),
                kind: Choice::at(0),
                start: String::new(),
                start_mark: String::new(),
                start_azimuth: String::new(),
                legs: String::new(),
                end_station: String::new(),
                end: String::new(),
                closing_angle: String::new(),
                closing_mark: String::new(),
                closing_azimuth: String::new(),
                method: Choice::at(0),
                balance_angles: true,
                angle_sigma: format_general(radians_to_arc_seconds(defaults.angle_sigma), 6),
                distance_sigma: format_general(defaults.distance_sigma_constant * 1000.0, 6),
                distance_ppm: format_general(defaults.distance_sigma_ppm, 6),
            },
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            Self::draw_form,
            &[],
            &[COMPUTE, ToolButton::new("addToDrawing", "Add to Drawing")],
        );
        match pressed {
            Some("compute") => {
                let report = self.compute(host, false);
                self.tool.verb_done(host, report);
            }
            Some("addToDrawing") => {
                let report = self.compute(host, true);
                self.tool.verb_done(host, report);
            }
            _ => {}
        }
    }

    /// Only the fields the chosen kind and method read are enabled, so a value typed where it
    /// would be ignored cannot look as if it counted.
    fn draw_form(ui: &mut Ui, form: &mut TraverseForm) {
        let kind = form.kind();
        let link = kind == TraverseKind::Link;
        let least_squares = form.method() == TraverseMethod::LeastSquares;
        form_row(ui, "Name:", |ui| {
            field(ui, "name", &mut form.name, "Traverse", "")
        });
        let kinds: Vec<&str> = KINDS.iter().map(|(label, _)| *label).collect();
        form_row(ui, "Kind:", |ui| {
            choice_of(ui, "kind", &kinds, &mut form.kind).on_hover_text(
                "Closed loop: back to the start. Link: from one known station to another. \
                 Open: nothing checks it.",
            )
        });
        form_row(ui, "Start station:", |ui| {
            field(
                ui,
                "start",
                &mut form.start,
                "E,N or point id",
                "Known coordinates of the first leg's station",
            )
        });
        form_row(ui, "Start reference mark:", |ui| {
            field(
                ui,
                "startMark",
                &mut form.start_mark,
                "E,N or point id",
                "The backsight - for a loop, the station the first leg runs to",
            );
            ui.label("or its azimuth");
            field(
                ui,
                "startAzimuth",
                &mut form.start_azimuth,
                "azimuth or bearing",
                "Used when no reference mark is given",
            );
        });
        form_row(ui, "Legs:", |ui| {
            drive::text_with(
                ui,
                "legs",
                &mut form.legs,
                TextOptions {
                    read_only: false,
                    rows: 8,
                    monospace: true,
                },
            )
            .on_hover_text(
                "One leg per line: station, angle turned there (no blanks inside it), distance \
                 to the next station. '#' starts a comment.",
            )
        });
        form_row(ui, "End station:", |ui| {
            field_if(
                ui,
                kind != TraverseKind::ClosedLoop,
                "endStation",
                &mut form.end_station,
                "its id",
                "The station the last leg ends on (link and open)",
            );
            ui.label("at");
            field_if(
                ui,
                link,
                "end",
                &mut form.end,
                "E,N or point id",
                "Known coordinates of the closing station (link)",
            );
        });
        form_row(ui, "Closing angle:", |ui| {
            field_if(
                ui,
                link,
                "closingAngle",
                &mut form.closing_angle,
                "optional",
                "The angle turned at the end station from the last station to the closing \
                 reference - the angular check of a link",
            )
        });
        form_row(ui, "Closing reference mark:", |ui| {
            field_if(
                ui,
                link,
                "closingMark",
                &mut form.closing_mark,
                "E,N or point id",
                "",
            );
            ui.label("or its azimuth");
            field_if(
                ui,
                link,
                "closingAzimuth",
                &mut form.closing_azimuth,
                "azimuth or bearing",
                "",
            );
        });
        let methods: Vec<&str> = TRAVERSE_METHODS.iter().map(|(label, _)| *label).collect();
        form_row(ui, "Adjustment:", |ui| {
            choice_of(ui, "method", &methods, &mut form.method)
                .on_hover_text("Least squares weighs the observations by the precisions below")
        });
        ui.add_enabled_ui(!least_squares, |ui| {
            drive::check(
                ui,
                "balanceAngles",
                "Balance the angular misclosure first",
                &mut form.balance_angles,
            )
        });
        form_row(ui, "Least-squares precision:", |ui| {
            field_if(
                ui,
                least_squares,
                "angleSigma",
                &mut form.angle_sigma,
                "",
                "Standard deviation of an angle, seconds",
            );
            ui.label("\" angles;");
            field_if(
                ui,
                least_squares,
                "distanceSigma",
                &mut form.distance_sigma,
                "",
                "Standard deviation of a distance: mm",
            );
            ui.label("mm +");
            field_if(
                ui,
                least_squares,
                "distancePpm",
                &mut form.distance_ppm,
                "",
                "... plus parts per million of its length",
            );
            ui.label("ppm distances");
        });
    }

    fn spec(&self, document: &Document) -> Result<TraverseSpec> {
        let form = &self.form;
        let defaults = TraverseSpec::default();
        let name = match text(&form.name) {
            "" => defaults.name.clone(),
            typed => String::from(typed),
        };
        let kind = form.kind();
        let start = in_field(
            "Start station",
            parse_survey_position(document, text(&form.start)),
        )?;
        let start_orientation = orientation(
            document,
            &form.start_mark,
            &form.start_azimuth,
            if kind == TraverseKind::ClosedLoop {
                "First-leg direction"
            } else {
                "Backsight"
            },
        )?;
        let legs = in_field("Legs", parse_traverse_legs(&form.legs))?;
        let mut end_station = String::new();
        if kind != TraverseKind::ClosedLoop {
            end_station = String::from(text(&form.end_station));
            if end_station.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "End station: name the station the last leg ends on",
                ));
            }
        }
        let mut end = defaults.end;
        let mut closing_angle = None;
        let mut closing_orientation = TraverseOrientation::default();
        if kind == TraverseKind::Link {
            end = in_field(
                "End station position",
                parse_survey_position(document, text(&form.end)),
            )?
            .point;
            if !text(&form.closing_angle).is_empty() {
                closing_angle = Some(in_field(
                    "Closing angle",
                    parse_survey_angle(text(&form.closing_angle)),
                )?);
                closing_orientation = orientation(
                    document,
                    &form.closing_mark,
                    &form.closing_azimuth,
                    "Closing reference",
                )?;
            }
        }
        let method = form.method();
        let mut precision: TraverseStochasticModel = defaults.precision;
        if method == TraverseMethod::LeastSquares {
            let angle = parse_survey_number(text(&form.angle_sigma), "the angle precision")?;
            let constant =
                parse_survey_number(text(&form.distance_sigma), "the distance precision")?;
            let ppm = parse_survey_number(text(&form.distance_ppm), "the distance ppm")?;
            precision.angle_sigma = arc_seconds_to_radians(angle);
            precision.distance_sigma_constant = constant / 1000.0; // mm to metres
            precision.distance_sigma_ppm = ppm;
        }
        Ok(TraverseSpec {
            name,
            kind,
            start: start.point,
            start_orientation,
            legs,
            end_station,
            end,
            closing_angle,
            closing_orientation,
            method,
            balance_angles: form.balance_angles,
            precision,
        })
    }

    fn compute(&self, host: &mut dyn Host, add: bool) -> Result<String> {
        let spec = self.spec(host.document())?;
        let result = compute_traverse_tool(&spec)?;
        let report = format_traverse_report(&result);
        if !add {
            return Ok(report);
        }
        let command = traverse_command(host.document(), &result)?;
        host.document_mut().execute(command)?;
        frame_views(host);
        let document = host.document();
        Ok(format!(
            "{report}\n  Added to layer {}: {} entities - the computed stations and the traverse \
             line",
            document.current_layer(),
            document.last_created_entities().len()
        ))
    }
}

// ---- level book -------------------------------------------------------------------------------

/// In the order of the Adjustment choice.
const LEVEL_ADJUSTMENTS: [(&str, LevelAdjustment); 3] = [
    ("None", LevelAdjustment::None),
    ("Equally between setups", LevelAdjustment::BySetups),
    ("By distance levelled", LevelAdjustment::ByDistance),
];

#[derive(Clone, Debug, PartialEq)]
struct LevelBookForm {
    name: String,
    start_level: String,
    closing_level: String,
    book: String,
    adjustment: Choice,
    allowance: String,
}

/// Survey > Level Book... (surveyLevelBookDialog).
pub struct SurveyLevelBookDialog {
    pub tool: SurveyToolFrame,
    form: LevelBookForm,
}

impl Default for SurveyLevelBookDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyLevelBookDialog {
    pub fn new() -> Self {
        Self {
            tool: SurveyToolFrame::new(
                "surveyLevelBookDialog",
                "Level Book",
                "Reduces a level book by height of collimation. One line per reading: point, \
                 backsight, intersight, foresight and - on a foresight line - the length \
                 levelled in that setup, with '-' for an empty column; a change point carries \
                 its foresight and its backsight on one line. The allowable misclosure is k x \
                 sqrt(K km), stated only when every setup has a length; 12 mm is the common \
                 third-order figure - use your specification's.",
            ),
            form: LevelBookForm {
                name: String::new(),
                start_level: String::new(),
                closing_level: String::new(),
                book: String::new(),
                adjustment: Choice::at(0),
                allowance: format_general(THIRD_ORDER_LEVEL_ALLOWANCE * 1000.0, 6),
            },
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            |ui, form| {
                form_row(ui, "Name:", |ui| {
                    field(ui, "name", &mut form.name, "Level run", "")
                });
                form_row(ui, "Opening benchmark RL:", |ui| {
                    field(
                        ui,
                        "startLevel",
                        &mut form.start_level,
                        "reduced level of the first point",
                        "",
                    )
                });
                form_row(ui, "Closing benchmark RL:", |ui| {
                    field(
                        ui,
                        "closingLevel",
                        &mut form.closing_level,
                        "optional",
                        "Known level of the last point; without it the run is not checked",
                    )
                });
                form_row(ui, "Book:", |ui| {
                    drive::text_with(
                        ui,
                        "book",
                        &mut form.book,
                        TextOptions {
                            read_only: false,
                            rows: 8,
                            monospace: true,
                        },
                    )
                    .on_hover_text(
                        "point BS IS FS [distance], '-' for an empty column, '#' a comment",
                    )
                });
                let adjustments: Vec<&str> =
                    LEVEL_ADJUSTMENTS.iter().map(|(label, _)| *label).collect();
                form_row(ui, "Adjustment:", |ui| {
                    choice_of(ui, "adjustment", &adjustments, &mut form.adjustment).on_hover_text(
                        "How the misclosure is shared out; needs a closing benchmark",
                    )
                });
                form_row(ui, "Allowance k (mm per \u{221a}km):", |ui| {
                    field(ui, "allowance", &mut form.allowance, "", "")
                });
            },
            &[],
            &[ToolButton::new("reduce", "Reduce")],
        );
        if pressed == Some("reduce") {
            let report = self.reduce();
            self.tool.verb_done(host, report);
        }
    }

    fn reduce(&self) -> Result<String> {
        let form = &self.form;
        let defaults = LevelBookSpec::default();
        let name = match text(&form.name) {
            "" => defaults.name.clone(),
            typed => String::from(typed),
        };
        let start_level = parse_survey_number(text(&form.start_level), "the opening benchmark RL")?;
        let closing_level = optional_number(&form.closing_level, "the closing benchmark RL")?;
        let lines = in_field("Book", parse_level_book(&form.book))?;
        let adjustment = LEVEL_ADJUSTMENTS[form
            .adjustment
            .index
            .unwrap_or(0)
            .min(LEVEL_ADJUSTMENTS.len() - 1)]
        .1;
        let allowance = parse_survey_number(text(&form.allowance), "the allowance k")?;
        let spec = LevelBookSpec {
            name,
            start_level,
            closing_level,
            lines,
            adjustment,
            allowance_k: allowance / 1000.0, // mm to metres
        };
        let result = compute_level_book(&spec)?;
        Ok(format_level_book_report(&result))
    }
}

// ---- coordinate converter ---------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct ConverterForm {
    source: String,
    target: String,
    points: String,
}

/// Survey > Coordinate Converter... (surveyCoordinateConverterDialog).
pub struct SurveyConverterDialog {
    pub tool: SurveyToolFrame,
    form: ConverterForm,
}

impl Default for SurveyConverterDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyConverterDialog {
    pub fn new() -> Self {
        Self {
            tool: SurveyToolFrame::new(
                "surveyCoordinateConverterDialog",
                "Coordinate Converter",
                "Converts coordinates between two coordinate systems named by EPSG code (4326, \
                 28356, ...) through PROJ, with the grid scale factor and convergence on each \
                 projected side. One point per line: [label] easting northing for a projected \
                 system, [label] latitude longitude for a geographic one, an angle written \
                 without blanks inside it (-33:51:24.5 or -33d51m24.5s). A datum change PROJ \
                 could only do approximately is refused. It reports; it does NOT move anything \
                 in the drawing - converting drawing entities is not part of this tool - and \
                 heights are not converted.",
            ),
            form: ConverterForm::default(),
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            |ui, form| {
                form_row(ui, "From system:", |ui| {
                    field(ui, "source", &mut form.source, "EPSG code, e.g. 4326", "")
                });
                form_row(ui, "To system:", |ui| {
                    field(ui, "target", &mut form.target, "EPSG code, e.g. 28356", "")
                });
                form_row(ui, "Points:", |ui| {
                    drive::text_with(
                        ui,
                        "points",
                        &mut form.points,
                        TextOptions {
                            read_only: false,
                            rows: 8,
                            monospace: true,
                        },
                    )
                    .on_hover_text(
                        "One point per line; Use Selection fills it from the selected points",
                    )
                });
            },
            &[USE_SELECTION],
            &[ToolButton::new("convert", "Convert")],
        );
        match pressed {
            Some("useSelection") => {
                self.tool.pane.message.clear();
                let status =
                    conversion_lines_for_selection(host.document(), text(&self.form.source))
                        .map(|lines| self.form.points = lines);
                self.tool.filler_done(host, status);
            }
            Some("convert") => {
                let report = convert_coordinates(
                    text(&self.form.source),
                    text(&self.form.target),
                    &self.form.points,
                )
                .map(|conversion| format_coordinate_conversion(&conversion));
                self.tool.verb_done(host, report);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sentence;

    #[test]
    fn a_library_phrase_becomes_a_sentence() {
        assert_eq!(sentence("angles are typed so"), "Angles are typed so.");
        assert_eq!(sentence(""), ".");
    }
}
