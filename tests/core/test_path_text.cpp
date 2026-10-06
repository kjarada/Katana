// A path as the text a person typed, and a file as the bytes it holds
// (core/path_text.hpp).

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <string>
#include <system_error>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/core/path_text.hpp"

namespace fs = std::filesystem;
using katana::core::ErrorCode;
using katana::core::pathFromUtf8;
using katana::core::pathToUtf8;
using katana::core::readFileBytes;

namespace {

// "Zürich" in UTF-8: the u-umlaut is the two bytes C3 BC.
const std::string kZurich = "Z\xC3\xBCrich";

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

// A fresh directory of one test's own, in one process's own, removed when the
// test ends.
struct Scratch {
    fs::path root;

    explicit Scratch(const char* name)
        : root(fs::temp_directory_path() / ("katana-core-tests-path-text-" + processTag()) / name)
    {
        fs::remove_all(root);
        fs::create_directories(root);
    }
    ~Scratch()
    {
        std::error_code ignored;
        fs::remove_all(root.parent_path(), ignored);
    }
};

} // namespace

TEST(PathText, Utf8TextNamesThePathItSpellsAndIsSaidBackAsTheSameText)
{
    const fs::path path = pathFromUtf8(kZurich + "/site plan.json");
    // One code point for the umlaut, whatever the platform's own encoding:
    // "Zürich" is six characters, not the seven its UTF-8 bytes are.
    EXPECT_EQ(path.parent_path().u32string().size(), 6u);
    EXPECT_TRUE(path.filename().u32string() == U"site plan.json");
    EXPECT_EQ(pathToUtf8(path.filename()), "site plan.json");
    EXPECT_EQ(pathToUtf8(path.parent_path()), kZurich);
    EXPECT_EQ(pathToUtf8(pathFromUtf8("plain/ascii.txt").filename()), "ascii.txt");
    EXPECT_TRUE(pathFromUtf8("").empty());
}

TEST(PathText, BytesThatAreNotUtf8AreTakenAsTheNarrowNameTheyAreRatherThanThrownOn)
{
    // "café" as an ANSI editor saves it: E9 alone is not UTF-8. Converting it
    // as UTF-8 throws - std::filesystem::path does, handed these bytes on
    // Windows - and a command line may hand over exactly this.
    const std::string ansi = "caf\xE9.txt";
    fs::path path;
    ASSERT_NO_THROW(path = pathFromUtf8(ansi));
    // Whatever character the platform's code page makes of E9, the ASCII
    // either side of it is untouched.
    const fs::path::string_type& native = path.native();
    ASSERT_GE(native.size(), 7u);
    const std::string head = "caf";
    const std::string tail = "txt";
    for (std::size_t i = 0; i < head.size(); ++i) {
        EXPECT_EQ(native[i], static_cast<fs::path::value_type>(head[i])) << i;
        EXPECT_EQ(native[native.size() - tail.size() + i],
                  static_cast<fs::path::value_type>(tail[i]))
            << i;
    }

    // And it is THE file the C runtime opens under that narrow name: one made
    // through a narrow open is the one read back through the path. In the
    // scratch directory by a relative name, so that nothing but the name
    // under test is narrow.
    const Scratch scratch("narrow-name");
    const fs::path before = fs::current_path();
    fs::current_path(scratch.root);
    bool made = false;
    {
        std::ofstream out(ansi, std::ios::binary);
        made = static_cast<bool>(out);
        out << "narrow";
    }
    const auto read = readFileBytes(pathFromUtf8(ansi));
    // Restored before anything can end the test: the scratch directory is
    // removed when it does.
    fs::current_path(before);
    if (!made) {
        // A file system that takes only UTF-8 names (APFS does) refuses this
        // one, so there is no file for the path to be the name of. That the
        // conversion does not throw is checked above, on every system.
        GTEST_SKIP() << "this system makes no file of that narrow name";
    }
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(*read, "narrow");
}

TEST(PathText, AFileIsReadAsEveryByteItHolds)
{
    const Scratch scratch("every-byte");
    // A carriage return, a NUL and a byte above 127: nothing is translated,
    // nothing ends the read early.
    const std::string bytes("a\r\nb\0c\xFF", 7);
    const fs::path path = scratch.root / pathFromUtf8(kZurich + ".bin");
    std::ofstream(path, std::ios::binary) << bytes;

    const auto read = readFileBytes(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(*read, bytes);
    EXPECT_EQ(read->size(), 7u);

    // An empty file is read, as nothing.
    const fs::path empty = scratch.root / "empty.bin";
    std::ofstream(empty, std::ios::binary).flush();
    const auto nothing = readFileBytes(empty);
    ASSERT_TRUE(nothing.ok()) << nothing.error().describe();
    EXPECT_TRUE(nothing->empty());
}

TEST(PathText, AFileThatIsNotThereAndADirectoryAreNotFoundNamingThePath)
{
    const Scratch scratch("not-found");
    const fs::path missing = scratch.root / "nowhere.bin";
    const auto absent = readFileBytes(missing);
    ASSERT_FALSE(absent.ok());
    EXPECT_EQ(absent.error().code, ErrorCode::NotFound);
    EXPECT_EQ(absent.error().context, pathToUtf8(missing));

    // A directory is not a file of no bytes.
    const auto directory = readFileBytes(scratch.root);
    ASSERT_FALSE(directory.ok());
    EXPECT_EQ(directory.error().code, ErrorCode::NotFound);
    EXPECT_EQ(directory.error().context, pathToUtf8(scratch.root));
}
