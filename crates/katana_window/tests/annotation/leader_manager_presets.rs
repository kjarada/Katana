//! The twins of tests/qt_widgets/annotation/test_leader_manager_presets.cpp: the Leaders
//! manager's presets, driven by object name as a person clicks it - a preset chosen writes its
//! template into the note, the surface it reads chosen beside it, and Apply or Make stores it, the
//! same edit LEADER preset= makes, one undo step.
//!
//! The surface is the plane z = 10 + 0.1 x + 0.2 y over 0..100 x 0..100 (two triangles; a TIN
//! interpolates a plane exactly): 21 at (30, 40), 20 at (60, 20).

use std::sync::Arc;

use katana_cad::annotation::leader_edit::leader_says;
use katana_cad::annotation::leader_presets::{leader_context, leader_presets};
use katana_entity::EntityId;
use katana_geometry::Point3;
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};
use katana_window::annotation::leader_manager::{LeaderManagerContext, LeaderManagerDialog, Tab};
use katana_window::host::RecordingHost;

use crate::support::{Bench, interpreter_host, leader, run};

const TEMPLATE: &str = "Template (read off the entity)";
/// The preset box's first entry: no preset.
const NO_PRESET: &str = "(none: the note as typed)";

/// A drawing holding the plane "ground".
fn ground_host() -> RecordingHost {
    let made = TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, 10.0),
            Point3::new(100.0, 0.0, 20.0),
            Point3::new(100.0, 100.0, 40.0),
            Point3::new(0.0, 100.0, 30.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{}", error.describe()));
    let mut store = SurfaceStore::new();
    store
        .add(NamedSurface {
            name: String::from("ground"),
            surface: Some(Arc::new(made)),
            source: String::from("test plane"),
            ..NamedSurface::default()
        })
        .expect("the surface is stored");
    let mut host = interpreter_host();
    host.document_mut()
        .set_surfaces(store, "TEST_SURFACES")
        .expect("the drawing holds it");
    host
}

/// A point at `at` with level `z`.
fn point(host: &mut RecordingHost, at: &str, z: &str) -> EntityId {
    let id = run(host, &format!("POINT {at}"));
    run(host, &format!("SELECT {id}"));
    run(host, &format!("PROP SET elevation {z}"));
    id
}

/// What leader `id` says, reading the drawing's surfaces.
fn says(bench: &Bench<LeaderManagerDialog>, id: EntityId) -> String {
    let document = bench.document();
    leader_says(
        document.model(),
        &leader(&bench.host, id),
        &leader_context(document),
    )
}

/// The preset box's entry for the preset `word`.
fn title_of(word: &str) -> &'static str {
    leader_presets()
        .iter()
        .find(|info| info.word == word)
        .map(|info| info.title)
        .unwrap_or_else(|| panic!("a preset {word}"))
}

fn manager(host: RecordingHost) -> Bench<LeaderManagerDialog> {
    Bench::new(host, |document| {
        LeaderManagerDialog::new(document, LeaderManagerContext::default())
    })
}

mod leader_manager_presets {
    use super::*;

    #[test]
    fn a_chosen_preset_writes_its_template_and_apply_stores_it_in_one_step() {
        let mut host = ground_host();
        let pit = point(&mut host, "30,40", "25");
        let id = run(&mut host, &format!("LEADER #{pit} 40,50 text=PIT"));
        let mut bench = manager(host);
        assert!(bench.dialog.show_leader(bench.host.document(), id));
        bench.draw();
        assert_eq!(
            bench.value("leaderPreset"),
            NO_PRESET,
            "a plain note is no preset"
        );
        assert!(!bench.enabled("leaderPresetReads"));

        bench.fill("leaderPreset", title_of("surface"));
        assert!(bench.enabled("leaderPresetReads"));
        assert_eq!(
            bench.control("leaderPresetReads").items,
            ["(first visible surface)", "ground"]
        );
        assert_eq!(bench.value("leaderNoteKind"), TEMPLATE);
        assert_eq!(bench.value("leaderNote"), "{dsurface.ground:+.3f}");
        // 25 above ground 21; the surface nobody chose is said.
        assert_eq!(bench.dialog.preview(), "+4.000");
        assert_eq!(
            bench.dialog.problem(),
            "Reads ground, the first visible surface."
        );
        assert_eq!(says(&bench, id), "PIT", "nothing stored before Apply");

        let before = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        assert_eq!(says(&bench, id), "+4.000");
        assert_eq!(leader(&bench.host, id).text, "{dsurface.ground:+.3f}");
    }

