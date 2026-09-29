// Where a third-party library's data is, found from where the library was
// loaded (core/library_data.hpp).

#include <gtest/gtest.h>

#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <string>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/core/library_data.hpp"

using katana::core::loadedLibraryPath;

TEST(LibraryData, ALoadedLibraryIsFoundByTheStartOfItsFileName)
{
#if defined(__linux__)
    // Every Linux process has the C library mapped, under a versioned name
    // (libc.so.6), which a prefix of its name must match.
    const auto libc = loadedLibraryPath("libc.so");
    ASSERT_TRUE(libc.has_value());
    EXPECT_TRUE(libc->filename().string().starts_with("libc.so"));
    EXPECT_TRUE(std::filesystem::exists(*libc)) << libc->string();
#elif defined(__APPLE__)
    // Every macOS process has libSystem mapped (libSystem.B.dylib).
    const auto system = loadedLibraryPath("libSystem.");
    ASSERT_TRUE(system.has_value());
    EXPECT_TRUE(system->filename().string().starts_with("libSystem."));
#else
    GTEST_SKIP() << "found by name on Linux and macOS only; Windows finds its DLLs where it needs "
                    "them";
#endif
}

TEST(LibraryData, DataBesideALibraryIsTheLibrarysGrandparentJoinedToThePathAsked)
{
#if defined(__linux__)
    // The C library itself, named relative to its directory's parent, is a
    // path known to exist without assuming where the distribution put it.
    const auto libc = loadedLibraryPath("libc.so");
    ASSERT_TRUE(libc.has_value());
    const std::filesystem::path real = std::filesystem::canonical(*libc);
    const std::filesystem::path relative = real.parent_path().filename() / real.filename();
    const auto found = katana::core::dataBesideLibrary("libc.so", relative);
    ASSERT_TRUE(found.has_value());
    EXPECT_TRUE(std::filesystem::equivalent(*found, real)) << found->string();

    EXPECT_FALSE(katana::core::dataBesideLibrary("libc.so", "katana_no_such_directory/x"));
#else
    GTEST_SKIP() << "the C library is found by name on Linux only";
#endif
}

namespace {

// The process id, so that two processes running these cases at once - the
// core suite and its simd_* rerun under ctest --parallel - never share a
// folder. With one fixed name, one removed the folder while the other held a
// file in it open, and the uncaught filesystem_error ended that whole run.
// A reused id is harmless: the folder is cleared before it is used.
std::string processTag()
{
#if defined(_WIN32)
    return std::to_string(_getpid());
#else
    return std::to_string(getpid());
#endif
}

// A throwaway prefix under the test's temporary directory, removed afterwards.
class ScratchPrefix {
  public:
    explicit ScratchPrefix(const std::string& name)
        : root_(std::filesystem::path(::testing::TempDir()) / (name + "_" + processTag()))
    {
        std::filesystem::remove_all(root_);
        std::filesystem::create_directories(root_);
    }
    ~ScratchPrefix() { std::filesystem::remove_all(root_); }
    ScratchPrefix(const ScratchPrefix&) = delete;
    ScratchPrefix& operator=(const ScratchPrefix&) = delete;

    std::filesystem::path touch(const std::filesystem::path& relative) const
    {
        const auto path = root_ / relative;
        std::filesystem::create_directories(path.parent_path());
        std::ofstream(path) << "x";
        return path;
    }
    const std::filesystem::path& root() const { return root_; }

  private:
    std::filesystem::path root_;
};

} // namespace

TEST(LibraryData, DataOfALibraryInLibIsInTheSharedPrefix)
{
    const ScratchPrefix prefix("katana_library_data_lib");
    const auto library = prefix.touch("lib/libproj.so.25");
    prefix.touch("share/proj/proj.db");
    const auto found = katana::core::dataBesideLibraryFile(library, "share/proj/proj.db");
    ASSERT_TRUE(found.has_value());
    EXPECT_TRUE(std::filesystem::equivalent(*found, prefix.root() / "share/proj/proj.db"));
}

