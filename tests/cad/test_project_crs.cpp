// The project's coordinate system (include/katana/cad/project_crs.hpp): read
// from what a person or an agent types, stored in one form, set as one undo
// step, found in the common list, suggested for a place, and driven from the
// command line with CRS.

#include <gtest/gtest.h>

#include <set>
#include <string>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/project_crs.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::CrsChoice;
using katana::cad::Document;
using katana::core::ErrorCode;

namespace {

std::size_t steps(const Document& document) { return document.history().undoCount(); }

} // namespace

TEST(ProjectCrs, EverySpellingOfACodeIsStoredAsEpsgColonCode)
{
    for (const char* text : {"EPSG:7856", "epsg:7856", "7856", "  EPSG:7856  "}) {
        auto stored = katana::cad::normaliseCoordinateSystem(text);
        ASSERT_TRUE(stored.ok()) << text << ": " << stored.error().describe();
        EXPECT_EQ(*stored, "EPSG:7856") << text;
    }
    auto empty = katana::cad::normaliseCoordinateSystem("   ");
    ASSERT_TRUE(empty.ok());
    EXPECT_EQ(*empty, "") << "blank text is local coordinates";
    auto nonsense = katana::cad::normaliseCoordinateSystem("not a coordinate system");
    ASSERT_FALSE(nonsense.ok());
    EXPECT_EQ(nonsense.error().code, ErrorCode::InvalidCRS);
}

TEST(ProjectCrs, ASystemIsDescribedByItsRegisteredName)
{
    auto mga = katana::cad::describeCoordinateSystem("EPSG:7856");
    ASSERT_TRUE(mga.ok()) << mga.error().describe();
    EXPECT_EQ(mga->id, "EPSG:7856");
    EXPECT_EQ(mga->name, "GDA2020 / MGA zone 56");
    EXPECT_TRUE(mga->projected);
    EXPECT_EQ(mga->units, "metre");
    ASSERT_TRUE(mga->areaOfUse.has_value());
    EXPECT_TRUE(mga->areaOfUse->contains(-33.87, 151.21)) << "Sydney is in MGA zone 56";

    auto wgs84 = katana::cad::describeCoordinateSystem("4326");
    ASSERT_TRUE(wgs84.ok());
    EXPECT_FALSE(wgs84->projected);
    EXPECT_EQ(wgs84->name, "WGS 84");
}

TEST(ProjectCrs, TheCommonListNamesEverySystemAsTheRegisterDoes)
{
    // Checked against PROJ's own database, so a typo in the list - a name or
    // a code - cannot reach a menu.
    std::set<std::string> ids;
    for (const CrsChoice& entry : katana::cad::commonCoordinateSystems()) {
        EXPECT_TRUE(ids.insert(entry.id).second) << entry.id << " is listed twice";
        auto description = katana::cad::describeCoordinateSystem(entry.id);
        ASSERT_TRUE(description.ok()) << entry.id << ": " << description.error().describe();
        EXPECT_EQ(description->name, entry.name) << entry.id;
        EXPECT_FALSE(entry.group.empty()) << entry.id;
    }
    EXPECT_GE(ids.size(), 140u);
}

TEST(ProjectCrs, FindMatchesEveryWordAnywhereInTheEntry)
{
    const auto zone56 = katana::cad::findCoordinateSystems("mga 56");
    ASSERT_EQ(zone56.size(), 2u);
    EXPECT_EQ(zone56[0].id, "EPSG:7856");
    EXPECT_EQ(zone56[1].id, "EPSG:28356");
    EXPECT_EQ(katana::cad::findCoordinateSystems("27700").size(), 1u);
    EXPECT_EQ(katana::cad::findCoordinateSystems("").size(),
              katana::cad::commonCoordinateSystems().size());
    EXPECT_TRUE(katana::cad::findCoordinateSystems("mga 99").empty());
}

