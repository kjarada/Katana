//! The twins of tests/qt_widgets/bim/test_road_dialogs.cpp's RoadWorkbench suite: the BIM menu's
//! road model (bim::road_workbench), installed in drive::WorkbenchHost - the stand-in for the bare
//! QMainWindow the C++ tests installed it in - over a host whose executor is the interpreter on
//! the same drawing.
//!
//! Not twinned: RoadWorkbench.AThreeDViewThatHidesALayerOfItsOwnDoesNotDrawTheRoadObjectsOnIt,
//! which opens its two 3D views through the workspace (view_workspace, wave 3) and hands it the
//! meshes.
//!
//! Show Road Model is the session's one switch (plan_painter's road_model_shown), process-wide as
//! the C++'s is. The C++ cases ran one at a time; these run on threads of one process, so the
//! cases that make a workbench - which reads the switch - or turn it hold `SWITCH` while they do.

mod road_workbench {
    use std::sync::{Mutex, MutexGuard};

    use katana_cad::Document;
    use katana_cad::bim::bim_object::bim_model;
    use katana_window::bim::road_workbench::{
        CORRIDORS_ACTION, QUANTITIES_ACTION, RoadWorkbench, RoadWorkbenchEvent,
        SHOW_ROAD_MODEL_ACTION,
    };
    use katana_window::command_reference_dialog::{command_reference_sections, window_help_text};
    use katana_window::drive::WorkbenchHost;
    use katana_window::host::RecordingHost;
    use katana_window::plan_painter::road_model_shown;

    use crate::driver::{Driven, ok};
    use crate::window_support::interpreter_host;

    static SWITCH: Mutex<()> = Mutex::new(());

    fn hold_switch() -> MutexGuard<'static, ()> {
        SWITCH.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A 100 m straight along +x with a level design at 10, and the standard templates.
    fn road(host: &mut RecordingHost) {
        ok(host, "ALIGN NEW road 0,0 100,0");
        ok(host, "ALIGN DESIGN road 0,10 100,10");
        ok(host, "ROADTEMPLATE DEFAULTS");
    }

    /// The meshes the workbench handed the views at each hand-over (setRoadMeshes), in order.
    fn handed(window: &mut WorkbenchHost<RoadWorkbench>, into: &mut Vec<usize>) {
        for event in window.workbench.take_events() {
            if let RoadWorkbenchEvent::RoadMeshes(count) = event {
                into.push(count);
            }
        }
    }

    fn logged(host: &RecordingHost) -> Vec<String> {
        host.logged().iter().map(|(text, _)| text.clone()).collect()
    }

    fn objects(host: &RecordingHost) -> usize {
        bim_model(host.document()).objects.len()
    }

    #[test]
    fn the_bim_menu_has_its_sections_and_the_road_model_goes_to_the3d_views() {
        let _held = hold_switch();
        let mut host = interpreter_host(Document::new(None));
        let mut window = WorkbenchHost::new(RoadWorkbench::new(host.document()));
        let mut meshes = Vec::new();
        let names: Vec<String> = window.menus()[0]
            .actions()
            .iter()
            .map(|action| action.name.clone())
            .collect();
        assert_eq!(
            names,
            [
                "bimRoadTemplates",
                "bimCorridors",
                "bimShowRoadModel",
                "bimCorridorQuantities"
            ]
        );
        assert!(
            window.tool_bars()[0]
                .actions()
                .iter()
                .any(|action| action.name == CORRIDORS_ACTION)
        );

        // No alignment with a design profile: the corridor items say where one is made, and
        // open nothing.
        window
            .trigger(&mut host, CORRIDORS_ACTION)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let said = logged(&host);
        assert!(
            said.last()
                .is_some_and(|text| text.starts_with("No alignment has a design profile")),
            "{said:?}"
        );

        // A corridor's objects reach the 3D views, every one on a layer shown.
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        window.workbench.sync_views(host.document());
        handed(&mut window, &mut meshes);
        assert_eq!(meshes.last().copied(), Some(objects(&host)));
        // Its kerbs' layer switched off: their two meshes are not handed over.
        ok(&mut host, "LAYER HIDE road/kerb");
        window.workbench.sync_views(host.document());
        handed(&mut window, &mut meshes);
        assert_eq!(meshes.last().copied(), Some(objects(&host) - 2));
        // Show Road Model off: none.
        window.workbench.set_show_checked(&mut host, false);
        handed(&mut window, &mut meshes);
        assert_eq!(meshes.last().copied(), Some(0));
        window.workbench.set_show_checked(&mut host, true); // the session's switch, back as it was
    }

