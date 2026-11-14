//! Terrain > Grid Points to DEM (geo/grid_dem_dialog.hpp; docs/geoprocessing.md, "Gridding"): the
//! window onto the RASTER GRID verb.
//!
//! The dialog grids nothing itself. It writes the RASTER GRID line its fields describe - shown, as
//! it will run, in gridCommand - and Run hands that line to the window's one executor as if it had
//! been typed (geo_dialog_support's GeoRunPanel): the grid is GDAL's, the verb's background job,
//! kept as a reference raster. The line is made by a pure function of the fields, tested without a
//! window.
//!
//! ```text
//! gridDemDialog:
//!   gridScope...   which points (ScopeFilterWidget: gridScopeDrawing, gridFilterLayer ...)
//!   gridOptions    the grid's options:
//!     gridMethod     GDAL's gridding method, from its catalogue
//!     gridCell       the cell's side; gridSize, columns x rows instead
//!     gridZ          where the heights come from: the geometry, or a property holding a number
//!     gridExtent, gridPower, gridRadius, gridName, gridToSurface
//!   gridCommand, gridPreview, gridRun, gridStatus, gridReply   (GeoRunPanel)
//! ```

use std::collections::BTreeSet;

use egui::Ui;
use katana_core::Result;
use katana_core::text::{parse_finite_double, trimmed};
use katana_entity::PropertyValue;
use katana_entity::entity::{ELEVATION_PROPERTY, ELEVATIONS_PROPERTY};
use katana_gis::processing as gp;

use super::geo_dialog_support::{GeoRunPanel, line_word};
use super::terrain_dialog_parts::{Framed, field, host_views, invalid, row, show_framed};
use crate::customisation::{DocumentWatcher, ScopeChoice, ScopeFilterWidget};
use crate::drive::controls::custom;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::drive::{Choice, check, choice};
use crate::host::Host;
use crate::shell::DialogFrame;

/// What the dialog's fields hold, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GridDemForm {
    /// The scope and filter words (ScopeFilterWidget::verb_words), or why the controls say none.
    pub scope: String,
    pub scope_error: String,
    pub method: String,
    pub cell: String,
    pub size: String,
    /// "geometry" or blank for the drawing's heights; else a property's name.
    pub z: String,
    pub extent: String,
    pub power: String,
    pub radius: String,
    pub name: String,
    pub to_surface: bool,
}

fn reads_as_number(text: &str) -> bool {
    parse_finite_double(text).is_some()
}

/// " key=value" for a number given, nothing for a blank field.
fn number_option(key: &str, text: &str, field: &str) -> Result<String> {
    let value = text.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    if !reads_as_number(value) {
        return Err(invalid(format!(
            "{field} must be a number, not '{value}'"
        )));
    }
    Ok(format!(" {key}={value}"))
}

/// An option's value is one word with no '=' and no quote: a blank would end it and a quoted word
/// is never an option.
fn word_option(key: &str, text: &str, field: &str) -> Result<String> {
    let value = text.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.chars().any(char::is_whitespace) || value.contains('=') || value.contains('"') {
        return Err(invalid(format!(
            "{field} is one word with no blank, '=' or double quote: '{value}'"
        )));
    }
    Ok(format!(" {key}={value}"))
}

/// GDAL's gridding methods: the leaves of `vector grid`, in its order, read at run time, so a
/// method GDAL adds is offered with no change here - as the verb itself reads them.
fn grid_methods() -> Vec<String> {
    gp::catalogue()
        .iter()
        .filter(|info| {
            !info.container
                && info.path.len() == 3
                && info.path[0] == "vector"
                && info.path[1] == "grid"
        })
        .map(|info| info.path[2].clone())
        .collect()
}

fn method_takes(method: &str, argument: &str) -> bool {
    let path = [
        String::from("vector"),
        String::from("grid"),
        String::from(method),
    ];
    gp::describe(&path).is_ok_and(|spec| spec.args.iter().any(|arg| arg.name == argument))
}

fn is_number(value: &PropertyValue) -> bool {
    match value {
        PropertyValue::Real(_) | PropertyValue::Integer(_) => true,
        PropertyValue::Text(text) => parse_finite_double(trimmed(text.as_bytes())).is_some(),
        PropertyValue::Boolean(_) => false,
    }
}

