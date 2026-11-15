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
use katana_core::json::{Json, JsonError, ObjectMap};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Spline2,
};

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
        Failure::Json(error) => make_error(ErrorCode::ParseFailure, "malformed JSON", error.what),
        Failure::Model(error) => error,
    })
}

/// other_error 501, which this module throws for its own refusals.
fn other_error(message: &str) -> JsonError {
    JsonError::other_error(501, message)
}

fn point_to_json(point: Point2) -> Json {
    Json::array_of([Json::from(point.x), Json::from(point.y)])
}

fn points_to_json(points: &[Point2]) -> Json {
    Json::array_of(points.iter().map(|&point| point_to_json(point)))
}

fn reals_to_json(values: &[f64]) -> Json {
    Json::array_of(values.iter().map(|&value| Json::from(value)))
}

fn point_from_json(value: &Json) -> Read<Point2> {
    if !value.is_array() || value.size() != 2 {
        return Err(other_error("a point must be an array of two numbers").into());
    }
    let x = value.at_index(0)?.get::<f64>()?;
    let y = value.at_index(1)?.get::<f64>()?;
    Ok(Point2::new(x, y))
}

fn points_from_json(value: &Json) -> Read<Vec<Point2>> {
    value.iter().map(point_from_json).collect()
}

/// An AnchorRef as {"entity": id, "point": "end", "index": 3}; the index only
/// when it says something.
fn anchor_to_json(reference: &AnchorRef) -> Json {
    let mut members = Json::object_of([
        ("entity", Json::from(reference.entity)),
        ("point", Json::from(to_string(reference.point))),
    ]);
    if reference.point == AnchorPoint::Vertex
        || reference.point == AnchorPoint::SegmentMid
        || (reference.point == AnchorPoint::Along && reference.index != 0)
    {
        members["index"] = Json::from(reference.index);
    }
    if reference.point == AnchorPoint::Along {
        members["parameter"] = Json::from(reference.parameter);
    }
    members
}

fn anchor_from_json(value: &Json) -> Read<AnchorRef> {
    let entity = value.at("entity")?.get::<u64>()?;
    let point = anchor_point_from_string(&value.at("point")?.get::<String>()?)
        .map_err(|error| other_error(&error.describe()))?;
    let index = value.value("index", 0_u32)?;
    let parameter = value.value("parameter", 0.0)?;
    Ok(AnchorRef {
        entity,
        point,
        index,
        parameter,
    })
}

/// The enumerations are written by name and read back through the same
/// from_string the command line uses, so the two cannot disagree.
fn enum_from_json<T>(value: &Json, parse: fn(&str) -> Result<T>) -> Read<T> {
    let name = value.get::<String>()?;
    parse(&name).map_err(|error| Failure::Json(other_error(&error.describe())))
}

