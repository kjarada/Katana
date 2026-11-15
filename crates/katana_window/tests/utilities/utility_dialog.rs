//! The twins of tests/qt_widgets/survey/test_utility_dialog.cpp: Survey > Subsurface Utilities (AS
//! 5488), the dialog (survey::utility_dialog) and its workbench (survey::utility_workbench), driven
//! by their object names as a person drives them by clicking.
//!
//! The dialog runs nothing of the AS 5488 library: it writes the UTILITY line its fields describe
//! and hands it to an executor. So what is tested here is that line - exactly the one a person
//! would type, for every tab and for both sources, a schedule file or what is drawn by the shared
//! scope and filter controls - that a line which cannot be written runs nothing and says why, that
//! the executor's reply is what the dialog shows, and that the workbench opens the dialog on the tab
//! of the menu item chosen and frames a DRAW's or a REGRADE's bounds in the plan views. The
//! executors and the interpreter are the tests' own, so nothing here needs the verb itself except
//! where a case runs the real one. Expected values are the C++'s.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use katana_cad::CommandInterpreter;
use katana_cad::view_set::{ViewId, ViewSet};
use katana_cad::viewport_layout::ViewKind;
use katana_commands::{create_layer, set_entity_property};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::PropertyValue;
use katana_entity::tables::Layer;
use katana_geometry::{Box2, Point2};
use katana_window::customisation::{ScopeChoice, ScopeFilterView};
use katana_window::drive::{Harness, MenuItem, WorkbenchHost};
use katana_window::host::{
    Answer, Clipboard, GeoPhase, Host, JobAwait, JobId, Jobs, PickId, RecordingHost, UserSettings,
    VerbOutcome, ViewAccess,
};
use katana_window::survey::utility_dialog::{
    UTILITY_TOOLS, UtilityDesignSource, UtilityDialogContext, UtilityExecute, UtilityForm,
    UtilitySource, UtilityTool, UtilityToolsDialog, utility_command_line, utility_source_of,
};
use katana_window::survey::utility_workbench::{
    InterpretFn, UtilityLines, UtilityServices, UtilityWorkbench,
};

use super::Bench;
use super::window_support::Scratch;

/// A DRAW reply as the contract writes it: the summary record, then a record per line.
const DRAW_REPLY: &str = "utilities drawn lines=2 vertices=5 segments=3 entities=9 layers=5 \
                          bounds=334000.000,6250000.000,334040.000,6250007.200\n\
                          line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 \
                          ql_c=12.502 ql_d=0.000\n\
                          line id=G1 type=gas length=40.000 ql_a=0.000 ql_b=0.000 ql_c=0.000 \
                          ql_d=40.000\n";

/// What a dialog handed its executor, and what the executor answers.
struct ExecutorState {
    lines: Vec<String>,
    reply: Result<String>,
}

#[derive(Clone)]
struct Executor {
    state: Rc<RefCell<ExecutorState>>,
}

impl Executor {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(ExecutorState {
                lines: Vec::new(),
                reply: Ok(String::from("done\n")),
            })),
        }
    }

    fn context(&self) -> UtilityDialogContext {
        let state = Rc::clone(&self.state);
        let execute: UtilityExecute = Box::new(move |_host: &mut dyn Host, line: &str| {
            let mut state = state.borrow_mut();
            state.lines.push(String::from(line));
            state.reply.clone()
        });
        UtilityDialogContext {
            execute: Some(execute),
            ..UtilityDialogContext::default()
        }
    }

    fn lines(&self) -> Vec<String> {
        self.state.borrow().lines.clone()
    }

    fn answer(&self, reply: Result<String>) {
        self.state.borrow_mut().reply = reply;
    }
}

/// A host with no drawing of the test's, headless or not.
fn host(headless: bool) -> RecordingHost {
    let mut host = RecordingHost::new();
    host.parts.headless = headless;
    host
}

/// The dialog over `context`, shown, on the harness.
fn dialog_bench(context: UtilityDialogContext, host: RecordingHost) -> Bench<UtilityToolsDialog> {
    let mut dialog = UtilityToolsDialog::new(context, &host);
    dialog.show(&host);
    Bench::new("utilityDialog", dialog, host)
}

fn form_for(tool: UtilityTool) -> UtilityForm {
    UtilityForm {
        tool,
        schedule: String::from("C:/survey/schedule.csv"),
        ..UtilityForm::default()
    }
}

fn drawn(tool: UtilityTool, scope: &str) -> UtilityForm {
    UtilityForm {
        tool,
        source: UtilitySource::Drawing,
        scope: String::from(scope),
        ..UtilityForm::default()
    }
}

