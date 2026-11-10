// CPLHTTPSetOptions under a C name, for gis::http_exchange (src/web_access.rs).
//
// cpl_http.h declares CPLHTTPSetOptions after CPL_C_END, inside
// `#if defined(__cplusplus)`, so its symbol is C++'s and Rust cannot name it.
// This file is compiled against the header of the GDAL the crate links (by
// build.rs, with every native file), so the call is checked by the compiler;
// src/ffi/http.rs declares the function below. Nothing may unwind into Rust:
// every exception is caught and reported as 1.

#include <cpl_http.h>

extern "C" int katana_http_set_options(void *curl, const char *url, const char *const *options,
                                       void **headers) noexcept
{
    *headers = nullptr;
    try {
        *headers = CPLHTTPSetOptions(curl, url, options);
        return 0;
    } catch (...) {
        return 1;
    }
}
