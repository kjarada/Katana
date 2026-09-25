// A plan's coordinate grid and the live key plan, as the sheet painter draws
// them (plan_grid.hpp, key_plan.hpp, src/katana_qt/sheet_painter): the grid's
// lines, crosses, ticks and labels where the geometry puts them, kept off the
// view's furniture; a key plan outlining the sheets as they are now, its own
// sheet shaded, and fitted to them all. Painted into QImages at 4 px a
// millimetre and read back pixel by pixel.
//
// With KATANA_SHEET_PNG set to a directory, every sheet a test paints is also
// written there as a PNG, to be looked at.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <functional>
#include <string>
#include <utility>
#include <vector>

#include <QColor>
#include <QImage>
#include <QPainter>

#include "katana/cad/plotting/key_plan.hpp"
#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/entity/model.hpp"
#include "katana/math/numerics.hpp"
#include "sheet_painter.hpp"

using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::qt::SheetPaintCache;
using katana::qt::SheetPaintOptions;
using katana::qt::SheetPaintStats;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;
using plotting::GridStyle;

namespace {

constexpr double kPpmm = 4.0;
constexpr double kPaperHeight = 297.0; // A3 landscape
const QRgb kWhite = qRgb(255, 255, 255);

Box2 box(double x0, double y0, double x1, double y1)
{
    return Box2(Point2(x0, y0), Point2(x1, y1));
}

// An A3 sheet without a frame, so nothing but the viewports is on it.
plotting::Sheet bare(std::string id, std::vector<plotting::Viewport> viewports = {})
{
    plotting::Sheet sheet;
    sheet.id = id;
    sheet.name = std::move(id);
    sheet.frame.clear();
    sheet.viewports = std::move(viewports);
    return sheet;
}

plotting::Viewport plan(std::string id, Box2 rect, double scale, Point2 centre,
                        double rotation = 0.0)
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = plotting::ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.centre = centre;
    viewport.rotation = rotation;
    return viewport;
}

plotting::Viewport keyPlan(std::string id, Box2 rect, double scale, Point2 centre)
{
    plotting::Viewport viewport = plan(std::move(id), rect, scale, centre);
    viewport.kind = plotting::ViewportKind::KeyPlan;
    return viewport;
}

// The 200 x 100 mm plan the grid tests draw: 1:1000 over (305 230,
// 6 250 410), so E 305 150 ... 305 300 fall at x 120 ... 270 and N 6 250 400
// and 6 250 450 at y 140 and 190.
plotting::Viewport gridded(GridStyle style, double interval, double rotation = 0.0)
{
    plotting::Viewport viewport = plan("vp1", box(100.0, 100.0, 300.0, 200.0), 1000.0,
                                       Point2(305230.0, 6250410.0), rotation);
    viewport.gridStyle = style;
    viewport.gridInterval = interval;
    return viewport;
}

