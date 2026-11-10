//! The records of the GIS verbs - IMPORT, EXPORT, INFO, REFS, COPC
//! (docs/interop.md, "Replies") - that more than one verb writes, and the
//! reading of any record as JSON for the MCP tools that hand them to an agent
//! as structured content: the port of src/katana_app/geo/gis_records.hpp/.cpp.
//!
//! ```text
//! reference id=1 kind=raster name=terrain width=120 height=90 ...
//! reference id=2 kind=pointcloud name=scan points=40000 source_points=40000 ...
//! surface name=ground triangles=1200 points=640 bounds=... zmin=... zmax=... source=...
//! ```
//!
//! One named surface's record is cad's surface_record
//! (cad/terrain/surface_verbs.rs). A reference layer's record, for a raster
//! and for a point cloud, joins this module with interop's reference data
//! (step 6, wave 2).

use katana_core::json::Json;
use katana_core::text::{parse_finite_double, parse_integer};

use super::replies::{Record, joined, parse_records};

/// Two or more finite numbers joined by commas; None for anything else, one
/// number included (that is a number, not a list).
fn number_list(text: &str) -> Option<Vec<f64>> {
    if !text.contains(',') {
        return None;
    }
    text.split(',').map(parse_finite_double).collect()
}

/// The fields that are words whatever they hold.
const WORDS: [&str; 21] = [
    "file",
    "name",
    "text",
    "crs",
    "format",
    "driver",
    "source",
    "kind",
    "element",
    "layer",
    "color",
    "role",
    "encoding",
    "version",
    "member",
    "path",
    "display",
    "derivation",
    "placement",
    "reason",
    "state",
];

/// A record as a JSON object: {"record": kind, field: value ...}. The kind is
/// every word before the first key= ("ifc exported"). A field is a number
/// when its text is one, true or false for yes and no, an array of numbers
/// for a comma list of them (bounds, always four or null; a scope's area; a
/// cell of 4,3) - except the fields that are always words (a file, a name, a
/// sentence), which stay strings whatever they hold: a layer called 12 is a
/// name.
pub fn record_json(record: &Record) -> Json {
    let mut object = Json::object_of([("record", Json::from(&record.kind))]);
    for (key, text) in &record.fields {
        let key = key.as_str();
        if WORDS.contains(&key) {
            object[key] = Json::from(text);
            continue;
        }
        // A list of numbers - bounds and a scope's area x0,y0,x1,y1, a cell
        // of 4,3, classes 2,6 - is an array of them, so an agent reads a
        // cell's width without splitting a string. bounds is always four, or
        // null.
        if let Some(numbers) = number_list(text) {
            object[key] = if key == "bounds" && numbers.len() != 4 {
                Json::Null
            } else {
                Json::array_of(numbers.into_iter().map(Json::from))
            };
            continue;
        }
        if key == "bounds" {
            object[key] = Json::Null;
            continue;
        }
        object[key] = if text == "yes" || text == "no" {
            Json::from(text == "yes")
        } else if let Some(whole) = parse_integer(text) {
            Json::from(whole)
        } else if let Some(real) = parse_finite_double(text) {
            Json::from(real)
        } else if text.is_empty() {
            Json::Null // absent, not an empty word
        } else {
            Json::from(text)
        };
    }
    if !record.body.is_empty() {
        object["body"] = Json::from(joined(&record.body, "\n"));
    }
    object
}

/// Every record of a reply, as record_json makes each.
pub fn records_json(reply: &str) -> Json {
    Json::array_of(
        parse_records(reply)
            .iter()
            .filter(|record| !record.kind.is_empty())
            .map(record_json),
    )
}
