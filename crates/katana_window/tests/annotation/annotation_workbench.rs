//! The cases of tests/qt_widgets/annotation that install the annotation workbench
//! (annotation_workbench): AnnotationManagers.TheScaleBoxSetsAndFollowsTheAnnotationScale of
//! test_annotation_ui.cpp, LeaderManager.TheAnnotateMenuOpensItOnEachTabWithLettersOfItsOwn of
//! test_leader_manager.cpp and TextEditDialog.TheAnnotateMenuOpensItByName of
//! test_text_edit_dialog.cpp. The C++ tests made the workbench over a bare QMainWindow with a
//! Format menu and toolbar of their own; here it is drawn in drive::WorkbenchHost, the stand-in
//! for that window, and its actions triggered by name as QAction::trigger triggers them.

use katana_window::annotation::AnnotationWorkbench;
use katana_window::annotation::annotation_workbench::SCALE_BOX;
use katana_window::drive::{Action, Harness, Menu, WorkbenchHost};
use katana_window::host::RecordingHost;

use crate::support::{interpreter_host, run};

/// The window with the workbench drawn: its menus, its toolbar's scale box, and its dialogs.
fn draw(harness: &mut Harness, window: &mut WorkbenchHost<AnnotationWorkbench>, host: &mut RecordingHost) {
    harness
        .run(|ui| {
            let _ = window.workbench.scale_box_ui(ui, host);
            window.ui(ui, host);
        })
        .unwrap_or_else(|refusal| panic!("{refusal}"));
}

mod annotation_managers {
    use super::*;

    #[test]
    fn the_scale_box_sets_and_follows_the_annotation_scale() {
        let mut host = interpreter_host();
        let mut window = WorkbenchHost::new(AnnotationWorkbench::new(host.document()));
        let mut harness = Harness::new();
        draw(&mut harness, &mut window, &mut host);
        let box_value = |harness: &Harness| {
            harness
                .control(SCALE_BOX)
                .unwrap_or_else(|| panic!("{SCALE_BOX} is drawn"))
                .value
        };
        assert_eq!(box_value(&harness), "1:1000");
        harness.fill(SCALE_BOX, "1:500");
        draw(&mut harness, &mut window, &mut host);
        assert_eq!(host.document().annotation_scale(), 500.0);
        host.document_mut()
            .set_annotation_scale(2000.0)
            .expect("the scale is set");
        draw(&mut harness, &mut window, &mut host);
        assert_eq!(
            box_value(&harness),
            "1:2000",
            "a scale set elsewhere is shown"
        );
        host.document_mut().undo().expect("the undo is done");
        draw(&mut harness, &mut window, &mut host);
        assert_eq!(box_value(&harness), "1:500");

        // The menu offers both managers, found by their object names.
        let names: Vec<String> = window
            .menus()
            .iter()
            .flat_map(|menu| {
                menu.actions()
                    .into_iter()
                    .map(|action| action.name.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(names.iter().any(|name| name == "formatTextStyles"));
        assert!(names.iter().any(|name| name == "formatLabelStyles"));
        let _ = window.workbench.show_text_styles(host.document());
        let _ = window.workbench.show_label_styles(host.document());
        draw(&mut harness, &mut window, &mut host);
        assert!(harness.target_record("textStyleManagerDialog").is_some());
        assert!(harness.target_record("labelStyleManagerDialog").is_some());
    }
}

mod leader_manager {
    use super::*;

    #[test]
    fn the_annotate_menu_opens_it_on_each_tab_with_letters_of_its_own() {
        let mut host = interpreter_host();
        let mut window = WorkbenchHost::new(AnnotationWorkbench::new(host.document()));
        // An Annotate menu whose tools have taken L and A already.
        let annotate = Menu::new("annotateMenu", "&Annotate")
            .action(Action::new("annotateLeaderTool", "&Leader"))
            .action(Action::new("annotateAlignedTool", "&Aligned Dimension"));
        window.workbench.add_leader_actions(&annotate);
        let added = window.workbench.annotate_menu();
        let mut letters: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for action in annotate.actions().into_iter().chain(added.actions()) {
            let at = action
                .text
                .find('&')
                .unwrap_or_else(|| panic!("{} has a menu letter", action.text));
            letters.push(action.text[at + 1..at + 2].to_uppercase());
            names.push(action.name.clone());
        }
        letters.sort();
        letters.dedup();
        assert_eq!(letters.len(), 5, "every item its own letter");
        for name in [
            "annotateLeaders",
            "annotateLeadersForSelection",
            "annotateArrangeLeaders",
        ] {
            assert!(names.iter().any(|listed| listed == name), "{name}");
        }
        // Each item opens the one kept dialog on its own tab.
        let mut harness = Harness::new();
        let mut kept: Option<usize> = None;
        for (name, tab) in [
            ("annotateArrangeLeaders", "Arrange"),
            ("annotateLeadersForSelection", "For Selection"),
            ("annotateLeaders", "Leader"),
        ] {
            window
                .trigger(&mut host, name)
                .unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
            draw(&mut harness, &mut window, &mut host);
            let shown = window
                .workbench
                .leader_manager()
                .map(|dialog| std::ptr::from_ref(dialog) as usize)
                .unwrap_or_else(|| panic!("{name}: no Leaders manager"));
            assert!(
                kept.is_none_or(|before| before == shown),
                "{name}: one kept instance"
            );
            kept = Some(shown);
            assert!(harness.target_record("leaderManagerDialog").is_some(), "{name}");
            assert_eq!(
                harness
                    .control("leaderTabs")
                    .unwrap_or_else(|| panic!("{name}: no tabs"))
                    .value,
                tab,
                "{name}"
            );
        }
        let again = std::ptr::from_ref(window.workbench.show_leaders(&mut host, 2)) as usize;
        assert_eq!(Some(again), kept);
    }
}

mod text_edit_dialog {
    use super::*;

    #[test]
    fn the_annotate_menu_opens_it_by_name() {
        let mut host = interpreter_host();
        let mut window = WorkbenchHost::new(AnnotationWorkbench::new(host.document()));
        let annotate = Menu::new("annotateMenu", "&Annotate");
        let action = window.workbench.add_edit_text_action(&annotate);
        assert_eq!(action.name, "annotateEditText");
        assert_eq!(
            action.dialog.as_deref(),
            Some("textEditDialog"),
            "what the headless --dialog opens it by"
        );
        assert!(
            window
                .workbench
                .annotate_menu()
                .actions()
                .iter()
                .any(|listed| listed.name == action.name)
        );
        run(&mut host, "TEXT 0,0 2.5 old");
        host.document_mut().selection_mut().set(vec![1]);
        host.document().notify_selection_changed();
        window
            .trigger(&mut host, "annotateEditText")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut harness = Harness::new();
        draw(&mut harness, &mut window, &mut host);
        let _ = window.workbench.show_text_edit(host.document());
        draw(&mut harness, &mut window, &mut host);
        assert_eq!(
            window
                .workbench
                .text_edit_dialog()
                .expect("the dialog is made")
                .text_id(),
            1
        );
        harness.target("textEditDialog").fill("textEditText", "new");
        draw(&mut harness, &mut window, &mut host);
        let dialog = window
            .workbench
            .text_edit_dialog()
            .expect("the dialog is made");
        assert!(dialog.apply(&mut host), "{}", dialog.problem());
        assert_eq!(host.lines(), ["TEXTEDIT 1 text=new"]);
    }
}
