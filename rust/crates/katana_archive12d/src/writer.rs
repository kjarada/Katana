//! Archive -> .12da text.
//!
//! The writer produces the forms the manual documents for the CURRENT element
//! types: every list of vertices is written as a `string super`, every
//! alignment as a `string super_alignment`. The superseded types (2d, 3d, 4d,
//! pipe, polyline, alignment, pipeline) are read but never written, which is
//! what the format's own program does.
//!
//! What is read is written back: `read_archive(write_archive(a))` equals `a`
//! for any archive made of the documented elements, with the superseded kinds
//! arriving as the super strings they were written as. That property is
//! tested, and it is what "every element accounted for" means on the way out.

use std::cmp::Ordering;

use katana_core::text::format_exact_real;

use crate::archive::{
    AlignmentPart, ArcString, Archive, AttributeList, AttributeValue, Breakline, CircleString,
    DraftingLeader, DraftingReference, DrainageRecord, DrainageString, Element, Field, FieldList,
    LasCloud, ModelRecord, PlotFrame, Segment, SegmentKind, SolvedGeometry, StringHeader,
    StringKind, SuperAlignment, SuperTin, TextString, Tin, Trimesh, TrimeshInfo, Vertex,
    VertexString,
};
use crate::reader::parse_real;
use crate::text_utilities::{equals_ignoring_case, format_hex_real, format_real};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOptions {
    /// Digits after the decimal point for coordinates and other reals. The
    /// writing program's own default is 8 (`// decimal_places 8`). Trailing
    /// zeros are trimmed, so 8 costs nothing for round numbers.
    pub decimal_places: i32,
    /// Write tin points as C99 hexadecimal floats, as the writing program does
    /// by default (`output_tin_hex_floats true`): a double written in hex
    /// reads back bit for bit, which decimal at 8 places does not.
    ///
    /// ON by default, because a tin's coordinates are not numbers a user
    /// typed: they are computed, and the SIGN OF A TRIANGLE'S AREA depends on
    /// their last bits. Eight places moves a point by up to 5e-9, which is enough to
    /// turn a sliver triangle inside out; a surface with one inside-out
    /// triangle is refused whole. Measured on plot_PW_example_data.12da: four
    /// of its eight surfaces would not read back.
    pub hex_float_tins: bool,
    /// Comment written at the head of the file, one line per line of text.
    pub banner: String,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            decimal_places: 8,
            hex_float_tins: true,
            banner: String::from("Written by Katana"),
        }
    }
}

/// The text is UTF-8. It never fails: an Archive cannot hold anything the
/// format has no way to say.
pub fn write_archive(archive: &Archive, options: &WriteOptions) -> String {
    Writer::new(options.clone(), &archive.models).run(archive)
}

/// A name, colour or text value as it must appear in the file: in double
/// quotes, with `"` and `\` escaped. Exposed because the rule is easy to get
/// subtly wrong and is tested on its own.
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        if character == '"' || character == '\\' {
            out.push('\\');
        }
        out.push(character);
    }
    out.push('"');
    out
}

// Flags of a nulling block, and values of the other long lists, per line. 16
// is what the writing program writes; it keeps a quarter-million flags to
// lines an editor will open.
const VALUES_PER_LINE: usize = 16;

