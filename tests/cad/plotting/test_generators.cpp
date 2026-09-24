// The smart sheet generators (include/katana/cad/plotting/generators.hpp).
//
// Paper sizes, scales and world positions are worked by hand in the comments
// from the A3 frame's tiling area - 24..409 x 36..286 mm, 385 x 250 mm - and
// the sheet scale ladder (1:250, 500, 750, 1000, 1250, 2000, 5000 ...). None
// is read off the code's output.

#include <gtest/gtest.h>

#include <array>
#include <cmath>
#include <numbers>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/terrain/tin_surface.hpp"

using katana::cad::PaperSize;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;
using katana::geometry::AlignmentPI;
using katana::geometry::HorizontalAlignment;
using katana::geometry::SolvedAlignment;

namespace {

void expectBox(const Box2& box, double x0, double y0, double x1, double y1)
{
    EXPECT_NEAR(box.min.x, x0, 1e-9);
    EXPECT_NEAR(box.min.y, y0, 1e-9);
    EXPECT_NEAR(box.max.x, x1, 1e-9);
    EXPECT_NEAR(box.max.y, y1, 1e-9);
}

void expectPoint(const Point2& point, double x, double y, double tolerance = 1e-9)
{
    EXPECT_NEAR(point.x, x, tolerance);
    EXPECT_NEAR(point.y, y, tolerance);
}

katana::core::Result<SolvedAlignment> alignmentThrough(std::vector<AlignmentPI> pis,
                                                       double startStation = 0.0)
{
    HorizontalAlignment definition;
    definition.pis = std::move(pis);
    definition.startStation = startStation;
    return katana::geometry::solveAlignment(definition);
}

// Every viewport id of `sheets`, which must all differ.
std::size_t distinctViewportIds(const std::vector<Sheet>& sheets)
{
    std::set<std::string> ids;
    std::size_t count = 0;
    for (const Sheet& sheet : sheets) {
        for (const Viewport& viewport : sheet.viewports) {
            ids.insert(viewport.id);
            ++count;
        }
    }
    EXPECT_EQ(ids.size(), count) << "two viewports share an id";
    return ids.size();
}

SheetSet setOf(std::vector<Sheet> sheets)
{
    SheetSet set;
    set.sheets = std::move(sheets);
    return set;
}

} // namespace

// ---- fitToSheet --------------------------------------------------------------------

TEST(SheetGenerators, AnExtentIsFittedAtTheLargestStandardScaleThatHoldsIt)
{
    // 300 x 100 m in 385 x 250 mm needs 1 : max(300000 / 385 = 779.2,
    // 100000 / 250 = 400) - the next ladder step up is 1 : 1000.
    const auto sheets = fitToSheet(Box2(Point2(0.0, 0.0), Point2(300.0, 100.0)));
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 1u);
    const Sheet& sheet = sheets->front();
    EXPECT_EQ(sheet.paper, PaperSize::A3);
    EXPECT_TRUE(sheet.landscape);
    EXPECT_EQ(sheet.frame, kBuiltInFrameId);
    EXPECT_EQ(sheet.id, "s1");
    ASSERT_EQ(sheet.viewports.size(), 1u);
    const Viewport& plan = sheet.viewports.front();
    EXPECT_EQ(plan.id, "vp1");
    EXPECT_EQ(plan.kind, ViewportKind::Plan);
    EXPECT_EQ(plan.scale, 1000.0);
    expectPoint(plan.centre, 150.0, 50.0);
    expectBox(plan.rect, 24.0, 36.0, 409.0, 286.0);
    EXPECT_TRUE(plan.northArrow);
    EXPECT_TRUE(plan.scaleBar);

    // 190 x 120 m: max(493.5, 480) - 1 : 500.
    EXPECT_EQ(fitToSheet(Box2(Point2(0.0, 0.0), Point2(190.0, 120.0)))->front().viewports[0].scale,
              500.0);
    // Exactly 385 x 250 m needs exactly 1 : 1000, which fits.
    EXPECT_EQ(fitToSheet(Box2(Point2(0.0, 0.0), Point2(385.0, 250.0)))->front().viewports[0].scale,
              1000.0);
}

