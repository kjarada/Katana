// The drawing's surfaces and display styles on the command line
// (cad/terrain/surface_verbs.hpp; docs/terrain.md, "Surfaces in the drawing").

#include "katana/cad/terrain/surface_verbs.hpp"

#include <algorithm>
#include <charconv>
#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Color;
using katana::terrain::NamedSurface;
using katana::terrain::SurfaceStore;
using Tokens = std::vector<std::string>;

namespace {

constexpr std::string_view kSurfaceUsage =
    "SURFACE RENAME <name> <new> | STYLE <name> <style> | SHOW <name>...|ALL | "
    "HIDE <name>...|ALL | COPY <name> <new>";
constexpr std::string_view kStyleUsage =
    "SURFACESTYLE LIST [JSON] | INFO <style> | NEW <style> [FROM <style>] [key=value ...] | "
    "SET <style> key=value ... | DELETE <style> | RENAME <style> <new> | DEFAULT <style>";

bool is(const Tokens& tokens, std::size_t at, std::string_view word)
{
    return at < tokens.size() && katana::core::equalsIgnoringCase(tokens[at], word);
}

std::string fixed3(double number)
{
    char buffer[64];
    const auto [end, error] =
        std::to_chars(buffer, buffer + sizeof(buffer), number, std::chars_format::fixed, 3);
    if (error != std::errc()) {
        return "nan";
    }
    std::string text(buffer, end);
    // A value that rounds to nothing has no sign (zmin=-0.000 reads as meant).
    return text == "-0.000" ? std::string("0.000") : text;
}

std::string yesNo(bool value)
{
    return value ? "yes" : "no";
}

katana::core::Error usage(std::string_view why, std::string_view grammar, std::string word = {})
{
    return makeError(ErrorCode::InvalidArgument, std::string(why) + ": " + std::string(grammar),
                     std::move(word));
}

katana::core::Error noSurface(const std::string& name)
{
    return makeError(ErrorCode::NotFound, "no surface has that name; SURFACE LIST names them",
                     name);
}

katana::core::Error noStyle(const std::string& name)
{
    return makeError(ErrorCode::NotFound,
                     "no surface style has that name; SURFACESTYLE LIST names them", name);
}

std::string joined(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        text += (text.empty() ? "" : "\n") + record;
    }
    return text;
}

// ---- SURFACE RENAME, STYLE, SHOW, HIDE, COPY ------------------------------------------------

Result<std::string> renameSurface(Document& document, const Tokens& tokens)
{
    if (tokens.size() != 4) {
        return usage("SURFACE RENAME names a surface and its new name", "SURFACE RENAME <name> <new>");
    }
    const NamedSurface* named = document.surfaces().find(tokens[2]);
    if (named == nullptr) {
        return noSurface(tokens[2]);
    }
    const std::string old = named->name;
    SurfaceStore next = document.surfaces();
    if (auto status = next.rename(old, tokens[3]); !status) {
        return status.error();
    }
    // A corridor names the surface its batters daylight on and a road object
    // the surface it stands on: left behind by a rename they would find no
    // surface, and lose their earthworks and their level, quietly (the next
    // build lists the problem). They follow it in the same step, as the
    // corridors of a renamed road template do (ROADTEMPLATE RENAME).
    if (const Status status = document.corridorsStatus(); !status) {
        return makeError(ErrorCode::InvalidState,
                         "the drawing's corridors cannot be read: nothing is changed",
                         status.error().describe());
    }
    if (const Status status = document.roadObjectsStatus(); !status) {
        return makeError(ErrorCode::InvalidState,
                         "the drawing's road objects cannot be read: nothing is changed",
                         status.error().describe());
    }
    road::CorridorSet corridors = document.corridors();
    std::size_t followingCorridors = 0;
    for (road::Corridor& corridor : corridors.corridors) {
        if (katana::core::equalsIgnoringCase(corridor.surface, old)) {
            corridor.surface = tokens[3];
            ++followingCorridors;
        }
    }
    road::RoadObjectSet objects = document.roadObjects();
    std::size_t followingObjects = 0;
    for (road::RoadObject& object : objects.objects) {
        if (katana::core::equalsIgnoringCase(object.placement.surface, old)) {
            object.placement.surface = tokens[3];
            ++followingObjects;
        }
    }
    auto surfacesStep = document.surfacesCommand(std::move(next), "SURFACE RENAME");
    if (!surfacesStep) {
        return surfacesStep.error();
    }
    auto corridorsStep = document.corridorsCommand(corridors, "SURFACE RENAME");
    if (!corridorsStep) {
        return corridorsStep.error();
    }
    auto objectsStep = document.roadObjectsCommand(objects, "SURFACE RENAME");
    if (!objectsStep) {
        return objectsStep.error();
    }
    auto transaction = std::make_unique<katana::commands::Transaction>("SURFACE RENAME");
    if (*surfacesStep) {
        transaction->add(std::move(*surfacesStep));
    }
    if (*corridorsStep) {
        transaction->add(std::move(*corridorsStep));
    }
    if (*objectsStep) {
        transaction->add(std::move(*objectsStep));
    }
    if (transaction->size() > 0) {
        if (auto status = document.execute(std::move(transaction)); !status) {
            return status.error();
        }
    }
    return "renamed surface=" + recordValue(old) + " name=" + recordValue(tokens[3]) +
           " corridors=" + std::to_string(followingCorridors) +
           " roadobjects=" + std::to_string(followingObjects) + "\n" +
           surfaceRecord(*document.surfaces().find(tokens[3]), document.surfaceStyles());
}

