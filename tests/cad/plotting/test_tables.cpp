// The drawing register and the revision table (include/katana/cad/plotting/
// tables.hpp): their rows, the table layout that fits them into a viewport,
// the register cover sheet and the one undoable step that adds it.
//
// Most layouts are measured with a fixed-width "font" - every character half
// its cap height wide, condensed to 0.9 of that (TableStyle::xFactor), so a
// character is 0.45 of the cap height - and the expected positions are worked
// by hand from it.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/tables.hpp"

using katana::cad::Document;
using katana::cad::PaperSize;
using katana::core::ErrorCode;
using namespace katana::cad::plotting;

namespace {

// Every character half its cap height wide, bold or not.
double monospace(std::string_view text, double capMm, bool /*bold*/)
{
    return 0.5 * capMm * static_cast<double>(text.size());
}

Box2 box(double x0, double y0, double x1, double y1) { return Box2(Point2(x0, y0), Point2(x1, y1)); }

// A two-column table - a centred number and a stretching title - of `count`
// rows "1".."count" titled "SHEET 1"...
TableSpec numbered(std::size_t count, std::string heading = "REGISTER")
{
    TableSpec spec;
    spec.heading = std::move(heading);
    spec.columns = {{"No.", HorizontalJustify::Centre}, {"TITLE", HorizontalJustify::Left, true}};
    for (std::size_t i = 1; i <= count; ++i) {
        spec.rows.push_back({std::to_string(i), "SHEET " + std::to_string(i)});
    }
    return spec;
}

const TableText* findText(const TableLayout& layout, std::string_view text)
{
    for (const TableText& item : layout.texts) {
        if (item.text == text) {
            return &item;
        }
    }
    return nullptr;
}

std::size_t countTexts(const TableLayout& layout, std::string_view text)
{
    return static_cast<std::size_t>(std::count_if(
        layout.texts.begin(), layout.texts.end(),
        [text](const TableText& item) { return item.text == text; }));
}

// Where a text lies on the paper when set with `measure`: from its baseline
// up by its cap height, as wide as it is drawn (condensed and squeezed).
Box2 extentOf(const TableText& item, const TextWidth& measure)
{
    const double width = measure(item.text, item.capMm, item.bold) * item.xFactor * item.squeeze;
    double left = item.anchor.x;
    if (item.horizontal == HorizontalJustify::Centre) {
        left -= width / 2.0;
    } else if (item.horizontal == HorizontalJustify::Right) {
        left -= width;
    }
    return box(left, item.anchor.y, left + width, item.anchor.y + item.capMm);
}

// Every text and rule of `layout` inside `rect`.
void expectInside(const TableLayout& layout, const Box2& rect, const TextWidth& measure)
{
    const Box2 slack = rect.inflated(1e-9);
    for (const TableText& item : layout.texts) {
        EXPECT_TRUE(slack.contains(extentOf(item, measure))) << item.text;
    }
    for (const TableRule& rule : layout.rules) {
        EXPECT_TRUE(slack.contains(rule.from) && slack.contains(rule.to));
    }
}

Viewport viewportOf(ViewportKind kind, double scale = 500.0, double exaggeration = 1.0)
{
    Viewport viewport;
    viewport.kind = kind;
    viewport.scale = scale;
    viewport.verticalExaggeration = exaggeration;
    return viewport;
}

Sheet sheetWith(std::string id, std::string name, Viewport viewport)
{
    Sheet sheet;
    sheet.id = std::move(id);
    sheet.name = std::move(name);
    viewport.id = "vp-" + sheet.id;
    sheet.viewports.push_back(std::move(viewport));
    return sheet;
}

// A key plan, a plan, a long section and a sheet of notes on A1, with two
// revisions issued, numbered "DS-7-01"...
SheetSet fourSheets()
{
    SheetSet set;
    set.numbering = "{set}-{n:02}";
    set.defaults.setNumber = "DS-7";
    set.revisions = {{"A", "01/02/26", "FIRST ISSUE", "JC"}, {"B", "03/02/26", "KERBS ADDED", "RE"}};
    set.sheets.push_back(sheetWith("s1", "KEY PLAN", viewportOf(ViewportKind::KeyPlan, 2000.0)));
    set.sheets.push_back(sheetWith("s2", "PLAN 1", viewportOf(ViewportKind::Plan, 500.0)));
    set.sheets.push_back(
        sheetWith("s3", "LONG SECTION", viewportOf(ViewportKind::LongSection, 500.0, 10.0)));
    set.sheets.push_back(sheetWith("s4", "NOTES", viewportOf(ViewportKind::Notes)));
    set.sheets.back().paper = PaperSize::A1;
    return set;
}

} // namespace

// ---- the rows ---------------------------------------------------------------------

TEST(SheetTables, TheRegisterListsEverySheetAsItsTitleBlockPrintsIt)
{
    const std::vector<RegisterRow> rows = drawingRegister(fourSheets());
    const std::vector<RegisterRow> expected{
        {"s1", "DS-7-01", "KEY PLAN", "1:2000", "A3", "B"},
        {"s2", "DS-7-02", "PLAN 1", "1:500", "A3", "B"},
        {"s3", "DS-7-03", "LONG SECTION", "H 1:500 V 1:50", "A3", "B"},
        {"s4", "DS-7-04", "NOTES", "N.T.S.", "A1", "B"},
    };
    EXPECT_EQ(rows, expected);
}

