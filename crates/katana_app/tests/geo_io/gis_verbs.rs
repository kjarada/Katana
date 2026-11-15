//! tests/geo/test_gis_verbs.cpp: IMPORT, EXPORT, INFO <file>, REFS and COPC as
//! the one executor runs them (docs/interop.md, "IMPORT, EXPORT, INFO, REFS
//! and COPC on every front end") - the records they reply with, what they add
//! to the drawing and the session, and a cancel that changes nothing. Driven
//! through the executor's context, as the window's workbench drives it, and
//! through a Session, as katana_cli and katana_mcp do. Its GisRecords suite is
//! the frame's (tests/geo_frame).
//!
//! The samples' facts, by hand:
//!
//! ```text
//! samples/gis/parcels.geojson  8 features - two lots, two roads, three
//!   control points and the spoil heaps, a MultiPolygon of two - so 9
//!   entities; its coordinates span (180, 0) to (365, 165); EPSG:32630.
//! samples/gis/terrain.asc  ncols 120, nrows 90, cellsize 1.5, lower-left
//!   corner (-5, -5): it spans (-5, -5) to (175, 130).
//! samples/gis/survey_scan.las  40 000 points (`pdal info`).
//! tests/archive12d/data/multiple_tins.12da  four tins of 9 points and 8
//!   triangles each; TIN SOUTH WEST spans (554000, 6883000) to (554100,
//!   6883100), heights 10.0 to 12.5.
//! ```
//!
//! REFS and INFO <file> are step 6's agent A's verbs, which these cases run
//! through the executor: they pass when wave 2 is integrated as one.

use std::path::{Path, PathBuf};

use katana_app::geo::replies::{Record, parse_records};
use katana_app::geo::{Context, FrontEnd, run_now};
use katana_cad::{CommandInterpreter, Document};
use katana_core::Result;
use katana_interop::ReferenceData;
use katana_terrain::SurfaceStore;

use crate::geo_fixture::{TempDir, checkout, generic, gis_samples};

/// The archive fixtures, beside the samples in the source tree.
fn archives() -> String {
    generic(&checkout("tests/archive12d/data"))
}

/// The C++'s TempDir: "katana gis verbs <name>", here named for the process
/// too.
fn scratch(name: &str) -> TempDir {
    TempDir::new("katana gis verbs ", name)
}

/// What the folder holds, by name.
fn names(dir: &TempDir) -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir.path()) {
        for entry in entries.flatten() {
            found.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found
}

/// A front end's side of the executor: a drawing, its interpreter, reference
/// layers and hooks, as the window and the session each hold them, lent to
/// the executor as a Context for each line.
struct Front {
    scratch: PathBuf,
    document: Document,
    interpreter: CommandInterpreter,
    reference: ReferenceData,
    front_end: FrontEnd,
}

impl Front {
    fn new(scratch: &Path) -> Self {
        Self {
            scratch: scratch.to_path_buf(),
            document: Document::new(None),
            interpreter: CommandInterpreter::new(),
            reference: ReferenceData::default(),
            front_end: FrontEnd::default(),
        }
    }

    fn context(&mut self) -> Context<'_> {
        Context::new(
            &mut self.document,
            &self.interpreter,
            &mut self.reference,
            &self.scratch,
            &self.front_end,
        )
    }

    fn run(&mut self, line: &str) -> Result<String> {
        run_now(&mut self.context(), line, None, None)
    }

    /// The drawing's surfaces, which the verbs change.
    fn surfaces(&self) -> &SurfaceStore {
        self.document.surfaces()
    }

    /// A line the interpreter must take.
    #[track_caller]
    fn type_line(&mut self, line: &str) {
        if let Err(error) = self.interpreter.run(&mut self.document, line) {
            panic!("{line}: {}", error.describe());
        }
    }
}

/// The records of kind `kind` in a reply.
fn records_of(reply: &str, kind: &str) -> Vec<Record> {
    parse_records(reply)
        .into_iter()
        .filter(|record| record.kind == kind)
        .collect()
}

fn field(record: &Record, key: &str) -> String {
    String::from(record.get(key).unwrap_or("<absent>"))
}

mod gis_verbs {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::AtomicBool;

    use katana_app::geo::{FarApartChoice, ImportShown, handles, prepare};
    use katana_core::ErrorCode;
    use katana_entity::{Color, Geometry};
    use katana_geometry::{Box2, Point2};

