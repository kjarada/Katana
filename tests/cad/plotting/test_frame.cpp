// The built-in plot frame (include/katana/cad/plotting/frame.hpp) and the
// sheet scale ladder (plot.hpp).
//
// Every expected value is read off resources/plot_frames/a3_landscape.json by
// hand - its counts, a cell's corners, a text's position plus its offset and
// raise - and then scaled or fitted by hand. None is the code's output.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/frame.hpp"

using namespace katana::cad;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;

namespace {

const Frame& a3()
{
    const auto& frame = builtInFrame();
    EXPECT_TRUE(frame.ok()) << (frame.ok() ? "" : frame.error().describe());
    static const Frame empty;
    return frame.ok() ? *frame : empty;
}

void expectBox(const Box2& box, double x0, double y0, double x1, double y1)
{
    EXPECT_NEAR(box.min.x, x0, 1e-9);
    EXPECT_NEAR(box.min.y, y0, 1e-9);
    EXPECT_NEAR(box.max.x, x1, 1e-9);
    EXPECT_NEAR(box.max.y, y1, 1e-9);
}

const FrameText* textAt(const Frame& frame, double x, double y)
{
    const auto found = std::find_if(frame.texts.begin(), frame.texts.end(), [&](const FrameText& t) {
        return std::abs(t.anchor.x - x) < 1e-9 && std::abs(t.anchor.y - y) < 1e-9;
    });
    return found == frame.texts.end() ? nullptr : &*found;
}

} // namespace

TEST(PlotFrame, TheBuiltInFrameHasEveryItemOfTheCommittedDataAndTwoUserSlots)
{
    // The JSON holds 145 entities: 57 "poly", 74 "text", 14 "symbol" (the 34
    // branding entities were removed before it was committed). The frame adds
    // two texts of its own - the organisation and notes slots - so 76 texts.
    // 16 cells.
    const Frame& frame = a3();
    EXPECT_EQ(frame.polylines.size(), 57u);
    EXPECT_EQ(frame.texts.size(), 76u);
    EXPECT_EQ(frame.symbols.size(), 14u);
    EXPECT_EQ(frame.cells.size(), 16u);
}

TEST(PlotFrame, TheSheetIsA3WithTheMeasuredMarginsAndDrawingArea)
{
    // sheet.width_mm 420, height_mm 297; margins L23 R10 T10 B35; the
    // viewport 23..410 x 35..287 (387 x 252 mm).
    const Frame& frame = a3();
    EXPECT_EQ(frame.widthMm, 420.0);
    EXPECT_EQ(frame.heightMm, 297.0);
    EXPECT_EQ(frame.margins.left, 23.0);
    EXPECT_EQ(frame.margins.right, 10.0);
    EXPECT_EQ(frame.margins.top, 10.0);
    EXPECT_EQ(frame.margins.bottom, 35.0);
    expectBox(frame.drawingArea, 23.0, 35.0, 410.0, 287.0);
    expectBox(frame.border, 21.825, 34.625, 411.309, 289.265);
    expectBox(frame.titleBlock, 21.825, 8.945, 411.309, 34.625);
}

TEST(PlotFrame, TheLogoOrganisationAndNotesCellsAreTheEmptySlots)
{
    const Frame& frame = a3();
    const FrameCell* logo = frame.cell("logo");
    ASSERT_NE(logo, nullptr);
    // 167.265..220.229 x 22.625..34.625: 52.964 x 12.0 mm.
    expectBox(logo->rect, 167.265, 22.625, 220.229, 34.625);
    EXPECT_NEAR(logo->rect.width(), 52.964, 1e-9);
    EXPECT_NEAR(logo->rect.height(), 12.0, 1e-9);
    const FrameCell* organisation = frame.cell("organisation");
    ASSERT_NE(organisation, nullptr);
    expectBox(organisation->rect, 297.117, 28.865, 401.843, 34.625);
    // The measured app's disclaimer cell is the notes slot here.
    EXPECT_EQ(frame.cell("disclaimer"), nullptr);
    const FrameCell* notes = frame.cell("notes");
    ASSERT_NE(notes, nullptr);
    expectBox(notes->rect, 220.229, 12.0, 297.117, 34.625);
    // Nothing is drawn in the logo cell: no text anchors inside it.
    for (const FrameText& text : frame.texts) {
        EXPECT_FALSE(logo->rect.contains(text.anchor)) << text.content;
    }
}

