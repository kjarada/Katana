//! The twins of tests/qt_widgets/bim/test_road_dialogs.cpp: the BIM menu's road dialogs, driven by
//! their object names as a person drives them by clicking and typing (RoadWorkbench's five cases
//! are V3's, with the road workbench).
//!
//! None of them changes anything itself: each writes the ROADTEMPLATE or CORRIDOR line a person
//! would type and hands it to the window's executor. So what is tested is each line - exactly the
//! one a person would type - that the drawing then holds what it says, and that what a dialog shows
//! is what the drawing holds. The executor here runs the line through the real CommandInterpreter
//! on the same drawing, which is where the window's command line sends it.

use std::sync::Arc;

use katana_cad::Document;
use katana_cad::bim::bim_object::bim_model;
use katana_cad::road::road_template::{KerbFacing, Side};
use katana_geometry::Point3;
use katana_terrain::{NamedSurface, TinSurface};
use katana_window::bim::corridor_dialog::CorridorDialog;
use katana_window::bim::corridor_quantities_dialog::CorridorQuantitiesDialog;
use katana_window::bim::road_dialog_support::{Table, rows_height};
use katana_window::bim::road_template_manager::{C_WIDTH, K_THICKNESS, RoadTemplateManagerDialog};

use crate::driver::{Driven, ok};
use crate::window_support::{Scratch, interpreter_host};

/// A 100 m straight along +x with a level design at 10, and the standard templates.
fn road(host: &mut katana_window::host::RecordingHost) {
    ok(host, "ALIGN NEW road 0,0 100,0");
    ok(host, "ALIGN DESIGN road 0,10 100,10");
    ok(host, "ROADTEMPLATE DEFAULTS");
}

