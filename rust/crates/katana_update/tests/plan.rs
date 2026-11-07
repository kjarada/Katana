//! The planner (plan.rs): which files of a release differ from the installed ones, found by
//! hashing the installed files now. The twin of tests/update/test_plan.cpp.
//!
//! The trees are the install fixtures of tests/update/fixtures (v1 and v2, built by
//! tools/update_make_fixtures.py) copied to a temporary root, and the manifests are the fixture
//! releases' own, written by the producer (tools/update_make_manifest.py) and read here a line a
//! file. The hashes in them are Python's hashlib's; the expectations (which files change, how many
//! bytes) are worked out by hand from the two trees:
//!
//! ```text
//! v1 -> v2   bin/katana.exe changes (207 bytes, 85 compressed, at 44 in the payload)
//!            bin/katana_cli.exe, bin/libcore-1.dll, share/katana/empty.txt and random.bin
//!              are the same bytes
//!            bin/libnew-2.dll (118, 70, at 418), share/katana/help/index.txt (82, 75, at 599)
//!              and share/katana/news.txt (54, 54, at 725) are new
//!            share/katana/readme.txt changes (55, 55, at 2938)
//!            etc/katana/settings.ini is a seed: it differs, and is kept, because it is there
//!            bin/libold-1.dll and share/katana/legacy/old.txt are v1's and not in v2: not the
//!              planner's business
//! so 5 files change, 207 + 118 + 82 + 54 + 55 = 516 bytes, 85 + 70 + 75 + 54 + 55 = 339
//! of them to download, and 2.6 * 516 = 1341.6 -> 1342 bytes of free space are needed.
//! ```

#![allow(
    unsafe_code,
    reason = "the Windows case makes a reparse point through DeviceIoControl directly"
)]

use katana_core::path_text::path_from_utf8;
use katana_core::sha256::{sha256_file_hex, sha256_hex};
use katana_core::{ErrorCode, Result};
use katana_update::fsops::is_link_reparse_tag;
use katana_update::install::Install;
use katana_update::manifest::{FileClass, Manifest, ManifestFile};
use katana_update::paths::{MAX_FILES, MAX_INSTALL_BYTES};
use katana_update::plan::{Plan, plan};
use std::collections::BTreeMap;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MIB: u64 = 1024 * 1024;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/update/fixtures"
);

fn read_file(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn write_file(path: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, bytes);
}

fn describe<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => String::from("ok"),
        Err(error) => error.describe(),
    }
}

// A folder under the system temp folder, removed when it goes out of scope.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "katana_t6_plan_{}_{clock}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("a scratch folder");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// A directory link (a junction on Windows: it needs no privilege; a symlink elsewhere) that is
// taken away again, without touching what it points at, when it goes out of scope.
struct DirectoryLink {
    link: PathBuf,
    made: bool,
}

impl DirectoryLink {
    fn new(link: &Path, target: &Path) -> Self {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        #[cfg(not(any(unix, windows)))]
        let made = {
            let _ = (link, target);
            false
        };
        Self {
            link: link.to_path_buf(),
            made,
        }
    }

    fn made(&self) -> bool {
        self.made
    }
}

impl Drop for DirectoryLink {
    fn drop(&mut self) {
        if self.made {
            // The link itself (remove, not remove_all): a symlink is a file to remove, a junction
            // a folder.
            if std::fs::remove_file(&self.link).is_err() {
                let _ = std::fs::remove_dir(&self.link);
            }
        }
    }
}

// An empty folder made a reparse point whose tag is not a name surrogate, as the folders that
// OneDrive keeps are (0x9000701A there; a real one cannot be made without OneDrive). The tag is a
// Microsoft one that no driver handles, so the system answers for the folder itself (it IS a
// folder with the reparse attribute) and refuses to look below it (error 1920). Taken away again
// by removing the folder, which needs no driver.
#[cfg(windows)]
struct FilterReparseFolder {
    folder: PathBuf,
    made: bool,
}

