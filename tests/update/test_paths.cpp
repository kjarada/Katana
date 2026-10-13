// The path rules (include/katana/update/paths.hpp, src/katana_update/paths.cpp): what a file
// name from a manifest, or one read back from a journal, must pass before anything is
// written under it.
//
// The expectations come from the rules as the header states them and from the shared table
// tests/update/fixtures/path_rules.txt, which the two Python tools (the producer and the
// verifier, tests/update/tool_tests) are held to as well. They are never read off the code
// under test. The table is read here too, and any line this program answers differently is
// a failure, so the engine, the helper and both tools agree by construction.

#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <initializer_list>
#include <limits>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/update/paths.hpp"

#ifndef KATANA_UPDATE_FIXTURES
#error "tests/update/CMakeLists.txt passes the fixtures folder as KATANA_UPDATE_FIXTURES"
#endif

using namespace katana;
namespace fs = std::filesystem;

namespace {

bool accepts(std::string_view path)
{
    return update::checkManifestPath(path).ok();
}

// A rule that is broken by this path alone, and only by it: the status is a refusal, an
// InvalidArgument whose context shows the path.
void expectRefused(std::string_view path)
{
    const core::Status status = update::checkManifestPath(path);
    ASSERT_FALSE(status.ok()) << "accepted: '" << path << "'";
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument) << path;
    EXPECT_FALSE(status.error().message.empty()) << path;
}

void expectAccepted(std::string_view path)
{
    const core::Status status = update::checkManifestPath(path);
    EXPECT_TRUE(status.ok()) << "refused: '" << path << "': "
                             << (status.ok() ? "" : status.error().describe());
}

void expectAllRefused(std::initializer_list<std::string_view> paths)
{
    for (const std::string_view path : paths) {
        expectRefused(path);
    }
}

void expectAllAccepted(std::initializer_list<std::string_view> paths)
{
    for (const std::string_view path : paths) {
        expectAccepted(path);
    }
}

int hexValue(char c)
{
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    return -1; // the table writes upper-case hex only
}

// The table's escape: %XX (two upper-case hex digits) for every byte outside 0x21 to 0x7E and
// for '%'. A malformed escape fails the test that reads the table, not silently.
bool decodeEscapes(std::string_view field, std::string& out)
{
    out.clear();
    for (std::size_t i = 0; i < field.size(); ++i) {
        if (field[i] != '%') {
            out.push_back(field[i]);
            continue;
        }
        if (i + 2 >= field.size()) {
            return false; // "%" needs two more characters
        }
        const int high = hexValue(field[i + 1]);
        const int low = hexValue(field[i + 2]);
        if (high < 0 || low < 0) {
            return false;
        }
        out.push_back(static_cast<char>(high * 16 + low));
        i += 2;
    }
    return true;
}

std::vector<std::string> sequentialPaths(std::size_t count)
{
    std::vector<std::string> paths;
    paths.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        paths.push_back("share/katana/data/f" + std::to_string(i) + ".dat");
    }
    return paths;
}

core::Status checkSet(const std::vector<std::string>& paths, const std::vector<std::uint64_t>& sizes)
{
    return update::checkManifestPaths(paths, sizes);
}

core::Status checkSetOfPaths(const std::vector<std::string>& paths)
{
    return checkSet(paths, std::vector<std::uint64_t>(paths.size(), 1));
}

constexpr std::uint64_t kGiB = 1024ull * 1024 * 1024;

} // namespace

// ---- the shared table ---------------------------------------------------------------------------

TEST(UpdatePathRulesTable, EveryLineOfTheSharedTableIsAnsweredAsTheTableSays)
{
    const fs::path table = fs::path(KATANA_UPDATE_FIXTURES) / "path_rules.txt";
    std::ifstream in(table, std::ios::binary);
    ASSERT_TRUE(in.is_open()) << "cannot open " << table.string();

    std::string line;
    int lineNumber = 0;
    int accepted = 0;
    int refused = 0;
    while (std::getline(in, line)) {
        ++lineNumber;
        if (!line.empty() && line.back() == '\r') {
            line.pop_back();
        }
        if (line.empty() || line.front() == '#') {
            continue;
        }
        // verdict TAB escaped-path TAB why; an empty path is an empty middle field.
        const std::size_t firstTab = line.find('\t');
        ASSERT_NE(firstTab, std::string::npos) << "line " << lineNumber << " has no tab";
        const std::size_t secondTab = line.find('\t', firstTab + 1);
        ASSERT_NE(secondTab, std::string::npos) << "line " << lineNumber << " has one tab only";
        const std::string verdict = line.substr(0, firstTab);
        const std::string escaped = line.substr(firstTab + 1, secondTab - firstTab - 1);
        const std::string why = line.substr(secondTab + 1);
        ASSERT_TRUE(verdict == "ok" || verdict == "no") << "line " << lineNumber << ": " << verdict;

        std::string path;
        ASSERT_TRUE(decodeEscapes(escaped, path))
            << "line " << lineNumber << ": bad escape in '" << escaped << "'";
        const bool expected = verdict == "ok";
        const core::Status status = update::checkManifestPath(path);
        EXPECT_EQ(status.ok(), expected)
            << "line " << lineNumber << " ('" << escaped << "', " << why << "): the table says "
            << verdict << ", checkManifestPath says "
            << (status.ok() ? "ok" : "no: " + status.error().describe());
        ++(expected ? accepted : refused);
    }
    // The table was read and holds both kinds: a loop over an empty or one-sided file would
    // pass, and say nothing.
    // (At least what the table held when this was written: 16 and 40. A truncated table fails.)
    EXPECT_GE(accepted, 16);
    EXPECT_GE(refused, 40);
}

