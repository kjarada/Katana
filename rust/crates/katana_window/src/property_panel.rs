//! The Properties panel's tree (property_panel.hpp): what the selection is and holds, read one level
//! at a time.
//!
//! The panel was a two-column table built whole on every refresh. A string brought in from a survey
//! archive carries its attributes flattened into '/' names - thirty per vertex over a thousand
//! vertices is thirty thousand rows - and every one of them was made before the panel could show
//! the first (the owner's request of 2026-09-26: "i want the attributes in the properties to show as
//! a tree structure, because some lines are very long and contain lots of attributes in the
//! vertices, so need a smart way to load all these information"). Now:
//!
//! * the rows are GROUPS - General, Geometry, Vertices, Attributes, Source - and the attributes are
//!   the tree their names make, read by cad's property_outline (the reading PROP TREE replies with,
//!   so the panel and an agent cannot disagree about it);
//! * a level is read only when it is opened, and made a page at a time (PROPERTY_PAGE rows): a
//!   "Show more" row at its end makes the next page;
//! * a vertex is one row, its position and height, with its own attributes beneath it;
//! * the filter box above lists every property of the selection whose name or value holds the words
//!   typed, however deep, a page at a time;
//! * which rows were open survives a refresh, so an undo elsewhere does not fold up the vertex
//!   being read.
//!
//! Several entities are read as one, as the attribute manager reads them: a value shows only where
//! all of them agree, else "<varies>". It edits nothing.
//!
//! THE RUST SHAPE. The C++'s levels held the Document by pointer and read it when opened. A level
//! here is handed the drawing when it is read and when its rows are made (`LevelReader`,
//! `RowReader`): the panel keeps no reference to it, and `show_selection` and every frame lend it.
//! The rows are addressed by their path of indices from the top (`RowPath`), as a QModelIndex's
//! parents are; a level's Show more row is the index after its last row. The tree is recorded for a
//! driver as `propertyTable` (the table it replaced was so named, so --report and DRIVE's '?' read
//! it): every row shown, depth first down every open row, "label | value" (main.cpp's report of an
//! item view). A row's tip is plain text, its name on the first line: egui's tooltips are not rich
//! text, where Qt's were.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use katana_cad::Document;
use katana_cad::annotation::label_layout::shown_label_text;
use katana_cad::annotation::leader_presets::{leader_context, preset_info, recognise_preset};
use katana_cad::property_outline::{
    PropertyOutlineEntry, has_properties_under, natural_less, property_outline_of,
};
use katana_cad::survey_coding::code_property_candidates;
use katana_core::text::{lowered, parse_integer};
use katana_entity::anchor::describe;
use katana_entity::entity::{
    ELEVATIONS_PROPERTY, heights_of, property_to_string, to_string, type_name,
};
use katana_entity::leader_values::leader_note;
use katana_entity::{Entity, EntityId, Geometry, LeaderContext, Model, PropertyMap, PropertyValue};
use katana_geometry::Point2;
use katana_math::numerics::{RAD_TO_DEG, normalize_angle};

use crate::drive::{
    Action, ControlKind, ControlRecord, LineOptions, action_item, custom, line_with,
};
use crate::format::{grouped, plan_number, plan_point};
use crate::host::Clipboard;
use crate::theme::palette;

/// Rows made at a time, in a level and in the filter's list: enough to fill a tall panel twice, few
/// enough to make in well under a frame.
pub const PROPERTY_PAGE: usize = 200;

/// The longest value drawn in the Value column, and the longest a tip carries: a per-vertex list of
/// heights is thousands of characters, and a row that long, or a tip, is a wall nobody reads. The
/// whole value is PROP LIST's and Edit > Attributes'.
const SHOWN_CHARACTERS: usize = 160;
const TIP_CHARACTERS: usize = 1200;

/// Opened groups are remembered by their key path, the keys joined by a character no key holds.
const PATH_SEPARATOR: char = '\u{1f}';
const PATH_SEPARATOR_TEXT: &str = "\u{1f}";

/// Typing in the filter waits for a pause: every keystroke would otherwise search the whole
/// selection again.
const FILTER_DELAY: Duration = Duration::from_millis(200);

/// A level's rows, read when the level is opened, lent the drawing.
pub type LevelReader = Rc<dyn Fn(&Document) -> PropertyLevel>;
/// A level's `index`-th row, made when its page is, lent the drawing.
pub type RowReader = Rc<dyn Fn(&Document, usize) -> PropertyRow>;

/// A level of rows, made on demand: `count` of them, the i-th by `row`.
#[derive(Clone, Default)]
pub struct PropertyLevel {
    pub count: usize,
    pub row: Option<RowReader>,
}

impl PropertyLevel {
    /// The level's `index`-th row; an empty row for a level with no reader.
    pub fn row(&self, document: &Document, index: usize) -> PropertyRow {
        self.row
            .as_ref()
            .map(|row| row(document, index))
            .unwrap_or_default()
    }
}

/// One row of the tree.
#[derive(Clone, Default)]
pub struct PropertyRow {
    pub label: String,
    pub value: String,
    /// The whole name and value when the columns cut them short.
    pub tip: String,
    /// Its identity among its siblings, which is what an opened row is remembered by across a
    /// refresh.
    pub key: String,
    /// A group: drawn bold.
    pub heading: bool,
    /// A count or <varies>, not a value: drawn muted.
    pub muted: bool,
    /// Opened when first shown.
    pub open: bool,
    /// Its level, read when it is first opened; None for a leaf.
    pub children: Option<LevelReader>,
}

/// A row's place: its index among its siblings, then its children's, from the top.
pub type RowPath = Vec<usize>;

// ---- what the rows say ------------------------------------------------------------------------

/// `text` cut to SHOWN_CHARACTERS, with an ellipsis.
fn shown(text: &str) -> String {
    if text.chars().count() <= SHOWN_CHARACTERS {
        return String::from(text);
    }
    let mut cut: String = text.chars().take(SHOWN_CHARACTERS).collect();
    cut.push('\u{2026}');
    cut
}

