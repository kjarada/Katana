// The RASTER SLOPE and RASTER ASPECT verbs (terrain_verbs.hpp, docs/terrain.md
// "Slope and aspect"): the slope or aspect of a surface or an elevation
// raster kept as a derived reference raster of its values, and the slope
// cut into classes drawn as areas - the same on katana_cli, katana_mcp and
// the window's command line, where Terrain > Analysis > Slope and Aspect
// builds these lines.
//
//   RASTER SLOPE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//                [unit=percent|degree] [classes=<b1>,<b2>,...] [areas=terrain/slope]
//                [min_area=<m2>] [NAME <n>] [<scope>] [PREVIEW]
//   RASTER ASPECT <source> [NAME <n>] [<scope>] [PREVIEW]
//
// GDAL computes both (`raster slope`, `raster aspect`: Horn's 3 x 3 window,
// the edges interpolated). The raster kept holds the values, at full
// precision, so another verb can read it as RASTER <id> - zonal statistics
// of slope, contours of it - and its display copy is coloured: by class
// when there are classes, else by the slope ramp.
//
// classes= are the breaks between slope classes: classes=5,10,25 makes
// [0,5), [5,10), [10,25) and [25, and up). GDAL's `raster reclassify` sorts
// the cells into them, `raster sieve` merges regions under min_area into
// their largest neighbour, and `raster polygonize` draws each region: a
// closed polyline on <areas>/<class> with slope_class, slope_from, slope_to
// (none on the last, open class) and slope_unit properties, in one undo
// step, and a report of each class's area. The first class begins at 0,
// below which no slope lies: a class bounded by -inf would take in the
// -9999 no-data GDAL gives a slope it cannot compute.
//
// A scope, last on the line, keeps the analysis inside its closed shapes:
// the raster is cut to their box (a cell wider, so the slope at the boundary
// is computed from the ground beyond it), the slope then to the shapes -
// GDAL's raster clip keeps every cell a shape touches - and the class areas,
// which are whole cells, to the shapes exactly (vector clip), so each
// class's area is its area inside them.

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/interop/geo/colour_ramps.hpp"
#include "katana/interop/import.hpp"
#include "replies.hpp"
#include "terrain_verbs.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kSlopeUsage =
    "RASTER SLOPE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> "
    "[unit=percent|degree] [classes=<b1>,<b2>,...] [areas=terrain/slope] [min_area=<m2>] "
    "[NAME <n>] [<scope>] [PREVIEW]";
constexpr const char* kAspectUsage =
    "RASTER ASPECT SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> [NAME <n>] "
    "[<scope>] [PREVIEW]";

// What polygonize names the field holding each region's class.
constexpr const char* kClassField = "slope_class";

katana::core::Error refusal(const std::string& why, const std::string& word = {})
{
    return makeError(ErrorCode::InvalidArgument, why, word);
}

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

struct SlopeWords {
    bool aspect = false;
    std::string unit = "percent";
    std::vector<double> breaks;
    std::string areas = "terrain/slope";
    bool areasGiven = false;
    std::optional<double> minArea;
    std::optional<std::string> name;
    std::optional<katana::cad::ScopeWords> scope;
    bool preview = false;
};

Result<std::vector<double>> breakList(const std::string& text, const std::string& unit)
{
    std::vector<double> breaks;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t comma = text.find(',', start);
        const std::string item =
            text.substr(start, comma == std::string::npos ? std::string::npos : comma - start);
        const auto value = katana::core::parseFiniteDouble(item);
        if (!value || !(*value > 0.0) || (!breaks.empty() && !(*value > breaks.back())) ||
            (unit == "degree" && !(*value < 90.0))) {
            return refusal("classes are the slopes between classes, ascending and above 0 (under "
                           "90 in degrees): classes=5,10,25",
                           text);
        }
        breaks.push_back(*value);
        if (comma == std::string::npos) {
            break;
        }
        start = comma + 1;
    }
    return breaks;
}

