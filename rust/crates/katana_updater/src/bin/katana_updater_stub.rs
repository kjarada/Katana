//! A stand-in for the person's Katana, for the tests that drive the real helper program
//! (tests/helper.rs, and the end-to-end driver tests/update/update_e2e.py). Never installed,
//! never shipped. The twin of tests/update/updater_stub.cpp.
//!
//! The helper starts `R/bin/katana.exe --update-health <tx>` to prove the new version starts, and
//! starts `R/bin/katana.exe` again at the end. The test builds an install whose katana.exe IS this
//! program (plus a tag appended to the file, so that v1 and v2 differ and each can tell which it
//! is), and says by environment what it should do:
//!
//! ```text
//!   KATANA_STUB_LOG              file to append one line to for every start (what ran, with what)
//!   KATANA_STUB_MODE             for --update-health: healthy (default: write the marker, exit 0),
//!                                exit1 (exit 1 without the marker), exit0 (exit 0 without it: a
//!                                start that ends well and never says it started), exit1_when_told
//!                                (exit 1 without the marker once the test has made the file
//!                                <KATANA_STUB_LOG>.go: until then the new version is running, and
//!                                the test can do something to the install for as long as it needs -
//!                                a pause of a fixed length would be a race against a busy machine),
//!                                marker_then_exit3, silent (never write it and never return: the
//!                                helper's timeout ends it)
//!   KATANA_UPDATE_STATE_DIR      where the marker goes, <dir>\healthy-<tx> (state.rs)
//!   KATANA_STUB_REGISTRY_BASE    an HKCU subkey: the line says whether the RunOnce value of the
//!                                helper is there at that moment (it must be, during the health
//!                                check, and gone afterwards)
//!   KATANA_STUB_MACHINE_RUNONCE  set to anything: the line also says whether the helper's value
//!                                is in the MACHINE's own RunOnce key at that moment (it is only
//!                                read; a test helper must never have put it there)
//!   KATANA_STUB_PID_FILE         file to write this process's id to, as decimal digits and a line
//!                                feed, before anything else: how a test learns which process the
//!                                Launcher started for it (the Launcher hands back no id)
//!
//!   --sleep <ms>                 stay running that long: a process whose image is under the
//!                                install, for the helper's scan to find
//! ```
//!
//! Each log line is
//!
//! ```text
//!   <kind> tag=<tag> exe=<path> cwd=<folder> OPENSSL_CONF=<value or -> [runonce=<value or ->]
//!   [machine_runonce=<value or ->] [tx=<tx> mode=<mode>]
//! ```
//!
//! UTF-8; the kind is health, sleep or start. The tag is read from the end of this very file: the
//! text KATANA-STUB-TAG= and the letters, digits, dots and hyphens that follow it, at its LAST
//! occurrence. A copy nobody tagged finds only this program's own literal, which nothing of that
//! kind follows: its tag is "none" (the literal is followed by a NUL, as a C++ string literal is,
//! because a Rust literal may sit against the next in the file).
//!
//! It is a REAL program on purpose. A test must never start a file that is not one (a text file
//! named .exe): Windows answers that with a message box somebody has to click.
//!
//! Windows only, as the C++ is (it refuses to compile elsewhere): the helper's process scan and
//! registry are. Cargo builds every binary of a package on every platform, so elsewhere this
//! program says so and exits 1. The Windows part is written, not compiled, until the port is
//! built on Windows (docs/rust.md, step 8).

