//! BIM > Road Objects > Barrier, Guide Posts, Sign, Light Poles, Culvert, Wall, Fence, Line
//! Marking (docs/road_objects.md, "In the window"): one dialog, a page per kind, that places a road
//! object. The port of bim/road_object_dialog.
//!
//! NOTHING IS PLACED HERE. The dialog writes the ROADOBJECT ADD line its fields describe - shown, as
//! it will run, in roadObjectLine - and Add hands it to the window's one executor (host::Host::run)
//! exactly as if it had been typed: echoed in the command log, kept in the history, one undo step.
//! Preview runs the same line with PREVIEW, which changes nothing. So there is nothing the dialog
//! can do that an agent cannot by typing the line it shows.
//!
//! The line is made by `road_object_add_line`, a pure function of the form, so it is tested without
//! a window. The object's own keys stand before its placement, so a scope's WHERE, which takes
//! every key=value after it, can end the line. A field left empty is left out - the kind's default,
//! which its placeholder shows. A number that does not read, a placement with no alignment or no
//! points, or a value a line cannot say (a double quote) refuses the whole line, saying which
//! field; nothing runs.
//!
//! Each page shows the kind's parameters, built from its own table (cad's road::road_objects::
//! parameters_of) - so a parameter added there appears here - and the preview beside it draws the
//! object those parameters make, by the same code the drawing is made by, on a straight level road:
//! a barrier, a wall, a fence or a marking seen along its line, which is its cross-section; a post,
//! a sign or a light pole from the front; a culvert across its barrel.
//!
//! The pages, the placement and the heights are the dialog frame's scrolling body; the line, the
//! buttons, the reply and the status are its footer, kept in reach on a laptop's screen.
//!
//! Object names, which the widget tests and the headless --dialog, --fill and --press steps drive
//! it by:
//!
//! ```text
//! roadObjectDialog           the dialog (every Road Objects action's data)
//! roadObjectBody             the pages, placement and heights (the frame's scrolling body)
//! roadObjectKinds            a tab per kind, in cad's order
//! roadObject_<kind>_<key>    a parameter: a field, or a choice whose first item is the default
//!                            ("roadObject_barrier_type")
//! roadObjectKindSource       where the kind's defaults come from
//! roadObjectPreview          the preview
//! roadObjectPlaceAlignment, roadObjectPlaceLines, roadObjectPlacePoints,
//! roadObjectPlaceDrawnPoints the placement: along an alignment, along the lines the scope takes,
//!                            at points typed, at the points the scope takes
//! roadObjectAlignment        the alignment (a choice of the drawing's)
//! roadObjectStation, roadObjectTo, roadObjectOffset, roadObjectSpacing
//! roadObjectPoints           "x,y[,z] x,y[,z] ..."
//! roadObjectSourceScope      the scope and filter (ScopeFilterWidget, its controls
//!                            roadObjectSourceScopeSelection, roadObjectSourceScopeDrawing,
//!                            roadObjectSourceLayers ...)
//! roadObjectHeights          automatic, profile, surface or line
//! roadObjectSurface          the surface (an editable choice)
//! roadObjectCrossfall, roadObjectDz   (the crossfall only along an alignment, standing on the
//!                            profile: nothing else reads it)
//! roadObjectName, roadObjectLayer, roadObjectColour, roadObjectMaterial, roadObjectFacility
//! roadObjectLine             the exact line Add will run (read-only)
//! roadObjectPreviewRun       run it with PREVIEW
//! roadObjectAdd              run it
//! roadObjectReply            what the line replied, or why it was refused
//! roadObjectStatus           what happened last
//! roadObjectClose
//! ```

use egui::Ui;
use katana_cad::Document;
use katana_cad::bim::bim_object::{BimModel, BimObject};
use katana_cad::road::road_objects::{
    KindShape, ParameterType, PlacementContext, RoadObject, RoadObjectKind, RoadObjectPlacement,
    apply_key, build_road_object, find_parameter, kind_source, kind_title, kind_word,
    parameters_of, road_object_kinds, shape_of,
};
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::Alignment;
use katana_geometry::{
    AlignmentPi, HorizontalAlignment, Point2, Point3, ProfilePvi, VerticalAlignment,
};
use katana_math::cpp;

use super::road_dialog_support::{Combo, PreviewRaster, button, field, preview, radio};
use crate::canvas::{
    Brush, Canvas, Color, FillRule, Pen, PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint,
    TextFlags,
};
use crate::customisation::document_watcher::DocumentWatcher;
use crate::customisation::scope_filter_widget::{ScopeChoice, ScopeFilterWidget};
use crate::drive::controls::{
    LineOptions, TextOptions, custom, label, label_as, line_with, tabs, text_with,
};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::shell::dialog::DialogFrame;
use crate::theme::palette;

/// Where the object goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Place {
    #[default]
    Alignment,
    Lines,
    Points,
    DrawnPoints,
}

