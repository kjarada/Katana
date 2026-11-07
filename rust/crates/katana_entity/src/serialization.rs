//! Text serialisation of the entity model, shared by project storage and by
//! the structured command API. The format is JSON; the JSON library is an
//! implementation detail and does not appear in this interface.
//!
//! Doubles are written with a representation that parses back to the
//! identical bit pattern, so save/load round trips are exact.
//!
//!   Point      {"position":[x,y],"type":"Point"}
//!   Line       {"end":[x,y],"start":[x,y],"type":"Line"}
//!   Arc        {"center":[x,y],"radius":r,"startAngle":a,"sweep":s,"type":"Arc"}
//!   Polyline   {"closed":false,"type":"Polyline","vertices":[[x,y],...]}
//!   Circle     {"center":[x,y],"radius":r,"type":"Circle"}
//!   Text       {"height":h,"position":[x,y],"rotation":a,"text":"...","type":"Text"}
//!   Dimension  {"end":[x,y],"offset":o,"start":[x,y],"textOverride":"...","type":"Dimension"}
//!
//! (the keys sorted, as nlohmann's std::map-based json writes them). Parsing
//! validates the geometry; malformed or invalid input yields ParseFailure or
//! InvalidGeometry, never a partially filled value.
//!
//! The writers return Result, as the C++ ones do: there, nlohmann's dump()
//! throws on a string that is not valid UTF-8. A `String` cannot be one, so
//! here they always succeed; the type is kept so the callers read as they do
//! in C++.

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Spline2,
};
use serde_json::{Map, Value};

use crate::annotation::{
    LabelKind, LabelStyle, label_marker_from_string, label_orientation_from_string,
    label_placement_from_string,
};
use crate::entity::{
    AnchorPoint, AnchorRef, Color, DimensionGeometry, DimensionKind, Entity, EntityType, Geometry,
    LabelGeometry, LeaderGeometry, PointGeometry, PropertyMap, PropertyValue, TextGeometry,
    TextJustify, anchor_point_from_string, callout_shape_from_string, dimension_kind_from_string,
    entity_type_from_string, text_justify_from_string, to_string,
};
use crate::entity_geometry::validate;
use crate::json::{
    self, JsonError, at, at_index, contains, get_bool, get_f64, get_f64_vec, get_i32, get_i64,
    get_string, get_u32, get_u64, items, other_error, real, reals, sorted_entries, value_or,
};
use crate::tables::arrow_head_from_string;

/// What a reader can fail with: a nlohmann exception, which the C++ catches at
/// each public function and turns into ParseFailure "malformed JSON", or an
/// error of the model's own, which it returns as it is.
enum Failure {
    Json(JsonError),
    Model(Error),
}

impl From<JsonError> for Failure {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Model(error)
    }
}

type Read<T> = std::result::Result<T, Failure>;

/// The C++ `guarded`: a nlohmann exception never leaves this module.
fn guarded<T>(read: Read<T>) -> Result<T> {
    read.map_err(|failure| match failure {
        Failure::Json(error) => make_error(ErrorCode::ParseFailure, "malformed JSON", error.0),
        Failure::Model(error) => error,
    })
}

