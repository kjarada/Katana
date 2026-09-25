#include "katana/cad/plotting/legend.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <optional>
#include <set>
#include <tuple>
#include <utility>
#include <variant>

#include <nlohmann/json.hpp>

#include "katana/cad/hatching.hpp"
#include "katana/cad/linework.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/spatial_query.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/chording.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

// ---- the window a plan shows ------------------------------------------------------

// A plan viewport's window on the ground: its rectangle at its scale, about
// its centre, turned by its rotation. Tests are made in the window's own
// frame, where the rectangle is axis-aligned: x along the paper, y up it.
struct Window {
    const LayerOverrides* hidden = nullptr;
    Point2 centre{};
    double cosine = 1.0;
    double sine = 0.0;
    double halfWidth = 0.0; // metres
    double halfHeight = 0.0;
    Box2 box; // the rotated rectangle's box on the ground: the broad phase

    // A ground point in the window's frame.
    [[nodiscard]] Point2 local(const Point2& p) const
    {
        const double dx = p.x - centre.x;
        const double dy = p.y - centre.y;
        return Point2(cosine * dx + sine * dy, -sine * dx + cosine * dy);
    }
    [[nodiscard]] Point2 world(double x, double y) const
    {
        return Point2(centre.x + cosine * x - sine * y, centre.y + sine * x + cosine * y);
    }
    [[nodiscard]] bool holds(const Point2& p, double reach = 0.0) const
    {
        const Point2 l = local(p);
        return std::abs(l.x) <= halfWidth + reach && std::abs(l.y) <= halfHeight + reach;
    }

    // Whether the segment a-b crosses the rectangle: Liang-Barsky clipping in
    // the window's frame, which keeps a part of it or none.
    [[nodiscard]] bool meets(const Point2& a, const Point2& b) const
    {
        const Point2 p = local(a);
        const Point2 q = local(b);
        const double dx = q.x - p.x;
        const double dy = q.y - p.y;
        double t0 = 0.0;
        double t1 = 1.0;
        const auto edge = [&t0, &t1](double direction, double room) {
            if (direction == 0.0) {
                return room >= 0.0;
            }
            const double t = room / direction;
            if (direction < 0.0) {
                t0 = std::max(t0, t);
            } else {
                t1 = std::min(t1, t);
            }
            return t0 <= t1;
        };
        return edge(-dx, p.x + halfWidth) && edge(dx, halfWidth - p.x) &&
               edge(-dy, p.y + halfHeight) && edge(dy, halfHeight - p.y);
    }

    [[nodiscard]] bool meets(const std::vector<Point2>& points, bool closed) const
    {
        if (points.size() == 1) {
            return holds(points.front());
        }
        for (std::size_t i = 1; i < points.size(); ++i) {
            if (meets(points[i - 1], points[i])) {
                return true;
            }
        }
        return closed && points.size() > 2 && meets(points.back(), points.front());
    }

    // Whether a box on the ground meets the rotated rectangle: two
    // rectangles are apart exactly when one of their four edge directions
    // separates them.
    [[nodiscard]] bool meets(const Box2& other) const
    {
        if (!other.intersects(box)) {
            return false;
        }
        Box2 turned;
        for (const Point2& corner : {other.min, other.max, Point2(other.min.x, other.max.y),
                                     Point2(other.max.x, other.min.y)}) {
            turned.expand(local(corner));
        }
        return turned.min.x <= halfWidth && turned.max.x >= -halfWidth &&
               turned.min.y <= halfHeight && turned.max.y >= -halfHeight;
    }
};

