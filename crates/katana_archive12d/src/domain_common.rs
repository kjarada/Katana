//! The pieces import and export share, exposed for testing (domain.rs).

use katana_entity::layer_path::{
    LAYER_SEPARATOR, MAXIMUM_LAYER_DEPTH, MAXIMUM_LAYER_NAME_LENGTH, join_layer_path, layer_depth,
    validate_layer_path,
};
use katana_entity::tables::built_in_symbol_for;
use katana_geometry::Point2;

use crate::archive::{Segment, Vertex};
use crate::plan_geometry::{ChordReport, chord_plan};
use crate::text_utilities::trimmed;

// What a model name that leaves no valid layer path becomes: the format's own
// name, as the C++ has it, and the value its tests hold.
const FALLBACK_LAYER: &str = "12d";

/// The Katana symbol a point linestyle is best drawn with, from its name:
/// "ELEC Pole - Power" is a pole, "DRAIN Gully Pit Point" a manhole, "STNS
/// Default MX Survey Mark" a target, "TOPO Natural Surface Point" a cross. An
/// archive carries only the NAME of a linestyle - what it looks like lives in
/// the writing program's project - so this is a reading of the name, made
/// once when the style is created and changed in the style manager if it is
/// wrong. Never empty: a name that says nothing is a circle.
pub fn symbol_for_linestyle(name: &str) -> &'static str {
    built_in_symbol_for(name)
}

/// An archive model name as a valid Katana layer path. Tree separators are
/// kept; what a layer path cannot hold (an empty level, "." or "..", a
/// control character, a backslash) is replaced, and the result always passes
/// validate_layer_path. An empty name becomes "12d".
pub fn layer_path_for_model(model_name: &str, prefix: &str) -> String {
    let mut segments = Vec::new();
    for piece in model_name.split(LAYER_SEPARATOR) {
        // A backslash is how a Windows user types a tree; as a layer name it
        // would read as a path separator wherever the name reaches a file
        // system.
        let mut segment: String = trimmed(piece)
            .chars()
            .map(|character| {
                if (character as u32) < 0x20 || character == '\u{7f}' || character == '\\' {
                    '_'
                } else {
                    character
                }
            })
            .collect();
        if segment == "." || segment == ".." {
            segment = String::from("_");
        }
        if !segment.is_empty() {
            segments.push(segment);
        }
    }

    let mut path = String::from(prefix);
    let prefix_depth = if path.is_empty() {
        0
    } else {
        layer_depth(&path)
    };
    for (i, segment) in segments.iter().enumerate() {
        // Levels past the depth limit are folded into the last one that fits,
        // so a deep tree loses nesting rather than the names in it.
        let room = prefix_depth + i < MAXIMUM_LAYER_DEPTH;
        if room || path.is_empty() {
            path = join_layer_path(&path, segment);
        } else {
            path.push('_');
            path.push_str(segment);
        }
    }
    if path.is_empty() {
        path = String::from(FALLBACK_LAYER);
    }
    if path.len() > MAXIMUM_LAYER_NAME_LENGTH {
        let mut bytes = path.into_bytes();
        bytes.truncate(MAXIMUM_LAYER_NAME_LENGTH);
        // Cutting mid-way through a multi-byte character would leave invalid
        // UTF-8: find the last lead byte and drop the character if the cut
        // took any of its continuation bytes.
        let mut lead = bytes.len();
        while lead > 0 && (bytes[lead - 1] & 0xC0) == 0x80 {
            lead -= 1;
        }
        if lead > 0 && bytes[lead - 1] >= 0xC0 {
            let byte = bytes[lead - 1];
            let continuation = if byte >= 0xF0 {
                3
            } else if byte >= 0xE0 {
                2
            } else {
                1
            };
            if bytes.len() - lead < continuation {
                bytes.truncate(lead - 1);
            }
        }
        // ... and cutting at a separator or a blank would leave an empty
        // level.
        while matches!(bytes.last(), Some(b'/' | b' ')) {
            bytes.pop();
        }
        // The C++ hands the bytes to validateLayerPath, which refuses any
        // that are not UTF-8; the cut above leaves none.
        let Ok(text) = String::from_utf8(bytes) else {
            return String::from(FALLBACK_LAYER);
        };
        path = text;
    }
    if validate_layer_path(&path).is_err() {
        return String::from(FALLBACK_LAYER);
    }
    path
}

/// The vertices of a string as a plan polyline, arcs and transitions chorded
/// to `tolerance`. Empty when the string has no vertices.
pub fn chorded_plan(
    vertices: &[Vertex],
    segments: &[Segment],
    closed: bool,
    tolerance: f64,
) -> Vec<Point2> {
    let mut report = ChordReport::default();
    chord_plan(vertices, segments, closed, tolerance, &mut report)
        .into_iter()
        .map(|point| point.point)
        .collect()
}
