//! IMPORT and EXPORT of a .dgn through the one executor (dgn_verbs.rs,
//! import_verb.rs, export_verb.rs) and through MCP's katana_import and
//! katana_export. Expected values are the drawing's own: a 30 x 20 rectangle
//! (area 600), a circle of radius 5 and a text, on one layer.

use katana_app::dgn_verbs::{SEED_2D, SEED_3D};
use katana_core::ErrorCode;
use katana_core::json::Json;
use katana_dgn::seed_info;
use katana_entity::Geometry;

use crate::geo_fixture::{GeoFixture, holds, must, refused};
use crate::mcp_client::{Client, is_error, object, text_of};

fn fixture(name: &str) -> GeoFixture {
    GeoFixture::new("katana-dgn-", name)
}

/// The drawing of the command line's DXF and DGN round trips, on
/// Survey/Kerb in red.
fn drawn(name: &str) -> GeoFixture {
    let mut fixture = fixture(name);
    for line in [
        "LAYER NEW Survey/Kerb #FF0000",
        "LAYER SET Survey/Kerb",
        "RECT 0,0 30,20",
        "CIRCLE 15,10 5",
        "TEXT 5,5 2.5 \"LOT 42\"",
    ] {
        fixture.type_line(line);
    }
    fixture
}

fn field(reply: &str, kind: &str, key: &str) -> String {
    let record = GeoFixture::record(reply, kind).unwrap_or_else(|| panic!("no {kind} in:\n{reply}"));
    String::from(record.get(key).unwrap_or_else(|| panic!("no {key} in:\n{reply}")))
}

/// The tally record of `element`: (read, imported).
fn tally(reply: &str, element: &str) -> (String, String) {
    let record = katana_app::geo::replies::parse_records(reply)
        .into_iter()
        .find(|record| record.kind == "tally" && record.get("element") == Some(element))
        .unwrap_or_else(|| panic!("no tally of {element} in:\n{reply}"));
    (
        String::from(record.get("read").unwrap_or_default()),
        String::from(record.get("imported").unwrap_or_default()),
    )
}

#[test]
fn gdal_ships_the_seed_designs_the_export_writes_on() {
    // GDAL's DGN writer writes on these (its ogrdgndatasource.cpp); so does
    // EXPORT of a .dgn given no seed=.
    for (name, three_d) in [(SEED_2D, false), (SEED_3D, true)] {
        let path = katana_gis::gdal_data_file(name)
            .unwrap_or_else(|| panic!("GDAL's data folder has no {name}"));
        let bytes = std::fs::read(&path).expect("readable");
        let info = must(seed_info(&bytes));
        assert_eq!(info.three_d, three_d, "{name}");
    }
}

#[test]
fn a_drawing_exported_and_imported_is_one_undo_step_on_its_level() {
    let mut drawing = drawn("round-trip");
    let file = drawing.scratch.file("site.dgn");
    let exported = drawing.ok(&format!("EXPORT \"{file}\""));
    assert_eq!(field(&exported, "exported", "kind"), "dgn");
    assert_eq!(field(&exported, "exported", "format"), "DGN V7 2D");
    assert_eq!(field(&exported, "exported", "entities"), "3");
    assert_eq!(field(&exported, "exported", "levels"), "1");
    assert_eq!(field(&exported, "exported", "units"), "m");
    // The drawing reaches 16 m from its middle with the margin: 0.1 mm.
    assert_eq!(field(&exported, "exported", "uor_per_master"), "10000");
    assert_eq!(field(&exported, "level", "number"), "1");
    assert_eq!(field(&exported, "level", "layer"), "Survey/Kerb");

    let mut fresh = fixture("round-trip-in");
    let imported = fresh.ok(&format!("IMPORT \"{file}\""));
    assert_eq!(field(&imported, "imported", "kind"), "dgn");
    assert_eq!(field(&imported, "imported", "entities"), "3");
    assert_eq!(field(&imported, "imported", "layers"), "1");
    // The rectangle is a shape, the circle an ellipse element, the text a
    // text.
    assert_eq!(tally(&imported, "Shape"), (String::from("1"), String::from("1")));
    assert_eq!(tally(&imported, "Ellipse"), (String::from("1"), String::from("1")));
    assert_eq!(tally(&imported, "Text"), (String::from("1"), String::from("1")));
    let on = fresh.on("level 1");
    assert_eq!(on.len(), 3);
    let area = on
        .iter()
        .find_map(|entity| match &entity.geometry {
            Geometry::Polyline(polyline) => Some(polyline.area()),
            _ => None,
        })
        .expect("the rectangle");
    assert_eq!(area, 600.0);
    // One step: one undo takes the entities and their layer.
    must(fresh.document.undo());
    assert_eq!(fresh.entity_count(), 0);
    assert!(fresh.document.model().layers.find("level 1").is_none());
}

