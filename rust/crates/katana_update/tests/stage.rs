//! stage() and preflight() (stage.rs): downloading what changed, byte for verified byte, and
//! proving the helper starts before the window closes. The twin of tests/update/test_stage.cpp.
//!
//! Where the expectations come from (CLAUDE.md, section 4). Two payloads are served:
//!
//! * the T3 fixture zip (tests/update/fixtures/release): a REAL zip made by the Python producer.
//!   The files an update from v1 to v2 must leave are the fixtures' install/v2 files, which are on
//!   disk beside it, and the offsets and sizes of the members are the numbers written in the
//!   signed v2 manifest. Python's zipfile is the oracle that those are right
//!   (tool_tests/test_make_manifest.py).
//! * a payload built here from a specification: members as RFC 1951 section 3.2.4 STORED blocks (a
//!   header byte, LEN, NLEN, the bytes), which needs no compressor, so the data the engine's
//!   inflater reads was not made by a deflater. The layout is written down as arithmetic so that
//!   the requests a correct engine makes are worked out, not observed.
//!
//! The server is a hop function behind make_fetch, so what is exercised is stage(), the planner's
//! ranges, the transport's rules and the inflater together, with no network.
//!
//! The stage folder is worked through the harness's portable file system (StdFs under a HookFs),
//! handed to the forms of stage() and preflight() that take one: the system's own
//! (make_system_fs_ops) is written for Windows only so far, and these tests are of the staging,
//! which is the same code on every platform. The forms without the argument - what the programs
//! call - are run once through the file system that ships, at the end, on Windows. Nothing is
//! ever started: the launcher is a fake, so the "helper" a test puts in an install is any bytes at
//! the helper's name.

pub mod test_key;

pub mod apply_harness;

use apply_harness::{
    FakeLauncher, HookFs, Scratch, StdFs, class_name, copy_tree, fixture_folder, injected,
    is_present, read_text, write_text,
};
use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::sha256::sha256_hex;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::apply::{APPLIER_PROTOCOL, installed_program, is_valid_transaction_id};
use katana_update::feed::{
    FeedConfig, FeedRoute, Fetch, HopFn, HttpHop, HttpHopReply, StopToken, fetch_failure_reason,
    fetch_fn, hop_fn, make_fetch,
};
use katana_update::fsops::{FileLock, FsOps};
use katana_update::install::{Install, InstallKind};
use katana_update::manifest::{FileClass, Manifest, parse_manifest};
use katana_update::plan::{Plan, plan};
use katana_update::process::{SpawnRequest, SpawnResult};
use katana_update::ranges::PARTIAL_DOWNLOADS_REFUSED;
use katana_update::stage::{
    PreparedApply, preflight_with, stage_lock_path, stage_with, staged_manifest_path,
    staged_signature_path,
};
use katana_update::state::run_path;
use katana_update::trust::verify_manifest;
use katana_update::version::parse_version;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use test_key::test_trust;

const SECRET_TOKEN: &str = "ghp_SECRET-TOKEN-0123456789";

// ---- a payload built from a specification ---------------------------------------------------

fn pattern(size: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).wrapping_add(1);
    (0..size)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8
        })
        .collect()
}

// RFC 1951, 3.2.4: BFINAL, BTYPE 00, then LEN, NLEN (little-endian, one's complement) and LEN
// bytes, in blocks of at most 65535.
fn stored_deflate(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        let n = 65535usize.min(bytes.len() - at);
        let last = at + n >= bytes.len();
        out.push(u8::from(last));
        out.push((n & 0xFF) as u8);
        out.push(((n >> 8) & 0xFF) as u8);
        out.push((!n & 0xFF) as u8);
        out.push(((!n >> 8) & 0xFF) as u8);
        out.extend_from_slice(&bytes[at..at + n]);
        at += n;
        if at >= bytes.len() {
            break;
        }
    }
    out
}

#[derive(Clone)]
struct Member {
    path: String,
    content: Vec<u8>,
    file_class: FileClass,
    // Bytes of the payload that belong to no member, before this one.
    gap_before: usize,
    // Added to the `size` the manifest signs.
    size_lie: i64,
    // When set, the signed hash is of THIS, not of the content.
    hash_of: Vec<u8>,
}

fn member(
    path: &str,
    content: impl Into<Vec<u8>>,
    file_class: FileClass,
    gap_before: usize,
) -> Member {
    Member {
        path: String::from(path),
        content: content.into(),
        file_class,
        gap_before,
        size_lie: 0,
        hash_of: Vec::new(),
    }
}

#[derive(Clone, Debug)]
struct Placed {
    path: String,
    content: Vec<u8>,
    offset: u64,
    csize: u64,
}

// Where the first member starts, as in a zip.
const PAYLOAD_HEAD: usize = 44;
const PAYLOAD_TAIL: usize = 100;
const SEQUENCE: u64 = 2000000000;

struct Built {
    payload: Vec<u8>,
    json: Vec<u8>,
    placed: Vec<Placed>,
    // parse_manifest of json, with the bytes a verified manifest carries.
    manifest: Manifest,
    signature_line: Vec<u8>,
}

impl Built {
    fn at(&self, path: &str) -> &Placed {
        self.placed
            .iter()
            .find(|one| one.path == path)
            .unwrap_or_else(|| panic!("no such member: {path}"))
    }
}

// Members must be given in path order (a manifest's files are sorted).
fn build(members: &[Member], applier: u32) -> Built {
    let mut payload = vec![0x50u8; PAYLOAD_HEAD];
    let mut placed: Vec<Placed> = Vec::new();
    let mut files = String::new();
    for one in members {
        payload.extend_from_slice(&pattern(one.gap_before, 7));
        let deflated = stored_deflate(&one.content);
        let here = Placed {
            path: one.path.clone(),
            content: one.content.clone(),
            offset: payload.len() as u64,
            csize: deflated.len() as u64,
        };
        payload.extend_from_slice(&deflated);
        if !files.is_empty() {
            files.push_str(",\n");
        }
        let hashed: &[u8] = if one.hash_of.is_empty() {
            &one.content
        } else {
            &one.hash_of
        };
        files.push_str(&format!(
            "{{\"class\":\"{}\",\"csize\":{},\"offset\":{},\"path\":\"{}\",\"sha256\":\"{}\",\
             \"size\":{}}}",
            class_name(one.file_class),
            here.csize,
            here.offset,
            one.path,
            sha256_hex(hashed),
            one.content.len() as i64 + one.size_lie
        ));
        placed.push(here);
    }
    payload.extend_from_slice(&[0x51u8; PAYLOAD_TAIL]);
    let json = format!(
        "{{\"applier\":{applier},\"channel\":\"stable\",\"expires\":\"2030-01-01T00:00:00Z\",\
         \"files\":[\n{files}\n],\"notes_url\":\"https://github.com/kjarada/Katana/releases/tag/\
         v1.0.0\",\"payload\":{{\"codec\":\"deflate\",\"name\":\"Katana-1.0.0-win64.update.zip\",\
         \"size\":{}}},\"product\":\"katana\",\"published\":\"2026-10-01T00:00:00Z\",\"schema\":1,\
         \"sequence\":{SEQUENCE},\"tag\":\"v1.0.0\",\"target\":\"win64\",\"version\":\"1.0.0\"}}\n",
        payload.len()
    )
    .into_bytes();
    let mut manifest = parse_manifest(&json)
        .unwrap_or_else(|error| panic!("the built manifest does not parse: {}", error.describe()));
    // stage() does not verify (verify_manifest did, upstream); it needs the bytes that were signed
    // and the line that signed them, to write beside the files. Any line will do here.
    let signature_line = format!("ed25519 d75a980182b10ab7 {}==\n", "A".repeat(86)).into_bytes();
    manifest.signed_json = json.clone();
    manifest.signature_line = signature_line.clone();
    Built {
        payload,
        json,
        placed,
        manifest,
        signature_line,
    }
}

// ---- the layout the multi-range tests use ----------------------------------------------------------
//
//   44          head
//   44          bin/a.dll      3000 bytes, one stored block:                 csize 3000 + 5 = 3005
//   3049        500 bytes of gap
//   3549        bin/b.dll      40 bytes:                                      csize 45
//   3594        2 MiB of gap (2097152, more than the 1 MiB at which ranges merge)
//   2100746     share/big.bin  3 MiB + 17 = 3145745 bytes: 48 blocks of 65535 (3145680) and a
//               block of 65, so 49 blocks:                                  csize 3145745 + 245 = 3145990
//   5246736     1.5 MiB of gap (1572864)
//   6819600     share/c.txt    10 bytes:                                      csize 15
//   6819615     tail 100
//
// So three requests: [44, 3594) length 3550 (a.dll and b.dll, 500 apart, are one range),
// [2100746, 5246736) length 3145990, and [6819600, 6819615) length 15.

fn three_ranges() -> Vec<Member> {
    vec![
        member("bin/a.dll", pattern(3000, 1), FileClass::Lib, 0),
        member("bin/b.dll", pattern(40, 2), FileClass::Lib, 500),
        member(
            "share/big.bin",
            pattern(3 * 1024 * 1024 + 17, 3),
            FileClass::Data,
            2 * 1024 * 1024,
        ),
        member("share/c.txt", pattern(10, 4), FileClass::Data, 1536 * 1024),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    start: u64,
    length: u64,
}

// ---- the server --------------------------------------------------------------------------------

fn range_of(hop: &HttpHop) -> Option<Range> {
    for header in &hop.headers {
        if let Some(spec) = header.strip_prefix("Range: bytes=") {
            let dash = spec.find('-')?;
            let first: u64 = spec[..dash].parse().ok()?;
            let last: u64 = spec[dash + 1..].parse().ok()?;
            return Some(Range {
                start: first,
                length: last - first + 1,
            });
        }
    }
    None
}

type BeforeHook<'a> = Box<dyn Fn(usize) + 'a>;
type MutateHook<'a> = Box<dyn Fn(&mut Vec<u8>) + 'a>;
type CustomHook<'a> = Box<dyn Fn(usize, &HttpHop) -> Option<Result<HttpHopReply>> + 'a>;

// Serves a payload as a release host does: 206 and a Content-Range for a range, 200 for none.
struct PayloadServer<'a> {
    payload: Vec<u8>,
    // Called with the index of the hop (from 0) before it is answered.
    before: Option<BeforeHook<'a>>,
    // Changes the body the server sends back for a range.
    mutate: Option<MutateHook<'a>>,
    // Answers instead, for a hop it wants to break.
    custom: Option<CustomHook<'a>>,
    ignore_range: bool,
    // When set, every hop must be to this address.
    url: String,
    hops: RefCell<Vec<HttpHop>>,
    ranges: RefCell<Vec<Range>>,
}

