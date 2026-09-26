// The RASTER SHADE verb (terrain_verbs.hpp, docs/terrain.md "Shading"): an
// elevation source drawn as a picture - hillshade, colour relief, relief over
// hillshade, slope shading, or the band as it is - kept as a derived
// reference raster the plan view and the sheet painter draw, the same on
// katana_cli, katana_mcp and the window's command line, where Terrain >
// Analysis > Terrain Shading builds these lines.
//
//   RASTER SHADE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//                [style=hillshade|relief|relief+hillshade|slope|plain]
//                [azimuth=315] [altitude=45] [z=1] [variant=regular|combined|multidirectional|igor]
//                [ramp=terrain|diverging|slope|grey|<file>] [range=<min>,<max>]
//                [NAME <n>] [save=<file.tif>] [OVERWRITE] [PREVIEW]
//
// The picture is made by GDAL's own algorithms, step by step through a
// RasterChain: hillshade, color-map (the ramp spread over the range, written
// as GDAL's colour-map text), blend --operator hsv-value (relief over
// hillshade: the relief's hue and saturation, the hillshade's value) and
// slope. It is made at display resolution: a raster longer than the
// display copy a reference raster keeps (4096 pixels) is averaged down to it
// first, since a finer picture would only be decimated again to be drawn.
// Every style ends as an RGBA picture, so it is drawn exactly as rendered -
// a single grey band would be stretched over its own range by the reader.

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

constexpr const char* kUsage =
    "RASTER SHADE SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> "
    "[style=hillshade|relief|relief+hillshade|slope|plain] [azimuth=315] [altitude=45] [z=1] "
    "[variant=regular|combined|multidirectional|igor] [ramp=terrain|diverging|slope|grey|<file>] "
    "[range=<min>,<max>] [NAME <n>] [save=<file.tif>] [OVERWRITE] [PREVIEW]";

katana::core::Error refusal(const std::string& why, const std::string& word = {})
{
    return makeError(ErrorCode::InvalidArgument, why, word);
}

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

enum class Style { Hillshade, Relief, ReliefHillshade, Slope, Plain };

struct StyleName {
    Style style;
    const char* word;
    katana::interop::RasterDisplayStyle display;
    const char* ramp; // the ramp it colours with unless ramp= says; "" for none
};

constexpr StyleName kStyles[] = {
    {Style::Hillshade, "hillshade", katana::interop::RasterDisplayStyle::Hillshade, ""},
    {Style::Relief, "relief", katana::interop::RasterDisplayStyle::Relief, "terrain"},
    {Style::ReliefHillshade, "relief+hillshade", katana::interop::RasterDisplayStyle::ReliefHillshade,
     "terrain"},
    {Style::Slope, "slope", katana::interop::RasterDisplayStyle::Slope, "slope"},
    {Style::Plain, "plain", katana::interop::RasterDisplayStyle::Plain, "grey"},
};

const StyleName& styleName(Style style)
{
    for (const StyleName& name : kStyles) {
        if (name.style == style) {
            return name;
        }
    }
    return kStyles[0];
}

bool lit(Style style)
{
    return style == Style::Hillshade || style == Style::ReliefHillshade;
}

struct ShadeWords {
    Style style = Style::Hillshade;
    std::optional<double> azimuth, altitude, zFactor;
    std::optional<std::string> variant;
    std::optional<std::string> ramp;
    std::optional<std::pair<double, double>> range;
    std::optional<std::string> name;
    std::optional<std::string> save;
    bool overwrite = false;
    bool preview = false;
};

