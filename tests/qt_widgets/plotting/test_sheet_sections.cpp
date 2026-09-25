// Section smarts in the sheet painter (src/katana_qt/sheet_painter,
// docs/plotting.md "Section smarts"): the automatic scale and exaggeration
// and the title block that reports them, cut and fill shaded and tabled, a
// chainage range shown exactly, the centreline's levels and each service's
// level and depth, and a note dropped rather than drawn over another.
// Painted into QImages at 4 px a millimetre and read back pixel by pixel;
// the numbers themselves are tested without pixels in
// tests/cad/plotting/test_section_fit.cpp and test_section_annotation.cpp.
//
// With KATANA_SHEET_PNG set to a directory, every sheet a test paints is also
// written there as a PNG, to be looked at.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <limits>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <QColor>
#include <QImage>
#include <QPainter>

#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "sheet_painter.hpp"

using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::qt::SheetPaintCache;
using katana::qt::SheetPaintOptions;
using katana::qt::SheetPaintStats;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

constexpr double kPpmm = 4.0; // ~100 dpi: every 0.25 mm line is a pixel
const QRgb kWhite = qRgb(255, 255, 255);
// The painter's shades and the design's ink.
const QColor kCut(250, 200, 200);
const QColor kFill(200, 236, 200);
const QColor kDesignRed(210, 0, 0);

Box2 box(double x0, double y0, double x1, double y1) { return Box2(Point2(x0, y0), Point2(x1, y1)); }

std::optional<katana::terrain::TinSurface> tin(std::vector<Point3> corners)
{
    std::vector<katana::terrain::TinTriangle> triangles{{0, 1, 2}, {0, 2, 3}};
    auto surface = katana::terrain::TinSurface::create(std::move(corners), std::move(triangles));
    EXPECT_TRUE(surface.ok());
    return surface.ok() ? std::optional(std::move(*surface)) : std::nullopt;
}

// A road east along y = 50 from x 10 to x 190, over ground that is the
// plane z = 18 + 0.05 x + 0.04 y. Chainage c (from `start`) is at
// x = 10 + c - start, where the ground is 20.5 + 0.05 (c - start). Its design
// profile is flat at 25, so the two cross 90 m along: fill before, cut after.
// Across the road the ground rises 0.04 a metre to the north - to the left,
// looking along it, where the offsets are negative.
struct Road {
    Model model;
    std::optional<katana::terrain::TinSurface> ground =
        tin({{0.0, 0.0, 18.0}, {200.0, 0.0, 28.0}, {200.0, 100.0, 32.0}, {0.0, 100.0, 22.0}});
    std::optional<katana::terrain::TinSurface> designPad;

    explicit Road(bool profile = true, double start = 0.0)
    {
        katana::entity::Alignment road;
        road.name = "ROAD";
        road.horizontal.pis = {{Point2(10.0, 50.0)}, {Point2(190.0, 50.0)}};
        road.horizontal.startStation = start;
        if (profile) {
            road.vertical = katana::geometry::VerticalAlignment{
                {{start, 25.0, 0.0}, {start + 180.0, 25.0, 0.0}}};
        }
        EXPECT_TRUE(model.alignments.add(road).ok());
    }

    // A service along the road at `y`, at `level` (none: drawn flat), on a
    // layer of its own.
    void service(const std::string& layer, double y, std::optional<double> level)
    {
        if (!model.layers.contains(layer)) {
            katana::entity::Layer shown;
            shown.name = layer;
            EXPECT_TRUE(model.layers.add(shown).ok());
        }
        Entity entity;
        entity.geometry = katana::geometry::Segment2{Point2(0.0, y), Point2(200.0, y)};
        entity.layer = layer;
        if (level) {
            katana::entity::setHeights(entity.properties, {level, level});
        }
        EXPECT_TRUE(model.entities.add(std::move(entity)).ok());
    }

    [[nodiscard]] SheetSource source() const
    {
        SheetSource source;
        source.plan.model = &model;
        source.surfaces.push_back({"GROUND", &*ground});
        if (designPad) {
            source.surfaces.push_back({"DESIGN PAD", &*designPad});
        }
        return source;
    }
};

plotting::Viewport longSection(Box2 rect, double scale, double exaggeration, Point2 centre)
{
    plotting::Viewport viewport;
    viewport.id = "vp1";
    viewport.kind = plotting::ViewportKind::LongSection;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.verticalExaggeration = exaggeration;
    viewport.centre = centre;
    viewport.source.alignment = "ROAD";
    return viewport;
}

