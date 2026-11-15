// The drawing of a graded utility schedule (utility_drawing.rs), built with no
// document: what layers, linetypes and entities UTILITY DRAW would add
// (tests/cad/utilities/test_utility_drawing.cpp).
//
// The sample is samples/utilities/schedule.csv, graded by hand in
// docs/subsurface_utilities.md and by the report the cli.utility_* tests pin:
// W1's segments grade QL-B, QL-B, QL-B, QL-A, QL-C - three detected, the trench
// between the potholes, then 12.5 m interpolated past the detected spacing - so
// W1 is three polylines, and a polyline ends exactly where the level changes.

use std::collections::BTreeSet;

use katana_cad::PlotSettings;
use katana_cad::plot::paper_colour;
use katana_cad::utilities::utility_drawing::{
    UTILITIES_DRAWN_RECORD, UtilityDrawOptions, UtilityDrawing, draw_utilities,
    format_utility_drawing, keys, points_layer_name, quality_level_layer_name,
    quality_level_linetype, quality_level_linetype_name, utility_type_colour, utility_type_word,
};
use katana_core::ErrorCode;
use katana_entity::entity::property_to_string;
use katana_entity::layer_path::is_layer_under;
use katana_entity::{Color, Entity, EntityType, Geometry, Layer, PropertyValue, validate};
use katana_geometry::{Point2, Polyline2};
use katana_math::cpp;
use katana_survey::subsurface::{self as sub, QualityLevel, UtilityLine, UtilityType};

use crate::{double_eq, near};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../samples/utilities");
const UTILITY_DOC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/subsurface_utilities.md"
);

fn sample_text(name: &str) -> String {
    let path = format!("{SAMPLES}/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn parsed(text: &str) -> Vec<UtilityLine> {
    sub::parse_utility_csv(text).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn sample() -> Vec<UtilityLine> {
    parsed(&sample_text("schedule.csv"))
}

fn drawn(lines: &[UtilityLine], options: &UtilityDrawOptions) -> UtilityDrawing {
    draw_utilities(lines, options).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn defaults() -> UtilityDrawOptions {
    UtilityDrawOptions::default()
}

fn text(entity: &Entity, key: &str) -> String {
    entity
        .properties
        .get(key)
        .map_or_else(|| String::from("(absent)"), property_to_string)
}

#[track_caller]
fn real(entity: &Entity, key: &str) -> f64 {
    match entity.properties.get(key) {
        Some(PropertyValue::Real(value)) => *value,
        _ => panic!("{key} is not a real"),
    }
}

fn has(entity: &Entity, key: &str) -> bool {
    entity.properties.contains_key(key)
}

// The entities of one service of one kind, in the order they were built.
fn of(drawing: &UtilityDrawing, line: &str, r#type: EntityType) -> Vec<Entity> {
    drawing
        .entities
        .iter()
        .filter(|entity| entity.r#type() == r#type && text(entity, keys::LINE) == line)
        .cloned()
        .collect()
}

#[track_caller]
fn point<'a>(drawing: &'a UtilityDrawing, vertex: &str) -> &'a Entity {
    drawing
        .entities
        .iter()
        .find(|entity| entity.r#type() == EntityType::Point && text(entity, keys::VERTEX) == vertex)
        .unwrap_or_else(|| panic!("no point {vertex}"))
}

fn layer_names(drawing: &UtilityDrawing) -> Vec<String> {
    drawing
        .layers
        .iter()
        .map(|layer| layer.name.clone())
        .collect()
}

#[track_caller]
fn layer_of<'a>(drawing: &'a UtilityDrawing, name: &str) -> &'a Layer {
    drawing
        .layers
        .iter()
        .find(|layer| layer.name == name)
        .unwrap_or_else(|| panic!("no layer {name}"))
}

#[track_caller]
fn polyline(entity: &Entity) -> &Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline,
        _ => panic!("not a polyline"),
    }
}

fn vertex_count(entity: &Entity) -> usize {
    polyline(entity).vertices.len()
}

#[track_caller]
fn position(entity: &Entity) -> Point2 {
    match &entity.geometry {
        Geometry::Point(point) => point.position,
        _ => panic!("not a point"),
    }
}