TEST(UpdatePathRulesTable, TheDecoderReadsPercentEscapesAsTheHeaderOfTheTableDescribesThem)
{
    std::string out;
    ASSERT_TRUE(decodeEscapes("bin/a%20b", out));
    EXPECT_EQ(out, "bin/a b");
    ASSERT_TRUE(decodeEscapes("share/caf%C3%A9.txt", out));
    EXPECT_EQ(out, "share/caf\xC3\xA9.txt");
    ASSERT_TRUE(decodeEscapes("a%25b", out));
    EXPECT_EQ(out, "a%b");
    ASSERT_TRUE(decodeEscapes("", out));
    EXPECT_EQ(out, "");
    EXPECT_FALSE(decodeEscapes("a%2", out));
    EXPECT_FALSE(decodeEscapes("a%", out));
    EXPECT_FALSE(decodeEscapes("a%zz", out));
    EXPECT_FALSE(decodeEscapes("a%2f", out)) << "lower-case hex is not what the table writes";
}

// ---- the plain names that must pass -------------------------------------------------------------

TEST(UpdatePaths, PlainNamesUnderTheFourInstallFoldersPass)
{
    expectAllAccepted({
        "bin/katana.exe",
        "bin/katana_cli.exe",
        "bin/libgdal-39.dll",
        "bin/platforms/qwindows.dll",
        "share/katana/readme.txt",
        "share/proj/proj.db",
        "etc/katana/settings.ini",
        "etc/ssl/certs/ca-bundle.crt",
        "lib/libx.so.1.2",
        "lib/x86_64-linux-gnu/libz.so.1",
        "share/katana/news-0.10.0.txt",
        "bin/a b.txt",    // a space inside a name is fine; at the end of one it is not
        "bin/.hidden",    // a dot file
        "bin/a.b.c.d",
    });
}

TEST(UpdatePaths, EveryPrintablePunctuationMarkThatNoRuleNamesIsAllowed)
{
    // The forbidden set is exactly backslash : * ? " < > | (and the controls). Every other
    // printable ASCII character is a legal name character on the three file systems.
    expectAllAccepted({
        "share/katana/a+b,c;d=e@f[1](2){3}^`!#$%&'-_.txt",
        "share/katana/~",       // a tilde with no digit after it
        "share/katana/a~",      // ... at the end of a name
        "share/katana/~a",      // ... at the start
        "share/katana/a~b~c",   //
    });
}

// ---- one rule at a time ---------------------------------------------------------------------------

TEST(UpdatePaths, AnEmptyPathIsRefused)
{
    expectRefused("");
}

TEST(UpdatePaths, AnAbsolutePathOrOneWithADriveIsRefused)
{
    expectAllRefused({"/bin/a.dll", "/", "//server/share/x", "C:/bin/a.dll", "C:bin/a.dll",
                      "c:/bin/a.dll", "\\bin\\a.dll", "\\\\server\\share\\x"});
}

TEST(UpdatePaths, ABackslashAColonOrAWildcardIsRefusedWhereverItStands)
{
    // One character at a time, in the middle of an otherwise good name: each is refused by its own.
    for (const char c : std::string_view("\\:*?\"<>|")) {
        const std::string inside = std::string("bin/a") + c + "b.dll";
        expectRefused(inside);
        expectRefused(std::string("bin/") + c);
        expectRefused(std::string("bin/x") + c);
        expectRefused(std::string(1, c) + "bin/x");
        expectRefused(std::string("share/dir") + c + "/x");
    }
    // An NTFS alternate data stream: a name, a colon, a stream.
    expectRefused("share/katana/readme.txt:hidden");
    expectRefused("share/katana/readme.txt::$DATA");
}

TEST(UpdatePaths, AnEmptyDotOrDotDotComponentIsRefused)
{
    expectAllRefused({"bin//a.dll", "bin/a.dll/", "bin/", "share/katana//x", "bin/./a.dll",
                      "bin/../etc/x", "bin/a/..", "bin/a/.", "./bin/a.dll", "../bin/a.dll",
                      "bin/a/../../b", "share/katana/..", "bin/..", "bin/."});
    // A dot with a name after it is a name, not a directory reference; three dots are a name
    // too, but one that ends in a dot, which Windows drops (the rule below).
    expectAllAccepted({"bin/..a/b", "bin/..a.dll", "bin/.a/b"});
    expectRefused("bin/.../a.dll");
}

TEST(UpdatePaths, TheFirstComponentMustBeBinShareEtcOrLibExactlyAndHaveAFileBelowIt)
{
    expectAllRefused({"other/a.txt", "LICENSE", "README.md", "bins/a", "sharee/a", "etcetera/a",
                      "libs/a", "Bin/a.dll", "BIN/a.dll", "Share/katana/a", "ETC/a", "Lib/a",
                      "bin", "share", "etc", "lib", "bin/", "docs/a.txt", "build/bin/a"});
    expectAllAccepted({"bin/a", "share/a", "etc/a", "lib/a"});
}

