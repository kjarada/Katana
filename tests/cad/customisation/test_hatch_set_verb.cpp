// HATCH SET (the Hatch Patterns tab's Save) and STYLE NEW name field value
// (its New Style Using This) through CommandInterpreter::run, as the window's
// command line, katana_cli and katana_mcp type them; and the rows the tab
// shows (include/katana/cad/hatch_pattern_rows.hpp).
//
// Degrees are what is typed and radians what is kept: 45 degrees is pi / 4.
// A pattern's offsets cannot be typed, so SET keeps them by position; one is
// made here through the command HATCH NEW itself runs,
// commands::createHatchPattern.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/hatch_pattern_rows.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/math/numerics.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::core::ErrorCode;
using katana::entity::HatchPattern;
using katana::math::kPi;

namespace {

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << " -> " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    ErrorCode fails(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " unexpectedly succeeded: " << (reply.ok() ? *reply : "");
        return reply.ok() ? ErrorCode::Internal : reply.error().code;
    }
    const HatchPattern& pattern(const std::string& name) const
    {
        static const HatchPattern missing{};
        const HatchPattern* found = document.model().hatchPatterns.find(name);
        EXPECT_NE(found, nullptr) << "no hatch pattern " << name;
        return found != nullptr ? *found : missing;
    }
    std::size_t undoCount() const { return document.history().undoCount(); }
};

} // namespace

TEST(HatchSet, SetReplacesTheFamiliesInDegreesAsOneUndoStep)
{
    Session s;
    s.ok("HATCH NEW brick 45 0.25");
    const std::size_t before = s.undoCount();
    EXPECT_EQ(s.ok("HATCH SET brick 45 0.5 135 0.5"), "hatch pattern brick updated (2 families)");
    EXPECT_EQ(s.undoCount(), before + 1);
    const HatchPattern& brick = s.pattern("brick");
    ASSERT_EQ(brick.families.size(), 2u);
    EXPECT_NEAR(brick.families[0].angle, kPi / 4.0, 1e-15);
    EXPECT_EQ(brick.families[0].spacing, 0.5);
    EXPECT_NEAR(brick.families[1].angle, 3.0 * kPi / 4.0, 1e-15);
    EXPECT_FALSE(brick.solid);

    s.ok("UNDO");
    ASSERT_EQ(s.pattern("brick").families.size(), 1u);
    EXPECT_EQ(s.pattern("brick").families[0].spacing, 0.25);
}

TEST(HatchSet, SetKeepsEachFamilysOffsetByPositionAndTheDescription)
{
    Session s;
    HatchPattern imported;
    imported.name = "paving";
    imported.description = "Pavers, from the council's file";
    imported.families = {{0.0, 1.0, 0.3}, {kPi / 2.0, 1.0, 0.7}};
    ASSERT_TRUE(s.document.execute(katana::commands::createHatchPattern(imported)).ok());
    s.ok("HATCH SET paving 0 2 90 2 45 3");
    const HatchPattern& paving = s.pattern("paving");
    ASSERT_EQ(paving.families.size(), 3u);
    EXPECT_EQ(paving.families[0].offset, 0.3);
    EXPECT_EQ(paving.families[1].offset, 0.7);
    EXPECT_EQ(paving.families[2].offset, 0.0); // a family added has none
    EXPECT_EQ(paving.families[0].spacing, 2.0);
    EXPECT_EQ(paving.description, "Pavers, from the council's file");
}

TEST(HatchSet, SetTurnsAPatternSolidAndBackWithoutKeepingWordsThatAreNoLongerTrue)
{
    Session s;
    s.ok("HATCH NEW brick 45 0.25");
    EXPECT_EQ(s.ok("HATCH SET brick solid"), "hatch pattern brick updated (solid)");
    EXPECT_TRUE(s.pattern("brick").solid);
    EXPECT_TRUE(s.pattern("brick").families.empty());
    EXPECT_EQ(s.pattern("brick").description, "Solid fill"); // as HATCH SOLID calls one

    s.ok("HATCH SOLID fill");
    ASSERT_EQ(s.pattern("fill").description, "Solid fill");
    s.ok("HATCH SET fill 30 1");
    EXPECT_FALSE(s.pattern("fill").solid);
    EXPECT_TRUE(s.pattern("fill").description.empty());
}

TEST(HatchSet, SettingWhatIsAlreadyThereIsNoUndoStep)
{
    Session s;
    s.ok("HATCH NEW brick 45 0.25 135 0.25");
    const std::size_t before = s.undoCount();
    EXPECT_EQ(s.ok("HATCH SET brick 45 0.25 135 0.25"), "hatch pattern brick unchanged");
    EXPECT_EQ(s.undoCount(), before);
}

