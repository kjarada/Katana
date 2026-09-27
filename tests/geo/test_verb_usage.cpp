// The grammar HELP gives and the docs document is the grammar the verbs
// accept (src/katana_app/geo/verb_table.cpp; docs/terrain.md, "Surfaces",
// "Gridding points to a DEM" and "The DEM tools"; docs/geoprocessing.md,
// "The GDAL verb"). One line per documented form, every option of it given,
// run through the executor as the window, katana_cli and katana_mcp run it:
// a form the help names that the verb refuses fails here. The heavy forms
// are previewed, which reads the whole line and binds its sources; the
// targets the help once left out (TO SURFACE, TO SELECTION, TO REPORT) run.

#include <filesystem>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "geo/geo_verbs.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

namespace geo = katana::app::geo;

const std::string kData = KATANA_GEO_TEST_DATA;
const std::string kSamples = KATANA_GIS_SAMPLES;

class GeoVerbUsage : public ::testing::Test {
  protected:
    std::filesystem::path scratch =
        std::filesystem::temp_directory_path() /
        ("katana-verb-usage-" +
         std::string(::testing::UnitTest::GetInstance()->current_test_info()->name()));
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch / "derived", {}, {}};

    void SetUp() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
        std::filesystem::create_directories(scratch, error);
    }
    void TearDown() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
    }

    [[nodiscard]] std::string at(const std::string& name) const
    {
        return "\"" + (scratch / name).generic_string() + "\"";
    }

    void accepted(const std::string& line)
    {
        const auto reply = geo::runNow(context, line);
        EXPECT_TRUE(reply.ok()) << line << "\n  " << (reply ? "" : reply.error().describe());
    }
};

