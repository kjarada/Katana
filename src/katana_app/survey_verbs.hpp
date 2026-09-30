#pragma once

// SURVEY READ and SURVEY IMPORT: a survey field file - any format surveyio
// reads (docs/survey.md) - read, or imported as a survey job, from a line.
// Here, in the session, because cad may not see surveyio
// (tools/check_layering.cmake); the window's command line runs the same
// function (MainWindow::runWorkbenchLine), so katana_cli, katana_mcp and the
// window give one reply.
//
//   SURVEY READ <file> [FORMAT <id>]
//   SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>] [SETTINGS <file>]
//                 [SET <key>=<value> ...]
//
// READ reads the file and says what the reader made of it, changing nothing.
// IMPORT does what the import wizard's Import does: the reduction, the points
// drawn on LAYER (survey/points), and the job kept on the drawing (Survey >
// Survey Jobs) - ONE undo step (cad::ImportSurveyJobCommand). The format is
// detected unless FORMAT names it; a detection that is not certain is
// refused, naming the candidates, so that a file is never read with a reader
// that only thinks it fits. Each option is given at most once.
//
// The reduction runs with the wizard's starting settings - ReductionSettings'
// defaults, holding the control the file declares - or, with SETTINGS, the
// settings a file holds in their stable text form (the one a job keeps,
// survey/reduction_settings.hpp), whole: its control lines are all the
// control. SET's items are lines of that same text, each changing the key it
// names; a control item (control=<id>;<file|drawing>;then constraint;sigma
// for northing, easting and elevation) holds its point as the wizard's Hold
// does, in place of the point of that id or after the others. A point held
// from the drawing is found among the drawing's survey points, as the
// wizard's are (cad::reductionContextFor), and the import reduces as the
// wizard's does (cad::reduceForDrawing): an id the drawing has at two places
// is refused. Both are read by survey::parseReductionSettings and refused
// with its words; a key SET names that this version does not know is
// refused, where a SETTINGS file's is skipped and reported, as the text
// form's versioning has it. A SETTINGS file is decoded as any text file is
// (core::decodeText), so a byte order mark or UTF-16 reads the same.
//
// The reply is records, one per line:
//
//   survey file=job.fld format=opcode-field-file parser=1.0 read=14 skipped=2 warnings=2
//   content setups=1 observations=12 points=2 unpositioned=2 features=1 control=0
//   declared crs="Australia/GDA2020, Zone 56" epsg=none
//   warning record=20 text="..."           (the first 20; then warnings_more=<n>)
//   not_carried text="no atmospheric settings"
//   settings file=held.txt set=1 differ=2   (IMPORT only; file= empty without SETTINGS)
//   setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0
//                                          (each line that is not the defaults')
//   settings_warning text="line 3: ..."    (a SETTINGS key this version skipped)
//   imported job=job-1 entities=4 layer=survey/points reduction_warnings=2  (IMPORT only)
//   held id=CP1 from=drawing entity=7 northing=5e+06 easting=5e+05 height=100
//                                          (each point held; from=file has no entity)
//   reduction method=radiation adjustments=0 rejected=0
//   adjustment method="network least squares (horizontal)" observations=10 unknowns=4
//     redundancy=6 variance_factor=0.1697573300133813 global_test=failed flagged=0 rejected=0
//                                          (each adjustment run; none where absent)
//   reduction_warning text="..."           (the first 20; then reduction_warnings_more=<n>)

#include <string>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/survey/reduction_report.hpp"

namespace katana::app {

// True for a line whose verb is SURVEY.
[[nodiscard]] bool isSurveyLine(std::string_view line);

// Runs a SURVEY line on `document`. InvalidArgument for a line the grammar
// above does not take; the settings parser's, the detection's, the reader's
// or the import's own error otherwise, with the drawing unchanged.
[[nodiscard]] katana::core::Result<std::string> runSurveyLine(katana::cad::Document& document,
                                                              std::string_view line);

// The reply's records of a reduction's outcome, each after a line break: the
// reduction (the settings' method in words, how many adjustments ran, the
// observations rejected - survey::rejectedObservations, the count the wizard
// shows), then each adjustment's statistics as its report holds them.
[[nodiscard]] std::string reductionRecords(const katana::survey::ReductionReport& report);

// The lines --help prints for it.
[[nodiscard]] const char* surveyHelpText();

} // namespace katana::app
