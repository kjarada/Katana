//! The one online case that changes what the process shares (step 5, agent
//! L2): tests/interop/test_online_fetch.cpp's
//! OnlineLive.AnExchangeSendsNoCredentialAPersonConfiguredForGdal, which sets
//! GDAL_HTTP_USERPWD in the environment.
//!
//! GDAL reads its options from the environment itself, so the variable must
//! be in the process's real environment, where Katana cannot override it. It
//! is never set in a running process (std::env::set_var is unsafe, since
//! another thread - GDAL's among them - may be reading the environment at that
//! moment): the case runs itself again as a child, the variable given to the
//! child when it is started (Command::env), and the child makes the exchange.
//! It is a binary of its own all the same, as the C++ case was a process of
//! its own. It reaches the network, and runs only with KATANA_ONLINE_TESTS=1,
//! as the C++ one does.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/online_support.rs"]
mod online_support;

mod online_fetch {
    mod online_live {
        use katana_gis::web_access::{HttpRequest, http_exchange};

        use crate::online_support::{
            LIVE_SKIP, default_user_agent, holds, live_tests_enabled, must,
        };

        // Set in the child's environment by the parent: make the exchange,
        // not the child.
        const CHILD: &str = "KATANA_ONLINE_ENVIRONMENT_CHILD";
        const VARIABLE: &str = "GDAL_HTTP_USERPWD";
        const CREDENTIALS: &str = "katana-test-user:not-a-password";

        #[test]
        fn an_exchange_sends_no_credential_a_person_configured_for_gdal() {
            if !live_tests_enabled() {
                eprintln!("{LIVE_SKIP}");
                return;
            }
            if std::env::var_os(CHILD).is_none() {
                // This case's name as libtest knows it: its module path less
                // the crate's (the target's) name.
                let path = module_path!().split_once("::").map_or("", |(_, rest)| rest);
                let name = format!(
                    "{path}::an_exchange_sends_no_credential_a_person_configured_for_gdal"
                );
                let child = std::process::Command::new(
                    std::env::current_exe().expect("this program"),
                )
                .args([name.as_str(), "--exact", "--nocapture", "--test-threads=1"])
                .env(CHILD, "1")
                .env(VARIABLE, CREDENTIALS)
                .output()
                .expect("the child runs");
                assert!(
                    child.status.success(),
                    "--- stdout ---\n{}\n--- stderr ---\n{}",
                    String::from_utf8_lossy(&child.stdout),
                    String::from_utf8_lossy(&child.stderr)
                );
                return;
            }
            // The child: the person's setting is in the environment GDAL reads.
            assert_eq!(
                std::env::var_os(VARIABLE).as_deref(),
                Some(std::ffi::OsStr::new(CREDENTIALS))
            );
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
