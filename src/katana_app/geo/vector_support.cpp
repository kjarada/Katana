// What the curated vector verbs share (vector_support.hpp).

#include "vector_support.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <utility>
#include <variant>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/math/numerics.hpp"
#include "replies.hpp"

namespace katana::app::geo::vector {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::EntityId;

namespace {

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
}

bool listed(const std::vector<std::string>& items, const std::string& item)
{
    return std::ranges::find(items, item) != items.end();
}

std::string textOf(const gp::FieldValue& value)
{
    return std::visit(
        [](const auto& held) -> std::string {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, std::monostate>) {
                return {};
            } else if constexpr (std::is_same_v<Held, bool>) {
                return held ? "true" : "false";
            } else if constexpr (std::is_same_v<Held, std::int64_t>) {
                return std::to_string(held);
            } else if constexpr (std::is_same_v<Held, double>) {
                return katana::core::formatExactReal(held);
            } else {
                return held;
            }
        },
        value);
}

bool emptyGeometry(const katana::gis::VectorGeometry& geometry)
{
    return geometry.parts.empty() || geometry.parts.front().empty();
}

} // namespace

// ---- a line's own words ----------------------------------------------------------------------

const std::string* VerbWords::option(std::string_view key) const
{
    const auto found = options.find(std::string(key));
    return found == options.end() ? nullptr : &found->second;
}

Result<VerbWords> readVerbWords(const Tokens& tokens, std::size_t begin, std::size_t end,
                                const WordRules& rules)
{
    // The clause alone, so a TO near its end cannot read past it (OVERLAY's
    // subject ends at WITH).
    Tokens clause;
    for (std::size_t i = begin; i < end && i < tokens.size(); ++i) {
        clause.words.push_back(tokens.words[i]);
        clause.quoted.push_back(tokens.quoted[i]);
    }
    VerbWords out;
    std::vector<std::string> scope;
    bool inWhere = false;
    std::optional<std::size_t> firstWhere;
    for (std::size_t i = 0; i < clause.size(); ++i) {
        const std::string& word = clause[i];
        const std::size_t equals = word.find('=');
        if (equals != std::string::npos && equals > 0) {
            const std::string key = katana::core::lowered(word.substr(0, equals));
            if (listed(rules.options, key)) {
                if (out.options.contains(key)) {
                    return makeError(ErrorCode::InvalidArgument, key + "= is given twice", word);
                }
                out.options.emplace(key, word.substr(equals + 1));
                continue;
            }
        }
        const std::string folded = clause.quoted[i] ? std::string() : upper(word);
        if (!folded.empty() && listed(rules.flags, folded)) {
            if (!out.flags.insert(folded).second) {
                return makeError(ErrorCode::InvalidArgument, folded + " is given twice", word);
            }
            continue;
        }
        if (folded == "TO") {
            if (!rules.target) {
                return makeError(ErrorCode::InvalidArgument, "TO is not a word of this verb: " +
                                                                 rules.usage,
                                 word);
            }
            if (out.target) {
                return makeError(ErrorCode::InvalidArgument, "one TO per line", word);
            }
            std::size_t at = i + 1;
            auto target = parseTarget(clause, at);
            if (!target) {
                return target.error();
            }
            out.target = std::move(target).value();
            i = at - 1;
            continue;
        }
        if (folded == "WHERE") {
            firstWhere = firstWhere.value_or(scope.size());
            inWhere = true;
        }
        scope.push_back(word);
        if (!inWhere && (folded == "LAYERS" || folded == "LAYER") && i + 1 < clause.size()) {
            scope.push_back(clause[++i]);
        }
    }
    std::size_t at = 0;
    auto parsed = katana::cad::parseScopeWords(scope, at);
    if (!parsed) {
        return parsed.error();
    }
    if (at < scope.size()) {
        // In the filter, a word that is no condition is named as one.
        if (firstWhere && at > *firstWhere) {
            katana::cad::ModifyFilter unused;
            if (auto status = katana::cad::parseWhereCondition(scope[at], unused); !status) {
                return status.error();
            }
        }
        return makeError(ErrorCode::InvalidArgument, "not a word of this verb: " + rules.usage,
                         scope[at]);
    }
    out.scope = std::move(parsed).value();
    return out;
}

