//! The Survey menu and toolbar (survey_workbench.hpp/.cpp; the owner's request of 2026-09-23:
//! "add survey menus and toolbars and wire the functionality").
//!
//! Every survey action and dialog belongs to this workbench. The window composes it with the
//! others (shell::workbench): it draws the rows this delivers, asks it to trigger its actions and
//! to draw its dialogs and its dock, and lends itself as the host for each call - so the survey
//! work, which grew a wizard, a point manager and reports, stays out of the window's own source.
//!
//! The menu, in sections:
//!
//! ```text
//! Survey Points           Import Survey Points..., Survey Jobs..., Export Survey Points...,
//!                         Point Manager (a dock), Point Report...
//! Coordinate Geometry     Inverse..., Forward Point..., Area of Selection, Parcel Report...,
//!                         Angle and Bearing Calculator...
//! Traverse and Levelling  Traverse..., Level Book...
//! Coordinates             Coordinate Converter...
//! Survey Coding           the Format menu's Survey Code Manager, and the window's Apply Survey
//!                         Codes (the CODE line) and Process Linework (the LINEWORK line), handed
//!                         over by the window (SurveyServices)
//! ```
//!
//! Each action's object name (surveyImport, surveyJobs, surveyExport, surveyPointManager,
//! surveyPointReport, surveyInverse, surveyForward, surveyArea, surveyParcelReport,
//! surveyAngleCalculator, surveyTraverse, surveyLevelBook, surveyCoordinateConverter) is what
//! --action and --dialog know it by. A dialog's object name is its action's plus "Dialog"; the
//! Point Manager is the dock SurveyPointsDock, which --survey-dock knows it by (the action's data,
//! `Action::opens`).
//!
//! The dock is registered with the window (`docks`, drawn by `dock_ui`); its title bar - Minimise,
//! Float, Close - is the window's dock chrome, as every panel's is.
//!
//! The computing is katana_cad's (survey_tools); the workbench and its dialogs only gather input
//! and show the reports.

use egui::Ui;
use katana_cad::survey_tools::{compute_area, format_area_report};

use super::survey_dialogs::{
    SurveyAngleDialog, SurveyConverterDialog, SurveyForwardDialog, SurveyInverseDialog,
    SurveyLevelBookDialog, SurveyTraverseDialog,
};
use super::survey_import_wizard::SurveyImportWizard;
use super::survey_jobs_dialog::SurveyJobsDialog;
use super::survey_parcel_dialog::SurveyParcelDialog;
use super::survey_points_ui::{SurveyExportDialog, SurveyPointReportDialog, SurveyPointsDock};
use crate::drive::{Action, Menu, ToolBar, ToolBarItem};
use crate::host::Host;
use crate::shell::{DockSpec, Workbench};

/// The Point Manager's dock: its object name and title.
pub const POINTS_DOCK: &str = "SurveyPointsDock";
const POINTS_DOCK_TITLE: &str = "Survey Points";

/// What the window hands the workbench (the C++ SurveyServices, less what the host is).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyServices {
    /// The Format menu's Survey Code Manager, shown under Survey Coding as well: where a surveyor
    /// looks for the code library. The window's row; None in a test.
    pub code_manager: Option<Action>,
    /// The window's Apply Survey Codes, which runs the CODE line on the selection, or on the
    /// drawing when nothing is selected: under Survey Coding and on the toolbar. None in a test.
    pub apply_survey_codes: Option<Action>,
    /// The window's Process Linework, which runs the LINEWORK line the same way: under Survey
    /// Coding, after Apply Survey Codes - coded points are what it strings. None in a test.
    pub survey_linework: Option<Action>,
    /// Whether the window's one executor is there for a dialog that changes the drawing through a
    /// verb line (the Parcel Report's labels). False in a test that gives none.
    pub executor: bool,
}

/// One survey action's row: object name, text, icon, status tip.
fn action(name: &str, text: &str, icon: &'static str, tip: &str) -> Action {
    Action::new(name, text).icon(icon).tip(tip)
}

/// The survey workbench (the C++ SurveyWorkbench).
pub struct SurveyWorkbench {
    services: SurveyServices,
    // One of each, made the first time it is opened and kept between uses, so that what was
    // typed survives closing it.
    inverse: Option<SurveyInverseDialog>,
    forward: Option<SurveyForwardDialog>,
    angle: Option<SurveyAngleDialog>,
    traverse: Option<SurveyTraverseDialog>,
    level_book: Option<SurveyLevelBookDialog>,
    converter: Option<SurveyConverterDialog>,
    import: Option<SurveyImportWizard>,
    jobs: Option<SurveyJobsDialog>,
    export: Option<SurveyExportDialog>,
    point_report: Option<SurveyPointReportDialog>,
    parcel: Option<SurveyParcelDialog>,
    point_manager: Option<SurveyPointsDock>,
    point_manager_shown: bool,
}

