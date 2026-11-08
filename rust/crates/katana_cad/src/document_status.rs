//! STATUS [JSON]: the drawing's state at a glance - its project, whether it
//! has unsaved changes, what it holds, what is current, the selection, the
//! undo history and the customisation loaded (docs/cad.md, "STATUS";
//! include/katana/cad/document_status.hpp, src/katana_cad/document_status.cpp).
//!
//! One definition, three front ends. katana_mcp's katana_status tool and its
//! katana://status resource were built in the MCP server alone; the window
//! had only pieces of it (the title, the current layer), and nothing an agent
//! driving the window or katana_cli could ask for. They all come from here
//! now: the interpreter's STATUS verb, the MCP tool and resource, and File >
//! Drawing Summary's Copy as JSON.

use katana_core::json::Json;
use katana_core::path_text::path_to_utf8;

use crate::document::Document;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentStatus {
    /// The project directory, UTF-8; None when the drawing has none yet. A
    /// directory whose name is not UTF-8 holds U+FFFD where the C++ kept the
    /// bytes (a String cannot), which is what its JSON said of them anyway.
    pub project: Option<String>,
    pub modified: bool,
    pub entities: usize,
    pub layers: usize,
    pub alignments: usize,
    pub current_layer: String,
    pub current_style: String, // empty: ByLayer
    pub selected: usize,
    pub undo_steps: usize,
    pub redo_steps: usize,
    pub style_library_definitions: usize,
    pub survey_code_rules: usize,
}

pub fn document_status(document: &Document) -> DocumentStatus {
    let model = document.model();
    DocumentStatus {
        project: document
            .project_directory()
            .map(|directory| String::from_utf8_lossy(&path_to_utf8(&directory)).into_owned()),
        modified: document.is_modified(),
        entities: model.entities.size(),
        layers: model.layers.size(),
        alignments: model.alignments.size(),
        current_layer: String::from(document.current_layer()),
        current_style: String::from(document.current_style()),
        selected: document.selection().size(),
        undo_steps: document.history().undo_count(),
        redo_steps: document.history().redo_count(),
        style_library_definitions: document.style_library().size(),
        survey_code_rules: document.survey_map().size(),
    }
}

/// Five lines of plain text, the last without a line end: what STATUS and
/// katana_status print.
pub fn format_status(status: &DocumentStatus) -> String {
    // The words katana_status said before it was moved here, kept exactly:
    // a client may have learnt to read them.
    let mut text = format!(
        "Project: {}{}\n",
        status
            .project
            .as_deref()
            .unwrap_or("(none - not saved to a project yet)"),
        if status.modified {
            "  [unsaved changes]"
        } else {
            ""
        }
    );
    text.push_str(&format!(
        "Entities: {}  Layers: {}  Alignments: {}\n",
        status.entities, status.layers, status.alignments
    ));
    text.push_str(&format!("Current layer: {}", status.current_layer));
    if !status.current_style.is_empty() {
        text.push_str(&format!("  Current style: {}", status.current_style));
    }
    text.push('\n');
    text.push_str(&format!(
        "Selected: {}  Undo steps: {}  Redo steps: {}\n",
        status.selected, status.undo_steps, status.redo_steps
    ));
    text.push_str(&format!(
        "Customisation: {} linestyles and symbols, {} survey code rules",
        status.style_library_definitions, status.survey_code_rules
    ));
    text
}

/// The status as a JSON object, keys in alphabetical order and indented by
/// two, with no line end after the closing brace: what STATUS JSON prints and
/// the katana://status resource holds. The keys are the C++ fields' names,
/// spelt as they are there; `project` is null when there is none.
pub fn status_json(status: &DocumentStatus) -> String {
    // nlohmann's object keeps its keys sorted, which is the order the
    // katana://status resource has always had.
    let json = Json::object_of([
        (
            "project",
            status.project.as_deref().map_or(Json::Null, Json::from),
        ),
        ("modified", Json::from(status.modified)),
        ("entities", Json::from(status.entities)),
        ("layers", Json::from(status.layers)),
        ("currentLayer", Json::from(status.current_layer.as_str())),
        ("currentStyle", Json::from(status.current_style.as_str())),
        ("selected", Json::from(status.selected)),
        ("alignments", Json::from(status.alignments)),
        ("undoSteps", Json::from(status.undo_steps)),
        ("redoSteps", Json::from(status.redo_steps)),
        (
            "styleLibraryDefinitions",
            Json::from(status.style_library_definitions),
        ),
        ("surveyCodeRules", Json::from(status.survey_code_rules)),
    ]);
    // A name that is not UTF-8 was shown with U+FFFD rather than thrown at a
    // client that only asked how the drawing stands; a String holds none.
    json.dump_with(2, ' ', false)
}
