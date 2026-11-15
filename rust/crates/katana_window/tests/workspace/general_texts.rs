//! The twin of tests/qt_widgets/customisation/test_general_texts.cpp's case on the Format menu:
//! no item or toolbar button of the customisation workbench names another program, or a file of a
//! format the managers no longer read, in its text or its tips.
//!
//! Not twinned here: NoTextTheStyleManagerShowsNamesAnotherProgram,
//! NoTextTheSymbolLibraryOrItsFileDialogsShowNamesAnotherProgram and
//! NoTextTheSurveyCodeManagerOrItsFileDialogsShowNamesAnotherProgram. They walk every widget's
//! text and tooltip and peek at the file dialogs the managers open; the drive registry records a
//! control's words but not its hover text, and a file dialog is the window's, so there is nothing
//! of the same reach to walk.

mod general_texts {
    use katana_window::customisation::customisation_workbench::{
        CustomisationServices, CustomisationWorkbench,
    };
    use katana_window::drive::Action;
    use katana_window::drive::action::{MenuItem, ToolBarItem};
    use katana_window::shell::Workbench;

    /// Where a text was seen, and the text.
    struct Seen {
        place: String,
        text: String,
    }

    fn add_action(seen: &mut Vec<Seen>, place: &str, action: &Action) {
        for text in [&action.text, &action.tip] {
            if !text.is_empty() {
                seen.push(Seen {
                    place: format!("{place} {}", action.name),
                    text: text.clone(),
                });
            }
        }
    }

    /// The persisted keys keep their spelling (the naming rule's exception), the key ALONE.
    const PERSISTED_KEYS: [&str; 3] = ["12d.name", "12d.style", "12d.breakline"];

    fn expect_none_names_another_program(seen: &[Seen]) {
        let other = format!("{}{}", "1", "2d");
        for each in seen {
            if PERSISTED_KEYS.contains(&each.text.as_str()) {
                continue;
            }
            assert!(
                !each.text.to_lowercase().contains(&other),
                "{}: {}",
                each.place,
                each.text
            );
        }
    }

    fn expect_none_names_an_older_format(seen: &[Seen]) {
        for each in seen {
            let lowered = each.text.to_lowercase();
            for word in [".4d", "mapfile", "survey code file", "style library", "symbol file"] {
                assert!(
                    !lowered.contains(word),
                    "{} says \"{word}\": {}",
                    each.place,
                    each.text
                );
            }
        }
    }

    fn any_contains(seen: &[Seen], part: &str) -> bool {
        seen.iter().any(|each| each.text.contains(part))
    }

    #[test]
    fn no_format_menu_item_or_toolbar_button_names_another_program_in_its_text_or_tips() {
        let bench = CustomisationWorkbench::new(CustomisationServices {
            layers: Some(Action::new("formatLayers", "Layers...")),
            ..CustomisationServices::default()
        });
        let mut seen = Vec::new();
        let menus = bench.menus();
        let mut names = Vec::new();
        for menu in &menus {
            seen.push(Seen {
                place: String::from("menu title"),
                text: menu.title.clone(),
            });
            for item in &menu.items {
                match item {
                    MenuItem::Action(action) => {
                        add_action(&mut seen, "menu", action);
                        names.push(action.name.clone());
                    }
                    MenuItem::Section(title) => seen.push(Seen {
                        place: String::from("menu section"),
                        text: title.clone(),
                    }),
                    MenuItem::Separator | MenuItem::Menu(_) => {}
                }
            }
        }
        for bar in bench.tool_bars() {
            for item in &bar.items {
                if let ToolBarItem::Action(action) = item {
                    add_action(&mut seen, "toolbar", action);
                }
            }
        }
        // The workbench's five, each with its tips; no item loads a file here now.
        for name in [
            "formatStyles",
            "formatSymbols",
            "formatSurveyCodes",
            "formatPurge",
            "formatGlobalModify",
        ] {
            assert!(names.iter().any(|each| each == name), "{name}");
        }
        for name in ["loadCustomisation", "replaceCustomisation"] {
            assert!(!names.iter().any(|each| each == name), "{name}");
        }
        expect_none_names_another_program(&seen);
        expect_none_names_an_older_format(&seen);
        assert!(any_contains(&seen, "the loaded library linestyles"));
        assert!(any_contains(
            &seen,
            "The survey codes of the session's customisation"
        ));
    }
}
