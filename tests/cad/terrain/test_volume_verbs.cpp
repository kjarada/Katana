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
#include "katana/cad/terrain/volume_verbs.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
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
    EXPECT_EQ(value(record(reply, "scope"), "matched"), "3");
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
    EXPECT_EQ(header, "record,polygon,entity,polygon_area,area,cut,fill,net\r");
    std::string row;
    std::getline(in, row);
    EXPECT_EQ(row, "total,,,,10000.000,0.000,10000.000,10000.000\r");
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