TEST(SheetTables, WhatIsTypedOnASheetWinsInTheRegisterAsOnTheSheet)
{
    SheetSet set = fourSheets();
    set.sheets[1].fields["revision"] = "C";      // this sheet was revised again
    set.sheets[2].fields["sheet_number"] = "LS-1";
    set.sheets[3].fields["sheet_name"] = "GENERAL NOTES";
    set.sheets[0].fields["scale"] = "1:2000 @ A3";
    const std::vector<RegisterRow> rows = drawingRegister(set);
    EXPECT_EQ(rows[1].revision, "C");
    EXPECT_EQ(rows[0].revision, "B");
    EXPECT_EQ(rows[2].number, "LS-1");
    EXPECT_EQ(rows[3].title, "GENERAL NOTES");
    EXPECT_EQ(rows[0].scale, "1:2000 @ A3");
}

TEST(SheetTables, AfterAReorderEachRowCarriesTheNumberItsSheetHasNow)
{
    SheetSet set = fourSheets();
    set.sheets[2].fields["sheet_number"] = "LS-1"; // typed: it goes where its sheet goes
    // The notes to the front, as moveSheet(3, 0) does.
    std::rotate(set.sheets.begin(), set.sheets.begin() + 3, set.sheets.end());
    const std::vector<RegisterRow> rows = drawingRegister(set);
    ASSERT_EQ(rows.size(), 4u);
    EXPECT_EQ(rows[0].sheetId, "s4");
    EXPECT_EQ(rows[0].number, "DS-7-01");
    EXPECT_EQ(rows[1].sheetId, "s1");
    EXPECT_EQ(rows[1].number, "DS-7-02");
    EXPECT_EQ(rows[3].sheetId, "s3");
    EXPECT_EQ(rows[3].number, "LS-1");
}

TEST(SheetTables, RevisionsAreListedNewestFirstAndCanBeLimitedToTheNewest)
{
    SheetSet set = fourSheets();
    set.revisions.push_back({"C", "07/02/26", "LEVELS CHECKED", "JC"});
    const auto codes = [&set](std::size_t limit) {
        std::string out;
        for (const Revision& revision : revisionRows(set, limit)) {
            out += revision.code;
        }
        return out;
    };
    EXPECT_EQ(codes(0), "CBA");
    EXPECT_EQ(codes(2), "CB");
    EXPECT_EQ(codes(1), "C");
    EXPECT_EQ(codes(10), "CBA");
    EXPECT_EQ(revisionRows(set, 1).front(), (Revision{"C", "07/02/26", "LEVELS CHECKED", "JC"}));
    EXPECT_TRUE(revisionRows(SheetSet{}, 0).empty());
}

// ---- the layout -------------------------------------------------------------------

TEST(SheetTables, AShortTableIsSetInTheLargestTextRuledHeavyOutsideAndLightInside)
{
    // 100 x 80 mm, three rows. At 2.5 mm text: the heading 1.25 x 2.5 =
    // 3.125 mm in a 6.25 mm band, the header row 5 mm, rows 5 mm - 26.25 mm
    // in all, so the largest text fits. The number column is its header
    // "No." (3 x 0.45 x 2.5 = 3.375) plus 1.25 each side: 5.875 mm; the
    // title column the other 94.125.
    TableSpec spec = numbered(3);
    const TableLayout layout = layoutTable(spec, box(0.0, 0.0, 100.0, 80.0), monospace);
    EXPECT_DOUBLE_EQ(layout.capMm, 2.5);
    EXPECT_DOUBLE_EQ(layout.rowHeightMm, 5.0);
    EXPECT_EQ(layout.blocks, 1u);
    EXPECT_EQ(layout.rowsShown, 3u);
    EXPECT_EQ(layout.rowsHidden, 0u);

    // The heading's band is 80 down to 73.75, the header's to 68.75, then
    // a row every 5 mm, in order.
    ASSERT_EQ(layout.rowBoxes.size(), 3u);
    EXPECT_EQ(layout.rowBoxes[0], box(0.0, 63.75, 100.0, 68.75));
    EXPECT_EQ(layout.rowBoxes[1], box(0.0, 58.75, 100.0, 63.75));
    EXPECT_EQ(layout.rowBoxes[2], box(0.0, 53.75, 100.0, 58.75));

    // Row 1's texts on the baseline that centres a 2.5 mm capital in the
    // 5 mm row (68.75 - 3.75 = 65): the number centred in its column, the
    // title 1.25 mm into its own.
    const TableText* number = findText(layout, "1");
    ASSERT_NE(number, nullptr);
    EXPECT_DOUBLE_EQ(number->anchor.x, 5.875 / 2.0);
    EXPECT_DOUBLE_EQ(number->anchor.y, 65.0);
    EXPECT_EQ(number->horizontal, HorizontalJustify::Centre);
    EXPECT_FALSE(number->bold);
    const TableText* title = findText(layout, "SHEET 1");
    ASSERT_NE(title, nullptr);
    EXPECT_DOUBLE_EQ(title->anchor.x, 5.875 + 1.25);
    EXPECT_DOUBLE_EQ(title->xFactor, 0.9);
    EXPECT_DOUBLE_EQ(title->squeeze, 1.0);
    // The heading in bold at 1.25 times the text; the headers in bold.
    const TableText* heading = findText(layout, "REGISTER");
    ASSERT_NE(heading, nullptr);
    EXPECT_TRUE(heading->bold);
    EXPECT_DOUBLE_EQ(heading->capMm, 3.125);
    EXPECT_DOUBLE_EQ(heading->anchor.y, 80.0 - (6.25 + 3.125) / 2.0);
    ASSERT_NE(findText(layout, "TITLE"), nullptr);
    EXPECT_TRUE(findText(layout, "TITLE")->bold);

    // Rules: the box, under the heading and under the header at 0.25; the
    // column rule and one under each row at 0.13. The column rule runs down
    // to the foot of the box, a ruled form with room for more.
    std::size_t heavy = 0;
    std::size_t light = 0;
    for (const TableRule& rule : layout.rules) {
        heavy += rule.weightMm == 0.25 ? 1 : 0;
        light += rule.weightMm == 0.13 ? 1 : 0;
    }
    EXPECT_EQ(heavy, 6u);
    EXPECT_EQ(light, 4u);
    EXPECT_TRUE(std::any_of(layout.rules.begin(), layout.rules.end(), [](const TableRule& rule) {
        return rule.weightMm == 0.13 && std::abs(rule.from.x - 5.875) < 1e-9 &&
               std::abs(rule.to.x - 5.875) < 1e-9 && rule.from.y == 73.75 && rule.to.y == 0.0;
    }));
    expectInside(layout, box(0.0, 0.0, 100.0, 80.0), monospace);
}

