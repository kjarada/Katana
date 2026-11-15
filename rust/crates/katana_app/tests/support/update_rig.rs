//! The UPDATE verbs' rig: each line of the grammar run for real over a fake
//! feed, a scratch install and a scratch state folder - the twin of the head
//! of tests/app/test_update_verbs.cpp.
//!
//! WHERE THE EXPECTATIONS COME FROM (the testing rule: never from this
//! program's own output). The releases are the fixtures of
//! tools/update_make_fixtures.py (tests/update/fixtures): two install trees,
//! v1 = 0.9.0 (sequence 1790000000) and v2 = 0.10.0 (sequence 1792312800),
//! and for each a manifest made by the Python producer and signed with
//! `openssl` by the published RFC 8032 test key. What an update from v1 to v2
//! fetches is read off the signed v2 manifest and the two trees, by hand:
//!
//! ```text
//! path                          v1                v2                     csize  offset
//! bin/katana.exe                180 bytes         207 bytes, other hash    85      44   CHANGED
//! bin/katana_cli.exe            the same hash                                           kept
//! bin/libcore-1.dll             the same hash                                           kept
//! bin/libnew-2.dll              absent            118 bytes                70     418   CHANGED
//! etc/katana/settings.ini       60 bytes          a seed: written only when absent      kept
//! share/katana/empty.txt        the same hash                                           kept
//! share/katana/help/index.txt   absent            82 bytes                 75     599   CHANGED
//! share/katana/news.txt         absent            54 bytes                 54     725   CHANGED
//! share/katana/random.bin       the same hash                                           kept
//! share/katana/readme.txt       52 bytes          55 bytes, other hash     55    2938   CHANGED
//! ```
//!
//! So 5 of the manifest's 10 files change, and their compressed sizes add to
//! 85 + 70 + 75 + 54 + 55 = 339: `files=5/10 download_bytes=339`. The changed
//! members lie between offset 44 and 2938 + 55 = 2993 with holes far below the
//! megabyte at which requests merge (katana_update's ranges), so a first
//! download is ONE request for bytes [44, 2993), 2949 of them. The payload is
//! 3791 bytes.
//!
//! The clock is the test's: every context says it is 2026-10-19T00:00:00Z,
//! which is 1792368000 (the v2 manifest's own sequence, 1792312800, is its
//! `published` time 2026-10-18T08:40:00Z, so midnight that day is 1792281600
//! and the next midnight 86400 later).
//!
//! The file system the staging goes through is the system's, which is written
//! for Windows only so far (katana_update's fsops): the tests that download or
//! apply run there, and everywhere else one test holds the verbs to their
//! refusal.
//!
//! THE RUST SHAPE. The fakes keep what they are told and what they were asked
//! in cells, since a context borrows them for as long as it lives and the C++
//! tests change a fake while a context made from it is held. A context
//! borrows its rig (Rig::context); a session's context source is a value the
//! session owns, so the rig it hands out is shared through an Rc (the process
//! target's CliSession).
//!
//! One owner (step 6's S1). A target that includes it declares, at its root,
//! `#[path = "../support/update_rig.rs"] pub mod update_rig;` and the update
//! layer's test keys, `#[path = "../../../katana_update/tests/test_key/mod.rs"]
//! pub mod test_key;`, which this file reads as `crate::test_key`.

#![allow(
    dead_code,
    reason = "Each test target that includes the rig uses a part of it."
)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use katana_app::{UpdateContext, UpdateReply, UpdateSurface};
use katana_core::error::make_error;
use katana_core::path_text::{generic_path_utf8, path_from_utf8};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::feed::fetch_fn;
use katana_update::{
    CredentialStore, FeedConfig, Fetch, FetchRequest, Install, InstallKind, Launcher, SpawnRequest,
    SpawnResult, installed_program, parse_version, staged_manifest_path, staged_signature_path,
};

use crate::test_key::test_trust;

pub const V1_SEQUENCE: u64 = 1790000000;
pub const V2_SEQUENCE: u64 = 1792312800;
/// 2026-10-19T00:00:00Z (the head of this file).
pub const NOW: i64 = 1792368000;
pub const NOW_TEXT: &str = "2026-10-19T00:00:00Z";

pub const SIGNATURE_URL: &str =
    "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json.sig";