TEST(LibraryData, DataOfALibraryInAnApplicationBundlesFrameworksIsInItsResources)
{
    // Katana.app/Contents/Frameworks/libproj.25.dylib, with its data in
    // Contents/Resources/share/proj, where code signing wants data kept.
    const ScratchPrefix bundle("katana_library_data_bundle");
    const auto library = bundle.touch("Contents/Frameworks/libproj.25.dylib");
    bundle.touch("Contents/Resources/share/proj/proj.db");
    bundle.touch("Contents/Resources/ssl/cacert.pem");
    const auto proj = katana::core::dataBesideLibraryFile(library, "share/proj/proj.db");
    ASSERT_TRUE(proj.has_value());
    EXPECT_TRUE(std::filesystem::equivalent(*proj, bundle.root() /
                                                       "Contents/Resources/share/proj/proj.db"));
    const auto certificates = katana::core::dataBesideLibraryFile(library, "ssl/cacert.pem");
    ASSERT_TRUE(certificates.has_value());
    EXPECT_TRUE(std::filesystem::equivalent(*certificates,
                                            bundle.root() / "Contents/Resources/ssl/cacert.pem"));
}

TEST(LibraryData, ResourcesAreLookedInOnlyBesideAFrameworksDirectory)
{
    // A library in lib/ of a prefix that happens to have a Resources folder
    // does not borrow data from it.
    const ScratchPrefix prefix("katana_library_data_not_bundle");
    const auto library = prefix.touch("lib/libproj.so.25");
    prefix.touch("Resources/share/proj/proj.db");
    EXPECT_FALSE(katana::core::dataBesideLibraryFile(library, "share/proj/proj.db"));
}

TEST(LibraryData, MissingDataBesideALibraryIsNotFound)
{
    const ScratchPrefix bundle("katana_library_data_missing");
    const auto library = bundle.touch("Contents/Frameworks/libgdal.dylib");
    EXPECT_FALSE(katana::core::dataBesideLibraryFile(library, "share/gdal"));
}

// An empty name is no library, wherever the process happens to stand. It is
// asked from inside a prefix that does hold share/gdal: libc++ makes an empty
// path the working directory, so a check that relied on the empty path
// staying relative found the data there (as it did on the Windows ARM64
// runner), while from a directory with nothing above it the same fault
// passes unseen.
TEST(LibraryData, AnEmptyNameIsRefusedWhereverTheWorkingDirectoryIs)
{
    const ScratchPrefix prefix("katana_library_data_empty");
    prefix.touch("share/gdal/gdalvrt.xsd");
    const std::filesystem::path inside = prefix.root() / "lib" / "bin";
    std::filesystem::create_directories(inside);
    const std::filesystem::path before = std::filesystem::current_path();
    std::filesystem::current_path(inside);
    const auto found = katana::core::dataBesideLibraryFile(std::filesystem::path{}, "share/gdal");
    std::filesystem::current_path(before);
    EXPECT_FALSE(found) << *found;
}

TEST(LibraryData, NoDataIsBesideALibraryThatIsNotLoaded)
{
    EXPECT_FALSE(katana::core::dataBesideLibrary("libkatana_no_such_library.so", "share"));
}

TEST(LibraryData, ALibraryThatIsNotLoadedIsNotFound)
{
    EXPECT_FALSE(loadedLibraryPath("libkatana_no_such_library.so").has_value());
}

TEST(LibraryData, AProjDataDirectoryGivenHoldsProjDb)
{
    // This process has no PROJ loaded, or the environment names PROJ's data
    // already; either way nothing is given, or what is given is real.
    const auto& data = katana::core::projDataDirectory();
    if (data.has_value()) {
        EXPECT_TRUE(std::filesystem::is_regular_file(*data / "proj.db")) << data->string();
        EXPECT_EQ(std::getenv("PROJ_DATA"), nullptr);
    }
}
