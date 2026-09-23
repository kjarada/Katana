#pragma once

// Survey field data into the drawing (PLAN.MD 45.3).
//
// This is the ONLY place a katana::survey value becomes a CAD entity, and it
// lives in cad rather than in surveyio deliberately: cad may not see surveyio
// (tools/check_layering.cmake), so a Leica, Trimble or Topcon type cannot reach
// the drawing even by accident. Everything that arrives here has already been
// normalised into metres, radians and northing/easting by a parser.
//
// The whole import is ONE command. One Ctrl+Z undoes an import of ten thousand
// points, not the last one of them - which is what PLAN.MD 45 asks for and what
// `applySurveyCodes` already does for the same reason.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/survey/data_model.hpp"

namespace katana::cad {

// Keys this import writes that are NOT configurable, because they describe the
// import rather than the survey. They are the brief's source-tracking fields:
// "Source Manufacturer / Source Format / Source File / Source Record", so that
// "where did point 102 come from" is answerable from the drawing alone.
//
// The provenance ones go in Entity::metadata, which entity.hpp reserves for
// exactly this; the coordinate source is a property because it is a fact about
// the survey that a report is entitled to show.
inline constexpr std::string_view kCoordinateSourceProperty = "survey.coordinate_source";
inline constexpr std::string_view kSourceManufacturerMeta = "survey.manufacturer";
inline constexpr std::string_view kSourceFormatMeta = "survey.format";
inline constexpr std::string_view kSourceFormatVersionMeta = "survey.format_version";
inline constexpr std::string_view kSourceFileMeta = "survey.file";
inline constexpr std::string_view kSourceRecordMeta = "survey.record";

struct SurveyImportOptions {
    // Where the points go: a '/'-separated layer path (entity/layer_path.hpp).
    std::string layer = "survey/points";
    // A layer per field code beneath `layer`, so that "EP" and "BM" can be
    // switched on and off independently. That is how a surveyor reads a drawing
    // and it costs one layer per distinct code; off, everything lands on
    // `layer`.
    bool layerPerCode = false;
    // A layer the drawing does not have is created as part of the same
    // transaction. Off, points on a missing layer are refused rather than
    // silently landing on "0" - a point on the wrong layer is worse than an
    // error, because nothing about it looks wrong later.
    bool createLayers = true;

    // Where the point's own fields are written. The defaults are exactly what
    // `codePropertyCandidates()` looks for, so `applySurveyCodes()` finds the
    // codes afterwards without being told where they are - importing field data
    // and then applying a mapfile to it is one workflow, not two.
    std::string codeProperty = "code";
    std::string pointNumberProperty = "point";
    std::string descriptionProperty = "description";

    // Provenance into Entity::metadata, which entity.hpp documents as the place
    // for it: the manufacturer, the format and its parser version, the source
    // file NAME and the record within it. This is what makes "where did point
    // 102 come from" answerable a year later.
    bool recordSource = true;
};

struct SurveyImportReport {
    std::size_t points = 0;
    std::vector<std::string> layersCreated; // in name order
    // Things worth telling the user that are not failures - a metadata field
    // that could not be written because the import already uses that property,
    // for instance. There is deliberately no "skipped" count: every point in a
    // project that passed survey::validateProject() is importable, so a point
    // that vanished would be a bug and not a statistic.
    std::vector<std::string> warnings;
};

// The project as one command: the layers it needs, then its points.
//
// Fails with the error `survey::validateProject` gives when the project is not
// internally consistent - it is called rather than re-checked here, so that an
// importer and this bridge cannot disagree about what a valid project is. Fails
// with InvalidArgument when a layer is missing and `createLayers` is off.
// Returns nullptr with NO error when the project has no points: "nothing to do"
// and "something went wrong" must be distinguishable, the same contract
// `applySurveyCodes` follows.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
importSurveyProject(const Document& document, const katana::survey::SurveyProject& project,
                    const SurveyImportOptions& options, SurveyImportReport* report = nullptr);

// The layer a point would land on under these options, exposed so that the
// import wizard can show the layers an import is about to create before it runs.
// Field codes are sanitised: a code is user data that can contain anything, and
// a '/' in it would silently create a nested layer subtree, because a layer name
// IS a path.
[[nodiscard]] std::string layerForPoint(const katana::survey::SurveyPoint& point,
                                        const SurveyImportOptions& options);

} // namespace katana::cad