TEST(UpdatePaths, ANonAsciiNameIsRefusedAsIsAnyControlCharacterAndDelete)
{
    // ASCII only (paths.hpp): é as UTF-8, the same é as a lone Latin-1 byte, a Cyrillic letter,
    // a CJK character, an emoji and a no-break space.
    expectAllRefused({"share/caf\xC3\xA9.txt", "share/caf\xE9.txt", "share/\xD0\xB0.txt",
                      "share/\xE6\x97\xA5.txt", "share/\xF0\x9F\x98\x80.txt",
                      "share/a\xC2\xA0" "b.txt", "share/\xEF\xBB\xBF" "a.txt"});
    // Every control character, one at a time, and DEL.
    for (int byte = 0; byte < 0x20; ++byte) {
        expectRefused(std::string("bin/a") + static_cast<char>(byte) + "b");
    }
    expectRefused(std::string("bin/a") + '\x7F' + "b");
    // Every byte from 0x80 to 0xFF, one at a time.
    for (int byte = 0x80; byte <= 0xFF; ++byte) {
        expectRefused(std::string("bin/a") + static_cast<char>(byte) + "b");
    }
    // The boundaries of the allowed range: 0x20 (space) and 0x7E (~) inside a name pass; the
    // bytes just outside, 0x1F and 0x7F, do not.
    expectAccepted("bin/a b");
    expectAccepted("bin/a~b");
    expectRefused(std::string("bin/a") + '\x1F' + "b");
    expectRefused(std::string("bin/a") + '\x7F' + "b");
}

TEST(UpdatePaths, ANulByteAnywhereIsRefusedAndNeverEndsThePathEarly)
{
    // A C string would stop at the NUL and see the good name "bin/a.dll".
    expectRefused(std::string("bin/a.dll\0", 10));
    expectRefused(std::string("bin/a.dll\0../../x", 17));
    expectRefused(std::string("bin/a\0.dll", 10));
    expectRefused(std::string("\0bin/a.dll", 10));
    expectRefused(std::string("\0", 1));
}

TEST(UpdatePaths, AComponentEndingInADotOrASpaceIsRefusedBecauseWindowsDropsIt)
{
    expectAllRefused({"bin/a.", "bin/a ", "bin/a..", "bin/a  ", "bin/a. ", "bin/a .",
                      "share/dir./x", "share/dir /x", "share/a./b/c.txt", "share/a/b /c.txt",
                      "bin/ ", "bin/."});
    expectAllAccepted({"bin/a.b", "bin/.a", "bin/a b", "bin/ a", "share/d.e/f.g"});
}

TEST(UpdatePaths, AWindowsDeviceNameIsRefusedInAnyCaseWithOrWithoutAnExtension)
{
    std::vector<std::string> devices{"con", "prn", "aux", "nul"};
    for (char digit = '1'; digit <= '9'; ++digit) {
        devices.push_back(std::string("com") + digit);
        devices.push_back(std::string("lpt") + digit);
    }
    ASSERT_EQ(devices.size(), 22u); // 4 + 9 + 9
    for (const std::string& lower : devices) {
        std::string upper = lower;
        std::transform(upper.begin(), upper.end(), upper.begin(), [](char c) {
            return c >= 'a' && c <= 'z' ? static_cast<char>(c - 'a' + 'A') : c;
        });
        std::string mixed = lower;
        mixed[0] = static_cast<char>(mixed[0] - 'a' + 'A');
        for (const std::string& name : {lower, upper, mixed}) {
            expectRefused("bin/" + name);
            expectRefused("bin/" + name + ".txt");
            expectRefused("bin/" + name + ".tar.gz");
            expectRefused("share/" + name + "/x.dat");  // as a folder
            expectRefused("share/katana/" + name + ".d/x.dat");
        }
    }
}

TEST(UpdatePaths, NamesThatMerelyStartLikeADeviceOrContainOneAreNotDevices)
{
    expectAllAccepted({"bin/console.exe",  // CON followed by letters
                       "bin/com.txt",      // COM with no digit
                       "bin/com10.dll",    // only COM1 to COM9 are devices
                       "bin/lpt10",        //
                       "bin/commander",    //
                       "bin/null",         // NUL is a device, NULL is not
                       "bin/nulls.txt",    //
                       "bin/auxiliary.dll",
                       "bin/prnt.dll",
                       "bin/con1.dll",     // CON with a digit is not CON
                       "bin/xcon.dll",     // the device name must be the whole stem
                       "bin/a.con",        // after the first dot it is an extension
                       "bin/a.nul.txt",
                       "bin/x-com1.dll"});
}

TEST(UpdatePaths, ComZeroAndLptZeroAreOrdinaryNamesUnlessTheSharedTableSaysOtherwise)
{
    // paths.hpp: COM1 to COM9 and LPT1 to LPT9 only. The producer's author found COM0 and
    // LPT0 ordinary on Windows 11, and the table has no line that makes them devices; if the
    // owner rules otherwise the table and this test change together.
    expectAllAccepted({"bin/com0", "bin/COM0.dll", "bin/lpt0", "bin/LPT0.txt"});
}

TEST(UpdatePaths, An8Dot3ShortNameIsRefusedWhereAComponentHasATildeThenADigit)
{
    expectAllRefused({"bin/PROGRA~1.DLL", "bin/progra~1.dll", "share/KATANA~12/x", "bin/a~0",
                      "bin/a~9", "bin/~1", "bin/~1.dll", "bin/a~1b~c", "bin/x~2.tar.gz",
                      "share/dir~3/file.txt"});
    expectAllAccepted({"bin/a~b.dll", "bin/a~", "bin/~a", "bin/~", "bin/a~~b",
                       "bin/a~.dll"});
    // A tilde ending one component and a digit starting the next are two components.
    expectAllAccepted({"share/a~/1.txt", "share/katana~/2/x"});
}

