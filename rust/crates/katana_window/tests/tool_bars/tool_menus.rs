//! The twins of tests/qt_widgets/tools/test_tool_menus.cpp: the menus, toolbars and command-line
//! words generated from the tool catalogue. Its cases that build the view workspace (starting a
//! tool in the active plan view, Esc and Enter there, text typed over the drawing, a view closed
//! or turned while its tool runs) are WS's.

mod tool_menus {
    use std::collections::BTreeMap;

    use katana_cad::interactive_tool::tool_catalog;
    use katana_window::drive::{Menu, MenuItem, ToolBar, ToolBarItem};
    use katana_window::icons::is_named_icon;
    use katana_window::tools::{
        BarMenu, ToolActions, ToolMenuTargets, fill_tool_menus, has_tool_icon, tool_id_for_command,
        with_mnemonic,
    };

    /// Every action a menu shows, its submenus' included, by object name, with how many times
    /// each appears.
    fn count_actions(menu: &Menu, seen: &mut BTreeMap<String, i32>) {
        for item in &menu.items {
            match item {
                MenuItem::Action(action) => *seen.entry(action.name.clone()).or_insert(0) += 1,
                MenuItem::Menu(submenu) => count_actions(submenu, seen),
                MenuItem::Section(_) | MenuItem::Separator => {}
            }
        }
    }

    /// The first item of `menu`, past the section title each group opens with: its object name
    /// and its text.
    fn first_item(menu: &Menu) -> Option<(String, String)> {
        menu.items.iter().find_map(|item| match item {
            MenuItem::Action(action) => Some((action.name.clone(), action.text.clone())),
            MenuItem::Menu(submenu) => Some((submenu.name.clone(), submenu.title.clone())),
            MenuItem::Section(_) | MenuItem::Separator => None,
        })
    }