/// A tip: the name, then the text, cut to TIP_CHARACTERS with how long it is.
fn tip_for(name: &str, text: &str) -> String {
    let length = text.chars().count();
    let body = if length <= TIP_CHARACTERS {
        String::from(text)
    } else {
        format!(
            "{}\u{2026} ({} characters)",
            text.chars().take(TIP_CHARACTERS).collect::<String>(),
            grouped(length as u64)
        )
    };
    format!("{name}\n{body}")
}

fn count_of(count: usize, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        grouped(count as u64),
        if count == 1 { one } else { many }
    )
}

fn leaf(label: &str, value: &str) -> PropertyRow {
    PropertyRow {
        label: String::from(label),
        value: shown(value),
        key: String::from(label),
        tip: if value.chars().count() > SHOWN_CHARACTERS {
            tip_for(label, value)
        } else {
            String::new()
        },
        ..PropertyRow::default()
    }
}

fn heading(label: &str, key: &str, summary: &str) -> PropertyRow {
    PropertyRow {
        label: String::from(label),
        key: String::from(key),
        value: String::from(summary),
        heading: true,
        muted: true,
        ..PropertyRow::default()
    }
}

/// A level of rows already made.
pub fn fixed_level(rows: Vec<PropertyRow>) -> PropertyLevel {
    let held = Rc::new(rows);
    PropertyLevel {
        count: held.len(),
        row: Some(Rc::new(move |_: &Document, index: usize| {
            held.get(index).cloned().unwrap_or_default()
        })),
    }
}

/// Where a level's properties come from: the entities, looked up afresh each time a level is read,
/// so a level opened after an edit reads the drawing as it is (and one whose entity has gone reads
/// nothing).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Subject {
    ids: Vec<EntityId>,
    /// The entities' source records (Entity::metadata) rather than their properties.
    metadata: bool,
    /// One entity whose vertices are listed in a Vertices group, which then holds their attributes
    /// and heights: how many.
    vertices_shown: usize,
}

impl Subject {
    fn maps<'a>(&self, document: &'a Document) -> Vec<&'a PropertyMap> {
        self.ids
            .iter()
            .filter_map(|&id| document.model().entities.find(id))
            .map(|entity| {
                if self.metadata {
                    &entity.metadata
                } else {
                    &entity.properties
                }
            })
            .collect()
    }
}

/// A vertex's and a segment's number read as one: "Vertex 3".
fn entry_label(parent: &str, name: &str) -> String {
    let numbered = !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit());
    if numbered && (parent == "vertex" || parent == "segment") {
        return format!(
            "{} {name}",
            if parent == "vertex" {
                "Vertex"
            } else {
                "Segment"
            }
        );
    }
    if name.is_empty() {
        String::from("(unnamed)")
    } else {
        String::from(name)
    }
}

fn outline_row(subject: &Rc<Subject>, entry: &PropertyOutlineEntry, parent: &str) -> PropertyRow {
    let mut row = PropertyRow {
        key: entry.name.clone(),
        label: entry_label(parent, &entry.name),
        ..PropertyRow::default()
    };
    if let Some(value) = &entry.value {
        let text = property_to_string(value);
        row.value = shown(&text);
        row.tip = tip_for(&entry.path, &format!("{text} ({})", type_name(value)));
        // The per-vertex heights are shown on the vertices, where they belong, rather than as a
        // line of a thousand numbers.
        if subject.vertices_shown > 0 && !subject.metadata && entry.path == ELEVATIONS_PROPERTY {
            row.value = String::from("see Vertices");
            row.muted = true;
        }
    } else if entry.varies {
        row.value = String::from("<varies>");
        row.muted = true;
        row.tip = tip_for(
            &entry.path,
            &format!(
                "held by {} of the {} selected, not all alike",
                grouped(entry.holders as u64),
                grouped(subject.ids.len() as u64)
            ),
        );
    } else if entry.children > 0 {
        row.value = count_of(entry.values, "value", "values");
        row.muted = true;
    }
    if entry.children > 0 {
        let subject = Rc::clone(subject);
        let path = entry.path.clone();
        row.children = Some(Rc::new(move |document: &Document| {
            outline_level(document, &subject, &path)
        }));
    }
    row
}

fn outline_level(document: &Document, subject: &Rc<Subject>, path: &str) -> PropertyLevel {
    let maps = subject.maps(document);
    let mut entries = property_outline_of(&maps, path);
    // The vertices' own attributes are under Vertices, beside their places - unless some name no
    // vertex the list has (a vertex since removed), when the branch stays here too, so nothing held
    // is out of sight.
    if path.is_empty() && subject.vertices_shown > 0 && !subject.metadata {
        let listed = |vertex: &PropertyOutlineEntry| {
            parse_integer(&vertex.name).is_some_and(|number| {
                number >= 1 && (number as u64) <= subject.vertices_shown as u64
            })
        };
        if property_outline_of(&maps, "vertex").iter().all(listed) {
            entries.retain(|entry| entry.path != "vertex");
        }
    }
    let entries = Rc::new(entries);
    let subject = Rc::clone(subject);
    let parent = String::from(path);
    PropertyLevel {
        count: entries.len(),
        row: Some(Rc::new(move |_: &Document, index: usize| {
            entries
                .get(index)
                .map(|entry| outline_row(&subject, entry, &parent))
                .unwrap_or_default()
        })),
    }
}

/// The vertices of a geometry that is a list of them; empty for any other.
fn vertices_of(geometry: &Geometry) -> Vec<Point2> {
    match geometry {
        Geometry::Polyline(polyline) => polyline.vertices.clone(),
        Geometry::Leader(leader) => leader.vertices.clone(),
        _ => Vec::new(),
    }
}

