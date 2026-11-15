//! The signed manifest of one release and one target: what every install file must hash to, and
//! where each file's bytes are in the release's payload zip. The unit of update is the FILE,
//! decided by SHA-256, never the version.
//!
//! On the wire (UTF-8, LF, keys sorted, files sorted by path), for example:
//!
//! ```text
//! {"applier":1,"channel":"stable","expires":"2027-04-18T08:00:00Z","files":[
//!  {"class":"program","csize":15500000,"exec":true,"offset":1234,
//!   "path":"bin/katana.exe","sha256":"<64 hex>","size":42312704}, ...],
//!  "notes_url":"https://github.com/<repo>/releases/tag/v0.3.0",
//!  "payload":{"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":212345678},
//!  "product":"katana","published":"2026-10-20T08:00:00Z","schema":1,
//!  "sequence":1792312800,"tag":"v0.3.0","target":"win64","version":"0.3.0"}
//! ```
//!
//! It is signed as those exact bytes (signature.rs, trust.rs) and PARSED ONLY AFTER the signature
//! has verified: [`parse_manifest`] is for trust.rs (and for tests of the parser itself), and
//! nothing that reads a network answer may call it on bytes it has not verified.
//!
//! Members this contract adds to the design's sample (to be kept in step by the producer,
//! tools/update_make_manifest.py): `"notify_only": true`, a manifest that announces a release
//! without offering to install it (the macOS bundle cannot be patched file by file). It has
//! `"files":[]` and no `"payload"`. Absent means false. Unknown members are ignored, so a newer
//! producer may add more.
//!
//! This runs on bytes whose signature has ALREADY verified, so it is not the trust gate; it is
//! what makes a signed manifest safe to act on. A signature says who wrote a value, not that the
//! value is sane: a number that wraps when two are added, a path that climbs out of the install,
//! an offset past the end of the zip are all things a correctly signed manifest could carry by
//! mistake (a producer bug) as readily as by malice, and the rest of the engine takes the
//! Manifest this returns at its word.
//!
//! Strict where a lax reader would be exploited, lenient where a newer producer may add: exact
//! types ("5.0", "1e3", a boolean for a number, a number for a string are refused); a duplicate
//! member anywhere, or nesting deeper than 16 levels, is refused (a reader that took the last of
//! two "sha256" members would disagree with one that took the first); members this reader does
//! not know are ignored, but still checked for those two.

use crate::feed::MAX_MANIFEST_BYTES;
use crate::paths::{MAX_FILES, MAX_INSTALL_BYTES, check_manifest_path, check_manifest_paths};
use crate::version::{Version, parse_version};
use katana_core::error::make_error;
use katana_core::json::{
    DomBuilder, Json, JsonError, JsonErrorKind, ObjectMap, Sax, SortedMap, sax_parse,
};
use katana_core::{Error, ErrorCode, Result, Status};
use std::collections::HashSet;

