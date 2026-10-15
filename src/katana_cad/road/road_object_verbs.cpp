// ROADOBJECT: road objects on the text command line
// (include/katana/cad/road/road_object_verbs.hpp, docs/road_objects.md).

#include "katana/cad/road/road_object_verbs.hpp"

#include <algorithm>
#include <cmath>
#include <format>
#include <iterator>
#include <map>
#include <optional>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/cad/road/road_object_mesh.hpp"
#include "katana/cad/road/road_object_scope.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/geometry/curves2d.hpp"

namespace katana::cad::road {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using Words = std::vector<std::string>;

namespace {

constexpr std::string_view kUsage =
    "ROADOBJECT KINDS [kind] | LIST [JSON] [scope] | INFO id | ADD kind <placement> [key=value "
    "...] [PREVIEW] | SET id key=value ... [PREVIEW] | COPY id [station=] [dx=] [dy=] "
    "[key=value ...] [PREVIEW] | DELETE id ...|ALL|scope [PREVIEW] | QUANTITIES [id ...|scope] | "
    "PROBLEMS   (HELP ROADOBJECT)";

Error usage(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

std::string upper(std::string_view text)
{
    return katana::core::uppered(text);
}

std::string lower(std::string_view text)
{
    return katana::core::lowered(text);
}

// A real in a reply: to a micrometre, then exactly, so a length that is
// 54.99999999999999 by its arithmetic reads 55 and a typed 0.787 reads 0.787.
std::string number(double value)
{
    const double rounded = std::round(value * 1e6) / 1e6;
    return katana::core::formatExactReal(rounded == 0.0 ? 0.0 : rounded);
}

std::string quantityKeys(const std::vector<bim::Quantity>& quantities)
{
    std::string out;
    for (const bim::Quantity& quantity : quantities) {
        out += " " + lower(quantity.name) + "=" + number(quantity.value);
    }
    return out;
}

std::string placementKeys(const RoadObject& object)
{
    const RoadObjectPlacement& placement = object.placement;
    std::string out = " placement=" + std::string(placementWord(placement.kind));
    if (placement.kind == PlacementKind::Alignment) {
        out += " alignment=" + recordValue(placement.alignment) +
               " station=" + number(placement.station);
        if (placement.to) {
            out += " to=" + number(*placement.to);
        }
        out += " offset=" + number(placement.offset);
    } else {
        out += " points=" + std::to_string(placement.vertices.size());
        if (placement.source != katana::entity::kInvalidEntityId) {
            out += " source=#" + std::to_string(placement.source);
        }
    }
    if (placement.spacing) {
        out += " spacing=" + number(*placement.spacing);
    }
    const HeightSource heights = heightSourceOf(placement);
    out += " heights=" + std::string(heightWord(heights));
    if (heights == HeightSource::Surface) {
        out += " surface=" + recordValue(placement.surface);
    }
    if (placement.crossfall != 0.0) {
        out += " crossfall=" + number(placement.crossfall);
    }
    if (placement.dz != 0.0) {
        out += " dz=" + number(placement.dz);
    }
    return out;
}

std::string objectRecord(std::string_view word, const RoadObject& object,
                         const bim::BimModel& built)
{
    std::string out = std::string(word) + " id=" + object.id +
                      " kind=" + std::string(kindWord(object.kind));
    if (!object.name.empty()) {
        out += " name=" + recordValue(object.name);
    }
    out += placementKeys(object);
    out += " layer=" + recordValue(layerOf(object));
    if (!object.facility.empty()) {
        out += " facility=" + recordValue(object.facility);
    }
    out += " objects=" + std::to_string(built.objects.size());
    out += quantityKeys(totalQuantities(built.objects));
    if (!built.problems.empty()) {
        out += " problem=" + recordValue(built.problems.front().message);
    }
    return out;
}

// key=value, split at the first '='; nullopt for a word without one.
std::optional<std::pair<std::string, std::string>> keyValue(const std::string& word)
{
    const std::size_t equals = word.find('=');
    if (equals == std::string::npos || equals == 0) {
        return std::nullopt;
    }
    return std::pair{word.substr(0, equals), word.substr(equals + 1)};
}

// "x,y" or "x,y,z": a point typed on the line.
std::optional<PlacedVertex> typedPoint(std::string_view word)
{
    if (word.find('=') != std::string_view::npos) {
        return std::nullopt;
    }
    std::vector<double> parts;
    std::size_t start = 0;
    while (start <= word.size()) {
        const std::size_t comma = word.find(',', start);
        const std::string_view part =
            word.substr(start, comma == std::string_view::npos ? word.size() - start
                                                               : comma - start);
        const auto value = katana::core::parseFiniteDouble(part);
        if (!value) {
            return std::nullopt;
        }
        parts.push_back(*value);
        if (comma == std::string_view::npos) {
            break;
        }
        start = comma + 1;
    }
    if (parts.size() != 2 && parts.size() != 3) {
        return std::nullopt;
    }
    PlacedVertex vertex;
    vertex.position = katana::geometry::Point2(parts[0], parts[1]);
    if (parts.size() == 3) {
        vertex.z = parts[2];
    }
    return vertex;
}

// "#12": an entity id.
std::optional<katana::entity::EntityId> typedId(std::string_view word)
{
    if (word.size() < 2 || word.front() != '#') {
        return std::nullopt;
    }
    const auto value = katana::core::parseInteger(word.substr(1));
    if (!value || *value < 1) {
        return std::nullopt;
    }
    return static_cast<katana::entity::EntityId>(*value);
}

// The vertices of a line the drawing holds, with its heights: a line's two
// ends, a polyline's vertices (a closed one's first again at the end, so a
// fence goes all the way round), a curve polyline's or an arc's chords.
// nullopt for anything that is not a line.
std::optional<std::vector<PlacedVertex>> lineOf(const katana::entity::Entity& entity)
{
    using katana::entity::heightsOf;
    std::vector<PlacedVertex> out;
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&entity.geometry)) {
        const auto heights = heightsOf(entity.properties, 2);
        out = {{segment->start, heights[0]}, {segment->end, heights[1]}};
    } else if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
        const auto heights = heightsOf(entity.properties, polyline->vertices.size());
        for (std::size_t i = 0; i < polyline->vertices.size(); ++i) {
            out.push_back({polyline->vertices[i], heights[i]});
        }
        if (polyline->closed && out.size() > 2) {
            out.push_back(out.front());
        }
    } else if (const auto* curve =
                   std::get_if<katana::geometry::CurvePolyline2>(&entity.geometry)) {
        for (const auto& step :
             curve->tessellateWithHeights(katana::geometry::kCurveChordTolerance)) {
            out.push_back({step.position, step.height});
        }
    } else if (const auto* arc = std::get_if<katana::geometry::Arc2>(&entity.geometry)) {
        const std::vector<katana::geometry::Point2> chords =
            katana::geometry::chordArc(*arc, katana::geometry::kCurveChordTolerance);
        // An arc keeps a height at each end - two in `elevations`, or the one
        // `elevation` both share - and one on a grade rises evenly by its
        // sweep, as section annotation reads it. The chords stand at equal
        // fractions of the sweep, so each takes that fraction of the rise.
        // An end with no height leaves every chord with none, and the
        // placement refuses it, naming surface=.
        const auto ends = heightsOf(entity.properties, 2);
        for (std::size_t i = 0; i < chords.size(); ++i) {
            std::optional<double> height;
            if (ends[0] && ends[1]) {
                const double fraction =
                    chords.size() > 1
                        ? static_cast<double>(i) / static_cast<double>(chords.size() - 1)
                        : 0.0;
                height = *ends[0] + (*ends[1] - *ends[0]) * fraction;
            }
            out.push_back({chords[i], height});
        }
    } else {
        return std::nullopt;
    }
    if (out.size() < 2) {
        return std::nullopt;
    }
    return out;
}

