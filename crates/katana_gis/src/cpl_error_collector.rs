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
//! The Rust shape: a guard over ffi::error's collecting handler, which pushes
//! the handler when it is made and pops it on drop, on this thread; neither
//! is Send nor Sync. `QuietHandler` is the C++'s
//! `CPLPushErrorHandler(CPLQuietErrorHandler)` / `CPLPopErrorHandler()` pair
//! as a guard; `quiet_errors` is the `QuietErrors` struct formats.cpp and
//! zip_container.cpp each define, which clears the last error first. Each is
//! dropped where the C++ pops.

use std::collections::BTreeMap;

use crate::ffi::error::CollectingHandler;

pub(crate) use crate::ffi::error::{QuietHandler, quiet_errors};

/// One failure GDAL raised (CE_Failure or worse).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) text: String,
}

pub(crate) struct CplErrorCollector {
    handler: CollectingHandler,
}

impl CplErrorCollector {
    pub(crate) fn new() -> Self {
        Self {
            handler: CollectingHandler::push(),
        }
    }

    pub(crate) fn size(&self) -> usize {
        self.handler.records().len()
    }

    /// The first failure raised since message `from`.
    pub(crate) fn first_failure(&self, from: usize) -> Option<Message> {
        self.handler
            .records()
            .iter()
            .skip(from)
            .find(|record| record.is_failure())
            .map(|record| Message {
                text: record.message.clone(),
            })
    }

    /// The warnings, each once, with how many times GDAL said it when it said
    /// it more than once: GDAL warns per feature, and a thousand copies of one
    /// sentence tell a person less than the sentence and the count.
    pub(crate) fn warnings(&self) -> Vec<String> {
        let records = self.handler.records();
        let mut order: Vec<&str> = Vec::new();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for record in records.iter().filter(|record| !record.is_failure()) {
            let count = counts.entry(record.message.as_str()).or_insert(0);
            if *count == 0 {
                order.push(record.message.as_str());
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
