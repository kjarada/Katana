#include "katana/entity/tables.hpp"

#include "katana/math/numerics.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <limits>
#include <utility>

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

Status validateName(std::string_view name, const char* what)
{
    if (name.empty()) {
        return makeError(ErrorCode::InvalidArgument, std::string(what) + " name is empty");
    }
    return {};
}

Status validateLineWeight(double lineWeight)
{
    if (!(std::isfinite(lineWeight) && lineWeight >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "line weight must be finite and not negative",
                         std::to_string(lineWeight));
    }
    return {};
}

// ASCII case-insensitive compare, for the reserved names an exchange format
// spells in capitals and a user may not.
[[nodiscard]] bool equalsIgnoringCase(std::string_view a, std::string_view b)
{
    if (a.size() != b.size()) {
        return false;
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
        if (std::tolower(static_cast<unsigned char>(a[i])) !=
            std::tolower(static_cast<unsigned char>(b[i]))) {
            return false;
        }
    }
    return true;
}

template <typename Table> auto collect(const Table& table)
{
    std::vector<typename Table::mapped_type> values;
    values.reserve(table.size());
    for (const auto& [name, value] : table) {
        values.push_back(value);
    }
    return values;
}

} // namespace

// ---- LayerDatabase ---------------------------------------------------------------

LayerDatabase::LayerDatabase()
{
    reset();
}

void LayerDatabase::reset()
{
    layers_.clear();
    layers_.emplace(std::string(kDefaultLayerName), Layer{});
}

Status LayerDatabase::add(Layer layer)
{
    if (auto status = validateLayerPath(layer.name); !status) {
        return status;
    }
    if (auto status = validateLineWeight(layer.lineWeight); !status) {
        return status;
    }
    if (contains(layer.name)) {
        return makeError(ErrorCode::AlreadyExists, "layer already exists", layer.name);
    }
    // Every ancestor is created as a real layer, so each node in the tree has
    // its own colour, visibility and lock. The alternative - implicit nodes
    // that exist only in the tree widget - gives the user a row they can see
    // and cannot switch off, which is worse than not grouping at all.
    //
    // Validation happened above and covers the ancestors too (a valid path has
    // only valid prefixes), so nothing here can fail partway.
    for (const std::string& ancestor : layerAncestors(layer.name)) {
        layers_.try_emplace(ancestor, Layer{ancestor, Color{}, true, false,
                                            std::string(kContinuousLinetype), 0.25});
    }
    std::string name = layer.name;
    layers_.emplace(std::move(name), std::move(layer));
    return {};
}

Status LayerDatabase::update(const Layer& layer)
{
    const auto found = layers_.find(layer.name);
    if (found == layers_.end()) {
        return makeError(ErrorCode::NotFound, "layer does not exist", layer.name);
    }
    if (auto status = validateLineWeight(layer.lineWeight); !status) {
        return status;
    }
    found->second = layer;
    return {};
}

Result<Layer> LayerDatabase::remove(std::string_view name)
{
    if (name == kDefaultLayerName) {
        return makeError(ErrorCode::InvalidArgument, "the default layer cannot be removed");
    }
    const auto found = layers_.find(name);
    if (found == layers_.end()) {
        return makeError(ErrorCode::NotFound, "layer does not exist", std::string(name));
    }
    if (hasChildren(name)) {
        // Deleting the branch silently, or leaving its children pointing at a
        // parent that is gone, are both worse than saying so.
        return makeError(ErrorCode::InvalidArgument,
                         "layer still has nested layers; remove the subtree instead",
                         std::string(name));
    }
    Layer removed = std::move(found->second);
    layers_.erase(found);
    return removed;
}

const Layer* LayerDatabase::find(std::string_view name) const
{
    const auto found = layers_.find(name);
    return found == layers_.end() ? nullptr : &found->second;
}

std::vector<std::string> LayerDatabase::names() const
{
    std::vector<std::string> result;
    result.reserve(layers_.size());
    for (const auto& [name, layer] : layers_) {
        result.push_back(name);
    }
    return result;
}

