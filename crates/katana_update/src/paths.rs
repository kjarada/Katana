//! The rules a file name from a manifest must pass before anything is written under it
//! (docs/update.md, "The path rules"). One rule set, used by the engine when a manifest is
//! verified and again by the helper program before it touches the install: the helper does not
//! trust the engine's answer, only the same code. tools/update_make_manifest.py and
//! tools/update_verify_release.py hold the same rules in Python, and tests/paths.rs holds all
//! three to the table tests/update/fixtures/path_rules.txt.
//!
//! A manifest path is relative, uses '/', and its first component is one of [`INSTALL_ROOTS`].
//! It is refused when it
//! - is empty, absolute, or has a drive or a root ('C:', a leading '/');
//! - has an empty, "." or ".." component;
//! - contains a byte outside printable ASCII (0x20 to 0x7E), so no non-ASCII name, no control
//!   character and no DEL: ASCII only is a rule on purpose. What two Unicode spellings of one
//!   name, or two cases of one non-ASCII letter, mean is the file system's business and differs
//!   between NTFS, APFS and ext4 (normalisation, case folding tables), so a rule that said "equal
//!   after folding" could be wrong on the machine that applies it. Every file Katana ships has an
//!   ASCII name (checked over the tree when this was written; the producer refuses one that does
//!   not, so a release cannot be made that no install accepts);
//! - contains a backslash, ':', or any of * ? " < > |;
//! - has a component ending in '.' or ' ' (Windows drops them, so two names become one);
//! - has a component that is a Windows device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9, with or
//!   without an extension, any case);
//! - has a component in 8.3 short form (contains '~' followed by digits);
//! - has a final component that is one of the reserved names - install.json, installed.json,
//!   installed.manifest.json, installed.manifest.json.sig - or has a ".update" component, ANY
//!   CASE: on NTFS and APFS share/katana/INSTALL.JSON is install.json. They are the updater's own
//!   records. (The design's pattern installed*.json is narrowed to these four literal names: the
//!   helper writes exactly these, and a name outside the list, installed.json.bak, is a file like
//!   any other.)
//!
//! And a whole manifest is refused when two paths are equal after ASCII case-folding (a file
//! system that folds case would write one over the other), when a path is also the folder of
//! another (bin/a and bin/a/b, also after folding: no volume holds both, so an install that
//! applied the set would fail half way), when it has more than [`MAX_FILES`] entries, or when the
//! sizes add up to more than [`MAX_INSTALL_BYTES`]. Only the letters A to Z fold: '[' and '{'
//! differ by the same bit and are two names. The cost of the set checks is the total length of
//! the paths times a logarithm, whatever the paths look like; no cap on the length of one path is
//! part of the rules (the table and both Python tools would have to carry it), so a signed
//! manifest may hold one path of megabytes and it is read in linear time.
//!
//! Paths are bytes here, as the C++'s are: a journal is read from a file, and a refusal of a byte
//! that is not UTF-8 must be reachable.

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Status};
use std::collections::HashSet;

