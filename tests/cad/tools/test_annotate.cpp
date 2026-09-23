// The Annotate family: Text, Linear and Aligned Dimension, and Leader, driven
// through ToolDriver as the plan view drives them. Every expected value is
// worked out by hand in the comment beside it; inputs are chosen exact in
// binary wherever the geometry allows, and where it does not (a 3-4-5
// direction, an angle through pi) the tolerance says why.

#include <cmath>
#include <string>
#include <utility>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/dimension_draw.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/tables.hpp"
#include "katana/math/numerics.hpp"
#include "tool_driver.hpp"

namespace {

using katana::cad::ToolFeedback;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::DimensionGeometry;
using katana::entity::Entity;
using katana::entity::TextGeometry;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using Outcome = katana::cad::ToolStep::Outcome;
namespace cmd = katana::commands;

// Every entity in the drawing, in id order - which is creation order.
std::vector<Entity> drawing(ToolDriver& driver)
{
    std::vector<Entity> out;
    driver.document().model().entities.forEach([&](const Entity& entity) { out.push_back(entity); });
    return out;
}

template <class Geometry>
std::vector<Geometry> shapesOf(ToolDriver& driver)
{
    std::vector<Geometry> out;
    for (const Entity& entity : drawing(driver)) {
        if (const auto* geometry = std::get_if<Geometry>(&entity.geometry)) {
            out.push_back(*geometry);
        }
    }
    return out;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

// ---- the catalogue -----------------------------------------------------------------

TEST(AnnotateTools, EachToolIsInTheAnnotateMenuUnderItsAutoCadCommandNames)
{
    const auto& catalog = katana::cad::toolCatalog();
    const std::vector<std::pair<std::string, std::vector<std::string>>> expected = {
        {"annotate.text", {"TEXT", "DTEXT", "DT"}},
        {"annotate.dimlinear", {"DIMLINEAR", "DLI"}},
        {"annotate.dimaligned", {"DIMALIGNED", "DAL"}},
        {"annotate.leader", {"LEADER", "LEAD", "LE"}},
    };
    for (const auto& [id, aliases] : expected) {
        const katana::cad::ToolInfo* info = catalog.find(id);
        ASSERT_NE(info, nullptr) << id;
        EXPECT_EQ(info->category, "Annotate") << id;
        EXPECT_FALSE(info->tip.empty()) << id;
        for (const std::string& alias : aliases) {
            const katana::cad::ToolInfo* byAlias = catalog.findByAlias(alias);
            ASSERT_NE(byAlias, nullptr) << alias;
            EXPECT_EQ(byAlias->id, id) << alias;
        }
    }
}

// ---- Text --------------------------------------------------------------------------

TEST(AnnotateText, ATypedHeightAndRotationPlaceOneLineAtTheStartPoint)
{
    ToolDriver driver;
    driver.start("annotate.text");
    EXPECT_EQ(driver.click(10.0, 20.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("3").outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("0").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_EQ(driver.type("Hello").outcome, Outcome::Continue);
    EXPECT_EQ(driver.executed(), 0) << "nothing is committed until the text is finished";
    const ToolStep done = driver.enter();
    EXPECT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(driver.executed(), 1);

    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0], (TextGeometry{Point2(10.0, 20.0), "Hello", 3.0, 0.0}));
    EXPECT_EQ(drawing(driver)[0].layer, "0") << "made on the current layer";
}

TEST(AnnotateText, EachLineGoesBelowTheLastByFiveThirdsOfTheHeightAndOneUndoRemovesThemAll)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 20.0);
    (void)driver.type("3");
    (void)driver.type("0");
    (void)driver.type("first");
    (void)driver.type("second");
    (void)driver.type("third");
    const ToolStep done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(done.message, "3 lines of text");
    EXPECT_EQ(driver.executed(), 1) << "three lines, one command";

    // Spacing 3 * 5 / 3 = 5, straight down for unrotated text: y = 20, 15, 10.
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 3u);
    EXPECT_EQ(texts[0].position, Point2(0.0, 20.0));
    EXPECT_EQ(texts[1].position, Point2(0.0, 15.0));
    EXPECT_EQ(texts[2].position, Point2(0.0, 10.0));
    EXPECT_EQ(texts[2].text, "third");

    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(drawing(driver).empty());
}

TEST(AnnotateText, APickedHeightIsTheDistanceFromTheStartPoint)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 0.0);
    // (3, 4) is 5 from the origin.
    (void)driver.click(3.0, 4.0);
    (void)driver.enter(); // rotation: the default, 0
    (void)driver.type("A");
    (void)driver.enter();
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0].height, 5.0);
    EXPECT_EQ(texts[0].rotation, 0.0);
}

TEST(AnnotateText, RelativeAndPolarInputMeasureFromTheStartPoint)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.type("10,20");
    // @0,3 is (10, 23): a height of 3.
    (void)driver.type("@0,3");
    // @4<90 is straight up from the start: a rotation of 90 degrees. The
    // polar point's x is 10 + 4 cos(pi/2), and 4 cos(pi/2) is 2.4e-16 rather
    // than 0 - below half an ulp of 10 today, but the test should not rest on
    // that rounding, so the angle is compared to within 1e-15.
    (void)driver.type("@4<90");
    (void)driver.type("north");
    (void)driver.enter();
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0].position, Point2(10.0, 20.0));
    EXPECT_EQ(texts[0].height, 3.0);
    EXPECT_NEAR(texts[0].rotation, katana::math::kHalfPi, 1e-15);
}

TEST(AnnotateText, ARotatedTextsNextLineIsBelowItAsItReads)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 0.0);
    (void)driver.type("3");
    // A picked point straight above: atan2(7, 0) is exactly the double
    // nearest pi/2.
    (void)driver.click(0.0, 7.0);
    (void)driver.type("one");
    (void)driver.type("two");
    (void)driver.enter();
    // Text reading up the page has "below" to its right: (0, -1) turned 90
    // degrees is (1, 0), so the second line is 5 along +x. sin(pi/2) is
    // exactly 1 in binary; cos(pi/2) is 6e-17, hence the tolerance on y.
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 2u);
    EXPECT_EQ(texts[0].rotation, katana::math::kHalfPi);
    EXPECT_EQ(texts[1].position.x, 5.0);
    EXPECT_NEAR(texts[1].position.y, 0.0, 1e-12);
}

