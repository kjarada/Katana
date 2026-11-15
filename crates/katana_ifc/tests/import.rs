//! Twin of tests/ifc/test_import.cpp: Katana's own files read back as the
//! drawing they were, other writers' files in their units and placements, and
//! what is not an IFC file - or is a hostile one - refused or read as far as
//! it makes sense.

pub mod support;

use std::f64::consts::PI;
use std::path::Path;

use katana_commands::command_stack::CommandStack;
use katana_core::ErrorCode;
use katana_entity::entity::{
    Entity, Geometry, PropertyValue, TextGeometry, heights_of, property_to_string, set_heights,
};
use katana_entity::model::Model;
use katana_entity::tables::Alignment;
use katana_geometry::alignment::{AlignmentPi, HorizontalAlignment};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::profile::{ProfilePvi, VerticalAlignment};
use katana_geometry::{Ellipse2, Point2, Polyline2, Spline2, Vec2};
use katana_ifc::{
    ExportInput, ExportOptions, IfcImport, ImportOptions, import_command, read_ifc, read_ifc_file,
    write_ifc,
};
use support::*;

// An IFC2X3 file in millimetres, as most building models are: a water main
// placed at (5000, 2000, 1000) mm with its x axis turned to north, its axis
// 3000 mm along that and 500 mm down. In metres, from (5, 2, 1) to (5, 5,
// 0.5). Its diameter, 150 mm, is a length and comes in as 0.15.
const MILLIMETRES: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView_V2.0]'),'2;1');
FILE_NAME('mm.ifc','2020-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('IFC2X3'));
ENDSEC;
DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);
#3=IFCUNITASSIGNMENT((#1,#2));
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#5,$);
#7=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'mm',$,$,$,$,(#6),#3);
#8=IFCLOCALPLACEMENT($,#5);
#9=IFCSITE('1YvctVUKr0kugbFTf53O9L',$,'Site',$,$,#8,$,$,.ELEMENT.,$,$,$,$,$);
#10=IFCCARTESIANPOINT((5000.,2000.,1000.));
#11=IFCDIRECTION((0.,1.,0.));
#12=IFCDIRECTION((0.,0.,1.));
#13=IFCAXIS2PLACEMENT3D(#10,#12,#11);
#14=IFCLOCALPLACEMENT(#8,#13);
#15=IFCCARTESIANPOINT((0.,0.,0.));
#16=IFCCARTESIANPOINT((3000.,0.,-500.));
#17=IFCPOLYLINE((#15,#16));
#18=IFCSHAPEREPRESENTATION(#6,'Axis','Curve3D',(#17));
#19=IFCPRODUCTDEFINITIONSHAPE($,$,(#18));
#20=IFCFLOWSEGMENT('2YvctVUKr0kugbFTf53O9L',$,'Main 1','A water main',$,#14,#19,'WM-1');
#21=IFCPROPERTYSINGLEVALUE('NominalDiameter',$,IFCPOSITIVELENGTHMEASURE(150.),$);
#22=IFCPROPERTYSINGLEVALUE('Material',$,IFCLABEL('DICL'),$);
#23=IFCPROPERTYSINGLEVALUE('InService',$,IFCBOOLEAN(.T.),$);
#24=IFCPROPERTYSET('3YvctVUKr0kugbFTf53O9L',$,'Pset_Main',$,(#21,#22,#23));
#25=IFCRELDEFINESBYPROPERTIES('0ZvctVUKr0kugbFTf53O9L',$,$,$,(#20),#24);
#26=IFCRELCONTAINEDINSPATIALSTRUCTURE('1ZvctVUKr0kugbFTf53O9L',$,$,$,(#20),#9);
ENDSEC;
END-ISO-10303-21;
"#;