TEST(SheetTables, MoreRowsAreSetInSmallerTextUntilTheyFit)
{
    // 20 rows take 2.5 + 2 + 40 = 44.5 cap heights (heading, header, rows).
    // In 89.2 mm that is 2.004 mm at most: 2.0 mm text, the largest step.
    const TableLayout layout =
        layoutTable(numbered(20), box(0.0, 0.0, 100.0, 89.2), monospace);
    EXPECT_NEAR(layout.capMm, 2.0, 1e-12);
    EXPECT_EQ(layout.blocks, 1u);
    EXPECT_EQ(layout.rowsShown, 20u);
    EXPECT_NEAR(layout.rowHeightMm, 4.0, 1e-12);
    expectInside(layout, box(0.0, 0.0, 100.0, 89.2), monospace);
}

TEST(SheetTables, RowsThatDoNotFitTheSmallestTextContinueInASecondBlock)
{
    // 30 rows need 64.5 cap heights in one block - 116.1 mm at the 1.8 mm
    // minimum, more than 89.2. In two blocks of 15 they need 34.5: 2.5 mm
    // text fits (86.25 mm), so the rows continue side by side at full size.
    const Box2 rect = box(0.0, 0.0, 200.0, 89.2);
    const TableLayout layout = layoutTable(numbered(30), rect, monospace);
    EXPECT_DOUBLE_EQ(layout.capMm, 2.5);
    EXPECT_EQ(layout.blocks, 2u);
    EXPECT_EQ(layout.rowsShown, 30u);
    ASSERT_EQ(layout.rowBoxes.size(), 30u);
    // Rows 1 to 15 down the first block, 16 to 30 down the second, in order.
    EXPECT_DOUBLE_EQ(layout.rowBoxes[14].min.x, 0.0);
    EXPECT_DOUBLE_EQ(layout.rowBoxes[15].min.x, 100.0);
    EXPECT_DOUBLE_EQ(layout.rowBoxes[15].max.y, layout.rowBoxes[0].max.y);
    for (std::size_t i = 1; i < 15; ++i) {
        EXPECT_DOUBLE_EQ(layout.rowBoxes[i].max.y, layout.rowBoxes[i - 1].min.y);
    }
    // Each block has its own header; the heading is once, across both.
    EXPECT_EQ(countTexts(layout, "TITLE"), 2u);
    EXPECT_EQ(countTexts(layout, "REGISTER"), 1u);
    // The blocks are parted by a heavy rule from the header down.
    EXPECT_TRUE(std::any_of(layout.rules.begin(), layout.rules.end(), [](const TableRule& rule) {
        return rule.weightMm == 0.25 && std::abs(rule.from.x - 100.0) < 1e-9 &&
               std::abs(rule.from.y - (89.2 - 6.25)) < 1e-9 && rule.to.y == 0.0;
    }));
    expectInside(layout, rect, monospace);
}

TEST(SheetTables, WhatTwoBlocksOfTheSmallestTextCannotHoldIsCountedOnTheLastLine)
{
    // 100 rows in 200 x 89.2 at 1.8 mm: 89.2 - 1.8 x 4.5 = 81.1 mm under the
    // headers, 22 rows of 3.6 mm to a block, 44 lines in two. The last line
    // counts the rest: 43 rows shown, "+57 more" right-aligned in the last
    // line of the second block, grey.
    const Box2 rect = box(0.0, 0.0, 200.0, 89.2);
    const TableLayout layout = layoutTable(numbered(100), rect, monospace);
    EXPECT_DOUBLE_EQ(layout.capMm, 1.8);
    EXPECT_EQ(layout.blocks, 2u);
    EXPECT_EQ(layout.rowsShown, 43u);
    EXPECT_EQ(layout.rowsHidden, 57u);
    const TableText* more = findText(layout, "+57 more");
    ASSERT_NE(more, nullptr);
    EXPECT_TRUE(more->muted);
    EXPECT_EQ(more->horizontal, HorizontalJustify::Right);
    EXPECT_NEAR(more->anchor.x, 200.0 - 0.9, 1e-9);
    // Its line's top is 81.1 - 21 x 3.6 = 5.5; its baseline 2.7 under that.
    EXPECT_NEAR(more->anchor.y, 2.8, 1e-9);
    // The rows shown are the first 43, in order.
    EXPECT_NE(findText(layout, "SHEET 43"), nullptr);
    EXPECT_EQ(findText(layout, "SHEET 44"), nullptr);
    // The count is one cell across its block: the second block's column rule
    // (after its "100"-wide number column, 3 x 0.45 x 1.8 + 1.8 = 4.23 mm)
    // stops at the count's line, 5.5 mm up; the first block's runs to the
    // foot of the box.
    const auto columnRuleFoot = [&layout](double x) {
        for (const TableRule& rule : layout.rules) {
            if (rule.weightMm == 0.13 && std::abs(rule.from.x - x) < 1e-9 &&
                std::abs(rule.to.x - x) < 1e-9) {
                return rule.to.y;
            }
        }
        return -1.0;
    };
    EXPECT_NEAR(columnRuleFoot(4.23), 0.0, 1e-9);
    EXPECT_NEAR(columnRuleFoot(104.23), 5.5, 1e-9);
    expectInside(layout, rect, monospace);
}

