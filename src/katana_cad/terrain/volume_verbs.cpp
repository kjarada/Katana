// The volume verbs (terrain/volume_verbs.hpp, docs/volumes.md). Each reads its
// words, finds the drawing's surfaces by name, hands the geometry to the
// terrain layer's exact methods (terrain/volume.hpp, overlay.hpp,
// section_volume.hpp) and writes what they measured as key=value records.

#include "katana/cad/terrain/volume_verbs.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <filesystem>
#include <format>
#include <fstream>
#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/cad/code_edit.hpp"
#include "katana/cad/csv_table.hpp"
#include "katana/cad/terrain/surface_verbs.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/math/summation.hpp"
#include "katana/terrain/overlay.hpp"
#include "katana/terrain/section_volume.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/volume.hpp"

namespace katana::cad {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::terrain::OverlayRegion;
using katana::terrain::TinSurface;
using Words = std::vector<std::string>;
using SurfacePtr = std::shared_ptr<const TinSurface>;

namespace {

// ---- words and numbers ----------------------------------------------------------------------

bool is(std::string_view word, std::string_view expected)
{
    return katana::core::equalsIgnoringCase(word, expected);
}

// A volume or an area as a reply gives it: to the millimetre (cubic or
// square), as the DEM tools' volumes are, and never "-0.000" - a quantity of
// nothing has no sign.
std::string fixed(double value)
{
    if (std::abs(value) < 0.0005) {
        value = 0.0;
    }
    return std::format("{:.3f}", value);
}

// A number the person typed, said back as it reads back.
std::string exact(double value)
{
    return katana::core::formatExactReal(value);
}

Error usage(std::string_view form)
{
    return makeError(ErrorCode::ParseFailure, "usage: " + std::string(form));
}

constexpr std::string_view kSurfacesUsage =
    "VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>] "
    "[csv=<file> [OVERWRITE]]";
constexpr std::string_view kDatumUsage =
    "VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>] "
    "[csv=<file> [OVERWRITE]]";
constexpr std::string_view kStockpileUsage =
    "VOLUME STOCKPILE <surface> INSIDE <polygon> [base=boundary|lowest] [level=<z>] [EACH] "
    "[bulk=<f>] [shrink=<f>] [csv=<file> [OVERWRITE]]";
constexpr std::string_view kSectionsUsage =
    "VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] [from=<s>] [to=<s>] "
    "[left=<m>] [right=<m>] [method=endarea|prismoidal|both] [curvature=on|off] [bulk=<f>] "
    "[shrink=<f>] [csv=<file> [OVERWRITE]]";
constexpr std::string_view kGridUsage =
    "VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=<f>] [shrink=<f>] "
    "[csv=<file> [OVERWRITE]]";
constexpr std::string_view kDifferenceUsage = "SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]";
constexpr std::string_view kEnvelopeUsage =
    "SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]";

// ---- the options -----------------------------------------------------------------------------

// Every key any VOLUME form takes. None is a WHERE key (scope_verbs.hpp), so
// one may stand after a scope's filter and still be the verb's.
constexpr std::array<std::string_view, 13> kOptionKeys = {
    "bulk", "shrink", "csv", "base", "level", "interval", "from", "to", "left", "right",
    "method", "cell", "curvature"};

struct Options {
    std::map<std::string, std::string, std::less<>> values;

