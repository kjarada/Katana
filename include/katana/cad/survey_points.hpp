#pragma once

// The drawing's survey points as a list, and what the import wizard, the
// point export, the Point Manager and the Point Report need from it (PLAN.MD
// 45 slices 10, 11 and 13).
//
// A SURVEY POINT here is a point entity carrying the point-number property
// the survey import writes (SurveyImportOptions::pointNumberProperty, "point"
// by default) - the same test the survey tools use to name a point. A point
// entity without one is a CAD point, not a survey mark: it has no id to list,
// export or match against an import, so it is left out and, where a caller
// asked for it by id, counted as left out rather than dropped silently.
//
// Everything here reads the drawing through the survey import's own property
// and metadata keys (survey_import.hpp) and heights through the one reader of
// heights (entity::heightsOf), so a point written by the import reads back as
// it went in. Drawing coordinates are taken as METRES, because that is what
// importSurveyProject writes into them: the survey model is metres and the
// bridge puts its eastings and northings into the drawing unchanged.
//
// Nothing here sees katana::surveyio (cad may not - tools/check_layering.cmake):
// the wizard hands a parsed survey::SurveyProject in and gets a command back,
// and the export hands survey::SurveyPoints to the delimited writer itself.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_import.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/survey/data_model.hpp"

namespace katana::cad {

// One survey point as the drawing holds it.
struct DrawingSurveyPoint {
    katana::entity::EntityId entity = katana::entity::kInvalidEntityId;
    std::string id; // the point number
    std::string code;
    std::string description;
    double easting = 0.0;  // drawing x
    double northing = 0.0; // drawing y
    std::optional<double> elevation; // absent is not zero
    std::string layer;
    std::string sourceFile; // the file NAME the import recorded; empty when none

