//! Help > Command Reference and Help > Keyboard Shortcuts (command_reference_dialog.rs,
//! keyboard_shortcuts_dialog.rs): how a help text is read into entries, that the reference holds
//! the interpreter's, the sheets', the utilities', the online, the update and the window's verbs and
//! the tools, and the two dialogs driven by their object names.
//!
//! The reference is built from the verbs' own help texts, so what is asserted is that a command a
//! person looks for is found, under the section it belongs to, with the verb a double-click puts on
//! the command line.

use katana_cad::CommandInterpreter;
use katana_window::command_reference_dialog::{
    CommandReferenceDialog, ReferenceEntry, ReferenceSection, command_reference_sections,
    reference_entries, summary_of, window_help_text,
};
use katana_window::drive::Harness;
use katana_window::host::{Clipboard, Host, RecordingHost};
use katana_window::keyboard_shortcuts_dialog::{
    KeyboardShortcutsDialog, ShortcutRow, native_key_text,
};

fn section_named<'a>(sections: &'a [ReferenceSection], name: &str) -> Option<&'a ReferenceSection> {
    sections.iter().find(|section| section.name == name)
}

fn entry_titled<'a>(section: &'a ReferenceSection, title: &str) -> Option<&'a ReferenceEntry> {
    section.entries.iter().find(|entry| entry.title == title)
}

/// The titles of the entries the tree shows, as "Section/Title".
fn shown_titles(dialog: &CommandReferenceDialog) -> Vec<String> {
    let mut titles = Vec::new();
    for (at, section) in dialog.sections().iter().enumerate() {
        for (number, entry) in section.entries.iter().enumerate() {
            if dialog.entry_shown(at, number) {
                titles.push(format!("{}/{}", section.name, entry.title));
            }
        }
    }
    titles
}

/// The first item of the tree titled `title`, its section's index and its own (QTreeWidget's
/// findItems, exactly and recursively).
fn find_item(dialog: &CommandReferenceDialog, title: &str) -> Option<(usize, usize)> {
    dialog
        .sections()
        .iter()
        .enumerate()
        .find_map(|(at, section)| {
            section
                .entries
                .iter()
                .position(|entry| entry.title == title)
                .map(|number| (at, number))
        })
}

/// The dialog drawn in a harness whose acts go to it.
fn reference_shown(dialog: &mut CommandReferenceDialog, host: &mut RecordingHost) -> Harness {
    let mut harness = Harness::new();
    harness.target("commandReferenceDialog");
    harness
        .show(|ui| {
            dialog.ui(ui, &mut *host);
        })
        .expect("the dialog is drawn");
    harness
}

mod command_reference {
    use super::*;