fn geometry_to_value(geometry: &Geometry) -> Json {
    match geometry {
        Geometry::Point(g) => Json::object_of([
            ("type", Json::from("Point")),
            ("position", point_to_json(g.position)),
        ]),
        Geometry::Line(g) => Json::object_of([
            ("type", Json::from("Line")),
            ("start", point_to_json(g.start)),
            ("end", point_to_json(g.end)),
        ]),
        Geometry::Arc(g) => Json::object_of([
            ("type", Json::from("Arc")),
            ("center", point_to_json(g.center)),
            ("radius", Json::from(g.radius)),
            ("startAngle", Json::from(g.start_angle)),
            ("sweep", Json::from(g.sweep)),
        ]),
        Geometry::Polyline(g) => Json::object_of([
            ("type", Json::from("Polyline")),
            ("closed", Json::from(g.closed)),
            ("vertices", points_to_json(&g.vertices)),
        ]),
        Geometry::Circle(g) => Json::object_of([
            ("type", Json::from("Circle")),
            ("center", point_to_json(g.center)),
            ("radius", Json::from(g.radius)),
        ]),
        // The members added on 2026-09-25 are written only when they differ
        // from their defaults, so a text or a dimension of the old kind is the
        // same JSON as before.
        Geometry::Text(g) => {
            let mut members = Json::object_of([
                ("type", Json::from("Text")),
                ("position", point_to_json(g.position)),
                ("text", Json::from(&g.text)),
                ("height", Json::from(g.height)),
                ("rotation", Json::from(g.rotation)),
            ]);
            if !g.style.is_empty() {
                members["style"] = Json::from(&g.style);
            }
            if g.paper_height != 0.0 {
                members["paperHeight"] = Json::from(g.paper_height);
            }
            if g.justify != TextJustify::BottomLeft {
                members["justify"] = Json::from(to_string(g.justify));
            }
            members
        }
        Geometry::Dimension(g) => {
            let mut members = Json::object_of([
                ("type", Json::from("Dimension")),
                ("start", point_to_json(g.start)),
                ("end", point_to_json(g.end)),
                ("offset", Json::from(g.offset)),
                ("textOverride", Json::from(&g.text_override)),
            ]);
            if g.kind != DimensionKind::Aligned {
                members["kind"] = Json::from(to_string(g.kind));
            }
            if g.angle != 0.0 {
                members["angle"] = Json::from(g.angle);
            }
            if g.uses_vertex() || g.vertex != Point2::default() {
                members["vertex"] = point_to_json(g.vertex);
            }
            for (key, reference) in [
                ("startRef", &g.start_ref),
                ("endRef", &g.end_ref),
                ("vertexRef", &g.vertex_ref),
            ] {
                if reference.associated() {
                    members[key] = anchor_to_json(reference);
                }
            }
            members
        }
        Geometry::Label(g) => {
            let mut members = Json::object_of([
                ("type", Json::from("Label")),
                ("style", Json::from(&g.style)),
                ("anchor", point_to_json(g.anchor)),
            ]);
            if g.target != 0 {
                members["target"] = Json::from(g.target);
            }
            if !g.alignment.is_empty() {
                members["alignment"] = Json::from(&g.alignment);
            }
            if g.part != -1 {
                members["part"] = Json::from(g.part);
            }
            if let Some(position) = g.position {
                members["position"] = point_to_json(position);
            }
            if !g.text_override.is_empty() {
                members["textOverride"] = Json::from(&g.text_override);
            }
            if !g.rule.is_empty() {
                members["rule"] = Json::from(&g.rule);
            }
            members
        }
        Geometry::Leader(g) => {
            let mut members = Json::object_of([
                ("type", Json::from("Leader")),
                ("vertices", points_to_json(&g.vertices)),
                ("text", Json::from(&g.text)),
                ("arrow", Json::from(to_string(g.arrow))),
                ("callout", Json::from(to_string(g.callout))),
                ("style", Json::from(&g.style)),
                ("paperHeight", Json::from(g.paper_height)),
                ("arrowSize", Json::from(g.arrow_size)),
                ("landing", Json::from(g.landing)),
            ]);
            if g.tip_ref.associated() {
                members["tipRef"] = anchor_to_json(&g.tip_ref);
            }
            if g.fields {
                members["fields"] = Json::from(true);
            }
            if !g.label_style.is_empty() {
                members["labelStyle"] = Json::from(&g.label_style);
            }
            if let Some(angle) = g.landing_angle {
                members["landingAngle"] = Json::from(angle);
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
                bulges.push(Json::from(vertex.bulge));
                any_bulge = any_bulge || vertex.bulge != 0.0;
                heights.push(vertex.height.map_or(Json::Null, Json::from));
                any_height = any_height || vertex.height.is_some();
            }
            let mut members = Json::object_of([
                ("type", Json::from("CurvePolyline")),
                ("closed", Json::from(g.closed)),
                ("vertices", Json::from(vertices)),
            ]);
            if any_bulge {
                members["bulges"] = Json::from(bulges);
            }
            if any_height {
                members["heights"] = Json::from(heights);
            }
            members
        }
        Geometry::Ellipse(g) => Json::object_of([
            ("type", Json::from("Ellipse")),
            ("center", point_to_json(g.center)),
            ("majorAxis", point_to_json(g.major_axis)),
            ("ratio", Json::from(g.ratio)),
            ("startParameter", Json::from(g.start_parameter)),
            ("sweep", Json::from(g.sweep)),
        ]),
        Geometry::Spline(g) => {
            let mut members = Json::object_of([
                ("type", Json::from("Spline")),
                ("degree", Json::from(g.degree)),
                ("controlPoints", points_to_json(&g.control_points)),
                ("knots", reals_to_json(&g.knots)),
            ]);
            if !g.weights.is_empty() {
                members["weights"] = reals_to_json(&g.weights);
            }
            if !g.fit_points.is_empty() {
                members["fitPoints"] = points_to_json(&g.fit_points);
            }
            members
        }
    }
}

fn text_from_json(j: &Json) -> Read<TextGeometry> {
    let mut text = TextGeometry {
        position: point_from_json(j.at("position")?)?,
        text: j.at("text")?.get::<String>()?,
        height: j.at("height")?.get::<f64>()?,
        rotation: j.at("rotation")?.get::<f64>()?,
        style: j.value("style", String::new())?,
        paper_height: j.value("paperHeight", 0.0)?,
        ..TextGeometry::default()
    };
    if j.contains("justify") {
        text.justify = enum_from_json(j.at("justify")?, text_justify_from_string)?;
    }
    Ok(text)
}

