//! The twins of tests/qt_widgets/test_gis_online.cpp: GIS > Online Data's dialog
//! (gis_online_dialog), driven by its object names as a person drives it by clicking: the
//! provider tree and its search, the details a layer shows, and the command the dialog hands on -
//! which must be exactly what the ONLINE IMPORT verb would have parsed, because the dialog has no
//! other way to import. The context is the test's own, so nothing here reaches the network or a
//! window. Expected values are the C++'s.

use std::collections::BTreeMap;

use egui::Ui;
use katana_core::{ErrorCode, Status};
use katana_interop::online_catalogue::{built_in_catalogue, merge_catalogues, parse_catalogue};
use katana_interop::{CkanResource, OnlineAreaKind, OnlineCatalogue, OnlineCommand, OnlineVerb};
use katana_window::gis_online_dialog::{OnlineDataDialog, OnlineDialogContext};

use super::support::{Bench, Drawn};

/// What the dialog did to its context.
struct Recorder {
    catalogue: OnlineCatalogue,
    crs: String,
    run: Vec<OnlineCommand>,
    custom: Vec<String>,
    keys: BTreeMap<String, String>,
}

impl OnlineDialogContext for Recorder {
    fn catalogue(&self) -> &OnlineCatalogue {
        &self.catalogue
    }

    fn project_crs(&self) -> String {
        self.crs.clone()
    }

    fn has_key(&mut self, key_name: &str) -> bool {
        self.keys.contains_key(key_name)
    }

    fn save_key(&mut self, key_name: &str, value: &str) {
        self.keys
            .insert(String::from(key_name), String::from(value));
    }

    fn run(&mut self, command: &OnlineCommand) -> Status {
        self.run.push(command.clone());
        Ok(())
    }

    fn add_custom(&mut self, url: &str) -> Status {
        self.custom.push(String::from(url));
        Ok(())
    }
}

impl Drawn<Recorder> for OnlineDataDialog {
    fn draw(&mut self, ui: &mut Ui, host: &mut Recorder) {
        self.ui(ui, host);
    }
}

fn with_built_in() -> Recorder {
    Recorder {
        catalogue: built_in_catalogue().expect("the built-in catalogue reads"),
        crs: String::from("EPSG:7856"),
        run: Vec::new(),
        custom: Vec::new(),
        keys: BTreeMap::new(),
    }
}

/// The dialog over the built-in catalogue, shown, on the harness.
fn online_bench() -> Bench<OnlineDataDialog, Recorder> {
    let recorder = with_built_in();
    let mut dialog = OnlineDataDialog::new(&recorder);
    dialog.show();
    Bench::new("onlineDataDialog", dialog, recorder)
}

/// Chooses a layer in the tree, and draws what that leaves.
fn select(bench: &mut Bench<OnlineDataDialog, Recorder>, provider: &str, layer: &str) {
    assert!(bench.dialog.select_layer(provider, layer), "{provider} {layer}");
    bench.draw();
}

mod gis_online_dialog {
    use super::*;

    #[test]
    fn every_control_has_its_object_name() {
        let bench = online_bench();
        assert_eq!(bench.dialog.object_name(), "onlineDataDialog");
        assert!(!bench.dialog.is_modal());
        for name in [
            "onlineSearch",
            "onlineProviders",
            "onlineDetails",
            "onlineArea",
            "onlineBox",
            "onlineBoxCrs",
            "onlineResolutionAuto",
            "onlineResolution",
            "onlineTargetLayer",
            "onlineTag",
            "onlineUseDates",
            "onlineFrom",
            "onlineTo",
            "onlineCloud",
            "onlineProjectCrs",
            "onlineCrs",
            "onlineKey",
            "onlineSaveKey",
            "onlineCustomUrl",
            "onlineAddCustom",
            "onlineImport",
            "onlineStatus",
            "onlineClose",
            "onlineTimeDefault",
            "onlineTime",
            "onlineAdvanced",
            "onlineTimeoutOn",
            "onlineTimeout",
            "onlineCatalogue",
            "onlineCatalogueSearch",
            "onlineCatalogueRun",
            "onlineCatalogueResults",
            "onlineCatalogueAdd",
        ] {
            assert!(bench.has(name), "{name}");
        }
    }

