#include "katana/cad/utilities/utility_verbs.hpp"

#include <cstddef>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/utilities/utility_data.hpp"
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

// Each verb's usage names the schedule first, as it did before the drawing
// became a source too, then the scope words that are the other source.
constexpr std::string_view kReportUsage =
    "UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>], or UTILITY REPORT <scope> "
    "[WHERE key=value ...] [MINCOVER <m>] [SPACING <m>]";
constexpr std::string_view kVerifyUsage =
    "UTILITY VERIFY <schedule.csv>, or UTILITY VERIFY <scope> [WHERE key=value ...]";
constexpr std::string_view kClearanceUsage =
    "UTILITY CLEARANCE <schedule.csv> | <scope> [WHERE key=value ...] DESIGN <design.csv> | "
    "#<entity id> [LEVEL <z>] | ALIGNMENT <name> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]";
constexpr std::string_view kCheckUsage =
    "UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>, or UTILITY CHECK <scope> "
    "[WHERE key=value ...] SCHEMA <schema.csv>";
constexpr std::string_view kDrawUsage =
    "UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]";
constexpr std::string_view kRegradeUsage =
    "UTILITY REGRADE <scope> [WHERE key=value ...] [SPACING <m>] [MINCOVER <m>]";
constexpr std::string_view kScheduleUsage =
    "UTILITY SCHEDULE <out.csv> <scope> [WHERE key=value ...] [SCHEMA <schema.csv>]";

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

