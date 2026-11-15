//! The credential stores: the Windows Credential Manager (credential_win.rs), the environment
//! variable (credential.rs), and redact_token (the twin of tests/update/test_credential.cpp).
//!
//! The Credential Manager cases run against the REAL one, under a target name made unique for the
//! process and the case ("Katana/update-token-test-<pid>-<tick>"), never the name a person's token
//! is kept under. They read what the store wrote with raw CredReadW and write what the store reads
//! with raw CredWriteW and with cmdkey, so the format is held to the system and not to the store's
//! own idea of it. Whatever happens in a case, the credential is deleted afterwards (the fixture's
//! drop); a run that is killed cannot do that, so the first case of the next run deletes the
//! leftovers of processes that have ended (sweep_orphaned_test_credentials). A machine whose
//! Credential Manager cannot be used (a service session with no profile, a stripped image) SKIPS
//! with the system's reason instead of failing.
//!
//! Expected values are worked out by hand: UTF-8 bytes from the code points (U+00E9 is C3 A9,
//! U+65E5 is E6 97 A5, U+672C is E6 9C AC, U+1F511 is F0 9F 94 91), the 2560-byte limit by asking
//! the system itself, redaction by counting the occurrences in a short string.

#![allow(
    unsafe_code,
    reason = "Edition 2024 marks changing the environment unsafe, and the Windows cases call the Credential Manager directly."
)]

use katana_core::Error;

// "tok" + e acute + "n-" + two CJK characters + "-" + a key emoji, as the UTF-8 bytes worked out
// above: 3 + 2 + 2 + 6 + 1 + 4 = 18 bytes.
const NON_ASCII_TOKEN: &str = "tok\u{e9}n-\u{65e5}\u{672c}-\u{1f511}";

// A token that no message may contain: its own text is what the checks look for.
const SECRET: &str = "SECRETVALUE";

// `size` bytes of a token that starts with SECRET, so a message that quotes any of it is caught.
fn token_of_size(size: usize) -> String {
    let mut token = String::new();
    while token.len() < size {
        token.push_str(SECRET);
        token.push('-');
    }
    token.truncate(size);
    token
}

fn mentions_secret(error: &Error) -> bool {
    error.describe().contains(SECRET)
}

// The bytes a token must not hold: the C0 controls 0x00-0x1F and DEL 0x7F, listed from the ASCII
// table and not from the rule under test. 0x00 is left out when `with_nul` is false (an
// environment variable ends at it).
fn control_bytes(with_nul: bool) -> Vec<u8> {
    let first = if with_nul { 0x00 } else { 0x01 };
    let mut bytes: Vec<u8> = (first..=0x1f).collect();
    bytes.push(0x7f);
    bytes
}

// The bytes a token may hold and the system does not trim: 0x21 '!' to 0x7E '~', every visible
// ASCII character (0x20 is a space, which a token may not hold inside it).
fn visible_bytes() -> Vec<u8> {
    (0x21..=0x7e).collect()
}

// The six bytes that are white space around a token and no part of one: space, then 0x09-0x0D
// (tab, line feed, vertical tab, form feed, carriage return).
const BLANKS: &[u8] = b" \t\n\x0b\x0c\r";

// For a failure message inside a loop over bytes.
fn byte_name(byte: u8) -> String {
    format!("byte {byte} (decimal)")
}

// The UTF-8 bytes of a code point below U+10000, from the encoding's own table and not from the
// code under test: U+0080-U+07FF is 110xxxxx 10xxxxxx, U+0800-U+FFFF is 1110xxxx 10xxxxxx
// 10xxxxxx (U+00A0 is C2 A0, U+2003 is E2 80 83, U+FEFF is EF BB BF).
fn utf8_of(code_point: u32) -> Vec<u8> {
    if code_point < 0x800 {
        vec![
            (0xc0 | (code_point >> 6)) as u8,
            (0x80 | (code_point & 0x3f)) as u8,
        ]
    } else {
        vec![
            (0xe0 | (code_point >> 12)) as u8,
            (0x80 | ((code_point >> 6) & 0x3f)) as u8,
            (0x80 | (code_point & 0x3f)) as u8,
        ]
    }
}

// The code points beyond ASCII that are white space or invisible, and that a token copied from a
// web page, a chat or a PDF carries at its ends: Unicode's White_Space list (U+0085, U+00A0,
// U+1680, U+2000-U+200A, U+2028, U+2029, U+202F, U+205F, U+3000) and the zero-width and direction
// format characters (U+180E, U+200B-U+200F, U+202A-U+202E, U+2060-U+206F, U+FEFF), each range
// written out by its ends. (U+2065 is unassigned and U+206A-U+206F are deprecated format controls;
// they sit inside the one run from U+205F to U+206F.)
fn invisible_code_points() -> Vec<u32> {
    let mut code_points = vec![0x0085, 0x00a0, 0x1680, 0x180e, 0x3000, 0xfeff];
    for (first, last) in [(0x2000, 0x200f), (0x2028, 0x202f), (0x205f, 0x206f)] {
        code_points.extend(first..=last);
    }
    code_points
}

// Visible characters next to the ends of those ranges, which a token may hold anywhere: the
// inverted exclamation mark U+00A1, the Canadian syllabics letter U+167F and the Ogham letter
// U+1681 beside U+1680, the hyphen U+2010 after U+200F, the hyphenation point U+2027 before
// U+2028, the per mille sign U+2030 after U+202F, the vertical four dots U+205E before U+205F,
// superscript zero U+2070 after U+206F, the ideographic comma U+3001 after U+3000, the full-width
// exclamation mark U+FF01.
fn visible_neighbours() -> Vec<u32> {
    vec![
        0x00a1, 0x167f, 0x1681, 0x2010, 0x2027, 0x2030, 0x205e, 0x2070, 0x3001, 0xff01,
    ]
}

fn code_point_name(code_point: u32) -> String {
    format!("code point {code_point} (decimal)")
}