// The WCAG contrast of two colours: 1 (none) to 21 (black on white). 3 is
// what a line or a symbol needs to be seen.
fn luminance(colour: Color) -> f64 {
    let linear = |channel: u8| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(colour.r) + 0.7152 * linear(colour.g) + 0.0722 * linear(colour.b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let la = luminance(a);
    let lb = luminance(b);
    (cpp::max(la, lb) + 0.05) / (cpp::min(la, lb) + 0.05)
}

fn every_type() -> Vec<UtilityType> {
    vec![
        UtilityType::Unknown,
        UtilityType::Electricity,
        UtilityType::Telecommunications,
        UtilityType::Gas,
        UtilityType::Water,
        UtilityType::RecycledWater,
        UtilityType::FireService,
        UtilityType::Sewer,
        UtilityType::Stormwater,
        UtilityType::Fuel,
        UtilityType::IntelligentTransport,
        UtilityType::Other,
    ]
}

// A service located across a pothole whose two records share one place: the
// trench between them is QL-A and has no length.
const SHARED_PLACE: &str = concat!(
    "line,point,easting,northing,method,level,level_ref,surface,h_unc,v_unc,path,type\n",
    "S1,S1-1,100.0,200.0,EML,,,,0.10,,,sewer\n",
    "S1,S1-2,105.0,200.0,pothole,19.0,top,20.0,0.02,0.02,exposed,\n",
    "S1,S1-3,105.0,200.0,pothole,19.0,top,20.0,0.02,0.02,,\n",
    "S1,S1-4,110.0,200.0,EML,,,,0.10,,,\n",
);

// The same pothole with its two records a rounding error apart, as a
// reprojected export leaves them: 10 nm, a length, but under the model's
// tolerance for one.
const NEARLY_ONE_PLACE: &str = concat!(
    "line,point,easting,northing,method,level,level_ref,surface,h_unc,v_unc,path,type\n",
    "S1,S1-1,100.0,200.0,EML,,,,0.10,,,sewer\n",
    "S1,S1-2,105.0,200.0,pothole,19.0,top,20.0,0.02,0.02,exposed,\n",
    "S1,S1-3,105.00000001,200.0,pothole,19.0,top,20.0,0.02,0.02,,\n",
    "S1,S1-4,110.0,200.0,EML,,,,0.10,,,\n",
);

// A colour's place on the hue wheel, as the textbook HSV model puts it: hue in
// degrees (red 0, yellow 60, green 120, blue 240), saturation and value 0 to 1.
// Independent of the program: it is what says "this sRGB is orange".
struct Hsv {
    hue: f64,
    saturation: f64,
    value: f64,
}

fn hsv(colour: Color) -> Hsv {
    let r = f64::from(colour.r) / 255.0;
    let g = f64::from(colour.g) / 255.0;
    let b = f64::from(colour.b) / 255.0;
    let high = cpp::max3(r, g, b);
    let span = high - cpp::min3(r, g, b);
    let mut hue = 0.0;
    if span > 0.0 {
        if high == r {
            hue = 60.0 * ((g - b) / span % 6.0);
        } else if high == g {
            hue = 60.0 * ((b - r) / span + 2.0);
        } else {
            hue = 60.0 * ((r - g) / span + 4.0);
        }
        if hue < 0.0 {
            hue += 360.0;
        }
    }
    Hsv {
        hue,
        saturation: if high == 0.0 { 0.0 } else { span / high },
        value: high,
    }
}

// The region of hue, saturation and value a colour name covers, for the ten
// colours AS 5488 names. The bands are wide enough that any plain example of
// the colour is in one, and none overlaps another (orange and brown differ in
// value, yellow and cream in saturation), so a value that is in a band is that
// colour and no other.
struct Band {
    name: &'static str,
    hue_low: f64,
    // A band across red is written with low > high: 352 to 8.
    hue_high: f64,
    saturation_low: f64,
    saturation_high: f64,
    value_low: f64,
    value_high: f64,
}

const fn band(
    name: &'static str,
    hue_low: f64,
    hue_high: f64,
    saturation_low: f64,
    saturation_high: f64,
    value_low: f64,
    value_high: f64,
) -> Band {
    Band {
        name,
        hue_low,
        hue_high,
        saturation_low,
        saturation_high,
        value_low,
        value_high,
    }
}

const BANDS: [Band; 10] = [
    band("blue", 200.0, 250.0, 0.5, 1.0, 0.5, 1.0),
    band("orange", 20.0, 40.0, 0.9, 1.0, 0.8, 1.0),
    band("white", 0.0, 360.0, 0.0, 0.02, 0.98, 1.0),
    band("yellow", 45.0, 60.0, 0.9, 1.0, 0.95, 1.0),
    band("purple", 260.0, 290.0, 0.5, 1.0, 0.5, 1.0),
    band("red", 352.0, 8.0, 0.7, 1.0, 0.8, 1.0),
    band("cream", 40.0, 55.0, 0.15, 0.35, 0.9, 1.0),
    band("green", 100.0, 140.0, 0.9, 1.0, 0.6, 1.0),
    band("brown", 20.0, 40.0, 0.5, 0.9, 0.3, 0.7),
    band("pink", 320.0, 345.0, 0.5, 0.8, 0.9, 1.0),
];

fn in_band(band: &Band, colour: Color) -> bool {
    let c = hsv(colour);
    let hue = if band.hue_low <= band.hue_high {
        c.hue >= band.hue_low && c.hue <= band.hue_high
    } else {
        c.hue >= band.hue_low || c.hue <= band.hue_high
    };
    hue && c.saturation >= band.saturation_low
        && c.saturation <= band.saturation_high
        && c.value >= band.value_low
        && c.value <= band.value_high
}

mod utility_drawing {
    use super::*;

    #[test]
    fn the_sample_is_drawn_as_runs_and_points() {
        let drawing = drawn(&sample(), &defaults());
        assert_eq!(drawing.lines.len(), 4);
        assert_eq!(drawing.vertices, 14);
        assert_eq!(drawing.segments, 10);
        // W1 three runs, E1 two, T1 and G1 one each; a point per vertex.
        assert_eq!(drawing.entities.len(), 7 + 14);
        assert_eq!(drawing.drawn_layers, 11);
        double_eq(drawing.bounds.min.x, 334000.0);
        double_eq(drawing.bounds.min.y, 6250000.0);
        double_eq(drawing.bounds.max.x, 334040.0);
        double_eq(drawing.bounds.max.y, 6250007.2);
        assert_eq!(
            format_utility_drawing(&drawing, UTILITIES_DRAWN_RECORD),
            concat!(
                "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 ",
                "bounds=334000.000,6250000.000,334040.000,6250007.200\n",
                "line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 ",
                "ql_d=0.000\n",
                "line id=E1 type=electricity length=27.001 ql_a=0.000 ql_b=9.001 ql_c=18.001 ",
                "ql_d=0.000\n",
                "line id=T1 type=telecommunications length=33.000 ql_a=0.000 ql_b=0.000 ",
                "ql_c=33.000 ql_d=0.000\n",
                "line id=G1 type=gas length=40.000 ql_a=0.000 ql_b=0.000 ql_c=0.000 ",
                "ql_d=40.000"
            )
        );
    }

    #[test]
    fn a_polyline_ends_exactly_where_the_graded_level_changes() {
        let drawing = drawn(&sample(), &defaults());
        let runs = of(&drawing, "W1", EntityType::Polyline);
        assert_eq!(runs.len(), 3);
        assert_eq!(text(&runs[0], keys::QUALITY_LEVEL), "QL-B");
        assert_eq!(text(&runs[0], keys::FROM), "W1-1");
        assert_eq!(text(&runs[0], keys::TO), "W1-X1");
        assert_eq!(vertex_count(&runs[0]), 4);
        assert_eq!(runs[0].layer, "utilities/water/QL-B");
        assert_eq!(text(&runs[1], keys::QUALITY_LEVEL), "QL-A");
        assert_eq!(text(&runs[1], keys::FROM), "W1-X1");
        assert_eq!(text(&runs[1], keys::TO), "W1-4");
        assert_eq!(vertex_count(&runs[1]), 2);
        assert_eq!(runs[1].layer, "utilities/water/QL-A");
        assert_eq!(text(&runs[2], keys::QUALITY_LEVEL), "QL-C");
        assert_eq!(text(&runs[2], keys::FROM), "W1-4");
        assert_eq!(text(&runs[2], keys::TO), "W1-5");
        assert_eq!(runs[2].layer, "utilities/water/QL-C");
        // The runs meet: each starts where the one before it ended.
        let first = polyline(&runs[0]);
        let second = polyline(&runs[1]);
        assert_eq!(first.vertices.last(), second.vertices.first());
        double_eq(second.vertices[0].x, 334016.080);
        double_eq(second.vertices[0].y, 6250000.310);
        // E1's weak radar pick makes its last two segments one QL-C run.
        let duct = of(&drawing, "E1", EntityType::Polyline);
        assert_eq!(duct.len(), 2);
        assert_eq!(text(&duct[1], keys::QUALITY_LEVEL), "QL-C");
        assert_eq!(vertex_count(&duct[1]), 3);
    }

    #[test]
    fn the_layers_are_by_type_and_level_parents_first() {
        let drawing = drawn(&sample(), &defaults());
        let expected = crate::strings(&[
            "utilities",
            "utilities/electricity",
            "utilities/electricity/QL-B",
            "utilities/electricity/QL-C",
            "utilities/electricity/points",
            "utilities/gas",
            "utilities/gas/QL-D",
            "utilities/gas/points",
            "utilities/telecommunications",
            "utilities/telecommunications/QL-C",
            "utilities/telecommunications/points",
            "utilities/water",
            "utilities/water/QL-A",
            "utilities/water/QL-B",
            "utilities/water/QL-C",
            "utilities/water/points",
        ]);
        assert_eq!(layer_names(&drawing), expected);
        // Colour by type, linetype by level; the group of a type in its colour.
        let water = utility_type_colour(UtilityType::Water);
        assert_eq!(layer_of(&drawing, "utilities/water").color, water);
        assert_eq!(layer_of(&drawing, "utilities/water/QL-A").color, water);
        assert_eq!(
            layer_of(&drawing, "utilities/water/QL-A").linetype,
            "continuous"
        );
        assert_eq!(
            layer_of(&drawing, "utilities/water/QL-B").linetype,
            "utility-ql-b"
        );
        assert_eq!(
            layer_of(&drawing, "utilities/water/QL-C").linetype,
            "utility-ql-c"
        );
        assert_eq!(
            layer_of(&drawing, "utilities/gas/QL-D").linetype,
            "utility-ql-d"
        );
        assert_eq!(
            layer_of(&drawing, "utilities/gas/QL-D").color,
            utility_type_colour(UtilityType::Gas)
        );
        assert_eq!(
            layer_of(&drawing, "utilities/water/points").linetype,
            "continuous"
        );
        assert_eq!(
            layer_of(&drawing, "utilities").color,
            Layer::default().color
        );
        // Only the levels drawn get a linetype, and QL-A needs none of its own.
        assert_eq!(drawing.linetypes.len(), 3);
        assert_eq!(drawing.linetypes[0].name, "utility-ql-b");
        assert_eq!(drawing.linetypes[1].name, "utility-ql-c");
        assert_eq!(drawing.linetypes[2].name, "utility-ql-d");
        // The entities are ByLayer: the layer is where a person changes them.
        for entity in &drawing.entities {
            assert!(entity.color.is_none());
            assert!(entity.style.is_empty());
        }
    }

    #[test]
    fn each_quality_level_has_a_distinct_linetype_in_model_metres() {
        assert_eq!(quality_level_linetype_name(QualityLevel::A), "continuous");
        assert!(quality_level_linetype(QualityLevel::A).is_continuous());
        // Distinct as the C++'s set of the element lengths: by value, here
        // through their bits, since no length is a NaN or a negative zero.
        let mut patterns: BTreeSet<Vec<u64>> = BTreeSet::new();
        for level in [QualityLevel::B, QualityLevel::C, QualityLevel::D] {
            let linetype = quality_level_linetype(level);
            assert_eq!(linetype.name, quality_level_linetype_name(level));
            assert!(validate(&linetype).is_ok(), "{}", linetype.name);
            assert!(!linetype.is_continuous());
            // A period a person can read at 1:200 to 1:500: between 1 and 5 mm
            // of paper at 1:500 (0.5 to 2.5 m), and so 2.5 to 12.5 mm at 1:200.
            assert!(linetype.pattern_length() >= 0.5, "{}", linetype.name);
            assert!(linetype.pattern_length() <= 2.5, "{}", linetype.name);
            patterns.insert(
                linetype
                    .pattern
                    .iter()
                    .map(|element| element.length.to_bits())
                    .collect(),
            );
        }
        assert_eq!(patterns.len(), 3);
        // Dashed, then with a dot, then only dots, as the evidence weakens.
        let dots = |level: QualityLevel| {
            quality_level_linetype(level)
                .pattern
                .iter()
                .filter(|element| element.is_dot())
                .count()
        };
        assert_eq!(dots(QualityLevel::B), 0);
        assert_eq!(dots(QualityLevel::C), 1);
        assert!(quality_level_linetype(QualityLevel::D).pattern[0].is_dot());
    }

    #[test]
    fn the_colour_table_is_as5488s_by_type_and_each_value_is_the_colour_its_name_says() {
        // The ten types AS 5488 names a colour for, with the name its Appendix
        // B table gives, as the published guides state it
        // (docs/subsurface_utilities.md, "Colours"). Written here, apart from
        // the table, so the table is held to it.
        let as5488 = [
            (UtilityType::Water, "blue"),
            (UtilityType::Electricity, "orange"),
            (UtilityType::Telecommunications, "white"),
            (UtilityType::Gas, "yellow"),
            (UtilityType::RecycledWater, "purple"),
            (UtilityType::FireService, "red"),
            (UtilityType::Sewer, "cream"),
            (UtilityType::Stormwater, "green"),
            (UtilityType::Fuel, "brown"),
            (UtilityType::Unknown, "pink"),
        ];
        assert_eq!(as5488.len(), 10);
        let mut named: BTreeSet<UtilityType> = BTreeSet::new();
        for (r#type, name) in as5488 {
            named.insert(r#type);
            let colour = utility_type_colour(r#type);
            let reads: Vec<&str> = BANDS
                .iter()
                .filter(|band| in_band(band, colour))
                .map(|band| band.name)
                .collect();
            // It reads as the colour it is named, and as no other of the ten.
            assert_eq!(
                reads,
                [name],
                "{} {}",
                utility_type_word(r#type),
                colour.to_hex()
            );
        }
        assert_eq!(named.len(), 10, "each type is named once");
        // The other two kinds have no AS 5488 colour and are Katana's choice:
        // every kind there is that the ten above do not name. An eleventh named
        // fails here until it is placed on purpose; a thirteenth kind added to
        // the enum fails TheListOfEveryTypeHasEveryEnumeratorOfUtilityType first,
        // which is what keeps every_type() honest.
        let katanas_own: BTreeSet<UtilityType> = every_type()
            .into_iter()
            .filter(|r#type| !named.contains(r#type))
            .collect();
        assert_eq!(
            katanas_own,
            BTreeSet::from([UtilityType::IntelligentTransport, UtilityType::Other])
        );
        assert_eq!(every_type().len(), named.len() + katanas_own.len());
    }

    #[test]
    fn the_colour_table_is_the_documented_one_value_for_value() {
        // The twelve values of docs/subsurface_utilities.md, "Colours":
        // Katana's rendering of AS 5488's names (the sRGB is Katana's choice,
        // so these are pinned exactly rather than by the hue bands above).
        // Written here from that table, not read from the program, so a value
        // changed in the program alone, or in the document alone, fails.
        let documented = [
            (UtilityType::Water, "#2F80ED"),
            (UtilityType::Electricity, "#E07000"),
            (UtilityType::Telecommunications, "#FFFFFF"),
            (UtilityType::Gas, "#FFD500"),
            (UtilityType::RecycledWater, "#9B59D0"),
            (UtilityType::FireService, "#E53935"),
            (UtilityType::Sewer, "#F0E2B0"),
            (UtilityType::Stormwater, "#00A800"),
            (UtilityType::Fuel, "#A36215"),
            (UtilityType::Unknown, "#F2559B"),
            (UtilityType::IntelligentTransport, "#0B9BA8"),
            (UtilityType::Other, "#949494"),
        ];
        assert_eq!(documented.len(), every_type().len());

        let text = std::fs::read_to_string(UTILITY_DOC)
            .unwrap_or_else(|error| panic!("no document at {UTILITY_DOC}: {error}"));
        let begin = text.find("### Colours").expect("a Colours section");
        // The section runs to the next heading of its level or above.
        let after = &text[begin + 1..];
        let end = [after.find("\n## "), after.find("\n### ")]
            .into_iter()
            .flatten()
            .min()
            .map(|end| end + begin + 1)
            .expect("a heading after the Colours section");
        let mut rows: Vec<String> = Vec::new();
        let mut at = begin;
        while at < end {
            let stop = text[at..]
                .find('\n')
                .map(|stop| stop + at)
                .filter(|&stop| stop <= end)
                .unwrap_or(end);
            let mut line = String::from(&text[at..stop]);
            while line.ends_with('\r') {
                line.pop();
            }
            rows.push(line);
            at = stop + 1;
        }

        for (r#type, hex) in documented {
            let word = utility_type_word(r#type);
            assert_eq!(utility_type_colour(r#type).to_hex(), hex, "{word}");
            // One row of the document's table is this type's, and shows this
            // value.
            let start = format!("| {word} |");
            let mine: Vec<&String> = rows.iter().filter(|row| row.starts_with(&start)).collect();
            assert_eq!(mine.len(), 1, "the document's table has one row for {word}");
            assert!(
                mine[0].contains(&format!("`{hex}`")),
                "{word} is {hex} but its row says: {}",
                mine[0]
            );
        }
    }

    #[test]
    fn the_list_of_every_type_has_every_enumerator_of_utility_type() {
        // every_type() is written by hand and the enum has no count, so the
        // enumerators are counted the way they are laid out: from zero, one
        // after another, ending at Other. A kind put in anywhere before Other
        // makes Other's value outrun the list.
        let types = every_type();
        let last = UtilityType::Other as i32;
        assert_eq!(
            types.len(),
            last as usize + 1,
            "a kind was added to UtilityType: place it in every_type() and in the colour table"
        );
        let values: BTreeSet<i32> = types.iter().map(|&r#type| r#type as i32).collect();
        assert_eq!(values.len(), types.len(), "no kind is listed twice");
        assert_eq!(values.first(), Some(&0));
        assert_eq!(values.last(), Some(&last));
        // The C++ goes on to cast Other + 1 to the enum and asks that it is
        // spelt "unknown", to catch a kind put after Other. A Rust enum cannot
        // hold a value it does not list, and utility_type_word and
        // to_string match every enumerator with no fallback, so such a kind
        // does not compile until it is spelt. What remains of the check is
        // the fallback's word itself, which the C++'s answer is.
        assert_eq!(sub::to_string(UtilityType::Unknown), "unknown");
        assert_eq!(utility_type_word(UtilityType::Unknown), "unknown");
    }

    #[test]
    fn on_paper_communications_prints_black_and_yellow_gas_and_cream_sewer_print_as_drawn() {
        // The plot's white rule turns white and near-white pens black and
        // nothing else (plot.rs), so AS 5488's white is legible on paper and
        // its yellow and cream are not darkened: they print light, which
        // docs/subsurface_utilities.md says. The day a plot rule darkens light
        // colours this test fails, and so do those words.
        let plot = PlotSettings::default();
        assert_eq!(
            paper_colour(utility_type_colour(UtilityType::Telecommunications), &plot),
            Color::new(0, 0, 0, 255)
        );
        assert_eq!(
            paper_colour(utility_type_colour(UtilityType::Gas), &plot),
            utility_type_colour(UtilityType::Gas)
        );
        assert_eq!(
            paper_colour(utility_type_colour(UtilityType::Sewer), &plot),
            utility_type_colour(UtilityType::Sewer)
        );
        let dark = PlotSettings {
            white_to_black: false,
            ..plot
        };
        assert_eq!(
            paper_colour(utility_type_colour(UtilityType::Telecommunications), &dark),
            utility_type_colour(UtilityType::Telecommunications),
            "it is the white rule that makes it black, not the colour"
        );
    }

    #[test]
    fn every_type_colour_reads_on_the_screen_and_on_paper_except_yellow_and_cream_which_print_light()
     {
        // The plan view's ground (the viewport widget's) and a colour plot on
        // white.
        let screen = Color::new(0x1E, 0x23, 0x29, 255);
        let paper = Color::new(255, 255, 255, 255);
        let plot = PlotSettings::default();
        // AS 5488's yellow gas and cream sewer cannot be 3:1 on white and still
        // be yellow and cream, so they keep the screen's 3:1 and print light.
        // Nothing else is excused, and neither is excused more than it needs.
        let print_light = BTreeSet::from([UtilityType::Gas, UtilityType::Sewer]);
        let mut words: BTreeSet<String> = BTreeSet::new();
        let mut colours: BTreeSet<String> = BTreeSet::new();
        for r#type in every_type() {
            let word = String::from(utility_type_word(r#type));
            assert!(!word.contains(' '), "{word}");
            let colour = utility_type_colour(r#type);
            colours.insert(colour.to_hex());
            assert!(
                contrast(colour, screen) >= 3.0,
                "{word} {}",
                colour.to_hex()
            );
            let on_paper = contrast(paper_colour(colour, &plot), paper);
            if print_light.contains(&r#type) {
                assert!(
                    on_paper < 3.0,
                    "{word} needs no exception: take it off the list"
                );
            } else {
                assert!(on_paper >= 3.0, "{word} {}", colour.to_hex());
            }
            words.insert(word);
        }
        // Worked by hand: yellow #FFD500 has luminance 0.2126 + 0.7152 *
        // 0.66539 = 0.68848, so 1.05 / 0.73848 = 1.4218 on white; cream
        // #F0E2B0 has 0.2126 * 0.87137 + 0.7152 * 0.76052 + 0.0722 * 0.43415 =
        // 0.76053, so 1.05 / 0.81053 = 1.2955.
        near(
            contrast(utility_type_colour(UtilityType::Gas), paper),
            1.4218,
            0.0005,
        );
        near(
            contrast(utility_type_colour(UtilityType::Sewer), paper),
            1.2955,
            0.0005,
        );
        assert_eq!(words.len(), every_type().len());
        assert_eq!(colours.len(), every_type().len());
        assert_eq!(
            utility_type_word(UtilityType::RecycledWater),
            "recycled-water"
        );
        assert_eq!(utility_type_word(UtilityType::FireService), "fire-service");
        assert_eq!(utility_type_word(UtilityType::IntelligentTransport), "its");
        assert_eq!(
            quality_level_layer_name("utilities", UtilityType::RecycledWater, QualityLevel::B),
            "utilities/recycled-water/QL-B"
        );
        assert_eq!(
            points_layer_name("utilities", UtilityType::Unknown),
            "utilities/unknown/points"
        );
    }

    #[test]
    fn the_runs_carry_the_service_and_why_they_are_limited() {
        let drawing = drawn(&sample(), &defaults());
        let runs = of(&drawing, "W1", EntityType::Polyline);
        assert_eq!(runs.len(), 3);
        for run in &runs {
            assert_eq!(text(run, keys::TYPE), "water");
            assert_eq!(text(run, keys::OWNER), "WaterCo");
            assert_eq!(text(run, keys::MATERIAL), "DICL");
            double_eq(real(run, keys::DIAMETER), 0.150);
            assert_eq!(text(run, keys::STATUS), "in service");
            assert_eq!(text(run, keys::DESCRIPTION), "trunk main");
        }
        assert_eq!(text(&runs[0], keys::LIMITED_BY), "the QL-B vertex W1-3");
        assert!(!has(&runs[1], keys::LIMITED_BY));
        assert!(
            text(&runs[2], keys::LIMITED_BY).contains("longer than the maximum detected spacing")
        );
        near(real(&runs[0], keys::LENGTH), 16.102, 0.0005);
        near(real(&runs[1], keys::LENGTH), 1.420, 0.0005);
        near(real(&runs[2], keys::LENGTH), 12.502, 0.0005);
        // Not recorded is not there: T1 has no material or size.
        let telco = of(&drawing, "T1", EntityType::Polyline);
        assert_eq!(telco.len(), 1);
        assert!(!has(&telco[0], keys::MATERIAL));
        assert!(!has(&telco[0], keys::DIAMETER));
        assert_eq!(text(&telco[0], keys::CONFIGURATION), "pit to pit");
        // Nor a status: a schedule with none gives no utility.status, where
        // "unknown" would read as if someone had recorded it.
        let unrecorded = of(
            &drawn(&parsed(SHARED_PLACE), &defaults()),
            "S1",
            EntityType::Polyline,
        );
        assert!(!unrecorded.is_empty());
        for run in &unrecorded {
            assert!(!has(run, keys::STATUS), "{}", text(run, keys::STATUS));
            assert_eq!(text(run, keys::TYPE), "sewer");
        }
    }

    #[test]
    fn the_points_carry_what_the_grading_found_at_each_vertex() {
        let options = UtilityDrawOptions {
            minimum_cover: Some(0.85),
            ..defaults()
        };
        let drawing = drawn(&sample(), &options);
        assert_eq!(of(&drawing, "E1", EntityType::Point).len(), 4);

        // The weak radar pick: claimed QL-B, graded QL-C, its level not
        // qualified.
        let pick = point(&drawing, "E1-3");
        assert_eq!(pick.layer, "utilities/electricity/points");
        assert_eq!(text(pick, keys::LINE), "E1");
        assert_eq!(text(pick, keys::METHOD), "ground penetrating radar");
        assert_eq!(text(pick, keys::QUALITY_LEVEL), "QL-C");
        assert_eq!(text(pick, keys::CLAIMED), "QL-B");
        assert!(has(pick, keys::OVER_CLAIM));
        double_eq(real(pick, keys::SERVICE_LEVEL), 19.40);
        assert_eq!(text(pick, keys::LEVEL_REFERENCE), "top");
        assert_eq!(text(pick, keys::LEVEL_QUALIFIED), "false");
        double_eq(real(pick, keys::SURFACE_LEVEL), 20.30);
        near(real(pick, keys::COVER), 0.900, 1e-9);
        assert!(has(pick, keys::COVER_NOTE));
        assert_eq!(text(pick, keys::COVER_BELOW_MINIMUM), "false");
        let at = position(pick);
        double_eq(at.x, 334018.0);
        double_eq(at.y, 6250003.1);

        // 0.800 of cover against 0.85 asked for.
        assert_eq!(
            text(point(&drawing, "E1-1"), keys::COVER_BELOW_MINIMUM),
            "true"
        );
        assert_eq!(
            text(point(&drawing, "W1-2"), keys::COVER_BELOW_MINIMUM),
            "false"
        );
        // A pothole that checks a detection says which.
        assert_eq!(text(point(&drawing, "W1-X1"), keys::VERIFIES), "W1-3");
        assert_eq!(text(point(&drawing, "W1-X1"), keys::QUALITY_LEVEL), "QL-A");
        assert_eq!(text(point(&drawing, "W1-X1"), keys::OVER_CLAIM), "(absent)");
        // No level: no service level, no cover, nothing below any minimum -
        // but the reason the cover is missing.
        let plain = point(&drawing, "W1-1");
        assert!(!has(plain, keys::SERVICE_LEVEL));
        assert!(!has(plain, keys::COVER));
        assert!(!has(plain, keys::COVER_BELOW_MINIMUM));
        assert!(!has(plain, keys::LEVEL_QUALIFIED));
        assert_eq!(
            text(plain, keys::COVER_NOTE),
            "no level or depth of the service"
        );

        // Without a minimum no point says whether it is below one.
        let unasked = drawn(&sample(), &defaults());
        assert!(!has(point(&unasked, "E1-1"), keys::COVER_BELOW_MINIMUM));
        assert!(has(point(&unasked, "E1-1"), keys::COVER));

        // A level given with no reference is on the top, as the grading and
        // the cover take it - the one reading the parsed schedule keeps.
        let unreferenced = drawn(
            &parsed(concat!(
                "line,point,easting,northing,method,level,surface,h_unc,type\n",
                "S2,S2-1,0,0,pothole,19.0,20.0,0.02,sewer\n",
                "S2,S2-2,5,0,pothole,19.1,20.1,0.02,\n"
            )),
            &defaults(),
        );
        assert_eq!(
            text(point(&unreferenced, "S2-1"), keys::LEVEL_REFERENCE),
            "top"
        );
        near(real(point(&unreferenced, "S2-1"), keys::COVER), 1.0, 1e-9);
    }

    #[test]
    fn each_point_carries_its_whole_schedule_row_so_the_points_are_the_schedule() {
        // W1-2: "W1,W1-2,334008.000,6250000.150,EML,19.20,centre,20.25,0.10,0.35,QL-B"
        // on a line whose path column says the pothole's stretch is exposed.
        let drawing = drawn(&sample(), &defaults());
        let w12 = point(&drawing, "W1-2");
        assert_eq!(text(w12, keys::ORDER), "2");
        double_eq(real(w12, keys::LEVEL), 19.20);
        assert_eq!(text(w12, keys::LEVEL_REFERENCE), "centre");
        double_eq(real(w12, keys::SURFACE_LEVEL), 20.25);
        double_eq(real(w12, keys::HORIZONTAL_UNCERTAINTY), 0.10);
        double_eq(real(w12, keys::VERTICAL_UNCERTAINTY), 0.35);
        assert_eq!(text(w12, keys::CLAIMED), "QL-B");
        assert_eq!(text(w12, keys::METHOD), "electromagnetic location");
        assert!(!has(w12, keys::DEPTH));
        // Its line's attributes, as the runs carry them.
        assert_eq!(text(w12, keys::TYPE), "water");
        assert_eq!(text(w12, keys::OWNER), "WaterCo");
        assert_eq!(text(w12, keys::MATERIAL), "DICL");
        double_eq(real(w12, keys::DIAMETER), 0.150);
        assert_eq!(text(w12, keys::DIAMETER_INSIDE), "false");
        assert_eq!(text(w12, keys::STATUS), "in service");
        assert_eq!(text(w12, keys::DESCRIPTION), "trunk main");
        // A path is to the next point: given for one stretch of W1, so on every
        // point of it but the last, the rest detected.
        assert_eq!(text(w12, keys::PATH), "detected");
        assert_eq!(text(point(&drawing, "W1-X1"), keys::PATH), "exposed");
        assert!(!has(point(&drawing, "W1-5"), keys::PATH));
        assert_eq!(text(point(&drawing, "W1-5"), keys::ORDER), "6");
        // A line with no path column value has none: every stretch detected.
        assert!(!has(point(&drawing, "E1-1"), keys::PATH));
        assert_eq!(text(point(&drawing, "T1-P1"), keys::PATH), "assumed");
        // No level and no depth: no reference, since there is nothing it is
        // of.
        assert!(!has(point(&drawing, "T1-P1"), keys::LEVEL_REFERENCE));

        // A depth and no level, as a delivery schema records it: the depth,
        // its reference, and the service level the surface makes of it - and
        // the schema's own fields, the point's and its line's.
        let delivery = drawn(&parsed(&sample_text("schedule_tfnsw.csv")), &defaults());
        let w1 = point(&delivery, "W1");
        assert!(!has(w1, keys::LEVEL));
        double_eq(real(w1, keys::DEPTH), 0.95);
        assert_eq!(text(w1, keys::LEVEL_REFERENCE), "top");
        near(real(w1, keys::SERVICE_LEVEL), 20.30 - 0.95, 1e-9);
        double_eq(real(w1, keys::DIAMETER), 0.150);
        assert_eq!(text(w1, keys::DIAMETER_INSIDE), "true");
        assert_eq!(
            text(w1, &format!("{}DateInfoObtained", keys::FIELD_PREFIX)),
            "2026/09/20"
        );
        assert_eq!(
            text(w1, &format!("{}AssetType", keys::FIELD_PREFIX)),
            "Water"
        );
        // Recorded as unknown, the reference is kept although there is no
        // level.
        assert_eq!(
            text(point(&delivery, "G1"), keys::LEVEL_REFERENCE),
            "unknown"
        );
    }

    #[test]
    fn spacing_changes_the_grading() {
        let mut options = defaults();
        options.grading.maximum_detected_spacing = 20.0;
        let drawing = drawn(&sample(), &options);
        // W1-4 -> W1-5, 12.5 m, is traced now: QL-B, and still its own run,
        // since the QL-A trench lies between it and the other QL-B stretch.
        let runs = of(&drawing, "W1", EntityType::Polyline);
        assert_eq!(runs.len(), 3);
        assert_eq!(text(&runs[2], keys::QUALITY_LEVEL), "QL-B");
        assert_eq!(runs[2].layer, "utilities/water/QL-B");
        near(
            drawing.lines[0].length_at[QualityLevel::C as usize],
            0.0,
            1e-9,
        );
        // W1 has nothing left at QL-C, so it has no QL-C layer; E1's weak pick
        // is a vertex, not a spacing, and keeps its own.
        let names = layer_names(&drawing);
        let count = |name: &str| names.iter().filter(|layer| *layer == name).count();
        assert_eq!(count("utilities/water/QL-C"), 0);
        assert_eq!(count("utilities/electricity/QL-C"), 1);
        let reply = format_utility_drawing(&drawing, UTILITIES_DRAWN_RECORD);
        assert_eq!(
            &reply[..reply.find('\n').unwrap_or(reply.len())],
            concat!(
                "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=10 ",
                "bounds=334000.000,6250000.000,334040.000,6250007.200"
            )
        );
    }

    #[test]
    fn a_line_that_cannot_be_graded_refuses_the_whole_drawing() {
        let lines = parsed(concat!(
            "line,point,easting,northing,method,h_unc,type\n",
            "W1,W1-1,0,0,EML,0.1,water\n",
            "W1,W1-2,10,0,EML,0.1,\n",
            "W9,W9-1,5,5,EML,0.1,water\n"
        ));
        assert_eq!(lines.len(), 2);
        let error = draw_utilities(&lines, &defaults()).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("line W9 cannot be graded"),
            "{}",
            error.describe()
        );
        assert!(error.message.contains("at least two vertices"));
        assert!(error.message.contains("nothing was drawn"));

        assert!(draw_utilities(&[], &defaults()).is_err());
    }

    #[test]
    fn a_layer_prefix_places_every_layer_under_it() {
        let mut options = UtilityDrawOptions {
            layer_prefix: String::from("Survey/Services"),
            ..defaults()
        };
        let drawing = drawn(&sample(), &options);
        let names = layer_names(&drawing);
        assert!(names.len() >= 3);
        assert_eq!(names[0], "Survey");
        assert_eq!(names[1], "Survey/Services");
        assert_eq!(names[2], "Survey/Services/electricity");
        for entity in &drawing.entities {
            assert!(
                is_layer_under(&entity.layer, "Survey/Services"),
                "{}",
                entity.layer
            );
        }
        options.layer_prefix = String::from("a//b");
        let refused = draw_utilities(&sample(), &options).expect_err("refused");
        assert!(refused.message.contains("not a layer path"));

        // A prefix that is a layer path but leaves no room for the two levels
        // drawn under it is refused here, by the prefix - not by the model
        // once the drawing's layers are being made. Fifteen levels are
        // seventeen with the type and the quality level; the model takes
        // sixteen.
        options.layer_prefix = String::from("a/b/c/d/e/f/g/h/i/j/k/l/m/n/o");
        let deep = draw_utilities(&sample(), &options).expect_err("refused");
        assert_eq!(deep.code, ErrorCode::InvalidArgument);
        assert!(
            deep.message.contains("the layer prefix leaves no room"),
            "{}",
            deep.describe()
        );
        assert!(deep.message.contains("nested too deeply (17 > 16)"));
        assert_eq!(deep.context, options.layer_prefix);
        // 490 characters take "/water/QL-B", but not "/telecommunications/QL-C",
        // which makes 514 of the model's 512.
        options.layer_prefix = "p".repeat(490);
        let wide = draw_utilities(&sample(), &options).expect_err("refused");
        assert!(
            wide.message
                .contains("/telecommunications/QL-C - layer name is too long"),
            "{}",
            wide.describe()
        );
    }

    #[test]
    fn a_run_of_no_length_is_not_drawn_but_its_points_are() {
        let drawing = drawn(&parsed(SHARED_PLACE), &defaults());
        assert_eq!(drawing.segments, 3);
        let runs = of(&drawing, "S1", EntityType::Polyline);
        assert_eq!(runs.len(), 2);
        assert_eq!(text(&runs[0], keys::QUALITY_LEVEL), "QL-B");
        assert_eq!(text(&runs[1], keys::QUALITY_LEVEL), "QL-B");
        assert_eq!(of(&drawing, "S1", EntityType::Point).len(), 4);
        assert_eq!(drawing.lines[0].polylines, 2);
        // No QL-A layer: nothing is on it.
        assert!(
            !layer_names(&drawing)
                .iter()
                .any(|name| name == "utilities/sewer/QL-A")
        );
    }

    #[test]
    fn a_run_a_rounding_error_long_is_not_drawn_either() {
        // Longer than nothing, shorter than the model's tolerance: were it
        // drawn, the model would refuse it, and with it the whole draw.
        let drawing = drawn(&parsed(NEARLY_ONE_PLACE), &defaults());
        assert_eq!(drawing.segments, 3);
        assert_eq!(of(&drawing, "S1", EntityType::Polyline).len(), 2);
        assert_eq!(of(&drawing, "S1", EntityType::Point).len(), 4);
        assert!(
            !layer_names(&drawing)
                .iter()
                .any(|name| name == "utilities/sewer/QL-A")
        );
        for entity in &drawing.entities {
            let valid = validate(&entity.geometry);
            assert!(
                valid.is_ok(),
                "{}: {}",
                text(entity, keys::VERTEX),
                valid
                    .err()
                    .map(|error| error.describe())
                    .unwrap_or_default()
            );
        }
    }
}
