// What a hatch pattern is drawn as, and which pattern an entity resolves to
// (include/katana/cad/hatching.hpp).

#include <gtest/gtest.h>

#include <vector>

#include "katana/cad/hatching.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/model.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using katana::entity::HatchLineFamily;
using katana::entity::HatchPattern;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

HatchPattern lineFamily(const char* name, double spacing)
{
    HatchPattern pattern;
    pattern.name = name;
    pattern.families.push_back(HatchLineFamily{0.0, spacing, 0.0});
    return pattern;
}

Polyline2 square(double size)
{
    Polyline2 polygon;
    polygon.vertices = {Point2(0, 0), Point2(size, 0), Point2(size, size), Point2(0, size)};
    polygon.closed = true;
    return polygon;
}

} // namespace

TEST(Hatching, AFamilyTooFineToResolveIsDrawnSolidRatherThanAsThousandsOfLines)
{
    // 0.2 m spacing at 5 pixels per metre is one pixel between lines: the gaps
    // cannot be seen, so the honest drawing is a solid tone and it costs one
    // polygon instead of thousands of clipped segments.
    const HatchPattern fine = lineFamily("fine", 0.2);
    HatchOptions zoomedOut;
    zoomedOut.viewScale = 5.0; // 0.2 * 5 = 1 pixel, under the 3 px threshold
    EXPECT_EQ(hatchDrawing(fine, zoomedOut), HatchDrawing::Solid);

    HatchOptions zoomedIn;
    zoomedIn.viewScale = 40.0; // 0.2 * 40 = 8 pixels
    EXPECT_EQ(hatchDrawing(fine, zoomedIn), HatchDrawing::Lines);
}

TEST(Hatching, ACrosshatchDoesNotBecomeHalfSolidAndHalfLines)
{
    // Two families at different spacings. If the decision were taken per family
    // there would be a zoom at which the coarse one still drew as lines while
    // the fine one had become solid, and the pattern would look like neither.
    HatchPattern crosshatch;
    crosshatch.name = "mixed";
    crosshatch.families.push_back(HatchLineFamily{0.0, 2.0, 0.0});
    crosshatch.families.push_back(HatchLineFamily{katana::math::kPi / 2.0, 0.1, 0.0});

    HatchOptions options;
    options.viewScale = 5.0; // coarse: 10 px (fine enough for lines); fine: 0.5 px
    EXPECT_EQ(hatchDrawing(crosshatch, options), HatchDrawing::Solid);
}

TEST(Hatching, TheBuiltInNonePatternDrawsNothingAndASolidOneIsAlwaysSolid)
{
    HatchPattern none;
    none.name = "none";
    HatchOptions options;
    options.viewScale = 100.0;
    EXPECT_EQ(hatchDrawing(none, options), HatchDrawing::None);

    HatchPattern solid;
    solid.name = "filled";
    solid.solid = true;
    EXPECT_EQ(hatchDrawing(solid, options), HatchDrawing::Solid);
    // A solid fill is a solid fill at every zoom - there is no spacing for the
    // threshold to act on.
    options.viewScale = 0.001;
    EXPECT_EQ(hatchDrawing(solid, options), HatchDrawing::Solid);
}

TEST(Hatching, AViewThatHasNotBeenScaledYetDrawsSolidRatherThanAskingForNonsense)
{
    // A zero or negative scale means the widget has not been laid out. Working
    // out a pixel spacing from it would be meaningless, so the cheap branch is
    // taken rather than generating a family for a view nobody is looking at.
    const HatchPattern pattern = lineFamily("hatch", 1.0);
    HatchOptions unset;
    unset.viewScale = 0.0;
    EXPECT_EQ(hatchDrawing(pattern, unset), HatchDrawing::Solid);
}

TEST(Hatching, EveryFamilyOfAPatternContributesItsOwnLines)
{
    // A crosshatch is two families, and the segments of both must come back -
    // a renderer that saw only the first would draw half a pattern and nothing
    // would report it.
    HatchPattern crosshatch;
    crosshatch.name = "cross";
    crosshatch.families.push_back(HatchLineFamily{0.0, 2.0, 0.0});
    crosshatch.families.push_back(HatchLineFamily{katana::math::kPi / 2.0, 2.0, 0.0});

    HatchPattern verticalOnly;
    verticalOnly.name = "vertical";
    verticalOnly.families.push_back(crosshatch.families[1]);

    const auto horizontal = hatchSegments(square(10.0), lineFamily("horizontal", 2.0));
    const auto vertical = hatchSegments(square(10.0), verticalOnly);
    const auto both = hatchSegments(square(10.0), crosshatch);

    // Not "twice the one-family count": the two families do not produce the
    // same number of lines even on a square. hatchLines counts an edge lying
    // exactly on a line under a half-open rule, so one bounding line of the
    // square is excluded per family - and which one depends on the direction
    // of that family's normal, giving 5 horizontal lines and 6 vertical. The
    // property that does hold is that the result is the families concatenated.
    EXPECT_FALSE(horizontal.empty());
    EXPECT_FALSE(vertical.empty());
    EXPECT_NE(horizontal.size(), vertical.size()); // the asymmetry above, pinned
    EXPECT_EQ(both.size(), horizontal.size() + vertical.size());
}

