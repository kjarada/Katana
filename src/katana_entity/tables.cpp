#include "katana/entity/tables.hpp"

#include "validation.hpp"

#include "katana/math/numerics.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

using katana::entity::detail::validateName;

Status validateLineWeight(double lineWeight)
{
    if (!(std::isfinite(lineWeight) && lineWeight >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "line weight must be finite and not negative",
                         std::to_string(lineWeight));
    }
    return {};
}

// A layer's own fields, its name apart (validateLayerPath). Its linetype,
// hatch pattern and dimension style are references by name, and a name that
// is not UTF-8 cannot be written to JSON or read back (audit MOD-12).
Status validateLayerFields(const Layer& layer)
{
    if (auto status = validateLineWeight(layer.lineWeight); !status) {
        return status;
    }
    if (!isValidUtf8(layer.linetype) || !isValidUtf8(layer.hatchPattern) ||
        !isValidUtf8(layer.dimensionStyle)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a layer's linetype, hatch pattern or dimension style is not UTF-8",
                         layer.name);
    }
    return {};
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
    layers_.emplace(std::string(kDefaultLayerName), Node{Layer{}});
    refreshSubtree(kDefaultLayerName);
}

// Everything under `name` is exactly the keys beginning "name/", and those are
// contiguous: they sort from "name/" up to "name0", '0' being the character
// after '/'. NOT the keys straight after `name` up to the first that is not a
// descendant: a sibling whose name continues with a character below '/' -
// "design 2", "design-old", "design.bak" - sorts BETWEEN "design" and
// "design/surface".
std::pair<LayerDatabase::Nodes::iterator, LayerDatabase::Nodes::iterator>
LayerDatabase::descendantRange(std::string_view name)
{
    std::string from(name);
    from += kLayerSeparator;
    std::string to(name);
    to += static_cast<char>(kLayerSeparator + 1);
    return {layers_.lower_bound(from), layers_.lower_bound(to)};
}

void LayerDatabase::refreshSubtree(std::string_view name)
{
    const auto refresh = [this](Node& node, std::string_view path) {
        node.shown = node.layer.visible;
        node.locked = node.layer.locked;
        const std::string_view parent = layerParent(path);
        if (parent.empty()) {
            return;
        }
        // A parent is a proper prefix of its child, so it sorts first and is
        // already current by the time the child is reached.
        const auto above = layers_.find(parent);
        if (above != layers_.end()) {
            node.shown = node.shown && above->second.shown;
            node.locked = node.locked || above->second.locked;
        }
    };
    const auto self = layers_.find(name);
    if (self == layers_.end()) {
        return;
    }
    refresh(self->second, self->first);
    const auto [first, last] = descendantRange(name);
    for (auto it = first; it != last; ++it) {
        refresh(it->second, it->first);
    }
}

ResolvedLayer LayerDatabase::resolve(std::string_view name) const
{
    const auto found = layers_.find(name);
    if (found == layers_.end()) {
        return {};
    }
    return {&found->second.layer, found->second.shown, found->second.locked};
}

Status LayerDatabase::checkAdd(const Layer& layer) const
{
    if (auto status = validateLayerPath(layer.name); !status) {
        return status;
    }
    if (auto status = validateLayerFields(layer); !status) {
        return status;
    }
    if (contains(layer.name)) {
        return makeError(ErrorCode::AlreadyExists, "layer already exists", layer.name);
    }
    return {};
}

Status LayerDatabase::checkUpdate(const Layer& layer) const
{
    if (!contains(layer.name)) {
        return makeError(ErrorCode::NotFound, "layer does not exist", layer.name);
    }
    return validateLayerFields(layer);
}

Status LayerDatabase::checkRemove(std::string_view name) const
{
    if (name == kDefaultLayerName) {
        return makeError(ErrorCode::InvalidArgument, "the default layer cannot be removed");
    }
    if (!contains(name)) {
        return makeError(ErrorCode::NotFound, "layer does not exist", std::string(name));
    }
    if (hasChildren(name)) {
        // Deleting the branch silently, or leaving its children pointing at a
        // parent that is gone, are both worse than saying so.
        return makeError(ErrorCode::InvalidArgument,
                         "layer still has nested layers; remove the subtree instead",
                         std::string(name));
    }
    return {};
}

