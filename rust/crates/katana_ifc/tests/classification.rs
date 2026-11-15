//! Twin of tests/ifc/test_classification.cpp: which IFC class a service of an
//! AS 5488 schedule and a drawing entity become, by the tables of
//! classification.rs, each case worked from the schema's class definitions
//! and AS 5488.2 Table A.4.

pub mod support;

use katana_entity::entity::{
    Entity, EntityType, Geometry, PointGeometry, PropertyValue, TextGeometry, set_heights,
};
use katana_geometry::{Circle2, Point2, Polyline2};
use katana_ifc::{
    ClassificationRule, IfcClass, asset_type_code, classification_text, classify_entity,
    classify_utility_point, classify_utility_run, contains_word, default_classification_rules,
    service_system_for,
};
use katana_survey::subsurface::{UtilityAttributes, UtilityType};

fn service(
    utility: UtilityType,
    configuration: &str,
    material: &str,
    description: &str,
) -> UtilityAttributes {
    UtilityAttributes {
        r#type: utility,
        configuration: String::from(configuration),
        material: String::from(material),
        description: String::from(description),
        ..UtilityAttributes::default()
    }
}

fn with_feature(utility: UtilityType, feature: &str) -> UtilityAttributes {
    let mut out = UtilityAttributes {
        r#type: utility,
        ..UtilityAttributes::default()
    };
    out.fields
        .insert(String::from("AssetFeature"), String::from(feature));
    out
}

fn point(layer: &str, code: &str) -> Entity {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(0.0, 0.0),
        }),
        layer: String::from(layer),
        ..Entity::default()
    };
    if !code.is_empty() {
        entity.properties.insert(
            String::from("code"),
            PropertyValue::Text(String::from(code)),
        );
    }
    entity
}

fn polyline(layer: &str) -> Entity {
    Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(20.0, 5.0),
            ],
            false,
        )),
        layer: String::from(layer),
        ..Entity::default()
    }
}

fn cls(entity: &str, predefined: &str, object_type: &str) -> IfcClass {
    IfcClass::new(entity, predefined, object_type)
}

// ---- services ---------------------------------------------------------------------

mod ifc_utility_class {
    use super::*;

    // The type alone decides a run's class when nothing says what it is laid
    // in: AS 5488.2 Table A.4's fluid services are pipes, its cable services
    // cables.
    #[test]
    fn a_run_is_a_pipe_or_a_cable_by_its_type_and_its_system_is_the_type() {
        let water = classify_utility_run(&service(UtilityType::Water, "", "", ""));
        assert_eq!(water.element, cls("IfcPipeSegment", "RIGIDSEGMENT", ""));
        assert_eq!(water.system, "WATERSUPPLY");
        assert_eq!(water.reason, "type water");

        let sewer = classify_utility_run(&service(UtilityType::Sewer, "", "", ""));
        assert_eq!(sewer.element, cls("IfcPipeSegment", "RIGIDSEGMENT", ""));
        assert_eq!(sewer.system, "SEWAGE");

        let telco = classify_utility_run(&service(UtilityType::Telecommunications, "", "", ""));
        assert_eq!(telco.element, cls("IfcCableSegment", "CABLESEGMENT", ""));
        assert_eq!(telco.system, "COMMUNICATION");

        // Traffic signals are the control of traffic; IFC's SIGNAL is railway
        // signalling.
        assert_eq!(
            classify_utility_run(&service(UtilityType::IntelligentTransport, "", "", "")).system,
            "CONTROL"
        );
        assert_eq!(
            classify_utility_run(&service(UtilityType::FireService, "", "", "")).system,
            "FIREPROTECTION"
        );
        // IFC has no recycled water system: USERDEFINED, said what it is.
        let recycled = classify_utility_run(&service(UtilityType::RecycledWater, "", "", ""));
        assert_eq!(recycled.system, "USERDEFINED");
        assert_eq!(recycled.system_object_type, "RECYCLEDWATER");
        assert_eq!(recycled.element, cls("IfcPipeSegment", "RIGIDSEGMENT", ""));
    }

