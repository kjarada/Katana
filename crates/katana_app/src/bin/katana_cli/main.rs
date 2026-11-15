//! katana_cli: the Katana engine without a GUI (the port of
//! src/katana_app/main.cpp).
//!
//! ```text
//! katana_cli                      interactive session on stdin
//! katana_cli script.kcs           run a script, one command per line
//! katana_cli -c "LINE 0,0 5,5" -c LIST
//! ```
//!
//! Scripts and -c commands stop at the first failing command and exit with
//! status 1, so the tool can be used in automated pipelines. Lines whose first
//! non-blank is '#' are comments. It drives exactly the same Document,
//! commands and storage as the desktop application; the verbs themselves are
//! the session's (katana_app::session).
//!
//! Two things here are the update system's (katana_app's update_verbs and
//! session), and act on this program rather than on a drawing: before
//! anything else, an update that was interrupted is put back
//! (recover_interrupted_update); and UPDATE APPLY CONFIRM must be the LAST
//! line of a run. The line only prepares: the run then ends, the session with
//! it, and only then is the update helper started, which waits for this
//! process to exit before it replaces a file. Nothing is started again
//! afterwards. A script or a -c with a line after it is refused before
//! anything is prepared; typed or piped on standard input, where the next
//! line cannot be looked at without waiting for it, the session ends with the
//! line and says so.
//!
//! Text is UTF-8 here, and what reaches the program is made so at the door
//! (docs/headless.md, "Text and names"): the arguments are read WIDE on
//! Windows (katana_update's utf8_arguments), where argv holds the ANSI code
//! page's version of them; a script is decoded as a text file is
//! (Session::read_script); a console's lines are read wide as well
//! (read_line), and its output is put in UTF-8 for the run, so the names a
//! reply prints are readable. On Windows any other line that is not UTF-8 - a
//! pipe from a program writing the ANSI code page - is read as Windows-1252 by
//! Session::run, which says so; elsewhere such a line is refused there.
//!
//! Standard input is read as BYTES, a line to a '\n', which is taken off and
//! nothing else: a '\r' before it stays, as std::getline leaves it (the
//! session's verbs read it as a blank, the UPDATE verbs as a separator).
//!
//! A panic out of a verb is caught by the session and said as `error:
//! Internal: ...`; the hook this program installs prints nothing, so it is not
//! said twice.

use std::io::{BufRead, StdinLock};
use std::process::ExitCode;

use katana_app::{Console, Session, Terminal, abandon_apply, recover_interrupted_update};
use katana_core::text::trimmed;
use katana_update::{ApplyRequest, launch_apply, utf8_arguments};