std::optional<PlacedVertex> pointOf(const katana::entity::Entity& entity)
{
    const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry);
    if (point == nullptr) {
        return std::nullopt;
    }
    return PlacedVertex{point->position, katana::entity::heightsOf(entity.properties, 1).front()};
}

std::string typeName(const katana::entity::Entity& entity)
{
    return lower(katana::entity::toString(entity.type()));
}

// The set as stored, refusing to work on objects that could not be read
// rather than overwrite them.
Result<RoadObjectSet> editableSet(const Document& document)
{
    if (const Status status = document.roadObjectsStatus(); !status) {
        return makeError(ErrorCode::CommandRejected,
                         "the drawing's road objects cannot be read, and are kept as they are",
                         status.error().describe());
    }
    return document.roadObjects();
}

// Made, or refused naming what stopped it: what every edit checks before it
// commits, so a definition the model would only list as a problem is never
// stored (checkBuilt, road_objects.hpp).
Status checkBuilds(const PlacementContext& context, const RoadObject& object)
{
    return checkBuilt(object, buildRoadObject(context, object));
}

// LIST, QUANTITIES and DELETE take the objects by id, or by the shared scope
// and filter (road_object_scope.hpp): when the word at `at` begins a scope it
// is read by the one parser from there, `at` is left at the first word after
// it, and the objects it takes are matched; nullopt when it does not, so the
// words are ids. A scope word is never ignored, and never read as an id that
// does not exist (docs/road_objects.md, "Scope and filter").
Result<std::optional<RoadObjectMatch>> readScope(const Document& document, const Words& args,
                                                 std::size_t& at, const ScopeViewProvider& views)
{
    if (at >= args.size() || !isScopeWord(args[at])) {
        return std::optional<RoadObjectMatch>{};
    }
    auto words = parseScopeWords(args, at);
    if (!words) {
        return words.error();
    }
    auto match = matchRoadObjects(document, *words, views);
    if (!match) {
        return match.error();
    }
    return std::optional<RoadObjectMatch>(std::move(*match));
}

// The words that ended a scope, which are the verb's own: `allowed` (PREVIEW
// for DELETE) and nothing else.
Status onlyAfterScope(std::string_view verb, const Words& args, std::size_t at,
                      std::string_view allowed)
{
    for (std::size_t i = at; i < args.size(); ++i) {
        if (allowed.empty() || upper(args[i]) != allowed) {
            return makeError(ErrorCode::ParseFailure,
                             std::format("not a word ROADOBJECT {} takes after a scope{}", verb,
                                         allowed.empty() ? std::string()
                                                         : std::format(": {} may end the line",
                                                                       allowed)),
                             args[i]);
        }
    }
    return {};
}