impl SurveyWorkbench {
    pub fn new(services: SurveyServices) -> Self {
        Self {
            services,
            inverse: None,
            forward: None,
            angle: None,
            traverse: None,
            level_book: None,
            converter: None,
            import: None,
            jobs: None,
            export: None,
            point_report: None,
            parcel: None,
            point_manager: None,
            point_manager_shown: false,
        }
    }

    /// Whether the Point Manager's dock is shown (its action ticked).
    pub fn point_manager_shown(&self) -> bool {
        self.point_manager_shown
    }

    /// Shows or hides the Point Manager, made the first time it is shown. Closing the dock by its
    /// own button unticks the action: the window tells the workbench here.
    pub fn show_point_manager(&mut self, show: bool) {
        if show && self.point_manager.is_none() {
            self.point_manager = Some(SurveyPointsDock::new());
        }
        self.point_manager_shown = show && self.point_manager.is_some();
    }

    /// The Point Manager, once made.
    pub fn point_manager(&self) -> Option<&SurveyPointsDock> {
        self.point_manager.as_ref()
    }

    fn survey_points_actions(&self) -> Vec<Action> {
        vec![
            action(
                "surveyImport",
                "&Import Survey Points...",
                "SurveyImport",
                "Read a file of surveyed points into the drawing, one undoable step",
            ),
            action(
                "surveyJobs",
                "Survey &Jobs...",
                "SurveyPointReport",
                "The field files imported as survey jobs: their reports, and adjusting them again",
            ),
            action(
                "surveyExport",
                "&Export Survey Points...",
                "SurveyExport",
                "Write the drawing's survey points to a delimited text file",
            ),
            action(
                "surveyPointManager",
                "Point &Manager",
                "SurveyPointManager",
                "The drawing's survey points in a table: filter, sort, select, zoom to",
            )
            .checkable(self.point_manager_shown)
            .opens(POINTS_DOCK),
            action(
                "surveyPointReport",
                "Point Re&port...",
                "SurveyPointReport",
                "The survey points as a report, to copy or save as CSV",
            ),
        ]
    }

    fn geometry_actions() -> Vec<Action> {
        vec![
            action(
                "surveyInverse",
                "In&verse...",
                "SurveyInverse",
                "Distance, azimuth, bearing and height difference between two points",
            ),
            action(
                "surveyForward",
                "&Forward Point...",
                "SurveyForward",
                "A new point from a start, a direction and a distance (radiation)",
            ),
            action(
                "surveyArea",
                "&Area of Selection",
                "SurveyArea",
                "Area and perimeter of the selected closed polylines and circles",
            ),
            // No mnemonic: every letter of its name is taken in this menu, and a shared one
            // would make Alt+letter cycle instead of choose.
            action(
                "surveyParcelReport",
                "Parcel Report...",
                "SurveyArea",
                "A closed polyline's courses, area and legal description, and course labels",
            ),
            action(
                "surveyAngleCalculator",
                "Angle and &Bearing Calculator...",
                "SurveyAngle",
                "Convert angles between DMS, decimal degrees, gons, radians and bearings",
            ),
        ]
    }

    fn traverse_actions() -> Vec<Action> {
        vec![
            action(
                "surveyTraverse",
                "&Traverse...",
                "SurveyTraverse",
                "Compute and adjust a traverse; add its stations to the drawing",
            ),
            action(
                "surveyLevelBook",
                "&Level Book...",
                "SurveyLevelBook",
                "Reduce a level book: reduced levels, misclosure and allowance",
            ),
        ]
    }

    fn converter_action() -> Action {
        action(
            "surveyCoordinateConverter",
            "&Coordinate Converter...",
            "SurveyConverter",
            "Convert coordinates between coordinate systems by EPSG code, with grid factors",
        )
    }

    /// Survey > Area of Selection: no dialog - the report goes to the log.
    fn area_of_selection(&mut self, host: &mut dyn Host) {
        let document = host.document();
        if document.selection().empty() {
            host.log(
                "Area of Selection: nothing is selected - select closed polylines or circles",
                true,
            );
            return;
        }
        let ids = document.selection().ids();
        match compute_area(document, &ids) {
            Err(error) => host.log(&format!("Area of Selection: {}", error.describe()), true),
            Ok(result) => host.log(&format_area_report(&result), false),
        }
    }
}

