// CUSTOMISE (customisation_verbs.hpp) through CommandInterpreter::run, on a
// Document of the test's own: what every word does, what each refuses, and
// that a refused line leaves the session and the files as they were.
//
// "The built-in" is the small customisation written out below, handed over
// as a host hands one over, and "the kept file" a path in a folder of the
// test's own. Nothing here reads what a build may have compiled in or what
// the environment names, so the tests are the same on every machine.
//
// Every count is by hand from the two files below:
//
//   kBuiltIn "Small Built In"   TEST Fence (a linestyle), TEST Peg (a symbol,
//                               at vertices); rules FE* feature (layer FENCES,
//                               colour sui gas, linestyle TEST Fence) and PG*
//                               symbol (TEST Peg); one colour, sui gas.
//                               2 definitions, 2 rules over 2 keys.
//   kSite "Site"                symbols TEST Peg (again) and SITE Tree; rules
//                               FE* feature (layer SITE FENCES), TR* symbol
//                               (SITE Tree), GT* feature (layer GATES,
//                               linestyle SITE Gate - which nothing defines).
//
// Site MERGED into the built-in: TEST Peg replaced, SITE Tree added (3
// definitions); FE* feature replaced, TR* symbol and GT* feature added (4
// rules over 4 keys: FE*, PG*, TR*, GT*); SITE Gate asked for and undefined.
// Site in the built-in's PLACE: TEST Fence and PG* are gone (2 definitions, 3
// rules over 3 keys), and the session takes Site's name, having nothing of the
// built-in's definitions or rules left to be named by.

#include <gtest/gtest.h>

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <memory>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_verbs.hpp"
#include "katana/cad/definition_edit.hpp"
#include "katana/cad/definition_users.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/tables.hpp"

namespace fs = std::filesystem;
using katana::cad::BuiltInCustomisation;
using katana::cad::CommandInterpreter;
using katana::cad::CustomisationHost;
using katana::cad::CustomisationOrigin;
using katana::cad::CustomisationState;
using katana::cad::Document;
using katana::core::ErrorCode;
using katana::core::ReplyRecord;
using katana::entity::Customisation;

namespace {

const std::string kBuiltIn = R"({
  "format": "katana-customisation", "version": 1,
  "name": "Small Built In",
  "description": "Two definitions and two rules, for tests",
  "notice": ["Written for these tests."],
  "colours": {"sui gas": "#FF7F00"},
  "linestyles": [
    {"name": "TEST Fence", "strokes": [["move", 0, 0], ["draw", 2, 0]]}
  ],
  "symbols": [
    {"name": "TEST Peg", "atVertices": true, "strokes": [["move", 0, 0], ["circle", 0.5]]}
  ],
  "codes": [
    {"key": "FE*", "sets": "feature", "layer": "FENCES", "colour": "sui gas",
     "linestyle": "TEST Fence"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "TEST Peg", "size": 1.5}}
  ]
})";

const std::string kSite = R"({
  "format": "katana-customisation", "version": 1, "name": "Site",
  "symbols": [
    {"name": "SITE Tree", "strokes": [["circle", 2]]},
    {"name": "TEST Peg", "strokes": [["circle", 1]]}
  ],
  "codes": [
    {"key": "FE*", "sets": "feature", "layer": "SITE FENCES"},
    {"key": "TR*", "sets": "symbol", "symbol": {"name": "SITE Tree"}},
    {"key": "GT*", "sets": "feature", "layer": "GATES", "linestyle": "SITE Gate"}
  ]
})";

// Three more, for what a PART of a session says of where it came from:
//
//   kOwn "Own"      one linestyle, OWN Line: a colleague's own session.
//   kMarks "Marks"  a symbol, MARK Cross, the rule MK* that draws it, and a
//                   notice of its own.
//   kTints "Tints"  neither definitions nor rules: a table of one colour, and
//                   a notice of its own.
//
// Marks and Tints MERGED into the built-in: the session is still "Small Built
// In" with its own notice, and its sources are Small Built In (definitions,
// rules, no notice of its own - that is the session's), Marks (definitions,
// rules, its notice) and Tints (neither, its notice); 3 definitions, 3 rules,
// 2 colours.
const std::string kOwn = R"({
  "format": "katana-customisation", "version": 1, "name": "Own",
  "linestyles": [
    {"name": "OWN Line", "strokes": [["move", 0, 0], ["draw", 1, 0]]}
  ]
})";

const std::string kMarks = R"({
  "format": "katana-customisation", "version": 1, "name": "Marks",
  "notice": ["Marks: drawn by hand."],
  "symbols": [
    {"name": "MARK Cross", "atVertices": true, "strokes": [["move", -1, 0], ["draw", 1, 0]]}
  ],
  "codes": [
    {"key": "MK*", "sets": "symbol", "symbol": {"name": "MARK Cross"}}
  ]
})";

const std::string kTints = R"({
  "format": "katana-customisation", "version": 1, "name": "Tints",
  "notice": ["Tints: free to use."],
  "colours": {"tint teal": "#008080"}
})";

// A style library as another program writes one: not a Katana customisation.
const std::string kLegacyLibrary = "worldstyle \"X\" { move 0 0 draw 1 0 }\n";

// FNV-1a, 64-bit, as 16 lower-case hexadecimal digits: the digest of a file's
// bytes, worked out here from the algorithm's published offset basis and
// prime rather than by the function the program uses.
std::string fnv1a(const std::string& bytes)
{
    std::uint64_t hash = 0xcbf29ce484222325ULL;
    for (const char ch : bytes) {
        hash ^= static_cast<unsigned char>(ch);
        hash *= 0x100000001b3ULL;
    }
    static const char* const digits = "0123456789abcdef";
    std::string text(16, '0');
    for (int i = 15; i >= 0; --i) {
        text[static_cast<std::size_t>(i)] = digits[hash & 0xF];
        hash >>= 4;
    }
    return text;
}

// The process id, so that two processes running these cases at once never
// share a folder (ctest runs a test program a case at a time, in parallel).
std::string processTag()
{
#if defined(_WIN32)
    return std::to_string(_getpid());
#else
    return std::to_string(getpid());
#endif
}

// A fresh directory for one test's files, removed when the test ends.
struct Scratch {
    fs::path root;

    explicit Scratch(const std::string& name)
        : root(fs::temp_directory_path() /
               ("katana-cad-tests-customisation-verbs-" + processTag()) / name)
    {
        fs::remove_all(root);
        fs::create_directories(root);
    }
    ~Scratch()
    {
        std::error_code ignored;
        fs::remove_all(root.parent_path(), ignored);
    }
    Scratch(const Scratch&) = delete;
    Scratch& operator=(const Scratch&) = delete;

    fs::path write(const std::string& name, const std::string& text) const
    {
        const fs::path path = root / name;
        std::ofstream(path, std::ios::binary) << text;
        return path;
    }
};

std::string contentsOf(const fs::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
}

// A path as a reply writes one: absolute, and with '/' on every platform.
std::string shown(const fs::path& path)
{
    const std::u8string text = fs::absolute(path).lexically_normal().generic_u8string();
    return std::string(text.begin(), text.end());
}

// A path as a line takes one: quoted, so that a folder with a blank in its
// name is still one word.
std::string typed(const fs::path& path) { return "\"" + shown(path) + "\""; }

Customisation parsed(const std::string& text)
{
    auto read = katana::entity::customisationFromJson(text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? std::string{} : read.error().describe());
    return read.ok() ? std::move(*read) : Customisation{};
}

BuiltInCustomisation builtInFrom(const std::string& text)
{
    BuiltInCustomisation builtIn;
    builtIn.customisation = std::make_shared<const Customisation>(parsed(text));
    builtIn.digest = fnv1a(text);
    return builtIn;
}

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

// The `nth` record of `reply` whose first word is `word`, read by the one
// reader of reply records (core::readReplyRecord), so that a value is compared
// as it is and not as it happens to be quoted.
ReplyRecord record(const std::string& reply, std::string_view word, std::size_t nth = 0)
{
    std::size_t seen = 0;
    for (const std::string_view line : katana::core::splitLines(reply)) {
        const auto one = katana::core::readReplyRecord(line);
        if (one && !one->words.empty() && one->words.front() == word && seen++ == nth) {
            return *one;
        }
    }
    ADD_FAILURE() << "no record \"" << word << "\" number " << nth << " in:\n" << reply;
    return {};
}

std::size_t recordsOf(const std::string& reply, std::string_view word)
{
    std::size_t seen = 0;
    for (const std::string_view line : katana::core::splitLines(reply)) {
        const auto one = katana::core::readReplyRecord(line);
        seen += one && !one->words.empty() && one->words.front() == word ? 1 : 0;
    }
    return seen;
}

std::string field(const ReplyRecord& from, std::string_view key)
{
    return from.value(key).value_or("(absent)");
}

// A session with no host: a bare Document, as a test or a tool has one.
struct Bare {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        if (!reply.ok()) {
            return {};
        }
        // A script fails a line on this word, so no reply to a line that
        // succeeded may hold it.
        EXPECT_FALSE(contains(*reply, "error")) << line << ":\n" << *reply;
        EXPECT_FALSE(reply->ends_with('\n')) << line;
        return *reply;
    }
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    const CustomisationState& state() const { return document.customisationState(); }
    // The built-in loaded as any file is, for the tests that need a session
    // and no host.
    void loadBuiltIn(const Scratch& scratch)
    {
        ok("CUSTOMISE " + typed(scratch.write("builtin.customisation.json", kBuiltIn)));
    }
};

// A session started as a front end starts one: the host handed to the
// interpreter, then startCustomisation.
struct Hosted : Bare {
    Scratch scratch;
    fs::path keptFile;
    CustomisationHost host;

    // The kept file in a folder that does not exist yet, as a per-user folder
    // nothing has written to does not; absolute and normal, as a front end
    // gives one, so that a reply names it by the very text handed over.
    explicit Hosted(const std::string& name, bool builtIn = true, bool kept = true)
        : scratch(name),
          keptFile(fs::absolute(scratch.root / "kept" / "customisation.json").lexically_normal())
    {
        if (builtIn) {
            host.builtIn = builtInFrom(kBuiltIn);
        }
        if (kept) {
            host.keptFile = keptFile;
        }
    }
    void start()
    {
        interpreter.setCustomisationHost(host);
        (void)katana::cad::startCustomisation(document, host);
    }
    fs::path backup() const { return fs::path(keptFile).concat(".bak"); }
};

// A definition put into the session's library as an editor puts one: from no
// customisation, and named by nothing until a test names it. `symbol`: listed
// as a symbol and drawn at vertices, else a linestyle.
void addDefinition(Document& document, const std::string& name, bool symbol = false)
{
    katana::entity::StyleLibrary library = document.styleLibrary();
    katana::entity::LineStyle definition;
    definition.name = name;
    definition.symbol = symbol;
    definition.atVertices = symbol;
    ASSERT_TRUE(katana::entity::addOrReplace(library, definition).ok());
    document.setStyleLibrary(library);
}

// {name, brought definitions, brought rules, its notice}.
using Sources = std::vector<katana::entity::CustomisationSourceNote>;

const char* const kDefaultLinework =
    "linework linework.start=ST linework.end=END linework.close=CL linework.arcstart=BC "
    "linework.arcend=EC linework.join=JPN linework.rectangle=RECT";
const char* const kNoStyles =
    "This drawing has no styles yet; import a drawing or survey that carries styles, or make "
    "one in Format > Styles and Linetypes or with STYLE NEW, to see the customisation take "
    "effect.";

} // namespace

// ---- the report ----------------------------------------------------------------------------

