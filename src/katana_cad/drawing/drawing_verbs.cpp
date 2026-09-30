// The drawing system's verbs of the command line (docs/drawing.md, "The
// command line"): CommandInterpreter's members for the VERTEX verbs and the
// whole-polyline edits (WEED, DENSIFY, STRAIGHTEN, CLOSE, OPEN, STARTVERTEX,
// VERTEXZ), the draw verbs (PLINE with ARC and LINE, PLINE3D, SPLINE,
// ELLIPSE, XLINE, RAY, DLINE) and the drafting settings (ORTHO, POLAR,
// TRACKING, ANGLES, LOCK, SNAP, DRAFTING). Kept here, with the rest of the
// drawing system, rather than in command_interpreter.cpp.
//
// Written for an agent as much as for a person, as the annotation verbs are:
//
//   * OPTIONS ARE key=value, in any order after the positional arguments,
//     keys case-insensitive; an unknown key is refused naming the known ones.
//   * REPLIES ARE RECORDS: `key=value` pairs separated by spaces, one record
//     per line; numbers read back exactly; what was made says its id.
//   * EVERY EDIT IS ONE STEP through Document::execute, built by the same
//     functions the grips, the tools and the Vertices panel use
//     (polyline_vertices.hpp, vertex_editing.hpp, vertex_table.hpp,
//     draw_shapes.hpp), so a vertex moved by a verb and one dragged by a
//     grip are the same edit. The drafting settings are not drawing edits:
//     they change at once and are not on the undo stack.
//
// Vertex indices count from 0, as the Vertices panel's first column does.

#include <algorithm>
#include <cmath>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/drawing/draw_shapes.hpp"
#include "katana/cad/drawing/drafting.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/drawing/vertex_table.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/polyline_vertices.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::CurvePolyline2;
using katana::geometry::CurveVertex;
using katana::geometry::Point2;
using katana::geometry::Vec2;
namespace cmd = katana::commands;
namespace geo = katana::geometry;