std::optional<Window> windowOf(const Viewport& viewport, const PlanWindow& at)
{
    if (viewport.rect.empty() || !(at.scale > 0.0) || !std::isfinite(at.scale) ||
        !at.centre.isFinite() || !std::isfinite(viewport.rotation)) {
        return std::nullopt;
    }
    Window window;
    window.hidden = &viewport.hiddenLayers;
    window.centre = at.centre;
    window.cosine = std::cos(viewport.rotation);
    window.sine = std::sin(viewport.rotation);
    // Paper millimetres to metres on the ground.
    window.halfWidth = 0.5 * viewport.rect.width() * at.scale / 1000.0;
    window.halfHeight = 0.5 * viewport.rect.height() * at.scale / 1000.0;
    for (const double x : {-window.halfWidth, window.halfWidth}) {
        for (const double y : {-window.halfHeight, window.halfHeight}) {
            window.box.expand(window.world(x, y));
        }
    }
    return window;
}

// ---- what an entity prints as -----------------------------------------------------

// What an entity's look depends on, besides its geometry: resolved once per
// distinct layer, style and colour, as the plan painter does.
struct Resolved {
    katana::entity::ResolvedLayer layer;
    katana::entity::ResolvedDisplay display;
    bool styled = false;  // it names a style the drawing has
    bool hatched = false; // its hatch pattern draws something
};

struct ResolvedKey {
    std::string layer;
    std::string style;
    std::optional<std::uint32_t> colour;
    friend auto operator<=>(const ResolvedKey&, const ResolvedKey&) = default;
};

class Resolver {
  public:
    explicit Resolver(const katana::entity::Model& model) : model_(model) {}

    const Resolved& operator()(const Entity& entity)
    {
        ResolvedKey key{entity.layer, entity.style, std::nullopt};
        if (entity.color) {
            const katana::entity::Color& c = *entity.color;
            key.colour = (std::uint32_t{c.r} << 24) | (std::uint32_t{c.g} << 16) |
                         (std::uint32_t{c.b} << 8) | std::uint32_t{c.a};
        }
        auto found = resolved_.find(key);
        if (found == resolved_.end()) {
            Resolved r;
            r.layer = model_.layers.resolve(entity.layer);
            r.display = katana::entity::resolveDisplay(model_, entity);
            r.styled = !entity.style.empty() && model_.styles.find(entity.style) != nullptr;
            r.hatched = resolveHatchPattern(model_, r.display) != nullptr;
            found = resolved_.emplace(std::move(key), std::move(r)).first;
        }
        return found->second;
    }

  private:
    const katana::entity::Model& model_;
    std::map<ResolvedKey, Resolved> resolved_;
};

// The mark an entity prints as; none for a dimension, which is annotation a
// legend does not explain.
std::optional<LegendKind> kindOf(const Entity& entity, const Resolved& resolved)
{
    using katana::entity::DimensionGeometry;
    using katana::entity::PointGeometry;
    using katana::entity::TextGeometry;
    if (std::holds_alternative<PointGeometry>(entity.geometry)) {
        return resolved.display.symbol.empty() ? LegendKind::Point : LegendKind::Symbol;
    }
    if (std::holds_alternative<TextGeometry>(entity.geometry)) {
        return LegendKind::Text;
    }
    if (std::holds_alternative<DimensionGeometry>(entity.geometry)) {
        return std::nullopt;
    }
    // Only a closed polyline is filled or hatched (the plan painter's rule).
    if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry);
        polyline != nullptr && polyline->closed && polyline->vertices.size() >= 3 &&
        resolved.hatched) {
        return LegendKind::Area;
    }
    return LegendKind::Line;
}

