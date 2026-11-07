//! The path rules: what a file name from a manifest, or one read back from a journal, must pass
//! before anything is written under it (the twin of tests/update/test_paths.cpp).
//!
//! The expectations come from the rules as the module states them and from the shared table
//! tests/update/fixtures/path_rules.txt, which the two Python tools (the producer and the
//! verifier, tests/update/tool_tests) are held to as well. They are never read off the code under
//! test. The table is read here too, and any line this program answers differently is a failure,
//! so the engine, the helper and both tools agree by construction.

use katana_core::{ErrorCode, Status};
use katana_update::paths::{check_manifest_path, check_manifest_paths};
use std::path::Path;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/update/fixtures"
);

fn shown(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

fn accepts(path: impl AsRef<[u8]>) -> bool {
    check_manifest_path(path).is_ok()
}

// A rule that is broken by this path alone, and only by it: the status is a refusal, an
// InvalidArgument whose context shows the path.
fn expect_refused(path: impl AsRef<[u8]>) {
    let path = path.as_ref();
    let Err(error) = check_manifest_path(path) else {
        panic!("accepted: '{}'", shown(path));
    };
    assert_eq!(error.code, ErrorCode::InvalidArgument, "{}", shown(path));
    assert!(!error.message.is_empty(), "{}", shown(path));
}

fn expect_accepted(path: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Err(error) = check_manifest_path(path) {
        panic!("refused: '{}': {}", shown(path), error.describe());
    }
}

fn expect_all_refused(paths: &[&str]) {
    for path in paths {
        expect_refused(path);
    }
}

fn expect_all_accepted(paths: &[&str]) {
    for path in paths {
        expect_accepted(path);
    }
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None, // the table writes upper-case hex only
    }
}

// The table's escape: %XX (two upper-case hex digits) for every byte outside 0x21 to 0x7E and for
// '%'. A malformed escape fails the test that reads the table, not silently.
fn decode_escapes(field: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        if field[i] != b'%' {
            out.push(field[i]);
            i += 1;
            continue;
        }
        if i + 2 >= field.len() {
            return None; // "%" needs two more characters
        }
        let high = hex_value(field[i + 1])?;
        let low = hex_value(field[i + 2])?;
        out.push(high * 16 + low);
        i += 3;
    }
    Some(out)
}

fn sequential_paths(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("share/katana/data/f{i}.dat"))
        .collect()
}

fn check_set(paths: &[impl AsRef<[u8]>], sizes: &[u64]) -> Status {
    check_manifest_paths(paths, sizes)
}

fn check_set_of_paths(paths: &[impl AsRef<[u8]>]) -> Status {
    check_set(paths, &vec![1; paths.len()])
}

const GIB: u64 = 1024 * 1024 * 1024;

// ---- the shared table ----------------------------------------------------------------------------

mod update_path_rules_table {
    use super::*;

    #[test]
    fn every_line_of_the_shared_table_is_answered_as_the_table_says() {
        let table = Path::new(FIXTURES).join("path_rules.txt");
        let bytes = std::fs::read(&table)
            .unwrap_or_else(|error| panic!("cannot open {}: {error}", table.display()));

        let mut accepted = 0;
        let mut refused = 0;
        for (index, mut line) in bytes.split(|&byte| byte == b'\n').enumerate() {
            let line_number = index + 1;
            if let Some(without) = line.strip_suffix(b"\r") {
                line = without;
            }
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            // verdict TAB escaped-path TAB why; an empty path is an empty middle field.
            let first_tab = line
                .iter()
                .position(|&byte| byte == b'\t')
                .unwrap_or_else(|| panic!("line {line_number} has no tab"));
            let second_tab = line[first_tab + 1..]
                .iter()
                .position(|&byte| byte == b'\t')
                .map(|at| at + first_tab + 1)
                .unwrap_or_else(|| panic!("line {line_number} has one tab only"));
            let verdict = &line[..first_tab];
            let escaped = &line[first_tab + 1..second_tab];
            let why = shown(&line[second_tab + 1..]);
            assert!(
                verdict == b"ok" || verdict == b"no",
                "line {line_number}: {}",
                shown(verdict)
            );

            let path = decode_escapes(escaped).unwrap_or_else(|| {
                panic!("line {line_number}: bad escape in '{}'", shown(escaped))
            });
            let expected = verdict == b"ok";
            let status = check_manifest_path(&path);
            assert_eq!(
                status.is_ok(),
                expected,
                "line {line_number} ('{}', {why}): the table says {}, check_manifest_path says {}",
                shown(escaped),
                shown(verdict),
                match &status {
                    Ok(()) => String::from("ok"),
                    Err(error) => format!("no: {}", error.describe()),
                }
            );
            if expected {
                accepted += 1;
            } else {
                refused += 1;
            }
        }
        // The table was read and holds both kinds: a loop over an empty or one-sided file would
        // pass, and say nothing. (At least what the table held when this was written: 16 and 40.
        // A truncated table fails.)
        assert!(accepted >= 16);
        assert!(refused >= 40);
    }