pub const INSTALL_ROOTS: [&str; 4] = ["bin", "share", "etc", "lib"];
pub const MAX_FILES: usize = 10_000;
pub const MAX_INSTALL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The three records the helper itself writes at commit and so journals (REPLACE, with the old
/// copy under old/): they are reserved names, so [`check_manifest_path`] refuses them, yet a
/// journal legitimately carries them.
pub const RECORD_PATHS: [&str; 3] = [
    "share/katana/installed.json",
    "share/katana/installed.manifest.json",
    "share/katana/installed.manifest.json.sig",
];

const RESERVED_FILE_NAMES: [&str; 4] = [
    "install.json",
    "installed.json",
    "installed.manifest.json",
    "installed.manifest.json.sig",
];
const WORK_FOLDER_NAME: &str = ".update";

// ASCII only, on purpose: only the letters A to Z fold, whatever the locale (u8's ASCII case
// functions touch nothing else).
fn equals_ignoring_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

// A path as it appears in an error: bounded (a manifest can be megabytes) and with every byte
// that is not printable ASCII spelled \xNN, so a NUL, a newline or a byte of a UTF-8 sequence
// cannot reshape a log line. An ordinary path is shown as it is.
fn excerpt(path: &[u8]) -> String {
    const SHOWN: usize = 200;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let shown = path.len().min(SHOWN);
    let mut out = String::with_capacity(shown);
    for &c in &path[..shown] {
        if (0x20..=0x7e).contains(&c) {
            out.push(char::from(c));
        } else {
            out.push_str("\\x");
            out.push(char::from(HEX[usize::from(c >> 4)]));
            out.push(char::from(HEX[usize::from(c & 0x0f)]));
        }
    }
    if path.len() > shown {
        out.push_str("...");
    }
    out
}

fn refuse(rule: &str, path: &[u8]) -> Error {
    make_error(ErrorCode::InvalidArgument, rule, excerpt(path))
}

// "con", "prn", "aux", "nul", "com1".."com9", "lpt1".."lpt9" in any ASCII case. COM0 and LPT0 are
// NOT devices here: they are ordinary file names on the Windows 11 the producer's author tested,
// and tests/update/fixtures/path_rules.txt has no line that says otherwise.
fn is_device_stem(stem: &[u8]) -> bool {
    if stem.len() == 3 {
        return ["con", "prn", "aux", "nul"]
            .iter()
            .any(|name| equals_ignoring_ascii_case(stem, name.as_bytes()));
    }
    if stem.len() == 4 && (b'1'..=b'9').contains(&stem[3]) {
        return equals_ignoring_ascii_case(&stem[..3], b"com")
            || equals_ignoring_ascii_case(&stem[..3], b"lpt");
    }
    false
}

// '~' followed by a digit, anywhere in the name: "PROGRA~1.DLL", "KATANA~12".
fn has_short_name_marker(name: &[u8]) -> bool {
    name.windows(2)
        .any(|pair| pair[0] == b'~' && pair[1].is_ascii_digit())
}

// Splits on '/' and on '\\' both: this is the "never removed" test as well as the manifest rule,
// and a backslash can only make a name reserved here, never less so.
fn components(path: &[u8]) -> impl Iterator<Item = &[u8]> {
    path.split(|byte| *byte == b'/' || *byte == b'\\')
}

/// The text with A to Z made a to z and every other byte as it was: the case folding that these
/// rules are written in (never a locale's). The one definition, for the helper too when it
/// compares manifest paths.
pub fn lower_ascii(text: impl AsRef<[u8]>) -> Vec<u8> {
    text.as_ref().to_ascii_lowercase()
}

/// True for the updater's own record names, wherever they sit and in any ASCII case
/// ("Share/Katana/INSTALL.JSON" is one): such a file is never in a manifest and never removed.
/// Takes a whole path or a bare name.
pub fn is_reserved_name(path: impl AsRef<[u8]>) -> bool {
    let mut reserved = false;
    let mut parts = components(path.as_ref()).peekable();
    while let Some(component) = parts.next() {
        let last = parts.peek().is_none();
        if equals_ignoring_ascii_case(component, WORK_FOLDER_NAME.as_bytes()) {
            reserved = true;
        }
        if last
            && RESERVED_FILE_NAMES
                .iter()
                .any(|name| equals_ignoring_ascii_case(component, name.as_bytes()))
        {
            reserved = true;
        }
    }
    reserved
}

/// One path against the rules above. InvalidArgument naming the rule; the context is the path.
pub fn check_manifest_path(path: impl AsRef<[u8]>) -> Status {
    let path = path.as_ref();
    if path.is_empty() {
        return Err(refuse("the path is empty", path));
    }

    // One byte at a time, before anything is split: a name with a byte outside 0x20 to 0x7E (a
    // non-ASCII letter, a control character, DEL, a NUL) never reaches the component rules,
    // whose comparisons assume ASCII.
    if path.iter().any(|byte| !(0x20..=0x7e).contains(byte)) {
        return Err(refuse(
            "the path has a byte outside printable ASCII (0x20 to 0x7E): names are ASCII only, \
             with no control character or DEL",
            path,
        ));
    }
    const FORBIDDEN: &[u8] = b"\\:*?\"<>|";
    if path.iter().any(|byte| FORBIDDEN.contains(byte)) {
        return Err(refuse(
            "the path has a backslash, a colon (a drive or a stream) or one of * ? \" < > |",
            path,
        ));
    }
    if path[0] == b'/' {
        return Err(refuse("the path is absolute", path));
    }

    let parts: Vec<&[u8]> = components(path).collect();

    for &component in &parts {
        if component.is_empty() {
            return Err(refuse(
                "the path has an empty component (a doubled or a trailing '/')",
                path,
            ));
        }
        if matches!(component, b"." | b"..") {
            return Err(refuse("the path has a '.' or '..' component", path));
        }
    }

    let rooted = INSTALL_ROOTS.iter().any(|root| root.as_bytes() == parts[0]);
    if !rooted {
        return Err(refuse(
            "the first component is not one of bin, share, etc, lib",
            path,
        ));
    }
    if parts.len() < 2 {
        return Err(refuse(
            "the path names an install folder, not a file below it",
            path,
        ));
    }

    for &component in &parts {
        if matches!(component.last().copied(), Some(b'.' | b' ')) {
            return Err(refuse(
                "a component ends in '.' or ' ' (Windows drops it, so two names become one)",
                path,
            ));
        }
        // The device test is on what precedes the FIRST dot: "NUL.txt" and "com1.tar.gz" open
        // the device, not a file.
        let stem_end = component
            .iter()
            .position(|byte| *byte == b'.')
            .unwrap_or(component.len());
        if is_device_stem(&component[..stem_end]) {
            return Err(refuse(
                "a component is a Windows device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9)",
                path,
            ));
        }
        if has_short_name_marker(component) {
            return Err(refuse(
                "a component is in 8.3 short form ('~' and a digit)",
                path,
            ));
        }
    }

    if is_reserved_name(path) {
        return Err(refuse(
            "the path names one of the updater's own records or its working folder \
             (install.json, installed.json, installed.manifest.json(.sig), .update)",
            path,
        ));
    }
    Ok(())
}

/// Every path alone, then the whole: duplicates after case-folding and the count. `sizes` is
/// parallel to `paths` and is checked against [`MAX_INSTALL_BYTES`] (and for overflow of the
/// sum).
pub fn check_manifest_paths(paths: &[impl AsRef<[u8]>], sizes: &[u64]) -> Status {
    if paths.len() != sizes.len() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "checkManifestPaths: one size is needed for each path",
            format!("{} paths, {} sizes", paths.len(), sizes.len()),
        ));
    }
    // The count first: a hostile list is refused before any work is done per entry.
    if paths.len() > MAX_FILES {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("the manifest lists more than {MAX_FILES} files"),
            format!("{} files", paths.len()),
        ));
    }

    for path in paths {
        check_manifest_path(path)?;
    }

    // A running total that is compared before it is added to: no sum of sizes can wrap.
    let mut total = 0u64;
    for (path, &size) in paths.iter().zip(sizes) {
        if size > MAX_INSTALL_BYTES - total {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("the files add up to more than {MAX_INSTALL_BYTES} bytes (2 GiB)"),
                excerpt(path.as_ref()),
            ));
        }
        total += size;
    }

    // Two names that a file system folding case would make one, and a name that is a file as
    // well as the folder of another (bin/a and bin/A/b): no volume can hold either pair, so an
    // install that applied the set would fail half way. Both after folding ASCII case.
    let folded: Vec<Vec<u8>> = paths.iter().map(lower_ascii).collect();
    let mut seen: HashSet<&[u8]> = HashSet::with_capacity(paths.len());
    for (path, name) in paths.iter().zip(&folded) {
        if !seen.insert(name.as_slice()) {
            return Err(refuse(
                "two paths are the same name once ASCII case is folded",
                path.as_ref(),
            ));
        }
    }
    // The folded names in byte order, each with its place in the list. The names are distinct
    // (above), so the order is total.
    let mut ordered: Vec<(&[u8], usize)> = folded
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_slice(), index))
        .collect();
    ordered.sort_unstable();

    // "Q is a folder of another path" is "some listed name starts with Q and a slash", and the
    // names that do are one run of the sorted list, which begins at the first name not below
    // "Q/". One binary search for each name, so the cost is the sum of the lengths times a
    // logarithm: it must not depend on how many slashes a name has (a signed manifest may hold
    // one path of megabytes, "bin/a/a/a/...", and a probe of every prefix of it is quadratic).
    for name in &folded {
        let mut folder = name.clone();
        folder.push(b'/');
        let below = ordered.partition_point(|(entry, _)| *entry < folder.as_slice());
        if let Some(&(entry, index)) = ordered.get(below)
            && entry.starts_with(&folder)
        {
            return Err(refuse(
                "a path is inside another path that is listed as a file (case folded)",
                paths[index].as_ref(),
            ));
        }
    }
    Ok(())
}

