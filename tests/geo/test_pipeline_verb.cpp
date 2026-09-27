// A GDAL pipeline through the executor (docs/geoprocessing.md, "Pipelines"):
// its result bound as a run's output - a layer, a reference raster - unless
// its last step prints. Values by hand on tests/geo/data/plane.asc: 40 x 30
// cells of 1 m, z = 100 + 0.05x at the centres, so from 100.025 to 101.975.

#include <filesystem>
#include <regex>
#include <set>
#include <string>
#include <system_error>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;

const std::string kPlane = std::string(KATANA_GEO_TEST_DATA) + "/plane.asc";

class PipelineVerb : public ::testing::Test {
  protected:
    // Emptied before and after: a raster an earlier run left there would
    // take the name a test expects.
    std::filesystem::path scratch = std::filesystem::temp_directory_path() / "katana-pipeline-verb";
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch, {}, {}};

    void SetUp() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
    }
    void TearDown() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
    }

    std::string ok(const std::string& line)
    {
        auto reply = geo::runNow(context, line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply ? "" : reply.error().describe());
        return reply ? *reply : std::string();
    }
};

TEST_F(PipelineVerb, APipelineEndingInWriteDrawsItsFeaturesOnALayer)
{
    // Contours every 0.5 m over 100.025 .. 101.975: 100.5, 101.0 and 101.5,
    // each buffered into one area - three, by hand.
    const std::string reply = ok("GDAL pipeline \"read ! contour --interval 0.5 ! buffer 0.1 ! "
                                 "write\" FROM input FILE \"" + kPlane + "\" TO LAYER gis/bands");
    EXPECT_NE(reply.find("output arg=output kind=vector target=layer layer=gis/bands created=3"),
              std::string::npos)
        << reply;
    EXPECT_EQ(document.model().entities.size(), 3u);
}

TEST_F(PipelineVerb, APipelineEndingInWriteKeepsItsRasterAsAReference)
{
    const std::string reply = ok("GDAL pipeline \"read ! hillshade ! write\" FROM input FILE \"" +
                                 kPlane + "\" TO REFERENCE shade");
    EXPECT_NE(reply.find("target=reference id=1 name=shade raster=40x30"), std::string::npos)
        << reply;
    EXPECT_EQ(reference.rasters().size(), 1u);
}

// FROM without an argument's name binds the pipeline's one input, which is
// optional (its read step may name a file): it was refused as "no required
// dataset left", and docs/geoprocessing.md's own example with it failed.
// The same three bands as the named FROM above.
TEST_F(PipelineVerb, AnUnnamedFromBindsThePipelinesOneInput)
{
    const std::string reply = ok("GDAL pipeline \"read ! contour --interval 0.5 ! buffer 0.1 ! "
                                 "write\" FROM FILE \"" +
                                 kPlane + "\" TO LAYER gis/bands");
    EXPECT_NE(reply.find("input arg=input source=file"), std::string::npos) << reply;
    EXPECT_NE(reply.find("output arg=output kind=vector target=layer layer=gis/bands created=3"),
              std::string::npos)
        << reply;
}

TEST_F(PipelineVerb, APipelineEndingInInfoPrints)
{
    const std::string reply = ok("GDAL pipeline \"read ! info\" FROM input FILE \"" + kPlane + "\"");
    EXPECT_NE(reply.find("text lines="), std::string::npos) << reply;
    EXPECT_TRUE(reference.rasters().empty());
}

TEST(PipelineContract, TheStepsThatPrintAreTheOnesGdalDeclaresSo)
{
    // The bridge leaves a pipeline's output unbound - to print - only when its
    // last step is one of these; a GDAL that adds a printing step fails here,
    // naming it, rather than a pipeline failing in front of a person.
    const auto spec = gp::describe({"pipeline"});
    ASSERT_TRUE(spec.ok());
    // GDAL writes a default of Infinity (vector grid's radius) bare, which is
    // no JSON; as a value it is null here.
    const std::regex notJson(R"(([:\[,])\s*-?(Infinity|NaN))");
    const nlohmann::json usage =
        nlohmann::json::parse(std::regex_replace(spec->usageJson, notJson, "$1null"));
    std::set<std::string> printing;
    for (const auto& step : usage.at("pipeline_algorithms")) {
        for (const auto& output : step.at("output_arguments")) {
            if (output.at("name") == "output-string") {
                printing.insert(step.at("name").get<std::string>());
            }
        }
    }
    EXPECT_EQ(printing, (std::set<std::string>{"compare", "export-schema", "info"}));
}

} // namespace
