// Leica DBX (src/katana_surveyio/leica_dbx.cpp): recognised and refused.
//
// The fixtures data/leica/dbx/JOB1.XCF and JOB1.X01 are a few made-up binary
// bytes each - DBX has no published layout, so nothing about their content is
// meant to resemble one. What is tested is the recognition (by name, binary
// content and the job's other file beside it) and the sentence a person gets.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <iterator>
#include <random>
#include <string>
#include <string_view>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/leica.hpp"
#include "katana/surveyio/reader.hpp"

using namespace katana::surveyio;
using katana::core::ErrorCode;
using katana::core::Result;

namespace {

std::filesystem::path dbxFolder()
{
    return std::filesystem::path(__FILE__).parent_path() / "data" / "leica" / "dbx";
}

std::string slurp(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

double dbxConfidence(std::string_view bytes, std::string_view name)
{
    for (const FormatRegistry::ProbeResult& result :
         formatRegistry().probeAll(probeOf(bytes, name))) {
        if (result.formatId == kLeicaDbxFormatId) {
            return result.signature.confidence;
        }
    }
    return -1.0;
}

constexpr std::string_view kExportAdvice =
    "export the job from Captivate/Infinity as GSI-16 or LandXML (or HeXML) and import that";

} // namespace

TEST(LeicaDbx, TheFormatIsRegisteredAsReadingNothing)
{
    Result<FormatDescriptor> format = formatRegistry().find(kLeicaDbxFormatId);
    ASSERT_TRUE(format.ok());
    EXPECT_EQ(format->manufacturer, Manufacturer::Leica);
    EXPECT_EQ(format->reads, FormatContent{});
    EXPECT_NE(format->humanName.find("export GSI or LandXML"), std::string::npos);
    // A reader exists only to give the refusal (see leica_dbx.cpp).
    EXPECT_NE(formatRegistry().reader(kLeicaDbxFormatId), nullptr);
}

TEST(LeicaDbx, AJobsIndexFileIsIdentifiedAsDbx)
{
    const std::string bytes = slurp(dbxFolder() / "JOB1.XCF");
    ASSERT_FALSE(bytes.empty());
    const Detection detection = detectFormat(probeOf(bytes, "JOB1.XCF"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kLeicaDbxFormatId);
    EXPECT_NE(detection.candidates().front().evidence.find("index file"), std::string::npos);
    // A data file of the job is recognised as well.
    EXPECT_GE(dbxConfidence(slurp(dbxFolder() / "JOB1.X01"), "JOB1.X01"), 0.7);
}

TEST(LeicaDbx, AGimpImageATextFileAndOtherExtensionsAreNotDbx)
{
    EXPECT_EQ(dbxConfidence(std::string("gimp xcf v011\0\0\0", 16), "picture.xcf"), 0.0);
    EXPECT_LT(dbxConfidence("plain text, not a job\r\n", "notes.xcf"), 0.7);
    EXPECT_EQ(dbxConfidence(std::string("\0\1\2", 3), "job.gsi"), 0.0);
    EXPECT_EQ(dbxConfidence(std::string("\0\1\2", 3), "job.x00"), 0.0);
    EXPECT_EQ(dbxConfidence("110001+00000001 81..00+00001000 \r\n", "job.gsi"), 0.0);
}

TEST(LeicaDbx, ReadingAJobIsRefusedWithTheExportToMakeInstead)
{
    const std::string bytes = slurp(dbxFolder() / "JOB1.XCF");
    Result<ReadResult> read = readSurvey(formatRegistry(), kLeicaDbxFormatId, bytes, "JOB1.XCF",
                                         ReadOptions{siblingsInFolder(dbxFolder())});
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::Unsupported);
    // The data file is found beside the index (asked for as "JOB1.x01"; the
    // folder lookup ignores case), which confirms the file is a job.
    EXPECT_NE(read.error().message.find("This is a Leica DBX job (JOB1.XCF with JOB1.x01"),
              std::string::npos)
        << read.error().message;
    EXPECT_NE(read.error().message.find(kExportAdvice), std::string::npos);
}

TEST(LeicaDbx, AJobFileOnItsOwnIsRefusedWithTheSameAdvice)
{
    Result<ReadResult> read =
        readSurvey(formatRegistry(), kLeicaDbxFormatId, std::string("\0\1", 2), "LONE.X01");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::Unsupported);
    EXPECT_NE(read.error().message.find("looks like a file of a Leica DBX job"),
              std::string::npos);
    EXPECT_NE(read.error().message.find(kExportAdvice), std::string::npos);
}

TEST(LeicaDbx, NoOtherFormatsFixtureIsTakenForADbxJob)
{
    // Every fixture in this tree outside the DBX folder, whoever wrote it:
    // none of them is a DBX file, so the probe must rule each one out.
    const std::filesystem::path data = dbxFolder().parent_path().parent_path();
    std::size_t others = 0;
    for (const auto& entry : std::filesystem::recursive_directory_iterator(data)) {
        if (!entry.is_regular_file() || entry.path().parent_path() == dbxFolder()) {
            continue;
        }
        const std::string bytes = slurp(entry.path());
        EXPECT_EQ(dbxConfidence(std::string_view(bytes).substr(0, kProbeBytes),
                                entry.path().filename().string()),
                  0.0)
            << entry.path().string();
        ++others;
    }
    EXPECT_GT(others, 0U);
}

TEST(LeicaDbx, RandomBytesUnderAJobsNamesAreProbedAndRefusedWithoutACrash)
{
    // The reader looks at no byte of the file, and the probe only at whether
    // it is binary; both must still answer for any content, of any length.
    std::mt19937 random(20260924); // fixed: a failure must be repeatable
    std::uniform_int_distribution<int> byte(0, 255);
    for (int round = 0; round < 200; ++round) {
        std::string noise(static_cast<std::size_t>(round * 37 % 5000), '\0');
        for (char& c : noise) {
            c = static_cast<char>(byte(random));
        }
        for (const char* name : {"JOB.XCF", "job.x01", "job.x13"}) {
            const double confidence = dbxConfidence(noise, name);
            EXPECT_GE(confidence, 0.0) << name << " round " << round;
            EXPECT_LE(confidence, 1.0) << name << " round " << round;
            Result<ReadResult> read = readSurvey(formatRegistry(), kLeicaDbxFormatId, noise, name);
            ASSERT_FALSE(read.ok());
            EXPECT_EQ(read.error().code, ErrorCode::Unsupported);
        }
    }
}
