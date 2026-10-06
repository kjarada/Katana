// Where a session's customisation comes from when it starts
// (customisation_host.hpp): reading a file, the built-in and the seam that
// says what it is for one run, and startCustomisation's choice between the
// kept file, the built-in and nothing.
//
// "The built-in" in these tests is the small customisation written out below,
// handed over by the test as a host hands one over. Nothing here reads the
// customisation a build may have compiled in, except to check that asking for
// it is safe whether or not there is one - so the suite is the same on a
// machine whose build has it and on one whose build has not.

#include <gtest/gtest.h>

#include <cstdint>
#include <cstdlib>
#include <exception>
#include <filesystem>
#include <fstream>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <vector>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/path_text.hpp"

namespace fs = std::filesystem;
using katana::cad::BuiltInCustomisation;
using katana::cad::CustomisationHost;
using katana::cad::CustomisationOrigin;
using katana::cad::CustomisationSource;
using katana::cad::CustomisationStart;
using katana::cad::CustomisationState;
using katana::cad::Document;
using katana::cad::DocumentChange;
using katana::core::ErrorCode;
using katana::entity::Customisation;

namespace {

// Counted by hand: 2 definitions, 1 of them listed as a symbol; 2 rules; 1
// colour; start spelled "S".
const std::string kSmall = R"({
  "format": "katana-customisation", "version": 1,
  "name": "Small Built In",
  "description": "Two definitions and two rules, for tests",
  "notice": ["Written for these tests."],
  "colours": {"sui gas": "#FF7F00"},
  "linework": {"start": "S", "end": "END", "close": "CL", "arcStart": "BC",
               "arcEnd": "EC", "join": "JPN", "rectangle": "RECT"},
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

// A user's own customisation: one rule, and what it was made from - the
// built-in of that name, in the edition of that digest - when `basedOnDigest`
// gives one.
std::string keptText(const std::string& basedOnDigest,
                     const std::string& basedOnName = "Small Built In")
{
    std::string text = R"({"format": "katana-customisation", "version": 1, "name": "Mine",)";
    if (!basedOnDigest.empty()) {
        text += R"( "basedOn": {"name": ")" + basedOnName + R"(", "digest": ")" + basedOnDigest +
                R"("},)";
    }
    text += R"( "codes": [{"key": "ZZ*", "sets": "feature", "layer": "MINE"}]})";
    return text;
}

// FNV-1a, 64-bit, as 16 lower-case hexadecimal digits: the digest of a file's
// bytes, worked out here from the algorithm's published offset basis and
// prime rather than by the function under test.
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
// share a folder: under ctest --parallel a test program is run a case at a
// time AND, for some suites, whole again at each SIMD level. With one fixed
// name, one process removed the folder while the other held a file in it
// open, and the uncaught filesystem_error ended that run. A reused id is
// harmless: the folder is cleared before it is used.
std::string processTag()
{
#if defined(_WIN32)
    return std::to_string(_getpid());
#else
    return std::to_string(getpid());
#endif
}

// A fresh directory for one test's files, in one process's own, removed when
// the test ends.
struct Scratch {
    fs::path root;

    explicit Scratch(const char* name)
        : root(fs::temp_directory_path() /
               ("katana-cad-tests-customisation-host-" + processTag()) / name)
    {
        fs::remove_all(root);
        fs::create_directories(root);
    }
    ~Scratch()
    {
        std::error_code ignored;
        fs::remove_all(root.parent_path(), ignored);
    }

    fs::path write(const char* name, const std::string& text) const
    {
        const fs::path path = root / name;
        std::ofstream(path, std::ios::binary) << text;
        return path;
    }
};

// `text` as a host holds a built-in: read, with the digest of its bytes.
BuiltInCustomisation builtInFrom(const std::string& text)
{
    BuiltInCustomisation builtIn;
    auto read = katana::entity::customisationFromJson(text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? std::string{} : read.error().describe());
    if (read.ok()) {
        builtIn.customisation = std::make_shared<const Customisation>(std::move(*read));
        builtIn.digest = fnv1a(text);
    }
    return builtIn;
}

// The seam, set for one test and put back as it was - set or not - when the
// test ends, so that no test here depends on, or leaves a mark on, the
// environment it runs in.
class ScopedSeam {
  public:
    // nullptr: the variable is not set at all.
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

    [[nodiscard]] const std::optional<std::string>& before() const { return before_; }

  private:
    static void set(const char* value)
    {
        const char* name = katana::cad::kBuiltInCustomisationVariable;
#if defined(_WIN32)
        // An empty value removes the variable.
        ASSERT_EQ(_putenv_s(name, value == nullptr ? "" : value), 0);
#else
        if (value == nullptr) {
            ASSERT_EQ(unsetenv(name), 0);
        } else {
            ASSERT_EQ(setenv(name, value, 1), 0);
        }
#endif
    }

    std::optional<std::string> before_;
};

// A built-in standing in for the compiled-in one. This build may have none,
// and the rule of the seam is about whatever a build has: with a stand-in, the
// tests of that rule are the same tests, and can fail, on every machine.
BuiltInCustomisation standIn()
{
    return builtInFrom(R"({"format": "katana-customisation", "version": 1, "name": "Stand In",
 "codes": [{"key": "SI*", "sets": "feature", "layer": "STAND IN"}]})");
}

} // namespace

// ---- reading a file --------------------------------------------------------------------------

