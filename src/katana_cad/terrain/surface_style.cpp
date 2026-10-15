// The surface display styles' data: the built-in set, the style a surface is
// drawn in, the checks and the versioned JSON form (cad/terrain/
// surface_style.hpp; docs/terrain.md, "Surfaces in the drawing"). The colour
// ramps themselves are surface_ramps.cpp's (docs/surface_display.md).

#include "katana/cad/terrain/surface_style.hpp"

#include <array>
#include <cmath>
#include <string>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Color;

namespace {

constexpr std::string_view kFormat = "katana-surface-styles";

// Colours as their RGB; opaque.
constexpr Color rgb(std::uint8_t r, std::uint8_t g, std::uint8_t b)
{
    return Color{r, g, b, 255};
}

} // namespace

const SurfaceDisplayStyle* SurfaceStyleSet::find(std::string_view name) const
{
    for (const SurfaceDisplayStyle& style : styles) {
        if (katana::core::equalsIgnoringCase(style.name, name)) {
            return &style;
        }
    }
    return nullptr;
}

// ---- the built-in set -------------------------------------------------------------------

SurfaceStyleSet builtInSurfaceStyles()
{
    SurfaceStyleSet set;
    set.defaultStyle = "Standard";

    // Its outline in plan and its faces shaded by height in 3D: what every
    // surface was drawn as before there were styles, so a drawing that never
    // chose one looks as it did.
    SurfaceDisplayStyle standard;
    standard.name = "Standard";
    standard.description = "The outline in plan; shaded by elevation in 3D";
    set.styles.push_back(standard);

    SurfaceDisplayStyle triangles;
    triangles.name = "Triangles";
    triangles.description = "Every triangle and the outline in plan; shaded with edges in 3D";
    triangles.showTriangles = true;
    triangles.shading = SurfaceStyle::ShadedWithEdges;
    set.styles.push_back(triangles);

    SurfaceDisplayStyle contours;
    contours.name = "Contours";
    contours.description = "Contours every 1 m, every fifth major and labelled, and the outline";
    contours.contours.show = true;
    contours.contours.interval = 1.0;
    contours.contours.majorEvery = 5;
    contours.contours.labelMajor = true;
    set.styles.push_back(contours);

    SurfaceDisplayStyle elevation;
    elevation.name = "Elevation Bands";
    elevation.description = "Faces in ten bands of elevation over the terrain ramp";
    elevation.planFaces = SurfaceFaces::Elevation;
    elevation.ramp = "terrain";
    elevation.banded = true;
    elevation.bands = 10;
    set.styles.push_back(elevation);

    // Grades a designer reads: 2% the least a surface drains at, 5% about the
    // steepest a footpath is laid at, 10% and 20% the steep road and the
    // steep batter, 33% (1 on 3) the steepest batter mown. The colours are
    // ColorBrewer 2.0's six-class RdYlGn (Brewer and Harrower, Penn State)
    // from green, flat, to red, steep.
    SurfaceDisplayStyle slope;
    slope.name = "Slope Bands";
    slope.description = "Faces banded by slope at 0, 2, 5, 10, 20 and 33 percent";
    slope.planFaces = SurfaceFaces::Slope;
    slope.faces3d = SurfaceFaces::Slope;
    slope.ramp = "slope";
    slope.banded = true;
    slope.stops = {{0.0, rgb(0x1A, 0x98, 0x50)},  {2.0, rgb(0x91, 0xCF, 0x60)},
                   {5.0, rgb(0xD9, 0xEF, 0x8B)},  {10.0, rgb(0xFE, 0xE0, 0x8B)},
                   {20.0, rgb(0xFC, 0x8D, 0x59)}, {33.0, rgb(0xD7, 0x30, 0x27)}};
    set.styles.push_back(slope);

    // For a difference surface (top minus base): below 0 is cut, blue; above
    // is fill, red; 0, no change, white. ColorBrewer 2.0's seven-class RdBu,
    // its order turned so blue is low, on stops symmetric about 0.
    SurfaceDisplayStyle cutFill;
    cutFill.name = "Cut Fill";
    cutFill.description = "A difference surface: cut blue below 0, fill red above";
    cutFill.planFaces = SurfaceFaces::Elevation;
    cutFill.ramp = "diverging";
    cutFill.stops = {{-2.0, rgb(0x21, 0x66, 0xAC)}, {-1.0, rgb(0x67, 0xA9, 0xCF)},
                     {-0.5, rgb(0xD1, 0xE5, 0xF0)}, {0.0, rgb(0xF7, 0xF7, 0xF7)},
                     {0.5, rgb(0xFD, 0xDB, 0xC7)},  {1.0, rgb(0xEF, 0x8A, 0x62)},
                     {2.0, rgb(0xB2, 0x18, 0x2B)}};
    set.styles.push_back(cutFill);

    SurfaceDisplayStyle outline;
    outline.name = "Outline";
    outline.description = "The outline alone; a wireframe in 3D";
    outline.shading = SurfaceStyle::Wireframe;
    set.styles.push_back(outline);
    return set;
}