QImage painted(const plotting::SheetSet& set, std::size_t index, const SheetSource& source,
               SheetPaintStats* stats = nullptr, const char* tag = "")
{
    const auto& sheet = set.sheets[index];
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    QImage image(static_cast<int>(std::lround(paper.widthMm * kPpmm)),
                 static_cast<int>(std::lround(paper.heightMm * kPpmm)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::gray);
    SheetPaintOptions options;
    options.pixelsPerMillimetre = kPpmm;
    SheetPaintCache cache;
    QPainter painter(&image);
    const SheetPaintStats result =
        katana::qt::paintSheet(painter, set, index, source, options, cache);
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

QImage painted(const plotting::Sheet& sheet, const SheetSource& source,
               SheetPaintStats* stats = nullptr, const char* tag = "")
{
    plotting::SheetSet set;
    set.sheets.push_back(sheet);
    return painted(set, 0, source, stats, tag);
}

// Calls `visit` with the colour of every pixel in the paper box.
void eachPixel(const QImage& image, Box2 paper, const std::function<void(QColor)>& visit)
{
    const int x0 = static_cast<int>(std::floor(paper.min.x * kPpmm));
    const int x1 = static_cast<int>(std::floor(paper.max.x * kPpmm));
    const int y0 = static_cast<int>(std::floor((kPaperHeight - paper.max.y) * kPpmm));
    const int y1 = static_cast<int>(std::floor((kPaperHeight - paper.min.y) * kPpmm));
    for (int y = std::max(y0, 0); y <= std::min(y1, image.height() - 1); ++y) {
        for (int x = std::max(x0, 0); x <= std::min(x1, image.width() - 1); ++x) {
            visit(image.pixelColor(x, y));
        }
    }
}

// Pixels in the paper box that are not white.
int inkIn(const QImage& image, Box2 paper)
{
    int count = 0;
    eachPixel(image, paper, [&count](QColor c) { count += c.rgb() != kWhite ? 1 : 0; });
    return count;
}

// The darkest pixel in the box: 255 for white paper, 0 for black ink.
int darkestIn(const QImage& image, Box2 paper)
{
    int darkest = 255;
    eachPixel(image, paper, [&darkest](QColor c) {
        darkest = std::min({darkest, c.red(), c.green(), c.blue()});
    });
    return darkest;
}

// Pixels whose colour matches `test`.
int countIn(const QImage& image, Box2 paper, const std::function<bool(QColor)>& test)
{
    int count = 0;
    eachPixel(image, paper, [&](QColor c) { count += test(c) ? 1 : 0; });
    return count;
}

// A key plan's red, for the other sheets' outlines and numbers.
bool reddish(QColor c) { return c.red() - c.green() > 70 && c.red() - c.blue() > 60; }
// Its blue, for "you are here".
bool bluish(QColor c) { return c.blue() - c.red() > 50; }

// Whether two images agree on every pixel of the paper box; where they first
// differ, in paper millimetres, when they do not.
::testing::AssertionResult sameIn(const QImage& a, const QImage& b, Box2 paper)
{
    const int x0 = static_cast<int>(std::floor(paper.min.x * kPpmm));
    const int x1 = static_cast<int>(std::floor(paper.max.x * kPpmm));
    const int y0 = static_cast<int>(std::floor((kPaperHeight - paper.max.y) * kPpmm));
    const int y1 = static_cast<int>(std::floor((kPaperHeight - paper.min.y) * kPpmm));
    for (int y = y0; y <= y1; ++y) {
        for (int x = x0; x <= x1; ++x) {
            if (a.pixel(x, y) != b.pixel(x, y)) {
                return ::testing::AssertionFailure()
                       << "they differ at (" << (x + 0.5) / kPpmm << ", "
                       << kPaperHeight - (y + 0.5) / kPpmm << ") mm: "
                       << QColor(a.pixel(x, y)).name().toStdString() << " and "
                       << QColor(b.pixel(x, y)).name().toStdString();
            }
        }
    }
    return ::testing::AssertionSuccess();
}

void addLine(Model& model, Point2 a, Point2 b)
{
    Entity entity;
    entity.geometry = Segment2{a, b};
    entity.layer = "0";
    ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
}

// A key-plan sheet and two plans side by side, 100 x 50 m each at 1:1000:
// sheet 2 over E 950..1050, sheet 3 over E 1050..1150, both N 1975..2025.
// The key plan, 200 x 150 mm at 1:2000 over (1050, 2000), shows sheet 2's
// outline at x 150..200, y 162.5..187.5, and sheet 3's at x 200..250.
plotting::SheetSet keyAndTwoPlans()
{
    const Box2 onPaper = box(50.0, 50.0, 150.0, 100.0);
    plotting::SheetSet set;
    set.sheets.push_back(bare(
        "s1", {keyPlan("vp1", box(100.0, 100.0, 300.0, 250.0), 2000.0, Point2(1050.0, 2000.0))}));
    set.sheets.push_back(bare("s2", {plan("vp2", onPaper, 1000.0, Point2(1000.0, 2000.0))}));
    set.sheets.push_back(bare("s3", {plan("vp3", onPaper, 1000.0, Point2(1100.0, 2000.0))}));
    return set;
}

} // namespace

// ---- the coordinate grid ---------------------------------------------------------------

TEST(SheetGrid, LinesAreFaintGreyAtTheirCoordinatesAndLabelledAtTheEdges)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    SheetPaintStats stats;
    const QImage lines = painted(bare("s1", {gridded(GridStyle::Lines, 50.0)}), source, &stats);
    EXPECT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_EQ(stats.viewportsDrawn, 1u);

    // E 305 200 runs up x 170 and N 6 250 400 across y 140: light grey, a
    // 0.13 mm hairline, never black.
    EXPECT_GT(inkIn(lines, box(169.5, 150.0, 170.5, 185.0)), 30);
    EXPECT_GE(darkestIn(lines, box(169.5, 150.0, 170.5, 185.0)), 180);
    EXPECT_GT(inkIn(lines, box(230.0, 139.5, 260.0, 140.5)), 30);
    // Nothing between the lines.
    EXPECT_EQ(inkIn(lines, box(140.0, 150.0, 150.0, 185.0)), 0);

    // Its labels, in ink: "E 305 200" along the bottom, centred on x 170,
    // and "N 6 250 400" up the left side, centred on y 140.
    EXPECT_LT(darkestIn(lines, box(165.0, 100.9, 175.0, 102.5)), 100);
    EXPECT_LT(darkestIn(lines, box(100.9, 135.0, 102.5, 145.0)), 100);
    // Up the right side and along the top too.
    EXPECT_LT(darkestIn(lines, box(297.5, 135.0, 299.1, 145.0)), 100);
    EXPECT_LT(darkestIn(lines, box(165.0, 197.5, 175.0, 199.1)), 100);

    // No grid: none of it.
    const QImage none =
        painted(bare("s1", {gridded(GridStyle::None, 50.0)}), source, nullptr, "_none");
    EXPECT_EQ(inkIn(none, box(169.5, 150.0, 170.5, 185.0)), 0);
    EXPECT_EQ(inkIn(none, box(165.0, 100.9, 175.0, 102.5)), 0);
    EXPECT_EQ(inkIn(none, box(100.9, 135.0, 102.5, 145.0)), 0);
}

