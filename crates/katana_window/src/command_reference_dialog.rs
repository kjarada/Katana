//! Help > Command Reference (command_reference_dialog.hpp; docs/desktop.md, "The Command
//! Reference and the keyboard shortcuts"): every command the window's command line takes, in one
//! searchable place.
//!
//! The reference is built from the texts the verbs' own code keeps - the interpreter's HELP (with
//! the annotation verbs), HELP SHEETS, HELP UTILITY, the ONLINE usage, the geoprocessing and the
//! survey field file verbs' help, HELP UPDATE - plus `window_help_text`, the verbs only the
//! window's front end runs, and the tool catalogue's names, aliases and tips. Nothing is written
//! twice: a verb whose help changes changes here, and katana_cli -h and katana_mcp's katana_help
//! print the same interpreter text.
//!
//! `window_help_text` is what the window's dispatcher takes before a line reaches the interpreter
//! (MainWindow::dispatchLine and runWorkbenchLine, K's): a verb added there is added here, and the
//! typed HELP prints it after the interpreter's help.
//!
//! The dialog is non-modal and changes nothing; a double-click puts an entry's verb on the command
//! line, for the person to finish and run (the dialog answers it; the window puts it there). Object
//! names:
//!
//! ```text
//! commandReferenceDialog   the dialog (Help > Command Reference, helpCommandReference; Help >
//!                          Sheets and Plotting Commands, helpSheetCommands, opens it at Sheets)
//! commandReferenceSearch   the filter: every word must occur in an entry (its section, title or
//!                          text), in any case
//! commandReferenceTree     the sections and their entries
//! commandReferenceDetail   the chosen entry's whole text (read-only)
//! commandReferenceCount    how many entries the filter shows
//! commandReferenceCopy     copy the chosen entry, or every entry shown
//! commandReferenceClose    close
//! ```

use std::collections::BTreeMap;

use egui::{Align, Rect, RichText, Sense, Ui, UiBuilder, Vec2};
use katana_cad::CommandInterpreter;
use katana_cad::interactive_tool::tool_catalog;

use crate::command_log::contains_ignoring_case;
use crate::drive::{
    ControlKind, ControlRecord, LineOptions, button, custom, label, line_with, simplified,
};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme;

/// One entry of the reference: a command, or a group of commands as the help text groups them
/// ("Layers  LAYER LIST | NEW name ...").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceEntry {
    /// What the tree shows: the help's label for a group ("Layers"), else the command's leading
    /// words ("SHEET NEW", "ONLINE IMPORT").
    pub title: String,
    /// What a double-click puts on the command line: the first command word ("LAYER", "SHEET
    /// NEW"); empty for prose.
    pub verb: String,
    /// The entry's lines as the help gives them.
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceSection {
    pub name: String,
    pub entries: Vec<ReferenceEntry>,
}

/// A command word as the helps write them: capitals and digits, as LAYER, PLOTSHEETS or COPC.
fn is_command_word(word: &str) -> bool {
    let mut bytes = word.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_uppercase())
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn entry_for(line: &str) -> ReferenceEntry {
    let mut entry = ReferenceEntry {
        text: String::from(line),
        ..ReferenceEntry::default()
    };
    let line = line.strip_prefix("usage: ").unwrap_or(line);
    let words: Vec<&str> = line.split(' ').filter(|word| !word.is_empty()).collect();
    let Some(&first) = words.first() else {
        return entry;
    };
    // A label is the help's first column ("Draw      POINT p ..."): a word followed by two or
    // more blanks, or one a command never is - with a lower-case letter ("AnnoScale ANNOSCALE").
    let after: String = line.chars().skip(first.chars().count()).take(2).collect();
    let is_label = after == "  " || first != first.to_uppercase();
    if is_label {
        entry.title = String::from(first.strip_suffix(':').unwrap_or(first));
        // The command the group starts with, when one follows at once.
        if words.len() > 1 && is_command_word(words[1]) {
            entry.verb = String::from(words[1]);
        }
        return entry;
    }
    // A command line: its leading command words, two at most, as "SHEET NEW" or "ONLINE IMPORT".
    let mut title: Vec<&str> = Vec::new();
    for &word in &words {
        if !is_command_word(word) || title.len() == 2 {
            break;
        }
        title.push(word);
    }
    entry.title = if title.is_empty() {
        String::from(first)
    } else {
        title.join(" ")
    };
    entry.verb = title.join(" ");
    entry
}

