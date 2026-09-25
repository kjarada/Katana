// Survey jobs on the drawing (cad/survey_job.hpp): the Document's job list,
// and the commands that import a job, re-adjust it and remove it.
//
// The drawing side is importSurveyPoints' - the same layers, codes,
// properties, provenance and existing-point rule as any survey import - so a
// job's points are ordinary survey points and there is one bridge from the
// survey model to the drawing, not two. What this file adds is the job: the
// raw bytes, the settings and the report kept beside the points, and the
// record of which entity holds which point at which coordinates
// (SurveyJob::placedPoints), which is how a re-adjustment moves the job's own
// points and nothing else.

#include "katana/cad/survey_job.hpp"

#include <algorithm>
#include <charconv>
#include <chrono>
#include <format>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <system_error>
#include <unordered_map>
#include <unordered_set>
#include <utility>

#include "katana/commands/change_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/storage/project_store.hpp"

namespace katana::cad {

using katana::commands::ChangeSet;
using katana::commands::ChangeSetCommand;
using katana::commands::CommandContext;
using katana::commands::CommandPtr;
using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::storage::ProjectStore;
namespace survey = katana::survey;

// ---- Document ----------------------------------------------------------------------

const katana::storage::SurveyJob* Document::findSurveyJob(std::string_view id) const
{
    for (const katana::storage::SurveyJob& job : surveyJobs_) {
        if (job.id == id) {
            return &job;
        }
    }
    return nullptr;
}

std::vector<SurveyJob>& SurveyJobAccess::jobs(Document& document)
{
    return document.surveyJobs_;
}

void SurveyJobAccess::changed(Document& document)
{
    ++document.surveyJobsGeneration_;
}

const char* toString(HandEditPolicy policy)
{
    switch (policy) {
    case HandEditPolicy::Keep:
        return "keep the edits made by hand";
    case HandEditPolicy::Overwrite:
        return "overwrite the edits made by hand";
    }
    return "keep the edits made by hand";
}

namespace {

// ---- shared ------------------------------------------------------------------------

// A job's commands hold the Document they change (the job list is not in the
// model), so the model they are executed on must be that Document's. A job
// command run on some other model would draw there and list the job here.
Status requireDocumentModel(const Document& document, const CommandContext& context)
{
    if (&context.model != &document.model()) {
        return makeError(ErrorCode::InvalidState,
                         "a survey job command must be run on the drawing it was made for");
    }
    return {};
}

std::string nowUtc()
{
    return std::format("{:%FT%TZ}",
                       std::chrono::floor<std::chrono::seconds>(std::chrono::system_clock::now()));
}

std::string metres(double value)
{
    std::string text = std::format("{:.3f}", value);
    if (text == "-0.000") {
        text.erase(0, 1);
    }
    return text;
}

void warn(survey::ReductionReport& report, std::string text)
{
    report.warnings.push_back(survey::ReportMessage{std::move(text), {}});
}

// The ids the settings hold as control FROM the drawing: those points are the
// drawing's, not the job's, and the job never draws, moves or deletes them.
std::set<std::string, std::less<>> drawingControlIds(const survey::ReductionSettings& settings)
{
    std::set<std::string, std::less<>> ids;
    for (const survey::ControlSelection& selection : settings.control) {
        if (selection.origin == survey::ControlOrigin::Drawing) {
            ids.insert(selection.point.pointId);
        }
    }
    return ids;
}

// What importSurveyPoints is handed: the reduced project's points and nothing
// that refers to them. The observations, stations and features stay out
// because some of their points are left out here (the drawing's control), and
// a project whose observations name a missing point is not a valid one; they
// are the job's raw data, kept in its bytes, not drawing. The unpositioned
// points stay in: the import's warning about them is the person's answer to
// "why was point 205 not drawn".
survey::SurveyProject drawableProject(const survey::SurveyProject& reduced,
                                      const std::set<std::string, std::less<>>& leaveOut)
{
    survey::SurveyProject out;
    out.name = reduced.name;
    out.coordinateSystem = reduced.coordinateSystem;
    out.units = reduced.units;
    out.metadata = reduced.metadata;
    out.source = reduced.source;
    out.points.reserve(reduced.points.size());
    for (const survey::SurveyPoint& point : reduced.points) {
        if (!leaveOut.contains(point.id)) {
            out.points.push_back(point);
        }
    }
    out.unpositionedPoints = reduced.unpositionedPoints;
    return out;
}

// The points importSurveyPoints will create, in the order it creates them: the
// project's order, less the ids it skips. That order is what pairs each
// created entity with its point; checkPlaced() then confirms it.
std::vector<const survey::SurveyPoint*> pointsToBeDrawn(const survey::SurveyProject& drawable,
                                                        const SurveyPointImportReport& report,
                                                        ExistingPointPolicy policy)
{
    std::set<std::string_view> skipped;
    if (policy == ExistingPointPolicy::Skip) {
        skipped.insert(report.existingIds.begin(), report.existingIds.end());
    }
    std::vector<const survey::SurveyPoint*> drawn;
    drawn.reserve(drawable.points.size());
    for (const survey::SurveyPoint& point : drawable.points) {
        if (!skipped.contains(point.id)) {
            drawn.push_back(&point);
        }
    }
    return drawn;
}

// Pairs the created point entities with the points they were made from, and
// checks each pairing by the point number the import wrote on the entity.
// A mismatch would make every later re-adjustment move the wrong points, so
// it is an error, never a guess.
Result<std::vector<SurveyJobPoint>>
placedPoints(const katana::entity::Model& model, const std::vector<EntityId>& created,
             const std::vector<const survey::SurveyPoint*>& drawn,
             std::string_view pointNumberProperty)
{
    std::vector<SurveyJobPoint> placed;
    placed.reserve(drawn.size());
    std::size_t next = 0;
    for (const EntityId id : created) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr ||
            !std::holds_alternative<katana::entity::PointGeometry>(entity->geometry)) {
            continue; // not a point: a layer's entity or a line, not one of these
        }
        if (next == drawn.size()) {
            return makeError(ErrorCode::Internal,
                             "the survey job drew more points than it computed",
                             "entity=" + std::to_string(id));
        }
        const survey::SurveyPoint& point = *drawn[next++];
        if (!pointNumberProperty.empty()) {
            const auto found = entity->properties.find(pointNumberProperty);
            if (found == entity->properties.end() ||
                katana::entity::toString(found->second) != point.id) {
                return makeError(ErrorCode::Internal,
                                 "a survey job's point was drawn out of order",
                                 "point=" + point.id + " entity=" + std::to_string(id));
            }
        }
        placed.push_back(
            SurveyJobPoint{point.id, id, point.northing, point.easting, point.elevation});
    }
    if (next != drawn.size()) {
        return makeError(ErrorCode::Internal, "the survey job drew fewer points than it computed",
                         std::to_string(drawn.size() - next) + " missing");
    }
    return placed;
}

