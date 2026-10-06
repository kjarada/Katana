#pragma once

// Field to finish in ONE step: a survey import that also codes what it drew
// and strings it.
//
// Importing a coded field file used to be three steps a person had to know
// about - import, Apply Survey Codes, Process Linework - each its own undo
// step, the second replacing the selection and the third reachable from one
// dialog only. `withSurveyFinish` makes them one command: it OWNS the command
// that draws the points, runs it, and then
//
//   1. codes the points that command created (applySurveyCodes);
//   2. draws the strings the file NUMBERED itself (drawSurveyFeatures, only
//      the features that have a name, name a point of this import and that a
//      rule makes a line);
//   3. strings the other created points by their codes (processLinework, by
//      the same rule).
//
// The decisions the rest of this header builds on:
//
// 1. EACH STEP IS PLANNED WHEN IT RUNS, as the styling of linework's lines
//    always was: applySurveyCodes and processLinework read the drawing, and
//    the points are not in it until the import has run; drawSurveyFeatures
//    fixes the layers it creates when it is planned, and creating a layer
//    that exists is refused, so it must be planned after the coding step has
//    made its layers. Undo and redo replay what was planned, so ids, layers
//    and styles come back exactly.
//
// 2. IT ACTS ON WHAT THE IMPORT CREATED AND NOTHING ELSE. An empty id list
//    means "every entity" to applySurveyCodes and "every point" to
//    processLinework, so neither is ever called with one: an import that drew
//    nothing codes nothing. A string of the file none of whose points this
//    import created is not drawn (a longer copy of a file, topped up, does
//    not draw the first copy's strings again). It never deletes a point.
//
// 3. ONLY A NAMED FEATURE IS A STRING OF THE FILE'S OWN. A field file that
//    keeps a string number beside the code gives each (code, number) a
//    feature named by the number: the file has said which points are one
//    line, and that is drawn as it stands. A feature with NO name says only
//    what its points' codes already say - and, from a reader whose format has
//    no string numbers, in pieces: one feature per run of consecutive shots,
//    so a kerb shot in sections across a road is many runs of one point and
//    none of them is the kerb. Such a feature is not drawn; its points are
//    strung by their codes, as Process Linework strings them.
//
// 4. A POINT IS IN AT MOST ONE AUTOMATIC LINE OF ITS OWN STRING. A point a
//    named feature OF ITS OWN CODE holds is that string's - the test
//    nameSurveyStrings names it by - and the feature has answered for it,
//    with a line or with the reason there is none, so it is not looked at
//    again by its code. A named feature of ANOTHER code (a line keyed between
//    two kerb points and coded as a boundary) is drawn or not on its own and
//    leaves its ends in their own code's string.
//
// 5. A LINE RUNS THROUGH ITS POINTS WHERE THEY STAND. A vertex is taken from
//    the point's entity when this import, or the job being re-adjusted, has
//    one in the drawing - so a point the person moved, and a re-adjustment
//    keeps where they put it, is still on its line. A point with no such
//    entity is, for a string of the file's own, where the file puts it: the
//    drawing's own control, a point a policy skipped, and one the person
//    deleted - deleting a mark does not take a vertex out of a kerb the file
//    strung, and the report counts the vertices that are at such a point. A
//    string strung by CODE has only the entities to read, and passes a
//    deleted point by.
//
// 6. A STEP THAT HAS NOTHING TO DO SAYS WHY AND IS NOT AN ERROR. No survey
//    codes loaded, no code in the file, no rule for any code: no layer, style
//    or line is made and no point is moved, and the report gives the reason
//    (SurveyFinishSkip). What IS an error - a rule that cannot become a layer
//    or a style, control codes spelled ambiguously - fails the whole import,
//    points included, with that error: an import that asked for codes and
//    silently got none would look like one whose codes matched nothing.

#include <cstddef>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/linework.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/cad/survey_import.hpp"
#include "katana/commands/command.hpp"
#include "katana/entity/entity.hpp"
#include "katana/survey/data_model.hpp"

namespace katana::cad {

struct SurveyFinishOptions {
    // Both OFF by default, so a caller that says nothing gets the import it
    // always got. The front ends turn them on from the customisation.
    bool codes = false;    // code the points the import creates
    bool linework = false; // string them: the file's features, then the rest by code
    // How points and lines are coded: `colourOf`, `createLayers`,
    // `createStyles`, `setAttributes`. `property` and `ids` are filled when
    // the step runs (the import's code property; what the import created).
    SurveyCodingOptions coding{};
    // The control codes a point's code may carry, and how a string is ordered
    // (LineworkOptions::codes and ::order).
    LineworkCodes controls{};
    LineworkOrder order = LineworkOrder::PointNumber;