// An id among the words of a line that gave a scope, or a scope word among
// ids: one or the other.
Error idsOrScope(std::string_view verb, const std::string& word)
{
    return makeError(ErrorCode::InvalidArgument,
                     std::format("ROADOBJECT {} takes ids or one scope (DRAWING, SELECTION, VIEW, "
                                 "AREA or LAYERS, then WHERE ...), not both",
                                 verb),
                     word);
}

// ---- the subcommands ------------------------------------------------------------------------

Result<std::string> kinds(const Words& args)
{
    std::string out;
    const auto line = [&out](const std::string& text) {
        out += (out.empty() ? "" : "\n") + text;
    };
    const auto shapeWord = [](KindShape shape) {
        switch (shape) {
        case KindShape::Linear:
            return "linear";
        case KindShape::Point:
            return "point";
        case KindShape::Crossing:
            return "crossing";
        }
        return "linear";
    };
    if (args.size() > 2) {
        auto kind = kindFromWord(args[2]);
        if (!kind) {
            return kind.error();
        }
        line(std::format("kind word={} title={} shape={} source={}", kindWord(*kind),
                         recordValue(kindTitle(*kind)), shapeWord(shapeOf(*kind)),
                         recordValue(kindSource(*kind))));
        for (const ParameterSpec& spec : parametersOf(*kind)) {
            std::string record = "parameter key=" + std::string(spec.key);
            switch (spec.type) {
            case ParameterType::Number:
                record += " type=number";
                if (!spec.unit.empty()) {
                    record += " unit=" + std::string(spec.unit);
                }
                record += " default=" + recordValue(spec.defaultText) +
                          " min=" + number(spec.minimum) + " max=" + number(spec.maximum);
                if (spec.whole) {
                    record += " whole=yes";
                }
                break;
            case ParameterType::Choice: {
                std::string choices;
                for (const std::string_view choice : spec.choices) {
                    choices += (choices.empty() ? "" : ",") + std::string(choice);
                }
                record += " type=choice default=" + recordValue(spec.defaultText) +
                          " choices=" + choices;
                break;
            }
            case ParameterType::Text:
                record += " type=text";
                break;
            }
            record += " description=" + recordValue(spec.description);
            line(record);
        }
        return out;
    }
    for (const RoadObjectKind kind : roadObjectKinds()) {
        std::string keys;
        for (const ParameterSpec& spec : parametersOf(kind)) {
            keys += (keys.empty() ? "" : ",") + std::string(spec.key);
        }
        line(std::format("kind word={} title={} shape={} parameters={}", kindWord(kind),
                         recordValue(kindTitle(kind)), shapeWord(shapeOf(kind)), keys));
    }
    return out;
}

Result<std::string> list(const Document& document, const Words& args,
                         const ScopeViewProvider& views)
{
    std::size_t at = 2;
    const bool json = at < args.size() && upper(args[at]) == "JSON";
    if (json) {
        ++at;
    }
    auto scoped = readScope(document, args, at, views);
    if (!scoped) {
        return scoped.error();
    }
    if (at < args.size()) {
        return makeError(ErrorCode::InvalidArgument,
                         "usage: ROADOBJECT LIST [JSON] [<scope> [WHERE key=value ...]]",
                         args[at]);
    }
    if (json && *scoped) {
        // The stored set is one document, and a scope's record cannot lead it
        // without making it something else.
        return makeError(ErrorCode::InvalidArgument,
                         "ROADOBJECT LIST JSON is the whole stored set: LIST with a scope gives "
                         "the objects it takes as records, and the katana_road_objects tool "
                         "gives them as data",
                         args[2]);
    }
    if (const Status status = document.roadObjectsStatus(); !status) {
        return status.error();
    }
    if (json) {
        return roadObjectsToJson(document.roadObjects());
    }
    const PlacementContext context = placementContext(document);
    std::string body;
    std::size_t problems = 0;
    std::size_t count = 0;
    for (const RoadObject& object : document.roadObjects().objects) {
        if (*scoped && std::ranges::find((*scoped)->ids, object.id) == (*scoped)->ids.end()) {
            continue;
        }
        const bim::BimModel built = buildRoadObject(context, object);
        problems += built.problems.size();
        ++count;
        body += "\n" + objectRecord("object", object, built);
    }
    return (*scoped ? (*scoped)->record() + "\n" : std::string()) +
           std::format("roadobjects count={} problems={}", count, problems) + body;
}

