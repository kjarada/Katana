// The 3D view's legend of a surface drawn in a display style
// (render_view_widget.cpp's drawLegend, docs/surface_display.md "In 3D"): the
// style's own colours, drawn with the plan view's legend panel, where a
// surface with no style has the one elevation ramp's bar.

#include <gtest/gtest.h>

#include <QImage>
#include <QMainWindow>

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
    ASSERT_TRUE(render->sceneLayers().legend.has_value());
    EXPECT_EQ(render->sceneLayers().legend->entries.size(), 4u);
    EXPECT_EQ(render->sceneLayers().legendStyle, "Bands");
    // Band 0 is the terrain ramp's start, (40, 70, 140), its swatch in the
    // legend panel at the view's top-right.
    EXPECT_TRUE(cornerHas(render->grab().toImage(), qRgb(40, 70, 140)));
}
