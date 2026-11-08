//! The road objects as JSON (src/katana_cad/road/road_object_json.cpp): what
//! the drawing keeps under its "road_objects" metadata key and ROADOBJECT LIST
//! JSON prints (road_objects.rs, which re-exports these as the header declares
//! them; docs/road_objects.md, "The JSON").

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::json::{Json, JsonError};
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::Color;
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_geometry::Point2;

use crate::road::road_objects::{
    ParameterValue, PlacedVertex, PlacementKind, ROAD_OBJECTS_VERSION, RoadObject,
    RoadObjectPlacement, RoadObjectSet, height_from_word, height_word, kind_from_word, kind_word,
    parse_parameter, placement_word,
};

const FORMAT: &str = "katana-road-objects";

// What the C++ reader throws and catches at its edge: the JSON library's own
// exceptions and its BadValue (both reported with their what() text), or an
// answer of its own given as it is.
enum Thrown {
    Refused(Error),
    What(String),
}

impl From<JsonError> for Thrown {
    fn from(error: JsonError) -> Self {
        Thrown::What(error.what)
    }
}

type Read<T> = std::result::Result<T, Thrown>;

fn bad<T>(what: impl Into<String>) -> Read<T> {
    Err(Thrown::What(what.into()))
}

fn vertex_json(vertex: &PlacedVertex) -> Json {
    // Absent is not zero: a vertex with no height says null.
    Json::array_of([
        Json::from(vertex.position.x),
        Json::from(vertex.position.y),
        vertex.z.map_or(Json::Null, Json::from),
    ])
}

fn vertex_from(json: &Json) -> Read<PlacedVertex> {
    if !json.is_array() || (json.size() != 2 && json.size() != 3) {
        return bad("a vertex is [x, y] or [x, y, z]");
    }
    let x = json.at_index(0)?.get::<f64>()?;
    let y = json.at_index(1)?.get::<f64>()?;
    let mut vertex = PlacedVertex {
        position: Point2::new(x, y),
        z: None,
    };
    if json.size() == 3 && !json.at_index(2)?.is_null() {
        vertex.z = Some(json.at_index(2)?.get::<f64>()?);
    }
    Ok(vertex)
}

fn placement_json(placement: &RoadObjectPlacement) -> Json {
    let mut json = Json::object_of([("kind", Json::from(placement_word(placement.kind)))]);
    if placement.kind == PlacementKind::Alignment {
        json["alignment"] = Json::from(placement.alignment.as_str());
        json["station"] = Json::from(placement.station);
        if let Some(to) = placement.to {
            json["to"] = Json::from(to);
        }
        json["offset"] = Json::from(placement.offset);
    } else {
        json["vertices"] = Json::array_of(placement.vertices.iter().map(vertex_json));
        if placement.source != INVALID_ENTITY_ID {
            json["source"] = Json::from(placement.source);
        }
    }
    if let Some(spacing) = placement.spacing {
        json["spacing"] = Json::from(spacing);
    }
    if let Some(heights) = placement.heights {
        json["heights"] = Json::from(height_word(heights));
    }
    if !placement.surface.is_empty() {
        json["surface"] = Json::from(placement.surface.as_str());
    }
    if placement.crossfall != 0.0 {
        json["crossfall"] = Json::from(placement.crossfall);
    }
    if placement.dz != 0.0 {
        json["dz"] = Json::from(placement.dz);
    }
    json
}

fn placement_from(json: &Json) -> Read<RoadObjectPlacement> {
    let mut placement = RoadObjectPlacement::default();
    let kind = json.at("kind")?.get::<String>()?;
    if kind == "alignment" {
        placement.kind = PlacementKind::Alignment;
        placement.alignment = json.at("alignment")?.get::<String>()?;
        placement.station = json.at("station")?.get::<f64>()?;
        if json.contains("to") {
            placement.to = Some(json.at("to")?.get::<f64>()?);
        }
        placement.offset = json.value("offset", 0.0)?;
    } else if kind == "line" || kind == "points" {
        placement.kind = if kind == "line" {
            PlacementKind::Line
        } else {
            PlacementKind::Points
        };
        for vertex in json.at("vertices")?.iter() {
            placement.vertices.push(vertex_from(vertex)?);
        }
        if placement.vertices.is_empty() {
            return bad("a placement along a line or at points has its points");
        }
        placement.source = json.value("source", INVALID_ENTITY_ID)?;
    } else {
        return bad(format!("unknown placement \"{kind}\""));
    }
    if json.contains("spacing") {
        let spacing = json.at("spacing")?.get::<f64>()?;
        placement.spacing = Some(spacing);
        if !(spacing > 0.0) {
            return bad("a spacing is more than 0");
        }
    }
    if json.contains("heights") {
        match height_from_word(&json.at("heights")?.get::<String>()?) {
            Ok(source) => placement.heights = Some(source),
            Err(error) => return bad(error.message),
        }
    }
    placement.surface = json.value("surface", String::new())?;
    placement.crossfall = json.value("crossfall", 0.0)?;
    placement.dz = json.value("dz", 0.0)?;
    Ok(placement)
}

