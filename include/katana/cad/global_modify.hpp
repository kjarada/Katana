#pragma once

// Global Modify (the owner's request of 2026-09-25: "a global modification
// tool that can act on data on a selected view, layers, selected features,
// etc., that can change styles, symbols, layers, colour, etc.").
//
// One request says three things, and each is a plain value here so that the
// window's dialog, the command line's MODIFY and a test all state it the same
// way:
//
//   WHERE   ModifyScope: the selection, what a view draws (its hidden layers
//           and, optionally, only what lies in its visible area), a list of
//           layers (with or without the layers beneath them), or the whole
//           drawing.
//   WHICH   ModifyFilter: narrows what the scope holds by type, layer, style,
//           colour, property and text, so "every point coded TREE on the
//           survey layers" is one request.
//   WHAT    GlobalModify: the entities' own attributes (layer, colour, style,
//           visibility, properties, text height, the symbol a point draws),
//           the layers they sit on (colour, linetype, weight, hatch,
//           dimension style, shown, locked) and the styles they wear
//           (colour, linetype, weight, hatch, symbol, symbol size).
//
// planGlobalModify turns the three into ONE command, so one Undo puts every
// entity, layer and style back (Rule 2): nothing here edits the model. What
// the plan reports - how many matched, how many change, which layers and
// styles are rewritten, what was left alone and why - is worked out before
// anything runs, so the dialog can show it as a preview and the command line
// can print it.
//
// FIELDS THAT ARE ABSENT ARE LEFT ALONE. Every field of a change is a
// std::optional; nullopt means "not asked", never "clear it". Where the value
// itself may be ByLayer, the value is an optional too: a colour field of
// `std::optional<std::optional<Color>>` holding `std::nullopt` inside is
// "make it ByLayer", and an empty outer optional is "do not touch the colour".
// (The same reading as Entity::color, where an empty optional is ByLayer.)
//
// WHAT IS NOT CHANGED, AND IS SAID. An entity on a locked layer is counted and
// left as it is, as Match Properties leaves one (modify_properties.cpp) -
// unless the same request unlocks that layer, in which case it is changed,
// because the layer is unlocked first. A request that locks a layer locks it
// after its entities are changed. A symbol is drawn only by a point (a line
// wearing a symbol style draws it at every vertex, D8, but "put a tree on
// these" means the points), so the other entities are counted, not moved.
//
// STYLE DEFINITIONS REACH BEYOND THE SCOPE. A style is shared: changing the
// symbol of the style the selected points wear changes it for every entity
// wearing that style, selected or not. The plan counts those others
// (`styleReachesOthers`) so no front end can hide it, and the dialog says it
// beside the button.

#include <cstddef>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// ---- where -------------------------------------------------------------------------------

enum class ScopeKind {
    Selection, // the document's selection
    View,      // what one view draws
    Layers,    // everything on the named layers
    Drawing,   // every entity in the drawing
};

// "selection", "view", "layers", "drawing".
[[nodiscard]] std::string_view toString(ScopeKind kind);

struct ModifyScope {
    ScopeKind kind = ScopeKind::Selection;

    // View: the layers that view hides of its own (ViewState::layers), on top
    // of the document's visibility. Null is a view that hides nothing of its
    // own, so the document rule alone decides what it draws.
    const LayerOverrides* view = nullptr;
    // View: when set, only entities whose extent overlaps this box - a plan
    // view's visible area (ViewTransform::visibleWorldBounds), so "what I can
    // see" means what is on screen, not everything on its layers.
    std::optional<katana::geometry::Box2> area{};

    // Layers: the layer paths, as the layer panel names them. Each must exist.
    std::vector<std::string> layers{};
    // Layers: a layer also takes everything nested beneath it, as hiding
    // "design" hides "design/surface/tin1" (layer_path.hpp).
    bool sublayers = true;
};

// ---- which -------------------------------------------------------------------------------

// A text pattern: '*' is any run of characters, '?' any one; ASCII letters
// match either case. What 12d and CAD programs take in a "name" field, and
// what the survey codes' keys already use ("AC*"). A pattern with neither is
// an exact match, apart from case.
[[nodiscard]] bool matchesPattern(std::string_view text, std::string_view pattern);

struct ModifyFilter {
    // Empty: every type.
    std::set<katana::entity::EntityType> types{};
    // Layer names or patterns; an entity passes on ANY of them. Empty: every
    // layer. Unlike ModifyScope::layers these are patterns and match the
    // layer's full path, so "survey/*" takes everything beneath "survey".
    std::vector<std::string> layers{};
    // The style an entity wears, as a pattern; "ByLayer" (any case) or ""
    // is an entity with no style of its own.
    std::optional<std::string> style{};
    // The entity's own colour; an inner nullopt is ByLayer.
    std::optional<std::optional<katana::entity::Color>> colour{};
    // An entity carrying this property (exact key). With `propertyValue`
    // also, its value as entity::toString prints it must match that pattern.
    std::optional<std::string> property{};
    std::optional<std::string> propertyValue{};
    // A Text entity's text, or a Dimension's override, as a pattern. Other
    // entities do not pass when it is set.
    std::optional<std::string> text{};
    // Leave out entities that are not drawn: switched off themselves, or on
    // a layer that is hidden. The View scope always leaves them out, since a
    // view does not draw them.
    bool drawnOnly = false;

