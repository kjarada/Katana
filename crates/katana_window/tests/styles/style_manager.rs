//! The twins of tests/qt_widgets/customisation/test_style_manager.cpp: the styles and linetypes
//! manager, driven through its controls by object name and judged by the Document - QT-02 (an
//! unedited Save changes nothing and pushes nothing), bulk edits that leave <varies> fields alone,
//! one undo step per merge and purge, reloads that keep the selection, and the linetype grid's
//! validation.

use katana_cad::Document;
use katana_cad::style_manager_rows::LinetypeOrigin;
use katana_commands::{
    CommandPtr, EntityAttributes, create_linetype, create_point, create_style, set_entity_style,
};
use katana_entity::style_library::add_or_replace;
use katana_entity::{
    EntityId, LineStyle, Linetype, LinetypeElement, Stroke, StrokeOp, Style, StyleLibrary,
    StyleUnits,
};
use katana_geometry::Point2;
use katana_window::customisation::CustomisationContext;
use katana_window::host::RecordingHost;
use katana_window::style_manager::{StyleManagerDialog, Tab};

use crate::support::{Bench, create, must, steps};

/// Two library definitions written for these tests: a paper linestyle (a linestyle: not `mode
/// vertex`, D2) and a vertex symbol. Only their names and kinds matter here.
fn test_library() -> StyleLibrary {
    let mut library = StyleLibrary::new();
    let kerb = LineStyle {
        name: String::from("TEST Dashed Kerb"),
        units: StyleUnits::Paper,
        length: 4.0,
        strokes: vec![
            Stroke {
                op: StrokeOp::Move,
                point: Point2::new(0.0, 0.0),
                ..Stroke::default()
            },
            Stroke {
                op: StrokeOp::Draw,
                point: Point2::new(1.5, 0.0),
                ..Stroke::default()
            },
        ],
        ..LineStyle::default()
    };
    library.add(kerb).expect("the kerb is added");
    let manhole = LineStyle {
        name: String::from("TEST Manhole"),
        at_vertices: true,
        strokes: vec![Stroke {
            op: StrokeOp::Circle,
            radius: 0.5,
            ..Stroke::default()
        }],
        ..LineStyle::default()
    };
    library.add(manhole).expect("the manhole is added");
    library
}

fn style(name: &str, linetype: &str) -> Style {
    Style {
        name: String::from(name),
        linetype: String::from(linetype),
        line_weight: 0.35,
        ..Style::default()
    }
}

fn linetype(name: &str, dash: f64, gap: f64) -> Linetype {
    Linetype {
        name: String::from(name),
        pattern: vec![
            LinetypeElement { length: dash },
            LinetypeElement { length: gap },
        ],
        ..Linetype::default()
    }
}

fn wearing(style: &str) -> EntityAttributes {
    EntityAttributes {
        layer: String::from("0"),
        style: String::from(style),
        color: None,
    }
}

/// The drawing:
///
/// ```text
/// linetype  DASHED {1, -0.5}
/// styles    "Old Survey"  linetype "Old Survey Kerb", symbol "Old Survey Pit", hatch "Old Survey
///                         Hatch" - three names NOTHING defines - weight 0.1234 and symbol size
///                         0.03125, which no number box here shows exactly; worn by nothing
///           Kerb          TEST Dashed Kerb (the library's), 0.35; one point
///           Fence         DASHED, 0.35; two points
/// ```
struct Fixture {
    host: RecordingHost,
    fence_points: Vec<EntityId>,
    kerb_point: EntityId,
}

