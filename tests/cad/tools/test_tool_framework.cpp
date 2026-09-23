// The interactive tool framework itself: the catalogue's rules, typed-input
// routing and the defaults every tool inherits. Each tool family has its own
// test file beside this one.

#include <gtest/gtest.h>

#include "katana/cad/interactive_tool.hpp"

namespace {

using katana::cad::InteractiveTool;
using katana::cad::ToolCatalog;
using katana::cad::ToolInfo;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::geometry::Point2;

// Records what reached it, so routing can be observed.
class Recorder final : public InteractiveTool {
  public:
    explicit Recorder(ToolInput wants) : wants_(wants) {}
    [[nodiscard]] std::string prompt() const override { return "record"; }
    [[nodiscard]] ToolInput expects() const override { return wants_; }
    ToolStep point(const Point2& at) override
    {
        points.push_back(at);
        last = at;
        return ToolStep::next();
    }
    ToolStep value(std::string_view text) override
    {
        values.emplace_back(text);
        return ToolStep::next();
    }
    [[nodiscard]] std::optional<Point2> lastPoint() const override { return last; }

    std::vector<Point2> points;
    std::vector<std::string> values;
    std::optional<Point2> last;

  private:
    ToolInput wants_;
};

ToolInfo validInfo(std::string id, std::vector<std::string> aliases)
{
    ToolInfo info;
    info.id = std::move(id);
    info.name = "Test";
    info.category = "Draw";
    info.group = "Lines";
    info.aliases = std::move(aliases);
    info.make = [](const katana::cad::ToolContext&) {
        return std::make_unique<Recorder>(ToolInput::Point);
    };
    return info;
}

} // namespace

TEST(ToolFramework, TypedCoordinatesArePointsAndBareNumbersAreValues)
{
    Recorder tool(ToolInput::Point);
    EXPECT_EQ(katana::cad::routeTypedInput(tool, "10,20").outcome, ToolStep::Outcome::Continue);
    EXPECT_EQ(katana::cad::routeTypedInput(tool, "12.5").outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(tool.points.size(), 1u);
    EXPECT_EQ(tool.points[0], Point2(10.0, 20.0));
    ASSERT_EQ(tool.values.size(), 1u);
    EXPECT_EQ(tool.values[0], "12.5");
}

TEST(ToolFramework, RelativeAndPolarInputMeasureFromTheToolsLastPoint)
{
    Recorder tool(ToolInput::Point);
    (void)katana::cad::routeTypedInput(tool, "10,20");
    (void)katana::cad::routeTypedInput(tool, "@3,4");
    ASSERT_EQ(tool.points.size(), 2u);
    EXPECT_EQ(tool.points[1], Point2(13.0, 24.0));
    // 5 at 90 degrees is straight up: (13, 24) + (0, 5). cos(90 deg) is not
    // exactly 0 in binary, so x is compared to within a few ulps of 13.
    (void)katana::cad::routeTypedInput(tool, "@5<90");
    ASSERT_EQ(tool.points.size(), 3u);
    EXPECT_NEAR(tool.points[2].x, 13.0, 1e-12);
    EXPECT_DOUBLE_EQ(tool.points[2].y, 29.0);
}

TEST(ToolFramework, RelativeInputBeforeAnyPointIsRefusedNotMeasuredFromTheOrigin)
{
    Recorder tool(ToolInput::Point);
    const ToolStep step = katana::cad::routeTypedInput(tool, "@3,4");
    EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected);
    EXPECT_TRUE(tool.points.empty());
}

TEST(ToolFramework, AMalformedPointIsRefusedWithAReason)
{
    Recorder tool(ToolInput::Point);
    const ToolStep step = katana::cad::routeTypedInput(tool, "10,abc");
    EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected);
    EXPECT_FALSE(step.message.empty());
    EXPECT_TRUE(tool.points.empty());
}

TEST(ToolFramework, ATextValueWithACommaIsTextWhenTheToolWantsAValue)
{
    Recorder tool(ToolInput::Value);
    (void)katana::cad::routeTypedInput(tool, "Road, north side");
    EXPECT_TRUE(tool.points.empty());
    ASSERT_EQ(tool.values.size(), 1u);
    EXPECT_EQ(tool.values[0], "Road, north side");
}