Result<SlopeWords> slopeWords(const Tokens& tokens, std::size_t at, bool aspect)
{
    SlopeWords words;
    words.aspect = aspect;
    std::optional<std::string> classes;
    const std::string takes =
        aspect ? "RASTER ASPECT takes NAME, a scope and PREVIEW"
               : "RASTER SLOPE takes unit=, classes=, areas=, min_area=, NAME, a scope and PREVIEW";
    while (at < tokens.size()) {
        if (tokens.is(at, "PREVIEW")) {
            words.preview = true;
            ++at;
        } else if (tokens.is(at, "NAME")) {
            if (at + 1 >= tokens.size() || tokens[at + 1].empty()) {
                return refusal("NAME needs the reference raster's name");
            }
            words.name = tokens[at + 1];
            at += 2;
        } else if (const auto option = keyValue(tokens, at); option && !aspect) {
            const auto& [key, text] = *option;
            if (key == "unit") {
                const std::string unit = katana::core::lowered(text);
                if (unit != "percent" && unit != "degree") {
                    return refusal("unit is percent or degree", text);
                }
                words.unit = unit;
            } else if (key == "classes") {
                classes = text;
            } else if (key == "areas") {
                if (auto valid = katana::entity::validateLayerPath(text); !valid) {
                    return refusal("areas is a layer path: " + valid.error().message, text);
                }
                words.areas = text;
                words.areasGiven = true;
            } else if (key == "min_area") {
                auto area = positiveOption("min_area", text);
                if (!area) {
                    return area.error();
                }
                words.minArea = *area;
            } else {
                return refusal(takes, tokens[at]);
            }
            ++at;
        } else if (!tokens.quoted[at] && katana::cad::isScopeWord(tokens[at])) {
            if (words.scope) {
                return refusal("one scope per line", tokens[at]);
            }
            auto scope = katana::cad::parseScopeWords(tokens.words, at);
            if (!scope) {
                return scope.error();
            }
            words.scope = std::move(scope).value();
        } else {
            return refusal(takes, tokens[at]);
        }
    }
    if (classes) {
        // Read after the line, so unit= may come after classes=.
        auto breaks = breakList(*classes, words.unit);
        if (!breaks) {
            return breaks.error();
        }
        words.breaks = std::move(breaks).value();
    }
    if (words.breaks.empty() && (words.minArea || words.areasGiven)) {
        return refusal("areas= and min_area= are the classes' areas: give classes= too");
    }
    return words;
}

// One class of slope: [from, to), the last open above.
struct SlopeClass {
    int number = 0; // 1-based, what reclassify writes
    double from = 0.0;
    std::optional<double> to;
    std::string name; // "5-10", "25+": its layer under areas=
    std::uint8_t r = 0, g = 0, b = 0;
};

std::vector<SlopeClass> classesOf(const std::vector<double>& breaks)
{
    const igeo::ColourRamp& ramp = *igeo::builtInRamp("slope");
    std::vector<SlopeClass> classes;
    const std::size_t count = breaks.size() + 1;
    for (std::size_t i = 0; i < count; ++i) {
        SlopeClass made;
        made.number = static_cast<int>(i + 1);
        made.from = i == 0 ? 0.0 : breaks[i - 1];
        if (i < breaks.size()) {
            made.to = breaks[i];
        }
        made.name = gdalNumber(made.from) + (made.to ? "-" + gdalNumber(*made.to) : "+");
        // The slope ramp read at the class's place among them: green for the
        // gentlest, red for the steepest.
        const double position =
            count > 1 ? static_cast<double>(i) / static_cast<double>(count - 1) : 0.5;
        const auto colours = igeo::spread(ramp, 0.0, 1.0);
        const auto after = std::ranges::find_if(
            colours, [&](const igeo::LegendEntry& entry) { return entry.value >= position; });
        const igeo::LegendEntry& upper = *after;
        const igeo::LegendEntry& lower = after == colours.begin() ? *after : *(after - 1);
        const double span = upper.value - lower.value;
        const double t = span > 0.0 ? (position - lower.value) / span : 0.0;
        const auto mix = [t](std::uint8_t a, std::uint8_t c) {
            return static_cast<std::uint8_t>(std::lround(a + t * (static_cast<double>(c) - a)));
        };
        made.r = mix(lower.r, upper.r);
        made.g = mix(lower.g, upper.g);
        made.b = mix(lower.b, upper.b);
        classes.push_back(made);
    }
    return classes;
}

// GDAL's reclassify mapping: [0,b1)=1; [b1,b2)=2; ...; [bk,inf]=k+1, and the
// no-data kept no-data.
std::string mappingOf(const std::vector<SlopeClass>& classes)
{
    std::string mapping;
    for (const SlopeClass& each : classes) {
        mapping += "[" + gdalNumber(each.from) + "," +
                   (each.to ? gdalNumber(*each.to) + ")" : std::string("inf]")) + "=" +
                   std::to_string(each.number) + "; ";
    }
    return mapping + "NO_DATA=NO_DATA";
}

