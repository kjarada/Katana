#pragma once

// A survey job: one imported field file, kept so that it can be reduced and
// adjusted again.
//
// The value lives in storage, not in cad, because storage has to save it and
// may not see cad (tools/check_layering.cmake); cad::SurveyJob is this type
// (cad/survey_job.hpp), and the commands that create and change jobs are
// there. Plain values only: nothing here reads a file or runs a reduction.
//
// Why the RAW BYTES are kept rather than the parsed project: "go back and
// edit the adjustment" has to start from what was imported, not from what an
// older reader made of it. Re-adjusting re-reads these bytes with the reader
// the job names; if that reader has since been fixed, the job says so through
// `parserVersion`, and the report of the re-run shows the difference.

#include <string>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/survey/reduction_settings.hpp"

namespace katana::storage {

// Another file the reader fetched while importing the job (a DBX job's other
// files, a RINEX navigation file), exactly as read.
struct SurveyJobFile {
    std::string name; // a file NAME, never a path (survey::sourceFileName)
    std::string bytes;

    friend bool operator==(const SurveyJobFile&, const SurveyJobFile&) = default;
};

struct SurveyJob {
    // Unique within the project and never reused, so a report, a log line or
    // an entity's metadata can name a job and still mean the same job after
    // others were deleted. Assigned by the import command.
    std::string id;
    std::string name;          // shown to the person; starts as the file name
    std::string formatId;      // surveyio FormatDescriptor::id that read it
    std::string parserVersion; // of that reader, when this job was last read
    std::string sourceFileName;
    std::string sourceBytes; // the whole file, exactly as imported
    std::vector<SurveyJobFile> siblingFiles{};
    katana::survey::ReductionSettings settings{}; // as last adjusted
    std::string layer;                             // where its points were put
    // The entities the job put on the drawing (points and feature lines), in
    // creation order. Re-adjusting moves these rather than making new ones,
    // so a label or a dimension attached to a point survives it.
    std::vector<katana::entity::EntityId> createdEntities{};
    // The last reduction report as rendered, and when it was made (ISO 8601).
    std::string reportText{};
    std::string reportHtml{};
    std::string reportCreatedUtc{};
    std::string importedUtc{};

    friend bool operator==(const SurveyJob&, const SurveyJob&) = default;
};

} // namespace katana::storage