    #[test]
    fn the_decoder_reads_percent_escapes_as_the_header_of_the_table_describes_them() {
        assert_eq!(
            decode_escapes(b"bin/a%20b").as_deref(),
            Some(&b"bin/a b"[..])
        );
        assert_eq!(
            decode_escapes(b"share/caf%C3%A9.txt").as_deref(),
            Some(&b"share/caf\xC3\xA9.txt"[..])
        );
        assert_eq!(decode_escapes(b"a%25b").as_deref(), Some(&b"a%b"[..]));
        assert_eq!(decode_escapes(b"").as_deref(), Some(&b""[..]));
        assert_eq!(decode_escapes(b"a%2"), None);
        assert_eq!(decode_escapes(b"a%"), None);
        assert_eq!(decode_escapes(b"a%zz"), None);
        assert_eq!(
            decode_escapes(b"a%2f"),
            None,
            "lower-case hex is not what the table writes"
        );
    }
}

// ---- one path at a time --------------------------------------------------------------------------

mod update_paths {
    use super::*;

    // ---- the plain names that must pass ------------------------------------------------------

    #[test]
    fn plain_names_under_the_four_install_folders_pass() {
        expect_all_accepted(&[
            "bin/katana.exe",
            "bin/katana_cli.exe",
            "bin/libgdal-39.dll",
            "bin/platforms/qwindows.dll",
            "share/katana/readme.txt",
            "share/proj/proj.db",
            "etc/katana/settings.ini",
            "etc/ssl/certs/ca-bundle.crt",
            "lib/libx.so.1.2",
            "lib/x86_64-linux-gnu/libz.so.1",
            "share/katana/news-0.10.0.txt",
            "bin/a b.txt", // a space inside a name is fine; at the end of one it is not
            "bin/.hidden", // a dot file
            "bin/a.b.c.d",
        ]);
    }

    #[test]
    fn every_printable_punctuation_mark_that_no_rule_names_is_allowed() {
        // The forbidden set is exactly backslash : * ? " < > | (and the controls). Every other
        // printable ASCII character is a legal name character on the three file systems.
        expect_all_accepted(&[
            "share/katana/a+b,c;d=e@f[1](2){3}^`!#$%&'-_.txt",
            "share/katana/~",     // a tilde with no digit after it
            "share/katana/a~",    // ... at the end of a name
            "share/katana/~a",    // ... at the start
            "share/katana/a~b~c", //
        ]);
    }

    // ---- one rule at a time ------------------------------------------------------------------

    #[test]
    fn an_empty_path_is_refused() {
        expect_refused("");
    }

    #[test]
    fn an_absolute_path_or_one_with_a_drive_is_refused() {
        expect_all_refused(&[
            "/bin/a.dll",
            "/",
            "//server/share/x",
            "C:/bin/a.dll",
            "C:bin/a.dll",
            "c:/bin/a.dll",
            "\\bin\\a.dll",
            "\\\\server\\share\\x",
        ]);
    }

    #[test]
    fn a_backslash_a_colon_or_a_wildcard_is_refused_wherever_it_stands() {
        // One character at a time, in the middle of an otherwise good name: each is refused by
        // its own.
        for c in "\\:*?\"<>|".chars() {
            expect_refused(format!("bin/a{c}b.dll"));
            expect_refused(format!("bin/{c}"));
            expect_refused(format!("bin/x{c}"));
            expect_refused(format!("{c}bin/x"));
            expect_refused(format!("share/dir{c}/x"));
        }
        // An NTFS alternate data stream: a name, a colon, a stream.
        expect_refused("share/katana/readme.txt:hidden");
        expect_refused("share/katana/readme.txt::$DATA");
    }

    #[test]
    fn an_empty_dot_or_dot_dot_component_is_refused() {
        expect_all_refused(&[
            "bin//a.dll",
            "bin/a.dll/",
            "bin/",
            "share/katana//x",
            "bin/./a.dll",
            "bin/../etc/x",
            "bin/a/..",
            "bin/a/.",
            "./bin/a.dll",
            "../bin/a.dll",
            "bin/a/../../b",
            "share/katana/..",
            "bin/..",
            "bin/.",
        ]);
        // A dot with a name after it is a name, not a directory reference; three dots are a name
        // too, but one that ends in a dot, which Windows drops (the rule below).
        expect_all_accepted(&["bin/..a/b", "bin/..a.dll", "bin/.a/b"]);
        expect_refused("bin/.../a.dll");
    }

    #[test]
    fn the_first_component_must_be_bin_share_etc_or_lib_exactly_and_have_a_file_below_it() {
        expect_all_refused(&[
            "other/a.txt",
            "LICENSE",
            "README.md",
            "bins/a",
            "sharee/a",
            "etcetera/a",
            "libs/a",
            "Bin/a.dll",
            "BIN/a.dll",
            "Share/katana/a",
            "ETC/a",
            "Lib/a",
            "bin",
            "share",
            "etc",
            "lib",
            "bin/",
            "docs/a.txt",
            "build/bin/a",
        ]);
        expect_all_accepted(&["bin/a", "share/a", "etc/a", "lib/a"]);
    }

