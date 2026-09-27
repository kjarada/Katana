#pragma once

// Where a third-party library's data files are, found from where the library
// itself was loaded (docs/building.md, "Linux").
//
// PROJ finds proj.db, and every grid, through a search path compiled in when
// it was built, unless PROJ_DATA says otherwise. A RELOCATABLE install - the
// Linux toolchain's conda-forge prefix, like MSYS2 on Windows - rewrites that
// compiled path when it is installed, and on Linux the rewritten one broke:
// with PDAL loaded (as it is in the application and in every katana_io test),
// PROJ opened its data DIRECTORY as proj.db and as each grid file. GDAL then
// identified no coordinate system, and PROJ called a grid that is not
// installed available, so a transformation chose it and failed. conda-forge
// papers over this with an activation script that sets PROJ_DATA; nothing
// that starts Katana runs it.
//
// So the data is looked for RELATIVE TO THE LIBRARY, the rule
// gdal_adapter.cpp already applies to GDAL's own data on Windows: libproj in
// <prefix>/lib, its data in <prefix>/share/proj. A distribution's PROJ, whose
// compiled path is right, has no share/proj beside its library directory
// (Debian's is /usr/lib/<triplet>), so nothing is changed there.

#include <filesystem>
#include <optional>
#include <string_view>

namespace katana::core {

// The file a loaded shared library was mapped from, found by the start of its
// file name ("libproj.so" matches libproj.so.25). Linux and macOS;
// std::nullopt elsewhere - on Windows gdal_adapter.cpp finds DLLs by name
// itself, and PROJ finds its data beside its own DLL.
[[nodiscard]] std::optional<std::filesystem::path>
loadedLibraryPath(std::string_view fileNamePrefix);

// <prefix>/<relative> for the loaded library found as loadedLibraryPath does,
// where the library is in <prefix>/lib - the layout of the toolchain prefix
// and of the Linux and macOS bundles (cmake/KatanaDeployUnix.cmake.in) - when
// that file or directory exists. std::nullopt when the library is not loaded
// or the path does not exist, as for a distribution's library in
// /usr/lib/<triplet>.
[[nodiscard]] std::optional<std::filesystem::path>
dataBesideLibrary(std::string_view fileNamePrefix, const std::filesystem::path& relative);

// The directory PROJ's data should be read from, when Katana should say:
// on Linux and macOS, <prefix>/share/proj beside the loaded libproj, when it holds
// proj.db and neither PROJ_DATA nor PROJ_LIB is set (an explicit choice
// outranks a default). std::nullopt otherwise - PROJ's own search is then
// right. Worked out once per process; safe from any thread. Each user of
// PROJ passes it to its own search path: proj_context_set_search_paths for a
// PROJ context, OSRSetPROJSearchPaths for GDAL's (and so PDAL's).
[[nodiscard]] const std::optional<std::filesystem::path>& projDataDirectory();

} // namespace katana::core