Status LayerDatabase::add(Layer layer)
{
    if (auto status = checkAdd(layer); !status) {
        return status;
    }
    // Every ancestor is created as a real layer, so each node in the tree has
    // its own colour, visibility and lock. The alternative - implicit nodes
    // that exist only in the tree widget - gives the user a row they can see
    // and cannot switch off, which is worse than not grouping at all.
    //
    // Validation happened above and covers the ancestors too (a valid path has
    // only valid prefixes), so nothing here can fail partway.
    const std::vector<std::string> ancestors = layerAncestors(layer.name);
    for (const std::string& ancestor : ancestors) {
        layers_.try_emplace(ancestor, Node{Layer{ancestor, Color{}, true, false,
                                                 std::string(kContinuousLinetype), 0.25}});
    }
    const std::string name = layer.name;
    layers_.emplace(name, Node{std::move(layer)});
    // From the topmost ancestor: one created just now is as new as the layer,
    // and refreshing from the root of the path covers both.
    refreshSubtree(ancestors.empty() ? std::string_view(name)
                                     : std::string_view(ancestors.front()));
    return {};
}

Status LayerDatabase::update(const Layer& layer)
{
    if (auto status = checkUpdate(layer); !status) {
        return status;
    }
    const auto found = layers_.find(layer.name);
    found->second.layer = layer;
    refreshSubtree(layer.name);
    return {};
}

Result<Layer> LayerDatabase::remove(std::string_view name)
{
    if (auto status = checkRemove(name); !status) {
        return status.error();
    }
    const auto found = layers_.find(name);
    Layer removed = std::move(found->second.layer);
    layers_.erase(found);
    return removed; // a leaf: nothing inherited from it
}

const Layer* LayerDatabase::find(std::string_view name) const
{
    const auto found = layers_.find(name);
    return found == layers_.end() ? nullptr : &found->second.layer;
}

std::vector<std::string> LayerDatabase::names() const
{
    std::vector<std::string> result;
    result.reserve(layers_.size());
    for (const auto& [name, node] : layers_) {
        (void)node;
        result.push_back(name);
    }
    return result;
}

std::vector<Layer> LayerDatabase::all() const
{
    std::vector<Layer> result;
    result.reserve(layers_.size());
    for (const auto& [name, node] : layers_) {
        (void)name;
        result.push_back(node.layer);
    }
    return result;
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
    // Any key beginning "name/". This used to test only the key straight
    // after `name`, on the belief that a child sorts immediately after its
    // parent - but "design 2" sorts between "design" and "design/surface", so
    // with both present "design" had no children and remove() orphaned them.
    std::string prefix(name);
    prefix += kLayerSeparator;
    const auto it = layers_.lower_bound(prefix);
    return it != layers_.end() && it->first.starts_with(prefix);
}

// A layer that is gone is neither shown nor editable: drawing entities that
// point at one would be drawing something the user cannot turn off or select
// through the layer tree, and editing them would edit what is not there.
bool LayerDatabase::effectivelyVisible(std::string_view name) const
{
    return resolve(name).shown;
}

