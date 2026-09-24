// One answer to what a linetype or symbol NAME draws
// (include/katana/cad/style_resolver.hpp): the viewport, the plot and every
// preview ask here, so these are the rules all of them draw by.
//
// Every definition is built inline; nothing depends on the gitignored
// reference files. Expected values are worked by hand in the comments.

#include <gtest/gtest.h>

#include <cmath>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/cad/style_resolver.hpp"

using katana::cad::DashOptions;
using katana::cad::DefinitionCache;
using katana::cad::LinetypeKind;
using katana::cad::StyleDrawing;
using katana::cad::StyleSamplePath;
using katana::cad::SymbolKind;
using katana::entity::LineStyle;
using katana::entity::Linetype;
using katana::entity::LinetypeElement;
using katana::entity::Model;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::Style;
using katana::entity::StyleLibrary;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

Stroke move(double x, double y)
{
    return Stroke{StrokeOp::Move, Point2(x, y)};
}

Stroke draw(double x, double y)
{
    return Stroke{StrokeOp::Draw, Point2(x, y)};
}

LineStyle definition(std::string name, std::vector<Stroke> strokes, bool atVertices = false)
{
    LineStyle style;
    style.name = std::move(name);
    style.strokes = std::move(strokes);
    style.atVertices = atVertices;
    return style;
}

StyleLibrary libraryOf(std::vector<LineStyle> definitions)
{
    StyleLibrary library;
    for (LineStyle& style : definitions) {
        EXPECT_TRUE(library.add(std::move(style)));
    }
    return library;
}

Linetype dashed(std::string name, std::vector<double> lengths)
{
    Linetype linetype;
    linetype.name = std::move(name);
    for (const double length : lengths) {
        linetype.pattern.push_back(LinetypeElement{length});
    }
    return linetype;
}

double strokeLength(const katana::cad::StyleStroke& stroke)
{
    double total = 0.0;
    for (std::size_t i = 1; i < stroke.path.vertices.size(); ++i) {
        total += stroke.path.vertices[i - 1].distanceTo(stroke.path.vertices[i]);
    }
    return total;
}

} // namespace

TEST(StyleResolver, ALibraryDefinitionBeatsTheBuiltInShapeOfTheSameName)
{
    // A library that defines "circle" as a single dash: the library is what
    // the drawing was customised with, so it answers first.
    const StyleLibrary library = libraryOf({definition("circle", {move(0, 0), draw(2, 0)})});
    const auto resolved = katana::cad::resolveSymbol(library, "circle");
    EXPECT_EQ(resolved.kind, SymbolKind::LibraryDefinition);
    ASSERT_NE(resolved.definition, nullptr);
    EXPECT_EQ(resolved.definition->name, "circle");

    // Drawn at (10, 20) at its own scale: the dash from (10, 20) to (12, 20),
    // not the built-in's 24-chord ring.
    const StyleDrawing drawing =
        katana::cad::pointSymbolDrawing(library, "circle", Point2(10, 20), 0.0, 0.0, 1.0, 5.0);
    ASSERT_EQ(drawing.strokes.size(), 1u);
    EXPECT_EQ(drawing.strokes[0].path.vertices,
              (std::vector<Point2>{Point2(10, 20), Point2(12, 20)}));
}

TEST(StyleResolver, AnUnknownSymbolFallsBackToABuiltInShapeAndSaysSo)
{
    const StyleLibrary empty;
    // "SEWR Manhole Cover" is a real library name no library here defines; its
    // words suggest the manhole, and the caller is told it is a guess.
    const auto guessed = katana::cad::resolveSymbol(empty, "SEWR Manhole Cover");
    EXPECT_EQ(guessed.kind, SymbolKind::BuiltInFallback);
    EXPECT_EQ(guessed.builtIn, "manhole");
    EXPECT_EQ(guessed.definition, nullptr);
    // A built-in name is itself, and not a guess.
    const auto exact = katana::cad::resolveSymbol(empty, "cross");
    EXPECT_EQ(exact.kind, SymbolKind::BuiltIn);
    EXPECT_EQ(exact.builtIn, "cross");
    // No name is no symbol: the plain point mark.
    EXPECT_EQ(katana::cad::resolveSymbol(empty, "").kind, SymbolKind::None);
    EXPECT_TRUE(katana::cad::pointSymbolDrawing(empty, "", Point2(0, 0), 0, 0, 1, 1).empty());
}