// Whether what `entity` prints reaches into `window`. By its shape, not its
// box: a pipe running diagonally past a twisted strip has a box that meets
// the strip and a line that never does. A hatched area that covers the whole
// window prints inside it with no edge there; a symbol reaches half its size
// beyond its point.
bool shows(const Window& window, const Entity& entity, const Resolved& resolved)
{
    // Arcs and circles are chorded to a thousandth of the window, far finer
    // than the question needs.
    const double tolerance =
        std::max(std::min(window.halfWidth, window.halfHeight) * 1e-3, 1e-6);
    return std::visit(
        [&](const auto& shape) -> bool {
            using T = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<T, katana::entity::PointGeometry>) {
                const double reach =
                    resolved.display.symbol.empty() ? 0.0 : 0.5 * resolved.display.symbolSize;
                return window.holds(shape.position, reach);
            } else if constexpr (std::is_same_v<T, Segment2>) {
                return window.meets(shape.start, shape.end);
            } else if constexpr (std::is_same_v<T, Polyline2>) {
                if (!window.meets(shape.boundingBox())) {
                    return false;
                }
                if (window.meets(shape.vertices, shape.closed)) {
                    return true;
                }
                return shape.closed && resolved.hatched && shape.contains(window.centre);
            } else if constexpr (std::is_same_v<T, Arc2>) {
                return window.meets(shape.boundingBox()) &&
                       window.meets(katana::geometry::chordArc(shape, tolerance), false);
            } else if constexpr (std::is_same_v<T, Circle2>) {
                return window.meets(shape.boundingBox()) &&
                       window.meets(katana::geometry::chordCircle(shape, tolerance), true);
            } else {
                // A text: the box its letters take.
                return window.meets(katana::entity::boundingBox(entity.geometry));
            }
        },
        entity.geometry);
}

// ---- grouping -----------------------------------------------------------------------

struct GroupKey {
    LegendKind kind = LegendKind::Line;
    std::string style; // for a styled group
    std::string layer; // for any other
    friend auto operator<=>(const GroupKey&, const GroupKey&) = default;
};

// How an entry looks, as a value to count.
using Look = std::tuple<std::uint32_t, double, std::string, std::string, double, std::string>;

Look lookOf(const katana::entity::ResolvedDisplay& d)
{
    const std::uint32_t colour = (std::uint32_t{d.color.r} << 24) |
                                 (std::uint32_t{d.color.g} << 16) |
                                 (std::uint32_t{d.color.b} << 8) | std::uint32_t{d.color.a};
    return {colour, d.lineWeight, d.linetype, d.symbol, d.symbolSize, d.hatchPattern};
}

struct Group {
    std::size_t count = 0;
    struct Seen {
        std::size_t count = 0;
        EntityId firstId = 0;
        katana::entity::ResolvedDisplay display;
    };
    std::map<Look, Seen> looks;
    // The codes of its coded entities and what the library says of them;
    // `undescribed` when a coded entity's code has no description.
    std::set<std::string> descriptions;
    std::set<std::string> codes;
    bool undescribed = false;
};

// The description the code library gives an entity's code, or empty.
class CodeLabels {
  public:
    CodeLabels(const katana::entity::SurveyMap* map, std::string property)
        : map_(map), property_(std::move(property))
    {
    }

    [[nodiscard]] bool active() const { return map_ != nullptr && !map_->empty(); }

    // The code the library is asked about, and its description; nothing for
    // an entity that carries no code.
    std::optional<std::pair<std::string, std::string>> of(const Entity& entity)
    {
        const std::string* carried = surveyCodeOf(entity, property_);
        if (carried == nullptr) {
            return std::nullopt;
        }
        // A point's field code is its string name and its linework controls
        // ("PABB ST"); the library knows the name (survey_coding.cpp).
        std::string code = *carried;
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry)) {
            if (FieldCode field = parseFieldCode(code, LineworkCodes{}); !field.name.empty()) {
                code = std::move(field.name);
            }
        }
        auto found = described_.find(code);
        if (found == described_.end()) {
            const katana::entity::SurveyMatch match = map_->lookup(code);
            found = described_.emplace(code, match.matched() ? match.resolved.comment : "").first;
        }
        return std::pair{found->first, found->second};
    }

  private:
    const katana::entity::SurveyMap* map_;
    std::string property_;
    std::map<std::string, std::string, std::less<>> described_;
};