impl Fixture {
    fn new() -> Self {
        let mut host = RecordingHost::new();
        host.document_mut().set_style_library(test_library());
        must(&mut host, create_linetype(linetype("DASHED", 1.0, -0.5)));

        let old = Style {
            name: String::from("Old Survey"),
            linetype: String::from("Old Survey Kerb"),
            symbol: String::from("Old Survey Pit"),
            hatch_pattern: String::from("Old Survey Hatch"),
            line_weight: 0.1234,
            symbol_size: 0.03125,
            description: String::from("from the survey"),
            ..Style::default()
        };
        must(&mut host, create_style(old));
        must(&mut host, create_style(style("Kerb", "TEST Dashed Kerb")));
        must(&mut host, create_style(style("Fence", "DASHED")));

        let kerb_point = create(
            &mut host,
            create_point(Point2::new(0.0, 0.0), wearing("Kerb")),
        );
        let fence_points = [1.0, 2.0]
            .into_iter()
            .map(|x| {
                create(
                    &mut host,
                    create_point(Point2::new(x, 0.0), wearing("Fence")),
                )
            })
            .collect();
        Self {
            host,
            fence_points,
            kerb_point,
        }
    }

    fn manager(self) -> Bench<StyleManagerDialog> {
        Bench::new(self.host, |document| {
            StyleManagerDialog::new(&CustomisationContext::default(), document)
        })
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| String::from(*name)).collect()
}

fn stored(bench: &Bench<StyleManagerDialog>, name: &str) -> Style {
    bench
        .document()
        .model()
        .styles
        .find(name)
        .unwrap_or_else(|| panic!("{name} is there"))
        .clone()
}

fn has_style(bench: &Bench<StyleManagerDialog>, name: &str) -> bool {
    bench.document().model().styles.contains(name)
}

fn style_of(bench: &Bench<StyleManagerDialog>, id: EntityId) -> String {
    let entity = bench.document().model().entities.find(id);
    entity.expect("the entity is there").style.clone()
}

fn pattern_of(bench: &Bench<StyleManagerDialog>, name: &str) -> Vec<f64> {
    let linetype = bench.document().model().linetypes.find(name);
    linetype
        .expect("the linetype is there")
        .pattern
        .iter()
        .map(|element| element.length)
        .collect()
}

fn execute(bench: &mut Bench<StyleManagerDialog>, command: CommandPtr) {
    must(&mut bench.host, command);
}

fn select_styles(bench: &mut Bench<StyleManagerDialog>, list: &[&str]) {
    bench
        .dialog
        .select_styles(bench.host.document(), &names(list));
    bench.draw();
}

fn select_linetype(bench: &mut Bench<StyleManagerDialog>, name: &str, origin: LinetypeOrigin) {
    bench
        .dialog
        .select_linetype(bench.host.document(), name, origin);
    bench.draw();
}

mod manager {
    use super::*;

    #[test]
    fn saving_a_style_naming_undefined_names_unedited_changes_nothing_and_pushes_no_undo_step() {
        // Audit QT-02: the old form showed a name it could not list as the previous row's, and
        // Save wrote that back - every imported style whose library was not loaded lost its
        // linetype and symbol.
        let mut bench = Fixture::new().manager();
        let before = stored(&bench, "Old Survey");
        let steps_before = bench.steps();

        select_styles(&mut bench, &["Old Survey"]);
        assert_eq!(bench.dialog.selected_styles(), ["Old Survey"]);
        // The form shows the stored names, kept and marked, not a neighbour's.
        assert_eq!(
            bench.dialog.linetype_picker().current_name(),
            "Old Survey Kerb"
        );
        assert!(!bench.dialog.linetype_picker().current_is_defined());
        assert_eq!(
            bench.dialog.symbol_picker().current_name(),
            "Old Survey Pit"
        );
        assert!(!bench.dialog.symbol_picker().current_is_defined());
        assert!(bench.value("styleHatch").starts_with("Old Survey Hatch"));

        // A person touches the description, retyping what was there, and saves.
        bench.fill("styleDescription", "from the survey");
        bench.press("styleSave");

        // Field by field first, so a failure says which one moved.
        let after = stored(&bench, "Old Survey");
        assert_eq!(after.linetype, "Old Survey Kerb");
        assert_eq!(after.symbol, "Old Survey Pit");
        assert_eq!(after.hatch_pattern, "Old Survey Hatch");
        assert_eq!(
            after.line_weight, 0.1234,
            "not what a number box rounds it to"
        );
        assert_eq!(after.symbol_size, 0.03125);
        assert_eq!(after, before, "every field");
        assert_eq!(bench.steps(), steps_before, "an unedited Save is no command");

        // And Save pressed again on the reloaded, untouched form: still nothing.
        bench.press("styleSave");
        assert_eq!(stored(&bench, "Old Survey"), before);
        assert_eq!(bench.steps(), steps_before);
        assert_eq!(
            bench.dialog.linetype_picker().current_name(),
            "Old Survey Kerb"
        );
    }

