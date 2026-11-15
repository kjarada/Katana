//! File > Project Coordinate System: the project's coordinate system chosen from the common list,
//! from what suits a place, or typed (an EPSG code, WKT or a PROJ string), each checked as it is
//! typed (docs/cad.md, "The project's coordinate system"). The port of project_crs_dialog.
//!
//! The place is given by whoever opens it, or typed as a longitude and latitude (projectCrsPlace,
//! then projectCrsSuggest) - filled in with the drawing's centre when the project has a system to
//! say where that is.
//!
//! A thin front end over the CRS verb: Set builds "CRS SET <text>" (Local Coordinates "CRS
//! CLEAR") and hands it to the window's one executor, so OK is ONE undo step, logged as if typed,
//! and everything here can be done without it (CRS SET, CRS FIND, CRS SUGGEST). It once called
//! Document::setCoordinateSystem itself, which is how a pasted WKT worked here while the same text
//! typed as CRS SET was refused. The status bar's coordinate-system button and GIS > Online Data
//! open it too. (The C++ ran the line on an interpreter of its own when it was given no runner, a
//! test's; here the host always runs it, and a test's host runs it on its own interpreter.)
//!
//! Non-modal, it follows the drawing while it is open: a system set from elsewhere (a typed CRS
//! SET, an undo) is shown in projectCrsCurrent, and in projectCrsText unless something else has
//! been typed there; a new or opened drawing is shown afresh - its system, its place, no place the
//! opener gave for the drawing before. Left open across an OPEN, it once showed the old project's
//! system and Set put it on the new one. The drawing's notifications are kept as they come and
//! followed when the dialog next draws, never inside the notification.
//!
//! Object names: projectCrsDialog; projectCrsCurrent, projectCrsSearch, projectCrsPlace,
//! projectCrsSuggest, projectCrsList (the systems, grouped: a group's row is its title, a system's
//! its name and code; a fill makes the row of that name current, as a click does), projectCrsText,
//! projectCrsCheck, projectCrsApply (Set), projectCrsCancel, projectCrsClear (Local Coordinates).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use egui::{RichText, Ui};
use katana_cad::project_crs::{
    describe_coordinate_system, find_coordinate_systems, suggest_coordinate_systems,
};
use katana_cad::{CrsChoice, Document, DocumentChange, ListenerHandle};
use katana_core::text::parse_finite_double;
use katana_gis::reproject::transform_point;

use crate::drive::controls::{self, LineOptions};
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::shell::DialogFrame;

/// The title of the group a place's systems head the list in.
const SUGGESTED: &str = "Suggested for this place";

/// One heading of the list and the systems under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrsGroup {
    pub title: String,
    pub choices: Vec<CrsChoice>,
    /// Shown open: a search shows every match; the whole list opens the suggestions and
    /// Australia, and leaves the 120 UTM zones folded.
    pub expanded: bool,
}

/// A row of the list: a group's, or a system's under it.
pub type CrsItem = (usize, Option<usize>);

/// "151.21, -33.87" (or with blanks between) as a longitude and a latitude; None for anything
/// else. The range is suggest_coordinate_systems's to judge, so it can say which number is wrong.
fn parse_place(text: &str) -> Option<(f64, f64)> {
    // The C++'s "[,\s]+": a comma, or what PCRE's \s takes without Unicode properties.
    let parts: Vec<&str> = text
        .trim()
        .split(|character: char| {
            matches!(
                character,
                ',' | ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r'
            )
        })
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() != 2 {
        return None;
    }
    // QString::toDouble's C reading of a number; its IEEE words ("inf", "nan") read as no number
    // here, which says "Not a place" as the range check after it would.
    Some((
        parse_finite_double(parts[0])?,
        parse_finite_double(parts[1])?,
    ))
}

/// Where the drawing is, as a longitude and a latitude: its centre, taken from the project's
/// system to WGS 84 in the longitude-first order the reprojection keeps. None for a project with
/// no system or an empty drawing, which have no place to give.
fn drawing_place(document: &Document) -> Option<(f64, f64)> {
    let crs = &document.metadata().coordinate_system;
    let extent = document.model().entities.bounds();
    if crs.is_empty() || extent.empty() {
        return None;
    }
    let centre = extent.center();
    let lon_lat = transform_point(centre.x, centre.y, crs, "EPSG:4326").ok()?;
    Some((lon_lat[0], lon_lat[1]))
}