std::size_t indexOfJob(const std::vector<SurveyJob>& jobs, std::string_view id)
{
    const auto found =
        std::find_if(jobs.begin(), jobs.end(), [&](const SurveyJob& job) { return job.id == id; });
    return static_cast<std::size_t>(found - jobs.begin());
}

Error noSuchJob(std::string_view id)
{
    return makeError(ErrorCode::NotFound, "this drawing has no survey job " + std::string(id));
}

// A job number that is never handed out twice: taken from the entity id
// counter, which is saved with the project and only ever rises (entity ids
// are never reused - entity_database.hpp). Consuming one id for it costs
// nothing and means no counter of the job list's own has to be saved, kept
// in step and got right on undo.
std::string newJobId(const Document& document, katana::entity::Model& model)
{
    EntityId number = model.entities.nextId();
    while (document.findSurveyJob("job-" + std::to_string(number)) != nullptr) {
        ++number;
    }
    model.entities.reserveIdsBelow(number + 1);
    return "job-" + std::to_string(number);
}

// ---- The import's drawing options, kept on the job (SurveyJob::importOptions)

// SurveyImportOptions less the layer (the job's own `layer`), as key=value
// lines under a versioned first line - the same shape as the reduction
// settings' text form, for the same reasons: a key a later build adds is
// skipped by this one, and text from a NEWER version is refused rather than
// half read, so a job is never drawn with options the person did not choose.
constexpr std::string_view kImportOptionsHeader = "katana-survey-import-options";
constexpr int kImportOptionsVersion = 1;
constexpr std::string_view kLayerPerCodeKey = "layer-per-code";
constexpr std::string_view kCreateLayersKey = "create-layers";
constexpr std::string_view kCodePropertyKey = "code-property";
constexpr std::string_view kPointNumberPropertyKey = "point-number-property";
constexpr std::string_view kDescriptionPropertyKey = "description-property";
constexpr std::string_view kRecordSourceKey = "record-source";

// A property name is the person's own text: a line break or a '%' in one is
// percent-encoded, so that a line stays one line.
std::string percentEncoded(std::string_view text)
{
    constexpr std::string_view hex = "0123456789ABCDEF";
    std::string out;
    out.reserve(text.size());
    for (const char c : text) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte == 0x7F || c == '%') {
            out += '%';
            out += hex[byte >> 4];
            out += hex[byte & 0x0F];
        } else {
            out += c;
        }
    }
    return out;
}

std::optional<std::string> percentDecoded(std::string_view text)
{
    const auto digit = [](char c) -> int {
        if (c >= '0' && c <= '9') {
            return c - '0';
        }
        if (c >= 'A' && c <= 'F') {
            return c - 'A' + 10;
        }
        if (c >= 'a' && c <= 'f') {
            return c - 'a' + 10;
        }
        return -1;
    };
    std::string out;
    out.reserve(text.size());
    for (std::size_t i = 0; i < text.size(); ++i) {
        if (text[i] != '%') {
            out += text[i];
            continue;
        }
        if (text.size() - i < 3) {
            return std::nullopt; // a '%' needs two hex digits after it
        }
        const int high = digit(text[i + 1]);
        const int low = digit(text[i + 2]);
        if (high < 0 || low < 0) {
            return std::nullopt;
        }
        out += static_cast<char>((high << 4) | low);
        i += 2;
    }
    return out;
}

std::string serialiseImportOptions(const SurveyImportOptions& options)
{
    std::string text;
    auto line = [&text](std::string_view key, std::string_view value) {
        text += key;
        text += '=';
        text += value;
        text += '\n';
    };
    line(kImportOptionsHeader, std::to_string(kImportOptionsVersion));
    line(kLayerPerCodeKey, options.layerPerCode ? "true" : "false");
    line(kCreateLayersKey, options.createLayers ? "true" : "false");
    line(kCodePropertyKey, percentEncoded(options.codeProperty));
    line(kPointNumberPropertyKey, percentEncoded(options.pointNumberProperty));
    line(kDescriptionPropertyKey, percentEncoded(options.descriptionProperty));
    line(kRecordSourceKey, options.recordSource ? "true" : "false");
    return text;
}