    use super::*;
    use crate::geo_fixture::{must, refused, shared_data};
    use crate::quoted;

    /// The C++ fixture: a scratch folder named after the test and a front
    /// end over it.
    fn fixture(name: &str) -> (TempDir, Front) {
        let scratch = scratch(name);
        let front = Front::new(scratch.path());
        (scratch, front)
    }

    fn sample(name: &str) -> String {
        quoted(&format!("{}/{name}", gis_samples()))
    }

    fn archive(name: &str) -> String {
        quoted(&format!("{}/{name}", archives()))
    }

    #[test]
    fn import_of_parcels_says_what_came_in_and_where_it_went() {
        let _shared = shared_data();
        let (_scratch, mut front) = fixture("ImportOfParcelsSaysWhatCameInAndWhereItWent");
        let reply = must(front.run(&format!("IMPORT {} LOCAL", sample("parcels.geojson"))));
        let records = parse_records(&reply);
        assert!(records.len() >= 2, "{reply}");
        // The imported record first, then where LOCAL put it: the lower-left
        // corner (180, 0) moved to 0,0, so everything by -180,0.
        assert_eq!(records[0].kind, "imported");
        assert_eq!(field(&records[0], "kind"), "vector");
        assert_eq!(field(&records[0], "entities"), "9");
        assert_eq!(field(&records[0], "layers"), "1");
        assert_eq!(field(&records[0], "bounds"), "0,0,185,165");
        assert!(field(&records[0], "crs").contains("EPSG:32630"), "{reply}");
        assert_eq!(records[1].kind, "placed");
        assert_eq!(field(&records[1], "placement"), "local");
        assert_eq!(field(&records[1], "east"), "-180");
        assert_eq!(field(&records[1], "north"), "0");
        assert!(
            field(&records[1], "text").starts_with("LOCAL: moved as one piece by -180.000,0.000")
        );
        assert_eq!(front.document.model().entities.size(), 9);

        // The whole import is one undo step.
        front.type_line("UNDO");
        assert_eq!(front.document.model().entities.size(), 0);
    }

    #[test]
    fn export_after_import_round_trips_parcels() {
        let _shared = shared_data();
        let (scratch, mut front) = fixture("ExportAfterImportRoundTripsParcels");
        must(front.run(&format!("IMPORT {}", sample("parcels.geojson"))));
        let file = scratch.file("round trip.gpkg");
        let exported = must(front.run(&format!("EXPORT {}", quoted(&file))));
        let record = records_of(&exported, "exported");
        assert_eq!(record.len(), 1, "{exported}");
        assert_eq!(field(&record[0], "file"), file);
        assert_eq!(field(&record[0], "kind"), "vector");
        assert_eq!(field(&record[0], "driver"), "GPKG");
        assert_eq!(field(&record[0], "features"), "9");
        assert_eq!(field(&record[0], "skipped"), "0");

        // Read back into a second drawing: the same 9 entities over the same
        // extent.
        let mut again = Front::new(scratch.path());
        let imported = must(again.run(&format!("IMPORT {}", quoted(&file))));
        let back = records_of(&imported, "imported");
        assert_eq!(back.len(), 1);
        assert_eq!(field(&back[0], "entities"), "9");
        assert_eq!(field(&back[0], "bounds"), "180,0,365,165");
        assert_eq!(again.document.model().entities.size(), 9);
    }