Result<std::string> info(const Document& document, const Words& args)
{
    if (args.size() < 3) {
        return usage("ROADOBJECT INFO id");
    }
    // A set that cannot be read is empty to find(); say why, not that the
    // object is not there.
    if (const Status status = document.roadObjectsStatus(); !status) {
        return status.error();
    }
    const RoadObject* object = document.roadObjects().find(args[2]);
    if (object == nullptr) {
        return makeError(ErrorCode::NotFound, "no road object has that id", args[2]);
    }
    const PlacementContext context = placementContext(document);
    const bim::BimModel built = buildRoadObject(context, *object);
    std::string out = objectRecord("object", *object, built);
    if (object->kind == RoadObjectKind::Culvert) {
        if (const auto ends = culvertEnds(context, *object)) {
            out += std::format(" inlet={},{},{} outlet={},{},{}", number(ends->inlet.x),
                               number(ends->inlet.y), number(ends->inlet.z),
                               number(ends->outlet.x), number(ends->outlet.y),
                               number(ends->outlet.z));
        }
    }
    const Parameters effective = effectiveParameters(*object);
    for (const ParameterSpec& spec : parametersOf(object->kind)) {
        out += "\nparameter key=" + std::string(spec.key);
        const auto value = effective.find(spec.key);
        out += " value=" + (value == effective.end()
                                ? recordValue(spec.defaultText.empty() ? std::string_view("none")
                                                                       : spec.defaultText)
                                : recordValue(parameterText(value->second)));
        out += object->parameters.contains(spec.key) ? " given=yes" : " given=no";
        if (!spec.unit.empty()) {
            out += " unit=" + std::string(spec.unit);
        }
    }
    for (const bim::BimObject& made : built.objects) {
        out += std::format("\npart id={} class={} type={} material={} triangles={} closed={}",
                           recordValue(made.id), made.ifcClass, made.predefinedType,
                           recordValue(made.material), made.mesh.triangleCount(),
                           isClosedMesh(made.mesh) ? "yes" : "no");
        out += quantityKeys(made.quantities);
    }
    for (const bim::BimProblem& problem : built.problems) {
        out += "\nproblem message=" + recordValue(problem.message);
    }
    return out;
}

struct AddRequest {
    std::optional<PlacementKind> placement;
    std::string alignment;
    std::vector<katana::entity::EntityId> ids;
    std::optional<ScopeWords> scope;
    std::vector<PlacedVertex> points;
    std::vector<std::pair<std::string, std::string>> options;
    bool preview = false;
};

Result<AddRequest> readAdd(const Words& args)
{
    AddRequest request;
    const auto placementOnce = [&request](PlacementKind kind) -> Status {
        if (request.placement) {
            return makeError(ErrorCode::InvalidArgument,
                             "give one placement: ALIGNMENT, ALONG or AT");
        }
        request.placement = kind;
        return {};
    };
    std::size_t i = 3;
    while (i < args.size()) {
        const std::string& word = args[i];
        const std::string folded = upper(word);
        if (folded == "PREVIEW") {
            request.preview = true;
            ++i;
            continue;
        }
        if (folded == "ALIGNMENT") {
            if (auto status = placementOnce(PlacementKind::Alignment); !status) {
                return status.error();
            }
            if (i + 1 >= args.size()) {
                return usage("ROADOBJECT ADD kind ALIGNMENT name station=s [to=s] [offset=o]");
            }
            request.alignment = args[i + 1];
            i += 2;
            continue;
        }
        if (folded == "ALONG" || folded == "AT") {
            const bool along = folded == "ALONG";
            if (auto status = placementOnce(along ? PlacementKind::Line : PlacementKind::Points);
                !status) {
                return status.error();
            }
            ++i;
            if (i < args.size() && upper(args[i]) == "FROM") {
                ++i;
                auto words = parseScopeWords(args, i);
                if (!words) {
                    return words.error();
                }
                request.scope = std::move(*words);
                continue;
            }
            const std::size_t before = i;
            while (i < args.size()) {
                if (along) {
                    const auto id = typedId(args[i]);
                    if (!id) {
                        break;
                    }
                    // One object along each line: a line named twice would be
                    // two objects on the same ground, doubling its quantities.
                    if (std::ranges::find(request.ids, *id) != request.ids.end()) {
                        return makeError(ErrorCode::InvalidArgument,
                                         "ALONG names that line twice; one object is made along "
                                         "each line",
                                         args[i]);
                    }
                    request.ids.push_back(*id);
                } else {
                    const auto point = typedPoint(args[i]);
                    if (!point) {
                        break;
                    }
                    request.points.push_back(*point);
                }
                ++i;
            }
            if (i == before) {
                return usage(along ? "ROADOBJECT ADD kind ALONG #id [#id ...] | ALONG FROM scope"
                                   : "ROADOBJECT ADD kind AT x,y[,z] [x,y[,z] ...] | AT FROM "
                                     "scope");
            }
            continue;
        }
        if (auto pair = keyValue(word)) {
            request.options.push_back(std::move(*pair));
            ++i;
            continue;
        }
        return makeError(ErrorCode::ParseFailure,
                         "not a word ROADOBJECT ADD takes here: a placement (ALIGNMENT, ALONG, "
                         "AT), key=value or PREVIEW",
                         word);
    }
    if (!request.placement) {
        return usage("ROADOBJECT ADD kind ALIGNMENT name station=s ... | ALONG #id ... | ALONG "
                     "FROM scope | AT x,y ... | AT FROM scope");
    }
    return request;
}