fn dimension_from_json(j: &Json) -> Read<DimensionGeometry> {
    let mut dimension = DimensionGeometry {
        start: point_from_json(j.at("start")?)?,
        end: point_from_json(j.at("end")?)?,
        offset: j.at("offset")?.get::<f64>()?,
        text_override: j.value("textOverride", String::new())?,
        ..DimensionGeometry::default()
    };
    if j.contains("kind") {
        dimension.kind = enum_from_json(j.at("kind")?, dimension_kind_from_string)?;
    }
    dimension.angle = j.value("angle", 0.0)?;
    if j.contains("vertex") {
        dimension.vertex = point_from_json(j.at("vertex")?)?;
    }
    for (key, reference) in [
        ("startRef", &mut dimension.start_ref),
        ("endRef", &mut dimension.end_ref),
        ("vertexRef", &mut dimension.vertex_ref),
    ] {
        if j.contains(key) {
            *reference = anchor_from_json(j.at(key)?)?;
        }
    }
    Ok(dimension)
}

fn label_from_json(j: &Json) -> Read<LabelGeometry> {
    let mut label = LabelGeometry {
        style: j.at("style")?.get::<String>()?,
        anchor: point_from_json(j.at("anchor")?)?,
        target: j.value("target", 0_u64)?,
        alignment: j.value("alignment", String::new())?,
        part: j.value("part", -1_i32)?,
        ..LabelGeometry::default()
    };
    if j.contains("position") {
        label.position = Some(point_from_json(j.at("position")?)?);
    }
    label.text_override = j.value("textOverride", String::new())?;
    label.rule = j.value("rule", String::new())?;
    Ok(label)
}

fn leader_from_json(j: &Json) -> Read<LeaderGeometry> {
    let mut leader = LeaderGeometry {
        vertices: points_from_json(j.at("vertices")?)?,
        text: j.value("text", String::new())?,
        arrow: enum_from_json(j.at("arrow")?, arrow_head_from_string)?,
        callout: enum_from_json(j.at("callout")?, callout_shape_from_string)?,
        style: j.value("style", String::new())?,
        paper_height: j.value("paperHeight", 0.0)?,
        arrow_size: j.value("arrowSize", 2.5)?,
        landing: j.value("landing", 2.5)?,
        ..LeaderGeometry::default()
    };
    if j.contains("tipRef") {
        leader.tip_ref = anchor_from_json(j.at("tipRef")?)?;
    }
    leader.fields = j.value("fields", false)?;
    leader.label_style = j.value("labelStyle", String::new())?;
    if j.contains("landingAngle") {
        leader.landing_angle = Some(j.at("landingAngle")?.get::<f64>()?);
    }
    Ok(leader)
}

/// A curve polyline's list member `key`, which must list one entry per
/// vertex when it is there.
fn list_of<'a>(j: &'a Json, key: &str, count: usize) -> Read<Option<&'a Json>> {
    if !j.contains(key) {
        return Ok(None);
    }
    let list = j.at(key)?;
    if !list.is_array() || list.size() != count {
        return Err(other_error(&format!("a polyline's {key} must list one per vertex")).into());
    }
    Ok(Some(list))
}

fn curve_polyline_from_json(j: &Json) -> Read<CurvePolyline2> {
    let closed = j.at("closed")?.get::<bool>()?;
    let mut vertices = Vec::new();
    for vertex in j.at("vertices")?.iter() {
        vertices.push(CurveVertex {
            position: point_from_json(vertex)?,
            bulge: 0.0,
            height: None,
        });
    }
    if let Some(bulges) = list_of(j, "bulges", vertices.len())? {
        for (index, vertex) in vertices.iter_mut().enumerate() {
            vertex.bulge = bulges.at_index(index)?.get::<f64>()?;
        }
    }
    if let Some(heights) = list_of(j, "heights", vertices.len())? {
        for (index, vertex) in vertices.iter_mut().enumerate() {
            let height = heights.at_index(index)?;
            if !height.is_null() {
                vertex.height = Some(height.get::<f64>()?);
            }
        }
    }
    Ok(CurvePolyline2 { vertices, closed })
}

