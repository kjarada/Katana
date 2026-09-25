// The Text tool's Style, Justify and Paper options, and the Multiline Text
// tool (src/katana_cad/tools/annotate_text.cpp, docs/annotation.md "Text"),
// driven through ToolDriver as the plan view drives them. What a tool makes
// is checked against what the TEXT and MTEXT verbs make of the same request,
// and every height against paper millimetres x scale / 1000 worked by hand.

#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"
#include "tool_driver.hpp"

namespace {

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::ToolInput;
using katana::cad::testing::ToolDriver;
using katana::entity::Entity;
using katana::entity::TextGeometry;
using katana::entity::TextJustify;
using katana::geometry::Point2;
using Outcome = katana::cad::ToolStep::Outcome;
namespace cmd = katana::commands;

std::vector<TextGeometry> texts(Document& document)
{
    std::vector<TextGeometry> out;
    document.model().entities.forEach([&](const Entity& entity) {
        if (const auto* text = std::get_if<TextGeometry>(&entity.geometry)) {
            out.push_back(*text);
        }
    });
    return out;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// A text style "Notes" 3.5 mm on paper, and the drawing at 1:500.
void notesAt500(Document& document)
{
    katana::entity::TextStyle notes;
    notes.name = "Notes";
    notes.paperHeight = 3.5;
    ASSERT_TRUE(document.execute(cmd::createTextStyle(notes)).ok());
    ASSERT_TRUE(document.setAnnotationScale(500.0).ok());
}

// What a verb line makes in a drawing set up the same way.
TextGeometry madeByVerb(const std::string& line)
{
    Document document;
    notesAt500(document);
    CommandInterpreter interpreter(document);
    const auto reply = interpreter.run(line);
    EXPECT_TRUE(reply.ok()) << line << ": " << reply.error().describe();
    const auto made = texts(document);
    return made.empty() ? TextGeometry{} : made.back();
}

} // namespace

TEST(AnnotateTextOptions, AStyleAndAJustificationMakeWhatTheTextVerbMakes)
{
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.text");
    EXPECT_TRUE(contains(driver.tool().prompt(), "[Style/Justify/Paper]"))
        << driver.tool().prompt();
    ASSERT_EQ(driver.type("S").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_TRUE(contains(driver.tool().prompt(), "text style")) << driver.tool().prompt();
    ASSERT_EQ(driver.type("Notes").outcome, Outcome::Continue);
    ASSERT_EQ(driver.type("j").outcome, Outcome::Continue);
    ASSERT_EQ(driver.type("MC").outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "style Notes, MC")) << driver.tool().prompt();

    // A paper-sized style: no height is asked, straight to the rotation.
    ASSERT_EQ(driver.click(10.0, 20.0).outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "rotation")) << driver.tool().prompt();
    (void)driver.type("0");
    (void)driver.type("Kerb");
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);

    const auto made = texts(driver.document());
    ASSERT_EQ(made.size(), 1u);
    EXPECT_EQ(made[0].style, "Notes");
    EXPECT_EQ(made[0].justify, TextJustify::MiddleCentre);
    EXPECT_EQ(made[0].paperHeight, 0.0) << "the style's paper height, not a copy of it";
    // 3.5 mm at 1:500 is 3.5 x 500 / 1000 = 1.75 m.
    EXPECT_DOUBLE_EQ(made[0].height, 1.75);
    EXPECT_EQ(made[0], madeByVerb("TEXT 10,20 \"Kerb\" style=Notes justify=MC"));
}

TEST(AnnotateTextOptions, APaperHeightSkipsTheHeightPromptAndIsTheVerbsPaper)
{
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.text");
    (void)driver.type("P");
    EXPECT_TRUE(contains(driver.tool().prompt(), "height on paper")) << driver.tool().prompt();
    ASSERT_EQ(driver.type("5").outcome, Outcome::Continue);
    (void)driver.click(0.0, 0.0);
    EXPECT_TRUE(contains(driver.tool().prompt(), "rotation")) << driver.tool().prompt();
    (void)driver.type("90");
    (void)driver.type("North");
    (void)driver.enter();
    const auto made = texts(driver.document());
    ASSERT_EQ(made.size(), 1u);
    EXPECT_EQ(made[0].style, "") << "no style was chosen";
    EXPECT_EQ(made[0].paperHeight, 5.0);
    // 5 mm at 1:500 is 2.5 m.
    EXPECT_DOUBLE_EQ(made[0].height, 2.5);
    EXPECT_EQ(made[0], madeByVerb("TEXT 0,0 1 \"North\" paper=5 rotation=90"));
}

TEST(AnnotateTextOptions, NoStyleAndNoPaperHeightAskTheHeightAsBefore)
{
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.text");
    (void)driver.type("S");
    (void)driver.type("Notes");
    (void)driver.type("S");
    EXPECT_TRUE(contains(driver.tool().prompt(), "<Notes>")) << driver.tool().prompt();
    ASSERT_EQ(driver.type(".").outcome, Outcome::Continue) << "a dot is no style";
    (void)driver.type("P");
    (void)driver.type("0");
    (void)driver.click(0.0, 0.0);
    EXPECT_TRUE(contains(driver.tool().prompt(), "Specify height")) << driver.tool().prompt();
    (void)driver.type("2");
    (void)driver.type("0");
    (void)driver.type("plain");
    (void)driver.enter();
    const auto made = texts(driver.document());
    ASSERT_EQ(made.size(), 1u);
    EXPECT_EQ(made[0], (TextGeometry{Point2(0.0, 0.0), "plain", 2.0, 0.0}));
}