    [[nodiscard]] const std::string* find(std::string_view key) const
    {
        const auto found = values.find(key);
        return found == values.end() ? nullptr : &found->second;
    }
};

// The verb's own key=value words, taken out of `words` wherever they stand;
// what is left is the verb's positional words. A key the form does not take
// is refused by name rather than ignored.
Result<Options> takeOptions(Words& words, std::string_view form,
                            std::span<const std::string_view> allowed)
{
    Options options;
    Words rest;
    for (std::string& word : words) {
        const std::size_t equals = word.find('=');
        if (equals != std::string::npos && equals > 0) {
            const std::string key = katana::core::lowered(std::string_view(word).substr(0, equals));
            if (std::find(kOptionKeys.begin(), kOptionKeys.end(), key) != kOptionKeys.end()) {
                if (std::find(allowed.begin(), allowed.end(), key) == allowed.end()) {
                    return makeError(ErrorCode::InvalidArgument,
                                     std::string(form) + " takes no " + key + "=", word);
                }
                if (options.values.contains(key)) {
                    return makeError(ErrorCode::InvalidArgument, key + "= is given twice", word);
                }
                options.values.emplace(key, word.substr(equals + 1));
                continue;
            }
        }
        rest.push_back(std::move(word));
    }
    words = std::move(rest);
    return options;
}

// A flag word (EACH, OVERWRITE, PREVIEW) wherever it stands, taken out.
bool takeFlag(Words& words, std::string_view flag)
{
    const auto found =
        std::find_if(words.begin(), words.end(), [&](const std::string& word) { return is(word, flag); });
    if (found == words.end()) {
        return false;
    }
    words.erase(found);
    return true;
}

Result<std::optional<double>> numberOption(const Options& options, std::string_view key)
{
    const std::string* text = options.find(key);
    if (text == nullptr) {
        return std::optional<double>{};
    }
    const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(*text));
    if (!value) {
        return makeError(ErrorCode::ParseFailure, std::string(key) + "= is a number",
                         std::string(key) + "=" + *text);
    }
    return std::optional<double>(*value);
}

Result<std::optional<double>> positiveOption(const Options& options, std::string_view key)
{
    auto value = numberOption(options, key);
    if (value && *value && !(**value > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + "= must be more than 0",
                         std::string(key) + "=" + *options.find(key));
    }
    return value;
}

// ---- factors ---------------------------------------------------------------------------------

struct Factors {
    double bulk = 1.0;
    double shrink = 1.0;
    bool given = false;
};

Result<Factors> factorsOf(const Options& options)
{
    Factors factors;
    auto bulk = positiveOption(options, "bulk");
    if (!bulk) {
        return bulk.error();
    }
    auto shrink = positiveOption(options, "shrink");
    if (!shrink) {
        return shrink.error();
    }
    factors.given = bulk->has_value() || shrink->has_value();
    factors.bulk = bulk->value_or(1.0);
    factors.shrink = shrink->value_or(1.0);
    return factors;
}

// The factored quantities, applied here and never to the geometry: the cut
// times bulk=, the fill times shrink=, each as given.
std::string factorFields(const Factors& factors, double cut, double fill)
{
    if (!factors.given) {
        return {};
    }
    const double cutAdjusted = cut * factors.bulk;
    const double fillAdjusted = fill * factors.shrink;
    return " bulk=" + exact(factors.bulk) + " shrink=" + exact(factors.shrink) +
           " cut.adjusted=" + fixed(cutAdjusted) + " fill.adjusted=" + fixed(fillAdjusted) +
           " net.adjusted=" + fixed(fillAdjusted - cutAdjusted);
}

// ---- the CSV file ----------------------------------------------------------------------------

// Written by the one CSV writer of the report verbs (cad/csv_table.hpp), replacing a file
// already there only with OVERWRITE. Written before nothing: the VOLUME verbs change no
// drawing data, so a file that cannot be written fails the reply and leaves nothing half
// done.
using Table = CsvTable;

// ---- surfaces --------------------------------------------------------------------------------

struct Named {
    std::string name; // as the drawing has it
    SurfacePtr surface;
};

Result<Named> surfaceNamed(const Document& document, const std::string& name)
{
    const katana::terrain::NamedSurface* named = document.surfaces().find(name);
    if (named == nullptr || named->surface == nullptr) {
        return makeError(ErrorCode::NotFound,
                         "the drawing has no surface named \"" + name +
                             "\"; SURFACE LIST lists them",
                         name);
    }
    return Named{named->name, named->surface};
}

std::string boxText(const katana::geometry::Box2& box)
{
    return fixed(box.min.x) + "," + fixed(box.min.y) + "," + fixed(box.max.x) + "," +
           fixed(box.max.y);
}

// "ground covers 0.000,0.000,100.000,100.000": what a refusal that two
// surfaces share no ground names of each, so a coordinate-system mistake shows.
std::string coverage(const Named& surface)
{
    return surface.name + " covers " + boxText(surface.surface->bounds());
}

// A refusal that two surfaces share no ground, with what each covers; any
// other error as it was.
Error withCoverage(const Error& error, const Named& first, const Named& second)
{
    if (error.code != ErrorCode::InvalidGeometry) {
        return error;
    }
    return makeError(error.code,
                     error.message + " (" + coverage(first) + "; " + coverage(second) + ")",
                     error.context);
}

// ---- measuring inside polygons -------------------------------------------------------------

// What one method measured over one area.
struct Measure {
    double cut = 0.0;
    double fill = 0.0;
    double area = 0.0; // the plan area measured
    double cutArea = 0.0;
    double fillArea = 0.0;
    std::size_t cells = 0;
    std::string fields{}; // the method's own, for this area's record
};

// How a method's records read.
struct Reading {
    std::string head;          // "volume method=exact base=ground top=design"
    bool levelReading = false; // above= and below= as well (a level or a base)
    bool areas = true;         // cut.area= and fill.area=
    bool cells = false;        // cells=
    // The method counts each cell once across the polygons together (the
    // grid): polygons that overlap are not refused, only said, and a cell on
    // an edge they share is the first polygon's.
    bool countsOnce = false;
    std::string nothing;       // why a measure of no area is refused
};

// One area: none (the whole of the surfaces) or a polygon and its region, with
// the regions of the polygons measured before it - which only a method that
// counts cells (the grid) has any use for.
using Measurer = std::function<Result<Measure>(const DrawingShape* polygon,
                                               const OverlayRegion* region,
                                               std::span<const OverlayRegion> earlier)>;

std::string measureFields(const Reading& reading, const Measure& measure, const Factors& factors)
{
    std::string text = " cut=" + fixed(measure.cut) + " fill=" + fixed(measure.fill) +
                       " net=" + fixed(measure.fill - measure.cut) + " area=" + fixed(measure.area);
    if (reading.levelReading) {
        text += " above=" + fixed(measure.cut) + " below=" + fixed(measure.fill);
    }
    if (reading.areas) {
        text += " cut.area=" + fixed(measure.cutArea) + " fill.area=" + fixed(measure.fillArea);
    }
    if (reading.cells) {
        text += " cells=" + std::to_string(measure.cells);
    }
    return text + measure.fields + factorFields(factors, measure.cut, measure.fill);
}

// What a table says about how it was made, as columns that repeat on every
// row: the reply's own head (method=, the surfaces or the level, the cell) and
// the factors. A file handed on has no reply beside it, and an exact table
// and a grid table of the same polygons would otherwise differ only in their
// numbers.
struct CsvInputs {
    std::vector<std::string> keys;
    std::vector<std::string> values;
};

CsvInputs csvInputs(const Reading& reading, const Factors& factors)
{
    CsvInputs inputs;
    if (const auto head = katana::core::readReplyRecord(reading.head)) {
        for (const auto& [key, value] : head->fields) {
            inputs.keys.push_back(key);
            inputs.values.push_back(value);
        }
    }
    if (factors.given) {
        inputs.keys.insert(inputs.keys.end(), {"bulk", "shrink"});
        inputs.values.insert(inputs.values.end(), {exact(factors.bulk), exact(factors.shrink)});
    }
    return inputs;
}

// The columns of a table of quantities, in the order of csvRow. The first
// eight, and the adjusted three, are where they always were; what the reply
// also says follows them, so a reader that counts columns still finds its own.
std::vector<std::string> csvHeader(const Reading& reading, const Factors& factors,
                                   const CsvInputs& inputs)
{
    std::vector<std::string> header = {"record", "polygon", "entity", "polygon_area",
                                       "area",   "cut",     "fill",   "net"};
    if (factors.given) {
        header.insert(header.end(), {"cut_adjusted", "fill_adjusted", "net_adjusted"});
    }
    if (reading.areas) {
        header.insert(header.end(), {"cut_area", "fill_area"});
    }
    if (reading.levelReading) {
        header.insert(header.end(), {"above", "below"});
    }
    if (reading.cells) {
        header.push_back("cells");
    }
    header.insert(header.end(), inputs.keys.begin(), inputs.keys.end());
    return header;
}

std::vector<std::string> csvRow(std::string kind, std::string index, std::string entity,
                                std::string polygonArea, const Measure& measure,
                                const Factors& factors, const Reading& reading,
                                const CsvInputs& inputs)
{
    std::vector<std::string> row{std::move(kind),         std::move(index),
                                 std::move(entity),       std::move(polygonArea),
                                 fixed(measure.area),     fixed(measure.cut),
                                 fixed(measure.fill),     fixed(measure.fill - measure.cut)};
    if (factors.given) {
        row.push_back(fixed(measure.cut * factors.bulk));
        row.push_back(fixed(measure.fill * factors.shrink));
        row.push_back(fixed(measure.fill * factors.shrink - measure.cut * factors.bulk));
    }
    if (reading.areas) {
        row.insert(row.end(), {fixed(measure.cutArea), fixed(measure.fillArea)});
    }
    if (reading.levelReading) {
        row.insert(row.end(), {fixed(measure.cut), fixed(measure.fill)});
    }
    if (reading.cells) {
        row.push_back(std::to_string(measure.cells));
    }
    row.insert(row.end(), inputs.values.begin(), inputs.values.end());
    return row;
}

std::string polygonName(const DrawingShape& polygon, std::size_t index)
{
    return polygon.entity ? "#" + std::to_string(*polygon.entity)
                          : "polygon " + std::to_string(index + 1);
}

// The plan area two regions share: the first made a 1 m slab over itself, cut
// by the second - the overlay already knows how.
Result<double> sharedArea(const OverlayRegion& a, const OverlayRegion& b)
{
    if (!a.bounds().intersects(b.bounds())) {
        return 0.0;
    }
    auto floor = a.atLevel(0.0);
    auto roof = a.atLevel(1.0);
    if (!floor || !roof) {
        return floor ? roof.error() : floor.error();
    }
    auto slab = katana::terrain::compareSurfaces(*floor, *roof, b);
    if (!slab) {
        return slab.error();
    }
    return slab->fill;
}

// Measures the whole, or each polygon, and writes the records: the scope's,
// one per polygon with EACH, then the total. Polygons that overlap would be
// counted twice in a total, so without EACH they are refused; with EACH each
// is its own record and the total says it counts the overlap twice.
Result<std::string> measureAreas(const Reading& reading,
                                 const std::optional<DrawingShapes>& polygons, bool each,
                                 const Factors& factors, const Measurer& measure,
                                 const Options& options, bool overwrite)
{
    std::vector<std::string> lines;
    Table table;
    const CsvInputs inputs = csvInputs(reading, factors);
    table.header = csvHeader(reading, factors, inputs);

    if (!polygons) {
        auto whole = measure(nullptr, nullptr, {});
        if (!whole) {
            return whole.error();
        }
        if (!(whole->area > 0.0)) {
            return makeError(ErrorCode::InvalidGeometry, reading.nothing);
        }
        lines.push_back(reading.head + measureFields(reading, *whole, factors));
        table.rows.push_back(csvRow("total", "", "", "", *whole, factors, reading, inputs));
    } else {
        if (polygons->scope) {
            // What the boundary's scope took, as every verb that takes a
            // scope says it: a record of its own, its kind first.
            lines.push_back("boundary " + scopeRecord(*polygons->scope));
        }
        // skipped.open= is what bounds no area: an open outline, and a
        // closed one of fewer than three corners.
        // holes= is the rings tagged as holes that joined the polygon they lie
        // in, so they are no polygon of their own and not in used=.
        lines.push_back("polygons used=" + std::to_string(polygons->shapes.size()) +
                        " holes=" + std::to_string(polygons->holesJoined) + " skipped.open=" +
                        std::to_string(polygons->skipped.open + polygons->skipped.tooFew) +
                        " skipped.other=" + std::to_string(polygons->skipped.noOutline));
        if (polygons->holesAlone != 0) {
            // As the feature-table conversion says it of the same rings.
            lines.push_back(
                "warning holes.alone=" + std::to_string(polygons->holesAlone) + " message=" +
                recordValue(std::to_string(polygons->holesAlone) +
                            " rings tagged as holes lie in no polygon of the scope and are "
                            "measured as polygons of their own"));
        }
        if (polygons->shapes.empty()) {
            // A scope that takes no closed outline is said, not refused.
            lines.push_back(reading.head + " ran=no reason=" +
                            recordValue("the scope took no closed polygon"));
            if (const std::string* path = options.find("csv")) {
                // Nothing ran, so nothing is written - the file a run before
                // left there is as it was, and the reply says so rather than
                // leave the person to find it still holds the old numbers.
                lines.push_back("csv file=" + recordValue(*path) + " written=no reason=" +
                                recordValue("the scope took no closed polygon"));
            }
            std::string reply;
            for (const std::string& line : lines) {
                reply += line + "\n";
            }
            reply.pop_back();
            return reply;
        }
        std::vector<OverlayRegion> regions;
        regions.reserve(polygons->shapes.size());
        for (std::size_t i = 0; i < polygons->shapes.size(); ++i) {
            auto region =
                OverlayRegion::fromPolygon(polygons->shapes[i].outline, polygons->shapes[i].holes);
            if (!region) {
                return makeError(region.error().code,
                                 polygonName(polygons->shapes[i], i) + ": " +
                                     region.error().message,
                                 region.error().context);
            }
            regions.push_back(std::move(*region));
        }
        std::vector<std::pair<std::size_t, std::size_t>> overlaps;
        for (std::size_t i = 0; i < regions.size(); ++i) {
            for (std::size_t j = i + 1; j < regions.size(); ++j) {
                auto shared = sharedArea(regions[i], regions[j]);
                if (!shared) {
                    return shared.error();
                }
                // Polygons that share an edge share a segment, of no area;
                // what is counted twice is area, and a square millimetre of
                // it is rounding, not ground.
                if (*shared > 1e-6) {
                    if (!each && !reading.countsOnce) {
                        return makeError(
                            ErrorCode::InvalidGeometry,
                            polygonName(polygons->shapes[i], i) + " and " +
                                polygonName(polygons->shapes[j], j) + " overlap by " +
                                fixed(*shared) +
                                " m2, which one total would count twice; give EACH for a record "
                                "per polygon");
                    }
                    overlaps.emplace_back(i, j);
                }
            }
        }
        katana::math::CompensatedSum cut;
        katana::math::CompensatedSum fill;
        katana::math::CompensatedSum area;
        katana::math::CompensatedSum cutArea;
        katana::math::CompensatedSum fillArea;
        katana::math::CompensatedSum polygonArea;
        std::size_t cells = 0;
        // What the method says of its own about each polygon (a stockpile's
        // base level, whether the toe's own heights were used): on the total
        // when every polygon says the same, which one polygon always does.
        // Where they differ there is no one value to give, and EACH gives each.
        std::optional<std::string> sharedFields;
        bool fieldsAgree = true;
        for (std::size_t i = 0; i < regions.size(); ++i) {
            const DrawingShape& polygon = polygons->shapes[i];
            auto measured =
                measure(&polygon, &regions[i], std::span<const OverlayRegion>(regions).first(i));
            if (!measured) {
                return makeError(measured.error().code,
                                 polygonName(polygon, i) + ": " + measured.error().message,
                                 measured.error().context);
            }
            cut.add(measured->cut);
            fill.add(measured->fill);
            area.add(measured->area);
            cutArea.add(measured->cutArea);
            fillArea.add(measured->fillArea);
            polygonArea.add(regions[i].area());
            cells += measured->cells;
            if (!sharedFields) {
                sharedFields = measured->fields;
            } else if (*sharedFields != measured->fields) {
                fieldsAgree = false;
            }
            const std::string entity = polygon.entity ? std::to_string(*polygon.entity) : "";
            if (each) {
                std::string record = "polygon index=" + std::to_string(i + 1);
                if (polygon.entity) {
                    record += " entity=" + entity;
                }
                record += " polygon.area=" + fixed(regions[i].area());
                if (!polygon.holes.empty()) {
                    record += " holes=" + std::to_string(polygon.holes.size());
                }
                record += measureFields(reading, *measured, factors);
                if (polygon.chorded) {
                    record += " chorded=yes";
                }
                lines.push_back(record);
                table.rows.push_back(csvRow("polygon", std::to_string(i + 1), entity,
                                            fixed(regions[i].area()), *measured, factors, reading,
                                            inputs));
            }
        }
        for (const auto& [i, j] : overlaps) {
            lines.push_back("warning overlap=" + std::to_string(i + 1) + "," +
                            std::to_string(j + 1) + " message=" +
                            recordValue(polygonName(polygons->shapes[i], i) + " and " +
                                        polygonName(polygons->shapes[j], j) +
                                        (reading.countsOnce
                                             ? " overlap: each cell is counted once, in the "
                                               "first, so area= is the union's and "
                                               "polygon.area= counts the overlap twice"
                                             : " overlap: the total counts the ground they "
                                               "share twice")));
        }
        Measure total;
        total.cut = cut.value();
        total.fill = fill.value();
        total.area = area.value();
        total.cutArea = cutArea.value();
        total.fillArea = fillArea.value();
        total.cells = cells;
        if (fieldsAgree && sharedFields) {
            total.fields = *sharedFields;
        }
        if (!(total.area > 0.0)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "nothing inside the polygons was measured: " + reading.nothing);
        }
        lines.push_back(reading.head + " polygons=" + std::to_string(regions.size()) +
                        " polygon.area=" + fixed(polygonArea.value()) +
                        measureFields(reading, total, factors));
        table.rows.push_back(csvRow("total", "", "", fixed(polygonArea.value()), total, factors,
                                    reading, inputs));
    }

