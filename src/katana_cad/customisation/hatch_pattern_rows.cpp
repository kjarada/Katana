#include "katana/cad/hatch_pattern_rows.hpp"

#include <format>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

std::vector<HatchPatternRow> hatchPatternRows(const katana::entity::Model& model)
{
    const katana::entity::TableUsage usage = katana::entity::tableUsage(model);
    std::vector<HatchPatternRow> rows;
    rows.reserve(model.hatchPatterns.size());
    model.hatchPatterns.forEach([&](const katana::entity::HatchPattern& pattern) {
        HatchPatternRow row;
        row.name = pattern.name;
        row.solid = pattern.solid;
        row.families = pattern.families.size();
        row.description = pattern.description;
        row.users = katana::entity::TableUsage::of(usage.hatchPatterns, pattern.name);
        row.builtIn = katana::entity::HatchPatternPolicy::isProtected(pattern.name);
        rows.push_back(std::move(row));
    });
    return rows;
}

std::string hatchPatternKind(const HatchPatternRow& row)
{
    if (row.solid) {
        return "solid";
    }
    if (row.families == 0) {
        return "draws nothing";
    }
    return std::to_string(row.families) + (row.families == 1 ? " family" : " families");
}

std::string hatchAngleDegrees(double radians)
{
    std::string text = std::format("{:.12g}", radians * katana::math::kRadToDeg);
    // A hair below zero is zero to the digits shown.
    return text == "-0" ? std::string("0") : text;
}

std::string hatchFamiliesText(const katana::entity::HatchPattern& pattern)
{
    std::string text;
    if (pattern.solid) {
        return text;
    }
    for (const katana::entity::HatchLineFamily& family : pattern.families) {
        if (!text.empty()) {
            text += ' ';
        }
        text += hatchAngleDegrees(family.angle) + " " +
                katana::core::formatExactReal(family.spacing);
    }
    return text;
}

std::string freeHatchPatternName(const katana::entity::Model& model, std::string_view base)
{
    std::string name(base);
    for (int suffix = 2; model.hatchPatterns.contains(name); ++suffix) {
        name = std::string(base) + " " + std::to_string(suffix);
    }
    return name;
}

} // namespace katana::cad