Result<ShadeWords> shadeWords(const Tokens& tokens, std::size_t at)
{
    ShadeWords words;
    const std::string takes = "RASTER SHADE takes style=, azimuth=, altitude=, z=, variant=, "
                              "ramp=, range=, NAME, save=, OVERWRITE and PREVIEW";
    while (at < tokens.size()) {
        if (tokens.is(at, "PREVIEW")) {
            words.preview = true;
            ++at;
            continue;
        }
        if (tokens.is(at, "OVERWRITE")) {
            words.overwrite = true;
            ++at;
            continue;
        }
        if (tokens.is(at, "NAME")) {
            if (at + 1 >= tokens.size() || tokens[at + 1].empty()) {
                return refusal("NAME needs the reference raster's name");
            }
            words.name = tokens[at + 1];
            at += 2;
            continue;
        }
        const auto option = keyValue(tokens, at);
        if (!option) {
            return refusal(takes, tokens[at]);
        }
        const auto& [key, text] = *option;
        if (key == "style") {
            const auto found = std::ranges::find_if(kStyles, [&](const StyleName& name) {
                return katana::core::equalsIgnoringCase(name.word, text);
            });
            if (found == std::end(kStyles)) {
                return refusal("style is hillshade, relief, relief+hillshade, slope or plain", text);
            }
            words.style = found->style;
        } else if (key == "azimuth") {
            auto azimuth = numberOption("azimuth", text);
            if (!azimuth || *azimuth < 0.0 || *azimuth > 360.0) {
                return refusal("azimuth is the light's direction, 0 to 360 degrees clockwise "
                               "from north",
                               text);
            }
            words.azimuth = *azimuth;
        } else if (key == "altitude") {
            auto altitude = numberOption("altitude", text);
            if (!altitude || *altitude < 0.0 || *altitude > 90.0) {
                return refusal("altitude is the light's height, 0 to 90 degrees", text);
            }
            words.altitude = *altitude;
        } else if (key == "z") {
            auto factor = positiveOption("z", text);
            if (!factor) {
                return factor.error();
            }
            words.zFactor = *factor;
        } else if (key == "variant") {
            const std::string variant = katana::core::lowered(text);
            if (variant != "regular" && variant != "combined" && variant != "multidirectional" &&
                variant != "igor") {
                return refusal("variant is regular, combined, multidirectional or igor", text);
            }
            words.variant = variant;
        } else if (key == "ramp") {
            if (text.empty()) {
                return refusal("ramp is terrain, diverging, slope, grey or a colour-map file");
            }
            words.ramp = text;
        } else if (key == "range") {
            const std::size_t comma = text.find(',');
            const auto low = katana::core::parseFiniteDouble(text.substr(0, comma));
            const auto high = comma == std::string::npos
                                  ? std::nullopt
                                  : katana::core::parseFiniteDouble(text.substr(comma + 1));
            if (!low || !high || !(*high > *low)) {
                return refusal("range is the values the ramp spans, low before high: "
                               "range=100,140",
                               text);
            }
            words.range = std::pair{*low, *high};
        } else if (key == "save") {
            const std::string extension =
                katana::core::lowered(pathOf(text).extension().string());
            if (extension != ".tif" && extension != ".tiff") {
                return refusal("save= writes a GeoTIFF: a .tif file", text);
            }
            words.save = text;
        } else {
            return refusal(takes, tokens[at]);
        }
        ++at;
    }
    if (!lit(words.style) &&
        (words.azimuth || words.altitude || words.zFactor || words.variant)) {
        return refusal(std::string("azimuth=, altitude=, z= and variant= light a hillshade; "
                                   "style=") +
                       styleName(words.style).word + " has none");
    }
    if (words.style == Style::Hillshade && (words.ramp || words.range)) {
        return refusal("a hillshade is grey; ramp= and range= colour relief, slope and plain");
    }
    return words;
}

// What the verb reads its source as, for a reference raster's default name.
std::string sourceName(const Context& context, const Source& source)
{
    switch (source.kind) {
    case Source::Kind::Surface:
        return source.surface;
    case Source::Kind::File:
        return pathOf(source.path).stem().string();
    case Source::Kind::Raster:
        for (const katana::interop::RasterOverlay& raster : context.reference.rasters()) {
            if (std::to_string(raster.id) == source.raster ||
                katana::core::equalsIgnoringCase(raster.name, source.raster)) {
                return raster.name;
            }
        }
        return source.raster;
    case Source::Kind::Drawing:
        break;
    }
    return "raster";
}

// The picture, made; what the apply keeps and says.
struct Rendered {
    std::filesystem::path file;
    std::vector<std::string> records; // resampled, shade, saved
    std::vector<igeo::LegendEntry> legend;
    std::vector<std::string> warnings;
};