pub const MANIFEST_URL: &str =
    "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json";
/// The payload of the release the manifest names: its tag and its payload.name.
pub const PAYLOAD_URL: &str =
    "https://github.com/kjarada/Katana/releases/download/v0.10.0/Katana-0.10.0-win64.update.zip";

/// What UPDATE CHECK says of v2 to a v1 install (the head of this file), with
/// its notes address.
pub const NOTES: &str = "https://github.com/kjarada/Katana/releases/tag/v0.10.0";
pub const AVAILABLE: &str = "update=available installed=0.9.0 latest=0.10.0 files=5/10 \
                             download_bytes=339 \
                             notes_url=https://github.com/kjarada/Katana/releases/tag/v0.10.0";
/// What UPDATE STATUS says of a v1 install with nothing kept, staged or stored.
pub const FRESH_STATUS: &str = "installed=0.9.0 target=win64 kind=archive staged=none \
                                skipped=none auto=on last_check=never token=absent last=none";

pub const SECRET: &str = "ghp_SECRET-TOKEN-0123456789";

/// The whole payload zip, as the v2 manifest signs its size.
pub const PAYLOAD_SIZE: u64 = 3791;

/// The files v2 changes, as the head of this file lists them.
pub const CHANGED: [&str; 5] = [
    "bin/katana.exe",
    "bin/libnew-2.dll",
    "share/katana/help/index.txt",
    "share/katana/news.txt",
    "share/katana/readme.txt",
];
pub const KEPT: [&str; 5] = [
    "bin/katana_cli.exe",
    "bin/libcore-1.dll",
    "etc/katana/settings.ini",
    "share/katana/empty.txt",
    "share/katana/random.bin",
];

/// tests/update/fixtures.
pub fn fixtures() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/update/fixtures"
    ))
}

/// A path as the C++'s generic_string() writes it.
pub fn generic(path: &Path) -> String {
    String::from_utf8_lossy(&generic_path_utf8(path)).into_owned()
}

pub fn bytes_of(file: &Path) -> Vec<u8> {
    std::fs::read(file).unwrap_or_else(|error| panic!("{}: {error}", generic(file)))
}

pub fn write_bytes(file: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(file, bytes).unwrap_or_else(|error| panic!("{}: {error}", generic(file)));
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|error| panic!("{}: {error}", generic(to)));
    let entries =
        std::fs::read_dir(from).unwrap_or_else(|error| panic!("{}: {error}", generic(from)));
    for entry in entries {
        let entry = entry.expect("a fixture folder lists");
        let target = to.join(entry.file_name());
        if entry
            .file_type()
            .expect("a fixture entry has a type")
            .is_dir()
        {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target)
                .unwrap_or_else(|error| panic!("{}: {error}", generic(&target)));
        }
    }
}

fn walk(root: &Path, folder: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(root, &path, files);
        } else if path.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            files.insert(generic(relative), bytes_of(&path));
        }
    }
}

/// Every file under `root`, by its path from there, with its bytes: what a
/// tree is, to say that it did not change.
pub fn tree_of(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}

