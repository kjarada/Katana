#pragma once

// SURVEY READ and SURVEY IMPORT: a survey field file - any format surveyio
// reads (docs/survey.md) - read, or imported as a survey job, from a line.
// Here, in the session, because cad may not see surveyio
// (tools/check_layering.cmake); the window's command line runs the same
// function (MainWindow::runWorkbenchLine), so katana_cli, katana_mcp and the
// window give one reply.
//
//   SURVEY READ <file> [FORMAT <id>]
//   SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>]
//
// READ reads the file and says what the reader made of it, changing nothing.
// IMPORT does what the import wizard's Import does with its defaults: the
// reduction with ReductionSettings' defaults and the control the file
// declares, the points drawn on LAYER (survey/points), and the job kept on
// the drawing (Survey > Survey Jobs) - ONE undo step
// (cad::ImportSurveyJobCommand). The format is detected unless FORMAT names
// it; a detection that is not certain is refused, naming the candidates, so
// that a file is never read with a reader that only thinks it fits.
//
// The reply is records, one per line:
//
//   survey file=job.fld format=opcode-field-file parser=1.0 read=14 skipped=2 warnings=2
//   content setups=1 observations=12 points=2 unpositioned=2 features=1 control=0
//   declared crs="Australia/GDA2020, Zone 56" epsg=none
//   warning record=20 text="..."           (the first 20; then warnings_more=<n>)
//   not_carried text="no atmospheric settings"
//   imported job=job-1 entities=4 layer=survey/points reduction_warnings=2  (IMPORT only)
//   reduction_warning text="..."           (the first 20; then reduction_warnings_more=<n>)

#include <string>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

namespace katana::app {

// True for a line whose verb is SURVEY.
[[nodiscard]] bool isSurveyLine(std::string_view line);

// Runs a SURVEY line on `document`. InvalidArgument for a line the grammar
// above does not take; the detection's, the reader's or the import's own
// error otherwise, with the drawing unchanged.
[[nodiscard]] katana::core::Result<std::string> runSurveyLine(katana::cad::Document& document,
                                                              std::string_view line);

// The lines --help prints for it.
[[nodiscard]] const char* surveyHelpText();

} // namespace katana::app