// The options the job's points were drawn with. Empty text is the defaults.
// The text comes from a project file, which may have been damaged or edited
// by hand, so every malformed line is an error naming the job and the line.
Result<SurveyImportOptions> parseImportOptions(std::string_view text, std::string_view jobId)
{
    const auto unreadable = [jobId](std::size_t lineNumber, std::string what) {
        return makeError(ErrorCode::ParseFailure,
                         "the drawing options of survey job " + std::string(jobId) +
                             " cannot be read: line " + std::to_string(lineNumber) + ": " +
                             std::move(what));
    };
    SurveyImportOptions options;
    bool sawHeader = false;
    std::size_t lineNumber = 0;
    std::size_t start = 0;
    while (start < text.size()) {
        const std::size_t cut = text.find('\n', start);
        std::string_view line =
            text.substr(start, cut == std::string_view::npos ? std::string_view::npos : cut - start);
        start = cut == std::string_view::npos ? text.size() : cut + 1;
        ++lineNumber;
        if (!line.empty() && line.back() == '\r') {
            line.remove_suffix(1);
        }
        if (line.empty() || line.front() == '#') {
            continue;
        }
        const std::size_t equals = line.find('=');
        if (equals == std::string_view::npos) {
            return unreadable(lineNumber, "expected key=value");
        }
        const std::string_view key = line.substr(0, equals);
        const std::string_view value = line.substr(equals + 1);
        if (!sawHeader) {
            int version = 0;
            const auto [end, error] =
                std::from_chars(value.data(), value.data() + value.size(), version);
            if (key != kImportOptionsHeader || error != std::errc{} ||
                end != value.data() + value.size() || version <= 0) {
                return unreadable(lineNumber, "it must begin with " +
                                                  std::string(kImportOptionsHeader) +
                                                  "=<version>");
            }
            if (version > kImportOptionsVersion) {
                return makeError(ErrorCode::Unsupported,
                                 "survey job " + std::string(jobId) +
                                     " was drawn by a newer Katana (drawing options version " +
                                     std::to_string(version) + "); this one reads up to version " +
                                     std::to_string(kImportOptionsVersion));
            }
            sawHeader = true;
            continue;
        }
        bool* flag = key == kLayerPerCodeKey   ? &options.layerPerCode
                     : key == kCreateLayersKey ? &options.createLayers
                     : key == kRecordSourceKey ? &options.recordSource
                                               : nullptr;
        if (flag != nullptr) {
            if (value != "true" && value != "false") {
                return unreadable(lineNumber, "'" + std::string(key) + "' must be true or false");
            }
            *flag = value == "true";
            continue;
        }
        std::string* name = key == kCodePropertyKey          ? &options.codeProperty
                            : key == kPointNumberPropertyKey ? &options.pointNumberProperty
                            : key == kDescriptionPropertyKey ? &options.descriptionProperty
                                                             : nullptr;
        if (name != nullptr) {
            std::optional<std::string> decoded = percentDecoded(value);
            if (!decoded) {
                return unreadable(lineNumber, "'" + std::string(key) +
                                                  "' has a '%' not followed by two hex digits");
            }
            *name = std::move(*decoded);
            continue;
        }
        // A key a later build added: skipped, as its versioning promises.
    }
    if (!text.empty() && !sawHeader) {
        return unreadable(lineNumber, "it must begin with " + std::string(kImportOptionsHeader) +
                                          "=<version>");
    }
    return options;
}

void renderInto(SurveyJob& job, const survey::ReductionReport& report, std::string createdUtc)
{
    job.reportText = survey::renderText(report);
    job.reportHtml = survey::renderHtml(report);
    job.reportCreatedUtc = std::move(createdUtc);
}

} // namespace

// ---- Import ------------------------------------------------------------------------

struct ImportSurveyJobCommand::State {
    // What validate() worked out, handed to the execute() that follows it
    // rather than worked out twice: the reduction is the expensive part of an
    // import (a network adjustment), and nothing can change the drawing
    // between the two calls.
    struct Plan {
        survey::ReductionOutcome outcome;
        survey::SurveyProject drawable;
        std::vector<const survey::SurveyPoint*> drawn; // into `drawable`
        CommandPtr draw;                               // nullptr: nothing to draw
    };

    Document* document = nullptr;
    SurveyJobImport request;
    ReductionFunction reduce;
    std::optional<Plan> pending;

    // After execute().
    CommandPtr drawCommand;
    SurveyJob job;       // while undone: the job as it was on the list
    std::size_t index = 0;
    std::string jobId;
    std::optional<survey::ReductionReport> report;
    bool onList = false;

    [[nodiscard]] Result<Plan> build(const CommandContext& context) const
    {
        if (auto status = requireDocumentModel(*document, context); !status) {
            return status.error();
        }
        const SurveyImportOptions& options = request.importOptions;
        if (!request.job.layer.empty() && request.job.layer != options.layer) {
            return makeError(ErrorCode::InvalidArgument,
                             "the survey job's layer and the layer its points are imported to "
                             "differ",
                             "job layer=" + request.job.layer + " import layer=" + options.layer);
        }
        if (!reduce) {
            return makeError(ErrorCode::InvalidArgument,
                             "no reduction was given for the survey job");
        }
        // A job the project could never save is refused before the
        // reduction and before anything is drawn: accepted, it would make
        // every later save of the whole drawing fail. The reader takes files
        // the database cannot hold (ProjectStore::kMaxSurveyJobBytes).
        // execute() checks again once the report and point lists are added.
        if (auto fits = ProjectStore::checkSurveyJobSize(request.job); !fits) {
            return fits.error();
        }
        auto outcome = reduce(request.raw, request.job.settings, request.context);
        if (!outcome) {
            return outcome.error();
        }
        Plan plan;
        plan.outcome = std::move(*outcome);
        plan.drawable =
            drawableProject(plan.outcome.reduced, drawingControlIds(request.job.settings));
        SurveyPointImportReport drawing;
        auto draw = importSurveyPoints(*document, plan.drawable, options, request.existingPoints,
                                       &drawing);
        if (!draw) {
            return draw.error();
        }
        plan.draw = std::move(*draw);
        if (plan.draw != nullptr) {
            if (auto status = plan.draw->validate(context); !status) {
                return status.error();
            }
            plan.drawn = pointsToBeDrawn(plan.drawable, drawing, request.existingPoints);
        }
        // What happened on the drawing belongs in the report of what happened
        // to the data: a point skipped because the drawing had it is as much
        // a result of this import as a rejected observation.
        for (std::string& warning : drawing.warnings) {
            warn(plan.outcome.report, std::move(warning));
        }
        return plan;
    }
};