Result<std::string> styleSurface(Document& document, const Tokens& tokens)
{
    if (tokens.size() != 4) {
        return usage("SURFACE STYLE names a surface and a style (\"\" for the drawing's default)",
                     "SURFACE STYLE <name> <style>");
    }
    const NamedSurface* named = document.surfaces().find(tokens[2]);
    if (named == nullptr) {
        return noSurface(tokens[2]);
    }
    // Kept as the style spells itself, so a surface names its style one way.
    std::string style;
    if (!tokens[3].empty()) {
        const SurfaceDisplayStyle* found = document.surfaceStyles().find(tokens[3]);
        if (found == nullptr) {
            return noStyle(tokens[3]);
        }
        style = found->name;
    }
    const std::string name = named->name;
    SurfaceStore next = document.surfaces();
    if (auto status = next.setStyle(name, style); !status) {
        return status.error();
    }
    if (auto status = document.setSurfaces(std::move(next), "SURFACE STYLE"); !status) {
        return status.error();
    }
    return surfaceRecord(*document.surfaces().find(name), document.surfaceStyles());
}

Result<std::string> showSurfaces(Document& document, const Tokens& tokens, bool visible)
{
    const std::string verb = visible ? "SHOW" : "HIDE";
    if (tokens.size() < 3) {
        return usage("SURFACE " + verb + " names the surfaces, or ALL",
                     "SURFACE " + verb + " <name>... | ALL");
    }
    std::vector<std::string> names;
    // ALL is the keyword unless a surface has that name: the tokenizer drops
    // the quotes, so SURFACE HIDE "ALL" - the line the Surfaces panel builds
    // for the check box of a surface called ALL - reaches here as the bare
    // word, and every surface would go with it. A surface's own name wins; to
    // reach all of them in such a drawing, list their names.
    if (tokens.size() == 3 && is(tokens, 2, "ALL") &&
        document.surfaces().find(tokens[2]) == nullptr) {
        for (const NamedSurface& named : document.surfaces().all()) {
            names.push_back(named.name);
        }
    } else {
        // Every name found before anything is changed: one unknown refuses
        // the whole line, as a half-done SHOW would surprise.
        for (std::size_t at = 2; at < tokens.size(); ++at) {
            const NamedSurface* named = document.surfaces().find(tokens[at]);
            if (named == nullptr) {
                return noSurface(tokens[at]);
            }
            names.push_back(named->name);
        }
    }
    SurfaceStore next = document.surfaces();
    std::size_t changed = 0;
    for (const std::string& name : names) {
        if (next.find(name)->visible != visible) {
            (void)next.setVisible(name, visible);
            ++changed;
        }
    }
    if (changed != 0) {
        if (auto status = document.setSurfaces(std::move(next), "SURFACE " + verb); !status) {
            return status.error();
        }
    }
    std::vector<std::string> records;
    for (const std::string& name : names) {
        records.push_back(surfaceRecord(*document.surfaces().find(name), document.surfaceStyles()));
    }
    records.push_back(std::string(visible ? "shown" : "hidden") +
                      " surfaces=" + std::to_string(names.size()) +
                      " changed=" + std::to_string(changed));
    return joined(records);
}

Result<std::string> copySurface(Document& document, const Tokens& tokens)
{
    if (tokens.size() != 4) {
        return usage("SURFACE COPY names a surface and the copy's name", "SURFACE COPY <name> <new>");
    }
    const NamedSurface* named = document.surfaces().find(tokens[2]);
    if (named == nullptr) {
        return noSurface(tokens[2]);
    }
    // The copy SHARES the triangulation: a surface is immutable, so a copy
    // costs a pointer, and an edit of either makes a new one of its own.
    NamedSurface copy = *named;
    copy.name = tokens[3];
    copy.source = "copy of " + named->name;
    // Kept by value: `named` points into the store setSurfaces replaces, so
    // reading it after the commit read freed memory (a crash that came and
    // went with what the allocator had put there since).
    const std::string from = named->name;
    SurfaceStore next = document.surfaces();
    if (auto status = next.add(copy); !status) {
        return status.error();
    }
    if (auto status = document.setSurfaces(std::move(next), "SURFACE COPY"); !status) {
        return status.error();
    }
    return "copied surface=" + recordValue(from) + " name=" + recordValue(copy.name) +
           "\n" + surfaceRecord(*document.surfaces().find(copy.name), document.surfaceStyles());
}