TEST(PlotFrame, TextAnchorsIncludeTheirOffsetAndRaise)
{
    const Frame& frame = a3();
    // "No. of" (#25): x 406.169 + offset -1.349 = 404.820; y 32.970 + raise
    // 0.063 = 33.033.
    const FrameText* noOf = textAt(frame, 404.82, 33.033);
    ASSERT_NE(noOf, nullptr);
    EXPECT_EQ(noOf->content, "No. of");
    EXPECT_EQ(noOf->capHeightMm, 1.0);
    EXPECT_EQ(noOf->xFactor, 0.82);
    // The scale value (#127): 125.835 + 6.545 = 132.380; 25.777 + 0.219 =
    // 25.996. Cap 3, and centred in the SCALE cell like the app does.
    const FrameText* scale = textAt(frame, 132.38, 25.996);
    ASSERT_NE(scale, nullptr);
    EXPECT_EQ(scale->content, " {scale}");
    EXPECT_EQ(scale->capHeightMm, 3.0);
    EXPECT_EQ(scale->centredIn, "scale");
    // "SHEET No." (#21) has no offset: 403.902, 20.518.
    const FrameText* sheetNo = textAt(frame, 403.902, 20.518);
    ASSERT_NE(sheetNo, nullptr);
    EXPECT_EQ(sheetNo->content, "SHEET No.");
    EXPECT_EQ(sheetNo->horizontal, HorizontalJustify::Left);
    EXPECT_EQ(sheetNo->vertical, VerticalJustify::Bottom);
    // "Survey Plan Filename:" (#36) runs up the left margin at 90 degrees.
    const FrameText* filename = textAt(frame, 20.856, 9.459);
    ASSERT_NE(filename, nullptr);
    EXPECT_EQ(filename->angleDegrees, 90.0);
}

TEST(PlotFrame, FieldTokensBecomeNamedFieldsAndEachSignOffHasItsOwnDate)
{
    const Frame& frame = a3();
    // #178 " $drawing_number" and #35 " $user_text<1,...>".
    EXPECT_EQ(textAt(frame, 404.018, 13.105)->content, " {sheet_number}");
    EXPECT_EQ(textAt(frame, 404.018, 26.009)->content, " {sheet_count}");
    EXPECT_EQ(textAt(frame, 404.018, 26.009)->centredIn, "sheet_count");
    // The five-line project block (#177), middle-centre at 350.275, 26.447,
    // its region-specific first-line label gone.
    const FrameText* project = textAt(frame, 350.275, 26.447);
    ASSERT_NE(project, nullptr);
    EXPECT_EQ(project->content, "{project_line_1}\n{project_line_2}\n{project_line_3}\n"
                                "{project_line_4}\nDrawing Set Number: {set_number}");
    EXPECT_EQ(project->horizontal, HorizontalJustify::Centre);
    EXPECT_EQ(project->vertical, VerticalJustify::Middle);
    // Multi-line text steps down by 1.5 x the cap height: 1.5 x 2.5.
    EXPECT_EQ(project->lineSpacingMm, 3.75);
    // The coordinate system and height datum are fields, not one region's text.
    EXPECT_EQ(textAt(frame, 107.967, 19.993)->content, "CO-ORD SYSTEM: {coordinate_system}");
    EXPECT_EQ(textAt(frame, 145.656, 20.118)->content, "HEIGHT DATUM: {height_datum}");
    // The six date texts: each sign-off row and the stamp its own field.
    EXPECT_EQ(textAt(frame, 209.758, 20.395)->content, "{locator_date}");
    EXPECT_EQ(textAt(frame, 209.8, 16.952)->content, "{surveyor_date}");
    EXPECT_EQ(textAt(frame, 209.741, 13.439)->content, "{compiler_date}");
    EXPECT_EQ(textAt(frame, 209.787, 9.91)->content, "{reviewer_date}");
    EXPECT_EQ(textAt(frame, 284.657, 10.6)->content, "{approver_date}");
    EXPECT_EQ(textAt(frame, 379.813, 1.821)->content,
              "{paper} Border version: 2.0    {plot_date}");
    // The file name prints twice: up the left margin and in its cell.
    EXPECT_EQ(frame.textsShowing("file_name").size(), 2u);
    // No raw token survives the parse.
    for (const FrameText& text : frame.texts) {
        EXPECT_EQ(text.content.find('$'), std::string::npos) << text.content;
    }
}