/// The first line of an entry's text, blanks run together: what the tree shows beside the title -
/// without a group's label, which the title column already shows ("Layers  LAYER LIST ..." reads
/// "LAYER LIST ...").
pub fn summary_of(entry: &ReferenceEntry) -> String {
    let first = simplified(entry.text.split('\n').next().unwrap_or(""));
    if entry.title != entry.verb
        && let Some(rest) = first.strip_prefix(&format!("{} ", entry.title))
    {
        return String::from(rest);
    }
    first
}

/// An entry's text as paragraphs for egui to wrap: the help's lines are wrapped by hand at about
/// 95 characters, and wrapped again by egui at the dialog's width they broke ragged - a line, then
/// a word or two alone ("in a", "form the"). A line goes on the one before it unless it starts an
/// entry (no blank before it, after the first) or a command of its own (indented, its first word a
/// command word of two letters or more: "CODE CENSUS ..." under "CODE ..."); a blank line parts
/// two paragraphs. Each line's own spacing is kept, as the help sets its columns with it.
pub fn unwrapped(text: &str) -> String {
    let mut paragraphs = String::new();
    // Whether the next line may go on the last one: false at the start and after a blank line.
    let mut open = false;
    for line in text.replace("\r\n", "\n").split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraphs.is_empty() {
                paragraphs.push_str("\n\n");
            }
            open = false;
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        let first = trimmed.split(' ').next().unwrap_or("");
        let starts_command = first.len() >= 2 && is_command_word(first);
        let new_line = indented && starts_command;
        if open && !new_line {
            paragraphs.push(' ');
        } else if open {
            paragraphs.push('\n');
        }
        paragraphs.push_str(trimmed);
        open = true;
    }
    paragraphs
}

/// A help text read into entries, in the layout every help here uses: an entry starts at a line
/// that does not start with a blank and runs on over the lines that do; the first paragraph, when a
/// blank line ends it and more follows, is the section's introduction ("About"); "usage: " before a
/// command is dropped.
pub fn reference_entries(help: &str) -> Vec<ReferenceEntry> {
    let text = help.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut entries: Vec<ReferenceEntry> = Vec::new();
    let mut start = 0;
    // The introduction: the first paragraph, when a blank line ends it and entries follow.
    let mut blank = 0;
    while blank < lines.len() && !lines[blank].trim().is_empty() {
        blank += 1;
    }
    let more = lines[blank.min(lines.len())..]
        .iter()
        .any(|line| !line.trim().is_empty());
    if blank > 0 && blank < lines.len() && more {
        entries.push(ReferenceEntry {
            title: String::from("About"),
            verb: String::new(),
            text: lines[..blank].join("\n"),
        });
        start = blank + 1;
    }
    let mut open = false;
    for line in lines.iter().skip(start) {
        if line.trim().is_empty() {
            open = false;
            continue;
        }
        if open
            && line.chars().next().is_some_and(char::is_whitespace)
            && let Some(last) = entries.last_mut()
        {
            last.text.push('\n');
            last.text.push_str(line);
            continue;
        }
        entries.push(entry_for(line.trim()));
        open = true;
    }
    entries
}

/// The verbs the window's front end runs itself, beside the interpreter's, in the interpreter's
/// help layout: what the window's dispatcher takes before a line reaches the interpreter. A verb
/// added there is added here - the typed HELP prints this after the interpreter's help, and the
/// reference shows it as its Window section. CUSTOMISE is the interpreter's verb, and the
/// interpreter's help - which the typed HELP prints first - has it, so it has no block here.
pub fn window_help_text() -> &'static str {
    r"Window    the verbs the desktop window's command line runs itself, beside the
          interpreter's. IMPORT, EXPORT, INFO <file>, REFS, COPC and UPDATE are
          katana_cli's too, and UTILITY is the interpreter's, run here through the
          utilities workbench; the rest are the window's alone, which katana_cli and
          katana_mcp refuse by name