Result<std::optional<double>> numberOption(const VerbWords& words, std::string_view key)
{
    const std::string* text = words.option(key);
    if (text == nullptr) {
        return std::optional<double>();
    }
    const auto number = katana::core::parseFiniteDouble(*text);
    if (!number) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + "= is a number",
                         std::string(key) + "=" + *text);
    }
    return std::optional<double>(*number);
}

Result<std::string> choiceOption(const VerbWords& words, std::string_view key,
                                 const std::vector<std::string>& choices, std::string_view fallback)
{
    const std::string* text = words.option(key);
    if (text == nullptr) {
        return std::string(fallback);
    }
    const std::string folded = katana::core::lowered(*text);
    if (listed(choices, folded)) {
        return folded;
    }
    std::string list;
    for (const std::string& choice : choices) {
        list += (list.empty() ? "" : "|") + choice;
    }
    return makeError(ErrorCode::InvalidArgument, std::string(key) + "= is " + list,
                     std::string(key) + "=" + *text);
}

std::vector<std::string> listOf(std::string_view text)
{
    std::vector<std::string> items;
    std::size_t from = 0;
    while (from <= text.size()) {
        const std::size_t comma = text.find(',', from);
        const std::size_t to = comma == std::string_view::npos ? text.size() : comma;
        const std::string_view item = katana::core::trimmed(text.substr(from, to - from));
        if (!item.empty()) {
            items.emplace_back(item);
        }
        if (comma == std::string_view::npos) {
            break;
        }
        from = comma + 1;
    }
    return items;
}

Result<std::string> layerTarget(const VerbWords& words, std::string_view verb,
                                std::string_view fallback)
{
    if (!words.target) {
        return std::string(fallback);
    }
    if (words.target->kind != Target::Kind::Layer) {
        return makeError(ErrorCode::Unsupported,
                         std::string(verb) + " draws its result on a layer: TO LAYER <path>");
    }
    return words.target->name;
}

// ---- the drawing as a feature set --------------------------------------------------------------

Result<BoundDrawing> bindScope(Context& context, const katana::cad::ScopeWords& scope)
{
    igeo::DrawingDatasetOptions options;
    options.crsWkt = projectCrs(context);
    return bindDrawing(context, scope, options);
}

const gp::FeatureTable* tableNamed(const gp::FeatureSet& set, std::string_view name)
{
    for (const gp::FeatureTable& table : set.tables) {
        if (table.name == name) {
            return &table;
        }
    }
    return nullptr;
}

std::optional<std::size_t> fieldIndex(const gp::FeatureTable& table, std::string_view name)
{
    for (std::size_t f = 0; f < table.fields.size(); ++f) {
        if (table.fields[f].name == name) {
            return f;
        }
    }
    return std::nullopt;
}

bool hasField(const gp::FeatureSet& set, std::string_view name)
{
    return std::ranges::any_of(set.tables, [&](const gp::FeatureTable& table) {
        return fieldIndex(table, name).has_value();
    });
}

std::optional<EntityId> featureId(const gp::FeatureTable& table, const gp::Feature& feature,
                                  std::string_view field)
{
    const auto index = fieldIndex(table, field);
    if (!index || *index >= feature.values.size()) {
        return std::nullopt;
    }
    if (const auto* id = std::get_if<std::int64_t>(&feature.values[*index]); id != nullptr && *id > 0) {
        return static_cast<EntityId>(*id);
    }
    return std::nullopt;
}