/// What the form holds, as text, for `road_object_add_line`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadObjectForm {
    pub kind: RoadObjectKind,
    pub place: Place,
    pub alignment: String,
    pub station: String,
    pub to: String,
    pub offset: String,
    pub spacing: String,
    pub points: String,
    /// The scope's words (ScopeFilterWidget::verb_words), for Lines and DrawnPoints.
    pub scope: String,
    /// Why the scope's words could not be made (a colour that does not read, no layer ticked ...),
    /// when they could not: said in place of "say which part".
    pub scope_error: String,
    /// "", profile, surface or line.
    pub heights: String,
    pub surface: String,
    pub crossfall: String,
    pub dz: String,
    pub name: String,
    pub layer: String,
    pub colour: String,
    pub material: String,
    pub facility: String,
    /// The kind's parameters given, key and value; empty values are left out.
    pub parameters: Vec<(String, String)>,
    pub preview: bool,
}

/// A value as a line takes it: as it is when it is one plain word, else in double quotes - which a
/// line has no way to type inside one.
fn line_word(value: &str, field: &str) -> Result<String> {
    if value.contains('"') {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{field} cannot hold a double quote: a line has no way to type one"),
            value,
        ));
    }
    if value.is_empty() || value.contains(' ') || value.contains('\t') {
        return Ok(format!("\"{value}\""));
    }
    Ok(String::from(value))
}

fn number_word(text: &str, field: &str) -> Result<String> {
    let trimmed = text.trim();
    if parse_finite_double(trimmed).is_none() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{field} must be a number"),
            text,
        ));
    }
    Ok(String::from(trimmed))
}

/// "x,y" or "x,y,z".
fn reads_as_point(word: &str) -> bool {
    let parts: Vec<&str> = word.split(',').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| parse_finite_double(part).is_some())
}

/// The ROADOBJECT ADD line `form` describes. InvalidArgument naming the field for an alignment
/// placement with no alignment or station, points with none, a number that does not read, a scope
/// missing where one is needed, or a value with a double quote in it.
pub fn road_object_add_line(form: &RoadObjectForm) -> Result<String> {
    let mut words: Vec<String> = vec![
        String::from("ROADOBJECT"),
        String::from("ADD"),
        String::from(kind_word(form.kind)),
    ];
    let text = |words: &mut Vec<String>, key: &str, value: &str| -> Status {
        if value.trim().is_empty() {
            return Ok(());
        }
        let said = line_word(value.trim(), key)?;
        words.push(format!("{key}={said}"));
        Ok(())
    };
    let number = |words: &mut Vec<String>, key: &str, value: &str| -> Status {
        if value.trim().is_empty() {
            return Ok(());
        }
        let said = number_word(value, key)?;
        words.push(format!("{key}={said}"));
        Ok(())
    };
    // The object's own keys first: after a scope's WHERE every key=value is a condition.
    for (key, value) in [
        ("name", &form.name),
        ("layer", &form.layer),
        ("colour", &form.colour),
        ("material", &form.material),
        ("facility", &form.facility),
        ("heights", &form.heights),
        ("surface", &form.surface),
    ] {
        text(&mut words, key, value.as_str())?;
    }
    for (key, value) in [("crossfall", &form.crossfall), ("dz", &form.dz)] {
        number(&mut words, key, value.as_str())?;
    }
    for (key, value) in &form.parameters {
        let spec = find_parameter(form.kind, key);
        if spec.is_some_and(|spec| spec.r#type == ParameterType::Number) {
            number(&mut words, key.as_str(), value.as_str())?;
        } else {
            text(&mut words, key.as_str(), value.as_str())?;
        }
    }
    match form.place {
        Place::Alignment => {
            if form.alignment.trim().is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "choose the alignment",
                ));
            }
            if form.station.trim().is_empty() {
                return Err(Error::new(ErrorCode::InvalidArgument, "give the station"));
            }
            let name = line_word(form.alignment.trim(), "the alignment")?;
            words.push(String::from("ALIGNMENT"));
            words.push(name);
            for (key, value) in [
                ("station", &form.station),
                ("to", &form.to),
                ("offset", &form.offset),
                ("spacing", &form.spacing),
            ] {
                number(&mut words, key, value.as_str())?;
            }
        }
        Place::Lines | Place::DrawnPoints => {
            if form.scope.trim().is_empty() {
                // The scope's own reason (a colour that does not read, no layer ticked, a view
                // that has closed) when it gave one: "say which part" would be wrong to someone
                // who has.
                let message = if form.scope_error.is_empty() {
                    String::from("say which part of the drawing")
                } else {
                    form.scope_error.clone()
                };
                return Err(Error::new(ErrorCode::InvalidArgument, message));
            }
            number(&mut words, "spacing", &form.spacing)?;
            // The scope ends the line, its WHERE with it.
            words.push(String::from(if form.place == Place::Lines {
                "ALONG"
            } else {
                "AT"
            }));
            words.push(String::from("FROM"));
            words.push(String::from(form.scope.trim()));
        }
        Place::Points => {
            let points: Vec<&str> = form
                .points
                .split(' ')
                .filter(|point| !point.is_empty())
                .collect();
            if points.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "give the points: x,y[,z] x,y[,z] ...",
                ));
            }
            for point in &points {
                if !reads_as_point(point) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "a point is x,y or x,y,z",
                        *point,
                    ));
                }
            }
            words.push(String::from("AT"));
            words.extend(points.iter().map(|point| String::from(*point)));
        }
    }
    if form.preview {
        words.push(String::from("PREVIEW"));
    }
    Ok(words.join(" "))
}

