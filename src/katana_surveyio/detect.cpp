#include "katana/surveyio/detect.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <fstream>
#include <ios>
#include <stdexcept>
#include <system_error>
#include <utility>

#include "katana/survey/data_model.hpp"

namespace katana::surveyio {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// ASCII only, deliberately. std::tolower takes an int and is undefined for a
// negative char, and it answers according to the C locale, which would make a
// format id or an extension mean different things on different machines.
constexpr char lowered(char c)
{
    return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

std::string twoDecimals(double value)
{
    char text[32];
    const int written = std::snprintf(text, sizeof(text), "%.2f", value);
    return written > 0 ? std::string(text, static_cast<std::size_t>(written)) : std::string("?");
}

// Empty when the id is usable.
//
// The id is written into saved settings and into every survey::SourceRecord an
// import produces, and is compared byte for byte. Restricting it to a lower case
// slug means two formats can never differ only by case or by some character a
// person cannot see in a diff.
std::string idProblem(std::string_view id)
{
    if (id.empty()) {
        return "a format descriptor has an empty id";
    }
    if (id.front() < 'a' || id.front() > 'z') {
        return "format id '" + std::string(id) + "' must start with a lower case letter";
    }
    for (const char c : id) {
        const bool allowed = (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-';
        if (!allowed) {
            return "format id '" + std::string(id) +
                   "' may contain only lower case letters, digits and '-'";
        }
    }
    return {};
}

struct Ranking {
    std::vector<FormatCandidate> candidates; // best first
    std::string notes;                       // probes that misbehaved, if any
};

Ranking rank(std::vector<FormatRegistry::ProbeResult> results)
{
    Ranking ranking;
    for (FormatRegistry::ProbeResult& result : results) {
        const double confidence = result.signature.confidence;
        if (!std::isfinite(confidence) || confidence < 0.0 || confidence > 1.0) {
            // Not clamped. A clamp would turn a broken probe into a confident
            // one and the wrong parser would run; saying so is the only honest
            // handling (PLAN.MD section 36).
            ranking.notes += ranking.notes.empty() ? "" : "; ";
            ranking.notes += "format '" + result.formatId + "' reported a confidence of " +
                             twoDecimals(confidence) + ", which is outside [0, 1]; it was ignored";
            continue;
        }
        if (confidence <= 0.0) {
            continue; // the probe ruled its format out
        }
        ranking.candidates.push_back(
            FormatCandidate{result.formatId, confidence, std::move(result.signature.evidence)});
    }
    // STABLE, and on confidence alone: probeAll walks the registry, which is
    // ordered by id, so equal confidences keep that order and the ranking never
    // depends on which parser registered first.
    std::stable_sort(ranking.candidates.begin(), ranking.candidates.end(),
                     [](const FormatCandidate& a, const FormatCandidate& b) {
                         return a.confidence > b.confidence;
                     });
    return ranking;
}

std::string withNotes(std::string summary, const std::string& notes)
{
    if (notes.empty()) {
        return summary;
    }
    return summary + " (" + notes + ")";
}

} // namespace

// ---- FormatDescriptor ------------------------------------------------------------

std::string describeFormat(const FormatDescriptor& descriptor)
{
    std::string text = descriptor.humanName.empty() ? descriptor.id : descriptor.humanName;
    text += ", import: ";
    text += descriptor.canImport ? "yes" : "no";
    text += ", export: ";
    text += descriptor.canExport ? "yes" : "no";
    text += ", parser: ";
    text += descriptor.parserVersion.empty() ? "unknown" : descriptor.parserVersion;
    return text;
}

// ---- FormatRegistry --------------------------------------------------------------

Status FormatRegistry::add(FormatDescriptor descriptor, FormatProbe probe)
{
    if (std::string reason = idProblem(descriptor.id); !reason.empty()) {
        return makeError(ErrorCode::InvalidArgument, std::move(reason), descriptor.humanName);
    }
    if (probe == nullptr) {
        return makeError(ErrorCode::InvalidArgument,
                         "format '" + descriptor.id + "' has no detection probe");
    }
    if (descriptor.humanName.empty()) {
        // What the application shows in a file dialog and in an import report.
        return makeError(ErrorCode::InvalidArgument,
                         "format '" + descriptor.id + "' has no human readable name");
    }
    if (descriptor.parserVersion.empty()) {
        // Without it an import cannot say which version of this code produced a
        // value, which is the whole point of survey::SourceRecord::formatVersion.
        return makeError(ErrorCode::InvalidArgument,
                         "format '" + descriptor.id + "' has no parser version");
    }
    if (!descriptor.canImport && !descriptor.canExport) {
        return makeError(ErrorCode::InvalidArgument, "format '" + descriptor.id +
                                                         "' can neither import nor export");
    }
    for (const std::string& extension : descriptor.extensions) {
        const bool usable =
            !extension.empty() && extension.front() != '.' &&
            std::all_of(extension.begin(), extension.end(),
                        [](char c) { return c == lowered(c); });
        if (!usable) {
            return makeError(ErrorCode::InvalidArgument,
                             "format '" + descriptor.id + "': extension '" + extension +
                                 "' must be lower case and without a leading dot",
                             "ProbeInput::extension is normalised that way");
        }
    }
    if (formats_.contains(descriptor.id)) {
        return makeError(ErrorCode::AlreadyExists,
                         "a survey format with id '" + descriptor.id + "' is already registered",
                         descriptor.humanName);
    }
    // The id is copied before the descriptor is moved from: the order in which a
    // call's arguments are evaluated is unspecified.
    std::string id = descriptor.id;
    formats_.emplace(std::move(id), Entry{std::move(descriptor), probe});
    return {};
}

std::vector<FormatDescriptor> FormatRegistry::formats() const
{
    std::vector<FormatDescriptor> all;
    all.reserve(formats_.size());
    for (const auto& [id, entry] : formats_) {
        all.push_back(entry.descriptor);
    }
    return all;
}

Result<FormatDescriptor> FormatRegistry::find(std::string_view id) const
{
    const auto found = formats_.find(id);
    if (found == formats_.end()) {
        return makeError(ErrorCode::NotFound,
                         "no survey format is registered with id '" + std::string(id) + "'");
    }
    return found->second.descriptor;
}

bool FormatRegistry::contains(std::string_view id) const
{
    return formats_.contains(id);
}

std::vector<FormatRegistry::ProbeResult> FormatRegistry::probeAll(const ProbeInput& input) const
{
    std::vector<ProbeResult> results;
    results.reserve(formats_.size());
    for (const auto& [id, entry] : formats_) {
        results.push_back(ProbeResult{id, entry.probe(input)});
    }
    return results;
}

FormatRegistry& formatRegistry()
{
    static FormatRegistry registry;
    return registry;
}

FormatRegistration::FormatRegistration(FormatDescriptor descriptor, FormatProbe probe)
{
    const std::string id = descriptor.id;
    if (Status status = formatRegistry().add(std::move(descriptor), probe); !status.ok()) {
        throw std::logic_error("surveyio: format '" + id +
                               "' could not be registered: " + status.error().describe());
    }
}

// ---- Detection -------------------------------------------------------------------

const char* toString(DetectionOutcome outcome)
{
    switch (outcome) {
    case DetectionOutcome::Identified:
        return "identified";
    case DetectionOutcome::Ambiguous:
        return "ambiguous";
    case DetectionOutcome::Uncertain:
        return "uncertain";
    case DetectionOutcome::Empty:
        return "empty";
    }
    return "uncertain";
}

Detection Detection::empty()
{
    Detection detection;
    detection.outcome_ = DetectionOutcome::Empty;
    detection.summary_ = "the file is empty";
    return detection;
}

Detection Detection::uncertain(std::vector<FormatCandidate> ranked, std::string summary)
{
    Detection detection;
    detection.outcome_ = DetectionOutcome::Uncertain;
    detection.candidates_ = std::move(ranked);
    detection.summary_ = std::move(summary);
    return detection;
}

Detection Detection::ambiguous(std::vector<FormatCandidate> ranked, std::string summary)
{
    Detection detection;
    detection.outcome_ = DetectionOutcome::Ambiguous;
    detection.candidates_ = std::move(ranked);
    detection.summary_ = std::move(summary);
    return detection;
}

Detection Detection::identified(FormatDescriptor descriptor, std::vector<FormatCandidate> ranked,
                                std::string summary)
{
    Detection detection;
    detection.outcome_ = DetectionOutcome::Identified;
    detection.descriptor_ = std::move(descriptor);
    detection.candidates_ = std::move(ranked);
    detection.summary_ = std::move(summary);
    return detection;
}

Result<FormatDescriptor> Detection::format() const
{
    if (outcome_ == DetectionOutcome::Identified && descriptor_) {
        return *descriptor_;
    }
    std::string context;
    for (const FormatCandidate& candidate : candidates_) {
        context += context.empty() ? "" : ", ";
        context += candidate.formatId + " " + twoDecimals(candidate.confidence);
    }
    return makeError(ErrorCode::NotFound,
                     std::string("the format of this file was not identified (") +
                         toString(outcome_) + "): " + summary_,
                     context);
}

ProbeInput probeOf(std::string_view bytes, std::string_view fileName, bool truncated)
{
    ProbeInput input;
    input.bytes = bytes;
    input.fileName = katana::survey::sourceFileName(fileName);
    input.truncated = truncated;
    // A name that BEGINS with a dot is a hidden file, not an extension.
    const std::size_t dot = input.fileName.find_last_of('.');
    if (dot != std::string::npos && dot > 0 && dot + 1 < input.fileName.size()) {
        input.extension = input.fileName.substr(dot + 1);
        for (char& c : input.extension) {
            c = lowered(c);
        }
    }
    return input;
}

Detection detectFormat(const ProbeInput& input)
{
    return detectFormat(input, formatRegistry());
}

Detection detectFormat(const ProbeInput& input, const FormatRegistry& registry)
{
    // Before the registry is consulted: an empty file is empty whatever is
    // registered, and no probe should have to handle zero bytes.
    if (input.bytes.empty()) {
        return Detection::empty();
    }
    if (registry.empty()) {
        return Detection::uncertain({}, "no survey format is registered");
    }

    Ranking ranking = rank(registry.probeAll(input));
    if (ranking.candidates.empty()) {
        return Detection::uncertain(
            {}, withNotes("no registered format recognises this file", ranking.notes));
    }

    // Every summary is built BEFORE the candidates are moved out of `ranking`:
    // the order in which a call's arguments are evaluated is unspecified, so
    // reading a candidate in one argument and moving the vector in another would
    // be reading from a moved-from vector on some builds and not others.
    const FormatCandidate best = ranking.candidates.front();
    const double runnerUp = ranking.candidates.size() > 1 ? ranking.candidates[1].confidence : 0.0;
    const std::string runnerUpId =
        ranking.candidates.size() > 1 ? ranking.candidates[1].formatId : std::string{};

    if (best.confidence < kIdentifiedConfidence) {
        const std::string summary =
            withNotes("no format is confident enough about this file; the closest is " +
                          best.formatId + " at " + twoDecimals(best.confidence) + " (" +
                          best.evidence + ")",
                      ranking.notes);
        return Detection::uncertain(std::move(ranking.candidates), summary);
    }
    if (best.confidence - runnerUp < kIdentificationMargin) {
        const std::string summary =
            withNotes(best.formatId + " and " + runnerUpId + " both fit this file (" +
                          twoDecimals(best.confidence) + " and " + twoDecimals(runnerUp) +
                          "); choose one",
                      ranking.notes);
        return Detection::ambiguous(std::move(ranking.candidates), summary);
    }

    Result<FormatDescriptor> descriptor = registry.find(best.formatId);
    if (!descriptor) {
        // The registry produced this id a moment ago, so it can only be gone if
        // the registry was changed underneath us. Reported as uncertain rather
        // than asserted away, because an uncertain answer is a safe one.
        const std::string summary =
            "format '" + best.formatId + "' answered the probe but is no longer registered";
        return Detection::uncertain(std::move(ranking.candidates), summary);
    }
    const std::string summary =
        withNotes(descriptor->humanName + " (" + best.formatId + ") at " +
                      twoDecimals(best.confidence) + ": " + best.evidence,
                  ranking.notes);
    return Detection::identified(std::move(descriptor).value(), std::move(ranking.candidates),
                                 summary);
}

Result<Detection> detectFile(const std::filesystem::path& path)
{
    return detectFile(path, formatRegistry());
}

Result<Detection> detectFile(const std::filesystem::path& path, const FormatRegistry& registry)
{
    const std::string name = path.filename().string();
    std::error_code ignored;
    if (!std::filesystem::is_regular_file(path, ignored)) {
        return makeError(ErrorCode::FileImportFailure, "'" + name + "' is not a readable file");
    }
    std::ifstream stream(path, std::ios::binary);
    if (!stream) {
        return makeError(ErrorCode::FileImportFailure, "cannot open '" + name + "'");
    }
    // One byte past the budget, purely to learn whether there is more to come.
    std::string bytes(kProbeBytes + 1, '\0');
    stream.read(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    if (stream.bad()) {
        return makeError(ErrorCode::FileImportFailure, "cannot read '" + name + "'");
    }
    const auto read = static_cast<std::size_t>(stream.gcount());
    const bool truncated = read > kProbeBytes;
    bytes.resize(std::min(read, kProbeBytes));
    return detectFormat(probeOf(bytes, name, truncated), registry);
}

// ---- Helpers for probes ----------------------------------------------------------

std::string_view withoutByteOrderMark(std::string_view bytes)
{
    const auto byteAt = [bytes](std::size_t i) { return static_cast<unsigned char>(bytes[i]); };
    // UTF-32LE first: its mark begins with the UTF-16LE one, so testing the
    // shorter mark first would leave two NULs at the front of every UTF-32 file.
    if (bytes.size() >= 4 && byteAt(0) == 0xFF && byteAt(1) == 0xFE && byteAt(2) == 0x00 &&
        byteAt(3) == 0x00) {
        return bytes.substr(4);
    }
    if (bytes.size() >= 4 && byteAt(0) == 0x00 && byteAt(1) == 0x00 && byteAt(2) == 0xFE &&
        byteAt(3) == 0xFF) {
        return bytes.substr(4);
    }
    if (bytes.size() >= 3 && byteAt(0) == 0xEF && byteAt(1) == 0xBB && byteAt(2) == 0xBF) {
        return bytes.substr(3);
    }
    if (bytes.size() >= 2 && byteAt(0) == 0xFF && byteAt(1) == 0xFE) {
        return bytes.substr(2);
    }
    if (bytes.size() >= 2 && byteAt(0) == 0xFE && byteAt(1) == 0xFF) {
        return bytes.substr(2);
    }
    return bytes;
}

std::vector<std::string_view> probeLines(const ProbeInput& input, std::size_t count)
{
    std::vector<std::string_view> lines;
    if (count == 0) {
        return lines;
    }
    lines.reserve(std::min<std::size_t>(count, 64));
    std::string_view rest = withoutByteOrderMark(input.bytes);
    while (!rest.empty() && lines.size() < count) {
        const std::size_t end = rest.find_first_of("\r\n");
        if (end == std::string_view::npos) {
            // No terminator: a real last line, unless the bytes were cut short,
            // in which case its length says where kProbeBytes fell and nothing
            // about the file.
            if (!input.truncated) {
                lines.push_back(rest);
            }
            break;
        }
        lines.push_back(rest.substr(0, end));
        std::size_t next = end + 1;
        if (rest[end] == '\r' && next < rest.size() && rest[next] == '\n') {
            ++next;
        }
        rest.remove_prefix(next);
    }
    return lines;
}

bool looksLikeText(std::string_view bytes)
{
    if (bytes.empty()) {
        return false; // nothing to judge
    }
    std::size_t control = 0;
    for (const char c : bytes) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte == 0x00) {
            return false;
        }
        if ((byte < 0x20 && byte != '\t' && byte != '\r' && byte != '\n') || byte == 0x7F) {
            ++control;
        }
    }
    // One byte in a hundred. Policy, not a measurement: a form feed or an escape
    // in a header is ordinary in instrument output, a page of them is not text.
    return control * 100 <= bytes.size();
}

} // namespace katana::surveyio
