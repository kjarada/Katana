#pragma once

// What the RW5 and GTS-7 readers share: packed-angle parsing and the builder
// that turns a stream of raw total-station records into one SurveyProject.
//
// Internal to surveyio (no include/ header): the two readers in topcon_rw5.cpp
// and topcon_gts.cpp are its only users. Both formats are journals written as
// the field work happened - a setup, its backsight, its shots, a stored point -
// so they need the same bookkeeping: a point is named long before (or without
// ever) being given coordinates, a setup's observations share a numbering of
// pointings, and a code on a shot strings that shot's point into a feature.

#include <cstddef>
#include <cstdint>
#include <functional>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "katana/survey/data_model.hpp"
#include "katana/surveyio/format.hpp"

namespace katana::surveyio::topcon {

// ---- Angles -----------------------------------------------------------------

// "DDD.MMSSsss" - degrees, then two digits of minutes, two of seconds and any
// decimals of a second, packed into one decimal number. Both the TDS RW5
// specification ("Angle (See MO for units)", degrees) and the Topcon Link
// manual's GTS-7 sample (322.33160 is 322 33' 16.0") write degrees this way.
//
// Read from the TEXT, digit by digit, never as a double: 0.0044 is 0 00' 44",
// and (0.0044 * 100 - 0) * 100 in binary floating point is 43.99999999999999.
// Missing trailing digits are zeros ("90.3" is 90 30' 00"). A sign applies to
// the whole angle ("-0.3000" is minus thirty minutes). nullopt when the text is
// not such a number or its minutes or seconds are 60 or more.
[[nodiscard]] std::optional<double> packedDegreesToRadians(std::string_view text) noexcept;

// Plain decimal gons (400 to the circle).
[[nodiscard]] std::optional<double> gonsToRadians(std::string_view text) noexcept;

// Into [0, 2*pi).
[[nodiscard]] double wrapToCircle(double radians) noexcept;

// A real number occupying the whole (trimmed) token.
[[nodiscard]] std::optional<double> parseReal(std::string_view text) noexcept;

// Metres per international foot and per US survey foot, exact by definition
// (EPSG:9002 and EPSG:9003), for the unit switches both formats carry.
inline constexpr double kMetresPerFoot = 0.3048;
inline constexpr double kMetresPerUsSurveyFoot = 1200.0 / 3937.0;

// ---- The builder ------------------------------------------------------------

// Which face a zenith READING was taken on, from the reading itself: a circle
// reading past 180 degrees exists only on face right (reverse); one below it is
// face left. Exactly 0 or 180 says nothing.
[[nodiscard]] katana::survey::Face faceOfZenithReading(double zenithRadians) noexcept;

// A zenith reading as the model holds it: in [0, pi]. A face-right reading
// z > pi is the same line of sight as 2*pi - z on face left.
[[nodiscard]] double zenithInModelRange(double zenithRadians) noexcept;

class RawProjectBuilder {
  public:
    // `source` is copied into every SourceRecord with the record number set.
    RawProjectBuilder(std::string_view fileName, katana::survey::SourceRecord source,
                      const ReadOptions& options);

    [[nodiscard]] const ReadOptions& options() const { return options_; }
    [[nodiscard]] katana::survey::SurveyProject& project() { return result_.project; }
    [[nodiscard]] const std::string& fileName() const { return fileName_; }

    // Provenance of one record.
    [[nodiscard]] katana::survey::SourceRecord source(std::size_t record) const;

    // A warning about a record that WAS read (in part or in full).
    void warn(std::size_t record, std::string message);
    // A record that was not imported: a warning and one more skipped record.
    void skip(std::size_t record, std::string message);
    void countRead() { ++result_.recordsRead; }
    void notCarried(std::string text);

    // ---- Points
    //
    // Every id a record names is "mentioned": a point that is never given
    // coordinates ends up in SurveyProject::unpositionedPoints, one that is in
    // SurveyProject::points, each in order of first mention.

