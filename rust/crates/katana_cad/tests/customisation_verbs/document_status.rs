//! STATUS [JSON] (document_status): the drawing's state, one definition for the interpreter's
//! verb, katana_mcp's katana_status and File > Drawing Summary
//! (tests/cad/customisation/test_document_status.cpp). The expected texts are written out by hand
//! from the fields each test sets: the words katana_status has always used, and the JSON with its
//! keys in alphabetical order, indented by two, as the katana://status resource has always been.

use katana_cad::document_status::{document_status, format_status, status_json};
use katana_cad::{CommandInterpreter, Document, DocumentStatus};

fn hand_made() -> DocumentStatus {
    DocumentStatus {
        project: Some(String::from("C:/work/site \"A\"\\plans")),
        modified: true,
        entities: 7,
        layers: 3,
        alignments: 2,
        current_layer: String::from("roads/kerb"),
        current_style: String::from("Fence"),
        selected: 1,
        undo_steps: 4,
        redo_steps: 1,
        style_library_definitions: 792,
        survey_code_rules: 11,
    }
}

// The reply to `line`, failing the test when it is refused.
#[track_caller]
fn ok(interpreter: &mut CommandInterpreter, document: &mut Document, line: &str) -> String {
    interpreter
        .run(document, line)
        .unwrap_or_else(|error| panic!("{line}: {}", error.describe()))
}

mod document_status {
    use super::*;

    #[test]
    fn an_empty_drawing_has_only_its_first_layer_and_no_project() {
        let document = Document::default();
        let status = document_status(&document);
        assert!(status.project.is_none());
        assert!(!status.modified);
        assert_eq!(status.entities, 0);
        assert_eq!(status.layers, 1);
        assert_eq!(status.current_layer, "0");
        assert!(status.current_style.is_empty());
        assert_eq!(status.undo_steps, 0);
        assert_eq!(status.redo_steps, 0);
    }

    #[test]
    fn it_follows_the_drawing_the_selection_and_the_history() {
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::new();
        ok(&mut interpreter, &mut document, "RECT 0,0 10,5");
        ok(&mut interpreter, &mut document, "CIRCLE 5,5 2");
        ok(&mut interpreter, &mut document, "LAYER NEW roads");
        ok(&mut interpreter, &mut document, "UNDO"); // the layer
        ok(&mut interpreter, &mut document, "SELECT 1");
        let status = document_status(&document);
        assert!(status.modified);
        assert_eq!(status.entities, 2);
        assert_eq!(status.layers, 1);
        assert_eq!(status.selected, 1);
        assert_eq!(status.undo_steps, 2);
        assert_eq!(status.redo_steps, 1);
    }

    #[test]
    fn the_text_is_katana_statuses_words() {
        assert_eq!(
            format_status(&hand_made()),
            concat!(
                "Project: C:/work/site \"A\"\\plans  [unsaved changes]\n",
                "Entities: 7  Layers: 3  Alignments: 2\n",
                "Current layer: roads/kerb  Current style: Fence\n",
                "Selected: 1  Undo steps: 4  Redo steps: 1\n",
                "Customisation: 792 linestyles and symbols, 11 survey code rules"
            )
        );
    }

    #[test]
    fn the_text_says_when_there_is_no_project_and_no_current_style() {
        let status = DocumentStatus {
            current_layer: String::from("0"),
            ..DocumentStatus::default()
        };
        assert_eq!(
            format_status(&status),
            concat!(
                "Project: (none - not saved to a project yet)\n",
                "Entities: 0  Layers: 0  Alignments: 0\n",
                "Current layer: 0\n",
                "Selected: 0  Undo steps: 0  Redo steps: 0\n",
                "Customisation: 0 linestyles and symbols, 0 survey code rules"
            )
        );
    }

    #[test]
    fn the_json_has_every_mcp_key_in_order_and_escapes_the_project() {
        assert_eq!(
            status_json(&hand_made()),
            concat!(
                "{\n",
                "  \"alignments\": 2,\n",
                "  \"currentLayer\": \"roads/kerb\",\n",
                "  \"currentStyle\": \"Fence\",\n",
                "  \"entities\": 7,\n",
                "  \"layers\": 3,\n",
                "  \"modified\": true,\n",
                "  \"project\": \"C:/work/site \\\"A\\\"\\\\plans\",\n",
                "  \"redoSteps\": 1,\n",
                "  \"selected\": 1,\n",
                "  \"styleLibraryDefinitions\": 792,\n",
                "  \"surveyCodeRules\": 11,\n",
                "  \"undoSteps\": 4\n",
                "}"
            )
        );
    }

    #[test]
    fn no_project_is_null_in_the_json() {
        let json = status_json(&DocumentStatus::default());
        assert!(json.contains("\"project\": null,"), "{json}");
        assert!(json.contains("\"modified\": false,"), "{json}");
    }

    // DocumentStatus.ANameThatIsNotUtf8IsReplacedRatherThanThrown is not twinned:
    // its one subject is a layer name holding a UTF-8 lead byte with no follower,
    // which DocumentStatus::current_layer, a String, cannot hold - the model holds
    // validated UTF-8 (docs/rust.md, "Step 2"). An exemption candidate.

    #[test]
    fn the_status_verb_answers_in_text_or_json() {
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::new();
        ok(&mut interpreter, &mut document, "RECT 0,0 10,5");
        let text = ok(&mut interpreter, &mut document, "STATUS");
        assert_eq!(text, format_status(&document_status(&document)));
        assert!(text.contains("Entities: 1  Layers: 1  Alignments: 0"));
        let json = ok(&mut interpreter, &mut document, "status json");
        assert_eq!(json, status_json(&document_status(&document)));
        // Asking changes nothing, so it is no undo step.
        assert_eq!(document.history().undo_count(), 1);
    }

    #[test]
    fn the_status_verb_refuses_a_form_it_does_not_know() {
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::new();
        for line in ["STATUS XML", "STATUS JSON extra"] {
            let error = interpreter.run(&mut document, line).expect_err(line);
            assert!(
                error.describe().contains("STATUS [JSON]"),
                "{}",
                error.describe()
            );
        }
        assert!(CommandInterpreter::help_text().contains("STATUS [JSON]"));
    }
}
