//! katana_update_e2e: the update engine driven from a command line, for the end-to-end tests
//! (tests/update/update_e2e.py, which also starts tools/update_fake_feed.py and the test helper,
//! katana_updater_test). Never installed. The twin of tests/update/e2e_main.cpp.
//!
//! WHY IT EXISTS. The programs that ship cannot be given another trust root or another feed: the
//! keys are pinned at build time and the engine takes them as a parameter (trust.rs). So a test
//! that wants the real engine against a release it signed itself needs a program that passes the
//! TEST key (the tests' test_key module) - this one - and a helper built the same way. It uses
//! katana_update and katana_core and nothing else, so it has no HTTP library: the transport is the
//! engine's own make_fetch (redirects, the token's rule, the answer of a range - feed.rs) over a
//! hop function written here on plain sockets. It is a binary of the helper's package and not of
//! the engine's because the end-to-end tests start it beside the helper, and cargo hands a test
//! the programs of its own package alone.
//!
//! THE WIRE REACHES 127.0.0.1 AND NOTHING ELSE. The Connection below has no way to name another
//! host: it connects to the loopback address and a port. An http:// address must spell 127.0.0.1
//! (no name is looked up); an https:// address is carried only where --connect says which
//! loopback port stands in for its host, in clear, and is refused otherwise. So no test can leave
//! the machine through this program, whatever a manifest, a redirect or a mistake names
//! (CLAUDE.md, section 4: a test may start its own server on 127.0.0.1).
//!
//! COMMAND LINES (UTF-8, read with katana_update's utf8_arguments; every folder absolute):
//!
//! ```text
//!   katana_update_e2e check|plan|stage --root R --state U (--feed URL | --github OWNER/NAME)
//!                     [--token T] [--connect HOST=PORT]... [--even-if-not-newer]
//!       check   asks the feed (check()) and applies what U/state.json holds (blocked, skipped)
//!               the way the window does
//!       plan    check, then hashes the install against the manifest (plan())
//!       stage   plan, then downloads what changed into U/stage-<sequence> (stage())
//!       --feed URL          a feed of the owner's own: http://127.0.0.1:<port>[/folder]
//!       --github OWNER/NAME GitHub's addresses for that repository, which --connect must route
//!       --token T           FeedConfig::token: the private-repository route's token
//!       --connect HOST=PORT requests the engine addresses to https://HOST go, in clear, to
//!                           127.0.0.1:PORT (what curl calls --connect-to, without TLS): a fake
//!                           stands in for that host. It is how the rule "the token goes to
//!                           api.github.com and to no address a redirect names" is tested here.
//!       --even-if-not-newer plan and stage a release that verified but is not above what is
//!                           installed, which the window never does: the way a test hands the
//!                           helper a downgrade, to see the helper refuse it by itself
//!   katana_update_e2e preflight --root R --state U --stage S
//!       reads the staged manifest back through verify_manifest and runs preflight(): the helper
//!       is copied to U/run-<tx> and its --selftest is run. No network.
//!   katana_update_e2e launch --root R --state U --stage S [--relaunch <path|none>]
//!                     [--wait-pid <n|self>]
//!       preflight, then launch_apply: the helper copy is started detached and this program
//!       ends, as the window does. By default the helper waits for THIS process and starts
//!       R/bin/katana afterwards.
//!   katana_update_e2e recover --root R --state U
//!       recover(R), the first lines of main() of every program. It finds the helper copy
//!       through state_directory(), so KATANA_UPDATE_STATE_DIR must name U (refused otherwise:
//!       a test must not look into the person's own update folder).
//! ```
//!
//! WHAT IT PRINTS (standard output, key=value, one a line, a control character as %XX):
//!
//! ```text
//!   always       target= kind= installed= installed_sequence=
//!   check        update=available|current|unreachable|unsupported|blocked latest= sequence=
//!                reason= route=download|api notes_url= detail=
//!   plan         files=<changed>/<total> download_bytes= changed_bytes= required_free_bytes=
//!                changed=<path> for each changed file, range=<offset>+<length> for each request
//!   stage        stage=<folder> progress=<done>/<total> progress_reports=<n>
//!                progress_steady=true|false (it never went backwards nor past its total: stage.rs)
//!                staged=<version>; `staged=none` when nothing was offered
//!   preflight    stage= version= transaction= run= helper=
//!   launch       preflight's, then wait_pid= relaunch= launched=<transaction>
//!   recover      recover=ok
//!   a failure    error=<ErrorCode> error_reason=<token of fetch_failure_reason>
//!                error_message= error_context=
//! ```
//!
//! EXIT CODES: 0 the call was made (a feed that cannot be read is an ANSWER, update=unreachable,
//! not a failure); 1 the engine answered with an error (the error= lines say which); 2 the command
//! line is wrong; 3 an unexpected error (a panic, where the C++ catches an exception).