TEST(ToolFramework, TheDefaultsRefuseWhatAToolDoesNotTakeAndSayWhatItWants)
{
    Recorder tool(ToolInput::Point);
    const ToolStep step = tool.InteractiveTool::entity(7, {0.0, 0.0});
    EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected);
    EXPECT_NE(step.message.find("point"), std::string::npos);
    EXPECT_EQ(tool.InteractiveTool::undo().outcome, ToolStep::Outcome::Rejected);
    const ToolStep finished = tool.InteractiveTool::enter();
    EXPECT_EQ(finished.outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(finished.command, nullptr);
}

TEST(ToolFramework, TheCatalogueRefusesADuplicateIdOrAlias)
{
    ToolCatalog catalog;
    ASSERT_TRUE(catalog.add(validInfo("test.one", {"ONE", "O1"})).ok());
    const auto sameId = catalog.add(validInfo("test.one", {"OTHER"}));
    ASSERT_FALSE(sameId.ok());
    EXPECT_EQ(sameId.error().code, katana::core::ErrorCode::AlreadyExists);
    const auto sameAlias = catalog.add(validInfo("test.two", {"O1"}));
    ASSERT_FALSE(sameAlias.ok());
    EXPECT_EQ(sameAlias.error().code, katana::core::ErrorCode::AlreadyExists);
    EXPECT_EQ(catalog.size(), 1u);
}

TEST(ToolFramework, TheCatalogueRefusesIncompleteEntries)
{
    ToolCatalog catalog;
    ToolInfo noFactory = validInfo("test.a", {"A1"});
    noFactory.make = nullptr;
    EXPECT_FALSE(catalog.add(noFactory).ok());
    ToolInfo lowerAlias = validInfo("test.b", {"line"});
    EXPECT_FALSE(catalog.add(lowerAlias).ok());
    ToolInfo letterShortcut = validInfo("test.c", {"C1"});
    letterShortcut.shortcut = "L";
    EXPECT_FALSE(catalog.add(letterShortcut).ok());
    ToolInfo noGroup = validInfo("test.d", {"D1"});
    noGroup.group.clear();
    EXPECT_FALSE(catalog.add(noGroup).ok());
    EXPECT_EQ(catalog.size(), 0u);
}

TEST(ToolFramework, AliasesAreFoundWhateverTheCaseTheUserTypes)
{
    ToolCatalog catalog;
    ASSERT_TRUE(catalog.add(validInfo("test.one", {"ONE"})).ok());
    ASSERT_NE(catalog.findByAlias("one"), nullptr);
    EXPECT_EQ(catalog.findByAlias("one")->id, "test.one");
    EXPECT_EQ(catalog.findByAlias("two"), nullptr);
}

TEST(ToolFramework, TheCatalogueListsToolsInMenuOrder)
{
    ToolCatalog catalog;
    ToolInfo late = validInfo("test.late", {"LATE"});
    late.order = 2;
    ToolInfo early = validInfo("test.early", {"EARLY"});
    early.order = 1;
    ToolInfo modify = validInfo("test.modify", {"MOD"});
    modify.category = "Modify";
    ASSERT_TRUE(catalog.add(late).ok());
    ASSERT_TRUE(catalog.add(modify).ok());
    ASSERT_TRUE(catalog.add(early).ok());
    const auto all = catalog.all();
    ASSERT_EQ(all.size(), 3u);
    EXPECT_EQ(all[0]->id, "test.early");
    EXPECT_EQ(all[1]->id, "test.late");
    EXPECT_EQ(all[2]->id, "test.modify");
}

TEST(ToolFramework, EveryToolInTheProgramsCatalogueRegistered)
{
    // A family's tool that the catalogue refused would be missing from every
    // menu with nothing to say why; the refusals are kept so this can see them.
    for (const std::string& problem : katana::cad::toolCatalogProblems()) {
        ADD_FAILURE() << problem;
    }
}
