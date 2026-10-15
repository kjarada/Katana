// The surface display styles' data (cad/terrain/surface_style.hpp): the
// built-in set the brief names, the style a surface is drawn in, the checks,
// and the versioned JSON form.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/cad/terrain/surface_style.hpp"
#include "surface_test_support.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::Color;

namespace {

std::vector<std::string> namesOf(const SurfaceStyleSet& set)
{
    std::vector<std::string> names;
    for (const SurfaceDisplayStyle& style : set.styles) {
        names.push_back(style.name);
    }
    return names;
}

// A style with every field other than its default, and doubles a text form
// would lose if it were not written in full.
SurfaceDisplayStyle everything()
{
    SurfaceDisplayStyle style;
    style.name = "Every Field";
    style.description = "set, all of it";
    style.showTriangles = true;
    style.triangleColour = Color{1, 2, 3, 255};
    style.showBoundary = false;
    style.boundaryColour = Color{4, 5, 6, 128};
    style.showPoints = true;
    style.pointColour = Color{7, 8, 9, 255};
    style.showBreaklines = true;
    style.breaklineColour = Color{10, 11, 12, 255};
    style.planFaces = SurfaceFaces::Aspect;
    style.planOpacity = 0.1 + 0.2; // 0.30000000000000004
    style.contours.show = true;
    style.contours.interval = 1.0 / 3.0;
    style.contours.majorEvery = 4;
    style.contours.base = -0.125;
    style.contours.minorColour = Color{13, 14, 15, 255};
    style.contours.majorColour = Color{16, 17, 18, 255};
    style.contours.labelMajor = true;
    style.ramp = "diverging";
    style.banded = true;
    style.bands = 7;
    style.stops = {{-1e-9, Color{0, 0, 255, 255}}, {12345.678901234567, Color{255, 0, 0, 255}}};
    style.rangeMin = -3.5;
    style.rangeMax = 1e300;
    style.flatColour = Color{19, 20, 21, 255};
    style.shading = SurfaceStyle::ShadedWithEdges;
    style.faces3d = SurfaceFaces::Flat;
    return style;
}

katana::terrain::NamedSurface surfaceIn(std::string style)
{
    katana::terrain::NamedSurface named;
    named.name = "ground";
    named.surface = katana::cad::surface_testing::tiltedSquare(0.0);
    named.style = std::move(style);
    return named;
}

} // namespace

TEST(SurfaceStyleSets, TheBuiltInSetIsTheSevenStylesInOrderWithStandardTheDefault)
{
    const SurfaceStyleSet set = builtInSurfaceStyles();
    EXPECT_EQ(namesOf(set), (std::vector<std::string>{"Standard", "Triangles", "Contours",
                                                      "Elevation Bands", "Slope Bands",
                                                      "Cut Fill", "Outline"}));
    EXPECT_EQ(set.defaultStyle, "Standard");
    EXPECT_TRUE(checkSurfaceStyleSet(set).ok()) << checkSurfaceStyleSet(set).error().describe();
}

