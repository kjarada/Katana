#pragma once

// Which format is this file?
//
// Detection inspects the extension, the leading bytes, the record layout and any
// known signature, and returns a RANKED list of candidates. It is deliberately
// hard to misuse: the answer is not a format id, it is a Detection whose
// `format()` hands back a descriptor only when one format actually won. Every
// other outcome comes back as a [[nodiscard]] Result the caller has to look at,
// and -Werror turns ignoring it into a build failure. Parsing an unknown file
// with an incompatible parser does not corrupt the file, it corrupts the survey
// that is built from it, which is found out much later.
//
// The formats themselves are not here. Each parser registers its own probe from
// its own translation unit - see the contract in format.hpp.

#include <cstddef>
#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/surveyio/format.hpp"

namespace katana::surveyio {

// How much of a file detection reads.
//
// Policy, not a measurement. 64 KiB is enough for the header and first entities
// of an XML document and for several hundred records of any fixed-width
// instrument format, and small enough that probing a file on a network share is
// not a pause a person notices. A probe that cannot decide within it must say so
// with a middling confidence rather than ask for more.
inline constexpr std::size_t kProbeBytes = 64 * 1024;

// The bar a format has to clear before a caller may use it without asking.
// Policy numbers, stated once here so that every format is judged by the same
// bar rather than by whatever each probe thinks "confident" means.
//   kIdentifiedConfidence  the winner must be at least this sure of itself.
//   kIdentificationMargin  and must beat the runner-up by at least this much.
// Two formats that both look 0.8 likely is precisely the case where guessing
// corrupts data, so that is Ambiguous and the caller has to choose.
inline constexpr double kIdentifiedConfidence = 0.7;
inline constexpr double kIdentificationMargin = 0.15;

enum class DetectionOutcome {
    Identified, // one format cleared the bar and left the next one behind
    Ambiguous,  // two or more are credible and too close to separate
    Uncertain,  // nothing was confident enough; the candidates say what was close
    Empty,      // there were no bytes to look at
};

[[nodiscard]] const char* toString(DetectionOutcome outcome);

// One format's answer, with the reason it gave.
struct FormatCandidate {
    std::string formatId;
    double confidence = 0.0;
    std::string evidence;

    friend bool operator==(const FormatCandidate&, const FormatCandidate&) = default;
};

// The result of looking at a file.
//
// Built only through the factories, so the invariant holds: a descriptor exists
// exactly when the outcome is Identified. There is no accessor that returns a
// plausible-looking format id for an uncertain file, because that is the
// mistake this type exists to prevent.
class Detection {
  public:
    // `ranked` is best first; detectFormat() is what normally builds these.
    [[nodiscard]] static Detection empty();
    [[nodiscard]] static Detection uncertain(std::vector<FormatCandidate> ranked,
                                             std::string summary);
    [[nodiscard]] static Detection ambiguous(std::vector<FormatCandidate> ranked,
                                             std::string summary);
    [[nodiscard]] static Detection identified(FormatDescriptor descriptor,
                                              std::vector<FormatCandidate> ranked,
                                              std::string summary);

    [[nodiscard]] DetectionOutcome outcome() const { return outcome_; }

    // The format to parse with - and the ONLY way to get one. NotFound for every
    // outcome but Identified, with the summary as the message and the ranked
    // candidates as the context, so a caller that just propagates the error still
    // reports something a person can act on.
    [[nodiscard]] katana::core::Result<FormatDescriptor> format() const;

    // Best first; equal confidences broken by id so the order never depends on
    // registration order. Formats that ruled themselves out are not listed.
    [[nodiscard]] const std::vector<FormatCandidate>& candidates() const { return candidates_; }

    // One sentence for a person: what was decided and why.
    [[nodiscard]] const std::string& summary() const { return summary_; }

  private:
    DetectionOutcome outcome_ = DetectionOutcome::Empty;
    std::optional<FormatDescriptor> descriptor_;
    std::vector<FormatCandidate> candidates_;
    std::string summary_;
};

// ---- Detecting -------------------------------------------------------------------

// Builds what every probe is given. `fileName` may be anything - a name, a path,
// something a file supplied - and only its name part survives
// (survey::sourceFileName). The extension is lower-cased here so that no probe
// has to do it and none of them disagree about how.
//
// `bytes` is not copied: it must outlive the ProbeInput.
[[nodiscard]] ProbeInput probeOf(std::string_view bytes, std::string_view fileName,
                                 bool truncated = false);

// Runs every registered probe and ranks what comes back.
[[nodiscard]] Detection detectFormat(const ProbeInput& input);
[[nodiscard]] Detection detectFormat(const ProbeInput& input, const FormatRegistry& registry);

// Reads at most kProbeBytes from `path` and detects on those. FileImportFailure
// when the file cannot be opened or read; an existing but empty file is not a
// failure, it is DetectionOutcome::Empty.
[[nodiscard]] katana::core::Result<Detection> detectFile(const std::filesystem::path& path);
[[nodiscard]] katana::core::Result<Detection> detectFile(const std::filesystem::path& path,
                                                         const FormatRegistry& registry);

// ---- Helpers for probes ----------------------------------------------------------
//
// Here rather than in each parser so that five probes do not answer the same
// question five ways, and so that adding a parser stays a one-file change.

// `bytes` with any UTF-8, UTF-16 or UTF-32 byte order mark stepped over. A file
// saved by a Windows editor carries one, and without this every signature test
// would have to know that "<LandXML" might not start at byte 0.
//
// Not archive12d's decodeText: that is a full decoder in a layer surveyio may not
// see (tools/check_layering.cmake), and a probe must not decode a file merely to
// look at it.
[[nodiscard]] std::string_view withoutByteOrderMark(std::string_view bytes);

// At most `count` lines of the probe's bytes, without their terminators. CR, LF
// and CRLF all end a line, because instrument files come off every operating
// system there is. When the input is truncated the last line is dropped - half a
// record is not a record, and a probe that measured its length would be measuring
// where kProbeBytes fell.
[[nodiscard]] std::vector<std::string_view> probeLines(const ProbeInput& input,
                                                       std::size_t count);

// True when the bytes are plausibly the text an instrument format is written in:
// no NUL, and control bytes other than tab, CR and LF no more than one byte in a
// hundred. UTF-16 fails this test - it is full of NULs - and that is deliberate:
// no format read here is UTF-16, and a probe that wants to accept one can look
// for the byte order mark itself. Empty bytes are not text: there is nothing to
// judge, and a probe must not treat "I saw nothing" as "I saw text".
[[nodiscard]] bool looksLikeText(std::string_view bytes);

} // namespace katana::surveyio