/// How a file is treated at the swap. The helper commits data, then lib, then program (so a
/// program that cannot load is the last thing replaced); a seed is written only when the file is
/// absent, so a user's edit survives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FileClass {
    Program,
    #[default]
    Lib,
    Data,
    Seed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManifestFile {
    /// Root-relative, '/', passes paths.rs.
    pub path: String,
    pub file_class: FileClass,
    /// "exec": set the execute bit where there is one.
    pub executable: bool,
    /// Bytes once inflated.
    pub size: u64,
    /// "csize": bytes of raw deflate data in the payload.
    pub compressed_size: u64,
    /// Where that deflate data starts in the payload zip.
    pub offset: u64,
    /// 64 lower-case hex digits of the inflated bytes.
    pub sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    /// "deflate".
    pub codec: String,
    /// The asset, Katana-<ver>-<token>.update.zip.
    pub name: String,
    /// The whole zip; it is never fetched whole.
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub schema: u32,
    /// The minimum helper protocol that can apply it.
    pub applier: u32,
    /// "stable".
    pub channel: String,
    /// UTC, RFC 3339.
    pub published: String,
    /// Advisory: "the information is old", never a refusal.
    pub expires: String,
    pub notes_url: String,
    /// Absent only when notify_only.
    pub payload: Option<Payload>,
    /// "katana".
    pub product: String,
    /// UTC epoch seconds at build: a re-run outranks its first attempt.
    pub sequence: u64,
    /// "v0.3.0".
    pub tag: String,
    /// win64 win-arm64 linux-x86_64 linux-aarch64 macos-arm64.
    pub target: String,
    pub version: Version,
    pub notify_only: bool,
    /// Sorted by path, none reserved, all pass paths.rs.
    pub files: Vec<ManifestFile>,
    /// Set by verify_manifest only (parse_manifest leaves them empty): the exact bytes that were
    /// signed and the signature line that covered them. The helper writes them beside the install
    /// as installed.manifest.json(.sig), the record a later update decides removals from.
    pub signed_json: Vec<u8>,
    pub signature_line: Vec<u8>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema: 1,
            applier: 1,
            channel: String::new(),
            published: String::new(),
            expires: String::new(),
            notes_url: String::new(),
            payload: None,
            product: String::new(),
            sequence: 0,
            tag: String::new(),
            target: String::new(),
            version: Version::default(),
            notify_only: false,
            files: Vec::new(),
            signed_json: Vec::new(),
            signature_line: Vec::new(),
        }
    }
}

const SCHEMA: u32 = 1;
// The sample has 3 levels (object, files, one file). Room for a newer producer's nested members,
// and far below anything that could exhaust the stack.
const MAX_DEPTH: usize = 16;
const MAX_TOKEN_BYTES: usize = 64;
const MAX_PAYLOAD_NAME_BYTES: usize = 255;
const MAX_URL_BYTES: usize = 2048;
// The zip holds every file deflated alone, and the files add up to at most MAX_INSTALL_BYTES, so
// the zip is that and a little (max_compressed_bytes below, and its headers): twice the install
// cap is room for all of it, and no signed number can make the payload, or a range in it, larger
// than that. The offsets and lengths go to APIs with signed 64-bit offsets (HTTP range
// arithmetic, file offsets) and stay far below them.
const MAX_PAYLOAD_BYTES: u64 = 2 * MAX_INSTALL_BYTES;
// A deflate stream is never empty: the shortest, an empty final block, is the two bytes 03 00
// (RFC 1951, 3.2.3; Python's zipfile writes exactly that, measured).
const MIN_COMPRESSED_BYTES: u64 = 2;
// 2100-01-01T00:00:00Z in epoch seconds.
const MAX_SEQUENCE: u64 = 4_102_444_800;

// The most a deflate stream of `size` bytes can be. An encoder that falls back to stored blocks
// when coding does not pay stays within a few hundredths of a percent of the input (zlib-ng
// levels 2 to 9 on random bytes: 0.03%, measured). One that does not compare (zlib-ng's quick
// level 1: 5.5% over, measured) codes each byte as a fixed-Huffman literal, and the byte values
// 144 to 255 take 9 bits there (RFC 1951, 3.2.6): at worst 9/8 of the input and 3 + 7 bits of
// block header and end-of-block, which is at most size + size / 8 + 3 bytes. An eighth and 64
// bytes cover that, so the bound does not depend on the encoder or the level the producer uses
// (it pins level 9 today). `size` is at most MAX_INSTALL_BYTES: nothing overflows.
const fn max_compressed_bytes(size: u64) -> u64 {
    size + size / 8 + 64
}

fn malformed(what: impl Into<String>, member: &str) -> Error {
    make_error(ErrorCode::ParseFailure, what, member)
}

fn invalid(what: impl Into<String>, member: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, what, member)
}

fn member_name(place: &str, key: &str) -> String {
    if place.is_empty() {
        key.to_owned()
    } else {
        format!("{place}.{key}")
    }
}