    #[test]
    fn a_help_text_is_read_into_labelled_groups_with_their_continuations() {
        let entries = reference_entries(
            "Points: x,y | @dx,dy\n\nDraw      POINT p | LINE p p\n          CIRCLE centre radius\n\
             Modify    (act on the selection)\nAnnoScale ANNOSCALE [N | 1:N]\n",
        );
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].title, "About");
        assert_eq!(entries[0].text, "Points: x,y | @dx,dy");
        assert_eq!(entries[1].title, "Draw");
        assert_eq!(entries[1].verb, "POINT");
        assert_eq!(
            entries[1].text,
            "Draw      POINT p | LINE p p\n          CIRCLE centre radius"
        );
        assert_eq!(entries[2].title, "Modify");
        assert!(
            entries[2].verb.is_empty(),
            "prose puts nothing on the command line"
        );
        assert_eq!(entries[3].title, "AnnoScale");
        assert_eq!(entries[3].verb, "ANNOSCALE");
    }

    #[test]
    fn command_lines_are_titled_by_their_leading_command_words() {
        let entries = reference_entries(
            "usage: ONLINE IMPORT <provider> <layer> area=view\n\
             SHEETS [LIST]                   every sheet and its views\n\
             SHEET NEW [name] [paper=A3]\n\
             \x20 options  rect=x0,y0,x1,y1\n",
        );
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].title, "ONLINE IMPORT");
        assert_eq!(entries[0].verb, "ONLINE IMPORT");
        assert_eq!(
            entries[0].text,
            "usage: ONLINE IMPORT <provider> <layer> area=view"
        );
        assert_eq!(entries[1].title, "SHEETS");
        assert_eq!(entries[2].title, "SHEET NEW");
        assert!(entries[2].text.ends_with("  options  rect=x0,y0,x1,y1"));
    }

    #[test]
    fn an_empty_help_has_no_entries() {
        assert!(reference_entries("").is_empty());
        assert!(reference_entries("\n\n").is_empty());
    }

    #[test]
    fn the_reference_holds_every_family_of_verbs_and_the_tools() {
        let sections = command_reference_sections();
        for name in [
            "Commands",
            "Sheets",
            "Subsurface utilities",
            "Online data",
            "Window",
            "Survey field files",
            "Updates",
            "Draw tools",
            "Modify tools",
        ] {
            assert!(section_named(&sections, name).is_some(), "{name}");
        }
        // SURVEY READ and IMPORT are the session's, and the window runs them too.
        let survey_files = section_named(&sections, "Survey field files").expect("survey files");
        let survey_verb = entry_titled(survey_files, "Survey").expect("Survey");
        assert_eq!(survey_verb.verb, "SURVEY");
        let commands = section_named(&sections, "Commands").expect("Commands");
        let dim_style = entry_titled(commands, "DimStyle").expect("DimStyle");
        assert_eq!(dim_style.verb, "DIMSTYLE");
        // The annotation verbs come with the interpreter's help.
        assert!(entry_titled(commands, "LabelStyle").is_some());

        let window = section_named(&sections, "Window").expect("Window");
        let copc = entry_titled(window, "COPC").expect("COPC");
        assert_eq!(copc.verb, "COPC");

        let online = section_named(&sections, "Online data").expect("Online data");
        assert!(entry_titled(online, "ONLINE PROVIDERS").is_some());
        assert!(entry_titled(online, "ONLINE IMPORT").is_some());

        let sheets = section_named(&sections, "Sheets").expect("Sheets");
        assert_eq!(sheets.entries[0].title, "About");
        assert!(entry_titled(sheets, "GENERATE").is_some());

        // A tool is found by its name, and a double-click starts it by its first alias.
        let draw = section_named(&sections, "Draw tools").expect("Draw tools");
        let line = entry_titled(draw, "Line").expect("Line");
        assert_eq!(line.verb, "LINE");
        assert!(line.text.contains("id draw.line"));
    }

    #[test]
    fn the_updates_section_is_the_update_verbs_own_help() {
        // The UPDATE family is the session's (katana_app::update_verbs), which keeps its help; the
        // reference shows that text and writes none of its own.
        let sections = command_reference_sections();
        let updates = section_named(&sections, "Updates").expect("Updates");
        assert!(!updates.entries.is_empty());
        // Found under the label the help gives the family, and a double-click puts the verb on the
        // command line.
        let update = entry_titled(updates, "Update").expect("Update");
        assert_eq!(update.verb, "UPDATE");

        let section = updates
            .entries
            .iter()
            .map(|entry| entry.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        for line in [
            "UPDATE CHECK",
            "UPDATE STATUS",
            "UPDATE DOWNLOAD CONFIRM",
            "UPDATE APPLY CONFIRM",
            "UPDATE SKIP <version> | UPDATE SKIP NONE",
            "UPDATE BLOCKED CLEAR",
            "UPDATE SET AUTO ON|OFF",
            "UPDATE TOKEN STATUS | UPDATE TOKEN CLEAR",
        ] {
            assert!(section.contains(line), "{line}");
        }
        // Nothing is written twice: every line of the verbs' help is in the section as the help
        // has it, and the section has no line the help lacks.
        let section_lines: Vec<&str> = section.split('\n').collect();
        let help_lines: Vec<&str> = katana_app::update_verbs::update_help_text()
            .split('\n')
            .filter(|line| !line.is_empty())
            .collect();
        // The family has eight lines, and the help says something of each.
        assert!(help_lines.len() >= 8);
        for line in &help_lines {
            assert!(section_lines.contains(line), "{line}");
        }
        assert_eq!(section_lines.len(), help_lines.len());
    }

    #[test]
    fn the_window_section_names_every_verb_the_window_runs_itself() {
        // What the window's dispatcher takes before the interpreter: the list the typed HELP adds
        // to the interpreter's. UPDATE is one of them.
        let help = window_help_text();
        for verb in [
            "SCRIPT",
            "IMPORT",
            "EXPORT",
            "INFO <file>",
            "REFS",
            "COPC",
            "PLOTSHEETS",
            "PLOT <file.pdf>",
            "SNAPSHOT",
            "GRID",
            "SNAP",
            "ONLINE",
            "UTILITY",
            "UPDATE",
            "QUIT",
            "LABELSTYLE",
        ] {
            assert!(help.contains(verb), "{verb}");
        }
        // It is a row of its own, found by its label, which sends the reader to the section that
        // has every line's usage.
        let rows = reference_entries(help);
        let row = rows
            .iter()
            .find(|row| row.title == "Update")
            .expect("the Update row");
        assert_eq!(row.verb, "UPDATE");
        assert!(row.text.contains("UPDATE CHECK"));
        assert!(row.text.contains("BLOCKED CLEAR"));
        assert!(row.text.contains("under Updates"), "{}", row.text);
        // ZOOM went to the interpreter with VIEWS: the window no longer runs it itself, and the
        // interpreter's help says both. So did CUSTOMISE: said once, by the interpreter - the
        // typed HELP prints both texts.
        assert!(!help.contains("ZOOM (Z)"));
        assert!(!help.contains("CUSTOMISE"));
        let commands = CommandInterpreter::help_text();
        for verb in [
            "VIEWS [LIST]",
            "VIEWS LINK",
            "VIEWS UNLINK",
            "ZOOM (Z)",
            "CENTRE x,y [SCALE",
            "Customise CUSTOMISE [REPLACE] <file> [<file>...]",
        ] {
            assert!(commands.contains(verb), "{verb}");
        }
    }
}

