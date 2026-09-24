#include <gtest/gtest.h>

#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <limits>
#include <string>
#include <system_error>

#include "katana/storage/project_store.hpp"
#include "katana/storage/sqlite_database.hpp"
#include "katana/storage/survey_job.hpp"

using namespace katana::storage;
namespace fs = std::filesystem;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Layer;
using katana::entity::PointGeometry;
using katana::geometry::Point2;

namespace {

// Each test gets its own empty directory, removed afterwards.
class SurveyJobStorage : public ::testing::Test {
  protected:
    void SetUp() override
    {
        const auto* info = ::testing::UnitTest::GetInstance()->current_test_info();
        root_ = fs::temp_directory_path() / "katana-storage-tests" /
                (std::string(info->test_suite_name()) + "." + info->name());
        std::error_code ignored;
        fs::remove_all(root_, ignored);
        fs::create_directories(root_);
    }
    void TearDown() override
    {
        std::error_code ignored;
        fs::remove_all(root_, ignored);
    }

    [[nodiscard]] fs::path projectDir() const { return root_ / "site.katana"; }

    fs::path root_;
};

// The committed fixtures live beside this file. __FILE__ is the absolute path
// the build gave the compiler, so this finds them from any working directory.
fs::path sourceDirectory()
{
    return fs::path(__FILE__).parent_path();
}

// A job with every field set to something the defaults are not, including
// bytes a TEXT column would mangle (a NUL, a lone CR, bytes that are not
// UTF-8) and doubles that only an exact round trip keeps.
SurveyJob fullJob(std::string id)
{
    SurveyJob job;
    job.id = std::move(id);
    job.name = "Blue beach day 2";
    job.formatId = "leica-gsi";
    job.parserVersion = "1.3";
    job.sourceFileName = "DAY2.GSI";
    job.sourceBytes = std::string("*110001+0000000000000101 21.324+0000000009000000\r\n", 51);
    job.sourceBytes += std::string("\0\r\xff\xfe binary tail", 17);
    job.siblingFiles = {SurveyJobFile{"DAY2.X01", std::string("\x01\x00\x02", 3)},
                        SurveyJobFile{"day2.nav", "3.04 N: GNSS NAV DATA"}};
    job.settings.atmospheric = katana::survey::AtmosphericCorrection::Fixed;
    job.settings.fixedPpm = 12.4;
    job.settings.method = katana::survey::AdjustmentMethod::Network;
    job.settings.networkDimension = katana::survey::NetworkDimension::HorizontalAndLevels;
    job.settings.refractionCoefficient = 0.1 + 0.02; // not exactly 0.12 in binary
    job.settings.control.push_back(katana::survey::ControlSelection{
        katana::survey::ControlPoint::fixedHorizontal("CP;1"),
        katana::survey::ControlOrigin::Drawing});
    job.layer = "survey/day 2";
    job.createdEntities = {7, 8, 9, 4'000'000'000ULL};
    job.reportText = "Reduction report\n  101  N 6250000.125\n";
    job.reportHtml = "<!DOCTYPE html><p>101 &amp; 102</p>";
    job.reportCreatedUtc = "2026-09-24T01:02:03Z";
    job.importedUtc = "2026-09-23T22:00:00Z";
    job.placedPoints = {
        SurveyJobPoint{"101", 7, 6250000.125, 300000.0625, 12.5},
        SurveyJobPoint{"102", 8, 1.0 / 3.0, -0.1, std::nullopt},
        SurveyJobPoint{"", 9, std::numeric_limits<double>::denorm_min(), 0.0, -0.0},
    };
    job.importOptions = "katana-survey-import-options=1\nlayer-per-code=true\n"
                        "point-number-property=pt%0Ano\n";
    return job;
}

ProjectContents contentsWith(std::vector<SurveyJob> jobs)
{
    ProjectContents contents;
    contents.layers.push_back(Layer{});
    contents.surveyJobs = std::move(jobs);
    return contents;
}

} // namespace

TEST_F(SurveyJobStorage, AJobStartsEmptyWithDefaultReductionSettings)
{
    const SurveyJob job;
    EXPECT_TRUE(job.id.empty());
    EXPECT_TRUE(job.sourceBytes.empty());
    EXPECT_TRUE(job.siblingFiles.empty());
    EXPECT_TRUE(job.createdEntities.empty());
    EXPECT_TRUE(job.placedPoints.empty());
    EXPECT_EQ(job.settings, katana::survey::ReductionSettings{});
    EXPECT_TRUE(ProjectContents{}.surveyJobs.empty());
}

TEST_F(SurveyJobStorage, EveryFieldOfEveryJobComesBackExactlyAndInOrder)
{
    const SurveyJob first = fullJob("job-12");
    SurveyJob second; // a job with nothing but an id: empty is not missing
    second.id = "job-3";
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok()) << store.error().describe();
        const auto status = store->save(contentsWith({first, second}));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    ASSERT_EQ(contents->surveyJobs.size(), 2U);
    // Creation order, not id order: "job-12" was made before "job-3" here.
    EXPECT_EQ(contents->surveyJobs[0], first);
    EXPECT_EQ(contents->surveyJobs[1], second);
    // Spelt out for the fields whose loss would be quiet.
    EXPECT_EQ(contents->surveyJobs[0].sourceBytes.size(), 68U); // 51 + 17
    EXPECT_EQ(contents->surveyJobs[0].settings.refractionCoefficient, 0.1 + 0.02);
    EXPECT_FALSE(contents->surveyJobs[0].placedPoints[1].elevation.has_value());
    EXPECT_TRUE(std::signbit(*contents->surveyJobs[0].placedPoints[2].elevation));
}