gp::FeatureTable mergedTable(const std::vector<const gp::FeatureTable*>& tables, std::string name)
{
    gp::FeatureTable merged;
    merged.name = std::move(name);
    bool first = true;
    for (const gp::FeatureTable* table : tables) {
        if (first) {
            merged.kind = table->kind;
            first = false;
        } else if (merged.kind != table->kind) {
            merged.kind = katana::gis::GeometryKind::Unknown;
        }
        merged.hasZ = merged.hasZ || table->hasZ;
        if (merged.crsWkt.empty()) {
            merged.crsWkt = table->crsWkt;
        }
        for (const gp::FieldDef& field : table->fields) {
            const auto at = fieldIndex(merged, field.name);
            if (!at) {
                merged.fields.push_back(field);
            } else if (merged.fields[*at].type != field.type) {
                merged.fields[*at].type = gp::FieldType::String;
            }
        }
    }
    for (const gp::FeatureTable* table : tables) {
        std::vector<std::size_t> into;
        for (const gp::FieldDef& field : table->fields) {
            into.push_back(*fieldIndex(merged, field.name));
        }
        for (const gp::Feature& feature : table->features) {
            gp::Feature copy;
            copy.parts = feature.parts;
            copy.values.assign(merged.fields.size(), gp::FieldValue(std::monostate{}));
            for (std::size_t f = 0; f < into.size() && f < feature.values.size(); ++f) {
                const bool text = merged.fields[into[f]].type == gp::FieldType::String &&
                                  !std::holds_alternative<std::string>(feature.values[f]) &&
                                  !std::holds_alternative<std::monostate>(feature.values[f]);
                copy.values[into[f]] = text ? gp::FieldValue(textOf(feature.values[f]))
                                            : feature.values[f];
            }
            merged.features.push_back(std::move(copy));
        }
    }
    return merged;
}

// ---- measures --------------------------------------------------------------------------------

double ringArea(const std::vector<katana::gis::GeoPoint>& ring)
{
    katana::geometry::Polyline2 line;
    line.closed = true;
    for (const katana::gis::GeoPoint& point : ring) {
        line.vertices.emplace_back(point.x, point.y);
    }
    // A ring GDAL hands back repeats its first point; the repeat adds nothing.
    return line.area();
}

double geometryArea(const katana::gis::VectorGeometry& geometry)
{
    if (geometry.kind != katana::gis::GeometryKind::Polygon || geometry.parts.empty()) {
        return 0.0;
    }
    double area = ringArea(geometry.parts.front());
    for (std::size_t r = 1; r < geometry.parts.size(); ++r) {
        area -= ringArea(geometry.parts[r]);
    }
    return area;
}

double geometryLength(const katana::gis::VectorGeometry& geometry)
{
    if (geometry.kind == katana::gis::GeometryKind::Point) {
        return 0.0;
    }
    double length = 0.0;
    for (const auto& ring : geometry.parts) {
        for (std::size_t i = 1; i < ring.size(); ++i) {
            length += std::hypot(ring[i].x - ring[i - 1].x, ring[i].y - ring[i - 1].y);
        }
        if (geometry.kind == katana::gis::GeometryKind::Polygon && ring.size() > 2 &&
            (ring.front().x != ring.back().x || ring.front().y != ring.back().y)) {
            length += std::hypot(ring.front().x - ring.back().x, ring.front().y - ring.back().y);
        }
    }
    return length;
}

double featureArea(const gp::Feature& feature)
{
    double area = 0.0;
    for (const katana::gis::VectorGeometry& part : feature.parts) {
        area += geometryArea(part);
    }
    return area;
}

double featureLength(const gp::Feature& feature)
{
    double length = 0.0;
    for (const katana::gis::VectorGeometry& part : feature.parts) {
        length += geometryLength(part);
    }
    return length;
}

Measures measure(const gp::FeatureSet& set)
{
    Measures measures;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            ++measures.features;
            if (std::ranges::all_of(feature.parts, emptyGeometry)) {
                ++measures.empty;
            }
            measures.area += featureArea(feature);
            measures.length += featureLength(feature);
        }
    }
    return measures;
}

gp::FeatureSet withoutEmpty(gp::FeatureSet set)
{
    for (gp::FeatureTable& table : set.tables) {
        for (gp::Feature& feature : table.features) {
            std::erase_if(feature.parts, emptyGeometry);
        }
        std::erase_if(table.features, [](const gp::Feature& feature) { return feature.parts.empty(); });
    }
    std::erase_if(set.tables, [](const gp::FeatureTable& table) { return table.features.empty(); });
    return set;
}

// ---- running --------------------------------------------------------------------------------