TEST(SheetTables, ARevisionTableThatDoesNotFitCountsTheEarlierOnesAndStaysOneBlock)
{
    SheetSet set;
    for (int i = 0; i < 40; ++i) {
        set.revisions.push_back({std::to_string(i), "01/01/26", "ISSUE", "JC"});
    }
    Viewport table;
    table.kind = ViewportKind::Revisions;
    table.rect = box(0.0, 0.0, 120.0, 40.0);
    const TableLayout layout = layoutViewportTable(set, 0, table, monospace);
    EXPECT_EQ(layout.blocks, 1u);
    ASSERT_GT(layout.rowsHidden, 0u);
    // Newest first: the last issued is at the top.
    ASSERT_FALSE(layout.rowBoxes.empty());
    EXPECT_NE(findText(layout, "39"), nullptr);
    EXPECT_EQ(findText(layout, "0"), nullptr);
    EXPECT_NE(findText(layout, "+" + std::to_string(layout.rowsHidden) + " earlier"), nullptr);
    EXPECT_EQ(layout.rowsShown + layout.rowsHidden, 40u);
    expectInside(layout, table.rect, monospace);
}

TEST(SheetTables, TheHighlightedRowIsShadedAcrossItsBlock)
{
    TableSpec spec = numbered(30);
    spec.highlight = 17;
    const TableLayout layout = layoutTable(spec, box(0.0, 0.0, 200.0, 89.2), monospace);
    ASSERT_EQ(layout.shaded.size(), 1u);
    EXPECT_EQ(layout.shaded[0], layout.rowBoxes[17]);
    EXPECT_DOUBLE_EQ(layout.shaded[0].width(), 100.0);
}

TEST(SheetTables, ALongDescriptionWrapsAndMakesItsRowTaller)
{
    // REV | DATE | DESCRIPTION | BY in 60 mm. At 2.5 mm the fixed columns
    // are "REV" 5.875, "01/02/26" 11.5 and "BY" 4.75 wide, leaving the
    // description 37.875 mm: 35.375 for text, 31.4 characters of 1.125 mm.
    const std::string description =
        "KERB LINES ON THE NORTH SIDE MOVED AFTER THE SECOND SURVEY OF THE CORRIDOR";
    TableSpec spec;
    spec.columns = {{"REV", HorizontalJustify::Centre},
                    {"DATE", HorizontalJustify::Centre},
                    {"DESCRIPTION", HorizontalJustify::Left, true, true},
                    {"BY", HorizontalJustify::Centre}};
    spec.rows = {{"A", "01/02/26", description, "JC"}};
    const Box2 rect = box(0.0, 0.0, 60.0, 60.0);
    const TableLayout layout = layoutTable(spec, rect, monospace);
    EXPECT_DOUBLE_EQ(layout.capMm, 2.5);
    const double room = 37.875 - 2.5;

    std::vector<const TableText*> lines;
    for (const TableText& item : layout.texts) {
        if (!item.bold && std::abs(item.anchor.x - (17.375 + 1.25)) < 1e-9) {
            lines.push_back(&item);
        }
    }
    ASSERT_GE(lines.size(), 2u);
    std::string joined;
    for (std::size_t i = 0; i < lines.size(); ++i) {
        // Each line fits its column without a squeeze, and the next word
        // would not have: the words break as late as they can.
        const double width = monospace(lines[i]->text, 2.5, false) * 0.9;
        EXPECT_LE(width, room + 1e-9);
        EXPECT_DOUBLE_EQ(lines[i]->squeeze, 1.0);
        if (i + 1 < lines.size()) {
            const std::string next = lines[i + 1]->text.substr(0, lines[i + 1]->text.find(' '));
            EXPECT_GT(monospace(lines[i]->text + " " + next, 2.5, false) * 0.9, room);
            // Lines step down 1.5 cap heights.
            EXPECT_DOUBLE_EQ(lines[i]->anchor.y - lines[i + 1]->anchor.y, 3.75);
        }
        joined += (i > 0 ? " " : "") + lines[i]->text;
    }
    EXPECT_EQ(joined, description);
    // The row is as tall as its lines: 2 cap heights, and 1.5 more a line.
    ASSERT_EQ(layout.rowBoxes.size(), 1u);
    EXPECT_DOUBLE_EQ(layout.rowBoxes[0].height(),
                     2.5 * (2.0 + 1.5 * static_cast<double>(lines.size() - 1)));
    // The other cells stay on the row's first line.
    EXPECT_DOUBLE_EQ(findText(layout, "JC")->anchor.y, lines.front()->anchor.y);
    expectInside(layout, rect, monospace);
}