fn object(members: Vec<(&str, Value)>) -> Map<String, Value> {
    members
        .into_iter()
        .map(|(key, value)| (String::from(key), value))
        .collect()
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn point_to_json(point: Point2) -> Value {
    Value::Array(vec![real(point.x), real(point.y)])
}

fn points_to_json(points: &[Point2]) -> Value {
    Value::Array(points.iter().map(|&point| point_to_json(point)).collect())
}

fn point_from_json(value: &Value) -> Read<Point2> {
    if !value.is_array() || json::size(value) != 2 {
        return Err(other_error("a point must be an array of two numbers").into());
    }
    let x = get_f64(at_index(value, 0)?)?;
    let y = get_f64(at_index(value, 1)?)?;
    Ok(Point2::new(x, y))
}

fn points_from_json(value: &Value) -> Read<Vec<Point2>> {
    items(value).into_iter().map(point_from_json).collect()
}

/// An AnchorRef as {"entity": id, "point": "end", "index": 3}; the index only
/// when it says something.
fn anchor_to_json(reference: &AnchorRef) -> Value {
    let mut members = object(vec![
        ("entity", Value::from(reference.entity)),
        ("point", text(to_string(reference.point))),
    ]);
    if reference.point == AnchorPoint::Vertex
        || reference.point == AnchorPoint::SegmentMid
        || (reference.point == AnchorPoint::Along && reference.index != 0)
    {
        members.insert(String::from("index"), Value::from(reference.index));
    }
    if reference.point == AnchorPoint::Along {
        members.insert(String::from("parameter"), real(reference.parameter));
    }
    Value::Object(members)
}

fn anchor_from_json(value: &Value) -> Read<AnchorRef> {
    let entity = get_u64(at(value, "entity")?)?;
    let point = anchor_point_from_string(&get_string(at(value, "point")?)?)
        .map_err(|error| other_error(&error.describe()))?;
    let index = value_or(value, "index", 0, get_u32)?;
    let parameter = value_or(value, "parameter", 0.0, get_f64)?;
    Ok(AnchorRef {
        entity,
        point,
        index,
        parameter,
    })
}

/// The enumerations are written by name and read back through the same
/// from_string the command line uses, so the two cannot disagree.
fn enum_from_json<T>(value: &Value, parse: fn(&str) -> Result<T>) -> Read<T> {
    let name = get_string(value)?;
    parse(&name).map_err(|error| Failure::Json(other_error(&error.describe())))
}

fn geometry_to_value(geometry: &Geometry) -> Value {
    let members = match geometry {
        Geometry::Point(g) => object(vec![
            ("type", text("Point")),
            ("position", point_to_json(g.position)),
        ]),
        Geometry::Line(g) => object(vec![
            ("type", text("Line")),
            ("start", point_to_json(g.start)),
            ("end", point_to_json(g.end)),
        ]),
        Geometry::Arc(g) => object(vec![
            ("type", text("Arc")),
            ("center", point_to_json(g.center)),
            ("radius", real(g.radius)),
            ("startAngle", real(g.start_angle)),
            ("sweep", real(g.sweep)),
        ]),
        Geometry::Polyline(g) => object(vec![
            ("type", text("Polyline")),
            ("closed", Value::Bool(g.closed)),
            ("vertices", points_to_json(&g.vertices)),
        ]),
        Geometry::Circle(g) => object(vec![
            ("type", text("Circle")),
            ("center", point_to_json(g.center)),
            ("radius", real(g.radius)),
        ]),
        // The members added on 2026-09-25 are written only when they differ
        // from their defaults, so a text or a dimension of the old kind is the
        // same JSON as before.
        Geometry::Text(g) => {
            let mut members = object(vec![
                ("type", text("Text")),
                ("position", point_to_json(g.position)),
                ("text", text(&g.text)),
                ("height", real(g.height)),
                ("rotation", real(g.rotation)),
            ]);
            if !g.style.is_empty() {
                members.insert(String::from("style"), text(&g.style));
            }
            if g.paper_height != 0.0 {
                members.insert(String::from("paperHeight"), real(g.paper_height));
            }
            if g.justify != TextJustify::BottomLeft {
                members.insert(String::from("justify"), text(to_string(g.justify)));
            }
            members
        }
        Geometry::Dimension(g) => {
            let mut members = object(vec![
                ("type", text("Dimension")),
                ("start", point_to_json(g.start)),
                ("end", point_to_json(g.end)),
                ("offset", real(g.offset)),
                ("textOverride", text(&g.text_override)),
            ]);
            if g.kind != DimensionKind::Aligned {
                members.insert(String::from("kind"), text(to_string(g.kind)));
            }
            if g.angle != 0.0 {
                members.insert(String::from("angle"), real(g.angle));
            }
            if g.uses_vertex() || g.vertex != Point2::default() {
                members.insert(String::from("vertex"), point_to_json(g.vertex));
            }
            for (key, reference) in [
                ("startRef", &g.start_ref),
                ("endRef", &g.end_ref),
                ("vertexRef", &g.vertex_ref),
            ] {
                if reference.associated() {
                    members.insert(String::from(key), anchor_to_json(reference));
                }
            }
            members
        }
        Geometry::Label(g) => {
            let mut members = object(vec![
                ("type", text("Label")),
                ("style", text(&g.style)),
                ("anchor", point_to_json(g.anchor)),
            ]);
            if g.target != 0 {
                members.insert(String::from("target"), Value::from(g.target));
            }
            if !g.alignment.is_empty() {
                members.insert(String::from("alignment"), text(&g.alignment));
            }
            if g.part != -1 {
                members.insert(String::from("part"), Value::from(g.part));
            }
            if let Some(position) = g.position {
                members.insert(String::from("position"), point_to_json(position));
            }
            if !g.text_override.is_empty() {
                members.insert(String::from("textOverride"), text(&g.text_override));
            }
            if !g.rule.is_empty() {
                members.insert(String::from("rule"), text(&g.rule));
            }
            members
        }
        Geometry::Leader(g) => {
            let mut members = object(vec![
                ("type", text("Leader")),
                ("vertices", points_to_json(&g.vertices)),
                ("text", text(&g.text)),
                ("arrow", text(to_string(g.arrow))),
                ("callout", text(to_string(g.callout))),
                ("style", text(&g.style)),
                ("paperHeight", real(g.paper_height)),
                ("arrowSize", real(g.arrow_size)),
                ("landing", real(g.landing)),
            ]);
            if g.tip_ref.associated() {
                members.insert(String::from("tipRef"), anchor_to_json(&g.tip_ref));
            }
            if g.fields {
                members.insert(String::from("fields"), Value::Bool(true));
            }
            if !g.label_style.is_empty() {
                members.insert(String::from("labelStyle"), text(&g.label_style));
            }
            if let Some(angle) = g.landing_angle {
                members.insert(String::from("landingAngle"), real(angle));
            }
            members
        }
        // Bulges and heights as lists beside the vertices, a null height for
        // "not surveyed"; the lists are omitted when every entry is 0 / null.
        Geometry::CurvePolyline(g) => {
            let mut vertices = Vec::with_capacity(g.vertices.len());
            let mut bulges = Vec::with_capacity(g.vertices.len());
            let mut heights = Vec::with_capacity(g.vertices.len());
            let mut any_bulge = false;
            let mut any_height = false;
            for vertex in &g.vertices {
                vertices.push(point_to_json(vertex.position));
                bulges.push(real(vertex.bulge));
                any_bulge = any_bulge || vertex.bulge != 0.0;
                heights.push(vertex.height.map_or(Value::Null, real));
                any_height = any_height || vertex.height.is_some();
            }
            let mut members = object(vec![
                ("type", text("CurvePolyline")),
                ("closed", Value::Bool(g.closed)),
                ("vertices", Value::Array(vertices)),
            ]);
            if any_bulge {
                members.insert(String::from("bulges"), Value::Array(bulges));
            }
            if any_height {
                members.insert(String::from("heights"), Value::Array(heights));
            }
            members
        }
        Geometry::Ellipse(g) => object(vec![
            ("type", text("Ellipse")),
            ("center", point_to_json(g.center)),
            ("majorAxis", point_to_json(g.major_axis)),
            ("ratio", real(g.ratio)),
            ("startParameter", real(g.start_parameter)),
            ("sweep", real(g.sweep)),
        ]),
        Geometry::Spline(g) => {
            let mut members = object(vec![
                ("type", text("Spline")),
                ("degree", Value::from(g.degree)),
                ("controlPoints", points_to_json(&g.control_points)),
                ("knots", reals(&g.knots)),
            ]);
            if !g.weights.is_empty() {
                members.insert(String::from("weights"), reals(&g.weights));
            }
            if !g.fit_points.is_empty() {
                members.insert(String::from("fitPoints"), points_to_json(&g.fit_points));
            }
            members
        }
    };
    Value::Object(members)
}

fn text_from_json(j: &Value) -> Read<TextGeometry> {
    let mut text = TextGeometry {
        position: point_from_json(at(j, "position")?)?,
        text: get_string(at(j, "text")?)?,
        height: get_f64(at(j, "height")?)?,
        rotation: get_f64(at(j, "rotation")?)?,
        style: value_or(j, "style", String::new(), get_string)?,
        paper_height: value_or(j, "paperHeight", 0.0, get_f64)?,
        ..TextGeometry::default()
    };
    if contains(j, "justify") {
        text.justify = enum_from_json(at(j, "justify")?, text_justify_from_string)?;
    }
    Ok(text)
}

fn dimension_from_json(j: &Value) -> Read<DimensionGeometry> {
    let mut dimension = DimensionGeometry {
        start: point_from_json(at(j, "start")?)?,
        end: point_from_json(at(j, "end")?)?,
        offset: get_f64(at(j, "offset")?)?,
        text_override: value_or(j, "textOverride", String::new(), get_string)?,
        ..DimensionGeometry::default()
    };
    if contains(j, "kind") {
        dimension.kind = enum_from_json(at(j, "kind")?, dimension_kind_from_string)?;
    }
    dimension.angle = value_or(j, "angle", 0.0, get_f64)?;
    if contains(j, "vertex") {
        dimension.vertex = point_from_json(at(j, "vertex")?)?;
    }
    for (key, reference) in [
        ("startRef", &mut dimension.start_ref),
        ("endRef", &mut dimension.end_ref),
        ("vertexRef", &mut dimension.vertex_ref),
    ] {
        if contains(j, key) {
            *reference = anchor_from_json(at(j, key)?)?;
        }
    }
    Ok(dimension)
}

fn label_from_json(j: &Value) -> Read<LabelGeometry> {
    let mut label = LabelGeometry {
        style: get_string(at(j, "style")?)?,
        anchor: point_from_json(at(j, "anchor")?)?,
        target: value_or(j, "target", 0, get_u64)?,
        alignment: value_or(j, "alignment", String::new(), get_string)?,
        part: value_or(j, "part", -1, get_i32)?,
        ..LabelGeometry::default()
    };
    if contains(j, "position") {
        label.position = Some(point_from_json(at(j, "position")?)?);
    }
    label.text_override = value_or(j, "textOverride", String::new(), get_string)?;
    label.rule = value_or(j, "rule", String::new(), get_string)?;
    Ok(label)
}

fn leader_from_json(j: &Value) -> Read<LeaderGeometry> {
    let mut leader = LeaderGeometry {
        vertices: points_from_json(at(j, "vertices")?)?,
        text: value_or(j, "text", String::new(), get_string)?,
        arrow: enum_from_json(at(j, "arrow")?, arrow_head_from_string)?,
        callout: enum_from_json(at(j, "callout")?, callout_shape_from_string)?,
        style: value_or(j, "style", String::new(), get_string)?,
        paper_height: value_or(j, "paperHeight", 0.0, get_f64)?,
        arrow_size: value_or(j, "arrowSize", 2.5, get_f64)?,
        landing: value_or(j, "landing", 2.5, get_f64)?,
        ..LeaderGeometry::default()
    };
    if contains(j, "tipRef") {
        leader.tip_ref = anchor_from_json(at(j, "tipRef")?)?;
    }
    leader.fields = value_or(j, "fields", false, get_bool)?;
    leader.label_style = value_or(j, "labelStyle", String::new(), get_string)?;
    if contains(j, "landingAngle") {
        leader.landing_angle = Some(get_f64(at(j, "landingAngle")?)?);
    }
    Ok(leader)
}

/// A curve polyline's list member `key`, which must list one entry per
/// vertex when it is there.
fn list_of<'a>(j: &'a Value, key: &str, count: usize) -> Read<Option<&'a Value>> {
    if !contains(j, key) {
        return Ok(None);
    }
    let list = at(j, key)?;
    if !list.is_array() || json::size(list) != count {
        return Err(other_error(&format!("a polyline's {key} must list one per vertex")).into());
    }
    Ok(Some(list))
}