/// "EPSG:7856  GDA2020 / MGA zone 56", or "no coordinate system" - what the status bar and the
/// online data dialog show.
pub fn project_crs_label(document: &Document) -> String {
    let stored = &document.metadata().coordinate_system;
    if stored.is_empty() {
        return String::from("no coordinate system");
    }
    match describe_coordinate_system(stored) {
        Ok(description) => format!("{}  {}", description.id, description.name),
        Err(_) => format!("{stored} (not recognised)"),
    }
}

/// What `typed` is as a coordinate system, for a check line under a box it is typed in: its code,
/// name, kind, units and area of use; "Not a coordinate system: ..." with the reason when it names
/// none; local coordinates when it is empty. With `place` (a longitude and latitude) an area of
/// use that does not hold it is said too. What projectCrsCheck and the start screen's
/// startCrsCheck say, in the same words.
pub fn crs_check_text(typed: &str, place: Option<(f64, f64)>) -> String {
    let typed = typed.trim();
    if typed.is_empty() {
        return String::from(
            "Local coordinates: no coordinate system. Online data and reprojection need one.",
        );
    }
    let description = match describe_coordinate_system(typed) {
        Ok(description) => description,
        Err(error) => return format!("Not a coordinate system: {}", error.message),
    };
    let mut line = format!(
        "{}  {} - {}, {}",
        description.id, description.name, description.kind, description.units
    );
    if let Some(area) = description.area_of_use {
        line.push_str(&format!(
            ", for {} to {} E, {} to {} N",
            qt_fixed(area.west, 1),
            qt_fixed(area.east, 1),
            qt_fixed(area.south, 1),
            qt_fixed(area.north, 1)
        ));
        if let Some((longitude, latitude)) = place
            && !area.contains(latitude, longitude)
        {
            line.push_str(" - NOT where this place is");
        }
    }
    line
}

/// File > Project Coordinate System (see the module comment).
pub struct ProjectCrsDialog {
    // First, so it is dropped first: no notification reaches a half-dropped dialog.
    _listener: ListenerHandle,
    notified: Arc<AtomicBool>,
    /// The parts of every change notified since the last frame.
    parts: Arc<AtomicU32>,
    frame: DialogFrame,
    visible: bool,
    place: Option<(f64, f64)>,
    /// The project's system as projectCrsText was last filled with it: while the box still says
    /// it, nothing has been typed there to keep.
    shown_system: String,
    current: String,
    search: String,
    place_text: String,
    groups: Vec<CrsGroup>,
    current_item: Option<CrsItem>,
    text: String,
    check: String,
    /// The project's system when the dialog was made: chooseProjectCrs's answer is whether it
    /// changed.
    before: String,
    /// Set (true) or Cancel (false), once one closed it.
    finished: Option<bool>,
}