TEST(SheetTables, ALineBreakStartsALineOnlyInAWrappingCell)
{
    // A sheet name typed over two lines is one line in the register; a
    // description's line break starts a new line, and a "\r\n" leaves no
    // stray '\r' in a word.
    TableSpec spec;
    spec.heading = "SHEET\nLIST";
    spec.columns = {{"TITLE", HorizontalJustify::Left, true},
                    {"DESCRIPTION", HorizontalJustify::Left, false, true}};
    spec.rows = {{"PLAN\nNORTH", "FIRST\r\nSECOND"}};
    const Box2 rect = box(0.0, 0.0, 120.0, 60.0);
    const TableLayout layout = layoutTable(spec, rect, monospace);
    EXPECT_NE(findText(layout, "SHEET LIST"), nullptr);
    EXPECT_NE(findText(layout, "PLAN NORTH"), nullptr);
    const TableText* first = findText(layout, "FIRST");
    const TableText* second = findText(layout, "SECOND");
    ASSERT_NE(first, nullptr);
    ASSERT_NE(second, nullptr);
    EXPECT_DOUBLE_EQ(first->anchor.y - second->anchor.y, 1.5 * layout.capMm);
    for (const TableText& item : layout.texts) {
        EXPECT_EQ(item.text.find_first_of("\r\n"), std::string::npos) << item.text;
    }
    // The row is two lines high: 2 cap heights and 1.5 more.
    ASSERT_EQ(layout.rowBoxes.size(), 1u);
    EXPECT_DOUBLE_EQ(layout.rowBoxes[0].height(), 3.5 * layout.capMm);
}

TEST(SheetTables, ALongTitleIsSetSmallerRatherThanSqueezedHard)
{
    // A 40-character title in 40 mm. At 2.5 mm it would be pressed to 0.70
    // of its width; at 2.2 mm it keeps 0.82, above the 0.8 preferred, and
    // 2.3 mm would give 0.78. So 2.2 mm, though 2.5 mm fits the height.
    TableSpec spec = numbered(1);
    spec.rows[0][1] = std::string(40, 'X');
    const TableLayout layout = layoutTable(spec, box(0.0, 0.0, 40.0, 80.0), monospace);
    EXPECT_NEAR(layout.capMm, 2.2, 1e-12);
    const TableText* title = findText(layout, std::string(40, 'X'));
    ASSERT_NE(title, nullptr);
    EXPECT_GE(title->squeeze, 0.8);
    EXPECT_LT(title->squeeze, 1.0);
    expectInside(layout, box(0.0, 0.0, 40.0, 80.0), monospace);
}

TEST(SheetTables, ATableWithNoRowsIsItsHeadingAndHeaderAndATinyOneIsNothing)
{
    const TableLayout empty = layoutTable(numbered(0), box(0.0, 0.0, 100.0, 40.0), monospace);
    EXPECT_DOUBLE_EQ(empty.capMm, 2.5);
    EXPECT_EQ(empty.rowsShown, 0u);
    EXPECT_EQ(empty.rowsHidden, 0u);
    EXPECT_NE(findText(empty, "TITLE"), nullptr);

    // 20 x 5 mm cannot hold the heading and the header even at 1.8 mm.
    const TableLayout tiny = layoutTable(numbered(3), box(0.0, 0.0, 20.0, 5.0), monospace);
    EXPECT_EQ(tiny.rowsShown, 0u);
    EXPECT_EQ(tiny.rowsHidden, 3u);
    EXPECT_TRUE(tiny.texts.empty());
    EXPECT_TRUE(tiny.rules.empty());
    // No columns, or no rectangle: nothing, every row counted.
    EXPECT_EQ(layoutTable(TableSpec{}, box(0.0, 0.0, 100.0, 100.0)).rowsHidden, 0u);
    EXPECT_EQ(layoutTable(numbered(2), Box2{}).rowsHidden, 2u);
}

TEST(SheetTables, TheSameTableGivesTheSameLayout)
{
    TableSpec spec = numbered(57);
    spec.highlight = 3;
    const Box2 rect = box(10.0, 20.0, 190.0, 120.0);
    EXPECT_EQ(layoutTable(spec, rect), layoutTable(spec, rect));
    EXPECT_EQ(layoutTable(spec, rect, monospace), layoutTable(spec, rect, monospace));
}

TEST(SheetTables, TheEstimateUsesArialsAdvanceWidths)
{
    // Arial's cap height is 1467 of its 2048 units, so a 1 mm capital is set
    // on a 2048 / 1467 mm em. A digit is 0.556 em; "W" 0.944 and "I" 0.278.
    const double em = 2048.0 / 1467.0;
    EXPECT_NEAR(estimateTextWidth("0", 1.0, false), 0.556 * em, 1e-12);
    EXPECT_NEAR(estimateTextWidth("00", 2.0, false), 4.0 * 0.556 * em, 1e-12);
    EXPECT_NEAR(estimateTextWidth("WI", 1.0, false), (0.944 + 0.278) * em, 1e-12);
    // Bold is wider where Arial Bold is: b is 0.611 against 0.556.
    EXPECT_NEAR(estimateTextWidth("b", 1.0, true), 0.611 * em, 1e-12);
    EXPECT_GT(estimateTextWidth("SHEET No.", 2.5, true), estimateTextWidth("SHEET No.", 2.5, false));
    // A character outside ASCII counts once, whatever its bytes: an accented
    // capital as a digit, a wide ideograph as a full em.
    EXPECT_NEAR(estimateTextWidth("\xC3\x89", 1.0, false), 0.556 * em, 1e-12);
    EXPECT_NEAR(estimateTextWidth("\xE5\x8C\x97", 1.0, false), 1.0 * em, 1e-12);
    EXPECT_EQ(estimateTextWidth("", 2.5, false), 0.0);
}

// ---- the viewports --------------------------------------------------------------------

