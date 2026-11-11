//! katana_mcp: the Katana engine as a Model Context Protocol server, so that
//! Claude can run engineering projects in Katana (docs/mcp.md). The port of
//! src/katana_app/mcp_main.cpp.
//!
//! ```text
//! katana_mcp                         serve MCP on stdin/stdout
//! katana_mcp --project <directory>   ... with that project already open
//! katana_mcp --help
//! ```
//!
//! An MCP client (Claude Desktop, Claude Code) starts it and talks to it over
//! its standard streams; it is not run by hand. Diagnostics go to stderr,
//! which the clients keep as the server's log.
//!
//! Standard input is read as BYTES, a message to a '\n', with a '\r' before
//! it taken off and blank lines skipped; the server reads the bytes as JSON
//! (katana_app's mcp::server). Each reply is written with a '\n' and flushed.
//!
//! A panic out of a tool or a verb is caught by the server or the session and
//! said in the reply; the hook this program installs prints nothing, so it is
//! not said twice in the log.

use std::fs::File;
use std::io::{BufRead, Write};
use std::process::ExitCode;

use katana_app::mcp::Server;
use katana_app::{Console, Session, Terminal, recover_interrupted_update};
use katana_update::utf8_arguments;
use katana_update::version::program_version_text;

/// The protocol's own stdout, taken before anything else can write to it.
///
/// stdout belongs to the protocol: one stray line - a library's printf, a
/// warning written straight to the file descriptor - and the client reads a
/// message that is not JSON and drops the connection. The server lends each
/// line a capture of its own, but that cannot catch C stdio or anything below
/// it (GDAL, PDAL). So file descriptor 1 is duplicated for the protocol and
/// then pointed at stderr: whatever else writes to "stdout" lands in the log,
/// Rust's own standard output (and so the Terminal console) among it.
#[cfg(not(windows))]
#[allow(
    unsafe_code,
    reason = "the protocol's stream is made with the C library's dup and dup2, which std does not \
              have; confined to this module"
)]
mod protocol {
    use std::ffi::c_int;
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::FromRawFd;

    // unistd.h
    unsafe extern "C" {
        fn dup(fd: c_int) -> c_int;
        fn dup2(old: c_int, new: c_int) -> c_int;
    }

    // The standard descriptors, STDOUT_FILENO and STDERR_FILENO.
    const OUTPUT: c_int = 1;
    const ERROR: c_int = 2;

    /// The protocol's stream; None when the descriptors could not be made.
    pub(super) fn take() -> Option<File> {
        let _ = std::io::stdout().flush();
        // SAFETY: dup reads this process's descriptor table and makes a new
        // descriptor for its standard output; nothing is borrowed.
        let protocol = unsafe { dup(OUTPUT) };
        // SAFETY: points this process's descriptor 1 at the file of its
        // descriptor 2; both are the process's own standard streams.
        if protocol < 0 || unsafe { dup2(ERROR, OUTPUT) } < 0 {
            return None;
        }
        // SAFETY: `protocol` is a descriptor dup has just made, open, and
        // owned by nothing else: the File is its only owner from here.
        Some(unsafe { File::from_raw_fd(protocol) })
    }
}

/// The same on Windows, through the C runtime's own calls, which keep the
/// runtime's descriptors and the process's standard handles in step (the
/// runtime's _dup2 of descriptor 1 sets STD_OUTPUT_HANDLE, which Rust's
/// stdout writes through). Written, not compiled before step 8.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the protocol's stream is made with the C runtime's _dup, _dup2, _setmode and \
              _get_osfhandle, which std does not have; confined to this module"
)]
mod protocol {
    use std::ffi::c_int;
    use std::fs::File;
    use std::io::Write;
    use std::os::windows::io::{FromRawHandle, RawHandle};

    // io.h and fcntl.h of the UCRT.
    unsafe extern "C" {
        fn _dup(fd: c_int) -> c_int;
        fn _dup2(old: c_int, new: c_int) -> c_int;
        fn _setmode(fd: c_int, mode: c_int) -> c_int;
        fn _get_osfhandle(fd: c_int) -> isize;
    }

    // fcntl.h: _O_BINARY.
    const BINARY: c_int = 0x8000;
    // What _fileno answers for stdin, stdout and stderr.
    const INPUT: c_int = 0;
    const OUTPUT: c_int = 1;
    const ERROR: c_int = 2;
    // _get_osfhandle's answer for a descriptor that is not open.
    const NO_HANDLE: isize = -1;