Result<std::string> add(Document& document, const Words& args, const ScopeViewProvider& views)
{
    if (args.size() < 3) {
        return usage("ROADOBJECT ADD kind <placement> [key=value ...] [PREVIEW]");
    }
    auto kind = kindFromWord(args[2]);
    if (!kind) {
        return kind.error();
    }
    auto request = readAdd(args);
    if (!request) {
        return request.error();
    }
    auto set = editableSet(document);
    if (!set) {
        return set.error();
    }
    // What the placement takes, before any object is made of it.
    std::string lead;
    struct Line {
        katana::entity::EntityId id;
        std::vector<PlacedVertex> vertices;
    };
    std::vector<Line> lines;
    std::vector<PlacedVertex> points = request->points;
    const auto& model = document.model();
    if (request->scope) {
        auto match = matchScope(document, *request->scope, views);
        if (!match) {
            return match.error();
        }
        std::map<std::string, std::size_t> skipped;
        for (const katana::entity::EntityId id : match->matched) {
            const katana::entity::Entity* entity = model.entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            if (*request->placement == PlacementKind::Line) {
                if (auto vertices = lineOf(*entity)) {
                    lines.push_back({id, std::move(*vertices)});
                    continue;
                }
            } else if (auto point = pointOf(*entity)) {
                points.push_back(*point);
                continue;
            }
            ++skipped[typeName(*entity)];
        }
        std::size_t left = 0;
        std::string which;
        for (const auto& [type, count] : skipped) {
            left += count;
            which += (which.empty() ? "" : ",") + type;
        }
        lead = scopeRecord(*match) +
               std::format(" {}={} skipped={}",
                           *request->placement == PlacementKind::Line ? "lines" : "points",
                           *request->placement == PlacementKind::Line ? lines.size()
                                                                      : points.size(),
                           left);
        if (left > 0) {
            lead += " skippedtypes=" + which;
        }
        if (lines.empty() && points.empty()) {
            // A scope that takes nothing to place on is reported, not refused.
            return lead + " added=0";
        }
    } else {
        for (const katana::entity::EntityId id : request->ids) {
            const katana::entity::Entity* entity = model.entities.find(id);
            if (entity == nullptr) {
                return makeError(ErrorCode::NotFound, "no entity has that id",
                                 "#" + std::to_string(id));
            }
            auto vertices = lineOf(*entity);
            if (!vertices) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::format("#{} is a {}, not a line", id, typeName(*entity)));
            }
            lines.push_back({id, std::move(*vertices)});
        }
    }

    // The objects: one along the alignment, one along each line, one at the
    // points.
    std::vector<RoadObject> made;
    const auto start = [&](PlacementKind placement) {
        RoadObject object;
        object.kind = *kind;
        object.placement.kind = placement;
        return object;
    };
    switch (*request->placement) {
    case PlacementKind::Alignment: {
        RoadObject object = start(PlacementKind::Alignment);
        object.placement.alignment = request->alignment;
        made.push_back(std::move(object));
        break;
    }
    case PlacementKind::Line:
        for (Line& line : lines) {
            RoadObject object = start(PlacementKind::Line);
            object.placement.vertices = std::move(line.vertices);
            object.placement.source = line.id;
            made.push_back(std::move(object));
        }
        break;
    case PlacementKind::Points: {
        RoadObject object = start(PlacementKind::Points);
        object.placement.vertices = std::move(points);
        made.push_back(std::move(object));
        break;
    }
    }
    const PlacementContext context = placementContext(document);
    std::string reply = lead;
    for (RoadObject& object : made) {
        object.id = set->takeId(object.kind);
        for (const auto& [key, value] : request->options) {
            if (auto status = applyKey(object, key, value); !status) {
                return status.error();
            }
        }
        if (auto status = checkBuilds(context, object); !status) {
            return status.error();
        }
        reply += (reply.empty() ? "" : "\n") +
                 objectRecord(request->preview ? "preview" : "added", object,
                              buildRoadObject(context, object));
    }
    if (request->preview) {
        return reply;
    }
    for (RoadObject& object : made) {
        set->objects.push_back(std::move(object));
    }
    if (auto status = document.setRoadObjects(*set, "ROADOBJECT ADD"); !status) {
        return status.error();
    }
    return reply;
}

Result<std::string> setKeys(Document& document, const Words& args)
{
    if (args.size() < 4) {
        return usage("ROADOBJECT SET id key=value ... [PREVIEW]");
    }
    auto set = editableSet(document);
    if (!set) {
        return set.error();
    }
    RoadObject* object = set->find(args[2]);
    if (object == nullptr) {
        return makeError(ErrorCode::NotFound, "no road object has that id", args[2]);
    }
    bool preview = false;
    std::string changed;
    for (std::size_t i = 3; i < args.size(); ++i) {
        if (upper(args[i]) == "PREVIEW") {
            preview = true;
            continue;
        }
        const auto pair = keyValue(args[i]);
        if (!pair) {
            return makeError(ErrorCode::ParseFailure, "ROADOBJECT SET takes key=value", args[i]);
        }
        if (auto status = applyKey(*object, pair->first, pair->second); !status) {
            return status.error();
        }
        changed += (changed.empty() ? "" : ",") + lower(pair->first);
    }
    if (changed.empty()) {
        return usage("ROADOBJECT SET id key=value ... [PREVIEW]");
    }
    const PlacementContext context = placementContext(document);
    if (auto status = checkBuilds(context, *object); !status) {
        return status.error();
    }
    const std::string reply = objectRecord(preview ? "preview" : "set", *object,
                                           buildRoadObject(context, *object)) +
                              " changed=" + changed;
    if (!preview) {
        if (auto status = document.setRoadObjects(*set, "ROADOBJECT SET"); !status) {
            return status.error();
        }
    }
    return reply;
}

