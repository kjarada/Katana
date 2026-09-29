#include "survey_verbs.hpp"

#include <chrono>
#include <cstddef>
#include <filesystem>
#include <format>
#include <fstream>
#include <iterator>
#include <memory>
#include <optional>
#include <system_error>
#include <utility>
#include <vector>

#include "import_records.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/core/text.hpp"
#include "katana/survey/reduction_settings.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/reader.hpp"

namespace katana::app {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
namespace survey = katana::survey;
namespace surveyio = katana::surveyio;

constexpr const char* kUsage =
    "SURVEY READ <file> [FORMAT <id>] | SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>]";

// Warnings listed in the reply; the rest are counted. The job's report keeps
// every one of them (Survey > Survey Jobs).
constexpr std::size_t kListedWarnings = 20;

bool sameWord(std::string_view a, std::string_view b)
{
    if (a.size() != b.size()) {
        return false;
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
        if (katana::core::asciiLower(a[i]) != katana::core::asciiLower(b[i])) {
            return false;
        }
    }
    return true;
}

struct SurveyLine {
    bool import = false;
    std::string path;
    std::string format;
    std::optional<std::string> layer;
};

Result<SurveyLine> parse(std::string_view line)
{
    auto tokens = katana::cad::CommandInterpreter::tokenize(line);
    if (!tokens) {
        return tokens.error();
    }
    const std::vector<std::string>& words = *tokens;
    if (words.size() < 3 || !(sameWord(words[1], "READ") || sameWord(words[1], "IMPORT"))) {
        return makeError(ErrorCode::InvalidArgument, std::string("usage: ") + kUsage);
    }
    SurveyLine parsed;
    parsed.import = sameWord(words[1], "IMPORT");
    parsed.path = words[2];
    for (std::size_t i = 3; i < words.size(); i += 2) {
        if (i + 1 >= words.size()) {
            return makeError(ErrorCode::InvalidArgument,
                             words[i] + " needs a value; " + std::string("usage: ") + kUsage);
        }
        if (sameWord(words[i], "FORMAT")) {
            parsed.format = words[i + 1];
        } else if (sameWord(words[i], "LAYER") && parsed.import) {
            parsed.layer = words[i + 1];
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "'" + words[i] + "' is not an option of SURVEY " +
                                 (parsed.import ? "IMPORT" : "READ") + "; usage: " + kUsage);
        }
    }
    return parsed;
}

Result<std::string> readBytes(const std::filesystem::path& path)
{
    std::error_code error;
    const auto size = std::filesystem::file_size(path, error);
    if (error) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be read",
                         pathText(path) + ": " + error.message());
    }
    if (size > surveyio::kMaxSurveyFileBytes) {
        return makeError(ErrorCode::InvalidArgument,
                         pathText(path) + " is larger than a survey file may be (" +
                             std::to_string(surveyio::kMaxSurveyFileBytes) + " bytes)");
    }
    std::ifstream stream(path, std::ios::binary);
    if (!stream) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be opened", pathText(path));
    }
    return std::string(std::istreambuf_iterator<char>(stream), std::istreambuf_iterator<char>());
}

// The format to read with: FORMAT's, or the one detection is certain of.
Result<std::string> formatFor(const SurveyLine& line, std::string_view bytes,
                              const std::string& name)
{
    if (!line.format.empty()) {
        auto known = surveyio::formatRegistry().find(line.format);
        if (!known) {
            return known.error();
        }
        return known->id;
    }
    const bool truncated = bytes.size() > surveyio::kProbeBytes;
    const surveyio::Detection detection = surveyio::detectFormat(surveyio::probeOf(
        truncated ? bytes.substr(0, surveyio::kProbeBytes) : bytes, name, truncated));
    auto format = detection.format();
    if (!format) {
        std::string candidates;
        for (const surveyio::FormatCandidate& candidate : detection.candidates()) {
            candidates += (candidates.empty() ? "" : ", ") + candidate.formatId;
        }
        return makeError(format.error().code,
                         detection.summary() + " - name the format with FORMAT <id>",
                         candidates.empty() ? std::string("no format claims the file")
                                            : "candidates: " + candidates);
    }
    return format->id;
}

std::size_t observationCount(const survey::SurveyProject& project)
{
    std::size_t count = project.observations.size();
    for (const survey::SurveyStation& station : project.stations) {
        count += station.observations.size();
    }
    return count;
}