// An IFC4 file georeferenced to MGA zone 56 with its x axis turned
// atan2(0.8, 0.6) from east, and its angles in degrees. A point at (10, 0,
// 2) is at E 300000 + 0.6 x 10, N 6200000 + 0.8 x 10, height 10 + 2; a
// quarter circle of radius 5 about the origin, trimmed 0 to 90 degrees,
// starts at the rotation's angle; the next quarter, trimmed by the points
// (0, 5) and (-5, 0), 90 degrees after it.
const ROTATED: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');
FILE_NAME('rotated.ifc','2020-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCDIMENSIONALEXPONENTS(0,0,0,0,0,0,0);
#3=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);
#4=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.017453292519943295),#3);
#5=IFCCONVERSIONBASEDUNIT(#2,.PLANEANGLEUNIT.,'DEGREE',#4);
#6=IFCUNITASSIGNMENT((#1,#5));
#7=IFCCARTESIANPOINT((0.,0.,0.));
#8=IFCAXIS2PLACEMENT3D(#7,$,$);
#9=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#8,$);
#10=IFCPROJECTEDCRS('EPSG:7856','GDA2020 / MGA zone 56',$,$,$,$,$);
#11=IFCMAPCONVERSION(#9,#10,300000.,6200000.,10.,0.6,0.8,$);
#12=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'rotated',$,$,$,$,(#9),#6);
#13=IFCLOCALPLACEMENT($,#8);
#14=IFCCARTESIANPOINT((10.,0.,2.));
#15=IFCSHAPEREPRESENTATION(#9,'Annotation','Point',(#14));
#16=IFCPRODUCTDEFINITIONSHAPE($,$,(#15));
#17=IFCANNOTATION('1YvctVUKr0kugbFTf53O9L',$,'Peg',$,$,#13,#16);
#18=IFCCARTESIANPOINT((0.,0.));
#19=IFCAXIS2PLACEMENT2D(#18,$);
#20=IFCCIRCLE(#19,5.);
#21=IFCTRIMMEDCURVE(#20,(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(90.)),.T.,.PARAMETER.);
#22=IFCSHAPEREPRESENTATION(#9,'Annotation','Curve2D',(#21));
#23=IFCPRODUCTDEFINITIONSHAPE($,$,(#22));
#24=IFCANNOTATION('2YvctVUKr0kugbFTf53O9L',$,'Kerb return',$,$,#13,#23);
#25=IFCCARTESIANPOINT((0.,5.));
#26=IFCCARTESIANPOINT((-5.,0.));
#27=IFCTRIMMEDCURVE(#20,(#25),(#26),.T.,.CARTESIAN.);
#28=IFCSHAPEREPRESENTATION(#9,'Annotation','Curve2D',(#27));
#29=IFCPRODUCTDEFINITIONSHAPE($,$,(#28));
#30=IFCANNOTATION('3YvctVUKr0kugbFTf53O9L',$,'Trimmed by points',$,$,#13,#29);
ENDSEC;
END-ISO-10303-21;
"#;

// An IFC 4.3 alignment that starts on a curve - a circular arc of radius
// 100 turning left for 50 m, then 50 m of tangent - which has no PI form:
// it comes in as its exact geometry. The arc ends 0.5 rad round, at
// (100 sin 0.5, 100 (1 - cos 0.5)).
const STARTS_ON_A_CURVE: &str = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [Alignment-basedView]'),'2;1');
FILE_NAME('curve.ifc','2020-01-01T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('IFC4X3_ADD2'));
ENDSEC;
DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);
#3=IFCUNITASSIGNMENT((#1,#2));
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#5,$);
#7=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'curve',$,$,$,$,(#6),#3);
#8=IFCLOCALPLACEMENT($,#5);
#9=IFCALIGNMENT('1YvctVUKr0kugbFTf53O9L',$,'Ramp A',$,$,#8,$,$);
#10=IFCALIGNMENTHORIZONTAL('2YvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$);
#11=IFCRELNESTS('3YvctVUKr0kugbFTf53O9L',$,$,$,#9,(#10));
#12=IFCCARTESIANPOINT((0.,0.));
#13=IFCALIGNMENTHORIZONTALSEGMENT($,$,#12,0.,100.,100.,50.,$,.CIRCULARARC.);
#14=IFCALIGNMENTSEGMENT('0ZvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$,#13);
#15=IFCCARTESIANPOINT((47.942553860420304,12.241743810962724));
#16=IFCALIGNMENTHORIZONTALSEGMENT($,$,#15,0.5,0.,0.,50.,$,.LINE.);
#17=IFCALIGNMENTSEGMENT('1ZvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$,#16);
#18=IFCCARTESIANPOINT((91.82168195493894,36.213020741172876));
#19=IFCALIGNMENTHORIZONTALSEGMENT($,$,#18,0.5,0.,0.,0.,$,.LINE.);
#20=IFCALIGNMENTSEGMENT('2ZvctVUKr0kugbFTf53O9L',$,$,$,$,#8,$,#19);
#21=IFCRELNESTS('3ZvctVUKr0kugbFTf53O9L',$,$,$,#10,(#14,#17,#20));
ENDSEC;
END-ISO-10303-21;
"#;

fn metadata(entity: &Entity, key: &str) -> String {
    entity
        .metadata
        .get(key)
        .map_or_else(|| String::from("<absent>"), property_to_string)
}

fn property(entity: &Entity, name: &str) -> String {
    entity
        .properties
        .get(name)
        .map_or_else(|| String::from("<absent>"), property_to_string)
}

fn with_name<'a>(imported: &'a IfcImport, name: &str) -> Option<&'a Entity> {
    imported
        .entities
        .iter()
        .find(|entity| metadata(entity, "ifc.name") == name)
}

