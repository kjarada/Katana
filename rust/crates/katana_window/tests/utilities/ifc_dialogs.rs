//! The twins of tests/qt_widgets/test_ifc_dialogs.cpp: File > Import IFC and Export IFC
//! (katana_window::ifc_dialogs) - the choices, what refuses them, and the line each hands the
//! window's command line - driven through the controls by their object names, with the window's
//! side supplied by the test as a command line that records the line and answers as the verb does
//! (katana_ifc::front_end's reply writers). Expected values are the C++'s.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Entity, Geometry, Model, PointGeometry};
use katana_geometry::{Point2, Polyline2};
use katana_ifc::{
    ExportInput, ExportOptions, format_export_reply, parse_export_arguments,
    parse_import_arguments, write_ifc,
};
use katana_window::format::grouped;
use katana_window::host::{Host, RecordingHost};
use katana_window::ifc_dialogs::{
    IfcExportContext, IfcExportDialog, IfcExportState, IfcImportContext, IfcImportDialog,
    IfcRunLine, IfcStateFn,
};

use super::Bench;

fn drawing(entities: usize, selected: usize) -> IfcExportState {
    IfcExportState {
        entities,
        selected,
        alignments: 1,
        surfaces: 2,
        coordinate_system: String::from("EPSG:7856  GDA2020 / MGA zone 56"),
        georeferenced: true,
        road_objects: 0,
    }
}

fn export_context(state: IfcExportState) -> IfcExportContext {
    let state_of: IfcStateFn = Box::new(move |_host: &dyn Host| state.clone());
    IfcExportContext {
        state: Some(state_of),
        run_line: None,
    }
}

/// The dialog over `context`, shown, on the harness; the session headless, as the C++'s context
/// said it was.
fn export_bench(context: IfcExportContext) -> Bench<IfcExportDialog> {
    let host = RecordingHost::new();
    let mut dialog = IfcExportDialog::new(context, &host);
    dialog.show(&host);
    Bench::new("fileExportIfcDialog", dialog, host)
}

fn import_bench(context: IfcImportContext) -> Bench<IfcImportDialog> {
    let mut dialog = IfcImportDialog::new(context);
    dialog.show();
    Bench::new("fileImportIfcDialog", dialog, RecordingHost::new())
}

/// A kerb and a pit: two classes a preview names.
fn kerb_and_pit() -> Model {
    let mut model = Model::new();
    model
        .entities
        .add(Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
                false,
            )),
            layer: String::from("Survey/Kerb"),
            ..Entity::default()
        })
        .expect("the kerb is added");
    model
        .entities
        .add(Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(5.0, 2.0),
            }),
            layer: String::from("Sewer/Pits"),
            ..Entity::default()
        })
        .expect("the pit is added");
    model
}

/// The window's command line, as far as an EXPORT line goes: `model` written in memory, and
/// answered as the verb answers.
fn export_line(model: &Rc<Model>, lines: &Rc<RefCell<Vec<String>>>) -> IfcRunLine {
    let model = Rc::clone(model);
    let lines = Rc::clone(lines);
    Box::new(move |_host: &mut dyn Host, line: &str| -> Result<String> {
        lines.borrow_mut().push(String::from(line));
        let Some(Ok(parsed)) = parse_export_arguments(line.get("EXPORT".len()..).unwrap_or(""))
        else {
            return Err(Error::new(ErrorCode::InvalidArgument, "not EXPORT"));
        };
        let written = write_ifc(
            &ExportInput {
                model: Some(&*model),
                ..ExportInput::default()
            },
            &ExportOptions::default(),
        )?;
        Ok(format_export_reply(&written, &parsed.path, parsed.preview))
    })
}

/// The classes table's rows, each its cells.
fn table_rows(bench: &Bench<IfcExportDialog>) -> Vec<Vec<String>> {
    bench
        .control("ifcExportClasses")
        .items
        .iter()
        .map(|row| row.split(" | ").map(String::from).collect())
        .collect()
}

// ---- export -------------------------------------------------------------------------------------

mod ifc_export_dialog {
    use super::*;

