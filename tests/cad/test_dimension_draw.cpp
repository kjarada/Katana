// Dimension geometry (PLAN.MD Phase 09).
//
// One builder feeds the 2D viewport, the 3D scene and the cull box, so these
// assertions are what keeps all three showing the same dimension. Everything is
// in MODEL units: the viewport previously drew a fixed 5-pixel tick and a
// 12-pixel label, which looks correct on screen and plots at whatever size the
// paper happens to give it.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>

#include "katana/entity/entity_geometry.hpp"

#include "katana/cad/dimension_draw.hpp"

using katana::cad::buildDimension;
using katana::cad::DimensionDrawing;
using katana::cad::resolveDimensionStyle;
using katana::entity::ArrowHead;
using katana::entity::DimensionGeometry;
using katana::entity::DimensionStyle;
using katana::entity::Entity;
using katana::entity::Layer;
using katana::entity::Model;
using katana::geometry::Point2;

namespace {

// A 10 m dimension along the x axis, offset 2 m to the left (+y).
DimensionGeometry horizontal(double offset = 2.0)
{
    return DimensionGeometry{Point2(0.0, 0.0), Point2(10.0, 0.0), offset, ""};
}

DimensionStyle plainStyle()
{
    DimensionStyle style;
    style.textHeight = 0.5;
    style.textGap = 0.2;
    style.extensionOffset = 0.1;
    style.extensionBeyond = 0.3;
    style.arrowSize = 0.6;
    style.arrowHead = ArrowHead::Tick;
    style.decimals = 2;
    return style;
}

} // namespace

TEST(DimensionDraw, TheDimensionLineSitsAtTheOffsetAndSpansTheMeasurement)
{
    const auto drawing = buildDimension(horizontal(2.0), plainStyle());

    EXPECT_DOUBLE_EQ(drawing.dimensionLine.start.x, 0.0);
    EXPECT_DOUBLE_EQ(drawing.dimensionLine.start.y, 2.0);
    EXPECT_DOUBLE_EQ(drawing.dimensionLine.end.x, 10.0);
    EXPECT_DOUBLE_EQ(drawing.dimensionLine.end.y, 2.0);
    EXPECT_DOUBLE_EQ(drawing.dimensionLine.length(), 10.0);
}

TEST(DimensionDraw, ExtensionLinesLeaveAGapAtTheFeatureAndOvershootTheDimensionLine)
{
    // DIMEXO and DIMEXE. Without the gap the extension line touches the thing
    // being measured, which is wrong on a plan; without the overshoot the
    // dimension line has nothing to run into.
    const DimensionStyle style = plainStyle();
    const auto drawing = buildDimension(horizontal(2.0), style);

    ASSERT_EQ(drawing.extensionLines.size(), 2u);
    // Starts 0.1 above the measured point, ends 0.3 past the dimension line.
    EXPECT_DOUBLE_EQ(drawing.extensionLines[0].start.x, 0.0);
    EXPECT_DOUBLE_EQ(drawing.extensionLines[0].start.y, 0.1);
    EXPECT_DOUBLE_EQ(drawing.extensionLines[0].end.y, 2.3);
    EXPECT_DOUBLE_EQ(drawing.extensionLines[1].start.x, 10.0);
    EXPECT_DOUBLE_EQ(drawing.extensionLines[1].end.y, 2.3);
}

TEST(DimensionDraw, ANegativeOffsetMirrorsTheWholeConstruction)
{
    // Everything that means "away from the measured points" has to flip
    // together. Getting only some of it right turns the dimension inside out:
    // the line on one side and the extensions and text on the other.
    const DimensionStyle style = plainStyle();
    const auto drawing = buildDimension(horizontal(-2.0), style);

    EXPECT_DOUBLE_EQ(drawing.dimensionLine.start.y, -2.0);
    ASSERT_EQ(drawing.extensionLines.size(), 2u);
    EXPECT_DOUBLE_EQ(drawing.extensionLines[0].start.y, -0.1) << "the gap is below now";
    EXPECT_DOUBLE_EQ(drawing.extensionLines[0].end.y, -2.3) << "and so is the overshoot";
    EXPECT_LT(drawing.textAnchor.y, -2.0) << "the text must be on the same side as the line";
}