/// The lines of `text` as std::getline reads them: a final line end makes no
/// empty line after it.
pub fn lines_of(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// The reply's text, or the refusal's whole description.
pub fn said(reply: &Result<UpdateReply>) -> String {
    match reply {
        Ok(reply) => reply.text.clone(),
        Err(error) => format!("error: {}", error.describe()),
    }
}

/// The first line of a reply that was given.
pub fn first_line_of(reply: &Result<UpdateReply>) -> String {
    lines_of(&said(reply))
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// The reply of a line that must be carried out (the C++'s
/// `ASSERT_TRUE(reply.ok()) << said(reply)`).
pub fn must(reply: Result<UpdateReply>) -> UpdateReply {
    match reply {
        Ok(reply) => reply,
        Err(error) => panic!("error: {}", error.describe()),
    }
}

/// The refusal of a line that must be refused (the C++'s
/// `ASSERT_FALSE(reply.ok()) << said(reply)`).
pub fn refused(reply: Result<UpdateReply>) -> Error {
    match reply {
        Ok(reply) => panic!("a reply where a refusal was expected: {}", reply.text),
        Err(error) => error,
    }
}

pub fn http_status(code: ErrorCode, number: i32) -> Error {
    make_error(
        code,
        format!("the server said {number}"),
        format!("status={number}; https://github.com/x"),
    )
}

/// A folder of a test's own, removed with it. Named by the test (its
/// thread's name is its path, of which the last word is the case), since
/// libtest runs the cases side by side, and numbered, since a test may have
/// two; the process is in the name as well.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub fn new() -> Self {
        static MADE: AtomicU64 = AtomicU64::new(0);
        let thread = std::thread::current();
        let test = thread
            .name()
            .and_then(|name| name.rsplit("::").next())
            .unwrap_or("none");
        let path = std::env::temp_dir().join(format!(
            "katana-update-verbs-{test}-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed) + 1
        ));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Default for Scratch {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A feed: addresses to bodies or failures, anything else a 404 as on the
/// real host; a range of a body when a range is asked for. Every request is
/// kept.
#[derive(Default)]
pub struct FakeFeed {
    bodies: RefCell<BTreeMap<String, Vec<u8>>>,
    failures: RefCell<BTreeMap<String, Error>>,
    pub requests: RefCell<Vec<FetchRequest>>,
}

impl FakeFeed {
    pub fn serve(&self, url: &str, body: Vec<u8>) {
        self.bodies.borrow_mut().insert(String::from(url), body);
        self.failures.borrow_mut().remove(url);
    }

    pub fn fail(&self, url: &str, error: Error) {
        self.failures.borrow_mut().insert(String::from(url), error);
    }

    pub fn fetch(&self) -> Fetch<'_> {
        fetch_fn(move |request, _stop, progress| {
            self.requests.borrow_mut().push(request.clone());
            if let Some(failure) = self.failures.borrow().get(&request.url) {
                return Err(failure.clone());
            }
            let part = {
                let bodies = self.bodies.borrow();
                let Some(body) = bodies.get(&request.url) else {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        "nothing at this address",
                        format!("status=404; {}", request.url),
                    ));
                };
                if request.range_length == 0 {
                    return Ok(body.clone());
                }
                if request.range_start + request.range_length > body.len() as u64 {
                    return Err(make_error(
                        ErrorCode::FileImportFailure,
                        "a range past the end of the file",
                        "status=416",
                    ));
                }
                let start = request.range_start as usize;
                body[start..start + request.range_length as usize].to_vec()
            };
            if let Some(progress) = progress {
                let size = part.len() as u64;
                progress(size / 2, size);
                progress(size, size);
            }
            Ok(part)
        })
    }

    pub fn requests_for(&self, url: &str) -> Vec<FetchRequest> {
        self.requests
            .borrow()
            .iter()
            .filter(|request| request.url == url)
            .cloned()
            .collect()
    }

    pub fn request_count(&self) -> usize {
        self.requests.borrow().len()
    }

    pub fn clear_requests(&self) {
        self.requests.borrow_mut().clear();
    }
}

pub struct FakeStore {
    pub token: RefCell<String>,
    pub place: Cell<&'static str>,
    pub read_failure: RefCell<Option<Error>>,
    pub clear_failure: RefCell<Option<Error>>,
    pub clears: Cell<i32>,
}

impl Default for FakeStore {
    fn default() -> Self {
        Self {
            token: RefCell::new(String::new()),
            place: Cell::new("test store"),
            read_failure: RefCell::new(None),
            clear_failure: RefCell::new(None),
            clears: Cell::new(0),
        }
    }
}

impl FakeStore {
    pub fn set_token(&self, token: &str) {
        *self.token.borrow_mut() = String::from(token);
    }

    pub fn set_read_failure(&self, failure: Error) {
        *self.read_failure.borrow_mut() = Some(failure);
    }

    pub fn set_clear_failure(&self, failure: Error) {
        *self.clear_failure.borrow_mut() = Some(failure);
    }
}

impl CredentialStore for FakeStore {
    fn read(&self) -> Result<String> {
        if let Some(failure) = self.read_failure.borrow().as_ref() {
            return Err(failure.clone());
        }
        let token = self.token.borrow();
        if token.is_empty() {
            return Err(make_error(
                ErrorCode::NotFound,
                "no token is stored",
                self.place.get(),
            ));
        }
        Ok(token.clone())
    }