    if (const std::string* path = options.find("csv")) {
        auto written = writeCsvTable(table, *path, overwrite);
        if (!written) {
            return written.error();
        }
        lines.push_back(*written);
    }
    std::string reply;
    for (const std::string& line : lines) {
        reply += (reply.empty() ? "" : "\n") + line;
    }
    return reply;
}

// INSIDE <polygon> [EACH] after the positional words at `at`, and nothing
// after them.
struct Inside {
    std::optional<DrawingShapes> polygons{};
    bool each = false;
};

Result<Inside> readInside(const Document& document, const Words& words, std::size_t at,
                          const ScopeViewProvider& views, std::string_view form, bool required)
{
    Inside inside;
    if (at < words.size() && is(words[at], "INSIDE")) {
        ++at;
        auto polygons = readVolumePolygons(document, words, at, views);
        if (!polygons) {
            return polygons.error();
        }
        inside.polygons = std::move(*polygons);
    } else if (required) {
        return usage(form);
    }
    if (at < words.size() && is(words[at], "EACH")) {
        if (!inside.polygons) {
            return makeError(ErrorCode::ParseFailure, "EACH follows INSIDE <polygon>", words[at]);
        }
        inside.each = true;
        ++at;
    }
    if (at < words.size()) {
        return makeError(ErrorCode::ParseFailure,
                         "not a word of this form; usage: " + std::string(form), words[at]);
    }
    return inside;
}

