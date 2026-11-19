//! The Windows Credential Manager store of the credential module. Compiled on Windows only.
//!
//! One generic credential (CRED_TYPE_GENERIC) under the target name "Katana/update-token",
//! persisted for the machine (CRED_PERSIST_LOCAL_MACHINE: it survives log-off and reboot, and is
//! not roamed to other machines, as CRED_PERSIST_ENTERPRISE would). The token is the blob, as
//! UTF-8 bytes. Read also takes a blob that is UTF-16LE text, because that is what `cmdkey
//! /generic:Katana/update-token /user:x /pass:<token>` writes (measured on Windows 11,
//! 2026-10-08: the password "abc" is stored as 61 00 62 00 63 00). UTF-8 text has no NUL inside
//! it, so a blob that has one inside is taken to be UTF-16 (blob_text has the limits of that
//! guess).
//!
//! The token is never logged and never in an error. The blob this file builds and the one the
//! system hands back are zeroed before they are released.
//!
//! read() can fail in four ways, which credential.rs lists for the callers that look a token up
//! quietly: NotFound (none stored), InvalidState (stored, cannot be used), Unsupported (the
//! Credential Manager cannot be used in this session) and Internal (any other system fault).
//!
//! The calls themselves are win32::credential's safe wrappers (docs/rust.md, "Unsafe confined to
//! FFI modules"), which zero and free what the system allocated.

use crate::credential::{CredentialStore, secure_zero};
use crate::credential_detail::usable_token;
use crate::win32::WideName;
use crate::win32::credential::{
    Credential, NewCredential, delete_generic, read_generic, write_generic,
};
use katana_core::error::make_error;
use katana_core::text::trimmed;
use katana_core::text_encoding::{TextEncoding, decode_text_as, is_valid_utf8};
use katana_core::{Error, ErrorCode, Result, Status};

// The Win32 error numbers, from Microsoft's "System Error Codes" list.
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NOT_SUPPORTED: u32 = 50;
const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;
const ERROR_NOT_FOUND: u32 = 1168;
const ERROR_NO_SUCH_LOGON_SESSION: u32 = 1312;
const ERROR_NO_SUCH_DOMAIN: u32 = 1355;
// wincred.h.
const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
const CRED_MAX_GENERIC_TARGET_NAME_LENGTH: usize = 32767;

// The codes that mean "the Credential Manager cannot be used in this session", as opposed to a
// fault in what was asked: a service or batch logon with no profile loaded, a locked-down or
// stripped Windows image.
fn is_unavailable(code: u32) -> bool {
    matches!(
        code,
        ERROR_ACCESS_DENIED
            | ERROR_NOT_SUPPORTED
            | ERROR_CALL_NOT_IMPLEMENTED
            | ERROR_NO_SUCH_LOGON_SESSION
            | ERROR_NO_SUCH_DOMAIN
    )
}

// The three system calls a store makes, so that a test can have them fail
// (make_credential_manager_store_failing_with). Each returns the Win32 call's error, read at once,
// when it fails.
type ReadCall = Box<dyn Fn(&WideName) -> std::result::Result<Credential, u32>>;
// The write is given the target and the blob; the rest of what is stored is the store's own.
type WriteCall = Box<dyn Fn(&WideName, &mut [u8]) -> std::result::Result<(), u32>>;
type RemoveCall = Box<dyn Fn(&WideName) -> std::result::Result<(), u32>>;

struct SystemCalls {
    read: ReadCall,
    write: WriteCall,
    remove: RemoveCall,
}

fn real_calls() -> SystemCalls {
    SystemCalls {
        read: Box::new(read_generic),
        write: Box::new(|target: &WideName, blob: &mut [u8]| {
            write_generic(NewCredential {
                target,
                comment: &WideName::from_text(
                    "Katana: read-only access token for update downloads",
                ),
                user: &WideName::from_text("Katana"),
                persist: CRED_PERSIST_LOCAL_MACHINE,
                blob,
            })
        }),
        remove: Box::new(delete_generic),
    }
}

fn calls_failing_with(code: u32) -> SystemCalls {
    SystemCalls {
        read: Box::new(move |_: &WideName| -> std::result::Result<Credential, u32> { Err(code) }),
        write: Box::new(move |_: &WideName, _: &mut [u8]| -> std::result::Result<(), u32> {
            Err(code)
        }),
        remove: Box::new(move |_: &WideName| -> std::result::Result<(), u32> { Err(code) }),
    }
}