TEST(CustomisationVerbs, AloneItReportsTheCountsThenRecordsThenTheCoverage)
{
    Hosted session("report");
    session.start();
    // 2 definitions in no group: TEST Peg is offered as a symbol (at
    // vertices, and listed as one), TEST Fence as a linestyle.
    const std::string expected =
        std::string("2 linestyle and symbol definitions in 0 groups: 1 offered as symbols, 1 as "
                    "linestyles (one definition can be both)\n"
                    "2 survey code rules over 2 distinct codes\n"
                    "customisation name=\"Small Built In\" origin=builtIn kept=yes definitions=2 "
                    "codes=2 rules=2 colours=1\n"
                    "source name=\"Small Built In\" definitions=yes rules=yes\n"
                    "automation auto.codes=on auto.linework=on\n") +
        kDefaultLinework + "\n" + kNoStyles;
    EXPECT_EQ(session.ok("CUSTOMISE"), expected);
    // The interpreter reads the verb as it reads every verb: in any case,
    // and by its other spelling.
    EXPECT_EQ(session.ok("customise"), expected);
    EXPECT_EQ(session.ok("CUSTOMIZE"), expected);
}

TEST(CustomisationVerbs, WithNothingLoadedItSaysSoAndHowToLoad)
{
    Bare session;
    EXPECT_EQ(session.ok("CUSTOMISE"),
              std::string("No customisation is loaded.\n"
                          "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n"
                          "customisation name=\"\" origin=none kept=no definitions=0 codes=0 "
                          "rules=0 colours=0\n"
                          "automation auto.codes=on auto.linework=on\n") +
                  kDefaultLinework);
}

TEST(CustomisationVerbs, ACustomisationThatBringsNeitherKindIsNotSaidToBeNoCustomisation)
{
    // A file of settings alone: a name, the seven control codes, the two
    // switches. Loaded, a customisation IS loaded - the record below the
    // first line names it and says where it came from - so the first line may
    // not say that none is. What is true of it is that nothing in it draws.
    const std::string settings = R"({
  "format": "katana-customisation", "version": 1, "name": "Set",
  "linework": {"start": "S", "end": "E", "close": "C", "arcStart": "PC", "arcEnd": "PT",
               "join": "J", "rectangle": "R"},
  "automation": {"codesOnSurveyImport": false, "lineworkOnSurveyImport": false}
})";
    const Scratch scratch("settings-only");
    Bare session;
    session.ok("CUSTOMISE " + typed(scratch.write("settings.json", settings)));
    EXPECT_EQ(session.ok("CUSTOMISE"),
              "No linestyle or symbol definitions and no survey code rules are loaded.\n"
              "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n"
              "customisation name=Set origin=loaded kept=no definitions=0 codes=0 rules=0 "
              "colours=0\n"
              "source name=Set definitions=no rules=no\n"
              "automation auto.codes=off auto.linework=off\n"
              "linework linework.start=S linework.end=E linework.close=C linework.arcstart=PC "
              "linework.arcend=PT linework.join=J linework.rectangle=R");
}

TEST(CustomisationVerbs, JsonIsTheSameAsOneObject)
{
    Hosted session("json");
    session.start();
    // Keys in alphabetical order, two blanks a level. By hand from kBuiltIn:
    // neither rule has anything wrong with it, the drawing has no styles, and
    // the session is based on the built-in itself, by the digest of its bytes.
    // `start`, the last member, is what the start found: the built-in read and
    // nothing was kept, so nothing went wrong and nothing was made from
    // another built-in. (The member was added on 2026-10-07; before it the
    // object ended at `sources`.)
    const std::string expected = R"({
  "automation": {
    "codesOnSurveyImport": true,
    "lineworkOnSurveyImport": true
  },
  "basedOn": {
    "digest": ")" + fnv1a(kBuiltIn) + R"(",
    "name": "Small Built In"
  },
  "builtIn": "Small Built In",
  "colours": {
    "sui gas": "#FF7F00"
  },
  "counts": {
    "codes": 2,
    "colours": 1,
    "definitions": 2,
    "groups": 0,
    "linestyles": 1,
    "rules": 2,
    "symbols": 1
  },
  "coverage": {
    "builtIn": 0,
    "named": 0,
    "notLinestyles": [],
    "resolved": 0,
    "styles": 0,
    "unresolved": []
  },
  "description": "Two definitions and two rules, for tests",
  "kept": true,
  "linework": {
    "arcEnd": "EC",
    "arcStart": "BC",
    "close": "CL",
    "end": "END",
    "join": "JPN",
    "rectangle": "RECT",
    "start": "ST"
  },
  "missing": [],
  "name": "Small Built In",
  "notice": [
    "Written for these tests."
  ],
  "origin": "builtIn",
  "problems": {
    "byKind": {},
    "cannotApply": 0,
    "rules": 2,
    "undefined": [],
    "warnings": 0
  },
  "sources": [
    {
      "definitions": true,
      "name": "Small Built In",
      "notice": [],
      "rules": true
    }
  ],
  "start": {
    "keptFromAnotherBuiltIn": false,
    "problems": []
  }
})";
    EXPECT_EQ(session.ok("CUSTOMISE JSON"), expected);
    EXPECT_EQ(session.ok("customise json"), expected);
}

TEST(CustomisationVerbs, JsonCountsWhatIsWrongWithTheRulesWithoutTheWordAScriptFailsOn)
{
    // Site alone: its GT* rule names a linestyle nothing defines (one
    // "unresolved linestyle" warning, and one undefined name).
    const Scratch scratch("json-problems");
    Bare session;
    session.ok("CUSTOMISE " + typed(scratch.write("site.json", kSite)));
    const std::string json = session.ok("CUSTOMISE JSON");
    EXPECT_TRUE(contains(json, "\"byKind\": {\n      \"unresolved linestyle\": 1\n    }")) << json;
    EXPECT_TRUE(contains(json, "\"cannotApply\": 0")) << json;
    EXPECT_TRUE(contains(json, "\"undefined\": [\n      \"SITE Gate\"\n    ]")) << json;
    EXPECT_TRUE(contains(json, "\"warnings\": 1")) << json;
    EXPECT_TRUE(contains(json, "\"origin\": \"loaded\"")) << json;
    EXPECT_TRUE(contains(json, "\"basedOn\": null")) << json;
}

TEST(CustomisationVerbs, JsonCountsWhatTheDrawingUsesAndCarriesEachSourcesNoticeAndWhatIsMissing)
{
    // The built-in with Marks merged in - a source with a notice of its own -
    // and a drawing of four styles:
    //
    //   Fence   linetype TEST Fence, which the library defines
    //   Ghost   linetype TEST Spare, a definition removed again from under it
    //   Mark    symbol cross, a shape Katana draws itself
    //   Plain   names nothing
    //
    // So 4 styles; 2 name a definition (Fence and Ghost) and 1 of those is
    // defined; 1 is drawn by Katana itself; TEST Spare is the one name the
    // styles give that nothing defines.
    Hosted session("json-coverage");
    session.start();
    session.ok("CUSTOMISE " + typed(session.scratch.write("marks.json", kMarks)));
    addDefinition(session.document, "TEST Spare");
    session.ok("STYLE NEW Fence");
    session.ok("STYLE SET Fence linetype \"TEST Fence\"");
    session.ok("STYLE NEW Ghost");
    session.ok("STYLE SET Ghost linetype \"TEST Spare\"");
    session.ok("STYLE NEW Mark");
    session.ok("STYLE SET Mark symbol cross");
    session.ok("STYLE NEW Plain");
    session.ok("CUSTOMISE REMOVE \"TEST Spare\" FORCE");

    const std::string json = session.ok("CUSTOMISE JSON");
    EXPECT_TRUE(contains(json, R"(  "coverage": {
    "builtIn": 1,
    "named": 2,
    "notLinestyles": [],
    "resolved": 1,
    "styles": 4,
    "unresolved": [
      "TEST Spare"
    ]
  },)"))
        << json;
    // Each source with its own notice: the built-in's is the session's, said
    // at the top, so its entry has none.
    EXPECT_TRUE(contains(json, R"(  "sources": [
    {
      "definitions": true,
      "name": "Small Built In",
      "notice": [],
      "rules": true
    },
    {
      "definitions": true,
      "name": "Marks",
      "notice": [
        "Marks: drawn by hand."
      ],
      "rules": true
    }
  ])"))
        << json;
    EXPECT_TRUE(contains(json, "  \"missing\": [],\n")) << json;

    // The project is recorded with the two customisations it is drawn with,
    // in load order. Opened where neither is loaded, both are missing.
    const fs::path project = session.scratch.root / "drawn.katana";
    ASSERT_TRUE(session.document.saveAs(project).ok());
    Bare other;
    ASSERT_TRUE(other.document.open(project).ok());
    EXPECT_TRUE(contains(other.ok("CUSTOMISE JSON"), R"(  "missing": [
    "Small Built In",
    "Marks"
  ],)"));
    EXPECT_TRUE(contains(other.ok("CUSTOMISE"), "missing name=\"Small Built In\"\n"
                                                "missing name=Marks"));
}

// ---- loading -------------------------------------------------------------------------------

TEST(CustomisationVerbs, AFileIsMergedIntoWhatIsLoadedAndTheReplySaysWhatItDid)
{
    Hosted session("merge");
    session.start();
    const fs::path site = session.scratch.write("site.json", kSite);
    const std::string reply = session.ok("CUSTOMISE " + typed(site));

    const ReplyRecord loaded = record(reply, "loaded");
    EXPECT_EQ(field(loaded, "file"), shown(site));
    EXPECT_EQ(field(loaded, "name"), "Site");
    EXPECT_EQ(field(loaded, "definitions_added"), "1");    // SITE Tree
    EXPECT_EQ(field(loaded, "definitions_replaced"), "1"); // TEST Peg
    EXPECT_EQ(field(loaded, "codes_added"), "2");          // TR* symbol, GT* feature
    EXPECT_EQ(field(loaded, "codes_replaced"), "1");       // FE* feature
    EXPECT_EQ(field(loaded, "colours_added"), "0");
    EXPECT_EQ(field(loaded, "colours_replaced"), "0");
    EXPECT_EQ(field(loaded, "linework"), "no");
    EXPECT_EQ(field(loaded, "automation"), "no");
    // A merge removes nothing, and says nothing of removing.
    EXPECT_EQ(recordsOf(reply, "removed"), 0u) << reply;
    // What the rules ask for that nothing defines, then what is loaded now.
    EXPECT_TRUE(reply.ends_with("undefined names=1\n"
                                "undefined name=\"SITE Gate\"\n"
                                "customisation name=\"Small Built In\" origin=loaded kept=no "
                                "definitions=3 codes=4 rules=4 colours=1"))
        << reply;

    // The session is what the reply says: the fence's rule is Site's now, the
    // peg's is still the built-in's, and it keeps the name it had.
    EXPECT_EQ(session.document.styleLibrary().size(), 3u);
    const auto& rules = session.document.surveyMap().rules();
    ASSERT_EQ(rules.size(), 4u);
    EXPECT_EQ(rules[0].key, "FE*");
    EXPECT_EQ(rules[0].model, "SITE FENCES");
    EXPECT_EQ(rules[1].key, "PG*");
    EXPECT_EQ(rules[2].key, "TR*");
    EXPECT_EQ(rules[3].key, "GT*");
    EXPECT_EQ(session.state().name, "Small Built In");
    EXPECT_EQ(session.state().origin, CustomisationOrigin::Loaded);
    EXPECT_FALSE(session.state().kept);
    ASSERT_EQ(session.state().sources.size(), 2u);
    EXPECT_EQ(session.state().sources[1].name, "Site");
}