TEST(Hatching, ABoundaryThatCannotBeHatchedYieldsNoSegmentsRatherThanFailing)
{
    // A renderer asks this once per entity per frame and has nothing useful to
    // do with an error. The model has already refused an invalid pattern.
    Polyline2 open = square(10.0);
    open.closed = false;
    EXPECT_TRUE(hatchSegments(open, lineFamily("hatch", 1.0)).empty());

    HatchPattern solid;
    solid.name = "filled";
    solid.solid = true;
    EXPECT_TRUE(hatchSegments(square(10.0), solid).empty()) << "a solid fill has no lines";
}

// ---- resolution through the ByLayer chain ----------------------------------------

TEST(Hatching, AnEntityTakesItsHatchFromItsLayerAndAStyleOverridesIt)
{
    katana::entity::Model model;
    ASSERT_TRUE(model.hatchPatterns.add(lineFamily("rock", 1.0)));
    ASSERT_TRUE(model.hatchPatterns.add(lineFamily("grass", 2.0)));

    katana::entity::Layer layer;
    layer.name = "ground";
    layer.hatchPattern = "rock";
    ASSERT_TRUE(model.layers.add(layer));

    katana::entity::Entity entity;
    entity.geometry = square(10.0);
    entity.layer = "ground";
    ASSERT_NE(resolveHatchPattern(model, entity), nullptr);
    EXPECT_EQ(resolveHatchPattern(model, entity)->name, "rock");

    katana::entity::Style style;
    style.name = "lawn";
    style.hatchPattern = "grass";
    ASSERT_TRUE(model.styles.add(style));
    entity.style = "lawn";
    ASSERT_NE(resolveHatchPattern(model, entity), nullptr);
    EXPECT_EQ(resolveHatchPattern(model, entity)->name, "grass");
}

TEST(Hatching, AStyleThatSaysNothingAboutHatchingLeavesTheLayerPatternAlone)
{
    // Style::hatchPattern is empty by default, which means ByLayer. If an empty
    // style field overrode the layer, naming any style at all would silently
    // un-hatch the entity.
    katana::entity::Model model;
    ASSERT_TRUE(model.hatchPatterns.add(lineFamily("rock", 1.0)));
    katana::entity::Layer layer;
    layer.name = "ground";
    layer.hatchPattern = "rock";
    ASSERT_TRUE(model.layers.add(layer));
    katana::entity::Style style; // no hatch pattern named
    style.name = "thin";
    ASSERT_TRUE(model.styles.add(style));

    katana::entity::Entity entity;
    entity.geometry = square(10.0);
    entity.layer = "ground";
    entity.style = "thin";
    ASSERT_NE(resolveHatchPattern(model, entity), nullptr);
    EXPECT_EQ(resolveHatchPattern(model, entity)->name, "rock");
}

TEST(Hatching, AnEntityOnAnUnhatchedOrMissingLayerResolvesToNothing)
{
    katana::entity::Model model;
    katana::entity::Entity entity;
    entity.geometry = square(10.0);
    entity.layer = "0"; // the default layer, which is not hatched
    EXPECT_EQ(resolveHatchPattern(model, entity), nullptr);

    // A layer that has been removed. The model permits the dangling reference
    // and a renderer must not crash on it.
    entity.layer = "deleted";
    EXPECT_EQ(resolveHatchPattern(model, entity), nullptr);
}

TEST(Hatching, APatternThatHasBeenDeletedReadsAsUnhatchedRatherThanCrashing)
{
    katana::entity::Model model;
    katana::entity::Layer layer;
    layer.name = "ground";
    layer.hatchPattern = "rock"; // never added to the table
    ASSERT_TRUE(model.layers.add(layer));

    katana::entity::Entity entity;
    entity.geometry = square(10.0);
    entity.layer = "ground";
    EXPECT_EQ(resolveHatchPattern(model, entity), nullptr);
}