#[test]
fn import_local_moves_the_design_as_one_piece() {
    let mut drawing = fixture("local");
    drawing.type_line("RECT 330000,6250000 330030,6250020");
    let file = drawing.scratch.file("mga.dgn");
    drawing.ok(&format!("EXPORT \"{file}\""));
    let mut fresh = fixture("local-in");
    let imported = fresh.ok(&format!("IMPORT \"{file}\" LOCAL"));
    assert_eq!(field(&imported, "placed", "placement"), "local");
    let bounds = fresh.document.model().entities.bounds();
    assert_eq!((bounds.min.x, bounds.min.y), (0.0, 0.0));
    assert_eq!((bounds.max.x, bounds.max.y), (30.0, 20.0));
}

#[test]
fn the_shared_scope_takes_part_of_the_drawing_and_the_reply_says_what_it_took() {
    let mut drawing = fixture("scope");
    for line in [
        "LAYER NEW A",
        "LAYER SET A",
        "RECT 0,0 10,10",
        "LAYER NEW B",
        "LAYER SET B",
        "RECT 20,0 30,10",
        "CIRCLE 25,5 2",
    ] {
        drawing.type_line(line);
    }
    let file = drawing.scratch.file("a.dgn");
    let exported = drawing.ok(&format!("EXPORT \"{file}\" LAYERS A"));
    assert_eq!(field(&exported, "scope", "matched"), "1");
    assert_eq!(field(&exported, "exported", "entities"), "1");
    // A scope that takes nothing writes nothing, and says so.
    // AREA x0,y0,x1,y1: the shared scope's one word (scope_verbs.rs), four numbers.
    let none = drawing.ok(&format!(
        "EXPORT \"{}\" AREA 100,100,110,110",
        drawing.scratch.file("none.dgn")
    ));
    holds(&none, "ran=no");
    assert!(!std::path::Path::new(&drawing.scratch.file("none.dgn")).exists());
    let mut fresh = fixture("scope-in");
    let imported = fresh.ok(&format!("IMPORT \"{file}\""));
    assert_eq!(field(&imported, "imported", "entities"), "1");
}

#[test]
fn a_file_written_is_the_seed_of_another_whose_units_it_keeps() {
    let mut drawing = drawn("seed");
    let first = drawing.scratch.file("first.dgn");
    drawing.ok(&format!("EXPORT \"{first}\""));
    // A seed of 1000 units a metre would show here as 1000; this one is the
    // first file, at 10 000, and the second keeps it.
    let second = drawing.scratch.file("second.dgn");
    let exported = drawing.ok(&format!("EXPORT \"{second}\" seed=\"{first}\""));
    assert_eq!(field(&exported, "exported", "uor_per_master"), "10000");
    let mut fresh = fixture("seed-in");
    let imported = fresh.ok(&format!("IMPORT \"{second}\""));
    assert_eq!(field(&imported, "imported", "entities"), "3");
}