TEST(SheetGenerators, AFittedLineAlongAnAxisHasAScaleAndAPointIsRefused)
{
    // A line 100 m long and no height: 100000 / 385 = 259.7 - 1 : 500.
    const auto line = fitToSheet(Box2(Point2(0.0, 5.0), Point2(100.0, 5.0)));
    ASSERT_TRUE(line.ok()) << line.error().describe();
    EXPECT_EQ(line->front().viewports[0].scale, 500.0);
    const auto point = fitToSheet(Box2(Point2(3.0, 4.0), Point2(3.0, 4.0)));
    ASSERT_FALSE(point.ok());
    EXPECT_EQ(point.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(fitToSheet(Box2{}).error().code, ErrorCode::InvalidArgument);
}

TEST(SheetGenerators, AFitOnA1UsesTheDoubledFrame)
{
    // A1's drawing area is 46..820 x 70..574 (twice A3's), tiled 772 x 502
    // inside the 1 mm inset: 300 x 100 m needs max(388.6, 199.2) - 1 : 500.
    const auto sheets =
        fitToSheet(Box2(Point2(0.0, 0.0), Point2(300.0, 100.0)), {PaperSize::A1, true});
    ASSERT_TRUE(sheets.ok());
    EXPECT_EQ(sheets->front().paper, PaperSize::A1);
    EXPECT_EQ(sheets->front().viewports[0].scale, 500.0);
    expectBox(sheets->front().viewports[0].rect, 47.0, 71.0, 819.0, 573.0);
}

// ---- gridSheets ----------------------------------------------------------------------

TEST(SheetGenerators, GridTilesAreNumberedInReadingOrderAfterTheKeyPlan)
{
    // 400 x 200 m at 1 : 500 with 10 m of overlap. A tile is 385 x 0.5 =
    // 192.5 by 250 x 0.5 = 125 m, stepping 182.5 across and 115 down:
    // ceil((400 - 10) / 182.5) = 3 columns, ceil((200 - 10) / 115) = 2 rows.
    // They cover 3 x 182.5 + 10 = 557.5 by 2 x 115 + 10 = 240 m, centred on
    // (200, 100): from x -78.75 and down from y 220.
    GridRequest request;
    request.area = Box2(Point2(0.0, 0.0), Point2(400.0, 200.0));
    request.scale = 500.0;
    request.overlapM = 10.0;
    const auto sheets = gridSheets(request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 7u); // the key plan and six tiles
    EXPECT_EQ(distinctViewportIds(*sheets), 7u);
    for (std::size_t i = 0; i < sheets->size(); ++i) {
        EXPECT_EQ((*sheets)[i].id, "s" + std::to_string(i + 1));
    }

    // Top-left tile, sheet 2: x -78.75..113.75, y 95..220, centre (17.5, 157.5).
    const Viewport& first = (*sheets)[1].viewports.front();
    EXPECT_EQ((*sheets)[1].name, "PLAN TILE 1");
    EXPECT_EQ(first.scale, 500.0);
    expectPoint(first.centre, 17.5, 157.5);
    // Top-right, sheet 4: from x -78.75 + 2 x 182.5 = 286.25, centre 382.5.
    expectPoint((*sheets)[3].viewports.front().centre, 382.5, 157.5);
    // Bottom-left, sheet 5: down from y 220 - 115 = 105, centre 42.5.
    expectPoint((*sheets)[4].viewports.front().centre, 17.5, 42.5);

    // The top-left tile has two neighbours: a match line down the middle of
    // the strip it shares with sheet 3 (x 113.75 - 5 = 108.75) and across the
    // middle of the strip it shares with sheet 5 (y 95 + 5 = 100).
    ASSERT_EQ(first.marks.size(), 2u);
    const SheetSet set = setOf(*sheets);
    const WorldMark& right = first.marks[0];
    EXPECT_EQ(right.kind, WorldMark::Kind::MatchLine);
    ASSERT_EQ(right.points.size(), 2u);
    EXPECT_NEAR(right.points[0].x, 108.75, 1e-9);
    EXPECT_NEAR(right.points[1].x, 108.75, 1e-9);
    EXPECT_EQ(markLabel(set, right), "MATCH LINE - SEE SHEET 3");
    const WorldMark& below = first.marks[1];
    EXPECT_NEAR(below.points[0].y, 100.0, 1e-9);
    EXPECT_EQ(markLabel(set, below), "MATCH LINE - SEE SHEET 5");
    // The middle tile of the bottom row (sheet 6) has three: left, right, up.
    EXPECT_EQ((*sheets)[5].viewports.front().marks.size(), 3u);
}

TEST(SheetGenerators, TheGridsKeyPlanOutlinesEveryTileAtAScaleThatHoldsThemAll)
{
    // The tiles span 557.5 x 240 m around (200, 100): 1 : max(557500 / 385 =
    // 1448, 240000 / 250 = 960) - 1 : 2000.
    GridRequest request;
    request.area = Box2(Point2(0.0, 0.0), Point2(400.0, 200.0));
    request.scale = 500.0;
    request.overlapM = 10.0;
    const auto sheets = gridSheets(request);
    ASSERT_TRUE(sheets.ok());
    const Sheet& key = sheets->front();
    EXPECT_EQ(key.name, "KEY PLAN");
    EXPECT_EQ(sheetScaleText(key), "1:2000");
    ASSERT_EQ(key.viewports.size(), 1u);
    const Viewport& view = key.viewports.front();
    EXPECT_EQ(view.kind, ViewportKind::KeyPlan);
    EXPECT_EQ(view.scale, 2000.0);
    expectPoint(view.centre, 200.0, 100.0);
    ASSERT_EQ(view.marks.size(), 6u);
    const SheetSet set = setOf(*sheets);
    for (std::size_t i = 0; i < 6; ++i) {
        EXPECT_EQ(view.marks[i].kind, WorldMark::Kind::SheetOutline);
        EXPECT_EQ(view.marks[i].points.size(), 4u);
        EXPECT_EQ(markLabel(set, view.marks[i]), std::to_string(i + 2));
    }
    // Without a key plan the tiles are sheets 1 to 6, and a match line
    // still names its neighbour correctly.
    request.keyPlan = false;
    const auto bare = gridSheets(request);
    ASSERT_TRUE(bare.ok());
    ASSERT_EQ(bare->size(), 6u);
    EXPECT_EQ(markLabel(setOf(*bare), bare->front().viewports.front().marks[0]),
              "MATCH LINE - SEE SHEET 2");
}

TEST(SheetGenerators, AGridRefusesAnOverlapAsLargeAsATileOrNoScale)
{
    GridRequest request;
    request.area = Box2(Point2(0.0, 0.0), Point2(400.0, 200.0));
    request.scale = 500.0;
    request.overlapM = 125.0; // a whole tile's height
    EXPECT_EQ(gridSheets(request).error().code, ErrorCode::InvalidArgument);
    request.overlapM = 0.0;
    request.scale = 0.0;
    EXPECT_EQ(gridSheets(request).error().code, ErrorCode::InvalidArgument);
    request.scale = 500.0;
    request.area = Box2{};
    EXPECT_EQ(gridSheets(request).error().code, ErrorCode::InvalidArgument);
}

// ---- stripSheets ---------------------------------------------------------------------

TEST(SheetGenerators, StripsFollowAStraightAlignmentRotatedToItsBearing)
{
    // (0, 0) to (600, 800): 1000 m on a bearing whose direction is
    // atan2(800, 600). A strip at 1 : 500 is 192.5 x 125 m; with 20 m of
    // overlap each covers 172.5 m of chainage: 0, 172.5, 345, 517.5, 690,
    // 862.5 and the last to 1000 - six sheets.
    const auto road = alignmentThrough({{Point2(0.0, 0.0)}, {Point2(600.0, 800.0)}});
    ASSERT_TRUE(road.ok()) << road.error().describe();
    StripRequest request;
    request.scale = 500.0;
    request.overlapM = 20.0;
    const auto sheets = stripSheets(*road, "ROAD", request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 6u);
    EXPECT_EQ(sheets->front().name, "ROAD CH 0.000 TO 172.500");
    EXPECT_EQ(sheets->back().name, "ROAD CH 862.500 TO 1000.000");
    const double bearing = std::atan2(800.0, 600.0);
    for (const Sheet& sheet : *sheets) {
        ASSERT_EQ(sheet.viewports.size(), 1u);
        const Viewport& plan = sheet.viewports.front();
        EXPECT_NEAR(plan.rotation, bearing, 1e-12);
        EXPECT_EQ(plan.scale, 500.0);
        EXPECT_EQ(plan.source.alignment, "ROAD");
    }
    // The first strip is centred on chainage 86.25: (0.6, 0.8) x 86.25.
    const Viewport& first = sheets->front().viewports.front();
    expectPoint(first.centre, 51.75, 69.0);
    EXPECT_EQ(first.source.chainageFrom, 0.0);
    EXPECT_EQ(first.source.chainageTo, 172.5);
    // The last on (862.5 + 1000) / 2 = 931.25: (558.75, 745).
    expectPoint(sheets->back().viewports.front().centre, 558.75, 745.0);

    // One match line on the first sheet, square across the alignment at CH
    // 172.5 (the point (103.5, 138)), 62.5 m either side along the left
    // normal (-0.8, 0.6): from (153.5, 100.5) on the right to (53.5, 175.5).
    ASSERT_EQ(first.marks.size(), 1u);
    const WorldMark& match = first.marks.front();
    expectPoint(match.points[0], 153.5, 100.5);
    expectPoint(match.points[1], 53.5, 175.5);
    EXPECT_EQ(markLabel(setOf(*sheets), match), "MATCH LINE CH 172.500 - SEE SHEET 2");
    // A middle sheet has one at each end.
    EXPECT_EQ((*sheets)[2].viewports.front().marks.size(), 2u);
}

TEST(SheetGenerators, AStripNeverLetsACurvedAlignmentLeaveItsViewport)
{
    // Two 200 m tangents and a 100 m radius quarter circle between them: the
    // curve bends out of a 192.5 x 125 m strip unless the strip is
    // shortened. Checked as a property, at every metre of every strip, in
    // the strip's own frame: along within +-96.25 m, across within +-62.5 m.
    const auto road = alignmentThrough(
        {{Point2(0.0, 0.0)}, {Point2(300.0, 0.0), 100.0}, {Point2(300.0, 300.0)}});
    ASSERT_TRUE(road.ok()) << road.error().describe();
    StripRequest request;
    request.scale = 500.0;
    const auto sheets = stripSheets(*road, "BEND", request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    // 200 + 50 pi + 200 = 557.1 m at no more than 192.5 m a strip: at least
    // three, and more where the curve shortened one.
    EXPECT_GE(sheets->size(), 3u);
    double covered = road->startStation();
    for (const Sheet& sheet : *sheets) {
        const Viewport& plan = sheet.viewports.front();
        // Contiguous: each strip starts where the last ended.
        EXPECT_NEAR(plan.source.chainageFrom, covered, 1e-9);
        covered = plan.source.chainageTo;
        const double c = std::cos(plan.rotation);
        const double s = std::sin(plan.rotation);
        for (double station = plan.source.chainageFrom; station <= plan.source.chainageTo;
             station += 1.0) {
            const Point2 at = *road->pointAtStation(station);
            const double dx = at.x - plan.centre.x;
            const double dy = at.y - plan.centre.y;
            EXPECT_LE(std::abs(dx * c + dy * s), 96.25 + 1e-9) << "chainage " << station;
            EXPECT_LE(std::abs(-dx * s + dy * c), 62.5 + 1e-9) << "chainage " << station;
        }
    }
    EXPECT_NEAR(covered, road->endStation(), 1e-9);
}

TEST(SheetGenerators, StripsCanStartWithAKeyPlanAndRefuseAnEmptyRange)
{
    const auto road = alignmentThrough({{Point2(0.0, 0.0)}, {Point2(600.0, 800.0)}});
    ASSERT_TRUE(road.ok());
    StripRequest request;
    request.scale = 500.0;
    request.overlapM = 20.0;
    request.keyPlan = true;
    const auto sheets = stripSheets(*road, "ROAD", request);
    ASSERT_TRUE(sheets.ok());
    ASSERT_EQ(sheets->size(), 7u);
    EXPECT_EQ(sheets->front().viewports.front().kind, ViewportKind::KeyPlan);
    EXPECT_EQ(sheets->front().viewports.front().marks.size(), 6u);
    // The first strip's match line now leads to sheet 3.
    EXPECT_EQ(markLabel(setOf(*sheets), (*sheets)[1].viewports.front().marks.front()),
              "MATCH LINE CH 172.500 - SEE SHEET 3");
    request.fromChainage = 500.0;
    request.toChainage = 500.0;
    EXPECT_EQ(stripSheets(*road, "ROAD", request).error().code, ErrorCode::InvalidArgument);
}

// ---- crossSectionSheets ----------------------------------------------------------------

TEST(SheetGenerators, CrossSectionsArePackedDownEachColumnInChainageOrder)
{
    // A 100 m alignment whose chainage starts at 1000, cut every 20 m: CH
    // 1000, 1020 ... 1100, six sections. Two rows by two columns: four on
    // the first sheet, two on the second. A cell is (385 - 3) / 2 = 191 by
    // (250 - 3) / 2 = 123.5 mm. A 40 m wide section in 80% of 191 mm needs
    // 1 : 40000 / 152.8 = 261.8 - 1 : 500. With nothing sampled the
    // exaggeration is 1 and each section is centred when it is drawn.
    const auto road = alignmentThrough({{Point2(0.0, 0.0)}, {Point2(100.0, 0.0)}}, 1000.0);
    ASSERT_TRUE(road.ok());
    CrossSectionRequest request;
    request.interval = 20.0;
    request.halfWidth = 20.0;
    request.rows = 2;
    request.columns = 2;
    const auto sheets = crossSectionSheets(*road, "ROAD", request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 2u);
    ASSERT_EQ((*sheets)[0].viewports.size(), 4u);
    ASSERT_EQ((*sheets)[1].viewports.size(), 2u);
    EXPECT_EQ((*sheets)[0].name, "ROAD CROSS SECTIONS CH 1000.000 TO 1060.000");
    EXPECT_EQ((*sheets)[1].name, "ROAD CROSS SECTIONS CH 1080.000 TO 1100.000");
    const std::vector<double> chainages{1000.0, 1020.0, 1040.0, 1060.0};
    for (std::size_t i = 0; i < 4; ++i) {
        const Viewport& section = (*sheets)[0].viewports[i];
        EXPECT_EQ(section.kind, ViewportKind::CrossSections);
        ASSERT_EQ(section.source.stations.size(), 1u);
        EXPECT_NEAR(section.source.stations[0], chainages[i], 1e-9);
        EXPECT_EQ(section.scale, 500.0);
        EXPECT_EQ(section.verticalExaggeration, 1.0);
        EXPECT_TRUE(section.autoCentre);
        EXPECT_EQ(section.source.sectionHalfWidth, 20.0);
    }
    // Down the first column, then the second: top-left, bottom-left,
    // top-right. The bottom row starts 123.5 + 3 below the top (286 - 126.5
    // = 159.5); the second column 191 + 3 right of the first (218).
    expectBox((*sheets)[0].viewports[0].rect, 24.0, 162.5, 215.0, 286.0);
    expectBox((*sheets)[0].viewports[1].rect, 24.0, 36.0, 215.0, 159.5);
    expectBox((*sheets)[0].viewports[2].rect, 218.0, 162.5, 409.0, 286.0);
    expectBox((*sheets)[1].viewports[0].rect, 24.0, 162.5, 215.0, 286.0);
}

TEST(SheetGenerators, GivenStationsAreSortedAndTheirDuplicatesDropped)
{
    const auto road = alignmentThrough({{Point2(0.0, 0.0)}, {Point2(100.0, 0.0)}});
    ASSERT_TRUE(road.ok());
    CrossSectionRequest request;
    request.stations = {60.0, 20.0, 40.0, 20.0};
    const auto sheets = crossSectionSheets(*road, "ROAD", request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 1u);
    ASSERT_EQ(sheets->front().viewports.size(), 3u);
    EXPECT_EQ(sheets->front().viewports[0].source.stations[0], 20.0);
    EXPECT_EQ(sheets->front().viewports[1].source.stations[0], 40.0);
    EXPECT_EQ(sheets->front().viewports[2].source.stations[0], 60.0);
}

TEST(SheetGenerators, TheExaggerationIsTheLargestThatFitsTheDeepestSection)
{
    // Ground z = x / 2 (exact in binary). The alignment runs north up x = 50,
    // so every section runs east-west from x 30 to x 70: ground 15 to 35, 20
    // m deep, centred on 25. At 1 : 500 (as above) 20 m is 40 mm; 80% of a
    // 123.5 mm cell is 98.8 mm, so the exaggeration may be up to 2.47: of
    // 1, 2, 2.5, 4 ... the largest that fits is 2.
    std::vector<katana::geometry::Point3> vertices{
        {0.0, 0.0, 0.0}, {100.0, 0.0, 50.0}, {100.0, 100.0, 50.0}, {0.0, 100.0, 0.0}};
    std::vector<katana::terrain::TinTriangle> triangles{{0, 1, 2}, {0, 2, 3}};
    auto ground = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
    ASSERT_TRUE(ground.ok()) << ground.error().describe();
    const auto road = alignmentThrough({{Point2(50.0, 10.0)}, {Point2(50.0, 90.0)}});
    ASSERT_TRUE(road.ok());
    CrossSectionRequest request;
    request.interval = 40.0;
    request.halfWidth = 20.0;
    request.rows = 2;
    request.columns = 2;
    request.surfaces = {{"GROUND", &*ground}};
    const auto sheets = crossSectionSheets(*road, "ROAD", request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->front().viewports.size(), 3u); // CH 0, 40, 80
    for (const Viewport& section : sheets->front().viewports) {
        EXPECT_EQ(section.scale, 500.0);
        EXPECT_EQ(section.verticalExaggeration, 2.0);
        EXPECT_FALSE(section.autoCentre);
        expectPoint(section.centre, 0.0, 25.0);
    }
}

TEST(SheetGenerators, CrossSectionsRefuseAnEmptyGridOrNoWidth)
{
    const auto road = alignmentThrough({{Point2(0.0, 0.0)}, {Point2(100.0, 0.0)}});
    ASSERT_TRUE(road.ok());
    CrossSectionRequest request;
    request.rows = 0;
    EXPECT_EQ(crossSectionSheets(*road, "ROAD", request).error().code,
              ErrorCode::InvalidArgument);
    request.rows = 4;
    request.halfWidth = 0.0;
    EXPECT_EQ(crossSectionSheets(*road, "ROAD", request).error().code,
              ErrorCode::InvalidArgument);
}

// ---- sheetsFromPlotFrames --------------------------------------------------------------

namespace {

katana::entity::Entity frameEntity(double width, double height, double scale, double rotation,
                                   Point2 origin, std::array<double, 4> margins,
                                   std::string layer, std::string name)
{
    katana::entity::Entity entity;
    katana::geometry::Polyline2 outline;
    outline.closed = true;
    outline.vertices = {origin, origin + katana::geometry::Vec2(1.0, 0.0),
                        origin + katana::geometry::Vec2(1.0, 1.0)};
    entity.geometry = outline;
    entity.layer = std::move(layer);
    auto& p = entity.properties;
    p["plot_frame.width"] = width;
    p["plot_frame.height"] = height;
    p["plot_frame.scale"] = scale;
    p["plot_frame.rotation"] = rotation;
    p["plot_frame.xorigin"] = origin.x;
    p["plot_frame.yorigin"] = origin.y;
    p["plot_frame.left_margin"] = margins[0];
    p["plot_frame.right_margin"] = margins[1];
    p["plot_frame.top_margin"] = margins[2];
    p["plot_frame.bottom_margin"] = margins[3];
    if (!name.empty()) {
        entity.metadata["12d.name"] = std::move(name);
    }
    return entity;
}

} // namespace

TEST(SheetGenerators, AnImportedPlotFrameBecomesASheetShowingTheGroundItCovered)
{
    katana::entity::Model model;
    // An A3 frame at 1 : 1000, rotated 30 degrees, its paper's corner at
    // (1000, 2000), with the built-in frame's own margins (L23 R10 T10 B35).
    ASSERT_TRUE(model.entities
                    .add(frameEntity(420.0, 297.0, 1000.0, 30.0, Point2(1000.0, 2000.0),
                                     {23.0, 10.0, 10.0, 35.0}, "FRAMES", "NORTH"))
                    .ok());
    // An A1 frame at 1 : 500, not rotated, margins 22 all round but 82 at
    // the bottom.
    ASSERT_TRUE(model.entities
                    .add(frameEntity(841.0, 594.0, 500.0, 0.0, Point2(0.0, 0.0),
                                     {22.0, 22.0, 22.0, 82.0}, "FRAMES", ""))
                    .ok());
    // 500 x 300 mm is no ISO A size.
    ASSERT_TRUE(model.entities
                    .add(frameEntity(500.0, 300.0, 1000.0, 0.0, Point2(0.0, 0.0),
                                     {10.0, 10.0, 10.0, 10.0}, "FRAMES", "ODD"))
                    .ok());
    std::vector<std::string> skipped;
    const auto sheets = sheetsFromPlotFrames(model, {}, &skipped);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 2u);
    ASSERT_EQ(skipped.size(), 1u);
    EXPECT_NE(skipped.front().find("not an ISO A size"), std::string::npos) << skipped.front();

    const Sheet& north = (*sheets)[0];
    EXPECT_EQ(north.name, "NORTH");
    EXPECT_EQ(north.paper, PaperSize::A3);
    EXPECT_TRUE(north.landscape);
    const Viewport& plan = north.viewports.front();
    // The frame's window on the paper, 23..410 x 35..287, is exactly the
    // built-in frame's drawing area; its centre (216.5, 161) mm is 216.5 m
    // along and 161 m up from the corner at 1 : 1000. Along (cos 30, sin 30)
    // = (0.8660254037844386, 0.5), up (-0.5, 0.8660254037844386):
    // x = 1000 + 187.4944999193310 - 80.5 = 1106.994499919331,
    // y = 2000 + 108.25 + 139.4300900092946 = 2247.680090009295.
    expectBox(plan.rect, 23.0, 35.0, 410.0, 287.0);
    EXPECT_EQ(plan.scale, 1000.0);
    EXPECT_NEAR(plan.rotation, std::numbers::pi / 6.0, 1e-15);
    expectPoint(plan.centre, 1106.994499919331, 2247.680090009295, 1e-9);
    EXPECT_TRUE(plan.hiddenLayers.hides("FRAMES"));

    const Sheet& a1 = (*sheets)[1];
    EXPECT_EQ(a1.paper, PaperSize::A1);
    EXPECT_EQ(a1.name, "plot frame 2"); // no name of its own
    // Its window 22..819 x 82..572 clipped to A1's drawing area 46..820 x
    // 70..574: 46..819 x 82..572, centre (432.5, 327) mm - at 1 : 500 the
    // point (216.25, 163.5) m from the corner.
    expectBox(a1.viewports.front().rect, 46.0, 82.0, 819.0, 572.0);
    expectPoint(a1.viewports.front().centre, 216.25, 163.5);
}

// ---- smartLayout -----------------------------------------------------------------------

namespace {

katana::entity::Model modelWithRoad()
{
    katana::entity::Model model;
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{Point2(0.0, 0.0)}, {Point2(1000.0, 0.0)}};
    EXPECT_TRUE(model.alignments.add(road).ok());
    return model;
}

} // namespace