TEST_F(SurveyJobStorage, ASecondSaveReplacesTheJobsRatherThanAddingToThem)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok()) << store.error().describe();
    ASSERT_TRUE(store->save(contentsWith({fullJob("job-1"), fullJob("job-2")})).ok());
    // The same ids again would break the UNIQUE constraint if the first
    // save's rows were still there; one job fewer shows a removal sticks.
    const auto status = store->save(contentsWith({fullJob("job-2")}));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    ASSERT_EQ(contents->surveyJobs.size(), 1U);
    EXPECT_EQ(contents->surveyJobs[0].id, "job-2");
    EXPECT_EQ(contents->surveyJobs[0].siblingFiles.size(), 2U);
}

TEST_F(SurveyJobStorage, AJobWithoutAnIdOrWithAnotherJobsIdIsRefusedAndNothingIsWritten)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok()) << store.error().describe();
    ASSERT_TRUE(store->save(contentsWith({fullJob("job-1")})).ok());

    const auto unnamed = store->save(contentsWith({fullJob("")}));
    ASSERT_FALSE(unnamed.ok());
    EXPECT_EQ(unnamed.error().code, ErrorCode::InvalidArgument);
    const auto twice = store->save(contentsWith({fullJob("job-5"), fullJob("job-5")}));
    ASSERT_FALSE(twice.ok());
    EXPECT_EQ(twice.error().code, ErrorCode::InvalidArgument);

    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    ASSERT_EQ(contents->surveyJobs.size(), 1U);
    EXPECT_EQ(contents->surveyJobs[0].id, "job-1");
}

