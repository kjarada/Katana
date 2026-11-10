// The road model as the IFC export takes it (katana_app's ifc_verbs,
// road_model_input; docs/ifc.md, "Road model"): a cad BimModel made by hand -
// the corridor and road-object builders are other modules' and are not
// needed to judge the conversion - turned into the ifc layer's plain data
// field by field, and written. Both front ends call this one function
// (katana_cli's EXPORT and the window's), so a corridor's elements are the
// same objects whichever wrote the file.

use katana_app::ifc_verbs::road_model_input;
use katana_cad::bim::bim_object::{BimModel, BimObject, BimProblem, Quantity};
use katana_entity::Color;
use katana_geometry::{Point2, Point3, Polyline2, TriangleMesh};
use katana_ifc::{ExportInput, ExportOptions, IfcClass, RoadQuantity, write_ifc};

// A 10 x 1 x 0.5 slab: eight corners, two triangles a side, wound outwards.
fn slab() -> TriangleMesh {
    TriangleMesh {
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(10.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 0.5),
            Point3::new(10.0, 0.0, 0.5),
            Point3::new(10.0, 1.0, 0.5),
            Point3::new(0.0, 1.0, 0.5),
        ],
        faces: vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [3, 7, 6],
            [3, 6, 2],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ],
    }
}

fn pair(name: &str, value: &str) -> (String, String) {
    (String::from(name), String::from(value))
}

fn course() -> BimObject {
    BimObject {
        id: String::from("corridor/Main/course/AC14/left/1"),
        name: String::from("AC14 wearing course"),
        kind: String::from("pavement"),
        facility: String::from("Main"),
        part: String::from("CARRIAGEWAY"),
        ifc_class: String::from("IfcCourse"),
        predefined_type: String::from("PAVEMENT"),
        material: String::from("Asphalt AC14"),
        colour: Color {
            r: 30,
            g: 30,
            b: 30,
            a: 255,
        },
        mesh: slab(),
        plan: vec![Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 1.0),
            ],
            false,
        )],
        quantities: vec![
            Quantity {
                name: String::from("GrossVolume"),
                value: 5.0,
                unit: String::from("m3"),
            },
            Quantity {
                name: String::from("Length"),
                value: 10.0,
                unit: String::from("m"),
            },
        ],
        properties: vec![pair("Component", "lane"), pair("Thickness", "0.050")],
        layer: String::from("Road/Main/Pavement"),
    }
}

fn problem(source: &str, message: &str) -> BimProblem {
    BimProblem {
        source: String::from(source),
        message: String::from(message),
    }
}

mod ifc_road_model_input {
    use super::*;

    #[test]
    fn every_field_of_a_bim_object_reaches_the_element() {
        let model = BimModel {
            objects: vec![course()],
            ..BimModel::default()
        };
        let input = road_model_input(&model);
        assert_eq!(input.elements.len(), 1);
        let element = &input.elements[0];
        let expected = course();
        assert_eq!(element.id, expected.id);
        assert_eq!(element.name, expected.name);
        assert_eq!(element.kind, expected.kind);
        assert_eq!(element.facility, expected.facility);
        assert_eq!(element.part, expected.part);
        assert_eq!(
            element.ifc_class,
            IfcClass::new("IfcCourse", "PAVEMENT", "")
        );
        assert_eq!(element.material, expected.material);
        assert_eq!(element.colour, expected.colour);
        assert_eq!(element.mesh.vertices, expected.mesh.vertices);
        assert_eq!(element.mesh.faces, expected.mesh.faces);
        assert_eq!(element.plan, expected.plan);
        assert_eq!(element.quantities.len(), 2);
        assert_eq!(
            element.quantities[0],
            RoadQuantity::new("GrossVolume", 5.0, "m3")
        );
        assert_eq!(
            element.quantities[1],
            RoadQuantity::new("Length", 10.0, "m")
        );
        assert_eq!(element.properties, expected.properties);
        assert_eq!(element.layer, expected.layer);
        assert!(element.host.is_empty());
        assert!(element.ground.is_empty());
        assert!(input.problems.is_empty());
    }

