//! What an install is, as the update engine and the helper see it, and the files that say so.
//! Everything is under the install root R, the directory that holds bin/ and share/ (the folder
//! of the unpacked archive, or the per-user installer's folder):
//!
//! * `R/share/katana/install.json`, what the package is:
//!   `{"schema":1,"target":"win64","version":"0.3.0","sequence":N}`. Written ONLY by the CPack
//!   install (cmake/KatanaPackaging.cmake), never by `cmake --install`, the `bundle` target, or
//!   the build tree. Without it the tree is a DEVELOPMENT tree: APPLY is refused and no automatic
//!   check runs (tools/check_install_rules.cmake asserts this). The target comes from here, never
//!   from the operating system: Windows on ARM running the x64 zip stays win64. `version` and
//!   `sequence` are OPTIONAL. They are THE COPY'S OWN BUILD, apart from anything an update
//!   recorded here: the version the package was built as, and the sequence of the release it
//!   belongs to (the number that release's update manifests carry). A package built outside the
//!   release workflow belongs to no release and names its version alone. A sequence with no
//!   version to belong to does not read (ParseFailure). A record that names neither (one made
//!   before the record carried them, and every test tree that says its target and no more) leaves
//!   the program that asks to stand for the copy: program_version(), and no sequence. Where the
//!   package names its version the asking program is NOT asked as well. The program that reads an
//!   install need not be one of its programs: the helper runs from a copy outside R and may be a
//!   later release's own, and its version is then the one it is about to install, not the
//!   install's.
//! * `R/share/katana/installed.json`, written by the helper at commit:
//!   `{"schema":1,"product":"katana","version":"0.3.0","sequence":N,"tag":"v0.3.0","target":"win64"}`.
//!   Absent on a fresh install: the installed version is then the copy's own (above), the
//!   sequence is 0 (whatever sequence the package names) and `has_installed_record` is false. A
//!   fresh install is NOT offered the release it came from: is_newer_than (trust.rs) ignores the
//!   sequence while there is no record, and the copy counts as the release it was made from. THE
//!   FLOOR AGAINST GOING BACK. Where the record is there it is not believed below the copy's own
//!   build: the installed build is the HIGHER of the two, in the order is_newer_than writes, by
//!   version, and within one version by sequence. The record is the helper's, and nothing else
//!   keeps it true: a newer zip unpacked by hand over a copy that had updated itself leaves it
//!   naming the older build, and read alone it would let an earlier signed release pass for a
//!   newer one and be installed over the programs that are there. So where the copy's own version
//!   is the higher, it is the installed version, and its sequence comes with it where it has one
//!   (a package of a release); where the versions are equal, the higher sequence is the installed
//!   one. A copy's own build with no sequence (a package that names its version alone, or the
//!   asking program) raises the version alone and the sequence stays the record's: such a copy is
//!   offered the release of its own version once more, which changes no file that is that
//!   release's already and writes the records that were stale.
//! * `R/share/katana/installed.manifest.json` and `.sig`, the signed manifest of the installed
//!   version, kept so that a later update removes only files that version put there. Absent on a
//!   fresh install: a first update deletes nothing.
//! * `R/.update/`, the lock and one folder per transaction (journal, new/, old/): the helper's
//!   working place, inside R so that every rename stays on one volume.
//!
//! The four names install.json, installed.json, installed.manifest.json and
//! installed.manifest.json.sig, and the folder .update, are reserved (paths.rs): a manifest can
//! never carry them. They are the literal names the helper writes, not a pattern:
//! installed.json.bak is a file like any other.
//!
//! WHERE R IS, by platform. Windows and Linux: R is the folder that holds bin/ and share/; the
//! programs are in R/bin, the records in R/share/katana. macOS: the bundle is Katana.app; its
//! programs are in Contents/MacOS and everything that is not code (share/katana/install.json
//! included, see KATANA_INSTALL_DATADIR in cmake/KatanaPackaging.cmake) in Contents/Resources. R
//! is Katana.app/Contents/Resources: it holds share/ but not bin/. That is enough, because the
//! macOS manifest is notify-only: R is read for install.json, which says what to ask the feed for,
//! and nothing is ever applied there. [`install_root_of_executable`] is the one function that
//! knows both layouts.