    // What a service is laid in decides before what it carries: the sample
    // schedule's E1 is "4 x 100 mm conduits", and a consumer asking for the
    // conduits must find them.
    #[test]
    fn conduits_culverts_troughs_and_fibre_are_the_classes_that_say_them() {
        let conduits = classify_utility_run(&service(
            UtilityType::Electricity,
            "4 x 100 mm conduits",
            "PVC conduits",
            "",
        ));
        assert_eq!(
            conduits.element,
            cls("IfcCableCarrierSegment", "CONDUITSEGMENT", "")
        );
        assert_eq!(conduits.system, "ELECTRICAL");
        assert_eq!(conduits.reason, "configuration \"4 x 100 mm conduits\"");

        let culvert = classify_utility_run(&with_feature(UtilityType::Stormwater, "Box Culvert"));
        assert_eq!(culvert.element, cls("IfcPipeSegment", "CULVERT", ""));
        assert_eq!(culvert.system, "STORMWATER");
        assert_eq!(culvert.reason, "AssetFeature \"Box Culvert\"");

        assert_eq!(
            classify_utility_run(&with_feature(UtilityType::Electricity, "Cable Trough")).element,
            cls("IfcCableCarrierSegment", "CABLETRUNKINGSEGMENT", "")
        );
        assert_eq!(
            classify_utility_run(&service(
                UtilityType::Telecommunications,
                "",
                "",
                "optic fibre 144 core"
            ))
            .element,
            cls("IfcCableSegment", "OPTICALCABLESEGMENT", "")
        );
        // A gas main in a sleeve described as a duct is still the gas main: a
        // fluid service is never a cable carrier.
        assert_eq!(
            classify_utility_run(&service(UtilityType::Gas, "in duct", "", "")).element,
            cls("IfcPipeSegment", "RIGIDSEGMENT", "")
        );
        // Whole words: "conductor" is not a conduit.
        assert_eq!(
            classify_utility_run(&service(
                UtilityType::Electricity,
                "",
                "copper conductor",
                ""
            ))
            .element,
            cls("IfcCableSegment", "CABLESEGMENT", "")
        );
    }

    // AS 5488's service of unknown type is exactly IfcDistributionFlowElement's
    // "an element of a distribution system, kind not said" - and not a proxy.
    #[test]
    fn a_service_of_unknown_kind_is_a_distribution_flow_element_unless_its_words_say_pipe() {
        let unknown = classify_utility_run(&service(UtilityType::Unknown, "", "", ""));
        assert_eq!(
            unknown.element,
            cls("IfcDistributionFlowElement", "", "UNKNOWN SERVICE")
        );
        assert_eq!(unknown.system, "NOTDEFINED");
        assert_eq!(
            classify_utility_run(&service(UtilityType::Unknown, "", "", "old pipe")).element,
            cls("IfcPipeSegment", "RIGIDSEGMENT", "")
        );
    }

    #[test]
    fn a_point_service_is_the_pit_valve_hydrant_or_marker_its_words_name() {
        let at = |utility: UtilityType, feature: &str| {
            classify_utility_point(&with_feature(utility, feature))
                .map_or_else(|| cls("none", "", ""), |result| result.element)
        };
        // Most specific first: a valve pit is a chamber, not a valve.
        assert_eq!(
            at(UtilityType::Water, "Valve Pit"),
            cls("IfcDistributionChamberElement", "VALVECHAMBER", "")
        );
        assert_eq!(
            at(UtilityType::Water, "Meter pit"),
            cls("IfcDistributionChamberElement", "METERCHAMBER", "")
        );
        assert_eq!(
            at(UtilityType::Sewer, "Manhole"),
            cls("IfcDistributionChamberElement", "MANHOLE", "")
        );
        assert_eq!(
            at(UtilityType::Telecommunications, "Pit"),
            cls("IfcDistributionChamberElement", "INSPECTIONPIT", "")
        );
        assert_eq!(
            at(UtilityType::Water, "Hydrant"),
            cls("IfcFireSuppressionTerminal", "FIREHYDRANT", "")
        );
        assert_eq!(
            at(UtilityType::Water, "Air Valve"),
            cls("IfcValve", "AIRRELEASE", "")
        );
        assert_eq!(
            at(UtilityType::Water, "Scour"),
            cls("IfcValve", "FLUSHING", "")
        );
        assert_eq!(
            at(UtilityType::Water, "Stop valve"),
            cls("IfcValve", "STOPCOCK", "")
        );
        assert_eq!(
            at(UtilityType::Gas, "Valve"),
            cls("IfcValve", "ISOLATING", "")
        );
        assert_eq!(
            at(UtilityType::Gas, "Marker post"),
            cls("IfcSign", "MARKER", "")
        );
        assert_eq!(
            at(UtilityType::Electricity, "Pillar"),
            cls("IfcJunctionBox", "POWER", "")
        );
        assert_eq!(
            at(UtilityType::Telecommunications, "Cabinet"),
            cls("IfcJunctionBox", "DATA", "")
        );
        // Nothing named: no class is claimed, and the point stays the survey
        // record it is.
        assert!(classify_utility_point(&service(UtilityType::Water, "", "", "")).is_none());
        assert!(classify_utility_point(&with_feature(UtilityType::Water, "Pipe")).is_none());
    }