    #[test]
    fn the_corridor_dialogs_open_for_corridors_whose_alignment_lost_its_profile() {
        // The dialogs' precondition - an alignment with a design profile - is for MAKING a
        // corridor. A drawing that has one still has something to show.
        let _held = hold_switch();
        let mut host = interpreter_host(Document::new(None));
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        ok(&mut host, "ALIGN CLEARPROFILE road");
        assert!(
            host.document()
                .model()
                .alignments
                .find("road")
                .is_some_and(|alignment| alignment.vertical.is_none())
        );
        host.parts.logged.clear();
        let window = WorkbenchHost::new(RoadWorkbench::new(host.document()));
        let mut driven = Driven::new(host, window, "corridorDialog");

        driven
            .dialog
            .trigger(&mut driven.host, CORRIDORS_ACTION)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        driven.settle();
        let opened = driven
            .dialog
            .workbench
            .corridor_dialog()
            .expect("the Corridors dialog did not open");
        assert!(opened.is_open());
        // It shows the corridor and what is wrong with it.
        assert_eq!(driven.rows("corridorList"), 1);
        assert!(
            driven.text("corridorProblems").contains("no design profile"),
            "{}",
            driven.text("corridorProblems")
        );
        // ... and the item itself said nothing about where a profile is made.
        for text in logged(&driven.host) {
            assert!(
                !text.starts_with("No alignment has a design profile"),
                "{text}"
            );
        }

        driven
            .dialog
            .trigger(&mut driven.host, QUANTITIES_ACTION)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        driven.settle();
        assert!(driven.dialog.workbench.quantities_dialog().is_some());

        // No corridor and no profile: as before, nothing opens and it says why.
        driven.ok("CORRIDOR DELETE Main");
        driven.host.parts.logged.clear();
        driven
            .dialog
            .trigger(&mut driven.host, CORRIDORS_ACTION)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let said = logged(&driven.host);
        assert!(
            said.last()
                .is_some_and(|text| text.starts_with("No alignment has a design profile")),
            "{said:?}"
        );
    }

