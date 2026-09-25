#pragma once

// Shared between the katana_io sources, never installed: GDAL's drivers are
// registered once per process (gdal_adapter.cpp, ensureRegistered), and every
// file here that calls GDAL must go through that one registration rather than
// call GDALAllRegister itself - a second registration path would be a second
// place GDAL_DATA has to be found.

namespace katana::gis::detail {

void ensureGdalRegistered();

} // namespace katana::gis::detail