    [[nodiscard]] bool empty() const
    {
        return types.empty() && layers.empty() && !style && !colour && !property &&
               !propertyValue && !text && !drawnOnly;
    }
};

// ---- what --------------------------------------------------------------------------------

// What a point draws, through the style it is moved into (symbol_assign.hpp).
struct SymbolModify {
    std::string name{}; // a library symbol or a built-in shape
    double size = 0.0;  // model units; 0 is the definition's own
};

struct EntityModify {
    // Moves them onto this layer, created (with default settings) when the
    // drawing has none of that name.
    std::optional<std::string> layer{};
    std::optional<std::optional<katana::entity::Color>> colour{};
    // A style the drawing has, or "" for ByLayer.
    std::optional<std::string> style{};
    std::optional<bool> visible{};
    // Set on every entity in scope, replacing a value of another type.
    std::vector<std::pair<std::string, katana::entity::PropertyValue>> setProperties{};
    std::vector<std::string> removeProperties{};
    // Text entities: the height of the text, in model units.
    std::optional<double> textHeight{};
    // Points: the symbol drawn at them. Applied after `style`, so a point
    // given both wears the symbol's style.
    std::optional<SymbolModify> symbol{};

    [[nodiscard]] bool empty() const
    {
        return !layer && !colour && !style && !visible && setProperties.empty() &&
               removeProperties.empty() && !textHeight && !symbol;
    }
};

// Applied to the layers the matched entities sit on - or, for the Layers
// scope, to the scope's layers themselves, entities or not.
struct LayerModify {
    std::optional<katana::entity::Color> colour{};
    // A model linetype or a loaded library linestyle (checkLinetypeName).
    std::optional<std::string> linetype{};
    std::optional<double> lineWeight{}; // millimetres on paper
    std::optional<std::string> hatchPattern{};
    // A dimension style the drawing has, or "" for the document default.
    std::optional<std::string> dimensionStyle{};
    std::optional<bool> visible{};
    std::optional<bool> locked{};

    [[nodiscard]] bool empty() const
    {
        return !colour && !linetype && !lineWeight && !hatchPattern && !dimensionStyle &&
               !visible && !locked;
    }
};

// Applied to the styles the matched entities wear (ByLayer is no style).
struct StyleModify {
    std::optional<std::optional<katana::entity::Color>> colour{};
    // A model linetype, a library linestyle or "ByLayer" (the layer's).
    std::optional<std::string> linetype{};
    std::optional<double> lineWeight{};
    // A hatch pattern the drawing has, or "" for ByLayer.
    std::optional<std::string> hatchPattern{};
    std::optional<std::string> symbol{};
    std::optional<double> symbolSize{};

    [[nodiscard]] bool empty() const
    {
        return !colour && !linetype && !lineWeight && !hatchPattern && !symbol && !symbolSize;
    }
};

struct GlobalModify {
    EntityModify entities{};
    LayerModify layers{};
    StyleModify styles{};

    [[nodiscard]] bool empty() const
    {
        return entities.empty() && layers.empty() && styles.empty();
    }
};

// ---- the plan ----------------------------------------------------------------------------

struct GlobalModifyPlan {
    // Everything the scope and the filter took, ascending.
    std::vector<katana::entity::EntityId> matched{};
    // Of those, the entities whose own attributes change, ascending.
    std::vector<katana::entity::EntityId> changed{};
    // Left alone: on a locked layer the request does not unlock.
    std::size_t locked = 0;
    // Left alone by the symbol: matched, but not points.
    std::size_t notPoints = 0;
    // Left alone by the text height: matched, but not text.
    std::size_t notText = 0;
    // The layers and styles whose definitions change, ascending.
    std::vector<std::string> layersChanged{};
    std::vector<std::string> stylesChanged{};
    // Entities OUTSIDE `matched` that wear a style in stylesChanged, and so
    // are redrawn by it too.
    std::size_t styleReachesOthers = 0;
    // A layer or style the command makes, when it does.
    std::optional<std::string> createsLayer{};
    std::optional<std::string> createsStyle{};

    // ONE undo step doing all of it; null when nothing would change, so no
    // empty step is ever pushed.
    katana::commands::CommandPtr command{};

    // "12 matched: 10 entities, 2 layers and 1 style change; 2 on a locked
    // layer left as they are". What the dialog shows as its preview and the
    // command line prints.
    [[nodiscard]] std::string summary() const;
};

// What `scope` and `filter` take, ascending by id. Fails with NotFound for a
// Layers scope naming a layer the drawing lacks, and with InvalidArgument for
// a Layers scope naming none.
[[nodiscard]] katana::core::Result<std::vector<katana::entity::EntityId>>
matchEntities(const Document& document, const ModifyScope& scope, const ModifyFilter& filter);

// The command and its report. Fails, changing nothing, for a request the
// drawing would refuse - a style, linetype, hatch pattern or dimension style
// it does not have, a layer name that is not a valid path, a weight, height
// or size that is not a positive finite number (a weight and a symbol size
// may be 0) - naming the field. Asking for no change at all is refused too:
// it is a mistake in the request, not a request to do nothing. Matching
// nothing is not a failure: the plan says so and carries no command.
[[nodiscard]] katana::core::Result<GlobalModifyPlan> planGlobalModify(const Document& document,
                                                                      const ModifyScope& scope,
                                                                      const ModifyFilter& filter,
                                                                      const GlobalModify& change);

} // namespace katana::cad
