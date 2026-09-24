// Trimble Survey Controller DC (src/katana_surveyio/trimble_dc.cpp): recognised
// and refused, because Trimble has not published its record layouts (the
// reasons are in that file). The fixture is synthetic: lines in the shape of
// the records, never interpreted beyond their record type.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <random>
#include <sstream>
#include <string>
#include <vector>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/reader.hpp"

using namespace katana::surveyio;
using katana::core::ErrorCode;
using katana::core::Result;

namespace {

constexpr std::string_view kId = "trimble-dc";

std::filesystem::path dataDir()
{
    return std::filesystem::path(__FILE__).parent_path() / "data";
}

std::string readFile(const std::filesystem::path& path)
{
    std::ifstream stream(path, std::ios::binary);
    std::ostringstream out;
    out << stream.rdbuf();
    return out.str();
}

const std::string& fixture()
{
    static const std::string bytes = readFile(dataDir() / "trimble_dc" / "synthetic_job.dc");
    return bytes;
}

double dcConfidence(std::string_view bytes, std::string_view name)
{
    for (const auto& result : formatRegistry().probeAll(probeOf(bytes, name))) {
        if (result.formatId == kId) {
            return result.signature.confidence;
        }
    }
    return -1.0; // not registered at all
}

} // namespace

TEST(TrimbleDc, isRegisteredSayingItReadsNothing)
{
    Result<FormatDescriptor> format = formatRegistry().find(kId);
    ASSERT_TRUE(format.ok());
    EXPECT_EQ(format->manufacturer, Manufacturer::Trimble);
    EXPECT_EQ(format->reads, FormatContent{});
    EXPECT_NE(format->humanName.find("export JobXML"), std::string::npos);
    EXPECT_EQ(format->extensions, std::vector<std::string>{"dc"});
}

TEST(TrimbleDc, aSurveyControllerFileIsIdentifiedByItsHeaderRecord)
{
    const Detection detection = detectFormat(probeOf(fixture(), "synthetic_job.dc"));
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, kId);
    EXPECT_NE(detection.candidates().front().evidence.find("Survey Controller"), std::string::npos);
    // The header alone identifies it whatever the file is called.
    EXPECT_GE(dcConfidence(fixture(), "renamed.txt"), 0.9);
}

TEST(TrimbleDc, readingItIsRefusedWithWhatToExportInsteadAndWhatTheFileHolds)
{
    Result<ReadResult> read = readSurvey(formatRegistry(), kId, fixture(), "synthetic_job.dc");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::Unsupported);
    EXPECT_NE(read.error().message.find("JobXML (.jxl)"), std::string::npos);
    EXPECT_NE(read.error().message.find("synthetic_job.dc"), std::string::npos);
    // 8 lines: 00 x1, 10 x1, 13 x1, 77 x1, D9 x3, E0 x1 - counted by hand.
    EXPECT_EQ(read.error().context, "8 record(s): 00 x1 10 x1 13 x1 77 x1 D9 x3 E0 x1");
}

TEST(TrimbleDc, aSokkiaSdrFileIsNotClaimedEvenWhenNamedDc)
{
    const std::string sdr = "00NMSDR33 V04-04.02    01-Jan-24 00:00 111111\n"
                            "10NMJOB1           111111\n"
                            "02TP0001    1000.0000 2000.0000 50.000 1.500\n";
    EXPECT_EQ(dcConfidence(sdr, "job.dc"), 0.0);
    EXPECT_EQ(dcConfidence(sdr, "job.sdr"), 0.0);
}

TEST(TrimbleDc, otherFormatsAreNotClaimedByTheDcProbe)
{
    std::vector<std::pair<std::string, std::string>> others = {
        {"tps_gnss_job.jxl", readFile(dataDir() / "trimble_jxl" / "tps_gnss_job.jxl")},
        {"job.gsi", "*110001+0000000000000A01 21.022+0000000009000000 22.022+0000000009000000\n"},
        {"job.rw5", "JB,NMJOB,DT03-05-2024,TM09:00:00\nMO,AD0,UN2,SF1.00000000,EC0,EO0.0,AU0\n"},
        {"obs.24o", "     3.04           OBSERVATION DATA    M                   RINEX VERSION / TYPE\n"},
    };
    for (const std::filesystem::directory_entry& entry :
         std::filesystem::directory_iterator(dataDir())) {
        if (entry.is_regular_file()) {
            others.emplace_back(entry.path().filename().string(), readFile(entry.path()));
        }
    }
    for (const auto& [name, bytes] : others) {
        EXPECT_EQ(dcConfidence(bytes, name), 0.0) << name;
    }
}

TEST(TrimbleDc, aDcExtensionAloneIsOnlyAWeakSuggestion)
{
    EXPECT_LE(dcConfidence("hello world\n", "notes.dc"), 0.1);
    const Detection detection = detectFormat(probeOf("hello world\n", "notes.dc"));
    EXPECT_NE(detection.outcome(), DetectionOutcome::Identified);
}

TEST(TrimbleDc, everyTruncationAndRandomBytesProbeAndRefuseWithoutACrash)
{
    const std::string& bytes = fixture();
    for (std::size_t length = 0; length <= bytes.size(); ++length) {
        const std::string_view cut = std::string_view(bytes).substr(0, length);
        const double confidence = dcConfidence(cut, "synthetic_job.dc");
        EXPECT_GE(confidence, 0.0);
        EXPECT_LE(confidence, 1.0);
        Result<ReadResult> read = readSurvey(formatRegistry(), kId, cut, "synthetic_job.dc");
        EXPECT_FALSE(read.ok());
    }
    std::mt19937 random(7u);
    for (int round = 0; round < 200; ++round) {
        std::string noise(static_cast<std::size_t>(random() % 4096), '\0');
        for (char& c : noise) {
            c = static_cast<char>(random() & 0xFF);
        }
        const double confidence = dcConfidence(noise, "noise.dc");
        EXPECT_GE(confidence, 0.0);
        EXPECT_LE(confidence, 1.0);
        EXPECT_FALSE(readSurvey(formatRegistry(), kId, noise, "noise.dc").ok());
    }
}