TEST(CustomisationHost, ReadingAFileGivesTheCustomisationAndTheDigestOfItsBytes)
{
    const Scratch scratch("read");
    const fs::path path = scratch.write("small.customisation.json", kSmall);

    const auto file = katana::cad::readCustomisationFile(path);
    ASSERT_TRUE(file.ok()) << file.error().describe();
    EXPECT_EQ(file->customisation.name, "Small Built In");
    EXPECT_EQ(file->customisation.library.size(), 2u);
    EXPECT_EQ(file->customisation.map.size(), 2u);
    // Which array a definition sits in says whether it is listed as a symbol,
    // and the customisation it sits in is where it is from.
    ASSERT_NE(file->customisation.library.find("TEST Peg"), nullptr);
    EXPECT_TRUE(file->customisation.library.find("TEST Peg")->symbol);
    ASSERT_NE(file->customisation.library.find("TEST Fence"), nullptr);
    EXPECT_FALSE(file->customisation.library.find("TEST Fence")->symbol);
    EXPECT_EQ(file->customisation.library.find("TEST Fence")->source, "Small Built In");
    // The digest is of the bytes as they are on disk.
    EXPECT_EQ(file->digest, fnv1a(kSmall));
    EXPECT_EQ(file->digest.size(), 16u);
}