TEST(SheetGenerators, ASmartPlanWithA3DViewAndLegendSharesOneSheet)
{
    // "Main and two panels right": the plan in the 243.4 x 247 mm main cell,
    // which a 300 x 200 m area needs 1 : max(300000 / 243.4 = 1232.5,
    // 200000 / 247 = 809.7) for - 1 : 1250.
    LayoutRequest request;
    request.planArea = Box2(Point2(0.0, 0.0), Point2(300.0, 200.0));
    request.model3d = true;
    request.legend = true;
    const auto sheets = smartLayout(katana::entity::Model{}, request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 1u);
    const Sheet& sheet = sheets->front();
    ASSERT_EQ(sheet.viewports.size(), 3u);
    EXPECT_EQ(sheet.viewports[0].kind, ViewportKind::Plan);
    EXPECT_EQ(sheet.viewports[0].scale, 1250.0);
    expectBox(sheet.viewports[0].rect, 25.5, 37.5, 268.9, 284.5);
    EXPECT_EQ(sheet.viewports[1].kind, ViewportKind::Model3D);
    expectBox(sheet.viewports[1].rect, 271.9, 162.5, 407.5, 284.5);
    EXPECT_EQ(sheet.viewports[2].kind, ViewportKind::Legend);
    expectBox(sheet.viewports[2].rect, 271.9, 37.5, 407.5, 159.5);
    EXPECT_EQ(sheetScaleText(sheet), "1:1250");
}