    #[test]
    fn choose_preset_reads_the_named_surface_and_refuses_what_the_drawing_has_not() {
        let mut host = ground_host();
        let pit = point(&mut host, "30,40", "18");
        let id = run(&mut host, &format!("LEADER #{pit} 40,50 text=PIT"));
        let mut bench = manager(host);
        assert!(bench.dialog.show_leader(bench.host.document(), id));
        // 18 under ground 21: a cut of 3.
        assert!(
            bench
                .dialog
                .choose_preset(bench.host.document(), "cutfill", "ground")
        );
        assert_eq!(bench.dialog.preview(), "CUT 3.000");
        assert!(
            bench.dialog.problem().is_empty(),
            "named, nothing was chosen for the user"
        );
        assert!(
            !bench
                .dialog
                .choose_preset(bench.host.document(), "cutfill", "nope")
        );
        assert!(
            bench.dialog.problem().contains("no surface or alignment"),
            "{}",
            bench.dialog.problem()
        );
        assert!(
            !bench
                .dialog
                .choose_preset(bench.host.document(), "bogus", "")
        );
        assert!(
            bench.dialog.problem().contains("no such leader preset"),
            "{}",
            bench.dialog.problem()
        );
        // A preset that reads only its target offers nothing to read.
        assert!(
            bench
                .dialog
                .choose_preset(bench.host.document(), "level", "")
        );
        bench.draw();
        assert!(!bench.enabled("leaderPresetReads"));
        assert_eq!(bench.dialog.preview(), "RL 18.000");
    }

    #[test]
    fn a_leader_shown_says_which_preset_it_is_until_its_note_is_typed_away() {
        let mut host = ground_host();
        let pit = point(&mut host, "30,40", "25");
        let id = run(
            &mut host,
            &format!("LEADER #{pit} 40,50 preset=cutfill surface=ground"),
        );
        let mut bench = manager(host);
        assert!(bench.dialog.show_leader(bench.host.document(), id));
        bench.draw();
        assert_eq!(bench.value("leaderPreset"), title_of("cutfill"));
        assert_eq!(bench.value("leaderPresetReads"), "ground");
        assert_eq!(bench.dialog.preview(), "FILL 4.000");
        // Typed away from the preset's template: no preset, and nothing applied.
        bench.fill("leaderNote", "CUT {cut.ground}");
        assert_eq!(bench.value("leaderPreset"), NO_PRESET);
        assert!(!bench.enabled("leaderPresetReads"));
        assert_eq!(says(&bench, id), "FILL 4.000");
    }

    #[test]
    fn for_selection_makes_a_preset_leader_to_each_selected_in_one_step() {
        let mut host = ground_host();
        // 25 over ground 21 at (30, 40); 30 over ground 20 at (60, 20).
        let a = point(&mut host, "30,40", "25");
        let b = point(&mut host, "60,20", "30");
        run(&mut host, &format!("SELECT {a} {b}"));
        let mut bench = manager(host);
        bench.dialog.show_tab(Tab::ForSelection);
        bench.draw();
        assert!(
            bench
                .dialog
                .choose_for_preset(bench.host.document(), "surface", "")
        );
        bench.draw();
        assert_eq!(bench.value("leaderForNote"), "{dsurface.ground:+.3f}");
        assert_eq!(
            bench.dialog.for_preview(),
            "+4.000",
            "for the first selected"
        );
        let before = bench.steps();
        assert!(
            bench.dialog.make_for_selection(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.steps(), before + 1);
        let made = bench.document().last_created_entities();
        assert_eq!(made.len(), 2);
        assert_eq!(says(&bench, made[0]), "+4.000");
        assert_eq!(says(&bench, made[1]), "+10.000");
    }

    #[test]
    fn the_values_offer_the_surfaces_values_at_the_tip() {
        let mut host = ground_host();
        let pit = point(&mut host, "30,40", "25");
        let id = run(&mut host, &format!("LEADER #{pit} 40,50 preset=level"));
        let mut bench = manager(host);
        assert!(bench.dialog.show_leader(bench.host.document(), id));
        bench.draw();
        let rows = bench.value_rows("leaderValues");
        assert!(
            rows.iter().any(|row| row == "surface.ground=21.000"),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row == "dsurface.ground=4.000"),
            "{rows:?}"
        );
        // The list says what the leader says now.
        assert_eq!(says(&bench, id), "RL 25.000");
    }
}
