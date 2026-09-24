#pragma once

// Survey jobs on the drawing: import a field file as a job, and go back to it
// later to change the reduction and adjustment.
//
//   wizard:   surveyio::readSurvey -> raw SurveyProject
//             ImportSurveyJobCommand  (reduce + adjust + draw + keep the job)
//   later:    ReadjustSurveyJobCommand (re-read the stored bytes, reduce and
//             adjust with new settings, MOVE the job's points, new report)
//
// Both are ONE undoable command each, like importSurveyProject: one Ctrl+Z
// takes the drawing and the job list back together.
//
// cad may not see surveyio (tools/check_layering.cmake), so everything that
// needs a reader is INJECTED: the import is handed the project already read,
// and the re-adjustment is handed a SurveyJobReader, which the application
// binds to surveyio::readSurvey over surveyio::siblingsInMemory(job's files).
// The reduction is injected as well - defaulting to survey::reduceAndAdjust -
// so the commands can be tested with a fake that returns a known outcome.

#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_import.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/storage/survey_job.hpp"
#include "katana/survey/reduction.hpp"

namespace katana::cad {

// The job value is storage's (it has to be saved, and storage may not see
// cad); these are the names cad code uses for it.
using SurveyJob = katana::storage::SurveyJob;
using SurveyJobFile = katana::storage::SurveyJobFile;
using SurveyJobPoint = katana::storage::SurveyJobPoint;

// The reduction, as the commands call it.
using ReductionFunction = std::function<katana::core::Result<katana::survey::ReductionOutcome>(
    const katana::survey::SurveyProject& raw, const katana::survey::ReductionSettings& settings,
    const katana::survey::ReductionContext& context)>;

// Re-reads a job's stored bytes (SurveyJob::sourceBytes and siblingFiles)
// into the raw project, with the reader SurveyJob::formatId names. The
// application supplies it; see the top of this file for why cad cannot.
using SurveyJobReader =
    std::function<katana::core::Result<katana::survey::SurveyProject>(const SurveyJob& job)>;

// What the drawing contributes to a reduction (survey::ReductionContext):
// its points, for control the person takes from the drawing, and - from the
// drawing's coordinate system through katana_geodesy - the grid scale
// factor, the geoid separation and the GNSS-to-grid conversions. A function
// the drawing cannot provide (no coordinate system set) is left EMPTY, so the
// reduction refuses the setting that needs it instead of assuming 1.0.
// `context.input` and `context.createdUtc` are the caller's to fill.
[[nodiscard]] katana::core::Result<katana::survey::ReductionContext>
reductionContextFor(const Document& document);

// ---- Import ------------------------------------------------------------------------

struct SurveyJobImport {
    // Everything the job keeps except what the command fills: `id` (assigned,
    // never reused), `createdEntities`, and the report fields. `settings` is
    // what the reduction runs with; `layer` must equal importOptions.layer.
    SurveyJob job;
    // The project as the reader returned it from job.sourceBytes.
    katana::survey::SurveyProject raw;
    katana::survey::ReductionContext context;
    SurveyImportOptions importOptions;
    // What to do with a computed point whose id a survey point in the drawing
    // already has (importSurveyPoints' rule, and its report). A control point
    // the settings take FROM the drawing (ControlOrigin::Drawing) is never
    // drawn again, whatever this says: it is the drawing's point, not the
    // job's.
    ExistingPointPolicy existingPoints = ExistingPointPolicy::Refuse;
};

// Reduces and adjusts `raw`, draws the result (as importSurveyProject does,
// provenance and all), and adds the job to the document with the rendered
// report. Undo removes the points and the job; redo restores both with the
// same entity ids and the same job id.
//
// validate() fails - and nothing is changed - when the reduction fails, with
// the reduction's own error: an import whose adjustment cannot run is not
// half-imported.
class ImportSurveyJobCommand final : public katana::commands::Command {
  public:
    ImportSurveyJobCommand(Document& document, SurveyJobImport request,
                           ReductionFunction reduce = &katana::survey::reduceAndAdjust);
    ~ImportSurveyJobCommand() override;

    [[nodiscard]] std::string_view name() const override { return "IMPORT_SURVEY_JOB"; }
    [[nodiscard]] katana::core::Status
    validate(const katana::commands::CommandContext& context) const override;
    [[nodiscard]] katana::core::Status execute(katana::commands::CommandContext& context) override;
    [[nodiscard]] katana::core::Status undo(katana::commands::CommandContext& context) override;
    [[nodiscard]] katana::core::Status redo(katana::commands::CommandContext& context) override;
    [[nodiscard]] std::vector<katana::entity::EntityId> createdEntities() const override;

    // After a successful execute(): the new job's id and the report of its
    // reduction. Empty / nullptr before.
    [[nodiscard]] const std::string& jobId() const;
    [[nodiscard]] const katana::survey::ReductionReport* report() const;

