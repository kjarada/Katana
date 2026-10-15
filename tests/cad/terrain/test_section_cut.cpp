// A section of the drawing (include/katana/cad/terrain/section_cut.hpp): the shown
// surfaces along an alignment, with a design profile beside them. What an open section
// view keeps - the section's own alignment and its design - is what cuts it again, so
// the cut is held to values worked by hand.
//
// The surfaces are planes with corners at exact heights, so the elevation of a cut
// at any station is a line worked out on paper, never read from a run:
//
//   ground  z = 10 + x / 10 over x 0..100, y 0..20  (10 at the west edge, 20 at the east)
//   raised  z = 15 + x / 10                         (ground, five metres up)
//   flat    z = 10 over x 0..300, y 0..20           (for the design profile's section)
//
// Each is two triangles split along the diagonal (0,0) - (x1,20), which the line
// y = 10 crosses at x = x1 / 2: a slope break the cut must find.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <memory>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/terrain/section_cut.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

using katana::cad::cutDrawingSection;
using katana::cad::Document;
using katana::cad::SampleReason;
using katana::cad::SectionDesign;
using katana::core::ErrorCode;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

// A rectangle x 0..width, y 0..20 whose height is `west` at x = 0 and `east` at x =
// width, as two triangles split on the diagonal from (0,0) to (width,20).
std::shared_ptr<const katana::terrain::TinSurface> slab(double width, double west, double east)
{
    auto surface = katana::terrain::TinSurface::create(
        {{0.0, 0.0, west}, {width, 0.0, east}, {width, 20.0, east}, {0.0, 20.0, west}},
        {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(surface.ok());
    return std::make_shared<const katana::terrain::TinSurface>(std::move(*surface));
}

void addSurface(Document& document, const std::string& name,
                std::shared_ptr<const katana::terrain::TinSurface> surface)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.add({name, std::move(surface), "test"}).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
}

void setVisible(Document& document, const std::string& name, bool visible)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.setVisible(name, visible).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
}

// East along y = 10 from x = 0 to x = `length`: station s is the point (s, 10).
Polyline2 eastAlong(double length)
{
    Polyline2 line;
    line.vertices = {Point2(0.0, 10.0), Point2(length, 10.0)};
    return line;
}

// The textbook sag the section tests use: PVIs (0, 16), (100, 13, L = 100), (300, 17),
// -3% into +2%: PVC 50 @ 14.5, PVT 150 @ 14.0, the low point at station 110 @ 13.6
// (x = 0.03 * 100 / 0.05 = 60 past the PVC).
katana::geometry::SolvedProfile sag()
{
    katana::geometry::VerticalAlignment definition;
    definition.pvis = {katana::geometry::ProfilePVI{0.0, 16.0},
                       katana::geometry::ProfilePVI{100.0, 13.0, 100.0},
                       katana::geometry::ProfilePVI{300.0, 17.0}};
    auto solved = katana::geometry::solveProfile(definition);
    EXPECT_TRUE(solved.ok());
    return *solved;
}

} // namespace