TEST(UpdatePaths, TheUpdatersOwnRecordsAreRefusedWhereverTheySitAndInAnyAsciiCase)
{
    expectAllRefused({
        "share/katana/install.json",
        "share/katana/installed.json",
        "share/katana/installed.manifest.json",
        "share/katana/installed.manifest.json.sig",
        // any case: on NTFS and APFS INSTALL.JSON is install.json
        "share/katana/INSTALL.JSON",
        "share/katana/Install.Json",
        "share/katana/INSTALLED.JSON",
        "share/katana/Installed.Manifest.JSON",
        "share/katana/installed.manifest.json.SIG",
        // not only in share/katana: the last component is what is reserved
        "bin/install.json",
        "etc/installed.json",
        "lib/sub/dir/installed.manifest.json.sig",
        "etc/katana/INSTALL.json",
    });
}

TEST(UpdatePaths, TheUpdatersWorkingFolderIsRefusedAsAnyComponentInAnyAsciiCase)
{
    expectAllRefused({"share/.update/x", "share/.UPDATE/x", "share/.Update/x", "bin/.update",
                      "bin/.UPDATE", "bin/sub/.update/deep/x", "lib/a/b/.update", "etc/.update/lock"});
    // A name that merely starts with it, or contains it, is an ordinary name.
    expectAllAccepted({"share/.updated/x", "share/.updates", "share/x.update/y", "share/a.update",
                       "share/.update.d/x", "share/update/x", "share/.upd/x"});
}

TEST(UpdatePaths, NamesThatOnlyLookLikeRecordsAreOrdinaryFiles)
{
    // The header narrows the design's installed*.json to the four literal names the helper
    // writes; a neighbour of them is a file like any other.
    expectAllAccepted({
        "share/katana/installed.json.bak",
        "share/katana/install.json.txt",
        "share/katana/installed.manifest.json.bak",
        "share/katana/installed.manifest.json.sig.bak",
        "share/katana/installed.other.json",
        "share/katana/xinstall.json",
        "share/katana/install.jsonx",
        "share/katana/my-installed.json",
        "share/katana/installed_json",
        "share/katana/install.json.d/x",   // a folder of that name is not the file
    });
}

// ---- lowerAscii --------------------------------------------------------------------------------------

TEST(UpdateLowerAscii, OnlyTheLettersAToZAreFoldedAndEveryOtherByteIsKept)
{
    EXPECT_EQ(update::lowerAscii(""), "");
    EXPECT_EQ(update::lowerAscii("Share/KATANA/Install.JSON"), "share/katana/install.json");
    // The neighbours of the two ends of each range differ from a letter by one: '@' (0x40) and '[' (0x5B)
    // either side of A to Z, '`' (0x60) and '{' (0x7B) either side of a to z. None of them is a letter.
    EXPECT_EQ(update::lowerAscii("@AZ[`az{"), "@az[`az{");
    // A byte above 0x7F is never a letter, whatever the locale: the UTF-8 of an A with a diaeresis
    // (0xC3 0x84) and a lone 0x80 are returned as they came, and so is a NUL inside the text.
    EXPECT_EQ(update::lowerAscii("\xC3\x84Z\x80"), "\xC3\x84z\x80");
    EXPECT_EQ(update::lowerAscii(std::string_view("A\0B", 3)), std::string_view("a\0b", 3));
}

// ---- isReservedName ----------------------------------------------------------------------------------

TEST(UpdateReservedNames, TheFourRecordNamesAreReservedAsABareNameOrAtTheEndOfAPath)
{
    for (const std::string_view name :
         {"install.json", "installed.json", "installed.manifest.json",
          "installed.manifest.json.sig"}) {
        EXPECT_TRUE(update::isReservedName(name)) << name;
        EXPECT_TRUE(update::isReservedName("share/katana/" + std::string(name))) << name;
        EXPECT_TRUE(update::isReservedName("a/b/c/d/" + std::string(name))) << name;
    }
}

TEST(UpdateReservedNames, AnyAsciiCaseIsTheSameName)
{
    EXPECT_TRUE(update::isReservedName("Share/Katana/INSTALL.JSON"));
    EXPECT_TRUE(update::isReservedName("INSTALLED.MANIFEST.JSON.SIG"));
    EXPECT_TRUE(update::isReservedName("x/Installed.Manifest.Json"));
    EXPECT_TRUE(update::isReservedName("iNsTaLlEd.JsOn"));
}

TEST(UpdateReservedNames, TheWorkFolderIsReservedAsAnyComponent)
{
    EXPECT_TRUE(update::isReservedName(".update"));
    EXPECT_TRUE(update::isReservedName(".UPDATE"));
    EXPECT_TRUE(update::isReservedName(".update/lock"));
    EXPECT_TRUE(update::isReservedName("R/.update/tx/journal"));
    EXPECT_TRUE(update::isReservedName("a/.Update/b/c"));
}

TEST(UpdateReservedNames, ABackslashSeparatesComponentsToo)
{
    // This is also the "never removed" test, which may be handed a native path: a backslash
    // can only make more names reserved here, not fewer.
    EXPECT_TRUE(update::isReservedName("share\\katana\\install.json"));
    EXPECT_TRUE(update::isReservedName("R\\.update\\lock"));
}

TEST(UpdateReservedNames, NamesThatAreNotRecordsAreNotReserved)
{
    for (const std::string_view name :
         {"", "/", "bin/katana.exe", "installed.json.bak", "install.json.txt", "xinstall.json",
          "installed.manifest.json.sigx", "installed.manifest", "installed", "install",
          ".updated", ".updat", "update", "x.update", "share/katana/installed.other.json",
          "install.json/x", "installed.json/y"}) {
        EXPECT_FALSE(update::isReservedName(name)) << "'" << name << "'";
    }
}

// ---- checkManifestPath: what an error says ---------------------------------------------------------------

