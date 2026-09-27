#pragma once

// The contract tests' one helper (docs/geoprocessing.md, "Contract tests"):
// GDAL's algorithm framework is provisional - 3.13 already turned every
// raster input into a dataset list - and MSYS2 upgrades GDAL on a rolling
// basis, so each package pins the algorithm paths and arguments it binds.
// When an upgrade renames or reshapes one, the test that fails names what
// changed, instead of a verb failing in front of a person.

#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "katana/gis/processing.hpp"

namespace katana::geo_test {

// The argument `name` of the algorithm at `path` exists, has `type`, and is
// required or not.
inline void expectArgument(const std::vector<std::string>& path, const std::string& name,
                           katana::gis::processing::ArgType type, bool required)
{
    namespace gp = katana::gis::processing;
    const auto spec = gp::describe(path);
    ASSERT_TRUE(spec.ok()) << gp::pathText(path) << ": "
                           << (spec.ok() ? std::string() : spec.error().describe());
    const gp::ArgSpec* found = nullptr;
    for (const gp::ArgSpec& arg : spec->args) {
        if (arg.name == name) {
            found = &arg;
        }
    }
    ASSERT_NE(found, nullptr) << gp::pathText(path) << " has no argument " << name;
    EXPECT_EQ(found->type, type) << gp::pathText(path) << " " << name << " is "
                                 << gp::toString(found->type) << ", not " << gp::toString(type);
    EXPECT_EQ(found->required, required)
        << gp::pathText(path) << " " << name << (required ? " is no longer" : " is now")
        << " required";
}

} // namespace katana::geo_test