/// The object `kind` and `parameters` make on a straight, level road - what the preview draws: a
/// linear kind 10 m along it, a point kind 2 m off it, a culvert across it. Parameters that do not
/// read are left at their defaults.
pub fn road_object_preview(kind: RoadObjectKind, parameters: &[(String, String)]) -> BimModel {
    // A straight road 30 m long on level ground.
    let road = Alignment {
        name: String::from("preview"),
        horizontal: HorizontalAlignment {
            pis: vec![
                AlignmentPi {
                    point: Point2::new(0.0, 0.0),
                    ..AlignmentPi::default()
                },
                AlignmentPi {
                    point: Point2::new(30.0, 0.0),
                    ..AlignmentPi::default()
                },
            ],
            ..HorizontalAlignment::default()
        },
        vertical: Some(VerticalAlignment {
            pvis: vec![
                ProfilePvi {
                    station: 0.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
                ProfilePvi {
                    station: 30.0,
                    elevation: 0.0,
                    curve_length: 0.0,
                },
            ],
        }),
        ..Alignment::default()
    };
    let mut placement = RoadObjectPlacement {
        alignment: road.name.clone(),
        ..RoadObjectPlacement::default()
    };
    match shape_of(kind) {
        KindShape::Linear => {
            placement.station = 10.0;
            placement.to = Some(20.0);
        }
        KindShape::Point => {
            placement.station = 15.0;
            placement.offset = 2.0;
        }
        KindShape::Crossing => placement.station = 15.0,
    }
    let mut object = RoadObject {
        id: String::from("preview"),
        kind,
        placement,
        ..RoadObject::default()
    };
    for (key, value) in parameters {
        // What does not read is left at its default: the line will say why.
        let _ = apply_key(&mut object, key, value);
    }
    let lent = &road;
    let context = PlacementContext {
        alignment: Some(Box::new(
            move |name: &str| {
                if name == lent.name { Some(lent) } else { None }
            },
        )),
        surface: None,
    };
    build_road_object(&context, &object)
}

/// What the preview is painted on: the palette's base, as the C++ widget's was.
pub fn object_preview_base() -> Color {
    palette::panel()
}

fn colour_of(colour: &katana_entity::Color) -> Color {
    Color::rgba(colour.r, colour.g, colour.b, colour.a)
}

/// QColor::darker(factor) for a factor over 100: the value of the colour's HSV divided by
/// factor/100, its hue and saturation kept - each channel scaled alike, which keeps both. (Qt
/// scales its 16-bit value; at 8 bits a channel this may differ from Qt's by one.)
fn darker(colour: Color, factor: u32) -> Color {
    let scale = |channel: u8| ((u32::from(channel) * 100) / factor) as u8;
    Color::rgba(scale(colour.r), scale(colour.g), scale(colour.b), colour.a)
}

fn is_headwall(object: &BimObject) -> bool {
    object.id.contains("/headwall/")
}

/// The object the parameters make, drawn as it is seen: a barrier, a wall, a fence, a marking
/// along its line - its cross-section; a post, a sign, a light pole from the front; a culvert
/// across its barrel. The ground is the line at level 0; a one metre bar gives the scale. `rect`
/// is the preview's (its paintEvent's rect()).
pub fn paint_object_preview(
    canvas: &mut dyn Canvas,
    rect: RectF,
    model: &BimModel,
    across_x: bool,
) {
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    canvas.fill_rect(rect, &Brush::solid(object_preview_base()));
    if model.objects.is_empty() {
        canvas.set_pen(Pen::new(palette::text()));
        let said = model
            .problems
            .first()
            .map_or("Nothing to show", |problem| problem.message.as_str());
        canvas.draw_text_in(
            rect.adjusted(8.0, 8.0, -8.0, -8.0),
            TextFlags::CENTER.wrapped(),
            said,
        );
        return;
    }
    // The view's horizontal axis: y along an alignment's x, x across a culvert's.
    let across = |point: &Point3| if across_x { point.x } else { -point.y };
    let mut h0 = f64::INFINITY;
    let mut h1 = -h0;
    let mut z0: f64 = 0.0;
    let mut z1: f64 = 0.0;
    for object in &model.objects {
        for vertex in &object.mesh.vertices {
            h0 = cpp::min(h0, across(vertex));
            h1 = cpp::max(h1, across(vertex));
            z0 = cpp::min(z0, vertex.z);
            z1 = cpp::max(z1, vertex.z);
        }
    }
    let margin = 16.0;
    let span_h = cpp::max(h1 - h0, 1.0);
    let span_z = cpp::max(z1 - z0, 1.0);
    let width = rect.width;
    let height = rect.height;
    let scale = cpp::min(
        (width - 2.0 * margin) / span_h,
        (height - 2.0 * margin - 14.0) / span_z,
    );
    // The C++'s !(scale > 0.0): a NaN scale is refused too.
    if scale.is_nan() || scale <= 0.0 {
        return;
    }
    let centre_h = (h0 + h1) / 2.0;
    let map = |h: f64, z: f64| {
        PointF::new(
            rect.x + width / 2.0 + (h - centre_h) * scale,
            rect.y + height - margin - 14.0 - (z - z0) * scale,
        )
    };
    // A culvert's headwalls stand round the ends of its barrel, and seen along it each is a slab as
    // large as the barrel and more: filled they would hide it, whichever is drawn first or last,
    // in a colour the barrel shares. They are drawn first, as outlines, and the barrel over them.
    for object in &model.objects {
        if !is_headwall(object) || object.mesh.vertices.is_empty() {
            continue;
        }
        let mut a0 = f64::INFINITY;
        let mut a1 = -a0;
        let mut c0 = a0;
        let mut c1 = -a0;
        for vertex in &object.mesh.vertices {
            a0 = cpp::min(a0, across(vertex));
            a1 = cpp::max(a1, across(vertex));
            c0 = cpp::min(c0, vertex.z);
            c1 = cpp::max(c1, vertex.z);
        }
        canvas.set_brush(Brush::none());
        canvas.set_pen(Pen::with(
            darker(colour_of(&object.colour), 160),
            1.5,
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Bevel,
        ));
        canvas.draw_rect(RectF::from_corners(map(a0, c1), map(a1, c0)));
    }
    for object in &model.objects {
        if is_headwall(object) {
            continue;
        }
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(colour_of(&object.colour)));
        for face in &object.mesh.faces {
            let triangle: Vec<PointF> = face
                .iter()
                .filter_map(|&index| object.mesh.vertices.get(index as usize))
                .map(|vertex| map(across(vertex), vertex.z))
                .collect();
            canvas.draw_polygon(&triangle, FillRule::OddEven);
        }
    }
    // The ground, and a metre for the scale.
    canvas.set_pen(Pen::with(
        palette::text(),
        1.0,
        PenStyle::Solid,
        PenCap::Square,
        PenJoin::Bevel,
    ));
    let ground = map(0.0, 0.0).y;
    canvas.draw_line(
        PointF::new(rect.x + margin / 2.0, ground),
        PointF::new(rect.x + width - margin / 2.0, ground),
    );
    let bar = PointF::new(rect.x + margin, rect.y + height - 8.0);
    canvas.draw_line(bar, bar + PointF::new(scale, 0.0));
    canvas.draw_text(bar + PointF::new(scale + 4.0, 4.0), "1 m");
}