ImportSurveyJobCommand::ImportSurveyJobCommand(Document& document, SurveyJobImport request,
                                               ReductionFunction reduce)
    : state_(std::make_unique<State>())
{
    state_->document = &document;
    state_->request = std::move(request);
    state_->reduce = std::move(reduce);
}

ImportSurveyJobCommand::~ImportSurveyJobCommand() = default;

Status ImportSurveyJobCommand::validate(const CommandContext& context) const
{
    auto plan = state_->build(context);
    if (!plan) {
        return plan.error();
    }
    state_->pending = std::move(*plan);
    return {};
}

Status ImportSurveyJobCommand::execute(CommandContext& context)
{
    State& s = *state_;
    if (!s.pending) {
        auto plan = s.build(context);
        if (!plan) {
            return plan.error();
        }
        s.pending = std::move(*plan);
    }
    State::Plan plan = std::move(*s.pending);
    s.pending.reset();

    std::string id = newJobId(*s.document, context.model);
    std::vector<EntityId> created;
    std::vector<SurveyJobPoint> placed;
    if (plan.draw != nullptr) {
        if (auto status = plan.draw->execute(context); !status) {
            return status;
        }
        created = plan.draw->createdEntities();
        auto pairs = placedPoints(context.model, created, plan.drawn,
                                  s.request.importOptions.pointNumberProperty);
        if (!pairs) {
            (void)plan.draw->undo(context); // all or nothing
            return pairs.error();
        }
        placed = std::move(*pairs);
    }

    // Nothing below can fail, so the request's job - its raw bytes may be
    // tens of megabytes - is MOVED onto the list rather than copied, and
    // what only the reduction needed is let go: this command lives on in the
    // undo history, and a copy of the raw project and of every drawing point
    // there would be dead weight.
    SurveyJob job = std::move(s.request.job);
    job.layer = s.request.importOptions.layer;
    job.createdEntities = std::move(created);
    job.placedPoints = std::move(placed);
    job.importOptions = serialiseImportOptions(s.request.importOptions);

    std::string stamp =
        s.request.context.createdUtc.empty() ? nowUtc() : s.request.context.createdUtc;
    if (job.importedUtc.empty()) {
        job.importedUtc = stamp;
    }
    renderInto(job, plan.outcome.report, std::move(stamp));
    // The whole job now, report and point lists included: validate() saw
    // only the file. Checked before the id is given, so the sentence names
    // the file the person chose, not an id they never saw.
    if (auto fits = ProjectStore::checkSurveyJobSize(job); !fits) {
        if (plan.draw != nullptr) {
            (void)plan.draw->undo(context); // all or nothing
        }
        s.request.job = std::move(job); // the bytes back where they came from
        return fits;
    }
    job.id = std::move(id);

    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    s.index = jobs.size();
    s.jobId = job.id;
    jobs.push_back(std::move(job));
    s.onList = true;
    s.report = std::move(plan.outcome.report);
    s.drawCommand = std::move(plan.draw);
    s.request.raw = {};
    s.request.context = {};
    SurveyJobAccess::changed(*s.document);
    return {};
}

Status ImportSurveyJobCommand::undo(CommandContext& context)
{
    State& s = *state_;
    if (!s.onList) {
        return makeError(ErrorCode::InvalidState, "the survey job import has not been done");
    }
    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    const std::size_t index = indexOfJob(jobs, s.jobId);
    if (index == jobs.size()) {
        return noSuchJob(s.jobId);
    }
    if (s.drawCommand != nullptr) {
        if (auto status = s.drawCommand->undo(context); !status) {
            return status;
        }
    }
    s.index = index;
    s.job = std::move(jobs[index]);
    jobs.erase(jobs.begin() + static_cast<std::ptrdiff_t>(index));
    s.onList = false;
    SurveyJobAccess::changed(*s.document);
    return {};
}

Status ImportSurveyJobCommand::redo(CommandContext& context)
{
    State& s = *state_;
    if (s.onList || s.jobId.empty()) {
        return makeError(ErrorCode::InvalidState, "the survey job import has not been undone");
    }
    if (s.drawCommand != nullptr) {
        if (auto status = s.drawCommand->redo(context); !status) {
            return status;
        }
    }
    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    const std::size_t index = std::min(s.index, jobs.size());
    jobs.insert(jobs.begin() + static_cast<std::ptrdiff_t>(index), std::move(s.job));
    s.job = {};
    s.onList = true;
    SurveyJobAccess::changed(*s.document);
    return {};
}