// The plan area a ring encloses, by the shoelace rule, whether or not its
// first vertex is repeated at its end.
double ringArea(const std::vector<katana::gis::GeoPoint>& ring)
{
    if (ring.size() < 3) {
        return 0.0;
    }
    // Relative to the first vertex, so large coordinates keep their digits.
    const double x0 = ring.front().x;
    const double y0 = ring.front().y;
    double twice = 0.0;
    for (std::size_t i = 0; i < ring.size(); ++i) {
        const katana::gis::GeoPoint& a = ring[i];
        const katana::gis::GeoPoint& b = ring[(i + 1) % ring.size()];
        twice += (a.x - x0) * (b.y - y0) - (b.x - x0) * (a.y - y0);
    }
    return std::abs(twice) / 2.0;
}

double polygonArea(const katana::gis::VectorGeometry& polygon)
{
    double area = 0.0;
    for (std::size_t r = 0; r < polygon.parts.size(); ++r) {
        area += (r == 0 ? 1.0 : -1.0) * ringArea(polygon.parts[r]);
    }
    return area;
}

// The regions polygonize drew, a table per class present, each feature
// carrying its class's properties; and each class's area and count.
struct ClassAreas {
    gp::FeatureSet set;
    std::vector<double> area;         // by class, index number - 1
    std::vector<std::size_t> regions; // likewise
};

ClassAreas classAreas(const gp::FeatureSet& drawn, const std::vector<SlopeClass>& classes,
                      const std::string& unit, const std::string& crs)
{
    ClassAreas result;
    result.area.assign(classes.size(), 0.0);
    result.regions.assign(classes.size(), 0);
    std::vector<gp::FeatureTable> tables(classes.size());
    for (std::size_t i = 0; i < classes.size(); ++i) {
        gp::FeatureTable& table = tables[i];
        table.name = classes[i].name;
        table.kind = katana::gis::GeometryKind::Polygon;
        table.crsWkt = crs;
        table.fields = {{"slope_class", gp::FieldType::Integer64},
                        {"slope_from", gp::FieldType::Real},
                        {"slope_to", gp::FieldType::Real},
                        {"slope_unit", gp::FieldType::String}};
    }
    for (const gp::FeatureTable& table : drawn.tables) {
        const auto field = std::ranges::find_if(
            table.fields, [](const gp::FieldDef& def) { return def.name == kClassField; });
        if (field == table.fields.end()) {
            continue;
        }
        const auto index = static_cast<std::size_t>(field - table.fields.begin());
        for (const gp::Feature& feature : table.features) {
            if (index >= feature.values.size()) {
                continue;
            }
            const gp::FieldValue& held = feature.values[index];
            const std::int64_t number = std::holds_alternative<std::int64_t>(held)
                                            ? std::get<std::int64_t>(held)
                                        : std::holds_alternative<double>(held)
                                            ? std::llround(std::get<double>(held))
                                            : 0;
            if (number < 1 || number > static_cast<std::int64_t>(classes.size())) {
                continue;
            }
            const auto slot = static_cast<std::size_t>(number - 1);
            const SlopeClass& each = classes[slot];
            gp::Feature made;
            made.parts = feature.parts;
            made.values = {std::int64_t{each.number}, each.from,
                           each.to ? gp::FieldValue(*each.to) : gp::FieldValue(std::monostate{}),
                           unit};
            for (const katana::gis::VectorGeometry& part : feature.parts) {
                result.area[slot] += polygonArea(part);
            }
            ++result.regions[slot];
            tables[slot].features.push_back(std::move(made));
        }
    }
    for (gp::FeatureTable& table : tables) {
        if (!table.features.empty()) {
            result.set.tables.push_back(std::move(table));
        }
    }
    return result;
}

// What the work hands the apply.
struct Analysed {
    std::filesystem::path values;  // the slope or aspect, full precision
    std::filesystem::path picture; // its display copy, coloured
    std::vector<std::string> records;
    std::optional<ClassAreas> areas;
    std::vector<std::string> warnings;
};