TEST(StyleResolver, ABuiltInShapeWithNoSizeTakesTheCallersFallbackHalfWidth)
{
    const StyleLibrary empty;
    // A cross is the square's two diagonals. With size 0 the half-width is
    // the fallback, 3: about (10, 20) the first diagonal runs (7, 17) to
    // (13, 23).
    const StyleDrawing fallback =
        katana::cad::pointSymbolDrawing(empty, "cross", Point2(10, 20), 0.0, 0.0, 1.0, 3.0);
    ASSERT_EQ(fallback.strokes.size(), 2u);
    EXPECT_EQ(fallback.strokes[0].path.vertices.front(), Point2(7, 17));
    EXPECT_EQ(fallback.strokes[0].path.vertices.back(), Point2(13, 23));
    EXPECT_EQ(fallback.strokes[0].pen, "") << "a built-in shape is in the entity's own pen";
    // A size is a WIDTH: 4 wide is 2 either side, (8, 18) to (12, 22).
    const StyleDrawing sized =
        katana::cad::pointSymbolDrawing(empty, "cross", Point2(10, 20), 4.0, 0.0, 1.0, 3.0);
    EXPECT_EQ(sized.strokes[0].path.vertices.front(), Point2(8, 18));
    EXPECT_EQ(sized.strokes[0].path.vertices.back(), Point2(12, 22));
}

TEST(StyleResolver, ALibraryLinestyleWinsOverAModelLinetypeOfItsNameAndTheCollisionIsReported)
{
    Model model;
    ASSERT_TRUE(model.linetypes.add(dashed("WATR Main", {1.0, -0.5})));
    const StyleLibrary library =
        libraryOf({definition("WATR Main", {move(0, 0), draw(3, 0), move(5, 0)})});

    const auto both = katana::cad::resolveLinetype(model, library, "WATR Main");
    EXPECT_EQ(both.kind, LinetypeKind::LibraryDefinition);
    ASSERT_NE(both.definition, nullptr);
    EXPECT_EQ(both.linetype, nullptr) << "the library's strokes, with no dash pattern on them";
    EXPECT_TRUE(both.collision);

    const auto modelOnly = katana::cad::resolveLinetype(model, StyleLibrary{}, "WATR Main");
    EXPECT_EQ(modelOnly.kind, LinetypeKind::ModelLinetype);
    ASSERT_NE(modelOnly.linetype, nullptr);
    EXPECT_FALSE(modelOnly.collision);

    const auto nobody = katana::cad::resolveLinetype(model, library, "Nothing Holds This");
    EXPECT_EQ(nobody.kind, LinetypeKind::Solid);
    EXPECT_FALSE(nobody.collision);
}

TEST(StyleResolver, AVertexSymbolIsNeverALinePattern)
{
    // D8: a `mode vertex` definition named as a linetype does not become a
    // pattern. The line is the model linetype of that name if there is one -
    // a collision worth reporting - and solid otherwise.
    const StyleLibrary library =
        libraryOf({definition("Post", {move(-1, 0), draw(1, 0)}, /*atVertices=*/true)});
    Model model;
    EXPECT_EQ(katana::cad::resolveLinetype(model, library, "Post").kind, LinetypeKind::Solid);
    ASSERT_TRUE(model.linetypes.add(dashed("Post", {1.0, -1.0})));
    const auto resolved = katana::cad::resolveLinetype(model, library, "Post");
    EXPECT_EQ(resolved.kind, LinetypeKind::ModelLinetype);
    EXPECT_TRUE(resolved.collision);
}