TEST(SheetTables, ARegisterViewportListsTheSetAndHighlightsTheSheetItIsOn)
{
    const SheetSet set = fourSheets();
    Viewport view;
    view.kind = ViewportKind::SheetIndex;
    view.rect = box(30.0, 40.0, 250.0, 280.0);
    const TableSpec spec = tableSpecFor(set, 2, view);
    EXPECT_EQ(spec.heading, "DRAWING REGISTER");
    ASSERT_EQ(spec.columns.size(), 5u);
    const std::vector<std::string> headers{"SHEET No.", "TITLE", "SCALE", "PAPER", "REV"};
    for (std::size_t j = 0; j < headers.size(); ++j) {
        EXPECT_EQ(spec.columns[j].header, headers[j]);
    }
    EXPECT_TRUE(spec.columns[1].stretch);
    EXPECT_EQ(spec.highlight, 2u);
    ASSERT_EQ(spec.rows.size(), 4u);
    EXPECT_EQ(spec.rows[2], (std::vector<std::string>{"DS-7-03", "LONG SECTION", "H 1:500 V 1:50",
                                                      "A3", "B"}));
    // A title of its own replaces the heading.
    view.title = "SHEET LIST";
    EXPECT_EQ(tableSpecFor(set, 2, view).heading, "SHEET LIST");

    // Laid out with the estimate: every row at the largest text, the
    // highlight on row 3, everything inside the viewport.
    const TableLayout layout = layoutViewportTable(set, 2, view);
    EXPECT_DOUBLE_EQ(layout.capMm, 2.5);
    EXPECT_EQ(layout.rowsShown, 4u);
    ASSERT_EQ(layout.shaded.size(), 1u);
    EXPECT_EQ(layout.shaded[0], layout.rowBoxes[2]);
    expectInside(layout, view.rect, estimateTextWidth);
}

TEST(SheetTables, ARevisionViewportShowsItsNewestRevisionsOnly)
{
    SheetSet set = fourSheets();
    set.revisions.push_back({"C", "07/02/26", "LEVELS CHECKED", "JC"});
    Viewport view;
    view.kind = ViewportKind::Revisions;
    view.rect = box(260.0, 40.0, 400.0, 120.0);
    view.revisionLimit = 2;
    const TableSpec spec = tableSpecFor(set, 0, view);
    EXPECT_EQ(spec.heading, "REVISIONS");
    ASSERT_EQ(spec.columns.size(), 4u);
    EXPECT_EQ(spec.columns[2].header, "DESCRIPTION");
    EXPECT_TRUE(spec.columns[2].wrap);
    EXPECT_EQ(spec.maxBlocks, 1u);
    EXPECT_FALSE(spec.highlight.has_value());
    ASSERT_EQ(spec.rows.size(), 2u);
    EXPECT_EQ(spec.rows[0], (std::vector<std::string>{"C", "07/02/26", "LEVELS CHECKED", "JC"}));
    EXPECT_EQ(spec.rows[1][0], "B");

    // Any other kind is no table.
    EXPECT_TRUE(tableSpecFor(set, 0, viewportOf(ViewportKind::Plan)).columns.empty());
    Viewport plan = viewportOf(ViewportKind::Plan);
    plan.rect = view.rect;
    EXPECT_TRUE(layoutViewportTable(set, 0, plan).texts.empty());
}

TEST(SheetTables, TheNewKindsHaveNamesTitlesRanksAndSizes)
{
    EXPECT_EQ(toString(ViewportKind::SheetIndex), "sheet_index");
    EXPECT_EQ(toString(ViewportKind::Revisions), "revisions");
    EXPECT_EQ(viewportKindFrom("sheet_index"), ViewportKind::SheetIndex);
    EXPECT_EQ(viewportKindFrom("revisions"), ViewportKind::Revisions);
    EXPECT_EQ(automaticTitle(viewportOf(ViewportKind::SheetIndex)), "DRAWING REGISTER");
    EXPECT_EQ(automaticTitle(viewportOf(ViewportKind::Revisions)), "REVISIONS");
    EXPECT_TRUE(isTileable(ViewportKind::SheetIndex));
    EXPECT_TRUE(isTileable(ViewportKind::Revisions));
    EXPECT_LT(tilingRank(ViewportKind::SheetIndex), tilingRank(ViewportKind::Revisions));
    EXPECT_EQ(minimumSize(ViewportKind::SheetIndex).width, 70.0);
    EXPECT_EQ(minimumSize(ViewportKind::Revisions).height, 20.0);
    // Tables are not drawn to scale: a register sheet is N.T.S.
    Sheet sheet;
    sheet.viewports = {viewportOf(ViewportKind::SheetIndex), viewportOf(ViewportKind::Revisions)};
    EXPECT_EQ(sheetScaleText(sheet), "N.T.S.");
}

// ---- the revision in the title block ------------------------------------------------

TEST(SheetTables, TheTitleBlockRevisionIsTheCurrentOneAndTheBuiltInFrameIsLeftAlone)
{
    // The built-in frame has no text for the revision, so it is drawn as it
    // was measured; the revision table is how its sheets show revisions.
    ASSERT_TRUE(builtInFrame().ok());
    EXPECT_TRUE(builtInFrame()->textsShowing("revision").empty());

    // A frame text that shows it prints the current revision: the latest
    // issued, or the one typed on the sheet.
    SheetSet set = fourSheets();
    FrameText rev;
    rev.content = "REV {revision}";
    rev.fields = {"revision"};
    const FieldContext context;
    EXPECT_EQ(expandTemplate(rev.content, resolveFields(set, 0, context)), "REV B");
    set.revisions.push_back({"C", "07/02/26", "LEVELS CHECKED", "JC"});
    EXPECT_EQ(expandTemplate(rev.content, resolveFields(set, 0, context)), "REV C");
    set.sheets[0].fields["revision"] = "B1";
    EXPECT_EQ(expandTemplate(rev.content, resolveFields(set, 0, context)), "REV B1");
}