    /// The submenu named `name` anywhere in `menu`.
    fn submenu<'a>(menu: &'a Menu, name: &str) -> Option<&'a Menu> {
        menu.items.iter().find_map(|item| match item {
            MenuItem::Menu(submenu) if submenu.name == name => Some(submenu),
            MenuItem::Menu(submenu) => self::submenu(submenu, name),
            _ => None,
        })
    }

    /// One menu and one toolbar per category the catalogue has.
    struct Menus {
        targets: ToolMenuTargets,
        actions: ToolActions,
    }

    fn menus() -> Menus {
        let mut targets = ToolMenuTargets::default();
        for info in tool_catalog().all() {
            let category = info.category.clone();
            targets
                .menus
                .entry(category.clone())
                .or_insert_with(|| Menu::new(&format!("{category}Menu"), &category));
            targets
                .tool_bars
                .entry(category.clone())
                .or_insert_with(|| ToolBar::new(&format!("{category}ToolBar"), &category));
        }
        let actions = fill_tool_menus(tool_catalog(), &mut targets);
        Menus { targets, actions }
    }

    #[test]
    fn every_catalogue_tool_is_in_a_menu_exactly_once_named_by_its_id() {
        let built = menus();
        let mut seen = BTreeMap::new();
        for menu in built.targets.menus.values() {
            count_actions(menu, &mut seen);
        }
        assert!(tool_catalog().size() > 0);
        for info in tool_catalog().all() {
            assert_eq!(seen.get(&info.id).copied().unwrap_or(0), 1, "{}", info.id);
        }
        // And nothing in the menus that is not a catalogue tool.
        assert_eq!(seen.len(), tool_catalog().size());
        assert!(built.actions.unplaced().is_empty());
    }

    #[test]
    fn each_tool_is_in_the_menu_of_its_own_category() {
        let built = menus();
        for info in tool_catalog().all() {
            let mut seen = BTreeMap::new();
            count_actions(&built.targets.menus[&info.category], &mut seen);
            assert_eq!(
                seen.get(&info.id).copied().unwrap_or(0),
                1,
                "{} is not in {}",
                info.id,
                info.category
            );
        }
    }

    #[test]
    fn an_action_carries_its_icon_tip_and_aliases_and_starts_its_tool() {
        let mut built = menus();
        let line = built
            .actions
            .action("draw.line")
            .expect("the Line tool has an action")
            .clone();
        assert_eq!(line.name, "draw.line");
        assert!(is_named_icon(line.icon), "{}", line.icon);
        let info = tool_catalog()
            .find("draw.line")
            .expect("Line is catalogued");
        assert_eq!(line.tip, info.tip);
        // The aliases as other CAD programs' tooltips give them: "(LINE, L)".
        let tooltip = built.actions.tooltip("draw.line").unwrap_or("");
        assert!(tooltip.contains("(LINE, L)"), "{tooltip}");

        let started: Vec<String> = built.actions.trigger("draw.line").into_iter().collect();
        assert_eq!(started, ["draw.line"]);
    }

    #[test]
    fn a_circle_variant_is_item_in_the_circle_submenu_named_by_its_variant() {
        // "Circle, 2 Points" is the Circle family's "2 Points".
        let built = menus();
        let family = submenu(&built.targets.menus["Draw"], "toolFamily.Draw.Circle")
            .expect("the Draw menu has the Circle family");
        let two_points = family.items.iter().find_map(|item| match item {
            MenuItem::Action(action) if action.name == "draw.circle.2p" => Some(action),
            _ => None,
        });
        let two_points = two_points.expect("the family holds Circle, 2 Points");
        // Its menu letter aside: "&2 Points".
        assert_eq!(two_points.text.replace('&', ""), "2 Points");
        let tooltip = built.actions.tooltip("draw.circle.2p").unwrap_or("");
        assert!(tooltip.contains("Circle, 2 Points"), "{tooltip}");
        // On the toolbar the family is one button that runs its first variant.
        let bar = &built.targets.tool_bars["Draw"];
        assert!(bar.items.contains(&ToolBarItem::Widget(String::from(
            "toolFamilyButton.Draw.Circle"
        ))));
        let button = built
            .actions
            .flyout("toolFamilyButton.Draw.Circle")
            .expect("the Circle family has a button");
        assert_eq!(button.default.name, "draw.circle");
    }

    #[test]
    fn lines_come_before_curves_in_the_draw_menu() {
        let built = menus();
        let draw = &built.targets.menus["Draw"];
        let (first, _) = first_item(draw).expect("the Draw menu has items");
        assert_eq!(first, "draw.point", "the Lines group, order 10");
        // Under its group's title.
        assert_eq!(draw.items[0], MenuItem::Section(String::from("Lines")));
    }

    #[test]
    fn the_running_tools_action_is_checked_and_none_when_it_ends() {
        let mut built = menus();
        let checked = |actions: &ToolActions, id: &str| {
            actions.action(id).and_then(|action| action.checked) == Some(true)
        };
        built.actions.set_active("modify.trim");
        assert!(checked(&built.actions, "modify.trim"));
        built.actions.set_active("draw.line");
        assert!(checked(&built.actions, "draw.line"));
        assert!(!checked(&built.actions, "modify.trim"));
        built.actions.set_active("");
        assert!(!checked(&built.actions, "draw.line"));
    }

    #[test]
    fn a_category_with_no_menu_is_reported_not_lost() {
        let mut targets = ToolMenuTargets::default();
        targets
            .menus
            .insert(String::from("Draw"), Menu::new("drawMenu", "Draw"));
        let actions = fill_tool_menus(tool_catalog(), &mut targets);
        assert!(
            actions
                .unplaced()
                .iter()
                .any(|category| category == "Modify")
        );
        // Its tools still have actions, for the window to place as it likes.
        assert!(actions.action("modify.move").is_some());
    }

    #[test]
    fn typed_aliases_start_their_tools_in_any_case() {
        assert_eq!(tool_id_for_command("L").as_deref(), Some("draw.line"));
        assert_eq!(tool_id_for_command("LINE").as_deref(), Some("draw.line"));
        assert_eq!(tool_id_for_command("line").as_deref(), Some("draw.line"));
        assert_eq!(tool_id_for_command(" TR ").as_deref(), Some("modify.trim"));
        assert_eq!(
            tool_id_for_command("draw.circle.ttr").as_deref(),
            Some("draw.circle.ttr"),
            "an id is a word too"
        );
        assert_eq!(tool_id_for_command("ZOOM"), None, "a command, not a tool");
        assert_eq!(tool_id_for_command(""), None);
    }

    #[test]
    fn a_toolbar_with_no_menu_still_owns_its_family_drop_downs() {
        // A window may give Draw a toolbar and no menu. The Circle family's drop-down then
        // belongs to the toolbar, so it goes with it rather than being left with no owner.
        let mut targets = ToolMenuTargets::default();
        targets
            .tool_bars
            .insert(String::from("Draw"), ToolBar::new("DrawToolBar", "Draw"));
        let actions = fill_tool_menus(tool_catalog(), &mut targets);
        let button = String::from("toolFamilyButton.Draw.Circle");
        assert!(
            targets.tool_bars["Draw"]
                .items
                .contains(&ToolBarItem::Widget(button.clone()))
        );
        let family = actions
            .family(&button)
            .expect("the Circle family has its button");
        assert!(!family.menu.items.is_empty());
        assert_eq!(family.owner, "DrawToolBar");
        assert!(actions.unplaced().iter().any(|category| category == "Draw"));
    }

    // ---- menus made for a category, menu letters, icons -----------------------------------

    /// The letter after a single '&', upper case; empty for none.
    fn letter_of(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut at = text.find('&');
        while let Some(index) = at {
            if index + 1 >= bytes.len() {
                break;
            }
            if bytes[index + 1] != b'&' {
                return String::from(char::from(bytes[index + 1].to_ascii_uppercase()));
            }
            at = text[index + 2..].find('&').map(|found| found + index + 2);
        }
        String::new()
    }

    /// Every item of `menu` and its submenus has a letter, none shared within one menu; the
    /// offenders are listed.
    fn check_letters(menu: &Menu, problems: &mut Vec<String>) {
        let mut seen: Vec<String> = Vec::new();
        for item in &menu.items {
            let text = match item {
                MenuItem::Action(action) => &action.text,
                MenuItem::Menu(submenu) => &submenu.title,
                MenuItem::Section(_) | MenuItem::Separator => continue,
            };
            let letter = letter_of(text);
            if letter.is_empty() {
                problems.push(format!("{} > {text} has no letter", menu.title));
            } else if seen.contains(&letter) {
                problems.push(format!("{} > {text} repeats {letter}", menu.title));
            }
            seen.push(letter);
            if let MenuItem::Menu(submenu) = item {
                check_letters(submenu, problems);
            }
        }
    }

    #[test]
    fn every_tool_menu_item_has_a_menu_letter_of_its_own() {
        let built = menus();
        let mut problems = Vec::new();
        for menu in built.targets.menus.values() {
            check_letters(menu, &mut problems);
        }
        assert!(problems.is_empty(), "{}", problems.join("; "));
    }

    #[test]
    fn a_menu_letter_prefers_the_start_of_a_word_and_skips_taken_letters() {
        let mut taken = String::from("M");
        assert_eq!(
            with_mnemonic("Match Properties", &mut taken),
            "Match &Properties"
        );
        assert_eq!(taken, "MP");
        // No word's first letter is free: the first free letter anywhere.
        assert_eq!(with_mnemonic("Map", &mut taken), "M&ap");
        // A text with a letter keeps it, and the letter counts as taken.
        assert_eq!(with_mnemonic("&Draw", &mut taken), "&Draw");
        assert_eq!(taken, "MPAD");
        // Nothing free: left alone rather than doubled up.
        let mut every = String::from("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");
        assert_eq!(with_mnemonic("List", &mut every), "List");
    }

    #[test]
    fn a_category_the_window_gave_no_menu_gets_one_beside_the_tool_menus_on_its_bar() {
        let mut targets = ToolMenuTargets {
            menu_bar: Some(vec![
                BarMenu::new("fileMenu", "&File"),
                BarMenu::new("drawMenu", "&Draw"),
                BarMenu::new("modifyMenu", "&Modify"),
                BarMenu::new("annotateMenu", "&Annotate"),
                BarMenu::new("terrainMenu", "&Terrain"),
                BarMenu::new("formatMenu", "F&ormat"),
            ]),
            ..ToolMenuTargets::default()
        };
        targets
            .menus
            .insert(String::from("Draw"), Menu::new("drawMenu", "&Draw"));
        targets
            .menus
            .insert(String::from("Modify"), Menu::new("modifyMenu", "&Modify"));
        targets.menus.insert(
            String::from("Annotate"),
            Menu::new("annotateMenu", "&Annotate"),
        );
        // The surface edit tools' category, given a submenu as the window gives it Terrain >
        // Edit Surface (docs/surface_editing.md): Tools stays the one category with no menu of
        // the window's, and a submenu is not on the bar, so Tools still goes after Annotate.
        targets.menus.insert(
            String::from("Terrain"),
            Menu::new("terrainEditSurfaceMenu", "Edi&t Surface"),
        );
        let actions = fill_tool_menus(tool_catalog(), &mut targets);
        assert!(actions.unplaced().is_empty());
        assert_eq!(actions.made_menus().len(), 1);
        let tools = &actions.made_menus()["Tools"];
        assert_eq!(tools.name, "toolsMenu");
        // After Annotate, before Terrain; T and O are the bar's already, so "Tools" underlines
        // its l.
        let titles: Vec<&str> = targets
            .menu_bar
            .as_ref()
            .expect("the bar is kept")
            .iter()
            .map(|top| top.title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "&File",
                "&Draw",
                "&Modify",
                "&Annotate",
                "Too&ls",
                "&Terrain",
                "F&ormat"
            ]
        );
        // Inquiry, then Select, as GROUP_ORDER has them.
        let mut seen = BTreeMap::new();
        count_actions(tools, &mut seen);
        assert_eq!(seen.get("inquiry.distance"), Some(&1));
        assert_eq!(seen.get("select.quick"), Some(&1));
        let (first, _) = first_item(tools).expect("the Tools menu has items");
        assert_eq!(first, "inquiry.distance");
        // Each group under its title, as a style that draws titles shows them.
        let sections: Vec<&str> = tools
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Section(title) => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(sections, ["Inquiry", "Select"]);
    }

    #[test]
    fn every_catalogue_tool_has_a_painted_icon() {
        for info in tool_catalog().all() {
            assert!(has_tool_icon(&info.id), "{}", info.id);
        }
    }

    #[test]
    fn names_whose_letters_earlier_items_used_up_still_get_one() {
        // Taken first come first served, Stretch, Mirror, Move and Rotate used up every letter
        // of "Trim", and the letters of "Match Properties" went the same way; the matching gives
        // both one, and the first letter where it is free.
        let built = menus();
        let modify: Vec<&str> = built.targets.menus["Modify"]
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action(action) => Some(action.text.as_str()),
                MenuItem::Menu(submenu) => Some(submenu.title.as_str()),
                MenuItem::Section(_) | MenuItem::Separator => None,
            })
            .collect();
        let all = modify.join(" | ");
        assert!(modify.contains(&"&Trim"), "{all}");
        assert!(
            modify
                .iter()
                .any(|text| text.replace('&', "") == "Match Properties"
                    && !letter_of(text).is_empty()),
            "{all}"
        );
        let (_, first) = first_item(&built.targets.menus["Draw"]).expect("Draw has items");
        assert_eq!(first, "&Point");
    }
}
