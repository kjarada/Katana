#include <gtest/gtest.h>

#include <memory>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;

TEST(SurveyJobs, ANewDocumentHasNoJobs)
{
    Document document;
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.findSurveyJob("job-1"), nullptr);
    EXPECT_EQ(document.surveyJobsGeneration(), 0U);
}

TEST(SurveyJobs, TheCommandsDoorFindsAndCountsChangesToTheJobList)
{
    Document document;
    SurveyJob job;
    job.id = "job-1";
    job.name = "site.gsi";
    SurveyJobAccess::jobs(document).push_back(job);
    SurveyJobAccess::changed(document);
    ASSERT_NE(document.findSurveyJob("job-1"), nullptr);
    EXPECT_EQ(document.findSurveyJob("job-1")->name, "site.gsi");
    EXPECT_EQ(document.surveyJobsGeneration(), 1U);
}

// The placeholders fail loudly and change nothing until the jobs builder
// lands; this pins that they do, and is replaced with the real behaviour.
TEST(SurveyJobs, UntilTheyAreImplementedTheJobCommandsRefuseAndChangeNothing)
{
    Document document;
    const auto before = document.modelRevision();
    const auto status =
        document.execute(std::make_unique<ImportSurveyJobCommand>(document, SurveyJobImport{}));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::Unsupported);
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.modelRevision(), before);
    EXPECT_FALSE(reductionContextFor(document).ok());
}
