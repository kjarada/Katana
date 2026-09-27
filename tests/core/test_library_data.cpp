// Where a third-party library's data is, found from where the library was
// loaded (core/library_data.hpp).

#include <gtest/gtest.h>

#include <cstdlib>
#include <filesystem>

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
