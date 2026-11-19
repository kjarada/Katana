//! The update layer's Windows system calls, and its only unsafe code on Windows (docs/rust.md,
//! "Unsafe confined to FFI modules"). Compiled on Windows only.
//!
//! One child per area - the file system (`fs`), processes (`process`: launching, the scan, the
//! token, the command line), the registry (`registry`) and the Credential Manager
//! (`credential`) - each a set of safe functions and owning types over the Win32 calls of that
//! area, with nothing else in it. What the update does with them - which flags, which retries,
//! which error codes mean what, the `os_error=N` texts - stays in the modules that decide it
//! (fsops_win, spawn_win, process_scan_win, registry_win, credential_win, install, state), which
//! are safe Rust under the workspace's `unsafe_code = "deny"`.
//!
//! The rules every wrapper keeps, so that its callers cannot misuse a call from safe code:
//! - a name handed to the system is a `WideName`, NUL-terminated by construction;
//! - a handle is owned (std's `OwnedHandle`, or a type of the area's that frees what the system
//!   allocated) and closed once, by its drop; one that is only borrowed for a call is a
//!   reference to its owner, so it is open for the call;
//! - a buffer the system writes into is a slice whose length is what the call is told;
//! - a call that reports failure through GetLastError is followed by `last_error()` with nothing
//!   in between, in the wrapper or at once in its caller, as before.

#![allow(
    unsafe_code,
    reason = "The Win32 calls of the update layer, behind safe wrappers; every unsafe block states why it is sound."
)]

pub(crate) mod credential;
pub(crate) mod fs;
pub(crate) mod process;
pub(crate) mod registry;

use windows_sys::Win32::Foundation::GetLastError;

/// This thread's last-error value, as the call before it left it.
pub(crate) fn last_error() -> u32 {
    // SAFETY: GetLastError only reads this thread's last-error value.
    unsafe { GetLastError() }
}

/// A wide (UTF-16) name with its terminating NUL, the form every Win32 call here takes a name
/// in. An interior NUL ends the name where the system reads it, as it did when the C++ handed it
/// a c_str().
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WideName(Vec<u16>);

impl WideName {
    /// `units` with a NUL appended.
    pub(crate) fn from_units(mut units: Vec<u16>) -> WideName {
        units.push(0);
        WideName(units)
    }

    /// The text of `text`, as UTF-16, with a NUL appended.
    pub(crate) fn from_text(text: &str) -> WideName {
        WideName::from_units(text.encode_utf16().collect())
    }

    fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }

    /// A pointer the system may write through, inside the name's own units (CreateProcessW's
    /// command line, CREDENTIALW's strings).
    fn as_mut_ptr(&mut self) -> *mut u16 {
        self.0.as_mut_ptr()
    }
}

/// A slice's length as the u32 a Win32 call takes it in: never more than the slice holds, so a
/// call told it never reaches past the end.
fn length_u32(length: usize) -> u32 {
    u32::try_from(length).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_name_ends_in_exactly_one_nul_after_its_text() {
        let name = WideName::from_text("ab");
        assert_eq!(name.0, vec![u16::from(b'a'), u16::from(b'b'), 0]);
        assert_eq!(WideName::from_units(Vec::new()).0, vec![0]);
    }

    #[test]
    fn a_length_past_u32_is_told_as_the_most_u32_holds() {
        assert_eq!(length_u32(7), 7);
        assert_eq!(length_u32(usize::MAX), u32::MAX);
    }
}