namespace {

std::string upperOf(std::string text)
{
    for (char& c : text) {
        if (c >= 'a' && c <= 'z') {
            c = static_cast<char>(c - 'a' + 'A');
        }
    }
    return text;
}

std::string lowerOf(std::string text)
{
    for (char& c : text) {
        if (c >= 'A' && c <= 'Z') {
            c = static_cast<char>(c - 'A' + 'a');
        }
    }
    return text;
}

katana::core::Error usageError(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

// The verbs' own short names, mapped to the name each is handled under.
const std::map<std::string, std::string, std::less<>>& drawingAliases()
{
    static const std::map<std::string, std::string, std::less<>> table = {
        {"3DPOLY", "PLINE3D"}, {"3DPOLYLINE", "PLINE3D"}, {"SPL", "SPLINE"},
        {"EL", "ELLIPSE"},     {"XL", "XLINE"},           {"DL", "DLINE"},
        {"VERTICES", "VERTEX"}, {"SIMPLIFY", "WEED"},     {"OSNAP", "SNAP"},
    };
    return table;
}

std::string canonical(const std::string& verb)
{
    const auto found = drawingAliases().find(verb);
    return found == drawingAliases().end() ? verb : found->second;
}

// ---- options -------------------------------------------------------------------------

bool isOptionKey(std::string_view key)
{
    return !key.empty() && std::all_of(key.begin(), key.end(), [](char c) {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
               c == '_';
    });
}

bool isOptionToken(std::string_view token)
{
    const std::size_t equals = token.find('=');
    return equals != std::string_view::npos && equals > 0 &&
           isOptionKey(token.substr(0, equals));
}

struct Arguments {
    std::vector<std::string> positional;
    // In the order given, since VERTEX SET applies its fields in turn.
    std::vector<std::pair<std::string, std::string>> options;

    [[nodiscard]] const std::string* find(std::string_view key) const
    {
        for (const auto& [k, v] : options) {
            if (k == key) {
                return &v;
            }
        }
        return nullptr;
    }
};

Result<Arguments> splitArguments(const std::vector<std::string>& args, std::size_t from,
                                 std::initializer_list<std::string_view> known)
{
    Arguments result;
    for (std::size_t i = from; i < args.size(); ++i) {
        const std::string& token = args[i];
        if (!isOptionToken(token)) {
            result.positional.push_back(token);
            continue;
        }
        const std::size_t equals = token.find('=');
        std::string key = lowerOf(token.substr(0, equals));
        if (std::find(known.begin(), known.end(), key) == known.end()) {
            std::string list;
            for (const std::string_view k : known) {
                list += list.empty() ? "" : " ";
                list += k;
            }
            return makeError(ErrorCode::InvalidArgument, "unknown option '" + key + "'",
                             "known: " + (list.empty() ? std::string("none") : list));
        }
        result.options.emplace_back(std::move(key), token.substr(equals + 1));
    }
    return result;
}

Result<double> numberOf(std::string_view text, const char* what)
{
    if (const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(text))) {
        return *value;
    }
    return makeError(ErrorCode::ParseFailure, std::string("expected a number for ") + what,
                     std::string(text));
}

Result<double> positiveOf(std::string_view text, const char* what)
{
    auto value = numberOf(text, what);
    if (value && !(*value > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, std::string(what) + " must be greater than zero",
                         std::string(text));
    }
    return value;
}

Result<bool> switchOf(const std::string& text, const char* what)
{
    const std::string value = lowerOf(text);
    if (value == "on" || value == "yes" || value == "true" || value == "1") {
        return true;
    }
    if (value == "off" || value == "no" || value == "false" || value == "0") {
        return false;
    }
    return makeError(ErrorCode::ParseFailure, std::string("expected on or off for ") + what, text);
}

Result<std::size_t> indexOf(std::string_view text, const char* what)
{
    const auto value = katana::core::parseInteger(katana::core::trimmed(text));
    if (!value || *value < 0) {
        return makeError(ErrorCode::ParseFailure,
                         std::string("expected a vertex index (0 or more) for ") + what,
                         std::string(text));
    }
    return static_cast<std::size_t>(*value);
}

Result<EntityId> entityIdOf(std::string_view text)
{
    std::string_view body = katana::core::trimmed(text);
    if (!body.empty() && body.front() == '#') {
        body.remove_prefix(1);
    }
    const auto value = katana::core::parseInteger(body);
    if (!value || *value <= 0) {
        return makeError(ErrorCode::ParseFailure, "expected an entity id (12 or #12)",
                         std::string(text));
    }
    return static_cast<EntityId>(*value);
}

// ---- replies ---------------------------------------------------------------------------

std::string real(double value)
{
    return katana::core::formatExactReal(value);
}

std::string onOff(bool value)
{
    return value ? "on" : "off";
}

std::string heightText(const std::optional<double>& height)
{
    return height ? real(*height) : std::string("none");
}

// Whole-circle bearing in decimal degrees, clockwise from north, 0 to 360.
std::string bearingDegrees(double radians)
{
    double degrees = 90.0 - radians * katana::math::kRadToDeg;
    degrees = std::fmod(degrees, 360.0);
    if (degrees < 0.0) {
        degrees += 360.0;
    }
    return real(degrees);
}

std::string kindText(const Entity& entity)
{
    return lowerOf(std::string(katana::entity::toString(entity.type())));
}

// The summary record of polyline `id` as it now stands.
std::string polylineRecord(const Document& document, EntityId id)
{
    const Entity* entity = document.model().entities.find(id);
    if (entity == nullptr) {
        return "id=" + std::to_string(id) + " deleted=yes";
    }
    const auto polyline = readPolyline(*entity);
    if (!polyline) {
        return "id=" + std::to_string(id) + " kind=" + kindText(*entity);
    }
    std::size_t heights = 0;
    for (const auto& vertex : polyline->vertices) {
        heights += vertex.height ? 1 : 0;
    }
    std::ostringstream out;
    out << "id=" << id << " kind=" << kindText(*entity)
        << " closed=" << (polyline->closed ? "yes" : "no")
        << " vertices=" << polyline->vertices.size() << " arcs=" << (polyline->hasArcs() ? "yes" : "no")
        << " heights=" << heights << " length=" << real(polyline->length());
    return out.str();
}

std::string vertexRecord(const VertexRow& row)
{
    std::ostringstream out;
    out << "index=" << row.index << " x=" << real(row.easting) << " y=" << real(row.northing)
        << " z=" << heightText(row.height) << " bulge=" << real(row.bulge)
        << " bearing=" << (row.bearing ? bearingDegrees(*row.bearing) : std::string("none"))
        << " distance=" << (row.distance ? real(*row.distance) : std::string("none"));
    return out.str();
}

std::string modeNames(SnapModes modes)
{
    std::string text;
    for (std::uint32_t bit = 1; bit != 0 && bit <= static_cast<std::uint32_t>(SnapMode::MidBetween);
         bit <<= 1) {
        if ((modes & bit) != 0) {
            // As modesOf reads it: one word, "apparentintersection" - a
            // blank inside a value would end it for every key=value reader.
            text += text.empty() ? "" : ",";
            for (const char c : lowerOf(toString(static_cast<SnapMode>(bit)))) {
                if (c != ' ') {
                    text += c;
                }
            }
        }
    }
    return text.empty() ? std::string("none") : text;
}

Result<SnapModes> modesOf(const std::string& text)
{
    SnapModes modes = 0;
    const std::string lower = lowerOf(text);
    if (lower == "none" || lower.empty()) {
        return modes;
    }
    if (lower == "all") {
        return kAllSnapModes;
    }
    std::size_t from = 0;
    while (from <= lower.size()) {
        const std::size_t comma = std::min(lower.find(',', from), lower.size());
        const std::string name = lower.substr(from, comma - from);
        bool found = false;
        // Every mode modeNames writes, so a record reads back.
        for (std::uint32_t bit = 1; bit <= static_cast<std::uint32_t>(SnapMode::MidBetween);
             bit <<= 1) {
            const std::string known = lowerOf(toString(static_cast<SnapMode>(bit)));
            std::string compact;
            for (const char c : known) {
                if (c != ' ') {
                    compact += c;
                }
            }
            if (name == known || name == compact || (name == "centre" && known == "center")) {
                modes |= bit;
                found = true;
            }
        }
        if (!found) {
            return makeError(ErrorCode::ParseFailure, "unknown snap mode '" + name + "'",
                             "known: endpoint midpoint center intersection perpendicular tangent "
                             "nearest grid quadrant node extension parallel "
                             "apparentintersection, all, none");
        }
        from = comma + 1;
    }
    return modes;
}

// An angle held in radians, in degrees to a nanodegree: typed as 15, it is
// held as 15 x pi / 180 and would come back as 14.999999999999998.
std::string degrees(double radians)
{
    return real(std::round(radians * katana::math::kRadToDeg * 1.0e9) / 1.0e9);
}

std::string draftingRecord(const DraftingSettings& s)
{
    std::ostringstream out;
    out << "ortho=" << onOff(s.ortho) << " polar=" << onOff(s.polar)
        << " increment=" << degrees(s.polarIncrement)
        << " tracking=" << onOff(s.objectTracking)
        << " angles=" << (s.angles == AngleConvention::Bearing ? "bearing" : "ccw")
        << " anglelock="
        << (s.angleLock ? degrees(*s.angleLock) : std::string("none"))
        << " lengthlock=" << (s.lengthLock ? real(*s.lengthLock) : std::string("none"))
        << " snap=" << onOff(s.snapEnabled) << " modes=" << modeNames(s.snapModes);
    return out.str();
}

} // namespace

// ---- routing --------------------------------------------------------------------------------