TEST(ProjectCrs, APlaceIsSuggestedItsZoneBestFirst)
{
    // Sydney: GDA2020 MGA 56, then GDA94 MGA 56, then WGS 84 UTM 56S, then WGS 84.
    auto sydney = katana::cad::suggestCoordinateSystems(151.21, -33.87);
    ASSERT_TRUE(sydney.ok());
    ASSERT_EQ(sydney->size(), 4u);
    EXPECT_EQ((*sydney)[0].id, "EPSG:7856");
    EXPECT_EQ((*sydney)[1].id, "EPSG:28356");
    EXPECT_EQ((*sydney)[2].id, "EPSG:32756");
    EXPECT_EQ((*sydney)[3].id, "EPSG:4326");
    // Perth is in zone 50.
    auto perth = katana::cad::suggestCoordinateSystems(115.86, -31.95);
    ASSERT_TRUE(perth.ok());
    EXPECT_EQ(perth->front().id, "EPSG:7850");
    // London: no MGA, UTM 30N.
    auto london = katana::cad::suggestCoordinateSystems(-0.12, 51.5);
    ASSERT_TRUE(london.ok());
    ASSERT_EQ(london->size(), 2u);
    EXPECT_EQ(london->front().id, "EPSG:32630");
    // Every suggestion is a real system.
    for (const auto& list : {*sydney, *perth, *london}) {
        for (const CrsChoice& entry : list) {
            auto description = katana::cad::describeCoordinateSystem(entry.id);
            ASSERT_TRUE(description.ok()) << entry.id;
            EXPECT_EQ(description->name, entry.name) << entry.id;
        }
    }
    // Beyond UTM's reach, WGS 84 alone.
    auto pole = katana::cad::suggestCoordinateSystems(10.0, 86.0);
    ASSERT_TRUE(pole.ok());
    ASSERT_EQ(pole->size(), 1u);
    EXPECT_EQ(pole->front().id, "EPSG:4326");
    EXPECT_EQ(katana::cad::suggestCoordinateSystems(190.0, 0.0).error().code,
              ErrorCode::InvalidArgument);
}

TEST(ProjectCrs, SettingItIsOneUndoStepAndARefusalChangesNothing)
{
    Document document;
    ASSERT_TRUE(document.metadata().coordinateSystem.empty());
    const std::size_t before = steps(document);

    ASSERT_TRUE(document.setCoordinateSystem("epsg:7856").ok());
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");
    EXPECT_EQ(steps(document), before + 1);
    EXPECT_EQ(document.history().undoName(), "SET_CRS");

    // The same system again, spelt another way: no step.
    ASSERT_TRUE(document.setCoordinateSystem("7856").ok());
    EXPECT_EQ(steps(document), before + 1);

    // Nonsense is refused and changes nothing.
    const auto refused = document.setCoordinateSystem("GDA3000 zone 99");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidCRS);
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");
    EXPECT_EQ(steps(document), before + 1);

    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.metadata().coordinateSystem, "");
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");

    ASSERT_TRUE(document.setCoordinateSystem("").ok());
    EXPECT_EQ(document.metadata().coordinateSystem, "") << "cleared: local coordinates";
}

TEST(ProjectCrs, TheCrsVerbShowsSetsFindsAndSuggests)
{
    Document document;
    CommandInterpreter interpreter(document);
    auto shown = interpreter.run("CRS");
    ASSERT_TRUE(shown.ok());
    EXPECT_EQ(*shown, "crs id=none (local coordinates)");

    const std::size_t before = steps(document);
    auto set = interpreter.run("CRS SET 7856");
    ASSERT_TRUE(set.ok()) << set.error().describe();
    EXPECT_EQ(*set, "crs id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" kind=\"projected\" units=metre");
    EXPECT_EQ(steps(document), before + 1);
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");

    auto found = interpreter.run("CRS FIND mga 56");
    ASSERT_TRUE(found.ok());
    EXPECT_EQ(*found, "found count=2\n"
                      "choice id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" group=\"Australia - GDA2020 MGA\"\n"
                      "choice id=EPSG:28356 name=\"GDA94 / MGA zone 56\" group=\"Australia - GDA94 MGA\"");

    auto suggested = interpreter.run("CRS SUGGEST 151.21,-33.87");
    ASSERT_TRUE(suggested.ok());
    EXPECT_EQ(suggested->substr(0, suggested->find('\n')), "suggested count=4");
    EXPECT_NE(suggested->find("choice id=EPSG:7856"), std::string::npos);

    auto refused = interpreter.run("CRS SET nowhere");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidCRS);
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");

    auto cleared = interpreter.run("CRS CLEAR");
    ASSERT_TRUE(cleared.ok());
    EXPECT_EQ(*cleared, "crs id=none (local coordinates)");
    EXPECT_NE(CommandInterpreter::helpText().find("CRS SET"), std::string::npos);
}