  private:
    // Behind a pointer so the implementation can change without touching
    // every file that includes this header.
    struct State;
    std::unique_ptr<State> state_;
};

// ---- Re-adjust ---------------------------------------------------------------------

// A job's point the person has changed since the job last placed it - moved,
// re-levelled or deleted - and what re-adjusting does about it.
enum class HandEditPolicy {
    // The edit stands: the point stays where the person put it, and a deleted
    // one stays deleted. The report names each one with the coordinates the
    // new run computed for it.
    Keep,
    // The new run's coordinates replace the edit, and a deleted point is drawn
    // again. The report names each one.
    Overwrite,
};

[[nodiscard]] const char* toString(HandEditPolicy policy);

struct SurveyJobReadjustment {
    std::string jobId;
    katana::survey::ReductionSettings settings; // the new settings
    katana::survey::ReductionContext context;   // `previous` is filled by the command
    HandEditPolicy handEdits = HandEditPolicy::Keep;
    // The version of the reader the SurveyJobReader used, which becomes the
    // job's parserVersion; empty leaves the job's as it was.
    std::string parserVersion{};
};

// What one re-adjustment did to the drawing, by point id, in the order of the
// job's points and then of the new run's.
struct SurveyJobChanges {
    std::vector<std::string> moved;   // given the new run's coordinates
    std::vector<std::string> created; // produced now, not drawn by the job before
    std::vector<std::string> removed; // no longer produced, so deleted
    // Changed by hand since the job last placed them, and deleted by hand.
    // Under HandEditPolicy::Keep these are left as the person left them.
    std::vector<std::string> editedByHand;
    std::vector<std::string> deletedByHand;
    // Produced now, but a survey point the job did not create already has
    // the id: not drawn, because the job never touches another's point.
    std::vector<std::string> notDrawn;

    friend bool operator==(const SurveyJobChanges&, const SurveyJobChanges&) = default;
};

// Re-reads the job's stored bytes, reduces and adjusts them with the new
// settings, and moves the job's existing points to the new coordinates (a
// point the new run no longer produces is deleted, a new one is created on
// the job's layer). The job's settings, parser version and report are
// replaced; the report shows each point's shift from the previous run. Undo
// puts every point, the settings and the report back.
//
// Only the job's own points are touched (SurveyJob::placedPoints). One the
// person has moved, re-levelled or deleted since the job placed it is found
// by comparing the two, named in the report and in changes(), and kept or
// overwritten as SurveyJobReadjustment::handEdits says. A new point whose id
// a point the job did not create already has is not drawn, and said so. A
// point of the job's that the new settings hold as control FROM THE DRAWING
// is left where it stands, under either policy: the run was held to it there.
//
// NotFound for a job the document does not have; the reader's or the
// reduction's own error otherwise, with nothing changed.
class ReadjustSurveyJobCommand final : public katana::commands::Command {
  public:
    ReadjustSurveyJobCommand(Document& document, SurveyJobReadjustment request,
                             SurveyJobReader read,
                             ReductionFunction reduce = &katana::survey::reduceAndAdjust);
    ~ReadjustSurveyJobCommand() override;

    [[nodiscard]] std::string_view name() const override { return "READJUST_SURVEY_JOB"; }
    [[nodiscard]] katana::core::Status
    validate(const katana::commands::CommandContext& context) const override;
    [[nodiscard]] katana::core::Status execute(katana::commands::CommandContext& context) override;
    [[nodiscard]] katana::core::Status undo(katana::commands::CommandContext& context) override;
    [[nodiscard]] katana::core::Status redo(katana::commands::CommandContext& context) override;
    [[nodiscard]] std::vector<katana::entity::EntityId> createdEntities() const override;

    [[nodiscard]] const katana::survey::ReductionReport* report() const;
    // After a successful execute(): what it did to the drawing.
    [[nodiscard]] const SurveyJobChanges& changes() const;

  private:
    struct State;
    std::unique_ptr<State> state_;
};

// ---- Remove ------------------------------------------------------------------------

// Takes a job off the document's list - its raw bytes, settings and report -
// and, when `withPoints`, deletes the entities it created that are still in
// the drawing (whether or not the person has edited them since: they are the
// job's). Without, the points stay as ordinary survey points. Undo puts the
// job back in its place in the list, and the entities with their ids.
// NotFound for a job the document does not have.
class RemoveSurveyJobCommand final : public katana::commands::Command {
  public:
    RemoveSurveyJobCommand(Document& document, std::string jobId, bool withPoints);
    ~RemoveSurveyJobCommand() override;

    [[nodiscard]] std::string_view name() const override { return "REMOVE_SURVEY_JOB"; }
    // The job's raw data goes with it.
    [[nodiscard]] bool isDestructive() const override { return true; }
    [[nodiscard]] katana::core::Status
    validate(const katana::commands::CommandContext& context) const override;
    [[nodiscard]] katana::core::Status execute(katana::commands::CommandContext& context) override;
    [[nodiscard]] katana::core::Status undo(katana::commands::CommandContext& context) override;
    [[nodiscard]] katana::core::Status redo(katana::commands::CommandContext& context) override;

  private:
    struct State;
    std::unique_ptr<State> state_;
};

// ---- The commands' door into the Document -------------------------------------------

// Document keeps its job list closed (there is no public setter, so nothing
// but an undoable command changes it). This is the one class it lets in, and
// only the survey job commands use it.
class SurveyJobAccess {
  public:
    [[nodiscard]] static std::vector<SurveyJob>& jobs(Document& document);
    // Bumps Document::surveyJobsGeneration(); call after every change.
    static void changed(Document& document);
};

} // namespace katana::cad
