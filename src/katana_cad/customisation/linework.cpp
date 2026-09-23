#include "katana/cad/linework.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <set>
#include <utility>

#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace {

namespace cmd = katana::commands;
namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
using katana::geometry::Vec2;

// ---- reading the points ----------------------------------------------------------

// The code a point carries, or nothing. Text only, as applySurveyCodes reads
// it: a number in the code property is a measurement named badly, not a code.
[[nodiscard]] const std::string* codeOf(const Entity& entity, const std::string& property)
{
    const auto found = entity.properties.find(property);
    if (found == entity.properties.end()) {
        return nullptr;
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr && !text->empty() ? text : nullptr;
}

// The point number as text: survey import writes text ("102", "A12"), but a
// drawing from elsewhere may hold an integer, and both name the same point.
[[nodiscard]] std::string pointNumberOf(const Entity& entity, const std::string& property)
{
    const auto found = entity.properties.find(property);
    if (found == entity.properties.end()) {
        return {};
    }
    if (const auto* text = std::get_if<std::string>(&found->second)) {
        return std::string(katana::core::trimmed(*text));
    }
    if (std::holds_alternative<bool>(found->second)) {
        return {}; // a flag is not a number
    }
    return katana::entity::toString(found->second);
}

// How a point number sorts: numbers by value, so "9" comes before "10", and
// before every number that is not one, which sort by their text.
struct NumberKey {
    bool numeric = false;
    double value = 0.0;
    std::string text{};

    friend bool operator<(const NumberKey& a, const NumberKey& b)
    {
        if (a.numeric != b.numeric) {
            return a.numeric;
        }
        return a.numeric ? a.value < b.value : a.text < b.text;
    }
};

[[nodiscard]] NumberKey numberKey(const std::string& number)
{
    NumberKey key;
    key.text = number;
    if (const auto value = katana::core::parseFiniteDouble(number)) {
        key.numeric = true;
        key.value = *value;
    }
    return key;
}

// A point that is going into a line.
struct Candidate {
    EntityId id = katana::entity::kInvalidEntityId;
    Point2 at{};
    std::optional<double> height{};
    std::string number{};
    std::string codeText{}; // the code as the point carries it
    FieldCode code{};
    std::string layer{};
};

// A stretch of one string between its start and its end.
struct Run {
    std::vector<const Candidate*> points{};
    bool closed = false;
    bool rectangle = false;
};

// The polyline a run becomes, before it is an entity.
struct Shape {
    std::vector<Point2> vertices{};
    std::vector<std::optional<double>> heights{};
    bool closed = false;
    bool rectangle = false;
    std::size_t curves = 0;

    void add(const Point2& at, std::optional<double> height)
    {
        vertices.push_back(at);
        heights.push_back(height);
    }
};

// ---- curves ----------------------------------------------------------------------

// Appends the arc of `arc`'s circle from `from` to `to`, travelling in
// `arc`'s direction, as chords no further than `tolerance` from it. `from` is
// already in the shape; `to` is appended exactly, so a surveyed point stays a
// vertex where it was measured.
//
// The chord of angle t on radius r stands off the arc by its sagitta,
// r(1 - cos(t/2)); keeping that within the tolerance gives the largest step
// t = 2 acos(1 - tolerance / r). Heights between two surveyed heights are
// interpolated along the arc, which is what a kerb or a channel between two
// shots is assumed to do; with either end unsurveyed the chords have none.
void appendArc(Shape& shape, const katana::geometry::Arc2& arc, const Candidate& from,
               const Candidate& to, double tolerance)
{
    const double startAngle = (from.at - arc.center).angle();
    const double endAngle = (to.at - arc.center).angle();
    const double delta = arc.sweep >= 0.0 ? katana::math::normalizeAngle(endAngle - startAngle)
                                          : -katana::math::normalizeAngle(startAngle - endAngle);
    const double ratio = std::max(-1.0, 1.0 - tolerance / arc.radius);
    const double step = 2.0 * std::acos(ratio);
    const auto chords =
        static_cast<std::size_t>(std::max(1.0, std::ceil(std::abs(delta) / step)));
    for (std::size_t i = 1; i < chords; ++i) {
        const double t = static_cast<double>(i) / static_cast<double>(chords);
        std::optional<double> height{};
        if (from.height && to.height) {
            height = katana::math::lerp(*from.height, *to.height, t);
        }
        shape.add(arc.pointAtAngle(startAngle + delta * t), height);
    }
    shape.add(to.at, to.height);
}

struct NoteSink {
    std::vector<LineworkNote>& notes;

    void operator()(const Candidate& at, LineworkNoteKind kind, std::string detail = {}) const
    {
        notes.push_back(LineworkNote{at.id, at.number, kind, std::move(detail)});
    }
};

// Appends the curve through points[first..last] (both in the shape's run;
// points[first] already added).
void appendCurve(Shape& shape, const std::vector<const Candidate*>& points, std::size_t first,
                 std::size_t last, double tolerance, const NoteSink& note)
{
    const std::size_t count = last - first; // segments in the curve
    if (count < 2) {
        // Two points are on every circle through them: there is no curve to
        // draw, only the straight line the surveyor may not have meant.
        note(*points[first], LineworkNoteKind::CurveTooShort);
        shape.add(points[last]->at, points[last]->height);
        return;
    }
    // Each consecutive three points define an arc; a segment left over at the
    // end lies on the arc through the last three. The curve counts as one
    // only if some piece of it was chorded: points in a straight line are
    // joined straight, and the report must not call that a curve.
    bool curved = false;
    auto piece = [&](std::size_t a, std::size_t b, std::size_t c, std::size_t segmentFrom) {
        const auto arc =
            katana::geometry::Arc2::throughPoints(points[a]->at, points[b]->at, points[c]->at);
        for (std::size_t i = segmentFrom; i < c; ++i) {
            if (!arc) {
                shape.add(points[i + 1]->at, points[i + 1]->height);
            } else {
                appendArc(shape, *arc, *points[i], *points[i + 1], tolerance);
            }
        }
        if (!arc) {
            note(*points[a], LineworkNoteKind::CurveCollinear);
        } else {
            curved = true;
        }
    };
    std::size_t k = first;
    while (k < last) {
        if (k + 2 <= last) {
            piece(k, k + 1, k + 2, k);
            k += 2;
        } else {
            piece(last - 2, last - 1, last, last - 1);
            k = last;
        }
    }
    if (curved) {
        ++shape.curves;
    }
}

// The polyline for one run, with its curves, its closure or its rectangle.
[[nodiscard]] Shape shapeOf(const Run& run, double tolerance, const NoteSink& note)
{
    Shape shape;
    const auto& points = run.points;
    shape.closed = run.closed;

    if (run.rectangle) {
        if (points.size() == 3) {
            // Side p0-p1, reaching p2's side of it by p2's distance from it.
            const Vec2 side = points[1]->at - points[0]->at;
            const double length = side.length();
            const Vec2 normal =
                length > tol::kGeometric ? side.perpendicular() / length : Vec2(0.0, 0.0);
            const double width = normal.dot(points[2]->at - points[0]->at);
            if (std::abs(width) > tol::kGeometric) {
                shape.rectangle = true;
                shape.closed = true;
                shape.add(points[0]->at, points[0]->height);
                shape.add(points[1]->at, points[1]->height);
                // Constructed, not surveyed: no height rather than a guess.
                shape.add(points[1]->at + normal * width, std::nullopt);
                shape.add(points[0]->at + normal * width, std::nullopt);
                return shape;
            }
            // The third point is on the first side: a line, not an area.
            note(*points.back(), LineworkNoteKind::RectangleShape, "no width: drawn open");
            shape.closed = false;
        } else {
            // Not three points, so there is no rectangle to construct; the
            // points themselves are the best account of the area, closed
            // when they can enclose one. The detail says which, because the
            // note is what tells the surveyor what became of their RECT.
            shape.closed = points.size() >= 3;
            note(*points.back(), LineworkNoteKind::RectangleShape,
                 std::to_string(points.size()) +
                     (shape.closed ? " points: drawn closed through them" : " points: drawn open"));
        }
    }
    if (shape.closed && points.size() < 3) {
        // Two points closed back on themselves enclose nothing.
        note(*points.back(), LineworkNoteKind::CloseTooShort);
        shape.closed = false;
    }

    // Where the curves are: from a point carrying arcStart to the next
    // carrying arcEnd. A point carrying both ends one and begins the next.
    std::vector<std::pair<std::size_t, std::size_t>> curves;
    std::optional<std::size_t> open;
    for (std::size_t i = 0; i < points.size(); ++i) {
        const FieldCode& code = points[i]->code;
        if (code.has(LineworkControl::ArcEnd)) {
            if (open) {
                curves.emplace_back(*open, i);
                open.reset();
            } else {
                note(*points[i], LineworkNoteKind::CurveEndWithoutStart);
            }
        }
        if (code.has(LineworkControl::ArcStart)) {
            if (open) { // a new curve begins where the last one ends
                curves.emplace_back(*open, i);
            }
            open = i;
        }
    }
    if (open) {
        // Taken to end at the string's last point, as the likeliest place the
        // surveyor meant; begun ON that point, there is nothing to curve.
        if (*open + 1 < points.size()) {
            note(*points[*open], LineworkNoteKind::CurveUnterminated,
                 "taken to end at the string's last point");
            curves.emplace_back(*open, points.size() - 1);
        } else {
            note(*points[*open], LineworkNoteKind::CurveUnterminated,
                 "begun on the string's last point: nothing curved");
        }
    }

    shape.add(points.front()->at, points.front()->height);
    std::size_t i = 0;
    auto curve = curves.begin();
    while (i + 1 < points.size()) {
        if (curve != curves.end() && curve->first == i) {
            appendCurve(shape, points, curve->first, curve->second, tolerance, note);
            i = curve->second;
            ++curve;
            continue;
        }
        shape.add(points[i + 1]->at, points[i + 1]->height);
        ++i;
    }
    return shape;
}

// Splits one string's points, in order, into runs at its start, end, close
// and rectangle codes.
[[nodiscard]] std::vector<Run> runsOf(const std::vector<Candidate>& ordered)
{
    std::vector<Run> runs;
    std::optional<Run> current;
    auto finish = [&] {
        if (current) {
            runs.push_back(std::move(*current));
            current.reset();
        }
    };
    for (const Candidate& point : ordered) {
        if (point.code.has(LineworkControl::Start)) {
            finish();
        }
        if (!current) {
            current.emplace();
        }
        current->points.push_back(&point);
        if (point.code.has(LineworkControl::Rectangle)) {
            current->rectangle = true;
            finish();
        } else if (point.code.has(LineworkControl::Close)) {
            current->closed = true;
            finish();
        } else if (point.code.has(LineworkControl::End)) {
            finish();
        }
    }
    finish();
    return runs;
}

// ---- styling, once the lines exist -----------------------------------------------

// Styles the lines another part of the same transaction has just created, by
// planning applySurveyCodes against the document at that moment - the only
// moment the lines exist to be planned for. Redo replays what was planned, so
// the ids and styles come back exactly.
class StyleLinesCommand final : public cmd::Command {
  public:
    StyleLinesCommand(const Document& document, const cmd::Command& lines,
                      SurveyCodingOptions options, std::shared_ptr<SurveyCodingReport> report)
        : document_(document), lines_(lines), options_(std::move(options)),
          report_(std::move(report))
    {
    }

    [[nodiscard]] std::string_view name() const override { return "STYLE_LINEWORK"; }

    [[nodiscard]] Status validate(const cmd::CommandContext& context) const override
    {
        // It plans against its Document; run on another model it would style
        // whatever that document's entities of the same ids are.
        if (&context.model != &document_.model()) {
            return makeError(ErrorCode::InvalidState,
                             "linework was planned for another document");
        }
        return {};
    }

    [[nodiscard]] Status execute(cmd::CommandContext& context) override
    {
        options_.ids = lines_.createdEntities();
        // An empty id list means EVERY entity to applySurveyCodes; with no
        // lines there is nothing to style, not everything.
        if (options_.ids.empty()) {
            return {};
        }
        auto planned = applySurveyCodes(document_, options_, report_.get());
        if (!planned) {
            return planned.error();
        }
        inner_ = std::move(*planned);
        if (!inner_) {
            return {};
        }
        if (auto status = inner_->validate(context); !status) {
            return status;
        }
        return inner_->execute(context);
    }

    [[nodiscard]] Status undo(cmd::CommandContext& context) override
    {
        return inner_ ? inner_->undo(context) : Status{};
    }

    [[nodiscard]] Status redo(cmd::CommandContext& context) override
    {
        return inner_ ? inner_->redo(context) : Status{};
    }

  private:
    const Document& document_;
    const cmd::Command& lines_; // owned by the same transaction, executed before this
    SurveyCodingOptions options_;
    std::shared_ptr<SurveyCodingReport> report_;
    cmd::CommandPtr inner_{};
};

// The code property to read when none is named: the first candidate any of
// these points carries, as applySurveyCodes chooses among all entities.
[[nodiscard]] std::string findCodeProperty(const std::vector<const Entity*>& points)
{
    for (const std::string& candidate : codePropertyCandidates()) {
        for (const Entity* point : points) {
            if (codeOf(*point, candidate) != nullptr) {
                return candidate;
            }
        }
    }
    return codePropertyCandidates().front();
}

// The point as a candidate for a line: where it is, how high, its number.
[[nodiscard]] Candidate candidateOf(const Entity& entity, const std::string& numberProperty)
{
    Candidate candidate;
    candidate.id = entity.id;
    candidate.at = std::get<katana::entity::PointGeometry>(entity.geometry).position;
    candidate.height = katana::entity::heightsOf(entity.properties, 1).front();
    candidate.number = pointNumberOf(entity, numberProperty);
    candidate.layer = entity.layer;
    return candidate;
}

// Every point in the drawing by its number. Several under one number are kept
// as several, so that a caller can refuse to guess between them.
[[nodiscard]] std::map<std::string, std::vector<const Entity*>>
pointsByNumber(const katana::entity::Model& model, const std::string& numberProperty)
{
    std::map<std::string, std::vector<const Entity*>> byNumber;
    model.entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry)) {
            const std::string number = pointNumberOf(entity, numberProperty);
            if (!number.empty()) {
                byNumber[number].push_back(&entity);
            }
        }
    });
    return byNumber;
}

