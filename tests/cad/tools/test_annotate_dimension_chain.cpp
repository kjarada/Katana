// The Baseline and Continue Dimension tools (annotate_dimension_chain.cpp),
// driven through ToolDriver and held to DIM BASELINE and DIM CONTINUE: the
// same points typed into a second drawing made the same way must make the
// same dimensions. The spacing and offsets are worked out by hand beside
// each check.

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
using katana::entity::AnchorPoint;
using katana::entity::AnchorRef;
using katana::entity::DimensionGeometry;
using katana::entity::DimensionKind;
using katana::entity::Entity;
using katana::geometry::Point2;
using Outcome = katana::cad::ToolStep::Outcome;

void run(Document& document, const std::string& line)
{
    CommandInterpreter interpreter(document);
    const auto reply = interpreter.run(line);
    ASSERT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
}

std::vector<Entity> dimensionsOf(Document& document)
{
    std::vector<Entity> out;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<DimensionGeometry>(entity.geometry)) {
            out.push_back(entity);
        }
    });
    return out;
}

const DimensionGeometry& geometryOf(const Entity& entity)
{
    return std::get<DimensionGeometry>(entity.geometry);
}

void expectSameDimensions(Document& drawn, Document& typed)
{
    const auto made = dimensionsOf(drawn);
    const auto expected = dimensionsOf(typed);
    ASSERT_EQ(made.size(), expected.size());
    for (std::size_t i = 0; i < made.size(); ++i) {
        EXPECT_EQ(made[i].geometry, expected[i].geometry) << i;
        EXPECT_EQ(made[i].layer, expected[i].layer) << i;
        EXPECT_EQ(made[i].style, expected[i].style) << i;
    }
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(DimensionChainTools, TheyAreInTheAnnotateMenuUnderTheirCommandNames)
{
    const auto& catalog = katana::cad::toolCatalog();
    const std::vector<std::pair<std::string, std::vector<std::string>>> expected = {
        {"annotate.dimbaseline", {"DIMBASELINE", "DBA"}},
        {"annotate.dimcontinue", {"DIMCONTINUE", "DCO"}},
        {"annotate.balloon", {"BALLOON"}},
    };
    for (const auto& [id, aliases] : expected) {
        const katana::cad::ToolInfo* info = catalog.find(id);
        ASSERT_NE(info, nullptr) << id;
        EXPECT_EQ(info->category, "Annotate") << id;
        EXPECT_FALSE(info->tip.empty()) << id;
        for (const std::string& alias : aliases) {
            ASSERT_NE(catalog.findByAlias(alias), nullptr) << alias;
            EXPECT_EQ(catalog.findByAlias(alias)->id, id) << alias;
        }
    }
}

TEST(DimensionChainTools, BaselineGoesOnFromTheNewestDimensionAsDimBaselineDoes)
{
    ToolDriver driver;
    run(driver.document(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
    driver.start("annotate.dimbaseline");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point) << "the newest is taken as the base";
    EXPECT_EQ(driver.tool().prompt(),
              "Specify next extension line origin (from dimension 1) or [Select/Spacing/Undo] "
              "<done>");
    (void)driver.click(20.0, 0.0);
    (void)driver.click(30.0, 0.0);
    const ToolStep done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_EQ(done.message, "2 baseline dimensions");
    EXPECT_EQ(driver.executed(), 1) << "the chain is one undo step";

    Document typed;
    run(typed, "DIM LINEAR 0,0 10,0 at=5,5");
    run(typed, "DIM BASELINE 1 20,0 30,0");
    expectSameDimensions(driver.document(), typed);
    // The Standard style's 2.5 text at 1 : 1000 is 2.5, so the lines are
    // 3.75 apart: the base's at 5, then 8.75 and 12.5, all from 0,0.
    const auto all = dimensionsOf(driver.document());
    ASSERT_EQ(all.size(), 3u);
    EXPECT_EQ(geometryOf(all[1]).kind, DimensionKind::Linear);
    EXPECT_EQ(geometryOf(all[1]).start, Point2(0.0, 0.0));
    EXPECT_EQ(geometryOf(all[1]).offset, 8.75);
    EXPECT_EQ(geometryOf(all[2]).offset, 12.5);
    EXPECT_EQ(geometryOf(all[2]).measurement(), 30.0);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(dimensionsOf(driver.document()).size(), 1u);
}

TEST(DimensionChainTools, SpacingIsBaselinesAndContinueChainsEndToEnd)
{
    ToolDriver driver;
    run(driver.document(), "DIM LINEAR 0,0 10,0 at=5,5");
    driver.start("annotate.dimbaseline");
    (void)driver.type("SP");
    EXPECT_EQ(driver.tool().prompt(), "Specify baseline spacing <3.75>");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    (void)driver.type("5");
    (void)driver.click(20.0, 0.0);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    Document typed;
    run(typed, "DIM LINEAR 0,0 10,0 at=5,5");
    run(typed, "DIM BASELINE 1 20,0 spacing=5");
    expectSameDimensions(driver.document(), typed);
    EXPECT_EQ(geometryOf(dimensionsOf(driver.document())[1]).offset, 10.0);

    // Continue from an aligned base: end to end, each keeping its offset.
    ToolDriver chain;
    run(chain.document(), "DIM ALIGNED 0,0 10,0 at=5,5");
    chain.start("annotate.dimcontinue");
    EXPECT_TRUE(contains(chain.tool().prompt(), "[Select/Undo] <done>")) << chain.tool().prompt();
    EXPECT_EQ(chain.type("SP").outcome, Outcome::Rejected) << "Continue has no spacing";
    (void)chain.click(20.0, 0.0);
    (void)chain.click(35.0, 0.0);
    const ToolStep done = chain.enter();
    EXPECT_EQ(done.message, "2 continued dimensions");
    Document continued;
    run(continued, "DIM ALIGNED 0,0 10,0 at=5,5");
    run(continued, "DIM CONTINUE 1 20,0 35,0");
    expectSameDimensions(chain.document(), continued);
    const auto all = dimensionsOf(chain.document());
    ASSERT_EQ(all.size(), 3u);
    EXPECT_EQ(geometryOf(all[2]).start, Point2(20.0, 0.0));
    EXPECT_EQ(geometryOf(all[2]).measurement(), 15.0);
}

TEST(DimensionChainTools, SelectTakesAnotherBaseAndOnlyALinearOrAlignedOne)
{
    ToolDriver driver;
    run(driver.document(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
    run(driver.document(), "CIRCLE 50,50 5");             // 2
    run(driver.document(), "DIM RADIUS 2");               // 3, not one to chain from
    run(driver.document(), "DIM ALIGNED 0,20 10,20 at=5,25"); // 4
    driver.start("annotate.dimcontinue");
    EXPECT_TRUE(contains(driver.tool().prompt(), "(from dimension 4)")) << driver.tool().prompt();
    (void)driver.type("S");
    EXPECT_EQ(driver.tool().prompt(), "Select base dimension");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.pick(2, 55.0, 50.0).outcome, Outcome::Rejected) << "a circle";
    EXPECT_EQ(driver.pick(3, 52.0, 52.0).outcome, Outcome::Rejected) << "a radius";
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue) << "back to the base it had";
    EXPECT_TRUE(contains(driver.tool().prompt(), "(from dimension 4)"));
    (void)driver.type("S");
    ASSERT_EQ(driver.pick(1, 5.0, 5.0).outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "(from dimension 1)"));
    (void)driver.click(20.0, 0.0);
    EXPECT_EQ(driver.type("S").outcome, Outcome::Rejected) << "not with a point placed";
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(geometryOf(dimensionsOf(driver.document()).back()).start, Point2(10.0, 0.0));
}

TEST(DimensionChainTools, EnterAfterAChainGoesOnFromItsLastAndEnterWithNonePlacedEnds)
{
    ToolDriver driver;
    run(driver.document(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
    driver.start("annotate.dimcontinue");
    (void)driver.click(20.0, 0.0);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done); // 2
    EXPECT_FALSE(driver.finished());
    EXPECT_TRUE(contains(driver.tool().prompt(), "(from dimension 2)")) << driver.tool().prompt();
    (void)driver.click(30.0, 0.0);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done); // 3, from 20,0
    EXPECT_EQ(geometryOf(dimensionsOf(driver.document()).back()).start, Point2(20.0, 0.0));
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished()) << "Enter, Enter ends it, as in AutoCAD";
    EXPECT_EQ(driver.executed(), 2);
}