use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8, read_file_bytes};
use katana_core::text::{lowered, trimmed};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result};
use katana_update::apply::{ApplyRequest, installed_program, launch_apply_with};
use katana_update::check::{CheckStatus, apply_update_state, check};
use katana_update::feed::{
    FeedRoute, HttpHop, HttpHopReply, default_feed_config, fetch_failure_reason, hop_fn,
    is_fetchable_url, is_loopback_http_url, make_fetch,
};
use katana_update::install::{Install, read_install};
use katana_update::manifest::Manifest;
use katana_update::plan::plan;
use katana_update::process::{make_system_launcher, make_system_process_ops, utf8_arguments};
use katana_update::recover::recover;
use katana_update::recover_internal::one_line;
use katana_update::stage::{preflight, stage, staged_manifest_path, staged_signature_path};
use katana_update::state::{read_state, stage_path, state_directory};
use katana_update::trust::verify_manifest;
use katana_updater::NoRegistry;
use std::any::Any;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[allow(
    dead_code,
    reason = "the module of the test keys is shared with the tests; this program uses the trust alone"
)]
#[path = "../../../katana_update/tests/test_key/mod.rs"]
mod test_key;

use test_key::test_trust;

const EXIT_OK: i32 = 0;
const EXIT_FAILED: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_INTERNAL: i32 = 3;

const USAGE: &str = concat!(
    "usage: katana_update_e2e check|plan|stage --root R --state U\n",
    "                         (--feed URL | --github OWNER/NAME) [--token T]\n",
    "                         [--connect HOST=PORT]... [--even-if-not-newer]\n",
    "       katana_update_e2e preflight --root R --state U --stage S\n",
    "       katana_update_e2e launch --root R --state U --stage S [--relaunch <path|none>]\n",
    "                         [--wait-pid <n|self>]\n",
    "       katana_update_e2e recover --root R --state U\n"
);

// ---- what is printed ----------------------------------------------------------------------------

// Standard output and standard error, written and flushed; a stream that cannot be written is not
// this program's failure (the exit code says what happened), as it is none for the C++'s streams.
fn write_out(bytes: &[u8]) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

fn write_error(bytes: &[u8]) {
    let mut error = std::io::stderr().lock();
    let _ = error.write_all(bytes);
    let _ = error.flush();
}

// `value` on one line: a control character becomes %XX, so that a value read from a file or a
// server cannot make one record look like two.
fn say(key: &str, value: impl AsRef<[u8]>) {
    let mut line = format!("{key}=").into_bytes();
    line.extend(one_line(value));
    line.push(b'\n');
    write_out(&line);
}

fn say_number(key: &str, value: u64) {
    write_out(format!("{key}={value}\n").as_bytes());
}

// The error= lines of a failure, and the exit code that goes with them. Keys of their own: a check
// that was made before the failure has already printed its `reason=`.
fn say_error(error: &Error) -> i32 {
    say("error", katana_core::error::to_string(error.code));
    say("error_reason", fetch_failure_reason(error));
    say("error_message", error.message.as_str());
    say("error_context", error.context.as_str());
    EXIT_FAILED
}

fn usage_failure(error: &Error) -> i32 {
    write_error(format!("katana_update_e2e: {}\n{USAGE}", error.describe()).as_bytes());
    EXIT_USAGE
}

// Bytes as a value the engine takes as text (an address, a token), or an error's context.
fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// std::from_chars into an int over the whole of `text`: an optional '-' and decimal digits, and
// nothing else (no '+', no blank). A number past an int's range is refused by every caller's own
// range check as from_chars refuses it.
fn whole_int(text: &[u8]) -> Option<i64> {
    let digits = text.strip_prefix(b"-").unwrap_or(text);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let magnitude: i64 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    Some(if text.starts_with(b"-") {
        -magnitude
    } else {
        magnitude
    })
}