fn line_of(form: &UtilityForm) -> String {
    utility_command_line(form).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn refusal_of(form: &UtilityForm) -> String {
    match utility_command_line(form) {
        Ok(line) => panic!("written as {line}"),
        Err(error) => {
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            error.message
        }
    }
}

/// A drawing with a layer for the scope to list, and one plan view (id 4) for its View choice to
/// offer: the C++'s DrawnBench.
fn drawn_host() -> RecordingHost {
    let mut host = host(true);
    host.document_mut()
        .execute(create_layer(Layer {
            name: String::from("utilities/water/points"),
            ..Layer::default()
        }))
        .expect("the layer is made");
    host
}

fn drawn_context(executor: &Executor) -> UtilityDialogContext {
    let mut context = executor.context();
    context.reads_drawing = true;
    context.views = Some(Box::new(|| {
        vec![ScopeFilterView {
            id: 4,
            title: String::from("Plan 2"),
            hidden: None,
            on_screen: Some(Box2::new(Point2::new(0.0, 0.0), Point2::new(40.0, 20.0))),
        }]
    }));
    context
}

/// An executor that runs the real verb on the host's drawing, recording each line.
fn interpreted(
    interpreter: &Rc<RefCell<CommandInterpreter>>,
    ran: &Rc<RefCell<Vec<String>>>,
) -> UtilityExecute {
    let interpreter = Rc::clone(interpreter);
    let ran = Rc::clone(ran);
    Box::new(move |host: &mut dyn Host, line: &str| {
        ran.borrow_mut().push(String::from(line));
        interpreter.borrow_mut().run(host.document_mut(), line)
    })
}

mod utility_dialog {
    use super::*;

    #[test]
    fn every_control_has_its_object_name() {
        let executor = Executor::new();
        let mut bench = dialog_bench(executor.context(), host(false));
        assert!(!bench.dialog.is_modal());
        assert!(
            !bench
                .harness
                .target_record("utilityDialog")
                .expect("the dialog is drawn")
                .modal
        );
        // A tab draws only its own page: each is brought forward in turn, as a person would to
        // reach its controls.
        let mut drawn: BTreeSet<String> = BTreeSet::new();
        for tool in UTILITY_TOOLS {
            bench.dialog.show_tool(tool);
            bench.draw();
            drawn.extend(
                bench
                    .harness
                    .snapshot()
                    .in_target("utilityDialog")
                    .map(|control| control.name.clone()),
            );
        }
        for name in [
            "utilityTabs",
            "utilitySchedule",
            "utilityScheduleBrowse",
            "utilityDesign",
            "utilityDesignBrowse",
            "utilitySchema",
            "utilitySchemaBrowse",
            "utilityMinCover",
            "utilitySpacing",
            "utilityWidth",
            "utilityH",
            "utilityV",
            "utilityMargin",
            "utilityLayerPrefix",
            "utilityCommand",
            "utilityRun",
            "utilityOutput",
            "utilityCopy",
            "utilitySave",
            "utilityStatus",
            "utilityClose",
            // Where the services come from, and the drawing's scope and filter.
            "utilitySourceFile",
            "utilitySourceDrawing",
            "utilityScope",
            "utilityScopeSelection",
            "utilityScopeView",
            "utilityView",
            "utilityOnScreen",
            "utilityScopeLayers",
            "utilityLayers",
            "utilitySublayers",
            "utilityScopeDrawing",
            "utilityTypePoint",
            "utilityFilterLayer",
            "utilityFilterProperty",
            "utilityFilterValue",
            "utilityDrawnOnly",
            // Clearance's works, and the Schedule tab.
            "utilityDesignFile",
            "utilityDesignEntity",
            "utilityDesignEntityId",
            "utilityDesignUseSelected",
            "utilityDesignLevel",
            "utilityDesignAlignment",
            "utilityDesignAlignmentName",
            "utilityScheduleOut",
            "utilityScheduleOutBrowse",
            "utilityScheduleSchema",
            "utilityScheduleSchemaBrowse",
            // Draw's fields for geometry in the drawing.
            "utilityGeometryGroup",
            "utilityServiceType",
            "utilityMethod",
            "utilityHUnc",
            "utilityVUnc",
            "utilityHeights",
            "utilityLevelRef",
            "utilityPath",
            "utilityOwner",
            "utilityMaterial",
            "utilityDiameter",
            "utilityServiceStatus",
            "utilityFields",
        ] {
            assert!(drawn.contains(name), "{name}");
        }
        // Seven since the tools act on what is drawn: Regrade and Schedule joined the five
        // (docs/subsurface_utilities.md, "Drawing data").
        assert_eq!(
            bench.control("utilityTabs").items,
            [
                "Draw",
                "Report",
                "Verify",
                "Clearance",
                "Check",
                "Regrade",
                "Schedule"
            ]
        );
        // What is shown is not edited: the line and the reply are read-only - what is written
        // there is drawn over by the dialog's own.
        bench.fill("utilityCommand", "typed by hand");
        assert_eq!(bench.text("utilityCommand"), "");
        bench.fill("utilityOutput", "typed by hand");
        assert_eq!(bench.text("utilityOutput"), "");
    }

    #[test]
    fn each_tab_writes_the_line_the_verb_reads() {
        let draw = UtilityForm {
            spacing: String::from("8"),
            min_cover: String::from("0.6"),
            layer_prefix: String::from("services/existing"),
            ..form_for(UtilityTool::Draw)
        };
        assert_eq!(
            utility_command_line(&draw).unwrap_or_default(),
            "UTILITY DRAW C:/survey/schedule.csv SPACING 8 MINCOVER 0.6 LAYER services/existing"
        );

        let report = UtilityForm {
            min_cover: String::from("0.6"),
            spacing: String::from("12.5"),
            ..form_for(UtilityTool::Report)
        };
        assert_eq!(
            utility_command_line(&report).unwrap_or_default(),
            "UTILITY REPORT C:/survey/schedule.csv MINCOVER 0.6 SPACING 12.5"
        );

        assert_eq!(
            utility_command_line(&form_for(UtilityTool::Verify)).unwrap_or_default(),
            "UTILITY VERIFY C:/survey/schedule.csv"
        );

        let clearance = UtilityForm {
            design: String::from("C:/survey/design.csv"),
            width: String::from("0.375"),
            horizontal: String::from("0.5"),
            vertical: String::from("0.3"),
            margin: String::from("2"),
            ..form_for(UtilityTool::Clearance)
        };
        assert_eq!(
            utility_command_line(&clearance).unwrap_or_default(),
            "UTILITY CLEARANCE C:/survey/schedule.csv C:/survey/design.csv WIDTH 0.375 H 0.5 V 0.3 \
             MARGIN 2"
        );

        let check = UtilityForm {
            schema: String::from("C:/survey/schema.csv"),
            ..form_for(UtilityTool::Check)
        };
        assert_eq!(
            utility_command_line(&check).unwrap_or_default(),
            "UTILITY CHECK C:/survey/schedule.csv SCHEMA C:/survey/schema.csv"
        );
    }

    #[test]
    fn a_path_with_blanks_is_quoted_and_a_blank_option_is_left_out() {
        let clearance = UtilityForm {
            tool: UtilityTool::Clearance,
            schedule: String::from("  C:/Site Survey/schedule.csv "),
            design: String::from("C:/Design Team/works.csv"),
            vertical: String::from(" 0.45 "),
            ..UtilityForm::default()
        };
        assert_eq!(
            utility_command_line(&clearance).unwrap_or_default(),
            "UTILITY CLEARANCE \"C:/Site Survey/schedule.csv\" \"C:/Design Team/works.csv\" V 0.45"
        );

        let draw = UtilityForm {
            tool: UtilityTool::Draw,
            schedule: String::from("C:/Site Survey/schedule.csv"),
            layer_prefix: String::from("existing services"),
            ..UtilityForm::default()
        };
        assert_eq!(
            utility_command_line(&draw).unwrap_or_default(),
            "UTILITY DRAW \"C:/Site Survey/schedule.csv\" LAYER \"existing services\""
        );

        let check = UtilityForm {
            tool: UtilityTool::Check,
            schedule: String::from("C:/survey/schedule.csv"),
            schema: String::from("C:/Client Files/schema.csv"),
            ..UtilityForm::default()
        };
        assert_eq!(
            utility_command_line(&check).unwrap_or_default(),
            "UTILITY CHECK C:/survey/schedule.csv SCHEMA \"C:/Client Files/schema.csv\""
        );
    }

    #[test]
    fn only_the_chosen_tools_fields_are_read() {
        // Numbers typed on other tabs, even ones that do not read, do not stop the tool that is
        // chosen.
        let verify = UtilityForm {
            min_cover: String::from("deep"),
            width: String::from("wide"),
            spacing: String::from("far"),
            design: String::from("C:/survey/design.csv"),
            ..form_for(UtilityTool::Verify)
        };
        assert_eq!(
            utility_command_line(&verify).unwrap_or_default(),
            "UTILITY VERIFY C:/survey/schedule.csv"
        );
    }

    #[test]
    fn a_file_left_empty_or_a_number_that_does_not_read_writes_no_line() {
        let blank = UtilityForm {
            tool: UtilityTool::Report,
            schedule: String::from("   "),
            ..UtilityForm::default()
        };
        let error = utility_command_line(&blank).expect_err("no schedule");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("utility schedule"));

        let error = utility_command_line(&form_for(UtilityTool::Clearance)).expect_err("no design");
        assert!(error.message.contains("design"));

        let error = utility_command_line(&form_for(UtilityTool::Check)).expect_err("no schema");
        assert!(error.message.contains("delivery schema"));

        let comma = UtilityForm {
            min_cover: String::from("0,6"),
            ..form_for(UtilityTool::Report)
        };
        let error = utility_command_line(&comma).expect_err("a comma");
        assert!(
            error
                .message
                .contains("minimum cover must be a number of metres, not '0,6'"),
            "{}",
            error.message
        );

        let words = UtilityForm {
            spacing: String::from("ten"),
            ..form_for(UtilityTool::Draw)
        };
        let error = utility_command_line(&words).expect_err("a word");
        assert!(error.message.contains("detected spacing"));

        let shallow = UtilityForm {
            min_cover: String::from("0.6 m"),
            ..form_for(UtilityTool::Draw)
        };
        let error = utility_command_line(&shallow).expect_err("a unit");
        assert!(
            error
                .message
                .contains("minimum cover must be a number of metres, not '0.6 m'"),
            "{}",
            error.message
        );

        let quote = UtilityForm {
            layer_prefix: String::from("say \"when\""),
            ..form_for(UtilityTool::Draw)
        };
        let error = utility_command_line(&quote).expect_err("a quote");
        assert!(error.message.contains("double quote"));
    }

    #[test]
    fn the_command_field_shows_the_line_as_it_will_run() {
        let executor = Executor::new();
        let mut bench = dialog_bench(executor.context(), host(false));
        bench.dialog.show_tool(UtilityTool::Report);
        bench.draw();
        // Nothing to run yet, and the field says why rather than showing a line.
        let command = bench.control("utilityCommand");
        assert!(command.value.is_empty());
        assert!(
            command.text.contains("utility schedule"),
            "{}",
            command.text
        );

        bench.fill("utilitySchedule", "C:/survey/schedule.csv");
        bench.fill("utilityMinCover", "0.6");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY REPORT C:/survey/schedule.csv MINCOVER 0.6"
        );

        // The tab is part of the line: the same schedule, another tool.
        bench.dialog.show_tool(UtilityTool::Verify);
        bench.draw();
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY VERIFY C:/survey/schedule.csv"
        );
        // The detected spacing and the minimum cover grade, so they are live only where something
        // is graded.
        assert!(!bench.enabled("utilitySpacing"));
        assert!(!bench.enabled("utilityMinCover"));
        bench.dialog.show_tool(UtilityTool::Draw);
        bench.draw();
        assert!(bench.enabled("utilitySpacing"));
        assert!(bench.enabled("utilityMinCover"));
        // The minimum cover given for the report is the draw's too: the verb flags each drawn
        // point against it.
        bench.fill("utilityLayerPrefix", "services");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY DRAW C:/survey/schedule.csv MINCOVER 0.6 LAYER services"
        );
    }

    #[test]
    fn bad_input_runs_nothing_and_says_why() {
        let executor = Executor::new();
        let mut bench = dialog_bench(executor.context(), host(false));
        bench.dialog.show_tool(UtilityTool::Draw);
        bench.draw();
        bench.press("utilityRun");
        assert!(executor.lines().is_empty());
        let status = bench.text("utilityStatus");
        assert!(status.contains("utility schedule"), "{status}");

        bench.fill("utilitySchedule", "C:/survey/schedule.csv");
        bench.fill("utilitySpacing", "10 m");
        bench.press("utilityRun");
        assert!(executor.lines().is_empty());
        assert!(bench.text("utilityStatus").contains("not '10 m'"));

        bench.fill("utilitySpacing", "");
        bench.dialog.show_tool(UtilityTool::Clearance);
        bench.draw();
        bench.press("utilityRun");
        assert!(executor.lines().is_empty());
        assert!(bench.text("utilityStatus").contains("design"));

        bench.dialog.show_tool(UtilityTool::Check);
        bench.draw();
        bench.press("utilityRun");
        assert!(executor.lines().is_empty());
        assert!(bench.text("utilityStatus").contains("delivery schema"));
        // Nothing ran, so there is nothing to copy or save.
        assert!(!bench.enabled("utilityCopy"));
        assert!(!bench.enabled("utilitySave"));
    }

    #[test]
    fn run_hands_the_line_to_the_executor_and_shows_its_reply() {
        let executor = Executor::new();
        executor.answer(Ok(String::from(DRAW_REPLY)));
        let mut bench = dialog_bench(executor.context(), host(false));
        bench.dialog.show_tool(UtilityTool::Draw);
        bench.draw();
        bench.fill("utilitySchedule", "C:/Site Survey/schedule.csv");
        bench.fill("utilitySpacing", "12");
        bench.press("utilityRun");

        assert_eq!(
            executor.lines(),
            ["UTILITY DRAW \"C:/Site Survey/schedule.csv\" SPACING 12"]
        );
        // The reply whole, without the newline that ends it.
        assert_eq!(format!("{}\n", bench.text("utilityOutput")), DRAW_REPLY);
        assert!(bench.text("utilityStatus").contains("one undo step"));

        bench.press("utilityCopy");
        let copied = bench.host.clipboard().text();
        assert_eq!(copied, Some(bench.text("utilityOutput")));
        assert!(bench.enabled("utilitySave"));
        let folder = Scratch::new("utility_dialog_run_hands_the_line");
        let path = folder.path("drawn.txt");
        bench
            .dialog
            .save_output_to(&path)
            .expect("the reply is saved");
        let saved = std::fs::read_to_string(&path).expect("the file reads");
        assert_eq!(saved, DRAW_REPLY);
    }

    #[test]
    fn a_failed_line_shows_why_in_the_output_and_its_first_line_in_the_status() {
        let executor = Executor::new();
        executor.answer(Err(Error::new(
            ErrorCode::InvalidArgument,
            "7 rows: 2 errors\n  AssetStatus \"In service\" is not listed",
        )));
        let mut bench = dialog_bench(executor.context(), host(false));
        bench.dialog.show_tool(UtilityTool::Check);
        bench.draw();
        bench.fill("utilitySchedule", "C:/survey/schedule.csv");
        bench.fill("utilitySchema", "C:/survey/schema.csv");
        bench.press("utilityRun");

        assert_eq!(
            executor.lines(),
            ["UTILITY CHECK C:/survey/schedule.csv SCHEMA C:/survey/schema.csv"]
        );
        assert_eq!(
            bench.text("utilityOutput"),
            "InvalidArgument: 7 rows: 2 errors\n  AssetStatus \"In service\" is not listed"
        );
        assert_eq!(bench.text("utilityStatus"), "7 rows: 2 errors");
        // What failed is still a report worth keeping.
        assert!(bench.enabled("utilityCopy"));
    }

    #[test]
    fn save_as_names_the_tool_whose_reply_is_shown_not_the_tab_in_front() {
        let executor = Executor::new();
        executor.answer(Ok(String::from(DRAW_REPLY)));
        let mut bench = dialog_bench(executor.context(), host(false));
        bench.dialog.show_tool(UtilityTool::Report);
        assert_eq!(bench.dialog.suggested_file_name(), "utility_report.txt");
        bench.dialog.show_tool(UtilityTool::Draw);
        bench.draw();
        bench.fill("utilitySchedule", "C:/survey/schedule.csv");
        bench.press("utilityRun");
        assert_eq!(bench.dialog.suggested_file_name(), "utility_draw.txt");
        // The records stay while the next run is prepared on another tab, and they are still the
        // draw's.
        bench.dialog.show_tool(UtilityTool::Report);
        bench.draw();
        assert_eq!(format!("{}\n", bench.text("utilityOutput")), DRAW_REPLY);
        assert_eq!(bench.dialog.suggested_file_name(), "utility_draw.txt");
        // A check that failed is still the check's report.
        executor.answer(Err(Error::new(
            ErrorCode::InvalidArgument,
            "7 rows: 2 errors",
        )));
        bench.dialog.show_tool(UtilityTool::Check);
        bench.draw();
        bench.fill("utilitySchema", "C:/survey/schema.csv");
        bench.press("utilityRun");
        bench.dialog.show_tool(UtilityTool::Verify);
        assert_eq!(bench.dialog.suggested_file_name(), "utility_check.txt");
    }

    #[test]
    fn a_headless_session_opens_no_file_dialog() {
        let executor = Executor::new();
        let mut bench = dialog_bench(executor.context(), host(true));
        // Were a file dialog opened here, the test would wait on it forever.
        bench.press("utilityScheduleBrowse");
        assert!(bench.text("utilityStatus").contains("fill utilitySchedule"));
        // The schema's Browse is on the Check tab, brought forward as a person would.
        bench.dialog.show_tool(UtilityTool::Check);
        bench.draw();
        bench.press("utilitySchemaBrowse");
        assert!(bench.text("utilityStatus").contains("fill utilitySchema"));
    }

    // ---- what is drawn as the source (docs/cad.md, "Scope and filter") --------------------------

    #[test]
    fn on_what_is_drawn_each_tab_writes_the_scope_words_where_the_schedule_was() {
        let mut report = UtilityForm {
            min_cover: String::from("0.6"),
            spacing: String::from("12.5"),
            ..drawn(
                UtilityTool::Report,
                "LAYERS utilities/water WHERE TYPE=point",
            )
        };
        assert_eq!(
            line_of(&report),
            "UTILITY REPORT LAYERS utilities/water WHERE TYPE=point MINCOVER 0.6 SPACING 12.5"
        );
        // The schedule's path is not read when the drawing is the source.
        report.schedule = String::from("C:/survey/schedule.csv");
        assert_eq!(
            line_of(&report),
            "UTILITY REPORT LAYERS utilities/water WHERE TYPE=point MINCOVER 0.6 SPACING 12.5"
        );
        assert_eq!(
            line_of(&drawn(UtilityTool::Verify, "VIEW 3")),
            "UTILITY VERIFY VIEW 3"
        );
        let check = UtilityForm {
            schema: String::from("C:/Client Files/schema.csv"),
            ..drawn(UtilityTool::Check, "SELECTION")
        };
        assert_eq!(
            line_of(&check),
            "UTILITY CHECK SELECTION SCHEMA \"C:/Client Files/schema.csv\""
        );
    }

    #[test]
    fn clearance_measures_against_a_design_file_a_drawn_line_or_an_alignment() {
        // A schedule against a design file: the positional line it always was.
        let mut file = UtilityForm {
            design: String::from("C:/survey/design.csv"),
            ..form_for(UtilityTool::Clearance)
        };
        assert_eq!(
            line_of(&file),
            "UTILITY CLEARANCE C:/survey/schedule.csv C:/survey/design.csv"
        );
        // On what is drawn the works follow DESIGN, which also ends the filter.
        let drawn_file = UtilityForm {
            design: String::from("C:/Design Team/works.csv"),
            width: String::from("0.375"),
            ..drawn(
                UtilityTool::Clearance,
                "DRAWING WHERE PROP=utility.type:water",
            )
        };
        assert_eq!(
            line_of(&drawn_file),
            "UTILITY CLEARANCE DRAWING WHERE PROP=utility.type:water DESIGN \"C:/Design \
             Team/works.csv\" WIDTH 0.375"
        );

        // A drawn line or polyline by its id, typed with the hash or without, at a level - or at
        // its own heights when none is given.
        let mut entity = UtilityForm {
            design_source: UtilityDesignSource::Entity,
            design_entity: String::from("42"),
            design_level: String::from("18.25"),
            horizontal: String::from("0.5"),
            ..form_for(UtilityTool::Clearance)
        };
        assert_eq!(
            line_of(&entity),
            "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN #42 LEVEL 18.25 H 0.5"
        );
        entity.design_entity = String::from(" #42 ");
        entity.design_level = String::new();
        assert_eq!(
            line_of(&entity),
            "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN #42 H 0.5"
        );
        // A level below the datum is a level.
        entity.design_level = String::from("-2.5");
        assert_eq!(
            line_of(&entity),
            "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN #42 LEVEL -2.5 H 0.5"
        );

        // An alignment by its name, quoted when it holds a blank.
        let mut alignment = UtilityForm {
            design_source: UtilityDesignSource::Alignment,
            alignment: String::from("Main Road"),
            margin: String::from("2"),
            ..drawn(UtilityTool::Clearance, "VIEW 1 EXTENTS")
        };
        assert_eq!(
            line_of(&alignment),
            "UTILITY CLEARANCE VIEW 1 EXTENTS DESIGN ALIGNMENT \"Main Road\" MARGIN 2"
        );

        // LEVEL is the drawn line's alone: a file and an alignment carry their own.
        file.design_level = String::from("5");
        alignment.design_level = String::from("5");
        assert_eq!(
            line_of(&file),
            "UTILITY CLEARANCE C:/survey/schedule.csv C:/survey/design.csv"
        );
        assert_eq!(
            line_of(&alignment),
            "UTILITY CLEARANCE VIEW 1 EXTENTS DESIGN ALIGNMENT \"Main Road\" MARGIN 2"
        );
    }

    #[test]
    fn regrade_and_schedule_always_read_what_is_drawn_and_draw_reads_either() {
        // Whatever source is chosen above the tabs.
        let regrade = UtilityForm {
            scope: String::from("LAYERS utilities"),
            spacing: String::from("20"),
            min_cover: String::from("0.6"),
            ..form_for(UtilityTool::Regrade)
        };
        assert_eq!(utility_source_of(&regrade), UtilitySource::Drawing);
        assert_eq!(
            line_of(&regrade),
            "UTILITY REGRADE LAYERS utilities SPACING 20 MINCOVER 0.6"
        );

        let mut schedule = UtilityForm {
            scope: String::from("DRAWING"),
            schedule_out: String::from("C:/Deliverables/as built.csv"),
            ..form_for(UtilityTool::Schedule)
        };
        assert_eq!(utility_source_of(&schedule), UtilitySource::Drawing);
        assert_eq!(
            line_of(&schedule),
            "UTILITY SCHEDULE \"C:/Deliverables/as built.csv\" DRAWING"
        );
        schedule.schedule_schema = String::from("C:/survey/schema.csv");
        assert_eq!(
            line_of(&schedule),
            "UTILITY SCHEDULE \"C:/Deliverables/as built.csv\" DRAWING SCHEMA C:/survey/schema.csv"
        );

        // Draw reads what is chosen: the survey or an import in the drawing, or the schedule
        // file.
        let mut draw = UtilityForm {
            schedule: String::from("C:/survey/schedule.csv"),
            spacing: String::from("8"),
            ..drawn(UtilityTool::Draw, "DRAWING")
        };
        assert_eq!(utility_source_of(&draw), UtilitySource::Drawing);
        assert_eq!(line_of(&draw), "UTILITY DRAW DRAWING SPACING 8");
        draw.source = UtilitySource::File;
        assert_eq!(utility_source_of(&draw), UtilitySource::File);
        assert_eq!(
            line_of(&draw),
            "UTILITY DRAW C:/survey/schedule.csv SPACING 8"
        );
    }

    #[test]
    fn a_draw_of_what_is_drawn_writes_what_the_geometry_cannot_say_in_the_verbs_order() {
        let mut draw = drawn(UtilityTool::Draw, "LAYERS survey WHERE PROP=code:W*");
        // Nothing given: each line and point says its own, or the verb refuses.
        assert_eq!(
            line_of(&draw),
            "UTILITY DRAW LAYERS survey WHERE PROP=code:W*"
        );
        draw.service_type = String::from("water");
        draw.method = String::from("EML");
        draw.horizontal_uncertainty = String::from("0.1");
        draw.vertical_uncertainty = String::from("0.3");
        draw.heights = String::from("surface");
        draw.level_reference = String::from("centre");
        draw.path = String::from("detected");
        draw.owner = String::from("Water Co");
        draw.material = String::from("DICL");
        draw.diameter = String::from("150");
        draw.status = String::from("in service");
        draw.fields = String::from("line=ASSET_ID, point=PT_ID");
        draw.spacing = String::from("8");
        draw.min_cover = String::from("0.6");
        draw.layer_prefix = String::from("located");
        assert_eq!(
            line_of(&draw),
            "UTILITY DRAW LAYERS survey WHERE PROP=code:W* TYPE water METHOD EML H_UNC 0.1 V_UNC \
             0.3 HEIGHTS surface LEVEL_REF centre PATH detected OWNER \"Water Co\" MATERIAL DICL \
             DIAMETER_MM 150 STATUS \"in service\" FIELDS \"line=ASSET_ID, point=PT_ID\" SPACING 8 \
             MINCOVER 0.6 LAYER located"
        );
        // A schedule says all of it in its columns: none of it is written.
        draw.source = UtilitySource::File;
        draw.schedule = String::from("C:/survey/schedule.csv");
        assert_eq!(
            line_of(&draw),
            "UTILITY DRAW C:/survey/schedule.csv SPACING 8 MINCOVER 0.6 LAYER located"
        );

        // What does not read is refused by its field, and nothing is written.
        let mut bad = drawn(UtilityTool::Draw, "DRAWING");
        bad.horizontal_uncertainty = String::from("ten cm");
        assert!(refusal_of(&bad).contains("The horizontal uncertainty must be a number of metres"));
        bad.horizontal_uncertainty.clear();
        bad.diameter = String::from("150mm");
        assert!(
            refusal_of(&bad).contains("The diameter must be a number of millimetres, not '150mm'")
        );
        bad.diameter.clear();
        bad.owner = String::from("The \"Water\" Board");
        assert!(refusal_of(&bad).contains("The owner holds a double quote"));
    }

    #[test]
    fn a_scope_or_works_that_cannot_be_said_writes_no_line() {
        // The scope controls' own refusal, said as theirs.
        let untaken = UtilityForm {
            scope_error: String::from("tick at least one layer to apply to"),
            ..drawn(UtilityTool::Report, "")
        };
        assert!(refusal_of(&untaken).contains("the drawing's scope: tick at least one layer"));
        assert!(refusal_of(&drawn(UtilityTool::Verify, "  ")).contains("what in the drawing"));
        // Regrade refuses it too, whatever the source above the tabs.
        let regrade = UtilityForm {
            scope_error: String::from("that view is no longer open; choose another"),
            ..form_for(UtilityTool::Regrade)
        };
        assert!(refusal_of(&regrade).contains("that view is no longer open"));

        let no_out = UtilityForm {
            scope: String::from("DRAWING"),
            ..form_for(UtilityTool::Schedule)
        };
        assert!(refusal_of(&no_out).contains("schedule to write"));

        let mut entity = UtilityForm {
            design_source: UtilityDesignSource::Entity,
            ..form_for(UtilityTool::Clearance)
        };
        assert!(refusal_of(&entity).contains("Use Selected"));
        for bad in ["#x", "0", "#0", "-3", "12a", "#", "1.5"] {
            entity.design_entity = String::from(bad);
            assert!(refusal_of(&entity).contains("#<entity id>, not '"), "{bad}");
        }
        entity.design_entity = String::from("#7");
        entity.design_level = String::from("high");
        assert!(
            refusal_of(&entity).contains("The design level must be a number of metres, not 'high'")
        );

        let mut alignment = UtilityForm {
            design_source: UtilityDesignSource::Alignment,
            ..form_for(UtilityTool::Clearance)
        };
        assert!(refusal_of(&alignment).contains("alignment"));
        alignment.alignment = String::from("say \"main\"");
        assert!(refusal_of(&alignment).contains("double quote"));
    }

    #[test]
    fn the_source_and_the_scope_controls_give_the_line_its_services() {
        let executor = Executor::new();
        let mut bench = dialog_bench(drawn_context(&executor), drawn_host());
        bench.dialog.show_tool(UtilityTool::Report);
        bench.draw();
        bench.fill("utilitySchedule", "C:/survey/schedule.csv");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY REPORT C:/survey/schedule.csv"
        );
        // The scope is live only when the drawing is the source, the schedule only when the file
        // is.
        assert!(!bench.enabled("utilityScope"));
        assert!(bench.enabled("utilitySchedule"));

        bench.press("utilitySourceDrawing");
        assert!(bench.enabled("utilityScope"));
        assert!(!bench.enabled("utilitySchedule"));
        assert!(!bench.enabled("utilityScheduleBrowse"));
        // What the utility tools act on unless told otherwise: all that is drawn.
        assert_eq!(bench.text("utilityCommand"), "UTILITY REPORT DRAWING");

        // The drawing's layers are listed, to tick.
        let layers = bench.control("utilityLayers");
        assert!(
            layers
                .items
                .iter()
                .any(|layer| layer == "utilities/water/points"),
            "{:?}",
            layers.items
        );
        // The window's plan view, by its id: its layers anywhere, or on screen.
        bench.press("utilityScopeView");
        assert_eq!(bench.control("utilityView").items, ["Plan 2 (VIEW 4)"]);
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY REPORT VIEW 4 EXTENTS"
        );
        bench.tick("utilityOnScreen", true);
        assert_eq!(bench.text("utilityCommand"), "UTILITY REPORT VIEW 4");
        bench.fill("utilityFilterProperty", "utility.type");
        bench.fill("utilityFilterValue", "water");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY REPORT VIEW 4 WHERE PROP=utility.type:water"
        );

        // Draw reads what is chosen: here what is drawn, the survey's or an import's geometry,
        // with the fields for what it cannot say live.
        bench.dialog.show_tool(UtilityTool::Draw);
        bench.draw();
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY DRAW VIEW 4 WHERE PROP=utility.type:water"
        );
        assert!(!bench.enabled("utilitySchedule"));
        assert!(bench.enabled("utilityScope"));
        assert!(bench.enabled("utilitySourceFile"));
        assert!(bench.enabled("utilitySourceDrawing"));
        assert!(bench.enabled("utilityGeometryGroup"));
        // Regrade reads the drawing, and grades: the spacing is live.
        bench.dialog.show_tool(UtilityTool::Regrade);
        bench.draw();
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY REGRADE VIEW 4 WHERE PROP=utility.type:water"
        );
        assert!(bench.enabled("utilityScope"));
        assert!(bench.enabled("utilitySpacing"));
        assert!(!bench.enabled("utilitySchedule"));
        bench.fill("utilitySpacing", "20");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY REGRADE VIEW 4 WHERE PROP=utility.type:water SPACING 20"
        );
        // Schedule names what it writes first.
        bench.dialog.show_tool(UtilityTool::Schedule);
        bench.draw();
        let command = bench.control("utilityCommand");
        assert!(command.value.is_empty());
        assert!(
            command.text.contains("schedule to write"),
            "{}",
            command.text
        );
        bench.fill("utilityScheduleOut", "C:/Deliverables/as built.csv");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY SCHEDULE \"C:/Deliverables/as built.csv\" VIEW 4 WHERE \
             PROP=utility.type:water"
        );

        // Run hands that line to the executor, as for every tab; the status is read from the
        // reply, here SCHEDULE's own record (HELP UTILITY).
        executor.answer(Ok(String::from(
            "utilities scheduled path=\"C:/Deliverables/as built.csv\" lines=1 vertices=6\n\
             scope=view view=4 area=0,0,40,20 where=\"PROP=utility.type:water\" matched=9 lines=1 \
             completed=0 ignored=0\n",
        )));
        let shown = bench.text("utilityCommand");
        bench.press("utilityRun");
        assert_eq!(executor.lines(), [shown]);
        assert!(bench.text("utilityStatus").starts_with("Written."));

        // Back to the file: the schedule is read again.
        bench.dialog.show_tool(UtilityTool::Verify);
        bench.draw();
        bench.press("utilitySourceFile");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY VERIFY C:/survey/schedule.csv"
        );
    }

    #[test]
    fn a_layer_made_while_the_dialog_is_open_is_listed_once_the_event_loop_turns() {
        let executor = Executor::new();
        let mut bench = dialog_bench(drawn_context(&executor), drawn_host());
        bench
            .host
            .document_mut()
            .execute(create_layer(Layer {
                name: String::from("utilities/gas/QL-D"),
                ..Layer::default()
            }))
            .expect("the layer is made");
        bench.draw();
        let layers = bench.control("utilityLayers");
        assert!(
            layers
                .items
                .iter()
                .any(|layer| layer == "utilities/gas/QL-D"),
            "{:?}",
            layers.items
        );
    }

    #[test]
    fn a_scope_that_takes_no_utility_line_is_said_so_and_nothing_is_said_regraded_or_written() {
        let mut host = drawn_host();
        let interpreter = Rc::new(RefCell::new(CommandInterpreter::new()));
        interpreter
            .borrow_mut()
            .run(host.document_mut(), "LINE 0,0 10,0")
            .expect("the line is drawn");
        // The real verb behind the dialog, so the status is held to the reply the verb gives.
        let ran = Rc::new(RefCell::new(Vec::new()));
        let mut context = drawn_context(&Executor::new());
        context.execute = Some(interpreted(&interpreter, &ran));
        let mut bench = dialog_bench(context, host);
        bench.dialog.show_tool(UtilityTool::Regrade);
        bench
            .dialog
            .scope_controls()
            .set_choice(ScopeChoice::Drawing);
        bench.draw();
        let steps = bench.host.document().history().undo_count();
        bench.press("utilityRun");
        assert_eq!(*ran.borrow(), ["UTILITY REGRADE DRAWING"]);
        let output = bench.text("utilityOutput");
        assert!(output.contains("no utility lines in the scope"), "{output}");
        // Not "Regraded as one undo step": no step was pushed for Undo to take.
        assert_eq!(
            bench.text("utilityStatus"),
            "Nothing in the scope is a utility line: nothing was regraded, and nothing was added \
             to the undo history."
        );
        assert_eq!(bench.host.document().history().undo_count(), steps);

        // Not "Written": there is no file.
        let folder = Scratch::new("utility_dialog_a_scope_that_takes_no_utility_line");
        let out = folder.path("out.csv");
        bench.dialog.show_tool(UtilityTool::Schedule);
        bench.draw();
        bench.fill("utilityScheduleOut", &out.to_string_lossy());
        bench.press("utilityRun");
        assert_eq!(ran.borrow().len(), 2);
        assert_eq!(
            bench.text("utilityStatus"),
            "Nothing in the scope is a utility line: nothing was written."
        );
        assert!(!out.exists());
    }

    #[test]
    fn a_draw_of_the_survey_in_the_drawing_runs_the_verb_and_says_when_nothing_was_left_to_draw() {
        // A located main as an import leaves it: a line named by its code.
        let mut host = drawn_host();
        let interpreter = Rc::new(RefCell::new(CommandInterpreter::new()));
        interpreter
            .borrow_mut()
            .run(host.document_mut(), "LINE 334000,6250000 334010,6250000")
            .expect("the main is drawn");
        let main = *host
            .document()
            .model()
            .entities
            .ids()
            .last()
            .expect("an entity");
        host.document_mut()
            .execute(set_entity_property(
                vec![main],
                "code",
                PropertyValue::from("W7"),
            ))
            .expect("the code is set");
        let ran = Rc::new(RefCell::new(Vec::new()));
        let mut context = drawn_context(&Executor::new());
        context.execute = Some(interpreted(&interpreter, &ran));
        let mut bench = dialog_bench(context, host);
        bench.dialog.show_tool(UtilityTool::Draw);
        bench.draw();
        // The file is the source: the geometry's fields are not the line's.
        assert!(!bench.enabled("utilityGeometryGroup"));
        bench.press("utilitySourceDrawing");
        assert!(bench.enabled("utilityGeometryGroup"));
        bench
            .dialog
            .scope_controls()
            .set_choice(ScopeChoice::Drawing);
        bench.draw();
        // The choices are the verb's own words, as --fill takes them.
        assert_eq!(bench.control("utilityMethod").items[0], "not given");
        bench.fill("utilityMethod", "EML");
        bench.fill("utilityServiceType", "water");
        bench.fill("utilityHUnc", "0.2");
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY DRAW DRAWING TYPE water METHOD EML H_UNC 0.2"
        );

        let steps = bench.host.document().history().undo_count();
        bench.press("utilityRun");
        assert_eq!(ran.borrow().len(), 1);
        let output = bench.text("utilityOutput");
        assert!(output.starts_with("utilities drawn lines=1 "), "{output}");
        assert!(output.contains("line id=W7 type=water"), "{output}");
        assert!(
            bench
                .text("utilityStatus")
                .starts_with("Drawn as one undo step")
        );
        assert_eq!(bench.host.document().history().undo_count(), steps + 1);

        // Again: the main is drawn already, and nothing else in the scope can be.
        bench.press("utilityRun");
        assert!(bench.text("utilityOutput").contains("nothing drawn"));
        assert_eq!(
            bench.text("utilityStatus"),
            "Nothing in the scope to draw: what it took is drawn already or cannot be a service. \
             Nothing was added to the undo history."
        );
        assert_eq!(bench.host.document().history().undo_count(), steps + 1);
    }

    #[test]
    fn use_selected_takes_the_one_selected_entity_as_the_works() {
        let mut host = drawn_host();
        let mut interpreter = CommandInterpreter::new();
        for line in [
            "LINE 334010,6250000 334010,6250010",
            "LINE 334020,6250000 334020,6250010",
        ] {
            interpreter
                .run(host.document_mut(), line)
                .expect("the line is drawn");
        }
        let ids = host.document().model().entities.ids();
        assert_eq!(ids.len(), 2);
        let executor = Executor::new();
        let mut bench = dialog_bench(drawn_context(&executor), host);
        bench.dialog.show_tool(UtilityTool::Clearance);
        bench.draw();
        bench.fill("utilitySchedule", "C:/survey/schedule.csv");
        // Its button is live once the works are a drawn line.
        assert!(!bench.enabled("utilityDesignUseSelected"));
        bench.press("utilityDesignEntity");
        assert!(bench.enabled("utilityDesignUseSelected"));

        // None, then two: which one is meant is not the dialog's to guess.
        bench.host.document_mut().selection_mut().set(Vec::new());
        assert!(!bench.dialog.use_selected_design(&bench.host));
        bench.draw();
        assert!(
            bench
                .text("utilityStatus")
                .contains("Select the line or polyline")
        );
        bench
            .host
            .document_mut()
            .selection_mut()
            .set(vec![ids[0], ids[1]]);
        assert!(!bench.dialog.use_selected_design(&bench.host));
        bench.draw();
        assert!(
            bench
                .text("utilityStatus")
                .contains("2 entities are selected")
        );

        bench.host.document_mut().selection_mut().set(vec![ids[1]]);
        bench.press("utilityDesignUseSelected");
        let id = format!("#{}", ids[1]);
        assert_eq!(bench.text("utilityDesignEntityId"), id);
        assert_eq!(
            bench.text("utilityCommand"),
            format!("UTILITY CLEARANCE C:/survey/schedule.csv DESIGN {id}")
        );
        bench.fill("utilityDesignLevel", "18.4");
        assert_eq!(
            bench.text("utilityCommand"),
            format!("UTILITY CLEARANCE C:/survey/schedule.csv DESIGN {id} LEVEL 18.4")
        );
    }

    #[test]
    fn the_drawings_alignments_are_offered_by_name() {
        let mut host = drawn_host();
        let mut interpreter = CommandInterpreter::new();
        interpreter
            .run(
                host.document_mut(),
                "ALIGN NEW MR1 334000,6250000 334040,6250010",
            )
            .expect("the alignment is made");
        let executor = Executor::new();
        let mut bench = dialog_bench(drawn_context(&executor), host);
        bench.dialog.show_tool(UtilityTool::Clearance);
        bench.draw();
        bench.fill("utilitySchedule", "C:/survey/schedule.csv");
        assert!(!bench.enabled("utilityDesignAlignmentName"));
        bench.press("utilityDesignAlignment");
        assert!(bench.enabled("utilityDesignAlignmentName"));
        assert_eq!(bench.control("utilityDesignAlignmentName").items.len(), 1);
        assert_eq!(
            bench.text("utilityCommand"),
            "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN ALIGNMENT MR1"
        );
        // One made while the dialog is open is offered once the event loop turns, in name order,
        // and the one chosen stays chosen.
        interpreter
            .run(
                bench.host.document_mut(),
                "ALIGN NEW Access 334000,6250005 334040,6250005",
            )
            .expect("the alignment is made");
        bench.draw();
        let names = bench.control("utilityDesignAlignmentName");
        assert_eq!(names.items.len(), 2);
        assert_eq!(names.items[0], "Access");
        assert_eq!(names.value, "MR1");
    }
}

