// The colour ramps terrain shading draws with (colour_ramps.hpp).

#include "katana/interop/geo/colour_ramps.hpp"

#include <algorithm>
#include <fstream>
#include <iterator>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/terrain/colour_ramps.hpp"

namespace katana::interop::geo {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

// The ramps are the terrain layer's one table (katana/terrain/colour_ramps.hpp)
// - the one a surface's display style names - as GDAL's colour map wants them:
// each stop with its place along the range. Built from it once, so a stop here
// is the style's stop (docs/surface_display.md, "The ramps").
const std::vector<ColourRamp>& ramps()
{
    static const std::vector<ColourRamp> all = [] {
        std::vector<ColourRamp> built;
        for (const katana::terrain::NamedColourRamp& named : katana::terrain::colourRamps()) {
            ColourRamp ramp;
            ramp.name = std::string(named.name);
            const std::size_t last = named.stops.size() - 1;
            for (std::size_t i = 0; i < named.stops.size(); ++i) {
                // Evenly spaced: i / 4 is exact in binary (0, .25, .5, .75, 1).
                ramp.stops.push_back({static_cast<double>(i) / static_cast<double>(last),
                                      named.stops[i].r, named.stops[i].g, named.stops[i].b});
            }
            built.push_back(std::move(ramp));
        }
        return built;
    }();
    return all;
}

std::uint8_t channel(double value)
{
    return static_cast<std::uint8_t>(std::clamp(value, 0.0, 255.0));
}

// The words of a colour-map line: blanks, tabs and commas separate.
std::vector<std::string> wordsOf(std::string_view line)
{
    std::vector<std::string> words;
    std::string word;
    for (const char c : line) {
        if (katana::core::isAsciiSpace(c) || c == ',') {
            if (!word.empty()) {
                words.push_back(std::move(word));
                word.clear();
            }
        } else {
            word += c;
        }
    }
    if (!word.empty()) {
        words.push_back(std::move(word));
    }
    return words;
}

} // namespace

const ColourRamp* builtInRamp(std::string_view name)
{
    // The one lookup: the table finds the ramp (any case), this finds its row.
    const katana::terrain::NamedColourRamp* found = katana::terrain::findColourRamp(name);
    if (found == nullptr) {
        return nullptr;
    }
    return &ramps()[static_cast<std::size_t>(found - katana::terrain::colourRamps().data())];
}

std::vector<std::string> builtInRampNames()
{
    std::vector<std::string> names;
    for (const std::string_view name : katana::terrain::colourRampNames()) {
        names.emplace_back(name);
    }
    return names;
}

std::vector<LegendEntry> spread(const ColourRamp& ramp, double low, double high)
{
    std::vector<LegendEntry> entries;
    entries.reserve(ramp.stops.size());
    for (const RampStop& stop : ramp.stops) {
        // The ends exactly: low + 1 * (high - low) need not be high.
        const double value = stop.position <= 0.0   ? low
                             : stop.position >= 1.0 ? high
                                                    : low + stop.position * (high - low);
        entries.push_back({value, stop.r, stop.g, stop.b});
    }
    return entries;
}

std::string colourMapText(const std::vector<LegendEntry>& entries, bool noData)
{
    std::string text;
    for (const LegendEntry& entry : entries) {
        text += katana::core::formatExactReal(entry.value) + " " + std::to_string(entry.r) + " " +
                std::to_string(entry.g) + " " + std::to_string(entry.b) + " 255\n";
    }
    if (noData) {
        text += "nv 0 0 0 0\n";
    }
    return text;
}

Result<std::vector<LegendEntry>> readColourMap(const std::filesystem::path& file, double low,
                                               double high)
{
    std::ifstream stream(file, std::ios::binary);
    if (!stream) {
        return makeError(ErrorCode::NotFound, "no colour-map file there",
                         file.generic_string());
    }
    const std::string bytes{std::istreambuf_iterator<char>(stream), std::istreambuf_iterator<char>()};
    auto decoded = katana::core::decodeText(bytes);
    if (!decoded) {
        return decoded.error();
    }
    std::vector<LegendEntry> entries;
    for (const std::string_view line : katana::core::splitLines(decoded->text)) {
        const std::vector<std::string> words = wordsOf(line);
        if (words.size() < 4) {
            continue;
        }
        std::string first = words[0];
        std::optional<double> value;
        if (!first.empty() && first.back() == '%') {
            first.pop_back();
            if (const auto percent = katana::core::parseFiniteDouble(first)) {
                value = low + *percent / 100.0 * (high - low);
            }
        } else {
            value = katana::core::parseFiniteDouble(first);
        }
        const auto r = katana::core::parseInteger(words[1]);
        const auto g = katana::core::parseInteger(words[2]);
        const auto b = katana::core::parseInteger(words[3]);
        if (!value || !r || !g || !b) {
            continue; // nv, a named colour, a comment
        }
        entries.push_back({*value, channel(static_cast<double>(*r)),
                           channel(static_cast<double>(*g)), channel(static_cast<double>(*b))});
    }
    if (entries.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the colour-map file has no line of a value and its colour",
                         file.generic_string());
    }
    std::ranges::stable_sort(entries, {}, &LegendEntry::value);
    return entries;
}

} // namespace katana::interop::geo