// std::from_chars into an unsigned 64-bit number over the whole of `text`: decimal digits only.
fn whole_u64(text: &[u8]) -> Option<u64> {
    if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse().ok()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---- the wire: one request to 127.0.0.1, redirects not followed ---------------------------------

// The OS's number for a socket's failure (errno, or WSAGetLastError on Windows), as the C++ reads
// it after the call failed.
fn os_error(error: &std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(0)
}

// A TCP connection to the loopback address. It takes a port and no host: there is no way to ask
// it for another machine. (The standard library starts Winsock itself; the C++'s SocketsLibrary
// has no twin.)
struct Connection {
    stream: TcpStream,
}

impl Connection {
    fn open(port: u16, timeout_seconds: i32) -> std::io::Result<Self> {
        // A feed that stops answering ends the request; it does not hang the test. The connect is
        // bounded by the same time, as SO_SNDTIMEO bounds the C++'s on Linux.
        let limit = Duration::from_secs(u64::try_from(timeout_seconds).unwrap_or(1));
        let to = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&to, limit)?;
        let _ = stream.set_read_timeout(Some(limit));
        let _ = stream.set_write_timeout(Some(limit));
        Ok(Self { stream })
    }
}

// An address taken apart far enough to send it: make_fetch has already held it to
// is_fetchable_url, so this reads and does not judge.
#[derive(Debug, Default)]
struct Address {
    https: bool,
    // As written: what the Host header carries.
    authority: String,
    // Lower case, without the port.
    host: String,
    // 0: none written.
    port: u16,
    // The path and the query, "/" when the address has neither.
    target: String,
}

fn parse_address(url: &str) -> Option<Address> {
    let mark = url.find("://")?;
    let scheme = url[..mark].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let rest = &url[mark + 3..];
    let end = rest.find(['/', '?', '#']);
    let authority = &rest[..end.unwrap_or(rest.len())];
    let target = end.map_or("", |end| &rest[end..]);
    // A fragment is never sent.
    let target = &target[..target.find('#').unwrap_or(target.len())];
    if authority.is_empty() || authority.contains('@') || authority.contains('[') {
        return None; // user-info, or an IPv6 literal, which this client does not speak
    }
    let colon = authority.find(':');
    let host = authority[..colon.unwrap_or(authority.len())].to_ascii_lowercase();
    let mut port: u16 = 0;
    if let Some(colon) = colon {
        let written = whole_int(&authority.as_bytes()[colon + 1..])?;
        port = u16::try_from(written).ok().filter(|&port| port >= 1)?;
    }
    if host.is_empty() {
        return None;
    }
    let target = if target.is_empty() {
        String::from("/")
    } else if target.starts_with('?') {
        format!("/{target}")
    } else {
        String::from(target)
    };
    Some(Address {
        https: scheme == "https",
        authority: String::from(authority),
        host,
        port,
        target,
    })
}

// What a hop says when no HTTP answer arrived: FileImportFailure, `status=0` first, then the hop's
// own words and never the address (feed.rs, HopFn).
fn no_answer(message: &str, words: &str) -> Error {
    make_error(
        ErrorCode::FileImportFailure,
        message,
        format!("status=0; {words}"),
    )
}

#[derive(Debug, Default)]
struct Head {
    status: i32,
    content_type: String,
    location: String,
    content_range: String,
    content_length: Option<u64>,
    transfer_encoded: bool,
}

// The status line and the headers of an answer, without the blank line that ends them.
fn parse_head(text: &[u8]) -> Option<Head> {
    let mut head = Head::default();
    let first_end = find(text, b"\r\n");
    let status_line = &text[..first_end.unwrap_or(text.len())];
    // "HTTP/1.1 206 Partial Content": the version, one blank, three digits.
    if !status_line.starts_with(b"HTTP/1.")
        || status_line.len() < 12
        || status_line[8] != b' '
        || (status_line.len() > 12 && status_line[12] != b' ')
    {
        return None;
    }
    head.status = whole_int(&status_line[9..12]).and_then(|code| i32::try_from(code).ok())?;
    if head.status < 100 {
        return None;
    }
    let mut rest: &[u8] = match first_end {
        Some(end) => &text[end + 2..],
        None => &[],
    };
    while !rest.is_empty() {
        let end = find(rest, b"\r\n");
        let line = &rest[..end.unwrap_or(rest.len())];
        rest = match end {
            Some(end) => &rest[end + 2..],
            None => &[],
        };
        let Some(colon) = line.iter().position(|&c| c == b':') else {
            continue;
        };
        let name = lowered(trimmed(&line[..colon]));
        let value = trimmed(&line[colon + 1..]);
        match name.as_slice() {
            b"content-type" => head.content_type = text_of(value),
            b"location" => head.location = text_of(value),
            b"content-range" => head.content_range = text_of(value),
            b"transfer-encoding" => head.transfer_encoded = true,
            b"content-length" => head.content_length = Some(whole_u64(value)?),
            _ => {}
        }
    }
    Some(head)
}