/// The drawing with surfaces of these names, each a tilted square that covers the 100 m road.
#[track_caller]
fn add_surfaces(document: &mut Document, names: &[&str]) {
    let surface = TinSurface::create(
        vec![
            Point3::new(-10.0, -50.0, 1.0),
            Point3::new(110.0, -50.0, 2.0),
            Point3::new(110.0, 50.0, 3.0),
            Point3::new(-10.0, 50.0, 2.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{}", error.describe()));
    let shared = Arc::new(surface);
    let mut store = document.surfaces().clone();
    for name in names {
        store
            .add(NamedSurface {
                name: String::from(*name),
                surface: Some(Arc::clone(&shared)),
                source: String::from("test"),
                ..NamedSurface::default()
            })
            .unwrap_or_else(|error| panic!("{}", error.describe()));
    }
    document
        .set_surfaces(store, "TEST")
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

fn template_manager() -> Driven<RoadTemplateManagerDialog> {
    let host = interpreter_host(Document::new(None));
    let dialog = RoadTemplateManagerDialog::new(host.document());
    Driven::new(host, dialog, "roadTemplateManagerDialog")
}

fn grid<'a>(dialog: &'a RoadTemplateManagerDialog, name: &str) -> &'a Table {
    dialog.table(name).unwrap_or_else(|| panic!("no {name}"))
}

/// The row of the components grid for side:name; None when it has none.
fn component_row(grid: &Table, side: &str, name: &str) -> Option<usize> {
    (0..grid.row_count()).find(|&row| grid.text(row, 0) == side && grid.text(row, 1) == name)
}

fn lane_width(document: &Document, name: &str, side: Side) -> f64 {
    let templates = document.road_templates();
    let road_template = templates.find(name).expect("the template");
    road_template.components[road_template.find(side, "lane").expect("its lane")].width
}

fn course_thickness(document: &Document, name: &str, course: &str) -> f64 {
    let templates = document.road_templates();
    let road_template = templates.find(name).expect("the template");
    let lane = &road_template.components[road_template
        .find(Side::Left, "lane")
        .expect("its left lane")];
    lane.courses
        .iter()
        .find(|each| each.name == course)
        .map_or(-1.0, |each| each.thickness)
}

/// Chooses the template `name`, as dialog.selectTemplate did, and lets the frame run.
#[track_caller]
fn select_template(driven: &mut Driven<RoadTemplateManagerDialog>, name: &str) {
    let Driven { dialog, host, .. } = &mut *driven;
    assert!(dialog.select_template(host.document(), name), "no {name}");
    driven.settle();
}

/// Chooses the components grid's row of side:name, as components->selectRow did.
#[track_caller]
fn select_component(driven: &mut Driven<RoadTemplateManagerDialog>, side: &str, name: &str) {
    let row = component_row(grid(&driven.dialog, "roadTemplateComponents"), side, name)
        .unwrap_or_else(|| panic!("no {side}:{name}"));
    let Driven { dialog, host, .. } = &mut *driven;
    assert!(dialog.select_row(host.document(), "roadTemplateComponents", row));
    driven.settle();
}

/// Types into a cell of a grid, as item->setText did, and lets the frame run.
#[track_caller]
fn edit(
    driven: &mut Driven<RoadTemplateManagerDialog>,
    table: &str,
    row: usize,
    column: usize,
    text: &str,
) {
    assert!(
        driven.dialog.edit_cell(table, row, column, text),
        "no cell {row},{column} in {table}"
    );
    driven.settle();
}

mod road_template_manager {
    use super::*;

    #[test]
    fn each_button_runs_its_road_template_line_as_one_step() {
        let mut d = template_manager();
        assert_eq!(d.rows("roadTemplateList"), 0);

        d.press("roadTemplateDefaults");
        assert_eq!(d.last(), "ROADTEMPLATE DEFAULTS");
        assert_eq!(d.rows("roadTemplateList"), 3);

        d.fill("roadTemplateName", "my road");
        d.press("roadTemplateNew");
        // A name with a blank is quoted, as a person would have to type it.
        assert_eq!(d.last(), "ROADTEMPLATE NEW \"my road\"");
        assert_eq!(d.dialog.current_template(), "my road");

        d.fill("roadTemplateAddSide", "both");
        d.fill("roadTemplateAddKind", "lane");
        let before = d.document().history().undo_count();
        d.press("roadTemplateAdd");
        assert_eq!(d.last(), "ROADTEMPLATE ADD \"my road\" both lane");
        assert_eq!(d.document().history().undo_count(), before + 1);
        let components = grid(&d.dialog, "roadTemplateComponents");
        assert_eq!(components.row_count(), 2);
        assert_eq!(components.text(0, 0), "left");
        assert_eq!(components.text(0, 1), "lane");

        // A grid is a buffer: an edit runs nothing until Apply.
        let lines_before = d.lines().len();
        edit(&mut d, "roadTemplateComponents", 0, 3, "3.25");
        assert_eq!(d.lines().len(), lines_before);
        d.press("roadTemplateApplyComponents");
        assert_eq!(
            d.last(),
            "ROADTEMPLATE SET \"my road\" left:lane width=3.25 crossfall=-3 layer=road/pavement"
        );
        assert_eq!(lane_width(d.document(), "my road", Side::Left), 3.25);
        assert_eq!(lane_width(d.document(), "my road", Side::Right), 3.5);

        // A cell that is not one number is refused naming it, before any line.
        edit(&mut d, "roadTemplateComponents", 1, 3, "3,5");
        let refused_before = d.lines().len();
        d.press("roadTemplateApplyComponents");
        assert_eq!(d.lines().len(), refused_before);
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("right:lane"), "{status}");
        d.press("roadTemplateRevertComponents");

        // The chosen component's courses: a lane comes with three.
        {
            let Driven { dialog, host, .. } = &mut d;
            assert!(dialog.select_row(host.document(), "roadTemplateComponents", 0));
        }
        d.settle();
        assert_eq!(d.rows("roadTemplateCourses"), 3);
        d.fill("roadTemplateCourseName", "seal");
        d.fill("roadTemplateCourseThickness", "0.01");
        d.fill("roadTemplateCourseMaterial", "spray seal");
        d.press("roadTemplateCourseAdd");
        assert_eq!(
            d.last(),
            "ROADTEMPLATE COURSE \"my road\" left:lane ADD seal thickness=0.01 \"material=spray seal\""
        );
        assert_eq!(d.rows("roadTemplateCourses"), 4);

        // Keys typed for the chosen component.
        d.fill("roadTemplateComponentKeys", "superelevated=off");
        d.press("roadTemplateSetKeys");
        assert_eq!(
            d.last(),
            "ROADTEMPLATE SET \"my road\" left:lane superelevated=off"
        );
        {
            let templates = d.document().road_templates();
            let made = templates.find("my road").expect("my road");
            assert!(
                !made.components[made.find(Side::Left, "lane").expect("left:lane")].superelevated
            );
        }

        d.press("roadTemplateRemove");
        assert_eq!(d.last(), "ROADTEMPLATE REMOVE \"my road\" left:lane");
        assert_eq!(d.rows("roadTemplateComponents"), 1);
    }

    #[test]
    fn a_line_typed_elsewhere_is_shown_and_a_refusal_is_said() {
        let mut d = template_manager();
        d.ok("ROADTEMPLATE NEW typed");
        d.settle(); // the watcher's delivery
        assert_eq!(d.rows("roadTemplateList"), 1);
        // Deleting a template a corridor uses is refused; the dialog says why.
        d.ok("ROADTEMPLATE DEFAULTS");
        d.ok("ALIGN NEW road 0,0 100,0");
        d.ok("ALIGN DESIGN road 0,10 100,10");
        d.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        d.settle();
        select_template(&mut d, "urban street");
        d.press("roadTemplateDelete");
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("corridor Main"), "{status}");
        assert!(d.document().road_templates().find("urban street").is_some());
    }

    // One user edit is one undo step (docs/desktop.md): an Apply over several rows is ONE line,
    // ROADTEMPLATE SET with a group for each row.
    #[test]
    fn an_apply_of_several_rows_is_one_line_and_one_undo_step() {
        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        select_template(&mut d, "rural two-lane");
        let components = grid(&d.dialog, "roadTemplateComponents");
        let left = component_row(components, "left", "lane").expect("left:lane");
        let right = component_row(components, "right", "lane").expect("right:lane");
        let shoulder = component_row(components, "right", "shoulder").expect("right:shoulder");
        // The standard rural two-lane: 3.5 m lanes and 2 m shoulders.
        assert_eq!(lane_width(d.document(), "rural two-lane", Side::Left), 3.5);

        edit(&mut d, "roadTemplateComponents", left, C_WIDTH, "3.25");
        edit(&mut d, "roadTemplateComponents", right, C_WIDTH, "3.75");
        edit(&mut d, "roadTemplateComponents", shoulder, C_WIDTH, "2.5");
        let lines = d.lines().len();
        let steps = d.document().history().undo_count();
        d.press("roadTemplateApplyComponents");
        assert_eq!(
            d.lines().len(),
            lines + 1,
            "one line for the grid, not one a row"
        );
        let line = d.last();
        assert!(
            line.starts_with("ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 "),
            "{line}"
        );
        assert!(line.contains(" right:lane width=3.75 "), "{line}");
        assert!(line.contains(" right:shoulder width=2.5 "), "{line}");
        assert_eq!(d.document().history().undo_count(), steps + 1);
        assert_eq!(lane_width(d.document(), "rural two-lane", Side::Left), 3.25);
        assert_eq!(
            lane_width(d.document(), "rural two-lane", Side::Right),
            3.75
        );

        // One Ctrl+Z takes all three back.
        d.ok("UNDO");
        assert_eq!(lane_width(d.document(), "rural two-lane", Side::Left), 3.5);
        assert_eq!(lane_width(d.document(), "rural two-lane", Side::Right), 3.5);
        assert_eq!(d.document().history().undo_count(), steps);
    }

    #[test]
    fn a_refused_apply_changes_nothing_and_keeps_the_edits_to_correct() {
        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        select_template(&mut d, "rural two-lane");
        let components = grid(&d.dialog, "roadTemplateComponents");
        let left = component_row(components, "left", "lane").expect("left:lane");
        let right = component_row(components, "right", "lane").expect("right:lane");
        let steps = d.document().history().undo_count();

        // A width of 0 is a number, so the dialog lets it through; the template refuses it.
        edit(&mut d, "roadTemplateComponents", left, C_WIDTH, "3.25");
        edit(&mut d, "roadTemplateComponents", right, C_WIDTH, "0");
        d.press("roadTemplateApplyComponents");
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("positive"), "{status}");
        assert_eq!(
            lane_width(d.document(), "rural two-lane", Side::Left),
            3.5,
            "the row before the refused one was applied"
        );
        assert_eq!(lane_width(d.document(), "rural two-lane", Side::Right), 3.5);
        assert_eq!(d.document().history().undo_count(), steps);
        // The person's edits are still in the grid to correct.
        let components = grid(&d.dialog, "roadTemplateComponents");
        assert_eq!(components.text(left, C_WIDTH), "3.25");
        assert_eq!(components.text(right, C_WIDTH), "0");

        edit(&mut d, "roadTemplateComponents", right, C_WIDTH, "3.75");
        d.press("roadTemplateApplyComponents");
        assert_eq!(lane_width(d.document(), "rural two-lane", Side::Left), 3.25);
        assert_eq!(
            lane_width(d.document(), "rural two-lane", Side::Right),
            3.75
        );
        assert_eq!(d.document().history().undo_count(), steps + 1);
    }

    #[test]
    fn an_apply_of_several_courses_is_one_line_and_a_refused_thickness_changes_none() {
        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        select_template(&mut d, "rural two-lane");
        select_component(&mut d, "left", "lane");
        let courses = grid(&d.dialog, "roadTemplateCourses");
        // A lane's pavement: 50 mm of AC14, 200 mm of base, 250 mm of subbase.
        assert_eq!(courses.row_count(), 3);
        assert_eq!(courses.text(0, 0), "AC14");
        assert_eq!(courses.text(1, 0), "base");
        let steps = d.document().history().undo_count();

        edit(&mut d, "roadTemplateCourses", 0, K_THICKNESS, "0.06");
        edit(&mut d, "roadTemplateCourses", 1, K_THICKNESS, "0");
        let lines = d.lines().len();
        d.press("roadTemplateApplyCourses");
        assert_eq!(
            d.lines().len(),
            lines + 1,
            "one line for the grid, not one a row"
        );
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("positive"), "{status}");
        assert_eq!(
            course_thickness(d.document(), "rural two-lane", "AC14"),
            0.05,
            "the course before the refused one was applied"
        );
        assert_eq!(d.document().history().undo_count(), steps);
        let courses = grid(&d.dialog, "roadTemplateCourses");
        assert_eq!(courses.text(0, K_THICKNESS), "0.06");
        assert_eq!(courses.text(1, K_THICKNESS), "0");

        edit(&mut d, "roadTemplateCourses", 1, K_THICKNESS, "0.3");
        d.press("roadTemplateApplyCourses");
        assert_eq!(
            d.last(),
            "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 \
             material=asphalt ifc=PAVEMENT base thickness=0.3 \"material=crushed rock\" \
             ifc=PAVEMENT"
        );
        assert_eq!(
            course_thickness(d.document(), "rural two-lane", "AC14"),
            0.06
        );
        assert_eq!(
            course_thickness(d.document(), "rural two-lane", "base"),
            0.3
        );
        assert_eq!(d.document().history().undo_count(), steps + 1);
    }

    #[test]
    fn pending_course_edits_are_dropped_when_another_template_is_chosen() {
        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        select_template(&mut d, "rural two-lane");
        select_component(&mut d, "left", "lane");
        let courses = grid(&d.dialog, "roadTemplateCourses");
        assert_eq!(courses.text(0, 0), "AC14");
        assert_eq!(courses.text(0, K_THICKNESS), "0.05");
        edit(&mut d, "roadTemplateCourses", 0, K_THICKNESS, "0.1");

        // The urban street has a left:lane with the same courses.
        select_template(&mut d, "urban street");
        let courses = grid(&d.dialog, "roadTemplateCourses");
        assert!(courses.row_count() > 0);
        assert_eq!(
            courses.text(0, K_THICKNESS),
            "0.05",
            "the grid shows rural two-lane's edit under urban street"
        );
        let lines = d.lines().len();
        d.press("roadTemplateApplyCourses");
        assert_eq!(
            d.lines().len(),
            lines,
            "there is nothing edited in urban street"
        );
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("Nothing is edited"), "{status}");
        assert_eq!(course_thickness(d.document(), "urban street", "AC14"), 0.05);
        assert_eq!(
            course_thickness(d.document(), "rural two-lane", "AC14"),
            0.05
        );
    }

    #[test]
    fn course_edits_survive_an_unrelated_line_but_not_a_change_to_the_component() {
        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        select_template(&mut d, "rural two-lane");
        select_component(&mut d, "left", "lane");
        edit(&mut d, "roadTemplateCourses", 0, K_THICKNESS, "0.1");

        // A line that touches no template: the edit stays, and Apply sends it.
        d.ok("LINE 0,0 10,10");
        d.settle();
        assert_eq!(
            grid(&d.dialog, "roadTemplateCourses").text(0, K_THICKNESS),
            "0.1"
        );
        d.press("roadTemplateApplyCourses");
        assert_eq!(
            d.last(),
            "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.1 \
             material=asphalt ifc=PAVEMENT"
        );
        assert_eq!(
            course_thickness(d.document(), "rural two-lane", "AC14"),
            0.1
        );

        // A line that changes the component's courses: the grid shows what the drawing holds, and
        // the edit that was pending is gone.
        edit(&mut d, "roadTemplateCourses", 0, K_THICKNESS, "0.2");
        d.ok("ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET base thickness=0.3");
        d.settle();
        let courses = grid(&d.dialog, "roadTemplateCourses");
        assert_eq!(courses.text(0, K_THICKNESS), "0.1");
        assert_eq!(courses.text(1, K_THICKNESS), "0.3");
        let lines = d.lines().len();
        d.press("roadTemplateApplyCourses");
        assert_eq!(d.lines().len(), lines, "nothing is edited now");
    }

    #[test]
    fn both_grids_keep_room_for_several_rows_at_the_opening_size() {
        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        select_template(&mut d, "urban street");
        // A row of a grid as egui lays one out: the height the grid is given rooms for.
        let row = rows_height(&d.harness.context().global_style(), 1.0);
        for name in ["roadTemplateComponents", "roadTemplateCourses"] {
            // Four rows below the header, as the finding saw one: the viewport is the area the
            // rows are drawn in.
            let height = d.control(name).rect.height();
            assert!(height >= 4.0 * row, "{name} is {height} points tall");
        }
    }

    // ---- templates in a file, and a kerb's facing (docs/road_design.md, "Template files") ----

    #[test]
    fn export_and_import_build_their_lines_from_the_file_field_and_the_replace_box() {
        let folder = Scratch::new("road-template-export");
        // A blank in the path: the line must quote it, as a person would have to.
        let file = folder.path("road templates.json");
        let path = file.to_string_lossy().into_owned();
        let quoted_path = format!("\"{path}\"");

        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        select_template(&mut d, "urban street");

        // No file named: nothing runs, and it says what to do.
        let lines = d.lines().len();
        d.press("roadTemplateExport");
        d.press("roadTemplateImport");
        assert_eq!(d.lines().len(), lines);
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("Type the file"), "{status}");

        // Export writes the chosen template; a second export of the file is refused by the
        // verb, and says OVERWRITE.
        d.fill("roadTemplateFile", &path);
        d.press("roadTemplateExport");
        assert_eq!(
            d.last(),
            format!("ROADTEMPLATE EXPORT \"urban street\" {quoted_path}")
        );
        assert!(file.exists());
        let status = d.text("roadTemplateStatus");
        assert!(status.starts_with("exported file="), "{status}");
        d.press("roadTemplateExport");
        assert_eq!(
            d.last(),
            format!("ROADTEMPLATE EXPORT \"urban street\" {quoted_path}")
        );
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("OVERWRITE"), "{status}");
        // Replace ticked: the line says so, and Export All writes every template.
        d.fill("roadTemplateOverwrite", "on");
        d.press("roadTemplateExportAll");
        assert_eq!(
            d.last(),
            format!("ROADTEMPLATE EXPORT ALL {quoted_path} OVERWRITE")
        );
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("templates=3"), "{status}");

        // Another drawing imports it: one line, one undo step, three templates.
        let mut other = template_manager();
        other.fill("roadTemplateFile", &path);
        other.press("roadTemplateImport");
        assert_eq!(other.last(), format!("ROADTEMPLATE IMPORT {quoted_path}"));
        assert_eq!(other.document().road_templates().templates.len(), 3);
        assert_eq!(other.document().history().undo_count(), 1);
        assert_eq!(other.rows("roadTemplateList"), 3);
        // Again: the drawing has those names; refused, nothing imported, and the box is the way
        // out.
        other.press("roadTemplateImport");
        assert_eq!(other.last(), format!("ROADTEMPLATE IMPORT {quoted_path}"));
        let status = other.text("roadTemplateStatus");
        assert!(status.contains("OVERWRITE"), "{status}");
        assert_eq!(other.document().history().undo_count(), 1);
        other.fill("roadTemplateOverwrite", "on");
        other.press("roadTemplateImport");
        assert_eq!(
            other.last(),
            format!("ROADTEMPLATE IMPORT {quoted_path} OVERWRITE")
        );
        let status = other.text("roadTemplateStatus");
        assert!(status.contains("replaced=3"), "{status}");
    }

    #[test]
    fn a_browse_in_a_headless_run_opens_no_file_dialog_and_a_template_called_all_is_not_exported_alone()
     {
        let mut d = template_manager();
        assert!(
            d.host.parts.headless,
            "the test's host is a headless session's"
        );
        d.fill("roadTemplateFile", "keep.json");
        d.press("roadTemplateBrowse");
        let status = d.text("roadTemplateStatus");
        assert!(
            status.contains("headless session opens no file dialog"),
            "{status}"
        );
        assert_eq!(d.text("roadTemplateFile"), "keep.json");

        // The verb reads ALL as every template, so Export of a template called All would write
        // them all: the dialog says why not, and runs nothing.
        d.fill("roadTemplateName", "All");
        d.press("roadTemplateNew");
        assert_eq!(d.dialog.current_template(), "All");
        let lines = d.lines().len();
        d.press("roadTemplateExport");
        assert_eq!(d.lines().len(), lines);
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("called ALL"), "{status}");
    }

    #[test]
    fn a_kerbs_facing_is_a_grid_column_an_apply_sends_and_nothing_else_has_one() {
        let mut d = template_manager();
        d.press("roadTemplateDefaults");
        d.ok("ROADTEMPLATE NEW strips");
        d.ok("ROADTEMPLATE ADD strips left kerb shape=edgestrip");
        d.settle();

        select_template(&mut d, "divided road");
        let components = grid(&d.dialog, "roadTemplateComponents");
        assert_eq!(components.column_count(), 8);
        assert_eq!(components.headers()[6], "Faces");
        let kerb = component_row(components, "left", "kerb").expect("left:kerb");
        let lane = component_row(components, "left", "lane1").expect("left:lane1");
        // The median's kerb falls going outward; it is edited in place. A lane has no face to
        // turn: an empty cell that cannot be edited.
        let facing = components.cell(kerb, 6).expect("the kerb's facing");
        assert_eq!(facing.text, "outward");
        assert!(facing.editable);
        let none = components.cell(lane, 6).expect("the lane's");
        assert!(none.text.is_empty());
        assert!(!none.editable);

        let steps = d.document().history().undo_count();
        edit(&mut d, "roadTemplateComponents", kerb, 6, "inward");
        d.press("roadTemplateApplyComponents");
        let line = d.last();
        assert!(
            line.starts_with("ROADTEMPLATE SET \"divided road\" left:kerb "),
            "{line}"
        );
        assert!(line.contains(" faces=inward "), "{line}");
        assert_eq!(d.document().history().undo_count(), steps + 1);
        {
            let templates = d.document().road_templates();
            let edited = templates.find("divided road").expect("divided road");
            assert_eq!(
                edited.components[edited.find(Side::Left, "kerb").expect("left:kerb")].faces,
                KerbFacing::Inward
            );
            assert_eq!(
                edited.components[edited.find(Side::Right, "kerb").expect("right:kerb")].faces,
                KerbFacing::Outward,
                "only the row that was edited"
            );
        }

        // A word that is neither is the verb's refusal, said, with nothing changed and the edit
        // kept to correct.
        let again = component_row(grid(&d.dialog, "roadTemplateComponents"), "left", "kerb")
            .expect("left:kerb");
        edit(&mut d, "roadTemplateComponents", again, 6, "sideways");
        d.press("roadTemplateApplyComponents");
        let status = d.text("roadTemplateStatus");
        assert!(status.contains("faces="), "{status}");
        assert_eq!(d.document().history().undo_count(), steps + 1);
        let components = grid(&d.dialog, "roadTemplateComponents");
        let row = component_row(components, "left", "kerb").expect("left:kerb");
        assert_eq!(components.text(row, 6), "sideways");

        // A flush edge strip has no face, so no cell to edit it in.
        select_template(&mut d, "strips");
        let components = grid(&d.dialog, "roadTemplateComponents");
        let strip = component_row(components, "left", "kerb").expect("the strip");
        let cell = components.cell(strip, 6).expect("its facing cell");
        assert!(cell.text.is_empty());
        assert!(!cell.editable);
    }
}

