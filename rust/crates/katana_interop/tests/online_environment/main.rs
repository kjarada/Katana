//! The one online case that changes what the process shares (step 5, agent
//! L2): tests/interop/test_online_fetch.cpp's
//! OnlineLive.AnExchangeSendsNoCredentialAPersonConfiguredForGdal, which sets
//! GDAL_HTTP_USERPWD in the environment.
//!
//! gtest ran each case in a process of its own; Rust runs a binary's cases on
//! parallel threads of one, and GDAL reads its options from the environment.
//! So the case is in a binary of its own, holds ENVIRONMENT for its whole
//! length, and puts the variable back before it lets go. It reaches the
//! network, and runs only with KATANA_ONLINE_TESTS=1, as the C++ one does.

#![allow(
    unsafe_code,
    reason = "Edition 2024 makes std::env::set_var and remove_var unsafe: they race a thread that \
              reads the environment, which the case here rules out by holding ENVIRONMENT."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/online_support.rs"]
mod online_support;

use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// What the process shares - its environment - is changed only while this is
/// held. A case that panics while holding it poisons it; the next takes it all
/// the same, since what the panicking case set was put back as it unwound.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// ENVIRONMENT, held until the returned guard is dropped: at the start of
/// every case here.
fn environment() -> MutexGuard<'static, ()> {
    ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sets `name` to `value`, or removes it for None. The caller holds
/// ENVIRONMENT.
fn set_variable(name: &str, value: Option<&OsStr>) {
    match value {
        // SAFETY: every case of this binary holds ENVIRONMENT while it reads or
        // changes the environment, and none starts a thread of its own while it
        // does, so no other thread of the binary reads it while it changes.
        Some(value) => unsafe { std::env::set_var(name, value) },
        // SAFETY: as above.
        None => unsafe { std::env::remove_var(name) },
    }
}

/// One variable of this process's environment, for a scope: GDAL reads its
/// options from the environment when nothing else set them. Put back as it
/// was, set or not, when it is dropped. Made only by a case holding
/// ENVIRONMENT, which it is handed as proof.
pub struct ScopedEnvironmentVariable {
    name: &'static str,
    before: Option<OsString>,
}

impl ScopedEnvironmentVariable {
    pub fn new(_held: &MutexGuard<'static, ()>, name: &'static str, value: &str) -> Self {
        let before = std::env::var_os(name);
        set_variable(name, Some(OsStr::new(value)));
        Self { name, before }
    }
}

impl Drop for ScopedEnvironmentVariable {
    fn drop(&mut self) {
        set_variable(self.name, self.before.as_deref());
    }
}

mod online_fetch {
    mod online_live {
        use katana_gis::web_access::{HttpRequest, http_exchange};

        use crate::online_support::{
            LIVE_SKIP, default_user_agent, holds, live_tests_enabled, must,
        };
        use crate::{ScopedEnvironmentVariable, environment};

        #[test]
        fn an_exchange_sends_no_credential_a_person_configured_for_gdal() {
            let held = environment();
            if !live_tests_enabled() {
                eprintln!("{LIVE_SKIP}");
                return;
            }
            // GDAL_HTTP_USERPWD is a person's setting for a map service. GDAL
            // puts it on every libcurl handle it configures, and libcurl then
            // sends it to whichever host is asked, as `Authorization: Basic`.
            // http_exchange is configured by GDAL and must send none of it
            // (web_access): its caller decides hop by hop what each host is
            // told. api.github.com tells the two apart by itself - a request
            // with credentials it does not know is answered 401 "Bad
            // credentials" (GitHub's REST documentation, "Failed login limit"),
            // the same request with none is answered. A rate limit (403) is
            // neither, so the test asks only that the answer is not the one
            // credentials get.
            let _configured = ScopedEnvironmentVariable::new(
                &held,
                "GDAL_HTTP_USERPWD",
                "katana-test-user:not-a-password",
            );
            let request = HttpRequest {
                url: String::from("https://api.github.com/zen"),
                user_agent: default_user_agent(),
                max_bytes: 1 << 16,
                retries: 1,
                ..HttpRequest::default()
            };
            let answer = must(http_exchange(&request, None, None));
            assert_ne!(
                answer.status, 401,
                "the credentials of GDAL_HTTP_USERPWD went with the request"
            );
            assert!(
                !holds(&answer.body, "Bad credentials"),
                "{}",
                String::from_utf8_lossy(&answer.body)
            );
        }
    }
}