std::vector<Layer> LayerDatabase::all() const
{
    return collect(layers_);
}

// ---- the layer tree ------------------------------------------------------------
//
// Every one of these is a range scan rather than a search, because the map is
// keyed by full path and so is already ordered as a pre-order walk of the tree.

Status LayerDatabase::ensure(std::string_view name)
{
    if (auto status = validateLayerPath(name); !status) {
        return status;
    }
    if (contains(name)) {
        return {};
    }
    Layer layer;
    layer.name = std::string(name);
    return add(std::move(layer));
}

std::vector<std::string> LayerDatabase::roots() const { return children({}); }

std::vector<std::string> LayerDatabase::children(std::string_view name) const
{
    std::vector<std::string> out;
    const std::size_t wanted = layerDepth(name) + 1;
    for (const auto& [path, layer] : layers_) {
        (void)layer;
        if (!isLayerUnder(path, name) || path == name) {
            continue;
        }
        if (layerDepth(path) == wanted) {
            out.push_back(path);
        }
    }
    return out;
}

std::vector<std::string> LayerDatabase::subtree(std::string_view name) const
{
    std::vector<std::string> out;
    if (!contains(name)) {
        return out;
    }
    for (const auto& [path, layer] : layers_) {
        (void)layer;
        if (isLayerUnder(path, name)) {
            out.push_back(path);
        }
    }
    return out;
}

bool LayerDatabase::hasChildren(std::string_view name) const
{
    // lower_bound past the node itself: the very next key is a child if any is,
    // because "a/b" sorts immediately after "a" and before any sibling of "a".
    auto it = layers_.upper_bound(name);
    return it != layers_.end() && isLayerUnder(it->first, name);
}

bool LayerDatabase::effectivelyVisible(std::string_view name) const
{
    const Layer* layer = find(name);
    if (layer == nullptr) {
        // A layer that is gone cannot be shown: drawing entities that point at
        // one would be drawing something the user cannot turn off or select
        // through the layer tree.
        return false;
    }
    if (!layer->visible) {
        return false;
    }
    for (const std::string& ancestor : layerAncestors(name)) {
        const Layer* above = find(ancestor);
        if (above != nullptr && !above->visible) {
            return false;
        }
    }
    return true;
}

bool LayerDatabase::effectivelyLocked(std::string_view name) const
{
    const Layer* layer = find(name);
    if (layer == nullptr) {
        return true; // cannot edit what is not there
    }
    if (layer->locked) {
        return true;
    }
    for (const std::string& ancestor : layerAncestors(name)) {
        const Layer* above = find(ancestor);
        if (above != nullptr && above->locked) {
            return true;
        }
    }
    return false;
}

Result<std::vector<Layer>> LayerDatabase::removeSubtree(std::string_view name)
{
    if (name == kDefaultLayerName) {
        return makeError(ErrorCode::InvalidArgument, "the default layer cannot be removed",
                         std::string(name));
    }
    if (!contains(name)) {
        return makeError(ErrorCode::NotFound, "layer does not exist", std::string(name));
    }
    const std::vector<std::string> paths = subtree(name);
    // The default layer can only be a root, so it can never be inside another
    // layer's subtree; asserting it anyway would be dead code.

    std::vector<Layer> removed;
    removed.reserve(paths.size());
    // Deepest first, so a caller replaying the list in reverse recreates
    // parents before children.
    for (auto it = paths.rbegin(); it != paths.rend(); ++it) {
        const auto found = layers_.find(*it);
        if (found == layers_.end()) {
            continue;
        }
        removed.push_back(found->second);
        layers_.erase(found);
    }
    return removed;
}

