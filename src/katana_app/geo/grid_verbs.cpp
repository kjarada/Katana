// RASTER GRID: surveyed points to a DEM (docs/terrain.md, "Gridding points to
// a DEM"; docs/geoprocessing.md, "T6").
//
//   RASTER GRID [<scope>] [method=linear|invdist|invdistnn|nearest|average|...]
//               [cell=<m> | size=<columns>x<rows>] [z=geometry|<property>]
//               [extent=scope|x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>]
//               [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>] | TO SURFACE <name>]
//               [OVERWRITE] [PREVIEW]
//
// The points - and the vertices of lines and areas - the scope takes are
// gridded by GDAL's `vector grid <method>`. The methods are the catalogue's,
// read at run time, so a method GDAL adds is offered with no change here.
//
// Heights. With z=geometry (the default) a vertex's height is the drawing's
// (entity::heightsOf): an entity without a height at every vertex is left out
// and counted (skipped.heightless), because GDAL reads a 2D point as z = 0 -
// a surface pulled down to the datum with no error anywhere. With
// z=<property> GDAL reads the property (--zfield), and an entity without a
// number there is left out and counted (skipped.no_z): GDAL reads a word in
// the field as 0 (measured: "x" on a point at the centre of four at 100 m
// made the centre 0, where it skips a field left null and gives 100).
//
// The grid. GDAL takes a resolution only with an extent, and rounds the
// extent to a whole number of cells by STRETCHING the cells (measured: 10 m
// at a 3 m cell came back as 3 cells of 3.333 m). So the extent defaults to
// the bounds of the vertices used, grown outwards to whole cells from the
// origin - every datum inside the grid, every cell the size asked for, and
// grids of one cell size line up - and a given extent keeps its lower-left
// corner and grows up and right to whole cells. size= takes the extent as it
// is. Cells no datum reaches hold interop::geo::kGridNoData, the surfaces'
// no-data value, declared on the band: GDAL's default of 0 would be read as
// ground at the datum.

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

#include "bindings.hpp"
#include "dem_support.hpp"
#include "geo_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/interop/geo/raster_products.hpp"
#include "katana/interop/terrain_io.hpp"
#include "replies.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr std::string_view kGridOptions[] = {"method", "cell",  "size",  "z",
                                             "extent", "power", "radius"};
// The most cells a grid may have: the surfaces' own cap (surfaceGrid), a
// 5000 x 5000 grid of doubles, 200 MB.
constexpr std::uint64_t kMaxGridCells = 25'000'000;
constexpr const char* kDefaultName = "dem";

// The methods `vector grid` has in this GDAL: its leaves, in its order.
std::vector<std::string> gridMethods()
{
    std::vector<std::string> methods;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (!info.container && info.path.size() == 3 && info.path[0] == "vector" &&
            info.path[1] == "grid") {
            methods.push_back(info.path[2]);
        }
    }
    return methods;
}

std::string listed(const std::vector<std::string>& words)
{
    std::string text;
    for (const std::string& word : words) {
        text += (text.empty() ? "" : ", ") + word;
    }
    return text;
}

bool hasArgument(const gp::AlgorithmSpec& spec, std::string_view name)
{
    return std::ranges::any_of(spec.args, [&](const gp::ArgSpec& arg) { return arg.name == name; });
}

// size=<columns>x<rows> (or a comma between them).
Result<std::pair<int, int>> sizeOption(const std::string& text)
{
    const std::size_t split = text.find_first_of("xX,");
    const auto refuse = [&] {
        return makeError(ErrorCode::InvalidArgument,
                         "size= is columns x rows, two whole numbers from 1: size=400x300",
                         "size=" + text);
    };
    if (split == std::string::npos) {
        return refuse();
    }
    const auto columns = katana::core::parseFiniteDouble(std::string_view(text).substr(0, split));
    const auto rows = katana::core::parseFiniteDouble(std::string_view(text).substr(split + 1));
    if (!columns || !rows || *columns < 1.0 || *rows < 1.0 || *columns != std::floor(*columns) ||
        *rows != std::floor(*rows) || *columns * *rows > static_cast<double>(kMaxGridCells)) {
        return refuse();
    }
    return std::pair<int, int>{static_cast<int>(*columns), static_cast<int>(*rows)};
}