TEST(SheetGrid, CrossesMarkTheIntersectionsAndTicksOnlyTheBorder)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    const QImage crosses = painted(bare("s1", {gridded(GridStyle::Crosses, 50.0)}), source);
    // A 3 mm cross at (170, 140): both arms, in ink.
    EXPECT_GT(inkIn(crosses, box(168.7, 139.6, 169.6, 140.4)), 0);
    EXPECT_GT(inkIn(crosses, box(169.6, 138.7, 170.4, 139.6)), 0);
    EXPECT_LT(darkestIn(crosses, box(168.5, 139.5, 171.5, 140.5)), 180);
    // Nothing along the line between two crosses, nor past an arm's end.
    EXPECT_EQ(inkIn(crosses, box(169.0, 145.0, 171.0, 185.0)), 0);
    EXPECT_EQ(inkIn(crosses, box(172.0, 139.0, 180.0, 141.0)), 0);

    const QImage ticks = painted(bare("s1", {gridded(GridStyle::Ticks, 50.0)}), source, nullptr,
                                 "_ticks");
    // A 2.5 mm tick in from the bottom border at x 170, and from the top.
    EXPECT_GT(inkIn(ticks, box(169.6, 100.3, 170.4, 102.2)), 0);
    EXPECT_GT(inkIn(ticks, box(169.6, 197.8, 170.4, 199.7)), 0);
    // Nothing inside: no line, no cross.
    EXPECT_EQ(inkIn(ticks, box(168.0, 108.0, 172.0, 190.0)), 0);
    // Its label past the tick's end: 2.5 mm of tick and 0.8 of gap.
    EXPECT_LT(darkestIn(ticks, box(165.0, 103.4, 175.0, 105.0)), 100);
}