TEST(AnnotateText, ATypedRotationIsInDegreesCounterClockwiseFromEast)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 0.0);
    (void)driver.type("3");
    (void)driver.type("90");
    (void)driver.type("up");
    (void)driver.type("next");
    (void)driver.enter();
    (void)driver.click(20.0, 0.0);
    (void)driver.type("3");
    (void)driver.type("-90");
    (void)driver.type("down");
    (void)driver.type("next");
    (void)driver.enter();
    // 90 degrees counter-clockwise is pi/2: the text reads up the page, so
    // "below" is +x and its second line is one spacing (3 x 5 / 3 = 5) along
    // +x, at (5, 0). -90 is -pi/2, clockwise: it reads down the page, "below"
    // is -x, and the second line is at 20 - 5 = 15. 90 x (pi / 180) may land
    // an ulp from the double nearest pi/2, hence 1e-15 on the angles; cos of
    // either is about 6e-17 rather than 0, hence the tolerance on y.
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 4u);
    EXPECT_NEAR(texts[0].rotation, katana::math::kHalfPi, 1e-15);
    EXPECT_NEAR(texts[1].position.x, 5.0, 1e-12);
    EXPECT_NEAR(texts[1].position.y, 0.0, 1e-12);
    EXPECT_NEAR(texts[2].rotation, -katana::math::kHalfPi, 1e-15);
    EXPECT_NEAR(texts[3].position.x, 15.0, 1e-12);
    EXPECT_NEAR(texts[3].position.y, 0.0, 1e-12);
}

TEST(AnnotateText, TheHeightOfferedIs2Point5WhenTheDrawingHasNoText)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(1.0, 2.0);
    EXPECT_TRUE(contains(driver.tool().prompt(), "<2.5>")) << driver.tool().prompt();
    (void)driver.enter();
    EXPECT_TRUE(contains(driver.tool().prompt(), "<0>")) << driver.tool().prompt();
    (void)driver.enter();
    (void)driver.type("x");
    (void)driver.enter();
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0].height, 2.5);
    EXPECT_EQ(texts[0].rotation, 0.0);
}

TEST(AnnotateText, TheHeightAndRotationOfferedAreTheNewestTextsInTheDrawing)
{
    ToolDriver driver;
    (void)driver.add(cmd::createText({Point2(0.0, 0.0), "old", 7.0, 0.0}));
    // 0.5 rad is 28.6479 degrees; the prompt shows it to nine decimals.
    (void)driver.add(cmd::createText({Point2(0.0, 50.0), "new", 4.0, 0.5}));
    driver.start("annotate.text");
    (void)driver.click(100.0, 0.0);
    EXPECT_TRUE(contains(driver.tool().prompt(), "<4>")) << driver.tool().prompt();
    (void)driver.enter();
    (void)driver.enter();
    (void)driver.type("same again");
    (void)driver.enter();
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 3u);
    EXPECT_EQ(texts[2].height, 4.0);
    EXPECT_EQ(texts[2].rotation, 0.5);
}

TEST(AnnotateText, EnterAtTheFirstPromptContinuesBelowTheNewestText)
{
    ToolDriver driver;
    (void)driver.add(cmd::createText({Point2(2.0, 10.0), "above", 3.0, 0.0}));
    driver.start("annotate.text");
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value) << "straight to the text";
    (void)driver.type("below");
    (void)driver.enter();
    // One line spacing under (2, 10) at height 3: y = 10 - 5 = 5.
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 2u);
    EXPECT_EQ(texts[1], (TextGeometry{Point2(2.0, 5.0), "below", 3.0, 0.0}));
}

TEST(AnnotateText, EnterAtTheFirstPromptWithNoTextInTheDrawingEndsTheTool)
{
    ToolDriver driver;
    driver.start("annotate.text");
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.command, nullptr);
    EXPECT_TRUE(driver.finished());
}

TEST(AnnotateText, EnterWithNoTextTypedEndsTheToolWithoutACommand)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 0.0);
    (void)driver.enter();
    (void)driver.enter();
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_TRUE(driver.finished());
}

TEST(AnnotateText, TheToolStartsAgainAfterEachText)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 0.0);
    (void)driver.enter();
    (void)driver.enter();
    (void)driver.type("one");
    const ToolStep done = driver.enter();
    EXPECT_TRUE(done.restart);
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_TRUE(contains(driver.tool().prompt(), "start point")) << driver.tool().prompt();
}

TEST(AnnotateText, TypedTextKeepsItsCommasAndTheLetterUIsTextNotUndo)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 0.0);
    (void)driver.enter();
    (void)driver.enter();
    EXPECT_EQ(driver.type("Road, north side").outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("U").outcome, Outcome::Continue);
    (void)driver.enter();
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 2u);
    EXPECT_EQ(texts[0].text, "Road, north side");
    EXPECT_EQ(texts[1].text, "U");
}

TEST(AnnotateText, UndoStepsBackOneInputAtATime)
{
    ToolDriver driver;
    driver.start("annotate.text");
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected) << "nothing to undo yet";
    (void)driver.click(0.0, 0.0);
    (void)driver.type("3");
    // Back to the height prompt, then a different height.
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "height")) << driver.tool().prompt();
    (void)driver.type("4");
    // U typed at the rotation prompt is Undo as well: back to the height.
    EXPECT_EQ(driver.type("u").outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "height")) << driver.tool().prompt();
    (void)driver.type("4");
    (void)driver.type("0");
    (void)driver.type("kept");
    (void)driver.type("dropped");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    (void)driver.enter();
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0], (TextGeometry{Point2(0.0, 0.0), "kept", 4.0, 0.0}));
}