// What joining coded points and drawing survey features share: the layer a
// line goes on, the line itself and its entry in the report, and the ONE
// command that builds them all.
class LineBuilder {
  public:
    // An empty `property` means the first of codePropertyCandidates(), as it
    // does to applySurveyCodes when no entity carries one: a line needs its
    // code under SOME name, because that is what the rules style it by, and a
    // property with an empty name is one the rest of Katana refuses. The name
    // used is the report's `property`, so the two cannot disagree.
    LineBuilder(const Document& document, const std::string& property, bool createLayers,
                LineworkReport& report)
        : document_(document),
          property_(property.empty() ? codePropertyCandidates().front() : property),
          createLayers_(createLayers), report_(report)
    {
        report_.property = property_;
    }

    // 12d's model is Katana's layer. With none - or one that is missing and
    // may not be created - the line stays with its points, on `fallback`.
    // Deciding is not creating: a layer is created only if a line goes on it.
    [[nodiscard]] katana::core::Result<std::string>
    layerFor(const katana::entity::SurveyMatch& match, const std::string& code,
             const std::string& fallback) const
    {
        const std::string& model = match.resolved.model;
        if (model.empty()) {
            return fallback;
        }
        if (auto status = katana::entity::validateLayerPath(model); !status) {
            return makeError(ErrorCode::InvalidArgument,
                             "the survey map gives a code a model that is not a valid layer name",
                             "code=" + code + " model=" + model + " " + status.error().context);
        }
        if (!document_.model().layers.contains(model) && !createLayers_) {
            return fallback;
        }
        return model;
    }

