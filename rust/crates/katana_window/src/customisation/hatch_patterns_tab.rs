//! The Hatch Patterns tab of Format > Styles and Linetypes (docs/desktop.md, "Styles and
//! Linetypes"): the drawing's hatch patterns - which the HATCH verb always had and the window only
//! offered by name in pick lists - listed with what uses them, and edited. The port of
//! customisation/hatch_patterns_tab.
//!
//! NOTHING IS CHANGED HERE. Every button writes the line a person would type - HATCH NEW, SOLID,
//! SET, DELETE, PURGE HATCHES, STYLE NEW - and runs it through the window's one executor
//! (host::Host::run), so it is echoed, kept in the history and one undo step. The rows are cad's
//! hatch_pattern_rows, below the window.
//!
//! The family grid is a buffer, as the Linetypes tab's pattern grid is: edited in place, saved by
//! one HATCH SET line, and never a line run from the grid's own change. The swatch draws the grid
//! as it stands, through the hatcher the plan view draws with (cad's hatch_segments), so what is
//! seen before Save is what Save will draw. A reload that did not change the pattern the grid was
//! filled from keeps its unsaved edits.
//!
//! No modal box: names are typed in the tab's name field. It hears the Document through a
//! DocumentWatcher, so it may outlive the Document and then does nothing.
//!
//! Object names:
//!
//! ```text
//! hatchPatternsPage          the tab
//! hatchPatternTable          name, kind (solid, N families), used by, description
//! hatchPatternEditor         the editor of the chosen pattern:
//!   hatchPatternSolid        a solid fill rather than line families
//!   hatchPatternFamilies     angle (degrees) and spacing (model units), a row a family
//!   hatchFamilyAdd, hatchFamilyRemove
//!   hatchPatternPreview      the swatch
//!   hatchPatternSave         HATCH SET name a s [a s ...] | HATCH SET name SOLID
//!   hatchPatternRevert       the editor as the drawing holds the pattern again
//! hatchPatternName           the name New, New Solid, Duplicate and New Style use
//! hatchPatternNew            HATCH NEW name a s ... from the family grid
//! hatchPatternNewSolid       HATCH SOLID name
//! hatchPatternDuplicate      the chosen pattern again under the name
//! hatchPatternDelete         HATCH DELETE name
//! hatchPatternPurge          PURGE HATCHES
//! hatchPatternSelectUsers    select the entities drawn with the chosen pattern
//! hatchPatternNewStyle       STYLE NEW name HATCH pattern: a style that hatches with it
//! hatchPatternStatus         what the last line replied, or why it was refused
//! ```
//!
//! What is the C++'s and not here. A cell of the family grid was a table item a test set the text
//! of; here the grid's cells are fields drawn in the grid, and `set_family_cell` is what typing
//! into one does. The swatch is drawn by the frame's own painter, as it is chrome the size of the
//! pane and never read back.

use egui::{Ui, pos2};
use katana_cad::document::Document;
use katana_cad::hatch_pattern_rows::{
    hatch_angle_degrees, hatch_families_text, hatch_pattern_kind, hatch_pattern_rows,
};
use katana_cad::hatching::hatch_segments;
use katana_cad::style_manager_rows::{UsageTable, entities_using};
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::tables::HatchPatternPolicy;
use katana_entity::{HatchLineFamily, HatchPattern, NamedTablePolicy, validate};
use katana_geometry::{Point2, Polyline2};
use katana_math::numerics::DEG_TO_RAD;

use super::customisation_context::CustomisationContext;
use super::document_watcher::DocumentWatcher;
use crate::annotation::dialog_support::table;
use crate::command_word::command_word;
use crate::drive::controls::{self, LineOptions, custom};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::theme::palette;

/// The tab's object name.
pub const HATCH_PATTERNS_PAGE: &str = "hatchPatternsPage";