Result<std::string> copy(Document& document, const Words& args)
{
    if (args.size() < 3) {
        return usage("ROADOBJECT COPY id [station=s] [dx=m] [dy=m] [key=value ...] [PREVIEW]");
    }
    auto set = editableSet(document);
    if (!set) {
        return set.error();
    }
    const RoadObject* source = set->find(args[2]);
    if (source == nullptr) {
        return makeError(ErrorCode::NotFound, "no road object has that id", args[2]);
    }
    RoadObject made = *source;
    const std::string from = source->id;
    made.id = set->takeId(made.kind);
    // A to= given is the copy's end whichever order the keys are typed in: only
    // an end inherited from the source moves with the station.
    bool endGiven = false;
    for (std::size_t i = 3; i < args.size(); ++i) {
        const auto pair = keyValue(args[i]);
        endGiven = endGiven || (pair && lower(pair->first) == "to");
    }
    bool preview = false;
    for (std::size_t i = 3; i < args.size(); ++i) {
        if (upper(args[i]) == "PREVIEW") {
            preview = true;
            continue;
        }
        const auto pair = keyValue(args[i]);
        if (!pair) {
            return makeError(ErrorCode::ParseFailure, "ROADOBJECT COPY takes key=value", args[i]);
        }
        const std::string key = lower(pair->first);
        RoadObjectPlacement& placement = made.placement;
        if (key == "station" && placement.kind == PlacementKind::Alignment) {
            // The copy keeps its length: its end moves with its start.
            const auto station = katana::core::parseFiniteDouble(pair->second);
            if (!station) {
                return makeError(ErrorCode::ParseFailure, "station= must be a number",
                                 pair->second);
            }
            if (placement.to && !endGiven) {
                *placement.to += *station - placement.station;
            }
            placement.station = *station;
            continue;
        }
        if (key == "dx" || key == "dy") {
            if (placement.kind == PlacementKind::Alignment) {
                return makeError(ErrorCode::InvalidArgument,
                                 "dx= and dy= move a copy placed along a line or at points; along "
                                 "an alignment give station= and offset=",
                                 from);
            }
            const auto shift = katana::core::parseFiniteDouble(pair->second);
            if (!shift) {
                return makeError(ErrorCode::ParseFailure, key + "= must be a number",
                                 pair->second);
            }
            for (PlacedVertex& vertex : placement.vertices) {
                (key == "dx" ? vertex.position.x : vertex.position.y) += *shift;
            }
            continue;
        }
        if (auto status = applyKey(made, pair->first, pair->second); !status) {
            return status.error();
        }
    }
    const PlacementContext context = placementContext(document);
    if (auto status = checkBuilds(context, made); !status) {
        return status.error();
    }
    const std::string reply =
        objectRecord(preview ? "preview" : "copied", made, buildRoadObject(context, made)) +
        " from=" + from;
    if (!preview) {
        set->objects.push_back(std::move(made));
        if (auto status = document.setRoadObjects(*set, "ROADOBJECT COPY"); !status) {
            return status.error();
        }
    }
    return reply;
}

Result<std::string> remove(Document& document, const Words& args,
                           const ScopeViewProvider& views)
{
    if (args.size() < 3) {
        return usage("ROADOBJECT DELETE id [id ...] | ALL | <scope> [WHERE key=value ...] "
                     "[PREVIEW]");
    }
    auto set = editableSet(document);
    if (!set) {
        return set.error();
    }
    bool preview = false;
    bool all = false;
    std::vector<std::string> ids;
    // ALL on its own is every object, and keeps its plain reply; ALL followed
    // by WHERE is the shared scope's DRAWING, with the scope's record.
    std::vector<std::string> given; // the words that are not PREVIEW
    for (std::size_t i = 2; i < args.size(); ++i) {
        if (upper(args[i]) != "PREVIEW") {
            given.push_back(args[i]);
        }
    }
    const bool bareAll = given.size() == 1 && upper(given.front()) == "ALL";
    std::string lead;
    std::size_t at = 2;
    auto scoped = bareAll ? Result<std::optional<RoadObjectMatch>>(std::optional<RoadObjectMatch>{})
                          : readScope(document, args, at, views);
    if (!scoped) {
        return scoped.error();
    }
    if (*scoped) {
        if (auto status = onlyAfterScope("DELETE", args, at, "PREVIEW"); !status) {
            return status.error();
        }
        preview = at < args.size();
        ids = (*scoped)->ids;
        lead = (*scoped)->record() + "\n";
    } else {
        for (std::size_t i = 2; i < args.size(); ++i) {
            const std::string folded = upper(args[i]);
            if (folded == "PREVIEW") {
                preview = true;
            } else if (folded == "ALL") {
                all = true;
            } else {
                if (isScopeWord(args[i])) {
                    return idsOrScope("DELETE", args[i]);
                }
                const RoadObject* object = set->find(args[i]);
                if (object == nullptr) {
                    return makeError(ErrorCode::NotFound, "no road object has that id", args[i]);
                }
                if (std::ranges::find(ids, object->id) == ids.end()) {
                    ids.push_back(object->id);
                }
            }
        }
    }
    if (all) {
        ids.clear();
        for (const RoadObject& object : set->objects) {
            ids.push_back(object.id);
        }
    }
    std::string named;
    for (const std::string& id : ids) {
        named += (named.empty() ? "" : ",") + id;
    }
    const std::string reply =
        lead + std::format("{} count={}{}", preview ? "preview delete" : "deleted", ids.size(),
                           ids.empty() ? std::string() : " ids=" + named);
    if (preview || ids.empty()) {
        return reply;
    }
    std::erase_if(set->objects, [&ids](const RoadObject& object) {
        return std::ranges::find(ids, object.id) != ids.end();
    });
    if (auto status = document.setRoadObjects(*set, "ROADOBJECT DELETE"); !status) {
        return status.error();
    }
    return reply;
}

