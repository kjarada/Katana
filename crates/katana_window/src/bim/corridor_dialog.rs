//! BIM > Corridors... (docs/road_design.md, "In the window"): the drawing's corridors - a road
//! template swept along an alignment and its design profile - their regions, their
//! superelevation, the surface their batters meet, and what they build: the 3D strings and the
//! finished and subgrade surfaces. The port of bim/corridor_dialog.
//!
//! NOTHING IS CHANGED HERE. Every button builds the CORRIDOR line a person would type and hands it
//! to the window's one executor (host::Host::run), so it is echoed, kept in the history and one undo
//! step. What it shows is Document::corridors and the road model's problems (bim::bim_model), so
//! the dialog and CORRIDOR INFO / PROBLEMS agree. Non-modal, kept by the window, reloaded through a
//! DocumentWatcher.
//!
//! THE FIELDS ARE A BUFFER, applied by Apply. Choosing a corridor fills them from it; a reload
//! (after any line, an undo, a redo, a new or opened drawing) refills them only when the chosen
//! corridor itself changed, so a typed interval survives an unrelated command and a changed
//! drawing is shown as it is. The combos show the name the corridor holds - found ignoring case, as
//! the verbs find it, or, for a surface that was removed or an alignment that lost its profile, as
//! an extra item - so Apply cannot re-point a corridor to whatever item happened to be showing.
//!
//! Object names:
//!
//! ```text
//! corridorDialog          the dialog (the action bimCorridors' data)
//! corridorList            Corridor, Alignment, Template, Surface, Interval, From, To,
//!                         Regions, Superelevation, Curve tolerance
//! corridorName            the name New and Rename use
//! corridorAlignment       the alignments with a design profile
//! corridorTemplate        the road templates
//! corridorSurface         (none) and the drawing's surfaces
//! corridorInterval, corridorFrom, corridorTo   metres; From and To may be empty
//! corridorCurveTolerance  the relative error of a course's area and volume on a curve (0.001;
//!                         0 adds no sections inside curves)
//! corridorNew             CORRIDOR NEW "name" ALIGNMENT a TEMPLATE t [surface=] interval=
//!                         [from=] [to=] [curvetol= when it is not the default]
//! corridorApply           CORRIDOR SET <chosen> alignment= template= surface= interval=
//!                         from= to= [curvetol= when it differs from the corridor's]
//! corridorRename          CORRIDOR RENAME <chosen> "name"
//! corridorDelete          CORRIDOR DELETE <chosen>
//! corridorRegions         From, To, Template
//! corridorRegionFrom, corridorRegionTo, corridorRegionTemplate
//! corridorRegionAdd       CORRIDOR REGION <c> ADD from to template
//! corridorRegionRemove    CORRIDOR REGION <c> REMOVE <the chosen row's from>
//! corridorSuperelevation  Station, Left %, Right %
//! corridorSuperStation, corridorSuperLeft, corridorSuperRight
//! corridorSuperAdd        CORRIDOR SUPERELEVATION <c> ADD station left right
//! corridorSuperRemove     CORRIDOR SUPERELEVATION <c> REMOVE <the chosen row's station>
//! corridorBuild           CORRIDOR BUILD <c>: sections, objects, cut and fill
//! corridorStrings         CORRIDOR STRINGS <c>: the 3D strings, as polylines
//! corridorSurfaces        CORRIDOR SURFACE <c>: the finished and subgrade surfaces
//! corridorProblems        what the chosen corridor could not build, a line each
//! corridorStatus          what the last line replied, or why it was refused
//! corridorClose
//! ```

use egui::Ui;
use katana_cad::Document;
use katana_cad::bim::bim_object::bim_model;
use katana_cad::road::corridor_model::{Corridor, DEFAULT_CURVE_TOLERANCE};
use katana_core::text::equals_ignoring_case;

use super::road_dialog_support::{
    Combo, StatusLine, Table, button, cell, field, muted, number, option, same_ignoring_case, word,
};
use crate::command_word::exact_number;
use crate::customisation::document_watcher::DocumentWatcher;
use crate::host::Host;
use crate::shell::dialog::DialogFrame;

/// What the surface choice shows for no target surface.
pub const NO_SURFACE: &str = "(none)";

fn station_text(station: Option<f64>) -> String {
    station.map(exact_number).unwrap_or_default()
}