// ---- SURFACESTYLE ---------------------------------------------------------------------------

std::size_t surfacesIn(const Document& document, const SurfaceDisplayStyle& style)
{
    const SurfaceStyleSet& styles = document.surfaceStyles();
    return static_cast<std::size_t>(
        std::ranges::count_if(document.surfaces().all(), [&](const NamedSurface& named) {
            return &styleOf(styles, named) == &style;
        }));
}

std::string styleRecord(const Document& document, const SurfaceDisplayStyle& style)
{
    const SurfaceStyleSet& styles = document.surfaceStyles();
    return "style name=" + recordValue(style.name) +
           " default=" + yesNo(katana::core::equalsIgnoringCase(styles.defaultStyle, style.name)) +
           " surfaces=" + std::to_string(surfacesIn(document, style)) +
           " faces=" + std::string(surfaceFacesWord(style.planFaces)) +
           " triangles=" + (style.showTriangles ? "on" : "off") +
           " boundary=" + (style.showBoundary ? "on" : "off") +
           " contours=" + (style.contours.show ? "on" : "off") +
           " shading=" + std::string(surfaceShadingWord(style.shading)) +
           " faces3d=" + std::string(surfaceFacesWord(style.faces3d)) +
           " description=" + recordValue(style.description);
}

std::string optionsRecord(const SurfaceDisplayStyle& style)
{
    std::string record = "options";
    for (const auto& [key, value] : surfaceStyleOptions(style)) {
        record += " " + key + "=" + recordValue(value);
    }
    return record;
}

// What a style's change refuses: styles a newer Katana wrote, which are kept.
Status stylesEditable(const Document& document)
{
    const Status status = document.surfaceStylesStatus();
    if (!status && status.error().code == ErrorCode::Unsupported) {
        return makeError(ErrorCode::CommandRejected,
                         "the surface styles were written by a newer version of Katana; they are "
                         "kept as they are",
                         status.error().describe());
    }
    return {};
}

// The key=value words from `at` on into `style`.
Status applyOptions(Document& document, SurfaceDisplayStyle& style, const Tokens& tokens,
                    std::size_t at)
{
    for (; at < tokens.size(); ++at) {
        const std::string& word = tokens[at];
        const std::size_t equals = word.find('=');
        if (equals == std::string::npos || equals == 0) {
            return usage("a style's fields are key=value", kStyleUsage, word);
        }
        if (auto status = setSurfaceStyleOption(
                style, katana::core::lowered(word.substr(0, equals)), word.substr(equals + 1),
                document.customisationState().colours);
            !status) {
            return status;
        }
    }
    return {};
}

Result<std::string> listStyles(const Document& document, const Tokens& tokens)
{
    const bool json = tokens.size() == 3 && is(tokens, 2, "JSON");
    if (tokens.size() > 3 || (tokens.size() == 3 && !json)) {
        return usage("SURFACESTYLE LIST takes JSON or nothing", "SURFACESTYLE LIST [JSON]");
    }
    if (json) {
        return surfaceStylesReport(document);
    }
    std::vector<std::string> records;
    for (const SurfaceDisplayStyle& style : document.surfaceStyles().styles) {
        records.push_back(styleRecord(document, style));
    }
    records.push_back("listed styles=" + std::to_string(document.surfaceStyles().styles.size()) +
                      " default=" + recordValue(document.surfaceStyles().defaultStyle));
    if (const Status status = document.surfaceStylesStatus(); !status) {
        records.push_back("warning text=" +
                          recordValue("the project's own styles cannot be read, so the built-in "
                                      "ones stand in: " +
                                      status.error().describe()));
    }
    return joined(records);
}

Result<std::string> styleInfo(const Document& document, const Tokens& tokens)
{
    if (tokens.size() != 3) {
        return usage("SURFACESTYLE INFO names one style", "SURFACESTYLE INFO <style>");
    }
    const SurfaceDisplayStyle* style = document.surfaceStyles().find(tokens[2]);
    if (style == nullptr) {
        return noStyle(tokens[2]);
    }
    return styleRecord(document, *style) + "\n" + optionsRecord(*style);
}

Result<std::string> newStyle(Document& document, const Tokens& tokens)
{
    if (tokens.size() < 3 || tokens[2].empty()) {
        return usage("SURFACESTYLE NEW names the style", kStyleUsage);
    }
    if (auto status = stylesEditable(document); !status) {
        return status.error();
    }
    if (document.surfaceStyles().find(tokens[2]) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "a surface style already has that name",
                         tokens[2]);
    }
    SurfaceDisplayStyle style;
    std::size_t at = 3;
    std::string from;
    if (is(tokens, at, "FROM")) {
        if (at + 1 >= tokens.size()) {
            return usage("FROM names the style to start from", kStyleUsage);
        }
        const SurfaceDisplayStyle* base = document.surfaceStyles().find(tokens[at + 1]);
        if (base == nullptr) {
            return noStyle(tokens[at + 1]);
        }
        style = *base;
        from = base->name;
        at += 2;
    }
    style.name = tokens[2];
    if (from.empty()) {
        style.description.clear();
    }
    if (auto status = applyOptions(document, style, tokens, at); !status) {
        return status.error();
    }
    SurfaceStyleSet styles = document.surfaceStyles();
    styles.styles.push_back(style);
    if (auto status = document.setSurfaceStyles(std::move(styles), "SURFACESTYLE NEW"); !status) {
        return status.error();
    }
    const SurfaceDisplayStyle& made = *document.surfaceStyles().find(style.name);
    return "created style=" + recordValue(made.name) + " from=" + recordValue(from) + "\n" +
           styleRecord(document, made) + "\n" + optionsRecord(made);
}