/// One page's parameter field: a text field, or a choice whose first item is the default (no
/// data) and the rest the kind's words.
struct ParameterField {
    key: &'static str,
    object_name: String,
    label: String,
    tip: &'static str,
    placeholder: String,
    text: String,
    choice: Option<Combo>,
}

impl ParameterField {
    fn value(&self) -> String {
        match &self.choice {
            Some(choice) => choice.current_data(),
            None => String::from(self.text.trim()),
        }
    }
}

/// Which controls are enabled, as the last refresh left them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Enabled {
    alignment: bool,
    station: bool,
    offset: bool,
    to: bool,
    spacing: bool,
    crossfall: bool,
    points: bool,
    scope: bool,
    add: bool,
}

struct Body {
    watcher: DocumentWatcher,
    executor: bool,
    kind_index: usize,
    pages: Vec<Vec<ParameterField>>,
    source: String,
    place: Place,
    alignment: Combo,
    station: String,
    to: String,
    offset: String,
    spacing: String,
    points: String,
    scope: ScopeFilterWidget,
    heights: Combo,
    surface: Combo,
    crossfall: String,
    dz: String,
    name: String,
    layer: String,
    colour: String,
    material: String,
    facility: String,
    line: String,
    reply: String,
    status: String,
    /// Whether the status says what is wrong with the form - which a refresh clears once the form
    /// is right - rather than how a run went, which a refresh keeps: the run's own change to the
    /// drawing reloads the form, and that reload wiped "Added - one undo step." as it was shown.
    status_says_form: bool,
    enabled: Enabled,
    preview_model: BimModel,
    preview_across_x: bool,
    raster: Option<PreviewRaster>,
    texture: Option<egui::TextureHandle>,
    close: bool,
}

