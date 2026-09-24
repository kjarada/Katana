// The survey job contract (cad/survey_job.hpp). The Document's side of it is
// real; the commands and reductionContextFor are placeholders the jobs builder
// replaces, and each fails loudly rather than pretending to have done
// something.

#include "katana/cad/survey_job.hpp"

#include <utility>

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

katana::core::Error notYet()
{
    return makeError(ErrorCode::Unsupported,
                     "survey jobs cannot be imported or re-adjusted in this build yet");
}

} // namespace

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

// ---- Placeholders ------------------------------------------------------------------

katana::core::Result<katana::survey::ReductionContext> reductionContextFor(const Document&)
{
    return notYet();
}

struct ImportSurveyJobCommand::State {
    Document* document = nullptr;
    SurveyJobImport request;
    ReductionFunction reduce;
    std::string jobId;
};

ImportSurveyJobCommand::ImportSurveyJobCommand(Document& document, SurveyJobImport request,
                                               ReductionFunction reduce)
    : state_(std::make_unique<State>(State{&document, std::move(request), std::move(reduce), {}}))
{
}

ImportSurveyJobCommand::~ImportSurveyJobCommand() = default;

Status ImportSurveyJobCommand::validate(const katana::commands::CommandContext&) const
{
    return notYet();
}

Status ImportSurveyJobCommand::execute(katana::commands::CommandContext&)
{
    return notYet();
}

Status ImportSurveyJobCommand::undo(katana::commands::CommandContext&)
{
    return notYet();
}

Status ImportSurveyJobCommand::redo(katana::commands::CommandContext&)
{
    return notYet();
}

std::vector<katana::entity::EntityId> ImportSurveyJobCommand::createdEntities() const
{
    return {};
}

const std::string& ImportSurveyJobCommand::jobId() const
{
    return state_->jobId;
}

const katana::survey::ReductionReport* ImportSurveyJobCommand::report() const
{
    return nullptr;
}

struct ReadjustSurveyJobCommand::State {
    Document* document = nullptr;
    SurveyJobReadjustment request;
    SurveyJobReader read;
    ReductionFunction reduce;
};

ReadjustSurveyJobCommand::ReadjustSurveyJobCommand(Document& document,
                                                   SurveyJobReadjustment request,
                                                   SurveyJobReader read, ReductionFunction reduce)
    : state_(std::make_unique<State>(
          State{&document, std::move(request), std::move(read), std::move(reduce)}))
{
}

ReadjustSurveyJobCommand::~ReadjustSurveyJobCommand() = default;

Status ReadjustSurveyJobCommand::validate(const katana::commands::CommandContext&) const
{
    return notYet();
}

Status ReadjustSurveyJobCommand::execute(katana::commands::CommandContext&)
{
    return notYet();
}

Status ReadjustSurveyJobCommand::undo(katana::commands::CommandContext&)
{
    return notYet();
}

Status ReadjustSurveyJobCommand::redo(katana::commands::CommandContext&)
{
    return notYet();
}

std::vector<katana::entity::EntityId> ReadjustSurveyJobCommand::createdEntities() const
{
    return {};
}

const katana::survey::ReductionReport* ReadjustSurveyJobCommand::report() const
{
    return nullptr;
}

} // namespace katana::cad