Result<gp::FeatureSet> runVector(const std::vector<std::string>& path, const gp::FeatureSet& input,
                                 const std::vector<std::string>& tokens, const std::stop_token& stop,
                                 const Progress& progress, std::vector<std::string>& warnings,
                                 const std::vector<std::pair<std::string, gp::FeatureSet>>& more)
{
    gp::RunRequest request;
    request.path = path;
    request.values.emplace_back("input", gp::ArgValue(gp::DatasetValue(input)));
    for (const auto& [arg, set] : more) {
        request.values.emplace_back(arg, gp::ArgValue(gp::DatasetValue(set)));
    }
    request.tokens = tokens;
    request.outputTo = gp::OutputTo::Memory;
    auto outputs = gp::run(request, stop, progress);
    if (!outputs) {
        return outputs.error();
    }
    for (const gp::Diagnostic& diagnostic : outputs->diagnostics) {
        if (!diagnostic.failure) {
            warnings.push_back(diagnostic.message);
        }
    }
    return outputs->features.value_or(gp::FeatureSet{});
}

Progress progressSpan(const Progress& progress, double from, double to)
{
    if (!progress) {
        return {};
    }
    return [progress, from, to](double fraction) {
        progress(from + (to - from) * std::clamp(fraction, 0.0, 1.0));
    };
}

// ---- applying -------------------------------------------------------------------------------

Result<std::vector<EntityId>> executeStep(Context& context, katana::commands::CommandPtr command)
{
    if (!command) {
        return std::vector<EntityId>{};
    }
    if (auto status = context.document.execute(std::move(command)); !status) {
        return status.error();
    }
    std::vector<EntityId> created = context.document.lastCreatedEntities();
    if (context.frame && !created.empty()) {
        katana::geometry::Box2 box;
        for (const EntityId id : created) {
            if (const katana::entity::Entity* entity = context.document.model().entities.find(id)) {
                const katana::geometry::Box2 one = katana::entity::boundingBox(entity->geometry);
                if (!one.empty()) {
                    box.expand(one.min);
                    box.expand(one.max);
                }
            }
        }
        if (!box.empty()) {
            context.frame(box);
        }
    }
    return created;
}

std::string outputRecord(std::string_view layer, const Applied& applied)
{
    return "output arg=output kind=vector " +
           (layer.empty() ? std::string("target=in-place") : "target=layer layer=" + value(layer)) +
           " created=" + std::to_string(applied.created) +
           " updated=" + std::to_string(applied.updated) +
           " deleted=" + std::to_string(applied.deleted) +
           " skipped=" + std::to_string(applied.skipped);
}

std::string gisRecord(std::string_view op, double seconds)
{
    return "gis op=" + value(op) + " seconds=" + fixed3(seconds) + " cancelled=no";
}

std::string notRunRecord(std::string_view op, std::string_view why)
{
    return "gis op=" + value(op) + " seconds=0.000 cancelled=no ran=no" +
           (why.empty() ? std::string() : " " + std::string(why));
}

std::string joined(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        if (!record.empty()) {
            text += (text.empty() ? "" : "\n") + record;
        }
    }
    return text;
}

Prepared answered(std::string title, std::string reply)
{
    Prepared prepared;
    prepared.title = std::move(title);
    prepared.reply = std::move(reply);
    return prepared;
}

std::vector<katana::entity::Entity> copiesOf(const katana::cad::Document& document,
                                             const std::vector<EntityId>& ids)
{
    std::vector<katana::entity::Entity> copies;
    for (const EntityId id : ids) {
        if (const katana::entity::Entity* entity = document.model().entities.find(id)) {
            copies.push_back(*entity);
        }
    }
    return copies;
}

std::optional<katana::entity::PropertyValue> propertyOf(const gp::FieldValue& value)
{
    return std::visit(
        [](const auto& held) -> std::optional<katana::entity::PropertyValue> {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, std::monostate>) {
                return std::nullopt;
            } else {
                return katana::entity::PropertyValue(held);
            }
        },
        value);
}

// ---- reshaping entities in place ---------------------------------------------------------------