    /// The protocol's stream; None when the descriptors could not be made.
    pub(super) fn take() -> Option<File> {
        let _ = std::io::stdout().flush();
        // SAFETY: _dup reads the runtime's descriptor table and makes a new
        // descriptor for its standard output; nothing is borrowed.
        let protocol = unsafe { _dup(OUTPUT) };
        // SAFETY: points the runtime's descriptor 1 (and the process's
        // standard output handle) at the file of descriptor 2.
        if protocol < 0 || unsafe { _dup2(ERROR, OUTPUT) } != 0 {
            return None;
        }
        // Binary, both ways: a message ends in "\n", never "\r\n".
        // SAFETY: both are open descriptors of this runtime.
        unsafe {
            _setmode(protocol, BINARY);
            _setmode(INPUT, BINARY);
        }
        // SAFETY: `protocol` is the open descriptor _dup has just made.
        let handle = unsafe { _get_osfhandle(protocol) };
        if handle == NO_HANDLE {
            return None;
        }
        // SAFETY: the handle is the one under `protocol`, which nothing else
        // in this program writes to or closes; the File owns it from here.
        Some(unsafe { File::from_raw_handle(handle as RawHandle) })
    }
}

fn send(protocol: &mut File, message: &str) {
    let mut bytes = Vec::with_capacity(message.len() + 1);
    bytes.extend_from_slice(message.as_bytes());
    bytes.push(b'\n');
    // A client that has gone away is not answered; the loop ends when its
    // input does.
    let _ = protocol.write_all(&bytes);
    let _ = protocol.flush();
}

const USAGE: &str = concat!(
    "usage: katana_mcp [--project <directory>]\n\n",
    "Serves the Katana engine to an MCP client (Claude Desktop, Claude Code) over\n",
    "stdin and stdout. Started by the client, not by hand; see docs/mcp.md.\n\n",
    "  --project <directory>  open this Katana project when the session starts\n"
);

fn main() -> ExitCode {
    // The server and the session catch a panic and say it in the reply; the
    // default hook would print it again, in the log. Process state, so the
    // program's and never the library's.
    std::panic::set_hook(Box::new(|_| {}));
    let mut terminal = Terminal;

    // First, before a file of the install is read: an update that was
    // interrupted is put back (katana_app's session). What it says goes to
    // stderr, the server's log; the protocol's stream is not touched.
    recover_interrupted_update(&mut terminal);

    // As UTF-8 and never from argv: on Windows that is the ANSI code page's
    // version of the command line (katana_update's process), so the
    // `--project C:/Users/José/site` an MCP client is configured with arrived
    // as bytes no path conversion accepts, and the server ended before it
    // answered `initialize`.
    let arguments = utf8_arguments();
    let mut project: Vec<u8> = Vec::new();
    let mut at = 1;
    while at < arguments.len() {
        let argument = &arguments[at];
        if argument.as_slice() == b"-h" || argument.as_slice() == b"--help" {
            terminal.out(USAGE);
            return ExitCode::SUCCESS;
        }
        if argument.as_slice() == b"--project" && at + 1 < arguments.len() {
            project = arguments[at + 1].clone();
            at += 2;
            continue;
        }
        terminal.err(&format!(
            "error: unexpected argument {}\n",
            String::from_utf8_lossy(argument)
        ));
        terminal.err(USAGE);
        return ExitCode::from(2);
    }

    let Some(mut protocol) = protocol::take() else {
        terminal.err("error: katana_mcp could not take its standard output for the protocol\n");
        return ExitCode::from(1);
    };
    // Standard output now reaches stderr, as everything but the protocol must
    // - the line that says which customisation the session started with among
    // it. Never None, which is a session of no program (katana_app's session).
    let program = arguments.first().map_or_else(
        || String::from("katana_mcp"),
        |first| String::from_utf8_lossy(first).into_owned(),
    );
    let mut session = Session::new(Some(&program), &mut terminal);
    if !project.is_empty() {
        if project.contains(&b'"') {
            terminal.err("error: a project path may not contain a double quote\n");
            return ExitCode::from(2);
        }
        let mut line = b"OPEN \"".to_vec();
        line.extend_from_slice(&project);
        line.push(b'"');
        if !session.run(&line, &mut terminal) {
            return ExitCode::from(1);
        }
    }
    terminal.err(&format!(
        "katana_mcp {}: serving MCP on stdio\n",
        program_version_text()
    ));

    let mut server = Server::new(program_version_text());
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    loop {
        // A line as std::getline reads it: up to a '\n', which is taken off;
        // a last line with no '\n' is a line too.
        let mut line = Vec::new();
        match input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.iter().all(|&byte| byte == b' ' || byte == b'\t') {
            continue;
        }
        if let Some(answer) = server.handle(&mut session, &line) {
            send(&mut protocol, &answer);
        }
    }
    drop(protocol);
    ExitCode::SUCCESS
}