impl Body {
    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    fn current_kind(&self) -> RoadObjectKind {
        let kinds = road_object_kinds();
        kinds[self.kind_index.min(kinds.len() - 1)]
    }

    fn form(&self) -> RoadObjectForm {
        let kind = self.current_kind();
        let mut made = RoadObjectForm {
            kind,
            place: self.place,
            alignment: self.alignment.current_text(),
            station: self.station.clone(),
            to: if self.enabled.to {
                self.to.clone()
            } else {
                String::new()
            },
            offset: self.offset.clone(),
            spacing: if self.enabled.spacing {
                self.spacing.clone()
            } else {
                String::new()
            },
            points: self.points.clone(),
            heights: self.heights.current_data(),
            surface: String::from(self.surface.current_text().trim()),
            crossfall: if self.enabled.crossfall {
                self.crossfall.clone()
            } else {
                String::new()
            },
            dz: self.dz.clone(),
            name: self.name.clone(),
            layer: self.layer.clone(),
            colour: self.colour.clone(),
            material: self.material.clone(),
            facility: self.facility.clone(),
            ..RoadObjectForm::default()
        };
        if made.place == Place::Lines || made.place == Place::DrawnPoints {
            match self.scope.verb_words() {
                Ok(words) => made.scope = words,
                Err(error) => made.scope_error = error.message,
            }
        }
        if let Some(page) = self.pages.get(kind as usize) {
            for parameter in page {
                let value = parameter.value();
                if !value.is_empty() {
                    made.parameters.push((String::from(parameter.key), value));
                }
            }
        }
        made
    }

    /// Makes the line again from the fields, and the preview: every edit's.
    fn refresh(&mut self) {
        let kind = self.current_kind();
        let shape = shape_of(kind);
        let place = self.place;
        let on_alignment = place == Place::Alignment;
        // The profile is carried out to an offset at the crossfall; an object that stands on a
        // surface, or on the heights of a line, has none to carry.
        let heights_word = self.heights.current_data();
        self.enabled = Enabled {
            alignment: on_alignment,
            station: on_alignment,
            offset: on_alignment,
            to: on_alignment && shape != KindShape::Crossing,
            spacing: shape == KindShape::Point
                && place != Place::Points
                && place != Place::DrawnPoints,
            crossfall: on_alignment
                && (heights_word == "profile"
                    || (heights_word.is_empty() && self.surface.current_text().trim().is_empty())),
            points: place == Place::Points,
            scope: place == Place::Lines || place == Place::DrawnPoints,
            add: self.enabled.add,
        };
        self.source = String::from(kind_source(kind));
        let form = self.form();
        match road_object_add_line(&form) {
            Ok(line) => {
                self.line = line;
                if self.status_says_form {
                    self.status.clear();
                    self.status_says_form = false;
                }
                self.enabled.add = true;
            }
            Err(error) => {
                self.line.clear();
                self.status = error.message;
                self.status_says_form = true;
                self.enabled.add = false;
            }
        }
        self.preview_model = road_object_preview(kind, &form.parameters);
        self.preview_across_x = shape == KindShape::Crossing;
    }

    fn reload(&mut self, document: &Document) {
        if !self.alive() {
            self.scope.reload_views();
            return;
        }
        self.scope.reload(document.model());
        let kept = self.alignment.current_text();
        self.alignment.clear();
        for name in document.model().alignments.names() {
            self.alignment.add_item(&name);
        }
        let index = self
            .alignment
            .find_text(&kept)
            .or((self.alignment.count() > 0).then_some(0));
        self.alignment.set_current_index(index);
        // With no alignment to place along, along lines is the place to start.
        if self.alignment.count() == 0 && self.place == Place::Alignment {
            self.place = Place::Lines;
        }
        let kept = self.surface.current_text();
        self.surface.clear();
        self.surface.add_item("");
        for surface in document.surfaces().all() {
            self.surface.add_item(&surface.name);
        }
        self.surface.set_current_text(&kept);
        self.refresh();
    }