Result<std::vector<std::pair<std::string, std::string>>>
LayerDatabase::renameSubtree(std::string_view from, std::string_view to)
{
    if (from == kDefaultLayerName) {
        return makeError(ErrorCode::InvalidArgument, "the default layer cannot be renamed",
                         std::string(from));
    }
    if (auto status = validateLayerPath(to); !status) {
        return status.error();
    }
    if (!contains(from)) {
        return makeError(ErrorCode::NotFound, "layer does not exist", std::string(from));
    }
    if (from == to) {
        return std::vector<std::pair<std::string, std::string>>{};
    }
    if (isLayerUnder(to, from)) {
        // "design" -> "design/old" would have to be its own descendant.
        return makeError(ErrorCode::InvalidArgument, "a layer cannot be moved inside itself",
                         std::string(from) + " -> " + std::string(to));
    }

    const std::vector<std::string> paths = subtree(from);
    std::vector<std::pair<std::string, std::string>> mapping;
    mapping.reserve(paths.size());
    for (const std::string& path : paths) {
        mapping.emplace_back(path, rewriteLayerPrefix(path, from, to));
    }

    // Every target is checked BEFORE anything moves, so a collision leaves the
    // tree exactly as it was rather than half renamed.
    for (const auto& [before, after] : mapping) {
        (void)before;
        if (contains(after) && !isLayerUnder(after, from)) {
            return makeError(ErrorCode::AlreadyExists, "a layer of that name already exists",
                             after);
        }
    }
    // The new parents have to exist too, and creating them cannot be allowed to
    // fail halfway either, so they are validated first and added after.
    for (const std::string& ancestor : layerAncestors(to)) {
        if (auto status = validateLayerPath(ancestor); !status) {
            return status.error();
        }
    }

    std::vector<Layer> moved;
    moved.reserve(mapping.size());
    for (const auto& [before, after] : mapping) {
        const auto found = layers_.find(before);
        if (found == layers_.end()) {
            continue;
        }
        Layer layer = found->second;
        layer.name = after;
        moved.push_back(std::move(layer));
    }
    for (const auto& [before, after] : mapping) {
        (void)after;
        layers_.erase(std::string(before));
    }
    for (const std::string& ancestor : layerAncestors(to)) {
        layers_.try_emplace(ancestor, Layer{ancestor, Color{}, true, false,
                                            std::string(kContinuousLinetype), 0.25});
    }
    for (Layer& layer : moved) {
        std::string key = layer.name;
        layers_.insert_or_assign(std::move(key), std::move(layer));
    }
    return mapping;
}

// ---- ArrowHead --------------------------------------------------------------------

std::string_view toString(ArrowHead head)
{
    switch (head) {
    case ArrowHead::None:
        return "None";
    case ArrowHead::Tick:
        return "Tick";
    case ArrowHead::ClosedFilled:
        return "ClosedFilled";
    case ArrowHead::Open:
        return "Open";
    case ArrowHead::Dot:
        return "Dot";
    }
    return "Unknown";
}

Result<ArrowHead> arrowHeadFromString(std::string_view name)
{
    for (const ArrowHead head : {ArrowHead::None, ArrowHead::Tick, ArrowHead::ClosedFilled,
                                 ArrowHead::Open, ArrowHead::Dot}) {
        if (equalsIgnoringCase(toString(head), name)) {
            return head;
        }
    }
    return makeError(ErrorCode::ParseFailure, "unknown arrow head", std::string(name));
}

// ---- DimensionStyle ---------------------------------------------------------------

Status validate(const DimensionStyle& style)
{
    if (auto status = validateName(style.name, "dimension style"); !status) {
        return status;
    }

    // Each of these gets its own sentence rather than one "invalid style": the
    // user set one number and needs to know which.
    struct Positive {
        const char* what;
        double value;
    };
    for (const Positive& field : {Positive{"text height", style.textHeight},
                                  Positive{"arrow size", style.arrowSize},
                                  Positive{"unit scale", style.unitScale}}) {
        if (!std::isfinite(field.value) || field.value <= 0.0) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(field.what) + " must be finite and greater than zero",
                             style.name);
        }
    }
    struct NonNegative {
        const char* what;
        double value;
    };
    for (const NonNegative& field :
         {NonNegative{"text gap", style.textGap},
          NonNegative{"extension offset", style.extensionOffset},
          NonNegative{"extension overshoot", style.extensionBeyond}}) {
        if (!std::isfinite(field.value) || field.value < 0.0) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(field.what) + " must be finite and not negative",
                             style.name);
        }
    }

    // Twelve is where a double stops having digits to give: a measurement of a
    // few thousand metres has about sixteen significant digits in total.
    if (style.decimals < 0 || style.decimals > 12) {
        return makeError(ErrorCode::InvalidArgument, "decimals must be between 0 and 12",
                         style.name + " has " + std::to_string(style.decimals));
    }

    if (!std::isfinite(style.roundTo) || style.roundTo < 0.0) {
        return makeError(ErrorCode::InvalidArgument, "the rounding step must not be negative",
                         style.name);
    }
    // No new epsilon: rounding to less than the geometric tolerance is rounding
    // to noise, and a tiny step makes value/step overflow before it rounds.
    if (style.roundTo > 0.0 && style.roundTo < katana::math::tolerance::kGeometric) {
        return makeError(ErrorCode::InvalidArgument,
                         "the rounding step is finer than the geometric tolerance",
                         style.name);
    }

    if (!isValidUtf8(style.prefix) || !isValidUtf8(style.suffix)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the prefix and suffix must be valid UTF-8", style.name);
    }
    return {};
}