TEST(SheetGrid, ARotatedGridIsDrawnWhereItsGeometrySays)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    const plotting::Viewport viewport = gridded(GridStyle::Lines, 20.0, katana::math::kPi / 6.0);
    const QImage image = painted(bare("s1", {viewport}), source);

    // Every line of the headless grid is on the paper halfway along it (the
    // labels, knocked out at its ends, are nowhere near its middle).
    const auto grid = plotting::planGrid(viewport, plotting::storedPlacement(viewport));
    ASSERT_TRUE(grid.ok());
    ASSERT_GE(grid->lines.size(), 10u);
    for (const plotting::GridLine& line : grid->lines) {
        const Point2 middle = (line.from + line.to) * 0.5;
        if ((line.to - line.from).length() < 20.0 || middle.x < 140.0) {
            continue; // a corner's short line, or the title's corner
        }
        const Box2 around = box(middle.x - 0.5, middle.y - 0.5, middle.x + 0.5, middle.y + 0.5);
        EXPECT_GT(inkIn(image, around), 0)
            << (line.axis == plotting::GridAxis::Easting ? "E " : "N ") << line.value;
    }
    // The viewport's centre, (305 230, 6 250 410), is the middle of a 20 m
    // square of the grid: 10 mm from every line, however it is turned.
    EXPECT_EQ(inkIn(image, box(197.0, 147.0, 203.0, 153.0)), 0);
    // A square, not a line level across the paper: the paper's own axes
    // through the centre cross the grid askew, so the ink on them is in
    // short runs.
    int run = 0;
    int longest = 0;
    for (double x = 150.0; x <= 250.0; x += 0.25) {
        run = inkIn(image, box(x, 150.0, x, 150.0)) > 0 ? run + 1 : 0;
        longest = std::max(longest, run);
    }
    EXPECT_LE(longest, 6);
}

TEST(SheetGrid, AGridTooFineToReadIsReportedAndThePlanStillDrawn)
{
    Model model;
    addLine(model, Point2(305200.0, 6250410.0), Point2(305260.0, 6250410.0));
    SheetSource source;
    source.plan.model = &model;
    plotting::Viewport dense = gridded(GridStyle::Lines, 0.5); // 0.5 mm apart at 1:1000
    SheetPaintStats stats;
    const QImage image = painted(bare("s1", {dense}), source, &stats);
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_EQ(stats.problems[0].rfind("vp1: a 0.5 m grid at 1:1000 is 0.5 mm apart", 0), 0u)
        << stats.problems[0];
    EXPECT_EQ(stats.viewportsDrawn, 1u);
    // The drawing is there, and no grey smear over it.
    EXPECT_GT(inkIn(image, box(175.0, 149.5, 225.0, 150.5)), 50);
    EXPECT_EQ(inkIn(image, box(150.0, 160.0, 250.0, 190.0)), 0);
}

TEST(SheetGrid, LabelsKeepOffTheScaleBarTitleAndNorthArrow)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    // Crosses, so nothing but a label inks the strip along the bottom. E
    // 305 250 falls at x 243, the left edge of the scale bar's knock-out
    // (243..299 x 100.8..109.4 for its 50 mm bar); its label, about 10 mm
    // wide, would straddle that edge.
    plotting::Viewport viewport = gridded(GridStyle::Crosses, 50.0);
    viewport.centre.x = 305207.0;
    viewport.scaleBar = true;
    viewport.northArrow = true;
    const QImage furnished = painted(bare("s1", {viewport}), source);
    EXPECT_EQ(inkIn(furnished, box(238.5, 100.9, 242.5, 102.5)), 0);
    // The label of E 305 200, clear of it all, is there.
    EXPECT_LT(darkestIn(furnished, box(188.0, 100.9, 198.0, 102.5)), 100);

    // Without the scale bar the label is set.
    viewport.scaleBar = false;
    const QImage bareBar = painted(bare("s1", {viewport}), source, nullptr, "_no_scale_bar");
    EXPECT_LT(darkestIn(bareBar, box(238.5, 100.9, 242.5, 102.5)), 100);

    // The title, the scale bar and the north arrow read exactly as they do
    // with no grid at all.
    viewport.scaleBar = true;
    viewport.gridStyle = GridStyle::Lines;
    viewport.gridInterval = 10.0; // a line every 10 mm, through all three
    const QImage lined = painted(bare("s1", {viewport}), source, nullptr, "_lines");
    viewport.gridStyle = GridStyle::None;
    const QImage plain = painted(bare("s1", {viewport}), source, nullptr, "_plain");
    EXPECT_TRUE(sameIn(lined, plain, box(102.3, 102.2, 115.0, 106.6)));  // "PLAN 1:1000"
    EXPECT_TRUE(sameIn(lined, plain, box(244.0, 101.3, 298.0, 109.0))); // the scale bar
    EXPECT_TRUE(sameIn(lined, plain, box(287.0, 185.0, 297.0, 199.0))); // the north arrow
    EXPECT_FALSE(sameIn(lined, plain, box(120.0, 120.0, 280.0, 180.0)));
}

