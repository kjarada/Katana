//! tests/interop/test_round_trip.cpp: the round trip, on the fixtures.
//!
//! Import a real archive, put everything it gave into a model, export that,
//! and import the export: the second reading must hold what the first did.
//! This is the test that would have caught every loss the earlier slices
//! fixed one at a time - a style dropped, a colour overwritten, a symbol
//! turned into metadata and left there - because it compares the WHOLE of
//! what came back rather than the field someone thought to look at.

use std::collections::BTreeMap;

use katana_archive12d::{ExportMesh, ExportSurface};
use katana_entity::entity::{property_to_string, to_string};
use katana_entity::{Geometry, Model, PropertyMap, PropertyValue, Style};
use katana_interop::Archive12dImportResult;
use katana_interop::archive12d::{
    Archive12dExportOptions, Archive12dImportOptions, export_archive12d, import_archive12d,
};

use crate::{TempDir, archive_fixture};

/// Everything an import gave, put into a model the way the application does.
fn model_of(imported: &Archive12dImportResult) -> Model {
    let mut model = Model::default();
    for layer in &imported.layers_needed {
        assert!(model.layers.add(layer.clone()).is_ok(), "{}", layer.name);
    }
    for style in &imported.styles_needed {
        assert!(model.styles.add(style.clone()).is_ok(), "{}", style.name);
    }
    for entity in &imported.entities {
        assert!(model.entities.add(entity.clone()).is_ok());
    }
    for alignment in &imported.alignments {
        assert!(
            model.alignments.add(alignment.clone()).is_ok(),
            "{}",
            alignment.name
        );
    }
    model
}

/// What a string is, reduced to what must survive: everything the domain
/// carries about it except the id, which is Katana's and not the file's.
#[derive(Clone, Debug, PartialEq)]
struct Summary {
    layer: String,
    style: String,
    r#type: String,
    vertices: usize,
    properties: PropertyMap,
    metadata: PropertyMap,
}

fn vertex_count(geometry: &Geometry) -> usize {
    match geometry {
        Geometry::Point(_) => 1,
        Geometry::Line(_) => 2,
        Geometry::Polyline(polyline) => polyline.vertices.len(),
        Geometry::Arc(_) => 0,
        Geometry::Circle(_) => 0,
        Geometry::Text(_) => 1,
        Geometry::Dimension(_) => 2,
        Geometry::Label(_) => 0,
        Geometry::Leader(leader) => leader.vertices.len(),
        Geometry::CurvePolyline(polyline) => polyline.vertices.len(),
        Geometry::Ellipse(_) => 0,
        Geometry::Spline(_) => 0,
    }
}

/// `std::round(value * 1e8) / 1e8`.
fn to_eight_places(value: f64) -> f64 {
    (value * 1e8).round() / 1e8
}

/// std::from_chars over the whole item: a number in the general format, no
/// leading '+'.
fn numeric(item: &str) -> Option<f64> {
    if item.starts_with('+') {
        return None;
    }
    item.parse::<f64>().ok()
}