plotting::Viewport crossSection(Box2 rect, double chainage, double scale, Point2 centre)
{
    plotting::Viewport viewport;
    viewport.id = "vp2";
    viewport.kind = plotting::ViewportKind::CrossSections;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.verticalExaggeration = 1.0;
    viewport.centre = centre;
    viewport.source.alignment = "ROAD";
    viewport.source.stations = {chainage};
    viewport.source.sectionHalfWidth = 20.0;
    return viewport;
}

plotting::SheetSet sheetWith(std::vector<plotting::Viewport> viewports)
{
    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = "SECTIONS";
    sheet.viewports = std::move(viewports);
    set.sheets.push_back(std::move(sheet));
    return set;
}

QImage painted(const plotting::SheetSet& set, const SheetSource& source,
               SheetPaintStats* stats = nullptr, const char* tag = "")
{
    const auto& sheet = set.sheets.front();
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    QImage image(static_cast<int>(std::lround(paper.widthMm * kPpmm)),
                 static_cast<int>(std::lround(paper.heightMm * kPpmm)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::gray);
    SheetPaintOptions options;
    options.pixelsPerMillimetre = kPpmm;
    SheetPaintCache cache;
    QPainter painter(&image);
    const SheetPaintStats result = katana::qt::paintSheet(painter, set, 0, source, options, cache);
    painter.end();
    if (stats != nullptr) {
        *stats = result;
    }
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        const auto* test = ::testing::UnitTest::GetInstance()->current_test_info();
        image.save(QString("%1/%2%3.png").arg(dir, test->name(), tag));
    }
    return image;
}

QPoint pixelOf(Point2 paper)
{
    return QPoint(static_cast<int>(std::floor(paper.x * kPpmm)),
                  static_cast<int>(std::floor((297.0 - paper.y) * kPpmm)));
}

QRect pixelsOf(Box2 paper)
{
    return QRect(pixelOf(Point2(paper.min.x, paper.max.y)), pixelOf(Point2(paper.max.x, paper.min.y)));
}

// Pixels in the paper box that pass `keep`.
template <typename Keep>
int pixelsIn(const QImage& image, Box2 paper, Keep keep)
{
    const QRect r = pixelsOf(paper).intersected(image.rect());
    int count = 0;
    for (int y = r.top(); y <= r.bottom(); ++y) {
        for (int x = r.left(); x <= r.right(); ++x) {
            count += keep(image.pixelColor(x, y)) ? 1 : 0;
        }
    }
    return count;
}

int inkIn(const QImage& image, Box2 paper)
{
    return pixelsIn(image, paper, [](const QColor& c) { return c.rgb() != kWhite; });
}

// Pixels within `tolerance` of `colour` on every channel: close enough to
// tell a shade from the grid's grey and the ground's brown.
int colourIn(const QImage& image, Box2 paper, QColor colour, int tolerance = 12)
{
    return pixelsIn(image, paper, [&](const QColor& c) {
        return std::abs(c.red() - colour.red()) <= tolerance &&
               std::abs(c.green() - colour.green()) <= tolerance &&
               std::abs(c.blue() - colour.blue()) <= tolerance;
    });
}

// Text: dark and neutral. At 4 px a millimetre a 1.4 mm letter is mostly
// its antialiased edge, so dark is anything under 150; nothing else in a
// section is both - the grid is a light grey hairline, a crossing's line a
// faint grey dash, each series in its own colour.
int textIn(const QImage& image, Box2 paper)
{
    return pixelsIn(image, paper, [](const QColor& c) {
        const int high = std::max({c.red(), c.green(), c.blue()});
        const int low = std::min({c.red(), c.green(), c.blue()});
        return high <= 150 && high - low <= 25;
    });
}

// The long section the tests below draw: 23..410 x 150..287 on A3, its
// section above the 7 mm title strip, 157..287. The band's four rows
// (design, ground, cut/fill, chainage) are 32 mm, so the plot is laid out
// 45..408 x 190..285 (the band's names 22 mm to its left), middle (226.5,
// 237.5). At 1 : 1000 and 10 times, centred on (CH 90, RL 25):
// X(c) = 136.5 + c and Y(z) = 237.5 + 10 (z - 25). The plot is then cut to
// the chainages shown, and the band and the names go with it.
const Box2 kLongRect = box(23.0, 150.0, 410.0, 287.0);
double longX(double chainage) { return 136.5 + chainage; }
double longY(double level) { return 237.5 + 10.0 * (level - 25.0); }
// The band's rows, top down: design 181..189, ground 173..181, cut/fill
// 165..173, chainage 157..165.
constexpr double kCutFillRow = 165.0;
constexpr double kChainageRow = 157.0;

