#include "katana/cad/hatching.hpp"

#include "katana/entity/display.hpp"

namespace katana::cad {

HatchDrawing hatchDrawing(const katana::entity::HatchPattern& pattern,
                          const HatchOptions& options)
{
    if (pattern.drawsNothing()) {
        return HatchDrawing::None;
    }
    if (pattern.solid) {
        return HatchDrawing::Solid;
    }
    // A non-finite or non-positive scale is a view that has not been set up
    // yet. Drawing the lines then would mean asking for a family whose spacing
    // in pixels is meaningless, so the cheap answer is the safe one.
    if (!(options.viewScale > 0.0)) {
        return HatchDrawing::Solid;
    }
    for (const katana::entity::HatchLineFamily& family : pattern.families) {
        if (family.spacing * options.viewScale < options.minimumSpacingPixels) {
            return HatchDrawing::Solid;
        }
    }
    return HatchDrawing::Lines;
}

std::vector<Segment2> hatchSegments(const Polyline2& boundary,
                                    const katana::entity::HatchPattern& pattern)
{
    std::vector<Segment2> segments;
    if (pattern.solid || pattern.drawsNothing()) {
        return segments;
    }
    for (const katana::entity::HatchLineFamily& family : pattern.families) {
        auto lines = katana::geometry::hatchLines(boundary, family.angle, family.spacing,
                                                  family.offset);
        if (!lines) {
            continue; // see the header: a renderer has no use for the reason
        }
        segments.insert(segments.end(), lines->begin(), lines->end());
    }
    return segments;
}

const katana::entity::HatchPattern* resolveHatchPattern(const katana::entity::Model& model,
                                                        const katana::entity::Entity& entity)
{
    const auto display = katana::entity::resolveDisplay(model, entity);
    const katana::entity::HatchPattern* pattern = model.hatchPatterns.find(display.hatchPattern);
    // An entity may name a pattern that has been removed, exactly as it may
    // name a removed layer. Not hatched is the right reading of that, and it
    // is what the document drew before the pattern existed.
    if (pattern == nullptr || pattern->drawsNothing()) {
        return nullptr;
    }
    return pattern;
}

} // namespace katana::cad
