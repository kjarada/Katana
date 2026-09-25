#include "katana/cad/command_interpreter.hpp"

#include "katana/cad/parcel.hpp"
#include "katana/cad/purge.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/survey_tools.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/table_usage.hpp"

#include "katana/entity/dimension_text.hpp"

#include <algorithm>
#include <cctype>
#include <charconv>
#include <cmath>
#include <map>
#include <iomanip>
#include <sstream>

#include "katana/entity/entity_geometry.hpp"

namespace katana::cad {

using katana::commands::CommandPtr;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Vec2;
namespace cmd = katana::commands;

namespace {

std::string upper(std::string text)
{
    std::transform(text.begin(), text.end(), text.begin(),
                   [](unsigned char ch) { return static_cast<char>(std::toupper(ch)); });
    return text;
}

// Splits on whitespace; double quotes group words and are removed.
Result<std::vector<std::string>> tokenize(std::string_view line)
{
    std::vector<std::string> tokens;
    std::string current;
    bool inQuotes = false;
    bool hasToken = false;
    for (const char ch : line) {
        if (ch == '"') {
            inQuotes = !inQuotes;
            hasToken = true; // "" is a valid empty token
        } else if (!inQuotes && std::isspace(static_cast<unsigned char>(ch)) != 0) {
            if (hasToken) {
                tokens.push_back(std::move(current));
                current.clear();
                hasToken = false;
            }
        } else {
            current += ch;
            hasToken = true;
        }
    }
    if (inQuotes) {
        return makeError(ErrorCode::ParseFailure, "unterminated quoted string");
    }
    if (hasToken) {
        tokens.push_back(std::move(current));
    }
    return tokens;
}

// std::from_chars is locale independent: "1.5" never becomes "1,5".
Result<double> parseNumber(std::string_view text);

// "station,elevation" or "station,elevation,curveLength", as ALIGN DESIGN
// takes its PVIs - one token per PVI, so a profile of any length is one line.
Result<katana::geometry::ProfilePVI> parsePVI(const std::string& text)
{
    std::vector<double> numbers;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t comma = text.find(',', start);
        const std::string_view part =
            std::string_view(text).substr(start, comma == std::string::npos ? std::string::npos
                                                                            : comma - start);
        const auto value = parseNumber(part);
        if (!value) {
            return makeError(ErrorCode::InvalidArgument, "a PVI is station,elevation[,curveLength]",
                             text);
        }
        numbers.push_back(*value);
        if (comma == std::string::npos) {
            break;
        }
        start = comma + 1;
    }
    if (numbers.size() < 2 || numbers.size() > 3) {
        return makeError(ErrorCode::InvalidArgument, "a PVI is station,elevation[,curveLength]",
                         text);
    }
    return katana::geometry::ProfilePVI{numbers[0], numbers[1],
                                        numbers.size() == 3 ? numbers[2] : 0.0};
}

Result<double> parseNumber(std::string_view text)
{
    double value = 0.0;
    const char* const end = text.data() + text.size();
    const char* begin = text.data();
    if (begin != end && *begin == '+') {
        ++begin; // from_chars rejects an explicit plus sign
    }
    const auto [parsed, error] = std::from_chars(begin, end, value);
    if (error != std::errc{} || parsed != end || !std::isfinite(value)) {
        return makeError(ErrorCode::ParseFailure, "expected a number", std::string(text));
    }
    return value;
}

Result<EntityId> parseId(std::string_view text)
{
    EntityId value = 0;
    const char* const end = text.data() + text.size();
    const auto [parsed, error] = std::from_chars(text.data(), end, value);
    if (error != std::errc{} || parsed != end || value == 0) {
        return makeError(ErrorCode::ParseFailure, "expected an entity id", std::string(text));
    }
    return value;
}

Result<int> parseCount(std::string_view text)
{
    int value = 0;
    const char* const end = text.data() + text.size();
    const auto [parsed, error] = std::from_chars(text.data(), end, value);
    if (error != std::errc{} || parsed != end || value < 1) {
        return makeError(ErrorCode::ParseFailure, "expected a positive whole number",
                         std::string(text));
    }
    return value;
}

katana::core::Error usage(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

const std::map<std::string, std::string, std::less<>>& aliases()
{
    static const std::map<std::string, std::string, std::less<>> table = {
        {"L", "LINE"},     {"PL", "PLINE"},      {"POLYLINE", "PLINE"}, {"C", "CIRCLE"},
        {"A", "ARC"},      {"PO", "POINT"},      {"REC", "RECT"},       {"RECTANGLE", "RECT"},
        {"T", "TEXT"},     {"DIMENSION", "DIM"}, {"M", "MOVE"},         {"CO", "COPY"},
        {"CP", "COPY"},    {"RO", "ROTATE"},     {"SC", "SCALE"},       {"MI", "MIRROR"},
        {"AR", "ARRAY"},   {"E", "ERASE"},       {"DELETE", "ERASE"},   {"DEL", "ERASE"},
        {"O", "OFFSET"},   {"TR", "TRIM"},       {"EX", "EXTEND"},      {"F", "FILLET"},
        {"CHA", "CHAMFER"}, {"U", "UNDO"},       {"LA", "LAYER"},       {"SEL", "SELECT"},
        {"LT", "LINETYPE"}, {"LTYPE", "LINETYPE"}, {"DS", "DIMSTYLE"}, {"HA", "HATCH"}, {"AL", "ALIGN"}, {"PARC", "PARCEL"}, {"ST", "STYLE"},
        {"RADIATE", "FORWARD"}, {"?", "HELP"},
    };
    return table;
}

std::string describe(const Entity& entity)
{
    std::ostringstream out;
    out.precision(12);
    out << entity.id << "  " << toString(entity.type()) << "  layer=" << entity.layer;
    struct Detail {
        std::ostream& out;
        void operator()(const katana::entity::PointGeometry& g) const
        {
            out << "  at " << g.position.x << "," << g.position.y;
        }
        void operator()(const katana::geometry::Segment2& g) const
        {
            out << "  " << g.start.x << "," << g.start.y << " -> " << g.end.x << "," << g.end.y
                << "  length=" << g.length();
        }
        void operator()(const katana::geometry::Arc2& g) const
        {
            out << "  centre " << g.center.x << "," << g.center.y << "  radius=" << g.radius
                << "  length=" << g.length();
        }
        void operator()(const katana::geometry::Polyline2& g) const
        {
            out << "  vertices=" << g.vertices.size() << (g.closed ? "  closed" : "  open")
                << "  length=" << g.length();
            if (g.closed) {
                out << "  area=" << g.area();
            }
        }
        void operator()(const katana::geometry::Circle2& g) const
        {
            out << "  centre " << g.center.x << "," << g.center.y << "  radius=" << g.radius
                << "  area=" << g.area();
        }
        void operator()(const katana::entity::TextGeometry& g) const
        {
            out << "  \"" << g.text << "\"  height=" << g.height;
        }
        void operator()(const katana::entity::DimensionGeometry& g) const
        {
            out << "  measures " << g.measurement();
        }
    };
    std::visit(Detail{out}, entity.geometry);
    if (!entity.visible) {
        out << "  hidden";
    }
    return out.str();
}

// "true"/"false" -> bool, whole numbers -> integer, other numbers -> real, else text.
// A value of a STATED type, for the times the guess below would be wrong:
// a code that reads as a number ("2"), a level that must stay real ("31").
katana::core::Result<katana::entity::PropertyValue> typedPropertyValue(const std::string& type,
                                                                      const std::string& text)
{
    const std::string wanted = upper(type);
    if (wanted == "TEXT" || wanted == "STRING") {
        return katana::entity::PropertyValue(text);
    }
    if (wanted == "INTEGER" || wanted == "INT") {
        std::int64_t integer = 0;
        const char* const end = text.data() + text.size();
        const auto [parsed, error] = std::from_chars(text.data(), end, integer);
        if (error != std::errc{} || parsed != end) {
            return makeError(ErrorCode::ParseFailure, "not an integer", text);
        }
        return katana::entity::PropertyValue(integer);
    }
    if (wanted == "REAL" || wanted == "DOUBLE") {
        const auto number = parseNumber(text);
        if (!number) {
            return number.error();
        }
        return katana::entity::PropertyValue(*number);
    }
    if (wanted == "BOOLEAN" || wanted == "BOOL") {
        const std::string folded = upper(text);
        if (folded == "TRUE" || folded == "1") {
            return katana::entity::PropertyValue(true);
        }
        if (folded == "FALSE" || folded == "0") {
            return katana::entity::PropertyValue(false);
        }
        return makeError(ErrorCode::ParseFailure, "not a boolean", text);
    }
    return makeError(ErrorCode::InvalidArgument,
                     "the type must be text, integer, real or boolean", type);
}

katana::entity::PropertyValue parsePropertyValue(const std::string& text)
{
    const std::string folded = upper(text);
    if (folded == "TRUE") {
        return true;
    }
    if (folded == "FALSE") {
        return false;
    }
    std::int64_t integer = 0;
    const char* const end = text.data() + text.size();
    if (const auto [parsed, error] = std::from_chars(text.data(), end, integer);
        error == std::errc{} && parsed == end) {
        return integer;
    }
    if (const auto number = parseNumber(text)) {
        return *number;
    }
    return text;
}

} // namespace

std::string CommandInterpreter::helpText()
{
    return R"(Points: x,y | @dx,dy (relative) | @dist<angle (polar, degrees). Angles in degrees.

Draw      POINT p | LINE p p [p...] | PLINE p p [p...] [CLOSE] | RECT p p
          CIRCLE centre radius | ARC p p p (start, on-arc, end)
          TEXT p height "text" | DIM p p offset
Modify    (act on the selection)
          MOVE dx,dy | COPY dx,dy | ROTATE centre angle | SCALE centre factor
          MIRROR p p [KEEP] | ARRAY rows columns dx,dy | ERASE
Edit      OFFSET id distance side-point | TRIM id pick-point cutter-id...
          EXTEND id pick-point boundary-id... | FILLET id id radius
          CHAMFER id id distance [distance2]
Select    SELECT ALL | NONE | id... | LAYER name | TYPE name
Layers    LAYER LIST | NEW name [#RRGGBB] | SET name | DELETE name
          LAYER SHOW|HIDE|LOCK|UNLOCK name | LAYER LTYPE layer linetype
Linetype  LINETYPE LIST | NEW name dash gap [dash gap ...] | RENAME old new | DELETE name
          LINETYPE MERGE from into   (repoints every layer and style, then deletes from)
          lengths are MODEL units: + dash, - gap, 0 dot. e.g. LINETYPE NEW fence 1 -0.5
Hatch     HATCH LIST | SOLID name | NEW name angle spacing [angle spacing ...] | DELETE name
          angle in DEGREES, spacing in MODEL units.  LAYER HATCH layer pattern attaches one
Style     STYLE LIST | SYMBOLS [filter] | NEW name | SET name field value | RENAME old new
          STYLE DELETE name | APPLY name | MERGE from into | USAGE [name] | CURRENT [name|-]
          fields: linetype weight colour hatch symbol symbolsize description; APPLY - = ByLayer
          linetype takes a model linetype, a loaded library linestyle or ByLayer (the layer's)
Purge     PURGE [STYLES|LINETYPES|HATCHES|ALL]   deletes what nothing uses, as one undo step
Align     ALIGN LIST | NEW name x,y x,y [x,y ...] | PI name x,y [radius [spIn [spOut]]]
          SET name index radius [spIn [spOut]] | START name station | STATIONS name interval
          DELETE name.  PI indices count from 0; radius 0 is a kink; spirals in MODEL units
          DESIGN name s,z[,L] s,z[,L] ... defines the design profile (parabolic vertical
          curves, symmetric); PVI name s z [L] appends; PROFILE name prints it with its
          high and low points; CLEARPROFILE name removes it
Parcel    PARCEL id            bearings, distances, area and centroid of a closed polyline
          PARCEL id LEGAL [name]   the deed wording;  PARCEL id LABEL [height]   text labels
Survey    INVERSE p p | INVERSE line-id   distance, azimuth, bearing; height difference,
          slope distance and grade when both ends have an elevation
          FORWARD p direction distance [dZ [name]]   a new point on the current layer, one
          undo step (alias RADIATE); dZ - for none. p is E,N or E,N,Z (easting first) or a
          point's id. direction: an azimuth 36d52m11.63s | 36:52:11.63 | 36-52-11.63 |
          36.8699 (decimal degrees) or a bearing N36d52m11.63sE ("N 36 52 11.63 E" quoted)
          AREA [id...]   area and perimeter of closed polylines and circles, each and in
          total (the selection when no ids); hectares when the project unit is the metre
DimStyle  DIMSTYLE LIST | NEW name | SET name field value | DELETE name
          fields TEXT GAP EXTOFF EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND PREFIX SUFFIX TRIM
          LAYER DIMSTYLE layer style   attaches one
Attribs   CHLAYER name | COLOR #RRGGBB|BYLAYER   (selection)
Props     PROP LIST | SET key value [text|integer|real|boolean] | DELETE key
          PROP RENAME old new   (selection; the type is guessed unless stated)
History   UNDO [n] | REDO [n]
File      NEW | OPEN directory | SAVE [directory]
Inspect   LIST | INFO id | HELP
Sheets    SHEETS [LIST] | JSON [path] | SAVE path | LOAD path      (HELP SHEETS: every option)
          SHEET NEW|REMOVE|MOVE|COPY|RENAME|SET|FIELD | VIEW ADD|SET|REMOVE|LIST | TILE n preset
          GENERATE fit|grid|strips|profile|sections|frames | TITLEBLOCK [LIST] | field value
          TITLEBLOCK REVISION ADD|REMOVE | LOGO path | PLOTSHEETS path.pdf [sheets=1,3-5] [dpi=300]
Aliases   L PL C A PO REC T M CO RO SC MI AR E O TR EX F CHA U LA SEL RADIATE ?)";
}

Result<std::vector<std::string>> CommandInterpreter::tokenize(std::string_view line)
{
    return katana::cad::tokenize(line);
}

Result<Point2> CommandInterpreter::parsePoint(const std::string& text)
{
    const bool relative = !text.empty() && text.front() == '@';
    const std::string body = relative ? text.substr(1) : text;
    if (relative && !lastPoint_) {
        return makeError(ErrorCode::InvalidState,
                         "a relative point needs a previous point in this session", text);
    }

    Vec2 value;
    if (const auto polar = body.find('<'); polar != std::string::npos) {
        if (!relative) {
            return makeError(ErrorCode::ParseFailure, "polar points must be relative: @dist<angle",
                             text);
        }
        const auto distance = parseNumber(std::string_view(body).substr(0, polar));
        const auto degrees = parseNumber(std::string_view(body).substr(polar + 1));
        if (!distance || !degrees) {
            return makeError(ErrorCode::ParseFailure, "expected @distance<angle", text);
        }
        value = Vec2(*distance, 0.0).rotated(*degrees * katana::math::kDegToRad);
    } else {
        const auto comma = body.find(',');
        if (comma == std::string::npos) {
            return makeError(ErrorCode::ParseFailure, "expected a point as x,y", text);
        }
        const auto x = parseNumber(std::string_view(body).substr(0, comma));
        const auto y = parseNumber(std::string_view(body).substr(comma + 1));
        if (!x || !y) {
            return makeError(ErrorCode::ParseFailure, "expected a point as x,y", text);
        }
        value = Vec2(*x, *y);
    }
    const Point2 point = relative ? *lastPoint_ + value : value;
    lastPoint_ = point;
    return point;
}

CommandInterpreter::Reply CommandInterpreter::requireSelection() const
{
    if (document_.selection().empty()) {
        return makeError(ErrorCode::InvalidState, "nothing is selected; use SELECT first");
    }
    return std::string{};
}

CommandInterpreter::Reply CommandInterpreter::finish(Status status, std::string message)
{
    if (!status) {
        return status.error();
    }
    return message;
}

namespace {

// ---- the table managers' verbs ------------------------------------------------------

std::string restOfLine(const std::vector<std::string>& args, std::size_t from)
{
    std::string text;
    for (std::size_t i = from; i < args.size(); ++i) {
        text += (i > from ? " " : "") + args[i];
    }
    return text;
}

// The one answer to "may a style or a layer be given this linetype name":
// a model linetype, or a loaded library definition drawn along a line - the
// same two places the viewport resolves it in (D2). A library linestyle used to
// be refused here although the model and the viewport both take one (audit
// CAD-06). A vertex symbol is refused BY NAME, because it is never drawn as
// a line pattern (D8) and "does not exist" would be untrue.
katana::core::Status checkLinetypeName(const Document& document, const std::string& name)
{
    if (document.model().linetypes.contains(name)) {
        return {};
    }
    if (const katana::entity::LineStyle* definition = document.definitionFor(name);
        definition != nullptr) {
        if (definition->atVertices) {
            return makeError(ErrorCode::InvalidArgument,
                             "that is a vertex symbol, not a linestyle: give it as the symbol",
                             name);
        }
        return {};
    }
    return makeError(ErrorCode::NotFound,
                     "no linetype of that name is in the drawing or the loaded library", name);
}

std::string countedNoun(std::size_t count, const char* one, const char* many)
{
    return std::to_string(count) + " " + (count == 1 ? one : many);
}

std::string listed(const std::vector<std::string>& names)
{
    std::string text;
    for (const std::string& name : names) {
        text += (text.empty() ? "" : ", ") + name;
    }
    return text;
}

// PURGE [STYLES|LINETYPES|HATCHES|ALL]. The current style is kept although
// nothing wears it yet: it is what the next thing drawn will be.
katana::core::Result<std::string> purgeTables(Document& document,
                                              const std::vector<std::string>& args)
{
    static constexpr const char* kPurgeUsage = "PURGE [STYLES|LINETYPES|HATCHES|ALL]";
    if (args.size() > 1) {
        return usage(kPurgeUsage);
    }
    const std::string what = args.empty() ? "ALL" : upper(args[0]);
    PurgeOptions options;
    if (what == "STYLES") {
        options.linetypes = options.hatches = false;
    } else if (what == "LINETYPES") {
        options.styles = options.hatches = false;
    } else if (what == "HATCHES") {
        options.styles = options.linetypes = false;
    } else if (what != "ALL") {
        return usage(kPurgeUsage);
    }
    if (!document.currentStyle().empty()) {
        options.keepStyles.push_back(document.currentStyle());
    }
    const katana::commands::TableItems plan = planPurge(document.model(), options);
    auto command = purgeCommand(document.model(), options);
    if (!command) {
        return command.error();
    }
    if (*command == nullptr) {
        return std::string("nothing to purge: everything is used");
    }
    if (auto status = document.execute(std::move(*command)); !status) {
        return status.error();
    }
    std::string reply = "purged " + countedNoun(plan.styles.size(), "style", "styles") + ", " +
                        countedNoun(plan.linetypes.size(), "linetype", "linetypes") + " and " +
                        countedNoun(plan.hatchPatterns.size(), "hatch pattern", "hatch patterns") +
                        " (one UNDO restores them)";
    const auto append = [&reply](const char* label, const std::vector<std::string>& names) {
        if (!names.empty()) {
            reply += std::string("\n  ") + label + ": " + listed(names);
        }
    };
    append("styles", plan.styles);
    append("linetypes", plan.linetypes);
    append("hatch patterns", plan.hatchPatterns);
    return reply;
}

} // namespace

CommandInterpreter::Reply CommandInterpreter::run(std::string_view line)
{
    auto tokens = tokenize(line);
    if (!tokens) {
        return tokens.error();
    }
    if (tokens->empty()) {
        return std::string{};
    }
    history_.emplace_back(line);

    std::string verb = upper(tokens->front());
    if (const auto alias = aliases().find(verb); alias != aliases().end()) {
        verb = alias->second;
    }
    const Tokens args(tokens->begin() + 1, tokens->end());

    if (verb == "HELP") {
        if (!args.empty() && plotting::isSheetVerb(args.front())) {
            return plotting::sheetVerbHelp();
        }
        return helpText();
    }
    if (plotting::isSheetVerb(verb)) {
        return plotting::runSheetVerb(document_, *tokens, sheetContext_);
    }
    for (const char* name : {"POINT", "LINE", "PLINE", "RECT", "CIRCLE", "ARC", "TEXT", "DIM"}) {
        if (verb == name) {
            return draw(verb, args);
        }
    }
    for (const char* name : {"MOVE", "COPY", "ROTATE", "SCALE", "MIRROR", "ARRAY", "ERASE"}) {
        if (verb == name) {
            return transform(verb, args);
        }
    }
    for (const char* name : {"OFFSET", "TRIM", "EXTEND", "FILLET", "CHAMFER"}) {
        if (verb == name) {
            return edit(verb, args);
        }
    }
    for (const char* name : {"CHLAYER", "COLOR", "PROP"}) {
        if (verb == name) {
            return attributes(verb, args);
        }
    }
    if (verb == "SELECT") {
        return select(args);
    }
    if (verb == "LAYER") {
        return layer(args);
    }
    if (verb == "LINETYPE") {
        return linetype(args);
    }
    if (verb == "DIMSTYLE") {
        return dimensionStyle(args);
    }
    if (verb == "HATCH") {
        return hatchPattern(args);
    }
    if (verb == "STYLE") {
        return style(args);
    }
    if (verb == "PURGE") {
        return purgeTables(document_, args);
    }
    if (verb == "ALIGN") {
        return alignment(args);
    }
    if (verb == "PARCEL") {
        return parcel(args);
    }
    if (verb == "INVERSE" || verb == "FORWARD" || verb == "AREA") {
        return survey(verb, args);
    }
    if (verb == "UNDO" || verb == "REDO") {
        return undoRedo(verb, args);
    }
    if (verb == "NEW" || verb == "OPEN" || verb == "SAVE") {
        return file(verb, args);
    }
    if (verb == "LIST" || verb == "INFO") {
        return inspect(verb, args);
    }
    return makeError(ErrorCode::ParseFailure, "unknown command; type HELP", verb);
}


// ---- styles -------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::style(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upper(args[0]);

    if (action == "LIST") {
        std::ostringstream out;
        out.precision(6);
        for (const katana::entity::Style& item : model.styles.all()) {
            out << "  " << item.name << "  linetype=" << item.linetype << "  weight=" << item.lineWeight;
            if (item.color) {
                out << "  colour=" << item.color->toHex();
            }
            if (!item.hatchPattern.empty()) {
                out << "  hatch=" << item.hatchPattern;
            }
            if (!item.symbol.empty()) {
                out << "  symbol=" << item.symbol;
                if (item.symbolSize > 0.0) {
                    out << "@" << item.symbolSize;
                }
            }
            if (!item.description.empty()) {
                out << "  " << item.description;
            }
            out << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text.empty() ? "no styles" : text;
    }
    if (action == "SYMBOLS") {
        // The catalogue's list, the one a symbol picker offers: the built-in
        // shapes and every library definition D3 calls a symbol - not only
        // the `mode vertex` ones, which are the minority of those in use.
        // Filtered by the rest of the line, case folded.
        const std::vector<CatalogueEntry> symbols =
            filterChoices(symbolChoices(document_), restOfLine(args, 1));
        std::ostringstream out;
        for (const CatalogueEntry& entry : symbols) {
            out << "  " << entry.name << "  (" << toString(entry.source);
            if (!entry.group.empty()) {
                out << ", " << entry.group;
            }
            out << ")";
            if (entry.users.used()) {
                out << "  " << entry.users.describe();
            }
            out << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text.empty() ? "no symbol matches" : text;
    }
    if (action == "USAGE") {
        const katana::entity::TableUsage usage = katana::entity::tableUsage(model);
        if (args.size() >= 2) {
            const std::string wanted = restOfLine(args, 1);
            const katana::entity::Users& users =
                katana::entity::TableUsage::of(usage.styles, wanted);
            if (!model.styles.contains(wanted) && !users.used()) {
                return makeError(ErrorCode::NotFound, "style does not exist", wanted);
            }
            return "style " + wanted + ": " + (users.used() ? users.describe() : "unused") +
                   (model.styles.contains(wanted) ? "" : " (not in the style table)");
        }
        std::ostringstream out;
        for (const auto& [name, users] : usage.styles) {
            out << "  " << name << "  " << countedNoun(users.entities, "entity", "entities")
                << (model.styles.contains(name) ? "" : "  (not in the style table)") << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text.empty() ? "no styles" : text;
    }
    if (action == "CURRENT") {
        if (args.size() >= 2) {
            const std::string wanted = restOfLine(args, 1);
            const bool byLayer = wanted == "-" || katana::entity::isByLayer(wanted);
            if (auto status = document_.setCurrentStyle(byLayer ? std::string() : wanted);
                !status) {
                return status.error();
            }
        }
        return "current style is " +
               (document_.currentStyle().empty() ? std::string("ByLayer")
                                                 : document_.currentStyle());
    }

    static constexpr const char* kUsage =
        "STYLE LIST | SYMBOLS [filter] | NEW name | SET name field value | RENAME old new |"
        " DELETE name | APPLY name | MERGE from into | USAGE [name] | CURRENT [name|-]\n"
        "  fields: linetype (a model linetype, a library linestyle or ByLayer), weight (mm),\n"
        "  colour (#RRGGBB or bylayer), hatch, symbol,\n"
        "  symbolsize (model units, 0 for the default mark), description\n"
        "  APPLY sets the style of the selection; APPLY - clears it (ByLayer)";
    if (args.size() < 2) {
        return usage(kUsage);
    }
    const std::string& name = args[1];
    // A library style name has spaces in it ("TOPO Natural Surface Point"), and
    // STYLE NEW TOPO Natural Surface Point used to make a style called
    // "TOPO" and drop the rest without a word. Everything but SET, whose
    // description takes the rest of the line, now says so.
    const auto wants = [&](std::size_t count) -> std::optional<Reply> {
        if (args.size() <= count) {
            return std::nullopt;
        }
        return Reply(makeError(ErrorCode::InvalidArgument,
                               "too many arguments: put a name with spaces in double quotes",
                               args[count]));
    };

    if (action == "NEW") {
        if (const auto wrong = wants(2)) {
            return *wrong;
        }
        katana::entity::Style item;
        item.name = name;
        return finish(document_.execute(cmd::createStyle(std::move(item))),
                      "style " + name + " created");
    }
    if (action == "DELETE") {
        if (const auto wrong = wants(2)) {
            return *wrong;
        }
        return finish(document_.execute(cmd::deleteStyle(name)), "style " + name + " deleted");
    }
    if (action == "RENAME") {
        if (args.size() < 3) {
            return usage("STYLE RENAME old new");
        }
        if (const auto wrong = wants(3)) {
            return *wrong;
        }
        return finish(document_.execute(cmd::renameStyle(name, args[2])),
                      "style " + name + " renamed to " + args[2] +
                          " (every entity wearing it came too)");
    }
    if (action == "MERGE") {
        if (args.size() < 3) {
            return usage("STYLE MERGE from into");
        }
        if (const auto wrong = wants(3)) {
            return *wrong;
        }
        const std::size_t moved =
            katana::entity::TableUsage::of(katana::entity::tableUsage(model).styles, name).entities;
        return finish(document_.execute(cmd::mergeStyle(name, args[2])),
                      "style " + name + " merged into " + args[2] + " (" +
                          countedNoun(moved, "entity", "entities") + " moved)");
    }
    if (action == "APPLY") {
        if (const auto wrong = wants(2)) {
            return *wrong;
        }
        const std::vector<EntityId> ids = document_.selection().ids();
        if (ids.empty()) {
            return makeError(ErrorCode::InvalidState, "nothing is selected");
        }
        const std::string wanted = name == "-" ? std::string() : name;
        if (!wanted.empty() && !model.styles.contains(wanted)) {
            return makeError(ErrorCode::NotFound, "style does not exist", wanted);
        }
        return finish(document_.execute(cmd::setEntityStyle(ids, wanted)),
                      std::to_string(ids.size()) + " entities set to style " +
                          (wanted.empty() ? "ByLayer" : wanted));
    }
    if (action == "SET") {
        if (args.size() < 4) {
            return usage(kUsage);
        }
        const katana::entity::Style* existing = model.styles.find(name);
        if (existing == nullptr) {
            return makeError(ErrorCode::NotFound, "style does not exist", name);
        }
        katana::entity::Style changed = *existing;
        const std::string field = upper(args[2]);
        const std::string& value = args[3];
        if (field == "LINETYPE") {
            // The rest of the line, as SYMBOL takes it: "WATR Main" unquoted
            // used to become "WATR" with "Main" dropped.
            const std::string linetypeName = restOfLine(args, 3);
            if (katana::entity::isByLayer(linetypeName)) {
                changed.linetype = std::string(katana::entity::kByLayerLinetype);
            } else {
                if (auto status = checkLinetypeName(document_, linetypeName); !status) {
                    return status.error();
                }
                changed.linetype = linetypeName;
            }
        } else if (field == "WEIGHT") {
            const auto weight = parseNumber(value);
            if (!weight) {
                return weight.error();
            }
            changed.lineWeight = *weight;
        } else if (field == "COLOUR" || field == "COLOR") {
            if (upper(value) == "BYLAYER") {
                changed.color.reset();
            } else {
                const auto colour = katana::entity::Color::fromHex(value);
                if (!colour) {
                    return colour.error();
                }
                changed.color = *colour;
            }
        } else if (field == "HATCH") {
            if (value != "-" && !model.hatchPatterns.contains(value)) {
                return makeError(ErrorCode::NotFound, "hatch pattern does not exist", value);
            }
            changed.hatchPattern = value == "-" ? std::string() : value;
        } else if (field == "SYMBOL") {
            // A library symbol's name has spaces in it - "CULT Bollard" -
            // so the rest of the line is the name, as DESCRIPTION does.
            std::string symbolName;
            for (std::size_t i = 3; i < args.size(); ++i) {
                symbolName += (i > 3 ? " " : "") + args[i];
            }
            if (symbolName == "-") {
                changed.symbol.clear();
            } else {
                // The model itself accepts any name, because a project can be
                // opened before its library is loaded (see entity::validate).
                // A person TYPING one should still be told about a typo, which
                // is the same courtesy HATCH above pays.
                if (!katana::entity::isBuiltInSymbolName(symbolName) &&
                    document_.definitionFor(symbolName) == nullptr) {
                    return makeError(ErrorCode::NotFound,
                                     "no symbol of that name is built in or in the loaded library",
                                     symbolName);
                }
                changed.symbol = symbolName;
            }
        } else if (field == "SYMBOLSIZE") {
            const auto size = parseNumber(value);
            if (!size) {
                return size.error();
            }
            changed.symbolSize = *size;
        } else if (field == "DESCRIPTION") {
            std::string text;
            for (std::size_t i = 3; i < args.size(); ++i) {
                text += (i > 3 ? " " : "") + args[i];
            }
            changed.description = text;
        } else {
            return usage(kUsage);
        }
        // Setting what is already there is not an edit: no undo step.
        auto command = cmd::updateStyleIfChanged(model, std::move(changed));
        if (command == nullptr) {
            return "style " + name + " unchanged";
        }
        return finish(document_.execute(std::move(command)), "style " + name + " updated");
    }
    return usage(kUsage);
}

// ---- draw -------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::draw(const std::string& verb, const Tokens& args)
{
    const cmd::EntityAttributes attributes = document_.currentAttributes();

    // Leading arguments that are points; PLINE may end with CLOSE.
    const auto parsePoints = [&](std::size_t count) -> Result<std::vector<Point2>> {
        std::vector<Point2> points;
        for (std::size_t i = 0; i < count; ++i) {
            auto point = parsePoint(args[i]);
            if (!point) {
                return point.error();
            }
            points.push_back(*point);
        }
        return points;
    };

    if (verb == "POINT") {
        if (args.size() != 1) {
            return usage("POINT x,y");
        }
        auto points = parsePoints(1);
        if (!points) {
            return points.error();
        }
        return finish(document_.execute(cmd::createPoint(points->front(), attributes)),
                      "point created");
    }
    if (verb == "LINE") {
        if (args.size() < 2) {
            return usage("LINE p p [p...]");
        }
        auto points = parsePoints(args.size());
        if (!points) {
            return points.error();
        }
        if (points->size() == 2) {
            return finish(document_.execute(cmd::createLine((*points)[0], (*points)[1], attributes)),
                          "line created");
        }
        auto chain = std::make_unique<cmd::Transaction>("CREATE_LINES");
        for (std::size_t i = 1; i < points->size(); ++i) {
            chain->add(cmd::createLine((*points)[i - 1], (*points)[i], attributes));
        }
        const std::size_t count = chain->size();
        return finish(document_.execute(std::move(chain)), std::to_string(count) + " lines created");
    }
    if (verb == "PLINE") {
        const bool closed = !args.empty() && (upper(args.back()) == "CLOSE" || upper(args.back()) == "C");
        const std::size_t pointCount = args.size() - (closed ? 1 : 0);
        if (pointCount < 2) {
            return usage("PLINE p p [p...] [CLOSE]");
        }
        auto points = parsePoints(pointCount);
        if (!points) {
            return points.error();
        }
        return finish(document_.execute(cmd::createPolyline(
                          katana::geometry::Polyline2{std::move(*points), closed}, attributes)),
                      "polyline created");
    }
    if (verb == "RECT") {
        if (args.size() != 2) {
            return usage("RECT corner corner");
        }
        auto points = parsePoints(2);
        if (!points) {
            return points.error();
        }
        const auto rectangle = katana::geometry::Rectangle2::fromCorners((*points)[0], (*points)[1]);
        return finish(document_.execute(cmd::createPolyline(rectangle.toPolyline(), attributes)),
                      "rectangle created");
    }
    if (verb == "CIRCLE") {
        if (args.size() != 2) {
            return usage("CIRCLE centre radius");
        }
        auto points = parsePoints(1);
        const auto radius = parseNumber(args[1]);
        if (!points) {
            return points.error();
        }
        if (!radius) {
            return radius.error();
        }
        return finish(document_.execute(cmd::createCircle(points->front(), *radius, attributes)),
                      "circle created");
    }
    if (verb == "ARC") {
        if (args.size() != 3) {
            return usage("ARC start point-on-arc end");
        }
        auto points = parsePoints(3);
        if (!points) {
            return points.error();
        }
        const auto arc =
            katana::geometry::Arc2::throughPoints((*points)[0], (*points)[1], (*points)[2]);
        if (!arc) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the three points are collinear or coincident");
        }
        return finish(document_.execute(cmd::createArc(*arc, attributes)), "arc created");
    }
    if (verb == "TEXT") {
        if (args.size() < 3) {
            return usage("TEXT x,y height \"text\"");
        }
        auto points = parsePoints(1);
        const auto height = parseNumber(args[1]);
        if (!points) {
            return points.error();
        }
        if (!height) {
            return height.error();
        }
        std::string text = args[2];
        for (std::size_t i = 3; i < args.size(); ++i) {
            text += " " + args[i];
        }
        return finish(document_.execute(cmd::createText(
                          {points->front(), std::move(text), *height, 0.0}, attributes)),
                      "text created");
    }
    // DIM
    if (args.size() != 3) {
        return usage("DIM p p offset");
    }
    auto points = parsePoints(2);
    const auto offset = parseNumber(args[2]);
    if (!points) {
        return points.error();
    }
    if (!offset) {
        return offset.error();
    }
    return finish(document_.execute(cmd::createDimension(
                      {(*points)[0], (*points)[1], *offset, std::string{}}, attributes)),
                  "dimension created");
}

// ---- transform ----------------------------------------------------------------------------

void CommandInterpreter::resetPointState()
{
    lastPoint_.reset();
}

CommandInterpreter::Reply CommandInterpreter::transform(const std::string& verb,
                                                        const Tokens& args)
{
    if (auto selected = requireSelection(); !selected) {
        return selected;
    }
    const std::vector<EntityId> ids = document_.selection().ids();
    const std::string count = std::to_string(ids.size());

    // A displacement is written like a point but is never relative to anything.
    const auto parseVector = [&](const std::string& text) -> Result<Vec2> {
        const auto comma = text.find(',');
        if (comma == std::string::npos) {
            return makeError(ErrorCode::ParseFailure, "expected a displacement as dx,dy", text);
        }
        const auto x = parseNumber(std::string_view(text).substr(0, comma));
        const auto y = parseNumber(std::string_view(text).substr(comma + 1));
        if (!x || !y) {
            return makeError(ErrorCode::ParseFailure, "expected a displacement as dx,dy", text);
        }
        return Vec2(*x, *y);
    };

    if (verb == "ERASE") {
        return finish(document_.execute(cmd::deleteEntities(ids)), count + " erased");
    }
    if (verb == "MOVE" || verb == "COPY") {
        if (args.size() != 1) {
            return usage(verb + " dx,dy");
        }
        const auto delta = parseVector(args[0]);
        if (!delta) {
            return delta.error();
        }
        return verb == "MOVE"
                   ? finish(document_.execute(cmd::moveEntities(ids, *delta)), count + " moved")
                   : finish(document_.execute(cmd::copyEntities(ids, *delta)), count + " copied");
    }
    if (verb == "ROTATE" || verb == "SCALE") {
        if (args.size() != 2) {
            return usage(verb == "ROTATE" ? "ROTATE centre angle" : "SCALE centre factor");
        }
        const auto center = parsePoint(args[0]);
        const auto value = parseNumber(args[1]);
        if (!center) {
            return center.error();
        }
        if (!value) {
            return value.error();
        }
        return verb == "ROTATE"
                   ? finish(document_.execute(cmd::rotateEntities(
                                ids, *center, *value * katana::math::kDegToRad)),
                            count + " rotated")
                   : finish(document_.execute(cmd::scaleEntities(ids, *center, *value)),
                            count + " scaled");
    }
    if (verb == "MIRROR") {
        if (args.size() < 2 || args.size() > 3 || (args.size() == 3 && upper(args[2]) != "KEEP")) {
            return usage("MIRROR p p [KEEP]");
        }
        const auto a = parsePoint(args[0]);
        if (!a) {
            return a.error();
        }
        const auto b = parsePoint(args[1]);
        if (!b) {
            return b.error();
        }
        return finish(document_.execute(cmd::mirrorEntities(ids, *a, *b, args.size() == 3)),
                      count + " mirrored");
    }
    // ARRAY
    if (args.size() != 3) {
        return usage("ARRAY rows columns dx,dy");
    }
    const auto rows = parseCount(args[0]);
    const auto columns = parseCount(args[1]);
    const auto spacing = parseVector(args[2]);
    if (!rows) {
        return rows.error();
    }
    if (!columns) {
        return columns.error();
    }
    if (!spacing) {
        return spacing.error();
    }
    return finish(document_.execute(cmd::arrayEntities(ids, *rows, *columns, *spacing)),
                  "array created");
}

// ---- edit ---------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::edit(const std::string& verb, const Tokens& args)
{
    const auto parseIds = [&](std::size_t from) -> Result<std::vector<EntityId>> {
        std::vector<EntityId> ids;
        for (std::size_t i = from; i < args.size(); ++i) {
            const auto id = parseId(args[i]);
            if (!id) {
                return id.error();
            }
            ids.push_back(*id);
        }
        return ids;
    };

    if (verb == "OFFSET") {
        if (args.size() != 3) {
            return usage("OFFSET id distance side-point");
        }
        const auto id = parseId(args[0]);
        const auto distance = parseNumber(args[1]);
        if (!id) {
            return id.error();
        }
        if (!distance) {
            return distance.error();
        }
        const auto side = parsePoint(args[2]);
        if (!side) {
            return side.error();
        }
        return finish(document_.execute(cmd::offsetEntity(*id, *distance, *side)), "offset created");
    }
    if (verb == "TRIM" || verb == "EXTEND") {
        if (args.size() < 3) {
            return usage(verb + " id pick-point other-id...");
        }
        const auto id = parseId(args[0]);
        if (!id) {
            return id.error();
        }
        const auto pick = parsePoint(args[1]);
        if (!pick) {
            return pick.error();
        }
        auto others = parseIds(2);
        if (!others) {
            return others.error();
        }
        return verb == "TRIM"
                   ? finish(document_.execute(cmd::trimEntity(*id, std::move(*others), *pick)),
                            "trimmed")
                   : finish(document_.execute(cmd::extendEntity(*id, std::move(*others), *pick)),
                            "extended");
    }
    if (verb == "FILLET") {
        if (args.size() != 3) {
            return usage("FILLET id id radius");
        }
        const auto first = parseId(args[0]);
        const auto second = parseId(args[1]);
        const auto radius = parseNumber(args[2]);
        if (!first) {
            return first.error();
        }
        if (!second) {
            return second.error();
        }
        if (!radius) {
            return radius.error();
        }
        return finish(document_.execute(cmd::filletEntities(*first, *second, *radius)), "filleted");
    }
    // CHAMFER
    if (args.size() < 3 || args.size() > 4) {
        return usage("CHAMFER id id distance [distance2]");
    }
    const auto first = parseId(args[0]);
    const auto second = parseId(args[1]);
    const auto distanceFirst = parseNumber(args[2]);
    const auto distanceSecond = args.size() == 4 ? parseNumber(args[3]) : distanceFirst;
    if (!first) {
        return first.error();
    }
    if (!second) {
        return second.error();
    }
    if (!distanceFirst) {
        return distanceFirst.error();
    }
    if (!distanceSecond) {
        return distanceSecond.error();
    }
    return finish(document_.execute(
                      cmd::chamferEntities(*first, *second, *distanceFirst, *distanceSecond)),
                  "chamfered");
}

// ---- select -------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::select(const Tokens& args)
{
    if (args.empty()) {
        return usage("SELECT ALL | NONE | id... | LAYER name | TYPE name");
    }
    const auto& model = document_.model();
    const std::string mode = upper(args[0]);
    std::vector<EntityId> ids;

    const auto collect = [&](const SelectionFilter& filter) {
        model.entities.forEach([&](const Entity& entity) {
            // The document rule: the interpreter knows no view, and SELECT
            // ALL meaning "what the last-clicked view shows" would make a
            // typed command depend on a click the user may not remember.
            if (isSelectable(model, entity, kNoLayerOverrides) && filter.accepts(entity)) {
                ids.push_back(entity.id);
            }
        });
    };

    if (mode == "NONE") {
        // nothing to collect
    } else if (mode == "ALL") {
        collect({});
    } else if (mode == "LAYER" && args.size() == 2) {
        if (!model.layers.contains(args[1])) {
            return makeError(ErrorCode::NotFound, "layer does not exist", args[1]);
        }
        SelectionFilter filter;
        filter.layer = args[1];
        collect(filter);
    } else if (mode == "TYPE" && args.size() == 2) {
        // entityTypeFromString expects the spelling of the enumerator, so the
        // name is title cased. An empty argument has no first character to
        // keep: begin() + 1 would then be past end() and the range would be
        // inverted, so it is left alone and rejected below by name.
        std::string name = upper(args[1]);
        if (!name.empty()) {
            std::transform(name.begin() + 1, name.end(), name.begin() + 1,
                           [](unsigned char ch) { return static_cast<char>(std::tolower(ch)); });
        }
        const auto type = katana::entity::entityTypeFromString(name);
        if (!type) {
            return type.error();
        }
        SelectionFilter filter;
        filter.types.insert(*type);
        collect(filter);
    } else {
        for (const std::string& text : args) {
            const auto id = parseId(text);
            if (!id) {
                return id.error();
            }
            const Entity* entity = model.entities.find(*id);
            if (entity == nullptr) {
                return makeError(ErrorCode::NotFound, "entity does not exist", text);
            }
            if (!isSelectable(model, *entity, kNoLayerOverrides)) {
                return makeError(ErrorCode::CommandRejected,
                                 "entity is hidden or on a hidden or locked layer", text);
            }
            ids.push_back(*id);
        }
    }
    document_.selection().set(std::move(ids));
    document_.notifySelectionChanged();
    return std::to_string(document_.selection().size()) + " selected";
}

// ---- layers -------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::dimensionStyle(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upper(args[0]);

    if (action == "LIST") {
        std::ostringstream out;
        out.precision(6);
        for (const katana::entity::DimensionStyle& style : model.dimensionStyles.all()) {
            out << "  " << style.name << "  text=" << style.textHeight
                << "  arrow=" << style.arrowSize << " " << katana::entity::toString(style.arrowHead)
                << "  decimals=" << style.decimals;
            if (style.unitScale != 1.0) {
                out << "  scale=" << style.unitScale;
            }
            if (style.roundTo > 0.0) {
                out << "  round=" << style.roundTo;
            }
            if (!style.suffix.empty()) {
                out << "  suffix='" << style.suffix << "'";
            }
            // What a dimension of exactly ten units would read as. This is the
            // question anyone setting a style is actually asking, and working
            // it out from the fields is guesswork.
            out << "  (10 reads as " << katana::entity::formatMeasurement(10.0, style) << ")";
            out << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text;
    }

    if (args.size() < 2) {
        return usage("DIMSTYLE LIST | NEW name | SET name field value | DELETE name");
    }
    const std::string& name = args[1];

    if (action == "NEW") {
        katana::entity::DimensionStyle created;
        created.name = name;
        return finish(document_.execute(cmd::createDimensionStyle(std::move(created))),
                      "dimension style " + name + " created");
    }
    if (action == "DELETE") {
        return finish(document_.execute(cmd::deleteDimensionStyle(name)),
                      "dimension style " + name + " deleted");
    }
    if (action == "SET") {
        if (args.size() < 4) {
            return usage("DIMSTYLE SET name field value\n"
                         "  fields: TEXT GAP EXTOFF EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND"
                         " PREFIX SUFFIX TRIM");
        }
        const katana::entity::DimensionStyle* current = model.dimensionStyles.find(name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "dimension style does not exist", name);
        }
        katana::entity::DimensionStyle changed = *current;
        const std::string field = upper(args[2]);
        const std::string& value = args[3];

        const auto number = [&value]() { return parseNumber(value); };
        if (field == "TEXT") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.textHeight = *v;
        } else if (field == "GAP") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.textGap = *v;
        } else if (field == "EXTOFF") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.extensionOffset = *v;
        } else if (field == "EXTBEYOND") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.extensionBeyond = *v;
        } else if (field == "ARROW") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.arrowSize = *v;
        } else if (field == "HEAD") {
            const auto head = katana::entity::arrowHeadFromString(value);
            if (!head) {
                return head.error();
            }
            changed.arrowHead = *head;
        } else if (field == "SCALE") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.unitScale = *v;
        } else if (field == "DECIMALS") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.decimals = static_cast<int>(*v);
        } else if (field == "ROUND") {
            const auto v = number();
            if (!v) {
                return v.error();
            }
            changed.roundTo = *v;
        } else if (field == "PREFIX") {
            changed.prefix = value;
        } else if (field == "SUFFIX") {
            changed.suffix = value;
        } else if (field == "TRIM") {
            const std::string on = upper(value);
            changed.suppressTrailingZeros = on == "ON" || on == "1" || on == "YES";
        } else {
            return makeError(ErrorCode::InvalidArgument, "unknown dimension style field", field);
        }

        return finish(document_.execute(cmd::updateDimensionStyle(std::move(changed))),
                      "dimension style " + name + " updated");
    }
    return usage("DIMSTYLE LIST | NEW name | SET name field value | DELETE name");
}