std::vector<EntityId> ImportSurveyJobCommand::createdEntities() const
{
    return state_->drawCommand != nullptr ? state_->drawCommand->createdEntities() : std::vector<EntityId>{};
}

const std::string& ImportSurveyJobCommand::jobId() const
{
    return state_->jobId;
}

const survey::ReductionReport* ImportSurveyJobCommand::report() const
{
    return state_->report ? &*state_->report : nullptr;
}

// ---- Re-adjust ---------------------------------------------------------------------

namespace {

// The fields of a job a re-adjustment replaces. Swapped in and out rather
// than keeping a copy of the whole job for undo, which would copy the raw
// field file - tens of megabytes - into every re-adjustment's undo step.
struct JobRevision {
    survey::ReductionSettings settings;
    std::string parserVersion;
    std::vector<EntityId> createdEntities;
    std::vector<SurveyJobPoint> placedPoints;
    std::string reportText;
    std::string reportHtml;
    std::string reportCreatedUtc;

    void swapWith(SurveyJob& job)
    {
        std::swap(settings, job.settings);
        std::swap(parserVersion, job.parserVersion);
        std::swap(createdEntities, job.createdEntities);
        std::swap(placedPoints, job.placedPoints);
        std::swap(reportText, job.reportText);
        std::swap(reportHtml, job.reportHtml);
        std::swap(reportCreatedUtc, job.reportCreatedUtc);
    }
};

// How a point the job placed stands now.
enum class PlacedState { AsPlaced, EditedByHand, DeletedByHand };

struct PlacedNow {
    PlacedState state = PlacedState::DeletedByHand;
    const Entity* entity = nullptr;
    katana::geometry::Point2 position{};
    std::optional<double> elevation{};
};

PlacedNow stateOf(const katana::entity::Model& model, const SurveyJobPoint& placed)
{
    PlacedNow now;
    now.entity = model.entities.find(placed.entity);
    const auto* point = now.entity == nullptr
                            ? nullptr
                            : std::get_if<katana::entity::PointGeometry>(&now.entity->geometry);
    if (point == nullptr) {
        now.entity = nullptr;
        return now;
    }
    now.position = point->position;
    now.elevation = katana::entity::heightsOf(now.entity->properties, 1).front();
    // Exact comparison on purpose: the job wrote these very doubles and the
    // project stores them bit for bit, so ANY difference is an edit - there
    // is no tolerance below which moving a mark by hand does not count.
    const bool moved = now.position.x != placed.easting || now.position.y != placed.northing;
    const bool relevelled = now.elevation != placed.elevation;
    now.state = moved || relevelled ? PlacedState::EditedByHand : PlacedState::AsPlaced;
    return now;
}

// The sentence's tail saying what the new run made of a point, for the report.
std::string newRunPuts(const survey::SurveyPoint* point)
{
    if (point == nullptr) {
        return " The new run does not compute it.";
    }
    return " The new run puts it at N " + metres(point->northing) + ", E " +
           metres(point->easting) + (point->elevation ? ", H " + metres(*point->elevation) : "") +
           ".";
}

} // namespace

struct ReadjustSurveyJobCommand::State {
    struct Plan {
        survey::ReductionReport report;
        survey::SurveyProject fresh;                  // the new points only
        std::vector<const survey::SurveyPoint*> drawn; // into `fresh`
        std::string pointNumberProperty;              // what the new points are numbered by
        CommandPtr entities;                          // nullptr: the drawing is unchanged
        JobRevision revision;                         // placedPoints without the new ones yet
        std::vector<EntityId> deleted;
        SurveyJobChanges changes;
    };

    Document* document = nullptr;
    SurveyJobReadjustment request;
    SurveyJobReader read;
    ReductionFunction reduce;
    std::optional<Plan> pending;

    // After execute().
    CommandPtr entities;
    JobRevision other; // the revision NOT on the job: the old one while applied
    bool applied = false;
    std::optional<survey::ReductionReport> report;
    SurveyJobChanges appliedChanges;

    [[nodiscard]] Result<Plan> build(const CommandContext& context) const;
};