Measure fromComparison(const katana::terrain::SurfaceComparison& comparison)
{
    Measure measure;
    measure.cut = comparison.cut;
    measure.fill = comparison.fill;
    measure.area = comparison.planArea;
    measure.cutArea = comparison.cutArea;
    measure.fillArea = comparison.fillArea;
    return measure;
}

Measure fromDatum(const katana::terrain::DatumVolume& volume)
{
    // A level reading: what stands above is cut, what lies below is fill.
    Measure measure;
    measure.cut = volume.above;
    measure.fill = volume.below;
    measure.area = volume.planArea;
    measure.cutArea = volume.planAreaAbove;
    measure.fillArea = volume.planAreaBelow;
    return measure;
}

// ---- VOLUME SURFACES ---------------------------------------------------------------------------

Result<std::string> volumeSurfaces(const Document& document, Words words,
                                   const ScopeViewProvider& views)
{
    static constexpr std::array<std::string_view, 3> kAllowed = {"bulk", "shrink", "csv"};
    auto options = takeOptions(words, "VOLUME SURFACES", kAllowed);
    if (!options) {
        return options.error();
    }
    const bool overwrite = takeFlag(words, "OVERWRITE");
    if (words.size() < 2) {
        return usage(kSurfacesUsage);
    }
    auto base = surfaceNamed(document, words[0]);
    if (!base) {
        return base.error();
    }
    auto top = surfaceNamed(document, words[1]);
    if (!top) {
        return top.error();
    }
    auto inside = readInside(document, words, 2, views, kSurfacesUsage, false);
    if (!inside) {
        return inside.error();
    }
    auto factors = factorsOf(*options);
    if (!factors) {
        return factors.error();
    }
    Reading reading;
    reading.head = "volume method=exact base=" + recordValue(base->name) +
                   " top=" + recordValue(top->name);
    reading.nothing = "the surfaces have no ground in common there (" + coverage(*base) + "; " +
                      coverage(*top) + ")";
    const TinSurface& existing = *base->surface;
    const TinSurface& design = *top->surface;
    const Measurer measure = [&](const DrawingShape*, const OverlayRegion* region,
                                 std::span<const OverlayRegion>) -> Result<Measure> {
        auto comparison = region == nullptr
                              ? katana::terrain::compareSurfaces(existing, design)
                              : katana::terrain::compareSurfaces(existing, design, *region);
        if (!comparison) {
            return comparison.error();
        }
        return fromComparison(*comparison);
    };
    if (!inside->polygons && !existing.bounds().intersects(design.bounds())) {
        return makeError(ErrorCode::InvalidGeometry,
                         base->name + " covers " + boxText(existing.bounds()) + " and " +
                             top->name + " covers " + boxText(design.bounds()) +
                             ": they have no ground in common");
    }
    return measureAreas(reading, inside->polygons, inside->each, *factors, measure, *options,
                        overwrite);
}

// ---- VOLUME DATUM -------------------------------------------------------------------------------