    #[test]
    fn a_non_ascii_name_is_refused_as_is_any_control_character_and_delete() {
        // ASCII only: e acute as UTF-8, the same e acute as a lone Latin-1 byte, a Cyrillic
        // letter, a CJK character, an emoji and a no-break space, and a byte-order mark.
        let non_ascii: [&[u8]; 7] = [
            b"share/caf\xC3\xA9.txt",
            b"share/caf\xE9.txt",
            b"share/\xD0\xB0.txt",
            b"share/\xE6\x97\xA5.txt",
            b"share/\xF0\x9F\x98\x80.txt",
            b"share/a\xC2\xA0b.txt",
            b"share/\xEF\xBB\xBFa.txt",
        ];
        for path in non_ascii {
            expect_refused(path);
        }
        let with_byte = |byte: u8| -> Vec<u8> {
            let mut path = b"bin/a".to_vec();
            path.push(byte);
            path.push(b'b');
            path
        };
        // Every control character, one at a time, and DEL.
        for byte in 0..0x20u8 {
            expect_refused(with_byte(byte));
        }
        expect_refused(with_byte(0x7f));
        // Every byte from 0x80 to 0xFF, one at a time.
        for byte in 0x80..=0xffu8 {
            expect_refused(with_byte(byte));
        }
        // The boundaries of the allowed range: 0x20 (space) and 0x7E (~) inside a name pass; the
        // bytes just outside, 0x1F and 0x7F, do not.
        expect_accepted("bin/a b");
        expect_accepted("bin/a~b");
        expect_refused(with_byte(0x1f));
        expect_refused(with_byte(0x7f));
    }

    #[test]
    fn a_nul_byte_anywhere_is_refused_and_never_ends_the_path_early() {
        // A C string would stop at the NUL and see the good name "bin/a.dll".
        expect_refused(b"bin/a.dll\0");
        expect_refused(b"bin/a.dll\0../../x");
        expect_refused(b"bin/a\0.dll");
        expect_refused(b"\0bin/a.dll");
        expect_refused(b"\0");
    }

    #[test]
    fn a_component_ending_in_a_dot_or_a_space_is_refused_because_windows_drops_it() {
        expect_all_refused(&[
            "bin/a.",
            "bin/a ",
            "bin/a..",
            "bin/a  ",
            "bin/a. ",
            "bin/a .",
            "share/dir./x",
            "share/dir /x",
            "share/a./b/c.txt",
            "share/a/b /c.txt",
            "bin/ ",
            "bin/.",
        ]);
        expect_all_accepted(&["bin/a.b", "bin/.a", "bin/a b", "bin/ a", "share/d.e/f.g"]);
    }

    #[test]
    fn a_windows_device_name_is_refused_in_any_case_with_or_without_an_extension() {
        let mut devices: Vec<String> = ["con", "prn", "aux", "nul"]
            .iter()
            .map(|name| String::from(*name))
            .collect();
        for digit in '1'..='9' {
            devices.push(format!("com{digit}"));
            devices.push(format!("lpt{digit}"));
        }
        assert_eq!(devices.len(), 22); // 4 + 9 + 9
        for lower in &devices {
            let upper = lower.to_ascii_uppercase();
            let mut mixed = lower.clone();
            mixed[..1].make_ascii_uppercase();
            for name in [lower, &upper, &mixed] {
                expect_refused(format!("bin/{name}"));
                expect_refused(format!("bin/{name}.txt"));
                expect_refused(format!("bin/{name}.tar.gz"));
                expect_refused(format!("share/{name}/x.dat")); // as a folder
                expect_refused(format!("share/katana/{name}.d/x.dat"));
            }
        }
    }

    #[test]
    fn names_that_merely_start_like_a_device_or_contain_one_are_not_devices() {
        expect_all_accepted(&[
            "bin/console.exe",   // CON followed by letters
            "bin/com.txt",       // COM with no digit
            "bin/com10.dll",     // only COM1 to COM9 are devices
            "bin/lpt10",         //
            "bin/commander",     //
            "bin/null",          // NUL is a device, NULL is not
            "bin/nulls.txt",     //
            "bin/auxiliary.dll", //
            "bin/prnt.dll",      //
            "bin/con1.dll",      // CON with a digit is not CON
            "bin/xcon.dll",      // the device name must be the whole stem
            "bin/a.con",         // after the first dot it is an extension
            "bin/a.nul.txt",     //
            "bin/x-com1.dll",    //
        ]);
    }

    #[test]
    fn com_zero_and_lpt_zero_are_ordinary_names_unless_the_shared_table_says_otherwise() {
        // COM1 to COM9 and LPT1 to LPT9 only. The producer's author found COM0 and LPT0 ordinary
        // on Windows 11, and the table has no line that makes them devices; if the owner rules
        // otherwise the table and this test change together.
        expect_all_accepted(&["bin/com0", "bin/COM0.dll", "bin/lpt0", "bin/LPT0.txt"]);
    }

    #[test]
    fn an8_dot3_short_name_is_refused_where_a_component_has_a_tilde_then_a_digit() {
        expect_all_refused(&[
            "bin/PROGRA~1.DLL",
            "bin/progra~1.dll",
            "share/KATANA~12/x",
            "bin/a~0",
            "bin/a~9",
            "bin/~1",
            "bin/~1.dll",
            "bin/a~1b~c",
            "bin/x~2.tar.gz",
            "share/dir~3/file.txt",
        ]);
        expect_all_accepted(&[
            "bin/a~b.dll",
            "bin/a~",
            "bin/~a",
            "bin/~",
            "bin/a~~b",
            "bin/a~.dll",
        ]);
        // A tilde ending one component and a digit starting the next are two components.
        expect_all_accepted(&["share/a~/1.txt", "share/katana~/2/x"]);
    }