TEST(SheetGenerators, ASmartPlanAtAFixedScaleTooLargeForOneSheetIsTiled)
{
    // 400 x 200 m at 1 : 500 needs 1 : 1039 on one sheet: tiles, 3 x 2
    // (as the grid test above, without overlap: ceil(400 / 192.5) = 3,
    // ceil(200 / 125) = 2) after a key plan.
    LayoutRequest request;
    request.planArea = Box2(Point2(0.0, 0.0), Point2(400.0, 200.0));
    request.scale = 500.0;
    const auto sheets = smartLayout(katana::entity::Model{}, request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 7u);
    EXPECT_EQ(sheets->front().viewports.front().kind, ViewportKind::KeyPlan);
    // At 1 : 500 an area that fits (190 x 120 m needs 1 : 493.5) is one sheet.
    request.planArea = Box2(Point2(0.0, 0.0), Point2(190.0, 120.0));
    const auto one = smartLayout(katana::entity::Model{}, request);
    ASSERT_TRUE(one.ok());
    ASSERT_EQ(one->size(), 1u);
    EXPECT_EQ(one->front().viewports.front().scale, 500.0);
}

TEST(SheetGenerators, ASmartPlanAndProfileThenCrossSectionsSpillsOntoMoreSheets)
{
    // A 1000 m straight road at 1 : 500. The plan takes the top cell of "Main
    // and panel below" (382 mm wide, 191 m at 1 : 500): ceil(1000 / 191) = 6
    // plan-and-profile sheets. Cross sections every 100 m are 11 (CH 0 to
    // 1000), 4 rows by 2 columns a sheet: 2 more sheets. 8 in all.
    const katana::entity::Model model = modelWithRoad();
    LayoutRequest request;
    request.alignment = "ROAD";
    request.planAlongAlignment = true;
    request.longSection = true;
    request.crossSectionInterval = 100.0;
    request.scale = 500.0;
    const auto sheets = smartLayout(model, request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 8u);
    for (std::size_t i = 0; i < sheets->size(); ++i) {
        EXPECT_EQ((*sheets)[i].id, "s" + std::to_string(i + 1));
    }
    distinctViewportIds(*sheets);
    for (std::size_t i = 0; i < 6; ++i) {
        const Sheet& sheet = (*sheets)[i];
        ASSERT_EQ(sheet.viewports.size(), 2u);
        const Viewport& plan = sheet.viewports[0];
        const Viewport& profile = sheet.viewports[1];
        EXPECT_EQ(plan.kind, ViewportKind::Plan);
        EXPECT_EQ(profile.kind, ViewportKind::LongSection);
        expectBox(plan.rect, 25.5, 127.5, 407.5, 284.5);
        expectBox(profile.rect, 25.5, 37.5, 407.5, 124.5);
        // The profile shows the plan's chainages at the plan's horizontal
        // scale, exaggerated 10 times (V 1 : 50).
        EXPECT_EQ(profile.source.chainageFrom, plan.source.chainageFrom);
        EXPECT_EQ(profile.source.chainageTo, plan.source.chainageTo);
        EXPECT_EQ(profile.scale, 500.0);
        EXPECT_EQ(sheetScaleText(sheet), "AS SHOWN");
        EXPECT_EQ(scaleText(profile), "H 1:500 V 1:50");
    }
    EXPECT_NEAR((*sheets)[0].viewports[0].source.chainageTo, 191.0, 1e-9);
    EXPECT_NEAR((*sheets)[5].viewports[0].source.chainageFrom, 955.0, 1e-9);
    EXPECT_EQ((*sheets)[6].viewports.size(), 8u);
    EXPECT_EQ((*sheets)[7].viewports.size(), 3u);
    EXPECT_EQ((*sheets)[7].viewports.back().source.stations.front(), 1000.0);
}