TEST(AnnotateText, BadInputIsRefusedWithAReasonAndTheToolStaysWhereItWas)
{
    ToolDriver driver;
    driver.start("annotate.text");
    EXPECT_EQ(driver.type("5").outcome, Outcome::Rejected) << "a number is not a start point";
    (void)driver.click(1.0, 1.0);
    const std::string heightPrompt = driver.tool().prompt();
    for (const char* bad : {"0", "-2", "abc"}) {
        const ToolStep step = driver.type(bad);
        EXPECT_EQ(step.outcome, Outcome::Rejected) << bad;
        EXPECT_FALSE(step.message.empty()) << bad;
        EXPECT_EQ(driver.tool().prompt(), heightPrompt) << bad;
    }
    (void)driver.type("2");
    const std::string rotationPrompt = driver.tool().prompt();
    EXPECT_NE(rotationPrompt, heightPrompt);
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected) << "no direction";
    EXPECT_EQ(driver.type("north").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), rotationPrompt);
    (void)driver.enter();
    EXPECT_EQ(driver.type("\xff\xfe").outcome, Outcome::Rejected) << "not UTF-8";
    EXPECT_EQ(driver.executed(), 0);

    // And a picked height of zero, back at the height prompt.
    (void)driver.undo();
    (void)driver.undo();
    EXPECT_EQ(driver.tool().prompt(), heightPrompt);
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected) << "zero height";
    EXPECT_EQ(driver.tool().prompt(), heightPrompt);
}

TEST(AnnotateText, ThePreviewShowsTheRubberBandThenTheLinesTypedAndWhereTheNextGoes)
{
    ToolDriver driver;
    driver.start("annotate.text");
    (void)driver.click(0.0, 0.0);
    const ToolFeedback height = driver.tool().preview({0.0, 3.0});
    ASSERT_EQ(height.shapes.size(), 1u);
    EXPECT_EQ(std::get<Segment2>(height.shapes[0]), (Segment2{Point2(0.0, 0.0), Point2(0.0, 3.0)}));
    (void)driver.type("3");
    (void)driver.type("0");
    (void)driver.type("typed");
    const ToolFeedback content = driver.tool().preview({50.0, 50.0});
    ASSERT_EQ(content.shapes.size(), 1u);
    EXPECT_EQ(std::get<TextGeometry>(content.shapes[0]),
              (TextGeometry{Point2(0.0, 0.0), "typed", 3.0, 0.0}));
    ASSERT_EQ(content.markers.size(), 1u);
    EXPECT_EQ(content.markers[0], Point2(0.0, -5.0)) << "the next line, 5 below";
}

// ---- Aligned dimension ---------------------------------------------------------------

TEST(AnnotateAlignedDimension, ItMeasuresTheTrueDistanceWithTheOffsetPositiveToTheLeft)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    // The origins are 10 apart along (0.8, 0.6); left of that is (-0.6, 0.8),
    // and (-3, 4) . (-0.6, 0.8) = 1.8 + 3.2 = 5. 0.6 and 0.8 are not exact in
    // binary, so the offset is compared to within rounding.
    const ToolStep done = driver.click(-3.0, 4.0);
    ASSERT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(done.message, "aligned dimension measuring 10");
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0].start, Point2(0.0, 0.0));
    EXPECT_EQ(dimensions[0].end, Point2(8.0, 6.0));
    EXPECT_NEAR(dimensions[0].offset, 5.0, 1e-12);
    EXPECT_EQ(dimensions[0].measurement(), 10.0);
    EXPECT_TRUE(dimensions[0].textOverride.empty());

    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(drawing(driver).empty());
}

TEST(AnnotateAlignedDimension, TheOffsetIsNegativeOnTheRightOfFirstToSecond)
{
    // Heading west from (10, 0) to (0, 0), left is south. Unit direction
    // (-1, 0) and its left normal (0, -1) are exact.
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 0.0);
    (void)driver.click(5.0, -3.0); // south: left, +3
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 0.0);
    (void)driver.click(5.0, 3.0); // north: right, -3
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 2u);
    EXPECT_EQ(dimensions[0].offset, 3.0);
    EXPECT_EQ(dimensions[1].offset, -3.0);
}

TEST(AnnotateAlignedDimension, TypedOriginsAndATypedOffsetPlaceItExactly)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.type("0,0");
    (void)driver.type("@10,0");
    const ToolStep done = driver.type("2.5");
    ASSERT_EQ(done.outcome, Outcome::Done);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(10.0, 0.0), 2.5, ""}));
}

TEST(AnnotateAlignedDimension, PolarAndRelativeInputMeasureFromTheLastOrigin)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.type("2,2");
    // @5<0 is 5 east of (2, 2): cos 0 and sin 0 are exact, so (7, 2).
    (void)driver.type("@5<0");
    // @0,3 from the second origin is (7, 5); heading east, left is north,
    // so the offset is 5 - 2 = 3.
    (void)driver.type("@0,3");
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(2.0, 2.0), Point2(7.0, 2.0), 3.0, ""}));
}

TEST(AnnotateAlignedDimension, TheTextOptionReplacesTheLabelAndEnterRestoresTheMeasurement)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 0.0);
    EXPECT_EQ(driver.type("T").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    (void)driver.type("12.50 m, approx");
    (void)driver.click(2.0, 1.0);
    // The second dimension is given a text and then, at the text prompt
    // again, Enter - which must take that text away, not keep it.
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 0.0);
    (void)driver.type("T");
    (void)driver.type("wrong");
    (void)driver.type("text");
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    (void)driver.click(2.0, 1.0);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 2u);
    EXPECT_EQ(dimensions[0].textOverride, "12.50 m, approx");
    EXPECT_TRUE(dimensions[1].textOverride.empty()) << dimensions[1].textOverride;
}

TEST(AnnotateAlignedDimension, UndoAfterEnterClearedTheTextBringsTheTextBack)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 0.0);
    (void)driver.type("T");
    (void)driver.type("kept");
    (void)driver.type("T");
    (void)driver.enter(); // back to the measured distance...
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue); // ...and back to "kept"
    (void)driver.click(2.0, 1.0);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0].textOverride, "kept");
}

TEST(AnnotateAlignedDimension, UndoTakesBackTheTextOptionFirstThenTheSecondOrigin)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected);
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 0.0);
    (void)driver.type("T");
    (void)driver.type("wrong");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue); // the text
    EXPECT_TRUE(contains(driver.tool().prompt(), "dimension line")) << driver.tool().prompt();
    EXPECT_EQ(driver.type("U").outcome, Outcome::Continue); // the second origin
    EXPECT_TRUE(contains(driver.tool().prompt(), "second")) << driver.tool().prompt();
    (void)driver.click(0.0, 6.0);
    (void)driver.click(-2.0, 3.0);
    // (0,0)->(0,6) heads north; left is west, so x = -2 is offset +2.
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(0.0, 6.0), 2.0, ""}));
}