/// Makes `combo` show `stored`: its item ignoring case - the verb finds a template, an alignment or
/// a surface that way (RoadTemplateSet::find, SurfaceStore::find), so a corridor naming "URBAN
/// STREET" is valid against a template called "urban street" - or, when it has none (a surface
/// since removed, an alignment that lost its design profile), an extra item holding the name.
/// QComboBox::setCurrentText does neither: on a combo that is not editable it keeps the item
/// showing before, and Apply then wrote THAT item over what the corridor names (docs/
/// road_design.md, "In the window"). (The C++ gave the extra item a tooltip; F0's choice draws its
/// items itself and has none.)
pub fn show_stored(combo: &mut Combo, stored: &str) {
    let found = combo
        .find_text(stored)
        .or_else(|| combo.find_text_ignoring_case(stored));
    let at = match found {
        Some(at) => at,
        None => {
            combo.add_item(stored);
            combo.count() - 1
        }
    };
    combo.set_current_index(Some(at));
}

/// What a refilled combo keeps of what it showed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keep {
    /// The first item: the caller is about to say what to show.
    Nothing,
    /// What it showed, when that is still one of the items.
    IfPresent,
    /// What it showed; a name no longer among the items stays, as an extra item, so that the combo
    /// never flips to some other item behind an unapplied edit or a name the corridor holds.
    Always,
}

/// Refills a combo with `items`.
fn refill(combo: &mut Combo, items: &[String], keep: Keep) {
    let shown = combo.current_text();
    combo.clear();
    combo.add_items(items);
    if !shown.is_empty() {
        match keep {
            Keep::Always => show_stored(combo, &shown),
            Keep::IfPresent => {
                if let Some(at) = combo.find_text(&shown) {
                    combo.set_current_index(Some(at));
                }
            }
            Keep::Nothing => {}
        }
    }
}

/// What Apply writes for a name a combo shows: the spelling the corridor already holds when it is
/// the same name in another case, so that Apply round-trips the stored text and re-points nothing.
fn spelled_as(shown: &str, stored: Option<&str>) -> String {
    match stored {
        Some(stored) if equals_ignoring_case(shown.as_bytes(), stored.as_bytes()) => {
            String::from(stored)
        }
        _ => String::from(shown),
    }
}

/// A field that must hold one number (or may be empty), as the line takes it; None, said on the
/// status line, for anything else.
fn number_field(
    status: &mut StatusLine,
    text: &str,
    what: &str,
    may_be_empty: bool,
) -> Option<String> {
    let text = text.trim();
    if text.is_empty() && may_be_empty {
        return Some(String::new());
    }
    if number(text).is_none() {
        status.set(
            format!("The {what} '{text}' is not a number; type one number, with a decimal point"),
            true,
        );
        return None;
    }
    Some(String::from(text))
}

/// The dialog's fields and what it keeps between frames (the C++'s Impl).
struct Body {
    watcher: DocumentWatcher,
    executor: bool,
    list: Table,
    name: String,
    alignment: Combo,
    road_template: Combo,
    surface: Combo,
    interval: String,
    curve_tolerance: String,
    from: String,
    to: String,
    regions: Table,
    region_from: String,
    region_to: String,
    region_template: Combo,
    superelevation: Table,
    super_station: String,
    super_left: String,
    super_right: String,
    problems: String,
    status: StatusLine,
    loading: bool,
    /// The stored corridor the definition fields were last filled from. The fields are a buffer,
    /// applied by Apply: a reload that found the chosen corridor as it was keeps what was typed, as
    /// the Road Template Manager's grids do; one that found it changed (an undo, a typed line, a
    /// new drawing) shows what the drawing holds.
    filled_from: Option<Corridor>,
    /// The drawing's alignments with a design profile, road templates and surfaces, as reload last
    /// read them: what the combos are filled with.
    alignment_items: Vec<String>,
    template_items: Vec<String>,
    surface_items: Vec<String>,
    close: bool,
}

impl Body {
    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    fn current_corridor(&self) -> String {
        self.list
            .current_row()
            .map(|row| self.list.text(row, 0))
            .unwrap_or_default()
    }

    fn current(&self, document: &Document) -> Option<Corridor> {
        if !self.alive() {
            return None;
        }
        let chosen = self.current_corridor();
        if chosen.is_empty() {
            return None;
        }
        document.corridors().find(&chosen).cloned()
    }