impl<'a> PayloadServer<'a> {
    fn new(payload: Vec<u8>) -> Self {
        Self {
            payload,
            before: None,
            mutate: None,
            custom: None,
            ignore_range: false,
            url: String::new(),
            hops: RefCell::new(Vec::new()),
            ranges: RefCell::new(Vec::new()),
        }
    }

    fn hop(&self) -> HopFn<'_> {
        hop_fn(move |one, _stop, progress| {
            let index = self.hops.borrow().len();
            self.hops.borrow_mut().push(one.clone());
            let named = range_of(one);
            if let Some(range) = named {
                self.ranges.borrow_mut().push(range);
            }
            if let Some(before) = &self.before {
                before(index);
            }
            if !self.url.is_empty() {
                assert_eq!(one.url, self.url);
            }
            if let Some(custom) = &self.custom
                && let Some(answer) = custom(index, one)
            {
                return answer;
            }
            let mut reply = HttpHopReply::default();
            match named {
                Some(range) if !self.ignore_range => {
                    let last = (range.start + range.length).min(self.payload.len() as u64);
                    reply.status = 206;
                    reply.body = self.payload[range.start as usize..last as usize].to_vec();
                    reply.content_range =
                        format!("bytes {}-{}/{}", range.start, last - 1, self.payload.len());
                }
                _ => {
                    reply.status = 200;
                    reply.body = self.payload.clone();
                }
            }
            reply.content_type = String::from("application/zip");
            if let Some(mutate) = &self.mutate {
                mutate(&mut reply.body);
            }
            // The real hop abandons a transfer at max_bytes, keeps none of it, and says what the
            // answer was (HttpHopReply::overran).
            if reply.body.len() as u64 > one.max_bytes {
                reply.body.clear();
                reply.overran = true;
                return Ok(reply);
            }
            if let Some(progress) = progress {
                let length = reply.body.len() as u64;
                progress(length / 2, length);
                progress(length, length);
            }
            Ok(reply)
        })
    }

    fn hops(&self) -> Vec<HttpHop> {
        self.hops.borrow().clone()
    }

    fn ranges(&self) -> Vec<Range> {
        self.ranges.borrow().clone()
    }
}

// ---- the rig -----------------------------------------------------------------------------------

struct Rig {
    // Kept for its folder, which goes when the rig does.
    _scratch: Scratch,
    root: PathBuf,
    state_dir: PathBuf,
    stage_dir: PathBuf,
    install: Install,
    feed: FeedConfig,
    manifest: Manifest,
    the_plan: Plan,
    progress: RefCell<Vec<(u64, u64)>>,
    // The disk, and over it the hooks a test breaks a write or names the free space with.
    disk: StdFs,
    hooked: HookFs<'static>,
    // The fixture release's payload (fixture_rig).
    v2_zip: Vec<u8>,
    // The installed helper (preflight_rig).
    helper_path: PathBuf,
}

impl Rig {
    fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let root = scratch.join("R");
        let state_dir = scratch.join("U");
        let stage_dir = state_dir.join(format!("stage-{SEQUENCE}"));
        std::fs::create_dir_all(&root).expect("the install folder");
        let install = Install {
            root: root.clone(),
            target: String::from("win64"),
            kind: InstallKind::Archive,
            version: parse_version("0.9.0").expect("a version"),
            sequence: 1790000000,
            has_installed_record: true,
            ..Install::default()
        };
        let feed = FeedConfig {
            repo: String::from("kjarada/Katana"),
            target: String::from("win64"),
            user_agent: String::from("Katana/0.9.0 (win64)"),
            ..FeedConfig::default()
        };
        Self {
            _scratch: scratch,
            root,
            state_dir,
            stage_dir,
            install,
            feed,
            manifest: Manifest::default(),
            the_plan: Plan::default(),
            progress: RefCell::new(Vec::new()),
            disk: StdFs,
            hooked: HookFs::new(&StdFs),
            v2_zip: Vec::new(),
            helper_path: PathBuf::new(),
        }
    }

    fn use_manifest(&mut self, next: Manifest) {
        self.manifest = next;
        self.the_plan = plan(&self.manifest, &self.install, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
    }

    fn use_built(&mut self, built: &Built) {
        self.use_manifest(built.manifest.clone());
    }

    fn run(&self, fetch: &Fetch<'_>) -> Status {
        self.run_stopped(fetch, None)
    }

    fn run_stopped(&self, fetch: &Fetch<'_>, stop: StopToken<'_>) -> Status {
        self.progress.borrow_mut().clear();
        let record: &dyn Fn(u64, u64) =
            &|done: u64, total: u64| self.progress.borrow_mut().push((done, total));
        stage_with(
            fetch,
            &self.feed,
            &self.manifest,
            &self.the_plan,
            &self.stage_dir,
            stop,
            Some(record),
            &self.hooked,
        )
    }

    fn progress(&self) -> Vec<(u64, u64)> {
        self.progress.borrow().clone()
    }

    fn v2_file(&self, path: &str) -> Vec<u8> {
        read_text(&fixture_folder().join("install").join("v2").join(path))
    }

    // The folders run-<tx> in the state folder (preflight_rig).
    fn run_folders(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.state_dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| {
                String::from_utf8_lossy(&path_to_utf8(Path::new(&entry.file_name()))).into_owned()
            })
            .filter(|name| name.starts_with("run-"))
            .collect()
    }
}

// Every regular file under `folder`, as '/'-separated paths relative to it.
fn files_under(folder: &Path) -> BTreeSet<String> {
    fn walk(folder: &Path, prefix: &str, found: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name =
                String::from_utf8_lossy(&path_to_utf8(Path::new(&entry.file_name()))).into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                walk(&path, &relative, found);
            } else if std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
                found.insert(relative);
            }
        }
    }
    let mut found = BTreeSet::new();
    walk(folder, "", &mut found);
    found
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|&name| String::from(name)).collect()
}

fn reason_of(status: &Status) -> String {
    match status {
        Ok(()) => String::from("ok"),
        Err(error) => fetch_failure_reason(error),
    }
}

fn describe<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => String::from("ok"),
        Err(error) => error.describe(),
    }
}

fn error_of<T>(result: Result<T>) -> Error {
    match result {
        Ok(_) => panic!("refused, it should have been"),
        Err(error) => error,
    }
}

// ---- the fixture release (T3's real zip) -----------------------------------------------------

fn fixture_rig(label: &str, zip_folder: &str, with_random_bin: bool) -> Rig {
    let mut rig = Rig::new(label);
    rig.v2_zip = read_text(
        &fixture_folder()
            .join("release")
            .join(zip_folder)
            .join("Katana-0.10.0-win64.update.zip"),
    );
    copy_tree(&fixture_folder().join("install").join("v1"), &rig.root);
    if !with_random_bin {
        let _ = std::fs::remove_file(rig.root.join("share/katana/random.bin"));
    }
    let json = read_text(&fixture_folder().join("release/v2/Katana-update-win64.json"));
    let sig = read_text(&fixture_folder().join("release/v2/Katana-update-win64.json.sig"));
    let verified = verify_manifest(json, sig, test_trust(), &rig.install)
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    rig.use_manifest(verified);
    rig
}

const V2_PAYLOAD: &str =
    "https://github.com/kjarada/Katana/releases/download/v0.10.0/Katana-0.10.0-win64.update.zip";