TEST(AnnotateAlignedDimension, CoincidentOriginsAndMissingInputAreRefused)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(3.0, 3.0);
    const std::string second = driver.tool().prompt();
    const ToolStep same = driver.click(3.0, 3.0);
    EXPECT_EQ(same.outcome, Outcome::Rejected);
    EXPECT_FALSE(same.message.empty());
    EXPECT_EQ(driver.tool().prompt(), second);
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("far").outcome, Outcome::Rejected);
    (void)driver.click(5.0, 3.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("north").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.executed(), 0);
}

TEST(AnnotateAlignedDimension, EnterPicksALineOrThePolylineSegmentNearestThePick)
{
    ToolDriver driver;
    const auto line = driver.add(cmd::createLine(Point2(0.0, 0.0), Point2(6.0, 0.0)));
    const auto polyline = driver.add(cmd::createPolyline(
        Polyline2{{Point2(10.0, 0.0), Point2(14.0, 0.0), Point2(14.0, 3.0)}, false}));
    const auto circle = driver.add(cmd::createCircle(Point2(30.0, 0.0), 2.0));
    driver.start("annotate.dimaligned");
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.pick(circle, 32.0, 0.0).outcome, Outcome::Rejected) << "not a line";
    (void)driver.pick(line, 3.0, 0.0);
    (void)driver.click(3.0, 2.0);
    // The polyline's second segment, (14,0)->(14,3), is nearest (14, 1.5).
    (void)driver.enter();
    (void)driver.pick(polyline, 14.0, 1.5);
    // Heading north, left is west: x = 15 is 1 to the right, offset -1.
    (void)driver.click(15.0, 1.5);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 2u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(6.0, 0.0), 2.0, ""}));
    EXPECT_EQ(dimensions[1], (DimensionGeometry{Point2(14.0, 0.0), Point2(14.0, 3.0), -1.0, ""}));
}

TEST(AnnotateAlignedDimension, EnterWhilePickingGoesBackToPickingOrigins)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.enter();
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.type("0,0").outcome, Outcome::Rejected) << "a pick is wanted";
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_TRUE(contains(driver.tool().prompt(), "first extension line")) << driver.tool().prompt();
}

TEST(AnnotateAlignedDimension, TextThatIsNotUtf8IsRefusedAtTheTextPrompt)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 0.0);
    (void)driver.type("T");
    // A lone 0xFF byte: never valid anywhere in UTF-8.
    const ToolStep step = driver.type("\xff");
    EXPECT_EQ(step.outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value) << "still at the text prompt";
}

TEST(AnnotateAlignedDimension, UndoAfterPickingALineGoesBackToPicking)
{
    ToolDriver driver;
    const auto line = driver.add(cmd::createLine(Point2(0.0, 0.0), Point2(6.0, 0.0)));
    driver.start("annotate.dimaligned");
    (void)driver.enter();
    (void)driver.pick(line, 3.0, 0.0);
    (void)driver.undo();
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
}

TEST(AnnotateAlignedDimension, ThePreviewIsTheDimensionAClickThereWouldMake)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(0.0, 0.0);
    const ToolFeedback band = driver.tool().preview({4.0, 0.0});
    ASSERT_EQ(band.shapes.size(), 1u);
    EXPECT_EQ(std::get<Segment2>(band.shapes[0]), (Segment2{Point2(0.0, 0.0), Point2(4.0, 0.0)}));
    (void)driver.click(4.0, 0.0);
    const ToolFeedback feedback = driver.tool().preview({2.0, 1.5});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(std::get<DimensionGeometry>(feedback.shapes[0]),
              (DimensionGeometry{Point2(0.0, 0.0), Point2(4.0, 0.0), 1.5, ""}));
}

TEST(AnnotateAlignedDimension, TheToolStartsAgainAfterEachDimension)
{
    ToolDriver driver;
    driver.start("annotate.dimaligned");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 0.0);
    EXPECT_TRUE(driver.click(2.0, 1.0).restart);
    EXPECT_FALSE(driver.finished());
    EXPECT_TRUE(contains(driver.tool().prompt(), "first extension line")) << driver.tool().prompt();
}

// ---- Linear dimension ----------------------------------------------------------------

TEST(AnnotateLinearDimension, ALineAboveTheOriginsMeasuresTheirHorizontalDistance)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    // (4, 10) is above both: horizontal. The origin farther from y = 10 is
    // (0, 0), so both project onto y = 0: start (0, 0), end (8, 0), which
    // measures 8; heading east, left is north, and y = 10 is offset +10.
    const ToolStep done = driver.click(4.0, 10.0);
    ASSERT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(done.message, "linear dimension measuring 8");
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(8.0, 0.0), 10.0, ""}));
    EXPECT_EQ(dimensions[0].measurement(), 8.0);

    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(drawing(driver).empty());
}

TEST(AnnotateLinearDimension, ALineBesideTheOriginsMeasuresTheirVerticalDistance)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    // (12, 3) is right of both: vertical. The origin farther from x = 12 is
    // (0, 0), so both project onto x = 0: start (0, 0), end (0, 6), which
    // measures 6; heading north, left is west, and x = 12 is offset -12.
    (void)driver.click(12.0, 3.0);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(0.0, 6.0), -12.0, ""}));
    EXPECT_EQ(dimensions[0].measurement(), 6.0);
}

TEST(AnnotateLinearDimension, APickedSegmentAndATypedLocationGiveTheSameProjection)
{
    ToolDriver driver;
    const auto line = driver.add(cmd::createLine(Point2(0.0, 0.0), Point2(6.0, 4.0)));
    driver.start("annotate.dimlinear");
    (void)driver.enter();
    (void)driver.pick(line, 3.0, 2.0);
    // (3, 9) is above both ends: horizontal. (0, 0) is 9 from y = 9 and
    // (6, 4) only 5, so both project onto y = 0: (0, 0) to (6, 0), offset 9.
    (void)driver.type("3,9");
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(6.0, 0.0), 9.0, ""}));
    EXPECT_EQ(dimensions[0].measurement(), 6.0);
}