fn vertex_level(document: &Document, subject: &Rc<Subject>) -> PropertyLevel {
    let Some(entity) = subject
        .ids
        .first()
        .and_then(|&id| document.model().entities.find(id))
    else {
        return PropertyLevel::default();
    };
    let points = Rc::new(vertices_of(&entity.geometry));
    let heights = Rc::new(heights_of(&entity.properties, points.len()));
    let subject = Rc::clone(subject);
    PropertyLevel {
        count: points.len(),
        row: Some(Rc::new(move |document: &Document, index: usize| {
            // Numbered from 1, as the archive numbers the attributes of each (vertex/1/...) and as
            // a person counts them.
            let number = format!("{}", index + 1);
            let mut value = points.get(index).map(plan_point).unwrap_or_default();
            // Absent is not zero: a vertex with no height shows none.
            if let Some(Some(height)) = heights.get(index) {
                value.push_str(", ");
                value.push_str(&plan_number(*height));
            }
            let path = format!("vertex/{number}");
            let has_attributes = subject
                .ids
                .first()
                .and_then(|&id| document.model().entities.find(id))
                .is_some_and(|now| has_properties_under(&now.properties, &path));
            let children: Option<LevelReader> = if has_attributes {
                let subject = Rc::clone(&subject);
                Some(Rc::new(move |document: &Document| {
                    outline_level(document, &subject, &path)
                }))
            } else {
                None
            };
            PropertyRow {
                label: format!("Vertex {number}"),
                key: number,
                value,
                children,
                ..PropertyRow::default()
            }
        })),
    }
}

fn degrees(radians: f64) -> String {
    format!("{} deg", plan_number(radians * RAD_TO_DEG))
}

/// What the geometry of one entity says, as rows. `leaders`: the drawing's surfaces and
/// alignments, which a leader's note may read.
fn geometry_rows(model: &Model, geometry: &Geometry, leaders: &LeaderContext) -> Vec<PropertyRow> {
    match geometry {
        Geometry::Point(g) => vec![leaf("Position", &plan_point(&g.position))],
        Geometry::Line(g) => vec![
            leaf("Start", &plan_point(&g.start)),
            leaf("End", &plan_point(&g.end)),
            leaf("Length", &plan_number(g.length())),
            leaf("Angle", &degrees(normalize_angle(g.delta().angle()))),
        ],
        Geometry::Arc(g) => vec![
            leaf("Centre", &plan_point(&g.center)),
            leaf("Radius", &plan_number(g.radius)),
            leaf("Start angle", &degrees(g.start_angle)),
            leaf("Sweep", &degrees(g.sweep)),
            leaf("Length", &plan_number(g.length())),
        ],
        Geometry::Polyline(g) => {
            let mut rows = vec![
                leaf("Vertices", &grouped(g.vertices.len() as u64)),
                leaf("Closed", if g.closed { "yes" } else { "no" }),
                leaf("Length", &plan_number(g.length())),
            ];
            if g.closed {
                rows.push(leaf("Area", &plan_number(g.area())));
            }
            rows
        }
        Geometry::Circle(g) => vec![
            leaf("Centre", &plan_point(&g.center)),
            leaf("Radius", &plan_number(g.radius)),
            leaf("Circumference", &plan_number(g.perimeter())),
            leaf("Area", &plan_number(g.area())),
        ],
        Geometry::Text(g) => {
            let mut rows = vec![
                leaf("Position", &plan_point(&g.position)),
                leaf("Text", &g.text),
                leaf("Height", &plan_number(g.height)),
                leaf("Rotation", &degrees(g.rotation)),
            ];
            if !g.style.is_empty() {
                rows.push(leaf("Text style", &g.style));
            }
            if g.paper_height > 0.0 {
                rows.push(leaf(
                    "Paper height",
                    &format!("{} mm", plan_number(g.paper_height)),
                ));
            }
            rows.push(leaf("Justify", to_string(g.justify)));
            rows
        }
        Geometry::Dimension(g) => {
            let mut rows = vec![
                leaf("Kind", to_string(g.kind)),
                leaf("Start", &plan_point(&g.start)),
                leaf("End", &plan_point(&g.end)),
                leaf("Measurement", &plan_number(g.measurement())),
                leaf("Offset", &plan_number(g.offset)),
            ];
            if g.uses_vertex() {
                rows.push(leaf("Vertex", &plan_point(&g.vertex)));
            }
            if g.start_ref.associated() || g.end_ref.associated() || g.vertex_ref.associated() {
                rows.push(leaf("Associative", "yes"));
            }
            rows
        }
        Geometry::Label(g) => {
            let mut rows = vec![leaf("Label style", &g.style)];
            if g.target != 0 {
                rows.push(leaf("Labels", &format!("{}", g.target)));
            } else {
                rows.push(leaf("Alignment", &g.alignment));
            }
            if g.part >= 0 {
                rows.push(leaf("Part", &format!("{}", g.part)));
            }
            if !g.rule.is_empty() {
                rows.push(leaf("Rule", &g.rule));
            }
            // What it says and where, as LABEL LIST replies them: the words through the same
            // resolver, so the two cannot disagree.
            rows.push(leaf("Text", &shown_label_text(model, g)));
            rows.push(leaf(
                "Position",
                &match &g.position {
                    Some(position) => format!("pinned at {}", plan_point(position)),
                    None => String::from("automatic"),
                },
            ));
            rows
        }
        Geometry::Leader(g) => {
            // The note as it is drawn: a smart leader's is read off the entity its tip is on
            // (docs/annotation.md, "Smart leaders").
            let codes: Vec<String> = code_property_candidates()
                .iter()
                .map(|&code| String::from(code))
                .collect();
            let mut rows = vec![
                leaf(
                    "Tip",
                    &g.vertices.first().map(plan_point).unwrap_or_default(),
                ),
                leaf("Vertices", &grouped(g.vertices.len() as u64)),
                leaf("Text", &leader_note(model, g, &codes, leaders)),
            ];
            if g.fields {
                rows.push(leaf("Template", &g.text));
                // A preset's template, by the name the Annotate menu gives it.
                if let Some(preset) = recognise_preset(&g.text) {
                    let info = preset_info(preset.preset);
                    let reads = if preset.reads.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", preset.reads)
                    };
                    rows.push(leaf("Preset", &format!("{}{reads}", info.title)));
                }
            }
            if !g.label_style.is_empty() {
                rows.push(leaf("Label style", &g.label_style));
            }
            if g.tip_ref.associated() {
                rows.push(leaf(
                    "On",
                    &format!("{} ({})", g.tip_ref.entity, describe(&g.tip_ref)),
                ));
            }
            rows.push(leaf("Callout", to_string(g.callout)));
            rows
        }
        // The drawing system's kinds; their vertices are edited in the Vertices panel.
        Geometry::CurvePolyline(g) => {
            let mut rows = vec![
                leaf("Vertices", &grouped(g.vertices.len() as u64)),
                leaf("Closed", if g.closed { "yes" } else { "no" }),
                leaf("Arcs", if g.has_arcs() { "yes" } else { "no" }),
                leaf("Heights", if g.has_heights() { "yes" } else { "no" }),
                leaf("Length", &plan_number(g.length())),
            ];
            if g.closed {
                rows.push(leaf("Area", &plan_number(g.area())));
            }
            rows
        }
        Geometry::Ellipse(g) => vec![
            leaf("Centre", &plan_point(&g.center)),
            leaf("Major radius", &plan_number(g.major_radius())),
            leaf("Minor radius", &plan_number(g.minor_radius())),
            leaf("Rotation", &degrees(g.major_axis.angle())),
            leaf("Sweep", &degrees(g.sweep)),
            leaf("Length", &plan_number(g.length())),
        ],
        Geometry::Spline(g) => vec![
            leaf("Degree", &format!("{}", g.degree)),
            leaf("Control points", &grouped(g.control_points.len() as u64)),
            leaf("Fit points", &grouped(g.fit_points.len() as u64)),
            leaf("Length", &plan_number(g.length())),
        ],
    }
}

