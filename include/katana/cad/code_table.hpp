#pragma once

// The survey code library as something a person can question: why a code
// gets what it gets, the map as a table of codes, which codes a drawing
// actually carries, and what is wrong with a map before it is applied.
//
// These are the foundations of the survey-code manager, and of the CLI verbs
// CODE EXPLAIN, CODE CENSUS, MAPFILE LIST and MAPFILE CHECK. They are here,
// below both front ends, so that the GUI and the CLI print the SAME thing:
// the two used to disagree about what applying codes had done.
//
// Library definitions and colours come in through callbacks, as they do for
// applySurveyCodes: the standard colour names are known to archive12d, which cad may
// not see, and a caller holding a Document passes definitionFor.

#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

// The library definition of a name, or nullptr (Document::definitionFor).
using DefinitionLookup = std::function<const katana::entity::LineStyle*(std::string_view)>;
// A standard colour name's RGB, or nullopt for a name it does not know.
using ColourLookup = std::function<std::optional<katana::entity::Color>(std::string_view)>;
// Whether Katana draws a symbol of this name without a library
// (entity::isBuiltInSymbolName).
using BuiltInSymbolTest = std::function<bool(std::string_view)>;

// ---- explaining one code ------------------------------------------------------

// Where one field of a code's combined result came from: the FIRST rule, most
// specific first, that says anything about it - the rule SurveyMap::lookup
// takes it from.
struct CodeFieldSource {
    // "model", "colour", "linestyle", "weight", "group", "comment",
    // "breakline", "tinable", "hide", "symbol", "text style", "pipe",
    // "vertex pipe" or "segment pipe".
    std::string field{};
    std::string value{}; // as text, for a person to read
    std::size_t rule = 0; // its index in SurveyMap::rules()
    std::string key{};
    katana::entity::SurveySection section = katana::entity::SurveySection::Map;
    // Later matching rules that set this field to something ELSE and lose -
    // most often the second of two loaded mapfiles disagreeing (the built-in
    // pair differ on `hide` for 190 keys). Most specific first.
    struct Overruled {
        std::size_t rule = 0;
        std::string key{};
        katana::entity::SurveySection section = katana::entity::SurveySection::Map;
        std::string value{};
    };
    std::vector<Overruled> overruled{};
};

// What a linestyle or symbol name resolves to.
struct CodeDefinition {
    std::string name{};       // as the rules give it; empty when they give none
    bool plain = false;       // the plain continuous line: "0", "1", "continuous"
    bool defined = false;     // the library has a definition of this name
    bool vertexMode = false;  // ... and it is `mode vertex`
    bool builtIn = false;     // a symbol no library defines but Katana draws itself
};

struct CodeColour {
    std::string name{};
    // Empty with a name: a colour name nothing here knows, which leaves the
    // entity's colour alone when codes are applied.
    std::optional<katana::entity::Color> rgb{};
};

struct CodeAttribute {
    std::string scope{}; // "string", "vertex" or "segment" - which list it is in
    katana::entity::SurveyAttribute attribute{};
    // Its value names another attribute ("$PipeDiameter"), which needs the
    // survey data the drawing came from: it is counted and left, not set.
    bool deferred = false;
    std::size_t rule = 0;
    std::string key{};
    katana::entity::SurveySection section = katana::entity::SurveySection::Map;
};

// A key the code would have met but for letter case or surrounding blanks -
// the reason "wm01" gets nothing from the rule for "WM*", which no rule will
// say for itself: matching is byte for byte.
enum class NearMissKind { Case, Whitespace };

[[nodiscard]] const char* toString(NearMissKind kind);

struct CodeNearMiss {
    std::string key{};
    NearMissKind kind = NearMissKind::Case;
};

struct CodeExplanation {
    std::string code{};
    katana::entity::SurveyMatchKind kind = katana::entity::SurveyMatchKind::None;
    bool matched = false; // SurveyMatch::matched(), decision D5
    katana::entity::SurveyRule resolved{}; // exactly what SurveyMap::lookup gives
    std::vector<std::size_t> rules{};      // every rule that matched, most specific first
    std::vector<CodeFieldSource> fields{}; // the fields the combination sets, in the order above
    CodeDefinition linestyle{};
    CodeDefinition symbol{};
    CodeColour colour{};
    std::vector<CodeAttribute> attributes{}; // as combined: string, then vertex, then segment
    std::vector<CodeNearMiss> nearMisses{};  // in key order
};

[[nodiscard]] CodeExplanation explainCode(const katana::entity::SurveyMap& map,
                                          std::string_view code,
                                          const DefinitionLookup& definitionOf,
                                          const ColourLookup& colourOf);

// ---- the map as a table of codes ------------------------------------------------

struct CodeTableRow {
    std::string key{};
    // The key's own shape: Exact, Prefix, or FallbackOnly for the bare `*`.
    katana::entity::SurveyMatchKind kind = katana::entity::SurveyMatchKind::Exact;
    std::vector<std::size_t> rules{}; // the rules with exactly this key, in map order
    std::vector<katana::entity::SurveySection> sections{}; // theirs, distinct, in section order
    // What a code this key catches resolves to, the less specific keys that
    // also catch it included: lookup(key) - for "WM*" that is WM*, then W*,
    // then `*`.
    katana::entity::SurveyRule combined{};
};