TEST(CustomisationVerbs, ReplaceTakesThePlaceOfEachKindAndSaysWhatIsGone)
{
    Hosted session("replace");
    session.start();
    const fs::path site = session.scratch.write("site.json", kSite);
    const std::string reply = session.ok("customise replace " + typed(site));

    const ReplyRecord loaded = record(reply, "loaded");
    EXPECT_EQ(field(loaded, "file"), shown(site));
    EXPECT_EQ(field(loaded, "definitions_added"), "1");
    EXPECT_EQ(field(loaded, "definitions_replaced"), "1");
    EXPECT_EQ(field(loaded, "codes_added"), "2");
    EXPECT_EQ(field(loaded, "codes_replaced"), "1");
    // TEST Fence and PG* were the built-in's and Site does not bring them.
    // Site brought both kinds, so nothing of the built-in's definitions or
    // rules is left to be named by, and the session takes Site's name. The
    // colour table is merged by name in either mode: sui gas stays.
    EXPECT_TRUE(reply.ends_with("removed definitions=1 codes=1\n"
                                "removed definition=\"TEST Fence\"\n"
                                "removed code=PG*\n"
                                "undefined names=1\n"
                                "undefined name=\"SITE Gate\"\n"
                                "customisation name=Site origin=loaded kept=no definitions=2 "
                                "codes=3 rules=3 colours=1"))
        << reply;
    EXPECT_FALSE(session.document.styleLibrary().contains("TEST Fence"));
    EXPECT_EQ(session.document.surveyMap().size(), 3u);
    ASSERT_EQ(session.state().sources.size(), 1u);
    EXPECT_EQ(session.state().sources[0].name, "Site");

    EXPECT_EQ(session.refused("CUSTOMISE REPLACE").message,
              "usage: CUSTOMISE [REPLACE] <file> [<file>...]");
}

TEST(CustomisationVerbs, AFileNamedTwiceIsReadOnceAndSaid)
{
    Hosted session("twice");
    session.start();
    const fs::path site = session.scratch.write("site.json", kSite);
    const std::string reply = session.ok("CUSTOMISE " + typed(site) + " " + typed(site));
    EXPECT_EQ(recordsOf(reply, "repeated"), 1u) << reply;
    EXPECT_EQ(field(record(reply, "repeated"), "file"), shown(site));
    EXPECT_EQ(recordsOf(reply, "loaded"), 1u) << reply;
    // Read twice, each of Site's three rules would be in the map twice: 7.
    EXPECT_EQ(session.document.surveyMap().size(), 4u);
}

TEST(CustomisationVerbs, AReplyCountsEveryNameAndListsTheFirstTwenty)
{
    // 25 symbols, D01 to D25, and then a library of one other in their
    // place: all 25 are gone. The count says 25; the names stop at 20, in
    // name order, so D01 to D20 are listed and D21 to D25 are not.
    std::string many = R"({"format": "katana-customisation", "version": 1, "name": "Many",)"
                       R"( "symbols": [)";
    for (int i = 1; i <= 25; ++i) {
        many += std::string(i > 1 ? ", " : "") + R"({"name": "D)" + (i < 10 ? "0" : "") +
                std::to_string(i) + R"(", "strokes": [["circle", 1]]})";
    }
    many += "]}";
    const std::string one = R"({"format": "katana-customisation", "version": 1, "name": "One",)"
                            R"( "symbols": [{"name": "Z", "strokes": [["circle", 1]]}]})";
    const Scratch scratch("twenty");
    Bare session;
    session.ok("CUSTOMISE " + typed(scratch.write("many.json", many)));
    ASSERT_EQ(session.document.styleLibrary().size(), 25u);

    const std::string reply =
        session.ok("CUSTOMISE REPLACE " + typed(scratch.write("one.json", one)));
    const ReplyRecord count = record(reply, "removed");
    EXPECT_EQ(field(count, "definitions"), "25");
    EXPECT_EQ(field(count, "codes"), "0");
    // The record of the count, then one a name.
    EXPECT_EQ(recordsOf(reply, "removed"), 21u) << reply;
    EXPECT_EQ(field(record(reply, "removed", 1), "definition"), "D01");
    EXPECT_EQ(field(record(reply, "removed", 20), "definition"), "D20");
    EXPECT_FALSE(contains(reply, "D21")) << reply;
    EXPECT_EQ(session.document.styleLibrary().size(), 1u);
}

TEST(CustomisationVerbs, ALoadIsAllOrNothingAndEveryFileThatDoesNotReadIsNamed)
{
    Hosted session("all-or-nothing");
    session.start();
    const Customisation before = session.document.customisation();
    const std::uint64_t generation = session.document.customisationGeneration();
    const fs::path site = session.scratch.write("site.json", kSite);
    const fs::path legacy = session.scratch.write("lines.4d", kLegacyLibrary);
    const fs::path absent = session.scratch.root / "nowhere.json";

    // A good file first, then one of another program's: the reader's own
    // words, the file beside them, and nothing of the good file installed.
    const katana::core::Error one = session.refused("CUSTOMISE " + typed(site) + " " +
                                                    typed(legacy));
    EXPECT_EQ(one.code, ErrorCode::ParseFailure);
    EXPECT_EQ(one.message, "not a Katana customisation file");
    EXPECT_TRUE(contains(one.context, "lines.4d")) << one.describe();

    // Two that do not read: both are listed, each with its reason.
    const katana::core::Error two = session.refused("CUSTOMISE REPLACE " + typed(legacy) + " " +
                                                    typed(site) + " " + typed(absent));
    EXPECT_TRUE(two.message.starts_with("2 of the 3 files were not read, so nothing was loaded\n"))
        << two.message;
    EXPECT_TRUE(contains(two.message, "\n  ParseFailure: not a Katana customisation file ["))
        << two.message;
    EXPECT_TRUE(contains(two.message, "\n  NotFound: the file cannot be opened [")) << two.message;
    EXPECT_TRUE(contains(two.message, "nowhere.json")) << two.message;

    // A survey code file of another program is XML, and is told the same as
    // its style library was.
    const fs::path codes = session.scratch.write(
        "codes.mapfile", "<?xml version=\"1.0\"?>\n<map_file><map_data/></map_file>\n");
    const katana::core::Error xml = session.refused("CUSTOMISE REPLACE " + typed(codes));
    EXPECT_EQ(xml.code, ErrorCode::ParseFailure);
    EXPECT_EQ(xml.message, "not a Katana customisation file");

    EXPECT_TRUE(session.document.customisation() == before);
    EXPECT_EQ(session.document.customisationGeneration(), generation);
    EXPECT_EQ(session.state().origin, CustomisationOrigin::BuiltIn);
}

TEST(CustomisationVerbs, ALoadTheSessionCouldNotBeRecordedWithIsRefusedWhole)
{
    // The merge judges the loads; the install judges the session's own parts.
    // A definition made in the session whose source could never be written
    // into a project (a path, not a name) stops a load that is itself sound.
    Hosted session("install-refused");
    session.start();
    katana::entity::StyleLibrary library = session.document.styleLibrary();
    katana::entity::LineStyle stray;
    stray.name = "STRAY";
    stray.source = "somewhere/else";
    ASSERT_TRUE(katana::entity::addOrReplace(library, stray).ok());
    session.document.setStyleLibrary(library);
    const Customisation before = session.document.customisation();

    const katana::core::Error error =
        session.refused("CUSTOMISE " + typed(session.scratch.write("site.json", kSite)));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(error.message, "the source of its definition \"STRAY\"")) << error.message;
    EXPECT_TRUE(session.document.customisation() == before);
}

TEST(CustomisationVerbs, AKeywordIsTheWholeFirstWordAndAFileSoNamedIsGivenWithItsDirectory)
{
    // Files called exactly as two of the keywords, in the working directory.
    const Scratch scratch("keywords");
    scratch.write("json", kSite);
    scratch.write("replace", kSite);
    const fs::path was = fs::current_path();
    fs::current_path(scratch.root);
    struct Restore {
        fs::path to;
        ~Restore()
        {
            std::error_code ignored;
            fs::current_path(to, ignored);
        }
    } restore{was};

    Bare session;
    // The bare words are the keywords, in any case: the report, and a
    // REPLACE with no file.
    EXPECT_TRUE(session.ok("CUSTOMISE json").starts_with("{\n")) << "the report, as JSON";
    EXPECT_EQ(session.document.surveyMap().size(), 0u);
    EXPECT_EQ(session.refused("CUSTOMISE Replace").message,
              "usage: CUSTOMISE [REPLACE] <file> [<file>...]");
    // With its directory, each is the file. (The command line removes quotes
    // before a verb sees its words, so a quoted "json" is the keyword too.)
    EXPECT_TRUE(session.ok("CUSTOMISE \"json\"").starts_with("{\n"));
    const std::string loaded = session.ok("CUSTOMISE ./json");
    EXPECT_EQ(field(record(loaded, "loaded"), "name"), "Site");
    EXPECT_EQ(session.document.surveyMap().size(), 3u);
    // After REPLACE the word is a file whatever it says. (Compared as files:
    // the working directory may be spelled another way than the folder was.)
    const std::string replaced = session.ok("CUSTOMISE REPLACE replace");
    const std::string named = field(record(replaced, "loaded"), "file");
    std::error_code unknown;
    EXPECT_TRUE(fs::equivalent(katana::core::pathFromUtf8(named), scratch.root / "replace",
                               unknown))
        << named;
    // A word that is no keyword is a file, and is refused as one that is
    // not there: LOAD is not a word of this verb.
    EXPECT_EQ(session.refused("CUSTOMISE LOAD ./json").code, ErrorCode::NotFound);

    // EXPORT's first word is the file, and one of EXPORT's own words there
    // is a line whose file was left out: CUSTOMISE EXPORT CODES once wrote
    // the whole session into a file called CODES. With its directory, a file
    // so called is the file. (The session holds Site: 3 rules.)
    for (const char* word : {"CODES", "linestyles", "Symbols", "NAME", "only"}) {
        const katana::core::Error error =
            session.refused(std::string("CUSTOMISE EXPORT ") + word);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << word;
        EXPECT_TRUE(error.message.starts_with("usage: CUSTOMISE EXPORT <file> ")) << error.message;
        EXPECT_TRUE(error.message.ends_with(std::string("is given with its directory (./") + word +
                                            ")"))
            << error.message;
        EXPECT_EQ(error.context, word);
        EXPECT_FALSE(fs::exists(scratch.root / word)) << word;
    }
    EXPECT_EQ(field(record(session.ok("CUSTOMISE EXPORT ./CODES"), "exported"), "rules"), "3");
    EXPECT_EQ(parsed(contentsOf(scratch.root / "CODES")).map.size(), 3u);
}

TEST(CustomisationVerbs, TheWordsThatTakeNothingRefuseAWordAfterThem)
{
    Hosted session("alone");
    session.start();
    for (const char* word : {"JSON", "RESET", "KEEP", "REVERT"}) {
        const katana::core::Error error =
            session.refused(std::string("CUSTOMISE ") + word + " now");
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << word;
        EXPECT_EQ(error.message, std::string("usage: CUSTOMISE ") + word);
    }
    EXPECT_FALSE(fs::exists(session.keptFile));
    EXPECT_EQ(session.state().origin, CustomisationOrigin::BuiltIn);
}

// ---- export --------------------------------------------------------------------------------

TEST(CustomisationVerbs, AnExportReadInTheSessionsPlaceGivesTheSameSession)
{
    // A session with everything in it: the built-in, Site merged in, a
    // control code respelled and a switch turned off.
    Hosted first("export-round-trip");
    first.start();
    first.ok("CUSTOMISE " + typed(first.scratch.write("site.json", kSite)));
    first.ok("CUSTOMISE SET linework.start=S auto.codes=off");
    const fs::path out = first.scratch.root / "session.customisation.json";

    const ReplyRecord exported = record(first.ok("CUSTOMISE EXPORT " + typed(out)), "exported");
    EXPECT_EQ(field(exported, "file"), shown(out));
    EXPECT_EQ(field(exported, "name"), "Small Built In");
    EXPECT_EQ(field(exported, "definitions"), "3");
    EXPECT_EQ(field(exported, "codes"), "4");
    EXPECT_EQ(field(exported, "rules"), "4");
    EXPECT_EQ(field(exported, "replaced"), "no");
    // An export changes nothing of the session.
    EXPECT_EQ(first.state().origin, CustomisationOrigin::Edited);

    // Read in the place of nothing, in a session with no host at all.
    Bare second;
    second.ok("CUSTOMISE REPLACE " + typed(out));
    EXPECT_TRUE(second.document.customisation() == first.document.customisation());
    EXPECT_EQ(second.state().linework.start, "S");
    EXPECT_FALSE(second.state().automation.codesOnSurveyImport);
    EXPECT_EQ(second.state().sources.size(), 2u);

    // Written again over the file that is there, and said.
    EXPECT_EQ(field(record(first.ok("CUSTOMISE EXPORT " + typed(out)), "exported"), "replaced"),
              "yes");
    EXPECT_FALSE(fs::exists(fs::path(out).concat(".tmp")));
}

