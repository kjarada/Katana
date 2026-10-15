// The 3D view's legend of a surface drawn in a display style
// (render_view_widget.cpp's drawLegend, docs/surface_display.md "In 3D"): the
// style's own colours, drawn with the plan view's legend panel, where a
// surface with no style has the one elevation ramp's bar.

#include <gtest/gtest.h>

#include <QImage>
#include <QMainWindow>

#include <string>
#include <vector>

#include "katana/cad/scene.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

namespace {

using katana::cad::Document;
using katana::cad::SceneSurface;
using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::cad::ViewKind;
using katana::qt::DrawnLegend;
using katana::qt::RenderViewWidget;
using katana::qt::ViewWorkspace;
using katana::qt::test::paint;
using katana::qt::test::processEvents;

// Whether `image` has a pixel of exactly `colour` in its top-right quarter,
// where the legend is drawn. A legend swatch is the band's colour unlit; the
// surface's own faces are lit (a flat face takes 0.865 of its colour under
// the default sun), so an exact match there is the legend's.
bool cornerHas(const QImage& image, QRgb colour)
{
    for (int y = 0; y < image.height() / 2; ++y) {
        for (int x = image.width() / 2; x < image.width(); ++x) {
            if ((image.pixel(x, y) & 0x00ffffffu) == (colour & 0x00ffffffu)) {
                return true;
            }
        }
    }
    return false;
}

} // namespace

TEST(SurfaceLegend3D, ASurfaceInABandedStyleShowsItsBandsInTheThreeDViewsLegend)
{
    Document document;
    QMainWindow window;
    auto* views = new ViewWorkspace(document, &window);
    window.setCentralWidget(views);
    window.resize(900, 700);
    window.show();
    processEvents();

    katana::terrain::TinInput input;
    input.points = {{0.0, 0.0, 10.0}, {100.0, 0.0, 12.0}, {100.0, 100.0, 14.0},
                    {0.0, 100.0, 11.0}, {50.0, 50.0, 13.0}};
    auto built = katana::terrain::buildTin(input);
    ASSERT_TRUE(built.ok()) << built.error().describe();
    std::vector<SceneSurface> surfaces(1);
    surfaces.front().name = "Ground";
    surfaces.front().surface = &built->surface;
    SurfaceDisplayStyle style;
    style.name = "Bands";
    style.faces3d = SurfaceFaces::Elevation;
    style.banded = true;
    style.bands = 4;
    katana::cad::applySurfaceDisplay(surfaces.front(), style);
    views->setSurfaces(&surfaces);

    const auto& model = views->openView(ViewKind::Model3D);
    processEvents();
    RenderViewWidget* render = views->renderView(model.id);
    ASSERT_NE(render, nullptr);
    render->resize(600, 450);
    paint(*render);
    ASSERT_EQ(render->sceneLayers().legends.size(), 1u);
    EXPECT_EQ(render->sceneLayers().legends[0].legend.entries.size(), 4u);
    EXPECT_EQ(render->sceneLayers().legends[0].style, "Bands");
    ASSERT_EQ(render->drawnLegends().size(), 1u);
    EXPECT_EQ(render->drawnLegends()[0].caption, "Ground - Bands");
    // Band 0 is the terrain ramp's start, (40, 70, 140), its swatch in the
    // legend panel at the view's top-right.
    EXPECT_TRUE(cornerHas(render->grab().toImage(), qRgb(40, 70, 140)));
}

namespace {

// One 3D view over `surfaces`, painted at 600 x `heightPixels`.
struct LegendView {
    Document document;
    QMainWindow window;
    ViewWorkspace* views = nullptr;
    RenderViewWidget* render = nullptr;

    LegendView(std::vector<SceneSurface>& surfaces, int heightPixels)
    {
        views = new ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        window.resize(900, 700);
        window.show();
        processEvents();
        views->setSurfaces(&surfaces);
        const auto& model = views->openView(ViewKind::Model3D);
        processEvents();
        render = views->renderView(model.id);
        if (render != nullptr) {
            render->resize(600, heightPixels);
            paint(*render);
        }
    }
};

// A TIN over a 100 x 100 square with a hump, whose relief gives a style
// something to band.
auto hill()
{
    katana::terrain::TinInput input;
    input.points = {{0.0, 0.0, 10.0}, {100.0, 0.0, 12.0}, {100.0, 100.0, 14.0},
                    {0.0, 100.0, 11.0}, {50.0, 50.0, 13.0}};
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << (built.ok() ? "" : built.error().describe());
    return std::move(*built);
}

SurfaceDisplayStyle bandsStyle(const std::string& name, SurfaceFaces faces, int bands)
{
    SurfaceDisplayStyle style;
    style.name = name;
    style.faces3d = faces;
    style.banded = true;
    style.bands = bands;
    return style;
}

} // namespace