/// The vertices of a line or polyline.
fn vertices_of(entity: &Entity) -> Vec<Point2> {
    match &entity.geometry {
        Geometry::Line(line) => vec![line.start, line.end],
        Geometry::Polyline(polyline) => polyline.vertices.clone(),
        _ => Vec::new(),
    }
}

fn read(text: &str, options: &ImportOptions) -> IfcImport {
    must(read_ifc(text, options))
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn pi(x: f64, y: f64, radius: f64, spiral_in: f64, spiral_out: f64) -> AlignmentPi {
    AlignmentPi {
        point: p(x, y),
        radius,
        spiral_in,
        spiral_out,
    }
}

fn pvi(station: f64, elevation: f64, curve_length: f64) -> ProfilePvi {
    ProfilePvi {
        station,
        elevation,
        curve_length,
    }
}

/// The drawing of test_export's scenario, smaller: MC01 and a kerb.
fn scenario() -> Model {
    let mut model = Model::default();
    must(model.alignments.add(Alignment {
        name: String::from("MC01"),
        description: String::new(),
        horizontal: HorizontalAlignment {
            start_station: 1000.0,
            pis: vec![
                pi(333900.0, 6249950.0, 0.0, 0.0, 0.0),
                pi(334000.0, 6250020.0, 80.0, 20.0, 20.0),
                pi(334100.0, 6249990.0, 60.0, 0.0, 0.0),
                pi(334200.0, 6250060.0, 0.0, 0.0, 0.0),
            ],
        },
        vertical: Some(VerticalAlignment {
            pvis: vec![
                pvi(1000.0, 21.5, 0.0),
                pvi(1080.0, 23.0, 60.0),
                pvi(1180.0, 20.0, 80.0),
                pvi(1330.5, 21.0, 0.0),
            ],
        }),
    }));
    let mut kerb = Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![
                p(333990.0, 6250000.0),
                p(334010.0, 6250004.0),
                p(334030.0, 6250008.0),
            ],
            false,
        )),
        layer: String::from("Survey/Kerb"),
        ..Entity::default()
    };
    set_heights(&mut kerb.properties, &[Some(20.10), None, Some(20.00)]);
    kerb.properties.insert(
        String::from("code"),
        PropertyValue::Text(String::from("KB")),
    );
    must(model.entities.add(kerb));
    let text = Entity {
        geometry: Geometry::Text(TextGeometry {
            position: p(334000.0, 6250020.0),
            text: String::from("LOT 42"),
            ..TextGeometry::default()
        }),
        layer: String::from("Survey/Text"),
        ..Entity::default()
    };
    must(model.entities.add(text));
    model
}