mod command_reference_dialog {
    use super::*;

    #[test]
    fn searching_finds_dim_style_copc_and_online() {
        let mut host = RecordingHost::new();
        let mut dialog = CommandReferenceDialog::new(command_reference_sections());
        assert_eq!(dialog.object_name(), "commandReferenceDialog");
        let all = dialog.shown_entries();
        assert!(all > 50);

        let mut harness = reference_shown(&mut dialog, &mut host);
        harness
            .fill("commandReferenceSearch", "dimstyle")
            .run(|ui| {
                dialog.ui(ui, &mut host);
            })
            .expect("the search is typed");
        assert!(dialog.shown_entries() < all);
        assert!(
            shown_titles(&dialog).contains(&String::from("Commands/DimStyle")),
            "{:?}",
            shown_titles(&dialog)
        );

        dialog.set_filter("COPC");
        assert!(shown_titles(&dialog).contains(&String::from("Window/COPC")));

        dialog.set_filter("ONLINE");
        let online = shown_titles(&dialog);
        assert!(online.contains(&String::from("Online data/ONLINE IMPORT")));
        assert!(online.contains(&String::from("Window/Online")));
        harness
            .run(|ui| {
                dialog.ui(ui, &mut host);
            })
            .expect("the dialog is drawn");
        assert_eq!(
            harness.report("commandReferenceCount").expect("the count"),
            format!("{} of {all} entries", online.len())
        );

        // Every word must occur: two words narrow what one finds.
        dialog.set_filter("layer colour");
        let both = dialog.shown_entries();
        dialog.set_filter("layer");
        assert!(both < dialog.shown_entries());

        dialog.set_filter("no command has this word xyzzy");
        assert_eq!(dialog.shown_entries(), 0);
        dialog.set_filter("");
        assert_eq!(dialog.shown_entries(), all);
    }

    #[test]
    fn a_double_click_puts_the_verb_on_the_command_line() {
        let mut host = RecordingHost::new();
        let mut dialog = CommandReferenceDialog::new(command_reference_sections());
        dialog.set_filter("DIMSTYLE LIST");
        let (section, entry) = find_item(&dialog, "DimStyle").expect("DimStyle");
        // Beside the title, the group's first line without its label.
        let summary = summary_of(&dialog.sections()[section].entries[entry]);
        assert!(summary.starts_with("DIMSTYLE LIST"), "{summary}");
        assert_eq!(
            dialog.activate(section, entry).as_deref(),
            Some("DIMSTYLE ")
        );
        dialog.set_current(section, Some(entry));
        assert!(dialog.detail().starts_with("DimStyle  DIMSTYLE LIST"));
        assert_eq!(dialog.copy_text(), dialog.detail());
        let mut harness = reference_shown(&mut dialog, &mut host);
        harness
            .press("commandReferenceCopy")
            .run(|ui| {
                dialog.ui(ui, &mut host);
            })
            .expect("Copy is pressed");
        assert_eq!(Clipboard::text(host.clipboard()), Some(dialog.copy_text()));
    }

    #[test]
    fn the_update_verbs_are_found_by_searching_and_put_on_the_command_line() {
        let mut dialog = CommandReferenceDialog::new(command_reference_sections());
        // A line of the family, as a person would look for it: the section that has its usage, and
        // the Window section's row that points there.
        dialog.set_filter("UPDATE APPLY CONFIRM");
        let found = shown_titles(&dialog);
        assert!(found.contains(&String::from("Updates/Update")), "{found:?}");
        assert!(found.contains(&String::from("Window/Update")), "{found:?}");

        // The section by its name, its entry chosen: the detail is the verbs' help from its first
        // line, and a double-click puts the verb on the command line, to be finished there.
        assert!(dialog.show_section("Updates"));
        let (section, entry) = dialog.current().expect("an item is chosen");
        let entry = entry.expect("an entry, not the section's row");
        assert_eq!(dialog.sections()[section].name, "Updates");
        assert_eq!(dialog.sections()[section].entries[entry].title, "Update");
        assert!(dialog.detail().starts_with("Update    UPDATE CHECK"));
        assert_eq!(dialog.activate(section, entry).as_deref(), Some("UPDATE "));
    }