    fn corridor_word(&mut self) -> Option<String> {
        let chosen = self.current_corridor();
        if chosen.is_empty() {
            self.status
                .set("Choose a corridor first, or make one with New.", true);
            return None;
        }
        word(&mut self.status, &chosen, "the corridor's name")
    }

    fn run(&mut self, host: &mut dyn Host, line: &str) -> bool {
        if !self.alive() {
            self.status
                .set("The drawing this dialog edited is gone.", true);
            return false;
        }
        let ok = super::road_dialog_support::run(host, self.executor, &mut self.status, line);
        self.reload(host.document());
        ok
    }

    /// The row of the corridor `name`, chosen; with `handler`, the selection's handler run as
    /// itemSelectionChanged ran it (not while loading).
    fn select_named(&mut self, document: &Document, name: &str) -> bool {
        let Some(row) = (0..self.list.row_count())
            .find(|&row| same_ignoring_case(&self.list.text(row, 0), name))
        else {
            return false;
        };
        if self.list.select_row(row) && !self.loading {
            self.fill_chosen(document, true);
        }
        true
    }

    /// alignment= template= surface= interval= from= to= [curvetol=], from the fields. `stored`
    /// is the corridor being set, or None for a new one: a name the combos show in another case
    /// than the corridor holds it is written as the corridor holds it.
    fn definition(&mut self, named: bool, stored: Option<&Corridor>) -> Option<String> {
        let alignment_name = spelled_as(
            &self.alignment.current_text(),
            stored.map(|corridor| corridor.alignment.as_str()),
        );
        let template_name = spelled_as(
            &self.road_template.current_text(),
            stored.map(|corridor| corridor.template_name.as_str()),
        );
        let alignment_word = word(&mut self.status, &alignment_name, "the alignment's name");
        let template_word = word(&mut self.status, &template_name, "the template's name");
        let (Some(alignment_word), Some(template_word)) = (alignment_word, template_word) else {
            return None;
        };
        if self.alignment.current_text().is_empty() || self.road_template.current_text().is_empty()
        {
            self.status.set(
                "A corridor needs an alignment with a design profile and a road template: \
                 Terrain > Alignment Manager, and Road Template Manager",
                true,
            );
            return None;
        }
        let step = number_field(&mut self.status, &self.interval, "interval", false);
        let start = number_field(&mut self.status, &self.from, "start station", true);
        let end = number_field(&mut self.status, &self.to, "end station", true);
        let tolerance = number_field(
            &mut self.status,
            &self.curve_tolerance,
            "curve tolerance",
            false,
        );
        let (Some(step), Some(start), Some(end), Some(tolerance)) = (step, start, end, tolerance)
        else {
            return None;
        };
        let shown_surface = self.surface.current_text();
        let surface_name = if shown_surface == NO_SURFACE {
            String::new()
        } else {
            spelled_as(
                &shown_surface,
                stored.map(|corridor| corridor.surface.as_str()),
            )
        };
        let surface_option = option(&mut self.status, "surface", &surface_name)?;
        let mut text = if named {
            format!("ALIGNMENT {alignment_word} TEMPLATE {template_word}")
        } else {
            format!("alignment={alignment_word} template={template_word}")
        };
        if named {
            if !surface_name.is_empty() {
                text.push(' ');
                text.push_str(&surface_option);
            }
            text.push_str(&format!(" interval={step}"));
            if !start.is_empty() {
                text.push_str(&format!(" from={start}"));
            }
            if !end.is_empty() {
                text.push_str(&format!(" to={end}"));
            }
        } else {
            // SET writes every key but the curve tolerance (below), an empty station clearing it.
            text.push_str(&format!(
                " {surface_option} interval={step} from={start} to={end}"
            ));
        }
        // The curve tolerance only when it is not what the corridor has (a new one has the
        // default): the field is refilled from the corridor, so an unedited one says nothing the
        // line needs to, and the line stays short.
        let held = stored.map_or(DEFAULT_CURVE_TOLERANCE, |corridor| corridor.curve_tolerance);
        if number(&tolerance) != Some(held) {
            text.push_str(&format!(" curvetol={tolerance}"));
        }
        Some(text)
    }

    fn refill_named(&mut self, keep: Keep) {
        refill(&mut self.alignment, &self.alignment_items, keep);
        refill(&mut self.road_template, &self.template_items, keep);
        refill(&mut self.surface, &self.surface_items, keep);
        // The region's template is a choice, not a name a corridor holds.
        refill(
            &mut self.region_template,
            &self.template_items,
            Keep::IfPresent,
        );
    }