    #[test]
    fn the_updaters_own_records_are_refused_wherever_they_sit_and_in_any_ascii_case() {
        expect_all_refused(&[
            "share/katana/install.json",
            "share/katana/installed.json",
            "share/katana/installed.manifest.json",
            "share/katana/installed.manifest.json.sig",
            // any case: on NTFS and APFS INSTALL.JSON is install.json
            "share/katana/INSTALL.JSON",
            "share/katana/Install.Json",
            "share/katana/INSTALLED.JSON",
            "share/katana/Installed.Manifest.JSON",
            "share/katana/installed.manifest.json.SIG",
            // not only in share/katana: the last component is what is reserved
            "bin/install.json",
            "etc/installed.json",
            "lib/sub/dir/installed.manifest.json.sig",
            "etc/katana/INSTALL.json",
        ]);
    }

    #[test]
    fn the_updaters_working_folder_is_refused_as_any_component_in_any_ascii_case() {
        expect_all_refused(&[
            "share/.update/x",
            "share/.UPDATE/x",
            "share/.Update/x",
            "bin/.update",
            "bin/.UPDATE",
            "bin/sub/.update/deep/x",
            "lib/a/b/.update",
            "etc/.update/lock",
        ]);
        // A name that merely starts with it, or contains it, is an ordinary name.
        expect_all_accepted(&[
            "share/.updated/x",
            "share/.updates",
            "share/x.update/y",
            "share/a.update",
            "share/.update.d/x",
            "share/update/x",
            "share/.upd/x",
        ]);
    }

    #[test]
    fn names_that_only_look_like_records_are_ordinary_files() {
        // The rules narrow the design's installed*.json to the four literal names the helper
        // writes; a neighbour of them is a file like any other.
        expect_all_accepted(&[
            "share/katana/installed.json.bak",
            "share/katana/install.json.txt",
            "share/katana/installed.manifest.json.bak",
            "share/katana/installed.manifest.json.sig.bak",
            "share/katana/installed.other.json",
            "share/katana/xinstall.json",
            "share/katana/install.jsonx",
            "share/katana/my-installed.json",
            "share/katana/installed_json",
            "share/katana/install.json.d/x", // a folder of that name is not the file
        ]);
    }

    // ---- what an error says ------------------------------------------------------------------

    #[test]
    fn an_error_shows_the_ordinary_path_as_it_is() {
        let error = check_manifest_path("bin/con.txt").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.context, "bin/con.txt");
    }

    #[test]
    fn an_error_spells_bytes_that_are_not_printable_and_bounds_a_huge_path() {
        let with_nul = check_manifest_path(b"bin/a\0b\n").unwrap_err();
        assert_eq!(with_nul.context, "bin/a\\x00b\\x0a");
        assert!(!with_nul.describe().contains('\n'));

        let non_ascii = check_manifest_path(b"share/caf\xC3\xA9").unwrap_err();
        assert_eq!(non_ascii.context, "share/caf\\xc3\\xa9");

        // A megabyte of path is refused without being copied into the message.
        let huge = check_manifest_path(format!("bin/{}.", "a".repeat(1 << 20))).unwrap_err();
        assert!(huge.context.len() <= 210);
    }

    #[test]
    fn each_rule_says_something_of_its_own() {
        // Different faults, different sentences: a person reading a log can tell them apart.
        let mut messages = Vec::new();
        for path in [
            "",
            "bin/a\x01",
            "bin/a:b",
            "/bin/a",
            "bin//a",
            "other/a",
            "bin",
            "bin/a.",
            "bin/con",
            "bin/a~1",
            "share/katana/install.json",
        ] {
            let error = check_manifest_path(path).expect_err(path);
            messages.push(error.message);
        }
        let mut sorted = messages.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), messages.len());
    }

    // ---- the real tree -----------------------------------------------------------------------

    // Every regular file below `folder`, as a path relative to `root` with '/' between its names.
    // A folder that is a link is not entered (std::filesystem's recursive_directory_iterator does
    // not follow one either); a file is one if what it names is.
    fn files_under(root: &Path, folder: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(folder).expect("the folder is listed") {
            let entry = entry.expect("an entry of the folder");
            let path = entry.path();
            let kind = entry.file_type().expect("the entry's kind");
            if kind.is_dir() {
                files_under(root, &path, out);
            } else if std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
                let relative = path.strip_prefix(root).expect("below the root");
                let names: Vec<String> = relative
                    .components()
                    .map(|name| name.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(names.join("/"));
            }
        }
    }

    #[test]
    fn every_file_of_the_fixture_install_trees_passes_except_the_installers_own_record() {
        // The plain ASCII names of a real tree pass: these are the trees the producer's fixtures
        // are made from (tests/update/fixtures/install/v1 and v2), a stub of the shipped layout.
        let mut seen = 0;
        for version in ["v1", "v2"] {
            let root = Path::new(FIXTURES).join("install").join(version);
            assert!(root.is_dir(), "{}", root.display());
            let mut files = Vec::new();
            files_under(&root, &root, &mut files);
            for relative in files {
                seen += 1;
                if relative == "share/katana/install.json" {
                    // The CPack install writes it into the package; no manifest holds it.
                    assert!(!accepts(&relative), "{relative}");
                } else {
                    expect_accepted(&relative);
                }
            }
        }
        assert!(seen >= 20);
    }
}

// ---- lower_ascii ---------------------------------------------------------------------------------

mod update_lower_ascii {
    use katana_update::paths::lower_ascii;