Result<std::string> setStyle(Document& document, const Tokens& tokens)
{
    if (tokens.size() < 4) {
        return usage("SURFACESTYLE SET names the style and at least one key=value", kStyleUsage);
    }
    if (auto status = stylesEditable(document); !status) {
        return status.error();
    }
    const SurfaceDisplayStyle* found = document.surfaceStyles().find(tokens[2]);
    if (found == nullptr) {
        return noStyle(tokens[2]);
    }
    const std::string name = found->name;
    SurfaceStyleSet styles = document.surfaceStyles();
    SurfaceDisplayStyle& style = *std::ranges::find_if(
        styles.styles, [&](const SurfaceDisplayStyle& each) { return each.name == name; });
    const SurfaceDisplayStyle before = style;
    if (auto status = applyOptions(document, style, tokens, 3); !status) {
        return status.error();
    }
    // The built-ins standing in for a set that could not be read are replaced
    // by the first set given (Document::setSurfacesAndStyles), whatever it
    // holds: that is a change, with an undo step, even when no field of this
    // style moved - so the reply says so rather than "changed=no" over a step
    // that was recorded and a kept text that is gone.
    const bool replacing = !document.surfaceStylesStatus();
    const bool changed = !(style == before) || replacing;
    if (auto status = document.setSurfaceStyles(std::move(styles), "SURFACESTYLE SET"); !status) {
        return status.error();
    }
    const SurfaceDisplayStyle& now = *document.surfaceStyles().find(name);
    return "set style=" + recordValue(name) + " changed=" + yesNo(changed) +
           (replacing ? " replaced=unreadable-styles" : "") + "\n" + styleRecord(document, now) +
           "\n" + optionsRecord(now);
}

Result<std::string> deleteStyle(Document& document, const Tokens& tokens)
{
    if (tokens.size() != 3) {
        return usage("SURFACESTYLE DELETE names one style", "SURFACESTYLE DELETE <style>");
    }
    if (auto status = stylesEditable(document); !status) {
        return status.error();
    }
    const SurfaceDisplayStyle* found = document.surfaceStyles().find(tokens[2]);
    if (found == nullptr) {
        return noStyle(tokens[2]);
    }
    const std::string name = found->name;
    if (katana::core::equalsIgnoringCase(document.surfaceStyles().defaultStyle, name)) {
        return makeError(ErrorCode::InvalidArgument,
                         "it is the default style; make another the default first "
                         "(SURFACESTYLE DEFAULT <style>)",
                         name);
    }
    SurfaceStyleSet styles = document.surfaceStyles();
    std::erase_if(styles.styles, [&](const SurfaceDisplayStyle& each) { return each.name == name; });
    // A surface drawn in it is the Document's refusal (setSurfaceStyles), with
    // the surface named.
    if (auto status = document.setSurfaceStyles(std::move(styles), "SURFACESTYLE DELETE"); !status) {
        return status.error();
    }
    return "deleted style=" + recordValue(name) +
           " styles=" + std::to_string(document.surfaceStyles().styles.size());
}

Result<std::string> renameStyle(Document& document, const Tokens& tokens)
{
    if (tokens.size() != 4) {
        return usage("SURFACESTYLE RENAME names a style and its new name",
                     "SURFACESTYLE RENAME <style> <new>");
    }
    if (auto status = stylesEditable(document); !status) {
        return status.error();
    }
    const SurfaceDisplayStyle* found = document.surfaceStyles().find(tokens[2]);
    if (found == nullptr) {
        return noStyle(tokens[2]);
    }
    const std::string old = found->name;
    const std::string& name = tokens[3];
    const SurfaceDisplayStyle* other = document.surfaceStyles().find(name);
    if (other != nullptr && other != found) {
        return makeError(ErrorCode::AlreadyExists, "a surface style already has that name", name);
    }
    SurfaceStyleSet styles = document.surfaceStyles();
    for (SurfaceDisplayStyle& style : styles.styles) {
        if (style.name == old) {
            style.name = name;
        }
    }
    if (katana::core::equalsIgnoringCase(styles.defaultStyle, old)) {
        styles.defaultStyle = name;
    }
    // Every surface drawn in it follows, in the same step.
    SurfaceStore surfaces = document.surfaces();
    std::size_t moved = 0;
    for (const NamedSurface& named : document.surfaces().all()) {
        if (katana::core::equalsIgnoringCase(named.style, old)) {
            (void)surfaces.setStyle(named.name, name);
            ++moved;
        }
    }
    if (auto status = document.setSurfacesAndStyles(std::move(surfaces), std::move(styles),
                                                     "SURFACESTYLE RENAME");
        !status) {
        return status.error();
    }
    return "renamed style=" + recordValue(old) + " name=" + recordValue(name) +
           " surfaces=" + std::to_string(moved);
}

