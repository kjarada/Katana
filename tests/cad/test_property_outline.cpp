// The property outline (property_outline.hpp): flat '/' names read as a tree
// one level at a time, several entities read as one, and PROP TREE's reply.
// Every expectation is worked out by hand from the maps the tests build.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/property_outline.hpp"
#include "katana/commands/entity_commands.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::EntityId;
using katana::entity::PropertyMap;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
namespace cmd = katana::commands;

namespace {

// A surveyed string as an archive import leaves it: a group two deep, a value
// of its own, and per-vertex attributes on vertices 1, 2 and 10.
PropertyMap surveyedString()
{
    return {
        {"Asset/Dimensions/Size", PropertyValue(std::int64_t{300})},
        {"Asset/Dimensions/Depth", PropertyValue(1.5)},
        {"Asset/Owner", PropertyValue(std::string("Council"))},
        {"Name", PropertyValue(std::string("KERB"))},
        {"vertex/1/QL", PropertyValue(std::string("B"))},
        {"vertex/1/Depth", PropertyValue(0.6)},
        {"vertex/2/QL", PropertyValue(std::string("A"))},
        {"vertex/10/QL", PropertyValue(std::string("C"))},
    };
}

std::vector<std::string> names(const std::vector<PropertyOutlineEntry>& entries)
{
    std::vector<std::string> out;
    for (const auto& entry : entries) {
        out.push_back(entry.name);
    }
    return out;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

// ---- the order ----------------------------------------------------------------------------

TEST(PropertyOutline, NaturalOrderPutsVertexTwoBeforeVertexTen)
{
    EXPECT_TRUE(naturalLess("vertex/2", "vertex/10"));
    EXPECT_FALSE(naturalLess("vertex/10", "vertex/2"));
    EXPECT_TRUE(naturalLess("9", "10"));
    // A run longer than any integer type still compares as a number.
    EXPECT_TRUE(naturalLess("vertex/99", "vertex/123456789012345678901234567890"));
    // Letters without regard to case: "asset" before "Name" before "vertex",
    // where the map's byte order puts every capital first.
    EXPECT_TRUE(naturalLess("asset", "Name"));
    EXPECT_TRUE(naturalLess("Name", "vertex"));
}

TEST(PropertyOutline, NaturalOrderIsTotalWhereCaseOrLeadingZerosAloneDiffer)
{
    // Equal when folded, so the plain byte order decides: 'N' (78) before
    // 'n' (110), and "007" before "7" ('0' before '7').
    EXPECT_TRUE(naturalLess("Name", "name"));
    EXPECT_FALSE(naturalLess("name", "Name"));
    EXPECT_TRUE(naturalLess("007", "7"));
    EXPECT_FALSE(naturalLess("7", "007"));
    // Irreflexive, and a prefix comes first.
    EXPECT_FALSE(naturalLess("QL", "QL"));
    EXPECT_TRUE(naturalLess("QL", "QL2"));
    EXPECT_FALSE(naturalLess("", ""));
    EXPECT_TRUE(naturalLess("", "a"));
}

// ---- one map ------------------------------------------------------------------------------

TEST(PropertyOutline, TheTopOfAnArchiveStringIsItsGroupsItsValuesAndItsVertices)
{
    const auto top = propertyOutline(surveyedString(), "");
    ASSERT_EQ(names(top), (std::vector<std::string>{"Asset", "Name", "vertex"}));

    // Asset: Dimensions and Owner beneath it, three values in all.
    EXPECT_EQ(top[0].path, "Asset");
    EXPECT_EQ(top[0].holders, 0u);
    EXPECT_FALSE(top[0].value.has_value());
    EXPECT_FALSE(top[0].varies);
    EXPECT_EQ(top[0].children, 2u);
    EXPECT_EQ(top[0].values, 3u);

    EXPECT_EQ(top[1].path, "Name");
    EXPECT_EQ(top[1].holders, 1u);
    EXPECT_EQ(top[1].value, PropertyValue(std::string("KERB")));
    EXPECT_EQ(top[1].children, 0u);

    // vertex: vertices 1, 2 and 10, four values.
    EXPECT_EQ(top[2].children, 3u);
    EXPECT_EQ(top[2].values, 4u);
}

TEST(PropertyOutline, VerticesAreListedInTheirNumbersOrder)
{
    const auto vertices = propertyOutline(surveyedString(), "vertex");
    ASSERT_EQ(names(vertices), (std::vector<std::string>{"1", "2", "10"}));
    EXPECT_EQ(vertices[0].path, "vertex/1");
    EXPECT_EQ(vertices[0].children, 2u);
    EXPECT_EQ(vertices[2].path, "vertex/10");
    EXPECT_EQ(vertices[2].children, 1u);

    const auto first = propertyOutline(surveyedString(), "vertex/1");
    ASSERT_EQ(names(first), (std::vector<std::string>{"Depth", "QL"}));
    EXPECT_EQ(first[0].value, PropertyValue(0.6));
    EXPECT_EQ(first[1].path, "vertex/1/QL");
}

TEST(PropertyOutline, ALevelTwoDeepHoldsItsValuesWithTheirTypes)
{
    const auto dimensions = propertyOutline(surveyedString(), "Asset/Dimensions");
    ASSERT_EQ(names(dimensions), (std::vector<std::string>{"Depth", "Size"}));
    EXPECT_EQ(dimensions[0].value, PropertyValue(1.5));
    EXPECT_EQ(dimensions[1].value, PropertyValue(std::int64_t{300}));
    EXPECT_EQ(dimensions[1].path, "Asset/Dimensions/Size");
}

TEST(PropertyOutline, ANameThatIsAValueAndABranchIsBoth)
{
    const PropertyMap pipe = {{"Pipe", PropertyValue(std::string("DN300"))},
                              {"Pipe/Material", PropertyValue(std::string("PVC"))}};
    const auto top = propertyOutline(pipe, "");
    ASSERT_EQ(top.size(), 1u);
    EXPECT_EQ(top[0].value, PropertyValue(std::string("DN300")));
    EXPECT_EQ(top[0].children, 1u);
    EXPECT_EQ(top[0].values, 1u);
}

TEST(PropertyOutline, APathNothingIsBeneathAndAnEmptyMapGiveNoEntries)
{
    EXPECT_TRUE(propertyOutline(surveyedString(), "Nothing").empty());
    // "Name" holds a value but nothing is beneath it.
    EXPECT_TRUE(propertyOutline(surveyedString(), "Name").empty());
    // "vert" is the start of a name, not a part of one.
    EXPECT_TRUE(propertyOutline(surveyedString(), "vert").empty());
    EXPECT_TRUE(propertyOutline(PropertyMap{}, "").empty());
    EXPECT_TRUE(propertyOutline(std::span<const PropertyMap* const>{}, "").empty());
}

TEST(PropertyOutline, AnEmptyPartOfANameIsAnEntryOfItsOwnRatherThanLost)
{
    const PropertyMap doubled = {{"a//b", PropertyValue(true)}};
    const auto under = propertyOutline(doubled, "a");
    ASSERT_EQ(under.size(), 1u);
    EXPECT_EQ(under[0].name, "");
    EXPECT_EQ(under[0].path, "a/");
    EXPECT_EQ(under[0].children, 1u);
}

TEST(PropertyOutline, WhetherAnythingIsUnderAPathNeedsNoLevelRead)
{
    const PropertyMap string = surveyedString();
    EXPECT_TRUE(hasPropertiesUnder(string, "vertex"));
    EXPECT_TRUE(hasPropertiesUnder(string, "vertex/10"));
    EXPECT_TRUE(hasPropertiesUnder(string, "Name"));
    EXPECT_FALSE(hasPropertiesUnder(string, "vertex/3"));
    EXPECT_FALSE(hasPropertiesUnder(string, "vert"));
    EXPECT_TRUE(hasPropertiesUnder(string, ""));
    EXPECT_FALSE(hasPropertiesUnder(PropertyMap{}, ""));
}

// ---- several maps -------------------------------------------------------------------------

TEST(PropertyOutline, SeveralEntitiesShowAValueOnlyWhereEveryOneHoldsTheSame)
{
    const PropertyMap a = {{"code", PropertyValue(std::string("TREE"))},
                           {"height", PropertyValue(1.0)},
                           {"only", PropertyValue(std::int64_t{1})},
                           {"g/x", PropertyValue(true)}};
    const PropertyMap b = {{"code", PropertyValue(std::string("TREE"))},
                           {"height", PropertyValue(2.0)},
                           {"g/x", PropertyValue(true)},
                           {"g/y", PropertyValue(false)}};
    const PropertyMap* const both[] = {&a, &b};
    const auto top = propertyOutline(std::span<const PropertyMap* const>(both), "");
    ASSERT_EQ(names(top), (std::vector<std::string>{"code", "g", "height", "only"}));

    EXPECT_EQ(top[0].value, PropertyValue(std::string("TREE")));
    EXPECT_EQ(top[0].holders, 2u);
    EXPECT_FALSE(top[0].varies);

    // g: a branch, x in both and y in one; three values counted per entity.
    EXPECT_FALSE(top[1].varies);
    EXPECT_EQ(top[1].holders, 0u);
    EXPECT_EQ(top[1].children, 2u);
    EXPECT_EQ(top[1].values, 3u);

    // Held differently.
    EXPECT_TRUE(top[2].varies);
    EXPECT_EQ(top[2].holders, 2u);
    EXPECT_FALSE(top[2].value.has_value());

    // Held by one of the two: as much a disagreement.
    EXPECT_TRUE(top[3].varies);
    EXPECT_EQ(top[3].holders, 1u);
    EXPECT_FALSE(top[3].value.has_value());
}

// ---- PROP TREE ----------------------------------------------------------------------------

namespace {

// Two points: the surveyed string's properties on the first, and code=TREE
// with a height on both.
struct PropTreeTest : ::testing::Test {
    Document document;
    CommandInterpreter interpreter{document};
    EntityId first = 0;
    EntityId second = 0;

    void SetUp() override
    {
        first = create(cmd::createPoint(Point2(0, 0), {"0", "", {}}));
        second = create(cmd::createPoint(Point2(10, 0), {"0", "", {}}));
        for (const auto& [key, value] : surveyedString()) {
            ASSERT_TRUE(document.execute(cmd::setEntityProperty({first}, key, value)).ok());
        }
        ASSERT_TRUE(document.execute(cmd::setEntityProperty({first}, "height", 1.0)).ok());
        ASSERT_TRUE(document.execute(cmd::setEntityProperty({second}, "height", 2.0)).ok());
    }

    EntityId create(cmd::CommandPtr command)
    {
        EXPECT_TRUE(document.execute(std::move(command)).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }

    std::string reply(const std::string& line)
    {
        auto result = interpreter.run(line);
        EXPECT_TRUE(result.ok()) << line << ": " << (result.ok() ? "" : result.error().describe());
        return result.ok() ? *result : std::string();
    }
};

} // namespace

TEST_F(PropTreeTest, TheSelectionsTopLevelIsARecordForTheLevelThenOneAnEntry)
{
    ASSERT_TRUE(interpreter.run("SELECT " + std::to_string(first)).ok());
    EXPECT_EQ(reply("PROP TREE"), "scope=selection matched=1 under=\"\" entries=4 from=0 shown=4\n"
                                  "path=Asset children=2 values=3\n"
                                  "path=height value=1 type=real\n"
                                  "path=Name value=KERB type=text\n"
                                  "path=vertex children=3 values=4");
}

TEST_F(PropTreeTest, UnderNamesTheLevelAndFromAndLimitPageThroughIt)
{
    ASSERT_TRUE(interpreter.run("SELECT " + std::to_string(first)).ok());
    EXPECT_EQ(reply("PROP TREE UNDER vertex FROM 1 LIMIT 1"),
              "scope=selection matched=1 under=vertex entries=3 from=1 shown=1\n"
              "path=vertex/2 children=1 values=1");
    // A trailing '/' names the same branch.
    EXPECT_EQ(reply("prop tree under vertex/10/"),
              "scope=selection matched=1 under=vertex/10 entries=1 from=0 shown=1\n"
              "path=vertex/10/QL value=C type=text");
    // Past the end is an empty page, not a refusal.
    EXPECT_EQ(reply("PROP TREE UNDER vertex FROM 7"),
              "scope=selection matched=1 under=vertex entries=3 from=3 shown=0");
}

TEST_F(PropTreeTest, ADrawingScopeReadsEveryEntityAndSaysWhereTheyDiffer)
{
    // Nothing selected: the scope is what is read, not the selection.
    const std::string text = reply("PROP TREE DRAWING");
    EXPECT_TRUE(contains(text, "scope=drawing matched=2 under=\"\" entries=4")) << text;
    EXPECT_TRUE(contains(text, "\npath=height varies=yes holders=2")) << text;
    // Name is only the first point's: it varies, held by one of two.
    EXPECT_TRUE(contains(text, "\npath=Name varies=yes holders=1")) << text;

    const std::string filtered = reply("PROP TREE DRAWING WHERE PROP=Name:KERB");
    EXPECT_TRUE(contains(filtered, "matched=1")) << filtered;
    EXPECT_TRUE(contains(filtered, "\npath=Name value=KERB type=text")) << filtered;
}

TEST_F(PropTreeTest, AScopeThatTakesNothingIsReportedNotRefused)
{
    EXPECT_EQ(reply("PROP TREE"), "scope=selection matched=0 under=\"\" entries=0 from=0 shown=0");
}

TEST_F(PropTreeTest, WordsItCannotReadAreRefusedNamingTheWord)
{
    for (const char* line : {"PROP TREE LIMIT 0", "PROP TREE FROM -1", "PROP TREE LIMIT many",
                             "PROP TREE UNDER", "PROP TREE BOGUS"}) {
        const auto result = interpreter.run(line);
        ASSERT_FALSE(result.ok()) << line;
        EXPECT_EQ(result.error().code, ErrorCode::ParseFailure) << line;
    }
    // Two scope words are the shared parser's refusal, in its own terms.
    EXPECT_FALSE(interpreter.run("PROP TREE DRAWING SELECTION").ok());
    const auto result = interpreter.run("PROP TREE BOGUS");
    ASSERT_FALSE(result.ok());
    EXPECT_TRUE(contains(result.error().describe(), "BOGUS")) << result.error().describe();
}