    #[test]
    fn the_tree_is_australia_by_state_then_global() {
        let bench = online_bench();
        let tops = bench.dialog.top_items();
        assert!(tops.len() >= 2);
        assert_eq!(tops[0].0, "Australia");
        assert_eq!(tops[1].0, "Global");
        let states = bench.dialog.children_of_top(0);
        assert!(states.iter().any(|state| state == "NSW"));
        assert!(states.iter().any(|state| state == "National"));
        // No custom services yet: the Custom branch is not shown empty.
        assert!(tops[2].1);
        assert_eq!(bench.text("onlineProjectCrs"), "EPSG:7856");
    }

    #[test]
    fn search_narrows_the_tree_and_chooses_what_is_left() {
        let mut bench = online_bench();
        bench.fill("onlineSearch", "sentinel");
        assert_eq!(
            bench.dialog.visible_layers(),
            ["sentinel2 truecolour", "sentinel2 composite"]
        );
        // The details describe the first of them, licence and attribution included.
        let details = bench.text("onlineDetails");
        assert!(
            details.contains("Contains modified Copernicus Sentinel data"),
            "{details}"
        );
        assert!(details.contains("Copernicus Sentinel data terms"));
        assert!(bench.enabled("onlineImport"));
        // The Sentinel-2 options are live; the OpenStreetMap tag is not.
        assert!(bench.enabled("onlineCloud"));
        assert!(!bench.enabled("onlineTag"));
        bench.fill("onlineSearch", "");
        assert!(bench.dialog.visible_layers().len() > 30);
    }