TEST(SheetGenerators, AnAutomaticPlanAndProfileFitsTheWholeRoadOnOneSheet)
{
    // 1000 m in 382 mm needs 1 : 2617.8 - 1 : 5000, one sheet.
    const katana::entity::Model model = modelWithRoad();
    LayoutRequest request;
    request.alignment = "ROAD";
    request.planAlongAlignment = true;
    request.longSection = true;
    const auto sheets = smartLayout(model, request);
    ASSERT_TRUE(sheets.ok()) << sheets.error().describe();
    ASSERT_EQ(sheets->size(), 1u);
    EXPECT_EQ(sheets->front().viewports[0].scale, 5000.0);
    EXPECT_EQ(sheets->front().viewports[1].scale, 5000.0);
}

TEST(SheetGenerators, ASmartLayoutRefusesAnUnknownAlignmentOrARequestForNothing)
{
    LayoutRequest request;
    EXPECT_EQ(smartLayout(katana::entity::Model{}, request).error().code,
              ErrorCode::InvalidArgument);
    request.alignment = "NOWHERE";
    request.longSection = true;
    EXPECT_EQ(smartLayout(katana::entity::Model{}, request).error().code, ErrorCode::NotFound);
}

TEST(SheetGenerators, TheSameRequestGivesTheSameSheets)
{
    const katana::entity::Model model = modelWithRoad();
    LayoutRequest request;
    request.alignment = "ROAD";
    request.planAlongAlignment = true;
    request.longSection = true;
    request.crossSectionInterval = 50.0;
    request.model3d = true;
    request.legend = true;
    request.scale = 1000.0;
    const auto first = smartLayout(model, request);
    const auto second = smartLayout(model, request);
    ASSERT_TRUE(first.ok() && second.ok());
    EXPECT_TRUE(*first == *second);
}

