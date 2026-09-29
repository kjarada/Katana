#pragma once

// What the RW5, GTS-7 and opcode field file readers share: packed-angle
// parsing (RW5 and GTS-7 only) and the builder
// that turns a stream of raw total-station records into one SurveyProject.
//
// Internal to surveyio (no include/ header): the readers in topcon_rw5.cpp,
// topcon_gts.cpp and opcode_field_file.cpp are its users - the last reads no
// Topcon format but a journal of the same kind, and the namespace keeps the
// name it was given first. All three formats are journals written as
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
#include <utility>
#include <variant>
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

// ---- Offsets ------------------------------------------------------------------

// What a shot reads: a circle reading (radians, clockwise), a zenith in the
// model's range [0, pi] and a slope distance (metres).
struct ShotReading {
    double circle = 0.0;
    double zenith = 0.0;
    double slope = 0.0;
};

// The reading of the point a shot's offsets put it at, from what was measured:
// `radial` along the plan line from the instrument to the target, positive
// away from it; `tangential` at right angles to that line, positive to the
// right looking from the instrument (a clockwise circle reads it as more);
// `height` added to the target's rise. With plan distance h = s sin z and
// rise v = s cos z, the point is at plan distance h' = hypot(h + radial,
// tangential) on the circle a + atan2(tangential, h + radial), rising
// v' = v + height: the reading is that circle, the zenith atan2(h', v') and
// the slope distance hypot(h', v'). nullopt when that point is within
// math::tolerance::kCoordinate of the instrument - its own mark, to which no
// shot has a direction or a zenith.
//
// One home for the arithmetic of every reader on this builder whose format
// records offsets. The opcode field file applies its 42, 43 and 44 through it;
// the GTS-7 reader keeps its OFFSET records unapplied (topcon_gts.cpp).
[[nodiscard]] std::optional<ShotReading> offsetShot(const ShotReading& measured, double radial,
                                                    double tangential, double height) noexcept;

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
    // The same, assigned into an existing record (its strings' buffers reused).
    void stampSource(katana::survey::SourceRecord& target, std::size_t record) const
    {
        target = sourceTemplate_;
        target.recordNumber = record;
    }

    // A warning about a record that WAS read (in part or in full). The
    // first kListedWarnings are listed; past them each is counted and
    // finish() says how many more there were. A file of a hundred million
    // unreadable lines (1 GiB is readSurvey's cap) would otherwise become
    // gigabytes of warnings - the program stops answering while it
    // allocates, which the size cap exists to prevent.
    static constexpr std::size_t kListedWarnings = 10000;
    void warn(std::size_t record, std::string message);
    // A record that was not imported: a warning and one more skipped record.
    void skip(std::size_t record, std::string message);
    void countRead() { ++result_.recordsRead; }
    // Records not imported that one warning already accounts for (the lines
    // after a file's own end marker).
    void countSkipped(std::size_t count) { result_.recordsSkipped += count; }
    [[nodiscard]] std::size_t recordsSkipped() const { return result_.recordsSkipped; }
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
    // Closes the open feature of `code` and `stringNumber`
    // (SurveyFeature::closed) and takes it off the open list, so a later point
    // of that code and string number starts a new feature rather than joining
    // a closed one. The number of points in it; nullopt when none is open.
    std::optional<std::size_t> closeFeature(std::string_view code, std::string_view stringNumber);
    // The same for the feature `id` was first strung into, if still open.
    std::optional<std::size_t> closeFeatureOf(std::string_view id);
    // The last point of the open feature of `code` and `stringNumber`.
    [[nodiscard]] std::optional<std::string> lastPointOf(std::string_view code,
                                                         std::string_view stringNumber) const;
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
    // A new observation of kind T at the end of the current setup, with its
    // provenance filled in and every other field default, for the caller to
    // fill. Built where it will live because a survey::Observation is a
    // 576-byte variant: making one on the stack and handing it over costs two
    // moves of it and of its six strings, three times a shot.
    template <typename T>
    T& stationObservation(std::size_t record)
    {
        T& observation = *std::get_if<T>(&result_.project.stations.back().observations.emplace_back(
            std::in_place_type<T>));
        stampSource(observation.source, record);
        return observation;
    }
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
    // featureIndex_'s key: the code and the string number.
    [[nodiscard]] static std::string featureKey(std::string_view code, std::string_view stringNumber);

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
    std::uint32_t lastPoint_ = kNotFound; // the point entry() found or made last
    std::size_t unlistedWarnings_ = 0;
    std::size_t firstUnlistedRecord_ = 0;
    IdIndex stationIndex_;
    IdIndex occupations_; // setups begun on each point
    IdIndex featureIndex_; // the OPEN feature of each code and string number
    std::vector<std::size_t> pointingCounters_;
};

} // namespace katana::surveyio::topcon