// The cross section the tests below draw: 23..210 x 35..145, its section
// 42..145. Under its plot 4 mm of offsets, a 4.5 mm caption and 1 mm; to its
// left the 6 mm the short levels need; so the plot is 29..208 x 51.5..143,
// middle (118.5, 97.25). At 1 : 200 true scale centred on (0, `level`):
// X(o) = 118.5 + 5 o and Y(z) = 97.25 + 5 (z - level).
const Box2 kCrossRect = box(23.0, 35.0, 210.0, 145.0);
double crossX(double offset) { return 118.5 + 5.0 * offset; }
double crossY(double level, double centre) { return 97.25 + 5.0 * (level - centre); }

} // namespace

// ---- cut and fill ---------------------------------------------------------------------

TEST(SheetSections, CutAndFillAreShadedBetweenTheDesignAndTheGround)
{
    const Road road;
    const auto viewport = longSection(kLongRect, 1000.0, 10.0, Point2(90.0, 25.0));
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();

    // At CH 40 the ground is 22.5, under the design's 25: fill, green.
    const Box2 fill = box(longX(35.0), longY(22.8), longX(45.0), longY(24.7));
    EXPECT_GT(colourIn(paper, fill, kFill), 40 * 76 * 8 / 10);
    EXPECT_EQ(colourIn(paper, fill, kCut), 0);
    // At CH 140 the ground is 27.5, over the design: cut, red.
    const Box2 cut = box(longX(135.0), longY(25.3), longX(145.0), longY(27.2));
    EXPECT_GT(colourIn(paper, cut, kCut), 40 * 76 * 8 / 10);
    EXPECT_EQ(colourIn(paper, cut, kFill), 0);
    // Nothing is shaded above the higher of the two or below the lower.
    EXPECT_EQ(colourIn(paper, box(longX(35.0), longY(25.3), longX(45.0), longY(29.0)), kFill), 0);
    EXPECT_EQ(colourIn(paper, box(longX(135.0), longY(21.0), longX(145.0), longY(24.7)), kCut), 0);

    // Without a design there is nothing to shade.
    const Road ground(false);
    const QImage bare = painted(sheetWith({viewport}), ground.source(), nullptr, "_no_design");
    EXPECT_EQ(colourIn(bare, kLongRect, kFill), 0);
    EXPECT_EQ(colourIn(bare, kLongRect, kCut), 0);
}

TEST(SheetSections, TheDataBandHasACutAndFillRowWhenThereIsADesign)
{
    const Road road;
    const auto viewport = longSection(kLongRect, 1000.0, 10.0, Point2(90.0, 25.0));
    const QImage paper = painted(sheetWith({viewport}), road.source());
    // "CUT/FILL" in the names column, 21.5 mm left of the plot's CH 0...
    const double names = longX(0.0) - 21.0;
    EXPECT_GT(textIn(paper, box(names, kCutFillRow + 1.5, names + 12.0, kCutFillRow + 6.5)), 20);
    // ...and design less ground turned in each 10 m column: "+2.500" at CH
    // 40, "0.000" at CH 90, "-2.500" at CH 140.
    for (const double chainage : {40.0, 90.0, 140.0}) {
        EXPECT_GT(textIn(paper, box(longX(chainage) - 0.6, kCutFillRow + 1.0, longX(chainage) + 0.6,
                                    kCutFillRow + 7.0)),
                  8)
            << chainage;
    }

    // Without a design the band is the ground and the chainage, 157..173:
    // no row names above it.
    const Road ground(false);
    const QImage bare = painted(sheetWith({viewport}), ground.source(), nullptr, "_no_design");
    EXPECT_GT(textIn(bare, box(names, kChainageRow + 1.5, names + 12.0, kChainageRow + 6.5)), 20);
    EXPECT_EQ(textIn(bare, box(names, 174.0, names + 12.0, 189.0)), 0);
}

