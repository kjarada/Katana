//! Where a dataset comes from (geo/binding_picker.hpp; docs/geoprocessing.md, "The window"): the
//! picker the GDAL Toolbox, its Pipeline tab and the DEM tools give each dataset an algorithm
//! reads.
//!
//! A dataset is what is drawn (the scope and filter controls, one ScopeFilterWidget), a reference
//! raster (RASTER <id>), a surface (SURFACE <name> [CELL <m>]) or a file (FILE <path>): the source
//! clause the executor reads after FROM. The picker writes that clause's words and opens nothing.
//!
//! ```text
//! <prefix>Source   the picker itself
//! <prefix>Kind     Drawing | Reference raster | Surface | File (and Not given, for an optional
//!                  dataset)
//! <prefix>Scope... the drawing's controls (ScopeFilterWidget named from <prefix>)
//! <prefix>Raster   the reference raster
//! <prefix>Surface  the surface; <prefix>Cell the cell it is sampled on
//! <prefix>File     the file; <prefix>Browse
//! ```
//!
//! `BindingList` is several datasets for one argument - a mosaic's tiles, raster calc's inputs: a
//! picker and the list its Add builds (<prefix><listName>, <prefix>Add, <prefix>Remove).

use egui::Ui;
use katana_core::Result;
use katana_core::text::parse_finite_double;
use katana_gis::processing::DatasetKind;

use super::geo_dialog_support::line_word;
use super::terrain_dialog_parts::{field, host_views, invalid, one_column, remove_current, row};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::controls::custom;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::drive::{Choice, button, choice, rows};
use crate::host::Host;
use crate::ifc_dialogs::{from_native_separators, get_open_file_name, to_native_separators};
use crate::survey::survey_dialogs::offstage;

/// The kinds a picker offers, as bits to combine; NONE is an optional dataset left out.
pub struct BindingKind;

impl BindingKind {
    pub const NONE: u32 = 0;
    pub const DRAWING: u32 = 1;
    pub const RASTER: u32 = 2;
    pub const SURFACE: u32 = 4;
    pub const FILE: u32 = 8;
}

/// The kinds in the order the Kind choice offers them, with its text.
const KINDS: [(u32, &str); 4] = [
    (BindingKind::DRAWING, "Drawing"),
    (BindingKind::RASTER, "Reference raster"),
    (BindingKind::SURFACE, "Surface"),
    (BindingKind::FILE, "File"),
];

/// The kinds that can give a dataset of GDAL's kinds (processing::DatasetKind bits): a vector from
/// the drawing or a file, a raster from a reference raster, a surface or a file. What the
/// executor's sources-for says in words.
pub fn binding_kinds_for(dataset_kinds: u32) -> u32 {
    let mut kinds = BindingKind::FILE;
    if dataset_kinds & DatasetKind::Vector as u32 != 0 {
        kinds |= BindingKind::DRAWING;
    }
    if dataset_kinds & DatasetKind::Raster as u32 != 0 {
        kinds |= BindingKind::RASTER | BindingKind::SURFACE;
    }
    kinds
}

/// Where one dataset comes from (see the module comment).
pub struct BindingPicker {
    prefix: String,
    /// The kinds offered, in the choice's order, and its texts.
    kinds: Vec<u32>,
    kind_texts: Vec<String>,
    kind: Choice,
    scope: Option<ScopeFilterWidget>,
    /// Each reference raster's id as a line writes it, and "<id>  <name>" as the choice shows it.
    raster_ids: Vec<String>,
    raster_texts: Vec<String>,
    raster: Choice,
    surfaces: Vec<String>,
    surface: Choice,
    cell: String,
    file: String,
    /// What the picker has to say that is no field's value - that a headless session opens no
    /// file dialog - for the dialog to show in its status.
    said: Option<String>,
}