// The range a ramp spans: the one range= gave, else the values'. The
// diverging ramp is made symmetric about zero, so zero is its white, as a
// difference's cut and fill need; a range of one value is widened a unit
// about it, so that value takes the ramp's middle colour.
std::pair<double, double> spanOf(const ShadeWords& words, const std::string& ramp,
                                 std::pair<double, double> values)
{
    if (words.range) {
        return *words.range;
    }
    if (katana::core::equalsIgnoringCase(ramp, "diverging")) {
        const double reach = std::max(std::abs(values.first), std::abs(values.second));
        values = {-reach, reach};
    }
    if (!(values.second > values.first)) {
        values = {values.first - 0.5, values.first + 0.5};
    }
    return values;
}

// The current raster coloured by the ramp over its range: color-map, no-data
// transparent. The legend it was painted with.
Result<std::vector<igeo::LegendEntry>> colour(RasterChain& chain, const ShadeWords& words,
                                              const std::string& defaultRamp,
                                              std::vector<std::string>& records,
                                              const std::stop_token& stop,
                                              const Progress& progress)
{
    auto values = valueRange(chain.current());
    if (!values) {
        return values.error();
    }
    if (!*values) {
        return makeError(ErrorCode::InvalidArgument,
                         "every cell of the raster is no-data: there is nothing to colour");
    }
    const std::string ramp = words.ramp.value_or(defaultRamp);
    const auto [low, high] = spanOf(words, ramp, **values);
    auto facts = chain.info();
    if (!facts) {
        return facts.error();
    }
    std::vector<igeo::LegendEntry> legend;
    std::filesystem::path map;
    if (const igeo::ColourRamp* builtIn = igeo::builtInRamp(ramp)) {
        legend = igeo::spread(*builtIn, low, high);
        auto written = chain.write("ramp.txt", igeo::colourMapText(legend, facts->noDataValue.has_value()));
        if (!written) {
            return written.error();
        }
        map = *written;
    } else {
        // A person's colour-map file, as GDAL reads it; its lines are the legend.
        map = pathOf(ramp);
        auto read = igeo::readColourMap(map, low, high);
        if (!read) {
            return read.error();
        }
        legend = std::move(read).value();
    }
    records.push_back("ramp name=" + value(igeo::builtInRamp(ramp) != nullptr
                                               ? igeo::builtInRamp(ramp)->name
                                               : map.filename().string()) +
                      " min=" + fixed3(low) + " max=" + fixed3(high) +
                      " from=" + (words.range ? "range" : "data"));
    auto coloured = chain.step({"raster", "color-map"},
                               {"--color-map=" + utf8Of(map), "--add-alpha"}, stop, progress);
    if (!coloured) {
        return coloured.error();
    }
    return legend;
}

std::vector<std::string> lightTokens(const ShadeWords& words)
{
    std::vector<std::string> tokens;
    if (words.azimuth) {
        tokens.push_back("--azimuth=" + gdalNumber(*words.azimuth));
    }
    if (words.altitude) {
        tokens.push_back("--altitude=" + gdalNumber(*words.altitude));
    }
    if (words.zFactor) {
        tokens.push_back("--zfactor=" + gdalNumber(*words.zFactor));
    }
    if (words.variant) {
        tokens.push_back("--variant=" + *words.variant);
    }
    return tokens;
}

// A hillshade as a picture: its greys, 1 to 255, drawn as they are, and its
// no-data 0 transparent.
constexpr const char* kHillshadeGreys = "1 1 1 1 255\n255 255 255 255 255\nnv 0 0 0 0\n";