impl Workbench for SurveyWorkbench {
    fn menus(&self) -> Vec<Menu> {
        // Sections rather than plain separators, as the GIS menu has them: a style that draws
        // their titles says what each group is for.
        let mut menu = Menu::new("surveyMenu", "&Survey").section("Survey Points");
        for each in self.survey_points_actions() {
            menu = menu.action(each);
        }
        menu = menu.section("Coordinate Geometry");
        for each in Self::geometry_actions() {
            menu = menu.action(each);
        }
        menu = menu.section("Traverse and Levelling");
        for each in Self::traverse_actions() {
            menu = menu.action(each);
        }
        menu = menu
            .section("Coordinates")
            .action(Self::converter_action())
            .section("Survey Coding");
        for each in [
            &self.services.code_manager,
            &self.services.apply_survey_codes,
            &self.services.survey_linework,
        ]
        .into_iter()
        .flatten()
        {
            menu = menu.action(each.clone());
        }
        vec![menu]
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        let mut bar = ToolBar::new("SurveyToolBar", "Survey");
        let points = self.survey_points_actions();
        // The toolbar has the first four: the Point Report is a menu item only.
        bar.items
            .extend(points.into_iter().take(4).map(ToolBarItem::Action));
        bar.items.push(ToolBarItem::Separator);
        bar.items.extend(
            Self::geometry_actions()
                .into_iter()
                .map(ToolBarItem::Action),
        );
        bar.items.push(ToolBarItem::Separator);
        bar.items.extend(
            Self::traverse_actions()
                .into_iter()
                .map(ToolBarItem::Action),
        );
        bar.items.push(ToolBarItem::Separator);
        bar.items
            .push(ToolBarItem::Action(Self::converter_action()));
        if let Some(apply) = &self.services.apply_survey_codes {
            bar.items.push(ToolBarItem::Separator);
            bar.items.push(ToolBarItem::Action(apply.clone()));
        }
        vec![bar]
    }

    fn docks(&self) -> Vec<DockSpec> {
        if self.point_manager_shown {
            vec![DockSpec::new(POINTS_DOCK, POINTS_DOCK_TITLE)]
        } else {
            Vec::new()
        }
    }

    /// The survey workbench runs no line of its own: SURVEY is the window's dispatcher's
    /// (katana_app's survey verbs), as the C++ workbench had no runLine.
    fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        let _ = (host, line);
        false
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        match name {
            "surveyImport" => self
                .import
                .get_or_insert_with(SurveyImportWizard::new)
                .show(),
            "surveyJobs" => self.jobs.get_or_insert_with(SurveyJobsDialog::new).show(),
            "surveyExport" => self
                .export
                .get_or_insert_with(SurveyExportDialog::new)
                .show(),
            "surveyPointManager" => {
                let shown = self.point_manager_shown;
                self.show_point_manager(!shown);
            }
            "surveyPointReport" => self
                .point_report
                .get_or_insert_with(SurveyPointReportDialog::new)
                .show(),
            "surveyInverse" => self
                .inverse
                .get_or_insert_with(SurveyInverseDialog::new)
                .show(),
            "surveyForward" => self
                .forward
                .get_or_insert_with(SurveyForwardDialog::new)
                .show(),
            "surveyArea" => self.area_of_selection(host),
            "surveyParcelReport" => {
                let executor = self.services.executor;
                self.parcel
                    .get_or_insert_with(|| SurveyParcelDialog::new(executor))
                    .show();
            }
            "surveyAngleCalculator" => self.angle.get_or_insert_with(SurveyAngleDialog::new).show(),
            "surveyTraverse" => self
                .traverse
                .get_or_insert_with(SurveyTraverseDialog::new)
                .show(),
            "surveyLevelBook" => self
                .level_book
                .get_or_insert_with(SurveyLevelBookDialog::new)
                .show(),
            "surveyCoordinateConverter" => self
                .converter
                .get_or_insert_with(SurveyConverterDialog::new)
                .show(),
            _ => return false,
        }
        true
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if let Some(dialog) = &mut self.inverse {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.forward {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.angle {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.traverse {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.level_book {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.converter {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.import {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.jobs {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.export {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.point_report {
            dialog.ui(ui, host);
        }
        if let Some(dialog) = &mut self.parcel {
            dialog.ui(ui, host);
        }
    }

    fn dock_ui(&mut self, ui: &mut Ui, host: &mut dyn Host, dock: &str) {
        if dock == POINTS_DOCK
            && self.point_manager_shown
            && let Some(points) = &mut self.point_manager
        {
            points.ui(ui, host);
        }
    }
}