std::string describeRead(const surveyio::ReadResult& read, const std::string& name)
{
    const survey::SurveyProject& project = read.project;
    std::string reply = "survey file=" + recordText(name) + " format=" + read.formatId +
                        " parser=" + recordText(read.parserVersion) +
                        " read=" + std::to_string(read.recordsRead) +
                        " skipped=" + std::to_string(read.recordsSkipped) +
                        " warnings=" + std::to_string(read.warnings.size());
    reply += "\ncontent setups=" + std::to_string(project.stations.size()) +
             " observations=" + std::to_string(observationCount(project)) +
             " points=" + std::to_string(project.points.size()) +
             " unpositioned=" + std::to_string(project.unpositionedPoints.size()) +
             " features=" + std::to_string(project.features.size()) +
             " control=" + std::to_string(project.controlPoints.size());
    const survey::DeclaredCoordinateSystem& crs = project.coordinateSystem;
    reply += "\ndeclared crs=" + (crs.name.empty() ? std::string("none") : recordText(crs.name)) +
             " epsg=" + (crs.epsgCode > 0 ? std::to_string(crs.epsgCode) : std::string("none"));
    for (std::size_t i = 0; i < read.warnings.size() && i < kListedWarnings; ++i) {
        const surveyio::ReadWarning& warning = read.warnings[i];
        reply += "\nwarning record=" + std::to_string(warning.record) +
                 " text=" + recordText(warning.message);
    }
    if (read.warnings.size() > kListedWarnings) {
        reply += "\nwarnings_more=" + std::to_string(read.warnings.size() - kListedWarnings);
    }
    for (const std::string& missing : read.notCarried) {
        reply += "\nnot_carried text=" + recordText(missing);
    }
    return reply;
}

std::string nowUtc()
{
    const auto now = std::chrono::floor<std::chrono::seconds>(std::chrono::system_clock::now());
    return std::format("{:%FT%TZ}", now);
}

} // namespace

bool isSurveyLine(std::string_view line)
{
    const std::string_view text = katana::core::trimmed(line);
    std::size_t end = 0;
    while (end < text.size() && !katana::core::isAsciiSpace(text[end])) {
        ++end;
    }
    return sameWord(text.substr(0, end), "SURVEY");
}

Result<std::string> runSurveyLine(katana::cad::Document& document, std::string_view line)
{
    auto parsed = parse(line);
    if (!parsed) {
        return parsed.error();
    }
    const std::filesystem::path path = pathFromText(parsed->path);
    auto bytes = readBytes(path);
    if (!bytes) {
        return bytes.error();
    }
    const std::string name = survey::sourceFileName(pathText(path));
    auto format = formatFor(*parsed, *bytes, name);
    if (!format) {
        return format.error();
    }
    surveyio::ReadOptions options;
    // The job's other files - a DBX folder, a RINEX navigation file - by name,
    // from the folder the file is in, as the wizard reads them.
    options.siblings = surveyio::siblingsInFolder(
        path.has_parent_path() ? path.parent_path() : std::filesystem::path("."));
    auto read = surveyio::readSurvey(surveyio::formatRegistry(), *format, *bytes, name, options);
    if (!read) {
        return read.error();
    }
    std::string reply = describeRead(*read, name);
    if (!parsed->import) {
        return reply;
    }

    auto context = katana::cad::reductionContextFor(document);
    if (!context) {
        return context.error();
    }
    context->input = surveyio::reportInputFor(*read, name);
    context->createdUtc = nowUtc();
    katana::cad::SurveyImportOptions importOptions;
    if (parsed->layer) {
        importOptions.layer = *parsed->layer;
    }
    katana::cad::SurveyJobImport request;
    request.job.name = name;
    request.job.formatId = read->formatId;
    request.job.parserVersion = read->parserVersion;
    request.job.sourceFileName = name;
    request.job.sourceBytes = std::move(*bytes);
    for (const surveyio::SiblingFile& sibling : read->siblingsRead) {
        request.job.siblingFiles.push_back({sibling.name, sibling.bytes});
    }
    // The wizard's starting settings: the defaults, holding what the file
    // declares as control.
    request.job.settings.control = survey::controlFromFile(read->project);
    request.job.layer = importOptions.layer;
    request.job.importedUtc = context->createdUtc;
    request.raw = std::move(read->project);
    request.context = std::move(*context);
    request.importOptions = importOptions;
    auto command =
        std::make_unique<katana::cad::ImportSurveyJobCommand>(document, std::move(request));
    const katana::cad::ImportSurveyJobCommand* job = command.get();
    if (const auto status = document.execute(std::move(command)); !status) {
        return status.error();
    }
    const survey::ReductionReport* report = job->report();
    reply += "\nimported job=" + recordText(job->jobId()) +
             " entities=" + std::to_string(document.lastCreatedEntities().size()) +
             " layer=" + recordText(importOptions.layer) + " reduction_warnings=" +
             std::to_string(report != nullptr ? report->warnings.size() : 0);
    // The reduction's own warnings, as the read's are listed: an agent has no
    // Survey Jobs dialog to open the report in.
    if (report != nullptr) {
        for (std::size_t i = 0; i < report->warnings.size() && i < kListedWarnings; ++i) {
            reply += "\nreduction_warning text=" + recordText(report->warnings[i].text);
        }
        if (report->warnings.size() > kListedWarnings) {
            reply += "\nreduction_warnings_more=" +
                     std::to_string(report->warnings.size() - kListedWarnings);
        }
    }
    return reply;
}

const char* surveyHelpText()
{
    return "Survey    SURVEY READ <file> [FORMAT <id>]  what a survey field file holds, as\n"
           "          the reader made it; nothing is changed\n"
           "          SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>]  reduced with the\n"
           "          default settings and drawn as a survey job, one undo step\n"
           "          (replies are key=value records, docs/survey.md)\n";
}

} // namespace katana::app