fn curve_polyline_from_json(j: &Value) -> Read<CurvePolyline2> {
    let closed = get_bool(at(j, "closed")?)?;
    let mut vertices = Vec::new();
    for vertex in items(at(j, "vertices")?) {
        vertices.push(CurveVertex {
            position: point_from_json(vertex)?,
            bulge: 0.0,
            height: None,
        });
    }
    if let Some(bulges) = list_of(j, "bulges", vertices.len())? {
        for (index, vertex) in vertices.iter_mut().enumerate() {
            vertex.bulge = get_f64(at_index(bulges, index)?)?;
        }
    }
    if let Some(heights) = list_of(j, "heights", vertices.len())? {
        for (index, vertex) in vertices.iter_mut().enumerate() {
            let height = at_index(heights, index)?;
            if !height.is_null() {
                vertex.height = Some(get_f64(height)?);
            }
        }
    }
    Ok(CurvePolyline2 { vertices, closed })
}

fn ellipse_from_json(j: &Value) -> Read<Ellipse2> {
    Ok(Ellipse2 {
        center: point_from_json(at(j, "center")?)?,
        major_axis: point_from_json(at(j, "majorAxis")?)?,
        ratio: get_f64(at(j, "ratio")?)?,
        start_parameter: get_f64(at(j, "startParameter")?)?,
        sweep: get_f64(at(j, "sweep")?)?,
    })
}