use crate::fsops::path_text;
use crate::json::Json;
use crate::records::{
    MAX_RECORD_BYTES, parse_failure, parse_record, read_optional_text, string_member,
};
use crate::registry::RegistryOps;
use crate::version::{Version, parse_version, program_version};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InstallKind {
    /// The per-user installer put it here (HKCU Software\Jarada\Katana equals R).
    Installer,
    /// An unpacked zip or tar.gz in a folder the user owns.
    Archive,
    /// No install.json: a build tree, a plain `cmake --install`.
    #[default]
    Development,
}

/// "installer", "archive", "development".
pub fn to_string(kind: InstallKind) -> &'static str {
    match kind {
        InstallKind::Installer => "installer",
        InstallKind::Archive => "archive",
        InstallKind::Development => "development",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Install {
    pub root: PathBuf,
    /// "" for Development.
    pub target: String,
    pub kind: InstallKind,
    pub product: String,
    /// The installed build: what a release must be above to be an update (is_newer_than).
    /// `version` is the higher of installed.json's and the copy's own (install.json's, else
    /// program_version()); with no installed.json it is the copy's own, and in a development tree
    /// program_version().
    pub version: Version,
    /// installed.json's, or the PACKAGE's (install.json's) where the package's build is the higher
    /// of the two and names one ("THE FLOOR" above). 0 with no installed.json, also where the
    /// package names a sequence.
    pub sequence: u64,
    /// installed.json was present: `version` and `sequence` are what the helper recorded, each
    /// raised to the copy's own build where that is the higher. It says that the helper's record
    /// is there, whichever build is the higher. False on a fresh install, where `sequence` is a
    /// placeholder (see above and is_newer_than).
    pub has_installed_record: bool,
    pub has_installed_manifest: bool,
}

impl Default for Install {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            target: String::new(),
            kind: InstallKind::Development,
            product: String::from("katana"),
            version: Version::default(),
            sequence: 0,
            has_installed_record: false,
            has_installed_manifest: false,
        }
    }
}

const INSTALL_RECORD: &str = "install.json";
const INSTALLED_RECORD: &str = "installed.json";
const INSTALLED_MANIFEST: &str = "installed.manifest.json";
const INSTALLED_SIGNATURE: &str = "installed.manifest.json.sig";

/// The folder of R's own files: R/share/katana.
pub fn install_record_directory(root: &Path) -> PathBuf {
    root.join("share").join("katana")
}

/// R/.update.
pub fn update_work_directory(root: &Path) -> PathBuf {
    root.join(".update")
}

// The token in install.json becomes part of a URL (feed.rs: Katana-update-<target>.json), so it is
// held to the characters the real ones use, without being a closed list that a new platform would
// have to change.
fn valid_target(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= 32
        && target
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}

// A build of Katana as one of the two records names it: its version, and the sequence of its
// release where the record has one.
#[derive(Clone, Copy, Debug, Default)]
struct RecordedBuild {
    version: Version,
    sequence: Option<u64>,
}

// The record's "version": a string that is a version (version.rs), or the record does not read.
fn version_member(record: &Json, file: &Path) -> Result<Version> {
    let text = string_member(record, "version", true, file, "")?;
    parse_version(&text).map_err(|error| {
        parse_failure(
            format!("\"version\" is not a version: {}", error.describe()),
            file,
        )
    })
}

// The record's "sequence": a whole number, not negative, that fits 64 bits.
fn sequence_member(record: &Json, file: &Path) -> Result<u64> {
    record
        .get("sequence")
        .and_then(Json::as_unsigned)
        .ok_or_else(|| parse_failure("\"sequence\" is not a whole number", file))
}

