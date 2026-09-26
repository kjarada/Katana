// Which GDAL algorithms run without asking (src/katana_io/geo/policy.cpp,
// docs/geoprocessing.md "Safety").

#include <algorithm>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "katana/gis/processing.hpp"

namespace {

namespace gp = katana::gis::processing;

const gp::AlgorithmInfo* infoOf(const std::vector<std::string>& path)
{
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (info.path == path) {
            return &info;
        }
    }
    return nullptr;
}

TEST(GdalPolicy, EveryLeafHasAPolicyAndAReasonForIt)
{
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (info.container) {
            EXPECT_EQ(info.policyReason, "container") << gp::pathText(info.path);
            continue;
        }
        const bool known = info.policyReason == "listed" || info.policyReason == "update" ||
                           info.policyReason == "unclassified";
        EXPECT_TRUE(known) << gp::pathText(info.path) << ": " << info.policyReason;
    }
}

TEST(GdalPolicy, UnclassifiedLeavesNeedConfirmAndAreNamed)
{
    // A leaf a GDAL upgrade adds is Confirm until someone judges it and puts
    // it in policy.cpp's table. This lists every one, so the upgrade that
    // brings one says which.
    std::vector<std::string> unclassified;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (!info.container && info.policyReason == "unclassified") {
            EXPECT_EQ(info.policy, gp::Policy::Confirm) << gp::pathText(info.path);
            unclassified.push_back(gp::pathText(info.path));
        }
    }
    std::string names;
    for (const std::string& name : unclassified) {
        names += (names.empty() ? "" : ", ") + name;
    }
    EXPECT_TRUE(unclassified.empty()) << "not yet judged in policy.cpp: " << names;
}

TEST(GdalPolicy, WhatChangesOrRemovesExistingDataNeedsConfirm)
{
    for (const std::vector<std::string>& path :
         std::vector<std::vector<std::string>>{{"vsi", "delete"},
                                               {"vsi", "move"},
                                               {"vsi", "sync"},
                                               {"vsi", "copy"},
                                               {"dataset", "delete"},
                                               {"dataset", "rename"},
                                               {"raster", "edit"},
                                               {"raster", "overview", "add"},
                                               {"raster", "update"},
                                               {"vector", "update"},
                                               {"driver", "gpkg", "repack"}}) {
        const gp::AlgorithmInfo* info = infoOf(path);
        ASSERT_NE(info, nullptr) << gp::pathText(path);
        EXPECT_EQ(info->policy, gp::Policy::Confirm) << gp::pathText(path);
    }
}

TEST(GdalPolicy, AnInputOpenedForUpdateMakesAnAlgorithmConfirmWhateverTheTableSays)
{
    // raster edit opens its dataset for update: the rule catches it even
    // before the table does.
    const gp::AlgorithmInfo* edit = infoOf({"raster", "edit"});
    ASSERT_NE(edit, nullptr);
    EXPECT_EQ(edit->policyReason, "update");
}

TEST(GdalPolicy, WhatOnlyReadsOrWritesANewOutputIsSafe)
{
    for (const std::vector<std::string>& path :
         std::vector<std::vector<std::string>>{{"raster", "hillshade"},
                                               {"raster", "info"},
                                               {"vector", "buffer"},
                                               {"vector", "grid", "linear"},
                                               {"pipeline"},
                                               {"vsi", "list"}}) {
        const gp::AlgorithmInfo* info = infoOf(path);
        ASSERT_NE(info, nullptr) << gp::pathText(path);
        EXPECT_EQ(info->policy, gp::Policy::Safe) << gp::pathText(path);
    }
}

TEST(GdalPolicy, WordsThatPrintInsteadOfRunAreRefused)
{
    for (const char* word : {"--help", "-h", "--help-doc", "--json-usage", "--progress", "--quiet",
                             "--config"}) {
        EXPECT_FALSE(gp::checkTokens({"raster", "hillshade"}, {word}).ok()) << word;
    }
    EXPECT_TRUE(gp::checkTokens({"raster", "hillshade"}, {"--zfactor=2", "-z", "2"}).ok());
}