    #[test]
    fn changing_only_the_description_of_a_style_naming_undefined_names_keeps_every_other_field() {
        // The audit's own scenario for QT-02: change only the description.
        let mut bench = Fixture::new().manager();
        let before = stored(&bench, "Old Survey");
        select_styles(&mut bench, &["Old Survey"]);
        bench.fill("styleDescription", "kerb and channel");
        let steps_before = bench.steps();
        bench.press("styleSave");

        let expected = Style {
            description: String::from("kerb and channel"),
            ..before
        };
        assert_eq!(stored(&bench, "Old Survey").linetype, "Old Survey Kerb");
        assert_eq!(stored(&bench, "Old Survey").symbol, "Old Survey Pit");
        assert_eq!(stored(&bench, "Old Survey"), expected);
        assert_eq!(bench.steps(), steps_before + 1);
    }

    #[test]
    fn a_bulk_edit_writes_the_edited_field_and_leaves_the_varies_fields_of_each_style() {
        let mut bench = Fixture::new().manager();
        select_styles(&mut bench, &["Kerb", "Fence"]);
        assert_eq!(bench.dialog.selected_styles(), ["Fence", "Kerb"]);

        // Their linetypes differ: the picker shows nothing, and says why.
        assert_eq!(bench.dialog.linetype_picker().current_name(), "");
        assert_eq!(bench.dialog.linetype_picker().placeholder_text(), "<varies>");
        // Their weights agree: 0.35 is shown.
        assert_eq!(bench.number("styleWeight"), 0.35);

        let steps_before = bench.steps();
        bench.fill("styleWeight", "0.5");
        bench.press("styleSave");

        assert_eq!(bench.steps(), steps_before + 1, "two styles, one undo step");
        assert_eq!(stored(&bench, "Kerb").line_weight, 0.5);
        assert_eq!(stored(&bench, "Fence").line_weight, 0.5);
        assert_eq!(
            stored(&bench, "Kerb").linetype,
            "TEST Dashed Kerb",
            "<varies> was left alone"
        );
        assert_eq!(stored(&bench, "Fence").linetype, "DASHED");
        // Still both selected after the reload, and still differing.
        assert_eq!(bench.dialog.selected_styles(), ["Fence", "Kerb"]);
        assert_eq!(bench.dialog.linetype_picker().placeholder_text(), "<varies>");
    }

    #[test]
    fn merging_a_style_through_the_dialog_is_one_undo_step_that_undo_restores() {
        let fixture = Fixture::new();
        let fence_points = fixture.fence_points.clone();
        let mut bench = fixture.manager();
        select_styles(&mut bench, &["Fence"]);
        let steps_before = bench.steps();

        bench.press("styleMerge");
        assert!(
            !bench.hidden("promptPanel"),
            "the target is asked for in the dialog, not a box"
        );
        // Fence itself is not offered: the targets are Kerb and Old Survey.
        assert_eq!(bench.rows("promptChoice"), 2);
        bench.fill("promptChoice", "Kerb");
        bench.press("promptOk");

        assert_eq!(bench.steps(), steps_before + 1);
        assert!(!has_style(&bench, "Fence"));
        for &id in &fence_points {
            assert_eq!(style_of(&bench, id), "Kerb");
        }
        assert_eq!(bench.dialog.selected_styles(), ["Kerb"], "the target");

        bench.press("undoButton");
        assert!(has_style(&bench, "Fence"));
        for &id in &fence_points {
            assert_eq!(style_of(&bench, id), "Fence");
        }
        assert_eq!(bench.steps(), steps_before);
    }