TEST(SheetTables, AFrameThatNamesTheRevisionFieldPrintsTheCurrentRevision)
{
    // A frame written for Katana names its fields as a sheet does. Its
    // "REV {revision}" is a field text, filled like the measured frame's.
    const std::string json = R"({
        "sheet": {"width_mm": 420, "height_mm": 297,
                  "margins_mm": {"left": 23, "right": 10, "top": 10, "bottom": 35},
                  "viewport_tbf": {"x0": 23, "y0": 35, "x1": 410, "y1": 287},
                  "drawing_border_tbf": {"x0": 22, "y0": 34, "x1": 411, "y1": 289},
                  "title_block_tbf": {"x0": 22, "y0": 9, "x1": 411, "y1": 34},
                  "sheet_edge_tbf": {"x0": 0, "y0": 0, "x1": 420, "y1": 297}},
        "cells": [],
        "entities": [
            {"index": 0, "kind": "text", "role": "label", "anchor_tbf": [300, 20],
             "cap_height_mm": 2.5, "x_factor": 0.82, "justify": "bottom-left", "angle_deg": 0,
             "font_face": "Arial", "bold": false, "colour_hex": "#000000",
             "raw_text": "REV {revision}"},
            {"index": 1, "kind": "text", "role": "label", "anchor_tbf": [300, 15],
             "cap_height_mm": 2.5, "x_factor": 0.82, "justify": "bottom-left", "angle_deg": 0,
             "font_face": "Arial", "bold": false, "colour_hex": "#000000",
             "raw_text": "{Not a field} and {{braces}}"}]})";
    const auto frame = parseFrame(json);
    ASSERT_TRUE(frame.ok()) << frame.error().describe();
    ASSERT_EQ(frame->textsShowing("revision"), (std::vector<std::size_t>{0}));
    const FrameText& rev = frame->texts[0];
    EXPECT_EQ(rev.role, FrameRole::Field);
    EXPECT_EQ(rev.content, "REV {revision}");
    // Braces around anything that is not a field name are text.
    EXPECT_TRUE(frame->texts[1].fields.empty());
    EXPECT_EQ(frame->texts[1].content, "{Not a field} and {{braces}}");

    SheetSet set = fourSheets();
    EXPECT_EQ(expandTemplate(rev.content, resolveFields(set, 1, FieldContext{})), "REV B");
    set.revisions.push_back({"C", "07/02/26", "LEVELS CHECKED", "JC"});
    EXPECT_EQ(expandTemplate(rev.content, resolveFields(set, 1, FieldContext{})), "REV C");
}

// ---- storage -----------------------------------------------------------------------