Script    SCRIPT <file.kcs> [CONTINUE]   run a katana_cli script a line at a time, stopping
          at the first line refused unless CONTINUE (File > Run Script); a line starting
          with # is a note and runs nothing; several lines pasted run as a script, but
          pasted at a prompt for text (Text, Multiline Text) they are its lines
Import    IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]   a drawing, vector data, a
          raster or a point cloud, by its extension; vector data, a .12da archive or a DXF
          moves as one piece: LOCAL puts its lower-left corner at 0,0, ALONGSIDE on the
          drawing's, OFFSET by dE east and dN north (GIS > Import Vector Data, Placement)
Export    EXPORT <file>   the drawing, by extension (DXF, GeoPackage, GeoJSON, SHP, .12da)
Info      INFO <file>   describe a data file through GDAL or PDAL (INFO id: an entity)
IFC       IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]
          [TOLERANCE <m>] [TAKECRS | KEEPCRS] | EXPORT <file.ifc> [UTILITIES <schedule.csv>]
          [SCHEMA <schema.csv>] [RULES <rules.csv>] [SPACING <m>] [NODRAWING] [NOENTITIES]
          [SELECTED] [NOALIGNMENTS] [NOSURFACES] [NOROADMODEL] [PREVIEW] | INFO <file.ifc> |
          IFC RULES <file.csv>   IFC 4.3 (File > Import IFC, Export IFC; docs/ifc.md)
Refs      REFS   the reference layers: rasters and point clouds
COPC      COPC <source> <destination.copc.laz>   convert a point cloud to COPC
PlotSheet PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono] [sheets=1,3-5]
          [dpi=n] [lineweight=f] [folder=path] [pattern=text]   plot the sheets (HELP SHEETS)
Plot      PLOT <file.pdf> [paper=A0..A4] [landscape|portrait] [fit|scale=N] [dpi=N]
          [style=colour|grey|mono] [lineweight=F] [margin=MM]   the drawing on one sheet, as
          the plan view shows it (File > Plot to PDF)
Snapshot  SNAPSHOT <file.png|.jpg|.tif> | CLIPBOARD [width=N] [height=N] [scale=F]
          [bg=theme|white|none] [view=plan|3d]   a picture of a view (File > Export View
          as Image, Edit > Copy View as Image)
View      GRID [ON|OFF] | SNAP [ON|OFF] (OSNAP) | SNAP <mode> [ON|OFF]   one of View > Snap
          Modes: Endpoint, Midpoint, Center, Intersection, Perpendicular, Tangent, Nearest,
          Grid | EXAGGERATION [factor]   elevations in the 3D and section views times 0.01
          to 1000 (View > Vertical Exaggeration); alone, the factor now. VIEWS and ZOOM are
          the interpreter's, answered by this window's views (Commands, Views and Zoom)
Online    ONLINE PROVIDERS | LAYERS | INFO | IMPORT | CUSTOM | KEY   online data (GIS >
          Online Data); the usage of each is under Online data
Utility   UTILITY REPORT|VERIFY|CLEARANCE|CHECK|DRAW|REGRADE|SCHEDULE   subsurface
          utilities, on a schedule or what is drawn (HELP UTILITY)
Update    UPDATE CHECK | STATUS | DOWNLOAD CONFIRM | APPLY CONFIRM | SKIP | BLOCKED CLEAR |
          SET AUTO | TOKEN   a newer Katana: asked about, downloaded and installed (Help >
          Check for Updates, and File > Settings > Updates); the usage of each is
          under Updates (HELP UPDATE)
Road      ROADMODEL SHOW [ON|OFF]   draw the road model in the views, or not (BIM > Show Road
          Model); the rest of ROADMODEL is the interpreter's, and under Road model
