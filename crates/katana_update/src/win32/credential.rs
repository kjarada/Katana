//! The Credential Manager calls of the update layer (credential_win): read, write and delete of
//! one generic credential, each returning the system's error, read at once, when it fails.

use std::ptr::NonNull;

use windows_sys::Win32::Security::Credentials::{
    CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
};

use super::{WideName, last_error, length_u32};

// wincred.h.
const CRED_TYPE_GENERIC: u32 = 1;
// winerror.h.
const ERROR_INVALID_DATA: u32 = 13;

/// What CredReadW allocated: read through `blob`, its blob zeroed in place and the whole freed
/// with CredFree, once, when dropped.
#[derive(Debug)]
pub(crate) struct Credential(NonNull<CREDENTIALW>);

impl Credential {
    /// The credential's blob; empty when it has none.
    pub(crate) fn blob(&self) -> &[u8] {
        // SAFETY: CredReadW succeeded, so the pointer is to a CREDENTIALW the system allocated,
        // whose blob is CredentialBlobSize bytes; both live until CredFree in drop, and the
        // slice borrows self.
        unsafe {
            let credential = self.0.as_ref();
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

impl Drop for Credential {
    fn drop(&mut self) {
        // SAFETY: as in blob(); no slice of it outlives this borrow of self; the blob is zeroed
        // in place before the system frees it, and the pointer is not used again.
        unsafe {
            let credential = self.0.as_mut();
            if !credential.CredentialBlob.is_null() {
                std::ptr::write_bytes(
                    credential.CredentialBlob,
                    0,
                    credential.CredentialBlobSize as usize,
                );
            }
            CredFree(self.0.as_ptr().cast_const().cast());
        }
    }
}

/// CredReadW of the generic credential `target`: it, or the system's error.
pub(crate) fn read_generic(target: &WideName) -> Result<Credential, u32> {
    let mut found: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `target` is NUL-terminated and `found` a place for the pointer the system
    // allocates, owned by the Credential made of it.
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut found) } == 0 {
        return Err(last_error());
    }
    // A success with no credential is not one the system gives; read as "not found" would hide
    // it, so it is a fault of the data (Internal, by credential_error's rule).
    NonNull::new(found)
        .map(Credential)
        .ok_or(ERROR_INVALID_DATA)
}

/// What write_generic stores: the strings are copied for the call, the blob is the caller's.
pub(crate) struct NewCredential<'a> {
    pub(crate) target: &'a WideName,
    pub(crate) comment: &'a WideName,
    pub(crate) user: &'a WideName,
    pub(crate) persist: u32,
    pub(crate) blob: &'a mut [u8],
}

/// CredWriteW of a generic credential: Ok, or the system's error.
pub(crate) fn write_generic(new: NewCredential<'_>) -> Result<(), u32> {
    // CREDENTIALW takes mutable pointers; the call copies what they point at, so these copies
    // need live only for it.
    let mut target = new.target.clone();
    let mut comment = new.comment.clone();
    let mut user = new.user.clone();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        Comment: comment.as_mut_ptr(),
        CredentialBlobSize: length_u32(new.blob.len()),
        CredentialBlob: new.blob.as_mut_ptr(),
        Persist: new.persist,
        UserName: user.as_mut_ptr(),
        ..CREDENTIALW::default()
    };
    // SAFETY: `credential` is a filled CREDENTIALW whose strings (NUL-terminated WideNames) and
    // blob (CredentialBlobSize bytes at most) outlive the call, which copies them.
    if unsafe { CredWriteW(&raw const credential, 0) } == 0 {
        return Err(last_error());
    }
    Ok(())
}

/// CredDeleteW of the generic credential `target`: Ok, or the system's error.
pub(crate) fn delete_generic(target: &WideName) -> Result<(), u32> {
    // SAFETY: `target` is NUL-terminated.
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        return Err(last_error());
    }
    Ok(())
}