    // `code` is what the line carries in the code property, so that
    // applySurveyCodes - now, and whenever it is applied again - codes the
    // line as its points were coded. `texts` are further text properties
    // (a survey feature's description); an empty value is not written.
    void emit(LineworkString built, const std::string& code, const Shape& shape,
              const std::vector<const Candidate*>& members,
              const std::vector<std::pair<std::string, std::string>>& texts = {})
    {
        Entity entity;
        entity.geometry = katana::geometry::Polyline2{shape.vertices, shape.closed};
        entity.layer = built.layer;
        // An empty code is no code (codeOf reads it so), not a code "".
        if (!code.empty()) {
            entity.properties.insert_or_assign(property_, PropertyValue(code));
        }
        for (const auto& [key, value] : texts) {
            if (!key.empty() && !value.empty()) {
                entity.properties.insert_or_assign(key, PropertyValue(value));
            }
        }
        katana::entity::setHeights(entity.properties, shape.heights);
        lines_.push_back(std::move(entity));
        if (!document_.model().layers.contains(built.layer)) {
            layersNeeded_.insert(built.layer);
        }

        built.closed = shape.closed;
        built.rectangle = shape.rectangle;
        built.curves = shape.curves;
        built.vertices = shape.vertices.size();
        for (const Candidate* member : members) {
            // A survey feature's points are the project's, not entities.
            if (member->id != katana::entity::kInvalidEntityId) {
                built.points.push_back(member->id);
                placed_.insert(member->id);
                if (!built.join) {
                    runMembers_.insert(member->id);
                }
            }
            built.pointNumbers.push_back(member->number);
        }
        report_.strings.push_back(std::move(built));
    }