// Which loopback port stands in for an https host (--connect), by the host in lower case.
type Routes = BTreeMap<String, u16>;

// The headers of an answer are a few hundred bytes; one that sends this many and no blank line is
// not speaking HTTP.
const MAX_HEAD_BYTES: usize = 64 * 1024;
// One request on this machine takes milliseconds. The engine's own allowance for a hop is an hour
// for a slow line (feed.rs); a test waits this long at most.
const MAX_TIMEOUT_SECONDS: i32 = 120;

fn stop_requested(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

// A HopFn over a plain socket: HTTP/1.1, one request a connection, no redirect followed
// (make_fetch follows them and rebuilds the headers for each hop, which is the code under test).
// The body ends at Content-Length, or at the end of the connection when the answer names no
// length; a transfer coding is not spoken (the fake feed never uses one).
fn hop_over_loopback(
    routes: &Routes,
    hop: &HttpHop,
    stop: Option<&AtomicBool>,
    progress: Option<&dyn Fn(u64, u64)>,
) -> Result<HttpHopReply> {
    if !is_fetchable_url(&hop.url, true) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "this address cannot be fetched",
        ));
    }
    let Some(address) = parse_address(&hop.url) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "this address cannot be fetched by the test client",
        ));
    };
    let port = if address.https {
        match routes.get(&address.host) {
            Some(&port) if address.port == 0 => port,
            _ => {
                return Err(no_answer(
                    "the test client reaches the loopback address only",
                    &format!("no --connect names the host {}", address.host),
                ));
            }
        }
    } else {
        if address.host != "127.0.0.1" {
            return Err(no_answer(
                "the test client reaches the loopback address only",
                "an http address must spell 127.0.0.1",
            ));
        }
        if address.port == 0 { 80 } else { address.port }
    };
    if stop_requested(stop) {
        return Err(cancelled());
    }

    let mut connection =
        match Connection::open(port, hop.timeout_seconds.clamp(1, MAX_TIMEOUT_SECONDS)) {
            Ok(connection) => connection,
            Err(error) => {
                return Err(no_answer(
                    "no connection to the feed",
                    &format!("os_error={}", os_error(&error)),
                ));
            }
        };
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\n",
        address.target, address.authority
    );
    if !hop.user_agent.is_empty() {
        request.push_str(&format!("User-Agent: {}\r\n", hop.user_agent));
    }
    request.push_str("Accept-Encoding: identity\r\nConnection: close\r\n");
    // hop_headers built these, the token among them when the rule allows it: sent as they are, so
    // that what the feed logs is what the engine decided.
    for header in &hop.headers {
        request.push_str(header);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    if let Err(error) = connection.stream.write_all(request.as_bytes()) {
        return Err(no_answer(
            "the request could not be sent",
            &format!("os_error={}", os_error(&error)),
        ));
    }

    let mut data: Vec<u8> = Vec::new(); // the head, then the body
    let mut body_start: Option<usize> = None;
    let mut head = Head::default();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if let Some(start) = body_start {
            let body = (data.len() - start) as u64;
            if let Some(progress) = progress {
                progress(body, head.content_length.unwrap_or(0));
            }
            if head.content_length.is_some_and(|length| body >= length) {
                break; // all of it
            }
            if body > hop.max_bytes {
                break; // more than was asked for: the transfer is abandoned here
            }
        }
        if stop_requested(stop) {
            return Err(cancelled());
        }
        let got = match connection.stream.read(&mut buffer) {
            Ok(got) => got,
            Err(error) => {
                return Err(no_answer(
                    "the feed did not answer in time, or the connection failed",
                    &format!("os_error={}", os_error(&error)),
                ));
            }
        };
        if got == 0 {
            break; // the feed closed the connection
        }
        data.extend_from_slice(&buffer[..got]);
        if body_start.is_none() {
            let Some(blank) = find(&data, b"\r\n\r\n") else {
                if data.len() > MAX_HEAD_BYTES {
                    return Err(no_answer(
                        "the feed's answer is not HTTP",
                        "no end of the headers",
                    ));
                }
                continue;
            };
            let Some(answered) = parse_head(&data[..blank]) else {
                return Err(no_answer(
                    "the feed's answer is not HTTP",
                    "a malformed status or length",
                ));
            };
            if answered.transfer_encoded {
                return Err(no_answer(
                    "the feed's answer uses a transfer coding the test client does not speak",
                    "Transfer-Encoding",
                ));
            }
            head = answered;
            body_start = Some(blank + 4);
        }
    }
    let Some(start) = body_start else {
        return Err(no_answer(
            "the connection closed before an answer arrived",
            &format!("{} bytes", data.len()),
        ));
    };

    let success = head.status == 200 || head.status == 206;
    let length = head.content_length;
    let mut reply = HttpHopReply {
        status: head.status,
        content_type: head.content_type,
        location: head.location,
        content_range: head.content_range,
        body: data[start..].to_vec(),
        overran: false,
    };
    if let Some(length) = length
        && reply.body.len() as u64 > length
    {
        reply.body.truncate(length as usize);
    }
    if reply.body.len() as u64 > hop.max_bytes {
        if success {
            // Not the answer, and never handed on (feed.rs, HttpHopReply::overran): the status and
            // the content type say what kind of wrong it is.
            reply.overran = true;
            reply.body.clear();
        } else {
            // An error's page cut short is still that error's page.
            reply.body.truncate(hop.max_bytes as usize);
        }
    } else if success
        && let Some(length) = length
        && (reply.body.len() as u64) < length
    {
        // The connection dropped inside the body: no answer, as libcurl's "transfer closed with
        // outstanding read data remaining" is none for the hop the programs use.
        return Err(no_answer(
            "the connection closed before the whole answer had arrived",
            &format!("{} of {length} bytes", reply.body.len()),
        ));
    }
    Ok(reply)
}