// ---- DimensionStyleDatabase -------------------------------------------------------

Status DimensionStylePolicy::validate(const DimensionStyle& style)
{
    return katana::entity::validate(style);
}

void DimensionStylePolicy::seed(NamedMap<DimensionStyle>& items)
{
    DimensionStyle standard;
    standard.name = std::string(kDefaultDimensionStyleName);
    items.emplace(standard.name, std::move(standard));
}

// ---- Linetype --------------------------------------------------------------------

double Linetype::patternLength() const
{
    // The sum of the ABSOLUTE lengths (DXF group 40). Absolute, because a gap
    // is stored negative and still occupies its length along the path.
    double total = 0.0;
    for (const LinetypeElement& element : pattern) {
        total += std::abs(element.length);
    }
    return total;
}

double Linetype::shortestElement() const
{
    // Dots are excluded: a dot has no length by definition, so including it
    // would make every pattern containing one report zero and be judged
    // unresolvable at any zoom.
    double shortest = std::numeric_limits<double>::infinity();
    for (const LinetypeElement& element : pattern) {
        if (element.isDot()) {
            continue;
        }
        shortest = std::min(shortest, std::abs(element.length));
    }
    return std::isfinite(shortest) ? shortest : 0.0;
}

Status validate(const Linetype& linetype)
{
    if (auto status = validateName(linetype.name, "linetype"); !status) {
        return status;
    }
    // BYLAYER and BYBLOCK are the values DXF group code 6 takes on an entity,
    // so they can never name a definition. Reserved now, before blocks exist
    // and need BYBLOCK, because doing it later would be a migration.
    for (const char* reserved : {"ByLayer", "ByBlock"}) {
        if (equalsIgnoringCase(linetype.name, reserved)) {
            return makeError(ErrorCode::InvalidArgument,
                             "that linetype name is reserved by the exchange format",
                             linetype.name);
        }
    }
    if (linetype.pattern.empty()) {
        return {}; // continuous
    }

    for (const LinetypeElement& element : linetype.pattern) {
        if (!std::isfinite(element.length)) {
            return makeError(ErrorCode::InvalidArgument, "linetype element is not finite",
                             linetype.name);
        }
    }
    // Alignment 'A' (DXF group 72, always 65) fits the pattern so a line begins
    // with a dash, so the first element cannot be a gap.
    if (linetype.pattern.front().isGap()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a linetype must begin with a dash or a dot, not a gap", linetype.name);
    }
    // Ending on a gap closes the period. Without it the pattern would place two
    // pen-down runs adjacently when it repeats, which is indistinguishable from
    // one longer dash, and Qt's dash array - which alternates on/off from on -
    // would come out with an odd length and silently mean something else.
    if (!linetype.pattern.back().isGap()) {
        return makeError(ErrorCode::InvalidArgument, "a linetype must end with a gap",
                         linetype.name);
    }
    for (std::size_t i = 1; i < linetype.pattern.size(); ++i) {
        if (linetype.pattern[i].isPenDown() == linetype.pattern[i - 1].isPenDown()) {
            return makeError(ErrorCode::InvalidArgument,
                             "linetype elements must alternate between pen down and pen up",
                             linetype.name + " at element " + std::to_string(i));
        }
    }
    if (!(linetype.patternLength() > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "linetype pattern has no length",
                         linetype.name);
    }
    return {};
}