/// What the buttons and the editor may do for the pattern chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Enabled {
    /// The editor, Delete and Duplicate: a pattern is chosen and it is not "none".
    editable: bool,
    /// The grid and its two buttons: editable, and not a solid fill.
    families: bool,
    /// Save and Revert: editable, with edits not yet saved.
    save: bool,
    /// New Style Using This and Select Users: any pattern is chosen.
    chosen: bool,
}

/// See the module comment.
pub struct HatchPatternsTab {
    context: CustomisationContext,
    /// The table: name, kind, used by, description.
    rows: Vec<Vec<String>>,
    current: Option<usize>,
    solid: bool,
    /// The family grid: an angle and a spacing a row, as typed.
    families: Vec<[String; 2]>,
    family_current: Option<usize>,
    /// What the swatch draws, or the note it shows in place of a pattern.
    swatch: Option<HatchPattern>,
    swatch_note: String,
    name: String,
    status: String,
    status_is_error: bool,
    /// The editor holds edits not yet saved, made to `edited_from` as the drawing held it; a
    /// reload that left that pattern alone keeps them.
    edited: bool,
    edited_from: Option<HatchPattern>,
    // Last, as in the C++: it goes first and delivers nothing to a half-dropped tab.
    watcher: DocumentWatcher,
}

