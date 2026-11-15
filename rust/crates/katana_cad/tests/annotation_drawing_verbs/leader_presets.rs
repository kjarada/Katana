//! tests/cad/annotation/test_leader_presets.cpp: leaders that read the
//! drawing's surfaces and alignments, and the presets (docs/annotation.md,
//! "What a leader can say" and "Presets"; annotation/leader_presets.rs), driven
//! as an agent drives them - the LEADER verbs on a Document holding a real
//! TIN - one undo step each.
//!
//! The surface is the plane z = 10 + 0.1 x + 0.2 y over the square 0..100 x
//! 0..100, two triangles; a TIN interpolates a plane exactly, so its level at
//! (30, 40) is 10 + 3 + 8 = 21 and at (60, 20) 10 + 6 + 4 = 20. Every other
//! expected value is worked out by hand beside it.

use std::sync::Arc;

use katana_cad::CommandInterpreter;
use katana_cad::annotation::drawing::estimated_measure;
use katana_cad::annotation::export_annotation::draw_annotation_for_export;
use katana_cad::annotation::leader_edit::leader_says;
use katana_cad::annotation::leader_presets::{
    LeaderPreset, PresetReads, leader_context, leader_preset_named, leader_presets as all_presets,
    preset_template, recognise_preset,
};
use katana_entity::leader_values::{anchor_values, check_leader_template};
use katana_entity::{
    AlignmentPlace, AnchorPoint, AnchorRef, Entity, EntityId, Geometry, LeaderContext,
    PointGeometry, PropertyValue,
};
use katana_geometry::{Point2, Point3};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

use crate::{Session, near, pt};

// The plane z = z0 + ax x + ay y over the square 0..100 x 0..100.
#[track_caller]
fn plane(z0: f64, ax: f64, ay: f64) -> Arc<TinSurface> {
    let z = |x: f64, y: f64| z0 + ax * x + ay * y;
    let made = TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, z(0.0, 0.0)),
            Point3::new(100.0, 0.0, z(100.0, 0.0)),
            Point3::new(100.0, 100.0, z(100.0, 100.0)),
            Point3::new(0.0, 100.0, z(0.0, 100.0)),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    );
    match made {
        Ok(surface) => Arc::new(surface),
        Err(error) => panic!("{}", error.describe()),
    }
}

fn named(name: &str, surface: Arc<TinSurface>, visible: bool) -> NamedSurface {
    NamedSurface {
        name: String::from(name),
        surface: Some(surface),
        source: String::from("test plane"),
        visible,
        ..NamedSurface::default()
    }
}

// A point at `at`, with an elevation when `z` is set.
#[track_caller]
fn point(s: &mut Session, at: Point2, z: Option<f64>) -> EntityId {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    if let Some(z) = z {
        entity
            .properties
            .insert(String::from("elevation"), PropertyValue::Real(z));
    }
    s.add(entity)
}

// The drawing's surfaces: "ground", the plane above, and any others.
#[track_caller]
fn surfaces(s: &mut Session, entries: Vec<NamedSurface>) {
    let mut store = SurfaceStore::new();
    for entry in entries {
        assert!(store.add(entry).is_ok());
    }
    assert!(s.document.set_surfaces(store, "TEST_SURFACES").is_ok());
}

#[track_caller]
fn ground(s: &mut Session) {
    surfaces(s, vec![named("ground", plane(10.0, 0.1, 0.2), true)]);
}

// The ground raised a metre, as one step.
#[track_caller]
fn raise_ground(s: &mut Session) {
    let mut raised = s.document.surfaces().clone();
    assert!(
        raised
            .replace("ground", Some(plane(11.0, 0.1, 0.2)))
            .is_ok()
    );
    assert!(s.document.set_surfaces(raised, "RAISE").is_ok());
}

// What leader `id` says, read as every front end reads it.
#[track_caller]
fn note(s: &Session, id: EntityId) -> String {
    leader_says(
        s.document.model(),
        &s.leader(id),
        &leader_context(&s.document),
    )
}

