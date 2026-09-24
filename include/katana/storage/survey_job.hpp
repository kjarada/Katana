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

#include <optional>
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

// One point the job put on the drawing: which entity holds it and the
// coordinates the job gave it, in the survey model's metres (northing, easting;
// the drawing's y and x).
//
// Kept on the job rather than read back from the entity because the entity is
// the PERSON'S from the moment it is drawn: re-adjusting compares the two to
// tell a point moved or re-levelled by hand since (whose edit the person may
// want kept) from one the job may move, and a point deleted by hand from one
// the new run no longer produces - neither of which the entity alone can say.
struct SurveyJobPoint {
    std::string pointId;
    katana::entity::EntityId entity = katana::entity::kInvalidEntityId;
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> elevation{}; // absent is not zero

    friend bool operator==(const SurveyJobPoint&, const SurveyJobPoint&) = default;
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
    // The points among createdEntities, one per point id, as the job last
    // placed them (SurveyJobPoint). A point the person deleted by hand stays
    // listed - the job still made it - so that re-adjusting can keep the
    // deletion rather than silently draw the point again.
    std::vector<SurveyJobPoint> placedPoints{};
    // How the import drew the job's points - a layer per field code or not,
    // the property names, provenance - as versioned text written and read by
    // cad (cad::SurveyImportOptions without the layer, which is `layer`
    // above). Kept so that a point a re-adjustment draws later goes where
    // and as the job's other points went. Storage keeps it as it is and never
    // reads it; empty means the defaults.
    std::string importOptions{};

    friend bool operator==(const SurveyJob&, const SurveyJob&) = default;
};

} // namespace katana::storage
