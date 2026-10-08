// The built-in LIBRARY (customisation_host.hpp): the several customisations a
// program is built with, the default first, found by name; the seam's name
// form; and how a session stands to them (builtInChoices).
//
// Two halves, and the first needs no build to have anything: the rules are
// tested against stand-ins the test writes out, so they are the same tests on
// a machine whose build compiled in other files. The second asks the BUILD
// what it compiled in and holds each file to the figures of its own census;
// it SKIPS what this build did not compile in (a build whose
// KATANA_BUILTIN_CUSTOMISATION or KATANA_ALTERNATE_CUSTOMISATIONS names no
// file), so the suite stays green without it.
//
// By hand, from the two stand-ins below:
//
//   "Alpha"      2 definitions (ALPHA Line, ALPHA Dot), 3 rules over 2 keys
//                (AA* twice, AB* once)
//   "Beta Two"   1 definition (BETA Line), 1 rule over 1 key (BB*)
//
// Katana Standard's figures are those of its census (tools/katana_standard/
// census.py, held in test_katana_standard.cpp): 186 definitions (73
// linestyles, 113 symbols), 770 rules over 245 keys, 28 colours. NSW's are
// test_builtin_census.cpp's: 792, 1,624 rules over 632 keys.

#include <gtest/gtest.h>

#include <cstddef>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/customisation_host.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/path_text.hpp"
#include "katana/entity/customisation.hpp"

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

namespace fs = std::filesystem;
using katana::cad::BuiltInChoice;
using katana::cad::BuiltInCustomisation;
using katana::cad::BuiltInEntry;
using katana::cad::CustomisationHost;
using katana::cad::CustomisationOrigin;
using katana::cad::Document;

namespace {

const std::string kAlpha = R"({
  "format": "katana-customisation", "version": 1, "name": "Alpha",
  "description": "The first stand-in",
  "linestyles": [{"name": "ALPHA Line", "strokes": [["move", 0, 0], ["draw", 1, 0]]}],
  "symbols": [{"name": "ALPHA Dot", "atVertices": true, "strokes": [["circle", 0.5]]}],
  "codes": [
    {"key": "AA*", "sets": "feature", "layer": "ALPHA A", "linestyle": "ALPHA Line"},
    {"key": "AA*", "sets": "symbol", "symbol": {"name": "ALPHA Dot"}},
    {"key": "AB*", "sets": "feature", "layer": "ALPHA B"}
  ]
})";

const std::string kBeta = R"({
  "format": "katana-customisation", "version": 1, "name": "Beta Two",
  "description": "The second stand-in",
  "linestyles": [{"name": "BETA Line", "strokes": [["move", 0, 0], ["draw", 2, 0]]}],
  "codes": [{"key": "BB*", "sets": "feature", "layer": "BETA", "linestyle": "BETA Line"}]
})";

BuiltInEntry entryFrom(const std::string& text, const std::string& source)
{
    BuiltInEntry entry;
    auto read = katana::entity::customisationFromJson(text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? std::string{} : read.error().describe());
    if (read.ok()) {
        entry.builtIn.customisation =
            std::make_shared<const katana::entity::Customisation>(std::move(*read));
        entry.builtIn.digest = katana::entity::customisationDigest(text);
        entry.name = entry.builtIn.customisation->name;
        entry.description = entry.builtIn.customisation->description;
    }
    entry.source = source;
    return entry;
}

std::vector<BuiltInEntry> twoEntries()
{
    return {entryFrom(kAlpha, "alpha.customisation.json"), entryFrom(kBeta, "beta.customisation.json")};
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

// A fresh directory for one test's files, in one process's own.
struct Scratch {
    fs::path root;

    explicit Scratch(const char* name)
        : root(fs::temp_directory_path() /
               ("katana-cad-tests-builtin-library-" + processTag()) / name)
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

    fs::path write(const char* name, const std::string& text) const
    {
        const fs::path path = root / name;
        std::ofstream(path, std::ios::binary) << text;
        return path;
    }
};

// The seam, set for one test and put back as it was - set or not - when the
// test ends.
class ScopedSeam {
  public:
    explicit ScopedSeam(const char* value)
    {
        if (const char* before = std::getenv(katana::cad::kBuiltInCustomisationVariable)) {
            before_ = before;
        }
        set(value);
    }
    ~ScopedSeam() { set(before_ ? before_->c_str() : nullptr); }
    ScopedSeam(const ScopedSeam&) = delete;
    ScopedSeam& operator=(const ScopedSeam&) = delete;

  private:
    static void set(const char* value)
    {
        const char* name = katana::cad::kBuiltInCustomisationVariable;
#if defined(_WIN32)
        EXPECT_EQ(_putenv_s(name, value == nullptr ? "" : value), 0);
#else
        if (value == nullptr) {
            EXPECT_EQ(unsetenv(name), 0);
        } else {
            EXPECT_EQ(setenv(name, value, 1), 0);
        }
#endif
    }

    std::optional<std::string> before_;
};

const BuiltInChoice* choiceNamed(const std::vector<BuiltInChoice>& choices, const char* name)
{
    for (const BuiltInChoice& choice : choices) {
        if (choice.name == name) {
            return &choice;
        }
    }
    return nullptr;
}

} // namespace