TEST(AnnotateLinearDimension, TheNearerOriginLiesOnItsExtensionLineWhenTheLevelsDifferByMoreThanDimexo)
{
    // The dimension of the first test, drawn: the extension lines run from
    // DIMEXO above (0, 0) and (8, 0) up past y = 10. The levels differ by 6,
    // more than DIMEXO, so the second origin, (8, 6), is on its line, which
    // carries on below it for 6 - 0.625 = 5.375 - the price of projecting
    // both origins onto the farther one's level.
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    (void)driver.click(4.0, 10.0);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    const katana::entity::DimensionStyle style;
    const auto drawn = katana::cad::buildDimension(dimensions[0], style);
    ASSERT_EQ(drawn.extensionLines.size(), 2u);
    // Standard style: DIMEXO 0.625, DIMEXE 1.25.
    EXPECT_EQ(drawn.extensionLines[0], (Segment2{Point2(0.0, 0.625), Point2(0.0, 11.25)}));
    EXPECT_EQ(drawn.extensionLines[1], (Segment2{Point2(8.0, 0.625), Point2(8.0, 11.25)}));
    EXPECT_EQ(drawn.extensionLines[1].distanceTo(Point2(8.0, 6.0)), 0.0);
}

TEST(AnnotateLinearDimension, WhenTheLevelsDifferByLessThanDimexoTheNearerOriginIsInTheGapBelowItsLine)
{
    // (0, 0) and (8, 0.25) with the line at y = 10: (0, 0) is farther, so
    // both project onto y = 0 and the second extension line starts DIMEXO
    // up, at (8, 0.625). The origin (8, 0.25) is 0.625 - 0.25 = 0.375 short
    // of it: in the gap every extension line leaves at its foot, as the
    // farther origin always is, not on the line. All of it exact in binary.
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 0.25);
    (void)driver.click(4.0, 10.0);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(8.0, 0.0), 10.0, ""}));
    const auto drawn = katana::cad::buildDimension(dimensions[0], katana::entity::DimensionStyle{});
    ASSERT_EQ(drawn.extensionLines.size(), 2u);
    EXPECT_EQ(drawn.extensionLines[1], (Segment2{Point2(8.0, 0.625), Point2(8.0, 11.25)}));
    EXPECT_EQ(drawn.extensionLines[1].distanceTo(Point2(8.0, 0.25)), 0.375);
}

TEST(AnnotateLinearDimension, HAndVChooseTheOrientationWhereverTheLineIsPlaced)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    // (14, 10) is off the corner, 6 out in x and 4 in y - vertical by
    // default - but H makes it horizontal: y = 10, projected onto the farther
    // level y = 0, measuring 8.
    EXPECT_EQ(driver.type("h").outcome, Outcome::Continue);
    (void)driver.click(14.0, 10.0);
    // V with the line above and left, at (-2, 10): the farther origin in x
    // is (8, 6), so both project onto x = 8: start (8, 0), end (8, 6),
    // heading north with west on the left, and x = -2 is offset +10.
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    EXPECT_EQ(driver.type("Vertical").outcome, Outcome::Continue);
    (void)driver.click(-2.0, 10.0);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 2u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(8.0, 0.0), 10.0, ""}));
    EXPECT_EQ(dimensions[1], (DimensionGeometry{Point2(8.0, 0.0), Point2(8.0, 6.0), 10.0, ""}));
    EXPECT_EQ(dimensions[1].measurement(), 6.0);
}

TEST(AnnotateLinearDimension, UndoTakesBackAChosenOrientation)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    (void)driver.type("H");
    (void)driver.undo();
    // Back to choosing by position: (12, 3) is beside, so vertical.
    (void)driver.click(12.0, 3.0);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0].measurement(), 6.0);
}

TEST(AnnotateLinearDimension, PlacementsThisModelCannotDrawAreRefusedWithAReason)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    const std::string location = driver.tool().prompt();
    // Inside the rectangle the origins span: horizontal or vertical?
    const ToolStep inside = driver.click(4.0, 3.0);
    EXPECT_EQ(inside.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(inside.message, "H or V")) << inside.message;
    EXPECT_EQ(driver.tool().prompt(), location);
    // Horizontal, but at y = 3, between the levels 0 and 6.
    (void)driver.type("H");
    const std::string horizontal = driver.tool().prompt();
    EXPECT_TRUE(contains(horizontal, "horizontal")) << horizontal;
    const ToolStep between = driver.click(12.0, 3.0);
    EXPECT_EQ(between.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(between.message, "between")) << between.message;
    EXPECT_EQ(driver.type("2.5").outcome, Outcome::Rejected) << "no typed offset for linear";
    EXPECT_EQ(driver.tool().prompt(), horizontal);
    EXPECT_EQ(driver.executed(), 0);
}

TEST(AnnotateLinearDimension, AnOrientationThatWouldMeasureNothingIsRefused)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    // One above the other: a horizontal dimension of them is zero.
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, 5.0);
    const ToolStep zero = driver.click(0.0, 9.0);
    EXPECT_EQ(zero.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(zero.message, "measure nothing")) << zero.message;
    // Beside them it is vertical, 5, projected onto x = 0 (a tie keeps the
    // first); heading north, x = 3 is 3 to the right, offset -3.
    EXPECT_EQ(driver.click(3.0, 2.0).outcome, Outcome::Done);
    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 1u);
    EXPECT_EQ(dimensions[0], (DimensionGeometry{Point2(0.0, 0.0), Point2(0.0, 5.0), -3.0, ""}));
}