TEST(DimensionDraw, TheLabelIsFormattedThroughTheStyle)
{
    DimensionStyle style = plainStyle();
    style.decimals = 2;
    style.suffix = " m";
    EXPECT_EQ(buildDimension(horizontal(), style).text, "10.00 m");

    style.unitScale = 1000.0;
    style.decimals = 0;
    style.suffix = " mm";
    EXPECT_EQ(buildDimension(horizontal(), style).text, "10000 mm");

    // An override wins verbatim, style and all.
    DimensionGeometry overridden = horizontal();
    overridden.textOverride = "TYP";
    EXPECT_EQ(buildDimension(overridden, style).text, "TYP");
}

TEST(DimensionDraw, TheTextIsSizedInModelUnitsAndCentredOnTheDimensionLine)
{
    DimensionStyle style = plainStyle();
    style.textHeight = 0.5;
    const auto drawing = buildDimension(horizontal(2.0), style);

    EXPECT_DOUBLE_EQ(drawing.textHeight, 0.5) << "model units, not pixels";
    // Sat DIMGAP above the dimension line.
    EXPECT_DOUBLE_EQ(drawing.textAnchor.y, 2.2);
    // Backed up half the text width from the middle, so it reads centred. The
    // advance is 0.6 of the height per character; "10.00" is five characters,
    // so the half width is 0.5 * 0.6 * 5 / 2 = 0.75.
    EXPECT_NEAR(drawing.textAnchor.x, 5.0 - 0.75, 1e-9) << drawing.text;
}

TEST(DimensionDraw, TheLabelIsNeverUpsideDown)
{
    // A dimension measured right to left runs at 180 degrees; rotating the text
    // with it would put it on its head. It flips instead, which is what every
    // CAD package does and what a plan has to do to be readable.
    const DimensionGeometry backwards{Point2(10.0, 0.0), Point2(0.0, 0.0), 2.0, ""};
    const auto drawing = buildDimension(backwards, plainStyle());

    EXPECT_LE(std::abs(drawing.textRotation), 1.5707963267948966 + 1e-9)
        << "rotation " << drawing.textRotation << " would read upside down";

    // Straight up is the boundary case: a vertical dimension reads bottom to
    // top, not top to bottom.
    const DimensionGeometry vertical{Point2(0.0, 0.0), Point2(0.0, 10.0), 2.0, ""};
    EXPECT_LE(std::abs(buildDimension(vertical, plainStyle()).textRotation),
              1.5707963267948966 + 1e-9);
}

TEST(DimensionDraw, EachArrowHeadProducesItsOwnShape)
{
    DimensionStyle style = plainStyle();

    style.arrowHead = ArrowHead::None;
    auto drawing = buildDimension(horizontal(), style);
    EXPECT_TRUE(drawing.arrowStrokes.empty());
    EXPECT_TRUE(drawing.arrowFills.empty());

    style.arrowHead = ArrowHead::Tick;
    drawing = buildDimension(horizontal(), style);
    EXPECT_EQ(drawing.arrowStrokes.size(), 2u) << "one oblique stroke per end";
    EXPECT_TRUE(drawing.arrowFills.empty());

    style.arrowHead = ArrowHead::Open;
    drawing = buildDimension(horizontal(), style);
    EXPECT_EQ(drawing.arrowStrokes.size(), 4u) << "two strokes per end";

    style.arrowHead = ArrowHead::ClosedFilled;
    drawing = buildDimension(horizontal(), style);
    EXPECT_TRUE(drawing.arrowStrokes.empty());
    ASSERT_EQ(drawing.arrowFills.size(), 2u);
    EXPECT_EQ(drawing.arrowFills[0].size(), 3u) << "a triangle";

    style.arrowHead = ArrowHead::Dot;
    drawing = buildDimension(horizontal(), style);
    ASSERT_EQ(drawing.arrowFills.size(), 2u);
    EXPECT_GT(drawing.arrowFills[0].size(), 6u) << "a polygon approximating a circle";
}

TEST(DimensionDraw, ArrowsPointOutwardFromTheDimensionLineEnds)
{
    DimensionStyle style = plainStyle();
    style.arrowHead = ArrowHead::ClosedFilled;
    style.arrowSize = 0.6;
    const auto drawing = buildDimension(horizontal(2.0), style);

    ASSERT_EQ(drawing.arrowFills.size(), 2u);
    // First arrow tips at the left end and its body extends to the RIGHT.
    EXPECT_DOUBLE_EQ(drawing.arrowFills[0][0].x, 0.0);
    EXPECT_GT(drawing.arrowFills[0][1].x, 0.0) << "the body must be inside the dimension";
    // Second tips at the right end with its body to the left.
    EXPECT_DOUBLE_EQ(drawing.arrowFills[1][0].x, 10.0);
    EXPECT_LT(drawing.arrowFills[1][1].x, 10.0);
}