    #[test]
    fn import_of_an_archive_adds_its_surfaces_to_the_drawing_in_the_imports_one_step() {
        let (_scratch, mut front) =
            fixture("ImportOfAnArchiveAddsItsSurfacesToTheDrawingInTheImportsOneStep");
        let reply = must(front.run(&format!("IMPORT {}", archive("multiple_tins.12da"))));
        let imported = records_of(&reply, "imported");
        assert_eq!(imported.len(), 1);
        assert_eq!(field(&imported[0], "kind"), "archive");
        assert_eq!(field(&imported[0], "entities"), "0");
        assert_eq!(field(&imported[0], "surfaces"), "4");

        // Kept, where a session once said it held none.
        assert_eq!(front.surfaces().all().len(), 4);
        let south_west = front.surfaces().find("TIN SOUTH WEST");
        assert!(south_west.is_some());
        let tin = south_west
            .and_then(|named| named.surface.as_ref())
            .expect("a drawing's surface has a triangulation");
        assert_eq!(tin.vertex_count(), 9);
        assert_eq!(tin.triangle_count(), 8);
        let surfaces = records_of(&reply, "surface");
        assert_eq!(surfaces.len(), 4);
        assert_eq!(field(&surfaces[0], "name"), "TIN SOUTH WEST");
        assert_eq!(field(&surfaces[0], "triangles"), "8");
        assert_eq!(field(&surfaces[0], "points"), "9");
        // The surface record is SURFACE LIST's, to the millimetre
        // (docs/terrain.md, "One surface record").
        assert_eq!(
            field(&surfaces[0], "bounds"),
            "554000.000,6883000.000,554100.000,6883100.000"
        );
        assert_eq!(field(&surfaces[0], "zmin"), "10.000");
        assert_eq!(field(&surfaces[0], "zmax"), "12.500");

        // And a geoprocessing line finds it by name.
        let bound =
            front.run("GDAL raster hillshade FROM SURFACE \"TIN SOUTH WEST\" CELL 10 PREVIEW");
        assert!(
            bound.is_ok(),
            "{}",
            bound
                .as_ref()
                .map_or_else(|error| error.describe(), Clone::clone)
        );

        // A second import of the same archive keeps both: the names are
        // taken, so the new ones are "name (2)".
        must(front.run(&format!("IMPORT {}", archive("multiple_tins.12da"))));
        assert_eq!(front.surfaces().all().len(), 8);
        assert!(front.surfaces().find("TIN SOUTH WEST (2)").is_some());

        // Each import is one undo step, its surfaces with it: one UNDO takes
        // the second archive's four back and leaves the first's.
        must(front.document.undo());
        assert_eq!(front.surfaces().all().len(), 4);
        assert!(front.surfaces().find("TIN SOUTH WEST (2)").is_none());
    }

    #[test]
    fn archive_import_uses_the_active_palette_and_adds_native_leaders_in_one_undo_step() {
        let (_scratch, mut front) =
            fixture("ArchiveImportUsesTheActivePaletteAndAddsNativeLeadersInOneUndoStep");
        front.type_line(&format!(
            "CUSTOMISE {}",
            archive("drafting_palette.customisation.json")
        ));
        let before = front.document.history().undo_count();
        let imported = must(front.run(&format!(
            "IMPORT {} LOCAL",
            archive("drafting_leaders.12da")
        )));
        assert_eq!(front.document.model().entities.size(), 4);
        assert_eq!(front.document.history().undo_count(), before + 1);
        let model = front.document.model();
        let line = model.entities.find(1).expect("entity 1");
        let symbol = model.entities.find(2).expect("entity 2");
        let note = model.entities.find(3).expect("entity 3");
        assert_eq!(
            line.color,
            Some(Color {
                r: 224,
                g: 128,
                b: 32,
                a: 255
            })
        );
        assert_eq!(
            symbol.color,
            Some(Color {
                r: 32,
                g: 112,
                b: 80,
                a: 255
            })
        );
        let Geometry::Leader(leader) = &note.geometry else {
            panic!("entity 3 is a leader");
        };
        assert_eq!(leader.text, "Service note\nFirst line");
        assert_eq!(leader.vertices.first(), Some(&Point2::new(0.0, 0.0)));
        assert_eq!(leader.vertices.last(), Some(&Point2::new(0.0, 8.0)));
        assert_eq!(leader.landing_angle, Some(0.0));
        assert!(imported.contains("read=2 imported=2"));
        assert!(!imported.contains("not read: drafting_element"));
        must(front.document.undo());
        assert_eq!(front.document.model().entities.size(), 0);
        assert!(!front.document.model().layers.contains("Test/Notes"));
        must(front.document.redo());
        assert_eq!(front.document.model().entities.size(), 4);
    }