    #[test]
    fn a_reload_after_an_external_command_and_its_undo_keeps_the_selected_row() {
        let mut bench = Fixture::new().manager();
        select_styles(&mut bench, &["Kerb"]);
        let rows_before = bench.rows("styleTable");

        // A style made elsewhere - the command line, say - sorts before Kerb, so Kerb's row index
        // moves; the selection follows the NAME.
        execute(&mut bench, create_style(style("Aaa", "DASHED")));
        assert_eq!(
            bench.rows("styleTable"),
            rows_before,
            "not rebuilt inside the notification"
        );
        bench.draw();
        assert_eq!(bench.rows("styleTable"), rows_before + 1);
        assert_eq!(bench.dialog.selected_styles(), ["Kerb"]);

        bench.host.document_mut().undo().expect("the undo is done");
        bench.draw();
        assert_eq!(bench.rows("styleTable"), rows_before);
        assert_eq!(bench.dialog.selected_styles(), ["Kerb"]);
        assert_eq!(
            bench.dialog.linetype_picker().current_name(),
            "TEST Dashed Kerb"
        );
    }

    #[test]
    fn deleting_a_style_still_worn_is_refused_with_the_count_and_the_first_holder() {
        let fixture = Fixture::new();
        let fence_points = fixture.fence_points.clone();
        let mut bench = fixture.manager();
        select_styles(&mut bench, &["Fence"]);
        let steps_before = bench.steps();
        bench.press("styleDelete");

        assert!(has_style(&bench, "Fence"));
        assert_eq!(bench.steps(), steps_before);
        // Worn by the two points; the first holder is the lower id.
        let expected = format!(
            "used by 2 entities, e.g. id={}",
            fence_points[0].min(fence_points[1])
        );
        let status = bench.text("statusLine");
        assert!(status.contains(&expected), "{status}");
        let messages = bench.host.logged();
        let (last, _) = messages.last().expect("the dialog logged");
        assert_eq!(*last, status, "the same sentence reaches the log");
    }

    #[test]
    fn purge_lists_what_nothing_uses_and_deletes_the_checked_items_as_one_step() {
        let mut bench = Fixture::new().manager();
        let steps_before = bench.steps();
        bench.press("stylePurge");
        assert!(!bench.hidden("purgePanel"));
        // Old Survey is worn by nothing; Kerb and Fence are worn, and DASHED is named by Fence.
        let items = bench.items("purgeList");
        assert!(items.iter().any(|item| item == "Style  Old Survey"));
        assert!(!items.iter().any(|item| item == "Style  Kerb"));
        assert!(!items.iter().any(|item| item == "Linetype  DASHED"));

        bench.press("purgeOk");
        assert!(!has_style(&bench, "Old Survey"));
        assert_eq!(bench.steps(), steps_before + 1);
        bench.host.document_mut().undo().expect("the undo is done");
        assert_eq!(stored(&bench, "Old Survey").linetype, "Old Survey Kerb");
    }

    #[test]
    fn apply_to_selection_uses_the_drawings_selection_as_it_is_when_pressed() {
        let fixture = Fixture::new();
        let fence_points = fixture.fence_points.clone();
        let mut bench = fixture.manager();
        select_styles(&mut bench, &["Kerb"]);
        assert!(
            !bench.enabled("styleApplyToSelection"),
            "nothing is selected in the drawing yet"
        );

        // Selected in the drawing AFTER the dialog opened: a non-modal manager sits beside the
        // drawing while the person picks.
        bench
            .host
            .document_mut()
            .selection_mut()
            .set(vec![fence_points[0]]);
        bench.host.document().notify_selection_changed();
        bench.draw();
        assert!(bench.enabled("styleApplyToSelection"));
        assert_eq!(
            bench.text("styleApplyToSelection"),
            "Apply to Selection (1)"
        );
        bench.press("styleApplyToSelection");
        assert_eq!(style_of(&bench, fence_points[0]), "Kerb");
        assert_eq!(style_of(&bench, fence_points[1]), "Fence");
    }