TEST(CustomisationHost, AFileThatIsNotThereOrIsNoCustomisationIsRefusedNamingTheFile)
{
    const Scratch scratch("refused");

    const auto missing = katana::cad::readCustomisationFile(scratch.root / "nowhere.json");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    EXPECT_NE(missing.error().context.find("nowhere.json"), std::string::npos)
        << missing.error().describe();

    const auto directory = katana::cad::readCustomisationFile(scratch.root);
    ASSERT_FALSE(directory.ok());
    EXPECT_EQ(directory.error().code, ErrorCode::NotFound);

    // Another program's format - here a style library's own text - is not
    // read, and is said to be what it is not.
    const fs::path library =
        scratch.write("lines.4d", "worldstyle \"X\" { move 0 0 draw 1 0 }\n");
    const auto other = katana::cad::readCustomisationFile(library);
    ASSERT_FALSE(other.ok());
    EXPECT_EQ(other.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(other.error().message, "not a Katana customisation file");
    EXPECT_EQ(other.error().context.rfind(katana::core::pathToUtf8(library), 0), 0u)
        << "the file first: " << other.error().describe();

    // An empty file is not one either, and is refused rather than read as a
    // customisation of nothing.
    const auto empty = katana::cad::readCustomisationFile(scratch.write("empty.json", ""));
    ASSERT_FALSE(empty.ok());
    EXPECT_EQ(empty.error().code, ErrorCode::ParseFailure);

    // A customisation with a member the format does not know: which FILE,
    // then which entry.
    const fs::path typo = scratch.write(
        "typo.json", R"({"format": "katana-customisation", "version": 1, "name": "T",
 "codes": [{"key": "WM*", "sets": "feature", "linesytle": "WATR Main"}]})");
    const auto strict = katana::cad::readCustomisationFile(typo);
    ASSERT_FALSE(strict.ok());
    EXPECT_EQ(strict.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(strict.error().context.rfind(katana::core::pathToUtf8(typo), 0), 0u)
        << strict.error().describe();
    EXPECT_NE(strict.error().message.find("\"WM*\""), std::string::npos)
        << strict.error().describe();
    EXPECT_NE(strict.error().message.find("\"linesytle\""), std::string::npos)
        << strict.error().describe();
}

// ---- the customisation compiled in, and the seam ---------------------------------------------

TEST(CustomisationHost, TheCompiledInCustomisationIsReadOrThereIsNoneAndNeverOneThatDidNotRead)
{
    // Whichever this build is. With none: none, and NO PROBLEM - an absent
    // built-in is the ordinary case, not a fault. With one: it was read, and
    // carries the digest of its bytes. What this build may not be is the
    // third thing: a file compiled in that is no customisation. The program
    // still starts with that, and says so; a release made from such a build
    // would draw plain lines, and this is the test that stops it - the
    // configure checks only that the file is there.
    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    if (compiledIn.customisation == nullptr) {
        EXPECT_TRUE(compiledIn.digest.empty());
        EXPECT_TRUE(compiledIn.problem.empty()) << compiledIn.problem;
    } else {
        EXPECT_TRUE(compiledIn.problem.empty());
        EXPECT_TRUE(katana::entity::validateCustomisationName(compiledIn.customisation->name).ok());
        EXPECT_EQ(compiledIn.digest.size(), 16u);
    }
    // Parsed once: the same object every time.
    EXPECT_EQ(&compiledIn, &katana::cad::compiledInCustomisation());
}

TEST(CustomisationHost, WithTheSeamUnsetTheBuiltInIsTheCompiledInOne)
{
    const ScopedSeam seam(nullptr);
    const BuiltInCustomisation builtIn = katana::cad::builtInCustomisation();
    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    EXPECT_EQ(builtIn.customisation, compiledIn.customisation) << "the very same one, shared";
    EXPECT_EQ(builtIn.digest, compiledIn.digest);
    EXPECT_EQ(builtIn.problem, compiledIn.problem);
}

TEST(CustomisationHost, TheSeamSaysNoneAndThereIsNoBuiltInWhateverIsCompiledIn)
{
    for (const char* none : {"none", "NONE", "None"}) {
        const ScopedSeam seam(none);
        const BuiltInCustomisation builtIn = katana::cad::builtInCustomisation();
        EXPECT_EQ(builtIn.customisation, nullptr) << none;
        EXPECT_TRUE(builtIn.digest.empty()) << none;
        EXPECT_TRUE(builtIn.problem.empty()) << "asked for, so not a problem: " << none;
    }
}

TEST(CustomisationHost, TheSeamNamesAFileAndThatFileIsTheBuiltIn)
{
    const Scratch scratch("seam-file");
    const fs::path path = scratch.write("small.customisation.json", kSmall);
    const ScopedSeam seam(path.string().c_str());

    const BuiltInCustomisation builtIn = katana::cad::builtInCustomisation();
    ASSERT_NE(builtIn.customisation, nullptr) << builtIn.problem;
    EXPECT_TRUE(builtIn.problem.empty());
    EXPECT_EQ(builtIn.customisation->name, "Small Built In");
    EXPECT_EQ(builtIn.customisation->library.size(), 2u);
    EXPECT_EQ(builtIn.customisation->map.size(), 2u);
    EXPECT_EQ(builtIn.digest, fnv1a(kSmall));
}

TEST(CustomisationHost, AFileTheSeamNamesThatDoesNotReadGivesNoBuiltInAndSaysWhy)
{
    const Scratch scratch("seam-unread");
    // Not there; and there, but not a customisation.
    const fs::path nowhere = scratch.root / "nowhere.json";
    const fs::path other = scratch.write("lines.4d", "worldstyle \"X\" { move 0 0 }\n");
    for (const fs::path& path : {nowhere, other}) {
        const ScopedSeam seam(path.string().c_str());
        const BuiltInCustomisation builtIn = katana::cad::builtInCustomisation();
        // None - and never the compiled-in one in its place: a test that
        // named a fixture and was given something else would pass on it.
        EXPECT_EQ(builtIn.customisation, nullptr) << path;
        EXPECT_TRUE(builtIn.digest.empty()) << path;
        EXPECT_NE(builtIn.problem.find(katana::cad::kBuiltInCustomisationVariable),
                  std::string::npos)
            << builtIn.problem;
        EXPECT_NE(builtIn.problem.find(path.filename().string()), std::string::npos)
            << builtIn.problem;
    }
}

TEST(CustomisationHost, TheSeamIsPutBackAsItWasWhenATestEnds)
{
    // The helper these tests lean on, checked: whatever the variable was when
    // the test began - set or not - it is that again afterwards.
    const char* variable = katana::cad::kBuiltInCustomisationVariable;
    const char* found = std::getenv(variable);
    const std::optional<std::string> before =
        found == nullptr ? std::nullopt : std::optional<std::string>(found);
    {
        const ScopedSeam seam("none");
        ASSERT_NE(std::getenv(variable), nullptr);
        EXPECT_STREQ(std::getenv(variable), "none");
        EXPECT_EQ(seam.before(), before);
    }
    const char* after = std::getenv(variable);
    EXPECT_EQ(after == nullptr ? std::nullopt : std::optional<std::string>(after), before);
}

// ---- the rule of the seam, against a built-in that IS compiled in ----------------------------
//
// The tests above go through the environment and whatever this build compiled
// in - nothing, in a clone and in CI - so "whatever is compiled in" and "never
// the compiled-in one in its place" could not fail there. These give the rule
// a compiled-in customisation to override.

TEST(CustomisationHost, WithNothingSaidTheBuiltInIsTheCompiledInOneAsItIs)
{
    const BuiltInCustomisation compiledIn = standIn();
    ASSERT_NE(compiledIn.customisation, nullptr);
    const BuiltInCustomisation builtIn = katana::cad::builtInCustomisationFor("", compiledIn);
    EXPECT_EQ(builtIn.customisation, compiledIn.customisation) << "the very same one, shared";
    EXPECT_EQ(builtIn.customisation->name, "Stand In");
    EXPECT_EQ(builtIn.digest, compiledIn.digest);
    EXPECT_TRUE(builtIn.problem.empty());

    // One that did not read is handed on with its reason, not hidden.
    BuiltInCustomisation damaged;
    damaged.problem = "the customisation compiled into this program is not read: it is damaged";
    const BuiltInCustomisation passed = katana::cad::builtInCustomisationFor("", damaged);
    EXPECT_EQ(passed.customisation, nullptr);
    EXPECT_EQ(passed.problem, damaged.problem);
}

TEST(CustomisationHost, SayingNoneGivesNoBuiltInThoughOneIsCompiledIn)
{
    const BuiltInCustomisation compiledIn = standIn();
    ASSERT_NE(compiledIn.customisation, nullptr);
    for (const char* none : {"none", "NONE", "None"}) {
        const BuiltInCustomisation builtIn =
            katana::cad::builtInCustomisationFor(none, compiledIn);
        EXPECT_EQ(builtIn.customisation, nullptr) << none;
        EXPECT_TRUE(builtIn.digest.empty()) << none;
        EXPECT_TRUE(builtIn.problem.empty()) << "asked for, so not a problem: " << none;
    }
}

TEST(CustomisationHost, AFileNamedTakesThePlaceOfTheCompiledInCustomisation)
{
    const Scratch scratch("seam-over-compiled-in");
    const fs::path path = scratch.write("small.customisation.json", kSmall);
    const BuiltInCustomisation builtIn =
        katana::cad::builtInCustomisationFor(katana::core::pathToUtf8(path), standIn());
    ASSERT_NE(builtIn.customisation, nullptr) << builtIn.problem;
    EXPECT_EQ(builtIn.customisation->name, "Small Built In") << "the file, not the stand-in";
    EXPECT_EQ(builtIn.digest, fnv1a(kSmall));
    EXPECT_TRUE(builtIn.problem.empty());
}

TEST(CustomisationHost, AFileNamedThatDoesNotReadGivesNoneAndNeverTheCompiledInOne)
{
    const Scratch scratch("seam-unread-over-compiled-in");
    // Not there; and there, but not a customisation.
    const fs::path nowhere = scratch.root / "nowhere.json";
    const fs::path other = scratch.write("lines.4d", "worldstyle \"X\" { move 0 0 }\n");
    const BuiltInCustomisation compiledIn = standIn();
    ASSERT_NE(compiledIn.customisation, nullptr);
    for (const fs::path& path : {nowhere, other}) {
        const BuiltInCustomisation builtIn =
            katana::cad::builtInCustomisationFor(katana::core::pathToUtf8(path), compiledIn);
        // A test that named a fixture and was given the compiled-in
        // customisation instead would pass on it.
        EXPECT_EQ(builtIn.customisation, nullptr) << path;
        EXPECT_TRUE(builtIn.digest.empty()) << path;
        EXPECT_NE(builtIn.problem.find(katana::cad::kBuiltInCustomisationVariable),
                  std::string::npos)
            << builtIn.problem;
        EXPECT_NE(builtIn.problem.find(path.filename().string()), std::string::npos)
            << builtIn.problem;
    }
}

TEST(CustomisationHost, TheFileIsNamedAsTextWhetherOrNotItsBytesAreUtf8)
{
    const Scratch scratch("seam-names");
    // UTF-8, under a directory whose name is not ASCII: "Zürich", the
    // u-umlaut the two bytes C3 BC.
    const fs::path zurich = scratch.root / katana::core::pathFromUtf8("Z\xC3\xBCrich");
    fs::create_directories(zurich);
    std::ofstream(zurich / "small.json", std::ios::binary) << kSmall;
    const BuiltInCustomisation named = katana::cad::builtInCustomisationFor(
        katana::core::pathToUtf8(zurich / "small.json"), {});
    ASSERT_NE(named.customisation, nullptr) << named.problem;
    EXPECT_EQ(named.customisation->name, "Small Built In");

    // Narrow bytes that are NOT UTF-8, which is how an environment gives a
    // name in the ANSI code page: "café" with its é the one byte E9. A path
    // built straight from them throws on Windows, out of a function that
    // promises to report and never throw. By a relative name in the scratch
    // directory, so that nothing but the name under test is narrow.
    const std::string narrow = "caf\xE9.customisation.json";
    const fs::path before = fs::current_path();
    fs::current_path(scratch.root);
    bool threw = false;
    bool made = false;
    BuiltInCustomisation absent;
    BuiltInCustomisation present;
    try {
        absent = katana::cad::builtInCustomisationFor(narrow, {});
        {
            std::ofstream out(narrow, std::ios::binary);
            made = static_cast<bool>(out);
            out << kSmall;
        }
        present = katana::cad::builtInCustomisationFor(narrow, {});
    } catch (const std::exception&) {
        threw = true;
    }
    // Restored before anything can end the test: the scratch directory is
    // removed when it does.
    fs::current_path(before);
    ASSERT_FALSE(threw) << "a file that cannot be named is reported, never thrown";
    // Not there yet: no built-in, and the reason.
    EXPECT_EQ(absent.customisation, nullptr);
    EXPECT_NE(absent.problem.find(katana::cad::kBuiltInCustomisationVariable), std::string::npos)
        << absent.problem;
    if (!made) {
        // A file system that takes only UTF-8 names refuses this one.
        GTEST_SKIP() << "this system makes no file of that narrow name";
    }
    // And it is THE file the C runtime made under that narrow name.
    ASSERT_NE(present.customisation, nullptr) << present.problem;
    EXPECT_EQ(present.customisation->name, "Small Built In");
}

TEST(CustomisationHost, TheRenamesMadeWithNoNameAnswerWithTheCompiledInNameWhateverTheSeamSays)
{
    // builtinRenames() with no name is for a caller with no Document to ask,
    // and answers with the customisation COMPILED INTO this program - no name,
    // and so nothing, in a build that has none. The seam changes the built-in
    // of a run, which a Document is told (CustomisationState::builtIn); it
    // does not change what was compiled in. With the seam naming the small
    // fixture, the table must still not answer with "Small Built In".
    const Scratch scratch("renames-and-the-seam");
    const fs::path path = scratch.write("small.customisation.json", kSmall);
    const ScopedSeam seam(path.string().c_str());
    ASSERT_NE(katana::cad::builtInCustomisation().customisation, nullptr);

    const BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    const std::string name =
        compiledIn.customisation ? compiledIn.customisation->name : std::string();
    ASSERT_NE(name, "Small Built In") << "this build compiled in the test's own fixture name";
    // Eight earlier names in two sets of four (customisation_record.hpp).
    const std::vector<katana::cad::RenamedSource> renames = katana::cad::builtinRenames();
    ASSERT_EQ(renames.size(), 8u);
    for (const katana::cad::RenamedSource& rename : renames) {
        EXPECT_EQ(rename.now, name);
    }
}

// ---- what a session starts with --------------------------------------------------------------

TEST(CustomisationStart, WithNoHostNothingIsInstalledAndThatIsNoProblem)
{
    Document document;
    const CustomisationStart report = katana::cad::startCustomisation(document, {});
    EXPECT_EQ(report.installed, CustomisationOrigin::None);
    EXPECT_TRUE(report.name.empty());
    EXPECT_EQ(report.definitions, 0u);
    EXPECT_EQ(report.rules, 0u);
    EXPECT_TRUE(report.problems.empty());
    EXPECT_FALSE(report.keptFromAnotherBuiltIn);
    // The Document is the empty one it was.
    EXPECT_TRUE(document.customisationState() == CustomisationState{});
    EXPECT_TRUE(document.styleLibrary().empty());
    EXPECT_TRUE(document.surveyMap().empty());
    EXPECT_EQ(document.customisationGeneration(), 0u);
}

TEST(CustomisationStart, InstallingABuiltInThatIsNotThereIsRefusedAndTheDocumentIsLeftAsItWas)
{
    // The one function a start and CUSTOMISE RESET both install the built-in
    // through. Handed a host's built-in that holds none, it refuses - the
    // verb asks first and never reaches this, a front end calling it may.
    Document document;
    const auto none =
        katana::cad::installBuiltInCustomisation(document, BuiltInCustomisation{}, true);
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(none.error().message, "there is no built-in customisation to install");
    EXPECT_TRUE(document.customisationState() == CustomisationState{});
    EXPECT_EQ(document.customisationGeneration(), 0u);

    // With one, `kept` is the caller's to say: a RESET while a kept file is
    // there installs the built-in as a session the next start would NOT give.
    const auto installed =
        katana::cad::installBuiltInCustomisation(document, builtInFrom(kSmall), false);
    ASSERT_TRUE(installed.ok()) << installed.error().describe();
    const CustomisationState& state = document.customisationState();
    EXPECT_EQ(state.origin, CustomisationOrigin::BuiltIn);
    EXPECT_FALSE(state.kept);
    ASSERT_TRUE(state.basedOn.has_value());
    EXPECT_EQ(state.basedOn->name, "Small Built In");
    EXPECT_EQ(state.basedOn->digest, fnv1a(kSmall));
}

TEST(CustomisationStart, TheBuiltInIsInstalledWhenNothingIsKept)
{
    const Scratch scratch("built-in");
    CustomisationHost host;
    host.builtIn = builtInFrom(kSmall);
    host.keptFile = scratch.root / "customisation.json"; // nobody has kept anything

    Document document;
    const CustomisationStart report = katana::cad::startCustomisation(document, host);

    EXPECT_EQ(report.installed, CustomisationOrigin::BuiltIn);
    EXPECT_EQ(report.name, "Small Built In");
    EXPECT_EQ(report.definitions, 2u);
    EXPECT_EQ(report.symbols, 1u);
    EXPECT_EQ(report.rules, 2u);
    EXPECT_EQ(report.colours, 1u);
    EXPECT_TRUE(report.problems.empty()) << report.problems.front();
    EXPECT_FALSE(report.keptFromAnotherBuiltIn);

    const CustomisationState& state = document.customisationState();
    EXPECT_EQ(state.name, "Small Built In");
    EXPECT_EQ(state.origin, CustomisationOrigin::BuiltIn);
    EXPECT_TRUE(state.kept) << "it is what the next start would give";
    EXPECT_EQ(state.builtIn, "Small Built In");
    EXPECT_EQ(state.description, "Two definitions and two rules, for tests");
    EXPECT_EQ(state.notice, (std::vector<std::string>{"Written for these tests."}));
    EXPECT_EQ(state.linework.start, "S");
    EXPECT_EQ(state.sources,
              (std::vector<CustomisationSource>{{"Small Built In", true, true, {}}}));
    // Based on itself, so that a copy kept from it can be told from one made
    // against another edition.
    ASSERT_TRUE(state.basedOn.has_value());
    EXPECT_EQ(state.basedOn->name, "Small Built In");
    EXPECT_EQ(state.basedOn->digest, fnv1a(kSmall));

    EXPECT_EQ(document.styleLibrary().size(), 2u);
    EXPECT_EQ(document.surveyMap().lookup("FE07").resolved.model, "FENCES");
    // The host's own copy is untouched: it is shared, not handed over.
    EXPECT_FALSE(host.builtIn.customisation->basedOn.has_value());
    EXPECT_FALSE(document.isModified());
}

TEST(CustomisationStart, TheKeptFileIsInstalledInPreferenceToTheBuiltIn)
{
    const Scratch scratch("kept");
    CustomisationHost host;
    host.builtIn = builtInFrom(kSmall);
    host.keptFile = scratch.write("customisation.json", keptText(fnv1a(kSmall)));

    Document document;
    const CustomisationStart report = katana::cad::startCustomisation(document, host);

    EXPECT_EQ(report.installed, CustomisationOrigin::Kept);
    EXPECT_EQ(report.name, "Mine");
    EXPECT_EQ(report.definitions, 0u);
    EXPECT_EQ(report.symbols, 0u);
    EXPECT_EQ(report.rules, 1u);
    EXPECT_EQ(report.colours, 0u);
    EXPECT_TRUE(report.problems.empty());
    EXPECT_FALSE(report.keptFromAnotherBuiltIn) << "made from this very built-in";

    const CustomisationState& state = document.customisationState();
    EXPECT_EQ(state.name, "Mine");
    EXPECT_EQ(state.origin, CustomisationOrigin::Kept);
    EXPECT_TRUE(state.kept);
    // The built-in's name is the host's, whatever is installed.
    EXPECT_EQ(state.builtIn, "Small Built In");
    EXPECT_TRUE(document.styleLibrary().empty()) << "the kept one, not the two merged";
    EXPECT_EQ(document.surveyMap().lookup("ZZ1").resolved.model, "MINE");
}

TEST(CustomisationStart, AKeptFileMadeFromAnotherEditionOfTheBuiltInIsInstalledAndSaidToBe)
{
    const Scratch scratch("another-edition");
    CustomisationHost host;
    host.builtIn = builtInFrom(kSmall);

    // Made from bytes that are not this built-in's.
    host.keptFile = scratch.write("earlier.json", keptText("0123456789abcdef"));
    Document earlier;
    const CustomisationStart moved = katana::cad::startCustomisation(earlier, host);
    EXPECT_EQ(moved.installed, CustomisationOrigin::Kept) << "it is still the user's";
    EXPECT_TRUE(moved.keptFromAnotherBuiltIn);
    EXPECT_TRUE(moved.problems.empty()) << "something to say, not something wrong";

    // Made from a built-in of ANOTHER NAME: another built-in altogether,
    // whatever its digest is said to be.
    host.keptFile =
        scratch.write("renamed.json", keptText(fnv1a(kSmall), "Another Built In"));
    Document renamed;
    const CustomisationStart other = katana::cad::startCustomisation(renamed, host);
    EXPECT_EQ(other.installed, CustomisationOrigin::Kept);
    EXPECT_TRUE(other.keptFromAnotherBuiltIn);

    // One that does not say what it was made from cannot be said to differ.
    host.keptFile = scratch.write("silent.json", keptText(""));
    Document silent;
    EXPECT_FALSE(katana::cad::startCustomisation(silent, host).keptFromAnotherBuiltIn);

    // Nor can any, where this program has no built-in to have moved on.
    CustomisationHost bare;
    bare.keptFile = scratch.root / "earlier.json";
    Document withNone;
    const CustomisationStart alone = katana::cad::startCustomisation(withNone, bare);
    EXPECT_EQ(alone.installed, CustomisationOrigin::Kept);
    EXPECT_FALSE(alone.keptFromAnotherBuiltIn);
    EXPECT_TRUE(withNone.customisationState().builtIn.empty());
}

TEST(CustomisationStart, ABuiltInThatSaysWhatItWasMadeFromIsStillInstalledBasedOnItself)
{
    // A built-in that was itself exported from a session names the earlier
    // customisation IT was made from. By hand: the session started from it is
    // based on the built-in - its name, the digest of its bytes - and not on
    // that earlier one; so a copy kept from the session, read at the next
    // start, was made from this very built-in, and nothing has moved on.
    // (Left based on the earlier one, the copy carried a digest that is not
    // this built-in's and was reported as made from another, at every start.)
    std::string exported = kSmall;
    const std::string name = R"("name": "Small Built In",)";
    const std::size_t at = exported.find(name);
    ASSERT_NE(at, std::string::npos);
    exported.insert(at + name.size(),
                    R"( "basedOn": {"name": "Earlier Edition", "digest": "0123456789abcdef"},)");

    const Scratch scratch("built-in-based-on-another");
    CustomisationHost host;
    host.builtIn = builtInFrom(exported);
    ASSERT_NE(host.builtIn.customisation, nullptr);
    ASSERT_TRUE(host.builtIn.customisation->basedOn.has_value()) << "the fixture does say";
    EXPECT_EQ(host.builtIn.customisation->basedOn->name, "Earlier Edition");
    host.keptFile = scratch.root / "customisation.json";

    Document first;
    ASSERT_EQ(katana::cad::startCustomisation(first, host).installed,
              CustomisationOrigin::BuiltIn);
    ASSERT_TRUE(first.customisationState().basedOn.has_value());
    EXPECT_EQ(first.customisationState().basedOn->name, "Small Built In");
    EXPECT_EQ(first.customisationState().basedOn->digest, fnv1a(exported));

    // Kept, by hand as KEEP will, and started again.
    const auto text = katana::entity::customisationToJson(first.customisation());
    ASSERT_TRUE(text.ok()) << text.error().describe();
    scratch.write("customisation.json", *text);
    Document second;
    const CustomisationStart report = katana::cad::startCustomisation(second, host);
    EXPECT_EQ(report.installed, CustomisationOrigin::Kept);
    EXPECT_TRUE(report.problems.empty());
    EXPECT_FALSE(report.keptFromAnotherBuiltIn);
}

TEST(CustomisationStart, StartingOverAChangedSessionGivesTheSessionAFreshStartGives)
{
    // What a reset to the built-in is: the built-in installed over a session
    // that was changed since. The small built-in spells start "S" and says
    // nothing of the automation. By hand, the reset session spells start "S"
    // again and has both switches ON, the defaults - the very session a new
    // Document started with the same host has, which is what `kept` means.
    CustomisationHost host;
    host.builtIn = builtInFrom(kSmall);
    Document changed;
    ASSERT_EQ(katana::cad::startCustomisation(changed, host).installed,
              CustomisationOrigin::BuiltIn);
    katana::entity::LineworkCodes spelled;
    spelled.start = "BEG";
    ASSERT_TRUE(changed.setLineworkCodes(spelled).ok());
    changed.setAutomation({false, false});
    ASSERT_FALSE(changed.customisationState().kept);

    ASSERT_EQ(katana::cad::startCustomisation(changed, host).installed,
              CustomisationOrigin::BuiltIn);
    Document fresh;
    ASSERT_EQ(katana::cad::startCustomisation(fresh, host).installed,
              CustomisationOrigin::BuiltIn);

    EXPECT_TRUE(changed.customisationState() == fresh.customisationState());
    EXPECT_EQ(changed.customisationState().linework.start, "S");
    EXPECT_TRUE(changed.customisationState().automation.codesOnSurveyImport);
    EXPECT_TRUE(changed.customisationState().automation.lineworkOnSurveyImport);
    EXPECT_TRUE(changed.customisationState().kept);
}

TEST(CustomisationStart, AKeptFileThatDoesNotReadIsReportedAndTheBuiltInStands)
{
    const Scratch scratch("unreadable");
    CustomisationHost host;
    host.builtIn = builtInFrom(kSmall);
    host.keptFile = scratch.write("customisation.json", "{ this is not a customisation");

    Document document;
    const CustomisationStart report = katana::cad::startCustomisation(document, host);

    EXPECT_EQ(report.installed, CustomisationOrigin::BuiltIn);
    EXPECT_EQ(report.name, "Small Built In");
    ASSERT_EQ(report.problems.size(), 1u);
    // Which file, why, and what was done instead.
    EXPECT_NE(report.problems[0].find("customisation.json"), std::string::npos)
        << report.problems[0];
    EXPECT_NE(report.problems[0].find("not a Katana customisation file"), std::string::npos)
        << report.problems[0];
    EXPECT_NE(report.problems[0].find("the built-in customisation is used"), std::string::npos)
        << report.problems[0];
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::BuiltIn);
    EXPECT_TRUE(document.customisationState().kept);
    EXPECT_EQ(document.styleLibrary().size(), 2u);

    // With no built-in to stand in, nothing is loaded, and that is said.
    CustomisationHost bare;
    bare.keptFile = host.keptFile;
    Document empty;
    const CustomisationStart nothing = katana::cad::startCustomisation(empty, bare);
    EXPECT_EQ(nothing.installed, CustomisationOrigin::None);
    ASSERT_EQ(nothing.problems.size(), 1u);
    EXPECT_NE(nothing.problems[0].find("no customisation is loaded"), std::string::npos)
        << nothing.problems[0];
    EXPECT_EQ(empty.customisationState().origin, CustomisationOrigin::None);
    EXPECT_TRUE(empty.styleLibrary().empty());
}

