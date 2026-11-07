//! The `update` layer: the auto-update engine: feed, signed manifest, plan, journal.
//!
//! The port of src/katana_update and include/katana/update (docs/rust.md, docs/update.md).
//!
//! The seams to the machine - [`FsOps`], [`ProcessOps`], [`Launcher`], [`RegistryOps`],
//! [`CredentialStore`] - are traits, so that the swap, its rollback and its recovery can be
//! driven in-process by tests with fakes. Every method takes `&self`, as one C++ reference is
//! handed to the apply, the journal and the test at once: an implementation that keeps state
//! (a fake's record, [`FaultFs`]'s counters) keeps it in a `Cell` or a `RefCell`.

pub mod command_line;
pub mod credential;
pub mod credential_detail;
#[cfg(windows)]
mod credential_win;
pub mod fault_fs;
pub mod feed;
pub mod fsops;
#[cfg(not(windows))]
mod fsops_posix;
#[cfg(windows)]
mod fsops_win;
pub mod inflate;
pub mod install;
mod json;
pub mod long_path;
pub mod manifest;
pub mod paths;
pub mod process;
pub mod ranges;
mod records;
pub mod registry;
pub mod remove_tree;
pub mod signature;
#[cfg(not(windows))]
mod spawn_posix;
#[cfg(windows)]
mod spawn_win;
pub mod trust;
pub mod version;

pub use credential::{
    CREDENTIAL_TARGET, CredentialStore, TOKEN_ENVIRONMENT_VARIABLE,
    make_environment_credential_store, make_system_credential_store, redact_token,
};
pub use fault_fs::{FaultFs, SimulatedCrash};
pub use feed::{
    FeedConfig, FeedRoute, Fetch, FetchRequest, HopFn, HttpHop, HttpHopReply, ProgressFn,
    ReleaseAsset, StopToken, default_feed_config, fetch_failure_reason, make_fetch,
    parse_release_assets,
};
pub use fsops::{
    EntryInfo, EntryKind, FileLock, FsOps, RETRYABLE_OS_ERRORS, RetryPolicy, is_link_reparse_tag,
    make_system_fs_ops,
};
pub use inflate::{InflateSink, inflate_raw, inflate_raw_to_string};
pub use install::{
    Install, InstallKind, install_record_directory, install_root_of_executable,
    install_root_of_running_program, read_install, update_work_directory,
};
pub use manifest::{
    FileClass, Manifest, ManifestFile, Payload, is_expired, parse_file_class, parse_manifest,
    parse_utc_time,
};
pub use paths::{
    INSTALL_ROOTS, MAX_FILES, MAX_INSTALL_BYTES, RECORD_PATHS, check_journal_directory,
    check_journal_path, check_manifest_path, check_manifest_paths, is_reserved_name, lower_ascii,
};
pub use process::{
    Launcher, ProcessOps, RunningProgram, SpawnRequest, SpawnResult, make_system_launcher,
    process_is_elevated, utf8_arguments,
};
pub use ranges::{
    MergedRange, PARTIAL_DOWNLOADS_REFUSED, RANGE_MAX_LENGTH, RANGE_MERGE_GAP, RangeMember,
    check_range_response, merge_ranges, range_header_value,
};
pub use registry::{RUN_ONCE_NAME, RegistryOps};
pub use signature::{
    PublicKey, SignatureBytes, SignatureLine, check_public_key, key_id_of, parse_public_key_hex,
    parse_signature_line, verify_ed25519,
};
pub use trust::{Trust, TrustedKey, is_newer_than, production_trust, verify_manifest};
pub use version::{Version, parse_version, program_version, program_version_text};