TEST(GdalPolicy, APipelineStepNamedExternalIsRefusedWhereverItStands)
{
    EXPECT_FALSE(gp::checkTokens({"pipeline"}, {"read", "x", "!", "external", "--command", "y"}).ok());
    EXPECT_FALSE(gp::checkTokens({"raster", "pipeline"}, {"EXTERNAL ! write"}).ok());
    // A word "external" that is no step - a value - is not refused.
    EXPECT_TRUE(gp::checkTokens({"raster", "pipeline"}, {"read external.tif ! write"}).ok());
    // Outside a pipeline, "external" is only a word.
    EXPECT_TRUE(gp::checkTokens({"raster", "info"}, {"external"}).ok());
}

TEST(GdalPolicy, APipelineStepThatWritesIntoAnExistingDatasetNeedsConfirmHoweverItIsQuoted)
{
    // Words one by one, one quoted text, and --pipeline=...: the same steps.
    EXPECT_EQ(gp::tailEffects({"vector", "pipeline"},
                              {"read", "src.gpkg", "!", "update", "dst.gpkg", "--key", "id"})
                  .confirmStep,
              "update");
    EXPECT_EQ(gp::tailEffects({"pipeline"}, {"read a.tif ! UPDATE b.tif"}).confirmStep, "update");
    EXPECT_EQ(gp::tailEffects({"raster", "pipeline"}, {"--pipeline=read a.tif ! update b.tif"})
                  .confirmStep,
              "update");
    // Nested in a tee.
    EXPECT_EQ(
        gp::tailEffects({"raster", "pipeline"}, {"read a.tif ! tee [ update b.tif ] ! write c.tif"})
            .confirmStep,
        "update");
    // A step no one has judged is Confirm, as an unclassified leaf is.
    EXPECT_EQ(gp::tailEffects({"pipeline"}, {"read a ! frobnicate ! write b"}).confirmStep,
              "frobnicate");
    // "update" as a value, and as a word outside a pipeline, is no step.
    EXPECT_EQ(gp::tailEffects({"pipeline"}, {"read update.tif ! write update"}).confirmStep, "");
    EXPECT_EQ(gp::tailEffects({"vector", "convert"}, {"update", "x"}).confirmStep, "");
    EXPECT_EQ(
        gp::tailEffects({"raster", "pipeline"}, {"read a.tif ! slope ! write b.tif"}).confirmStep,
        "");
}

TEST(GdalPolicy, WordsThatChangeAnExistingDatasetAreFoundInsideAQuotedPipeline)
{
    EXPECT_EQ(gp::tailEffects({"pipeline"}, {"read a.asc ! write --overwrite b.tif"}).overwriteWord,
              "--overwrite");
    EXPECT_EQ(gp::tailEffects({"pipeline"}, {"read", "a.asc", "!", "write", "--append", "b.gpkg"})
                  .overwriteWord,
              "--append");
    EXPECT_EQ(
        gp::tailEffects({"vector", "pipeline"},
                        {"read a.gpkg ! tee [ write --overwrite-layer=true c.gpkg ] ! write d"})
            .overwriteWord,
        "--overwrite-layer=true");
    EXPECT_EQ(gp::tailEffects({"pipeline"}, {"read a.asc ! write b.tif"}).overwriteWord, "");
}

TEST(GdalPolicy, RasterizeAddAndTileResumeChangeAnExistingDataset)
{
    // rasterize --add burns into the raster its output names, with no
    // --update (measured with gdal vector rasterize: the file's checksum
    // changed); tile --resume writes into a tile set already there.
    EXPECT_EQ(gp::tailEffects({"vector", "rasterize"}, {"--burn", "5", "--add", "a.gpkg", "b.tif"})
                  .overwriteWord,
              "--add");
    EXPECT_EQ(gp::tailEffects({"raster", "tile"}, {"a.tif", "tiles", "--resume"}).overwriteWord,
              "--resume");
    // A longer name that begins the same is another option.
    EXPECT_EQ(gp::tailEffects({"raster", "clip"}, {"--add-alpha"}).overwriteWord, "");
}

