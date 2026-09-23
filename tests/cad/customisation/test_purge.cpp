// cad::planPurge and cad::purgeCommand: what nothing uses, found to a
// fixpoint, deleted as one undo step, and never an empty one.

#include <gtest/gtest.h>

#include "katana/cad/document.hpp"
#include "katana/cad/purge.hpp"

using namespace katana::cad;
using katana::commands::TableItems;

namespace {

// The drawing every test reads:
//
//   linetypes  fence  named only by style Post, which nothing wears
//              dash   named by layer kerbs
//              rail   named by style Kerb, which a point wears
//              spare  named by nothing
//   hatches    grass  named only by Post
//              stone  named by layer kerbs
//              brick  named by nothing
//   styles     Post (fence, grass) and Spare: worn by nothing
//              Kerb (rail): worn by the one point
//
// So a purge takes Post and Spare, and - because Post goes - fence and
// grass with them; spare and brick because nothing names them. It keeps
// dash and stone (a layer names them), rail (Kerb is worn), and the
// protected continuous and none.
struct Purge : ::testing::Test {
    Document document;

    void SetUp() override
    {
        for (const char* name : {"fence", "dash", "rail", "spare"}) {
            katana::entity::Linetype linetype;
            linetype.name = name;
            linetype.pattern = {{1.0}, {-0.5}};
            must(katana::commands::createLinetype(linetype));
        }
        for (const char* name : {"grass", "stone", "brick"}) {
            katana::entity::HatchPattern pattern;
            pattern.name = name;
            pattern.families = {{0.0, 1.0, 0.0}};
            must(katana::commands::createHatchPattern(pattern));
        }
        katana::entity::Layer kerbs;
        kerbs.name = "kerbs";
        kerbs.linetype = "dash";
        kerbs.hatchPattern = "stone";
        must(katana::commands::createLayer(kerbs));
        must(katana::commands::createStyle(style("Post", "fence", "grass")));
        must(katana::commands::createStyle(style("Spare", "continuous", "")));
        must(katana::commands::createStyle(style("Kerb", "rail", "")));
        katana::commands::EntityAttributes attributes;
        attributes.style = "Kerb";
        must(katana::commands::createPoint(katana::geometry::Point2(0.0, 0.0), attributes));
    }

