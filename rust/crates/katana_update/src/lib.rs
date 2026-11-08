//! The `update` layer: the auto-update engine: feed, signed manifest, plan, journal.
//!
//! The port of src/katana_update and include/katana/update (docs/rust.md, docs/update.md).
//!
//! The seams to the machine - [`FsOps`], [`ProcessOps`], [`Launcher`], [`RegistryOps`],
//! [`CredentialStore`] - are traits, so that the swap, its rollback and its recovery can be
//! driven in-process by tests with fakes. Every method takes `&self`, as one C++ reference is
//! handed to the apply, the journal and the test at once: an implementation that keeps state
//! (a fake's record, [`FaultFs`]'s counters) keeps it in a `Cell` or a `RefCell`.

pub mod apply;
pub mod check;
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
pub mod journal;
pub mod long_path;
pub mod manifest;
pub mod paths;
pub mod plan;
pub mod process;
#[cfg(not(windows))]
mod process_scan_posix;
#[cfg(windows)]
mod process_scan_win;
pub mod ranges;
mod records;
pub mod recover;
#[doc(hidden)]
pub mod recover_internal;
pub mod registry;
#[cfg(windows)]
mod registry_win;
pub mod remove_tree;
pub mod signature;
#[cfg(not(windows))]
mod spawn_posix;
#[cfg(windows)]
mod spawn_win;
pub mod stage;
pub mod state;
pub mod trust;
pub mod version;

pub use apply::{
    APPLIER_PROTOCOL, ApplyOutcome, ApplyRequest, ApplyResult, ApplyServices, ApplyTimings,
    HEALTH_SWITCH, apply, helper_arguments, installed_program, is_valid_transaction_id,
    launch_apply, launch_apply_with, new_transaction_id, parse_helper_arguments, rollback,
};
pub use check::{CheckResult, CheckStatus, apply_update_state, check};
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
pub use journal::{
    Journal, JournalOp, JournalRecord, JournalState, ParsedJournal, crc32, encode_record,
    needs_recovery, parse_journal, parse_journal_op,
};
pub use manifest::{
    FileClass, Manifest, ManifestFile, Payload, is_expired, parse_file_class, parse_manifest,
    parse_utc_time,
};
pub use paths::{
    INSTALL_ROOTS, MAX_FILES, MAX_INSTALL_BYTES, RECORD_PATHS, check_journal_directory,
    check_journal_path, check_manifest_path, check_manifest_paths, is_reserved_name, lower_ascii,
};
pub use plan::{FREE_SPACE_FACTOR, Plan, PlannedFile, plan};
pub use process::{
    Launcher, ProcessOps, RunningProgram, SpawnRequest, SpawnResult, make_system_launcher,
    process_is_elevated, utf8_arguments,
};
pub use process::{make_system_process_ops, running_programs};
pub use ranges::{
    MergedRange, PARTIAL_DOWNLOADS_REFUSED, RANGE_MAX_LENGTH, RANGE_MERGE_GAP, RangeMember,
    check_range_response, merge_ranges, range_header_value,
};
pub use recover::{
    Confirmed, RECOVER_WAIT, Settled, confirm_started, recover, recover_in_process, settle_unproved,
};
pub use registry::make_system_registry_ops;
pub use registry::{RUN_ONCE_NAME, RegistryOps};
pub use signature::{
    PublicKey, SignatureBytes, SignatureLine, check_public_key, key_id_of, parse_public_key_hex,
    parse_signature_line, verify_ed25519,
};
pub use stage::{
    PreparedApply, STAGED_MANIFEST_NAME, STAGED_SIGNATURE_NAME, preflight, preflight_with, stage,
    stage_lock_path, stage_with, staged_manifest_path, staged_signature_path,
};
pub use state::{
    CHECK_INTERVAL, CHECK_JITTER_MAX, LastResult, UpdateState, append_cleanup, check_is_due,
    clear_last_result, health_marker_path, process_cleanup_list, read_last_result, read_state,
    run_path, stage_path, state_directory, write_last_result, write_state,
};
pub use trust::{Trust, TrustedKey, is_newer_than, production_trust, verify_manifest};
pub use version::{Version, parse_version, program_version, program_version_text};
