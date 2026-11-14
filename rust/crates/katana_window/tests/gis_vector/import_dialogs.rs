//! The twins of tests/qt_widgets/geo/test_import_dialogs.cpp: GIS > Import Vector Data, Import
//! Raster and Import Point Cloud (gis_import_dialogs): each dialog's fields make the IMPORT line
//! a person would type, shown in <d>Command, and the vector dialog's Preview runs that line with
//! PREVIEW through the window's executor. The verb itself is tested in katana_app's geo twins.
//! Expected values are the C++'s.
//!
//! The placement group remembers the last choice in the person's settings; each test's host has
//! settings of its own, in memory, so each starts from Keep.

use std::path::PathBuf;

use katana_geometry::Box2;
use katana_interop::{
    PointCloudDescription, RasterDescription, SourceDescription, SourceKind,
    SubdatasetDescription, VectorLayerDescription,
};
use katana_window::geo::gis_tool_dialog::GisDialogContext;
use katana_window::gis_import_dialogs::{
    PointCloudImportDialog, RasterImportDialog, VectorImportDialog,
};
use katana_window::host::{RecordingHost, VerbOutcome};

use super::support::{Bench, fixed, ok, runner_context};

fn vector_source() -> SourceDescription {
    SourceDescription {
        path: PathBuf::from("C:/data/site lots.gpkg"),
        kind: SourceKind::Vector,
        driver: String::from("GPKG"),
        vector_layers: vec![
            VectorLayerDescription {
                name: String::from("lots"),
                feature_count: 3,
                geometry_type: String::from("Polygon"),
                ..VectorLayerDescription::default()
            },
            VectorLayerDescription {
                name: String::from("roads"),
                feature_count: 2,
                geometry_type: String::from("LineString"),
                ..VectorLayerDescription::default()
            },
        ],
        ..SourceDescription::default()
    }
}

/// The vector dialog on the source above, over `host`, on the harness.
fn vector_bench(
    context: GisDialogContext,
    mut host: RecordingHost,
) -> Bench<VectorImportDialog, RecordingHost> {
    let dialog = VectorImportDialog::new(&vector_source(), Box2::default(), context, &mut host);
    Bench::new("vectorImportDialog", dialog, host)
}

fn plain_vector_bench() -> Bench<VectorImportDialog, RecordingHost> {
    vector_bench(GisDialogContext::default(), RecordingHost::new())
}

mod vector_import_dialog {
    use super::*;

    #[test]
    fn the_fields_build_exactly_the_typed_line() {
        let mut bench = plain_vector_bench();
        // Nothing chosen: the whole file, the path quoted.
        assert_eq!(
            bench.text("vectorImportCommand"),
            "IMPORT \"C:/data/site lots.gpkg\""
        );

        // The second layer's box cleared (the C++ set the list item's check state).
        bench.dialog.set_layer_ticked(1, false);
        bench.fill("vectorImportWhere", "kind = 'lot'");
        bench.fill("vectorImportFields", "name, kind");
        bench.fill("vectorImportTarget", "site");
        bench.fill("vectorImportMax", "5");
        bench.fill("vectorImportOpenOptions", "LIST_ALL_TABLES=NO");
        // The choice whose word is "project".
        bench.fill("vectorImportCrs", "Into the project's coordinate system");
        bench.fill("vectorImportAssumeCrs", "EPSG:28356");
        bench.tick("vectorImportAttributes", false);
        assert_eq!(
            bench.text("vectorImportCommand"),
            "IMPORT \"C:/data/site lots.gpkg\" layers=lots where=\"kind = 'lot'\" \
             fields=name,kind attributes=no target=site max=5 oo=LIST_ALL_TABLES=NO \
             crs=project srs=EPSG:28356"
        );
    }

    #[test]
    fn the_scope_and_clip_follow_the_line_and_a_placement_comes_first() {
        let mut bench = plain_vector_bench();
        bench.tick("vectorImportUseScope", true);
        bench.press("vectorImportScopeDrawing");
        bench.tick("vectorImportClip", true);
        assert_eq!(
            bench.text("vectorImportCommand"),
            "IMPORT \"C:/data/site lots.gpkg\" DRAWING clip"
        );

        // A scope is in the drawing's coordinates, which a placement moves the data away from:
        // no line, and Import says why by being off.
        bench.press("importPlacementLocal");
        assert!(bench.dialog.command().is_err());
        bench.tick("vectorImportUseScope", false);
        assert_eq!(
            bench.text("vectorImportCommand"),
            "IMPORT \"C:/data/site lots.gpkg\" LOCAL"
        );
    }

    #[test]
    fn sql_with_a_layer_left_out_has_no_line() {
        let mut bench = plain_vector_bench();
        bench.fill("vectorImportSql", "SELECT * FROM lots");
        // The choice whose word is "sqlite".
        bench.fill("vectorImportDialect", "SQLite");
        assert_eq!(
            bench.text("vectorImportCommand"),
            "IMPORT \"C:/data/site lots.gpkg\" sql=\"SELECT * FROM lots\" dialect=sqlite"
        );
        bench.dialog.set_layer_ticked(0, false);
        bench.draw();
        assert!(bench.dialog.command().is_err());
        assert!(!bench.enabled("vectorImportPreview"));
    }

