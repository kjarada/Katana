//! The twins of tests/qt_widgets/geo/test_formats_dialog.cpp: GIS > Processing - GDAL > Formats
//! (geo::formats_dialog): the FORMATS line the choices make, the table filled from its records, a
//! driver's options on selecting it, and the line handed to the window's executor on Run. Found
//! by object name, as a headless run finds them. Expected values are the C++'s.

use katana_window::drive::{Menu, MenuItem};
use katana_window::geo::formats_dialog::{FormatsDialog, KeptFormatsDialog, add_formats_item};
use katana_window::geo::geo_workbench::GeoMenus;
use katana_window::host::RecordingHost;

use super::support::{Bench, fixed, ok};

/// The window's side: a host whose executor records the lines handed to it and answers ok, and
/// whose log is what the table's filling must leave empty.
fn window() -> RecordingHost {
    let mut host = fixed(ok(""));
    host.parts.scratch = std::env::temp_dir().join("katana-formats-dialog-test");
    host
}

fn formats_bench() -> Bench<FormatsDialog, RecordingHost> {
    let mut host = window();
    let mut dialog = FormatsDialog::new(&mut host);
    dialog.show();
    Bench::new("gisFormatsDialog", dialog, host)
}

/// One column of a table's rows.
fn column(rows: &[Vec<String>], column: usize) -> Vec<String> {
    rows.iter().map(|row| row[column].clone()).collect()
}

mod formats_dialog {
    use super::*;

    #[test]
    fn the_table_is_the_formats_line_its_choices_make() {
        let mut bench = formats_bench();
        for name in ["gisFormatsTable", "gisFormatsCommand", "gisFormatsCount"] {
            assert!(bench.has(name), "{name}");
        }
        assert_eq!(bench.text("gisFormatsCommand"), "FORMATS");
        let everything = bench.dialog.format_count();
        assert!(everything > 50, "GDAL has well over fifty formats");
        assert!(
            bench
                .text("gisFormatsCount")
                .starts_with(&format!("{everything} formats (GDAL 3.")),
            "{}",
            bench.text("gisFormatsCount")
        );

        bench.fill("gisFormatsKind", "Vector");
        bench.fill("gisFormatsCapability", "Writes");
        bench.fill("gisFormatsFilter", "fgb");
        assert_eq!(bench.text("gisFormatsCommand"), "FORMATS VECTOR WRITE fgb");
        assert_eq!(bench.dialog.line(), bench.text("gisFormatsCommand"));
        let table = bench.dialog.formats();
        assert_eq!(table.len(), 1);
        assert_eq!(table[0][0], "FlatGeobuf");
        assert_eq!(table[0][2], "vector");
        assert_eq!(table[0][4], "vector");
        assert_eq!(table[0][5], "fgb");
        assert!(
            bench.host.logged().is_empty(),
            "the table fills without filling the log"
        );
    }

    #[test]
    fn a_filter_word_that_is_a_keyword_stays_a_word() {
        let mut bench = formats_bench();
        // A quote typed in the filter is dropped: the line has no escape for one.
        bench.fill("gisFormatsFilter", "raster \"tile\"");
        assert_eq!(bench.dialog.line(), "FORMATS \"raster\" tile");
        // Every row holds both words - GDAL's "GDAL Raster Tile Index" (GTI) does - where RASTER
        // unquoted would have listed every raster format.
        let table = bench.dialog.formats();
        assert!(!table.is_empty());
        for row in table {
            let text = format!("{} {} {}", row[0], row[1], row[5]).to_lowercase();
            assert!(text.contains("raster"), "{text}");
            assert!(text.contains("tile"), "{text}");
        }
    }

    #[test]
    fn selecting_a_driver_shows_its_options() {
        let mut bench = formats_bench();
        bench.fill("gisFormatsFilter", "geopackage");
        let drivers = column(bench.dialog.formats(), 0);
        assert!(
            drivers.iter().any(|driver| driver == "GPKG"),
            "{}",
            drivers.join(",")
        );
        // A click on the driver's row: the row of that name made current.
        bench.fill("gisFormatsTable", "GPKG");
        assert!(bench.has("gisFormatsOptions"));
        let options = bench.dialog.options();
        let names = column(options, 1);
        let list_all = names
            .iter()
            .position(|name| name == "LIST_ALL_TABLES")
            .unwrap_or_else(|| panic!("{}", names.join(",")));
        assert_eq!(options[list_all][0], "open");
        assert_eq!(options[list_all][3], "AUTO");
        assert_eq!(options[list_all][4], "AUTO YES NO");
    }

    #[test]
    fn run_hands_the_line_to_the_windows_executor() {
        let mut bench = formats_bench();
        bench.fill("gisFormatsKind", "Raster");
        bench.press("gisFormatsRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], "FORMATS RASTER");
    }

    #[test]
    fn the_menu_item_shows_one_kept_dialog() {
        let mut host = window();
        let mut menus = GeoMenus::new(Menu::new("gisMenu", "&GIS"), Menu::new("terrainMenu", ""));
        add_formats_item(&mut menus);
        let action = menus
            .gis()
            .items
            .iter()
            .find_map(|item| match item {
                MenuItem::Action(action) if action.name == "gisFormats" => Some(action.clone()),
                _ => None,
            })
            .expect("the action gisFormats, in the GIS menu");
        assert_eq!(action.dialog.as_deref(), Some("gisFormatsDialog"));
        // The action shows the window's one dialog, made the first time and kept.
        let mut kept = KeptFormatsDialog::new();
        assert!(kept.dialog().is_none());
        kept.trigger(&mut host);
        let mut bench = Bench::new("gisFormatsDialog", kept, host);
        {
            let dialog = bench.dialog.dialog().expect("the dialog was made");
            assert_eq!(dialog.object_name(), "gisFormatsDialog");
            assert!(dialog.is_visible());
            assert!(!dialog.is_modal());
        }
        assert!(bench.harness.target_record("gisFormatsDialog").is_some());
        // The second time shows the same dialog: what was typed into it is still there.
        bench.fill("gisFormatsFilter", "fgb");
        bench.dialog.trigger(&mut bench.host);
        bench.draw();
        assert_eq!(bench.dialog.dialog().map(FormatsDialog::line).as_deref(), Some("FORMATS fgb"));
    }
}
