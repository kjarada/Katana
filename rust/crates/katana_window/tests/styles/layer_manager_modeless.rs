//! The twins of tests/qt_widgets/customisation/test_layer_manager_modeless.cpp: the Layers dialog
//! as a non-modal window beside the drawing - it reloads once, at its next frame, when the drawing
//! changes anywhere; it asks for names in a prompt row and takes a colour typed or picked without
//! waiting; and it outlives its Document without touching it.

use katana_cad::Document;
use katana_commands::{EntityAttributes, create_layer, create_point, update_layer};
use katana_entity::{Color, Layer};
use katana_geometry::Point2;
use katana_window::host::RecordingHost;
use katana_window::layer_manager::LayerManagerDialog;

use crate::support::{Bench, create, must, steps};

fn layer_named(name: &str) -> Layer {
    Layer {
        name: String::from(name),
        ..Layer::default()
    }
}

/// Layer "survey" with a colour and a weight; the host's log is what the dialog said.
fn fixture_host() -> RecordingHost {
    let mut host = RecordingHost::new();
    let survey = Layer {
        color: Color::new(0, 200, 120, 255),
        line_weight: 0.35,
        ..layer_named("survey")
    };
    must(&mut host, create_layer(survey));
    host
}

fn fixture() -> Bench<LayerManagerDialog> {
    Bench::new(fixture_host(), LayerManagerDialog::new)
}

fn stored(bench: &Bench<LayerManagerDialog>, name: &str) -> Layer {
    bench
        .document()
        .model()
        .layers
        .find(name)
        .unwrap_or_else(|| panic!("{name} is there"))
        .clone()
}

fn select(bench: &mut Bench<LayerManagerDialog>, name: &str) {
    bench.dialog.select_layer(bench.host.document(), name);
    bench.draw();
}

/// The table's first column, as the last frame drew it.
fn table_names(bench: &Bench<LayerManagerDialog>) -> Vec<String> {
    (0..bench.rows("layerTable"))
        .map(|row| bench.row("layerTable", row)[0].clone())
        .collect()
}

/// Types `text` into the prompt and presses OK.
fn answer(bench: &mut Bench<LayerManagerDialog>, text: &str) {
    bench.fill("layerPromptName", text);
    bench.press("layerPromptOk");
}

mod layers_modeless {
    use super::*;

    #[test]
    fn a_command_run_elsewhere_reloads_the_table_once_from_the_event_loop_keeping_the_selection() {
        let mut bench = fixture();
        select(&mut bench, "survey");
        assert_eq!(bench.dialog.selected_layer(), "survey");

        must(&mut bench.host, create_layer(layer_named("roads")));
        must(&mut bench.host, create_layer(layer_named("kerbs")));
        // Not inside the command: the reload is deferred (docs/cad.md).
        assert_eq!(bench.dialog.table_names(), ["0", "survey"]);
        bench.draw();
        assert_eq!(table_names(&bench), ["0", "kerbs", "roads", "survey"]);
        assert_eq!(
            bench.dialog.selected_layer(),
            "survey",
            "the selection survives the reload"
        );

        // And an undo made anywhere is seen the same way.
        bench.host.document_mut().undo().expect("the undo is done");
        bench.draw();
        assert_eq!(table_names(&bench), ["0", "roads", "survey"]);
    }

    #[test]
    fn new_and_new_child_ask_for_the_name_in_the_dialog_and_select_what_they_made() {
        let mut bench = fixture();
        assert!(bench.hidden("layerPrompt"));

        bench.press("layerNew");
        // A row inside the dialog, not a box: nothing modal is open.
        assert!(!bench.hidden("layerPrompt"));
        let frame = bench.harness.snapshot();
        assert!(frame.targets.iter().all(|target| !target.modal));
        answer(&mut bench, "  design/surface  "); // the blanks either end are never meant
        assert!(bench.hidden("layerPrompt"));
        assert!(bench.document().model().layers.contains("design/surface"));
        bench.draw();
        assert_eq!(bench.dialog.selected_layer(), "design/surface");

        select(&mut bench, "design");
        bench.press("layerNewChild");
        assert!(!bench.hidden("layerPrompt"));
        answer(&mut bench, "tin1");
        assert!(bench.document().model().layers.contains("design/tin1"));
        bench.draw();
        assert_eq!(bench.dialog.selected_layer(), "design/tin1");
    }

    #[test]
    fn rename_or_move_offers_the_path_and_takes_the_layer_and_its_entities_with_it() {
        let mut host = fixture_host();
        let point = create(
            &mut host,
            create_point(
                Point2::new(1.0, 1.0),
                EntityAttributes {
                    layer: String::from("survey"),
                    style: String::new(),
                    color: None,
                },
            ),
        );
        let mut bench = Bench::new(host, LayerManagerDialog::new);
        select(&mut bench, "survey");
        bench.press("layerMove");
        assert_eq!(
            bench.value("layerPromptName"),
            "survey",
            "the path to edit, not an empty field"
        );
        bench.fill("layerPromptName", "site/survey");
        bench.dialog.accept_prompt(&mut bench.host); // Enter takes the answer, as OK does

        assert!(!bench.document().model().layers.contains("survey"));
        let moved = bench.document().model().entities.find(point);
        assert_eq!(moved.expect("the point is there").layer, "site/survey");
        bench.draw();
        assert_eq!(bench.dialog.selected_layer(), "site/survey");
    }

