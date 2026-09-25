// The drawing register and the revision table on paper (src/katana_qt/
// sheet_painter.cpp, src/katana_qt/plotting/sheet_tables): drawn where the
// table layout puts them, the current sheet's row shaded, and the register
// reporting the scale each sheet's title block prints. Painted into QImages
// at 4 px a millimetre and read back pixel by pixel.
//
// With KATANA_SHEET_PNG set to a directory, every sheet painted here is also
// written there as a PNG, to be looked at.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdlib>
#include <format>
#include <string>
#include <vector>

#include <QColor>
#include <QImage>
#include <QPainter>

#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/plotting/tables.hpp"
#include "katana/entity/model.hpp"
#include "plotting/sheet_tables.hpp"
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

namespace {

constexpr double kPpmm = 4.0;
const QRgb kWhite = qRgb(255, 255, 255);

Box2 box(double x0, double y0, double x1, double y1) { return Box2(Point2(x0, y0), Point2(x1, y1)); }

plotting::Viewport tableAt(plotting::ViewportKind kind, Box2 rect, std::string id = "vp1")
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    return viewport;
}

plotting::Sheet sheetNamed(std::string id, std::string name)
{
    plotting::Sheet sheet;
    sheet.id = std::move(id);
    sheet.name = std::move(name);
    return sheet;
}