    fn run(&mut self, host: &mut dyn Host, as_preview: bool) {
        let mut asked = self.form();
        asked.preview = as_preview;
        let made = match road_object_add_line(&asked) {
            Ok(made) => made,
            Err(error) => {
                self.status = error.message;
                self.status_says_form = true;
                return;
            }
        };
        self.status_says_form = false;
        if !self.executor {
            self.status = String::from("There is no command line to run it on here.");
            return;
        }
        let outcome = host.run(&made);
        self.reply = if outcome.ok {
            outcome.reply.clone()
        } else {
            outcome.error.clone()
        };
        if !outcome.ok {
            self.status = format!("Refused: {}", outcome.error);
            return;
        }
        // Worded from the reply: a scope that takes nothing is not refused (the verb reports
        // "added=0" and makes no undo step), and "Added - one undo step." over that would be
        // false.
        let lead = if as_preview {
            "preview id="
        } else {
            "added id="
        };
        let count = outcome
            .reply
            .split('\n')
            .filter(|each| each.starts_with(lead))
            .count();
        self.status = if count == 0 {
            String::from(if as_preview {
                "Previewed: the scope took nothing to place on."
            } else {
                "Nothing added: the scope took nothing to place on."
            })
        } else if as_preview {
            if count == 1 {
                String::from("Previewed: nothing was changed.")
            } else {
                format!("Previewed {count}: nothing was changed.")
            }
        } else if count == 1 {
            String::from("Added - one undo step.")
        } else {
            format!("Added {count} - one undo step.")
        };
    }

    // ---- drawing --------------------------------------------------------------------------------

