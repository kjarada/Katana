//! The attribute layout of each product class the export can write: whether
//! it has a Tag (every IfcElement does) and a PredefinedType. A class outside
//! this list is refused (ClassificationRule validation in export.rs) rather
//! than written with the wrong number of attributes - which a reader would
//! reject or, worse, read shifted. Each row was checked against the
//! IFC4X3_ADD2 schema (tools/check_ifc.py runs IfcOpenShell's validation of
//! every file the tests write; tools/ifc_product_classes.py --layouts checks
//! the rows of the C++ header product_layout.hpp, which these are, against
//! the schema).

use crate::predefined_types::PREDEFINED_TYPES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProductLayout {
    pub(crate) entity: &'static str,
    pub(crate) has_tag: bool,
    pub(crate) has_predefined_type: bool,
    /// An IfcFeatureElement: it has "no independent containment relationship
    /// to the spatial structure" (IFC 4.3.2, 5.4.3.30, IfcFeatureElement) and
    /// exists only on the element it voids (IfcFeatureElementSubtraction,
    /// through IfcRelVoidsElement) or adheres to (IfcSurfaceFeature, through
    /// IfcRelAdheresToElement) - an inverse the schema makes mandatory. Only
    /// the road model, which names that element, writes one (road.rs); a
    /// drawing entity's rule may not name one, since a line on a layer has
    /// nothing to be a feature of.
    pub(crate) feature: bool,
    /// A feature that VOIDS its host (IfcFeatureElementSubtraction), rather
    /// than adhering to it. Such a void takes no material:
    /// IfcRelAssociatesMaterial's NoVoidElement rule refuses an
    /// IfcFeatureElementSubtraction among its objects.
    pub(crate) voids: bool,
}

const fn layout(entity: &'static str, has_tag: bool, has_predefined_type: bool) -> ProductLayout {
    ProductLayout {
        entity,
        has_tag,
        has_predefined_type,
        feature: false,
        voids: false,
    }
}

const fn feature(entity: &'static str, voids: bool) -> ProductLayout {
    ProductLayout {
        entity,
        has_tag: true,
        has_predefined_type: true,
        feature: true,
        voids,
    }
}

/// Classes IFC 4.3 deprecates (IFC102 of the buildingSMART validation
/// service: IfcCivilElement, IfcElectricDistributionBoard, IfcFlowSegment and
/// its siblings, IfcTextLiteral ...) are deliberately absent.
///
/// The road model's classes (docs/ifc.md, "Road model"), by their sections
/// of the IFC 4.3.2.0 documentation: IfcCourse 6.6.3.2, IfcEarthworksCut
/// 6.6.3.4, IfcEarthworksFill 6.6.3.6, IfcPavement 6.6.3.12, IfcKerb 7.9.3.1,
/// IfcFooting 7.11.3.5, IfcSurfaceFeature 7.11.3.16, IfcLightFixture 7.4.3.33;
/// and by their type enumerations IfcSign (6.6.2.8), IfcRailing (6.1.2.14),
/// IfcWall (6.1.2.22), IfcSlab (6.1.2.19), IfcColumn (6.1.2.6), IfcMember
/// (6.1.2.12), IfcPipeSegment (7.5.2.26). Each is an IfcElement with a Tag and
/// an optional PredefinedType: nine attributes in all.
pub(crate) const PRODUCT_LAYOUTS: [ProductLayout; 28] = [
    layout("IfcAnnotation", false, true),
    layout("IfcBorehole", true, false),
    layout("IfcCableCarrierSegment", true, true),
    layout("IfcCableSegment", true, true),
    layout("IfcColumn", true, true),
    layout("IfcCourse", true, true),
    layout("IfcDistributionChamberElement", true, true),
    layout("IfcDistributionFlowElement", true, false),
    feature("IfcEarthworksCut", true),
    layout("IfcEarthworksFill", true, true),
    layout("IfcFireSuppressionTerminal", true, true),
    layout("IfcFooting", true, true),
    layout("IfcGeographicElement", true, true),
    layout("IfcJunctionBox", true, true),
    layout("IfcKerb", true, true),
    layout("IfcLightFixture", true, true),
    layout("IfcMember", true, true),
    layout("IfcPavement", true, true),
    layout("IfcPipeFitting", true, true),
    layout("IfcPipeSegment", true, true),
    layout("IfcRailing", true, true),
    layout("IfcSign", true, true),
    layout("IfcSlab", true, true),
    feature("IfcSurfaceFeature", false),
    layout("IfcValve", true, true),
    layout("IfcWall", true, true),
    layout("IfcWasteTerminal", true, true),
    layout("IfcBuildingElementProxy", true, true),
];

pub(crate) fn find_product_layout(entity: &str) -> Option<ProductLayout> {
    PRODUCT_LAYOUTS
        .iter()
        .find(|layout| layout.entity == entity)
        .copied()
}

/// Callers have checked the class with find_product_layout first; an
/// unchecked one is written as an element with a Tag and a PredefinedType.
pub(crate) fn product_layout(entity: &str) -> ProductLayout {
    find_product_layout(entity).unwrap_or(ProductLayout {
        entity: "",
        has_tag: true,
        has_predefined_type: true,
        feature: false,
        voids: false,
    })
}

/// The values of `entity`'s PredefinedType enumeration in IFC4X3_ADD2,
/// separated by blanks (predefined_types.rs, generated from the schema by
/// tools/ifc_product_classes.py --predefined); None for a class without one.
/// Shared by a rules file's check (classification.rs) and the road model's
/// (road.rs), which check the classes they name against the same table.
pub(crate) fn predefined_types_of(entity: &str) -> Option<&'static str> {
    PREDEFINED_TYPES
        .iter()
        .find(|(name, _)| *name == entity)
        .map(|(_, values)| *values)
}

/// True when `values` (as predefined_types_of gives them) lists `word`.
pub(crate) fn lists_word(values: &str, word: &str) -> bool {
    let bytes = values.as_bytes();
    let mut at = 0;
    while at <= bytes.len() {
        let end = crate::support::find_from(bytes, b" ", at).unwrap_or(bytes.len());
        if &values[at..end] == word {
            return true;
        }
        at = end + 1;
    }
    false
}