// The number of whole cells that cover `span`, allowing for the rounding of
// a span that is a whole number of cells when worked exactly (100 / 5).
double wholeCells(double span, double cell)
{
    return std::max(1.0, std::ceil(span / cell - 1e-9));
}

// A feature's value of the field at `index`, as a number: a real or an
// integer, or text that reads as one. Anything else - absent, a word - is no
// height.
bool numericField(const gp::Feature& feature, std::size_t index)
{
    if (index >= feature.values.size()) {
        return false;
    }
    const gp::FieldValue& held = feature.values[index];
    if (const auto* real = std::get_if<double>(&held)) {
        return std::isfinite(*real);
    }
    if (std::holds_alternative<std::int64_t>(held)) {
        return true;
    }
    if (const auto* text = std::get_if<std::string>(&held)) {
        return katana::core::parseFiniteDouble(katana::core::trimmed(*text)).has_value();
    }
    return false;
}

// Leaves out, and counts as no_z, the features with no number in the field
// `key`, keeping the scope record's counts true to what GDAL is given.
void keepNumeric(igeo::DrawingDataset& dataset, const std::string& key)
{
    std::size_t dropped = 0;
    for (gp::FeatureTable& table : dataset.set.tables) {
        std::optional<std::size_t> index;
        for (std::size_t f = 0; f < table.fields.size(); ++f) {
            if (table.fields[f].name == key) {
                index = f;
            }
        }
        const std::size_t before = table.features.size();
        std::erase_if(table.features, [&](const gp::Feature& feature) {
            return !index || !numericField(feature, *index);
        });
        const std::size_t removed = before - table.features.size();
        dropped += removed;
        std::size_t& count = table.kind == katana::gis::GeometryKind::Point ? dataset.stats.points
                             : table.kind == katana::gis::GeometryKind::Polygon
                                 ? dataset.stats.polygons
                                 : dataset.stats.lines;
        count -= std::min(count, removed);
    }
    std::erase_if(dataset.set.tables, [](const gp::FeatureTable& table) { return table.features.empty(); });
    if (dropped != 0) {
        dataset.stats.used -= std::min(dataset.stats.used, dropped);
        dataset.stats.skipped["no_z"] += dropped;
    }
}

// The box round every vertex GDAL will read.
katana::geometry::Box2 vertexBounds(const gp::FeatureSet& set)
{
    katana::geometry::Box2 box;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            for (const katana::gis::VectorGeometry& geometry : feature.parts) {
                for (const auto& part : geometry.parts) {
                    for (const katana::gis::GeoPoint& point : part) {
                        box.expand(katana::geometry::Point2(point.x, point.y));
                    }
                }
            }
        }
    }
    return box;
}

std::string real(double number)
{
    return katana::core::formatExactReal(number);
}

} // namespace

