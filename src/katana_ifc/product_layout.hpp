#pragma once

// The attribute layout of each product class the export can write: whether
// it has a Tag (every IfcElement does) and a PredefinedType. A class outside
// this list is refused (ClassificationRule validation in export.cpp) rather
// than written with the wrong number of attributes - which a reader would
// reject or, worse, read shifted. Each row was checked against the
// IFC4X3_ADD2 schema (tools/check_ifc.py runs IfcOpenShell's validation of
// every file the tests write).

#include <optional>
#include <string_view>

namespace katana::ifc::detail {

struct ProductLayout {
    std::string_view entity;
    bool hasTag = true;
    bool hasPredefinedType = true;
};

// Classes IFC 4.3 deprecates (IFC102 of the buildingSMART validation
// service: IfcCivilElement, IfcElectricDistributionBoard, IfcFlowSegment and
// its siblings, IfcTextLiteral ...) are deliberately absent.
inline constexpr ProductLayout kProductLayouts[] = {
    {"IfcAnnotation", false, true},
    {"IfcBorehole", true, false},
    {"IfcCableCarrierSegment", true, true},
    {"IfcCableSegment", true, true},
    {"IfcColumn", true, true},
    {"IfcDistributionChamberElement", true, true},
    {"IfcDistributionFlowElement", true, false},
    {"IfcFireSuppressionTerminal", true, true},
    {"IfcGeographicElement", true, true},
    {"IfcJunctionBox", true, true},
    {"IfcKerb", true, true},
    {"IfcMember", true, true},
    {"IfcPavement", true, true},
    {"IfcPipeFitting", true, true},
    {"IfcPipeSegment", true, true},
    {"IfcRailing", true, true},
    {"IfcSign", true, true},
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
    return findProductLayout(entity).value_or(ProductLayout{entity, true, true});
}

} // namespace katana::ifc::detail
