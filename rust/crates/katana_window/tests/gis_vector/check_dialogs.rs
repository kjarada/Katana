//! The twins of tests/qt_widgets/geo/test_check_dialogs.cpp: GIS > Check - GDAL > Check
//! Geometry..., Repair Geometry... and Gaps and Overlaps... (geo::geometry_check_dialog,
//! geo::coverage_dialog): the line each builds, the problems table a check's reply fills, and a
//! run through the window's executor. The verbs themselves are tested in katana_app's geo twins.
//! Expected values are the C++'s.

use katana_entity::Geometry;
use katana_window::geo::coverage_dialog::{GisCoverageDialog, GisCoverageForm, gis_coverage_line};
use katana_window::geo::geometry_check_dialog::{
    GisCheckDialog, GisCheckForm, GisRepairDialog, GisRepairForm, gis_check_line, gis_repair_line,
};
use katana_window::geo::gis_tool_dialog::GisTool;
use katana_window::host::{Host, VerbOutcome};

use super::support::{Bench, WindowHost, fixed, ok, runner_context, window_context};

fn line_of(made: katana_core::Result<String>) -> String {
    made.unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod gis_check_lines {
    use super::*;

    #[test]
    fn each_is_said_as_its_verb_takes_it() {
        let mut check = GisCheckForm::default();
        check.scope.words = String::from("LAYERS lots");
        assert_eq!(line_of(gis_check_line(&check)), "GIS CHECK LAYERS lots");
        check.markers = String::from("qa/geometry");
        assert_eq!(
            line_of(gis_check_line(&check)),
            "GIS CHECK LAYERS lots markers=qa/geometry"
        );
        let mut repair = GisRepairForm::default();
        repair.scope.words = String::from("SELECTION");
        assert_eq!(line_of(gis_repair_line(&repair)), "GIS REPAIR SELECTION");
        repair.method = String::from("structure");
        assert_eq!(
            line_of(gis_repair_line(&repair)),
            "GIS REPAIR SELECTION method=structure"
        );
    }

    #[test]
    fn a_clean_is_written_only_with_replace() {
        let mut form = GisCoverageForm::default();
        form.scope.words = String::from("DRAWING");
        form.gap = String::from("0.05");
        form.markers = String::from("qa");
        assert_eq!(
            line_of(gis_coverage_line(&form)),
            "GIS COVERAGE CHECK DRAWING gap=0.05 markers=qa"
        );
        form.clean = true;
        form.snap = String::from("0.001");
        let refused = gis_coverage_line(&form).expect_err("no line");
        assert!(refused.message.contains("Replace"));
        form.replace = true;
        form.merge = String::from("max-area");
        assert_eq!(
            line_of(gis_coverage_line(&form)),
            "GIS COVERAGE CLEAN DRAWING gap=0.05 snap=0.001 merge=max-area REPLACE"
        );
        form.gap = String::from("narrow");
        assert!(gis_coverage_line(&form).is_err());
    }
}

mod gis_check_dialog {
    use super::*;

    #[test]
    fn every_field_has_its_object_name_and_problems_fill_the_table() {
        let host = fixed(ok(
            "gis op=check seconds=0.001 cancelled=no\nproblem kind=self-intersection entity=7 \
             at=35,5 reason=Self-intersection\ncheck features=2 problems=1 entities=1",
        ));
        let mut dialog = GisCheckDialog::new(runner_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisCheckDialog", dialog, host);
        for name in [
            "gisCheckScope",
            "gisCheckMarkers",
            "gisCheckProblems",
            "gisCheckCommand",
            "gisCheckPreview",
            "gisCheckRun",
            "gisCheckReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        bench.press("gisCheckScopeDrawing");
        bench.fill("gisCheckMarkers", "qa");
        bench.press("gisCheckRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], "GIS CHECK DRAWING markers=qa");
        let table = bench.dialog.frame().problems().expect("a problems table");
        assert_eq!(table.len(), 1);
        assert_eq!(table[0][0], "self-intersection");
        assert_eq!(table[0][1], "7");
        assert_eq!(table[0][2], "35,5");
        assert!(
            bench
                .dialog
                .status_text()
                .starts_with("1 problems on 1 entities"),
            "{}",
            bench.dialog.status_text()
        );
    }

    // (30,0) (40,10) (40,0) (30,10) crosses itself at (35,5); repaired, it is two triangles of
    // 25 m2.
    #[test]
    fn a_bow_tie_is_found_then_repaired_through_the_executor() {
        let mut host = WindowHost::new("katana-gis-check-dialog-test");
        host.type_line("PLINE 30,0 40,10 40,0 30,10 CLOSE");
        let mut check = GisCheckDialog::new(window_context(), &host);
        check.reload(&host);
        // The C++ clicked the scope's radio button on a dialog it never showed; the scope's own
        // setter is that click.
        check
            .frame_mut()
            .scope_controls_mut()
            .expect("a scope")
            .set_choice(katana_window::customisation::ScopeChoice::Drawing);
        check.run(&mut host);
        {
            let table = check.frame().problems().expect("a problems table");
            assert_eq!(table.len(), 1, "{}", check.reply_text());
            assert_eq!(table[0][2], "35,5");
        }
        let mut repair = GisRepairDialog::new(window_context(), &host);
        repair.reload(&host);
        repair
            .frame_mut()
            .scope_controls_mut()
            .expect("a scope")
            .set_choice(katana_window::customisation::ScopeChoice::Drawing);
        repair.run(&mut host);
        assert!(
            repair.reply_text().contains("split entity=1 parts=2"),
            "{}",
            repair.reply_text()
        );
        let mut area = 0.0;
        host.document().model().entities.for_each(|entity| {
            match &entity.geometry {
                Geometry::Polyline(polyline) => area += polyline.area(),
                _ => panic!("a repaired piece is not a polyline"),
            }
        });
        assert!((area - 50.0_f64).abs() <= 1e-9, "{area}");
        check.run(&mut host);
        assert_eq!(check.frame().problems().map(<[Vec<String>]>::len), Some(0));
    }
}

mod gis_coverage_dialog {
    use super::*;

    #[test]
    fn the_mode_chooses_which_fields_apply() {
        let host = fixed(VerbOutcome::new(true, "", ""));
        let mut dialog = GisCoverageDialog::new(runner_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisCoverageDialog", dialog, host);
        for name in [
            "gisCoverageScope",
            "gisCoverageMode",
            "gisCoverageGap",
            "gisCoverageSnap",
            "gisCoverageMerge",
            "gisCoverageReplace",
            "gisCoverageMarkers",
            "gisCoverageProblems",
            "gisCoverageRun",
        ] {
            assert!(bench.has(name), "{name}");
        }
        assert!(!bench.enabled("gisCoverageSnap"));
        assert!(bench.enabled("gisCoverageMarkers"));
        bench.press("gisCoverageScopeDrawing");
        bench.fill("gisCoverageMode", "clean");
        assert!(bench.enabled("gisCoverageSnap"));
        assert!(!bench.enabled("gisCoverageMarkers"));
        // A clean without Replace ticked is no line at all: Run is off.
        assert!(!bench.enabled("gisCoverageRun"));
        bench.tick("gisCoverageReplace", true);
        assert_eq!(
            bench.text("gisCoverageCommand"),
            "GIS COVERAGE CLEAN DRAWING REPLACE"
        );
    }
}
