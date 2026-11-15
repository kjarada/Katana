//! A file a job writes, put in place only when the job applies
//! (docs/interop.md, "EXPORT and COPC as jobs"), the port of
//! src/katana_app/geo/staged_files.hpp/.cpp.
//!
//! A job's work may not change anything a person can see: a cancelled job
//! never applies, whenever the cancel came (src/katana_qt/jobs.hpp). The
//! writers - GDAL's, the DXF and archive writers, PDAL's COPC - cannot be
//! stopped part way, so EXPORT and COPC write into a folder of their own
//! beside the target, and their apply moves what was written into place. A
//! job cancelled after its write leaves nothing: the folder is removed with
//! the job.
//!
//! Beside the target, not in a scratch folder elsewhere, so the move is a
//! rename on the same volume. A folder, not a scratch name, because a
//! shapefile is several files named by its stem, a writer takes its driver
//! from the extension, and a .12daz names its member after the file.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};

use super::geo_verbs::system_message;
use crate::import_records::path_text;

/// `path.filename()` as the C++ reads it: nothing for a path that ends in a
/// separator (Rust's file_name would give the folder's name).
fn file_name_of(path: &Path) -> Option<&OsStr> {
    let text = path.as_os_str().as_encoded_bytes();
    if text.is_empty() || text.ends_with(b"/") || (cfg!(windows) && text.ends_with(b"\\")) {
        return None;
    }
    path.file_name()
}

/// `path.parent_path()`, or "." when it has none: the folder a bare file
/// name is written in.
fn folder_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// `path.string()`, the native spelling, and `path.generic_string()`, with
/// '/' for the separator.
fn spellings_of(path: &Path) -> (String, String) {
    let native = path.to_string_lossy().into_owned();
    let generic = if cfg!(windows) {
        native.replace('\\', "/")
    } else {
        native.clone()
    };
    (native, generic)
}

#[derive(Debug)]
pub struct StagedFiles {
    folder: PathBuf,
    target: PathBuf,
    write_to: PathBuf,
}

impl StagedFiles {
    /// FileExportFailure when the folder cannot be made: the target's folder
    /// does not exist, or cannot be written.
    pub fn beside(target: &Path) -> Result<Arc<StagedFiles>> {
        let Some(name) = file_name_of(target) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a file to write is named",
                path_text(target),
            ));
        };
        let parent = folder_of(target);
        // One process may stage several at once (two jobs), and two processes
        // may write into one folder: the id and a count keep them apart.
        static MADE: AtomicU64 = AtomicU64::new(0);
        let made = MADE.fetch_add(1, Ordering::Relaxed) + 1;
        let folder = parent.join(format!(".katana-staging-{}-{made}", std::process::id()));
        if let Err(error) = std::fs::create_dir(&folder) {
            // The C++'s create_directory answers false, with no error, for a
            // folder that is already there.
            let why = if error.kind() == std::io::ErrorKind::AlreadyExists && folder.is_dir() {
                String::from("the folder is in the way")
            } else {
                system_message(&error)
            };
            return Err(make_error(
                ErrorCode::FileExportFailure,
                format!("cannot write beside it: {why}"),
                path_text(target),
            ));
        }
        let write_to = folder.join(name);
        Ok(Arc::new(StagedFiles {
            folder,
            target: target.to_path_buf(),
            write_to,
        }))
    }

    /// Where the writer writes: the folder, under the target's own name.
    pub fn write_to(&self) -> &Path {
        &self.write_to
    }

    /// Every file written moved beside the target, each replacing one of the
    /// same name. FileExportFailure naming the file that could not be moved.
    pub fn place(&self) -> Status {
        let parent = folder_of(&self.target);
        let mut written = Vec::new();
        let read = std::fs::read_dir(&self.folder).and_then(|entries| {
            for entry in entries {
                written.push(entry?.path());
            }
            Ok(())
        });
        if let Err(error) = read {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                format!(
                    "what was written cannot be read back: {}",
                    system_message(&error)
                ),
                path_text(&self.folder),
            ));
        }
        for from in &written {
            let to = parent.join(from.file_name().unwrap_or_default());
            let mut moved = std::fs::rename(from, &to);
            if moved.is_err() {
                // A rename onto a file that exists is refused on some
                // systems: the file the export replaces goes first, as a
                // writer asked to replace it would have removed it. The
                // rename's answer is the one that counts.
                let _ = std::fs::remove_file(&to);
                moved = std::fs::rename(from, &to);
            }
            if let Err(error) = moved {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    format!(
                        "cannot put the written file in place: {}",
                        system_message(&error)
                    ),
                    path_text(&to),
                ));
            }
        }
        Ok(())
    }

    /// A writer's failure as the person asked for it: the staging folder's
    /// path, in the message or the context, is the target's. An export
    /// refused for its CRS named ".katana-staging-19705-1/f.kml", a folder no
    /// one made and that is gone by the time they read it.
    pub fn as_target(&self, mut error: Error) -> Error {
        let parent = folder_of(&self.target);
        let (write_native, write_generic) = spellings_of(&self.write_to);
        let (target_native, target_generic) = spellings_of(&self.target);
        let (folder_native, folder_generic) = spellings_of(&self.folder);
        let (parent_native, parent_generic) = spellings_of(&parent);
        // The file first, then the folder alone (a shapefile's .dbf beside
        // it), in each spelling a writer may have used.
        let spellings = [
            (path_text(&self.write_to), path_text(&self.target)),
            (write_native, target_native),
            (write_generic, target_generic),
            (path_text(&self.folder), path_text(&parent)),
            (folder_native, parent_native),
            (folder_generic, parent_generic),
        ];
        for text in [&mut error.message, &mut error.context] {
            for (from, to) in &spellings {
                if !from.is_empty() {
                    *text = text.replace(from.as_str(), to);
                }
            }
        }
        error
    }
}

impl Drop for StagedFiles {
    /// Removes the folder and whatever is still in it.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}
