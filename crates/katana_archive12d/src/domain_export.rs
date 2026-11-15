//! Katana's domain model -> an archive (domain.rs).

use std::collections::{BTreeMap, HashSet};
use std::f64::consts::PI;

use katana_core::error::{Error, ErrorCode, Result};
use katana_entity::entity::heights_of;
use katana_entity::tables::CONTINUOUS_LINETYPE;
use katana_entity::{Alignment, Entity, EntityId, Geometry, Model, PropertyMap, PropertyValue};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::curves2d::{CURVE_CHORD_TOLERANCE, arc_from_bulge};
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentShape, Point2, ProfileElementKind, Spiral2, TriangleMesh, VerticalAlignment,
};
use katana_terrain::TinSurface;

use crate::archive::{
    AlignmentPart, Attribute, AttributeList, AttributeValue, Breakline, CircleString, FieldList,
    Segment, SegmentKind, SolvedGeometry, StringHeader, StringKind, SuperAlignment, TextString,
    Tin, Trimesh, TrimeshInfo, Vertex, VertexString,
};
use crate::domain::{
    DomainExport, ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, ExportMesh, ExportOptions,
    ExportSurface, META_BREAKLINE, META_CHAINAGE, META_COLOUR, META_COLOUR_RGB, META_ELEMENT,
    META_EXTRA_PREFIX, META_NAME, META_POINT_IDS, META_STRING_STYLE, META_SYMBOL_PREFIX,
    nearest_standard_colour, standard_colour,
};
use crate::reader::parse_real;
use crate::text_utilities::equals_ignoring_case;

/// Points, lines, polylines, arcs, circles and text all have a .12da form and
/// are written. Dimensions do not: they are skipped and counted.
pub fn from_domain(
    model: &Model,
    surfaces: &[ExportSurface<'_>],
    options: &ExportOptions,
    meshes: &[ExportMesh<'_>],
) -> Result<DomainExport> {
    if !(options.curve_tolerance > 0.0) || !options.curve_tolerance.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the curve tolerance must be positive",
        ));
    }
    if let Some(shift) = options.origin_shift
        && !(shift.x.is_finite() && shift.y.is_finite())
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the origin shift is not finite",
        ));
    }
    Ok(Exporter::new(model, options).run(surfaces, meshes))
}

/// Splits a list written by import (`12d.segment_colours`,
/// `12d.interface_modes` ...): blank-separated, an item with a blank in it in
/// double quotes with the .12da escapes. The inverse of what import joins,
/// exposed so export and a test read the same list the same way.
pub fn split_list(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut items = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            i += 1;
            continue;
        }
        let mut item = Vec::new();
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                }
                item.push(bytes[i]);
                i += 1;
            }
            i += 1; // the closing quote, if there was one
        } else {
            while i < bytes.len() && bytes[i] != b' ' && bytes[i] != b'\t' {
                item.push(bytes[i]);
                i += 1;
            }
        }
        // Only an ASCII backslash or quote is ever dropped, so the bytes kept
        // are still the UTF-8 they were cut from.
        items.push(String::from_utf8_lossy(&item).into_owned());
    }
    items
}

// The model alignments are written into. Katana's alignments belong to the
// drawing, not to a layer, and an archive string must be in some model.
const ALIGNMENT_MODEL: &str = "alignments";

fn degrees(radians: f64) -> f64 {
    let value = (radians * 180.0 / PI) % 360.0;
    if value < 0.0 { value + 360.0 } else { value }
}

fn text_of<'m>(map: &'m PropertyMap, key: &str) -> Option<&'m String> {
    match map.get(key) {
        Some(PropertyValue::Text(text)) => Some(text),
        _ => None,
    }
}

fn real_of(map: &PropertyMap, key: &str) -> Option<f64> {
    match map.get(key) {
        Some(PropertyValue::Real(real)) => Some(*real),
        Some(PropertyValue::Integer(integer)) => Some(*integer as f64),
        _ => None,
    }
}

// "Asset/Dimensions/Size" -> group Asset { group Dimensions { Size } }: the
// inverse of the flattening import does.
fn insert_attribute(attributes: &mut AttributeList, path: &str, value: &PropertyValue) {
    if let Some(slash) = path.find('/')
        && slash > 0
        && slash + 1 < path.len()
    {
        let group_name = &path[..slash];
        for attribute in attributes.iter_mut() {
            if attribute.name == group_name
                && let AttributeValue::Group(members) = &mut attribute.value
            {
                insert_attribute(members, &path[slash + 1..], value);
                return;
            }
        }
        let mut members = AttributeList::new();
        insert_attribute(&mut members, &path[slash + 1..], value);
        attributes.push(Attribute {
            name: String::from(group_name),
            value: AttributeValue::Group(members),
            declared_type: String::new(),
        });
        return;
    }
    let value = match value {
        // The archive has no boolean attribute.
        PropertyValue::Boolean(flag) => AttributeValue::Integer(if *flag { 1 } else { 0 }),
        PropertyValue::Integer(integer) => AttributeValue::Integer(*integer),
        PropertyValue::Real(real) => AttributeValue::Real(*real),
        PropertyValue::Text(text) => AttributeValue::Text(text.clone()),
    };
    attributes.push(Attribute {
        name: String::from(path),
        value,
        declared_type: String::new(),
    });
}