// What a package says of ITSELF in install.json, beside its target:
//
//   {"schema":1,"target":"win64","version":"0.5.0","sequence":1792312800}
//
// `version` is the version the package was built as, and `sequence` the sequence of the release it
// belongs to: the number the release workflow settles before it builds and writes into that
// release's update manifests. A package built outside the workflow has no release and names its
// version alone. One that names neither says nothing of itself (None): the record as it was before
// it carried them, which every test tree that only says its target still is. A sequence with no
// version to belong to is no record of a build, and does not read.
fn package_build(record: &Json, file: &Path) -> Result<Option<RecordedBuild>> {
    let names_sequence = record.get("sequence").is_some();
    if record.get("version").is_none() {
        if names_sequence {
            return Err(parse_failure(
                "the record has a \"sequence\" and no \"version\" it could belong to",
                file,
            ));
        }
        return Ok(None);
    }
    let version = version_member(record, file)?;
    let sequence = if names_sequence {
        Some(sequence_member(record, file)?)
    } else {
        None
    };
    Ok(Some(RecordedBuild { version, sequence }))
}

// The path without a trailing separator, normalised lexically (std::filesystem's
// lexically_normal, and then the folder itself where the text ended in a separator): "." names are
// dropped, a ".." takes the name before it with it, and one at the root goes. The state module's
// cleanup list compares its lines by the same rule (state.cpp's two copies of it are one here).
pub(crate) fn bare(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(component),
            },
            _ => parts.push(component),
        }
    }
    parts.iter().collect()
}