TEST(AnnotateLinearDimension, OffTheCornerOfLevelOriginsTheOrientationThatMeasuresSomethingIsChosen)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    // A level pair, as a horizontal edge gives: (15, 3) is 5 past its end
    // and 3 above it. Farther out in x would mean vertical, which measures
    // nothing here, but the line IS above the origins, so it is horizontal:
    // start (0, 0), end (10, 0) (a tie of levels keeps the first), measuring
    // 10; heading east, left is north, so y = 3 is offset +3.
    (void)driver.click(0.0, 0.0);
    (void)driver.click(10.0, 0.0);
    const DimensionGeometry across{Point2(0.0, 0.0), Point2(10.0, 0.0), 3.0, ""};
    const ToolFeedback feedback = driver.tool().preview({15.0, 3.0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(std::get<DimensionGeometry>(feedback.shapes[0]), across);
    // Level with the origins, past the end, neither orientation measures
    // anything from there: still refused.
    EXPECT_EQ(driver.click(15.0, 0.0).outcome, Outcome::Rejected);
    // A forced V is not overridden: it is what the user asked for.
    (void)driver.type("V");
    EXPECT_EQ(driver.click(15.0, 3.0).outcome, Outcome::Rejected);
    (void)driver.undo();
    const ToolStep done = driver.click(15.0, 3.0);
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_EQ(done.message, "linear dimension measuring 10");

    // One above the other, the same the other way: (2, 9) is 4 above and 2
    // beside, but horizontal would measure nothing, so vertical: start
    // (0, 0), end (0, 5); heading north, left is west, so x = 2 is -2.
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, 5.0);
    EXPECT_EQ(driver.click(2.0, 9.0).outcome, Outcome::Done);

    const auto dimensions = shapesOf<DimensionGeometry>(driver);
    ASSERT_EQ(dimensions.size(), 2u);
    EXPECT_EQ(dimensions[0], across);
    EXPECT_EQ(dimensions[1], (DimensionGeometry{Point2(0.0, 0.0), Point2(0.0, 5.0), -2.0, ""}));
}

TEST(AnnotateLinearDimension, ThePreviewFollowsTheOrientationTheCursorChooses)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    const ToolFeedback above = driver.tool().preview({4.0, 10.0});
    ASSERT_EQ(above.shapes.size(), 1u);
    EXPECT_EQ(std::get<DimensionGeometry>(above.shapes[0]),
              (DimensionGeometry{Point2(0.0, 0.0), Point2(8.0, 0.0), 10.0, ""}));
    // Inside the rectangle there is no dimension to show, only the origins.
    const ToolFeedback inside = driver.tool().preview({4.0, 3.0});
    EXPECT_TRUE(inside.shapes.empty());
    EXPECT_EQ(inside.markers.size(), 2u);
}

// ---- Leader --------------------------------------------------------------------------

TEST(AnnotateLeader, ALeaderIsItsLineItsArrowheadAndItsNoteInOneCommand)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    (void)driver.type("AB");
    const ToolStep done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(done.message, "leader with 1 line of text");
    EXPECT_EQ(driver.executed(), 1);

    const auto entities = drawing(driver);
    ASSERT_EQ(entities.size(), 3u);
    // The last segment rises at atan(6/8) = 36.9 degrees, more than 15, so a
    // hook one arrowhead (2.5) long runs right from (8, 6) to (10.5, 6).
    EXPECT_EQ(std::get<Polyline2>(entities[0].geometry),
              (Polyline2{{Point2(0.0, 0.0), Point2(8.0, 6.0), Point2(10.5, 6.0)}, false}));
    // Standard style's closed arrow, 2.5 long and 2.5/3 each side: pointing
    // along (-0.8, -0.6), its back is (0,0) + (0.8, 0.6) * 2.5 = (2, 1.5); the
    // left normal (0.6, -0.8) times 2.5/3 is (0.5, -2/3), so its corners are
    // (2.5, 1.5 - 2/3) and (1.5, 1.5 + 2/3). 0.8 and 0.6 are not exact in
    // binary: compared to within rounding.
    const auto& head = std::get<Polyline2>(entities[1].geometry);
    EXPECT_TRUE(head.closed);
    ASSERT_EQ(head.vertices.size(), 3u);
    EXPECT_EQ(head.vertices[0], Point2(0.0, 0.0));
    EXPECT_NEAR(head.vertices[1].x, 2.5, 1e-12);
    EXPECT_NEAR(head.vertices[1].y, 1.5 - 2.0 / 3.0, 1e-12);
    EXPECT_NEAR(head.vertices[2].x, 1.5, 1e-12);
    EXPECT_NEAR(head.vertices[2].y, 1.5 + 2.0 / 3.0, 1e-12);
    // The note stands DIMGAP 0.625 right of the hook's end, its first line's
    // middle level with it: baseline 6 - 2.5 / 2 = 4.75.
    EXPECT_EQ(std::get<TextGeometry>(entities[2].geometry),
              (TextGeometry{Point2(11.125, 4.75), "AB", 2.5, 0.0}));

    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(drawing(driver).empty()) << "one undo removes line, arrow and note";
}

TEST(AnnotateLeader, ANoteLeftOfTheEndIsRightAlignedToItLineByLine)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(4.0, 8.0);
    (void)driver.click(0.0, 8.0);
    (void)driver.enter();
    (void)driver.type("AB");
    (void)driver.type("C");
    const ToolStep done = driver.enter();
    EXPECT_EQ(done.message, "leader with 2 lines of text");
    // The last segment runs level and left: no hook, and the note ends
    // 0.625 left of (0, 8). "AB" is 2 x 0.6 x 2.5 = 3 wide, so it starts at
    // -3.625; "C" is 1.5 wide and starts at -2.125. Baselines 8 - 1.25 = 6.75
    // and one spacing, 2.5 x 5 / 3, lower.
    const auto polylines = shapesOf<Polyline2>(driver);
    ASSERT_EQ(polylines.size(), 2u);
    EXPECT_EQ(polylines[0],
              (Polyline2{{Point2(10.0, 0.0), Point2(4.0, 8.0), Point2(0.0, 8.0)}, false}));
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 2u);
    EXPECT_EQ(texts[0], (TextGeometry{Point2(-3.625, 6.75), "AB", 2.5, 0.0}));
    EXPECT_EQ(texts[1].position.x, -2.125);
    EXPECT_DOUBLE_EQ(texts[1].position.y, 6.75 - 12.5 / 3.0);
}

