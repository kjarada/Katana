//! The Launcher of the process module on Linux and macOS, which is NOT WRITTEN YET (task T18:
//! posix_spawn or fork and exec, setsid for a detached child). Compiled everywhere except
//! Windows.
//!
//! Until it is written both operations REFUSE, with the code Unsupported, and start nothing. An
//! error is the only honest answer here: run() that returned an exit code would be telling a
//! preflight that a helper passed its self-test, or a swap that the new version started, and
//! start_detached() that returned success would let a window close for a helper that never ran.
//! (The file system of these platforms refuses first in every real path - fsops_posix.rs - so
//! this is reached only by a caller that holds a launcher and no file system: launchApply.)
//!
//! The refusal's context begins `reason=platform`, the token the stage and the preflight refuse
//! under here, so that launchApply's caller reads the same reason with the feed's
//! fetchFailureReason as theirs. Without the token that function reads an Unsupported as
//! `too_large`, a download that was too big, which is nothing a launcher can mean.

use crate::process::{Launcher, SpawnRequest, SpawnResult};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};

fn refusal(operation: &str) -> Error {
    make_error(
        ErrorCode::Unsupported,
        "Katana does not start its update helper on this system: updates are checked for here, \
         and installed by hand",
        format!("reason=platform; os_error=0 Launcher::{operation}"),
    )
}

struct NoLauncher;

impl Launcher for NoLauncher {
    fn run(&self, _request: &SpawnRequest) -> Result<SpawnResult> {
        Err(refusal("run"))
    }

    fn start_detached(&self, _request: &SpawnRequest) -> Status {
        Err(refusal("startDetached"))
    }
}

pub(crate) fn make_system_launcher() -> Box<dyn Launcher> {
    Box::new(NoLauncher)
}

// The elevated half of a split token is a thing of Windows: there is none here, and no update is
// applied here either (`reason=platform`, above).
pub(crate) fn process_is_elevated() -> bool {
    false
}