TEST(DimensionChainTools, WithNoDimensionTheBaseIsAskedForAndEnterEnds)
{
    ToolDriver driver;
    driver.start("annotate.dimbaseline");
    EXPECT_EQ(driver.tool().prompt(), "Select base dimension");
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected);
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(driver.executed(), 0);
}

TEST(DimensionChainTools, UndoTakesBackAPointAndEscKeepsThoseGiven)
{
    ToolDriver driver;
    run(driver.document(), "DIM LINEAR 0,0 10,0 at=5,5");
    driver.start("annotate.dimbaseline");
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected) << "no origin given yet";
    (void)driver.click(20.0, 0.0);
    (void)driver.click(30.0, 0.0);
    (void)driver.click(40.0, 0.0);
    EXPECT_EQ(driver.type("U").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().lastPoint(), std::optional<Point2>(Point2(30.0, 0.0)));
    const ToolStep kept = driver.cancel();
    ASSERT_EQ(kept.outcome, Outcome::Done);
    EXPECT_EQ(kept.message, "2 baseline dimensions");
    EXPECT_EQ(dimensionsOf(driver.document()).size(), 3u);
    EXPECT_TRUE(driver.finished());
}

TEST(DimensionChainTools, AnOriginThatWouldMeasureNothingIsRefusedWhenItIsGiven)
{
    ToolDriver driver;
    run(driver.document(), "DIM LINEAR 0,0 10,0 at=5,5");
    driver.start("annotate.dimcontinue");
    // From the base's end, 10,0, to 10,7 measures nothing across.
    const ToolStep refused = driver.click(10.0, 7.0);
    EXPECT_EQ(refused.outcome, Outcome::Rejected);
    EXPECT_FALSE(refused.message.empty());
    (void)driver.click(20.0, 0.0);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(dimensionsOf(driver.document()).size(), 2u);
}

TEST(DimensionChainTools, ASnappedOriginFollowsTheEntityItWasSnappedTo)
{
    ToolDriver driver;
    run(driver.document(), "DIM LINEAR 0,0 10,0 at=5,5"); // 1
    run(driver.document(), "LINE 20,0 20,-5");            // 2
    driver.start("annotate.dimcontinue");
    (void)driver.clickSnapped(20.1, 0.1); // line 2's start
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    const DimensionGeometry made = geometryOf(dimensionsOf(driver.document()).back());
    EXPECT_EQ(made.endRef, (AnchorRef{2, AnchorPoint::Start, 0}));
    Document typed;
    run(typed, "DIM LINEAR 0,0 10,0 at=5,5");
    run(typed, "LINE 20,0 20,-5");
    run(typed, "DIM CONTINUE 1 #2.start");
    expectSameDimensions(driver.document(), typed);
    // The line moves 4 right: the dimension now measures 10 to 24.
    run(driver.document(), "SELECT 2");
    run(driver.document(), "MOVE 4,0");
    EXPECT_EQ(geometryOf(dimensionsOf(driver.document()).back()).measurement(), 14.0);
}