    fn reload(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let mut alignments = Vec::new();
        document.model().alignments.for_each(|each| {
            if each.vertical.is_some() {
                alignments.push(each.name.clone());
            }
        });
        let templates: Vec<String> = document
            .road_templates()
            .templates
            .iter()
            .map(|each| each.name.clone())
            .collect();
        let mut surfaces = vec![String::from(NO_SURFACE)];
        surfaces.extend(
            document
                .surfaces()
                .all()
                .iter()
                .map(|each| each.name.clone()),
        );
        let no_alignment = alignments.is_empty();
        self.alignment_items = alignments;
        self.template_items = templates;
        self.surface_items = surfaces;
        // The three the corridor itself names keep a name the drawing no longer offers, as an
        // extra item (show_stored); the region's is only a choice.
        self.refill_named(Keep::Always);

        let chosen = self.current_corridor();
        self.loading = true;
        self.list.clear();
        for each in &document.corridors().corridors {
            self.list.push_row(vec![
                cell(each.name.as_str()),
                cell(each.alignment.as_str()),
                cell(each.template_name.as_str()),
                cell(each.surface.as_str()),
                cell(exact_number(each.interval)),
                cell(station_text(each.start)),
                cell(station_text(each.end)),
                cell(format!("{}", each.regions.len())),
                cell(format!("{}", each.superelevation.len())),
                cell(exact_number(each.curve_tolerance)),
            ]);
        }
        if !chosen.is_empty() {
            self.select_named(document, &chosen);
        }
        self.loading = false;
        self.fill_chosen(document, false);
        if no_alignment {
            self.status.set(
                "No alignment has a design profile. Define one in Terrain > Alignment Manager \
                 (Vertical), or with ALIGN DESIGN name s,z ...",
                true,
            );
        }
    }

    /// `force` fills the definition fields from the chosen corridor whatever they hold: a choice
    /// of another corridor, as against a reload.
    fn fill_chosen(&mut self, document: &Document, force: bool) {
        self.regions.clear();
        self.superelevation.clear();
        self.problems.clear();
        let Some(corridor) = self.current(document) else {
            self.filled_from = None;
            return;
        };
        if force || self.filled_from.as_ref() != Some(&corridor) {
            // From the drawing's own names, so an extra item for a name the last corridor held
            // and this one does not is not left in the lists.
            self.refill_named(Keep::Nothing);
            self.name = corridor.name.clone();
            show_stored(&mut self.alignment, &corridor.alignment);
            show_stored(&mut self.road_template, &corridor.template_name);
            let surface = if corridor.surface.is_empty() {
                NO_SURFACE
            } else {
                corridor.surface.as_str()
            };
            show_stored(&mut self.surface, surface);
            self.interval = exact_number(corridor.interval);
            self.curve_tolerance = exact_number(corridor.curve_tolerance);
            self.from = station_text(corridor.start);
            self.to = station_text(corridor.end);
            self.filled_from = Some(corridor.clone());
        }
        for region in &corridor.regions {
            self.regions.push_row(vec![
                cell(exact_number(region.start)),
                cell(exact_number(region.end)),
                cell(region.template_name.as_str()),
            ]);
        }
        for point in &corridor.superelevation {
            self.superelevation.push_row(vec![
                cell(exact_number(point.station)),
                cell(exact_number(point.left)),
                cell(exact_number(point.right)),
            ]);
        }
        // What it could not build, from the road model the views draw.
        let source = format!("corridor {}", corridor.name);
        let lines: Vec<String> = bim_model(document)
            .problems
            .iter()
            .filter(|problem| problem.source == source)
            .map(|problem| problem.message.clone())
            .collect();
        self.problems = if lines.is_empty() {
            String::from("No problems: everything it names is there.")
        } else {
            lines.join("\n")
        };
    }

    // ---- what each button does ----------------------------------------------------------------

    /// New and Rename take the name from the field BEFORE the line runs: the reload that follows
    /// it refills the field from the corridor still chosen, so reading it afterwards would choose
    /// that corridor again.
    fn press_new(&mut self, host: &mut dyn Host) {
        let new_name = String::from(self.name.trim());
        let new_word = word(&mut self.status, &new_name, "the new name");
        let rest = self.definition(true, None);
        if let (Some(new_word), Some(rest)) = (new_word, rest)
            && self.run(host, &format!("CORRIDOR NEW {new_word} {rest}"))
        {
            self.select_named(host.document(), &new_name);
        }
    }