// A fragment of a signed string for a message: bounded and with no byte that is not printable
// ASCII, so a NUL or a newline in a value cannot reshape a log line.
fn excerpt(text: &str) -> String {
    const SHOWN: usize = 64;
    let bytes = text.as_bytes();
    let shown = bytes.len().min(SHOWN);
    let mut out = String::with_capacity(shown + 3);
    for &byte in &bytes[..shown] {
        out.push(if (0x20..=0x7e).contains(&byte) {
            char::from(byte)
        } else {
            '?'
        });
    }
    if bytes.len() > shown {
        out.push_str("...");
    }
    out
}

// ---- the JSON, with the two refusals the reader does not make by itself -----------------------

/// Why a watched parse gave no value.
pub(crate) enum Unread {
    /// Nesting deeper than the limit: the outermost object or list is level 1.
    TooDeep,
    /// A member written twice in one object, by the name the reader ends up with (an escape read).
    Duplicate(String),
    /// The exception nlohmann threw: a parse_error (and the byte it stopped at), or the
    /// out_of_range 406 of a number past the largest double.
    Json(JsonError),
}

/// The C++'s parser callback `watch` (manifest.cpp's; feed.cpp's makes the depth check alone):
/// one level more at each object_start and array_start, refused when it passes `max_depth`, one
/// less at each end; and with `refuse_duplicates`, the names seen so far in each open object,
/// refused at the key event of one seen before. A callback that refuses throws, which ends the
/// parse; one that does not keeps everything, so every event goes on to the tree builder, as
/// nlohmann's callback parser builds the tree.
struct Watch {
    tree: DomBuilder<SortedMap>,
    max_depth: usize,
    refuse_duplicates: bool,
    depth: usize,
    keys_seen: Vec<HashSet<String>>,
    refusal: Option<Unread>,
}

impl Watch {
    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > self.max_depth {
            self.refusal = Some(Unread::TooDeep);
            return false;
        }
        true
    }
}

impl Sax for Watch {
    fn null(&mut self) -> bool {
        self.tree.null()
    }

    fn boolean(&mut self, value: bool) -> bool {
        self.tree.boolean(value)
    }

    fn number_integer(&mut self, value: i64) -> bool {
        self.tree.number_integer(value)
    }

    fn number_unsigned(&mut self, value: u64) -> bool {
        self.tree.number_unsigned(value)
    }

    fn number_float(&mut self, value: f64, written: &str) -> bool {
        self.tree.number_float(value, written)
    }

    fn string(&mut self, value: &mut String) -> bool {
        self.tree.string(value)
    }

    fn start_object(&mut self) -> bool {
        if !self.enter() {
            return false;
        }
        if self.refuse_duplicates {
            self.keys_seen.push(HashSet::new());
        }
        self.tree.start_object()
    }

    fn key(&mut self, value: &mut String) -> bool {
        if self.refuse_duplicates
            && let Some(seen) = self.keys_seen.last_mut()
            && !seen.insert(value.clone())
        {
            self.refusal = Some(Unread::Duplicate(value.clone()));
            return false;
        }
        self.tree.key(value)
    }

    fn end_object(&mut self) -> bool {
        if self.refuse_duplicates {
            self.keys_seen.pop();
        }
        self.depth -= 1;
        self.tree.end_object()
    }

    fn start_array(&mut self) -> bool {
        if !self.enter() {
            return false;
        }
        self.tree.start_array()
    }

    fn end_array(&mut self) -> bool {
        self.depth -= 1;
        self.tree.end_array()
    }

    fn parse_error(&mut self, position: usize, token: &str, error: &JsonError) -> bool {
        self.tree.parse_error(position, token, error)
    }
}

/// `Json::parse(text, watch)`: the text read as nlohmann reads it (katana_core::json), with the
/// watch's refusals.
pub(crate) fn parse_watched(
    text: &[u8],
    max_depth: usize,
    refuse_duplicates: bool,
) -> std::result::Result<Json, Unread> {
    let mut watch = Watch {
        tree: DomBuilder::new(),
        max_depth,
        refuse_duplicates,
        depth: 0,
        keys_seen: Vec::new(),
        refusal: None,
    };
    let read = sax_parse(text, &mut watch);
    if let Some(refusal) = watch.refusal {
        return Err(refusal);
    }
    if read {
        return Ok(watch.tree.into_value());
    }
    Err(Unread::Json(watch.tree.into_error().expect(
        "a parse the watch did not stop ends at the parse error the tree builder keeps",
    )))
}

