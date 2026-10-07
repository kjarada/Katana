#include "katana/cad/survey_code_verbs.hpp"

#include <algorithm>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using Words = std::vector<std::string>;

namespace {

constexpr const char* kApplyUsage =
    "CODE [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]";
constexpr const char* kCensusUsage =
    "CODE CENSUS [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]";

katana::core::Error usage(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

bool is(const std::string& word, std::string_view name)
{
    return katana::core::equalsIgnoringCase(word, name);
}

// The words from `from` on, joined by one blank: a code, a property or a
// filter, which the command line typed as the rest of the line.
std::string restOf(const Words& words, std::size_t from)
{
    std::string text;
    for (std::size_t i = from; i < words.size(); ++i) {
        text += (i > from ? " " : "") + words[i];
    }
    return text;
}

// The formatters end every line with '\n'; a reply does not, since each front
// end ends it (katana_cli prints one, the window's log is a line a message).
std::string reply(std::string text)
{
    while (!text.empty() && text.back() == '\n') {
        text.pop_back();
    }
    return text;
}

// The remedy names the file that DOES load: "use CUSTOMISE <file>" once meant
// a survey code file, and sent people to load a kind of file that is gone.
katana::core::Status requireMap(const Document& document)
{
    if (document.surveyMap().empty()) {
        return makeError(ErrorCode::InvalidState,
                         "no survey codes are loaded; CUSTOMISE <file> loads a Katana "
                         "customisation file");
    }
    return {};
}

// What the words after CODE, or after CODE CENSUS, ask for.
struct CodeRequest {
    ScopeWords scope{};
    std::string property{}; // empty: found, as applySurveyCodes finds it
    bool preview = false;
};

// Whether `words[at]` begins the scope form rather than being the property
// the line names by position (survey_code_verbs.hpp, "the first word"). A
// word that needs the word after it and has none cannot begin anything:
// "CODE Layer" is the property Layer.
bool beginsScopeForm(const Words& words, std::size_t at)
{
    const std::string& word = words[at];
    const bool followed = at + 1 < words.size();
    if (is(word, "PREVIEW")) {
        return true;
    }
    if (is(word, "PROPERTY") || is(word, "LAYER") || is(word, "LAYERS") || is(word, "AREA")) {
        return followed;
    }
    return isScopeWord(word);
}

// [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW], the last two in either
// order and on either side of the scope. The scope and its filter are the
// shared parser's; with no scope word it is the DRAWING, a bare WHERE
// included, which is this verb's own default. ONE grammar for CODE and for
// CODE CENSUS: PREVIEW is read for both, and a census, which changes nothing,
// has nothing to do for it.
Result<CodeRequest> parseRequest(const Words& words, std::size_t at, const char* usageText)
{
    CodeRequest request;
    request.scope.source = ScopeSource::Drawing;
    if (at < words.size() && !beginsScopeForm(words, at)) {
        // The property by position, as CODE has always taken it: the whole
        // rest of the line, so a name of two words needs no quotes.
        request.property = restOf(words, at);
        return request;
    }
    bool scoped = false;
    bool named = false;
    while (at < words.size()) {
        const std::string& word = words[at];
        if (is(word, "PROPERTY")) {
            if (named || at + 1 >= words.size()) {
                return usage(usageText);
            }
            request.property = words[at + 1];
            named = true;
            at += 2;
            continue;
        }
        if (is(word, "PREVIEW")) {
            request.preview = true;
            ++at;
            continue;
        }
        if (!scoped && isScopeWord(word)) {
            const bool bareWhere = is(word, "WHERE");
            auto scope = parseScopeWords(words, at);
            if (!scope) {
                return scope.error();
            }
            request.scope = std::move(*scope);
            if (bareWhere) {
                // The shared parser reads no scope word as the selection.
                request.scope.source = ScopeSource::Drawing;
            }
            scoped = true;
            continue;
        }
        return usage(usageText);
    }
    return request;
}

// CODE [<scope>] ... [PREVIEW]: apply the loaded codes to what the scope took.
Result<std::string> apply(Document& document, const Words& args, const ColourLookup& colourOf,
                          const ScopeViewProvider& views)
{
    const auto request = parseRequest(args, 0, kApplyUsage);
    if (!request) {
        return request.error();
    }
    const auto match = matchScope(document, request->scope, views);
    if (!match) {
        return match.error();
    }
    std::string text = scopeRecord(*match) + "\n";

    SurveyCodingReport report;
    katana::commands::CommandPtr command;
    if (match->matched.empty()) {
        // A scope that took nothing is reported, not refused - and is NOT
        // handed on: an empty id list means every entity to applySurveyCodes.
        report.property = request->property.empty()
                              ? findCodeProperty(document.model(), {})
                              : request->property;
    } else {
        SurveyCodingOptions options;
        options.property = request->property;
        options.ids = match->matched;
        options.colourOf = colourOf;
        auto planned = applySurveyCodes(document, options, &report);
        if (!planned) {
            return planned.error();
        }
        command = std::move(*planned);
    }
    text += formatCodingReport(report);
    if (command == nullptr) {
        return text + "Nothing to change.";
    }
    if (request->preview) {
        return text + "Preview: nothing was changed.";
    }
    if (auto status = document.execute(std::move(command)); !status) {
        return status.error();
    }
    return text + "Applied as one command. UNDO puts it all back.";
}

// CODE CENSUS [<scope>] ...: the codes what the scope took carries. PREVIEW
// is taken and asks nothing more of it: the two forms are one grammar, and a
// line built for CODE with CENSUS put in must not turn into a usage error for
// the one word that promises to change nothing.
Result<std::string> census(const Document& document, const Words& args,
                           const ScopeViewProvider& views)
{
    const auto request = parseRequest(args, 1, kCensusUsage);
    if (!request) {
        return request.error();
    }
    const auto match = matchScope(document, request->scope, views);
    if (!match) {
        return match.error();
    }
    return scopeRecord(*match) + "\n" +
           reply(formatCodeCensus(codeCensus(document, match->matched, request->property)));
}

Result<std::string> code(Document& document, const Words& args, const ColourLookup& colourOf,
                         const ScopeViewProvider& views)
{
    const std::string word = args.empty() ? std::string{} : args.front();
    // CHECK takes no word: one given is a mistake, said before anything else.
    if (is(word, "CHECK") && args.size() != 1) {
        return usage("CODE CHECK");
    }
    if (is(word, "EXPLAIN") && args.size() < 2) {
        return usage("CODE EXPLAIN <code>");
    }
    if (auto status = requireMap(document); !status) {
        return status.error();
    }
    const katana::entity::SurveyMap& map = document.surveyMap();
    if (is(word, "EXPLAIN")) {
        const CodeExplanation explanation = explainCode(
            map, restOf(args, 1),
            [&document](std::string_view name) { return document.definitionFor(name); },
            colourOf);
        return reply(formatCodeExplanation(explanation));
    }
    if (is(word, "CENSUS")) {
        return census(document, args, views);
    }
    if (is(word, "LIST")) {
        return reply(formatCodeTable(codeTable(map), restOf(args, 1)));
    }
    if (is(word, "CHECK")) {
        return codeCheckReply(
            lintSurveyMap(map, document.styleLibrary(), colourOf,
                          [](std::string_view name) {
                              return katana::entity::isBuiltInSymbolName(name);
                          }),
            map.size());
    }
    return apply(document, args, colourOf, views);
}

} // namespace

bool isSurveyCodeVerb(std::string_view verb)
{
    return katana::core::equalsIgnoringCase(verb, "CODE");
}

Result<std::string> runSurveyCodeVerb(Document& document, const std::vector<std::string>& args,
                                      const ScopeViewProvider& views)
{
    // The Document's own answer and no other: its customisation's table, then
    // the standard names, asked at each call so that it follows a
    // customisation loaded since (colourLookup).
    return code(document, args, colourLookup(document), views);
}

Result<std::string> codeCheckReply(const std::vector<LintIssue>& issues, std::size_t rules)
{
    std::string text = reply(formatLint(issues, rules));
    const auto errors = static_cast<std::size_t>(
        std::count_if(issues.begin(), issues.end(),
                      [](const LintIssue& issue) { return issue.severity == LintSeverity::Error; }));
    if (errors != 0) {
        return makeError(ErrorCode::InvalidState,
                         "the loaded survey codes have " + std::to_string(errors) +
                             (errors == 1 ? " error" : " errors") +
                             ": a rule cannot be applied as written\n" + text);
    }
    return text;
}

} // namespace katana::cad