// ---- finding one by name --------------------------------------------------------------------

TEST(BuiltInLibrary, AnEntryIsFoundByItsNameInAnyLetterCaseAndOnlyByItsWholeName)
{
    const std::vector<BuiltInEntry> entries = twoEntries();
    for (const char* spelling : {"Beta Two", "beta two", "BETA TWO", "bEtA tWo"}) {
        const BuiltInEntry* found = katana::cad::findBuiltIn(entries, spelling);
        ASSERT_NE(found, nullptr) << spelling;
        EXPECT_EQ(found->name, "Beta Two") << spelling;
    }
    EXPECT_EQ(katana::cad::findBuiltIn(entries, "Alpha")->name, "Alpha");
    // The whole name: not a part of it, not with a blank more, and not empty.
    for (const char* wrong : {"Beta", "Two", "Beta Two ", " Beta Two", "Alph", "", "alpha.customisation.json"}) {
        EXPECT_EQ(katana::cad::findBuiltIn(entries, wrong), nullptr) << "\"" << wrong << "\"";
    }
    EXPECT_EQ(katana::cad::findBuiltIn({}, "Alpha"), nullptr);
}

// ---- the list built from files ---------------------------------------------------------------

// builtIns() is builtInsFrom over what the build compiled in, so its rules are
// tested here over files the test writes out: no build can reach a second file
// of one name, or a file that is no customisation, unless it is made so.

TEST(BuiltInLibrary, AListFromFilesHoldsThemInTheirOrderWithTheirNamesSourcesAndDigests)
{
    const std::vector<katana::cad::BuiltInFile> files{{"alpha.customisation.json", kAlpha},
                                                      {"beta.customisation.json", kBeta}};
    const std::vector<BuiltInEntry> list = katana::cad::builtInsFrom(files);
    ASSERT_EQ(list.size(), 2u);
    EXPECT_EQ(list[0].name, "Alpha");
    EXPECT_EQ(list[0].description, "The first stand-in");
    EXPECT_EQ(list[0].source, "alpha.customisation.json");
    EXPECT_EQ(list[0].builtIn.digest, katana::entity::customisationDigest(kAlpha));
    ASSERT_NE(list[0].builtIn.customisation, nullptr);
    EXPECT_EQ(list[1].name, "Beta Two");
    EXPECT_EQ(list[1].source, "beta.customisation.json");
    EXPECT_EQ(list[1].builtIn.digest, katana::entity::customisationDigest(kBeta));
    EXPECT_TRUE(katana::cad::builtInsFrom({}).empty()) << "no files, no built-ins";
}

TEST(BuiltInLibrary, TheFirstFileTakesTheParseItIsGivenAndOnlyTheFirst)
{
    const std::vector<BuiltInEntry> parsed = twoEntries();
    const std::vector<katana::cad::BuiltInFile> files{{"alpha.customisation.json", kAlpha},
                                                      {"beta.customisation.json", kBeta}};
    // The default is the parse compiledInCustomisation() made: shared, not read twice.
    const std::vector<BuiltInEntry> shared = katana::cad::builtInsFrom(files, &parsed[0].builtIn);
    ASSERT_EQ(shared.size(), 2u);
    EXPECT_EQ(shared[0].builtIn.customisation, parsed[0].builtIn.customisation);
    EXPECT_NE(shared[1].builtIn.customisation, parsed[1].builtIn.customisation)
        << "the second file is read here, whatever the first was given";
    // Handed none, every file is read, so the first is a parse of its own.
    const std::vector<BuiltInEntry> read = katana::cad::builtInsFrom(files);
    EXPECT_NE(read[0].builtIn.customisation, parsed[0].builtIn.customisation);
    EXPECT_EQ(read[0].name, "Alpha");
}