    void must(katana::commands::CommandPtr command)
    {
        const auto status = document.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    static katana::entity::Style style(const char* name, const char* linetype, const char* hatch)
    {
        katana::entity::Style made;
        made.name = name;
        made.linetype = linetype;
        made.hatchPattern = hatch;
        return made;
    }
};

} // namespace

TEST_F(Purge, ThePlanIsWhatNothingUsesIncludingWhatOnlyAPurgedStyleNamed)
{
    const TableItems plan = planPurge(document.model());
    EXPECT_EQ(plan.styles, (std::vector<std::string>{"Post", "Spare"}));
    EXPECT_EQ(plan.linetypes, (std::vector<std::string>{"fence", "spare"}))
        << "fence is freed by purging Post, in the same purge";
    EXPECT_EQ(plan.hatchPatterns, (std::vector<std::string>{"brick", "grass"}));
}

TEST_F(Purge, ProtectedItemsAreNeverPlannedEvenInAnEmptyDrawing)
{
    Document empty;
    // A new drawing has continuous, none, layer 0 and nothing using them.
    EXPECT_TRUE(planPurge(empty.model()).empty());
    auto command = purgeCommand(empty.model());
    ASSERT_TRUE(command.ok());
    EXPECT_EQ(*command, nullptr) << "nothing to purge is no command, not an empty one";
}

TEST_F(Purge, ContinuousAndNoneAreNeverPlannedEvenWhenNothingNamesThem)
{
    // In the drawings above layer 0 names continuous and none, so they are
    // "used" and the protection is never what keeps them out. Point layer
    // 0 at dash and stone: now continuous is named only by Spare, which is
    // purged, and none by nothing (Kerb's hatch is empty, so the point
    // reaches layer 0's stone). Only the protection can exclude them.
    katana::entity::Layer zero = *document.model().layers.find("0");
    zero.linetype = "dash";
    zero.hatchPattern = "stone";
    must(katana::commands::updateLayer(zero));

    // Without the protection these would be {continuous, fence, spare} and
    // {brick, grass, none}.
    const TableItems plan = planPurge(document.model());
    EXPECT_EQ(plan.styles, (std::vector<std::string>{"Post", "Spare"}));
    EXPECT_EQ(plan.linetypes, (std::vector<std::string>{"fence", "spare"}));
    EXPECT_EQ(plan.hatchPatterns, (std::vector<std::string>{"brick", "grass"}));

    // And so the purge goes through: a plan holding continuous would be
    // refused as a whole, and remove nothing - not even spare.
    auto command = purgeCommand(document.model());
    ASSERT_TRUE(command.ok());
    ASSERT_NE(*command, nullptr);
    must(std::move(*command));
    EXPECT_FALSE(document.model().linetypes.contains("spare"));
    EXPECT_TRUE(document.model().linetypes.contains("continuous"));
    EXPECT_TRUE(document.model().hatchPatterns.contains("none"));
}

TEST_F(Purge, ALinetypeAStayingStyleNamesIsKept)
{
    // Styles not purged: Post stays, so fence and grass stay with it.
    const TableItems linetypesOnly =
        planPurge(document.model(), PurgeOptions{.styles = false, .linetypes = true, .hatches = true});
    EXPECT_TRUE(linetypesOnly.styles.empty());
    EXPECT_EQ(linetypesOnly.linetypes, std::vector<std::string>{"spare"});
    EXPECT_EQ(linetypesOnly.hatchPatterns, std::vector<std::string>{"brick"});

    // The current style is kept although no entity wears it, and what it
    // names is kept with it.
    const TableItems keepingPost =
        planPurge(document.model(), PurgeOptions{.keepStyles = {"Post"}});
    EXPECT_EQ(keepingPost.styles, std::vector<std::string>{"Spare"});
    EXPECT_EQ(keepingPost.linetypes, std::vector<std::string>{"spare"});
    EXPECT_EQ(keepingPost.hatchPatterns, std::vector<std::string>{"brick"});
}

TEST_F(Purge, OnePurgeIsOneUndoStepAndOneUndoRestoresEverything)
{
    const auto stylesBefore = document.model().styles.all();
    const auto linetypesBefore = document.model().linetypes.all();
    const auto hatchesBefore = document.model().hatchPatterns.all();
    const std::size_t steps = document.history().undoCount();

    auto command = purgeCommand(document.model());
    ASSERT_TRUE(command.ok());
    ASSERT_NE(*command, nullptr);
    EXPECT_TRUE((*command)->isDestructive());
    must(std::move(*command));
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    // What is left: styles Kerb; linetypes continuous, dash, rail; hatches
    // none, stone.
    EXPECT_EQ(document.model().styles.names(), std::vector<std::string>{"Kerb"});
    EXPECT_EQ(document.model().linetypes.names(),
              (std::vector<std::string>{"continuous", "dash", "rail"}));
    EXPECT_EQ(document.model().hatchPatterns.names(),
              (std::vector<std::string>{"none", "stone"}));

    // A second purge finds nothing: the plan was already a fixpoint.
    auto again = purgeCommand(document.model());
    ASSERT_TRUE(again.ok());
    EXPECT_EQ(*again, nullptr);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().styles.all(), stylesBefore);
    EXPECT_EQ(document.model().linetypes.all(), linetypesBefore);
    EXPECT_EQ(document.model().hatchPatterns.all(), hatchesBefore);
}

TEST_F(Purge, AskingForNothingIsAMistakeNotAnEmptyPurge)
{
    const auto command =
        purgeCommand(document.model(), PurgeOptions{.styles = false, .linetypes = false, .hatches = false});
    ASSERT_FALSE(command.ok());
    EXPECT_EQ(command.error().code, katana::core::ErrorCode::InvalidArgument);
}
