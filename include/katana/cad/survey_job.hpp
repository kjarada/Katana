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
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/storage/survey_job.hpp"
#include "katana/survey/reduction.hpp"

namespace katana::cad {

// The job value is storage's (it has to be saved, and storage may not see
// cad); these are the names cad code uses for it.
using SurveyJob = katana::storage::SurveyJob;
using SurveyJobFile = katana::storage::SurveyJobFile;

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

struct SurveyJobReadjustment {
    std::string jobId;
    katana::survey::ReductionSettings settings; // the new settings
    katana::survey::ReductionContext context;   // `previous` is filled by the command
};

// Re-reads the job's stored bytes, reduces and adjusts them with the new
// settings, and moves the job's existing points to the new coordinates (a
// point the new run no longer produces is deleted, a new one is created on
// the job's layer). The job's settings, parser version and report are
// replaced; the report shows each point's shift from the previous run. Undo
// puts every point, the settings and the report back.
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
