// The colour ramps terrain shading draws with (colour_ramps.hpp).

#include "katana/interop/geo/colour_ramps.hpp"

#include <algorithm>
#include <array>
#include <fstream>
#include <iterator>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"

namespace katana::interop::geo {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

// The colours are ColorBrewer's (Cynthia Brewer, colorbrewer2.org), chosen
// there to stay distinguishable in order and to people with the common
// colour-vision deficiencies: RdBu for the diverging ramp and RdYlGn,
// reversed, for slope, each at five of its eleven classes. The terrain ramp
// follows the hypsometric tints of printed relief maps: green lowland, pale
// tan, brown, white summits.
const std::array<ColourRamp, 4>& ramps()
{
    static const std::array<ColourRamp, 4> all{
        ColourRamp{"terrain",
                   {{0.0, 38, 115, 0},
                    {0.25, 132, 186, 34},
                    {0.5, 245, 232, 142},
                    {0.75, 168, 112, 0},
                    {1.0, 255, 255, 255}}},
        ColourRamp{"diverging",
                   {{0.0, 33, 102, 172},
                    {0.25, 103, 169, 207},
                    {0.5, 247, 247, 247},
                    {0.75, 239, 138, 98},
                    {1.0, 178, 24, 43}}},
        ColourRamp{"slope",
                   {{0.0, 26, 150, 65},
                    {0.25, 166, 217, 106},
                    {0.5, 255, 255, 191},
                    {0.75, 253, 174, 97},
                    {1.0, 215, 25, 28}}},
        ColourRamp{"grey", {{0.0, 0, 0, 0}, {1.0, 255, 255, 255}}},
    };
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
    for (const ColourRamp& ramp : ramps()) {
        if (katana::core::equalsIgnoringCase(ramp.name, name)) {
            return &ramp;
        }
    }
    return nullptr;
}

std::vector<std::string> builtInRampNames()
{
    std::vector<std::string> names;
    for (const ColourRamp& ramp : ramps()) {
        names.push_back(ramp.name);
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