TEST(SurfaceStyleSets, EachBuiltInStyleShowsWhatItIsNamedFor)
{
    const SurfaceStyleSet set = builtInSurfaceStyles();
    const SurfaceDisplayStyle& standard = *set.find("Standard");
    EXPECT_TRUE(standard.showBoundary);
    EXPECT_FALSE(standard.showTriangles);
    EXPECT_EQ(standard.faces3d, SurfaceFaces::Elevation);
    EXPECT_NE(standard.shading, SurfaceStyle::Wireframe);

    const SurfaceDisplayStyle& triangles = *set.find("Triangles");
    EXPECT_TRUE(triangles.showTriangles);
    EXPECT_TRUE(triangles.showBoundary);

    const SurfaceDisplayStyle& contours = *set.find("Contours");
    EXPECT_TRUE(contours.contours.show);
    EXPECT_EQ(contours.contours.interval, 1.0);
    EXPECT_EQ(contours.contours.majorEvery, 5);
    EXPECT_TRUE(contours.contours.labelMajor);
    EXPECT_TRUE(contours.showBoundary);

    const SurfaceDisplayStyle& elevation = *set.find("Elevation Bands");
    EXPECT_EQ(elevation.planFaces, SurfaceFaces::Elevation);
    EXPECT_TRUE(elevation.banded);
    EXPECT_EQ(elevation.bands, 10);
    EXPECT_EQ(elevation.ramp, "terrain");

    const SurfaceDisplayStyle& slope = *set.find("Slope Bands");
    EXPECT_EQ(slope.planFaces, SurfaceFaces::Slope);
    EXPECT_EQ(slope.ramp, "slope");
    std::vector<double> stops;
    for (const ColourStop& stop : slope.stops) {
        stops.push_back(stop.value);
    }
    EXPECT_EQ(stops, (std::vector<double>{0.0, 2.0, 5.0, 10.0, 20.0, 33.0}));

    // Symmetric about 0: every stop has its negative, cut (below 0) bluer
    // than it is red and fill (above) redder than it is blue.
    const SurfaceDisplayStyle& cutFill = *set.find("Cut Fill");
    EXPECT_EQ(cutFill.ramp, "diverging");
    ASSERT_FALSE(cutFill.stops.empty());
    for (const ColourStop& stop : cutFill.stops) {
        const bool mirrored = std::ranges::any_of(
            cutFill.stops, [&](const ColourStop& other) { return other.value == -stop.value; });
        EXPECT_TRUE(mirrored) << stop.value;
        if (stop.value < 0.0) {
            EXPECT_GT(stop.colour.b, stop.colour.r) << stop.value;
        } else if (stop.value > 0.0) {
            EXPECT_GT(stop.colour.r, stop.colour.b) << stop.value;
        }
    }

    const SurfaceDisplayStyle& outline = *set.find("Outline");
    EXPECT_TRUE(outline.showBoundary);
    EXPECT_FALSE(outline.showTriangles);
    EXPECT_FALSE(outline.contours.show);
    EXPECT_EQ(outline.shading, SurfaceStyle::Wireframe);
}

TEST(SurfaceStyleSets, AStyleIsFoundByItsNameInAnyCase)
{
    const SurfaceStyleSet set = builtInSurfaceStyles();
    ASSERT_NE(set.find("cut fill"), nullptr);
    EXPECT_EQ(set.find("cut fill")->name, "Cut Fill");
    EXPECT_EQ(set.find("Nothing"), nullptr);
}

TEST(SurfaceStyleSets, ASurfaceIsDrawnInItsOwnStyleElseTheDefaultElseStandard)
{
    SurfaceStyleSet set = builtInSurfaceStyles();
    EXPECT_EQ(styleOf(set, surfaceIn("contours")).name, "Contours");
    EXPECT_EQ(styleOf(set, surfaceIn("")).name, "Standard");
    set.defaultStyle = "Outline";
    EXPECT_EQ(styleOf(set, surfaceIn("")).name, "Outline");
    EXPECT_EQ(styleOf(set, surfaceIn("since deleted")).name, "Outline");
    // A set with neither: the built-in Standard, never nothing.
    const SurfaceStyleSet empty;
    EXPECT_EQ(styleOf(empty, surfaceIn("Contours")).name, "Standard");
}