TEST(AnnotateLeader, ASteepLastSegmentHeadingLeftGetsItsHookToTheLeftAndTheNoteEndsBeyondIt)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(4.0, 8.0);
    (void)driver.enter();
    (void)driver.type("AB");
    (void)driver.enter();
    // The last segment is (-6, 8): atan(8 / 6) = 53.1 degrees off level,
    // heading left, so the hook runs one arrowhead (2.5) LEFT to (1.5, 8).
    // The note is right-aligned 0.625 short of the hook's end, at 0.875;
    // "AB" is 2 x 0.6 x 2.5 = 3 wide, so it starts at -2.125, baseline
    // 8 - 2.5 / 2 = 6.75.
    const auto polylines = shapesOf<Polyline2>(driver);
    ASSERT_EQ(polylines.size(), 2u);
    EXPECT_EQ(polylines[0],
              (Polyline2{{Point2(10.0, 0.0), Point2(4.0, 8.0), Point2(1.5, 8.0)}, false}));
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0], (TextGeometry{Point2(-2.125, 6.75), "AB", 2.5, 0.0}));
}

TEST(AnnotateLeader, ALastSegmentTypedDueSouthAs270DegreesPutsTheNoteOnTheRight)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.type("0,0");
    (void)driver.type("@10<0");
    // 270 degrees is due south, but cos(3 pi / 2) in binary is -1.8e-16, not
    // 0, so the point is 10 x that = 1.8e-15 left of x = 10: the double just
    // below 10. sin rounds to exactly -1, so y is -10. That is straight down
    // within rounding, which the rule puts on the right: a hook 2.5 right to
    // (12.5, -10) and the note 0.625 beyond it at 13.125, baseline
    // -10 - 2.5 / 2 = -11.25. Compared to within 1e-12, far below the 12.5
    // between the two sides.
    (void)driver.type("@10<270");
    (void)driver.enter();
    (void)driver.type("NOTE");
    (void)driver.enter();
    const auto polylines = shapesOf<Polyline2>(driver);
    ASSERT_EQ(polylines.size(), 2u);
    ASSERT_EQ(polylines[0].vertices.size(), 4u);
    EXPECT_NEAR(polylines[0].vertices[3].x, 12.5, 1e-12);
    EXPECT_EQ(polylines[0].vertices[3].y, -10.0);
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_NEAR(texts[0].position.x, 13.125, 1e-12);
    EXPECT_EQ(texts[0].position.y, -11.25);
}

TEST(AnnotateLeader, ALastSegmentWithinFifteenDegreesOfLevelGetsNoHook)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(0.0, 0.0);
    // atan(2 / 10) is 11.3 degrees.
    (void)driver.click(10.0, 2.0);
    (void)driver.enter();
    (void)driver.type("N");
    (void)driver.enter();
    const auto polylines = shapesOf<Polyline2>(driver);
    ASSERT_EQ(polylines.size(), 2u);
    EXPECT_EQ(polylines[0].vertices.size(), 2u);
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0].position, Point2(10.625, 0.75));
}

TEST(AnnotateLeader, ALeaderWithoutANoteIsItsLineAndArrowheadOnly)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, 10.0);
    (void)driver.enter();
    const ToolStep done = driver.enter();
    EXPECT_EQ(done.message, "leader with no text");
    const auto entities = drawing(driver);
    ASSERT_EQ(entities.size(), 2u);
    // No note, so no hook, although the line is vertical.
    EXPECT_EQ(std::get<Polyline2>(entities[0].geometry).vertices.size(), 2u);
}

TEST(AnnotateLeader, ItsArrowAndNoteTakeTheCurrentLayersDimensionStyle)
{
    ToolDriver driver;
    katana::entity::DimensionStyle style;
    style.name = "Survey";
    style.arrowHead = katana::entity::ArrowHead::Dot;
    style.arrowSize = 4.0;
    style.textHeight = 3.0;
    style.textGap = 1.0;
    ASSERT_TRUE(driver.document().execute(cmd::createDimensionStyle(style)).ok());
    katana::entity::Layer layer;
    layer.name = "Notes";
    layer.dimensionStyle = "Survey";
    ASSERT_TRUE(driver.document().execute(cmd::createLayer(layer)).ok());
    ASSERT_TRUE(driver.document().setCurrentLayer("Notes").ok());

    driver.start("annotate.leader");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(10.0, 0.0);
    (void)driver.enter();
    (void)driver.type("X");
    (void)driver.enter();
    // A dot head is a circle a quarter of the arrow size across in radius:
    // 4 / 4 = 1. The note: 1 right of (10, 0), baseline 0 - 3 / 2 = -1.5,
    // 3 tall.
    const auto circles = shapesOf<Circle2>(driver);
    ASSERT_EQ(circles.size(), 1u);
    EXPECT_EQ(circles[0].center, Point2(0.0, 0.0));
    EXPECT_EQ(circles[0].radius, 1.0);
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0], (TextGeometry{Point2(11.0, -1.5), "X", 3.0, 0.0}));
    for (const Entity& entity : drawing(driver)) {
        EXPECT_EQ(entity.layer, "Notes");
    }
}

namespace {

// A leader from (0, 0) east to (10, 0) with the note "X", drawn on a layer
// whose dimension style has `head` 3 long - a size whose third is 1, so the
// working below stays in small numbers. Pointing away from the line the
// arrow runs west, along (-1, 0); its left normal is (0, -1).
std::vector<Entity> leaderWithArrowHead(katana::entity::ArrowHead head)
{
    ToolDriver driver;
    katana::entity::DimensionStyle style;
    style.name = "Heads";
    style.arrowHead = head;
    style.arrowSize = 3.0;
    EXPECT_TRUE(driver.document().execute(cmd::createDimensionStyle(style)).ok());
    katana::entity::Layer layer;
    layer.name = "Notes";
    layer.dimensionStyle = "Heads";
    EXPECT_TRUE(driver.document().execute(cmd::createLayer(layer)).ok());
    EXPECT_TRUE(driver.document().setCurrentLayer("Notes").ok());
    driver.start("annotate.leader");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(10.0, 0.0);
    (void)driver.enter();
    (void)driver.type("X");
    (void)driver.enter();
    return drawing(driver);
}

} // namespace