fn spline_from_json(j: &Value) -> Read<Spline2> {
    let mut spline = Spline2 {
        degree: get_i32(at(j, "degree")?)?,
        control_points: points_from_json(at(j, "controlPoints")?)?,
        knots: get_f64_vec(at(j, "knots")?)?,
        ..Spline2::default()
    };
    if contains(j, "weights") {
        spline.weights = get_f64_vec(at(j, "weights")?)?;
    }
    if contains(j, "fitPoints") {
        spline.fit_points = points_from_json(at(j, "fitPoints")?)?;
    }
    Ok(spline)
}

fn geometry_from_value(j: &Value) -> Read<Geometry> {
    let entity_type = entity_type_from_string(&get_string(at(j, "type")?)?)?;
    let geometry = match entity_type {
        EntityType::Point => Geometry::Point(PointGeometry {
            position: point_from_json(at(j, "position")?)?,
        }),
        EntityType::Line => {
            let start = point_from_json(at(j, "start")?)?;
            let end = point_from_json(at(j, "end")?)?;
            Geometry::Line(Segment2::new(start, end))
        }
        EntityType::Arc => {
            let center = point_from_json(at(j, "center")?)?;
            let radius = get_f64(at(j, "radius")?)?;
            let start_angle = get_f64(at(j, "startAngle")?)?;
            let sweep = get_f64(at(j, "sweep")?)?;
            Geometry::Arc(Arc2::new(center, radius, start_angle, sweep))
        }
        EntityType::Polyline => {
            let closed = get_bool(at(j, "closed")?)?;
            let vertices = points_from_json(at(j, "vertices")?)?;
            Geometry::Polyline(Polyline2::new(vertices, closed))
        }
        EntityType::Circle => {
            let center = point_from_json(at(j, "center")?)?;
            let radius = get_f64(at(j, "radius")?)?;
            Geometry::Circle(Circle2::new(center, radius))
        }
        EntityType::Text => Geometry::Text(text_from_json(j)?),
        EntityType::Dimension => Geometry::Dimension(dimension_from_json(j)?),
        EntityType::Label => Geometry::Label(label_from_json(j)?),
        EntityType::Leader => Geometry::Leader(leader_from_json(j)?),
        EntityType::CurvePolyline => Geometry::CurvePolyline(curve_polyline_from_json(j)?),
        EntityType::Ellipse => Geometry::Ellipse(ellipse_from_json(j)?),
        EntityType::Spline => Geometry::Spline(spline_from_json(j)?),
    };
    validate(&geometry)?;
    Ok(geometry)
}