    #[test]
    fn preview_runs_the_line_with_preview_and_shows_the_match() {
        let mut bench = vector_bench(
            runner_context(),
            fixed(ok(
                "import file=\"C:/data/site lots.gpkg\" kind=vector preview=yes features=2 of=5 \
                 entities=2 layers=1",
            )),
        );
        bench.fill("vectorImportWhere", "kind = 'lot'");
        bench.press("vectorImportPreview");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "IMPORT \"C:/data/site lots.gpkg\" where=\"kind = 'lot'\" PREVIEW"
        );
        assert_eq!(
            bench.text("vectorImportMatchCount"),
            "2 of 5 features match; 2 entities would be imported."
        );
    }

    // What a preview says is GDAL's text; read as rich text, a "<code>" in it was taken for a
    // tag and vanished. A label here is its text and nothing else, so the C++'s second
    // assertion - that the label's format is plain text - has nothing to ask.
    #[test]
    fn a_message_is_shown_as_typed_not_as_markup() {
        let mut bench = vector_bench(
            runner_context(),
            fixed(VerbOutcome::new(
                false,
                "",
                "ParseFailure: no such column <code> in lots",
            )),
        );
        bench.fill("vectorImportWhere", "code = 1");
        bench.press("vectorImportPreview");
        assert_eq!(
            bench.text("vectorImportMatchCount"),
            "ParseFailure: no such column <code> in lots"
        );
    }

    #[test]
    fn a_double_quote_in_a_field_has_no_line() {
        let mut bench = plain_vector_bench();
        bench.fill("vectorImportWhere", "name = \"A\"");
        assert!(bench.dialog.command().is_err());
        assert!(bench.text("vectorImportCommand").is_empty());
    }
}

mod import_dialogs {
    use super::*;

    // A headless run presses a button by its object name; these had none.
    #[test]
    fn import_and_cancel_have_their_dialogs_names() {
        let vector = plain_vector_bench();
        assert_eq!(vector.control("vectorImportRun").text, "Import");
        assert!(vector.has("vectorImportCancel"));
        let raster = SourceDescription {
            path: PathBuf::from("C:/data/ortho.tif"),
            kind: SourceKind::Raster,
            raster: Some(RasterDescription::default()),
            ..SourceDescription::default()
        };
        let raster = Bench::new(
            "rasterImportDialog",
            RasterImportDialog::new(&raster),
            RecordingHost::new(),
        );
        assert!(raster.has("rasterImportRun"));
        assert!(raster.has("rasterImportCancel"));
        let cloud = SourceDescription {
            path: PathBuf::from("C:/data/scan.las"),
            kind: SourceKind::PointCloud,
            point_cloud: Some(PointCloudDescription::default()),
            ..SourceDescription::default()
        };
        let cloud = Bench::new(
            "pointCloudImportDialog",
            PointCloudImportDialog::new(&cloud),
            RecordingHost::new(),
        );
        assert!(cloud.has("pointCloudImportRun"));
        assert!(cloud.has("pointCloudImportCancel"));
    }
}

mod raster_import_dialog {
    use super::*;

    #[test]
    fn the_fields_build_exactly_the_typed_line() {
        let source = SourceDescription {
            path: PathBuf::from("C:/data/ortho.tif"),
            kind: SourceKind::Raster,
            raster: Some(RasterDescription {
                band_count: 4,
                ..RasterDescription::default()
            }),
            subdatasets: vec![
                SubdatasetDescription {
                    name: String::from("GTIFF_DIR:1:C:/data/ortho.tif"),
                    description: String::from("Page 1"),
                },
                SubdatasetDescription {
                    name: String::from("GTIFF_DIR:2:C:/data/ortho.tif"),
                    description: String::from("Page 2"),
                },
            ],
            ..SourceDescription::default()
        };
        let mut bench = Bench::new(
            "rasterImportDialog",
            RasterImportDialog::new(&source),
            RecordingHost::new(),
        );
        assert_eq!(
            bench.text("rasterImportCommand"),
            "IMPORT \"C:/data/ortho.tif\""
        );

        bench.fill("rasterImportName", "near infrared");
        // The choice whose number is 2048.
        bench.fill("rasterImportMaxPixels", "2048 px  (16 MB at most)");
        bench.fill("rasterImportBand", "4");
        // The third item: the second dataset inside.
        bench.fill("rasterImportSubdataset", "2  Page 2");
        // The choice whose word is "adopt".
        bench.fill(
            "rasterImportCrs",
            "Set the project's from the raster, if it has none",
        );
        assert_eq!(
            bench.text("rasterImportCommand"),
            "IMPORT \"C:/data/ortho.tif\" name=\"near infrared\" maxpixels=2048 band=4 \
             subdataset=2 crs=adopt"
        );
    }
}

mod point_cloud_import_dialog {
    use super::*;

    #[test]
    fn the_fields_build_exactly_the_typed_line() {
        let source = SourceDescription {
            path: PathBuf::from("C:/data/scan.las"),
            kind: SourceKind::PointCloud,
            point_cloud: Some(PointCloudDescription {
                point_count: 1_000_000,
                ..PointCloudDescription::default()
            }),
            ..SourceDescription::default()
        };
        let mut bench = Bench::new(
            "pointCloudImportDialog",
            PointCloudImportDialog::new(&source),
            RecordingHost::new(),
        );
        assert_eq!(
            bench.text("pointCloudImportCommand"),
            "IMPORT \"C:/data/scan.las\""
        );
        bench.fill("pointCloudImportBudget", "500000");
        // ASPRS class 2, ground.
        bench.fill("pointCloudImportClasses", "2 - Ground");
        assert_eq!(
            bench.text("pointCloudImportCommand"),
            "IMPORT \"C:/data/scan.las\" budget=500000 class=2"
        );
    }
}
