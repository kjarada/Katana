//! Where File > Open and File > Save As start (docs/desktop.md, "The File menu").
//!
//! A file dialog given a bare name or none starts in the process's WORKING folder: for a zip copy
//! that is the program's own bin\ (Explorer starts a double-clicked exe in its folder), for the
//! installed copy the install folder under %LOCALAPPDATA%\Programs - so the first project of a new
//! user was proposed as bin\untitled.katana, and deleting the folder, as the README says to remove
//! the program, deleted the project with it. The window proposes, in this order, the folder the
//! open project is in, the folder the last project was opened or saved in (kept between sessions),
//! and the person's Documents folder; the first that is there.

use std::path::{Path, PathBuf};

use katana_core::path_text::{environment_variable, path_from_utf8};

/// The first of `project_parent`, `remembered` and `documents` that names a folder that exists;
/// the home folder when none does. Each may be empty. Never the working folder.
pub fn project_dialog_folder(project_parent: &str, remembered: &str, documents: &str) -> String {
    for candidate in [project_parent, remembered, documents] {
        if !candidate.is_empty() && Path::new(candidate).is_dir() {
            return String::from(candidate);
        }
    }
    home_path()
}

/// `QDir::homePath()`: the person's home folder, with '/' between its parts as Qt gives every path;
/// the root of the file system when it cannot be told.
pub fn home_path() -> String {
    let home = home_folder();
    if home.as_os_str().is_empty() {
        return String::from("/");
    }
    let text = String::from_utf8_lossy(&katana_core::path_text::path_to_utf8(&home)).into_owned();
    let mut generic = text.replace('\\', "/");
    // Qt cleans the path: no trailing separator but on a root.
    while generic.len() > 1 && generic.ends_with('/') && !generic.ends_with(":/") {
        generic.pop();
    }
    generic
}

/// The folder `home_path` names, as the platform says it: $HOME on Unix (Qt's
/// QFileSystemEngine::homePath), and on Windows the profile folder (USERPROFILE), else
/// HOMEDRIVE and HOMEPATH, else HOME. Empty when none is set.
fn home_folder() -> PathBuf {
    #[cfg(windows)]
    {
        let profile = environment_variable(Some("USERPROFILE"));
        if !profile.is_empty() {
            return path_from_utf8(&profile);
        }
        let drive = environment_variable(Some("HOMEDRIVE"));
        let path = environment_variable(Some("HOMEPATH"));
        if !drive.is_empty() && !path.is_empty() {
            let mut joined = drive;
            joined.extend_from_slice(&path);
            return path_from_utf8(&joined);
        }
    }
    let home = environment_variable(Some("HOME"));
    if home.is_empty() {
        return PathBuf::new();
    }
    path_from_utf8(&home)
}