// Properties keyed `<prefix>/<index>/<path>` back into one attribute list per
// index, 1-based as import wrote them. `count` sizes the result; an index past
// it - a vertex removed since import - is dropped.
fn positional_attributes(entity: &Entity, prefix: &str, count: usize) -> Vec<AttributeList> {
    let mut lists: Vec<AttributeList> = Vec::new();
    for (key, value) in &entity.properties {
        let bytes = key.as_bytes();
        if !key.starts_with(prefix) || key.len() <= prefix.len() + 1 || bytes[prefix.len()] != b'/'
        {
            continue;
        }
        let Some(offset) = key[prefix.len() + 1..].find('/') else {
            continue;
        };
        let slash = prefix.len() + 1 + offset;
        if slash + 1 >= key.len() {
            continue;
        }
        // Wrapping, as the C++'s size_t arithmetic does on a run of digits
        // too long for it.
        let mut index: usize = 0;
        for &byte in &bytes[prefix.len() + 1..slash] {
            if !byte.is_ascii_digit() {
                index = 0;
                break;
            }
            index = index
                .wrapping_mul(10)
                .wrapping_add(usize::from(byte - b'0'));
        }
        if index == 0 || index > count {
            continue;
        }
        if lists.len() < count {
            lists.resize(count, AttributeList::new());
        }
        insert_attribute(&mut lists[index - 1], &key[slash + 1..], value);
    }
    lists
}

// A spiral as the format describes one: from its straight end, whichever end
// of the segment that is. See plan_geometry.rs for where that was learned.
fn spiral_segment(spiral: &Spiral2) -> Segment {
    let mut segment = Segment {
        kind: SegmentKind::Spiral,
        ..Segment::default()
    };
    let p = &mut segment.parameters;
    let leading = spiral.start_curvature.abs() <= spiral.end_curvature.abs();
    // Positive turns right in the format.
    let radius_of = |curvature: f64| {
        if curvature == 0.0 {
            0.0
        } else {
            -1.0 / curvature
        }
    };
    p.set_text("type", "natural clothoid");
    p.set_integer("leading", if leading { 1 } else { 0 });
    p.set_real("l1", 0.0);
    if leading {
        p.set_real("r1", radius_of(spiral.start_curvature));
        p.set_real("a1", degrees(spiral.start_direction));
        p.set_real("l2", spiral.length);
        p.set_real("r2", radius_of(spiral.end_curvature));
        p.set_real("a2", degrees(spiral.end_direction()));
    } else {
        // Travelled backwards every turn changes hand, so the radii change
        // sign as well as the directions turning about. (0 stays +0: a "-0" in
        // the file would be read correctly and look like a bug.)
        let reversed = |curvature: f64| {
            let radius = radius_of(curvature);
            if radius == 0.0 { 0.0 } else { -radius }
        };
        p.set_real("r1", reversed(spiral.end_curvature));
        p.set_real("a1", degrees(spiral.end_direction() + PI));
        p.set_real("l2", spiral.length);
        p.set_real("r2", reversed(spiral.start_curvature));
        p.set_real("a2", degrees(spiral.start_direction + PI));
    }
    segment
}

// A list only fits the geometry it was read from; one of another length
// describes vertices that no longer exist and is left out.
fn reals(entity: &Entity, key: &str, count: usize) -> Vec<f64> {
    let mut values = Vec::new();
    if let Some(text) = text_of(&entity.metadata, key) {
        for item in split_list(text) {
            if let Some(value) = parse_real(&item) {
                values.push(value);
            }
        }
    }
    if values.len() != count {
        values.clear();
    }
    values
}

fn restore_flags(
    entity: &Entity,
    key: &str,
    one: &mut Option<bool>,
    list: &mut Vec<bool>,
    count: usize,
) {
    let Some(text) = text_of(&entity.metadata, key) else {
        return;
    };
    let items = split_list(text);
    if items.len() == 1 && count != 1 {
        *one = Some(items[0] != "0");
        return;
    }
    if items.len() == count {
        for item in &items {
            list.push(item != "0");
        }
    }
}