    // A USERDEFINED class must say what it is (each class's CorrectPredefinedType
    // rule): the object's ObjectType property, in any letter case, else its kind
    // in capitals. A listed predefined type takes no object type.
    #[test]
    fn a_user_defined_class_is_its_object_type_property_else_its_kind_in_capitals() {
        let mut named = course();
        named.ifc_class = String::from("IfcWall");
        named.predefined_type = String::from("USERDEFINED");
        named.kind = String::from("wall");
        named.properties.push(pair("objecttype", "NOISEBARRIER"));
        let mut unnamed = named.clone();
        unnamed.id = String::from("object/wall-2");
        unnamed.properties.pop();
        let mut listed = course();
        listed.properties.push(pair("ObjectType", "ignored"));
        let model = BimModel {
            objects: vec![named, unnamed, listed],
            ..BimModel::default()
        };
        let input = road_model_input(&model);
        assert_eq!(input.elements.len(), 3);
        assert_eq!(input.elements[0].ifc_class.object_type, "NOISEBARRIER");
        assert_eq!(input.elements[1].ifc_class.object_type, "WALL");
        assert_eq!(input.elements[2].ifc_class.object_type, "");
    }

    // A feature's host - the course a marking is painted on - and the surface a
    // cut is taken from are properties of the object, read in any letter case;
    // "Surface" and "Target Surface" name the ground as "Ground" does, the first
    // of them given winning.
    #[test]
    fn a_features_host_and_ground_are_its_properties_in_any_letter_case() {
        let mut marking = course();
        marking.ifc_class = String::from("IfcSurfaceFeature");
        marking.predefined_type = String::from("LINEMARKING");
        marking.properties = vec![pair("HOST", "corridor/Main/course/AC14/left/1")];
        let mut cut = course();
        cut.ifc_class = String::from("IfcEarthworksCut");
        cut.predefined_type = String::from("CUT");
        cut.properties = vec![pair("target surface", "existing"), pair("Ground", "later")];
        let model = BimModel {
            objects: vec![marking, cut.clone()],
            ..BimModel::default()
        };
        let input = road_model_input(&model);
        assert_eq!(input.elements.len(), 2);
        assert_eq!(input.elements[0].host, "corridor/Main/course/AC14/left/1");
        assert!(input.elements[0].ground.is_empty());
        assert_eq!(input.elements[1].ground, "existing");
        // They are properties of the object still, written as it has them.
        assert_eq!(input.elements[1].properties, cut.properties);
    }

    // What the model could not build is said where it is, so the export's
    // warnings name the corridor or object.
    #[test]
    fn the_models_problems_say_where_they_are() {
        let model = BimModel {
            problems: vec![
                problem("corridor Main", "the alignment has no profile"),
                problem("", "no template is defined"),
            ],
            ..BimModel::default()
        };
        let input = road_model_input(&model);
        assert_eq!(
            input.problems,
            vec![
                String::from("corridor Main: the alignment has no profile"),
                String::from("no template is defined"),
            ]
        );
        assert!(input.elements.is_empty());
    }

    // Converted and written: the road, its part and the course, as the export
    // writes any road model; the model's problem among the warnings.
    #[test]
    fn a_converted_model_is_written_as_a_road_its_part_and_its_element() {
        let model = BimModel {
            objects: vec![course()],
            problems: vec![problem("corridor Main", "the profile ends at 8")],
        };
        let input = ExportInput {
            road: road_model_input(&model),
            ..ExportInput::default()
        };
        let options = ExportOptions {
            project_name: String::from("Test"),
            ..ExportOptions::default()
        };
        let written = match write_ifc(&input, &options) {
            Ok(written) => written,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(written.roads, 1);
        assert_eq!(written.road_parts, 1);
        assert_eq!(written.road_elements, 1);
        assert_eq!(written.classes.get("IfcCourse"), Some(&1));
        assert!(written.text.contains("=IFCROADPART("));
        assert!(written.text.contains("'Main CARRIAGEWAY'"));
        assert!(
            written.warnings.contains(&String::from(
                "the road model: corridor Main: the profile ends at 8"
            )),
            "{:?}",
            written.warnings
        );
    }
}