Result<std::string> quantities(const Document& document, const Words& args,
                               const ScopeViewProvider& views)
{
    if (const Status status = document.roadObjectsStatus(); !status) {
        return status.error();
    }
    const RoadObjectSet& set = document.roadObjects();
    std::size_t at = 2;
    auto scoped = readScope(document, args, at, views);
    if (!scoped) {
        return scoped.error();
    }
    if (*scoped) {
        if (auto status = onlyAfterScope("QUANTITIES", args, at, {}); !status) {
            return status.error();
        }
    }
    std::vector<const RoadObject*> chosen;
    if (*scoped) {
        for (const RoadObject& object : set.objects) {
            if (std::ranges::find((*scoped)->ids, object.id) != (*scoped)->ids.end()) {
                chosen.push_back(&object);
            }
        }
    } else {
        for (std::size_t i = 2; i < args.size(); ++i) {
            if (isScopeWord(args[i])) {
                return idsOrScope("QUANTITIES", args[i]);
            }
            const RoadObject* object = set.find(args[i]);
            if (object == nullptr) {
                return makeError(ErrorCode::NotFound, "no road object has that id", args[i]);
            }
            // An id named twice is one object, as DELETE reads it: counting it
            // twice would double its length and its count in the totals.
            if (std::ranges::find(chosen, object) == chosen.end()) {
                chosen.push_back(object);
            }
        }
        if (chosen.empty()) {
            for (const RoadObject& object : set.objects) {
                chosen.push_back(&object);
            }
        }
    }
    const PlacementContext context = placementContext(document);
    std::string out;
    const auto line = [&out](const std::string& text) {
        out += (out.empty() ? "" : "\n") + text;
    };
    if (*scoped) {
        line((*scoped)->record());
    }
    // The totals by kind, in the order the kinds are listed.
    std::map<RoadObjectKind, std::pair<std::size_t, std::vector<bim::BimObject>>> byKind;
    for (const RoadObject* object : chosen) {
        bim::BimModel built = buildRoadObject(context, *object);
        for (const bim::BimObject& made : built.objects) {
            line(std::format("quantity object={} part={} class={} type={}", object->id,
                             recordValue(made.id), made.ifcClass, made.predefinedType) +
                 quantityKeys(made.quantities));
        }
        for (const bim::BimProblem& problem : built.problems) {
            line("problem object=" + object->id + " message=" + recordValue(problem.message));
        }
        auto& [count, objects] = byKind[object->kind];
        ++count;
        std::ranges::move(built.objects, std::back_inserter(objects));
    }
    for (const auto& [kind, entry] : byKind) {
        line(std::format("total kind={} objects={}", kindWord(kind), entry.first) +
             quantityKeys(totalQuantities(entry.second)));
    }
    if (chosen.empty()) {
        line("roadobjects count=0");
    }
    return out;
}

Result<std::string> problems(const Document& document)
{
    const bim::BimModel model = bim::placedObjects(document);
    if (model.problems.empty()) {
        return std::string("problems=0");
    }
    std::string out;
    for (const bim::BimProblem& problem : model.problems) {
        out += (out.empty() ? "" : "\n") + std::string("problem source=") +
               recordValue(problem.source) + " message=" + recordValue(problem.message);
    }
    return out;
}

} // namespace

bool isRoadObjectVerb(std::string_view verb)
{
    const std::string folded = upper(verb);
    return folded == "ROADOBJECT" || folded == "ROADOBJECTS";
}

Result<std::string> runRoadObjectVerb(Document& document, const Words& tokens,
                                      const ScopeViewProvider& views)
{
    if (tokens.size() < 2) {
        return usage(kUsage);
    }
    const std::string action = upper(tokens[1]);
    if (action == "KINDS") {
        return kinds(tokens);
    }
    if (action == "LIST") {
        return list(document, tokens, views);
    }
    if (action == "INFO") {
        return info(document, tokens);
    }
    if (action == "ADD") {
        return add(document, tokens, views);
    }
    if (action == "SET") {
        return setKeys(document, tokens);
    }
    if (action == "COPY") {
        return copy(document, tokens);
    }
    if (action == "DELETE") {
        return remove(document, tokens, views);
    }
    if (action == "QUANTITIES") {
        return quantities(document, tokens, views);
    }
    if (action == "PROBLEMS") {
        return problems(document);
    }
    return makeError(ErrorCode::ParseFailure, "not a ROADOBJECT word; " + std::string(kUsage),
                     tokens[1]);
}