// ---- the workbench ------------------------------------------------------------------------------

/// The window's views as the workbench reaches them: one plan view, and each frame asked of them
/// recorded (ViewWorkspace::zoomTo frames a box on its centre).
struct FakeViews {
    views: Rc<RefCell<ViewSet>>,
    zoomed: Rc<RefCell<Vec<Box2>>>,
}

impl ViewAccess for FakeViews {
    fn view_set(&self) -> Rc<RefCell<ViewSet>> {
        Rc::clone(&self.views)
    }
    fn zoom_to(&mut self, area: &Box2) {
        self.zoomed.borrow_mut().push(*area);
    }
    fn zoom_extents_all(&mut self) {}
    fn zoom_to_selection(&mut self, _view: ViewId) {}
    fn active_plan_view(&self) -> Option<ViewId> {
        self.views
            .borrow()
            .most_recent(ViewKind::Plan)
            .map(|state| state.id)
    }
    fn plan_views(&self) -> Vec<ViewId> {
        self.views
            .borrow()
            .views()
            .iter()
            .filter(|state| state.kind == ViewKind::Plan)
            .map(|state| state.id)
            .collect()
    }
    fn refresh_all(&mut self) {}
    fn pick_point(&mut self) -> PickId {
        0
    }
    fn picked_point(&mut self, _pick: PickId) -> Option<Option<Point2>> {
        Some(None)
    }
}