fn parse_json(text: &[u8]) -> Result<Json> {
    parse_watched(text, MAX_DEPTH, true).map_err(|unread| match unread {
        Unread::TooDeep => malformed(
            format!("the JSON is nested more than {MAX_DEPTH} levels deep"),
            "",
        ),
        Unread::Duplicate(key) => {
            malformed("a member is written twice in one object", &excerpt(&key))
        }
        // Not the reader's own words: they quote the token it was in, which may be megabytes of
        // a manifest holding raw bytes, control characters and invalid UTF-8 among them. The
        // place is enough.
        Unread::Json(error) if error.kind == JsonErrorKind::ParseError => malformed(
            format!(
                "not well-formed JSON: the reader stopped at byte {}",
                error.byte
            ),
            "",
        ),
        // nlohmann's out_of_range 406, which the C++ reports by its id.
        Unread::Json(error) => malformed(
            format!("the JSON could not be read (reader error {})", error.id),
            "",
        ),
    })
}

// ---- reading members ---------------------------------------------------------------------------

// One JSON object and the name it goes by in messages ("" for the manifest itself, then "payload"
// or "files[3]"). Each reader gives ParseFailure for a missing or mistyped member, the contract's
// word for "this is not the shape of a manifest"; rules about VALUES are InvalidArgument and are
// the caller's.
struct Members<'a> {
    object: &'a SortedMap,
    place: String,
}

impl<'a> Members<'a> {
    fn new(object: &'a SortedMap, place: impl Into<String>) -> Self {
        Self {
            object,
            place: place.into(),
        }
    }

    fn name(&self, key: &str) -> String {
        member_name(&self.place, key)
    }

    // None when the member is absent.
    fn find(&self, key: &str) -> Option<&'a Json> {
        self.object.get(key)
    }

    fn string(&self, key: &str) -> Result<String> {
        let Some(value) = self.find(key) else {
            return Err(self.missing(key));
        };
        match value.as_str() {
            Some(text) => Ok(text.to_owned()),
            None => Err(malformed("the member must be a string", &self.name(key))),
        }
    }

    // An unsigned integer written as one: "5.0", "1e3" and "-0" are floats or signed to the
    // reader and are refused, as is a number past 2^64 - 1 (the reader takes it for a float).
    fn unsigned_integer(&self, key: &str) -> Result<u64> {
        let Some(value) = self.find(key) else {
            return Err(self.missing(key));
        };
        value.as_unsigned().ok_or_else(|| {
            malformed(
                "the member must be a whole number from 0 to 18446744073709551615",
                &self.name(key),
            )
        })
    }

    // Absent is false; present must be a boolean.
    fn optional_boolean(&self, key: &str) -> Result<bool> {
        let Some(value) = self.find(key) else {
            return Ok(false);
        };
        value
            .as_bool()
            .ok_or_else(|| malformed("the member must be true or false", &self.name(key)))
    }

    fn array(&self, key: &str) -> Result<&'a Vec<Json>> {
        let Some(value) = self.find(key) else {
            return Err(self.missing(key));
        };
        value
            .as_array()
            .ok_or_else(|| malformed("the member must be a list", &self.name(key)))
    }

    fn missing(&self, key: &str) -> Error {
        malformed("the member is missing", &self.name(key))
    }
}

// ---- value rules -------------------------------------------------------------------------------

