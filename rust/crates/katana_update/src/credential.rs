//! The one secret the update system can hold: a read-only access token for a private release
//! repository, for people who update from one. While the default release repository is public no
//! token is needed and none is asked for (docs/update.md, "The access token").
//!
//! Rules, each tested:
//! - NEVER compiled into a binary.
//! - NEVER in a verb: verb lines are logged, so a token on one would be written to the log.
//!   `UPDATE TOKEN STATUS` prints present|absent only; `UPDATE TOKEN CLEAR` removes it; there is
//!   no verb that sets or shows it. The window's Settings > Updates page writes it through
//!   write() and shows only that one is stored.
//! - NEVER in a message, a log line or an error context: describe a token as "token" and nothing
//!   else. [`redact_token`] is what a log line that has to mention a header passes it through.
//! - Sent only over https to exactly api.github.com, never along a redirect (the feed's
//!   bearer token).
//!
//! Where it lives: the environment variable KATANA_UPDATE_TOKEN (read-only; the one source on
//! Linux and macOS, and an override everywhere) and, on Windows, the Credential Manager under the
//! target name "Katana/update-token" (CredReadW, CredWriteW, CredDeleteW), written by the
//! settings page. Precedence when both exist is the caller's (the feed: environment first).
//!
//! A token is a secret, so nothing here puts one anywhere but into the Result a caller asked for:
//! not into a message, a context, a panic or a log line. An error describes the token as "token"
//! and gives its LENGTH at most. The buffers this module owns are zeroed before they are freed
//! (a String a caller makes of the result is the interface's cost).
//!
//! What read() can return, for a caller that looks a token up quietly (the automatic check, the
//! `UPDATE TOKEN STATUS` verb): NotFound is the normal "none"; InvalidState is a value that is
//! there and cannot be used (a line break in it, too long, a space inside); on Windows the
//! Credential Manager adds Unsupported (not usable in this session) and Internal (a system
//! fault). Every one of them means "no token to send" to such a caller; whether to say so, and
//! whether an unusable variable falls back to the Credential Manager, is the caller's to decide.
//!
//! What write() can return (the Credential Manager's; the environment's is Unsupported, always):
//! InvalidArgument for a token that is empty, over 2560 bytes, not UTF-8, or that holds a control
//! character, a space or an invisible character (credential_detail::usable_token); Unsupported
//! when the Credential Manager cannot be used in this session; Internal for any other system
//! fault. clear() returns success when none was stored, else Unsupported or Internal.

use crate::credential_detail::usable_token;
use katana_core::error::make_error;
use katana_core::path_text::environment_variable;
use katana_core::text::trimmed;
use katana_core::{Error, ErrorCode, Result, Status};

pub const TOKEN_ENVIRONMENT_VARIABLE: &str = "KATANA_UPDATE_TOKEN";
pub const CREDENTIAL_TARGET: &str = "Katana/update-token";

pub trait CredentialStore {
    /// The stored token, without the white space and the invisible characters at its ends (ASCII
    /// blanks, a no-break or zero-width space, a direction mark, a byte-order mark: what a token
    /// copied from a web page or a chat arrives with). Never logs the value.
    ///
    /// The errors, which a caller must tell apart:
    /// - NotFound: there is none: unset, empty or only ASCII blanks. "Absent" is the normal
    ///   state, so it is not an error to report loudly.
    /// - InvalidState: there is something, and it is not a usable token (a control character, an
    ///   inner space such as a pasted "Bearer " prefix, or an invisible character inside it, over
    ///   2560 bytes, not UTF-8; a value of only invisible characters too). The message names the
    ///   place, never the value, so a person can correct it.
    /// - Unsupported: Windows only: the Credential Manager cannot be used in this session (a
    ///   service or batch logon with no profile, a stripped image).
    /// - Internal: Windows only: any other Credential Manager failure; the Windows error number
    ///   is in the message.
    ///
    /// A caller that has more than one source (the feed: environment first, then this) treats
    /// EVERY error other than NotFound as "no token from this source", says so once, and goes on
    /// to the next source: a bad KATANA_UPDATE_TOKEN must not hide a valid token in the
    /// Credential Manager, and a broken Credential Manager must not stop an update from a public
    /// repository.
    fn read(&self) -> Result<String>;

    /// Stores the token (replacing one). The same token rule as read(): InvalidArgument for an
    /// empty, over-long, control-character, inner-space, invisible-character or non-UTF-8 token.
    /// Unsupported where the store is read-only (the environment), and for an unusable Credential
    /// Manager; Internal for its other failures. Bytes, as the C++'s string_view is: a token that
    /// is not UTF-8 is a refusal the caller can be given.
    fn write(&self, token: &[u8]) -> Status;