/// Keyed by name and position so the comparison does not depend on the order
/// the exporter happened to write things in: the C++'s multimap, its equal
/// keys in the order they were put in.
fn summarise(imported: &Archive12dImportResult) -> BTreeMap<String, Vec<Summary>> {
    let mut out: BTreeMap<String, Vec<Summary>> = BTreeMap::new();
    for entity in &imported.entities {
        let mut summary = Summary {
            layer: entity.layer.clone(),
            style: entity.style.clone(),
            r#type: String::from(to_string(entity.r#type())),
            vertices: vertex_count(&entity.geometry),
            properties: entity.properties.clone(),
            metadata: entity.metadata.clone(),
        };
        // Heights at the precision the FILE is written to. An archive
        // carries eight decimal places by default - its program's own
        // (WriteOptions::decimal_places) - so a height that has been through
        // the format differs from one computed in memory in the tenth digit,
        // which is a nanometre of ground. Comparing the text of the doubles
        // would call that a difference; rounding both sides to what the
        // format holds compares what the file actually says.
        for key in ["elevation", "elevations"] {
            match summary.properties.get_mut(key) {
                Some(PropertyValue::Real(one)) => *one = to_eight_places(*one),
                Some(PropertyValue::Text(list)) => {
                    let mut rounded = String::new();
                    for item in list.split(' ') {
                        if !rounded.is_empty() {
                            rounded.push(' ');
                        }
                        match numeric(item) {
                            Some(value) => rounded.push_str(&property_to_string(
                                &PropertyValue::Real(to_eight_places(value)),
                            )),
                            None => rounded.push_str(item),
                        }
                    }
                    *list = rounded;
                }
                _ => {}
            }
        }
        // The source file is the file it came from, which differs by design.
        summary.metadata.remove("source");
        let name = match summary.metadata.get("12d.name") {
            Some(PropertyValue::Text(name)) => name.clone(),
            Some(other) => panic!("12d.name is not text: {other:?}"),
            None => String::new(),
        };
        let key = format!("{name}|{}|{}", summary.r#type, summary.vertices);
        out.entry(key).or_default().push(summary);
    }
    out
}

fn styles_by_name(imported: &Archive12dImportResult) -> BTreeMap<String, Style> {
    let mut out = BTreeMap::new();
    for style in &imported.styles_needed {
        out.entry(style.name.clone())
            .or_insert_with(|| style.clone());
    }
    out
}

/// Import, export, import, export, import.
///
/// The FIRST export normalises: the format has several spellings of one thing
/// and Katana writes one of them. An arc goes out as a two-vertex super
/// string (docs/interop.md), a drainage string as its line plus a point per
/// pit, a full_tin as a tin. So the first reading and the second are not
/// expected to match string for string - and everything after that is, since
/// the drawing is then already in the form Katana writes.
///
/// What the first pass must not do is LOSE anything: a style, a symbol on a
/// style, a layer, a mesh, a mesh's face colours, or ground.
fn passes(fixture_name: &str, count: usize) -> Vec<Archive12dImportResult> {
    let directory = TempDir::new("katana-roundtrip-", fixture_name);
    let mut results = Vec::new();
    let current = import_archive12d(
        &archive_fixture(fixture_name),
        &Archive12dImportOptions::default(),
    );
    match current {
        Ok(current) => results.push(current),
        Err(error) => panic!("{}", error.describe()),
    }
    for pass in 1..count {
        let previous = results.last().expect("a reading to export");
        let model = model_of(previous);
        let surfaces: Vec<ExportSurface<'_>> = previous
            .surfaces
            .iter()
            .map(|surface| ExportSurface {
                name: surface.name.clone(),
                surface: Some(&surface.surface),
            })
            .collect();
        let meshes: Vec<ExportMesh<'_>> = previous
            .meshes
            .iter()
            .map(|mesh| ExportMesh {
                name: mesh.name.clone(),
                layer: mesh.layer.clone(),
                colour_name: mesh.colour_name.clone(),
                mesh: Some(&mesh.mesh),
                face_colour_names: mesh.face_colour_names.clone(),
            })
            .collect();
        let out = directory.file(&format!("pass{pass}.12da"));
        if let Err(error) = export_archive12d(
            &model,
            &surfaces,
            &out,
            &Archive12dExportOptions::default(),
            &meshes,
        ) {
            panic!("{}", error.describe());
        }
        match import_archive12d(&out, &Archive12dImportOptions::default()) {
            Ok(next) => results.push(next),
            Err(error) => panic!("{}", error.describe()),
        }
    }
    results
}

fn round_trip(fixture_name: &str) {
    let results = passes(fixture_name, 3);
    assert_eq!(results.len(), 3, "{fixture_name}");
    let first = &results[0];
    let second = &results[1];
    let third = &results[2];

    // ---- nothing is lost on the way out ------------------------------------
    // By name, not in order: the list is in the order the styles were first
    // met, and the export writes the elements in its own order.
    assert_eq!(
        styles_by_name(second),
        styles_by_name(first),
        "{fixture_name}: every style, with its symbol and size, came back"
    );
    assert_eq!(
        second.layers_needed.len(),
        first.layers_needed.len(),
        "{fixture_name}: layers"
    );
    assert_eq!(
        second.meshes.len(),
        first.meshes.len(),
        "{fixture_name}: meshes"
    );
    for (before, after) in first.meshes.iter().zip(&second.meshes) {
        assert_eq!(after.mesh.faces, before.mesh.faces, "mesh {}", before.name);
        assert_eq!(after.mesh.vertices, before.mesh.vertices);
        assert_eq!(after.face_colour_names, before.face_colour_names);
    }
    assert!(
        second.entities.len() >= first.entities.len(),
        "{fixture_name}: a string may become two (a drainage line and its pits), never none"
    );
    assert!(
        second.surfaces.len() >= first.surfaces.len(),
        "{fixture_name}: surfaces"
    );
    for (before, after) in first.surfaces.iter().zip(&second.surfaces) {
        assert_eq!(
            after.surface.triangle_count(),
            before.surface.triangle_count(),
            "surface {}",
            before.name
        );
    }

    // ---- and the form Katana writes is stable ------------------------------
    // This is the assertion with teeth: once the drawing is in Katana's own
    // spelling, every further trip through the format must change nothing at
    // all. A field quietly dropped or added on export shows up here even
    // when the first pass could excuse it as normalisation.
    assert_eq!(
        third.entities.len(),
        second.entities.len(),
        "{fixture_name}"
    );
    assert_eq!(
        styles_by_name(third),
        styles_by_name(second),
        "{fixture_name}"
    );
    assert_eq!(
        third.layers_needed.len(),
        second.layers_needed.len(),
        "{fixture_name}"
    );
    assert_eq!(
        third.tally, second.tally,
        "{fixture_name}: the same elements in the same numbers"
    );
    assert_eq!(third.meshes.len(), second.meshes.len(), "{fixture_name}");
    for (before, after) in second.meshes.iter().zip(&third.meshes) {
        assert_eq!(after.mesh.faces, before.mesh.faces);
        assert_eq!(after.face_colour_names, before.face_colour_names);
    }

    let before = summarise(second);
    let after = summarise(third);
    let count = |summaries: &BTreeMap<String, Vec<Summary>>| -> usize {
        summaries.values().map(Vec::len).sum()
    };
    assert_eq!(count(&before), count(&after), "{fixture_name}");
    for (key, summaries) in &before {
        for summary in summaries {
            let found = after
                .get(key)
                .is_some_and(|candidates| candidates.contains(summary));
            assert!(
                found,
                "{fixture_name}: no string came back as: {key} on layer {} in style {}",
                summary.layer, summary.style
            );
        }
    }
}

mod round_trip {
    use super::*;

    #[test]
    fn every_geometry_comes_back_as_it_went() {
        round_trip("all_geometries.12da");
    }

    #[test]
    fn a_drainage_string_comes_back_with_its_pipes_and_pits() {
        round_trip("drainage_strings.12da");
    }

    #[test]
    fn symbols_come_back_on_the_styles_that_carry_them() {
        round_trip("symbols.12da");
    }

    #[test]
    fn several_tins_come_back_as_several_surfaces() {
        round_trip("multiple_tins.12da");
    }

    #[test]
    fn a_super_tin_comes_back_with_its_members() {
        round_trip("super_tin.12da");
    }

    #[test]
    fn a_zipped_archive_round_trips_as_its_plain_form_does() {
        round_trip("all_geometries.12daz");
    }
}