// ---- the command line ---------------------------------------------------------------------------

const COMMANDS: [&str; 6] = ["check", "plan", "stage", "preflight", "launch", "recover"];
const VALUE_OPTIONS: [&str; 9] = [
    "--root",
    "--state",
    "--feed",
    "--github",
    "--token",
    "--connect",
    "--stage",
    "--relaunch",
    "--wait-pid",
];

#[derive(Debug, Default)]
struct Arguments {
    command: String,
    // An option that takes a value, once.
    values: BTreeMap<String, Vec<u8>>,
    // --connect, any number of times.
    routes: Routes,
    even_if_not_newer: bool,
}

impl Arguments {
    fn has(&self, option: &str) -> bool {
        self.values.contains_key(option)
    }

    fn value(&self, option: &str) -> &[u8] {
        self.values
            .get(option)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn text(&self, option: &str) -> String {
        text_of(self.value(option))
    }
}

fn usage_error(message: &str, context: impl Into<String>) -> Error {
    make_error(ErrorCode::InvalidArgument, message, context)
}

// `words` is the command line without argv[0].
fn parse_arguments(words: &[Vec<u8>]) -> Result<Arguments> {
    let mut arguments = Arguments::default();
    let Some(first) = words.first() else {
        return Err(usage_error("no command: say what to do", ""));
    };
    arguments.command = text_of(first);
    if !COMMANDS.contains(&arguments.command.as_str()) {
        return Err(usage_error("unknown command", utf8_or_windows1252(first)));
    }
    let mut i = 1;
    while i < words.len() {
        let option = text_of(&words[i]);
        if option == "--even-if-not-newer" {
            arguments.even_if_not_newer = true;
            i += 1;
            continue;
        }
        if !VALUE_OPTIONS.contains(&option.as_str()) {
            return Err(usage_error(
                "unknown option",
                utf8_or_windows1252(&words[i]),
            ));
        }
        if i + 1 >= words.len() {
            return Err(usage_error("option without a value", option));
        }
        let value = &words[i + 1];
        i += 2;
        if option == "--connect" {
            // HOST=PORT: the loopback port that stands in for an https host.
            let equals = value.iter().position(|&c| c == b'=');
            let digits = &value[equals.map_or(0, |at| at + 1)..];
            let port = whole_int(digits)
                .and_then(|port| u16::try_from(port).ok())
                .filter(|&port| port >= 1);
            match (equals, port) {
                (Some(equals), Some(port)) if equals != 0 => {
                    let host = text_of(&lowered(&value[..equals]));
                    arguments.routes.insert(host, port);
                }
                _ => {
                    return Err(usage_error(
                        "--connect takes HOST=PORT",
                        utf8_or_windows1252(value),
                    ));
                }
            }
            continue;
        }
        match arguments.values.entry(option) {
            Entry::Occupied(given) => {
                return Err(usage_error("option given twice", given.key().clone()));
            }
            Entry::Vacant(free) => {
                free.insert(value.clone());
            }
        }
    }

    for required in ["--root", "--state"] {
        if arguments.value(required).is_empty() {
            return Err(usage_error("missing option", required));
        }
    }
    let asks_the_feed = ["check", "plan", "stage"].contains(&arguments.command.as_str());
    if asks_the_feed {
        if arguments.has("--feed") == arguments.has("--github") {
            return Err(usage_error("give --feed or --github, and not both", ""));
        }
        if arguments.has("--feed") {
            // The one kind of feed this program fetches from by itself (feed.rs: plain http is let
            // through on the loopback only); anything else needs --github and --connect.
            let feed = arguments.text("--feed");
            let loopback = parse_address(&feed).is_some_and(|address| address.host == "127.0.0.1");
            if !is_loopback_http_url(&feed) || !is_fetchable_url(&feed, true) || !loopback {
                return Err(usage_error(
                    "--feed is an http address on the loopback: http://127.0.0.1:<port>",
                    feed,
                ));
            }
        }
    } else if arguments.has("--feed")
        || arguments.has("--github")
        || arguments.has("--token")
        || !arguments.routes.is_empty()
        || arguments.even_if_not_newer
    {
        return Err(usage_error(
            "this command asks no feed",
            arguments.command.clone(),
        ));
    }
    let needs_the_stage = ["preflight", "launch"].contains(&arguments.command.as_str());
    if needs_the_stage != arguments.has("--stage") {
        return Err(usage_error(
            if needs_the_stage {
                "missing option"
            } else {
                "this command takes no stage folder"
            },
            "--stage",
        ));
    }
    if arguments.command != "launch" && (arguments.has("--relaunch") || arguments.has("--wait-pid"))
    {
        return Err(usage_error(
            "only launch takes --relaunch and --wait-pid",
            arguments.command.clone(),
        ));
    }
    Ok(arguments)
}

// An option that names a folder: absolute, because the helper is told these very paths and a
// relative one would depend on a working folder nobody chose.
fn absolute_folder(arguments: &Arguments, option: &str) -> Result<PathBuf> {
    let path = path_from_utf8(arguments.value(option));
    if !path.is_absolute() {
        return Err(usage_error(
            &format!("{option} must be an absolute path"),
            utf8_or_windows1252(arguments.value(option)),
        ));
    }
    Ok(path)
}

// ---- what the commands share --------------------------------------------------------------------

fn say_install(install: &Install) {
    say("target", install.target.as_str());
    say("kind", katana_update::install::to_string(install.kind));
    say(
        "installed",
        katana_update::version::to_string(&install.version),
    );
    say_number("installed_sequence", install.sequence);
}

// The staged release, read back the way `UPDATE STATUS` and the helper read it: through the
// signature check, with the test trust.
fn read_staged_manifest(stage: &Path, install: &Install) -> Result<Manifest> {
    // The token first in the context, where fetch_failure_reason reads it: the helper's own word
    // for a stage it will not apply (apply.rs).
    let refused = |why: Error| {
        make_error(
            why.code,
            why.message,
            format!("reason=staged_manifest; {}", why.context),
        )
    };
    let json = read_file_bytes(&staged_manifest_path(stage)).map_err(refused)?;
    let signature = read_file_bytes(&staged_signature_path(stage)).map_err(refused)?;
    verify_manifest(json, signature, test_trust(), install).map_err(refused)
}

// stage()'s progress, held to what stage.rs promises of it.
struct ProgressWatch {
    reports: Cell<u64>,
    done: Cell<u64>,
    total: Cell<u64>,
    steady: Cell<bool>,
}

impl ProgressWatch {
    fn new() -> Self {
        Self {
            reports: Cell::new(0),
            done: Cell::new(0),
            total: Cell::new(0),
            steady: Cell::new(true),
        }
    }