// The context's surfaceLevel(name, at): a context that reads no surface is
// the C++'s empty std::function, which throws when called.
#[track_caller]
fn surface_level(context: &LeaderContext, name: &str, at: Point2) -> Option<f64> {
    let Some(level) = &context.surface_level else {
        panic!("the context reads no surface");
    };
    level(name, at)
}

#[track_caller]
fn alignment_place(context: &LeaderContext, name: &str, at: Point2) -> Option<AlignmentPlace> {
    let Some(place) = &context.alignment_place else {
        panic!("the context reads no alignment");
    };
    place(name, at)
}

// The texts the export draws for `leader`, read through `context`.
#[track_caller]
fn export_texts(s: &Session, leader: EntityId, context: &LeaderContext) -> Vec<String> {
    let drawn = draw_annotation_for_export(
        s.document.model(),
        s.document.annotation_scale(),
        &estimated_measure(),
        context,
    );
    let Some(shapes) = drawn.get(&leader) else {
        panic!("the export did not draw leader {leader}");
    };
    let mut found = Vec::new();
    for shape in shapes {
        if let Geometry::Text(text) = shape {
            found.push(text.text.clone());
        }
    }
    found
}

// The place of a point: its Position.
fn position_of(id: EntityId) -> AnchorRef {
    AnchorRef {
        entity: id,
        point: AnchorPoint::Position,
        ..AnchorRef::default()
    }
}

mod leader_presets {
    use super::*;

    #[test]
    fn preset_level_says_the_points_level_and_is_read_live() {
        let mut s = Session::default();
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        let made = s.run(&format!("LEADER #{pit} 40,50 preset=level"));
        let leader = s.last();
        assert!(made.contains("text=\"RL 25.000\""), "{made}");
        assert!(made.contains(" preset=level"), "{made}");
        assert!(
            !made.contains("chosen="),
            "a level reads nothing to choose: {made}"
        );
        assert!(s.leader(leader).fields);
        assert_eq!(s.leader(leader).text, "RL {rl:.3f}");
        s.run(&format!("SELECT {pit}"));
        s.run("PROP SET elevation 26.5");
        assert_eq!(note(&s, leader), "RL 26.500");
        assert!(
            s.run(&format!("LEADER LIST {leader}"))
                .contains("preset=level")
        );
    }

    #[test]
    fn a_surface_named_none_is_the_first_visible_and_the_reply_says_it_was_chosen() {
        let mut s = Session::default();
        // A hidden surface first: the first VISIBLE one is ground.
        surfaces(
            &mut s,
            vec![
                named("old", plane(0.0, 0.0, 0.0), false),
                named("ground", plane(10.0, 0.1, 0.2), true),
            ],
        );
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        // 25 above ground 21: +4.
        let chosen = s.run(&format!("LEADER #{pit} 40,50 preset=surface"));
        assert!(chosen.contains("text=+4.000"), "{chosen}");
        assert!(chosen.contains("preset=surface surface=ground"), "{chosen}");
        assert!(
            chosen.contains("chosen=\"first visible surface\""),
            "{chosen}"
        );
        assert_eq!(s.leader(s.last()).text, "{dsurface.ground:+.3f}");
        // Named - in any case, as a surface is found - it is the drawing's
        // spelling that goes into the template, and nothing was chosen.
        let asked = s.run(&format!(
            "LEADER #{pit} 40,50 preset=surface surface=GROUND"
        ));
        assert!(!asked.contains("chosen="), "{asked}");
        assert_eq!(s.leader(s.last()).text, "{dsurface.ground:+.3f}");
        // The hidden one, named, is read all the same: hiding is the picture's.
        s.run(&format!("LEADER #{pit} 40,50 preset=surface surface=old"));
        assert_eq!(note(&s, s.last()), "+25.000");
    }