fn open_corridors(host: katana_window::host::RecordingHost) -> Driven<CorridorDialog> {
    let dialog = CorridorDialog::new(host.document());
    Driven::new(host, dialog, "corridorDialog")
}

#[track_caller]
fn select_corridor(driven: &mut Driven<CorridorDialog>, name: &str) {
    let Driven { dialog, host, .. } = &mut *driven;
    assert!(dialog.select_corridor(host.document(), name), "no {name}");
    driven.settle();
}

mod corridor_dialog {
    use super::*;

    #[test]
    fn new_builds_its_line_from_the_fields_and_the_tables_follow_the_drawing() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        let mut d = open_corridors(host);
        d.fill("corridorName", "Main");
        d.fill("corridorAlignment", "road");
        d.fill("corridorTemplate", "urban street");
        d.fill("corridorSurface", "(none)");
        d.fill("corridorInterval", "20");
        d.press("corridorNew");
        assert_eq!(
            d.last(),
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" interval=20"
        );
        assert_eq!(d.document().corridors().corridors.len(), 1);
        assert_eq!(d.rows("corridorList"), 1);
        assert_eq!(d.dialog.current_corridor(), "Main");
        // With no target surface it builds the pavement and says why not more.
        let problems = d.text("corridorProblems");
        assert!(problems.contains("no target surface"), "{problems}");