fn concatenated(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

// ---- the environment -------------------------------------------------------------------------

mod environment {
    use katana_core::path_text::{environment_variable, path_from_utf8};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    const NAME: &str = "KATANA_UPDATE_TOKEN";

    // The cases of this binary run on threads of one process, and the variable is the process's:
    // one case at a time changes it.
    static ENVIRONMENT: Mutex<()> = Mutex::new(());

    fn put(value: &[u8]) {
        // An empty value removes the variable on both platforms.
        if value.is_empty() {
            // SAFETY: the variable is changed only while ENVIRONMENT is held, and the standard
            // library's own lock orders it with every read through std::env.
            unsafe { std::env::remove_var(NAME) };
        } else {
            // SAFETY: as above. On Windows the UTF-8 is made the text itself (wide), not the code
            // page's reading of it, as the C++ test does with _wputenv_s.
            unsafe { std::env::set_var(NAME, path_from_utf8(value).as_os_str()) };
        }
    }

    // Sets KATANA_UPDATE_TOKEN for one case and puts back what was there (a developer's machine
    // may have a real one) - set in this process, never in anything a case starts.
    pub struct TokenVariable {
        original: Vec<u8>,
        _lock: MutexGuard<'static, ()>,
    }

    impl TokenVariable {
        pub fn saved() -> Self {
            let lock = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
            Self {
                original: environment_variable(Some(NAME)),
                _lock: lock,
            }
        }

        // The value as UTF-8 text (or bytes, where the platform keeps bytes).
        pub fn set(&self, value: impl AsRef<[u8]>) {
            put(value.as_ref());
        }

        pub fn unset(&self) {
            put(b"");
        }
    }

    impl Drop for TokenVariable {
        fn drop(&mut self) {
            put(&self.original);
        }
    }
}

mod credential_constants {
    use katana_update::credential::{CREDENTIAL_TARGET, TOKEN_ENVIRONMENT_VARIABLE};

    #[test]
    fn the_variable_and_the_target_are_the_names_the_documentation_promises() {
        assert_eq!(TOKEN_ENVIRONMENT_VARIABLE, "KATANA_UPDATE_TOKEN");
        assert_eq!(CREDENTIAL_TARGET, "Katana/update-token");
    }
}

mod environment_credential_store_test {
    use super::environment::TokenVariable;
    use super::*;
    use katana_core::ErrorCode;
    use katana_update::credential::{CredentialStore, make_environment_credential_store};

    struct Fixture {
        variable: TokenVariable,
        store: Box<dyn CredentialStore>,
    }

    fn fixture() -> Fixture {
        Fixture {
            variable: TokenVariable::saved(),
            store: make_environment_credential_store(),
        }
    }

    fn read_ok(store: &dyn CredentialStore) -> String {
        store
            .read()
            .unwrap_or_else(|error| panic!("{}", error.describe()))
    }

    #[test]
    fn reads_the_token_from_the_variable() {
        let f = fixture();
        f.variable
            .set("ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD");
        assert_eq!(
            read_ok(f.store.as_ref()),
            "ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD"
        );
        assert_eq!(f.store.r#where(), "environment");
    }

    #[test]
    fn reads_a_token_with_non_ascii_characters_exactly() {
        let f = fixture();
        f.variable.set(NON_ASCII_TOKEN);
        assert_eq!(read_ok(f.store.as_ref()), NON_ASCII_TOKEN);
    }

    #[test]
    fn drops_the_white_space_a_shell_leaves_around_the_value() {
        // `export KATANA_UPDATE_TOKEN="$(cat token.txt)"` keeps no newline, but a hand-edited
        // profile or a CI secret often does: the token is what is between.
        let f = fixture();
        f.variable.set(" \tabc123 \r\n");
        assert_eq!(read_ok(f.store.as_ref()), "abc123");
    }

    #[test]
    fn a_variable_that_is_unset_empty_or_only_white_space_is_no_token() {
        let f = fixture();
        f.variable.unset();
        assert_eq!(f.store.read().unwrap_err().code, ErrorCode::NotFound);

        f.variable.set("");
        assert_eq!(f.store.read().unwrap_err().code, ErrorCode::NotFound);

        f.variable.set("  \t ");
        assert_eq!(f.store.read().unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn a_line_break_inside_the_value_is_refused_not_sent_in_a_header() {
        // "abc\r\nX-Evil: 1" would end the Authorization header and begin another.
        let f = fixture();
        f.variable.set(format!("{SECRET}\r\nX-Evil: 1"));
        let error = f.store.read().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(!mentions_secret(&error), "{}", error.describe());
        // It says which variable to correct.
        assert!(error.message.contains("KATANA_UPDATE_TOKEN"));
    }

    #[test]
    fn a_space_inside_the_value_is_refused_and_says_to_leave_the_word_out() {
        // "Bearer <token>" pasted whole would be sent as "Authorization: Bearer Bearer <token>":
        // a 401 that names no cause. Refused here, with a sentence that does.
        let f = fixture();
        f.variable.set(format!("Bearer {SECRET}"));
        let error = f.store.read().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(!mentions_secret(&error), "{}", error.describe());
        assert!(error.message.contains("KATANA_UPDATE_TOKEN"));
        assert!(error.message.contains("space"), "{}", error.describe());
        assert!(error.message.contains("Bearer"), "{}", error.describe());

        // A space in the middle of a longer value is the same; one at the ends is trimmed
        // (above).
        f.variable.set(format!("{SECRET} {SECRET}"));
        let error = f.store.read().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(!mentions_secret(&error), "{}", error.describe());
    }

    #[test]
    fn every_control_character_inside_the_value_is_refused() {
        // 0x01-0x1F and 0x7F, each between two letters: an environment variable ends at 0x00, so
        // that one is the Credential Manager cases' (below). Every one would put a byte in an HTTP
        // header that it cannot carry, the line feed and the carriage return first. The
        // boundaries are the point: 0x1F and 0x0E-0x1B are not the "usual" line-break
        // characters, and 0x7F is outside the 0x00-0x1F run.
        let f = fixture();
        for byte in control_bytes(false) {
            f.variable
                .set(concatenated(&[SECRET.as_bytes(), &[byte], b"cd"]));
            let error = f.store.read().unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidState, "{}", byte_name(byte));
            assert!(!mentions_secret(&error), "{}", error.describe());
        }
    }

    #[test]
    fn a_control_character_at_an_end_is_refused_unless_it_is_one_of_the_six_blanks() {
        let f = fixture();
        for byte in control_bytes(false) {
            let blank = BLANKS.contains(&byte);
            for value in [
                concatenated(&[&[byte], SECRET.as_bytes()]),
                concatenated(&[SECRET.as_bytes(), &[byte]]),
                concatenated(&[&[byte], SECRET.as_bytes(), &[byte]]),
            ] {
                f.variable.set(&value);
                let read = f.store.read();
                if blank {
                    // Trimmed: the token is what is between.
                    let token =
                        read.unwrap_or_else(|error| panic!("{}: {}", byte_name(byte), error));
                    assert_eq!(token, SECRET, "{}", byte_name(byte));
                } else {
                    let error = read.unwrap_err();
                    assert_eq!(error.code, ErrorCode::InvalidState, "{}", byte_name(byte));
                    assert!(!mentions_secret(&error), "{}", error.describe());
                }
            }
        }
    }

    #[test]
    fn every_visible_ascii_character_inside_the_value_is_accepted() {
        // 0x21 '!' to 0x7E '~': base64 and URL-safe alphabets, punctuation, a quote, a percent
        // sign. A rule that read "below 0x7F" wrongly as "below 0x7E", or ">= 0x21" as "> 0x21",
        // fails here.
        let f = fixture();
        for byte in visible_bytes() {
            let value = format!("ab{}cd", char::from(byte));
            f.variable.set(&value);
            let token = f
                .store
                .read()
                .unwrap_or_else(|error| panic!("{}: {}", byte_name(byte), error));
            assert_eq!(token, value);
        }
    }

    #[test]
    fn each_blank_around_the_value_is_dropped_and_the_word_between_is_kept() {
        // The six white-space bytes of the C locale, one at a time at both ends.
        let f = fixture();
        for &blank in BLANKS {
            f.variable
                .set(concatenated(&[&[blank], b"abc123", &[blank]]));
            let token = f
                .store
                .read()
                .unwrap_or_else(|error| panic!("{}: {}", byte_name(blank), error));
            assert_eq!(token, "abc123");
        }
    }

    #[test]
    fn every_invisible_or_non_ascii_blank_around_the_value_is_dropped() {
        // A no-break space, a zero-width space, a direction mark or a byte-order mark comes along
        // with a token copied from a web page, a chat or a PDF; katana_core's trimmed knows only
        // ASCII, and the server would answer 401 with no cause.
        let f = fixture();
        for code_point in invisible_code_points() {
            let blank = utf8_of(code_point);
            f.variable.set(concatenated(&[&blank, b"abc123", &blank]));
            let token = f
                .store
                .read()
                .unwrap_or_else(|error| panic!("{}: {}", code_point_name(code_point), error));
            assert_eq!(token, "abc123");
        }
    }

    #[test]
    fn blanks_of_every_kind_around_the_value_are_dropped_in_any_order() {
        // Five at the front and four at the back, ASCII and not, interleaved: a pass of one kind
        // would leave the others. U+00A0 is C2 A0, U+FEFF is EF BB BF, U+200B is E2 80 8B.
        let f = fixture();
        f.variable
            .set(b"\xEF\xBB\xBF \t\xC2\xA0 abc123\xE2\x80\x8B\r\n\xC2\xA0 ");
        assert_eq!(read_ok(f.store.as_ref()), "abc123");
    }

    #[test]
    fn an_invisible_character_inside_the_value_is_refused_like_a_space() {
        let f = fixture();
        for code_point in invisible_code_points() {
            f.variable.set(concatenated(&[
                SECRET.as_bytes(),
                &utf8_of(code_point),
                b"cd",
            ]));
            let error = f.store.read().unwrap_err();
            let name = code_point_name(code_point);
            assert_eq!(error.code, ErrorCode::InvalidState, "{name}");
            assert!(!mentions_secret(&error), "{}", error.describe());
            assert!(error.message.contains("KATANA_UPDATE_TOKEN"), "{name}");
            assert!(error.message.contains("invisible"), "{}", error.describe());
        }
    }

    #[test]
    fn the_characters_next_to_the_invisible_ones_are_kept_even_at_an_end() {
        let f = fixture();
        for code_point in visible_neighbours() {
            let visible = utf8_of(code_point);
            for value in [
                concatenated(&[b"ab", &visible, b"cd"]),
                concatenated(&[&visible, b"abc123"]),
                concatenated(&[b"abc123", &visible]),
                concatenated(&[&visible, b"abc123", &visible]),
            ] {
                f.variable.set(&value);
                let token = f
                    .store
                    .read()
                    .unwrap_or_else(|error| panic!("{}: {}", code_point_name(code_point), error));
                assert_eq!(token.as_bytes(), value.as_slice());
            }
        }
    }

    #[test]
    fn a_value_of_nothing_but_invisible_characters_is_not_a_token_and_says_so() {
        // Not "unset": a person put something there, and the sentence names the variable.
        let f = fixture();
        f.variable
            .set(concatenated(&[&utf8_of(0x00a0), &utf8_of(0xfeff), b" "]));
        let error = f.store.read().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(error.message.contains("KATANA_UPDATE_TOKEN"));
    }

    // (Windows holds a value as UTF-16, so a value that is not text cannot be set there.)
    #[cfg(not(windows))]
    #[test]
    fn a_value_that_is_not_utf8_is_refused() {
        // 0xC3 0x28 is a lead byte followed by a byte that is not a continuation.
        let f = fixture();
        f.variable
            .set(concatenated(&[SECRET.as_bytes(), b"\xC3\x28"]));
        let error = f.store.read().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(!mentions_secret(&error), "{}", error.describe());
    }

    #[test]
    fn a_token_over_the_size_the_credential_manager_holds_is_refused() {
        let f = fixture();
        f.variable.set(token_of_size(2560));
        assert_eq!(read_ok(f.store.as_ref()).len(), 2560);

        f.variable.set(token_of_size(2561));
        let error = f.store.read().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(!mentions_secret(&error), "{}", error.describe());
    }

    #[test]
    fn writing_and_clearing_are_unsupported_and_say_why_and_change_nothing() {
        let f = fixture();
        f.variable.set("abc123");

        let written = f.store.write(SECRET.as_bytes()).unwrap_err();
        assert_eq!(written.code, ErrorCode::Unsupported);
        assert!(written.message.contains("KATANA_UPDATE_TOKEN"));
        assert!(!mentions_secret(&written), "{}", written.describe());

        let cleared = f.store.clear().unwrap_err();
        assert_eq!(cleared.code, ErrorCode::Unsupported);
        assert!(cleared.message.contains("KATANA_UPDATE_TOKEN"));

        // The variable is what it was: neither call touched it.
        assert_eq!(read_ok(f.store.as_ref()), "abc123");
    }

    #[cfg(not(windows))]
    #[test]
    fn where_there_is_no_keyring_the_system_store_is_the_environment() {
        let f = fixture();
        f.variable.set("abc123");
        let system = katana_update::credential::make_system_credential_store();
        assert_eq!(system.r#where(), "environment");
        assert_eq!(read_ok(system.as_ref()), "abc123");
    }
}

// ---- redact_token ----------------------------------------------------------------------------

mod redact_token {
    use super::NON_ASCII_TOKEN;
    use katana_update::credential::redact_token;

    #[test]
    fn replaces_the_token_in_a_header_line() {
        assert_eq!(
            redact_token("Authorization: Bearer abc123", "abc123"),
            "Authorization: Bearer ***"
        );
    }

    #[test]
    fn replaces_every_occurrence_not_just_the_first() {
        assert_eq!(redact_token("abc-abc/abc", "abc"), "***-***/***");
    }

    #[test]
    fn text_without_the_token_is_unchanged() {
        assert_eq!(redact_token("no secret here", "abc123"), "no secret here");
        assert_eq!(redact_token("", "abc123"), "");
    }

    #[test]
    fn an_empty_token_changes_nothing_rather_than_masking_between_every_character() {
        assert_eq!(redact_token("abc", ""), "abc");
        assert_eq!(redact_token("", ""), "");
    }

    #[test]
    fn a_token_shorter_than_four_characters_is_still_replaced() {
        assert_eq!(redact_token("xaxbx", "x"), "***a***b***");
        assert_eq!(redact_token("pass: ab, ab", "ab"), "pass: ***, ***");
    }

    #[test]
    fn occurrences_that_overlap_are_one_mask_so_no_part_of_the_token_is_left() {
        // "aaa" holds "aa" at 0 and at 1: covered together it is all three characters.
        assert_eq!(redact_token("aaa", "aa"), "***");
        // "aaaa": at 0, 1 and 2, which together cover all four.
        assert_eq!(redact_token("aaaa", "aa"), "***");
        // "ababab" and "abab": at 0 and at 2, covering 6 characters.
        assert_eq!(redact_token("ababab", "abab"), "***");
        assert_eq!(redact_token("x ababab y", "abab"), "x *** y");
    }

    #[test]
    fn occurrences_that_only_touch_are_two_masks() {
        assert_eq!(redact_token("tokentoken", "token"), "******");
        assert_eq!(redact_token("[token][token]", "token"), "[***][***]");
    }

    #[test]
    fn a_token_that_is_the_whole_text_is_masked() {
        assert_eq!(redact_token("abc123", "abc123"), "***");
    }

    #[test]
    fn a_token_longer_than_the_text_cannot_be_in_it() {
        assert_eq!(redact_token("abc", "abc123"), "abc");
    }

    #[test]
    fn matches_the_token_literally_and_case_sensitively() {
        // '.', '*' and '+' are characters of this token, not a pattern.
        assert_eq!(redact_token("x a.b*c+ y aXb c", "a.b*c+"), "x *** y aXb c");
        assert_eq!(redact_token("ABC", "abc"), "ABC");
    }

    #[test]
    fn replaces_a_token_with_non_ascii_characters() {
        assert_eq!(
            redact_token(&format!("Bearer {NON_ASCII_TOKEN} sent"), NON_ASCII_TOKEN),
            "Bearer *** sent"
        );
    }

    #[test]
    fn replaces_the_token_wherever_it_is_in_a_url_too() {
        assert_eq!(
            redact_token("https://api.github.com/x?access_token=abc123&y=1", "abc123"),
            "https://api.github.com/x?access_token=***&y=1"
        );
    }
}

// ---- the Windows Credential Manager (written, not compiled until step 8) ---------------------

#[cfg(windows)]
mod windows_support {
    use core::ffi::c_void;
    use katana_update::credential::CredentialStore;
    use katana_update::credential_detail::make_credential_manager_store;
    use std::ptr::null_mut;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::Credentials::{
        CREDENTIALW, CredDeleteW, CredEnumerateW, CredFree, CredReadW, CredWriteW,
    };
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    use windows_sys::Win32::System::Threading::{
        CreateMutexW, GetCurrentProcessId, OpenProcess, ReleaseMutex, WaitForSingleObject,
    };

    // wincred.h, winerror.h, winnt.h and winbase.h.
    pub const CRED_TYPE_GENERIC: u32 = 1;
    pub const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
    pub const ERROR_NOT_FOUND: u32 = 1168;
    const ERROR_INVALID_PARAMETER: u32 = 87;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_ABANDONED: u32 = 0x80;
    const WAIT_TIMEOUT: u32 = 0x102;

    pub fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn terminated(text: &[u16]) -> Vec<u16> {
        text.iter().copied().chain([0]).collect()
    }

    // SAFETY (callers): `text` is a NUL-terminated wide string.
    unsafe fn wide_until_nul(text: *const u16) -> Vec<u16> {
        if text.is_null() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut at = 0;
        // SAFETY: the string is NUL-terminated (the caller's promise).
        unsafe {
            while *text.add(at) != 0 {
                out.push(*text.add(at));
                at += 1;
            }
        }
        out
    }

    // What the system itself holds under a target, read with the Win32 call and not through the
    // store.
    #[derive(Default)]
    pub struct RawCredential {
        pub found: bool,
        pub error: u32,
        pub kind: u32,
        pub persist: u32,
        pub blob: Vec<u8>,
        pub target_name: Vec<u16>,
    }

    pub fn raw_read(target: &[u16]) -> RawCredential {
        let name = terminated(target);
        let mut credential: *mut CREDENTIALW = null_mut();
        // SAFETY: a NUL-terminated name and a place for the pointer the system allocates.
        if unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut credential) } == 0 {
            return RawCredential {
                // SAFETY: GetLastError only reads this thread's last-error value.
                error: unsafe { GetLastError() },
                ..RawCredential::default()
            };
        }
        // SAFETY: the call succeeded, so `credential` is the system's structure until CredFree.
        unsafe {
            let found = &*credential;
            let blob = if found.CredentialBlob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(found.CredentialBlob, found.CredentialBlobSize as usize)
                    .to_vec()
            };
            let raw = RawCredential {
                found: true,
                error: 0,
                kind: found.Type,
                persist: found.Persist,
                blob,
                target_name: wide_until_nul(found.TargetName),
            };
            CredFree(credential as *const c_void);
            raw
        }
    }

    // Stores `blob` exactly, as another program would. Returns the system's error, 0 on success.
    pub fn raw_write(target: &[u16], blob: &[u8]) -> u32 {
        let mut bytes = blob.to_vec();
        let mut name = terminated(target);
        let mut user = terminated(&wide("test"));
        // SAFETY: plain data, all zero is nothing set.
        let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = name.as_mut_ptr();
        credential.CredentialBlobSize = bytes.len() as u32;
        credential.CredentialBlob = bytes.as_mut_ptr();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = user.as_mut_ptr();
        // SAFETY: every pointer is to a buffer of this function that outlives the call.
        if unsafe { CredWriteW(&raw const credential, 0) } != 0 {
            0
        } else {
            // SAFETY: as in raw_read.
            unsafe { GetLastError() }
        }
    }

    pub fn raw_delete(target: &[u16]) {
        let name = terminated(target);
        // SAFETY: a NUL-terminated name.
        unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) };
    }

    // The bytes of UTF-16LE text from its code units, for a blob another tool would write.
    pub fn utf16le(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
    }

    // The prefix of every target name these cases make. The names end "-<process id>-<tick>".
    pub const TEST_TARGET_PREFIX: &str = "Katana/update-token-test-";

    pub fn current_process_id() -> u32 {
        // SAFETY: no argument, no state.
        unsafe { GetCurrentProcessId() }
    }

    // Whether the process named in a test target ("...-<process id>-<tick>") may still be
    // running. A name that does not have that shape, a process this user may not open, and this
    // process itself all answer yes: only a credential that is certainly an orphan is deleted.
    pub fn owner_may_be_running(target: &[u16]) -> bool {
        let dash = u16::from(b'-');
        let Some(last_dash) = target.iter().rposition(|&unit| unit == dash) else {
            return true;
        };
        if last_dash == 0 {
            return true;
        }
        let Some(dash_before) = target[..last_dash].iter().rposition(|&unit| unit == dash) else {
            return true;
        };
        let digits = &target[dash_before + 1..last_dash];
        if digits.is_empty()
            || digits.len() > 9
            || !digits
                .iter()
                .all(|&unit| (u16::from(b'0')..=u16::from(b'9')).contains(&unit))
        {
            return true;
        }
        let process = digits.iter().fold(0u32, |value, &unit| {
            value * 10 + u32::from(unit - u16::from(b'0'))
        });
        if process == current_process_id() {
            return true;
        }
        // SAFETY: a query-only open of a process by its id.
        let handle: HANDLE = unsafe { OpenProcess(SYNCHRONIZE, 0, process) };
        if handle.is_null() {
            // Only "there is no such process" proves it is gone; "access denied" proves it is
            // there.
            // SAFETY: as in raw_read.
            return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
        }
        // SAFETY: the handle was just opened, closed once.
        unsafe {
            let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
            CloseHandle(handle);
            running
        }
    }

    // Deletes the credentials that a run of this binary left behind because it was killed or
    // crashed (a drop cannot run then): the ones under TEST_TARGET_PREFIX whose process has ended.
    // Never touches the target a person's token is under, which does not start with the prefix.
    pub fn sweep_orphaned_test_credentials() {
        let mut count = 0u32;
        let mut found: *mut *mut CREDENTIALW = null_mut();
        let filter = terminated(&wide(&format!("{TEST_TARGET_PREFIX}*")));
        // SAFETY: a NUL-terminated filter and places for the count and the array.
        if unsafe { CredEnumerateW(filter.as_ptr(), 0, &raw mut count, &raw mut found) } == 0 {
            return; // none, or the Credential Manager cannot be used (the probe skips then)
        }
        let mut orphans = Vec::new();
        // SAFETY: the array holds `count` credentials until CredFree.
        unsafe {
            for i in 0..count as usize {
                let credential = &**found.add(i);
                let target = wide_until_nul(credential.TargetName);
                if credential.Type == CRED_TYPE_GENERIC && !owner_may_be_running(&target) {
                    orphans.push(target);
                }
            }
            CredFree(found as *const c_void);
        }
        for target in &orphans {
            raw_delete(target);
        }
    }

    // One case at a time in the whole session, whatever the number of processes and threads. The
    // Credential Manager does not keep reads and writes from different processes in order:
    // measured on Windows 11 on 2026-10-08, with six processes writing and reading unrelated
    // targets, a CredReadW returned the previous value of a target the same process had just
    // written, a credential just written was not found, and a deleted one came back. Nothing a
    // program does in one process can cause or fix that, and a person's Katana writes the token
    // once, so only the test needs the order. A named mutex in the session's namespace (Local\)
    // is released by the system if a process holding it is killed, and is owned by a thread, so
    // the cases of one process wait for each other too.
    pub struct CredentialManagerLock {
        mutex: HANDLE,
        held: bool,
    }

    impl CredentialManagerLock {
        pub fn open() -> Self {
            let name = terminated(&wide("Local\\KatanaUpdateCredentialTests"));
            // SAFETY: a NUL-terminated name and no security attributes.
            let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
            Self { mutex, held: false }
        }

        // Waits for the cases of the others; false when the lock cannot be had in `wait_ms`. A
        // process that was killed while it held the lock leaves it "abandoned", which grants it.
        pub fn acquire(&mut self, wait_ms: u32) -> bool {
            if self.mutex.is_null() {
                return false;
            }
            // SAFETY: the mutex handle is this lock's own.
            let result = unsafe { WaitForSingleObject(self.mutex, wait_ms) };
            self.held = result == WAIT_OBJECT_0 || result == WAIT_ABANDONED;
            self.held
        }
    }

    impl Drop for CredentialManagerLock {
        fn drop(&mut self) {
            // SAFETY: the mutex is released by the thread that holds it, and closed once.
            unsafe {
                if self.held {
                    ReleaseMutex(self.mutex);
                }
                if !self.mutex.is_null() {
                    CloseHandle(self.mutex);
                }
            }
        }
    }

    pub struct Fixture {
        pub target_utf8: String,
        pub target: Vec<u16>,
        pub store: Box<dyn CredentialStore>,
        // Last, so that it is released after the drop below has deleted the credential.
        _lock: CredentialManagerLock,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            raw_delete(&self.target);
        }
    }

    static SWEEP: Once = Once::new();
    static CASES: AtomicU64 = AtomicU64::new(0);

    // The fixture of a case, or None when the case is skipped: the Credential Manager cannot be
    // used in this session.
    pub fn set_up() -> Option<Fixture> {
        let mut lock = CredentialManagerLock::open();
        // Ten minutes: the longest case takes seconds, and the whole suite is some forty of them.
        assert!(
            lock.acquire(600_000),
            "could not get the lock that keeps the cases from running at the same time"
        );
        SWEEP.call_once(sweep_orphaned_test_credentials);
        // The wide name is built from its own literals and digits, not converted from the narrow
        // one by the code under test or by anything like it: the raw calls below are held to the
        // name as the system sees it. The case number keeps two cases of one millisecond apart.
        let process = current_process_id();
        // SAFETY: no argument, no state.
        let tick = unsafe { GetTickCount64() }
            .wrapping_mul(1000)
            .wrapping_add(CASES.fetch_add(1, Ordering::Relaxed));
        let target_utf8 = format!("{TEST_TARGET_PREFIX}{process}-{tick}");
        let mut target = wide(TEST_TARGET_PREFIX);
        target.extend(wide(&format!("{process}")));
        target.push(u16::from(b'-'));
        target.extend(wide(&format!("{tick}")));
        let store = make_credential_manager_store(&target_utf8);
        let fixture = Fixture {
            target_utf8,
            target,
            store,
            _lock: lock,
        };
        // Nothing may be there before: the name is unique, and a leftover would make a case pass
        // on another case's token.
        assert!(!raw_read(&fixture.target).found);
        // Can this session use the Credential Manager at all? A write that is refused as "not
        // available" skips every case with the system's reason; any other failure is real.
        if let Err(probe) = fixture.store.write(b"probe") {
            if probe.code == katana_core::ErrorCode::Unsupported {
                eprintln!(
                    "the Credential Manager cannot be used here: {}",
                    probe.describe()
                );
                return None;
            }
            panic!("probe write failed: {}", probe.describe());
        }
        assert!(fixture.store.clear().is_ok());
        Some(fixture)
    }
}