fn exported_scenario(model: &Model) -> String {
    let mut options = ExportOptions::default();
    options.georeference.name = String::from("EPSG:7856");
    let input = ExportInput {
        model: Some(model),
        ..ExportInput::default()
    };
    must(write_ifc(&input, &options)).text
}

// ---- Katana's own files -------------------------------------------------------------

mod ifc_import {
    use super::*;

    // What Katana writes comes back as what it was: the alignment as its PIs
    // and PVIs (reconstructed from the business logic, not stored anywhere in
    // the file), the entities where they were in MGA, on their layers, with
    // their heights - an absent one still absent - and their properties.
    #[test]
    fn an_export_read_back_is_the_drawing_it_was() {
        let model = scenario();
        let imported = read(&exported_scenario(&model), &ImportOptions::default());
        assert_eq!(imported.schema, "IFC4X3_ADD2");
        assert_eq!(imported.coordinate_system, "EPSG:7856");
        assert_eq!(imported.alignments_as_polylines, 0);
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

        assert_eq!(imported.alignments.len(), 1);
        let original = model.alignments.find("MC01").unwrap();
        let back = &imported.alignments[0];
        assert_eq!(back.name, "MC01");
        near(back.horizontal.start_station, 1000.0, 1e-9);
        assert_eq!(back.horizontal.pis.len(), original.horizontal.pis.len());
        for (a, b) in back.horizontal.pis.iter().zip(&original.horizontal.pis) {
            near(a.point.x, b.point.x, 1e-6);
            near(a.point.y, b.point.y, 1e-6);
            near(a.radius, b.radius, 1e-6);
            near(a.spiral_in, b.spiral_in, 1e-6);
            near(a.spiral_out, b.spiral_out, 1e-6);
        }
        let back_vertical = back.vertical.as_ref().expect("the profile is read back");
        let original_vertical = original.vertical.as_ref().unwrap();
        assert_eq!(back_vertical.pvis.len(), original_vertical.pvis.len());
        for (a, b) in back_vertical.pvis.iter().zip(&original_vertical.pvis) {
            near(a.station, b.station, 1e-6);
            near(a.elevation, b.elevation, 1e-6);
            near(a.curve_length, b.curve_length, 1e-6);
        }

        let mut kerb = None;
        let mut text = None;
        for entity in &imported.entities {
            if metadata(entity, "ifc.class") == "IfcKerb" {
                kerb = Some(entity);
            } else if matches!(entity.geometry, Geometry::Text(_)) {
                text = Some(entity);
            }
        }
        let kerb = kerb.expect("the kerb is read back");
        assert_eq!(kerb.layer, "Survey/Kerb");
        let vertices = vertices_of(kerb);
        assert_eq!(vertices.len(), 3);
        near(vertices[0].x, 333990.0, 1e-6);
        near(vertices[2].y, 6250008.0, 1e-6);
        let heights = heights_of(&kerb.properties, 3);
        let (Some(first), Some(_)) = (heights[0], heights[2]) else {
            panic!("the kerb's heights are not read back: {heights:?}");
        };
        near(first, 20.10, 1e-9);
        assert!(heights[1].is_none()); // absent is not zero
        assert_eq!(property(kerb, "code"), "KB");
        assert_eq!(metadata(kerb, "ifc.container"), "Site");

        let text = text.expect("the text is read back");
        let Geometry::Text(literal) = &text.geometry else {
            unreachable!("found as a text");
        };
        assert_eq!(literal.text, "LOT 42");
        near(literal.position.x, 334000.0, 1e-6);
        assert_eq!(text.layer, "Survey/Text");
    }