        d.fill("corridorSuperStation", "30");
        d.fill("corridorSuperLeft", "-3");
        d.fill("corridorSuperRight", "-3");
        d.press("corridorSuperAdd");
        assert_eq!(d.last(), "CORRIDOR SUPERELEVATION Main ADD 30 -3 -3");
        assert_eq!(d.rows("corridorSuperelevation"), 1);

        d.fill("corridorRegionFrom", "40");
        d.fill("corridorRegionTo", "60");
        d.fill("corridorRegionTemplate", "divided road");
        d.press("corridorRegionAdd");
        assert_eq!(d.last(), "CORRIDOR REGION Main ADD 40 60 \"divided road\"");
        assert_eq!(d.rows("corridorRegions"), 1);

        // Apply writes every key, an empty station clearing it.
        d.fill("corridorFrom", "10");
        d.press("corridorApply");
        assert_eq!(
            d.last(),
            "CORRIDOR SET Main alignment=road template=\"urban street\" surface= interval=20 \
             from=10 to="
        );
        assert_eq!(
            d.document().corridors().find("Main").expect("Main").start,
            Some(10.0)
        );

        // A field that is not one number refuses before any line runs.
        d.fill("corridorInterval", "ten");
        let before = d.lines().len();
        d.press("corridorApply");
        assert_eq!(d.lines().len(), before);
        assert!(d.text("corridorStatus").contains("interval"));