fn properties_to_value(properties: &PropertyMap) -> Value {
    Value::Object(
        properties
            .iter()
            .map(|(name, value)| {
                let member = match value {
                    PropertyValue::Boolean(flag) => Value::Bool(*flag),
                    PropertyValue::Integer(integer) => Value::from(*integer),
                    PropertyValue::Real(number) => real(*number),
                    PropertyValue::Text(words) => text(words),
                };
                (name.clone(), member)
            })
            .collect(),
    )
}

fn properties_from_value(j: &Value) -> Read<PropertyMap> {
    let Value::Object(map) = j else {
        return Err(Error::new(ErrorCode::ParseFailure, "properties must be a JSON object").into());
    };
    let mut properties = PropertyMap::new();
    // In key order, as nlohmann's std::map walks them, so the first value
    // refused is the one the C++ names.
    for (name, value) in sorted_entries(map) {
        let property = match value {
            Value::Bool(flag) => PropertyValue::Boolean(*flag),
            Value::Number(number) if !number.is_f64() => PropertyValue::Integer(get_i64(value)?),
            Value::Number(_) => PropertyValue::Real(get_f64(value)?),
            Value::String(words) => PropertyValue::Text(words.clone()),
            _ => {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "property values must be boolean, number or string",
                    name.as_str(),
                )
                .into());
            }
        };
        properties.entry(name.clone()).or_insert(property);
    }
    Ok(properties)
}