    // The drawing system's curves (docs/drawing.md) written by the export
    // come back as linework on the same curves: an elliptical arc as the arc,
    // not the whole ellipse it is trimmed from; a rational spline, not a
    // point at 0,0.
    #[test]
    fn an_exported_elliptical_arc_and_spline_read_back_on_their_curves() {
        let mut model = scenario();
        // Semi-axes 4 east and 2, from eccentric anomaly 5.5 for 2 radians -
        // through the parameter 0, so the file's trim end is below its start.
        let arc = Ellipse2 {
            center: p(334020.0, 6250020.0),
            major_axis: Vec2::new(4.0, 0.0),
            ratio: 0.5,
            start_parameter: 5.5,
            sweep: 2.0,
        };
        must(model.entities.add(Entity {
            geometry: Geometry::Ellipse(arc),
            layer: String::from("Survey/Detail"),
            ..Entity::default()
        }));
        let curve = Spline2 {
            degree: 2,
            control_points: vec![
                p(334020.0, 6250040.0),
                p(334025.0, 6250045.0),
                p(334030.0, 6250040.0),
            ],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            weights: vec![1.0, 0.5, 1.0],
            ..Spline2::default()
        };
        must(model.entities.add(Entity {
            geometry: Geometry::Spline(curve.clone()),
            layer: String::from("Survey/Curve"),
            ..Entity::default()
        }));

        let imported = read(&exported_scenario(&model), &ImportOptions::default());
        let mut ellipse_back = None;
        let mut spline_back = None;
        for entity in &imported.entities {
            if entity.layer == "Survey/Detail" {
                ellipse_back = Some(entity);
            } else if entity.layer == "Survey/Curve" {
                spline_back = Some(entity);
            }
        }
        let ellipse_back = ellipse_back.expect("the elliptical arc is read back");
        let spline_back = spline_back.expect("the spline is read back");

        let Geometry::Polyline(arc_back) = &ellipse_back.geometry else {
            panic!(
                "the elliptical arc is not linework: {:?}",
                ellipse_back.geometry
            );
        };
        assert!(!arc_back.closed, "the arc came back as the whole ellipse");
        assert!(arc_back.vertices.len() >= 3);
        near(
            arc_back.vertices[0].distance_to(arc.start_point()),
            0.0,
            1e-6,
        );
        near(
            arc_back
                .vertices
                .last()
                .unwrap()
                .distance_to(arc.end_point()),
            0.0,
            1e-6,
        );
        for &vertex in &arc_back.vertices {
            near(arc.distance_to(vertex), 0.0, 1e-6); // a vertex off the ellipse
        }

        let Geometry::Polyline(spline_line) = &spline_back.geometry else {
            panic!("the spline did not come back as linework");
        };
        assert!(spline_line.vertices.len() >= 3);
        // Clamped: it starts and ends on its end control points.
        near(
            spline_line.vertices[0].distance_to(curve.control_points[0]),
            0.0,
            1e-6,
        );
        near(
            spline_line
                .vertices
                .last()
                .unwrap()
                .distance_to(*curve.control_points.last().unwrap()),
            0.0,
            1e-6,
        );
        for &vertex in &spline_line.vertices {
            // Spline2::distance_to measures to its own chords, to a tenth of
            // CURVE_CHORD_TOLERANCE (curves2d.rs): the bound on a vertex ON it.
            assert!(
                curve.distance_to(vertex) <= 0.1 * CURVE_CHORD_TOLERANCE + 1e-9,
                "a vertex off the spline: {vertex:?}"
            );
        }
    }

    // ---- other writers' files ---------------------------------------------------

