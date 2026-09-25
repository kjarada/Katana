#include "katana/cad/utilities/utility_verbs.hpp"

#include <cstddef>
#include <filesystem>
#include <fstream>
#include <initializer_list>
#include <iterator>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/delivery_schema.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"
#include "katana/survey/subsurface/utility_report.hpp"
#include "katana/survey/subsurface/verification.hpp"

namespace katana::cad::utilities {

namespace sub = katana::survey::subsurface;
using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using Words = std::vector<std::string>;

namespace {

Error usage(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

// A path typed on the command line is UTF-8 text; on Windows a narrow string
// would be read in the ANSI code page instead. Only for bytes that ARE UTF-8:
// converting any others throws, and nothing up the call chain catches it.
std::filesystem::path pathFrom(std::string_view utf8)
{
    std::u8string text;
    text.reserve(utf8.size());
    for (const char c : utf8) {
        text += static_cast<char8_t>(c);
    }
    return std::filesystem::path(text);
}

// The window's lines are always UTF-8. katana_cli's arguments, its console and
// a script saved from an ANSI editor on Windows arrive in the ANSI code page,
// where "café" is not UTF-8; those bytes are opened as the narrow name they
// are, which the C runtime reads in that code page - as the verbs did before
// they moved into the interpreter.
std::ifstream openFile(const std::string& path)
{
    if (core::isValidUtf8(path)) {
        return std::ifstream(pathFrom(path), std::ios::binary);
    }
    return std::ifstream(path, std::ios::binary);
}

Result<std::string> readFile(const std::string& path)
{
    std::ifstream file = openFile(path);
    if (!file) {
        return makeError(ErrorCode::NotFound, "cannot read " + path);
    }
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

// A reader's error with the file it was reading named, so a batch that reads
// three files says which one was wrong.
Error inFile(Error error, const std::string& path)
{
    error.context = error.context.empty() ? path : error.context + " in " + path;
    return error;
}

Result<std::vector<sub::UtilityLine>> readSchedule(const std::string& path)
{
    const auto text = readFile(path);
    if (!text) {
        return text.error();
    }
    auto lines = sub::parseUtilityCsv(*text);
    if (!lines) {
        return inFile(lines.error(), path);
    }
    return std::move(lines).value();
}

// A report as a reply: the interpreter's replies end without a line break,
// and the front end adds one, so the text printed is the report exactly.
std::string reply(std::string text)
{
    while (!text.empty() && text.back() == '\n') {
        text.pop_back();
    }
    return text;
}

// KEYWORD <value> pairs after the positional arguments, each keyword at most
// once. A number must be finite and not negative; LAYER takes a word.
struct Options {
    std::vector<std::pair<std::string, double>> numbers;
    std::optional<std::string> layer;

    [[nodiscard]] std::optional<double> get(std::string_view name) const
    {
        for (const auto& [key, value] : numbers) {
            if (key == name) {
                return value;
            }
        }
        return std::nullopt;
    }
};

Result<Options> readOptions(const Words& args, std::size_t first,
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
            return makeError(ErrorCode::InvalidArgument, "unknown option " + args[i]);
        }
        if (options.get(name) || (name == "LAYER" && options.layer)) {
            return makeError(ErrorCode::InvalidArgument, name + " given twice");
        }
        if (name == "LAYER") {
            if (i + 1 >= args.size()) {
                return makeError(ErrorCode::InvalidArgument, "LAYER needs a layer name");
            }
            options.layer = args[i + 1];
            continue;
        }
        const auto value =
            i + 1 < args.size() ? core::parseFiniteDouble(args[i + 1]) : std::nullopt;
        if (!value || *value < 0.0) {
            return makeError(ErrorCode::InvalidArgument,
                             name + " needs a number of metres, not negative");
        }
        options.numbers.emplace_back(name, *value);
    }
    return options;
}

sub::GradingSettings grading(const Options& options)
{
    sub::GradingSettings settings;
    if (const auto spacing = options.get("SPACING")) {
        settings.maximumDetectedSpacing = *spacing;
    }
    return settings;
}

Result<std::string> report(const Words& args)
{
    if (args.size() < 2) {
        return usage("UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>]");
    }
    const auto options = readOptions(args, 2, {"MINCOVER", "SPACING"});
    if (!options) {
        return options.error();
    }
    const auto lines = readSchedule(args[1]);
    if (!lines) {
        return lines.error();
    }
    auto text = sub::renderInvestigationReport(*lines, grading(*options), options->get("MINCOVER"));
    if (!text) {
        return text.error();
    }
    return reply(std::move(text).value());
}

Result<std::string> verify(const Words& args)
{
    if (args.size() != 2) {
        return usage("UTILITY VERIFY <schedule.csv>");
    }
    const auto lines = readSchedule(args[1]);
    if (!lines) {
        return lines.error();
    }
    return reply(sub::renderVerificationReport(sub::verifyDetections(*lines)));
}

Result<std::string> clearance(const Words& args)
{
    if (args.size() < 3) {
        return usage("UTILITY CLEARANCE <schedule.csv> <design.csv> [WIDTH <m>] [H <m>] [V <m>] "
                     "[MARGIN <m>]");
    }
    const auto options = readOptions(args, 3, {"WIDTH", "H", "V", "MARGIN"});
    if (!options) {
        return options.error();
    }
    const auto lines = readSchedule(args[1]);
    if (!lines) {
        return lines.error();
    }
    const auto designText = readFile(args[2]);
    if (!designText) {
        return designText.error();
    }
    auto design = sub::parseDesignCsv(*designText, args[2]);
    if (!design) {
        return inFile(design.error(), args[2]);
    }
    design->halfWidth = options->get("WIDTH").value_or(0.0) / 2.0;
    sub::ClearanceRequirement requirement;
    requirement.horizontal = options->get("H").value_or(requirement.horizontal);
    requirement.vertical = options->get("V").value_or(requirement.vertical);
    requirement.unverifiedMargin = options->get("MARGIN").value_or(requirement.unverifiedMargin);

    const auto results = sub::checkClearance(*design, *lines, requirement);
    if (!results) {
        return results.error();
    }
    return reply(sub::renderClearanceReport(*design, *results, requirement));
}

// UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>. A schedule with errors
// against the schema is REFUSED, so that a script gates a delivery on it -
// and the refusal carries the whole check, since a script that stops needs
// to say why.
Result<std::string> check(const Words& args)
{
    if (args.size() != 4 || !core::equalsIgnoringCase(args[2], "SCHEMA")) {
        return usage("UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>");
    }
    const auto schemaText = readFile(args[3]);
    if (!schemaText) {
        return schemaText.error();
    }
    const auto schema = sub::parseDeliverySchema(*schemaText);
    if (!schema) {
        return inFile(schema.error(), args[3]);
    }
    const auto scheduleText = readFile(args[1]);
    if (!scheduleText) {
        return scheduleText.error();
    }
    const auto result = sub::checkDelivery(*scheduleText, *schema);
    if (!result) {
        return inFile(result.error(), args[1]);
    }
    std::string text = reply(sub::renderSchemaCheck(*result, *schema));
    if (const std::size_t errors = result->count(sub::FindingSeverity::Error); errors != 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "the schedule does not meet the schema: " + std::to_string(errors) +
                             (errors == 1 ? " error" : " errors") + "\n" + text);
    }
    return text;
}

// UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]:
// the graded schedule into the drawing as one undo step (utility_drawing.hpp).
Result<std::string> draw(Document& document, const Words& args)
{
    if (args.size() < 2) {
        return usage("UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]");
    }
    const auto options = readOptions(args, 2, {"SPACING", "MINCOVER", "LAYER"});
    if (!options) {
        return options.error();
    }
    const auto lines = readSchedule(args[1]);
    if (!lines) {
        return lines.error();
    }
    UtilityDrawOptions drawOptions;
    drawOptions.grading = grading(*options);
    drawOptions.minimumCover = options->get("MINCOVER");
    if (options->layer) {
        drawOptions.layerPrefix = *options->layer;
    }
    const auto drawing = drawUtilities(*lines, drawOptions);
    if (!drawing) {
        return drawing.error();
    }
    if (auto status = document.execute(utilityDrawCommand(document.model(), *drawing)); !status) {
        return status.error();
    }
    return formatUtilityDrawing(*drawing);
}

} // namespace