TEST(SheetSections, ADesignProfileLiesOverTheGroundWhateverTheChainageStartsAt)
{
    // The same road numbered from CH 1000, its profile too: the design is
    // still over the ground, filling to CH 1090 and cutting after.
    const Road road(true, 1000.0);
    const auto viewport = longSection(kLongRect, 1000.0, 10.0, Point2(1090.0, 25.0));
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_GT(colourIn(paper, box(longX(35.0), longY(22.8), longX(45.0), longY(24.7)), kFill),
              40 * 76 * 8 / 10);
    EXPECT_GT(colourIn(paper, box(longX(135.0), longY(25.3), longX(145.0), longY(27.2)), kCut),
              40 * 76 * 8 / 10);
}

// ---- a chainage range -----------------------------------------------------------------

TEST(SheetSections, ALongSectionWithARangeShowsExactlyThatRangeWithItsEndsLabelled)
{
    const Road road;
    auto viewport = longSection(kLongRect, 1000.0, 10.0, Point2(90.0, 25.0));
    viewport.source.chainageFrom = 30.0;
    viewport.source.chainageTo = 150.0;
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();

    // The plot is x 166.5..286.5, the range's ends: nothing of the section
    // - no line, no shade, no band - is drawn past either. The band's names
    // stand in the 22 mm left of it, from x 145.
    EXPECT_EQ(inkIn(paper, box(longX(152.0), 158.0, 408.0, 284.0)), 0);
    EXPECT_EQ(inkIn(paper, box(46.0, 158.0, longX(30.0) - 23.0, 284.0)), 0);
    // Right up to them it is drawn: the shades at both ends.
    EXPECT_GT(colourIn(paper, box(longX(31.0), longY(22.3), longX(35.0), longY(24.7)), kFill), 0);
    EXPECT_GT(colourIn(paper, box(longX(145.0), longY(25.3), longX(149.0), longY(27.5)), kCut), 0);
    // Each end is written in full just inside it, "30.000" and "150.000",
    // down its chainage row and with its levels above.
    for (const double row : {kChainageRow, kCutFillRow}) {
        EXPECT_GT(textIn(paper, box(longX(30.0) + 0.7, row + 1.0, longX(30.0) + 1.9, row + 7.0)), 8);
        EXPECT_GT(textIn(paper, box(longX(150.0) - 1.9, row + 1.0, longX(150.0) - 0.7, row + 7.0)), 8);
    }

    // Without a range the range is the whole road: nothing past CH 180, and
    // its last column has its levels too.
    viewport.source.chainageFrom = 0.0;
    viewport.source.chainageTo = 0.0;
    const QImage whole = painted(sheetWith({viewport}), road.source(), nullptr, "_whole");
    EXPECT_EQ(inkIn(whole, box(longX(182.0), 158.0, 408.0, 284.0)), 0);
    EXPECT_GT(colourIn(whole, box(longX(170.0), longY(25.3), longX(178.0), longY(28.0)), kCut), 0);
    EXPECT_GT(textIn(whole, box(longX(180.0) - 1.9, kCutFillRow + 1.0, longX(180.0) - 0.7,
                                kCutFillRow + 7.0)),
              8);
}

// ---- cross sections -------------------------------------------------------------------

TEST(SheetSections, ACrossSectionLabelsTheDesignAndGroundLevelsAtItsCentreline)
{
    // At CH 60 the profile's design is 25.000 and the ground 23.500: filled
    // 1.500. Three lines, right of the centreline, just above the design.
    const Road road;
    const auto viewport = crossSection(kCrossRect, 60.0, 200.0, Point2(0.0, 24.0));
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_EQ(stats.sectionNotesDropped, 0u);
    const double design = crossY(25.0, 24.0);
    EXPECT_GT(textIn(paper, box(crossX(0.0) + 1.0, design + 1.4, crossX(0.0) + 15.0, design + 7.4)), 60);
    // Nothing left of it: the note took its first place.
    EXPECT_EQ(textIn(paper, box(crossX(0.0) - 15.0, design + 1.4, crossX(0.0) - 1.0, design + 7.4)), 0);
    // A tick across the centreline at the design's level, in the design's red.
    EXPECT_GT(colourIn(paper, box(crossX(0.0) - 0.9, design - 0.3, crossX(0.0) + 0.9, design + 0.3),
                       kDesignRed, 40),
              0);

    // Without a profile, the ground alone.
    const Road ground(false);
    const QImage bare = painted(sheetWith({viewport}), ground.source(), nullptr, "_ground");
    const double groundY = crossY(23.5, 24.0);
    EXPECT_GT(textIn(bare, box(crossX(0.0) + 1.0, groundY + 1.4, crossX(0.0) + 15.0, groundY + 2.8)), 20);
    EXPECT_EQ(textIn(bare, box(crossX(0.0) + 1.0, groundY + 3.4, crossX(0.0) + 15.0, groundY + 8.0)), 0);
    EXPECT_EQ(colourIn(bare, kCrossRect, kDesignRed, 40), 0);
}