TEST(UpdatePaths, AnErrorShowsTheOrdinaryPathAsItIs)
{
    const core::Status status = update::checkManifestPath("bin/con.txt");
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(status.error().context, "bin/con.txt");
}

TEST(UpdatePaths, AnErrorSpellsBytesThatAreNotPrintableAndBoundsAHugePath)
{
    const core::Status withNul = update::checkManifestPath(std::string("bin/a\0b\n", 8));
    ASSERT_FALSE(withNul.ok());
    EXPECT_EQ(withNul.error().context, "bin/a\\x00b\\x0a");
    EXPECT_EQ(withNul.error().describe().find('\n'), std::string::npos);

    const core::Status nonAscii = update::checkManifestPath("share/caf\xC3\xA9");
    ASSERT_FALSE(nonAscii.ok());
    EXPECT_EQ(nonAscii.error().context, "share/caf\\xc3\\xa9");

    // A megabyte of path is refused without being copied into the message.
    const core::Status huge = update::checkManifestPath("bin/" + std::string(1u << 20, 'a') + ".");
    ASSERT_FALSE(huge.ok());
    EXPECT_LE(huge.error().context.size(), 210u);
}

TEST(UpdatePaths, EachRuleSaysSomethingOfItsOwn)
{
    // Different faults, different sentences: a person reading a log can tell them apart.
    std::vector<std::string> messages;
    for (const std::string_view path :
         {"", "bin/a\x01", "bin/a:b", "/bin/a", "bin//a", "other/a", "bin", "bin/a.", "bin/con",
          "bin/a~1", "share/katana/install.json"}) {
        const core::Status status = update::checkManifestPath(path);
        ASSERT_FALSE(status.ok()) << path;
        messages.push_back(status.error().message);
    }
    std::vector<std::string> sorted = messages;
    std::sort(sorted.begin(), sorted.end());
    EXPECT_EQ(std::adjacent_find(sorted.begin(), sorted.end()), sorted.end());
}

// ---- checkManifestPaths: the set ---------------------------------------------------------------------

TEST(UpdatePathSet, AnEmptyListIsFine)
{
    EXPECT_TRUE(checkSet({}, {}).ok());
}

TEST(UpdatePathSet, ADistinctListOfGoodPathsPasses)
{
    EXPECT_TRUE(checkSet({"bin/a.dll", "bin/b.dll", "share/katana/x"}, {1, 2, 3}).ok());
}

TEST(UpdatePathSet, OneSizeIsNeededForEachPath)
{
    const core::Status status = checkSet({"bin/a.dll", "bin/b.dll"}, {1});
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_FALSE(checkSet({"bin/a.dll"}, {1, 2}).ok());
    EXPECT_FALSE(checkSet({}, {1}).ok());
}

TEST(UpdatePathSet, ABadPathAnywhereInTheListFailsTheWholeAndIsNamed)
{
    const core::Status status =
        checkSet({"bin/a.dll", "bin/b.dll", "bin/c:d.dll", "bin/e.dll"}, {1, 1, 1, 1});
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(status.error().context, "bin/c:d.dll");
    // ... also the first and the last.
    EXPECT_FALSE(checkSet({"C:/x", "bin/b.dll"}, {1, 1}).ok());
    EXPECT_FALSE(checkSet({"bin/a.dll", "bin/con"}, {1, 1}).ok());
}