    /// Removes it; success when none was stored. Unsupported where read-only or where the
    /// Credential Manager cannot be used; Internal for its other failures.
    fn clear(&self) -> Status;

    /// "environment" or "credential manager": where it came from, for a sentence that names the
    /// place but never the value.
    fn r#where(&self) -> &str;

    /// The wide target name a Credential Manager store passes to the system, None for any other
    /// store: what credential_detail::credential_target_of reports (the C++ asks with a
    /// dynamic_cast).
    #[cfg(windows)]
    #[doc(hidden)]
    fn credential_manager_target(&self) -> Option<&[u16]> {
        None
    }
}

// Overwrites `bytes` with zeros in a way the compiler may not remove as a dead store: black_box
// must be assumed to read them.
pub(crate) fn secure_zero(bytes: &mut [u8]) {
    bytes.fill(0);
    let _ = std::hint::black_box(&*bytes);
}

struct EnvironmentCredentialStore;

impl CredentialStore for EnvironmentCredentialStore {
    fn read(&self) -> Result<String> {
        let name = TOKEN_ENVIRONMENT_VARIABLE;
        let mut raw = environment_variable(Some(name));
        if trimmed(&raw).is_empty() {
            // Unset, empty, or only white space: the normal state, not a fault.
            secure_zero(&mut raw);
            return Err(make_error(
                ErrorCode::NotFound,
                format!("no token: {name} is not set"),
                name,
            ));
        }
        let usable = usable_token(trimmed(&raw), name).map(String::from);
        secure_zero(&mut raw);
        // The same error with a sentence that says where the value is: the caller shows it to a
        // person who must know which variable to correct. The text of the value is never in it.
        usable.map_err(|error| Error {
            code: ErrorCode::InvalidState,
            message: format!("{name} is set but is not a usable token: {}", error.message),
            context: error.context,
        })
    }

    fn write(&self, _token: &[u8]) -> Status {
        Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "the token is read from the environment variable {TOKEN_ENVIRONMENT_VARIABLE}, \
                 which Katana does not set: set it in the environment that starts Katana"
            ),
            "environment",
        ))
    }

    fn clear(&self) -> Status {
        Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "the token is read from the environment variable {TOKEN_ENVIRONMENT_VARIABLE}, \
                 which Katana does not change: remove it from the environment that starts Katana"
            ),
            "environment",
        ))
    }

    fn r#where(&self) -> &str {
        "environment"
    }
}

/// KATANA_UPDATE_TOKEN, read through katana_core's environment_variable. Everywhere.
pub fn make_environment_credential_store() -> Box<dyn CredentialStore> {
    Box::new(EnvironmentCredentialStore)
}

/// Windows: the Credential Manager. Elsewhere: the environment store (there is no keyring in this
/// version).
pub fn make_system_credential_store() -> Box<dyn CredentialStore> {
    #[cfg(windows)]
    {
        crate::credential_win::make_credential_manager_store(CREDENTIAL_TARGET)
    }
    #[cfg(not(windows))]
    {
        make_environment_credential_store()
    }
}

/// `text` with every occurrence of `token` replaced by "***" (and a non-empty token shorter than 4
/// characters still replaced). For the one place a log line must quote a header or a URL.
pub fn redact_token(text: &str, token: &str) -> String {
    if token.is_empty() {
        return text.to_owned();
    }
    const MASK: &str = "***";
    let haystack = text.as_bytes();
    let needle = token.as_bytes();
    let find = |from: usize| -> Option<usize> {
        haystack
            .get(from..)?
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|at| at + from)
    };
    // Every occurrence is covered, and occurrences that overlap are covered together: with the
    // token "aa", "aaa" holds two, and masking only the first would leave the third "a" - part of
    // the token - in the line. Occurrences that merely touch ("tokentoken") are two masks.
    let mut covered: Vec<(usize, usize)> = Vec::new(); // [begin, end) of each mask
    let mut from = 0;
    while let Some(hit) = find(from) {
        let count = covered.len();
        if count > 0 && hit < covered[count - 1].1 {
            covered[count - 1].1 = hit + needle.len();
        } else {
            covered.push((hit, hit + needle.len()));
        }
        from = hit + 1;
    }
    // A match of a whole UTF-8 token in UTF-8 text begins and ends on character boundaries, so
    // every slice below is one.
    let mut redacted = String::with_capacity(text.len());
    let mut copied = 0; // text before this has been written out
    for (begin, end) in covered {
        redacted.push_str(&text[copied..begin]);
        redacted.push_str(MASK);
        copied = end;
    }
    redacted.push_str(&text[copied..]);
    redacted
}