TEST(CustomisationStart, ABuiltInThatWasNotReadIsReportedAndNothingIsInstalled)
{
    // What builtInCustomisation gives for a file the seam names that does
    // not read: none, and why.
    CustomisationHost host;
    host.builtIn.problem = "the built-in customisation is not read: it is damaged";
    Document document;
    const CustomisationStart report = katana::cad::startCustomisation(document, host);
    EXPECT_EQ(report.installed, CustomisationOrigin::None);
    EXPECT_EQ(report.problems, (std::vector<std::string>{host.builtIn.problem}));
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::None);
    EXPECT_TRUE(document.customisationState().builtIn.empty());
}

TEST(CustomisationStart, ABuiltInTheDocumentRefusesIsReportedAndNothingIsInstalled)
{
    // Built in code, as no file could be read: a name no project could record.
    Customisation unusable;
    unusable.name = "roads/2026";
    CustomisationHost host;
    host.builtIn.customisation = std::make_shared<const Customisation>(unusable);
    Document document;
    const CustomisationStart report = katana::cad::startCustomisation(document, host);
    EXPECT_EQ(report.installed, CustomisationOrigin::None);
    ASSERT_EQ(report.problems.size(), 1u);
    EXPECT_EQ(report.problems[0].rfind("the built-in customisation is not installed: ", 0), 0u)
        << report.problems[0];
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::None);
    EXPECT_FALSE(document.customisationState().kept);
}