TEST(SectionCut, TheShownSurfacesAreCutAlongTheAlignmentAtTheDrawingsInterval)
{
    Document document;
    addSurface(document, "ground", slab(100.0, 10.0, 20.0));
    const auto cut = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(cut.ok()) << (cut.ok() ? "" : cut.error().describe());

    // 100 m of alignment at 100 / 2000 = 0.05 m is below the 0.1 m floor: 0.1 m,
    // so a thousand intervals and at least 1,001 stations.
    EXPECT_DOUBLE_EQ(cut->interval, 0.1);
    EXPECT_NEAR(cut->section.length, 100.0, 1e-9);
    ASSERT_EQ(cut->section.surfaces.size(), 1u);
    const katana::cad::SectionSurface& ground = cut->section.surfaces.front();
    EXPECT_EQ(ground.name, "ground");
    EXPECT_GE(ground.samples.size(), 1001u);

    // Every station is on the plane z = 10 + s / 10, to the arithmetic of one
    // barycentric interpolation; the first and the last are the plane's ends.
    ASSERT_TRUE(ground.samples.front().elevation.has_value());
    EXPECT_NEAR(ground.samples.front().station, 0.0, 1e-12);
    EXPECT_NEAR(*ground.samples.front().elevation, 10.0, 1e-9);
    EXPECT_NEAR(ground.samples.back().station, 100.0, 1e-9);
    EXPECT_NEAR(*ground.samples.back().elevation, 20.0, 1e-9);
    for (const katana::cad::SectionSample& sample : ground.samples) {
        ASSERT_TRUE(sample.elevation.has_value()) << sample.station;
        EXPECT_NEAR(*sample.elevation, 10.0 + sample.station / 10.0, 1e-9) << sample.station;
    }
    // The diagonal (0,0) - (100,20) is crossed by y = 10 at x = 50, height 15: a
    // slope break, which the cut places a sample on.
    const auto breakAt = std::find_if(
        ground.samples.begin(), ground.samples.end(), [&](const katana::cad::SectionSample& sample) {
            const std::size_t at = static_cast<std::size_t>(&sample - ground.samples.data());
            return ground.reasons[at] == SampleReason::SurfaceBreak &&
                   std::abs(sample.station - 50.0) < 1e-5;
        });
    ASSERT_NE(breakAt, ground.samples.end()) << "no sample on the slope break at 50";
    EXPECT_NEAR(*breakAt->elevation, 15.0, 1e-6);
}

TEST(SectionCut, AHiddenSurfaceIsLeftOutOfTheCutAndPutBackWhenShown)
{
    Document document;
    addSurface(document, "ground", slab(100.0, 10.0, 20.0));
    addSurface(document, "raised", slab(100.0, 15.0, 25.0));
    auto cut = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(cut.ok());
    ASSERT_EQ(cut->section.surfaces.size(), 2u);

    setVisible(document, "raised", false);
    cut = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(cut.ok());
    ASSERT_EQ(cut->section.surfaces.size(), 1u) << "a hidden surface is in no picture";
    EXPECT_EQ(cut->section.surfaces.front().name, "ground");

    setVisible(document, "raised", true);
    cut = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(cut.ok());
    ASSERT_EQ(cut->section.surfaces.size(), 2u);
    // The raised surface is the ground plus five, at every station: z = 15 + s / 10.
    for (const katana::cad::SectionSample& sample : cut->section.surfaces[1].samples) {
        ASSERT_TRUE(sample.elevation.has_value());
        EXPECT_NEAR(*sample.elevation, 15.0 + sample.station / 10.0, 1e-9) << sample.station;
    }
}

TEST(SectionCut, ADrawingWithNoSurfaceIsNotFoundAndOneWithEveryOneHiddenIsInvalidState)
{
    // Two refusals a caller words differently - build one, or show one - so each has
    // its own code.
    Document document;
    auto cut = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_FALSE(cut.ok());
    EXPECT_EQ(cut.error().code, ErrorCode::NotFound);

    addSurface(document, "ground", slab(100.0, 10.0, 20.0));
    setVisible(document, "ground", false);
    cut = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_FALSE(cut.ok());
    EXPECT_EQ(cut.error().code, ErrorCode::InvalidState);
}

TEST(SectionCut, AnAlignmentOfOnePointOrNoneIsRefusedAsInvalidArgument)
{
    Document document;
    addSurface(document, "ground", slab(100.0, 10.0, 20.0));
    Polyline2 one;
    one.vertices = {Point2(10.0, 10.0)};
    const auto single = cutDrawingSection(document, one);
    ASSERT_FALSE(single.ok());
    EXPECT_EQ(single.error().code, ErrorCode::InvalidArgument);
    const auto none = cutDrawingSection(document, Polyline2{});
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::InvalidArgument);
}

TEST(SectionCut, TheIntervalIsATwoThousandthOfTheLengthAndNoFinerThanOneHundredMillimetres)
{
    // max(length / 2000, 0.1), by hand: 100 -> 0.05 -> 0.1; 200 -> 0.1 exactly; 4,000
    // -> 2; a million metres -> 500; a degenerate length of 0 -> the floor.
    EXPECT_DOUBLE_EQ(katana::cad::drawingSectionInterval(100.0), 0.1);
    EXPECT_DOUBLE_EQ(katana::cad::drawingSectionInterval(200.0), 0.1);
    EXPECT_DOUBLE_EQ(katana::cad::drawingSectionInterval(4000.0), 2.0);
    EXPECT_DOUBLE_EQ(katana::cad::drawingSectionInterval(1'000'000.0), 500.0);
    EXPECT_DOUBLE_EQ(katana::cad::drawingSectionInterval(0.0), 0.1);
}

