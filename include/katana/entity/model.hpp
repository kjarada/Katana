#pragma once

// The authoritative CAD domain model: entities plus the tables they refer to.
// Everything else (views, renderer, storage, AI) reads or mutates this through
// the command system.

#include "katana/entity/annotation.hpp"
#include "katana/entity/entity_database.hpp"
#include "katana/entity/tables.hpp"

namespace katana::entity {

struct Model {
    EntityDatabase entities;
    LayerDatabase layers;
    StyleDatabase styles;
    LinetypeDatabase linetypes;
    DimensionStyleDatabase dimensionStyles;
    HatchPatternDatabase hatchPatterns;
    AlignmentDatabase alignments;
    PropertyDatabase properties;
    // The annotation tables (entity/annotation.hpp, docs/annotation.md).
    TextStyleDatabase textStyles;
    LabelStyleDatabase labelStyles;
    LabelRuleDatabase labelRules;

    // Back to the state of a new, empty document. Entity ids are not reused.
    void reset()
    {
        entities.clear();
        layers.reset();
        styles.reset();
        linetypes.reset();
        dimensionStyles.reset();
        hatchPatterns.reset();
        alignments.reset();
        properties.reset();
        textStyles.reset();
        labelStyles.reset();
        labelRules.reset();
    }

    // Replaces this model's contents with another's, preserving this model's
    // entity observer. See EntityDatabase::adoptContents for why a plain
    // move-assignment is not good enough.
    // ADDING A TABLE ABOVE MEANS ADDING A LINE HERE. This list is written out
    // by hand and the compiler does not check it, so a table left out is
    // simply dropped every time a project is loaded - with no error anywhere.
    // That is not hypothetical: hatchPatterns was lost exactly this way, and
    // the only thing that caught it was a round-trip test that saved a pattern
    // and looked for it again. See docs/model.md.
    void adoptContents(Model&& other)
    {
        layers = std::move(other.layers);
        styles = std::move(other.styles);
        linetypes = std::move(other.linetypes);
        dimensionStyles = std::move(other.dimensionStyles);
        hatchPatterns = std::move(other.hatchPatterns);
        alignments = std::move(other.alignments);
        properties = std::move(other.properties);
        textStyles = std::move(other.textStyles);
        labelStyles = std::move(other.labelStyles);
        labelRules = std::move(other.labelRules);
        entities.adoptContents(std::move(other.entities));
    }
};

} // namespace katana::entity
