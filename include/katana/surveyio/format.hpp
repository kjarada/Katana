#pragma once

// Survey field-data formats: what each one is, what it can carry, and the
// registry every parser puts itself into.
//
// surveyio turns manufacturer field data - Leica, Trimble, Topcon, LandXML, a
// coordinate CSV - into katana::survey values and nothing else. It does not
// transform coordinates and it cannot: see survey::DeclaredCoordinateSystem for
// why recording what a file DECLARES is the only honest thing to do with it.
//
// Conventions inherited from katana::survey and not restated per field: metres,
// radians, azimuths clockwise in [0, 2*pi). A parser reading degrees or gons
// converts on the way in and records what it converted from in
// survey::DeclaredUnits.

#include <cstddef>
#include <functional>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/data_model.hpp"

namespace katana::surveyio {

// Who defined the format. This list is deliberately longer than the formats that
// exist today: adding a value is an edit to this line, and several parsers
// landing at once must not all need it. A vendor that is not here and is wanted
// by only one format is `Other`, spelled out in FormatDescriptor::humanName.
enum class Manufacturer {
    Generic,      // no vendor at all: a coordinate CSV, plain delimited text
    OpenStandard, // a published specification with no owner: LandXML and the like
    Leica,
    Trimble,
    Topcon,
    Sokkia,
    Nikon,
    Geomax,
    Carlson,
    Other,
};

[[nodiscard]] constexpr const char* toString(Manufacturer manufacturer)
{
    switch (manufacturer) {
    case Manufacturer::Generic:
        return "Generic";
    case Manufacturer::OpenStandard:
        return "Open standard";
    case Manufacturer::Leica:
        return "Leica";
    case Manufacturer::Trimble:
        return "Trimble";
    case Manufacturer::Topcon:
        return "Topcon";
    case Manufacturer::Sokkia:
        return "Sokkia";
    case Manufacturer::Nikon:
        return "Nikon";
    case Manufacturer::Geomax:
        return "GeoMax";
    case Manufacturer::Carlson:
        return "Carlson";
    case Manufacturer::Other:
        return "Other";
    }
    return "Other";
}

// What a format can carry.
//
// Stated per format so that the application can say what will be LOST before an
// import runs - "this format carries points only; the raw observations in the
// job will not come across" - instead of dropping half a file quietly, which
// PLAN.MD section 36 forbids.
struct FormatContent {
    bool points = false;
    bool observations = false;
    bool stations = false;         // instrument setups with their backsights
    bool features = false;         // coded strings
    bool coordinateSystem = false; // the file declares one (declares, never transforms)
    bool instrumentSettings = false; // survey::InstrumentSettings per setup
    bool gnss = false;               // GNSS positions, vectors or sessions

    friend bool operator==(const FormatContent&, const FormatContent&) = default;
};

// One format, as the user interface should describe it.
//
// The point of this type is that the application can say "Leica GSI-16, import:
// yes, export: no, parser: 1.0" rather than "all Leica files supported", which is
// a promise no parser keeps. `parserVersion` is the version of THIS code, not of
// the format: when a file imports differently after an upgrade, that string is
// what says so, and it is the string that lands in survey::SourceRecord.
struct FormatDescriptor {
    // Stable identity: lower case letters, digits and '-' only. It appears in
    // saved settings, in SourceRecord and in this library's own error messages,
    // so it never changes and is never translated. e.g. "leica-gsi16".
    std::string id;
    std::string humanName; // "Leica GSI-16"
    Manufacturer manufacturer = Manufacturer::Generic;
    FormatContent reads;   // what an import of this format can produce
    bool canImport = false;
    bool canExport = false;
    std::string parserVersion;           // this parser's version, e.g. "1.0"
    std::vector<std::string> extensions; // lower case, no dot, e.g. {"gsi"}

    friend bool operator==(const FormatDescriptor&, const FormatDescriptor&) = default;
};

// "Leica GSI-16, import: yes, export: no, parser: 1.0" - one phrasing, so every
// place that lists formats lists them the same way.
[[nodiscard]] std::string describeFormat(const FormatDescriptor& descriptor);

// ---- Detection probes ------------------------------------------------------------

// What a probe is given, prepared once and shared by every probe so that
// detection reads the file once however many formats are registered.
struct ProbeInput {
    // The first kProbeBytes of the file (detect.hpp), or all of it when it is
    // shorter. A VIEW: whatever owns the bytes must outlive the ProbeInput.
    std::string_view bytes;
    // File NAME only, never a path - built with survey::sourceFileName(), for the
    // reason given there. A probe is never handed something it could open.
    std::string fileName;
    std::string extension;  // lower case, no dot; empty when the name has none
    bool truncated = false; // the file is longer than `bytes`
};

// A probe's answer about ONE format.
//
// `confidence` is in [0, 1]: 0 rules the format out, 1 means a signature that
// cannot belong to anything else. Anything outside that range is a bug in the
// probe and the candidate is dropped and reported, never clamped - a clamp would
// turn a broken probe into a confident one. `evidence` says WHY in a few words,
// because a detection a person has to second-guess is one they cannot check:
// "extension .gsi, every record 24 bytes, word 11 first".
struct FormatSignature {
    double confidence = 0.0;
    std::string evidence;