// The symbol blocks a line's vertices carried, kept as one list per key (see
// META_SYMBOL_PREFIX), go back as one block (`symbol_value`) or one per
// vertex (`symbol_data`) by the length of the lists. Lists of unequal length
// are the sign of an edit that broke them, and are not written: a symbol
// block with a missing field is one the format's program cannot draw.
fn restore_symbol_blocks(entity: &Entity, string: &mut VertexString) {
    let mut columns: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in &entity.metadata {
        let Some(suffix) = key.strip_prefix(META_SYMBOL_PREFIX) else {
            continue;
        };
        if let PropertyValue::Text(text) = value {
            columns.push((String::from(suffix), split_list(text)));
        }
    }
    let Some((_, first)) = columns.first() else {
        return;
    };
    let count = first.len();
    let aligned = columns.iter().all(|(_, values)| values.len() == count);
    if !aligned || count == 0 {
        return;
    }
    let mut blocks = vec![FieldList::new(); count];
    for (key, values) in &columns {
        for (block, value) in blocks.iter_mut().zip(values) {
            if !value.is_empty() {
                block.add(
                    key.clone(),
                    value.clone(),
                    key == "style" || key == "colour",
                );
            }
        }
    }
    if count == 1 {
        string.symbol = blocks.pop();
    } else {
        string.symbols = blocks;
    }
}

struct Exporter<'a> {
    model: &'a Model,
    options: &'a ExportOptions,
    result: DomainExport,
    dimensions: usize,
    annotations: usize,
    // By alignment name, so the alignment can write what its centreline wore.
    centrelines: BTreeMap<String, &'a Entity>,
}

impl<'a> Exporter<'a> {
    fn new(model: &'a Model, options: &'a ExportOptions) -> Self {
        Self {
            model,
            options,
            result: DomainExport::default(),
            dimensions: 0,
            annotations: 0,
            centrelines: BTreeMap::new(),
        }
    }

    fn run(mut self, surfaces: &[ExportSurface<'_>], meshes: &[ExportMesh<'_>]) -> DomainExport {
        let wanted: HashSet<EntityId> = self.options.entities.iter().copied().collect();
        let model = self.model;
        for id in model.entities.ids() {
            let Some(entity) = model.entities.find(id) else {
                continue;
            };
            if !wanted.is_empty() && !wanted.contains(&entity.id) {
                continue;
            }
            if self.write_entity(entity) {
                self.result.entities_written += 1;
            } else {
                self.result.entities_skipped += 1;
            }
        }
        if !self.centrelines.is_empty() {
            self.result.warnings.push(format!(
                "{} centreline polylines were not written as strings: the alignments they came from wrote them",
                self.centrelines.len()
            ));
        }
        if self.annotations != 0 {
            self.result.warnings.push(format!(
                "{} labels and leaders were not exported: the archive has no annotation drawn for a view",
                self.annotations
            ));
        }
        if self.dimensions != 0 {
            self.result.warnings.push(format!(
                "{} dimensions were not written: the 12da format has no dimension element",
                self.dimensions
            ));
        }
        if self.options.include_alignments && wanted.is_empty() {
            for alignment in model.alignments.all() {
                self.write_alignment(&alignment);
            }
        }
        for surface in surfaces {
            if let Some(tin) = surface.surface
                && !tin.empty()
            {
                self.write_surface(&surface.name, tin);
            }
        }
        for mesh in meshes {
            if let Some(triangles) = mesh.mesh
                && !triangles.empty()
            {
                self.write_mesh(mesh, triangles);
            }
        }
        self.result
    }

    fn vertex(&self, point: Point2, z: Option<f64>) -> Vertex {
        let shift = self.options.origin_shift.unwrap_or_default();
        Vertex {
            x: point.x + shift.x,
            y: point.y + shift.y,
            z,
        }
    }

    // The linestyle a string is drawn with: the LINETYPE of its Katana style,
    // not the style's name. Import names a style after the library linestyle
    // and gives it that linestyle as its linetype, so the two are the same
    // until someone renames the style - and a style renamed in Katana must not
    // rename the linestyle the archive draws with, which no library would
    // then define. The name is written only for a style the model does not
    // hold, where it is the only information there is.
    //
    // ByLayer (no style, or a style whose linetype is "ByLayer") is the
    // layer's linetype. Katana's "continuous" is the format's "1", its default
    // solid linestyle (manual 1.4.3): the format has no linestyle called
    // "continuous" to draw it with.
    fn linestyle_of(&self, entity: &Entity) -> String {
        let style = if entity.style.is_empty() {
            None
        } else {
            self.model.styles.find(&entity.style)
        };
        if !entity.style.is_empty() && style.is_none() {
            return entity.style.clone();
        }
        let mut linetype: &str = CONTINUOUS_LINETYPE;
        if let Some(style) = style {
            linetype = style.linetype.as_str();
        }
        if style.is_none() || equals_ignoring_case(linetype, "ByLayer") {
            linetype = match self.model.layers.find(&entity.layer) {
                Some(layer) => layer.linetype.as_str(),
                None => CONTINUOUS_LINETYPE,
            };
        }
        if linetype.is_empty() || equals_ignoring_case(linetype, CONTINUOUS_LINETYPE) {
            return String::from("1");
        }
        String::from(linetype)
    }