    fn see(&self, now_done: u64, now_total: u64) {
        if now_done < self.done.get()
            || now_done > now_total
            || (self.reports.get() > 0 && now_total != self.total.get())
        {
            self.steady.set(false);
        }
        self.reports.set(self.reports.get() + 1);
        self.done.set(now_done);
        self.total.set(now_total);
    }
}

// ---- check, plan, stage -------------------------------------------------------------------------

fn run_feed_command(arguments: &Arguments, install: &Install, state: &Path) -> i32 {
    let mut feed = default_feed_config(&install.target);
    if arguments.has("--feed") {
        feed.feed_base = arguments.text("--feed");
        // "a base URL without a trailing '/'" (feed.rs)
        while feed.feed_base.ends_with('/') {
            feed.feed_base.pop();
        }
    } else {
        feed.feed_base.clear();
        feed.repo = arguments.text("--github");
    }
    feed.token = arguments.text("--token");

    let routes = arguments.routes.clone();
    let fetch = make_fetch(hop_fn(move |hop, stop, progress| {
        hop_over_loopback(&routes, hop, stop, progress)
    }));

    let mut result = match check(&fetch, &feed, install, test_trust(), None) {
        Ok(result) => result,
        Err(error) => return say_error(&error),
    };
    // What the person decided and what failed here before, as the window applies it. A state file
    // that cannot be read is the defaults (state.rs), and said.
    match read_state(state) {
        Ok(recorded) => apply_update_state(&mut result, &recorded),
        Err(error) => say("state_error", error.describe()),
    }
    say("update", katana_update::check::to_string(result.status));
    say("latest", result.latest_version.as_str());
    say_number(
        "sequence",
        result
            .manifest
            .as_ref()
            .map_or(0, |manifest| manifest.sequence),
    );
    say("reason", result.reason.as_str());
    say(
        "route",
        if result.route == FeedRoute::Api {
            "api"
        } else {
            "download"
        },
    );
    say("notes_url", result.notes_url.as_str());
    say("detail", result.detail.as_str());
    if arguments.command == "check" {
        return EXIT_OK;
    }

    let offered = result.status == CheckStatus::Available;
    let forced = arguments.even_if_not_newer
        && result.manifest.is_some()
        && result.status == CheckStatus::Current;
    if !offered && !forced {
        if arguments.command == "stage" {
            say("staged", "none");
        }
        return EXIT_OK;
    }
    let Some(manifest) = result.manifest.as_ref() else {
        unreachable!("check() sets the manifest of every answer that offers a release (check.rs)");
    };
    let planned = match plan(manifest, install, None) {
        Ok(planned) => planned,
        Err(error) => return say_error(&error),
    };
    say(
        "files",
        format!("{}/{}", planned.changed_files, planned.total_files),
    );
    say_number("download_bytes", planned.download_bytes);
    say_number("changed_bytes", planned.changed_bytes);
    say_number("required_free_bytes", planned.required_free_bytes);
    for file in &planned.files {
        if file.changed && file.index < manifest.files.len() {
            say("changed", manifest.files[file.index].path.as_str());
        }
    }
    for range in &planned.ranges {
        say("range", format!("{}+{}", range.offset, range.length));
    }
    if arguments.command == "plan" {
        return EXIT_OK;
    }

    // The payload is fetched the way the manifest was (check.rs, CheckResult::route).
    feed.route = result.route;
    let stage_directory = stage_path(state, manifest.sequence);
    say("stage", path_to_utf8(&stage_directory));
    let watch = ProgressWatch::new();
    let progress: &dyn Fn(u64, u64) = &|done, total| watch.see(done, total);
    let staged = stage(
        &fetch,
        &feed,
        manifest,
        &planned,
        &stage_directory,
        None,
        Some(progress),
    );
    say(
        "progress",
        format!("{}/{}", watch.done.get(), watch.total.get()),
    );
    say_number("progress_reports", watch.reports.get());
    say(
        "progress_steady",
        if watch.steady.get() { "true" } else { "false" },
    );
    if let Err(error) = staged {
        return say_error(&error);
    }
    say(
        "staged",
        katana_update::version::to_string(&manifest.version),
    );
    EXIT_OK
}

// ---- preflight, launch --------------------------------------------------------------------------

fn run_apply_command(arguments: &Arguments, install: &Install, state: &Path) -> i32 {
    let stage = match absolute_folder(arguments, "--stage") {
        Ok(stage) => stage,
        Err(error) => return usage_failure(&error),
    };
    // Read before anything is started, so that a wrong value starts nothing.
    let mut request = ApplyRequest {
        root: install.root.clone(),
        stage_directory: stage.clone(),
        state_directory: state.to_path_buf(),
        relaunch: installed_program(&install.root, "katana"),
        ..ApplyRequest::default()
    };
    if arguments.has("--relaunch") {
        let relaunch = arguments.value("--relaunch");
        request.relaunch = if relaunch == b"none" {
            PathBuf::new()
        } else {
            path_from_utf8(relaunch)
        };
        if !request.relaunch.as_os_str().is_empty() && !request.relaunch.is_absolute() {
            write_error(
                format!("katana_update_e2e: --relaunch must be an absolute path or none\n{USAGE}")
                    .as_bytes(),
            );
            return EXIT_USAGE;
        }
    }
    // The window's own flow: the helper waits for the program that started it to be gone.
    request.wait_pid = make_system_process_ops().current_pid();
    let wait_pid = arguments.value("--wait-pid");
    if !wait_pid.is_empty() && wait_pid != b"self" {
        match whole_u64(wait_pid) {
            Some(pid) => request.wait_pid = pid,
            None => {
                write_error(
                    format!("katana_update_e2e: --wait-pid takes a process id or self\n{USAGE}")
                        .as_bytes(),
                );
                return EXIT_USAGE;
            }
        }
    }

    let manifest = match read_staged_manifest(&stage, install) {
        Ok(manifest) => manifest,
        Err(error) => return say_error(&error),
    };
    say("stage", path_to_utf8(&stage));
    say(
        "version",
        katana_update::version::to_string(&manifest.version),
    );

    let launcher = make_system_launcher();
    let prepared = match preflight(&manifest, install, &stage, state, launcher.as_ref()) {
        Ok(prepared) => prepared,
        Err(error) => return say_error(&error),
    };
    say("transaction", prepared.transaction.as_str());
    say("run", path_to_utf8(&prepared.run_directory));
    say("helper", path_to_utf8(&prepared.helper));
    if arguments.command == "preflight" {
        return EXIT_OK;
    }

    request.helper = prepared.helper.clone();
    request.transaction = prepared.transaction.clone();
    say_number("wait_pid", request.wait_pid);
    say(
        "relaunch",
        if request.relaunch.as_os_str().is_empty() {
            b"none".to_vec()
        } else {
            path_to_utf8(&request.relaunch)
        },
    );
    // Every line is flushed before the helper exists (write_out): it waits for this process to be
    // gone, and whoever reads these lines must not wait for the helper.
    if let Err(error) = launch_apply_with(&request, launcher.as_ref()) {
        return say_error(&error);
    }
    say("launched", request.transaction.as_str());
    EXIT_OK
}

// ---- recover ------------------------------------------------------------------------------------

// std::filesystem::equivalent: both are there and are one file (one folder) by the file system's
// identity, whatever their spelling; false when either is not there, as equivalent then reports an
// error. POSIX compares the device and the inode; on Windows the standard library has no stable
// form of the file's index, and the two final paths the system resolves are compared instead.
fn equivalent(first: &Path, second: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(first), std::fs::metadata(second)) {
            (Ok(one), Ok(other)) => one.dev() == other.dev() && one.ino() == other.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (std::fs::canonicalize(first), std::fs::canonicalize(second)) {
            (Ok(one), Ok(other)) => one == other,
            _ => false,
        }
    }
}

