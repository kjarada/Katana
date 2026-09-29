#pragma once

// What the tests of the readers on the raw builder share (RW5, GTS, the opcode
// field file, Sokkia SDR): fixtures on disk, the angles the expected values
// are worked in, and finding things in a read project.

#include <gtest/gtest.h>

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <numbers>
#include <random>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

#include "katana/survey/data_model.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/reader.hpp"

namespace topcon_test {

inline std::filesystem::path dataDirectory()
{
    return std::filesystem::path(__FILE__).parent_path() / "data";
}

inline std::string fixture(const std::filesystem::path& relative)
{
    std::ifstream stream(dataDirectory() / relative, std::ios::binary);
    EXPECT_TRUE(stream.good()) << "missing fixture " << relative.string();
    return std::string(std::istreambuf_iterator<char>(stream), std::istreambuf_iterator<char>());
}

// Degrees, minutes and seconds to radians, the way a surveyor writes the
// expected value down: through whole arc-seconds.
inline double dms(int degrees, int minutes, double seconds)
{
    return ((degrees * 60.0 + minutes) * 60.0 + seconds) * (std::numbers::pi / 648000.0);
}

inline katana::core::Result<katana::surveyio::ReadResult> read(std::string_view formatId,
                                                               std::string_view bytes,
                                                               std::string_view name)
{
    return katana::surveyio::readSurvey(katana::surveyio::formatRegistry(), formatId, bytes, name);
}

inline const katana::survey::SurveyPoint* point(const katana::survey::SurveyProject& project,
                                                std::string_view id)
{
    for (const katana::survey::SurveyPoint& p : project.points) {
        if (p.id == id) {
            return &p;
        }
    }
    return nullptr;
}

inline const katana::survey::UnpositionedPoint*
unpositioned(const katana::survey::SurveyProject& project, std::string_view id)
{
    for (const katana::survey::UnpositionedPoint& p : project.unpositionedPoints) {
        if (p.id == id) {
            return &p;
        }
    }
    return nullptr;
}

inline const katana::survey::SurveyFeature* feature(const katana::survey::SurveyProject& project,
                                                    std::string_view code)
{
    for (const katana::survey::SurveyFeature& f : project.features) {
        if (f.code == code) {
            return &f;
        }
    }
    return nullptr;
}

// The observations of kind T a station holds towards `to`, in file order.
template <typename T>
std::vector<T> observationsTo(const katana::survey::SurveyStation& station, std::string_view to)
{
    std::vector<T> found;
    for (const katana::survey::Observation& observation : station.observations) {
        if (const T* typed = std::get_if<T>(&observation)) {
            if constexpr (requires { typed->to; }) {
                if (typed->to == to) {
                    found.push_back(*typed);
                }
            }
        }
    }
    return found;
}

inline bool anyWarningContains(const katana::surveyio::ReadResult& result, std::size_t record,
                               std::string_view text)
{
    for (const katana::surveyio::ReadWarning& warning : result.warnings) {
        if (warning.record == record && warning.message.find(text) != std::string::npos) {
            return true;
        }
    }
    return false;
}

inline bool anyNotCarriedContains(const katana::surveyio::ReadResult& result,
                                  std::string_view text)
{
    for (const std::string& missing : result.notCarried) {
        if (missing.find(text) != std::string::npos) {
            return true;
        }
    }
    return false;
}

// Short samples of the OTHER field formats Katana reads, written here by hand
// in each format's own layout (GSI words, JobXML elements, Survey Controller
// records, RINEX header labels in columns 61-80), so that the Topcon probes
// can be shown not to claim them without these tests depending on another
// reader's fixtures.
struct ForeignSample {
    std::string_view name;
    std::string_view bytes;
};

inline constexpr ForeignSample kForeignSamples[] = {
    {"station.gsi", "*110001+0000000000000001 81..10+0000000000100000 82..10+0000000000200000 "
                    "83..10+0000000000005000 \r\n"
                    "*110002+0000000000000002 21.022+0000000004500000 22.022+0000000009000000 "
                    "31..00+0000000000141421 87..10+0000000000001600 \r\n"},
    {"station8.gsi", "110001+00000001 81..10+00100000 82..10+00200000 83..10+00005000 \r\n"
                     "110002+00000002 21.022+04500000 22.022+09000000 31..00+00141421 \r\n"
                     "410003+000000KB 42....+00000001 \r\n"},
    {"job.jxl", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
                "<JOBFile jobName=\"SITE\" version=\"5.6\" product=\"Field\">\n<FieldBook>\n"
                "<StationRecord ID=\"1\"><StationName>SS1</StationName>"
                "<TheodoliteHeight>1.5</TheodoliteHeight></StationRecord>\n"
                "<PointRecord ID=\"2\"><Name>BS</Name><Circle><HorizontalCircle>0</HorizontalCircle>"
                "<VerticalCircle>90</VerticalCircle><EDMDistance>10</EDMDistance></Circle>"
                "</PointRecord>\n</FieldBook>\n</JOBFile>\n"},
    {"site.dc", "00NMSC V10-70       000001-Jan-26 08:00 113111\n"
                "10NMSITE            121111\n"
                "E0NM             S1        1.500000000000000\n"},
    {"base2560.24o",
     "     3.04           OBSERVATION DATA    M                   RINEX VERSION / TYPE\n"
     "hand written        katana tests        20240912 120000 UTC PGM / RUN BY / DATE\n"
     "BASE                                                        MARKER NAME\n"
     "  -4052052.7340  4212836.1970 -2545105.1730                  APPROX POSITION XYZ\n"
     "        1.5000        0.0000        0.0000                  ANTENNA: DELTA H/E/N\n"
     "G    4 C1C L1C D1C S1C                                      SYS / # / OBS TYPES\n"
     "                                                            END OF HEADER\n"
     "> 2024 09 12 12 00  0.0000000  0  1\n"
     "G05  23629347.915   124171637.94305     -353.232          45.000\n"},
    {"base2560.24n",
     "     2.11           N: GPS NAV DATA                         RINEX VERSION / TYPE\n"
     "hand written        katana tests        20240912 120000 UTC PGM / RUN BY / DATE\n"
     "                                                            END OF HEADER\n"
     " 5 24  9 12 12  0  0.0 1.234567890123D-04 1.023181539495D-11 0.000000000000D+00\n"},
    {"points.xml", "<?xml version=\"1.0\"?>\n"
                   "<LandXML xmlns=\"http://www.landxml.org/schema/LandXML-1.2\" version=\"1.2\">\n"
                   "<CgPoints><CgPoint name=\"SS\">1000 2000 50</CgPoint></CgPoints>\n"
                   "</LandXML>\n"},
};

// Every prefix of `bytes`, and bytes of every value, must come back as an
// error or a result - never a crash, never a hang, and never an Internal
// error: readSurvey turns an exception a reader throws into one, so a reader
// that threw on every input would otherwise pass as having answered. Returns
// how many reads were made so a test can say it made them.
inline std::size_t readEveryTruncationAndNoise(std::string_view formatId, std::string_view bytes,
                                               std::string_view name)
{
    std::size_t reads = 0;
    const auto answered = [&](const katana::core::Result<katana::surveyio::ReadResult>& result,
                              std::string_view what) {
        if (!result.ok() && result.error().code == katana::core::ErrorCode::Internal) {
            ADD_FAILURE() << result.error().describe() << " (" << what << ")";
        }
        ++reads;
    };
    for (std::size_t length = 0; length <= bytes.size(); ++length) {
        answered(read(formatId, bytes.substr(0, length), name), "a truncation");
    }
    std::mt19937 random(20260924u);
    std::uniform_int_distribution<int> byte(0, 255);
    std::string mutated(bytes);
    for (int round = 0; round < 300; ++round) {
        std::string copy = mutated;
        std::uniform_int_distribution<std::size_t> where(0, copy.empty() ? 0 : copy.size() - 1);
        for (int flips = 0; flips < 8 && !copy.empty(); ++flips) {
            copy[where(random)] = static_cast<char>(byte(random));
        }
        answered(read(formatId, copy, name), "bytes changed at random");
        std::string noise(static_cast<std::size_t>(byte(random)) * 4, '\0');
        for (char& c : noise) {
            c = static_cast<char>(byte(random));
        }
        answered(read(formatId, noise, name), "random bytes");
    }
    return reads;
}

} // namespace topcon_test
