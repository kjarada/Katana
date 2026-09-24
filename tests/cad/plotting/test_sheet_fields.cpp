// Title-block fields and their automatic values (sheet_set.hpp).
//
// The expected texts are what a drafter writes on the sheet, worked out from
// the rule each field states; none is copied from the code's output.

#include <gtest/gtest.h>

#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/sheet_set.hpp"

using katana::cad::PaperSize;
using namespace katana::cad::plotting;

namespace {

Viewport viewportOf(ViewportKind kind, double scale, double exaggeration = 1.0)
{
    Viewport viewport;
    viewport.kind = kind;
    viewport.scale = scale;
    viewport.verticalExaggeration = exaggeration;
    return viewport;
}

SheetSet threeSheets()
{
    SheetSet set;
    for (const char* name : {"KEY PLAN", "PLAN 1", "PLAN 2"}) {
        Sheet sheet;
        sheet.name = name;
        sheet.viewports.push_back(viewportOf(ViewportKind::Plan, 500.0));
        set.sheets.push_back(sheet);
    }
    return set;
}

FieldContext context()
{
    FieldContext c;
    c.projectName = "Main Road Upgrade";
    c.projectDescription = "Detail survey";
    c.coordinateSystem = "EPSG:7856";
    c.fileName = "main_road.katana";
    c.plotDate = "24/09/26";
    return c;
}

} // namespace

TEST(SheetFields, SheetNumberAndCountAreBothAutomatic)
{
    const SheetSet set = threeSheets();
    const auto second = resolveFields(set, 1, context());
    EXPECT_EQ(second.at("sheet_number"), "2");
    // The count follows the set: the owner's app left it as typed.
    EXPECT_EQ(second.at("sheet_count"), "3");
    EXPECT_EQ(second.at("sheet_name"), "PLAN 1");
    EXPECT_EQ(second.at("paper"), "A3");
    EXPECT_TRUE(resolveFields(set, 3, context()).empty()); // no fourth sheet
}

TEST(SheetFields, ANumberingPatternPadsAndPrefixes)
{
    SheetSet set = threeSheets();
    set.numbering = "{set}-{n:02} of {N}";
    set.defaults.setNumber = "DS-7";
    EXPECT_EQ(resolveFields(set, 2, context()).at("sheet_number"), "DS-7-03 of 3");
    EXPECT_EQ(formatSheetNumber("{n:03}", 7, 12, ""), "007");
    EXPECT_EQ(formatSheetNumber("C{n}", 12, 12, ""), "C12");
    // An unknown token is kept as written rather than silently dropped.
    EXPECT_EQ(formatSheetNumber("{x}{n}", 1, 1, ""), "{x}1");
}

TEST(SheetFields, EachSignOffKeepsItsOwnDateAndAMissingOneIsThePlotDate)
{
    SheetSet set = threeSheets();
    set.defaults.surveyor = {"J. CITIZEN", "01/02/26"};
    set.defaults.reviewer = {"A. N. OTHER", ""};
    const auto fields = resolveFields(set, 0, context());
    EXPECT_EQ(fields.at("surveyor_name"), "J. CITIZEN");
    EXPECT_EQ(fields.at("surveyor_date"), "01/02/26");
    EXPECT_EQ(fields.at("reviewer_name"), "A. N. OTHER");
    EXPECT_EQ(fields.at("reviewer_date"), "24/09/26");
    EXPECT_EQ(fields.at("plot_date"), "24/09/26");
}

TEST(SheetFields, ProjectValuesComeFromTheProjectUnlessTheSetSaysOtherwise)
{
    SheetSet set = threeSheets();
    auto fields = resolveFields(set, 0, context());
    EXPECT_EQ(fields.at("project_line_1"), "Main Road Upgrade");
    EXPECT_EQ(fields.at("project_line_2"), "Detail survey");
    EXPECT_EQ(fields.at("project_line_3"), "");
    EXPECT_EQ(fields.at("coordinate_system"), "EPSG:7856");
    EXPECT_EQ(fields.at("file_name"), "main_road.katana");
    set.defaults.projectLines = {"Stage 2", "", "Lot 7"};
    set.defaults.coordinateSystem = "GRID 56";
    fields = resolveFields(set, 0, context());
    EXPECT_EQ(fields.at("project_line_1"), "Stage 2");
    EXPECT_EQ(fields.at("project_line_2"), "Detail survey"); // empty falls back
    EXPECT_EQ(fields.at("project_line_3"), "Lot 7");
    EXPECT_EQ(fields.at("coordinate_system"), "GRID 56");
}

TEST(SheetFields, AValueTypedOnTheSheetAlwaysWins)
{
    SheetSet set = threeSheets();
    set.sheets[1].fields["sheet_count"] = "12";
    set.sheets[1].fields["sheet_number"] = "C-2";
    set.sheets[1].fields["scale"] = "1:250 @ A1";
    const auto fields = resolveFields(set, 1, context());
    EXPECT_EQ(fields.at("sheet_count"), "12");
    EXPECT_EQ(fields.at("sheet_number"), "C-2");
    EXPECT_EQ(fields.at("scale"), "1:250 @ A1");
    // The other sheets are untouched.
    EXPECT_EQ(resolveFields(set, 2, context()).at("sheet_count"), "3");
}