TEST(SheetSections, ACrossSectionShadesBetweenADesignSurfaceAndTheGround)
{
    // A pad at 23.5 across the whole section, level with the ground at the
    // centreline. The ground (21.5 + 0.04 y) rises to the north, left of the
    // centreline: it is cut there, and filled to the right.
    Road road;
    road.designPad =
        tin({{50.0, 30.0, 23.5}, {90.0, 30.0, 23.5}, {90.0, 70.0, 23.5}, {50.0, 70.0, 23.5}});
    const auto viewport = crossSection(kCrossRect, 60.0, 200.0, Point2(0.0, 23.5));
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();
    // 8 to 12 m left the ground is 23.82 to 23.98, over the pad's 97.25:
    // cut from y 98.85 up. 8 to 12 m right it is 23.18 to 23.02: fill down to
    // 95.65. Between the lines' pens, each is all shade.
    const Box2 cut = box(crossX(-12.0), 97.8, crossX(-8.0), 98.5);
    const Box2 fill = box(crossX(8.0), 96.0, crossX(12.0), 96.8);
    EXPECT_GT(colourIn(paper, cut, kCut), 80 * 2 * 8 / 10);
    EXPECT_GT(colourIn(paper, fill, kFill), 80 * 3 * 8 / 10);
    EXPECT_EQ(colourIn(paper, cut, kFill), 0);
    EXPECT_EQ(colourIn(paper, fill, kCut), 0);
}

TEST(SheetSections, AServiceIsLabelledWithItsLayerLevelAndDepthAndItsOffsetUnderIt)
{
    // Water 5 m left of the centreline (y 55) at 22.00, where the ground is
    // 23.70: "WATER RL 22.00 D 1.70", stood up left of its line from just
    // above its marker, and "-5.00" under the marker.
    Road road;
    road.service("WATER", 55.0, 22.0);
    const auto viewport = crossSection(kCrossRect, 60.0, 200.0, Point2(0.0, 23.0));
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_EQ(stats.sectionNotesDropped, 0u);
    const double x = crossX(-5.0);
    const double marker = crossY(22.0, 23.0);
    // The marker: a ring at the level.
    EXPECT_GT(textIn(paper, box(x - 1.0, marker - 1.0, x + 1.0, marker + 1.0)), 4);
    // The note: about 16 mm of turned text from 1.2 mm above the marker.
    EXPECT_GT(textIn(paper, box(x - 1.7, marker + 1.4, x - 0.5, marker + 12.0)), 40);
    EXPECT_EQ(textIn(paper, box(x + 0.5, marker + 1.4, x + 1.7, marker + 12.0)), 0);
    // The offset, under the marker.
    EXPECT_GT(textIn(paper, box(x - 1.5, marker - 2.4, x + 1.5, marker - 1.3)), 5);

    // Hidden, it is neither drawn nor noted.
    auto hidden = viewport;
    hidden.hiddenLayers.hide("WATER");
    const QImage without = painted(sheetWith({hidden}), road.source(), nullptr, "_hidden");
    EXPECT_EQ(textIn(without, box(x - 1.7, marker - 2.4, x + 1.7, marker + 12.0)), 0);
}