TEST(HatchSet, SetRefusesWhatCannotBeAPatternAndKeepsTheOneThereWas)
{
    Session s;
    s.ok("HATCH NEW brick 45 0.25");
    EXPECT_EQ(s.fails("HATCH SET nosuch 45 1"), ErrorCode::NotFound);
    EXPECT_EQ(s.fails("HATCH SET brick"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.fails("HATCH SET brick 45"), ErrorCode::InvalidArgument);        // no spacing
    EXPECT_EQ(s.fails("HATCH SET brick 45 1 90"), ErrorCode::InvalidArgument);   // half a family
    // SOLID with a number after it is read as families, and SOLID is no angle.
    EXPECT_EQ(s.fails("HATCH SET brick SOLID 45"), ErrorCode::ParseFailure);
    EXPECT_EQ(s.fails("HATCH SET brick 45 -1"), ErrorCode::InvalidArgument);     // model refuses
    EXPECT_EQ(s.fails("HATCH SET brick 45 x"), ErrorCode::ParseFailure);
    // "none" is what every unhatched layer draws; it cannot be given a fill,
    // which the command refuses as CommandRejected
    // (TablePolicy<HatchPattern>::updatable in table_commands.cpp).
    EXPECT_EQ(s.fails("HATCH SET none 45 1"), ErrorCode::CommandRejected);
    EXPECT_EQ(s.pattern("brick").families.size(), 1u);
    EXPECT_EQ(s.pattern("brick").families[0].spacing, 0.25);
    EXPECT_TRUE(s.pattern("none").drawsNothing());
}

TEST(HatchSet, StyleNewWithAFieldIsAStyleMadeWithItSetInOneStep)
{
    Session s;
    s.ok("HATCH NEW brick 45 0.25");
    const std::size_t before = s.undoCount();
    EXPECT_EQ(s.ok("STYLE NEW \"brick paving\" HATCH brick"), "style brick paving created");
    EXPECT_EQ(s.undoCount(), before + 1);
    const katana::entity::Style* made = s.document.model().styles.find("brick paving");
    ASSERT_NE(made, nullptr);
    EXPECT_EQ(made->hatchPattern, "brick");
    // SET's own reading of a field: a pattern that does not exist is
    // refused, and nothing is made.
    EXPECT_EQ(s.fails("STYLE NEW ghost HATCH nosuch"), ErrorCode::NotFound);
    EXPECT_EQ(s.document.model().styles.find("ghost"), nullptr);
    s.ok("STYLE NEW kerb COLOUR #FF0000");
    ASSERT_NE(s.document.model().styles.find("kerb"), nullptr);
    EXPECT_TRUE(s.document.model().styles.find("kerb")->color.has_value());
    // A second word that is no field is still a name that wanted quotes.
    EXPECT_EQ(s.fails("STYLE NEW TOPO Natural Surface"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.document.model().styles.find("TOPO"), nullptr);
    EXPECT_EQ(s.fails("STYLE NEW lonely HATCH"), ErrorCode::InvalidArgument); // no value
}

// A field whose value is one word takes one word: a second field after it
// was once dropped, the style made and nothing said. NEW and SET alike.
TEST(HatchSet, AWordAfterAOneWordFieldsValueIsRefusedRatherThanDropped)
{
    Session s;
    const std::size_t before = s.undoCount();
    EXPECT_EQ(s.fails("STYLE NEW a colour #FF0000 weight 0.5"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.document.model().styles.find("a"), nullptr);
    EXPECT_EQ(s.undoCount(), before);
    for (const char* line : {"STYLE NEW b weight 0.5 colour #FF0000", "STYLE NEW c hatch - x",
                             "STYLE NEW d symbolsize 1.5 2"}) {
        EXPECT_EQ(s.fails(line), ErrorCode::InvalidArgument) << line;
    }
    s.ok("STYLE NEW e colour #FF0000");
    EXPECT_EQ(s.fails("STYLE SET e weight 0.5 colour #00FF00"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.document.model().styles.find("e")->lineWeight,
              katana::entity::Style{}.lineWeight);
    // A name takes the rest of the line, as it always has.
    s.ok("STYLE SET e description kerb and channel");
    EXPECT_EQ(s.document.model().styles.find("e")->description, "kerb and channel");
}

TEST(HatchPatternRows, EveryPatternIsARowWithItsKindAndWhatUsesIt)
{
    Session s;
    s.ok("HATCH NEW brick 45 0.25 135 0.25");
    s.ok("HATCH SOLID fill");
    s.ok("LAYER NEW site");
    s.ok("LAYER HATCH site brick");
    const std::vector<katana::cad::HatchPatternRow> rows =
        katana::cad::hatchPatternRows(s.document.model());
    ASSERT_EQ(rows.size(), 3u);
    EXPECT_EQ(rows[0].name, "brick");
    EXPECT_EQ(katana::cad::hatchPatternKind(rows[0]), "2 families");
    EXPECT_EQ(rows[0].users.layers, std::vector<std::string>{"site"});
    EXPECT_FALSE(rows[0].builtIn);
    EXPECT_EQ(katana::cad::hatchPatternKind(rows[1]), "solid");
    EXPECT_FALSE(rows[1].users.used());
    EXPECT_EQ(rows[2].name, "none");
    EXPECT_EQ(katana::cad::hatchPatternKind(rows[2]), "draws nothing");
    EXPECT_TRUE(rows[2].builtIn);
}

TEST(HatchPatternRows, TheFamiliesReadBackAsTheDegreesTyped)
{
    Session s;
    // 30 degrees is kept as 0.5235987755982988 radians, which multiplies
    // back to 29.999999999999996; the text is the 30 that was typed.
    s.ok("HATCH NEW slope 30 0.125 120 0.5");
    EXPECT_EQ(katana::cad::hatchFamiliesText(s.pattern("slope")), "30 0.125 120 0.5");
    // Retyped, the text makes the same pattern.
    s.ok("HATCH NEW copy " + katana::cad::hatchFamiliesText(s.pattern("slope")));
    EXPECT_EQ(s.pattern("copy").families, s.pattern("slope").families);
    s.ok("HATCH SOLID fill");
    EXPECT_EQ(katana::cad::hatchFamiliesText(s.pattern("fill")), "");
}

TEST(HatchPatternRows, AFreeNameCountsOnPastTheOnesTaken)
{
    Session s;
    EXPECT_EQ(katana::cad::freeHatchPatternName(s.document.model(), "brick"), "brick");
    s.ok("HATCH NEW brick 45 0.25");
    s.ok("HATCH NEW \"brick 2\" 45 0.5");
    EXPECT_EQ(katana::cad::freeHatchPatternName(s.document.model(), "brick"), "brick 3");
}