    #[test]
    fn the_sheets_item_opens_the_reference_at_the_sheet_verbs() {
        let mut host = RecordingHost::new();
        let mut dialog = CommandReferenceDialog::new(command_reference_sections());
        dialog.set_filter("COPC");
        assert!(dialog.show_section("Sheets"));
        let harness = reference_shown(&mut dialog, &mut host);
        assert_eq!(
            harness
                .report("commandReferenceSearch")
                .expect("the search"),
            ""
        );
        let (section, entry) = dialog.current().expect("an item is chosen");
        assert!(entry.is_some());
        assert_eq!(dialog.sections()[section].name, "Sheets");
        assert!(dialog.detail().starts_with("Sheets on the command line"));
        assert!(!dialog.show_section("No such section"));
    }
}

/// The dialog drawn in a harness whose acts go to it.
fn shortcuts_shown(dialog: &mut KeyboardShortcutsDialog) -> Harness {
    let mut harness = Harness::new();
    harness.target("keyboardShortcutsDialog");
    harness
        .show(|ui| dialog.ui(ui))
        .expect("the dialog is drawn");
    harness
}

fn row(key: &str, command: &str, menu: &str, tip: &str) -> ShortcutRow {
    ShortcutRow {
        key: String::from(key),
        command: String::from(command),
        menu: String::from(menu),
        tip: String::from(tip),
    }
}

/// The Key column of the table as a driver reads it.
fn keys(harness: &Harness) -> Vec<String> {
    harness
        .control("keyboardShortcutsTable")
        .expect("the table")
        .items
        .iter()
        .map(|item| String::from(item.split(" | ").next().unwrap_or("")))
        .collect()
}

mod keyboard_shortcuts_dialog {
    use super::*;

    #[test]
    fn every_key_is_listed_and_a_clash_is_marked() {
        let rows = vec![
            row("Ctrl+L", "Layers", "Format", "The drawing's layers"),
            row("Ctrl+L", "Line", "Draw", "Draw a line"),
            row("Ctrl+S", "Save", "File", "Save the project"),
        ];
        let mut dialog =
            KeyboardShortcutsDialog::new(rows, &[String::from("Ctrl+L: formatLayers, Line")]);
        assert_eq!(dialog.object_name(), "keyboardShortcutsDialog");
        let mut harness = shortcuts_shown(&mut dialog);
        let shown = keys(&harness);
        assert_eq!(shown.len(), 3);
        // A key is shown as the platform writes it: "Ctrl+L" on Windows and Linux, "⌘L" on macOS,
        // where Ctrl is the Command key.
        let clash = format!("{} (clash)", native_key_text("Ctrl+L"));
        assert_eq!(shown.iter().filter(|key| **key == clash).count(), 2);
        let save = native_key_text("Ctrl+S");
        assert_eq!(shown.iter().filter(|key| **key == save).count(), 1);
        assert!(
            harness
                .report("keyboardShortcutsClashes")
                .expect("the clashes")
                .starts_with("1 key reaches more than one command")
        );

        harness
            .fill("keyboardShortcutsSearch", "file save")
            .run(|ui| dialog.ui(ui))
            .expect("the search is typed");
        assert_eq!(dialog.shown_rows(), 1);
        dialog.set_filter("ctrl+l");
        assert_eq!(dialog.shown_rows(), 2);
        harness
            .run(|ui| dialog.ui(ui))
            .expect("the dialog is drawn");
        assert_eq!(
            harness.report("keyboardShortcutsCount").expect("the count"),
            "2 of 3 keys"
        );
    }

    #[test]
    fn no_clash_is_said_as_such() {
        let mut dialog = KeyboardShortcutsDialog::new(
            vec![row("F7", "Grid", "View", "Show or hide the grid")],
            &[],
        );
        let harness = shortcuts_shown(&mut dialog);
        assert_eq!(
            harness
                .report("keyboardShortcutsClashes")
                .expect("the clashes"),
            "Every key reaches one command."
        );
        assert_eq!(keys(&harness)[0], "F7");
    }
}