TEST(StyleResolver, ALineWhoseLinetypeNamesItsOwnSymbolIsAPlainLine)
{
    // What an archive import writes for a symbol string: name, linetype and
    // symbol all the symbol's name. The definition is NOT `mode vertex` - as
    // 147 of the symbols the reference survey code files use are not - and it used to
    // be laid along the line as a pattern.
    const StyleLibrary library =
        libraryOf({definition("CULT Fence Post", {move(0, 0), draw(0, 1)})});
    const Model model;
    EXPECT_EQ(katana::cad::resolveLinePattern(model, library, "CULT Fence Post", "CULT Fence Post")
                  .kind,
              LinetypeKind::Solid);
    // The same name as a linetype with no symbol is a linestyle, as D2 says.
    EXPECT_EQ(katana::cad::resolveLinePattern(model, library, "CULT Fence Post", "").kind,
              LinetypeKind::LibraryDefinition);
}

TEST(StyleResolver, TheSamplePathIsARunACornerAndAHalfCircleInsideItsBox)
{
    // Width 8: the run ends at 0.45 * 8 = 3.6, the corner is (0.6 * 8,
    // 0.25 * 8) = (4.8, 2), and the arc of radius 0.2 * 8 = 1.6 about
    // (6.4, 2) dips to (6.4, 0.4) and ends at (8, 2).
    const StyleSamplePath sample = katana::cad::styleSamplePath(8.0);
    ASSERT_EQ(sample.vertices.size(), 4u);
    const Point2 expected[] = {Point2(0, 0), Point2(3.6, 0), Point2(4.8, 2), Point2(8, 2)};
    for (std::size_t i = 0; i < 4; ++i) {
        EXPECT_NEAR(sample.vertices[i].x, expected[i].x, 1e-12) << i;
        EXPECT_NEAR(sample.vertices[i].y, expected[i].y, 1e-12) << i;
    }
    ASSERT_GT(sample.path.vertices.size(), 4u) << "the arc is chorded";
    double lowest = 10.0;
    for (std::size_t i = 3; i < sample.path.vertices.size(); ++i) {
        const Point2& p = sample.path.vertices[i];
        EXPECT_NEAR(p.distanceTo(Point2(6.4, 2)), 1.6, 1e-12) << "on the arc";
        lowest = std::min(lowest, p.y);
    }
    EXPECT_NEAR(lowest, 0.4, 1e-12);
    for (const Point2& p : sample.path.vertices) {
        EXPECT_GE(p.x, -1e-12);
        EXPECT_LE(p.x, 8.0 + 1e-12);
        EXPECT_GE(p.y, -1e-12);
        EXPECT_LE(p.y, 2.0 + 1e-12);
    }
}

TEST(StyleResolver, AModelLinetypeSampleIsItsDashSpansCountedByHand)
{
    // Dash 2, gap 1 along (0,0) -> (4,0) -> (4,3), seven long. The pattern
    // runs on across the corner: dash 0..2; gap 2..3; dash 3..5, which the
    // corner at 4 splits into (3,0)-(4,0) and (4,0)-(4,1); gap 5..6; dash
    // 6..7, cut by the end at (4,3). Four spans.
    Model model;
    ASSERT_TRUE(model.linetypes.add(dashed("DASH", {2.0, -1.0})));
    Style style;
    style.name = "S";
    style.linetype = "DASH";
    const StyleSamplePath sample{Polyline2{{Point2(0, 0), Point2(4, 0), Point2(4, 3)}, false},
                                 {Point2(0, 0), Point2(4, 0), Point2(4, 3)}};
    DashOptions options;
    options.viewScale = 1.0; // the shortest element, 1, is one pixel: resolvable
    const StyleDrawing drawing =
        katana::cad::styleSampleDrawing(model, StyleLibrary{}, style, sample, 1.0, options);
    const std::vector<std::vector<Point2>> spans{{Point2(0, 0), Point2(2, 0)},
                                                 {Point2(3, 0), Point2(4, 0)},
                                                 {Point2(4, 0), Point2(4, 1)},
                                                 {Point2(4, 2), Point2(4, 3)}};
    ASSERT_EQ(drawing.strokes.size(), spans.size());
    for (std::size_t i = 0; i < spans.size(); ++i) {
        EXPECT_EQ(drawing.strokes[i].path.vertices, spans[i]) << i;
    }
}