/// What the path of a DIRECTORY (an MKDIR record) read back from a journal must pass: the same as
/// a file's under [`check_manifest_path`], and also a bare install folder - "etc", "lib" - which a
/// release that brings that folder for the first time makes, and which rollback must remove
/// again. Never one of [`RECORD_PATHS`] (they are files). A bare folder stays refused by
/// [`check_journal_path`], for the records that name a FILE: a forged "REMOVE bin" must not
/// delete a whole install folder.
pub fn check_journal_directory(path: impl AsRef<[u8]>) -> Status {
    let path = path.as_ref();
    // A release that first brings etc or lib makes that folder itself, and rollback removes it
    // again, so the journal names it bare. Only for a folder: a bare name in an ADD, REPLACE or
    // REMOVE record would let a forged line delete a whole install folder, and
    // check_journal_path keeps refusing it.
    if INSTALL_ROOTS.iter().any(|root| root.as_bytes() == path) {
        return Ok(());
    }
    // The records are files; a folder spelled as one is as forged as any other.
    check_manifest_path(path)
}

/// What a path read back from a journal must pass (the journal is not authenticated):
/// [`check_manifest_path`], or exactly one of [`RECORD_PATHS`], spelled as the helper writes it.
/// NOT compared ignoring case, unlike the reserved names: on a case-sensitive file system
/// "Share/katana/installed.json" is another place, outside bin, share, etc and lib, and the
/// journal promises that a forged line cannot reach one. A record spelled in another case is
/// refused here, and skipped like any forged line.
pub fn check_journal_path(path: impl AsRef<[u8]>) -> Status {
    let path = path.as_ref();
    if RECORD_PATHS.iter().any(|record| record.as_bytes() == path) {
        return Ok(());
    }
    check_manifest_path(path)
}