bool CommandInterpreter::isDrawingVerb(const std::string& verb, const Tokens& args)
{
    static const std::set<std::string, std::less<>> kVerbs = {
        "VERTEX", "WEED",   "DENSIFY", "STRAIGHTEN", "CLOSE",    "STARTVERTEX", "VERTEXZ",
        "PLINE3D", "SPLINE", "ELLIPSE", "XLINE",     "RAY",      "DLINE",       "ORTHO",
        "POLAR",  "TRACKING", "ANGLES", "LOCK",      "SNAP",     "DRAFTING"};
    const std::string name = canonical(verb);
    if (kVerbs.contains(name)) {
        return true;
    }
    if (name == "PLINE") {
        // Every PLINE, the plain PLINE p p [CLOSE] included, so that each
        // replies with the id of what it made.
        return true;
    }
    if (name == "OPEN") {
        // OPEN directory opens a project; OPEN with no argument, SELECTION
        // or #ids opens polylines.
        return std::all_of(args.begin(), args.end(), [](const std::string& token) {
            return upperOf(token) == "SELECTION" || (!token.empty() && token.front() == '#');
        });
    }
    return false;
}

CommandInterpreter::Reply CommandInterpreter::drawingVerb(const std::string& verb,
                                                          const Tokens& args)
{
    const std::string name = canonical(verb);
    if (name == "VERTEX") {
        return vertexVerb(args);
    }
    if (name == "WEED" || name == "DENSIFY" || name == "STRAIGHTEN" || name == "CLOSE" ||
        name == "OPEN" || name == "STARTVERTEX" || name == "VERTEXZ") {
        return polylineVerb(name, args);
    }
    if (name == "ORTHO" || name == "POLAR" || name == "TRACKING" || name == "ANGLES" ||
        name == "LOCK" || name == "SNAP" || name == "DRAFTING") {
        return draftingVerb(name, args);
    }
    return drawShapeVerb(name, args);
}