impl ProjectCrsDialog {
    /// The dialog over `document`. `place` is a longitude and latitude to suggest systems for (the
    /// area a GIS import is about to fetch); without one, the list alone.
    pub fn new(document: &Document, place: Option<(f64, f64)>) -> Self {
        let notified = Arc::new(AtomicBool::new(false));
        let parts = Arc::new(AtomicU32::new(0));
        let listener = {
            let notified = Arc::clone(&notified);
            let parts = Arc::clone(&parts);
            document.add_change_listener(move |change: &DocumentChange<'_>| {
                parts.fetch_or(change.parts, Ordering::AcqRel);
                notified.store(true, Ordering::Release);
            })
        };
        let shown_system = document.metadata().coordinate_system.clone();
        let mut dialog = Self {
            _listener: listener,
            notified,
            parts,
            frame: DialogFrame::new("projectCrsDialog", "Project Coordinate System"),
            visible: false,
            place,
            text: shown_system.clone(),
            before: shown_system.clone(),
            shown_system,
            current: format!("Now: {}", project_crs_label(document)),
            search: String::new(),
            place_text: String::new(),
            groups: Vec::new(),
            current_item: None,
            check: String::new(),
            finished: None,
        };
        dialog.show_place(document);
        dialog.fill("");
        dialog.recheck();
        dialog
    }

    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.finished = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// What the text box holds.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What the check says of it.
    pub fn check(&self) -> &str {
        &self.check
    }

    /// What projectCrsCurrent says: "Now: " and the project's system.
    pub fn current_label(&self) -> &str {
        &self.current
    }

    /// What projectCrsPlace holds.
    pub fn place_text(&self) -> &str {
        &self.place_text
    }

    /// What projectCrsSearch holds.
    pub fn search(&self) -> &str {
        &self.search
    }

    /// The list, its groups in order.
    pub fn groups(&self) -> &[CrsGroup] {
        &self.groups
    }

    /// The text box set, as typing sets it: a change is checked.
    pub fn set_text(&mut self, text: &str) {
        if self.text != text {
            self.text = String::from(text);
            self.recheck();
        }
    }

    /// The search box set: the list filtered.
    pub fn set_search(&mut self, text: &str) {
        if self.search != text {
            self.search = String::from(text);
            let filter = self.search.clone();
            self.fill(&filter);
        }
    }

    pub fn set_place_text(&mut self, text: &str) {
        self.place_text = String::from(text);
    }

    /// A row of the list made current, as a click does: a system's fills the text box with its
    /// code; a group's leaves it.
    pub fn set_current(&mut self, item: Option<CrsItem>) {
        self.current_item = item;
        if let Some((group, Some(choice))) = item
            && let Some(id) = self
                .groups
                .get(group)
                .and_then(|found| found.choices.get(choice))
                .map(|found| found.id.clone())
            && !id.is_empty()
        {
            self.set_text(&id);
        }
    }

    /// The place box: the opener's place, else the drawing's centre, else empty.
    fn show_place(&mut self, document: &Document) {
        let known = self.place.or_else(|| drawing_place(document));
        self.place_text = match known {
            Some((longitude, latitude)) => {
                format!("{}, {}", qt_fixed(longitude, 6), qt_fixed(latitude, 6))
            }
            None => String::new(),
        };
    }

    /// The drawing's changes since the last frame. The system is set by an undoable step, which
    /// says only History, so every step is looked at - and passed over unless the system moved.
    pub fn follow(&mut self, document: &Document) {
        if !self.notified.swap(false, Ordering::AcqRel) {
            return;
        }
        let parts = self.parts.swap(0, Ordering::AcqRel);
        let replaced = DocumentChange::of(parts).has(DocumentChange::REPLACED);
        let stored = document.metadata().coordinate_system.clone();
        if !replaced && stored == self.shown_system {
            return;
        }
        self.current = format!("Now: {}", project_crs_label(document));
        // What was typed is kept through a change to this drawing's system; a replaced drawing
        // takes nothing of the last one's, the place its opener gave included.
        if replaced || self.text == self.shown_system {
            self.set_text(&stored);
        }
        self.shown_system = stored;
        if replaced {
            self.place = None;
            self.show_place(document);
            let filter = self.search.clone();
            self.fill(&filter);
            self.recheck();
        }
    }

    fn fill(&mut self, filter: &str) {
        self.groups.clear();
        self.current_item = None;
        // What suits the place first, when there is one and no search.
        if let Some((longitude, latitude)) = self.place
            && filter.trim().is_empty()
            && let Ok(suggested) = suggest_coordinate_systems(longitude, latitude)
        {
            self.groups.push(CrsGroup {
                title: String::from(SUGGESTED),
                choices: suggested,
                expanded: false,
            });
        }
        let mut groups: BTreeMap<String, usize> = BTreeMap::new();
        for entry in find_coordinate_systems(filter) {
            let at = match groups.get(&entry.group) {
                Some(at) => *at,
                None => {
                    self.groups.push(CrsGroup {
                        title: entry.group.clone(),
                        choices: Vec::new(),
                        expanded: false,
                    });
                    let at = self.groups.len() - 1;
                    groups.insert(entry.group.clone(), at);
                    at
                }
            };
            self.groups[at].choices.push(entry);
        }
        // A search shows every match; the whole list opens the suggestions and Australia, and
        // leaves the 120 UTM zones folded.
        let searched = !filter.trim().is_empty();
        for group in &mut self.groups {
            group.expanded = searched || !group.title.starts_with("World - WGS 84 UTM");
        }
    }

    fn recheck(&mut self) {
        self.check = crs_check_text(&self.text, self.place);
    }

    /// Suggest: the systems for the place typed in projectCrsPlace listed first, as CRS SUGGEST
    /// lists them. False, with the reason in the check line, when it is not a longitude and a
    /// latitude.
    pub fn suggest(&mut self) -> bool {
        let Some((longitude, latitude)) = parse_place(&self.place_text) else {
            self.check =
                String::from("Not a place: type a longitude and a latitude, e.g. 151.21, -33.87");
            return false;
        };
        if let Err(error) = suggest_coordinate_systems(longitude, latitude) {
            self.check = format!("Not a place: {}", error.message);
            return false;
        }
        self.place = Some((longitude, latitude));
        // The suggestions head the whole list, which a search would narrow.
        self.search.clear();
        self.fill("");
        self.recheck();
        true
    }

    /// Sets the project's coordinate system from the text box, as one step, by the CRS verb run
    /// through the window's one executor. False, with the verb's refusal in the check line, when
    /// it names no system. The text goes on the line as typed: CRS SET reads everything after SET
    /// verbatim, so a WKT keeps its quotes.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        let typed = self.text.trim();
        let line = if typed.is_empty() {
            String::from("CRS CLEAR")
        } else {
            format!("CRS SET {typed}")
        };
        let outcome = host.run(&line);
        if !outcome.ok {
            self.check = format!("Not set: {}", outcome.error);
            return false;
        }
        // projectCrsCurrent is follow's to update, from the step just taken.
        true
    }

    fn accept(&mut self) {
        self.finished = Some(true);
        self.hide();
    }

    fn reject(&mut self) {
        self.finished = Some(false);
        self.hide();
    }

    /// How the dialog last closed: Some(true) by Set or Local Coordinates, Some(false) by Cancel;
    /// None while it is open.
    pub fn finished(&self) -> Option<bool> {
        self.finished
    }

    /// Whether the project's coordinate system is other than it was when the dialog was made:
    /// chooseProjectCrs's answer, read once the dialog has finished.
    pub fn changed_since_opened(&self, document: &Document) -> bool {
        document.metadata().coordinate_system != self.before
    }

    /// The row whose first cell is `text`, the groups' and their systems' in the list's order, as
    /// a fill finds it (shown open or not).
    fn find_item(&self, text: &str) -> Option<CrsItem> {
        for (at, group) in self.groups.iter().enumerate() {
            if group.title == text {
                return Some((at, None));
            }
            if let Some(choice) = group.choices.iter().position(|entry| entry.name == text) {
                return Some((at, Some(choice)));
            }
        }
        None
    }

    /// The list's rows as --report reads a tree: down every group shown open, a row's cells joined
    /// by " | " (a group's code cell empty).
    pub fn list_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for group in &self.groups {
            rows.push(format!("{} | ", group.title));
            if group.expanded {
                for entry in &group.choices {
                    rows.push(format!("{} | {}", entry.name, entry.id));
                }
            }
        }
        rows
    }

    /// One frame of the dialog, when it is shown: the drawing's changes followed first.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.follow(host.document());
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui, &mut *lent.host),
            |ui, lent| lent.dialog.footer(ui, &mut *lent.host),
        );
        self.frame = frame;
        if shown.closed {
            self.reject();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.set_max_width(540.0);
        let current = self.current.clone();
        controls::label(ui, "projectCrsCurrent", &current);
        if controls::line_with(
            ui,
            "projectCrsSearch",
            &mut self.search,
            LineOptions {
                placeholder: "Search: mga 56, gda2020, utm 55s, 27700 ...",
                width: Some(520.0),
                ..LineOptions::default()
            },
        )
        .changed()
        {
            let filter = self.search.clone();
            self.fill(&filter);
        }
        // A place to suggest systems for, as CRS SUGGEST lon,lat does: the one the dialog was
        // opened with, or else the drawing's centre when the project's system can say where that
        // is.
        ui.horizontal(|ui| {
            ui.label("Suggest for a place:");
            let place = controls::line_with(
                ui,
                "projectCrsPlace",
                &mut self.place_text,
                LineOptions {
                    placeholder: "longitude, latitude - e.g. 151.21, -33.87",
                    width: Some(260.0),
                    ..LineOptions::default()
                },
            );
            let entered =
                place.lost_focus() && ui.ctx().input(|input| input.key_pressed(egui::Key::Enter));
            let pressed = controls::button(ui, "projectCrsSuggest", "Suggest")
                .on_hover_text("List the systems that suit this place first")
                .clicked();
            if entered || pressed {
                self.suggest();
            }
        });
        self.list_ui(ui, host);
        ui.label("Or type an EPSG code, WKT or a PROJ string (blank for local coordinates):");
        if controls::line_with(
            ui,
            "projectCrsText",
            &mut self.text,
            LineOptions {
                width: Some(520.0),
                ..LineOptions::default()
            },
        )
        .changed()
        {
            self.recheck();
        }
        let check = self.check.clone();
        controls::label_as(
            ui,
            "projectCrsCheck",
            &check,
            egui::Label::new(RichText::new(check.as_str())).wrap(),
        );
    }

    /// The list: each group a row that opens and folds it, its systems under it.
    fn list_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let name = "projectCrsList";
        let taken = take(ui, name);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(ui, taken, Err(fill_disabled(&taken.posted)));
                    } else if let Some(item) = self.find_item(text) {
                        self.set_current(Some(item));
                        settle(ui, taken, Ok(()));
                    } else {
                        settle(ui, taken, Err(lists_no(name, text)));
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let mut clicked: Option<CrsItem> = None;
        let mut toggled: Option<usize> = None;
        let mut double = false;
        let groups = &self.groups;
        let current_item = self.current_item;
        let output = egui::ScrollArea::vertical()
            .id_salt(name)
            .min_scrolled_height(220.0)
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (at, group) in groups.iter().enumerate() {
                    // Plain marks, which every font the window may carry draws.
                    let arrow = if group.expanded { "-" } else { "+" };
                    if ui
                        .selectable_label(
                            current_item == Some((at, None)),
                            format!("{arrow} {}", group.title),
                        )
                        .clicked()
                    {
                        toggled = Some(at);
                    }
                    if !group.expanded {
                        continue;
                    }
                    for (choice, entry) in group.choices.iter().enumerate() {
                        let row = ui
                            .horizontal(|ui| {
                                ui.add_space(18.0);
                                ui.selectable_label(
                                    current_item == Some((at, Some(choice))),
                                    format!("{}   {}", entry.name, entry.id),
                                )
                            })
                            .inner;
                        if row.clicked() {
                            clicked = Some((at, Some(choice)));
                        }
                        if row.double_clicked() {
                            clicked = Some((at, Some(choice)));
                            double = true;
                        }
                    }
                }
            });
        let response = ui.interact(output.inner_rect, ui.id().with(name), egui::Sense::hover());
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        if let Some(at) = toggled
            && let Some(group) = self.groups.get_mut(at)
        {
            group.expanded = !group.expanded;
        }
        if let Some(item) = clicked {
            self.set_current(Some(item));
        }
        if double && self.apply(host) {
            self.accept();
        }
        let mut record = ControlRecord::new(name, ControlKind::Rows);
        record.items = self.list_rows();
        record.value = match self.current_item {
            Some((at, None)) => self.groups.get(at).map(|group| group.title.clone()),
            Some((at, Some(choice))) => self
                .groups
                .get(at)
                .and_then(|group| group.choices.get(choice))
                .map(|entry| entry.name.clone()),
            None => None,
        }
        .unwrap_or_default();
        controls::custom(ui, &response, record);
    }

    /// Set, Cancel and Local Coordinates: kept below the scrolling body.
    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            if controls::button(ui, "projectCrsApply", "Set").clicked() && self.apply(&mut *host) {
                self.accept();
            }
            if controls::button(ui, "projectCrsCancel", "Cancel").clicked() {
                self.reject();
            }
            if controls::button(ui, "projectCrsClear", "Local Coordinates")
                .on_hover_text("No coordinate system: the drawing's own grid")
                .clicked()
            {
                self.set_text("");
                if self.apply(&mut *host) {
                    self.accept();
                }
            }
        });
    }
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut ProjectCrsDialog,
    host: &'a mut (dyn Host + 'h),
}

/// chooseProjectCrs: the dialog made modal and shown, for whoever waits for its answer - an egui
/// window cannot wait in a nested loop, so the answer is read when it has finished
/// (`finished`, then `changed_since_opened`). A headless run is given the non-modal dialog by the
/// window instead, as the C++'s is.
pub fn choose_project_crs(document: &Document, place: Option<(f64, f64)>) -> ProjectCrsDialog {
    let mut dialog = ProjectCrsDialog::new(document, place);
    dialog.frame = dialog.frame.clone().modal(true);
    dialog.show();
    dialog
}