// The size the brief names. Every byte value appears, so a column that
// re-encoded text, stopped at a NUL or truncated would show.
TEST_F(SurveyJobStorage, AFiftyMegabyteFieldFileAndItsSiblingRoundTripByteForByte)
{
    constexpr std::size_t kSize = 50U * 1024U * 1024U;
    SurveyJob job = fullJob("job-1");
    job.sourceBytes.resize(kSize);
    std::uint32_t state = 2463534242U; // xorshift32: every byte value, no pattern
    for (char& c : job.sourceBytes) {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        c = static_cast<char>(state & 0xFFU);
    }
    job.siblingFiles[1].bytes = job.sourceBytes.substr(kSize / 2); // 25 MB more

    using Clock = std::chrono::steady_clock;
    const auto started = Clock::now();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok()) << store.error().describe();
        const auto status = store->save(contentsWith({job}));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    const auto saved = Clock::now();
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    const auto loaded = Clock::now();
    ASSERT_EQ(contents->surveyJobs.size(), 1U);
    EXPECT_TRUE(contents->surveyJobs[0].sourceBytes == job.sourceBytes);
    EXPECT_TRUE(contents->surveyJobs[0].siblingFiles == job.siblingFiles);
    EXPECT_EQ(contents->surveyJobs[0], job);
    // Recorded, not asserted: a shared machine's timing is not a property.
    const auto ms = [](auto d) {
        return std::chrono::duration_cast<std::chrono::milliseconds>(d).count();
    };
    RecordProperty("save_ms", std::to_string(ms(saved - started)));
    RecordProperty("load_ms", std::to_string(ms(loaded - saved)));
    std::printf("75 MB of survey job files: save %lld ms, open + load %lld ms\n",
                static_cast<long long>(ms(saved - started)),
                static_cast<long long>(ms(loaded - saved)));
}

// A project written by main's code before survey jobs existed (schema 9, made
// with main's katana_cli: two points and a line on layer survey/points, then
// SAVE). It opens with every entity as it was, no jobs, migrated to the
// current schema after a backup - and then keeps a job like any other.
TEST_F(SurveyJobStorage, AProjectSavedBeforeSurveyJobsOpensUnchangedAndThenKeepsOne)
{
    fs::create_directories(projectDir());
    fs::copy_file(sourceDirectory() / "fixtures" / "schema9_main" / "project.db",
                  projectDir() / "project.db");
    {
        auto raw = SqliteDatabase::open(projectDir() / "project.db",
                                        SqliteDatabase::OpenMode::ReadOnly);
        ASSERT_TRUE(raw.ok());
        ASSERT_EQ(*raw->userVersion(), 9);
    }

    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    EXPECT_EQ(*store->schemaVersion(), 10);
    EXPECT_EQ(store->listBackups().size(), 1U); // taken before the migration
    auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    EXPECT_TRUE(contents->surveyJobs.empty());
    EXPECT_EQ(contents->metadata.name, "schema9");
    EXPECT_EQ(contents->nextEntityId, 4U);
    ASSERT_EQ(contents->entities.size(), 3U);
    // The coordinates the CLI was given: POINT 300000.125,6250000.5 and
    // POINT 300010.25,6250020.75 - binary fractions, so exact.
    EXPECT_EQ(std::get<PointGeometry>(contents->entities[0].geometry).position,
              Point2(300000.125, 6250000.5));
    EXPECT_EQ(std::get<PointGeometry>(contents->entities[1].geometry).position,
              Point2(300010.25, 6250020.75));
    EXPECT_EQ(contents->entities[2].layer, "survey/points");

    SurveyJob job = fullJob("job-4");
    job.createdEntities = {1, 2};
    contents->surveyJobs.push_back(job);
    ASSERT_TRUE(store->save(*contents).ok());
    const auto again = store->load();
    ASSERT_TRUE(again.ok()) << again.error().describe();
    ASSERT_EQ(again->surveyJobs.size(), 1U);
    EXPECT_EQ(again->surveyJobs[0], job);
    EXPECT_EQ(again->entities.size(), 3U);
}

// The sample drawing in the repository was saved by an older main still
// (schema 8), so it runs two migrations to get here.
TEST_F(SurveyJobStorage, TheSampleDrawingFromAnOlderSchemaOpensWithNoJobs)
{
    const fs::path sample = sourceDirectory().parent_path().parent_path() / "samples" / "site_plan";
    ASSERT_TRUE(fs::is_regular_file(sample / "project.db"));
    fs::create_directories(projectDir());
    fs::copy_file(sample / "project.db", projectDir() / "project.db");
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    EXPECT_EQ(*store->schemaVersion(), ProjectStore::kCurrentSchemaVersion);
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    EXPECT_TRUE(contents->surveyJobs.empty());
    EXPECT_FALSE(contents->entities.empty());
}