// std::isalnum in the C locale, which is ASCII, or an underscore.
fn is_plain_word(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

// A name or a word as the manual requires it: bare when it is purely
// alphanumeric, in quotes otherwise.
fn word(text: &str) -> String {
    if is_plain_word(text) {
        String::from(text)
    } else {
        quoted(text)
    }
}

fn flag(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn truth(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

// The model an element is in; empty for tins and super tins, which belong to
// the project rather than to a model.
fn model_of(element: &Element) -> &str {
    match element {
        Element::VertexString(string) => &string.header.model,
        Element::ArcString(arc) => &arc.header.model,
        Element::CircleString(circle) => &circle.header.model,
        Element::TextString(text) => &text.header.model,
        Element::PlotFrame(frame) => &frame.header.model,
        Element::DrainageString(drainage) => &drainage.header.model,
        Element::SuperAlignment(alignment) => &alignment.header.model,
        Element::LasCloud(cloud) => &cloud.header.model,
        Element::DraftingLeader(leader) => &leader.header.model,
        Element::Trimesh(mesh) => &mesh.model,
        Element::Tin(_) | Element::SuperTin(_) => "",
    }
}

// The null value the file is written with. It must equal no real height in
// the archive: a height read under one null value and written under another
// would otherwise come back as null - silently, from a plain read and write.
// The archive's own value is kept when it is safe; failing that, sentinels
// far below any elevation on Earth (the Mariana Trench is -11 km) are tried.
fn safe_null_value(archive: &Archive) -> f64 {
    fn collect_all(heights: &mut Vec<f64>, vertices: &[Vertex]) {
        heights.extend(vertices.iter().filter_map(|vertex| vertex.z));
    }
    fn collect_one(heights: &mut Vec<f64>, z: Option<f64>) {
        if let Some(z) = z {
            heights.push(z);
        }
    }
    let mut heights = Vec::new();
    for element in &archive.elements {
        match element {
            Element::VertexString(string) => {
                collect_all(&mut heights, &string.vertices);
                collect_one(&mut heights, string.constant_z);
            }
            Element::ArcString(arc) => {
                collect_one(&mut heights, arc.centre.z);
                collect_one(&mut heights, arc.start.z);
                collect_one(&mut heights, arc.end.z);
            }
            Element::CircleString(circle) => collect_one(&mut heights, circle.centre.z),
            Element::TextString(text) => collect_one(&mut heights, text.position.z),
            Element::DraftingLeader(leader) => {
                collect_one(&mut heights, leader.arrow.z);
                collect_one(&mut heights, leader.hook.z);
            }
            Element::DrainageString(drainage) => {
                collect_all(&mut heights, &drainage.vertices);
                for control in &drainage.property_controls {
                    collect_all(&mut heights, &control.vertices);
                }
            }
            Element::Tin(tin) => collect_all(&mut heights, &tin.points),
            Element::Trimesh(mesh) => collect_all(&mut heights, &mesh.vertices),
            Element::PlotFrame(_)
            | Element::SuperAlignment(_)
            | Element::LasCloud(_)
            | Element::SuperTin(_) => {}
        }
    }
    // std::sort and std::binary_search by operator<: -0 and +0 are one height,
    // as they are to the reader's comparison with the null value.
    let order = |a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(Ordering::Equal);
    heights.sort_by(order);
    let collides = |candidate: f64| {
        heights
            .binary_search_by(|height| order(height, &candidate))
            .is_ok()
    };
    if !collides(archive.null_value) {
        return archive.null_value;
    }
    for candidate in [-999.0, -9999.0, -99999.0, -999999.0, -9999999.0] {
        if !collides(candidate) {
            return candidate;
        }
    }
    // Every sentinel is a real height somewhere: take the first free integer
    // below the lowest of them, which no survey reaches.
    let mut candidate = heights[0].floor() - 1.0;
    while collides(candidate) {
        candidate -= 1.0;
    }
    candidate
}

struct Writer<'a> {
    options: WriteOptions,
    out: String,
    indent: usize,
    null_value: f64,
    current_model: String,
    models: &'a [ModelRecord],
    declared_models: Vec<String>,
}

impl<'a> Writer<'a> {
    fn new(options: WriteOptions, models: &'a [ModelRecord]) -> Self {
        Self {
            options,
            out: String::new(),
            indent: 0,
            null_value: -999.0,
            current_model: String::new(),
            models,
            declared_models: Vec::new(),
        }
    }

    // ---- text ------------------------------------------------------------------

    fn line(&mut self, text: &str) {
        self.out.extend(std::iter::repeat_n(' ', self.indent * 2));
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn open(&mut self, keyword: &str) {
        self.line(&format!("{keyword} {{"));
        self.indent += 1;
    }

    fn close(&mut self) {
        self.indent -= 1;
        self.line("}");
    }

    fn real(&self, value: f64) -> String {
        format_real(value, self.options.decimal_places)
    }

    fn height(&self, z: Option<f64>) -> String {
        self.real(z.unwrap_or(self.null_value))
    }

    fn pair(&mut self, key: &str, value: &str) {
        let mut text = String::from(key);
        // Values line up in a column, as the writing program's do; it is only
        // legibility, but these files are read by people.
        while text.len() < 9 {
            text.push(' ');
        }
        text.push(' ');
        text.push_str(value);
        self.line(&text);
    }

    fn field(&mut self, item: &Field) {
        if item.value.is_empty() && !item.quoted {
            self.line(&item.key); // a keyword that had no value when it was read
        } else if item.quoted || !(is_plain_word(&item.value) || parse_real(&item.value).is_some())
        {
            self.pair(&item.key, &quoted(&item.value));
        } else {
            self.pair(&item.key, &item.value);
        }
    }

    fn fields(&mut self, list: &FieldList) {
        for item in list.fields() {
            // A string's own null value, read into its extras so that its
            // heights could be told from its nulls, must NOT be written: every
            // null in this file is written with the ONE value at its head.
            if item.key == "null_value" {
                continue;
            }
            self.field(item);
        }
    }

    fn field_block(&mut self, keyword: &str, list: &FieldList) {
        self.open(keyword);
        self.fields(list);
        self.close();
    }

    fn attributes(&mut self, list: &AttributeList) {
        if list.is_empty() {
            return;
        }
        self.open("attributes");
        self.attribute_body(list);
        self.close();
    }

    fn attribute_body(&mut self, list: &AttributeList) {
        for attribute in list {
            match &attribute.value {
                AttributeValue::Group(group) => {
                    self.open("group");
                    self.pair("name", &quoted(&attribute.name));
                    self.open("attributes"); // written even when empty: a group is its attributes
                    self.attribute_body(group);
                    self.close();
                    self.close();
                }
                AttributeValue::Integer(integer) => {
                    let kind = if attribute.declared_type.is_empty() {
                        "integer"
                    } else {
                        attribute.declared_type.as_str()
                    };
                    self.line(&format!("{kind} {} {integer}", quoted(&attribute.name)));
                }
                AttributeValue::Real(number) => {
                    self.line(&format!(
                        "real {} {}",
                        quoted(&attribute.name),
                        format_exact_real(*number)
                    ));
                }
                AttributeValue::Text(text) => {
                    let kind = if attribute.declared_type.is_empty() {
                        "text"
                    } else {
                        attribute.declared_type.as_str()
                    };
                    self.line(&format!(
                        "{kind} {} {}",
                        quoted(&attribute.name),
                        quoted(text)
                    ));
                }
            }
        }
    }

    // A long list of short values, several to a line.
    fn value_block(&mut self, keyword: &str, values: impl IntoIterator<Item = String>) {
        self.open(keyword);
        let mut text = String::new();
        let mut on_line = 0;
        for value in values {
            if on_line != 0 {
                text.push(' ');
            }
            text.push_str(&value);
            on_line += 1;
            if on_line == VALUES_PER_LINE {
                self.line(&text);
                text.clear();
                on_line = 0;
            }
        }
        if on_line != 0 {
            self.line(&text);
        }
        self.close();
    }

    fn boolean_block(&mut self, keyword: &str, values: &[bool]) {
        if !values.is_empty() {
            self.value_block(
                keyword,
                values.iter().map(|&value| String::from(flag(value))),
            );
        }
    }

    fn text_block(&mut self, keyword: &str, values: &[String]) {
        if !values.is_empty() {
            self.value_block(keyword, values.iter().map(|value| quoted(value)));
        }
    }

    fn properties_list(&mut self, keyword: &str, records: &[FieldList]) {
        if records.is_empty() {
            return;
        }
        self.open(keyword);
        for record in records {
            self.field_block("properties", record);
        }
        self.close();
    }

    fn attributes_list(&mut self, keyword: &str, records: &[AttributeList]) {
        if records.is_empty() {
            return;
        }
        self.open(keyword);
        for record in records {
            self.open("attributes"); // one per vertex even when empty: position is identity
            self.attribute_body(record);
            self.close();
        }
        self.close();
    }

    // ---- shared pieces of strings --------------------------------------------------

    fn model(&mut self, name: &str) {
        if name == self.current_model {
            return;
        }
        self.current_model = String::from(name);
        let models = self.models;
        let record = models
            .iter()
            .find(|candidate| equals_ignoring_case(&candidate.name, name));
        let already_declared = self.declared_models.iter().any(|declared| declared == name);
        if let Some(record) = record
            && !already_declared
        {
            // The block form is the only one that can carry attributes
            // (manual 1.4.1b); once is enough.
            self.declared_models.push(String::from(name));
            self.out.push('\n');
            self.open("model");
            self.pair("name", &quoted(name));
            self.fields(&record.extras);
            self.attributes(&record.attributes);
            self.close();
            return;
        }
        self.out.push('\n');
        self.line(&format!("model {}", quoted(name)));
    }

    fn header(&mut self, head: &StringHeader) {
        self.pair("name", &quoted(&head.name));
        let chainage = self.real(head.chainage);
        self.pair("chainage", &chainage);
        self.pair(
            "breakline",
            if head.breakline == Breakline::Point {
                "point"
            } else {
                "line"
            },
        );
        self.pair("colour", &word(&head.colour));
        self.pair("style", &quoted(&head.style));
        self.fields(&head.extras);
        self.attributes(&head.attributes);
    }

    fn vertex_rows(&mut self, keyword: &str, vertices: &[Vertex], with_z: bool) {
        self.open(keyword);
        for vertex in vertices {
            let mut text = format!("{} {}", self.real(vertex.x), self.real(vertex.y));
            if with_z {
                text.push(' ');
                text.push_str(&self.height(vertex.z));
            }
            self.line(&text);
        }
        self.close();
    }

    // x y z radius bulge: the geometry of the segment leaving each vertex.
    fn polyline_rows(&mut self, vertices: &[Vertex], segments: &[Segment]) {
        self.open("data");
        for (i, vertex) in vertices.iter().enumerate() {
            let arc = segments
                .get(i)
                .filter(|segment| segment.kind == SegmentKind::Arc);
            let text = format!(
                "{} {} {} {} {}",
                self.real(vertex.x),
                self.real(vertex.y),
                self.height(vertex.z),
                arc.map_or_else(|| String::from("0"), |segment| self.real(segment.radius)),
                if arc.is_some_and(|segment| segment.major) {
                    "1"
                } else {
                    "0"
                }
            );
            self.line(&text);
        }
        self.close();
    }

    // radius_data and major_data when every curved segment is an arc, which is
    // the compact form; geometry_data as soon as one is a transition.
    fn segment_geometry(&mut self, segments: &[Segment], always_geometry_data: bool) {
        if segments.is_empty() {
            return;
        }
        let only_arcs = segments.iter().all(|segment| {
            segment.kind == SegmentKind::Straight || segment.kind == SegmentKind::Arc
        });
        if only_arcs && !always_geometry_data {
            let places = self.options.decimal_places;
            self.value_block(
                "radius_data",
                segments.iter().map(|segment| {
                    if segment.kind == SegmentKind::Arc {
                        format_real(segment.radius, places)
                    } else {
                        String::from("0")
                    }
                }),
            );
            self.value_block(
                "major_data",
                segments.iter().map(|segment| {
                    String::from(flag(segment.kind == SegmentKind::Arc && segment.major))
                }),
            );
            return;
        }
        self.open("geometry_data");
        for segment in segments {
            match segment.kind {
                SegmentKind::Straight => self.line("straight { }"),
                SegmentKind::Arc => {
                    let text = format!(
                        "arc {{ radius {} major {} }}",
                        self.real(segment.radius),
                        flag(segment.major)
                    );
                    self.line(&text);
                }
                SegmentKind::Spiral => self.field_block("spiral", &segment.parameters),
                SegmentKind::Curve => self.field_block("curve", &segment.parameters),
                SegmentKind::Parabola => self.field_block("parabola", &segment.parameters),
            }
        }
        self.close();
    }

    // ---- elements --------------------------------------------------------------------

    fn write_vertex_string(&mut self, string: &VertexString) {
        self.model(&string.header.model);
        if string.kind == StringKind::Face || string.kind == StringKind::Interface {
            self.write_simple_vertex_string(string);
            return;
        }
        self.open("string super");
        self.header(&string.header);
        self.pair("closed", truth(string.closed));

        let any_height = string.vertices.iter().any(|vertex| vertex.z.is_some());
        if any_height {
            self.vertex_rows("data_3d", &string.vertices, true);
        } else {
            if let Some(z) = string.constant_z {
                let text = self.real(z);
                self.pair("z", &text);
            }
            self.vertex_rows("data_2d", &string.vertices, false);
        }
        self.segment_geometry(&string.segments, false);

        if let Some(interval) = &string.interval {
            self.field_block("interval", interval);
        }
        self.text_block("colour_data", &string.segment_colours);
        self.text_block("point_data", &string.point_ids);

        if let Some(diameter) = string.diameter {
            let text = self.real(diameter);
            self.pair("diameter_value", &text);
        }
        if !string.diameters.is_empty() {
            let places = self.options.decimal_places;
            self.value_block(
                "diameter_data",
                string
                    .diameters
                    .iter()
                    .map(|&value| format_real(value, places)),
            );
        }
        if let Some(culvert) = string.culvert {
            self.open("culvert_value");
            let width = self.real(culvert[0]);
            self.pair("width", &width);
            let height = self.real(culvert[1]);
            self.pair("height", &height);
            self.close();
        }
        if !string.culverts.is_empty() {
            self.open("culvert_data");
            for culvert in &string.culverts {
                let text = format!(
                    "properties {{ width {} height {} }}",
                    self.real(culvert[0]),
                    self.real(culvert[1])
                );
                self.line(&text);
            }
            self.close();
        }
        if !string.justify.is_empty() {
            self.pair("justify", &word(&string.justify));
        }

        self.flag_value("vertex_tinable_value", string.vertex_tinable_value);
        self.boolean_block("vertex_tinable_data", &string.vertex_tinable);
        self.flag_value("segment_tinable_value", string.segment_tinable_value);
        self.boolean_block("segment_tinable_data", &string.segment_tinable);
        self.flag_value("vertex_visible_value", string.vertex_visible_value);
        self.boolean_block("vertex_visible_data", &string.vertex_visible);
        self.flag_value("segment_visible_value", string.segment_visible_value);
        self.boolean_block("segment_visible_data", &string.segment_visible);

        if let Some(value) = &string.vertex_text_value {
            self.pair("vertex_text_value", &quoted(value));
        }
        self.text_block("vertex_text_data", &string.vertex_text);
        if let Some(annotation) = &string.vertex_annotation {
            self.field_block("vertex_annotate_value", annotation);
        }
        self.properties_list("vertex_annotate_data", &string.vertex_annotations);
        if let Some(value) = &string.segment_text_value {
            self.pair("segment_text_value", &quoted(value));
        }
        self.text_block("segment_text_data", &string.segment_text);
        if let Some(annotation) = &string.segment_annotation {
            self.field_block("segment_annotate_value", annotation);
        }
        self.properties_list("segment_annotate_data", &string.segment_annotations);

        if let Some(symbol) = &string.symbol {
            self.field_block("symbol_value", symbol);
        }
        self.properties_list("symbol_data", &string.symbols);
        self.attributes_list("vertex_attribute_data", &string.vertex_attributes);
        self.attributes_list("segment_attribute_data", &string.segment_attributes);
        self.close();
    }

    fn flag_value(&mut self, key: &str, value: Option<bool>) {
        if let Some(value) = value {
            self.pair(key, flag(value));
        }
    }

    // Face and interface strings are current types with a `data` block of
    // their own; they are not super strings and are not written as one.
    fn write_simple_vertex_string(&mut self, string: &VertexString) {
        let with_modes = string.kind == StringKind::Interface;
        self.open(if with_modes {
            "string interface"
        } else {
            "string face"
        });
        self.header(&string.header);
        self.open("data");
        for (i, vertex) in string.vertices.iter().enumerate() {
            let mut text = format!(
                "{} {} {}",
                self.real(vertex.x),
                self.real(vertex.y),
                self.height(vertex.z)
            );
            if with_modes {
                let mode = string.interface_modes.get(i).copied().unwrap_or(0);
                text.push_str(&format!(" {mode}"));
            }
            self.line(&text);
        }
        self.close();
        self.close();
    }

    fn vertex_fields(&mut self, vertex: &Vertex, x: &str, y: &str, z: &str) {
        let text = self.real(vertex.x);
        self.pair(x, &text);
        let text = self.real(vertex.y);
        self.pair(y, &text);
        let text = self.height(vertex.z);
        self.pair(z, &text);
    }

    fn write_arc(&mut self, arc: &ArcString) {
        self.model(&arc.header.model);
        self.open("string arc");
        self.header(&arc.header);
        let radius = self.real(arc.radius);
        self.pair("radius", &radius);
        self.vertex_fields(&arc.centre, "xcentre", "ycentre", "zcentre");
        self.vertex_fields(&arc.start, "xstart", "ystart", "zstart");
        self.vertex_fields(&arc.end, "xend", "yend", "zend");
        self.close();
    }

    fn write_circle(&mut self, circle: &CircleString) {
        self.model(&circle.header.model);
        self.open(if circle.feature {
            "string feature"
        } else {
            "string circle"
        });
        self.header(&circle.header);
        let radius = self.real(circle.radius);
        self.pair("radius", &radius);
        self.vertex_fields(&circle.centre, "xcentre", "ycentre", "zcentre");
        self.close();
    }

    fn write_text(&mut self, text: &TextString) {
        self.model(&text.header.model);
        self.open("string text");
        self.header(&text.header);
        self.fields(&text.annotation);
        self.vertex_fields(&text.position, "x", "y", "z");
        self.pair("text", &quoted(&text.text));
        self.close();
    }

    fn write_plot_frame(&mut self, frame: &PlotFrame) {
        self.model(&frame.header.model);
        self.open("string plot_frame");
        self.header(&frame.header);
        self.fields(&frame.fields);
        self.close();
    }

    fn drafting_reference(&mut self, reference: &DraftingReference) {
        self.open(&reference.name);
        self.fields(&reference.fields);
        for block in &reference.blocks {
            self.drafting_reference(block);
        }
        self.close();
    }

    fn write_leader(&mut self, leader: &DraftingLeader) {
        self.model(&leader.header.model);
        self.open("drafting_element");
        self.header(&leader.header);
        self.open("leader");
        self.fields(&leader.fields);
        self.vertex_fields(&leader.arrow, "arrow_x", "arrow_y", "arrow_z");
        self.vertex_fields(&leader.hook, "hook_x", "hook_y", "hook_z");
        if let Some(reference) = &leader.arrow_reference {
            self.drafting_reference(reference);
        }
        if !leader.overrides.fields().is_empty() {
            self.open("overrides");
            self.fields(&leader.overrides);
            self.close();
        }
        self.close();
        self.close();
    }

    fn drainage_record(&mut self, keyword: &str, record: &DrainageRecord) {
        self.open(keyword);
        self.fields(&record.fields);
        self.attributes(&record.attributes);
        if !record.vertices.is_empty() {
            self.polyline_rows(&record.vertices, &record.segments);
        }
        self.close();
    }

    fn write_drainage(&mut self, drainage: &DrainageString) {
        self.model(&drainage.header.model);
        self.open("string drainage");
        self.header(&drainage.header);
        if let Some(outfall) = drainage.outfall {
            let text = self.real(outfall);
            self.pair("outfall", &text);
        }
        if let Some(direction) = drainage.flow_direction {
            self.pair("flow_direction", &format!("{direction}"));
        }
        self.polyline_rows(&drainage.vertices, &drainage.segments);
        for pit in &drainage.pits {
            self.drainage_record(
                if drainage.pits_are_version2 {
                    "pit_v2"
                } else {
                    "pit"
                },
                pit,
            );
        }
        for pipe in &drainage.pipes {
            self.drainage_record("pipe", pipe);
        }
        for control in &drainage.property_controls {
            self.drainage_record("property_control", control);
        }
        for connection in &drainage.house_connections {
            self.drainage_record("house_connection", connection);
        }
        self.close();
    }

    fn parts(&mut self, keyword: &str, list: &[AlignmentPart]) {
        if list.is_empty() {
            return;
        }
        self.open(keyword);
        for part in list {
            self.open(&part.kind);
            self.fields(&part.fields);
            self.attributes(&part.attributes);
            self.close();
        }
        self.close();
    }

    fn solved_geometry(&mut self, keyword: &str, geometry: &SolvedGeometry) {
        self.open(keyword);
        self.fields(&geometry.header);
        self.pair("closed", truth(geometry.closed));
        if let Some(interval) = &geometry.interval {
            self.field_block("interval", interval);
        }
        self.vertex_rows("data_2d", &geometry.vertices, false);
        self.segment_geometry(&geometry.segments, true);
        self.text_block("colour_data", &geometry.segment_colours);
        self.close();
    }

    fn write_alignment(&mut self, alignment: &SuperAlignment) {
        self.model(&alignment.header.model);
        self.open("string super_alignment");
        self.header(&alignment.header);
        self.pair("closed", truth(alignment.closed));
        if !alignment.spiral_type.is_empty() {
            self.pair("spiral_type", &quoted(&alignment.spiral_type));
        }
        if let Some(valid) = alignment.valid_horizontal {
            self.pair("valid_horizontal", truth(valid));
        }
        if let Some(valid) = alignment.valid_vertical {
            self.pair("valid_vertical", truth(valid));
        }
        if let Some(diameter) = alignment.diameter {
            self.open("pipe_value");
            let text = self.real(diameter);
            self.pair("diameter", &text);
            self.close();
        }
        if let Some(length) = alignment.pipe_length {
            let text = self.real(length);
            self.pair("length", &text);
        }
        self.parts("horizontal_parts", &alignment.horizontal_parts);
        if let Some(data) = &alignment.horizontal_data {
            self.solved_geometry("horizontal_data", data);
        }
        self.parts("vertical_parts", &alignment.vertical_parts);
        if let Some(data) = &alignment.vertical_data {
            self.solved_geometry("vertical_data", data);
        }
        self.close();
    }

    fn write_cloud(&mut self, cloud: &LasCloud) {
        self.model(&cloud.header.model);
        self.open("string las_cloud_data");
        self.header(&cloud.header);
        let reference = !cloud.reference_file.is_empty();
        self.open(if reference { "ref_data" } else { "data" });
        if !cloud.categories.is_empty() {
            self.value_block(
                "categories",
                cloud
                    .categories
                    .iter()
                    .map(|&value| String::from(truth(value))),
            );
        }
        let format = if cloud.format.is_empty() {
            "v12_p0"
        } else {
            cloud.format.as_str()
        };
        if reference {
            self.pair("file_name", &quoted(&cloud.reference_file));
        } else {
            self.pair("format", format);
        }
        if !cloud.range.empty() {
            self.field_block("range", &cloud.range);
        }
        if !reference {
            self.open(&format!("points_{format}"));
            let point_format = cloud.point_format;
            let modern = point_format >= 6;
            let has_time = modern || matches!(point_format, 1 | 3 | 4 | 5);
            let has_colour = matches!(point_format, 2 | 3 | 5 | 7 | 8 | 10);
            for point in &cloud.points {
                let mut text = format!(
                    "p {{ x {} y {} z {} i {} rn {} rc {}",
                    self.real(point.x),
                    self.real(point.y),
                    self.real(point.z),
                    point.intensity,
                    point.return_number,
                    point.return_count
                );
                if modern {
                    text.push_str(&format!(
                        " cf {} sc {}",
                        point.classification_flags, point.scanner_channel
                    ));
                }
                text.push_str(&format!(
                    " sd {} fe {} cl {}",
                    point.scan_direction, point.flight_line_edge, point.classification
                ));
                if modern {
                    text.push_str(&format!(" ud {} sr {}", point.user_data, point.scan_angle));
                } else {
                    text.push_str(&format!(" sr {} ud {}", point.scan_angle, point.user_data));
                }
                text.push_str(&format!(" id {}", point.point_source_id));
                if has_time {
                    text.push_str(&format!(" t {}", format_exact_real(point.gps_time)));
                }
                if has_colour {
                    text.push_str(&format!(" c {}", point.colour));
                }
                if point_format == 8 || point_format == 10 {
                    text.push_str(&format!(" ir {}", point.near_infrared));
                }
                text.push_str(" }");
                self.line(&text);
            }
            self.close();
        }
        self.close();
        self.close();
    }

    fn write_tin(&mut self, tin: &Tin) {
        // A full_tin's neighbours are mandatory (manual 1.4.7.1). A tin that
        // has none - one Katana built, say - is written in the format that
        // does not need them.
        let full = tin.full
            && tin.neighbours.len() == tin.triangles.len()
            && tin.visible.len() == tin.triangles.len();
        self.out.push('\n');
        self.open(if full { "full_tin" } else { "tin" });
        self.pair("name", &quoted(&tin.name));
        self.fields(&tin.extras);
        self.attributes(&tin.attributes);

        self.open("points");
        for point in &tin.points {
            let text = if self.options.hex_float_tins {
                format!(
                    "{} {} {}",
                    format_hex_real(point.x),
                    format_hex_real(point.y),
                    format_hex_real(point.z.unwrap_or(self.null_value))
                )
            } else {
                format!(
                    "{} {} {}",
                    self.real(point.x),
                    self.real(point.y),
                    self.height(point.z)
                )
            };
            self.line(&text);
        }
        self.close();

        // A `tin` lists visible triangles only; the colours block follows the
        // same order, so it is filtered alongside.
        let written: Vec<usize> = (0..tin.triangles.len())
            .filter(|&i| full || tin.is_surface_triangle(i))
            .collect();
        self.open("triangles");
        for &i in &written {
            let triangle = tin.triangles[i];
            self.line(&format!(
                "{} {} {}",
                triangle[0].wrapping_add(1),
                triangle[1].wrapping_add(1),
                triangle[2].wrapping_add(1)
            ));
        }
        self.close();

        if full {
            self.open("neighbours");
            for row in &tin.neighbours {
                let numbers = row.map(|neighbour| {
                    if neighbour == Tin::NO_NEIGHBOUR {
                        0
                    } else {
                        neighbour.wrapping_add(1)
                    }
                });
                self.line(&format!("{} {} {}", numbers[0], numbers[1], numbers[2]));
            }
            self.close();
            self.value_block(
                "nulling",
                tin.visible
                    .iter()
                    .map(|&visible| String::from(if visible { "2" } else { "1" })),
            );
        }
        if !tin.colour.is_empty() {
            self.pair("colour", &word(&tin.colour));
        }
        // Per-triangle colours (manual 1.4.7, both forms), one per triangle
        // written. A list of the wrong length cannot be lined up and is left
        // out; the reader said so when it read it.
        if tin.colours.len() == tin.triangles.len() {
            self.value_block(
                "colours",
                written.iter().map(|&i| {
                    let colour = &tin.colours[i];
                    if colour == "-1" {
                        colour.clone()
                    } else {
                        word(colour)
                    }
                }),
            );
        }
        if !tin.input.empty() || !tin.input_models.is_empty() {
            self.open("input");
            self.fields(&tin.input);
            self.text_block("models", &tin.input_models);
            self.close();
        }
        self.close();
    }

    fn write_super_tin(&mut self, super_tin: &SuperTin) {
        self.out.push('\n');
        self.open("super_tin");
        self.pair("name", &quoted(&super_tin.name));
        self.fields(&super_tin.extras);
        self.attributes(&super_tin.attributes);
        if !super_tin.colour.is_empty() {
            self.pair("colour", &word(&super_tin.colour));
        }
        self.open("tins"); // mandatory, so written even when empty
        for name in &super_tin.tins {
            self.line(&quoted(name));
        }
        self.close();
        self.close();
    }

    fn infos(&mut self, keyword: &str, list: &[TrimeshInfo]) {
        if list.is_empty() {
            return;
        }
        self.open(keyword);
        for info in list {
            self.line(&format!(
                "{} {} {} {}",
                info.flag,
                info.key,
                word(&info.colour),
                quoted(&info.name)
            ));
        }
        self.close();
    }

    fn flags(&mut self, keyword: &str, values: &[u32]) {
        if !values.is_empty() {
            self.value_block(keyword, values.iter().map(|value| format!("{value}")));
        }
    }

    fn write_trimesh(&mut self, mesh: &Trimesh) {
        self.model(&mesh.model);
        self.open("primitive_3d");
        self.pair("name", &quoted(&mesh.name));
        self.pair("colour", &word(&mesh.colour));
        self.fields(&mesh.extras);
        self.attributes(&mesh.attributes);
        self.open("trimesh_3d");
        if !mesh.info.empty() {
            self.field_block("info", &mesh.info);
        }
        self.vertex_rows("vertices", &mesh.vertices, true);
        self.open("faces");
        for face in &mesh.faces {
            self.line(&format!(
                "{} {} {}",
                face[0].wrapping_add(1),
                face[1].wrapping_add(1),
                face[2].wrapping_add(1)
            ));
        }
        self.close();
        if !mesh.edges.is_empty() {
            self.open("edges");
            for edge in &mesh.edges {
                self.line(&format!(
                    "{} {}",
                    edge[0].wrapping_add(1),
                    edge[1].wrapping_add(1)
                ));
            }
            self.close();
        }
        if let Some(blend) = mesh.blend {
            let text = self.real(blend);
            self.pair("blend", &text);
        }
        self.infos("vertex_infos", &mesh.vertex_infos);
        self.flags("vertex_flags", &mesh.vertex_flags);
        self.infos("edge_infos", &mesh.edge_infos);
        self.flags("edge_flags", &mesh.edge_flags);
        self.infos("face_infos", &mesh.face_infos);
        self.flags("face_flags", &mesh.face_flags);
        self.close();
        self.close();
    }

    fn write(&mut self, element: &Element) {
        match element {
            Element::VertexString(string) => self.write_vertex_string(string),
            Element::ArcString(arc) => self.write_arc(arc),
            Element::CircleString(circle) => self.write_circle(circle),
            Element::TextString(text) => self.write_text(text),
            Element::PlotFrame(frame) => self.write_plot_frame(frame),
            Element::DrainageString(drainage) => self.write_drainage(drainage),
            Element::SuperAlignment(alignment) => self.write_alignment(alignment),
            Element::LasCloud(cloud) => self.write_cloud(cloud),
            Element::Tin(tin) => self.write_tin(tin),
            Element::SuperTin(super_tin) => self.write_super_tin(super_tin),
            Element::Trimesh(mesh) => self.write_trimesh(mesh),
            Element::DraftingLeader(leader) => self.write_leader(leader),
        }
    }

    fn run(mut self, archive: &Archive) -> String {
        self.null_value = safe_null_value(archive);

        let banner = self.options.banner.clone();
        if !banner.is_empty() {
            let mut start = 0;
            while start <= banner.len() {
                match banner[start..].find('\n') {
                    None => {
                        self.line(&format!("// {}", &banner[start..]));
                        break;
                    }
                    Some(offset) => {
                        self.line(&format!("// {}", &banner[start..start + offset]));
                        start += offset + 1;
                    }
                }
            }
        }
        for setting in &archive.header_settings {
            let value = if setting.quoted {
                quoted(&setting.value)
            } else {
                setting.value.clone()
            };
            self.line(&format!("// {} {value}", setting.key));
        }
        self.out.push('\n');
        // The null VALUE, not the `null` keyword version 15 of the writing
        // program also accepts: the value is the documented form (manual
        // 1.4.5) and every version reads it.
        let null = self.real(self.null_value);
        self.line(&format!("null {null}"));

        if !archive.project_attributes.is_empty() {
            self.out.push('\n');
            self.open("project_attributes");
            self.attribute_body(&archive.project_attributes);
            self.close();
        }
        // A model that holds nothing still exists, and a model declared with
        // attributes must keep them; both are written even if no element
        // follows.
        for record in &archive.models {
            self.model(&record.name);
        }
        for name in &archive.model_names {
            let used = archive
                .elements
                .iter()
                .any(|element| equals_ignoring_case(model_of(element), name));
            if !used {
                self.model(name);
            }
        }
        for element in &archive.elements {
            if !matches!(element, Element::Tin(_) | Element::SuperTin(_)) {
                self.out.push('\n');
            }
            self.write(element);
        }
        self.out
    }
}