Result<Rendered> render(const gp::DatasetValue& input, const ShadeWords& words,
                        const std::filesystem::path& scratch, const std::stop_token& stop,
                        const Progress& progress)
{
    RasterChain chain(input, scratch);
    Rendered rendered;
    auto facts = chain.info();
    if (!facts) {
        return facts.error();
    }
    // At display resolution: the picture is drawn from a copy of at most
    // this many pixels a side, so a finer one only costs time.
    const int display = katana::interop::RasterImportOptions{}.maxPixels;
    const int longest = std::max(facts->width, facts->height);
    if (longest > display) {
        const int step = (longest + display - 1) / display;
        const int width = (facts->width + step - 1) / step;
        const int height = (facts->height + step - 1) / step;
        auto resized = chain.step({"raster", "resize"},
                                  {"--size=" + std::to_string(width) + "," + std::to_string(height),
                                   "--resampling=average"},
                                  stop, progress);
        if (!resized) {
            return resized.error();
        }
        rendered.records.push_back("resampled from=" + std::to_string(facts->width) + "x" +
                                   std::to_string(facts->height) + " to=" + std::to_string(width) +
                                   "x" + std::to_string(height));
    }
    const StyleName& style = styleName(words.style);
    std::string shade = "shade style=" + std::string(style.word);
    if (lit(words.style)) {
        shade += " azimuth=" + gdalNumber(words.azimuth.value_or(315.0)) +
                 " altitude=" + gdalNumber(words.altitude.value_or(45.0)) +
                 " z=" + gdalNumber(words.zFactor.value_or(1.0)) +
                 " variant=" + words.variant.value_or("regular");
    } else if (words.style == Style::Slope) {
        // Degrees: a picture's legend reads naturally in them, and they are
        // bounded (0 to 90) where percent is not.
        shade += " unit=degree";
    }
    rendered.records.push_back(shade);

    switch (words.style) {
    case Style::Hillshade: {
        auto shaded = chain.step({"raster", "hillshade"}, lightTokens(words), stop, progress);
        if (!shaded) {
            return shaded.error();
        }
        auto map = chain.write("greys.txt", kHillshadeGreys);
        if (!map) {
            return map.error();
        }
        auto greys = chain.step({"raster", "color-map"},
                                {"--color-map=" + utf8Of(*map), "--add-alpha"}, stop, progress);
        if (!greys) {
            return greys.error();
        }
        break;
    }
    case Style::ReliefHillshade: {
        RasterChain light(chain.current(), scratch);
        auto shaded = light.step({"raster", "hillshade"}, lightTokens(words), stop, progress);
        if (!shaded) {
            return shaded.error();
        }
        auto legend = colour(chain, words, style.ramp, rendered.records, stop, progress);
        if (!legend) {
            return legend.error();
        }
        rendered.legend = std::move(legend).value();
        auto blended = chain.step({"raster", "blend"}, {"--operator=hsv-value"}, stop, progress,
                                  {{"overlay", gp::ArgValue(light.current())}});
        if (!blended) {
            return blended.error();
        }
        for (const gp::Diagnostic& warning : light.warnings) {
            rendered.warnings.push_back(warning.message);
        }
        break;
    }
    case Style::Slope: {
        auto slope = chain.step({"raster", "slope"}, {"--unit=degree"}, stop, progress);
        if (!slope) {
            return slope.error();
        }
        [[fallthrough]];
    }
    case Style::Relief:
    case Style::Plain: {
        auto legend = colour(chain, words, style.ramp, rendered.records, stop, progress);
        if (!legend) {
            return legend.error();
        }
        rendered.legend = std::move(legend).value();
        break;
    }
    }
    for (const gp::Diagnostic& warning : chain.warnings) {
        rendered.warnings.push_back(warning.message);
    }
    if (words.save) {
        // The picture as rendered, a tiled DEFLATE GeoTIFF with the source's
        // georeferencing: for delivery, beside the reference raster kept.
        const auto* file = std::get_if<gp::DatasetPath>(&chain.current());
        if (file == nullptr) {
            return makeError(ErrorCode::Internal, "the picture was never written to a file");
        }
        std::error_code error;
        const std::filesystem::path save = pathOf(*words.save);
        if (!save.parent_path().empty()) {
            std::filesystem::create_directories(save.parent_path(), error);
        }
        std::filesystem::copy_file(pathOf(file->path), save,
                                   std::filesystem::copy_options::overwrite_existing, error);
        if (error) {
            return makeError(ErrorCode::FileExportFailure,
                             "could not write the picture: " + error.message(), *words.save);
        }
        rendered.records.push_back("saved file=" + value(utf8Of(save)) + " driver=GTiff");
    }
    static std::atomic<std::uint64_t> made{0};
    auto kept = chain.keep(scratch / ("katana-shade-" + std::to_string(++made) + ".tif"));
    if (!kept) {
        return kept.error();
    }
    rendered.file = *kept;
    return rendered;
}

} // namespace