    [[nodiscard]] const std::set<EntityId>& placed() const { return placed_; }

    // The points some run of their own string is drawn through - the only
    // points a line stands in for. A point reached only by a join (a tree, an
    // uncoded control point) is in a line but is not replaced by one: its own
    // code, number and symbol are still only on it.
    [[nodiscard]] const std::set<EntityId>& runMembers() const { return runMembers_; }

    // Layers, lines, their styling and the removal of `remove`, as one
    // command; nullptr when no line was built.
    [[nodiscard]] cmd::CommandPtr finish(std::string name, SurveyCodingOptions coding,
                                         std::vector<EntityId> remove)
    {
        // A point a line reached is in a line, whatever its own run made of
        // it: a lone point, or a point code, that a join reached.
        std::erase_if(report_.unplaced,
                      [&](const UnplacedPoint& point) { return placed_.contains(point.id); });
        std::stable_sort(report_.unplaced.begin(), report_.unplaced.end(),
                         [](const UnplacedPoint& a, const UnplacedPoint& b) { return a.id < b.id; });
        if (lines_.empty()) {
            return {};
        }
        report_.layersCreated.assign(layersNeeded_.begin(), layersNeeded_.end());

        auto transaction = std::make_unique<cmd::Transaction>(std::move(name));
        for (const std::string& layerName : report_.layersCreated) {
            katana::entity::Layer layer;
            layer.name = layerName;
            transaction->add(cmd::createLayer(std::move(layer)));
        }
        cmd::CommandPtr create = cmd::createEntities(std::move(lines_));
        const cmd::Command& created = *create;
        transaction->add(std::move(create));

        auto styling = std::make_shared<SurveyCodingReport>();
        report_.styling = styling;
        coding.property = property_;
        transaction->add(std::make_unique<StyleLinesCommand>(document_, created,
                                                             std::move(coding), std::move(styling)));

        report_.pointsRemoved = remove.size();
        if (!remove.empty()) {
            transaction->add(cmd::deleteEntities(std::move(remove)));
        }
        return transaction;
    }

