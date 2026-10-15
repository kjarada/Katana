// SURFACE LEGEND and SURFACE PLAN through the interpreter
// (cad/terrain/surface_display_verbs.hpp, docs/surface_display.md "On the
// command line"), on a drawing whose surface is the 10 x 10 square z = x and
// whose style is made here, so every number in a reply is worked out below.

#include <gtest/gtest.h>

#include <memory>
#include <string>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::core::ErrorCode;
using katana::terrain::NamedSurface;
using katana::terrain::SurfaceStore;
using katana::terrain::TinSurface;

std::shared_ptr<const TinSurface> square()
{
    auto tin = TinSurface::create({{0.0, 0.0, 0.0}, {10.0, 0.0, 10.0}, {10.0, 10.0, 10.0}, {0.0, 10.0, 0.0}},
                                  {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(tin.ok());
    return std::make_shared<const TinSurface>(std::move(*tin));
}

// The drawing: the built-in styles and "Test Bands" - plan faces by
// elevation in five bands, triangles drawn, no boundary; in 3D faces by
// slope (banded too: the banding is the plan's and the 3D view's alike).
struct Drawing {
    Document document;
    CommandInterpreter interpreter{document};

    explicit Drawing(bool visible = true)
    {
        katana::cad::SurfaceStyleSet styles = katana::cad::builtInSurfaceStyles();
        SurfaceDisplayStyle bands;
        bands.name = "Test Bands";
        bands.planFaces = SurfaceFaces::Elevation;
        bands.banded = true;
        bands.bands = 5;
        bands.showTriangles = true;
        bands.showBoundary = false;
        bands.faces3d = SurfaceFaces::Slope;
        styles.styles.push_back(bands);
        EXPECT_TRUE(document.setSurfaceStyles(styles).ok());
        SurfaceStore store;
        EXPECT_TRUE(store.add(NamedSurface{"ground", square(), "test", "Test Bands", visible}).ok());
        EXPECT_TRUE(document.setSurfaces(store).ok());
    }

    std::string run(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
};

bool has(const std::string& text, const std::string& part) { return text.find(part) != std::string::npos; }

} // namespace

TEST(SurfaceDisplayVerbs, LegendListsTheBandsOfTheSurfacesPlanStyle)
{
    Drawing drawing;
    const std::string reply = drawing.run("SURFACE LEGEND ground");
    // Five bands of 2 over the surface's own 0 to 10, coloured the terrain
    // ramp at 0, 1/4, 1/2, 3/4 and 1: #28468C (40, 70, 140) first, #F5F5F5
    // (245, 245, 245) last.
    EXPECT_TRUE(has(reply, "legend surface=\"ground\" style=\"Test Bands\" view=plan faces=elevation kind=bands "
                           "entries=5"))
        << reply;
    EXPECT_TRUE(has(reply, "entry index=1 colour=#28468C from=0 to=2 label=\"0 - 2\"")) << reply;
    EXPECT_TRUE(has(reply, "entry index=5 colour=#F5F5F5 from=8 to=10 label=\"8 - 10\"")) << reply;
}

TEST(SurfaceDisplayVerbs, LegendIn3DReadsFaces3dOverTheSurfacesOwnRange)
{
    Drawing drawing;
    // Slope: z = x rises 1 in 1, 100 % on both faces, so the range is 0 to
    // 100 and five bands are 20 wide.
    const std::string reply = drawing.run("SURFACE LEGEND ground 3D");
    EXPECT_TRUE(has(reply, "view=3d faces=slope kind=bands entries=5")) << reply;
    EXPECT_TRUE(has(reply, "entry index=1 colour=#28468C from=0 to=20")) << reply;
}

TEST(SurfaceDisplayVerbs, LegendAsJsonIsOneObjectOfTheSameEntries)
{
    Drawing drawing;
    const std::string reply = drawing.run("SURFACE LEGEND ground JSON");
    EXPECT_EQ(reply.front(), '{');
    EXPECT_TRUE(has(reply, "\"surface\":\"ground\"")) << reply;
    EXPECT_TRUE(has(reply, "\"kind\":\"bands\"")) << reply;
    EXPECT_TRUE(has(reply, "\"colour\":\"#28468C\"")) << reply;
    EXPECT_TRUE(has(reply, "\"label\":\"0 - 2\"")) << reply;
}

TEST(SurfaceDisplayVerbs, PlanSaysWhatThePlanViewDrawsOfTheSurface)
{
    Drawing drawing;
    // The whole surface through 80 x 80 pixels: an eighth of a unit a pixel.
    // Its two triangles' five edges (4 sides, 1 diagonal) are 80 and 113 px
    // long, all stroked; the image is 80 x 80 and every pixel covered once.
    const std::string reply = drawing.run("SURFACE PLAN ground pixels=80,80");
    EXPECT_TRUE(has(reply, "plan surface=\"ground\" style=\"Test Bands\" visible=yes pixels=80,80 "
                           "unitsPerPixel=0.125 triangles=2 inView=2 edges=5 tinted=0 faces=elevation "
                           "image=80x80 covered=6400"))
        << reply;
    EXPECT_TRUE(has(reply, "minor=off major=off")) << reply;
    EXPECT_TRUE(has(reply, "\nscope=surface")) << reply;
}

TEST(SurfaceDisplayVerbs, PlanLooksThroughAnAreaAndSaysWhichOne)
{
    Drawing drawing;
    const std::string reply = drawing.run("SURFACE PLAN ground AREA 0,0,5,5 pixels=50,50");
    // A tenth of a unit a pixel; both triangles reach the corner square.
    EXPECT_TRUE(has(reply, "unitsPerPixel=0.1 triangles=2 inView=2")) << reply;
    EXPECT_TRUE(has(reply, "scope=area area=0,0,5,5")) << reply;
}

TEST(SurfaceDisplayVerbs, AHiddenSurfaceIsStillDescribedAndSaysItIsHidden)
{
    Drawing drawing(false);
    EXPECT_TRUE(has(drawing.run("SURFACE PLAN ground pixels=80,80"), "visible=no"));
}

TEST(SurfaceDisplayVerbs, TheVerbsChangeNothingAndLeaveNoUndoStep)
{
    Drawing drawing;
    const std::size_t steps = drawing.document.history().undoCount();
    const auto revision = drawing.document.modelRevision();
    (void)drawing.run("SURFACE LEGEND ground");
    (void)drawing.run("SURFACE PLAN ground");
    EXPECT_EQ(drawing.document.history().undoCount(), steps);
    EXPECT_EQ(drawing.document.modelRevision(), revision);
}

TEST(SurfaceDisplayVerbs, EveryRefusalSaysWhatWasWrong)
{
    Drawing drawing;
    const auto refused = [&drawing](const std::string& line, ErrorCode code, const std::string& part) {
        auto reply = drawing.interpreter.run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, code) << line << ": " << reply.error().describe();
        EXPECT_TRUE(has(reply.error().describe(), part)) << line << ": " << reply.error().describe();
    };
    refused("SURFACE LEGEND nothing", ErrorCode::NotFound, "nothing");
    refused("SURFACE PLAN nothing", ErrorCode::NotFound, "nothing");
    refused("SURFACE LEGEND", ErrorCode::ParseFailure, "LEGEND");
    refused("SURFACE LEGEND ground SIDEWAYS", ErrorCode::InvalidArgument, "SIDEWAYS");
    refused("SURFACE PLAN ground WHERE TYPE=point", ErrorCode::InvalidArgument, "WHERE");
    refused("SURFACE PLAN ground SELECTION", ErrorCode::InvalidArgument, "AREA");
    refused("SURFACE PLAN ground pixels=0,10", ErrorCode::InvalidArgument, "pixels=");
    refused("SURFACE PLAN ground pixels=wide", ErrorCode::InvalidArgument, "pixels=");
    refused("SURFACE PLAN ground AREA 0,0,5", ErrorCode::ParseFailure, "AREA");
    // Headless there is no plan view: VIEW is refused naming AREA.
    auto view = drawing.interpreter.run("SURFACE PLAN ground VIEW");
    ASSERT_FALSE(view.ok());
    EXPECT_TRUE(has(view.error().describe(), "AREA")) << view.error().describe();
}

TEST(SurfaceDisplayVerbs, HelpNamesBoth)
{
    const std::string help = CommandInterpreter::helpText();
    EXPECT_TRUE(has(help, "SURFACE LEGEND"));
    EXPECT_TRUE(has(help, "SURFACE PLAN"));
}

namespace {

// The window's answer for VIEW: a plan view of `width` x `height` pixels at 8
// pixels a unit about (5, 5), which has the id 7 - or one with no plan
// extent at all, a 3D view's.
katana::cad::ScopeViewProvider planViewOf(double width, double height, bool withFrame = true)
{
    return [=](std::optional<std::uint32_t>) -> katana::core::Result<katana::cad::ScopeView> {
        katana::cad::ViewTransform transform;
        transform.resize(width, height);
        transform.scale = 8.0;
        transform.center = katana::geometry::Point2(5.0, 5.0);
        katana::cad::ScopeView view;
        view.id = 7;
        view.area = transform.visibleWorldBounds();
        if (withFrame) {
            view.frame = transform;
        }
        return view;
    };
}

} // namespace

TEST(SurfaceDisplayVerbs, PlanOfTheViewIsWhatTheViewDrawsAtItsOwnSizeAndScale)
{
    Drawing drawing;
    // 200 x 80 pixels at 8 a unit about (5, 5) shows x from -7.5 to 17.5 and
    // y from 0 to 10: the square, whole, with 7.5 empty units either side. The
    // surface is judged at the view's 8 pixels a unit - unitsPerPixel 1/8 -
    // as the view draws it: 80 x 80 image pixels, all covered (6 400), and the
    // five edges, 80 and 113 px long, stroked. Reframed in the default 1600 x
    // 1000 it was judged at 25 / 1600 = 0.015625 units a pixel, a 640 x 640
    // image of 409 600 pixels - a different picture.
    drawing.interpreter.setScopeContext(planViewOf(200.0, 80.0));
    const std::string reply = drawing.run("SURFACE PLAN ground VIEW");
    EXPECT_TRUE(has(reply, "pixels=200,80 unitsPerPixel=0.125 triangles=2 inView=2 edges=5 tinted=0 "
                           "faces=elevation image=80x80 covered=6400"))
        << reply;
    EXPECT_TRUE(has(reply, "scope=view view=7 area=-7.5,0,17.5,10")) << reply;
    // The same view in the plan verb's own default frame reads differently,
    // which is what the old reply was.
    const std::string fitted = drawing.run("SURFACE PLAN ground VIEW pixels=1600,1000");
    EXPECT_TRUE(has(fitted, "pixels=1600,1000 unitsPerPixel=0.015625 ")) << fitted;
    // pixels= still asks for another frame of the view's area: 25 x 10 units
    // in 80 x 80 pixels is 25 / 80 = 0.3125 units a pixel.
    const std::string asked = drawing.run("SURFACE PLAN ground VIEW pixels=80,80");
    EXPECT_TRUE(has(asked, "pixels=80,80 unitsPerPixel=0.3125 ")) << asked;
}

TEST(SurfaceDisplayVerbs, PlanOfAViewWithNoTransformFitsItsAreaAsBeforeAndEXTENTSIsRefusedByName)
{
    Drawing drawing;
    // A provider that gives the area and no transform: fitted to the default
    // 1600 x 1000 as an AREA is - the 10 units over 1000 px.
    drawing.interpreter.setScopeContext(planViewOf(80.0, 80.0, false));
    EXPECT_TRUE(has(drawing.run("SURFACE PLAN ground VIEW"), "pixels=1600,1000 unitsPerPixel=0.01 "));
    // VIEW EXTENTS has no area to look at: said so, not that the view has no
    // plan area.
    auto extents = drawing.interpreter.run("SURFACE PLAN ground VIEW EXTENTS");
    ASSERT_FALSE(extents.ok());
    EXPECT_EQ(extents.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(has(extents.error().describe(), "EXTENTS")) << extents.error().describe();
    EXPECT_FALSE(has(extents.error().describe(), "no plan area")) << extents.error().describe();
    // A view with no plan extent (a 3D view's) is refused as it was.
    drawing.interpreter.setScopeContext([](std::optional<std::uint32_t>) -> katana::core::Result<katana::cad::ScopeView> {
        katana::cad::ScopeView view;
        view.id = 9;
        return view;
    });
    auto threeD = drawing.interpreter.run("SURFACE PLAN ground VIEW 9");
    ASSERT_FALSE(threeD.ok());
    EXPECT_TRUE(has(threeD.error().describe(), "no plan area")) << threeD.error().describe();
}

TEST(SurfaceDisplayVerbs, PlanOfAnAreaThatIsAPointIsRefusedNotAnsweredWithZeroCounts)
{
    Drawing drawing;
    // The square covers (5, 5). A plan of a point is never worked out, and
    // its reply - every count 0 - was that of a point the surface does not
    // cover.
    for (const char* line : {"SURFACE PLAN ground AREA 5,5,5,5", "SURFACE PLAN ground AREA 500,500,500,500"}) {
        auto reply = drawing.interpreter.run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(has(reply.error().describe(), "AREA has no width and no height")) << reply.error().describe();
    }
    // A line (no width, some height) has a frame.
    EXPECT_TRUE(has(drawing.run("SURFACE PLAN ground AREA 5,5,5,6"), "inView="));
}

TEST(SurfaceDisplayVerbs, LegendOfAWireframeIn3DIsNoColoursForItsMeshIsOneGrey)
{
    Drawing drawing;
    drawing.run("SURFACE STYLE ground Outline");
    // Outline is faces3d=elevation drawn as a grey mesh: it colours no face,
    // so it has no gradient to explain (it said nine entries).
    const std::string reply = drawing.run("SURFACE LEGEND ground 3D");
    EXPECT_TRUE(has(reply, "view=3d faces=none kind=none entries=0")) << reply;
}

TEST(SurfaceDisplayVerbs, LegendOfARangeTheSurfaceRunsPastOpensItsEndBands)
{
    Drawing drawing;
    // z = x from 0 to 10; five bands over 3 to 7 are 0.8 wide: below 3.8 the
    // first colour covers 0 to 3.8, above 6.2 the last covers 6.2 to 10.
    drawing.run("SURFACESTYLE NEW Focus FROM \"Test Bands\" min=3 max=7");
    drawing.run("SURFACE STYLE ground Focus");
    const std::string reply = drawing.run("SURFACE LEGEND ground");
    const std::size_t first = reply.find("entry index=1");
    const std::size_t second = reply.find("entry index=2");
    const std::size_t last = reply.find("entry index=5");
    ASSERT_NE(first, std::string::npos) << reply;
    ASSERT_NE(second, std::string::npos) << reply;
    ASSERT_NE(last, std::string::npos) << reply;
    const std::string firstLine = reply.substr(first, second - first);
    EXPECT_TRUE(has(firstLine, "label=\"< 3.8\"")) << firstLine;
    EXPECT_FALSE(has(firstLine, " from=")) << firstLine;
    const std::string lastLine = reply.substr(last);
    EXPECT_TRUE(has(lastLine, "label=\"6.2 +\"")) << lastLine;
    EXPECT_FALSE(has(lastLine, " to=")) << lastLine;
    const std::string json = drawing.run("SURFACE LEGEND ground JSON");
    EXPECT_TRUE(has(json, "\"from\":null")) << json;
    EXPECT_TRUE(has(json, "\"to\":null")) << json;
}