impl BindingPicker {
    /// A picker offering `kinds`, its controls named from `prefix`, over the window's reference
    /// rasters, surfaces, drawing and views.
    pub fn new(prefix: &str, kinds: u32, host: &mut dyn Host) -> Self {
        let offered: Vec<(u32, &str)> = KINDS
            .iter()
            .copied()
            .filter(|(kind, _)| kinds & kind != 0)
            .collect();
        let scope = (kinds & BindingKind::DRAWING != 0).then(|| {
            let mut scope = ScopeFilterWidget::new(prefix);
            scope.views = host_views(host);
            scope.set_choice(ScopeChoice::Drawing);
            scope
        });
        let mut picker = Self {
            prefix: String::from(prefix),
            kinds: offered.iter().map(|(kind, _)| *kind).collect(),
            kind_texts: offered.iter().map(|(_, text)| String::from(*text)).collect(),
            kind: if offered.is_empty() {
                Choice::default()
            } else {
                Choice::at(0)
            },
            scope,
            raster_ids: Vec::new(),
            raster_texts: Vec::new(),
            raster: Choice::default(),
            surfaces: Vec::new(),
            surface: Choice::default(),
            cell: String::new(),
            file: String::new(),
            said: None,
        };
        picker.reload(&*host);
        picker
    }

    fn named(&self, part: &str) -> String {
        format!("{}{part}", self.prefix)
    }

    fn offers(&self, kind: u32) -> bool {
        self.kinds.contains(&kind)
    }

    /// The kind chosen (a BindingKind bit, or NONE).
    pub fn kind(&self) -> u32 {
        self.kind
            .index
            .and_then(|at| self.kinds.get(at))
            .copied()
            .unwrap_or(BindingKind::NONE)
    }

    /// The texts the Kind choice offers, in order.
    pub fn kind_texts(&self) -> &[String] {
        &self.kind_texts
    }

    /// Chooses `kind` when the picker offers it; false when it does not.
    pub fn set_kind(&mut self, kind: u32) -> bool {
        match self.kinds.iter().position(|offered| *offered == kind) {
            Some(index) => {
                self.kind.index = Some(index);
                true
            }
            None => false,
        }
    }

    /// For an optional dataset: offers "Not given" first, and chooses it.
    pub fn set_optional(&mut self) {
        if self.offers(BindingKind::NONE) {
            return;
        }
        self.kinds.insert(0, BindingKind::NONE);
        self.kind_texts.insert(0, String::from("Not given"));
        self.kind.index = Some(0);
    }

    /// The words of the source clause, or why the controls say none, naming the field; empty for
    /// an optional dataset not given.
    pub fn words(&self) -> Result<String> {
        match self.kind() {
            BindingKind::NONE => Ok(String::new()),
            BindingKind::DRAWING => {
                let Some(scope) = &self.scope else {
                    return Err(invalid("choose where the data comes from"));
                };
                scope.verb_words().map_err(|error| {
                    invalid(format!("the drawing's scope: {}", error.message))
                })
            }
            BindingKind::RASTER => match self.raster.index.and_then(|at| self.raster_ids.get(at)) {
                Some(id) => Ok(format!("RASTER {id}")),
                None => Err(invalid("choose a reference raster")),
            },
            BindingKind::SURFACE => {
                let Some(surface) = self.surface.index.and_then(|at| self.surfaces.get(at)) else {
                    return Err(invalid("choose a surface"));
                };
                let name = line_word(surface, "the surface's name")?;
                let mut words = format!("SURFACE {name}");
                let cell = self.cell.trim();
                if !cell.is_empty() {
                    if !parse_finite_double(cell).is_some_and(|length| length > 0.0) {
                        return Err(invalid(format!(
                            "the cell must be a positive number of metres, not '{cell}'"
                        )));
                    }
                    words.push_str(&format!(" CELL {cell}"));
                }
                Ok(words)
            }
            BindingKind::FILE => {
                let path = self.file.trim();
                if path.is_empty() {
                    return Err(invalid("choose a file"));
                }
                let word = line_word(&from_native_separators(path), "the file's path")?;
                Ok(format!("FILE {word}"))
            }
            _ => Err(invalid("choose where the data comes from")),
        }
    }

