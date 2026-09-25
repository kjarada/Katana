// The Label Objects tool (annotate_label.cpp), driven through ToolDriver as
// the plan view drives it, and held to the LABEL verb: the same answers
// typed as one LABEL line in another drawing must make the same label. The
// drawings are made with the verbs an agent would type, so the ids are the
// order of the lines below.

#include <optional>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/entity/entity.hpp"
#include "tool_driver.hpp"

namespace {

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::LabelGeometry;
using katana::geometry::Point2;
using Outcome = katana::cad::ToolStep::Outcome;

void run(Document& document, const std::string& line)
{
    CommandInterpreter interpreter(document);
    const auto reply = interpreter.run(line);
    ASSERT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
}

EntityId newest(Document& document)
{
    const auto ids = document.lastCreatedEntities();
    EXPECT_EQ(ids.size(), 1u);
    return ids.empty() ? 0 : ids.front();
}

// Every label in the drawing, in id order, with its layer.
std::vector<Entity> labelsOf(Document& document)
{
    std::vector<Entity> out;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<LabelGeometry>(entity.geometry)) {
            out.push_back(entity);
        }
    });
    return out;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// The drawing both sides of a comparison start from: the standard label
// styles, then a closed 30 x 40 lot (id 1) and a line 0,50 -> 10,50 (id 2).
void standardDrawing(Document& document)
{
    run(document, "LABELSTYLE DEFAULTS");
    run(document, "PLINE 0,0 30,0 30,40 0,40 CLOSE");
    run(document, "LINE 0,50 10,50");
}

void select(Document& document, std::vector<EntityId> ids)
{
    document.selection().set(std::move(ids));
}

} // namespace

TEST(AnnotateLabelTool, ItIsInTheAnnotateMenuUnderLabelsAndABareLabelStartsIt)
{
    const auto& catalog = katana::cad::toolCatalog();
    const katana::cad::ToolInfo* info = catalog.find("annotate.label");
    ASSERT_NE(info, nullptr);
    EXPECT_EQ(info->name, "Label Objects");
    EXPECT_EQ(info->category, "Annotate");
    EXPECT_EQ(info->group, "Labels");
    for (const char* alias : {"LABELOBJECTS", "LBL", "LABEL"}) {
        const katana::cad::ToolInfo* byAlias = catalog.findByAlias(alias);
        ASSERT_NE(byAlias, nullptr) << alias;
        EXPECT_EQ(byAlias->id, "annotate.label") << alias;
    }
}

TEST(AnnotateLabelTool, APreselectedLotIsLabelledWhereTheTextIsPutAsTheVerbLabelsIt)
{
    ToolDriver driver;
    standardDrawing(driver.document());
    select(driver.document(), {1});
    driver.start("annotate.label");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point) << "the selection is taken as given";
    EXPECT_EQ(driver.tool().prompt(), "Specify text location or [Style/Part/Text/Undo] <automatic>");
    EXPECT_EQ(driver.type("S").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_EQ(driver.type("Lot Area").outcome, Outcome::Continue);
    const ToolStep done = driver.click(10.0, 12.0);
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_EQ(done.message, "1 label in style Lot Area");
    EXPECT_EQ(driver.executed(), 1);

    Document typed;
    standardDrawing(typed);
    run(typed, "LABEL 1 style=\"Lot Area\" at=10,12");
    const auto made = labelsOf(driver.document());
    const auto expected = labelsOf(typed);
    ASSERT_EQ(made.size(), 1u);
    ASSERT_EQ(expected.size(), 1u);
    EXPECT_EQ(made[0].geometry, expected[0].geometry);
    EXPECT_EQ(made[0].layer, expected[0].layer);
    const auto& label = std::get<LabelGeometry>(made[0].geometry);
    EXPECT_EQ(label.target, 1u);
    EXPECT_EQ(label.position, std::optional<Point2>(Point2(10.0, 12.0)));

    // One undo takes it back.
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(labelsOf(driver.document()).empty());
}