TEST(CustomisationStart, WhatAStartFoundStaysOnTheDocumentAndIsInItsJsonReport)
{
    // A front end says what a start found once, where its errors go. Whoever
    // asks what the session holds later - a client of katana_mcp, which never
    // sees that stream - was told `origin: builtIn` and not that the
    // customisation it kept had been refused. So it is kept on the Document,
    // and the report gives it.
    const Scratch scratch("start-on-the-document");
    CustomisationHost host;
    host.builtIn = builtInFrom(kSmall);
    host.keptFile = scratch.write("customisation.json", "{ this is not a customisation");

    Document document;
    const CustomisationStart report = katana::cad::startCustomisation(document, host);
    const std::string sentence = "the kept customisation is not read, so the built-in "
                                 "customisation is used: ParseFailure: not a Katana "
                                 "customisation file";
    ASSERT_EQ(report.problems.size(), 1u);
    EXPECT_EQ(report.problems[0].rfind(sentence, 0), 0u) << report.problems[0];
    EXPECT_EQ(document.customisationState().startProblems, report.problems);
    EXPECT_FALSE(document.customisationState().keptFromAnotherBuiltIn);
    // Keys in alphabetical order, two blanks a level: `start` is the last
    // member, and the sentence begins its one line.
    const std::string json = katana::cad::customisationJson(document);
    EXPECT_NE(json.find("\"start\": {\n    \"keptFromAnotherBuiltIn\": false,\n    \"problems\": "
                        "[\n      \"" + sentence),
              std::string::npos)
        << json;

    // It is of the START: an edit of the session and a load into it change
    // neither.
    document.setAutomation({false, false});
    auto loaded = katana::entity::customisationFromJson(keptText(""));
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    ASSERT_TRUE(
        document.installCustomisation(std::move(*loaded), CustomisationOrigin::Loaded).ok());
    EXPECT_EQ(document.customisationState().name, "Mine");
    EXPECT_EQ(document.customisationState().startProblems, report.problems);

    // A kept customisation made from another edition of the built-in is no
    // problem, and is said too.
    host.keptFile = scratch.write("earlier.json", keptText("0123456789abcdef"));
    Document earlier;
    ASSERT_TRUE(katana::cad::startCustomisation(earlier, host).keptFromAnotherBuiltIn);
    EXPECT_TRUE(earlier.customisationState().keptFromAnotherBuiltIn);
    EXPECT_TRUE(earlier.customisationState().startProblems.empty());
    EXPECT_NE(katana::cad::customisationJson(earlier).find(
                  "\"start\": {\n    \"keptFromAnotherBuiltIn\": true,\n    \"problems\": []\n  }"),
              std::string::npos);

    // A start that found nothing leaves nothing - over what an earlier start
    // left, too: the first Document, started again with no kept file.
    const std::string nothing =
        "\"start\": {\n    \"keptFromAnotherBuiltIn\": false,\n    \"problems\": []\n  }";
    CustomisationHost clean;
    clean.builtIn = host.builtIn;
    EXPECT_TRUE(katana::cad::startCustomisation(document, clean).problems.empty());
    EXPECT_TRUE(document.customisationState().startProblems.empty());
    EXPECT_NE(katana::cad::customisationJson(document).find(nothing), std::string::npos);
    // And a Document that was never started says the same.
    const Document never;
    EXPECT_NE(katana::cad::customisationJson(never).find(nothing), std::string::npos);
}

