//! The manifest reader (the twin of tests/update/test_manifest.cpp): the design's sample read field
//! by field, the fixtures (the producer's real manifests, good and bad), every missing or mistyped
//! member, every rule on a value, the places where a file's bytes may be in the payload, the caps
//! at exactly their boundaries, and hostile input (numbers too large, offsets that wrap, NUL bytes,
//! deep nesting, repeated members).
//!
//! Expectations are worked out from the contract (manifest.rs, the design's section 1) and from
//! independent oracles: UTC epoch seconds from Python's calendar.timegm, the sums and counts of the
//! fixtures from Python's json module, SHA-256 of "abc" and "" from FIPS 180-2. None is read off
//! the code under test (CLAUDE.md, section 4).

use katana_core::path_text::{path_from_utf8, read_file_bytes};
use katana_core::{Error, ErrorCode, Result};
use katana_update::feed::MAX_MANIFEST_BYTES;
use katana_update::manifest::{
    FileClass, Manifest, is_expired, parse_file_class, parse_manifest, parse_utc_time,
};
use katana_update::paths::{MAX_FILES, MAX_INSTALL_BYTES, check_manifest_path};
use katana_update::version;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/update/fixtures"
);

// SHA-256("abc") and SHA-256("") as FIPS 180-2 (appendix B.1) and every table of test vectors
// print them: any 64 lower-case hex digits would do for the parser; these are real hashes.
const HASH_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const HASH_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

const U64_MAX: u64 = u64::MAX;
const I64_MAX: u64 = i64::MAX as u64;
const GIB: u64 = 1024 * 1024 * 1024;

fn json_string(text: &str) -> String {
    format!("\"{text}\"")
}

// A JSON object as text, member by member in the order written, so a test can state the bytes it
// hands the parser (a number as "5.0", a string with a \u0000 escape, a member twice) instead of
// building a value the library would normalise.
#[derive(Clone, Default)]
struct Object {
    members: Vec<(String, String)>,
}

impl Object {
    fn set(&mut self, name: &str, raw_value: &str) -> &mut Self {
        if let Some(member) = self.members.iter_mut().find(|member| member.0 == name) {
            member.1 = raw_value.to_owned();
            return self;
        }
        self.members.push((name.to_owned(), raw_value.to_owned()));
        self
    }

    // Written again even when present: a duplicate member.
    fn add(&mut self, name: &str, raw_value: &str) -> &mut Self {
        self.members.push((name.to_owned(), raw_value.to_owned()));
        self
    }

    fn erase(&mut self, name: &str) -> &mut Self {
        self.members.retain(|member| member.0 != name);
        self
    }

    fn dump(&self) -> String {
        let mut out = String::from("{");
        for (i, (name, value)) in self.members.iter().enumerate() {
            out.push_str(if i == 0 { "\"" } else { ",\"" });
            out.push_str(name);
            out.push_str("\":");
            out.push_str(value);
        }
        out.push('}');
        out
    }
}

fn file_entry(path: &str) -> Object {
    file_entry_of(path, "data")
}

fn file_entry_of(path: &str, file_class: &str) -> Object {
    let mut file = Object::default();
    file.set("class", &json_string(file_class))
        .set("csize", "2")
        .set("offset", "0")
        .set("path", &json_string(path))
        .set("sha256", &json_string(HASH_EMPTY))
        .set("size", "0");
    file
}

#[derive(Clone, Default)]
struct Doc {
    top: Object,
    files: Vec<Object>,
}

impl Doc {
    fn text(&self) -> String {
        let mut copy = self.top.clone();
        let list: Vec<String> = self.files.iter().map(Object::dump).collect();
        copy.set("files", &format!("[{}]", list.join(",")));
        copy.dump()
    }
}

// The design's sample (design section 1), with real hashes: two files, the program first. 1234 +
// 15,500,000 and 15,501,300 + 4,100,000 both end inside the 212,345,678-byte payload.
fn sample() -> Doc {
    let mut doc = Doc::default();
    doc.top
        .set("applier", "1")
        .set("channel", &json_string("stable"))
        .set("expires", &json_string("2027-04-18T08:00:00Z"))
        .set("files", "[]")
        .set(
            "notes_url",
            &json_string("https://github.com/kjarada/Katana/releases/tag/v0.3.0"),
        )
        .set(
            "payload",
            r#"{"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":212345678}"#,
        )
        .set("product", &json_string("katana"))
        .set("published", &json_string("2026-10-20T08:00:00Z"))
        .set("schema", "1")
        .set("sequence", "1792312800")
        .set("tag", &json_string("v0.3.0"))
        .set("target", &json_string("win64"))
        .set("version", &json_string("0.3.0"));

    let mut program = Object::default();
    program
        .set("class", &json_string("program"))
        .set("csize", "15500000")
        .set("exec", "true")
        .set("offset", "1234")
        .set("path", &json_string("bin/katana.exe"))
        .set("sha256", &json_string(HASH_ABC))
        .set("size", "42312704");
    let mut library = Object::default();
    library
        .set("class", &json_string("lib"))
        .set("csize", "4100000")
        .set("offset", "15501300")
        .set("path", &json_string("bin/libgdal-39.dll"))
        .set("sha256", &json_string(HASH_EMPTY))
        .set("size", "11800000");
    doc.files = vec![program, library];
    doc
}

fn parse(text: impl AsRef<[u8]>) -> Result<Manifest> {
    parse_manifest(text)
}

fn parse_doc(doc: &Doc) -> Result<Manifest> {
    parse(doc.text())
}

fn described(parsed: &Result<Manifest>) -> String {
    match parsed {
        Ok(_) => String::new(),
        Err(error) => error.describe(),
    }
}

fn expect_accepted(doc: &Doc, what: &str) {
    let manifest = parse_doc(doc);
    assert!(manifest.is_ok(), "{what}: {}", described(&manifest));
}

// A refusal of the given kind. `member` is what the error's context names ("" for none).
fn expect_refused(text: impl AsRef<[u8]>, code: ErrorCode, member: &str, what: &str) {
    let error = parse(text).expect_err(&format!("{what}: accepted"));
    assert_eq!(error.code, code, "{what}: {}", error.describe());
    if !member.is_empty() {
        assert_eq!(error.context, member, "{what}: {}", error.describe());
    }
}

fn expect_malformed(doc: &Doc, member: &str, what: &str) {
    expect_refused(doc.text(), ErrorCode::ParseFailure, member, what);
}

fn expect_invalid(doc: &Doc, member: &str, what: &str) {
    expect_refused(doc.text(), ErrorCode::InvalidArgument, member, what);
}

fn read_fixture(relative: &str) -> Vec<u8> {
    read_file_bytes(&path_from_utf8(format!("{FIXTURES}/{relative}")))
        .unwrap_or_else(|error| panic!("{relative}: {}", error.describe()))
}

// A manifest of `count` files, each 2 bytes of deflate at its own offset, sorted by path (the
// zero-padded number is the sort key) and each of size `size`.
fn many_files(count: usize, size: u64) -> Doc {
    let mut doc = sample();
    doc.files.clear();
    for i in 0..count {
        let mut file = file_entry(&format!("share/katana/data/f{i:06}.dat"));
        file.set("offset", &format!("{}", i * 2))
            .set("size", &format!("{size}"));
        doc.files.push(file);
    }
    doc.top.set(
        "payload",
        &format!(
            r#"{{"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":{}}}"#,
            count * 2 + 100
        ),
    );
    doc
}