Result<Prepared> prepareRasterGrid(Context& context, const Tokens& tokens, std::string_view line)
{
    auto words = splitOptions(tokens, 2, kGridOptions, "RASTER GRID");
    if (!words) {
        return words.error();
    }
    const auto option = [&](const char* key) -> std::optional<std::string> {
        const auto found = words->options.find(key);
        return found == words->options.end() ? std::nullopt : std::optional(found->second);
    };

    // The scope, NAME, TO and the flags.
    const Tokens& rest = words->rest;
    std::optional<katana::cad::ScopeWords> scope;
    std::optional<Target> target;
    std::string name;
    bool preview = false, overwrite = false;
    for (std::size_t at = 0; at < rest.size();) {
        if (rest.is(at, "NAME")) {
            if (at + 1 >= rest.size() || !name.empty()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "NAME takes the reference raster's name, once", rest[at]);
            }
            name = rest[at + 1];
            at += 2;
        } else if (rest.is(at, "TO")) {
            if (target) {
                return makeError(ErrorCode::InvalidArgument, "one TO per line");
            }
            ++at;
            auto parsed = parseTarget(rest, at);
            if (!parsed) {
                return parsed.error();
            }
            target = std::move(parsed).value();
        } else if (rest.is(at, "PREVIEW")) {
            preview = true;
            ++at;
        } else if (rest.is(at, "OVERWRITE")) {
            overwrite = true;
            ++at;
        } else if (!scope && !rest.quoted[at] && katana::cad::isScopeWord(rest[at])) {
            auto parsed = katana::cad::parseScopeWords(rest.words, at);
            if (!parsed) {
                return parsed.error();
            }
            scope = std::move(parsed).value();
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "RASTER GRID takes a scope, method=, cell= or size=, z=, extent=, "
                             "power=, radius=, NAME, TO, OVERWRITE and PREVIEW",
                             rest[at]);
        }
    }

    // The method, from the catalogue.
    const std::string method = katana::core::lowered(option("method").value_or("linear"));
    const std::vector<std::string> methods = gridMethods();
    if (std::ranges::find(methods, method) == methods.end()) {
        return makeError(ErrorCode::InvalidArgument,
                         "method= is one of GDAL's gridding methods: " + listed(methods),
                         "method=" + method);
    }
    const std::vector<std::string> path{"vector", "grid", method};
    auto spec = gp::describe(path);
    if (!spec) {
        return spec.error();
    }
    for (const char* key : {"power", "radius"}) {
        if (option(key) && !hasArgument(*spec, key)) {
            std::vector<std::string> taking;
            for (const std::string& other : methods) {
                auto described = gp::describe({"vector", "grid", other});
                if (described && hasArgument(*described, key)) {
                    taking.push_back(other);
                }
            }
            return makeError(ErrorCode::InvalidArgument,
                             std::string(key) + "= is for the methods " + listed(taking) +
                                 ", not " + method,
                             std::string(key) + "=" + *option(key));
        }
    }
    std::optional<double> power, radius;
    if (const auto text = option("power")) {
        auto number = positiveOption("power", *text);
        if (!number) {
            return number.error();
        }
        power = *number;
    }
    if (const auto text = option("radius")) {
        auto number = positiveOption("radius", *text);
        if (!number) {
            return number.error();
        }
        radius = *number;
    }

    // The grid's size: a cell, or columns and rows - never both.
    if (option("cell") && option("size")) {
        return makeError(ErrorCode::InvalidArgument,
                         "cell= and size= both say how big the grid is; give one");
    }
    std::optional<double> cell;
    if (const auto text = option("cell")) {
        auto number = positiveOption("cell", *text);
        if (!number) {
            return number.error();
        }
        cell = *number;
    }
    std::optional<std::pair<int, int>> size;
    if (const auto text = option("size")) {
        auto parsed = sizeOption(*text);
        if (!parsed) {
            return parsed.error();
        }
        size = *parsed;
    }
    std::optional<std::array<double, 4>> givenExtent;
    if (const auto text = option("extent"); text && !katana::core::equalsIgnoringCase(*text, "scope")) {
        auto box = boxOption("extent", *text);
        if (!box) {
            return box.error();
        }
        givenExtent = *box;
    }
    const std::string zText = option("z").value_or("geometry");
    const bool zFromGeometry = katana::core::equalsIgnoringCase(zText, "geometry");

    // Where the result goes: a reference raster unless TO says otherwise.
    Target to = target.value_or(Target{});
    if (to.kind == Target::Kind::Layer || to.kind == Target::Kind::Selection ||
        to.kind == Target::Kind::Report) {
        return makeError(ErrorCode::Unsupported,
                         "RASTER GRID makes a raster: TO REFERENCE [<name>] keeps it as a "
                         "reference raster, TO FILE <path> writes it, TO SURFACE <name> keeps it "
                         "as a surface");
    }
    if (!name.empty() && !to.name.empty() && to.kind == Target::Kind::Reference) {
        return makeError(ErrorCode::InvalidArgument, "NAME and TO REFERENCE both name the raster; "
                                                     "give one");
    }
    if (to.kind == Target::Kind::File) {
        std::error_code error;
        if (std::filesystem::exists(pathOfUtf8(to.name), error) && !overwrite) {
            return makeError(ErrorCode::AlreadyExists, "the file exists; add OVERWRITE to replace it",
                             to.name);
        }
    }

    // The drawing's points: what the scope takes, heights required unless
    // they come from a property.
    igeo::DrawingDatasetOptions options;
    options.crsWkt = projectCrs(context);
    options.requireHeights = zFromGeometry;
    auto bound = bindDrawing(context, scope.value_or(katana::cad::ScopeWords{}), options);
    if (!bound) {
        return bound.error();
    }
    if (!zFromGeometry) {
        keepNumeric(bound->dataset, zText);
    }
    const Source drawing; // Kind::Drawing
    std::vector<std::string> records{inputRecord("input", drawing), scopeRecord("input", *bound)};
    for (const std::string& warning : bound->dataset.stats.warnings) {
        records.push_back(warningRecord(warning));
    }
    const std::string zRecord = zFromGeometry ? std::string("geometry") : value(zText);
    const std::string head = "grid method=" + method + " algorithm=" + value(gp::pathText(path)) +
                             " z=" + zRecord;
    const gp::FeatureSet& set = bound->dataset.set;
    if (set.tables.empty()) {
        // Nothing to grid is said, as a scope that takes nothing is, and
        // nothing runs.
        records.insert(records.begin(), head + " ran=no");
        Prepared prepared;
        prepared.title = "RASTER GRID";
        prepared.reply = joinRecords(records);
        return prepared;
    }

    // The grid: extent and cell or size.
    const katana::geometry::Box2 data = vertexBounds(set);
    double x0 = 0.0, y0 = 0.0, x1 = 0.0, y1 = 0.0;
    if (givenExtent) {
        x0 = (*givenExtent)[0];
        y0 = (*givenExtent)[1];
        x1 = (*givenExtent)[2];
        y1 = (*givenExtent)[3];
    } else {
        x0 = data.min.x;
        y0 = data.min.y;
        x1 = data.max.x;
        y1 = data.max.y;
    }
    if (!cell && !size) {
        cell = katana::interop::suggestedCellSize(katana::geometry::Box2(
            katana::geometry::Point2(x0, y0), katana::geometry::Point2(x1, y1)));
    }
    int columns = 0, rows = 0;
    double xres = 0.0, yres = 0.0;
    if (cell) {
        if (!givenExtent) {
            // Outwards to whole cells from the origin, so grids of one cell
            // size line up and a datum on the edge is inside.
            x0 = std::floor(x0 / *cell + 1e-9) * *cell;
            y0 = std::floor(y0 / *cell + 1e-9) * *cell;
        }
        const double across = wholeCells(x1 - x0, *cell);
        const double down = wholeCells(y1 - y0, *cell);
        if (across * down > static_cast<double>(kMaxGridCells)) {
            const double fits = std::sqrt((x1 - x0) * (y1 - y0) / static_cast<double>(kMaxGridCells));
            return makeError(ErrorCode::InvalidArgument,
                             "a " + real(*cell) + " cell makes " + real(across) + " x " +
                                 real(down) + " cells, more than " + std::to_string(kMaxGridCells) +
                                 "; a cell of " + fixed3(fits) + " or more fits",
                             "cell=" + real(*cell));
        }
        columns = static_cast<int>(across);
        rows = static_cast<int>(down);
        x1 = x0 + columns * *cell;
        y1 = y0 + rows * *cell;
        xres = yres = *cell;
    } else {
        if (!(x1 > x0) || !(y1 > y0)) {
            return makeError(ErrorCode::InvalidArgument,
                             "the points span no area for size= to divide; give extent=");
        }
        columns = size->first;
        rows = size->second;
        xres = (x1 - x0) / columns;
        yres = (y1 - y0) / rows;
    }
    const std::string extent = real(x0) + "," + real(y0) + "," + real(x1) + "," + real(y1);

    gp::RunRequest request;
    request.path = path;
    request.tokens.push_back("--extent=" + extent);
    if (cell) {
        request.tokens.push_back("--resolution=" + real(*cell) + "," + real(*cell));
    } else {
        request.tokens.push_back("--size=" + std::to_string(columns) + "," + std::to_string(rows));
    }
    request.tokens.push_back("--nodata=" + real(igeo::kGridNoData));
    // Every table a scope made: GDAL grids one layer unless told which.
    for (const gp::FeatureTable& table : set.tables) {
        request.tokens.push_back("--input-layer=" + table.name);
    }
    if (!zFromGeometry) {
        request.tokens.push_back("--zfield=" + zText);
    }
    if (power) {
        request.tokens.push_back("--power=" + real(*power));
    }
    if (radius) {
        request.tokens.push_back("--radius=" + real(*radius));
    }
    request.values.emplace_back("input", gp::ArgValue(gp::DatasetValue(set)));
    request.overwrite = overwrite;
    if (to.kind == Target::Kind::File) {
        request.outputTo = gp::OutputTo::File;
        request.outputPath = to.name;
        request.outputFormat = to.format;
    } else {
        // Written once, where a derived raster is kept (the GDAL verb's rule).
        request.outputTo = gp::OutputTo::Memory;
        request.maxMemoryCells = 0;
        request.spillDirectory = utf8OfPath(derivedFolder(context));
    }

    const std::string cellText = xres == yres ? real(xres) : real(xres) + "," + real(yres);
    const std::string grid = head + " cell=" + cellText + " extent=" + extent +
                             " size=" + std::to_string(columns) + "x" + std::to_string(rows);
    if (preview) {
        if (auto valid = gp::validate(request); !valid) {
            return valid.error();
        }
        records.insert(records.begin(), grid + " preview=yes");
        records.push_back("preview valid=yes changed=no");
        Prepared prepared;
        prepared.title = "RASTER GRID";
        prepared.reply = joinRecords(records);
        return prepared;
    }

    ApplyRequest apply;
    apply.target = to;
    if (to.kind == Target::Kind::Reference || to.kind == Target::Kind::Default) {
        apply.target.kind = Target::Kind::Reference;
        apply.target.name = !to.name.empty() ? to.name : name;
    }
    apply.defaultName = kDefaultName;
    apply.result.operation = gp::pathText(path);
    apply.result.commandName = std::string(katana::core::trimmed(line));

    Prepared prepared;
    prepared.title = "RASTER GRID";
    prepared.work = [request, records, apply, grid](const std::stop_token& stop,
                                                     const Progress& progress) -> Result<Apply> {
        auto outputs = gp::run(request, stop, progress);
        if (!outputs) {
            return outputs.error();
        }
        auto kept = std::make_shared<gp::RunOutputs>(std::move(outputs).value());
        return Apply([kept, records, apply, grid](Context& ctx) -> Result<std::string> {
            const double seconds = kept->seconds;
            auto applied = applyOutputs(ctx, apply, std::move(*kept));
            if (!applied) {
                return applied.error();
            }
            std::vector<std::string> reply{grid + " seconds=" + fixed3(seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(*applied);
            return joinRecords(reply);
        });
    };
    return prepared;
}

} // namespace katana::app::geo