/// A console on Windows: its output code page, and its lines read as UTF-16.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the console's code page and wide reading are Windows calls, confined to this module"
)]
mod windows_console {
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetConsoleOutputCP, GetStdHandle, ReadConsoleW, STD_ERROR_HANDLE,
        STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleOutputCP,
    };

    // The UTF-8 code page (CP_UTF8, WinNls.h).
    const CP_UTF8: u32 = 65001;

    // Ctrl+Z, the end of a console's input as std::cin reads it.
    const END_OF_INPUT: u16 = 0x1A;

    // The handle of standard input or output when it is a console, else None:
    // a pipe or a file is read and written as the bytes it is.
    fn console_handle(which: STD_HANDLE) -> Option<HANDLE> {
        // SAFETY: GetStdHandle only reads the process's standard handles.
        let handle = unsafe { GetStdHandle(which) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut mode: u32 = 0;
        // SAFETY: `mode` is a live u32 the call writes; the handle is the
        // process's own.
        if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
            return None;
        }
        Some(handle)
    }

    /// Standard input when it is a console.
    pub(super) fn input_console() -> Option<HANDLE> {
        console_handle(STD_INPUT_HANDLE)
    }

    // A console shows the bytes a program writes through its OUTPUT code
    // page, which is the OEM one (850, 437) unless set: a reply holding
    // "Grüße" in UTF-8 would be mojibake on the screen. Set to UTF-8 for the
    // run and put back after, so the person's shell is as it was. Only for a
    // console - a pipe is not transcoded by the page, and a program reading
    // katana_cli's output reads the UTF-8 it always did.
    pub(super) struct OutputInUtf8 {
        before: u32,
        changed: bool,
    }

    impl OutputInUtf8 {
        pub(super) fn for_the_run() -> Self {
            let mut set = Self {
                before: 0,
                changed: false,
            };
            if console_handle(STD_OUTPUT_HANDLE).is_some()
                || console_handle(STD_ERROR_HANDLE).is_some()
            {
                // SAFETY: reads this process's console output code page.
                set.before = unsafe { GetConsoleOutputCP() };
                // SAFETY: sets this process's console output code page, which
                // the drop puts back.
                if set.before != 0
                    && set.before != CP_UTF8
                    && unsafe { SetConsoleOutputCP(CP_UTF8) } != 0
                {
                    set.changed = true;
                }
            }
            set
        }
    }

    impl Drop for OutputInUtf8 {
        fn drop(&mut self) {
            if self.changed {
                // SAFETY: puts back the code page this value read at its start.
                unsafe {
                    SetConsoleOutputCP(self.before);
                }
            }
        }
    }

    // One line of the console as UTF-8, without its terminator; None at the
    // end of input. Read as UTF-16 (ReadConsoleW) and converted: std::cin got
    // the bytes of the console's INPUT code page, so the "é" typed at the
    // prompt arrived as 0x82 (OEM 850), which is no UTF-8 and no Windows-1252
    // either - "SAVE C:/Users/José/site" ended the session with the drawing
    // unsaved.
    pub(super) fn read_line(console: HANDLE) -> Option<Vec<u8>> {
        let mut wide: Vec<u16> = Vec::new();
        loop {
            let mut chunk = [0u16; 256];
            let mut read: u32 = 0;
            // SAFETY: `chunk` holds 256 UTF-16 units, the count passed; `read`
            // is a live u32 the call writes; no input control is passed.
            let ok = unsafe {
                ReadConsoleW(
                    console,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                    std::ptr::null(),
                )
            };
            if ok == 0 || read == 0 {
                if wide.is_empty() {
                    return None; // the console was closed
                }
                break;
            }
            wide.extend_from_slice(&chunk[..read as usize]);
            if wide.last() == Some(&u16::from(b'\n')) {
                break;
            }
        }
        // Ctrl+Z is the end of input, as it is to std::cin.
        if wide.contains(&END_OF_INPUT) {
            return None;
        }
        while wide
            .last()
            .is_some_and(|&unit| unit == u16::from(b'\n') || unit == u16::from(b'\r'))
        {
            wide.pop();
        }
        Some(katana_update::process::utf8_from_wide(&wide).into_bytes())
    }
}

// One line of standard input as bytes, as std::getline reads it: up to a
// '\n', which is taken off; a '\r' before it stays. None at the end of input.
fn read_bytes_line(input: &mut StdinLock<'_>) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    match input.read_until(b'\n', &mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            Some(line)
        }
    }
}

// One line of standard input; on Windows a console's is read wide
// (windows_console::read_line), a pipe's or a file's as its bytes.
#[cfg(windows)]
fn read_line(input: &mut StdinLock<'_>) -> Option<Vec<u8>> {
    match windows_console::input_console() {
        Some(console) => windows_console::read_line(console),
        None => read_bytes_line(input),
    }
}

#[cfg(not(windows))]
fn read_line(input: &mut StdinLock<'_>) -> Option<Vec<u8>> {
    read_bytes_line(input)
}

// Whether nothing is run after line `at`: every later line, up to a QUIT, is
// blank or a note.
fn is_last_line_of_run(lines: &[Vec<u8>], at: usize) -> bool {
    for later in &lines[at + 1..] {
        if Session::is_quit(later) {
            break;
        }
        let body = trimmed(later);
        if !body.is_empty() && body[0] != b'#' {
            return false;
        }
    }
    true
}

fn run_batch(session: &mut Session, lines: &[Vec<u8>], terminal: &mut Terminal) -> u8 {
    for (at, line) in lines.iter().enumerate() {
        if Session::is_quit(line) {
            break;
        }
        // Refused before it prepares anything: a line after it would be run
        // by a session that has already handed the install to the helper, or
        // not at all.
        if Session::is_update_apply(line) && !is_last_line_of_run(lines, at) {
            terminal.err(
                "error: Unsupported: UPDATE APPLY CONFIRM must be the last line of a katana_cli \
                 run: the run ends with it and the update helper then replaces the program, so \
                 put every other line before it\n",
            );
            return 1;
        }
        if !session.run(line, terminal) {
            return 1;
        }
    }
    0
}

