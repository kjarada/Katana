#pragma once

// GDAL's PROJ search path, set once before GDAL or PDAL first looks anything
// up (core/library_data.hpp has why). Private to katana_io: the GDAL adapter
// calls it before registering the drivers, and the point-cloud engine before
// every PDAL entry point, since PDAL reads and writes coordinate systems
// through GDAL's OSR - and both files may see core, not each other.

#include <mutex>
#include <string>

#include <ogr_srs_api.h>

#include "katana/core/library_data.hpp"

namespace katana::io_detail {

inline void pointGdalAtProjData()
{
    static std::once_flag once;
    std::call_once(once, [] {
        if (const auto& data = katana::core::projDataDirectory()) {
            const std::string path = data->string();
            const char* const paths[] = {path.c_str(), nullptr};
            OSRSetPROJSearchPaths(paths);
        }
    });
}

} // namespace katana::io_detail