std::ofstream createFile(const std::string& path)
{
    if (core::isValidUtf8(path)) {
        return std::ofstream(pathFrom(path), std::ios::binary | std::ios::trunc);
    }
    return std::ofstream(path, std::ios::binary | std::ios::trunc);
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

Result<sub::DeliverySchema> readSchema(const std::string& path)
{
    const auto text = readFile(path);
    if (!text) {
        return text.error();
    }
    auto schema = sub::parseDeliverySchema(*text);
    if (!schema) {
        return inFile(schema.error(), path);
    }
    return std::move(schema).value();
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

// KEYWORD <value> pairs after the other arguments, each keyword at most
// once. A length must be finite and not negative; a level may be any finite
// number, below the datum too; LAYER takes a word.
enum class OptionKind { Metres, Level, Word };

struct OptionSpec {
    std::string_view name;
    OptionKind kind = OptionKind::Metres;
};

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
                            const std::vector<OptionSpec>& allowed)
{
    Options options;
    for (std::size_t i = first; i < args.size(); i += 2) {
        const OptionSpec* spec = nullptr;
        for (const OptionSpec& candidate : allowed) {
            if (core::equalsIgnoringCase(args[i], candidate.name)) {
                spec = &candidate;
            }
        }
        if (spec == nullptr) {
            return makeError(ErrorCode::InvalidArgument, "unknown option " + args[i]);
        }
        const std::string name(spec->name);
        if (options.get(name) || (spec->kind == OptionKind::Word && options.layer)) {
            return makeError(ErrorCode::InvalidArgument, name + " given twice");
        }
        if (spec->kind == OptionKind::Word) {
            if (i + 1 >= args.size()) {
                return makeError(ErrorCode::InvalidArgument, name + " needs a layer name");
            }
            options.layer = args[i + 1];
            continue;
        }
        const auto value =
            i + 1 < args.size() ? core::parseFiniteDouble(args[i + 1]) : std::nullopt;
        if (spec->kind == OptionKind::Level && !value) {
            return makeError(ErrorCode::InvalidArgument, name + " needs a level, in metres");
        }
        if (!value || *value < 0.0) {
            return makeError(ErrorCode::InvalidArgument,
                             name + " needs a number of metres, not negative");
        }
        options.numbers.emplace_back(name, *value);
    }
    return options;
}

bool isOptionWord(std::string_view word, const std::vector<OptionSpec>& allowed)
{
    for (const OptionSpec& spec : allowed) {
        if (core::equalsIgnoringCase(word, spec.name)) {
            return true;
        }
    }
    return false;
}

sub::GradingSettings grading(const Options& options)
{
    sub::GradingSettings settings;
    if (const auto spacing = options.get("SPACING")) {
        settings.maximumDetectedSpacing = *spacing;
    }
    return settings;
}

// ---- where the services come from --------------------------------------------------------

// The first word after the verb's own says: a scope word or WHERE is the
// drawing, by the shared scope words (scope_verbs.hpp); anything else is the
// path of a schedule.
struct SourceWords {
    std::optional<ScopeWords> scope;
    std::string path;
};

Result<SourceWords> readSourceWords(const Words& args, std::size_t& at)
{
    SourceWords source;
    if (isScopeWord(args[at])) {
        auto scope = parseScopeWords(args, at);
        if (!scope) {
            return scope.error();
        }
        source.scope = std::move(scope).value();
    } else {
        source.path = args[at++];
    }
    return source;
}

struct Source {
    std::vector<sub::UtilityLine> lines;
    // The drawing: what the scope took, as the first record of the reply,
    // and the services it read, each whole.
    std::optional<std::string> record;
    std::optional<UtilityData> data;
};

Result<Source> loadSource(const Document& document, const SourceWords& words,
                          const ScopeViewProvider& views)
{
    Source source;
    if (!words.scope) {
        auto lines = readSchedule(words.path);
        if (!lines) {
            return lines.error();
        }
        source.lines = std::move(lines).value();
        return source;
    }
    auto match = matchScope(document, *words.scope, views);
    if (!match) {
        return match.error();
    }
    auto data = readUtilityData(document.model(), match->matched);
    if (!data) {
        return data.error();
    }
    source.record = scopeRecord(*match) + utilityDataKeys(*data);
    source.lines = data->lines();
    source.data = std::move(data).value();
    return source;
}

// A reply on drawing data leads with what the scope took; a scope that took
// no service says so, and is not a failure.
std::string withRecord(const Source& source, std::string text)
{
    return source.record ? *source.record + "\n" + text : text;
}

std::string nothingIn(const Source& source, std::string_view what)
{
    return *source.record + "\nno utility lines in the scope: nothing " + std::string(what);
}

Result<std::string> report(const Document& document, const Words& args,
                           const ScopeViewProvider& views)
{
    if (args.size() < 2) {
        return usage(kReportUsage);
    }
    std::size_t at = 1;
    const auto words = readSourceWords(args, at);
    if (!words) {
        return words.error();
    }
    const auto options = readOptions(args, at, {{"MINCOVER"}, {"SPACING"}});
    if (!options) {
        return options.error();
    }
    const auto source = loadSource(document, *words, views);
    if (!source) {
        return source.error();
    }
    if (source->record && source->lines.empty()) {
        return nothingIn(*source, "to report");
    }
    auto text = sub::renderInvestigationReport(source->lines, grading(*options),
                                               options->get("MINCOVER"));
    if (!text) {
        return text.error();
    }
    return withRecord(*source, reply(std::move(text).value()));
}

Result<std::string> verify(const Document& document, const Words& args,
                           const ScopeViewProvider& views)
{
    if (args.size() < 2) {
        return usage(kVerifyUsage);
    }
    std::size_t at = 1;
    const auto words = readSourceWords(args, at);
    if (!words) {
        return words.error();
    }
    if (at != args.size()) {
        return usage(kVerifyUsage);
    }
    const auto source = loadSource(document, *words, views);
    if (!source) {
        return source.error();
    }
    if (source->record && source->lines.empty()) {
        return nothingIn(*source, "to verify");
    }
    return withRecord(*source,
                      reply(sub::renderVerificationReport(sub::verifyDetections(source->lines))));
}

// The proposed works CLEARANCE measures against: a design file, an entity
// drawn as the centre line, or a document alignment.
struct DesignWords {
    std::string path;
    std::optional<katana::entity::EntityId> entity;
    std::optional<std::string> alignment;
};

const std::vector<OptionSpec>& clearanceOptions()
{
    static const std::vector<OptionSpec> kOptions{
        {"WIDTH"}, {"H"}, {"V"}, {"MARGIN"}, {"LEVEL", OptionKind::Level}};
    return kOptions;
}

Result<DesignWords> readDesignWords(const Words& args, std::size_t& at)
{
    // DESIGN may be left out: the design file second, as it always was.
    if (at < args.size() && core::equalsIgnoringCase(args[at], "DESIGN")) {
        ++at;
    }
    if (at >= args.size() || isOptionWord(args[at], clearanceOptions())) {
        return usage(kClearanceUsage);
    }
    DesignWords design;
    const std::string& word = args[at++];
    if (word.size() > 1 && word.front() == '#') {
        const auto id = core::parseInteger(std::string_view(word).substr(1));
        if (!id || *id < 1) {
            return makeError(ErrorCode::ParseFailure, "a design entity is #<entity id>", word);
        }
        design.entity = static_cast<katana::entity::EntityId>(*id);
    } else if (core::equalsIgnoringCase(word, "ALIGNMENT")) {
        if (at >= args.size()) {
            return makeError(ErrorCode::InvalidArgument,
                             "ALIGNMENT needs the alignment's name; ALIGN LIST lists them");
        }
        design.alignment = args[at++];
    } else {
        design.path = word;
    }
    return design;
}

Result<sub::DesignAlignment> loadDesign(const Document& document, const DesignWords& words,
                                        std::optional<double> level)
{
    if (level && !words.entity) {
        return makeError(ErrorCode::InvalidArgument,
                         "LEVEL is the level of a design drawn as an entity (#<entity id>); a "
                         "design file and an alignment carry their own");
    }
    if (words.entity) {
        const katana::entity::Entity* entity = document.model().entities.find(*words.entity);
        if (entity == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no entity #" + std::to_string(*words.entity) + " in the drawing");
        }
        return designFromEntity(*entity, level);
    }
    if (words.alignment) {
        const katana::entity::Alignment* alignment =
            document.model().alignments.find(*words.alignment);
        if (alignment == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no alignment named " + *words.alignment + "; ALIGN LIST lists them");
        }
        return designFromAlignment(*alignment);
    }
    const auto text = readFile(words.path);
    if (!text) {
        return text.error();
    }
    auto design = sub::parseDesignCsv(*text, words.path);
    if (!design) {
        return inFile(design.error(), words.path);
    }
    return std::move(design).value();
}

Result<std::string> clearance(const Document& document, const Words& args,
                              const ScopeViewProvider& views)
{
    if (args.size() < 3) {
        return usage(kClearanceUsage);
    }
    std::size_t at = 1;
    const auto words = readSourceWords(args, at);
    if (!words) {
        return words.error();
    }
    const auto designWords = readDesignWords(args, at);
    if (!designWords) {
        return designWords.error();
    }
    const auto options = readOptions(args, at, clearanceOptions());
    if (!options) {
        return options.error();
    }
    const auto source = loadSource(document, *words, views);
    if (!source) {
        return source.error();
    }
    auto design = loadDesign(document, *designWords, options->get("LEVEL"));
    if (!design) {
        return design.error();
    }
    if (source->record && source->lines.empty()) {
        return nothingIn(*source, "to clear");
    }
    design->halfWidth = options->get("WIDTH").value_or(0.0) / 2.0;
    sub::ClearanceRequirement requirement;
    requirement.horizontal = options->get("H").value_or(requirement.horizontal);
    requirement.vertical = options->get("V").value_or(requirement.vertical);
    requirement.unverifiedMargin = options->get("MARGIN").value_or(requirement.unverifiedMargin);

    const auto results = sub::checkClearance(*design, source->lines, requirement);
    if (!results) {
        return results.error();
    }
    return withRecord(*source, reply(sub::renderClearanceReport(*design, *results, requirement)));
}

// UTILITY CHECK <schedule.csv | scope> SCHEMA <schema.csv>. A schedule with
// errors against the schema is REFUSED, so that a script gates a delivery on
// it - and the refusal carries the whole check, since a script that stops
// needs to say why. The drawing is checked as the schedule UTILITY SCHEDULE
// would write from it in the schema's own words (utilityCsvDialect): what the
// drawing would deliver.
Result<std::string> check(const Document& document, const Words& args,
                          const ScopeViewProvider& views)
{
    if (args.size() < 2) {
        return usage(kCheckUsage);
    }
    std::size_t at = 1;
    const auto words = readSourceWords(args, at);
    if (!words) {
        return words.error();
    }
    if (at + 2 != args.size() || !core::equalsIgnoringCase(args[at], "SCHEMA")) {
        return usage(kCheckUsage);
    }
    const auto schema = readSchema(args[at + 1]);
    if (!schema) {
        return schema.error();
    }
    std::string scheduleText;
    std::optional<Source> drawn;
    if (words->scope) {
        auto source = loadSource(document, *words, views);
        if (!source) {
            return source.error();
        }
        if (source->lines.empty()) {
            return nothingIn(*source, "to check");
        }
        auto text = sub::writeUtilityCsv(source->lines, sub::utilityCsvDialect(*schema));
        if (!text) {
            return text.error();
        }
        scheduleText = std::move(text).value();
        drawn = std::move(source).value();
    } else {
        auto text = readFile(words->path);
        if (!text) {
            return text.error();
        }
        scheduleText = std::move(text).value();
    }
    const auto result = sub::checkDelivery(scheduleText, *schema);
    if (!result) {
        return words->scope ? result.error() : inFile(result.error(), words->path);
    }
    std::string text = reply(sub::renderSchemaCheck(*result, *schema));
    if (drawn) {
        text = withRecord(*drawn, std::move(text));
    }
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
        return usage(kDrawUsage);
    }
    if (isScopeWord(args[1])) {
        return makeError(ErrorCode::InvalidArgument,
                         "UTILITY DRAW draws a schedule file; what is drawn already is graded "
                         "and drawn again by " +
                             std::string(kRegradeUsage));
    }
    const auto options =
        readOptions(args, 2, {{"SPACING"}, {"MINCOVER"}, {"LAYER", OptionKind::Word}});
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

// UTILITY REGRADE <scope> [SPACING <m>] [MINCOVER <m>]: the lines in scope
// graded again from their points, their runs drawn again and their points'
// graded properties replaced, as ONE undo step (utility_data.hpp).
Result<std::string> regrade(Document& document, const Words& args,
                            const ScopeViewProvider& views)
{
    if (args.size() < 2 || !isScopeWord(args[1])) {
        return usage(kRegradeUsage);
    }
    std::size_t at = 1;
    const auto words = readSourceWords(args, at);
    if (!words) {
        return words.error();
    }
    const auto options = readOptions(args, at, {{"SPACING"}, {"MINCOVER"}});
    if (!options) {
        return options.error();
    }
    const auto source = loadSource(document, *words, views);
    if (!source) {
        return source.error();
    }
    if (source->lines.empty()) {
        return nothingIn(*source, "to regrade");
    }
    UtilityDrawOptions drawOptions;
    drawOptions.grading = grading(*options);
    drawOptions.minimumCover = options->get("MINCOVER");
    auto plan = planUtilityRegrade(document.model(), *source->data, drawOptions);
    if (!plan) {
        return plan.error();
    }
    if (plan->command) {
        if (auto status = document.execute(std::move(plan->command)); !status) {
            return status.error();
        }
    }
    // The drawing's records, the regrade's first so a front end frames it
    // as it frames a draw, then what the scope took.
    const std::string records = formatUtilityDrawing(
        plan->drawing, "utilities regraded changed=" + std::to_string(plan->changed));
    const std::size_t firstEnd = records.find('\n');
    return records.substr(0, firstEnd) + "\n" + *source->record +
           (firstEnd == std::string::npos ? std::string() : records.substr(firstEnd));
}

// UTILITY SCHEDULE <out.csv> <scope> [SCHEMA <schema.csv>]: the lines in scope
// written as a schedule the reader takes back - a drawing edited in CAD made
// a deliverable - in the schema's words when one is named.
Result<std::string> schedule(const Document& document, const Words& args,
                             const ScopeViewProvider& views)
{
    if (args.size() < 3 || !isScopeWord(args[2])) {
        return usage(kScheduleUsage);
    }
    const std::string& path = args[1];
    std::size_t at = 2;
    const auto words = readSourceWords(args, at);
    if (!words) {
        return words.error();
    }
    std::optional<sub::DeliverySchema> schema;
    if (at < args.size()) {
        if (at + 2 != args.size() || !core::equalsIgnoringCase(args[at], "SCHEMA")) {
            return usage(kScheduleUsage);
        }
        auto read = readSchema(args[at + 1]);
        if (!read) {
            return read.error();
        }
        schema = std::move(read).value();
    }
    const auto source = loadSource(document, *words, views);
    if (!source) {
        return source.error();
    }
    if (source->lines.empty()) {
        return nothingIn(*source, "written");
    }
    auto text = sub::writeUtilityCsv(source->lines,
                                     schema ? sub::utilityCsvDialect(*schema) : sub::UtilityCsvDialect{});
    if (!text) {
        return text.error();
    }
    std::ofstream file = createFile(path);
    file.write(text->data(), static_cast<std::streamsize>(text->size()));
    file.close();
    if (!file) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path);
    }
    std::size_t vertices = 0;
    for (const sub::UtilityLine& line : source->lines) {
        vertices += line.vertices.size();
    }
    return "utilities scheduled path=" + recordValue(path) +
           " lines=" + std::to_string(source->lines.size()) +
           " vertices=" + std::to_string(vertices) + "\n" + *source->record;
}

} // namespace

