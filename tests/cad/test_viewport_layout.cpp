// Tiled viewports (PLAN.MD Phase 08).
//
// The arithmetic tested here decides which viewport a click lands in and how
// big each one's framebuffer is. Getting it a pixel wrong shows up as a seam or
// as clicks activating the neighbouring view, both of which are much easier to
// pin down here than in a running window.

#include <gtest/gtest.h>

#include <set>

#include "katana/cad/viewport_layout.hpp"

using katana::cad::CellRect;
using katana::cad::LayoutKind;
using katana::cad::ViewKind;
using katana::cad::ViewportLayout;

namespace {

constexpr LayoutKind kAllLayouts[] = {LayoutKind::Single,     LayoutKind::SplitVertical,
                                      LayoutKind::SplitHorizontal, LayoutKind::ThreeLeft,
                                      LayoutKind::ThreeTop,   LayoutKind::Quad};

} // namespace

TEST(ViewportLayout, StartsAsASinglePlanView)
{
    const ViewportLayout layout;
    ASSERT_EQ(layout.size(), 1u);
    EXPECT_EQ(layout.layout(), LayoutKind::Single);
    EXPECT_EQ(layout.cell(0).kind, ViewKind::Plan);
    EXPECT_EQ(layout.activeIndex(), 0u);
}

TEST(ViewportLayout, EveryLayoutTilesTheAreaExactlyOnce)
{
    for (LayoutKind kind : kAllLayouts) {
        const auto rects = katana::cad::layoutRects(kind);
        ASSERT_FALSE(rects.empty());

        double area = 0.0;
        for (const CellRect& rect : rects) {
            EXPECT_GT(rect.width, 0.0);
            EXPECT_GT(rect.height, 0.0);
            EXPECT_GE(rect.x, 0.0);
            EXPECT_GE(rect.y, 0.0);
            EXPECT_LE(rect.x + rect.width, 1.0 + 1e-12);
            EXPECT_LE(rect.y + rect.height, 1.0 + 1e-12);
            area += rect.width * rect.height;
        }
        // Tiled, not floating: the cells cover the area and do not overlap, so
        // the areas must add to exactly one.
        EXPECT_NEAR(area, 1.0, 1e-12) << katana::cad::toString(kind);
    }
}

TEST(ViewportLayout, EveryPointBelongsToExactlyOneCell)
{
    for (LayoutKind kind : kAllLayouts) {
        ViewportLayout layout;
        layout.setLayout(kind);

        // A fine sweep including the shared borders at 0.5, where a closed
        // rectangle test would match two cells.
        for (int i = 0; i < 40; ++i) {
            for (int j = 0; j < 40; ++j) {
                const double u = static_cast<double>(i) / 40.0;
                const double v = static_cast<double>(j) / 40.0;
                int matches = 0;
                for (std::size_t c = 0; c < layout.size(); ++c) {
                    if (layout.cell(c).rect.contains(u, v)) {
                        ++matches;
                    }
                }
                ASSERT_EQ(matches, 1) << katana::cad::toString(kind) << " at " << u << "," << v;
                EXPECT_TRUE(layout.cellAt(u, v).has_value());
            }
        }
    }
}

TEST(ViewportLayout, PointsOutsideTheAreaHitNothing)
{
    ViewportLayout layout;
    layout.setLayout(LayoutKind::Quad);
    EXPECT_FALSE(layout.cellAt(-0.01, 0.5).has_value());
    EXPECT_FALSE(layout.cellAt(0.5, 1.0).has_value()) << "the far edge is exclusive";
    EXPECT_FALSE(layout.cellAt(1.0, 0.5).has_value());
    EXPECT_FALSE(layout.cellAt(std::nan(""), 0.5).has_value());
}

TEST(ViewportLayout, QuadPutsPlanThreeDSectionAndElevationInThatOrder)
{
    ViewportLayout layout;
    layout.setLayout(LayoutKind::Quad);
    ASSERT_EQ(layout.size(), 4u);
    EXPECT_EQ(layout.cell(0).kind, ViewKind::Plan);
    EXPECT_EQ(layout.cell(1).kind, ViewKind::Model3D);
    EXPECT_EQ(layout.cell(2).kind, ViewKind::Section);
    EXPECT_EQ(layout.cell(3).kind, ViewKind::Elevation);
}

TEST(ViewportLayout, GrowingTheLayoutKeepsTheViewTheUserAlreadyHad)
{
    ViewportLayout layout;
    layout.setPixelSize(800, 600);
    ASSERT_TRUE(layout.setCellKind(0, ViewKind::Model3D).ok());
    layout.cell(0).camera.setTarget(katana::math::Vec3(123.0, 456.0, 7.0));
    layout.cell(0).camera.setDistance(42.0);

    layout.setLayout(LayoutKind::Quad);

    ASSERT_EQ(layout.size(), 4u);
    EXPECT_EQ(layout.cell(0).kind, ViewKind::Model3D) << "cell 0 must survive the split";
    EXPECT_EQ(layout.cell(0).camera.target().x, 123.0);
    EXPECT_EQ(layout.cell(0).camera.distance(), 42.0);

    // And shrinking back keeps it too.
    layout.setLayout(LayoutKind::Single);
    ASSERT_EQ(layout.size(), 1u);
    EXPECT_EQ(layout.cell(0).camera.distance(), 42.0);
}

TEST(ViewportLayout, ShrinkingMovesTheActiveCellInsteadOfLeavingItDangling)
{
    ViewportLayout layout;
    layout.setLayout(LayoutKind::Quad);
    layout.setActiveIndex(3);
    ASSERT_EQ(layout.activeIndex(), 3u);

    layout.setLayout(LayoutKind::SplitVertical);
    ASSERT_EQ(layout.size(), 2u);
    EXPECT_LT(layout.activeIndex(), layout.size()) << "the active index must stay in range";
}