Result<Analysed> analyse(const gp::DatasetValue& input, const SlopeWords& words,
                         const std::vector<SlopeClass>& classes, const ScopeAreas& areas,
                         const std::string& crs, const std::filesystem::path& scratch,
                         const std::stop_token& stop, const Progress& progress)
{
    RasterChain chain(input, scratch);
    Analysed result;
    auto facts = chain.info();
    if (!facts) {
        return facts.error();
    }
    const auto& gt = facts->geotransform;
    const double cell = std::max(std::hypot(gt[1], gt[4]), std::hypot(gt[2], gt[5]));
    if (words.scope) {
        // A cell wider than the areas, within the raster: Horn's window at
        // the boundary then reads real ground, not an edge rule.
        const katana::geometry::Box2 wanted = areas.box.inflated(cell);
        const katana::geometry::Box2 extent = rasterExtent(*facts);
        const katana::geometry::Box2 cut(
            {std::max(wanted.min.x, extent.min.x), std::max(wanted.min.y, extent.min.y)},
            {std::min(wanted.max.x, extent.max.x), std::min(wanted.max.y, extent.max.y)});
        if (cut.empty() || cut.width() < cell || cut.height() < cell) {
            return makeError(ErrorCode::InvalidArgument,
                             "the scope's closed shapes lie off the raster");
        }
        auto clipped =
            chain.step({"raster", "clip"},
                       {"--bbox=" + gdalNumber(cut.min.x) + "," + gdalNumber(cut.min.y) + "," +
                        gdalNumber(cut.max.x) + "," + gdalNumber(cut.max.y)},
                       stop, progress);
        if (!clipped) {
            return clipped.error();
        }
    }
    auto computed = words.aspect
                        ? chain.step({"raster", "aspect"}, {}, stop, progress)
                        : chain.step({"raster", "slope"}, {"--unit=" + words.unit}, stop, progress);
    if (!computed) {
        return computed.error();
    }
    if (words.scope) {
        auto inside = chain.step({"raster", "clip"}, {"--geometry=" + areasWkt(areas.areas)}, stop,
                                 progress);
        if (!inside) {
            return inside.error();
        }
    }
    static std::atomic<std::uint64_t> made{0};
    const std::string stem = (words.aspect ? "katana-aspect-" : "katana-slope-") +
                             std::to_string(++made);
    auto values = chain.keep(scratch / (stem + ".tif"));
    if (!values) {
        return values.error();
    }
    result.values = *values;
    auto kept = chain.info();
    if (!kept) {
        return kept.error();
    }
    const std::optional<double> side = squareCellOf(*kept);
    const double cellArea = std::abs(kept->geotransform[1] * kept->geotransform[5] -
                                     kept->geotransform[2] * kept->geotransform[4]);
    result.records.push_back(
        std::string(words.aspect ? "aspect convention=azimuth flat=nodata"
                                 : "slope unit=" + words.unit + " method=horn") +
        " raster=" + std::to_string(kept->width) + "x" + std::to_string(kept->height) +
        " cell=" + (side ? katana::core::formatExactReal(*side) : std::string()));

    std::string map;
    std::vector<std::string> colourTokens;
    if (!words.aspect && !classes.empty()) {
        auto reclassified = chain.step({"raster", "reclassify"},
                                       {"--mapping=" + mappingOf(classes), "--ot=Int16"}, stop,
                                       progress);
        if (!reclassified) {
            return reclassified.error();
        }
        if (words.minArea) {
            // The least region kept, in whole cells: a region is sieved when
            // it covers fewer cells than min_area does.
            const double cells = std::ceil(*words.minArea / cellArea);
            auto sieved = chain.step({"raster", "sieve"},
                                     {"--size-threshold=" + gdalNumber(cells)}, stop, progress);
            if (!sieved) {
                return sieved.error();
            }
            result.records.push_back("sieved min_area=" + gdalNumber(*words.minArea) +
                                     " cells=" + gdalNumber(cells));
        }
        auto regions = chain.features({"raster", "polygonize"},
                                      {"--attribute-name=" + std::string(kClassField)}, stop,
                                      progress);
        if (!regions) {
            return regions.error();
        }
        if (words.scope && !regions->tables.empty()) {
            // The regions are whole cells, and the raster's clip keeps every
            // cell a shape touches: cut to the shapes themselves, so a class's
            // area is its area inside them and the classes add up to them.
            gp::RunRequest cut;
            cut.path = {"vector", "clip"};
            cut.values.emplace_back("input", gp::ArgValue(gp::DatasetValue(std::move(*regions))));
            cut.tokens = {"--geometry=" + areasWkt(areas.areas)};
            cut.outputTo = gp::OutputTo::Memory;
            auto inside = gp::run(cut, stop, progress);
            if (!inside) {
                return inside.error();
            }
            regions = inside->features ? std::move(*inside->features) : gp::FeatureSet{};
            for (const gp::Diagnostic& warning : inside->diagnostics) {
                result.warnings.push_back(warning.message);
            }
        }
        result.areas = classAreas(*regions, classes, words.unit, crs);
        for (const SlopeClass& each : classes) {
            map += std::to_string(each.number) + " " + std::to_string(each.r) + " " +
                   std::to_string(each.g) + " " + std::to_string(each.b) + " 255\n";
        }
        colourTokens.push_back("--color-selection=exact");
    } else if (!words.aspect) {
        // No classes: the slope ramp over the slope's own range, which GDAL
        // reads itself from the percentages.
        for (const igeo::RampStop& colour : igeo::builtInRamp("slope")->stops) {
            map += gdalNumber(colour.position * 100.0) + "% " + std::to_string(colour.r) + " " +
                   std::to_string(colour.g) + " " + std::to_string(colour.b) + " 255\n";
        }
    } else {
        // Aspect is a direction: a grey ramp would draw north (0) and north
        // (360) as black and white. Eight compass colours, the ramp's hues
        // around the circle, north at both ends.
        const std::uint8_t compass[9][3] = {{215, 25, 28},  {253, 174, 97}, {255, 255, 191},
                                            {166, 217, 106}, {26, 150, 65},  {43, 131, 186},
                                            {94, 60, 153},   {194, 75, 160}, {215, 25, 28}};
        for (int i = 0; i <= 8; ++i) {
            map += gdalNumber(45.0 * i) + " " + std::to_string(compass[i][0]) + " " +
                   std::to_string(compass[i][1]) + " " + std::to_string(compass[i][2]) + " 255\n";
        }
    }
    map += "nv 0 0 0 0\n";
    auto written = chain.write("colours.txt", map);
    if (!written) {
        return written.error();
    }
    colourTokens.insert(colourTokens.begin(), {"--color-map=" + utf8Of(*written), "--add-alpha"});
    auto coloured = chain.step({"raster", "color-map"}, colourTokens, stop, progress);
    if (!coloured) {
        return coloured.error();
    }
    auto picture = chain.keep(scratch / (stem + "-picture.tif"));
    if (!picture) {
        return picture.error();
    }
    result.picture = *picture;
    for (const gp::Diagnostic& warning : chain.warnings) {
        result.warnings.push_back(warning.message);
    }
    return result;
}