fn is_alphanumeric(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_name_character(c: u8) -> bool {
    is_alphanumeric(c) || c == b'.' || c == b'_' || c == b'-'
}

// product, channel, target, tag and the payload's name end up in a URL, a file name or a message;
// letters, digits and . _ - are all they need, and nothing else is let through. The first is a
// letter or a digit: "." and ".." are dot-segments of the URL (releases/download/<tag>/<name>)
// and a leading dot is a hidden file.
fn check_name(value: &str, max_bytes: usize, member: &str) -> Status {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(invalid(
            format!("the value must be 1 to {max_bytes} characters"),
            member,
        ));
    }
    if !is_alphanumeric(bytes[0]) {
        return Err(invalid(
            "the value must start with a letter or a digit",
            member,
        ));
    }
    if !bytes.iter().all(|&c| is_name_character(c)) {
        return Err(invalid(
            "the value may hold only letters, digits and . _ -",
            member,
        ));
    }
    Ok(())
}

fn check_notes_url(value: &str, member: &str) -> Status {
    const SCHEME: &str = "https://";
    if !value.starts_with(SCHEME) || value.len() == SCHEME.len() || value.len() > MAX_URL_BYTES {
        return Err(invalid(
            "the value must be an https:// address of at most 2048 characters",
            member,
        ));
    }
    if value.bytes().any(|byte| byte <= 0x20 || byte >= 0x7f) {
        return Err(invalid(
            "the address may hold only printable ASCII and no space",
            member,
        ));
    }
    Ok(())
}

fn is_lower_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

// The days from 1970-01-01 to a civil date (H. Hinnant's days_from_civil): the proleptic
// Gregorian calendar, with March as the first month of the shifted year so that a leap day is the
// last day of it.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = (year - era * 400) as u32;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + i64::from(day_of_era) - 719_468
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

// ---- file classes ------------------------------------------------------------------------------

/// "program", "lib", "data", "seed".
pub fn to_string(file_class: FileClass) -> &'static str {
    match file_class {
        FileClass::Program => "program",
        FileClass::Lib => "lib",
        FileClass::Data => "data",
        FileClass::Seed => "seed",
    }
}

/// The class a name spells; InvalidArgument for any other text (the context is the text).
pub fn parse_file_class(text: &str) -> Result<FileClass> {
    for file_class in [
        FileClass::Program,
        FileClass::Lib,
        FileClass::Data,
        FileClass::Seed,
    ] {
        if text == to_string(file_class) {
            return Ok(file_class);
        }
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        "not a file class (program, lib, data or seed)",
        excerpt(text),
    ))
}

// ---- UTC times ---------------------------------------------------------------------------------

/// "YYYY-MM-DDTHH:MM:SSZ" (UTC, as the producer writes `published` and `expires`) as seconds
/// since 1970-01-01T00:00:00Z. None unless it is exactly that form and a real date from 1970 (so
/// 2027-02-29 and a month 13 are not times, and neither is a leap second, :60).
pub fn parse_utc_time(text: &str) -> Option<i64> {
    // YYYY-MM-DDTHH:MM:SSZ: 20 bytes, the separators in fixed places, digits everywhere else.
    const SHAPE: &[u8; 20] = b"dddd-dd-ddTdd:dd:ddZ";
    let text = text.as_bytes();
    if text.len() != SHAPE.len() {
        return None;
    }
    for (&shape, &c) in SHAPE.iter().zip(text) {
        let fits = if shape == b'd' {
            c.is_ascii_digit()
        } else {
            c == shape
        };
        if !fits {
            return None;
        }
    }
    let number = |from: usize, length: usize| {
        text[from..from + length]
            .iter()
            .fold(0u32, |value, &c| value * 10 + u32::from(c - b'0'))
    };
    let year = number(0, 4);
    let month = number(5, 2);
    let day = number(8, 2);
    let hour = number(11, 2);
    let minute = number(14, 2);
    let second = number(17, 2);

    if year < 1970 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    const DAYS_IN_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let days_in_month = DAYS_IN_MONTH[(month - 1) as usize]
        + if month == 2 && is_leap_year(year) {
            1
        } else {
            0
        };
    if !(1..=days_in_month).contains(&day) {
        return None;
    }
    Some(
        days_from_civil(i64::from(year), month, day) * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
            + i64::from(second),
    )
}

