#include "survey_drawing.hpp"

#include <algorithm>
#include <cmath>
#include <string>
#include <vector>

#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/tables.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/math/numerics.hpp"

namespace katana::bench {

namespace {

using katana::entity::Color;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

// SplitMix64 (Steele, Lea and Flood 2014): a fixed sequence for a fixed seed
// on every compiler, which std::uniform_real_distribution does not promise.
class Stream {
  public:
    explicit Stream(std::uint64_t seed) : state_(seed) {}
    std::uint64_t next()
    {
        std::uint64_t z = (state_ += 0x9e3779b97f4a7c15ULL);
        z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ULL;
        z = (z ^ (z >> 27)) * 0x94d049bb133111ebULL;
        return z ^ (z >> 31);
    }
    // [0, 1) with 53 bits, the top of the word.
    double unit() { return static_cast<double>(next() >> 11) * 0x1.0p-53; }
    double between(double low, double high) { return low + (high - low) * unit(); }
    std::size_t below(std::size_t count) { return static_cast<std::size_t>(next() % count); }

  private:
    std::uint64_t state_;
};

// A palette of the colours a survey drawing's layers come in: white, the
// greys and the saturated primaries a mapfile uses.
const std::vector<Color>& palette()
{
    static const std::vector<Color> colours{
        {255, 255, 255, 255}, {255, 0, 0, 255},   {0, 255, 0, 255},     {0, 128, 255, 255},
        {255, 255, 0, 255},   {0, 255, 255, 255}, {255, 0, 255, 255},   {255, 128, 0, 255},
        {160, 160, 160, 255}, {128, 64, 0, 255},  {0, 160, 80, 255},    {200, 120, 255, 255}};
    return colours;
}

// A wandering string: a kerb, a fence, a drainage line - consecutive
// vertices a few metres apart, turning gently, as a surveyed string does.
Polyline2 wander(Stream& stream, const Point2& start, std::size_t vertices, double step)
{
    Polyline2 line;
    line.vertices.reserve(vertices);
    double heading = stream.between(0.0, katana::math::kTwoPi);
    Point2 at = start;
    for (std::size_t i = 0; i < vertices; ++i) {
        line.vertices.push_back(at);
        heading += stream.between(-0.35, 0.35);
        const double length = step * stream.between(0.6, 1.4);
        at = Point2(at.x + length * std::cos(heading), at.y + length * std::sin(heading));
    }
    return line;
}

} // namespace

katana::core::Result<SurveyDrawingSummary> buildSurveyDrawing(katana::cad::Document& document,
                                                              const SurveyDrawingOptions& options)
{
    namespace cmd = katana::commands;
    Stream stream(options.seed);
    SurveyDrawingSummary summary;

    // ---- the library's names, in name order ------------------------------------------
    std::vector<std::string> symbols;
    std::vector<std::string> linestyles;
    document.styleLibrary().forEach([&](const katana::entity::LineStyle& style) {
        if (style.strokes.empty()) {
            return;
        }
        // By what each definition says it is (LineStyle::symbol), not by the
        // name of the file it came from: one customisation now holds both
        // kinds under one name, and a test of the name would build no
        // linestyle styles at all without a word.
        if (style.atVertices || style.symbol) {
            symbols.push_back(style.name);
        } else {
            linestyles.push_back(style.name);
        }
    });
    if (symbols.empty()) {
        for (const std::string_view shape : katana::entity::symbolNames()) {
            symbols.emplace_back(shape);
        }
    }
    // Forty of each: the real drawing has 217 styles, most of them used by a
    // handful of entities, and a few dozen carry nearly all of them.
    const std::size_t symbolStyles = std::min<std::size_t>(40, symbols.size());
    const std::size_t lineStyles = std::min<std::size_t>(40, linestyles.size());

    // ---- tables --------------------------------------------------------------------------
    const std::vector<std::string> layers{"survey/points", "survey/strings", "survey/contours",
                                          "survey/text",   "design/lots",    "design/dimensions"};
    for (std::size_t i = 0; i < layers.size(); ++i) {
        katana::entity::Layer layer;
        layer.name = layers[i];
        layer.color = palette()[i % palette().size()];
        layer.lineWeight = i == 2 ? 0.18 : 0.25;
        if (auto status = document.execute(cmd::createLayer(layer)); !status) {
            return status.error();
        }
    }
    katana::entity::Linetype dashed;
    dashed.name = "survey dashed";
    dashed.description = "Dashed 2 m / 1 m";
    dashed.pattern = {{2.0}, {-1.0}};
    if (auto status = document.execute(cmd::createLinetype(dashed)); !status) {
        return status.error();
    }
    katana::entity::HatchPattern hatch;
    hatch.name = "lot hatch";
    hatch.families = {{katana::math::kPi / 4.0, 2.0, 0.0}};
    if (auto status = document.execute(cmd::createHatchPattern(hatch)); !status) {
        return status.error();
    }
    katana::entity::HatchPattern solid;
    solid.name = "lot solid";
    solid.solid = true;
    if (auto status = document.execute(cmd::createHatchPattern(solid)); !status) {
        return status.error();
    }

    std::vector<std::string> pointStyles;
    for (std::size_t i = 0; i < symbolStyles; ++i) {
        katana::entity::Style style;
        style.name = "PT " + std::to_string(i);
        style.symbol = symbols[i];
        style.color = palette()[(i * 5) % palette().size()];
        if (auto status = document.execute(cmd::createStyle(style)); !status) {
            return status.error();
        }
        pointStyles.push_back(style.name);
    }
    summary.symbolStyles = pointStyles.size();
    std::vector<std::string> stringStyles;
    for (std::size_t i = 0; i < lineStyles; ++i) {
        katana::entity::Style style;
        style.name = "LS " + std::to_string(i);
        style.linetype = linestyles[i];
        style.color = palette()[(i * 7 + 1) % palette().size()];
        if (auto status = document.execute(cmd::createStyle(style)); !status) {
            return status.error();
        }
        stringStyles.push_back(style.name);
    }
    summary.linestyleStyles = stringStyles.size();
    for (const auto& [name, hatchName] :
         {std::pair<std::string, std::string>{"LOT", "lot hatch"}, {"LOT SOLID", "lot solid"}}) {
        katana::entity::Style style;
        style.name = name;
        style.hatchPattern = hatchName;
        if (auto status = document.execute(cmd::createStyle(style)); !status) {
            return status.error();
        }
    }
    katana::entity::Style dashedStyle;
    dashedStyle.name = "CONTOUR DASHED";
    dashedStyle.linetype = dashed.name;
    if (auto status = document.execute(cmd::createStyle(dashedStyle)); !status) {
        return status.error();
    }

    // ---- entities --------------------------------------------------------------------------
    std::vector<Entity> entities;
    const auto anywhere = [&]() {
        return Point2(options.origin.x + stream.between(0.0, options.widthMetres),
                      options.origin.y + stream.between(0.0, options.heightMetres));
    };
    const auto add = [&](katana::entity::Geometry geometry, const std::string& layer,
                         const std::string& style) {
        Entity entity;
        entity.geometry = std::move(geometry);
        entity.layer = layer;
        entity.style = style;
        entities.push_back(std::move(entity));
    };

    // Coded points, clustered along the strings the way a pick-up is: a
    // point every few metres along a walked line, not scattered uniformly.
    for (std::size_t i = 0; i < options.codedPoints;) {
        const Polyline2 walk = wander(stream, anywhere(), 25, 6.0);
        const std::string& style = pointStyles.empty() ? std::string()
                                                       : pointStyles[stream.below(pointStyles.size())];
        for (const Point2& at : walk.vertices) {
            if (i++ == options.codedPoints) {
                break;
            }
            add(katana::entity::PointGeometry{at}, layers[0], style);
            ++summary.vertices;
        }
    }
    for (std::size_t i = 0; i < options.styledStrings; ++i) {
        Polyline2 line = wander(stream, anywhere(), 6 + stream.below(12), 5.0);
        summary.vertices += line.vertices.size();
        add(std::move(line), layers[1],
            stringStyles.empty() ? std::string() : stringStyles[stream.below(stringStyles.size())]);
    }
    // Contours: long, dense strings running right across the site, so a
    // zoomed-in view cuts hundreds of them part way - the case clipping to
    // the view is for.
    for (std::size_t i = 0; i < options.plainStrings; ++i) {
        const double y = options.origin.y + options.heightMetres * (static_cast<double>(i) + 0.5) /
                                                static_cast<double>(options.plainStrings);
        Polyline2 line;
        const std::size_t vertices = 400;
        line.vertices.reserve(vertices);
        const double phase = stream.between(0.0, katana::math::kTwoPi);
        for (std::size_t v = 0; v < vertices; ++v) {
            const double x = options.origin.x + options.widthMetres * static_cast<double>(v) /
                                                    static_cast<double>(vertices - 1);
            line.vertices.emplace_back(x, y + 6.0 * std::sin(phase + 0.01 * (x - options.origin.x)));
        }
        summary.vertices += line.vertices.size();
        add(std::move(line), layers[2], i % 5 == 4 ? dashedStyle.name : std::string());
    }
    for (std::size_t i = 0; i < options.labels; ++i) {
        katana::entity::TextGeometry text;
        text.position = anywhere();
        text.text = "RL " + std::to_string(20 + stream.below(80)) + "." +
                    std::to_string(stream.below(1000));
        // The few heights a drawing's text styles give, not a spread: a
        // drawing's labels come in a handful of sizes.
        constexpr double kHeights[] = {1.0, 1.8, 2.5, 3.5};
        text.height = kHeights[stream.below(4)];
        text.rotation = stream.between(-0.5, 0.5);
        add(std::move(text), layers[3], {});
    }
    for (std::size_t i = 0; i < options.arcs; ++i) {
        const Point2 centre = anywhere();
        const double radius = stream.between(2.0, 40.0);
        if (i % 3 == 0) {
            add(katana::geometry::Circle2{centre, radius}, layers[1], {});
        } else {
            add(katana::geometry::Arc2{centre, radius, stream.between(0.0, katana::math::kTwoPi),
                                       stream.between(0.3, 3.0)},
                layers[1], {});
        }
    }
    for (std::size_t i = 0; i < options.hatchedLots; ++i) {
        const Point2 corner = anywhere();
        const double w = stream.between(15.0, 40.0);
        const double h = stream.between(20.0, 50.0);
        Polyline2 lot{{corner, Point2(corner.x + w, corner.y), Point2(corner.x + w, corner.y + h),
                       Point2(corner.x, corner.y + h)},
                      true};
        summary.vertices += lot.vertices.size();
        add(std::move(lot), layers[4], i % 10 == 9 ? "LOT SOLID" : "LOT");
    }
    for (std::size_t i = 0; i < options.dimensions; ++i) {
        katana::entity::DimensionGeometry dimension;
        dimension.start = anywhere();
        const double angle = stream.between(0.0, katana::math::kTwoPi);
        const double length = stream.between(5.0, 60.0);
        dimension.end = Point2(dimension.start.x + length * std::cos(angle),
                               dimension.start.y + length * std::sin(angle));
        dimension.offset = stream.between(1.0, 4.0);
        add(std::move(dimension), layers[5], {});
    }

    summary.entities = entities.size();
    for (const Entity& entity : entities) {
        summary.extent.expand(katana::entity::boundingBox(entity.geometry));
    }
    if (auto status = document.execute(cmd::createEntities(std::move(entities))); !status) {
        return status.error();
    }
    return summary;
}

} // namespace katana::bench
