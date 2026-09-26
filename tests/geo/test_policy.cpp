// Which GDAL algorithms run without asking (src/katana_io/geo/policy.cpp,
// docs/geoprocessing.md "Safety").

#include <algorithm>
#include <string>
#include <vector>

#include <gtest/gtest.h>

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

} // namespace