TEST(CustomisationStart, RecordingWhatAStartFoundIsNoEditOfTheSessionAndIsReportedOnce)
{
    Document document;
    auto small = katana::entity::customisationFromJson(kSmall);
    ASSERT_TRUE(small.ok()) << small.error().describe();
    ASSERT_TRUE(
        document.installCustomisation(std::move(*small), CustomisationOrigin::BuiltIn, true).ok());
    std::vector<std::uint32_t> calls;
    const Document::ListenerHandle handle = document.addListener(
        [&calls](const DocumentChange& change) { calls.push_back(change.parts); });
    const std::uint64_t before = document.customisationGeneration();
    const std::vector<std::string> problems{"the kept customisation is not read"};

    document.setCustomisationStart(problems, true);
    // One notification, of the customisation's state alone, as for every
    // other part of it.
    ASSERT_EQ(calls.size(), 1u);
    EXPECT_EQ(calls[0], std::uint32_t{DocumentChange::Customisation});
    EXPECT_EQ(document.customisationGeneration(), before + 1);
    const CustomisationState& state = document.customisationState();
    EXPECT_EQ(state.startProblems, problems);
    EXPECT_TRUE(state.keptFromAnotherBuiltIn);
    // What a start found is no edit: the session is still the built-in, still
    // what the next start would give, and the drawing is untouched.
    EXPECT_EQ(state.origin, CustomisationOrigin::BuiltIn);
    EXPECT_TRUE(state.kept);
    EXPECT_FALSE(document.isModified());

    // The value it already has is nothing at all.
    document.setCustomisationStart(problems, true);
    EXPECT_EQ(calls.size(), 1u);
    EXPECT_EQ(document.customisationGeneration(), before + 1);
}

