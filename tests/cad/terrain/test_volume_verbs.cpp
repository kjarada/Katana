// The volume verbs (cad/terrain/volume_verbs.hpp, docs/volumes.md) driven
// through CommandInterpreter::run on a Document whose surfaces are set as a
// front end sets them (Document::setSurfaces). Every expected number is a
// solid worked by hand in the comment beside it: slabs between parallel
// planes, a pyramid, a wedge whose area grows with the square of the station.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <functional>
#include <memory>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/terrain/surface_verbs.hpp"
#include "katana/cad/terrain/volume_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_builder.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::core::ErrorCode;
using katana::core::ReplyRecord;
using katana::geometry::Point3;
using katana::terrain::TinInput;
using katana::terrain::TinSurface;

namespace {

// A surface of the points of a square grid over [x0, x0 + size]^2, every
// `step`, at the heights `z` gives: any triangulation of points on a plane is
// that plane, so a plane's grid is the plane exactly.
TinSurface gridSurface(double x0, double y0, double size, double step,
                       const std::function<double(double, double)>& z)
{
    TinInput input;
    const int count = static_cast<int>(std::lround(size / step));
    for (int j = 0; j <= count; ++j) {
        for (int i = 0; i <= count; ++i) {
            const double x = x0 + step * i;
            const double y = y0 + step * j;
            input.points.emplace_back(x, y, z(x, y));
        }
    }
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built.ok() ? std::move(built->surface) : TinSurface{};
}

// The pyramid on [0, 100]^2, apex 30 high at the centre, its four ridges
// breaklines: exactly four faces, (1/3) 100^2 30 = 100 000 m^3 above z = 0.
TinSurface pyramid()
{
    TinInput input;
    input.points = {Point3(0, 0, 0), Point3(100, 0, 0), Point3(100, 100, 0), Point3(0, 100, 0),
                    Point3(50, 50, 30)};
    for (std::size_t corner = 0; corner < 4; ++corner) {
        input.breaklines.push_back({{input.points[corner], input.points[4]}, false});
    }
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built.ok() ? std::move(built->surface) : TinSurface{};
}

class VolumeVerbs : public ::testing::Test {
  protected:
    Document document;
    CommandInterpreter interpreter{document};

    void addSurface(const std::string& name, TinSurface surface)
    {
        katana::terrain::SurfaceStore store = document.surfaces();
        katana::terrain::NamedSurface named;
        named.name = name;
        named.surface = std::make_shared<const TinSurface>(std::move(surface));
        named.source = "test";
        ASSERT_TRUE(store.add(std::move(named)).ok());
        ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
    }

    // The ground z = 10 + 0.05 x and a design 1 m above it, both over
    // [0, 100]^2.
    void addParallelPlanes()
    {
        addSurface("ground", gridSurface(0, 0, 100, 10, [](double x, double) {
                       return 10.0 + 0.05 * x;
                   }));
        addSurface("design", gridSurface(0, 0, 100, 20, [](double x, double) {
                       return 11.0 + 0.05 * x;
                   }));
    }

    std::string ran(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string();
    }

    ErrorCode refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was not refused: " << (reply.ok() ? *reply : "");
        return reply.ok() ? ErrorCode::Internal : reply.error().code;
    }

    static std::vector<ReplyRecord> records(const std::string& reply, const std::string& kind)
    {
        std::vector<ReplyRecord> found;
        std::istringstream lines(reply);
        std::string line;
        while (std::getline(lines, line)) {
            const auto record = katana::core::readReplyRecord(line);
            if (record && !record->words.empty() && record->words.front() == kind) {
                found.push_back(*record);
            }
        }
        return found;
    }

    static std::optional<ReplyRecord> record(const std::string& reply, const std::string& kind)
    {
        const auto all = records(reply, kind);
        return all.empty() ? std::nullopt : std::optional<ReplyRecord>(all.front());
    }

    // The entity id the last drawn thing got: the drawing's largest.
    katana::entity::EntityId lastId() const
    {
        katana::entity::EntityId last = 0;
        document.model().entities.forEach([&](const katana::entity::Entity& entity) {
            last = std::max(last, entity.id);
        });
        return last;
    }
};

std::string value(const std::optional<ReplyRecord>& record, std::string_view key)
{
    return record ? record->value(key).value_or("<none>") : "<no record>";
}

} // namespace

// ---- VOLUME SURFACES ----------------------------------------------------------------------------

TEST_F(VolumeVerbs, TwoParallelPlanesOneMetreApartHoldTheSlabOverTheWholeAndInsideATypedSquare)
{
    addParallelPlanes();
    // The whole: 1 m over 100 x 100.
    const std::string whole = ran("VOLUME SURFACES ground design");
    const auto total = record(whole, "volume");
    EXPECT_EQ(value(total, "method"), "exact");
    EXPECT_EQ(value(total, "fill"), "10000.000");
    EXPECT_EQ(value(total, "cut"), "0.000");
    EXPECT_EQ(value(total, "net"), "10000.000");
    EXPECT_EQ(value(total, "area"), "10000.000");
    // Inside the 10 x 10 square typed corner by corner: 100.
    const std::string inside = ran("VOLUME SURFACES ground design INSIDE 23,27 33,27 33,37 23,37");
    const auto square = record(inside, "volume");
    EXPECT_EQ(value(square, "fill"), "100.000");
    EXPECT_EQ(value(square, "area"), "100.000");
    EXPECT_EQ(value(square, "polygons"), "1");
    EXPECT_EQ(value(record(inside, "polygons"), "used"), "1");
}

TEST_F(VolumeVerbs, EachPolygonAScopeTakesHasItsOwnRecordAndAnOpenLineIsCountedNotUsed)
{
    addParallelPlanes();
    ASSERT_TRUE(interpreter.run("LAYER NEW lots").ok());
    ASSERT_TRUE(interpreter.run("LAYER SET lots").ok());
    ran("PLINE 10,10 20,10 20,20 10,20 CLOSE");   // 100 m2
    ran("PLINE 50,50 70,50 70,60 50,60 CLOSE");   // 200 m2
    ran("LINE 0,90 90,90");                       // bounds nothing
    const std::string reply = ran("VOLUME SURFACES ground design INSIDE FROM LAYERS lots EACH");
    const auto boundary = record(reply, "boundary");
    EXPECT_EQ(value(boundary, "scope"), "layers");
    EXPECT_EQ(value(boundary, "matched"), "3");
    const auto used = record(reply, "polygons");
    EXPECT_EQ(value(used, "used"), "2");
    EXPECT_EQ(value(used, "skipped.open"), "1");
    const auto each = records(reply, "polygon");
    ASSERT_EQ(each.size(), 2u) << reply;
    EXPECT_EQ(each[0].value("fill").value_or(""), "100.000");
    EXPECT_EQ(each[0].value("polygon.area").value_or(""), "100.000");
    EXPECT_TRUE(each[0].value("entity").has_value());
    EXPECT_EQ(each[1].value("fill").value_or(""), "200.000");
    EXPECT_EQ(value(record(reply, "volume"), "fill"), "300.000");
}