TEST(SheetTables, TheNewKindsAndTheRevisionLimitSurviveAJsonRoundTrip)
{
    SheetSet set = fourSheets();
    Sheet cover;
    cover.id = "s9";
    Viewport index;
    index.id = "vp8";
    index.kind = ViewportKind::SheetIndex;
    index.rect = box(24.0, 36.0, 270.0, 286.0);
    Viewport revisions;
    revisions.id = "vp9";
    revisions.kind = ViewportKind::Revisions;
    revisions.rect = box(273.0, 36.0, 409.0, 286.0);
    revisions.revisionLimit = 3;
    cover.viewports = {index, revisions};
    set.sheets.insert(set.sheets.begin(), cover);

    const auto text = sheetSetToJson(set);
    ASSERT_TRUE(text.ok());
    EXPECT_NE(text->find("\"kind\":\"sheet_index\""), std::string::npos);
    EXPECT_NE(text->find("\"kind\":\"revisions\""), std::string::npos);
    EXPECT_NE(text->find("\"revision_limit\":3"), std::string::npos);
    // Only the one viewport that limits its revisions says so.
    EXPECT_EQ(text->find("\"revision_limit\""), text->rfind("\"revision_limit\""));
    const auto back = sheetSetFromJson(*text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(*back, set);
}

TEST(SheetTables, ARevisionLimitThatIsNotACountIsRefused)
{
    const auto withLimit = [](const std::string& limit) {
        return R"({"format": "katana-sheets", "version": 1, "sheets": [{"id": "s1", "viewports": [)"
               R"({"id": "vp1", "kind": "revisions", "revision_limit": )" +
               limit + "}]}]}";
    };
    const auto good = sheetSetFromJson(withLimit("4"));
    ASSERT_TRUE(good.ok());
    EXPECT_EQ(good->sheets[0].viewports[0].revisionLimit, 4u);
    for (const std::string bad : {"-1", "2.5", "\"3\"", "true"}) {
        const auto read = sheetSetFromJson(withLimit(bad));
        ASSERT_FALSE(read.ok()) << bad;
        EXPECT_EQ(read.error().code, ErrorCode::ParseFailure) << bad;
    }
}

// ---- the register cover sheet ------------------------------------------------------------

TEST(SheetTables, TheRegisterSheetHasTheRegisterBesideTheRevisions)
{
    const auto sheet = registerSheet(fourSheets());
    ASSERT_TRUE(sheet.ok()) << sheet.error().describe();
    EXPECT_EQ(sheet->name, "DRAWING REGISTER");
    EXPECT_EQ(sheet->id, "s1");
    EXPECT_EQ(sheet->paper, PaperSize::A3);
    EXPECT_FALSE(sheet->frameLegend);
    ASSERT_EQ(sheet->viewports.size(), 2u);
    const Viewport& index = sheet->viewports[0];
    const Viewport& revisions = sheet->viewports[1];
    EXPECT_EQ(index.kind, ViewportKind::SheetIndex);
    EXPECT_EQ(revisions.kind, ViewportKind::Revisions);
    EXPECT_EQ(index.id, "vp1");
    EXPECT_EQ(revisions.id, "vp2");
    // "Main and panel right": the register in the 66% cell, the revisions
    // in the panel beside it.
    const std::vector<Box2> cells = presetCells(TilingPreset::MainRight, tilingArea(*sheet));
    EXPECT_EQ(index.rect, cells[0]);
    EXPECT_EQ(revisions.rect, cells[1]);
    // The same set gives the same sheet.
    EXPECT_EQ(*registerSheet(fourSheets()), *sheet);

    // On the paper asked for.
    SheetTemplate a1;
    a1.paper = PaperSize::A1;
    const auto big = registerSheet(fourSheets(), a1);
    ASSERT_TRUE(big.ok());
    EXPECT_EQ(big->paper, PaperSize::A1);
    EXPECT_EQ(big->viewports[0].rect, presetCells(TilingPreset::MainRight, tilingArea(*big))[0]);
}

TEST(SheetTables, ASetWithARegisterIsNotGivenASecond)
{
    SheetSet set = fourSheets();
    set.sheets[2].viewports.push_back(viewportOf(ViewportKind::SheetIndex));
    const auto sheet = registerSheet(set);
    ASSERT_FALSE(sheet.ok());
    EXPECT_EQ(sheet.error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(sheet.error().context, "sheet DS-7-03 (LONG SECTION)");
}

TEST(SheetTables, ARegisterOfAManySheetSetFillsItsCellAndCountsTheRest)
{
    // 150 sheets and the cover: at the smallest text the A3 cell holds two
    // blocks of rows; the rest are counted, and the count is every row not
    // shown.
    SheetSet set;
    for (int i = 1; i <= 150; ++i) {
        set.sheets.push_back(sheetWith("s" + std::to_string(i + 1), "PLAN TILE " + std::to_string(i),
                                       viewportOf(ViewportKind::Plan, 500.0)));
    }
    auto cover = registerSheet(set);
    ASSERT_TRUE(cover.ok());
    set.sheets.insert(set.sheets.begin(), *cover);
    const Viewport& view = set.sheets[0].viewports[0];
    const TableLayout layout = layoutViewportTable(set, 0, view);
    EXPECT_DOUBLE_EQ(layout.capMm, 1.8);
    EXPECT_EQ(layout.blocks, 2u);
    EXPECT_GT(layout.rowsShown, 100u);
    EXPECT_EQ(layout.rowsShown + layout.rowsHidden, 151u);
    ASSERT_GT(layout.rowsHidden, 0u);
    EXPECT_NE(findText(layout, "+" + std::to_string(layout.rowsHidden) + " more"), nullptr);
    // The cover is the sheet the register is on: its own row is shaded.
    ASSERT_EQ(layout.shaded.size(), 1u);
    EXPECT_EQ(layout.shaded[0], layout.rowBoxes[0]);
    expectInside(layout, view.rect, estimateTextWidth);
}

TEST(SheetTables, AddingTheRegisterPutsItFirstInOneStepWithFreshIds)
{
    Document document;
    GridRequest grid;
    grid.area = Box2(Point2(0.0, 0.0), Point2(400.0, 200.0));
    grid.scale = 500.0;
    auto tiles = gridSheets(grid);
    ASSERT_TRUE(tiles.ok());
    ASSERT_TRUE(addSheets(document, std::move(*tiles)).ok());
    const SheetSet before = document.sheetSet();
    ASSERT_EQ(before.sheets.size(), 7u); // a key plan and six tiles, s1 to s7
    const std::size_t steps = document.history().undoCount();

    const auto id = addRegisterSheet(document);
    ASSERT_TRUE(id.ok()) << id.error().describe();
    const SheetSet& set = document.sheetSet();
    ASSERT_EQ(set.sheets.size(), 8u);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(set.sheets[0].name, "DRAWING REGISTER");
    EXPECT_EQ(set.sheets[0].id, *id);
    // Fresh ids: after every sheet and viewport already in the set.
    EXPECT_EQ(*id, "s8");
    EXPECT_EQ(set.sheets[0].viewports[0].id, "vp8");
    EXPECT_EQ(set.sheets[0].viewports[1].id, "vp9");
    // The others follow, numbered one on; the key plan's outline of the
    // first tile follows it by id to its new number.
    EXPECT_EQ(set.sheets[1].id, "s1");
    EXPECT_EQ(markLabel(set, set.sheets[1].viewports[0].marks[0]), "3");
    const std::vector<RegisterRow> rows = drawingRegister(set);
    EXPECT_EQ(rows[0].title, "DRAWING REGISTER");
    EXPECT_EQ(rows[0].scale, "N.T.S.");
    EXPECT_EQ(rows[1].number, "2");
    EXPECT_EQ(rows[1].title, "KEY PLAN");

    // A second is refused and changes nothing.
    const auto again = addRegisterSheet(document);
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(document.history().undoCount(), steps + 1);

    // One undo takes it away, exactly.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet(), before);
}

TEST(SheetTables, TheRegisterCanStartAnEmptySet)
{
    Document document;
    const auto id = addRegisterSheet(document);
    ASSERT_TRUE(id.ok());
    EXPECT_EQ(*id, "s1");
    ASSERT_EQ(document.sheetSet().sheets.size(), 1u);
    const std::vector<RegisterRow> rows = drawingRegister(document.sheetSet());
    ASSERT_EQ(rows.size(), 1u);
    EXPECT_EQ(rows[0].number, "1");
}