    friend bool operator==(const FormatSignature&, const FormatSignature&) = default;
};

// Named for the call site: `return ruledOut();` reads better than `return {};`
// where the whole point of the line is that this is not the format.
[[nodiscard]] inline FormatSignature ruledOut()
{
    return {};
}

// A plain function pointer rather than std::function: a probe is a free function
// in a parser's own translation unit, it captures nothing, and the registry is
// then a value that allocates nothing per format.
using FormatProbe = FormatSignature (*)(const ProbeInput&);

// ---- Readers ---------------------------------------------------------------------
//
// A format that can be imported in one step registers a READER beside its
// probe: bytes in, a survey::SurveyProject out. readSurvey() (reader.hpp) is
// how anything calls one; it adds the checks every reader would otherwise
// repeat (size caps, the sibling-file rule, validateProject) and stamps the
// result with the format's id and parser version.

// Another file of the same job, by name and content.
struct SiblingFile {
    std::string name; // a file NAME, never a path
    std::string bytes;

    friend bool operator==(const SiblingFile&, const SiblingFile&) = default;
};

// Returns the bytes of the file called `name` IN THE SAME FOLDER as the file
// being read. Some formats are more than one file: a Leica DBX job is a folder
// of files that name each other; a RINEX observation file has its navigation
// file beside it.
//
// SECURITY. A reader passes a NAME it found in the file, and the name is
// untrusted: the lookup refuses anything survey::sourceFileName() would
// change (a directory, "..", a drive, an alternate data stream), and the
// implementations in reader.hpp read only regular files in the one folder,
// capped in size. NotFound when there is no such file - a reader that can do
// without it goes on with a warning; one that cannot fails, naming the file.
using SiblingLookup = std::function<katana::core::Result<std::string>(std::string_view name)>;

struct ReadOptions {
    // Empty: no sibling can be read (a single file handed over on its own).
    SiblingLookup siblings{};
    // Standard deviations for observations whose file states none: every
    // observation must carry sigma > 0 (survey::validateObservation). The
    // reduction replaces them with the person's own (reduction_settings.hpp),
    // so these only have to be sensible, not right.
    katana::survey::ObservationPrecision precision{};
};

// A warning with the place it is about, so a person can open the file at the
// record. `record` is 1-based: a line for a text format, a record or block for
// a binary one; 0 when the warning is about the file as a whole.
struct ReadWarning {
    std::string fileName{}; // the file read, or the sibling the warning is about
    std::size_t record = 0;
    std::string message{};

    friend bool operator==(const ReadWarning&, const ReadWarning&) = default;
};

// "job.gsi record 12: target height 99.999 read as unset" - one phrasing, so
// seven readers do not invent seven.
[[nodiscard]] std::string describe(const ReadWarning& warning);

struct ReadResult {
    katana::survey::SurveyProject project{};
    // A record that cannot be read is a WARNING naming it, never a silently
    // dropped value; an unreadable FILE is an error in the Result instead.
    std::vector<ReadWarning> warnings{};
    std::size_t recordsRead = 0;
    // Records the format defines that this reader does not handle, counted
    // (see ImportResult::recordsSkipped).
    std::size_t recordsSkipped = 0;
    // What this FILE did not carry that an import usually needs, in words:
    // "no instrument heights", "no atmospheric settings - the atmospheric
    // correction state is unknown". Shown before the import and in the report.
    std::vector<std::string> notCarried{};

    // Filled by readSurvey(), not by the reader:
    std::string formatId{};
    std::string parserVersion{};
    std::vector<SiblingFile> siblingsRead{}; // every sibling the reader fetched, in fetch order
};

// A reader: the file's bytes, its NAME (survey::sourceFileName already
// applied) and the options. A plain function pointer for the reason given
// beside FormatProbe. It must not open anything itself; other files come
// through ReadOptions::siblings.
using FormatReader = katana::core::Result<ReadResult> (*)(std::string_view bytes,
                                                          std::string_view fileName,
                                                          const ReadOptions& options);

// ---- The registry ----------------------------------------------------------------

// The formats one program knows about.
//
// A value, not a singleton. The process-wide instance is formatRegistry() below,
// but a test (or a caller that wants to detect against a restricted set) builds
// its own, which is why registration is a method and not a free function.
//
// Ordered by id, never hashed, so that iteration - and therefore the ranking of
// two equally confident candidates - is the same on every run.
class FormatRegistry {
  public:
    // InvalidArgument when the descriptor is not usable: an empty or non-slug id,
    // an empty human name or parser version, no probe, a format that can
    // neither import nor export, or a reader for a format that cannot import.
    // AlreadyExists when the id is taken - two parsers answering to one name is
    // a programming error, not a preference. `reader` may be null: a format
    // can be detected without being readable in one step (see reader.hpp for
    // the delimited-points format, which is read through its column layout).
    katana::core::Status add(FormatDescriptor descriptor, FormatProbe probe,
                             FormatReader formatReader = nullptr);