    #[test]
    fn lengths_come_in_as_metres_whatever_the_files_unit_and_placements_are_followed() {
        let options = ImportOptions {
            source_name: String::from("mm.ifc"),
            ..ImportOptions::default()
        };
        let imported = read(MILLIMETRES, &options);
        assert_eq!(imported.schema, "IFC2X3");
        assert_eq!(imported.products, 1);
        assert_eq!(imported.products_imported, 1);
        assert_eq!(imported.classes["IfcFlowSegment"], 1);
        let main = with_name(&imported, "Main 1").expect("the main is read");
        let vertices = vertices_of(main);
        assert_eq!(vertices.len(), 2);
        near(vertices[0].x, 5.0, 1e-12);
        near(vertices[0].y, 2.0, 1e-12);
        near(vertices[1].x, 5.0, 1e-12);
        near(vertices[1].y, 5.0, 1e-12);
        let heights = heights_of(&main.properties, 2);
        let (Some(start), Some(end)) = (heights[0], heights[1]) else {
            panic!("the main has no heights: {heights:?}");
        };
        near(start, 1.0, 1e-12);
        near(end, 0.5, 1e-12);

        assert_eq!(property(main, "Pset_Main/NominalDiameter"), "0.15");
        assert_eq!(property(main, "Pset_Main/Material"), "DICL");
        assert_eq!(property(main, "Pset_Main/InService"), "true");
        assert_eq!(metadata(main, "ifc.class"), "IfcFlowSegment");
        assert_eq!(metadata(main, "ifc.globalId"), "2YvctVUKr0kugbFTf53O9L");
        assert_eq!(metadata(main, "ifc.description"), "A water main");
        assert_eq!(metadata(main, "ifc.tag"), "WM-1");
        assert_eq!(metadata(main, "ifc.container"), "Site");
        assert_eq!(metadata(main, "source"), "mm.ifc");
        // With no presentation layer, a layer by where it is and what it is.
        assert_eq!(main.layer, "IFC/Site/FlowSegment");
        assert!(imported.coordinate_system.is_empty());
    }

    #[test]
    fn a_map_conversion_puts_the_file_where_it_is_and_angles_are_in_the_files_unit() {
        let imported = read(ROTATED, &ImportOptions::default());
        assert_eq!(imported.coordinate_system, "EPSG:7856");
        let peg = with_name(&imported, "Peg").expect("the peg is read");
        let Geometry::Point(point) = &peg.geometry else {
            panic!("the peg is not a point: {:?}", peg.geometry);
        };
        near(point.position.x, 300006.0, 1e-9);
        near(point.position.y, 6200008.0, 1e-9);
        let Some(height) = heights_of(&peg.properties, 1)[0] else {
            panic!("the peg has no height");
        };
        near(height, 12.0, 1e-9);

        let kerb_return = with_name(&imported, "Kerb return").expect("the kerb return is read");
        let Geometry::Arc(arc) = &kerb_return.geometry else {
            panic!("the kerb return is not an arc: {:?}", kerb_return.geometry);
        };
        near(arc.center.x, 300000.0, 1e-9);
        near(arc.center.y, 6200000.0, 1e-9);
        near(arc.radius, 5.0, 1e-12);
        near(arc.start_angle, 0.8_f64.atan2(0.6), 1e-12);
        near(arc.sweep, PI / 2.0, 1e-12);

        let by_points =
            with_name(&imported, "Trimmed by points").expect("the second quarter is read");
        let Geometry::Arc(next) = &by_points.geometry else {
            panic!("the second quarter is not an arc: {:?}", by_points.geometry);
        };
        near(next.start_angle, 0.8_f64.atan2(0.6) + PI / 2.0, 1e-12);
        near(next.sweep, PI / 2.0, 1e-12);
    }

    #[test]
    fn an_alignment_with_no_pi_form_comes_in_as_its_exact_geometry_and_says_why() {
        let options = ImportOptions {
            curve_tolerance: 0.001,
            ..ImportOptions::default()
        };
        let imported = read(STARTS_ON_A_CURVE, &options);
        assert!(imported.alignments.is_empty());
        assert_eq!(imported.alignments_as_polylines, 1);
        assert!(!imported.warnings.is_empty());
        assert!(
            imported.warnings[0].contains("Ramp A"),
            "{}",
            imported.warnings[0]
        );
        let ramp = imported
            .entities
            .iter()
            .rev()
            .find(|entity| metadata(entity, "ifc.alignment") == "Ramp A")
            .expect("the alignment is read as linework");
        assert_eq!(ramp.layer, "IFC/Alignments");
        let vertices = vertices_of(ramp);
        assert!(vertices.len() >= 4);
        near(vertices[0].x, 0.0, 1e-9);
        near(vertices[0].y, 0.0, 1e-9);
        let end = *vertices.last().unwrap();
        near(end.x, 100.0 * 0.5_f64.sin() + 50.0 * 0.5_f64.cos(), 1e-6);
        near(
            end.y,
            100.0 * (1.0 - 0.5_f64.cos()) + 50.0 * 0.5_f64.sin(),
            1e-6,
        );
        // Every vertex on the arc is on the circle about (0, 100), and the
        // chords stand no further than the tolerance from it.
        for vertex in &vertices {
            if vertex.x < 100.0 * 0.5_f64.sin() - 1e-9 {
                near(vertex.x.hypot(vertex.y - 100.0), 100.0, 1e-9);
            }
        }
        for pair in vertices.windows(2) {
            let middle = p((pair[0].x + pair[1].x) / 2.0, (pair[0].y + pair[1].y) / 2.0);
            if pair[1].x < 100.0 * 0.5_f64.sin() + 1e-9 {
                assert!(100.0 - middle.x.hypot(middle.y - 100.0) <= 0.001 + 1e-12);
            }
        }
    }

