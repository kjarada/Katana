//! The twin of tests/survey/test_subsurface_schedule_writer.cpp.
//!
//! A schedule written from the model (write_utility_csv) reads back as the
//! lines it was written from - field for field, doubles to the bit - so that a
//! drawing edited in CAD becomes a deliverable again; and in a client's words
//! when a delivery schema is given (utility_csv_dialect).

use std::collections::BTreeMap;

use katana_core::{ErrorCode, Result};
use katana_survey::coordinate::Coordinate2;
use katana_survey::subsurface::*;

const EML: LocationMethod = LocationMethod::ElectromagneticLocation;

fn read_back(lines: &[UtilityLine], dialect: &UtilityCsvDialect) -> Vec<UtilityLine> {
    let text =
        write_utility_csv(lines, dialect).unwrap_or_else(|error| panic!("{}", error.describe()));
    parse_utility_csv(&text).unwrap_or_else(|error| panic!("{}\n{text}", error.describe()))
}

/// `writeUtilityCsv` of one line, in the reader's own words.
fn write(line: &UtilityLine) -> Result<String> {
    write_utility_csv(std::slice::from_ref(line), &UtilityCsvDialect::default())
}

fn vertex(id: &str, easting: f64, northing: f64, method: LocationMethod) -> UtilityVertex {
    UtilityVertex {
        id: String::from(id),
        position: Coordinate2 { northing, easting },
        evidence: PositionEvidence {
            method,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn cells(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (String::from(*name), String::from(*value)))
        .collect()
}

// Every field the format has, set.
fn everything() -> UtilityLine {
    let mut a = vertex("P1", 334000.125, 6250000.1, EML);
    a.level = Some(19.2);
    a.level_reference = LevelReference::Centre;
    a.surface_level = Some(20.25);
    a.evidence.horizontal_uncertainty = Some(0.1);
    a.evidence.vertical_uncertainty = Some(0.35);
    a.claimed = Some(QualityLevel::B);
    a.fields = cells(&[("DateInfoObtained", "2026/09/20")]);
    let mut b = vertex(
        " P2",
        334008.0,
        6250000.15,
        LocationMethod::NonDestructiveExcavation,
    );
    b.depth = Some(0.95);
    b.surface_level = Some(20.3);
    b.verifies = String::from("P1");
    b.evidence.has_level = true;
    let mut c = vertex("P3", 334020.0, 6250000.2, LocationMethod::Unknown);
    c.level_reference = LevelReference::Unknown;
    a.evidence.has_level = true;
    UtilityLine {
        id: String::from("W-0001"),
        attributes: UtilityAttributes {
            r#type: UtilityType::RecycledWater,
            status: UtilityStatus::Abandoned,
            owner: String::from("Water Co, North"),
            material: String::from("\"DICL\" lined"),
            diameter: 0.375,
            diameter_is_inside: true,
            configuration: String::from("4 x 100"),
            description: String::from("#1 trunk main "),
            fields: cells(&[("Capacity", "210kPa"), ("Clash", "Soft")]),
            ..Default::default()
        },
        vertices: vec![a, b, c],
        path_evidence: vec![PathEvidence::Exposed, PathEvidence::Assumed],
    }
}

mod subsurface_schedule_writer {
    use super::*;

    #[test]
    fn every_field_reads_back_exactly() {
        let lines = [everything()];
        assert_eq!(read_back(&lines, &UtilityCsvDialect::default()), lines);
    }

    #[test]
    fn every_word_value_reads_back_as_itself() {
        let mut lines = Vec::new();
        for kind in [
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
        ] {
            lines.push(UtilityLine {
                id: format!("T{}", to_string(kind)),
                attributes: UtilityAttributes {
                    r#type: kind,
                    ..Default::default()
                },
                vertices: vec![vertex("a", 0.0, 0.0, EML), vertex("b", 1.0, 0.0, EML)],
                ..Default::default()
            });
        }
        for status in [
            UtilityStatus::Unknown,
            UtilityStatus::InService,
            UtilityStatus::Disused,
            UtilityStatus::Abandoned,
            UtilityStatus::Proposed,
        ] {
            lines.push(UtilityLine {
                id: format!("S{}", to_string(status)),
                attributes: UtilityAttributes {
                    status,
                    ..Default::default()
                },
                vertices: vec![vertex("a", 0.0, 0.0, EML), vertex("b", 1.0, 0.0, EML)],
                ..Default::default()
            });
        }
        let mut methods = UtilityLine {
            id: String::from("M"),
            ..Default::default()
        };
        for method in [
            LocationMethod::Unknown,
            LocationMethod::Records,
            LocationMethod::Anecdotal,
            LocationMethod::SurfaceFeature,
            LocationMethod::ElectromagneticLocation,
            LocationMethod::GroundPenetratingRadar,
            LocationMethod::OtherGeophysical,
            LocationMethod::NonDestructiveExcavation,
            LocationMethod::OpenExcavation,
        ] {
            methods
                .vertices
                .push(vertex(&format!("m{}", to_string(method)), 0.0, 0.0, method));
        }
        for reference in [
            LevelReference::Top,
            LevelReference::Centre,
            LevelReference::Invert,
            LevelReference::Unknown,
        ] {
            let mut v = vertex(&format!("r{}", to_string(reference)), 0.0, 0.0, EML);
            v.level = Some(10.0);
            v.level_reference = reference;
            v.evidence.has_level = true;
            methods.vertices.push(v);
        }
        for claim in [
            QualityLevel::A,
            QualityLevel::B,
            QualityLevel::C,
            QualityLevel::D,
        ] {
            let mut v = vertex(&format!("q{}", to_string(claim)), 0.0, 0.0, EML);
            v.claimed = Some(claim);
            methods.vertices.push(v);
        }
        methods.path_evidence = vec![PathEvidence::Detected; methods.vertices.len() - 1];
        methods.path_evidence[1] = PathEvidence::Exposed;
        methods.path_evidence[2] = PathEvidence::Assumed;
        lines.push(methods);
        assert_eq!(read_back(&lines, &UtilityCsvDialect::default()), lines);
    }

    #[test]
    fn every_whole_millimetre_diameter_reads_back_to_the_bit() {
        // A diameter read from a schedule is millimetres divided by 1000; the
        // writer must find the millimetres that divide back to it exactly.
        let mut lines = Vec::new();
        for millimetres in 1..=3000 {
            for inside in [false, true] {
                lines.push(UtilityLine {
                    id: format!("{millimetres}{}", if inside { "i" } else { "o" }),
                    attributes: UtilityAttributes {
                        diameter: f64::from(millimetres) / 1000.0,
                        diameter_is_inside: inside,
                        ..Default::default()
                    },
                    vertices: vec![vertex("a", 0.0, 0.0, EML), vertex("b", 1.0, 0.0, EML)],
                    ..Default::default()
                });
            }
        }
        let back = read_back(&lines, &UtilityCsvDialect::default());
        assert_eq!(back.len(), lines.len());
        for (read, line) in back.iter().zip(&lines) {
            assert_eq!(
                read.attributes.diameter, line.attributes.diameter,
                "{}",
                line.id
            );
            assert_eq!(
                read.attributes.diameter_is_inside,
                line.attributes.diameter_is_inside
            );
        }
    }

    #[test]
    fn only_the_columns_that_hold_something_are_written() {
        let line = UtilityLine {
            id: String::from("G1"),
            vertices: vec![
                vertex("a", 1.0, 2.0, LocationMethod::Records),
                vertex("b", 3.0, 4.0, EML),
            ],
            ..Default::default()
        };
        let text = write(&line).unwrap();
        assert_eq!(
            text,
            "line,point,easting,northing,method\n\
             G1,a,1,2,records\n\
             G1,b,3,4,electromagnetic location\n"
        );
        // Nothing at all is a header alone, which reads as no lines.
        let none = write_utility_csv(&[], &UtilityCsvDialect::default()).unwrap();
        assert_eq!(none, "line,point,easting,northing,method\n");
        assert!(parse_utility_csv(&none).unwrap().is_empty());
    }

    #[test]
    fn what_a_schedule_cannot_hold_is_refused_by_name() {
        let mut line = everything();
        line.vertices[1].verifies = String::from("two\nlines");
        assert_eq!(
            write(&line).unwrap_err().message,
            "line W-0001 point  P2: verifies holds a line break; a schedule cannot hold it"
        );

        line = everything();
        line.attributes
            .fields
            .insert(String::from("Colour"), String::from("blue"));
        assert_eq!(write(&line).unwrap_err().code, ErrorCode::InvalidArgument);
        line = everything();
        line.vertices[0]
            .fields
            .insert(String::from("Capacity"), String::from("1"));
        assert!(
            write(&line)
                .unwrap_err()
                .message
                .contains("carried by the other")
        );
        line = everything();
        line.path_evidence.truncate(line.path_evidence.len() - 1);
        assert_eq!(write(&line).unwrap_err().code, ErrorCode::InvalidArgument);
        line = everything();
        line.vertices[2].id = String::from("P1");
        assert!(
            write(&line)
                .unwrap_err()
                .message
                .contains("a point id twice")
        );
        line = everything();
        line.vertices[1].depth = Some(-0.5);
        assert_eq!(write(&line).unwrap_err().code, ErrorCode::InvalidArgument);
        line = everything();
        line.id.clear();
        assert_eq!(write(&line).unwrap_err().code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_schemas_dialect_names_the_columns_and_spells_the_values_its_way() {
        let schema = parse_delivery_schema(concat!(
            "kind,attribute,value,detail,label\n",
            "schema,Example,1,,\n",
            "field,AssetIdentifier,Alphanumerical,Yes,Asset Identifier\n",
            "field,AssetTypeCode,Domain List: Asset Type Code,Yes,Asset Type Code\n",
            "domain,AssetTypeCode,W,,\n",
            "domain,AssetTypeCode,E,,\n",
            "field,Situation,Domain List: Status,Yes,Status\n",
            "domain,Situation,Live,,\n",
            "domain,Situation,In Service,,\n",
            "field,DepthLocation,Domain List: Depth Location,Yes,Depth Location\n",
            "domain,DepthLocation,Top of Concrete Encasement,,\n",
            "domain,DepthLocation,Top of Pipe,,\n",
            "domain,DepthLocation,Top Row Invert,,\n",
        ))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let dialect = utility_csv_dialect(&schema);
        // By an alias of the column, and by a label when the attribute is none.
        assert_eq!(dialect.headers["line"], "AssetIdentifier");
        assert_eq!(dialect.headers["type"], "AssetTypeCode");
        assert_eq!(dialect.headers["status"], "Status");
        assert_eq!(dialect.headers["level_ref"], "DepthLocation");
        assert!(!dialect.headers.contains_key("method"));
        // The first listed spelling of each value.
        assert_eq!(dialect.spellings["type"]["water"], "W");
        assert_eq!(dialect.spellings["status"]["in service"], "Live");
        assert_eq!(
            dialect.spellings["level_ref"]["top"],
            "Top of Concrete Encasement"
        );
        assert_eq!(dialect.spellings["level_ref"]["invert"], "Top Row Invert");

        let mut line = UtilityLine {
            id: String::from("W-1"),
            attributes: UtilityAttributes {
                r#type: UtilityType::Water,
                status: UtilityStatus::InService,
                ..Default::default()
            },
            vertices: vec![vertex("a", 0.0, 0.0, EML), vertex("b", 5.0, 0.0, EML)],
            ..Default::default()
        };
        line.vertices[0].depth = Some(1.0);
        line.vertices[0].evidence.has_level = true;
        let text = write_utility_csv(std::slice::from_ref(&line), &dialect).unwrap();
        assert_eq!(
            text,
            concat!(
                "AssetIdentifier,point,easting,northing,method,DepthLocation,depth,",
                "AssetTypeCode,Status\n",
                "W-1,a,0,0,electromagnetic location,Top of Concrete Encasement,1,W,Live\n",
                "W-1,b,5,0,electromagnetic location,,,W,Live\n",
            )
        );
        let back = read_back(std::slice::from_ref(&line), &dialect);
        assert_eq!(back, [line]);
    }

    #[test]
    fn cells_the_reader_reads_as_not_recorded_are_written_as_the_schedule_wrote_them() {
        // Each of these reads as "not recorded", or in part: a claim of Unknown,
        // a Depth Location with nothing measured on it, a type Not Specified, a
        // status Unknown, a size Not Applicable, a W x H size, a size beside a
        // diameter_mm.
        let lines = parse_utility_csv(concat!(
            "line,point,easting,northing,method,level_ref,ql,type,status,size,diameter_mm\n",
            "A,a1,0,0,EML,Top of Pipe,Unknown,Not Specified,Unknown,Not Applicable,\n",
            "A,a2,5,0,EML,,Unknown,Not Specified,Unknown,Not Applicable,\n",
            "B,b1,0,1,EML,,QL-B,water,live,1200 x 900,\n",
            "B,b2,5,1,EML,,,water,live,1200 x 900,\n",
            "C,c1,0,2,EML,,,gas,,110,150\n",
            "C,c2,5,2,EML,,,gas,,110,150\n",
        ))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(lines.len(), 3);
        let a = &lines[0];
        assert_eq!(a.attributes.r#type, UtilityType::Unknown);
        assert_eq!(a.attributes.status, UtilityStatus::Unknown);
        assert_eq!(a.attributes.diameter, 0.0);
        assert!(a.vertices[0].claimed.is_none());
        assert_eq!(a.vertices[0].level_reference, a.vertices[1].level_reference);
        assert_eq!(lines[1].attributes.diameter, 1.2);
        assert_eq!(lines[2].attributes.diameter, 0.15);

        // Written back as they were written, and read back as they were read.
        let text = write_utility_csv(&lines, &UtilityCsvDialect::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            text,
            concat!(
                "line,point,easting,northing,method,level_ref,ql,type,diameter_mm,size,status\n",
                "A,a1,0,0,electromagnetic location,Top of Pipe,Unknown,Not Specified,,",
                "Not Applicable,Unknown\n",
                "A,a2,5,0,electromagnetic location,,Unknown,Not Specified,,Not Applicable,Unknown\n",
                "B,b1,0,1,electromagnetic location,,QL-B,water,,1200 x 900,in service\n",
                "B,b2,5,1,electromagnetic location,,,water,,1200 x 900,in service\n",
                "C,c1,0,2,electromagnetic location,,,gas,150,110,\n",
                "C,c2,5,2,electromagnetic location,,,gas,150,110,\n",
            )
        );
        assert_eq!(parse_utility_csv(&text).unwrap(), lines);

        // What a schedule words the writer's own way is not kept: nothing to say
        // again.
        let plain = parse_utility_csv(concat!(
            "line,point,easting,northing,method,size,level_ref,depth\n",
            "P,p1,0,0,EML,150,Top of Pipe,0.9\n",
            "P,p2,5,0,EML,150,,\n",
        ))
        .unwrap();
        assert!(plain[0].attributes.recorded.is_empty());
        assert!(plain[0].vertices[0].recorded.is_empty());

        // An edit made since wins over what was kept: each kept cell is written
        // only while it still reads as the value held.
        let mut edited = lines.clone();
        edited[0].attributes.status = UtilityStatus::InService;
        edited[0].vertices[0].claimed = Some(QualityLevel::C);
        edited[0].vertices[0].level = Some(19.0);
        edited[1].attributes.diameter = 0.3;
        let rewritten = write_utility_csv(&edited, &UtilityCsvDialect::default()).unwrap();
        assert!(
            rewritten.contains(
                "A,a1,0,0,electromagnetic location,19,top,QL-C,Not Specified,,Not Applicable,\
                 in service\n"
            ),
            "{rewritten}"
        );
        assert!(
            rewritten.contains("B,b1,0,1,electromagnetic location,,,QL-B,water,,300,in service\n"),
            "{rewritten}"
        );
    }
}
