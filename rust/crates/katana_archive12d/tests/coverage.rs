//! The twin of tests/archive12d/test_coverage.cpp: every row of the coverage
//! table is an element the reader knows, and the handling it claims is the
//! handling it gets.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

use std::collections::{BTreeMap, BTreeSet};

use katana_archive12d::coverage::{Handling, element_coverage};
use katana_archive12d::{
    DomainImport, ImportOptions, ReadOptions, element_keyword, read_archive, to_domain,
};

// The smallest instance of each element the manual defines that still says
// something - one per row of element_coverage(). Keyed by the row's keyword.
fn minimal_instances() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("model", "model \"M\" string super { data_2d { 0 0 } }"),
        ("colour", "colour blue string super { data_2d { 0 0 } }"),
        ("style", "style \"Kerb\" string super { data_2d { 0 0 } }"),
        (
            "breakline",
            "breakline line string super { data_2d { 0 0 } }",
        ),
        ("null", "null -1 string super { data_3d { 0 0 -1 } }"),
        (
            "attributes",
            "string super { attributes { text \"a\" \"b\" } data_2d { 0 0 } }",
        ),
        (
            "project_attributes",
            "project_attributes { text \"a\" \"b\" }",
        ),
        (
            "drafting_element/leader",
            "drafting_element { leader { arrow_x 0 arrow_y 0 hook_x 3 hook_y 4 display_text Note } }",
        ),
        (
            "tin",
            "tin { name \"T\" points { 0 0 0 1 0 0 0 1 0 } triangles { 1 3 2 } }",
        ),
        (
            "full_tin",
            "full_tin { name \"T\" points { 0 0 0 0 9 0 9 9 0 9 0 0 1 1 1 2 1 1 1 2 1 } \
             triangles { 5 7 6 } neighbours { 0 0 0 } nulling { 2 } }",
        ),
        ("super_tin", "super_tin { name \"S\" tins { \"T\" } }"),
        (
            "primitive_3d",
            "primitive_3d { name M colour red trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 } \
             faces { 1 2 3 } } }",
        ),
        (
            "string arc",
            "string arc { radius 1 xcentre 0 ycentre 0 zcentre 0 xstart 1 ystart 0 zstart 0 \
             xend 0 yend 1 zend 0 }",
        ),
        (
            "string circle",
            "string circle { radius 1 xcentre 0 ycentre 0 zcentre 0 }",
        ),
        (
            "string drainage",
            "string drainage { data { 0 0 0 0 0  1 1 0 0 0 } pit { name \"P\" x 0 y 0 z 1 } }",
        ),
        (
            "string face",
            "string face { data { 0 0 0  1 0 0  1 1 0 } }",
        ),
        (
            "string feature",
            "string feature { radius 1 xcentre 0 ycentre 0 zcentre 0 }",
        ),
        (
            "string interface",
            "string interface { data { 0 0 0 -1  1 1 0 1 } }",
        ),
        (
            "string plot_frame",
            "string plot_frame { width 420 height 297 scale 500 xorigin 0 yorigin 0 }",
        ),
        ("string super", "string super { data_3d { 0 0 0  1 1 0 } }"),
        (
            "string super_alignment",
            "string super_alignment { name \"A\" horizontal_parts { \
             ip { id 100 x 0 y 0 } ip { id 200 x 9 y 0 } } }",
        ),
        ("string text", "string text { x 0 y 0 z 0 text \"T\" }"),
        ("string 2d", "string 2d { z 1 data { 0 0  1 1 } }"),
        ("string 3d", "string 3d { data { 0 0 0  1 1 0 } }"),
        (
            "string 4d",
            "string 4d { data { 0 0 0 \"a\"  1 1 0 \"b\" } }",
        ),
        (
            "string pipe",
            "string pipe { diameter 0.3 data { 0 0 0  1 1 0 } }",
        ),
        (
            "string polyline",
            "string polyline { data { 0 0 0 0 0  1 1 0 0 0 } }",
        ),
        (
            "string alignment",
            "string alignment { name \"A\" hipdata { 0 0 0  9 0 0 } }",
        ),
        (
            "string pipeline",
            "string pipeline { name \"P\" diameter 0.3 hipdata { 0 0 0  9 0 0 } }",
        ),
        (
            "string las_cloud_data",
            "string las_cloud_data { data { format v10_p0 points_v10_p0 { p { x 0 y 0 z 0 } } } }",
        ),
    ])
}

fn things_imported(domain: &DomainImport) -> usize {
    domain.entities.len()
        + domain.alignments.len()
        + domain.surfaces.len()
        + domain.clouds.len()
        + domain.meshes.len()
}

mod coverage {
    use super::*;

    #[test]
    fn every_element_of_the_format_is_read_and_none_is_left_unrecognised() {
        let instances = minimal_instances();
        let mut seen = BTreeSet::new();
        for row in element_coverage() {
            let keyword = row.keyword;
            assert!(seen.insert(keyword), "{keyword} is listed twice");
            let instance = instances.get(keyword).unwrap_or_else(|| {
                panic!("{keyword} is in the coverage table but this test has no instance of it")
            });

            let archive = read_archive(instance, &ReadOptions::default())
                .unwrap_or_else(|error| panic!("{keyword}: {}", error.describe()));
            assert!(
                archive.unrecognised.is_empty(),
                "{keyword} is claimed as covered but the reader skipped {:?}",
                archive.unrecognised.keys().next()
            );
            assert!(
                archive.warnings.is_empty(),
                "{keyword}: {:?}",
                archive.warnings.first()
            );

            // An element that IS one (rather than a command or a block inside
            // one) must come out under its own keyword.
            let is_element = keyword.starts_with("string ")
                || keyword == "tin"
                || keyword == "full_tin"
                || keyword == "super_tin"
                || keyword == "primitive_3d";
            if is_element {
                assert_eq!(archive.elements.len(), 1, "{keyword}");
                assert_eq!(element_keyword(&archive.elements[0]), keyword);
            }

            // And the handling the table claims is the handling it gets.
            let domain = to_domain(&archive, &ImportOptions::default())
                .unwrap_or_else(|error| panic!("{keyword}: {}", error.describe()));
            if row.handling == Handling::ReadOnly {
                assert_eq!(
                    things_imported(&domain),
                    0,
                    "{keyword} is listed as read-only"
                );
            } else {
                assert!(
                    things_imported(&domain) > 0,
                    "{keyword} is listed as imported"
                );
            }
        }
        assert_eq!(
            seen.len(),
            instances.len(),
            "this test holds an instance of an element the coverage table does not list"
        );
    }

    #[test]
    fn the_table_lists_every_string_type_the_manual_defines() {
        // Manual 1.5: the current types 1.5.1 - 1.5.10, the superseded 1.5.11
        // - 1.5.17, and the LAS cloud 1.5.18 - eighteen in all.
        let mut strings = 0;
        let mut sections = BTreeSet::new();
        for row in element_coverage() {
            if row.keyword.starts_with("string ") {
                strings += 1;
                assert!(
                    sections.insert(String::from(row.manual_section)),
                    "two string types claim section {}",
                    row.manual_section
                );
            }
        }
        assert_eq!(strings, 18);
        for section in 1..=18 {
            let wanted = format!("1.5.{section}");
            assert!(sections.contains(&wanted), "{wanted}");
        }
    }
}
