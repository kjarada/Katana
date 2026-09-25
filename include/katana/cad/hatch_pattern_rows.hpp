#pragma once

// What the Hatch Patterns tab of Format > Styles and Linetypes shows, below
// Qt so it can be tested - the counterpart of style_manager_rows.hpp for the
// model's hatch patterns (docs/desktop.md, "Styles and Linetypes").
//
// The tab only renders these rows and runs HATCH lines; who uses a pattern is
// entity::tableUsage's one answer, so the tab, HATCH DELETE's refusal and
// PURGE HATCHES cannot disagree about what is used.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/entity/model.hpp"
#include "katana/entity/table_usage.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

struct HatchPatternRow {
    std::string name{};
    bool solid = false;
    std::size_t families = 0;
    std::string description{};
    // The layers and styles naming it and the entities drawn with it.
    katana::entity::Users users{};
    // "none", what an unhatched layer resolves to: not given a fill, not
    // deleted (entity::HatchPatternPolicy).
    bool builtIn = false;
};

// Every pattern in the table, ascending by name.
[[nodiscard]] std::vector<HatchPatternRow> hatchPatternRows(const katana::entity::Model& model);

// "solid", "1 family", "3 families" or "draws nothing".
[[nodiscard]] std::string hatchPatternKind(const HatchPatternRow& row);

// A family's angle (radians, as the model holds it) in the degrees HATCH
// takes, to twelve significant digits: the degrees a person typed. The
// radians cannot hold most of those exactly - 30 degrees is stored as
// 0.5235987755982988, which multiplies back to 29.999999999999996 - and
// twelve digits give "30" again, which HATCH turns into the same radians.
[[nodiscard]] std::string hatchAngleDegrees(double radians);

// The families as HATCH NEW and HATCH SET take them - "45 0.25 135 0.25":
// hatchAngleDegrees, and the spacing as the shortest text that reads back as
// the same double. Empty for a solid pattern or one that draws nothing.
[[nodiscard]] std::string hatchFamiliesText(const katana::entity::HatchPattern& pattern);

// `base`, else "base 2", "base 3" ... - the first name no hatch pattern has.
[[nodiscard]] std::string freeHatchPatternName(const katana::entity::Model& model,
                                               std::string_view base);

} // namespace katana::cad