Result<std::string> volumeDatum(const Document& document, Words words,
                                const ScopeViewProvider& views)
{
    static constexpr std::array<std::string_view, 3> kAllowed = {"bulk", "shrink", "csv"};
    auto options = takeOptions(words, "VOLUME DATUM", kAllowed);
    if (!options) {
        return options.error();
    }
    const bool overwrite = takeFlag(words, "OVERWRITE");
    if (words.size() < 2) {
        return usage(kDatumUsage);
    }
    auto surface = surfaceNamed(document, words[0]);
    if (!surface) {
        return surface.error();
    }
    const auto level = katana::core::parseFiniteDouble(katana::core::trimmed(words[1]));
    if (!level) {
        return makeError(ErrorCode::ParseFailure, "the level is a number", words[1]);
    }
    auto inside = readInside(document, words, 2, views, kDatumUsage, false);
    if (!inside) {
        return inside.error();
    }
    auto factors = factorsOf(*options);
    if (!factors) {
        return factors.error();
    }
    Reading reading;
    reading.head = "volume method=exact surface=" + recordValue(surface->name) +
                   " level=" + exact(*level);
    reading.levelReading = true;
    reading.nothing = "the polygons are off the surface (" + coverage(*surface) + ")";
    const TinSurface& ground = *surface->surface;
    const double datum = *level;
    const Measurer measure = [&](const DrawingShape*, const OverlayRegion* region,
                                 std::span<const OverlayRegion>) -> Result<Measure> {
        auto volume = region == nullptr ? katana::terrain::volumeToDatum(ground, datum)
                                        : katana::terrain::volumeToDatum(ground, datum, *region);
        if (!volume) {
            return volume.error();
        }
        return fromDatum(*volume);
    };
    return measureAreas(reading, inside->polygons, inside->each, *factors, measure, *options,
                        overwrite);
}

// ---- VOLUME STOCKPILE ---------------------------------------------------------------------------

Result<std::string> volumeStockpile(const Document& document, Words words,
                                    const ScopeViewProvider& views)
{
    static constexpr std::array<std::string_view, 5> kAllowed = {"base", "level", "csv", "bulk",
                                                                  "shrink"};
    auto options = takeOptions(words, "VOLUME STOCKPILE", kAllowed);
    if (!options) {
        return options.error();
    }
    const bool overwrite = takeFlag(words, "OVERWRITE");
    if (words.empty()) {
        return usage(kStockpileUsage);
    }
    auto surface = surfaceNamed(document, words[0]);
    if (!surface) {
        return surface.error();
    }
    auto inside = readInside(document, words, 1, views, kStockpileUsage, true);
    if (!inside) {
        return inside.error();
    }
    auto factors = factorsOf(*options);
    if (!factors) {
        return factors.error();
    }
    auto level = numberOption(*options, "level");
    if (!level) {
        return level.error();
    }
    katana::terrain::StockpileOptions stockpile;
    std::string baseWord = "boundary";
    if (const std::string* base = options->find("base")) {
        baseWord = katana::core::lowered(*base);
        if (baseWord != "boundary" && baseWord != "lowest") {
            return makeError(ErrorCode::InvalidArgument,
                             "base= is boundary or lowest; a level is level=<z>", "base=" + *base);
        }
        if (level->has_value()) {
            return makeError(ErrorCode::InvalidArgument,
                             "give base= or level=, not both: level= is a base of its own");
        }
    }
    if (level->has_value()) {
        stockpile.base = katana::terrain::StockpileBase::Level;
        stockpile.level = **level;
        baseWord = "level";
    } else if (baseWord == "lowest") {
        stockpile.base = katana::terrain::StockpileBase::Lowest;
    }
    Reading reading;
    reading.head = "volume method=exact surface=" + recordValue(surface->name) +
                   " base=" + baseWord;
    if (level->has_value()) {
        reading.head += " level=" + exact(**level);
    }
    reading.levelReading = true;
    reading.nothing = "the polygons are off the surface (" + coverage(*surface) + ")";
    const TinSurface& ground = *surface->surface;
    const Measurer measure = [&](const DrawingShape* polygon, const OverlayRegion*,
                                 std::span<const OverlayRegion>) -> Result<Measure> {
        katana::terrain::StockpileOptions these = stockpile;
        these.boundaryHeights = polygon->heights;
        these.holes = polygon->holes;
        auto volume = katana::terrain::stockpileVolume(ground, polygon->outline, these);
        if (!volume) {
            return volume.error();
        }
        Measure measured = fromDatum(volume->volume);
        if (volume->base == katana::terrain::StockpileBase::Boundary) {
            measured.fields = std::string(" heights=") + (volume->boundaryHeights ? "own" : "surface");
        } else if (volume->base == katana::terrain::StockpileBase::Lowest && volume->level) {
            measured.fields = " lowest=" + fixed(*volume->level);
        }
        return measured;
    };
    return measureAreas(reading, inside->polygons, inside->each, *factors, measure, *options,
                        overwrite);
}

// ---- VOLUME GRID --------------------------------------------------------------------------------

Result<std::string> volumeGrid(const Document& document, Words words,
                               const ScopeViewProvider& views)
{
    static constexpr std::array<std::string_view, 4> kAllowed = {"cell", "bulk", "shrink", "csv"};
    auto options = takeOptions(words, "VOLUME GRID", kAllowed);
    if (!options) {
        return options.error();
    }
    const bool overwrite = takeFlag(words, "OVERWRITE");
    if (words.size() < 2) {
        return usage(kGridUsage);
    }
    auto base = surfaceNamed(document, words[0]);
    if (!base) {
        return base.error();
    }
    auto top = surfaceNamed(document, words[1]);
    if (!top) {
        return top.error();
    }
    auto cell = positiveOption(*options, "cell");
    if (!cell) {
        return cell.error();
    }
    if (!cell->has_value()) {
        return makeError(ErrorCode::InvalidArgument,
                         "VOLUME GRID needs the cell: cell=<m>; usage: " + std::string(kGridUsage));
    }
    auto inside = readInside(document, words, 2, views, kGridUsage, false);
    if (!inside) {
        return inside.error();
    }
    if (inside->each) {
        return makeError(ErrorCode::ParseFailure,
                         "VOLUME GRID counts each cell once across all the polygons, so it has "
                         "no EACH; VOLUME SURFACES, DATUM and STOCKPILE take it");
    }
    auto factors = factorsOf(*options);
    if (!factors) {
        return factors.error();
    }
    Reading reading;
    reading.head = "volume method=grid base=" + recordValue(base->name) +
                   " top=" + recordValue(top->name) + " cell=" + exact(**cell);
    reading.areas = false;
    reading.cells = true;
    reading.countsOnce = true;
    reading.nothing = "no cell's centre is on both surfaces (" + coverage(*base) + "; " +
                      coverage(*top) + ")";
    const TinSurface& existing = *base->surface;
    const TinSurface& design = *top->surface;
    const double side = **cell;
    const Measurer measure = [&](const DrawingShape*, const OverlayRegion* region,
                                 std::span<const OverlayRegion> earlier) -> Result<Measure> {
        // A cell on an edge two polygons share is the first polygon's, so the
        // polygons' cells, summed, are those of their union.
        auto grid = katana::terrain::gridVolume(existing, design, side, region, nullptr, earlier);
        if (!grid) {
            return grid.error();
        }
        Measure measured;
        measured.cut = grid->cut;
        measured.fill = grid->fill;
        measured.area = grid->planArea;
        measured.cells = grid->cells;
        return measured;
    };
    return measureAreas(reading, inside->polygons, false, *factors, measure, *options, overwrite);
}