const SurfaceDisplayStyle& styleOf(const SurfaceStyleSet& styles,
                                   const katana::terrain::NamedSurface& surface)
{
    if (!surface.style.empty()) {
        if (const SurfaceDisplayStyle* own = styles.find(surface.style)) {
            return *own;
        }
    }
    if (const SurfaceDisplayStyle* fallback = styles.find(styles.defaultStyle)) {
        return *fallback;
    }
    // A set with neither - made by hand, never by the Document, which keeps
    // its default among its styles - is drawn in the built-in Standard.
    static const SurfaceDisplayStyle kStandard = builtInSurfaceStyles().styles.front();
    return kStandard;
}

// ---- the checks -------------------------------------------------------------------------

namespace {

Status refuse(const SurfaceDisplayStyle& style, std::string field, std::string why)
{
    return makeError(ErrorCode::InvalidArgument,
                     "style \"" + style.name + "\": " + field + " " + std::move(why),
                     "field=" + field);
}

bool isRamp(std::string_view name)
{
    for (const std::string_view ramp : surfaceRampNames()) {
        if (katana::core::equalsIgnoringCase(ramp, name)) {
            return true;
        }
    }
    return false;
}

} // namespace

Status checkSurfaceStyle(const SurfaceDisplayStyle& style)
{
    if (katana::core::trimmed(style.name).empty()) {
        return makeError(ErrorCode::InvalidArgument, "a surface style needs a name", "field=name");
    }
    // Text that reaches the model is UTF-8 (core/text_encoding.hpp): the set is
    // written to the project as JSON, whose writer throws on anything else - out
    // of a save, which promises a Status. THE door for a style's text; every
    // way to a style (SURFACESTYLE NEW/SET/RENAME, the dialog, a project's
    // own JSON) ends in checkSurfaceStyleSet. Checked before anything echoes
    // the name, and the message does not: it is the bytes that are wrong.
    if (!katana::core::isValidUtf8(style.name)) {
        return makeError(ErrorCode::InvalidArgument, "a surface style's name is not valid UTF-8",
                         "field=name");
    }
    if (!katana::core::isValidUtf8(style.description)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a surface style's description is not valid UTF-8", "field=description");
    }
    // A line names a style in quotes, and has no way to type a quote inside
    // them; a line break would end the line.
    if (style.name.find_first_of("\"\r\n") != std::string::npos) {
        return refuse(style, "name", "holds a double quote or a line break, which a command line "
                                     "cannot name");
    }
    // The same for a description: SURFACESTYLE INFO prints it for the person to
    // type back, and the tokenizer reads a quote as the end of the word.
    if (style.description.find_first_of("\"\r\n") != std::string::npos) {
        return refuse(style, "description",
                      "holds a double quote or a line break, which a command line cannot carry");
    }
    if (style.name != katana::core::trimmed(style.name)) {
        return refuse(style, "name", "begins or ends with a blank");
    }
    const SurfaceContours& contours = style.contours;
    if (!std::isfinite(contours.interval) || contours.interval <= 0.0) {
        return refuse(style, "interval", "must be a finite length greater than zero");
    }
    if (!std::isfinite(contours.base)) {
        return refuse(style, "base", "must be a finite level");
    }
    if (contours.majorEvery < 0) {
        return refuse(style, "major", "must be 0 (none) or a count of intervals");
    }
    if (!std::isfinite(style.planOpacity) || style.planOpacity < 0.0 || style.planOpacity > 1.0) {
        return refuse(style, "opacity", "must be from 0 to 1");
    }
    if (style.bands < 2 || style.bands > kMaximumSurfaceBands) {
        return refuse(style, "bands", "must be 2 to " + std::to_string(kMaximumSurfaceBands));
    }
    if (style.stops.size() > static_cast<std::size_t>(kMaximumSurfaceBands)) {
        return refuse(style, "stops", "may be at most " + std::to_string(kMaximumSurfaceBands));
    }
    for (std::size_t i = 0; i < style.stops.size(); ++i) {
        if (!std::isfinite(style.stops[i].value)) {
            return refuse(style, "stops", "must be finite values");
        }
        if (i > 0 && !(style.stops[i].value > style.stops[i - 1].value)) {
            return refuse(style, "stops", "must rise from one to the next");
        }
    }
    if ((style.rangeMin && !std::isfinite(*style.rangeMin)) ||
        (style.rangeMax && !std::isfinite(*style.rangeMax))) {
        return refuse(style, "min", "and max must be finite values");
    }
    if (style.rangeMin && style.rangeMax && !(*style.rangeMin < *style.rangeMax)) {
        return refuse(style, "min", "must be below max");
    }
    if (!isRamp(style.ramp)) {
        std::string names;
        for (const std::string_view ramp : surfaceRampNames()) {
            names += (names.empty() ? "" : ", ") + std::string(ramp);
        }
        return refuse(style, "ramp", "\"" + style.ramp + "\" is no ramp; the ramps are " + names);
    }
    return {};
}

