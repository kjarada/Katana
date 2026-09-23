#include "modify_edit_support.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <limits>
#include <system_error>
#include <utility>

#include "katana/commands/change_set.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/entity_geometry.hpp"

namespace katana::cad::tools::modify_edit {

namespace tol = katana::math::tolerance;
using katana::commands::ChangeSet;
using katana::commands::ChangeSetCommand;
using katana::commands::CommandContext;
using katana::core::Result;

// ---- typed input -------------------------------------------------------------------

bool isOption(std::string_view text, std::string_view word, std::string_view shortForm)
{
    const std::string_view typed = katana::core::trimmed(text);
    return katana::core::equalsIgnoringCase(typed, word) ||
           katana::core::equalsIgnoringCase(typed, shortForm);
}

std::optional<double> parseNumber(std::string_view text)
{
    return katana::core::parseFiniteDouble(katana::core::trimmed(text));
}

std::string formatNumber(double value)
{
    // The shortest text that reads back exactly, as formatExactReal writes -
    // but never in exponent form, which a person reading "within 1e-04" in a
    // prompt should not have to decode. Beyond 1e15 a fixed figure is a wall
    // of digits, and no drawing value is that large, so the exact form stays.
    if (!std::isfinite(value) || std::abs(value) >= 1e15) {
        return katana::core::formatExactReal(value);
    }
    std::array<char, 64> buffer{};
    const auto written =
        std::to_chars(buffer.data(), buffer.data() + buffer.size(), value, std::chars_format::fixed);
    if (written.ec != std::errc{}) {
        return katana::core::formatExactReal(value);
    }
    return std::string(buffer.data(), written.ptr);
}

std::string asSentence(std::string_view text)
{
    std::string out(katana::core::trimmed(text));
    if (out.empty()) {
        return out;
    }
    if (out.front() >= 'a' && out.front() <= 'z') {
        out.front() = static_cast<char>(out.front() - 'a' + 'A');
    }
    if (out.back() != '.' && out.back() != '?' && out.back() != '!') {
        out.push_back('.');
    }
    return out;
}

// ---- the drawing -------------------------------------------------------------------

std::string kindName(const Geometry& geometry)
{
    switch (katana::entity::typeOf(geometry)) {
    case katana::entity::EntityType::Point:
        return "point";
    case katana::entity::EntityType::Line:
        return "line";
    case katana::entity::EntityType::Arc:
        return "arc";
    case katana::entity::EntityType::Polyline:
        return std::get<Polyline2>(geometry).closed ? "closed polyline" : "polyline";
    case katana::entity::EntityType::Circle:
        return "circle";
    case katana::entity::EntityType::Text:
        return "text";
    case katana::entity::EntityType::Dimension:
        return "dimension";
    }
    return "object";
}

std::string withArticle(const std::string& noun)
{
    const bool vowel = !noun.empty() && std::string_view("aeiou").find(noun.front()) !=
                                            std::string_view::npos;
    return (vowel ? "an " : "a ") + noun;
}

std::optional<std::string> refusalToEdit(const Document& document, EntityId id)
{
    const Entity* entity = document.model().entities.find(id);
    if (entity == nullptr) {
        return "That object is no longer in the drawing.";
    }
    if (document.model().layers.effectivelyLocked(entity->layer)) {
        return "That " + kindName(entity->geometry) + " is on the locked layer '" +
               entity->layer + "'; unlock the layer to edit it.";
    }
    return std::nullopt;
}

std::vector<EntityId> selectionNow(const ToolContext& context, const std::vector<EntityId>& picked)
{
    std::vector<EntityId> ids = picked;
    if (context.document != nullptr) {
        // The live selection rather than the one the tool started with: the
        // view changes it while a Selection step is open, and an entity the
        // user deselected there must not be acted on.
        const auto live = context.document->selection().ids();
        ids.insert(ids.end(), live.begin(), live.end());
    } else {
        ids.insert(ids.end(), context.selection.begin(), context.selection.end());
    }
    std::ranges::sort(ids);
    ids.erase(std::unique(ids.begin(), ids.end()), ids.end());
    if (context.document != nullptr) {
        std::erase_if(ids, [&](EntityId id) {
            return !context.document->model().entities.contains(id);
        });
    }
    return ids;
}

// ---- heights -----------------------------------------------------------------------

std::vector<Point2> heightVertices(const Geometry& geometry)
{
    if (const auto* point = std::get_if<katana::entity::PointGeometry>(&geometry)) {
        return {point->position};
    }
    if (const auto* segment = std::get_if<Segment2>(&geometry)) {
        return {segment->start, segment->end};
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        return {arc->startPoint(), arc->endPoint()};
    }
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        return polyline->vertices;
    }
    if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        return {circle->center};
    }
    if (const auto* text = std::get_if<katana::entity::TextGeometry>(&geometry)) {
        return {text->position};
    }
    return {}; // a dimension has no heights of its own
}

