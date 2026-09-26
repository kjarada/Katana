#include "katana/core/library_data.hpp"

#include <cstdlib>
#include <string>
#include <system_error>

#if defined(__linux__)
#include <link.h>
#elif defined(__APPLE__)
#include <mach-o/dyld.h>

#include <cstdint>
#endif

namespace katana::core {

std::optional<std::filesystem::path> loadedLibraryPath(std::string_view fileNamePrefix)
{
#if defined(__linux__)
    // By NAME among the loaded objects, not by dladdr on one of the library's
    // functions: in an executable that is not position-independent, the
    // address of an imported function is its PLT stub in the executable, and
    // dladdr would name the executable - the MinGW import-thunk trap
    // gdal_adapter.cpp describes, in its Linux form.
    struct Search {
        std::string_view prefix;
        std::optional<std::filesystem::path> found;
    } search{fileNamePrefix, std::nullopt};
    dl_iterate_phdr(
        [](dl_phdr_info* info, std::size_t /*size*/, void* data) -> int {
            auto& s = *static_cast<Search*>(data);
            if (info->dlpi_name == nullptr || *info->dlpi_name == '\0') {
                return 0; // the executable itself
            }
            const std::filesystem::path path(info->dlpi_name);
            if (path.filename().string().starts_with(s.prefix)) {
                s.found = path;
                return 1; // stop
            }
            return 0;
        },
        &search);
    return search.found;
#elif defined(__APPLE__)
    // dyld's own list of loaded images, by name for the reason given above.
    // Image 0 is the executable; its name never starts like a library's.
    const std::uint32_t count = _dyld_image_count();
    for (std::uint32_t i = 0; i < count; ++i) {
        const char* const name = _dyld_get_image_name(i);
        if (name == nullptr || *name == '\0') {
            continue;
        }
        const std::filesystem::path path(name);
        if (path.filename().string().starts_with(fileNamePrefix)) {
            return path;
        }
    }
    return std::nullopt;
#else
    (void)fileNamePrefix;
    return std::nullopt;
#endif
}

std::optional<std::filesystem::path> dataBesideLibrary(std::string_view fileNamePrefix,
                                                       const std::filesystem::path& relative)
{
    const auto library = loadedLibraryPath(fileNamePrefix);
    if (!library) {
        return std::nullopt;
    }
    std::error_code ignored;
    // The loader's name may be <prefix>/lib/./libproj.so.25 (an rpath with a
    // dot in it), or a symbolic link; canonical makes parent_path mean the
    // directory the file really is in.
    const std::filesystem::path real = std::filesystem::weakly_canonical(*library, ignored);
    const std::filesystem::path data = real.parent_path().parent_path() / relative;
    if (std::filesystem::exists(data, ignored)) {
        return data;
    }
    return std::nullopt;
}

const std::optional<std::filesystem::path>& projDataDirectory()
{
    static const std::optional<std::filesystem::path> directory =
        []() -> std::optional<std::filesystem::path> {
#if defined(__linux__) || defined(__APPLE__)
        if (std::getenv("PROJ_DATA") != nullptr || std::getenv("PROJ_LIB") != nullptr) {
            return std::nullopt;
        }
#if defined(__APPLE__)
        constexpr std::string_view library = "libproj."; // libproj.25.dylib
#else
        constexpr std::string_view library = "libproj.so";
#endif
        if (auto data = dataBesideLibrary(library, "share/proj/proj.db")) {
            return data->parent_path();
        }
#endif
        return std::nullopt;
    }();
    return directory;
}

} // namespace katana::core
