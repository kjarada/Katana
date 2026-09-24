#pragma once

// What the RW5 and GTS tests share: fixtures on disk, the angles the expected
// values are worked in, and finding things in a read project.

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

// Every prefix of `bytes`, and bytes of every value, must come back as an
// error or a result - never a crash, never a hang. Returns how many reads
// were made so a test can say it made them.
inline std::size_t readEveryTruncationAndNoise(std::string_view formatId, std::string_view bytes,
                                               std::string_view name)
{
    std::size_t reads = 0;
    for (std::size_t length = 0; length <= bytes.size(); ++length) {
        const auto result = read(formatId, bytes.substr(0, length), name);
        (void)result.ok();
        ++reads;
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
        (void)read(formatId, copy, name).ok();
        ++reads;
        std::string noise(static_cast<std::size_t>(byte(random)) * 4, '\0');
        for (char& c : noise) {
            c = static_cast<char>(byte(random));
        }
        (void)read(formatId, noise, name).ok();
        ++reads;
    }
    return reads;
}

} // namespace topcon_test