std::string roadObjectVerbHelp()
{
    std::string text = R"(Road objects: furniture and structures placed along an alignment or a line
(docs/road_objects.md). Every edit is one undo step; PREVIEW changes nothing.

ROADOBJECT KINDS [kind]          the kinds, or one kind's keys with units, ranges, defaults
ROADOBJECT LIST [JSON]           a record per object, or the stored JSON
ROADOBJECT LIST <scope> [WHERE k=v ...]     the objects a scope takes (below)
ROADOBJECT INFO id               its keys, the 3D objects made of it, its problems
ROADOBJECT ADD kind [key=value ...] <placement> [key=value ...] [PREVIEW]
  placement: ALIGNMENT name station=s [to=s] [offset=o] [spacing=m]
             ALONG #id [#id ...] | ALONG FROM <scope> [WHERE k=v ...]   one along each line
             AT x,y[,z] ... | AT FROM <scope> [WHERE k=v ...]            at the points
  (after WHERE every key=value is a condition: give an object's keys before ALONG or AT)
ROADOBJECT SET id key=value ... [PREVIEW]
ROADOBJECT COPY id [station=s] [dx=m] [dy=m] [key=value ...] [PREVIEW]
ROADOBJECT DELETE id [id ...] | ALL | <scope> [WHERE k=v ...] [PREVIEW]
ROADOBJECT QUANTITIES [id ... | <scope> [WHERE k=v ...]]   per 3D object, then totals by kind
ROADOBJECT PROBLEMS              what could not be made, and why
Every object: name= layer= colour=#RRGGBB|name material= facility= (the road it is part of)
  station= to= offset= (left positive) spacing= (guide posts, signs, lights) heights=profile|surface|line
  surface=name crossfall=% (from the centreline out, on the profile) dz= (raised from the ground)
  (empty: offset= crossfall= dz= go back to 0; to= spacing= heights= surface= and the kind's keys clear)
<scope> is SELECTION | DRAWING | VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY], as every
  verb on drawing data takes it; its record (scope=... matched=N) leads the reply, and a scope that
  takes nothing is reported. Of a road object: LAYERS and WHERE LAYER= read its layer; AREA and VIEW
  its plan extent (an object that is not made has none: unmade=N); SELECTION the objects placed
  ALONG a selected line. WHERE PROP=key:pattern reads the keys SET writes (kind, id, type, offset ...),
  COLOUR= the colour it is drawn in, DRAWN a layer that is shown; TYPE=, STYLE= and TEXT= are refused.
Kinds:)";
    for (const RoadObjectKind kind : roadObjectKinds()) {
        std::string keys;
        for (const ParameterSpec& spec : parametersOf(kind)) {
            keys += " " + std::string(spec.key) + "=";
        }
        text += std::format("\n  {:<10}{}", kindWord(kind), keys);
    }
    return text;
}

namespace {

// The report of the drawing's road objects, or of the ones `match` took.
std::string reportOf(const Document& document, const RoadObjectMatch* match)
{
    using Json = nlohmann::json;
    Json report{{"format", "katana-road-objects-report"}, {"version", 1}};
    if (match != nullptr) {
        report["scope"] = Json{{"record", match->record()},
                               {"matched", match->ids.size()},
                               {"unmade", match->unmade}};
    }
    Json objects = Json::array();
    Json problems = Json::array();
    if (const Status status = document.roadObjectsStatus(); !status) {
        problems.push_back(
            Json{{"source", "road objects"}, {"message", status.error().describe()}});
    }
    const PlacementContext context = placementContext(document);
    const auto stored = roadObjectsToJson(document.roadObjects());
    Json definitions = stored ? Json::parse(*stored, nullptr, false) : Json();
    const Json& list = definitions.is_object() && definitions.contains("objects")
                           ? definitions["objects"]
                           : Json::array();
    std::size_t index = 0;
    for (const RoadObject& object : document.roadObjects().objects) {
        Json entry = index < list.size() ? list[index] : Json{{"id", object.id}};
        ++index;
        if (match != nullptr && std::ranges::find(match->ids, object.id) == match->ids.end()) {
            continue;
        }
        const bim::BimModel built = buildRoadObject(context, object);
        Json made = Json::array();
        for (const bim::BimObject& part : built.objects) {
            Json quantitiesJson = Json::object();
            for (const bim::Quantity& quantity : part.quantities) {
                quantitiesJson[quantity.name] = quantity.value;
            }
            made.push_back(Json{{"id", part.id},
                                {"name", part.name},
                                {"ifc_class", part.ifcClass},
                                {"predefined_type", part.predefinedType},
                                {"layer", part.layer},
                                {"material", part.material},
                                {"triangles", part.mesh.triangleCount()},
                                {"closed", isClosedMesh(part.mesh)},
                                {"quantities", std::move(quantitiesJson)}});
        }
        entry["made"] = std::move(made);
        Json own = Json::array();
        for (const bim::BimProblem& problem : built.problems) {
            own.push_back(problem.message);
            problems.push_back(Json{{"source", problem.source}, {"message", problem.message}});
        }
        entry["problems"] = std::move(own);
        objects.push_back(std::move(entry));
    }
    report["objects"] = std::move(objects);
    report["problems"] = std::move(problems);
    return report.dump(2, ' ', false, Json::error_handler_t::replace);
}

} // namespace

std::string roadObjectsReport(const Document& document)
{
    return reportOf(document, nullptr);
}

Result<std::string> roadObjectsReport(const Document& document, const ScopeWords& words,
                                      const ScopeViewProvider& views)
{
    auto match = matchRoadObjects(document, words, views);
    if (!match) {
        return match.error();
    }
    return reportOf(document, &*match);
}

} // namespace katana::cad::road