fn run_recover(install: &Install, state: &Path) -> i32 {
    // recover() finds the helper copy through state_directory(), not through an argument. Held to
    // the folder this run was given, so that a test which forgot the variable does not go looking
    // in the person's own update folder.
    let library = state_directory();
    if library.as_os_str().is_empty() || !equivalent(&library, state) {
        write_error(
            b"katana_update_e2e: recover needs KATANA_UPDATE_STATE_DIR to name the folder given \
              as --state\n",
        );
        return EXIT_USAGE;
    }
    if let Err(error) = recover(&install.root) {
        return say_error(&error);
    }
    say("recover", "ok");
    EXIT_OK
}

fn run() -> i32 {
    let mut words = utf8_arguments();
    if !words.is_empty() {
        words.remove(0);
    }
    let arguments = match parse_arguments(&words) {
        Ok(arguments) => arguments,
        Err(error) => return usage_failure(&error),
    };
    let (root, state) = match (
        absolute_folder(&arguments, "--root"),
        absolute_folder(&arguments, "--state"),
    ) {
        (Ok(root), Ok(state)) => (root, state),
        (Err(error), _) | (_, Err(error)) => return usage_failure(&error),
    };

    let install = match read_install(&root, &NoRegistry) {
        Ok(install) => install,
        Err(error) => return say_error(&error),
    };
    say_install(&install);

    match arguments.command.as_str() {
        "recover" => run_recover(&install, &state),
        "preflight" | "launch" => run_apply_command(&arguments, &install, &state),
        _ => run_feed_command(&arguments, &install, &state),
    }
}

// What a panic says, as the C++'s what() says what an exception does.
fn panic_text(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    String::from("an unexpected error")
}

fn main() -> ExitCode {
    // Standard error says what the C++ says there and no more: a panic is caught where the C++
    // catches an exception, and said below.
    std::panic::set_hook(Box::new(|_| {}));
    let code = match catch_unwind(AssertUnwindSafe(run)) {
        Ok(code) => code,
        Err(payload) => {
            write_error(
                format!(
                    "katana_update_e2e: unexpected error: {}\n",
                    panic_text(payload.as_ref())
                )
                .as_bytes(),
            );
            EXIT_INTERNAL
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(u8::MAX))
}