/// The window side of the workbench: a host whose one executor echoes a line and hands it back to
/// the workbench's line runner, as MainWindow::runVerbLine does - never to a running tool.
struct BenchHost {
    inner: RecordingHost,
    lines: Option<UtilityLines>,
    /// Every line the executor was handed, as the command line was typed into.
    typed: Vec<String>,
}

impl Host for BenchHost {
    fn run(&mut self, line: &str) -> VerbOutcome {
        self.typed.push(String::from(line));
        self.inner.log(&format!("> {line}"), false);
        if let Some(lines) = self.lines.clone() {
            lines.run_line(self, line);
        }
        VerbOutcome::new(true, "", "")
    }
    fn document(&self) -> &katana_cad::Document {
        self.inner.document()
    }
    fn document_mut(&mut self) -> &mut katana_cad::Document {
        self.inner.document_mut()
    }
    fn interpreter(&self) -> &CommandInterpreter {
        self.inner.interpreter()
    }
    fn reference(&self) -> &katana_interop::ReferenceData {
        self.inner.reference()
    }
    fn reference_mut(&mut self) -> &mut katana_interop::ReferenceData {
        self.inner.reference_mut()
    }
    fn geo(&mut self, phase: GeoPhase<'_>) {
        self.inner.geo(phase);
    }
    fn headless(&self) -> bool {
        self.inner.headless()
    }
    fn log(&mut self, text: &str, is_error: bool) {
        self.inner.log(text, is_error);
    }
    fn warn_user(&mut self, title: &str, text: &str) {
        self.inner.warn_user(title, text);
    }
    fn views(&mut self) -> Option<&mut dyn ViewAccess> {
        self.inner.views()
    }
    fn jobs(&mut self) -> &mut dyn Jobs {
        self.inner.jobs()
    }
    fn wait_for(&mut self, id: JobId) {
        self.inner.wait_for(id);
    }
    fn wait_for_all(&mut self) {
        self.inner.wait_for_all();
    }
    fn await_job(&mut self, started: &VerbOutcome, done: JobAwait) -> bool {
        self.inner.await_job(started, done)
    }
    fn clipboard(&mut self) -> &mut dyn Clipboard {
        self.inner.clipboard()
    }
    fn settings(&mut self) -> &mut UserSettings {
        self.inner.settings()
    }
}

