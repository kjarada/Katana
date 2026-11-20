//! Where a session's line prints: a console its caller lends it for each call.
//!
//! The C++ writes a line's reply to `std::cout` and its refusals, warnings
//! and notes to `std::cerr`; katana_mcp swaps both buffers around each line,
//! and the tests capture them process-wide. Rust has no buffer to swap, and
//! cargo runs a binary's cases on parallel threads, so the session writes
//! through a console lent to it for the call: the process's streams in
//! katana_cli, a capture in katana_mcp and in the tests. Rejected: a session
//! that owns a boxed console (a reader could get the text back only by
//! downcasting); `run` returning its text (an update download's progress and
//! the start-up line would print late); a process-wide capture (it races
//! parallel tests).

use std::io::Write;

/// The two streams a line writes to. A text is written as it is given: the
/// line's end, where there is one, is the caller's.
pub trait Console {
    /// What the C++ wrote to `std::cout`: a reply, a report.
    fn out(&mut self, text: &str);
    /// What it wrote to `std::cerr`: `error:`, `warning:`, `note:`.
    fn err(&mut self, text: &str);
}

/// The process's own streams, each write flushed as `std::endl` flushed, so
/// an update download's progress shows as it happens. A write that fails (a
/// closed pipe) is passed over, as the C++'s streams pass it over.
#[derive(Clone, Copy, Debug, Default)]
pub struct Terminal;

impl Console for Terminal {
    fn out(&mut self, text: &str) {
        let mut stream = std::io::stdout().lock();
        let _ = stream.write_all(text.as_bytes());
        let _ = stream.flush();
    }

    fn err(&mut self, text: &str) {
        let mut stream = std::io::stderr().lock();
        let _ = stream.write_all(text.as_bytes());
        let _ = stream.flush();
    }
}

/// For a Windows GUI program (katana.exe, `windows_subsystem = "windows"`): started from a
/// console with its output not redirected, it is attached to that console, so a headless
/// switch's report (`--report`, `--screenshot`) prints there as it did when the program was a
/// console one. Started from Explorer there is no console to attach and nothing happens;
/// started with its output piped (a test, the updater's health check), Windows hands it the
/// pipes and it keeps them. Elsewhere it does nothing.
pub fn attach_parent_console() {
    #[cfg(windows)]
    windows_console::attach_unless_redirected();
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "AttachConsole is a Win32 function; std has no way to attach to a parent's console."
)]
mod windows_console {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

    pub(super) fn attach_unless_redirected() {
        // The process's standard output handle, from std: its as_raw_handle is GetStdHandle of
        // STD_OUTPUT_HANDLE, asked at each call, with "no handle" told as null (the test below
        // keeps both of the system's ways of saying it). katana_cli reads its handles so too.
        let out: HANDLE = std::io::stdout().as_raw_handle();
        if out.is_null() || out == INVALID_HANDLE_VALUE {
            // SAFETY: AttachConsole attaches this process to its parent's console, or fails
            // (no parent console, one already attached) and changes nothing; it takes no pointer.
            // A failure is ignored: the window runs without a console, as from Explorer.
            let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
        }
    }
}

/// What a line printed, held: katana_mcp's capture of a line, and a test's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured {
    pub out: String,
    pub err: String,
}

impl Console for Captured {
    fn out(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn err(&mut self, text: &str) {
        self.err.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::{Captured, Console};

    // A capture keeps each stream's writes in their order and apart.
    #[test]
    fn a_capture_holds_each_streams_text_in_order_and_apart() {
        let mut captured = Captured::default();
        captured.out("one\n");
        captured.err("error: two\n");
        captured.out("three\n");
        assert_eq!(captured.out, "one\nthree\n");
        assert_eq!(captured.err, "error: two\n");
    }

    // Lent as a trait object, as a session takes it.
    #[test]
    fn a_capture_lent_as_a_console_receives_the_writes() {
        let mut captured = Captured::default();
        {
            let console: &mut dyn Console = &mut captured;
            console.out("reply\n");
        }
        assert_eq!(captured.out, "reply\n");
        assert!(captured.err.is_empty());
    }
}