TEST(SurfaceStyleSets, TheJsonFormGivesBackEveryFieldExactly)
{
    SurfaceStyleSet set = builtInSurfaceStyles();
    set.styles.push_back(everything());
    set.defaultStyle = "Every Field";
    const std::string text = surfaceStylesToJson(set);
    auto back = surfaceStylesFromJson(text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_TRUE(*back == set);
    const SurfaceDisplayStyle& read = *back->find("Every Field");
    EXPECT_EQ(read.planOpacity, 0.1 + 0.2);
    EXPECT_EQ(read.contours.interval, 1.0 / 3.0);
    EXPECT_EQ(read.boundaryColour.a, 128);
    // The same set is the same text.
    EXPECT_EQ(surfaceStylesToJson(*back), text);
}

TEST(SurfaceStyleSets, ANewerVersionIsUnsupportedAndAnythingElseAParseFailure)
{
    EXPECT_EQ(surfaceStylesFromJson(R"({"format":"katana-surface-styles","version":2,"styles":[]})")
                  .error()
                  .code,
              ErrorCode::Unsupported);
    EXPECT_EQ(surfaceStylesFromJson("not json").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(surfaceStylesFromJson(R"({"format":"katana-sheets","version":1})").error().code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(surfaceStylesFromJson(
                  R"({"format":"katana-surface-styles","version":1,"styles":[{"name":"A","faces":"stripes"}],"default":"A"})")
                  .error()
                  .code,
              ErrorCode::ParseFailure);
    // Read, but not a set that can be drawn: the checks' refusal.
    EXPECT_EQ(surfaceStylesFromJson(
                  R"({"format":"katana-surface-styles","version":1,"styles":[{"name":"A","opacity":2}],"default":"A"})")
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
}

TEST(SurfaceStyleSets, AStyleThatCannotBeDrawnIsRefusedNamingTheField)
{
    const auto refusedFor = [](auto change) {
        SurfaceDisplayStyle style;
        style.name = "Probe";
        change(style);
        const auto status = checkSurfaceStyle(style);
        EXPECT_FALSE(status.ok());
        return status.ok() ? std::string() : status.error().context;
    };
    static constexpr double nan = std::numeric_limits<double>::quiet_NaN();
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.name.clear(); }), "field=name");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.name = "say \"hi\""; }), "field=name");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.contours.interval = 0.0; }),
              "field=interval");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.contours.interval = nan; }),
              "field=interval");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.contours.majorEvery = -1; }),
              "field=major");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.planOpacity = 1.5; }), "field=opacity");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.bands = 1; }), "field=bands");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.stops = {{2.0, {}}, {1.0, {}}}; }),
              "field=stops");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.stops = {{1.0, {}}, {1.0, {}}}; }),
              "field=stops");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.stops = {{nan, {}}}; }), "field=stops");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) {
                  s.rangeMin = 5.0;
                  s.rangeMax = 5.0;
              }),
              "field=min");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.rangeMax = nan; }), "field=min");
    EXPECT_EQ(refusedFor([](SurfaceDisplayStyle& s) { s.ramp = "no such ramp"; }), "field=ramp");
    // The degenerate style that is fine: a single stop, an open range.
    SurfaceDisplayStyle fine;
    fine.name = "Fine";
    fine.stops = {{0.0, {}}};
    fine.rangeMin = 0.0;
    EXPECT_TRUE(checkSurfaceStyle(fine).ok());
}

TEST(SurfaceStyleSets, ASetNeedsAStyleNoTwoOfOneNameAndADefaultAmongThem)
{
    EXPECT_FALSE(checkSurfaceStyleSet(SurfaceStyleSet{}).ok());
    SurfaceStyleSet twice = builtInSurfaceStyles();
    twice.styles.push_back(twice.styles.front());
    twice.styles.back().name = "STANDARD";
    EXPECT_EQ(checkSurfaceStyleSet(twice).error().code, ErrorCode::InvalidArgument);
    SurfaceStyleSet lost = builtInSurfaceStyles();
    lost.defaultStyle = "Gone";
    EXPECT_EQ(checkSurfaceStyleSet(lost).error().code, ErrorCode::InvalidArgument);
}

TEST(SurfaceStyleSets, TheWordsOfFacesAndShadingReadBackInAnyCase)
{
    for (const SurfaceFaces faces : {SurfaceFaces::None, SurfaceFaces::Flat,
                                     SurfaceFaces::Elevation, SurfaceFaces::Slope,
                                     SurfaceFaces::Aspect}) {
        EXPECT_EQ(parseSurfaceFaces(surfaceFacesWord(faces)), faces);
    }
    for (const SurfaceStyle shading : {SurfaceStyle::Automatic, SurfaceStyle::Shaded,
                                       SurfaceStyle::ShadedWithEdges, SurfaceStyle::Wireframe,
                                       SurfaceStyle::Hidden}) {
        EXPECT_EQ(parseSurfaceShading(surfaceShadingWord(shading)), shading);
    }
    EXPECT_EQ(parseSurfaceShading("EDGES"), SurfaceStyle::ShadedWithEdges);
    EXPECT_FALSE(parseSurfaceFaces("stripes").has_value());
}