/// The C++'s Bench: the workbench installed in a stand-in window with one plan view, an
/// interpreter that answers what the test says, and a command line that echoes and hands the line
/// back to the workbench.
struct WindowBench {
    harness: Harness,
    window: WorkbenchHost<UtilityWorkbench>,
    host: BenchHost,
    interpreted: Rc<RefCell<Vec<String>>>,
    reply: Rc<RefCell<Result<String>>>,
    zoomed: Rc<RefCell<Vec<Box2>>>,
    views: Rc<RefCell<ViewSet>>,
}

impl WindowBench {
    fn new() -> Self {
        let views = Rc::new(RefCell::new(ViewSet::new()));
        views.borrow_mut().add(ViewKind::Plan);
        let zoomed = Rc::new(RefCell::new(Vec::new()));
        let mut inner = RecordingHost::new().with_views(Box::new(FakeViews {
            views: Rc::clone(&views),
            zoomed: Rc::clone(&zoomed),
        }));
        inner.parts.headless = true;
        let interpreted = Rc::new(RefCell::new(Vec::new()));
        let reply: Rc<RefCell<Result<String>>> =
            Rc::new(RefCell::new(Ok(String::from(DRAW_REPLY))));
        let interpret: InterpretFn = {
            let interpreted = Rc::clone(&interpreted);
            let reply = Rc::clone(&reply);
            Box::new(move |_host: &mut dyn Host, line: &str| {
                interpreted.borrow_mut().push(String::from(line));
                reply.borrow().clone()
            })
        };
        let workbench = UtilityWorkbench::new(UtilityServices {
            reads_drawing: false,
            interpret: Some(interpret),
        });
        let host = BenchHost {
            inner,
            lines: Some(workbench.lines()),
            typed: Vec::new(),
        };
        let mut harness = Harness::new();
        harness.target("utilityDialog");
        Self {
            harness,
            window: WorkbenchHost::new(workbench),
            host,
            interpreted,
            reply,
            zoomed,
            views,
        }
    }