// ---- VOLUME SECTIONS ----------------------------------------------------------------------------

Result<std::string> volumeSections(const Document& document, Words words)
{
    static constexpr std::array<std::string_view, 10> kAllowed = {
        "interval", "from", "to", "left", "right", "method", "curvature", "bulk", "shrink", "csv"};
    auto options = takeOptions(words, "VOLUME SECTIONS", kAllowed);
    if (!options) {
        return options.error();
    }
    const bool overwrite = takeFlag(words, "OVERWRITE");
    if (words.size() != 4 || !is(words[2], "ALIGNMENT")) {
        return usage(kSectionsUsage);
    }
    auto base = surfaceNamed(document, words[0]);
    if (!base) {
        return base.error();
    }
    auto top = surfaceNamed(document, words[1]);
    if (!top) {
        return top.error();
    }
    const katana::entity::Alignment* alignment = document.model().alignments.find(words[3]);
    if (alignment == nullptr) {
        return makeError(ErrorCode::NotFound,
                         "the drawing has no alignment named \"" + words[3] +
                             "\"; ALIGN LIST lists them",
                         words[3]);
    }
    auto solved = katana::geometry::solveAlignment(alignment->horizontal);
    if (!solved) {
        return solved.error();
    }
    auto factors = factorsOf(*options);
    if (!factors) {
        return factors.error();
    }

    katana::terrain::AlignmentVolumeOptions sections;
    auto interval = positiveOption(*options, "interval");
    if (!interval) {
        return interval.error();
    }
    sections.interval = interval->value_or(10.0);
    for (const auto& [key, field] :
         {std::pair<std::string_view, std::optional<double>*>{"from", &sections.from},
          {"to", &sections.to},
          {"left", &sections.left},
          {"right", &sections.right}}) {
        auto value = numberOption(*options, key);
        if (!value) {
            return value.error();
        }
        *field = *value;
    }
    bool endArea = true;
    bool prismoidal = false;
    if (const std::string* method = options->find("method")) {
        const std::string word = katana::core::lowered(*method);
        if (word == "prismoidal") {
            endArea = false;
            prismoidal = true;
        } else if (word == "both") {
            prismoidal = true;
        } else if (word != "endarea") {
            return makeError(ErrorCode::InvalidArgument,
                             "method= is endarea, prismoidal or both", "method=" + *method);
        }
    }
    sections.prismoidal = prismoidal;
    if (const std::string* word = options->find("curvature")) {
        const std::string setting = katana::core::lowered(katana::core::trimmed(*word));
        if (setting == "off") {
            sections.curvature = false;
        } else if (setting != "on") {
            return makeError(ErrorCode::InvalidArgument, "curvature= is on or off",
                             "curvature=" + *word);
        }
    }
    const std::string methodWord =
        endArea ? (prismoidal ? "both" : "endarea") : "prismoidal";
    auto volumes = katana::terrain::volumesAlongAlignment(*base->surface, *top->surface, *solved,
                                                          sections);
    if (!volumes) {
        if (volumes.error().code == ErrorCode::InvalidGeometry) {
            // Nothing to measure: which alignment, and what each surface
            // covers, as the other forms name theirs.
            const Error named = withCoverage(volumes.error(), *base, *top);
            return makeError(named.code, "alignment " + alignment->name + ": " + named.message,
                             named.context);
        }
        return volumes.error();
    }

    // The mass haul ordinate: cut taken as positive and fill as negative,
    // summed from the start - the mass diagram's convention - with the
    // factors applied.
    std::vector<std::string> lines;
    Table table;
    table.header = {"station", "key", "cut_area", "fill_area", "width", "cut", "fill",
                    "cut_total", "fill_total", "mass"};
    if (endArea && prismoidal) {
        table.header.insert(table.header.end(), {"cut_prismoidal", "fill_prismoidal",
                                                 "cut_total_prismoidal", "fill_total_prismoidal",
                                                 "mass_prismoidal"});
    }
    // The columns above are the section table. The ones that follow say how it
    // was made, on every row: with method=prismoidal the plain cut, fill and
    // mass columns hold the prismoidal volumes, which only this column tells
    // from an end-area file, and the factors behind a factored mass column are
    // here too.
    const std::size_t quantityColumns = table.header.size();
    table.header.insert(table.header.end(), {"method", "curvature", "base", "top", "alignment",
                                             "interval", "left", "right"});
    if (factors->given) {
        table.header.insert(table.header.end(), {"bulk", "shrink"});
    }
    katana::math::CompensatedSum cutTotal;
    katana::math::CompensatedSum fillTotal;
    katana::math::CompensatedSum prismCutTotal;
    katana::math::CompensatedSum prismFillTotal;
    std::size_t empty = 0;
    for (std::size_t i = 0; i < volumes->sections.size(); ++i) {
        const katana::terrain::StationSection& section = volumes->sections[i];
        if (!(section.width > 0.0)) {
            ++empty;
        }
        std::string record = "section station=" + fixed(section.station);
        if (section.key) {
            record += " key=yes";
        }
        record += " cut.area=" + fixed(section.cutArea) + " fill.area=" + fixed(section.fillArea) +
                  " width=" + fixed(section.width);
        std::vector<std::string> row{fixed(section.station), section.key ? "yes" : "no",
                                     fixed(section.cutArea), fixed(section.fillArea),
                                     fixed(section.width)};
        if (i > 0) {
            const katana::terrain::SectionPanel& panel = volumes->panels[i - 1];
            const double cut = endArea ? panel.endAreaCut : panel.prismoidalCut;
            const double fill = endArea ? panel.endAreaFill : panel.prismoidalFill;
            cutTotal.add(cut);
            fillTotal.add(fill);
            const double mass = cutTotal.value() * factors->bulk - fillTotal.value() * factors->shrink;
            record += " cut=" + fixed(cut) + " fill=" + fixed(fill) +
                      " cut.total=" + fixed(cutTotal.value()) +
                      " fill.total=" + fixed(fillTotal.value()) + " mass=" + fixed(mass);
            row.insert(row.end(), {fixed(cut), fixed(fill), fixed(cutTotal.value()),
                                   fixed(fillTotal.value()), fixed(mass)});
            if (prismoidal && panel.middle) {
                record += " middle.cut.area=" + fixed(panel.middle->cutArea) +
                          " middle.fill.area=" + fixed(panel.middle->fillArea);
            }
            if (panel.curvature != 0.0) {
                // On a curve: its radius at the middle of the panel and which
                // way it turns, and what the correction did to this panel's
                // volumes (the method of cut= and fill=).
                const katana::terrain::CurvatureShift& shift =
                    endArea ? panel.endAreaShift : panel.prismoidalShift;
                record += " radius=" + fixed(1.0 / std::abs(panel.curvature)) +
                          " turn=" + (panel.curvature > 0.0 ? "left" : "right") +
                          " curvature.cut=" + fixed(shift.cut) +
                          " curvature.fill=" + fixed(shift.fill);
            }
            if (endArea && prismoidal) {
                prismCutTotal.add(panel.prismoidalCut);
                prismFillTotal.add(panel.prismoidalFill);
                const double prismMass = prismCutTotal.value() * factors->bulk -
                                         prismFillTotal.value() * factors->shrink;
                record += " cut.prismoidal=" + fixed(panel.prismoidalCut) +
                          " fill.prismoidal=" + fixed(panel.prismoidalFill) +
                          " mass.prismoidal=" + fixed(prismMass);
                row.insert(row.end(), {fixed(panel.prismoidalCut), fixed(panel.prismoidalFill),
                                       fixed(prismCutTotal.value()),
                                       fixed(prismFillTotal.value()), fixed(prismMass)});
            }
        } else {
            row.resize(quantityColumns);
        }
        row.insert(row.end(), {methodWord, sections.curvature ? "on" : "off", base->name,
                               top->name, alignment->name, exact(sections.interval),
                               fixed(volumes->left), fixed(volumes->right)});
        if (factors->given) {
            row.insert(row.end(), {exact(factors->bulk), exact(factors->shrink)});
        }
        lines.push_back(record);
        table.rows.push_back(std::move(row));
    }

    const std::string range = " base=" + recordValue(base->name) + " top=" +
                              recordValue(top->name) + " alignment=" +
                              recordValue(alignment->name) + " from=" +
                              fixed(volumes->fromStation) + " to=" + fixed(volumes->toStation) +
                              " interval=" + exact(sections.interval) + " left=" +
                              fixed(volumes->left) + " right=" + fixed(volumes->right) +
                              " curvature=" + (sections.curvature ? "on" : "off") +
                              " sections=" + std::to_string(volumes->sections.size()) +
                              " sections.empty=" + std::to_string(empty);
    // With the correction on, how much it moved the total (negative: took
    // away), so a figure that differs from the plain end-area one says by
    // how much.
    const auto totals = [&](const char* method, double cut, double fill,
                            const katana::terrain::CurvatureShift& shift) {
        std::string record = std::string("volume method=") + method + range +
                             " cut=" + fixed(cut) + " fill=" + fixed(fill) +
                             " net=" + fixed(fill - cut) + factorFields(*factors, cut, fill);
        if (sections.curvature) {
            record += " curvature.cut=" + fixed(shift.cut) + " curvature.fill=" + fixed(shift.fill);
        }
        return record;
    };
    if (endArea) {
        lines.push_back(totals("endarea", volumes->endAreaCut, volumes->endAreaFill,
                               volumes->endAreaShift));
    }
    if (prismoidal) {
        std::string record = totals("prismoidal", volumes->prismoidalCut, volumes->prismoidalFill,
                                    volumes->prismoidalShift);
        // The prismoidal correction: what the average end area overstates
        // (or understates) by.
        record += " correction.cut=" + fixed(volumes->endAreaCut - volumes->prismoidalCut) +
                  " correction.fill=" + fixed(volumes->endAreaFill - volumes->prismoidalFill);
        lines.push_back(record);
    }
    if (const std::string* path = options->find("csv")) {
        auto written = writeCsvTable(table, *path, overwrite);
        if (!written) {
            return written.error();
        }
        lines.push_back(*written);
    }
    std::string reply;
    for (const std::string& line : lines) {
        reply += (reply.empty() ? "" : "\n") + line;
    }
    return reply;
}