    friend bool operator==(const DrawingSurveyPoint&, const DrawingSurveyPoint&) = default;
};

// Every survey point in the drawing, in entity id order (the order they were
// added), whatever layer they are on - a hidden layer's points are still the
// survey's points.
[[nodiscard]] std::vector<DrawingSurveyPoint>
drawingSurveyPoints(const Document& document, const SurveyImportOptions& options = {});

// The survey points among `ids`, in the order given, and a count of the ids
// that are not one (another kind of entity, a point with no number, or an id
// the drawing does not have).
struct SurveyPointPick {
    std::vector<DrawingSurveyPoint> points;
    std::size_t notSurveyPoints = 0;
};
[[nodiscard]] SurveyPointPick surveyPointsAmong(const Document& document,
                                                std::span<const katana::entity::EntityId> ids,
                                                const SurveyImportOptions& options = {});

// The survey model's point for one in the drawing - id, northing, easting,
// elevation (still absent when absent), code and description - for a writer.
// Where it came from is not carried over: an exported file is a new source.
[[nodiscard]] katana::survey::SurveyPoint toSurveyPoint(const DrawingSurveyPoint& point);

// ---- importing into a drawing that already has points --------------------------------------

// What to do with an imported point whose id a survey point in the drawing
// already has. There is no silent default: the wizard shows the choice, and
// each one's consequence is said in the report.
enum class ExistingPointPolicy {
    // Nothing is imported; the error names the ids. For a file that should be
    // new to this drawing, a clash means it is the wrong file or a repeat.
    Refuse,
    // The drawing's point stays; the file's point of that id is not imported.
    // For topping up a drawing with the new points of a longer file.
    Skip,
    // The drawing's point is deleted and the file's is imported, in the same
    // command (one undo puts the old one back). For a re-observed control file.
    Replace,
    // Both stay: the drawing then has two points of that id, which the report
    // says. For a file whose numbering restarts and is known to.
    KeepBoth,
};

[[nodiscard]] const char* toString(ExistingPointPolicy policy);

struct SurveyPointImportReport {
    SurveyImportReport import; // what importSurveyProject reported
    // Ids of the project that a survey point in the drawing already has, in
    // the project's order, each once.
    std::vector<std::string> existingIds;
    std::size_t skipped = 0;  // points not imported under Skip
    std::size_t replaced = 0; // drawing entities deleted under Replace
    // Every warning of the import, and one sentence for what the policy did.
    std::vector<std::string> warnings;
};

// `project` into the drawing as ONE command: under Replace, the deletion of
// the points it replaces and then importSurveyProject's layers and points.
// AlreadyExists, naming the ids (the first dozen, then how many more), under
// Refuse when any id is already in the drawing; the errors of
// importSurveyProject otherwise. nullptr with no error when nothing is left to
// import - an empty file, or every point skipped - which the report says.
//
// The command is built, not executed: the wizard calls this to show what an
// import WILL do on its last page and again to do it, so the preview and the
// import cannot disagree.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
importSurveyPoints(const Document& document, const katana::survey::SurveyProject& project,
                   const SurveyImportOptions& options, ExistingPointPolicy policy,
                   SurveyPointImportReport* report = nullptr);

// ---- coordinate systems ---------------------------------------------------------------------

// The system a person says a file is in, checked against the EPSG database so
// that a mistyped code is refused rather than recorded. InvalidCRS / NotFound
// from geodesy for a code it does not know.
[[nodiscard]] katana::core::Result<katana::survey::DeclaredCoordinateSystem>
declaredSystemFromEpsg(int code);

// Every point of `project` from projected system `sourceEpsg` to projected
// system `targetEpsg` through katana::geodesy's transformer (never a formula of
// this module's own). The parser never transforms (PLAN.MD 45.2); this is the
// separate, stated step a person asks for by naming BOTH codes.
//
// Horizontal only: eastings and northings are transformed, heights are carried
// through unchanged - a height datum is not a projection, and no vertical
// transformation is made or implied. The model's metres are converted into
// each system's own linear unit and back. Unsupported for a geographic,
// geocentric or compound system (a delimited point file holds grid
// coordinates; the reader refuses latitude and longitude); the transformer's
// error, naming the point, for a point it cannot transform - all or nothing.
// On success the project declares the target system, and its metadata records
// "transformed from" and "transformation" (the operation PROJ used).
[[nodiscard]] katana::core::Result<katana::survey::SurveyProject>
transformSurveyProject(katana::survey::SurveyProject project, int sourceEpsg, int targetEpsg);

// ---- reports --------------------------------------------------------------------------------

// The Point Report: a heading, then one row per point - id, easting,
// northing, elevation (blank when absent), code, description, source file -
// with coordinates to `decimals` places (0 to 12), then a count.
[[nodiscard]] std::string formatPointReport(std::span<const DrawingSurveyPoint> points,
                                            int decimals = 3);

// The same rows as CSV (RFC 4180: a field holding a comma, a quote, a line
// break or blanks at either end is quoted, a quote doubled), with a header
// line, CRLF line ends and an absent elevation as an empty field.
[[nodiscard]] std::string pointReportCsv(std::span<const DrawingSurveyPoint> points,
                                         int decimals = 3);

// What the import wizard's last page shows and the log records. The parser's
// part (file, format, records read, its warnings) is given as text because
// cad does not see surveyio; the drawing's part is this module's report.
struct SurveyImportSummary {
    std::string fileName;
    std::string format;         // "Delimited points 1.0"
    std::string layout;         // the layout template, when the format has one
    std::size_t recordsRead = 0;
    std::vector<std::string> readerWarnings;
    std::string coordinateSystem; // "unknown" or what the person declared
    std::string transformation;   // empty when none was asked for
    std::string units;            // what the file's numbers were declared in
    ExistingPointPolicy policy = ExistingPointPolicy::Refuse;
};

// Lines of a report: what was read, every warning as a sentence, and either
// the error blocking the import or the points and layers it will create.
// `blocking` is that error, when there is one.
[[nodiscard]] std::string formatSurveyImportReport(const SurveyImportSummary& summary,
                                                   const SurveyPointImportReport& report,
                                                   const katana::core::Error* blocking = nullptr);

} // namespace katana::cad