    #[test]
    fn an_archives_meshes_go_to_the_front_end_that_shows_them() {
        // all_geometries.12da holds one primitive_3d of 4 triangles and two
        // tins (tests/archive12d/data, its own comments).
        let (scratch, mut front) = fixture("AnArchivesMeshesGoToTheFrontEndThatShowsThem");
        let line = format!("IMPORT {}", archive("all_geometries.12da"));
        let held = must(front.run(&line));
        let none = records_of(&held, "meshes");
        assert_eq!(none.len(), 1);
        assert_eq!(field(&none[0], "count"), "1");
        assert_eq!(field(&none[0], "held"), "no");
        assert!(held.contains("this session holds no meshes"));

        let mut window = Front::new(scratch.path());
        let shown: Rc<RefCell<Option<ImportShown>>> = Rc::new(RefCell::new(None));
        let into = Rc::clone(&shown);
        window.front_end.imported = Some(Box::new(move |what: ImportShown| {
            *into.borrow_mut() = Some(what);
        }));
        let kept = must(window.run(&line));
        let shown = shown.borrow();
        let shown = shown.as_ref().expect("the window was shown the meshes");
        assert_eq!(shown.meshes.len(), 1);
        assert_eq!(shown.meshes[0].mesh.triangle_count(), 4);
        assert_eq!(shown.surfaces, 2);
        assert_eq!(field(&records_of(&kept, "meshes")[0], "held"), "yes");
        assert!(!kept.contains("this session holds no meshes"));
    }