TEST(GdalPolicy, EveryStepGdalsPipelinesOfferIsJudged)
{
    // As every leaf is: a step a GDAL upgrade adds is Confirm until judged,
    // and this names it.
    std::vector<std::string> unjudged;
    std::size_t steps = 0;
    for (const std::vector<std::string>& path : std::vector<std::vector<std::string>>{
             {"pipeline"}, {"raster", "pipeline"}, {"vector", "pipeline"}}) {
        auto spec = gp::describe(path);
        ASSERT_TRUE(spec.ok()) << gp::pathText(path);
        // GDAL writes an unbounded default as a bare Infinity, which is not
        // JSON; the step names are all this reads.
        std::string text = spec->usageJson;
        for (const std::string bare : {"-Infinity", "Infinity", "NaN"}) {
            for (std::size_t at = text.find(":" + bare); at != std::string::npos;
                 at = text.find(":" + bare, at)) {
                text.replace(at + 1, bare.size(), "null");
            }
        }
        const nlohmann::json usage = nlohmann::json::parse(text);
        ASSERT_TRUE(usage.contains("pipeline_algorithms")) << gp::pathText(path);
        for (const nlohmann::json& step : usage["pipeline_algorithms"]) {
            ++steps;
            std::string reason;
            (void)gp::stepPolicy(step["name"].get<std::string>(), reason);
            if (reason == "unclassified") {
                unjudged.push_back(step["name"].get<std::string>());
            }
        }
    }
    EXPECT_GT(steps, 0u);
    std::string names;
    for (const std::string& name : unjudged) {
        names += (names.empty() ? "" : ", ") + name;
    }
    EXPECT_TRUE(unjudged.empty()) << "pipeline steps not yet judged in policy.cpp: " << names;
    std::string reason;
    EXPECT_EQ(gp::stepPolicy("update", reason), gp::Policy::Confirm);
    EXPECT_EQ(gp::stepPolicy("write", reason), gp::Policy::Safe);
    EXPECT_EQ(gp::stepPolicy("external", reason), gp::Policy::Confirm);
    EXPECT_EQ(reason, "refused");
}

TEST(GdalPolicy, OnlyAnAlgorithmThatReadsZIsGivenHeightsOnly)
{
    EXPECT_TRUE(gp::readsHeights({"vector", "grid", "nearest"}, {}));
    EXPECT_TRUE(gp::readsHeights({"vector", "grid", "linear"}, {"--size", "2,2"}));
    EXPECT_FALSE(gp::readsHeights({"vector", "grid", "linear"}, {"--zfield=h"}));
    EXPECT_FALSE(gp::readsHeights({"vector", "grid", "count"}, {}));
    EXPECT_TRUE(gp::readsHeights({"vector", "rasterize"}, {"--3d"}));
    EXPECT_FALSE(gp::readsHeights({"vector", "rasterize"}, {"--burn", "1"}));
    EXPECT_FALSE(gp::readsHeights({"vector", "buffer"}, {"--distance", "1"}));
    EXPECT_TRUE(gp::readsHeights({"vector", "pipeline"}, {"read x ! grid invdist ! write y"}));
    EXPECT_FALSE(gp::readsHeights({"pipeline"}, {"read x ! grid invdist --zfield h ! write y"}));
    EXPECT_FALSE(gp::readsHeights({"pipeline"}, {"read x ! grid count ! write y"}));
    EXPECT_TRUE(
        gp::readsHeights({"pipeline"}, {"read", "x", "!", "rasterize", "--3d", "!", "write"}));
}

} // namespace