    /// Refills the reference rasters, the surfaces and the scope's layers and views, keeping what
    /// is chosen.
    pub fn reload(&mut self, host: &dyn Host) {
        if self.offers(BindingKind::RASTER) {
            let kept = self
                .raster
                .index
                .and_then(|at| self.raster_ids.get(at))
                .cloned();
            self.raster_ids.clear();
            self.raster_texts.clear();
            for raster in host.reference().rasters() {
                self.raster_ids.push(format!("{}", raster.id));
                self.raster_texts
                    .push(format!("{}  {}", raster.id, raster.name));
            }
            let again = kept.and_then(|id| self.raster_ids.iter().position(|listed| *listed == id));
            self.raster.index = again.or(if self.raster_ids.is_empty() {
                None
            } else {
                Some(0)
            });
        }
        if self.offers(BindingKind::SURFACE) {
            let kept = self.surface.current_text(&self.surfaces);
            self.surfaces = host
                .document()
                .surfaces()
                .all()
                .iter()
                .map(|surface| surface.name.clone())
                .collect();
            let again = self.surfaces.iter().position(|name| *name == kept);
            self.surface.index = again.or(if self.surfaces.is_empty() {
                None
            } else {
                Some(0)
            });
        }
        if let Some(scope) = &mut self.scope {
            scope.reload(host.document().model());
        }
    }

    /// The drawing's controls; None when the picker offers no drawing.
    pub fn scope(&mut self) -> Option<&mut ScopeFilterWidget> {
        self.scope.as_mut()
    }

    /// What the File field holds.
    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn set_file(&mut self, path: &str) {
        self.file = String::from(path);
    }

    /// What the reference raster choice shows; empty when none is chosen.
    pub fn raster_text(&self) -> String {
        self.raster.current_text(&self.raster_texts)
    }

    /// The reference rasters the choice offers, each "<id>  <name>".
    pub fn raster_texts(&self) -> &[String] {
        &self.raster_texts
    }

    /// What the picker said since it was last asked (a headless Browse); taken once.
    pub fn take_said(&mut self) -> Option<String> {
        self.said.take()
    }

    pub(crate) fn say(&mut self, text: &str) {
        self.said = Some(String::from(text));
    }

    fn browse(&mut self, host: &dyn Host) {
        if host.headless() {
            let text = format!(
                "A headless session opens no file dialog: fill {} with the path instead.",
                self.named("File")
            );
            self.say(&text);
            return;
        }
        let path = get_open_file_name("Choose a Dataset", self.file.trim(), "All files (*)");
        if !path.is_empty() {
            self.file = to_native_separators(&path);
        }
    }