    fn press_apply(&mut self, host: &mut dyn Host) {
        let Some(corridor_word) = self.corridor_word() else {
            return;
        };
        let stored = self.current(host.document());
        if let Some(rest) = self.definition(false, stored.as_ref()) {
            self.run(host, &format!("CORRIDOR SET {corridor_word} {rest}"));
        }
    }

    fn press_rename(&mut self, host: &mut dyn Host) {
        let new_name = String::from(self.name.trim());
        let corridor_word = self.corridor_word();
        let new_word = word(&mut self.status, &new_name, "the new name");
        if let (Some(corridor_word), Some(new_word)) = (corridor_word, new_word)
            && self.run(host, &format!("CORRIDOR RENAME {corridor_word} {new_word}"))
        {
            self.select_named(host.document(), &new_name);
        }
    }

    /// A line naming the chosen corridor: `verb` and the word.
    fn press_named(&mut self, host: &mut dyn Host, verb: &str) {
        if let Some(corridor_word) = self.corridor_word() {
            self.run(host, &format!("{verb} {corridor_word}"));
        }
    }

    fn press_region_add(&mut self, host: &mut dyn Host) {
        let Some(corridor_word) = self.corridor_word() else {
            return;
        };
        let start = number_field(&mut self.status, &self.region_from, "region's start", false);
        let end = number_field(&mut self.status, &self.region_to, "region's end", false);
        let template_name = self.region_template.current_text();
        let template_word = word(&mut self.status, &template_name, "the region's template");
        if let (Some(start), Some(end), Some(template_word)) = (start, end, template_word) {
            self.run(
                host,
                &format!("CORRIDOR REGION {corridor_word} ADD {start} {end} {template_word}"),
            );
        }
    }

    fn press_region_remove(&mut self, host: &mut dyn Host) {
        let corridor_word = self.corridor_word();
        let row = self.regions.current_row();
        let Some(corridor_word) = corridor_word else {
            return;
        };
        let Some(row) = row else {
            self.status.set("Choose the region to remove.", true);
            return;
        };
        let from = self.regions.text(row, 0);
        self.run(
            host,
            &format!("CORRIDOR REGION {corridor_word} REMOVE {from}"),
        );
    }

    fn press_super_add(&mut self, host: &mut dyn Host) {
        let Some(corridor_word) = self.corridor_word() else {
            return;
        };
        let station = number_field(&mut self.status, &self.super_station, "station", false);
        let left = number_field(&mut self.status, &self.super_left, "left crossfall", false);
        let right = number_field(
            &mut self.status,
            &self.super_right,
            "right crossfall",
            false,
        );
        if let (Some(station), Some(left), Some(right)) = (station, left, right) {
            self.run(
                host,
                &format!("CORRIDOR SUPERELEVATION {corridor_word} ADD {station} {left} {right}"),
            );
        }
    }

    fn press_super_remove(&mut self, host: &mut dyn Host) {
        let corridor_word = self.corridor_word();
        let row = self.superelevation.current_row();
        let Some(corridor_word) = corridor_word else {
            return;
        };
        let Some(row) = row else {
            self.status.set("Choose the entry to remove.", true);
            return;
        };
        let station = self.superelevation.text(row, 0);
        self.run(
            host,
            &format!("CORRIDOR SUPERELEVATION {corridor_word} REMOVE {station}"),
        );
    }

    // ---- drawing --------------------------------------------------------------------------------

    fn body_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.list.ui(ui).selection_changed && !self.loading {
            self.fill_chosen(host.document(), true);
        }