TEST(AnnotateTextOptions, AnOptionItCannotTakeIsRefusedAndTheToolWaitsWhereItWas)
{
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.text");
    (void)driver.type("S");
    const std::string stylePrompt = driver.tool().prompt();
    const auto missing = driver.type("Nope");
    EXPECT_EQ(missing.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(missing.message, "no text style 'Nope'")) << missing.message;
    EXPECT_EQ(driver.tool().prompt(), stylePrompt);
    ASSERT_EQ(driver.enter().outcome, Outcome::Continue) << "Enter keeps the style shown";
    (void)driver.type("J");
    EXPECT_EQ(driver.type("XX").outcome, Outcome::Rejected);
    ASSERT_EQ(driver.undo().outcome, Outcome::Continue) << "back to the start point";
    (void)driver.type("P");
    EXPECT_EQ(driver.type("-1").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("tall").outcome, Outcome::Rejected);
    (void)driver.enter();
    EXPECT_TRUE(contains(driver.tool().prompt(), "start point")) << driver.tool().prompt();
    EXPECT_EQ(driver.executed(), 0);
}

TEST(AnnotateTextOptions, UndoFromAPaperSizedTextsRotationGoesBackToItsStartPoint)
{
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.text");
    (void)driver.type("S");
    (void)driver.type("Notes");
    (void)driver.click(1.0, 1.0);
    ASSERT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "start point"))
        << "it never asked for a height: " << driver.tool().prompt();
}

TEST(AnnotateTextOptions, TheNextTextIsOfferedTheChoicesTheLastWasMadeWith)
{
    // As the height is (annotate_text.cpp): read from the newest text, so
    // the tool that starts again after a text offers what that text has.
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.text");
    (void)driver.type("S");
    (void)driver.type("Notes");
    (void)driver.type("J");
    (void)driver.type("TR");
    (void)driver.click(0.0, 0.0);
    (void)driver.enter();
    (void)driver.type("first");
    ASSERT_TRUE(driver.enter().restart);
    EXPECT_TRUE(contains(driver.tool().prompt(), "style Notes, TR")) << driver.tool().prompt();
    // Enter continues below it in the same style: 1.75 x 5 / 3 below.
    (void)driver.enter();
    (void)driver.type("second");
    (void)driver.enter();
    const auto made = texts(driver.document());
    ASSERT_EQ(made.size(), 2u);
    EXPECT_EQ(made[1].style, "Notes");
    EXPECT_EQ(made[1].justify, TextJustify::TopRight);
    EXPECT_DOUBLE_EQ(made[1].position.y, -1.75 * 5.0 / 3.0);
}

TEST(AnnotateMultilineText, EveryLineIsOneEntityAsMtextMakesIt)
{
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.mtext");
    EXPECT_TRUE(contains(driver.tool().prompt(), "insertion point")) << driver.tool().prompt();
    EXPECT_TRUE(contains(driver.tool().prompt(), "style Standard")) << "paper-sized, as MTEXT is";
    ASSERT_EQ(driver.click(5.0, 5.0).outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "rotation")) << driver.tool().prompt();
    (void)driver.enter();
    (void)driver.type("PIT 12");
    (void)driver.type("IL 10.50");
    EXPECT_EQ(driver.executed(), 0);
    const auto done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(done.message, "a text of 2 lines");
    EXPECT_EQ(driver.executed(), 1) << "one step";

    const auto made = texts(driver.document());
    ASSERT_EQ(made.size(), 1u) << "one entity";
    EXPECT_EQ(made[0].text, "PIT 12\nIL 10.50");
    EXPECT_EQ(made[0].style, "Standard");
    // Standard is 2.5 mm on paper (annotation.hpp): 2.5 x 500 / 1000 = 1.25.
    EXPECT_DOUBLE_EQ(made[0].height, 1.25);
    EXPECT_EQ(made[0], madeByVerb("MTEXT 5,5 \"PIT 12\\nIL 10.50\""));

    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(texts(driver.document()).empty());
}

TEST(AnnotateMultilineText, ItTakesTheOptionsAndPreviewsTheBlockAsOneText)
{
    ToolDriver driver;
    notesAt500(driver.document());
    driver.start("annotate.mtext");
    (void)driver.type("S");
    (void)driver.type("Notes");
    (void)driver.type("J");
    (void)driver.type("TL");
    (void)driver.click(0.0, 10.0);
    (void)driver.type("30");
    (void)driver.type("one");
    (void)driver.type("two");
    const auto preview = driver.tool().preview(Point2(50.0, 50.0));
    ASSERT_EQ(preview.shapes.size(), 1u);
    const auto& block = std::get<TextGeometry>(preview.shapes[0]);
    EXPECT_EQ(block.text, "one\ntwo");
    EXPECT_EQ(block.justify, TextJustify::TopLeft);
    (void)driver.enter();
    EXPECT_EQ(texts(driver.document()).back(),
              madeByVerb("MTEXT 0,10 \"one\\ntwo\" style=Notes justify=TL rotation=30"));
}

TEST(AnnotateMultilineText, EnterAtItsFirstPromptEndsTheToolAndItsAliasesStartIt)
{
    ToolDriver driver;
    // Even with a text in the drawing: Multiline Text does not continue one.
    (void)driver.add(cmd::createText({Point2(0.0, 0.0), "above", 3.0, 0.0}));
    driver.start("annotate.mtext");
    const auto step = driver.enter();
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.command, nullptr);

    const auto& catalog = katana::cad::toolCatalog();
    for (const char* alias : {"MTEXT", "MT"}) {
        const katana::cad::ToolInfo* info = catalog.findByAlias(alias);
        ASSERT_NE(info, nullptr) << alias;
        EXPECT_EQ(info->id, "annotate.mtext");
        EXPECT_EQ(info->group, "Text");
    }
}
