//! The twin of tests/archive12d/test_drafting_leaders.cpp: drafting leaders
//! read, written back whole, and imported as native leaders.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use std::f64::consts::PI;

use katana_archive12d::{
    Archive, DomainImport, DraftingLeader, ImportOptions, ReadOptions, WriteOptions, read_archive,
    to_domain, write_archive,
};
use katana_entity::geometry_blob::{geometry_from_blob, geometry_to_blob};
use katana_entity::serialization::{geometry_from_json, geometry_to_json};
use katana_entity::{Geometry, LeaderGeometry, PointGeometry};
use katana_geometry::{Point2, Vec2};

use support::{double_eq, fixture, holds, prop_text, shape};

fn read_text(text: impl AsRef<[u8]>) -> Archive {
    read_archive(text, &ReadOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn import_archive(archive: &Archive) -> DomainImport {
    to_domain(archive, &ImportOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn warns(imported: &DomainImport, text: &str) -> bool {
    imported
        .warnings
        .iter()
        .any(|warning| warning.contains(text))
}

fn leader_of(geometry: &Geometry) -> LeaderGeometry {
    match geometry {
        Geometry::Leader(leader) => leader.clone(),
        other => panic!("not a leader: {other:?}"),
    }
}

mod archive_drafting {
    use super::*;

    #[test]
    fn leader_blocks_are_read_and_rewritten_without_losing_their_fields() {
        let archive = read_text(fixture("drafting_leaders.12da"));
        assert_eq!(archive.elements.len(), 4);
        assert!(archive.unrecognised.is_empty());
        let leader = support::get::<DraftingLeader>(&archive.elements[2]);
        assert_eq!(leader.header.model, "Test/Notes");
        assert_eq!(leader.arrow.x, 100.0);
        assert_eq!(leader.hook.y, 208.0);
        assert_eq!(
            leader.fields.text("display_text", ""),
            "Service note\\nFirst line"
        );
        assert_eq!(leader.overrides.real("text_size"), Some(1.25));
        let again = read_text(write_archive(&archive, &WriteOptions::default()));
        assert_eq!(again.elements, archive.elements);
    }

    #[test]
    fn imported_leaders_keep_their_notes_paper_sizes_sides_and_local_placement() {
        let archive = read_text(fixture("drafting_leaders.12da"));
        let options = ImportOptions {
            origin_shift: Some(Vec2::new(100.0, 200.0)),
            ..ImportOptions::default()
        };
        let imported =
            to_domain(&archive, &options).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(imported.entities.len(), 4);
        let left = shape::<LeaderGeometry>(&imported.entities[2].geometry);
        let right = shape::<LeaderGeometry>(&imported.entities[3].geometry);
        assert_eq!(
            left.vertices,
            vec![Point2::new(0.0, 0.0), Point2::new(0.0, 8.0)]
        );
        assert_eq!(left.text, "Service note\nFirst line");
        double_eq(left.paper_height, 1.25);
        double_eq(left.arrow_size, 1.0);
        double_eq(left.landing, 2.5);
        assert_eq!(left.landing_angle, Some(0.0));
        assert_eq!(
            right.text, "Other note",
            "display text, not an unevaluated external template"
        );
        assert_eq!(right.landing_angle, Some(PI));
        double_eq(right.paper_height, 2.0);
        double_eq(right.arrow_size, 2.0);
        double_eq(right.landing, 5.0);
        assert_eq!(imported.entities[2].layer, "Test/Notes");
        let tally = imported
            .tally
            .iter()
            .find(|row| row.keyword == "drafting_element/leader")
            .expect("a tally of the leaders");
        assert_eq!(tally.read, 2);
        assert_eq!(tally.imported, 2);
        assert!(!warns(&imported, "DIFFERENT symbol"));
        let blob = geometry_to_blob(&Geometry::Leader(left.clone()))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let stored =
            geometry_from_blob(&blob).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(leader_of(&stored), *left);
        let json = geometry_to_json(&Geometry::Leader(right.clone()))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let restored =
            geometry_from_json(&json).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(leader_of(&restored), *right);
    }

    #[test]
    fn missing_or_non_finite_coordinates_do_not_create_an_invented_leader_at_the_origin() {
        for coordinates in [
            "arrow_y 2 hook_x 3 hook_y 4",
            "arrow_x nan arrow_y 2 hook_x 3 hook_y 4",
            "arrow_x 1 arrow_y 2 hook_x inf hook_y 4",
        ] {
            let archive = read_text(format!(
                "drafting_element {{ leader {{ {coordinates} }} }} string super {{ data_2d {{ 7 9 }} }}"
            ));
            let imported = import_archive(&archive);
            assert_eq!(imported.entities.len(), 1);
            assert!(holds::<PointGeometry>(&imported.entities[0].geometry));
            assert!(!imported.warnings.is_empty());
        }
    }

    #[test]
    fn invalid_sizes_angles_justification_and_coincident_tips_are_reported() {
        for fields in [
            "hook_angle nan",
            "justify_x Unknown",
            "overrides { text_size -1 }",
            "overrides { arrow_size inf }",
            "overrides { hook_length nope }",
        ] {
            let archive = read_text(format!(
                "drafting_element {{ leader {{ arrow_x 0 arrow_y 0 hook_x 3 hook_y 4 {fields} }} }}"
            ));
            let imported = import_archive(&archive);
            assert!(imported.entities.is_empty(), "{fields}");
            assert!(!imported.warnings.is_empty(), "{fields}");
        }
        for coordinates in [
            "arrow_x 1 arrow_y 2 hook_x 1 hook_y 2",
            "arrow_x -1e308 arrow_y 0 hook_x 1e308 hook_y 0",
        ] {
            let archive = read_text(format!("drafting_element {{ leader {{ {coordinates} }} }}"));
            let imported = import_archive(&archive);
            assert!(imported.entities.is_empty());
            assert!(!imported.warnings.is_empty());
        }
    }

    #[test]
    fn empty_notes_and_zero_sizes_are_valid_and_unknown_nested_blocks_are_counted() {
        let archive = read_text(
            r#"
        drafting_element {
          leader {
            arrow_x 1 arrow_y 2 hook_x 3 hook_y 4 hook_angle 90
            display_text ""
            overrides { arrow_size 0 hook_length 0 text_size 0 }
            future_setting { value 1 }
          }
        }"#,
        );
        assert_eq!(
            archive.unrecognised["drafting_element/leader/future_setting"],
            1
        );
        let imported = import_archive(&archive);
        assert_eq!(imported.entities.len(), 1);
        let leader = shape::<LeaderGeometry>(&imported.entities[0].geometry);
        assert!(leader.text.is_empty());
        assert_eq!(leader.landing_angle, Some(PI / 2.0));
        double_eq(leader.arrow_size, 0.0);
        double_eq(leader.landing, 0.0);
        double_eq(leader.paper_height, 0.0);
    }

    #[test]
    fn missing_and_duplicate_leader_blocks_are_not_reported_as_successful_imports() {
        let empty = read_text("drafting_element { time_created \"today\" }");
        assert!(empty.elements.is_empty());
        assert!(!empty.warnings.is_empty());
        let duplicate = read_archive(
            "drafting_element { leader { arrow_x 0 arrow_y 0 hook_x 1 hook_y 1 } \
             leader { arrow_x 1 arrow_y 1 hook_x 2 hook_y 2 } }",
            &ReadOptions::default(),
        );
        assert!(duplicate.is_err());
        let truncated = read_archive(
            "drafting_element { leader { arrow_x 0",
            &ReadOptions::default(),
        );
        assert!(truncated.is_err());
    }

    #[test]
    fn external_arrow_references_are_retained_without_guessing_a_target_entity() {
        let archive = read_text(
            r#"
        drafting_element {
          leader {
            arrow_x 1 arrow_y 2 hook_x 3 hook_y 4 display_text "Snapshot"
            arrow {
              computator_horz_point_reference {
                valid true
                computator_horz_reference {
                  valid true direction 1
                  reference { model_name "Original services" model_id 12 name "Service 8" id 81 }
                  part { part 3 }
                }
              }
            }
          }
        }"#,
        );
        assert!(archive.unrecognised.is_empty());
        let again = read_text(write_archive(&archive, &WriteOptions::default()));
        assert_eq!(again.elements, archive.elements);
        let imported = import_archive(&archive);
        assert_eq!(imported.entities.len(), 1);
        let entity = &imported.entities[0];
        let leader = shape::<LeaderGeometry>(&entity.geometry);
        assert!(!leader.tip_ref.associated());
        assert_eq!(leader.vertices[0], Point2::new(1.0, 2.0));
        assert_eq!(
            prop_text(
                &entity.metadata,
                "12d.leader.arrow.computator_horz_point_reference[0].computator_horz_reference[0].reference[0].name"
            ),
            "Service 8"
        );
        assert!(warns(&imported, "external arrow references"));
    }
}