/// The RASTER GRID line `form` describes, exactly as it would be typed:
///
/// ```text
/// RASTER GRID <scope> [method=<m>] [cell=<m> | size=<c>x<r>] [z=<property>]
///             [extent=x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>] [TO SURFACE <name>]
/// ```
///
/// A blank field is left out. InvalidArgument naming the field for a scope the controls cannot say,
/// a number that does not read (a comma is not a decimal point, as it is not on the command line),
/// a size that is not columns x rows, an extent that is not four numbers, a property name or
/// method with a blank or an '=' in it (an option's value is one word), and a double quote.
pub fn grid_dem_command_line(form: &GridDemForm) -> Result<String> {
    if !form.scope_error.is_empty() {
        return Err(invalid(format!(
            "the points' scope: {}",
            form.scope_error
        )));
    }
    let scope = form.scope.trim();
    if scope.is_empty() {
        return Err(invalid("choose which points to grid first"));
    }
    let z = form.z.trim();
    let z_from_geometry = z.is_empty() || z.to_lowercase() == "geometry";
    let parts = [
        word_option("method", &form.method, "the method"),
        number_option("cell", &form.cell, "the cell size"),
        word_option("size", &form.size, "the grid's size"),
        if z_from_geometry {
            Ok(String::new())
        } else {
            word_option("z", z, "the height property")
        },
        word_option("extent", &form.extent, "the extent"),
        number_option("power", &form.power, "the power"),
        number_option("radius", &form.radius, "the radius"),
    ];
    let mut line = format!("RASTER GRID {scope}");
    for part in parts {
        line.push_str(&part?);
    }
    let size = form.size.trim();
    if !size.is_empty() {
        let sides: Vec<&str> = size.split(['x', 'X', ',']).collect();
        if sides.len() != 2 || !reads_as_number(sides[0]) || !reads_as_number(sides[1]) {
            return Err(invalid(
                "the grid's size is columns x rows, two whole numbers: 400x300",
            ));
        }
    }
    let extent = form.extent.trim();
    if !extent.is_empty() {
        let corners: Vec<&str> = extent.split(',').collect();
        if corners.len() != 4 || !corners.iter().all(|corner| reads_as_number(corner)) {
            return Err(invalid("the extent is four numbers, x0,y0,x1,y1"));
        }
    }
    let name = form.name.trim();
    if !name.is_empty() {
        line.push_str(&format!(" NAME {}", line_word(name, "the name")?));
    }
    if form.to_surface {
        let surface = if name.is_empty() { "dem" } else { name };
        line.push_str(&format!(
            " TO SURFACE {}",
            line_word(surface, "the surface's name")?
        ));
    }
    Ok(line)
}

/// Terrain > Grid Points to DEM (see the module comment).
pub struct GridDemDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
    frame: DialogFrame,
    visible: bool,
    scope: ScopeFilterWidget,
    methods: Vec<String>,
    method: Choice,
    cell: String,
    size: String,
    z_items: Vec<String>,
    z: Choice,
    extent: String,
    power: String,
    radius: String,
    name: String,
    to_surface: bool,
    /// Whether the method chosen takes a power and a radius, from its own arguments.
    takes_power: bool,
    takes_radius: bool,
    panel: GeoRunPanel,
}