    #[test]
    fn the_note_follows_the_surface_through_an_edit_and_its_undo() {
        let mut s = Session::default();
        ground(&mut s);
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        s.run(&format!(
            "LEADER #{pit} 40,50 preset=surface surface=ground"
        ));
        let leader = s.last();
        assert_eq!(note(&s, leader), "+4.000");
        // The ground raised a metre: 25 above 22.
        raise_ground(&mut s);
        assert_eq!(note(&s, leader), "+3.000");
        assert!(s.document.undo().is_ok());
        assert_eq!(note(&s, leader), "+4.000");
    }

    #[test]
    fn cut_fill_says_the_cut_below_and_the_fill_above_one_step_for_many() {
        let mut s = Session::default();
        ground(&mut s);
        // 18 under ground 21 at (30, 40): a cut of 3. 30 over ground 20 at
        // (60, 20): a fill of 10.
        let low = point(&mut s, pt(30.0, 40.0), Some(18.0));
        let high = point(&mut s, pt(60.0, 20.0), Some(30.0));
        let before = s.steps();
        let made = s.run(&format!(
            "LEADER FOR {low} {high} preset=cutfill surface=ground"
        ));
        assert_eq!(s.steps(), before + 1, "one undo step");
        assert!(made.contains("created leaders=2"), "{made}");
        assert!(made.contains("preset=cutfill surface=ground"), "{made}");
        let ids = s.document.last_created_entities();
        assert_eq!(ids.len(), 2);
        assert_eq!(note(&s, ids[0]), "CUT 3.000");
        assert_eq!(note(&s, ids[1]), "FILL 10.000");
    }

    #[test]
    fn preset_station_reads_the_alignment_its_offset_and_its_profile() {
        let mut s = Session::default();
        // MC01 runs east along the x axis, designed from 20 at station 0 to 21
        // at 100: 20.3 at station 30. (30, -4) is 4 to the right of it.
        s.run("ALIGN NEW MC01 0,0 100,0");
        s.run("ALIGN DESIGN MC01 0,20 100,21");
        let pit = point(&mut s, pt(30.0, -4.0), Some(25.0));
        let made = s.run(&format!("LEADER #{pit} 40,-10 preset=station"));
        assert!(made.contains("preset=station alignment=MC01"), "{made}");
        assert!(made.contains("chosen=\"first alignment\""), "{made}");
        assert_eq!(note(&s, s.last()), "CH 0+030.000\nOS -4.000");
        // 25 above the design's 20.3.
        s.run(&(format!("LEADER #{pit} 40,-10 ") + "template=\"{dprofile.MC01:+.3f}\""));
        assert_eq!(note(&s, s.last()), "+4.700");
    }

    #[test]
    fn what_a_preset_cannot_make_is_refused_and_nothing_is_made() {
        let mut s = Session::default();
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        let at = format!("LEADER #{pit} 40,50 ");
        let line = |rest: &str| format!("{at}{rest}");
        // No surface in the drawing yet.
        assert!(
            s.refusal(&line("preset=surface"))
                .contains("the drawing has none")
        );
        surfaces(&mut s, vec![named("ground", plane(10.0, 0.1, 0.2), false)]);
        assert!(
            s.refusal(&line("preset=surface"))
                .contains("every surface is hidden")
        );
        ground(&mut s);
        assert!(
            s.refusal(&line("preset=bogus"))
                .contains("no such leader preset")
        );
        assert!(
            s.refusal(&line("preset=surface surface=nope"))
                .contains("no surface of that name")
        );
        assert!(
            s.refusal(&line("preset=level surface=ground"))
                .contains("surface= is for preset=surface")
        );
        assert!(
            s.refusal(&line("template=\"{rl}\" surface=ground"))
                .contains("say what a preset= reads")
        );
        assert!(
            s.refusal(&line("preset=level text=A"))
                .contains("one of text=")
        );
        assert!(
            s.refusal(&line("preset=station"))
                .contains("reads an alignment")
        );
        assert!(
            s.refusal(&line("preset=station alignment=MC09"))
                .contains("no alignment of that name")
        );
        // A point with no level says no level; one off the surface no height
        // above it - each refused saying why, not made blank.
        let flat = point(&mut s, pt(30.0, 40.0), None);
        assert!(
            s.refusal(&format!("LEADER #{flat} 40,50 preset=level"))
                .contains("would say nothing")
        );
        let far = point(&mut s, pt(150.0, 40.0), Some(25.0));
        assert!(
            s.refusal(&format!("LEADER #{far} 160,50 preset=surface"))
                .contains("the tip is off surface ground")
        );
        assert_eq!(
            s.document.model().entities.size(),
            3,
            "three points and no leader"
        );
    }