    /// One kind's page; true when a control of it was edited.
    fn page(&mut self, ui: &mut Ui, host: &dyn Host, kind: u32) -> bool {
        let mut changed = false;
        match kind {
            BindingKind::DRAWING => {
                if let Some(scope) = &mut self.scope {
                    changed = scope.ui(ui);
                }
            }
            BindingKind::RASTER => {
                let name = self.named("Raster");
                row(ui, "Raster:", |ui| {
                    changed = choice(ui, &name, &self.raster_texts, &mut self.raster)
                        .on_hover_text(
                            "A reference raster, read at full precision from its own file \
                             (RASTER <id>)",
                        )
                        .changed();
                    if self.raster_texts.is_empty() {
                        ui.weak("no reference raster yet - GIS > Import Raster, or REFS");
                    }
                });
            }
            BindingKind::SURFACE => {
                let name = self.named("Surface");
                row(ui, "Surface:", |ui| {
                    changed = choice(ui, &name, &self.surfaces, &mut self.surface)
                        .on_hover_text(
                            "A surface, sampled at the centre of each cell (SURFACE <name>)",
                        )
                        .changed();
                    if self.surfaces.is_empty() {
                        ui.weak("no surface yet - Terrain > Surfaces");
                    }
                });
                let name = self.named("Cell");
                row(ui, "Cell (m):", |ui| {
                    if field(ui, &name, &mut self.cell, "suggested for its extent")
                        .on_hover_text(
                            "CELL: the side of each cell the surface is sampled on, metres",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                });
            }
            BindingKind::FILE => {
                let name = self.named("File");
                let browse_name = self.named("Browse");
                let browse = row(ui, "File:", |ui| {
                    if field(ui, &name, &mut self.file, "a path, a /vsi path or a URL")
                        .on_hover_text(
                            "FILE: GDAL opens it as it is - a /vsi path or a URL passes through",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    button(ui, &browse_name, "Browse...").clicked()
                });
                if browse {
                    let before = self.file.clone();
                    self.browse(host);
                    changed = changed || self.file != before;
                }
            }
            _ => {}
        }
        changed
    }

    /// Draws the picker; true when any control was edited this frame.
    pub fn ui(&mut self, ui: &mut Ui, host: &dyn Host) -> bool {
        let mut changed = false;
        let outer = ui.vertical(|ui| {
            let name = self.named("Kind");
            row(ui, "From:", |ui| {
                changed = choice(ui, &name, &self.kind_texts, &mut self.kind)
                    .on_hover_text(
                        "Where the data comes from: what is drawn, a reference raster, a surface \
                         or a file",
                    )
                    .changed();
            });
            // The pages not in front are still there, as a QStackedWidget's are: what they hold
            // can be filled, pressed and read.
            let current = self.kind();
            let kinds = self.kinds.clone();
            for kind in kinds {
                if kind == BindingKind::NONE {
                    continue;
                }
                if kind == current {
                    if self.page(ui, host, kind) {
                        changed = true;
                    }
                } else {
                    let salt = format!("{}Page{kind}", self.prefix);
                    if offstage(ui, &salt, |ui| self.page(ui, host, kind)) {
                        changed = true;
                    }
                }
            }
        });
        let name = self.named("Source");
        custom(
            ui,
            &outer.response,
            ControlRecord::new(&name, ControlKind::Other),
        );
        changed
    }
}

/// Several datasets for one argument - a mosaic's tiles, raster calc's inputs: a picker, and the
/// list its Add builds. With nothing listed, the picker's own dataset is the one.
pub struct BindingList {
    prefix: String,
    list_name: String,
    picker: BindingPicker,
    listed: Vec<String>,
    current: Option<usize>,
}

impl BindingList {
    /// A list whose picker offers `kinds`, the list named `prefix` and `list_name`.
    pub fn new(prefix: &str, kinds: u32, list_name: &str, host: &mut dyn Host) -> Self {
        Self {
            prefix: String::from(prefix),
            list_name: String::from(list_name),
            picker: BindingPicker::new(prefix, kinds, host),
            listed: Vec::new(),
            current: None,
        }
    }

    pub fn picker(&mut self) -> &mut BindingPicker {
        &mut self.picker
    }

    /// The source clauses listed, in order.
    pub fn listed(&self) -> &[String] {
        &self.listed
    }

    /// The source clauses, in order: the list's, or the picker's alone.
    pub fn words(&self) -> Result<Vec<String>> {
        if !self.listed.is_empty() {
            return Ok(self.listed.clone());
        }
        let one = self.picker.words()?;
        Ok(if one.is_empty() { Vec::new() } else { vec![one] })
    }

    /// The picker's dataset added to the list; `Err` saying why when the picker says none.
    pub fn add(&mut self) -> std::result::Result<(), String> {
        match self.picker.words() {
            Err(error) => Err(error.message),
            Ok(words) if words.is_empty() => Err(String::from("choose a dataset to add first")),
            Ok(words) => {
                if !self.listed.contains(&words) {
                    self.listed.push(words);
                }
                Ok(())
            }
        }
    }

    pub fn reload(&mut self, host: &dyn Host) {
        self.picker.reload(host);
    }

    /// Draws the picker and its list; true when either was edited this frame.
    pub fn ui(&mut self, ui: &mut Ui, host: &dyn Host) -> bool {
        let mut changed = self.picker.ui(ui, host);
        let list_name = format!("{}{}", self.prefix, self.list_name);
        let add_name = format!("{}Add", self.prefix);
        let remove_name = format!("{}Remove", self.prefix);
        let listed = one_column(&self.listed);
        let (add, remove) = ui
            .horizontal(|ui| {
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(format!("{list_name}Scroll"))
                        .max_height(110.0)
                        .show(ui, |ui| {
                            rows(ui, &list_name, &listed, &mut self.current).on_hover_text(
                                "The datasets, in order; none listed: the one picked above",
                            );
                        });
                });
                ui.vertical(|ui| {
                    let add = button(ui, &add_name, "Add").clicked();
                    let remove = button(ui, &remove_name, "Remove").clicked();
                    (add, remove)
                })
                .inner
            })
            .inner;
        if add {
            match self.add() {
                Ok(()) => changed = true,
                Err(why) => self.picker.say(&why),
            }
        }
        if remove {
            remove_current(&mut self.listed, &mut self.current);
            changed = true;
        }
        changed
    }
}