TEST(StyleResolver, ALibraryLinestyleSampleIsItsStrokesWithNoSolidLineAndNoDashesLeaked)
{
    // "L" is a 1-long dash and a 1-long gap (period 2). The model holds a
    // quarter-unit dash of the SAME name; D2 says the library wins and none
    // of that pattern reaches its strokes. Along a straight 9, the repeats
    // at 0, 2, 4, 6 and 8 give five 1-long dashes - no 9-long solid line,
    // and no quarter-unit pieces.
    Model model;
    ASSERT_TRUE(model.linetypes.add(dashed("L", {0.25, -0.25})));
    const StyleLibrary library = libraryOf({definition("L", {move(0, 0), draw(1, 0), move(2, 0)})});
    Style style;
    style.name = "S";
    style.linetype = "L";
    const StyleSamplePath sample{Polyline2{{Point2(0, 0), Point2(9, 0)}, false},
                                 {Point2(0, 0), Point2(9, 0)}};
    DashOptions options;
    options.viewScale = 1.0;
    const StyleDrawing drawing =
        katana::cad::styleSampleDrawing(model, library, style, sample, 1.0, options);
    ASSERT_EQ(drawing.strokes.size(), 5u);
    for (const auto& stroke : drawing.strokes) {
        EXPECT_NEAR(strokeLength(stroke), 1.0, 1e-12);
    }
    EXPECT_EQ(drawing.strokes.front().path.vertices.front(), Point2(0, 0));
    EXPECT_EQ(drawing.strokes.back().path.vertices.back(), Point2(9, 0));
}

TEST(StyleResolver, ASampleCarriesItsStylesSymbolAtEveryVertex)
{
    // D8. A continuous line with a cross 2 wide (1 either side) at each of the
    // sample's four vertices: the line, then two diagonals per vertex - nine
    // strokes, the first diagonal of each running from (v - 1) to (v + 1).
    Style style;
    style.name = "Fence";
    style.symbol = "cross";
    style.symbolSize = 2.0;
    const StyleSamplePath sample = katana::cad::styleSamplePath(8.0);
    const StyleDrawing drawing = katana::cad::styleSampleDrawing(
        Model{}, StyleLibrary{}, style, sample, 1.0, DashOptions{});
    ASSERT_EQ(drawing.strokes.size(), 1u + 4u * 2u);
    EXPECT_EQ(drawing.strokes[0].path.vertices, sample.path.vertices) << "the line itself";
    for (std::size_t v = 0; v < 4; ++v) {
        const Point2& at = sample.vertices[v];
        const auto& diagonal = drawing.strokes[1 + 2 * v].path.vertices;
        EXPECT_NEAR(diagonal.front().x, at.x - 1.0, 1e-12) << v;
        EXPECT_NEAR(diagonal.front().y, at.y - 1.0, 1e-12) << v;
        EXPECT_NEAR(diagonal.back().x, at.x + 1.0, 1e-12) << v;
        EXPECT_NEAR(diagonal.back().y, at.y + 1.0, 1e-12) << v;
    }
}