TEST(BuiltInLibrary, ALaterFileWithAnEarlierOnesNameInAnyLetterCaseIsNotOffered)
{
    // The copy says it is "ALPHA", and has the second stand-in's single definition.
    std::string copy = kBeta;
    const std::string was = "\"name\": \"Beta Two\"";
    copy.replace(copy.find(was), was.size(), "\"name\": \"ALPHA\"");
    const std::vector<katana::cad::BuiltInFile> files{{"alpha.customisation.json", kAlpha},
                                                      {"copy.customisation.json", copy},
                                                      {"beta.customisation.json", kBeta}};
    const std::vector<BuiltInEntry> list = katana::cad::builtInsFrom(files);
    ASSERT_EQ(list.size(), 2u) << "a name picks one entry";
    EXPECT_EQ(list[0].source, "alpha.customisation.json") << "the earlier file keeps the name";
    EXPECT_NE(list[0].builtIn.customisation->library.find("ALPHA Line"), nullptr)
        << "the earlier file's own contents";
    EXPECT_EQ(list[0].builtIn.customisation->library.find("BETA Line"), nullptr);
    EXPECT_EQ(list[1].source, "beta.customisation.json") << "the files after it are still read";
    EXPECT_EQ(katana::cad::findBuiltIn(list, "alpha")->source, "alpha.customisation.json");
}

TEST(BuiltInLibrary, AFileThatIsNoCustomisationIsListedByItsFileNameWithTheReason)
{
    const std::vector<katana::cad::BuiltInFile> files{{"broken.customisation.json", "{ not a customisation"},
                                                      {"beta.customisation.json", kBeta}};
    const std::vector<BuiltInEntry> list = katana::cad::builtInsFrom(files);
    ASSERT_EQ(list.size(), 2u);
    // Listed, so that a person can see that it is there and why it is of no use...
    EXPECT_EQ(list[0].name, "broken.customisation.json");
    EXPECT_EQ(list[0].source, "broken.customisation.json");
    EXPECT_EQ(list[0].builtIn.customisation, nullptr);
    EXPECT_NE(list[0].builtIn.problem.find("broken.customisation.json"), std::string::npos)
        << list[0].builtIn.problem;
    EXPECT_NE(list[0].builtIn.problem.find("is not read"), std::string::npos) << list[0].builtIn.problem;
    // ...and the next file is not held up by it.
    EXPECT_EQ(list[1].name, "Beta Two");
    ASSERT_NE(list[1].builtIn.customisation, nullptr);

    // A default that did not read was parsed by someone else, and is taken as
    // it is: its reason is theirs, its place the file name.
    BuiltInCustomisation failed;
    failed.problem = "the reason the default was refused";
    const std::vector<BuiltInEntry> withFailed = katana::cad::builtInsFrom(files, &failed);
    ASSERT_EQ(withFailed.size(), 2u);
    EXPECT_EQ(withFailed[0].name, "broken.customisation.json");
    EXPECT_EQ(withFailed[0].builtIn.problem, "the reason the default was refused");
}

// ---- what to tell of a name that is a built-in ------------------------------------------------

TEST(BuiltInLibrary, AMissingNameThatIsABuiltInIsToldWithTheVerbThatMakesItTheSession)
{
    const std::vector<BuiltInEntry> entries = twoEntries();
    const auto advice = [&](const std::vector<std::string>& missing,
                            const std::vector<BuiltInEntry>& offered) {
        return katana::cad::builtInAdviceFor(missing, offered);
    };
    // Spelled as the entry spells it, whatever case the project recorded.
    EXPECT_EQ(advice({"beta two"}, entries),
              "CUSTOMISE BUILTIN Beta Two makes the built-in \"Beta Two\" the session");
    // A name that is no built-in's, among others that are not, says nothing...
    EXPECT_EQ(advice({"Site", "streets.4d"}, entries), "");
    // ...and one that is, among those, is said alone.
    EXPECT_EQ(advice({"Site", "ALPHA"}, entries),
              "CUSTOMISE BUILTIN Alpha makes the built-in \"Alpha\" the session");
    // Two of them: a clause each, in the project's order.
    EXPECT_EQ(advice({"Beta Two", "Alpha"}, entries),
              "CUSTOMISE BUILTIN Beta Two makes the built-in \"Beta Two\" the session; "
              "CUSTOMISE BUILTIN Alpha makes the built-in \"Alpha\" the session");
    EXPECT_EQ(advice({}, entries), "");
    EXPECT_EQ(advice({"Alpha"}, {}), "") << "a program with no built-in";
}