// ---- prepareForAppend -----------------------------------------------------------------

TEST(SheetGenerators, AppendedSheetsTakeFreshIdsAndTheirMarksFollow)
{
    // A set with sheets s1 and s2 and viewports vp1..vp3; a generator's two
    // sheets (its own s1 and s2, vp1 and vp2) whose first leads to its
    // second. Appended they become s3 and s4 with vp4 and vp5, and the mark
    // leads to s4.
    SheetSet set;
    set.sheets.resize(2);
    set.sheets[0].id = "s1";
    set.sheets[1].id = "s2";
    set.sheets[0].viewports.resize(2);
    set.sheets[0].viewports[0].id = "vp1";
    set.sheets[0].viewports[1].id = "vp2";
    set.sheets[1].viewports.resize(1);
    set.sheets[1].viewports[0].id = "vp3";
    std::vector<Sheet> more(2);
    more[0].id = "s1";
    more[1].id = "s2";
    more[0].viewports.resize(1);
    more[0].viewports[0].id = "vp1";
    more[0].viewports[0].marks.push_back({WorldMark::Kind::MatchLine, {}, "", "s2"});
    more[1].viewports.resize(1);
    more[1].viewports[0].id = "vp2";
    prepareForAppend(set, more);
    EXPECT_EQ(more[0].id, "s3");
    EXPECT_EQ(more[1].id, "s4");
    EXPECT_EQ(more[0].viewports[0].id, "vp4");
    EXPECT_EQ(more[1].viewports[0].id, "vp5");
    EXPECT_EQ(more[0].viewports[0].marks[0].sheet, "s4");
}