// ---- SURFACE DIFFERENCE and ENVELOPE -------------------------------------------------------------

// NAME <new> and PREVIEW wherever they stand, taken out of `words`.
struct MakeWords {
    std::optional<std::string> name{};
    bool preview = false;
};

Result<MakeWords> takeMakeWords(Words& words, std::string_view form)
{
    MakeWords made;
    made.preview = takeFlag(words, "PREVIEW");
    for (std::size_t i = 0; i < words.size(); ++i) {
        if (is(words[i], "NAME")) {
            if (i + 1 >= words.size() || made.name) {
                return makeError(ErrorCode::ParseFailure,
                                 "NAME takes the new surface's name, once; usage: " +
                                     std::string(form),
                                 words[i]);
            }
            made.name = words[i + 1];
            words.erase(words.begin() + static_cast<std::ptrdiff_t>(i),
                        words.begin() + static_cast<std::ptrdiff_t>(i) + 2);
            --i;
        }
    }
    if (made.name && katana::core::trimmed(*made.name).empty()) {
        return makeError(ErrorCode::InvalidArgument, "a surface's name cannot be empty");
    }
    return made;
}

// The new surface added to the drawing's - ONE undo step named after the
// verb - or, with PREVIEW, only described. The reply's surface line is
// cad::surfaceRecord, the one record every reply that names a surface writes
// (so it has visible=, and names the style the surface is drawn in, which is
// the drawing's default when `style` is empty or no longer there). `style` is
// the caller's: a difference is a cut/fill map and wants "Cut Fill", an
// envelope holds real levels and must not have it.
Result<std::string> addMadeSurface(Document& document, const MakeWords& made,
                                   const std::string& suggested, std::string source,
                                   katana::terrain::OverlaySurface madeSurface,
                                   std::string firstRecord, std::string_view stepName,
                                   std::string style)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    std::string name = made.name ? *made.name : store.uniqueName(suggested);
    if (made.name && store.find(name) != nullptr) {
        return makeError(ErrorCode::AlreadyExists,
                         "the drawing already has a surface named \"" + name + "\"", name);
    }
    katana::terrain::NamedSurface named;
    named.name = name;
    named.surface = std::make_shared<const TinSurface>(std::move(madeSurface.surface));
    named.source = std::move(source);
    named.style = std::move(style);
    named.visible = true;
    const std::string reply =
        std::move(firstRecord) + "\n" + surfaceRecord(named, document.surfaceStyles());
    if (made.preview) {
        return reply + " preview=yes";
    }
    if (auto added = store.add(std::move(named)); !added) {
        return added.error();
    }
    if (auto committed = document.setSurfaces(std::move(store), std::string(stepName));
        !committed) {
        return committed.error();
    }
    return reply;
}