namespace {

// The height a fraction `t` of the way from a vertex of height `a` to one of
// height `b`: that vertex's own at either end, else interpolated when both are
// known. A vertex exactly at an end must not lose its height because the
// other end has none.
std::optional<double> interpolated(std::optional<double> a, std::optional<double> b, double t,
                                   bool atStart, bool atEnd)
{
    if (atStart) {
        return a;
    }
    if (atEnd) {
        return b;
    }
    if (a && b) {
        return *a + (*b - *a) * t;
    }
    return std::nullopt;
}

} // namespace

std::optional<double> heightAt(const Entity& original, const Point2& p)
{
    const std::size_t count = heightVertices(original.geometry).size();
    if (count == 0) {
        return std::nullopt;
    }
    const auto heights = katana::entity::heightsOf(original.properties, count);
    const Geometry& geometry = original.geometry;
    if (const auto* segment = std::get_if<Segment2>(&geometry)) {
        if (segment->distanceTo(p) > tol::kGeometric) {
            return std::nullopt;
        }
        return interpolated(heights[0], heights[1], segment->parameterOf(p),
                            p.distanceTo(segment->start) <= tol::kGeometric,
                            p.distanceTo(segment->end) <= tol::kGeometric);
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        if (arc->distanceTo(p) > tol::kGeometric) {
            return std::nullopt;
        }
        return interpolated(heights[0], heights[1], arc->parameterOfAngle((p - arc->center).angle()),
                            p.distanceTo(arc->startPoint()) <= tol::kGeometric,
                            p.distanceTo(arc->endPoint()) <= tol::kGeometric);
    }
    if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        // A circle is level: its one height is the height of every point on it.
        const bool onIt = circle->distanceTo(p) <= tol::kGeometric ||
                          circle->center.distanceTo(p) <= tol::kGeometric;
        return onIt ? heights[0] : std::nullopt;
    }
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        std::optional<std::size_t> best;
        double bestDistance = std::numeric_limits<double>::infinity();
        for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
            const double d = polyline->segment(i).distanceTo(p);
            if (d < bestDistance) {
                bestDistance = d;
                best = i;
            }
        }
        if (!best || bestDistance > tol::kGeometric) {
            return std::nullopt;
        }
        const Segment2 segment = polyline->segment(*best);
        const std::size_t next = (*best + 1) % polyline->vertices.size();
        return interpolated(heights[*best], heights[next], segment.parameterOf(p),
                            p.distanceTo(segment.start) <= tol::kGeometric,
                            p.distanceTo(segment.end) <= tol::kGeometric);
    }
    if (const auto* point = std::get_if<katana::entity::PointGeometry>(&geometry)) {
        return point->position.distanceTo(p) <= tol::kGeometric ? heights[0] : std::nullopt;
    }
    return std::nullopt;
}

void carryHeights(const Entity& original, Entity& piece)
{
    std::vector<std::optional<double>> heights;
    for (const Point2& vertex : heightVertices(piece.geometry)) {
        heights.push_back(heightAt(original, vertex));
    }
    katana::entity::setHeights(piece.properties, heights);
}

// ---- curves ------------------------------------------------------------------------

void appendEdges(const Geometry& geometry, std::vector<Curve2>& out)
{
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
            if (!polyline->segment(i).isDegenerate()) {
                out.emplace_back(polyline->segment(i));
            }
        }
        return;
    }
    if (auto curve = asCurve(geometry)) {
        out.push_back(*curve);
    }
}

Geometry asGeometry(const Curve2& curve)
{
    return std::visit([](const auto& c) -> Geometry { return c; }, curve);
}

std::optional<Curve2> asCurve(const Geometry& geometry)
{
    if (const auto* segment = std::get_if<Segment2>(&geometry)) {
        return Curve2{*segment};
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        return Curve2{*arc};
    }
    if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        return Curve2{*circle};
    }
    return std::nullopt;
}

// ---- PolylinePath ------------------------------------------------------------------

PolylinePath::PolylinePath(Polyline2 polyline) : polyline_(std::move(polyline))
{
    path_ = polyline_.vertices;
    if (polyline_.closed && !path_.empty()) {
        path_.push_back(path_.front());
    }
    stations_.push_back(0.0);
    for (std::size_t i = 1; i < path_.size(); ++i) {
        stations_.push_back(stations_.back() + path_[i - 1].distanceTo(path_[i]));
    }
}

double PolylinePath::stationOf(std::size_t segmentIndex, const Point2& pointOnSegment) const
{
    const Segment2 piece = segment(segmentIndex);
    return stations_[segmentIndex] + piece.parameterOf(pointOnSegment) * piece.length();
}

double PolylinePath::stationOf(const Point2& p) const
{
    std::size_t best = 0;
    double bestDistance = std::numeric_limits<double>::infinity();
    for (std::size_t i = 0; i + 1 < path_.size(); ++i) {
        const double d = segment(i).distanceTo(p);
        if (d < bestDistance) {
            bestDistance = d;
            best = i;
        }
    }
    return path_.size() < 2 ? 0.0 : stationOf(best, p);
}