    [[nodiscard]] std::vector<FormatDescriptor> formats() const;
    [[nodiscard]] katana::core::Result<FormatDescriptor> find(std::string_view id) const;
    [[nodiscard]] bool contains(std::string_view id) const;
    // The reader registered for `id`, or nullptr when there is none (or no
    // such format).
    [[nodiscard]] FormatReader reader(std::string_view id) const;
    [[nodiscard]] std::size_t size() const { return formats_.size(); }
    [[nodiscard]] bool empty() const { return formats_.empty(); }

    // Every registered probe run against one input, in id order. detect.hpp turns
    // these into a ranked Detection; this is the raw material and is exposed so
    // that the ranking can be tested apart from the probing.
    struct ProbeResult {
        std::string formatId;
        FormatSignature signature;
    };
    [[nodiscard]] std::vector<ProbeResult> probeAll(const ProbeInput& input) const;

  private:
    struct Entry {
        FormatDescriptor descriptor;
        FormatProbe probe = nullptr;
        FormatReader reader = nullptr;
    };
    std::map<std::string, Entry, std::less<>> formats_;
};

// The registry the whole program shares. Function-local, so it is built on first
// use: a namespace-scope registry would be a static initialisation order race
// against the self-registering objects below, which run in an order the standard
// does not define.
[[nodiscard]] FormatRegistry& formatRegistry();

// ---- How a parser adds its format ------------------------------------------------
//
// THIS IS THE CONTRACT. A format is added from the parser's OWN translation unit,
// so that parsers written at the same time never touch a shared table:
//
//     // src/katana_surveyio/leica_gsi.cpp
//     namespace {
//
//     katana::surveyio::FormatSignature probeGsi(const katana::surveyio::ProbeInput& input)
//     {
//         if (input.extension != "gsi") {
//             return katana::surveyio::ruledOut();
//         }
//         ...
//         return {0.95, "extension .gsi and every record 24 bytes"};
//     }
//
//     katana::surveyio::FormatDescriptor descriptor()
//     {
//         katana::surveyio::FormatDescriptor format;
//         format.id = "leica-gsi16";
//         format.humanName = "Leica GSI-16";
//         format.manufacturer = katana::surveyio::Manufacturer::Leica;
//         format.reads = {.points = true, .observations = true, .stations = true};
//         format.canImport = true;
//         format.parserVersion = "1.0";
//         format.extensions = {"gsi"};
//         return format;
//     }
//
//     katana::core::Result<katana::surveyio::ReadResult>
//     readGsi(std::string_view bytes, std::string_view fileName,
//             const katana::surveyio::ReadOptions& options)
//     {
//         ... stream over `bytes` with std::from_chars; a bad record is a
//         ReadWarning naming it, never a dropped value ...
//     }
//
//     const katana::surveyio::FormatRegistration kRegistration{descriptor(), &probeGsi,
//                                                              &readGsi};
//
//     } // namespace
//
// What the mechanism depends on:
//   * The id is yours alone. A clash is a programming error and this constructor
//     throws rather than let two formats answer to one name; FormatRegistry::add
//     is the same check as a Status, for callers building their own registry.
//   * Put the file in src/katana_surveyio/ and add nothing to any CMakeLists:
//     that directory is globbed precisely so that adding a parser is adding a
//     file. Your object file defines nothing anything else references, so the
//     module's archive is linked whole to stop the linker dropping it; the
//     measurement behind that is in src/katana_surveyio/CMakeLists.txt.
//   * A probe must be cheap and must not open anything. It gets the first
//     kProbeBytes of the file and the file's NAME, and that is all it may look at.
//   * Registration order is irrelevant. The registry is ordered by id and the
//     ranking never depends on who registered first.
class FormatRegistration {
  public:
    // Throws std::logic_error when the registry rejects the descriptor. This runs
    // during static initialisation, where there is no caller to hand a Status to,
    // and a format that silently failed to register would show up as "this file
    // is not recognised" months later - the silent failure PLAN.MD section 36
    // forbids. Failing at start-up with the reason is the loud alternative.
    FormatRegistration(FormatDescriptor descriptor, FormatProbe probe,
                       FormatReader reader = nullptr);

    FormatRegistration(const FormatRegistration&) = delete;
    FormatRegistration& operator=(const FormatRegistration&) = delete;
};

// ---- What a parser returns -------------------------------------------------------

// One shape for every importer, so that five of them do not return five.
//
// A warning is a sentence a person can act on ("record 412: target height 99.999
// read as unset"). It is never how a failure is reported: a failure is an error
// in the katana::core::Result the importer returns.
struct ImportResult {
    katana::survey::SurveyProject project;
    std::string formatId; // the FormatDescriptor::id that read it
    std::vector<std::string> warnings;
    std::size_t recordsRead = 0;
    // Records the format defines and this parser does not yet handle. Counted
    // rather than ignored, so "it imported fine" and "it imported the third of
    // the file I understand" are distinguishable.
    std::size_t recordsSkipped = 0;
};

} // namespace katana::surveyio