fn ellipse_from_json(j: &Json) -> Read<Ellipse2> {
    Ok(Ellipse2 {
        center: point_from_json(j.at("center")?)?,
        major_axis: point_from_json(j.at("majorAxis")?)?,
        ratio: j.at("ratio")?.get::<f64>()?,
        start_parameter: j.at("startParameter")?.get::<f64>()?,
        sweep: j.at("sweep")?.get::<f64>()?,
    })
}

fn spline_from_json(j: &Json) -> Read<Spline2> {
    let mut spline = Spline2 {
        degree: j.at("degree")?.get::<i32>()?,
        control_points: points_from_json(j.at("controlPoints")?)?,
        knots: j.at("knots")?.get::<Vec<f64>>()?,
        ..Spline2::default()
    };
    if j.contains("weights") {
        spline.weights = j.at("weights")?.get::<Vec<f64>>()?;
    }
    if j.contains("fitPoints") {
        spline.fit_points = points_from_json(j.at("fitPoints")?)?;
    }
    Ok(spline)
}

fn geometry_from_value(j: &Json) -> Read<Geometry> {
    let entity_type = entity_type_from_string(&j.at("type")?.get::<String>()?)?;
    let geometry = match entity_type {
        EntityType::Point => Geometry::Point(PointGeometry {
            position: point_from_json(j.at("position")?)?,
        }),
        EntityType::Line => {
            let start = point_from_json(j.at("start")?)?;
            let end = point_from_json(j.at("end")?)?;
            Geometry::Line(Segment2::new(start, end))
        }
        EntityType::Arc => {
            let center = point_from_json(j.at("center")?)?;
            let radius = j.at("radius")?.get::<f64>()?;
            let start_angle = j.at("startAngle")?.get::<f64>()?;
            let sweep = j.at("sweep")?.get::<f64>()?;
            Geometry::Arc(Arc2::new(center, radius, start_angle, sweep))
        }
        EntityType::Polyline => {
            let closed = j.at("closed")?.get::<bool>()?;
            let vertices = points_from_json(j.at("vertices")?)?;
            Geometry::Polyline(Polyline2::new(vertices, closed))
        }
        EntityType::Circle => {
            let center = point_from_json(j.at("center")?)?;
            let radius = j.at("radius")?.get::<f64>()?;
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

fn properties_to_value(properties: &PropertyMap) -> Json {
    let mut object = Json::object();
    for (name, value) in properties {
        object[name.as_str()] = match value {
            PropertyValue::Boolean(flag) => Json::from(*flag),
            PropertyValue::Integer(integer) => Json::from(*integer),
            PropertyValue::Real(number) => Json::from(*number),
            PropertyValue::Text(words) => Json::from(words),
        };
    }
    object
}

fn properties_from_value(j: &Json) -> Read<PropertyMap> {
    let Some(map) = j.as_object() else {
        return Err(Error::new(ErrorCode::ParseFailure, "properties must be a JSON object").into());
    };
    let mut properties = PropertyMap::new();
    // In key order, as nlohmann's std::map walks them, so the first value
    // refused is the one the C++ names.
    for (name, value) in map.iter() {
        let property = match value {
            Json::Boolean(flag) => PropertyValue::Boolean(*flag),
            Json::NumberInteger(_) | Json::NumberUnsigned(_) => {
                PropertyValue::Integer(value.get::<i64>()?)
            }
            Json::NumberFloat(_) => PropertyValue::Real(value.get::<f64>()?),
            Json::String(words) => PropertyValue::Text(words.clone()),
            _ => {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "property values must be boolean, number or string",
                    name,
                )
                .into());
            }
        };
        properties.entry(name.to_owned()).or_insert(property);
    }
    Ok(properties)
}

pub fn geometry_to_json(geometry: &Geometry) -> Result<String> {
    Ok(geometry_to_value(geometry).dump())
}

pub fn geometry_from_json(json: &str) -> Result<Geometry> {
    guarded(
        Json::parse(json)
            .map_err(Failure::from)
            .and_then(|value| geometry_from_value(&value)),
    )
}

pub fn properties_to_json(properties: &PropertyMap) -> Result<String> {
    Ok(properties_to_value(properties).dump())
}

pub fn properties_from_json(json: &str) -> Result<PropertyMap> {
    guarded(
        Json::parse(json)
            .map_err(Failure::from)
            .and_then(|value| properties_from_value(&value)),
    )
}

/// {"color":"#RRGGBB"|null,"geometry":{...},"id":1,"layer":"0",
///  "metadata":{...},"properties":{...},"style":"","visible":true}
pub fn entity_to_json(entity: &Entity) -> Result<String> {
    let members = Json::object_of([
        ("id", Json::from(entity.id)),
        ("layer", Json::from(&entity.layer)),
        ("style", Json::from(&entity.style)),
        ("visible", Json::from(entity.visible)),
        ("geometry", geometry_to_value(&entity.geometry)),
        ("properties", properties_to_value(&entity.properties)),
        ("metadata", properties_to_value(&entity.metadata)),
        (
            "color",
            entity
                .color
                .map_or(Json::Null, |color| Json::from(color.to_hex())),
        ),
    ]);
    Ok(members.dump())
}

fn entity_from_value(j: &Json) -> Read<Entity> {
    let mut entity = Entity {
        id: j.value("id", 0_u64)?,
        layer: j.value("layer", String::from("0"))?,
        style: j.value("style", String::new())?,
        visible: j.value("visible", true)?,
        ..Entity::default()
    };
    if j.contains("color") && !j.at("color")?.is_null() {
        entity.color = Some(Color::from_hex(&j.at("color")?.get::<String>()?)?);
    }
    entity.geometry = geometry_from_value(j.at("geometry")?)?;
    if j.contains("properties") {
        entity.properties = properties_from_value(j.at("properties")?)?;
    }
    if j.contains("metadata") {
        entity.metadata = properties_from_value(j.at("metadata")?)?;
    }
    Ok(entity)
}

pub fn entity_from_json(json: &str) -> Result<Entity> {
    guarded(
        Json::parse(json)
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
    let mut members = Json::object_of([("version", Json::from(LABEL_STYLE_DEFINITION_VERSION))]);
    // Always written: the template is what a style is for, and a reader of
    // the raw row should see it.
    members["text"] = Json::from(&style.text);
    if style.text_style != defaults.text_style {
        members["textStyle"] = Json::from(&style.text_style);
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
            members[key] = Json::from(value);
        }
    }
    if style.placement != defaults.placement {
        members["placement"] = Json::from(to_string(style.placement));
    }
    if style.orientation != defaults.orientation {
        members["orientation"] = Json::from(to_string(style.orientation));
    }
    if style.marker != defaults.marker {
        members["marker"] = Json::from(to_string(style.marker));
    }
    if style.leader != defaults.leader {
        members["leader"] = Json::from(style.leader);
    }
    if style.displace != defaults.displace {
        members["displace"] = Json::from(style.displace);
    }
    if style.priority != defaults.priority {
        members["priority"] = Json::from(style.priority);
    }
    if let Some(color) = style.color {
        members["color"] = Json::from(color.to_hex());
    }
    Ok(members.dump())
}

fn colour_from_json(value: &Json) -> Read<Color> {
    let name = value.get::<String>()?;
    Color::from_hex(&name).map_err(|error| Failure::Json(other_error(&error.describe())))
}

fn label_style_from_value(j: &Json, name: &str, kind: LabelKind) -> Read<LabelStyle> {
    let version = j.at("version")?.get::<i32>()?;
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
        text: j.at("text")?.get::<String>()?,
        text_style: j.value("textStyle", defaults.text_style.clone())?,
        paper_height: j.value("paperHeight", defaults.paper_height)?,
        offset: j.value("offset", defaults.offset)?,
        marker_size: j.value("markerSize", defaults.marker_size)?,
        minimum_length: j.value("minimumLength", defaults.minimum_length)?,
        interval: j.value("interval", defaults.interval)?,
        tick_interval: j.value("tickInterval", defaults.tick_interval)?,
        tick_length: j.value("tickLength", defaults.tick_length)?,
        ..defaults
    };
    if j.contains("placement") {
        read.placement = enum_from_json(j.at("placement")?, label_placement_from_string)?;
    }
    if j.contains("orientation") {
        read.orientation = enum_from_json(j.at("orientation")?, label_orientation_from_string)?;
    }
    if j.contains("marker") {
        read.marker = enum_from_json(j.at("marker")?, label_marker_from_string)?;
    }
    read.leader = j.value("leader", read.leader)?;
    read.displace = j.value("displace", read.displace)?;
    read.priority = j.value("priority", read.priority)?;
    if j.contains("color") {
        read.color = Some(colour_from_json(j.at("color")?)?);
    }
    Ok(read)
}

/// Fills everything but `name` and `kind` of `style` from `json`.
pub fn label_style_definition_from_json(json: &str, style: &mut LabelStyle) -> Status {
    let read = guarded(
        Json::parse(json)
            .map_err(Failure::from)
            .and_then(|value| label_style_from_value(&value, &style.name, style.kind)),
    )?;
    *style = read;
    Ok(())
}