Status checkSurfaceStyleSet(const SurfaceStyleSet& styles)
{
    if (styles.styles.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a drawing keeps at least one surface style");
    }
    for (std::size_t i = 0; i < styles.styles.size(); ++i) {
        const SurfaceDisplayStyle& style = styles.styles[i];
        if (auto status = checkSurfaceStyle(style); !status) {
            return status;
        }
        for (std::size_t j = 0; j < i; ++j) {
            if (katana::core::equalsIgnoringCase(styles.styles[j].name, style.name)) {
                return makeError(ErrorCode::InvalidArgument, "two surface styles have one name",
                                 "style=" + style.name);
            }
        }
    }
    if (styles.find(styles.defaultStyle) == nullptr) {
        return makeError(ErrorCode::InvalidArgument,
                         "the default surface style is not one of the styles",
                         "default=" + styles.defaultStyle);
    }
    return {};
}

// ---- words ------------------------------------------------------------------------------

namespace {

constexpr std::array<std::pair<SurfaceFaces, std::string_view>, 5> kFaces{{
    {SurfaceFaces::None, "none"},
    {SurfaceFaces::Flat, "flat"},
    {SurfaceFaces::Elevation, "elevation"},
    {SurfaceFaces::Slope, "slope"},
    {SurfaceFaces::Aspect, "aspect"},
}};

constexpr std::array<std::pair<SurfaceStyle, std::string_view>, 5> kShading{{
    {SurfaceStyle::Automatic, "automatic"},
    {SurfaceStyle::Shaded, "shaded"},
    {SurfaceStyle::ShadedWithEdges, "edges"},
    {SurfaceStyle::Wireframe, "wireframe"},
    {SurfaceStyle::Hidden, "hidden"},
}};

} // namespace

std::string_view surfaceFacesWord(SurfaceFaces faces)
{
    for (const auto& [value, word] : kFaces) {
        if (value == faces) {
            return word;
        }
    }
    return "none";
}

std::optional<SurfaceFaces> parseSurfaceFaces(std::string_view word)
{
    for (const auto& [value, name] : kFaces) {
        if (katana::core::equalsIgnoringCase(name, word)) {
            return value;
        }
    }
    return std::nullopt;
}

std::string_view surfaceShadingWord(SurfaceStyle shading)
{
    for (const auto& [value, word] : kShading) {
        if (value == shading) {
            return word;
        }
    }
    return "automatic";
}

std::optional<SurfaceStyle> parseSurfaceShading(std::string_view word)
{
    for (const auto& [value, name] : kShading) {
        if (katana::core::equalsIgnoringCase(name, word)) {
            return value;
        }
    }
    return std::nullopt;
}

// ---- the JSON form ----------------------------------------------------------------------
//
// Every field of every style is written, so a reader never supplies a default
// for what a writer meant: a later build may change a default without a new
// version. A key a reader does not know is skipped, so a field added later
// within version 1 reads in an older build as the style without it.