TEST(AnnotateLabelTool, WithNoneUsedYetTheFirstStyleByNameThatCanLabelTheObjectIsOffered)
{
    // The standard styles by name: Arc Data (arcs), then Bearing Distance
    // (segments) - the first that can label a line.
    ToolDriver driver;
    standardDrawing(driver.document());
    select(driver.document(), {2});
    driver.start("annotate.label");
    (void)driver.type("Style");
    EXPECT_EQ(driver.tool().prompt(), "Enter label style name <Bearing Distance>");
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue) << "Enter keeps the one offered";
    const ToolStep done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_EQ(done.message, "1 label in style Bearing Distance");
    const auto made = labelsOf(driver.document());
    ASSERT_EQ(made.size(), 1u);
    const auto& label = std::get<LabelGeometry>(made[0].geometry);
    EXPECT_FALSE(label.position.has_value()) << "Enter leaves the text to the placer";
    EXPECT_EQ(label.style, "Bearing Distance");
}

TEST(AnnotateLabelTool, TheStyleOfferedNextIsTheNewestHandPlacedLabelsWhenItFits)
{
    ToolDriver driver;
    standardDrawing(driver.document());
    run(driver.document(), "PLINE 40,0 60,0 60,20 CLOSE"); // id 3
    run(driver.document(), "LABEL 1 style=\"Lot Area\"");
    select(driver.document(), {3});
    driver.start("annotate.label");
    (void)driver.type("S");
    EXPECT_EQ(driver.tool().prompt(), "Enter label style name <Lot Area>");
    (void)driver.enter();
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);

    // A line cannot wear an area style: the first that can label it instead.
    select(driver.document(), {2});
    driver.start("annotate.label");
    (void)driver.type("S");
    EXPECT_EQ(driver.tool().prompt(), "Enter label style name <Bearing Distance>");
}

TEST(AnnotateLabelTool, SeveralObjectsAreOneUndoStepAndShareNoPlace)
{
    ToolDriver driver;
    standardDrawing(driver.document());
    run(driver.document(), "LINE 0,60 20,60"); // id 3
    select(driver.document(), {});
    driver.start("annotate.label");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection);
    EXPECT_EQ(driver.tool().prompt(), "Select objects to label or [Style/Part/Text], then press Enter");
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected) << "the objects come first";
    (void)driver.pick(2, 5.0, 50.0);
    EXPECT_EQ(driver.pick(2, 5.0, 50.0).outcome, Outcome::Rejected) << "the same line twice";
    (void)driver.pick(3, 5.0, 60.0);
    const ToolStep chosen = driver.enter();
    ASSERT_EQ(chosen.outcome, Outcome::Continue);
    EXPECT_EQ(chosen.message, "2 objects to label in style Bearing Distance");
    EXPECT_TRUE(contains(driver.tool().prompt(), "label the 2 objects"));
    const ToolStep shared = driver.click(5.0, 70.0);
    EXPECT_EQ(shared.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(shared.message, "cannot share one place")) << shared.message;
    const ToolStep done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_EQ(done.message, "2 labels in style Bearing Distance");
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(labelsOf(driver.document()).size(), 2u);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(labelsOf(driver.document()).empty()) << "one step took back both";
}

TEST(AnnotateLabelTool, PartAndTextAreTheVerbsPartAndText)
{
    ToolDriver driver;
    standardDrawing(driver.document());
    select(driver.document(), {1});
    driver.start("annotate.label");
    (void)driver.type("S");
    (void)driver.type("Bearing Distance");
    (void)driver.type("P");
    EXPECT_EQ(driver.tool().prompt(),
              "Enter the polyline segment to label, 0 for the first, or [All] <all>");
    EXPECT_EQ(driver.type("-1").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("1.5").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("1").outcome, Outcome::Continue);
    (void)driver.type("T");
    EXPECT_EQ(driver.type("KERB, 150").outcome, Outcome::Continue) << "a comma is text here";
    ASSERT_EQ(driver.click(35.0, 20.0).outcome, Outcome::Done);

    Document typed;
    standardDrawing(typed);
    run(typed, "LABEL 1 style=\"Bearing Distance\" part=1 text=\"KERB, 150\" at=35,20");
    const auto made = labelsOf(driver.document());
    const auto expected = labelsOf(typed);
    ASSERT_EQ(made.size(), 1u);
    ASSERT_EQ(expected.size(), 1u);
    EXPECT_EQ(made[0].geometry, expected[0].geometry);
    const auto& label = std::get<LabelGeometry>(made[0].geometry);
    EXPECT_EQ(label.part, 1);
    EXPECT_EQ(label.textOverride, "KERB, 150");
}