    #[test]
    fn leader_presets_lists_each_with_its_template_and_tool() {
        let mut s = Session::default();
        let list = s.run("LEADER PRESETS");
        assert_eq!(list.matches('\n').count(), 6, "seven presets: {list}");
        assert!(
            list.contains(concat!(
                "preset=level title=Level reads=target template=\"RL {rl:.3f}\" ",
                "tool=annotate.leader.level"
            )),
            "{list}"
        );
        assert!(
            list.contains(concat!(
                "preset=surface title=\"Surface Difference\" reads=surface ",
                "template={dsurface.NAME:+.3f} tool=annotate.leader.surface"
            )),
            "{list}"
        );
        assert!(list.contains("preset=details"), "{list}");
        assert!(
            list.contains("tool=none"),
            "details has no tool of its own: {list}"
        );
    }

    #[test]
    fn list_json_says_what_each_leader_says_now_and_what_it_reads() {
        let mut s = Session::default();
        ground(&mut s);
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        s.run(&format!("LEADER #{pit} 40,50 preset=surface"));
        let json = s.run("LEADER LIST JSON");
        assert!(json.contains("\"preset\": \"surface\""), "{json}");
        assert!(json.contains("\"reads\": \"ground\""), "{json}");
        assert!(json.contains("\"text\": \"+4.000\""), "{json}");
        assert!(json.contains(&format!("\"target\": {pit}")), "{json}");
        assert!(
            json.contains("\"surfaces\": [\n    \"ground\"\n  ]"),
            "{json}"
        );
        assert!(json.contains("\"name\": \"cutfill\""), "{json}");
    }

    #[test]
    fn values_list_each_surfaces_values_that_hold_at_the_place() {
        let mut s = Session::default();
        ground(&mut s);
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        let values = s.run(&format!("LEADER VALUES #{pit}"));
        assert!(
            values.contains("value=surface.ground text=21.000"),
            "{values}"
        );
        assert!(
            values.contains("value=dsurface.ground text=4.000"),
            "{values}"
        );
        assert!(values.contains("value=fill.ground text=4.000"), "{values}");
        assert!(
            !values.contains("value=cut.ground"),
            "above the ground, no cut: {values}"
        );
    }

    #[test]
    fn each_presets_template_is_recognised_with_the_name_it_reads() {
        for info in all_presets() {
            let made = match preset_template(info.preset, "Existing Ground") {
                Ok(made) => made,
                Err(error) => panic!("{}: {}", info.word, error.describe()),
            };
            let Some(recognised) = recognise_preset(&made) else {
                panic!("{made}");
            };
            assert_eq!(recognised.preset, info.preset, "{made}");
            assert_eq!(
                recognised.reads,
                if info.reads == PresetReads::Target {
                    ""
                } else {
                    "Existing Ground"
                },
                "{made}"
            );
            assert!(check_leader_template(&made).is_ok(), "{made}");
        }
        assert!(
            recognise_preset("RL {rl:.2f}").is_none(),
            "not the preset's format"
        );
        assert!(
            recognise_preset("CUT {cut.a:.3f}\nFILL {fill.b:.3f}").is_none(),
            "cut and fill of two surfaces"
        );
        // A name the template language cannot write is refused; a preset that
        // reads only its target ignores any.
        assert!(preset_template(LeaderPreset::Surface, "a:b").is_err());
        assert!(preset_template(LeaderPreset::Station, "").is_err());
        assert_eq!(
            preset_template(LeaderPreset::Level, "a:b").ok().as_deref(),
            Some("RL {rl:.3f}")
        );
        assert_eq!(
            leader_preset_named("CutFill").ok(),
            Some(LeaderPreset::CutFill)
        );
    }

