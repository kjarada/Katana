//! GDAL's drivers, registered once per process (src/katana_io/gdal_registry.hpp,
//! implemented in gdal_adapter.cpp as `ensureRegistered`). Every module of the
//! crate that calls GDAL goes through `ensure_registered` rather than calling
//! GDALAllRegister itself: a second registration path would be a second place
//! GDAL_DATA has to be found.
//!
//! GDAL's driver manager is process-global. Registering it once, lazily, is
//! the whole of the lifetime management: it is deliberately NEVER destroyed.
//! GDALDestroyDriverManager() tears down state shared by every open dataset
//! in the process, so calling it when one dataset closes - as the C++ adapter
//! once did - invalidates every other dataset and every later open
//! (GdalAdapter.SeveralDatasetsCanBeOpenAtOnce). The driver manager is freed
//! by process exit, which is correct and costs nothing.
//!
//! The configuration set here (GDAL_DATA, CURL_CA_BUNDLE and, on Windows,
//! GDAL_HTTP_USE_CAPI_STORE) is GDAL's global configuration, set once; every
//! other option the crate sets is thread-local and put back.

#![allow(
    unsafe_code,
    reason = "Registers GDAL's drivers and sets its global configuration; every call states why it is sound."
)]

use std::ffi::CStr;
use std::path::Path;
use std::sync::Once;

use katana_core::library_data::data_beside_library;

use crate::ffi::{cpl, gdal};

pub(crate) fn ensure_registered() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        locate_gdal_data();
        locate_certificates();
        // GDAL's PROJ pointed at its data before the drivers are registered,
        // where the C++ calls pointGdalAtProjData; the helper is
        // pointcloud's, which gis may see, so there is one once-flag for it.
        katana_pointcloud::proj_search_paths::point_gdal_at_proj_data();
        // SAFETY: registers every driver; GDAL guards its driver manager.
        unsafe { gdal::GDALAllRegister() };
        // Keep GDAL's chatter off stderr; failures are reported through
        // Result with CPLGetLastErrorMsg() as context instead.
        // SAFETY: installs GDAL's own quiet handler as the global one.
        unsafe { cpl::CPLSetErrorHandler(Some(cpl::CPLQuietErrorHandler)) };
    });
}

/// Whether GDAL's configuration has `key`, from the environment or set.
fn configured(key: &CStr) -> bool {
    // SAFETY: a NUL-terminated key and a NULL default; the answer is only
    // tested, never read.
    !unsafe { cpl::CPLGetConfigOption(key.as_ptr(), std::ptr::null()) }.is_null()
}

fn set_configuration(key: &CStr, value: &[u8]) {
    let value = cpl::c_string(value);
    // SAFETY: NUL-terminated key and value; GDAL copies both.
    unsafe { cpl::CPLSetConfigOption(key.as_ptr(), value.as_ptr()) };
}

/// Tells GDAL where its support files are, when nothing else has.
///
/// GDAL reads data files at run time - header.dxf and trailer.dxf, the
/// templates every DXF it writes is built from, among others - and finds them
/// through GDAL_DATA or a directory compiled in when GDAL was built. On MSYS2
/// that compiled-in path is useless (the package is relocatable), and only an
/// MSYS2 LOGIN SHELL sets GDAL_DATA. Started any other way GDAL found
/// nothing, and DXF export failed with "failed to find template header file
/// header.dxf".
///
/// The fix is the one PROJ uses for proj.db: look RELATIVE TO THE LIBRARY.
/// The GDAL library sits in <prefix>/bin (Windows) or <prefix>/lib and its
/// data in <prefix>/share/gdal, for the toolchain and for a bundle alike. A
/// GDAL_DATA the user has set is respected: an explicit choice outranks a
/// default.
#[cfg(windows)]
fn locate_gdal_data() {
    if configured(c"GDAL_DATA") {
        return;
    }
    // Found by NAME among the loaded modules (katana_core::library_data has
    // why not by an address): libgdal-38.dll from MinGW, gdal.dll or
    // gdal308.dll from MSVC.
    let data = data_beside_library("libgdal", Path::new("share/gdal"))
        .or_else(|| data_beside_library("gdal", Path::new("share/gdal")));
    if let Some(data) = data {
        // GDAL takes UTF-8 (GDAL_FILENAME_IS_UTF8 defaults to YES), and
        // forward slashes on every platform.
        set_configuration(c"GDAL_DATA", &cpl::generic_path(&data));
    }
}