    fn write(&self, value: &[u8]) -> Status {
        *self.token.borrow_mut() = String::from_utf8_lossy(value).into_owned();
        Ok(())
    }

    fn clear(&self) -> Status {
        self.clears.set(self.clears.get() + 1);
        if let Some(failure) = self.clear_failure.borrow().as_ref() {
            return Err(failure.clone());
        }
        self.token.borrow_mut().clear();
        Ok(())
    }

    fn r#where(&self) -> &str {
        self.place.get()
    }
}

/// Starts nothing: it keeps what it was asked to run, and says the helper's
/// self-test ended as `exit_code`.
#[derive(Default)]
pub struct FakeLauncher {
    pub runs: RefCell<Vec<SpawnRequest>>,
    pub detached: RefCell<Vec<SpawnRequest>>,
    pub exit_code: Cell<i32>,
}

impl Launcher for FakeLauncher {
    fn run(&self, request: &SpawnRequest) -> Result<SpawnResult> {
        self.runs.borrow_mut().push(request.clone());
        Ok(SpawnResult {
            exit_code: self.exit_code.get(),
            ..SpawnResult::default()
        })
    }

    fn start_detached(&self, request: &SpawnRequest) -> Status {
        self.detached.borrow_mut().push(request.clone());
        Ok(())
    }
}

/// A v1 install in a scratch folder, an empty state folder beside it, and a
/// feed that serves nothing until told to.
pub struct Rig {
    pub scratch: Scratch,
    pub root: PathBuf,
    pub state: PathBuf,
    pub install: Install,
    pub feed_config: FeedConfig,
    pub feed: FakeFeed,
    pub store: FakeStore,
    pub launcher: FakeLauncher,
}

impl Rig {
    pub fn new() -> Self {
        let scratch = Scratch::new();
        let root = scratch.path().join("R");
        let state = scratch.path().join("U");
        copy_tree(&fixtures().join("install").join("v1"), &root);
        let install = Install {
            root: root.clone(),
            target: String::from("win64"),
            kind: InstallKind::Archive,
            version: parse_version("0.9.0").expect("0.9.0 is a version"),
            sequence: V1_SEQUENCE,
            has_installed_record: true,
            ..Install::default()
        };
        let feed_config = FeedConfig {
            repo: String::from("kjarada/Katana"),
            target: String::from("win64"),
            user_agent: String::from("Katana/0.9.0 (win64)"),
            ..FeedConfig::default()
        };
        Self {
            scratch,
            root,
            state,
            install,
            feed_config,
            feed: FakeFeed::default(),
            store: FakeStore::default(),
            launcher: FakeLauncher::default(),
        }
    }

    /// The v2 release on the feed: its signature, its manifest and its payload.
    pub fn serve_v2(&self) {
        let release = fixtures().join("release").join("v2");
        self.feed.serve(
            SIGNATURE_URL,
            bytes_of(&release.join("Katana-update-win64.json.sig")),
        );
        self.feed.serve(
            MANIFEST_URL,
            bytes_of(&release.join("Katana-update-win64.json")),
        );
        self.feed.serve(
            PAYLOAD_URL,
            bytes_of(&release.join("Katana-0.10.0-win64.update.zip")),
        );
    }

    /// 0.10.0 as the helper recorded it: what the v2 release is not newer than.
    pub fn install_v2(&mut self) {
        self.install.version = parse_version("0.10.0").expect("0.10.0 is a version");
        self.install.sequence = V2_SEQUENCE;
    }

    /// What make_update_context makes for `surface`, over the fakes: a state
    /// folder for katana_cli and the window, none for a headless window or
    /// katana_mcp.
    pub fn context(&self, surface: UpdateSurface) -> UpdateContext<'_> {
        let credentials: &dyn CredentialStore = &self.store;
        let launcher: &dyn Launcher = &self.launcher;
        UpdateContext {
            surface,
            install: self.install.clone(),
            trust: Some(test_trust()),
            feed: self.feed_config.clone(),
            fetch: self.feed.fetch(),
            state_directory: if matches!(surface, UpdateSurface::Cli | UpdateSurface::Window) {
                self.state.clone()
            } else {
                PathBuf::new()
            },
            credentials: Some(credentials),
            launcher: Some(launcher),
            now_epoch: NOW,
            ..UpdateContext::default()
        }
    }

    /// katana_cli's context (the C++'s default argument).
    pub fn cli(&self) -> UpdateContext<'_> {
        self.context(UpdateSurface::Cli)
    }

    pub fn stage_directory(&self) -> PathBuf {
        self.state.join(format!("stage-{V2_SEQUENCE}"))
    }
}