TEST(UpdatePathSet, ExactlyTenThousandFilesPassAndOneMoreDoesNot)
{
    EXPECT_EQ(update::kMaxFiles, 10'000u); // the design's cap (section 1), written out here

    const std::vector<std::string> atTheCap = sequentialPaths(10'000);
    EXPECT_TRUE(checkSetOfPaths(atTheCap).ok());

    const std::vector<std::string> overTheCap = sequentialPaths(10'001);
    const core::Status status = checkSetOfPaths(overTheCap);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);

    EXPECT_TRUE(checkSetOfPaths(sequentialPaths(9'999)).ok());
}

TEST(UpdatePathSet, TheCountIsCheckedBeforeAnyPathSoAHugeListCostsNothing)
{
    // 10,001 paths that are each also wrong: the error is the count's, the cheap test.
    std::vector<std::string> bad(10'001, "C:/x");
    const core::Status status = checkSetOfPaths(bad);
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("10000"), std::string::npos) << status.error().message;
}

TEST(UpdatePathSet, ExactlyTwoGibiBytesPassAndOneByteMoreDoesNot)
{
    EXPECT_EQ(update::kMaxInstallBytes, 2 * kGiB);

    EXPECT_TRUE(checkSet({"bin/a.dll"}, {2 * kGiB}).ok());
    EXPECT_FALSE(checkSet({"bin/a.dll"}, {2 * kGiB + 1}).ok());
    // The sum, not each one.
    EXPECT_TRUE(checkSet({"bin/a.dll", "bin/b.dll"}, {kGiB, kGiB}).ok());
    EXPECT_FALSE(checkSet({"bin/a.dll", "bin/b.dll"}, {kGiB, kGiB + 1}).ok());
    EXPECT_FALSE(checkSet({"bin/a.dll", "bin/b.dll", "bin/c.dll"}, {kGiB, kGiB, 1}).ok());
    EXPECT_TRUE(checkSet({"bin/a.dll", "bin/b.dll", "bin/c.dll"}, {kGiB, kGiB - 1, 1}).ok());
    // Zero-byte files add nothing.
    EXPECT_TRUE(checkSet({"bin/a.dll", "bin/b.dll"}, {2 * kGiB, 0}).ok());
}

TEST(UpdatePathSet, SizesThatWouldWrapWhenAddedAreRefusedNotSummedToSomethingSmall)
{
    const std::uint64_t max = std::numeric_limits<std::uint64_t>::max();
    // 2^64 - 1 plus 2 wraps to 1, which a naive running total would accept as tiny.
    EXPECT_FALSE(checkSet({"bin/a.dll", "bin/b.dll"}, {max, 2}).ok());
    EXPECT_FALSE(checkSet({"bin/a.dll", "bin/b.dll"}, {2, max}).ok());
    EXPECT_FALSE(checkSet({"bin/a.dll"}, {max}).ok());
    // Two halves of 2^63 add up to exactly 2^64, which is 0 when wrapped.
    const std::uint64_t half = std::uint64_t{1} << 63;
    EXPECT_FALSE(checkSet({"bin/a.dll", "bin/b.dll"}, {half, half}).ok());
    // Many sizes, each under the cap, whose sum wraps past 2^64 on the way.
    EXPECT_FALSE(checkSet({"bin/a.dll", "bin/b.dll", "bin/c.dll"}, {2 * kGiB, max, 2}).ok());
}

TEST(UpdatePathSet, TwoPathsThatAreOneNameOnceAsciiCaseIsFoldedAreRefused)
{
    for (const auto& pair : std::vector<std::pair<std::string, std::string>>{
             {"bin/Foo.dll", "bin/foo.dll"},
             {"bin/FOO.DLL", "bin/foo.dll"},
             {"bin/a.dll", "bin/A.dll"},
             {"share/Katana/x", "share/katana/x"},
             {"share/katana/x", "share/katana/X"},
             {"bin/a_b-c.d", "bin/A_B-C.D"}}) {
        const core::Status status = checkSetOfPaths({pair.first, pair.second});
        ASSERT_FALSE(status.ok()) << pair.first << " and " << pair.second;
        EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
        // ... in either order, and with other names between them.
        EXPECT_FALSE(checkSetOfPaths({pair.second, pair.first}).ok());
        EXPECT_FALSE(checkSetOfPaths({pair.first, "bin/zzz1.dll", "bin/zzz2.dll", pair.second}).ok());
    }
    // The very same path twice is the same name too.
    EXPECT_FALSE(checkSetOfPaths({"bin/a.dll", "bin/a.dll"}).ok());
}

TEST(UpdatePathSet, OnlyTheLettersAToZFoldSoPunctuationThatDiffersByOneBitIsNotADuplicate)
{
    // '[' (0x5B) and '{' (0x7B), '@' (0x40) and '`' (0x60), '\\' excluded by another rule:
    // they differ by 0x20 as 'A' and 'a' do, but they are not letters and have no other case.
    EXPECT_TRUE(checkSetOfPaths({"share/a[b", "share/a{b"}).ok());
    EXPECT_TRUE(checkSetOfPaths({"share/@x", "share/`x"}).ok());
    EXPECT_TRUE(checkSetOfPaths({"share/a^b", "share/a~b"}).ok());   // 0x5E and 0x7E
    // Different letters and digits stay different.
    EXPECT_TRUE(checkSetOfPaths({"bin/a1.dll", "bin/a2.dll", "bin/b1.dll"}).ok());
}

TEST(UpdatePathSet, ANameThatIsAFileAndAlsoTheFolderOfAnotherIsRefused)
{
    EXPECT_FALSE(checkSetOfPaths({"share/x", "share/x/y"}).ok());
    EXPECT_FALSE(checkSetOfPaths({"share/x/y", "share/x"}).ok());
    EXPECT_FALSE(checkSetOfPaths({"share/x", "share/x/y/z"}).ok());
    EXPECT_FALSE(checkSetOfPaths({"share/x/y", "share/x/y/z.txt"}).ok());
    // ... after case folding, as the duplicate rule is.
    EXPECT_FALSE(checkSetOfPaths({"share/X", "share/x/y"}).ok());
    EXPECT_FALSE(checkSetOfPaths({"share/x", "share/X/y"}).ok());
    // A shared PREFIX of characters is not a shared folder.
    EXPECT_TRUE(checkSetOfPaths({"share/x", "share/xy/z"}).ok());
    EXPECT_TRUE(checkSetOfPaths({"share/x", "share/x.d/y"}).ok());
    EXPECT_TRUE(checkSetOfPaths({"share/katana/readme", "share/katana/readme.txt"}).ok());
    // Siblings in one folder are fine.
    EXPECT_TRUE(checkSetOfPaths({"share/x/y", "share/x/z"}).ok());
}

TEST(UpdatePathSet, AFolderIsFoundWhereverTheByteOrderPutsItsFilesAndNamesBetweenAreNotInTheWay)
{
    // In byte order '-' (0x2D) and '.' (0x2E) sort BEFORE '/' (0x2F), so "bin/a-b" and
    // "bin/a.d" sit between "bin/a" and "bin/a/c": the file and the file below it are not
    // neighbours in a sorted list, and a check of neighbours only would miss the pair.
    EXPECT_FALSE(checkSetOfPaths({"bin/a", "bin/a-b", "bin/a.d", "bin/a/c"}).ok());
    EXPECT_FALSE(checkSetOfPaths({"bin/a/c", "bin/a.d", "bin/a-b", "bin/a"}).ok());
    EXPECT_FALSE(checkSetOfPaths({"share/x/y", "share/x/y-z", "share/x/y.w", "share/x/y/w"}).ok());
    // '0' (0x30) and 'a' (0x61) sort after '/', so these come after the folder's files.
    EXPECT_FALSE(checkSetOfPaths({"bin/a", "bin/a0", "bin/aa", "bin/a/c", "bin/b"}).ok());
    // The same names without a file below "bin/a": only neighbours by spelling, no folder.
    EXPECT_TRUE(checkSetOfPaths({"bin/a", "bin/a-b", "bin/a.d", "bin/a0", "bin/aa", "bin/a_", "bin/b/a"}).ok());
    // A file below a folder that is not itself listed is an ordinary tree.
    EXPECT_TRUE(checkSetOfPaths({"bin/a/b", "bin/a/c/d", "bin/a-b/c", "bin/a.d/c"}).ok());
}

TEST(UpdatePathSet, TheFileInsideAFileIsNamedByTheInnerPathAsWritten)
{
    // "bin/A/C" is inside "bin/a" once case is folded; the error shows it as the manifest wrote it.
    const core::Status status = checkSetOfPaths({"bin/a", "bin/A-b", "bin/A/C"});
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(status.error().context, "bin/A/C");
}

TEST(UpdatePathSet, OnePathOfMegabytesIsCheckedInTimeProportionalToItsLength)
{
    // "bin/a/a/a/..." of 2 MiB has about a million slashes. A signed manifest may hold such a
    // path (the cap on the manifest is 8 MiB), and a check that looked at every prefix of it
    // against the listed names would copy and hash a million prefixes of a mean half a MiB:
    // 10^12 bytes of work, in the helper, with the lock held. The bound is generous: a pass
    // takes milliseconds.
    std::string longPath = "bin";
    while (longPath.size() < 2 * 1024 * 1024) {
        longPath += "/a";
    }
    ASSERT_TRUE(accepts(longPath));

    const auto begin = std::chrono::steady_clock::now();
    EXPECT_FALSE(checkSetOfPaths({longPath, "bin/b", "share/x", longPath + "/x", longPath + "/y"}).ok())
        << "longPath is a file and the folder of longPath/x";
    EXPECT_TRUE(checkSetOfPaths({longPath, "bin/b", "share/x"}).ok());
    EXPECT_TRUE(checkSetOfPaths({longPath + "/x", longPath + "/y", "bin/a/b"}).ok());
    // "bin/a/a" is a file and the folder of longPath: found at the shallowest slash.
    EXPECT_FALSE(checkSetOfPaths({longPath, "bin/a/a"}).ok());
    EXPECT_FALSE(checkSetOfPaths({"bin/a/a", longPath}).ok());
    // Two of them, equal but for case in the middle: a duplicate once folded.
    std::string shouting = longPath;
    shouting[(shouting.size() / 2) & ~std::size_t{1}] = 'A'; // an even index holds an 'a'
    ASSERT_NE(shouting, longPath);
    ASSERT_TRUE(accepts(shouting));
    EXPECT_FALSE(checkSetOfPaths({longPath, shouting}).ok());
    const auto elapsed = std::chrono::steady_clock::now() - begin;
    EXPECT_LT(elapsed, std::chrono::seconds(10));
}

// ---- checkJournalPath ---------------------------------------------------------------------------------------

TEST(UpdateJournalPaths, EveryPathAManifestMayHoldIsAJournalPath)
{
    for (const std::string_view path : {"bin/katana.exe", "share/katana/readme.txt",
                                        "etc/katana/settings.ini", "lib/libx.so.1.2",
                                        "share/katana/installed.json.bak"}) {
        EXPECT_TRUE(update::checkJournalPath(path).ok()) << path;
    }
}

TEST(UpdateJournalPaths, TheHelpersOwnThreeRecordsAreJournalPathsAlthoughNoManifestMayHoldThem)
{
    ASSERT_EQ(update::kRecordPaths.size(), 3u);
    for (const std::string_view record : update::kRecordPaths) {
        EXPECT_TRUE(update::isReservedName(record)) << record;
        EXPECT_FALSE(update::checkManifestPath(record).ok()) << record;
        EXPECT_TRUE(update::checkJournalPath(record).ok()) << record;
    }
    // Spelled out, as the header lists them.
    EXPECT_TRUE(update::checkJournalPath("share/katana/installed.json").ok());
    EXPECT_TRUE(update::checkJournalPath("share/katana/installed.manifest.json").ok());
    EXPECT_TRUE(update::checkJournalPath("share/katana/installed.manifest.json.sig").ok());
}

TEST(UpdateJournalPaths, NoOtherReservedNameIsAJournalPath)
{
    // install.json is written by the installer, never by the helper, so a journal that
    // names it is forged; so is the work folder and every other spelling.
    for (const std::string_view path :
         {"share/katana/install.json", "bin/installed.json", "etc/installed.manifest.json",
          "lib/installed.manifest.json.sig", ".update/lock", "share/.update/x", "bin/.update",
          "R/.update/tx/journal"}) {
        EXPECT_FALSE(update::checkJournalPath(path).ok()) << path;
    }
}

TEST(UpdateJournalPaths, TheThreeRecordsAreAcceptedExactlyAsTheHelperWritesThemAndInNoOtherCase)
{
    // On a case-sensitive file system Share/Katana/... is another place, outside bin, share,
    // etc and lib, and the journal is not authenticated (journal.hpp): a forged line must not
    // get there, so the exception for the records is by exact spelling.
    for (const std::string_view path :
         {"Share/katana/installed.json", "share/Katana/installed.json",
          "share/katana/INSTALLED.JSON", "SHARE/KATANA/INSTALLED.MANIFEST.JSON",
          "share/katana/installed.manifest.json.SIG"}) {
        EXPECT_FALSE(update::checkJournalPath(path).ok()) << path;
    }
}

TEST(UpdateJournalPaths, ARecordNameWithAnythingAddedOrAroundItIsRefused)
{
    for (const std::string_view path :
         {"share/katana/installed.json ", "share/katana/installed.json.", "share/katana/installed.json/",
          "/share/katana/installed.json", "share/katana//installed.json",
          "share/katana/./installed.json", "share/katana/../katana/installed.json",
          "share\\katana\\installed.json", "share/katana/installed.json:stream",
          " share/katana/installed.json", "share/katana/installed.json\r"}) {
        EXPECT_FALSE(update::checkJournalPath(path).ok()) << path;
    }
    EXPECT_FALSE(update::checkJournalPath(std::string("share/katana/installed.json\0", 28)).ok());
    EXPECT_FALSE(update::checkJournalPath(std::string("share/katana/installed.json\0x", 29)).ok());
}

TEST(UpdateJournalPaths, AJournalPathCannotLeaveTheInstallFolders)
{
    // Journal paths are untrusted: the manifest rules hold, so no path can name a place
    // outside R's bin, share, etc and lib (the file system's own reparse-point test is the
    // apply code's).
    for (const std::string_view path :
         {"", "/", "/etc/passwd", "C:/Windows/system32/x.dll", "..", "../x", "bin/../../x",
          "bin/../../../etc/passwd", "bin\\..\\..\\x", "\\\\?\\C:\\x", "\\\\.\\pipe\\x",
          "other/x", "LICENSE", "bin", "bin/", "share/con", "bin/a:b", "bin/PROGRA~1"}) {
        EXPECT_FALSE(update::checkJournalPath(path).ok()) << "'" << path << "'";
    }
}

TEST(UpdateJournalDirectories, ABareInstallFolderIsADirectoryTheJournalMayNameButNeverAFile)
{
    // A release that first brings etc or lib makes the folder and journals MKDIR for it; the
    // rollback must be able to remove it. The same bare name in an ADD, REPLACE or REMOVE record
    // would let a forged line delete the whole folder, so checkJournalPath keeps refusing it.
    for (const std::string_view root : update::kInstallRoots) {
        EXPECT_TRUE(update::checkJournalDirectory(root).ok()) << root;
        EXPECT_FALSE(update::checkJournalPath(root).ok()) << root;
        EXPECT_FALSE(update::checkManifestPath(root).ok()) << root;
    }
    ASSERT_EQ(update::kInstallRoots.size(), 4u);
    for (const std::string_view root : {"bin", "share", "etc", "lib"}) {
        EXPECT_TRUE(update::checkJournalDirectory(root).ok()) << root;
    }
}

TEST(UpdateJournalDirectories, AFolderBelowAnInstallFolderPassesAsAFilesPathWould)
{
    for (const std::string_view path : {"share/katana", "etc/katana/profiles", "bin/platforms", "lib/x", "share/proj",
                                        "share/katana/installed.json.bak"}) {
        EXPECT_TRUE(update::checkJournalDirectory(path).ok()) << path;
    }
}

TEST(UpdateJournalDirectories, OnlyTheFourExactNamesAreBareFoldersAndNothingAroundThemIs)
{
    for (const std::string_view path : {"", "Bin", "SHARE", "Etc", "LIB", "bin/", "/bin", "bin//", "bin/.", "bin/..",
                                        "other", "usr", "binx", "bin ", "bin.", "bin\\", "bin:", "./bin", "../bin",
                                        "C:/bin", "bin/bin/..", "share/katana/../.."}) {
        EXPECT_FALSE(update::checkJournalDirectory(path).ok()) << "'" << path << "'";
    }
    EXPECT_FALSE(update::checkJournalDirectory(std::string("bin\0", 4)).ok());
    EXPECT_FALSE(update::checkJournalDirectory(std::string("etc\0lib", 7)).ok());
}

TEST(UpdateJournalDirectories, ADirectoryIsHeldToTheManifestRulesAndNoRecordIsOne)
{
    // The three records are files; a MKDIR of one is as forged as any other line.
    for (const std::string_view record : update::kRecordPaths) {
        EXPECT_FALSE(update::checkJournalDirectory(record).ok()) << record;
    }
    for (const std::string_view path :
         {"bin/../x", "bin/con", "bin/a~1", "share/.update", ".update", "share/katana/install.json",
          "share/katana/INSTALLED.JSON", "bin/a.", "bin\\a", "share/katana/a:b", "other/x", "bin/a\x01"}) {
        EXPECT_FALSE(update::checkJournalDirectory(path).ok()) << path;
    }
}

// ---- the real tree ---------------------------------------------------------------------------------------------

TEST(UpdatePaths, EveryFileOfTheFixtureInstallTreesPassesExceptTheInstallersOwnRecord)
{
    // The plain ASCII names of a real tree pass: these are the trees the producer's fixtures are
    // made from (tests/update/fixtures/install/v1 and v2), a stub of the shipped layout.
    int seen = 0;
    for (const char* version : {"v1", "v2"}) {
        const fs::path root = fs::path(KATANA_UPDATE_FIXTURES) / "install" / version;
        ASSERT_TRUE(fs::is_directory(root)) << root.string();
        for (const fs::directory_entry& entry : fs::recursive_directory_iterator(root)) {
            if (!entry.is_regular_file()) {
                continue;
            }
            const std::string relative = fs::relative(entry.path(), root).generic_string();
            ++seen;
            if (relative == "share/katana/install.json") {
                // The CPack install writes it into the package; no manifest holds it.
                EXPECT_FALSE(accepts(relative)) << relative;
            } else {
                expectAccepted(relative);
            }
        }
    }
    EXPECT_GE(seen, 20);
}
