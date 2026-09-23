#include "katana/entity/display.hpp"

#include "katana/core/text.hpp"

namespace katana::entity {

bool isByLayer(std::string_view linetype)
{
    return katana::core::equalsIgnoringCase(linetype, kByLayerLinetype);
}

std::string_view resolvedLinetype(const Layer* layer, const Style* style)
{
    if (style != nullptr && !isByLayer(style->linetype)) {
        return style->linetype;
    }
    // A layer has no layer above it to inherit from, so a layer that says
    // ByLayer - which nothing here offers, but a hand-made table could hold -
    // draws what a missing layer does, rather than naming a linetype nobody
    // can define.
    if (layer != nullptr && !isByLayer(layer->linetype)) {
        return layer->linetype;
    }
    return kContinuousLinetype;
}

std::string_view resolvedHatchPattern(const Layer* layer, const Style* style)
{
    if (style != nullptr && !style->hatchPattern.empty()) {
        return style->hatchPattern;
    }
    if (layer != nullptr && !layer->hatchPattern.empty()) {
        return layer->hatchPattern;
    }
    return kNoHatch;
}

ResolvedDisplay resolveDisplay(const Model& model, const Entity& entity)
{
    ResolvedDisplay resolved;

    const Layer* layer = model.layers.find(entity.layer);
    const Style* style = entity.style.empty() ? nullptr : model.styles.find(entity.style);

    if (layer != nullptr) {
        resolved.color = layer->color;
        resolved.lineWeight = layer->lineWeight;
    }
    // A named style overrides the layer. lineWeight is not optional on a
    // Style, so naming a style takes it; the linetype is taken unless the
    // style says ByLayer; colour is optional and an empty one means "keep
    // using the layer's", which is what ByLayer means.
    if (style != nullptr) {
        resolved.lineWeight = style->lineWeight;
        if (style->color.has_value()) {
            resolved.color = *style->color;
        }
        // A symbol is the style's alone: a layer has none, and a point
        // with no style draws the plain mark.
        resolved.symbol = style->symbol;
        resolved.symbolSize = style->symbolSize;
    }
    resolved.linetype = std::string(resolvedLinetype(layer, style));
    resolved.hatchPattern = std::string(resolvedHatchPattern(layer, style));

    // The entity's own colour overrides everything. It is the only per-entity
    // override the model has; there is deliberately no per-entity lineWeight or
    // linetype, because those belong to the layer or the style.
    if (entity.color.has_value()) {
        resolved.color = *entity.color;
    }

    return resolved;
}

} // namespace katana::entity
