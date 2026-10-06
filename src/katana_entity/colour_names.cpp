#include "katana/entity/colour_names.hpp"

#include <algorithm>
#include <array>

#include "katana/core/text.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

struct NamedColour {
    std::string_view name; // already folded
    Color colour;
};

// A customisation and a .12da archive carry colour NAMES; the RGB behind a
// name is defined by whoever wrote the file and can be redefined there, so no
// table here can be "right". These are the names such files use as standard,
// at the RGB the same names have in the X11 / CSS colour list - the nearest
// thing to a public definition of what "orange" or "brown" means. Two
// departures: X11's DarkGray (169) is LIGHTER than its Gray (128), an accident
// of history that would draw "dark grey" paler than "grey", so dark and light
// grey are set either side of grey instead; and "green" is X11's Lime
// (0, 255, 0), the pure primary every CAD palette means by it, not X11's
// half-bright Green.
constexpr std::array<NamedColour, 27> kStandardColours = {{
    {"red", {255, 0, 0, 255}},
    {"green", {0, 255, 0, 255}},
    {"blue", {0, 0, 255, 255}},
    {"yellow", {255, 255, 0, 255}},
    {"cyan", {0, 255, 255, 255}},
    {"magenta", {255, 0, 255, 255}},
    {"white", {255, 255, 255, 255}},
    {"black", {0, 0, 0, 255}},
    {"grey", {128, 128, 128, 255}},
    {"orange", {255, 165, 0, 255}},
    {"brown", {165, 42, 42, 255}},
    {"purple", {128, 0, 128, 255}},
    {"pink", {255, 192, 203, 255}},
    {"violet", {238, 130, 238, 255}},
    {"dark red", {139, 0, 0, 255}},
    {"dark green", {0, 100, 0, 255}},
    {"dark blue", {0, 0, 139, 255}},
    {"dark cyan", {0, 139, 139, 255}},
    {"dark magenta", {139, 0, 139, 255}},
    {"dark orange", {255, 140, 0, 255}},
    {"dark grey", {64, 64, 64, 255}},
    {"light grey", {192, 192, 192, 255}},
    {"light blue", {173, 216, 230, 255}},
    {"light green", {144, 238, 144, 255}},
    {"light cyan", {224, 255, 255, 255}},
    {"light yellow", {255, 255, 224, 255}},
    {"light pink", {255, 182, 193, 255}},
}};

// The standard colour whose name is `folded`, a name already through
// foldColourName.
[[nodiscard]] const NamedColour* standardEntry(std::string_view folded)
{
    const auto found =
        std::find_if(kStandardColours.begin(), kStandardColours.end(),
                     [folded](const NamedColour& entry) { return entry.name == folded; });
    return found == kStandardColours.end() ? nullptr : &*found;
}

} // namespace

std::string foldColourName(std::string_view name)
{
    std::string key = katana::core::lowered(name);
    std::replace(key.begin(), key.end(), '_', ' ');
    std::replace(key.begin(), key.end(), '-', ' ');
    // Trimmed AFTER the separators have become blanks, so that one at an end
    // goes as a blank there does and "red_" is "red". The other order, which
    // this had while it was the archive import's own, left "red " - a folded
    // name with a blank at its end, which folded again was "red": two answers
    // from one fold, and a table could then hold "red_" beside the standard
    // red it is another spelling of.
    key = std::string(katana::core::trimmed(key));
    if (key == "gray") {
        key = "grey";
    } else if (key.ends_with(" gray")) {
        key.replace(key.size() - 4, 4, "grey");
    }
    return key;
}

std::optional<Color> standardColour(std::string_view name)
{
    const NamedColour* entry = standardEntry(foldColourName(name));
    return entry == nullptr ? std::nullopt : std::optional<Color>(entry->colour);
}

std::vector<std::string> standardColourNames()
{
    std::vector<std::string> names;
    names.reserve(kStandardColours.size());
    for (const NamedColour& entry : kStandardColours) {
        names.emplace_back(entry.name);
    }
    return names;
}

std::string nearestStandardColour(const Color& colour)
{
    std::string_view best = kStandardColours.front().name;
    long bestDistance = -1;
    for (const NamedColour& entry : kStandardColours) {
        const long dr = static_cast<long>(entry.colour.r) - colour.r;
        const long dg = static_cast<long>(entry.colour.g) - colour.g;
        const long db = static_cast<long>(entry.colour.b) - colour.b;
        const long distance = dr * dr + dg * dg + db * db;
        if (bestDistance < 0 || distance < bestDistance) {
            bestDistance = distance;
            best = entry.name;
        }
    }
    return std::string(best);
}

Status ColourTable::add(std::string name, const Color& colour)
{
    if (!isValidUtf8(name)) {
        return makeError(ErrorCode::InvalidArgument, "a colour name is not valid UTF-8");
    }
    std::string folded = foldColourName(name);
    // Blanks and separators fold away at the ends: a name made of nothing
    // else names nothing.
    if (folded.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a colour name is empty");
    }
    if (standardEntry(folded) != nullptr) {
        return makeError(ErrorCode::InvalidArgument,
                         "a standard colour name cannot be given another colour: \"" + folded +
                             "\" means the same in every drawing",
                         name);
    }
    if (const auto existing = entries_.find(folded); existing != entries_.end()) {
        return makeError(ErrorCode::InvalidArgument,
                         "two colour names read alike: names are compared without regard to "
                         "case, and with '_' and '-' as a blank",
                         "\"" + existing->second.name + "\" and \"" + name + "\"");
    }
    entries_.emplace(std::move(folded), Entry{std::move(name), colour});
    return {};
}

std::optional<Color> ColourTable::find(std::string_view name) const
{
    const auto found = entries_.find(foldColourName(name));
    return found == entries_.end() ? std::nullopt : std::optional<Color>(found->second.colour);
}

std::vector<ColourTable::Entry> ColourTable::entries() const
{
    std::vector<Entry> all;
    all.reserve(entries_.size());
    for (const auto& [folded, entry] : entries_) {
        all.push_back(entry);
    }
    return all;
}

std::optional<Color> resolveColour(const ColourTable& table, std::string_view name)
{
    if (const auto own = table.find(name)) {
        return own;
    }
    return standardColour(name);
}

} // namespace katana::entity