TEST_F(VolumeVerbs, AFullEllipseAndAPolylineDrawnBackToItsStartAreVolumePolygons)
{
    // The slab is 1 m thick, so the fill inside a polygon is its area. The
    // square [10, 20]^2 drawn back to its first corner but not closed: 100.
    // The ellipse of semi-axes 20 and 10 about (50, 50): pi 200 = 628.319,
    // less what its chords cut off - each within 1 mm of the curve, so at
    // most its perimeter (Ramanujan: pi (90 - sqrt 3500) = 96.9) x 1 mm,
    // 0.097; 0.15 allows a chord's farthest point to be off its middle.
    addParallelPlanes();
    katana::geometry::Polyline2 drawnBack;
    drawnBack.vertices = {{10, 10}, {20, 10}, {20, 20}, {10, 20}, {10, 10}};
    ASSERT_TRUE(document.execute(katana::commands::createPolyline(drawnBack)).ok());
    const auto square = lastId();
    katana::geometry::Ellipse2 whole;
    whole.center = {50, 50};
    whole.majorAxis = {20, 0};
    whole.ratio = 0.5;
    katana::entity::Entity ellipse;
    ellipse.geometry = whole;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({ellipse})).ok());
    const auto basin = lastId();

    const std::string reply = ran("VOLUME SURFACES ground design INSIDE #" +
                                  std::to_string(square) + " #" + std::to_string(basin) + " EACH");
    const auto each = records(reply, "polygon");
    ASSERT_EQ(each.size(), 2u) << reply;
    EXPECT_EQ(each[0].value("fill").value_or(""), "100.000");
    EXPECT_FALSE(each[0].value("chorded").has_value());
    const auto fill = katana::core::parseFiniteDouble(each[1].value("fill").value_or(""));
    ASSERT_TRUE(fill.has_value()) << reply;
    const double ellipseArea = 200.0 * katana::math::kPi;
    EXPECT_LE(*fill, ellipseArea + 0.0005); // the reply rounds to the millimetre
    EXPECT_GE(*fill, ellipseArea - 0.15);
    EXPECT_EQ(each[1].value("chorded").value_or(""), "yes");
}

TEST_F(VolumeVerbs, OverlappingPolygonsAreRefusedForOneTotalAndWarnedOfWithEach)
{
    addParallelPlanes();
    ran("PLINE 10,10 30,10 30,30 10,30 CLOSE");
    const auto first = lastId();
    ran("PLINE 20,20 40,20 40,40 20,40 CLOSE"); // shares a 10 x 10 corner
    const auto second = lastId();
    const std::string ids = "#" + std::to_string(first) + " #" + std::to_string(second);
    EXPECT_EQ(refused("VOLUME SURFACES ground design INSIDE " + ids), ErrorCode::InvalidGeometry);
    const std::string reply = ran("VOLUME SURFACES ground design INSIDE " + ids + " EACH");
    EXPECT_TRUE(record(reply, "warning").has_value()) << reply;
    // Each is 400; the total counts the shared 100 twice, as it says.
    EXPECT_EQ(value(record(reply, "volume"), "fill"), "800.000");
}

TEST_F(VolumeVerbs, FactorsAreAppliedInTheReplyAndNeverToTheVolumes)
{
    // The ground at 10 and a design z = 10 + 0.1 (x - 50) over [0, 100]^2:
    // cut and fill are each 100 x 0.1 x 50^2 / 2 = 12 500. bulk 1.25 makes
    // the cut 15 625, shrink 1.1 the fill 13 750: net -1875.
    addSurface("ground", gridSurface(0, 0, 100, 25, [](double, double) { return 10.0; }));
    addSurface("design", gridSurface(0, 0, 100, 25, [](double x, double) {
                   return 10.0 + 0.1 * (x - 50.0);
               }));
    const auto total = record(ran("VOLUME SURFACES ground design bulk=1.25 shrink=1.1"), "volume");
    EXPECT_EQ(value(total, "cut"), "12500.000");
    EXPECT_EQ(value(total, "fill"), "12500.000");
    EXPECT_EQ(value(total, "cut.adjusted"), "15625.000");
    EXPECT_EQ(value(total, "fill.adjusted"), "13750.000");
    EXPECT_EQ(value(total, "net.adjusted"), "-1875.000");
    EXPECT_EQ(value(total, "bulk"), "1.25");
}

TEST_F(VolumeVerbs, AScopeThatTakesNoPolygonIsSaidAndNotRefused)
{
    addParallelPlanes();
    ran("LINE 0,0 10,10");
    const std::string reply = ran("VOLUME SURFACES ground design INSIDE FROM DRAWING");
    EXPECT_EQ(value(record(reply, "polygons"), "used"), "0");
    EXPECT_EQ(value(record(reply, "volume"), "ran"), "no");
}

TEST_F(VolumeVerbs, ACsvIsWrittenAndAFileAlreadyThereIsReplacedOnlyWhenAsked)
{
    addParallelPlanes();
    const std::filesystem::path file =
        std::filesystem::temp_directory_path() /
        ("katana_volume_verbs_" + std::to_string(reinterpret_cast<std::uintptr_t>(this)) + ".csv");
    std::filesystem::remove(file);
    const std::string path = katana::core::pathToUtf8(file);
    const std::string reply = ran("VOLUME SURFACES ground design csv=\"" + path + "\"");
    EXPECT_EQ(value(record(reply, "csv"), "rows"), "1");
    std::ifstream in(file, std::ios::binary);
    std::string header;
    std::getline(in, header);
    // The eight columns where they always were, then what the reply also says:
    // the areas (the whole 10 000 m2 is fill, the design 1 m above the ground
    // everywhere) and the method and the surfaces that made the table.
    EXPECT_EQ(header, "record,polygon,entity,polygon_area,area,cut,fill,net,cut_area,fill_area,"
                      "method,base,top\r");
    std::string row;
    std::getline(in, row);
    EXPECT_EQ(row, "total,,,,10000.000,0.000,10000.000,10000.000,0.000,10000.000,exact,ground,"
                   "design\r");
    in.close();
    EXPECT_EQ(refused("VOLUME SURFACES ground design csv=\"" + path + "\""),
              ErrorCode::AlreadyExists);
    ran("VOLUME SURFACES ground design csv=\"" + path + "\" OVERWRITE");
    std::filesystem::remove(file);
}