Result<ReadjustSurveyJobCommand::State::Plan>
ReadjustSurveyJobCommand::State::build(const CommandContext& context) const
{
    if (auto status = requireDocumentModel(*document, context); !status) {
        return status.error();
    }
    const SurveyJob* job = document->findSurveyJob(request.jobId);
    if (job == nullptr) {
        return noSuchJob(request.jobId);
    }
    if (!read || !reduce) {
        return makeError(ErrorCode::InvalidArgument,
                         "no reader or no reduction was given for re-adjusting the survey job");
    }
    // A point this run draws for the first time goes where and as the import
    // drew the job's others - a layer per code, the same property names - so
    // the job's points stay together on the drawing. Read before the
    // reduction: options that cannot be read end the run before its costly
    // part.
    auto drawOptions = parseImportOptions(job->importOptions, job->id);
    if (!drawOptions) {
        return drawOptions.error();
    }
    SurveyImportOptions options = std::move(*drawOptions);
    options.layer = job->layer.empty() ? SurveyImportOptions{}.layer : job->layer;
    auto raw = read(*job);
    if (!raw) {
        return raw.error();
    }

    // The previous run is what the job last PLACED, not where the points are
    // now: the report's "shift from the previous run" is about the survey,
    // and a hand edit is reported on its own.
    survey::ReductionContext reductionContext = request.context;
    reductionContext.previous.clear();
    reductionContext.previous.reserve(job->placedPoints.size());
    for (const SurveyJobPoint& placed : job->placedPoints) {
        survey::ComputedPoint previous;
        previous.id = placed.pointId;
        previous.northing = placed.northing;
        previous.easting = placed.easting;
        previous.elevation = placed.elevation;
        reductionContext.previous.push_back(std::move(previous));
    }
    auto outcome = reduce(*raw, request.settings, reductionContext);
    if (!outcome) {
        return outcome.error();
    }
    Plan plan;
    plan.report = std::move(outcome->report);
    const auto leaveOut = drawingControlIds(request.settings);
    std::unordered_map<std::string_view, const survey::SurveyPoint*> computed;
    computed.reserve(outcome->reduced.points.size());
    for (const survey::SurveyPoint& point : outcome->reduced.points) {
        if (!leaveOut.contains(point.id)) {
            computed.emplace(point.id, &point);
        }
    }

    const bool keepEdits = request.handEdits == HandEditPolicy::Keep;
    const katana::entity::Model& model = context.model;
    std::unordered_set<std::string_view> accountedFor; // ids the job's own points answer
    ChangeSet change;
    SurveyJobChanges& changes = plan.changes;
    std::vector<SurveyJobPoint> kept;
    kept.reserve(job->placedPoints.size());
    for (const SurveyJobPoint& placed : job->placedPoints) {
        const auto found = computed.find(placed.pointId);
        const survey::SurveyPoint* now = found == computed.end() ? nullptr : found->second;
        const PlacedNow current = stateOf(model, placed);
        if (current.state == PlacedState::DeletedByHand) {
            changes.deletedByHand.push_back(placed.pointId);
            warn(plan.report, "Point " + placed.pointId +
                                  " was deleted from the drawing after the job placed it; " +
                                  (keepEdits || now == nullptr ? "it stays deleted."
                                                               : "it is drawn again.") +
                                  newRunPuts(now));
            if (keepEdits) {
                kept.push_back(placed);
                accountedFor.insert(placed.pointId);
            }
            // Overwrite: not accounted for, so a point the new run computes is
            // drawn again below like any new one.
            continue;
        }
        if (leaveOut.contains(placed.pointId)) {
            // The new settings hold this point as control FROM THE DRAWING:
            // the run took it where the drawing has it, so there is nothing
            // to move it to - and it must not be deleted as "no longer
            // computed", which would take away the mark the adjustment was
            // just held to (and the next run would find no such point). It
            // stays the job's, recorded where the job put it, so a hand edit
            // remains visible as one to a later run that computes it again.
            if (current.state == PlacedState::EditedByHand) {
                changes.editedByHand.push_back(placed.pointId);
                warn(plan.report, "Point " + placed.pointId +
                                      " was moved or re-levelled by hand after the job placed it; "
                                      "the new run holds it as control from the drawing, where it "
                                      "now stands, so it is left there.");
            }
            kept.push_back(placed);
            accountedFor.insert(placed.pointId);
            continue;
        }
        if (current.state == PlacedState::EditedByHand) {
            changes.editedByHand.push_back(placed.pointId);
            warn(plan.report, "Point " + placed.pointId +
                                  " was moved or re-levelled by hand after the job placed it; " +
                                  (keepEdits ? "the edit is kept." : "the edit is overwritten.") +
                                  newRunPuts(now));
            if (keepEdits) {
                kept.push_back(placed);
                accountedFor.insert(placed.pointId);
                continue;
            }
        }
        accountedFor.insert(placed.pointId);
        if (now == nullptr) {
            change.remove.push_back(placed.entity);
            plan.deleted.push_back(placed.entity);
            changes.removed.push_back(placed.pointId);
            continue;
        }
        SurveyJobPoint moved{placed.pointId, placed.entity, now->northing, now->easting,
                             now->elevation};
        const bool same = current.position.x == moved.easting &&
                          current.position.y == moved.northing &&
                          current.elevation == moved.elevation;
        if (!same) {
            Entity updated = *current.entity;
            updated.geometry =
                katana::entity::PointGeometry{katana::geometry::Point2(now->easting, now->northing)};
            katana::entity::setHeights(updated.properties, {now->elevation});
            if (now->coordinateSource != survey::CoordinateSource::Unknown) {
                updated.properties.insert_or_assign(
                    std::string(kCoordinateSourceProperty),
                    katana::entity::PropertyValue(std::string(toString(now->coordinateSource))));
            }
            change.modify.push_back(std::move(updated));
            changes.moved.push_back(placed.pointId);
        }
        kept.push_back(std::move(moved));
    }
    if (!changes.removed.empty()) {
        warn(plan.report, std::to_string(changes.removed.size()) +
                              " point(s) the job placed are not computed by the new run and are "
                              "deleted from the drawing.");
    }

    // Points the job has not drawn before: drawn as the import drew the job's
    // others (`options`, from the job), and never over
    // a survey point the job did not create (Skip) - that point is someone
    // else's, and the report says it was left alone.
    // Built from the unaccounted points only, not copied whole and pruned:
    // on a re-adjustment that merely moves points this is nearly empty.
    plan.fresh.name = outcome->reduced.name;
    plan.fresh.coordinateSystem = outcome->reduced.coordinateSystem;
    plan.fresh.units = outcome->reduced.units;
    plan.fresh.source = outcome->reduced.source;
    for (const survey::SurveyPoint& point : outcome->reduced.points) {
        if (!accountedFor.contains(point.id) && !leaveOut.contains(point.id)) {
            plan.fresh.points.push_back(point);
        }
    }
    plan.pointNumberProperty = options.pointNumberProperty;
    SurveyPointImportReport drawing;
    CommandPtr draw;
    if (!plan.fresh.points.empty()) {
        auto built =
            importSurveyPoints(*document, plan.fresh, options, ExistingPointPolicy::Skip, &drawing);
        if (!built) {
            return built.error();
        }
        draw = std::move(*built);
        plan.drawn = pointsToBeDrawn(plan.fresh, drawing, ExistingPointPolicy::Skip);
        for (const survey::SurveyPoint* point : plan.drawn) {
            changes.created.push_back(point->id);
        }
        changes.notDrawn = drawing.existingIds;
        if (!changes.notDrawn.empty()) {
            warn(plan.report,
                 std::to_string(changes.notDrawn.size()) +
                     " point(s) the new run computes are not drawn, because the drawing already "
                     "has a survey point of that id that this job did not create.");
        }
    }

    CommandPtr move;
    if (!change.empty()) {
        move = std::make_unique<ChangeSetCommand>(
            "MOVE_SURVEY_JOB_POINTS",
            [change = std::move(change)](const CommandContext&) -> Result<ChangeSet> {
                return change;
            },
            !plan.deleted.empty());
    }
    // A Transaction only when there are two parts: a ChangeSetCommand keeps
    // the set its validate() built for its execute(), where a Transaction
    // validates its first part again on execute - for a re-adjustment that
    // moves every point of a large job, that is a second copy of all of them.
    if (move != nullptr && draw != nullptr) {
        auto transaction = std::make_unique<katana::commands::Transaction>("READJUST_SURVEY_JOB");
        transaction->add(std::move(move));
        transaction->add(std::move(draw));
        plan.entities = std::move(transaction);
    } else {
        plan.entities = move != nullptr ? std::move(move) : std::move(draw);
    }
    if (plan.entities != nullptr) {
        if (auto status = plan.entities->validate(context); !status) {
            return status.error();
        }
    }

    plan.revision.settings = request.settings;
    plan.revision.parserVersion =
        request.parserVersion.empty() ? job->parserVersion : request.parserVersion;
    plan.revision.createdEntities = job->createdEntities;
    if (!plan.deleted.empty()) {
        const std::unordered_set<EntityId> deleted(plan.deleted.begin(), plan.deleted.end());
        std::erase_if(plan.revision.createdEntities,
                      [&](EntityId id) { return deleted.contains(id); });
    }
    plan.revision.placedPoints = std::move(kept);
    return plan;
}