  private:
    const Document& document_;
    std::string property_;
    bool createLayers_ = true;
    LineworkReport& report_;
    std::vector<Entity> lines_{};
    std::set<EntityId> placed_{};
    std::set<EntityId> runMembers_{};
    std::set<std::string> layersNeeded_{};
};

} // namespace

// ---- string names ------------------------------------------------------------------

StringName splitStringName(const katana::entity::SurveyMap& map, std::string_view name)
{
    StringName split;
    const auto rules = map.match(name);
    if (rules.empty()) {
        return split;
    }
    split.matched = true;
    const std::string& key = rules.front()->key; // the most specific
    if (key == "*") {
        split.key = std::string(name);
        split.fallbackOnly = true;
        return split;
    }
    split.key = key;
    if (key.back() == '*') {
        split.number = std::string(name.substr(key.size() - 1));
    }
    return split;
}

// ---- control codes --------------------------------------------------------------

namespace {

[[nodiscard]] std::vector<std::pair<LineworkControl, const std::string*>>
spellings(const LineworkCodes& codes)
{
    return {{LineworkControl::Start, &codes.start},       {LineworkControl::End, &codes.end},
            {LineworkControl::Close, &codes.close},       {LineworkControl::ArcStart, &codes.arcStart},
            {LineworkControl::ArcEnd, &codes.arcEnd},     {LineworkControl::Join, &codes.join},
            {LineworkControl::Rectangle, &codes.rectangle}};
}

} // namespace

Status validate(const LineworkCodes& codes)
{
    const auto all = spellings(codes);
    for (std::size_t i = 0; i < all.size(); ++i) {
        const std::string& spelling = *all[i].second;
        if (std::any_of(spelling.begin(), spelling.end(), katana::core::isAsciiSpace)) {
            return makeError(ErrorCode::InvalidArgument,
                             "a linework control code cannot contain a blank: codes are split "
                             "on blanks, so it could never be matched",
                             std::string(toString(all[i].first)) + "=\"" + spelling + "\"");
        }
        for (std::size_t j = i + 1; j < all.size() && !spelling.empty(); ++j) {
            if (katana::core::equalsIgnoringCase(spelling, *all[j].second)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "two linework controls are spelled alike, so the token would "
                                 "mean both",
                                 std::string(toString(all[i].first)) + " and " +
                                     std::string(toString(all[j].first)) + " are \"" +
                                     spelling + "\"");
            }
        }
    }
    return {};
}

std::string_view toString(LineworkControl control)
{
    switch (control) {
    case LineworkControl::Start:
        return "start";
    case LineworkControl::End:
        return "end";
    case LineworkControl::Close:
        return "close";
    case LineworkControl::ArcStart:
        return "arcStart";
    case LineworkControl::ArcEnd:
        return "arcEnd";
    case LineworkControl::Join:
        return "join";
    case LineworkControl::Rectangle:
        return "rectangle";
    }
    return "unknown";
}

bool FieldCode::has(LineworkControl control) const
{
    return std::find(controls.begin(), controls.end(), control) != controls.end();
}

FieldCode parseFieldCode(std::string_view code, const LineworkCodes& codes)
{
    std::vector<std::string_view> tokens;
    std::size_t at = 0;
    while (at < code.size()) {
        while (at < code.size() && katana::core::isAsciiSpace(code[at])) {
            ++at;
        }
        const std::size_t begin = at;
        while (at < code.size() && !katana::core::isAsciiSpace(code[at])) {
            ++at;
        }
        if (at > begin) {
            tokens.push_back(code.substr(begin, at - begin));
        }
    }

    FieldCode parsed;
    if (tokens.empty()) {
        return parsed;
    }
    parsed.name = std::string(tokens.front());
    const auto all = spellings(codes);
    for (std::size_t i = 1; i < tokens.size(); ++i) {
        const auto control =
            std::find_if(all.begin(), all.end(), [&](const auto& entry) {
                return !entry.second->empty() &&
                       katana::core::equalsIgnoringCase(tokens[i], *entry.second);
            });
        if (control == all.end()) {
            parsed.unknownTokens.emplace_back(tokens[i]);
            continue;
        }
        parsed.controls.push_back(control->first);
        if (control->first == LineworkControl::Join) {
            // The next token is the point to join to, unless it is itself a
            // control - "JPN CL" is a join with its number forgotten, not a
            // join to a point called CL.
            const bool next = i + 1 < tokens.size() &&
                              std::none_of(all.begin(), all.end(), [&](const auto& entry) {
                                  return !entry.second->empty() &&
                                         katana::core::equalsIgnoringCase(tokens[i + 1],
                                                                          *entry.second);
                              });
            if (next) {
                parsed.joinTo = std::string(tokens[++i]);
            } else {
                parsed.joinWithoutTarget = true;
            }
        }
    }
    return parsed;
}

