#include "katana/core/library_data.hpp"

#include <cstdlib>
#include <string>
#include <system_error>

#if defined(__linux__)
#include <link.h>
#elif defined(__APPLE__)
#include <mach-o/dyld.h>

#include <cstdint>
#elif defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
// PSAPI_VERSION 2 maps EnumProcessModules onto the copy in kernel32, so no
// extra library is linked for it.
#define PSAPI_VERSION 2
#include <psapi.h>

#include <algorithm>
#include <vector>
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
#elif defined(_WIN32)
    // By NAME among the loaded modules, for the reason above: asking Windows
    // which module holds the address of a function gives, under MinGW, the
    // import THUNK inside the importing program, so the program itself is
    // named (gdal_adapter.cpp found this the hard way). Wide characters
    // throughout: a path outside the ANSI code page (a Cyrillic or CJK folder
    // name) does not survive a narrow string.
    std::vector<HMODULE> modules(1024);
    DWORD bytes = 0;
    if (EnumProcessModules(GetCurrentProcess(), modules.data(),
                           static_cast<DWORD>(modules.size() * sizeof(HMODULE)), &bytes) == 0) {
        return std::nullopt;
    }
    modules.resize(std::min<std::size_t>(modules.size(), bytes / sizeof(HMODULE)));
    for (const HMODULE module : modules) {
        std::wstring candidate(512, L' ');
        for (;;) {
            const DWORD length = GetModuleFileNameW(module, candidate.data(),
                                                    static_cast<DWORD>(candidate.size()));
            if (length == 0) {
                candidate.clear();
                break;
            }
            if (length < candidate.size()) {
                candidate.resize(length);
                break;
            }
            candidate.resize(candidate.size() * 2); // truncated: longer than the buffer
        }
        if (candidate.empty()) {
            continue;
        }
        const std::filesystem::path path(candidate);
        const std::wstring name = path.filename().wstring();
        // The prefix is ASCII (a library's name); compared without regard to
        // ASCII case, as Windows file names are.
        const auto lower = [](wchar_t c) {
            return c >= L'A' && c <= L'Z' ? static_cast<wchar_t>(c - L'A' + L'a') : c;
        };
        if (name.size() >= fileNamePrefix.size() &&
            std::equal(fileNamePrefix.begin(), fileNamePrefix.end(), name.begin(),
                       [&](char wanted, wchar_t found) {
                           return lower(static_cast<wchar_t>(static_cast<unsigned char>(wanted))) ==
                                  lower(found);
                       })) {
            return path;
        }
    }
    return std::nullopt;
#else
    (void)fileNamePrefix;
    return std::nullopt;
#endif
}

std::optional<std::filesystem::path> dataBesideLibraryFile(const std::filesystem::path& library,
                                                           const std::filesystem::path& relative)
{
    // An empty name would look beside the working directory. It is refused
    // here, not by the is_absolute test below: libstdc++'s weakly_canonical
    // leaves an empty path empty, but libc++'s makes it the working directory,
    // which is absolute.
    if (library.empty()) {
        return std::nullopt;
    }
    std::error_code ignored;
    // The loader's name may be <prefix>/lib/./libproj.so.25 (an rpath with a
    // dot in it), or a symbolic link; canonical makes parent_path mean the
    // directory the file really is in.
    const std::filesystem::path real = std::filesystem::weakly_canonical(library, ignored);
    if (!real.is_absolute()) {
        return std::nullopt; // a relative name is not a loaded library's
    }
    const std::filesystem::path directory = real.parent_path();
    const std::filesystem::path prefix = directory.parent_path();
    if (const auto data = prefix / relative; std::filesystem::exists(data, ignored)) {
        return data;
    }
    if (directory.filename() == "Frameworks") {
        if (const auto data = prefix / "Resources" / relative;
            std::filesystem::exists(data, ignored)) {
            return data;
        }
    }
    return std::nullopt;
}

std::optional<std::filesystem::path> dataBesideLibrary(std::string_view fileNamePrefix,
                                                       const std::filesystem::path& relative)
{
    const auto library = loadedLibraryPath(fileNamePrefix);
    if (!library) {
        return std::nullopt;
    }
    return dataBesideLibraryFile(*library, relative);
}

const std::optional<std::filesystem::path>& bundledProjDataDirectory()
{
    static const std::optional<std::filesystem::path> directory =
        []() -> std::optional<std::filesystem::path> {
#if defined(__APPLE__)
        constexpr std::string_view library = "libproj."; // libproj.25.dylib
#elif defined(_WIN32)
        constexpr std::string_view library = "libproj"; // libproj-25.dll
#else
        constexpr std::string_view library = "libproj.so";
#endif
        if (auto data = dataBesideLibrary(library, "share/proj/proj.db")) {
            return data->parent_path();
        }
        return std::nullopt;
    }();
    return directory;
}

const std::optional<std::filesystem::path>& projDataDirectory()
{
    static const std::optional<std::filesystem::path> directory =
        []() -> std::optional<std::filesystem::path> {
#if defined(__linux__) || defined(__APPLE__)
        if (std::getenv("PROJ_DATA") != nullptr || std::getenv("PROJ_LIB") != nullptr) {
            return std::nullopt;
        }
        return bundledProjDataDirectory();
#else
        // Windows: PROJ finds share/proj beside its DLL by itself; the
        // environment's choice, if any, is tried first and the bundled data
        // is what its users fall back on (bundledProjDataDirectory).
        return std::nullopt;
#endif
    }();
    return directory;
}

} // namespace katana::core