Quit      QUIT | EXIT   close the window; in a script, end the script
Tools     a bare tool word typed here starts the tool: LINE, L, C, TR, or an id such as
          draw.circle.ttr; with arguments it is the interpreter's (LINE 0,0 10,0), and so
          is a script's, a paste's or a dialog's line, as katana_cli runs it (ERASE there
          erases the selection). A bare LS typed starts the List tool, where katana_cli
          reads LS as LABELSTYLE"
}

/// Every section of the reference: the interpreter's commands (the annotation verbs among them),
/// sheets, subsurface utilities, volumes, the road model, road objects, online data,
/// geoprocessing, survey field files, updates (the UPDATE verbs' own help), the window's own
/// verbs, and the tools by menu.
pub fn command_reference_sections() -> Vec<ReferenceSection> {
    let section = |name: &str, help: &str| ReferenceSection {
        name: String::from(name),
        entries: reference_entries(help),
    };
    let mut sections = vec![
        section("Commands", &CommandInterpreter::help_text()),
        section(
            "Sheets",
            &katana_cad::plotting::sheet_verbs::sheet_verb_help(),
        ),
        section(
            "Subsurface utilities",
            &katana_cad::utilities::utility_verbs::utility_verb_help(),
        ),
        // The VOLUME verbs and SURFACE DIFFERENCE and ENVELOPE (HELP VOLUME).
        section(
            "Volumes",
            &katana_cad::terrain::volume_verbs::volume_verb_help(),
        ),
        // The road model (docs/road_design.md): HELP CORRIDOR.
        section(
            "Road model",
            &katana_cad::road::road_verbs::road_verb_help(),
        ),
        // Road objects (docs/road_objects.md): HELP ROADOBJECT.
        section(
            "Road objects",
            &katana_cad::road::road_object_verbs::road_object_verb_help(),
        ),
        section("Online data", &katana_interop::online_verbs::online_usage()),
        // GDAL and the geoprocessing verbs, from the executor's own table, so the window and
        // katana_help cannot list them differently.
        section("Geoprocessing", &katana_app::geo::help_text()),
        // SURVEY READ and IMPORT, from the session's own help, as katana_help says them.
        section(
            "Survey field files",
            katana_app::survey_verbs::survey_help_text(),
        ),
        // The UPDATE verbs, from the session's own help: what HELP UPDATE and katana_cli --help
        // print, so a line added to the family is here at once.
        section("Updates", katana_app::update_verbs::update_help_text()),
        section("Window", window_help_text()),
    ];
    // The tools, a section a menu, in the menus' order: what each is called, the words that start
    // it, its key and what it does.
    let mut tools: BTreeMap<String, Vec<ReferenceEntry>> = BTreeMap::new();
    let mut categories: Vec<String> = Vec::new();
    for tool in tool_catalog().all() {
        if !tools.contains_key(&tool.category) {
            categories.push(tool.category.clone());
        }
        let aliases = tool.aliases.join(", ");
        let entry = ReferenceEntry {
            title: tool.name.clone(),
            verb: tool
                .aliases
                .first()
                .cloned()
                .unwrap_or_else(|| tool.id.clone()),
            text: format!(
                "{} ({}){}\n          {}\n          id {}, {} > {}",
                tool.name,
                if tool.aliases.is_empty() {
                    tool.id.as_str()
                } else {
                    aliases.as_str()
                },
                if tool.shortcut.is_empty() {
                    String::new()
                } else {
                    format!("   {}", tool.shortcut)
                },
                tool.tip,
                tool.id,
                tool.category,
                tool.group
            ),
        };
        tools.entry(tool.category.clone()).or_default().push(entry);
    }
    for category in categories {
        // "Draw tools"; the menu already called Tools stays "Tools".
        let name = if category.ends_with("Tools") {
            category.clone()
        } else {
            format!("{category} tools")
        };
        sections.push(ReferenceSection {
            name,
            entries: tools.remove(&category).unwrap_or_default(),
        });
    }
    sections
}

// ---- the dialog -------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ReferenceState {
    sections: Vec<ReferenceSection>,
    search: String,
    filtered_for: Option<String>,
    /// For each section, whether each of its entries is shown.
    shown: Vec<Vec<bool>>,
    expanded: Vec<bool>,
    /// The item chosen: a section, and one of its entries or none (the section's own row).
    current: Option<(usize, Option<usize>)>,
    count: String,
    detail: String,
    /// A verb a double-click asked to put on the command line.
    inserted: Option<String>,
    copy: bool,
    close: bool,
}