    fn page_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        let index = self.kind_index;
        let Some(page) = self.pages.get_mut(index) else {
            return false;
        };
        egui::Grid::new(("roadObjectPage", index)).show(ui, |ui| {
            for parameter in page.iter_mut() {
                ui.label(parameter.label.as_str());
                let response = match &mut parameter.choice {
                    Some(choice) => {
                        let edited = choice.ui(ui, &parameter.object_name);
                        changed |= edited;
                        None
                    }
                    None => Some(line_with(
                        ui,
                        &parameter.object_name,
                        &mut parameter.text,
                        LineOptions {
                            placeholder: &parameter.placeholder,
                            ..LineOptions::default()
                        },
                    )),
                };
                if let Some(response) = response {
                    if response.changed() {
                        changed = true;
                    }
                    response.on_hover_text(parameter.tip);
                }
                ui.end_row();
            }
        });
        changed
    }

    fn body_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        // ---- the pages and the preview ----
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let titles: Vec<&str> = road_object_kinds()
                    .iter()
                    .map(|&kind| kind_title(kind))
                    .collect();
                if tabs(ui, "roadObjectKinds", &titles, &mut self.kind_index).changed() {
                    changed = true;
                }
                if self.page_ui(ui) {
                    changed = true;
                }
                label_as(
                    ui,
                    "roadObjectKindSource",
                    &self.source,
                    egui::Label::new(self.source.as_str()).wrap(),
                );
            });
            let model = &self.preview_model;
            let across_x = self.preview_across_x;
            preview(
                ui,
                "roadObjectPreview",
                egui::vec2(220.0, 160.0),
                self.raster.as_ref(),
                &mut self.texture,
                &mut |canvas: &mut dyn Canvas| {
                    paint_object_preview(
                        canvas,
                        RectF::new(0.0, 0.0, 220.0, 160.0),
                        model,
                        across_x,
                    );
                },
            );
        });

        // ---- where it goes ----
        let placement = ui.group(|ui| {
            ui.strong("Placement");
            egui::Grid::new("roadObjectPlaceGrid").show(ui, |ui| {
                if radio(
                    ui,
                    "roadObjectPlaceAlignment",
                    "Along an &alignment",
                    self.place == Place::Alignment,
                )
                .clicked()
                    && self.place != Place::Alignment
                {
                    self.place = Place::Alignment;
                    changed = true;
                }
                ui.add_enabled_ui(self.enabled.alignment, |ui| {
                    if self.alignment.ui(ui, "roadObjectAlignment") {
                        changed = true;
                    }
                });
                ui.label("station");
                changed |= enabled_field(
                    ui,
                    self.enabled.station,
                    "roadObjectStation",
                    &mut self.station,
                    "chainage",
                );
                ui.label("to");
                changed |= enabled_field(
                    ui,
                    self.enabled.to,
                    "roadObjectTo",
                    &mut self.to,
                    "end chainage",
                );
                ui.end_row();
                ui.label("");
                ui.label("");
                ui.label("offset");
                changed |= enabled_field(
                    ui,
                    self.enabled.offset,
                    "roadObjectOffset",
                    &mut self.offset,
                    "0 (left of the centreline +)",
                );
                ui.label("spacing");
                changed |= enabled_field(
                    ui,
                    self.enabled.spacing,
                    "roadObjectSpacing",
                    &mut self.spacing,
                    "one every ... m",
                );
                ui.end_row();
                if radio(
                    ui,
                    "roadObjectPlacePoints",
                    "At points &typed",
                    self.place == Place::Points,
                )
                .clicked()
                    && self.place != Place::Points
                {
                    self.place = Place::Points;
                    changed = true;
                }
                changed |= enabled_field(
                    ui,
                    self.enabled.points,
                    "roadObjectPoints",
                    &mut self.points,
                    "x,y[,z] x,y[,z] ...",
                );
                ui.end_row();
                if radio(
                    ui,
                    "roadObjectPlaceLines",
                    "Along &lines in the drawing",
                    self.place == Place::Lines,
                )
                .clicked()
                    && self.place != Place::Lines
                {
                    self.place = Place::Lines;
                    changed = true;
                }
                // Not "&drawing": Add is A&dd, and two widgets with one mnemonic make the key go
                // to neither - it cycles between them.
                if radio(
                    ui,
                    "roadObjectPlaceDrawnPoints",
                    "At points in the dra&wing",
                    self.place == Place::DrawnPoints,
                )
                .clicked()
                    && self.place != Place::DrawnPoints
                {
                    self.place = Place::DrawnPoints;
                    changed = true;
                }
                ui.end_row();
            });
            ui.add_enabled_ui(self.enabled.scope, |ui| {
                if self.scope.ui(ui) {
                    changed = true;
                }
            });
        });
        custom(
            ui,
            &placement.response,
            ControlRecord::new("roadObjectPlacement", ControlKind::Other),
        );

        // ---- what it stands on, and what it is called ----
        ui.horizontal_top(|ui| {
            let heights = ui.group(|ui| {
                ui.strong("Heights");
                egui::Grid::new("roadObjectHeightsForm").show(ui, |ui| {
                    ui.label("Heights from");
                    if self.heights.ui(ui, "roadObjectHeights") {
                        changed = true;
                    }
                    ui.end_row();
                    ui.label("Surface");
                    if self.surface.ui(ui, "roadObjectSurface") {
                        changed = true;
                    }
                    ui.end_row();
                    ui.label("Crossfall (%)");
                    changed |= enabled_field(
                        ui,
                        self.enabled.crossfall,
                        "roadObjectCrossfall",
                        &mut self.crossfall,
                        "0 %",
                    );
                    ui.end_row();
                    ui.label("Raised by (m)");
                    changed |= enabled_field(ui, true, "roadObjectDz", &mut self.dz, "0 m");
                    ui.end_row();
                });
            });
            custom(
                ui,
                &heights.response,
                ControlRecord::new("roadObjectHeightsGroup", ControlKind::Other),
            );
            let common = ui.group(|ui| {
                ui.strong("Object");
                egui::Grid::new("roadObjectCommonForm").show(ui, |ui| {
                    for (label_text, name, value, placeholder) in [
                        (
                            "Name",
                            "roadObjectName",
                            &mut self.name,
                            "the kind and its id",
                        ),
                        (
                            "Layer",
                            "roadObjectLayer",
                            &mut self.layer,
                            "road/objects/<kind>",
                        ),
                        (
                            "Colour",
                            "roadObjectColour",
                            &mut self.colour,
                            "#RRGGBB or a colour name",
                        ),
                        (
                            "Material",
                            "roadObjectMaterial",
                            &mut self.material,
                            "the kind's",
                        ),
                        (
                            "Road",
                            "roadObjectFacility",
                            &mut self.facility,
                            "the road it is part of",
                        ),
                    ] {
                        ui.label(label_text);
                        changed |= enabled_field(ui, true, name, value, placeholder);
                        ui.end_row();
                    }
                });
            });
            custom(
                ui,
                &common.response,
                ControlRecord::new("roadObjectCommon", ControlKind::Other),
            );
        });
        changed
    }

    fn footer_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            line_with(
                ui,
                "roadObjectLine",
                &mut self.line,
                LineOptions {
                    read_only: true,
                    width: Some(560.0),
                    ..LineOptions::default()
                },
            );
            let enabled = self.enabled.add;
            ui.add_enabled_ui(enabled, |ui| {
                if button(ui, "roadObjectPreviewRun", "&Preview", "").clicked() {
                    self.run(host, true);
                }
                if button(ui, "roadObjectAdd", "A&dd", "").clicked() {
                    self.run(host, false);
                }
            });
            if button(ui, "roadObjectClose", "&Close", "").clicked() {
                self.close = true;
            }
        });
        text_with(
            ui,
            "roadObjectReply",
            &mut self.reply,
            TextOptions {
                read_only: true,
                rows: 4,
                monospace: false,
            },
        );
        label(ui, "roadObjectStatus", &self.status);
    }
}

/// A field drawn enabled or not: true when a person or a driver changed it.
fn enabled_field(
    ui: &mut Ui,
    enabled: bool,
    name: &str,
    value: &mut String,
    placeholder: &str,
) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        field(ui, name, value, placeholder, "").changed()
    })
    .inner
}

/// The Road Objects placing dialog (see the module comment).
pub struct RoadObjectDialog {
    frame: DialogFrame,
    open: bool,
    body: Body,
}