TEST(SectionCut, ADesignProfileIsCutBesideTheGroundUnderTheNameItWasGiven)
{
    Document document;
    addSurface(document, "ground", slab(300.0, 10.0, 10.0));
    const SectionDesign design{sag(), "design Main"};
    const auto cut = cutDrawingSection(document, eastAlong(300.0), &design);
    ASSERT_TRUE(cut.ok()) << (cut.ok() ? "" : cut.error().describe());
    ASSERT_EQ(cut->section.surfaces.size(), 2u);
    EXPECT_EQ(cut->section.surfaces[0].name, "ground");
    const katana::cad::SectionSurface& line = cut->section.surfaces[1];
    EXPECT_EQ(line.name, "design Main");
    // The profile's own figures, worked by hand in the sag's comment: 16 at the start,
    // 17 at the end, the low point 13.6, and 16 - 0.03 * 50 = 14.5 at the PVC (50).
    EXPECT_NEAR(*line.samples.front().elevation, 16.0, 1e-12);
    EXPECT_NEAR(*line.samples.back().elevation, 17.0, 1e-12);
    EXPECT_NEAR(*line.minElevation, 13.6, 1e-12);
    const auto pvc = std::find_if(line.samples.begin(), line.samples.end(),
                                  [](const katana::cad::SectionSample& sample) {
                                      return sample.station == 50.0;
                                  });
    ASSERT_NE(pvc, line.samples.end());
    EXPECT_NEAR(*pvc->elevation, 14.5, 1e-12);
}

TEST(SectionCut, ADesignWithNoNameIsRefusedAndNothingIsCut)
{
    Document document;
    addSurface(document, "ground", slab(300.0, 10.0, 10.0));
    const SectionDesign nameless{sag(), ""};
    const auto cut = cutDrawingSection(document, eastAlong(300.0), &nameless);
    ASSERT_FALSE(cut.ok());
    EXPECT_EQ(cut.error().code, ErrorCode::InvalidArgument);
}

TEST(SectionCut, AnEntityCrossingTheAlignmentIsACrossingAtItsStation)
{
    // A line x = 30 from y 0 to y 20 crosses the alignment y = 10 at station 30: the
    // model is read, so what is drawn across the section shows on it.
    Document document;
    addSurface(document, "ground", slab(100.0, 10.0, 20.0));
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(30.0, 0.0), Point2(30.0, 20.0)))
                    .ok());
    const auto cut = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(cut.ok());
    ASSERT_EQ(cut->section.crossings.size(), 1u);
    EXPECT_NEAR(cut->section.crossings.front().station, 30.0, 1e-9);
}

TEST(SectionCut, TheSameCutAfterASurfaceIsRaisedFollowsItAndAfterTheUndoGoesBack)
{
    // What the re-cut of an open section is: the same alignment cut again after the
    // drawing's surfaces changed gives the surfaces as they are now, and undo puts the
    // first picture back. Ground raised by five: 15 + s / 10 at station s; undone: 10 +
    // s / 10.
    Document document;
    addSurface(document, "ground", slab(100.0, 10.0, 20.0));
    const auto before = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(before.ok());
    EXPECT_NEAR(*before->section.surfaces.front().samples.back().elevation, 20.0, 1e-9);

    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.replace("ground", slab(100.0, 15.0, 25.0)).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "RAISE").ok());
    const auto raised = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(raised.ok());
    EXPECT_NEAR(*raised->section.surfaces.front().samples.back().elevation, 25.0, 1e-9);

    ASSERT_TRUE(document.undo().ok());
    const auto undone = cutDrawingSection(document, eastAlong(100.0));
    ASSERT_TRUE(undone.ok());
    EXPECT_NEAR(*undone->section.surfaces.front().samples.back().elevation, 20.0, 1e-9);
}