    #[test]
    fn only_the_letters_a_to_z_are_folded_and_every_other_byte_is_kept() {
        assert_eq!(lower_ascii(""), b"");
        assert_eq!(
            lower_ascii("Share/KATANA/Install.JSON"),
            b"share/katana/install.json"
        );
        // The neighbours of the two ends of each range differ from a letter by one: '@' (0x40)
        // and '[' (0x5B) either side of A to Z, '`' (0x60) and '{' (0x7B) either side of a to z.
        // None of them is a letter.
        assert_eq!(lower_ascii("@AZ[`az{"), b"@az[`az{");
        // A byte above 0x7F is never a letter, whatever the locale: the UTF-8 of an A with a
        // diaeresis (0xC3 0x84) and a lone 0x80 are returned as they came, and so is a NUL
        // inside the text.
        assert_eq!(lower_ascii(b"\xC3\x84Z\x80"), b"\xC3\x84z\x80");
        assert_eq!(lower_ascii(b"A\0B"), b"a\0b");
    }
}

// ---- is_reserved_name ----------------------------------------------------------------------------

mod update_reserved_names {
    use katana_update::paths::is_reserved_name;

    #[test]
    fn the_four_record_names_are_reserved_as_a_bare_name_or_at_the_end_of_a_path() {
        for name in [
            "install.json",
            "installed.json",
            "installed.manifest.json",
            "installed.manifest.json.sig",
        ] {
            assert!(is_reserved_name(name), "{name}");
            assert!(is_reserved_name(format!("share/katana/{name}")), "{name}");
            assert!(is_reserved_name(format!("a/b/c/d/{name}")), "{name}");
        }
    }

    #[test]
    fn any_ascii_case_is_the_same_name() {
        assert!(is_reserved_name("Share/Katana/INSTALL.JSON"));
        assert!(is_reserved_name("INSTALLED.MANIFEST.JSON.SIG"));
        assert!(is_reserved_name("x/Installed.Manifest.Json"));
        assert!(is_reserved_name("iNsTaLlEd.JsOn"));
    }

    #[test]
    fn the_work_folder_is_reserved_as_any_component() {
        assert!(is_reserved_name(".update"));
        assert!(is_reserved_name(".UPDATE"));
        assert!(is_reserved_name(".update/lock"));
        assert!(is_reserved_name("R/.update/tx/journal"));
        assert!(is_reserved_name("a/.Update/b/c"));
    }

    #[test]
    fn a_backslash_separates_components_too() {
        // This is also the "never removed" test, which may be handed a native path: a backslash
        // can only make more names reserved here, not fewer.
        assert!(is_reserved_name("share\\katana\\install.json"));
        assert!(is_reserved_name("R\\.update\\lock"));
    }

    #[test]
    fn names_that_are_not_records_are_not_reserved() {
        for name in [
            "",
            "/",
            "bin/katana.exe",
            "installed.json.bak",
            "install.json.txt",
            "xinstall.json",
            "installed.manifest.json.sigx",
            "installed.manifest",
            "installed",
            "install",
            ".updated",
            ".updat",
            "update",
            "x.update",
            "share/katana/installed.other.json",
            "install.json/x",
            "installed.json/y",
        ] {
            assert!(!is_reserved_name(name), "'{name}'");
        }
    }
}

// ---- check_manifest_paths: the set ---------------------------------------------------------------

mod update_path_set {
    use super::*;
    use katana_update::paths::{MAX_FILES, MAX_INSTALL_BYTES};
    use std::time::{Duration, Instant};

    #[test]
    fn an_empty_list_is_fine() {
        assert!(check_set(&[] as &[&str], &[]).is_ok());
    }

    #[test]
    fn a_distinct_list_of_good_paths_passes() {
        assert!(check_set(&["bin/a.dll", "bin/b.dll", "share/katana/x"], &[1, 2, 3]).is_ok());
    }

    #[test]
    fn one_size_is_needed_for_each_path() {
        let error = check_set(&["bin/a.dll", "bin/b.dll"], &[1]).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(check_set(&["bin/a.dll"], &[1, 2]).is_err());
        assert!(check_set(&[] as &[&str], &[1]).is_err());
    }

    #[test]
    fn a_bad_path_anywhere_in_the_list_fails_the_whole_and_is_named() {
        let error = check_set(
            &["bin/a.dll", "bin/b.dll", "bin/c:d.dll", "bin/e.dll"],
            &[1, 1, 1, 1],
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.context, "bin/c:d.dll");
        // ... also the first and the last.
        assert!(check_set(&["C:/x", "bin/b.dll"], &[1, 1]).is_err());
        assert!(check_set(&["bin/a.dll", "bin/con"], &[1, 1]).is_err());
    }

    #[test]
    fn exactly_ten_thousand_files_pass_and_one_more_does_not() {
        assert_eq!(MAX_FILES, 10_000); // the design's cap (section 1), written out here

        let at_the_cap = sequential_paths(10_000);
        assert!(check_set_of_paths(&at_the_cap).is_ok());

        let over_the_cap = sequential_paths(10_001);
        let error = check_set_of_paths(&over_the_cap).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);

