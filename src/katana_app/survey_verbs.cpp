#include "survey_verbs.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <filesystem>
#include <format>
#include <fstream>
#include <functional>
#include <iterator>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "import_records.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/survey/reduction_settings.hpp"
#include "katana/survey/report_summary.hpp"
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
    "SURVEY READ <file> [FORMAT <id>] | SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>] "
    "[SETTINGS <file>] [SET <key>=<value> ...]";

// The words that begin an option, and so end SET's items: a key of the
// settings text is never one of them.
constexpr std::string_view kOptions[] = {"FORMAT", "LAYER", "SETTINGS", "SET"};

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

// The option `word` names, as kOptions spells it; empty for any other word.
std::string_view optionNamed(std::string_view word)
{
    for (const std::string_view option : kOptions) {
        if (sameWord(word, option)) {
            return option;
        }
    }
    return {};
}

struct SurveyLine {
    bool import = false;
    std::string path;
    std::string format;
    std::optional<std::string> layer;
    std::optional<std::string> settingsFile;
    std::vector<std::string> set; // SET's items, as given
};

Result<SurveyLine> parse(std::string_view line)
{
    auto tokens = katana::cad::CommandInterpreter::tokenize(line);
    if (!tokens) {
        return tokens.error();
    }
    const std::vector<std::string>& words = *tokens;
    const auto refused = [](const std::string& why) {
        return makeError(ErrorCode::InvalidArgument, why + "; usage: " + kUsage);
    };
    if (words.size() < 3 || !(sameWord(words[1], "READ") || sameWord(words[1], "IMPORT"))) {
        return makeError(ErrorCode::InvalidArgument, std::string("usage: ") + kUsage);
    }
    SurveyLine parsed;
    parsed.import = sameWord(words[1], "IMPORT");
    parsed.path = words[2];
    // Each option once: a second FORMAT or SETTINGS has no meaning the first
    // does not contradict, and taking the last one said nothing of the other.
    std::set<std::string_view> given;
    for (std::size_t i = 3; i < words.size();) {
        const std::string_view option = optionNamed(words[i]);
        if (option.empty() || (!parsed.import && option != "FORMAT")) {
            return refused("'" + words[i] + "' is not an option of SURVEY " +
                           (parsed.import ? "IMPORT" : "READ"));
        }
        if (!given.insert(option).second) {
            return refused(std::string(option) + " is given twice");
        }
        if (option == "SET") {
            // Its items run to the next option word or the end of the line.
            for (++i; i < words.size() && optionNamed(words[i]).empty(); ++i) {
                parsed.set.push_back(words[i]);
            }
            if (parsed.set.empty()) {
                return refused("SET needs at least one <key>=<value>");
            }
            continue;
        }
        if (i + 1 >= words.size()) {
            return refused(words[i] + " needs a value");
        }
        const std::string& value = words[i + 1];
        if (option == "FORMAT") {
            parsed.format = value;
        } else if (option == "LAYER") {
            parsed.layer = value;
        } else {
            parsed.settingsFile = value;
        }
        i += 2;
    }
    return parsed;
}