namespace {

// The field a style that cannot be drawn is refused for ("" when it is drawn).
template <typename Change> std::string fieldRefusedFor(Change change)
{
    SurfaceDisplayStyle style;
    style.name = "Probe";
    change(style);
    const auto status = checkSurfaceStyle(style);
    return status.ok() ? std::string() : status.error().context;
}

} // namespace

TEST(SurfaceStyleSets, TheBandCountIsHeldToTwoThroughTheMaximumNotLeftToGrowWithoutBound)
{
    // The 3D view cuts each triangle at every band edge it crosses, so its
    // geometry grew by about 643 triangles a band on the 21 182-triangle
    // sample terrain (28 078 at 10 bands, 3 235 844 at 5 000, 32 163 345 at
    // 50 000): a count the verb allowed up to 1 000 000 built tens of
    // gigabytes. One number, 100, the form's own range, for the line, the
    // form and a project file.
    ASSERT_EQ(kMaximumSurfaceBands, 100);
    EXPECT_EQ(fieldRefusedFor([](SurfaceDisplayStyle& s) { s.bands = 2; }), "");
    EXPECT_EQ(fieldRefusedFor([](SurfaceDisplayStyle& s) { s.bands = 100; }), "");
    EXPECT_EQ(fieldRefusedFor([](SurfaceDisplayStyle& s) { s.bands = 101; }), "field=bands");
    EXPECT_EQ(fieldRefusedFor([](SurfaceDisplayStyle& s) { s.bands = 1'000'000; }), "field=bands");
    EXPECT_EQ(fieldRefusedFor([](SurfaceDisplayStyle& s) { s.bands = std::numeric_limits<int>::max(); }),
              "field=bands");
    EXPECT_EQ(fieldRefusedFor([](SurfaceDisplayStyle& s) { s.bands = 1; }), "field=bands");
    EXPECT_EQ(fieldRefusedFor([](SurfaceDisplayStyle& s) { s.bands = -3; }), "field=bands");
    // The refusal says the range it holds.
    SurfaceDisplayStyle many;
    many.name = "Many";
    many.bands = 101;
    const auto status = checkSurfaceStyle(many);
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("2 to 100"), std::string::npos) << status.error().describe();
}

TEST(SurfaceStyleSets, AListOfStopsIsHeldToTheSameMaximumBecauseABandedOneIsABandAStop)
{
    const auto stopsOf = [](int count) {
        std::vector<ColourStop> stops;
        for (int i = 0; i < count; ++i) {
            stops.push_back({static_cast<double>(i), Color{0, 0, 0, 255}});
        }
        return stops;
    };
    EXPECT_EQ(fieldRefusedFor([&](SurfaceDisplayStyle& s) { s.stops = stopsOf(100); }), "");
    EXPECT_EQ(fieldRefusedFor([&](SurfaceDisplayStyle& s) { s.stops = stopsOf(101); }), "field=stops");
}

TEST(SurfaceStyleSets, AProjectFileWithAMillionBandsIsRefusedWhenItIsRead)
{
    SurfaceStyleSet set = builtInSurfaceStyles();
    std::string text = surfaceStylesToJson(set);
    const std::string ten = "\"bands\":10";
    const std::size_t at = text.find(ten);
    ASSERT_NE(at, std::string::npos) << text;
    text.replace(at, ten.size(), "\"bands\":1000000");
    const auto read = surfaceStylesFromJson(text);
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(read.error().describe().find("bands"), std::string::npos) << read.error().describe();
    // The same text with a count within the maximum reads.
    std::string fine = surfaceStylesToJson(set);
    fine.replace(fine.find(ten), ten.size(), "\"bands\":100");
    EXPECT_TRUE(surfaceStylesFromJson(fine).ok());
}
