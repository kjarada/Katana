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

#![allow(
    unsafe_code,
    reason = "The Credential Manager is reached through its Win32 calls, which are foreign functions."
)]

use crate::credential::{CredentialStore, secure_zero};
use crate::credential_detail::usable_token;
use katana_core::error::make_error;
use katana_core::text::trimmed;
use katana_core::text_encoding::{TextEncoding, decode_text_as, is_valid_utf8};
use katana_core::{Error, ErrorCode, Result, Status};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::Security::Credentials::{
    CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
};

// The Win32 error numbers, from Microsoft's "System Error Codes" list.
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NOT_SUPPORTED: u32 = 50;
const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;
const ERROR_NOT_FOUND: u32 = 1168;
const ERROR_NO_SUCH_LOGON_SESSION: u32 = 1312;
const ERROR_NO_SUCH_DOMAIN: u32 = 1355;
// wincred.h.
const CRED_TYPE_GENERIC: u32 = 1;
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

fn last_error() -> u32 {
    // SAFETY: GetLastError only reads this thread's last-error value.
    unsafe { GetLastError() }
}

// The three system calls a store makes, so that a test can have them fail
// (make_credential_manager_store_failing_with). Each says whether the Win32 call succeeded and
// leaves its error for GetLastError.
type ReadCall = Box<dyn Fn(*const u16, *mut *mut CREDENTIALW) -> bool>;
type WriteCall = Box<dyn Fn(*const CREDENTIALW) -> bool>;
type RemoveCall = Box<dyn Fn(*const u16) -> bool>;

struct SystemCalls {
    read: ReadCall,
    write: WriteCall,
    remove: RemoveCall,
}

fn real_calls() -> SystemCalls {
    SystemCalls {
        read: Box::new(|target: *const u16, found: *mut *mut CREDENTIALW| {
            // SAFETY: `target` is the store's NUL-terminated wide name and `found` a place for
            // the pointer the system allocates.
            unsafe { CredReadW(target, CRED_TYPE_GENERIC, 0, found) != 0 }
        }),
        write: Box::new(|credential: *const CREDENTIALW| {
            // SAFETY: `credential` points at a filled CREDENTIALW whose strings and blob outlive
            // the call, which copies them.
            unsafe { CredWriteW(credential, 0) != 0 }
        }),
        remove: Box::new(|target: *const u16| {
            // SAFETY: `target` is the store's NUL-terminated wide name.
            unsafe { CredDeleteW(target, CRED_TYPE_GENERIC, 0) != 0 }
        }),
    }
}

fn calls_failing_with(code: u32) -> SystemCalls {
    let fail = move || {
        // SAFETY: SetLastError only sets this thread's last-error value.
        unsafe { SetLastError(code) };
        false
    };
    SystemCalls {
        read: Box::new(move |_: *const u16, _: *mut *mut CREDENTIALW| fail()),
        write: Box::new(move |_: *const CREDENTIALW| fail()),
        remove: Box::new(move |_: *const u16| fail()),
    }
}

// What CredReadW allocated: its blob zeroed, then freed with CredFree.
struct Found(*mut CREDENTIALW);

impl Found {
    fn blob(&self) -> &[u8] {
        // SAFETY: CredReadW succeeded, so the pointer is to a CREDENTIALW the system allocated,
        // whose blob is CredentialBlobSize bytes; both live until CredFree in drop.
        unsafe {
            let credential = &*self.0;
            if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
            }
        }
    }
}

impl Drop for Found {
    fn drop(&mut self) {
        // SAFETY: as in blob(); the blob is zeroed in place before the system frees it, and the
        // pointer is not used again.
        unsafe {
            let credential = &mut *self.0;
            if !credential.CredentialBlob.is_null() {
                std::ptr::write_bytes(
                    credential.CredentialBlob,
                    0,
                    credential.CredentialBlobSize as usize,
                );
            }
            CredFree(self.0 as *const core::ffi::c_void);
        }
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
    target_terminated: Vec<u16>,
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
        let mut target_terminated = wide.clone();
        target_terminated.push(0);
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
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        if !(self.calls.read)(self.target_terminated.as_ptr(), &raw mut found) {
            let code = last_error();
            if code == ERROR_NOT_FOUND {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "no token is stored",
                    self.target_utf8.as_str(),
                ));
            }
            return Err(credential_error(code, "read", &self.target_utf8));
        }
        let credential = Found(found);
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
        // is what is zeroed after.
        let mut blob: Vec<u8> = usable.as_bytes().to_vec();
        let mut target = self.target_terminated.clone();
        let mut comment: Vec<u16> = "Katana: read-only access token for update downloads"
            .encode_utf16()
            .chain([0])
            .collect();
        let mut user: Vec<u16> = "Katana".encode_utf16().chain([0]).collect();
        // SAFETY: CREDENTIALW is plain data, and all zero is a credential with no field set.
        let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target.as_mut_ptr();
        credential.Comment = comment.as_mut_ptr();
        // At most MAX_TOKEN_BYTES (usable_token).
        credential.CredentialBlobSize = blob.len() as u32;
        credential.CredentialBlob = blob.as_mut_ptr();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = user.as_mut_ptr();
        let stored = (self.calls.write)(&raw const credential);
        let code = if stored { 0 } else { last_error() };
        secure_zero(&mut blob);
        if !stored {
            return Err(credential_error(code, "store", &self.target_utf8));
        }
        Ok(())
    }

    fn clear(&self) -> Status {
        if !(self.calls.remove)(self.target_terminated.as_ptr()) {
            let code = last_error();
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
