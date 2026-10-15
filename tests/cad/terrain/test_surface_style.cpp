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
#include "katana/core/text_encoding.hpp"
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

TEST(SurfaceStyleSets, TextThatIsNotUtf8IsRefusedAndTheMessageDoesNotCarryItsBytes)
{
    // Windows-1252 as katana_cli's ANSI arguments carry it: 0xB0 is the degree
    // sign and 0xE9 e-acute. Neither is UTF-8 (a continuation byte on its own; a
    // three-byte lead with nothing after it), and the set is written as JSON.
    SurfaceDisplayStyle named;
    named.name = "Pente\xB0";
    const auto badName = checkSurfaceStyle(named);
    ASSERT_FALSE(badName.ok());
    EXPECT_EQ(badName.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(badName.error().context, "field=name");
    // Not echoed: a message holding the bytes would reach the same writers.
    EXPECT_TRUE(katana::core::isValidUtf8(badName.error().describe())) << badName.error().describe();

    SurfaceDisplayStyle described;
    described.name = "Fine";
    described.description = "caf\xE9";
    const auto badDescription = checkSurfaceStyle(described);
    ASSERT_FALSE(badDescription.ok());
    EXPECT_EQ(badDescription.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(badDescription.error().context, "field=description");
    EXPECT_TRUE(katana::core::isValidUtf8(badDescription.error().describe()));

    // The same text as UTF-8 - "Pente" + U+00B0, "caf" + U+00E9 - is fine, and so
    // are letters from other scripts.
    SurfaceDisplayStyle fine;
    fine.name = "Pente\xC2\xB0 \xE6\xB5\x8B";
    fine.description = "caf\xC3\xA9";
    EXPECT_TRUE(checkSurfaceStyle(fine).ok());
}

TEST(SurfaceStyleSets, ADescriptionACommandLineCannotCarryIsRefused)
{
    // SURFACESTYLE INFO prints the description for the person to type back; the
    // tokenizer ends a word at a quote and a line at a break.
    const auto refusedFor = [](const std::string& description) {
        SurfaceDisplayStyle style;
        style.name = "Probe";
        style.description = description;
        const auto status = checkSurfaceStyle(style);
        return status.ok() ? std::string("accepted") : status.error().context;
    };
    EXPECT_EQ(refusedFor("say \"hi\""), "field=description");
    EXPECT_EQ(refusedFor("two\nlines"), "field=description");
    EXPECT_EQ(refusedFor("one\rtwo"), "field=description");
    EXPECT_EQ(refusedFor(""), "accepted");
    EXPECT_EQ(refusedFor("C:\\dir 'quoted' = fine"), "accepted");
}

TEST(SurfaceStyleSets, TheJsonWriterNeverThrowsForASetBuiltByHandWithBadBytes)
{
    // The Document only holds a checked set, so this is the writer's own
    // guarantee: nlohmann's default throws type_error.316 and the verbs, the
    // resource and the save are not ready for an exception.
    SurfaceStyleSet set = builtInSurfaceStyles();
    SurfaceDisplayStyle bad;
    bad.name = "Pente\xB0";
    bad.description = "caf\xE9";
    set.styles.push_back(bad);
    std::string text;
    EXPECT_NO_THROW(text = surfaceStylesToJson(set));
    EXPECT_TRUE(katana::core::isValidUtf8(text));
    // U+FFFD, EF BF BD, where the bad byte was; the letters round it stay.
    EXPECT_NE(text.find("Pente\xEF\xBF\xBD"), std::string::npos) << text;
    EXPECT_NE(text.find("caf\xEF\xBF\xBD"), std::string::npos) << text;
}

TEST(SurfaceStyleSets, AFormatThatIsNotAStringIsAParseFailureNotAnException)
{
    // value("format", "") throws type_error.302 for a number, null, an object or
    // an array; the header promises ParseFailure for text that is not the form.
    for (const std::string text :
         {R"({"format":5,"version":1,"styles":[]})", R"({"format":null,"version":1,"styles":[]})",
          R"({"format":{},"version":1,"styles":[]})", R"({"format":[],"version":1,"styles":[]})",
          R"({"format":true,"version":1,"styles":[]})", R"({"version":1,"styles":[]})",
          R"([])", R"(null)", R"(7)", R"("katana-surface-styles")"}) {
        try {
            const auto read = surfaceStylesFromJson(text);
            ASSERT_FALSE(read.ok()) << text;
            EXPECT_EQ(read.error().code, ErrorCode::ParseFailure) << text;
        } catch (const std::exception& thrown) {
            ADD_FAILURE() << text << " threw " << thrown.what();
        }
    }
    // The right type with the right word still reads: the guard is not a refusal
    // of everything.
    EXPECT_TRUE(surfaceStylesFromJson(surfaceStylesToJson(builtInSurfaceStyles())).ok());
}

TEST(SurfaceStyleSets, TextWithABadByteInItsJsonIsAParseFailureNotAnException)
{
    // A project whose style text holds Windows-1252 (0xE9) is no JSON: the parser
    // refuses the string, so the set never reaches the Document and its save.
    const std::string bad = std::string(R"({"format":"katana-surface-styles","version":1,)"
                                        R"("default":"A","styles":[{"name":"A","description":"caf)") +
                            "\xE9" + R"("}]})";
    try {
        const auto read = surfaceStylesFromJson(bad);
        ASSERT_FALSE(read.ok());
        EXPECT_EQ(read.error().code, ErrorCode::ParseFailure);
    } catch (const std::exception& thrown) {
        ADD_FAILURE() << "threw " << thrown.what();
    }
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