#[cfg(windows)]
mod stub {
    #![allow(
        unsafe_code,
        reason = "the helper's RunOnce value is read through RegGetValueW, a foreign function"
    )]

    use std::ffi::{OsStr, OsString};
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::Path;
    use std::ptr::null_mut;
    use std::time::Duration;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegGetValueW};

    // The values of the Windows SDK's headers (winerror.h, winnt.h, winreg.h).
    const ERROR_SUCCESS: u32 = 0;
    const FILE_SHARE_READ: u32 = 0x1;
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;

    // The longest a health check of this program stays when nothing ends it (the modes silent and
    // exit1_when_told), in milliseconds: two minutes, which is what a test gives a whole run of
    // the helper (tests/helper.rs, PATIENCE). A test that passes never sees them out: the helper's
    // health wait ends a silent start, and the test's file ends the other. They are for the run
    // that went wrong, so that a stub nobody waits for any more still goes.
    const LONGEST_STAY: u32 = 120_000;
    // How often exit1_when_told looks for its file, in milliseconds.
    const LOOK_EVERY: u32 = 20;

    // The key the tag follows, with the NUL that ends it in this file (see the top).
    const TAG_KEY: &[u8] = b"KATANA-STUB-TAG=\0";

    fn utf8(text: &OsStr) -> String {
        text.to_string_lossy().into_owned()
    }

    // A variable of the environment, or empty when it is not set or set to nothing (which Windows
    // reads as not set).
    fn environment(name: &str) -> OsString {
        std::env::var_os(name).unwrap_or_default()
    }

    // The tag after the last KATANA-STUB-TAG= in this program's own file (see the top).
    fn own_tag(own: &Path) -> String {
        // std's File::open shares reading, writing and deleting, as the C++ opens the file.
        let Ok(bytes) = std::fs::read(own) else {
            return String::from("unreadable");
        };
        let key = &TAG_KEY[..TAG_KEY.len() - 1];
        let Some(at) = bytes.windows(key.len()).rposition(|window| window == key) else {
            return String::from("none");
        };
        let begin = at + key.len();
        let part_of_a_tag = |c: u8| c.is_ascii_alphanumeric() || c == b'.' || c == b'-';
        let mut end = begin;
        while end < bytes.len() && end - begin < 32 && part_of_a_tag(bytes[end]) {
            end += 1;
        }
        if end == begin {
            return String::from("none");
        }
        String::from_utf8_lossy(&bytes[begin..end]).into_owned()
    }

    fn append_line(path: &OsStr, line: &str) {
        if path.is_empty() {
            return;
        }
        // FILE_APPEND_DATA, shared for everything, created when absent: the C++'s CreateFileW.
        let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) else {
            return;
        };
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }

    // The RunOnce value of the helper under HKCU\<base> - under HKCU itself for an empty base - or
    // "-" when it is not there.
    fn run_once_value(base: &OsStr) -> String {
        let mut key: Vec<u16> = base.encode_wide().collect();
        if !key.is_empty() {
            key.push(u16::from(b'\\'));
        }
        key.extend(r"Software\Microsoft\Windows\CurrentVersion\RunOnce".encode_utf16());
        key.push(0);
        let name: Vec<u16> = "KatanaUpdateRecover".encode_utf16().chain([0]).collect();
        let mut value = vec![0u16; 1024];
        let mut bytes = (value.len() * 2) as u32;
        // SAFETY: the key and the name are NUL-terminated; `value` holds `bytes` bytes, which the
        // call writes at most; the type pointer may be null.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                value.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            return String::from("-");
        }
        value.truncate(bytes as usize / 2);
        while value.last() == Some(&0) {
            value.pop();
        }
        utf8(&OsString::from_wide(&value))
    }

    // The process id, into the file a test named (see the top): one write of the whole text, so a
    // reader that sees the line feed has the whole number. Written, and shared for reading only,
    // as the C++ creates it.
    fn write_own_process_id(path: &OsStr) {
        if path.is_empty() {
            return;
        }
        let Ok(mut file) = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
        else {
            return;
        };
        let _ = file.write_all(format!("{}\n", std::process::id()).as_bytes());
    }

    // std::stoul of the C++, read into the 32 bits a DWORD has: leading blanks, a sign, digits, and
    // whatever follows the digits ignored; no digits, or a number past the type, throws there,
    // which ends the program abnormally.
    fn parse_milliseconds(text: &str) -> u32 {
        let text = text.trim_start();
        let (negative, digits) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        let count = digits.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(value) = digits[..count].parse::<u32>() else {
            std::process::abort();
        };
        if negative {
            value.wrapping_neg()
        } else {
            value
        }
    }

    fn sleep(milliseconds: u32) {
        std::thread::sleep(Duration::from_millis(u64::from(milliseconds)));
    }

    pub fn main() -> i32 {
        write_own_process_id(&environment("KATANA_STUB_PID_FILE"));
        let log_path = environment("KATANA_STUB_LOG");
        let mode = utf8(&environment("KATANA_STUB_MODE"));
        let openssl = environment("OPENSSL_CONF");
        let registry_base = environment("KATANA_STUB_REGISTRY_BASE");

        // The words as the C runtime hands them to the C++'s main (the stub is only ever given
        // ASCII ones).
        let arguments: Vec<String> = std::env::args_os().map(|word| utf8(&word)).collect();
        let first = arguments.get(1).map_or("", String::as_str);
        let own = std::env::current_exe().unwrap_or_default();
        let folder = std::env::current_dir().unwrap_or_default();

        let mut line = String::from(match first {
            "--update-health" => "health",
            "--sleep" => "sleep",
            _ => "start",
        });
        line.push_str(&format!(
            " tag={} exe={} cwd={} OPENSSL_CONF={}",
            own_tag(&own),
            utf8(own.as_os_str()),
            utf8(folder.as_os_str()),
            if openssl.is_empty() {
                String::from("-")
            } else {
                utf8(&openssl)
            }
        ));
        if !registry_base.is_empty() {
            line.push_str(&format!(" runonce={}", run_once_value(&registry_base)));
        }
        if !environment("KATANA_STUB_MACHINE_RUNONCE").is_empty() {
            line.push_str(&format!(
                " machine_runonce={}",
                run_once_value(OsStr::new(""))
            ));
        }
        let health_tx = if first == "--update-health" {
            arguments.get(2)
        } else {
            None
        };
        if let Some(tx) = health_tx {
            line.push_str(&format!(
                " tx={tx} mode={}",
                if mode.is_empty() {
                    "healthy"
                } else {
                    mode.as_str()
                }
            ));
        }
        append_line(&log_path, &line);

        if first == "--sleep"
            && let Some(time) = arguments.get(2)
        {
            sleep(parse_milliseconds(time));
            return 0;
        }
        if let Some(tx) = health_tx {
            match mode.as_str() {
                "exit1" => return 1,
                "exit0" => return 0,
                "exit1_when_told" => {
                    // Told by a file, which is looked for until it is there. Not for ever: a test
                    // that died before it made the file must not leave this process behind. With
                    // no log named there is no file to be told by, and the exit is at once.
                    let mut told = log_path.clone();
                    told.push(".go");
                    let mut waited: u32 = 0;
                    while !log_path.is_empty()
                        && waited < LONGEST_STAY
                        && std::fs::symlink_metadata(&told).is_err()
                    {
                        sleep(LOOK_EVERY);
                        waited += LOOK_EVERY;
                    }
                    return 1;
                }
                "silent" => {
                    sleep(LONGEST_STAY);
                    return 0;
                }
                _ => {}
            }
            let state = environment("KATANA_UPDATE_STATE_DIR");
            if !state.is_empty() {
                let mut marker = state;
                marker.push(format!("\\healthy-{tx}"));
                if let Ok(mut file) = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .share_mode(FILE_SHARE_READ)
                    .open(&marker)
                {
                    let _ = file.write_all(b"ok\n");
                }
            }
            return if mode == "marker_then_exit3" { 3 } else { 0 };
        }
        0
    }
}

fn main() {
    #[cfg(windows)]
    {
        std::process::exit(stub::main());
    }
    #[cfg(not(windows))]
    {
        let _ = std::io::Write::write_all(
            &mut std::io::stderr(),
            b"the stub is for Windows only: the helper's process scan and registry are\n",
        );
        std::process::exit(1);
    }
}