TEST(DimensionDraw, TheExtentCoversWhatIsDrawnNotJustWhatIsMeasured)
{
    // THE reason the extent is computed here. entity::boundingBox for a
    // Dimension covers only the measured points and the dimension line, so a
    // viewport culling against it drops a dimension whose label is still on
    // screen.
    const DimensionStyle style = plainStyle();
    const auto drawing = buildDimension(horizontal(2.0), style);

    ASSERT_FALSE(drawing.extent.empty());
    // Above the text, which sits at 2.2 and is 0.5 tall.
    EXPECT_GE(drawing.extent.max.y, 2.7 - 1e-9);
    // Below the measured points.
    EXPECT_LE(drawing.extent.min.y, 0.0 + 1e-9);
    // And wider than the measurement, because the text is centred and the
    // arrows have width.
    EXPECT_LE(drawing.extent.min.x, 0.0 + 1e-9);
    EXPECT_GE(drawing.extent.max.x, 10.0 - 1e-9);

    // Strictly bigger than what the entity layer reports.
    const katana::geometry::Box2 plain =
        katana::entity::boundingBox(katana::entity::Geometry{horizontal(2.0)});
    EXPECT_GT(drawing.extent.max.y, plain.max.y)
        << "the drawn extent must exceed the measured one, or culling is wrong";
}

TEST(DimensionDraw, ADegenerateDimensionDrawsNothingRatherThanGuessing)
{
    const DimensionStyle style = plainStyle();

    // Coincident points: no direction to offset along.
    const DimensionGeometry point{Point2(5.0, 5.0), Point2(5.0, 5.0), 2.0, ""};
    EXPECT_TRUE(buildDimension(point, style).empty());

    // A non-finite offset would put the dimension line at infinity.
    const DimensionGeometry broken{Point2(0.0, 0.0), Point2(10.0, 0.0),
                                   std::numeric_limits<double>::quiet_NaN(), ""};
    EXPECT_TRUE(buildDimension(broken, style).empty());

    // A zero offset is legitimate: the dimension line runs through the points.
    EXPECT_FALSE(buildDimension(horizontal(0.0), style).empty());
}

// ---- resolution ---------------------------------------------------------------------

TEST(DimensionDraw, AStyleResolvesThroughTheLayerThenTheDocumentDefault)
{
    Model model;

    DimensionStyle site;
    site.name = "site";
    site.decimals = 1;
    ASSERT_TRUE(model.dimensionStyles.add(site).ok());

    Layer styled;
    styled.name = "dims";
    styled.dimensionStyle = "site";
    ASSERT_TRUE(model.layers.add(styled).ok());

    Entity onStyledLayer;
    onStyledLayer.layer = "dims";
    onStyledLayer.geometry = horizontal();
    EXPECT_EQ(resolveDimensionStyle(model, onStyledLayer).decimals, 1);

    // A layer naming no style falls through to Standard.
    Entity onPlainLayer;
    onPlainLayer.layer = "0";
    onPlainLayer.geometry = horizontal();
    EXPECT_EQ(resolveDimensionStyle(model, onPlainLayer).name, "Standard");
}

TEST(DimensionDraw, AMissingLayerOrStyleStillDrawsInTheDefault)
{
    // A dimension on a deleted layer, or one naming a style that has gone, must
    // still appear: leaving it off the drawing because its styling is missing
    // would remove information from a plan.
    Model model;

    Layer dangling;
    dangling.name = "dims";
    dangling.dimensionStyle = "no-such-style";
    ASSERT_TRUE(model.layers.add(dangling).ok());

    Entity onDangling;
    onDangling.layer = "dims";
    onDangling.geometry = horizontal();
    EXPECT_EQ(resolveDimensionStyle(model, onDangling).name, "Standard");

    Entity orphan;
    orphan.layer = "deleted";
    orphan.geometry = horizontal();
    EXPECT_EQ(resolveDimensionStyle(model, orphan).name, "Standard");

    // And the resolved style actually produces a drawing.
    EXPECT_FALSE(buildDimension(horizontal(), resolveDimensionStyle(model, orphan)).empty());
}