    // AS 5488.2-2019 Table A.4, as a delivery schema's AssetTypeCode takes it.
    #[test]
    fn asset_type_codes_are_those_of_as5488_part2_table_a4() {
        assert_eq!(asset_type_code(UtilityType::Telecommunications), "C");
        assert_eq!(asset_type_code(UtilityType::Stormwater), "D");
        assert_eq!(asset_type_code(UtilityType::Electricity), "E");
        assert_eq!(asset_type_code(UtilityType::FireService), "F");
        assert_eq!(asset_type_code(UtilityType::Gas), "G");
        assert_eq!(asset_type_code(UtilityType::IntelligentTransport), "I");
        assert_eq!(asset_type_code(UtilityType::Fuel), "P");
        assert_eq!(asset_type_code(UtilityType::Sewer), "S");
        assert_eq!(asset_type_code(UtilityType::Water), "W");
        assert_eq!(asset_type_code(UtilityType::Unknown), "N");
        // The table has no code for these, and none is made up.
        assert_eq!(asset_type_code(UtilityType::RecycledWater), "");
        assert_eq!(asset_type_code(UtilityType::Other), "");
    }
}

// ---- drawing entities ---------------------------------------------------------------

mod ifc_entity_class {
    use super::*;

    #[test]
    fn the_text_is_the_layer_the_code_and_the_string_name_in_words_of_capitals() {
        let mut kerb = point("Survey/Kerb_line", "KB01");
        assert_eq!(classification_text(&kerb), "SURVEY KERB LINE KB01");
        kerb.metadata.insert(
            String::from("12d.name"),
            PropertyValue::Text(String::from("lip of kerb")),
        );
        assert_eq!(
            classification_text(&kerb),
            "SURVEY KERB LINE KB01 LIP OF KERB"
        );
    }

    #[test]
    fn words_match_whole_a_prefix_with_a_star_and_a_phrase_in_sequence() {
        assert!(contains_word("SURVEY KERB KB01", "KERB"));
        assert!(contains_word("SURVEY KERB KB01", "kerb"));
        assert!(!contains_word("SURVEY KERBSIDE", "KERB"));
        assert!(contains_word("SURVEY KERBSIDE", "KERB*"));
        assert!(contains_word("ELECTRICAL HV", "ELEC*"));
        assert!(!contains_word("SELECT", "ELEC*"));
        assert!(contains_word("ROAD EDGE OF SEAL", "EDGE OF SEAL"));
        assert!(!contains_word("ROAD EDGE SEAL OF", "EDGE OF SEAL"));
        assert!(!contains_word("ROAD", ""));
        // STORMWATER is one word: it is not WATER.
        assert!(!contains_word("STORMWATER PITS", "WATER"));
    }

    #[test]
    fn a_service_system_is_named_by_its_words_the_most_specific_first() {
        assert_eq!(service_system_for("STORMWATER PITS"), "STORMWATER");
        assert_eq!(service_system_for("SW PIPES"), "STORMWATER");
        assert_eq!(service_system_for("SEWER MH"), "SEWAGE");
        assert_eq!(service_system_for("RECYCLED WATER"), "RECYCLEDWATER");
        assert_eq!(service_system_for("WATER MAINS"), "WATERSUPPLY");
        assert_eq!(service_system_for("NBN PIT"), "COMMUNICATION");
        assert_eq!(service_system_for("ELECTRICAL"), "ELECTRICAL");
        assert_eq!(service_system_for("SURVEY KERB"), "");
    }

