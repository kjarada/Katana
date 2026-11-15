//! What the update skeleton itself promises, which no later task owns: the version reaches the
//! library, the test keys are the RFC's and nothing a build pins, and the seams of the contract
//! are traits a test double can implement and the apply services can be made of. The twin of
//! tests/update/test_skeleton.cpp. The behaviour behind each seam is the other test files'; these
//! cases stay true whatever is behind them.

pub mod test_key;

use katana_core::{Result, Status};
use katana_update::apply::ApplyServices;
use katana_update::fault_fs::FaultFs;
use katana_update::fsops::{EntryInfo, FileLock, FsOps};
use katana_update::journal::Journal;
use katana_update::process::{Launcher, ProcessOps, RunningProgram, SpawnRequest, SpawnResult};
use katana_update::registry::RegistryOps;
use katana_update::version::program_version_text;
use std::path::{Path, PathBuf};
use test_key::test_trust;

// The version the project declares, read here as the C++ test is handed it (KATANA_EXPECTED_VERSION
// from project(Katana VERSION ...)), and not from the library under test.
fn expected_version() -> String {
    let cmake = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../CMakeLists.txt"
    ))
    .expect("the checkout's CMakeLists.txt");
    let start = cmake.find("project(Katana").expect("project(Katana ...)");
    let arguments = &cmake[start..start + cmake[start..].find(')').expect("a closing bracket")];
    let mut words = arguments.split_ascii_whitespace();
    while let Some(word) = words.next() {
        if word == "VERSION" {
            return words.next().expect("a version").to_owned();
        }
    }
    panic!("project(Katana ...) names no VERSION");
}

// The seams of the contract, each as a double a test would write: they implement every method and
// nothing else, so a method added to a trait without a thought for its fakes breaks this file's
// build.
struct FakeLock;

impl FileLock for FakeLock {}

struct FakeFs;

impl FsOps for FakeFs {
    fn stat(&self, _path: &Path) -> Result<EntryInfo> {
        Ok(EntryInfo::default())
    }
    fn list_directory(&self, _directory: &Path) -> Result<Vec<PathBuf>> {
        Ok(Vec::new())
    }
    fn read_file(&self, _path: &Path) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
    fn resolve(&self, path: &Path) -> Result<PathBuf> {
        Ok(path.to_path_buf())
    }
    fn free_space(&self, _path: &Path) -> Result<u64> {
        Ok(1u64 << 40)
    }
    fn make_directories(&self, _directory: &Path) -> Status {
        Ok(())
    }
    fn copy_file(&self, _from: &Path, _to: &Path) -> Status {
        Ok(())
    }
    fn rename_file(&self, _from: &Path, _to: &Path) -> Status {
        Ok(())
    }
    fn remove_file(&self, _path: &Path) -> Status {
        Ok(())
    }
    fn remove_empty_directory(&self, _path: &Path) -> Status {
        Ok(())
    }
    fn append_durable(&self, _path: &Path, _bytes: &[u8]) -> Status {
        Ok(())
    }
    fn write_durable(&self, _path: &Path, _bytes: &[u8]) -> Status {
        Ok(())
    }
    fn set_executable(&self, _path: &Path) -> Status {
        Ok(())
    }
    fn sync_directory(&self, _directory: &Path) -> Status {
        Ok(())
    }
    fn try_lock(&self, _path: &Path) -> Result<Box<dyn FileLock>> {
        Ok(Box::new(FakeLock))
    }
}

struct FakeProcesses;

impl ProcessOps for FakeProcesses {
    fn current_pid(&self) -> u64 {
        1
    }
    fn is_alive(&self, _pid: u64) -> bool {
        false
    }
    fn holders_under(&self, _root: &Path) -> Result<Vec<RunningProgram>> {
        Ok(Vec::new())
    }
}

struct FakeLauncher;

impl Launcher for FakeLauncher {
    fn run(&self, _request: &SpawnRequest) -> Result<SpawnResult> {
        Ok(SpawnResult::default())
    }
    fn start_detached(&self, _request: &SpawnRequest) -> Status {
        Ok(())
    }
}

struct FakeRegistry;