namespace {

using Ordered = nlohmann::ordered_json;
using Json = nlohmann::json;

Ordered colourJson(const Color& colour)
{
    return colour.toHex();
}

Ordered styleJson(const SurfaceDisplayStyle& style)
{
    Ordered stops = Ordered::array();
    for (const ColourStop& stop : style.stops) {
        stops.push_back(Ordered{{"value", stop.value}, {"colour", colourJson(stop.colour)}});
    }
    const SurfaceContours& c = style.contours;
    return Ordered{
        {"name", style.name},
        {"description", style.description},
        {"triangles", {{"show", style.showTriangles}, {"colour", colourJson(style.triangleColour)}}},
        {"boundary", {{"show", style.showBoundary}, {"colour", colourJson(style.boundaryColour)}}},
        {"points", {{"show", style.showPoints}, {"colour", colourJson(style.pointColour)}}},
        {"breaklines",
         {{"show", style.showBreaklines}, {"colour", colourJson(style.breaklineColour)}}},
        {"faces", std::string(surfaceFacesWord(style.planFaces))},
        {"opacity", style.planOpacity},
        {"contours",
         {{"show", c.show},
          {"interval", c.interval},
          {"major", c.majorEvery},
          {"base", c.base},
          {"minor_colour", colourJson(c.minorColour)},
          {"major_colour", colourJson(c.majorColour)},
          {"labels", c.labelMajor}}},
        {"ramp", style.ramp},
        {"banded", style.banded},
        {"bands", style.bands},
        {"stops", std::move(stops)},
        {"min", style.rangeMin ? Ordered(*style.rangeMin) : Ordered()},
        {"max", style.rangeMax ? Ordered(*style.rangeMax) : Ordered()},
        {"flat_colour", colourJson(style.flatColour)},
        {"shading", std::string(surfaceShadingWord(style.shading))},
        {"faces3d", std::string(surfaceFacesWord(style.faces3d))},
    };
}

// The reader's refusals, as ParseFailure naming where.
struct Malformed {
    std::string what;
};

const Json* member(const Json& object, const char* key)
{
    const auto found = object.find(key);
    return found == object.end() ? nullptr : &*found;
}

template <typename T> void read(const Json& object, const char* key, T& out, const std::string& where)
{
    const Json* value = member(object, key);
    if (value == nullptr) {
        return;
    }
    try {
        out = value->get<T>();
    } catch (const Json::exception&) {
        throw Malformed{where + ": " + key + " is not what it should be"};
    }
}

void readColour(const Json& object, const char* key, Color& out, const std::string& where)
{
    std::string text;
    read(object, key, text, where);
    if (text.empty()) {
        return;
    }
    const auto colour = Color::fromHex(text);
    if (!colour) {
        throw Malformed{where + ": " + key + " is not #RRGGBB"};
    }
    out = *colour;
}

// {"show": ..., "colour": ...} under `key`.
void readShown(const Json& object, const char* key, bool& show, Color& colour,
               const std::string& where)
{
    const Json* part = member(object, key);
    if (part == nullptr) {
        return;
    }
    if (!part->is_object()) {
        throw Malformed{where + ": " + key + " is not an object"};
    }
    read(*part, "show", show, where + "." + key);
    readColour(*part, "colour", colour, where + "." + key);
}

void readFaces(const Json& object, const char* key, SurfaceFaces& out, const std::string& where)
{
    std::string word;
    read(object, key, word, where);
    if (word.empty()) {
        return;
    }
    const auto faces = parseSurfaceFaces(word);
    if (!faces) {
        throw Malformed{where + ": " + key + " \"" + word + "\" is no kind of faces"};
    }
    out = *faces;
}

void readOptional(const Json& object, const char* key, std::optional<double>& out,
                  const std::string& where)
{
    const Json* value = member(object, key);
    if (value == nullptr || value->is_null()) {
        out.reset();
        return;
    }
    if (!value->is_number()) {
        throw Malformed{where + ": " + key + " is not a number"};
    }
    out = value->get<double>();
}

SurfaceDisplayStyle readStyle(const Json& item, std::size_t index)
{
    std::string where = "style " + std::to_string(index + 1);
    if (!item.is_object()) {
        throw Malformed{where + " is not an object"};
    }
    SurfaceDisplayStyle style;
    read(item, "name", style.name, where);
    where = "style \"" + style.name + "\"";
    read(item, "description", style.description, where);
    readShown(item, "triangles", style.showTriangles, style.triangleColour, where);
    readShown(item, "boundary", style.showBoundary, style.boundaryColour, where);
    readShown(item, "points", style.showPoints, style.pointColour, where);
    readShown(item, "breaklines", style.showBreaklines, style.breaklineColour, where);
    readFaces(item, "faces", style.planFaces, where);
    read(item, "opacity", style.planOpacity, where);
    if (const Json* contours = member(item, "contours")) {
        if (!contours->is_object()) {
            throw Malformed{where + ": contours is not an object"};
        }
        SurfaceContours& c = style.contours;
        read(*contours, "show", c.show, where + ".contours");
        read(*contours, "interval", c.interval, where + ".contours");
        read(*contours, "major", c.majorEvery, where + ".contours");
        read(*contours, "base", c.base, where + ".contours");
        readColour(*contours, "minor_colour", c.minorColour, where + ".contours");
        readColour(*contours, "major_colour", c.majorColour, where + ".contours");
        read(*contours, "labels", c.labelMajor, where + ".contours");
    }
    read(item, "ramp", style.ramp, where);
    read(item, "banded", style.banded, where);
    read(item, "bands", style.bands, where);
    if (const Json* stops = member(item, "stops")) {
        if (!stops->is_array()) {
            throw Malformed{where + ": stops is not a list"};
        }
        for (const Json& each : *stops) {
            if (!each.is_object()) {
                throw Malformed{where + ": a stop is not an object"};
            }
            ColourStop stop;
            read(each, "value", stop.value, where + ".stops");
            readColour(each, "colour", stop.colour, where + ".stops");
            style.stops.push_back(stop);
        }
    }
    readOptional(item, "min", style.rangeMin, where);
    readOptional(item, "max", style.rangeMax, where);
    readColour(item, "flat_colour", style.flatColour, where);
    std::string shading;
    read(item, "shading", shading, where);
    if (!shading.empty()) {
        const auto parsed = parseSurfaceShading(shading);
        if (!parsed) {
            throw Malformed{where + ": shading \"" + shading + "\" is no kind of shading"};
        }
        style.shading = *parsed;
    }
    readFaces(item, "faces3d", style.faces3d, where);
    return style;
}

} // namespace