namespace {

using katana::gis::GeoPoint;
using katana::gis::GeometryKind;
using katana::gis::VectorGeometry;

// A ring without the repeat of its first point GDAL closes it with.
std::vector<GeoPoint> openRing(const std::vector<GeoPoint>& ring)
{
    std::vector<GeoPoint> points = ring;
    if (points.size() > 1 && points.front().x == points.back().x &&
        points.front().y == points.back().y) {
        points.pop_back();
    }
    return points;
}

// Coordinates pass through GDAL's MEM datasets as doubles, so a vertex an
// algorithm left alone comes back bit for bit; the tolerance is the model's
// geometric one, for the arithmetic of one that was moved and put back.
bool samePoint(const GeoPoint& a, const GeoPoint& b)
{
    return std::abs(a.x - b.x) <= katana::math::tolerance::kGeometric &&
           std::abs(a.y - b.y) <= katana::math::tolerance::kGeometric;
}

bool sameHoles(const std::vector<std::vector<GeoPoint>>& before,
               const std::vector<std::vector<GeoPoint>>& after)
{
    if (before.size() != after.size()) {
        return false;
    }
    std::vector<bool> used(after.size(), false);
    for (const auto& hole : before) {
        bool found = false;
        for (std::size_t h = 0; h < after.size() && !found; ++h) {
            if (!used[h] && sameRing(hole, after[h])) {
                used[h] = true;
                found = true;
            }
        }
        if (!found) {
            return false;
        }
    }
    return true;
}

std::vector<std::vector<GeoPoint>> holesOf(const VectorGeometry& polygon)
{
    return polygon.parts.size() > 1
               ? std::vector<std::vector<GeoPoint>>(polygon.parts.begin() + 1, polygon.parts.end())
               : std::vector<std::vector<GeoPoint>>{};
}

double signedRingArea(const std::vector<GeoPoint>& ring)
{
    katana::geometry::Polyline2 line;
    line.closed = true;
    for (const GeoPoint& point : openRing(ring)) {
        line.vertices.emplace_back(point.x, point.y);
    }
    return line.signedArea();
}

std::vector<std::optional<double>> heightsOf(const std::vector<GeoPoint>& points, bool hasZ)
{
    std::vector<std::optional<double>> heights;
    for (const GeoPoint& point : points) {
        heights.push_back(hasZ && std::isfinite(point.z) ? std::optional<double>(point.z)
                                                         : std::nullopt);
    }
    return heights;
}

// A part as a copy of `from`: a ring as a closed polyline, a line as a line
// or polyline.
katana::entity::Entity copyWith(const katana::entity::Entity& from, const std::vector<GeoPoint>& points,
                                bool ring, bool hasZ)
{
    katana::entity::Entity made = from;
    made.id = katana::entity::kInvalidEntityId;
    const std::vector<GeoPoint> used = ring ? openRing(points) : points;
    if (!ring && used.size() == 2) {
        made.geometry = katana::geometry::Segment2{{used[0].x, used[0].y}, {used[1].x, used[1].y}};
    } else {
        katana::geometry::Polyline2 line;
        line.closed = ring;
        for (const GeoPoint& point : used) {
            line.vertices.emplace_back(point.x, point.y);
        }
        made.geometry = std::move(line);
    }
    katana::entity::setHeights(made.properties, heightsOf(used, hasZ));
    return made;
}

} // namespace

std::map<EntityId, const gp::Feature*> featuresById(const gp::FeatureSet& set)
{
    std::map<EntityId, const gp::Feature*> byId;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            if (const auto id = featureId(table, feature)) {
                byId.emplace(*id, &feature);
            }
        }
    }
    return byId;
}

bool sameRing(const std::vector<GeoPoint>& a, const std::vector<GeoPoint>& b)
{
    const std::vector<GeoPoint> first = openRing(a);
    const std::vector<GeoPoint> second = openRing(b);
    if (first.size() != second.size()) {
        return false;
    }
    const std::size_t n = first.size();
    for (std::size_t start = 0; start < n; ++start) {
        if (!samePoint(first[0], second[start])) {
            continue;
        }
        bool forward = true;
        bool backward = true;
        for (std::size_t i = 0; i < n && (forward || backward); ++i) {
            forward = forward && samePoint(first[i], second[(start + i) % n]);
            backward = backward && samePoint(first[i], second[(start + n - i) % n]);
        }
        if (forward || backward) {
            return true;
        }
    }
    return n == 0;
}