std::string folded(std::string_view text)
{
    std::string out(text);
    std::transform(out.begin(), out.end(), out.begin(), [](unsigned char c) {
        return static_cast<char>(c >= 'A' && c <= 'Z' ? c - 'A' + 'a' : c);
    });
    return out;
}

// ---- the viewports -------------------------------------------------------------------

void placedPlans(const Sheet& sheet, std::vector<const Viewport*>& out)
{
    for (const Viewport& viewport : sheet.viewports) {
        if (viewport.kind == ViewportKind::Plan && !viewport.rect.empty()) {
            out.push_back(&viewport);
        }
    }
}

Point2 turned(const Point2& v, double radians)
{
    const double c = std::cos(radians);
    const double s = std::sin(radians);
    return Point2(c * v.x - s * v.y, s * v.x + c * v.y);
}

} // namespace

// ---- names ------------------------------------------------------------------------------

std::string_view toString(LegendScope scope)
{
    switch (scope) {
    case LegendScope::ThisSheet:
        return "this_sheet";
    case LegendScope::WholeSet:
        return "whole_set";
    case LegendScope::WholeDrawing:
        return "whole_drawing";
    }
    return "this_sheet";
}

std::optional<LegendScope> legendScopeFrom(std::string_view name)
{
    for (const LegendScope scope :
         {LegendScope::ThisSheet, LegendScope::WholeSet, LegendScope::WholeDrawing}) {
        if (toString(scope) == name) {
            return scope;
        }
    }
    return std::nullopt;
}

std::string_view toString(LegendKind kind)
{
    switch (kind) {
    case LegendKind::Symbol:
        return "symbol";
    case LegendKind::Point:
        return "point";
    case LegendKind::Line:
        return "line";
    case LegendKind::Area:
        return "area";
    case LegendKind::Text:
        return "text";
    }
    return "line";
}

// ---- the window of an automatic plan --------------------------------------------------

PlanWindow fittedPlanWindow(const katana::entity::Model& model, const Viewport& viewport)
{
    PlanWindow at{viewport.scale, viewport.centre};
    if ((!viewport.autoScale && !viewport.autoCentre) || viewport.rect.empty()) {
        return at;
    }
    // What the viewport shows: its stretch of an alignment, or the drawing.
    std::vector<Point2> points;
    const ViewportSource& from = viewport.source;
    if (!from.alignment.empty()) {
        if (const auto* alignment = model.alignments.find(from.alignment)) {
            if (auto solved = katana::geometry::solveAlignment(alignment->horizontal)) {
                double a = from.chainageFrom;
                double b = from.chainageTo;
                if (!(b > a)) {
                    a = solved->startStation();
                    b = solved->endStation();
                }
                for (int i = 0; i <= 64; ++i) {
                    if (auto p = solved->pointAtStation(a + (b - a) * i / 64.0)) {
                        points.push_back(*p);
                    }
                }
            }
        }
    }
    if (points.empty()) {
        Box2 box = drawnExtent(model, viewport.hiddenLayers);
        for (const katana::entity::Alignment& alignment : model.alignments.all()) {
            if (const auto solved = katana::geometry::solveAlignment(alignment.horizontal)) {
                for (const Point2& vertex : solved->toPolyline(1.0).vertices) {
                    box.expand(vertex);
                }
            }
        }
        if (box.empty()) {
            return at;
        }
        points = {box.min, box.max, Point2(box.min.x, box.max.y), Point2(box.max.x, box.min.y)};
    }
    if (viewport.autoCentre) {
        Box2 box;
        for (const Point2& p : points) {
            box.expand(turned(p, -viewport.rotation));
        }
        at.centre = turned(box.center(), viewport.rotation);
    }
    if (viewport.autoScale) {
        double halfW = 0.0;
        double halfH = 0.0;
        for (const Point2& p : points) {
            const Point2 d = turned(p - at.centre, -viewport.rotation);
            halfW = std::max(halfW, std::abs(d.x));
            halfH = std::max(halfH, std::abs(d.y));
        }
        const double needed = std::max(2.0 * halfW * 1000.0 / viewport.rect.width(),
                                       2.0 * halfH * 1000.0 / viewport.rect.height()) *
                              1.04;
        if (needed > 0.0) {
            if (auto scale = sheetScaleAtLeast(needed)) {
                at.scale = *scale;
            }
        }
    }
    return at;
}