impl HatchPatternsTab {
    /// `context.has_runner` may be false (the tab then says it cannot run the line), and
    /// `select_and_show` None.
    pub fn new(context: &CustomisationContext, document: &Document) -> Self {
        let mut tab = Self {
            context: context.clone(),
            rows: Vec::new(),
            current: None,
            solid: false,
            families: Vec::new(),
            family_current: None,
            swatch: None,
            swatch_note: String::new(),
            name: String::new(),
            status: String::new(),
            status_is_error: false,
            edited: false,
            edited_from: None,
            watcher: DocumentWatcher::new(document),
        };
        tab.reload(document);
        tab
    }

    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// The chosen pattern as the drawing holds it.
    fn current<'a>(&self, document: &'a Document) -> Option<&'a HatchPattern> {
        if !self.alive() {
            return None;
        }
        let chosen = self.current_pattern();
        if chosen.is_empty() {
            None
        } else {
            document.model().hatch_patterns.find(&chosen)
        }
    }

    fn set_status(&mut self, text: &str, is_error: bool) {
        self.status = String::from(text);
        self.status_is_error = is_error;
    }

    /// What the last line replied, or why it was refused.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Chooses the pattern `name` in the table; false when there is none.
    pub fn select_pattern(&mut self, document: &Document, name: &str) -> bool {
        match self
            .rows
            .iter()
            .position(|row| row.first().is_some_and(|cell| cell == name))
        {
            Some(row) => {
                self.choose_row(document, row);
                true
            }
            None => false,
        }
    }

    /// A row of the table chosen, by a person, a driver or `select_pattern`: the editor shows it.
    /// A row chosen again changes nothing, as a selection that did not change signals nothing.
    fn choose_row(&mut self, document: &Document, row: usize) {
        if self.current == Some(row) {
            return;
        }
        self.current = Some(row);
        self.edited = false;
        self.load_editor(document);
    }

    /// The chosen pattern's name; empty when there is none.
    pub fn current_pattern(&self) -> String {
        self.current
            .and_then(|row| self.rows.get(row))
            .and_then(|row| row.first())
            .cloned()
            .unwrap_or_default()
    }

    /// The table's rows: name, kind, used by, description.
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// The family grid as it stands: an angle and a spacing a row.
    pub fn families(&self) -> &[[String; 2]] {
        &self.families
    }

    pub fn is_solid(&self) -> bool {
        self.solid
    }

    /// The name field's text, set as typing sets it.
    pub fn set_name(&mut self, text: &str) {
        self.name = String::from(text);
    }

    /// A cell of the family grid typed into: column 0 the angle, 1 the spacing. Nothing runs; the
    /// grid is marked edited and the swatch drawn again.
    pub fn set_family_cell(&mut self, document: &Document, row: usize, column: usize, text: &str) {
        let Some(cell) = self
            .families
            .get_mut(row)
            .and_then(|cells| cells.get_mut(column))
        else {
            return;
        };
        if cell == text {
            return; // a cell set to what it holds changes nothing
        }
        *cell = String::from(text);
        self.mark_edited(document);
    }

    /// The Solid fill box ticked or cleared.
    pub fn set_solid(&mut self, document: &Document, solid: bool) {
        if self.solid == solid {
            return;
        }
        self.solid = solid;
        self.mark_edited(document);
    }

    /// One line through the window's executor, what it said shown; true when it was carried out.
    /// The tab reloads at once so the result is on screen when this returns.
    fn run(&mut self, host: &mut dyn Host, line: &str) -> bool {
        if !self.alive() {
            self.set_status("The drawing this tab edited is gone.", true);
            return false;
        }
        if !self.context.has_runner {
            self.set_status(
                &format!("Nothing here can run {line}: the dialog has no command line."),
                true,
            );
            return false;
        }
        let outcome = host.run(line);
        if outcome.ok {
            let first = outcome.reply.split('\n').find(|said| !said.is_empty());
            self.set_status(first.unwrap_or(line), false);
        } else if outcome.error.is_empty() {
            self.set_status(&format!("{line} was refused."), true);
        } else {
            self.set_status(&outcome.error, true);
        }
        self.reload(host.document());
        outcome.ok
    }

    /// The name field as a command word, or None, said, when it is empty or cannot be one.
    fn typed_name(&mut self) -> Option<String> {
        let typed = String::from(self.name.trim());
        if typed.is_empty() {
            self.set_status("Type a name first.", true);
            return None;
        }
        match command_word(&typed, "the name") {
            Ok(word) => Some(word),
            Err(error) => {
                self.set_status(&error.message, true);
                None
            }
        }
    }

    fn chosen_name(&mut self) -> Option<String> {
        let chosen = self.current_pattern();
        if chosen.is_empty() {
            self.set_status("Choose a hatch pattern first.", true);
            return None;
        }
        match command_word(&chosen, "the pattern's name") {
            Ok(word) => Some(word),
            Err(error) => {
                self.set_status(&error.message, true);
                None
            }
        }
    }

    /// Reads the drawing again: the table, keeping the pattern chosen, and the editor.
    pub fn reload(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let keep = self.current_pattern();
        self.load_table(document, &keep);
        self.load_editor(document);
    }

    fn load_table(&mut self, document: &Document, keep: &str) {
        let rows = hatch_pattern_rows(document.model());
        self.current = rows.iter().position(|row| row.name == keep);
        self.rows = rows
            .iter()
            .map(|row| {
                vec![
                    row.name.clone(),
                    hatch_pattern_kind(row),
                    if row.users.used() {
                        row.users.describe()
                    } else {
                        String::from("nothing")
                    },
                    row.description.clone(),
                ]
            })
            .collect();
    }

    fn load_editor(&mut self, document: &Document) {
        let pattern = self.current(document);
        // Unsaved edits survive a reload that left their pattern as it was.
        if self.edited && pattern.is_some() && pattern == self.edited_from.as_ref() {
            self.show_swatch(document);
            return;
        }
        self.solid = pattern.is_some_and(|pattern| pattern.solid);
        self.families = pattern
            .map(|pattern| {
                pattern
                    .families
                    .iter()
                    .map(|family| {
                        [
                            hatch_angle_degrees(family.angle),
                            format_exact_real(family.spacing),
                        ]
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.family_current = None;
        self.edited = false;
        self.edited_from = pattern.cloned();
        self.show_swatch(document);
    }

    fn enabled(&self, document: &Document) -> Enabled {
        let pattern = self.current(document);
        // "none" draws nothing on purpose and is not the tab's to change.
        let editable = pattern.is_some_and(|pattern| {
            !<HatchPatternPolicy as NamedTablePolicy<HatchPattern>>::is_protected(&pattern.name)
        });
        Enabled {
            editable,
            families: editable && !self.solid,
            save: editable && self.edited,
            chosen: pattern.is_some(),
        }
    }

    /// The families as the grid holds them, "a s a s ...", or why they cannot be: a row with one
    /// number.
    fn grid_families(&self) -> Result<String> {
        let mut words: Vec<&str> = Vec::new();
        for (row, cells) in self.families.iter().enumerate() {
            let angle = cells[0].trim();
            let spacing = cells[1].trim();
            if angle.is_empty() && spacing.is_empty() {
                continue; // a row added and never filled
            }
            if angle.is_empty() || spacing.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("family {} needs an angle and a spacing", row + 1),
                ));
            }
            words.push(angle);
            words.push(spacing);
        }
        if words.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a pattern needs a family of lines, or to be a solid fill",
            ));
        }
        Ok(words.join(" "))
    }

    /// The pattern the editor would save, for the swatch.
    fn editor_pattern(&self) -> Result<HatchPattern> {
        let mut pattern = HatchPattern {
            name: self
                .edited_from
                .as_ref()
                .map_or_else(|| String::from("preview"), |from| from.name.clone()),
            ..HatchPattern::default()
        };
        if self.solid {
            pattern.solid = true;
            return Ok(pattern);
        }
        let grid = self.grid_families()?;
        let words: Vec<&str> = grid.split(' ').collect();
        for (index, pair) in words.chunks_exact(2).enumerate() {
            let (Some(angle), Some(spacing)) =
                (parse_finite_double(pair[0]), parse_finite_double(pair[1]))
            else {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("family {} is not two numbers", index + 1),
                ));
            };
            let mut family = HatchLineFamily {
                angle: angle * DEG_TO_RAD,
                spacing,
                ..HatchLineFamily::default()
            };
            // Drawn with the offset HATCH SET keeps (by position), so the swatch shows what Save
            // will draw.
            if let Some(kept) = self
                .edited_from
                .as_ref()
                .and_then(|from| from.families.get(index))
            {
                family.offset = kept.offset;
            }
            pattern.families.push(family);
        }
        validate(&pattern)?;
        Ok(pattern)
    }

    fn show_swatch(&mut self, document: &Document) {
        if self.current(document).is_none() {
            self.swatch = None;
            self.swatch_note = String::from("Choose a hatch pattern");
            return;
        }
        match self.editor_pattern() {
            Ok(pattern) => {
                self.swatch = Some(pattern);
                self.swatch_note.clear();
            }
            Err(error) => {
                self.swatch = None;
                self.swatch_note = error.message;
            }
        }
    }

    fn mark_edited(&mut self, document: &Document) {
        self.edited = true;
        self.show_swatch(document);
    }

    // ---- what the buttons do -------------------------------------------------------------------

    fn add_family(&mut self, document: &Document) {
        self.families.push([String::new(), String::new()]);
        self.family_current = Some(self.families.len() - 1);
        self.mark_edited(document);
    }

    fn remove_family(&mut self, document: &Document) {
        let Some(row) = self.family_current.filter(|&row| row < self.families.len()) else {
            self.set_status("Choose the family to remove first.", true);
            return;
        };
        self.families.remove(row);
        // QTableWidget::removeRow leaves the row that took its place current, else the last.
        self.family_current = if self.families.is_empty() {
            None
        } else {
            Some(row.min(self.families.len() - 1))
        };
        self.mark_edited(document);
    }

    fn revert(&mut self, document: &Document) {
        self.edited = false;
        self.load_editor(document);
    }

    fn save(&mut self, host: &mut dyn Host) {
        let Some(chosen) = self.chosen_name() else {
            return;
        };
        if self.solid {
            self.run(host, &format!("HATCH SET {chosen} SOLID"));
            return;
        }
        match self.grid_families() {
            Ok(grid) => {
                self.run(host, &format!("HATCH SET {chosen} {grid}"));
            }
            Err(error) => self.set_status(&error.message, true),
        }
    }

    fn create(&mut self, host: &mut dyn Host) {
        let Some(typed) = self.typed_name() else {
            return;
        };
        match self.grid_families() {
            Ok(grid) => {
                if self.run(host, &format!("HATCH NEW {typed} {grid}")) {
                    let made = String::from(self.name.trim());
                    self.select_pattern(host.document(), &made);
                }
            }
            Err(error) => self.set_status(&error.message, true),
        }
    }

    fn create_solid(&mut self, host: &mut dyn Host) {
        if let Some(typed) = self.typed_name()
            && self.run(host, &format!("HATCH SOLID {typed}"))
        {
            let made = String::from(self.name.trim());
            self.select_pattern(host.document(), &made);
        }
    }

    fn duplicate(&mut self, host: &mut dyn Host) {
        let pattern = self.current(host.document()).cloned();
        let typed = self.typed_name();
        let (Some(pattern), Some(typed)) = (pattern, typed) else {
            return;
        };
        let line = if pattern.solid {
            format!("HATCH SOLID {typed}")
        } else {
            format!("HATCH NEW {typed} {}", hatch_families_text(&pattern))
        };
        if self.run(host, &line) {
            let made = String::from(self.name.trim());
            self.select_pattern(host.document(), &made);
        }
    }

    fn new_style(&mut self, host: &mut dyn Host) {
        let Some(chosen) = self.chosen_name() else {
            return;
        };
        if let Some(typed) = self.typed_name() {
            self.run(host, &format!("STYLE NEW {typed} HATCH {chosen}"));
        }
    }

    fn delete(&mut self, host: &mut dyn Host) {
        if let Some(chosen) = self.chosen_name() {
            self.run(host, &format!("HATCH DELETE {chosen}"));
        }
    }

    fn select_users(&mut self, host: &mut dyn Host) {
        let chosen = self.current_pattern();
        if !self.alive() || chosen.is_empty() {
            return;
        }
        let ids = entities_using(
            host.document().model(),
            UsageTable::HatchPattern,
            std::slice::from_ref(&chosen),
        );
        if ids.is_empty() {
            self.set_status(&format!("No entity is hatched with {chosen}."), false);
            return;
        }
        let count = ids.len();
        match self.context.select_and_show.clone() {
            Some(select_and_show) => select_and_show(host, &ids),
            None => {
                host.document_mut().selection_mut().set(ids);
                host.document().notify_selection_changed();
            }
        }
        self.set_status(
            &format!(
                "{count} entit{} selected.",
                if count == 1 { "y" } else { "ies" }
            ),
            false,
        );
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the tab - the drawing's changes since the last frame first.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if let Some(changes) = self.watcher.deliver(host.document())
            && changes.model
        {
            self.reload(host.document());
        }
        // The page is a control of its own to a driver, as the C++ page is a named widget.
        let page = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
        custom(
            ui,
            &page,
            ControlRecord::new(HATCH_PATTERNS_PAGE, ControlKind::Other),
        );
        let enabled = self.enabled(host.document());
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let shown = table(
                    ui,
                    "hatchPatternTable",
                    &["Name", "Kind", "Used by", "Description"],
                    &self.rows,
                    self.current,
                    &[],
                    None,
                );
                if let Some(row) = shown.chosen {
                    self.choose_row(host.document(), row);
                }
            });
            ui.add_enabled_ui(enabled.editable, |ui| {
                ui.group(|ui| self.editor(ui, host, enabled));
            });
        });

        ui.horizontal(|ui| {
            ui.label("Name:");
            controls::line_with(
                ui,
                "hatchPatternName",
                &mut self.name,
                LineOptions {
                    placeholder: "name for New, New Solid, Duplicate or New Style",
                    ..LineOptions::default()
                },
            );
            if controls::button(ui, "hatchPatternNew", "New")
                .on_hover_text("HATCH NEW name angle spacing ...: the families in the grid")
                .clicked()
            {
                self.create(host);
            }
            if controls::button(ui, "hatchPatternNewSolid", "New Solid")
                .on_hover_text("HATCH SOLID name: a solid fill")
                .clicked()
            {
                self.create_solid(host);
            }
            ui.add_enabled_ui(enabled.editable, |ui| {
                if controls::button(ui, "hatchPatternDuplicate", "Duplicate")
                    .on_hover_text("The chosen pattern again, under the name")
                    .clicked()
                {
                    self.duplicate(host);
                }
            });
            ui.add_enabled_ui(enabled.chosen, |ui| {
                if controls::button(ui, "hatchPatternNewStyle", "New Style Using This")
                    .on_hover_text(
                        "STYLE NEW name HATCH pattern: a style that hatches with the chosen \
                         pattern",
                    )
                    .clicked()
                {
                    self.new_style(host);
                }
            });
        });

        ui.horizontal(|ui| {
            ui.add_enabled_ui(enabled.editable, |ui| {
                if controls::button(ui, "hatchPatternDelete", "Delete")
                    .on_hover_text("HATCH DELETE name: refused while a layer or style names it")
                    .clicked()
                {
                    self.delete(host);
                }
            });
            if controls::button(ui, "hatchPatternPurge", "Purge")
                .on_hover_text("PURGE HATCHES: delete every pattern nothing uses, one undo step")
                .clicked()
            {
                self.run(host, "PURGE HATCHES");
            }
            let colour = if self.status_is_error {
                palette::error()
            } else {
                palette::text_muted()
            };
            controls::label_as(
                ui,
                "hatchPatternStatus",
                &self.status,
                egui::Label::new(egui::RichText::new(self.status.as_str()).color(colour.to_egui()))
                    .wrap(),
            );
            ui.add_enabled_ui(enabled.chosen, |ui| {
                if controls::button(ui, "hatchPatternSelectUsers", "Select Users")
                    .on_hover_text("Select the entities hatched with the chosen pattern")
                    .clicked()
                {
                    self.select_users(host);
                }
            });
        });
    }

    /// The editor of the chosen pattern, drawn in its group.
    fn editor(&mut self, ui: &mut Ui, host: &mut dyn Host, enabled: Enabled) {
        // The group is a control of its own to a driver: whether the chosen pattern is the tab's
        // to change is read off it.
        let heading = ui.strong("Pattern");
        custom(
            ui,
            &heading,
            ControlRecord::new("hatchPatternEditor", ControlKind::Other),
        );
        let mut solid = self.solid;
        if controls::check(ui, "hatchPatternSolid", "Solid fill", &mut solid)
            .on_hover_text("A filled area rather than families of lines")
            .changed()
        {
            self.set_solid(host.document(), solid);
        }
        ui.add_enabled_ui(enabled.families, |ui| {
            let mut typed: Option<(usize, usize, String)> = None;
            let grid = egui::Grid::new("hatchPatternFamilies")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.strong("Angle (degrees)");
                    ui.strong("Spacing");
                    ui.end_row();
                    for (row, cells) in self.families.iter().enumerate() {
                        for (column, cell) in cells.iter().enumerate() {
                            let mut text = cell.clone();
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut text)
                                    .id_salt(("hatchPatternFamilies", row, column))
                                    .desired_width(90.0),
                            );
                            if field.gained_focus() || field.clicked() {
                                self.family_current = Some(row);
                            }
                            if field.changed() {
                                typed = Some((row, column, text));
                            }
                        }
                        ui.end_row();
                    }
                });
            let response = grid.response.on_hover_text(
                "One family of parallel lines a row: the angle counter-clockwise from east in \
                 degrees, the spacing in model units - a crosshatch is two",
            );
            let mut record = ControlRecord::new("hatchPatternFamilies", ControlKind::Rows);
            record.items = self.families.iter().map(|cells| cells.join(" | ")).collect();
            record.value = self
                .family_current
                .and_then(|row| self.families.get(row))
                .map(|cells| cells[0].clone())
                .unwrap_or_default();
            custom(ui, &response, record);
            // An edit marks the grid and redraws the swatch; nothing runs from the grid's own
            // change (docs/desktop.md, "The rules a dialog or panel follows").
            if let Some((row, column, text)) = typed {
                self.set_family_cell(host.document(), row, column, &text);
            }
            ui.horizontal(|ui| {
                if controls::button(ui, "hatchFamilyAdd", "Add Family")
                    .on_hover_text("Another family of lines")
                    .clicked()
                {
                    self.add_family(host.document());
                }
                if controls::button(ui, "hatchFamilyRemove", "Remove Family")
                    .on_hover_text("Drop the chosen family")
                    .clicked()
                {
                    self.remove_family(host.document());
                }
            });
        });
        self.swatch_ui(ui);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(enabled.save, |ui| {
                if controls::button(ui, "hatchPatternRevert", "Revert")
                    .on_hover_text("Forget the edits made above")
                    .clicked()
                {
                    self.revert(host.document());
                }
                if controls::button(ui, "hatchPatternSave", "Save")
                    .on_hover_text("HATCH SET name angle spacing ... (or SOLID): one undo step")
                    .clicked()
                {
                    self.save(host);
                }
            });
        });
    }

    /// The pattern drawn over a square, as the plan view draws it: the families through cad's
    /// hatch_segments, which the view calls; a solid pattern filled. The square is eight of the
    /// widest spacing across, so a handful of lines show whatever the pattern's size; the
    /// world-origin anchoring the hatcher keeps is why the square is centred on the origin.
    fn swatch_ui(&self, ui: &mut Ui) {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(140.0, 140.0), egui::Sense::hover());
        let mut record = ControlRecord::new("hatchPatternPreview", ControlKind::Other);
        record.text.clone_from(&self.swatch_note);
        custom(ui, &response, record);
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, palette::panel().to_egui());
        let side = rect.width().min(rect.height()) - 16.0;
        let frame = egui::Rect::from_center_size(rect.center(), egui::vec2(side, side));
        let border = egui::Stroke::new(1.0, palette::border().to_egui());
        for (from, to) in [
            (frame.left_top(), frame.right_top()),
            (frame.right_top(), frame.right_bottom()),
            (frame.right_bottom(), frame.left_bottom()),
            (frame.left_bottom(), frame.left_top()),
        ] {
            painter.line_segment([from, to], border);
        }
        let Some(pattern) = self.swatch.as_ref().filter(|pattern| !pattern.draws_nothing()) else {
            let said = if self.swatch_note.is_empty() {
                "Draws nothing"
            } else {
                self.swatch_note.as_str()
            };
            painter.text(
                frame.center(),
                egui::Align2::CENTER_CENTER,
                said,
                egui::FontId::default(),
                palette::text_muted().to_egui(),
            );
            return;
        };
        if pattern.solid {
            painter.rect_filled(frame.shrink(1.0), 0.0, palette::accent().to_egui());
            return;
        }
        let mut widest = 0.0;
        for family in &pattern.families {
            widest = katana_math::cpp::max(widest, family.spacing);
        }
        let half = 4.0 * widest;
        let square = Polyline2 {
            vertices: vec![
                Point2::new(-half, -half),
                Point2::new(half, -half),
                Point2::new(half, half),
                Point2::new(-half, half),
            ],
            closed: true,
            ..Polyline2::default()
        };
        let scale = f64::from(side) / (2.0 * half);
        let at = |point: Point2| {
            pos2(
                frame.center().x + (point.x * scale) as f32,
                frame.center().y - (point.y * scale) as f32,
            )
        };
        let ink = egui::Stroke::new(1.0, palette::accent().to_egui());
        for segment in hatch_segments(&square, pattern) {
            painter.line_segment([at(segment.start), at(segment.end)], ink);
        }
    }
}