TEST(SheetFields, TheScaleIsTheMainViewportsAndASectionStatesBoth)
{
    Sheet plan;
    plan.viewports.push_back(viewportOf(ViewportKind::Plan, 500.0));
    EXPECT_EQ(sheetScaleText(plan), "1:500");
    // A long section at 1:500 exaggerated 10 times: V 1 : 500 / 10 = 50.
    Sheet section;
    section.viewports.push_back(viewportOf(ViewportKind::LongSection, 500.0, 10.0));
    EXPECT_EQ(sheetScaleText(section), "H 1:500 V 1:50");
    // Equal scales still say both.
    section.viewports.front().verticalExaggeration = 1.0;
    EXPECT_EQ(sheetScaleText(section), "H 1:500 V 1:500");
    // 1:250 at 8 times is 1:31.25, which a scale rule reads as 1:31.
    section.viewports.front() = viewportOf(ViewportKind::CrossSections, 250.0, 8.0);
    EXPECT_EQ(sheetScaleText(section), "H 1:250 V 1:31");
    // A plan and a section at different scales: AS SHOWN.
    Sheet both = plan;
    both.viewports.push_back(viewportOf(ViewportKind::LongSection, 500.0, 10.0));
    EXPECT_EQ(sheetScaleText(both), "AS SHOWN");
    // Two plans at one scale, and a 3D view and legend (not to scale): 1:500.
    Sheet two = plan;
    two.viewports.push_back(viewportOf(ViewportKind::Plan, 500.0));
    two.viewports.push_back(viewportOf(ViewportKind::Model3D, 123.0));
    two.viewports.push_back(viewportOf(ViewportKind::Legend, 1.0));
    EXPECT_EQ(sheetScaleText(two), "1:500");
    // Nothing drawn to scale.
    Sheet legend;
    legend.viewports.push_back(viewportOf(ViewportKind::Legend, 1.0));
    EXPECT_EQ(sheetScaleText(legend), "N.T.S.");
}

TEST(SheetFields, AutomaticTitlesNameTheViewAndItsScale)
{
    EXPECT_EQ(automaticTitle(viewportOf(ViewportKind::Plan, 500.0)), "PLAN 1:500");
    EXPECT_EQ(automaticTitle(viewportOf(ViewportKind::LongSection, 1000.0, 10.0)),
              "LONG SECTION H 1:1000 V 1:100");
    Viewport cross = viewportOf(ViewportKind::CrossSections, 200.0, 2.0);
    cross.source.stations = {120.0};
    EXPECT_EQ(automaticTitle(cross), "CROSS SECTION CH 120.000");
    EXPECT_EQ(automaticTitle(viewportOf(ViewportKind::KeyPlan, 5000.0)), "KEY PLAN");
}

TEST(SheetFields, FrameTextsFillFromTheResolvedFields)
{
    const auto& frame = builtInFrame();
    ASSERT_TRUE(frame.ok());
    SheetSet set = threeSheets();
    set.defaults.setNumber = "DS-7";
    const auto fields = resolveFields(set, 0, context());
    const auto project = frame->textsShowing("set_number");
    ASSERT_EQ(project.size(), 1u);
    EXPECT_EQ(expandTemplate(frame->texts[project.front()].content, fields),
              "Main Road Upgrade\nDetail survey\n\n\nDrawing Set Number: DS-7");
    const auto stamp = frame->textsShowing("paper");
    ASSERT_EQ(stamp.size(), 1u);
    EXPECT_EQ(expandTemplate(frame->texts[stamp.front()].content, fields),
              "A3 Border version: 2.0    24/09/26");
}

TEST(SheetFields, ATemplateLeavesUnknownFieldsEmptyAndKeepsDoubledBraces)
{
    const std::map<std::string, std::string, std::less<>> fields{{"a", "1"}};
    EXPECT_EQ(expandTemplate("x{a}y{b}z", fields), "x1yz");
    EXPECT_EQ(expandTemplate("{{a}", fields), "{a}");
    EXPECT_EQ(expandTemplate("open {a", fields), "open {a");
}

TEST(SheetFields, MarksNameTheSheetTheyLeadToByItsCurrentNumber)
{
    SheetSet set = threeSheets();
    set.sheets[0].id = "s1";
    set.sheets[1].id = "s2";
    set.sheets[2].id = "s3";
    WorldMark match{WorldMark::Kind::MatchLine, {}, "MATCH LINE CH 250.000", "s3"};
    EXPECT_EQ(markLabel(set, match), "MATCH LINE CH 250.000 - SEE SHEET 3");
    // Reordered, the same mark follows its sheet.
    std::swap(set.sheets[1], set.sheets[2]);
    EXPECT_EQ(markLabel(set, match), "MATCH LINE CH 250.000 - SEE SHEET 2");
    WorldMark outline{WorldMark::Kind::SheetOutline, {}, "", "s2"};
    EXPECT_EQ(markLabel(set, outline), "3");
    // A mark whose sheet has gone keeps its label alone.
    match.sheet = "s9";
    EXPECT_EQ(markLabel(set, match), "MATCH LINE CH 250.000");
}

TEST(SheetFields, TheDrawingAreaIsTheFramesOrThePaperLessTenMillimetres)
{
    Sheet a3;
    const Box2 framed = drawingArea(a3);
    EXPECT_EQ(framed.min.x, 23.0);
    EXPECT_EQ(framed.max.y, 287.0);
    Sheet a1;
    a1.paper = PaperSize::A1;
    EXPECT_EQ(drawingArea(a1).max.x, 820.0); // 410 x 2
    // Portrait A4, no frame: 10..200 x 10..287.
    Sheet portrait;
    portrait.paper = PaperSize::A4;
    portrait.landscape = false;
    const Box2 bare = drawingArea(portrait);
    EXPECT_EQ(bare.min.x, 10.0);
    EXPECT_EQ(bare.max.x, 200.0);
    EXPECT_EQ(bare.max.y, 287.0);
}