// The text a blob holds, still unchecked. UTF-8 is what write() stores. A NUL is no part of UTF-8
// text, so a blob that has one inside it is taken to be UTF-16LE (cmdkey's form) and is decoded;
// NULs at the END only are the terminator of a C string and are dropped. The guess has two known
// limits, both of blobs that no tool is known to write, and both left alone because the only rule
// that would tell them apart is one that refuses real UTF-16 tokens:
//   - UTF-16 text in which no code unit has a zero byte (two CJK characters, U+65E5 U+672C, are
//     E5 65 2C 67) shows no NUL, is read as UTF-8, which it is not, and is refused as
//     InvalidState.
//   - A UTF-8 blob with a NUL inside it (write() refuses a NUL, so it is another program's) and an
//     even length is read as UTF-16, and gives a token of other characters: 61 62 63 00 64 65 is
//     U+6261 U+0063 U+6564. The server then answers 401, as it does to any wrong token. (An odd
//     length is refused here: UTF-16 is whole code units.) What tells it from real UTF-16 text
//     such as 'a', U+00E9, U+65E5, which a case holds, is a script rule.
// A GitHub token is ASCII, whose UTF-16 always has NULs, so the guess is not worth more.
fn blob_text(blob: &[u8]) -> Result<Vec<u8>> {
    let mut without_terminator = blob;
    while let [rest @ .., 0] = without_terminator {
        without_terminator = rest;
    }
    if !without_terminator.contains(&0) {
        // No NUL inside: UTF-8 (and a one-character UTF-16 token, "a" as 61 00, which is "a" read
        // either way).
        return Ok(without_terminator.to_vec());
    }
    // UTF-16LE: whole code units only, and a terminator some tools append is not part of the
    // token.
    let mut units = blob;
    while let [rest @ .., 0, 0] = units {
        units = rest;
    }
    if !units.len().is_multiple_of(2) {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "the stored token is neither UTF-8 nor UTF-16 text",
        ));
    }
    match decode_text_as(units, TextEncoding::Utf16LittleEndian) {
        Ok(decoded) => Ok(decoded.text.into_bytes()),
        Err(_) => Err(Error::new(
            ErrorCode::InvalidState,
            "the stored token is not well-formed UTF-16 text",
        )),
    }
}

struct CredentialManagerStore {
    calls: SystemCalls,
    target_utf8: String,
    // The name passed to the system (credential_target_of), and the same with its terminator.
    target: Vec<u16>,
    target_terminated: WideName,
}