// `what` is the file's part in the line, for the refusal: "the file", "the
// settings file".
Result<std::string> readBytes(const std::filesystem::path& path, const std::string& what)
{
    std::error_code error;
    const auto size = std::filesystem::file_size(path, error);
    if (error) {
        return makeError(ErrorCode::FileImportFailure, what + " cannot be read",
                         pathText(path) + ": " + error.message());
    }
    // A settings file is held to a survey file's limit too: it names at most
    // every point of one, and nothing larger is text a person meant.
    if (size > surveyio::kMaxSurveyFileBytes) {
        return makeError(ErrorCode::InvalidArgument,
                         pathText(path) + " is larger than a survey file may be (" +
                             std::to_string(surveyio::kMaxSurveyFileBytes) + " bytes)");
    }
    std::ifstream stream(path, std::ios::binary);
    if (!stream) {
        return makeError(ErrorCode::FileImportFailure, what + " cannot be opened", pathText(path));
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

// ---- the reduction settings: SETTINGS and SET --------------------------------------
//
// One grammar, the settings' stable text form (survey/reduction_settings.hpp):
// a SETTINGS file is that text, and each SET item is one line of it, read by
// survey::parseReductionSettings with its own messages. Nothing of the form
// is spelt here - its first line and its keys are taken from what
// serialiseReductionSettings writes, so they cannot fall behind it.

// The text form's first line, its name and version, with the line break.
const std::string& settingsHeader()
{
    static const std::string header = [] {
        const std::string text = survey::serialiseReductionSettings(survey::ReductionSettings{});
        return text.substr(0, text.find('\n') + 1);
    }();
    return header;
}

// Every key of the text form, in its order: the defaults' lines, and one
// control point's for the key a control line has.
const std::string& settingKeys()
{
    static const std::string keys = [] {
        survey::ReductionSettings sample;
        sample.control.push_back(survey::ControlSelection{});
        sample.control.back().point.pointId = "id";
        const std::string text = survey::serialiseReductionSettings(sample);
        std::string list;
        const std::vector<std::string_view> lines = katana::core::splitLines(text);
        for (std::size_t i = 1; i < lines.size(); ++i) {
            const std::string_view line = lines[i];
            list += (list.empty() ? "" : ", ") + std::string(line.substr(0, line.find('=')));
        }
        return list;
    }();
    return keys;
}

// What the line asks of the settings, gathered before the survey file is
// read, so a mistyped key is refused without reading a large file first.
struct SettingsRequest {
    std::optional<survey::ReductionSettings> fromFile; // SETTINGS', when given
    std::string fileName;                              // its name, for the reply
    std::vector<std::string> fileWarnings;             // keys it holds that this version does not
    std::vector<std::string> items;                    // SET's, as given
    std::set<std::string, std::less<>> keys;           // the keys SET's items change, control apart
    std::vector<survey::ControlSelection> control;     // SET's control items, read
};

// One SET item, read as the one line of a settings text after its version
// line, so its value is checked by the same parser and validation a file's
// line is. `request` gathers what it changes.
katana::core::Status readSetItem(const std::string& item, SettingsRequest& request)
{
    const std::string where = "SET " + item;
    if (item.find_first_of("\r\n") != std::string::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "SET item " + std::to_string(request.items.size() + 1) +
                             " holds a line break; each item is one line of the settings");
    }
    const std::size_t equals = item.find('=');
    // The parser passes over a note and a blank line without a word, which
    // for an item typed on purpose would be a setting silently not made.
    if (item.empty() || item.front() == '#' || equals == std::string::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "SET takes <key>=<value> items, and '" + item + "' is not one", where);
    }
    std::vector<std::string> unknown;
    auto one = survey::parseReductionSettings(settingsHeader() + item + "\n", &unknown);
    if (!one) {
        return makeError(one.error().code, one.error().message, where);
    }
    // In a file, a key this version does not know is skipped, since a later
    // Katana may have written it. Typed here and now, it is a mistake.
    if (!unknown.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "'" + item.substr(0, equals) +
                             "' is not a reduction setting; the settings are " + settingKeys(),
                         where);
    }
    if (!one->control.empty()) {
        const std::string& id = one->control.front().point.pointId;
        if (std::ranges::any_of(request.control, [&id](const survey::ControlSelection& held) {
                return held.point.pointId == id;
            })) {
            return makeError(ErrorCode::InvalidArgument,
                             "control point '" + id + "' is listed twice", where);
        }
        request.control.push_back(std::move(one->control.front()));
    } else if (!request.keys.insert(item.substr(0, equals)).second) {
        return makeError(ErrorCode::InvalidArgument,
                         "'" + item.substr(0, equals) + "' is given twice", where);
    }
    request.items.push_back(item);
    return {};
}

Result<SettingsRequest> settingsRequest(const SurveyLine& line)
{
    SettingsRequest request;
    if (line.settingsFile) {
        const std::filesystem::path path = pathFromText(*line.settingsFile);
        auto bytes = readBytes(path, "the settings file");
        if (!bytes) {
            return bytes.error();
        }
        // Decoded as every text file Katana reads is (core/text_encoding.hpp):
        // the byte order mark an editor saves, or its UTF-16, is the same
        // settings, where the parser alone would call the text not settings.
        auto text = katana::core::decodeText(*bytes);
        if (!text) {
            return makeError(text.error().code, text.error().message,
                             "SETTINGS " + pathText(path));
        }
        if (text->guessed) {
            request.fileWarnings.push_back(
                "the settings file is not UTF-8; it was read as " +
                std::string(katana::core::toString(text->encoding)) +
                ", so an accented point id may be wrong");
        }
        auto settings = survey::parseReductionSettings(text->text, &request.fileWarnings);
        if (!settings) {
            return makeError(settings.error().code, settings.error().message,
                             "SETTINGS " + pathText(path));
        }
        request.fromFile = std::move(*settings);
        request.fileName = survey::sourceFileName(pathText(path));
    }
    for (const std::string& item : line.set) {
        if (auto status = readSetItem(item, request); !status) {
            return status.error();
        }
    }
    return request;
}