bool sameShape(const gp::Feature& before, const gp::Feature& after)
{
    if (before.parts.size() != after.parts.size()) {
        return false;
    }
    for (std::size_t p = 0; p < before.parts.size(); ++p) {
        const VectorGeometry& was = before.parts[p];
        const VectorGeometry& now = after.parts[p];
        if (was.kind != now.kind || was.parts.empty() != now.parts.empty()) {
            return false;
        }
        if (was.parts.empty()) {
            continue;
        }
        if (was.kind == GeometryKind::Polygon) {
            if (!sameRing(was.parts.front(), now.parts.front()) ||
                !sameHoles(holesOf(was), holesOf(now))) {
                return false;
            }
            continue;
        }
        const auto& line = was.parts.front();
        const auto& other = now.parts.front();
        if (line.size() != other.size()) {
            return false;
        }
        for (std::size_t i = 0; i < line.size(); ++i) {
            if (!samePoint(line[i], other[i])) {
                return false;
            }
        }
    }
    return true;
}

Result<Reshape> reshapeCommand(const katana::entity::Model& model,
                               const std::vector<Reshaped>& reshaped, const std::string& op,
                               const std::string& commandName)
{
    Reshape out;
    gp::FeatureSet updates;
    gp::FeatureTable& table = updates.tables.emplace_back();
    table.name = "reshaped";
    table.fields = {{"katana_id", gp::FieldType::Integer64}};
    std::vector<katana::entity::Entity> made;
    // The id the first made entity will have (ids are monotonic and the
    // command makes them in order), so a part's rings name their exterior.
    const EntityId firstId = model.entities.nextId();
    std::size_t emptied = 0, holesChanged = 0, missing = 0, flattened = 0;
    for (const Reshaped& one : reshaped) {
        const katana::entity::Entity* entity = model.entities.find(one.id);
        if (entity == nullptr) {
            ++missing;
            continue;
        }
        const GeometryKind kind =
            one.before.parts.empty() ? GeometryKind::Unknown : one.before.parts.front().kind;
        std::vector<const VectorGeometry*> parts;
        for (const VectorGeometry& part : one.parts) {
            if (part.kind == kind && !emptyGeometry(part)) {
                parts.push_back(&part);
            }
        }
        if (parts.empty()) {
            ++emptied;
            continue;
        }
        const auto size = [kind](const VectorGeometry* part) {
            return kind == GeometryKind::Polygon ? geometryArea(*part) : geometryLength(*part);
        };
        std::size_t best = 0;
        for (std::size_t p = 1; p < parts.size(); ++p) {
            best = size(parts[p]) > size(parts[best]) ? p : best;
        }
        VectorGeometry kept = *parts[best];
        if (kind == GeometryKind::Polygon) {
            // An area is one ring and its holes entities of their own, which
            // stay: a result that changed the holes cannot be written on it.
            if (!sameHoles(holesOf(one.before.parts.front()), holesOf(kept))) {
                ++holesChanged;
                continue;
            }
            kept.parts.resize(1);
            const auto* ring = std::get_if<katana::geometry::Polyline2>(&entity->geometry);
            const double was = ring != nullptr ? ring->signedArea() : 1.0;
            if ((was < 0.0) != (signedRingArea(kept.parts.front()) < 0.0)) {
                std::ranges::reverse(kept.parts.front());
            }
        }
        const bool hadHeights = std::ranges::any_of(one.before.parts, [](const VectorGeometry& part) {
            return part.hasZ;
        });
        flattened += hadHeights && !kept.hasZ ? 1u : 0u;
        gp::Feature feature;
        feature.parts.push_back(std::move(kept));
        feature.values.emplace_back(static_cast<std::int64_t>(one.id));
        table.features.push_back(std::move(feature));
        if (parts.size() == 1) {
            continue;
        }
        Split split;
        split.id = one.id;
        split.parts = parts.size();
        for (std::size_t p = 0; p < parts.size(); ++p) {
            if (p == best) {
                continue;
            }
            const VectorGeometry& part = *parts[p];
            const std::size_t exterior = made.size();
            for (std::size_t r = 0; r < part.parts.size(); ++r) {
                katana::entity::Entity copy =
                    copyWith(*entity, part.parts[r], kind == GeometryKind::Polygon, part.hasZ);
                if (part.parts.size() > 1) {
                    copy.properties["gis.ring"] = std::string(r == 0 ? "exterior" : "hole");
                    copy.properties["gis.part"] = static_cast<std::int64_t>(firstId + exterior);
                }
                copy.properties["gis.op"] = op;
                copy.properties["gis.source"] = static_cast<std::int64_t>(one.id);
                made.push_back(std::move(copy));
                ++split.made;
                if (kind != GeometryKind::Polygon) {
                    break; // a line has one part
                }
            }
        }
        out.splits.push_back(split);
    }
    igeo::ResultOptions options;
    options.mode = igeo::ResultMode::UpdateGeometry;
    options.commandName = commandName;
    auto plan = igeo::resultCommand(model, updates, options);
    if (!plan) {
        return plan.error();
    }
    out.updated = plan->updated;
    out.created = made.size();
    out.left = emptied + holesChanged + missing + plan->skipped;
    for (std::string& warning : plan->warnings) {
        out.warnings.push_back(std::move(warning));
    }
    if (emptied != 0) {
        out.warnings.push_back(std::to_string(emptied) +
                               " entities came back with nothing of their kind and were left as "
                               "they were");
    }
    if (holesChanged != 0) {
        out.warnings.push_back(std::to_string(holesChanged) +
                               " areas came back with other holes; an area and its holes are "
                               "entities of their own, so they were left as they were");
    }
    if (missing != 0) {
        out.warnings.push_back(std::to_string(missing) +
                               " results name an entity the drawing no longer has");
    }
    if (flattened != 0) {
        out.warnings.push_back(std::to_string(flattened) +
                               " entities came back without heights and are now in plan");
    }
    if (!plan->command && made.empty()) {
        return out;
    }
    auto transaction = std::make_unique<cmd::Transaction>(commandName);
    if (plan->command) {
        transaction->add(std::move(plan->command));
    }
    if (!made.empty()) {
        transaction->add(cmd::createEntities(std::move(made)));
    }
    out.command = std::move(transaction);
    return out;
}