    #[test]
    fn select_users_selects_the_entities_wearing_the_selected_styles() {
        let fixture = Fixture::new();
        let mut expected = fixture.fence_points.clone();
        expected.push(fixture.kerb_point);
        expected.sort_unstable();
        let mut bench = fixture.manager();
        select_styles(&mut bench, &["Kerb", "Fence"]);
        bench.press("styleSelectUsers");
        assert_eq!(bench.document().selection().ids(), expected);
    }

    #[test]
    fn make_current_feeds_new_work_and_marks_the_row_and_pressed_again_draws_by_layer() {
        let mut bench = Fixture::new().manager();
        select_styles(&mut bench, &["Kerb"]);
        bench.press("styleMakeCurrent");
        bench.draw();
        assert_eq!(bench.document().current_style(), "Kerb");
        // The selected row is Kerb's: the second, after Fence, in name order.
        assert_eq!(bench.dialog.selected_styles(), ["Kerb"]);
        assert_eq!(bench.row("styleTable", 1)[0], "Kerb  (current)");
        assert_eq!(bench.text("styleMakeCurrent"), "Draw ByLayer");

        bench.press("styleMakeCurrent");
        bench.draw();
        assert_eq!(bench.document().current_style(), "");
    }

    #[test]
    fn an_invalid_pattern_disables_save_and_a_pattern_edited_back_saves_nothing() {
        let mut bench = Fixture::new().manager();
        select_linetype(&mut bench, "DASHED", LinetypeOrigin::Drawing);
        assert_eq!(bench.rows("patternGrid"), 2);
        bench
            .dialog
            .set_pattern_current(bench.host.document(), 0);
        bench.draw();

        // A second dash straight after the first: dashes and gaps must alternate.
        bench.press("patternAddDash");
        assert_eq!(bench.rows("patternGrid"), 3);
        assert!(!bench.enabled("linetypeSave"));
        let invalid = bench.text("patternStatus");
        assert!(!invalid.starts_with("Valid"), "{invalid}");

        // Removed again: valid, and equal to what is stored.
        bench.press("patternRemove");
        assert_eq!(bench.rows("patternGrid"), 2);
        assert!(bench.text("patternStatus").starts_with("Valid"));
        let steps_before = bench.steps();
        bench.press("linetypeSave");
        assert_eq!(
            bench.steps(),
            steps_before,
            "update_linetype_if_changed: no change, no step"
        );
        assert_eq!(pattern_of(&bench, "DASHED").len(), 2);
    }

    #[test]
    fn a_gap_length_edited_in_the_grid_is_saved_as_one_step() {
        let mut bench = Fixture::new().manager();
        select_linetype(&mut bench, "DASHED", LinetypeOrigin::Drawing);
        assert!(bench.has("patternLength1"));
        bench.fill("patternLength1", "0.25");
        let steps_before = bench.steps();
        bench.press("linetypeSave");
        assert_eq!(bench.steps(), steps_before + 1);
        // The gap keeps its sign: a gap of 0.25 is stored as -0.25; the dash nobody touched is
        // still exactly 1.
        assert_eq!(pattern_of(&bench, "DASHED"), [1.0, -0.25]);
    }

    #[test]
    fn a_library_linestyle_is_read_only_and_makes_a_new_style_using_it() {
        let mut bench = Fixture::new().manager();
        select_linetype(&mut bench, "TEST Dashed Kerb", LinetypeOrigin::Library);
        assert!(!bench.enabled("linetypeDelete"));
        assert!(!bench.enabled("linetypeRename"));
        assert!(bench.text("libraryDetails").contains("Paper linestyle"));

        bench.press("newStyleUsing");
        // The linestyle's own name is free among the styles.
        assert_eq!(bench.value("promptName"), "TEST Dashed Kerb");
        bench.press("promptOk");
        assert!(has_style(&bench, "TEST Dashed Kerb"));
        assert_eq!(
            stored(&bench, "TEST Dashed Kerb").linetype,
            "TEST Dashed Kerb"
        );
        assert_eq!(bench.dialog.selected_styles(), ["TEST Dashed Kerb"]);
    }