    fn draw(&mut self) {
        let window = &mut self.window;
        let host = &mut self.host;
        self.harness
            .run(|ui| window.ui(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    fn run_line(&mut self, line: &str) -> bool {
        self.window.run_line(&mut self.host, line)
    }

    fn text(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    /// The last line of the log, and whether it was an error.
    fn last_logged(&self) -> (String, bool) {
        self.host
            .inner
            .logged()
            .last()
            .cloned()
            .expect("something was logged")
    }

    /// The centre of the one box framed.
    fn framed_centre(&self) -> Point2 {
        let zoomed = self.zoomed.borrow();
        assert_eq!(zoomed.len(), 1, "{zoomed:?}");
        zoomed[0].center()
    }
}

fn near(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

mod utility_workbench {
    use super::*;

    #[test]
    fn each_menu_item_opens_the_one_dialog_on_its_tab() {
        let mut bench = WindowBench::new();
        // The section's title, then its seven items, in the order of the tabs.
        let menus = bench.window.menus();
        assert_eq!(menus.len(), 1);
        let items: Vec<String> = menus[0]
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Section(title) => format!("[{title}]"),
                MenuItem::Action(action) => action.name.clone(),
                MenuItem::Separator => String::from("-"),
                MenuItem::Menu(menu) => menu.name.clone(),
            })
            .collect();
        assert_eq!(
            items,
            [
                "[Subsurface Utilities (AS 5488)]",
                "utilityDraw",
                "utilityReport",
                "utilityVerify",
                "utilityClearance",
                "utilityCheck",
                "utilityRegrade",
                "utilityWriteSchedule"
            ]
        );
        let expected = [
            ("utilityClearance", UtilityTool::Clearance),
            ("utilityDraw", UtilityTool::Draw),
            ("utilityCheck", UtilityTool::Check),
            ("utilityReport", UtilityTool::Report),
            ("utilityVerify", UtilityTool::Verify),
            ("utilityRegrade", UtilityTool::Regrade),
            ("utilityWriteSchedule", UtilityTool::Schedule),
        ];
        for (name, tool) in expected {
            let action = bench
                .window
                .action(name)
                .unwrap_or_else(|| panic!("{name}"));
            // The dialog's name, which is how --dialog finds what the item opened.
            assert_eq!(action.dialog.as_deref(), Some("utilityDialog"));
            bench
                .window
                .trigger(&mut bench.host, name)
                .unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
            bench.draw();
            let frame = bench.harness.snapshot();
            let dialogs = frame
                .targets
                .iter()
                .filter(|target| target.name == "utilityDialog")
                .count();
            assert_eq!(dialogs, 1, "{name}");
            assert_eq!(
                frame
                    .control("utilityDialog", "utilityTabs")
                    .map(|tabs| tabs.value.clone()),
                Some(String::from(tool.title())),
                "{name}"
            );
            assert_eq!(bench.window.workbench.action(tool).name, name);
        }
    }

    #[test]
    fn a_draw_frames_what_it_added_in_the_plan_views() {
        let mut bench = WindowBench::new();
        assert!(bench.run_line("utility  draw C:/survey/schedule.csv"));
        assert_eq!(
            *bench.interpreted.borrow(),
            ["utility  draw C:/survey/schedule.csv"]
        );
        let (logged, is_error) = bench.last_logged();
        assert!(!is_error);
        assert!(logged.starts_with("utilities drawn lines=2"), "{logged}");
        // Framed on the middle of bounds=, thousands of kilometres from where a new view looks.
        let centre = bench.framed_centre();
        assert!(near(centre.x, 334020.0, 1e-6), "{}", centre.x);
        assert!(near(centre.y, 6250003.6, 1e-6), "{}", centre.y);
    }

    #[test]
    fn a_regrade_frames_what_it_drew_again() {
        let mut bench = WindowBench::new();
        *bench.reply.borrow_mut() = Ok(String::from(
            "utilities regraded changed=1 lines=2 vertices=5 segments=3 entities=9 layers=5 \
             bounds=334000.000,6250000.000,334040.000,6250007.200\n\
             scope=drawing matched=9 lines=2 completed=0 ignored=0\n",
        ));
        assert!(bench.run_line("UTILITY REGRADE DRAWING SPACING 20"));
        assert_eq!(bench.interpreted.borrow().len(), 1);
        let centre = bench.framed_centre();
        assert!(near(centre.x, 334020.0, 1e-6), "{}", centre.x);
        assert!(near(centre.y, 6250003.6, 1e-6), "{}", centre.y);
    }

    #[test]
    fn the_dialogs_view_scope_offers_the_windows_views_by_their_ids() {
        let mut bench = WindowBench::new();
        bench
            .window
            .workbench
            .open(&mut bench.host, UtilityTool::Report);
        bench.draw();
        let open: Vec<ViewId> = bench
            .views
            .borrow()
            .views()
            .iter()
            .map(|state| state.id)
            .collect();
        assert!(!open.is_empty());
        let view = bench
            .harness
            .control("utilityView")
            .expect("the view choice is drawn");
        assert_eq!(view.items.len(), open.len());
        assert_eq!(view.items[0], format!("Plan 1 (VIEW {})", open[0]));
        let dialog = bench.window.workbench.dialog(&mut bench.host);
        dialog.set_source(UtilitySource::Drawing);
        dialog.scope_controls().set_choice(ScopeChoice::View);
        dialog.scope_controls().set_view_index(0);
        bench.draw();
        assert_eq!(
            bench.text("utilityCommand"),
            format!("UTILITY REPORT VIEW {} EXTENTS", open[0])
        );
    }

    #[test]
    fn only_a_draw_that_worked_is_framed() {
        let mut bench = WindowBench::new();
        // A report is not framed, whatever it says.
        assert!(bench.run_line("UTILITY REPORT C:/survey/schedule.csv"));
        assert!(bench.zoomed.borrow().is_empty());
        // Nor is a draw that was refused - said in the log as an error.
        *bench.reply.borrow_mut() = Err(Error::new(
            ErrorCode::InvalidArgument,
            "line W9 has one vertex; nothing was drawn",
        ));
        assert!(bench.run_line("UTILITY DRAW C:/survey/schedule.csv"));
        assert!(bench.zoomed.borrow().is_empty());
        assert_eq!(
            bench.last_logged(),
            (
                String::from("InvalidArgument: line W9 has one vertex; nothing was drawn"),
                true
            )
        );
    }

    #[test]
    fn only_a_utility_line_is_taken() {
        let mut bench = WindowBench::new();
        assert!(!bench.run_line("LINE 0,0 10,0"));
        assert!(!bench.run_line("UTILITYDRAW x.csv"));
        assert!(bench.interpreted.borrow().is_empty());
        assert!(bench.host.inner.logged().is_empty());
    }

    #[test]
    fn the_dialogs_run_goes_through_the_windows_command_line() {
        let mut bench = WindowBench::new();
        bench
            .window
            .workbench
            .open(&mut bench.host, UtilityTool::Draw);
        bench.draw();
        bench
            .harness
            .fill("utilitySchedule", "C:/survey/schedule.csv");
        bench.draw();
        bench.harness.press("utilityRun");
        bench.draw();
        // Typed on the command line - so echoed, kept and undone as a typed line is - and from
        // there to the interpreter.
        assert_eq!(bench.host.typed, ["UTILITY DRAW C:/survey/schedule.csv"]);
        assert_eq!(bench.interpreted.borrow().len(), 1);
        assert_eq!(format!("{}\n", bench.text("utilityOutput")), DRAW_REPLY);
        assert!(near(bench.framed_centre().x, 334020.0, 1e-6));
    }

    #[test]
    fn a_line_the_command_line_did_not_run_is_an_error() {
        // A command line that swallows the line - a tool taking it for an answer - leaves the
        // dialog nothing to show but that.
        let swallowed = UtilityWorkbench::new(UtilityServices::default());
        let mut host =
            RecordingHost::new().answering(Answer::Fixed(VerbOutcome::new(true, "", "")));
        let reply = swallowed.execute(&mut host, "UTILITY VERIFY C:/survey/schedule.csv");
        assert_eq!(
            reply
                .expect_err("the line never reached the workbench")
                .code,
            ErrorCode::InvalidState
        );
    }

    // How the bounds= record is read is the verb's (draw_reply_bounds); what is the workbench's
    // is that a reply which carries no box moves no view, even from a DRAW that worked.
    #[test]
    fn a_draw_reply_without_a_box_leaves_the_views_where_they_were() {
        let mut bench = WindowBench::new();
        *bench.reply.borrow_mut() = Ok(String::from("utilities drawn lines=1 bounds=5,2,3,4\n"));
        assert!(bench.run_line("UTILITY DRAW C:/survey/schedule.csv"));
        assert!(bench.zoomed.borrow().is_empty());
        assert!(!bench.last_logged().1);
    }
}