TEST_F(SurveyJobStorage, ADamagedListOfPlacedPointsFailsTheLoadWithASentence)
{
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(contentsWith({fullJob("job-1")})).ok());
    }
    {
        auto raw = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(raw.ok());
        // Cut the blob short in the middle of the first point.
        ASSERT_TRUE(raw->execute("UPDATE survey_jobs SET placed_points ="
                                 " substr(placed_points, 1, 20)")
                        .ok());
    }
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok());
    const auto contents = store->load();
    ASSERT_FALSE(contents.ok());
    EXPECT_EQ(contents.error().code, ErrorCode::DatabaseFailure);
    EXPECT_NE(contents.error().message.find("damaged"), std::string::npos);
    EXPECT_NE(contents.error().context.find("job-1"), std::string::npos);
}

TEST_F(SurveyJobStorage, SettingsWrittenByANewerVersionAreRefusedRatherThanHalfRead)
{
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(contentsWith({fullJob("job-1")})).ok());
    }
    {
        auto raw = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(raw.ok());
        ASSERT_TRUE(raw->execute("UPDATE survey_jobs SET settings ="
                                 " 'katana-reduction-settings=99' || char(10) || 'atmospheric=none'")
                        .ok());
    }
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok());
    const auto contents = store->load();
    ASSERT_FALSE(contents.ok());
    EXPECT_EQ(contents.error().code, ErrorCode::Unsupported);
    EXPECT_NE(contents.error().message.find("job-1"), std::string::npos);
}

// The packed lists are read back from a file that may have been damaged or
// edited by hand, so every shortened form of a sound one must fail the load
// with an error - never read past its end, never crash, never load as a
// different job.
TEST_F(SurveyJobStorage, EveryTruncationOfThePackedListsIsRefusedCleanly)
{
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(contentsWith({fullJob("job-1")})).ok());
    }
    std::int64_t placedSize = 0;
    std::int64_t createdSize = 0;
    {
        auto raw = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(raw.ok());
        auto sizes = raw->prepare(
            "SELECT length(placed_points), length(created_entities) FROM survey_jobs");
        ASSERT_TRUE(sizes.ok());
        ASSERT_TRUE(*sizes->step());
        placedSize = sizes->columnInt64(0);
        createdSize = sizes->columnInt64(1);
        ASSERT_TRUE(raw->execute("CREATE TABLE sound AS SELECT placed_points, created_entities"
                                 " FROM survey_jobs")
                        .ok());
    }
    ASSERT_GT(placedSize, 60);
    ASSERT_EQ(createdSize, 32); // four ids of eight bytes
    const auto loadAfter = [&](const std::string& sql) {
        {
            auto raw = SqliteDatabase::open(projectDir() / "project.db");
            EXPECT_TRUE(raw.ok());
            EXPECT_TRUE(raw->execute("UPDATE survey_jobs SET placed_points = (SELECT"
                                     " placed_points FROM sound), created_entities = (SELECT"
                                     " created_entities FROM sound);" + sql)
                            .ok());
        }
        auto store = ProjectStore::open(projectDir());
        EXPECT_TRUE(store.ok());
        return store->load();
    };
    for (std::int64_t keep = 0; keep < placedSize; ++keep) {
        const auto contents = loadAfter("UPDATE survey_jobs SET placed_points = substr(" +
                                        std::string("placed_points, 1, ") + std::to_string(keep) +
                                        ")");
        ASSERT_FALSE(contents.ok()) << "placed points cut to " << keep << " bytes";
    }
    for (std::int64_t keep = 1; keep < createdSize; ++keep) {
        if (keep % 8 == 0) {
            continue; // a whole number of ids is a shorter list, not damage
        }
        const auto contents = loadAfter("UPDATE survey_jobs SET created_entities = substr(" +
                                        std::string("created_entities, 1, ") +
                                        std::to_string(keep) + ")");
        ASSERT_FALSE(contents.ok()) << "created entities cut to " << keep << " bytes";
    }
    // And the sound bytes still load: the loop did not pass by always failing.
    const auto sound = loadAfter("");
    ASSERT_TRUE(sound.ok()) << sound.error().describe();
    EXPECT_EQ(sound->surveyJobs.front(), fullJob("job-1"));
}