impl CredentialManagerStore {
    // `target` is UTF-8, not empty, at most 32767 characters (CRED_MAX_GENERIC_TARGET_NAME_LENGTH
    // in wincred.h): a precondition, a panic when broken, as the C++ throws
    // std::invalid_argument. A credential target is a name, not a path, so it is converted
    // directly.
    fn new(target: &[u8], calls: SystemCalls) -> Self {
        const REFUSAL: &str = "a credential target is non-empty UTF-8 of at most 32767 characters";
        let text = match std::str::from_utf8(target) {
            Ok(text) if !text.is_empty() && is_valid_utf8(target) => text,
            _ => panic!("{REFUSAL}"),
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        if wide.len() > CRED_MAX_GENERIC_TARGET_NAME_LENGTH {
            panic!("{REFUSAL}");
        }
        let target_terminated = WideName::from_units(wide.clone());
        Self {
            calls,
            target_utf8: text.to_owned(),
            target: wide,
            target_terminated,
        }
    }
}

impl CredentialStore for CredentialManagerStore {
    fn read(&self) -> Result<String> {
        let credential = match (self.calls.read)(&self.target_terminated) {
            Ok(credential) => credential,
            Err(code) => {
                if code == ERROR_NOT_FOUND {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "no token is stored",
                        self.target_utf8.as_str(),
                    ));
                }
                return Err(credential_error(code, "read", &self.target_utf8));
            }
        };
        let blob = credential.blob();
        if blob.is_empty() {
            return Err(make_error(
                ErrorCode::NotFound,
                "no token is stored: the credential is empty",
                self.target_utf8.as_str(),
            ));
        }
        let mut text = match blob_text(blob) {
            Ok(text) => text,
            Err(mut error) => {
                error.context = self.target_utf8.clone();
                return Err(error);
            }
        };
        if trimmed(&text).is_empty() {
            secure_zero(&mut text);
            return Err(make_error(
                ErrorCode::NotFound,
                "no token is stored: the credential is empty",
                self.target_utf8.as_str(),
            ));
        }
        let usable = usable_token(trimmed(&text), &self.target_utf8).map(String::from);
        secure_zero(&mut text);
        usable.map_err(|error| Error {
            code: ErrorCode::InvalidState,
            message: format!("the stored token is not usable: {}", error.message),
            context: error.context,
        })
    }

    fn write(&self, token: &[u8]) -> Status {
        let usable = usable_token(trimmed(token), &self.target_utf8)?;
        // CredWriteW takes mutable pointers and copies what they point at; this copy of the token
        // (at most MAX_TOKEN_BYTES, usable_token) is what is zeroed after.
        let mut blob: Vec<u8> = usable.as_bytes().to_vec();
        let stored = (self.calls.write)(&self.target_terminated, blob.as_mut_slice());
        secure_zero(&mut blob);
        if let Err(code) = stored {
            return Err(credential_error(code, "store", &self.target_utf8));
        }
        Ok(())
    }

    fn clear(&self) -> Status {
        if let Err(code) = (self.calls.remove)(&self.target_terminated) {
            if code == ERROR_NOT_FOUND {
                return Ok(()); // nothing was stored: the state asked for
            }
            return Err(credential_error(code, "remove", &self.target_utf8));
        }
        Ok(())
    }

    fn r#where(&self) -> &str {
        "credential manager"
    }

    fn credential_manager_target(&self) -> Option<&[u16]> {
        Some(self.target.as_slice())
    }
}

/// The same store under another target name, so that a test never touches the one a person's
/// token is in. `target` is UTF-8, not empty, at most 32767 characters - a precondition, a panic
/// when broken.
pub fn make_credential_manager_store(target: impl AsRef<[u8]>) -> Box<dyn CredentialStore> {
    Box::new(CredentialManagerStore::new(target.as_ref(), real_calls()))
}

/// The same store with the three system calls (CredReadW, CredWriteW, CredDeleteW) replaced by
/// calls that fail with the Win32 error `code`, nothing stored or removed. A test cannot make a
/// session without a Credential Manager, and with this it holds what read(), write() and clear()
/// each make of a failure: the action they name and the kind of error. Same precondition on
/// `target`.
pub fn make_credential_manager_store_failing_with(
    target: impl AsRef<[u8]>,
    code: u32,
) -> Box<dyn CredentialStore> {
    Box::new(CredentialManagerStore::new(
        target.as_ref(),
        calls_failing_with(code),
    ))
}

/// What the Win32 error `code` of a Credential Manager call means to a caller, as an Error that
/// names `action` ("read", "store", "remove") and carries `target` as its context. The codes that
/// mean "the Credential Manager cannot be used in this session" are Unsupported (5
/// ERROR_ACCESS_DENIED, 50 ERROR_NOT_SUPPORTED, 120 ERROR_CALL_NOT_IMPLEMENTED, 1312
/// ERROR_NO_SUCH_LOGON_SESSION, 1355 ERROR_NO_SUCH_DOMAIN); every other code is Internal. The
/// number is in the message, so access denied is not hidden by being called "not available".
pub fn credential_error(code: u32, action: &str, target: &str) -> Error {
    if is_unavailable(code) {
        return make_error(
            ErrorCode::Unsupported,
            format!(
                "the Windows Credential Manager is not available in this session (Windows error \
                 {code})"
            ),
            target,
        );
    }
    make_error(
        ErrorCode::Internal,
        format!(
            "the Windows Credential Manager could not {action} the token (Windows error {code})"
        ),
        target,
    )
}

/// The wide target name `store` passes to the system, or empty when `store` is not a Credential
/// Manager store. A seam for a test, which checks the name a person's `cmdkey` command has to use
/// without reading or writing the credential under it.
pub fn credential_target_of(store: &dyn CredentialStore) -> Vec<u16> {
    store
        .credential_manager_target()
        .map(<[u16]>::to_vec)
        .unwrap_or_default()
}