    [[nodiscard]] bool any() const { return codes || linework; }
};

// Why a step did not run. `None`: it ran.
enum class SurveyFinishSkip {
    None,
    NotAsked,      // its option is off
    NoPoints,      // the import drew no point, so there was nothing to act on
    NoSurveyCodes, // no survey codes are loaded (the drawing's map is empty)
    NoCodesInFile, // none of the points, and no feature, carries a code
    NoRuleMatches, // no rule answers any of those codes
};

// A word for a reply line: "ran", "not-asked", "no-points", "no-survey-codes",
// "no-codes-in-file", "no-rule-matches".
[[nodiscard]] std::string_view toString(SurveyFinishSkip skip);

// What the finish did, filled when the command runs (empty before, and left
// as the run filled it by an undo).
struct SurveyFinishReport {
    // Point entities the wrapped command created: an import's count of
    // points, whatever was then drawn through them.
    std::size_t points = 0;

    // ---- codes ----
    SurveyFinishSkip whyNotCoded = SurveyFinishSkip::NotAsked;
    // What applySurveyCodes did to those points. When the step did not run
    // for want of a code or a rule, only what was SEEN is here - `property`,
    // `coded`, `fallbackOnly` and the two lists of codes with no rule - and
    // nothing of what a run would have done.
    SurveyCodingReport coding{};

    // ---- linework ----
    SurveyFinishSkip whyNotStrung = SurveyFinishSkip::NotAsked;
    // The file's own strings: one LineworkString per NAMED feature drawn, and
    // the named features not drawn with why (drawSurveyFeatures under
    // onlyRuledLines; UnplacedFeature::index is the feature's place in the
    // `strings` handed to withSurveyFinish). A feature with no name, and one
    // that names no point of this import, is in neither: nothing was asked of
    // it.
    LineworkReport features{};
    std::vector<UnplacedFeature> unplacedFeatures{};
    // Points left to those strings: each is held by a named feature of its
    // own code, which has answered for it - a line, or the reason there is
    // none - so it is not strung again by its code.
    std::size_t pointsInFeatures = 0;
    // The rest, strung by code (processLinework under onlyRuledLines): its
    // lines, and each point not in one with why.
    LineworkReport strung{};
    // Every line CREATED, features first, in creation order.
    std::vector<katana::entity::EntityId> lines{};
    // A re-adjusted job only (SurveyFinishEarlier), of the lines its earlier
    // run drew:
    //   - redrawn: strung again by this run, and kept as the SAME entities -
    //     with their ids, layers and styles, and whatever follows them - at
    //     this run's vertices (a line that already ran there is among them,
    //     untouched), in the order strung;
    //   - removed: no longer strung by this run, and deleted;
    //   - kept: how many were left exactly as that run drew them, because the
    //     linework step did not run.
    std::vector<katana::entity::EntityId> earlierLinesRedrawn{};
    std::vector<katana::entity::EntityId> earlierLinesRemoved{};
    std::size_t earlierLinesKept = 0;
    // ... and how many vertices of the lines of the file's own strings are
    // at a point the person deleted (SurveyFinishEarlier::deletedByHand):
    // there the line runs through where this run puts the point.
    std::size_t verticesAtDeletedPoints = 0;

