//! GIS > Analysis - GDAL > Buffer...: the window onto GIS BUFFER (geo/buffer_dialog.hpp;
//! docs/geoprocessing.md, "V1"), built on the GIS dialog frame (gis_tool_dialog): its line is
//! shown as it will run and Run hands it to the window's one executor.
//!
//! Its fields, by object name (the frame's are in gis_tool_dialog; the scope's are gisBufferScope,
//! gisBufferScopeDrawing, ...):
//!
//! ```text
//! gisBufferDistance          the distance, metres; negative shrinks an area
//! gisBufferDistanceProperty  or the property holding each entity's own distance
//!                            (distance=prop:<key>); it wins when filled
//! gisBufferSide              both | left | right (of a line, walking along it)
//! gisBufferCaps              round | flat | square line ends
//! gisBufferJoins             round | mitre | bevel corners
//! gisBufferDissolve          merge the results
//! gisBufferDissolveBy        ... by these properties (a,b); blank merges all
//! gisBufferLayer             the layer the result goes to (default gis/buffer)
//! ```

use egui::Ui;
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};

use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_layer_target,
    gis_scope, gis_word,
};
use crate::drive::{Choice, check, choice};
use crate::host::Host;

/// What the Buffer dialog's fields hold, as typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GisBufferForm {
    pub scope: GisScopeWords,
    pub distance: String,
    pub distance_property: String,
    pub side: String,
    pub caps: String,
    pub joins: String,
    pub dissolve: bool,
    pub dissolve_by: String,
    pub layer: String,
}

impl Default for GisBufferForm {
    fn default() -> Self {
        Self {
            scope: GisScopeWords::default(),
            distance: String::new(),
            distance_property: String::new(),
            side: String::from("both"),
            caps: String::from("round"),
            joins: String::from("round"),
            dissolve: false,
            dissolve_by: String::new(),
            layer: String::new(),
        }
    }
}

fn invalid(message: String) -> Error {
    make_error(ErrorCode::InvalidArgument, message, String::new())
}

/// The line `form` describes, exactly as it would be typed:
///
/// ```text
/// GIS BUFFER <scope> distance=<m>|distance=prop:<key> [side=left|right] [caps=flat|square]
///            [joins=mitre|bevel] [DISSOLVE|dissolve=a,b] [TO LAYER <layer>]
/// ```
///
/// A choice left at the verb's default is left out. InvalidArgument naming the field for a scope
/// the controls cannot say, a distance that is not a number or is 0, neither a distance nor a
/// property, and a double quote anywhere.
pub fn gis_buffer_line(form: &GisBufferForm) -> Result<String> {
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("GIS BUFFER {scope}");
    let property = form.distance_property.trim();
    if !property.is_empty() {
        let word = gis_word(
            &format!("distance=prop:{property}"),
            "The distance property",
        )?;
        line.push(' ');
        line.push_str(&word);
    } else {
        let distance = form.distance.trim();
        if distance.is_empty() {
            return Err(invalid(String::from(
                "give the distance, or the property that holds each one",
            )));
        }
        match parse_finite_double(distance) {
            Some(value) if value != 0.0 => {}
            _ => {
                return Err(invalid(format!(
                    "the distance must be a number of metres other than 0, not '{distance}'"
                )));
            }
        }
        line.push_str(" distance=");
        line.push_str(distance);
    }
    if form.side != "both" {
        line.push_str(" side=");
        line.push_str(&form.side);
    }
    if form.caps != "round" {
        line.push_str(" caps=");
        line.push_str(&form.caps);
    }
    if form.joins != "round" {
        line.push_str(" joins=");
        line.push_str(&form.joins);
    }
    if form.dissolve {
        let by = form.dissolve_by.trim();
        if by.is_empty() {
            line.push_str(" DISSOLVE");
        } else {
            let word = gis_word(&format!("dissolve={by}"), "The properties to merge by")?;
            line.push(' ');
            line.push_str(&word);
        }
    }
    line.push_str(&gis_layer_target(&form.layer)?);
    Ok(line)
}