// ---- the legend ---------------------------------------------------------------------------

Result<Legend> computeLegend(const katana::entity::Model& model, const SheetSet& set,
                             std::size_t sheetIndex, LegendScope scope,
                             const LegendOptions& options)
{
    if (sheetIndex >= set.sheets.size()) {
        return makeError(ErrorCode::InvalidArgument, "no sheet at that index",
                         std::to_string(sheetIndex));
    }
    const auto resolveWindow = [&](const Viewport& viewport) {
        return options.window ? options.window(viewport) : fittedPlanWindow(model, viewport);
    };

    // The plans looked through, after the fall-backs: a sheet without a plan
    // lists what the set shows, and a set without one the whole drawing.
    Legend legend;
    std::vector<const Viewport*> own;
    placedPlans(set.sheets[sheetIndex], own);
    std::vector<const Viewport*> plans;
    LegendScope used = scope;
    if (used == LegendScope::ThisSheet) {
        plans = own;
        if (plans.empty()) {
            used = LegendScope::WholeSet;
        }
    }
    if (used == LegendScope::WholeSet) {
        for (const Sheet& sheet : set.sheets) {
            placedPlans(sheet, plans);
        }
        if (plans.empty()) {
            used = LegendScope::WholeDrawing;
        }
    }
    legend.scope = used;
    if (used == LegendScope::WholeDrawing) {
        plans.clear();
    }

    // The samples' scale: the plan the legend stands beside, when it has one.
    if (!own.empty()) {
        legend.scale = resolveWindow(*own.front()).scale;
    } else if (!plans.empty()) {
        legend.scale = resolveWindow(*plans.front()).scale;
    }

    Resolver resolve(model);
    std::vector<const Entity*> shown;
    if (used == LegendScope::WholeDrawing) {
        model.entities.forEach([&](const Entity& entity) {
            const Resolved& resolved = resolve(entity);
            if (entity.visible && resolved.layer.shown && kindOf(entity, resolved)) {
                shown.push_back(&entity);
            }
        });
    } else {
        // A symbol reaches past its point, so the index is asked for the
        // window grown by the furthest a style's symbol reaches.
        double reach = 0.0;
        model.styles.forEach([&reach](const katana::entity::Style& style) {
            if (!style.symbol.empty() && std::isfinite(style.symbolSize)) {
                reach = std::max(reach, 0.5 * style.symbolSize);
            }
        });
        std::vector<katana::geometry::SpatialId> scratch;
        for (const Viewport* plan : plans) {
            const std::optional<Window> window = windowOf(*plan, resolveWindow(*plan));
            if (!window) {
                continue;
            }
            ++legend.windows;
            detail::forEachCandidate(
                model, options.index, window->box.inflated(reach), scratch,
                [&](const Entity& entity) {
                    const Resolved& resolved = resolve(entity);
                    if (!entity.visible || !resolved.layer.shown ||
                        window->hidden->hides(entity.layer) || !kindOf(entity, resolved) ||
                        !shows(*window, entity, resolved)) {
                        return;
                    }
                    shown.push_back(&entity);
                });
        }
        // An entity two plans show is counted once.
        std::sort(shown.begin(), shown.end(),
                  [](const Entity* a, const Entity* b) { return a->id < b->id; });
        shown.erase(std::unique(shown.begin(), shown.end()), shown.end());
    }

    // The code library's words, when there is a library.
    std::string property = options.codeProperty;
    const bool coded = options.codes != nullptr && !options.codes->empty();
    if (coded && property.empty()) {
        std::vector<EntityId> ids;
        ids.reserve(shown.size());
        for (const Entity* entity : shown) {
            ids.push_back(entity->id);
        }
        property = findCodeProperty(model, ids);
    }
    CodeLabels labels(coded ? options.codes : nullptr, property);

    std::map<GroupKey, Group> groups;
    for (const Entity* entity : shown) {
        const Resolved& resolved = resolve(*entity);
        GroupKey key;
        key.kind = *kindOf(*entity, resolved);
        if (resolved.styled) {
            key.style = entity->style;
        } else {
            key.layer = entity->layer;
        }
        Group& group = groups[key];
        ++group.count;
        auto [look, fresh] = group.looks.try_emplace(lookOf(resolved.display));
        if (fresh) {
            look->second.firstId = entity->id;
            look->second.display = resolved.display;
        }
        ++look->second.count;
        if (labels.active()) {
            if (auto code = labels.of(*entity)) {
                group.codes.insert(code->first);
                if (code->second.empty()) {
                    group.undescribed = true;
                } else {
                    group.descriptions.insert(code->second);
                }
            }
        }
    }

    for (const auto& [key, group] : groups) {
        // The look most of them print with; a tie goes to the one met first.
        const Group::Seen* best = nullptr;
        for (const auto& [look, seen] : group.looks) {
            if (best == nullptr || seen.count > best->count ||
                (seen.count == best->count && seen.firstId < best->firstId)) {
                best = &seen;
            }
        }
        LegendEntry entry;
        entry.kind = key.kind;
        entry.style = key.style;
        entry.layer = key.layer;
        entry.label = key.style.empty() ? key.layer : key.style;
        if (!group.undescribed && group.descriptions.size() == 1) {
            entry.label = *group.descriptions.begin();
            entry.code = *group.codes.begin();
        }
        const katana::entity::ResolvedDisplay& display = best->display;
        entry.colour = display.color;
        entry.lineWeight = display.lineWeight;
        entry.linetype = display.linetype;
        entry.symbol = display.symbol;
        entry.symbolSize = display.symbolSize;
        entry.hatchPattern = display.hatchPattern;
        entry.count = group.count;
        legend.entries.push_back(std::move(entry));
    }
    std::sort(legend.entries.begin(), legend.entries.end(),
              [](const LegendEntry& a, const LegendEntry& b) {
                  const std::string fa = folded(a.label);
                  const std::string fb = folded(b.label);
                  return std::tie(a.kind, fa, a.label, a.style, a.layer) <
                         std::tie(b.kind, fb, b.label, b.style, b.layer);
              });
    return legend;
}

