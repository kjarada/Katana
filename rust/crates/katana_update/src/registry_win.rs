//! The Windows registry of the registry module: the few HKCU values an update reads and writes
//! (the C++'s src/katana_updater/registry_win.cpp, which the update library compiles on Windows
//! only, since the programs read installer_location() to say `kind=installer`). Per-user only, so
//! nothing here asks for administrator rights (design, D1).
//!
//! ```text
//! HKCU\Software\Jarada\Katana                        default value: the folder the installer used
//! HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce
//!                                                    value RUN_ONCE_NAME: the recovery command
//! HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Katana
//!                                                    DisplayVersion and DisplayName, only if the
//!                                                    key is there (the installer made it)
//! ```
//!
//! RunOnce is Windows' own mechanism: at the next logon the shell runs each value once and deletes
//! it first. The helper writes ours before it starts renaming files and removes it when the
//! transaction ends, so the entry exists only while a swap could have been cut short. Microsoft
//! documents a limit of 260 characters on the command line of a Run or RunOnce value; a longer one
//! is refused here, with the reason, rather than written as an entry that would not run (apply()
//! treats the entry as a second safety net behind recovery at the next start of any program, and
//! goes on without it).
//!
//! THE ENTRY IS FLUSHED TO DISK BEFORE set_run_once RETURNS (RegFlushKey). The registry writes
//! lazily - a value set now reaches the hive file some seconds later - and the one case this entry
//! exists for is the machine that loses power in the middle of the commit, with a katana.exe that
//! may no longer load: the journal's COMMITTING record is flushed (FsOps::append_durable), and an
//! entry that was still only in memory would be gone at the very logon that needs it. Microsoft
//! calls the flush expensive, which is why nothing else here asks for one: it is paid once for an
//! update, before the first file moves. Removing the entry is not flushed: an entry that outlives
//! its transaction runs `--recover` on an install that is whole, which finds nothing to do.
//!
//! THE TEST SEAM. make_registry_ops_under(base) is the same code with every key path re-rooted
//! under the HKCU subkey `base`, so that a test writes and deletes keys of its own and never the
//! machine's RunOnce or Uninstall entries. Nothing in the production programs calls it; the helper
//! built for the tests (--registry-base) does.

#![allow(
    unsafe_code,
    reason = "The registry is reached through its Win32 calls, which are foreign functions."
)]

use crate::registry::RegistryOps;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegFlushKey,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};

// The values of the Windows SDK's headers (winerror.h, winnt.h, winreg.h).
const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_UNSUPPORTED_TYPE: u32 = 1630;
const KEY_QUERY_VALUE: u32 = 0x0001;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;
const RRF_RT_REG_SZ: u32 = 0x0000_0002;
const MAX_PATH: usize = 260;

const LOCATION_KEY: &str = r"Software\Jarada\Katana";
const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Katana";
// Microsoft, "Run and RunOnce Registry Keys": the command line of a value is limited to this.
const RUN_ONCE_COMMAND_LIMIT: usize = 260;

fn registry_error(code: ErrorCode, message: &str, status: u32, what: &str) -> Error {
    make_error(code, message, format!("os_error={status} {what}"))
}

fn is_missing(status: u32) -> bool {
    status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND
}

fn terminated(text: &[u16]) -> Vec<u16> {
    let mut out = text.to_vec();
    out.push(0);
    out
}

// A key path for a message: the registry's names are UTF-16, a message is UTF-8.
fn narrow(text: &[u16]) -> String {
    String::from_utf16_lossy(text)
}

// The bytes of a REG_SZ: the text and its terminator, as UTF-16 in the machine's order.
fn reg_sz_bytes(text: &[u16]) -> Vec<u8> {
    terminated(text)
        .iter()
        .flat_map(|unit| unit.to_ne_bytes())
        .collect()
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the key is one this Key opened, closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }
}

struct WindowsRegistry {
    // Empty for the machine's real keys, else an HKCU subkey with no leading or trailing
    // backslash.
    base: Vec<u16>,
}

