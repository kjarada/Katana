//! GDAL's error handlers and its last error, as safe guards and calls: the
//! one place the crate pushes, pops and receives CPL's error handlers
//! (cpl_error.h). cpl_error_collector.rs and the algorithm bridge build their
//! own records from what is collected here.
//!
//! CPLPushErrorHandlerEx is a per-thread stack, so a collector sees exactly
//! the messages raised on its own thread, however many run at once. A
//! handler pushed on one thread must be popped on that thread, so every guard
//! here is neither Send nor Sync, and is popped by its Drop.
//!
//! The collecting handler is pushed with the address of the guard's records,
//! boxed so that the address holds however the guard moves. The callback is
//! `extern "C"`, catches every panic (none may unwind into GDAL), and reaches
//! the records through a RefCell: GDAL calls it while Rust holds the guard by
//! shared reference, and a message raised while the records are being read
//! (none is: reading calls no GDAL) is dropped rather than aliased.

#![allow(
    unsafe_code,
    reason = "Pushes and pops GDAL's error handlers and receives its callback; every call states why it is sound."
)]

use std::cell::{Ref, RefCell};
use std::ffi::{c_char, c_void};
use std::marker::PhantomData;

use super::cpl;

/// One message GDAL raised: its class (CE_Warning, CE_Failure, CE_Fatal),
/// its error number (a CPLE_ code) and its text, decoded lossily.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ErrorRecord {
    pub(crate) kind: cpl::CplErr,
    pub(crate) number: cpl::CplErrorNum,
    pub(crate) message: String,
}

impl ErrorRecord {
    /// CE_Failure or worse; otherwise a warning.
    pub(crate) fn is_failure(&self) -> bool {
        self.kind >= cpl::CE_FAILURE
    }
}

/// A handler that keeps every warning and failure raised on this thread
/// while it is the top of the thread's stack (debug messages and CE_None are
/// not kept). Popped on drop.
pub(crate) struct CollectingHandler {
    records: Box<RefCell<Vec<ErrorRecord>>>,
    // Not Send, not Sync: the handler is on this thread's stack.
    _thread: PhantomData<*const ()>,
}

impl CollectingHandler {
    pub(crate) fn push() -> Self {
        let handler = Self {
            records: Box::new(RefCell::new(Vec::new())),
            _thread: PhantomData,
        };
        let data: *const RefCell<Vec<ErrorRecord>> = &*handler.records;
        // SAFETY: the user data is the boxed list, which lives as long as the
        // guard, whose Drop pops this handler on this thread before the box
        // is freed; `collect` reads it only as the RefCell it is.
        unsafe { cpl::CPLPushErrorHandlerEx(Some(collect), data.cast_mut().cast::<c_void>()) };
        handler
    }

    /// What was collected so far, in the order GDAL raised it.
    pub(crate) fn records(&self) -> Ref<'_, Vec<ErrorRecord>> {
        self.records.borrow()
    }
}

impl Drop for CollectingHandler {
    fn drop(&mut self) {
        // SAFETY: pops the handler `push` pushed on this thread (the guard
        // cannot leave it); handlers pushed after it were popped by their own
        // guards, which are dropped first.
        unsafe { cpl::CPLPopErrorHandler() };
    }
}

extern "C" fn collect(kind: cpl::CplErr, number: cpl::CplErrorNum, message: *const c_char) {
    if kind == cpl::CE_DEBUG || kind == cpl::CE_NONE {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: GDAL calls this handler only while it is the top of this
        // thread's stack, whose user data is a CollectingHandler's boxed list.
        let data = unsafe { cpl::CPLGetErrorHandlerUserData() };
        if data.is_null() {
            return;
        }
        // SAFETY: the user data is a live RefCell<Vec<ErrorRecord>> (above),
        // read through a shared reference only.
        let records = unsafe { &*data.cast::<RefCell<Vec<ErrorRecord>>>() };
        // SAFETY: GDAL passes NULL or a NUL-terminated message, valid for the call.
        let message = unsafe { cpl::decoded(message) };
        if let Ok(mut records) = records.try_borrow_mut() {
            records.push(ErrorRecord {
                kind,
                number,
                message,
            });
        }
    });
}

/// GDAL's quiet handler, pushed until the guard goes: the C++'s
/// `CPLPushErrorHandler(CPLQuietErrorHandler)` / `CPLPopErrorHandler()` pair.
pub(crate) struct QuietHandler {
    _thread: PhantomData<*const ()>,
}

impl QuietHandler {
    pub(crate) fn push() -> Self {
        // SAFETY: pushes GDAL's own handler, which needs no user data, on
        // this thread's stack; Drop pops it.
        unsafe { cpl::CPLPushErrorHandler(Some(cpl::CPLQuietErrorHandler)) };
        Self {
            _thread: PhantomData,
        }
    }
}

impl Drop for QuietHandler {
    fn drop(&mut self) {
        // SAFETY: pops the handler `push` pushed on this thread.
        unsafe { cpl::CPLPopErrorHandler() };
    }
}

/// The C++'s `QuietErrors`: GDAL's last error cleared, then its quiet handler
/// pushed, so that what a call raises stays here and what it leaves as the
/// last error is its own.
pub(crate) fn quiet_errors() -> QuietHandler {
    reset_last_error();
    QuietHandler::push()
}

/// GDAL's quiet handler as the process-wide one: what a thread raises with
/// no handler of its own pushed (GDAL's own worker threads) goes nowhere.
pub(crate) fn install_quiet_global_handler() {
    // SAFETY: installs GDAL's own handler, which needs no user data; the
    // handler it replaces is not kept.
    unsafe { cpl::CPLSetErrorHandler(Some(cpl::CPLQuietErrorHandler)) };
}

/// `CPLErrorReset()`: this thread's last error cleared.
pub(crate) fn reset_last_error() {
    // SAFETY: no argument; the last error is this thread's.
    unsafe { cpl::CPLErrorReset() };
}

/// `CPLGetLastErrorMsg()`: this thread's last error message, copied; empty
/// when there is none.
pub(crate) fn last_error_message() -> String {
    // SAFETY: GDAL's per-thread message buffer, NUL-terminated, copied before
    // anything else on this thread calls GDAL.
    unsafe { cpl::decoded(cpl::CPLGetLastErrorMsg()) }
}