    // The header the archive needs, from what import left in the metadata
    // where there is any, and from the entity itself where there is not.
    fn header_for(&self, entity: &Entity, fallback: Breakline) -> StringHeader {
        let mut header = StringHeader {
            model: entity.layer.clone(),
            ..StringHeader::default()
        };
        if let Some(name) = text_of(&entity.metadata, META_NAME) {
            header.name = name.clone();
        }
        // A point that took its symbol's style still writes the linestyle it
        // came with; anything else writes the linestyle its style draws with.
        header.style = match text_of(&entity.metadata, META_STRING_STYLE) {
            Some(style) => style.clone(),
            None => self.linestyle_of(entity),
        };
        header.chainage = real_of(&entity.metadata, META_CHAINAGE).unwrap_or(0.0);
        header.breakline = match text_of(&entity.metadata, META_BREAKLINE) {
            None => fallback,
            Some(breakline) if breakline == "point" => Breakline::Point,
            Some(_) => Breakline::Line,
        };

        // Colour: the name the entity came with, unless its colour has been
        // changed since, in which case the name would be a lie.
        let named = text_of(&entity.metadata, META_COLOUR);
        let mut rgb = entity.color;
        if rgb.is_none()
            && let Some(layer) = self.model.layers.find(&entity.layer)
        {
            rgb = Some(layer.color);
        }
        // A point that took its colour from its symbol (META_SYMBOL_PREFIX)
        // has a string colour that is deliberately not the entity's, so the
        // entity's colour must not be used to second-guess the name.
        let colour_is_the_symbols =
            text_of(&entity.metadata, &format!("{META_SYMBOL_PREFIX}colour")).is_some();
        let imported_rgb = text_of(&entity.metadata, META_COLOUR_RGB);
        let standard = named.and_then(|name| standard_colour(name));
        let colour_unchanged = match imported_rgb {
            Some(hex) => entity.color.is_some_and(|color| color.to_hex() == *hex),
            None => entity.color.is_none() || standard.is_none() || standard == entity.color,
        };
        header.colour = if let Some(named) = named
            && (colour_is_the_symbols || colour_unchanged)
        {
            named.clone()
        } else if let Some(rgb) = rgb {
            nearest_standard_colour(rgb)
        } else {
            String::from("red") // the format's own default (manual 1.4.2)
        };

        if self.options.properties_as_attributes {
            for (key, value) in &entity.properties {
                // Heights are geometry, and `vertex/3/...` belongs to vertex 3
                // (finish() puts those back where they came from).
                if key != ELEVATION_PROPERTY
                    && key != ELEVATIONS_PROPERTY
                    && !key.starts_with("vertex/")
                    && !key.starts_with("segment/")
                {
                    insert_attribute(&mut header.attributes, key, value);
                }
            }
        }
        // The scalars import kept as `12d.x.<key>`, in key order rather than
        // the order they were read, which the format does not care about.
        for (key, value) in &entity.metadata {
            if let Some(suffix) = key.strip_prefix(META_EXTRA_PREFIX)
                && let PropertyValue::Text(text) = value
            {
                header.extras.add(suffix, text.clone(), false);
            }
        }
        header
    }

    // The polyline an alignment was imported as, which the alignment will
    // write for itself. Importing a super_alignment gives BOTH an Alignment
    // and a polyline of its centreline - the polyline so that something is
    // drawn when the alignment cannot be solved - and writing both puts the
    // same line in the file twice. Worse, it compounds: each round trip read
    // the duplicate back and added another.
    fn is_centreline_of_a_written_alignment(&mut self, entity: &'a Entity) -> bool {
        if !self.options.include_alignments {
            return false;
        }
        let Some(element) = text_of(&entity.metadata, META_ELEMENT) else {
            return false;
        };
        if element != "string super_alignment"
            && element != "string alignment"
            && element != "string pipeline"
        {
            return false;
        }
        let Some(name) = text_of(&entity.metadata, META_NAME) else {
            return false;
        };
        if !self.model.alignments.contains(name) {
            return false;
        }
        // Kept so the alignment can write the layer, colour and style the
        // centreline wore. Without this the appearance would go with the
        // duplicate: an alignment carries its geometry and its name, and
        // everything else about how it is drawn lived on the polyline.
        self.centrelines.entry(name.clone()).or_insert(entity);
        true
    }