impl RoadObjectDialog {
    /// The dialog over `document`, shown, on the first kind's page.
    pub fn new(document: &Document) -> Self {
        let mut pages = Vec::new();
        for &kind in road_object_kinds() {
            let mut fields = Vec::new();
            for spec in parameters_of(kind) {
                let mut label_text = String::from(spec.key);
                if !spec.unit.is_empty() {
                    label_text.push_str(&format!(" ({})", spec.unit));
                }
                let choice = (spec.r#type == ParameterType::Choice).then(|| {
                    let mut combo = Combo::new();
                    combo.add_item_with_data(&format!("default ({})", spec.default_text), "");
                    for &word in spec.choices {
                        combo.add_item_with_data(word, word);
                    }
                    combo
                });
                let placeholder = if spec.default_text.is_empty() {
                    String::from("none")
                } else {
                    format!("default: {}", spec.default_text)
                };
                fields.push(ParameterField {
                    key: spec.key,
                    object_name: format!("roadObject_{}_{}", kind_word(kind), spec.key),
                    label: label_text,
                    tip: spec.description,
                    placeholder,
                    text: String::new(),
                    choice,
                });
            }
            pages.push(fields);
        }
        let mut heights = Combo::new();
        heights.add_item_with_data("automatic", "");
        for word in ["profile", "surface", "line"] {
            heights.add_item_with_data(word, word);
        }
        let mut scope = ScopeFilterWidget::new("roadObjectSource");
        scope.set_choice(ScopeChoice::Selection);
        let mut body = Body {
            watcher: DocumentWatcher::new(document),
            executor: true,
            kind_index: 0,
            pages,
            source: String::new(),
            place: Place::Alignment,
            alignment: Combo::new(),
            station: String::new(),
            to: String::new(),
            offset: String::new(),
            spacing: String::new(),
            points: String::new(),
            scope,
            heights,
            surface: Combo::editable(),
            crossfall: String::new(),
            dz: String::new(),
            name: String::new(),
            layer: String::new(),
            colour: String::new(),
            material: String::new(),
            facility: String::new(),
            line: String::new(),
            reply: String::new(),
            status: String::new(),
            status_says_form: false,
            enabled: Enabled::default(),
            preview_model: BimModel::default(),
            preview_across_x: false,
            raster: None,
            texture: None,
            close: false,
        };
        body.reload(document);
        body.refresh();
        Self {
            frame: DialogFrame::new("roadObjectDialog", "Road Objects"),
            open: true,
            body,
        }
    }

    /// A dialog with no executor (the C++'s empty CommandRunner): it says it cannot run.
    pub fn without_executor(mut self) -> Self {
        self.body.executor = false;
        self
    }

    /// The workspace's open views, for the scope's "What a view shows" (the C++ context's views).
    pub fn set_views(&mut self, views: crate::customisation::scope_filter_widget::ViewsFn) {
        self.body.scope.views = Some(views);
    }

    /// The window's raster canvas, for the preview to be painted on screen.
    pub fn set_raster(&mut self, raster: PreviewRaster) {
        self.body.raster = Some(raster);
    }

    /// The scope and filter, as Qt's widget tree gave a test its controls.
    pub fn scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.body.scope
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// QWidget::show; the views, alignments, surfaces and layers read again when it was hidden
    /// (showEvent: the views open now, not when the dialog was made).
    pub fn show(&mut self, document: &Document) {
        if !self.open {
            self.open = true;
            self.body.reload(document);
        }
    }

    pub fn hide(&mut self) {
        self.open = false;
    }

    /// The page of `kind`.
    pub fn show_kind(&mut self, kind: RoadObjectKind) {
        let index = kind as usize;
        if self.body.kind_index != index {
            self.body.kind_index = index;
            self.body.refresh();
        }
    }

    pub fn current_kind(&self) -> RoadObjectKind {
        self.body.current_kind()
    }

    /// The form as the fields hold it.
    pub fn form(&self) -> RoadObjectForm {
        self.body.form()
    }

    /// The alignments, surfaces, layers and views again.
    pub fn reload(&mut self, document: &Document) {
        self.body.reload(document);
    }

    /// The preview painted into `canvas` over `rect`, as it is on screen.
    pub fn paint_preview(&self, canvas: &mut dyn Canvas, rect: RectF) {
        paint_object_preview(
            canvas,
            rect,
            &self.body.preview_model,
            self.body.preview_across_x,
        );
    }

    /// Draws the dialog for this frame, when it is shown: the edits of the frame refresh the line
    /// before the footer draws it. Then the watcher's delivery, as the C++'s event loop delivered
    /// it after a click (corridor_dialog's `ui` says why).
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if self.open {
            let ctx = ui.ctx().clone();
            let mut lent = (&mut self.body, &mut *host);
            let shown = self.frame.show(
                &ctx,
                &mut lent,
                |ui, lent| {
                    let drawn = ui.vertical(|ui| lent.0.body_ui(ui));
                    custom(
                        ui,
                        &drawn.response,
                        ControlRecord::new("roadObjectBody", ControlKind::Other),
                    );
                    if drawn.inner {
                        lent.0.refresh();
                    }
                },
                |ui, lent| lent.0.footer_ui(ui, &mut *lent.1),
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
