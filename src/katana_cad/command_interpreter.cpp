#include "katana/cad/command_interpreter.hpp"

#include "katana/entity/dimension_text.hpp"

#include <algorithm>
#include <cctype>
#include <charconv>
#include <cmath>
#include <map>
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
        {"LT", "LINETYPE"}, {"LTYPE", "LINETYPE"}, {"DS", "DIMSTYLE"}, {"HA", "HATCH"},
        {"?", "HELP"},
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
Linetype  LINETYPE LIST | NEW name dash gap [dash gap ...] | DELETE name
          lengths are MODEL units: + dash, - gap, 0 dot. e.g. LINETYPE NEW fence 1 -0.5
Hatch     HATCH LIST | SOLID name | NEW name angle spacing [angle spacing ...] | DELETE name
          angle in DEGREES, spacing in MODEL units.  LAYER HATCH layer pattern attaches one
DimStyle  DIMSTYLE LIST | NEW name | SET name field value | DELETE name
          fields TEXT GAP EXTOFF EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND PREFIX SUFFIX TRIM
          LAYER DIMSTYLE layer style   attaches one
Attribs   CHLAYER name | COLOR #RRGGBB|BYLAYER | PROP key value   (selection)
History   UNDO [n] | REDO [n]
File      NEW | OPEN directory | SAVE [directory]
Inspect   LIST | INFO id | HELP
Aliases   L PL C A PO REC T M CO RO SC MI AR E O TR EX F CHA U LA SEL ?)";
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
        return helpText();
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
            if (isSelectable(model, entity) && filter.accepts(entity)) {
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
            if (!isSelectable(model, *entity)) {
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

CommandInterpreter::Reply CommandInterpreter::linetype(const Tokens& args)
{
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
        return usage("LINETYPE LIST | NEW name dash gap [dash gap ...] | DELETE name");
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
    return usage("LINETYPE LIST | NEW name dash gap [dash gap ...] | DELETE name");
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
        if (!model.linetypes.contains(args[2])) {
            return makeError(ErrorCode::NotFound, "linetype does not exist", args[2]);
        }
        katana::entity::Layer changed = *existing;
        changed.linetype = args[2];
        return finish(document_.execute(cmd::updateLayer(std::move(changed))),
                      "layer " + name + " uses linetype " + args[2]);
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
    // PROP
    if (args.size() != 2) {
        return usage("PROP key value");
    }
    return finish(document_.execute(cmd::setEntityProperty(ids, args[0], parsePropertyValue(args[1]))),
                  "property " + args[0] + " set on " + count);
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