        d.press("corridorStrings");
        assert_eq!(d.last(), "CORRIDOR STRINGS Main");
        assert!(d.document().model().entities.size() > 0);
    }

    // The verbs find a template and a surface ignoring case and store the name as typed
    // (docs/road_design.md), so a corridor may hold "URBAN STREET" and GROUND while the drawing
    // has "urban street" and "ground".
    #[test]
    fn apply_keeps_the_template_and_surface_a_corridor_names_in_another_case() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        add_surfaces(host.document_mut(), &["ground"]);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"URBAN STREET\" surface=GROUND",
        );
        {
            let corridors = host.document().corridors();
            let main = corridors.find("Main").expect("Main");
            assert_eq!(main.template_name, "URBAN STREET");
            assert_eq!(main.surface, "GROUND");
            assert_eq!(main.interval, 10.0);
        }

        let mut d = open_corridors(host);
        assert_eq!(d.dialog.current_corridor(), "Main");
        // The combos show the drawing's own item for it, not the first template (rural two-lane)
        // and not (none), which is what a non-editable combo falls back to when setCurrentText
        // finds no exact match.
        assert_eq!(d.text("corridorTemplate"), "urban street");
        assert_eq!(d.text("corridorSurface"), "ground");

        d.fill("corridorInterval", "20");
        d.press("corridorApply");
        // Written as the corridor holds them: the same names, in the same case.
        assert_eq!(
            d.last(),
            "CORRIDOR SET Main alignment=road template=\"URBAN STREET\" surface=GROUND \
             interval=20 from= to="
        );
        {
            let corridors = d.document().corridors();
            let main = corridors.find("Main").expect("Main");
            assert_eq!(main.template_name, "URBAN STREET");
            assert_eq!(main.surface, "GROUND");
            assert_eq!(main.interval, 20.0);
        }
        // The target surface is still found, so the earthworks are still built.
        let problems = d.text("corridorProblems");
        assert!(!problems.contains("no target surface"), "{problems}");
    }

    #[test]
    fn a_chosen_corridor_names_what_the_drawing_lacks_so_the_combo_shows_that_and_not_the_last_one()
    {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        add_surfaces(host.document_mut(), &["ground", "old"]);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" surface=ground",
        );
        ok(
            &mut host,
            "CORRIDOR NEW Second ALIGNMENT road TEMPLATE \"rural two-lane\" surface=old",
        );
        let mut store = host.document().surfaces().clone();
        assert!(store.remove("old"));
        host.document_mut()
            .set_surfaces(store, "TEST")
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        let mut d = open_corridors(host);
        select_corridor(&mut d, "Main");
        assert_eq!(d.text("corridorSurface"), "ground");
        select_corridor(&mut d, "Second");
        // Not Main's ground, which was showing: the surface Second names.
        assert_eq!(d.text("corridorSurface"), "old");
        assert_eq!(d.text("corridorTemplate"), "rural two-lane");

        // Apply writes what the corridor holds, which the drawing refuses to keep as it has no
        // such surface: said, and nothing re-pointed.
        d.press("corridorApply");
        assert_eq!(
            d.last(),
            "CORRIDOR SET Second alignment=road template=\"rural two-lane\" surface=old \
             interval=10 from= to="
        );
        let status = d.text("corridorStatus");
        assert!(status.contains("no surface old"), "{status}");
        assert_eq!(
            d.document()
                .corridors()
                .find("Second")
                .expect("Second")
                .surface,
            "old"
        );
        assert_eq!(
            d.document().corridors().find("Main").expect("Main").surface,
            "ground"
        );

        // The person can say what it should be: (none) clears the target.
        d.fill("corridorSurface", "(none)");
        d.press("corridorApply");
        assert_eq!(
            d.document()
                .corridors()
                .find("Second")
                .expect("Second")
                .surface,
            ""
        );
        assert_eq!(
            d.document().corridors().find("Main").expect("Main").surface,
            "ground"
        );
    }

    #[test]
    fn new_leaves_the_new_corridor_chosen_and_its_buttons_act_on_it() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        let mut d = open_corridors(host);
        // Main is chosen when the dialog opens: the case the first test of this dialog (with no
        // corridor yet) never reached.
        assert_eq!(d.dialog.current_corridor(), "Main");

        d.fill("corridorName", "Second");
        d.fill("corridorTemplate", "rural two-lane");
        d.fill("corridorInterval", "20");
        d.press("corridorNew");
        assert_eq!(
            d.last(),
            "CORRIDOR NEW Second ALIGNMENT road TEMPLATE \"rural two-lane\" interval=20"
        );
        assert!(d.document().corridors().find("Second").is_some());
        assert_eq!(d.dialog.current_corridor(), "Second");
        assert_eq!(d.text("corridorName"), "Second");
        assert_eq!(d.text("corridorInterval"), "20");
        assert_eq!(d.text("corridorTemplate"), "rural two-lane");

        d.press("corridorBuild");
        assert_eq!(d.last(), "CORRIDOR BUILD Second");
        d.press("corridorDelete");
        assert_eq!(d.last(), "CORRIDOR DELETE Second");
        assert!(d.document().corridors().find("Second").is_none());
        assert!(d.document().corridors().find("Main").is_some());
    }

    #[test]
    fn rename_leaves_the_renamed_corridor_chosen() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        ok(
            &mut host,
            "CORRIDOR NEW Other ALIGNMENT road TEMPLATE \"rural two-lane\"",
        );
        let mut d = open_corridors(host);
        select_corridor(&mut d, "Main");
        d.fill("corridorName", "Renamed");
        d.press("corridorRename");
        assert_eq!(d.last(), "CORRIDOR RENAME Main Renamed");
        assert_eq!(d.dialog.current_corridor(), "Renamed");
        assert_eq!(d.text("corridorTemplate"), "urban street");
        d.press("corridorBuild");
        assert_eq!(d.last(), "CORRIDOR BUILD Renamed");
    }

    #[test]
    fn typed_edits_survive_an_unrelated_command_and_a_change_to_the_corridor_is_shown() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        let mut d = open_corridors(host);
        assert_eq!(d.dialog.current_corridor(), "Main");

        d.fill("corridorInterval", "25");
        d.fill("corridorFrom", "5");
        d.fill("corridorTemplate", "divided road");
        // Any command that changes the model reloads the dialog.
        d.ok("LINE 0,0 10,10");
        d.settle();
        assert_eq!(d.text("corridorInterval"), "25");
        assert_eq!(d.text("corridorFrom"), "5");
        assert_eq!(d.text("corridorTemplate"), "divided road");
        d.press("corridorApply");
        assert_eq!(
            d.last(),
            "CORRIDOR SET Main alignment=road template=\"divided road\" surface= interval=25 \
             from=5 to="
        );
        assert_eq!(
            d.document()
                .corridors()
                .find("Main")
                .expect("Main")
                .interval,
            25.0
        );

        // A change to the chosen corridor itself - here a typed line, as an undo or a redo is -
        // shows what the drawing holds, whatever was typed.
        d.fill("corridorInterval", "33");
        d.ok("CORRIDOR SET Main interval=40");
        d.settle();
        assert_eq!(d.text("corridorInterval"), "40");
        d.ok("UNDO");
        d.settle();
        assert_eq!(d.text("corridorInterval"), "25");
        assert_eq!(d.text("corridorFrom"), "5");
    }

    // The curve tolerance (docs/road_design.md, "Building a corridor") is a field of the
    // definition, in the buffer with the others: said in the line only when it is not what the
    // corridor has, so the lines stay the short ones they were.
    #[test]
    fn the_curve_tolerance_is_in_the_line_only_when_it_is_not_what_the_corridor_holds() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        let mut d = open_corridors(host);
        assert_eq!(d.text("corridorCurveTolerance"), "0.001");

        d.fill("corridorName", "Main");
        d.fill("corridorAlignment", "road");
        d.fill("corridorTemplate", "urban street");
        d.fill("corridorSurface", "(none)");
        d.press("corridorNew");
        assert_eq!(
            d.last(),
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" interval=10",
            "the default is not said"
        );
        assert_eq!(
            d.document()
                .corridors()
                .find("Main")
                .expect("Main")
                .curve_tolerance,
            0.001
        );

        // A new one with another tolerance says it, after the interval.
        d.fill("corridorName", "Second");
        d.fill("corridorCurveTolerance", "0.005");
        d.press("corridorNew");
        assert_eq!(
            d.last(),
            "CORRIDOR NEW Second ALIGNMENT road TEMPLATE \"urban street\" interval=10 \
             curvetol=0.005"
        );
        assert_eq!(
            d.document()
                .corridors()
                .find("Second")
                .expect("Second")
                .curve_tolerance,
            0.005
        );
        assert_eq!(d.dialog.current_corridor(), "Second");
        // The field shows what the chosen corridor holds, and so does the list.
        assert_eq!(d.text("corridorCurveTolerance"), "0.005");
        let list = d.dialog.table("corridorList").expect("corridorList");
        assert_eq!(list.column_count(), 10);
        assert_eq!(list.headers()[9], "Curve tolerance");
        assert_eq!(list.text(1, 9), "0.005");
        assert_eq!(list.text(0, 9), "0.001");

        // Apply with the field as it was filled says nothing of it; with it edited, it does - and
        // 0 is how the verb is told to add no sections.
        d.press("corridorApply");
        assert_eq!(
            d.last(),
            "CORRIDOR SET Second alignment=road template=\"urban street\" surface= interval=10 \
             from= to="
        );
        d.fill("corridorCurveTolerance", "0");
        d.press("corridorApply");
        assert_eq!(
            d.last(),
            "CORRIDOR SET Second alignment=road template=\"urban street\" surface= interval=10 \
             from= to= curvetol=0"
        );
        assert_eq!(
            d.document()
                .corridors()
                .find("Second")
                .expect("Second")
                .curve_tolerance,
            0.0
        );

        // Not one number: said on the status line, and no line runs. One the corridor refuses
        // (over a tenth) is the corridor's refusal, shown, and the field keeps what was typed for
        // the person to correct.
        let lines = d.lines().len();
        d.fill("corridorCurveTolerance", "tiny");
        d.press("corridorApply");
        assert_eq!(d.lines().len(), lines);
        let status = d.text("corridorStatus");
        assert!(status.contains("curve tolerance"), "{status}");
        d.fill("corridorCurveTolerance", "0.5");
        d.press("corridorApply");
        assert_eq!(d.lines().len(), lines + 1);
        let status = d.text("corridorStatus");
        assert!(status.contains("curve tolerance"), "{status}");
        assert_eq!(
            d.document()
                .corridors()
                .find("Second")
                .expect("Second")
                .curve_tolerance,
            0.0
        );
        assert_eq!(d.text("corridorCurveTolerance"), "0.5");
    }
}