CommandInterpreter::Reply CommandInterpreter::hatchPattern(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upper(args[0]);

    if (action == "LIST") {
        std::ostringstream out;
        out.precision(6);
        for (const katana::entity::HatchPattern& pattern : model.hatchPatterns.all()) {
            out << "  " << pattern.name;
            if (pattern.solid) {
                out << "  solid";
            } else if (pattern.families.empty()) {
                out << "  (draws nothing)";
            } else {
                for (const katana::entity::HatchLineFamily& family : pattern.families) {
                    out << "  [" << family.angle * katana::math::kRadToDeg << " deg @ "
                        << family.spacing;
                    if (family.offset != 0.0) {
                        out << " offset " << family.offset;
                    }
                    out << "]";
                }
            }
            if (!pattern.description.empty()) {
                out << "  " << pattern.description;
            }
            out << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text;
    }

    if (args.size() < 2) {
        return usage("HATCH LIST | SOLID name | NEW name angle spacing [angle spacing ...]"
                     " | DELETE name");
    }
    const std::string& name = args[1];

    if (action == "DELETE") {
        return finish(document_.execute(cmd::deleteHatchPattern(name)),
                      "hatch pattern " + name + " deleted");
    }
    if (action == "SOLID") {
        katana::entity::HatchPattern pattern;
        pattern.name = name;
        pattern.solid = true;
        pattern.description = "Solid fill";
        return finish(document_.execute(cmd::createHatchPattern(std::move(pattern))),
                      "hatch pattern " + name + " created (solid)");
    }
    if (action == "NEW") {
        // Angles are DEGREES here and radians in the model. Every other angle
        // the interpreter takes is in degrees, because that is what a drafter
        // types; converting at the edge keeps the model in one unit.
        if (args.size() < 4 || (args.size() - 2) % 2 != 0) {
            return usage("HATCH NEW name angle spacing [angle spacing ...]\n"
                         "  angle in DEGREES, spacing in MODEL units."
                         " e.g. HATCH NEW brick 45 0.25   (crosshatch: 45 0.25 135 0.25)");
        }
        katana::entity::HatchPattern pattern;
        pattern.name = name;
        for (std::size_t i = 2; i + 1 < args.size(); i += 2) {
            const auto angle = parseNumber(args[i]);
            if (!angle) {
                return angle.error();
            }
            const auto spacing = parseNumber(args[i + 1]);
            if (!spacing) {
                return spacing.error();
            }
            katana::entity::HatchLineFamily family;
            family.angle = *angle * katana::math::kDegToRad;
            family.spacing = *spacing;
            pattern.families.push_back(family);
        }
        return finish(document_.execute(cmd::createHatchPattern(std::move(pattern))),
                      "hatch pattern " + name + " created (" +
                          std::to_string(pattern.families.size()) + " families)");
    }
    return usage("HATCH LIST | SOLID name | NEW name angle spacing [angle spacing ...]"
                 " | DELETE name");
}

CommandInterpreter::Reply CommandInterpreter::alignment(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upper(args[0]);
    const char* const kUsage =
        "ALIGN LIST | NEW name x,y x,y [x,y ...] | PI name x,y [radius [spiralIn [spiralOut]]]"
        "\n      | SET name index radius [spiralIn [spiralOut]] | START name station"
        "\n      | STATIONS name interval | DELETE name     (PI indices count from 0)"
        "\n      | DESIGN name station,elevation[,curveLength] ... (at least two PVIs)"
        "\n      | PVI name station elevation [curveLength] | PROFILE name | CLEARPROFILE name";

    if (action == "LIST") {
        std::ostringstream out;
        out.precision(3);
        out << std::fixed;
        for (const katana::entity::Alignment& alignment : model.alignments.all()) {
            out << "  " << alignment.name << "  " << alignment.horizontal.pis.size() << " PIs";
            // Stored alignments always solve - add() refused any that could
            // not - so a failure here is a defect worth showing, not hiding.
            if (auto solved = katana::geometry::solveAlignment(alignment.horizontal)) {
                out << "  " << solved->startStation() << " to " << solved->endStation()
                    << "  length " << solved->length();
            } else {
                out << "  (does not solve: " << solved.error().describe() << ")";
            }
            if (!alignment.description.empty()) {
                out << "  " << alignment.description;
            }
            out << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text;
    }
    if (args.size() < 2) {
        return usage(kUsage);
    }
    const std::string& name = args[1];

    if (action == "NEW") {
        if (args.size() < 4) {
            return usage("ALIGN NEW name x,y x,y [x,y ...]   at least two PIs");
        }
        katana::entity::Alignment created;
        created.name = name;
        for (std::size_t i = 2; i < args.size(); ++i) {
            const auto point = parsePoint(args[i]);
            if (!point) {
                return point.error();
            }
            created.horizontal.pis.push_back(katana::geometry::AlignmentPI{*point});
        }
        return finish(document_.execute(cmd::createAlignment(std::move(created))),
                      "alignment " + name + " created with " +
                          std::to_string(args.size() - 2) + " PIs");
    }
    if (action == "DELETE") {
        return finish(document_.execute(cmd::deleteAlignment(name)),
                      "alignment " + name + " deleted");
    }

    const katana::entity::Alignment* existing = model.alignments.find(name);
    if (existing == nullptr) {
        return makeError(ErrorCode::NotFound, "alignment does not exist", name);
    }
    katana::entity::Alignment changed = *existing;

    if (action == "PI") {
        if (args.size() < 3) {
            return usage("ALIGN PI name x,y [radius [spiralIn [spiralOut]]]   appends a PI");
        }
        const auto point = parsePoint(args[2]);
        if (!point) {
            return point.error();
        }
        katana::geometry::AlignmentPI pi{*point};
        double* fields[] = {&pi.radius, &pi.spiralIn, &pi.spiralOut};
        for (std::size_t i = 3; i < args.size() && i < 6; ++i) {
            const auto value = parseNumber(args[i]);
            if (!value) {
                return value.error();
            }
            *fields[i - 3] = *value;
        }
        changed.horizontal.pis.push_back(pi);
        return finish(document_.execute(cmd::updateAlignment(std::move(changed))),
                      "alignment " + name + " now has " +
                          std::to_string(existing->horizontal.pis.size() + 1) + " PIs");
    }
    if (action == "SET") {
        if (args.size() < 4) {
            return usage("ALIGN SET name index radius [spiralIn [spiralOut]]   index from 0");
        }
        const auto index = parseNumber(args[2]);
        if (!index) {
            return index.error();
        }
        if (*index < 0.0 || *index >= static_cast<double>(changed.horizontal.pis.size())) {
            return makeError(ErrorCode::InvalidArgument, "no such PI", "index=" + args[2]);
        }
        katana::geometry::AlignmentPI& pi = changed.horizontal.pis[static_cast<std::size_t>(*index)];
        double* fields[] = {&pi.radius, &pi.spiralIn, &pi.spiralOut};
        for (std::size_t i = 3; i < args.size() && i < 6; ++i) {
            const auto value = parseNumber(args[i]);
            if (!value) {
                return value.error();
            }
            *fields[i - 3] = *value;
        }
        return finish(document_.execute(cmd::updateAlignment(std::move(changed))),
                      "alignment " + name + " PI " + args[2] + " updated");
    }
    if (action == "START") {
        if (args.size() < 3) {
            return usage("ALIGN START name station");
        }
        const auto station = parseNumber(args[2]);
        if (!station) {
            return station.error();
        }
        changed.horizontal.startStation = *station;
        return finish(document_.execute(cmd::updateAlignment(std::move(changed))),
                      "alignment " + name + " starts at " + args[2]);
    }
    if (action == "STATIONS") {
        if (args.size() < 3) {
            return usage("ALIGN STATIONS name interval");
        }
        const auto interval = parseNumber(args[2]);
        if (!interval) {
            return interval.error();
        }
        if (!(*interval > 0.0)) {
            return makeError(ErrorCode::InvalidArgument, "the interval must be positive");
        }
        auto solved = katana::geometry::solveAlignment(existing->horizontal);
        if (!solved) {
            return solved.error();
        }
        // Every interval station plus every key station, in order, once. A
        // setting-out table with the TS, SC, CS and ST missing is not one.
        std::vector<double> stations = solved->keyStations();
        for (double s = solved->startStation(); s < solved->endStation(); s += *interval) {
            stations.push_back(s);
        }
        std::sort(stations.begin(), stations.end());
        stations.erase(std::unique(stations.begin(), stations.end(),
                                   [](double a, double b) { return std::abs(a - b) < 1e-9; }),
                       stations.end());
        if (stations.size() > 100000) {
            return makeError(ErrorCode::InvalidArgument, "that interval gives too many stations",
                             std::to_string(stations.size()) + " stations, the limit is 100000");
        }
        std::ostringstream out;
        out << std::fixed;
        out.precision(3);
        out << "  station        x             y        direction  radius\n";
        for (double s : stations) {
            const auto point = solved->pointAtStation(s);
            const auto direction = solved->directionAtStation(s);
            const auto curvature = solved->curvatureAtStation(s);
            if (!point || !direction || !curvature) {
                continue;
            }
            out << "  " << std::setw(10) << s << "  " << std::setw(12) << point->x << "  "
                << std::setw(12) << point->y << "  " << std::setw(8)
                << *direction * katana::math::kRadToDeg << "  ";
            if (*curvature == 0.0) {
                out << "straight";
            } else {
                out << std::setw(8) << 1.0 / *curvature;
            }
            out << "\n";
        }
        std::string text = out.str();
        text.pop_back();
        return text;
    }
    if (action == "DESIGN") {
        // The whole profile at once, because a profile with one PVI cannot be
        // built and the model refuses to hold one - so it cannot be grown from
        // nothing one PVI at a time. PVI appends to a profile that exists.
        if (args.size() < 4) {
            return usage("ALIGN DESIGN name station,elevation[,curveLength] ...   at least two;"
                         " the first and last carry no curve");
        }
        katana::geometry::VerticalAlignment profile;
        for (std::size_t i = 2; i < args.size(); ++i) {
            const auto pvi = parsePVI(args[i]);
            if (!pvi) {
                return pvi.error();
            }
            profile.pvis.push_back(*pvi);
        }
        changed.vertical = std::move(profile);
        return finish(document_.execute(cmd::updateAlignment(std::move(changed))),
                      "alignment " + name + " designed with " + std::to_string(args.size() - 2) +
                          " PVIs");
    }
    if (action == "PVI") {
        if (args.size() < 4) {
            return usage("ALIGN PVI name station elevation [curveLength]   appends a PVI to an"
                         " existing design profile; use DESIGN to start one");
        }
        if (!existing->vertical.has_value()) {
            return makeError(ErrorCode::InvalidState,
                             "the alignment has no design profile to append to; define one with"
                             " ALIGN DESIGN",
                             name);
        }
        const auto station = parseNumber(args[2]);
        if (!station) {
            return station.error();
        }
        const auto elevation = parseNumber(args[3]);
        if (!elevation) {
            return elevation.error();
        }
        double curveLength = 0.0;
        if (args.size() > 4) {
            const auto length = parseNumber(args[4]);
            if (!length) {
                return length.error();
            }
            curveLength = *length;
        }
        if (!changed.vertical.has_value()) {
            changed.vertical.emplace();
        }
        changed.vertical->pvis.push_back(
            katana::geometry::ProfilePVI{*station, *elevation, curveLength});
        return finish(document_.execute(cmd::updateAlignment(std::move(changed))),
                      "alignment " + name + " profile now has " +
                          std::to_string(changed.vertical->pvis.size()) + " PVIs");
    }
    if (action == "PROFILE") {
        if (!existing->vertical.has_value()) {
            return "alignment " + name + " has no design profile";
        }
        auto solved = katana::geometry::solveProfile(*existing->vertical);
        if (!solved) {
            return solved.error();
        }
        std::ostringstream out;
        out << std::fixed;
        out.precision(3);
        out << "  PVIs:";
        for (const katana::geometry::ProfilePVI& pvi : existing->vertical->pvis) {
            out << "  " << pvi.station << " @ " << pvi.elevation;
            if (pvi.curveLength > 0.0) {
                out << " L=" << pvi.curveLength;
            }
        }
        out << "\n";
        for (const katana::geometry::ProfileElement& element : solved->elements()) {
            out << "  " << (element.kind == katana::geometry::ProfileElementKind::Curve
                                ? "curve  "
                                : "tangent")
                << "  " << std::setw(10) << element.startStation << " to " << std::setw(10)
                << element.startStation + element.length << "  grade " << std::setw(7)
                << element.startGrade * 100.0 << "%";
            if (element.kind == katana::geometry::ProfileElementKind::Curve) {
                out << " to " << std::setw(7) << element.endGrade * 100.0 << "%";
            }
            out << "\n";
        }
        for (const katana::geometry::ProfileExtremum& point : solved->highLowPoints()) {
            out << "  " << (point.high ? "high point" : "low point ") << " at " << point.station
                << " @ " << point.elevation << "\n";
        }
        std::string text = out.str();
        text.pop_back();
        return text;
    }
    if (action == "CLEARPROFILE") {
        changed.vertical.reset();
        return finish(document_.execute(cmd::updateAlignment(std::move(changed))),
                      "alignment " + name + " profile removed");
    }
    return usage(kUsage);
}

CommandInterpreter::Reply CommandInterpreter::parcel(const Tokens& args)
{
    const char* const kUsage = "PARCEL id | PARCEL id LEGAL [name] | PARCEL id LABEL [height]";
    if (args.empty()) {
        return usage(kUsage);
    }
    const auto id = parseId(args[0]);
    if (!id) {
        return id.error();
    }
    const auto& model = document_.model();
    const Entity* entity = model.entities.find(*id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "no entity with that id", args[0]);
    }
    const auto* boundary = std::get_if<katana::geometry::Polyline2>(&entity->geometry);
    if (boundary == nullptr) {
        return makeError(ErrorCode::InvalidGeometry, "a parcel must be a closed polyline", args[0]);
    }
    auto report = parcelReport(*boundary);
    if (!report) {
        return report.error();
    }
    const std::string action = args.size() > 1 ? upper(args[1]) : "REPORT";

    if (action == "LEGAL") {
        const std::string name = args.size() > 2 ? args[2] : "Parcel " + args[0];
        return legalDescription(*report, name);
    }
    if (action == "LABEL") {
        double height = 2.5;
        if (args.size() > 2) {
            const auto value = parseNumber(args[2]);
            if (!value) {
                return value.error();
            }
            height = *value;
        }
        auto labels = parcelLabels(*report, height);
        if (!labels) {
            return labels.error();
        }
        // One undo step for the lot: nobody wants to undo a parcel's labels
        // one bearing at a time.
        const cmd::EntityAttributes attributes = document_.currentAttributes();
        auto chain = std::make_unique<cmd::Transaction>("PARCEL_LABELS");
        for (katana::entity::TextGeometry& label : *labels) {
            chain->add(cmd::createText(std::move(label), attributes));
        }
        const std::size_t count = labels->size();
        return finish(document_.execute(std::move(chain)),
                      std::to_string(count) + " labels created on the current layer");
    }
    if (action != "REPORT") {
        return usage(kUsage);
    }

    std::ostringstream out;
    out << std::fixed;
    out.precision(3);
    out << "  course  from                      bearing           distance\n";
    std::size_t index = 1;
    for (const ParcelCourse& course : report->courses) {
        out << "  " << std::setw(4) << index++ << "    " << std::setw(10) << course.from.x << ","
            << std::setw(10) << course.from.y << "   " << course.bearing << "   " << std::setw(10)
            << course.distance << "\n";
    }
    out << "  area " << report->area << " m2 (" << report->area / 10000.0 << " ha), perimeter "
        << report->perimeter << " m, centroid " << report->centroid.x << "," << report->centroid.y
        << ", drawn " << (report->clockwise ? "clockwise" : "counter-clockwise");
    return out.str();
}

// ---- survey tools ---------------------------------------------------------------------------
//
// Parsing only: every number comes from survey_tools.hpp, and so does every
// word of the reply, which is the report the Survey menu's dialogs show.

CommandInterpreter::Reply CommandInterpreter::survey(const std::string& verb, const Tokens& args)
{
    if (verb == "INVERSE") {
        const char* const kUsage = "INVERSE p p | INVERSE line-id   (p = E,N or E,N,Z or a point id)";
        Result<InverseResult> result = makeError(ErrorCode::Internal, "unreached");
        if (args.size() == 1) {
            const auto id = parseId(args[0]);
            if (!id) {
                return usage(kUsage);
            }
            const auto ends = endsOfLine(document_, *id);
            if (!ends) {
                return ends.error();
            }
            result = computeInverse(ends->first, ends->second);
        } else if (args.size() == 2) {
            const auto from = parseSurveyPosition(document_, args[0]);
            if (!from) {
                return from.error();
            }
            const auto to = parseSurveyPosition(document_, args[1]);
            if (!to) {
                return to.error();
            }
            result = computeInverse(*from, *to);
        } else {
            return usage(kUsage);
        }
        if (!result) {
            return result.error();
        }
        return formatInverseReport(*result);
    }

    if (verb == "FORWARD") {
        if (args.size() < 3 || args.size() > 5) {
            return usage("FORWARD p direction distance [dZ [name]]   (dZ - for none)");
        }
        ForwardInput input;
        const auto from = parseSurveyPosition(document_, args[0]);
        if (!from) {
            return from.error();
        }
        input.from = *from;
        const auto direction = parseSurveyDirection(args[1]);
        if (!direction) {
            return direction.error();
        }
        input.azimuth = *direction;
        const auto distance = parseSurveyNumber(args[2], "the horizontal distance");
        if (!distance) {
            return distance.error();
        }
        input.distance = *distance;
        // The name only ever comes fifth, so a point numbered "102" is never
        // taken for a height difference of 102.
        if (args.size() >= 4 && args[3] != "-") {
            const auto dz = parseSurveyNumber(args[3], "the height difference");
            if (!dz) {
                return dz.error();
            }
            input.heightDifference = *dz;
        }
        if (args.size() == 5) {
            input.name = args[4];
        }
        const auto result = computeForward(input);
        if (!result) {
            return result.error();
        }
        auto command = forwardPointCommand(document_, *result);
        if (!command) {
            return command.error();
        }
        return finish(document_.execute(std::move(*command)),
                      formatForwardReport(*result) + "\n  Created on layer " +
                          document_.currentLayer());
    }

    // AREA
    std::vector<EntityId> ids;
    if (args.empty()) {
        if (auto selected = requireSelection(); !selected) {
            return selected.error();
        }
        ids = document_.selection().ids();
    }
    for (const std::string& arg : args) {
        const auto id = parseId(arg);
        if (!id) {
            return usage("AREA [id...]   (the selection when no ids)");
        }
        ids.push_back(*id);
    }
    const auto result = computeArea(document_, ids);
    if (!result) {
        return result.error();
    }
    return formatAreaReport(*result);
}

CommandInterpreter::Reply CommandInterpreter::linetype(const Tokens& args)
{
    static constexpr const char* kLinetypeUsage =
        "LINETYPE LIST | NEW name dash gap [dash gap ...] | RENAME old new | DELETE name |"
        " MERGE from into";
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upper(args[0]);

    if (action == "LIST") {
        std::ostringstream out;
        out.precision(6);
        for (const katana::entity::Linetype& linetype : model.linetypes.all()) {
            out << "  " << linetype.name;
            if (linetype.isContinuous()) {
                out << "  (solid)";
            } else {
                out << "  [";
                for (std::size_t i = 0; i < linetype.pattern.size(); ++i) {
                    out << (i == 0 ? "" : " ") << linetype.pattern[i].length;
                }
                out << "]  period=" << linetype.patternLength();
            }
            if (!linetype.description.empty()) {
                out << "  " << linetype.description;
            }
            out << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text;
    }

    if (args.size() < 2) {
        return usage(kLinetypeUsage);
    }
    const std::string& name = args[1];

    if (action == "NEW") {
        if (args.size() < 3) {
            return usage("LINETYPE NEW name dash gap [dash gap ...]"
                         "   (positive = dash, negative = gap, 0 = dot; model units)");
        }
        katana::entity::Linetype created;
        created.name = name;
        created.description = "User defined";
        for (std::size_t i = 2; i < args.size(); ++i) {
            const auto length = parseNumber(args[i]);
            if (!length) {
                return length.error();
            }
            created.pattern.push_back(katana::entity::LinetypeElement{*length});
        }
        // validate() has the real rules; this just gives the common mistake a
        // better sentence than "must end with a gap".
        if (created.pattern.size() % 2 != 0) {
            return makeError(ErrorCode::InvalidArgument,
                             "give a gap after every dash: lengths come in pairs",
                             std::to_string(created.pattern.size()) + " given");
        }
        return finish(document_.execute(cmd::createLinetype(std::move(created))),
                      "linetype " + name + " created");
    }
    if (action == "DELETE") {
        return finish(document_.execute(cmd::deleteLinetype(name)),
                      "linetype " + name + " deleted");
    }
    if (action == "RENAME") {
        if (args.size() < 3) {
            return usage("LINETYPE RENAME old new");
        }
        return finish(document_.execute(cmd::renameLinetype(name, args[2])),
                      "linetype " + name + " renamed to " + args[2] +
                          " (every layer and style naming it came too)");
    }
    if (action == "MERGE") {
        if (args.size() != 3) {
            return usage("LINETYPE MERGE from into   (put a name with spaces in double quotes)");
        }
        return finish(document_.execute(cmd::mergeLinetype(name, args[2])),
                      "linetype " + name + " merged into " + args[2] +
                          " (every layer and style naming it now names " + args[2] + ")");
    }
    return usage(kLinetypeUsage);
}

CommandInterpreter::Reply CommandInterpreter::layer(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upper(args[0]);

    if (action == "LIST") {
        std::ostringstream out;
        for (const katana::entity::Layer& layer : model.layers.all()) {
            out << (layer.name == document_.currentLayer() ? "* " : "  ") << layer.name << "  "
                << layer.color.toHex() << (layer.visible ? "" : "  hidden")
                << (layer.locked ? "  locked" : "") << "  ("
                << model.entities.countOnLayer(layer.name) << " entities)\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text;
    }
    if (args.size() < 2) {
        return usage("LAYER LIST | NEW name [#RRGGBB] | SET|DELETE|SHOW|HIDE|LOCK|UNLOCK name"
                     " | LTYPE layer linetype");
    }
    const std::string& name = args[1];

    if (action == "NEW") {
        katana::entity::Layer created;
        created.name = name;
        if (args.size() >= 3) {
            const auto color = katana::entity::Color::fromHex(args[2]);
            if (!color) {
                return color.error();
            }
            created.color = *color;
        }
        return finish(document_.execute(cmd::createLayer(std::move(created))),
                      "layer " + name + " created");
    }
    if (action == "SET") {
        return finish(document_.setCurrentLayer(name), "current layer is " + name);
    }
    if (action == "LTYPE") {
        // LAYER LTYPE <layer> <linetype>. Without a way to say this, a defined
        // pattern can never be attached to anything and the whole feature is
        // unreachable from the application.
        if (args.size() < 3) {
            return usage("LAYER LTYPE layer linetype");
        }
        const katana::entity::Layer* existing = model.layers.find(name);
        if (existing == nullptr) {
            return makeError(ErrorCode::NotFound, "layer does not exist", name);
        }
        // The same names STYLE SET linetype takes, bar ByLayer: a layer is
        // what ByLayer inherits FROM.
        const std::string linetypeName = restOfLine(args, 2);
        if (katana::entity::isByLayer(linetypeName)) {
            return makeError(ErrorCode::InvalidArgument,
                             "a layer cannot be ByLayer: it is what ByLayer inherits from",
                             linetypeName);
        }
        if (auto status = checkLinetypeName(document_, linetypeName); !status) {
            return status.error();
        }
        katana::entity::Layer changed = *existing;
        changed.linetype = linetypeName;
        return finish(document_.execute(cmd::updateLayer(std::move(changed))),
                      "layer " + name + " uses linetype " + linetypeName);
    }
    if (action == "HATCH") {
        if (args.size() < 3) {
            return usage("LAYER HATCH layer pattern");
        }
        const katana::entity::Layer* existing = model.layers.find(name);
        if (existing == nullptr) {
            return makeError(ErrorCode::NotFound, "layer does not exist", name);
        }
        if (!model.hatchPatterns.contains(args[2])) {
            return makeError(ErrorCode::NotFound, "hatch pattern does not exist", args[2]);
        }
        katana::entity::Layer changed = *existing;
        changed.hatchPattern = args[2];
        return finish(document_.execute(cmd::updateLayer(std::move(changed))),
                      "layer " + name + " uses hatch pattern " + args[2]);
    }
    if (action == "DIMSTYLE") {
        if (args.size() < 3) {
            return usage("LAYER DIMSTYLE layer style");
        }
        const katana::entity::Layer* existing = model.layers.find(name);
        if (existing == nullptr) {
            return makeError(ErrorCode::NotFound, "layer does not exist", name);
        }
        if (!model.dimensionStyles.contains(args[2])) {
            return makeError(ErrorCode::NotFound, "dimension style does not exist", args[2]);
        }
        katana::entity::Layer changed = *existing;
        changed.dimensionStyle = args[2];
        return finish(document_.execute(cmd::updateLayer(std::move(changed))),
                      "layer " + name + " uses dimension style " + args[2]);
    }
    if (action == "DELETE") {
        return finish(document_.execute(cmd::deleteLayer(name)), "layer " + name + " deleted");
    }

    const katana::entity::Layer* current = model.layers.find(name);
    if (current == nullptr) {
        return makeError(ErrorCode::NotFound, "layer does not exist", name);
    }
    katana::entity::Layer changed = *current;
    if (action == "SHOW" || action == "HIDE") {
        changed.visible = action == "SHOW";
    } else if (action == "LOCK" || action == "UNLOCK") {
        changed.locked = action == "LOCK";
        if (changed.locked && name == document_.currentLayer()) {
            return makeError(ErrorCode::CommandRejected, "the current layer cannot be locked", name);
        }
    } else {
        return makeError(ErrorCode::ParseFailure, "unknown LAYER action", action);
    }
    return finish(document_.execute(cmd::updateLayer(std::move(changed))),
                  "layer " + name + " updated");
}

// ---- attributes ---------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::attributes(const std::string& verb,
                                                         const Tokens& args)
{
    if (auto selected = requireSelection(); !selected) {
        return selected;
    }
    const std::vector<EntityId> ids = document_.selection().ids();
    const std::string count = std::to_string(ids.size());

    if (verb == "CHLAYER") {
        if (args.size() != 1) {
            return usage("CHLAYER name");
        }
        return finish(document_.execute(cmd::setEntityLayer(ids, args[0])),
                      count + " moved to layer " + args[0]);
    }
    if (verb == "COLOR") {
        if (args.size() != 1) {
            return usage("COLOR #RRGGBB | BYLAYER");
        }
        std::optional<katana::entity::Color> color;
        if (upper(args[0]) != "BYLAYER") {
            const auto parsed = katana::entity::Color::fromHex(args[0]);
            if (!parsed) {
                return parsed.error();
            }
            color = *parsed;
        }
        return finish(document_.execute(cmd::setEntityColor(ids, color)), count + " recoloured");
    }
    // PROP. Verb-first, as LAYER, STYLE, LINETYPE and HATCH are: the old
    // bare `PROP key value` could not be extended without becoming
    // ambiguous, since a property may be named DELETE.
    static constexpr const char* kPropUsage =
        "PROP LIST | SET key value [text|integer|real|boolean] | DELETE key | RENAME old new";
    const std::string action = args.empty() ? std::string("LIST") : upper(args[0]);

    if (action == "LIST") {
        std::ostringstream out;
        for (const EntityId id : ids) {
            const katana::entity::Entity* entity = document_.model().entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            out << "  " << id << ":";
            if (entity->properties.empty()) {
                out << " (none)";
            }
            for (const auto& [key, value] : entity->properties) {
                out << " " << key << "=" << katana::entity::toString(value) << " ("
                    << katana::entity::typeName(value) << ")";
            }
            out << "\n";
        }
        std::string text = out.str();
        if (!text.empty()) {
            text.pop_back();
        }
        return text;
    }
    if (action == "SET") {
        if (args.size() < 3 || args.size() > 4) {
            return usage(kPropUsage);
        }
        // The type is guessed from the value unless it is stated. Survey code
        // attributes are typed, and "2" as text is not 2 as an integer.
        auto value = args.size() == 4 ? typedPropertyValue(args[3], args[2])
                                      : katana::core::Result<katana::entity::PropertyValue>(
                                            parsePropertyValue(args[2]));
        if (!value) {
            return value.error();
        }
        return finish(document_.execute(cmd::setEntityProperty(ids, args[1], std::move(*value))),
                      "property " + args[1] + " set on " + count);
    }
    if (action == "DELETE") {
        if (args.size() != 2) {
            return usage(kPropUsage);
        }
        return finish(document_.execute(cmd::removeEntityProperty(ids, args[1])),
                      "property " + args[1] + " removed from " + count);
    }
    if (action == "RENAME") {
        if (args.size() != 3) {
            return usage(kPropUsage);
        }
        return finish(document_.execute(cmd::renameEntityProperty(ids, args[1], args[2])),
                      "property " + args[1] + " renamed to " + args[2] + " on " + count);
    }
    return usage(kPropUsage);
}

// ---- history / file / inspect -----------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::undoRedo(const std::string& verb, const Tokens& args)
{
    int steps = 1;
    if (!args.empty()) {
        const auto parsed = parseCount(args[0]);
        if (!parsed) {
            return parsed.error();
        }
        steps = *parsed;
    }
    int done = 0;
    for (; done < steps; ++done) {
        const Status status = verb == "UNDO" ? document_.undo() : document_.redo();
        if (!status) {
            if (done == 0) {
                return status.error();
            }
            break; // ran out of history part way: report what was done
        }
    }
    return std::to_string(done) + (verb == "UNDO" ? " undone" : " redone");
}

CommandInterpreter::Reply CommandInterpreter::file(const std::string& verb, const Tokens& args)
{
    if (verb == "NEW") {
        document_.newDocument();
        resetPointState();
        return std::string("new drawing");
    }
    if (verb == "OPEN") {
        if (args.size() != 1) {
            return usage("OPEN project-directory");
        }
        resetPointState();
        // Sequenced deliberately. Building the message inside the finish() call
        // made the entity count an ARGUMENT alongside document_.open(), and the
        // order in which function arguments are evaluated is unspecified in
        // C++ - so the count was read before the open and every OPEN reported
        // "(0 entities)". It is not undefined behaviour, just unspecified, and
        // it reads as correct code.
        auto status = document_.open(args[0]);
        const std::size_t count = document_.model().entities.size();
        return finish(std::move(status),
                      "opened " + args[0] + " (" + std::to_string(count) + " entities)");
    }
    // SAVE
    if (args.size() > 1) {
        return usage("SAVE [project-directory]");
    }
    if (args.size() == 1) {
        return finish(document_.saveAs(args[0]), "saved to " + args[0]);
    }
    return finish(document_.save(), "saved");
}

CommandInterpreter::Reply CommandInterpreter::inspect(const std::string& verb,
                                                      const Tokens& args) const
{
    const auto& model = document_.model();
    if (verb == "INFO") {
        if (args.size() != 1) {
            return usage("INFO id");
        }
        const auto id = parseId(args[0]);
        if (!id) {
            return id.error();
        }
        const Entity* entity = model.entities.find(*id);
        if (entity == nullptr) {
            return makeError(ErrorCode::NotFound, "entity does not exist", args[0]);
        }
        std::string text = describe(*entity);
        for (const auto& [key, value] : entity->properties) {
            text += "\n    " + key + " = ";
            std::visit(
                [&](const auto& v) {
                    std::ostringstream out;
                    out << std::boolalpha << v;
                    text += out.str();
                },
                value);
        }
        return text;
    }
    std::string text = std::to_string(model.entities.size()) + " entities";
    model.entities.forEach([&](const Entity& entity) { text += "\n" + describe(entity); });
    return text;
}

} // namespace katana::cad
