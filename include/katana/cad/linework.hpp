#pragma once

// Field-to-finish linework: joining coded survey points into lines.
//
// A surveyor walking a kerb shoots "KB1", "KB1", "KB1" and expects a line, not
// three dots. The mapfile says WHICH codes are lines (`SurveyRule::breakline`)
// and how they are drawn; this is what actually joins them. Civil 3D ("Process
// Linework"), TBC ("Process Feature Codes"), Carlson ("Field to Finish") and
// 12d's field reduction all do it, and until this Katana had nothing.
//
// Three decisions the rest of this header builds on:
//
// 1. WHICH POINTS FORM ONE STRING. The first token of a point's code is its
//    STRING NAME: every point named "WM01" is one water main, every point named
//    "WM02" another. `splitStringName` says how that name divides into the
//    rule key it is coded by ("WM*") and a string number ("01"). The split is
//    reported rather than used for grouping: two names are two strings whatever
//    they split into, because "WM01" and "WM1" are two strings in the field.
//
// 2. CONTROL CODES ARE DATA. A 12d mapfile has no start/end/close codes - 12d
//    strings by name alone - so what "ST" means is not in any file Katana
//    reads. `LineworkCodes` holds the spellings, with defaults that are common
//    field conventions and nothing more. They are configurable, and they are
//    NOT 12d mapfile data.
//
// 3. NOTHING IS DROPPED SILENTLY. Every point that is not in a line is in the
//    report with the reason, and every token that was not understood is in the
//    report with the point it was on. A point left out of a kerb line looks
//    exactly like a kerb that was surveyed short.
//
// What the model can carry, and so what this builds:
//   * `Polyline2` is straight segments only - it has no bulge or arc segment.
//     A curve ("BC" ... "EC") is therefore CHORDED: densified into straight
//     segments whose sagitta (the gap between chord and arc) is at most
//     `LineworkOptions::chordTolerance`. The report counts the curves so
//     chorded.
//   * A polyline has no Z of its own. Heights are kept per vertex in the
//     `elevations` property through `entity::setHeights`, the one writer the
//     12d import and the survey import share; a point with no height gives its
//     vertex no height (absent is not zero).
//
// The lines are styled by CALLING `applySurveyCodes` on them, when the command
// runs, rather than by a second copy of its naming here: a line coded "WM01"
// then wears exactly the style a point coded "WM01" would, and a change to how
// that style is chosen reaches both at once.

#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/cad/survey_import.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

// ---- string names -------------------------------------------------------------

// How a string name divides into the rule that codes it and a string number.
//
// THE RULE: the key of the most specific rule that matches the name decides
// it. A prefix key "WM*" takes its prefix ("WM") and leaves the remainder as
// the number, so "WM01" is key "WM*", number "01". An exact key ("PABB") is
// the whole name, so there is no number: one string per code. "WM" under "WM*"
// likewise has no number. When the most specific rule is the bare "*" the name
// is FALLBACK-ONLY (audit CAD-05's word): nothing specific knows it, so it is
// its own key with no number rather than "*" plus the whole name as a
// "number". No rule at all gives an empty key and `matched` false.
//
// "Most specific" is SurveyMap::match's order - exact, then the longest
// prefix - so this can never disagree with the rule that styles the line.
struct StringName {
    std::string key{};    // "WM*", "PABB", "*" or empty
    std::string number{}; // "01", or empty
    bool matched = false;      // some rule matched, "*" included
    bool fallbackOnly = false; // ... and it was only "*"
};

[[nodiscard]] StringName splitStringName(const katana::entity::SurveyMap& map,
                                         std::string_view name);

// ---- control codes --------------------------------------------------------------

// The spellings of the control codes a point's code may carry after its string
// name: "KB1 ST", "FL CL", "KB1 BC", "FN3 JPN 105".
//
// CONFIGURABLE, NOT 12D MAPFILE DATA. The defaults are spellings common in
// field practice, chosen so that an untouched table does something sensible;
// no product's file was copied for them:
//   ST   start   - "start", the usual two-letter field abbreviation
//   END  end     - written out in most code sets
//   CL   close   - "close", as in "FL CL"
//   BC   arcStart, EC arcEnd - "begin curve" / "end curve", the road-design
//                  abbreviations surveyors already use for tangent points
//   JPN  join    - "join point number": also draw a line from this point to
//                  the point whose number follows ("JPN 105")
//   RECT rectangle - "rectangle": three points make a rectangle (see below)
// An empty spelling switches that control off.
struct LineworkCodes {
    std::string start = "ST";
    std::string end = "END";
    std::string close = "CL";
    std::string arcStart = "BC";
    std::string arcEnd = "EC";
    std::string join = "JPN";
    std::string rectangle = "RECT";
};