fn open_quantities(host: katana_window::host::RecordingHost) -> Driven<CorridorQuantitiesDialog> {
    let dialog = CorridorQuantitiesDialog::new(host.document());
    Driven::new(host, dialog, "corridorQuantitiesDialog")
}

mod corridor_quantities {
    use super::*;

    #[test]
    fn lists_every_objects_quantities_and_saves_them_as_csv() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        let mut d = open_quantities(host);
        d.fill("corridorQuantitiesCorridor", "Main");
        d.press("corridorQuantitiesRun");
        assert_eq!(d.last(), "CORRIDOR QUANTITIES Main");
        let model = bim_model(d.document());
        assert_eq!(d.rows("corridorQuantitiesTable"), model.objects.len());
        // No target surface: nothing was measured, which is not a road with 0.000 of cut
        // (CLAUDE.md: absent is not zero).
        let totals = d.text("corridorQuantitiesTotals");
        assert!(totals.starts_with("Cut and fill not measured"), "{totals}");

        // Browse opens no file dialog in a headless session; the path is typed.
        d.press("corridorQuantitiesBrowse");
        assert!(d.text("corridorQuantitiesStatus").contains("headless"));
        let folder = Scratch::new("corridor-quantities-csv");
        let saved_to = folder.path("quantities.csv");
        let file = saved_to.as_path();
        let path = file.to_string_lossy().into_owned();
        d.fill("corridorQuantitiesCsvPath", &path);
        d.press("corridorQuantitiesSaveCsv");
        // The path as the line takes it - quoted only were it to hold a blank.
        let last = d.last();
        assert!(last.starts_with("CORRIDOR QUANTITIES Main "), "{last}");
        assert!(last.contains("csv="), "{last}");
        assert!(file.exists());