    #[test]
    fn every_control_has_its_object_name() {
        let bench = export_bench(export_context(drawing(5, 0)));
        assert!(!bench.dialog.is_modal());
        let frame = bench.harness.snapshot();
        // What --dialog fileExportIfc finds.
        assert!(frame.target("fileExportIfcDialog").is_some());
        for name in [
            "ifcExportFile",
            "ifcExportBrowse",
            "ifcExportEntities",
            "ifcExportSelectedOnly",
            "ifcExportAlignments",
            "ifcExportSurfaces",
            "ifcExportSchedule",
            "ifcExportScheduleBrowse",
            "ifcExportSchema",
            "ifcExportSchemaBrowse",
            "ifcExportSpacing",
            "ifcExportRules",
            "ifcExportRulesBrowse",
            "ifcExportSaveRules",
            "ifcExportCrs",
            "ifcExportCommand",
            "ifcExportClasses",
            "ifcExportCheck",
            "ifcExportPreview",
            "ifcExportExport",
            "ifcExportClose",
            "ifcExportRoadModel",
        ] {
            assert!(
                frame.control("fileExportIfcDialog", name).is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn it_shows_the_windows_counts_selection_and_coordinate_system() {
        let mut bench = export_bench(export_context(drawing(5, 0)));
        assert_eq!(
            bench.control("ifcExportEntities").text,
            "Drawing entities (5)"
        );
        // Nothing selected: the choice says so, and ticked, refuses the export rather than
        // widening it to every entity.
        assert!(
            bench
                .control("ifcExportSelectedOnly")
                .text
                .contains("nothing is selected")
        );
        bench.tick("ifcExportSelectedOnly", true);
        let check = bench.dialog.check_without_file();
        assert!(
            check.starts_with("Selected entities only is ticked, and nothing is selected"),
            "{check}"
        );
        bench.tick("ifcExportSelectedOnly", false);
        let crs = bench.text("ifcExportCrs");
        assert!(crs.starts_with("Georeferenced in EPSG:7856"), "{crs}");

        let mut local = drawing(5, 2);
        local.coordinate_system.clear();
        local.georeferenced = false;
        let second = export_bench(export_context(local));
        assert!(second.enabled("ifcExportSelectedOnly"));
        assert_eq!(
            second.control("ifcExportSelectedOnly").text,
            "Selected entities only (2 selected)"
        );
        let crs = second.text("ifcExportCrs");
        assert!(
            crs.contains("Not georeferenced: the project has no coordinate system"),
            "{crs}"
        );
    }

    // What the verb refuses, the dialog refuses before the button is pressed.
    #[test]
    fn export_waits_for_a_file_a_positive_spacing_and_the_schedule_a_schema_describes() {
        let mut bench = export_bench(export_context(drawing(5, 0)));
        assert!(!bench.enabled("ifcExportExport"));
        assert!(bench.enabled("ifcExportPreview")); // a preview needs no file
        assert_eq!(bench.text("ifcExportCheck"), "Name the .ifc file to write.");
        bench.fill("ifcExportFile", "site.dxf");
        assert_eq!(bench.dialog.check(), "The file must end in .ifc.");
        bench.fill("ifcExportFile", "site.IFC");
        assert!(bench.enabled("ifcExportExport"));
        let check = bench.text("ifcExportCheck");
        assert!(
            check.starts_with("Ready to write 5 entities, 1 alignments, 2 surfaces"),
            "{check}"
        );

        // The schema's field is not live until a schedule is chosen: the C++ set its text all the
        // same, as code may, and so does this.
        assert!(bench.dialog.set_field_text("ifcExportSchema", "schema.csv"));
        bench.draw();
        assert!(!bench.enabled("ifcExportExport"));
        assert!(!bench.enabled("ifcExportPreview"));
        assert!(bench.dialog.check().contains("describes a schedule"));
        bench.fill("ifcExportSchedule", "schedule.csv");
        assert!(bench.enabled("ifcExportExport"));

        for bad in ["0", "-5", "ten", ""] {
            bench.fill("ifcExportSpacing", bad);
            assert!(!bench.enabled("ifcExportExport"), "{bad}");
            assert_eq!(
                bench.dialog.check(),
                "The detected spacing must be a positive number of metres."
            );
        }
        bench.fill("ifcExportSpacing", "15");
        assert!(bench.enabled("ifcExportExport"));

        // Nothing chosen to write, and no schedule: refused.
        bench.fill("ifcExportSchema", "");
        bench.fill("ifcExportSchedule", "");
        for name in [
            "ifcExportEntities",
            "ifcExportAlignments",
            "ifcExportSurfaces",
        ] {
            bench.tick(name, false);
        }
        assert!(!bench.enabled("ifcExportExport"));
        assert!(bench.dialog.check().starts_with("Nothing to write"));
        bench.fill("ifcExportSchedule", "schedule.csv"); // the investigation alone
        assert!(bench.enabled("ifcExportExport"));
    }

    // The dialog's whole output is a line: every choice is a word of it, shown as it is edited,
    // and the verb reads it back as the choices made.
    #[test]
    fn the_line_carries_every_choice_as_the_verb_reads_it() {
        let mut bench = export_bench(export_context(drawing(5, 3)));
        assert!(bench.control("ifcExportCommand").value.is_empty()); // no file yet: no line to run
        // Read-only: what is written there is drawn over by the dialog's own line.
        bench.fill("ifcExportCommand", "typed by hand");
        assert_eq!(bench.text("ifcExportCommand"), "");
        bench.fill("ifcExportFile", "C:/jobs/site plan.ifc");
        assert_eq!(
            bench.text("ifcExportCommand"),
            "EXPORT \"C:/jobs/site plan.ifc\""
        );
        bench.fill("ifcExportSchedule", "schedule.csv");
        bench.fill("ifcExportSchema", "schema.csv");
        bench.fill("ifcExportRules", "rules.csv");
        bench.fill("ifcExportSpacing", "12.5");
        bench.tick("ifcExportSelectedOnly", true);
        bench.tick("ifcExportSurfaces", false);
        let expected = "EXPORT \"C:/jobs/site plan.ifc\" UTILITIES \"schedule.csv\" SCHEMA \
                        \"schema.csv\" RULES \"rules.csv\" SPACING 12.5 NOSURFACES SELECTED";
        assert_eq!(bench.dialog.line(), expected);
        assert_eq!(bench.text("ifcExportCommand"), expected);

        let line = bench.dialog.line();
        let back = parse_export_arguments(&line["EXPORT".len()..])
            .expect("an EXPORT line")
            .expect("it reads");
        assert_eq!(back, bench.dialog.arguments());

        // The spacing the verb takes by default is not said; selected-only means nothing once the
        // entities are left out; leaving out every part of the drawing - the road model is one -
        // is NODRAWING.
        bench.fill("ifcExportSpacing", "10");
        bench.tick("ifcExportEntities", false);
        bench.tick("ifcExportAlignments", false);
        bench.tick("ifcExportRoadModel", false);
        assert_eq!(
            bench.dialog.line(),
            "EXPORT \"C:/jobs/site plan.ifc\" UTILITIES \"schedule.csv\" SCHEMA \"schema.csv\" \
             RULES \"rules.csv\" NODRAWING"
        );
    }

    // The table is the writer's own account, read from the reply of the line with PREVIEW added:
    // here the real writer on a real drawing, answered as the verb answers.
    #[test]
    fn preview_shows_the_writers_account_class_by_class() {
        let model = Rc::new(kerb_and_pit());
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut context = export_context(drawing(2, 0));
        context.run_line = Some(export_line(&model, &lines));
        let mut bench = export_bench(context);
        bench.press("ifcExportPreview");
        assert_eq!(*lines.borrow(), ["EXPORT \"preview.ifc\" PREVIEW"]); // no file named yet
        let rows = table_rows(&bench);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0][0], "layer Sewer/Pits");
        assert_eq!(rows[0][2], "IfcDistributionChamberElement INSPECTIONPIT");
        assert_eq!(rows[0][3], "SEWAGE");
        assert_eq!(rows[0][4], "rule pit");
        assert_eq!(rows[1][0], "layer Survey/Kerb");
        assert_eq!(rows[1][1], "1");
        assert_eq!(rows[1][2], "IfcKerb NOTDEFINED");
        let check = bench.text("ifcExportCheck");
        assert!(
            check.starts_with("Preview: 2 objects would be written"),
            "{check}"
        );

        // A preview that is refused says why, and shows nothing.
        let mut context = export_context(drawing(2, 0));
        let refuses: IfcRunLine = Box::new(|_host: &mut dyn Host, _line: &str| -> Result<String> {
            Err(Error::new(ErrorCode::NotFound, "cannot read s.csv"))
        });
        context.run_line = Some(refuses);
        let mut refused = export_bench(context);
        refused.press("ifcExportPreview");
        assert!(table_rows(&refused).is_empty());
        assert_eq!(
            refused.text("ifcExportCheck"),
            "NotFound: cannot read s.csv"
        );
    }

    #[test]
    fn export_hands_its_line_to_the_window_and_shows_what_was_written() {
        let model = Rc::new(kerb_and_pit());
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut context = export_context(drawing(2, 0));
        context.run_line = Some(export_line(&model, &lines));
        let mut bench = export_bench(context);
        bench.fill("ifcExportFile", "out.ifc");
        bench.press("ifcExportExport");
        assert_eq!(*lines.borrow(), ["EXPORT \"out.ifc\""]);
        assert_eq!(table_rows(&bench).len(), 2);
        let written = write_ifc(
            &ExportInput {
                model: Some(&*model),
                ..ExportInput::default()
            },
            &ExportOptions::default(),
        )
        .expect("the model is written");
        assert_eq!(
            bench.text("ifcExportCheck"),
            format!(
                "Wrote out.ifc: {} instances (the log has the reply).",
                written.instances
            )
        );
    }

    // Headless, a Browse button would open a box no one can answer: it says so.
    #[test]
    fn browse_asks_no_one_in_a_headless_session() {
        let mut bench = export_bench(export_context(drawing(5, 0)));
        bench.press("ifcExportBrowse");
        assert!(
            bench
                .text("ifcExportCheck")
                .contains("type its path instead")
        );
        bench.press("ifcExportSaveRules");
        assert!(bench.text("ifcExportCheck").contains("type IFC RULES"));
    }

    // ---- what the review found ------------------------------------------------------------------

    // The selection is the drawing's, and changes while the dialog stays open: read again before
    // each export, a cleared one refuses "Selected entities only" instead of writing every entity.
    #[test]
    fn a_selection_cleared_while_it_is_open_refuses_selected_only() {
        let now = Rc::new(RefCell::new(drawing(5, 2)));
        let ran = Rc::new(Cell::new(false));
        let state_of: IfcStateFn = {
            let now = Rc::clone(&now);
            Box::new(move |_host: &dyn Host| now.borrow().clone())
        };
        let run_line: IfcRunLine = {
            let ran = Rc::clone(&ran);
            Box::new(move |_host: &mut dyn Host, _line: &str| -> Result<String> {
                ran.set(true);
                Ok(String::new())
            })
        };
        let mut bench = export_bench(IfcExportContext {
            state: Some(state_of),
            run_line: Some(run_line),
        });
        bench.fill("ifcExportFile", "out.ifc");
        bench.tick("ifcExportSelectedOnly", true);
        assert!(bench.enabled("ifcExportExport"));
        now.borrow_mut().selected = 0; // SELECT NONE, in the drawing beside it
        bench.press("ifcExportExport");
        assert!(!ran.get());
        assert!(
            bench
                .text("ifcExportCheck")
                .starts_with("Selected entities only is ticked, and nothing is selected")
        );
        assert_eq!(bench.control("ifcExportSelectedOnly").checked, Some(true)); // as chosen
    }

    // A table beside "Ready" is what will be written: a changed choice clears it, and a problem
    // replaces the last result instead of standing behind it.
    #[test]
    fn a_changed_choice_clears_the_preview_and_a_problem_replaces_the_last_result() {
        let model = Rc::new(kerb_and_pit());
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut context = export_context(drawing(2, 0));
        context.run_line = Some(export_line(&model, &lines));
        let mut bench = export_bench(context);
        bench.press("ifcExportPreview");
        assert_eq!(table_rows(&bench).len(), 2);
        bench.fill("ifcExportSpacing", "12"); // grades differently: the account is no longer this one
        assert!(table_rows(&bench).is_empty());

        bench.fill("ifcExportFile", "out.ifc");
        bench.press("ifcExportExport");
        let check = bench.text("ifcExportCheck");
        assert!(check.starts_with("Wrote out.ifc"), "{check}");
        bench.fill("ifcExportSpacing", "0");
        assert_eq!(
            bench.text("ifcExportCheck"),
            "The detected spacing must be a positive number of metres."
        );
    }

    // The road model (docs/ifc.md, "Road model"): the box counts its objects, is ticked by
    // default - so a line that leaves nothing out is the line it always was - and unticked, the
    // line says NOROADMODEL. A road model alone is something to write; unticked with nothing
    // else, there is nothing.
    #[test]
    fn the_road_model_box_counts_its_objects_and_unticked_the_line_says_no_road_model() {
        let mut road = drawing(0, 0);
        road.alignments = 0;
        road.surfaces = 0;
        road.road_objects = 1234;
        let mut bench = export_bench(export_context(road));
        let box_ = bench.control("ifcExportRoadModel");
        assert_eq!(box_.checked, Some(true));
        assert_eq!(box_.text, format!("Road model ({} objects)", grouped(1234)));
        bench.fill("ifcExportFile", "road.ifc");
        assert_eq!(bench.dialog.line(), "EXPORT \"road.ifc\"");
        assert!(bench.dialog.check().is_empty(), "{}", bench.dialog.check());
        let check = bench.text("ifcExportCheck");
        assert!(check.contains("road model objects"), "{check}");

        bench.tick("ifcExportRoadModel", false);
        assert!(!bench.dialog.arguments().road_model);
        let check = bench.dialog.check();
        assert!(check.starts_with("Nothing to write"), "{check}");

        // With something else to write, unticked is a word of the line, which the verb reads back
        // as the choices made.
        let mut second = export_bench(export_context(drawing(5, 0)));
        second.fill("ifcExportFile", "site.ifc");
        second.tick("ifcExportRoadModel", false);
        assert_eq!(second.dialog.line(), "EXPORT \"site.ifc\" NOROADMODEL");
        let line = second.dialog.line();
        let back = parse_export_arguments(&line["EXPORT".len()..])
            .expect("an EXPORT line")
            .expect("it reads");
        assert_eq!(back, second.dialog.arguments());
    }

    // Preview needs everything but the file - not merely a missing file.
    #[test]
    fn preview_waits_for_everything_but_the_file() {
        let mut bench = export_bench(export_context(drawing(5, 0)));
        assert!(bench.enabled("ifcExportPreview")); // no file yet
        bench.fill("ifcExportSpacing", "-5");
        assert!(!bench.enabled("ifcExportPreview"));
        assert_eq!(
            bench.dialog.check_without_file(),
            "The detected spacing must be a positive number of metres."
        );
        bench.fill("ifcExportSpacing", "5");
        assert!(bench.enabled("ifcExportPreview"));
    }
}

// ---- import -------------------------------------------------------------------------------------

mod ifc_import_dialog {
    use super::*;