TEST_F(GeoVerbUsage, EveryFormTheHelpGivesIsAccepted)
{
    // What the forms read: plane.asc (40 x 30 cells of 1 m) as the reference
    // raster "plane" and the surface "ground", the sample scan as the cloud
    // "scan", levelled points, the lots, and one closed boundary inside the
    // plane on the layer "boundary".
    const std::string plane = "\"" + kData + "/plane.asc\"";
    const std::string lots = "\"" + kData + "/lots.geojson\"";
    for (const std::string& setup :
         {"IMPORT " + plane, "IMPORT \"" + kSamples + "/survey_scan.las\" name=scan",
          "IMPORT \"" + kData + "/plane_points.geojson\"", "IMPORT " + lots,
          std::string("SURFACE FROM RASTER plane NAME ground")}) {
        ASSERT_TRUE(geo::runNow(context, setup).ok()) << setup;
    }
    for (const char* setup : {"LAYER NEW boundary", "LAYER SET boundary", "RECT 2,2 20,20"}) {
        ASSERT_TRUE(interpreter.run(setup).ok()) << setup;
    }

    const std::vector<std::string> lines = {
        // GDAL: every source kind, every target kind, every clause word.
        "GDAL VERSION",
        "GDAL LIST raster JSON",
        "GDAL HELP raster hillshade JSON",
        "GDAL RUN raster hillshade --zfactor=2 FROM input RASTER plane TO output REFERENCE shade "
        "CONFIRM OVERWRITE PREVIEW",
        "GDAL raster hillshade FROM SURFACE ground CELL 2 TO FILE " + at("h.tif") +
            " FORMAT GTiff PREVIEW",
        "GDAL raster hillshade FROM FILE " + plane + " TO SURFACE shaded",
        "GDAL vector buffer --distance=1 FROM FILE " + lots + " LAYER lots TO LAYER gis/b PREVIEW",
        "GDAL vector buffer --distance=1 FROM LAYERS lots ONLY WHERE TYPE=polyline TO SELECTION",
        "GDAL vector buffer --distance=1 FROM AREA 0,0,200,200 TO REPORT",
        // SURFACE: each source with its options, and EXPORT with all of its.
        "SURFACE LIST JSON",
        "SURFACE INFO ground",
        "SURFACE FROM RASTER plane max=500 AREA 0,0,10,10 NAME a PREVIEW",
        "SURFACE FROM FILE " + plane + " max=500 AREA 0,0,10,10 NAME b PREVIEW",
        "SURFACE FROM CLOUD scan classes=2 max=100 NAME c PREVIEW",
        "SURFACE FROM LAYERS plane_points NAME d PREVIEW",
        "SURFACE EXPORT ground " + at("g.tif") +
            " cell=2 type=Float32 cog format=GTiff co=COMPRESS=DEFLATE OVERWRITE PREVIEW",
        // RASTER GRID: its options and each target.
        "RASTER GRID LAYERS plane_points method=invdist cell=2 z=geometry extent=scope power=2 "
        "radius=10 NAME g PREVIEW",
        "RASTER GRID LAYERS plane_points method=nearest size=10x10 extent=0,0,40,30 TO FILE " +
            at("grid.tif") + " FORMAT GTiff OVERWRITE PREVIEW",
        "RASTER GRID LAYERS plane_points method=nearest size=10x10 extent=0,0,40,30 TO SURFACE "
        "gridded",
        // The DEM tools: each <raster> kind, each tool's options, and the
        // words every tool takes.
        "RASTER MOSAIC RASTER plane FILE " + plane + " resolution=highest SAVE " + at("m.tif") +
            " NAME m OVERWRITE PREVIEW",
        "RASTER CLIP SURFACE ground CELL 1 AREA 0,0,10,10 NAME cl PREVIEW",
        "RASTER CLIP RASTER plane LAYERS boundary TO FILE " + at("c.tif") +
            " FORMAT GTiff OVERWRITE PREVIEW",
        "RASTER FILL RASTER plane distance=5 smoothing=1 strategy=nearest TO REFERENCE f PREVIEW",
        "RASTER FOOTPRINT FILE " + plane + " TO LAYER gis/fp PREVIEW",
        "RASTER FOOTPRINT RASTER plane TO FILE " + at("fp.geojson") + " OVERWRITE PREVIEW",
        "RASTER REPROJECT RASTER plane crs=EPSG:28356 from=EPSG:28355 resampling=bilinear cell=2 "
        "PREVIEW",
        "RASTER REPROJECT RASTER plane like=plane from=EPSG:28356 PREVIEW",
        "RASTER DIFFERENCE RASTER plane MINUS SURFACE ground CELL 1 LAYERS boundary "
        "resampling=bilinear TO SURFACE difference",
        // EXPORT of a reference cloud.
        "EXPORT " + at("scan.laz") + " CLOUD scan PREVIEW",
    };
    for (const std::string& line : lines) {
        accepted(line);
    }
    // The targets run, not only previewed, made what they name.
    for (const char* name : {"shaded", "gridded", "difference"}) {
        EXPECT_NE(surfaces.find(name), nullptr) << name;
    }
}

// And HELP names the words those lines use that its rows once left out: the
// GDAL verb's FILE LAYER and its SURFACE, SELECTION and REPORT targets,
// SURFACE's format= and CLOUD's max=, and what a DEM tool's <raster> is and
// the words each takes.
TEST_F(GeoVerbUsage, TheHelpNamesEveryWordTheFormsUse)
{
    const std::string help = geo::helpText();
    for (const char* words :
         {"FILE <path> [LAYER <name>]; TO takes", "SURFACE <name>, SELECTION or REPORT",
          "CLOUD <id|name> [classes=2,...] [max=<points>]", "[format=<driver>]",
          "<raster> is\n          RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>",
          "[FORMAT <driver>] | TO SURFACE <name>] [OVERWRITE] [PREVIEW]",
          "[resolution=same|highest|lowest|average|<x>,<y>] [SAVE <file>]"}) {
        EXPECT_NE(help.find(words), std::string::npos) << words;
    }
}

} // namespace
