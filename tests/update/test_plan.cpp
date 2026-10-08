// The planner (include/katana/update/plan.hpp): which files of a release differ from the
// installed ones, found by hashing the installed files now.
//
// The trees are the install fixtures of tests/update/fixtures (v1 and v2, built by
// tools/update_make_fixtures.py) copied to a temporary root, and the manifests are the
// fixture releases' own, written by the producer (tools/update_make_manifest.py) and read
// here a line a file. The hashes in them are Python's hashlib's; the expectations - which
// files change, how many bytes - are worked out by hand from the two trees:
//
//   v1 -> v2   bin/katana.exe changes (207 bytes, 85 compressed, at 44 in the payload)
//              bin/katana_cli.exe, bin/libcore-1.dll, share/katana/empty.txt and random.bin
//                are the same bytes
//              bin/libnew-2.dll (118, 70, at 418), share/katana/help/index.txt (82, 75, at 599)
//                and share/katana/news.txt (54, 54, at 725) are new
//              share/katana/readme.txt changes (55, 55, at 2938)
//              etc/katana/settings.ini is a seed: it differs, and is kept, because it is there
//              bin/libold-1.dll and share/katana/legacy/old.txt are v1's and not in v2: not the
//                planner's business
//   so 5 files change, 207 + 118 + 82 + 54 + 55 = 516 bytes, 85 + 70 + 75 + 54 + 55 = 339
//   of them to download, and 2.6 * 516 = 1341.6 -> 1342 bytes of free space are needed.

#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <regex>
#include <stop_token>
#include <string>
#include <thread>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/sha256.hpp"
#include "katana/update/paths.hpp"
#include "katana/update/plan.hpp"

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#endif

using namespace katana;
namespace fs = std::filesystem;