// One row per distinct key, in key order.
[[nodiscard]] std::vector<CodeTableRow> codeTable(const katana::entity::SurveyMap& map);
// Whether a row is one a person searching for `filter` means: a substring of
// its key, comment, group, model, colour, linestyle or symbol, with case
// ignored - searching folds case, storing never does (decision D3). An empty
// filter matches every row.
[[nodiscard]] bool codeTableRowMatches(const CodeTableRow& row, std::string_view filter);

// ---- the codes a drawing carries ------------------------------------------------

struct CodeCensusRow {
    std::string code{};
    std::size_t entities = 0;
    katana::entity::SurveyMatchKind kind = katana::entity::SurveyMatchKind::None;
    bool matched = false;
    std::string model{}; // what the combined rule gives, empty for none
};

struct CodeCensus {
    std::string property{}; // what was asked for, or what was found
    std::size_t coded = 0;  // entities carrying a code
    std::vector<CodeCensusRow> codes{}; // one per distinct code, in code order
};

// Every distinct code the drawing's entities carry under `property` (empty:
// found as applySurveyCodes finds it), counted, and classed against the
// loaded map. Each distinct code is looked up once.
[[nodiscard]] CodeCensus codeCensus(const Document& document, std::string_view property = {});

// ---- what is wrong with a map -----------------------------------------------------

enum class LintSeverity {
    Error,   // the rule cannot be applied as written
    Warning, // it applies, but not as its author meant
};

enum class LintKind {
    UnresolvedLinestyle,    // a linestyle no loaded library defines
    UnresolvedSymbol,       // a symbol no library defines and Katana cannot draw
    SymbolNotSymbolCapable, // a symbol rule naming a definition known not to be a symbol
    LinestyleIsVertex,      // a linestyle naming a `mode vertex` definition
    UnknownColour,          // a colour name the colour table does not know
    InvalidLayerPath,       // a model that cannot become a layer
    NoModel,                // a map_data rule that puts its code nowhere
    DuplicateRule,          // the same as an earlier rule, field for field
    ShadowedRule,           // earlier rules with its key already say all it says
    KeyWhitespace,          // a key with surrounding blanks, which matches no code typed
};

[[nodiscard]] const char* toString(LintSeverity severity);
[[nodiscard]] const char* toString(LintKind kind);

struct LintIssue {
    std::size_t rule = 0; // index in SurveyMap::rules()
    std::string key{};
    katana::entity::SurveySection section = katana::entity::SurveySection::Map;
    LintSeverity severity = LintSeverity::Warning;
    LintKind kind = LintKind::UnresolvedLinestyle;
    std::string message{};
};

// What is wrong with ONE rule on its own - everything but DuplicateRule and
// ShadowedRule, which need the rest of the map. For an editor's form before
// the rule is committed: SurveyMap refuses a rule validate() refuses, so
// KeyWhitespace and InvalidLayerPath can only be seen here, on a rule not
// yet in a map. `index` is what the issues cite.
//
// A symbol is taken to be symbol-capable when it is `mode vertex` or was read
// from a file whose name contains "symbol" (decision D3); one read from
// somewhere unknown (LineStyle::source empty) is given the benefit of the
// doubt, because most symbols the reference mapfiles use are not `mode
// vertex`. `colourOf` and `isBuiltInSymbol` may be empty: then no colour is
// checked, and no symbol name is excused as built in.
[[nodiscard]] std::vector<LintIssue> lintSurveyRule(const katana::entity::SurveyRule& rule,
                                                    std::size_t index,
                                                    const katana::entity::StyleLibrary& library,
                                                    const ColourLookup& colourOf,
                                                    const BuiltInSymbolTest& isBuiltInSymbol);

// Every issue in the map, by rule index and then in the order of LintKind:
// lintSurveyRule for each rule, and the rules that can never contribute.
[[nodiscard]] std::vector<LintIssue> lintSurveyMap(const katana::entity::SurveyMap& map,
                                                   const katana::entity::StyleLibrary& library,
                                                   const ColourLookup& colourOf,
                                                   const BuiltInSymbolTest& isBuiltInSymbol);

// ---- one report text for every front end ------------------------------------------
//
// Plain text, lines ending in '\n'. The CLI prints them as they are; a dialog
// shows the same words.

[[nodiscard]] std::string formatCodingReport(const SurveyCodingReport& report);
[[nodiscard]] std::string formatCodeExplanation(const CodeExplanation& explanation);
[[nodiscard]] std::string formatCodeTable(const std::vector<CodeTableRow>& rows,
                                          std::string_view filter = {});
[[nodiscard]] std::string formatCodeCensus(const CodeCensus& census);
[[nodiscard]] std::string formatLint(const std::vector<LintIssue>& issues, std::size_t rules);
[[nodiscard]] std::string formatCoverage(const CustomisationCoverage& coverage);

} // namespace katana::cad
