// The preset leader tools (annotate_leader_presets.rs; docs/annotation.md,
// "Presets"; docs/tools.md): Level, Surface Difference, Cut/Fill, Point
// Name, Code and Station Leader, driven through ToolDriver as the plan view
// drives them, each checked against the LEADER line that makes the same
// leader (tests/cad/tools/test_leader_preset_tools.cpp).
//
// The surface is the plane z = 10 + 0.1 x + 0.2 y over 0..100 x 0..100 (two
// triangles; a TIN interpolates a plane exactly): 21 at (30, 40).

use std::sync::Arc;

use katana_cad::Document;
use katana_cad::annotation::leader_edit::leader_says;
use katana_cad::annotation::leader_presets::leader_context;
use katana_commands::create_entities;
use katana_entity::{Entity, EntityId, Geometry, PointGeometry, PropertyValue};
use katana_geometry::{Point2, Point3};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

use crate::ok;

fn plane(z0: f64) -> Arc<TinSurface> {
    let z = |x: f64, y: f64| z0 + 0.1 * x + 0.2 * y;
    match TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, z(0.0, 0.0)),
            Point3::new(100.0, 0.0, z(100.0, 0.0)),
            Point3::new(100.0, 100.0, z(100.0, 100.0)),
            Point3::new(0.0, 100.0, z(0.0, 100.0)),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    ) {
        Ok(surface) => Arc::new(surface),
        Err(error) => panic!("{}", error.describe()),
    }
}

// "ground" (z0 = 10) and "design" (z0 = 12) in `document`.
#[track_caller]
fn add_surfaces(document: &mut Document) {
    let mut store = SurfaceStore::default();
    for (name, z0) in [("ground", 10.0), ("design", 12.0)] {
        let entry = NamedSurface {
            name: String::from(name),
            surface: Some(plane(z0)),
            source: String::from("test plane"),
            ..NamedSurface::default()
        };
        ok(store.add(entry));
    }
    ok(document.set_surfaces(store, "TEST_SURFACES"));
}

#[track_caller]
fn add_point(document: &mut Document, at: Point2, z: Option<f64>) -> EntityId {
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    if let Some(z) = z {
        point
            .properties
            .insert(String::from("elevation"), PropertyValue::from(z));
    }
    ok(document.execute(create_entities(vec![point])));
    document.last_created_entities()[0]
}

fn leaders_of(document: &Document) -> Vec<Entity> {
    let mut found = Vec::new();
    document.model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Leader(_)) {
            found.push(entity.clone());
        }
    });
    found
}

#[track_caller]
fn says(document: &Document, leader: &Entity) -> String {
    let Geometry::Leader(geometry) = &leader.geometry else {
        panic!("not a leader: {:?}", leader.geometry);
    };
    leader_says(document.model(), geometry, &leader_context(document))
}

mod leader_preset_tools {
    use katana_cad::annotation::leader_presets::leader_presets;
    use katana_cad::interactive_tool::{tool_catalog, tool_catalog_problems};
    use katana_cad::{CommandInterpreter, Document};
    use katana_commands::create_entities;
    use katana_entity::{Entity, Geometry, PointGeometry, PropertyValue};
    use katana_math::numerics::PI;