std::vector<std::string> splitRecords(const std::vector<Split>& splits,
                                      const std::vector<EntityId>& created)
{
    std::vector<std::string> records;
    std::size_t at = 0;
    for (const Split& split : splits) {
        std::string ids;
        for (std::size_t i = 0; i < split.made && at < created.size(); ++i, ++at) {
            ids += (ids.empty() ? "" : ",") + std::to_string(created[at]);
        }
        records.push_back("split entity=" + std::to_string(split.id) +
                          " parts=" + std::to_string(split.parts) + " kept=" +
                          std::to_string(split.id) + " created=" + ids);
    }
    return records;
}

katana::commands::CommandPtr replaceMarkers(const katana::entity::Model& model,
                                            const std::string& layer, const std::string& kind,
                                            std::vector<katana::entity::Entity> markers,
                                            const std::string& commandName, std::size_t& removed)
{
    std::vector<EntityId> stale;
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        const auto tag = entity.properties.find("gis.marker");
        if (entity.layer == layer && tag != entity.properties.end() &&
            katana::entity::toString(tag->second) == kind) {
            stale.push_back(entity.id);
        }
    });
    removed = stale.size();
    if (stale.empty() && markers.empty()) {
        return nullptr;
    }
    auto transaction = std::make_unique<cmd::Transaction>(commandName);
    if (!stale.empty()) {
        transaction->add(cmd::deleteEntities(std::move(stale)));
    }
    if (!markers.empty()) {
        if (!model.layers.contains(layer)) {
            katana::entity::Layer made;
            made.name = layer;
            transaction->add(cmd::createLayer(made));
        }
        for (katana::entity::Entity& marker : markers) {
            marker.layer = layer;
            marker.properties["gis.marker"] = kind;
        }
        transaction->add(cmd::createEntities(std::move(markers)));
    }
    return transaction;
}

} // namespace katana::app::geo::vector