    #[test]
    fn diagnostics_list_the_undefined_names_and_select_their_users() {
        // Old Survey names three undefined names, but only its linetype and symbol are resolved
        // names (a hatch is not); nothing wears it, so give one entity that style.
        let mut fixture = Fixture::new();
        let kerb_point = fixture.kerb_point;
        must(
            &mut fixture.host,
            set_entity_style(vec![kerb_point], "Old Survey"),
        );
        let mut bench = fixture.manager();
        assert_eq!(bench.rows("diagnosticTable"), 2);
        assert_eq!(bench.row("diagnosticTable", 0)[1], "Old Survey Kerb");
        assert_eq!(bench.row("diagnosticTable", 1)[1], "Old Survey Pit");
        // The first row chosen, by what its first cell says.
        bench.fill("diagnosticTable", "Linetype not defined");
        bench.press("diagnosticSelectUsers");
        assert_eq!(bench.document().selection().ids(), [kerb_point]);
    }

    #[test]
    fn the_dialog_may_outlive_its_document_and_then_neither_reads_nor_changes_it() {
        // The dialog's Document dies and a stranger stands where it was. A dialog still reading
        // the Document it is lent would read the stranger, so a read shows up as the stranger's
        // data on the form.
        let probe = |description: &str| Style {
            description: String::from(description),
            ..style("Probe", "continuous")
        };
        let mut document = Document::new(None);
        document
            .execute(create_style(probe("first")))
            .expect("the probe is made");
        let dialog = StyleManagerDialog::new(&CustomisationContext::default(), &document);
        drop(document);
        let mut host = RecordingHost::new();
        must(&mut host, create_style(probe("stranger")));
        let stranger_steps = steps(&host);
        let mut bench = Bench::showing(host, dialog);

        // Selecting the row, as a click does, loads no form from it...
        select_styles(&mut bench, &["Probe"]);
        assert_eq!(bench.value("styleDescription"), "");
        // ...and from then on every control but Close is off...
        assert!(!bench.enabled("managerTabs"));
        assert!(!bench.enabled("undoButton"));
        assert!(!bench.enabled("redoButton"));
        assert!(bench.enabled("closeButton"));
        // ...and a handler reached all the same acts on nothing: New asks for no name, Undo undoes
        // nothing.
        bench.dialog.ask_new_style(bench.host.document());
        bench.draw();
        assert!(bench.hidden("promptPanel"));
        bench.dialog.undo(&mut bench.host);
        assert_eq!(steps(&bench.host), stranger_steps);
        assert_eq!(stored(&bench, "Probe").description, "stranger");
    }

    #[test]
    fn the_chips_and_the_search_filter_the_styles_and_the_chips_count_them() {
        let mut bench = Fixture::new().manager();
        assert!(bench.has("stylesPage"));
        assert_eq!(bench.rows("styleTable"), 3);
        // Used: Kerb (1 point) and Fence (2); unused: Old Survey; missing: Old Survey.
        assert_eq!(bench.text("filterMissing"), "Missing (1)");
        assert_eq!(bench.text("filterUsed"), "Used (2)");
        bench.press("filterMissing");
        assert_eq!(bench.rows("styleTable"), 1);
        assert_eq!(bench.row("styleTable", 0)[0], "Old Survey");

        bench.press("filterAll");
        // The search folds case, and looks in the linetype too: "dashed" is Fence's DASHED and
        // Kerb's TEST Dashed Kerb.
        bench.fill("filterText", "dashed");
        assert_eq!(bench.rows("styleTable"), 2);
    }