// Whether two paths name one file or folder that exists: the same device and file number, as
// std::filesystem::equivalent asks the system. False when either cannot be looked at.
#[cfg(unix)]
fn equivalent(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(windows)]
fn equivalent(a: &Path, b: &Path) -> bool {
    match (windows::file_identity(a), windows::file_identity(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(not(any(unix, windows)))]
fn equivalent(_a: &Path, _b: &Path) -> bool {
    false
}

// Whether the folder the installer recorded is the install root: the same folder when both exist
// (case, junctions and trailing separators then do not matter), else the same text.
fn same_folder(recorded: &Path, root: &Path) -> bool {
    equivalent(recorded, root) || bare(recorded) == bare(root)
}

fn is_regular_file(path: &Path) -> bool {
    path.is_file()
}

#[doc(hidden)]
pub mod detail {
    use super::*;

    /// read_install for a program of version `program`: all of it, with the version of the
    /// program that asks handed in instead of taken from this build. Where the package does not
    /// name its own version (package_build) the installed version is the higher of the helper's
    /// record and the program's own, so what read_install answers there depends on the version of
    /// the binary it is linked into. This is the same reading without that: the rule can be shown
    /// for a program below, at and above a record without a build of each, and a record that
    /// names a fixed version can be read by a build of any version. In no public header of the C++
    /// (tests/update/test_state.cpp declares it); read_install is its one caller in the library.
    pub fn read_install_as(
        root: &Path,
        registry: &dyn RegistryOps,
        program: &Version,
    ) -> Result<Install> {
        if root.as_os_str().is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "no install folder was given",
            ));
        }
        let mut install = Install {
            root: root.to_path_buf(),
            ..Install::default()
        };
        let records = install_record_directory(root);

        let install_file = records.join(INSTALL_RECORD);
        let Some(install_text) = read_optional_text(&install_file, MAX_RECORD_BYTES)? else {
            // A development tree: nothing is updated here. What was not installed by the helper
            // counts as the version it was built as.
            install.version = *program;
            return Ok(install);
        };

        // ---- install.json: where this copy came from ---------------------------------------
        let record = parse_record(&install_text, &install_file, 1)?;
        let target = string_member(&record, "target", true, &install_file, "")?;
        if !valid_target(&target) {
            return Err(parse_failure(
                "\"target\" is not a platform token (lower-case letters, digits, '-' and '_')",
                &install_file,
            ));
        }
        install.target = target;

        // THE COPY'S OWN BUILD, apart from anything an update recorded here: what the package
        // says of itself. A package that says nothing (one made before install.json carried it)
        // is stood for by the program that asks: the version it was built as, and no sequence.
        //
        // Where the package does say, the asking program is NOT asked as well, and that is on
        // purpose. The program that reads an install need not be one of its programs: the helper
        // runs from a copy in the per-user folder, and the helper that installs a release may be
        // that release's own (a manifest whose `applier` is above what the installed helper
        // speaks). Its version is then the version it is about to install, and taken for the
        // install's it would make every such release "not newer": a `downgrade` refused on a copy
        // that never updated, and on one whose record carries a later sequence than the
        // release's (a later build of an older line).
        let own = package_build(&record, &install_file)?.unwrap_or(RecordedBuild {
            version: *program,
            sequence: None,
        });

        // The registry decides installer or archive on Windows; elsewhere it has nothing, and a
        // registry that cannot be read is no reason to refuse to update: the difference is only
        // whether Settings > Apps shows the new version.
        install.kind = match registry.installer_location() {
            Ok(Some(location)) if same_folder(&location, root) => InstallKind::Installer,
            _ => InstallKind::Archive,
        };

        // ---- installed.json: the build the helper last put here ----------------------------
        let installed_file = records.join(INSTALLED_RECORD);
        if let Some(installed_text) = read_optional_text(&installed_file, MAX_RECORD_BYTES)? {
            let installed_record = parse_record(&installed_text, &installed_file, 1)?;
            install.version = version_member(&installed_record, &installed_file)?;
            install.sequence = sequence_member(&installed_record, &installed_file)?;
            install.has_installed_record = true;
            // THE FLOOR AGAINST GOING BACK is the higher of the build the helper recorded and the
            // copy's own (`own`, above), as is_newer_than (trust.rs) orders builds: by version,
            // and within one version by sequence. The record is the helper's, written when an
            // update commits, and nothing else keeps it true: a newer zip unpacked by hand over a
            // copy that had updated itself leaves it saying the older build, and read alone it
            // would let an earlier signed release pass for a newer one and be installed over the
            // programs that are there. The zip brings its own install.json, and with it its floor.
            //
            // Both halves of the build are raised where the copy's own has both. A version alone
            // would leave the record's stale sequence under it, and a manifest of that same
            // version is judged by the sequences: the first of two builds of one version would be
            // "newer" than the second build unpacked over a copy of an older version.
            //
            // That is what remains where the copy's own build has no sequence (a package that
            // names its version alone, or none, so that the asking program stands for it): only
            // the version is raised, the sequence stays the record's, and such a copy is offered
            // the release of its own version once more, which changes no file where the files are
            // that release's already and writes the records that were stale.
            // `has_installed_record` says that the helper's record is there, whichever build is
            // the higher.
            match own.version.cmp(&install.version) {
                Ordering::Greater => {
                    install.version = own.version;
                    if let Some(sequence) = own.sequence {
                        install.sequence = sequence;
                    }
                }
                Ordering::Equal => {
                    if let Some(sequence) =
                        own.sequence.filter(|&package| package > install.sequence)
                    {
                        install.sequence = sequence;
                    }
                }
                Ordering::Less => {}
            }
        } else {
            // A fresh install: the copy's own version. The sequence stays 0 and there is no
            // record, so a release of this same version is not an update (is_newer_than),
            // whatever sequence the package names.
            install.version = own.version;
        }

        install.has_installed_manifest = is_regular_file(&records.join(INSTALLED_MANIFEST))
            && is_regular_file(&records.join(INSTALLED_SIGNATURE));
        Ok(install)
    }
}

/// Reads R's records. A missing install.json is not an error (Development); a malformed one is
/// ParseFailure, an unknown schema Unsupported. `kind` asks the registry (RegistryOps) on Windows
/// and is Archive elsewhere.
pub fn read_install(root: &Path, registry: &dyn RegistryOps) -> Result<Install> {
    // The program's own version is this build's (version.rs). One that does not read is a broken
    // build, and is said before any file of the install is looked at.
    let program = program_version()?;
    detail::read_install_as(root, registry, &program)
}

// True when the path's last component is `ascii` in any ASCII case. A name that is not Unicode,
// or that holds a character beyond ASCII, is no such name.
fn name_is(path: &Path, ascii: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.is_ascii() && name.eq_ignore_ascii_case(ascii))
}