// ---- the live key plan -----------------------------------------------------------------

TEST(SheetKeyPlan, TheOtherSheetsAreOutlinedAsTheyAreNow)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::SheetSet set = keyAndTwoPlans();
    const QImage before = painted(set, 0, source);
    // Sheet 2's outline: its left side at x 150, its number in its middle.
    EXPECT_GT(countIn(before, box(149.5, 170.0, 150.5, 180.0), reddish), 10);
    EXPECT_GT(countIn(before, box(170.0, 170.0, 180.0, 180.0), reddish), 5);
    // Sheet 3's, beside it.
    EXPECT_GT(countIn(before, box(249.5, 170.0, 250.5, 180.0), reddish), 10);
    EXPECT_GT(countIn(before, box(220.0, 170.0, 230.0, 180.0), reddish), 5);

    // Sheet 3's plan panned 100 m north: its outline follows at once, 50 mm
    // up the key plan. Nothing stored had to be brought up to date.
    set.sheets[2].viewports[0].centre = Point2(1100.0, 2100.0);
    const QImage after = painted(set, 0, source, nullptr, "_panned");
    EXPECT_EQ(countIn(after, box(249.5, 170.0, 250.5, 180.0), reddish), 0);
    EXPECT_EQ(countIn(after, box(220.0, 170.0, 230.0, 180.0), reddish), 0);
    EXPECT_GT(countIn(after, box(249.5, 220.0, 250.5, 230.0), reddish), 10);
    EXPECT_GT(countIn(after, box(220.0, 220.0, 230.0, 230.0), reddish), 5);
    // Sheet 2's stays.
    EXPECT_GT(countIn(after, box(149.5, 170.0, 150.5, 180.0), reddish), 10);

    // A sheet removed is gone from it.
    set.sheets.erase(set.sheets.begin() + 2);
    const QImage removed = painted(set, 0, source, nullptr, "_removed");
    EXPECT_EQ(countIn(removed, box(249.5, 220.0, 250.5, 230.0), reddish), 0);
    EXPECT_GT(countIn(removed, box(149.5, 170.0, 150.5, 180.0), reddish), 10);
}