TEST(StyleResolver, ACacheReturnsFreshGeometryOnceTheLibraryGenerationMoves)
{
    // Through a real Document, whose setStyleLibrary bumps the generation.
    katana::cad::Document document;
    document.setStyleLibrary(libraryOf({definition("S", {move(0, 0), draw(1, 0)})}));
    DefinitionCache cache;
    const auto before =
        cache.find(document.styleLibrary(), document.libraryGeneration(), "S");
    ASSERT_NE(before, nullptr);
    EXPECT_EQ(before->runs.at(0).points.back(), Point2(1, 0));
    EXPECT_EQ(cache.find(document.styleLibrary(), document.libraryGeneration(), "S"), before)
        << "the same generation is answered from the cache";

    // The library is replaced: "S" now runs to (5, 0).
    document.setStyleLibrary(libraryOf({definition("S", {move(0, 0), draw(5, 0)})}));
    const auto after = cache.find(document.styleLibrary(), document.libraryGeneration(), "S");
    ASSERT_NE(after, nullptr);
    EXPECT_EQ(after->runs.at(0).points.back(), Point2(5, 0));
    EXPECT_EQ(cache.generation(), document.libraryGeneration());
    // What was handed out before is owned, not a view into the replaced
    // library, so it still reads what it read.
    EXPECT_EQ(before->runs.at(0).points.back(), Point2(1, 0));

    // A name the library lacks is remembered as absent, and the cached and
    // uncached drawings agree.
    EXPECT_EQ(cache.find(document.styleLibrary(), document.libraryGeneration(), "none"), nullptr);
    EXPECT_EQ(katana::cad::pointSymbolDrawing(cache, document.styleLibrary(),
                                              document.libraryGeneration(), "S", Point2(1, 1),
                                              0.0, 0.0, 1.0, 1.0)
                  .strokes[0]
                  .path.vertices,
              katana::cad::pointSymbolDrawing(document.styleLibrary(), "S", Point2(1, 1), 0.0,
                                              0.0, 1.0, 1.0)
                  .strokes[0]
                  .path.vertices);
}

TEST(StyleResolver, ASymbolUnderThreePixelsIsDrawnAsADot)
{
    // A symbol 2 units across: 2 px at 1 px a unit (a dot), 4 px at 2.
    const StyleLibrary library =
        libraryOf({definition("Box", {move(-1, -1), draw(1, -1), draw(1, 1)}, true)});
    const StyleDrawing drawing =
        katana::cad::pointSymbolDrawing(library, "Box", Point2(0, 0), 0.0, 0.0, 1.0, 1.0);
    EXPECT_TRUE(katana::cad::belowSymbolDetail(drawing, 1.0));
    EXPECT_FALSE(katana::cad::belowSymbolDetail(drawing, 2.0));
    EXPECT_FALSE(katana::cad::belowSymbolDetail(StyleDrawing{}, 1.0)) << "nothing to replace";
}

TEST(StyleResolver, TheDrawnExtentCountsATextsHeightTimesItsLength)
{
    // "AB", 1 high, anchored at (10, 0): 1 x 2 either side of the anchor,
    // x 8..12 and y -2..2 - more than any font draws, which is the safe side
    // for culling a label that reaches into the view.
    StyleDrawing drawing;
    drawing.texts.push_back(katana::cad::StyleTextMark{Point2(10, 0), "AB", 1.0});
    const auto box = katana::cad::drawnExtent(drawing);
    EXPECT_EQ(box.min, Point2(8, -2));
    EXPECT_EQ(box.max, Point2(12, 2));
}

TEST(StyleResolver, AVertexSymbolGoesOnEveryVertexOfALineAndNowhereOnACircle)
{
    using katana::entity::Geometry;
    EXPECT_EQ(katana::cad::symbolVertices(
                  Geometry{katana::geometry::Segment2{Point2(0, 0), Point2(3, 4)}}),
              (std::vector<Point2>{Point2(0, 0), Point2(3, 4)}));
    const Polyline2 fence{{Point2(0, 0), Point2(1, 0), Point2(1, 1)}, false};
    EXPECT_EQ(katana::cad::symbolVertices(Geometry{fence}), fence.vertices);
    EXPECT_TRUE(
        katana::cad::symbolVertices(Geometry{katana::geometry::Circle2{Point2(0, 0), 1.0}})
            .empty());
}