bool isUtilityVerb(std::string_view verb)
{
    return core::equalsIgnoringCase(verb, "UTILITY");
}

Result<std::string> runUtilityVerb(Document& document, const std::vector<std::string>& tokens,
                                   const ScopeViewProvider& views)
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
        return report(document, args, views);
    }
    if (action == "verify") {
        return verify(document, args, views);
    }
    if (action == "clearance") {
        return clearance(document, args, views);
    }
    if (action == "check") {
        return check(document, args, views);
    }
    if (action == "draw") {
        return draw(document, args);
    }
    if (action == "regrade") {
        return regrade(document, args, views);
    }
    if (action == "schedule") {
        return schedule(document, args, views);
    }
    return usage("UTILITY REPORT | VERIFY | CLEARANCE | CHECK | DRAW | REGRADE | SCHEDULE ... "
                 "(HELP UTILITY)");
}

std::optional<katana::geometry::Box2> drawReplyBounds(std::string_view reply)
{
    constexpr std::string_view kKey = " bounds=";
    std::string_view first = reply.substr(0, reply.find('\n'));
    // A reply read back from a file or a pipe on Windows ends its lines in
    // CR LF; the record is the same.
    if (first.ends_with('\r')) {
        first.remove_suffix(1);
    }
    const std::size_t at = first.find(kKey);
    if (!(first.starts_with("utilities drawn ") || first.starts_with("utilities regraded ")) ||
        at == std::string_view::npos) {
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

Every verb but DRAW acts on a schedule file OR on what is drawn: a first word that is a
scope word takes the lines UTILITY DRAW drew, by the scope and filter every verb shares
(HELP: "Scope") - SELECTION | DRAWING | VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 |
LAYERS a,b [ONLY], then [WHERE key=value ...]. A scope that takes part of a line takes
all of it; the reply leads with "scope=... matched= lines= completed= ignored="
(completed: lines read whole from beyond the scope; ignored: entities with no utility
data).

UTILITY REPORT <schedule.csv> | <scope> [MINCOVER <m>] [SPACING <m>]
          grade located services by AS 5488 quality level: vertices, segments, length at
          each level, depth of cover (flagged below MINCOVER), findings. SPACING is the
          longest detected segment that keeps QL-B (default 10)
UTILITY VERIFY <schedule.csv> | <scope>
          QL-B detections against the QL-A exposures that name them in the verifies column
UTILITY CLEARANCE <schedule.csv> | <scope> [DESIGN] <design.csv> | #<id> [LEVEL <z>] |
          ALIGNMENT <name> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
          clearance of proposed works from each service, widened by its quality level's
          tolerance. The works: a design CSV; a line or polyline drawn (#id), at LEVEL or
          its own heights; or an alignment, at its design profile's levels. WIDTH of the
          works, H and V the clearances required, MARGIN how near an unmeasured (QL-C, QL-D)
          service is unconfirmed
UTILITY CHECK <schedule.csv> | <scope> SCHEMA <schema.csv>
          the schedule against a client's delivery schema: mandatory attributes and their
          value lists (tools/utility_schema_domains.py makes the schema file from a TfNSW
          Utility Schema workbook); the drawing is checked as the schedule it would write
          in the schema's words. Refused, with the whole check, when there are errors
UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
          the graded services into the drawing as ONE undo step: a polyline per run at one
          quality level on <prefix>/<type>/QL-A..QL-D (linetype by level, colour by type),
          a point per vertex on <prefix>/<type>/points carrying its whole schedule row as
          utility.* properties. LAYER defaults to "utilities"; MINCOVER marks each point's
          cover against it.
          Reply: "utilities drawn lines= vertices= segments= entities= layers= bounds=x0,y0,x1,y1"
          then "line id= type= length= ql_a= ql_b= ql_c= ql_d=" for each service
UTILITY REGRADE <scope> [SPACING <m>] [MINCOVER <m>]
          grade the lines in scope again from their points as they are now (moved, levels
          or methods edited) and draw their runs again, as ONE undo step; nothing changed,
          no step. Reply: "utilities regraded changed= lines= ... bounds=", the scope, lines
UTILITY SCHEDULE <out.csv> <scope> [SCHEMA <schema.csv>]
          write the lines in scope as a schedule this reads back as they are: a drawing
          edited in CAD made a deliverable, in the schema's words when SCHEMA is given)";
}

} // namespace katana::cad::utilities