    #[test]
    fn loading_a_library_that_defines_a_missing_name_reloads_the_marks_once() {
        let mut bench = Fixture::new().manager();
        select_styles(&mut bench, &["Old Survey"]);
        assert!(!bench.dialog.linetype_picker().current_is_defined());

        // A customisation load elsewhere brings "Old Survey Kerb" as a linestyle.
        let mut library = bench.document().style_library().clone();
        let kerb = LineStyle {
            name: String::from("Old Survey Kerb"),
            ..library
                .find("TEST Dashed Kerb")
                .expect("the kerb is there")
                .clone()
        };
        library.add(kerb).expect("the kerb is added");
        bench.host.document_mut().set_style_library(library);
        bench.draw();

        assert_eq!(bench.dialog.selected_styles(), ["Old Survey"]);
        assert!(bench.dialog.linetype_picker().current_is_defined());
        // Its symbol is still undefined, so the style is still "missing".
        assert_eq!(bench.text("filterMissing"), "Missing (1)");
        assert_eq!(
            bench.rows("diagnosticTable"),
            1,
            "only the symbol is left"
        );
    }

    #[test]
    fn a_reload_caused_elsewhere_keeps_a_form_part_way_through_an_edit() {
        let mut bench = Fixture::new().manager();
        select_styles(&mut bench, &["Kerb"]);
        bench.fill("styleDescription", "kerb and gutter");

        // Something else changes the drawing while the person is typing.
        execute(&mut bench, create_style(style("Zzz", "DASHED")));
        bench.draw();
        assert_eq!(
            bench.value("styleDescription"),
            "kerb and gutter",
            "the edit is not thrown away"
        );

        bench.press("styleSave");
        assert_eq!(stored(&bench, "Kerb").description, "kerb and gutter");
        assert_eq!(stored(&bench, "Kerb").linetype, "TEST Dashed Kerb");
    }

    #[test]
    fn renaming_a_style_through_the_prompt_takes_its_entities_and_keeps_it_selected() {
        let fixture = Fixture::new();
        let fence_points = fixture.fence_points.clone();
        let mut bench = fixture.manager();
        select_styles(&mut bench, &["Fence"]);
        bench.press("styleRename");
        assert_eq!(
            bench.value("promptName"),
            "Fence",
            "offered the current name to change"
        );
        bench.fill("promptName", "  Timber Fence ");
        bench.press("promptOk");

        assert!(!has_style(&bench, "Fence"));
        assert!(
            has_style(&bench, "Timber Fence"),
            "blanks either end dropped"
        );
        for &id in &fence_points {
            assert_eq!(style_of(&bench, id), "Timber Fence");
        }
        assert_eq!(bench.dialog.selected_styles(), ["Timber Fence"]);
        assert!(bench.hidden("promptPanel"));
    }

    #[test]
    fn merging_drawing_linetypes_is_one_undo_step_and_repoints_their_styles() {
        let mut fixture = Fixture::new();
        must(
            &mut fixture.host,
            create_linetype(linetype("HIDDEN", 0.5, -0.25)),
        );
        must(
            &mut fixture.host,
            create_style(style("Hidden Edge", "HIDDEN")),
        );
        let mut bench = fixture.manager();
        select_linetype(&mut bench, "HIDDEN", LinetypeOrigin::Drawing);
        let steps_before = bench.steps();

        bench.press("linetypeMerge");
        // Drawing linetypes only: continuous and DASHED (a library linestyle is no merge target -
        // the commands cannot see the library).
        assert_eq!(bench.rows("promptChoice"), 2);
        bench.fill("promptChoice", "DASHED");
        bench.press("promptOk");

        assert_eq!(bench.steps(), steps_before + 1);
        assert!(!bench.document().model().linetypes.contains("HIDDEN"));
        assert_eq!(stored(&bench, "Hidden Edge").linetype, "DASHED");
        assert_eq!(
            bench.dialog.selected_linetypes(),
            [(String::from("DASHED"), LinetypeOrigin::Drawing)]
        );
    }

    #[test]
    fn changing_an_element_to_a_dot_in_the_grid_disables_its_length_and_saves_a_dot() {
        let mut bench = Fixture::new().manager();
        select_linetype(&mut bench, "DASHED", LinetypeOrigin::Drawing);
        assert!(bench.has("patternKind0"));
        // As a person's choice arrives: the kind chosen in the row's own box.
        bench.fill("patternKind0", "Dot");
        assert!(bench.has("patternLength0"));
        assert!(!bench.enabled("patternLength0"), "a dot has no length");
        assert_eq!(bench.number("patternLength0"), 0.0);

        bench.press("linetypeSave");
        // {1, -0.5} with its dash made a dot: {0, -0.5}, a dot every half metre.
        assert_eq!(pattern_of(&bench, "DASHED"), [0.0, -0.5]);
    }

