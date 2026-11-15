/*
 * The GDAL declarations of src/ffi.rs held to GDAL's own headers: every size
 * and constant the Rust copies is asserted here, so a GDAL whose headers say
 * otherwise fails the build (build.rs compiles this file) rather than
 * misbehaving. The same numbers are Rust const assertions in src/ffi.rs.
 *
 * These are the eight calls of the PROJ search path
 * (src/proj_search_paths.rs, the twin of src/katana_io/proj_search_paths.hpp).
 * They are the pointcloud layer's own: it may see core only, not gis, whose
 * declarations are katana_gis's (docs/rust.md, step 5).
 */

#include <cpl_error.h>
#include <ogr_core.h>
#include <ogr_srs_api.h>

/* CPLErr is a C enumeration, which Rust declares as a c_int alias. */
_Static_assert(sizeof(CPLErr) == sizeof(int), "CPLErr is int-sized");
_Static_assert(sizeof(CPLErrorNum) == sizeof(int), "CPLErrorNum is an int");
_Static_assert(sizeof(OGRErr) == sizeof(int), "OGRErr is an int");
_Static_assert(OGRERR_NONE == 0, "OGRERR_NONE is 0");
_Static_assert(sizeof(OGRSpatialReferenceH) == sizeof(void *),
               "OGRSpatialReferenceH is a pointer");
_Static_assert(sizeof(CPLErrorHandler) == sizeof(void (*)(void)),
               "CPLErrorHandler is a function pointer");