// Refuses a spelling containing a blank (a code is split on blanks, so it
// could never be matched) and two controls spelled alike (a token that means
// two things means neither). Spellings are compared ignoring ASCII case,
// because tokens are matched that way - see `parseFieldCode`.
[[nodiscard]] katana::core::Status validate(const LineworkCodes& codes);

enum class LineworkControl { Start, End, Close, ArcStart, ArcEnd, Join, Rectangle };

[[nodiscard]] std::string_view toString(LineworkControl control);

// A point's code taken apart.
struct FieldCode {
    std::string name{}; // the first token: the string name, "KB1"
    std::vector<LineworkControl> controls{}; // in the order written
    // The point number after the join token; empty when there was none (which
    // is itself reported as a note, not ignored).
    std::string joinTo{};
    bool joinWithoutTarget = false;
    // Every token after the name that is neither a control nor the join's
    // point number, as written. Reported, never silently ignored.
    std::vector<std::string> unknownTokens{};

    [[nodiscard]] bool has(LineworkControl control) const;
};

// Splits `code` on ASCII blanks. The FIRST token is always the string name,
// even when it is spelled like a control: a code "CL" is a string named CL.
// Controls after it are matched IGNORING ASCII CASE: they are keywords typed
// on a controller keypad, and "st" there means what "ST" means. The string
// name keeps its case, because the mapfile's keys are matched with case.
[[nodiscard]] FieldCode parseFieldCode(std::string_view code, const LineworkCodes& codes);

// ---- processing ------------------------------------------------------------------

enum class LineworkOrder {
    // By the point number property, numerically where the number is a number
    // ("9" before "10"); numbers before names that are not numbers, names in
    // byte order; ties in entity order. A point without a number is NOT placed
    // (it has no place in that order), and is reported.
    PointNumber,
    // By entity id: creation order, which for an imported survey is the order
    // the file listed the points - the order they were observed.
    EntityOrder,
};

struct LineworkOptions {
    // The property holding the code. EMPTY means find it, exactly as
    // applySurveyCodes does: the first of codePropertyCandidates() that any
    // point carries.
    std::string property{};
    // Where the point number is, as survey import writes it.
    std::string pointNumberProperty = "point";
    LineworkOrder order = LineworkOrder::PointNumber;
    // Only these entities, or every point in the drawing when empty. Entities
    // among them that are not points are counted, not processed.
    std::vector<katana::entity::EntityId> ids{};
    // Off: the points a run of their own string went into are deleted in the
    // same command - the line now stands for them. Points not in any line
    // are always kept, and so is a point only a join ("JPN") reached: a tree
    // or an uncoded control point joined to is not replaced by the join line.
    bool keepPoints = true;
    LineworkCodes codes{};
    // The largest gap, in model units, between a chord and the curve it stands
    // for. 5 mm: below the accuracy of a detail survey, so the chords are not
    // the error a person sees, and a 10 m radius needs only ~25 chords per
    // quarter turn.
    double chordTolerance = 0.005;
    // How the lines are styled: passed to applySurveyCodes with `property`
    // and `ids` replaced by the lines' own. Pass `colourOf` here as for
    // applySurveyCodes. `createLayers` also governs the layers the lines are
    // put on.
    SurveyCodingOptions coding{};
};

// Why a point is not in any line.
enum class UnplacedReason {
    NoCode,        // no code in the property
    NoRule,        // no rule for its name and no control code on it
    PointCode,     // its rules make it a point (breakline Point) or never say it is a line
    NoPointNumber, // ordering by number, and it has none
    LonePoint,     // the only point of its string: one point makes no line
    Coincident,    // every point of its string is in one place: no length to draw
};

[[nodiscard]] std::string_view toString(UnplacedReason reason);