// ---- report words ----------------------------------------------------------------

std::string_view toString(UnplacedReason reason)
{
    switch (reason) {
    case UnplacedReason::NoCode:
        return "no code";
    case UnplacedReason::NoRule:
        return "no rule for its code and no control code";
    case UnplacedReason::PointCode:
        return "its code is a point code";
    case UnplacedReason::NoPointNumber:
        return "no point number to order it by";
    case UnplacedReason::LonePoint:
        return "the only point of its string";
    case UnplacedReason::Coincident:
        return "every point of its string is in one place";
    }
    return "unknown";
}

std::string_view toString(LineworkNoteKind kind)
{
    switch (kind) {
    case LineworkNoteKind::UnknownToken:
        return "unknown token";
    case LineworkNoteKind::JoinWithoutTarget:
        return "join with no point number";
    case LineworkNoteKind::JoinTargetMissing:
        return "join target not found";
    case LineworkNoteKind::CurveTooShort:
        return "curve with no point between its ends, drawn straight";
    case LineworkNoteKind::CurveCollinear:
        return "curve points in a straight line, drawn straight";
    case LineworkNoteKind::CurveUnterminated:
        return "curve never ended"; // the detail says what was drawn instead
    case LineworkNoteKind::CurveEndWithoutStart:
        return "curve end with no curve begun";
    case LineworkNoteKind::CloseTooShort:
        return "closing a string of two points, drawn open";
    case LineworkNoteKind::RectangleShape:
        // Whether the points were then drawn open or closed depends on how
        // many there were, so the detail says which.
        return "a rectangle needs three points with width: none constructed";
    case LineworkNoteKind::DuplicatePointNumber:
        return "two points of one string share a number";
    case LineworkNoteKind::NoRuleForName:
        // processLinework meets this only for a line made by a control code;
        // drawSurveyFeatures for a feature whose code no rule knows.
        return "no rule for its code: nothing styles the line";
    case LineworkNoteKind::FallbackOnlyName:
        return "a line only because the fallback rule \"*\" says so";
    case LineworkNoteKind::UnpositionedPoint:
        return "a point with no coordinates, left out of the line";
    }
    return "unknown";
}

std::string_view toString(UnplacedFeatureReason reason)
{
    switch (reason) {
    case UnplacedFeatureReason::TooFewPoints:
        return "fewer than two of its points have a position";
    case UnplacedFeatureReason::Coincident:
        return "its positioned points are all in one place";
    }
    return "unknown";
}

// ---- processing ------------------------------------------------------------------

