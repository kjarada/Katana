// What the archive tests share: gtest's comparisons, the C++ tests' helpers
// (`read`, `import`, `only<T>`, the warnings searches), the fixtures under
// tests/archive12d/data, and the std::get of the variants they inspect.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use std::path::PathBuf;

use katana_archive12d::{
    ArcString, Archive, Attribute, AttributeList, AttributeValue, CircleString, DomainImport,
    DraftingLeader, DrainageString, Element, ImportOptions, LasCloud, PlotFrame, ReadOptions,
    SuperAlignment, SuperTin, TextString, Tin, Trimesh, Vertex, VertexString, read_archive,
    to_domain,
};
use katana_entity::{
    Entity, Geometry, LeaderGeometry, PointGeometry, PropertyMap, PropertyValue, TextGeometry,
};
use katana_geometry::{Arc2, Circle2, Polyline2, Segment2};

/// EXPECT_NEAR(actual, expected, epsilon).
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// KATANA_ARCHIVE12D_TEST_DATA: the hand-written archives the C++ reads.
pub fn data_path(name: &str) -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/archive12d/data"
    ))
    .join(name)
}

pub fn fixture(name: &str) -> Vec<u8> {
    let path = data_path(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The C++ tests' `read`: the text read with the default options, which must
/// succeed.
pub fn read(text: &str) -> Archive {
    match read_archive(text, &ReadOptions::default()) {
        Ok(archive) => archive,
        Err(error) => panic!("{}", error.describe()),
    }
}

// These fixtures are hand-written, and a hand-written .12da inherits the
// format's CURRENT BREAKLINE TYPE, whose default is `point` (commands,
// 1.4.4). A fixture that means a line has to say so, exactly as every string
// in a real archive does - the writing program puts the flag on all 25,659
// strings of a production file and leaves nothing to the default. Rather
// than repeat it in every fixture, the helper states it once at file level,
// which is the format's own way of saying it; a fixture that wants POINTS
// says `breakline point` inside the string and overrides this.
pub const BREAKLINE_LINE: &str = "breakline line\n";

/// The C++ tests' `import`: BREAKLINE_LINE and the text, read and imported.
pub fn import_with(text: &str, options: &ImportOptions) -> DomainImport {
    let archive = read(&format!("{BREAKLINE_LINE}{text}"));
    match to_domain(&archive, options) {
        Ok(domain) => domain,
        Err(error) => panic!("{}", error.describe()),
    }
}

pub fn import(text: &str) -> DomainImport {
    import_with(text, &ImportOptions::default())
}

pub fn any_contains(list: &[String], needle: &str) -> bool {
    list.iter().any(|item| item.contains(needle))
}

pub fn joined(list: &[String]) -> String {
    let mut out = String::new();
    for item in list {
        out.push_str("\n  ");
        out.push_str(item);
    }
    out
}

pub fn any_warning_contains(domain: &DomainImport, needle: &str) -> bool {
    any_contains(&domain.warnings, needle)
}

pub fn all_warnings(domain: &DomainImport) -> String {
    joined(&domain.warnings)
}

/// A vertex, as the C++ writes `Vertex{x, y, z}`.
pub fn at(x: f64, y: f64, z: Option<f64>) -> Vertex {
    Vertex { x, y, z }
}

pub fn attribute(name: &str, value: AttributeValue) -> Attribute {
    Attribute {
        name: String::from(name),
        value,
        declared_type: String::new(),
    }
}

pub fn text_attribute(name: &str, value: &str) -> Attribute {
    attribute(name, AttributeValue::Text(String::from(value)))
}

// std::get on an attribute's value.
pub fn attribute_text(attribute: &Attribute) -> &str {
    match &attribute.value {
        AttributeValue::Text(text) => text,
        other => panic!("{}: {other:?} is not text", attribute.name),
    }
}

pub fn attribute_real(attribute: &Attribute) -> f64 {
    match &attribute.value {
        AttributeValue::Real(real) => *real,
        other => panic!("{}: {other:?} is not real", attribute.name),
    }
}

pub fn attribute_integer(attribute: &Attribute) -> i64 {
    match &attribute.value {
        AttributeValue::Integer(integer) => *integer,
        other => panic!("{}: {other:?} is not an integer", attribute.name),
    }
}

pub fn attribute_group(attribute: &Attribute) -> &AttributeList {
    match &attribute.value {
        AttributeValue::Group(group) => group,
        other => panic!("{}: {other:?} is not a group", attribute.name),
    }
}

// std::get on a property's value; the key must be there.
pub fn prop_real(map: &PropertyMap, key: &str) -> f64 {
    match map.get(key) {
        Some(PropertyValue::Real(real)) => *real,
        other => panic!("{key}: {other:?} is not a real"),
    }
}

pub fn prop_integer(map: &PropertyMap, key: &str) -> i64 {
    match map.get(key) {
        Some(PropertyValue::Integer(integer)) => *integer,
        other => panic!("{key}: {other:?} is not an integer"),
    }
}

pub fn prop_text<'m>(map: &'m PropertyMap, key: &str) -> &'m str {
    match map.get(key) {
        Some(PropertyValue::Text(text)) => text,
        other => panic!("{key}: {other:?} is not text"),
    }
}

/// The C++ tests' `meta`: the text metadata under `key`, None when it is not
/// text or not there.
pub fn meta<'e>(entity: &'e Entity, key: &str) -> Option<&'e str> {
    match entity.metadata.get(key) {
        Some(PropertyValue::Text(text)) => Some(text),
        _ => None,
    }
}

/// std::get_if on an archive element.
pub trait Kind: Sized {
    fn of(element: &Element) -> Option<&Self>;
}

macro_rules! kind {
    ($($variant:ident),+ $(,)?) => {
        $(impl Kind for $variant {
            fn of(element: &Element) -> Option<&Self> {
                match element {
                    Element::$variant(value) => Some(value),
                    _ => None,
                }
            }
        })+
    };
}

kind!(
    VertexString,
    ArcString,
    CircleString,
    TextString,
    PlotFrame,
    DrainageString,
    SuperAlignment,
    LasCloud,
    Tin,
    SuperTin,
    Trimesh,
    DraftingLeader,
);

/// std::get: the element, which must be a T.
pub fn get<T: Kind>(element: &Element) -> &T {
    T::of(element).expect("the element is of another kind")
}

/// The C++ tests' `only<T>`: the archive's one element, which must be a T.
pub fn only<T: Kind>(archive: &Archive) -> &T {
    assert_eq!(archive.elements.len(), 1);
    get(&archive.elements[0])
}

/// std::get_if on an entity's geometry.
pub trait Shape: Sized {
    fn of(geometry: &Geometry) -> Option<&Self>;
}

macro_rules! shape {
    ($($variant:ident($kind:ty)),+ $(,)?) => {
        $(impl Shape for $kind {
            fn of(geometry: &Geometry) -> Option<&Self> {
                match geometry {
                    Geometry::$variant(value) => Some(value),
                    _ => None,
                }
            }
        })+
    };
}

shape!(
    Point(PointGeometry),
    Line(Segment2),
    Arc(Arc2),
    Polyline(Polyline2),
    Circle(Circle2),
    Text(TextGeometry),
    Leader(LeaderGeometry),
);

/// std::get: the geometry, which must be a T.
pub fn shape<T: Shape>(geometry: &Geometry) -> &T {
    T::of(geometry).expect("the geometry is of another kind")
}

/// std::holds_alternative.
pub fn holds<T: Shape>(geometry: &Geometry) -> bool {
    T::of(geometry).is_some()
}