TEST(SheetSections, NotesStandClearOfOtherServicesLinesAndOneWithNoRoomIsDropped)
{
    // Water 3 m left and gas 3.3 m left, both at 22.00: 1.5 mm apart on the
    // paper. The water, nearer the middle, is placed first; left of its line
    // its note would lie on the gas's, so it goes right, and the gas's note
    // goes left of the gas: each beside its own line and on no other.
    Road road;
    road.service("WATER", 53.0, 22.0);
    road.service("GAS", 53.3, 22.0);
    const auto viewport = crossSection(kCrossRect, 60.0, 200.0, Point2(0.0, 23.0));
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_EQ(stats.sectionNotesDropped, 0u);
    const double marker = crossY(22.0, 23.0);
    const double water = crossX(-3.0);
    const double gas = crossX(-3.3);
    EXPECT_GT(textIn(paper, box(water + 0.5, marker + 1.4, water + 1.7, marker + 12.0)), 40);
    EXPECT_GT(textIn(paper, box(gas - 1.7, marker + 1.4, gas - 0.5, marker + 12.0)), 40);
    // Between the two lines, nothing.
    EXPECT_EQ(textIn(paper, box(gas + 0.2, marker + 1.4, water - 0.2, marker + 12.0)), 0);

    // Power 3.6 m left as well: the gas, between two lines 1.5 mm either
    // side, has no place that is not on one of them. It is left out, and
    // counted; the power's note goes left of it.
    road.service("POWER", 53.6, 22.0);
    const QImage three = painted(sheetWith({viewport}), road.source(), &stats, "_three");
    EXPECT_EQ(stats.sectionNotesDropped, 1u);
    const double power = crossX(-3.6);
    EXPECT_GT(textIn(three, box(water + 0.5, marker + 1.4, water + 1.7, marker + 12.0)), 40);
    EXPECT_GT(textIn(three, box(power - 1.7, marker + 1.4, power - 0.5, marker + 12.0)), 40);
    EXPECT_EQ(textIn(three, box(power + 0.2, marker + 1.4, water - 0.2, marker + 12.0)), 0);
}

// ---- the automatic scale --------------------------------------------------------------

TEST(SheetSections, AnAutomaticSectionScaleIsFittedAndReportedInTheTitleBlock)
{
    const Road road;
    // The whole drawing area: 23..410 x 35..287. The band's four rows leave
    // the plot 45..408 x 75..285, 363 x 210 mm. The 180 m road in 90% of
    // 363 mm needs 1 : 551: 1 : 750. Its levels, 20.5 to 29.5, are 9 m; at
    // 1 : 750 90% of 210 mm allows 15.75 times: 10.
    const Box2 area = box(23.0, 35.0, 410.0, 287.0);
    auto viewport = longSection(area, 500.0, 1.0, Point2());
    viewport.autoScale = true;
    viewport.autoCentre = true;
    SheetPaintCache cache;
    const plotting::SectionFit fit = katana::qt::resolveSectionViewport(viewport, road.source(), cache);
    EXPECT_EQ(fit.scale, 750.0);
    EXPECT_EQ(fit.exaggeration, 10.0);

    // Painted automatically, the sheet is the sheet painted at H 1:750
    // V 1:75: its section, its title and its title block's scale.
    const QImage automatic = painted(sheetWith({viewport}), road.source(), nullptr, "_auto");
    auto fixed = viewport;
    fixed.autoScale = false;
    fixed.scale = 750.0;
    fixed.verticalExaggeration = 10.0;
    EXPECT_EQ(plotting::scaleText(fixed), "H 1:750 V 1:75");
    EXPECT_TRUE(automatic == painted(sheetWith({fixed}), road.source(), nullptr, "_fixed"));
    // Not the sheet at the viewport's own H 1:500 V 1:500: the title block's
    // scale cell says otherwise.
    const QImage own =
        painted(sheetWith({longSection(area, 500.0, 1.0, Point2())}), road.source(), nullptr, "_own");
    const QRect scaleCell = pixelsOf(plotting::builtInFrame()->cell("scale")->rect);
    EXPECT_NE(automatic.copy(scaleCell), own.copy(scaleCell));
}

TEST(SheetSections, AutomaticCrossSectionsShareOneScaleThatFitsTheirWidth)
{
    // Two cross sections, 20 m each side, in 23..210 x 35..287: each row is
    // 122.5 mm of the 245 above the title, its plot 179 x 111 mm (a 6 mm
    // level column; the offsets and caption under it). 40 m in 90% of 179 mm
    // needs 1 : 248: 1 : 250. The deeper section spans 2.3 m of level (at
    // CH 60 the ground's 22.7 to the design's 25; at CH 120 the design's 25
    // to the ground's 27.3): 9.2 mm at 1 : 250, so 10 times fits 90% of
    // 111 mm and 20 does not.
    const Road road;
    auto viewport = crossSection(box(23.0, 35.0, 210.0, 287.0), 60.0, 100.0, Point2());
    viewport.source.stations = {60.0, 120.0};
    viewport.autoScale = true;
    viewport.autoCentre = true;
    SheetPaintCache cache;
    const plotting::SectionFit fit = katana::qt::resolveSectionViewport(viewport, road.source(), cache);
    EXPECT_EQ(fit.scale, 250.0);
    EXPECT_EQ(fit.exaggeration, 10.0);
    SheetPaintStats stats;
    painted(sheetWith({viewport}), road.source(), &stats);
    EXPECT_TRUE(stats.problems.empty());
    EXPECT_EQ(stats.viewportsDrawn, 1u);

    // A viewport that is not automatic keeps its own.
    viewport.autoScale = false;
    const plotting::SectionFit own = katana::qt::resolveSectionViewport(viewport, road.source(), cache);
    EXPECT_EQ(own.scale, 100.0);
    EXPECT_EQ(own.exaggeration, 1.0);
}