    #[test]
    fn import_hands_on_exactly_the_command_the_verb_would_parse() {
        let mut bench = online_bench();
        select(&mut bench, "nsw-spatial", "lots");
        bench.fill("onlineArea", "Typed box");
        bench.fill("onlineBox", "151.215, -33.865, 151.2, -33.875");
        bench.fill("onlineBoxCrs", "WGS 84 longitude, latitude");
        bench.fill("onlineTargetLayer", "cadastre/lots from nsw");
        bench.press("onlineImport");

        assert_eq!(bench.host.run.len(), 1);
        let command = &bench.host.run[0];
        assert_eq!(command.verb, OnlineVerb::Import);
        assert_eq!(command.provider, "nsw-spatial");
        assert_eq!(command.layer, "lots");
        assert_eq!(command.area, OnlineAreaKind::Box);
        assert!(command.box_is_lon_lat);
        // Corners in either order make the same box.
        assert_eq!(command.r#box.min_x, 151.2);
        assert_eq!(command.r#box.min_y, -33.875);
        assert_eq!(command.r#box.max_x, 151.215);
        assert_eq!(command.r#box.max_y, -33.865);
        assert_eq!(command.target_layer, "cadastre/lots from nsw");
        // A vector layer has no resolution to give.
        assert!(command.resolution.is_none());
        assert!(bench.text("onlineStatus").contains("in the background"));
    }

    #[test]
    fn a_raster_carries_its_resolution_when_not_automatic() {
        let mut bench = online_bench();
        select(&mut bench, "copernicus", "dem");
        let command = bench
            .dialog
            .command()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(command.area, OnlineAreaKind::View);
        assert!(command.resolution.is_none());
        bench.tick("onlineResolutionAuto", false);
        bench.fill("onlineResolution", "12.5");
        let command = bench.dialog.command().expect("a command");
        assert_eq!(command.resolution, Some(12.5));
    }

    #[test]
    fn a_malformed_box_is_said_in_the_status_and_nothing_runs() {
        let mut bench = online_bench();
        select(&mut bench, "osm", "buildings");
        bench.fill("onlineArea", "Typed box");
        bench.fill("onlineBox", "1,2,3");
        bench.press("onlineImport");
        assert!(bench.host.run.is_empty());
        assert!(bench.text("onlineStatus").contains("four numbers"));
    }

    #[test]
    fn no_layer_chosen_means_no_import() {
        let mut bench = online_bench();
        assert!(!bench.enabled("onlineImport"));
        assert_eq!(
            bench.dialog.command().expect_err("no command").code,
            ErrorCode::InvalidArgument
        );
        // A catalogue search is not importable either.
        select(&mut bench, "data-gov-au", "search");
        assert!(!bench.enabled("onlineImport"));
        assert!(
            bench
                .text("onlineDetails")
                .contains("ONLINE LAYERS data-gov-au")
        );
    }

    #[test]
    fn a_catalogue_is_searched_from_the_dialog_and_a_result_added_as_a_custom_service() {
        let mut bench = online_bench();
        // Not a catalogue: no search offered.
        select(&mut bench, "osm", "buildings");
        assert!(!bench.visible("onlineCatalogue"));

        select(&mut bench, "data-gov-au", "search");
        assert!(bench.visible("onlineCatalogue"));
        // Nothing typed: said, and nothing runs.
        bench.press("onlineCatalogueRun");
        assert!(bench.host.run.is_empty());
        assert!(
            bench
                .text("onlineStatus")
                .contains("search the catalogue for")
        );

        bench.fill("onlineCatalogueSearch", "  flood   mapping ");
        bench.press("onlineCatalogueRun");
        assert_eq!(bench.host.run.len(), 1);
        assert_eq!(bench.host.run[0].verb, OnlineVerb::Layers);
        assert_eq!(bench.host.run[0].provider, "data-gov-au");
        assert_eq!(bench.host.run[0].filter, "flood mapping");

        // What the workbench's search found comes back to be listed.
        bench.dialog.show_catalogue_results(&[
            CkanResource {
                dataset: String::from("flood-studies"),
                title: String::from("Flood Studies"),
                format: String::from("WMS"),
                url: String::from("https://example.org/wms?key=SECRET"),
                licence: String::from("CC-BY"),
            },
            CkanResource {
                dataset: String::from("levees"),
                title: String::from("Levees"),
                format: String::from("WFS"),
                url: String::from("https://example.org/wfs"),
                licence: String::from("CC-BY"),
            },
        ]);
        bench.draw();
        let results = bench.dialog.catalogue_results();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1][0], "Levees");
        assert_eq!(results[1][2], "WFS");
        assert!(
            !results[0][3].contains("SECRET"),
            "an address is shown with its key taken out"
        );
        assert!(
            bench
                .text("onlineStatus")
                .starts_with("2 web services found")
        );

        // The second result made current, as a click on its row does.
        bench.fill("onlineCatalogueResults", "Levees");
        bench.press("onlineCatalogueAdd");
        assert_eq!(bench.host.run.len(), 2);
        assert_eq!(bench.host.run[1].verb, OnlineVerb::Custom);
        assert_eq!(bench.host.run[1].url, "https://example.org/wfs");
        // The first result's full address, key and all, is what is asked.
        bench.fill("onlineCatalogueResults", "Flood Studies");
        bench.press("onlineCatalogueAdd");
        assert_eq!(bench.host.run.len(), 3);
        assert_eq!(bench.host.run[2].url, "https://example.org/wms?key=SECRET");
        assert!(!bench.text("onlineStatus").contains("SECRET"));

        // A search that finds nothing says so.
        bench.dialog.show_catalogue_results(&[]);
        bench.draw();
        assert!(bench.dialog.catalogue_results().is_empty());
        assert!(!bench.enabled("onlineCatalogueAdd"));
    }

    #[test]
    fn a_time_enabled_layer_takes_a_date_and_others_do_not() {
        let mut bench = online_bench();
        assert!(bench.has("onlineTimeDefault"));
        assert!(bench.has("onlineTime"));
        // NASA GIBS's tiles hold {time} in their address.
        select(&mut bench, "nasa-gibs", "modis-terra");
        assert!(bench.enabled("onlineTimeDefault"));
        assert!(
            !bench.enabled("onlineTime"),
            "the latest, until a date is chosen"
        );
        let command = bench
            .dialog
            .command()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(command.time.is_empty());

        bench.tick("onlineTimeDefault", false);
        assert!(bench.enabled("onlineTime"));
        bench.fill("onlineTime", "2024-01-15");
        let command = bench
            .dialog
            .command()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(command.time, "2024-01-15");
        assert!(bench.text("onlineDetails").contains("time series"));

        // A layer with no time dimension neither offers nor writes one.
        select(&mut bench, "copernicus", "dem");
        assert!(!bench.enabled("onlineTimeDefault"));
        assert!(!bench.enabled("onlineTime"));
        let command = bench.dialog.command().expect("a command");
        assert!(command.time.is_empty());
    }

    #[test]
    fn a_give_up_after_is_written_only_when_asked_for() {
        let mut bench = online_bench();
        select(&mut bench, "copernicus", "dem");
        assert!(bench.has("onlineTimeoutOn"));
        assert!(!bench.enabled("onlineTimeout"));
        assert_eq!(
            bench.value("onlineTimeout"),
            "600",
            "a headless import's own deadline"
        );
        let command = bench.dialog.command().expect("a command");
        assert!(command.timeout_seconds.is_none());

        bench.tick("onlineTimeoutOn", true);
        bench.fill("onlineTimeout", "45");
        let command = bench
            .dialog
            .command()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(command.timeout_seconds, Some(45));
    }

    #[test]
    fn the_details_say_the_services_system_version_and_limits() {
        let mut bench = online_bench();
        // OpenStreetMap buildings, through Overpass: an area limit.
        select(&mut bench, "osm", "buildings");
        let details = bench.text("onlineDetails");
        assert!(details.contains("Service CRS"), "{details}");
        assert!(
            details.contains("square kilometres an import"),
            "{details}"
        );
        // NASA GIBS: Web Mercator tiles to zoom 9 (the catalogue's maxZoom).
        select(&mut bench, "nasa-gibs", "modis-terra");
        let details = bench.text("onlineDetails");
        assert!(details.contains("EPSG:3857"), "{details}");
        assert!(details.contains("tiles to zoom 9"), "{details}");
    }

    // The C++ also asked that the field's echo mode is Password; a field's record does not say
    // how it is drawn, and the field is made with `password` set (gis_online_dialog).
    #[test]
    fn a_key_is_saved_under_the_layers_key_name_and_never_shown_back() {
        let mut bench = online_bench();
        select(&mut bench, "nasa-gibs", "modis-terra");
        bench.fill("onlineKey", "abc123");
        bench.press("onlineSaveKey");
        assert_eq!(
            bench.host.keys.get("nasa-gibs").map(String::as_str),
            Some("abc123")
        );
        assert!(bench.text("onlineKey").is_empty());
        assert!(!bench.text("onlineStatus").contains("abc123"));
        assert!(!bench.text("onlineDetails").contains("abc123"));
    }

    #[test]
    fn add_custom_service_needs_an_address() {
        let mut bench = online_bench();
        bench.press("onlineAddCustom");
        assert!(bench.host.custom.is_empty());
        assert!(bench.text("onlineStatus").contains("address"));
        bench.fill("onlineCustomUrl", "https://example.org/wms?key=SECRET");
        bench.press("onlineAddCustom");
        assert_eq!(bench.host.custom.len(), 1);
        assert_eq!(bench.host.custom[0], "https://example.org/wms?key=SECRET");
        // What the status repeats has the key taken out.
        let status = bench.text("onlineStatus");
        assert!(!status.contains("SECRET"), "{status}");
    }

    #[test]
    fn a_custom_provider_appears_under_custom_after_a_refresh() {
        let mut bench = online_bench();
        let custom = parse_catalogue(
            r#"{"providers": [{"id": "custom-example-org",
        "title": "Example WMS", "group": "Custom", "licence": "L", "attribution": "A",
        "services": [{"id": "wms", "title": "W", "type": "wms", "endpoint": "https://example.org/wms",
          "layers": [{"id": "roads", "title": "Roads", "kind": "imagery", "layer": "roads"}]}]}]}"#,
            true,
        )
        .expect("the custom catalogue reads");
        bench.host.catalogue =
            merge_catalogues(std::mem::take(&mut bench.host.catalogue), &custom);
        bench.dialog.refresh_catalogue(&bench.host);
        bench.draw();
        let tops = bench.dialog.top_items();
        assert!(!tops[2].1);
        assert_eq!(tops[2].0, "Custom");
        assert!(bench.dialog.select_layer("custom-example-org", "roads"));
    }
}