impl WindowsRegistry {
    fn under(&self, key: &str) -> Vec<u16> {
        let key: Vec<u16> = key.encode_utf16().collect();
        if self.base.is_empty() {
            return key;
        }
        let mut path = self.base.clone();
        path.push(u16::from(b'\\'));
        path.extend_from_slice(&key);
        path
    }
}

impl RegistryOps for WindowsRegistry {
    fn installer_location(&self) -> Result<Option<PathBuf>> {
        let path = self.under(LOCATION_KEY);
        let subkey = terminated(&path);
        let mut value = vec![0u16; MAX_PATH];
        for _ in 0..8 {
            let mut bytes = (value.len() * 2) as u32;
            // The default value (no name), a string; environment variables in an expandable one
            // are expanded, as an installer may have written %LOCALAPPDATA%. RRF_RT_REG_SZ alone
            // asks for exactly that: without RRF_NOEXPAND an expandable string is expanded and
            // handed back AS a REG_SZ, so it passes the restriction.
            // SAFETY: `subkey` is NUL-terminated; `value` holds `bytes` bytes, which the call
            // writes at most; the type pointer may be null.
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    null(),
                    RRF_RT_REG_SZ,
                    null_mut(),
                    value.as_mut_ptr().cast(),
                    &mut bytes,
                )
            };
            if status == ERROR_SUCCESS {
                // The text ends at the first NUL of the BUFFER, which was all NULs before the
                // call - not at the size reported. For an expandable string that size is the
                // stored text's, not the expanded one's: after a shorter expansion the stored
                // text's leftover follows the NUL, and a longer one would be cut short by it, and
                // the install taken for an archive because the folder is then not R.
                let end = value
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(value.len());
                if end == 0 {
                    return Ok(None);
                }
                return Ok(Some(PathBuf::from(OsString::from_wide(&value[..end]))));
            }
            if status == ERROR_MORE_DATA {
                // The size asked for can be the stored text's and not the expanded one's (the
                // same oddity), so the buffer also at least doubles: eight rounds reach the
                // longest path Windows has.
                let wanted = (bytes as usize / 2 + 1).max(value.len() * 2);
                value = vec![0u16; wanted];
                continue;
            }
            if is_missing(status) || status == ERROR_UNSUPPORTED_TYPE {
                // No installer key, or a value that is not a string: this is not an installer's.
                return Ok(None);
            }
            return Err(registry_error(
                ErrorCode::Internal,
                "the installer's folder cannot be read",
                status,
                &format!("HKCU\\{}", narrow(&path)),
            ));
        }
        Err(registry_error(
            ErrorCode::Internal,
            "the installer's folder cannot be read",
            ERROR_MORE_DATA,
            &format!("HKCU\\{}", narrow(&path)),
        ))
    }

    fn set_run_once(&self, name: &str, command: &str) -> Status {
        let wide_name: Vec<u16> = name.encode_utf16().collect();
        let wide_command: Vec<u16> = command.encode_utf16().collect();
        if wide_name.is_empty() || wide_command.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the RunOnce name and command must be non-empty UTF-8",
            ));
        }
        if wide_command.len() > RUN_ONCE_COMMAND_LIMIT {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the command is longer than the 260 characters Windows runs from RunOnce",
                format!("{} characters", wide_command.len()),
            ));
        }
        let path = self.under(RUN_ONCE_KEY);
        let subkey = terminated(&path);
        let mut key = Key(null_mut());
        // KEY_QUERY_VALUE is what RegFlushKey asks of the handle; nothing is read through it.
        // SAFETY: `subkey` is NUL-terminated; no class, no security attributes, no disposition;
        // the key handle is written to a Key, which closes it.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                null(),
                0,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                null(),
                &mut key.0,
                null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error(
                ErrorCode::InvalidState,
                "the RunOnce key cannot be opened",
                status,
                &format!("HKCU\\{}", narrow(&path)),
            ));
        }
        let value_name = terminated(&wide_name);
        let data = reg_sz_bytes(&wide_command);
        // SAFETY: the key is open for setting values; the name is NUL-terminated and `data` holds
        // the bytes the call is told.
        let status = unsafe {
            RegSetValueExW(
                key.0,
                value_name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr(),
                data.len() as u32,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error(
                ErrorCode::InvalidState,
                "the RunOnce value cannot be written",
                status,
                name,
            ));
        }
        // To the disk now (see the top). Best effort: the value is written either way, and an
        // entry that is only late is still better than none.
        // SAFETY: the key is open with KEY_QUERY_VALUE.
        unsafe { RegFlushKey(key.0) };
        Ok(())
    }

    fn clear_run_once(&self, name: &str) -> Status {
        let wide_name: Vec<u16> = name.encode_utf16().collect();
        if wide_name.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the RunOnce name must be non-empty UTF-8",
            ));
        }
        let path = self.under(RUN_ONCE_KEY);
        let subkey = terminated(&path);
        let value_name = terminated(&wide_name);
        // SAFETY: both strings are NUL-terminated.
        let status =
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey.as_ptr(), value_name.as_ptr()) };
        if status == ERROR_SUCCESS || is_missing(status) {
            return Ok(()); // already gone is gone
        }
        Err(registry_error(
            ErrorCode::InvalidState,
            "the RunOnce value cannot be removed",
            status,
            name,
        ))
    }

    fn set_installed_version(&self, display_version: &str, display_name: &str) -> Status {
        let version: Vec<u16> = display_version.encode_utf16().collect();
        let name: Vec<u16> = display_name.encode_utf16().collect();
        let path = self.under(UNINSTALL_KEY);
        let subkey = terminated(&path);
        let mut key = Key(null_mut());
        // Opened, never created: only the installer makes this key, and an archive install has
        // none (Settings > Apps lists what an installer registered, nothing else).
        // SAFETY: `subkey` is NUL-terminated; the key handle is written to a Key, which closes it.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key.0,
            )
        };
        if is_missing(status) {
            return Ok(());
        }
        if status != ERROR_SUCCESS {
            return Err(registry_error(
                ErrorCode::InvalidState,
                "the Uninstall key cannot be opened",
                status,
                &format!("HKCU\\{}", narrow(&path)),
            ));
        }
        let write = |value_name: &str, text: &[u16]| -> u32 {
            let value_name = terminated(&value_name.encode_utf16().collect::<Vec<u16>>());
            let data = reg_sz_bytes(text);
            // SAFETY: the key is open for setting values; the name is NUL-terminated and `data`
            // holds the bytes the call is told.
            unsafe {
                RegSetValueExW(
                    key.0,
                    value_name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr(),
                    data.len() as u32,
                )
            }
        };
        let mut status = write("DisplayVersion", &version);
        if status == ERROR_SUCCESS {
            status = write("DisplayName", &name);
        }
        if status != ERROR_SUCCESS {
            return Err(registry_error(
                ErrorCode::InvalidState,
                "the installed version cannot be recorded",
                status,
                "DisplayVersion",
            ));
        }
        Ok(())
    }
}

pub(crate) fn make_system_registry_ops() -> Box<dyn RegistryOps> {
    Box::new(WindowsRegistry { base: Vec::new() })
}

/// The test seam (see the top): the same keys under HKCU\<base>. None for a base that is empty,
/// or starts or ends with a backslash: it would not name a folder of keys of its own.
#[allow(
    dead_code,
    reason = "the helper built for the tests (--registry-base) calls it; it is ported with the \
              helper"
)]
pub(crate) fn make_registry_ops_under(base: &str) -> Option<Box<dyn RegistryOps>> {
    let wide: Vec<u16> = base.encode_utf16().collect();
    let backslash = u16::from(b'\\');
    if wide.is_empty() || wide.first() == Some(&backslash) || wide.last() == Some(&backslash) {
        return None;
    }
    Some(Box::new(WindowsRegistry { base: wide }))
}