// The settings the reduction runs with. The start is the wizard's - the
// defaults, holding the control the file declares - or the SETTINGS file's,
// whole: its control lines are all the control there is. SET's items then
// change what they name, a control item holding its point as the wizard's
// Hold does: in place of a point of that id, or after the others. The result
// is read back through the parser, so it is validated as a whole.
Result<survey::ReductionSettings> settingsFor(const SettingsRequest& request,
                                              const survey::SurveyProject& project)
{
    survey::ReductionSettings settings;
    if (request.fromFile) {
        settings = *request.fromFile;
    } else {
        settings.control = survey::controlFromFile(project);
    }
    if (request.items.empty()) {
        return settings;
    }
    for (const survey::ControlSelection& held : request.control) {
        const auto same =
            std::ranges::find(settings.control, held.point.pointId,
                              [](const survey::ControlSelection& s) { return s.point.pointId; });
        if (same != settings.control.end()) {
            *same = held;
        } else {
            settings.control.push_back(held);
        }
    }
    std::string text;
    const std::string start = survey::serialiseReductionSettings(settings);
    for (const std::string_view line : katana::core::splitLines(start)) {
        if (!request.keys.contains(line.substr(0, line.find('=')))) {
            text += line;
            text += '\n';
        }
    }
    for (const std::string& item : request.items) {
        if (request.keys.contains(std::string_view(item).substr(0, item.find('=')))) {
            text += item;
            text += '\n';
        }
    }
    return survey::parseReductionSettings(text);
}

// Where the settings came from and every line of their text form that is not
// the defaults' - in its order and its words, so a line reads back as a SET
// item - then what the SETTINGS file held that this version skipped.
std::string describeSettings(const survey::ReductionSettings& used,
                             const SettingsRequest& request)
{
    const std::string defaults = survey::serialiseReductionSettings(survey::ReductionSettings{});
    const std::vector<std::string_view> defaultLines = katana::core::splitLines(defaults);
    const std::set<std::string_view> unchanged(defaultLines.begin(), defaultLines.end());
    const std::string text = survey::serialiseReductionSettings(used);
    std::string lines;
    std::size_t differ = 0;
    for (const std::string_view line : katana::core::splitLines(text)) {
        if (unchanged.contains(line)) {
            continue;
        }
        const std::size_t equals = line.find('=');
        lines += "\nsetting key=" + recordText(line.substr(0, equals)) +
                 " value=" + recordText(line.substr(equals + 1));
        ++differ;
    }
    std::string reply = "settings file=" + recordText(request.fileName) +
                        " set=" + std::to_string(request.items.size()) +
                        " differ=" + std::to_string(differ) + lines;
    for (const std::string& warning : request.fileWarnings) {
        reply += "\nsettings_warning text=" + recordText(warning);
    }
    return reply;
}

// " northing=5e+06 easting=5e+05 height=100", each number the shortest text
// that reads back exactly, as every record writes one; height=none for a
// point with no height, which is not a height of zero.
std::string placeFields(double northing, double easting, const std::optional<double>& height)
{
    return " northing=" + recordNumber(northing) + " easting=" + recordNumber(easting) +
           " height=" + (height ? recordNumber(*height) : std::string("none"));
}

