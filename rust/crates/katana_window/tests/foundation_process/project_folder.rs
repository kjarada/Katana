//! The twins of tests/qt_widgets/test_project_folder.cpp: where File > Open and Save As start
//! (project_folder): the folder of the open project, else the last one used, else Documents - and
//! never the working folder, which is the program's own bin\ for a zip copy and its install folder
//! for the installed one, so that a first project was proposed there and went with the program
//! when its folder was deleted.

use std::path::Path;

use katana_window::project_folder::{home_path, project_dialog_folder};

use crate::process_lock;
use crate::window_support::Scratch;

mod project_folder {
    use super::*;

    fn text(path: &Path) -> String {
        String::from(path.to_str().expect("a scratch folder's path is UTF-8"))
    }

    #[test]
    fn the_open_projects_folder_comes_first_then_the_last_one_then_documents() {
        let _process = process_lock();
        let scratch = Scratch::new("project-folder-order");
        let parent = text(&scratch.folder("parent"));
        let last = text(&scratch.folder("last"));
        let documents = text(&scratch.folder("documents"));

        assert_eq!(project_dialog_folder(&parent, &last, &documents), parent);
        assert_eq!(project_dialog_folder("", &last, &documents), last);
        assert_eq!(project_dialog_folder("", "", &documents), documents);
    }

    #[test]
    fn a_folder_that_has_gone_is_skipped() {
        let _process = process_lock();
        let scratch = Scratch::new("project-folder-gone");
        let real = scratch.folder("real");
        let gone = text(&real.join("deleted since"));
        let real = text(&real);
        assert!(!Path::new(&gone).exists());
        assert_eq!(project_dialog_folder(&gone, &gone, &real), real);
        assert_eq!(project_dialog_folder(&gone, &real, &gone), real);
    }

    #[test]
    fn with_no_folder_to_offer_it_is_the_home_folder_and_never_the_working_one() {
        let _process = process_lock();
        let scratch = Scratch::new("project-folder-working");
        let working = scratch.folder("working");
        let before = std::env::current_dir().expect("the working folder is known");
        std::env::set_current_dir(&working).expect("the working folder moves");
        let gone = text(&working.join("nothing here"));
        let none = project_dialog_folder("", "", "");
        let all_gone = project_dialog_folder(&gone, &gone, &gone);
        std::env::set_current_dir(&before).expect("the working folder moves back");

        assert_eq!(none, home_path());
        assert_eq!(all_gone, home_path());
        assert_ne!(
            Path::new(&none),
            working.as_path(),
            "the dialog would open in the working folder"
        );
    }
}