struct UnplacedPoint {
    katana::entity::EntityId id = katana::entity::kInvalidEntityId;
    std::string pointNumber{}; // empty when it has none
    std::string code{};        // as the property holds it
    UnplacedReason reason = UnplacedReason::NoCode;
};

// Things worth telling that did not stop a point being placed.
enum class LineworkNoteKind {
    UnknownToken,       // a token that is neither a control nor a join target
    JoinWithoutTarget,  // "JPN" with no point number after it
    JoinTargetMissing,  // no point has that number (or several do)
    CurveTooShort,      // "BC" then "EC" with no point between: drawn straight
    CurveCollinear,     // three curve points in a straight line: drawn straight, not counted
    // "BC" with no "EC": taken to end at the string's last point, or - begun
    // on that point - nothing curved. The detail says which.
    CurveUnterminated,
    CurveEndWithoutStart, // "EC" with no open curve: nothing to end
    CloseTooShort,      // "CL" on a string of two points: drawn open
    // "RECT" on other than three points with width, so no rectangle is
    // constructed and the points are drawn as they are: closed through them
    // when there are more than three, open when there are two or when the
    // third lies on the first side's line. The detail says which.
    RectangleShape,
    DuplicatePointNumber, // two points of one string share a number
    NoRuleForName,      // a line by control code alone: nothing styles it
    FallbackOnlyName,   // a line only because the bare "*" rule says so
    // drawSurveyFeatures only: a feature names a point the file gives no
    // coordinates for (survey::UnpositionedPoint). Left out of the line - it
    // has nowhere to be - and said so, because the line then runs straight
    // past a point the surveyor strung into it.
    UnpositionedPoint,
};

[[nodiscard]] std::string_view toString(LineworkNoteKind kind);

struct LineworkNote {
    katana::entity::EntityId id = katana::entity::kInvalidEntityId;
    std::string pointNumber{};
    LineworkNoteKind kind = LineworkNoteKind::UnknownToken;
    std::string detail{}; // the token, the missing number, the string name
};

// One line built.
struct LineworkString {
    std::string name{};   // "WM01"
    std::string key{};    // "WM*" - see StringName
    std::string number{}; // "01"
    std::vector<katana::entity::EntityId> points{}; // in the order joined
    std::vector<std::string> pointNumbers{};         // the same, by number
    bool closed = false;
    bool rectangle = false;
    bool join = false;            // a "JPN" line rather than a run of the string
    std::size_t curves = 0;       // curves chorded into it
    std::size_t vertices = 0;     // vertices of the polyline, chords included
    std::string layer{};
};

struct LineworkReport {
    std::string property{}; // the code property actually read
    std::size_t considered = 0; // point entities looked at
    std::size_t notPoints = 0;  // entities asked about that are not points
    std::vector<LineworkString> strings{}; // by name, then in the order built
    std::vector<UnplacedPoint> unplaced{}; // in entity order
    std::vector<LineworkNote> notes{};
    std::vector<std::string> layersCreated{}; // in name order
    // Points deleted because a run of their string replaced them (keepPoints
    // off); join targets no run placed are never among them.
    std::size_t pointsRemoved = 0;
    // What applySurveyCodes did to the lines. It runs when the command first
    // executes - it cannot plan styles for lines that do not exist yet - so
    // this is empty until then, and filled by that execution.
    std::shared_ptr<const SurveyCodingReport> styling{};
};

struct LineworkResult {
    // ONE undoable command: layers, lines, their styling and (keepPoints off)
    // the removal of the points they replace. nullptr when there is nothing to
    // build - "nothing to do" and "failed" stay distinguishable, the contract
    // applySurveyCodes and importSurveyProject follow.
    katana::commands::CommandPtr command{};
    LineworkReport report{};
};

// Plans the linework for `document`. Fails on invalid options (control codes
// spelled ambiguously, a tolerance that is not a positive finite number) or a
// rule model that is not a valid layer name - never on a point it cannot
// place, which is reported instead.
//
// The command must be executed on THIS document: it styles the lines by
// planning applySurveyCodes against it when it runs, and refuses any other.
//
// Only a point is a candidate: its code's resolved breakline is Line, or it
// carries a control code. Its string name groups it; the order orders it;
// "ST" begins a new line at it, "END" ends the line at it, "CL" ends and
// closes, "RECT" ends it as a rectangle. The line goes on the rule's model
// (12d's model is Katana's layer), or the first point's layer when the rule
// names none.
//
// Curves. "BC" begins a curve at a point and "EC" ends it; each consecutive
// three curve points define an arc ((p0,p1,p2), (p2,p3,p4), ...), and when one
// segment is left over it lies on the arc through the last three. Two points
// cannot define a curve and are joined straight, with a note.
//
// Rectangles. Three points p0, p1, p2 with "RECT" make the closed rectangle
// with side p0-p1, reaching p2's side of it by p2's distance from that side.
// The two constructed corners have no height: they were not surveyed.
[[nodiscard]] katana::core::Result<LineworkResult>
processLinework(const Document& document, const LineworkOptions& options);