        // ---- the definition ----
        ui.group(|ui| {
            ui.strong("Corridor");
            egui::Grid::new("corridorDefinition").show(ui, |ui| {
                ui.label("Name");
                field(
                    ui,
                    "corridorName",
                    &mut self.name,
                    "name",
                    "The name New and Rename give a corridor",
                );
                ui.label("Alignment");
                self.alignment.ui(ui, "corridorAlignment");
                ui.end_row();
                ui.label("Template");
                self.road_template.ui(ui, "corridorTemplate");
                ui.label("Target surface");
                self.surface.ui(ui, "corridorSurface");
                ui.end_row();
                ui.label("Interval (m)");
                field(
                    ui,
                    "corridorInterval",
                    &mut self.interval,
                    "10",
                    "Metres between sections",
                );
                ui.label("Stations");
                ui.horizontal(|ui| {
                    field(
                        ui,
                        "corridorFrom",
                        &mut self.from,
                        "start",
                        "The first station; empty: where it begins",
                    );
                    field(
                        ui,
                        "corridorTo",
                        &mut self.to,
                        "end",
                        "The last station; empty: where it ends",
                    );
                });
                ui.end_row();
                ui.label("Curve tolerance");
                field(
                    ui,
                    "corridorCurveTolerance",
                    &mut self.curve_tolerance,
                    "0.001",
                    "The most a course's area and volume on a curve may be out, as a fraction \
                     (0.001 is 0.1 %): sections are added inside a curve until the chords are \
                     that close. 0 adds none",
                );
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if button(
                    ui,
                    "corridorNew",
                    "New",
                    "CORRIDOR NEW: this alignment and template, with the layers its objects go on",
                )
                .clicked()
                {
                    self.press_new(host);
                }
                if button(
                    ui,
                    "corridorApply",
                    "Apply",
                    "CORRIDOR SET: the chosen corridor as the fields say",
                )
                .clicked()
                {
                    self.press_apply(host);
                }
                if button(
                    ui,
                    "corridorRename",
                    "Rename",
                    "CORRIDOR RENAME: the chosen corridor",
                )
                .clicked()
                {
                    self.press_rename(host);
                }
                if button(
                    ui,
                    "corridorDelete",
                    "Delete",
                    "CORRIDOR DELETE: the chosen corridor",
                )
                .clicked()
                {
                    self.press_named(host, "CORRIDOR DELETE");
                }
            });
        });

        // ---- regions and superelevation, side by side ----
        ui.horizontal_top(|ui| {
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.strong("Regions: another template between two stations");
                    self.regions.ui(ui);
                    ui.horizontal(|ui| {
                        field(
                            ui,
                            "corridorRegionFrom",
                            &mut self.region_from,
                            "from",
                            "Where the region starts",
                        );
                        field(
                            ui,
                            "corridorRegionTo",
                            &mut self.region_to,
                            "to",
                            "Where it ends",
                        );
                        self.region_template.ui(ui, "corridorRegionTemplate");
                        if button(ui, "corridorRegionAdd", "Add", "CORRIDOR REGION ... ADD")
                            .clicked()
                        {
                            self.press_region_add(host);
                        }
                        if button(
                            ui,
                            "corridorRegionRemove",
                            "Remove",
                            "CORRIDOR REGION ... REMOVE: the chosen region",
                        )
                        .clicked()
                        {
                            self.press_region_remove(host);
                        }
                    });
                });
            });
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.strong("Superelevation: crossfall each side, percent outward");
                    self.superelevation.ui(ui);
                    ui.horizontal(|ui| {
                        field(
                            ui,
                            "corridorSuperStation",
                            &mut self.super_station,
                            "station",
                            "The station",
                        );
                        field(
                            ui,
                            "corridorSuperLeft",
                            &mut self.super_left,
                            "left %",
                            "The left side's crossfall there, outward: -3 falls",
                        );
                        field(
                            ui,
                            "corridorSuperRight",
                            &mut self.super_right,
                            "right %",
                            "The right side's crossfall there, outward",
                        );
                        if button(
                            ui,
                            "corridorSuperAdd",
                            "Add",
                            "CORRIDOR SUPERELEVATION ... ADD (replacing one at the station)",
                        )
                        .clicked()
                        {
                            self.press_super_add(host);
                        }
                        if button(
                            ui,
                            "corridorSuperRemove",
                            "Remove",
                            "CORRIDOR SUPERELEVATION ... REMOVE: the chosen entry",
                        )
                        .clicked()
                        {
                            self.press_super_remove(host);
                        }
                    });
                });
            });
        });

        // ---- what it builds ----
        ui.horizontal(|ui| {
            if button(
                ui,
                "corridorBuild",
                "Build",
                "CORRIDOR BUILD: its sections, objects, cut and fill, and problems",
            )
            .clicked()
            {
                self.press_named(host, "CORRIDOR BUILD");
            }
            if button(
                ui,
                "corridorStrings",
                "Build Strings",
                "CORRIDOR STRINGS: its 3D strings as polylines, replacing the ones made before",
            )
            .clicked()
            {
                self.press_named(host, "CORRIDOR STRINGS");
            }
            if button(
                ui,
                "corridorSurfaces",
                "Build Surfaces",
                "CORRIDOR SURFACE: its finished and subgrade surfaces into the drawing's surfaces",
            )
            .clicked()
            {
                self.press_named(host, "CORRIDOR SURFACE");
            }
        });
        muted(ui, "corridorProblems", &self.problems);
        self.status.ui(ui, "corridorStatus");
    }

    /// The row of buttons the dialog frame keeps in sight: Close.
    fn footer_ui(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, "corridorClose", "Close", "Close the dialog").clicked() {
                self.close = true;
            }
        });
    }
}