#[cfg(windows)]
impl FilterReparseFolder {
    fn new(folder: &Path) -> Self {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::CreateFileW;
        use windows_sys::Win32::System::IO::DeviceIoControl;
        // winnt.h, fileapi.h and winioctl.h.
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const OPEN_EXISTING: u32 = 3;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
        // REPARSE_GUID_DATA_BUFFER's header as a Microsoft tag uses it: the tag, the length of
        // the data, a reserved word, then the data.
        #[repr(C)]
        struct Buffer {
            tag: u32,
            data_length: u16,
            reserved: u16,
            data: [u8; 4],
        }
        const _: () = assert!(std::mem::size_of::<Buffer>() == 12);

        let _ = std::fs::create_dir_all(folder);
        let mut made = false;
        let wide: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide` is a NUL-terminated wide path; no security attributes and no template.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            let buffer = Buffer {
                tag: 0x8000_1234,
                data_length: 4,
                reserved: 0,
                data: [0; 4],
            };
            let mut returned = 0u32;
            // SAFETY: `handle` is the folder opened above; `buffer` is a readable struct of the
            // size passed; there is no output buffer and no overlapped structure.
            made = unsafe {
                DeviceIoControl(
                    handle,
                    FSCTL_SET_REPARSE_POINT,
                    (&raw const buffer).cast(),
                    12,
                    std::ptr::null_mut(),
                    0,
                    &raw mut returned,
                    std::ptr::null_mut(),
                )
            } != 0;
            // SAFETY: `handle` is open and is closed once, here.
            unsafe { CloseHandle(handle) };
        }
        Self {
            folder: folder.to_path_buf(),
            made,
        }
    }

    fn made(&self) -> bool {
        self.made
    }
}

#[cfg(windows)]
impl Drop for FilterReparseFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.folder);
    }
}

// ---- the fixtures -------------------------------------------------------------------------------

fn fixture_tree(version: &str) -> PathBuf {
    path_from_utf8(format!("{FIXTURES}/install/{version}"))
}

// Everything under `from`, copied to `to` (std::filesystem::copy with copy_options::recursive).
fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a folder to copy into");
    for entry in std::fs::read_dir(from).expect("a folder to copy") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("its kind").is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("a copy");
        }
    }
}

// A copy of an install fixture at `root`.
fn copy_tree(version: &str, root: &Path) {
    copy_folder(&fixture_tree(version), root);
}

fn class_of(name: &str) -> FileClass {
    match name {
        "program" => FileClass::Program,
        "lib" => FileClass::Lib,
        "seed" => FileClass::Seed,
        _ => FileClass::Data,
    }
}

// The manifest of a fixture release, from the lines the producer wrote: each file is a line
// `{"class":"..","csize":N,["exec":true,]"offset":N,"path":"..","sha256":"..","size":N}`, read
// here by those names (the C++ reads them with one regular expression).
fn manifest_of(release: &str) -> Manifest {
    let file = path_from_utf8(format!(
        "{FIXTURES}/release/{release}/Katana-update-win64.json"
    ));
    let text = String::from_utf8(read_file(&file)).expect("a manifest of UTF-8");
    let mut manifest = Manifest::default();
    for entry in text.split("{\"class\":\"").skip(1) {
        let after = |key: &str| {
            let at = entry.find(key).expect("a member of the line") + key.len();
            &entry[at..]
        };
        let number = |key: &str| -> u64 {
            let digits: String = after(key)
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().expect("a number")
        };
        let quoted = |key: &str| -> String {
            let rest = after(key);
            String::from(&rest[..rest.find('"').expect("a closing quotation mark")])
        };
        manifest.files.push(ManifestFile {
            file_class: class_of(&entry[..entry.find('"').expect("the class")]),
            compressed_size: number("\"csize\":"),
            offset: number("\"offset\":"),
            path: quoted("\"path\":\""),
            sha256: quoted("\"sha256\":\""),
            size: number("\"size\":"),
            ..ManifestFile::default()
        });
    }
    manifest
}

fn install_at(root: &Path) -> Install {
    Install {
        root: root.to_path_buf(),
        ..Install::default()
    }
}

fn plan_of(manifest: &Manifest, root: &Path) -> Result<Plan> {
    plan(manifest, &install_at(root), None)
}

fn plan_stopped(manifest: &Manifest, root: &Path, stop: &AtomicBool) -> Result<Plan> {
    plan(manifest, &install_at(root), Some(stop))
}

fn ok(result: Result<Plan>) -> Plan {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

// The indexes the plan marks as changed.
fn changed_indexes(plan: &Plan) -> Vec<usize> {
    plan.files
        .iter()
        .filter(|file| file.changed)
        .map(|file| file.index)
        .collect()
}

// Every file under `root`: relative path (with '/') -> SHA-256. Not for trees holding links.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, folder: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(folder).expect("a folder") {
            let entry = entry.expect("an entry");
            let kind = entry.file_type().expect("its kind");
            if kind.is_dir() {
                walk(root, &entry.path(), out);
            } else if kind.is_file() {
                let path = entry.path();
                let relative: Vec<String> = path
                    .strip_prefix(root)
                    .expect("under the root")
                    .components()
                    .map(|name| name.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let hash = sha256_file_hex(&path).unwrap_or_else(|_| String::from("unreadable"));
                out.insert(relative.join("/"), hash);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

// A manifest of absent files with the given sizes, for the arithmetic.
fn absent_files(sizes: &[u64]) -> Manifest {
    let mut manifest = Manifest::default();
    for (i, &size) in sizes.iter().enumerate() {
        manifest.files.push(ManifestFile {
            path: format!("bin/f{i}.dll"),
            file_class: FileClass::Lib,
            size,
            compressed_size: 2,
            offset: 100 + 10 * i as u64,
            sha256: "a".repeat(64),
            ..ManifestFile::default()
        });
    }
    manifest
}

// How many units the system counts in a path: bytes on POSIX, UTF-16 units on Windows (the C++'s
// path::native().size()).
fn native_len(path: &Path) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count()
    }
    #[cfg(not(windows))]
    {
        path.as_os_str().len()
    }
}

// How the file system wants a path that may pass the 260 characters of the ordinary calls: on
// Windows with the \\?\ prefix, which is how the library names such a file (long_path.rs); the
// case builds the tree with it and plans it with the ORDINARY spelling, as a command line carries
// it.
fn for_the_system(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let mut prefixed = std::ffi::OsString::from(r"\\?\");
        prefixed.push(path.as_os_str());
        PathBuf::from(prefixed)
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

fn write_deep(path: &Path, bytes: &str) {
    std::fs::write(for_the_system(path), bytes).expect("a file under the deep root");
}

// A root of exactly 250 characters under `top`, built of long names, so that its folder
// share/katana is 263 characters deep and the files in it 270 and more.
fn deep_root_under(top: &Path) -> PathBuf {
    let mut root = top.to_path_buf();
    let mut index = 0u8;
    while native_len(&root) + 61 <= 190 {
        root.push(String::from(char::from(b'b' + index % 20)).repeat(60));
        index += 1;
    }
    let rest = 250 - native_len(&root) - 1;
    root.push("z".repeat(rest));
    root
}

// Takes the deep tree away again, name by name, by the names the system accepts (the same note as
// in the apply's cases).
struct RemoveDeep<'a> {
    top: &'a Path,
    root: &'a Path,
    files: &'a [PathBuf],
}

impl Drop for RemoveDeep<'_> {
    fn drop(&mut self) {
        for file in self.files {
            let _ = std::fs::remove_file(for_the_system(file));
        }
        let mut folder = self.root.join("share").join("katana");
        while native_len(&folder) >= native_len(self.top) {
            let _ = std::fs::remove_dir(for_the_system(&folder));
            let Some(parent) = folder.parent() else {
                break;
            };
            folder = parent.to_path_buf();
        }
    }
}

// What a lock on a file looks like where the case can make one: no sharing on Windows (a virus
// scanner, a running copy), no permission elsewhere. None when this user reads the file anyway.
#[cfg(windows)]
struct Unreadable {
    _held: std::fs::File,
}

#[cfg(windows)]
fn make_unreadable(file: &Path) -> Option<Unreadable> {
    use std::os::windows::fs::OpenOptionsExt;
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(file)
        .expect("the file, held");
    Some(Unreadable { _held: held })
}

#[cfg(unix)]
struct Unreadable {
    file: PathBuf,
}

#[cfg(unix)]
fn make_unreadable(file: &Path) -> Option<Unreadable> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o000)).expect("no permission");
    let unreadable = Unreadable {
        file: file.to_path_buf(),
    };
    if std::fs::File::open(file).is_ok() {
        return None; // dropped: the permissions are put back
    }
    Some(unreadable)
}

#[cfg(unix)]
impl Drop for Unreadable {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.file, std::fs::Permissions::from_mode(0o700));
    }
}

mod plan {
    use super::*;

    // ---- nothing, one, many ---------------------------------------------------------------------

    #[test]
    fn an_install_equal_to_the_manifest_changes_nothing() {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let plan = ok(plan_of(&manifest_of("v1"), temp.path()));
        assert_eq!(plan.total_files, 9);
        assert_eq!(plan.changed_files, 0);
        assert_eq!(plan.changed_bytes, 0);
        assert_eq!(plan.download_bytes, 0);
        assert_eq!(plan.required_free_bytes, 0);
        assert!(plan.ranges.is_empty());
        assert_eq!(plan.files.len(), 9, "one entry per manifest file");
        for (i, file) in plan.files.iter().enumerate() {
            assert_eq!(file.index, i, "in manifest order");
            assert!(!file.changed, "{i}");
        }
    }

    #[test]
    fn the_next_release_over_the_installed_one_changes_exactly_the_files_that_differ_or_are_absent()
    {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let v2 = manifest_of("v2");
        assert_eq!(v2.files.len(), 10);
        let plan = ok(plan_of(&v2, temp.path()));

        // Manifest order: 0 bin/katana.exe, 1 bin/katana_cli.exe, 2 bin/libcore-1.dll,
        // 3 bin/libnew-2.dll, 4 etc/katana/settings.ini, 5 share/katana/empty.txt,
        // 6 share/katana/help/index.txt, 7 share/katana/news.txt, 8 share/katana/random.bin,
        // 9 share/katana/readme.txt.
        assert_eq!(changed_indexes(&plan), vec![0, 3, 6, 7, 9]);
        assert_eq!(plan.total_files, 10);
        assert_eq!(plan.changed_files, 5);
        assert_eq!(plan.changed_bytes, 516);
        assert_eq!(
            plan.download_bytes, 339,
            "compressed sizes: what is fetched"
        );
        assert_eq!(plan.required_free_bytes, 1342);

        // The payload bytes to fetch: the five members lie within 2.1 KB of each other, so one
        // request from the first (44) to the end of the last (2938 + 55 = 2993).
        assert_eq!(plan.ranges.len(), 1);
        assert_eq!(plan.ranges[0].offset, 44);
        assert_eq!(plan.ranges[0].length, 2949);
        assert_eq!(plan.ranges[0].members.len(), 5);
        let expected: [(usize, u64, u64); 5] = [
            (0, 44, 85),
            (3, 418, 70),
            (6, 599, 75),
            (7, 725, 54),
            (9, 2938, 55),
        ];
        for (i, &(index, offset, length)) in expected.iter().enumerate() {
            assert_eq!(plan.ranges[0].members[i].file_index, index, "{i}");
            assert_eq!(plan.ranges[0].members[i].offset, offset, "{i}");
            assert_eq!(plan.ranges[0].members[i].length, length, "{i}");
        }
    }

    #[test]
    fn a_download_is_counted_in_compressed_bytes_and_the_disk_need_in_inflated_ones() {
        // random.bin cannot be compressed: 2048 bytes in 2053 (a stored block and its header).
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let _ = std::fs::remove_file(temp.path().join("share").join("katana").join("random.bin"));
        let plan = ok(plan_of(&manifest_of("v1"), temp.path()));
        assert_eq!(changed_indexes(&plan), vec![7]);
        assert_eq!(plan.changed_bytes, 2048);
        assert_eq!(plan.download_bytes, 2053);
        assert_eq!(
            plan.required_free_bytes, 5325,
            "2.6 * 2048 = 5324.8, rounded up"
        );
    }

    #[test]
    fn files_that_are_not_in_the_tree_at_all_are_all_changed() {
        let temp = TempDir::new(); // an empty root: a first install over nothing
        let plan = ok(plan_of(&manifest_of("v2"), temp.path()));
        assert_eq!(plan.changed_files, 10);
        assert_eq!(
            plan.changed_bytes,
            207 + 144 + 208 + 118 + 59 + 82 + 54 + 2048 + 55
        );
    }

    // ---- the person's files ---------------------------------------------------------------------

    #[test]
    fn a_seed_the_person_edited_is_kept_and_one_that_is_gone_is_written_again() {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let seed = temp.path().join("etc").join("katana").join("settings.ini");
        // As installed (v1's bytes, which v2's manifest does not match): present, so kept.
        let plan = ok(plan_of(&manifest_of("v2"), temp.path()));
        assert!(!plan.files[4].changed);
        // Edited into something else entirely, or emptied: still the person's.
        write_file(&seed, "# my settings\nunits = feet\n");
        let plan = ok(plan_of(&manifest_of("v2"), temp.path()));
        assert!(!plan.files[4].changed);
        write_file(&seed, "");
        let plan = ok(plan_of(&manifest_of("v2"), temp.path()));
        assert!(!plan.files[4].changed);
        // Deleted: written (59 bytes in the release).
        let _ = std::fs::remove_file(&seed);
        let plan = ok(plan_of(&manifest_of("v2"), temp.path()));
        assert!(plan.files[4].changed);
        assert_eq!(plan.changed_files, 6);
        assert_eq!(plan.changed_bytes, 516 + 59);
    }

    #[test]
    fn an_edited_file_that_is_not_a_seed_is_changed_back_to_the_releases_bytes() {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        write_file(
            &temp.path().join("share").join("katana").join("readme.txt"),
            "edited by hand\n",
        );
        let plan = ok(plan_of(&manifest_of("v1"), temp.path()));
        assert_eq!(changed_indexes(&plan), vec![8]);
        assert_eq!(plan.changed_bytes, 52);
        assert_eq!(plan.required_free_bytes, 136, "2.6 * 52 = 135.2");
    }

    #[test]
    fn files_the_manifest_does_not_list_are_never_touched_nor_mentioned() {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        // The person's own: next to the program, in a folder of the release's, and in a folder of
        // their own.
        write_file(&temp.path().join("bin").join("my_plugin.dll"), "plugin");
        write_file(
            &temp
                .path()
                .join("share")
                .join("katana")
                .join("my_notes.txt"),
            "notes",
        );
        write_file(
            &temp
                .path()
                .join("share")
                .join("katana")
                .join("mine")
                .join("deep.txt"),
            "deep",
        );
        write_file(
            &temp.path().join("etc").join("katana").join("local.ini"),
            "local",
        );
        let before = snapshot(temp.path());
        // v1's ten files (install.json too) and the four above.
        assert_eq!(before.len(), 10 + 4);

        let plan = ok(plan_of(&manifest_of("v2"), temp.path()));
        assert_eq!(
            plan.files.len(),
            10,
            "only the manifest's files are in the plan"
        );
        assert_eq!(
            snapshot(temp.path()),
            before,
            "planning writes, removes and renames nothing"
        );
        // v1's own files that v2 drops are still there too: removing them is the helper's
        // decision.
        assert!(temp.path().join("bin").join("libold-1.dll").exists());
        assert!(
            temp.path()
                .join("share")
                .join("katana")
                .join("legacy")
                .join("old.txt")
                .exists()
        );
    }

    // ---- the hash is taken now ------------------------------------------------------------------

    #[test]
    fn every_kind_of_difference_makes_a_file_changed() {
        let original = read_file(&fixture_tree("v1").join("bin").join("libcore-1.dll"));
        assert_eq!(original.len(), 208);
        let mut flipped = original.clone();
        flipped[100] ^= 1;
        let mut last_flipped = original.clone();
        *last_flipped.last_mut().expect("a byte") ^= 0x80;
        let mut longer = original.clone();
        longer.push(b'x');
        let variants: [(&str, Vec<u8>); 6] = [
            ("one byte flipped", flipped),
            ("the last byte flipped", last_flipped),
            ("one byte missing", original[..original.len() - 1].to_vec()),
            ("one byte more", longer),
            ("empty", Vec::new()),
            (
                "the same length of something else",
                vec![b'z'; original.len()],
            ),
        ];
        for (what, bytes) in variants {
            let temp = TempDir::new();
            copy_tree("v1", temp.path());
            write_file(&temp.path().join("bin").join("libcore-1.dll"), &bytes);
            let plan = plan_of(&manifest_of("v1"), temp.path())
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            assert_eq!(changed_indexes(&plan), vec![2], "{what}");
        }
    }

    #[test]
    fn the_plan_is_never_from_a_cache_so_a_change_that_keeps_the_size_and_the_time_is_seen() {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let file = temp.path().join("bin").join("katana_cli.exe");
        let v1 = manifest_of("v1");
        let plan = ok(plan_of(&v1, temp.path()));
        assert_eq!(plan.changed_files, 0);

        let time = std::fs::metadata(&file)
            .and_then(|metadata| metadata.modified())
            .expect("a modification time");
        let mut bytes = read_file(&file);
        bytes[10] ^= 0x01;
        write_file(&file, &bytes);
        // Same size, same modification time, different bytes.
        std::fs::File::options()
            .write(true)
            .open(&file)
            .and_then(|opened| opened.set_modified(time))
            .expect("the time put back");
        let metadata = std::fs::metadata(&file).expect("the file");
        assert_eq!(metadata.len(), 144);
        assert_eq!(metadata.modified().expect("a modification time"), time);

        let plan = ok(plan_of(&v1, temp.path()));
        assert_eq!(changed_indexes(&plan), vec![1]);
    }

    #[test]
    fn a_file_longer_than_any_block_is_hashed_whole() {
        // 16 MiB + 3 bytes, byte i being i % 251; SHA-256 from Python's hashlib.
        let size: u64 = 16 * MIB + 3;
        let temp = TempDir::new();
        let file = temp.path().join("bin").join("big.bin");
        let pattern: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        write_file(&file, &pattern);
        let mut manifest = absent_files(&[size]);
        manifest.files[0].path = String::from("bin/big.bin");
        manifest.files[0].sha256 =
            String::from("aa4b9bb6fe503123df159e5cef142033e8561b9361dd41f0ea241e15242dbfb6");
        let plan = ok(plan_of(&manifest, temp.path()));
        assert_eq!(plan.changed_files, 0);

        // One byte of the last block changed: seen.
        {
            let mut edit = std::fs::File::options()
                .write(true)
                .open(&file)
                .expect("the file");
            edit.seek(SeekFrom::Start(size - 2)).expect("a place in it");
            edit.write_all(&[0xFF]).expect("a byte"); // never a byte of the pattern, 0 to 250
        }
        let plan = ok(plan_of(&manifest, temp.path()));
        assert_eq!(plan.changed_files, 1);
    }

    // ---- cancellation ---------------------------------------------------------------------------

    #[test]
    fn a_stop_requested_before_the_start_cancels_before_any_file_is_read() {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let stop = AtomicBool::new(true);
        let error = plan_stopped(&manifest_of("v1"), temp.path(), &stop)
            .expect_err("a stop requested before the start");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(error.message, "cancelled");
    }

    #[test]
    fn a_stop_requested_while_files_are_being_hashed_cancels_between_files() {
        // Sixteen files of 8 MiB (zeros, made sparse, 128 MiB to hash): long enough for another
        // thread to ask a moment after the start. The request is made after 20, then 5, then 0 ms:
        // the first that lands mid-way is the case; the last is certain.
        let temp = TempDir::new();
        let mut manifest = Manifest::default();
        for i in 0..16 {
            let entry = ManifestFile {
                path: format!("bin/part{i}.bin"),
                file_class: FileClass::Lib,
                sha256: "0".repeat(64),
                size: 8 * MIB,
                compressed_size: 100,
                ..ManifestFile::default()
            };
            let file = temp.path().join(&entry.path);
            std::fs::create_dir_all(file.parent().expect("bin")).expect("bin");
            std::fs::File::create(&file)
                .and_then(|created| created.set_len(8 * MIB))
                .expect("a sparse file");
            manifest.files.push(entry);
        }
        let mut cancelled = false;
        for delay in [20u64, 5, 0] {
            let stop = AtomicBool::new(false);
            let plan = std::thread::scope(|scope| {
                scope.spawn(|| {
                    std::thread::sleep(Duration::from_millis(delay));
                    stop.store(true, Ordering::Relaxed);
                });
                plan_stopped(&manifest, temp.path(), &stop)
            });
            if let Err(error) = plan {
                assert_eq!(error.code, ErrorCode::InvalidState);
                assert_eq!(error.message, "cancelled");
                cancelled = true;
                break;
            }
        }
        assert!(cancelled);
    }

    // ---- the root -------------------------------------------------------------------------------

    #[test]
    fn a_root_that_is_not_a_folder_is_not_found() {
        let temp = TempDir::new();
        let manifest = manifest_of("v1");
        assert_eq!(
            plan_of(&manifest, &temp.path().join("missing"))
                .expect_err("a missing root")
                .code,
            ErrorCode::NotFound
        );
        write_file(&temp.path().join("a_file"), "x");
        assert_eq!(
            plan_of(&manifest, &temp.path().join("a_file"))
                .expect_err("a file for a root")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            plan_of(&manifest, Path::new("")).expect_err("no root").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn an_install_root_that_is_itself_a_link_is_followed_because_the_link_is_the_installs_own_location()
     {
        // A person who put the program in a folder reached through a link (a junction to another
        // disk) is not refused: only what is BELOW the root is not followed.
        let temp = TempDir::new();
        copy_tree("v1", &temp.path().join("real"));
        let link = DirectoryLink::new(&temp.path().join("linked"), &temp.path().join("real"));
        if !link.made() {
            eprintln!("this machine does not let the test make a directory link");
            return;
        }
        let plan = ok(plan_of(&manifest_of("v1"), &temp.path().join("linked")));
        assert_eq!(plan.changed_files, 0);
    }

    // ---- a manifest that names a place outside the install --------------------------------------

    #[test]
    fn a_path_that_could_leave_the_install_is_refused_and_reads_nothing_outside_it() {
        let temp = TempDir::new();
        copy_tree("v1", &temp.path().join("root"));
        // A file outside the root that has exactly the bytes (and so the hash) the manifest asks
        // for: a plan that followed the path would call it unchanged.
        write_file(&temp.path().join("outside.txt"), "outside");
        let hash = sha256_hex("outside");
        let absolute = temp
            .path()
            .join("outside.txt")
            .to_string_lossy()
            .replace('\\', "/");
        let paths = [
            "../outside.txt",
            "bin/../../outside.txt",
            "bin/..",
            absolute.as_str(),
            "/outside.txt",
            "bin\\..\\..\\outside.txt",
            "bin//x",
            "bin/./x",
            "",
            "bin/",
            "/",
            "bin/x:stream",
            "C:/outside.txt",
            "..",
        ];
        for path in paths {
            let mut manifest = absent_files(&[7]);
            manifest.files[0].path = String::from(path);
            manifest.files[0].sha256 = hash.clone();
            let error =
                plan_of(&manifest, &temp.path().join("root")).expect_err(&format!("\"{path}\""));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "\"{path}\"");
        }
    }

    // ---- links are never followed ---------------------------------------------------------------

    #[test]
    fn a_folder_on_the_way_that_is_a_link_is_refused_by_the_name_of_the_file_it_would_have_reached()
    {
        let temp = TempDir::new();
        copy_tree("v1", &temp.path().join("root"));
        // share/ replaced by a link to a folder elsewhere that holds the very same files.
        copy_tree("v1", &temp.path().join("elsewhere"));
        let _ = std::fs::remove_dir_all(temp.path().join("root").join("share"));
        let link = DirectoryLink::new(
            &temp.path().join("root").join("share"),
            &temp.path().join("elsewhere").join("share"),
        );
        if !link.made() {
            eprintln!("this machine does not let the test make a directory link");
            return;
        }
        let error = plan_of(&manifest_of("v1"), &temp.path().join("root"))
            .expect_err("the files are all there, through the link");
        assert_eq!(error.code, ErrorCode::InvalidState);
        // Manifest order is bin/, etc/, then share/: the first file reached through the link.
        assert_eq!(error.context, "share/katana/empty.txt");
        assert!(error.message.contains("link"), "{}", error.message);
        assert!(error.message.contains("share/katana/empty.txt"));
        // What it pointed at is as it was.
        assert!(
            temp.path()
                .join("elsewhere")
                .join("share")
                .join("katana")
                .join("readme.txt")
                .exists()
        );
    }

    #[test]
    fn a_file_that_is_a_link_is_not_read_and_counts_as_changed_unless_it_is_a_seed() {
        let temp = TempDir::new();
        copy_tree("v1", &temp.path().join("root"));
        write_file(&temp.path().join("elsewhere").join("file.txt"), "x");
        // bin/libcore-1.dll (a lib) and etc/katana/settings.ini (a seed) each replaced by a link.
        let _ = std::fs::remove_file(temp.path().join("root").join("bin").join("libcore-1.dll"));
        let _ = std::fs::remove_file(
            temp.path()
                .join("root")
                .join("etc")
                .join("katana")
                .join("settings.ini"),
        );
        let lib_link = DirectoryLink::new(
            &temp.path().join("root").join("bin").join("libcore-1.dll"),
            &temp.path().join("elsewhere"),
        );
        let seed_link = DirectoryLink::new(
            &temp
                .path()
                .join("root")
                .join("etc")
                .join("katana")
                .join("settings.ini"),
            &temp.path().join("elsewhere"),
        );
        if !lib_link.made() || !seed_link.made() {
            eprintln!("this machine does not let the test make a directory link");
            return;
        }
        let plan = ok(plan_of(&manifest_of("v1"), &temp.path().join("root")));
        assert_eq!(
            changed_indexes(&plan),
            vec![2],
            "the link in a lib's place is replaced; the seed link is kept"
        );
        assert!(
            temp.path().join("elsewhere").join("file.txt").exists(),
            "what it points at is untouched"
        );
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_reparse_point_that_is_not_a_link_is_not_refused_as_one() {
        // OneDrive's folders are `Directory, ReparsePoint` and hold ordinary files; an install
        // unpacked into a synced folder must not be refused as "reached through a link".
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let _ = std::fs::remove_dir_all(temp.path().join("share"));
        let placeholder = FilterReparseFolder::new(&temp.path().join("share"));
        if !placeholder.made() {
            eprintln!("this machine does not let the test make a reparse point");
            return;
        }
        let error = plan_of(&manifest_of("v1"), temp.path())
            .expect_err("the system will not look below the fake placeholder");
        // Not "reached through a link" (InvalidState): the folder was entered, and it was the
        // system that refused to go on.
        assert_eq!(
            error.code,
            ErrorCode::FileImportFailure,
            "{}",
            error.describe()
        );
        assert!(!error.message.contains("link"), "{}", error.message);
        assert!(error.context.contains("os_error=1920"), "{}", error.context);
    }

    #[test]
    fn a_file_where_a_folder_should_be_and_a_folder_where_a_file_should_be_are_refused_by_name() {
        {
            let temp = TempDir::new();
            copy_tree("v1", temp.path());
            let _ = std::fs::remove_dir_all(temp.path().join("share"));
            write_file(&temp.path().join("share"), "not a folder");
            let error =
                plan_of(&manifest_of("v1"), temp.path()).expect_err("a file where share/ goes");
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert_eq!(error.context, "share/katana/empty.txt");
        }
        {
            let temp = TempDir::new();
            copy_tree("v1", temp.path());
            let _ = std::fs::remove_file(temp.path().join("bin").join("katana.exe"));
            std::fs::create_dir(temp.path().join("bin").join("katana.exe")).expect("a folder");
            let error = plan_of(&manifest_of("v1"), temp.path())
                .expect_err("a folder where bin/katana.exe goes");
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert_eq!(error.context, "bin/katana.exe");
        }
    }

    #[test]
    fn a_file_that_cannot_be_read_is_a_plan_error_that_names_it() {
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let file = temp.path().join("bin").join("libcore-1.dll");
        // Held open by another program with no sharing, as a virus scanner or a running copy
        // might; elsewhere, no permission.
        let Some(unreadable) = make_unreadable(&file) else {
            eprintln!("this user can read a file with no permissions (root?)");
            return;
        };
        let plan = plan_of(&manifest_of("v1"), temp.path());
        drop(unreadable);
        let error =
            plan.expect_err("an unreadable file is not silently counted as changed or as kept");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
        assert!(
            error.message.contains("bin/libcore-1.dll"),
            "{}",
            error.message
        );
        assert!(
            error.context.starts_with("bin/libcore-1.dll"),
            "{}",
            error.context
        );
    }

    #[test]
    fn a_seed_that_exists_is_never_opened_so_one_that_cannot_be_read_is_kept_and_not_an_error() {
        // The same lock as above, on the seed instead of a lib: the plan must not even try to read
        // it. A planner that hashed the seed and then ignored the answer would fail here, as would
        // one that failed on a seed it could not read.
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        let seed = temp.path().join("etc").join("katana").join("settings.ini");
        let Some(unreadable) = make_unreadable(&seed) else {
            eprintln!("this user can read a file with no permissions (root?)");
            return;
        };
        let plan = plan_of(&manifest_of("v2"), temp.path());
        drop(unreadable);
        let plan = plan.unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            !plan.files[4].changed,
            "index 4 is etc/katana/settings.ini, a seed"
        );
        assert_eq!(
            plan.changed_files, 5,
            "the five files the release changes, and not the seed"
        );
    }

    #[test]
    fn an_install_under_non_ascii_names_is_planned_like_any_other() {
        // UTF-8 in the manifest and in the root, wide characters on the file system (Windows).
        let temp = TempDir::new();
        // a-umlaut (C3 A4) in the root, two CJK characters (E6 97 A5 E6 9C AC) after a space.
        let root_name = "K\u{e4}tana \u{65e5}\u{672c}";
        assert_eq!(
            root_name.len(),
            14,
            "K, 2 bytes, tana, a space, 3 and 3: valid UTF-8, once"
        );
        let root = temp.path().join(path_from_utf8(root_name));
        copy_tree("v1", &root);
        let cafe = "share/katana/caf\u{e9}.txt"; // e acute: 2 bytes, not 4
        let map = "share/katana/\u{5730}\u{56f3}.txt"; // two CJK characters
        assert_eq!(cafe.len(), "share/katana/cafe.txt".len() + 1);
        assert_eq!(map.len(), "share/katana/.txt".len() + 6);
        write_file(&root.join(path_from_utf8(cafe)), "as released");
        write_file(&root.join(path_from_utf8(map)), "edited by hand");

        let mut manifest = manifest_of("v1");
        assert_eq!(manifest.files.len(), 9);
        for (path, bytes) in [(cafe, "as released"), (map, "as released")] {
            let offset = 5000 + manifest.files.len() as u64;
            manifest.files.push(ManifestFile {
                path: String::from(path),
                file_class: FileClass::Data,
                size: bytes.len() as u64,
                compressed_size: bytes.len() as u64,
                offset,
                sha256: sha256_hex(bytes),
                ..ManifestFile::default()
            });
        }
        let plan = ok(plan_of(&manifest, &root));
        assert_eq!(
            changed_indexes(&plan),
            vec![10],
            "the edited file only; the other 10 are kept"
        );
        assert_eq!(plan.changed_bytes, 11);
    }

    #[test]
    fn an_install_under_a_very_deep_folder_is_planned_by_the_names_the_file_system_accepts() {
        // The root is 250 characters: share/katana under it is 263, and every file in that folder
        // is past the 260 of the ordinary calls. A program that has no long-path manifest is told
        // "no such file" for such a name unless it is given the prefixed one; the planner then
        // reports a file that is there and right as absent and changed (a needless download) and
        // cannot see a link at that depth at all.
        let temp = TempDir::new();
        let top = temp.path().join("a".repeat(8));
        if native_len(&top) > 190 {
            eprintln!("the temporary folder is too deep to build a root of 250 characters in it");
            return;
        }
        let root = deep_root_under(&top);
        assert_eq!(native_len(&root), 250);
        let folder = root.join("share").join("katana");
        let made = std::fs::create_dir_all(for_the_system(&folder));
        assert!(made.is_ok(), "{made:?}");

        let kept = "as released\n"; // 12 bytes
        let edited = "edited by hand\n"; // 15 bytes
        let released = "as released, 2\n"; // 15 bytes
        let files = [folder.join("readme.txt"), folder.join("settings.txt")];
        let _remove = RemoveDeep {
            top: &top,
            root: &root,
            files: &files,
        };
        write_deep(&files[0], kept);
        write_deep(&files[1], edited);
        assert!(native_len(&files[0]) > 260, "the point of the case");

        let mut manifest = Manifest::default();
        let mut add = |path: &str, bytes: &str, size: u64| {
            let offset = 100 + 10 * manifest.files.len() as u64;
            manifest.files.push(ManifestFile {
                path: String::from(path),
                file_class: FileClass::Data,
                size,
                compressed_size: 3,
                offset,
                sha256: sha256_hex(bytes),
                ..ManifestFile::default()
            });
        };
        add("share/katana/readme.txt", kept, kept.len() as u64); // 0: the same bytes
        add("share/katana/settings.txt", released, released.len() as u64); // 1: the file has been edited
        add("share/katana/gone.txt", "never there", 11); // 2: absent

        let plan = ok(plan_of(&manifest, &root));
        assert_eq!(
            changed_indexes(&plan),
            vec![1, 2],
            "readme.txt is there and right, even at 270 characters"
        );
        assert_eq!(plan.changed_bytes, 15 + 11);
        // 2.6 * 26 = 67.6, rounded up.
        assert_eq!(plan.required_free_bytes, 68);
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_file_whose_name_differs_only_in_case_is_the_same_file() {
        // Windows finds bin/katana.exe under the name Katana.EXE: the bytes decide, as for any
        // other file, and the name on the disk is left as it is.
        let temp = TempDir::new();
        copy_tree("v1", temp.path());
        std::fs::rename(
            temp.path().join("bin").join("katana.exe"),
            temp.path().join("bin").join("Katana.EXE"),
        )
        .expect("a rename");
        let manifest = manifest_of("v1");
        assert_eq!(manifest.files[0].path, "bin/katana.exe");

        let plan = ok(plan_of(&manifest, temp.path()));
        assert_eq!(plan.changed_files, 0);
        write_file(&temp.path().join("bin").join("Katana.EXE"), "another build");
        let plan = ok(plan_of(&manifest, temp.path()));
        assert_eq!(changed_indexes(&plan), vec![0]);

        let names: Vec<String> = std::fs::read_dir(temp.path().join("bin"))
            .expect("bin")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|name| name == "Katana.EXE"),
            "the planner renamed nothing"
        );
    }

    #[test]
    fn only_a_reparse_tag_that_redirects_a_path_makes_it_a_link() {
        // A name surrogate (bit 29 of the tag) is a link; every other reparse point stays where it
        // is. The tags are winnt.h's, and the OneDrive one is read from the owner's machine's own
        // folder with `fsutil reparsepoint query`.
        let cases: [(u32, bool, &str); 11] = [
            (0xA000_000C, true, "IO_REPARSE_TAG_SYMLINK"),
            (0xA000_0003, true, "IO_REPARSE_TAG_MOUNT_POINT (a junction)"),
            (0xA000_001D, true, "IO_REPARSE_TAG_LX_SYMLINK (WSL)"),
            (0x9000_001A, false, "IO_REPARSE_TAG_CLOUD"),
            (
                0x9000_701A,
                false,
                "a OneDrive folder placeholder (IO_REPARSE_TAG_CLOUD_7)",
            ),
            (0x9000_F01A, false, "IO_REPARSE_TAG_CLOUD_F"),
            (0x8000_0013, false, "IO_REPARSE_TAG_DEDUP"),
            (0x8000_0017, false, "IO_REPARSE_TAG_WOF (compressed file)"),
            (0x8000_001B, false, "IO_REPARSE_TAG_APPEXECLINK"),
            (0x9000_001C, false, "IO_REPARSE_TAG_PROJFS"),
            (0x0000_0000, false, "no tag"),
        ];
        for (tag, link, what) in cases {
            assert_eq!(is_link_reparse_tag(tag), link, "{what}");
        }
    }

    // ---- the arithmetic of the totals -----------------------------------------------------------

    #[test]
    fn the_free_space_needed_is_two_point_six_times_the_changed_bytes_rounded_up() {
        // 2.6 * n by hand: n * 26 / 10 rounded up.
        let table: [(u64, u64); 12] = [
            (0, 0),                   // nothing to stage
            (1, 3),                   // 2.6
            (2, 6),                   // 5.2
            (3, 8),                   // 7.8
            (4, 11),                  // 10.4
            (5, 13),                  // 13.0 exactly: not 14
            (10, 26),                 // 26.0 exactly
            (1000, 2600),             // 2600.0 exactly
            (1001, 2603),             // 2602.6
            (516, 1342),              // 1341.6
            (MIB, 2726298),           // 2.6 * 1048576 = 2726297.6
            (2147483648, 5583457485), // the 2 GiB an install may hold: 5583457484.8
        ];
        let temp = TempDir::new();
        for (bytes, needed) in table {
            let plan = plan_of(&absent_files(&[bytes]), temp.path())
                .unwrap_or_else(|error| panic!("{bytes}: {}", error.describe()));
            assert_eq!(plan.changed_bytes, bytes);
            assert_eq!(plan.required_free_bytes, needed, "{bytes} bytes");
        }
    }

    #[test]
    fn the_free_space_is_figure_from_the_sum_not_from_each_file() {
        // 1 + 1 + 1 = 3 bytes -> 7.8 -> 8, where three files rounded up one by one would say 9.
        let temp = TempDir::new();
        let plan = ok(plan_of(&absent_files(&[1, 1, 1]), temp.path()));
        assert_eq!(plan.changed_bytes, 3);
        assert_eq!(plan.required_free_bytes, 8);
    }

    #[test]
    fn a_manifest_beyond_the_limits_of_an_install_is_refused_before_any_file_is_read() {
        let temp = TempDir::new();
        // 2 GiB is the most an install may hold (paths.rs); one byte more is refused.
        let at_most = plan_of(&absent_files(&[MAX_INSTALL_BYTES]), temp.path());
        assert!(at_most.is_ok(), "{}", describe(&at_most));
        assert_eq!(
            plan_of(&absent_files(&[MAX_INSTALL_BYTES + 1]), temp.path())
                .expect_err("one byte more")
                .code,
            ErrorCode::InvalidArgument
        );
        // Two files that are each within it and together are not; and a sum that wraps 64 bits.
        assert_eq!(
            plan_of(&absent_files(&[1536 * MIB, 1536 * MIB]), temp.path())
                .expect_err("3 GiB")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            plan_of(&absent_files(&[u64::MAX, 2]), temp.path())
                .expect_err("a sum that wraps")
                .code,
            ErrorCode::InvalidArgument
        );
        // More files than an install may have.
        let mut many = Manifest::default();
        for i in 0..MAX_FILES + 1 {
            many.files.push(ManifestFile {
                path: format!("bin/f{i}"),
                ..ManifestFile::default()
            });
        }
        assert_eq!(
            plan_of(&many, temp.path())
                .expect_err("one file too many")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- the requests ---------------------------------------------------------------------------

    #[test]
    fn only_changed_files_are_fetched_and_files_that_are_kept_between_them_are_not_in_the_range() {
        // v2's own tree with katana.exe (44) and readme.txt (2938) altered: the files between them
        // in the payload are kept, and are not members of the request that crosses them.
        let temp = TempDir::new();
        copy_tree("v2", temp.path());
        write_file(&temp.path().join("bin").join("katana.exe"), "old program");
        write_file(
            &temp.path().join("share").join("katana").join("readme.txt"),
            "old readme",
        );
        let plan = ok(plan_of(&manifest_of("v2"), temp.path()));
        assert_eq!(changed_indexes(&plan), vec![0, 9]);
        assert_eq!(plan.ranges.len(), 1);
        assert_eq!(plan.ranges[0].offset, 44);
        assert_eq!(plan.ranges[0].length, 2949);
        assert_eq!(plan.ranges[0].members.len(), 2);
        assert_eq!(plan.ranges[0].members[0].file_index, 0);
        assert_eq!(plan.ranges[0].members[1].file_index, 9);
    }

    #[test]
    fn changed_files_far_apart_in_the_payload_are_separate_requests() {
        // Offsets 100, 110, then 100 MiB on: two requests (a gap of 100 MiB is not worth
        // bridging).
        let temp = TempDir::new();
        let mut manifest = absent_files(&[10, 10, 10]);
        manifest.files[2].offset = 100 * MIB;
        let plan = ok(plan_of(&manifest, temp.path()));
        assert_eq!(plan.ranges.len(), 2);
        assert_eq!(plan.ranges[0].offset, 100);
        assert_eq!(plan.ranges[0].length, 12); // [100,102) and [110,112): 100..112
        assert_eq!(plan.ranges[0].members.len(), 2);
        assert_eq!(plan.ranges[1].offset, 100 * MIB);
        assert_eq!(plan.ranges[1].length, 2);
    }

    #[test]
    fn a_manifest_with_no_files_is_a_plan_with_nothing_to_do() {
        let temp = TempDir::new();
        let plan = ok(plan_of(&Manifest::default(), temp.path()));
        assert_eq!(plan.total_files, 0);
        assert!(plan.files.is_empty());
        assert!(plan.ranges.is_empty());
    }
}