    // A string whose style draws a symbol is written with the block the
    // format draws it from (see META_SYMBOL_PREFIX): the `symbol_value` form,
    // which puts one symbol on EVERY vertex.
    fn set_symbol_from_style(&self, entity: &Entity, string: &mut VertexString) {
        let Some(style) = self.model.styles.find(&entity.style) else {
            return;
        };
        if style.symbol.is_empty() {
            return;
        }
        let mut symbol = FieldList::new();
        // The symbol's library name, which the style carries; the style's own
        // name is Katana's, and may have been changed since import.
        symbol.set_text("style", style.symbol.clone());
        let colour = text_of(&entity.metadata, &format!("{META_SYMBOL_PREFIX}colour"));
        symbol.set_text(
            "colour",
            colour
                .cloned()
                .unwrap_or_else(|| string.header.colour.clone()),
        );
        // The block's fields are kept as the text they were, so a size that
        // differs from the style's is text too.
        let size = text_of(&entity.metadata, &format!("{META_SYMBOL_PREFIX}size"));
        symbol.set_real(
            "size",
            size.and_then(|size| parse_real(size))
                .unwrap_or(style.symbol_size),
        );
        for key in ["rotation", "offset", "raise"] {
            let kept = text_of(&entity.metadata, &format!("{META_SYMBOL_PREFIX}{key}"));
            symbol.add(
                key,
                kept.cloned().unwrap_or_else(|| String::from("0")),
                false,
            );
        }
        string.symbol = Some(symbol);
    }

    fn write_entity(&mut self, entity: &'a Entity) -> bool {
        if self.is_centreline_of_a_written_alignment(entity) {
            return true; // written, by its alignment
        }
        match &entity.geometry {
            Geometry::Point(point) => {
                let mut string = VertexString {
                    header: self.header_for(entity, Breakline::Point),
                    ..VertexString::default()
                };
                string
                    .vertices
                    .push(self.vertex(point.position, heights_of(&entity.properties, 1)[0]));
                self.set_symbol_from_style(entity, &mut string);
                self.finish(entity, string);
                true
            }
            Geometry::Line(segment) => {
                let mut string = VertexString {
                    header: self.header_for(entity, Breakline::Line),
                    ..VertexString::default()
                };
                let heights = heights_of(&entity.properties, 2);
                string.vertices.push(self.vertex(segment.start, heights[0]));
                string.vertices.push(self.vertex(segment.end, heights[1]));
                self.finish(entity, string);
                true
            }
            Geometry::Polyline(polyline) => {
                let mut string = VertexString {
                    header: self.header_for(entity, Breakline::Line),
                    ..VertexString::default()
                };
                // A face or an interface string is written back as one: they
                // are current types with forms of their own (manual 1.5.4,
                // 1.5.6), not super strings.
                if let Some(element) = text_of(&entity.metadata, META_ELEMENT) {
                    if element == "string face" {
                        string.kind = StringKind::Face;
                    } else if element == "string interface" {
                        string.kind = StringKind::Interface;
                    }
                }
                string.closed = polyline.closed;
                let heights = heights_of(&entity.properties, polyline.vertices.len());
                for (&position, &height) in polyline.vertices.iter().zip(&heights) {
                    string.vertices.push(self.vertex(position, height));
                }
                self.finish(entity, string);
                true
            }
            Geometry::Arc(arc) => {
                // A super string of two vertices joined by an arc: the current
                // form, and the only one that can say "the larger arc".
                let mut string = VertexString {
                    header: self.header_for(entity, Breakline::Line),
                    ..VertexString::default()
                };
                let heights = heights_of(&entity.properties, 2);
                string
                    .vertices
                    .push(self.vertex(arc.start_point(), heights[0]));
                string
                    .vertices
                    .push(self.vertex(arc.end_point(), heights[1]));
                // Counter-clockwise is a positive sweep here and a NEGATIVE
                // radius in the archive, whose positive turns right.
                string.segments.push(Segment {
                    kind: SegmentKind::Arc,
                    radius: if arc.sweep > 0.0 {
                        -arc.radius
                    } else {
                        arc.radius
                    },
                    major: arc.sweep.abs() > PI,
                    ..Segment::default()
                });
                self.finish(entity, string);
                true
            }
            Geometry::Circle(circle) => {
                let string = CircleString {
                    header: self.header_for(entity, Breakline::Line),
                    radius: circle.radius,
                    centre: self.vertex(circle.center, heights_of(&entity.properties, 1)[0]),
                    ..CircleString::default()
                };
                self.result.archive.elements.push(string.into());
                true
            }
            Geometry::Text(text) => {
                let mut string = TextString {
                    header: self.header_for(entity, Breakline::Point),
                    text: text.text.clone(),
                    position: self.vertex(text.position, heights_of(&entity.properties, 1)[0]),
                    ..TextString::default()
                };
                string.annotation.set_real("worldsize", text.height);
                string.annotation.set_real("angle", degrees(text.rotation));
                // Katana anchors text at the left end of its baseline.
                string.annotation.set_text("justify", "bottom-left");
                self.result.archive.elements.push(string.into());
                true
            }
            Geometry::Dimension(_) => {
                self.dimensions += 1;
                false
            }
            // Labels and leaders are drawn for a view, at a scale the archive
            // has no place for; counted and said, as a dimension is.
            Geometry::Label(_) | Geometry::Leader(_) => {
                self.annotations += 1;
                false
            }
            // A polyline with arcs is a super string with arc segments - the
            // archive's own form of it - and its heights, the geometry's own,
            // one a vertex (a null where it has none).
            Geometry::CurvePolyline(polyline) => {
                let mut string = VertexString {
                    header: self.header_for(entity, Breakline::Line),
                    closed: polyline.closed,
                    ..VertexString::default()
                };
                for vertex in &polyline.vertices {
                    string
                        .vertices
                        .push(self.vertex(vertex.position, vertex.height));
                }
                if polyline.has_arcs() {
                    let segment_count = polyline.segment_count();
                    for (i, vertex) in polyline.vertices.iter().enumerate().take(segment_count) {
                        let mut segment = Segment::default();
                        if let Some(arc) = arc_from_bulge(
                            vertex.position,
                            polyline.vertices[polyline.segment_end(i)].position,
                            vertex.bulge,
                        ) {
                            segment.kind = SegmentKind::Arc;
                            // Counter-clockwise is a NEGATIVE radius in the
                            // archive, whose positive turns right (as Arc2's).
                            segment.radius = if arc.sweep > 0.0 {
                                -arc.radius
                            } else {
                                arc.radius
                            };
                            segment.major = arc.sweep.abs() > PI;
                        }
                        string.segments.push(segment);
                    }
                }
                self.finish(entity, string);
                true
            }
            // No ellipse or spline in the archive: the chords of one, as a
            // line, at the drawing tolerance.
            Geometry::Ellipse(ellipse) => self.chords(
                entity,
                ellipse.tessellate(CURVE_CHORD_TOLERANCE),
                ellipse.is_full(),
            ),
            Geometry::Spline(spline) => self.chords(
                entity,
                spline.tessellate(CURVE_CHORD_TOLERANCE),
                spline.is_closed_shape(),
            ),
        }
    }