impl RegistryOps for FakeRegistry {
    fn installer_location(&self) -> Result<Option<PathBuf>> {
        Ok(None)
    }
    fn set_run_once(&self, _name: &str, _command: &str) -> Status {
        Ok(())
    }
    fn clear_run_once(&self, _name: &str) -> Status {
        Ok(())
    }
    fn set_installed_version(&self, _display_version: &str, _display_name: &str) -> Status {
        Ok(())
    }
}

mod update_skeleton {
    use super::*;

    #[test]
    fn the_program_version_is_the_one_project_writes_and_the_release_workflow_can_read() {
        // project(Katana VERSION ...) reaches the library as KATANA_VERSION (build.rs): katana_cli
        // has no definition of its own, which is why the library carries one.
        assert_eq!(program_version_text(), expected_version());

        // And it has the form release.yml's sed reads, digits and dots (MAJOR.MINOR to
        // MAJOR.MINOR.PATCH.BUILD), worked out here and not by the parser under test.
        let text = program_version_text();
        assert!(!text.is_empty());
        let mut dots = 0;
        let mut digit_before_every_dot = true;
        let mut previous = b'.';
        for &c in text.as_bytes() {
            if c == b'.' {
                dots += 1;
                digit_before_every_dot = digit_before_every_dot && previous.is_ascii_digit();
            } else {
                assert!(c.is_ascii_digit(), "not a digit: {}", char::from(c));
            }
            previous = c;
        }
        assert!(dots >= 1);
        assert!(dots <= 3);
        assert!(digit_before_every_dot);
        assert!(text.as_bytes().last().is_some_and(u8::is_ascii_digit));
    }

    #[test]
    fn the_test_keys_are_rfc8032_vectors_one_and_two_not_keys_any_build_pins() {
        // RFC 8032, section 7.1, TEST 1 and TEST 2, public keys byte by byte as the RFC prints
        // them (and as OpenSSL derives them from the seeds of the test key module).
        const TEST1: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        const TEST2: [u8; 32] = [
            0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a, 0x92, 0xb7, 0x0a, 0xa7, 0x4d, 0x1b,
            0x7e, 0xbc, 0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4, 0x96, 0x8c, 0xc0, 0xcd, 0x55, 0xf1,
            0x2a, 0xf4, 0x66, 0x0c,
        ];

        let trust = test_trust();
        assert!(!trust.placeholder);
        assert_eq!(trust.keys.len(), 2);
        assert_eq!(trust.keys[0].key, TEST1);
        assert_eq!(trust.keys[1].key, TEST2);
        // The key id is the first 16 hex digits of the key, as signature.rs says.
        assert_eq!(trust.keys[0].id, "d75a980182b10ab7");
        assert_eq!(trust.keys[1].id, "3d4017c3e843895a");
    }

    #[test]
    fn the_apply_services_are_made_of_seams_a_test_can_fake() {
        let fs = FakeFs;
        let processes = FakeProcesses;
        let launcher = FakeLauncher;
        let registry = FakeRegistry;

        // The helper's trust is a parameter: here the test key, as in the helper built for the
        // tests.
        let services = ApplyServices::new(&fs, &processes, &launcher, &registry, test_trust());
        assert!(std::ptr::addr_eq(services.fs, &fs));
        assert!(std::ptr::eq(services.trust, test_trust()));
        // The design's waits, which a test shortens by passing its own timings.
        assert_eq!(services.timings.wait_for_parent.as_millis(), 60_000);
        assert_eq!(services.timings.wait_for_holders.as_millis(), 30_000);
        assert_eq!(services.timings.health_wait.as_millis(), 60_000);
    }

    #[test]
    fn the_fault_injector_and_the_journal_wrap_the_same_seam_the_apply_uses() {
        let fs = FakeFs;
        // Both take the FsOps they are given and keep it: the fault loops put a FaultFs between
        // the apply and the disk, and the journal writes through whatever it is handed, so a crash
        // is seen at every journal write too.
        let faults = FaultFs::new(&fs);
        let journal = Journal::new(&faults, PathBuf::from("journal"));
        assert_eq!(journal.path(), Path::new("journal"));
    }
}