    #[test]
    fn local_moves_everything_alignments_included() {
        let model = scenario();
        let text = exported_scenario(&model);
        let options = ImportOptions {
            origin_shift: Some(Vec2::new(333900.0, 6249900.0)),
            ..ImportOptions::default()
        };
        let shifted = read(&text, &options);
        assert_eq!(shifted.alignments.len(), 1);
        near(shifted.alignments[0].horizontal.pis[0].point.x, 0.0, 1e-6);
        near(shifted.alignments[0].horizontal.pis[0].point.y, 50.0, 1e-6);
        // The extent is everything's: MC01's first PI is the furthest west.
        near(shifted.bounds.min.x, 0.0, 1e-6);
    }

    // ---- what is not an IFC file, or is a hostile one -------------------------

    #[test]
    fn refuses_what_is_not_an_ifc_file_naming_the_line() {
        let code = |text: &str| match read_ifc(text, &ImportOptions::default()) {
            Ok(_) => String::from("ok"),
            Err(error) => error.describe(),
        };
        assert!(code("").contains("does not begin ISO-10303-21"));
        assert!(code("0\nSECTION\n").contains("does not begin ISO-10303-21"));
        let header = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\n\
                      FILE_NAME('','',(''),(''),'','','');\n";
        let ifc4 = format!("{header}FILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n");
        // Truncated.
        assert!(code(&format!("{ifc4}#1=IFCCARTESIANPOINT((0.,0.")).contains("ParseFailure"));
        assert!(
            code(&format!(
                "{ifc4}#1=IFCLABEL('never closed);\nENDSEC;\nEND-ISO-10303-21;\n"
            ))
            .contains("never closed")
        );
        assert!(
            code(&format!(
                "{ifc4}#1=IFCCARTESIANPOINT((0.,0.));\n#1=IFCCARTESIANPOINT((1.,0.));\n\
                 ENDSEC;\nEND-ISO-10303-21;\n"
            ))
            .contains("defined twice")
        );
        assert!(
            code(&format!(
                "{ifc4}#99999999999=IFCCARTESIANPOINT((0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n"
            ))
            .contains("out of range")
        );
        assert!(
            code(&format!(
                "{ifc4}#1=IFCCARTESIANPOINT((1.E999,0.));\nENDSEC;\nEND-ISO-10303-21;\n"
            ))
            .contains("is not a number")
        );
        // Lists nested past any schema's need are refused, not recursed into.
        assert!(
            code(&format!(
                "{ifc4}#1=X({}{});\nENDSEC;\nEND-ISO-10303-21;\n",
                "(".repeat(10000),
                ")".repeat(10000)
            ))
            .contains("nest more than")
        );
        // A STEP file of another schema is not an IFC file.
        let other = read_ifc(
            format!(
                "{header}FILE_SCHEMA(('AP214'));\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n"
            ),
            &ImportOptions::default(),
        )
        .expect_err("another schema is refused");
        assert_eq!(other.code, ErrorCode::Unsupported);
        // The error names the line.
        let bad = read_ifc(
            format!("{ifc4}#1=IFCCARTESIANPOINT((0.,0.));\n#2=?;\n"),
            &ImportOptions::default(),
        )
        .expect_err("a broken instance is refused");
        assert_eq!(bad.code, ErrorCode::ParseFailure);
        assert_eq!(bad.context, "line 9");

        let missing = read_ifc_file(Path::new("/no/such/file.ifc"), &ImportOptions::default())
            .expect_err("no file is refused");
        assert_eq!(missing.code, ErrorCode::NotFound);
    }

