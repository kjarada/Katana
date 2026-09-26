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
    return "output arg=output kind=vector target=layer layer=" + value(layer) +
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