impl GridDemDialog {
    /// The dialog over the window's drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        // The points: Global Modify's own scope and filter controls.
        let mut scope = ScopeFilterWidget::new("grid");
        scope.views = host_views(host);
        scope.set_choice(ScopeChoice::Drawing);
        let methods = grid_methods();
        // setCurrentText on a choice that is not editable: linear when GDAL has it, else the
        // first.
        let method = methods
            .iter()
            .position(|method| method == "linear")
            .or(if methods.is_empty() { None } else { Some(0) });
        let mut dialog = Self {
            // The layers and properties follow the drawing.
            watcher: DocumentWatcher::new(host.document()),
            // Non-modal: kept open beside the drawing, which a run changes.
            frame: DialogFrame::new("gridDemDialog", "Grid Points to DEM"),
            visible: false,
            scope,
            methods,
            method: Choice {
                index: method,
                ..Choice::default()
            },
            cell: String::new(),
            size: String::new(),
            z_items: Vec::new(),
            z: Choice::editable(""),
            extent: String::new(),
            power: String::new(),
            radius: String::new(),
            name: String::new(),
            to_surface: false,
            takes_power: false,
            takes_radius: false,
            panel: GeoRunPanel::new("grid", true),
        };
        dialog.method_chosen();
        dialog.reload(&*host);
        dialog
    }

    /// Shows the dialog: the views open now, not when the dialog was made.
    pub fn show(&mut self, host: &dyn Host) {
        self.reload(host);
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Refills the scope's layers and views and the properties gridZ offers, keeping what is
    /// chosen.
    pub fn reload(&mut self, host: &dyn Host) {
        let model = host.document().model();
        self.scope.reload(model);
        // The drawing's properties that hold a number somewhere; the heights of the geometry are
        // "geometry", not their property.
        let kept = self.z.text.clone();
        let mut keys = BTreeSet::new();
        model.entities.for_each(|entity| {
            for (key, value) in &entity.properties {
                if key != ELEVATION_PROPERTY && key != ELEVATIONS_PROPERTY && is_number(value) {
                    keys.insert(key.clone());
                }
            }
        });
        self.z_items = vec![String::from("geometry")];
        self.z_items.extend(keys);
        let shown = if kept.is_empty() {
            String::from("geometry")
        } else {
            kept
        };
        self.z.index = self.z_items.iter().position(|item| *item == shown);
        self.z.text = shown;
    }

    pub fn form(&self) -> GridDemForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        GridDemForm {
            scope,
            scope_error,
            method: self.method.current_text(&self.methods),
            cell: self.cell.clone(),
            size: self.size.clone(),
            z: self.z.text.clone(),
            extent: self.extent.clone(),
            power: if self.takes_power {
                self.power.clone()
            } else {
                String::new()
            },
            radius: if self.takes_radius {
                self.radius.clone()
            } else {
                String::new()
            },
            name: self.name.clone(),
            to_surface: self.to_surface,
        }
    }

    pub fn command(&self) -> Result<String> {
        grid_dem_command_line(&self.form())
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    pub fn run_panel(&mut self) -> &mut GeoRunPanel {
        &mut self.panel
    }

    /// The methods gridMethod offers: GDAL's own, from its catalogue.
    pub fn methods(&self) -> &[String] {
        &self.methods
    }

    /// The properties gridZ offers, "geometry" first.
    pub fn height_sources(&self) -> &[String] {
        &self.z_items
    }

    fn method_chosen(&mut self) {
        // Only what the method takes, from its own arguments.
        let method = self.method.current_text(&self.methods);
        self.takes_power = method_takes(&method, "power");
        self.takes_radius = method_takes(&method, "radius");
    }

    /// One frame of the dialog, when it is shown: its drawing's changes delivered first.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        if self
            .watcher
            .deliver(host.document())
            .is_some_and(|changes| changes.model)
        {
            self.reload(&*host);
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
    }

    fn options(&mut self, ui: &mut Ui) {
        ui.label("Grid");
        let method_changed = row(ui, "Method:", |ui| {
            choice(ui, "gridMethod", &self.methods, &mut self.method)
                .on_hover_text(
                    "GDAL's gridding method (method=): linear interpolates over a triangulation \
                     of the points and reproduces a plane exactly; invdist and invdistnn weight \
                     by inverse distance; nearest takes the nearest point; average, minimum, \
                     maximum, range and count summarise the points in a search ellipse",
                )
                .changed()
        });
        if method_changed {
            self.method_chosen();
        }
        // A cell or a size, never both.
        let cell_given = !self.cell.trim().is_empty();
        let size_given = !self.size.trim().is_empty();
        row(ui, "Cell (m):", |ui| {
            ui.add_enabled_ui(!size_given, |ui| {
                field(ui, "gridCell", &mut self.cell, "suggested for the extent").on_hover_text(
                    "cell=: each cell's side, metres. The extent grows outwards to whole cells, \
                     so every cell is this size and grids of one cell size line up",
                );
            });
            ui.label("or size:");
            ui.add_enabled_ui(!cell_given, |ui| {
                field(ui, "gridSize", &mut self.size, "or columns x rows: 400x300").on_hover_text(
                    "size=: the grid's columns and rows over the extent as it is, instead of a \
                     cell",
                );
            });
        });
        row(ui, "Heights from:", |ui| {
            choice(ui, "gridZ", &self.z_items, &mut self.z).on_hover_text(
                "z=: where each point's height comes from - the geometry's own heights, or a \
                 property holding a number. A point without one is left out and counted, never \
                 read as 0",
            );
        });
        row(ui, "Extent:", |ui| {
            field(ui, "gridExtent", &mut self.extent, "the scope's bounds").on_hover_text(
                "extent=x0,y0,x1,y1: the area gridded; blank for the bounds of the points used, \
                 outwards to whole cells",
            );
        });
        let takes_power = self.takes_power;
        row(ui, "Power:", |ui| {
            ui.add_enabled_ui(takes_power, |ui| {
                field(ui, "gridPower", &mut self.power, "GDAL's default").on_hover_text(
                    "power=: the weighting power of the inverse-distance methods",
                );
            });
        });
        let takes_radius = self.takes_radius;
        row(ui, "Search radius (m):", |ui| {
            ui.add_enabled_ui(takes_radius, |ui| {
                field(ui, "gridRadius", &mut self.radius, "GDAL's default").on_hover_text(
                    "radius=: how far from a cell a point may be and count, metres",
                );
            });
        });
        row(ui, "Name:", |ui| {
            field(ui, "gridName", &mut self.name, "dem").on_hover_text(
                "NAME: the reference raster's name; a name taken becomes <name>-2",
            );
        });
        check(
            ui,
            "gridToSurface",
            "Keep as a named surface (TO SURFACE)",
            &mut self.to_surface,
        )
        .on_hover_text(
            "TO SURFACE: keep the grid as a named surface (SURFACE LIST) instead of a reference \
             raster, named by Name (dem when blank)",
        );
        ui.label(
            "The points in the scope, and the vertices of its lines and areas, gridded by GDAL \
             into a DEM kept as a reference raster (REFS lists it). Cells no point reaches hold \
             no data, never 0.",
        );
    }
}

impl Framed for GridDemDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                self.scope.ui(ui);
            });
            let options = ui.group(|ui| {
                ui.vertical(|ui| self.options(ui));
            });
            custom(
                ui,
                &options.response,
                ControlRecord::new("gridOptions", ControlKind::Other),
            );
        });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let made = self.command();
        let _ = self.panel.ui(ui, host, &made);
    }
}