pub fn geometry_to_json(geometry: &Geometry) -> Result<String> {
    Ok(json::dump(&geometry_to_value(geometry)))
}

pub fn geometry_from_json(json: &str) -> Result<Geometry> {
    guarded(
        json::parse(json)
            .map_err(Failure::from)
            .and_then(|value| geometry_from_value(&value)),
    )
}

pub fn properties_to_json(properties: &PropertyMap) -> Result<String> {
    Ok(json::dump(&properties_to_value(properties)))
}

pub fn properties_from_json(json: &str) -> Result<PropertyMap> {
    guarded(
        json::parse(json)
            .map_err(Failure::from)
            .and_then(|value| properties_from_value(&value)),
    )
}

/// {"color":"#RRGGBB"|null,"geometry":{...},"id":1,"layer":"0",
///  "metadata":{...},"properties":{...},"style":"","visible":true}
pub fn entity_to_json(entity: &Entity) -> Result<String> {
    let members = object(vec![
        ("id", Value::from(entity.id)),
        ("layer", text(&entity.layer)),
        ("style", text(&entity.style)),
        ("visible", Value::Bool(entity.visible)),
        ("geometry", geometry_to_value(&entity.geometry)),
        ("properties", properties_to_value(&entity.properties)),
        ("metadata", properties_to_value(&entity.metadata)),
        (
            "color",
            entity
                .color
                .map_or(Value::Null, |color| text(&color.to_hex())),
        ),
    ]);
    Ok(json::dump(&Value::Object(members)))
}

fn entity_from_value(j: &Value) -> Read<Entity> {
    let mut entity = Entity {
        id: value_or(j, "id", 0, get_u64)?,
        layer: value_or(j, "layer", String::from("0"), get_string)?,
        style: value_or(j, "style", String::new(), get_string)?,
        visible: value_or(j, "visible", true, get_bool)?,
        ..Entity::default()
    };
    if contains(j, "color") && !at(j, "color")?.is_null() {
        entity.color = Some(Color::from_hex(&get_string(at(j, "color")?)?)?);
    }
    entity.geometry = geometry_from_value(at(j, "geometry")?)?;
    if contains(j, "properties") {
        entity.properties = properties_from_value(at(j, "properties")?)?;
    }
    if contains(j, "metadata") {
        entity.metadata = properties_from_value(at(j, "metadata")?)?;
    }
    Ok(entity)
}

pub fn entity_from_json(json: &str) -> Result<Entity> {
    guarded(
        json::parse(json)
            .map_err(Failure::from)
            .and_then(|value| entity_from_value(&value)),
    )
}

// ---- label style definitions ------------------------------------------------------

/// The version label_style_definition_to_json writes.
const LABEL_STYLE_DEFINITION_VERSION: i32 = 1;