std::string CommandInterpreter::drawingHelpText()
{
    return R"(Drawing (docs/drawing.md): vertex indices count from 0; options are key=value;
          replies are key=value records.  target = id (12 or #12) | SELECTION.
          Points x,y[,z] | @dx,dy[,dz] | @dist<dir (dir in the ANGLES convention, or a
          quadrant bearing N45d30'E); a z is the vertex's height.  Each edit is one undo step.
Vertex    VERTEX LIST id   a summary record, then one per vertex: index x y z bulge
          bearing (whole-circle degrees) distance (the segment's chord)
          VERTEX INSERT id p|#id@x,y[,z]|#id.sN [after=N]   #id@x,y: on the line nearest x,y
          VERTEX DELETE id N [N...] | MOVE id N p (@ is from the vertex, dz changes its height)
          VERTEX SET id N [x= y= z=|none bulge= bearing= distance=]   as the Vertices panel
Polyline  WEED target tolerance= [keep=on|off]   (keep: survey-point vertices stay)
          DENSIFY target interval= [chord=]  | CLOSE target | OPEN [target]
          STRAIGHTEN id N N [side=short|other] | VERTEXZ id GRADE N N [side=short|other]
          (a closed polyline's side with fewer vertices between, as the window's tools)
          STARTVERTEX id N | VERTEXZ id N z|none | VERTEXZ target INTERPOLATE
          (OPEN takes #ids or SELECTION; OPEN directory opens a project)
Draw      PLINE p p [ARC p...] [LINE p...] [CLOSE]   ARC: tangent arcs; heights as x,y,z
          PLINE3D p p [p...] [CLOSE] [z=]   z= is the height of points given without one
          SPLINE p p [p...] [control=on|off] [degree=3] | DLINE p p [p...] width= [CLOSE]
          ELLIPSE centre axis-end minor=|ratio= [start=deg end=deg]   (eccentric anomaly)
          XLINE p p [p...] | XLINE p angle=dir [angle=...] | RAY p p [p...]   construction
          layer: drawn and snapped to, never plotted
Drafting  ORTHO [on|off] | POLAR [on|off] [increment=deg] | TRACKING [on|off]
          ANGLES [ccw|bearing] | LOCK [angle=dir|none] [length=d|none] | LOCK OFF
          SNAP [on|off] [modes=a,b|all|none] [add=a,b] [remove=a,b] | DRAFTING
          each replies with the whole drafting record; not undo steps
)";
}

// ---- helpers shared by the verbs ----------------------------------------------------------

namespace {

// The polylines a target names: ids, or SELECTION (every polyline selected).
Result<std::vector<EntityId>> targetsOf(const Document& document,
                                        const std::vector<std::string>& tokens, bool emptyIsSelection)
{
    std::vector<EntityId> ids;
    const auto& model = document.model();
    const bool selection =
        (tokens.empty() && emptyIsSelection) ||
        (tokens.size() == 1 && upperOf(tokens.front()) == "SELECTION");
    if (selection) {
        for (const EntityId id : document.selection().ids()) {
            const Entity* entity = model.entities.find(id);
            if (entity != nullptr && isPolylineEntity(*entity)) {
                ids.push_back(id);
            }
        }
        if (ids.empty()) {
            return makeError(ErrorCode::InvalidState, "no polyline is selected");
        }
        return ids;
    }
    if (tokens.empty()) {
        return makeError(ErrorCode::InvalidArgument, "name the polyline: an id or SELECTION");
    }
    for (const std::string& token : tokens) {
        auto id = entityIdOf(token);
        if (!id) {
            return id.error();
        }
        const Entity* entity = model.entities.find(*id);
        if (entity == nullptr) {
            return makeError(ErrorCode::NotFound, "entity does not exist", token);
        }
        if (!isPolylineEntity(*entity)) {
            return makeError(ErrorCode::InvalidArgument,
                             "entity " + std::to_string(*id) + " is a " + kindText(*entity) +
                                 ", not a polyline");
        }
        ids.push_back(*id);
    }
    return ids;
}

Result<CurvePolyline2> polylineOf(const Document& document, EntityId id)
{
    const Entity* entity = document.model().entities.find(id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist", std::to_string(id));
    }
    auto polyline = readPolyline(*entity);
    if (!polyline) {
        return makeError(ErrorCode::InvalidArgument, "entity " + std::to_string(id) + " is a " +
                                                         kindText(*entity) + ", not a polyline");
    }
    return *polyline;
}

Status checkIndex(const CurvePolyline2& polyline, std::size_t index)
{
    if (index >= polyline.vertices.size()) {
        return makeError(ErrorCode::InvalidArgument,
                         "vertex " + std::to_string(index) + " does not exist; the polyline has " +
                             std::to_string(polyline.vertices.size()) + " (0 to " +
                             std::to_string(polyline.vertices.size() - 1) + ")");
    }
    return {};
}

} // namespace

Result<PrecisePoint> CommandInterpreter::parseDrawingPoint(const std::string& text,
                                                           std::optional<Point2> from)
{
    auto point = parsePrecisePoint(text, from ? from : lastPoint_, document_.drafting());
    if (!point) {
        return point.error();
    }
    lastPoint_ = point->point;
    return point;
}

// ---- VERTEX ---------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::vertexVerb(const Tokens& args)
{
    if (args.size() < 2) {
        return usageError("VERTEX LIST|INSERT|DELETE|MOVE|SET id ...");
    }
    const std::string sub = upperOf(args[0]);
    if (sub != "LIST" && sub != "INSERT" && sub != "DELETE" && sub != "MOVE" && sub != "SET") {
        return makeError(ErrorCode::ParseFailure,
                         "unknown VERTEX action; LIST INSERT DELETE MOVE SET", args[0]);
    }
    auto id = entityIdOf(args[1]);
    if (!id) {
        return id.error();
    }
    auto polyline = polylineOf(document_, *id);
    if (!polyline) {
        return polyline.error();
    }

    if (sub == "LIST") {
        if (args.size() != 2) {
            return usageError("VERTEX LIST id");
        }
        std::string text = polylineRecord(document_, *id);
        for (const VertexRow& row : vertexRows(*polyline)) {
            text += "\n" + vertexRecord(row);
        }
        return text;
    }

    if (sub == "INSERT") {
        auto parsed = splitArguments(args, 2, {"after"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.size() != 1) {
            return usageError("VERTEX INSERT id x,y[,z] | #id@x,y[,z] | #id.sN [after=N]");
        }
        // A point, or a point OF an entity (parseAnchoredPoint): "#12@x,y" is
        // the place on #12 nearest x,y - ON the line, as the window's Insert
        // Vertex puts a click - and "#12.s3" segment 3's middle. On this
        // polyline the anchor names its segment itself, where nearestSegment
        // could take a neighbour at a vertex.
        const std::string& given = parsed->positional[0];
        Point2 where;
        std::optional<double> z;
        std::optional<std::size_t> anchoredSegment;
        if (!given.empty() && given.front() == '#') {
            // "#12@x,y,z": on the line, with z its height, as x,y,z typed at
            // the window's Insert Vertex is. The anchor reads x,y alone.
            std::string anchorText = given;
            if (const std::size_t at = given.find('@'); at != std::string::npos) {
                const std::string_view near = std::string_view(given).substr(at + 1);
                if (std::ranges::count(near, ',') == 2) {
                    const std::size_t comma = near.rfind(',');
                    auto height = numberOf(near.substr(comma + 1), "the height");
                    if (!height) {
                        return height.error();
                    }
                    z = *height;
                    anchorText = given.substr(0, at + 1 + comma);
                }
            }
            auto anchored = parseAnchoredPoint(anchorText);
            if (!anchored) {
                return anchored.error();
            }
            where = anchored->point;
            if (anchored->ref.entity == *id &&
                (anchored->ref.point == katana::entity::AnchorPoint::Along ||
                 anchored->ref.point == katana::entity::AnchorPoint::SegmentMid)) {
                anchoredSegment = anchored->ref.index;
            }
        } else {
            auto at = parseDrawingPoint(given);
            if (!at) {
                return at.error();
            }
            where = at->point;
            z = at->z;
        }
        std::size_t segment = 0;
        if (const std::string* after = parsed->find("after")) {
            auto index = indexOf(*after, "after");
            if (!index) {
                return index.error();
            }
            const std::size_t segments =
                polyline->vertices.size() - (polyline->closed ? 0 : 1);
            if (*index >= segments) {
                return makeError(ErrorCode::InvalidArgument,
                                 "there is no segment after vertex " + std::to_string(*index));
            }
            segment = *index;
        } else if (anchoredSegment) {
            segment = *anchoredSegment;
        } else if (const auto nearest = geo::nearestSegment(*polyline, where)) {
            segment = *nearest;
        }
        auto status = document_.execute(editPolyline(
            *id, "VERTEX_INSERT", [segment, where, z](const CurvePolyline2& p) {
                return geo::insertVertex(p, segment, where, z);
            }));
        if (!status) {
            return status.error();
        }
        return polylineRecord(document_, *id) + " inserted=" + std::to_string(segment + 1);
    }

    if (sub == "DELETE") {
        if (args.size() < 3) {
            return usageError("VERTEX DELETE id N [N...]");
        }
        std::vector<std::size_t> indices;
        for (std::size_t i = 2; i < args.size(); ++i) {
            auto index = indexOf(args[i], "DELETE");
            if (!index) {
                return index.error();
            }
            if (auto ok = checkIndex(*polyline, *index); !ok) {
                return ok.error();
            }
            indices.push_back(*index);
        }
        const std::size_t count = indices.size();
        auto status = document_.execute(editPolyline(
            *id, "VERTEX_DELETE", [indices = std::move(indices)](const CurvePolyline2& p) {
                return geo::deleteVertices(p, indices);
            }));
        if (!status) {
            return status.error();
        }
        return polylineRecord(document_, *id) + " deleted=" + std::to_string(count);
    }

    if (sub == "MOVE") {
        if (args.size() != 4) {
            return usageError("VERTEX MOVE id N x,y[,z] | @dx,dy[,dz] | @dist<dir");
        }
        auto index = indexOf(args[2], "MOVE");
        if (!index) {
            return index.error();
        }
        if (auto ok = checkIndex(*polyline, *index); !ok) {
            return ok.error();
        }
        auto to = parseDrawingPoint(args[3], polyline->vertices[*index].position);
        if (!to) {
            return to.error();
        }
        const std::size_t at = *index;
        const PrecisePoint target = *to;
        const std::optional<double> base = polyline->vertices[at].height;
        const bool relative = !args[3].empty() && args[3].front() == '@';
        auto status = document_.execute(editPolyline(
            *id, "VERTEX_MOVE", [at, target, base, relative](const CurvePolyline2& p) {
                auto moved = geo::moveVertex(p, at, target.point);
                if (!moved || !target.z) {
                    return moved;
                }
                // A relative dz is a change of height; an absolute z is the height.
                if (relative && !base) {
                    return geo::PolylineResult(makeError(
                        ErrorCode::InvalidArgument,
                        "the vertex has no height for a dz to change; give z absolutely"));
                }
                return geo::setVertexHeight(*moved, at, relative ? *base + *target.z : *target.z);
            }));
        if (!status) {
            return status.error();
        }
        const auto after = polylineOf(document_, *id);
        return polylineRecord(document_, *id) + "\n" +
               vertexRecord(vertexRows(*after)[at]);
    }

    if (sub == "SET") {
        auto parsed = splitArguments(
            args, 2, {"x", "y", "e", "n", "easting", "northing", "z", "height", "bulge", "bearing",
                      "distance"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.size() != 1 || parsed->options.empty()) {
            return usageError("VERTEX SET id N [x= y= z= bulge= bearing= distance=]");
        }
        auto index = indexOf(parsed->positional[0], "SET");
        if (!index) {
            return index.error();
        }
        if (auto ok = checkIndex(*polyline, *index); !ok) {
            return ok.error();
        }
        std::vector<std::pair<VertexColumn, std::string>> cells;
        for (const auto& [key, value] : parsed->options) {
            const auto column = vertexColumnFromString(key == "z" ? std::string("height") : key);
            if (!column) {
                return makeError(ErrorCode::InvalidArgument, "unknown column '" + key + "'");
            }
            cells.emplace_back(*column, value);
        }
        const std::size_t at = *index;
        auto status = document_.execute(editPolyline(
            *id, "VERTEX_SET", [at, cells = std::move(cells)](const CurvePolyline2& p) {
                geo::PolylineResult current = p;
                for (const auto& [column, text] : cells) {
                    current = editVertexCell(*current, at, column, text, AngleConvention::Bearing);
                    if (!current) {
                        return current;
                    }
                }
                return current;
            }));
        if (!status) {
            return status.error();
        }
        const auto after = polylineOf(document_, *id);
        return polylineRecord(document_, *id) + "\n" + vertexRecord(vertexRows(*after)[at]);
    }
    return makeError(ErrorCode::Internal, "unreachable VERTEX action", args[0]);
}

// ---- whole-polyline edits -----------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::polylineVerb(const std::string& verb,
                                                           const Tokens& args)
{
    // Replies with one record per polyline edited.
    const auto records = [&](const std::vector<EntityId>& ids, const std::string& extra) {
        std::string text;
        for (const EntityId id : ids) {
            text += (text.empty() ? "" : "\n") + polylineRecord(document_, id) + extra;
        }
        return text;
    };
    const auto run = [&](const std::vector<EntityId>& ids, cmd::CommandPtr command,
                         const std::string& extra = {}) -> Reply {
        if (auto status = document_.execute(std::move(command)); !status) {
            return status.error();
        }
        return records(ids, extra);
    };

    if (verb == "WEED") {
        auto parsed = splitArguments(args, 0, {"tolerance", "keep"});
        if (!parsed) {
            return parsed.error();
        }
        const std::string* tolerance = parsed->find("tolerance");
        if (tolerance == nullptr) {
            return usageError("WEED target tolerance= [keep=on|off]");
        }
        auto value = positiveOf(*tolerance, "tolerance");
        if (!value) {
            return value.error();
        }
        bool keep = true;
        if (const std::string* k = parsed->find("keep")) {
            auto on = switchOf(*k, "keep");
            if (!on) {
                return on.error();
            }
            keep = *on;
        }
        auto ids = targetsOf(document_, parsed->positional, false);
        if (!ids) {
            return ids.error();
        }
        std::map<EntityId, std::size_t> before;
        for (const EntityId id : *ids) {
            before[id] = polylineOf(document_, id)->vertices.size();
        }
        const double tol = *value;
        auto status = document_.execute(editEachPolyline(
            *ids, "WEED",
            [tol, keep](const katana::entity::Model& model, EntityId, const CurvePolyline2& p) {
                return geo::weed(p, tol,
                                 keep ? verticesOnSurveyPoints(model, p) : std::vector<bool>{});
            }));
        if (!status) {
            return status.error();
        }
        std::string text;
        for (const EntityId id : *ids) {
            text += (text.empty() ? "" : "\n") + polylineRecord(document_, id) +
                    " before=" + std::to_string(before[id]);
        }
        return text;
    }

    if (verb == "DENSIFY") {
        auto parsed = splitArguments(args, 0, {"interval", "chord"});
        if (!parsed) {
            return parsed.error();
        }
        const std::string* interval = parsed->find("interval");
        const std::string* chord = parsed->find("chord");
        if (interval == nullptr && chord == nullptr) {
            return usageError("DENSIFY target interval= [chord=]");
        }
        double step = 0.0;
        double chordTolerance = 0.0;
        if (interval != nullptr) {
            auto v = numberOf(*interval, "interval");
            if (!v) {
                return v.error();
            }
            step = *v;
        }
        if (chord != nullptr) {
            auto v = numberOf(*chord, "chord");
            if (!v) {
                return v.error();
            }
            chordTolerance = *v;
        }
        if (step < 0.0 || chordTolerance < 0.0 || (step == 0.0 && chordTolerance == 0.0)) {
            return makeError(ErrorCode::InvalidArgument,
                             "give an interval or a chord tolerance greater than zero");
        }
        auto ids = targetsOf(document_, parsed->positional, false);
        if (!ids) {
            return ids.error();
        }
        return run(*ids, editPolylines(*ids, "DENSIFY", [step, chordTolerance](const CurvePolyline2& p) {
                       return geo::densify(p, step, chordTolerance);
                   }));
    }

    if (verb == "CLOSE" || verb == "OPEN") {
        auto ids = targetsOf(document_, args, true);
        if (!ids) {
            return ids.error();
        }
        const bool close = verb == "CLOSE";
        return run(*ids, editPolylines(*ids, close ? "CLOSE" : "OPEN", [close](const CurvePolyline2& p) {
                       if (p.closed == close) {
                           return geo::PolylineResult(p);
                       }
                       return close ? geo::closePolyline(p) : geo::openPolyline(p);
                   }));
    }

    // The verbs on one polyline, with vertex indices.
    if (args.empty()) {
        return usageError(verb == "STRAIGHTEN"    ? "STRAIGHTEN id N N"
                          : verb == "STARTVERTEX" ? "STARTVERTEX id N"
                                                  : "VERTEXZ id N z|none | target INTERPOLATE | "
                                                    "id GRADE N N");
    }
    if (verb == "VERTEXZ" && args.size() >= 2 && upperOf(args.back()) == "INTERPOLATE") {
        const Tokens targets(args.begin(), args.end() - 1);
        auto ids = targetsOf(document_, targets, false);
        if (!ids) {
            return ids.error();
        }
        return run(*ids, editPolylines(*ids, "INTERPOLATE_Z", [](const CurvePolyline2& p) {
                       return geo::interpolateHeights(p);
                   }));
    }
    auto id = entityIdOf(args[0]);
    if (!id) {
        return id.error();
    }
    auto polyline = polylineOf(document_, *id);
    if (!polyline) {
        return polyline.error();
    }
    const auto indexIn = [&](const std::string& text) -> Result<std::size_t> {
        auto index = indexOf(text, verb.c_str());
        if (!index) {
            return index.error();
        }
        if (auto ok = checkIndex(*polyline, *index); !ok) {
            return ok.error();
        }
        return *index;
    };
    const auto indexAt = [&](std::size_t position) { return indexIn(args[position]); };
    const std::vector<EntityId> one{*id};

    if (verb == "STRAIGHTEN" ||
        (verb == "VERTEXZ" && args.size() >= 4 && upperOf(args[1]) == "GRADE")) {
        const bool grade = verb == "VERTEXZ";
        auto parsed = splitArguments(args, grade ? 2 : 1, {"side"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.size() != 2) {
            return usageError(grade ? "VERTEXZ id GRADE N N [side=short|other]"
                                    : "STRAIGHTEN id N N [side=short|other]");
        }
        auto from = indexIn(parsed->positional[0]);
        auto to = indexIn(parsed->positional[1]);
        if (!from) {
            return from.error();
        }
        if (!to) {
            return to.error();
        }
        // A closed polyline's shorter side between the two, as the window's
        // Straighten and Grade take it, or with side=other the other way
        // round, as their O does (cad::vertexRange, the one rule).
        bool otherSide = false;
        if (const std::string* side = parsed->find("side")) {
            const std::string word = lowerOf(*side);
            if (word != "short" && word != "other") {
                return makeError(ErrorCode::InvalidArgument, "side is short or other", *side);
            }
            if (word == "other" && !polyline->closed) {
                return makeError(ErrorCode::InvalidArgument,
                                 "side=other goes the other way round a closed polyline, and "
                                 "polyline " + std::to_string(*id) + " is open");
            }
            otherSide = word == "other";
        }
        const std::size_t a = *from;
        const std::size_t b = *to;
        return run(one, editPolyline(*id, grade ? "GRADE" : "STRAIGHTEN",
                                     [a, b, grade, otherSide](const CurvePolyline2& p) {
                                         const auto [walkFrom, walkTo] =
                                             vertexRange(p, a, b, otherSide);
                                         return grade ? geo::gradeBetween(p, walkFrom, walkTo)
                                                      : geo::straighten(p, walkFrom, walkTo);
                                     }));
    }
    if (verb == "STARTVERTEX") {
        if (args.size() != 2) {
            return usageError("STARTVERTEX id N");
        }
        auto index = indexAt(1);
        if (!index) {
            return index.error();
        }
        const std::size_t at = *index;
        return run(one, editPolyline(*id, "START_VERTEX", [at](const CurvePolyline2& p) {
                       return geo::changeStartVertex(p, at);
                   }));
    }
    // VERTEXZ id N z|none
    if (args.size() != 3) {
        return usageError("VERTEXZ id N z|none | target INTERPOLATE | id GRADE N N");
    }
    auto index = indexAt(1);
    if (!index) {
        return index.error();
    }
    std::optional<double> height;
    if (lowerOf(args[2]) != "none" && args[2] != "-") {
        auto value = numberOf(args[2], "the height");
        if (!value) {
            return value.error();
        }
        height = *value;
    }
    const std::size_t at = *index;
    return run(one, editPolyline(*id, "VERTEX_Z", [at, height](const CurvePolyline2& p) {
                   return geo::setVertexHeight(p, at, height);
               }));
}

// ---- the draw verbs ---------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::drawShapeVerb(const std::string& verb,
                                                            const Tokens& args)
{
    const cmd::EntityAttributes attributes = document_.currentAttributes();
    // Runs `command` and replies with a record for each entity it made.
    const auto made = [&](cmd::CommandPtr command) -> Reply {
        if (auto status = document_.execute(std::move(command)); !status) {
            return status.error();
        }
        std::string text;
        for (const EntityId id : document_.lastCreatedEntities()) {
            const Entity* entity = document_.model().entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            text += text.empty() ? "" : "\n";
            text += isPolylineEntity(*entity)
                        ? polylineRecord(document_, id)
                        : "id=" + std::to_string(id) + " kind=" + kindText(*entity) +
                              " layer=" + entity->layer;
        }
        return text;
    };

    if (verb == "PLINE" || verb == "PLINE3D") {
        const bool threeD = verb == "PLINE3D";
        auto parsed = threeD ? splitArguments(args, 0, {"z"}) : splitArguments(args, 0, {});
        if (!parsed) {
            return parsed.error();
        }
        std::optional<double> defaultZ;
        if (const std::string* z = parsed->find("z")) {
            auto value = numberOf(*z, "z");
            if (!value) {
                return value.error();
            }
            defaultZ = *value;
        }
        CurvePolyline2 path;
        bool arc = false;
        bool closed = false;
        const auto& tokens = parsed->positional;
        for (std::size_t i = 0; i < tokens.size(); ++i) {
            const std::string word = upperOf(tokens[i]);
            if (!threeD && (word == "ARC" || word == "A")) {
                arc = true;
                continue;
            }
            if (!threeD && (word == "LINE" || word == "L")) {
                arc = false;
                continue;
            }
            if ((word == "CLOSE" || word == "C") && i + 1 == tokens.size()) {
                closed = true;
                continue;
            }
            auto point = parseDrawingPoint(tokens[i]);
            if (!point) {
                return point.error();
            }
            if (!path.vertices.empty() &&
                point->point.distanceTo(path.vertices.back().position) <=
                    katana::math::tolerance::kGeometric) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "two points in a row are the same point", tokens[i]);
            }
            if (arc && !path.vertices.empty()) {
                path.vertices.back().bulge = tangentBulge(path, point->point);
            }
            path.vertices.push_back(CurveVertex{point->point, 0.0, point->z ? point->z : defaultZ});
        }
        if (path.vertices.size() < 2 || (closed && path.vertices.size() < 3)) {
            return usageError(threeD ? "PLINE3D p p [p...] [CLOSE] [z=]"
                                     : "PLINE p p [ARC p...] [LINE p...] [CLOSE]");
        }
        if (closed) {
            path.closed = true;
            // In arc mode the closing segment is a tangent arc too, as the
            // Polyline tool closes.
            path.vertices.back().bulge = arc ? tangentBulge(path, path.vertices.front().position) : 0.0;
        }
        auto entity = writePolyline(drawnEntity(katana::entity::Geometry{}, attributes), path);
        if (!entity) {
            return entity.error();
        }
        return made(createDrawn("CREATE_POLYLINE", {std::move(*entity)}));
    }

    if (verb == "SPLINE") {
        auto parsed = splitArguments(args, 0, {"control", "degree"});
        if (!parsed) {
            return parsed.error();
        }
        bool control = false;
        int degree = 3;
        if (const std::string* c = parsed->find("control")) {
            auto on = switchOf(*c, "control");
            if (!on) {
                return on.error();
            }
            control = *on;
        }
        if (const std::string* d = parsed->find("degree")) {
            const auto value = katana::core::parseInteger(*d);
            if (!value || *value < 1 || *value > 10) {
                return makeError(ErrorCode::InvalidArgument, "degree must be from 1 to 10", *d);
            }
            degree = static_cast<int>(*value);
        }
        std::vector<Point2> points;
        for (const std::string& token : parsed->positional) {
            auto point = parseDrawingPoint(token);
            if (!point) {
                return point.error();
            }
            points.push_back(point->point);
        }
        if (points.size() < 2) {
            return usageError("SPLINE p p [p...] [control=on|off] [degree=3]");
        }
        auto spline = control ? geo::Spline2::fromControlPoints(std::move(points), degree)
                              : geo::Spline2::throughPoints(std::move(points), degree);
        if (!spline) {
            return spline.error();
        }
        return made(createDrawn("CREATE_SPLINE", {drawnEntity(std::move(*spline), attributes)}));
    }

    if (verb == "ELLIPSE") {
        auto parsed = splitArguments(args, 0, {"minor", "ratio", "start", "end"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.size() != 2) {
            return usageError("ELLIPSE centre axis-end minor=|ratio= [start=deg end=deg]");
        }
        auto centre = parseDrawingPoint(parsed->positional[0]);
        if (!centre) {
            return centre.error();
        }
        auto axisEnd = parseDrawingPoint(parsed->positional[1]);
        if (!axisEnd) {
            return axisEnd.error();
        }
        const double major = centre->point.distanceTo(axisEnd->point);
        double other = 0.0;
        if (const std::string* minor = parsed->find("minor")) {
            auto v = positiveOf(*minor, "minor");
            if (!v) {
                return v.error();
            }
            other = *v;
        } else if (const std::string* ratio = parsed->find("ratio")) {
            auto v = positiveOf(*ratio, "ratio");
            if (!v) {
                return v.error();
            }
            other = *v * major;
        } else {
            return usageError("ELLIPSE centre axis-end minor=|ratio= [start=deg end=deg]");
        }
        auto shape = geo::Ellipse2::fromAxes(centre->point, axisEnd->point, other);
        if (!shape) {
            return makeError(ErrorCode::InvalidGeometry, "the ellipse's axes need lengths");
        }
        const std::string* start = parsed->find("start");
        const std::string* end = parsed->find("end");
        if ((start == nullptr) != (end == nullptr)) {
            return makeError(ErrorCode::InvalidArgument, "an elliptical arc needs start= and end=");
        }
        if (start != nullptr) {
            auto a = numberOf(*start, "start");
            auto b = numberOf(*end, "end");
            if (!a) {
                return a.error();
            }
            if (!b) {
                return b.error();
            }
            const double from = *a * katana::math::kDegToRad;
            const double sweep = katana::math::normalizeAngle(*b * katana::math::kDegToRad - from);
            if (!(sweep > katana::math::tolerance::kAngular)) {
                return makeError(ErrorCode::InvalidGeometry, "the arc's end is its start");
            }
            shape->startParameter = from;
            shape->sweep = sweep;
        }
        return made(createDrawn("CREATE_ELLIPSE", {drawnEntity(*shape, attributes)}));
    }

    if (verb == "XLINE" || verb == "RAY") {
        const bool ray = verb == "RAY";
        auto parsed = ray ? splitArguments(args, 0, {}) : splitArguments(args, 0, {"angle"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.empty()) {
            return usageError(ray ? "RAY p p [p...]" : "XLINE p p [p...] | XLINE p angle=dir");
        }
        auto base = parseDrawingPoint(parsed->positional[0]);
        if (!base) {
            return base.error();
        }
        std::vector<geo::Segment2> lines;
        for (const auto& [key, value] : parsed->options) {
            auto direction = parseDirection(value, document_.drafting().angles);
            if (!direction) {
                return direction.error();
            }
            lines.push_back(constructionSegment(
                base->point, Vec2(std::cos(*direction), std::sin(*direction)), ray));
        }
        for (std::size_t i = 1; i < parsed->positional.size(); ++i) {
            auto through = parseDrawingPoint(parsed->positional[i], base->point);
            if (!through) {
                return through.error();
            }
            const Vec2 d = through->point - base->point;
            if (d.length() <= katana::math::tolerance::kGeometric) {
                return makeError(ErrorCode::InvalidGeometry, "a through point is the base point",
                                 parsed->positional[i]);
            }
            lines.push_back(constructionSegment(base->point, d.normalized(), ray));
        }
        if (lines.empty()) {
            return usageError(ray ? "RAY p p [p...]" : "XLINE p p [p...] | XLINE p angle=dir");
        }
        return made(createConstruction(document_.model(), attributes, lines, ray));
    }

    // DLINE
    auto parsed = splitArguments(args, 0, {"width"});
    if (!parsed) {
        return parsed.error();
    }
    const std::string* width = parsed->find("width");
    if (width == nullptr) {
        return usageError("DLINE p p [p...] width= [CLOSE]");
    }
    auto w = positiveOf(*width, "width");
    if (!w) {
        return w.error();
    }
    std::vector<Point2> points;
    bool closed = false;
    for (std::size_t i = 0; i < parsed->positional.size(); ++i) {
        const std::string word = upperOf(parsed->positional[i]);
        if ((word == "CLOSE" || word == "C") && i + 1 == parsed->positional.size()) {
            closed = true;
            continue;
        }
        auto point = parseDrawingPoint(parsed->positional[i]);
        if (!point) {
            return point.error();
        }
        points.push_back(point->point);
    }
    if (points.size() < 2 || (closed && points.size() < 3)) {
        return usageError("DLINE p p [p...] width= [CLOSE]");
    }
    const auto sides = doubleLineSides(geo::Polyline2{points, closed}, *w);
    if (sides.size() != 2) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the path is too tight for a double line of this width");
    }
    std::vector<Entity> entities;
    for (const auto& side : sides) {
        entities.push_back(drawnEntity(side, attributes));
    }
    return made(createDrawn("CREATE_DOUBLE_LINE", std::move(entities)));
}

// ---- drafting settings ----------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::draftingVerb(const std::string& verb,
                                                           const Tokens& args)
{
    DraftingSettings settings = document_.drafting();
    const auto flag = [&](bool& value, const Tokens& tokens) -> Status {
        if (tokens.size() > 1) {
            return usageError(verb + " [on|off]");
        }
        if (tokens.size() == 1) {
            auto on = switchOf(tokens[0], verb.c_str());
            if (!on) {
                return on.error();
            }
            value = *on;
        }
        return {};
    };

    Status status;
    if (verb == "ORTHO" || verb == "TRACKING") {
        status = flag(verb == "ORTHO" ? settings.ortho : settings.objectTracking, args);
    } else if (verb == "POLAR") {
        auto parsed = splitArguments(args, 0, {"increment"});
        if (!parsed) {
            return parsed.error();
        }
        status = flag(settings.polar, parsed->positional);
        if (status) {
            if (const std::string* increment = parsed->find("increment")) {
                auto v = positiveOf(*increment, "increment");
                if (!v) {
                    return v.error();
                }
                if (*v > 180.0) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "the polar increment is at most 180 degrees", *increment);
                }
                settings.polarIncrement = *v * katana::math::kDegToRad;
            }
        }
    } else if (verb == "ANGLES") {
        if (args.size() > 1) {
            return usageError("ANGLES [ccw|bearing]");
        }
        if (args.size() == 1) {
            const std::string value = lowerOf(args[0]);
            if (value == "bearing" || value == "bearings") {
                settings.angles = AngleConvention::Bearing;
            } else if (value == "ccw" || value == "degrees" || value == "counterclockwise") {
                settings.angles = AngleConvention::Counterclockwise;
            } else {
                return makeError(ErrorCode::ParseFailure, "expected ccw or bearing", args[0]);
            }
        }
    } else if (verb == "LOCK") {
        if (args.size() == 1 && upperOf(args[0]) == "OFF") {
            settings.angleLock.reset();
            settings.lengthLock.reset();
        } else {
            auto parsed = splitArguments(args, 0, {"angle", "length"});
            if (!parsed) {
                return parsed.error();
            }
            if (!parsed->positional.empty()) {
                return usageError("LOCK [angle=dir|none] [length=d|none] | LOCK OFF");
            }
            if (const std::string* angle = parsed->find("angle")) {
                if (lowerOf(*angle) == "none") {
                    settings.angleLock.reset();
                } else {
                    auto direction = parseDirection(*angle, settings.angles);
                    if (!direction) {
                        return direction.error();
                    }
                    settings.angleLock = *direction;
                }
            }
            if (const std::string* length = parsed->find("length")) {
                if (lowerOf(*length) == "none") {
                    settings.lengthLock.reset();
                } else {
                    auto v = positiveOf(*length, "length");
                    if (!v) {
                        return v.error();
                    }
                    settings.lengthLock = *v;
                }
            }
        }
    } else if (verb == "SNAP") {
        auto parsed = splitArguments(args, 0, {"modes", "add", "remove"});
        if (!parsed) {
            return parsed.error();
        }
        status = flag(settings.snapEnabled, parsed->positional);
        if (status) {
            for (const auto& [key, value] : parsed->options) {
                auto modes = modesOf(value);
                if (!modes) {
                    return modes.error();
                }
                if (key == "modes") {
                    settings.snapModes = *modes;
                } else if (key == "add") {
                    settings.snapModes |= *modes;
                } else {
                    settings.snapModes &= ~*modes;
                }
            }
        }
    } else if (!args.empty()) { // DRAFTING
        return usageError("DRAFTING");
    }
    if (!status) {
        return status.error();
    }
    if (!(settings == document_.drafting())) {
        document_.drafting() = settings;
        document_.notifyDraftingChanged();
    }
    return draftingRecord(document_.drafting());
}

} // namespace katana::cad