Result<std::string> defaultStyle(Document& document, const Tokens& tokens)
{
    if (tokens.size() != 3) {
        return usage("SURFACESTYLE DEFAULT names one style", "SURFACESTYLE DEFAULT <style>");
    }
    if (auto status = stylesEditable(document); !status) {
        return status.error();
    }
    const SurfaceDisplayStyle* found = document.surfaceStyles().find(tokens[2]);
    if (found == nullptr) {
        return noStyle(tokens[2]);
    }
    SurfaceStyleSet styles = document.surfaceStyles();
    styles.defaultStyle = found->name;
    if (auto status = document.setSurfaceStyles(std::move(styles), "SURFACESTYLE DEFAULT");
        !status) {
        return status.error();
    }
    return "default style=" + recordValue(document.surfaceStyles().defaultStyle);
}

// ---- the option words -----------------------------------------------------------------------

Result<bool> onOff(std::string_view key, std::string_view value)
{
    for (const std::string_view on : {"on", "yes", "true", "1"}) {
        if (katana::core::equalsIgnoringCase(value, on)) {
            return true;
        }
    }
    for (const std::string_view off : {"off", "no", "false", "0"}) {
        if (katana::core::equalsIgnoringCase(value, off)) {
            return false;
        }
    }
    return makeError(ErrorCode::InvalidArgument, std::string(key) + " is on or off",
                     std::string(key) + "=" + std::string(value));
}

Result<Color> colourOf(std::string_view key, std::string_view value,
                       const katana::entity::ColourTable& colours)
{
    if (value.starts_with('#')) {
        auto hex = Color::fromHex(value);
        if (hex) {
            return *hex;
        }
    } else if (const auto named = katana::entity::resolveColour(colours, value)) {
        return *named;
    }
    return makeError(ErrorCode::InvalidArgument,
                     std::string(key) + " is a colour: #RRGGBB, #RRGGBBAA or a colour name",
                     std::string(key) + "=" + std::string(value));
}

Result<double> numberOf(std::string_view key, std::string_view value)
{
    const auto number = katana::core::parseFiniteDouble(value);
    if (!number) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + " is a number",
                         std::string(key) + "=" + std::string(value));
    }
    return *number;
}

Result<int> countOf(std::string_view key, std::string_view value)
{
    const auto number = katana::core::parseInteger(value);
    if (!number || *number < 0 || *number > 1'000'000) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + " is a whole number",
                         std::string(key) + "=" + std::string(value));
    }
    return static_cast<int>(*number);
}

Result<std::vector<ColourStop>> stopsOf(std::string_view value,
                                        const katana::entity::ColourTable& colours)
{
    std::vector<ColourStop> stops;
    if (katana::core::trimmed(value).empty()) {
        return stops;
    }
    std::size_t start = 0;
    while (start <= value.size()) {
        const std::size_t end = std::min(value.find(';', start), value.size());
        const std::string_view item = katana::core::trimmed(value.substr(start, end - start));
        const std::size_t colon = item.find(':');
        if (colon == std::string_view::npos) {
            return makeError(ErrorCode::InvalidArgument,
                             "stops are value:colour pairs separated by ';': stops=0:#1A9850;2:"
                             "#91CF60",
                             "stops=" + std::string(value));
        }
        auto number = numberOf("stops", item.substr(0, colon));
        if (!number) {
            return number.error();
        }
        auto colour = colourOf("stops", item.substr(colon + 1), colours);
        if (!colour) {
            return colour.error();
        }
        stops.push_back(ColourStop{*number, *colour});
        if (end == value.size()) {
            break;
        }
        start = end + 1;
    }
    return stops;
}

std::string stopsText(const std::vector<ColourStop>& stops)
{
    std::string text;
    for (const ColourStop& stop : stops) {
        text += (text.empty() ? "" : ";") + katana::core::formatExactReal(stop.value) + ":" +
                stop.colour.toHex();
    }
    return text;
}

} // namespace

// ---- the module's entry points ----------------------------------------------------------------