TEST(BuiltInLibrary, ABuiltInThatDidNotReadIsNotOfferedToAProjectThatNamesIt)
{
    std::vector<BuiltInEntry> entries = twoEntries();
    BuiltInEntry damaged;
    damaged.name = "damaged.customisation.json";
    damaged.source = damaged.name;
    damaged.builtIn.problem = "not read";
    entries.push_back(damaged);
    // CUSTOMISE BUILTIN would refuse it, so the words do not send anyone to it.
    EXPECT_EQ(katana::cad::builtInAdviceFor(std::vector<std::string>{"damaged.customisation.json"},
                                            entries),
              "");
}

// ---- the seam: the name form -----------------------------------------------------------------

TEST(BuiltInLibrary, TheSeamNamesACompiledInBuiltInAndThatBuiltInIsTheRunsOwn)
{
    const std::vector<BuiltInEntry> entries = twoEntries();
    const BuiltInCustomisation standIn = entries.front().builtIn;
    for (const char* name : {"Beta Two", "beta two", "BETA TWO"}) {
        const BuiltInCustomisation builtIn = katana::cad::builtInCustomisationFor(name, standIn, entries);
        ASSERT_NE(builtIn.customisation, nullptr) << name << ": " << builtIn.problem;
        EXPECT_EQ(builtIn.customisation->name, "Beta Two") << name;
        // The very one the list holds, shared, with its digest.
        EXPECT_EQ(builtIn.customisation, entries[1].builtIn.customisation) << name;
        EXPECT_EQ(builtIn.digest, entries[1].builtIn.digest) << name;
        EXPECT_TRUE(builtIn.problem.empty()) << name;
    }
    // Unset still gives the default the program has, and `none` still none:
    // the name form takes nothing from either.
    EXPECT_EQ(katana::cad::builtInCustomisationFor("", standIn, entries).customisation,
              standIn.customisation);
    EXPECT_EQ(katana::cad::builtInCustomisationFor("none", standIn, entries).customisation, nullptr);
}

TEST(BuiltInLibrary, NoneBeatsABuiltInOfThatNameAndAPathBeatsAName)
{
    // A built-in called "none" is not reachable by the seam: `none` is no built-in.
    std::vector<BuiltInEntry> entries = twoEntries();
    entries.push_back(entryFrom(R"({"format": "katana-customisation", "version": 1, "name": "None",
 "codes": [{"key": "NN*", "sets": "feature", "layer": "NONE"}]})", ""));
    ASSERT_NE(entries.back().builtIn.customisation, nullptr);
    const BuiltInCustomisation none = katana::cad::builtInCustomisationFor("None", {}, entries);
    EXPECT_EQ(none.customisation, nullptr);
    EXPECT_TRUE(none.problem.empty()) << "asked for, so not a problem";

    // A value with a folder in it is a path, even when what follows the last
    // separator is a name: the file is read, and the built-in of that name is not.
    const Scratch scratch("path-beats-name");
    const fs::path file = scratch.write("Alpha", kBeta);
    const BuiltInCustomisation read = katana::cad::builtInCustomisationFor(
        katana::core::pathToUtf8(file), entries.front().builtIn, entries);
    ASSERT_NE(read.customisation, nullptr) << read.problem;
    EXPECT_EQ(read.customisation->name, "Beta Two") << "the file's, not the list's Alpha";
}

TEST(BuiltInLibrary, AFileCalledLikeABuiltInInTheWorkingFolderIsGivenWithItsFolder)
{
    // `./Alpha` has a separator, so it is a file; `Alpha` is the built-in.
    const Scratch scratch("dot-slash");
    scratch.write("Alpha", kBeta);
    const std::vector<BuiltInEntry> entries = twoEntries();
    const fs::path before = fs::current_path();
    fs::current_path(scratch.root);
    BuiltInCustomisation byName;
    BuiltInCustomisation byFile;
    try {
        byName = katana::cad::builtInCustomisationFor("Alpha", {}, entries);
        byFile = katana::cad::builtInCustomisationFor("./Alpha", {}, entries);
    } catch (...) {
        fs::current_path(before);
        throw;
    }
    fs::current_path(before);
    ASSERT_NE(byName.customisation, nullptr) << byName.problem;
    EXPECT_EQ(byName.customisation->name, "Alpha") << "the built-in, the file beside it unread";
    ASSERT_NE(byFile.customisation, nullptr) << byFile.problem;
    EXPECT_EQ(byFile.customisation->name, "Beta Two") << "the file";
}