/// `expires` is advisory: when it has passed, the dialog says the information is old and the
/// update is still offered. So this is a question for the caller to word, never a refusal: false
/// while `now_epoch` is not after `expires`, and false when `expires` cannot be read
/// (parse_manifest has already refused such a time).
pub fn is_expired(manifest: &Manifest, now_epoch: i64) -> bool {
    parse_utc_time(&manifest.expires).is_some_and(|expires| now_epoch > expires)
}

// ---- the manifest ------------------------------------------------------------------------------

fn read_time(members: &Members<'_>, key: &str) -> Result<String> {
    let text = members.string(key)?;
    if parse_utc_time(&text).is_none() {
        return Err(invalid(
            "the value must be a real UTC time written YYYY-MM-DDTHH:MM:SSZ, from 1970",
            &members.name(key),
        ));
    }
    Ok(text)
}

fn read_payload(json: &Json) -> Result<Payload> {
    let Some(object) = json.as_object() else {
        return Err(malformed("the member must be an object", "payload"));
    };
    let members = Members::new(object, "payload");

    let codec = members.string("codec")?;
    // The only codec the apply code can inflate (the zip's members are raw deflate).
    if codec != "deflate" {
        return Err(invalid(
            "the codec must be \"deflate\"",
            &members.name("codec"),
        ));
    }

    let name = members.string("name")?;
    check_name(&name, MAX_PAYLOAD_NAME_BYTES, &members.name("name"))?;

    let size = members.unsigned_integer("size")?;
    if size == 0 || size > MAX_PAYLOAD_BYTES {
        return Err(invalid(
            "the size must be from 1 to 4294967296 (4 GiB)",
            &members.name("size"),
        ));
    }
    Ok(Payload { codec, name, size })
}

fn read_file(json: &Json, index: usize, payload: Option<&Payload>) -> Result<ManifestFile> {
    let place = format!("files[{index}]");
    let Some(object) = json.as_object() else {
        return Err(malformed("the entry must be an object", &place));
    };
    let members = Members::new(object, place);

    let path = members.string("path")?;
    if let Err(rule) = check_manifest_path(&path) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{}: {}", members.name("path"), rule.message),
            rule.context,
        ));
    }

    let class_name = members.string("class")?;
    let file_class = parse_file_class(&class_name)
        .map_err(|error| invalid(error.message, &members.name("class")))?;

    let executable = members.optional_boolean("exec")?;

    let size = members.unsigned_integer("size")?;
    let compressed = members.unsigned_integer("csize")?;
    let offset = members.unsigned_integer("offset")?;
    if size > MAX_INSTALL_BYTES {
        return Err(invalid(
            "a file larger than 2 GiB (2147483648 bytes)",
            &members.name("size"),
        ));
    }
    if compressed < MIN_COMPRESSED_BYTES {
        return Err(invalid(
            "a deflate stream is never shorter than two bytes",
            &members.name("csize"),
        ));
    }
    // offset + csize <= payload.size, written so that nothing is added: with the payload at most
    // 2^63 - 1, "offset <= payload and csize <= payload - offset" cannot wrap, where offset +
    // csize can (offset 2^64 - 1 and csize 3 add up to 2).
    let payload_size = payload.map_or(0, |payload| payload.size);
    if offset > payload_size || compressed > payload_size - offset {
        return Err(invalid(
            "the file's bytes are not inside the payload",
            &members.name("offset"),
        ));
    }
    if compressed > max_compressed_bytes(size) {
        return Err(invalid(
            "a deflate stream is not longer than its file by more than deflate's worst case \
             (an eighth and 64 bytes)",
            &members.name("csize"),
        ));
    }

    let sha256 = members.string("sha256")?;
    if !is_lower_hex64(&sha256) {
        return Err(invalid(
            "the hash must be 64 lower-case hexadecimal digits",
            &members.name("sha256"),
        ));
    }
    Ok(ManifestFile {
        path,
        file_class,
        executable,
        size,
        compressed_size: compressed,
        offset,
        sha256,
    })
}