// Where each point the settings hold is held: at the drawing's survey point
// of its id - the entity too, the first of the id as the reduction takes it
// (cad::reduceForDrawing refuses an id whose points are not one mark) - or at
// the file's. Taken from what the import is handed, before it runs; an id
// neither has is the reduction's to refuse.
std::string heldRecords(const survey::ReductionSettings& settings,
                        const survey::SurveyProject& raw,
                        const std::vector<katana::cad::DrawingSurveyPoint>& drawing)
{
    std::string records;
    for (const survey::ControlSelection& selection : settings.control) {
        const std::string& id = selection.point.pointId;
        if (selection.origin == survey::ControlOrigin::Drawing) {
            const auto point = std::ranges::find(drawing, id, &katana::cad::DrawingSurveyPoint::id);
            if (point != drawing.end()) {
                records += "\nheld id=" + recordText(id) +
                           " from=drawing entity=" + std::to_string(point->entity) +
                           placeFields(point->northing, point->easting, point->elevation);
            }
        } else {
            const auto point = std::ranges::find(raw.points, id, &survey::SurveyPoint::id);
            if (point != raw.points.end()) {
                records += "\nheld id=" + recordText(id) + " from=file" +
                           placeFields(point->northing, point->easting, point->elevation);
            }
        }
    }
    return records;
}

} // namespace