/// The Corridors dialog (see the module comment).
pub struct CorridorDialog {
    frame: DialogFrame,
    open: bool,
    body: Body,
}

impl CorridorDialog {
    /// The dialog over `document`, shown, its first corridor chosen.
    pub fn new(document: &Document) -> Self {
        let mut body = Body {
            watcher: DocumentWatcher::new(document),
            executor: true,
            list: Table::new(
                "corridorList",
                &[
                    "Corridor",
                    "Alignment",
                    "Template",
                    "Surface",
                    "Interval",
                    "From",
                    "To",
                    "Regions",
                    "Superelevation",
                    "Curve tolerance",
                ],
            )
            .max_height(140.0),
            name: String::new(),
            alignment: Combo::new(),
            road_template: Combo::new(),
            surface: Combo::new(),
            interval: String::from("10"),
            curve_tolerance: exact_number(DEFAULT_CURVE_TOLERANCE),
            from: String::new(),
            to: String::new(),
            regions: Table::new("corridorRegions", &["From", "To", "Template"]),
            region_from: String::new(),
            region_to: String::new(),
            region_template: Combo::new(),
            superelevation: Table::new(
                "corridorSuperelevation",
                &["Station", "Left (%)", "Right (%)"],
            ),
            super_station: String::new(),
            super_left: String::new(),
            super_right: String::new(),
            problems: String::new(),
            status: StatusLine::default(),
            loading: false,
            filled_from: None,
            alignment_items: Vec::new(),
            template_items: Vec::new(),
            surface_items: Vec::new(),
            close: false,
        };
        body.reload(document);
        if body.list.row_count() > 0 && body.list.select_row(0) {
            body.fill_chosen(document, true);
        }
        Self {
            frame: DialogFrame::new("corridorDialog", "Corridors"),
            open: true,
            body,
        }
    }

    /// A dialog with no executor (the C++'s empty CommandRunner): it says it cannot run a line.
    pub fn without_executor(mut self) -> Self {
        self.body.executor = false;
        self
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// QWidget::show: shown again where it was.
    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn hide(&mut self) {
        self.open = false;
    }

    /// Chooses the corridor `name` (ignoring case); false when there is none.
    pub fn select_corridor(&mut self, document: &Document, name: &str) -> bool {
        self.body.select_named(document, name)
    }

    /// The chosen corridor's name; empty when there is none.
    pub fn current_corridor(&self) -> String {
        self.body.current_corridor()
    }

    /// The tables, by their object names: corridorList, corridorRegions, corridorSuperelevation.
    pub fn table(&self, name: &str) -> Option<&Table> {
        [
            &self.body.list,
            &self.body.regions,
            &self.body.superelevation,
        ]
        .into_iter()
        .find(|table| table.name() == name)
    }

    /// Draws the dialog for this frame, when it is shown; then the watcher's delivery.
    ///
    /// The delivery is the C++'s from the event loop: after what a person did in the frame, as a
    /// click ran before the event loop delivered, and seen in the frame it asks for.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.open {
            let ctx = ui.ctx().clone();
            let mut lent = (&mut self.body, &mut *host);
            let shown = self.frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body_ui(ui, &mut *lent.1),
                |ui, lent| lent.0.footer_ui(ui),
            );
            if shown.closed || std::mem::take(&mut self.body.close) {
                self.open = false;
            }
        }
        if let Some(changes) = self.body.watcher.deliver(host.document())
            && changes.model
        {
            self.body.reload(host.document());
            ui.ctx().request_repaint();
        }
    }
}