ReadjustSurveyJobCommand::ReadjustSurveyJobCommand(Document& document,
                                                   SurveyJobReadjustment request,
                                                   SurveyJobReader read, ReductionFunction reduce)
    : state_(std::make_unique<State>())
{
    state_->document = &document;
    state_->request = std::move(request);
    state_->read = std::move(read);
    state_->reduce = std::move(reduce);
}

ReadjustSurveyJobCommand::~ReadjustSurveyJobCommand() = default;

Status ReadjustSurveyJobCommand::validate(const CommandContext& context) const
{
    auto plan = state_->build(context);
    if (!plan) {
        return plan.error();
    }
    state_->pending = std::move(*plan);
    return {};
}

Status ReadjustSurveyJobCommand::execute(CommandContext& context)
{
    State& s = *state_;
    if (!s.pending) {
        auto plan = s.build(context);
        if (!plan) {
            return plan.error();
        }
        s.pending = std::move(*plan);
    }
    State::Plan plan = std::move(*s.pending);
    s.pending.reset();

    if (plan.entities != nullptr) {
        if (auto status = plan.entities->execute(context); !status) {
            return status;
        }
        // The new points follow the moved and deleted ones in the command's
        // created list; only the drawing part creates anything.
        const std::vector<EntityId> created = plan.entities->createdEntities();
        auto placed =
            placedPoints(context.model, created, plan.drawn, plan.pointNumberProperty);
        if (!placed) {
            (void)plan.entities->undo(context); // all or nothing
            return placed.error();
        }
        plan.revision.createdEntities.insert(plan.revision.createdEntities.end(), created.begin(),
                                             created.end());
        plan.revision.placedPoints.insert(plan.revision.placedPoints.end(), placed->begin(),
                                          placed->end());
    }
    SurveyJob* job = nullptr;
    for (SurveyJob& candidate : SurveyJobAccess::jobs(*s.document)) {
        if (candidate.id == s.request.jobId) {
            job = &candidate;
        }
    }
    if (job == nullptr) { // cannot happen: build() found it and nothing ran since
        if (plan.entities != nullptr) {
            (void)plan.entities->undo(context);
        }
        return noSuchJob(s.request.jobId);
    }
    plan.revision.reportText = survey::renderText(plan.report);
    plan.revision.reportHtml = survey::renderHtml(plan.report);
    plan.revision.reportCreatedUtc =
        s.request.context.createdUtc.empty() ? nowUtc() : s.request.context.createdUtc;
    plan.revision.swapWith(*job);
    // A longer report and more points may take a job the project could just
    // hold past what it can: refused, as the import would be, rather than
    // left to fail every later save.
    if (auto fits = ProjectStore::checkSurveyJobSize(*job); !fits) {
        plan.revision.swapWith(*job); // back as it was
        if (plan.entities != nullptr) {
            (void)plan.entities->undo(context);
        }
        return fits;
    }
    s.other = std::move(plan.revision); // now the old revision
    s.entities = std::move(plan.entities);
    s.report = std::move(plan.report);
    s.appliedChanges = std::move(plan.changes);
    s.applied = true;
    s.request.context = {}; // the drawing's points, needed by the reduction only
    s.read = {};
    SurveyJobAccess::changed(*s.document);
    return {};
}