    fn chords(&mut self, entity: &Entity, mut points: Vec<Point2>, shut: bool) -> bool {
        let mut string = VertexString {
            header: self.header_for(entity, Breakline::Line),
            ..VertexString::default()
        };
        if shut && points.len() > 3 {
            points.pop();
            string.closed = true;
        }
        let height = heights_of(&entity.properties, 1)[0];
        for &point in &points {
            string.vertices.push(self.vertex(point, height));
        }
        self.finish(entity, string);
        true
    }

    fn finish(&mut self, entity: &Entity, mut string: VertexString) {
        if let Some(ids) = text_of(&entity.metadata, META_POINT_IDS) {
            let parts: Vec<String> = ids.split(',').map(String::from).collect();
            if parts.len() == string.vertices.len() {
                string.point_ids = parts;
            }
        }
        let vertices = string.vertices.len();
        let segments = string.segment_count();

        if let Some(diameter) = real_of(&entity.metadata, "12d.diameter") {
            string.diameter = Some(diameter);
        } else {
            string.diameters = reals(entity, "12d.diameters", segments);
        }
        let width = real_of(&entity.metadata, "12d.culvert_width");
        let height = real_of(&entity.metadata, "12d.culvert_height");
        if let (Some(width), Some(height)) = (width, height) {
            string.culvert = Some([width, height]);
        } else {
            let sizes = reals(entity, "12d.culverts", 2 * segments);
            let (pairs, _) = sizes.as_chunks::<2>();
            string.culverts.extend_from_slice(pairs);
        }
        if let Some(justify) = text_of(&entity.metadata, "12d.justify") {
            string.justify = justify.clone();
        }
        restore_flags(
            entity,
            "12d.vertex_visible",
            &mut string.vertex_visible_value,
            &mut string.vertex_visible,
            vertices,
        );
        restore_flags(
            entity,
            "12d.segment_visible",
            &mut string.segment_visible_value,
            &mut string.segment_visible,
            segments,
        );
        restore_flags(
            entity,
            "12d.vertex_tinable",
            &mut string.vertex_tinable_value,
            &mut string.vertex_tinable,
            vertices,
        );
        restore_flags(
            entity,
            "12d.segment_tinable",
            &mut string.segment_tinable_value,
            &mut string.segment_tinable,
            segments,
        );
        if let Some(colours) = text_of(&entity.metadata, "12d.segment_colours") {
            let items = split_list(colours);
            if items.len() == segments || items.len() == vertices {
                string.segment_colours = items;
            }
        }
        if string.kind == StringKind::Interface {
            let modes = reals(entity, "12d.interface_modes", vertices);
            // The modes were written as whole numbers by import; a cast of
            // anything else is the C++'s, there undefined out of range and
            // saturated here.
            string
                .interface_modes
                .extend(modes.iter().map(|&mode| mode as i32));
            string.interface_modes.resize(vertices, 0);
        }
        if self.options.properties_as_attributes {
            string.vertex_attributes = positional_attributes(entity, "vertex", vertices);
            string.segment_attributes = positional_attributes(entity, "segment", segments);
        }
        // The style's symbol first - it is the one the drawing is drawn with -
        // and the kept per-vertex lists only where there is no single symbol.
        if string.symbol.is_none() {
            self.set_symbol_from_style(entity, &mut string);
        }
        if string.symbol.is_none() {
            restore_symbol_blocks(entity, &mut string);
        }
        self.result.archive.elements.push(string.into());
    }

