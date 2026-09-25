#include "katana/cad/document_status.hpp"

#include <sstream>

#include <nlohmann/json.hpp>

#include "katana/entity/model.hpp"

namespace katana::cad {

DocumentStatus documentStatus(const Document& document)
{
    const katana::entity::Model& model = document.model();
    DocumentStatus status;
    if (const auto directory = document.projectDirectory()) {
        const std::u8string text = directory->u8string();
        status.project = std::string(reinterpret_cast<const char*>(text.data()), text.size());
    }
    status.modified = document.isModified();
    status.entities = model.entities.size();
    status.layers = model.layers.size();
    status.alignments = model.alignments.size();
    status.currentLayer = document.currentLayer();
    status.currentStyle = document.currentStyle();
    status.selected = document.selection().size();
    status.undoSteps = document.history().undoCount();
    status.redoSteps = document.history().redoCount();
    status.styleLibraryDefinitions = document.styleLibrary().size();
    status.surveyCodeRules = document.surveyMap().size();
    return status;
}

std::string formatStatus(const DocumentStatus& status)
{
    // The words katana_status said before it was moved here, kept exactly:
    // a client may have learnt to read them.
    std::ostringstream text;
    text << "Project: " << status.project.value_or("(none - not saved to a project yet)")
         << (status.modified ? "  [unsaved changes]" : "") << '\n'
         << "Entities: " << status.entities << "  Layers: " << status.layers
         << "  Alignments: " << status.alignments << '\n'
         << "Current layer: " << status.currentLayer;
    if (!status.currentStyle.empty()) {
        text << "  Current style: " << status.currentStyle;
    }
    text << '\n'
         << "Selected: " << status.selected << "  Undo steps: " << status.undoSteps
         << "  Redo steps: " << status.redoSteps << '\n'
         << "Customisation: " << status.styleLibraryDefinitions << " linestyles and symbols, "
         << status.surveyCodeRules << " survey code rules";
    return text.str();
}

std::string statusJson(const DocumentStatus& status)
{
    // nlohmann's object keeps its keys sorted, which is the order the
    // katana://status resource has always had.
    const nlohmann::json json{
        {"project", status.project ? nlohmann::json(*status.project) : nlohmann::json(nullptr)},
        {"modified", status.modified},
        {"entities", status.entities},
        {"layers", status.layers},
        {"currentLayer", status.currentLayer},
        {"currentStyle", status.currentStyle},
        {"selected", status.selected},
        {"alignments", status.alignments},
        {"undoSteps", status.undoSteps},
        {"redoSteps", status.redoSteps},
        {"styleLibraryDefinitions", status.styleLibraryDefinitions},
        {"surveyCodeRules", status.surveyCodeRules},
    };
    // A name that is not UTF-8 is shown with U+FFFD rather than thrown at a
    // client that only asked how the drawing stands.
    return json.dump(2, ' ', false, nlohmann::json::error_handler_t::replace);
}

} // namespace katana::cad
