#pragma once

// The authoritative CAD domain model: entities plus the tables they refer to.
// Everything else (views, renderer, storage, AI) reads or mutates this through
// the command system.

#include "katana/entity/entity_database.hpp"
#include "katana/entity/tables.hpp"

namespace katana::entity {

struct Model {
    EntityDatabase entities;
    LayerDatabase layers;
    StyleDatabase styles;
    PropertyDatabase properties;

    // Back to the state of a new, empty document. Entity ids are not reused.
    void reset()
    {
        entities.clear();
        layers.reset();
        styles.reset();
        properties.reset();
    }

    // Replaces this model's contents with another's, preserving this model's
    // entity observer. See EntityDatabase::adoptContents for why a plain
    // move-assignment is not good enough.
    void adoptContents(Model&& other)
    {
        layers = std::move(other.layers);
        styles = std::move(other.styles);
        properties = std::move(other.properties);
        entities.adoptContents(std::move(other.entities));
    }
};

} // namespace katana::entity