TEST(SheetKeyPlan, TheNumberIsTheSheetsNumberNow)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::SheetSet set = keyAndTwoPlans();
    // The number over sheet 3's outline, cut out of two paints: "3" as the
    // set is, and "2" once sheet 3 is moved ahead of sheet 2. The outline
    // itself does not move.
    const Box2 number = box(218.0, 170.0, 232.0, 180.0);
    const QImage three = painted(set, 0, source);
    std::swap(set.sheets[1], set.sheets[2]);
    const QImage two = painted(set, 0, source, nullptr, "_reordered");
    EXPECT_FALSE(sameIn(three, two, number));
    EXPECT_TRUE(sameIn(three, two, box(249.3, 165.0, 250.7, 185.0)));
    // And it is the "2" sheet 2's outline had before: the same pixels, 50 mm
    // along (a stray edge pixel of antialiasing allowed).
    const auto cut = [](const QImage& image, Box2 at) {
        return image.copy(static_cast<int>(std::lround(at.min.x * kPpmm)),
                          static_cast<int>(std::lround((kPaperHeight - at.max.y) * kPpmm)),
                          static_cast<int>(std::lround(at.width() * kPpmm)),
                          static_cast<int>(std::lround(at.height() * kPpmm)));
    };
    const QImage now = cut(two, number);
    const QImage then = cut(three, box(168.0, 170.0, 182.0, 180.0));
    ASSERT_EQ(now.size(), then.size());
    int differing = 0;
    int red = 0;
    for (int y = 0; y < now.height(); ++y) {
        for (int x = 0; x < now.width(); ++x) {
            const QColor a = now.pixelColor(x, y);
            const QColor b = then.pixelColor(x, y);
            differing += std::abs(a.red() - b.red()) > 16 || std::abs(a.green() - b.green()) > 16 ||
                                 std::abs(a.blue() - b.blue()) > 16
                             ? 1
                             : 0;
            red += reddish(a) ? 1 : 0;
        }
    }
    EXPECT_GT(red, 20);
    EXPECT_LE(differing, 4);
}

TEST(SheetKeyPlan, ItsOwnSheetsPlanIsShadedAsYouAreHere)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::SheetSet set = keyAndTwoPlans();
    // Sheet 2 gets a key plan inset of its own, beside its plan: 1:2000 over
    // (1050, 2000) in 200..400 x 100..250, so its own plan is outlined at
    // x 250..300, y 162.5..187.5, and sheet 3's at x 300..350.
    set.sheets[1].viewports.push_back(
        keyPlan("vp4", box(200.0, 100.0, 400.0, 250.0), 2000.0, Point2(1050.0, 2000.0)));
    const QImage here = painted(set, 1, source);
    EXPECT_GT(countIn(here, box(255.0, 165.0, 265.0, 170.0), bluish), 30 * 16);
    EXPECT_EQ(countIn(here, box(330.0, 165.0, 340.0, 170.0), bluish), 0);
    EXPECT_GT(countIn(here, box(349.5, 170.0, 350.5, 180.0), reddish), 10);

    // From the key-plan sheet, sheet 2's plan is just another outline.
    const QImage elsewhere = painted(set, 0, source, nullptr, "_key_sheet");
    EXPECT_EQ(countIn(elsewhere, box(155.0, 165.0, 165.0, 170.0), bluish), 0);
    EXPECT_GT(countIn(elsewhere, box(149.5, 170.0, 150.5, 180.0), reddish), 10);
}

TEST(SheetKeyPlan, StoredOutlinesAreNotDrawnButItsOtherMarksAre)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::SheetSet set = keyAndTwoPlans();
    plotting::Viewport& key = set.sheets[0].viewports[0];
    // An outline stored when the key plan was made, of where sheet 3 was
    // then (20 x 10 mm around (200, 205) on the paper), and a match line.
    plotting::WorldMark stale;
    stale.kind = plotting::WorldMark::Kind::SheetOutline;
    stale.sheet = "s3";
    stale.points = {Point2(1030.0, 2050.0), Point2(1070.0, 2050.0), Point2(1070.0, 2070.0),
                    Point2(1030.0, 2070.0)};
    key.marks.push_back(stale);
    plotting::WorldMark match;
    match.points = {Point2(990.0, 1930.0), Point2(1110.0, 1930.0)};
    match.label = "MATCH LINE";
    key.marks.push_back(match);

    const QImage image = painted(set, 0, source);
    EXPECT_EQ(countIn(image, box(185.0, 195.0, 215.0, 215.0), reddish), 0);
    // The match line, dash-dot in ink at y 140 from x 170 to 230.
    EXPECT_LT(darkestIn(image, box(175.0, 139.5, 225.0, 140.5)), 100);
}