TEST(CustomisationStart, ASessionWrittenToTheKeptFileIsTheSessionTheNextStartGives)
{
    // What KEEP will do, by hand: the session as one customisation, written
    // as a file's text to the kept file. The next start, of another Document
    // with the same host, reads it back as the same session - kept, from the
    // same built-in.
    const Scratch scratch("round-trip");
    CustomisationHost host;
    host.builtIn = builtInFrom(kSmall);
    host.keptFile = scratch.root / "customisation.json";

    Document first;
    ASSERT_EQ(katana::cad::startCustomisation(first, host).installed,
              CustomisationOrigin::BuiltIn);
    // An edit, so that what is kept is not simply the built-in.
    first.setAutomation({false, true});
    const Customisation session = first.customisation();
    const auto text = katana::entity::customisationToJson(session);
    ASSERT_TRUE(text.ok()) << text.error().describe();
    scratch.write("customisation.json", *text);

    Document second;
    const CustomisationStart report = katana::cad::startCustomisation(second, host);
    EXPECT_EQ(report.installed, CustomisationOrigin::Kept);
    EXPECT_TRUE(report.problems.empty());
    EXPECT_FALSE(report.keptFromAnotherBuiltIn);
    EXPECT_TRUE(second.customisation() == session);
    EXPECT_FALSE(second.customisationState().automation.codesOnSurveyImport);
    EXPECT_TRUE(second.customisationState().kept);
    EXPECT_EQ(second.customisationState().origin, CustomisationOrigin::Kept);
}