#[test]
fn seed_is_a_dgns_alone_and_gdals_options_are_not_a_dgns() {
    let mut drawing = drawn("refusals");
    let error = refused(
        drawing.run(&format!("EXPORT \"{}\" seed=x.dgn", drawing.scratch.file("a.gpkg"))),
        "seed= on a GeoPackage",
    );
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    holds(&error.message, "seed=");
    let error = refused(
        drawing.run(&format!(
            "EXPORT \"{}\" seed=\"{}\"",
            drawing.scratch.file("a.dgn"),
            drawing.scratch.file("no such seed.dgn")
        )),
        "a seed that is not there",
    );
    assert_eq!(error.code, ErrorCode::NotFound);
    let error = refused(
        drawing.run(&format!("EXPORT \"{}\" crs=project", drawing.scratch.file("a.dgn"))),
        "crs= on a .dgn",
    );
    holds(&error.message, "is an option of a GDAL format");
    let error = refused(
        drawing.run(&format!("IMPORT \"{}\" crs=project", drawing.scratch.file("a.dgn"))),
        "crs= on a .dgn import",
    );
    holds(&error.message, "not an option of a dgn import");
}

#[test]
fn preview_says_what_would_be_written_and_writes_nothing() {
    let mut drawing = drawn("preview");
    let file = drawing.scratch.file("preview.dgn");
    let reply = drawing.ok(&format!("EXPORT \"{file}\" PREVIEW"));
    assert_eq!(field(&reply, "export", "kind"), "dgn");
    assert_eq!(field(&reply, "export", "entities"), "3");
    assert!(!std::path::Path::new(&file).exists());
}

#[test]
fn a_v8_design_and_a_dxf_named_dgn_are_refused_by_name() {
    let mut fixture = fixture("v8");
    // An OLE compound document's signature, as every V8 design begins.
    let mut v8 = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    v8.resize(4096, 0);
    let v8_file = fixture.scratch.file("connect.dgn");
    std::fs::write(&v8_file, &v8).expect("written");
    let error = refused(fixture.run(&format!("IMPORT \"{v8_file}\"")), "a V8 design");
    assert_eq!(error.code, ErrorCode::Unsupported);
    holds(&error.message, "V8");
    holds(&error.message, "save it as V7");
    let dxf_file = fixture.scratch.file("drawing.dgn");
    std::fs::write(&dxf_file, "  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n  0\nEOF\n").expect("written");
    let error = refused(fixture.run(&format!("IMPORT \"{dxf_file}\"")), "a DXF named .dgn");
    holds(&error.message, "DXF file named .dgn");
    let empty = fixture.scratch.file("empty.dgn");
    std::fs::write(&empty, b"").expect("written");
    let error = refused(fixture.run(&format!("IMPORT \"{empty}\"")), "an empty file");
    holds(&error.message, "empty");
    assert_eq!(fixture.entity_count(), 0);
}

#[test]
fn mcp_export_and_import_take_a_dgn_and_its_seed_as_a_person_types_them() {
    let mut client = Client::new();
    client.initialize();
    let folder = crate::geo_fixture::TempDir::new("katana-dgn-", "mcp");
    let _ = client.run_lines(&["RECT 0,0 30,20"]);
    let first = folder.file("first.dgn");
    let exported = client.call("katana_export", object([("path", Json::from(first.as_str()))]));
    assert!(!is_error(&exported), "{}", text_of(&exported));
    let record = &exported["structuredContent"]["records"][0];
    assert_eq!(record["record"], Json::from("exported"));
    assert_eq!(record["kind"], Json::from("dgn"));
    let second = folder.file("second.dgn");
    let seeded = client.call(
        "katana_export",
        object([
            ("path", Json::from(second.as_str())),
            ("seed", Json::from(first.as_str())),
        ]),
    );
    assert!(!is_error(&seeded), "{}", text_of(&seeded));
    let imported = client.call("katana_import", object([("path", Json::from(second.as_str()))]));
    assert!(!is_error(&imported), "{}", text_of(&imported));
    let record = &imported["structuredContent"]["records"][0];
    assert_eq!(record["kind"], Json::from("dgn"));
    assert_eq!(record["entities"], Json::from(1));
    assert_eq!(client.session.document().model().entities.size(), 2);
    // A seed that is not a string is refused by the tool before any line.
    let refused = client.call(
        "katana_export",
        object([("path", Json::from(second.as_str())), ("seed", Json::from(7))]),
    );
    assert!(is_error(&refused), "{}", text_of(&refused));
}
