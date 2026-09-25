#include "katana/cad/survey_code_verbs.hpp"

#include <algorithm>
#include <string>
#include <utility>
#include <vector>

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

katana::core::Status requireMap(const Document& document)
{
    if (document.surveyMap().empty()) {
        return makeError(ErrorCode::InvalidState,
                         "no survey codes are loaded; use CUSTOMISE <file> first");
    }
    return {};
}

// CODE [<property>] | CODE EXPLAIN <code> | CODE CENSUS [<property>].
Result<std::string> code(Document& document, const Words& args, const ColourLookup& colourOf)
{
    if (auto status = requireMap(document); !status) {
        return status.error();
    }
    const std::string word = args.empty() ? std::string{} : args.front();
    if (is(word, "EXPLAIN")) {
        if (args.size() < 2) {
            return usage("CODE EXPLAIN <code>");
        }
        const CodeExplanation explanation = explainCode(
            document.surveyMap(), restOf(args, 1),
            [&document](std::string_view name) { return document.definitionFor(name); },
            colourOf);
        return reply(formatCodeExplanation(explanation));
    }
    if (is(word, "CENSUS")) {
        return reply(formatCodeCensus(codeCensus(document, restOf(args, 1))));
    }

    SurveyCodingOptions options;
    options.property = restOf(args, 0);
    options.colourOf = colourOf;
    SurveyCodingReport report;
    auto command = applySurveyCodes(document, options, &report);
    if (!command) {
        return command.error();
    }
    std::string text = formatCodingReport(report);
    if (*command == nullptr) {
        return text + "Nothing to change.";
    }
    if (auto status = document.execute(std::move(*command)); !status) {
        return status.error();
    }
    return text + "Applied as one command. UNDO puts it all back.";
}

// MAPFILE LIST [<filter>] | MAPFILE CHECK.
Result<std::string> mapfile(const Document& document, const Words& args,
                            const ColourLookup& colourOf)
{
    const std::string word = args.empty() ? std::string{} : args.front();
    const bool list = is(word, "LIST");
    if (!list && !(is(word, "CHECK") && args.size() == 1)) {
        return usage("MAPFILE LIST [<filter>] | MAPFILE CHECK");
    }
    if (auto status = requireMap(document); !status) {
        return status.error();
    }
    const katana::entity::SurveyMap& map = document.surveyMap();
    if (list) {
        return reply(formatCodeTable(codeTable(map), restOf(args, 1)));
    }
    return mapfileCheckReply(
        lintSurveyMap(map, document.styleLibrary(), colourOf,
                      [](std::string_view name) { return katana::entity::isBuiltInSymbolName(name); }),
        map.size());
}

} // namespace

bool isSurveyCodeVerb(std::string_view verb)
{
    return katana::core::equalsIgnoringCase(verb, "CODE") ||
           katana::core::equalsIgnoringCase(verb, "MAPFILE");
}

Result<std::string> runSurveyCodeVerb(Document& document, const std::vector<std::string>& tokens,
                                      const ColourLookup& colourOf)
{
    if (tokens.empty() || !isSurveyCodeVerb(tokens.front())) {
        return makeError(ErrorCode::InvalidArgument, "not a CODE or MAPFILE line");
    }
    const Words args(tokens.begin() + 1, tokens.end());
    return is(tokens.front(), "CODE") ? code(document, args, colourOf)
                                      : mapfile(document, args, colourOf);
}

Result<std::string> mapfileCheckReply(const std::vector<LintIssue>& issues, std::size_t rules)
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