Result<Prepared> prepareSlopeOrAspect(Context& context, const Tokens& tokens,
                                      std::string_view line, bool aspect)
{
    std::size_t at = 2;
    const char* verb = aspect ? "RASTER ASPECT" : "RASTER SLOPE";
    auto source = bindTerrainSource(context, tokens, at, verb);
    if (!source) {
        return source.error();
    }
    auto words = slopeWords(tokens, at, aspect);
    if (!words) {
        return words.error();
    }
    // No breaks, no classes: the slope alone.
    const std::vector<SlopeClass> classes =
        words->breaks.empty() ? std::vector<SlopeClass>{} : classesOf(words->breaks);
    std::vector<std::string> records{source->record};
    ScopeAreas areas;
    if (words->scope) {
        auto bound = bindAreas(context, *words->scope, "area");
        if (!bound) {
            return bound.error();
        }
        areas = std::move(bound).value();
        records.insert(records.end(), areas.records.begin(), areas.records.end());
    }
    const std::string name =
        words->name.value_or(sourceName(context, source->source) + (aspect ? "-aspect" : "-slope"));
    const bool nothingInside = words->scope && areas.areas.empty();
    if (words->preview || nothingInside) {
        // A scope with no closed shape has nothing to analyse: said, not
        // refused.
        records.push_back(std::string(aspect ? "aspect" : "slope") + " name=" + value(name) +
                          " made=no");
        if (words->preview) {
            records.push_back("preview valid=yes changed=no");
        }
        return answeredWith(verb, joinedRecords(records));
    }
    if (!words->breaks.empty()) {
        for (const SlopeClass& each : classes) {
            const std::string layer = words->areas + "/" + each.name;
            if (auto valid = katana::entity::validateLayerPath(layer); !valid) {
                return refusal("a class's layer is not a layer path: " + valid.error().message,
                               layer);
            }
        }
    }

    Prepared prepared;
    prepared.title = aspect ? "Aspect" : "Slope Analysis";
    const SlopeWords options = *words;
    const DeferredDataset dataset = source->dataset;
    const std::filesystem::path scratch = context.scratch;
    const std::string command(katana::core::trimmed(line));
    const std::string crs = projectCrs(context);
    auto shared = std::make_shared<const ScopeAreas>(std::move(areas));
    prepared.work = [dataset, options, classes, shared, crs, scratch, records, name,
                     command](const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        auto input = dataset();
        if (!input) {
            return input.error();
        }
        auto analysed = analyse(*input, options, classes, *shared, crs, scratch, stop, progress);
        if (!analysed) {
            return analysed.error();
        }
        auto kept = std::make_shared<Analysed>(std::move(analysed).value());
        return Apply([kept, options, classes, records, name,
                      command](Context& ctx) -> Result<std::string> {
            std::vector<std::string> reply = records;
            reply.insert(reply.end(), kept->records.begin(), kept->records.end());
            // The values, as a derived reference raster another verb can read.
            ApplyRequest request;
            request.target.kind = Target::Kind::Reference;
            request.target.name = name;
            request.defaultName = name;
            request.result.commandName = command;
            gp::RunOutputs outputs;
            outputs.file = utf8Of(kept->values);
            auto applied = applyOutputs(ctx, request, std::move(outputs));
            std::error_code removed;
            if (!applied) {
                std::filesystem::remove(kept->picture, removed);
                return applied.error();
            }
            reply.push_back(*applied);
            // Drawn as its coloured picture, whose grid is the values' own.
            auto picture = katana::interop::importRaster(kept->picture);
            std::filesystem::remove(kept->picture, removed);
            for (const Record& record : parseRecords(*applied)) {
                const auto id = record.kind == "output" ? record.get("id") : std::nullopt;
                const auto number = id ? katana::core::parseInteger(*id) : std::nullopt;
                katana::interop::RasterOverlay* raster =
                    number ? ctx.reference.findRaster(
                                 static_cast<katana::interop::ReferenceId>(*number))
                           : nullptr;
                if (raster == nullptr) {
                    continue;
                }
                if (picture) {
                    raster->rgba = std::move(picture->rgba);
                    raster->width = picture->width;
                    raster->height = picture->height;
                    raster->geotransform = picture->geotransform;
                }
                if (!options.aspect) {
                    raster->displayStyle = katana::interop::RasterDisplayStyle::Slope;
                }
            }
            if (!picture) {
                reply.push_back(warningRecord("the coloured picture could not be read, so the "
                                              "raster is drawn grey: " +
                                              picture.error().message));
            }
            if (ctx.changed) {
                ctx.changed();
            }
            if (kept->areas) {
                // The class areas: one command, one undo step.
                igeo::ResultOptions result;
                result.targetLayer = kept->areas->set.tables.size() == 1
                                         ? options.areas + "/" + kept->areas->set.tables[0].name
                                         : options.areas;
                result.operation = "RASTER SLOPE";
                result.commandName = command;
                auto plan = igeo::resultCommand(ctx.document.model(), kept->areas->set, result);
                if (!plan) {
                    return plan.error();
                }
                if (plan->command) {
                    if (auto status = ctx.document.execute(std::move(plan->command)); !status) {
                        return status.error();
                    }
                }
                reply.push_back("output arg=areas kind=vector target=layer layer=" +
                                value(options.areas) + " created=" + std::to_string(plan->created));
                for (const SlopeClass& each : classes) {
                    const auto slot = static_cast<std::size_t>(each.number - 1);
                    reply.push_back("class name=" + value(each.name) +
                                    " from=" + gdalNumber(each.from) +
                                    " to=" + (each.to ? gdalNumber(*each.to) : std::string()) +
                                    " unit=" + options.unit +
                                    " area=" + fixed3(kept->areas->area[slot]) +
                                    " polygons=" + std::to_string(kept->areas->regions[slot]));
                }
                for (const SlopeClass& each : classes) {
                    reply.push_back("legend value=" + gdalNumber(each.from) +
                                    " r=" + std::to_string(each.r) + " g=" + std::to_string(each.g) +
                                    " b=" + std::to_string(each.b));
                }
            }
            for (const std::string& warning : kept->warnings) {
                reply.push_back(warningRecord(warning));
            }
            return joinedRecords(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareSlope(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareSlopeOrAspect(context, tokens, line, false);
}

Result<Prepared> prepareAspect(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareSlopeOrAspect(context, tokens, line, true);
}

std::string slopeUsage()
{
    return kSlopeUsage;
}

std::string aspectUsage()
{
    return kAspectUsage;
}

} // namespace katana::app::geo
