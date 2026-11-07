//! The authoritative CAD domain model: entities plus the tables they refer
//! to. Everything else (views, renderer, storage, AI) reads or mutates this
//! through the command system.

use crate::annotation::{LabelRuleDatabase, LabelStyleDatabase, TextStyleDatabase};
use crate::entity_database::EntityDatabase;
use crate::tables::{
    AlignmentDatabase, DimensionStyleDatabase, HatchPatternDatabase, LayerDatabase,
    LinetypeDatabase, PropertyDatabase, StyleDatabase,
};

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub entities: EntityDatabase,
    pub layers: LayerDatabase,
    pub styles: StyleDatabase,
    pub linetypes: LinetypeDatabase,
    pub dimension_styles: DimensionStyleDatabase,
    pub hatch_patterns: HatchPatternDatabase,
    pub alignments: AlignmentDatabase,
    pub properties: PropertyDatabase,
    // The annotation tables (annotation.rs, docs/annotation.md).
    pub text_styles: TextStyleDatabase,
    pub label_styles: LabelStyleDatabase,
    pub label_rules: LabelRuleDatabase,
}

impl Model {
    pub fn new() -> Self {
        Self::default()
    }

    /// Back to the state of a new, empty document. Entity ids are not reused.
    pub fn reset(&mut self) {
        self.entities.clear();
        self.layers.reset();
        self.styles.reset();
        self.linetypes.reset();
        self.dimension_styles.reset();
        self.hatch_patterns.reset();
        self.alignments.reset();
        self.properties.reset();
        self.text_styles.reset();
        self.label_styles.reset();
        self.label_rules.reset();
    }

    /// Replaces this model's contents with another's, preserving this model's
    /// entity observer (EntityDatabase::adopt_contents says why a plain
    /// assignment is not good enough).
    ///
    /// ADDING A TABLE ABOVE MEANS ADDING A LINE HERE. A table left out is
    /// simply dropped every time a project is loaded - with no error anywhere.
    /// That is not hypothetical: hatch patterns were lost exactly this way
    /// (docs/model.md). The destructuring below makes the compiler hold that
    /// rule, which the C++ could not.
    pub fn adopt_contents(&mut self, other: Model) {
        let Model {
            entities,
            layers,
            styles,
            linetypes,
            dimension_styles,
            hatch_patterns,
            alignments,
            properties,
            text_styles,
            label_styles,
            label_rules,
        } = other;
        self.layers = layers;
        self.styles = styles;
        self.linetypes = linetypes;
        self.dimension_styles = dimension_styles;
        self.hatch_patterns = hatch_patterns;
        self.alignments = alignments;
        self.properties = properties;
        self.text_styles = text_styles;
        self.label_styles = label_styles;
        self.label_rules = label_rules;
        self.entities.adopt_contents(entities);
    }
}