/// A choice of fixed words, the first chosen as a QComboBox chooses it: what the GIS dialogs'
/// option boxes are.
pub(crate) struct Words {
    pub(crate) items: Vec<String>,
    pub(crate) chosen: Choice,
}

impl Words {
    pub(crate) fn new(items: &[&str]) -> Self {
        Self {
            items: items.iter().map(|item| String::from(*item)).collect(),
            chosen: Choice::at(0),
        }
    }

    /// QComboBox::currentText.
    pub(crate) fn text(&self) -> String {
        self.chosen.current_text(&self.items)
    }

    /// The current text, or `fallback` when it is empty.
    pub(crate) fn text_or(&self, fallback: &str) -> String {
        let text = self.text();
        if text.is_empty() {
            String::from(fallback)
        } else {
            text
        }
    }

    /// The choice under `label`, named `name`, with `tip`.
    pub(crate) fn ui(&mut self, ui: &mut Ui, name: &str, label: &str, tip: &str) {
        ui.horizontal(|ui| {
            ui.label(label);
            choice(ui, name, &self.items, &mut self.chosen).on_hover_text(tip);
        });
    }
}

/// GIS > Analysis - GDAL > Buffer... (see the module comment).
pub struct GisBufferDialog {
    frame: GisToolFrame,
    distance: String,
    property: String,
    side: Words,
    caps: Words,
    joins: Words,
    dissolve: bool,
    dissolve_by: String,
    layer: String,
}

impl GisBufferDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        Self {
            frame: GisToolFrame::new("gisBuffer", "Buffer", context, host, true),
            distance: String::new(),
            property: String::new(),
            side: Words::new(&["both", "left", "right"]),
            caps: Words::new(&["round", "flat", "square"]),
            joins: Words::new(&["round", "mitre", "bevel"]),
            dissolve: false,
            dissolve_by: String::new(),
            layer: String::new(),
        }
    }

    /// The window's Buffer dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn form(&self) -> GisBufferForm {
        GisBufferForm {
            scope: self.frame.scope_words(),
            distance: self.distance.clone(),
            distance_property: self.property.clone(),
            side: self.side.text_or("both"),
            caps: self.caps.text_or("round"),
            joins: self.joins.text_or("round"),
            dissolve: self.dissolve,
            dissolve_by: self.dissolve_by.clone(),
            layer: self.layer.clone(),
        }
    }
}

impl GisTool for GisBufferDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_buffer_line(&self.form())
    }

    fn fields_ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        gis_field(
            ui,
            "gisBufferDistance",
            "Distance (m):",
            &mut self.distance,
            "e.g. 5; negative shrinks an area",
            "distance=: how far out from each entity - an easement's half width, a clearance; a \
             negative distance sets an area in (a setback)",
        );
        gis_field(
            ui,
            "gisBufferDistanceProperty",
            "Or distance from property:",
            &mut self.property,
            "none - the distance above for every entity",
            "distance=prop:<key>: each entity buffered by the number its own property holds (a \
             clearance by voltage); one without it is skipped and counted",
        );
        self.side.ui(
            ui,
            "gisBufferSide",
            "Side:",
            "side=: both sides of a line, or only its left or right, walking along it",
        );
        self.caps.ui(
            ui,
            "gisBufferCaps",
            "Line ends:",
            "caps=: how a line's ends are finished; a round buffer of a point is drawn as an \
             exact circle",
        );
        self.joins.ui(
            ui,
            "gisBufferJoins",
            "Corners:",
            "joins=: how corners are finished; mitre keeps a lot's corners square",
        );
        check(
            ui,
            "gisBufferDissolve",
            "Merge the results",
            &mut self.dissolve,
        )
        .on_hover_text("DISSOLVE: one area where the buffers overlap, instead of one each");
        gis_field(
            ui,
            "gisBufferDissolveBy",
            "Merge by properties:",
            &mut self.dissolve_by,
            "none - merge everything",
            "dissolve=a,b: merge only results whose properties agree",
        );
        gis_field(
            ui,
            "gisBufferLayer",
            "Result layer:",
            &mut self.layer,
            "default gis/buffer",
            "TO LAYER: the layer the areas are drawn on",
        );
    }
}