bool isUtilityVerb(std::string_view verb)
{
    return core::equalsIgnoringCase(verb, "UTILITY");
}

Result<std::string> runUtilityVerb(Document& document, const std::vector<std::string>& tokens)
{
    if (tokens.empty() || !isUtilityVerb(tokens.front())) {
        return makeError(ErrorCode::ParseFailure, "not a UTILITY command; type HELP UTILITY",
                         tokens.empty() ? std::string{} : tokens.front());
    }
    // The action is the first word after the verb; the words from there are
    // as the verb's own functions number them, the action at 0.
    const Words args(tokens.begin() + 1, tokens.end());
    const std::string action = args.empty() ? std::string() : core::lowered(args.front());
    if (action == "report") {
        return report(args);
    }
    if (action == "verify") {
        return verify(args);
    }
    if (action == "clearance") {
        return clearance(args);
    }
    if (action == "check") {
        return check(args);
    }
    if (action == "draw") {
        return draw(document, args);
    }
    return usage("UTILITY REPORT | VERIFY | CLEARANCE | CHECK | DRAW ... (HELP UTILITY)");
}

std::optional<katana::geometry::Box2> drawReplyBounds(std::string_view reply)
{
    constexpr std::string_view kRecord = "utilities drawn ";
    constexpr std::string_view kKey = " bounds=";
    std::string_view first = reply.substr(0, reply.find('\n'));
    // A reply read back from a file or a pipe on Windows ends its lines in
    // CR LF; the record is the same.
    if (first.ends_with('\r')) {
        first.remove_suffix(1);
    }
    const std::size_t at = first.find(kKey);
    if (!first.starts_with(kRecord) || at == std::string_view::npos) {
        return std::nullopt;
    }
    std::string_view rest = first.substr(at + kKey.size());
    rest = rest.substr(0, rest.find(' '));
    std::vector<double> numbers;
    while (numbers.size() < 5) {
        const std::size_t comma = rest.find(',');
        const auto value = core::parseFiniteDouble(rest.substr(0, comma));
        if (!value) {
            return std::nullopt;
        }
        numbers.push_back(*value);
        if (comma == std::string_view::npos) {
            break;
        }
        rest = rest.substr(comma + 1);
    }
    if (numbers.size() != 4 || numbers[0] > numbers[2] || numbers[1] > numbers[3]) {
        return std::nullopt;
    }
    return katana::geometry::Box2(katana::geometry::Point2(numbers[0], numbers[1]),
                                  katana::geometry::Point2(numbers[2], numbers[3]));
}

