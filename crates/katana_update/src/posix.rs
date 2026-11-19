//! The update layer's one POSIX call with no form in std, and its only unsafe code off Windows
//! (docs/rust.md, "Unsafe confined to FFI modules"): kill(2) with signal 0, which
//! process_scan_posix asks whether a process is there. Declared by hand: katana_update does not
//! depend on the libc crate, and one function does not justify adding it.

#![allow(
    unsafe_code,
    reason = "kill(2) has no safe form in the standard library; the one call is confined here"
)]

// pid_t is a 32-bit signed integer on Linux and on macOS (sys/types.h).
pub(crate) type PidT = i32;

unsafe extern "C" {
    fn kill(pid: PidT, signal: i32) -> i32;
}

/// kill(pid, 0): Ok when the process is there and may be signalled, else the errno. It sends
/// nothing. `pid` is passed as it is: the caller keeps 0 and the negative ids (a process group to
/// kill()) away from it.
pub(crate) fn signal_nothing(pid: PidT) -> Result<(), i32> {
    // SAFETY: kill with signal 0 sends nothing; it reads its two integer arguments and touches no
    // memory of ours.
    if unsafe { kill(pid, 0) } == 0 {
        return Ok(());
    }
    // errno, read at once.
    Err(std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_answers_signal_nothing() {
        let pid = PidT::try_from(std::process::id()).expect("a pid fits pid_t");
        assert_eq!(signal_nothing(pid), Ok(()));
    }
}