    fn write_alignment(&mut self, alignment: &Alignment) {
        let solved = match solve_alignment(&alignment.horizontal) {
            Ok(solved) => solved,
            Err(error) => {
                self.result.warnings.push(format!(
                    "alignment \"{}\" was not written: {}",
                    alignment.name, error.message
                ));
                return;
            }
        };
        let mut out = SuperAlignment::default();
        if let Some(&centreline) = self.centrelines.get(&alignment.name) {
            out.header = self.header_for(centreline, Breakline::Line);
        } else {
            out.header.model = String::from(ALIGNMENT_MODEL);
            out.header.colour = String::from("red");
            out.header.style = String::from("1");
        }
        out.header.name = alignment.name.clone();
        out.header.chainage = alignment.horizontal.start_station;
        // Katana's transition is the exact Euler spiral, which is what the
        // format calls the natural clothoid; its plain "clothoid" is an
        // approximation.
        out.spiral_type = String::from("natural clothoid");
        out.valid_horizontal = Some(true);
        let shift = self.options.origin_shift.unwrap_or_default();

        let mut part_id: i64 = 0;
        for pi in &alignment.horizontal.pis {
            let spirals = pi.radius > 0.0 && (pi.spiral_in > 0.0 || pi.spiral_out > 0.0);
            let kind = if spirals {
                "spiral"
            } else if pi.radius > 0.0 {
                "arc"
            } else {
                "ip"
            };
            let mut part = AlignmentPart {
                kind: String::from(kind),
                ..AlignmentPart::default()
            };
            part_id += 100; // manual 1.5.9.1.2: multiples of 100
            part.fields.set_integer("id", part_id);
            if kind != "ip" {
                part.fields.set_real("r", pi.radius);
            }
            if spirals {
                part.fields.set_real("l1", pi.spiral_in);
                part.fields.set_real("l2", pi.spiral_out);
            }
            part.fields.set_real("x", pi.point.x + shift.x);
            part.fields.set_real("y", pi.point.y + shift.y);
            out.horizontal_parts.push(part);
        }

        let mut plan = SolvedGeometry::default();
        plan.header.set_text("name", alignment.name.clone());
        for element in solved.elements() {
            let mut segment = Segment::default();
            let start = match &element.shape {
                AlignmentShape::Segment(line) => line.start,
                AlignmentShape::Arc(arc) => {
                    segment.kind = SegmentKind::Arc;
                    segment.radius = if arc.sweep > 0.0 {
                        -arc.radius
                    } else {
                        arc.radius
                    };
                    segment.major = arc.sweep.abs() > PI;
                    arc.start_point()
                }
                AlignmentShape::Spiral(spiral) => {
                    segment = spiral_segment(spiral);
                    spiral.start
                }
            };
            plan.vertices.push(self.vertex(start, None));
            plan.segments.push(segment);
        }
        if let Some(end) = solved.point_at_station(solved.end_station()) {
            plan.vertices.push(self.vertex(end, None));
        }
        out.horizontal_data = Some(plan);

        if let Some(vertical) = &alignment.vertical {
            self.write_vertical(vertical, &mut part_id, &mut out);
        }
        self.result.archive.elements.push(out.into());
        self.result.alignments_written += 1;
    }