    [[nodiscard]] bool codesRan() const { return whyNotCoded == SurveyFinishSkip::None; }
    [[nodiscard]] bool lineworkRan() const { return whyNotStrung == SurveyFinishSkip::None; }
    // Codes the points carry that no rule more specific than "*" answers:
    // the unmatched and the fallback-only together, in name order.
    [[nodiscard]] std::vector<std::string> codesWithNoRule() const;
    // Layers and styles created by all three steps, distinct, in name order.
    [[nodiscard]] std::vector<std::string> layersCreated() const;
    [[nodiscard]] std::vector<std::string> stylesCreated() const;
    // Strings of the file and points in no line: unplacedFeatures plus
    // strung.unplaced. Most are no fault - a survey mark is a point - so see
    // the reasons before calling it a problem (finishWarnings does).
    [[nodiscard]] std::size_t unplaced() const
    {
        return unplacedFeatures.size() + strung.unplaced.size();
    }
};

// The report as sentences, for a log: per step that was asked for, what ran
// with its counts or why it did not, and then what was left over - the codes
// with no rule, the strings and points in no line by reason, a job's lines
// deleted or left as an earlier adjustment drew them.
[[nodiscard]] std::vector<std::string> describe(const SurveyFinishReport& report);

// Those of `describe`'s sentences a person may have to act on, and none of
// the ones that only say what was done:
//   - a step that was asked for and had nothing to go on (no survey codes
//     loaded, no code in the file, no rule for any code);
//   - codes no rule answers;
//   - a string or a point left out of every line for a reason other than
//     being what it is (a point code, or a point with no code);
//   - a job's lines deleted, left as an earlier adjustment drew them, or run
//     through a point that was deleted by hand.
// Empty for a finish that did all it was asked. This is what a survey job
// adds to its report's warnings; the rest is in its finish report.
[[nodiscard]] std::vector<std::string> finishWarnings(const SurveyFinishReport& report);

// What a RE-ADJUSTED job hands over beside its command: the points it drew
// before, which are strung again with the new ones (and never coded again -
// they were, or the person has changed them since), and the lines its earlier
// run drew. When the linework step runs, each of those that this run strings
// again is redrawn in place and the others are deleted; when it does not,
// they are left alone and counted. Empty for an import.
struct SurveyFinishEarlier {
    std::vector<katana::entity::EntityId> points{};
    std::vector<katana::entity::EntityId> lines{};
    // The ids of the job's points the person deleted from the drawing and the
    // re-adjustment leaves deleted. Nothing is done about them - a string of
    // the file's runs through where the run puts each, a string by code
    // passes it by - but the report counts the vertices that are at one.
    std::vector<std::string> deletedByHand{};
};

// Gives each of `points` that a NAMED feature of its own code holds that
// name as metadata kSurveyStringProperty, which the import then writes as a
// property like any other metadata key. For the field files that keep a
// string number apart from the code: the builder they share leaves the code
// alone on the point and the number only in the feature's name. The FIRST
// such feature, in the project's order, names the point; a feature of another
// code does not (a point shot again under a second code is still of its
// first), and a point that already has the key keeps it.
//
// Call it on the project before planning its import, and only for an import
// that is to be finished INTO A DRAWING THAT HAS SURVEY CODES, as a survey job
// does: without a finish, or with no codes loaded, the points are then drawn
// exactly as they always were, to the last property. (With codes loaded a
// point of a numbered string carries its number whether or not a rule then
// answers it - the one thing a finish with nothing to do leaves behind.)
void nameSurveyStrings(std::vector<katana::survey::SurveyPoint>& points,
                       const std::vector<katana::survey::SurveyFeature>& features);

// ONE command that owns `points`, runs it, and then codes and strings what it
// created, as `options` say. `points` is the unexecuted command of
// importSurveyPoints or importSurveyProject (or a job's own); `import` the
// options it was planned with.
//
// `strings` is what the file strung: the project's points, unpositioned
// points and features (all the points, also one that was not drawn - a line
// runs through where the file puts it). Pass an empty project for a file with
// no features - a delimited point list - and its points are strung by their
// codes alone. It is let go once the command has run: redo replays, and the
// undo history holds no copy of the survey.
//
// Returns `points` itself, unwrapped, when neither option is on - the caller
// then has exactly the command it had - and nullptr for a null `points`
// (nothing to import is still nothing to do). Otherwise the composite, named
// as `points` is: one undo takes back the import with its codes and its
// lines, and its createdEntities() are the points and then the lines.
//
// It must be executed on THIS document, as processLinework's command. Its
// validate() refuses another, and control codes spelled ambiguously
// (entity::validate), before anything is drawn.
//
// `report` is filled when it runs; for a null or unwrapped `points` it is
// filled at once, with NoPoints or NotAsked.
[[nodiscard]] katana::commands::CommandPtr
withSurveyFinish(const Document& document, katana::commands::CommandPtr points,
                 katana::survey::SurveyProject strings, const SurveyImportOptions& import,
                 SurveyFinishOptions options, std::shared_ptr<SurveyFinishReport> report = {},
                 SurveyFinishEarlier earlier = {});

} // namespace katana::cad