TEST(CustomisationVerbs, AnExportOfAPartHoldsThatPartAndNoSettings)
{
    Hosted session("export-parts");
    session.start();
    session.ok("CUSTOMISE SET linework.start=S");
    const auto exportedTo = [&session](const std::string& name, const std::string& words) {
        const fs::path file = session.scratch.root / name;
        const std::string reply = session.ok("CUSTOMISE EXPORT " + typed(file) + " " + words);
        return std::pair<ReplyRecord, Customisation>{record(reply, "exported"),
                                                     parsed(contentsOf(file))};
    };

    // The codes alone: both rules, no definition, the colours their names
    // mean - and neither the control codes nor the switches, which merged
    // into a colleague's session would reset theirs.
    const auto [codesReply, codes] = exportedTo("codes.json", "CODES");
    EXPECT_EQ(field(codesReply, "definitions"), "0");
    EXPECT_EQ(field(codesReply, "codes"), "2");
    EXPECT_EQ(field(codesReply, "rules"), "2");
    EXPECT_EQ(codes.library.size(), 0u);
    EXPECT_EQ(codes.map.size(), 2u);
    EXPECT_EQ(codes.colours.size(), 1u);
    EXPECT_FALSE(codes.linework.has_value());
    EXPECT_FALSE(codes.automation.has_value());
    EXPECT_FALSE(codes.basedOn.has_value());
    EXPECT_EQ(codes.name, "Small Built In");
    EXPECT_EQ(codes.notice, std::vector<std::string>{"Written for these tests."});
    // Of its one source it holds the rules and no definition, and says so.
    EXPECT_EQ(codes.sources, (Sources{{"Small Built In", false, true, {}}}));

    // One kind of definition, then the other.
    const auto [symbolsReply, symbols] = exportedTo("symbols.json", "symbols");
    EXPECT_EQ(field(symbolsReply, "definitions"), "1");
    EXPECT_EQ(field(symbolsReply, "rules"), "0");
    EXPECT_TRUE(symbols.library.contains("TEST Peg"));
    EXPECT_EQ(symbols.library.size(), 1u);
    EXPECT_EQ(symbols.map.size(), 0u);
    EXPECT_EQ(symbols.sources, (Sources{{"Small Built In", true, false, {}}}));
    const auto [linesReply, lines] = exportedTo("lines.json", "LINESTYLES CODES");
    EXPECT_EQ(field(linesReply, "definitions"), "1");
    EXPECT_EQ(field(linesReply, "rules"), "2");
    EXPECT_TRUE(lines.library.contains("TEST Fence"));
    EXPECT_EQ(lines.sources, (Sources{{"Small Built In", true, true, {}}}));

    // Named definitions alone - and no codes unless CODES is said - under a
    // name of the export's own.
    const auto [onlyReply, only] = exportedTo("peg.json", "ONLY \"TEST Peg\" NAME Pegs");
    EXPECT_EQ(field(onlyReply, "name"), "Pegs");
    EXPECT_EQ(field(onlyReply, "definitions"), "1");
    EXPECT_EQ(field(onlyReply, "codes"), "0");
    EXPECT_EQ(only.name, "Pegs");
    EXPECT_EQ(only.library.size(), 1u);
    EXPECT_EQ(only.map.size(), 0u);
    const auto [bothReply, both] =
        exportedTo("both.json", "NAME Both CODES LINESTYLES SYMBOLS ONLY \"TEST Fence\"");
    EXPECT_EQ(field(bothReply, "definitions"), "1");
    EXPECT_EQ(field(bothReply, "rules"), "2");
    EXPECT_EQ(both.name, "Both");

    // ONLY chooses among the DEFINITIONS and a kind word among the kinds, so
    // the names are open to both kinds of definition until LINESTYLES or
    // SYMBOLS narrows them. CODES said beside ONLY adds the codes and narrows
    // nothing: it once switched both kinds of definition off, and the symbol
    // asked for by name was refused as "not being written".
    const auto [codesAndPegReply, codesAndPeg] =
        exportedTo("codes-and-peg.json", "CODES ONLY \"TEST Peg\"");
    EXPECT_EQ(field(codesAndPegReply, "definitions"), "1");
    EXPECT_EQ(field(codesAndPegReply, "codes"), "2");
    EXPECT_EQ(field(codesAndPegReply, "rules"), "2");
    EXPECT_TRUE(codesAndPeg.library.contains("TEST Peg"));
    EXPECT_EQ(codesAndPeg.library.size(), 1u);
    EXPECT_EQ(codesAndPeg.map.size(), 2u);
    // A linestyle and a symbol by name, with no kind word at all.
    const auto [twoReply, two] = exportedTo("two.json", "ONLY \"TEST Fence\" \"TEST Peg\"");
    EXPECT_EQ(field(twoReply, "definitions"), "2");
    EXPECT_EQ(field(twoReply, "rules"), "0");
    EXPECT_EQ(two.library.size(), 2u);
    EXPECT_EQ(two.map.size(), 0u);

    // The whole session does hold the settings: it is what a KEEP writes.
    const auto [wholeReply, whole] = exportedTo("whole.json", "");
    EXPECT_EQ(field(wholeReply, "definitions"), "2");
    ASSERT_TRUE(whole.linework.has_value());
    EXPECT_EQ(whole.linework->start, "S");
    EXPECT_TRUE(whole.automation.has_value());
    EXPECT_TRUE(whole.basedOn.has_value());
    EXPECT_EQ(whole.sources, (Sources{{"Small Built In", true, true, {}}}));
}

TEST(CustomisationVerbs, APartLoadedElsewhereAnswersOnlyForWhatItHolds)
{
    // One symbol of the built-in, exported for a colleague. The session is
    // "Small Built In" - definitions and rules - and the part holds one of
    // its definitions and none of its rules, so that is what the part says:
    // the source, with definitions and without rules.
    Hosted author("part-author");
    author.start();
    const fs::path project = author.scratch.root / "site.katana";
    ASSERT_TRUE(author.document.saveAs(project).ok());
    const fs::path pegs = author.scratch.root / "pegs.json";
    author.ok("CUSTOMISE EXPORT " + typed(pegs) + " ONLY \"TEST Peg\" NAME Pegs");

    const Customisation part = parsed(contentsOf(pegs));
    EXPECT_EQ(part.name, "Pegs");
    EXPECT_EQ(part.sources, (Sources{{"Small Built In", true, false, {}}}));
    EXPECT_EQ(part.notice, std::vector<std::string>{"Written for these tests."});
    ASSERT_EQ(part.library.size(), 1u);
    // The definition itself still says which customisation it came from.
    EXPECT_EQ(part.library.find("TEST Peg")->source, "Small Built In");
    EXPECT_TRUE(part.map.empty());

    // The colleague has a customisation of their own, opens the author's
    // project - which was drawn with one they do not have - and loads the
    // part. Their session holds one definition of "Small Built In" and none
    // of its rules, and says exactly that. (The part once listed the
    // author's sources as they stood: "definitions=yes rules=yes", of a
    // customisation the colleague had one symbol of.)
    Bare colleague;
    colleague.ok("CUSTOMISE " + typed(author.scratch.write("own.json", kOwn)));
    ASSERT_TRUE(colleague.document.open(project).ok());
    ASSERT_EQ(colleague.state().missingAtOpen, std::vector<std::string>{"Small Built In"});
    const std::string loaded = colleague.ok("CUSTOMISE " + typed(pegs));
    EXPECT_EQ(field(record(loaded, "loaded"), "name"), "Pegs");
    EXPECT_EQ(colleague.state().sources,
              (Sources{{"Own", true, false, {}},
                       {"Small Built In", true, false, {"Written for these tests."}}}));
    EXPECT_TRUE(contains(colleague.ok("CUSTOMISE"),
                         "source name=Own definitions=yes rules=no\n"
                         "source name=\"Small Built In\" definitions=yes rules=no\n"));
    EXPECT_EQ(colleague.document.surveyMap().size(), 0u);

    // What a save records the drawing as drawn with: both, while the
    // colleague's library holds a definition of each.
    ASSERT_TRUE(colleague.document.save().ok());
    EXPECT_EQ(colleague.document.metadata().customisation,
              (std::vector<std::string>{"Own", "Small Built In"}));
    // The one symbol removed again, nothing of "Small Built In" is left in
    // the session, and the next save no longer says the drawing is drawn
    // with it. A source that brought definitions alone is recorded only
    // while it still defines something; one said to have brought RULES is
    // taken at its word for ever, which is what the part's untrue "rules"
    // made of it.
    colleague.ok("CUSTOMISE REMOVE \"TEST Peg\"");
    ASSERT_TRUE(colleague.document.save().ok());
    EXPECT_EQ(colleague.document.metadata().customisation, std::vector<std::string>{"Own"});
}

