//! Nothing but the program opens a window (the step-7 brief, "Never open a window"; the plan's
//! sections 4.7 and 7, risk 8): WSLg sets DISPLAY in every WSL process, so a test or a headless run
//! that made a winit window would open it on the owner's desktop. eframe is the crate's only road
//! to winit (and egui-winit), and only the program `katana` - src/bin/katana/main.rs, its desktop
//! path - may name it. This fails naming every other file of the crate that does.

use std::fs;
use std::path::{Path, PathBuf};

mod window_display {
    use super::*;

    /// Every .rs file under `folder`, depth first.
    fn rust_files(folder: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                rust_files(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    #[test]
    fn no_file_but_the_program_names_the_desktop_runner() {
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let program = crate_root
            .join("src")
            .join("bin")
            .join("katana")
            .join("main.rs");
        // Put together, so this file does not name it.
        let needle = ["eframe", "::"].concat();
        let mut files = Vec::new();
        rust_files(&crate_root.join("src"), &mut files);
        rust_files(&crate_root.join("tests"), &mut files);
        rust_files(&crate_root.join("benches"), &mut files);
        rust_files(&crate_root.join("examples"), &mut files);
        assert!(!files.is_empty(), "the crate's sources are found");
        let naming: Vec<String> = files
            .iter()
            .filter(|file| **file != program)
            .filter(|file| {
                fs::read_to_string(file).is_ok_and(|text| text.contains(needle.as_str()))
            })
            .map(|file| format!("{}", file.display()))
            .collect();
        assert!(
            naming.is_empty(),
            "only src/bin/katana/main.rs may name {needle}; these do: {naming:?}"
        );
    }
}