fn check_file_list(files: &[ManifestFile]) -> Status {
    for (i, pair) in files.windows(2).enumerate() {
        let order = pair[0].path.as_bytes().cmp(pair[1].path.as_bytes());
        let member = format!("files[{}].path", i + 1);
        match order {
            std::cmp::Ordering::Equal => {
                return Err(invalid("the same path is listed twice", &member));
            }
            std::cmp::Ordering::Greater => {
                return Err(invalid("files are not sorted by path", &member));
            }
            std::cmp::Ordering::Less => {}
        }
    }
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let sizes: Vec<u64> = files.iter().map(|file| file.size).collect();
    check_manifest_paths(&paths, &sizes)
}

/// Parses and validates a manifest: well-formed JSON of the shape above; schema 1; every file
/// passing check_manifest_paths; hashes of the right form (64 lower-case hex); the version
/// parsing; payload present unless notify_only; files sorted and unique. And, for every file,
/// where its bytes are in the payload, so that nothing that is signed can send a request outside
/// the zip or make a range the length of the whole disk:
///
/// * csize >= 2: a deflate stream is never empty, even for an empty file (the shortest, an empty
///   final block, is the two bytes 03 00);
/// * offset + csize <= payload.size, computed so that it cannot wrap; a notify-only manifest has
///   no files, so there is nothing to place;
/// * size <= MAX_INSTALL_BYTES for each file (the sum is check_manifest_paths's);
/// * csize <= size + size/8 + 64: a deflate stream is as long as its file and at most deflate's
///   worst case more. And payload.size <= 2 * MAX_INSTALL_BYTES (4 GiB).
///
/// Every member of the sample above is required with its exact type (a whole number is not "5.0"
/// or "1e3"), except "exec" (absent is false), "notify_only" (absent is false) and "payload"
/// (absent exactly when notify-only). Members it does not know are ignored; a member written
/// twice, nesting deeper than 16 levels, a byte-order mark, a NUL byte anywhere in the text (a
/// JSON reader takes it for the end of its input) and an input over MAX_MANIFEST_BYTES are
/// refused. Beyond that: sequence from 1 to 4102444800 (2100-01-01, the producer's range: a
/// number past it, such as the time in milliseconds, would outrank every later build); product,
/// channel, target and tag are 1 to 64 letters, digits or . _ - starting with a letter or a digit
/// (that they are THIS install's is verify_manifest's); notes_url is https://; published and
/// expires are real UTC times (expiry is never an error); payload.codec is "deflate",
/// payload.name is such a name too (up to 255), and payload.size is 1 to 4294967296; a manifest
/// that installs lists at least one file and a notify-only one none; `applier` is any whole number
/// from 1 to 2^32-1 (one above what this build speaks is not refused: the apply code decides to
/// use the staged helper).
///
/// ParseFailure for malformed JSON or a missing or mistyped member; InvalidArgument for a value
/// that breaks a rule. The context names the member (files[3].csize, payload.size), except where a
/// path rule refuses a path: there the context is the path (shortened, with every byte that is not
/// printable ASCII spelled in hexadecimal) and, for a single path, the message names the member.
/// An error never holds raw input: a JSON syntax error says where the reader stopped (a byte
/// offset), not what it was reading.
pub fn parse_manifest(json: impl AsRef<[u8]>) -> Result<Manifest> {
    let json = json.as_ref();
    if json.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(malformed(
            format!("the manifest is larger than {MAX_MANIFEST_BYTES} bytes"),
            &format!("{} bytes", json.len()),
        ));
    }
    // A JSON reader would skip a byte-order mark; the contract says UTF-8 without one, and the
    // signed bytes are what is parsed.
    if json.starts_with(b"\xEF\xBB\xBF") {
        return Err(malformed("the manifest starts with a byte-order mark", ""));
    }
    // The JSON reader takes a NUL byte for the end of its input, so "{...}" + NUL + anything
    // would be read as the object alone while every other reader (the Python verifier, a C
    // string) sees something else. JSON has no raw NUL anywhere (inside a string it is written
    // \u0000), so none is legal and none is let through.
    if json.contains(&0) {
        return Err(malformed("the manifest holds a NUL byte", ""));
    }

    let parsed = parse_json(json)?;
    let Some(root) = parsed.as_object() else {
        return Err(malformed("the manifest must be a JSON object", ""));
    };
    let members = Members::new(root, "");
    let mut manifest = Manifest::default();

    // The schema first: a manifest of another schema may differ in every member below.
    let schema = members.unsigned_integer("schema")?;
    if schema != u64::from(SCHEMA) {
        return Err(invalid(
            "this program reads manifest schema 1 only",
            "schema",
        ));
    }
    manifest.schema = SCHEMA;

    let applier = members.unsigned_integer("applier")?;
    // The minimum helper protocol. A value above what this build speaks is NOT refused here: it
    // means the staged helper must be used, which is the apply code's decision.
    let Some(applier) = u32::try_from(applier).ok().filter(|&applier| applier >= 1) else {
        return Err(invalid(
            "the applier protocol must be from 1 to 4294967295",
            "applier",
        ));
    };
    manifest.applier = applier;

    // product, target and channel are checked here for their form; that they are THIS install's
    // product, target and the stable channel is verify_manifest's (trust.rs).
    for key in ["product", "channel", "target", "tag"] {
        let text = members.string(key)?;
        check_name(&text, MAX_TOKEN_BYTES, key)?;
        match key {
            "product" => manifest.product = text,
            "channel" => manifest.channel = text,
            "target" => manifest.target = text,
            _ => manifest.tag = text,
        }
    }

    let version = members.string("version")?;
    manifest.version =
        parse_version(&version).map_err(|error| invalid(error.message, "version"))?;

    let sequence = members.unsigned_integer("sequence")?;
    if sequence == 0 {
        return Err(invalid(
            "the sequence must be 1 or more (0 means none, in an install record)",
            "sequence",
        ));
    }
    // The producer's own range (tools/update_make_manifest.py, MAX_SEQUENCE). A number past it,
    // such as the time in milliseconds, would outrank every later build of the same version.
    if sequence > MAX_SEQUENCE {
        return Err(invalid(
            "the sequence is UTC seconds and at most 4102444800 (2100-01-01), not milliseconds",
            "sequence",
        ));
    }
    manifest.sequence = sequence;

    manifest.published = read_time(&members, "published")?;
    // Read for its form only; being in the past is not a refusal (is_expired).
    manifest.expires = read_time(&members, "expires")?;

    let notes_url = members.string("notes_url")?;
    check_notes_url(&notes_url, "notes_url")?;
    manifest.notes_url = notes_url;

    manifest.notify_only = members.optional_boolean("notify_only")?;

    let payload_json = members.find("payload");
    if manifest.notify_only {
        if payload_json.is_some() {
            return Err(invalid("a notify-only manifest has no payload", "payload"));
        }
    } else {
        let Some(payload_json) = payload_json else {
            return Err(members.missing("payload"));
        };
        manifest.payload = Some(read_payload(payload_json)?);
    }

    let entries = members.array("files")?;
    if manifest.notify_only && !entries.is_empty() {
        return Err(invalid("a notify-only manifest lists no files", "files"));
    }
    // An installable manifest with no files would, applied, remove everything the last version
    // installed and put nothing back.
    if !manifest.notify_only && entries.is_empty() {
        return Err(invalid(
            "a manifest that installs lists at least one file",
            "files",
        ));
    }
    if entries.len() > MAX_FILES {
        return Err(invalid(format!("more than {MAX_FILES} files"), "files"));
    }
    manifest.files.reserve(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        let file = read_file(entry, i, manifest.payload.as_ref())?;
        manifest.files.push(file);
    }
    check_file_list(&manifest.files)?;
    Ok(manifest)
}