/// A label style's definition (everything but its name and kind, which the
/// project store keeps as columns) as versioned JSON: {"version": 1, ...},
/// each member written only when it differs from LabelStyle's default, and a
/// member left out read as that default - so a member added later needs no
/// schema migration. A version newer than this build's is refused
/// (Unsupported) rather than read wrongly.
pub fn label_style_definition_to_json(style: &LabelStyle) -> Result<String> {
    let defaults = LabelStyle::default();
    let mut members = object(vec![(
        "version",
        Value::from(LABEL_STYLE_DEFINITION_VERSION),
    )]);
    // Always written: the template is what a style is for, and a reader of
    // the raw row should see it.
    members.insert(String::from("text"), text(&style.text));
    if style.text_style != defaults.text_style {
        members.insert(String::from("textStyle"), text(&style.text_style));
    }
    // Doubles are compared bit for bit through ==, which is right here: they
    // are either the default literal or a value someone set.
    for (key, value, fallback) in [
        ("paperHeight", style.paper_height, defaults.paper_height),
        ("offset", style.offset, defaults.offset),
        ("markerSize", style.marker_size, defaults.marker_size),
        (
            "minimumLength",
            style.minimum_length,
            defaults.minimum_length,
        ),
        ("interval", style.interval, defaults.interval),
        ("tickInterval", style.tick_interval, defaults.tick_interval),
        ("tickLength", style.tick_length, defaults.tick_length),
    ] {
        if value != fallback {
            members.insert(String::from(key), real(value));
        }
    }
    if style.placement != defaults.placement {
        members.insert(String::from("placement"), text(to_string(style.placement)));
    }
    if style.orientation != defaults.orientation {
        members.insert(
            String::from("orientation"),
            text(to_string(style.orientation)),
        );
    }
    if style.marker != defaults.marker {
        members.insert(String::from("marker"), text(to_string(style.marker)));
    }
    if style.leader != defaults.leader {
        members.insert(String::from("leader"), Value::Bool(style.leader));
    }
    if style.displace != defaults.displace {
        members.insert(String::from("displace"), Value::Bool(style.displace));
    }
    if style.priority != defaults.priority {
        members.insert(String::from("priority"), Value::from(style.priority));
    }
    if let Some(color) = style.color {
        members.insert(String::from("color"), text(&color.to_hex()));
    }
    Ok(json::dump(&Value::Object(members)))
}

fn colour_from_json(value: &Value) -> Read<Color> {
    let name = get_string(value)?;
    Color::from_hex(&name).map_err(|error| Failure::Json(other_error(&error.describe())))
}

fn label_style_from_value(j: &Value, name: &str, kind: LabelKind) -> Read<LabelStyle> {
    let version = get_i32(at(j, "version")?)?;
    if version > LABEL_STYLE_DEFINITION_VERSION {
        return Err(make_error(
            ErrorCode::Unsupported,
            "the label style was written by a newer version of Katana",
            format!("version={version}"),
        )
        .into());
    }
    let defaults = LabelStyle::default();
    let mut read = LabelStyle {
        name: name.to_owned(),
        kind,
        text: get_string(at(j, "text")?)?,
        text_style: value_or(j, "textStyle", defaults.text_style.clone(), get_string)?,
        paper_height: value_or(j, "paperHeight", defaults.paper_height, get_f64)?,
        offset: value_or(j, "offset", defaults.offset, get_f64)?,
        marker_size: value_or(j, "markerSize", defaults.marker_size, get_f64)?,
        minimum_length: value_or(j, "minimumLength", defaults.minimum_length, get_f64)?,
        interval: value_or(j, "interval", defaults.interval, get_f64)?,
        tick_interval: value_or(j, "tickInterval", defaults.tick_interval, get_f64)?,
        tick_length: value_or(j, "tickLength", defaults.tick_length, get_f64)?,
        ..defaults
    };
    if contains(j, "placement") {
        read.placement = enum_from_json(at(j, "placement")?, label_placement_from_string)?;
    }
    if contains(j, "orientation") {
        read.orientation = enum_from_json(at(j, "orientation")?, label_orientation_from_string)?;
    }
    if contains(j, "marker") {
        read.marker = enum_from_json(at(j, "marker")?, label_marker_from_string)?;
    }
    read.leader = value_or(j, "leader", read.leader, get_bool)?;
    read.displace = value_or(j, "displace", read.displace, get_bool)?;
    read.priority = value_or(j, "priority", read.priority, get_i32)?;
    if contains(j, "color") {
        read.color = Some(colour_from_json(at(j, "color")?)?);
    }
    Ok(read)
}

/// Fills everything but `name` and `kind` of `style` from `json`.
pub fn label_style_definition_from_json(json: &str, style: &mut LabelStyle) -> Status {
    let read = guarded(
        json::parse(json)
            .map_err(Failure::from)
            .and_then(|value| label_style_from_value(&value, &style.name, style.kind)),
    )?;
    *style = read;
    Ok(())
}