// One file placed in a payload of `payload_size` bytes; the rest of the sample around it. The file
// is as large as its stream (a stored file: deflate's own bound on the stream, which is the next
// group of tests, never comes into it), at most 2 GiB.
fn placed(offset: u64, csize: u64, payload_size: u64) -> Doc {
    let mut doc = sample();
    doc.files = vec![file_entry("bin/a")];
    doc.files[0]
        .set("offset", &format!("{offset}"))
        .set("csize", &format!("{csize}"))
        .set("size", &format!("{}", csize.min(2 * GIB)));
    doc.top.set(
        "payload",
        &format!(r#"{{"codec":"deflate","name":"a.zip","size":{payload_size}}}"#),
    );
    doc
}

// Every byte of an error's text is printable ASCII and the whole is short: nothing of the input is
// carried into a log line or a dialog.
fn expect_bounded_printable(error: &Error, what: &str) {
    let text = error.describe();
    assert!(text.len() < 400, "{what}");
    for byte in text.bytes() {
        assert!((0x20..=0x7e).contains(&byte), "{what}: byte {byte}");
    }
}

struct Fixture {
    directory: &'static str,
    manifest: &'static str,
    zip: &'static str, // "" for a notify-only manifest
    files: usize,
    sum_size: u64,
    sum_compressed: u64,
    version: &'static str,
    sequence: u64,
    tag: &'static str,
    published: &'static str,
    expires: &'static str,
    payload_size: u64,
}

// The counts, sums and instants are Python's json over the same files
// (tools/update_make_fixtures.py made them; this table does not read them back through the code
// under test).
const V1: Fixture = Fixture {
    directory: "release/v1",
    manifest: "Katana-update-win64.json",
    zip: "Katana-0.9.0-win64.update.zip",
    files: 9,
    sum_size: 2879,
    sum_compressed: 2535,
    version: "0.9.0",
    sequence: 1_790_000_000,
    tag: "v0.9.0",
    published: "2026-09-21T14:13:20Z",
    expires: "2027-03-20T14:13:20Z",
    payload_size: 3607,
};
const V2: Fixture = Fixture {
    directory: "release/v2",
    manifest: "Katana-update-win64.json",
    zip: "Katana-0.10.0-win64.update.zip",
    files: 10,
    sum_size: 2975,
    sum_compressed: 2601,
    version: "0.10.0",
    sequence: 1_792_312_800,
    tag: "v0.10.0",
    published: "2026-10-18T08:40:00Z",
    expires: "2027-04-16T08:40:00Z",
    payload_size: 3791,
};

fn expect_fixture_read(fixture: &Fixture) {
    let name = format!("{}/{}", fixture.directory, fixture.manifest);
    let m =
        parse(read_fixture(&name)).unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
    assert_eq!(m.files.len(), fixture.files, "{name}");
    assert_eq!(version::to_string(&m.version), fixture.version, "{name}");
    assert_eq!(m.sequence, fixture.sequence, "{name}");
    assert_eq!(m.tag, fixture.tag, "{name}");
    assert_eq!(m.published, fixture.published, "{name}");
    assert_eq!(m.expires, fixture.expires, "{name}");
    assert_eq!(m.product, "katana");
    assert_eq!(m.channel, "stable");
    assert_eq!(m.target, "win64");
    assert_eq!(m.schema, 1);
    assert_eq!(m.applier, 1);
    assert!(!m.notify_only);
    let payload = m
        .payload
        .as_ref()
        .unwrap_or_else(|| panic!("{name}: no payload"));
    assert_eq!(payload.codec, "deflate");
    assert_eq!(payload.name, fixture.zip);
    assert_eq!(payload.size, fixture.payload_size, "{name}");

    let mut sum_size = 0u64;
    let mut sum_compressed = 0u64;
    for file in &m.files {
        sum_size += file.size;
        sum_compressed += file.compressed_size;
        // Every range is inside the payload (the check the reader makes), against the REAL zip's
        // length from the file system.
        assert!(
            file.offset + file.compressed_size <= fixture.payload_size,
            "{}",
            file.path
        );
    }
    assert_eq!(sum_size, fixture.sum_size, "{name}");
    assert_eq!(sum_compressed, fixture.sum_compressed, "{name}");

    // The payload size the manifest states is the zip's size on disk.
    let zip = path_from_utf8(format!("{FIXTURES}/{}/{}", fixture.directory, fixture.zip));
    let on_disk = std::fs::metadata(&zip).map(|metadata| metadata.len()).ok();
    assert_eq!(on_disk, Some(fixture.payload_size), "{name}");
}

// ---- the design's sample -----------------------------------------------------------------------

mod update_manifest {
    use super::*;

    #[test]
    fn the_designs_sample_is_read_field_by_field() {
        let m = parse_doc(&sample()).unwrap_or_else(|error| panic!("{}", error.describe()));

        assert_eq!(m.schema, 1);
        assert_eq!(m.applier, 1);
        assert_eq!(m.channel, "stable");
        assert_eq!(m.published, "2026-10-20T08:00:00Z");
        assert_eq!(m.expires, "2027-04-18T08:00:00Z");
        assert_eq!(
            m.notes_url,
            "https://github.com/kjarada/Katana/releases/tag/v0.3.0"
        );
        assert_eq!(m.product, "katana");
        assert_eq!(m.sequence, 1_792_312_800);
        assert_eq!(m.tag, "v0.3.0");
        assert_eq!(m.target, "win64");
        assert_eq!(m.version.components, [0, 3, 0, 0]);
        assert_eq!(m.version.written, 3);
        assert!(!m.notify_only);

        let payload = m.payload.as_ref().expect("a payload");
        assert_eq!(payload.codec, "deflate");
        assert_eq!(payload.name, "Katana-0.3.0-win64.update.zip");
        assert_eq!(payload.size, 212_345_678);

        assert_eq!(m.files.len(), 2);
        assert_eq!(m.files[0].path, "bin/katana.exe");
        assert_eq!(m.files[0].file_class, FileClass::Program);
        assert!(m.files[0].executable);
        assert_eq!(m.files[0].size, 42_312_704);
        assert_eq!(m.files[0].compressed_size, 15_500_000);
        assert_eq!(m.files[0].offset, 1234);
        assert_eq!(m.files[0].sha256, HASH_ABC);
        assert_eq!(m.files[1].path, "bin/libgdal-39.dll");
        assert_eq!(m.files[1].file_class, FileClass::Lib);
        assert!(!m.files[1].executable);
        assert_eq!(m.files[1].size, 11_800_000);
        assert_eq!(m.files[1].compressed_size, 4_100_000);
        assert_eq!(m.files[1].offset, 15_501_300);
        assert_eq!(m.files[1].sha256, HASH_EMPTY);

        // verify_manifest fills these (manifest.rs); the parser alone does not.
        assert!(m.signed_json.is_empty());
        assert!(m.signature_line.is_empty());
    }

    #[test]
    fn the_same_properties_hold_for_the_designs_own_layout_over_several_lines() {
        // Whitespace between tokens is JSON's; the producer writes one member per line.
        let layout = format!(
            r#"{{
"applier":1,
"channel":"stable",
"expires":"2027-04-18T08:00:00Z",
"files":[
{{"class":"program","csize":15500000,"exec":true,"offset":1234,"path":"bin/katana.exe","sha256":"{HASH_ABC}","size":42312704}},
{{"class":"lib","csize":4100000,"offset":15501300,"path":"bin/libgdal-39.dll","sha256":"{HASH_EMPTY}","size":11800000}}
],
"notes_url":"https://github.com/kjarada/Katana/releases/tag/v0.3.0",
"payload":{{"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":212345678}},
"product":"katana",
"published":"2026-10-20T08:00:00Z",
"schema":1,
"sequence":1792312800,
"tag":"v0.3.0",
"target":"win64",
"version":"0.3.0"
}}
"#
        );
        let parsed = parse(&layout).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.version.written, 3);
    }

    #[test]
    fn the_files_are_kept_in_the_order_they_are_written() {
        let mut doc = sample();
        doc.files = vec![
            file_entry("bin/a.dll"),
            file_entry("bin/b.dll"),
            file_entry("etc/c.ini"),
            file_entry("share/d"),
        ];
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        let paths: Vec<&str> = parsed.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["bin/a.dll", "bin/b.dll", "etc/c.ini", "share/d"]);
    }

    #[test]
    fn the_four_file_classes_are_all_read() {
        let mut doc = sample();
        doc.files = vec![
            file_entry_of("bin/a", "program"),
            file_entry_of("bin/b", "lib"),
            file_entry_of("share/c", "data"),
            file_entry_of("share/d", "seed"),
        ];
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.files[0].file_class, FileClass::Program);
        assert_eq!(parsed.files[1].file_class, FileClass::Lib);
        assert_eq!(parsed.files[2].file_class, FileClass::Data);
        assert_eq!(parsed.files[3].file_class, FileClass::Seed);
    }

    #[test]
    fn exec_is_optional_and_absent_means_false() {
        // The producer writes "exec" only when true (tools/update_make_manifest.py file_entry).
        let mut doc = sample();
        doc.files = vec![
            file_entry("bin/a"),
            file_entry("bin/b"),
            file_entry("bin/c"),
        ];
        doc.files[1].set("exec", "true");
        doc.files[2].set("exec", "false");
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(!parsed.files[0].executable);
        assert!(parsed.files[1].executable);
        assert!(!parsed.files[2].executable);
    }

    // ---- a notify-only manifest --------------------------------------------------------------

    #[test]
    fn a_notify_only_manifest_has_no_files_and_no_payload() {
        let mut doc = sample();
        doc.top.erase("payload").set("notify_only", "true");
        doc.files.clear();
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(parsed.notify_only);
        assert!(parsed.files.is_empty());
        assert!(parsed.payload.is_none());
    }

    #[test]
    fn a_notify_only_manifest_with_a_payload_or_files_is_refused() {
        let mut with_payload = sample();
        with_payload.top.set("notify_only", "true");
        with_payload.files.clear();
        expect_invalid(&with_payload, "payload", "notify-only with a payload");

        let mut with_files = sample();
        with_files.top.erase("payload").set("notify_only", "true");
        expect_invalid(&with_files, "files", "notify-only with files");
    }

    #[test]
    fn notify_only_false_is_the_same_as_absent() {
        let mut doc = sample();
        doc.top.set("notify_only", "false");
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(!parsed.notify_only);
        // ... so it needs a payload and files like any other.
        let mut no_payload = sample();
        no_payload.top.set("notify_only", "false").erase("payload");
        expect_malformed(&no_payload, "payload", "not notify-only, no payload");
    }

    #[test]
    fn notify_only_must_be_a_boolean() {
        for wrong in ["\"true\"", "1", "0", "null", "[]", "{}", "\"\""] {
            let mut doc = sample();
            doc.top.erase("payload").set("notify_only", wrong);
            doc.files.clear();
            expect_malformed(&doc, "notify_only", wrong);
        }
    }

    #[test]
    fn a_manifest_that_installs_needs_at_least_one_file() {
        // Applied, an empty file list would remove everything the last version installed and put
        // nothing back.
        let mut doc = sample();
        doc.files.clear();
        expect_invalid(&doc, "files", "no files");
    }

    // ---- unknown members ---------------------------------------------------------------------

    #[test]
    fn members_it_does_not_know_are_ignored_at_every_level() {
        let mut doc = sample();
        doc.top
            .set(
                "future",
                r#"{"a":[1,2.5,{"b":null,"c":[true,false]}],"d":"x"}"#,
            )
            .set("another", "[[1],[2]]")
            .set("zz", "12345678901234567890123456789");
        doc.files[0]
            .set("mode", "493")
            .set("note", &json_string("kept out of the reader"));
        doc.files[1].set("signature", "{\"nested\":[]}");
        doc.top.set(
            "payload",
            r#"{"codec":"deflate","etag":"abc","name":"Katana-0.3.0-win64.update.zip","size":212345678}"#,
        );
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(
            parsed.payload.as_ref().expect("a payload").size,
            212_345_678
        );
    }

    // ---- missing and mistyped members --------------------------------------------------------

    #[test]
    fn every_required_member_of_the_manifest_is_required() {
        for name in [
            "applier",
            "channel",
            "expires",
            "files",
            "notes_url",
            "payload",
            "product",
            "published",
            "schema",
            "sequence",
            "tag",
            "target",
            "version",
        ] {
            // From the top object itself: a Doc always writes its own "files".
            let mut top = sample().top;
            top.erase(name);
            expect_refused(
                top.dump(),
                ErrorCode::ParseFailure,
                name,
                &format!("without {name}"),
            );
        }
    }

    #[test]
    fn every_member_has_its_own_type_and_no_other_is_accepted() {
        let not_string = ["1", "1.5", "true", "null", "[]", "{}", "[\"a\"]"];
        for name in [
            "channel",
            "expires",
            "notes_url",
            "product",
            "published",
            "tag",
            "target",
            "version",
        ] {
            for wrong in not_string {
                let mut doc = sample();
                doc.top.set(name, wrong);
                expect_malformed(&doc, name, &format!("{name} = {wrong}"));
            }
        }
        // A whole number: not a string, a float, a signed one, a boolean, a list. "5.0" and "1e3"
        // are floats to a JSON reader even though they are whole numbers to a person.
        let not_whole_number = [
            "\"1\"", "1.0", "1.5", "1e0", "1E0", "10e-1", "-1", "-0", "true", "null", "[]", "{}",
            "[1]",
        ];
        for name in ["applier", "schema", "sequence"] {
            for wrong in not_whole_number {
                let mut doc = sample();
                doc.top.set(name, wrong);
                expect_malformed(&doc, name, &format!("{name} = {wrong}"));
            }
        }
        for wrong in ["\"[]\"", "null", "{}", "1", "true", "\"\""] {
            // Doc::text() builds the list itself, so the wrong value goes into the top object.
            let mut top = sample().top;
            top.set("files", wrong);
            expect_refused(
                top.dump(),
                ErrorCode::ParseFailure,
                "files",
                &format!("files = {wrong}"),
            );
        }
    }

    #[test]
    fn the_payload_has_its_three_members_and_their_types() {
        let with_payload = |payload: &str| {
            let mut doc = sample();
            doc.top.set("payload", payload);
            doc
        };
        expect_malformed(
            &with_payload(r#"{"name":"a.zip","size":100000000}"#),
            "payload.codec",
            "payload without codec",
        );
        expect_malformed(
            &with_payload(r#"{"codec":"deflate","size":100000000}"#),
            "payload.name",
            "payload without name",
        );
        expect_malformed(
            &with_payload(r#"{"codec":"deflate","name":"a.zip"}"#),
            "payload.size",
            "payload without size",
        );
        for wrong in ["\"x\"", "[]", "null", "1", "true"] {
            expect_malformed(
                &with_payload(wrong),
                "payload",
                &format!("payload = {wrong}"),
            );
        }
        expect_malformed(
            &with_payload(r#"{"codec":1,"name":"a.zip","size":100000000}"#),
            "payload.codec",
            "codec number",
        );
        expect_malformed(
            &with_payload(r#"{"codec":"deflate","name":1,"size":100000000}"#),
            "payload.name",
            "name number",
        );
        expect_malformed(
            &with_payload(r#"{"codec":"deflate","name":"a.zip","size":"100000000"}"#),
            "payload.size",
            "size string",
        );
        expect_malformed(
            &with_payload(r#"{"codec":"deflate","name":"a.zip","size":1e8}"#),
            "payload.size",
            "size float",
        );
    }

    #[test]
    fn every_member_of_a_file_entry_is_required_except_exec() {
        for name in ["class", "csize", "offset", "path", "sha256", "size"] {
            let mut doc = sample();
            doc.files[1].erase(name);
            expect_malformed(
                &doc,
                &format!("files[1].{name}"),
                &format!("file without {name}"),
            );
        }
    }

    #[test]
    fn the_members_of_a_file_entry_have_their_own_types() {
        for name in ["class", "path", "sha256"] {
            for wrong in ["1", "1.5", "true", "null", "[]", "{}"] {
                let mut doc = sample();
                doc.files[0].set(name, wrong);
                expect_malformed(
                    &doc,
                    &format!("files[0].{name}"),
                    &format!("{name} = {wrong}"),
                );
            }
        }
        for name in ["csize", "offset", "size"] {
            for wrong in [
                "\"1\"", "1.0", "2.5", "1e3", "-1", "-0", "true", "null", "[]",
            ] {
                let mut doc = sample();
                doc.files[0].set(name, wrong);
                expect_malformed(
                    &doc,
                    &format!("files[0].{name}"),
                    &format!("{name} = {wrong}"),
                );
            }
        }
        for wrong in ["\"true\"", "1", "0", "null", "[]", "{}"] {
            let mut doc = sample();
            doc.files[1].set("exec", wrong);
            expect_malformed(&doc, "files[1].exec", &format!("exec = {wrong}"));
        }
    }

    #[test]
    fn each_entry_of_the_files_list_must_be_an_object() {
        for wrong in ["\"bin/a\"", "1", "null", "[]", "true", "[{}]"] {
            let mut top = sample().top;
            top.set("files", &format!("[{wrong}]"));
            expect_refused(
                top.dump(),
                ErrorCode::ParseFailure,
                "files[0]",
                &format!("entry = {wrong}"),
            );
        }
    }

    // ---- value rules -------------------------------------------------------------------------

    #[test]
    fn only_schema_one_is_read() {
        for wrong in ["0", "2", "3", "100", "4294967297", "18446744073709551615"] {
            let mut doc = sample();
            doc.top.set("schema", wrong);
            expect_invalid(&doc, "schema", &format!("schema {wrong}"));
        }
        // The schema is judged first: a manifest of another schema may differ in every other
        // member.
        let mut other = sample();
        other
            .top
            .set("schema", "2")
            .set("product", "[]")
            .set("version", "{}");
        other.files.clear();
        expect_invalid(&other, "schema", "schema 2 with everything else different");
    }

    #[test]
    fn the_applier_protocol_is_a_whole_number_from_one_to_the_largest_thirty_two_bit_one() {
        for good in ["1", "2", "7", "4294967295"] {
            let mut doc = sample();
            doc.top.set("applier", good);
            let parsed =
                parse_doc(&doc).unwrap_or_else(|error| panic!("{good}: {}", error.describe()));
            assert_eq!(parsed.applier, good.parse::<u32>().expect("a number"));
        }
        for bad in ["0", "4294967296", "18446744073709551615"] {
            let mut doc = sample();
            doc.top.set("applier", bad);
            expect_invalid(&doc, "applier", &format!("applier {bad}"));
        }
    }

    #[test]
    fn an_applier_above_what_this_build_speaks_is_not_a_parse_error() {
        // It means the staged helper must be used; refusing it here would stop the very update
        // that brings the newer helper.
        let mut doc = sample();
        doc.top.set("applier", "2");
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.applier, 2);
    }

    #[test]
    fn the_sequence_is_utc_seconds_from_one_to_the_year2100() {
        // The producer's own range (update_make_manifest.py): 1 to 4102444800, which is
        // 2100-01-01T00:00:00Z (Python calendar.timegm((2100, 1, 1, 0, 0, 0))).
        let mut doc = sample();
        doc.top.set("sequence", "1");
        assert_eq!(parse_doc(&doc).expect("sequence 1").sequence, 1);
        doc.top.set("sequence", "4102444800");
        assert_eq!(
            parse_doc(&doc).expect("sequence 2100").sequence,
            4_102_444_800
        );

        doc.top.set("sequence", "0");
        expect_invalid(&doc, "sequence", "sequence 0");
        doc.top.set("sequence", "4102444801");
        expect_invalid(&doc, "sequence", "one second past 2100");
        // The same instant as the fixtures' (1792312800) in milliseconds: a signed typo that would
        // outrank every later build of the same version.
        doc.top.set("sequence", "1792312800000");
        expect_invalid(&doc, "sequence", "epoch milliseconds");
        doc.top.set("sequence", "18446744073709551615");
        expect_invalid(&doc, "sequence", "sequence 2^64-1");
        // 2^64 is a float to the reader, so it is a mistyped member, not a wrapped 0.
        doc.top.set("sequence", "18446744073709551616");
        expect_malformed(&doc, "sequence", "sequence 2^64");
    }

    #[test]
    fn product_channel_target_and_tag_are_names_of_one_to_sixty_four_safe_characters() {
        let long = "x".repeat(64);
        let longer = "x".repeat(65);
        for key in ["product", "channel", "target", "tag"] {
            for good in [
                "katana",
                "stable",
                "win64",
                "win-arm64",
                "linux-x86_64",
                "macos-arm64",
                "v0.3.0",
                "A",
                "a.b_c-d",
                long.as_str(),
            ] {
                let mut doc = sample();
                doc.top.set(key, &json_string(good));
                expect_accepted(&doc, &format!("{key} = {good}"));
            }
            for bad in [
                "",
                longer.as_str(),
                "a b",
                "a/b",
                "a\\\\b",
                "..\\/x",
                "a:b",
                "a?b",
                "a\\\"b",
                "caf\u{e9}",
                "\\u00e9",
                "a\\u0000b",
                "\\u0000",
                "a\\nb",
                "a\\tb",
                "a%41",
                "a;b",
                "a b ",
                // "." and ".." are dot-segments of the URL the tag and the asset name go into
                // (releases/download/<tag>/<name>); the rest are first characters that are
                // neither a letter nor a digit.
                ".",
                "..",
                "...",
                ".hidden",
                ".v0.3.0",
                "-v1",
                "_v1",
                "-",
                "_",
            ] {
                let mut doc = sample();
                doc.top.set(key, &json_string(bad));
                expect_invalid(&doc, key, &format!("{key} = {bad}"));
            }
        }
    }

    #[test]
    fn a_name_may_have_dots_dashes_and_underscores_anywhere_after_its_first_character() {
        for key in ["product", "channel", "target", "tag"] {
            for good in ["0", "9a", "a.", "a..", "a-", "a_", "a..b", "7.-_"] {
                let mut doc = sample();
                doc.top.set(key, &json_string(good));
                expect_accepted(&doc, &format!("{key} = {good}"));
            }
        }
    }

    #[test]
    fn product_target_and_channel_are_checked_for_their_form_not_for_who_they_belong_to() {
        // That the product is katana, the target the install's own and the channel stable is
        // verify_manifest's (trust.rs): the reader only knows the shape.
        let mut doc = sample();
        doc.top
            .set("product", &json_string("other"))
            .set("target", &json_string("win-arm64"))
            .set("channel", &json_string("beta"));
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.product, "other");
        assert_eq!(parsed.target, "win-arm64");
        assert_eq!(parsed.channel, "beta");
    }

    #[test]
    fn the_version_must_be_major_minor_patch_build_of_digits() {
        for (text, components, written) in [
            ("0.3", [0, 3, 0, 0], 2u8),
            ("0.3.0", [0, 3, 0, 0], 3),
            ("0.10.0", [0, 10, 0, 0], 3),
            ("1.2.3.4", [1, 2, 3, 4], 4),
        ] {
            let mut doc = sample();
            doc.top.set("version", &json_string(text));
            let parsed =
                parse_doc(&doc).unwrap_or_else(|error| panic!("{text}: {}", error.describe()));
            assert_eq!(parsed.version.components, components, "{text}");
            assert_eq!(parsed.version.written, written, "{text}");
        }
        for bad in [
            "",
            "1",
            "1.",
            "1.2.3.4.5",
            "v1.2",
            "1.2-beta",
            "1.2 ",
            " 1.2",
            "1.1234567890",
            "4294967296.0",
            "a.b",
            "1.2\\u0000",
            "1..2",
            "-1.2",
            "1.2\\n",
        ] {
            let mut doc = sample();
            doc.top.set("version", &json_string(bad));
            expect_invalid(&doc, "version", &format!("version {bad}"));
        }
    }

    #[test]
    fn published_and_expires_are_real_utc_times() {
        for key in ["published", "expires"] {
            for good in [
                "2026-10-20T08:00:00Z",
                "1970-01-01T00:00:00Z",
                "2024-02-29T23:59:59Z",
                "2000-02-29T00:00:00Z",
                "9999-12-31T23:59:59Z",
            ] {
                let mut doc = sample();
                doc.top.set(key, &json_string(good));
                expect_accepted(&doc, &format!("{key} = {good}"));
            }
            for bad in [
                "",
                "2026-10-20",
                "2026-10-20 08:00:00Z",
                "2026-10-20T08:00:00",
                "2026-10-20T08:00:00z",
                "2026-10-20t08:00:00Z",
                "2026-10-20T08:00:00+00:00",
                "2026-10-20T08:00:00.000Z",
                "2026-10-20T08:00Z",
                "2026-10-20T8:00:00Z",
                "26-10-20T08:00:00Z",
                "2026-13-20T08:00:00Z",
                "2026-00-20T08:00:00Z",
                "2026-10-00T08:00:00Z",
                "2026-10-32T08:00:00Z",
                "2026-04-31T08:00:00Z",
                "2027-02-29T00:00:00Z",
                "2100-02-29T00:00:00Z",
                "2026-10-20T24:00:00Z",
                "2026-10-20T08:60:00Z",
                "2026-10-20T08:00:60Z",
                "1969-12-31T23:59:59Z",
                "0001-01-01T00:00:00Z",
                "2026-10-20T08:00:00Z ",
                " 2026-10-20T08:00:00Z",
                "2026-1O-20T08:00:00Z",
                "2026-10-20T08:00:00Z\\u0000",
            ] {
                let mut doc = sample();
                doc.top.set(key, &json_string(bad));
                expect_invalid(&doc, key, &format!("{key} = {bad}"));
            }
        }
    }

    #[test]
    fn expiry_is_advisory_a_manifest_that_has_expired_is_still_read() {
        let mut doc = sample();
        doc.top
            .set("published", &json_string("2001-01-01T00:00:00Z"))
            .set("expires", &json_string("2001-06-30T00:00:00Z"));
        let parsed = parse_doc(&doc).unwrap_or_else(|error| panic!("{}", error.describe()));
        // ... and the caller can ask, and word the answer (is_expired), without any refusal.
        assert!(is_expired(&parsed, 1_800_000_000));
        // An expires before published is odd but not this reader's business.
        let mut odd = sample();
        odd.top
            .set("published", &json_string("2026-10-20T08:00:00Z"))
            .set("expires", &json_string("2020-01-01T00:00:00Z"));
        assert!(parse_doc(&odd).is_ok());
    }

    #[test]
    fn the_notes_address_is_an_https_address_of_printable_characters() {
        for good in [
            String::from("https://github.com/kjarada/Katana/releases/tag/v0.3.0"),
            String::from("https://x"),
            String::from("https://example.org/a?b=c&d=e#f"),
            format!("https://{}", "a".repeat(2048 - 8)),
        ] {
            let mut doc = sample();
            doc.top.set("notes_url", &json_string(&good));
            expect_accepted(
                &doc,
                &format!("notes_url = {}", &good[..good.len().min(40)]),
            );
        }
        for bad in [
            String::new(),
            String::from("https://"),
            String::from("http://github.com/x"),
            String::from("ftp://x"),
            String::from("file:///c:/x"),
            String::from("javascript:alert(1)"),
            String::from("HTTPS://x"),
            String::from("//github.com/x"),
            String::from("github.com/x"),
            String::from("https://a b"),
            String::from("https://a\\tb"),
            String::from("https://a\\u0000b"),
            String::from("https://a\\u007fb"),
            String::from("https://caf\u{e9}"),
            String::from("https://a\\nb"),
            format!("https://{}", "a".repeat(2048 - 8 + 1)),
        ] {
            let mut doc = sample();
            doc.top.set("notes_url", &json_string(&bad));
            let shown: String = bad.chars().take(40).collect();
            expect_invalid(&doc, "notes_url", &format!("notes_url = {shown}"));
        }
    }

    #[test]
    fn the_payload_is_deflate_with_a_safe_name_and_a_size_of_one_byte_to_four_gibi_bytes() {
        let with_payload = |codec: &str, name: &str, size: &str| {
            let mut doc = sample();
            doc.files = vec![file_entry("bin/a")];
            doc.top.set(
                "payload",
                &format!(r#"{{"codec":{codec},"name":{name},"size":{size}}}"#),
            );
            doc
        };
        // Twice MAX_INSTALL_BYTES: 2 * 2 GiB = 4 GiB = 2^32.
        expect_accepted(
            &with_payload(
                "\"deflate\"",
                "\"Katana-0.3.0-win64.update.zip\"",
                "4294967296",
            ),
            "size 4 GiB",
        );
        expect_accepted(
            &with_payload("\"deflate\"", &json_string(&"a".repeat(255)), "100"),
            "a 255-character name",
        );
        expect_accepted(&with_payload("\"deflate\"", "\"a\"", "2"), "size 2");

        for codec in [
            "\"zstd\"",
            "\"Deflate\"",
            "\"DEFLATE\"",
            "\"\"",
            "\"deflate \"",
            "\"gzip\"",
            "\"stored\"",
        ] {
            expect_invalid(
                &with_payload(codec, "\"a.zip\"", "100"),
                "payload.codec",
                codec,
            );
        }
        for name in [
            String::new(),
            "a".repeat(256),
            String::from("a/b.zip"),
            String::from("a\\\\b.zip"),
            String::from(".hidden"),
            String::from(".."),
            String::from("."),
            String::from("-a.zip"),
            String::from("_a.zip"),
            String::from("a b.zip"),
            String::from("a\\u0000.zip"),
            String::from("caf\u{e9}.zip"),
            String::from("a:b"),
            String::from("a%2Fb"),
        ] {
            expect_invalid(
                &with_payload("\"deflate\"", &json_string(&name), "100"),
                "payload.name",
                &format!("name {name}"),
            );
        }
        for size in [
            "0",
            "4294967297",
            "4294967298",
            "9223372036854775807",
            "9223372036854775808",
            "18446744073709551615",
        ] {
            expect_invalid(
                &with_payload("\"deflate\"", "\"a.zip\"", size),
                "payload.size",
                size,
            );
        }
    }

    #[test]
    fn the_error_of_a_file_value_names_the_file_and_the_member() {
        let mut doc = sample();
        doc.files[1].set("class", &json_string("exe"));
        expect_invalid(&doc, "files[1].class", "class exe");

        let mut hash = sample();
        hash.files[0].set("sha256", &json_string("xyz"));
        expect_invalid(&hash, "files[0].sha256", "hash xyz");

        // A path error carries the path as its context and the member in its message.
        let mut path = sample();
        path.files[1].set("path", &json_string("bin/../x"));
        let error = parse_doc(&path).expect_err("accepted");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("files[1].path"), "{}", error.message);
        assert_eq!(error.context, "bin/../x");
    }

    #[test]
    fn the_file_class_is_one_of_the_four_spelled_in_lower_case() {
        for bad in [
            "",
            "Program",
            "PROGRAM",
            "Lib",
            "DATA",
            "seed ",
            " seed",
            "programs",
            "exe",
            "binary",
            "resource",
            "data\\u0000",
            "caf\u{e9}",
        ] {
            let mut doc = sample();
            doc.files[0].set("class", &json_string(bad));
            expect_invalid(&doc, "files[0].class", &format!("class {bad}"));
        }
    }

    #[test]
    fn the_hash_is_sixty_four_lower_case_hex_digits_and_nothing_else() {
        let good = "a".repeat(64);
        let mut ok = sample();
        ok.files[0].set("sha256", &json_string(&good));
        expect_accepted(&ok, "64 digits");
        let a63 = "a".repeat(63);
        for bad in [
            String::new(),
            a63.clone(),
            "a".repeat(65),
            "a".repeat(128),
            "A".repeat(64),
            format!("{a63}g"),
            format!("{a63}G"),
            format!("0x{}", "a".repeat(62)),
            format!(" {a63}"),
            format!("{a63} "),
            format!("{a63}\\u0000"),
            format!("{}{}", "a".repeat(32), "F".repeat(32)),
            format!("{a63}/"),
            format!("{a63}:"),
            format!("{a63}`"),
            format!("{a63}\u{e9}"),
        ] {
            let mut doc = sample();
            doc.files[0].set("sha256", &json_string(&bad));
            expect_invalid(&doc, "files[0].sha256", &format!("hash '{bad}'"));
        }
    }

    #[test]
    fn every_path_rule_applies_to_the_files_of_a_manifest() {
        for bad in [
            "",
            "/bin/a",
            "C:/bin/a",
            "bin\\\\a",
            "bin/a:b",
            "bin/../a",
            "bin/./a",
            "bin//a",
            "other/a",
            "bin",
            "bin/a.",
            "bin/a ",
            "bin/con",
            "bin/NUL.txt",
            "bin/PROGRA~1.DLL",
            "share/katana/install.json",
            "share/katana/INSTALLED.JSON",
            "share/.update/x",
            "share/caf\u{e9}",
            "bin/a*b",
            "bin/a\\u0001b",
            "bin/a\\u007fb",
            "bin/a\\u0000b",
        ] {
            let mut doc = sample();
            doc.files = vec![file_entry(bad)];
            let error = parse_doc(&doc).expect_err(&format!("accepted: {bad}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{bad}");
            assert!(
                error.message.contains("files[0].path"),
                "{bad}: {}",
                error.message
            );
        }
    }

    #[test]
    fn the_paths_of_the_shared_table_are_judged_here_as_they_are_by_the_rules_themselves() {
        // The manifest reader and check_manifest_path must not differ: a path accepted by one and
        // refused by the other would be a hole or a release nobody can install.
        for path in [
            "bin/katana.exe",
            "bin/a b.txt",
            "bin/com10.dll",
            "share/katana/installed.json.bak",
            "share/.updated/x",
            "bin/.hidden",
            "bin/COM0",
            "bin/a~b.dll",
        ] {
            assert!(check_manifest_path(path).is_ok(), "{path}");
            let mut doc = sample();
            doc.files = vec![file_entry(path)];
            expect_accepted(&doc, path);
        }
    }
}

// ---- where a file's bytes are ------------------------------------------------------------------

mod update_manifest_placement {
    use super::*;

    #[test]
    fn a_deflate_stream_is_never_shorter_than_two_bytes() {
        // The shortest deflate stream, an empty final block, is the two bytes 03 00 (RFC 1951,
        // section 3.2.3: BFINAL=1, BTYPE=01, end-of-block code 0000000 = 3 bits + 7 bits = 10
        // bits).
        expect_invalid(&placed(0, 0, 100), "files[0].csize", "csize 0");
        expect_invalid(&placed(0, 1, 100), "files[0].csize", "csize 1");
        expect_accepted(&placed(0, 2, 100), "csize 2");
        expect_accepted(&placed(0, 3, 100), "csize 3");
    }

    #[test]
    fn the_last_byte_of_the_data_may_be_the_last_byte_of_the_payload_and_no_later() {
        expect_accepted(&placed(0, 100, 100), "offset 0, csize 100, payload 100");
        expect_invalid(
            &placed(0, 101, 100),
            "files[0].offset",
            "one byte past the end",
        );
        expect_accepted(&placed(98, 2, 100), "offset+csize == payload.size");
        expect_invalid(
            &placed(99, 2, 100),
            "files[0].offset",
            "offset+csize == payload.size+1",
        );
        expect_invalid(&placed(98, 3, 100), "files[0].offset", "offset 98, csize 3");
        expect_accepted(&placed(1, 99, 100), "offset 1, csize 99");
        expect_invalid(
            &placed(1, 100, 100),
            "files[0].offset",
            "offset 1, csize 100",
        );
    }

    #[test]
    fn an_offset_at_or_past_the_end_of_the_payload_is_refused() {
        expect_invalid(
            &placed(100, 2, 100),
            "files[0].offset",
            "offset == payload.size",
        );
        expect_invalid(
            &placed(101, 2, 100),
            "files[0].offset",
            "offset > payload.size",
        );
        expect_invalid(
            &placed(100_000, 2, 100),
            "files[0].offset",
            "offset far past",
        );
    }

    #[test]
    fn offset_and_length_that_wrap_when_added_are_refused_not_summed_to_something_small() {
        // 2^64 - 1 + 3 = 2 (mod 2^64): a check written offset + csize <= size would accept it.
        expect_invalid(
            &placed(U64_MAX, 3, 100),
            "files[0].offset",
            "offset 2^64-1, csize 3",
        );
        expect_invalid(
            &placed(U64_MAX - 1, 3, 100),
            "files[0].offset",
            "offset 2^64-2, csize 3",
        );
        expect_invalid(
            &placed(U64_MAX, 2, 100),
            "files[0].offset",
            "offset 2^64-1, csize 2",
        );
        expect_invalid(
            &placed(U64_MAX, U64_MAX, 100),
            "files[0].offset",
            "both 2^64-1",
        );
        // 2 + (2^64 - 1) = 1 (mod 2^64)
        expect_invalid(
            &placed(2, U64_MAX, 100),
            "files[0].offset",
            "offset 2, csize 2^64-1",
        );
        // offset = payload.size, csize = 2^64 - payload.size: the sum is exactly 2^64, which is 0
        expect_invalid(
            &placed(100, U64_MAX - 99, 100),
            "files[0].offset",
            "sum is exactly 2^64",
        );
        expect_invalid(
            &placed(50, U64_MAX - 49, 100),
            "files[0].offset",
            "sum is exactly 2^64, offset 50",
        );
        // 2^63 + 2^63 = 2^64
        expect_invalid(
            &placed(I64_MAX + 1, I64_MAX + 1, 100),
            "files[0].offset",
            "two halves of 2^64",
        );
        // The same wraps against the largest payload the reader accepts, 2^32 bytes: 2^32 plus
        // 2^64 - 2^32 is exactly 2^64, which is 0; 2^32 - 1 plus 2^64 - 2^32 + 3 is 2.
        const LARGEST_PAYLOAD: u64 = 1 << 32;
        expect_invalid(
            &placed(
                LARGEST_PAYLOAD,
                U64_MAX - LARGEST_PAYLOAD + 1,
                LARGEST_PAYLOAD,
            ),
            "files[0].offset",
            "payload 2^32, offset 2^32, csize 2^64-2^32",
        );
        expect_invalid(
            &placed(
                LARGEST_PAYLOAD - 1,
                U64_MAX - LARGEST_PAYLOAD + 4,
                LARGEST_PAYLOAD,
            ),
            "files[0].offset",
            "payload 2^32, offset 2^32-1, csize 2^64-2^32+3",
        );
        expect_invalid(
            &placed(I64_MAX, I64_MAX, LARGEST_PAYLOAD),
            "files[0].offset",
            "both 2^63-1",
        );
    }

    #[test]
    fn the_largest_payload_is_four_gibi_bytes_and_data_may_end_at_its_last_byte() {
        const LARGEST_PAYLOAD: u64 = 4 * GIB;
        assert_eq!(LARGEST_PAYLOAD, 2 * MAX_INSTALL_BYTES);
        expect_accepted(
            &placed(LARGEST_PAYLOAD - 2, 2, LARGEST_PAYLOAD),
            "data in the last two bytes of 4 GiB",
        );
        expect_invalid(
            &placed(LARGEST_PAYLOAD - 1, 2, LARGEST_PAYLOAD),
            "files[0].offset",
            "one byte too many",
        );
        // A payload one byte over the cap is refused for its size, whatever is placed in it.
        expect_invalid(
            &placed(0, 2, LARGEST_PAYLOAD + 1),
            "payload.size",
            "payload 4 GiB + 1",
        );
        expect_invalid(&placed(0, 2, I64_MAX), "payload.size", "payload 2^63-1");
    }

    #[test]
    fn a_stream_may_be_its_files_size_plus_a_deflates_worst_case_and_not_one_byte_more() {
        // The bound is size + size / 8 + 64 (integer division). Worked out by hand:
        //   size 0 -> 0 + 0 + 64 = 64;  size 7 -> 7 + 0 + 64 = 71;  size 8 -> 8 + 1 + 64 = 73;
        //   size 999 -> 999 + 124 + 64 = 1187;  size 1000 -> 1000 + 125 + 64 = 1189;
        //   size 2^31 -> 2147483648 + 268435456 + 64 = 2415919168.
        for (size, largest) in [
            (0u64, 64u64),
            (7, 71),
            (8, 73),
            (999, 1187),
            (1000, 1189),
            (2 * GIB, 2_415_919_168),
        ] {
            let with_stream = |csize: u64| {
                let mut doc = placed(0, csize, 4 * GIB);
                doc.files[0].set("size", &format!("{size}"));
                doc
            };
            expect_accepted(
                &with_stream(largest),
                &format!("size {size}, csize {largest}"),
            );
            expect_invalid(
                &with_stream(largest + 1),
                "files[0].csize",
                &format!("size {size}, csize {}", largest + 1),
            );
        }
    }

    #[test]
    fn a_stream_an_encoder_can_really_make_of_an_incompressible_file_is_accepted() {
        let stream_of = |size: u64, csize: u64| {
            let mut doc = placed(0, csize, 4 * GIB);
            doc.files[0].set("size", &format!("{size}"));
            doc
        };
        // An encoder that codes every byte of 144 to 255 as a 9-bit fixed-Huffman literal (RFC
        // 1951, 3.2.6) writes 3 bits of header, 9 bits a byte and 7 bits of end-of-block:
        // ceil((9n + 10) / 8) bytes. By hand: n=1: 19 bits -> 3; n=2: 28 -> 4; n=100: 910 -> 114
        // (113.75); n=4096: 36874 -> 4610 (4609.25); n=65535: 589825 -> 73729 (73728.125).
        for (size, stream) in [
            (1u64, 3u64),
            (2, 4),
            (100, 114),
            (4096, 4610),
            (65535, 73729),
        ] {
            expect_accepted(
                &stream_of(size, stream),
                &format!("size {size}, fixed-Huffman stream {stream}"),
            );
        }
        // And a real one, from an independent encoder: Python 3.14.7 (zlib-ng 2.2.4), level 1,
        // random.Random(0).randbytes(5_000_000) -> 5,273,379 bytes, 5.5% over (level 9:
        // 5,001,520). The old bound of a part in a thousand refused this release for every
        // installed copy.
        expect_accepted(
            &stream_of(5_000_000, 5_273_379),
            "zlib-ng level 1 on 5,000,000 random bytes",
        );
        expect_accepted(
            &stream_of(5_000_000, 5_001_520),
            "zlib-ng level 9 on the same bytes",
        );
    }

    #[test]
    fn a_stream_of_any_length_that_is_placed_past_the_payload_is_refused_for_its_place_first() {
        // Both are wrong; the range check is the one a caller can act on first. A stream of
        // 2^64-1 bytes of a 10-byte file is in no payload.
        let mut doc = placed(0, 2, 100);
        doc.files[0]
            .set("csize", "18446744073709551615")
            .set("size", "10");
        expect_invalid(&doc, "files[0].offset", "csize 2^64-1 in a payload of 100");
        // And inside the payload but far beyond the file: refused for the stream's own length.
        let mut inside = placed(0, 2, 4 * GIB);
        inside.files[0]
            .set("csize", &format!("{GIB}"))
            .set("size", "10");
        expect_invalid(
            &inside,
            "files[0].csize",
            "a 1 GiB stream for a 10-byte file",
        );
    }

    #[test]
    fn a_file_may_be_as_large_as_two_gibi_bytes_and_no_larger() {
        assert_eq!(MAX_INSTALL_BYTES, 2 * GIB);
        let mut at_cap = placed(0, 2, 100);
        at_cap.files[0].set("size", &format!("{}", 2 * GIB));
        expect_accepted(&at_cap, "size 2 GiB");
        let mut past = placed(0, 2, 100);
        past.files[0].set("size", &format!("{}", 2 * GIB + 1));
        expect_invalid(&past, "files[0].size", "size 2 GiB + 1");
        let mut huge = placed(0, 2, 100);
        huge.files[0].set("size", "18446744073709551615");
        expect_invalid(&huge, "files[0].size", "size 2^64-1");
    }

    #[test]
    fn several_files_are_each_placed_inside_the_payload_and_may_share_nothing_in_particular() {
        // Offsets are not required to be ordered or disjoint: the reader checks each file's own
        // range.
        let mut doc = sample();
        doc.files = vec![
            file_entry("bin/a"),
            file_entry("bin/b"),
            file_entry("bin/c"),
        ];
        doc.files[0]
            .set("offset", "90")
            .set("csize", "10")
            .set("size", "10");
        doc.files[1]
            .set("offset", "0")
            .set("csize", "100")
            .set("size", "100");
        doc.files[2]
            .set("offset", "95")
            .set("csize", "5")
            .set("size", "5");
        doc.top.set(
            "payload",
            r#"{"codec":"deflate","name":"a.zip","size":100}"#,
        );
        expect_accepted(&doc, "three files in 100 bytes");
        // The second one past the end is the one named.
        doc.files[1].set("csize", "101");
        expect_invalid(&doc, "files[1].offset", "the second past the end");
    }
}

// ---- order, uniqueness and the caps ------------------------------------------------------------

mod update_manifest_files {
    use super::*;

    #[test]
    fn the_list_must_be_sorted_by_path_in_byte_order() {
        let mut doc = sample();
        doc.files = vec![file_entry("bin/b"), file_entry("bin/a")];
        expect_invalid(&doc, "files[1].path", "b then a");

        // Byte order, not case-insensitive order: 'B' (0x42) sorts before 'a' (0x61).
        let mut upper_first = sample();
        upper_first.files = vec![file_entry("bin/B"), file_entry("bin/a")];
        expect_accepted(&upper_first, "B then a");
        let mut lower_first = sample();
        lower_first.files = vec![file_entry("bin/a"), file_entry("bin/B")];
        expect_invalid(&lower_first, "files[1].path", "a then B");

        // A folder's files do not sort among its siblings' by anything but the bytes: '/' (0x2F)
        // is above '-' (0x2D) and '.' (0x2E) and below '0' (0x30).
        let mut folder = sample();
        folder.files = vec![
            file_entry("share/a-b"),
            file_entry("share/a.b"),
            file_entry("share/a/b"),
            file_entry("share/a0"),
        ];
        expect_accepted(&folder, "- . / 0");
        let mut wrong_slash = sample();
        wrong_slash.files = vec![
            file_entry("share/a-b"),
            file_entry("share/a/b"),
            file_entry("share/a.b"),
        ];
        expect_invalid(&wrong_slash, "files[2].path", "- / .");
    }

    #[test]
    fn the_paths_must_be_unique_exactly_and_after_folding_case() {
        let mut exact = sample();
        exact.files = vec![file_entry("bin/a"), file_entry("bin/a")];
        expect_invalid(&exact, "files[1].path", "the same path twice");

        // 'A' sorts before 'a', so this list is sorted and the duplicate is the case-folded kind.
        let mut folded = sample();
        folded.files = vec![file_entry("bin/A.dll"), file_entry("bin/a.dll")];
        let error = parse_doc(&folded).expect_err("accepted");
        assert_eq!(error.code, ErrorCode::InvalidArgument);

        let mut folder = sample();
        folder.files = vec![file_entry("share/Katana/x"), file_entry("share/katana/x")];
        assert!(parse_doc(&folder).is_err());
    }

    #[test]
    fn a_name_that_is_a_file_and_the_folder_of_another_is_refused() {
        let mut doc = sample();
        doc.files = vec![file_entry("share/x"), file_entry("share/x/y")];
        assert!(parse_doc(&doc).is_err());
        let mut sibling = sample();
        sibling.files = vec![file_entry("share/x"), file_entry("share/x.d/y")];
        expect_accepted(&sibling, "x and x.d/y");
    }

    #[test]
    fn exactly_ten_thousand_files_are_accepted_and_one_more_is_not() {
        assert_eq!(MAX_FILES, 10_000);

        let at_cap = many_files(10_000, 0).text();
        let parsed = parse(&at_cap).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.files.len(), 10_000);
        assert_eq!(
            parsed.files.first().expect("files").path,
            "share/katana/data/f000000.dat"
        );
        assert_eq!(
            parsed.files.last().expect("files").path,
            "share/katana/data/f009999.dat"
        );
        assert!(
            (at_cap.len() as u64) < MAX_MANIFEST_BYTES,
            "the cap's manifest must itself fit"
        );

        expect_refused(
            many_files(10_001, 0).text(),
            ErrorCode::InvalidArgument,
            "files",
            "10,001 files",
        );
        expect_accepted(&many_files(9_999, 0), "9,999 files");
    }

    #[test]
    fn the_sizes_of_all_the_files_may_add_up_to_two_gibi_bytes_and_no_more() {
        let at_cap = many_files(2, GIB);
        expect_accepted(&at_cap, "1 GiB + 1 GiB");
        let mut over = many_files(2, GIB);
        over.files[1].set("size", &format!("{}", GIB + 1));
        let error = parse_doc(&over).expect_err("accepted");
        assert_eq!(error.code, ErrorCode::InvalidArgument);

        // Three files whose sizes are each fine and whose sum is 2 GiB + 1.
        let mut three = many_files(3, 0);
        three.files[0].set("size", &format!("{GIB}"));
        three.files[1].set("size", &format!("{GIB}"));
        three.files[2].set("size", "1");
        assert!(parse_doc(&three).is_err());
        three.files[2].set("size", "0");
        expect_accepted(&three, "2 GiB in two of three");
    }

    #[test]
    fn the_manifest_itself_is_bounded_to_the_largest_the_engine_will_fetch() {
        let text = sample().text();
        assert!((text.len() as u64) < MAX_MANIFEST_BYTES);

        // Exactly MAX_MANIFEST_BYTES: JSON white space after the object is harmless.
        let at_cap = format!(
            "{text}{}",
            " ".repeat(MAX_MANIFEST_BYTES as usize - text.len())
        );
        assert_eq!(at_cap.len() as u64, MAX_MANIFEST_BYTES);
        assert!(parse(&at_cap).is_ok());
        // One byte more is refused before it is read.
        let error = parse(format!("{at_cap} ")).expect_err("accepted");
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }
}

// ---- hostile text ------------------------------------------------------------------------------

mod update_manifest_hostile {
    use super::*;

    #[test]
    fn text_that_is_not_a_json_object_is_a_parse_failure() {
        let texts: &[&[u8]] = &[
            b"",
            b" ",
            b"\n",
            b"null",
            b"true",
            b"123",
            b"\"text\"",
            b"[]",
            b"[{}]",
            b"{",
            b"}",
            b"{}}",
            b"{}{}",
            b"{} x",
            b"{\"a\":}",
            b"{\"a\" 1}",
            b"{a:1}",
            b"{'a':1}",
            b"{\"a\":1,}",
            b"[1,]",
            b"{\"a\":01}",
            b"{\"a\":.5}",
            b"{\"a\":+1}",
            b"{\"a\":NaN}",
            b"{\"a\":Infinity}",
            b"{\"a\":-Infinity}",
            b"{\"a\":undefined}",
            b"{\"a\":\"\\x41\"}",
            b"{\"a\":\"\\u12\"}",
            b"{\"a\":\"\\ud800\"}",
            b"{\"a\":\"\\udc00\"}",
            b"{\"a\":\"line\nbreak\"}",
            b"{\"a\":\"tab\there\"}",
            b"/* c */ {}",
            b"{} // c",
            b"{\"a\":1 /* c */}",
            b"# c\n{}",
            b"{\0}",
            b"{\"a\":\"\0\"}",
            b"\0{}",
            b"{}\0",
            b"{\"a\":\"\xFF\"}",
            b"{\"a\":\"\xC0\x80\"}",
            b"{\"a\":\"\xED\xA0\x80\"}",
            b"{\"a\":\"\xE2\x82\"}",
            b"{\"\xFF\":1}",
        ];
        for &text in texts {
            let error =
                parse(text).expect_err(&format!("accepted: {}", String::from_utf8_lossy(text)));
            assert_eq!(
                error.code,
                ErrorCode::ParseFailure,
                "{}: {}",
                String::from_utf8_lossy(text),
                error.describe()
            );
            assert!(!error.describe().contains('\0'));
        }
    }

    #[test]
    fn a_valid_object_without_the_members_is_missing_them_not_malformed_json() {
        expect_refused("{}", ErrorCode::ParseFailure, "schema", "empty object");
    }

    #[test]
    fn a_byte_order_mark_is_refused_though_a_json_reader_might_skip_it() {
        let with_bom = format!("\u{feff}{}", sample().text());
        assert!(with_bom.as_bytes().starts_with(b"\xEF\xBB\xBF"));
        expect_refused(&with_bom, ErrorCode::ParseFailure, "", "BOM");
        // Without it, the same text is the sample.
        assert!(parse(sample().text()).is_ok());
    }

    #[test]
    fn a_nul_byte_outside_a_string_is_refused_because_the_json_reader_takes_it_for_the_end() {
        // The JSON reader stops at a NUL as at the end of its input, so a good manifest followed by
        // a NUL and anything at all would be read as the good manifest alone, while the Python
        // verifier, or anything that sees the whole file, reads something else. Each of these is
        // a complete, valid manifest with a NUL placed somewhere it ends the read.
        let good = sample().text();
        assert!(parse(&good).is_ok());
        let half = good.len() / 2;
        let texts = [
            format!("{good}\0"),
            format!("{good}\0garbage"),
            format!("{good}\0{good}"),
            format!("{good}\n\0"),
            format!("{good}\0\0"),
            format!("\0{good}"),
            format!("{}\0{}", &good[..half], &good[half..]),
            format!("{}\0}}", &good[..good.len() - 1]),
        ];
        for (i, text) in texts.iter().enumerate() {
            let error = parse(text).expect_err(&format!("case {i} was read"));
            assert_eq!(error.code, ErrorCode::ParseFailure, "{i}");
            assert!(error.message.contains("NUL"), "{i}: {}", error.message);
        }
    }

    #[test]
    fn trailing_garbage_after_the_object_is_refused() {
        let text = sample().text();
        for tail in ["x", "{}", ",", "\n,", " 1", "\0"] {
            expect_refused(
                format!("{text}{tail}"),
                ErrorCode::ParseFailure,
                "",
                &format!("tail '{tail}'"),
            );
        }
        // White space is not garbage.
        assert!(parse(format!("{text} \n\t\r\n ")).is_ok());
        assert!(parse(format!(" \n{text}")).is_ok());
    }

    #[test]
    fn a_number_too_large_for_sixty_four_bits_is_refused_not_wrapped() {
        // Past 2^64 - 1 a whole number is a float to the reader, so it is a mistyped member; far
        // enough past it (more than a double holds, about 1.8e308) the JSON reader itself refuses.
        // Either way the value is never reduced modulo 2^64 into a small one.
        let nines = "9".repeat(400);
        for key in ["sequence", "applier", "schema"] {
            let mut doc = sample();
            doc.top.set(key, &nines);
            expect_malformed(&doc, "", &format!("{key} = 400 nines"));
            doc.top.set(key, "18446744073709551616"); // 2^64
            expect_malformed(&doc, key, &format!("{key} = 2^64"));
        }
        for key in ["csize", "offset", "size"] {
            let mut doc = sample();
            let member = format!("files[0].{key}");
            doc.files[0].set(key, &nines);
            expect_malformed(&doc, "", &format!("{key} = 400 nines"));
            doc.files[0].set(key, "18446744073709551616"); // 2^64
            expect_malformed(&doc, &member, &format!("{key} = 2^64"));
            doc.files[0].set(key, "99999999999999999999999999999999999999");
            expect_malformed(&doc, &member, &format!("{key} = 38 nines"));
            doc.files[0].set(key, "36893488147419103232"); // 2^65
            expect_malformed(&doc, &member, &format!("{key} = 2^65"));
        }
        let mut exponent = sample();
        exponent.files[0].set("size", "1e308");
        expect_malformed(&exponent, "files[0].size", "1e308");
        exponent.files[0].set("size", "1e999999");
        expect_malformed(&exponent, "", "1e999999");
    }

    #[test]
    fn a_nul_byte_inside_a_string_is_refused_wherever_the_value_is_used() {
        // \u0000 is legal JSON and gives a string with a NUL in it, which a C string would end at.
        // Each of these would read as a good value if the NUL ended the string.
        for key in [
            "channel",
            "product",
            "tag",
            "target",
            "version",
            "notes_url",
            "published",
            "expires",
        ] {
            let value = match key {
                "version" => "0.3.0\\u0000junk",
                "notes_url" => "https://github.com/x\\u0000junk",
                "published" | "expires" => "2026-10-20T08:00:00Z\\u0000junk",
                _ => "ok\\u0000junk",
            };
            let mut doc = sample();
            doc.top.set(key, &json_string(value));
            expect_invalid(&doc, key, &format!("{key} with a NUL"));
        }
        let mut path = sample();
        path.files[0].set("path", &json_string("bin/a.dll\\u0000../../x"));
        assert!(parse_doc(&path).is_err());
        let mut class = sample();
        class.files[0].set("class", &json_string("data\\u0000"));
        expect_invalid(&class, "files[0].class", "class with a NUL");
        let mut hash = sample();
        hash.files[0].set(
            "sha256",
            &json_string(&format!("{}\\u0000", &HASH_ABC[..63])),
        );
        expect_invalid(&hash, "files[0].sha256", "hash with a NUL");
        // A member NAME with a NUL is just an unknown member.
        let mut name = sample();
        name.top.set("future\\u0000x", "1");
        assert!(parse_doc(&name).is_ok());
    }

    #[test]
    fn nesting_at_the_limit_is_read_and_one_level_deeper_is_refused() {
        // The manifest object is level 1; a member holding N nested lists reaches level 1 + N. The
        // limit is 16 levels (manifest.rs).
        let nested = |levels: usize| {
            let mut doc = sample();
            doc.top.set(
                "future",
                &format!("{}{}", "[".repeat(levels), "]".repeat(levels)),
            );
            doc
        };
        expect_accepted(&nested(15), "16 levels in all");
        expect_refused(
            nested(16).text(),
            ErrorCode::ParseFailure,
            "",
            "17 levels in all",
        );
        expect_refused(
            nested(100).text(),
            ErrorCode::ParseFailure,
            "",
            "101 levels",
        );
    }

    #[test]
    fn deep_nesting_that_never_closes_is_refused_without_exhausting_the_stack() {
        for open in ["[", "{\"a\":", "[{\"a\":"] {
            let text = open.repeat(200_000);
            let error = parse(&text).expect_err(open);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{open}");
        }
        // The same inside the manifest's own members.
        let deep = format!("{{\"future\":{}", "[".repeat(100_000));
        assert!(parse(&deep).is_err());
        // Nested objects, closed, 50 deep inside a file entry.
        let mut doc = sample();
        let objects = format!("{}1{}", "{\"k\":".repeat(50), "}".repeat(50));
        doc.files[0].set("extra", &objects);
        assert!(parse_doc(&doc).is_err());
    }

    #[test]
    fn a_member_written_twice_is_refused_wherever_it_is_written() {
        // A reader that kept the last of two "sha256" members and one that kept the first would
        // disagree about what is signed: so there is no "which one", the manifest is refused.
        let mut top = sample();
        top.top.add("version", &json_string("9.9.9"));
        expect_refused(
            top.text(),
            ErrorCode::ParseFailure,
            "version",
            "version twice",
        );

        let mut schema = sample();
        schema.top.add("schema", "1");
        expect_refused(
            schema.text(),
            ErrorCode::ParseFailure,
            "schema",
            "schema twice, the same value",
        );

        let mut file = sample();
        file.files[1].add("sha256", &json_string(HASH_ABC));
        expect_refused(
            file.text(),
            ErrorCode::ParseFailure,
            "sha256",
            "sha256 twice in a file",
        );

        let mut payload = sample();
        payload.top.set(
            "payload",
            r#"{"codec":"deflate","codec":"deflate","name":"a.zip","size":1000000000}"#,
        );
        expect_refused(
            payload.text(),
            ErrorCode::ParseFailure,
            "codec",
            "codec twice",
        );

        let mut unknown = sample();
        unknown.top.set("future", r#"{"a":1,"b":{"c":2,"c":3}}"#);
        expect_refused(
            unknown.text(),
            ErrorCode::ParseFailure,
            "c",
            "a duplicate inside an unknown member",
        );

        let mut unknown_top = sample();
        unknown_top.top.add("future", "1").add("future", "2");
        expect_refused(
            unknown_top.text(),
            ErrorCode::ParseFailure,
            "future",
            "an unknown member twice",
        );

        // The same name in two different objects is not a duplicate: every file has a "path".
        let mut fine = sample();
        fine.files[0].set("class", &json_string("program"));
        fine.top.set("future", r#"{"path":1,"files":{"path":2}}"#);
        expect_accepted(&fine, "the same name in different objects");
    }

    #[test]
    fn two_spellings_of_one_member_name_are_one_member_written_twice() {
        // "version" is "version" once the escape is read; a check made on the raw text, not on
        // the name the reader ends up with, would see two different members.
        let mut top = sample();
        top.top.add("vers\\u0069on", &json_string("9.9.9"));
        expect_refused(
            top.text(),
            ErrorCode::ParseFailure,
            "version",
            "version twice, once escaped",
        );

        let mut file = sample();
        file.files[0].add("sha\\u0032\\u0035\\u0036", &json_string(HASH_ABC));
        expect_refused(
            file.text(),
            ErrorCode::ParseFailure,
            "sha256",
            "sha256 twice, once escaped",
        );
    }

    #[test]
    fn the_error_of_a_hostile_value_never_carries_raw_control_characters() {
        let mut doc = sample();
        doc.top.set("tag", &json_string("a\\nb\\u0000c\\u001b[31m"));
        let error = parse_doc(&doc).expect_err("accepted");
        let text = error.describe();
        for byte in text.bytes() {
            assert!(byte >= 0x20, "{text}");
        }

        let mut path = sample();
        path.files[0].set("path", &json_string("bin/a\\nb\\u0000\\u001b"));
        let error = parse_doc(&path).expect_err("accepted");
        for byte in error.describe().bytes() {
            assert!(byte >= 0x20);
        }
    }

    #[test]
    fn a_json_syntax_error_says_where_the_reader_stopped_and_carries_none_of_the_input() {
        // nlohmann's own text ends with the token it was in: here a megabyte of x, cut by a raw
        // control character (JSON strings may not hold one), which would put 1 MB into the error.
        let long_string = format!("{{\"a\":\"{}\x01\"}}", "x".repeat(1_000_000));
        let error = parse(&long_string).expect_err("accepted");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        expect_bounded_printable(&error, "1 MB string cut by a control character");
        // The place is given: the control character is the byte at index 1,000,006 (6 bytes of
        // '{"a":"' and 1,000,000 x), so the offset the reader names is 1,000,006 to 1,000,008
        // whether it counts from 0 or 1 and whether it counts the byte that stopped it.
        let message = &error.message;
        let at = message.find("byte ").unwrap_or_else(|| panic!("{message}"));
        let digits: String = message[at + 5..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let offset: u64 = digits.parse().unwrap_or_else(|_| panic!("{message}"));
        assert!(offset >= 1_000_006, "{message}");
        assert!(offset <= 1_000_008, "{message}");

        // Raw bytes that are not UTF-8 in a string, in a key and in front of the object; a
        // sequence cut short; a DEL and an escape character where a literal or a number goes.
        let texts: Vec<Vec<u8>> = vec![
            b"{\"a\":\"\xFF\xFE\"}".to_vec(),
            b"{\"\xC0\x80\":1}".to_vec(),
            b"{\"a\":\"\xE2\x82\"}".to_vec(),
            {
                let mut hostile = b"{\"a\":\"".to_vec();
                hostile.extend_from_slice(&[0xff; 5000]);
                hostile.extend_from_slice(b"\"}");
                hostile
            },
            b"\xFF\xFE{}".to_vec(),
            b"{\"a\":tru\x7F}".to_vec(),
            b"{\"a\":1\x1B[31m}".to_vec(),
        ];
        for text in &texts {
            let error = parse(text).expect_err(&format!("{}", text.len()));
            expect_bounded_printable(&error, &format!("hostile bytes, length {}", text.len()));
        }
    }

    #[test]
    fn a_manifest_of_many_unknown_members_is_read_in_reasonable_time() {
        // 200,000 unknown members of a few bytes each (about 2 MB): nothing quadratic in the
        // reader. A pass takes well under a second; a reader that compared every member with every
        // other would need 2 * 10^10 comparisons, so a generous bound tells them apart.
        let mut doc = sample();
        let mut members = String::from("{");
        for i in 0..200_000 {
            members.push_str(if i == 0 { "\"k" } else { ",\"k" });
            members.push_str(&format!("{i}\":1"));
        }
        members.push('}');
        doc.top.set("future", &members);
        let text = doc.text();
        assert!((text.len() as u64) < MAX_MANIFEST_BYTES);
        let begin = std::time::Instant::now();
        let parsed = parse(&text);
        let elapsed = begin.elapsed();
        assert!(parsed.is_ok());
        assert!(elapsed < std::time::Duration::from_secs(10));
    }
}

// ---- file classes ------------------------------------------------------------------------------

mod update_file_class {
    use super::*;
    use katana_update::manifest::to_string;

    #[test]
    fn each_class_has_its_one_lower_case_name() {
        assert_eq!(to_string(FileClass::Program), "program");
        assert_eq!(to_string(FileClass::Lib), "lib");
        assert_eq!(to_string(FileClass::Data), "data");
        assert_eq!(to_string(FileClass::Seed), "seed");
    }

    #[test]
    fn the_names_parse_back_and_nothing_else_does() {
        for file_class in [
            FileClass::Program,
            FileClass::Lib,
            FileClass::Data,
            FileClass::Seed,
        ] {
            assert_eq!(parse_file_class(to_string(file_class)), Ok(file_class));
        }
        for bad in [
            "", "Program", "LIB", "Data", "seeds", "program ", " lib", "exe", "p", "data\n",
        ] {
            let error = parse_file_class(bad).expect_err(bad);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{bad}");
        }
        assert!(parse_file_class("data\0").is_err());
    }
}

// ---- UTC times and expiry ----------------------------------------------------------------------

mod update_utc_time {
    use super::*;

    #[test]
    fn reads_the_seconds_since_the_epoch_as_the_calendar_counts_them() {
        // The values are Python's calendar.timegm over the same instants.
        for (text, epoch) in [
            ("1970-01-01T00:00:00Z", 0i64),
            ("1970-01-01T00:00:01Z", 1),
            ("1972-02-29T00:00:00Z", 68_169_600),
            ("1999-12-31T23:59:59Z", 946_684_799),
            ("2000-02-29T12:34:56Z", 951_827_696),
            ("2000-03-01T00:00:00Z", 951_868_800),
            ("2024-02-29T00:00:00Z", 1_709_164_800),
            ("2026-10-18T08:40:00Z", 1_792_312_800),
            ("2026-10-20T08:00:00Z", 1_792_483_200),
            ("2026-12-31T23:59:59Z", 1_798_761_599),
            ("2027-04-18T08:00:00Z", 1_808_035_200),
            ("2038-01-19T03:14:07Z", 2_147_483_647),
            ("2038-01-19T03:14:08Z", 2_147_483_648),
            ("2100-02-28T00:00:00Z", 4_107_456_000),
            ("2100-03-01T00:00:00Z", 4_107_542_400),
            ("9999-12-31T23:59:59Z", 253_402_300_799),
        ] {
            assert_eq!(parse_utc_time(text), Some(epoch), "{text}");
        }
    }

    #[test]
    fn the_designs_sequence_is_the_published_time_of_the_fixtures() {
        // tools/update_make_manifest.py writes sequence = published in epoch seconds (1792312800).
        let published = parse_utc_time("2026-10-18T08:40:00Z").expect("a time");
        assert_eq!(published, 1_792_312_800);
        // expires = published + 180 days = 180 * 86400 = 15,552,000 seconds later.
        let expires = parse_utc_time("2027-04-16T08:40:00Z").expect("a time");
        assert_eq!(expires - published, 15_552_000);
    }

    #[test]
    fn a_text_that_is_not_exactly_that_form_or_not_a_real_date_is_nothing() {
        for bad in [
            "",
            "x",
            "2026-10-20T08:00:00",
            "2026-10-20T08:00:00Zz",
            "2026-10-20T08:00:00z",
            "2026-10-20 08:00:00Z",
            "2026/10/20T08:00:00Z",
            "2026-10-20T08-00-00Z",
            "2026-10-20T08:00:00+0000",
            "20261020T080000Z",
            "2026-1-20T08:00:00Z",
            "2026-10-2T08:00:00Z",
            "+026-10-20T08:00:00Z",
            "2026-10-20T08:00:0\u{b2}Z",
            "2026-10-20T0\u{668}:00:00Z",
            "1969-12-31T23:59:59Z",
            "2026-00-10T00:00:00Z",
            "2026-13-10T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-06-31T00:00:00Z",
            "2026-09-31T00:00:00Z",
            "2026-11-31T00:00:00Z",
            "2026-01-32T00:00:00Z",
            "2026-01-00T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:60:00Z",
            "2026-01-01T00:00:60Z",
            "1900-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
        ] {
            assert_eq!(parse_utc_time(bad), None, "{bad}");
        }
        assert_eq!(parse_utc_time("2026-10-20T08:00:00Z\0"), None);
        assert_eq!(parse_utc_time("\u{0}026-10-20T08:00:00Z"), None);
        // The leap-day rule: divisible by 4, except centuries, except those divisible by 400.
        assert!(parse_utc_time("2024-02-29T00:00:00Z").is_some());
        assert!(parse_utc_time("2000-02-29T00:00:00Z").is_some());
        assert!(parse_utc_time("2100-02-29T00:00:00Z").is_none());
        assert!(parse_utc_time("2023-02-29T00:00:00Z").is_none());
    }

    #[test]
    fn the_longest_months_have_their_last_day_and_no_more() {
        for last in [
            "2026-01-31",
            "2026-03-31",
            "2026-04-30",
            "2026-05-31",
            "2026-06-30",
            "2026-07-31",
            "2026-08-31",
            "2026-09-30",
            "2026-10-31",
            "2026-11-30",
            "2026-12-31",
            "2026-02-28",
        ] {
            assert!(
                parse_utc_time(&format!("{last}T00:00:00Z")).is_some(),
                "{last}"
            );
        }
    }
}

mod update_expiry {
    use super::*;

    #[test]
    fn expired_means_now_is_after_the_expiry_instant_and_nothing_is_refused_by_it() {
        let manifest = Manifest {
            expires: String::from("2027-04-16T08:40:00Z"), // 1807864800
            ..Manifest::default()
        };
        assert!(!is_expired(&manifest, 0));
        assert!(!is_expired(&manifest, 1_807_864_799));
        assert!(!is_expired(&manifest, 1_807_864_800)); // at the instant, not yet after it
        assert!(is_expired(&manifest, 1_807_864_801));
        assert!(is_expired(&manifest, i64::MAX));
        assert!(!is_expired(&manifest, i64::MIN));
    }

    #[test]
    fn an_expiry_that_cannot_be_read_is_never_expired() {
        let mut manifest = Manifest::default(); // expires is empty
        assert!(!is_expired(&manifest, 4_000_000_000));
        manifest.expires = String::from("soon");
        assert!(!is_expired(&manifest, 4_000_000_000));
    }
}

// ---- the producer's fixtures -------------------------------------------------------------------

mod update_manifest_fixtures {
    use super::*;

    #[test]
    fn the_first_release_is_read_as_its_producer_wrote_it() {
        expect_fixture_read(&V1);
    }

    #[test]
    fn the_second_release_is_read_as_its_producer_wrote_it() {
        expect_fixture_read(&V2);
        let m = parse(read_fixture("release/v2/Katana-update-win64.json")).expect("v2 reads");
        // Spot checks against the file itself (its first and last entry).
        let first = m.files.first().expect("files");
        assert_eq!(first.path, "bin/katana.exe");
        assert_eq!(first.file_class, FileClass::Program);
        assert!(first.executable);
        assert_eq!(first.size, 207);
        assert_eq!(first.compressed_size, 85);
        assert_eq!(first.offset, 44);
        assert_eq!(
            first.sha256,
            "b70d132208d7dd410c0daba7f5fcb5a1594c844a73d499845bea1856fb6bf8da"
        );
        let last = m.files.last().expect("files");
        assert_eq!(last.path, "share/katana/readme.txt");
        assert_eq!(last.file_class, FileClass::Data);
        assert!(!last.executable);
        assert_eq!(
            last.sha256,
            "44b16c8491aaefa9a3070a9e1afa87d82c2af76c1b21d6c6494d3348237c3d7c"
        );
        // The empty file: size 0 and the shortest deflate stream, two bytes, and the hash of "".
        let empty = m
            .files
            .iter()
            .find(|file| file.path == "share/katana/empty.txt")
            .expect("the empty file");
        assert_eq!(empty.size, 0);
        assert_eq!(empty.compressed_size, 2);
        assert_eq!(empty.sha256, HASH_EMPTY);
        // The seed.
        let seed = m
            .files
            .iter()
            .find(|file| file.file_class == FileClass::Seed)
            .expect("a seed");
        assert_eq!(seed.path, "etc/katana/settings.ini");
    }

    #[test]
    fn the_spare_signed_release_has_the_manifest_of_the_second() {
        // Only the signature differs, the manifest is byte for byte the second release's.
        assert_eq!(
            read_fixture("release/v2-spare-signed/Katana-update-win64.json"),
            read_fixture("release/v2/Katana-update-win64.json")
        );
        expect_fixture_read(&Fixture {
            directory: "release/v2-spare-signed",
            ..V2
        });
    }

    #[test]
    fn the_macos_manifest_is_notify_only() {
        let m = parse(read_fixture(
            "release/v2-macos-notify/Katana-update-macos-arm64.json",
        ))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(m.notify_only);
        assert!(m.files.is_empty());
        assert!(m.payload.is_none());
        assert_eq!(m.target, "macos-arm64");
        assert_eq!(version::to_string(&m.version), "0.10.0");
        assert_eq!(m.sequence, 1_792_312_800);
        assert_eq!(m.tag, "v0.10.0");
        assert_eq!(
            m.notes_url,
            "https://github.com/kjarada/Katana/releases/tag/v0.10.0"
        );
    }

    #[test]
    fn the_order_of_the_two_releases_is_the_versions_and_not_the_text() {
        let v1 = parse(read_fixture("release/v1/Katana-update-win64.json")).expect("v1 reads");
        let v2 = parse(read_fixture("release/v2/Katana-update-win64.json")).expect("v2 reads");
        // 0.10.0 is above 0.9.0 (10 > 9); as text "0.10.0" < "0.9.0".
        assert!(v2.version > v1.version);
        assert!(
            v2.tag < v1.tag,
            "the tags sort the wrong way as text, which is why no one compares them"
        );
        assert!(v2.sequence > v1.sequence);
    }

    #[test]
    fn a_downgrade_by_version_is_an_ordinary_manifest_for_the_later_checks_to_judge() {
        // It parses: that it is below what is installed is is_newer_than's to say (trust.rs). Its
        // sequence is the HIGHEST of all the fixtures, so only the version keeps it from winning.
        let downgrade = parse(read_fixture(
            "release/bad/downgrade-version/Katana-update-win64.json",
        ))
        .expect("the downgrade reads");
        let v2 = parse(read_fixture("release/v2/Katana-update-win64.json")).expect("v2 reads");
        assert_eq!(version::to_string(&downgrade.version), "0.9.0");
        assert_eq!(downgrade.sequence, 1_792_316_400);
        assert!(downgrade.sequence > v2.sequence);
        assert!(downgrade.version < v2.version);
    }

    #[test]
    fn a_downgrade_by_sequence_is_the_same_version_one_second_earlier() {
        let earlier = parse(read_fixture(
            "release/bad/downgrade-sequence/Katana-update-win64.json",
        ))
        .expect("the downgrade reads");
        let v2 = parse(read_fixture("release/v2/Katana-update-win64.json")).expect("v2 reads");
        assert_eq!(earlier.version, v2.version);
        assert_eq!(earlier.sequence, 1_792_312_799);
        assert_eq!(v2.sequence - earlier.sequence, 1);
    }

    #[test]
    fn a_flipped_bit_in_the_hash_is_still_a_manifest_the_signature_is_what_catches_it() {
        // One hex digit changed ('b' to 'c'): a well-formed manifest with a different hash. Only
        // the signature can know it is not what was signed.
        let parsed = parse(read_fixture(
            "release/bad/flipped-manifest-bit/Katana-update-win64.json",
        ))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            parsed.files.first().expect("files").sha256,
            "c70d132208d7dd410c0daba7f5fcb5a1594c844a73d499845bea1856fb6bf8da"
        );
    }

    #[test]
    fn the_path_traversal_manifest_is_refused_by_its_dot_dot_path() {
        let error = parse(read_fixture(
            "release/bad/path-traversal/Katana-update-win64.json",
        ))
        .expect_err("accepted");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.context, "bin/../../evil.dll");
        assert!(error.message.contains("files[0].path"), "{}", error.message);
    }

    #[test]
    fn the_bad_variants_of_the_signature_and_the_payload_are_manifests_like_any_other() {
        // swapped-sig, tampered-member, wrong-key and wrong-key-claims-active carry the second
        // release's manifest (the refusal is the signature's or the member hash's).
        let v2 = read_fixture("release/v2/Katana-update-win64.json");
        for name in [
            "swapped-sig",
            "tampered-member",
            "wrong-key",
            "wrong-key-claims-active",
        ] {
            let text = read_fixture(&format!("release/bad/{name}/Katana-update-win64.json"));
            assert_eq!(text, v2, "{name}");
            assert!(parse(&text).is_ok(), "{name}");
        }
    }

    #[test]
    fn the_second_release_is_refused_at_every_length_shorter_than_itself() {
        // A manifest cut off anywhere is not a manifest (a truncated download): no prefix of the
        // whole is accepted, whichever byte it stops at.
        let whole = read_fixture("release/v2/Katana-update-win64.json");
        assert!(parse(&whole).is_ok());
        for length in 0..whole.len() - 1 {
            assert!(
                parse(&whole[..length]).is_err(),
                "accepted a prefix of {length} bytes"
            );
        }
        // The trailing newline is the only byte that may go.
        assert_eq!(whole.last(), Some(&b'\n'));
        assert!(parse(&whole[..whole.len() - 1]).is_ok());
    }
}