TEST(SheetSections, AStoredScaleThatIsNotPositiveIsReportedNotDrawn)
{
    // A scale read from a file unchecked: 0 would put every label at infinity.
    const Road road;
    auto viewport = longSection(kLongRect, 0.0, 10.0, Point2(90.0, 25.0));
    SheetPaintStats stats;
    const QImage paper = painted(sheetWith({viewport}), road.source(), &stats);
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_NE(stats.problems.front().find("not a positive number"), std::string::npos);
    EXPECT_EQ(stats.viewportsDrawn, 0u);
    EXPECT_EQ(colourIn(paper, kLongRect, kCut), 0);
}

// ---- nothing outside, nothing over another ---------------------------------------------

TEST(SheetSections, NothingOfASectionIsDrawnOutsideItsViewport)
{
    // A long section with a range and cross sections full of services, on a
    // sheet with no frame: every pixel off the two rectangles (and their
    // 0.25 mm outlines) is still the paper's white.
    Road road;
    road.service("WATER", 55.0, 22.0);
    road.service("GAS", 53.3, 22.5);
    road.service("POWER", 45.0, 30.0);
    road.service("KERB", 47.0, std::nullopt);
    auto along = longSection(box(40.0, 170.0, 300.0, 260.0), 500.0, 10.0, Point2(90.0, 25.0));
    along.source.chainageFrom = 20.0;
    along.source.chainageTo = 160.0;
    along.autoScale = true;
    along.autoCentre = true;
    auto across = crossSection(box(60.0, 40.0, 200.0, 150.0), 60.0, 100.0, Point2(0.0, 24.0));
    across.id = "vp2";
    across.source.stations = {40.0, 90.0, 140.0};
    across.autoScale = true;
    across.autoCentre = true;
    auto set = sheetWith({along, across});
    set.sheets.front().frame.clear();
    SheetPaintStats stats;
    const QImage paper = painted(set, road.source(), &stats);
    ASSERT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_EQ(stats.viewportsDrawn, 2u);

    const auto inside = [](Point2 p, const Box2& r) {
        return p.x > r.min.x - 0.6 && p.x < r.max.x + 0.6 && p.y > r.min.y - 0.6 && p.y < r.max.y + 0.6;
    };
    int outside = 0;
    for (int y = 0; y < paper.height(); ++y) {
        for (int x = 0; x < paper.width(); ++x) {
            const Point2 at((x + 0.5) / kPpmm, 297.0 - (y + 0.5) / kPpmm);
            if (!inside(at, along.rect) && !inside(at, across.rect) && paper.pixel(x, y) != kWhite) {
                ++outside;
            }
        }
    }
    EXPECT_EQ(outside, 0);
    // And something is drawn in each.
    EXPECT_GT(textIn(paper, along.rect), 500);
    EXPECT_GT(textIn(paper, across.rect), 500);
}