    #[test]
    fn refs_lists_what_was_imported_as_records() {
        let _shared = shared_data();
        let (_scratch, mut front) = fixture("RefsListsWhatWasImportedAsRecords");
        let empty = must(front.run("REFS"));
        assert_eq!(empty, "references rasters=0 clouds=0 missing=0");

        must(front.run(&format!("IMPORT {}", sample("terrain.asc"))));
        must(front.run(&format!("IMPORT {}", sample("survey_scan.las"))));
        let listed = must(front.run("REFS LIST"));
        let layers = records_of(&listed, "reference");
        assert_eq!(layers.len(), 2, "{listed}");
        assert_eq!(field(&layers[0], "id"), "1");
        assert_eq!(field(&layers[0], "kind"), "raster");
        assert_eq!(field(&layers[0], "name"), "terrain");
        assert_eq!(field(&layers[0], "width"), "120");
        assert_eq!(field(&layers[0], "height"), "90");
        assert_eq!(field(&layers[0], "bounds"), "-5,-5,175,130");
        assert_eq!(field(&layers[0], "visible"), "yes");
        assert_eq!(field(&layers[1], "id"), "2");
        assert_eq!(field(&layers[1], "kind"), "pointcloud");
        assert_eq!(field(&layers[1], "points"), "40000");
        assert_eq!(field(&layers[1], "source_points"), "40000");
        let summary = records_of(&listed, "references");
        assert_eq!(summary.len(), 1);
        assert_eq!(field(&summary[0], "rasters"), "1");
        assert_eq!(field(&summary[0], "clouds"), "1");

        let error = refused(front.run("REFS SHUFFLE"), "a REFS word there is not");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn copc_records_the_file_and_leaves_nothing_beside_it() {
        let _shared = shared_data();
        let (scratch, mut front) = fixture("CopcRecordsTheFileAndLeavesNothingBesideIt");
        let destination = scratch.file("site scan.copc.laz");
        let reply = must(front.run(&format!(
            "COPC {} {}",
            sample("survey_scan.las"),
            quoted(&destination)
        )));
        let converted = records_of(&reply, "converted");
        assert_eq!(converted.len(), 1, "{reply}");
        assert_eq!(field(&converted[0], "file"), destination);
        assert_eq!(field(&converted[0], "format"), "copc");
        // The file, and only the file: the folder it was written in is gone.
        assert_eq!(names(&scratch), vec![String::from("site scan.copc.laz")]);

        let described = must(front.run(&format!("INFO {}", quoted(&destination))));
        let cloud = records_of(&described, "pointcloud");
        assert_eq!(cloud.len(), 1, "{described}");
        assert_eq!(field(&cloud[0], "points"), "40000");
        assert_eq!(field(&cloud[0], "copc"), "yes");

        let usage = refused(
            front.run(&format!("COPC {}", sample("survey_scan.las"))),
            "COPC with no destination",
        );
        assert!(
            usage
                .message
                .contains("usage: COPC <source> <destination.copc.laz>")
        );
    }

    // EXPORT <file.las|.laz> writes a reference cloud as it is held, which
    // GIS > Export Point Cloud runs: a budgeted import holds a sample, and the
    // reply says so rather than passing it off as the survey. The file read
    // back holds what was held; the file's 40 000 points (`pdal info`) are its
    // source's.
    #[test]
    fn export_of_a_point_cloud_writes_the_cloud_as_held_and_says_when_it_is_a_sample() {
        let _shared = shared_data();
        let (scratch, mut front) =
            fixture("ExportOfAPointCloudWritesTheCloudAsHeldAndSaysWhenItIsASample");
        must(front.run(&format!(
            "IMPORT {} budget=10000",
            sample("survey_scan.las")
        )));
        let layer = records_of(&must(front.run("REFS LIST")), "reference");
        assert_eq!(layer.len(), 1);
        let held = field(&layer[0], "points");
        // A sample, or the test proves nothing about one.
        assert_ne!(held, "40000");

        let file = scratch.file("scan sample.laz");
        must(front.run(&format!("EXPORT {} PREVIEW", quoted(&file))));
        assert!(names(&scratch).is_empty());

        // No CLOUD: the one there is.
        let reply = must(front.run(&format!("EXPORT {}", quoted(&file))));
        let exported = records_of(&reply, "exported");
        assert_eq!(exported.len(), 1, "{reply}");
        assert_eq!(field(&exported[0], "kind"), "cloud");
        assert_eq!(field(&exported[0], "format"), "laz");
        assert_eq!(field(&exported[0], "cloud"), "survey_scan");
        assert_eq!(field(&exported[0], "points"), held);
        assert_eq!(field(&exported[0], "source_points"), "40000");
        assert_eq!(field(&exported[0], "sample"), "yes");
        assert_eq!(records_of(&reply, "warning").len(), 1, "{reply}");
        assert_eq!(names(&scratch), vec![String::from("scan sample.laz")]);
        let described = must(front.run(&format!("INFO {}", quoted(&file))));
        let cloud = records_of(&described, "pointcloud");
        assert_eq!(cloud.len(), 1, "{described}");
        assert_eq!(field(&cloud[0], "points"), held);

        // Two clouds: CLOUD says which, by id or name; without it, refused.
        must(front.run(&format!("IMPORT {}", sample("survey_scan.las"))));
        let ambiguous = refused(
            front.run(&format!("EXPORT {}", quoted(&scratch.file("whole.las")))),
            "an export of one of two clouds that names none",
        );
        assert_eq!(ambiguous.code, ErrorCode::InvalidArgument);
        assert!(ambiguous.message.contains("CLOUD <id|name>"));
        let whole = must(front.run(&format!(
            "EXPORT {} CLOUD 2",
            quoted(&scratch.file("whole.las"))
        )));
        assert_eq!(field(&records_of(&whole, "exported")[0], "points"), "40000");
        assert_eq!(field(&records_of(&whole, "exported")[0], "sample"), "no");
        assert!(records_of(&whole, "warning").is_empty(), "{whole}");

        // What a cloud's export does not take is refused, not ignored.
        for line in [
            format!("EXPORT {} DRAWING", quoted(&scratch.file("x.las"))),
            format!("EXPORT {} CLOUD nothing", quoted(&scratch.file("x.las"))),
            format!("EXPORT {} CLOUD 2", quoted(&scratch.file("x.copc.laz"))),
        ] {
            assert!(front.run(&line).is_err(), "{line}");
        }
    }

    #[test]
    fn a_cancelled_import_imports_nothing() {
        let _shared = shared_data();
        let (_scratch, mut front) = fixture("ACancelledImportImportsNothing");
        let line = format!("IMPORT {}", sample("parcels.geojson"));
        let prepared = must(prepare(&mut front.context(), &line));
        assert!(prepared.reply.is_none());
        assert_eq!(prepared.title, "IMPORT parcels.geojson");
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("an import has work");
        let error = refused(work(Some(&stop), None), "a stopped import");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(error.message, "cancelled");

        // A job cancelled after its read never applies: the Apply it made is
        // dropped, and the drawing never sees what was read.
        let again = must(prepare(&mut front.context(), &line));
        {
            let work = again.work.expect("an import has work");
            let dropped = work(None, None);
            assert!(dropped.is_ok(), "the read itself succeeds");
        }
        assert_eq!(front.document.model().entities.size(), 0);
        assert!(front.document.model().layers.contains("0"));
        assert!(!front.document.model().layers.contains("parcels"));
    }

    #[test]
    fn a_cancelled_export_writes_nothing() {
        let (scratch, mut front) = fixture("ACancelledExportWritesNothing");
        front.type_line("RECT 0,0 10,5");
        let file = scratch.file("out.geojson");
        let line = format!("EXPORT {}", quoted(&file));
        {
            // Written by the work - beside its place - and dropped with the
            // job.
            let prepared = must(prepare(&mut front.context(), &line));
            let work = prepared.work.expect("an export has work");
            let apply = work(None, None);
            assert!(apply.is_ok(), "the write itself succeeds");
            assert!(!Path::new(&file).exists());
        }
        assert!(!Path::new(&file).exists());
        assert!(names(&scratch).is_empty());

        // The C++ runs one prepared work three times (a std::function); a
        // Rust Work runs once, so each run here is prepared from the same
        // line, against the same drawing.
        let stopped = must(prepare(&mut front.context(), &line));
        let stop = AtomicBool::new(true);
        let work = stopped.work.expect("an export has work");
        let error = refused(work(Some(&stop), None), "a stopped export");
        assert_eq!(error.message, "cancelled");
        assert!(names(&scratch).is_empty());

        // Applied, it is in place.
        let prepared = must(prepare(&mut front.context(), &line));
        let work = prepared.work.expect("an export has work");
        let apply = must(work(None, None));
        must(apply(&mut front.context()));
        assert!(Path::new(&file).exists());
    }

    #[test]
    fn an_export_replaces_a_file_of_the_same_name() {
        let (scratch, mut front) = fixture("AnExportReplacesAFileOfTheSameName");
        let file = scratch.file("replaced.geojson");
        front.type_line("RECT 0,0 10,5");
        must(front.run(&format!("EXPORT {}", quoted(&file))));
        front.type_line("CIRCLE 50,50 5");
        let second = must(front.run(&format!("EXPORT {}", quoted(&file))));
        assert_eq!(field(&records_of(&second, "exported")[0], "features"), "2");

        let mut again = Front::new(scratch.path());
        must(again.run(&format!("IMPORT {}", quoted(&file))));
        assert_eq!(again.document.model().entities.size(), 2);
        assert_eq!(names(&scratch), vec![String::from("replaced.geojson")]);
    }

    #[test]
    fn the_far_apart_question_decides_where_the_data_lands() {
        // A drawing near the origin and tins some 6.9 million metres away:
        // merged as they are, one would be a dot at a zoom that shows both.
        let (scratch, mut front) = fixture("TheFarApartQuestionDecidesWhereTheDataLands");
        let line = format!("IMPORT {}", archive("multiple_tins.12da"));
        front.type_line("RECT 0,0 10,5");

        // Nobody to ask - a session: kept, and said what to type instead.
        let kept = must(front.run(&line));
        let warnings = records_of(&kept, "warning");
        assert!(!warnings.is_empty());
        assert!(
            warnings
                .last()
                .is_some_and(|last| field(last, "text").contains("IMPORT <file> LOCAL"))
        );
        assert!(records_of(&kept, "placed").is_empty());
        let south_west = |front: &Front| -> Box2 {
            *front
                .surfaces()
                .find("TIN SOUTH WEST")
                .and_then(|named| named.surface.as_ref())
                .expect("the surface is there")
                .bounds()
        };
        assert_eq!(south_west(&front).min.x, 554000.0);

        // The window's question, answered Cancel: nothing comes in.
        let mut cancelled = Front::new(scratch.path());
        cancelled.type_line("RECT 0,0 10,5");
        cancelled.front_end.far_apart = Some(Box::new(|_: &Box2, _: &Box2, _: &str| {
            FarApartChoice::Cancel
        }));
        let error = refused(cancelled.run(&line), "a far-apart import cancelled");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(cancelled.surfaces().all().is_empty());

        // Answered Shift Alongside: the tins' lower-left corner (554000,
        // 6883000) onto the drawing's (0, 0), and the move said.
        let mut alongside = Front::new(scratch.path());
        alongside.type_line("RECT 0,0 10,5");
        let advised: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let said = Rc::clone(&advised);
        alongside.front_end.far_apart =
            Some(Box::new(move |_: &Box2, incoming: &Box2, advice: &str| {
                *said.borrow_mut() = String::from(advice);
                assert_eq!(incoming.min.x, 554000.0);
                FarApartChoice::Alongside
            }));
        let moved = must(alongside.run(&line));
        assert!(!advised.borrow().is_empty());
        let placed = records_of(&moved, "placed");
        assert_eq!(placed.len(), 1, "{moved}");
        assert_eq!(field(&placed[0], "placement"), "alongside");
        assert_eq!(field(&placed[0], "east"), "-554000");
        assert_eq!(field(&placed[0], "north"), "-6883000");
        let landed = south_west(&alongside);
        assert_eq!(landed.min.x, 0.0);
        assert_eq!(landed.min.y, 0.0);
        assert_eq!(landed.max.x, 100.0);
        assert_eq!(landed.max.y, 100.0);
    }

    #[test]
    fn info_of_an_entity_is_the_interpreters_and_of_a_file_the_executors() {
        let _shared = shared_data();
        let (_scratch, mut front) =
            fixture("InfoOfAnEntityIsTheInterpretersAndOfAFileTheExecutors");
        assert!(!handles("INFO #1"));
        assert!(!handles("INFO 987654321")); // no file of that name here
        assert!(handles("INFO terrain.tif"));
        assert!(handles("INFO \"C:/a b/terrain.tif\""));
        assert!(handles("info")); // the executor's, to refuse with its usage
        let usage = refused(front.run("INFO"), "INFO alone");
        assert_eq!(usage.code, ErrorCode::InvalidArgument);
        assert!(usage.message.contains("| INFO <id>"));

        let described = must(front.run(&format!("INFO {}/terrain.asc", gis_samples())));
        let raster = records_of(&described, "raster");
        assert_eq!(raster.len(), 1, "{described}");
        assert_eq!(field(&raster[0], "width"), "120");
        assert_eq!(field(&raster[0], "height"), "90");
        assert_eq!(field(&raster[0], "bounds"), "-5,-5,175,130");
    }

    // `katana <path>` opens a folder as a project and hands every other
    // argument to IMPORT, so a project folder mistyped arrives as a path with
    // no extension that is not there: it is said to be no such folder or
    // file, and not "no importer for ''". A file that IS there with no
    // extension, and a path that is not there with an extension nobody reads,
    // keep the message every unknown extension has.
    #[test]
    fn a_path_with_no_extension_that_is_not_there_is_no_such_project_directory_or_file() {
        let (_scratch, mut front) =
            fixture("APathWithNoExtensionThatIsNotThereIsNoSuchProjectDirectoryOrFile");
        let folder = scratch("mistyped");
        let missing = folder.file("no_such_project");
        let gone = refused(
            front.run(&format!("IMPORT {}", quoted(&missing))),
            "a path that is not there",
        );
        assert_eq!(gone.code, ErrorCode::NotFound);
        assert_eq!(
            gone.message,
            format!("no such project directory or file: {missing}")
        );

        // A data file with no extension that exists: nothing reads it.
        std::fs::write(folder.path().join("plain"), "1,2,3\n").expect("the file is written");
        let present = refused(
            front.run(&format!("IMPORT {}", quoted(&folder.file("plain")))),
            "a file with no extension",
        );
        assert_eq!(present.code, ErrorCode::Unsupported);
        assert_eq!(present.message, "no importer for ''");

        // An extension nobody reads, on a file that is not there, as ever.
        let unknown = refused(
            front.run(&format!("IMPORT {}", quoted(&folder.file("notes.xyzzy")))),
            "an extension nobody reads",
        );
        assert_eq!(unknown.message, "no importer for '.xyzzy'");
    }

    #[test]
    fn import_refuses_what_it_cannot_read() {
        let _shared = shared_data();
        let (scratch, mut front) = fixture("ImportRefusesWhatItCannotRead");
        let usage = refused(front.run("IMPORT"), "IMPORT alone");
        assert_eq!(usage.code, ErrorCode::InvalidArgument);
        assert!(usage.message.contains("usage: IMPORT <file>"));

        let unknown = refused(front.run("IMPORT notes.xyzzy"), "an unknown extension");
        assert_eq!(unknown.code, ErrorCode::Unsupported);
        assert!(unknown.message.contains("no importer for '.xyzzy'"));

        let placed = refused(
            front.run(&format!("IMPORT {} LOCAL", sample("terrain.asc"))),
            "a placement for a raster",
        );
        assert_eq!(
            placed.message,
            "LOCAL is not supported for rasters and point clouds, which are reference data drawn \
             at their own coordinates"
        );

        refused(
            front.run(&format!(
                "IMPORT {}",
                quoted(&scratch.file("absent.geojson"))
            )),
            "a file that is not there",
        );
        assert!(front.document.model().entities.empty());
    }

    #[test]
    fn a_dxf_comes_in_and_goes_out_in_records() {
        // A 30 x 20 rectangle, a circle and a text on a nested layer, out and
        // back: three entities, three layers (0, Survey, Survey/Kerb).
        let (scratch, mut front) = fixture("ADxfComesInAndGoesOutInRecords");
        front.type_line("LAYER NEW Survey/Kerb #FF0000");
        front.type_line("LAYER SET Survey/Kerb");
        front.type_line("RECT 0,0 30,20");
        front.type_line("CIRCLE 15,10 5");
        front.type_line("TEXT 5,5 2.5 \"LOT 42\"");
        let file = scratch.file("site plan.dxf");
        let exported = must(front.run(&format!("EXPORT {}", quoted(&file))));
        let out = records_of(&exported, "exported");
        assert_eq!(out.len(), 1, "{exported}");
        assert_eq!(field(&out[0], "kind"), "dxf");
        assert_eq!(field(&out[0], "format"), "DXF R2000");
        assert_eq!(field(&out[0], "entities"), "3");
        assert_eq!(field(&out[0], "layers"), "3");

        let mut again = Front::new(scratch.path());
        let imported = must(again.run(&format!("IMPORT {} OFFSET=10,-20.5", quoted(&file))));
        let read = records_of(&imported, "imported");
        assert_eq!(read.len(), 1, "{imported}");
        assert_eq!(field(&read[0], "kind"), "dxf");
        assert_eq!(field(&read[0], "entities"), "3");
        assert_eq!(field(&read[0], "bounds"), "10,-20.5,40,-0.5");
        assert_eq!(
            field(&records_of(&imported, "placed")[0], "placement"),
            "offset"
        );
        // LWPOLYLINE, CIRCLE, TEXT.
        assert_eq!(records_of(&imported, "tally").len(), 3);
    }
}

// ---- through a Session, as katana_cli and katana_mcp run it ----

mod gis_session {
    use katana_app::{Captured, Session};