// A transport that answers every request from the hop given.
fn fetch_of(hop: HopFn<'_>) -> Fetch<'_> {
    make_fetch(hop)
}

// ---- preflight -----------------------------------------------------------------------------------

// R with an installed helper, and a state folder.
fn preflight_rig(label: &str) -> Rig {
    let mut rig = Rig::new(label);
    rig.helper_path = installed_program(&rig.root, "katana_updater");
    write_text(&rig.helper_path, "INSTALLED HELPER, protocol 1");
    write_text(
        &rig.root.join("share/katana/install.json"),
        "{\"schema\":1,\"target\":\"win64\"}",
    );
    std::fs::create_dir_all(&rig.state_dir).expect("the state folder");
    rig
}

fn helper_name(root: &Path) -> String {
    let helper = installed_program(root, "katana_updater");
    String::from_utf8_lossy(&path_to_utf8(Path::new(
        helper.file_name().unwrap_or_default(),
    )))
    .into_owned()
}

fn manifest_with_helper(applier: u32, helper_content: &str, root: &Path) -> Manifest {
    let built = build(
        &[
            member(
                &format!("bin/{}", helper_name(root)),
                helper_content,
                FileClass::Program,
                0,
            ),
            member("share/other.txt", "o", FileClass::Data, 0),
        ],
        applier,
    );
    built.manifest
}

fn prepare(rig: &Rig, manifest: &Manifest, launcher: &FakeLauncher<'_>) -> Result<PreparedApply> {
    preflight_with(
        manifest,
        &rig.install,
        &rig.stage_dir,
        &rig.state_dir,
        launcher,
        &rig.hooked,
    )
}

mod stage {
    use super::*;

    // ---- the happy path, on the real zip ---------------------------------------------------

    #[test]
    fn an_update_from_v1_to_v2_fetches_only_the_changed_files_in_one_range_and_stages_them_verified()
     {
        let rig = fixture_rig("stage_v1_v2", "v2", true);
        let mut server = PayloadServer::new(rig.v2_zip.clone());
        server.url = String::from(V2_PAYLOAD);
        let status = rig.run(&fetch_of(server.hop()));
        assert!(status.is_ok(), "{}", describe(&status));

        // Changed between v1 and v2 (tools/update_make_fixtures.py): bin/katana.exe,
        // share/katana/readme.txt; arriving: bin/libnew-2.dll, share/katana/news.txt,
        // share/katana/help/index.txt. etc/katana/settings.ini changed too, but it is a seed and v1
        // has it: it is never read, and so never replaced. katana_cli.exe, libcore-1.dll,
        // empty.txt and random.bin are the same bytes.
        assert_eq!(rig.the_plan.changed_files, 5);
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&[
                "bin/katana.exe",
                "bin/libnew-2.dll",
                "share/katana/help/index.txt",
                "share/katana/news.txt",
                "share/katana/readme.txt",
                "manifest.json",
                "manifest.json.sig"
            ])
        );
        for path in [
            "bin/katana.exe",
            "bin/libnew-2.dll",
            "share/katana/help/index.txt",
            "share/katana/news.txt",
            "share/katana/readme.txt",
        ] {
            assert_eq!(
                read_text(&rig.stage_dir.join(path)),
                rig.v2_file(path),
                "{path}"
            );
        }

        // The manifest of v2 lists these members of the payload (offset, csize): katana.exe (44,
        // 85), libnew-2.dll (418, 70), help/index.txt (599, 75), news.txt (725, 54), readme.txt
        // (2938, 55). The gaps are far below 1 MiB, so they are ONE request from 44 to 2938 + 55 =
        // 2993.
        let ranges = server.ranges();
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            ranges[0],
            Range {
                start: 44,
                length: 2993 - 44
            }
        );
        let hops = server.hops();
        assert_eq!(hops.len(), 1);
        assert_eq!(hops[0].max_bytes, 2949, "max_bytes is the range length");

        // The staged record is the signed manifest, byte for byte.
        assert_eq!(
            read_text(&rig.stage_dir.join("manifest.json")),
            read_text(&fixture_folder().join("release/v2/Katana-update-win64.json"))
        );
        assert_eq!(
            read_text(&rig.stage_dir.join("manifest.json.sig")),
            read_text(&fixture_folder().join("release/v2/Katana-update-win64.json.sig"))
        );
    }

    #[test]
    fn progress_counts_the_compressed_bytes_to_fetch_from_zero_to_the_total() {
        let rig = fixture_rig("stage_progress", "v2", true);
        let server = PayloadServer::new(rig.v2_zip.clone());
        assert!(rig.run(&fetch_of(server.hop())).is_ok());
        let progress = rig.progress();
        assert!(progress.len() >= 3);
        assert_eq!(progress[0], (0, 2949));
        assert_eq!(progress[progress.len() - 1], (2949, 2949));
        let mut last = 0;
        for &(done, total) in &progress {
            assert_eq!(total, 2949);
            assert!(done >= last, "progress never goes back");
            assert!(done <= total);
            last = done;
        }
    }

    #[test]
    fn a_repeated_stage_fetches_nothing_and_writes_the_same_record() {
        let rig = fixture_rig("stage_repeat", "v2", true);
        let first = PayloadServer::new(rig.v2_zip.clone());
        assert!(rig.run(&fetch_of(first.hop())).is_ok());
        let second = PayloadServer::new(rig.v2_zip.clone());
        assert!(rig.run(&fetch_of(second.hop())).is_ok());
        assert!(
            second.hops().is_empty(),
            "every file is staged and verified: nothing to ask for"
        );
        let progress = rig.progress();
        let (done, total) = progress[progress.len() - 1];
        assert_eq!(done, total);
        assert!(is_present(&rig.stage_dir.join("manifest.json")));
        assert!(is_present(&rig.stage_dir.join("manifest.json.sig")));
        assert_eq!(files_under(&rig.stage_dir).len(), 7);
    }

    // ---- tampering and damage ---------------------------------------------------------------

    #[test]
    fn a_tampered_member_is_refused_and_nothing_unverified_is_kept() {
        // T3's bad/tampered-member: v2's zip with one byte of share/katana/random.bin changed,
        // and v2's genuine manifest. The install here lacks random.bin, so it is among the
        // changes.
        let rig = fixture_rig("stage_tampered", "bad/tampered-member", false);
        let server = PayloadServer::new(rig.v2_zip.clone());
        let status = rig.run(&fetch_of(server.hop()));
        assert_eq!(reason_of(&status), "staged_hash");
        let error = error_of(status);
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.context.contains("share/katana/random.bin"));

        assert!(!is_present(&rig.stage_dir.join("share/katana/random.bin")));
        assert!(
            !is_present(&rig.stage_dir.join("manifest.json")),
            "an unfinished stage is not complete"
        );
        assert!(!is_present(&rig.stage_dir.join("manifest.json.sig")));
        // What was verified before the bad member (in the payload's order) stays, and is right.
        for path in files_under(&rig.stage_dir) {
            assert_eq!(
                read_text(&rig.stage_dir.join(&path)),
                rig.v2_file(&path),
                "{path}"
            );
        }
        assert!(!is_present(&rig.stage_dir.join(".partial").join("x")));
        assert!(
            files_under(&rig.stage_dir.join(".partial")).is_empty(),
            "no half file is left"
        );

        // The same server, repaired, is resumed from where it stopped: only what is missing is
        // asked.
        let repaired = PayloadServer::new(read_text(
            &fixture_folder().join("release/v2/Katana-0.10.0-win64.update.zip"),
        ));
        assert!(rig.run(&fetch_of(repaired.hop())).is_ok());
        let ranges = repaired.ranges();
        assert_eq!(ranges.len(), 1);
        // random.bin (832, 2053) and readme.txt (2938, 55) were not staged: [832, 2993).
        assert_eq!(
            ranges[0],
            Range {
                start: 832,
                length: 2993 - 832
            }
        );
        assert_eq!(
            read_text(&rig.stage_dir.join("share/katana/random.bin")),
            rig.v2_file("share/katana/random.bin")
        );
    }

    #[test]
    fn a_member_that_is_not_the_signed_file_is_refused_whatever_is_wrong_with_it() {
        let x = |size_lie: i64, hash_of: &str| Member {
            size_lie,
            hash_of: hash_of.as_bytes().to_vec(),
            ..member("bin/x.dll", pattern(100, 9), FileClass::Lib, 0)
        };
        let cases: [(&str, Member, &str); 3] = [
            (
                "a hash that is not the content's",
                x(0, "another file"),
                "staged_hash",
            ),
            (
                "a signed size one byte more than the data",
                x(1, ""),
                "staged_corrupt",
            ),
            (
                "a signed size one byte less than the data",
                x(-1, ""),
                "staged_corrupt",
            ),
        ];
        for (what, bad, reason) in cases {
            let mut rig = Rig::new("stage_bad_member");
            let built = build(
                &[member("bin/a.dll", pattern(50, 8), FileClass::Lib, 0), bad],
                1,
            );
            rig.use_built(&built);
            let server = PayloadServer::new(built.payload.clone());
            let status = rig.run(&fetch_of(server.hop()));
            assert_eq!(reason_of(&status), reason, "{what}");
            assert_eq!(error_of(status).code, ErrorCode::InvalidArgument, "{what}");
            assert!(!is_present(&rig.stage_dir.join("bin/x.dll")), "{what}");
            assert!(!is_present(&rig.stage_dir.join("manifest.json")), "{what}");
            assert_eq!(
                read_text(&rig.stage_dir.join("bin/a.dll")),
                built.at("bin/a.dll").content,
                "{what}: the verified file before it stays"
            );
            assert!(
                files_under(&rig.stage_dir.join(".partial")).is_empty(),
                "{what}"
            );
        }
    }

    #[test]
    fn damaged_deflate_data_is_refused_as_corrupt() {
        let mut rig = Rig::new("stage_corrupt");
        let built = build(
            &[member("bin/a.dll", pattern(500, 5), FileClass::Lib, 0)],
            1,
        );
        rig.use_built(&built);
        let mut server = PayloadServer::new(built.payload.clone());
        // The stored block's NLEN must be the complement of LEN: flip a bit of it.
        server.mutate = Some(Box::new(|body: &mut Vec<u8>| body[3] ^= 0x01));
        let status = rig.run(&fetch_of(server.hop()));
        assert_eq!(reason_of(&status), "staged_corrupt");
        assert!(files_under(&rig.stage_dir).is_empty());
    }

    // ---- the network's answers -------------------------------------------------------------

    #[test]
    fn a_server_that_ignores_range_is_refused_before_it_floods_the_disk() {
        let rig = fixture_rig("stage_ignore_range", "v2", true);
        let mut server = PayloadServer::new(rig.v2_zip.clone());
        server.ignore_range = true; // answers every range with the whole 3791-byte zip and a 200
        let status = rig.run(&fetch_of(server.hop()));
        assert_eq!(reason_of(&status), "partial_downloads");
        let error = error_of(status);
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(error.message, PARTIAL_DOWNLOADS_REFUSED);
        let hops = server.hops();
        assert_eq!(hops.len(), 1, "and no second try");
        assert_eq!(
            hops[0].max_bytes, 2949,
            "the cut-off is the range, not the payload"
        );
        assert!(!is_present(&rig.stage_dir.join("manifest.json")));
        assert!(files_under(&rig.stage_dir).is_empty());
    }

    #[test]
    fn the_servers_refusals_are_reported_with_their_status() {
        let cases: [(i32, ErrorCode, &str); 5] = [
            (403, ErrorCode::FileImportFailure, "http_403"),
            (404, ErrorCode::NotFound, "http_404"),
            (429, ErrorCode::FileImportFailure, "http_429"),
            (416, ErrorCode::FileImportFailure, "http_416"),
            (503, ErrorCode::FileImportFailure, "http_503"),
        ];
        for (code, expected, reason) in cases {
            let rig = fixture_rig("stage_status", "v2", true);
            let mut server = PayloadServer::new(rig.v2_zip.clone());
            server.custom = Some(Box::new(move |_index: usize, _hop: &HttpHop| {
                Some(Ok(HttpHopReply {
                    status: code,
                    body: b"<html>no</html>".to_vec(),
                    ..HttpHopReply::default()
                }))
            }));
            let status = rig.run(&fetch_of(server.hop()));
            assert_eq!(reason_of(&status), reason);
            assert_eq!(error_of(status).code, expected, "{code}");
            assert!(files_under(&rig.stage_dir).is_empty(), "{code}");
        }
    }

    #[test]
    fn a_web_page_in_place_of_the_range_is_a_portal() {
        let rig = fixture_rig("stage_portal", "v2", true);
        let mut server = PayloadServer::new(rig.v2_zip.clone());
        server.custom = Some(Box::new(|_index: usize, _hop: &HttpHop| {
            Some(Ok(HttpHopReply {
                status: 200,
                content_type: String::from("text/html"),
                body: b"<html>Please sign in</html>".to_vec(),
                ..HttpHopReply::default()
            }))
        }));
        let status = rig.run(&fetch_of(server.hop()));
        assert!(status.is_err());
        assert_eq!(reason_of(&status), "html_page");
    }

    #[test]
    fn a_truncated_or_misplaced_range_is_refused_and_stages_nothing() {
        {
            let rig = fixture_rig("stage_truncated", "v2", true);
            let mut server = PayloadServer::new(rig.v2_zip.clone());
            // Content-Range still says all of it.
            server.mutate = Some(Box::new(|body: &mut Vec<u8>| {
                body.pop();
            }));
            let status = rig.run(&fetch_of(server.hop()));
            assert_eq!(
                reason_of(&status),
                "bad_range",
                "a 206 that is not the range, not a status"
            );
            assert_eq!(error_of(status).code, ErrorCode::FileImportFailure);
            assert!(files_under(&rig.stage_dir).is_empty());
        }
        {
            // The server's file is another length than the release names: it may have been
            // replaced.
            let rig = fixture_rig("stage_total", "v2", true);
            let mut longer = rig.v2_zip.clone();
            longer.extend_from_slice(b"extra");
            let server = PayloadServer::new(longer);
            let status = rig.run(&fetch_of(server.hop()));
            assert_eq!(reason_of(&status), "bad_range");
            assert!(files_under(&rig.stage_dir).is_empty());
        }
        {
            // Another part of the file than the one asked for.
            let rig = fixture_rig("stage_shifted", "v2", true);
            let zip = rig.v2_zip.clone();
            let mut server = PayloadServer::new(rig.v2_zip.clone());
            server.custom = Some(Box::new(move |_index: usize, hop: &HttpHop| {
                let range = range_of(hop).unwrap_or(Range {
                    start: 0,
                    length: 0,
                });
                let start = range.start as usize + 1;
                Some(Ok(HttpHopReply {
                    status: 206,
                    body: zip[start..start + range.length as usize].to_vec(),
                    content_range: format!(
                        "bytes {}-{}/{}",
                        range.start + 1,
                        range.start + range.length,
                        zip.len()
                    ),
                    ..HttpHopReply::default()
                }))
            }));
            assert_eq!(reason_of(&rig.run(&fetch_of(server.hop()))), "bad_range");
            assert!(files_under(&rig.stage_dir).is_empty());
        }
    }

    #[test]
    fn a_transport_that_hands_over_a_body_of_the_wrong_length_is_refused_by_the_stage_itself() {
        // make_fetch holds a range to its length; a Fetch of another making (a test's, a later
        // transport's) may not, and the slices cut from the body are only right if it is that
        // long.
        let rig = fixture_rig("stage_short_range", "v2", true);
        for length in [0usize, 2948, 2950] {
            let asked = Cell::new(0);
            let careless = fetch_fn(|_request, _stop, _progress| {
                asked.set(asked.get() + 1);
                Ok(vec![b'x'; length]) // the range is 2949 bytes
            });
            let status = rig.run(&careless);
            assert_eq!(reason_of(&status), "short_range", "{length}");
            assert_eq!(
                error_of(status).code,
                ErrorCode::FileImportFailure,
                "{length}"
            );
            assert_eq!(asked.get(), 1, "{length}");
            assert!(files_under(&rig.stage_dir).is_empty(), "{length}");
        }
    }

    // ---- several ranges, cancellation and resume ------------------------------------------

    #[test]
    fn members_far_apart_are_separate_ranges_and_near_ones_are_merged() {
        let mut rig = Rig::new("stage_ranges");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let server = PayloadServer::new(built.payload.clone());
        let status = rig.run(&fetch_of(server.hop()));
        assert!(status.is_ok(), "{}", describe(&status));

        // The arithmetic of the layout (see above three_ranges): worked out here from the placed
        // members, not from what merge_ranges returned.
        let a = built.at("bin/a.dll");
        let b = built.at("bin/b.dll");
        let big = built.at("share/big.bin");
        let c = built.at("share/c.txt");
        assert_eq!(a.offset, 44);
        assert_eq!(b.offset + b.csize, 3594);
        assert_eq!(big.offset, 2100746);
        assert_eq!(big.csize, 3145990);
        assert_eq!(c.offset, 6819600);
        assert_eq!(c.csize, 15);
        let ranges = server.ranges();
        assert_eq!(ranges.len(), 3);
        assert_eq!(
            ranges[0],
            Range {
                start: 44,
                length: 3550
            }
        );
        assert_eq!(
            ranges[1],
            Range {
                start: 2100746,
                length: 3145990
            }
        );
        assert_eq!(
            ranges[2],
            Range {
                start: 6819600,
                length: 15
            }
        );
        for placed in &built.placed {
            assert_eq!(
                read_text(&rig.stage_dir.join(&placed.path)),
                placed.content,
                "{}",
                placed.path
            );
        }
        // Progress: from 0 to the sum of the three requests.
        let progress = rig.progress();
        assert_eq!(progress[0].0, 0);
        assert_eq!(progress[progress.len() - 1], (3149555, 3149555));
    }

    #[test]
    fn a_cancelled_stage_leaves_only_verified_files_and_the_next_one_resumes() {
        let mut rig = Rig::new("stage_cancel");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);

        // Cancelled while the second request is being answered (the stop arrives before its
        // members are written).
        let source = AtomicBool::new(false);
        let mut server = PayloadServer::new(built.payload.clone());
        server.before = Some(Box::new(|index: usize| {
            if index == 1 {
                source.store(true, Ordering::Relaxed);
            }
        }));
        let status = rig.run_stopped(&fetch_of(server.hop()), Some(&source));
        assert_eq!(reason_of(&status), "cancelled");
        assert_eq!(error_of(status).code, ErrorCode::InvalidState);

        // The first request's members are complete and right; nothing of the second is under a
        // final name, no half file, no staged record.
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&["bin/a.dll", "bin/b.dll"])
        );
        assert_eq!(
            read_text(&rig.stage_dir.join("bin/a.dll")),
            built.at("bin/a.dll").content
        );
        assert_eq!(
            read_text(&rig.stage_dir.join("bin/b.dll")),
            built.at("bin/b.dll").content
        );
        assert!(!is_present(&rig.stage_dir.join(".partial")));

        // Resumed: the two ranges still missing are the only requests made.
        let second = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(second.hop())).is_ok());
        let ranges = second.ranges();
        assert_eq!(ranges.len(), 2);
        assert_eq!(
            ranges[0],
            Range {
                start: 2100746,
                length: 3145990
            }
        );
        assert_eq!(
            ranges[1],
            Range {
                start: 6819600,
                length: 15
            }
        );
        // and the progress starts where the first left off: the 3005 + 45 bytes of a.dll and
        // b.dll, which are staged, and ends at those plus the two requests (the 500-byte gap is
        // not asked for again).
        assert_eq!(rig.progress()[0], (3050, 3149055));
        assert_eq!(files_under(&rig.stage_dir).len(), 6);
        for placed in &built.placed {
            assert_eq!(
                read_text(&rig.stage_dir.join(&placed.path)),
                placed.content,
                "{}",
                placed.path
            );
        }
    }

    #[test]
    fn a_transfer_that_is_cancelled_inside_the_fetch_leaves_no_half_file() {
        let mut rig = Rig::new("stage_cancel_inside");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let mut server = PayloadServer::new(built.payload.clone());
        server.custom = Some(Box::new(|index: usize, _hop: &HttpHop| {
            if index == 2 {
                return Some(Err(Error::new(ErrorCode::InvalidState, "cancelled")));
            }
            None
        }));
        let status = rig.run(&fetch_of(server.hop()));
        assert_eq!(reason_of(&status), "cancelled");
        // Two ranges were staged, the third was cancelled: every name under the folder is a good
        // file.
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&["bin/a.dll", "bin/b.dll", "share/big.bin"])
        );
        for path in files_under(&rig.stage_dir) {
            for placed in &built.placed {
                if placed.path == path {
                    assert_eq!(
                        read_text(&rig.stage_dir.join(&path)),
                        placed.content,
                        "{path}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_stop_requested_before_the_start_does_nothing() {
        let mut rig = Rig::new("stage_stopped");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let server = PayloadServer::new(built.payload.clone());
        let source = AtomicBool::new(true);
        let status = rig.run_stopped(&fetch_of(server.hop()), Some(&source));
        assert_eq!(reason_of(&status), "cancelled");
        assert!(server.hops().is_empty());
        assert!(!is_present(&rig.stage_dir), "not even the folder");
    }

    #[test]
    fn a_stop_requested_between_two_requests_makes_no_further_request() {
        let mut rig = Rig::new("stage_stop_between");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let server = PayloadServer::new(built.payload.clone());
        let source = AtomicBool::new(false);
        // The progress calls are: the start (1), the two the server makes while it answers the
        // first request (2, 3), and the report after that request's files are staged (4). The
        // stop arrives on the fourth: after the first request, before the second.
        let calls = Cell::new(0);
        let progress: &dyn Fn(u64, u64) = &|_done: u64, _total: u64| {
            calls.set(calls.get() + 1);
            if calls.get() == 4 {
                source.store(true, Ordering::Relaxed);
            }
        };
        let status = stage_with(
            &fetch_of(server.hop()),
            &rig.feed,
            &rig.manifest,
            &rig.the_plan,
            &rig.stage_dir,
            Some(&source),
            Some(progress),
            &rig.hooked,
        );
        assert_eq!(reason_of(&status), "cancelled");
        assert_eq!(
            server.hops().len(),
            1,
            "no request is made once the stop is seen"
        );
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&["bin/a.dll", "bin/b.dll"])
        );
    }

    #[test]
    fn a_file_that_changed_on_disk_since_it_was_staged_is_fetched_again() {
        let mut rig = Rig::new("stage_rot");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let first = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(first.hop())).is_ok());

        // The same size, one byte different (a bad sector, an editor); and a file of another
        // size.
        let mut rotten = built.at("bin/b.dll").content.clone();
        rotten[5] ^= 0x40;
        write_text(&rig.stage_dir.join("bin/b.dll"), &rotten);
        write_text(&rig.stage_dir.join("share/c.txt"), "too short");
        let second = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(second.hop())).is_ok());
        // b.dll is in the first range and c.txt in the third; each alone is a range of its own
        // now.
        let ranges = second.ranges();
        assert_eq!(ranges.len(), 2);
        assert_eq!(
            ranges[0],
            Range {
                start: built.at("bin/b.dll").offset,
                length: built.at("bin/b.dll").csize
            }
        );
        assert_eq!(
            ranges[1],
            Range {
                start: 6819600,
                length: 15
            }
        );
        for placed in &built.placed {
            assert_eq!(
                read_text(&rig.stage_dir.join(&placed.path)),
                placed.content,
                "{}",
                placed.path
            );
        }
    }

    #[test]
    fn a_record_of_an_earlier_complete_stage_does_not_survive_a_failed_run() {
        let mut rig = Rig::new("stage_stale_record");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let first = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(first.hop())).is_ok());
        assert!(is_present(&rig.stage_dir.join("manifest.json")));

        // A file goes missing and the network is down: the run fails, and must not leave a stage
        // that still says it is complete.
        std::fs::remove_file(rig.stage_dir.join("share/c.txt")).expect("the staged file");
        let mut broken = PayloadServer::new(built.payload.clone());
        broken.custom = Some(Box::new(|_index: usize, _hop: &HttpHop| {
            Some(Err(make_error(
                ErrorCode::FileImportFailure,
                "no network",
                "status=0; https://github.com/x",
            )))
        }));
        let status = rig.run(&fetch_of(broken.hop()));
        assert_eq!(reason_of(&status), "offline");
        assert!(!is_present(&rig.stage_dir.join("manifest.json")));
        assert!(!is_present(&rig.stage_dir.join("manifest.json.sig")));
    }

    #[test]
    fn a_file_that_cannot_be_put_in_its_place_fails_the_stage_and_leaves_no_half_file() {
        // The disk refuses the rename of bin/b.dll to its final name (error 5, as a file somebody
        // holds open gives). a.dll, before it in the same range, is staged; b.dll is under no name
        // at all; and the run that follows a repaired disk asks only for what is missing.
        let mut rig = Rig::new("stage_rename_fails");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        rig.hooked.set_before_rename(|_from: &Path, to: &Path| {
            if to.file_name() == Some(OsStr::new("b.dll")) {
                return Some(injected(5, to));
            }
            None
        });
        let server = PayloadServer::new(built.payload.clone());
        let status = rig.run(&fetch_of(server.hop()));
        assert_eq!(reason_of(&status), "write_failed");
        let error = error_of(status);
        assert_eq!(error.code, ErrorCode::FileExportFailure);
        // The token, then the manifest's path, then the file system's own number (fsops.rs).
        assert!(
            error
                .context
                .starts_with("reason=write_failed; path=bin/b.dll; os_error=5 "),
            "{}",
            error.context
        );
        assert_eq!(
            server.hops().len(),
            1,
            "nothing more is fetched into a folder that refuses"
        );
        assert_eq!(files_under(&rig.stage_dir), names(&["bin/a.dll"]));
        assert!(!is_present(&rig.stage_dir.join(".partial")));

        *rig.hooked.before_rename.borrow_mut() = None;
        let second = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(second.hop())).is_ok());
        // b.dll alone (a.dll, 500 bytes before it, is staged), then the two far ranges.
        let ranges = second.ranges();
        assert_eq!(ranges.len(), 3);
        assert_eq!(
            ranges[0],
            Range {
                start: 3549,
                length: 45
            }
        );
        assert_eq!(
            ranges[1],
            Range {
                start: 2100746,
                length: 3145990
            }
        );
        assert_eq!(
            ranges[2],
            Range {
                start: 6819600,
                length: 15
            }
        );
        for placed in &built.placed {
            assert_eq!(
                read_text(&rig.stage_dir.join(&placed.path)),
                placed.content,
                "{}",
                placed.path
            );
        }
    }

    #[test]
    fn progress_never_goes_backwards_or_past_the_total_whatever_the_transport_reports() {
        // One member of 100 bytes: a stored block of 105, so one request of 105 bytes. The
        // transport counts 80 bytes, starts the request again (a dropped connection: 10), and then
        // reports more than there is.
        let mut rig = Rig::new("stage_progress_restart");
        let built = build(
            &[member("bin/a.dll", pattern(100, 1), FileClass::Lib, 0)],
            1,
        );
        rig.use_built(&built);
        let server = PayloadServer::new(built.payload.clone());
        let inner = server.hop();
        let hop = hop_fn(|one, stop, progress| {
            if let Some(progress) = progress {
                progress(80, 105);
                progress(10, 105);
                progress(100000, 0);
            }
            inner.as_ref().expect("the server's hop")(one, stop, None)
        });
        assert!(rig.run(&fetch_of(hop)).is_ok());
        let progress = rig.progress();
        assert!(progress.len() >= 5);
        assert_eq!(progress[0], (0, 105));
        assert_eq!(progress[1], (80, 105));
        assert_eq!(
            progress[2],
            (80, 105),
            "the restart is not shown as a step back"
        );
        assert_eq!(
            progress[3],
            (105, 105),
            "and no more than the request holds"
        );
        assert_eq!(progress[progress.len() - 1], (105, 105));
    }

    #[test]
    fn the_staged_record_is_the_signed_bytes_and_the_signature_line_exactly() {
        let mut rig = Rig::new("stage_record");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let server = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(server.hop())).is_ok());
        assert_eq!(read_text(&staged_manifest_path(&rig.stage_dir)), built.json);
        assert_eq!(
            read_text(&staged_signature_path(&rig.stage_dir)),
            built.signature_line
        );
        assert_eq!(
            staged_manifest_path(&rig.stage_dir).file_name(),
            Some(OsStr::new("manifest.json"))
        );
        assert_eq!(
            staged_signature_path(&rig.stage_dir).file_name(),
            Some(OsStr::new("manifest.json.sig"))
        );
    }

    // ---- free space and refused input -------------------------------------------------------

    #[test]
    fn not_enough_free_space_is_refused_before_anything_is_fetched() {
        let mut rig = Rig::new("stage_space");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        rig.the_plan.required_free_bytes = u64::MAX; // more than any volume has
        let server = PayloadServer::new(built.payload.clone());
        let status = rig.run(&fetch_of(server.hop()));
        assert_eq!(reason_of(&status), "low_disk_space");
        let error = error_of(status);
        assert_eq!(error.code, ErrorCode::FileExportFailure);
        assert!(error.context.contains("need="));
        assert!(error.context.contains("free="));
        assert!(server.hops().is_empty());
        assert!(files_under(&rig.stage_dir).is_empty());
    }

    #[test]
    fn the_free_space_needed_is_the_plans_figure_less_what_is_already_staged_to_the_byte() {
        // The four files of three_ranges() are 3000 + 40 + 3145745 + 10 = 3148795 bytes, and 2.6
        // times that is 8186867 exactly (3148795 * 26 = 81868670). A volume with one byte less is
        // refused before anything is asked for; a volume with exactly that much is not.
        let built = build(&three_ranges(), 1);
        {
            let mut rig = Rig::new("stage_space_exact");
            rig.use_built(&built);
            assert_eq!(rig.the_plan.required_free_bytes, 8186867);
            rig.hooked.free_bytes.set(Some(8186866));
            let refused = PayloadServer::new(built.payload.clone());
            let status = rig.run(&fetch_of(refused.hop()));
            assert_eq!(reason_of(&status), "low_disk_space");
            assert_eq!(
                error_of(status).context,
                "reason=low_disk_space; need=8186867 bytes; free=8186866 bytes"
            );
            assert!(refused.hops().is_empty());

            rig.hooked.free_bytes.set(Some(8186867));
            let enough = PayloadServer::new(built.payload.clone());
            assert!(rig.run(&fetch_of(enough.hop())).is_ok());
        }
        {
            // a.dll and b.dll (3040 bytes) are staged by a run that is then cancelled. They are on
            // the disk already, so what the next run needs free is 8186867 - 3040 = 8183827.
            let mut rig = Rig::new("stage_space_resumed");
            rig.use_built(&built);
            let source = AtomicBool::new(false);
            let mut first = PayloadServer::new(built.payload.clone());
            first.before = Some(Box::new(|index: usize| {
                if index == 1 {
                    source.store(true, Ordering::Relaxed);
                }
            }));
            assert!(
                rig.run_stopped(&fetch_of(first.hop()), Some(&source))
                    .is_err()
            );
            assert_eq!(
                files_under(&rig.stage_dir),
                names(&["bin/a.dll", "bin/b.dll"])
            );

            rig.hooked.free_bytes.set(Some(8183826));
            let refused = PayloadServer::new(built.payload.clone());
            let status = rig.run(&fetch_of(refused.hop()));
            assert_eq!(
                error_of(status).context,
                "reason=low_disk_space; need=8183827 bytes; free=8183826 bytes"
            );
            assert!(refused.hops().is_empty());
            assert_eq!(
                files_under(&rig.stage_dir),
                names(&["bin/a.dll", "bin/b.dll"]),
                "what was staged is kept for the day there is room"
            );

            rig.hooked.free_bytes.set(Some(8183827));
            let enough = PayloadServer::new(built.payload.clone());
            assert!(rig.run(&fetch_of(enough.hop())).is_ok());
        }
    }

    #[test]
    fn a_plan_of_two_and_a_half_times_the_changed_bytes_is_the_plans_figure_and_it_is_honoured() {
        // plan() computes ceil(2.6 * changed_bytes) (plan.rs); stage() takes that figure as given
        // and needs it free, less what is already staged.
        let mut rig = Rig::new("stage_figure");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let changed: u64 = 3000 + 40 + (3 * 1024 * 1024 + 17) + 10;
        assert_eq!(rig.the_plan.changed_bytes, changed);
        assert_eq!(
            rig.the_plan.required_free_bytes,
            (changed * 26).div_ceil(10)
        );
    }

    #[test]
    fn an_unverified_manifest_or_a_plan_that_is_not_its_is_refused() {
        let mut rig = Rig::new("stage_refused");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        let server = PayloadServer::new(built.payload.clone());
        let fetch = fetch_of(server.hop());
        let staged = |manifest: &Manifest, plan: &Plan, folder: &Path| {
            stage_with(
                &fetch,
                &rig.feed,
                manifest,
                plan,
                folder,
                None,
                None,
                &rig.hooked,
            )
        };

        let mut unverified = built.manifest.clone();
        unverified.signed_json.clear();
        assert_eq!(
            reason_of(&staged(&unverified, &rig.the_plan, &rig.stage_dir)),
            "unverified_manifest"
        );
        let mut unverified = built.manifest.clone();
        unverified.signature_line.clear();
        assert_eq!(
            reason_of(&staged(&unverified, &rig.the_plan, &rig.stage_dir)),
            "unverified_manifest"
        );

        let mut notify = built.manifest.clone();
        notify.notify_only = true;
        assert_eq!(
            reason_of(&staged(&notify, &rig.the_plan, &rig.stage_dir)),
            "notify_only"
        );
        let mut no_payload = built.manifest.clone();
        no_payload.payload = None;
        assert_eq!(
            reason_of(&staged(&no_payload, &rig.the_plan, &rig.stage_dir)),
            "notify_only"
        );

        let mut other = rig.the_plan.clone();
        other.files.pop();
        assert_eq!(
            error_of(staged(&built.manifest, &other, &rig.stage_dir)).code,
            ErrorCode::InvalidArgument
        );
        let mut wild = rig.the_plan.clone();
        wild.files[0].index = 99;
        assert_eq!(
            error_of(staged(&built.manifest, &wild, &rig.stage_dir)).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            error_of(staged(&built.manifest, &rig.the_plan, Path::new(""))).code,
            ErrorCode::InvalidArgument
        );
        let no_transport: Fetch<'_> = None;
        assert_eq!(
            error_of(stage_with(
                &no_transport,
                &rig.feed,
                &built.manifest,
                &rig.the_plan,
                &rig.stage_dir,
                None,
                None,
                &rig.hooked
            ))
            .code,
            ErrorCode::InvalidArgument
        );
        assert!(server.hops().is_empty());
    }

    #[test]
    fn nothing_to_change_stages_the_record_alone() {
        // Every file is already installed and right: no request, and a complete stage all the
        // same.
        let mut rig = Rig::new("stage_nothing");
        let built = build(
            &[member("bin/a.dll", pattern(100, 1), FileClass::Lib, 0)],
            1,
        );
        write_text(&rig.root.join("bin/a.dll"), &built.at("bin/a.dll").content);
        rig.use_built(&built);
        assert_eq!(rig.the_plan.changed_files, 0);
        let server = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(server.hop())).is_ok());
        assert!(server.hops().is_empty());
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&["manifest.json", "manifest.json.sig"])
        );
    }

    #[test]
    fn an_empty_file_is_staged_like_any_other() {
        let mut rig = Rig::new("stage_empty_file");
        let built = build(
            &[
                member("share/empty.txt", "", FileClass::Data, 0),
                member("share/one.txt", "x", FileClass::Data, 0),
            ],
            1,
        );
        rig.use_built(&built);
        let server = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(server.hop())).is_ok());
        assert_eq!(read_text(&rig.stage_dir.join("share/empty.txt")), b"");
        assert!(is_present(&rig.stage_dir.join("share/empty.txt")));
        assert_eq!(read_text(&rig.stage_dir.join("share/one.txt")), b"x");
    }

    // ---- one stage at a time ----------------------------------------------------------------

    #[test]
    fn the_lock_of_a_stage_folder_is_a_file_beside_it() {
        // stage.rs: the folder's own name with ".lock" after it, beside the folder - so the folder
        // holds the release's files and the two records, and nothing else.
        assert_eq!(
            stage_lock_path(&Path::new("U").join("stage-2000000000")),
            Path::new("U").join("stage-2000000000.lock")
        );
        assert_eq!(
            stage_lock_path(Path::new("U/stage-5/")),
            Path::new("U/stage-5.lock"),
            "a trailing separator names the same folder"
        );
    }

    #[test]
    fn a_second_stage_of_the_same_folder_is_refused_as_busy_and_touches_nothing() {
        let mut rig = Rig::new("stage_busy");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        // What another run - the window's, while katana_cli starts this one - has in the folder:
        // a member it is writing, and the record of an earlier complete run.
        write_text(
            &rig.stage_dir.join(".partial").join("2"),
            "another run's half file",
        );
        write_text(&rig.stage_dir.join("manifest.json"), "another run's record");
        // That run holds the folder's lock. (The harness's file system keeps a lock exclusive
        // between holders, as the real one does between processes.)
        let other: Box<dyn FileLock> = rig
            .disk
            .try_lock(&stage_lock_path(&rig.stage_dir))
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        let server = PayloadServer::new(built.payload.clone());
        let status = rig.run(&fetch_of(server.hop()));
        assert_eq!(reason_of(&status), "stage_busy");
        assert_eq!(error_of(status).code, ErrorCode::AlreadyExists);
        assert!(server.hops().is_empty(), "nothing is asked of the network");
        // Neither the other run's temporary file nor its record was deleted or rewritten.
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&[".partial/2", "manifest.json"])
        );
        assert_eq!(
            read_text(&rig.stage_dir.join(".partial").join("2")),
            b"another run's half file"
        );
        assert_eq!(
            read_text(&rig.stage_dir.join("manifest.json")),
            b"another run's record"
        );

        // The other run ends. The same call now goes through and leaves a complete stage ...
        drop(other);
        let second = PayloadServer::new(built.payload.clone());
        let again = rig.run(&fetch_of(second.hop()));
        assert!(again.is_ok(), "{}", describe(&again));
        assert_eq!(second.ranges().len(), 3);
        assert_eq!(read_text(&rig.stage_dir.join("manifest.json")), built.json);
        assert!(!is_present(&rig.stage_dir.join(".partial")));
        // ... and so does one straight after it: the lock is let go when stage() returns.
        let third = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(third.hop())).is_ok());
    }

    #[test]
    fn what_stands_in_a_staged_files_way_is_removed_and_the_file_is_staged() {
        // Somebody left a folder, with files in it, where bin/b.dll goes, a file where the folder
        // `share` goes, and a folder where the staged record goes. A stage that stopped at any of
        // them would stop at it on every later run as well; the names under the stage folder are
        // the stage's (stage.rs), so they go and every name under the folder is again a verified
        // file or a record.
        let mut rig = Rig::new("stage_in_the_way");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        write_text(
            &rig.stage_dir.join("bin/b.dll").join("inner").join("x.txt"),
            "in the way",
        );
        write_text(
            &rig.stage_dir.join("bin/b.dll").join("y.txt"),
            "in the way too",
        );
        write_text(&rig.stage_dir.join("share"), "a file named like a folder");
        write_text(
            &rig.stage_dir.join("manifest.json").join("x.txt"),
            "a folder named like the record",
        );

        let server = PayloadServer::new(built.payload.clone());
        let status = rig.run(&fetch_of(server.hop()));
        assert!(status.is_ok(), "{}", describe(&status));
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&[
                "bin/a.dll",
                "bin/b.dll",
                "share/big.bin",
                "share/c.txt",
                "manifest.json",
                "manifest.json.sig"
            ])
        );
        for placed in &built.placed {
            assert_eq!(
                read_text(&rig.stage_dir.join(&placed.path)),
                placed.content,
                "{}",
                placed.path
            );
        }
        assert_eq!(read_text(&staged_manifest_path(&rig.stage_dir)), built.json);
    }

    // ---- older stage folders ----------------------------------------------------------------

    #[test]
    fn folders_of_older_releases_are_swept_and_nothing_else_is() {
        let mut rig = Rig::new("stage_sweep");
        let built = build(
            &[member("bin/a.dll", pattern(100, 1), FileClass::Lib, 0)],
            1,
        );
        rig.use_built(&built);
        let older1 = rig.state_dir.join("stage-1");
        let older2 = rig.state_dir.join("stage-1999999999");
        let newer = rig.state_dir.join("stage-2000000001");
        let not_a_number = rig.state_dir.join("stage-abc");
        let bare = rig.state_dir.join("stage-");
        let other = rig.state_dir.join("run-0123abcd");
        let a_file = rig.state_dir.join("stage-5");
        write_text(&older1.join("bin/old.dll"), "old");
        write_text(&older2.join("share/deep/er/old.txt"), "old");
        write_text(
            &newer.join("bin/new.dll"),
            "newer than this release: not ours to delete",
        );
        write_text(&not_a_number.join("x"), "x");
        write_text(&bare.join("x"), "x");
        write_text(&other.join("katana_updater.exe"), "a helper copy");
        write_text(&a_file, "a file named like a folder");

        let server = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(server.hop())).is_ok());
        assert!(!is_present(&older1));
        assert!(!is_present(&older2));
        assert!(is_present(&newer.join("bin/new.dll")));
        assert!(is_present(&not_a_number.join("x")));
        assert!(is_present(&bare.join("x")));
        assert!(is_present(&other.join("katana_updater.exe")));
        assert!(is_present(&a_file), "only folders are swept");
        assert!(is_present(&rig.stage_dir.join("bin/a.dll")));
    }

    #[test]
    fn an_older_folder_that_another_program_is_filling_is_left_and_the_lock_files_go_with_their_folders()
     {
        let mut rig = Rig::new("stage_sweep_locked");
        let built = build(
            &[member("bin/a.dll", pattern(100, 1), FileClass::Lib, 0)],
            1,
        );
        rig.use_built(&built);
        let busy = rig.state_dir.join("stage-7");
        let idle = rig.state_dir.join("stage-8");
        // The helper deletes the folder of a release it installed, and not the lock beside it.
        let orphan_lock = rig.state_dir.join("stage-9.lock");
        let newer_lock = rig.state_dir.join("stage-2000000001.lock");
        write_text(&busy.join("bin/half.dll"), "being downloaded");
        write_text(&idle.join("bin/old.dll"), "old");
        write_text(&stage_lock_path(&idle), "");
        write_text(&orphan_lock, "");
        write_text(&newer_lock, "");
        let filling: Box<dyn FileLock> = rig
            .disk
            .try_lock(&stage_lock_path(&busy))
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        let server = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(server.hop())).is_ok());
        assert!(
            is_present(&busy.join("bin/half.dll")),
            "another program holds its lock"
        );
        assert!(is_present(&stage_lock_path(&busy)));
        assert!(!is_present(&idle));
        assert!(!is_present(&stage_lock_path(&idle)));
        assert!(!is_present(&orphan_lock));
        assert!(
            is_present(&newer_lock),
            "a newer release's is not this one's to delete"
        );
        assert!(
            is_present(&stage_lock_path(&rig.stage_dir)),
            "the lock's file stays when the lock is let go"
        );

        // That program ends: the next stage sweeps its folder as well.
        drop(filling);
        let second = PayloadServer::new(built.payload.clone());
        assert!(rig.run(&fetch_of(second.hop())).is_ok());
        assert!(!is_present(&busy));
        assert!(!is_present(&stage_lock_path(&busy)));
    }

    // ---- the private-repository route -------------------------------------------------------

    #[test]
    fn on_the_api_route_the_payload_is_found_by_tag_and_the_token_never_reaches_the_storage_host() {
        let mut rig = Rig::new("stage_api");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        rig.feed.route = FeedRoute::Api;
        rig.feed.token = String::from(SECRET_TOKEN);

        let storage = PayloadServer::new(built.payload.clone());
        let storage_hop = storage.hop();
        let seen: RefCell<Vec<HttpHop>> = RefCell::new(Vec::new());
        let signed_url = "https://objects.example.org/blob/abc?sp=r&sig=SIGNATURE-IN-QUERY";
        let payload_size = built.payload.len();
        let hop = hop_fn(|one, stop, progress| {
            seen.borrow_mut().push(one.clone());
            if one.url == "https://api.github.com/repos/kjarada/Katana/releases/tags/v1.0.0" {
                return Ok(HttpHopReply {
                    status: 200,
                    content_type: String::from("application/json"),
                    body: format!(
                        "{{\"assets\":[{{\"id\":5,\"name\":\"Katana-update-win64.json\"}},\
                         {{\"id\":777,\"name\":\"Katana-1.0.0-win64.update.zip\",\"size\":\
                         {payload_size}}}]}}"
                    )
                    .into_bytes(),
                    ..HttpHopReply::default()
                });
            }
            if one.url == "https://api.github.com/repos/kjarada/Katana/releases/assets/777" {
                return Ok(HttpHopReply {
                    status: 302,
                    location: String::from(signed_url),
                    ..HttpHopReply::default()
                });
            }
            assert_eq!(one.url, signed_url);
            storage_hop.as_ref().expect("the storage's hop")(one, stop, progress)
        });
        let status = rig.run(&fetch_of(hop));
        assert!(status.is_ok(), "{}", describe(&status));

        // 1 lookup, then for each of the 3 ranges: the API hop and the storage hop.
        let seen = seen.borrow();
        assert_eq!(seen.len(), 1 + 3 * 2);
        let bearer = format!("Authorization: Bearer {SECRET_TOKEN}");
        let has = |one: &HttpHop, line: &str| one.headers.iter().any(|header| header == line);
        assert!(has(&seen[0], &bearer));
        assert!(has(&seen[0], "Accept: application/vnd.github+json"));
        for (i, one) in seen.iter().enumerate().skip(1) {
            let api_hop = i % 2 == 1;
            assert_eq!(has(one, &bearer), api_hop, "{}", one.url);
            assert!(has(one, "Accept: application/octet-stream"), "{}", one.url);
            assert!(range_of(one).is_some(), "{}", one.url);
        }
        for placed in &built.placed {
            assert_eq!(
                read_text(&rig.stage_dir.join(&placed.path)),
                placed.content,
                "{}",
                placed.path
            );
        }
    }

    #[test]
    fn the_api_route_refuses_what_does_not_add_up() {
        let built = build(&three_ranges(), 1);
        let lookup = |body: &str, route: FeedRoute, token: &str| {
            let mut rig = Rig::new("stage_api_bad");
            rig.use_built(&built);
            rig.feed.route = route;
            rig.feed.token = String::from(token);
            let hops = Cell::new(0usize);
            let hop = hop_fn(|_one, _stop, _progress| {
                hops.set(hops.get() + 1);
                Ok(HttpHopReply {
                    status: 200,
                    body: body.as_bytes().to_vec(),
                    ..HttpHopReply::default()
                })
            });
            let reason = reason_of(&rig.run(&fetch_of(hop)));
            (reason, hops.get())
        };
        assert_eq!(
            lookup("{\"assets\":[]}", FeedRoute::Api, SECRET_TOKEN).0,
            "asset_missing"
        );
        assert_eq!(
            lookup(
                "{\"assets\":[{\"id\":1,\"name\":\"Katana-1.0.0-win64.update.zip\",\"size\":12}]}",
                FeedRoute::Api,
                SECRET_TOKEN
            )
            .0,
            "payload_mismatch",
            "the listing's length is not the signed one: the release may have been replaced"
        );
        assert_eq!(
            lookup("not json", FeedRoute::Api, SECRET_TOKEN).0,
            "malformed"
        );
        let no_token = lookup("{}", FeedRoute::Api, "");
        assert_eq!(no_token.0, "no_token");
        assert_eq!(no_token.1, 0, "no token, no request");
    }

    #[test]
    fn the_download_route_never_carries_a_token_even_when_the_config_holds_one() {
        let mut rig = Rig::new("stage_download_token");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        rig.feed.token = String::from(SECRET_TOKEN); // route stays Download
        let mut server = PayloadServer::new(built.payload.clone());
        server.url = String::from(
            "https://github.com/kjarada/Katana/releases/download/v1.0.0/Katana-1.0.0-win64.update.zip",
        );
        assert!(rig.run(&fetch_of(server.hop())).is_ok());
        let hops = server.hops();
        assert_eq!(hops.len(), 3);
        for hop in &hops {
            for header in &hop.headers {
                assert!(!header.contains(SECRET_TOKEN), "{header}");
                assert!(!header.starts_with("Authorization"));
            }
            assert_eq!(hop.user_agent, "Katana/0.9.0 (win64)");
        }
    }

    #[test]
    fn no_error_of_a_stage_names_the_token() {
        let mut rig = Rig::new("stage_token_errors");
        let built = build(&three_ranges(), 1);
        rig.use_built(&built);
        rig.feed.route = FeedRoute::Api;
        rig.feed.token = String::from(SECRET_TOKEN);
        for code in [401, 403, 404, 429] {
            let hop = hop_fn(move |_one, _stop, _progress| {
                Ok(HttpHopReply {
                    status: code,
                    ..HttpHopReply::default()
                })
            });
            let status = rig.run(&fetch_of(hop));
            let error = error_of(status);
            assert!(!error.describe().contains(SECRET_TOKEN), "{code}");
        }
    }
}