fn main() -> ExitCode {
    // The session catches a panic out of a verb and says it as `error:
    // Internal: ...`; the default hook would print it a second time. Process
    // state, so the program's and never the library's.
    std::panic::set_hook(Box::new(|_| {}));
    let mut terminal = Terminal;

    // First: a program must not read the files of an install that an
    // interrupted update left half swapped (katana_app's session).
    recover_interrupted_update(&mut terminal);

    #[cfg(windows)]
    let _console_output = windows_console::OutputInUtf8::for_the_run();
    // As UTF-8 and never from argv, which on Windows is the ANSI code page's
    // version of the command line (katana_update's process): a path with an
    // accented letter in it ended the program in a filesystem error, and one
    // with a letter outside the page - a Polish or Chinese profile name - was
    // refused as "not found" under a name of '?'s.
    let arguments = utf8_arguments();
    let mut batch: Vec<Vec<u8>> = Vec::new();
    let mut at = 1;
    while at < arguments.len() {
        let argument = &arguments[at];
        if argument.as_slice() == b"-h" || argument.as_slice() == b"--help" {
            terminal.out(&format!(
                "usage: katana_cli [script-file] [-c \"command\"]...\n\n{}",
                Session::help_text()
            ));
            return ExitCode::SUCCESS;
        }
        if argument.as_slice() == b"-c" {
            let Some(command) = arguments.get(at + 1) else {
                terminal.err("error: -c needs a command\n");
                return ExitCode::from(2);
            };
            batch.push(command.clone());
            at += 2;
            continue;
        }
        match Session::read_script(argument, &mut terminal) {
            Ok(script) => batch.extend(script.into_iter().map(String::into_bytes)),
            Err(error) => {
                terminal.err(&format!(
                    "error: cannot read script {}: {}\n",
                    String::from_utf8_lossy(argument),
                    error.describe()
                ));
                return ExitCode::from(2);
            }
        }
        at += 1;
    }

    // What an UPDATE APPLY CONFIRM left to be launched, taken out of the
    // session before it ends.
    let mut apply: Option<ApplyRequest> = None;
    let mut status: u8 = 0;
    {
        // After the arguments, so that --help and a bad argument say nothing
        // of the customisation loaded. Never None, which is a session of no
        // program and starts with no customisation (katana_app's session): a
        // katana_cli started with no argv[0] is still katana_cli.
        let program = arguments.first().map_or_else(
            || String::from("katana_cli"),
            |first| String::from_utf8_lossy(first).into_owned(),
        );
        let mut session = Session::new(Some(&program), &mut terminal);
        if !batch.is_empty() {
            status = run_batch(&mut session, &batch, &mut terminal);
        } else {
            terminal.out("Katana command line. HELP lists commands, QUIT leaves.\n");
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            loop {
                terminal.out("> ");
                let Some(line) = read_line(&mut input) else {
                    break;
                };
                if Session::is_quit(&line) {
                    break;
                }
                // Interactive: report and carry on.
                session.run(line, &mut terminal);
                // Typed, an UPDATE APPLY CONFIRM that succeeded is the last
                // line by being it: the session ends here. That is said,
                // because a line after it is not run - one piped in behind it
                // is dropped here, where run_batch refuses the same lines
                // before anything is prepared. Standard input cannot be
                // looked ahead in: reading on would wait for a person who has
                // nothing more to type.
                apply = session.take_pending_apply();
                if apply.is_some() {
                    terminal.err(
                        "note: the session ends with UPDATE APPLY CONFIRM and the update helper \
                         is started now; a line typed or piped after it is not run\n",
                    );
                    break;
                }
            }
        }
        if status == 0 && apply.is_none() {
            apply = session.take_pending_apply();
        }
    }

    // The session has ended and its drawing is closed. Only now is the helper
    // started; it waits for this process to exit, and this process starts
    // nothing after it.
    if let Some(request) = &apply
        && let Err(launched) = launch_apply(request)
    {
        // The helper's copy the line had made is nobody's now: it is listed
        // for the sweep (katana_app's update_verbs, abandon_apply).
        let _ = abandon_apply(request);
        terminal.err(&format!(
            "error: the update helper could not be started, so nothing was installed and the \
             release stays downloaded: {}\n",
            launched.describe()
        ));
        return ExitCode::from(1);
    }
    ExitCode::from(status)
}