    use super::*;
    use crate::geo_fixture::{must, shared_data};
    use crate::quoted;

    #[test]
    fn the_session_replies_with_the_executors_own_records() {
        let _shared = shared_data();
        // One IMPORT line, through a Session and through the executor as the
        // window's workbench runs it: the same text, record for record.
        let line = format!(
            "IMPORT {} LOCAL",
            quoted(&format!("{}/parcels.geojson", gis_samples()))
        );
        let scratch = scratch("session");
        let mut window = Front::new(scratch.path());
        let direct = must(window.run(&line));

        let mut started = Captured::default();
        let mut session = Session::new(None, &mut started);
        let mut captured = Captured::default();
        assert!(session.run(&line, &mut captured));
        assert_eq!(captured.out, format!("{direct}\n"));
        assert_eq!(captured.err, "");
        assert_eq!(session.document().model().entities.size(), 9);

        // HELP says the verbs once, from the executor's table.
        let help = Session::help_text();
        assert!(help.contains("IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]"));
        assert!(help.contains("REFS [LIST]"));
        assert!(!help.contains("Interop   IMPORT"));
    }

    #[test]
    fn a_refusal_goes_to_stderr_and_changes_nothing() {
        let _shared = shared_data();
        let mut started = Captured::default();
        let mut session = Session::new(None, &mut started);
        let mut captured = Captured::default();
        assert!(!session.run(
            format!(
                "IMPORT {} OFFSET=1,2",
                quoted(&format!("{}/terrain.asc", gis_samples()))
            ),
            &mut captured
        ));
        assert_eq!(captured.out, "");
        assert_eq!(
            captured.err,
            "error: InvalidArgument: OFFSET is not supported for rasters and point clouds, which \
             are reference data drawn at their own coordinates\n"
        );
        assert!(session.document().model().entities.empty());
    }
}