#[cfg(windows)]
mod credential_manager_store_test {
    use super::windows_support::*;
    use super::*;
    use katana_core::ErrorCode;
    use katana_update::credential_detail::{credential_target_of, make_credential_manager_store};

    fn read_ok(f: &Fixture) -> String {
        f.store
            .read()
            .unwrap_or_else(|error| panic!("{}", error.describe()))
    }

    fn cmdkey_stores(target: &str) -> bool {
        std::process::Command::new("cmdkey")
            .args([
                format!("/generic:{target}"),
                "/user:u".into(),
                "/pass:abc".into(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn read_before_anything_was_written_is_not_found() {
        let Some(f) = set_up() else { return };
        assert_eq!(f.store.read().unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn a_token_written_is_read_back_equal() {
        let Some(f) = set_up() else { return };
        assert!(
            f.store
                .write(b"ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD")
                .is_ok()
        );
        assert_eq!(read_ok(&f), "ghp_0123456789abcdefghijklmnopqrstuvwxyzABCD");
        assert_eq!(f.store.r#where(), "credential manager");
    }

    #[test]
    fn the_system_holds_a_generic_local_machine_credential_of_the_utf8_bytes() {
        let Some(f) = set_up() else { return };
        assert!(f.store.write(b"ghp_abc").is_ok());
        let raw = raw_read(&f.target);
        assert!(raw.found, "{}", raw.error);
        assert_eq!(raw.kind, CRED_TYPE_GENERIC);
        assert_eq!(raw.persist, CRED_PERSIST_LOCAL_MACHINE);
        assert_eq!(raw.target_name, f.target);
        // The seven ASCII bytes, with no terminator and no UTF-16 padding.
        assert_eq!(raw.blob, b"ghp_abc");
    }

    #[test]
    fn a_token_with_non_ascii_characters_is_read_back_equal_and_stored_as_its_utf8() {
        let Some(f) = set_up() else { return };
        assert_eq!(NON_ASCII_TOKEN.len(), 18);
        assert!(f.store.write(NON_ASCII_TOKEN.as_bytes()).is_ok());
        assert_eq!(read_ok(&f), NON_ASCII_TOKEN);
        let raw = raw_read(&f.target);
        assert!(raw.found, "{}", raw.error);
        assert_eq!(raw.blob, NON_ASCII_TOKEN.as_bytes());
    }

    #[test]
    fn writing_again_replaces_the_token_and_leaves_one_credential() {
        let Some(f) = set_up() else { return };
        assert!(f.store.write(b"first-token").is_ok());
        assert!(f.store.write(b"second-token").is_ok());
        assert_eq!(read_ok(&f), "second-token");
        assert_eq!(raw_read(&f.target).blob, b"second-token");
        // One clear removes it: there was never a second.
        assert!(f.store.clear().is_ok());
        assert!(!raw_read(&f.target).found);
    }

    #[test]
    fn clear_removes_the_token_so_read_afterwards_is_not_found() {
        let Some(f) = set_up() else { return };
        assert!(f.store.write(b"abc123").is_ok());
        assert!(f.store.clear().is_ok());

        assert_eq!(f.store.read().unwrap_err().code, ErrorCode::NotFound);

        let raw = raw_read(&f.target);
        assert!(!raw.found);
        assert_eq!(raw.error, ERROR_NOT_FOUND);
    }

    #[test]
    fn clearing_when_nothing_is_stored_succeeds() {
        let Some(f) = set_up() else { return };
        assert!(f.store.clear().is_ok());
        assert!(f.store.clear().is_ok());
    }

    #[test]
    fn white_space_around_a_token_is_not_stored() {
        let Some(f) = set_up() else { return };
        assert!(f.store.write(b"  abc123\r\n").is_ok());
        assert_eq!(raw_read(&f.target).blob, b"abc123");
        assert_eq!(read_ok(&f), "abc123");
    }

    #[test]
    fn a_token_of_exactly_the_limit_is_stored_and_one_byte_more_is_refused() {
        // The limit is the system's, not the store's idea of it: the same two sizes are put to the
        // Credential Manager directly, so a system that moved the limit fails here, not in the
        // field.
        let Some(f) = set_up() else { return };
        let at_limit = token_of_size(2560);
        let over_limit = token_of_size(2561);
        assert_eq!(raw_write(&f.target, at_limit.as_bytes()), 0);
        assert_ne!(raw_write(&f.target, over_limit.as_bytes()), 0);
        assert!(f.store.clear().is_ok());

        assert!(f.store.write(at_limit.as_bytes()).is_ok());
        assert_eq!(read_ok(&f), at_limit);

        let refused = f.store.write(over_limit.as_bytes()).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(refused.message.contains("2561"), "{}", refused.describe());
        assert!(!mentions_secret(&refused), "{}", refused.describe());

        // The refused write changed nothing: the token at the limit is still there.
        assert_eq!(read_ok(&f), at_limit);
    }

    #[test]
    fn a_token_that_cannot_be_used_in_a_header_is_refused_and_nothing_is_stored() {
        let Some(f) = set_up() else { return };
        let secret = SECRET.as_bytes();
        let cases: [Vec<u8>; 11] = [
            Vec::new(),
            b" \t\r\n".to_vec(),
            concatenated(&[secret, b"\nX-Evil: 1"]),
            concatenated(&[secret, b"\r\nX-Evil: 1"]),
            concatenated(&[secret, b"\x01"]),
            concatenated(&[secret, b"\x7f"]),
            concatenated(&[secret, b"\0x"]),
            concatenated(&[secret, b"\xC3\x28"]),
            // A word before the token, as "Bearer <token>" pasted whole; a space inside it; a
            // tab.
            concatenated(&[b"Bearer ", secret]),
            concatenated(&[secret, b" ", secret]),
            concatenated(&[secret, b"\t", secret]),
        ];
        for token in &cases {
            let Err(refused) = f.store.write(token) else {
                panic!("accepted a token of {} bytes", token.len());
            };
            assert_eq!(refused.code, ErrorCode::InvalidArgument);
            assert!(!mentions_secret(&refused), "{}", refused.describe());
            assert!(!raw_read(&f.target).found);
        }
    }

    #[test]
    fn a_space_inside_a_token_is_refused_and_says_to_leave_the_word_out() {
        let Some(f) = set_up() else { return };
        let refused = f
            .store
            .write(format!("Bearer {SECRET}").as_bytes())
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(refused.message.contains("space"), "{}", refused.describe());
        assert!(refused.message.contains("Bearer"), "{}", refused.describe());
        assert!(!mentions_secret(&refused), "{}", refused.describe());

        // Only the white space AROUND a token is dropped (above): "  abc123  " is "abc123".
        assert!(f.store.write(b"  abc123  ").is_ok());
        assert_eq!(raw_read(&f.target).blob, b"abc123");
    }

    #[test]
    fn invisible_characters_around_a_token_are_not_stored_and_inside_it_are_refused() {
        // A token copied from a web page: a no-break space (U+00A0, C2 A0) in front and a
        // byte-order mark (U+FEFF, EF BB BF) behind are dropped, as ASCII blanks are.
        let Some(f) = set_up() else { return };
        assert!(
            f.store
                .write(&concatenated(&[
                    &utf8_of(0x00a0),
                    b"abc123",
                    &utf8_of(0xfeff)
                ]))
                .is_ok()
        );
        assert_eq!(raw_read(&f.target).blob, b"abc123");
        assert!(f.store.clear().is_ok());

        // Inside, every one of them is refused with the sentence that names the cause, and
        // nothing is stored.
        for code_point in invisible_code_points() {
            let name = code_point_name(code_point);
            let refused = f
                .store
                .write(&concatenated(&[
                    SECRET.as_bytes(),
                    &utf8_of(code_point),
                    b"cd",
                ]))
                .unwrap_err();
            assert_eq!(refused.code, ErrorCode::InvalidArgument, "{name}");
            assert!(
                refused.message.contains("invisible"),
                "{}",
                refused.describe()
            );
            assert!(!mentions_secret(&refused), "{}", refused.describe());
            assert!(!raw_read(&f.target).found, "{name}");
        }

        // The visible characters beside them are stored and read back, inside and at the ends.
        for code_point in visible_neighbours() {
            let visible = utf8_of(code_point);
            let value = concatenated(&[&visible, b"abc", &visible]);
            assert!(
                f.store.write(&value).is_ok(),
                "{}",
                code_point_name(code_point)
            );
            assert_eq!(raw_read(&f.target).blob, value);
            assert_eq!(read_ok(&f).as_bytes(), value.as_slice());
        }
    }

    #[test]
    fn a_stored_blob_with_invisible_characters_at_its_ends_is_read_without_them() {
        // A file saved as "UTF-8" by an editor that adds a byte-order mark, a token pasted with a
        // no-break space after it; stored by another program, so the blob is exactly this.
        let Some(f) = set_up() else { return };
        assert_eq!(
            raw_write(&f.target, &concatenated(&[&utf8_of(0xfeff), b"abc123"])),
            0
        );
        assert_eq!(read_ok(&f), "abc123");

        assert_eq!(
            raw_write(&f.target, &concatenated(&[b"abc123", &utf8_of(0x00a0)])),
            0
        );
        assert_eq!(read_ok(&f), "abc123");

        // Inside, it is not a token.
        assert_eq!(
            raw_write(
                &f.target,
                &concatenated(&[SECRET.as_bytes(), &utf8_of(0x00a0), b"x"])
            ),
            0
        );
        let error = f.store.read().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(!mentions_secret(&error), "{}", error.describe());
    }

    #[test]
    fn a_truncated_invisible_character_is_not_utf8_and_is_refused() {
        // The first one or two bytes of U+2003 (E2 80 83) and of U+FEFF (EF BB BF), at the front,
        // at the back, and alone: the rule that looks for these characters must find the end of
        // the text and stop, and the text is not UTF-8.
        let Some(f) = set_up() else { return };
        let truncated: [&[u8]; 8] = [
            b"\xE2",
            b"\xE2\x80",
            b"\xEF",
            b"\xEF\xBB",
            b"abc\xE2\x80",
            b"\xE2\x80abc",
            b"abc\xEF\xBB",
            b"\xEF\xBBabc",
        ];
        for value in truncated {
            let Err(refused) = f.store.write(value) else {
                panic!("accepted a token of {} bytes", value.len());
            };
            assert_eq!(refused.code, ErrorCode::InvalidArgument);
            assert!(!raw_read(&f.target).found);
        }
    }

    #[test]
    fn a_token_another_tool_stored_as_utf16_is_read_as_text() {
        // cmdkey stores the password "abc" as 61 00 62 00 63 00 (measured on Windows 11,
        // 2026-10-08).
        let Some(f) = set_up() else { return };
        assert_eq!(raw_write(&f.target, &utf16le(&[0x61, 0x62, 0x63])), 0);
        assert_eq!(read_ok(&f), "abc");

        // With a terminator, as some tools write: not part of the token.
        assert_eq!(raw_write(&f.target, &utf16le(&[0x61, 0x62, 0x63, 0x00])), 0);
        assert_eq!(read_ok(&f), "abc");

        // 'a', U+00E9, U+65E5: the UTF-8 of the last two is C3 A9 and E6 97 A5.
        assert_eq!(raw_write(&f.target, &utf16le(&[0x61, 0xe9, 0x65e5])), 0);
        assert_eq!(read_ok(&f).as_bytes(), b"a\xC3\xA9\xE6\x97\xA5");

        // U+1F511 is the surrogate pair D83D DD11 in UTF-16, F0 9F 94 91 in UTF-8.
        assert_eq!(raw_write(&f.target, &utf16le(&[0x61, 0xd83d, 0xdd11])), 0);
        assert_eq!(read_ok(&f).as_bytes(), b"a\xF0\x9F\x94\x91");
    }

    #[test]
    fn a_token_another_tool_stored_as_a_utf8_c_string_is_read_without_its_terminator() {
        let Some(f) = set_up() else { return };
        assert_eq!(raw_write(&f.target, b"abc123\0"), 0);
        assert_eq!(read_ok(&f), "abc123");

        // The same bytes with two terminators.
        assert_eq!(raw_write(&f.target, b"abc123\0\0"), 0);
        assert_eq!(read_ok(&f), "abc123");
    }

    #[test]
    fn a_token_made_with_cmdkey_is_read() {
        // The real tool, as the independent writer. Its own failure (not on this machine, refused)
        // is a skip: this case is about reading what it wrote.
        let Some(f) = set_up() else { return };
        if !cmdkey_stores(&f.target_utf8) {
            eprintln!("cmdkey could not store a credential here");
            return;
        }
        assert_eq!(read_ok(&f), "abc");
    }

    #[test]
    fn writing_over_a_token_made_with_cmdkey_replaces_it_with_the_utf8_token() {
        // The sequence a person follows: the documented cmdkey command, and later the Settings
        // page (write) with another token. The credential cmdkey made is replaced, not added to,
        // and what is left is the store's own form, the UTF-8 bytes, not cmdkey's UTF-16.
        let Some(f) = set_up() else { return };
        if !cmdkey_stores(&f.target_utf8) {
            eprintln!("cmdkey could not store a credential here");
            return;
        }
        // The premise, measured on Windows 11 on 2026-10-08: "abc" is 61 00 62 00 63 00.
        let before = raw_read(&f.target);
        assert!(before.found, "{}", before.error);
        assert_eq!(before.blob, utf16le(&[0x61, 0x62, 0x63]));

        assert!(f.store.write(b"new-token").is_ok());
        let after = raw_read(&f.target);
        assert!(after.found, "{}", after.error);
        assert_eq!(after.blob, b"new-token"); // 9 bytes, no NUL
        assert_eq!(after.kind, CRED_TYPE_GENERIC);
        assert_eq!(after.persist, CRED_PERSIST_LOCAL_MACHINE);
        assert_eq!(read_ok(&f), "new-token");

        // One credential under the name: a clear leaves nothing.
        assert!(f.store.clear().is_ok());
        assert!(!raw_read(&f.target).found);
    }

    #[test]
    fn every_control_character_inside_a_token_is_refused_and_nothing_is_stored() {
        // 0x00-0x1F and 0x7F between a secret and a letter. Each is refused for the same reason
        // (an HTTP header cannot carry it), the line feed and the carriage return being the
        // dangerous ones; the boundaries 0x1F and 0x7F are the ones a rule written with < or <=
        // gets wrong.
        let Some(f) = set_up() else { return };
        for byte in control_bytes(true) {
            let refused = f
                .store
                .write(&concatenated(&[SECRET.as_bytes(), &[byte], b"x"]))
                .unwrap_err();
            assert_eq!(
                refused.code,
                ErrorCode::InvalidArgument,
                "{}",
                byte_name(byte)
            );
            assert!(!mentions_secret(&refused), "{}", refused.describe());
            assert!(!raw_read(&f.target).found, "{}", byte_name(byte));
        }
    }

    #[test]
    fn a_control_character_at_an_end_is_refused_unless_it_is_one_of_the_six_blanks() {
        let Some(f) = set_up() else { return };
        for byte in control_bytes(true) {
            let blank = BLANKS.contains(&byte);
            for value in [
                concatenated(&[&[byte], SECRET.as_bytes()]),
                concatenated(&[SECRET.as_bytes(), &[byte]]),
                concatenated(&[&[byte], SECRET.as_bytes(), &[byte]]),
            ] {
                let written = f.store.write(&value);
                if blank {
                    // Trimmed: what the system keeps is the token alone.
                    assert!(written.is_ok(), "{}", byte_name(byte));
                    assert_eq!(raw_read(&f.target).blob, SECRET.as_bytes());
                    assert!(f.store.clear().is_ok());
                } else {
                    let refused = written.unwrap_err();
                    assert_eq!(
                        refused.code,
                        ErrorCode::InvalidArgument,
                        "{}",
                        byte_name(byte)
                    );
                    assert!(!mentions_secret(&refused), "{}", refused.describe());
                    assert!(!raw_read(&f.target).found);
                }
            }
        }
    }

    #[test]
    fn every_visible_ascii_character_inside_a_token_is_stored_and_read_back() {
        let Some(f) = set_up() else { return };
        for byte in visible_bytes() {
            let value = format!("ab{}cd", char::from(byte));
            assert!(
                f.store.write(value.as_bytes()).is_ok(),
                "{}",
                byte_name(byte)
            );
            assert_eq!(raw_read(&f.target).blob, value.as_bytes());
            assert_eq!(read_ok(&f), value);
        }
    }

    #[test]
    fn a_stored_control_character_is_refused_on_read_too() {
        // Another program stored it, or the machine was set up before this rule. 0x01-0x1F and
        // 0x7F between a secret and a letter; 0x00 is the UTF-16 and terminator cases' (above and
        // below).
        let Some(f) = set_up() else { return };
        for byte in control_bytes(false) {
            assert_eq!(
                raw_write(
                    &f.target,
                    &concatenated(&[SECRET.as_bytes(), &[byte], b"x"])
                ),
                0
            );
            let error = f.store.read().unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidState, "{}", byte_name(byte));
            assert!(!mentions_secret(&error), "{}", error.describe());
        }
        // The six blanks at the end of a stored token are not part of it.
        for &blank in BLANKS {
            assert_eq!(
                raw_write(&f.target, &concatenated(&[b"abc123", &[blank]])),
                0
            );
            assert_eq!(read_ok(&f), "abc123", "{}", byte_name(blank));
        }
    }

    #[test]
    fn the_sweep_deletes_only_the_credentials_of_processes_that_have_ended() {
        // Process ids are multiples of four and small; 999999996 is no process. 4 is the System
        // process, which a user cannot open: access denied proves it is there, so its credential
        // stays.
        let Some(_f) = set_up() else { return };
        let dead = wide(&format!("{TEST_TARGET_PREFIX}999999996-1"));
        let dead_non_ascii = wide(&format!("{TEST_TARGET_PREFIX}\u{e9}\u{65e5}-999999996-2"));
        let unopenable = wide(&format!("{TEST_TARGET_PREFIX}4-1"));
        let this_process = wide(&format!("{TEST_TARGET_PREFIX}{}-1", current_process_id()));
        let no_shape = wide(&format!("{TEST_TARGET_PREFIX}abc"));
        let not_ours = wide("Katana/update-token-testing-999999996-1");
        // Made before anything is written, so it runs after a failure too.
        struct DeleteAllOnExit(Vec<Vec<u16>>);
        impl Drop for DeleteAllOnExit {
            fn drop(&mut self) {
                for name in &self.0 {
                    raw_delete(name);
                }
            }
        }
        let guard = DeleteAllOnExit(vec![
            dead.clone(),
            dead_non_ascii.clone(),
            unopenable.clone(),
            this_process.clone(),
            no_shape.clone(),
            not_ours.clone(),
        ]);
        for name in &guard.0 {
            assert_eq!(raw_write(name, b"x"), 0);
        }

        sweep_orphaned_test_credentials();

        assert!(!raw_read(&dead).found);
        assert!(!raw_read(&dead_non_ascii).found);
        assert!(raw_read(&unopenable).found);
        assert!(raw_read(&this_process).found);
        assert!(raw_read(&no_shape).found);
        assert!(raw_read(&not_ours).found);
    }

    #[test]
    fn a_stored_blob_that_is_not_a_token_is_an_error_that_does_not_quote_it() {
        // A control character in a UTF-8 blob, UTF-8 that is not well formed, an odd-length blob
        // with a NUL inside (neither UTF-8 nor UTF-16), a lone surrogate in UTF-16, and a space
        // inside ("Bearer <token>", in UTF-8 and in UTF-16: 'a' U+0020 'b').
        let Some(f) = set_up() else { return };
        let secret = SECRET.as_bytes();
        let blobs = [
            concatenated(&[secret, b"\x01x"]),
            concatenated(&[secret, b"\xC3\x28"]),
            concatenated(&[secret, b"\0x"]),
            utf16le(&[0x61, 0xd83d]),
            concatenated(&[b"Bearer ", secret]),
            utf16le(&[0x61, 0x20, 0x62]),
        ];
        for blob in &blobs {
            assert_eq!(raw_write(&f.target, blob), 0);
            let Err(error) = f.store.read() else {
                panic!("read a token from a blob of {} bytes", blob.len());
            };
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert!(!mentions_secret(&error), "{}", error.describe());
        }
    }

    #[test]
    fn a_credential_with_nothing_in_it_is_no_token() {
        // An empty password, or one of only terminators and white space, is a credential that
        // exists and holds no token: absent, not an error.
        let Some(f) = set_up() else { return };
        let blobs = [
            Vec::new(),
            vec![0u8, 0],
            b" \r\n".to_vec(),
            utf16le(&[0x20, 0x0a]),
        ];
        for blob in &blobs {
            let written = raw_write(&f.target, blob);
            if written != 0 {
                // The system's rule, not the store's: an empty blob may not be kept at all.
                assert!(
                    blob.is_empty(),
                    "a blob of {} bytes: error {written}",
                    blob.len()
                );
                continue;
            }
            let Err(error) = f.store.read() else {
                panic!("read a token from a blob of {} bytes", blob.len());
            };
            assert_eq!(error.code, ErrorCode::NotFound);
        }
    }

    #[test]
    fn a_non_ascii_target_reaches_the_system_as_the_characters_it_names() {
        // U+00E9 is C3 A9, U+65E5 is E6 97 A5, U+1F511 is F0 9F 94 91 in UTF-8, and the UTF-16
        // code units E9, 65E5 and the surrogate pair D83D DD11. The store gets the UTF-8 bytes;
        // the wide name is written out as the code units the system must receive. Under the test
        // prefix, so that the sweep finds it if this process is killed.
        let Some(_f) = set_up() else { return };
        // SAFETY: no argument, no state.
        let tick = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        let suffix = format!("{}-{tick}", current_process_id());
        let utf8: Vec<u8> = concatenated(&[
            b"Katana/update-token-test-",
            b"\xC3\xA9\xE6\x97\xA5\xF0\x9F\x94\x91",
            b"-",
            suffix.as_bytes(),
        ]);
        let mut wide_name = wide(TEST_TARGET_PREFIX);
        wide_name.extend_from_slice(&[0x00e9, 0x65e5, 0xd83d, 0xdd11, u16::from(b'-')]);
        wide_name.extend(suffix.bytes().map(u16::from));
        // Made before anything is written, so it runs after a failure too.
        struct DeleteOnExit(Vec<u16>);
        impl Drop for DeleteOnExit {
            fn drop(&mut self) {
                raw_delete(&self.0);
            }
        }
        let _guard = DeleteOnExit(wide_name.clone());

        let store = make_credential_manager_store(&utf8);
        assert_eq!(credential_target_of(store.as_ref()), wide_name);
        assert!(store.write(b"tok-1").is_ok());
        let raw = raw_read(&wide_name);
        assert!(raw.found, "{}", raw.error);
        assert_eq!(raw.target_name, wide_name);
        assert_eq!(raw.blob, b"tok-1");
        assert_eq!(
            store
                .read()
                .unwrap_or_else(|error| panic!("{}", error.describe())),
            "tok-1"
        );
        assert!(store.clear().is_ok());
        assert!(!raw_read(&wide_name).found);
    }
}

#[cfg(windows)]
mod credential_manager_target {
    use super::windows_support::wide;
    use katana_update::credential::{
        make_environment_credential_store, make_system_credential_store,
    };
    use katana_update::credential_detail::{credential_target_of, make_credential_manager_store};

    fn panics(target: &[u8]) -> bool {
        std::panic::catch_unwind(|| make_credential_manager_store(target)).is_err()
    }

    #[test]
    fn the_system_store_is_the_credential_manager_and_a_target_must_be_named() {
        // Not read or written: the name a person's token is under is never touched by a test.
        assert_eq!(
            make_system_credential_store().r#where(),
            "credential manager"
        );
        assert!(panics(b""));
        assert!(panics(b"bad\xC3\x28"));
        // 32767 characters is the most the system takes as a generic target
        // (CRED_MAX_GENERIC_TARGET_NAME_LENGTH), and one more is refused before any call.
        assert!(!panics("a".repeat(32767).as_bytes()));
        assert!(panics("a".repeat(32768).as_bytes()));
    }

    #[test]
    fn the_system_store_is_under_the_name_the_documentation_tells_a_person_to_use() {
        // The literal, not CREDENTIAL_TARGET: `cmdkey /generic:Katana/update-token /user:x
        // /pass:...` is what a person types, so the name the system store passes to the system is
        // held to it. Nothing is read or written under it.
        let system = make_system_credential_store();
        assert_eq!(
            credential_target_of(system.as_ref()),
            wide("Katana/update-token")
        );

        // The seam reports the store's own name, not a fixed one, and none for another kind of
        // store.
        let other = make_credential_manager_store("Katana/other");
        assert_eq!(credential_target_of(other.as_ref()), wide("Katana/other"));
        let environment = make_environment_credential_store();
        assert!(credential_target_of(environment.as_ref()).is_empty());
    }
}

#[cfg(windows)]
mod credential_manager_errors {
    use katana_core::ErrorCode;
    use katana_update::credential_detail::credential_error;

    #[test]
    fn the_codes_for_no_credential_manager_in_this_session_are_unsupported() {
        // By number, from Microsoft's "System Error Codes" list, not by the constants the code
        // uses.
        let codes: [(u32, &str); 5] = [
            (5, "ERROR_ACCESS_DENIED"),
            (50, "ERROR_NOT_SUPPORTED"),
            (120, "ERROR_CALL_NOT_IMPLEMENTED"),
            (1312, "ERROR_NO_SUCH_LOGON_SESSION"),
            (1355, "ERROR_NO_SUCH_DOMAIN"),
        ];
        for (number, name) in codes {
            let error = credential_error(number, "read", "Katana/x");
            assert_eq!(error.code, ErrorCode::Unsupported, "{name}");
            assert!(
                error.message.contains("not available in this session"),
                "{name}: {}",
                error.describe()
            );
            // The number is there, so a refusal of access is not hidden by the sentence.
            assert!(
                error.message.contains(&format!("(Windows error {number})")),
                "{name}: {}",
                error.describe()
            );
            assert_eq!(error.context, "Katana/x", "{name}");
        }
    }

    #[test]
    fn every_other_code_is_an_internal_fault_that_names_the_action_and_the_code() {
        // 1783 is what CredWriteW answers for a blob over the limit (measured on Windows 11,
        // 2026-10-08), 87 is ERROR_INVALID_PARAMETER, 1168 is ERROR_NOT_FOUND (read and clear
        // take that one before they ask), 2 is ERROR_FILE_NOT_FOUND; the others sit next to the
        // codes above, so that a rule that matched a range would be caught.
        for number in [2, 49, 51, 87, 119, 121, 1168, 1311, 1313, 1354, 1356, 1783] {
            for action in ["read", "store", "remove"] {
                let error = credential_error(number, action, "Katana/x");
                assert_eq!(error.code, ErrorCode::Internal, "{number}");
                assert!(
                    error
                        .message
                        .contains(&format!("could not {action} the token")),
                    "{number}: {}",
                    error.describe()
                );
                assert!(
                    error.message.contains(&format!("(Windows error {number})")),
                    "{number}: {}",
                    error.describe()
                );
                assert_eq!(error.context, "Katana/x", "{number}");
            }
        }
    }
}

#[cfg(windows)]
mod credential_manager_failures {
    use super::*;
    use katana_core::ErrorCode;
    use katana_update::credential_detail::make_credential_manager_store_failing_with;

    #[test]
    fn each_operation_names_its_own_action_and_maps_the_code_to_its_kind_of_error() {
        // The system's three calls are replaced by calls that fail with a given code (nothing is
        // read, stored or removed), so that what read(), write() and clear() each make of the
        // failure is held to their own words: the mapping function alone (below) would pass if
        // write() said "read". 1312 is ERROR_NO_SUCH_LOGON_SESSION (no Credential Manager in this
        // session), 87 is ERROR_INVALID_PARAMETER (any other fault).
        let token = format!("{SECRET}-1");
        {
            let store = make_credential_manager_store_failing_with("Katana/x", 1312);
            let looked = store.read().unwrap_err();
            assert_eq!(looked.code, ErrorCode::Unsupported);
            assert_eq!(looked.context, "Katana/x");
            let written = store.write(token.as_bytes()).unwrap_err();
            assert_eq!(written.code, ErrorCode::Unsupported);
            assert_eq!(written.context, "Katana/x");
            assert!(!mentions_secret(&written), "{}", written.describe());
            let cleared = store.clear().unwrap_err();
            assert_eq!(cleared.code, ErrorCode::Unsupported);
            assert_eq!(cleared.context, "Katana/x");
            for error in [&looked, &written, &cleared] {
                assert!(
                    error
                        .message
                        .contains("not available in this session (Windows error 1312)"),
                    "{}",
                    error.describe()
                );
            }
        }
        {
            let store = make_credential_manager_store_failing_with("Katana/x", 87);
            let looked = store.read().unwrap_err();
            assert_eq!(looked.code, ErrorCode::Internal);
            assert_eq!(looked.context, "Katana/x");
            assert!(
                looked
                    .message
                    .contains("could not read the token (Windows error 87)"),
                "{}",
                looked.describe()
            );

            let written = store.write(token.as_bytes()).unwrap_err();
            assert_eq!(written.code, ErrorCode::Internal);
            assert_eq!(written.context, "Katana/x");
            assert!(
                written
                    .message
                    .contains("could not store the token (Windows error 87)"),
                "{}",
                written.describe()
            );
            assert!(!mentions_secret(&written), "{}", written.describe());

            let cleared = store.clear().unwrap_err();
            assert_eq!(cleared.code, ErrorCode::Internal);
            assert_eq!(cleared.context, "Katana/x");
            assert!(
                cleared
                    .message
                    .contains("could not remove the token (Windows error 87)"),
                "{}",
                cleared.describe()
            );
        }
    }

    #[test]
    fn not_found_is_no_token_for_read_and_success_for_clear_but_a_fault_for_write() {
        // 1168 is ERROR_NOT_FOUND. Reading what is not there is the normal "none"; clearing it is
        // the state asked for; but storing cannot meet it, so a write that is told that is a
        // fault.
        let store = make_credential_manager_store_failing_with("Katana/x", 1168);
        assert_eq!(store.read().unwrap_err().code, ErrorCode::NotFound);
        assert!(store.clear().is_ok());
        let written = store.write(b"abc123").unwrap_err();
        assert_eq!(written.code, ErrorCode::Internal);
        assert!(
            written.message.contains("(Windows error 1168)"),
            "{}",
            written.describe()
        );
    }

    #[test]
    fn an_unusable_token_is_refused_before_the_system_is_asked() {
        // The refusal is the token's (InvalidArgument), not the system's (87): the rule runs
        // first, so a person is told what is wrong with the token even where the Credential
        // Manager is down.
        let store = make_credential_manager_store_failing_with("Katana/x", 1312);
        let refused = store
            .write(format!("Bearer {SECRET}").as_bytes())
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }
}
