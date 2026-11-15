//! The process seam of the process module on Linux and macOS (the C++'s
//! src/katana_updater/process_scan_posix.cpp, which the update library compiles everywhere except
//! Windows). Two of its three questions are answered here; the third is NOT WRITTEN YET (task T18:
//! /proc/<pid>/exe and the maps of each process on Linux).
//!
//! ```text
//! current_pid    getpid(). The trait returns a number and has no way to say "not here", so the
//!                only other thing this could do is panic - at whichever program asked for its own
//!                id, on a platform where it has every right to ask.
//! is_alive       kill(pid, 0), which sends nothing and reports whether the process is there. The
//!                trait returns a bool: "false" of a live process is the one answer that would let
//!                a swap start under a running program, so it is the true one or none. A process
//!                that has ended and has not been reaped by its parent yet (a zombie) still counts
//!                as alive; the swap that waits on this is T18's, and so is the choice of what to
//!                do about it.
//! holders_under  REFUSES, with the code Unsupported. Which programs run from an install is the
//!                question the scan answers, and "none" from a function that did not look would be
//!                read as "nothing is running, go on". apply() takes the refusal for what it is:
//!                `scan_failed`, nothing touched. Its context begins `reason=platform`, so that a
//!                caller of running_programs() reads the reason stage() and preflight() give here
//!                (fetch_failure_reason, feed.rs) and not `too_large`, which is what that function
//!                makes of an Unsupported with no token.
//! ```

#![allow(
    unsafe_code,
    reason = "kill(2) has no safe form in the standard library; the one call is confined here"
)]

use crate::process::{ProcessOps, RunningProgram};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};
use std::path::Path;

// pid_t is a 32-bit signed integer on Linux and on macOS (sys/types.h).
type PidT = i32;

// errno's EPERM: 1 on Linux and on macOS (asm-generic/errno-base.h, sys/errno.h).
const EPERM: i32 = 1;

unsafe extern "C" {
    fn kill(pid: PidT, signal: i32) -> i32;
}

struct PosixProcessOps;

impl ProcessOps for PosixProcessOps {
    fn current_pid(&self) -> u64 {
        u64::from(std::process::id())
    }

    fn is_alive(&self, pid: u64) -> bool {
        // 0 is "every process of this group" to kill(), and a number too wide for a pid_t would
        // become another process's id, or a negative one, which kill() reads as a group: neither
        // is put to it. (process.rs says the same of the Windows scan.)
        let Ok(pid) = PidT::try_from(pid) else {
            return false;
        };
        if pid == 0 {
            return false;
        }
        // SAFETY: kill with signal 0 sends nothing; it reads its two integer arguments and
        // touches no memory of ours.
        if unsafe { kill(pid, 0) } == 0 {
            return true;
        }
        // EPERM: the process is there and is another user's. Otherwise (ESRCH) there is none.
        std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
    }

    fn holders_under(&self, _root: &Path) -> Result<Vec<RunningProgram>> {
        Err(make_error(
            ErrorCode::Unsupported,
            "the programs running from an install cannot be listed on this system yet",
            "reason=platform; os_error=0 ProcessOps::holdersUnder",
        ))
    }
}

pub(crate) fn make_system_process_ops() -> Box<dyn ProcessOps> {
    Box::new(PosixProcessOps)
}