katana::core::Result<LineworkResult> processLinework(const Document& document,
                                                     const LineworkOptions& options)
{
    if (auto status = validate(options.codes); !status) {
        return status.error();
    }
    if (!(std::isfinite(options.chordTolerance) && options.chordTolerance > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the chord tolerance must be a positive distance",
                         // Exact, and the same in every locale: std::to_string
                         // would print 1e-9 as "0.000000".
                         "chordTolerance=" +
                             katana::core::formatExactReal(options.chordTolerance));
    }

    const katana::entity::Model& model = document.model();
    const katana::entity::SurveyMap& map = document.surveyMap();
    LineworkResult result;
    LineworkReport& report = result.report;

    // In entity order whatever order they were given in: that IS the entity
    // order a string may be joined in, and it makes the report deterministic.
    std::vector<EntityId> subject = options.ids;
    if (subject.empty()) {
        subject = model.entities.ids();
    }
    std::sort(subject.begin(), subject.end());
    subject.erase(std::unique(subject.begin(), subject.end()), subject.end());

    std::vector<const Entity*> points;
    for (const EntityId id : subject) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr ||
            !std::holds_alternative<katana::entity::PointGeometry>(entity->geometry)) {
            ++report.notPoints;
            continue;
        }
        points.push_back(entity);
    }
    const std::string property =
        options.property.empty() ? findCodeProperty(points) : options.property;
    NoteSink note{report.notes};

    // Resolved once per name rather than once per point: a survey of 30,000
    // points has a few hundred names, and lookup walks every rule.
    std::map<std::string, katana::entity::SurveyMatch> resolved;
    auto resolve = [&](const std::string& name) -> const katana::entity::SurveyMatch& {
        auto found = resolved.find(name);
        if (found == resolved.end()) {
            found = resolved.emplace(name, map.lookup(name)).first;
        }
        return found->second;
    };

    std::map<std::string, std::vector<Candidate>> strings; // by name
    for (const Entity* entity : points) {
        ++report.considered;
        Candidate candidate = candidateOf(*entity, options.pointNumberProperty);
        const std::string* code = codeOf(*entity, property);
        candidate.codeText = code != nullptr ? *code : std::string{};
        candidate.code = parseFieldCode(candidate.codeText, options.codes);
        auto unplace = [&](UnplacedReason reason) {
            report.unplaced.push_back(
                UnplacedPoint{entity->id, candidate.number, candidate.codeText, reason});
        };
        if (candidate.code.name.empty()) {
            unplace(UnplacedReason::NoCode);
            continue;
        }
        for (const std::string& token : candidate.code.unknownTokens) {
            note(candidate, LineworkNoteKind::UnknownToken, token);
        }
        if (candidate.code.joinWithoutTarget) {
            note(candidate, LineworkNoteKind::JoinWithoutTarget);
        }

        const katana::entity::SurveyMatch& match = resolve(candidate.code.name);
        const bool line = match.resolved.breakline == katana::entity::SurveyBreakline::Line ||
                          !candidate.code.controls.empty();
        if (!line) {
            unplace(match.empty() ? UnplacedReason::NoRule : UnplacedReason::PointCode);
            continue;
        }
        if (options.order == LineworkOrder::PointNumber && candidate.number.empty()) {
            unplace(UnplacedReason::NoPointNumber);
            continue;
        }
        strings[candidate.code.name].push_back(std::move(candidate));
    }

    // Every point in the drawing by number, for joins: a join may reach a
    // point outside the selection, because the code says so.
    const bool anyJoin = std::any_of(strings.begin(), strings.end(), [](const auto& entry) {
        return std::any_of(entry.second.begin(), entry.second.end(),
                           [](const Candidate& c) { return !c.code.joinTo.empty(); });
    });
    const auto byNumber = anyJoin ? pointsByNumber(model, options.pointNumberProperty)
                                  : std::map<std::string, std::vector<const Entity*>>{};

    LineBuilder builder(document, property, options.coding.createLayers, report);
    auto unplace = [&](const Candidate& point, UnplacedReason reason) {
        report.unplaced.push_back(UnplacedPoint{point.id, point.number, point.codeText, reason});
    };

    for (auto& [name, candidates] : strings) {
        if (options.order == LineworkOrder::PointNumber) {
            // Stable, so equal numbers keep entity order.
            std::stable_sort(candidates.begin(), candidates.end(),
                             [](const Candidate& a, const Candidate& b) {
                                 return numberKey(a.number) < numberKey(b.number);
                             });
            for (std::size_t i = 1; i < candidates.size(); ++i) {
                if (candidates[i].number == candidates[i - 1].number) {
                    note(candidates[i], LineworkNoteKind::DuplicatePointNumber,
                         candidates[i].number);
                }
            }
        }

        const StringName split = splitStringName(map, name);
        if (!split.matched) {
            note(candidates.front(), LineworkNoteKind::NoRuleForName, name);
        } else if (split.fallbackOnly) {
            note(candidates.front(), LineworkNoteKind::FallbackOnlyName, name);
        }
        auto layer = builder.layerFor(resolve(name), name, candidates.front().layer);
        if (!layer) {
            return layer.error();
        }
        LineworkString header;
        header.name = name;
        header.key = split.key;
        header.number = split.number;
        header.layer = *layer;

        for (const Run& run : runsOf(candidates)) {
            if (run.points.size() < 2) {
                unplace(*run.points.front(), UnplacedReason::LonePoint);
                continue;
            }
            const Shape shape = shapeOf(run, options.chordTolerance, note);
            if (!(katana::geometry::Polyline2{shape.vertices, shape.closed}.length() >
                  tol::kGeometric)) {
                for (const Candidate* member : run.points) {
                    unplace(*member, UnplacedReason::Coincident);
                }
                continue;
            }
            builder.emit(header, name, shape, run.points);
        }

        for (const Candidate& from : candidates) {
            if (from.code.joinTo.empty()) {
                continue;
            }
            const auto targets = byNumber.find(from.code.joinTo);
            if (targets == byNumber.end() || targets->second.size() != 1 ||
                targets->second.front()->id == from.id) {
                note(from, LineworkNoteKind::JoinTargetMissing, from.code.joinTo);
                continue;
            }
            const Candidate to =
                candidateOf(*targets->second.front(), options.pointNumberProperty);
            if (!(from.at.distanceTo(to.at) > tol::kGeometric)) {
                note(from, LineworkNoteKind::JoinTargetMissing,
                     from.code.joinTo + " is in the same place");
                continue;
            }
            Shape shape;
            shape.add(from.at, from.height);
            shape.add(to.at, to.height);
            LineworkString joined = header;
            joined.join = true;
            builder.emit(std::move(joined), name, shape, {&from, &to});
        }
    }

    std::vector<EntityId> remove;
    if (!options.keepPoints) {
        // Only the points a run of their own string replaced. A join target
        // no run placed is kept, in the selection or out of it: the join line
        // does not carry its code, its number or its symbol. Runs are built
        // from the selection alone, so this never reaches outside it.
        const auto& members = builder.runMembers();
        remove.assign(members.begin(), members.end());
    }
    // nullptr when there is nothing to build, and no error.
    result.command = builder.finish("PROCESS_LINEWORK", options.coding, std::move(remove));
    return result;
}