std::string surfaceStylesReport(const Document& document)
{
    const std::string set = surfaceStylesToJson(document.surfaceStyles());
    const Status status = document.surfaceStylesStatus();
    if (status) {
        return set;
    }
    // The built-ins stand in for a set this build cannot read, and the plain
    // SURFACESTYLE LIST says so in a warning record. The JSON must as well, or
    // an agent reading it is told the drawing has the built-ins at version 1
    // while the project keeps a set it cannot see - and its next edit is then
    // refused. Added here and not in surfaceStylesToJson, which is the form a
    // save writes into the project. No exceptions: the text is this build's own
    // output, and a parse that somehow failed leaves the set without the note.
    nlohmann::ordered_json root = nlohmann::ordered_json::parse(set, nullptr, false);
    if (root.is_discarded() || !root.is_object()) {
        return set;
    }
    const katana::core::Error& why = status.error();
    root["standing_in"] = {{"code", std::string(katana::core::toString(why.code))},
                           {"message", why.describe()}};
    // A set a newer Katana wrote is kept as it is and every edit refused; a
    // damaged one is replaced by the first set given (docs/terrain.md).
    root["editable"] = why.code != ErrorCode::Unsupported;
    return root.dump(-1, ' ', false, nlohmann::ordered_json::error_handler_t::replace);
}

std::string surfaceRecord(const NamedSurface& named, const SurfaceStyleSet& styles)
{
    const katana::terrain::TinSurface& surface = *named.surface;
    const katana::geometry::Box2& box = surface.bounds();
    return "surface name=" + recordValue(named.name) +
           " triangles=" + std::to_string(surface.triangleCount()) +
           " points=" + std::to_string(surface.vertexCount()) + " bounds=" + fixed3(box.min.x) +
           "," + fixed3(box.min.y) + "," + fixed3(box.max.x) + "," + fixed3(box.max.y) +
           " zmin=" + fixed3(surface.minElevation()) + " zmax=" + fixed3(surface.maxElevation()) +
           " source=" + (named.source.empty() ? std::string() : recordValue(named.source)) +
           " style=" + recordValue(styleOf(styles, named).name) + " visible=" + yesNo(named.visible);
}

Result<std::vector<std::string>> importSurfaces(Document& document,
                                                katana::commands::CommandPtr command,
                                                std::vector<NamedSurface> surfaces,
                                                std::string stepName)
{
    if (surfaces.empty()) {
        if (command) {
            if (auto status = document.execute(std::move(command)); !status) {
                return status.error();
            }
        }
        return std::vector<std::string>{};
    }
    SurfaceStore next = document.surfaces();
    std::vector<std::string> names;
    for (NamedSurface& surface : surfaces) {
        surface.name = next.uniqueName(surface.name);
        names.push_back(surface.name);
        if (auto status = next.add(std::move(surface)); !status) {
            return status.error();
        }
    }
    const std::string step = command ? std::string(command->name()) : stepName;
    auto surfacesStep = document.surfacesCommand(std::move(next), step);
    if (!surfacesStep) {
        return surfacesStep.error();
    }
    auto transaction = std::make_unique<katana::commands::Transaction>(step);
    if (command) {
        transaction->add(std::move(command));
    }
    if (*surfacesStep) {
        transaction->add(std::move(*surfacesStep));
    }
    if (transaction->size() != 0) {
        if (auto status = document.execute(std::move(transaction)); !status) {
            return status.error();
        }
    }
    std::vector<std::string> records;
    for (const std::string& name : names) {
        records.push_back(surfaceRecord(*document.surfaces().find(name), document.surfaceStyles()));
    }
    return records;
}

bool isSurfaceVerb(const Tokens& tokens)
{
    if (tokens.empty()) {
        return false;
    }
    if (katana::core::equalsIgnoringCase(tokens[0], "SURFACESTYLE")) {
        return true;
    }
    if (!katana::core::equalsIgnoringCase(tokens[0], "SURFACE") || tokens.size() < 2) {
        return false;
    }
    for (const std::string_view word : {"RENAME", "STYLE", "SHOW", "HIDE", "COPY"}) {
        if (katana::core::equalsIgnoringCase(tokens[1], word)) {
            return true;
        }
    }
    return false;
}

Result<std::string> runSurfaceVerb(Document& document, const Tokens& tokens)
{
    if (is(tokens, 0, "SURFACE")) {
        if (is(tokens, 1, "RENAME")) {
            return renameSurface(document, tokens);
        }
        if (is(tokens, 1, "STYLE")) {
            return styleSurface(document, tokens);
        }
        if (is(tokens, 1, "SHOW")) {
            return showSurfaces(document, tokens, true);
        }
        if (is(tokens, 1, "HIDE")) {
            return showSurfaces(document, tokens, false);
        }
        if (is(tokens, 1, "COPY")) {
            return copySurface(document, tokens);
        }
        return usage("not a word of this SURFACE family", kSurfaceUsage,
                     tokens.size() > 1 ? tokens[1] : std::string());
    }
    if (!is(tokens, 0, "SURFACESTYLE")) {
        return usage("not a surface verb", kStyleUsage, tokens.empty() ? std::string() : tokens[0]);
    }
    if (tokens.size() == 1 || is(tokens, 1, "LIST")) {
        return listStyles(document, tokens.size() == 1 ? Tokens{"SURFACESTYLE", "LIST"} : tokens);
    }
    if (is(tokens, 1, "INFO")) {
        return styleInfo(document, tokens);
    }
    if (is(tokens, 1, "NEW")) {
        return newStyle(document, tokens);
    }
    if (is(tokens, 1, "SET")) {
        return setStyle(document, tokens);
    }
    if (is(tokens, 1, "DELETE")) {
        return deleteStyle(document, tokens);
    }
    if (is(tokens, 1, "RENAME")) {
        return renameStyle(document, tokens);
    }
    if (is(tokens, 1, "DEFAULT")) {
        return defaultStyle(document, tokens);
    }
    return usage("SURFACESTYLE takes LIST, INFO, NEW, SET, DELETE, RENAME or DEFAULT", kStyleUsage,
                 tokens[1]);
}