mod preflight {
    use super::*;

    #[test]
    fn the_installed_helper_is_copied_to_the_run_folder_and_run_with_selftest() {
        let rig = preflight_rig("preflight_ok");
        let launcher = FakeLauncher::default();
        let manifest = manifest_with_helper(1, "STAGED HELPER", &rig.root);
        let prepared = prepare(&rig, &manifest, &launcher)
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        assert!(is_valid_transaction_id(&prepared.transaction));
        assert_eq!(
            prepared.run_directory,
            run_path(&rig.state_dir, &prepared.transaction)
        );
        assert_eq!(
            prepared.helper.parent(),
            Some(prepared.run_directory.as_path())
        );
        assert_eq!(prepared.helper.file_name(), rig.helper_path.file_name());
        assert_eq!(
            read_text(&prepared.helper),
            b"INSTALLED HELPER, protocol 1",
            "the installed helper, because the manifest needs no newer protocol"
        );

        let ran = launcher.ran();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].executable, prepared.helper);
        assert_eq!(ran[0].arguments, vec![String::from("--selftest")]);
        assert!(
            launcher.started().is_empty(),
            "preflight starts nothing that outlives it"
        );
        assert_eq!(rig.run_folders().len(), 1);
        // Nothing of the install was touched.
        assert_eq!(read_text(&rig.helper_path), b"INSTALLED HELPER, protocol 1");
    }

    #[test]
    fn a_newer_protocol_runs_the_staged_helper_after_its_hash_is_checked() {
        let rig = preflight_rig("preflight_staged");
        let launcher = FakeLauncher::default();
        let manifest =
            manifest_with_helper(APPLIER_PROTOCOL + 1, "STAGED HELPER protocol 2", &rig.root);
        let name = helper_name(&rig.root);

        // Not staged yet: refused, with no run folder left behind.
        let missing = error_of(prepare(&rig, &manifest, &launcher));
        assert_eq!(fetch_failure_reason(&missing), "helper_missing");
        assert!(rig.run_folders().is_empty());
        assert!(launcher.ran().is_empty());

        // Staged but not the signed bytes: refused.
        write_text(&rig.stage_dir.join("bin").join(&name), "SOMETHING ELSE");
        let wrong = error_of(prepare(&rig, &manifest, &launcher));
        assert_eq!(fetch_failure_reason(&wrong), "helper_missing");
        assert!(rig.run_folders().is_empty());

        // Staged and right: that one runs, not the installed one.
        write_text(
            &rig.stage_dir.join("bin").join(&name),
            "STAGED HELPER protocol 2",
        );
        let prepared = prepare(&rig, &manifest, &launcher)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read_text(&prepared.helper), b"STAGED HELPER protocol 2");
        let ran = launcher.ran();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].executable, prepared.helper);
        assert_eq!(
            read_text(&rig.helper_path),
            b"INSTALLED HELPER, protocol 1",
            "the install is untouched"
        );
    }

    #[test]
    fn a_staged_helper_that_changes_between_the_check_and_the_copy_is_not_run() {
        // The stage folder is the user's to write. Here the staged helper is the signed one when
        // it is hashed there, and another file by the time it is copied: it is swapped at the last
        // moment there is, as the copy begins. What must be the signed helper is the COPY in the
        // run folder - the file that would run. First a swap that changes the length, then one
        // that keeps it (24 bytes each): it is the hash that is held, not the size.
        for swapped in ["ANOTHER PROGRAM ALTOGETHER", "STAGED HELPER protocol 3"] {
            let rig = preflight_rig("preflight_swapped");
            let launcher = FakeLauncher::default();
            let manifest =
                manifest_with_helper(APPLIER_PROTOCOL + 1, "STAGED HELPER protocol 2", &rig.root);
            write_text(
                &rig.stage_dir.join("bin").join(helper_name(&rig.root)),
                "STAGED HELPER protocol 2",
            );
            rig.hooked.set_before_copy(move |from: &Path, _to: &Path| {
                write_text(from, swapped);
                None // and the copy is made, of what is there now
            });
            let prepared = error_of(prepare(&rig, &manifest, &launcher));
            assert_eq!(prepared.code, ErrorCode::InvalidState, "{swapped}");
            assert_eq!(
                fetch_failure_reason(&prepared),
                "helper_missing",
                "{swapped}"
            );
            assert!(
                launcher.ran().is_empty(),
                "what was copied is not the signed helper: {swapped}"
            );
            assert!(rig.run_folders().is_empty(), "{swapped}");
        }
    }

    #[test]
    fn a_newer_protocol_with_no_helper_in_the_manifest_is_refused() {
        let rig = preflight_rig("preflight_no_helper");
        let launcher = FakeLauncher::default();
        let built = build(
            &[member("share/other.txt", "o", FileClass::Data, 0)],
            APPLIER_PROTOCOL + 1,
        );
        let prepared = error_of(prepare(&rig, &built.manifest, &launcher));
        assert_eq!(fetch_failure_reason(&prepared), "helper_missing");
        assert!(rig.run_folders().is_empty());
    }

    #[test]
    fn a_helper_that_fails_its_selftest_is_reported_and_leaves_no_run_folder() {
        let rig = preflight_rig("preflight_fail");
        let manifest = manifest_with_helper(1, "x", &rig.root);

        {
            let launcher = FakeLauncher::default();
            launcher.set_on_run(|_request: &SpawnRequest| {
                Ok(SpawnResult {
                    exit_code: 3,
                    output: b"selftest: OpenSSL could not be initialised\n".to_vec(),
                    ..SpawnResult::default()
                })
            });
            let prepared = error_of(prepare(&rig, &manifest, &launcher));
            assert_eq!(fetch_failure_reason(&prepared), "helper_failed");
            assert!(prepared.context.contains("exit=3"));
            assert!(
                prepared
                    .context
                    .contains("OpenSSL could not be initialised")
            );
            assert!(rig.run_folders().is_empty());
        }
        {
            let launcher = FakeLauncher::default();
            launcher.set_on_run(|_request: &SpawnRequest| {
                Ok(SpawnResult {
                    timed_out: true,
                    ..SpawnResult::default()
                })
            });
            let prepared = error_of(prepare(&rig, &manifest, &launcher));
            assert_eq!(fetch_failure_reason(&prepared), "helper_timeout");
            assert!(rig.run_folders().is_empty());
        }
        {
            let launcher = FakeLauncher::default();
            launcher.set_on_run(|_request: &SpawnRequest| {
                Err(Error::new(
                    ErrorCode::FileImportFailure,
                    "CreateProcess failed: 193",
                ))
            });
            let prepared = error_of(prepare(&rig, &manifest, &launcher));
            assert_eq!(fetch_failure_reason(&prepared), "helper_not_started");
            assert!(prepared.message.contains("193"));
            assert!(rig.run_folders().is_empty());
        }
    }

    #[test]
    fn an_install_without_a_helper_is_refused_before_anything_is_created() {
        let rig = preflight_rig("preflight_absent");
        std::fs::remove_file(&rig.helper_path).expect("the helper");
        let launcher = FakeLauncher::default();
        let manifest = manifest_with_helper(1, "x", &rig.root);
        let prepared = error_of(prepare(&rig, &manifest, &launcher));
        assert_eq!(fetch_failure_reason(&prepared), "helper_missing");
        assert!(rig.run_folders().is_empty());
        assert!(launcher.ran().is_empty());

        let no_state = preflight_with(
            &manifest,
            &rig.install,
            &rig.stage_dir,
            Path::new(""),
            &launcher,
            &rig.hooked,
        );
        assert_eq!(error_of(no_state).code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn each_run_gets_its_own_transaction_and_folder() {
        let rig = preflight_rig("preflight_twice");
        let launcher = FakeLauncher::default();
        let manifest = manifest_with_helper(1, "x", &rig.root);
        let first = prepare(&rig, &manifest, &launcher)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let second = prepare(&rig, &manifest, &launcher)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_ne!(first.transaction, second.transaction);
        assert_ne!(first.run_directory, second.run_directory);
        assert_eq!(rig.run_folders().len(), 2);
    }

    #[test]
    fn a_helper_that_cannot_be_copied_is_a_write_failure_that_leaves_no_run_folder_and_starts_nothing()
     {
        let rig = preflight_rig("preflight_copy_fails");
        let launcher = FakeLauncher::default();
        let manifest = manifest_with_helper(1, "x", &rig.root);
        // ERROR_DISK_FULL.
        rig.hooked
            .set_before_copy(|_from: &Path, to: &Path| Some(injected(112, to)));
        let prepared = error_of(prepare(&rig, &manifest, &launcher));
        assert_eq!(prepared.code, ErrorCode::FileExportFailure);
        assert_eq!(fetch_failure_reason(&prepared), "write_failed");
        assert!(
            prepared.context.contains("os_error=112"),
            "{}",
            prepared.context
        );
        assert!(rig.run_folders().is_empty());
        assert!(
            launcher.ran().is_empty(),
            "a helper that was not copied is not started"
        );
    }
}

// ---- once through the file system that ships -------------------------------------------------------

// Everything above hands stage() and preflight() the harness's portable file system. The programs
// call the forms without one, which use make_system_fs_ops: long paths, and a rename that waits out
// a virus scanner. The fixture release once through those, from an empty stage to a preflighted
// helper, and once more to see the resume find its own files.
#[cfg(windows)]
mod stage_on_the_system_file_system {
    use super::*;
    use katana_update::stage::{preflight, stage};

    #[test]
    fn the_fixture_release_is_staged_resumed_and_its_helper_preflighted() {
        let rig = fixture_rig("stage_system_fs", "v2", true);
        let progress: RefCell<Vec<(u64, u64)>> = RefCell::new(Vec::new());
        let record: &dyn Fn(u64, u64) =
            &|done: u64, total: u64| progress.borrow_mut().push((done, total));

        let server = PayloadServer::new(rig.v2_zip.clone());
        let status = stage(
            &fetch_of(server.hop()),
            &rig.feed,
            &rig.manifest,
            &rig.the_plan,
            &rig.stage_dir,
            None,
            Some(record),
        );
        assert!(status.is_ok(), "{}", describe(&status));
        assert_eq!(
            files_under(&rig.stage_dir),
            names(&[
                "bin/katana.exe",
                "bin/libnew-2.dll",
                "share/katana/help/index.txt",
                "share/katana/news.txt",
                "share/katana/readme.txt",
                "manifest.json",
                "manifest.json.sig"
            ])
        );
        for path in [
            "bin/katana.exe",
            "bin/libnew-2.dll",
            "share/katana/help/index.txt",
            "share/katana/news.txt",
            "share/katana/readme.txt",
        ] {
            assert_eq!(
                read_text(&rig.stage_dir.join(path)),
                rig.v2_file(path),
                "{path}"
            );
        }
        assert_eq!(
            read_text(&rig.stage_dir.join("manifest.json")),
            read_text(&fixture_folder().join("release/v2/Katana-update-win64.json"))
        );
        let ranges = server.ranges();
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            ranges[0],
            Range {
                start: 44,
                length: 2949
            }
        );
        let recorded = progress.borrow().clone();
        assert!(!recorded.is_empty());
        assert_eq!(recorded[recorded.len() - 1], (2949, 2949));

        // One staged file goes missing: the second run finds the other four by their hashes and
        // asks for that file and no other.
        std::fs::remove_file(rig.stage_dir.join("share/katana/news.txt")).expect("the file");
        let second = PayloadServer::new(rig.v2_zip.clone());
        assert!(
            stage(
                &fetch_of(second.hop()),
                &rig.feed,
                &rig.manifest,
                &rig.the_plan,
                &rig.stage_dir,
                None,
                None,
            )
            .is_ok()
        );
        let ranges = second.ranges();
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            ranges[0],
            Range {
                start: 725,
                length: 54
            },
            "news.txt: offset 725, csize 54"
        );
        assert_eq!(
            read_text(&rig.stage_dir.join("share/katana/news.txt")),
            rig.v2_file("share/katana/news.txt")
        );

        // The helper: any bytes at its name (the launcher is a fake; nothing is started).
        let installed_helper = installed_program(&rig.root, "katana_updater");
        write_text(&installed_helper, "THE INSTALLED HELPER");
        let launcher = FakeLauncher::default();
        let prepared = preflight(
            &rig.manifest,
            &rig.install,
            &rig.stage_dir,
            &rig.state_dir,
            &launcher,
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            prepared.run_directory,
            run_path(&rig.state_dir, &prepared.transaction)
        );
        assert_eq!(read_text(&prepared.helper), b"THE INSTALLED HELPER");
        let ran = launcher.ran();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].executable, prepared.helper);
        assert_eq!(ran[0].arguments, vec![String::from("--selftest")]);
    }
}
