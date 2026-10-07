#pragma once

// LINEWORK: joining coded points into lines from the text command line
// (docs/survey_coding.md, "Linework"). The interpreter's, so the window's
// command line, katana_cli, katana_mcp and an AI agent all run the same verb;
// until now Process Linework was one tab of the Survey Code Manager and
// nothing else.
//
//   LINEWORK [<scope>] [WHERE k=v ...] [PROPERTY <name>] [ORDER number|entity]
//            [CHORD <length>] [PREVIEW]
//
// <scope> is the shared scope and filter words (scope_verbs.hpp), and comes
// first. With no scope word it is THE SELECTION WHEN ANYTHING IS SELECTED,
// ELSE THE WHOLE DRAWING - also under a bare WHERE - and the reply says which
// (scope=). PROPERTY names the property the codes are read from, as CODE's
// PROPERTY does; without it the property is found (codePropertyCandidates).
// ORDER says how the points of one string are ordered: by their point number
// (the default) or by entity, which for an import is the order the file listed
// them. CHORD is how far, in the drawing's units, the straight segments drawn
// for a curve ("BC" ... "EC") may stray from it: LineworkOptions::
// chordTolerance, 0.005 unless said. PREVIEW plans and reports, and changes
// nothing. Each of the verb's own words is given at most once.
//
// PROPERTY and CHORD came with the Survey Code Manager's Linework tab, which
// had both as controls of its own while it ran processLinework itself: it now
// runs this line (CLAUDE.md section 1, one code path), and a control the line
// could not say would have had to go. A property is what CODE already takes,
// so that points coded under a name of the drawing's own can be strung as
// they were coded; a chord length is geometry no survey code holds.
//
// It is cad::processLinework over the points the scope takes, with what the
// drawing's customisation says of linework: the control codes are the
// Document's (customisationState().linework) and the lines are styled with
// the colours of its resolver (colour_lookup.hpp). A line is drawn where a
// rule makes its code a line or a control code asks for one - the run was
// asked for by name, so a control code alone still draws, as it does from the
// Survey Code Manager. processLinework decides that point by point, so where
// NO rule makes the code a line only the points that themselves carry a
// control code are joined: of "KB ST", "KB", "KB CL" the first and the third
// make the line and the second is unplaced, with the reason. It NEVER removes
// a point. Every line it draws is ONE undo step; a run that draws nothing is
// no step at all.
//
// POINTS THEIR SURVEY JOB HAS ALREADY STRUNG ARE LEFT OUT, and counted. A job
// imported with linework on (survey_finish.hpp) drew its own lines - the
// file's numbered strings among them, which only the file knows - and owns
// them: a re-adjustment redraws them in place and Remove Job deletes them. A
// second line strung through the same points here would lie on top of the
// job's and be nobody's. How it is known: the job's stored options say it was
// imported with linework (SurveyJobOptions::drawLinework), the point is among
// the job's createdEntities, and a polyline among those same createdEntities,
// still in the drawing, carries the point's STRING NAME - its code followed
// by its string number (survey_coding.hpp). So a point of such a job whose
// string the job drew no line of - a rule added since, a line deleted by hand
// - is still strung here; and a job whose option text cannot be read refuses
// the run, naming the job, rather than have it guess. Such a job's points are
// strung BY THEIR CODES, as any points are: a string the file closed and
// began again under one number is one name, and comes out as one open line.
//
// A LINE THE DRAWING ALREADY HOLDS IS NOT DRAWN AGAIN, and is counted
// (LineworkOptions::skipLinesAlreadyDrawn): a polyline that carries the same
// code and string number and runs through the same vertices at the same
// heights, closed alike. The verb is run again and again over the same points
// - after a rule is written, after more are surveyed - and the job rule above
// knows nothing of the lines the verb itself drew, nor of an import's that is
// no job's: without this each run laid the same lines on top of the last. It
// is the VERY line that is known, not its name: a string that has gained a
// point, or whose point was moved or re-levelled, is another line and is
// drawn, and the line that was there stays until someone erases it. (Redrawing
// it in place, as a job redraws its own, would take whichever line carries
// that name - and two surveys of one site both have a KB1.)
//
// The reply is records, one a line:
//
//   linework scope=drawing matched=14 considered=9 lines=2 unplaced=3 notes=1
//                                          (preview=yes after them for a PREVIEW)
//   left_out=4 reason=strung-by-their-job  (points; only when some were left out)
//   left_out=3 reason=already-drawn lines=1
//                                          (lines= the lines not drawn again, and
//                                          the points they run through, each once;
//                                          only when there is one)
//   string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB entity=31
//                                          (each line, the first 50; then
//                                          strings_more=<n>; join=yes on a join
//                                          line; no entity= in a preview)
//   unplaced reason="its code is a point code" points=2
//                                          (each reason a point is in no line)
//   note kind="unknown token" count=1      (each kind of note)
//
// matched= is what the scope took (scopeRecord); considered= the point
// entities among it that were looked at - the ones left to their job are not
// among them, the ones of a line already drawn are; lines= the lines drawn
// (in a preview, to be drawn). A scope that takes nothing, or no point, is
// answered with the same records and zeros: it is not a failure.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/survey_finish.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