TEST(PlotFrame, TheSlotsSitWhereTheRemovedBrandingWas)
{
    const Frame& frame = a3();
    ASSERT_EQ(frame.textsShowing("organisation").size(), 1u);
    const FrameText& organisation = frame.texts[frame.textsShowing("organisation").front()];
    // Centred in the organisation cell: (297.117 + 401.843) / 2 = 349.480,
    // (28.865 + 34.625) / 2 = 31.745; the removed header's cap 3.738.
    EXPECT_NEAR(organisation.anchor.x, 349.48, 1e-9);
    EXPECT_NEAR(organisation.anchor.y, 31.745, 1e-9);
    EXPECT_EQ(organisation.capHeightMm, 3.738);
    EXPECT_EQ(organisation.role, FrameRole::Slot);
    const FrameText& notes = frame.texts[frame.textsShowing("notes").front()];
    // The removed notes' first line at 220.918, 32.961, cap 0.85, and a line
    // pitch of 1.5 x 0.85 = 1.275.
    EXPECT_EQ(notes.anchor.x, 220.918);
    EXPECT_EQ(notes.anchor.y, 32.961);
    EXPECT_NEAR(notes.lineSpacingMm, 1.275, 1e-12);
}

TEST(PlotFrame, TheConstructionBorderIsAGuideThatDoesNotPlot)
{
    // Two construction rectangles (entities 39 and 41) at the paper edge, in
    // orange (#ff8000) at 0.18 mm; every other line plots.
    const Frame& frame = a3();
    std::size_t guides = 0;
    for (const FramePolyline& line : frame.polylines) {
        if (line.role == FrameRole::Construction) {
            ++guides;
            EXPECT_FALSE(line.plots);
            EXPECT_EQ(line.colour.toHex(), "#FF8000");
        } else {
            EXPECT_TRUE(line.plots);
        }
    }
    EXPECT_EQ(guides, 2u);
}

TEST(PlotFrame, TheDashedUnderlinesAreTwoPointTwoOnOnePointOne)
{
    // 12 field underlines, "DGN Style 1": dash 2.2 mm, gap 1.1 mm, 0.53 mm.
    const Frame& frame = a3();
    std::size_t dashed = 0;
    for (const FramePolyline& line : frame.polylines) {
        if (line.role == FrameRole::Underline) {
            ++dashed;
            EXPECT_EQ(line.dashMm, 2.2);
            EXPECT_EQ(line.gapMm, 1.1);
            EXPECT_EQ(line.weightMm, 0.53);
        }
    }
    EXPECT_EQ(dashed, 12u);
}

TEST(PlotFrame, AClosedRingDoesNotRepeatItsFirstPoint)
{
    // The border (#0) is five points in the data, the last the first again.
    const Frame& frame = a3();
    ASSERT_FALSE(frame.polylines.empty());
    const FramePolyline& border = frame.polylines.front();
    EXPECT_TRUE(border.closed);
    EXPECT_EQ(border.points.size(), 4u);
}