/// The install root R for a program whose executable is at `executable` (an absolute path, as the
/// operating system gives it). Looks at the disk, never writes:
///
/// * the executable's folder is named bin (any case) and the folder above it has a directory
///   share: R is the folder above (Windows, Linux, and a build tree, which then has no
///   install.json and so reads as Development);
/// * or the folder is Contents/MacOS of a bundle whose Contents/Resources has a directory share:
///   R is Contents/Resources (macOS, see above);
/// * otherwise NotFound.
///
/// Matching of the names bin, MacOS, Contents and Resources ignores ASCII case, because a path
/// typed or stored in another case still names the same folder on the file systems Windows and
/// macOS use. In the C++ it is one of the finished functions of contract.cpp, with its tests in
/// tests/update/test_contract.cpp.
pub fn install_root_of_executable(executable: &Path) -> Result<PathBuf> {
    let directory = executable.parent().unwrap_or(Path::new(""));

    // Windows, Linux, a build tree: <R>/bin/<program>, with <R>/share beside bin.
    if name_is(directory, "bin")
        && let Some(root) = directory
            .parent()
            .filter(|root| !root.as_os_str().is_empty())
        && root.join("share").is_dir()
    {
        return Ok(root.to_path_buf());
    }
    // macOS: Katana.app/Contents/MacOS/<program>, data in Contents/Resources/share.
    if name_is(directory, "MacOS") {
        let contents = directory.parent().unwrap_or(Path::new(""));
        if name_is(contents, "Contents") {
            let resources = contents.join("Resources");
            if resources.join("share").is_dir() {
                return Ok(resources);
            }
        }
    }
    Err(make_error(
        ErrorCode::NotFound,
        "this program is not inside an install (no bin/ beside share/, and not in Contents/MacOS \
         of an application bundle)",
        path_text(executable),
    ))
}

/// install_root_of_executable of the running program's own executable path (GetModuleFileNameW;
/// /proc/self/exe; _NSGetExecutablePath on macOS).
pub fn install_root_of_running_program() -> Result<PathBuf> {
    install_root_of_executable(&running_executable()?)
}

// std::env::current_exe is GetModuleFileNameW, grown until the path fits.
#[cfg(windows)]
fn running_executable() -> Result<PathBuf> {
    std::env::current_exe().map_err(|error| {
        make_error(
            ErrorCode::NotFound,
            "the program's own path cannot be found",
            format!("os_error={}", error.raw_os_error().unwrap_or(0)),
        )
    })
}

// std::env::current_exe is _NSGetExecutablePath; the C++ resolves its links as well.
#[cfg(target_os = "macos")]
fn running_executable() -> Result<PathBuf> {
    let executable = std::env::current_exe().map_err(|_| {
        Error::new(
            ErrorCode::NotFound,
            "the program's own path cannot be found",
        )
    })?;
    std::fs::canonicalize(&executable).map_err(|error| {
        make_error(
            ErrorCode::NotFound,
            "the program's own path cannot be resolved",
            format!("{error}"),
        )
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn running_executable() -> Result<PathBuf> {
    std::fs::read_link("/proc/self/exe").map_err(|error| {
        make_error(
            ErrorCode::NotFound,
            "the program's own path cannot be found (/proc/self/exe)",
            format!("{error}"),
        )
    })
}

// std::filesystem::equivalent on Windows: the volume and the file index of each, read from a
// handle that may be a folder's (FILE_FLAG_BACKUP_SEMANTICS) and that lets every other process on.
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the file identity is read through the Win32 handle API, confined to this adapter"
)]
mod windows {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
    };

    pub(super) fn file_identity(path: &Path) -> Option<(u32, u32, u32)> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call; no security
        // attributes and no template are passed.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid value of that plain struct.
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `handle` is the open handle above and `information` a writable struct of the
        // type the call fills.
        let read = unsafe { GetFileInformationByHandle(handle, &raw mut information) } != 0;
        // SAFETY: `handle` is open and is closed once, here.
        unsafe { CloseHandle(handle) };
        read.then_some((
            information.dwVolumeSerialNumber,
            information.nFileIndexHigh,
            information.nFileIndexLow,
        ))
    }
}