    void mentionPoint(std::string_view id, std::size_t record);
    // Coordinates for `id`, metres. The FIRST coordinates a file states are
    // kept: control is stored before it is observed, and a later value for the
    // same name is a check or a recomputation. A later value that differs is a
    // warning naming both records and is kept in the point's metadata, never
    // dropped; one that is the same is a repeat and needs no comment.
    void positionPoint(std::string_view id, double northing, double easting,
                       std::optional<double> elevation, katana::survey::CoordinateSource how,
                       std::size_t record);
    // The field code of `id` (the first word of `description` when `code` is
    // empty is the caller's business). A non-empty code strings the point into
    // the feature named by code and string number, in the order met.
    void codePoint(std::string_view id, std::string_view code, std::string_view description,
                   std::string_view stringNumber, std::size_t record);
    void addPointMetadata(std::string_view id, std::string key, std::string value);
    [[nodiscard]] bool hasPoint(std::string_view id) const;

    // ---- Setups

    // Starts a setup on `pointId`. Its id is the point id, or "<point> (2)",
    // "(3)" ... when the point is occupied again, so every station id is unique.
    katana::survey::SurveyStation& beginStation(std::string_view pointId, double instrumentHeight,
                                                const katana::survey::InstrumentSettings& settings,
                                                std::size_t record);
    [[nodiscard]] katana::survey::SurveyStation* currentStation();
    // The next pointing number of the current setup (1, 2, ...).
    [[nodiscard]] std::size_t nextPointing();
    void addStationObservation(katana::survey::Observation observation);
    void addLooseObservation(katana::survey::Observation observation);
    void addStationNote(std::string_view note);

    // The finished result. The builder is spent afterwards.
    [[nodiscard]] ReadResult finish();

  private:
    static constexpr std::uint32_t kNoFeature = UINT32_MAX;
    static constexpr std::uint32_t kNotFound = UINT32_MAX;

    struct PointEntry {
        std::string id;
        std::size_t hash = 0; // of id, kept so the table can grow without rehashing text
        bool positioned = false;
        double northing = 0.0;
        double easting = 0.0;
        std::optional<double> elevation;
        katana::survey::CoordinateSource coordinateSource =
            katana::survey::CoordinateSource::Unknown;
        std::size_t positionRecord = 0;
        std::size_t sourceRecord = 0; // the SourceRecord is built once, in finish()
        std::string code;
        std::string description;
        std::map<std::string, std::string> metadata;
        std::uint32_t feature = kNoFeature;
    };

    struct TransparentHash {
        using is_transparent = void;
        std::size_t operator()(std::string_view text) const noexcept
        {
            return std::hash<std::string_view>{}(text);
        }
    };
    using IdIndex = std::unordered_map<std::string, std::uint32_t, TransparentHash, std::equal_to<>>;

    PointEntry& entry(std::string_view id, std::size_t record);
    [[nodiscard]] std::uint32_t findPoint(std::string_view id, std::size_t hash) const;
    void placePoint(std::uint32_t index);

    std::string fileName_;
    katana::survey::SourceRecord sourceTemplate_;
    ReadOptions options_;
    ReadResult result_;
    std::vector<PointEntry> points_;
    // Point ids -> index in points_, open addressing over points_' own ids:
    // one table of 32-bit slots instead of a node and a second copy of the id
    // per point, which on a job of a million shots was a third of the read
    // (measured: 1.2 us per std::unordered_map insert on this toolchain).
    // 0 is an empty slot; otherwise index + 1. Kept at most half full.
    std::vector<std::uint32_t> pointSlots_;
    IdIndex stationIndex_;
    IdIndex occupations_; // setups begun on each point
    IdIndex featureIndex_;
    std::vector<std::size_t> pointingCounters_;
};

} // namespace katana::surveyio::topcon