    #[test]
    fn a_frozen_surface_leader_keeps_what_it_said_when_frozen() {
        let mut s = Session::default();
        ground(&mut s);
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        s.run(&format!("LEADER #{pit} 40,50 preset=surface"));
        let leader = s.last();
        s.run(&format!("LEADER FREEZE {leader}"));
        assert!(!s.leader(leader).fields);
        assert_eq!(
            s.leader(leader).text,
            "+4.000",
            "frozen as the surface made it read"
        );
        raise_ground(&mut s);
        assert_eq!(note(&s, leader), "+4.000");
    }

    #[test]
    fn leader_set_makes_a_leaders_note_a_preset() {
        let mut s = Session::default();
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        s.run(&format!("LEADER #{pit} 40,50 text=PIT"));
        let leader = s.last();
        let before = s.steps();
        let set = s.run(&format!("LEADER SET {leader} preset=level"));
        assert_eq!(s.steps(), before + 1);
        assert!(set.contains("updated leaders=1 preset=level"), "{set}");
        assert_eq!(note(&s, leader), "RL 25.000");
    }

    #[test]
    fn the_context_is_the_drawing_as_it_was_when_it_was_made() {
        let mut s = Session::default();
        ground(&mut s);
        s.run("ALIGN NEW MC01 0,0 100,0");
        let before = leader_context(&s.document);
        assert_eq!(before.surfaces, ["ground"]);
        assert_eq!(before.alignments, ["MC01"]);
        assert!(surface_level(&before, "ground", pt(30.0, 40.0)).is_some());
        // A surface is found ignoring case.
        let Some(level) = surface_level(&before, "GROUND", pt(30.0, 40.0)) else {
            panic!("GROUND was not found");
        };
        near(level, 21.0, 1e-9);
        assert!(
            surface_level(&before, "ground", pt(150.0, 40.0)).is_none(),
            "off it"
        );
        assert!(
            alignment_place(&before, "MC01", pt(150.0, 0.0)).is_none(),
            "beyond its end"
        );
        let Some(place) = alignment_place(&before, "MC01", pt(30.0, -4.0)) else {
            panic!("(30, -4) is not along MC01");
        };
        near(place.station, 30.0, 1e-9);
        near(place.offset, -4.0, 1e-9);
        assert!(place.profile.is_none(), "MC01 has no design profile");
        // The ground raised: the context made before reads what it was made
        // from; one made now, the raised ground.
        raise_ground(&mut s);
        let Some(then) = surface_level(&before, "ground", pt(30.0, 40.0)) else {
            panic!("the context made before lost the ground");
        };
        near(then, 21.0, 1e-9);
        let Some(now) = surface_level(&leader_context(&s.document), "ground", pt(30.0, 40.0))
        else {
            panic!("the context made now has no ground");
        };
        near(now, 22.0, 1e-9);
    }

    #[test]
    fn list_and_the_export_say_the_note_the_surface_gives_it() {
        let mut s = Session::default();
        ground(&mut s);
        let pit = point(&mut s, pt(30.0, 40.0), Some(25.0));
        s.run(&format!("LEADER #{pit} 40,50 preset=surface"));
        let leader = s.last();
        assert!(s.run("LIST").contains("\"+4.000\""));
        assert!(s.run(&format!("INFO {leader}")).contains("\"+4.000\""));
        // A file is handed the note as the plan view draws it, read off the
        // surface through the context; with none, the line is left out and
        // the leader is drawn bare.
        assert_eq!(
            export_texts(&s, leader, &leader_context(&s.document)),
            ["+4.000"]
        );
        assert!(export_texts(&s, leader, &LeaderContext::default()).is_empty());
    }

