#pragma once

// How an entity's drawn appearance is resolved (PLAN.MD Phase 09).
//
// There must be exactly ONE answer to "what colour is this entity, how thick is
// it, and what linetype does it use", because every renderer has to agree. Before
// this existed there were three different answers:
//
//   * src/katana_cad/scene.cpp used the entity's own colour or a fixed default,
//     ignoring the layer AND the style entirely - so in the 3D view an entity on
//     a red layer drew grey, because ByLayer is the default and nothing resolved
//     it;
//   * src/katana_qt/viewport_widget.cpp used the entity's own colour or the
//     layer's, ignoring the style;
//   * nothing at all read Style::color, Style::lineWeight or Style::linetype.
//     The Style table was validated on write and persisted faithfully, and was
//     inert: a style could not change how anything looked.
//
// THE CHAIN, outermost override first. This is the DXF model (ByLayer /
// ByBlock), which is what a CAD user expects and what an exchange format
// assumes:
//
//   colour      entity.color, else the style's colour if it has one, else the
//               layer's colour.
//   lineWeight  the style's if the entity names one, else the layer's.
//   linetype    the style's if the entity names one and it is not "ByLayer",
//               else the layer's.
//   hatch       the style's if it names one, else the layer's.
//
// Colour and the hatch pattern are the two that are optional on a Style, which
// is why they are the ones that can fall through a named style to the layer.
// That asymmetry is in the Style type itself (`std::optional<Color> color` is
// documented "empty: ByLayer", and so is `hatchPattern`), not invented here.
//
// A linetype has no empty value to say "ByLayer" with - "" is a name like any
// other in the tables - so a Style says it with the word itself, which DXF
// already reserves (group code 6) and validate(Linetype) already refuses as a
// linetype NAME. A symbol-only or colour-only style used to override the
// layer's linetype with "continuous" whether it meant to or not; one whose
// linetype is ByLayer now leaves it alone. The weight has no such word: a
// sentinel weight would be a storage change, and those are deferred.
//
// A missing layer or style resolves to the defaults rather than dereferencing
// null. An entity can reference a layer that has been removed - the model
// permits it, and a renderer must not crash on it.

#include <string>
#include <string_view>

#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"

namespace katana::entity {

// What a Style::linetype says to inherit the layer's linetype.
inline constexpr std::string_view kByLayerLinetype = "ByLayer";
// In any case, because the reservation it relies on is in any case: no
// linetype can be called "BYLAYER" either, so the spelling cannot be taken
// for a name.
[[nodiscard]] bool isByLayer(std::string_view linetype);

struct ResolvedDisplay {
    Color color{};
    double lineWeight = 0.25;                       // millimetres on paper
    std::string linetype{kContinuousLinetype};
    // "none" when the entity is not hatched, never empty, so a renderer looks
    // the name up rather than testing for emptiness first.
    std::string hatchPattern{kNoHatch};
    // For a point: the symbol its style draws it with, kNoSymbol for the
    // viewport's plain mark; and the size, 0 for the viewport's own.
    std::string symbol{kNoSymbol};
    double symbolSize = 0.0;

    friend bool operator==(const ResolvedDisplay&, const ResolvedDisplay&) = default;
};

// Never fails: every step has a defined fallback, ending at the built-in
// defaults. Returning a Result would make every draw call check something that
// cannot happen, and would tempt a renderer into ignoring it.
[[nodiscard]] ResolvedDisplay resolveDisplay(const Model& model, const Entity& entity);

// The two NAMES of the chain above, for a layer and a style already found
// (either may be null: a missing layer, or no style or a missing one). They
// are what resolveDisplay itself reads, so a caller that counts entities
// per linetype or hatch - entity::tableUsage - cannot come to a different
// answer from the one the entities are drawn with. They return views into
// the tables, or the built-in names, and copy nothing.
[[nodiscard]] std::string_view resolvedLinetype(const Layer* layer, const Style* style);
[[nodiscard]] std::string_view resolvedHatchPattern(const Layer* layer, const Style* style);

} // namespace katana::entity