TEST(AnnotateLeader, AnOpenArrowheadIsTwoStrokesMeetingAtTheTip)
{
    const auto entities = leaderWithArrowHead(katana::entity::ArrowHead::Open);
    ASSERT_EQ(entities.size(), 3u) << "line, arrowhead, note";
    // Back 3 along (1, 0) from the tip is (3, 0); a third of 3 either side
    // along the normal gives (3, -1) and (3, 1). An OPEN polyline through the
    // tip, so its two strokes are not joined across the back.
    const auto& head = std::get<Polyline2>(entities[1].geometry);
    EXPECT_FALSE(head.closed);
    ASSERT_EQ(head.vertices.size(), 3u);
    EXPECT_EQ(head.vertices[1], Point2(0.0, 0.0));
    EXPECT_NEAR(head.vertices[0].x, 3.0, 1e-12);
    EXPECT_NEAR(head.vertices[2].x, 3.0, 1e-12);
    EXPECT_NEAR(std::abs(head.vertices[0].y), 1.0, 1e-12);
    EXPECT_NEAR(head.vertices[0].y, -head.vertices[2].y, 1e-12) << "one either side";
    EXPECT_TRUE(std::holds_alternative<TextGeometry>(entities[2].geometry));
}

TEST(AnnotateLeader, ATickArrowheadIsOneStrokeThroughTheTipAt45Degrees)
{
    const auto entities = leaderWithArrowHead(katana::entity::ArrowHead::Tick);
    ASSERT_EQ(entities.size(), 3u) << "line, arrowhead, note";
    // Along plus normal is (-1, -1): the stroke leans / at 45 degrees, 3 long
    // and centred on the tip, so its ends are +-(1.5 / sqrt 2)(1, 1) =
    // +-(1.0607, 1.0607). 1 / sqrt 2 is not exact: compared within rounding.
    // A stroke has no direction, so either end may come first.
    const auto& tick = std::get<Segment2>(entities[1].geometry);
    const double reach = 1.5 / std::sqrt(2.0);
    EXPECT_NEAR(std::abs(tick.start.x), reach, 1e-12);
    EXPECT_NEAR(tick.start.y, tick.start.x, 1e-12) << "leans /";
    EXPECT_NEAR(tick.end.x, -tick.start.x, 1e-12) << "centred on the tip";
    EXPECT_NEAR(tick.end.y, -tick.start.y, 1e-12) << "centred on the tip";
    EXPECT_TRUE(std::holds_alternative<TextGeometry>(entities[2].geometry));
}

TEST(AnnotateLeader, WithNoArrowheadALeaderIsItsLineAndNoteOnly)
{
    const auto entities = leaderWithArrowHead(katana::entity::ArrowHead::None);
    ASSERT_EQ(entities.size(), 2u);
    EXPECT_EQ(std::get<Polyline2>(entities[0].geometry),
              (Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0)}, false}));
    EXPECT_TRUE(std::holds_alternative<TextGeometry>(entities[1].geometry));
}

TEST(AnnotateLeader, TypedAndPolarPointsAndTheAnnotationOption)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.type("0,0");
    // @10<0 is (10, 0): cos 0 and sin 0 are exact.
    (void)driver.type("@10<0");
    (void)driver.type("@0,-5");
    EXPECT_EQ(driver.type("A").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    (void)driver.enter();
    const auto polylines = shapesOf<Polyline2>(driver);
    ASSERT_GE(polylines.size(), 1u);
    EXPECT_EQ(polylines[0],
              (Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, -5.0)}, false}));
}

TEST(AnnotateLeader, UndoTakesBackTheLastPointOrLineOfText)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected);
    (void)driver.click(0.0, 0.0);
    (void)driver.click(5.0, 0.0);
    (void)driver.click(9.0, 9.0);
    EXPECT_EQ(driver.type("U").outcome, Outcome::Continue);
    (void)driver.enter();
    (void)driver.type("keep");
    (void)driver.type("drop");
    (void)driver.undo();
    (void)driver.enter();
    const auto polylines = shapesOf<Polyline2>(driver);
    ASSERT_EQ(polylines.size(), 2u);
    EXPECT_EQ(polylines[0], (Polyline2{{Point2(0.0, 0.0), Point2(5.0, 0.0)}, false}));
    const auto texts = shapesOf<TextGeometry>(driver);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(texts[0].text, "keep");
}

TEST(AnnotateLeader, DegenerateInputIsRefusedAndTheToolStaysWhereItWas)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(2.0, 2.0);
    const std::string prompt = driver.tool().prompt();
    EXPECT_EQ(driver.click(2.0, 2.0).outcome, Outcome::Rejected) << "zero-length segment";
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "only the tip";
    EXPECT_EQ(driver.type("A").outcome, Outcome::Rejected) << "no annotation yet";
    EXPECT_EQ(driver.type("7").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), prompt);
    (void)driver.click(6.0, 2.0);
    (void)driver.enter();
    EXPECT_EQ(driver.type("\xc3").outcome, Outcome::Rejected) << "a truncated UTF-8 sequence";
    EXPECT_EQ(driver.executed(), 0);
}

TEST(AnnotateLeader, ThePreviewIsTheLeaderWithTheCursorAsItsNextPoint)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(0.0, 0.0);
    const ToolFeedback feedback = driver.tool().preview({10.0, 0.0});
    // The line to the cursor and the standard closed arrowhead at the tip,
    // pointing west: back (2.5, 0), corners (2.5, -2.5/3) and (2.5, 2.5/3).
    ASSERT_EQ(feedback.shapes.size(), 2u);
    EXPECT_EQ(std::get<Polyline2>(feedback.shapes[0]),
              (Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0)}, false}));
    const auto& head = std::get<Polyline2>(feedback.shapes[1]);
    ASSERT_EQ(head.vertices.size(), 3u);
    EXPECT_EQ(head.vertices[1].x, 2.5);
    EXPECT_DOUBLE_EQ(std::abs(head.vertices[1].y), 2.5 / 3.0);
}

TEST(AnnotateLeader, TheToolStartsAgainAfterEachLeader)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(5.0, 0.0);
    (void)driver.enter();
    EXPECT_TRUE(driver.enter().restart);
    EXPECT_FALSE(driver.finished());
    EXPECT_FALSE(driver.tool().lastPoint().has_value());
}