// ---- survey features ---------------------------------------------------------------

katana::core::Result<SurveyFeatureResult>
drawSurveyFeatures(const Document& document, const katana::survey::SurveyProject& project,
                   const SurveyFeatureOptions& options)
{
    // Asked of the survey layer, as importSurveyProject asks it, so the two
    // cannot disagree about a valid project. It also guarantees what the loop
    // relies on: every point a feature names is in one of the two lists, and
    // every coordinate is finite.
    if (auto status = katana::survey::validateProject(project); !status) {
        return status.error();
    }

    const katana::entity::SurveyMap& map = document.surveyMap();
    const SurveyImportOptions& import = options.import;
    SurveyFeatureResult result;
    LineworkReport& report = result.report;
    NoteSink note{report.notes};

    std::map<std::string_view, const katana::survey::SurveyPoint*> positioned;
    for (const auto& point : project.points) {
        positioned.emplace(point.id, &point);
    }

    // An import that wrote no code (an empty import.codeProperty) still
    // gives the LINES theirs, under the first candidate name - see
    // LineBuilder - or nothing would style them, and nothing would say so.
    LineBuilder builder(document, import.codeProperty, options.coding.createLayers, report);
    for (std::size_t index = 0; index < project.features.size(); ++index) {
        const katana::survey::SurveyFeature& feature = project.features[index];
        const std::string& name = feature.name.empty() ? feature.code : feature.name;

        // The feature's points as candidates, in the file's order. They carry
        // no control codes: a feature says its closure itself, and shapeOf
        // then only joins them straight and checks the closure.
        std::vector<Candidate> candidates;
        std::vector<const katana::survey::SurveyPoint*> sources;
        for (const std::string& id : feature.pointIds) {
            const auto found = positioned.find(id);
            if (found == positioned.end()) {
                // validateProject put it in unpositionedPoints.
                Candidate missing;
                missing.number = id;
                note(missing, LineworkNoteKind::UnpositionedPoint, name);
                continue;
            }
            const katana::survey::SurveyPoint& point = *found->second;
            Candidate candidate;
            candidate.number = point.id;
            // Easting first, as importSurveyProject draws a point: a CAD x is
            // an easting.
            candidate.at = Point2(point.easting, point.northing);
            candidate.height = point.elevation;
            candidate.codeText = feature.code;
            candidates.push_back(std::move(candidate));
            sources.push_back(&point);
        }
        auto unplace = [&](UnplacedFeatureReason reason) {
            result.unplaced.push_back(UnplacedFeature{index, name, feature.code, reason});
        };
        if (candidates.size() < 2) {
            unplace(UnplacedFeatureReason::TooFewPoints);
            continue;
        }

        Run run;
        run.closed = feature.closed;
        for (const Candidate& candidate : candidates) {
            run.points.push_back(&candidate);
        }
        const Shape shape = shapeOf(run, 1.0, note); // straight only: no tolerance used
        if (!(katana::geometry::Polyline2{shape.vertices, shape.closed}.length() >
              tol::kGeometric)) {
            unplace(UnplacedFeatureReason::Coincident);
            continue;
        }

        // The rules are keyed on the CODE, and the line carries the code, so
        // the code is what is split and resolved - the name may be anything
        // the file calls the string ("Kerb 1").
        const StringName split = splitStringName(map, feature.code);
        if (!split.matched) {
            note(candidates.front(), LineworkNoteKind::NoRuleForName, feature.code);
        } else if (split.fallbackOnly) {
            note(candidates.front(), LineworkNoteKind::FallbackOnlyName, feature.code);
        }
        // With its points when no rule names a model: the layer the import
        // put the first of them on.
        const std::string pointsLayer = layerForPoint(*sources.front(), import);
        if (auto status = katana::entity::validateLayerPath(pointsLayer); !status) {
            return makeError(ErrorCode::InvalidArgument,
                             "the layer the import options give this feature's points is not "
                             "a valid layer name",
                             "feature=" + name + " layer=" + pointsLayer + " " +
                                 status.error().context);
        }
        auto layer = builder.layerFor(map.lookup(feature.code), feature.code, pointsLayer);
        if (!layer) {
            return layer.error();
        }
        if (!document.model().layers.contains(*layer) && !options.coding.createLayers) {
            return makeError(ErrorCode::NotFound,
                             "the drawing has no layer for this feature and layers may not "
                             "be created",
                             "feature=" + name + " layer=" + *layer);
        }

        LineworkString built;
        built.name = name;
        built.key = split.key;
        built.number = split.number;
        built.layer = *layer;
        builder.emit(std::move(built), feature.code, shape, run.points,
                     {{import.descriptionProperty, feature.description}});
    }

    result.command = builder.finish("DRAW_SURVEY_FEATURES", options.coding, {});
    return result;
}

} // namespace katana::cad