    #[test]
    fn show_road_model_has_a_line_that_switches_it_on_or_off_or_toggles() {
        // ROADMODEL SHOW [ON|OFF] is the window's, as GRID is (docs/road_design.md, "In the
        // window"): the workbench takes it before the interpreter does and switches what the
        // menu's item switches, saying roadmodel=on or off once; any other ROADMODEL line is not
        // its.
        let _held = hold_switch();
        let mut host = interpreter_host(Document::new(None));
        let mut window = WorkbenchHost::new(RoadWorkbench::new(host.document()));
        let mut meshes = Vec::new();
        assert!(window.action(SHOW_ROAD_MODEL_ACTION).is_some());
        road(&mut host);
        ok(
            &mut host,
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        );
        window.workbench.sync_views(host.document());
        handed(&mut window, &mut meshes);
        let objects = objects(&host);
        assert!(objects > 0);
        assert!(window.workbench.road_model_checked());
        assert_eq!(meshes.last().copied(), Some(objects));

        // Not its: the rest of ROADMODEL, other verbs, nothing, and SHOW alone as a word.
        for other in [
            "ROADMODEL",
            "ROADMODEL LIST",
            "ROADMODEL JSON",
            "CORRIDOR SHOW",
            "SHOW",
            "",
            "roadmodels show",
        ] {
            host.parts.logged.clear();
            assert!(!window.run_line(&mut host, other), "{other}");
            assert!(host.logged().is_empty(), "{other}");
        }

        // OFF: the menu item, the views and the painter follow, and it says so once.
        host.parts.logged.clear();
        assert!(window.run_line(&mut host, "ROADMODEL SHOW OFF"));
        handed(&mut window, &mut meshes);
        assert!(!window.workbench.road_model_checked());
        assert!(!road_model_shown());
        assert_eq!(meshes.last().copied(), Some(0));
        assert_eq!(
            host.logged().len(),
            1,
            "the item's own message is not said as well"
        );
        assert_eq!(host.logged()[0].0, "roadmodel=off");
        assert!(!host.logged()[0].1);
        // The words are read in any case, and OFF again changes nothing more.
        host.parts.logged.clear();
        assert!(window.run_line(&mut host, "roadmodel show off"));
        assert!(!window.workbench.road_model_checked());
        assert_eq!(
            host.logged().last().map(|(text, _)| text.as_str()),
            Some("roadmodel=off")
        );

        // Alone, it is the other of the two; ON puts the objects back.
        host.parts.logged.clear();
        assert!(window.run_line(&mut host, "ROADMODEL SHOW"));
        handed(&mut window, &mut meshes);
        assert!(window.workbench.road_model_checked());
        assert!(road_model_shown());
        assert_eq!(meshes.last().copied(), Some(objects));
        assert_eq!(
            host.logged().last().map(|(text, _)| text.as_str()),
            Some("roadmodel=on")
        );
        assert!(window.run_line(&mut host, "ROADMODEL SHOW"));
        handed(&mut window, &mut meshes);
        assert!(!window.workbench.road_model_checked());
        assert_eq!(meshes.last().copied(), Some(0));
        assert!(window.run_line(&mut host, "ROADMODEL SHOW ON"));
        handed(&mut window, &mut meshes);
        assert!(window.workbench.road_model_checked());
        assert_eq!(meshes.last().copied(), Some(objects));

        // A word that is neither is its usage, as an error, and changes nothing.
        for wrong in [
            "ROADMODEL SHOW MAYBE",
            "ROADMODEL SHOW ON OFF",
            "ROADMODEL SHOW 1",
        ] {
            host.parts.logged.clear();
            assert!(window.run_line(&mut host, wrong), "{wrong}");
            assert_eq!(host.logged().len(), 1, "{wrong}");
            assert_eq!(
                host.logged()[0].0,
                "usage: ROADMODEL SHOW [ON|OFF]",
                "{wrong}"
            );
            assert!(host.logged()[0].1, "{wrong}");
            assert!(window.workbench.road_model_checked(), "{wrong}");
        }
        // The menu's own item still says what it did, in its own words.
        host.parts.logged.clear();
        window.workbench.set_show_checked(&mut host, false);
        assert_eq!(host.logged().len(), 1);
        assert_eq!(host.logged()[0].0, "road model hidden");
        window.workbench.set_show_checked(&mut host, true); // the session's switch, back as it was
    }

    #[test]
    fn the_command_reference_has_the_road_verbs_in_a_section_of_their_own() {
        // Help > Command Reference reads HELP CORRIDOR as a section of its own, so a word added to
        // the road verbs' help is there at once; and the window's own row names the line that is
        // the window's alone.
        let sections = command_reference_sections();
        let found = sections
            .iter()
            .find(|section| section.name == "Road model")
            .expect("a Road model section");
        assert!(!found.entries.is_empty());
        assert_eq!(found.entries[0].title, "About");
        let all = found
            .entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for line in [
            "ROADTEMPLATE ADD",
            "ROADTEMPLATE EXPORT <t>|ALL <file.json>",
            "ROADTEMPLATE IMPORT <file.json> [OVERWRITE]",
            "CORRIDOR NEW",
            "CORRIDOR QUANTITIES",
            "ROADMODEL SHOW [ON|OFF]",
        ] {
            assert!(all.contains(line), "{line}");
        }
        // A verb is found by its title, and a double-click puts it on the command line.
        let corridor = found
            .entries
            .iter()
            .find(|entry| entry.title == "CORRIDOR NEW")
            .expect("a CORRIDOR NEW entry");
        assert_eq!(corridor.verb, "CORRIDOR NEW");
        // The Commands section still has the one row of the family.
        let commands = sections
            .iter()
            .find(|section| section.name == "Commands")
            .expect("a Commands section");
        assert!(commands.entries.iter().any(|entry| entry.title == "Road"));
        // The window's rows name ROADMODEL SHOW, as they name GRID.
        assert!(window_help_text().contains("ROADMODEL SHOW [ON|OFF]"));
    }
}
