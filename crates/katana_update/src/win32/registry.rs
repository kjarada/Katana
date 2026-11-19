//! The registry calls of the update layer (registry_win), all under HKEY_CURRENT_USER: safe
//! wrappers that return the calls' own status (a Win32 error number, ERROR_SUCCESS being 0).

use std::ptr::{null, null_mut};

use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegFlushKey,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};

use super::{WideName, length_u32};

// winerror.h, winnt.h.
const ERROR_SUCCESS: u32 = 0;
const REG_SZ: u32 = 1;

/// An open key, closed once, when dropped.
#[derive(Debug)]
pub(crate) struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key is one this value opened (create_key and open_key make a Key only of a
        // call that succeeded), closed once, here.
        unsafe { RegCloseKey(self.0) };
    }
}

/// RegGetValueW of the default value (no name) of HKCU\`subkey`, restricted by `flags`, into
/// `buffer`: the status, and the bytes the call wrote or would need.
pub(crate) fn get_default_value(subkey: &WideName, flags: u32, buffer: &mut [u16]) -> (u32, u32) {
    let mut bytes = length_u32(buffer.len().saturating_mul(2));
    // SAFETY: `subkey` is NUL-terminated; `buffer` holds at least `bytes` bytes, which the call
    // writes at most; the type pointer may be null.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            null(),
            flags,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut bytes,
        )
    };
    (status, bytes)
}

/// RegCreateKeyExW of HKCU\`subkey`, no class, options or security attributes, with `access`:
/// the key, or the status when it fails.
pub(crate) fn create_key(subkey: &WideName, access: u32) -> Result<Key, u32> {
    let mut key: HKEY = null_mut();
    // SAFETY: `subkey` is NUL-terminated; no class, no security attributes, no disposition; the
    // key handle is written into a local and owned by the Key made of it.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            null(),
            0,
            access,
            null(),
            &raw mut key,
            null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(status);
    }
    Ok(Key(key))
}

/// RegOpenKeyExW of HKCU\`subkey` with `access`: the key, or the status when it fails.
pub(crate) fn open_key(subkey: &WideName, access: u32) -> Result<Key, u32> {
    let mut key: HKEY = null_mut();
    // SAFETY: `subkey` is NUL-terminated; the key handle is written into a local and owned by
    // the Key made of it.
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &raw mut key) };
    if status != ERROR_SUCCESS {
        return Err(status);
    }
    Ok(Key(key))
}

impl Key {
    /// RegSetValueExW of the value `name` as a REG_SZ of `data`, its bytes as stored (the UTF-16
    /// text and its terminator): the status.
    pub(crate) fn set_string(&self, name: &WideName, data: &[u8]) -> u32 {
        // SAFETY: the key is open (owned by self); the name is NUL-terminated and `data` holds
        // the bytes the call is told.
        unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr(),
                length_u32(data.len()),
            )
        }
    }

    /// RegFlushKey: the status.
    pub(crate) fn flush(&self) -> u32 {
        // SAFETY: the key is open.
        unsafe { RegFlushKey(self.0) }
    }
}

/// RegDeleteKeyValueW of the value `name` of HKCU\`subkey`: the status.
pub(crate) fn delete_value(subkey: &WideName, name: &WideName) -> u32 {
    // SAFETY: both names are NUL-terminated.
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey.as_ptr(), name.as_ptr()) }
}
