//! COPC (docs/interop.md, "Point-cloud level of detail"), the port of
//! src/katana_app/geo/copc_verb.cpp: a point cloud file rewritten whole as a
//! Cloud Optimised Point Cloud, every point kept, so a later IMPORT can ask
//! for a level of detail instead of decimating.
//!
//! ```text
//! COPC <source> <destination.copc.laz>
//! ```
//!
//! Each path one word or quoted. The conversion is the work, written beside
//! its place and moved there by the apply (staged_files.rs): a cancelled COPC
//! leaves no half-made file.

use std::path::Path;

use katana_core::{Error, ErrorCode, Result};
use katana_pointcloud::PointCloudEngine;

use super::bindings::Tokens;
use super::geo_verbs::{Context, Prepared, make_apply, make_work, stop_requested};
use super::staged_files::StagedFiles;
use crate::import_records::{path_from_text, path_text, record_text};

const USAGE: &str = "usage: COPC <source> <destination.copc.laz>";

fn cancelled() -> Error {
    Error::new(ErrorCode::InvalidState, "cancelled")
}

/// The verb, its row in the verb table's I0 block (verb_table.rs).
pub fn prepare_copc(_context: &mut Context<'_>, tokens: &Tokens, _line: &str) -> Result<Prepared> {
    if tokens.size() != 3 {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let source = path_from_text(&tokens[1]);
    let destination = path_from_text(&tokens[2]);
    let title = format!(
        "COPC {}",
        path_text(Path::new(source.file_name().unwrap_or_default()))
    );
    let work = make_work(move |stop, _progress| {
        if stop_requested(stop) {
            return Err(cancelled());
        }
        let staged = StagedFiles::beside(&destination)?;
        if let Err(error) = PointCloudEngine.convert_to_copc(&source, staged.write_to()) {
            return Err(staged.as_target(error));
        }
        if stop_requested(stop) {
            return Err(cancelled());
        }
        let reply = format!(
            "converted source={} file={} format=copc",
            record_text(&path_text(&source)),
            record_text(&path_text(&destination))
        );
        Ok(make_apply(move |_context| {
            staged.place()?;
            Ok(reply)
        }))
    });
    Ok(Prepared {
        title,
        reply: None,
        work: Some(work),
    })
}