bool LayerDatabase::effectivelyLocked(std::string_view name) const
{
    return resolve(name).locked;
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
        removed.push_back(found->second.layer);
        layers_.erase(found);
    }
    return removed; // a whole branch: nothing left inherits from it
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
        Layer layer = found->second.layer;
        layer.name = after;
        moved.push_back(std::move(layer));
    }
    for (const auto& [before, after] : mapping) {
        (void)after;
        layers_.erase(std::string(before));
    }
    const std::vector<std::string> newAncestors = layerAncestors(to);
    for (const std::string& ancestor : newAncestors) {
        layers_.try_emplace(ancestor, Node{Layer{ancestor, Color{}, true, false,
                                                 std::string(kContinuousLinetype), 0.25}});
    }
    for (Layer& layer : moved) {
        std::string key = layer.name;
        layers_.insert_or_assign(std::move(key), Node{std::move(layer)});
    }
    // The moved branch inherits from its new parents now, some perhaps just
    // created: refresh from the topmost of them.
    refreshSubtree(newAncestors.empty() ? to : std::string_view(newAncestors.front()));
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
        if (katana::core::equalsIgnoringCase(toString(head), name)) {
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
        if (katana::core::equalsIgnoringCase(linetype.name, reserved)) {
            return makeError(ErrorCode::InvalidArgument,
                             "that linetype name is reserved by the exchange format",
                             linetype.name);
        }
    }
    // Every string that reaches the model is UTF-8 (entity.hpp); a
    // description that is not cannot be saved, and the save would fail far
    // from here without naming the linetype (audit MOD-12).
    if (!isValidUtf8(linetype.description)) {
        return makeError(ErrorCode::InvalidArgument, "linetype description is not valid UTF-8",
                         linetype.name);
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

// ---- hatch patterns --------------------------------------------------------------

Status validate(const HatchPattern& pattern)
{
    if (auto status = validateName(pattern.name, "hatch pattern"); !status) {
        return status;
    }
    // Saved as JSON like the name (audit MOD-12).
    if (!isValidUtf8(pattern.description)) {
        return makeError(ErrorCode::InvalidArgument,
                         "hatch pattern description is not valid UTF-8", pattern.name);
    }
    if (pattern.solid && !pattern.families.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a solid hatch pattern cannot also carry line families", pattern.name);
    }
    // A pattern that draws nothing is almost always a half-finished definition,
    // and it would be invisible on the drawing with no error anywhere - the
    // silent failure PLAN.MD section 36 forbids. "none" is the one pattern
    // whose whole purpose is to draw nothing.
    if (pattern.drawsNothing() && pattern.name != kNoHatch) {
        return makeError(ErrorCode::InvalidArgument,
                         "a hatch pattern must be solid or have at least one line family",
                         pattern.name);
    }
    for (const HatchLineFamily& family : pattern.families) {
        if (!std::isfinite(family.angle) || !std::isfinite(family.offset)) {
            return makeError(ErrorCode::InvalidArgument,
                             "hatch family angle and offset must be finite", pattern.name);
        }
        if (!(family.spacing > 0.0) || !std::isfinite(family.spacing)) {
            return makeError(ErrorCode::InvalidArgument,
                             "hatch family spacing must be a positive length", pattern.name);
        }
    }
    return {};
}

Status HatchPatternPolicy::validate(const HatchPattern& pattern)
{
    return katana::entity::validate(pattern);
}

Status HatchPatternPolicy::checkUpdate(const HatchPattern& pattern)
{
    if (pattern.name == kNoHatch && !pattern.drawsNothing()) {
        return makeError(ErrorCode::InvalidArgument, "the \"none\" hatch pattern cannot be given "
                                                     "a fill");
    }
    return {};
}

void HatchPatternPolicy::seed(NamedMap<HatchPattern>& items)
{
    // Only "none" is built in, for the same reason only "continuous" is: the
    // ISO and ANSI pattern sets are defined in paper millimetres and inches,
    // and a survey drawing here is in metres, so anything shipped would be a
    // conversion of numbers not to hand or an invention that a test would then
    // pin as though it were a standard. Patterns come from the user or from an
    // imported file.
    HatchPattern none;
    none.name = std::string(kNoHatch);
    none.description = "Not hatched";
    items.emplace(none.name, std::move(none));
}

// ---- alignments ------------------------------------------------------------------

Status validate(const Alignment& alignment)
{
    if (auto status = validateName(alignment.name, "alignment"); !status) {
        return status;
    }
    if (!isValidUtf8(alignment.description)) {
        return makeError(ErrorCode::InvalidArgument, "alignment description is not valid UTF-8",
                         alignment.name);
    }
    // An alignment with no PIs is a name with nothing behind it. Two is the
    // least that has a direction; solveAlignment says so itself, and its
    // message is the one the user should read.
    auto solved = katana::geometry::solveAlignment(alignment.horizontal);
    if (!solved) {
        return makeError(solved.error().code, solved.error().message,
                         "alignment " + alignment.name +
                             (solved.error().context.empty() ? "" : ": " + solved.error().context));
    }
    if (alignment.vertical.has_value()) {
        auto profile = katana::geometry::solveProfile(*alignment.vertical);
        if (!profile) {
            return makeError(profile.error().code, profile.error().message,
                             "alignment " + alignment.name + " profile" +
                                 (profile.error().context.empty()
                                      ? ""
                                      : ": " + profile.error().context));
        }
    }
    return {};
}

Status AlignmentPolicy::validate(const Alignment& alignment)
{
    return katana::entity::validate(alignment);
}

// The set a viewport can paint. Names as the survey packages use them: a
// 12d "TOPO Natural Surface Point" style, for instance, is a cross.
const std::vector<std::string_view>& symbolNames()
{
    static const std::vector<std::string_view> names = {
        "circle", "square", "triangle", "diamond", "cross", "plus", "tick", "star",
        "dot",    "ring",   "tree",     "pole",    "manhole", "arrow", "flag", "target"};
    return names;
}

std::string_view builtInSymbolFor(std::string_view name)
{
    const std::string key = katana::core::lowered(name); // ASCII, whatever the locale
    const auto has = [&key](std::string_view word) { return key.find(word) != std::string::npos; };
    // Most specific first: "Pole - Light" is a pole, "Suspended Light" a
    // light, "Gully Pit Point" a pit and not a point.
    if (has("tree") || has("shrub") || has("palm")) {
        return "tree";
    }
    if (has("manhole") || has("pit") || has("chamber") || has("sump")) {
        return "manhole";
    }
    if (has("pole") || has("post") || has("column") || has("pier")) {
        return "pole";
    }
    if (has("mark") || has("station") || has("bench") || has("stns") || has("control")) {
        return "target";
    }
    if (has("sign") || has("flag")) {
        return "flag";
    }
    if (has("light") || has("lamp") || has("lantern")) {
        return "star";
    }
    if (has("valve") || has("hydrant") || has("tap") || has("meter")) {
        return "diamond";
    }
    if (has("bollard") || has("peg") || has("nail") || has("spike")) {
        return "dot";
    }
    if (has("point") || has("spot") || has("surface") || has("level") || has("invert") ||
        has("obvert")) {
        return "cross";
    }
    return "circle";
}

bool isBuiltInSymbolName(std::string_view name)
{
    if (name == kNoSymbol) {
        return true;
    }
    const auto& names = symbolNames();
    return std::find(names.begin(), names.end(), name) != names.end();
}

Status validate(const Style& style)
{
    if (auto status = validateName(style.name, "style"); !status) {
        return status;
    }
    if (auto status = validateLineWeight(style.lineWeight); !status) {
        return status;
    }
    // A symbol name is RESOLVED when the style is drawn, exactly as
    // `Style::linetype` already is, and not checked against a closed set
    // here. It stopped being a closed set when a project could load a 12d
    // symbol library: one production customisation alone names 473 of them,
    // and none is one of the sixteen Katana draws without a library
    // (PLAN.MD 20.3). A name with no definition behind it draws the plain
    // point mark and is reported by whoever looked it up - the same thing
    // that already happens for a linetype the document does not have.
    if (!isValidUtf8(style.symbol)) {
        return makeError(ErrorCode::InvalidArgument, "symbol name is not valid UTF-8",
                         style.name);
    }
    // The linetype is resolved when drawn, like the symbol, and for the same
    // reason is not checked against a table: a 12d linestyle lives in the
    // library, not the model. "ByLayer" (display.hpp) is accepted like any
    // other name - it is what a style that inherits its layer's linetype
    // says. Only the encoding is checked, as it is for every string here.
    if (!isValidUtf8(style.linetype) || !isValidUtf8(style.hatchPattern)) {
        return makeError(ErrorCode::InvalidArgument,
                         "linetype or hatch pattern name is not valid UTF-8", style.name);
    }
    if (!(std::isfinite(style.symbolSize) && style.symbolSize >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "symbol size must be finite and not negative",
                         std::to_string(style.symbolSize));
    }
    if (!isValidUtf8(style.description)) {
        return makeError(ErrorCode::InvalidArgument, "style description is not valid UTF-8",
                         style.name);
    }
    return {};
}

Status StylePolicy::validate(const Style& style)
{
    return katana::entity::validate(style);
}

// ---- PropertyDatabase ------------------------------------------------------------

Status PropertyDatabase::define(PropertyDefinition definition)
{
    if (auto status = validateName(definition.name, "property"); !status) {
        return status;
    }
    if (!isValidUtf8(definition.description)) {
        return makeError(ErrorCode::InvalidArgument, "property description is not valid UTF-8",
                         definition.name);
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