TEST_F(VolumeVerbs, WhatCannotBeMeasuredIsRefusedSayingWhy)
{
    addParallelPlanes();
    addSurface("far", gridSurface(500, 500, 10, 10, [](double, double) { return 1.0; }));
    EXPECT_EQ(refused("VOLUME SURFACES ground nothing"), ErrorCode::NotFound);
    EXPECT_EQ(refused("VOLUME SURFACES ground far"), ErrorCode::InvalidGeometry);
    ran("LINE 0,0 10,10");
    EXPECT_EQ(refused("VOLUME SURFACES ground design INSIDE #" + std::to_string(lastId())),
              ErrorCode::InvalidGeometry); // a line is no polygon
    EXPECT_EQ(refused("VOLUME SURFACES ground design INSIDE #99999"), ErrorCode::NotFound);
    EXPECT_EQ(refused("VOLUME SURFACES ground design INSIDE 0,0 10,0"), ErrorCode::ParseFailure);
    EXPECT_EQ(refused("VOLUME SURFACES ground design EACH"), ErrorCode::ParseFailure);
    EXPECT_EQ(refused("VOLUME SURFACES ground design interval=5"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refused("VOLUME SURFACES ground design bulk=0"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refused("VOLUME SURFACES ground"), ErrorCode::ParseFailure);
    EXPECT_EQ(refused("VOLUME SIDEWAYS ground design"), ErrorCode::ParseFailure);
    // A polygon wholly off the surfaces measures nothing, which is said.
    EXPECT_EQ(refused("VOLUME SURFACES ground design INSIDE 200,200 210,200 210,210"),
              ErrorCode::InvalidGeometry);
}

namespace {

// A file's lines (CRLF removed) or none.
std::vector<std::string> linesOf(const std::filesystem::path& file)
{
    std::vector<std::string> lines;
    std::ifstream in(file, std::ios::binary);
    std::string line;
    while (std::getline(in, line)) {
        if (!line.empty() && line.back() == '\r') {
            line.pop_back();
        }
        lines.push_back(line);
    }
    return lines;
}

std::filesystem::path scratchFile(const void* owner, const char* stem)
{
    return std::filesystem::temp_directory_path() /
           (std::string("katana_volume_verbs_") + stem + "_" +
            std::to_string(reinterpret_cast<std::uintptr_t>(owner)) + ".csv");
}

} // namespace

TEST_F(VolumeVerbs, ATableNamesItsMethodAndInputsAndCarriesTheAreasLevelsAndCellsOfTheReply)
{
    // Parallel planes 1 m apart over [0, 100]^2, ground z = 10 + 0.05 x. The
    // columns after the eighth say what the reply says: for the exact surfaces
    // 0 of cut area and the whole 10 000 m2 of fill area, which design 1 m
    // above the ground everywhere gives; for a level the above and below, and
    // the level.
    addParallelPlanes();
    const auto file = scratchFile(this, "columns");
    std::filesystem::remove(file);
    const std::string path = katana::core::pathToUtf8(file);
    const auto csv = [&](const std::string& line) {
        std::filesystem::remove(file);
        ran(line + " csv=\"" + path + "\"");
        return linesOf(file);
    };

    // The exact surfaces.
    auto surfaces = csv("VOLUME SURFACES ground design");
    ASSERT_EQ(surfaces.size(), 2u);
    EXPECT_EQ(surfaces[0], "record,polygon,entity,polygon_area,area,cut,fill,net,cut_area,"
                           "fill_area,method,base,top");
    EXPECT_EQ(surfaces[1], "total,,,,10000.000,0.000,10000.000,10000.000,0.000,10000.000,exact,"
                           "ground,design");

    // The grid: 10 m cells, a hundred of them, each 1 m deep: 10 000 m3 -
    // the same numbers as the exact file, and the method and the cell are what
    // tell the two apart.
    auto grid = csv("VOLUME GRID ground design cell=10");
    ASSERT_EQ(grid.size(), 2u);
    EXPECT_EQ(grid[0], "record,polygon,entity,polygon_area,area,cut,fill,net,cells,method,base,"
                       "top,cell");
    EXPECT_EQ(grid[1], "total,,,,10000.000,0.000,10000.000,10000.000,100,grid,ground,design,10");

    // A level: z = 10 + 0.05 x crosses 11 at x = 20. Above it, x from 20 to
    // 100: 100 x (0.025 (100^2 - 20^2) - 80) = 100 x 160 = 16 000 m3 over
    // 80 x 100 = 8000 m2; below it, 100 x (20 - 0.025 x 20^2) = 100 x 10 =
    // 1000 m3 over 2000 m2; net 1000 - 16000 = -15 000.
    auto level = csv("VOLUME DATUM ground 11");
    ASSERT_EQ(level.size(), 2u);
    EXPECT_EQ(level[0], "record,polygon,entity,polygon_area,area,cut,fill,net,cut_area,fill_area,"
                        "above,below,method,surface,level");
    EXPECT_EQ(level[1], "total,,,,10000.000,16000.000,1000.000,-15000.000,8000.000,2000.000,"
                        "16000.000,1000.000,exact,ground,11");

    // The factors are columns too, after the quantities they adjust.
    auto factored = csv("VOLUME SURFACES ground design bulk=1.25 shrink=1.1");
    ASSERT_EQ(factored.size(), 2u);
    EXPECT_EQ(factored[0], "record,polygon,entity,polygon_area,area,cut,fill,net,cut_adjusted,"
                           "fill_adjusted,net_adjusted,cut_area,fill_area,method,base,top,bulk,"
                           "shrink");
    EXPECT_EQ(factored[1], "total,,,,10000.000,0.000,10000.000,10000.000,0.000,11000.000,"
                           "11000.000,0.000,10000.000,exact,ground,design,1.25,1.1");
    std::filesystem::remove(file);
}

TEST_F(VolumeVerbs, ASectionTableSaysWhetherItsVolumesAreEndAreaOrPrismoidalAndWhatMadeThem)
{
    // Sections every 50 m along y = 50, 10 m each side of the parallel planes
    // 1 m apart: 20 m2 on each of the three, so 50 x 20 = 1000 m3 a panel by
    // either formula (Simpson's rule on a constant is the constant). With
    // method=prismoidal the plain cut and fill columns hold the prismoidal
    // volumes - which the method column is the only thing to say - and the mass
    // ordinate is the cut x bulk less the fill x shrink: 0 - 1000 x 1.5, then
    // -3000.
    addParallelPlanes();
    ran("ALIGN NEW cl 0,50 100,50");
    const auto file = scratchFile(this, "sections");
    std::filesystem::remove(file);
    const std::string path = katana::core::pathToUtf8(file);
    ran("VOLUME SECTIONS ground design ALIGNMENT cl interval=50 left=10 right=10 "
        "method=prismoidal bulk=1.2 shrink=1.5 csv=\"" + path + "\"");
    const auto lines = linesOf(file);
    ASSERT_EQ(lines.size(), 4u);
    EXPECT_EQ(lines[0], "station,key,cut_area,fill_area,width,cut,fill,cut_total,fill_total,mass,"
                        "method,base,top,alignment,interval,left,right,bulk,shrink");
    EXPECT_EQ(lines[1], "0.000,yes,0.000,20.000,20.000,,,,,,prismoidal,ground,design,cl,50,"
                        "10.000,10.000,1.2,1.5");
    EXPECT_EQ(lines[2], "50.000,no,0.000,20.000,20.000,0.000,1000.000,0.000,1000.000,-1500.000,"
                        "prismoidal,ground,design,cl,50,10.000,10.000,1.2,1.5");
    EXPECT_EQ(lines[3], "100.000,yes,0.000,20.000,20.000,0.000,1000.000,0.000,2000.000,-3000.000,"
                        "prismoidal,ground,design,cl,50,10.000,10.000,1.2,1.5");
    std::filesystem::remove(file);
}

TEST_F(VolumeVerbs, ACsvWithAScopeThatTakesNoPolygonWritesNothingAndSaysSo)
{
    // The file a run before left is as it was - even with OVERWRITE, as nothing
    // ran - and the reply names the file and says it was not written, where it
    // used to say nothing of it.
    addParallelPlanes();
    const auto file = scratchFile(this, "norun");
    {
        std::ofstream out(file, std::ios::binary | std::ios::trunc);
        out << "OLD TABLE FROM AN EARLIER RUN\r\n";
    }
    const std::string path = katana::core::pathToUtf8(file);
    const std::string reply = ran("VOLUME SURFACES ground design INSIDE FROM AREA 500,500,600,600 csv=\"" +
                                  path + "\" OVERWRITE");
    EXPECT_EQ(value(record(reply, "volume"), "ran"), "no");
    const auto written = record(reply, "csv");
    ASSERT_TRUE(written.has_value()) << reply;
    EXPECT_EQ(value(written, "file"), path);
    EXPECT_EQ(value(written, "written"), "no");
    EXPECT_FALSE(written->value("rows").has_value());
    const auto kept = linesOf(file);
    ASSERT_EQ(kept.size(), 1u);
    EXPECT_EQ(kept[0], "OLD TABLE FROM AN EARLIER RUN");
    // A file that was not there is not made.
    std::filesystem::remove(file);
    const std::string again = ran("VOLUME SURFACES ground design INSIDE FROM AREA 500,500,600,600 csv=\"" +
                                  path + "\"");
    EXPECT_EQ(value(record(again, "csv"), "written"), "no");
    EXPECT_FALSE(std::filesystem::exists(file));
    // Asked of nothing, a line with no csv= says nothing of a file.
    EXPECT_FALSE(record(ran("VOLUME SURFACES ground design INSIDE FROM AREA 500,500,600,600"), "csv")
                     .has_value());
}

TEST_F(VolumeVerbs, ATotalSaysWhichBaseAStockpileWasMeasuredAgainstAndWhichHeightsItsToeUsed)
{
    // The pyramid [0, 100]^2, apex 30 high at the centre: its height is
    // 0.6 min(x, y) on the lower-left quarter and 30 - 0.6 (max(x, y) - 50) on
    // the upper right. Against base=lowest the level is the lowest point on the
    // boundary: the square [10, 40]^2 has it at the corner (10, 10), 0.6 x 10 =
    // 6, and [55, 80]^2 at (80, 80), 30 - 0.6 x 30 = 12. A toe typed with every
    // height is its own; one with none uses the ground's. The total of ONE
    // polygon has what its EACH record has; of two that differ it has the
    // fields they share and EACH has the rest.
    addSurface("pile", pyramid());
    const std::string lowerLeft = "10,10 40,10 40,40 10,40";
    const std::string upperRight = "55,55 80,55 80,80 55,80";
    EXPECT_EQ(value(record(ran("VOLUME STOCKPILE pile INSIDE " + lowerLeft + " base=lowest"),
                           "volume"),
                    "lowest"),
              "6.000");
    EXPECT_EQ(value(record(ran("VOLUME STOCKPILE pile INSIDE " + upperRight + " base=lowest"),
                           "volume"),
                    "lowest"),
              "12.000");
    EXPECT_EQ(value(record(ran("VOLUME STOCKPILE pile INSIDE " + lowerLeft), "volume"), "heights"),
              "surface");
    EXPECT_EQ(value(record(ran("VOLUME STOCKPILE pile INSIDE 10,10,6 40,10,6 40,40,24 10,40,6"),
                           "volume"),
                    "heights"),
              "own");
    // Two polygons that differ in the level they found: no one value for the
    // total, and each polygon's own with EACH.
    ran("PLINE " + lowerLeft + " CLOSE");
    const auto first = lastId();
    ran("PLINE " + upperRight + " CLOSE");
    const auto second = lastId();
    const std::string pair = " INSIDE #" + std::to_string(first) + " #" + std::to_string(second);
    const std::string differing = ran("VOLUME STOCKPILE pile" + pair + " base=lowest");
    EXPECT_FALSE(record(differing, "volume")->value("lowest").has_value()) << differing;
    const std::string each = ran("VOLUME STOCKPILE pile" + pair + " base=lowest EACH");
    const auto polygons = records(each, "polygon");
    ASSERT_EQ(polygons.size(), 2u) << each;
    EXPECT_EQ(polygons[0].value("lowest").value_or(""), "6.000");
    EXPECT_EQ(polygons[1].value("lowest").value_or(""), "12.000");
    // Two that agree: the shared field is on the total.
    EXPECT_EQ(value(record(ran("VOLUME STOCKPILE pile" + pair), "volume"), "heights"), "surface");
}

TEST_F(VolumeVerbs, ARefusalThatTwoSurfacesShareNoGroundNamesWhatEachCovers)
{
    // a is the triangle (0,0) (60,0) (0,60), b the triangle (100,100) (40,100)
    // (100,40): the boxes meet on [40, 60]^2 over triangles that do not. The
    // refusal names each surface and its extent, for every form, so a person
    // who has drawn one in the wrong coordinates sees it.
    TinInput first;
    first.points = {Point3(0, 0, 0), Point3(60, 0, 0), Point3(0, 60, 0)};
    addSurface("a", katana::terrain::buildTin(first)->surface);
    TinInput second;
    second.points = {Point3(100, 100, 1), Point3(40, 100, 1), Point3(100, 40, 1)};
    addSurface("b", katana::terrain::buildTin(second)->surface);
    ran("ALIGN NEW cl 0,50 100,50");
    const std::string a = "a covers 0.000,0.000,60.000,60.000";
    const std::string b = "b covers 40.000,40.000,100.000,100.000";
    for (const char* line : {"VOLUME SURFACES a b", "VOLUME GRID a b cell=1",
                             "VOLUME SECTIONS a b ALIGNMENT cl", "SURFACE DIFFERENCE a b",
                             "SURFACE ENVELOPE a b MAX"}) {
        const auto reply = interpreter.run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidGeometry) << line;
        EXPECT_NE(reply.error().message.find(a), std::string::npos)
            << line << ": " << reply.error().message;
        EXPECT_NE(reply.error().message.find(b), std::string::npos)
            << line << ": " << reply.error().message;
    }
    // A polygon off a surface names that surface's extent.
    for (const char* line : {"VOLUME DATUM a 1 INSIDE 200,200 210,200 210,210",
                             "VOLUME STOCKPILE a INSIDE 200,200 210,200 210,210 level=1"}) {
        const auto reply = interpreter.run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_NE(reply.error().message.find(a), std::string::npos)
            << line << ": " << reply.error().message;
    }
    // Boxes that do not meet name them as before.
    addSurface("far", gridSurface(500, 500, 10, 10, [](double, double) { return 1.0; }));
    const auto apart = interpreter.run("VOLUME SURFACES a far");
    ASSERT_FALSE(apart.ok());
    EXPECT_NE(apart.error().message.find("far covers 500.000,500.000,510.000,510.000"),
              std::string::npos)
        << apart.error().message;
}

// ---- VOLUME DATUM and STOCKPILE --------------------------------------------------------------------

TEST_F(VolumeVerbs, ATiltedPlaneAgainstALevelInsideARectangleIsCutEastAndFillWest)
{
    // z = 10 + 0.05 x against the level 11 inside [0, 40] x [0, 10]: the
    // plane crosses 11 at x = 20. Above: 10 x the integral of 0.05 x - 1
    // from 20 to 40 = 10 x 10 = 100; below the same, from 0 to 20.
    addParallelPlanes();
    const auto total = record(ran("VOLUME DATUM ground 11 INSIDE 0,0 40,0 40,10 0,10"), "volume");
    EXPECT_EQ(value(total, "cut"), "100.000");
    EXPECT_EQ(value(total, "above"), "100.000");
    EXPECT_EQ(value(total, "fill"), "100.000");
    EXPECT_EQ(value(total, "below"), "100.000");
    EXPECT_EQ(value(total, "net"), "0.000");
    EXPECT_EQ(value(total, "area"), "400.000");
    EXPECT_EQ(value(total, "level"), "11");
    EXPECT_EQ(refused("VOLUME DATUM ground high"), ErrorCode::ParseFailure);
}

TEST_F(VolumeVerbs, APyramidStockpileIsAThirdOfItsBaseTimesItsHeightAgainstEachKindOfBase)
{
    // The pyramid's toe is the square [0, 100]^2 at 0: 100 000 m^3 above a
    // base of the toe's heights, and above its lowest point (0) the same;
    // above level -1, 10 000 m^2 more of 1 m: 110 000.
    addSurface("pile", pyramid());
    const std::string toe = " INSIDE 0,0 100,0 100,100 0,100";
    const auto boundary = record(ran("VOLUME STOCKPILE pile" + toe), "volume");
    EXPECT_EQ(value(boundary, "above"), "100000.000");
    EXPECT_EQ(value(boundary, "cut"), "100000.000");
    EXPECT_EQ(value(boundary, "below"), "0.000");
    EXPECT_EQ(value(boundary, "base"), "boundary");
    const auto lowest = record(ran("VOLUME STOCKPILE pile" + toe + " base=lowest"), "volume");
    EXPECT_EQ(value(lowest, "above"), "100000.000");
    const auto level = record(ran("VOLUME STOCKPILE pile" + toe + " level=-1"), "volume");
    EXPECT_EQ(value(level, "above"), "110000.000");
    EXPECT_EQ(value(level, "base"), "level");
    EXPECT_EQ(refused("VOLUME STOCKPILE pile"), ErrorCode::ParseFailure);
    EXPECT_EQ(refused("VOLUME STOCKPILE pile" + toe + " base=middle"), ErrorCode::InvalidArgument);
}

// ---- VOLUME SECTIONS and GRID -----------------------------------------------------------------------

TEST_F(VolumeVerbs, SectionsAlongAnAlignmentGiveTheEndAreaThePrismoidalAndTheMassHaul)
{
    // The wedge: the ground z = 0 and over it the triangle (0,0), (100,-10),
    // (100,10) of z = 0.1 x, so a section at s is s / 5 wide and 0.1 s deep,
    // s^2 / 50. Every 10 m: end areas 6700, prismoidal 20 000 / 3 = 6666.667,
    // the correction 33.333. All fill, so the mass ordinate falls to -6700.
    addSurface("ground", gridSurface(-20, -60, 140, 140, [](double, double) { return 0.0; }));
    TinInput wedge;
    wedge.points = {Point3(0, 0, 0), Point3(100, -10, 10), Point3(100, 10, 10)};
    addSurface("wedge", katana::terrain::buildTin(wedge)->surface);
    ran("ALIGN NEW cl 0,0 100,0");
    const std::string reply = ran(
        "VOLUME SECTIONS ground wedge ALIGNMENT cl interval=10 left=20 right=20 method=both");
    const auto sections = records(reply, "section");
    ASSERT_EQ(sections.size(), 11u) << reply;
    EXPECT_EQ(sections.front().value("station").value_or(""), "0.000");
    EXPECT_EQ(sections.front().value("key").value_or(""), "yes");
    EXPECT_EQ(sections[5].value("fill.area").value_or(""), "50.000"); // 50^2 / 50
    EXPECT_EQ(sections.back().value("mass").value_or(""), "-6700.000");
    EXPECT_EQ(sections.back().value("fill.total").value_or(""), "6700.000");
    const auto totals = records(reply, "volume");
    ASSERT_EQ(totals.size(), 2u);
    EXPECT_EQ(totals[0].value("method").value_or(""), "endarea");
    EXPECT_EQ(totals[0].value("fill").value_or(""), "6700.000");
    EXPECT_EQ(totals[1].value("method").value_or(""), "prismoidal");
    EXPECT_EQ(totals[1].value("fill").value_or(""), "6666.667");
    EXPECT_EQ(totals[1].value("correction.fill").value_or(""), "33.333");
    // The exact overlay agrees with the prismoidal: 0.1 x 1000 x 200 / 3.
    EXPECT_EQ(value(record(ran("VOLUME SURFACES ground wedge"), "volume"), "fill"), "6666.667");

    EXPECT_EQ(refused("VOLUME SECTIONS ground wedge ALIGNMENT nowhere"), ErrorCode::NotFound);
    EXPECT_EQ(refused("VOLUME SECTIONS ground wedge ALIGNMENT cl method=simpson"),
              ErrorCode::InvalidArgument);
    EXPECT_EQ(refused("VOLUME SECTIONS ground wedge ALIGNMENT cl interval=0"),
              ErrorCode::InvalidArgument);
}

TEST_F(VolumeVerbs, ASlabMeasuredNorthwardsIsAsWholeAsEastwardsWhereItsEndSectionsLieOnItsEdges)
{
    // 1 m over [0, 100]^2 is 10 000 m3. The sections at stations 0 and 100 of a
    // north-going alignment lie on the slab's south and north edges, and the
    // tilt of cos(pi/2) in their ends used to halve them: 9500.
    addSurface("lo", gridSurface(0, 0, 100, 100, [](double, double) { return 10.0; }));
    addSurface("hi", gridSurface(0, 0, 100, 100, [](double, double) { return 11.0; }));
    ran("ALIGN NEW north 50,0 50,100");
    ran("ALIGN NEW east 0,50 100,50");
    for (const char* alignment : {"north", "east"}) {
        const std::string reply =
            ran(std::string("VOLUME SECTIONS lo hi ALIGNMENT ") + alignment + " interval=10");
        const auto sections = records(reply, "section");
        ASSERT_EQ(sections.size(), 11u) << alignment;
        EXPECT_EQ(sections.front().value("width").value_or(""), "100.000") << alignment;
        EXPECT_EQ(sections.back().value("width").value_or(""), "100.000") << alignment;
        EXPECT_EQ(value(record(reply, "volume"), "fill"), "10000.000") << alignment;
        EXPECT_EQ(value(record(reply, "volume"), "sections.empty"), "0") << alignment;
    }
}

TEST_F(VolumeVerbs, SectionsRoundACurveAreRefusedUntilTheirReachIsShortOfItsRadius)
{
    // A 1 m slab over [0, 200]^2 is 40 000 m3 (VOLUME SURFACES). The alignment
    // 20,20 to 180,20 to 180,180 with a curve of radius 40 is 240 + 20 pi long;
    // with sections across all the common ground (the default, 283 m) the
    // sections cross one another and the sum was 62 101, which is no volume
    // there is. A band 15 m either side is 30 x (240 + 20 pi) = 9084.956,
    // whatever the interval.
    addSurface("lo", gridSurface(0, 0, 200, 200, [](double, double) { return 10.0; }));
    addSurface("hi", gridSurface(0, 0, 200, 200, [](double, double) { return 11.0; }));
    ran("ALIGN NEW cl 20,20 180,20 180,180");
    ran("ALIGN SET cl 1 40");
    EXPECT_EQ(value(record(ran("VOLUME SURFACES lo hi"), "volume"), "fill"), "40000.000");
    EXPECT_EQ(refused("VOLUME SECTIONS lo hi ALIGNMENT cl"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refused("VOLUME SECTIONS lo hi ALIGNMENT cl interval=1 left=100 right=100"),
              ErrorCode::InvalidArgument);
    for (const char* interval : {"10", "1", "0.1"}) {
        const auto band = record(ran(std::string("VOLUME SECTIONS lo hi ALIGNMENT cl interval=") +
                                     interval + " left=15 right=15"),
                                 "volume");
        EXPECT_EQ(value(band, "fill"), "9084.956") << interval;
    }
}

TEST_F(VolumeVerbs, SectionsOfAnAlignmentThatRunsClearOfTheSurfacesAreRefusedNamingEverything)
{
    addParallelPlanes(); // [0, 100]^2
    ran("ALIGN NEW away 500,500 600,500");
    const auto reply = interpreter.run("VOLUME SECTIONS ground design ALIGNMENT away");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::InvalidGeometry);
    // Which alignment, which surfaces and what each covers.
    for (const char* named : {"alignment away", "ground covers 0.000,0.000,100.000,100.000",
                              "design covers 0.000,0.000,100.000,100.000"}) {
        EXPECT_NE(reply.error().message.find(named), std::string::npos)
            << named << " in: " << reply.error().message;
    }
}

TEST_F(VolumeVerbs, TheGridOverstatesThePyramidByTwentyCellsSquared)
{
    // terrain's GridVolume test works it: on 5 m cells the midpoint rule is
    // 20 x 5^2 = 500 over the exact 100 000, on 400 cells.
    addSurface("flat", gridSurface(0, 0, 100, 100, [](double, double) { return 0.0; }));
    addSurface("pile", pyramid());
    const auto grid = record(ran("VOLUME GRID flat pile cell=5"), "volume");
    EXPECT_EQ(value(grid, "method"), "grid");
    EXPECT_EQ(value(grid, "fill"), "100500.000");
    EXPECT_EQ(value(grid, "cells"), "400");
    EXPECT_EQ(refused("VOLUME GRID flat pile"), ErrorCode::InvalidArgument); // no cell=
}

TEST_F(VolumeVerbs, LotsThatShareAnEdgeOnAGridLineAreCountedOnceByTheGridAsByTheExactMethod)
{
    // 1 m over [0, 50]^2, 1 m cells with centres at k + 0.5. Lots [5, 25.5] x
    // [5, 45] and [25.5, 45] x [5, 45] share the edge x = 25.5, a line of
    // centres: 840 + 800 = 1640 cells when each is measured on its own and
    // summed, the 40 on the edge twice. Their union [5, 45]^2 holds 40 x 40 =
    // 1600 cells, and 1600 m2 is also what the exact method gives.
    addSurface("lo", gridSurface(0, 0, 50, 50, [](double, double) { return 10.0; }));
    addSurface("hi", gridSurface(0, 0, 50, 50, [](double, double) { return 11.0; }));
    ran("PLINE 5,5 25.5,5 25.5,45 5,45 CLOSE");
    const auto lotA = lastId();
    ran("PLINE 25.5,5 45,5 45,45 25.5,45 CLOSE");
    const auto lotB = lastId();
    ran("PLINE 5,5 45,5 45,45 5,45 CLOSE");
    const auto lots = lastId();
    const std::string pair = " INSIDE #" + std::to_string(lotA) + " #" + std::to_string(lotB);
    const auto grid = record(ran("VOLUME GRID lo hi cell=1" + pair), "volume");
    EXPECT_EQ(value(grid, "cells"), "1600");
    EXPECT_EQ(value(grid, "fill"), "1600.000");
    EXPECT_EQ(value(grid, "area"), "1600.000");
    EXPECT_EQ(value(grid, "polygon.area"), "1600.000");
    const auto whole =
        record(ran("VOLUME GRID lo hi cell=1 INSIDE #" + std::to_string(lots)), "volume");
    EXPECT_EQ(value(whole, "cells"), "1600");
    EXPECT_EQ(value(record(ran("VOLUME SURFACES lo hi" + pair), "volume"), "fill"), "1600.000");
    // Other cells on the same lots: the two lots together always hold the
    // cells of their union, none twice.
    for (const char* cell : {"2", "0.5", "5"}) {
        const auto pairedLots =
            record(ran(std::string("VOLUME GRID lo hi cell=") + cell + pair), "volume");
        const auto wholeLots = record(
            ran(std::string("VOLUME GRID lo hi cell=") + cell + " INSIDE #" + std::to_string(lots)),
            "volume");
        EXPECT_EQ(value(pairedLots, "cells"), value(wholeLots, "cells")) << cell;
        EXPECT_EQ(value(pairedLots, "fill"), value(wholeLots, "fill")) << cell;
    }
}

TEST_F(VolumeVerbs, OverlappingPolygonsOnTheGridAreOneUnionAndSaidSoAndEachIsRefusedByName)
{
    // [5, 30] x [5, 45] and [20, 45] x [5, 45] overlap on [20, 30]. The grid
    // counts each cell once, 1600 of them; the exact method refuses the pair
    // without EACH. polygon.area= is the two polygons' own, 1000 + 1000 cells
    // of 1 m2 - the overlap twice, which the warning says.
    addSurface("lo", gridSurface(0, 0, 50, 50, [](double, double) { return 10.0; }));
    addSurface("hi", gridSurface(0, 0, 50, 50, [](double, double) { return 11.0; }));
    ran("PLINE 5,5 30,5 30,45 5,45 CLOSE");
    const auto first = lastId();
    ran("PLINE 20,5 45,5 45,45 20,45 CLOSE");
    const auto second = lastId();
    const std::string pair = " INSIDE #" + std::to_string(first) + " #" + std::to_string(second);
    const std::string reply = ran("VOLUME GRID lo hi cell=1" + pair);
    const auto grid = record(reply, "volume");
    EXPECT_EQ(value(grid, "cells"), "1600");
    EXPECT_EQ(value(grid, "fill"), "1600.000");
    EXPECT_EQ(value(grid, "polygon.area"), "2000.000");
    EXPECT_TRUE(record(reply, "warning").has_value()) << reply;
    EXPECT_EQ(refused("VOLUME SURFACES lo hi" + pair), ErrorCode::InvalidGeometry);

    // EACH is refused by GRID, and the refusal does not send a person to a
    // form that refuses it: it names the forms that take it.
    const auto each = interpreter.run("VOLUME GRID lo hi cell=1" + pair + " EACH");
    ASSERT_FALSE(each.ok());
    EXPECT_EQ(each.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(each.error().message.find("DATUM and STOCKPILE take it"), std::string::npos)
        << each.error().message;
}

// ---- SURFACE DIFFERENCE and ENVELOPE --------------------------------------------------------------

TEST_F(VolumeVerbs, TheDifferenceIsANewCutFillSurfaceInOneUndoStep)
{
    addParallelPlanes();
    const std::string preview = ran("SURFACE DIFFERENCE ground design PREVIEW");
    EXPECT_EQ(value(record(preview, "surface"), "preview"), "yes");
    EXPECT_EQ(document.surfaces().find("design-ground"), nullptr);

    const std::string reply = ran("SURFACE DIFFERENCE ground design");
    const auto made = record(reply, "surface");
    EXPECT_EQ(value(made, "name"), "design-ground");
    EXPECT_EQ(value(made, "zmin"), "1.000");
    EXPECT_EQ(value(made, "zmax"), "1.000");
    const katana::terrain::NamedSurface* difference = document.surfaces().find("design-ground");
    ASSERT_NE(difference, nullptr);
    EXPECT_EQ(difference->style, "Cut Fill");
    // Integrated at 0 it is the comparison's fill.
    EXPECT_EQ(value(record(ran("VOLUME DATUM design-ground 0"), "volume"), "above"), "10000.000");

    EXPECT_EQ(refused("SURFACE DIFFERENCE ground design NAME design-ground"),
              ErrorCode::AlreadyExists);
    ran("UNDO");
    EXPECT_EQ(document.surfaces().find("design-ground"), nullptr);
}

TEST_F(VolumeVerbs, TheUpperEnvelopeOfCrossingPlanesIsTheHigherOfThem)
{
    // max(10, 10 + 0.1 (x - 50)) over [0, 100]^2 stands 12 500 above 10:
    // the triangle of 0.1 x 50^2 / 2 along x, 100 along y.
    addSurface("ground", gridSurface(0, 0, 100, 25, [](double, double) { return 10.0; }));
    addSurface("design", gridSurface(0, 0, 100, 25, [](double x, double) {
                   return 10.0 + 0.1 * (x - 50.0);
               }));
    const std::string reply = ran("SURFACE ENVELOPE ground design MAX NAME top");
    EXPECT_EQ(value(record(reply, "envelope"), "side"), "max");
    const auto volume = record(ran("VOLUME DATUM top 10"), "volume");
    EXPECT_EQ(value(volume, "above"), "12500.000");
    EXPECT_EQ(value(volume, "below"), "0.000");
    EXPECT_EQ(refused("SURFACE ENVELOPE ground design HIGHEST"), ErrorCode::ParseFailure);
    EXPECT_EQ(refused("SURFACE ENVELOPE ground design"), ErrorCode::ParseFailure);
}

TEST_F(VolumeVerbs, TheEnvelopeIsDrawnInTheDefaultStyleNotTheCutFillMapOfADifference)
{
    // An envelope holds real levels (here 10 to 15), not a difference about 0:
    // in the Cut Fill style, whose ramp ends at +2, every face past the top stop
    // would be drawn in the one colour of fill. Only the difference is a map.
    addSurface("ground", gridSurface(0, 0, 100, 25, [](double, double) { return 10.0; }));
    addSurface("design", gridSurface(0, 0, 100, 25, [](double x, double) {
                   return 10.0 + 0.1 * (x - 50.0);
               }));
    const std::string upper = ran("SURFACE ENVELOPE ground design MAX NAME upper");
    const katana::terrain::NamedSurface* envelope = document.surfaces().find("upper");
    ASSERT_NE(envelope, nullptr);
    EXPECT_TRUE(envelope->style.empty()) << envelope->style;
    EXPECT_EQ(value(record(upper, "surface"), "style"), document.surfaceStyles().defaultStyle);
    EXPECT_EQ(value(record(upper, "surface"), "zmax"), "15.000");

    const std::string lower = ran("SURFACE ENVELOPE ground design MIN NAME lower");
    EXPECT_NE(value(record(lower, "surface"), "style"), "Cut Fill");

    // The difference of the same two is the map.
    const std::string map = ran("SURFACE DIFFERENCE ground design NAME map");
    EXPECT_EQ(value(record(map, "surface"), "style"), "Cut Fill");
    EXPECT_EQ(document.surfaces().find("map")->style, "Cut Fill");
}

TEST_F(VolumeVerbs, ASurfaceTheseVerbsMakeIsReportedAsSurfaceInfoReportsItWithOrWithoutCutFill)
{
    // One surface record for every reply that names a surface
    // (cad::surfaceRecord): the same keys and values the INFO of the new surface
    // gives, visible= among them, and a style that names a style - the drawing's
    // default once Cut Fill is gone, not a word that names none.
    addParallelPlanes();
    //
    // SURFACE INFO is a geoprocessing verb of katana_app (it lives with LIST and
    // REMOVE beside the GDAL ones), so this layer cannot run it: it answers with
    // cad::surfaceRecord of the stored surface, and so is that applied to the
    // surface the verb stored.
    const auto infoOf = [&](const std::string& name) {
        const katana::terrain::NamedSurface* stored = document.surfaces().find(name);
        EXPECT_NE(stored, nullptr) << name;
        return stored == nullptr
                   ? std::string()
                   : katana::cad::surfaceRecord(*stored, document.surfaceStyles());
    };
    const auto sameAsInfo = [&](const std::string& reply, const std::string& name) {
        const auto made = record(reply, "surface");
        const auto info = record(infoOf(name), "surface");
        ASSERT_TRUE(made.has_value()) << reply;
        ASSERT_TRUE(info.has_value());
        for (const char* key : {"name", "triangles", "points", "bounds", "zmin", "zmax", "source",
                                "style", "visible"}) {
            EXPECT_EQ(value(made, key), value(info, key)) << key << " of " << name;
        }
        EXPECT_EQ(value(made, "visible"), "yes");
    };
    sameAsInfo(ran("SURFACE ENVELOPE ground design MAX NAME upper"), "upper");
    sameAsInfo(ran("SURFACE DIFFERENCE ground design NAME withCutFill"), "withCutFill");

    // A style a surface is drawn in cannot be deleted: the map goes first.
    ran("UNDO");
    ASSERT_EQ(document.surfaces().find("withCutFill"), nullptr);
    ran("SURFACESTYLE DELETE \"Cut Fill\"");
    const std::string preview = ran("SURFACE DIFFERENCE ground design NAME without PREVIEW");
    EXPECT_EQ(value(record(preview, "surface"), "preview"), "yes");
    EXPECT_EQ(value(record(preview, "surface"), "style"), document.surfaceStyles().defaultStyle);
    EXPECT_EQ(value(record(preview, "surface"), "visible"), "yes");
    sameAsInfo(ran("SURFACE DIFFERENCE ground design NAME without"), "without");
    EXPECT_NE(value(record(infoOf("without"), "surface"), "style"), "default");
}

TEST_F(VolumeVerbs, HelpVolumeGivesEveryForm)
{
    const std::string help = ran("HELP VOLUME");
    for (const char* form : {"VOLUME SURFACES", "VOLUME DATUM", "VOLUME STOCKPILE",
                             "VOLUME SECTIONS", "VOLUME GRID", "SURFACE DIFFERENCE",
                             "SURFACE ENVELOPE"}) {
        EXPECT_NE(help.find(form), std::string::npos) << form;
    }
    EXPECT_NE(CommandInterpreter::helpText().find("HELP VOLUME"), std::string::npos);
}