// ---- survey features --------------------------------------------------------------

// Some field files string their points themselves - a LandXML PlanFeature, a
// controller's line record - and those arrive as survey::SurveyFeature: a
// code, the point ids in observation order, and whether it closes. There the
// FILE has said which points are one line, so this draws exactly that, and
// the code table decides only where each line goes and how it looks, as it
// does for processLinework's lines (the same layer choice, the same styling
// by applySurveyCodes when the command runs).
//
// Use this OR processLinework on one set of points, not both: a kerb the file
// strung and whose points are also coded KB1 would be drawn twice.
struct SurveyFeatureOptions {
    // How the project was imported: the code and description properties the
    // lines carry (import.codeProperty, import.descriptionProperty), and the
    // layer its points went on (layerForPoint), which is where a line goes
    // when its code names no model - with its points, as processLinework's
    // does. Pass the options the import was given. import.createLayers is NOT
    // read: coding.createLayers governs every layer here, as it does there.
    // An EMPTY import.codeProperty (the import wrote no code) still gives each
    // line its code, under codePropertyCandidates().front(), which the report
    // names: the rules style a line by the code it carries.
    SurveyImportOptions import{};
    // As LineworkOptions::coding: how the lines are styled, and whether a
    // missing layer may be created.
    SurveyCodingOptions coding{};
};

// Why a feature is not drawn.
enum class UnplacedFeatureReason {
    TooFewPoints, // fewer than two of its points have a position: no line
    Coincident,   // its positioned points are all in one place: no length to draw
};

[[nodiscard]] std::string_view toString(UnplacedFeatureReason reason);

struct UnplacedFeature {
    std::size_t index = 0; // in SurveyProject::features
    std::string name{};
    std::string code{};
    UnplacedFeatureReason reason = UnplacedFeatureReason::TooFewPoints;
};

struct SurveyFeatureResult {
    // ONE undoable command: the layers and the lines, with their styling;
    // nullptr when no feature makes a line.
    katana::commands::CommandPtr command{};
    // One LineworkString per feature drawn, in the project's order: `name` is
    // the feature's name, or its code when it has none; `key` and `number`
    // split its CODE (the code is what the rules are keyed on and what the
    // line carries); `points` is EMPTY, because the points are the project's
    // and need not be in the drawing, and `pointNumbers` holds the project's
    // point ids in the order joined. Also the notes (a point without a
    // position, a close on two points, a code no rule knows), layersCreated
    // and styling. `considered`, `notPoints`, `unplaced` and `pointsRemoved`
    // are about point entities and stay empty.
    LineworkReport report{};
    std::vector<UnplacedFeature> unplaced{}; // in the project's order
};

// Plans the lines of `project`'s features on `document`, from the project's
// own coordinates (a SurveyPoint is (easting, northing) in the drawing, as
// importSurveyProject puts it, and its height is kept per vertex, absent
// staying absent). Straight segments only: a feature carries no curve, so the
// control codes processLinework reads are not looked for here.
//
// Fails with survey::validateProject's error for an inconsistent project, as
// importSurveyProject does; with InvalidArgument for a rule model that is not a
// valid layer name, or with NotFound for a layer the drawing lacks when
// coding.createLayers is off and neither the rule's model nor the points'
// layer exists (a line put on some other layer would look right and be wrong).
// Never fails on a feature it cannot draw: that is in `unplaced`.
//
// The command must be executed on THIS document, as processLinework's.
[[nodiscard]] katana::core::Result<SurveyFeatureResult>
drawSurveyFeatures(const Document& document, const katana::survey::SurveyProject& project,
                   const SurveyFeatureOptions& options);

} // namespace katana::cad
