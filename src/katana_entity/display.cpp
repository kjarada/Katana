#include "katana/entity/display.hpp"

namespace katana::entity {

ResolvedDisplay resolveDisplay(const Model& model, const Entity& entity)
{
    ResolvedDisplay resolved;

    const Layer* layer = model.layers.find(entity.layer);
    if (layer != nullptr) {
        resolved.color = layer->color;
        resolved.lineWeight = layer->lineWeight;
        resolved.linetype = layer->linetype;
    }

    // A named style overrides the layer. lineWeight and linetype are not
    // optional on a Style, so naming a style takes both; colour is optional and
    // an empty one means "keep using the layer's", which is what ByLayer means.
    if (!entity.style.empty()) {
        if (const Style* style = model.styles.find(entity.style); style != nullptr) {
            resolved.lineWeight = style->lineWeight;
            resolved.linetype = style->linetype;
            if (style->color.has_value()) {
                resolved.color = *style->color;
            }
        }
    }

    // The entity's own colour overrides everything. It is the only per-entity
    // override the model has; there is deliberately no per-entity lineWeight or
    // linetype, because those belong to the layer or the style.
    if (entity.color.has_value()) {
        resolved.color = *entity.color;
    }

    return resolved;
}

} // namespace katana::entity