    #[test]
    fn a_gap_length_typed_through_zero_on_the_way_is_still_saved_as_a_gap() {
        let mut bench = Fixture::new().manager();
        select_linetype(&mut bench, "DASHED", LinetypeOrigin::Drawing);
        assert!(bench.has("patternLength1"));
        // As a person types 0.25 over the 0.5 shown: every keystroke is a value, and the first,
        // "0", is a length of nothing on the way to 0.25.
        for typed in ["0", "0.2", "0.25"] {
            bench.fill("patternLength1", typed);
        }
        assert_eq!(
            bench.number("patternLength1"),
            0.25,
            "the typing reached the box"
        );
        assert_eq!(bench.value("patternKind1"), "Gap");
        let status = bench.text("patternStatus");
        assert!(status.starts_with("Valid"), "{status}");

        let steps_before = bench.steps();
        bench.press("linetypeSave");
        assert_eq!(bench.steps(), steps_before + 1);
        // {1, -0.5} with the gap row's length typed as 0.25: {1, -0.25}, the row still the gap its
        // box says it is.
        assert_eq!(pattern_of(&bench, "DASHED"), [1.0, -0.25]);
    }

    #[test]
    fn a_group_filter_whose_group_a_replaced_library_drops_filters_nothing_any_more() {
        let mut fixture = Fixture::new();
        let mut roads = test_library();
        let kerb = LineStyle {
            group: String::from("Roads"),
            ..roads
                .find("TEST Dashed Kerb")
                .expect("the kerb is there")
                .clone()
        };
        add_or_replace(&mut roads, kerb).expect("the kerb is replaced");
        fixture.host.document_mut().set_style_library(roads);
        let mut bench = fixture.manager();
        bench.dialog.show_tab(Tab::Linetypes);
        bench.draw();
        bench.fill("linetypeGroup", "Roads");
        assert_eq!(bench.rows("linetypeTable"), 1, "TEST Dashed Kerb alone");

        // A customisation Replace elsewhere: the library no longer has "Roads".
        bench
            .host
            .document_mut()
            .set_style_library(test_library());
        bench.draw();
        assert_eq!(bench.value("linetypeGroup"), "All groups");
        // What the box says is what filters: every linetype again. By hand: continuous and DASHED
        // from the drawing, TEST Dashed Kerb from the library (TEST Manhole is drawn at vertices:
        // a symbol, not a linetype).
        assert_eq!(bench.rows("linetypeTable"), 3);
    }

    #[test]
    fn several_selected_linetypes_stay_selected_through_a_reload_caused_elsewhere() {
        let mut fixture = Fixture::new();
        must(
            &mut fixture.host,
            create_linetype(linetype("HIDDEN", 0.5, -0.25)),
        );
        let mut bench = fixture.manager();
        // DASHED clicked, then HIDDEN clicked with the command key held.
        let both = [
            (String::from("DASHED"), LinetypeOrigin::Drawing),
            (String::from("HIDDEN"), LinetypeOrigin::Drawing),
        ];
        select_linetype(&mut bench, "DASHED", LinetypeOrigin::Drawing);
        bench
            .dialog
            .select_linetypes(bench.host.document(), &both);
        bench.draw();
        assert_eq!(bench.dialog.selected_linetypes(), both);
        assert!(bench.enabled("linetypeMerge"));

        // Only the current style changes, elsewhere; the dialog reloads for it.
        bench
            .host
            .document_mut()
            .set_current_style("Kerb")
            .expect("Kerb is made current");
        bench.draw();
        assert_eq!(bench.dialog.selected_linetypes(), both);
        assert!(bench.enabled("linetypeMerge"));
    }
}
