#include <gtest/gtest.h>

#include <filesystem>
#include <string>
#include <system_error>

#include "katana/storage/project_store.hpp"
#include "katana/storage/survey_job.hpp"

using namespace katana::storage;
namespace fs = std::filesystem;
using katana::core::ErrorCode;

TEST(SurveyJobStorage, AJobStartsEmptyWithDefaultReductionSettings)
{
    const SurveyJob job;
    EXPECT_TRUE(job.id.empty());
    EXPECT_TRUE(job.sourceBytes.empty());
    EXPECT_TRUE(job.siblingFiles.empty());
    EXPECT_TRUE(job.createdEntities.empty());
    EXPECT_EQ(job.settings, katana::survey::ReductionSettings{});
    EXPECT_TRUE(ProjectContents{}.surveyJobs.empty());
}

// Until the survey job table exists, a project holding a job must be refused
// rather than saved without it: losing the raw bytes behind an imported survey
// is exactly the silent loss the job exists to prevent. The jobs builder
// replaces this with a round trip.
TEST(SurveyJobStorage, AProjectHoldingAJobIsRefusedRatherThanSavedWithoutIt)
{
    const fs::path root = fs::temp_directory_path() / "katana-storage-tests" / "survey-job-refusal";
    std::error_code ignored;
    fs::remove_all(root, ignored);
    fs::create_directories(root);
    auto store = ProjectStore::create(root / "site.katana", {});
    ASSERT_TRUE(store.ok()) << store.error().describe();
    ProjectContents contents;
    SurveyJob job;
    job.id = "job-1";
    job.sourceBytes = "raw bytes";
    contents.surveyJobs.push_back(job);
    const auto status = store->save(contents);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::Unsupported);
    fs::remove_all(root, ignored);
}
