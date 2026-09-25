// STATUS [JSON] (document_status.hpp): the drawing's state, one definition for
// the interpreter's verb, katana_mcp's katana_status and File > Drawing
// Summary. The expected texts are written out by hand from the fields each
// test sets: the words katana_status has always used, and the JSON with its
// keys in alphabetical order, indented by two, as the katana://status
// resource has always been.

#include <gtest/gtest.h>

#include <string>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/document_status.hpp"
#include "katana/entity/style_library.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::DocumentStatus;

namespace {

DocumentStatus handMade()
{
    DocumentStatus status;
    status.project = "C:/work/site \"A\"\\plans";
    status.modified = true;
    status.entities = 7;
    status.layers = 3;
    status.alignments = 2;
    status.currentLayer = "roads/kerb";
    status.currentStyle = "Fence";
    status.selected = 1;
    status.undoSteps = 4;
    status.redoSteps = 1;
    status.styleLibraryDefinitions = 792;
    status.surveyCodeRules = 11;
    return status;
}

TEST(DocumentStatus, AnEmptyDrawingHasOnlyItsFirstLayerAndNoProject)
{
    Document document;
    const DocumentStatus status = katana::cad::documentStatus(document);
    EXPECT_FALSE(status.project.has_value());
    EXPECT_FALSE(status.modified);
    EXPECT_EQ(status.entities, 0U);
    EXPECT_EQ(status.layers, 1U);
    EXPECT_EQ(status.currentLayer, "0");
    EXPECT_TRUE(status.currentStyle.empty());
    EXPECT_EQ(status.undoSteps, 0U);
    EXPECT_EQ(status.redoSteps, 0U);
}

TEST(DocumentStatus, ItFollowsTheDrawingTheSelectionAndTheHistory)
{
    Document document;
    CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("RECT 0,0 10,5").ok());
    ASSERT_TRUE(interpreter.run("CIRCLE 5,5 2").ok());
    ASSERT_TRUE(interpreter.run("LAYER NEW roads").ok());
    ASSERT_TRUE(interpreter.run("UNDO").ok()); // the layer
    ASSERT_TRUE(interpreter.run("SELECT 1").ok());
    const DocumentStatus status = katana::cad::documentStatus(document);
    EXPECT_TRUE(status.modified);
    EXPECT_EQ(status.entities, 2U);
    EXPECT_EQ(status.layers, 1U);
    EXPECT_EQ(status.selected, 1U);
    EXPECT_EQ(status.undoSteps, 2U);
    EXPECT_EQ(status.redoSteps, 1U);
}

TEST(DocumentStatus, TheTextIsKatanaStatusesWords)
{
    EXPECT_EQ(katana::cad::formatStatus(handMade()),
              "Project: C:/work/site \"A\"\\plans  [unsaved changes]\n"
              "Entities: 7  Layers: 3  Alignments: 2\n"
              "Current layer: roads/kerb  Current style: Fence\n"
              "Selected: 1  Undo steps: 4  Redo steps: 1\n"
              "Customisation: 792 linestyles and symbols, 11 survey code rules");
}

TEST(DocumentStatus, TheTextSaysWhenThereIsNoProjectAndNoCurrentStyle)
{
    DocumentStatus status;
    status.currentLayer = "0";
    EXPECT_EQ(katana::cad::formatStatus(status),
              "Project: (none - not saved to a project yet)\n"
              "Entities: 0  Layers: 0  Alignments: 0\n"
              "Current layer: 0\n"
              "Selected: 0  Undo steps: 0  Redo steps: 0\n"
              "Customisation: 0 linestyles and symbols, 0 survey code rules");
}

TEST(DocumentStatus, TheJsonHasEveryMcpKeyInOrderAndEscapesTheProject)
{
    EXPECT_EQ(katana::cad::statusJson(handMade()),
              "{\n"
              "  \"alignments\": 2,\n"
              "  \"currentLayer\": \"roads/kerb\",\n"
              "  \"currentStyle\": \"Fence\",\n"
              "  \"entities\": 7,\n"
              "  \"layers\": 3,\n"
              "  \"modified\": true,\n"
              "  \"project\": \"C:/work/site \\\"A\\\"\\\\plans\",\n"
              "  \"redoSteps\": 1,\n"
              "  \"selected\": 1,\n"
              "  \"styleLibraryDefinitions\": 792,\n"
              "  \"surveyCodeRules\": 11,\n"
              "  \"undoSteps\": 4\n"
              "}");
}

TEST(DocumentStatus, NoProjectIsNullInTheJson)
{
    const std::string json = katana::cad::statusJson(DocumentStatus{});
    EXPECT_NE(json.find("\"project\": null,"), std::string::npos) << json;
    EXPECT_NE(json.find("\"modified\": false,"), std::string::npos) << json;
}

TEST(DocumentStatus, ANameThatIsNotUtf8IsReplacedRatherThanThrown)
{
    DocumentStatus status;
    status.currentLayer = std::string("caf\xC3", 4); // a UTF-8 lead byte with no follower
    const std::string json = katana::cad::statusJson(status);
    EXPECT_NE(json.find("\"currentLayer\": \"caf\xEF\xBF\xBD\""), std::string::npos) << json;
}

TEST(DocumentStatus, TheStatusVerbAnswersInTextOrJson)
{
    Document document;
    CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("RECT 0,0 10,5").ok());
    const auto text = interpreter.run("STATUS");
    ASSERT_TRUE(text.ok());
    EXPECT_EQ(*text, katana::cad::formatStatus(katana::cad::documentStatus(document)));
    EXPECT_NE(text->find("Entities: 1  Layers: 1  Alignments: 0"), std::string::npos);
    const auto json = interpreter.run("status json");
    ASSERT_TRUE(json.ok());
    EXPECT_EQ(*json, katana::cad::statusJson(katana::cad::documentStatus(document)));
    // Asking changes nothing, so it is no undo step.
    EXPECT_EQ(document.history().undoCount(), 1U);
}

TEST(DocumentStatus, TheStatusVerbRefusesAFormItDoesNotKnow)
{
    Document document;
    CommandInterpreter interpreter(document);
    for (const char* line : {"STATUS XML", "STATUS JSON extra"}) {
        const auto reply = interpreter.run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_NE(reply.error().describe().find("STATUS [JSON]"), std::string::npos);
    }
    EXPECT_NE(CommandInterpreter::helpText().find("STATUS [JSON]"), std::string::npos);
}

} // namespace
