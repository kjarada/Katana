#pragma once

// The attribute layout of each product class the export can write: whether
// it has a Tag (every IfcElement does) and a PredefinedType. A class outside
// this list is refused (ClassificationRule validation in export.cpp) rather
// than written with the wrong number of attributes - which a reader would
// reject or, worse, read shifted. Each row was checked against the
// IFC4X3_ADD2 schema (tools/check_ifc.py runs IfcOpenShell's validation of
// every file the tests write; tools/ifc_product_classes.py --layouts checks
// the rows themselves against the schema).

#include <optional>
#include <string_view>

namespace katana::ifc::detail {

struct ProductLayout {
    std::string_view entity;
    bool hasTag = true;
    bool hasPredefinedType = true;
    // An IfcFeatureElement: it has "no independent containment relationship
    // to the spatial structure" (IFC 4.3.2, 5.4.3.30, IfcFeatureElement) and
    // exists only on the element it voids (IfcFeatureElementSubtraction,
    // through IfcRelVoidsElement) or adheres to (IfcSurfaceFeature, through
    // IfcRelAdheresToElement) - an inverse the schema makes mandatory. Only
    // the road model, which names that element, writes one (road.cpp); a
    // drawing entity's rule may not name one, since a line on a layer has
    // nothing to be a feature of.
    bool feature = false;
};

// Classes IFC 4.3 deprecates (IFC102 of the buildingSMART validation
// service: IfcCivilElement, IfcElectricDistributionBoard, IfcFlowSegment and
// its siblings, IfcTextLiteral ...) are deliberately absent.
//
// The road model's classes (docs/ifc.md, "Road model"), by their sections
// of the IFC 4.3.2.0 documentation: IfcCourse 6.6.3.2, IfcEarthworksCut
// 6.6.3.4, IfcEarthworksFill 6.6.3.6, IfcPavement 6.6.3.12, IfcKerb 7.9.3.1,
// IfcFooting 7.11.3.5, IfcSurfaceFeature 7.11.3.16, IfcLightFixture 7.4.3.33;
// and by their type enumerations IfcSign (6.6.2.8), IfcRailing (6.1.2.14),
// IfcWall (6.1.2.22), IfcSlab (6.1.2.19), IfcColumn (6.1.2.6), IfcMember
// (6.1.2.12), IfcPipeSegment (7.5.2.26). Each is an IfcElement with a Tag and
// an optional PredefinedType: nine attributes in all.
inline constexpr ProductLayout kProductLayouts[] = {
    {"IfcAnnotation", false, true},
    {"IfcBorehole", true, false},
    {"IfcCableCarrierSegment", true, true},
    {"IfcCableSegment", true, true},
    {"IfcColumn", true, true},
    {"IfcCourse", true, true},
    {"IfcDistributionChamberElement", true, true},
    {"IfcDistributionFlowElement", true, false},
    {"IfcEarthworksCut", true, true, true},
    {"IfcEarthworksFill", true, true},
    {"IfcFireSuppressionTerminal", true, true},
    {"IfcFooting", true, true},
    {"IfcGeographicElement", true, true},
    {"IfcJunctionBox", true, true},
    {"IfcKerb", true, true},
    {"IfcLightFixture", true, true},
    {"IfcMember", true, true},
    {"IfcPavement", true, true},
    {"IfcPipeFitting", true, true},
    {"IfcPipeSegment", true, true},
    {"IfcRailing", true, true},
    {"IfcSign", true, true},
    {"IfcSlab", true, true},
    {"IfcSurfaceFeature", true, true, true},
    {"IfcValve", true, true},
    {"IfcWall", true, true},
    {"IfcWasteTerminal", true, true},
    {"IfcBuildingElementProxy", true, true},
};

[[nodiscard]] inline std::optional<ProductLayout> findProductLayout(std::string_view entity)
{
    for (const ProductLayout& layout : kProductLayouts) {
        if (layout.entity == entity) {
            return layout;
        }
    }
    return std::nullopt;
}

// Callers have checked the class with findProductLayout first; an unchecked
// one is written as an element with a Tag and a PredefinedType.
[[nodiscard]] inline ProductLayout productLayout(std::string_view entity)
{
    return findProductLayout(entity).value_or(ProductLayout{entity, true, true, false});
}

// The values of `entity`'s PredefinedType enumeration in IFC4X3_ADD2,
// separated by blanks (predefined_types.inc, generated from the schema by
// tools/ifc_product_classes.py --predefined); nullopt for a class without
// one. classification.cpp.
[[nodiscard]] std::optional<std::string_view> predefinedTypesOf(std::string_view entity);
// True when `values` (as predefinedTypesOf gives them) lists `word`.
[[nodiscard]] bool listsWord(std::string_view values, std::string_view word);

} // namespace katana::ifc::detail