    fn write_vertical(
        &mut self,
        vertical: &VerticalAlignment,
        part_id: &mut i64,
        out: &mut SuperAlignment,
    ) {
        let profile = match solve_profile(vertical) {
            Ok(profile) => profile,
            Err(error) => {
                self.result.warnings.push(format!(
                    "the profile of alignment \"{}\" was not written: {}",
                    out.header.name, error.message
                ));
                return;
            }
        };
        out.valid_vertical = Some(true);
        for pvi in &vertical.pvis {
            let mut part = AlignmentPart {
                kind: String::from(if pvi.curve_length > 0.0 {
                    "length"
                } else {
                    "ip"
                }),
                ..AlignmentPart::default()
            };
            *part_id += 100;
            part.fields.set_integer("id", *part_id);
            if pvi.curve_length > 0.0 {
                part.fields.set_real("l", pvi.curve_length);
            }
            part.fields.set_real("x", pvi.station);
            part.fields.set_real("y", pvi.elevation);
            out.vertical_parts.push(part);
        }
        let mut grade = SolvedGeometry::default();
        grade.header.set_text("name", out.header.name.clone());
        for element in profile.elements() {
            grade.vertices.push(Vertex {
                x: element.start_station,
                y: element.start_elevation,
                z: None,
            });
            let mut segment = Segment::default();
            if element.kind == ProfileElementKind::Curve
                && let Some(pvi) = vertical.pvis.get(element.pvi)
            {
                segment.kind = SegmentKind::Parabola;
                segment.parameters.set_real("chainage", pvi.station);
                segment.parameters.set_real("height", pvi.elevation);
            }
            grade.segments.push(segment);
        }
        if let Some(last) = profile.elements().last() {
            grade.vertices.push(Vertex {
                x: last.start_station + last.length,
                y: last.end_elevation(),
                z: None,
            });
        }
        out.vertical_data = Some(grade);
    }

    fn write_mesh(&mut self, mesh: &ExportMesh<'_>, triangles: &TriangleMesh) {
        let shift = self.options.origin_shift.unwrap_or_default();
        let mut trimesh = Trimesh {
            name: if mesh.name.is_empty() {
                String::from("trimesh")
            } else {
                mesh.name.clone()
            },
            model: mesh.layer.clone(),
            colour: if mesh.colour_name.is_empty() {
                String::from("green")
            } else {
                mesh.colour_name.clone()
            },
            vertices: triangles
                .vertices
                .iter()
                .map(|vertex| Vertex {
                    x: vertex.x + shift.x,
                    y: vertex.y + shift.y,
                    z: Some(vertex.z),
                })
                .collect(),
            faces: triangles.faces.clone(),
            ..Trimesh::default()
        };
        // The face colours become the `face_infos` table and the ONE-based
        // `face_flags` that index it, which is how the format carries them:
        // one info per DISTINCT colour, because that is what the format is
        // for.
        if mesh.face_colour_names.len() == triangles.faces.len() {
            let mut distinct: Vec<&String> = Vec::new();
            trimesh.face_flags.reserve(mesh.face_colour_names.len());
            for colour in &mesh.face_colour_names {
                if colour.is_empty() {
                    trimesh.face_flags.push(0);
                    continue;
                }
                match distinct.iter().position(|known| *known == colour) {
                    None => {
                        distinct.push(colour);
                        trimesh.face_infos.push(TrimeshInfo {
                            flag: distinct.len() as i64,
                            colour: colour.clone(),
                            ..TrimeshInfo::default()
                        });
                        trimesh.face_flags.push(distinct.len() as u32);
                    }
                    Some(found) => trimesh.face_flags.push((found + 1) as u32),
                }
            }
        }
        self.result.archive.elements.push(trimesh.into());
        self.result.meshes_written += 1;
    }

    fn write_surface(&mut self, name: &str, surface: &TinSurface) {
        // The `tin` form: visible triangles only, no neighbours to get wrong.
        // The manual recommends it to "most software packages".
        let shift = self.options.origin_shift.unwrap_or_default();
        let tin = Tin {
            name: if name.is_empty() {
                String::from("surface")
            } else {
                String::from(name)
            },
            colour: String::from("green"),
            points: surface
                .vertices()
                .iter()
                .map(|point| Vertex {
                    x: point.x + shift.x,
                    y: point.y + shift.y,
                    z: Some(point.z),
                })
                .collect(),
            // Counter-clockwise here, clockwise in the archive.
            triangles: surface
                .triangles()
                .iter()
                .map(|triangle| [triangle[0], triangle[2], triangle[1]])
                .collect(),
            ..Tin::default()
        };
        self.result.archive.elements.push(tin.into());
        self.result.surfaces_written += 1;
    }
}