std::string reductionRecords(const survey::ReductionReport& report)
{
    std::string records = "\nreduction method=" + recordText(survey::methodText(report.settings)) +
                          " adjustments=" + std::to_string(report.adjustments.size()) +
                          " rejected=" + std::to_string(survey::rejectedObservations(report));
    for (const survey::AdjustmentReport& adjustment : report.adjustments) {
        const std::optional<survey::ReportGlobalTest>& test = adjustment.globalTest;
        records += "\nadjustment method=" + recordText(adjustment.method) +
                   " observations=" + std::to_string(adjustment.observations) +
                   " unknowns=" + std::to_string(adjustment.unknowns) +
                   " redundancy=" + std::to_string(adjustment.redundancy) + " variance_factor=" +
                   (adjustment.varianceFactor ? recordNumber(*adjustment.varianceFactor)
                                              : std::string("none")) +
                   " global_test=" + (test ? (test->passed ? "passed" : "failed") : "none") +
                   " flagged=" + std::to_string(adjustment.flaggedOutliers.size()) +
                   " rejected=" + std::to_string(adjustment.rejectedOutliers.size());
    }
    return records;
}

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
    std::optional<SettingsRequest> wanted;
    if (parsed->import) {
        auto asked = settingsRequest(*parsed);
        if (!asked) {
            return asked.error();
        }
        wanted = std::move(*asked);
    }
    const std::filesystem::path path = pathFromText(parsed->path);
    auto bytes = readBytes(path, "the file");
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

    auto settings = settingsFor(*wanted, read->project);
    if (!settings) {
        return settings.error();
    }
    // The drawing's points, for the control held from the drawing: the
    // wizard's context, from the one function both take it from.
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
    request.job.settings = *settings;
    request.job.layer = importOptions.layer;
    request.job.importedUtc = context->createdUtc;
    // Before the import draws anything, and before the project is handed on.
    const std::string held = heldRecords(*settings, read->project,
                                         katana::cad::drawingSurveyPoints(document));
    request.raw = std::move(read->project);
    request.context = std::move(*context);
    request.importOptions = importOptions;
    // The command's own reduction, cad::reduceForDrawing, as the wizard's.
    auto command =
        std::make_unique<katana::cad::ImportSurveyJobCommand>(document, std::move(request));
    const katana::cad::ImportSurveyJobCommand* job = command.get();
    if (const auto status = document.execute(std::move(command)); !status) {
        return status.error();
    }
    reply += "\n" + describeSettings(*settings, *wanted);
    const survey::ReductionReport* report = job->report();
    reply += "\nimported job=" + recordText(job->jobId()) +
             " entities=" + std::to_string(document.lastCreatedEntities().size()) +
             " layer=" + recordText(importOptions.layer) + " reduction_warnings=" +
             std::to_string(report != nullptr ? report->warnings.size() : 0);
    reply += held;
    // What the adjustment made of the data, and then the reduction's own
    // warnings, as the read's are listed: an agent has no Survey Jobs dialog
    // to open the report in.
    if (report != nullptr) {
        reply += reductionRecords(*report);
        for (std::size_t i = 0; i < report->warnings.size() && i < kListedWarnings; ++i) {
            reply += "\nreduction_warning text=" + recordText(report->warnings[i].text);
        }
        if (report->warnings.size() > kListedWarnings) {
            reply += "\nreduction_warnings_more=" +
                     std::to_string(report->warnings.size() - kListedWarnings);
        }
        // Each setup the reduction positioned by resection, as the report's
        // Resections table has it: where, from which points, how well (one
        // sigma, metres), its redundancy (plan + heights), how many of its
        // residuals the outlier test flagged, the geometry's dilution and
        // whether that is weak, its checks, how far the file's own
        // coordinates for the station were, which setup had radiated the
        // station and how far from the resection, and a scale factor that was
        // not applied. Then each residual flagged or rejected, one record
        // each, so an agent sees which without the report and past the
        // warnings' cap.
        const auto optionalNumber = [](const std::optional<double>& value) {
            return value ? recordNumber(*value) : std::string("none");
        };
        for (const survey::ResectionReport& resection : report->resections) {
            std::string from;
            for (const std::string& target : resection.targets) {
                from += (from.empty() ? "" : ",") + target;
            }
            const std::size_t flagged =
                resection.horizontal.flaggedOutliers.size() +
                (resection.height ? resection.height->flaggedOutliers.size() : 0);
            const std::optional<double> fileOffset =
                resection.fileNorthingDifference && resection.fileEastingDifference
                    ? std::optional<double>(std::hypot(*resection.fileNorthingDifference,
                                                       *resection.fileEastingDifference))
                    : std::nullopt;
            const std::optional<double> radiationOffset =
                resection.radiatedNorthingDifference && resection.radiatedEastingDifference
                    ? std::optional<double>(std::hypot(*resection.radiatedNorthingDifference,
                                                       *resection.radiatedEastingDifference))
                    : std::nullopt;
            reply += "\nresection setup=" + recordText(resection.stationId) +
                     " point=" + recordText(resection.pointId) + " from=" + recordText(from) +
                     " northing=" + recordNumber(resection.northing) +
                     " easting=" + recordNumber(resection.easting) +
                     " height=" + optionalNumber(resection.elevation) +
                     " sigma_n=" + recordNumber(resection.sigmaNorthing) +
                     " sigma_e=" + recordNumber(resection.sigmaEasting) +
                     " sigma_h=" + optionalNumber(resection.sigmaElevation) +
                     " redundancy=" + std::to_string(resection.horizontal.redundancy) + "+" +
                     (resection.height ? std::to_string(resection.height->redundancy)
                                       : std::string("0")) +
                     " flagged=" + std::to_string(flagged) +
                     " dilution=" + recordNumber(resection.dilution) +
                     " weak=" + (resection.weakGeometry ? "1" : "0") +
                     " checks=" + std::to_string(resection.checks) +
                     " file_offset=" + optionalNumber(fileOffset) + " radiated_from=" +
                     (resection.radiatedFrom.empty() ? std::string("none")
                                                     : recordText(resection.radiatedFrom)) +
                     " radiation_offset=" + optionalNumber(radiationOffset) +
                     " unapplied_scale_factor=" + optionalNumber(resection.unappliedScaleFactor);
            const auto residuals = [&reply, &resection](const survey::AdjustmentReport& adjustment) {
                for (const survey::ReportResidual& residual : adjustment.residuals) {
                    if (!residual.flagged && !residual.rejected) {
                        continue;
                    }
                    reply += "\nresection_residual setup=" + recordText(resection.stationId) +
                             " observation=" + recordText(residual.observation) +
                             " residual=" + recordNumber(residual.residual) +
                             " unit=" + (residual.angular ? "rad" : "m") +
                             " sigma=" + recordNumber(residual.sigma) + " standardised=" +
                             (residual.standardised ? recordNumber(*residual.standardised)
                                                    : std::string("none")) +
                             " state=" + (residual.rejected ? "rejected" : "flagged");
                }
            };
            residuals(resection.horizontal);
            if (resection.height) {
                residuals(*resection.height);
            }
        }
    }
    return reply;
}

const char* surveyHelpText()
{
    return "Survey    SURVEY READ <file> [FORMAT <id>]  what a survey field file holds, as\n"
           "          the reader made it; nothing is changed\n"
           "          SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>] [SETTINGS <file>]\n"
           "          [SET <key>=<value> ...]  reduced and drawn as a survey job, one undo\n"
           "          step: with the defaults and the control the file declares, or a\n"
           "          SETTINGS file's (the settings text a job keeps), each SET item one\n"
           "          line of that text; SET control=CP1;drawing;fixed;0;fixed;0;fixed;0\n"
           "          holds the drawing's point CP1 (refused where the drawing has two\n"
           "          CP1s apart); the reply says where each point was held and what the\n"
           "          adjustment made of the data (key=value records, docs/survey.md)\n";
}

} // namespace katana::app