std::string utilityVerbHelp()
{
    return R"(AS 5488 subsurface utilities (docs/subsurface_utilities.md). A schedule is a CSV of
located vertices, one row each; a path with blanks is quoted. Lengths are metres.

UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>]
          grade located services by AS 5488 quality level: vertices, segments, length at
          each level, depth of cover (flagged below MINCOVER), findings. SPACING is the
          longest detected segment that keeps QL-B (default 10)
UTILITY VERIFY <schedule.csv>
          QL-B detections against the QL-A exposures that name them in the verifies column
UTILITY CLEARANCE <schedule.csv> <design.csv> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
          clearance of proposed works from each service, widened by its quality level's
          tolerance: WIDTH of the works, H and V the clearances required, MARGIN how near
          an unmeasured (QL-C, QL-D) service is unconfirmed
UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>
          the schedule against a client's delivery schema: mandatory attributes and their
          value lists (tools/utility_schema_domains.py makes the schema file from a TfNSW
          Utility Schema workbook). Refused, with the whole check, when there are errors
UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
          the graded services into the drawing as ONE undo step: a polyline per run at one
          quality level on <prefix>/<type>/QL-A..QL-D (linetype by level, colour by type),
          a point per vertex on <prefix>/<type>/points, utility.* properties on each.
          LAYER defaults to "utilities"; MINCOVER marks each point's cover against it.
          Reply: "utilities drawn lines= vertices= segments= entities= layers= bounds=x0,y0,x1,y1"
          then "line id= type= length= ql_a= ql_b= ql_c= ql_d=" for each service)";
}

} // namespace katana::cad::utilities