Status ReadjustSurveyJobCommand::undo(CommandContext& context)
{
    State& s = *state_;
    if (!s.applied) {
        return makeError(ErrorCode::InvalidState, "the re-adjustment has not been done");
    }
    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    const std::size_t index = indexOfJob(jobs, s.request.jobId);
    if (index == jobs.size()) {
        return noSuchJob(s.request.jobId);
    }
    if (s.entities != nullptr) {
        if (auto status = s.entities->undo(context); !status) {
            return status;
        }
    }
    s.other.swapWith(jobs[index]);
    s.applied = false;
    SurveyJobAccess::changed(*s.document);
    return {};
}

Status ReadjustSurveyJobCommand::redo(CommandContext& context)
{
    State& s = *state_;
    if (s.applied) {
        return makeError(ErrorCode::InvalidState, "the re-adjustment has not been undone");
    }
    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    const std::size_t index = indexOfJob(jobs, s.request.jobId);
    if (index == jobs.size()) {
        return noSuchJob(s.request.jobId);
    }
    if (s.entities != nullptr) {
        if (auto status = s.entities->redo(context); !status) {
            return status;
        }
    }
    s.other.swapWith(jobs[index]);
    s.applied = true;
    SurveyJobAccess::changed(*s.document);
    return {};
}

std::vector<EntityId> ReadjustSurveyJobCommand::createdEntities() const
{
    return state_->entities != nullptr ? state_->entities->createdEntities()
                                       : std::vector<EntityId>{};
}

const survey::ReductionReport* ReadjustSurveyJobCommand::report() const
{
    return state_->report ? &*state_->report : nullptr;
}

const SurveyJobChanges& ReadjustSurveyJobCommand::changes() const
{
    return state_->appliedChanges;
}

// ---- Remove ------------------------------------------------------------------------

struct RemoveSurveyJobCommand::State {
    Document* document = nullptr;
    std::string jobId;
    bool withPoints = false;

    CommandPtr entities; // nullptr: no points to delete
    SurveyJob job;       // while removed
    std::size_t index = 0;
    bool removed = false;

    [[nodiscard]] Result<CommandPtr> build(const CommandContext& context) const
    {
        if (auto status = requireDocumentModel(*document, context); !status) {
            return status.error();
        }
        const SurveyJob* found = document->findSurveyJob(jobId);
        if (found == nullptr) {
            return noSuchJob(jobId);
        }
        if (!withPoints) {
            return CommandPtr{};
        }
        ChangeSet change;
        for (const EntityId id : found->createdEntities) {
            if (context.model.entities.contains(id)) { // one the person deleted is gone already
                change.remove.push_back(id);
            }
        }
        if (change.empty()) {
            return CommandPtr{};
        }
        CommandPtr command = std::make_unique<ChangeSetCommand>(
            "DELETE_SURVEY_JOB_POINTS",
            [change = std::move(change)](const CommandContext&) -> Result<ChangeSet> {
                return change;
            },
            true);
        if (auto status = command->validate(context); !status) {
            return status.error();
        }
        return command;
    }
};

RemoveSurveyJobCommand::RemoveSurveyJobCommand(Document& document, std::string jobId,
                                               bool withPoints)
    : state_(std::make_unique<State>())
{
    state_->document = &document;
    state_->jobId = std::move(jobId);
    state_->withPoints = withPoints;
}

RemoveSurveyJobCommand::~RemoveSurveyJobCommand() = default;

Status RemoveSurveyJobCommand::validate(const CommandContext& context) const
{
    auto built = state_->build(context);
    if (!built) {
        return built.error();
    }
    return {};
}

Status RemoveSurveyJobCommand::execute(CommandContext& context)
{
    State& s = *state_;
    auto built = s.build(context);
    if (!built) {
        return built.error();
    }
    s.entities = std::move(*built);
    if (s.entities != nullptr) {
        if (auto status = s.entities->execute(context); !status) {
            return status;
        }
    }
    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    s.index = indexOfJob(jobs, s.jobId);
    s.job = std::move(jobs[s.index]);
    jobs.erase(jobs.begin() + static_cast<std::ptrdiff_t>(s.index));
    s.removed = true;
    SurveyJobAccess::changed(*s.document);
    return {};
}

Status RemoveSurveyJobCommand::undo(CommandContext& context)
{
    State& s = *state_;
    if (!s.removed) {
        return makeError(ErrorCode::InvalidState, "the survey job has not been removed");
    }
    if (s.entities != nullptr) {
        if (auto status = s.entities->undo(context); !status) {
            return status;
        }
    }
    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    const std::size_t index = std::min(s.index, jobs.size());
    jobs.insert(jobs.begin() + static_cast<std::ptrdiff_t>(index), std::move(s.job));
    s.job = {};
    s.removed = false;
    SurveyJobAccess::changed(*s.document);
    return {};
}

Status RemoveSurveyJobCommand::redo(CommandContext& context)
{
    State& s = *state_;
    if (s.removed) {
        return makeError(ErrorCode::InvalidState, "the survey job removal has not been undone");
    }
    std::vector<SurveyJob>& jobs = SurveyJobAccess::jobs(*s.document);
    const std::size_t index = indexOfJob(jobs, s.jobId);
    if (index == jobs.size()) {
        return noSuchJob(s.jobId);
    }
    if (s.entities != nullptr) {
        if (auto status = s.entities->redo(context); !status) {
            return status;
        }
    }
    s.index = index;
    s.job = std::move(jobs[index]);
    jobs.erase(jobs.begin() + static_cast<std::ptrdiff_t>(index));
    s.removed = true;
    SurveyJobAccess::changed(*s.document);
    return {};
}

} // namespace katana::cad
