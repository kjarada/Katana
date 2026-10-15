#pragma once

// GDAL's PROJ search path, set once before GDAL or PDAL first looks anything
// up (core/library_data.hpp has why). Private to katana_io: the GDAL adapter
// calls it before registering the drivers, and the point-cloud engine before
// every PDAL entry point, since PDAL reads and writes coordinate systems
// through GDAL's OSR - and both files may see core, not each other.

#include <cstdlib>
#include <filesystem>
#include <mutex>
#include <string>

#include <cpl_error.h>
#include <ogr_srs_api.h>

#include "katana/core/library_data.hpp"

namespace katana::io_detail {

// Whether GDAL's PROJ can open proj.db as things stand: EPSG:4326 is read from
// the database (GDAL carries no definitions of its own), so the import fails
// when the database does not open - PROJ_LIB naming a folder without one, or
// with one of another PROJ's layout, which PROJ rejects.
inline bool gdalFindsProjDatabase()
{
    CPLPushErrorHandler(CPLQuietErrorHandler);
    OGRSpatialReferenceH reference = OSRNewSpatialReference(nullptr);
    const bool found = OSRImportFromEPSG(reference, 4326) == OGRERR_NONE;
    OSRDestroySpatialReference(reference);
    CPLPopErrorHandler();
    CPLErrorReset(); // what the probe said is not a later call's last error
    return found;
}

// PROJ takes a path as UTF-8; path::string() on Windows is the ANSI code page,
// which turns a character outside it into '?'.
inline std::string projPathText(const std::filesystem::path& path)
{
    const std::u8string utf8 = path.generic_u8string();
    return std::string(utf8.begin(), utf8.end());
}

inline void pointGdalAtProjData()
{
    static std::once_flag once;
    std::call_once(once, [] {
        if (const auto& data = katana::core::projDataDirectory()) {
            const std::string path = projPathText(*data);
            const char* const paths[] = {path.c_str(), nullptr};
            OSRSetPROJSearchPaths(paths);
            return;
        }
        // PROJ_DATA or PROJ_LIB in the environment replaces the search beside
        // the DLL, and another GIS package leaves one machine-wide naming data
        // this PROJ cannot open (core/library_data.hpp). An explicit choice
        // that works is kept; one that does not falls back on the data Katana
        // carries, as geodesy's contexts do. Asked only when the environment
        // says something, so an ordinary start pays nothing for it.
        if (std::getenv("PROJ_DATA") == nullptr && std::getenv("PROJ_LIB") == nullptr) {
            return;
        }
        const auto& bundled = katana::core::bundledProjDataDirectory();
        if (bundled && !gdalFindsProjDatabase()) {
            const std::string path = projPathText(*bundled);
            const char* const paths[] = {path.c_str(), nullptr};
            OSRSetPROJSearchPaths(paths);
        }
    });
}

} // namespace katana::io_detail