impl ReferenceState {
    /// setFilter: the entries matching every word of `search` shown; a search opens what it found,
    /// and the whole reference opens closed, to be read by its sections.
    fn filter(&mut self) {
        if self.filtered_for.as_deref() == Some(self.search.as_str()) {
            return;
        }
        self.filtered_for = Some(self.search.clone());
        let words: Vec<&str> = self
            .search
            .split(' ')
            .filter(|word| !word.is_empty())
            .collect();
        let mut shown_total = 0;
        let mut total = 0;
        self.shown = Vec::with_capacity(self.sections.len());
        self.expanded = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            let shown: Vec<bool> = section
                .entries
                .iter()
                .map(|entry| {
                    let haystack = format!("{} {} {}", section.name, entry.title, entry.text);
                    words
                        .iter()
                        .all(|word| contains_ignoring_case(&haystack, word))
                })
                .collect();
            let visible = shown.iter().filter(|shown| **shown).count();
            self.expanded.push(!words.is_empty() && visible > 0);
            shown_total += visible;
            total += section.entries.len();
            self.shown.push(shown);
        }
        self.count = if words.is_empty() {
            format!("{total} entries")
        } else {
            format!("{shown_total} of {total} entries")
        };
    }

    fn entry_shown(&self, section: usize, entry: usize) -> bool {
        self.shown
            .get(section)
            .and_then(|shown| shown.get(entry))
            .copied()
            .unwrap_or(false)
    }

    fn section_shown(&self, section: usize) -> bool {
        self.shown
            .get(section)
            .is_some_and(|shown| shown.iter().any(|shown| *shown))
    }

    fn shown_entries(&self) -> usize {
        self.shown.iter().flatten().filter(|shown| **shown).count()
    }

    fn entry(&self, section: usize, entry: usize) -> Option<&ReferenceEntry> {
        self.sections.get(section)?.entries.get(entry)
    }

    /// What Copy copies: the chosen entry's text, or every entry shown.
    fn copy_text(&self) -> String {
        if let Some((section, Some(entry))) = self.current
            && self.entry_shown(section, entry)
            && let Some(chosen) = self.entry(section, entry)
        {
            return chosen.text.clone();
        }
        let mut shown = Vec::new();
        for (at, section) in self.sections.iter().enumerate() {
            for (number, entry) in section.entries.iter().enumerate() {
                if self.entry_shown(at, number) {
                    shown.push(entry.text.clone());
                }
            }
        }
        shown.join("\n")
    }

    fn show_detail(&mut self) {
        self.detail = match self.current {
            Some((section, Some(entry))) => self
                .entry(section, entry)
                .map(|chosen| chosen.text.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
    }

    fn choose(&mut self, item: (usize, Option<usize>)) {
        self.current = Some(item);
        self.show_detail();
    }

    /// A double-click on an entry: its verb, followed by a blank, for the command line.
    fn activate(&mut self, section: usize, entry: usize) -> Option<String> {
        let chosen = self.entry(section, entry)?;
        if chosen.verb.is_empty() {
            return None;
        }
        Some(format!("{} ", chosen.verb))
    }

    /// The tree as main.cpp's report reads an item view: each section, then - when it is open -
    /// each of its entries, every row's cells joined by " | ".
    fn tree_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for (at, section) in self.sections.iter().enumerate() {
            let visible = self
                .shown
                .get(at)
                .map_or(0, |shown| shown.iter().filter(|shown| **shown).count());
            rows.push(format!(
                "{} | {visible} of {}",
                section.name,
                section.entries.len()
            ));
            if self.expanded.get(at).copied().unwrap_or(false) {
                for entry in &section.entries {
                    rows.push(format!("{} | {}", entry.title, summary_of(entry)));
                }
            }
        }
        rows
    }

    fn body(&mut self, ui: &mut Ui) {
        line_with(
            ui,
            "commandReferenceSearch",
            &mut self.search,
            LineOptions {
                placeholder: "Search: words that must all occur, such as layer colour",
                // Across the table it searches, as its placeholder is long.
                width: Some(f32::INFINITY),
                ..LineOptions::default()
            },
        );
        self.filter();
        let mut chosen = None;
        let mut activated = None;
        let mut toggled = None;
        // The tree is a table in the window's one look (dialog_layout): a raised header naming its
        // two columns over them, whole rows striped and the chosen one lit across the table. Its
        // header was two words in a row over the rows, which did not line up under them, and a
        // chosen row a pill the size of its text.
        let row_height = ui.spacing().interact_size.y;
        let indent = crate::drive::controls::EXPANDER_WIDTH + ui.spacing().item_spacing.x;
        let titles = self.title_column(ui);
        let width = ui.available_width();
        let header = Rect::from_min_size(ui.cursor().min, Vec2::new(width, row_height));
        ui.allocate_rect(header, Sense::hover());
        ui.painter()
            .rect_filled(header, 0.0, theme::raised().to_egui());
        for (x, words) in [(indent, "Command"), (indent + titles, "What it takes")] {
            cell_text(
                ui,
                header,
                header.left() + x,
                header.right(),
                RichText::new(words)
                    .strong()
                    .color(theme::text_muted().to_egui()),
            );
        }
        let tree = egui::ScrollArea::vertical()
            .id_salt("commandReferenceTreeScroll")
            .max_height(360.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut striped = false;
                for (at, section) in self.sections.iter().enumerate() {
                    if !self.section_shown(at) {
                        continue;
                    }
                    let visible = self.shown[at].iter().filter(|shown| **shown).count();
                    let open = self.expanded.get(at).copied().unwrap_or(false);
                    let (rect, row) =
                        whole_row(ui, row_height, self.current == Some((at, None)), striped);
                    striped = !striped;
                    // The expander over the row's left end, drawn after the row so a click on it
                    // is its own; either opens or closes the section.
                    let mut cell = ui.new_child(UiBuilder::new().max_rect(rect));
                    let expander = crate::drive::controls::tree_expander(&mut cell, Some(open));
                    cell_text(
                        ui,
                        rect,
                        rect.left() + indent,
                        rect.right(),
                        RichText::new(section.name.as_str()).strong(),
                    );
                    cell_text(
                        ui,
                        rect,
                        rect.left() + indent + titles,
                        rect.right(),
                        RichText::new(format!("{visible} of {}", section.entries.len()))
                            .color(theme::text_muted().to_egui()),
                    );
                    if row.clicked() || expander.clicked() {
                        toggled = Some(at);
                        chosen = Some((at, None));
                    }
                    if !open {
                        continue;
                    }
                    for (number, entry) in section.entries.iter().enumerate() {
                        if !self.shown[at][number] {
                            continue;
                        }
                        let (rect, row) = whole_row(
                            ui,
                            row_height,
                            self.current == Some((at, Some(number))),
                            striped,
                        );
                        striped = !striped;
                        // Under its section's name, past the section's expander.
                        cell_text(
                            ui,
                            rect,
                            rect.left() + indent,
                            rect.left() + indent + titles,
                            RichText::new(entry.title.as_str()),
                        );
                        cell_text(
                            ui,
                            rect,
                            rect.left() + indent + titles,
                            rect.right(),
                            RichText::new(summary_of(entry)),
                        );
                        let row = row.on_hover_text(entry.text.as_str());
                        if row.double_clicked() {
                            activated = Some((at, number));
                        }
                        if row.clicked() {
                            chosen = Some((at, Some(number)));
                        }
                    }
                }
            });
        let mut record = ControlRecord::new("commandReferenceTree", ControlKind::Rows);
        record.items = self.tree_rows();
        let rows = ui
            .interact(tree.inner_rect, tree.id.with("rows"), egui::Sense::hover())
            .on_hover_text("Double-click a command to put it on the command line");
        custom(ui, &rows, record);
        if let Some(at) = toggled
            && let Some(open) = self.expanded.get_mut(at)
        {
            *open = !*open;
        }
        if let Some(item) = chosen {
            self.choose(item);
        }
        if let Some((section, entry)) = activated {
            self.inserted = self.activate(section, entry);
        }
        self.detail_ui(ui);
    }

    /// How wide the title column is: the widest title shown and a column's gap, at most
    /// TITLE_COLUMN, so a long one is cut short rather than pushing every summary across.
    fn title_column(&self, ui: &Ui) -> f32 {
        let mut widest: f32 = 0.0;
        for (at, section) in self.sections.iter().enumerate() {
            if !self.expanded.get(at).copied().unwrap_or(false) {
                widest = widest.max(crate::shell::dialog::one_line_width(ui, &section.name));
                continue;
            }
            for (number, entry) in section.entries.iter().enumerate() {
                if self.entry_shown(at, number) {
                    widest = widest.max(crate::shell::dialog::one_line_width(ui, &entry.title));
                }
            }
        }
        (widest + crate::dialog_layout::TABLE_COLUMN_GAP).min(TITLE_COLUMN)
    }

    /// The chosen entry's whole text, as paragraphs egui wraps (`unwrapped`), under the tree in a
    /// well of its own - drawn only once an entry is chosen. Before, an empty box six lines high
    /// stood under the tree until then; now one muted line says what goes there. Recorded as
    /// commandReferenceDetail, its value the entry's text as the help gives it.
    fn detail_ui(&mut self, ui: &mut Ui) {
        ui.add_space(crate::dialog_layout::GAP);
        let mut record = ControlRecord::new("commandReferenceDetail", ControlKind::Text);
        record.value = self.detail.clone();
        if self.detail.is_empty() {
            let hint = ui.label(
                RichText::new("Choose a command to read all it takes.")
                    .color(theme::text_muted().to_egui()),
            );
            custom(ui, &hint, record);
            return;
        }
        let paragraphs = unwrapped(&self.detail);
        let shown = egui::Frame::new()
            .fill(theme::palette::input().to_egui())
            .stroke(egui::Stroke::new(1.0, theme::border().to_egui()))
            .corner_radius(crate::theme::style::CORNER_RADIUS)
            .inner_margin(crate::drive::controls::FIELD_INSET)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("commandReferenceDetailScroll")
                    .max_height(crate::dialog_layout::rows_height(ui, DETAIL_ROWS))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(paragraphs.as_str())
                                .wrap()
                                .selectable(true),
                        );
                    });
            });
        custom(ui, &shown.response, record);
    }

    fn footer(&mut self, ui: &mut Ui) {
        let count = self.count.clone();
        // The count at the left; Copy and Close at the right.
        let ((), (copy, close)) = crate::shell::dialog::button_row(
            ui,
            |ui| {
                label(ui, "commandReferenceCount", &count);
            },
            |ui| {
                let copy = button(ui, "commandReferenceCopy", "Copy")
                    .on_hover_text("Copy the chosen command, or every command shown")
                    .clicked();
                let close = button(ui, "commandReferenceClose", "Close").clicked();
                (copy, close)
            },
        );
        if copy {
            self.copy = true;
        }
        if close {
            self.close = true;
        }
    }
}