namespace {

constexpr std::uint64_t kMiB = 1024 * 1024;

std::string readFile(const fs::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

void writeFile(const fs::path& path, const std::string& bytes)
{
    fs::create_directories(path.parent_path());
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
}

// A folder under the system temp folder, removed when it goes out of scope.
class TempDir {
  public:
    TempDir()
    {
        static std::atomic<int> counter{0};
        path_ = fs::temp_directory_path() /
                ("katana_t6_plan_" +
                 std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) + "_" +
                 std::to_string(counter++));
        fs::create_directories(path_);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;
    ~TempDir()
    {
        std::error_code ignored;
        fs::remove_all(path_, ignored);
    }
    const fs::path& path() const { return path_; }

  private:
    fs::path path_;
};

// A directory link (a junction on Windows - it needs no privilege - a symlink elsewhere) that
// is taken away again, without touching what it points at, when it goes out of scope.
class DirectoryLink {
  public:
    DirectoryLink(const fs::path& link, const fs::path& target) : link_(link)
    {
#ifdef _WIN32
        // `cmd /c "..."`: the outer quotes are removed by cmd, the inner ones stay.
        const std::string command =
            "\"mklink /J \"" + link.string() + "\" \"" + target.string() + "\" >NUL 2>&1\"";
        made_ = std::system(command.c_str()) == 0;
#else
        std::error_code error;
        fs::create_directory_symlink(target, link, error);
        made_ = !error;
#endif
    }
    DirectoryLink(const DirectoryLink&) = delete;
    DirectoryLink& operator=(const DirectoryLink&) = delete;
    ~DirectoryLink()
    {
        if (made_) {
            std::error_code ignored;
            fs::remove(link_, ignored); // the link itself: remove(), not remove_all()
        }
    }
    bool made() const { return made_; }

  private:
    fs::path link_;
    bool made_ = false;
};

// ---- the fixtures -------------------------------------------------------------------------

fs::path fixtureTree(const std::string& version)
{
    return fs::path(KATANA_UPDATE_FIXTURES) / "install" / version;
}

// A copy of an install fixture at `root`.
void copyTree(const std::string& version, const fs::path& root)
{
    fs::copy(fixtureTree(version), root, fs::copy_options::recursive);
}

update::FileClass classOf(const std::string& name)
{
    if (name == "program") {
        return update::FileClass::Program;
    }
    if (name == "lib") {
        return update::FileClass::Lib;
    }
    if (name == "seed") {
        return update::FileClass::Seed;
    }
    return update::FileClass::Data;
}

// The manifest of a fixture release, from the lines the producer wrote.
update::Manifest manifestOf(const std::string& release)
{
    const fs::path file =
        fs::path(KATANA_UPDATE_FIXTURES) / "release" / release / "Katana-update-win64.json";
    const std::string text = readFile(file);
    static const std::regex line(
        R"re("class":"([a-z]+)","csize":([0-9]+),(?:"exec":true,)?"offset":([0-9]+),)re"
        R"re("path":"([^"]+)","sha256":"([0-9a-f]{64})","size":([0-9]+))re");
    update::Manifest manifest;
    for (std::sregex_iterator it(text.begin(), text.end(), line), end; it != end; ++it) {
        update::ManifestFile entry;
        entry.fileClass = classOf((*it)[1].str());
        entry.compressedSize = std::stoull((*it)[2].str());
        entry.offset = std::stoull((*it)[3].str());
        entry.path = (*it)[4].str();
        entry.sha256 = (*it)[5].str();
        entry.size = std::stoull((*it)[6].str());
        manifest.files.push_back(entry);
    }
    return manifest;
}

update::Install installAt(const fs::path& root)
{
    update::Install install;
    install.root = root;
    return install;
}

core::Result<update::Plan> planOf(const update::Manifest& manifest, const fs::path& root,
                                  std::stop_token stop = {})
{
    return update::plan(manifest, installAt(root), stop);
}

// The indexes the plan marks as changed.
std::vector<std::size_t> changedIndexes(const update::Plan& plan)
{
    std::vector<std::size_t> out;
    for (const update::PlannedFile& file : plan.files) {
        if (file.changed) {
            out.push_back(file.index);
        }
    }
    return out;
}

using Indexes = std::vector<std::size_t>;

// Every file under `root`: relative path (with '/') -> SHA-256. Not for trees holding links.
std::map<std::string, std::string> snapshot(const fs::path& root)
{
    std::map<std::string, std::string> out;
    for (const fs::directory_entry& entry : fs::recursive_directory_iterator(root)) {
        if (entry.is_regular_file()) {
            const auto hash = core::sha256FileHex(entry.path());
            out[entry.path().lexically_relative(root).generic_string()] =
                hash.ok() ? *hash : "unreadable";
        }
    }
    return out;
}

// A manifest of absent files with the given sizes, for the arithmetic.
update::Manifest absentFiles(const std::vector<std::uint64_t>& sizes)
{
    update::Manifest manifest;
    for (std::size_t i = 0; i < sizes.size(); ++i) {
        update::ManifestFile entry;
        entry.path = "bin/f" + std::to_string(i) + ".dll";
        entry.fileClass = update::FileClass::Lib;
        entry.size = sizes[i];
        entry.compressedSize = 2;
        entry.offset = 100 + 10 * i;
        entry.sha256 = std::string(64, 'a');
        manifest.files.push_back(entry);
    }
    return manifest;
}

} // namespace

// ---- nothing, one, many -------------------------------------------------------------------

TEST(Plan, AnInstallEqualToTheManifestChangesNothing)
{
    TempDir temp;
    copyTree("v1", temp.path());
    const auto plan = planOf(manifestOf("v1"), temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(plan->totalFiles, 9u);
    EXPECT_EQ(plan->changedFiles, 0u);
    EXPECT_EQ(plan->changedBytes, 0u);
    EXPECT_EQ(plan->downloadBytes, 0u);
    EXPECT_EQ(plan->requiredFreeBytes, 0u);
    EXPECT_TRUE(plan->ranges.empty());
    ASSERT_EQ(plan->files.size(), 9u) << "one entry per manifest file";
    for (std::size_t i = 0; i < plan->files.size(); ++i) {
        EXPECT_EQ(plan->files[i].index, i) << "in manifest order";
        EXPECT_FALSE(plan->files[i].changed) << i;
    }
}

TEST(Plan, TheNextReleaseOverTheInstalledOneChangesExactlyTheFilesThatDifferOrAreAbsent)
{
    TempDir temp;
    copyTree("v1", temp.path());
    const update::Manifest v2 = manifestOf("v2");
    ASSERT_EQ(v2.files.size(), 10u);
    const auto plan = planOf(v2, temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();

    // Manifest order: 0 bin/katana.exe, 1 bin/katana_cli.exe, 2 bin/libcore-1.dll,
    // 3 bin/libnew-2.dll, 4 etc/katana/settings.ini, 5 share/katana/empty.txt,
    // 6 share/katana/help/index.txt, 7 share/katana/news.txt, 8 share/katana/random.bin,
    // 9 share/katana/readme.txt.
    EXPECT_EQ(changedIndexes(*plan), (Indexes{0, 3, 6, 7, 9}));
    EXPECT_EQ(plan->totalFiles, 10u);
    EXPECT_EQ(plan->changedFiles, 5u);
    EXPECT_EQ(plan->changedBytes, 516u);
    EXPECT_EQ(plan->downloadBytes, 339u) << "compressed sizes: what is fetched";
    EXPECT_EQ(plan->requiredFreeBytes, 1342u);

    // The payload bytes to fetch: the five members lie within 2.1 KB of each other, so one request
    // from the first (44) to the end of the last (2938 + 55 = 2993).
    ASSERT_EQ(plan->ranges.size(), 1u);
    EXPECT_EQ(plan->ranges[0].offset, 44u);
    EXPECT_EQ(plan->ranges[0].length, 2949u);
    ASSERT_EQ(plan->ranges[0].members.size(), 5u);
    const struct {
        std::size_t index;
        std::uint64_t offset;
        std::uint64_t length;
    } expected[] = {{0, 44, 85}, {3, 418, 70}, {6, 599, 75}, {7, 725, 54}, {9, 2938, 55}};
    for (std::size_t i = 0; i < 5; ++i) {
        EXPECT_EQ(plan->ranges[0].members[i].fileIndex, expected[i].index) << i;
        EXPECT_EQ(plan->ranges[0].members[i].offset, expected[i].offset) << i;
        EXPECT_EQ(plan->ranges[0].members[i].length, expected[i].length) << i;
    }
}

TEST(Plan, ADownloadIsCountedInCompressedBytesAndTheDiskNeedInInflatedOnes)
{
    // random.bin cannot be compressed: 2048 bytes in 2053 (a stored block and its header).
    TempDir temp;
    copyTree("v1", temp.path());
    fs::remove(temp.path() / "share" / "katana" / "random.bin");
    const auto plan = planOf(manifestOf("v1"), temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(changedIndexes(*plan), (Indexes{7}));
    EXPECT_EQ(plan->changedBytes, 2048u);
    EXPECT_EQ(plan->downloadBytes, 2053u);
    EXPECT_EQ(plan->requiredFreeBytes, 5325u) << "2.6 * 2048 = 5324.8, rounded up";
}

TEST(Plan, FilesThatAreNotInTheTreeAtAllAreAllChanged)
{
    TempDir temp; // an empty root: a first install over nothing
    const auto plan = planOf(manifestOf("v2"), temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(plan->changedFiles, 10u);
    EXPECT_EQ(plan->changedBytes, 207u + 144u + 208u + 118u + 59u + 0u + 82u + 54u + 2048u + 55u);
}

// ---- the person's files -------------------------------------------------------------------

TEST(Plan, ASeedThePersonEditedIsKeptAndOneThatIsGoneIsWrittenAgain)
{
    TempDir temp;
    copyTree("v1", temp.path());
    const fs::path seed = temp.path() / "etc" / "katana" / "settings.ini";
    // As installed (v1's bytes, which v2's manifest does not match): present, so kept.
    auto plan = planOf(manifestOf("v2"), temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_FALSE(plan->files[4].changed);
    // Edited into something else entirely, or emptied: still the person's.
    writeFile(seed, "# my settings\nunits = feet\n");
    plan = planOf(manifestOf("v2"), temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_FALSE(plan->files[4].changed);
    writeFile(seed, "");
    plan = planOf(manifestOf("v2"), temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_FALSE(plan->files[4].changed);
    // Deleted: written (59 bytes in the release).
    fs::remove(seed);
    plan = planOf(manifestOf("v2"), temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_TRUE(plan->files[4].changed);
    EXPECT_EQ(plan->changedFiles, 6u);
    EXPECT_EQ(plan->changedBytes, 516u + 59u);
}

TEST(Plan, AnEditedFileThatIsNotASeedIsChangedBackToTheReleasesBytes)
{
    TempDir temp;
    copyTree("v1", temp.path());
    writeFile(temp.path() / "share" / "katana" / "readme.txt", "edited by hand\n");
    const auto plan = planOf(manifestOf("v1"), temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(changedIndexes(*plan), (Indexes{8}));
    EXPECT_EQ(plan->changedBytes, 52u);
    EXPECT_EQ(plan->requiredFreeBytes, 136u) << "2.6 * 52 = 135.2";
}

TEST(Plan, FilesTheManifestDoesNotListAreNeverTouchedNorMentioned)
{
    TempDir temp;
    copyTree("v1", temp.path());
    // The person's own: next to the program, in a folder of the release's, and in a folder of their
    // own.
    writeFile(temp.path() / "bin" / "my_plugin.dll", "plugin");
    writeFile(temp.path() / "share" / "katana" / "my_notes.txt", "notes");
    writeFile(temp.path() / "share" / "katana" / "mine" / "deep.txt", "deep");
    writeFile(temp.path() / "etc" / "katana" / "local.ini", "local");
    const auto before = snapshot(temp.path());
    ASSERT_EQ(before.size(), 10u + 4u); // v1's ten files (install.json too) and the four above

    const auto plan = planOf(manifestOf("v2"), temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(plan->files.size(), 10u) << "only the manifest's files are in the plan";
    EXPECT_EQ(snapshot(temp.path()), before) << "planning writes, removes and renames nothing";
    // v1's own files that v2 drops are still there too: removing them is the helper's decision.
    EXPECT_TRUE(fs::exists(temp.path() / "bin" / "libold-1.dll"));
    EXPECT_TRUE(fs::exists(temp.path() / "share" / "katana" / "legacy" / "old.txt"));
}

// ---- the hash is taken now ----------------------------------------------------------------

TEST(Plan, EveryKindOfDifferenceMakesAFileChanged)
{
    const std::string original = readFile(fixtureTree("v1") / "bin" / "libcore-1.dll");
    ASSERT_EQ(original.size(), 208u);
    std::vector<std::pair<const char*, std::string>> variants;
    variants.push_back({"one byte flipped", [&] {
                            std::string s = original;
                            s[100] = static_cast<char>(s[100] ^ 1);
                            return s;
                        }()});
    variants.push_back({"the last byte flipped", [&] {
                            std::string s = original;
                            s.back() = static_cast<char>(s.back() ^ 0x80);
                            return s;
                        }()});
    variants.push_back({"one byte missing", original.substr(0, original.size() - 1)});
    variants.push_back({"one byte more", original + "x"});
    variants.push_back({"empty", ""});
    variants.push_back({"the same length of something else", std::string(original.size(), 'z')});
    for (const auto& [what, bytes] : variants) {
        TempDir temp;
        copyTree("v1", temp.path());
        writeFile(temp.path() / "bin" / "libcore-1.dll", bytes);
        const auto plan = planOf(manifestOf("v1"), temp.path());
        ASSERT_TRUE(plan.ok()) << what << ": " << plan.error().describe();
        EXPECT_EQ(changedIndexes(*plan), (Indexes{2})) << what;
    }
}

TEST(Plan, ThePlanIsNeverFromACacheSoAChangeThatKeepsTheSizeAndTheTimeIsSeen)
{
    TempDir temp;
    copyTree("v1", temp.path());
    const fs::path file = temp.path() / "bin" / "katana_cli.exe";
    const update::Manifest v1 = manifestOf("v1");
    auto plan = planOf(v1, temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_EQ(plan->changedFiles, 0u);

    const auto time = fs::last_write_time(file);
    std::string bytes = readFile(file);
    bytes[10] = static_cast<char>(bytes[10] ^ 0x01);
    writeFile(file, bytes);
    fs::last_write_time(file, time); // same size, same modification time, different bytes
    ASSERT_EQ(fs::file_size(file), 144u);
    ASSERT_EQ(fs::last_write_time(file), time);

    plan = planOf(v1, temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_EQ(changedIndexes(*plan), (Indexes{1}));
}

TEST(Plan, AFileLongerThanAnyBlockIsHashedWhole)
{
    // 16 MiB + 3 bytes, byte i being i % 251; SHA-256 from Python's hashlib.
    constexpr std::uint64_t size = 16 * kMiB + 3;
    TempDir temp;
    const fs::path file = temp.path() / "bin" / "big.bin";
    fs::create_directories(file.parent_path());
    {
        std::ofstream out(file, std::ios::binary);
        std::string chunk;
        std::uint64_t written = 0;
        while (written < size) {
            chunk.clear();
            for (std::uint64_t i = written; i < std::min(size, written + kMiB); ++i) {
                chunk.push_back(static_cast<char>(i % 251));
            }
            out.write(chunk.data(), static_cast<std::streamsize>(chunk.size()));
            written += chunk.size();
        }
    }
    update::Manifest manifest = absentFiles({size});
    manifest.files[0].path = "bin/big.bin";
    manifest.files[0].sha256 = "aa4b9bb6fe503123df159e5cef142033e8561b9361dd41f0ea241e15242dbfb6";
    auto plan = planOf(manifest, temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(plan->changedFiles, 0u);

    // One byte of the last block changed: seen.
    {
        std::fstream edit(file, std::ios::binary | std::ios::in | std::ios::out);
        edit.seekp(static_cast<std::streamoff>(size - 2));
        edit.put('\xff'); // never a byte of the pattern, which is 0 to 250
    }
    plan = planOf(manifest, temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_EQ(plan->changedFiles, 1u);
}

// ---- cancellation -------------------------------------------------------------------------

TEST(Plan, AStopRequestedBeforeTheStartCancelsBeforeAnyFileIsRead)
{
    TempDir temp;
    copyTree("v1", temp.path());
    std::stop_source source;
    source.request_stop();
    const auto plan = planOf(manifestOf("v1"), temp.path(), source.get_token());
    ASSERT_FALSE(plan.ok());
    EXPECT_EQ(plan.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(plan.error().message, "cancelled");
}

TEST(Plan, AStopRequestedWhileFilesAreBeingHashedCancelsBetweenFiles)
{
    // Sixteen files of 8 MiB (zeros, made sparse, 128 MiB to hash): long enough for another thread
    // to ask a moment after the start. The request is made after 20, then 5, then 0 ms - the first
    // that lands mid-way is the case; the last is certain.
    TempDir temp;
    update::Manifest manifest;
    for (int i = 0; i < 16; ++i) {
        update::ManifestFile entry;
        entry.path = "bin/part" + std::to_string(i) + ".bin";
        entry.fileClass = update::FileClass::Lib;
        entry.sha256 = std::string(64, '0');
        entry.size = 8 * kMiB;
        entry.compressedSize = 100;
        manifest.files.push_back(entry);
        const fs::path file = temp.path() / entry.path;
        fs::create_directories(file.parent_path());
        std::ofstream(file, std::ios::binary).close();
        fs::resize_file(file, 8 * kMiB);
    }
    bool cancelled = false;
    for (const int delayMs : {20, 5, 0}) {
        std::stop_source source;
        std::thread asker([&source, delayMs] {
            std::this_thread::sleep_for(std::chrono::milliseconds(delayMs));
            source.request_stop();
        });
        const auto plan = planOf(manifest, temp.path(), source.get_token());
        asker.join();
        if (!plan.ok()) {
            EXPECT_EQ(plan.error().code, core::ErrorCode::InvalidState);
            EXPECT_EQ(plan.error().message, "cancelled");
            cancelled = true;
            break;
        }
    }
    EXPECT_TRUE(cancelled);
}

// ---- the root -----------------------------------------------------------------------------

TEST(Plan, ARootThatIsNotAFolderIsNotFound)
{
    TempDir temp;
    const update::Manifest manifest = manifestOf("v1");
    EXPECT_EQ(planOf(manifest, temp.path() / "missing").error().code, core::ErrorCode::NotFound);
    writeFile(temp.path() / "a_file", "x");
    EXPECT_EQ(planOf(manifest, temp.path() / "a_file").error().code, core::ErrorCode::NotFound);
    EXPECT_EQ(planOf(manifest, fs::path()).error().code, core::ErrorCode::NotFound);
}

TEST(Plan, AnInstallRootThatIsItselfALinkIsFollowedBecauseTheLinkIsTheInstallsOwnLocation)
{
    // A person who put the program in a folder reached through a link (a junction to another disk)
    // is not refused: only what is BELOW the root is not followed.
    TempDir temp;
    copyTree("v1", temp.path() / "real");
    DirectoryLink link(temp.path() / "linked", temp.path() / "real");
    if (!link.made()) {
        GTEST_SKIP() << "this machine does not let the test make a directory link";
    }
    const auto plan = planOf(manifestOf("v1"), temp.path() / "linked");
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(plan->changedFiles, 0u);
}

// ---- a manifest that names a place outside the install ------------------------------------

TEST(Plan, APathThatCouldLeaveTheInstallIsRefusedAndReadsNothingOutsideIt)
{
    TempDir temp;
    copyTree("v1", temp.path() / "root");
    // A file outside the root that has exactly the bytes (and so the hash) the manifest asks for: a
    // plan that followed the path would call it unchanged.
    writeFile(temp.path() / "outside.txt", "outside");
    const std::string hash = core::sha256Hex("outside");
    const std::string absolute = (temp.path() / "outside.txt").generic_string();
    const std::vector<std::string> paths{"../outside.txt",
                                         "bin/../../outside.txt",
                                         "bin/..",
                                         absolute,
                                         "/outside.txt",
                                         "bin\\..\\..\\outside.txt",
                                         "bin//x",
                                         "bin/./x",
                                         "",
                                         "bin/",
                                         "/",
                                         "bin/x:stream",
                                         "C:/outside.txt",
                                         ".."};
    for (const std::string& path : paths) {
        update::Manifest manifest = absentFiles({7});
        manifest.files[0].path = path;
        manifest.files[0].sha256 = hash;
        const auto plan = planOf(manifest, temp.path() / "root");
        ASSERT_FALSE(plan.ok()) << '"' << path << '"';
        EXPECT_EQ(plan.error().code, core::ErrorCode::InvalidArgument) << '"' << path << '"';
    }
}

// ---- links are never followed -------------------------------------------------------------

TEST(Plan, AFolderOnTheWayThatIsALinkIsRefusedByTheNameOfTheFileItWouldHaveReached)
{
    TempDir temp;
    copyTree("v1", temp.path() / "root");
    // share/ replaced by a link to a folder elsewhere that holds the very same files.
    copyTree("v1", temp.path() / "elsewhere");
    fs::remove_all(temp.path() / "root" / "share");
    DirectoryLink link(temp.path() / "root" / "share", temp.path() / "elsewhere" / "share");
    if (!link.made()) {
        GTEST_SKIP() << "this machine does not let the test make a directory link";
    }
    const auto plan = planOf(manifestOf("v1"), temp.path() / "root");
    ASSERT_FALSE(plan.ok()) << "the files are all there, through the link";
    EXPECT_EQ(plan.error().code, core::ErrorCode::InvalidState);
    // Manifest order is bin/, etc/, then share/: the first file reached through the link.
    EXPECT_EQ(plan.error().context, "share/katana/empty.txt");
    EXPECT_NE(plan.error().message.find("link"), std::string::npos) << plan.error().message;
    EXPECT_NE(plan.error().message.find("share/katana/empty.txt"), std::string::npos);
    // What it pointed at is as it was.
    EXPECT_TRUE(fs::exists(temp.path() / "elsewhere" / "share" / "katana" / "readme.txt"));
}

TEST(Plan, AFileThatIsALinkIsNotReadAndCountsAsChangedUnlessItIsASeed)
{
    TempDir temp;
    copyTree("v1", temp.path() / "root");
    fs::create_directories(temp.path() / "elsewhere");
    writeFile(temp.path() / "elsewhere" / "file.txt", "x");
    // bin/libcore-1.dll (a lib) and etc/katana/settings.ini (a seed) each replaced by a link.
    fs::remove(temp.path() / "root" / "bin" / "libcore-1.dll");
    fs::remove(temp.path() / "root" / "etc" / "katana" / "settings.ini");
    DirectoryLink libLink(temp.path() / "root" / "bin" / "libcore-1.dll",
                          temp.path() / "elsewhere");
    DirectoryLink seedLink(temp.path() / "root" / "etc" / "katana" / "settings.ini",
                           temp.path() / "elsewhere");
    if (!libLink.made() || !seedLink.made()) {
        GTEST_SKIP() << "this machine does not let the test make a directory link";
    }
    const auto plan = planOf(manifestOf("v1"), temp.path() / "root");
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(changedIndexes(*plan), (Indexes{2}))
        << "the link in a lib's place is replaced; the seed link is kept";
    EXPECT_TRUE(fs::exists(temp.path() / "elsewhere" / "file.txt"))
        << "what it points at is untouched";
}

TEST(Plan, AFileWhereAFolderShouldBeAndAFolderWhereAFileShouldBeAreRefusedByName)
{
    {
        TempDir temp;
        copyTree("v1", temp.path());
        fs::remove_all(temp.path() / "share");
        writeFile(temp.path() / "share", "not a folder");
        const auto plan = planOf(manifestOf("v1"), temp.path());
        ASSERT_FALSE(plan.ok());
        EXPECT_EQ(plan.error().code, core::ErrorCode::InvalidState);
        EXPECT_EQ(plan.error().context, "share/katana/empty.txt");
    }
    {
        TempDir temp;
        copyTree("v1", temp.path());
        fs::remove(temp.path() / "bin" / "katana.exe");
        fs::create_directory(temp.path() / "bin" / "katana.exe");
        const auto plan = planOf(manifestOf("v1"), temp.path());
        ASSERT_FALSE(plan.ok());
        EXPECT_EQ(plan.error().code, core::ErrorCode::InvalidState);
        EXPECT_EQ(plan.error().context, "bin/katana.exe");
    }
}

TEST(Plan, AFileThatCannotBeReadIsAPlanErrorThatNamesIt)
{
    TempDir temp;
    copyTree("v1", temp.path());
    const fs::path file = temp.path() / "bin" / "libcore-1.dll";
#ifdef _WIN32
    // Held open by another program with no sharing, as a virus scanner or a running copy might.
    HANDLE held = CreateFileW(file.c_str(), GENERIC_READ, 0, nullptr, OPEN_EXISTING,
                              FILE_ATTRIBUTE_NORMAL, nullptr);
    ASSERT_NE(held, INVALID_HANDLE_VALUE);
#else
    fs::permissions(file, fs::perms::none);
    if (std::ifstream(file).is_open()) {
        GTEST_SKIP() << "this user can read a file with no permissions (root?)";
    }
#endif
    const auto plan = planOf(manifestOf("v1"), temp.path());
#ifdef _WIN32
    CloseHandle(held);
#else
    fs::permissions(file, fs::perms::owner_all);
#endif
    ASSERT_FALSE(plan.ok()) << "an unreadable file is not silently counted as changed or as kept";
    EXPECT_EQ(plan.error().code, core::ErrorCode::FileImportFailure);
    EXPECT_NE(plan.error().message.find("bin/libcore-1.dll"), std::string::npos)
        << plan.error().message;
    EXPECT_EQ(plan.error().context.rfind("bin/libcore-1.dll", 0), 0u) << plan.error().context;
}

// ---- the arithmetic of the totals ---------------------------------------------------------

TEST(Plan, TheFreeSpaceNeededIsTwoPointSixTimesTheChangedBytesRoundedUp)
{
    // 2.6 * n by hand: n * 26 / 10 rounded up.
    const struct {
        std::uint64_t bytes;
        std::uint64_t needed;
    } table[] = {
        {0, 0},                         // nothing to stage
        {1, 3},                         // 2.6
        {2, 6},                         // 5.2
        {3, 8},                         // 7.8
        {4, 11},                        // 10.4
        {5, 13},                        // 13.0 exactly: not 14
        {10, 26},                       // 26.0 exactly
        {1000, 2600},                   // 2600.0 exactly
        {1001, 2603},                   // 2602.6
        {516, 1342},                    // 1341.6
        {kMiB, 2726298},                // 2.6 * 1048576 = 2726297.6
        {2147483648ull, 5583457485ull}, // the 2 GiB an install may hold: 5583457484.8
    };
    TempDir temp;
    for (const auto& row : table) {
        const auto plan = planOf(absentFiles({row.bytes}), temp.path());
        ASSERT_TRUE(plan.ok()) << row.bytes << ": " << plan.error().describe();
        EXPECT_EQ(plan->changedBytes, row.bytes);
        EXPECT_EQ(plan->requiredFreeBytes, row.needed) << row.bytes << " bytes";
    }
}

TEST(Plan, TheFreeSpaceIsFigureFromTheSumNotFromEachFile)
{
    // 1 + 1 + 1 = 3 bytes -> 7.8 -> 8, where three files rounded up one by one would say 9.
    TempDir temp;
    const auto plan = planOf(absentFiles({1, 1, 1}), temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_EQ(plan->changedBytes, 3u);
    EXPECT_EQ(plan->requiredFreeBytes, 8u);
}

TEST(Plan, AManifestBeyondTheLimitsOfAnInstallIsRefusedBeforeAnyFileIsRead)
{
    TempDir temp;
    // 2 GiB is the most an install may hold (paths.hpp); one byte more is refused.
    EXPECT_TRUE(planOf(absentFiles({update::kMaxInstallBytes}), temp.path()).ok());
    EXPECT_EQ(planOf(absentFiles({update::kMaxInstallBytes + 1}), temp.path()).error().code,
              core::ErrorCode::InvalidArgument);
    // Two files that are each within it and together are not; and a sum that wraps 64 bits.
    EXPECT_EQ(planOf(absentFiles({1536 * kMiB, 1536 * kMiB}), temp.path()).error().code,
              core::ErrorCode::InvalidArgument);
    EXPECT_EQ(planOf(absentFiles({UINT64_MAX, 2}), temp.path()).error().code,
              core::ErrorCode::InvalidArgument);
    // More files than an install may have.
    update::Manifest many;
    for (std::size_t i = 0; i < update::kMaxFiles + 1; ++i) {
        update::ManifestFile entry;
        entry.path = "bin/f" + std::to_string(i);
        many.files.push_back(entry);
    }
    EXPECT_EQ(planOf(many, temp.path()).error().code, core::ErrorCode::InvalidArgument);
}

// ---- the requests -------------------------------------------------------------------------

TEST(Plan, OnlyChangedFilesAreFetchedAndFilesThatAreKeptBetweenThemAreNotInTheRange)
{
    // v2's own tree with katana.exe (44) and readme.txt (2938) altered: the files between them in
    // the payload are kept, and are not members of the request that crosses them.
    TempDir temp;
    copyTree("v2", temp.path());
    writeFile(temp.path() / "bin" / "katana.exe", "old program");
    writeFile(temp.path() / "share" / "katana" / "readme.txt", "old readme");
    const auto plan = planOf(manifestOf("v2"), temp.path());
    ASSERT_TRUE(plan.ok()) << plan.error().describe();
    EXPECT_EQ(changedIndexes(*plan), (Indexes{0, 9}));
    ASSERT_EQ(plan->ranges.size(), 1u);
    EXPECT_EQ(plan->ranges[0].offset, 44u);
    EXPECT_EQ(plan->ranges[0].length, 2949u);
    ASSERT_EQ(plan->ranges[0].members.size(), 2u);
    EXPECT_EQ(plan->ranges[0].members[0].fileIndex, 0u);
    EXPECT_EQ(plan->ranges[0].members[1].fileIndex, 9u);
}

TEST(Plan, ChangedFilesFarApartInThePayloadAreSeparateRequests)
{
    // Offsets 100, 110, then 100 MiB on: two requests (a gap of 100 MiB is not worth bridging).
    TempDir temp;
    update::Manifest manifest = absentFiles({10, 10, 10});
    manifest.files[2].offset = 100 * kMiB;
    const auto plan = planOf(manifest, temp.path());
    ASSERT_TRUE(plan.ok());
    ASSERT_EQ(plan->ranges.size(), 2u);
    EXPECT_EQ(plan->ranges[0].offset, 100u);
    EXPECT_EQ(plan->ranges[0].length, 12u); // [100,102) and [110,112): 100..112
    EXPECT_EQ(plan->ranges[0].members.size(), 2u);
    EXPECT_EQ(plan->ranges[1].offset, 100 * kMiB);
    EXPECT_EQ(plan->ranges[1].length, 2u);
}

TEST(Plan, AManifestWithNoFilesIsAPlanWithNothingToDo)
{
    TempDir temp;
    const auto plan = planOf(update::Manifest{}, temp.path());
    ASSERT_TRUE(plan.ok());
    EXPECT_EQ(plan->totalFiles, 0u);
    EXPECT_TRUE(plan->files.empty());
    EXPECT_TRUE(plan->ranges.empty());
}