// ---- LinetypeDatabase ------------------------------------------------------------

Status LinetypePolicy::validate(const Linetype& linetype)
{
    return katana::entity::validate(linetype);
}

Status LinetypePolicy::checkUpdate(const Linetype& linetype)
{
    if (linetype.name == kContinuousLinetype && !linetype.isContinuous()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the continuous linetype cannot be given a pattern");
    }
    return {};
}

void LinetypePolicy::seed(NamedMap<Linetype>& items)
{
    // Only "continuous" is built in.
    //
    // No "dashed", "center" or "hidden" is seeded, deliberately. AutoCAD's
    // acad.lin set is in imperial drawing units and its ISO set is in
    // millimetres, while a survey drawing here is in metres - so any table
    // shipped would be either a conversion of numbers not to hand or an
    // invention. A test would then pin the invention as though it were a
    // standard. Patterns are defined by the user or read from an imported
    // file; see docs/model.md.
    Linetype continuous;
    continuous.name = std::string(kContinuousLinetype);
    continuous.description = "Solid line";
    items.emplace(continuous.name, std::move(continuous));
}

// ---- StyleDatabase ---------------------------------------------------------------

Status StylePolicy::validate(const Style& style)
{
    if (auto status = validateName(style.name, "style"); !status) {
        return status;
    }
    return validateLineWeight(style.lineWeight);
}

// ---- PropertyDatabase ------------------------------------------------------------

Status PropertyDatabase::define(PropertyDefinition definition)
{
    if (auto status = validateName(definition.name, "property"); !status) {
        return status;
    }
    if (find(definition.name) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "property is already defined", definition.name);
    }
    if (definition.defaultValue && typeOf(*definition.defaultValue) != definition.type) {
        return makeError(ErrorCode::InvalidArgument,
                         "default value does not match the property type", definition.name);
    }
    std::string name = definition.name;
    definitions_.emplace(std::move(name), std::move(definition));
    return {};
}

Result<PropertyDefinition> PropertyDatabase::undefine(std::string_view name)
{
    const auto found = definitions_.find(name);
    if (found == definitions_.end()) {
        return makeError(ErrorCode::NotFound, "property is not defined", std::string(name));
    }
    PropertyDefinition removed = std::move(found->second);
    definitions_.erase(found);
    return removed;
}

const PropertyDefinition* PropertyDatabase::find(std::string_view name) const
{
    const auto found = definitions_.find(name);
    return found == definitions_.end() ? nullptr : &found->second;
}

std::vector<PropertyDefinition> PropertyDatabase::all() const
{
    return collect(definitions_);
}

Status PropertyDatabase::validate(std::string_view name, const PropertyValue& value) const
{
    const PropertyDefinition* definition = find(name);
    if (definition != nullptr && typeOf(value) != definition->type) {
        return makeError(ErrorCode::InvalidArgument, "property value has the wrong type",
                         std::string(name));
    }
    if (const double* real = std::get_if<double>(&value); real != nullptr && !std::isfinite(*real)) {
        return makeError(ErrorCode::InvalidArgument, "property value is not finite",
                         std::string(name));
    }
    // Strings are persisted through a JSON writer that throws on invalid UTF-8,
    // from a path that returns Status. Rejecting the bytes here - which covers
    // entity properties AND metadata - keeps that impossible.
    if (!isValidUtf8(name)) {
        return makeError(ErrorCode::InvalidArgument, "property name is not valid UTF-8",
                         std::string(name));
    }
    if (const auto* text = std::get_if<std::string>(&value); text != nullptr &&
                                                             !isValidUtf8(*text)) {
        return makeError(ErrorCode::InvalidArgument, "property value is not valid UTF-8",
                         std::string(name));
    }
    return {};
}

} // namespace katana::entity
