//! The online request builders, web access and discovery (step 5, wave L,
//! agent L2), one module per C++ test file:
//!
//! ```text
//! online_requests  tests/interop/test_online_requests.cpp  every case
//! online_fetch     tests/interop/test_online_fetch.cpp     OnlineDiscovery; two OnlineLive cases
//! ```
//!
//! Each gtest suite is a module inside its file's module, named as
//! tools/check_rust_parity.py expects. test_online_fetch.cpp's OnlineFetch
//! cases and its Copernicus live case are wave M's (online_fetch); its live
//! case that sets GDAL_HTTP_USERPWD is in tests/online_environment, a binary of
//! its own, since libtest runs these cases on threads of one process.
//!
//! No case here reaches the network but the OnlineLive ones, which run only
//! with KATANA_ONLINE_TESTS=1, as the C++ ones do.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]
#![allow(
    clippy::excessive_precision,
    reason = "Expected values are copied digit for digit from the C++ tests."
)]

#[path = "../support/online_support.rs"]
mod online_support;

mod online_fetch;
mod online_requests;