std::string surfaceStylesToJson(const SurfaceStyleSet& styles)
{
    Ordered list = Ordered::array();
    for (const SurfaceDisplayStyle& style : styles.styles) {
        list.push_back(styleJson(style));
    }
    const Ordered root{{"format", kFormat},
                       {"version", SurfaceStyleSet::kVersion},
                       {"default", styles.defaultStyle},
                       {"styles", std::move(list)}};
    // Never throws. The Document only ever holds a set checkSurfaceStyleSet
    // accepted, whose text is UTF-8, so for it `replace` changes nothing; it
    // is what keeps a set built by hand - a test's, a tool's - from throwing
    // out of a reply or a save with nlohmann's type_error.316, and U+FFFD says
    // where. The stored value is guarded apart, in Document::writeSurfaceState,
    // which refuses to save such a set rather than rewrite a name in it.
    return root.dump(-1, ' ', false, Ordered::error_handler_t::replace);
}

Result<SurfaceStyleSet> surfaceStylesFromJson(std::string_view text)
{
    Json root;
    try {
        root = Json::parse(text);
    } catch (const Json::exception&) {
        return makeError(ErrorCode::ParseFailure, "the surface styles are not JSON");
    }
    // The type is checked before the value is read: nlohmann's value() THROWS
    // type_error.302 on a "format" that is a number, an object or null, and this
    // is called from Document::open, which promises the built-ins in place of
    // text it cannot read - not an exception out of the open.
    const Json* format = root.is_object() ? member(root, "format") : nullptr;
    if (format == nullptr || !format->is_string() ||
        format->get_ref<const std::string&>() != kFormat) {
        return makeError(ErrorCode::ParseFailure, "not a set of surface styles");
    }
    const Json* version = member(root, "version");
    if (version == nullptr || !version->is_number_integer()) {
        return makeError(ErrorCode::ParseFailure, "the surface styles have no version");
    }
    if (version->get<long long>() > SurfaceStyleSet::kVersion) {
        return makeError(ErrorCode::Unsupported,
                         "the surface styles were written by a newer version of Katana",
                         "version " + std::to_string(version->get<long long>()));
    }
    SurfaceStyleSet set;
    try {
        read(root, "default", set.defaultStyle, "the set");
        const Json* styles = member(root, "styles");
        if (styles == nullptr || !styles->is_array()) {
            throw Malformed{"the set has no list of styles"};
        }
        std::size_t index = 0;
        for (const Json& item : *styles) {
            set.styles.push_back(readStyle(item, index++));
        }
    } catch (const Malformed& malformed) {
        return makeError(ErrorCode::ParseFailure,
                         "the surface styles are not in the expected form: " + malformed.what);
    }
    if (auto status = checkSurfaceStyleSet(set); !status) {
        return status.error();
    }
    return set;
}

} // namespace katana::cad