Result<Legend> legendFor(const Document& document, std::string_view viewportId)
{
    const SheetSet& set = document.sheetSet();
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        for (const Viewport& viewport : set.sheets[i].viewports) {
            if (viewport.id != viewportId) {
                continue;
            }
            if (viewport.kind != ViewportKind::Legend) {
                return makeError(ErrorCode::InvalidArgument, "the viewport is not a legend",
                                 viewport.id + " is a " + std::string(toString(viewport.kind)));
            }
            LegendOptions options;
            options.index = &document.spatialIndex();
            options.codes = &document.surveyMap();
            return computeLegend(document.model(), set, i, viewport.legendScope, options);
        }
    }
    return makeError(ErrorCode::NotFound, "no viewport with that id", std::string(viewportId));
}

Status setLegendScope(Document& document, std::string_view viewportId, LegendScope scope)
{
    return editViewport(
        document, viewportId,
        [scope](Viewport& viewport) -> Status {
            if (viewport.kind != ViewportKind::Legend) {
                return makeError(ErrorCode::InvalidArgument, "the viewport is not a legend",
                                 viewport.id + " is a " +
                                     std::string(toString(viewport.kind)));
            }
            viewport.legendScope = scope;
            return {};
        },
        "LEGEND_SCOPE");
}

std::string legendJson(const Legend& legend)
{
    using Json = nlohmann::json;
    Json entries = Json::array();
    for (const LegendEntry& entry : legend.entries) {
        entries.push_back(Json{{"kind", toString(entry.kind)},
                               {"label", entry.label},
                               {"style", entry.style},
                               {"layer", entry.layer},
                               {"code", entry.code},
                               {"colour", entry.colour.toHex()},
                               {"weight", entry.lineWeight},
                               {"linetype", entry.linetype},
                               {"symbol", entry.symbol},
                               {"symbol_size", entry.symbolSize},
                               {"hatch", entry.hatchPattern},
                               {"count", entry.count}});
    }
    const Json root{{"scope", toString(legend.scope)},
                    {"windows", legend.windows},
                    {"scale", legend.scale},
                    {"entries", std::move(entries)}};
    // Replaced rather than thrown on: the text is for reading, and a name
    // from an old file with a stray byte must not cost the whole answer.
    return root.dump(-1, ' ', false, Json::error_handler_t::replace);
}