TEST(CustomisationVerbs, APartNamesTheSourcesOfWhatItHoldsAndCarriesEveryNotice)
{
    // The built-in with Marks and Tints merged in (the head of this file):
    // sources Small Built In (definitions, rules), Marks (definitions, rules,
    // its notice) and Tints (neither kind, its notice); the session's own
    // notice is the built-in's. Two colours: the built-in's sui gas, which
    // its FE* rule names, and Tints' tint teal, which nothing names until a
    // definition below is given a pen of it.
    //
    // The rule is cad::customisationPart's - the one rule the verb and the
    // two managers' export buttons write a part by (customisation_part.hpp).
    // SEVEN EXPECTATIONS HERE CHANGED when the verb took it, each marked
    // "was" below. This test first pinned the verb's own rule, by which every
    // colour of the session went with every part, so that a table of colours
    // was a source of every part while the session had a colour at all. By
    // the one rule a part carries only the colours its written rules and
    // pens NAME - the rest would overwrite colours of whoever loads it that
    // nothing in it uses - and so a table of colours is a source only of a
    // part that carries a colour; left out, its notice is written with the
    // part's own, as that of any source left out is. What did NOT change is
    // the verb's side of the rule: a source is listed for what is written of
    // it, and no notice is lost.
    Hosted session("part-sources");
    session.start();
    session.ok("CUSTOMISE " + typed(session.scratch.write("marks.json", kMarks)) + " " +
               typed(session.scratch.write("tints.json", kTints)));
    const std::vector<std::string> marksNotice{"Marks: drawn by hand."};
    const std::vector<std::string> tintsNotice{"Tints: free to use."};
    ASSERT_EQ(session.state().sources, (Sources{{"Small Built In", true, true, {}},
                                                 {"Marks", true, true, marksNotice},
                                                 {"Tints", false, false, tintsNotice}}));
    ASSERT_EQ(session.state().colours.size(), 2u);
    const auto part = [&session](const std::string& name, const std::string& words) {
        const fs::path file = session.scratch.root / name;
        session.ok("CUSTOMISE EXPORT " + typed(file) + " " + words);
        return parsed(contentsOf(file));
    };

    // One symbol, which came from Marks and has no pen. Nothing of the
    // built-in's is written, so it is no source of the part. No colour is
    // named, so none is carried (was: both), Tints is no source (was: one,
    // "for the colours, which go with every part"), and its notice is
    // written after the part's own (was: the part's own alone).
    const Customisation cross = part("cross.json", "ONLY \"MARK Cross\" NAME Crosses");
    EXPECT_EQ(cross.sources, (Sources{{"Marks", true, false, marksNotice}}));
    EXPECT_EQ(cross.notice,
              (std::vector<std::string>{"Written for these tests.", "Tints: free to use."}));
    EXPECT_TRUE(cross.colours.empty());

    // The codes alone: every source that brought rules, said to have brought
    // rules and no more. FE* names sui gas, so that colour is carried - not
    // tint teal, which no rule names - and with a colour in the part the
    // table of colours is a source of it: nothing says whose a colour is.
    const Customisation codes = part("codes.json", "CODES");
    EXPECT_EQ(codes.sources, (Sources{{"Small Built In", false, true, {}},
                                      {"Marks", false, true, marksNotice},
                                      {"Tints", false, false, tintsNotice}}));
    EXPECT_EQ(codes.notice, std::vector<std::string>{"Written for these tests."});
    EXPECT_EQ(codes.map.size(), 3u);
    ASSERT_EQ(codes.colours.size(), 1u);
    EXPECT_EQ(codes.colours.entries().front().name, "sui gas");

    // The linestyles alone: TEST Fence, the built-in's, which has no pen.
    // Marks brought a symbol and a rule and neither is written, so it is not
    // a source of this part - and its notice is not dropped for that: it is
    // written with the part's own, after it. Nor is Tints a source, no
    // colour being carried (was: one), and its notice follows (was: absent).
    const Customisation lines = part("lines.json", "LINESTYLES");
    EXPECT_EQ(lines.sources, (Sources{{"Small Built In", true, false, {}}}));
    EXPECT_EQ(lines.notice,
              (std::vector<std::string>{"Written for these tests.", "Marks: drawn by hand.",
                                        "Tints: free to use."}));
    EXPECT_TRUE(lines.colours.empty());

    // Loaded into an empty session, the first part brings the one source it
    // lists (was: Tints too) and, of the rest, the notices it carries (was:
    // the built-in's alone).
    Bare other;
    other.ok("CUSTOMISE " + typed(session.scratch.root / "cross.json"));
    EXPECT_EQ(other.state().name, "Crosses");
    EXPECT_EQ(other.state().notice,
              (std::vector<std::string>{"Written for these tests.", "Tints: free to use."}));
    EXPECT_EQ(other.state().sources, (Sources{{"Marks", true, false, marksNotice}}));

    // A symbol made in this session, with a pen of the table's colour spelled
    // another way (one name, by the fold a colour name is compared in). The
    // part carries that one colour, as the table spells it, and so the table
    // of colours is a source of it; the symbol itself came from no
    // customisation and adds none.
    katana::entity::StyleLibrary library = session.document.styleLibrary();
    katana::entity::LineStyle painted;
    painted.name = "TEST Painted";
    painted.symbol = true;
    painted.atVertices = true;
    painted.strokes = {
        katana::entity::Stroke{.op = katana::entity::StrokeOp::Pen, .pen = "Tint_Teal"},
        katana::entity::Stroke{.op = katana::entity::StrokeOp::Circle, .radius = 1.0}};
    ASSERT_TRUE(library.add(painted).ok());
    session.document.setStyleLibrary(std::move(library));
    const Customisation teal = part("painted.json", "ONLY \"TEST Painted\"");
    EXPECT_EQ(teal.sources, (Sources{{"Tints", false, false, tintsNotice}}));
    EXPECT_EQ(teal.notice,
              (std::vector<std::string>{"Written for these tests.", "Marks: drawn by hand."}));
    ASSERT_EQ(teal.colours.size(), 1u);
    EXPECT_EQ(teal.colours.entries().front().name, "tint teal");

    // With no colour in the session at all, a table of colours is a source
    // of nothing: the same symbol again.
    session.document.setColourTable({});
    const Customisation bare = part("bare.json", "ONLY \"TEST Painted\"");
    EXPECT_TRUE(bare.sources.empty());
    EXPECT_EQ(bare.notice,
              (std::vector<std::string>{"Written for these tests.", "Marks: drawn by hand.",
                                        "Tints: free to use."}));
    EXPECT_TRUE(bare.colours.empty());
}

TEST(CustomisationVerbs, TheCodesWrittenAndReadBackInTheirPlaceLeaveTheSourcesWithWhatEachBrought)
{
    // The round trip that edits the codes: EXPORT f CODES, change the file,
    // REPLACE f. The same three-source session as above. The part lists the
    // two sources that brought rules, as bringing rules, and the table of
    // colours; read back in the place of the rules, each source is again
    // what it was - definitions it kept, rules it brought again - and in the
    // same order. (A part that listed no sources would come back as ONE
    // source of the session's name, and Marks would have lost its rules.)
    // The one thing more: a file's own notice goes with the source of its
    // name, so the built-in's entry now says the session's notice too.
    Hosted session("part-round-trip");
    session.start();
    session.ok("CUSTOMISE " + typed(session.scratch.write("marks.json", kMarks)) + " " +
               typed(session.scratch.write("tints.json", kTints)));
    const std::vector<std::string> marksNotice{"Marks: drawn by hand."};
    const std::vector<std::string> tintsNotice{"Tints: free to use."};
    const std::string file = typed(session.scratch.root / "codes.json");
    session.ok("CUSTOMISE EXPORT " + file + " CODES");
    const std::string reply = session.ok("CUSTOMISE REPLACE " + file);
    // The same 3 rules took the place of the 3: nothing is gone.
    EXPECT_EQ(recordsOf(reply, "removed"), 0u) << reply;
    EXPECT_EQ(session.document.surveyMap().size(), 3u);
    EXPECT_EQ(session.document.styleLibrary().size(), 3u);
    EXPECT_EQ(session.state().sources,
              (Sources{{"Small Built In", true, true, {"Written for these tests."}},
                       {"Marks", true, true, marksNotice},
                       {"Tints", false, false, tintsNotice}}));
    EXPECT_EQ(session.state().name, "Small Built In");
}

