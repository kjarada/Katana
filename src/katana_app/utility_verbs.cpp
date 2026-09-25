#include "utility_verbs.hpp"

#include <cstddef>
#include <fstream>
#include <initializer_list>
#include <iostream>
#include <iterator>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/delivery_schema.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"
#include "katana/survey/subsurface/utility_report.hpp"
#include "katana/survey/subsurface/verification.hpp"

namespace katana::app {

namespace {

namespace sub = katana::survey::subsurface;

// Blank-separated words, a double-quoted word kept whole with its blanks.
// nullopt for a quote that is never closed.
std::optional<std::vector<std::string>> words(std::string_view text)
{
    std::vector<std::string> out;
    std::size_t at = 0;
    while (at < text.size()) {
        if (text[at] == ' ' || text[at] == '\t') {
            ++at;
            continue;
        }
        if (text[at] == '"') {
            const std::size_t end = text.find('"', at + 1);
            if (end == std::string_view::npos) {
                return std::nullopt;
            }
            out.emplace_back(text.substr(at + 1, end - at - 1));
            at = end + 1;
            continue;
        }
        const std::size_t end = text.find_first_of(" \t", at);
        out.emplace_back(text.substr(at, end == std::string_view::npos ? end : end - at));
        at = end == std::string_view::npos ? text.size() : end;
    }
    return out;
}

std::optional<std::string> readFile(const std::string& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

bool fail(const std::string& message)
{
    std::cerr << "error: " << message << '\n';
    return false;
}

std::optional<std::vector<sub::UtilityLine>> readSchedule(const std::string& path)
{
    const auto text = readFile(path);
    if (!text) {
        fail("NotFound: cannot read " + path);
        return std::nullopt;
    }
    auto lines = sub::parseUtilityCsv(*text);
    if (!lines) {
        fail(lines.error().describe() + " in " + path);
        return std::nullopt;
    }
    return std::move(lines).value();
}

// KEYWORD <number> pairs after the positional arguments. Each keyword may be
// given once; its number must be finite and not negative.
struct Options {
    std::vector<std::pair<std::string, double>> values;

    [[nodiscard]] std::optional<double> get(std::string_view name) const
    {
        for (const auto& [key, value] : values) {
            if (key == name) {
                return value;
            }
        }
        return std::nullopt;
    }
};

std::optional<Options> readOptions(const std::vector<std::string>& args, std::size_t first,
                                   std::initializer_list<std::string_view> allowed)
{
    Options options;
    for (std::size_t i = first; i < args.size(); i += 2) {
        std::string name;
        for (const std::string_view candidate : allowed) {
            if (core::equalsIgnoringCase(args[i], candidate)) {
                name = std::string(candidate);
            }
        }
        if (name.empty()) {
            fail("InvalidArgument: unknown option " + args[i]);
            return std::nullopt;
        }
        if (options.get(name)) {
            fail("InvalidArgument: " + name + " given twice");
            return std::nullopt;
        }
        const auto value =
            i + 1 < args.size() ? core::parseFiniteDouble(args[i + 1]) : std::nullopt;
        if (!value || *value < 0.0) {
            fail("InvalidArgument: " + name + " needs a number of metres, not negative");
            return std::nullopt;
        }
        options.values.emplace_back(name, *value);
    }
    return options;
}

bool report(const std::vector<std::string>& args)
{
    if (args.size() < 2) {
        return fail("InvalidArgument: usage: UTILITY REPORT <schedule.csv> [MINCOVER <m>] "
                    "[SPACING <m>]");
    }
    const auto options = readOptions(args, 2, {"MINCOVER", "SPACING"});
    const auto lines = options ? readSchedule(args[1]) : std::nullopt;
    if (!lines) {
        return false;
    }
    sub::GradingSettings settings;
    if (const auto spacing = options->get("SPACING")) {
        settings.maximumDetectedSpacing = *spacing;
    }
    const auto text = sub::renderInvestigationReport(*lines, settings, options->get("MINCOVER"));
    if (!text) {
        return fail(text.error().describe());
    }
    std::cout << *text;
    return true;
}

bool verify(const std::vector<std::string>& args)
{
    if (args.size() != 2) {
        return fail("InvalidArgument: usage: UTILITY VERIFY <schedule.csv>");
    }
    const auto lines = readSchedule(args[1]);
    if (!lines) {
        return false;
    }
    std::cout << sub::renderVerificationReport(sub::verifyDetections(*lines));
    return true;
}

bool clearance(const std::vector<std::string>& args)
{
    if (args.size() < 3) {
        return fail("InvalidArgument: usage: UTILITY CLEARANCE <schedule.csv> <design.csv> "
                    "[WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]");
    }
    const auto options = readOptions(args, 3, {"WIDTH", "H", "V", "MARGIN"});
    const auto lines = options ? readSchedule(args[1]) : std::nullopt;
    if (!lines) {
        return false;
    }
    const auto designText = readFile(args[2]);
    if (!designText) {
        return fail("NotFound: cannot read " + args[2]);
    }
    auto design = sub::parseDesignCsv(*designText, args[2]);
    if (!design) {
        return fail(design.error().describe() + " in " + args[2]);
    }
    design->halfWidth = options->get("WIDTH").value_or(0.0) / 2.0;
    sub::ClearanceRequirement requirement;
    requirement.horizontal = options->get("H").value_or(requirement.horizontal);
    requirement.vertical = options->get("V").value_or(requirement.vertical);
    requirement.unverifiedMargin = options->get("MARGIN").value_or(requirement.unverifiedMargin);

    const auto results = sub::checkClearance(*design, *lines, requirement);
    if (!results) {
        return fail(results.error().describe());
    }
    std::cout << sub::renderClearanceReport(*design, *results, requirement);
    return true;
}

// UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>: exit status 1 when the
// schedule has errors against the schema, so that a script can gate on it.
bool check(const std::vector<std::string>& args)
{
    if (args.size() != 4 || !core::equalsIgnoringCase(args[2], "SCHEMA")) {
        return fail("InvalidArgument: usage: UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>");
    }
    const auto schemaText = readFile(args[3]);
    if (!schemaText) {
        return fail("NotFound: cannot read " + args[3]);
    }
    const auto schema = sub::parseDeliverySchema(*schemaText);
    if (!schema) {
        return fail(schema.error().describe() + " in " + args[3]);
    }
    const auto scheduleText = readFile(args[1]);
    if (!scheduleText) {
        return fail("NotFound: cannot read " + args[1]);
    }
    const auto result = sub::checkDelivery(*scheduleText, *schema);
    if (!result) {
        return fail(result.error().describe() + " in " + args[1]);
    }
    std::cout << sub::renderSchemaCheck(*result, *schema);
    return result->count(sub::FindingSeverity::Error) == 0;
}

} // namespace

bool runUtilityVerb(std::string_view argument)
{
    const auto args = words(argument);
    if (!args) {
        return fail("InvalidArgument: a quoted path is never closed");
    }
    const std::string action = args->empty() ? std::string() : core::lowered((*args)[0]);
    if (action == "report") {
        return report(*args);
    }
    if (action == "verify") {
        return verify(*args);
    }
    if (action == "clearance") {
        return clearance(*args);
    }
    if (action == "check") {
        return check(*args);
    }
    return fail("InvalidArgument: usage: UTILITY REPORT | VERIFY | CLEARANCE | CHECK ... "
                "(katana_cli --help)");
}

const char* utilityHelpText()
{
    return "Utilities UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>]\n"
           "          grade located services by AS 5488 quality level: vertices,\n"
           "          segments, length at each level, depth of cover, findings\n"
           "          UTILITY VERIFY <schedule.csv>  QL-B detections against the\n"
           "          QL-A exposures that name them in the verifies column\n"
           "          UTILITY CLEARANCE <schedule.csv> <design.csv> [WIDTH <m>]\n"
           "          [H <m>] [V <m>] [MARGIN <m>]  clearance of proposed works\n"
           "          from each service, widened by its quality level's tolerance\n"
           "          UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>  the schedule\n"
           "          against a client's delivery schema: mandatory attributes and\n"
           "          their value lists (tools/utility_schema_domains.py makes the\n"
           "          schema file from a TfNSW Utility Schema workbook)\n";
}

} // namespace katana::app