TEST(AnnotateLabelTool, AStyleThatCannotLabelTheObjectIsRefusedWhenItIsNamed)
{
    ToolDriver driver;
    standardDrawing(driver.document());
    select(driver.document(), {2});
    driver.start("annotate.label");
    (void)driver.type("S");
    const ToolStep wrongKind = driver.type("Point Number");
    EXPECT_EQ(wrongKind.outcome, Outcome::Rejected);
    // In the words LABEL refuses it with, and naming the object as LABEL does.
    CommandInterpreter interpreter(driver.document());
    const auto typed = interpreter.run("LABEL 2 style=\"Point Number\"");
    ASSERT_FALSE(typed.ok());
    EXPECT_EQ(wrongKind.message, typed.error().message + " (id=2)");
    EXPECT_TRUE(contains(typed.error().context, "id=2")) << typed.error().context;
    const ToolStep missing = driver.type("Nowhere");
    EXPECT_EQ(missing.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(missing.message, "there is no label style \"Nowhere\"; the drawing has "
                                          "Arc Data, Bearing Distance"))
        << missing.message;
    // Another case of a name the drawing has is that style.
    EXPECT_EQ(driver.type("bearing distance").outcome, Outcome::Continue);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(std::get<LabelGeometry>(labelsOf(driver.document())[0].geometry).style,
              "Bearing Distance");
}

TEST(AnnotateLabelTool, UndoTakesBackTheOptionsThenTheChoiceThenThePicks)
{
    ToolDriver driver;
    standardDrawing(driver.document());
    select(driver.document(), {});
    driver.start("annotate.label");
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected) << "nothing picked yet";
    (void)driver.pick(2, 5.0, 50.0);
    (void)driver.enter();
    (void)driver.type("T");
    (void)driver.type("EDGE");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point) << "the text is taken back first";
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection) << "then the choice";
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue) << "then the pick";
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected);
    // The text taken back is not on the label made after.
    (void)driver.pick(2, 5.0, 50.0);
    (void)driver.enter();
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(std::get<LabelGeometry>(labelsOf(driver.document())[0].geometry)
                    .textOverride.empty());
}

TEST(AnnotateLabelTool, AfterLabellingTheSelectionIsClearedAndEnterWithNothingChosenEndsIt)
{
    ToolDriver driver;
    standardDrawing(driver.document());
    select(driver.document(), {2});
    driver.start("annotate.label");
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.document().selection().ids().empty());
    EXPECT_FALSE(driver.finished()) << "it starts again";
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection)
        << "asking for the next objects, not labelling the same one again";
    const ToolStep end = driver.enter();
    EXPECT_EQ(end.outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(labelsOf(driver.document()).size(), 1u);
}

TEST(AnnotateLabelTool, ALabelThatCouldNotBeMadeIsRefusedWithTheVerbsReasonAndTheToolWaits)
{
    // No label styles at all.
    ToolDriver bare;
    run(bare.document(), "LINE 0,0 10,0");
    select(bare.document(), {1});
    bare.start("annotate.label");
    const ToolStep none = bare.enter();
    EXPECT_EQ(none.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(none.message, "no label style that can label this")) << none.message;

    // A point with no number: Point Number would say nothing, as LABEL says.
    ToolDriver driver;
    standardDrawing(driver.document());
    run(driver.document(), "POINT 5,5"); // id 3
    select(driver.document(), {3});
    driver.start("annotate.label");
    (void)driver.type("S");
    (void)driver.type("Point Number");
    const std::string prompt = driver.tool().prompt();
    const ToolStep silent = driver.enter();
    EXPECT_EQ(silent.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(silent.message, "the label would say nothing")) << silent.message;
    EXPECT_EQ(driver.tool().prompt(), prompt);
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(newest(driver.document()), 3u) << "nothing was made after the point";
}