        // A file already there is kept unless the box says to replace it: the line is the one a
        // person would type, with OVERWRITE only when asked for.
        std::fs::write(file, "keep me\n").expect("the file is written over");
        d.press("corridorQuantitiesSaveCsv");
        let last = d.last();
        assert!(!last.contains("OVERWRITE"), "{last}");
        let status = d.text("corridorQuantitiesStatus");
        assert!(status.contains("already there"), "{status}");
        assert_eq!(std::fs::read(file).expect("the file"), b"keep me\n");
        d.fill("corridorQuantitiesOverwrite", "on");
        d.press("corridorQuantitiesSaveCsv");
        let last = d.last();
        assert!(last.contains(" OVERWRITE"), "{last}");
        assert!(
            std::fs::read(file)
                .expect("the file")
                .starts_with(b"id,name,kind,")
        );
    }

    // "Shows the DOCUMENT's state after an edit, an undo, a redo, New and Open" (docs/
    // desktop.md): a listing of a road model the drawing no longer has is not left on the screen,
    // nor handed out by Copy.
    #[test]
    fn the_listing_is_cleared_when_the_road_model_changes_under_it() {
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        let mut d = open_quantities(host);
        let listed = |d: &mut Driven<CorridorQuantitiesDialog>| {
            d.fill("corridorQuantitiesCorridor", "Main");
            d.press("corridorQuantitiesRun");
        };

        // Nothing listed yet: a change has nothing to clear and says nothing.
        d.ok("LINE 0,50 10,50");
        d.settle();
        assert!(d.text("corridorQuantitiesStatus").is_empty());

        listed(&mut d);
        assert!(d.rows("corridorQuantitiesTable") > 0);
        let totals = d.text("corridorQuantitiesTotals");
        assert!(!totals.is_empty());
        assert!(totals.contains("asphalt"), "{totals}");

        // A line drawn leaves the road model as it was: the listing stays.
        d.ok("LINE 0,60 10,60");
        d.settle();
        assert!(d.rows("corridorQuantitiesTable") > 0);
        assert!(!d.text("corridorQuantitiesTotals").is_empty());

        // A template edit changes it: the listing is cleared, and the status says to list again.
        d.ok("ROADTEMPLATE COURSE \"urban street\" left:lane SET AC14 thickness=0.5");
        d.settle();
        assert_eq!(d.rows("corridorQuantitiesTable"), 0);
        assert!(d.text("corridorQuantitiesTotals").is_empty());
        let status = d.text("corridorQuantitiesStatus");
        assert!(status.contains("List Quantities again"), "{status}");

        // Listed again it shows the drawing as it is now; an undo is a change too, and Copy -
        // pressed before the watcher has delivered - hands out nothing.
        listed(&mut d);
        assert!(d.rows("corridorQuantitiesTable") > 0);
        d.ok("UNDO");
        d.press("corridorQuantitiesCopy");
        let status = d.text("corridorQuantitiesStatus");
        assert!(status.contains("List Quantities again"), "{status}");
        assert_eq!(d.rows("corridorQuantitiesTable"), 0);

        // A new drawing has no corridor: the listing of the old one goes.
        listed(&mut d);
        assert!(d.rows("corridorQuantitiesTable") > 0);
        d.ok("NEW");
        d.settle();
        assert_eq!(d.rows("corridorQuantitiesTable"), 0);
        assert!(d.text("corridorQuantitiesTotals").is_empty());
        assert!(
            d.text("corridorQuantitiesStatus")
                .contains("List Quantities again")
        );
    }
}