TEST(BuiltInLibrary, AWordThatIsNoBuiltInIsReadAsAFileAndTheReasonListsTheNamesThereAre)
{
    const std::vector<BuiltInEntry> entries = twoEntries();
    const Scratch scratch("mistyped");
    const fs::path before = fs::current_path();
    fs::current_path(scratch.root); // a folder with no file called Alfa
    BuiltInCustomisation builtIn;
    try {
        builtIn = katana::cad::builtInCustomisationFor("Alfa", entries.front().builtIn, entries);
    } catch (...) {
        fs::current_path(before);
        throw;
    }
    fs::current_path(before);
    // None - and never the default in its place.
    EXPECT_EQ(builtIn.customisation, nullptr);
    EXPECT_NE(builtIn.problem.find(katana::cad::kBuiltInCustomisationVariable), std::string::npos)
        << builtIn.problem;
    EXPECT_NE(builtIn.problem.find("Alfa"), std::string::npos) << builtIn.problem;
    EXPECT_NE(builtIn.problem.find("\"Alpha\", \"Beta Two\""), std::string::npos) << builtIn.problem;
    EXPECT_NE(builtIn.problem.find("./Alfa"), std::string::npos) << builtIn.problem;

    // With no names to offer there is nothing to list, and it reads as it did.
    const BuiltInCustomisation alone = katana::cad::builtInCustomisationFor("Alfa", {}, {});
    EXPECT_EQ(alone.problem.find("either"), std::string::npos) << alone.problem;
}

TEST(BuiltInLibrary, ABuiltInThatDidNotReadIsHandedOnWithItsReasonWhenItsNameIsAsked)
{
    std::vector<BuiltInEntry> entries;
    BuiltInEntry damaged;
    damaged.name = "damaged.customisation.json";
    damaged.source = damaged.name;
    damaged.builtIn.problem = "the customisation compiled into this program is not read: damaged";
    entries.push_back(damaged);
    const BuiltInCustomisation builtIn =
        katana::cad::builtInCustomisationFor("damaged.customisation.json", {}, entries);
    EXPECT_EQ(builtIn.customisation, nullptr);
    EXPECT_EQ(builtIn.problem, damaged.builtIn.problem);
}

// ---- the choices -----------------------------------------------------------------------------

TEST(BuiltInLibrary, EachChoiceCountsWhatItsBuiltInHoldsAndKnowsTheDefaultAndTheActiveOne)
{
    const std::vector<BuiltInEntry> entries = twoEntries();
    CustomisationHost host;
    host.builtIn = entries.front().builtIn;
    host.builtIns = entries;
    Document document;
    ASSERT_EQ(katana::cad::startCustomisation(document, host).installed, CustomisationOrigin::BuiltIn);

    const std::vector<BuiltInChoice> choices = katana::cad::builtInChoices(document, entries);
    ASSERT_EQ(choices.size(), 2u);
    EXPECT_EQ(choices[0].name, "Alpha");
    EXPECT_EQ(choices[0].description, "The first stand-in");
    EXPECT_EQ(choices[0].source, "alpha.customisation.json");
    EXPECT_EQ(choices[0].definitions, 2u);
    EXPECT_EQ(choices[0].rules, 3u);
    EXPECT_EQ(choices[0].codes, 2u);
    EXPECT_TRUE(choices[0].isDefault);
    EXPECT_TRUE(choices[0].isActive);
    EXPECT_TRUE(choices[0].problem.empty());
    EXPECT_EQ(choices[1].name, "Beta Two");
    EXPECT_EQ(choices[1].definitions, 1u);
    EXPECT_EQ(choices[1].rules, 1u);
    EXPECT_EQ(choices[1].codes, 1u);
    EXPECT_FALSE(choices[1].isDefault);
    EXPECT_FALSE(choices[1].isActive);

    // Another built-in in the session's place: it is the active one, the
    // default stays the run's.
    ASSERT_TRUE(katana::cad::installBuiltInCustomisation(document, entries[1].builtIn, false).ok());
    const std::vector<BuiltInChoice> after = katana::cad::builtInChoices(document, entries);
    EXPECT_TRUE(after[0].isDefault);
    EXPECT_FALSE(after[0].isActive);
    EXPECT_FALSE(after[1].isDefault);
    EXPECT_TRUE(after[1].isActive);
}