    #[test]
    fn every_point_on_the_surface_inside_a_triangle_is_a_fill_of_nothing_never_a_cut() {
        // The plane z = 10 + 0.1 x + 0.2 y, and points whose RL is that to the
        // last digit the arithmetic gives. The TIN interpolates by barycentric
        // weights, which is not the same rounding, so about one such point in
        // six came out a few bits BELOW the surface and read as a cut of
        // 4e-15: of a 19 x 19 grid, 61 "CUT 0.000" beside 281 "FILL 0.000".
        // Hand-worked: at (5.4, 5) the plane is 10 + 0.54 + 1.0 = 11.54.
        let mut s = Session::default();
        ground(&mut s);
        let first = point(&mut s, pt(5.4, 5.0), Some(11.54));
        assert!(
            s.run(&format!("LEADER #{first} 20,20 preset=cutfill"))
                .contains("text=\"FILL 0.000\"")
        );
        // A leader that says only the fill is accepted on a point on grade.
        assert!(
            s.run(&(format!("LEADER #{first} 25,25 ") + "template=\"FILL {fill.ground:.3f}\""))
                .contains("created leader")
        );
        let context = leader_context(&s.document);
        let mut on_grade = 0;
        for i in 0..20 {
            for j in 0..20 {
                let x = 1.7 * f64::from(i) + 0.3;
                let y = 2.3 * f64::from(j) + 0.7;
                let id = point(&mut s, pt(x, y), Some(10.0 + 0.1 * x + 0.2 * y));
                let entity = s.entity(id);
                let values = anchor_values(&entity, &position_of(id), pt(x, y), &[], &context);
                assert!(
                    !values.contains_key("cut.ground"),
                    "({x}, {y}) read as a cut"
                );
                assert!(values.contains_key("fill.ground"), "({x}, {y})");
                on_grade += 1;
            }
        }
        assert_eq!(on_grade, 400);
        // A point a micrometre under the surface is under it, by more than the
        // tolerance: a cut of a micrometre, not a fill.
        let below = point(&mut s, pt(5.4, 5.0), Some(11.54 - 1.0e-6));
        let entity = s.entity(below);
        let cut = anchor_values(&entity, &position_of(below), pt(5.4, 5.0), &[], &context);
        assert!(cut.contains_key("cut.ground"));
        assert!(!cut.contains_key("fill.ground"));
    }

    #[test]
    fn the_usage_of_leader_set_names_the_preset_options_and_help_reads_as_sentences() {
        // LEADER SET accepts preset= surface= and alignment=; its usage line,
        // said when no option is typed, listed only the older ones, and HELP
        // ran the new paragraph on from "...ends in a dot" with a lower-case
        // "also".
        let mut s = Session::default();
        let pit = point(&mut s, pt(10.0, 10.0), Some(5.0));
        s.run(&format!("LEADER #{pit} 20,20 text=A"));
        let leader = s.last();
        let usage = s.refusal(&format!("LEADER SET {leader}"));
        assert!(usage.contains("usage: LEADER SET"), "{usage}");
        assert!(usage.contains("preset="), "{usage}");
        assert!(usage.contains("surface=name"), "{usage}");
        assert!(usage.contains("alignment=name"), "{usage}");
        let help = CommandInterpreter::help_text();
        assert!(
            help.contains("(#id.inside) ends in a dot."),
            "the sentence ends"
        );
        assert!(!help.contains("ends in a dot\n"), "and does not run on");
        assert!(!help.contains("ends in a dot\r"));
        assert!(
            help.contains("A note may also read the drawing's surfaces and alignments at the tip")
        );
    }
}