        assert!(check_set_of_paths(&sequential_paths(9_999)).is_ok());
    }

    #[test]
    fn the_count_is_checked_before_any_path_so_a_huge_list_costs_nothing() {
        // 10,001 paths that are each also wrong: the error is the count's, the cheap test.
        let bad = ["C:/x"; 10_001];
        let error = check_set_of_paths(&bad).unwrap_err();
        assert!(error.message.contains("10000"), "{}", error.message);
    }

    #[test]
    fn exactly_two_gibi_bytes_pass_and_one_byte_more_does_not() {
        assert_eq!(MAX_INSTALL_BYTES, 2 * GIB);

        assert!(check_set(&["bin/a.dll"], &[2 * GIB]).is_ok());
        assert!(check_set(&["bin/a.dll"], &[2 * GIB + 1]).is_err());
        // The sum, not each one.
        assert!(check_set(&["bin/a.dll", "bin/b.dll"], &[GIB, GIB]).is_ok());
        assert!(check_set(&["bin/a.dll", "bin/b.dll"], &[GIB, GIB + 1]).is_err());
        assert!(check_set(&["bin/a.dll", "bin/b.dll", "bin/c.dll"], &[GIB, GIB, 1]).is_err());
        assert!(check_set(&["bin/a.dll", "bin/b.dll", "bin/c.dll"], &[GIB, GIB - 1, 1]).is_ok());
        // Zero-byte files add nothing.
        assert!(check_set(&["bin/a.dll", "bin/b.dll"], &[2 * GIB, 0]).is_ok());
    }

    #[test]
    fn sizes_that_would_wrap_when_added_are_refused_not_summed_to_something_small() {
        let max = u64::MAX;
        // 2^64 - 1 plus 2 wraps to 1, which a naive running total would accept as tiny.
        assert!(check_set(&["bin/a.dll", "bin/b.dll"], &[max, 2]).is_err());
        assert!(check_set(&["bin/a.dll", "bin/b.dll"], &[2, max]).is_err());
        assert!(check_set(&["bin/a.dll"], &[max]).is_err());
        // Two halves of 2^63 add up to exactly 2^64, which is 0 when wrapped.
        let half = 1u64 << 63;
        assert!(check_set(&["bin/a.dll", "bin/b.dll"], &[half, half]).is_err());
        // Many sizes, each under the cap, whose sum wraps past 2^64 on the way.
        assert!(check_set(&["bin/a.dll", "bin/b.dll", "bin/c.dll"], &[2 * GIB, max, 2]).is_err());
    }

    #[test]
    fn two_paths_that_are_one_name_once_ascii_case_is_folded_are_refused() {
        for (first, second) in [
            ("bin/Foo.dll", "bin/foo.dll"),
            ("bin/FOO.DLL", "bin/foo.dll"),
            ("bin/a.dll", "bin/A.dll"),
            ("share/Katana/x", "share/katana/x"),
            ("share/katana/x", "share/katana/X"),
            ("bin/a_b-c.d", "bin/A_B-C.D"),
        ] {
            let Err(error) = check_set_of_paths(&[first, second]) else {
                panic!("{first} and {second}");
            };
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            // ... in either order, and with other names between them.
            assert!(check_set_of_paths(&[second, first]).is_err());
            assert!(check_set_of_paths(&[first, "bin/zzz1.dll", "bin/zzz2.dll", second]).is_err());
        }
        // The very same path twice is the same name too.
        assert!(check_set_of_paths(&["bin/a.dll", "bin/a.dll"]).is_err());
    }

    #[test]
    fn only_the_letters_a_to_z_fold_so_punctuation_that_differs_by_one_bit_is_not_a_duplicate() {
        // '[' (0x5B) and '{' (0x7B), '@' (0x40) and '`' (0x60), '\\' excluded by another rule:
        // they differ by 0x20 as 'A' and 'a' do, but they are not letters and have no other case.
        assert!(check_set_of_paths(&["share/a[b", "share/a{b"]).is_ok());
        assert!(check_set_of_paths(&["share/@x", "share/`x"]).is_ok());
        assert!(check_set_of_paths(&["share/a^b", "share/a~b"]).is_ok()); // 0x5E and 0x7E
        // Different letters and digits stay different.
        assert!(check_set_of_paths(&["bin/a1.dll", "bin/a2.dll", "bin/b1.dll"]).is_ok());
    }

    #[test]
    fn a_name_that_is_a_file_and_also_the_folder_of_another_is_refused() {
        assert!(check_set_of_paths(&["share/x", "share/x/y"]).is_err());
        assert!(check_set_of_paths(&["share/x/y", "share/x"]).is_err());
        assert!(check_set_of_paths(&["share/x", "share/x/y/z"]).is_err());
        assert!(check_set_of_paths(&["share/x/y", "share/x/y/z.txt"]).is_err());
        // ... after case folding, as the duplicate rule is.
        assert!(check_set_of_paths(&["share/X", "share/x/y"]).is_err());
        assert!(check_set_of_paths(&["share/x", "share/X/y"]).is_err());
        // A shared PREFIX of characters is not a shared folder.
        assert!(check_set_of_paths(&["share/x", "share/xy/z"]).is_ok());
        assert!(check_set_of_paths(&["share/x", "share/x.d/y"]).is_ok());
        assert!(check_set_of_paths(&["share/katana/readme", "share/katana/readme.txt"]).is_ok());
        // Siblings in one folder are fine.
        assert!(check_set_of_paths(&["share/x/y", "share/x/z"]).is_ok());
    }

    #[test]
    fn a_folder_is_found_wherever_the_byte_order_puts_its_files_and_names_between_are_not_in_the_way()
     {
        // In byte order '-' (0x2D) and '.' (0x2E) sort BEFORE '/' (0x2F), so "bin/a-b" and
        // "bin/a.d" sit between "bin/a" and "bin/a/c": the file and the file below it are not
        // neighbours in a sorted list, and a check of neighbours only would miss the pair.
        assert!(check_set_of_paths(&["bin/a", "bin/a-b", "bin/a.d", "bin/a/c"]).is_err());
        assert!(check_set_of_paths(&["bin/a/c", "bin/a.d", "bin/a-b", "bin/a"]).is_err());
        assert!(
            check_set_of_paths(&["share/x/y", "share/x/y-z", "share/x/y.w", "share/x/y/w"])
                .is_err()
        );
        // '0' (0x30) and 'a' (0x61) sort after '/', so these come after the folder's files.
        assert!(check_set_of_paths(&["bin/a", "bin/a0", "bin/aa", "bin/a/c", "bin/b"]).is_err());
        // The same names without a file below "bin/a": only neighbours by spelling, no folder.
        assert!(
            check_set_of_paths(&[
                "bin/a", "bin/a-b", "bin/a.d", "bin/a0", "bin/aa", "bin/a_", "bin/b/a"
            ])
            .is_ok()
        );
        // A file below a folder that is not itself listed is an ordinary tree.
        assert!(check_set_of_paths(&["bin/a/b", "bin/a/c/d", "bin/a-b/c", "bin/a.d/c"]).is_ok());
    }

    #[test]
    fn the_file_inside_a_file_is_named_by_the_inner_path_as_written() {
        // "bin/A/C" is inside "bin/a" once case is folded; the error shows it as the manifest
        // wrote it.
        let error = check_set_of_paths(&["bin/a", "bin/A-b", "bin/A/C"]).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.context, "bin/A/C");
    }

    #[test]
    fn one_path_of_megabytes_is_checked_in_time_proportional_to_its_length() {
        // "bin/a/a/a/..." of 2 MiB has about a million slashes. A signed manifest may hold such a
        // path (the cap on the manifest is 8 MiB), and a check that looked at every prefix of it
        // against the listed names would copy and hash a million prefixes of a mean half a MiB:
        // 10^12 bytes of work, in the helper, with the lock held. The bound is generous: a pass
        // takes milliseconds.
        let mut long_path = String::from("bin");
        while long_path.len() < 2 * 1024 * 1024 {
            long_path.push_str("/a");
        }
        assert!(accepts(&long_path));

        let begin = Instant::now();
        let with_x = format!("{long_path}/x");
        let with_y = format!("{long_path}/y");
        assert!(
            check_set_of_paths(&[
                long_path.as_str(),
                "bin/b",
                "share/x",
                with_x.as_str(),
                with_y.as_str()
            ])
            .is_err(),
            "long_path is a file and the folder of long_path/x"
        );
        assert!(check_set_of_paths(&[long_path.as_str(), "bin/b", "share/x"]).is_ok());
        assert!(check_set_of_paths(&[with_x.as_str(), with_y.as_str(), "bin/a/b"]).is_ok());
        // "bin/a/a" is a file and the folder of long_path: found at the shallowest slash.
        assert!(check_set_of_paths(&[long_path.as_str(), "bin/a/a"]).is_err());
        assert!(check_set_of_paths(&["bin/a/a", long_path.as_str()]).is_err());
        // Two of them, equal but for case in the middle: a duplicate once folded.
        let mut shouting = long_path.clone().into_bytes();
        let at = (shouting.len() / 2) & !1; // an even index holds an 'a'
        shouting[at] = b'A';
        assert_ne!(shouting, long_path.as_bytes());
        assert!(accepts(&shouting));
        assert!(check_set_of_paths(&[long_path.as_bytes(), shouting.as_slice()]).is_err());
        assert!(begin.elapsed() < Duration::from_secs(10));
    }
}