TEST(CustomisationVerbs, WhatAnExportRefusesWritesNoFile)
{
    Hosted session("export-refused");
    session.start();
    const fs::path out = session.scratch.root / "out.json";
    const std::string file = typed(out);

    for (const std::string& words : {std::string(), file + " EVERYTHING", file + " ONLY",
                                     file + " NAME", file + " ONLY \"TEST Peg\" NAME A B"}) {
        const katana::core::Error error = session.refused("CUSTOMISE EXPORT " + words);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << words;
        EXPECT_TRUE(error.message.starts_with("usage: CUSTOMISE EXPORT <file> ")) << error.message;
    }
    // A definition the library lacks, and one whose kind is left out: asked
    // for by name, neither is dropped silently.
    EXPECT_EQ(session.refused("CUSTOMISE EXPORT " + file + " ONLY Nowhere").code,
              ErrorCode::NotFound);
    EXPECT_EQ(session.refused("CUSTOMISE EXPORT " + file + " LINESTYLES ONLY \"TEST Peg\"").code,
              ErrorCode::InvalidArgument);
    // After ONLY every word is a definition, so a kind word put there is
    // asked for as one. It is none, and the refusal says where it belongs.
    const katana::core::Error after =
        session.refused("CUSTOMISE EXPORT " + file + " ONLY \"TEST Peg\" CODES");
    EXPECT_EQ(after.code, ErrorCode::NotFound);
    EXPECT_EQ(after.context, "CODES");
    EXPECT_EQ(after.message,
              "after ONLY every word is a definition's name, and the library has no definition "
              "of this name; a word of EXPORT's own goes before ONLY");
    // A name a project could not record.
    const katana::core::Error name = session.refused("CUSTOMISE EXPORT " + file + " NAME a/b");
    EXPECT_EQ(name.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(name.message.starts_with("NAME: ")) << name.message;
    EXPECT_FALSE(fs::exists(out));
    EXPECT_FALSE(fs::exists(fs::path(out).concat(".tmp")));

    // Nothing loaded: nothing to export. Rules set with no customisation
    // ever installed have no name to be written under, until one is given.
    Bare bare;
    EXPECT_EQ(bare.refused("CUSTOMISE EXPORT " + file).message,
              "no customisation is loaded, so there is nothing to export");
    bare.document.setSurveyMap(parsed(kSite).map);
    const katana::core::Error unnamed = bare.refused("CUSTOMISE EXPORT " + file);
    EXPECT_EQ(unnamed.code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(unnamed.message, "CUSTOMISE EXPORT <file> NAME <name>"))
        << unnamed.message;
    EXPECT_FALSE(fs::exists(out));
    EXPECT_EQ(field(record(bare.ok("CUSTOMISE EXPORT " + file + " NAME Mine"), "exported"),
                    "rules"),
              "3");
    EXPECT_EQ(parsed(contentsOf(out)).name, "Mine");
}

// ---- reset, keep, revert -------------------------------------------------------------------

TEST(CustomisationVerbs, KeepWritesTheSessionAndTheNextStartGivesIt)
{
    Hosted session("keep");
    session.start();
    ASSERT_TRUE(session.state().kept);
    session.ok("CUSTOMISE SET auto.codes=off");
    EXPECT_FALSE(session.state().kept);

    const ReplyRecord kept = record(session.ok("CUSTOMISE KEEP"), "kept");
    EXPECT_EQ(field(kept, "file"), shown(session.keptFile));
    EXPECT_EQ(field(kept, "written"), "yes");
    EXPECT_EQ(field(kept, "name"), "Small Built In");
    EXPECT_EQ(field(kept, "definitions"), "2");
    EXPECT_EQ(field(kept, "codes"), "2");
    EXPECT_EQ(field(kept, "rules"), "2");
    EXPECT_EQ(field(kept, "backup"), "none");
    EXPECT_TRUE(session.state().kept);
    // The folder was made for it, and nothing is left beside it.
    ASSERT_TRUE(fs::exists(session.keptFile));
    EXPECT_FALSE(fs::exists(fs::path(session.keptFile).concat(".tmp")));
    EXPECT_FALSE(fs::exists(session.backup()));

    // What was written is the session, and says what it was made from: the
    // built-in, by name and by the digest of its bytes.
    const Customisation written = parsed(contentsOf(session.keptFile));
    EXPECT_TRUE(written == session.document.customisation());
    ASSERT_TRUE(written.basedOn.has_value());
    EXPECT_EQ(written.basedOn->name, "Small Built In");
    EXPECT_EQ(written.basedOn->digest, fnv1a(kBuiltIn));
    ASSERT_TRUE(written.automation.has_value());
    EXPECT_FALSE(written.automation->codesOnSurveyImport);

    // A second session, started by the same host: the kept one, not the
    // built-in.
    Document next;
    const auto start = katana::cad::startCustomisation(next, session.host);
    EXPECT_EQ(start.installed, CustomisationOrigin::Kept);
    EXPECT_FALSE(start.keptFromAnotherBuiltIn);
    EXPECT_FALSE(next.customisationState().automation.codesOnSurveyImport);
}

TEST(CustomisationVerbs, ALaterKeepLeavesTheEarlierFileBesideItAndRevertReadsTheKeptOne)
{
    Hosted session("keep-twice");
    session.start();
    session.ok("CUSTOMISE SET auto.codes=off");
    session.ok("CUSTOMISE KEEP");
    const std::string first = contentsOf(session.keptFile);

    session.ok("CUSTOMISE SET linework.start=S");
    const ReplyRecord kept = record(session.ok("CUSTOMISE KEEP"), "kept");
    EXPECT_EQ(field(kept, "written"), "yes");
    EXPECT_EQ(field(kept, "backup"), shown(session.backup()));
    EXPECT_EQ(contentsOf(session.backup()), first);
    EXPECT_NE(contentsOf(session.keptFile), first);

    // An edit that is not kept, then back to what is.
    session.ok("CUSTOMISE SET auto.linework=off linework.start=BEGIN");
    EXPECT_FALSE(session.state().kept);
    const ReplyRecord reverted = record(session.ok("CUSTOMISE REVERT"), "reverted");
    EXPECT_EQ(field(reverted, "file"), shown(session.keptFile));
    EXPECT_EQ(field(reverted, "name"), "Small Built In");
    EXPECT_EQ(field(reverted, "rules"), "2");
    EXPECT_EQ(session.state().origin, CustomisationOrigin::Kept);
    EXPECT_TRUE(session.state().kept);
    EXPECT_EQ(session.state().linework.start, "S");
    EXPECT_TRUE(session.state().automation.lineworkOnSurveyImport);
    EXPECT_FALSE(session.state().automation.codesOnSurveyImport);

    // A third keep: the .bak is the file as it was before THIS keep, and the
    // .bak that was there is replaced by it.
    const std::string second = contentsOf(session.keptFile);
    session.ok("CUSTOMISE SET auto.linework=off");
    EXPECT_EQ(field(record(session.ok("CUSTOMISE KEEP"), "kept"), "backup"),
              shown(session.backup()));
    EXPECT_EQ(contentsOf(session.backup()), second);
    EXPECT_NE(contentsOf(session.keptFile), second);
    EXPECT_FALSE(fs::exists(fs::path(session.keptFile).concat(".tmp")));
}

TEST(CustomisationVerbs, ResetGivesTheBuiltInAndKeepingItRetiresTheKeptFile)
{
    Hosted session("reset");
    session.start();
    session.ok("CUSTOMISE " + typed(session.scratch.write("site.json", kSite)));
    session.ok("CUSTOMISE SET linework.start=S auto.codes=off");
    session.ok("CUSTOMISE KEEP");
    const std::string keptText = contentsOf(session.keptFile);

    // The built-in, whole: Site's definitions and rules are gone, and the
    // control code and the switch are the built-in's again - which says
    // neither, so the defaults. Not "kept": a kept file is there, and the
    // next start would read that.
    EXPECT_EQ(session.ok("CUSTOMISE RESET"),
              "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=no");
    EXPECT_EQ(session.state().origin, CustomisationOrigin::BuiltIn);
    EXPECT_FALSE(session.state().kept);
    EXPECT_EQ(session.state().linework.start, "ST");
    EXPECT_TRUE(session.state().automation.codesOnSurveyImport);
    EXPECT_EQ(session.document.surveyMap().size(), 2u);
    ASSERT_TRUE(session.state().basedOn.has_value());
    EXPECT_EQ(session.state().basedOn->digest, fnv1a(kBuiltIn));
    EXPECT_EQ(contentsOf(session.keptFile), keptText) << "a RESET writes nothing";

    // Kept as it is, a copy of the built-in would be read at every later
    // start in the place of whatever the built-in had become. So nothing is
    // written, and the kept file is set aside.
    const ReplyRecord kept = record(session.ok("CUSTOMISE KEEP"), "kept");
    EXPECT_EQ(field(kept, "file"), shown(session.keptFile));
    EXPECT_EQ(field(kept, "written"), "no");
    EXPECT_EQ(field(kept, "reason"), "the-session-is-the-built-in");
    EXPECT_EQ(field(kept, "backup"), shown(session.backup()));
    EXPECT_FALSE(fs::exists(session.keptFile));
    EXPECT_EQ(contentsOf(session.backup()), keptText);
    EXPECT_TRUE(session.state().kept);

    // The next start gives the built-in, and a KEEP there has nothing to do.
    Document next;
    EXPECT_EQ(katana::cad::startCustomisation(next, session.host).installed,
              CustomisationOrigin::BuiltIn);
    const ReplyRecord again = record(session.ok("CUSTOMISE KEEP"), "kept");
    EXPECT_EQ(field(again, "written"), "no");
    EXPECT_EQ(field(again, "backup"), "none");
    EXPECT_FALSE(fs::exists(session.keptFile));
    // With no kept file, a RESET is what the next start gives.
    EXPECT_EQ(session.ok("CUSTOMISE RESET"),
              "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=yes");
}

TEST(CustomisationVerbs, KeepIsRefusedWhenTheKeptFileChangedOnDiskSinceTheSessionReadIt)
{
    Hosted session("keep-changed");
    session.start();
    session.ok("CUSTOMISE SET auto.codes=off");
    session.ok("CUSTOMISE KEEP");

    // Another Katana keeps its own there.
    const std::string others =
        R"({"format": "katana-customisation", "version": 1, "name": "Theirs",)"
        R"( "codes": [{"key": "ZZ*", "sets": "feature", "layer": "THEIRS"}]})";
    std::ofstream(session.keptFile, std::ios::binary) << others;

    session.ok("CUSTOMISE SET linework.start=S");
    const katana::core::Error error = session.refused("CUSTOMISE KEEP");
    EXPECT_EQ(error.code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(error.message, "changed on disk since this session read it"))
        << error.message;
    EXPECT_TRUE(contains(error.message, "CUSTOMISE REVERT")) << error.message;
    EXPECT_EQ(contentsOf(session.keptFile), others) << "what the other kept is not written over";
    EXPECT_FALSE(fs::exists(session.backup()));
    EXPECT_FALSE(session.state().kept);

    // REVERT reads it; from then on it is the file this session saw, and a
    // KEEP writes over it, leaving it beside as the .bak.
    session.ok("CUSTOMISE REVERT");
    EXPECT_EQ(session.state().name, "Theirs");
    session.ok("CUSTOMISE SET auto.linework=off");
    EXPECT_EQ(field(record(session.ok("CUSTOMISE KEEP"), "kept"), "written"), "yes");
    EXPECT_EQ(contentsOf(session.backup()), others);
}

TEST(CustomisationVerbs, KeepIsRefusedOverAFileThatAppearedDoesNotReadOrIsNewer)
{
    // It was not there when the session began, and is now.
    {
        Hosted session("keep-appeared");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        fs::create_directories(session.keptFile.parent_path());
        std::ofstream(session.keptFile, std::ios::binary) << kSite;
        const katana::core::Error error = session.refused("CUSTOMISE KEEP");
        EXPECT_TRUE(contains(error.message, "changed on disk")) << error.message;
        EXPECT_EQ(contentsOf(session.keptFile), kSite);
    }
    // It was there, and is not a customisation: its owner may mean to mend it.
    {
        Hosted session("keep-unreadable");
        fs::create_directories(session.keptFile.parent_path());
        std::ofstream(session.keptFile, std::ios::binary) << "{ not json";
        session.start();
        EXPECT_EQ(session.state().origin, CustomisationOrigin::BuiltIn)
            << "the built-in stands in for a kept file that does not read";
        session.ok("CUSTOMISE SET auto.codes=off");
        const katana::core::Error error = session.refused("CUSTOMISE KEEP");
        EXPECT_EQ(error.code, ErrorCode::InvalidState);
        EXPECT_TRUE(contains(error.message, "does not read as a Katana customisation"))
            << error.message;
        EXPECT_EQ(contentsOf(session.keptFile), "{ not json");
    }
    // It was written by a newer Katana, and holds what this one cannot write
    // back.
    {
        Hosted session("keep-newer");
        const std::string newer =
            R"({"format": "katana-customisation", "version": 2, "name": "Later"})";
        fs::create_directories(session.keptFile.parent_path());
        std::ofstream(session.keptFile, std::ios::binary) << newer;
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        const katana::core::Error error = session.refused("CUSTOMISE KEEP");
        EXPECT_EQ(error.code, ErrorCode::InvalidState);
        EXPECT_TRUE(contains(error.message, "was written by a newer Katana")) << error.message;
        EXPECT_EQ(contentsOf(session.keptFile), newer);
        // And REVERT says why it cannot read it.
        EXPECT_EQ(session.refused("CUSTOMISE REVERT").code, ErrorCode::Unsupported);
    }
    // Something is there that cannot be read at all - here a folder of the
    // kept file's own name. It is not written over unseen either.
    {
        Hosted session("keep-cannot-read");
        fs::create_directories(session.keptFile);
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        const katana::core::Error error = session.refused("CUSTOMISE KEEP");
        EXPECT_EQ(error.code, ErrorCode::InvalidState);
        EXPECT_EQ(error.message,
                  "CUSTOMISE KEEP: the kept customisation file cannot be read, so it is not "
                  "written over: the file cannot be opened: it is a directory");
        EXPECT_TRUE(fs::is_directory(session.keptFile));
        EXPECT_FALSE(session.state().kept);
    }
}

TEST(CustomisationVerbs, AWriteThatFailsIsRefusedAndLeavesWhatWasThere)
{
    // EXPORT to a name a folder has: the text is written beside it and cannot
    // be put in its place. The folder stands, and nothing is left beside it.
    {
        Hosted session("write-export");
        session.start();
        const fs::path folder = session.scratch.root / "a-folder";
        fs::create_directories(folder);
        const katana::core::Error error = session.refused("CUSTOMISE EXPORT " + typed(folder));
        EXPECT_EQ(error.code, ErrorCode::FileExportFailure);
        EXPECT_EQ(error.message, "the file cannot be written");
        EXPECT_TRUE(contains(error.context, "a-folder")) << error.context;
        EXPECT_TRUE(fs::is_directory(folder));
        EXPECT_FALSE(fs::exists(fs::path(folder).concat(".tmp")));
    }
    // KEEP where the kept file's folder cannot be made: a file stands where
    // the folder would be. The session stays not kept.
    {
        Hosted session("write-keep");
        const std::string was = "a file, where the kept file's folder would be";
        const fs::path blocker = session.scratch.write("blocker", was);
        session.keptFile = blocker / "customisation.json";
        session.host.keptFile = session.keptFile;
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        const katana::core::Error error = session.refused("CUSTOMISE KEEP");
        EXPECT_EQ(error.code, ErrorCode::FileExportFailure);
        EXPECT_EQ(error.message, "the file cannot be written");
        EXPECT_EQ(contentsOf(blocker), was);
        EXPECT_FALSE(session.state().kept);
    }
    // KEEP where the file that is there cannot be kept beside it: a folder
    // has the .bak's name. Nothing is written - the kept file is as it was.
    {
        Hosted session("write-backup");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        session.ok("CUSTOMISE KEEP");
        const std::string first = contentsOf(session.keptFile);
        fs::create_directories(session.backup());
        session.ok("CUSTOMISE SET linework.start=S");
        const katana::core::Error error = session.refused("CUSTOMISE KEEP");
        EXPECT_EQ(error.code, ErrorCode::FileExportFailure);
        EXPECT_EQ(error.message,
                  "the file that is there could not be kept beside it, so nothing was written");
        EXPECT_EQ(contentsOf(session.keptFile), first);
        EXPECT_FALSE(fs::exists(fs::path(session.keptFile).concat(".tmp")));
        EXPECT_FALSE(session.state().kept);
    }
}

TEST(CustomisationVerbs, KeepRefusesASessionWithNoNameUntilAnExportAndAReplaceNameIt)
{
    // Rules made with no customisation ever installed - an editor's, on an
    // empty session: there is a customisation, and it has no name.
    Hosted session("keep-unnamed", false, true);
    session.start();
    session.document.setSurveyMap(parsed(kSite).map);
    ASSERT_EQ(session.state().name, "");

    const katana::core::Error error = session.refused("CUSTOMISE KEEP");
    EXPECT_EQ(error.code, ErrorCode::InvalidState);
    EXPECT_EQ(error.message,
              "CUSTOMISE KEEP: the session's customisation has no name to be kept under; "
              "CUSTOMISE EXPORT <file> NAME <name> writes it under one, and CUSTOMISE REPLACE "
              "<file> then gives the session that name");
    EXPECT_FALSE(fs::exists(session.keptFile));

    // Done as it says: the session is "Mine", and is kept. Site's 3 rules
    // over 3 keys, and no definition - only its rules were set.
    const std::string file = typed(session.scratch.root / "mine.json");
    session.ok("CUSTOMISE EXPORT " + file + " NAME Mine");
    session.ok("CUSTOMISE REPLACE " + file);
    EXPECT_EQ(session.state().name, "Mine");
    const ReplyRecord kept = record(session.ok("CUSTOMISE KEEP"), "kept");
    EXPECT_EQ(field(kept, "written"), "yes");
    EXPECT_EQ(field(kept, "name"), "Mine");
    EXPECT_EQ(field(kept, "definitions"), "0");
    EXPECT_EQ(field(kept, "codes"), "3");
    EXPECT_EQ(field(kept, "rules"), "3");
    EXPECT_EQ(parsed(contentsOf(session.keptFile)).name, "Mine");
}

TEST(CustomisationVerbs, WithNoHostResetKeepAndRevertAreRefusedByNameAndTheRestWorks)
{
    const Scratch scratch("no-host");
    Bare session;
    session.loadBuiltIn(scratch);
    const Customisation before = session.document.customisation();
    for (const char* word : {"RESET", "KEEP", "REVERT"}) {
        const katana::core::Error error = session.refused(std::string("CUSTOMISE ") + word);
        EXPECT_EQ(error.code, ErrorCode::InvalidState) << word;
        EXPECT_TRUE(error.message.starts_with(std::string("CUSTOMISE ") + word + " needs "))
            << error.message;
    }
    EXPECT_TRUE(session.document.customisation() == before);
    // KEEP and REVERT say what would name a kept file.
    EXPECT_TRUE(contains(session.refused("CUSTOMISE KEEP").message, "KATANA_CUSTOMISATION"));

    // Everything else is a session's own.
    EXPECT_TRUE(session.ok("CUSTOMISE").starts_with("2 linestyle and symbol definitions"));
    EXPECT_TRUE(session.ok("CUSTOMISE JSON").starts_with("{\n"));
    session.ok("CUSTOMISE " + typed(scratch.write("site.json", kSite)));
    session.ok("CUSTOMISE SET auto.codes=off");
    session.ok("CUSTOMISE REMOVE CODE GT*");
    session.ok("CUSTOMISE REMOVE \"SITE Tree\" FORCE");
    session.ok("CUSTOMISE EXPORT " + typed(scratch.root / "out.json"));
    EXPECT_EQ(parsed(contentsOf(scratch.root / "out.json")).map.size(), 3u);
}

TEST(CustomisationVerbs, AHostWithNoBuiltInOrNoKeptFileRefusesTheVerbThatNeedsIt)
{
    // A build with no built-in: RESET has nothing to give.
    Hosted noBuiltIn("no-built-in", false, true);
    noBuiltIn.start();
    EXPECT_EQ(noBuiltIn.refused("CUSTOMISE RESET").message,
              "CUSTOMISE RESET needs a built-in customisation, and this program has none");
    // Nothing is loaded, so nothing can be kept, and nothing is kept to go
    // back to.
    EXPECT_EQ(noBuiltIn.refused("CUSTOMISE KEEP").message,
              "CUSTOMISE KEEP: no customisation is loaded, so there is nothing to keep");
    const katana::core::Error revert = noBuiltIn.refused("CUSTOMISE REVERT");
    EXPECT_EQ(revert.code, ErrorCode::NotFound);
    EXPECT_TRUE(contains(revert.message, "no customisation is kept")) << revert.message;
    EXPECT_FALSE(fs::exists(noBuiltIn.keptFile));

    // A built-in that was asked for and did not read: RESET says why.
    Hosted damaged("damaged-built-in", false, false);
    damaged.host.builtIn.problem = "the built-in customisation does not read: line 3";
    damaged.start();
    EXPECT_EQ(damaged.refused("CUSTOMISE RESET").message,
              "CUSTOMISE RESET cannot use the built-in customisation: the built-in customisation "
              "does not read: line 3");

    // A front end that keeps nothing (katana_cli without the variable).
    Hosted noKept("no-kept-file", true, false);
    noKept.start();
    for (const char* word : {"KEEP", "REVERT"}) {
        const katana::core::Error error = noKept.refused(std::string("CUSTOMISE ") + word);
        EXPECT_EQ(error.code, ErrorCode::InvalidState) << word;
        EXPECT_EQ(error.message,
                  std::string("CUSTOMISE ") + word +
                      " needs a kept customisation file, and this session has none; the "
                      "environment variable KATANA_CUSTOMISATION names one");
    }
    // RESET needs only the built-in, and with no kept file the session is
    // what the next start gives.
    noKept.ok("CUSTOMISE SET auto.codes=off");
    EXPECT_EQ(noKept.ok("CUSTOMISE RESET"),
              "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=yes");
}

// ---- remove --------------------------------------------------------------------------------

TEST(CustomisationVerbs, RemovingADefinitionInUseIsRefusedSayingWhoUsesItUnlessForced)
{
    Hosted session("remove");
    session.start();
    // A style of the drawing names the fence as its linetype; the FE* rule
    // (rule 0, a feature rule) names it as its linestyle.
    session.ok("STYLE NEW Fence");
    session.ok("STYLE SET Fence linetype \"TEST Fence\"");

    const katana::core::Error error = session.refused("CUSTOMISE REMOVE \"TEST Fence\"");
    EXPECT_EQ(error.code, ErrorCode::InvalidState);
    EXPECT_EQ(error.message,
              "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was removed; "
              "FORCE removes what is used too, and what names it then draws plain\n"
              "  \"TEST Fence\": rule #0 FE* (feature) names it\n"
              "  \"TEST Fence\": the drawing's style \"Fence\" names it");
    EXPECT_TRUE(session.document.styleLibrary().contains("TEST Fence"));
    EXPECT_EQ(session.state().origin, CustomisationOrigin::BuiltIn);

    // The peg is named by the PG* rule (rule 1, a symbol rule) as its symbol.
    // One used and one not, in one line: neither goes.
    katana::entity::StyleLibrary library = session.document.styleLibrary();
    katana::entity::LineStyle spare;
    spare.name = "TEST Spare";
    ASSERT_TRUE(katana::entity::addOrReplace(library, spare).ok());
    session.document.setStyleLibrary(library);
    const katana::core::Error two = session.refused("CUSTOMISE REMOVE \"TEST Spare\" \"TEST Peg\"");
    EXPECT_EQ(two.message,
              "CUSTOMISE REMOVE: 1 of the 2 definitions named are in use, so nothing was "
              "removed; FORCE removes what is used too, and what names it then draws plain\n"
              "  \"TEST Peg\": rule #1 PG* (symbol) names it");
    EXPECT_EQ(session.document.styleLibrary().size(), 3u);

    // One nothing names goes without FORCE; a name given twice is one name.
    EXPECT_EQ(session.ok("CUSTOMISE REMOVE \"TEST Spare\" \"TEST Spare\""),
              "removed definition=\"TEST Spare\" rules=0 styles=0 layers=0");
    // Forced, the reply still says who names what is gone.
    EXPECT_EQ(session.ok("CUSTOMISE REMOVE \"TEST Fence\" \"TEST Peg\" force"),
              "removed definition=\"TEST Fence\" rules=1 styles=1 layers=0\n"
              "removed definition=\"TEST Peg\" rules=1 styles=0 layers=0");
    EXPECT_TRUE(session.document.styleLibrary().empty());
    EXPECT_EQ(session.document.surveyMap().size(), 2u) << "the rules are not removed with them";
    EXPECT_EQ(session.state().origin, CustomisationOrigin::Edited);
    EXPECT_FALSE(session.state().kept);
}

TEST(CustomisationVerbs, ALayersLinetypeAndAStylesSymbolAreUsesOfADefinitionToo)
{
    // Two definitions no rule names: a linestyle a LAYER is drawn with, and a
    // symbol a STYLE draws its points with. An entity with no style of its
    // own is drawn with its layer's linetype, so the layer holds the
    // definition in place exactly as a style does - and was once not asked:
    // the definition went without FORCE and without a word.
    Hosted session("remove-layer");
    session.start();
    addDefinition(session.document, "TEST Hedge");
    addDefinition(session.document, "TEST Stake", true);
    session.ok("LAYER NEW trees");
    session.ok("LAYER LTYPE trees \"TEST Hedge\"");
    session.ok("STYLE NEW Mark");
    session.ok("STYLE SET Mark symbol \"TEST Stake\"");

    const katana::core::Error hedge = session.refused("CUSTOMISE REMOVE \"TEST Hedge\"");
    EXPECT_EQ(hedge.code, ErrorCode::InvalidState);
    EXPECT_EQ(hedge.message,
              "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was removed; "
              "FORCE removes what is used too, and what names it then draws plain\n"
              "  \"TEST Hedge\": the drawing's layer \"trees\" names it");
    const katana::core::Error stake = session.refused("CUSTOMISE REMOVE \"TEST Stake\"");
    EXPECT_EQ(stake.message,
              "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was removed; "
              "FORCE removes what is used too, and what names it then draws plain\n"
              "  \"TEST Stake\": the drawing's style \"Mark\" names it");
    EXPECT_EQ(session.document.styleLibrary().size(), 4u);

    EXPECT_EQ(session.ok("CUSTOMISE REMOVE \"TEST Hedge\" \"TEST Stake\" FORCE"),
              "removed definition=\"TEST Hedge\" rules=0 styles=0 layers=1\n"
              "removed definition=\"TEST Stake\" rules=0 styles=1 layers=0");
    EXPECT_EQ(session.document.styleLibrary().size(), 2u);
}

TEST(CustomisationVerbs, AStyleThatNamesADefinitionBothWaysIsCitedAndCountedOnce)
{
    // REMOVE asks cad::definitionUsers who names a definition - the function
    // the window's definition editor lists them with. That answer says HOW
    // each user names it, so a style whose linetype AND symbol are the
    // definition is in it twice; the verb says only THAT a style names it.
    // Three styles, made as a file's are (no verb checks what a style names):
    // Alpha draws its points with the stake, Both names it both ways, Zed
    // names it as its linetype. By hand: 3 styles, each cited once, in name
    // order.
    Hosted session("remove-both-ways");
    session.start();
    addDefinition(session.document, "TEST Stake", true);
    const auto style = [&session](const char* name, const char* linetype, const char* symbol) {
        katana::entity::Style made;
        made.name = name;
        made.linetype = linetype;
        made.symbol = symbol;
        ASSERT_TRUE(session.document.execute(katana::commands::createStyle(made)).ok()) << name;
    };
    style("Zed", "TEST Stake", "");
    style("Both", "TEST Stake", "TEST Stake");
    style("Alpha", "", "TEST Stake");
    const katana::cad::DefinitionUsers users =
        katana::cad::definitionUsers(session.document, "TEST Stake");
    ASSERT_EQ(users.linetypeStyles, (std::vector<std::string>{"Both", "Zed"}));
    ASSERT_EQ(users.symbolStyles, (std::vector<std::string>{"Alpha", "Both"}));

    const katana::core::Error refusal = session.refused("CUSTOMISE REMOVE \"TEST Stake\"");
    EXPECT_EQ(refusal.code, ErrorCode::InvalidState);
    EXPECT_EQ(refusal.message,
              "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was removed; "
              "FORCE removes what is used too, and what names it then draws plain\n"
              "  \"TEST Stake\": the drawing's style \"Alpha\" names it\n"
              "  \"TEST Stake\": the drawing's style \"Both\" names it\n"
              "  \"TEST Stake\": the drawing's style \"Zed\" names it");
    EXPECT_EQ(session.ok("CUSTOMISE REMOVE \"TEST Stake\" FORCE"),
              "removed definition=\"TEST Stake\" rules=0 styles=3 layers=0");
}

TEST(CustomisationVerbs, TheLineAnEditorBuildsToRemoveADefinitionIsReadAsThatDefinitionAlone)
{
    // The window's definition editor does not remove a definition: it runs
    // the line cad::removeDefinitionLine builds (definition_edit.hpp), and
    // this verb reads it. The two were written side by side, so here they
    // meet: every name that function will put on a line is read back by the
    // verb as that one definition - and the names it refuses to put on one
    // are exactly the words this verb would read as its own.
    //
    // By hand from the tokenizer's contract (cad/annotation/command_words.hpp:
    // it splits on blanks, groups quoted words, removes the quotes and has no
    // escape) and from this family's grammar (CODE is a keyword only as
    // REMOVE's first word, FORCE only as its last; every other word of the
    // family is a keyword only as CUSTOMISE's first). So inside the quotes a
    // name may hold anything but a double quote or a line break - blanks at
    // its ends, two in a row, '#', ';', '=', an apostrophe, text outside
    // ASCII - and may BE any word of the family but those two.
    Bare session;
    const std::vector<std::string> names{
        "TEST Valve", " led by a blank", "two  blanks", "ends in one ", "REMOVE", "SET",
        "ONLY",       "JSON",            "replace",     "CODES",        "KEEP",   "Code Red",
        "Forced",     "FORCE main",      "main FORCE",  "7 up",         "a#b",    "#first",
        "a;b",        "auto.codes=on",   "it's",        "\xC3\x9C" "ber Mark"};
    for (const std::string& name : names) {
        addDefinition(session.document, name);
    }
    ASSERT_EQ(session.document.styleLibrary().size(), names.size());

    std::size_t left = names.size();
    bool force = false;
    for (const std::string& name : names) {
        // Plain and forced by turns: both lines are the editor's.
        const auto line = katana::cad::removeDefinitionLine(name, force);
        ASSERT_TRUE(line.ok()) << name << ": " << line.error().describe();
        EXPECT_EQ(*line, "CUSTOMISE REMOVE \"" + name + "\"" + (force ? " FORCE" : ""));
        const std::string reply = session.ok(*line);
        EXPECT_TRUE(reply.starts_with("removed definition=")) << name << ": " << reply;
        EXPECT_TRUE(reply.ends_with(" rules=0 styles=0 layers=0")) << name << ": " << reply;
        EXPECT_FALSE(session.document.styleLibrary().contains(name)) << name;
        EXPECT_EQ(session.document.styleLibrary().size(), --left) << name;
        force = !force;
    }

    // The two words no line can carry as a name, in any case - and why: the
    // verb reads each as its own word, so the line an editor would have built
    // does something else or nothing. A library can hold such a name, out of
    // a file.
    for (const char* word : {"CODE", "code", "FORCE", "Force"}) {
        addDefinition(session.document, word);
        for (const bool forced : {false, true}) {
            const auto line = katana::cad::removeDefinitionLine(word, forced);
            ASSERT_FALSE(line.ok()) << word;
            EXPECT_EQ(line.error().code, ErrorCode::InvalidArgument) << word;
        }
        // Written out all the same, the plain line is a usage refusal: CODE
        // with no key after it, or FORCE with no definition before it.
        const katana::core::Error plain =
            session.refused(std::string("CUSTOMISE REMOVE \"") + word + "\"");
        EXPECT_EQ(plain.code, ErrorCode::InvalidArgument) << word;
        EXPECT_TRUE(plain.message.starts_with("usage: CUSTOMISE REMOVE ")) << plain.message;
        EXPECT_TRUE(session.document.styleLibrary().contains(word)) << word;
    }
    // And "remove the definition CODE anyway" is read as "remove the survey
    // code FORCE", which no rule has.
    const katana::core::Error misread = session.refused("CUSTOMISE REMOVE \"CODE\" FORCE");
    EXPECT_EQ(misread.code, ErrorCode::NotFound);
    EXPECT_EQ(misread.context, "\"FORCE\"");
    EXPECT_TRUE(session.document.styleLibrary().contains("CODE"));
}

TEST(CustomisationVerbs, RemovingWhatIsNotThereRemovesNothing)
{
    Hosted session("remove-unknown");
    session.start();
    const katana::core::Error error =
        session.refused("CUSTOMISE REMOVE \"TEST Peg\" Nowhere \"Not Here\" FORCE");
    EXPECT_EQ(error.code, ErrorCode::NotFound);
    EXPECT_EQ(error.context, "\"Nowhere\", \"Not Here\"");
    EXPECT_EQ(session.document.styleLibrary().size(), 2u);

    const katana::core::Error code = session.refused("CUSTOMISE REMOVE CODE PG* pg* ZZ*");
    EXPECT_EQ(code.code, ErrorCode::NotFound);
    // A key is matched as it is written, as a code is: pg* is not PG*.
    EXPECT_EQ(code.context, "\"pg*\", \"ZZ*\"");
    EXPECT_EQ(session.document.surveyMap().size(), 2u);

    for (const char* line : {"CUSTOMISE REMOVE", "CUSTOMISE REMOVE CODE", "CUSTOMISE REMOVE FORCE"}) {
        const katana::core::Error usage = session.refused(line);
        EXPECT_EQ(usage.code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(usage.message.starts_with("usage: CUSTOMISE REMOVE ")) << usage.message;
    }
    EXPECT_EQ(session.state().origin, CustomisationOrigin::BuiltIn);
}

TEST(CustomisationVerbs, RemoveCodeTakesEveryRuleOfEachKey)
{
    // Site merged in: FE* feature, PG* symbol, TR* symbol, GT* feature. A
    // second FE* rule in another section, so that one key has two.
    Hosted session("remove-code");
    session.start();
    session.ok("CUSTOMISE " + typed(session.scratch.write("site.json", kSite)));
    katana::entity::SurveyMap map = session.document.surveyMap();
    katana::entity::SurveyRule surface;
    surface.key = "FE*";
    surface.section = katana::entity::SurveySection::Tinable;
    surface.tinable = true;
    ASSERT_TRUE(map.add(surface).ok());
    session.document.setSurveyMap(map);
    ASSERT_EQ(session.document.surveyMap().size(), 5u);

    EXPECT_EQ(session.ok("CUSTOMISE REMOVE CODE FE* GT* FE*"),
              "removed code=FE* rules=2\nremoved code=GT* rules=1");
    const auto& rules = session.document.surveyMap().rules();
    ASSERT_EQ(rules.size(), 2u);
    EXPECT_EQ(rules[0].key, "PG*");
    EXPECT_EQ(rules[1].key, "TR*");
    EXPECT_EQ(session.document.styleLibrary().size(), 3u) << "definitions are not codes";
}

// ---- settings ------------------------------------------------------------------------------

TEST(CustomisationVerbs, SetChangesTheSwitchesAndTheControlCodesAndSaysThemBack)
{
    Hosted session("set");
    session.start();
    // Keys in any case; the reply in the form the keys are documented in.
    EXPECT_EQ(session.ok("CUSTOMISE SET auto.codes=off AUTO.LINEWORK=Off linework.start=S "
                         "linework.arcStart=PC"),
              "set auto.codes=off auto.linework=off linework.start=S linework.arcstart=PC");
    EXPECT_FALSE(session.state().automation.codesOnSurveyImport);
    EXPECT_FALSE(session.state().automation.lineworkOnSurveyImport);
    EXPECT_EQ(session.state().linework.start, "S");
    EXPECT_EQ(session.state().linework.arcStart, "PC");
    EXPECT_EQ(session.state().linework.end, "END") << "what was not named is as it was";
    EXPECT_EQ(session.state().origin, CustomisationOrigin::Edited);
    EXPECT_FALSE(session.state().kept);

    // An empty spelling switches a control off.
    EXPECT_EQ(session.ok("CUSTOMISE SET linework.join= auto.codes=on"),
              "set linework.join=\"\" auto.codes=on");
    EXPECT_EQ(session.state().linework.join, "");
    EXPECT_TRUE(session.state().automation.codesOnSurveyImport);

    // The report says both in the words SET takes, so either line can be
    // typed back - and typed back, changes nothing.
    const std::string report = session.ok("CUSTOMISE");
    EXPECT_TRUE(contains(report, "automation auto.codes=on auto.linework=off\n")) << report;
    const std::string linework =
        "linework.start=S linework.end=END linework.close=CL linework.arcstart=PC "
        "linework.arcend=EC linework.join=\"\" linework.rectangle=RECT";
    EXPECT_TRUE(contains(report, "linework " + linework + "\n")) << report;
    const std::uint64_t generation = session.document.customisationGeneration();
    session.ok("CUSTOMISE SET " + linework + " auto.codes=on auto.linework=off");
    EXPECT_EQ(session.document.customisationGeneration(), generation);
}

TEST(CustomisationVerbs, OneRefusedItemSetsNoneOfThem)
{
    Hosted session("set-refused");
    session.start();
    const std::uint64_t generation = session.document.customisationGeneration();
    // Two controls spelled alike mean neither; a spelling with a blank can
    // never be met in a field code. Both are refused by the one judge of
    // linework codes, and the switch named beside them is not turned.
    for (const char* line : {"CUSTOMISE SET auto.codes=off linework.start=END",
                             "CUSTOMISE SET auto.codes=off linework.close=end",
                             "CUSTOMISE SET auto.codes=off \"linework.start=A B\""}) {
        const katana::core::Error error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(error.message.starts_with("CUSTOMISE SET: the linework codes would be "
                                              "refused, so nothing was set: "))
            << error.message;
    }
    // What is not a key, not a value, or not an item at all.
    EXPECT_EQ(session.refused("CUSTOMISE SET auto.linework=off auto.codes=maybe").message,
              "auto.codes is on or off");
    EXPECT_TRUE(session.refused("CUSTOMISE SET auto.codes=off linework.finish=F")
                    .message.starts_with("not a SET key; usage: CUSTOMISE SET "));
    EXPECT_TRUE(session.refused("CUSTOMISE SET auto.codes=off name=Mine")
                    .message.starts_with("not a SET key; "));
    EXPECT_TRUE(session.refused("CUSTOMISE SET auto.codes=off linework.start")
                    .message.starts_with("a SET item is key=value; "));
    // A value with no key before its '='.
    EXPECT_TRUE(session.refused("CUSTOMISE SET auto.codes=off =S")
                    .message.starts_with("a SET item is key=value; "));
    EXPECT_TRUE(session.refused("CUSTOMISE SET").message.starts_with("usage: CUSTOMISE SET "));
    // A key given twice is two answers to one question, whatever the case
    // of its letters: neither is taken. (The later once won, and the reply
    // said both back.)
    const katana::core::Error twice =
        session.refused("CUSTOMISE SET linework.start=S linework.start=XX");
    EXPECT_EQ(twice.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(twice.message,
              "a SET key is given once in a line, and linework.start is given twice");
    EXPECT_EQ(twice.context, "linework.start=XX");
    EXPECT_EQ(session.refused("CUSTOMISE SET auto.codes=off AUTO.CODES=on").message,
              "a SET key is given once in a line, and auto.codes is given twice");

    EXPECT_TRUE(session.state().automation.codesOnSurveyImport);
    EXPECT_TRUE(session.state().automation.lineworkOnSurveyImport);
    EXPECT_EQ(session.state().linework.start, "ST");
    EXPECT_EQ(session.document.customisationGeneration(), generation);
    EXPECT_TRUE(session.state().kept);
}

// ---- help ----------------------------------------------------------------------------------

TEST(CustomisationVerbs, TheHelpNamesEveryWordAndHelpCustomiseSaysEveryReply)
{
    const std::string help = CommandInterpreter::helpText();
    // The literal the session's own help carried, so that what pins it there
    // (the cli and MCP help tests) passes on this block alone once the
    // session's goes.
    EXPECT_TRUE(contains(help, "Customise CUSTOMISE [REPLACE] <file> [<file>...]"));
    for (const char* word : {"CUSTOMISE JSON", "CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] "
                                               "[SYMBOLS]",
                             "CUSTOMISE RESET", "KEEP", "REVERT", "CUSTOMISE REMOVE name... [FORCE]",
                             "REMOVE CODE key...", "CUSTOMISE SET auto.codes=on|off",
                             "HELP CUSTOMISE"}) {
        EXPECT_TRUE(contains(help, word)) << word;
    }

    Bare session;
    const std::string family = session.interpreter.run("HELP CUSTOMISE").valueOr({});
    EXPECT_EQ(family, katana::cad::customisationVerbHelp());
    EXPECT_EQ(session.interpreter.run("help customize").valueOr({}), family);
    for (const char* reply : {"customisation name=", "source name=", "automation auto.codes=",
                              "linework linework.start=", "missing", "loaded file=",
                              "removed definitions=", "undefined names=", "exported file=",
                              "reset name=", "kept file=", "written=no "
                                                          "reason=the-session-is-the-built-in",
                              "reverted file=", "removed definition=", "removed code=",
                              "set <key>=<value>", "./json", "CUSTOMIZE",
                              // What the family says of its own edges: a layer
                              // is a use, the file comes first, a key once, and
                              // what a part lists.
                              "rules= styles= layers=", "(./CODES)",
                              "A key is given once in a line",
                              "lists as its sources only those it holds something of",
                              "of either kind unless LINESTYLES or SYMBOLS says which"}) {
        EXPECT_TRUE(contains(family, reply)) << reply;
    }
    // Neither names another program's file kinds, nor its name.
    for (const std::string& text : {help, family}) {
        EXPECT_FALSE(contains(text, ".mapfile"));
        EXPECT_FALSE(contains(text, ".4d"));
        EXPECT_FALSE(contains(text, "12d"));
    }
}