QImage painted(const plotting::SheetSet& set, const SheetSource& source, SheetPaintStats* stats = nullptr,
               bool editing = false, const char* tag = "")
{
    const auto& sheet = set.sheets[0];
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    QImage image(static_cast<int>(std::lround(paper.widthMm * kPpmm)),
                 static_cast<int>(std::lround(paper.heightMm * kPpmm)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::gray);
    SheetPaintOptions options;
    options.pixelsPerMillimetre = kPpmm;
    options.construction = editing;
    options.slotHints = editing;
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

// Pixels in the paper box for which `test` holds.
template <typename Test> int count(const QImage& image, Box2 paper, Test test)
{
    const QPoint a = pixelOf(Point2(paper.min.x, paper.max.y));
    const QPoint b = pixelOf(Point2(paper.max.x, paper.min.y));
    int n = 0;
    for (int y = std::max(a.y(), 0); y <= std::min(b.y(), image.height() - 1); ++y) {
        for (int x = std::max(a.x(), 0); x <= std::min(b.x(), image.width() - 1); ++x) {
            n += test(image.pixelColor(x, y)) ? 1 : 0;
        }
    }
    return n;
}

int inkIn(const QImage& image, Box2 paper)
{
    return count(image, paper, [](const QColor& c) { return c.rgb() != kWhite; });
}

// Pixels of the light shade a highlighted row is filled with.
int shadeIn(const QImage& image, Box2 paper)
{
    return count(image, paper, [](const QColor& c) {
        return c.red() > 215 && c.red() < 245 && c.red() == c.green() && c.green() == c.blue();
    });
}

int pixelsIn(Box2 paper)
{
    return static_cast<int>(std::lround(paper.width() * kPpmm)) *
           static_cast<int>(std::lround(paper.height() * kPpmm));
}

// A cover with a register, and two plans after it.
plotting::SheetSet registerSet(Box2 rect)
{
    plotting::SheetSet set;
    set.sheets.push_back(sheetNamed("s1", "COVER"));
    set.sheets.back().viewports.push_back(tableAt(plotting::ViewportKind::SheetIndex, rect));
    for (const char* name : {"PLAN 1", "PLAN 2"}) {
        set.sheets.push_back(sheetNamed("s" + std::to_string(set.sheets.size() + 1), name));
        plotting::Viewport plan;
        plan.id = "vp" + std::to_string(set.sheets.size());
        plan.kind = plotting::ViewportKind::Plan;
        plan.rect = box(30.0, 40.0, 400.0, 280.0);
        plan.scale = 500.0;
        set.sheets.back().viewports.push_back(plan);
    }
    return set;
}

} // namespace

TEST(SheetTablePainter, TheRegisterIsRuledWhereItsLayoutSaysAndItsOwnSheetsRowIsShaded)
{
    const Box2 rect = box(30.0, 50.0, 250.0, 270.0);
    const plotting::SheetSet set = registerSet(rect);
    Model model;
    SheetSource source;
    source.plan.model = &model;
    SheetPaintStats stats;
    const QImage paper = painted(set, source, &stats);
    EXPECT_TRUE(stats.problems.empty());
    EXPECT_EQ(stats.viewportsDrawn, 1u);

    // Three rows at the largest text whatever the font: the rows' places
    // are the layout's, and so are the painter's.
    const plotting::TableLayout layout = plotting::layoutViewportTable(set, 0, set.sheets[0].viewports[0]);
    ASSERT_EQ(layout.rowsShown, 3u);
    ASSERT_DOUBLE_EQ(layout.capMm, 2.5);
    const Box2 first = layout.rowBoxes[0];
    const Box2 second = layout.rowBoxes[1];

    // The heavy rule under the header, across the table.
    EXPECT_GT(inkIn(paper, box(rect.min.x + 5.0, first.max.y - 0.2, rect.max.x - 5.0, first.max.y + 0.2)),
              static_cast<int>(0.9 * (rect.width() - 10.0) * kPpmm));
    // Row 1 is the cover, the sheet the register is drawn on: shaded where
    // it has no text (the middle of its title cell), row 2 not.
    const Box2 blankOfFirst(Point2(rect.min.x + 60.0, first.min.y + 0.8),
                            Point2(rect.min.x + 120.0, first.max.y - 0.8));
    EXPECT_GT(shadeIn(paper, blankOfFirst), pixelsIn(blankOfFirst) * 9 / 10);
    const Box2 blankOfSecond(Point2(rect.min.x + 60.0, second.min.y + 0.8),
                             Point2(rect.min.x + 120.0, second.max.y - 0.8));
    EXPECT_EQ(inkIn(paper, blankOfSecond), 0);
    // The rows' texts: ink at the left of each row.
    EXPECT_GT(inkIn(paper, box(rect.min.x + 1.0, second.min.y + 0.5, rect.min.x + 60.0, second.max.y - 0.5)),
              20);
    // A table's heading is its title: nothing written in the bottom-left
    // corner, where the other views put theirs.
    EXPECT_EQ(inkIn(paper, box(rect.min.x + 1.0, rect.min.y + 1.0, rect.min.x + 12.0, rect.min.y + 7.0)), 0);
    // The heading in the band at the top.
    EXPECT_GT(inkIn(paper, box(rect.min.x + 1.0, rect.max.y - 6.0, rect.min.x + 60.0, rect.max.y - 1.0)),
              50);
}

TEST(SheetTablePainter, ARevisionTableListsItsRowsAndAnEmptyOneExplainsItselfOnlyOnScreen)
{
    const Box2 rect = box(260.0, 150.0, 400.0, 270.0);
    plotting::SheetSet set;
    set.sheets.push_back(sheetNamed("s1", "COVER"));
    set.sheets.back().viewports.push_back(tableAt(plotting::ViewportKind::Revisions, rect));
    Model model;
    SheetSource source;
    source.plan.model = &model;

    // No revisions: on paper, nothing in the middle; in the editor, a hint.
    const Point2 middle = rect.center();
    const Box2 around(Point2(middle.x - 20.0, middle.y - 1.0), Point2(middle.x + 20.0, middle.y + 1.0));
    SheetPaintStats stats;
    EXPECT_EQ(inkIn(painted(set, source, &stats), around), 0);
    EXPECT_TRUE(stats.problems.empty());
    EXPECT_GT(inkIn(painted(set, source, nullptr, true, "_editing"), around), 20);

    // Two revisions: the newest in the first row, both drawn.
    set.revisions = {{"A", "01/02/26", "FIRST ISSUE", "JC"},
                     {"B", "03/02/26", "KERBS ADDED ALONG THE NORTH SIDE", "RE"}};
    const QImage paper = painted(set, source, &stats, false, "_two");
    EXPECT_TRUE(stats.problems.empty());
    const plotting::TableLayout layout =
        plotting::layoutViewportTable(set, 0, set.sheets[0].viewports[0]);
    ASSERT_EQ(layout.rowsShown, 2u);
    for (const Box2& row : layout.rowBoxes) {
        EXPECT_GT(inkIn(paper, box(row.min.x + 0.5, row.min.y + 0.5, row.max.x - 0.5, row.max.y - 0.5)), 40);
    }
    // No row shaded - a revision table has no current row: the blank end of
    // the newest row's description cell, past its text and short of the BY
    // column, is paper white.
    const Box2 newest = layout.rowBoxes[0];
    const Box2 blank(Point2(newest.max.x - 40.0, newest.min.y + 0.8),
                     Point2(newest.max.x - 12.0, newest.max.y - 0.8));
    EXPECT_EQ(inkIn(paper, blank), 0);
}

TEST(SheetTablePainter, ARegisterTooSmallForItsRowsSaysSo)
{
    // 60 sheets in a 70 x 40 mm register: most are counted, not listed, and
    // the painter says so for the editor and the plot log.
    plotting::SheetSet set = registerSet(box(30.0, 50.0, 100.0, 90.0));
    for (int i = 3; i < 60; ++i) {
        set.sheets.push_back(sheetNamed("s" + std::to_string(i + 1), "PLAN " + std::to_string(i)));
    }
    Model model;
    SheetSource source;
    source.plan.model = &model;
    SheetPaintStats stats;
    painted(set, source, &stats);
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_TRUE(stats.problems[0].starts_with("vp1: ")) << stats.problems[0];
    EXPECT_TRUE(stats.problems[0].ends_with(" of 60 rows do not fit; make the view larger"))
        << stats.problems[0];
    // The view is still drawn: the rows that fit, and the count.
    EXPECT_EQ(stats.viewportsDrawn, 1u);
}

TEST(SheetTablePainter, TheRegisterReportsTheScaleAnAutomaticPlanIsDrawnAt)
{
    // A 1000 m line in a 370 x 240 mm plan left on automatic: drawn at the
    // first sheet scale that holds it with 4% to spare, not at the 1:500 it
    // was stored with - and the register says what the title block says.
    Model model;
    Entity line;
    line.geometry = Segment2{Point2(0.0, 0.0), Point2(1000.0, 0.0)};
    line.layer = "0";
    ASSERT_TRUE(model.entities.add(std::move(line)).ok());
    plotting::SheetSet set = registerSet(box(30.0, 50.0, 250.0, 270.0));
    plotting::Viewport& plan = set.sheets[1].viewports[0];
    plan.autoScale = true;
    plan.autoCentre = true;
    SheetSource source;
    source.plan.model = &model;

    const katana::qt::ResolvedViewport resolved = katana::qt::resolvePlanViewport(plan, source);
    ASSERT_NE(resolved.scale, 500.0);
    const auto drawn = katana::qt::drawnRegister(set, source);
    ASSERT_EQ(drawn.size(), 3u);
    EXPECT_EQ(drawn[1].scale, std::format("1:{}", std::llround(resolved.scale)));
    // The model alone knows only the scale last chosen.
    EXPECT_EQ(plotting::drawingRegister(set)[1].scale, "1:500");
    // A fixed scale is left as it is.
    EXPECT_EQ(drawn[2].scale, "1:500");
    const plotting::SheetSet resolvedSet = katana::qt::resolvedSheetSet(set, source);
    EXPECT_FALSE(resolvedSet.sheets[1].viewports[0].autoScale);
    EXPECT_EQ(resolvedSet.sheets[2], set.sheets[2]);
}

// A sheet whose only scaled view is an automatic key plan: the register says
// the scale the key plan is drawn at, fitted to the sheets' outlines as the
// painter fits it, not to the drawing - here a stray line 20 km away that
// would make it a far smaller scale.
TEST(SheetTablePainter, TheRegisterReportsAKeyPlansScaleAsThePainterFitsIt)
{
    Model model;
    for (const Segment2& segment : {Segment2{Point2(0.0, 0.0), Point2(100.0, 0.0)},
                                    Segment2{Point2(20000.0, 20000.0), Point2(20010.0, 20000.0)}}) {
        Entity line;
        line.geometry = segment;
        line.layer = "0";
        ASSERT_TRUE(model.entities.add(std::move(line)).ok());
    }
    plotting::SheetSet set = registerSet(box(30.0, 50.0, 250.0, 270.0));
    set.sheets.push_back(sheetNamed("s4", "KEY"));
    plotting::Viewport key;
    key.id = "vp9";
    key.kind = plotting::ViewportKind::KeyPlan;
    key.rect = box(30.0, 40.0, 130.0, 110.0);
    key.autoScale = true;
    key.autoCentre = true;
    set.sheets.back().viewports.push_back(key);
    SheetSource source;
    source.plan.model = &model;

    const double painted = katana::qt::resolvePlanViewport(key, source, set, 3).scale;
    const double ofTheDrawing = katana::qt::resolvePlanViewport(key, source).scale;
    ASSERT_LT(painted, ofTheDrawing) << "the outlines are nearer than the stray line";
    const auto drawn = katana::qt::drawnRegister(set, source);
    ASSERT_EQ(drawn.size(), 4u);
    EXPECT_EQ(drawn[3].scale, std::format("1:{}", std::llround(painted)));
}

TEST(SheetTablePainter, TheRegisterCoverPlotsBothTablesAndNoUtilityLegend)
{
    // The cover registerSheet makes, put first in a set of six tiles and a
    // key plan with two revisions issued: the register in the big cell, the
    // revisions beside it, the cover's own row shaded, no utility legend.
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::GridRequest grid;
    grid.area = box(0.0, 0.0, 400.0, 200.0);
    grid.scale = 500.0;
    auto tiles = plotting::gridSheets(grid);
    ASSERT_TRUE(tiles.ok());
    plotting::SheetSet set;
    set.revisions = {{"A", "01/02/26", "FIRST ISSUE", "JC"},
                     {"B", "03/02/26", "KERBS ADDED ALONG THE NORTH SIDE", "RE"}};
    plotting::prepareForAppend(set, *tiles);
    set.sheets = std::move(*tiles);
    auto cover = plotting::registerSheet(set);
    ASSERT_TRUE(cover.ok()) << cover.error().describe();
    std::vector<plotting::Sheet> one{std::move(*cover)};
    plotting::prepareForAppend(set, one);
    set.sheets.insert(set.sheets.begin(), std::move(one.front()));
    ASSERT_EQ(set.sheets.size(), 8u);

    SheetPaintStats stats;
    const QImage paper = painted(set, source, &stats);
    EXPECT_TRUE(stats.problems.empty()) << stats.problems.front();
    EXPECT_EQ(stats.viewportsDrawn, 2u);

    const plotting::Viewport& index = set.sheets[0].viewports[0];
    const plotting::Viewport& revisions = set.sheets[0].viewports[1];
    ASSERT_EQ(index.kind, plotting::ViewportKind::SheetIndex);
    ASSERT_EQ(revisions.kind, plotting::ViewportKind::Revisions);
    EXPECT_LT(index.rect.max.x, revisions.rect.min.x);

    // Every sheet listed and both revisions, at the largest text.
    const plotting::TableLayout rows = plotting::layoutViewportTable(set, 0, index);
    EXPECT_EQ(rows.rowsShown, 8u);
    EXPECT_EQ(rows.rowsHidden, 0u);
    const plotting::TableLayout issued = plotting::layoutViewportTable(set, 0, revisions);
    EXPECT_EQ(issued.rowsShown, 2u);
    for (const Box2& row : rows.rowBoxes) {
        EXPECT_GT(inkIn(paper, box(row.min.x + 0.5, row.min.y + 0.5, row.min.x + 60.0,
                                   row.max.y - 0.5)),
                  20);
    }
    for (const Box2& row : issued.rowBoxes) {
        EXPECT_GT(inkIn(paper, box(row.min.x + 0.5, row.min.y + 0.5, row.max.x - 0.5,
                                   row.max.y - 0.5)),
                  40);
    }
    // The cover's row is shaded; the key plan's after it is not.
    const Box2 own = rows.rowBoxes[0];
    const Box2 blank(Point2(own.min.x + 80.0, own.min.y + 0.8),
                     Point2(own.min.x + 140.0, own.max.y - 0.8));
    EXPECT_GT(shadeIn(paper, blank), pixelsIn(blank) * 9 / 10);
    EXPECT_EQ(shadeIn(paper, Box2(Point2(blank.min.x, rows.rowBoxes[1].min.y + 0.8),
                                  Point2(blank.max.x, rows.rowBoxes[1].max.y - 0.8))),
              0);
    // No utility legend: its block, bottom left of the title block, is bare.
    const auto frame = katana::qt::sheetFrame(set.sheets[0]);
    ASSERT_TRUE(frame.has_value());
    const plotting::FrameCell* legend = frame->cell("legend");
    ASSERT_NE(legend, nullptr);
    const Box2 inside = legend->rect.inflated(-2.0);
    EXPECT_EQ(inkIn(paper, inside), 0);
}

TEST(SheetTablePainter, ARealisticCoverIsLegible)
{
    // Eleven sheets of mixed scales and papers, four revisions, one with a
    // long description: every row and revision listed, nothing below 1.8 mm.
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::SheetSet set;
    set.numbering = "{set}-{n:02}";
    set.defaults.setNumber = "C-104";
    set.revisions = {{"A", "12/01/26", "PRELIMINARY ISSUE FOR COMMENT", "JC"},
                     {"B", "03/02/26", "KERBS ADDED ALONG THE NORTH SIDE", "RE"},
                     {"C", "17/03/26",
                      "DRAINAGE REDESIGNED BETWEEN CH 120 AND CH 480 FOLLOWING THE GEOTECHNICAL "
                      "REPORT; PIT LIDS, INVERT LEVELS AND PIPE CLASSES REVISED THROUGHOUT; "
                      "SEE SHEETS 04 TO 07",
                      "JC"},
                     {"D", "02/04/26", "ISSUED FOR CONSTRUCTION", "MK"}};
    const char* names[] = {"GENERAL ARRANGEMENT AND KEY PLAN", "SITE PLAN", "PLAN SHEET 1",
                           "PLAN SHEET 2", "PLAN SHEET 3", "LONGITUDINAL SECTION MC01",
                           "CROSS SECTIONS CH 0 TO CH 240", "CROSS SECTIONS CH 260 TO CH 500",
                           "DRAINAGE DETAILS", "PAVEMENT DETAILS AND TYPICAL SECTIONS",
                           "GENERAL NOTES"};
    const double scales[] = {2000, 1000, 500, 500, 500, 500, 200, 200, 50, 20, 0};
    const katana::cad::PaperSize papers[] = {
        katana::cad::PaperSize::A1, katana::cad::PaperSize::A1, katana::cad::PaperSize::A1,
        katana::cad::PaperSize::A1, katana::cad::PaperSize::A1, katana::cad::PaperSize::A3,
        katana::cad::PaperSize::A3, katana::cad::PaperSize::A3, katana::cad::PaperSize::A3,
        katana::cad::PaperSize::A3, katana::cad::PaperSize::A4};
    for (std::size_t i = 0; i < std::size(names); ++i) {
        plotting::Sheet sheet = sheetNamed("s" + std::to_string(i + 1), names[i]);
        sheet.paper = papers[i];
        plotting::Viewport view;
        view.id = "vp" + std::to_string(i + 1);
        view.kind = scales[i] > 0 ? plotting::ViewportKind::Plan : plotting::ViewportKind::Notes;
        view.scale = scales[i] > 0 ? scales[i] : 500.0;
        view.rect = box(30.0, 40.0, 200.0, 200.0);
        sheet.viewports.push_back(view);
        set.sheets.push_back(std::move(sheet));
    }
    set.sheets[9].fields["revision"] = "B";
    auto cover = plotting::registerSheet(set);
    ASSERT_TRUE(cover.ok());
    std::vector<plotting::Sheet> one{std::move(*cover)};
    plotting::prepareForAppend(set, one);
    set.sheets.insert(set.sheets.begin(), std::move(one.front()));

    SheetPaintStats stats;
    painted(set, source, &stats);
    EXPECT_TRUE(stats.problems.empty()) << stats.problems.front();
    const auto rows = plotting::layoutViewportTable(set, 0, set.sheets[0].viewports[0]);
    const auto issued = plotting::layoutViewportTable(set, 0, set.sheets[0].viewports[1]);
    EXPECT_EQ(rows.rowsHidden, 0u);
    EXPECT_EQ(issued.rowsHidden, 0u);
    for (const auto* layout : {&rows, &issued}) {
        EXPECT_GE(layout->capMm, 1.8);
        for (const auto& item : layout->texts) {
            EXPECT_GE(item.squeeze, 0.8) << item.text;
        }
    }
}

TEST(SheetTablePainter, ALongRegisterContinuesInASecondBlockAndLongRevisionsWrap)
{
    // A cover over 150 sheets and four revisions, one described at length:
    // the register fills two blocks of the smallest text and counts the
    // rest; the long description wraps inside its cell and its row is taller.
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::SheetSet set;
    for (int i = 1; i <= 150; ++i) {
        set.sheets.push_back(sheetNamed("s" + std::to_string(i + 1),
                                        "PLAN AND LONGITUDINAL SECTION CH " + std::to_string(i * 100)));
    }
    set.revisions = {{"A", "01/02/26", "FIRST ISSUE", "JC"},
                     {"B", "03/02/26", "KERBS ADDED", "RE"},
                     {"C", "05/02/26",
                      "LEVELS CHECKED AGAINST THE NEW CONTROL, DRAINAGE PITS RENUMBERED AND THE "
                      "EASTERN BOUNDARY REDRAWN FROM THE LATEST TITLE SEARCH",
                      "JC"},
                     {"D", "09/02/26", "ISSUED FOR CONSTRUCTION", "RE"}};
    auto cover = plotting::registerSheet(set);
    ASSERT_TRUE(cover.ok());
    set.sheets.insert(set.sheets.begin(), std::move(*cover));

    SheetPaintStats stats;
    const QImage paper = painted(set, source, &stats);
    const plotting::Viewport& index = set.sheets[0].viewports[0];
    const plotting::Viewport& revisions = set.sheets[0].viewports[1];
    const plotting::TableLayout rows = plotting::layoutViewportTable(set, 0, index);
    EXPECT_EQ(rows.blocks, 2u);
    EXPECT_DOUBLE_EQ(rows.capMm, 1.8);
    ASSERT_GT(rows.rowsHidden, 0u);
    ASSERT_EQ(stats.problems.size(), 1u);
    EXPECT_TRUE(stats.problems[0].starts_with(index.id + ": ")) << stats.problems[0];
    // The last row shown is in the second block, and it is drawn.
    const Box2 last = rows.rowBoxes.back();
    EXPECT_NEAR(last.min.x, index.rect.center().x, 1e-9);
    EXPECT_GT(inkIn(paper, box(last.min.x + 0.3, last.min.y + 0.3, last.min.x + 40.0, last.max.y - 0.3)), 20);

    const plotting::TableLayout issued = plotting::layoutViewportTable(set, 0, revisions);
    ASSERT_EQ(issued.rowsShown, 4u);
    EXPECT_EQ(issued.rowsHidden, 0u);
    // Newest first: D, then the long C, whose row holds several lines.
    const Box2 longRow = issued.rowBoxes[1];
    EXPECT_GT(longRow.height(), 2.0 * issued.rowHeightMm);
    EXPECT_NEAR(issued.rowBoxes[0].height(), issued.rowHeightMm, 1e-9);
    // Ink on the wrapped lines, low in the tall row as well as at its top.
    EXPECT_GT(inkIn(paper, box(longRow.min.x + 20.0, longRow.min.y + 0.3, longRow.max.x - 10.0,
                               longRow.min.y + issued.rowHeightMm)),
              20);
}