    #[test]
    fn a_refused_name_is_said_on_the_status_line_and_in_the_log_and_changes_nothing() {
        let mut bench = fixture();
        let before = bench.steps();
        bench.press("layerNew");
        answer(&mut bench, "a//b");
        assert_eq!(bench.steps(), before);
        assert!(!bench.document().model().layers.contains("a//b"));
        let status = bench.text("layerStatus");
        assert!(status.contains("empty level"), "{status}");
        let said = bench.host.logged();
        assert!(said.last().expect("the dialog said so").1, "an error");
    }

    #[test]
    fn a_colour_is_typed_as_hex_or_picked_in_a_dialog_that_does_not_block() {
        let mut bench = fixture();
        select(&mut bench, "survey");
        assert_eq!(bench.value("layerColourText"), "#00C878", "0, 200, 120");

        // Something that is not a colour is refused, and the field shows the last good one again.
        bench.fill("layerColourText", "orange-ish");
        bench.dialog.finish_colour_text(&mut bench.host);
        bench.draw();
        assert_eq!(bench.value("layerColourText"), "#00C878");
        assert!(bench.host.logged().last().expect("it was said").1);

        // Typed: #ff8000 is 255, 128, 0, and is shown as the model writes it.
        bench.fill("layerColourText", "#ff8000");
        bench.dialog.finish_colour_text(&mut bench.host);
        bench.draw();
        assert_eq!(bench.value("layerColourText"), "#FF8000");
        bench.press("layerSave");
        assert_eq!(stored(&bench, "survey").color, Color::new(255, 128, 0, 255));
        assert_eq!(
            stored(&bench, "survey").line_weight,
            0.35,
            "only the colour was edited"
        );

        // Picked: the button returns at once - the picker is opened, nothing waits on it - and its
        // answer arrives when it is accepted.
        bench.draw();
        bench.press("layerColour");
        assert!(bench.dialog.colour_dialog_open());
        assert!(bench.has("layerColourDialog"));
        bench.dialog.pick_colour(Color::new(0, 0, 255, 255));
        bench.draw();
        assert_eq!(bench.value("layerColourText"), "#0000FF");
        bench.press("layerSave");
        assert_eq!(stored(&bench, "survey").color, Color::new(0, 0, 255, 255));
    }

    #[test]
    fn unsaved_edits_survive_a_reload_unless_their_layer_changed_underneath() {
        let mut bench = fixture();
        select(&mut bench, "survey");
        bench.fill("layerWeight", "0.5");

        // Another layer's creation reloads the table; the form's edit stays.
        must(&mut bench.host, create_layer(layer_named("other")));
        bench.draw();
        assert_eq!(bench.number("layerWeight"), 0.5);
        bench.press("layerSave");
        assert_eq!(stored(&bench, "survey").line_weight, 0.5);

        // Survey itself hidden elsewhere: the form shows the layer as it is now, so a Save cannot
        // write the old visibility back.
        bench.draw();
        bench.fill("layerWeight", "1");
        let mut hidden = stored(&bench, "survey");
        hidden.visible = false;
        must(&mut bench.host, update_layer(hidden));
        bench.draw();
        assert!(!bench.checked("layerVisible"));
        assert_eq!(bench.number("layerWeight"), 0.5);
    }

    #[test]
    fn the_dialog_may_outlive_its_document_and_then_neither_reads_nor_changes_it() {
        // The Document dies and a stranger stands where it was: a dialog still reading the
        // Document it is lent would show the stranger's layer.
        let document = Document::new(None);
        let dialog = LayerManagerDialog::new(&document);
        drop(document);
        let mut host = RecordingHost::new();
        must(
            &mut host,
            create_layer(Layer {
                line_weight: 2.0,
                ..layer_named("stranger")
            }),
        );
        let stranger_steps = steps(&host);
        let mut bench = Bench::showing(host, dialog);
        assert_eq!(table_names(&bench), ["0"], "no reload from it");

        // Selecting a row, as a click does, loads no form: the stranger's "0" would show its
        // 0.25 mm, where the untouched field shows 0.
        select(&mut bench, "0");
        assert_eq!(bench.number("layerWeight"), 0.0);
        // ...and from then on every control but Close is off...
        assert!(!bench.enabled("layerNew"));
        assert!(!bench.enabled("layerSave"));
        assert!(!bench.enabled("layerColourText"));
        assert!(bench.enabled("closeButton"));
        // ...and a handler reached all the same acts on nothing.
        bench.dialog.ask_new();
        bench.draw();
        assert!(bench.hidden("layerPrompt"));
        bench.dialog.delete(&mut bench.host);
        bench.dialog.make_current(&mut bench.host);
        assert_eq!(steps(&bench.host), stranger_steps);
        assert_eq!(bench.document().current_layer(), "0");
        assert!(bench.text("layerStatus").contains("has closed"));
    }
}