// ---- the layout ------------------------------------------------------------------------------

LegendLayout layoutLegend(const Box2& rect, std::span<const double> labelWidths,
                          const LegendMetrics& metrics)
{
    LegendLayout layout;
    const std::size_t count = labelWidths.size();
    if (count == 0 || rect.empty()) {
        return layout;
    }
    const double left = rect.min.x + metrics.margin;
    const double top = rect.max.y - metrics.heading;
    const double width = rect.width() - 2.0 * metrics.margin;
    const double height = top - (rect.min.y + metrics.margin);
    if (!(width > 0.0) || !(height >= metrics.pitch)) {
        layout.more = count;
        return layout;
    }
    const auto rowsFit = static_cast<std::size_t>(std::floor(height / metrics.pitch + 1e-9));
    double widest = 0.0;
    for (const double w : labelWidths) {
        widest = std::max(widest, std::isfinite(w) ? w : 0.0);
    }
    const double natural = metrics.sampleWidth + metrics.labelGap + widest;
    layout.columnWidth = std::min(natural, width);
    const auto columnsFit = std::max<std::size_t>(
        1, static_cast<std::size_t>(std::floor((width + metrics.columnGap) /
                                               (layout.columnWidth + metrics.columnGap) +
                                               1e-9)));
    const std::size_t capacity = rowsFit * columnsFit;
    std::size_t shown = count;
    if (count > capacity) {
        // The last place says how many are left out.
        shown = capacity - 1;
        layout.more = count - shown;
    }
    // As few columns as hold them, the rows shared out evenly.
    const std::size_t places = shown + (layout.more > 0 ? 1 : 0);
    layout.columns = (places + rowsFit - 1) / rowsFit;
    layout.rows = (places + layout.columns - 1) / layout.columns;
    const auto placeOf = [&](std::size_t n) {
        const std::size_t column = n / layout.rows;
        const std::size_t row = n % layout.rows;
        const double x =
            left + static_cast<double>(column) * (layout.columnWidth + metrics.columnGap);
        const double y = top - (static_cast<double>(row) + 0.5) * metrics.pitch;
        return Point2(x, y);
    };
    for (std::size_t n = 0; n < shown; ++n) {
        const Point2 at = placeOf(n);
        LegendCell cell;
        cell.entry = n;
        cell.sample = Box2(Point2(at.x, at.y - 0.5 * metrics.sampleHeight),
                           Point2(at.x + metrics.sampleWidth, at.y + 0.5 * metrics.sampleHeight));
        cell.label = Point2(at.x + metrics.sampleWidth + metrics.labelGap, at.y);
        cell.labelRoom = std::max(
            layout.columnWidth - metrics.sampleWidth - metrics.labelGap, 0.0);
        layout.cells.push_back(cell);
    }
    if (layout.more > 0) {
        layout.moreAt = placeOf(shown);
    }
    return layout;
}

} // namespace katana::cad::plotting