/// The groups of one entity.
fn entity_rows(document: &Document, entity: &Entity) -> Vec<PropertyRow> {
    let mut rows = Vec::new();

    let mut general = heading("General", "general", "");
    general.open = true;
    let mut general_rows = vec![
        leaf("Id", &format!("{}", entity.id)),
        leaf("Type", to_string(entity.r#type())),
        leaf("Layer", &entity.layer),
        leaf(
            "Colour",
            &entity
                .color
                .map_or_else(|| String::from("ByLayer"), |colour| colour.to_hex()),
        ),
    ];
    if !entity.visible {
        general_rows.push(leaf("Visible", "no"));
    }
    general.children = Some(fixed_reader(general_rows));
    rows.push(general);

    let vertices = vertices_of(&entity.geometry).len();
    let mut geometry = heading("Geometry", "geometry", "");
    geometry.open = true;
    // The surfaces and alignments only for a leader, the one entity that reads them.
    let leaders = if matches!(entity.geometry, Geometry::Leader(_)) {
        leader_context(document)
    } else {
        LeaderContext::default()
    };
    let mut shape_rows = geometry_rows(document.model(), &entity.geometry, &leaders);
    // Their count is the Vertices group's own, beside the list.
    if vertices > 0 {
        shape_rows.retain(|row| row.key != "Vertices");
    }
    geometry.children = Some(fixed_reader(shape_rows));
    rows.push(geometry);

    let subject = Rc::new(Subject {
        ids: vec![entity.id],
        metadata: false,
        vertices_shown: vertices,
    });
    if vertices > 0 {
        let mut list = heading(
            "Vertices",
            "vertices",
            &count_of(vertices, "vertex", "vertices"),
        );
        list.tip = tip_for(
            "Vertices",
            "Each vertex's position and height, with its own attributes beneath it; read a page \
             at a time when opened.",
        );
        let listed = Rc::clone(&subject);
        list.children = Some(Rc::new(move |document: &Document| {
            vertex_level(document, &listed)
        }));
        rows.push(list);
    }

    if !entity.properties.is_empty() {
        let mut attributes = heading(
            "Attributes",
            "attributes",
            &count_of(entity.properties.len(), "value", "values"),
        );
        attributes.open = true;
        let read = Rc::clone(&subject);
        attributes.children = Some(Rc::new(move |document: &Document| {
            outline_level(document, &read, "")
        }));
        rows.push(attributes);
    }

    if !entity.metadata.is_empty() {
        let source = Rc::new(Subject {
            metadata: true,
            ..(*subject).clone()
        });
        let mut record = heading(
            "Source",
            "source",
            &count_of(entity.metadata.len(), "value", "values"),
        );
        record.tip = tip_for(
            "Source",
            "Where the entity came from: what the file it was imported from said about it, kept \
             to be written back.",
        );
        record.children = Some(Rc::new(move |document: &Document| {
            outline_level(document, &source, "")
        }));
        rows.push(record);
    }
    rows
}

/// A reader of a level already made.
fn fixed_reader(rows: Vec<PropertyRow>) -> LevelReader {
    let level = fixed_level(rows);
    Rc::new(move |_: &Document| level.clone())
}

/// Several entities: how many of each type, then what they hold, read as one.
fn selection_rows(document: &Document, ids: &[EntityId]) -> Vec<PropertyRow> {
    // Opened at once only up to here: past it the shared attributes are read when a person asks,
    // since reading them is a pass over every entity.
    const OPEN_ATTRIBUTES_UP_TO: usize = 500;

    let mut rows = Vec::new();
    let mut general = heading("General", "general", "");
    general.open = true;
    let mut general_rows = vec![leaf(
        "Selection",
        &count_of(ids.len(), "entity", "entities"),
    )];
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut any_properties = false;
    for &id in ids {
        if let Some(entity) = document.model().entities.find(id) {
            *by_type
                .entry(String::from(to_string(entity.r#type())))
                .or_default() += 1;
            any_properties = any_properties || !entity.properties.is_empty();
        }
    }
    for (kind, count) in &by_type {
        general_rows.push(leaf(kind, &grouped(*count as u64)));
    }
    general.children = Some(fixed_reader(general_rows));
    rows.push(general);

    if any_properties {
        let subject = Rc::new(Subject {
            ids: ids.to_vec(),
            metadata: false,
            vertices_shown: 0,
        });
        let mut attributes = heading("Attributes", "attributes", "shared, or <varies>");
        attributes.tip = tip_for(
            "Attributes",
            "What the selected entities hold, read as one: a value where they all agree, else \
             <varies>.",
        );
        attributes.open = ids.len() <= OPEN_ATTRIBUTES_UP_TO;
        attributes.children = Some(Rc::new(move |document: &Document| {
            outline_level(document, &subject, "")
        }));
        rows.push(attributes);
    }
    rows
}

/// ASCII case folding, so a search of thirty thousand values folds no letter outside ASCII; a letter
/// outside ASCII is matched as typed.
fn holds(text: &str, folded: &[u8]) -> bool {
    if folded.is_empty() {
        return true;
    }
    lowered(text)
        .windows(folded.len())
        .any(|window| window == folded)
}

/// The top of the tree for what `document` has selected: two rows when nothing is, the groups for
/// one entity, and General (with how many of each type) and the shared Attributes for several.
pub fn selection_level(document: &Document) -> PropertyLevel {
    let ids = document.selection().ids();
    if ids.is_empty() {
        return fixed_level(vec![
            leaf("Selection", "none"),
            leaf("Entities", &format!("{}", document.model().entities.size())),
        ]);
    }
    if ids.len() == 1 {
        if let Some(entity) = document.model().entities.find(ids[0]) {
            return fixed_level(entity_rows(document, entity));
        }
        return fixed_level(vec![leaf("Selection", "an entity that no longer exists")]);
    }
    fixed_level(selection_rows(document, &ids))
}

/// Every property and source value of the selection whose name or value holds `text` (any case),
/// by whole name, in name order.
pub fn matching_level(document: &Document, text: &str) -> PropertyLevel {
    let ids = document.selection().ids();
    if ids.is_empty() {
        return fixed_level(vec![leaf(
            "Filter",
            "select something to look through its properties",
        )]);
    }
    let words = text.trim();
    let folded = lowered(words);

    // Every matching name, with the selection's value for it: shared, or varying when they differ
    // or only some hold it.
    struct Found {
        holders: usize,
        first: Option<PropertyValue>,
        differs: bool,
    }
    let mut found: BTreeMap<(bool, String), Found> = BTreeMap::new();
    let mut searched: usize = 0;
    for &id in &ids {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        for source in [false, true] {
            let map = if source {
                &entity.metadata
            } else {
                &entity.properties
            };
            for (key, value) in map {
                searched += 1;
                if !holds(key, &folded) && !holds(&property_to_string(value), &folded) {
                    continue;
                }
                let entry = found.entry((source, key.clone())).or_insert(Found {
                    holders: 0,
                    first: None,
                    differs: false,
                });
                entry.holders += 1;
                match &entry.first {
                    None => entry.first = Some(value.clone()),
                    Some(first) if first != value => entry.differs = true,
                    Some(_) => {}
                }
            }
        }
    }

    let mut rows = vec![heading(
        "Matches",
        "matches",
        &format!(
            "{} of {} values hold \"{words}\"",
            grouped(found.len() as u64),
            grouped(searched as u64)
        ),
    )];
    let mut ordered: Vec<((bool, String), Found)> = found.into_iter().collect();
    ordered.sort_by(|(a, _), (b, _)| {
        // Properties before the source record, then the names in their natural order.
        a.0.cmp(&b.0).then_with(|| {
            if natural_less(&a.1, &b.1) {
                std::cmp::Ordering::Less
            } else if natural_less(&b.1, &a.1) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
    });
    for ((source, name), entry) in ordered {
        let label = if source {
            format!("Source: {name}")
        } else {
            name
        };
        let varies = entry.differs || entry.holders < ids.len();
        let value = entry
            .first
            .as_ref()
            .map(property_to_string)
            .unwrap_or_default();
        let mut row = leaf(&label, if varies { "<varies>" } else { value.as_str() });
        row.muted = varies;
        row.tip = tip_for(
            &label,
            &if varies {
                format!(
                    "held by {} of the {} selected, not all alike",
                    grouped(entry.holders as u64),
                    grouped(ids.len() as u64)
                )
            } else {
                value.clone()
            },
        );
        rows.push(row);
    }
    fixed_level(rows)
}

// ---- the tree -----------------------------------------------------------------------------------

/// One row as the tree holds it: its level read when it is opened and made a page at a time.
#[derive(Clone, Default)]
struct Node {
    data: PropertyRow,
    is_more: bool,
    level: Option<PropertyLevel>,
    children: Vec<Node>,
    /// The Show more row, while the level has rows not yet made.
    more: Option<Box<Node>>,
    expanded: bool,
}

impl Node {
    fn row_count(&self) -> usize {
        if self.is_more {
            return 0;
        }
        self.children.len() + usize::from(self.more.is_some())
    }

    fn child(&self, index: usize) -> Option<&Node> {
        if index < self.children.len() {
            return self.children.get(index);
        }
        if index == self.children.len() {
            return self.more.as_deref();
        }
        None
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut Node> {
        if index < self.children.len() {
            return self.children.get_mut(index);
        }
        if index == self.children.len() {
            return self.more.as_deref_mut();
        }
        None
    }

    /// The next page of the level, with a Show more row after it while rows are left.
    fn make_page(&mut self, document: &Document) {
        let made = self.children.len();
        let total = self.level.as_ref().map_or(0, |level| level.count);
        let page = PROPERTY_PAGE.min(total - total.min(made));
        self.more = None;
        if let Some(level) = self.level.clone() {
            for index in made..made + page {
                self.children.push(Node {
                    data: level.row(document, index),
                    ..Node::default()
                });
            }
        }
        if self.children.len() < total {
            let now = self.children.len();
            self.more = Some(Box::new(Node {
                data: PropertyRow {
                    label: format!(
                        "Show {} more",
                        grouped(PROPERTY_PAGE.min(total - now) as u64)
                    ),
                    value: format!("{} of {} shown", grouped(now as u64), grouped(total as u64)),
                    tip: String::from(
                        "Show more\nClick to make the next rows. A level is read a page at a \
                         time, so that a string with thousands of vertices opens at once.",
                    ),
                    key: format!("{PATH_SEPARATOR}more"),
                    ..PropertyRow::default()
                },
                is_more: true,
                ..Node::default()
            }));
        }
    }
}

/// The Properties panel (see the module comment).
pub struct PropertyTreePanel {
    root: Node,
    /// Bumped at every refresh, so a Show more clicked before one leaves a level refreshed away
    /// alone.
    generation: u64,
    filter: String,
    /// When the filter's words, typed, are to be searched for.
    filter_due: Option<Instant>,
    /// The drawing shown last, by its address, never read through: a new drawing is a new subject.
    document_key: usize,
    /// What was shown last, and which of its rows were open, by key path; whether the rows are the
    /// tree rather than the filter's list.
    subject: Vec<EntityId>,
    showing_tree: bool,
    opened: BTreeSet<String>,
    /// The groups that open by themselves which a person closed: kept closed from one selection to
    /// the next, as a person expects of a panel.
    closed_groups: BTreeSet<String>,
    /// The rows selected, for Copy.
    selected: Vec<RowPath>,
    /// A level whose Show more row was clicked: made a page longer once the click is over, at the
    /// next frame, since the page takes the clicked row away.
    pending_more: Option<(RowPath, u64)>,
    /// The tree scrolled back to its top at the next frame: a new subject.
    scroll_to_top: bool,
}

impl Default for PropertyTreePanel {
    fn default() -> Self {
        Self::new()
    }
}

impl PropertyTreePanel {
    pub fn new() -> Self {
        Self {
            root: Node::default(),
            generation: 0,
            filter: String::new(),
            filter_due: None,
            document_key: 0,
            subject: Vec::new(),
            showing_tree: false,
            opened: BTreeSet::new(),
            closed_groups: BTreeSet::new(),
            selected: Vec::new(),
            pending_more: None,
            scroll_to_top: false,
        }
    }

    fn node(&self, path: &[usize]) -> Option<&Node> {
        let mut at = &self.root;
        for &index in path {
            at = at.child(index)?;
        }
        Some(at)
    }

    fn node_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut at = &mut self.root;
        for &index in path {
            at = at.child_mut(index)?;
        }
        Some(at)
    }

    /// Shows the selection of `document`. The rows open before keep open where they are still
    /// there.
    pub fn show_selection(&mut self, document: &Document) {
        let key = std::ptr::from_ref(document) as usize;
        if self.document_key != key {
            self.subject.clear();
        }
        self.document_key = key;
        self.show(document);
    }

    /// The selection shown again: the tree as it was left where the subject is the one shown
    /// before, else its groups as they were left; or, with words in the filter, what holds them.
    fn show(&mut self, document: &Document) {
        let ids = document.selection().ids();
        let same_subject = ids == self.subject;
        if self.showing_tree {
            self.remember();
        }
        let text = String::from(self.filter.trim());
        let top = if text.is_empty() {
            selection_level(document)
        } else {
            matching_level(document, &text)
        };
        self.reset(top, document);
        self.showing_tree = text.is_empty();
        if self.showing_tree {
            self.restore(same_subject, document);
            self.scroll_to_top = !same_subject;
        }
        self.subject = ids;
    }

    fn reset(&mut self, top: PropertyLevel, document: &Document) {
        self.generation += 1;
        self.selected.clear();
        self.root = Node {
            level: Some(top),
            ..Node::default()
        };
        self.root.make_page(document);
    }

    /// The key path of the row at `path`.
    fn path_of(&self, path: &[usize]) -> String {
        let mut keys = Vec::new();
        let mut at = &self.root;
        for &index in path {
            let Some(next) = at.child(index) else {
                break;
            };
            keys.push(next.data.key.clone());
            at = next;
        }
        keys.join(PATH_SEPARATOR_TEXT)
    }

    /// Which rows are open, by key path; and the self-opening groups a person closed.
    fn remember(&mut self) {
        self.opened.clear();
        let mut pending: Vec<RowPath> = vec![Vec::new()];
        while let Some(parent) = pending.pop() {
            let count = self.node(&parent).map_or(0, Node::row_count);
            for row in 0..count {
                let mut path = parent.clone();
                path.push(row);
                let Some((open, opens_by_itself, key)) = self
                    .node(&path)
                    .map(|node| (node.expanded, node.data.open, node.data.key.clone()))
                else {
                    continue;
                };
                if parent.is_empty() {
                    if opens_by_itself && !open {
                        self.closed_groups.insert(key);
                    } else if open {
                        self.closed_groups.remove(&key);
                    }
                }
                if open {
                    let opened = self.path_of(&path);
                    self.opened.insert(opened);
                    pending.push(path);
                }
            }
        }
    }

    /// The rows that were open, opened again where they still are; for a new subject only its
    /// groups, as they were left.
    fn restore(&mut self, same_subject: bool, document: &Document) {
        for row in 0..self.root.row_count() {
            let Some(node) = self.root.child(row) else {
                continue;
            };
            let key = node.data.key.clone();
            let by_default = node.data.open;
            let reopen = if by_default {
                !self.closed_groups.contains(&key)
            } else {
                self.opened.contains(&key)
            };
            if reopen {
                self.expand(&[row], document);
            }
        }
        if !same_subject {
            return;
        }
        let mut deeper: Vec<String> = self
            .opened
            .iter()
            .filter(|path| path.contains(PATH_SEPARATOR))
            .cloned()
            .collect();
        // Parents before children.
        deeper.sort_by_key(|path| path.matches(PATH_SEPARATOR).count());
        for keys in deeper {
            let mut at: RowPath = Vec::new();
            let mut found = true;
            for key in keys.split(PATH_SEPARATOR) {
                if !at.is_empty() && self.can_fetch_more(&at) && self.row_count(&at) == 0 {
                    self.fetch_more(&at, document);
                }
                let count = self.row_count(&at);
                let next = (0..count).find(|&row| {
                    let mut child = at.clone();
                    child.push(row);
                    self.node(&child).is_some_and(|node| node.data.key == key)
                });
                match next {
                    Some(row) => at.push(row),
                    None => {
                        found = false;
                        break;
                    }
                }
            }
            if found {
                self.expand(&at, document);
            }
        }
    }

    // ---- the model, as the C++'s QAbstractItemModel answered ------------------------------------

    /// How many rows are made beneath `path`: its children and its Show more row.
    pub fn row_count(&self, path: &[usize]) -> usize {
        self.node(path).map_or(0, Node::row_count)
    }

    /// An expander before the level is read: that is what lets a row with thirty thousand values
    /// beneath it cost nothing until it is opened.
    pub fn has_children(&self, path: &[usize]) -> bool {
        let Some(node) = self.node(path) else {
            return false;
        };
        if node.is_more {
            return false;
        }
        if path.is_empty() {
            return node.row_count() > 0;
        }
        match &node.level {
            Some(level) => level.count > 0,
            None => node.data.children.is_some(),
        }
    }

    /// Only a level not yet read.
    pub fn can_fetch_more(&self, path: &[usize]) -> bool {
        self.node(path).is_some_and(|node| {
            !node.is_more && node.level.is_none() && node.data.children.is_some()
        })
    }

    /// Reads the level and makes its first page.
    pub fn fetch_more(&mut self, path: &[usize], document: &Document) {
        if !self.can_fetch_more(path) {
            return;
        }
        if let Some(node) = self.node_mut(path) {
            node.level = node.data.children.as_ref().map(|read| read(document));
            node.make_page(document);
        }
    }

    /// The next page of a level already read, where its Show more row was.
    pub fn show_more(&mut self, path: &[usize], document: &Document) -> bool {
        let Some(node) = self.node_mut(path) else {
            return false;
        };
        let total = match &node.level {
            Some(level) if !node.is_more => level.count,
            _ => return false,
        };
        if node.children.len() >= total {
            return false;
        }
        node.make_page(document);
        true
    }

    /// What the row at `path` says in its first column; empty for no row.
    pub fn label(&self, path: &[usize]) -> String {
        self.node(path)
            .map(|node| node.data.label.clone())
            .unwrap_or_default()
    }

    /// What the row at `path` says in its second column.
    pub fn value(&self, path: &[usize]) -> String {
        self.node(path)
            .map(|node| node.data.value.clone())
            .unwrap_or_default()
    }

    /// Whether the row at `path` is a level's Show more row.
    pub fn is_more(&self, path: &[usize]) -> bool {
        self.node(path).is_some_and(|node| node.is_more)
    }

    /// The row beneath `parent` whose first column says `label`.
    pub fn find_row(&self, label: &str, parent: &[usize]) -> Option<RowPath> {
        let node = self.node(parent)?;
        (0..node.row_count()).find_map(|row| {
            node.child(row)
                .filter(|child| child.data.label == label)
                .map(|_| {
                    let mut path = parent.to_vec();
                    path.push(row);
                    path
                })
        })
    }

    // ---- what a person does ---------------------------------------------------------------------

    pub fn is_expanded(&self, path: &[usize]) -> bool {
        !path.is_empty() && self.node(path).is_some_and(|node| node.expanded)
    }

    /// Opens a row as a click on its expander does: its level read the first time, and only its
    /// first page made.
    pub fn expand(&mut self, path: &[usize], document: &Document) {
        if self.can_fetch_more(path) && self.row_count(path) == 0 {
            self.fetch_more(path, document);
        }
        if !self.has_children(path) {
            return;
        }
        if let Some(node) = self.node_mut(path) {
            node.expanded = true;
        }
    }

    pub fn collapse(&mut self, path: &[usize]) {
        if let Some(node) = self.node_mut(path) {
            node.expanded = false;
        }
    }

    /// A click (or Enter) on a row: a Show more row makes the next page of its level where it is,
    /// once the click is over - at the next frame (`process_pending`).
    pub fn click(&mut self, path: &[usize]) {
        if !self.is_more(path) {
            return;
        }
        let level = path[..path.len() - 1].to_vec();
        self.pending_more = Some((level, self.generation));
    }

    /// What the next frame does first: a Show more clicked, made - unless its level was refreshed
    /// away meanwhile, when it is left alone (the top level is never refreshed away).
    pub fn process_pending(&mut self, document: &Document) {
        if let Some((level, generation)) = self.pending_more.take()
            && (level.is_empty() || generation == self.generation)
        {
            self.show_more(&level, document);
        }
    }

    /// Selects the rows at `paths`, as a click (with Ctrl, several) does.
    pub fn select(&mut self, paths: Vec<RowPath>) {
        self.selected = paths;
    }

    /// What Copy copies: the selected rows, name and value separated by a tab, one a line; a Show
    /// more row is not one. None when nothing is selected.
    pub fn copy_text(&self) -> Option<String> {
        let mut paths = self.selected.clone();
        paths.sort();
        let lines: Vec<String> = paths
            .iter()
            .filter_map(|path| self.node(path))
            .filter(|node| !node.is_more)
            .map(|node| {
                if node.data.value.is_empty() {
                    node.data.label.clone()
                } else {
                    format!("{}\t{}", node.data.label, node.data.value)
                }
            })
            .collect();
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// The Copy action (propertyCopy): the selected rows on the clipboard.
    pub fn copy(&self, clipboard: &mut dyn Clipboard) {
        if let Some(text) = self.copy_text() {
            let _ = clipboard.set_text(&text);
        }
    }

    /// What the filter holds.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The filter typed at `now`: cleared, the tree back at once, as it was left; otherwise its
    /// words searched for after a pause (`refresh_due`).
    pub fn set_filter(&mut self, text: &str, document: &Document, now: Instant) {
        self.filter = String::from(text);
        if text.trim().is_empty() {
            self.filter_due = None;
            self.show(document);
        } else {
            self.filter_due = Some(now + FILTER_DELAY);
        }
    }

    /// The filter's words searched for when the pause after the typing is over at `now`. True
    /// when they were.
    pub fn refresh_due(&mut self, document: &Document, now: Instant) -> bool {
        match self.filter_due {
            Some(due) if now >= due => {
                self.filter_due = None;
                self.show(document);
                true
            }
            _ => false,
        }
    }

    /// The rows shown, depth first down every open row, with their depth: what a person reads.
    fn visible_rows(&self) -> Vec<(RowPath, usize)> {
        let mut rows = Vec::new();
        let mut pending: Vec<RowPath> = (0..self.root.row_count())
            .rev()
            .map(|row| vec![row])
            .collect();
        while let Some(path) = pending.pop() {
            let Some(node) = self.node(&path) else {
                continue;
            };
            if node.expanded {
                for row in (0..node.row_count()).rev() {
                    let mut child = path.clone();
                    child.push(row);
                    pending.push(child);
                }
            }
            let depth = path.len() - 1;
            rows.push((path, depth));
        }
        rows
    }

    /// What --report propertyTable says: every row shown, "label | value", joined by " ; ".
    pub fn report(&self) -> String {
        self.report_rows().join(" ; ")
    }

    fn report_rows(&self) -> Vec<String> {
        self.visible_rows()
            .iter()
            .filter_map(|(path, _)| self.node(path))
            .map(|node| format!("{} | {}", node.data.label, node.data.value))
            .collect()
    }

    /// The panel drawn over `document`: the filter, then the tree. Copy (its context menu, and
    /// Ctrl+C) puts the selected rows on `clipboard`.
    pub fn ui(&mut self, ui: &mut Ui, document: &Document, clipboard: &mut dyn Clipboard) {
        let now = Instant::now();
        self.process_pending(document);
        self.refresh_due(document, now);
        let response = line_with(
            ui,
            "propertyFilter",
            &mut self.filter,
            LineOptions {
                placeholder: "Filter properties and values...",
                ..LineOptions::default()
            },
        )
        .on_hover_text(
            "Filter\nList every property of the selection whose name or value holds these words, \
             however deep in the tree - a vertex's attributes too.",
        );
        if response.changed() {
            let text = self.filter.clone();
            self.set_filter(&text, document, now);
        }
        if let Some(due) = self.filter_due {
            ui.ctx()
                .request_repaint_after(due.saturating_duration_since(now));
        }

        enum Act {
            Toggle(RowPath),
            Click(RowPath, bool),
        }
        let mut acts = Vec::new();
        let mut copy = false;
        let rows = self.visible_rows();
        let accent = palette::accent().to_egui();
        let muted = palette::text_muted().to_egui();
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("propertyTableScroll")
            .auto_shrink([false, false]);
        if std::mem::take(&mut self.scroll_to_top) {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        let shown = scroll.show(ui, |ui| {
            for (path, depth) in &rows {
                let Some(node) = self.node(path) else {
                    continue;
                };
                let expander = self.has_children(path);
                let selected = self.selected.contains(path);
                ui.horizontal(|ui| {
                    ui.add_space(14.0 * *depth as f32);
                    if expander {
                        let arrow = if node.expanded {
                            "\u{25be}"
                        } else {
                            "\u{25b8}"
                        };
                        if ui.small_button(arrow).clicked() {
                            acts.push(Act::Toggle(path.clone()));
                        }
                    } else {
                        ui.add_space(18.0);
                    }
                    let mut label = RichText::new(node.data.label.as_str());
                    if node.data.heading {
                        label = label.strong();
                    }
                    if node.is_more {
                        label = label.italics().color(accent);
                    }
                    let mut name = ui.selectable_label(selected, label);
                    if !node.data.tip.is_empty() {
                        name = name.on_hover_text(node.data.tip.as_str());
                    }
                    if name.clicked() {
                        let extend = ui.input(|input| input.modifiers.command);
                        acts.push(Act::Click(path.clone(), extend));
                    }
                    // The tree's context menu (Qt::ActionsContextMenu): Copy.
                    name.context_menu(|ui| {
                        let action = Action::new("propertyCopy", "Copy")
                            .tip("Copy the selected rows, name and value separated by a tab");
                        if action_item(ui, &action).clicked() {
                            copy = true;
                            ui.close();
                        }
                    });
                    let mut value = RichText::new(node.data.value.as_str());
                    if node.data.muted || node.is_more {
                        value = value.color(if node.is_more { accent } else { muted });
                    }
                    ui.label(value);
                });
            }
        });
        let table = ui.interact(
            shown.inner_rect,
            shown.id.with("rows"),
            egui::Sense::hover(),
        );
        let mut record = ControlRecord::new("propertyTable", ControlKind::Rows);
        record.items = self.report_rows();
        custom(ui, &table, record);
        if ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        }) && !self.selected.is_empty()
        {
            copy = true;
        }
        for act in acts {
            match act {
                Act::Toggle(path) => {
                    if self.is_expanded(&path) {
                        self.collapse(&path);
                    } else {
                        self.expand(&path, document);
                    }
                }
                Act::Click(path, extend) => {
                    if self.is_more(&path) {
                        self.click(&path);
                    } else if extend {
                        if !self.selected.contains(&path) {
                            self.selected.push(path);
                        }
                    } else {
                        self.selected = vec![path];
                    }
                }
            }
        }
        if copy {
            self.copy(clipboard);
        }
    }
}