/// Linux and macOS: a relocated prefix - the Linux and macOS bundles - keeps
/// libgdal in lib/ and its data in share/gdal, while the path compiled into
/// GDAL names the toolchain prefix on the machine that built it.
#[cfg(not(windows))]
fn locate_gdal_data() {
    if configured(c"GDAL_DATA") {
        return;
    }
    #[cfg(target_os = "macos")]
    let library = "libgdal."; // libgdal.38.dylib
    #[cfg(not(target_os = "macos"))]
    let library = "libgdal.so";
    if let Some(data) = data_beside_library(library, Path::new("share/gdal")) {
        set_configuration(c"GDAL_DATA", &cpl::native_path(&data));
    }
}

/// Tells libcurl, through GDAL, which certificates https:// is checked
/// against, when nothing else has. conda-forge's libcurl, which the Linux and
/// macOS builds use, looks at <its prefix>/ssl/cacert.pem, compiled in, so in
/// a relocated bundle it would find none and refuse every https:// answer.
/// The deploy copies the file to ssl/cacert.pem beside lib/ and this points
/// GDAL at it. A distribution's libcurl has no ssl/ beside its directory, so
/// its own default stands; CURL_CA_BUNDLE or SSL_CERT_FILE, set by the user,
/// win.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn locate_certificates() {
    if configured(c"CURL_CA_BUNDLE") || configured(c"SSL_CERT_FILE") {
        return;
    }
    #[cfg(target_os = "macos")]
    let library = "libcurl."; // libcurl.4.dylib
    #[cfg(not(target_os = "macos"))]
    let library = "libcurl.so";
    if let Some(bundle) = data_beside_library(library, Path::new("ssl/cacert.pem")) {
        set_configuration(c"CURL_CA_BUNDLE", &cpl::native_path(&bundle));
    }
}

/// WINDOWS does two things, both by GDAL's own settings so that GIS > Online
/// Data, GDAL's web drivers and the updater all get them:
///
/// 1. It names the bundle the deploy put beside bin/ (etc/ssl/certs/
///    ca-bundle.crt), in UTF-8. MSYS2's libcurl finds it by itself, but works
///    out where bin/ is with the ANSI API, so an install under a folder with a
///    character outside the ANSI code page is told a mangled path and refuses
///    every https:// answer. It is a config option set in this process, a
///    UTF-8 string, and not an environment variable: the environment's narrow
///    strings are ANSI and would mangle it again.
/// 2. It turns on GDAL_HTTP_USE_CAPI_STORE, which adds the certificates of the
///    Windows store to what OpenSSL trusts, ON TOP of the bundle: a company
///    proxy or an antivirus that inspects https:// re-signs with a root of its
///    own that it installs in the store. The person's own
///    GDAL_HTTP_USE_CAPI_STORE (NO included) wins, as a bundle they name does.
///
/// Rejected: CURLSSLOPT_NATIVE_CA on the handles of Katana's own, which would
/// not reach the requests GDAL makes itself.
#[cfg(windows)]
fn locate_certificates() {
    // A certificate file the person named wins, and is read WIDE: GDAL reads
    // CURL_CA_BUNDLE and SSL_CERT_FILE with getenv, which on Windows is the
    // ANSI code page's version, so each is handed over as the UTF-8 it is, as
    // a config option, which outranks the environment in GDAL and makes the
    // test just below see it.
    for name in [c"CURL_CA_BUNDLE", c"SSL_CERT_FILE"] {
        let value = katana_core::path_text::environment_variable(name.to_str().ok());
        if !value.is_empty() {
            set_configuration(name, &value);
        }
    }
    if !configured(c"CURL_CA_BUNDLE")
        && !configured(c"GDAL_CURL_CA_BUNDLE")
        && !configured(c"SSL_CERT_FILE")
    {
        // libcurl's own DLL, else GDAL's (the same bin/): the bundle is
        // beside whichever is loaded.
        let relative = Path::new("etc/ssl/certs/ca-bundle.crt");
        let bundle = data_beside_library("libcurl", relative)
            .or_else(|| data_beside_library("libgdal", relative));
        if let Some(bundle) = bundle {
            set_configuration(c"CURL_CA_BUNDLE", &cpl::generic_path(&bundle));
        }
    }
    if !configured(c"GDAL_HTTP_USE_CAPI_STORE") {
        set_configuration(c"GDAL_HTTP_USE_CAPI_STORE", b"YES");
    }
}

/// Neither Linux, macOS nor Windows: no rule for the certificates is written.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn locate_certificates() {}