TEST(ViewportLayout, ActivatingAnIndexOutsideTheLayoutDoesNothing)
{
    ViewportLayout layout;
    layout.setLayout(LayoutKind::SplitVertical);
    layout.setActiveIndex(1);
    layout.setActiveIndex(9); // nonsense
    EXPECT_EQ(layout.activeIndex(), 1u) << "a bad index must not silently activate another view";
}

TEST(ViewportLayout, SettingTheKindOfAMissingCellIsReportedNotIgnored)
{
    ViewportLayout layout;
    const auto status = layout.setCellKind(5, ViewKind::Section);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, katana::core::ErrorCode::InvalidArgument);
}

TEST(ViewportLayout, PixelRectanglesTileTheWindowWithNoSeamAndNoOverlap)
{
    // Odd sizes on purpose: 801 does not divide by two, which is exactly where
    // a naive floor leaves a one-pixel gap down the middle.
    for (const auto& size : {std::pair{800, 600}, std::pair{801, 601}, std::pair{1023, 767},
                             std::pair{3, 5}}) {
        for (LayoutKind kind : kAllLayouts) {
            ViewportLayout layout;
            layout.setLayout(kind);
            layout.setPixelSize(size.first, size.second);

            // Count how many cells claim each pixel.
            std::vector<int> cover(static_cast<std::size_t>(size.first) *
                                       static_cast<std::size_t>(size.second),
                                   0);
            for (std::size_t c = 0; c < layout.size(); ++c) {
                const auto rect = layout.pixelRect(c);
                ASSERT_TRUE(rect.ok());
                for (int y = static_cast<int>(rect->y);
                     y < static_cast<int>(rect->y + rect->height); ++y) {
                    for (int x = static_cast<int>(rect->x);
                         x < static_cast<int>(rect->x + rect->width); ++x) {
                        if (x < size.first && y < size.second) {
                            ++cover[static_cast<std::size_t>(y) *
                                        static_cast<std::size_t>(size.first) +
                                    static_cast<std::size_t>(x)];
                        }
                    }
                }
            }
            for (std::size_t i = 0; i < cover.size(); ++i) {
                ASSERT_EQ(cover[i], 1)
                    << katana::cad::toString(kind) << " at " << size.first << "x" << size.second
                    << " pixel " << i;
            }
        }
    }
}

TEST(ViewportLayout, CellsNeverReportAZeroSizedFramebuffer)
{
    // A window dragged down to nothing must not ask for a 0 x 0 framebuffer,
    // which Framebuffer::create rejects.
    ViewportLayout layout;
    layout.setLayout(LayoutKind::Quad);
    for (int size : {0, 1, 2, 3}) {
        layout.setPixelSize(size, size);
        for (std::size_t c = 0; c < layout.size(); ++c) {
            const auto rect = layout.pixelRect(c);
            ASSERT_TRUE(rect.ok());
            EXPECT_GE(rect->width, 1.0) << "size " << size << " cell " << c;
            EXPECT_GE(rect->height, 1.0);
        }
    }
}

TEST(ViewportLayout, ResizingUpdatesEveryCellsCameraViewport)
{
    ViewportLayout layout;
    layout.setLayout(LayoutKind::Quad);
    layout.setPixelSize(1000, 800);

    for (std::size_t c = 0; c < layout.size(); ++c) {
        // A camera whose viewport disagreed with its framebuffer would be
        // rejected by the rasteriser, so this has to stay in step.
        const auto rect = layout.pixelRect(c);
        ASSERT_TRUE(rect.ok());
        EXPECT_EQ(layout.cell(c).camera.viewportWidth(), static_cast<int>(rect->width));
        EXPECT_EQ(layout.cell(c).camera.viewportHeight(), static_cast<int>(rect->height));
        EXPECT_EQ(layout.cell(c).pixelWidth, static_cast<int>(rect->width));
    }
}

TEST(ViewportLayout, ChangingAKindGivesTheCellAnAppropriateProjection)
{
    ViewportLayout layout;
    layout.setPixelSize(400, 400);

    ASSERT_TRUE(layout.setCellKind(0, ViewKind::Model3D).ok());
    EXPECT_EQ(layout.cell(0).camera.projection(), katana::render::Projection::Perspective);

    ASSERT_TRUE(layout.setCellKind(0, ViewKind::Elevation).ok());
    // A side view is measured off with a scale rule, so it must be orthographic.
    EXPECT_EQ(layout.cell(0).camera.projection(), katana::render::Projection::Orthographic);

    ASSERT_TRUE(layout.setCellKind(0, ViewKind::Plan).ok());
    EXPECT_EQ(layout.cell(0).camera.projection(), katana::render::Projection::Orthographic);
}

TEST(ViewportLayout, EveryLayoutAndViewKindHasAName)
{
    std::set<std::string> names;
    for (LayoutKind kind : kAllLayouts) {
        const std::string name = katana::cad::toString(kind);
        EXPECT_NE(name, "Unknown");
        EXPECT_TRUE(names.insert(name).second) << "duplicate layout name " << name;
    }
    names.clear();
    for (ViewKind kind : {ViewKind::Plan, ViewKind::Model3D, ViewKind::Section,
                          ViewKind::Elevation}) {
        const std::string name = katana::cad::toString(kind);
        EXPECT_NE(name, "Unknown");
        EXPECT_TRUE(names.insert(name).second) << "duplicate view name " << name;
    }
}