/// The widest the tree's title column grows: a command of two words ("SHEETS PAGESETUP") and its
/// gap. A group's long label is cut short at it rather than pushing every summary across.
const TITLE_COLUMN: f32 = 220.0;

/// How many lines of the chosen entry show before its well scrolls.
const DETAIL_ROWS: f32 = 6.0;

/// The dialog's body: the title column and a summary of most help lines, which run to about 95
/// characters.
const BODY_WIDTH: f32 = 760.0;

/// One whole row of a tree, `height` high across what holds it: lit when `chosen`, the faint
/// stripe when `striped`, the hover colour under the pointer (dialog_layout::table_row_colour's
/// look). Its rectangle and its response to a click. The Command Reference's and the Project
/// Coordinate System's trees: a selectable label in a row is a pill the size of its text.
pub(crate) fn whole_row(
    ui: &mut Ui,
    height: f32,
    chosen: bool,
    striped: bool,
) -> (Rect, egui::Response) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let fill = if chosen {
        Some(crate::dialog_layout::selected_row_colour())
    } else if response.hovered() {
        Some(theme::hover().to_egui())
    } else if striped {
        Some(ui.visuals().faint_bg_color)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    (rect, response)
}

/// `text` on one line in `row` from `left`, cut short with an ellipsis at `right`, centred in the
/// row's height.
pub(crate) fn cell_text(ui: &Ui, row: Rect, left: f32, right: f32, text: RichText) {
    let room = (right - left - crate::dialog_layout::TABLE_COLUMN_GAP / 2.0).max(0.0);
    let mut job = std::sync::Arc::unwrap_or_clone(egui::WidgetText::from(text).into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        Align::Center,
    ));
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.painter().layout_job(job);
    let at = egui::pos2(left, row.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, theme::text().to_egui());
}