    #[test]
    fn every_control_has_its_object_name() {
        let bench = import_bench(IfcImportContext::default());
        assert!(!bench.dialog.is_modal());
        let frame = bench.harness.snapshot();
        assert!(frame.target("fileImportIfcDialog").is_some());
        for name in [
            "ifcImportFile",
            "ifcImportBrowse",
            "ifcImportDescribe",
            "ifcImportSummary",
            "ifcImportLocal",
            "ifcImportAlignments",
            "ifcImportElements",
            "ifcImportSurfaces",
            "ifcImportTolerance",
            "ifcImportTakeCrs",
            "ifcImportCommand",
            "ifcImportCheck",
            "ifcImportImport",
            "ifcImportClose",
        ] {
            assert!(
                frame.control("fileImportIfcDialog", name).is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn the_line_carries_every_choice_and_local_takes_no_coordinate_system() {
        let mut bench = import_bench(IfcImportContext::default());
        assert!(!bench.enabled("ifcImportImport"));
        assert_eq!(bench.dialog.check(), "Name the IFC file to import.");
        bench.dialog.set_file("site.ifc");
        bench.draw();
        assert!(bench.enabled("ifcImportImport"));
        // The dialog always answers the coordinate-system question, so its line never asks:
        // ticked by default, TAKECRS.
        assert_eq!(
            bench.text("ifcImportCommand"),
            "IMPORT \"site.ifc\" TAKECRS"
        );
        bench.tick("ifcImportTakeCrs", false);
        assert_eq!(bench.dialog.line(), "IMPORT \"site.ifc\" KEEPCRS");

        bench.tick("ifcImportLocal", true);
        bench.tick("ifcImportSurfaces", false);
        bench.fill("ifcImportTolerance", "0.01");
        // Moved to the origin, the data is in no system the file names.
        assert!(!bench.enabled("ifcImportTakeCrs"));
        assert_eq!(
            bench.dialog.line(),
            "IMPORT \"site.ifc\" LOCAL NOSURFACES TOLERANCE 0.01"
        );
        let line = bench.dialog.line();
        let back = parse_import_arguments(&line["IMPORT".len()..])
            .expect("an IMPORT line")
            .expect("it reads");
        assert_eq!(back, bench.dialog.arguments());

        bench.fill("ifcImportTolerance", "0");
        assert!(!bench.enabled("ifcImportImport"));
        assert!(bench.text("ifcImportCommand").is_empty());
        bench.fill("ifcImportTolerance", "0.01");
        for name in ["ifcImportAlignments", "ifcImportElements"] {
            bench.tick(name, false);
        }
        assert!(!bench.enabled("ifcImportImport"));
        assert!(bench.dialog.check().starts_with("Nothing to import"));
    }

    #[test]
    fn describe_runs_info_and_import_runs_its_line() {
        let lines = Rc::new(RefCell::new(Vec::<String>::new()));
        let cancel = Rc::new(Cell::new(false));
        let run_line: IfcRunLine = {
            let lines = Rc::clone(&lines);
            let cancel = Rc::clone(&cancel);
            Box::new(move |_host: &mut dyn Host, line: &str| -> Result<String> {
                lines.borrow_mut().push(String::from(line));
                if line == "INFO \"missing.ifc\"" {
                    return Err(Error::new(ErrorCode::NotFound, "the file cannot be read"));
                }
                if line.starts_with("INFO") {
                    return Ok(String::from(
                        "ifc described file=\"site.ifc\" schema=IFC4X3_ADD2",
                    ));
                }
                if cancel.get() {
                    return Err(Error::new(ErrorCode::CommandRejected, "Import cancelled."));
                }
                Ok(String::from(
                    "ifc imported file=\"site.ifc\" schema=IFC4X3_ADD2 crs=\"\" entities=29 \
                     alignments=1 surfaces=0",
                ))
            })
        };
        let mut bench = import_bench(IfcImportContext {
            run_line: Some(run_line),
        });
        bench.dialog.set_file("site.ifc");
        bench.draw();
        bench.press("ifcImportDescribe");
        assert_eq!(*lines.borrow(), ["INFO \"site.ifc\""]);
        assert_eq!(
            bench.text("ifcImportSummary"),
            "ifc described file=\"site.ifc\" schema=IFC4X3_ADD2"
        );
        bench.press("ifcImportImport");
        assert_eq!(lines.borrow().len(), 2);
        assert_eq!(lines.borrow()[1], "IMPORT \"site.ifc\" TAKECRS");
        assert_eq!(
            bench.text("ifcImportCheck"),
            "Imported 29 entities, 1 alignments and 0 surfaces from site.ifc: the log has the \
             reply."
        );

        // A person who cancels the far-apart question has not had a failure.
        cancel.set(true);
        bench.press("ifcImportImport");
        assert_eq!(bench.text("ifcImportCheck"), "Import cancelled.");

        bench.dialog.set_file("missing.ifc");
        bench.draw();
        bench.press("ifcImportDescribe");
        assert!(bench.text("ifcImportSummary").is_empty());
        assert_eq!(
            bench.text("ifcImportCheck"),
            "NotFound: the file cannot be read"
        );
        // Read at last, the file's earlier failure no longer stands.
        bench.dialog.set_file("site.ifc");
        bench.draw();
        bench.press("ifcImportDescribe");
        assert!(bench.text("ifcImportCheck").starts_with("Ready to import"));
    }
}