    #[test]
    fn the_default_rules_type_services_features_and_road_furniture() {
        let rules = default_classification_rules();
        let of = |entity: &Entity| classify_entity(entity, rules);

        let kerb = of(&polyline("Survey/Kerb"));
        assert_eq!(kerb.ifc_class, cls("IfcKerb", "NOTDEFINED", ""));
        assert_eq!(kerb.system, "");
        assert_eq!(kerb.rule, "kerb");

        assert_eq!(
            of(&polyline("Survey/Fence")).ifc_class,
            cls("IfcRailing", "FENCE", "")
        );
        assert_eq!(
            of(&polyline("Retaining Wall")).ifc_class,
            cls("IfcWall", "RETAININGWALL", "")
        );
        assert_eq!(
            of(&polyline("Contours/Major")).ifc_class,
            cls("IfcAnnotation", "CONTOURLINE", "")
        );

        // A point on a pits layer is a pit in the layer's system; a line on
        // the same layer is the service, not a pit.
        let pit = of(&point("Stormwater/Pits", ""));
        assert_eq!(
            pit.ifc_class,
            cls("IfcDistributionChamberElement", "INSPECTIONPIT", "")
        );
        assert_eq!(pit.system, "STORMWATER");
        let pipe = of(&polyline("Stormwater/Pits"));
        assert_eq!(pipe.ifc_class, cls("IfcPipeSegment", "RIGIDSEGMENT", ""));
        assert_eq!(pipe.system, "STORMWATER");

        let manhole = Entity {
            geometry: Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), 0.6)),
            layer: String::from("SEWER MH"),
            ..Entity::default()
        };
        assert_eq!(
            of(&manhole).ifc_class,
            cls("IfcDistributionChamberElement", "MANHOLE", "")
        );
        assert_eq!(of(&manhole).system, "SEWAGE");

        // A rule may say its system: a hydrant is fire protection wherever it
        // is.
        let hydrant = of(&point("Survey/Points", "FH"));
        assert_eq!(
            hydrant.ifc_class,
            cls("IfcFireSuppressionTerminal", "FIREHYDRANT", "")
        );
        assert_eq!(hydrant.system, "FIREPROTECTION");

        assert_eq!(
            of(&polyline("Comms/Conduits")).ifc_class,
            cls("IfcCableCarrierSegment", "CONDUITSEGMENT", "")
        );
        assert_eq!(of(&polyline("Comms/Conduits")).system, "COMMUNICATION");
        assert_eq!(
            of(&point("Geotech", "BH")).ifc_class,
            cls("IfcBorehole", "", "")
        );
    }

    // What no rule names is an IfcAnnotation of the kind it is - never a
    // proxy.
    #[test]
    fn what_no_rule_names_is_the_annotation_of_its_kind() {
        let rules = default_classification_rules();
        let of = |entity: &Entity| classify_entity(entity, rules).ifc_class;

        assert_eq!(
            of(&point("Survey/Points", "")),
            cls("IfcAnnotation", "SURVEY", "")
        );
        assert_eq!(
            of(&polyline("Linework")),
            cls("IfcAnnotation", "NOTDEFINED", "")
        );
        let mut surveyed = polyline("Linework");
        set_heights(&mut surveyed.properties, &[Some(20.0), Some(20.1), None]);
        assert_eq!(of(&surveyed), cls("IfcAnnotation", "SURVEY", ""));

        let text = Entity {
            geometry: Geometry::Text(TextGeometry {
                position: Point2::new(0.0, 0.0),
                text: String::from("LOT 42"),
                ..TextGeometry::default()
            }),
            ..Entity::default()
        };
        assert_eq!(of(&text), cls("IfcAnnotation", "TEXT", ""));

        // Across every kind and a spread of layer names, with the default
        // rules.
        for layer in [
            "0",
            "Survey/Kerb",
            "SEWER",
            "Stormwater/Pits",
            "Trees",
            "Proxy",
            "Building",
            "Misc",
        ] {
            for mut entity in [point(layer, ""), polyline(layer), text.clone()] {
                entity.layer = String::from(layer);
                assert_ne!(of(&entity).entity, "IfcBuildingElementProxy", "{layer}");
            }
        }
        for rule in rules {
            assert_ne!(
                rule.target.entity, "IfcBuildingElementProxy",
                "{}",
                rule.name
            );
        }
    }

    // A project whose layers say otherwise passes its own rules, which
    // replace the defaults.
    #[test]
    fn a_projects_own_rules_replace_the_defaults() {
        let rules = vec![
            ClassificationRule {
                name: String::from("light pole"),
                words: vec![String::from("POLE*"), String::from("LP")],
                kinds: vec![EntityType::Point],
                target: cls("IfcColumn", "COLUMN", ""),
                system: String::new(),
            },
            ClassificationRule {
                name: String::from("drainage line"),
                words: vec![String::from("DRN")],
                kinds: Vec::new(),
                target: cls("IfcPipeSegment", "RIGIDSEGMENT", ""),
                system: String::from("STORMWATER"),
            },
        ];
        let pole = classify_entity(&point("Lighting", "LP"), &rules);
        assert_eq!(pole.ifc_class, cls("IfcColumn", "COLUMN", ""));
        assert_eq!(pole.rule, "light pole");
        // The kind limits a rule: a line on a pole layer is not a pole.
        assert_eq!(
            classify_entity(&polyline("Poles"), &rules).ifc_class,
            cls("IfcAnnotation", "NOTDEFINED", "")
        );
        let drain = classify_entity(&polyline("DRN"), &rules);
        assert_eq!(drain.system, "STORMWATER");
        // The defaults are not consulted.
        assert_eq!(
            classify_entity(&polyline("Survey/Kerb"), &rules).ifc_class,
            cls("IfcAnnotation", "NOTDEFINED", "")
        );
    }
}