TEST(BuiltInLibrary, ACopyOfABuiltInIsNotTheActiveOneAndASessionWithNoHostHasNoDefault)
{
    const std::vector<BuiltInEntry> entries = twoEntries();
    // The built-in, installed and then edited: it is a copy, not the built-in.
    Document edited;
    ASSERT_TRUE(katana::cad::installBuiltInCustomisation(edited, entries[0].builtIn, true).ok());
    edited.setAutomation({false, false});
    for (const BuiltInChoice& choice : katana::cad::builtInChoices(edited, entries)) {
        EXPECT_FALSE(choice.isActive) << choice.name;
        EXPECT_FALSE(choice.isDefault) << "no host told the Document what the run's built-in is";
    }
    // A Document with nothing in it, and an empty list.
    const Document empty;
    EXPECT_TRUE(katana::cad::builtInChoices(empty, {}).empty());
    for (const BuiltInChoice& choice : katana::cad::builtInChoices(empty, entries)) {
        EXPECT_FALSE(choice.isActive);
    }
}

TEST(BuiltInLibrary, AChoiceThatDidNotReadHasItsReasonAndNoCounts)
{
    BuiltInEntry damaged;
    damaged.name = "damaged.customisation.json";
    damaged.builtIn.problem = "not read";
    const Document document;
    const std::vector<BuiltInChoice> choices = katana::cad::builtInChoices(document, {&damaged, 1});
    ASSERT_EQ(choices.size(), 1u);
    EXPECT_EQ(choices[0].problem, "not read");
    EXPECT_EQ(choices[0].definitions, 0u);
    EXPECT_EQ(choices[0].rules, 0u);
    EXPECT_FALSE(choices[0].isActive);
}

TEST(BuiltInLibrary, AHostOffersItsOwnListAndOtherwiseWhatWasCompiledIn)
{
    CustomisationHost host;
    EXPECT_EQ(&katana::cad::builtInsOf(host), &katana::cad::builtIns());
    host.builtIns = twoEntries();
    EXPECT_EQ(&katana::cad::builtInsOf(host), &*host.builtIns);
    host.builtIns = std::vector<BuiltInEntry>{};
    EXPECT_TRUE(katana::cad::builtInsOf(host).empty()) << "an empty list is a list";
}

// ---- what this build compiled in -------------------------------------------------------------

TEST(BuiltInLibrary, TheCompiledInListHasTheDefaultFirstSharedAndEveryNameOnce)
{
    const std::vector<BuiltInEntry>& entries = katana::cad::builtIns();
    EXPECT_EQ(&entries, &katana::cad::builtIns()) << "parsed once";
    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    if (entries.empty()) {
        EXPECT_EQ(compiledIn.customisation, nullptr);
        GTEST_SKIP() << "this build compiled no built-in customisation in";
    }
    for (std::size_t at = 0; at < entries.size(); ++at) {
        EXPECT_FALSE(entries[at].name.empty()) << at;
        EXPECT_FALSE(entries[at].source.empty()) << at << ": compiled in from a file";
        EXPECT_TRUE(entries[at].builtIn.problem.empty()) << entries[at].name << ": "
                                                         << entries[at].builtIn.problem;
        // One name, one entry, whatever the letter case.
        EXPECT_EQ(katana::cad::findBuiltIn(entries, entries[at].name), &entries[at]);
    }
    if (compiledIn.customisation != nullptr) {
        // The default is the first, and the one parse is shared.
        EXPECT_EQ(entries.front().builtIn.customisation, compiledIn.customisation);
        EXPECT_EQ(entries.front().builtIn.digest, compiledIn.digest);
    }
}