Point2 PolylinePath::pointAt(double station) const
{
    if (path_.empty()) {
        return {};
    }
    const double s = std::clamp(station, 0.0, length());
    for (std::size_t i = 0; i + 1 < path_.size(); ++i) {
        if (s > stations_[i + 1] + tol::kGeometric) {
            continue;
        }
        // A station at a vertex returns the vertex itself, not a sum that
        // rounds a last bit away from it.
        if (std::abs(s - stations_[i]) <= tol::kGeometric) {
            return path_[i];
        }
        if (std::abs(s - stations_[i + 1]) <= tol::kGeometric) {
            return path_[i + 1];
        }
        const double span = stations_[i + 1] - stations_[i];
        return segment(i).pointAt(span > 0.0 ? (s - stations_[i]) / span : 0.0);
    }
    return path_.back();
}

namespace {

void dropRepeats(std::vector<Point2>& vertices)
{
    vertices.erase(std::unique(vertices.begin(), vertices.end(),
                               [](const Point2& a, const Point2& b) {
                                   return a.distanceTo(b) <= tol::kGeometric;
                               }),
                   vertices.end());
}

} // namespace

Polyline2 PolylinePath::between(double from, double to) const
{
    Polyline2 out;
    out.vertices.push_back(pointAt(from));
    for (std::size_t i = 0; i < path_.size(); ++i) {
        if (stations_[i] > from + tol::kGeometric && stations_[i] < to - tol::kGeometric) {
            out.vertices.push_back(path_[i]);
        }
    }
    out.vertices.push_back(pointAt(to));
    dropRepeats(out.vertices);
    return out;
}

Polyline2 PolylinePath::wrapping(double from, double to) const
{
    Polyline2 out = between(from, length());
    const Polyline2 tail = between(0.0, to);
    out.vertices.insert(out.vertices.end(), tail.vertices.begin(), tail.vertices.end());
    dropRepeats(out.vertices);
    return out;
}

// ---- EditSession -------------------------------------------------------------------

const Entity* EditSession::original(EntityId id) const
{
    return document_->model().entities.find(id);
}

std::vector<Entity> EditSession::pieces(EntityId id) const
{
    if (const auto found = state_.edited.find(id); found != state_.edited.end()) {
        return found->second;
    }
    if (const Entity* entity = original(id)) {
        return {*entity};
    }
    return {};
}

std::vector<EntityId> EditSession::editedIds() const
{
    std::vector<EntityId> ids;
    for (const auto& entry : state_.edited) {
        ids.push_back(entry.first);
    }
    return ids;
}

std::vector<Geometry> EditSession::shapes() const
{
    std::vector<Geometry> out;
    for (const auto& entry : state_.edited) {
        for (const Entity& piece : entry.second) {
            out.push_back(piece.geometry);
        }
    }
    for (const Entity& entity : state_.added) {
        out.push_back(entity.geometry);
    }
    return out;
}

void EditSession::begin() { history_.push_back(state_); }

void EditSession::replace(EntityId id, std::vector<Entity> pieces)
{
    state_.edited[id] = std::move(pieces);
}

void EditSession::add(Entity entity)
{
    entity.id = katana::entity::kInvalidEntityId;
    state_.added.push_back(std::move(entity));
}

bool EditSession::undo()
{
    if (history_.empty()) {
        return false;
    }
    state_ = std::move(history_.back());
    history_.pop_back();
    return true;
}

katana::commands::CommandPtr EditSession::commit(std::string name) const
{
    ChangeSet changes;
    for (const auto& [id, pieces] : state_.edited) {
        if (pieces.empty()) {
            changes.remove.push_back(id);
            continue;
        }
        Entity first = pieces.front();
        first.id = id;
        changes.modify.push_back(std::move(first));
        for (std::size_t i = 1; i < pieces.size(); ++i) {
            Entity piece = pieces[i];
            piece.id = katana::entity::kInvalidEntityId;
            changes.add.push_back(std::move(piece));
        }
    }
    changes.add.insert(changes.add.end(), state_.added.begin(), state_.added.end());
    // The set is worked out now, from the drawing the user saw while picking,
    // and handed over whole: the command's validation still checks it against
    // the model it is applied to.
    return std::make_unique<ChangeSetCommand>(
        std::move(name),
        [changes = std::move(changes)](const CommandContext&) -> Result<ChangeSet> {
            return changes;
        });
}

std::optional<std::size_t> nearestPiece(const std::vector<Entity>& pieces, const Point2& at)
{
    std::optional<std::size_t> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    for (std::size_t i = 0; i < pieces.size(); ++i) {
        const double d = katana::entity::distanceTo(pieces[i].geometry, at);
        if (d < bestDistance) {
            bestDistance = d;
            best = i;
        }
    }
    return best;
}

} // namespace katana::cad::tools::modify_edit