TEST(SurfaceLegend3D, EachVisibleStyledSurfaceHasItsLegendStackedInDrawingOrderAndNoOtherHasOne)
{
    const auto built = hill();
    // In drawing order: "Ground" in elevation bands, "Hidden" in the same but
    // not visible, "Plain" with no display style (the one ramp's bar), and
    // "Design" in slope bands. Two legends, Ground's above Design's.
    std::vector<SceneSurface> surfaces(4);
    for (SceneSurface& item : surfaces) {
        item.surface = &built.surface;
    }
    surfaces[0].name = "Ground";
    katana::cad::applySurfaceDisplay(surfaces[0], bandsStyle("Bands", SurfaceFaces::Elevation, 4));
    surfaces[1].name = "Hidden";
    katana::cad::applySurfaceDisplay(surfaces[1], bandsStyle("Bands", SurfaceFaces::Elevation, 4));
    surfaces[1].visible = false;
    surfaces[2].name = "Plain";
    surfaces[3].name = "Design";
    katana::cad::applySurfaceDisplay(surfaces[3], bandsStyle("Steep", SurfaceFaces::Slope, 3));

    LegendView view(surfaces, 450);
    ASSERT_NE(view.render, nullptr);
    const std::vector<DrawnLegend>& drawn = view.render->drawnLegends();
    ASSERT_EQ(drawn.size(), 2u);
    EXPECT_EQ(drawn[0].caption, "Ground - Bands");
    EXPECT_EQ(drawn[1].caption, "Design - Steep");
    // Down the top-right corner, 16 px in: one panel above the other with a
    // gap between, neither over the view's edge, their right edges together.
    EXPECT_DOUBLE_EQ(drawn[0].panel.top(), 16.0);
    EXPECT_DOUBLE_EQ(drawn[0].panel.right(), view.render->width() - 16.0);
    EXPECT_DOUBLE_EQ(drawn[1].panel.right(), view.render->width() - 16.0);
    EXPECT_LT(drawn[0].panel.bottom(), drawn[1].panel.top());
    EXPECT_LE(drawn[1].panel.bottom(), view.render->height() - 16.0);
    // Each panel is its own style's colours, unlit, in the corner: band 0 of
    // the elevation bands is the terrain ramp's start, (40, 70, 140), and of
    // the slope bands the slope ramp's, (0, 255, 38).
    const QImage image = view.render->grab().toImage();
    EXPECT_TRUE(cornerHas(image, qRgb(40, 70, 140)));
    EXPECT_TRUE(cornerHas(image, qRgb(0, 255, 38)));
    EXPECT_EQ(view.render->sceneLayers().legends.size(), 2u);
}

TEST(SurfaceLegend3D, WithNoSurfaceInAStyleThereIsNoLegendAndTheOneRampKeepsItsBar)
{
    const auto built = hill();
    std::vector<SceneSurface> surfaces(1);
    surfaces[0].name = "Ground";
    surfaces[0].surface = &built.surface;
    LegendView view(surfaces, 450);
    ASSERT_NE(view.render, nullptr);
    EXPECT_TRUE(view.render->drawnLegends().empty());
    EXPECT_TRUE(view.render->sceneLayers().legends.empty());
    EXPECT_TRUE(view.render->sceneLayers().hasRamp());
}

TEST(SurfaceLegend3D, MoreLegendsThanTheViewHoldsAreLeftOutWholeNotCutOffOrOverlapped)
{
    // Eight surfaces in banded styles, each legend a caption, a title and four
    // rows - well over 80 px - in a view 300 px high with 16 px margins:
    // room for the first few, so the later ones are left out, whole.
    const auto built = hill();
    std::vector<SceneSurface> surfaces(8);
    for (std::size_t i = 0; i < surfaces.size(); ++i) {
        surfaces[i].name = "S" + std::to_string(i + 1);
        surfaces[i].surface = &built.surface;
        katana::cad::applySurfaceDisplay(surfaces[i], bandsStyle("Bands", SurfaceFaces::Elevation, 4));
    }
    LegendView view(surfaces, 300);
    ASSERT_NE(view.render, nullptr);
    const std::vector<DrawnLegend>& drawn = view.render->drawnLegends();
    ASSERT_GE(drawn.size(), 1u);
    EXPECT_LT(drawn.size(), surfaces.size());
    for (std::size_t i = 0; i < drawn.size(); ++i) {
        EXPECT_EQ(drawn[i].caption, "S" + std::to_string(i + 1) + " - Bands") << "in drawing order";
        if (i > 0) {
            EXPECT_LT(drawn[i - 1].panel.bottom(), drawn[i].panel.top());
            EXPECT_LE(drawn[i].panel.bottom(), view.render->height() - 16.0) << "cut by the edge";
        }
    }
    // The scene still holds all eight: it is the view that has no more room.
    EXPECT_EQ(view.render->sceneLayers().legends.size(), surfaces.size());
}