TEST(PlotFrame, OtherASizesScaleTheA3FrameUniformly)
{
    // A1: min(841 / 420, 594 / 297) = min(2.00238, 2) = 2 exactly.
    EXPECT_EQ(frameScaleFor(PaperSize::A1, true), 2.0);
    // A4 landscape: min(297 / 420, 210 / 297) = 210 / 297 = 0.70707...
    EXPECT_EQ(frameScaleFor(PaperSize::A4, true), 210.0 / 297.0);
    // A0: min(1189 / 420 = 2.83095, 841 / 297 = 2.83165) = 1189 / 420.
    EXPECT_EQ(frameScaleFor(PaperSize::A0, true), 1189.0 / 420.0);
    // A portrait sheet has no frame.
    EXPECT_EQ(frameScaleFor(PaperSize::A3, false), 0.0);

    const auto a1 = frameFor(kBuiltInFrameId, PaperSize::A1, true);
    ASSERT_TRUE(a1.ok()) << a1.error().describe();
    EXPECT_EQ(a1->widthMm, 841.0);
    EXPECT_EQ(a1->heightMm, 594.0);
    // Everything doubles: the drawing area 46..820 x 70..574, the logo cell
    // 334.530..440.458 x 45.25..69.25, the 0.53 mm border 1.06 mm, "SHEET
    // No." at 807.804, 41.036 with a 2 mm cap.
    expectBox(a1->drawingArea, 46.0, 70.0, 820.0, 574.0);
    expectBox(a1->cell("logo")->rect, 334.53, 45.25, 440.458, 69.25);
    EXPECT_NEAR(a1->polylines.front().weightMm, 1.06, 1e-12);
    const FrameText* sheetNo = textAt(*a1, 807.804, 41.036);
    ASSERT_NE(sheetNo, nullptr);
    EXPECT_EQ(sheetNo->capHeightMm, 2.0);
    // The millimetre of slack A1 has over twice A3 (841 against 840) goes to
    // the right margin: 841 - 820 = 21; the top is 594 - 574 = 20.
    EXPECT_NEAR(a1->margins.right, 21.0, 1e-9);
    EXPECT_NEAR(a1->margins.top, 20.0, 1e-9);
    EXPECT_EQ(a1->margins.left, 46.0);
    // The construction guide follows the paper's edge, not the scaled frame's.
    for (const FramePolyline& line : a1->polylines) {
        if (line.role == FrameRole::Construction) {
            double right = 0.0;
            for (const Point2& point : line.points) {
                right = std::max(right, point.x);
            }
            EXPECT_GE(right, 841.0);
        }
    }
}

TEST(PlotFrame, APortraitSheetOrAnUnknownFrameIsRefused)
{
    EXPECT_EQ(frameFor(kBuiltInFrameId, PaperSize::A3, false).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(frameFor("no such frame", PaperSize::A3, true).error().code, ErrorCode::NotFound);
}

TEST(PlotFrame, ALogoIsFittedIntoItsCellWithItsAspectKept)
{
    // The logo cell less 1 mm all round is 50.964 x 10 mm. A 4:1 image is
    // limited by the height: 40 x 10 mm, centred on the cell's centre
    // (193.747, 28.625).
    const Box2 cell = a3().cell("logo")->rect;
    const Box2 wide = fitImage(cell, 400.0, 100.0);
    expectBox(wide, 173.747, 23.625, 213.747, 33.625);
    // A 10:1 image is limited by the width: 50.964 x 5.0964 mm.
    const Box2 banner = fitImage(cell, 1000.0, 100.0);
    EXPECT_NEAR(banner.width(), 50.964, 1e-9);
    EXPECT_NEAR(banner.height(), 5.0964, 1e-9);
    // An image with no size has nowhere to go.
    EXPECT_TRUE(fitImage(cell, 0.0, 100.0).empty());
}

TEST(PlotFrame, ACorruptFrameIsAParseFailureNotACrash)
{
    EXPECT_EQ(parseFrame("not json").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseFrame("{}").error().code, ErrorCode::ParseFailure);
}

TEST(SheetScales, TheSheetLadderIsFinerThanThePlotLadderAndKeepsItsSteps)
{
    // A section needs 1:10 to 1:75 and the steps between the plot ladder's.
    EXPECT_EQ(kSheetScales.front(), 1.0);
    EXPECT_EQ(kSheetScales.back(), 50000.0);
    EXPECT_TRUE(std::is_sorted(kSheetScales.begin(), kSheetScales.end()));
    // Every step of the single-page plot's ladder is on the sheet ladder too.
    for (const double scale : kStandardScales) {
        EXPECT_NE(std::find(kSheetScales.begin(), kSheetScales.end(), scale), kSheetScales.end())
            << scale;
    }
    EXPECT_EQ(*sheetScaleAtLeast(60.0), 75.0);
    EXPECT_EQ(*sheetScaleAtLeast(501.3), 750.0);
    EXPECT_EQ(*sheetScaleAtLeast(1100.0), 1250.0);
    EXPECT_EQ(*sheetScaleAtLeast(500.0), 500.0); // exactly fits
    EXPECT_EQ(*sheetScaleAtLeast(60000.0), 60000.0);
    EXPECT_EQ(sheetScaleAtLeast(0.0).error().code, ErrorCode::InvalidArgument);
}