Result<Prepared> prepareShade(Context& context, const Tokens& tokens, std::string_view line)
{
    std::size_t at = 2;
    auto source = bindTerrainSource(context, tokens, at, "RASTER SHADE");
    if (!source) {
        return source.error();
    }
    auto words = shadeWords(tokens, at);
    if (!words) {
        return words.error();
    }
    if (words->save && !words->overwrite) {
        std::error_code error;
        if (std::filesystem::exists(pathOf(*words->save), error)) {
            return makeError(ErrorCode::AlreadyExists,
                             "the file exists; add OVERWRITE to replace it", *words->save);
        }
    }
    if (words->ramp && igeo::builtInRamp(*words->ramp) == nullptr) {
        std::error_code error;
        if (!std::filesystem::exists(pathOf(*words->ramp), error)) {
            return makeError(ErrorCode::NotFound,
                             "ramp is terrain, diverging, slope, grey or a colour-map file, and "
                             "no file is there",
                             *words->ramp);
        }
    }
    const StyleName& style = styleName(words->style);
    std::string name = words->name.value_or(sourceName(context, source->source) + "-" +
                                            (words->style == Style::ReliefHillshade
                                                 ? std::string("relief-hillshade")
                                                 : std::string(style.word)));
    std::vector<std::string> records{source->record};
    if (words->preview) {
        records.push_back("shade style=" + std::string(style.word) + " name=" + value(name));
        records.push_back("preview valid=yes changed=no");
        return answeredWith("RASTER SHADE", joinedRecords(records));
    }

    Prepared prepared;
    prepared.title = "Terrain Shading";
    const ShadeWords options = *words;
    const DeferredDataset dataset = source->dataset;
    const std::filesystem::path scratch = context.scratch;
    const std::string command(katana::core::trimmed(line));
    const katana::interop::RasterDisplayStyle display = style.display;
    prepared.work = [dataset, options, scratch, records, name, command,
                     display](const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        auto input = dataset();
        if (!input) {
            return input.error();
        }
        auto rendered = render(*input, options, scratch, stop, progress);
        if (!rendered) {
            return rendered.error();
        }
        auto kept = std::make_shared<Rendered>(std::move(rendered).value());
        return Apply([kept, records, name, command, display](Context& ctx) -> Result<std::string> {
            ApplyRequest request;
            request.target.kind = Target::Kind::Reference;
            request.target.name = name;
            request.defaultName = name;
            request.result.commandName = command;
            gp::RunOutputs outputs;
            outputs.file = utf8Of(kept->file);
            auto applied = applyOutputs(ctx, request, std::move(outputs));
            if (!applied) {
                return applied.error();
            }
            // The reference raster is drawn as what it is: a shading.
            for (const Record& record : parseRecords(*applied)) {
                const auto id = record.kind == "output" ? record.get("id") : std::nullopt;
                if (!id) {
                    continue;
                }
                const auto number = katana::core::parseInteger(*id);
                if (katana::interop::RasterOverlay* raster =
                        number ? ctx.reference.findRaster(static_cast<katana::interop::ReferenceId>(*number))
                               : nullptr) {
                    raster->displayStyle = display;
                }
            }
            std::vector<std::string> reply = records;
            reply.insert(reply.end(), kept->records.begin(), kept->records.end());
            reply.push_back(*applied);
            for (const igeo::LegendEntry& entry : kept->legend) {
                reply.push_back("legend value=" + fixed3(entry.value) + " r=" +
                                std::to_string(entry.r) + " g=" + std::to_string(entry.g) +
                                " b=" + std::to_string(entry.b));
            }
            for (const std::string& warning : kept->warnings) {
                reply.push_back(warningRecord(warning));
            }
            return joinedRecords(reply);
        });
    };
    return prepared;
}

std::string shadeUsage()
{
    return kUsage;
}

} // namespace katana::app::geo
