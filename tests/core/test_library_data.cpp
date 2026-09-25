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
#else
    GTEST_SKIP() << "found by name on Linux only; Windows finds its DLLs where it needs them";
#endif
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