TEST(SheetKeyPlan, AnAutomaticKeyPlanTakesInEverySheetAndTheDrawing)
{
    Model model;
    addLine(model, Point2(0.0, 0.0), Point2(10.0, 10.0));
    SheetSource source;
    source.plan.model = &model;
    plotting::SheetSet set = keyAndTwoPlans();
    plotting::Viewport& key = set.sheets[0].viewports[0];
    key.autoScale = true;
    key.autoCentre = true;

    // On the drawing alone it would show the line at the origin and neither
    // sheet; with the set it takes in both.
    const auto drawingOnly = katana::qt::resolvePlanViewport(key, source);
    const auto withSheets = katana::qt::resolvePlanViewport(key, source, set, 0);
    const auto outlines = plotting::keyPlanOutlines(set, 0);
    ASSERT_EQ(outlines.size(), 2u);
    const auto inside = [&](const katana::qt::ResolvedViewport& at, const Point2& world) {
        return key.rect.contains(
            plotting::planWorldToPaper(key, {at.scale, at.centre}, world));
    };
    for (const plotting::KeyPlanOutline& outline : outlines) {
        for (const Point2& corner : outline.corners) {
            EXPECT_FALSE(inside(drawingOnly, corner));
            EXPECT_TRUE(inside(withSheets, corner));
        }
    }
    EXPECT_TRUE(inside(withSheets, Point2(0.0, 0.0)));
    EXPECT_TRUE(inside(withSheets, Point2(10.0, 10.0)));
    // A plan is fitted as before: the set changes nothing for it.
    const plotting::Viewport& tile = set.sheets[1].viewports[0];
    plotting::Viewport automatic = tile;
    automatic.autoScale = true;
    automatic.autoCentre = true;
    const auto alone = katana::qt::resolvePlanViewport(automatic, source);
    const auto inSet = katana::qt::resolvePlanViewport(automatic, source, set, 1);
    EXPECT_EQ(alone.scale, inSet.scale);
    EXPECT_EQ(alone.centre, inSet.centre);

    SheetPaintStats stats;
    const QImage image = painted(set, 0, source, &stats);
    EXPECT_TRUE(stats.problems.empty());
    // Both outlines are on the paper, wherever the fit put them.
    const plotting::PlanPlacement placed{withSheets.scale, withSheets.centre};
    for (const plotting::KeyPlanOutline& outline : outlines) {
        const Point2 a = plotting::planWorldToPaper(key, placed, outline.corners[0]);
        const Point2 b = plotting::planWorldToPaper(key, placed, outline.corners[1]);
        const Point2 middle = (a + b) * 0.5; // its bottom side's middle
        EXPECT_GT(countIn(image, box(middle.x - 1.0, middle.y - 0.6, middle.x + 1.0, middle.y + 0.6),
                          reddish),
                  0);
    }
}