fn object_json(object: &RoadObject) -> Json {
    let mut json = Json::object_of([
        ("id", Json::from(object.id.as_str())),
        ("kind", Json::from(kind_word(object.kind))),
    ]);
    // What was not given is left out, so a file says what a person chose.
    if !object.name.is_empty() {
        json["name"] = Json::from(object.name.as_str());
    }
    if !object.layer.is_empty() {
        json["layer"] = Json::from(object.layer.as_str());
    }
    if let Some(colour) = object.colour {
        json["colour"] = Json::from(colour.to_hex());
    }
    if !object.material.is_empty() {
        json["material"] = Json::from(object.material.as_str());
    }
    if !object.facility.is_empty() {
        json["facility"] = Json::from(object.facility.as_str());
    }
    json["placement"] = placement_json(&object.placement);
    let mut parameters = Json::object();
    for (key, value) in &object.parameters {
        parameters[key.as_str()] = match value {
            ParameterValue::Number(number) => Json::from(*number),
            ParameterValue::Text(text) => Json::from(text.as_str()),
        };
    }
    json["parameters"] = parameters;
    json
}

fn object_from(json: &Json) -> Read<RoadObject> {
    let mut object = RoadObject {
        id: json.at("id")?.get::<String>()?,
        ..RoadObject::default()
    };
    if object.id.is_empty() {
        return bad("an object has an id");
    }
    match kind_from_word(&json.at("kind")?.get::<String>()?) {
        Ok(kind) => object.kind = kind,
        Err(error) => return bad(format!("{} ({})", error.message, object.id)),
    }
    object.name = json.value("name", String::new())?;
    object.layer = json.value("layer", String::new())?;
    if json.contains("colour") {
        match Color::from_hex(&json.at("colour")?.get::<String>()?) {
            Ok(colour) => object.colour = Some(colour),
            Err(error) => return bad(format!("object {}: {}", object.id, error.message)),
        }
    }
    object.material = json.value("material", String::new())?;
    object.facility = json.value("facility", String::new())?;
    object.placement = placement_from(json.at("placement")?)?;
    if json.contains("parameters") {
        for (key, value) in json.at("parameters")?.items() {
            // Read through the one parser, so a value edited out of range in
            // the file is refused as a typed one would be.
            let text = if value.is_number() {
                format_exact_real(value.get::<f64>()?)
            } else {
                value.get::<String>()?
            };
            match parse_parameter(object.kind, &key, &text) {
                Ok(parsed) => {
                    object.parameters.insert(key.into_owned(), parsed);
                }
                Err(error) => return bad(format!("object {}: {}", object.id, error.message)),
            }
        }
    }
    Ok(object)
}

/// The set as the drawing keeps it, indented by two: {"format":
/// "katana-road-objects", "version": 1, "next": n, "objects": [...]}. The C++
/// fails with InvalidArgument for text that is not UTF-8 (typed on a console
/// in another code page), which a String cannot hold.
pub fn road_objects_to_json(set: &RoadObjectSet) -> Result<String> {
    let mut root = Json::object_of([
        ("format", Json::from(FORMAT)),
        ("version", Json::from(ROAD_OBJECTS_VERSION)),
        ("next", Json::from(set.next_number)),
    ]);
    root["objects"] = Json::array_of(set.objects.iter().map(object_json));
    Ok(root.dump_with(2, ' ', false))
}

fn read_objects(text: &str) -> Read<RoadObjectSet> {
    let root = Json::parse(text)?;
    if root.value("format", String::new())? != FORMAT {
        return Err(Thrown::Refused(Error::new(
            ErrorCode::ParseFailure,
            "not a set of road objects",
        )));
    }
    let version = root.at("version")?.get::<i32>()?;
    if version > ROAD_OBJECTS_VERSION {
        return Err(Thrown::Refused(make_error(
            ErrorCode::Unsupported,
            "the road objects were written by a newer version of Katana",
            format!("version {version}"),
        )));
    }
    let mut set = RoadObjectSet {
        next_number: root.value("next", 1_u64)?,
        ..RoadObjectSet::default()
    };
    let mut ids: BTreeSet<String> = BTreeSet::new();
    // `value("objects", json::array())`: the member, or no objects.
    let empty = Json::array();
    for object in root.find("objects").unwrap_or(&empty).iter() {
        let read = object_from(object)?;
        if !ids.insert(read.id.to_ascii_lowercase()) {
            return bad(format!("two objects are called {}", read.id));
        }
        set.objects.push(read);
    }
    if set.next_number < 1 {
        return bad("the next number is 1 or more");
    }
    Ok(set)
}

/// The set read back. ParseFailure for text that is not one, naming what;
/// Unsupported for a newer version.
pub fn road_objects_from_json(text: &str) -> Result<RoadObjectSet> {
    match read_objects(text) {
        Ok(set) => Ok(set),
        Err(Thrown::Refused(error)) => Err(error),
        Err(Thrown::What(what)) => Err(make_error(
            ErrorCode::ParseFailure,
            "the road objects are not in the expected format",
            what,
        )),
    }
}