TEST(BuiltInLibrary, TheDefaultBuildCompilesKatanaStandardInAsTheDefaultAndItHoldsItsCensus)
{
    const std::vector<BuiltInEntry>& entries = katana::cad::builtIns();
    if (entries.empty() || entries.front().source != "katana-standard.customisation.json") {
        GTEST_SKIP() << "this build's default built-in is not Katana Standard";
    }
    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    ASSERT_NE(compiledIn.customisation, nullptr) << compiledIn.problem;
    EXPECT_EQ(compiledIn.customisation->name, "Katana Standard");
    // The census figures, as test_katana_standard.cpp holds the file to them:
    // here it is the COMPILED-IN copy, which is what a program starts with.
    EXPECT_EQ(compiledIn.customisation->library.size(), 186u);
    EXPECT_EQ(compiledIn.customisation->map.size(), 770u);
    EXPECT_EQ(compiledIn.customisation->map.keys().size(), 245u);
    EXPECT_EQ(compiledIn.customisation->colours.size(), 28u);
    EXPECT_EQ(compiledIn.digest.size(), 16u);
    // And its choice figures say the same.
    Document document;
    const std::vector<BuiltInChoice> choices = katana::cad::builtInChoices(document, entries);
    const BuiltInChoice* standard = choiceNamed(choices, "Katana Standard");
    ASSERT_NE(standard, nullptr);
    EXPECT_EQ(standard->definitions, 186u);
    EXPECT_EQ(standard->rules, 770u);
    EXPECT_EQ(standard->codes, 245u);
    EXPECT_EQ(standard->source, "katana-standard.customisation.json");
}

TEST(BuiltInLibrary, TheNswAlternateIsListedAfterTheDefaultAndNeverIsIt)
{
    const BuiltInEntry* nsw = katana::cad::findBuiltIn(katana::cad::builtIns(), "NSW");
    if (nsw == nullptr) {
        GTEST_SKIP() << "this build has no NSW customisation compiled in";
    }
    ASSERT_NE(nsw->builtIn.customisation, nullptr) << nsw->builtIn.problem;
    EXPECT_EQ(nsw->source, "nsw.customisation.json");
    // test_builtin_census.cpp's figures.
    Document document;
    const std::vector<BuiltInChoice> choices = katana::cad::builtInChoices(document, {nsw, 1});
    const BuiltInChoice* choice = choiceNamed(choices, "NSW");
    ASSERT_NE(choice, nullptr);
    EXPECT_EQ(choice->definitions, 792u);
    EXPECT_EQ(choice->rules, 1624u);
    EXPECT_EQ(choice->codes, 632u);
    // It is an alternate: the default, when the build has one, is another.
    if (katana::cad::compiledInCustomisation().customisation != nullptr) {
        EXPECT_NE(katana::cad::compiledInCustomisation().customisation->name, "NSW")
            << "the default build's default is Katana Standard";
        EXPECT_NE(katana::cad::compiledInCustomisation().customisation, nsw->builtIn.customisation);
        EXPECT_NE(&katana::cad::builtIns().front(), nsw);
    }
}

TEST(BuiltInLibrary, TheSeamInTheEnvironmentNamesACompiledInBuiltInInAnyLetterCase)
{
    const BuiltInEntry* nsw = katana::cad::findBuiltIn(katana::cad::builtIns(), "NSW");
    if (nsw == nullptr) {
        GTEST_SKIP() << "this build has no NSW customisation compiled in";
    }
    for (const char* spelling : {"NSW", "nsw", "Nsw"}) {
        const ScopedSeam seam(spelling);
        const BuiltInCustomisation builtIn = katana::cad::builtInCustomisation();
        ASSERT_NE(builtIn.customisation, nullptr) << spelling << ": " << builtIn.problem;
        EXPECT_EQ(builtIn.customisation, nsw->builtIn.customisation) << spelling;
        EXPECT_EQ(builtIn.digest, nsw->builtIn.digest) << spelling;
    }
    // The compiled-in default is untouched by what the seam says.
    const ScopedSeam seam("NSW");
    EXPECT_NE(katana::cad::compiledInCustomisation().customisation, nsw->builtIn.customisation);
}

TEST(BuiltInLibrary, TheSeamInTheEnvironmentNamesTheDefaultByNameToo)
{
    const std::vector<BuiltInEntry>& entries = katana::cad::builtIns();
    if (entries.empty() || katana::cad::compiledInCustomisation().customisation == nullptr) {
        GTEST_SKIP() << "this build has no default built-in compiled in";
    }
    const ScopedSeam seam(entries.front().name.c_str());
    const BuiltInCustomisation builtIn = katana::cad::builtInCustomisation();
    EXPECT_EQ(builtIn.customisation, katana::cad::compiledInCustomisation().customisation);
}