TEST(SheetKeyPlan, PlacedHeadlessOverTheModelEveryViewportIsWhereThePainterPutsIt)
{
    // placePlan (key_plan.hpp) is what a command line without a window
    // places plans with; over a drawing of entities it must agree with the
    // painter: a plan over the drawing less a hidden layer, one along a stretch
    // of an alignment, a fixed one, and an automatic key plan in a set.
    Model model;
    katana::entity::Layer far;
    far.name = "FAR";
    ASSERT_TRUE(model.layers.add(far));
    addLine(model, Point2(100.0, 200.0), Point2(460.0, 330.0));
    Entity stray;
    stray.geometry = Segment2{Point2(9000.0, 9000.0), Point2(9010.0, 9000.0)};
    stray.layer = "FAR";
    ASSERT_TRUE(model.entities.add(std::move(stray)).ok());
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{Point2(0.0, 0.0)}, {Point2(300.0, 0.0)}, {Point2(300.0, 400.0)}};
    road.horizontal.pis[1].radius = 80.0;
    ASSERT_TRUE(model.alignments.add(road).ok());
    SheetSource source;
    source.plan.model = &model;

    plotting::Viewport drawing = plan("vp1", box(40.0, 60.0, 290.0, 230.0), 500.0, Point2(), 0.3);
    drawing.autoScale = true;
    drawing.autoCentre = true;
    drawing.hiddenLayers.hide("FAR");
    plotting::Viewport along = plan("vp2", box(30.0, 40.0, 400.0, 150.0), 500.0, Point2(), -0.2);
    along.autoCentre = true;
    along.autoScale = true;
    along.source.alignment = "ROAD";
    along.source.chainageFrom = 150.0;
    along.source.chainageTo = 420.0;
    plotting::Viewport fixed = plan("vp3", box(30.0, 40.0, 130.0, 90.0), 250.0, Point2(7.0, 8.0));
    plotting::Viewport key = keyPlan("vp4", box(300.0, 40.0, 400.0, 110.0), 1.0, Point2());
    key.autoScale = true;
    key.autoCentre = true;
    plotting::SheetSet set;
    set.sheets.push_back(bare("s1", {drawing, key}));
    set.sheets.push_back(bare("s2", {along}));
    set.sheets.push_back(bare("s3", {fixed}));

    for (std::size_t sheet = 0; sheet < set.sheets.size(); ++sheet) {
        for (const plotting::Viewport& viewport : set.sheets[sheet].viewports) {
            const auto painter = katana::qt::resolvePlanViewport(viewport, source, set, sheet);
            const plotting::PlanPlacement headless =
                plotting::placePlan(model, set, sheet, viewport);
            EXPECT_EQ(painter.scale, headless.scale) << viewport.id;
            EXPECT_NEAR(painter.centre.x, headless.centre.x, 1e-6) << viewport.id;
            EXPECT_NEAR(painter.centre.y, headless.centre.y, 1e-6) << viewport.id;
        }
    }
    // Hidden, the stray is not fitted; the key plan still takes in all three
    // sheets, and the fixed plan kept its own.
    EXPECT_LE(plotting::placePlan(model, drawing).scale, 5000.0);
    plotting::Viewport shown = drawing;
    shown.hiddenLayers.show("FAR");
    EXPECT_GE(plotting::placePlan(model, shown).scale, 50000.0);
    EXPECT_EQ(plotting::placePlan(model, fixed), plotting::storedPlacement(fixed));
    const plotting::PlanPlacement keyAt = plotting::placePlan(model, set, 0, key);
    for (const plotting::KeyPlanOutline& outline :
         plotting::keyPlanOutlines(set, 0, plotting::modelPlacer(model))) {
        for (const Point2& corner : outline.corners) {
            EXPECT_TRUE(key.rect.contains(plotting::planWorldToPaper(key, keyAt, corner)))
                << outline.viewportId;
        }
    }
}

TEST(SheetKeyPlan, AnAutomaticPlanIsFittedByTheSameRuleHeadlessAsPainted)
{
    // fitPlanPlacement (key_plan.hpp) places the other sheets' automatic
    // plans for a key plan; it must agree with the painter's own fit, or an
    // outline would not be where its sheet draws.
    Model model;
    addLine(model, Point2(100.0, 200.0), Point2(460.0, 330.0));
    addLine(model, Point2(120.0, 190.0), Point2(300.0, 420.0));
    SheetSource source;
    source.plan.model = &model;
    for (const double rotation : {0.0, 0.4, -1.1}) {
        plotting::Viewport viewport =
            plan("vp1", box(40.0, 60.0, 290.0, 230.0), 500.0, Point2(), rotation);
        viewport.autoScale = true;
        viewport.autoCentre = true;
        const auto painter = katana::qt::resolvePlanViewport(viewport, source);
        const std::vector<Point2> extent = {Point2(100.0, 190.0), Point2(460.0, 190.0),
                                            Point2(460.0, 420.0), Point2(100.0, 420.0)};
        const plotting::PlanPlacement headless = plotting::fitPlanPlacement(viewport, extent);
        EXPECT_EQ(painter.scale, headless.scale) << rotation;
        EXPECT_NEAR(painter.centre.x, headless.centre.x, 1e-6) << rotation;
        EXPECT_NEAR(painter.centre.y, headless.centre.y, 1e-6) << rotation;
    }
}
