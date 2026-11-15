//! GDAL's errors and warnings, collected for one call on the thread it runs
//! on (src/katana_io/cpl_error_collector.hpp), and the quiet handler the
//! C++ pushes around calls whose errors it reports itself.
//!
//! CPLPushErrorHandlerEx is a per-thread stack, so a collector sees exactly
//! the messages of the call on its own thread, however many run at once.
//! Messages raised on threads GDAL starts itself skip it and reach the
//! process-wide CPLQuietErrorHandler the registry installs, which stays: a
//! global handler per call would take other threads' messages too.
//!
//! The Rust shape: a guard. `CplErrorCollector::new` pushes the handler with
//! the address of the guard's message list, boxed so that the address holds
//! however the guard moves, and `Drop` pops it. A handler pushed on one thread
//! must be popped on that thread, so the guard is neither Send nor Sync. The
//! callback is `extern "C"`, catches every panic (none may unwind into GDAL),
//! and reaches the list through a RefCell: GDAL calls it while Rust holds the
//! guard by shared reference.
//!
//! `QuietHandler` is the C++'s `CPLPushErrorHandler(CPLQuietErrorHandler)` /
//! `CPLPopErrorHandler()` pair as a guard; `quiet_errors` is the
//! `QuietErrors` struct formats.cpp and zip_container.cpp each define, which
//! clears the last error first. Each is dropped where the C++ pops.

#![allow(
    unsafe_code,
    reason = "Pushes and pops GDAL's error handlers and receives its callback; every call states why it is sound."
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{c_char, c_void};
use std::marker::PhantomData;

use crate::ffi::cpl;

/// One message GDAL raised.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Message {
    /// CE_Failure or worse; otherwise a warning.
    pub(crate) failure: bool,
    pub(crate) text: String,
}

pub(crate) struct CplErrorCollector {
    messages: Box<RefCell<Vec<Message>>>,
    // Not Send, not Sync: the handler is on this thread's stack.
    _thread: PhantomData<*const ()>,
}

impl CplErrorCollector {
    pub(crate) fn new() -> Self {
        let collector = Self {
            messages: Box::new(RefCell::new(Vec::new())),
            _thread: PhantomData,
        };
        let data: *const RefCell<Vec<Message>> = &*collector.messages;
        // SAFETY: the user data is the boxed list, which lives as long as the
        // collector, whose Drop pops this handler on this thread before the
        // box is freed.
        unsafe { cpl::CPLPushErrorHandlerEx(Some(handler), data.cast_mut().cast::<c_void>()) };
        collector
    }

    pub(crate) fn size(&self) -> usize {
        self.messages.borrow().len()
    }

    /// The first failure raised since message `from`.
    pub(crate) fn first_failure(&self, from: usize) -> Option<Message> {
        self.messages
            .borrow()
            .iter()
            .skip(from)
            .find(|message| message.failure)
            .cloned()
    }

    /// The warnings, each once, with how many times GDAL said it when it said
    /// it more than once: GDAL warns per feature, and a thousand copies of one
    /// sentence tell a person less than the sentence and the count.
    pub(crate) fn warnings(&self) -> Vec<String> {
        let messages = self.messages.borrow();
        let mut order: Vec<&str> = Vec::new();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for message in messages.iter().filter(|message| !message.failure) {
            let count = counts.entry(message.text.as_str()).or_insert(0);
            if *count == 0 {
                order.push(message.text.as_str());
            }
            *count += 1;
        }
        order
            .into_iter()
            .map(|text| match counts[text] {
                1 => text.to_owned(),
                count => format!("{text} ({count} times)"),
            })
            .collect()
    }
}

impl Drop for CplErrorCollector {
    fn drop(&mut self) {
        // SAFETY: pops the handler `new` pushed on this thread (the guard
        // cannot leave it); handlers pushed after it were popped by their own
        // guards, which are dropped first.
        unsafe { cpl::CPLPopErrorHandler() };
    }
}

extern "C" fn handler(kind: cpl::CplErr, _number: cpl::CplErrorNum, message: *const c_char) {
    if kind == cpl::CE_DEBUG || kind == cpl::CE_NONE {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: GDAL calls this handler only while it is the top of this
        // thread's stack, whose user data is the collector's boxed list.
        let data = unsafe { cpl::CPLGetErrorHandlerUserData() };
        if data.is_null() {
            return;
        }
        // SAFETY: the user data is a live RefCell<Vec<Message>> (above).
        let messages = unsafe { &*data.cast::<RefCell<Vec<Message>>>() };
        // SAFETY: GDAL passes NULL or a NUL-terminated message, valid for the call.
        let text = unsafe { cpl::decoded(message) };
        if let Ok(mut messages) = messages.try_borrow_mut() {
            messages.push(Message {
                failure: kind >= cpl::CE_FAILURE,
                text,
            });
        }
    });
}

/// GDAL's quiet handler, pushed until the guard goes.
pub(crate) struct QuietHandler {
    _thread: PhantomData<*const ()>,
}

impl QuietHandler {
    pub(crate) fn push() -> Self {
        // SAFETY: pushes GDAL's own handler on this thread's stack; Drop pops it.
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
    // SAFETY: clears this thread's last error; no argument.
    unsafe { cpl::CPLErrorReset() };
    QuietHandler::push()
}