std::string surfaceVerbSummary()
{
    return "Surfaces  SURFACE RENAME <name> <new> | STYLE <name> <style> | SHOW|HIDE <name>...|ALL\n"
           "          | COPY <name> <new>   the drawing's surfaces, saved with it, one undo step\n"
           "          each (SURFACE LIST|INFO|REMOVE|FROM|EXPORT: the geoprocessing verbs)\n"
           "          SURFACESTYLE LIST [JSON] | INFO | NEW <style> [FROM <style>] [key=value]\n"
           "          | SET <style> key=value ... | DELETE | RENAME | DEFAULT   the styles a\n"
           "          surface is drawn in (HELP SURFACESTYLE: every key)\n";
}

std::string surfaceVerbHelp()
{
    return std::string(kSurfaceUsage) + "\n" + std::string(kStyleUsage) +
           "\n"
           "A surface is named in any case; a name with blanks is quoted. SURFACE STYLE <name> \"\"\n"
           "gives it back to the drawing's default style. SHOW and HIDE change what the views\n"
           "draw only: a hidden surface is still sampled, measured and exported.\n"
           "A style's keys, for NEW and SET (INFO prints them, to be typed back):\n"
           "  description=<text>\n"
           "  triangles=on|off trianglecolour=<c>   boundary=on|off boundarycolour=<c>\n"
           "  points=on|off pointcolour=<c>         breaklines=on|off breaklinecolour=<c>\n"
           "  faces=none|flat|elevation|slope|aspect opacity=0..1   (the plan view)\n"
           "  contours=on|off interval=<m> major=<every n> base=<level> minorcolour=<c>\n"
           "  majorcolour=<c> labels=on|off\n"
           "  ramp=<name> banded=on|off bands=<n> stops=<v>:<c>;<v>:<c>... (empty clears)\n"
           "  min=<v> max=<v> (empty: the surface's own range) flatcolour=<c>\n"
           "  shading=automatic|shaded|edges|wireframe|hidden faces3d=flat|elevation|slope|aspect\n"
           "  (the 3D view). A colour <c> is #RRGGBB, #RRGGBBAA or a colour name.\n"
           "Built in: Standard (the default), Triangles, Contours, Elevation Bands, Slope Bands,\n"
           "Cut Fill and Outline. A style a surface is drawn in, or the default, is not deleted.\n";
}