Result<std::string> surfaceDifference(Document& document, Words words)
{
    auto made = takeMakeWords(words, kDifferenceUsage);
    if (!made) {
        return made.error();
    }
    if (words.size() != 2) {
        return usage(kDifferenceUsage);
    }
    auto base = surfaceNamed(document, words[0]);
    if (!base) {
        return base.error();
    }
    auto top = surfaceNamed(document, words[1]);
    if (!top) {
        return top.error();
    }
    auto difference = katana::terrain::differenceSurface(*base->surface, *top->surface);
    if (!difference) {
        return withCoverage(difference.error(), *base, *top);
    }
    std::string record = "difference base=" + recordValue(base->name) +
                         " top=" + recordValue(top->name) +
                         " pieces=" + std::to_string(difference->pieces) +
                         " retriangulated=" + (difference->retriangulated ? "yes" : "no");
    // The cut/fill style when the drawing still has it; otherwise its default.
    const SurfaceDisplayStyle* cutFill = document.surfaceStyles().find("Cut Fill");
    return addMadeSurface(document, *made, top->name + "-" + base->name,
                          "difference " + top->name + " minus " + base->name,
                          std::move(*difference), std::move(record), "SURFACE DIFFERENCE",
                          cutFill != nullptr ? cutFill->name : std::string());
}

Result<std::string> surfaceEnvelope(Document& document, Words words)
{
    auto made = takeMakeWords(words, kEnvelopeUsage);
    if (!made) {
        return made.error();
    }
    if (words.size() != 3) {
        return usage(kEnvelopeUsage);
    }
    katana::terrain::Envelope which = katana::terrain::Envelope::Upper;
    if (is(words[2], "MIN")) {
        which = katana::terrain::Envelope::Lower;
    } else if (!is(words[2], "MAX")) {
        return makeError(ErrorCode::ParseFailure, "the envelope is MAX or MIN", words[2]);
    }
    auto first = surfaceNamed(document, words[0]);
    if (!first) {
        return first.error();
    }
    auto second = surfaceNamed(document, words[1]);
    if (!second) {
        return second.error();
    }
    auto envelope = katana::terrain::envelopeSurface(*first->surface, *second->surface, which);
    if (!envelope) {
        return withCoverage(envelope.error(), *first, *second);
    }
    const char* side = which == katana::terrain::Envelope::Upper ? "max" : "min";
    std::string record = std::string("envelope first=") + recordValue(first->name) +
                         " second=" + recordValue(second->name) + " side=" + side +
                         " pieces=" + std::to_string(envelope->pieces) +
                         " retriangulated=" + (envelope->retriangulated ? "yes" : "no");
    return addMadeSurface(document, *made, first->name + "-" + second->name + "-" + side,
                          std::string("envelope ") + side + " of " + first->name + " and " +
                              second->name,
                          std::move(*envelope), std::move(record), "SURFACE ENVELOPE",
                          std::string()); // real levels, not a difference about 0: the default style
}

} // namespace

bool isVolumeVerb(std::string_view verb, const std::vector<std::string>& args)
{
    if (is(verb, "VOLUME")) {
        return true;
    }
    return is(verb, "SURFACE") && !args.empty() &&
           (is(args.front(), "DIFFERENCE") || is(args.front(), "ENVELOPE"));
}

Result<std::string> runVolumeVerb(Document& document, const std::vector<std::string>& tokens,
                                  const ScopeViewProvider& views)
{
    if (tokens.size() < 2) {
        return makeError(ErrorCode::ParseFailure,
                         "VOLUME SURFACES | DATUM | STOCKPILE | SECTIONS | GRID ...; HELP VOLUME "
                         "gives every form");
    }
    const std::string& method = tokens[1];
    Words words(tokens.begin() + 2, tokens.end());
    if (is(tokens[0], "SURFACE")) {
        if (is(method, "DIFFERENCE")) {
            return surfaceDifference(document, std::move(words));
        }
        return surfaceEnvelope(document, std::move(words));
    }
    if (is(method, "SURFACES")) {
        return volumeSurfaces(document, std::move(words), views);
    }
    if (is(method, "DATUM")) {
        return volumeDatum(document, std::move(words), views);
    }
    if (is(method, "STOCKPILE")) {
        return volumeStockpile(document, std::move(words), views);
    }
    if (is(method, "SECTIONS")) {
        return volumeSections(document, std::move(words));
    }
    if (is(method, "GRID")) {
        return volumeGrid(document, std::move(words), views);
    }
    return makeError(ErrorCode::ParseFailure,
                     "VOLUME is SURFACES, DATUM, STOCKPILE, SECTIONS or GRID; HELP VOLUME gives "
                     "every form",
                     method);
}

std::string volumeVerbHelp()
{
    return R"(Volumes between the drawing's surfaces (docs/volumes.md). Every VOLUME form changes
nothing; csv=<file> also writes the records as a table (OVERWRITE replaces a file).

VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>] [csv=<file>]
          exact, on the overlay of the two: fill where the top is above the base, cut where
          it is below, net = fill - cut; inside the polygon its edges cut the triangles
VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=] [shrink=] [csv=]
          against a level: what stands above it is cut (above=), what lies below is fill
VOLUME STOCKPILE <surface> INSIDE <polygon> [base=boundary|lowest] [level=<z>] [EACH] [bulk=]
          [shrink=] [csv=]
          a pile (above=, the cut that removes it) or a pit (below=) against a base made
          from its boundary: the boundary's heights (its own at every vertex, else the
          surface's), the lowest point of the surface along it, or level=<z>
VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] [from=] [to=] [left=] [right=]
          [method=endarea|prismoidal|both] [curvature=on|off] [bulk=] [shrink=] [csv=]
          cross sections square to the alignment at whole multiples of the interval and at
          its key stations, each area exact; average end area, or prismoidal with a middle
          section in each pair; one record per section with the running totals and the
          mass haul ordinate (cut positive); left= and right= default to across the common
          ground, which a curve or a corner refuses: sections that cross one another count
          ground twice, and so do two stretches that run side by side, so give a reach
          under the curve's radius or under half the distance between the stretches;
          on a curve the volume is corrected for the centroid of each section travelling
          a different distance than the centreline (Pappus; curvature=off gives the plain
          figure along the centreline, curvature.cut= and curvature.fill= say by how much
          it moved the total)
VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=] [shrink=] [csv=]
          the midpoint rule on cells at whole multiples of the cell: a cross-check; inside
          several polygons each cell is counted once, in the first that holds its centre
          (lots that share an edge, or overlap, are not counted twice, and it has no EACH)
SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]
          the top minus the base as a new surface (the cut/fill map), styled Cut Fill; one
          undo step
SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]
          the higher (MAX) or lower (MIN) of the two as a new surface, in the default
          style (it holds real levels, not a difference); one undo step

<polygon>  #id [#id ...] (closed polylines, curve polylines, circles and ellipses) |
           x,y[,z] x,y[,z] x,y[,z] ... |
           FROM SELECTION | DRAWING | VIEW [id] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY]
           [WHERE k=v ...]; several polygons give one total, refused if they overlap
           (GRID counts each cell once instead), and EACH a record per polygon as well
bulk=      multiplies the cut, shrink= the fill, in the reply only (cut.adjusted=,
           fill.adjusted=, net.adjusted=, and the mass ordinate))";
}

} // namespace katana::cad
