#include "katana/core/library_data.hpp"

#include <cstdlib>
#include <string>
#include <system_error>

#if defined(__linux__)
#include <link.h>
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
#else
    (void)fileNamePrefix;
    return std::nullopt;
#endif
}

const std::optional<std::filesystem::path>& projDataDirectory()
{
    static const std::optional<std::filesystem::path> directory =
        []() -> std::optional<std::filesystem::path> {
#if defined(__linux__)
        if (std::getenv("PROJ_DATA") != nullptr || std::getenv("PROJ_LIB") != nullptr) {
            return std::nullopt;
        }
        const auto library = loadedLibraryPath("libproj.so");
        if (!library) {
            return std::nullopt;
        }
        std::error_code ignored;
        // The loader's name may be <prefix>/lib/./libproj.so.25 (an rpath
        // with a dot in it); canonical makes parent_path mean the directory.
        const std::filesystem::path real = std::filesystem::weakly_canonical(*library, ignored);
        const std::filesystem::path data = real.parent_path().parent_path() / "share" / "proj";
        if (std::filesystem::is_regular_file(data / "proj.db", ignored)) {
            return data;
        }
#endif
        return std::nullopt;
    }();
    return directory;
}

} // namespace katana::core