Status setSurfaceStyleOption(SurfaceDisplayStyle& style, std::string_view key,
                             std::string_view value, const katana::entity::ColourTable& colours)
{
    // Worked on a copy and kept only whole: a refused value leaves the style
    // as it was.
    SurfaceDisplayStyle next = style;
    const auto flag = [&](bool& field) -> Status {
        auto on = onOff(key, value);
        if (!on) {
            return on.error();
        }
        field = *on;
        return {};
    };
    const auto colour = [&](Color& field) -> Status {
        auto parsed = colourOf(key, value, colours);
        if (!parsed) {
            return parsed.error();
        }
        field = *parsed;
        return {};
    };
    const auto real = [&](double& field) -> Status {
        auto parsed = numberOf(key, value);
        if (!parsed) {
            return parsed.error();
        }
        field = *parsed;
        return {};
    };
    const auto count = [&](int& field) -> Status {
        auto parsed = countOf(key, value);
        if (!parsed) {
            return parsed.error();
        }
        field = *parsed;
        return {};
    };
    const auto faces = [&](SurfaceFaces& field, bool noneAllowed) -> Status {
        const auto parsed = parseSurfaceFaces(value);
        if (!parsed || (!noneAllowed && *parsed == SurfaceFaces::None)) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(key) + (noneAllowed
                                                     ? " is none, flat, elevation, slope or aspect"
                                                     : " is flat, elevation, slope or aspect"),
                             std::string(key) + "=" + std::string(value));
        }
        field = *parsed;
        return {};
    };
    const auto optional = [&](std::optional<double>& field) -> Status {
        if (katana::core::trimmed(value).empty()) {
            field.reset();
            return {};
        }
        auto parsed = numberOf(key, value);
        if (!parsed) {
            return parsed.error();
        }
        field = *parsed;
        return {};
    };

    Status status;
    if (key == "description") {
        // Read as a typed text, as the annotation and sheet verbs read theirs
        // (core::unescapeTyped): two backslashes are one. SURFACESTYLE INFO
        // prints the description through recordValue, which doubles every
        // backslash, so reading it plain doubled them again on each trip.
        next.description = katana::core::unescapeTyped(value);
        if (next.description.find_first_of("\r\n") != std::string::npos) {
            status = makeError(ErrorCode::InvalidArgument,
                               "description holds a line break (a backslash before an n is one; "
                               "type two backslashes for a backslash)",
                               "description=" + std::string(value));
        }
    } else if (key == "triangles") {
        status = flag(next.showTriangles);
    } else if (key == "trianglecolour") {
        status = colour(next.triangleColour);
    } else if (key == "boundary") {
        status = flag(next.showBoundary);
    } else if (key == "boundarycolour") {
        status = colour(next.boundaryColour);
    } else if (key == "points") {
        status = flag(next.showPoints);
    } else if (key == "pointcolour") {
        status = colour(next.pointColour);
    } else if (key == "breaklines") {
        status = flag(next.showBreaklines);
    } else if (key == "breaklinecolour") {
        status = colour(next.breaklineColour);
    } else if (key == "faces") {
        status = faces(next.planFaces, true);
    } else if (key == "opacity") {
        status = real(next.planOpacity);
    } else if (key == "contours") {
        status = flag(next.contours.show);
    } else if (key == "interval") {
        status = real(next.contours.interval);
    } else if (key == "major") {
        status = count(next.contours.majorEvery);
    } else if (key == "base") {
        status = real(next.contours.base);
    } else if (key == "minorcolour") {
        status = colour(next.contours.minorColour);
    } else if (key == "majorcolour") {
        status = colour(next.contours.majorColour);
    } else if (key == "labels") {
        status = flag(next.contours.labelMajor);
    } else if (key == "ramp") {
        // Kept as the ramp spells itself, so a style names a ramp one way.
        next.ramp = std::string(value);
        for (const std::string_view ramp : surfaceRampNames()) {
            if (katana::core::equalsIgnoringCase(ramp, value)) {
                next.ramp = std::string(ramp);
            }
        }
    } else if (key == "banded") {
        status = flag(next.banded);
    } else if (key == "bands") {
        status = count(next.bands);
    } else if (key == "stops") {
        auto stops = stopsOf(value, colours);
        if (!stops) {
            status = stops.error();
        } else {
            next.stops = std::move(*stops);
        }
    } else if (key == "min") {
        status = optional(next.rangeMin);
    } else if (key == "max") {
        status = optional(next.rangeMax);
    } else if (key == "flatcolour") {
        status = colour(next.flatColour);
    } else if (key == "shading") {
        const auto shading = parseSurfaceShading(value);
        if (!shading) {
            status = makeError(ErrorCode::InvalidArgument,
                               "shading is automatic, shaded, edges, wireframe or hidden",
                               "shading=" + std::string(value));
        } else {
            next.shading = *shading;
        }
    } else if (key == "faces3d") {
        status = faces(next.faces3d, false);
    } else {
        status = makeError(ErrorCode::InvalidArgument,
                           "a surface style has no such key; HELP SURFACESTYLE lists them",
                           std::string(key));
    }
    if (!status) {
        return status;
    }
    style = std::move(next);
    return {};
}

std::vector<std::pair<std::string, std::string>> surfaceStyleOptions(const SurfaceDisplayStyle& s)
{
    const auto onOffText = [](bool value) { return std::string(value ? "on" : "off"); };
    const auto real = [](double value) { return katana::core::formatExactReal(value); };
    const auto optional = [&](const std::optional<double>& value) {
        return value ? real(*value) : std::string();
    };
    return {
        {"description", s.description},
        {"triangles", onOffText(s.showTriangles)},
        {"trianglecolour", s.triangleColour.toHex()},
        {"boundary", onOffText(s.showBoundary)},
        {"boundarycolour", s.boundaryColour.toHex()},
        {"points", onOffText(s.showPoints)},
        {"pointcolour", s.pointColour.toHex()},
        {"breaklines", onOffText(s.showBreaklines)},
        {"breaklinecolour", s.breaklineColour.toHex()},
        {"faces", std::string(surfaceFacesWord(s.planFaces))},
        {"opacity", real(s.planOpacity)},
        {"contours", onOffText(s.contours.show)},
        {"interval", real(s.contours.interval)},
        {"major", std::to_string(s.contours.majorEvery)},
        {"base", real(s.contours.base)},
        {"minorcolour", s.contours.minorColour.toHex()},
        {"majorcolour", s.contours.majorColour.toHex()},
        {"labels", onOffText(s.contours.labelMajor)},
        {"ramp", s.ramp},
        {"banded", onOffText(s.banded)},
        {"bands", std::to_string(s.bands)},
        {"stops", stopsText(s.stops)},
        {"min", optional(s.rangeMin)},
        {"max", optional(s.rangeMax)},
        {"flatcolour", s.flatColour.toHex()},
        {"shading", std::string(surfaceShadingWord(s.shading))},
        {"faces3d", std::string(surfaceFacesWord(s.faces3d))},
    };
}

} // namespace katana::cad
