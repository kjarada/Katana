// What the foundation binds of GDAL's algorithms, pinned (contract_support.hpp,
// docs/geoprocessing.md "Contract tests"). A GDAL upgrade that renames or
// reshapes one of these fails here, naming it.

#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "contract_support.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/processing.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::geo_test::expectArgument;

TEST(GdalContract, TheFoundationsAlgorithmsTakeTheArgumentsItBinds)
{
    // Every raster input became a dataset list in 3.13.
    expectArgument({"raster", "hillshade"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "hillshade"}, "output", gp::ArgType::Dataset, true);
    expectArgument({"raster", "hillshade"}, "output-format", gp::ArgType::String, false);
    expectArgument({"raster", "hillshade"}, "overwrite", gp::ArgType::Boolean, false);
    expectArgument({"raster", "hillshade"}, "creation-option", gp::ArgType::StringList, false);
    expectArgument({"vector", "buffer"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"vector", "buffer"}, "distance", gp::ArgType::Real, true);
    expectArgument({"raster", "info"}, "output-string", gp::ArgType::String, false);
    expectArgument({"dataset", "check"}, "return-code", gp::ArgType::Integer, false);
    expectArgument({"raster", "calc"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"pipeline"}, "pipeline", gp::ArgType::String, false);
    expectArgument({"pipeline"}, "input", gp::ArgType::DatasetList, false);
}

TEST(GdalContract, NoDatasetArgumentIsNamedLikeASourceKeyword)
{
    // FROM <arg> <source>: an argument named like a source or a scope word
    // would be read as the source, and FROM could not name it.
    const std::vector<std::string> keywords{"raster",  "surface", "file",    "selection", "sel",
                                            "drawing", "all",     "view",    "area",      "layers",
                                            "layer",   "where",   "from",    "to",        "confirm",
                                            "overwrite", "preview"};
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (info.container) {
            continue;
        }
        const auto spec = gp::describe(info.path);
        ASSERT_TRUE(spec.ok()) << gp::pathText(info.path);
        for (const gp::ArgSpec& arg : spec->args) {
            if (!arg.isDataset()) {
                continue;
            }
            std::vector<std::string> names{arg.name};
            names.insert(names.end(), arg.aliases.begin(), arg.aliases.end());
            for (const std::string& name : names) {
                for (const std::string& keyword : keywords) {
                    EXPECT_FALSE(katana::core::equalsIgnoringCase(name, keyword))
                        << gp::pathText(info.path) << " " << name;
                }
            }
        }
    }
}

TEST(GdalContract, NameOnlyInputsAreTheOnesTheBridgeStages)
{
    // The measured exceptions to "a name or an object": these take names
    // only, which is why a grid in memory is staged under /vsimem for them.
    for (const std::vector<std::string>& path : std::vector<std::vector<std::string>>{
             {"raster", "calc"}, {"raster", "index"}, {"vector", "index"}}) {
        const auto spec = gp::describe(path);
        ASSERT_TRUE(spec.ok());
        bool found = false;
        for (const gp::ArgSpec& arg : spec->args) {
            if (arg.name == "input") {
                found = true;
                EXPECT_TRUE(arg.acceptsName && !arg.acceptsObject) << gp::pathText(path);
            }
        }
        EXPECT_TRUE(found) << gp::pathText(path);
    }
}

} // namespace