// ---- check_journal_path --------------------------------------------------------------------------

mod update_journal_paths {
    use katana_update::paths::{
        RECORD_PATHS, check_journal_path, check_manifest_path, is_reserved_name,
    };

    #[test]
    fn every_path_a_manifest_may_hold_is_a_journal_path() {
        for path in [
            "bin/katana.exe",
            "share/katana/readme.txt",
            "etc/katana/settings.ini",
            "lib/libx.so.1.2",
            "share/katana/installed.json.bak",
        ] {
            assert!(check_journal_path(path).is_ok(), "{path}");
        }
    }

    #[test]
    fn the_helpers_own_three_records_are_journal_paths_although_no_manifest_may_hold_them() {
        assert_eq!(RECORD_PATHS.len(), 3);
        for record in RECORD_PATHS {
            assert!(is_reserved_name(record), "{record}");
            assert!(check_manifest_path(record).is_err(), "{record}");
            assert!(check_journal_path(record).is_ok(), "{record}");
        }
        // Spelled out, as the module lists them.
        assert!(check_journal_path("share/katana/installed.json").is_ok());
        assert!(check_journal_path("share/katana/installed.manifest.json").is_ok());
        assert!(check_journal_path("share/katana/installed.manifest.json.sig").is_ok());
    }

    #[test]
    fn no_other_reserved_name_is_a_journal_path() {
        // install.json is written by the installer, never by the helper, so a journal that names
        // it is forged; so is the work folder and every other spelling.
        for path in [
            "share/katana/install.json",
            "bin/installed.json",
            "etc/installed.manifest.json",
            "lib/installed.manifest.json.sig",
            ".update/lock",
            "share/.update/x",
            "bin/.update",
            "R/.update/tx/journal",
        ] {
            assert!(check_journal_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn the_three_records_are_accepted_exactly_as_the_helper_writes_them_and_in_no_other_case() {
        // On a case-sensitive file system Share/Katana/... is another place, outside bin, share,
        // etc and lib, and the journal is not authenticated: a forged line must not get there, so
        // the exception for the records is by exact spelling.
        for path in [
            "Share/katana/installed.json",
            "share/Katana/installed.json",
            "share/katana/INSTALLED.JSON",
            "SHARE/KATANA/INSTALLED.MANIFEST.JSON",
            "share/katana/installed.manifest.json.SIG",
        ] {
            assert!(check_journal_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn a_record_name_with_anything_added_or_around_it_is_refused() {
        for path in [
            "share/katana/installed.json ",
            "share/katana/installed.json.",
            "share/katana/installed.json/",
            "/share/katana/installed.json",
            "share/katana//installed.json",
            "share/katana/./installed.json",
            "share/katana/../katana/installed.json",
            "share\\katana\\installed.json",
            "share/katana/installed.json:stream",
            " share/katana/installed.json",
            "share/katana/installed.json\r",
        ] {
            assert!(check_journal_path(path).is_err(), "{path}");
        }
        assert!(check_journal_path(b"share/katana/installed.json\0").is_err());
        assert!(check_journal_path(b"share/katana/installed.json\0x").is_err());
    }

    #[test]
    fn a_journal_path_cannot_leave_the_install_folders() {
        // Journal paths are untrusted: the manifest rules hold, so no path can name a place
        // outside R's bin, share, etc and lib (the file system's own reparse-point test is the
        // apply code's).
        for path in [
            "",
            "/",
            "/etc/passwd",
            "C:/Windows/system32/x.dll",
            "..",
            "../x",
            "bin/../../x",
            "bin/../../../etc/passwd",
            "bin\\..\\..\\x",
            "\\\\?\\C:\\x",
            "\\\\.\\pipe\\x",
            "other/x",
            "LICENSE",
            "bin",
            "bin/",
            "share/con",
            "bin/a:b",
            "bin/PROGRA~1",
        ] {
            assert!(check_journal_path(path).is_err(), "'{path}'");
        }
    }
}

mod update_journal_directories {
    use katana_update::paths::{
        INSTALL_ROOTS, RECORD_PATHS, check_journal_directory, check_journal_path,
        check_manifest_path,
    };

    #[test]
    fn a_bare_install_folder_is_a_directory_the_journal_may_name_but_never_a_file() {
        // A release that first brings etc or lib makes the folder and journals MKDIR for it; the
        // rollback must be able to remove it. The same bare name in an ADD, REPLACE or REMOVE
        // record would let a forged line delete the whole folder, so check_journal_path keeps
        // refusing it.
        for root in INSTALL_ROOTS {
            assert!(check_journal_directory(root).is_ok(), "{root}");
            assert!(check_journal_path(root).is_err(), "{root}");
            assert!(check_manifest_path(root).is_err(), "{root}");
        }
        assert_eq!(INSTALL_ROOTS.len(), 4);
        for root in ["bin", "share", "etc", "lib"] {
            assert!(check_journal_directory(root).is_ok(), "{root}");
        }
    }

    #[test]
    fn a_folder_below_an_install_folder_passes_as_a_files_path_would() {
        for path in [
            "share/katana",
            "etc/katana/profiles",
            "bin/platforms",
            "lib/x",
            "share/proj",
            "share/katana/installed.json.bak",
        ] {
            assert!(check_journal_directory(path).is_ok(), "{path}");
        }
    }

    #[test]
    fn only_the_four_exact_names_are_bare_folders_and_nothing_around_them_is() {
        for path in [
            "",
            "Bin",
            "SHARE",
            "Etc",
            "LIB",
            "bin/",
            "/bin",
            "bin//",
            "bin/.",
            "bin/..",
            "other",
            "usr",
            "binx",
            "bin ",
            "bin.",
            "bin\\",
            "bin:",
            "./bin",
            "../bin",
            "C:/bin",
            "bin/bin/..",
            "share/katana/../..",
        ] {
            assert!(check_journal_directory(path).is_err(), "'{path}'");
        }
        assert!(check_journal_directory(b"bin\0").is_err());
        assert!(check_journal_directory(b"etc\0lib").is_err());
    }

    #[test]
    fn a_directory_is_held_to_the_manifest_rules_and_no_record_is_one() {
        // The three records are files; a MKDIR of one is as forged as any other line.
        for record in RECORD_PATHS {
            assert!(check_journal_directory(record).is_err(), "{record}");
        }
        for path in [
            "bin/../x",
            "bin/con",
            "bin/a~1",
            "share/.update",
            ".update",
            "share/katana/install.json",
            "share/katana/INSTALLED.JSON",
            "bin/a.",
            "bin\\a",
            "share/katana/a:b",
            "other/x",
            "bin/a\x01",
        ] {
            assert!(check_journal_directory(path).is_err(), "{path}");
        }
    }
}