impl Default for Rig {
    fn default() -> Self {
        Self::new()
    }
}

/// The v2 release downloaded and ready in the update folder `state`, made by
/// hand: the two records stage() writes last (katana_update's stage) are all
/// that says a stage is complete, and they are the feed's own signed bytes.
/// No file of the release is beside them, and none is looked for by the lines
/// that only say what is staged.
pub fn put_a_complete_stage_of_v2_in(state: &Path) {
    let release = fixtures().join("release").join("v2");
    let stage = state.join(format!("stage-{V2_SEQUENCE}"));
    write_bytes(
        &staged_manifest_path(&stage),
        bytes_of(&release.join("Katana-update-win64.json")),
    );
    write_bytes(
        &staged_signature_path(&stage),
        bytes_of(&release.join("Katana-update-win64.json.sig")),
    );
}

/// The two records of an install in `root` (katana_update's install): a copy
/// that was installed as 0.8.0 and then updated itself to 0.9.0, the older
/// release of the fixtures. install.json names the package's own build beside
/// its target - an earlier one than the fixtures' (8 < 9, and 1780000000 <
/// 1790000000) - and installed.json is what the helper wrote for 0.9.0.
pub fn write_install_records(root: &Path) {
    let records = root.join("share").join("katana");
    write_bytes(
        &records.join("install.json"),
        "{\"schema\":1,\"target\":\"win64\",\"version\":\"0.8.0\",\"sequence\":1780000000}\n",
    );
    write_bytes(
        &records.join("installed.json"),
        "{\"schema\":1,\"product\":\"katana\",\"version\":\"0.9.0\",\"sequence\":1790000000,\
         \"tag\":\"v0.9.0\",\"target\":\"win64\"}\n",
    );
}

/// A program to stand where an install's helper is. A REAL one, although
/// nothing here starts it - the launcher of every test is a fake: a file
/// named .exe that is no program is what Windows answers with a message box
/// the day something does start it, so no test makes one. The C++ took the
/// build's own helper when that was built and this test program otherwise;
/// the Rust takes this test program (docs/rust.md, step 6: the helper of
/// another package is not looked for).
pub fn a_real_program() -> PathBuf {
    std::env::current_exe().expect("this test program has a path")
}

/// The helper of the install in `root`: R/bin/katana_updater(.exe).
pub fn put_a_helper_in(root: &Path) -> PathBuf {
    let helper = installed_program(root, "katana_updater");
    if let Some(parent) = helper.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::copy(a_real_program(), &helper) {
        panic!("the helper could not be put in the scratch install: {error}");
    }
    helper
}

/// What `transaction=` of an APPLY's printed record says: "ready=<v>
/// transaction=<tx>".
pub fn transaction_of(printed: &str) -> String {
    const KEY: &str = " transaction=";
    let Some(at) = printed.find(KEY) else {
        return String::new();
    };
    let from = &printed[at + KEY.len()..];
    let end = from.find(['\r', '\n']).unwrap_or(from.len());
    String::from(&from[..end])
}

/// The lines of U/cleanup.list as paths: what is listed for the sweep
/// (katana_update's state: one absolute path to a line, UTF-8).
pub fn listed_for_the_sweep(state: &Path) -> Vec<PathBuf> {
    let list = state.join("cleanup.list");
    if !list.exists() {
        return Vec::new();
    }
    lines_of(&String::from_utf8_lossy(&bytes_of(&list)))
        .into_iter()
        .map(path_from_utf8)
        .collect()
}

/// The folders U/run-<tx> in `state`: one for each helper copy a preflight
/// made (katana_update's state).
pub fn run_folders_in(state: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(state) else {
        return found;
    };
    for entry in entries.flatten() {
        if generic(Path::new(&entry.file_name())).starts_with("run-") {
            found.push(entry.path());
        }
    }
    found
}