/// Help > Command Reference (see the module comment).
pub struct CommandReferenceDialog {
    frame: DialogFrame,
    state: ReferenceState,
    /// Whether it is shown.
    pub open: bool,
}

impl CommandReferenceDialog {
    pub fn new(sections: Vec<ReferenceSection>) -> Self {
        let mut state = ReferenceState {
            sections,
            ..ReferenceState::default()
        };
        state.filter();
        Self {
            frame: DialogFrame::new("commandReferenceDialog", "Command Reference")
                .width(BODY_WIDTH),
            state,
            open: true,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    pub fn sections(&self) -> &[ReferenceSection] {
        &self.state.sections
    }

    /// Shows the entries matching `text` (what typing in the search does).
    pub fn set_filter(&mut self, text: &str) {
        self.state.search = String::from(text);
        self.state.filter();
    }

    /// Clears the filter and brings `section` forward, expanded, its first entry chosen. False for
    /// a section the reference does not have.
    pub fn show_section(&mut self, name: &str) -> bool {
        let Some(at) = self
            .state
            .sections
            .iter()
            .position(|section| section.name == name)
        else {
            return false;
        };
        self.set_filter("");
        if let Some(open) = self.state.expanded.get_mut(at) {
            *open = true;
        }
        let first = if self.state.sections[at].entries.is_empty() {
            None
        } else {
            Some(0)
        };
        self.state.choose((at, first));
        true
    }

    /// How many entries the filter shows.
    pub fn shown_entries(&self) -> usize {
        self.state.shown_entries()
    }

    /// Whether entry `entry` of section `section` is shown.
    pub fn entry_shown(&self, section: usize, entry: usize) -> bool {
        self.state.entry_shown(section, entry)
    }

    /// The item chosen: a section and one of its entries, or the section's own row.
    pub fn current(&self) -> Option<(usize, Option<usize>)> {
        self.state.current
    }

    /// Chooses an item, as a click on it does.
    pub fn set_current(&mut self, section: usize, entry: Option<usize>) {
        self.state.choose((section, entry));
    }

    /// What commandReferenceDetail shows.
    pub fn detail(&self) -> &str {
        &self.state.detail
    }

    /// What Copy copies: the chosen entry's text, or every entry shown.
    pub fn copy_text(&self) -> String {
        self.state.copy_text()
    }

    /// A double-click on an entry: the verb, followed by a blank, the window puts on its command
    /// line; None for prose.
    pub fn activate(&mut self, section: usize, entry: usize) -> Option<String> {
        self.state.activate(section, entry)
    }

    /// Draws the dialog while it is open. Answers a verb a double-click asked to put on the command
    /// line; Copy puts its text on the host's clipboard.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) -> Option<String> {
        if !self.open {
            return None;
        }
        let ctx = ui.ctx().clone();
        let shown = self.frame.show(
            &ctx,
            &mut self.state,
            |ui, state| state.body(ui),
            |ui, state| state.footer(ui),
        );
        if std::mem::take(&mut self.state.copy) {
            let text = self.state.copy_text();
            let _ = host.clipboard().set_text(&text);
        }
        if shown.closed || std::mem::take(&mut self.state.close) {
            self.open = false;
        }
        self.state.inserted.take()
    }
}