    // A placement relative to itself, a reference to nothing, a
    // representation that is not one: read, the bad parts counted, never a
    // hang or a crash.
    #[test]
    fn a_file_that_breaks_the_schema_is_read_as_far_as_it_makes_sense() {
        let text = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('','',(''),(''),'','','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCCARTESIANPOINT((1.,2.,3.));
#2=IFCAXIS2PLACEMENT3D(#1,$,$);
#3=IFCLOCALPLACEMENT(#3,#2);
#4=IFCSHAPEREPRESENTATION($,'Axis','Curve3D',(#77,#1));
#5=IFCPRODUCTDEFINITIONSHAPE($,$,(#4,#88));
#6=IFCANNOTATION('1YvctVUKr0kugbFTf53O9L',$,'Loop',$,$,#3,#5);
#7=IFCWALL('2YvctVUKr0kugbFTf53O9L',$,'Nowhere',$,$,$,#99,$,$);
#8=IFCPIPESEGMENT('3YvctVUKr0kugbFTf53O9L',$,'Odd',$,$,#1,'text',$,$);
#9=IFCALIGNMENT('0ZvctVUKr0kugbFTf53O9L',$,'Empty',$,$,$,$,$);
#10=IFCRELNESTS('1ZvctVUKr0kugbFTf53O9L',$,$,$,#9,(#9,#10,#123));
#11=IFCMAPCONVERSION($,$,'east',$,$,$,$,$);
#12=IFCSIUNIT(*,.LENGTHUNIT.,.GIGA.,.PARSEC.);
#13=IFCUNITASSIGNMENT((#12,#12));
ENDSEC;
END-ISO-10303-21;
"#;
        let mut imported = read(text, &ImportOptions::default());
        assert_eq!(imported.products, 3);
        assert!(imported.entities.len() <= 4);
        assert!(!imported.warnings.is_empty());
        // What did come in is fit for the drawing, which checks every entity.
        let mut model = Model::default();
        let mut stack = CommandStack::new(&mut model, None);
        if let Some(command) = import_command(&mut imported, &model) {
            must(stack.execute(&mut model, command));
        }
    }
}

// ---- into the drawing ---------------------------------------------------------------

mod ifc_import_command {
    use super::*;

    #[test]
    fn the_import_is_one_undo_step_and_a_second_import_renames_its_alignment() {
        let mut model = Model::default();
        let mut stack = CommandStack::new(&mut model, None);
        let source = scenario();
        let text = exported_scenario(&source);

        let mut first = read(&text, &ImportOptions::default());
        let entities = first.entities.len();
        let command = import_command(&mut first, &model).expect("there is something to import");
        must(stack.execute(&mut model, command));
        assert_eq!(model.entities.size(), entities);
        assert!(model.alignments.contains("MC01"));
        assert!(model.layers.contains("Survey/Kerb"));
        assert!(model.layers.contains("Survey"));
        assert_eq!(stack.undo_count(), 1);

        let mut second = read(&text, &ImportOptions::default());
        let again = import_command(&mut second, &model).expect("there is something to import");
        must(stack.execute(&mut model, again));
        assert!(model.alignments.contains("MC01 (2)"));
        let last = second.warnings.last().expect("the rename is said");
        assert!(last.contains("MC01 (2)"), "{last}");
        assert_eq!(model.entities.size(), 2 * entities);

        must(stack.undo(&mut model));
        must(stack.undo(&mut model));
        assert_eq!(model.entities.size(), 0);
        assert!(!model.alignments.contains("MC01"));
        assert!(!model.layers.contains("Survey/Kerb"));

        let mut nothing = IfcImport::default();
        assert!(import_command(&mut nothing, &model).is_none());
    }
}