TEST(SheetSections, AxisValuesStepOutUntilTheyClearEachOther)
{
    // A road from CH 100000 drawn at 1 : 5 in a viewport too low for its
    // band (40 mm: the plot is 26 mm with the band's 32 under it), so its
    // chainages are written under the plot, at y 159.7..161.2. At 200 mm a
    // metre the 10 mm grid would be 0.05 m, and "100090.05" is wider than
    // 10 mm less the 2 mm between labels: the step goes to 0.1 m, 20 mm.
    const Road road(true, 100000.0);
    auto viewport = longSection(box(23.0, 150.0, 410.0, 190.0), 5.0, 1.0, Point2());
    viewport.autoCentre = true;
    const QImage paper = painted(sheetWith({viewport}), road.source());

    // The columns with ink along the labels' strip, and the runs of them:
    // glyphs of one label are under 0.6 mm apart, labels much more.
    const QRect strip = pixelsOf(box(24.0, 159.8, 409.0, 161.1));
    std::vector<int> inked;
    for (int x = strip.left(); x <= strip.right(); ++x) {
        for (int y = strip.top(); y <= strip.bottom(); ++y) {
            if (textIn(paper, box(x / kPpmm, 297.0 - (y + 1) / kPpmm, (x + 1) / kPpmm,
                                  297.0 - y / kPpmm)) > 0) {
                inked.push_back(x);
                break;
            }
        }
    }
    ASSERT_FALSE(inked.empty());
    std::vector<std::pair<int, int>> labels{{inked.front(), inked.front()}};
    for (const int x : inked) {
        if (x - labels.back().second > static_cast<int>(0.6 * kPpmm)) {
            labels.emplace_back(x, x);
        } else {
            labels.back().second = x;
        }
    }
    ASSERT_GE(labels.size(), 10u);
    for (std::size_t i = 1; i < labels.size(); ++i) {
        // Each at least 2 mm clear of the one before, and the grid 20 mm.
        EXPECT_GE(labels[i].first - labels[i - 1].second, static_cast<int>(2.0 * kPpmm) - 1) << i;
        const double pitch = ((labels[i].first + labels[i].second) -
                              (labels[i - 1].first + labels[i - 1].second)) /
                             (2.0 * kPpmm);
        EXPECT_NEAR(pitch, 20.0, 1.0) << i;
    }
}

// ---- a sheet to look at -----------------------------------------------------------------

TEST(SheetSections, ASheetOfALongSectionAndServiceCrossSections)
{
    // What a drafter would plot: the road's long section from CH 20 to 160
    // across the top, and three cross sections through its services below,
    // all automatic. Written to KATANA_SHEET_PNG to be looked at.
    Road road;
    road.service("WATER", 55.0, 22.0);
    road.service("GAS", 53.3, 22.5);
    road.service("POWER", 42.0, 30.0);
    road.service("KERB", 46.0, std::nullopt);
    auto along = longSection(box(24.0, 160.0, 409.0, 286.0), 500.0, 10.0, Point2());
    along.source.chainageFrom = 20.0;
    along.source.chainageTo = 160.0;
    along.autoScale = true;
    along.autoCentre = true;
    auto across = crossSection(box(24.0, 36.0, 409.0, 157.0), 60.0, 100.0, Point2());
    across.source.stations = {40.0, 140.0};
    across.autoScale = true;
    across.autoCentre = true;
    SheetPaintStats stats;
    painted(sheetWith({along, across}), road.source(), &stats);
    EXPECT_TRUE(stats.problems.empty());
    EXPECT_EQ(stats.viewportsDrawn, 2u);
    EXPECT_EQ(stats.sectionNotesDropped, 0u);
}

TEST(SheetSections, WhatCannotBeDrawnIsReportedNotCountedAsDrawn)
{
    const Road road;
    // A range that lies off the plot of a fixed centre: 30 m to 50 m at
    // 1 : 1000, centred 500 m away.
    auto off = longSection(kLongRect, 1000.0, 10.0, Point2(590.0, 25.0));
    off.source.chainageFrom = 30.0;
    off.source.chainageTo = 50.0;
    SheetPaintStats stats;
    painted(sheetWith({off}), road.source(), &stats, "_off");
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_NE(stats.problems.front().find("off the plot"), std::string::npos);
    EXPECT_EQ(stats.viewportsDrawn, 0u);

    // A viewport too small for any plot.
    const auto tiny = longSection(box(23.0, 150.0, 40.0, 162.0), 1000.0, 10.0, Point2(90.0, 25.0));
    painted(sheetWith({tiny}), road.source(), &stats, "_tiny");
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_NE(stats.problems.front().find("no room"), std::string::npos);
    EXPECT_EQ(stats.viewportsDrawn, 0u);

    // A chainage that is not a finite number, as a file could hold.
    auto infinite = longSection(kLongRect, 1000.0, 10.0, Point2(90.0, 25.0));
    infinite.source.chainageTo = std::numeric_limits<double>::infinity();
    painted(sheetWith({infinite}), road.source(), &stats, "_infinite");
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_NE(stats.problems.front().find("not a finite number"), std::string::npos);
    EXPECT_EQ(stats.viewportsDrawn, 0u);
}