    use super::{add_point, add_surfaces, leaders_of, says};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, contains, near, ok, pt};

    #[test]
    fn each_preset_has_a_tool_of_its_own_on_the_annotate_menu_with_its_verb() {
        let tools = [
            ("annotate.leader.level", "LEADERLEVEL"),
            ("annotate.leader.surface", "LEADERSURFACE"),
            ("annotate.leader.cutfill", "LEADERCUTFILL"),
            ("annotate.leader.name", "LEADERNAME"),
            ("annotate.leader.code", "LEADERCODE"),
            ("annotate.leader.station", "LEADERSTATION"),
        ];
        for (id, verb) in tools {
            let Some(info) = tool_catalog().find(id) else {
                panic!("{id}");
            };
            assert_eq!(info.category, "Annotate", "{id}");
            assert_eq!(info.group, "Leaders", "{id}");
            assert!(
                tool_catalog()
                    .find_by_alias(verb)
                    .is_some_and(|found| std::ptr::eq(found, info)),
                "{verb}"
            );
            assert!(
                !info.name.contains(','),
                "{id}: not a family, so each is a button of its own"
            );
        }
        // And every preset with a tool names it.
        for preset in leader_presets() {
            if !preset.tool.is_empty() {
                assert!(
                    tool_catalog().find(preset.tool).is_some(),
                    "{}",
                    preset.word
                );
            }
        }
        assert!(tool_catalog_problems().is_empty());
    }

    #[test]
    fn the_level_leader_tool_makes_what_the_leader_line_makes() {
        let mut driver = ToolDriver::new();
        let pit = add_point(driver.document_mut(), pt(30.0, 40.0), Some(25.0));
        driver.start("annotate.leader.level");
        let tip = driver.click(30.0, 40.0);
        assert_eq!(tip.outcome, CONTINUE, "{}", tip.message);
        assert!(
            contains(
                &tip.message,
                &format!("Level leader on Point {pit}: RL 25.000")
            ),
            "{}",
            tip.message
        );
        let done = driver.click(40.0, 50.0);
        assert_eq!(done.outcome, DONE, "{}", done.message);
        assert_eq!(driver.executed(), 1);

        // The same answers typed.
        let mut typed = Document::default();
        let mut interpreter = CommandInterpreter::new();
        add_point(&mut typed, pt(30.0, 40.0), Some(25.0));
        if let Err(error) = interpreter.run(&mut typed, "LEADER #1@30,40 40,50 preset=level") {
            panic!("{}", error.describe());
        }

        let made = leaders_of(driver.document());
        let line = leaders_of(&typed);
        assert_eq!(made.len(), 1);
        assert_eq!(line.len(), 1);
        assert_eq!(made[0].geometry, line[0].geometry);
        assert_eq!(made[0].layer, line[0].layer);
        assert_eq!(says(driver.document(), &made[0]), "RL 25.000");
    }

    #[test]
    fn the_surface_tool_reads_the_surface_chosen_with_s_and_says_the_difference() {
        let mut driver = ToolDriver::new();
        add_surfaces(driver.document_mut());
        add_point(driver.document_mut(), pt(30.0, 40.0), Some(25.0));
        driver.start("annotate.leader.surface");
        assert!(
            contains(&driver.prompt(), "[Surface]"),
            "{}",
            driver.prompt()
        );
        assert!(
            contains(&driver.prompt(), "the first visible surface"),
            "{}",
            driver.prompt()
        );
        assert_eq!(driver.r#type("S").outcome, CONTINUE);
        assert_eq!(driver.r#type("nope").outcome, REJECTED, "no such surface");
        assert_eq!(driver.r#type("DESIGN").outcome, CONTINUE);
        assert!(
            contains(&driver.prompt(), "surface: design"),
            "{}",
            driver.prompt()
        );
        driver.click(30.0, 40.0);
        assert_eq!(driver.click(40.0, 50.0).outcome, DONE);
        // design is 12 + 3 + 8 = 23 at (30, 40): RL 25 is 2 above it.
        let made = leaders_of(driver.document());
        assert_eq!(made.len(), 1);
        let Geometry::Leader(leader) = &made[0].geometry else {
            panic!("not a leader");
        };
        assert_eq!(leader.text, "{dsurface.design:+.3f}");
        assert_eq!(says(driver.document(), &made[0]), "+2.000");
    }

    #[test]
    fn a_surface_tool_offers_the_surface_the_newest_leader_of_its_preset_reads() {
        let mut driver = ToolDriver::new();
        add_surfaces(driver.document_mut());
        add_point(driver.document_mut(), pt(30.0, 40.0), Some(25.0));
        let mut interpreter = CommandInterpreter::new();
        if let Err(error) = interpreter.run(
            driver.document_mut(),
            "LEADER #1 40,50 preset=cutfill surface=design",
        ) {
            panic!("{}", error.describe());
        }
        driver.start("annotate.leader.cutfill");
        assert!(
            contains(&driver.prompt(), "surface: design"),
            "{}",
            driver.prompt()
        );
        driver.click(30.0, 40.0);
        assert_eq!(driver.click(20.0, 50.0).outcome, DONE);
        // 25 over design's 23: a fill of 2.
        let made = leaders_of(driver.document());
        assert_eq!(made.len(), 2);
        assert_eq!(says(driver.document(), &made[1]), "FILL 2.000");
    }

    #[test]
    fn a_point_with_nothing_to_say_is_refused_at_the_first_click_saying_why() {
        let mut driver = ToolDriver::new();
        add_point(driver.document_mut(), pt(30.0, 40.0), None);
        driver.start("annotate.leader.level");
        let refused = driver.click(30.0, 40.0);
        assert_eq!(refused.outcome, REJECTED);
        assert!(
            contains(&refused.message, "would say nothing"),
            "{}",
            refused.message
        );
        assert!(
            contains(&driver.prompt(), "Pick what the note reads"),
            "still asking for the point"
        );
        // And a click on nothing.
        let nothing = driver.click(70.0, 70.0);
        assert_eq!(nothing.outcome, REJECTED);
        assert!(
            contains(&nothing.message, "nothing is under the cursor"),
            "{}",
            nothing.message
        );
        assert_eq!(driver.executed(), 0);
    }

    #[test]
    fn enter_puts_the_note_where_leader_for_puts_it() {
        let mut driver = ToolDriver::new();
        add_point(driver.document_mut(), pt(30.0, 40.0), Some(25.0));
        driver.start("annotate.leader.level");
        driver.click(30.0, 40.0);
        assert_eq!(driver.enter().outcome, DONE);
        // LeadersForOptions' 10 mm on paper at 45 degrees, at the drawing's
        // scale: 10 x scale / 1000 model units along (cos 45, sin 45).
        let reach = 10.0 * driver.document().annotation_scale() / 1000.0;
        let step = reach * (0.25 * PI).cos();
        let made = leaders_of(driver.document());
        assert_eq!(made.len(), 1);
        let Geometry::Leader(leader) = &made[0].geometry else {
            panic!("not a leader");
        };
        let vertices = &leader.vertices;
        assert_eq!(vertices.len(), 2);
        near(vertices[1].x, 30.0 + step, 1e-9);
        near(vertices[1].y, 40.0 + step, 1e-9);
    }

    #[test]
    fn the_tool_starts_again_so_a_row_of_levels_is_two_clicks_each() {
        let mut driver = ToolDriver::new();
        add_point(driver.document_mut(), pt(30.0, 40.0), Some(25.0));
        add_point(driver.document_mut(), pt(60.0, 20.0), Some(26.25));
        driver.start("annotate.leader.level");
        driver.click(30.0, 40.0);
        driver.click(35.0, 45.0);
        driver.click(60.0, 20.0);
        driver.click(65.0, 25.0);
        assert_eq!(driver.executed(), 2, "one step each");
        assert!(!driver.finished());
        let made = leaders_of(driver.document());
        assert_eq!(made.len(), 2);
        assert_eq!(says(driver.document(), &made[0]), "RL 25.000");
        assert_eq!(says(driver.document(), &made[1]), "RL 26.250");
        // Enter at the first prompt ends it.
        assert_eq!(driver.enter().outcome, DONE);
        assert!(driver.finished());
    }

    #[test]
    fn name_code_and_station_tools_say_what_their_presets_say() {
        let mut driver = ToolDriver::new();
        let mut interpreter = CommandInterpreter::new();
        // MC01 east along the x axis: (30, -4) is at station 30, 4 to the right.
        if let Err(error) = interpreter.run(driver.document_mut(), "ALIGN NEW MC01 0,0 100,0") {
            panic!("{}", error.describe());
        }
        let mut point = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: pt(30.0, -4.0),
            }),
            ..Entity::default()
        };
        point
            .properties
            .insert(String::from("point"), PropertyValue::from("1001"));
        point
            .properties
            .insert(String::from("code"), PropertyValue::from("TB"));
        point
            .properties
            .insert(String::from("elevation"), PropertyValue::from(25.0));
        ok(driver.document_mut().execute(create_entities(vec![point])));

        let expected = [
            ("annotate.leader.name", "1001"),
            ("annotate.leader.code", "TB"),
            ("annotate.leader.station", "CH 0+030.000\nOS -4.000"),
        ];
        for (tool, note) in expected {
            driver.start(tool);
            assert_eq!(driver.click(30.0, -4.0).outcome, CONTINUE, "{tool}");
            assert_eq!(driver.click(40.0, 5.0).outcome, DONE, "{tool}");
            let made = leaders_of(driver.document());
            let Some(newest) = made.last() else {
                panic!("no leader");
            };
            assert_eq!(says(driver.document(), newest), note, "{tool}");
        }
    }
}