// True for LINEWORK, in any case.
[[nodiscard]] bool isLineworkVerb(std::string_view verb);

// Runs one LINEWORK line: `tokens` is the whole line split into words with
// the quotes removed (CommandInterpreter::tokenize), the verb first. `views`
// answers the scope word VIEW (the window's; none headless).
//
// ParseFailure for a word the grammar above does not take, naming it; the
// scope's own refusals (scope_verbs.hpp); a survey job's ParseFailure or
// Unsupported when its option text cannot be read and it owns a point the
// scope took; processLinework's own error otherwise (control codes spelled
// alike, a rule's layer that is not a layer name). A refused run changes
// nothing.
[[nodiscard]] katana::core::Result<std::string>
runLineworkVerb(Document& document, const std::vector<std::string>& tokens,
                const ScopeViewProvider& views = {});

// The verb with every word it takes and what the reply holds, for HELP
// LINEWORK.
[[nodiscard]] std::string lineworkVerbHelp();

// How many lines a LINEWORK reply lists, each a record; the rest are counted
// (strings_more=). A survey of a few thousand points strings a few hundred
// lines, and a reply is read by a person in a log as well as by an agent.
inline constexpr std::size_t kLineworkStringsListed = 50;

// ---- the plan, apart from the words -------------------------------------------------
//
// What a LINEWORK line plans over the entities its scope took: the verb's own
// planning, for a caller that shows the WHOLE plan before the line is run.
// The Survey Code Manager's Linework tab is one - its Preview lists every
// line, every point in none and every note, where the reply counts them - and
// its Execute then runs the line. What it previewed must therefore be planned
// by the function the line is planned by, not by a second reading of the
// rules at the top of this header (the Document's control codes and colours,
// no point removed, the points their job has strung left out, a line already
// drawn not drawn again): two readings come to differ, and the preview would
// then show something Execute does not do.

// What a line says beyond its scope and PREVIEW.
struct LineworkWords {
    // PROPERTY. Empty: found, as applySurveyCodes finds it.
    std::string property{};
    LineworkOrder order = LineworkOrder::PointNumber; // ORDER
    // CHORD, in model units. None: LineworkOptions::chordTolerance as it
    // stands.
    std::optional<double> chord{};
};

struct LineworkPlan {
    // Of the entities asked about, the points their survey job has already
    // strung: left out of `planned`. In the order asked.
    std::vector<katana::entity::EntityId> strungByTheirJob{};
    // processLinework over the rest. An empty report and no command when
    // nothing is left to look at: an empty list is never handed on, because
    // to processLinework it means every point in the drawing.
    LineworkResult planned{};
};

// Plans, and changes nothing. `matched` is what a scope took (matchScope, or
// matchEntities for a caller that holds a ModifyScope and ModifyFilter). The
// errors are runLineworkVerb's, less the grammar's and the scope's: a survey
// job whose option text cannot be read, and processLinework's own (a chord
// length that is not a positive finite number among them). The command, when
// there is one, must be executed on `document`.
[[nodiscard]] katana::core::Result<LineworkPlan>
planLinework(const Document& document, const std::vector<katana::entity::EntityId>& matched,
             const LineworkWords& words);

// ---- what a survey import does beyond drawing its points ---------------------------
//
// The finish of survey_finish.hpp as the drawing's customisation asks for it.
// ONE place decides it, so SURVEY IMPORT and the import wizard cannot come to
// differ: the test that holds the two together compares the whole job.
//
//   WHETHER  What the line or the dialog said (`codes`, `linework`); where it
//            said nothing, the Document's automation switch
//            (customisationState().automation - both on until a customisation
//            or a setting says otherwise).
//   NOT AT ALL WITH NO SURVEY CODES LOADED. `options.codes` and
//            `options.linework` are then off whatever was asked, and the
//            import is the plain one to the last byte - the same job text,
//            the same report. The finish would draw nothing either way
//            (SurveyFinishSkip::NoSurveyCodes), but asked for it would still
//            store the job as one to finish and add two warnings to its
//            report; and a re-adjustment codes only the points it draws for
//            the first time, so a job stored that way would come out half
//            coded once codes were loaded. A job imported with no codes
//            loaded is a job that was not coded, and is kept as one.
//   HOW      Colours through the Document's resolver (colour_lookup.hpp), the
//            control codes the Document's (customisationState().linework),
//            strings ordered by point number.
struct SurveyImportFinish {
    bool codesAsked = false;    // said so, or the Document's switch is on
    bool lineworkAsked = false; // likewise
    // The drawing has survey codes; without them nothing is handed on.
    bool surveyCodesLoaded = false;
    // What to hand the import (SurveyJobImport::finish, withSurveyFinish).
    SurveyFinishOptions options{};
};

[[nodiscard]] SurveyImportFinish surveyImportFinish(const Document& document,
                                                    std::optional<bool> codes = std::nullopt,
                                                    std::optional<bool> linework = std::nullopt);

} // namespace katana::cad
