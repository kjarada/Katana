//! tests/cad/annotation/test_leader_scope.cpp: the leader verbs take the one
//! scope and filter grammar (CLAUDE.md 1.1; docs/annotation.md, "Scope and
//! filter"): LEADER LIST, VALUES, SET, FREEZE, DETACH, PROP, ALIGN and FOR and
//! BALLOON FOR and RENUMBER act on ids, or on what [SELECTION | VIEW | DRAWING
//! | AREA | LAYERS] [WHERE ...] takes, and say how many it took. A surface or
//! an alignment a note reads follows its rename and is counted when it goes
//! (the leaders' reading by name, L3).
//!
//! Every expected value is worked out by hand in a comment beside it.
//!
//! Not twinned: LeaderScope.AByteThatIsNotUtf8InAnyNameIsWrittenAsTheReplacementCharacterNotThrown
//! (L4), whose only subject is a byte that is not UTF-8 in a plain-data
//! report: a Rust String cannot hold one (leader_presets.rs says so where the
//! report is declared), so it is an exemption candidate.

use std::sync::Arc;

use katana_cad::ScopeView;
use katana_cad::annotation::command_words::anchor_word;
use katana_cad::annotation::leader_edit::{
    ReadKind, leader_says, leaders_reading, rename_leader_reads,
};
use katana_cad::annotation::leader_presets::{leader_context, leaders_json, leaders_report};
use katana_core::Result;
use katana_entity::label_text::rename_template_fields;
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, Entity, EntityId, Geometry, PointGeometry, PropertyMap,
    PropertyValue,
};
use katana_geometry::{Box2, Point2, Point3};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

use crate::{Session, leading_number, pt, real_of};

// A point on `layer` (which exists) with its properties.
#[track_caller]
fn point(s: &mut Session, at: Point2, layer: &str, properties: PropertyMap) -> EntityId {
    s.add(Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        layer: String::from(layer),
        properties,
        ..Entity::default()
    })
}

// One real property.
fn real(key: &str, value: f64) -> PropertyMap {
    PropertyMap::from([(String::from(key), PropertyValue::Real(value))])
}

// Runs a line that makes one entity and says its id.
#[track_caller]
fn made(s: &mut Session, line: &str) -> EntityId {
    s.run(line);
    s.last()
}

#[track_caller]
fn says(s: &Session, id: EntityId) -> String {
    leader_says(
        s.document.model(),
        &s.leader(id),
        &leader_context(&s.document),
    )
}

// Where leader `id`'s note hangs: its last vertex.
#[track_caller]
fn hang(s: &Session, id: EntityId) -> Point2 {
    match s.leader(id).vertices.last() {
        Some(&at) => at,
        None => panic!("leader {id} has no vertex"),
    }
}

fn first_line(reply: &str) -> &str {
    reply.find('\n').map_or(reply, |at| &reply[..at])
}

// The ids of the "id=N ..." lines of a LEADER LIST reply, in the order given.
fn listed_ids(reply: &str) -> Vec<EntityId> {
    reply
        .lines()
        .filter_map(|line| line.strip_prefix("id="))
        .map(leading_number)
        .collect()
}

fn count_lines_starting(reply: &str, prefix: &str) -> usize {
    reply
        .lines()
        .filter(|line| line.starts_with(prefix))
        .count()
}

// Two pits and five leaders on three layers:
//
//   layer 0      p1 (0, 0) invert 1.5       p2 (50, 50) invert 2.5
//   layer notes  a1 smart, tip on p1, note hangs at (5, 5)
//                a2 smart, tip on p2, note hangs at (55, 55)
//                a4 smart, tip on p1, note hangs at (6, 6)
//                b1 plain, tip on nothing, (100, 100) to (110, 110)
//   layer other  a3 smart, tip on p1, note hangs at (-5, -5)
//
// Every smart note is "IL {prop.invert:.2f}": 1.50 on p1, 2.50 on p2.
struct Pits {
    s: Session,
    p1: EntityId,
    p2: EntityId,
    a1: EntityId,
    a2: EntityId,
    a3: EntityId,
    a4: EntityId,
    b1: EntityId,
}

impl Pits {
    #[track_caller]
    fn new() -> Self {
        let mut s = Session::default();
        s.run("LAYER NEW notes");
        s.run("LAYER NEW other");
        let p1 = point(&mut s, pt(0.0, 0.0), "0", real("invert", 1.5));
        let p2 = point(&mut s, pt(50.0, 50.0), "0", real("invert", 2.5));
        s.run("LAYER SET notes");
        let note = " template=\"IL {prop.invert:.2f}\"";
        let a1 = made(&mut s, &(format!("LEADER #{p1} 5,5") + note));
        let a2 = made(&mut s, &(format!("LEADER #{p2} 55,55") + note));
        let a4 = made(&mut s, &(format!("LEADER #{p1} 6,6") + note));
        let b1 = made(&mut s, "LEADER 100,100 110,110 text=plain");
        s.run("LAYER SET other");
        let a3 = made(&mut s, &(format!("LEADER #{p1} -5,-5") + note));
        s.run("LAYER SET 0");
        Self {
            s,
            p1,
            p2,
            a1,
            a2,
            a3,
            a4,
            b1,
        }
    }
}

// Points with a level on layer plan (q1, q2) and on layer other (q3). No case
// reads q2, so it is made and not kept.
struct Levels {
    s: Session,
    q1: EntityId,
    q3: EntityId,
}

impl Levels {
    #[track_caller]
    fn new() -> Self {
        let mut s = Session::default();
        s.run("LAYER NEW plan");
        s.run("LAYER NEW other");
        let q1 = point(&mut s, pt(0.0, 0.0), "plan", real("elevation", 25.0));
        point(&mut s, pt(10.0, 0.0), "plan", real("elevation", 30.0));
        let q3 = point(&mut s, pt(20.0, 0.0), "other", real("elevation", 40.0));
        Self { s, q1, q3 }
    }
}

#[track_caller]
fn plane(z0: f64, ax: f64, ay: f64) -> Arc<TinSurface> {
    let z = |x: f64, y: f64| z0 + ax * x + ay * y;
    let tin = TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, z(0.0, 0.0)),
            Point3::new(100.0, 0.0, z(100.0, 0.0)),
            Point3::new(100.0, 100.0, z(100.0, 100.0)),
            Point3::new(0.0, 100.0, z(0.0, 100.0)),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    );
    match tin {
        Ok(surface) => Arc::new(surface),
        Err(error) => panic!("{}", error.describe()),
    }
}

// The plane z = 10 + 0.1 x + 0.2 y, named "ground": 21 at (30, 40), by hand
// 10 + 3 + 8. A point at RL 25 there is 4 above it.
struct Ground {
    s: Session,
    pit: EntityId,
}

impl Ground {
    #[track_caller]
    fn new() -> Self {
        let mut s = Session::default();
        let entry = NamedSurface {
            name: String::from("ground"),
            surface: Some(plane(10.0, 0.1, 0.2)),
            source: String::from("test plane"),
            visible: true,
            ..NamedSurface::default()
        };
        let mut store = SurfaceStore::new();
        assert!(store.add(entry).is_ok());
        assert!(s.document.set_surfaces(store, "TEST_SURFACES").is_ok());
        s.run("LAYER NEW notes");
        let pit = point(&mut s, pt(30.0, 40.0), "0", real("elevation", 25.0));
        Self { s, pit }
    }
}

fn anchor(entity: EntityId, point: AnchorPoint, index: u32, parameter: f64) -> AnchorRef {
    AnchorRef {
        entity,
        point,
        index,
        parameter,
    }
}

mod leader_scope {
    use super::*;

    // ---- reading: LIST and VALUES ---------------------------------------------------------------

    #[test]
    fn list_takes_the_leaders_of_a_layer_and_says_how_many_matched() {
        let Pits {
            mut s,
            a1,
            a2,
            a3,
            a4,
            b1,
            ..
        } = Pits::new();
        let reply = s.run("LEADER LIST LAYERS notes");
        // notes holds a1, a2, a4 and b1: four. The other layer's a3 is not.
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=notes sublayers=yes matched=4",
            "{reply}"
        );
        assert_eq!(listed_ids(&reply), [a1, a2, a4, b1], "{reply}");
        // The whole drawing: five leaders, and the points are not among them.
        let all = s.run("LEADER LIST DRAWING");
        assert_eq!(first_line(&all), "scope=drawing matched=5", "{all}");
        assert_eq!(listed_ids(&all).len(), 5);
        // Ids still name them, and carry no scope record.
        let named = s.run(&format!("LEADER LIST {a2} {a3}"));
        assert_eq!(listed_ids(&named), [a2, a3], "{named}");
        assert!(!named.contains("scope="), "{named}");
        // And a bare LIST is every leader, as it always was.
        assert_eq!(listed_ids(&s.run("LEADER LIST")).len(), 5);
    }

    #[test]
    fn list_takes_a_where_filter_and_an_area_and_the_selection() {
        let Pits {
            mut s,
            p1,
            a1,
            a2,
            a3,
            a4,
            b1,
            ..
        } = Pits::new();
        // Only the smart notes on notes: WHERE LAYER= is the filter's, on the
        // leader's own layer.
        let on_notes = s.run("LEADER LIST DRAWING WHERE LAYER=notes");
        assert_eq!(listed_ids(&on_notes), [a1, a2, a4, b1], "{on_notes}");
        assert!(first_line(&on_notes).contains("matched=4"), "{on_notes}");
        // AREA -1,-1,30,30 overlaps the boxes of a1 (0,0)-(5,5), a4 (0,0)-(6,6)
        // and a3 (-5,-5)-(0,0), and not a2 (50,50)-(55,55) or b1 (100,100)-
        // (110,110). Listed in id order: a3 was made after a4.
        let area = s.run("LEADER LIST AREA -1,-1,30,30");
        assert_eq!(
            first_line(&area),
            "scope=area area=-1,-1,30,30 matched=3",
            "{area}"
        );
        assert_eq!(listed_ids(&area), [a1, a4, a3], "{area}");
        // The selection: only the leader selected.
        s.run(&format!("SELECT {a2}"));
        let selected = s.run("LEADER LIST SELECTION");
        assert_eq!(
            first_line(&selected),
            "scope=selection matched=1",
            "{selected}"
        );
        assert_eq!(listed_ids(&selected), [a2]);
        // The selection of a point, which is no leader, takes no leader: said,
        // not refused.
        s.run(&format!("SELECT {p1}"));
        let none = s.run("LEADER LIST SELECTION");
        assert_eq!(none, "scope=selection matched=0");
    }

    #[test]
    fn a_scope_that_takes_no_leader_lists_none_and_never_all() {
        let Pits { mut s, .. } = Pits::new();
        // The trap: a list of ids that is empty means "all" to leaders_json,
        // and a scope that took nothing must not become that.
        let text = s.run("LEADER LIST DRAWING WHERE TYPE=point");
        assert!(text.contains("matched=0"), "{text}");
        assert!(listed_ids(&text).is_empty(), "{text}");
        let json = s.run("LEADER LIST DRAWING WHERE TYPE=point JSON");
        assert!(json.contains("\"leaders\": []"), "{json}");
        assert!(json.contains("\"matched\": 0"), "{json}");
        // JSON before or after the scope reads the same, and says the scope.
        let after = s.run("LEADER LIST LAYERS notes JSON");
        let before = s.run("LEADER LIST JSON LAYERS notes");
        assert_eq!(after, before);
        assert!(after.contains("\"matched\": 4"), "{after}");
        assert!(
            after.contains("\"record\": \"scope=layers layers=notes sublayers=yes matched=4\""),
            "{after}"
        );
        // Four leaders, each with an "id" member; the presets have "name".
        assert_eq!(after.matches("\"id\":").count(), 4);
        // With no scope the JSON has none.
        assert!(!s.run("LEADER LIST JSON").contains("\"scope\""));
    }

    #[test]
    fn ids_and_a_scope_together_are_refused_and_nothing_is_guessed() {
        let Pits { mut s, p1, a1, .. } = Pits::new();
        assert!(
            s.refusal(&format!("LEADER LIST {a1} DRAWING"))
                .contains("by id or by a scope")
        );
        assert!(
            s.refusal(&format!("LEADER FREEZE {a1} LAYERS notes"))
                .contains("by id or by a scope")
        );
        assert!(
            s.refusal(&format!("LEADER FOR {p1} DRAWING"))
                .contains("by id or by a scope")
        );
        // Two scopes, a layer that is not there, a view with no window.
        assert!(
            s.refusal("LEADER LIST DRAWING SELECTION")
                .contains("one scope")
        );
        assert!(
            s.refusal("LEADER LIST LAYERS nowhere")
                .contains("layer does not exist")
        );
        assert!(s.refusal("LEADER LIST VIEW").contains("AREA"));
        // A word after the scope that is not the verb's.
        assert!(
            s.refusal("LEADER FREEZE DRAWING WHERE LAYER=notes banana")
                .contains("banana")
        );
    }

    #[test]
    fn view_is_the_windows_plan_view_with_its_own_hidden_layers() {
        let Pits {
            mut s,
            a1,
            a2,
            a4,
            b1,
            ..
        } = Pits::new();
        s.interpreter
            .set_scope_context(Some(Box::new(|_id: Option<u32>| -> Result<ScopeView> {
                let mut view = ScopeView {
                    id: 7,
                    area: Some(Box2::new(pt(-1.0, -1.0), pt(30.0, 30.0))),
                    ..ScopeView::default()
                };
                view.layers.hide("other");
                Ok(view)
            })));
        // In the box: a1, a3 and a4 as above; the view hides "other", so not a3.
        let reply = s.run("LEADER LIST VIEW");
        assert_eq!(
            first_line(&reply),
            "scope=view view=7 area=-1,-1,30,30 matched=2",
            "{reply}"
        );
        assert_eq!(listed_ids(&reply), [a1, a4], "{reply}");
        // Without its area: everything it draws - a1, a2, a4, b1.
        let extents = s.run("LEADER LIST VIEW EXTENTS");
        assert_eq!(listed_ids(&extents), [a1, a2, a4, b1], "{extents}");
    }

    #[test]
    fn values_lists_the_block_of_each_leader_on_an_entity_and_counts_the_rest() {
        let Pits {
            mut s,
            p1,
            p2,
            a1,
            a2,
            ..
        } = Pits::new();
        let reply = s.run("LEADER VALUES LAYERS notes");
        // a1, a2 and a4 are on a pit; b1 is on nothing, so it has no values.
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=notes sublayers=yes matched=4 skipped=1",
            "{reply}"
        );
        assert_eq!(count_lines_starting(&reply, "leader="), 3, "{reply}");
        assert!(
            reply.contains(&format!("leader={a1} target={p1} type=Point")),
            "{reply}"
        );
        assert!(
            reply.contains(&format!("leader={a2} target={p2} type=Point")),
            "{reply}"
        );
        assert!(reply.contains("value=prop.invert"), "{reply}");
        // One leader by id is what it was: no scope record.
        let one = s.run(&format!("LEADER VALUES {a1}"));
        assert!(!one.contains("scope="), "{one}");
        assert!(one.contains(&format!("leader={a1}")), "{one}");
    }

    // ---- editing: SET, FREEZE, DETACH, PROP, ALIGN ------------------------------------------------

    #[test]
    fn set_changes_the_leaders_a_scope_takes_in_one_step_and_no_others() {
        let Pits {
            mut s,
            a1,
            a2,
            a3,
            a4,
            b1,
            ..
        } = Pits::new();
        let before = s.leader(a3).arrow;
        let steps = s.steps();
        let reply = s.run("LEADER SET LAYERS notes arrow=dot");
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=notes sublayers=yes matched=4",
            "{reply}"
        );
        assert!(reply.contains("updated leaders=4"), "{reply}");
        for leader in [a1, a2, a4, b1] {
            assert_eq!(s.leader(leader).arrow, ArrowHead::Dot, "{leader}");
        }
        assert_eq!(s.leader(a3).arrow, before, "a3 is on another layer");
        assert_eq!(s.steps(), steps + 1, "four leaders, one step");
        s.run("UNDO");
        assert_eq!(s.leader(a1).arrow, before);
    }

    #[test]
    fn a_where_key_after_where_is_the_condition_and_not_the_verbs_own_option() {
        let Pits {
            mut s, a1, a3, b1, ..
        } = Pits::new();
        let steps = s.steps();
        // text= before WHERE would be the new note; after it, TEXT= is the
        // filter's - which only a text entity can pass - so nothing matches and
        // no leader's note changes.
        let reply = s.run("LEADER SET DRAWING arrow=tick WHERE TEXT=zzz");
        assert!(reply.contains("matched=0"), "{reply}");
        assert!(reply.contains("updated leaders=0"), "{reply}");
        assert_eq!(s.steps(), steps);
        assert_eq!(says(&s, a1), "IL 1.50");
        // The option BEFORE WHERE is the note: every leader on notes, "NEW".
        let noted = s.run("LEADER SET DRAWING text=NEW WHERE LAYER=notes");
        assert!(noted.contains("updated leaders=4"), "{noted}");
        assert_eq!(says(&s, a1), "NEW");
        assert_eq!(says(&s, b1), "NEW");
        assert_eq!(says(&s, a3), "IL 1.50", "a3 is not on notes");
    }

    #[test]
    fn freeze_and_detach_take_a_scope_and_an_empty_one_is_said_not_refused() {
        let Pits {
            mut s, a1, a2, a3, ..
        } = Pits::new();
        let steps = s.steps();
        let frozen = s.run("LEADER FREEZE LAYERS notes");
        // The smart notes among the four: a1, a2 and a4. b1 is plain already.
        assert_eq!(
            frozen,
            "scope=layers layers=notes sublayers=yes matched=4\nfrozen leaders=3"
        );
        assert!(!s.leader(a1).fields);
        assert_eq!(s.leader(a1).text, "IL 1.50");
        assert_eq!(s.leader(a2).text, "IL 2.50");
        assert!(s.leader(a3).fields, "a3 is on another layer");
        assert_eq!(s.steps(), steps + 1);
        s.run("UNDO");
        assert!(s.leader(a1).fields);

        let detached = s.run("LEADER DETACH LAYERS notes");
        // a1, a2 and a4 let go (three), each frozen first; b1 has no tip.
        assert_eq!(
            detached,
            "scope=layers layers=notes sublayers=yes matched=4\ndetached leaders=3 frozen=3"
        );
        assert!(!s.leader(a2).tip_ref.associated());
        assert!(s.leader(a3).tip_ref.associated());

        let before = s.steps();
        let none = s.run("LEADER FREEZE DRAWING WHERE LAYER=nowhere*");
        assert!(none.contains("matched=0"), "{none}");
        assert!(none.contains("frozen leaders=0"), "{none}");
        assert_eq!(s.steps(), before, "nothing taken, no step");
    }

    #[test]
    fn an_edit_of_the_selection_with_no_leader_selected_still_tells_you_to_select_first() {
        let Pits { mut s, p1, a2, .. } = Pits::new();
        assert!(
            s.refusal("LEADER FREEZE SELECTION")
                .contains("no leader is selected")
        );
        s.run(&format!("SELECT {p1}"));
        assert!(
            s.refusal("LEADER SET SELECTION arrow=dot")
                .contains("no leader is selected")
        );
        s.run(&format!("SELECT {a2}"));
        let reply = s.run("LEADER FREEZE SELECTION");
        assert_eq!(reply, "scope=selection matched=1\nfrozen leaders=1");
    }

    #[test]
    fn prop_sets_the_attribute_on_the_entity_each_leader_is_on_once_and_counts_the_skipped() {
        let Pits {
            mut s,
            p1,
            p2,
            a2,
            a3,
            ..
        } = Pits::new();
        let steps = s.steps();
        let reply = s.run("LEADER PROP LAYERS notes SET invert 9.5 real");
        // a1 and a4 are on p1, a2 on p2: two entities. b1 is on nothing: skipped.
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=notes sublayers=yes matched=4 entities=2 skipped=1",
            "{reply}"
        );
        assert_eq!(count_lines_starting(&reply, "leader="), 3, "{reply}");
        let invert = |s: &Session, pit: EntityId| real_of(s.entity(pit).properties.get("invert"));
        assert_eq!(invert(&s, p1), 9.5);
        assert_eq!(invert(&s, p2), 9.5);
        assert_eq!(says(&s, a2), "IL 9.50");
        assert_eq!(says(&s, a3), "IL 9.50", "a3 reads p1 too");
        assert_eq!(s.steps(), steps + 1, "two entities, one step");

        s.run("LEADER PROP LAYERS notes DELETE invert");
        assert!(!s.entity(p1).properties.contains_key("invert"));
        assert!(!s.entity(p2).properties.contains_key("invert"));

        // One leader by id: the reply it always was.
        let Pits {
            s: mut t,
            p1: t_p1,
            a1: t_a1,
            ..
        } = Pits::new();
        let one = t.run(&format!("LEADER PROP {t_a1} SET invert 7 real"));
        assert!(!one.contains("scope="), "{one}");
        assert!(
            one.contains(&format!("leader={t_a1} target={t_p1} property=invert")),
            "{one}"
        );
    }

    #[test]
    fn prop_over_leaders_with_no_entity_to_set_it_on_is_refused_with_the_single_leaders_reason() {
        let Pits { mut s, b1, .. } = Pits::new();
        s.run(&format!("SELECT {b1}"));
        let single = s.refusal(&format!("LEADER PROP {b1} SET x 1 real"));
        assert!(single.contains("tip is on no entity"), "{single}");
        // The scope of that one plain leader says the same, and sets nothing.
        let steps = s.steps();
        let scoped = s.refusal("LEADER PROP SELECTION SET x 1 real");
        assert!(scoped.contains("tip is on no entity"), "{scoped}");
        assert_eq!(s.steps(), steps);
    }

    #[test]
    fn align_puts_the_notes_of_a_scope_in_a_column_and_leaves_the_others() {
        let Pits {
            mut s,
            a1,
            a2,
            a3,
            a4,
            b1,
            ..
        } = Pits::new();
        let reply = s.run("LEADER ALIGN LAYERS notes x=70");
        // Four notes hang at (5,5), (55,55), (6,6) and (110,110): each moves to
        // x = 70 and keeps its y.
        assert_eq!(
            reply,
            "scope=layers layers=notes sublayers=yes matched=4\naligned leaders=4 x=70"
        );
        assert_eq!(hang(&s, a1), pt(70.0, 5.0));
        assert_eq!(hang(&s, a2), pt(70.0, 55.0));
        assert_eq!(hang(&s, a4), pt(70.0, 6.0));
        assert_eq!(hang(&s, b1), pt(70.0, 110.0));
        assert_eq!(hang(&s, a3), pt(-5.0, -5.0), "another layer");
    }

    // ---- making: FOR ------------------------------------------------------------------------------

    #[test]
    fn for_makes_a_leader_to_each_entity_a_scope_takes_in_one_step() {
        let Levels { mut s, q1, .. } = Levels::new();
        let steps = s.steps();
        let reply = s.run("LEADER FOR LAYERS plan preset=level");
        // q1 and q2 are on plan; q3 is not.
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=plan sublayers=yes matched=2",
            "{reply}"
        );
        let ids = s.document.last_created_entities();
        assert_eq!(ids.len(), 2);
        assert!(
            reply.contains(&format!(
                "created leaders=2 ids={},{} skipped=0 preset=level",
                ids[0], ids[1]
            )),
            "{reply}"
        );
        assert_eq!(says(&s, ids[0]), "RL 25.000");
        assert_eq!(says(&s, ids[1]), "RL 30.000");
        assert_eq!(s.leader(ids[0]).tip_ref.entity, q1);
        assert_eq!(s.steps(), steps + 1);
        s.run("UNDO");
        assert_eq!(s.steps(), steps);
    }

    #[test]
    fn for_takes_a_where_filter_after_its_own_options_and_an_area() {
        let Levels { mut s, .. } = Levels::new();
        // The levels of every point in the drawing: the three. `preset` is no
        // WHERE key, so it stays the verb's after WHERE.
        let all = s.run("LEADER FOR DRAWING WHERE TYPE=point preset=level");
        assert!(first_line(&all).contains("matched=3"), "{all}");
        assert_eq!(s.document.last_created_entities().len(), 3);
        // text= before WHERE is the note, LAYER= after it the filter: "NOTE" on
        // the two points of plan - and none on other.
        let Levels { s: mut t, .. } = Levels::new();
        let noted = t.run("LEADER FOR DRAWING text=NOTE WHERE TYPE=point LAYER=plan");
        assert!(first_line(&noted).contains("matched=2"), "{noted}");
        let created = t.document.last_created_entities();
        assert_eq!(created.len(), 2);
        assert_eq!(says(&t, created[0]), "NOTE");
        // AREA -1,-1,11,1: q1 at (0,0) and q2 at (10,0) are inside, q3 at (20,0)
        // is not.
        let Levels { s: mut u, .. } = Levels::new();
        let area = u.run("LEADER FOR AREA -1,-1,11,1 preset=level");
        assert!(
            first_line(&area).contains("scope=area area=-1,-1,11,1 matched=2"),
            "{area}"
        );
    }

    #[test]
    fn for_reports_a_scope_that_takes_nothing_and_refuses_an_empty_selection() {
        let Levels { mut s, q3, .. } = Levels::new();
        let steps = s.steps();
        let none = s.run("LEADER FOR LAYERS plan WHERE TYPE=circle preset=level");
        assert!(first_line(&none).contains("matched=0"), "{none}");
        assert!(
            none.contains("created leaders=0 ids= skipped=0 preset=level"),
            "{none}"
        );
        assert_eq!(s.steps(), steps);
        assert!(
            s.refusal("LEADER FOR SELECTION preset=level")
                .contains("nothing is selected")
        );
        assert!(s.refusal("LEADER FOR VIEW preset=level").contains("AREA"));
        // The selection still works as it did.
        s.run(&format!("SELECT {q3}"));
        let one = s.run("LEADER FOR SELECTION preset=level");
        assert_eq!(first_line(&one), "scope=selection matched=1", "{one}");
        assert_eq!(says(&s, s.last()), "RL 40.000");
    }

    #[test]
    fn balloon_for_numbers_the_balloons_of_a_scope_and_renumber_takes_one_too() {
        let Levels { mut s, q3, .. } = Levels::new();
        let made_reply = s.run("BALLOON FOR LAYERS plan");
        assert!(made_reply.contains("created balloons=2"), "{made_reply}");
        let balloons = s.document.last_created_entities();
        assert_eq!(balloons.len(), 2);
        // On from the highest there is (none): 1 and 2.
        assert_eq!(s.leader(balloons[0]).text, "1");
        assert_eq!(s.leader(balloons[1]).text, "2");

        // A third on another layer, numbered 3; renumber only the layer plan's.
        s.run("LAYER SET other");
        let third = made(&mut s, &format!("BALLOON FOR {q3}"));
        assert_eq!(s.leader(third).text, "3");
        s.run("LAYER SET 0");
        let steps = s.steps();
        // The scope is the leaders' own layer: they were made on layer 0, so
        // LAYERS plan takes none and nothing is renumbered.
        let none = s.run("BALLOON RENUMBER LAYERS plan start=10");
        assert_eq!(
            none,
            "scope=layers layers=plan sublayers=yes matched=0\nballoons=0 renumbered=0"
        );
        assert_eq!(s.steps(), steps);
        // By id: the two on layer 0 become 10 and 11, the one on other keeps 3.
        let two = s.run("BALLOON RENUMBER DRAWING WHERE LAYER=0 start=10");
        assert_eq!(
            two,
            "scope=drawing where=\"LAYER=0\" matched=2\nballoons=2 renumbered=2"
        );
        assert_eq!(s.leader(balloons[0]).text, "10");
        assert_eq!(s.leader(balloons[1]).text, "11");
        assert_eq!(s.leader(third).text, "3");
        // With no scope: every numbered balloon, in the order made, from 1.
        let every = s.run("BALLOON RENUMBER");
        // 10 -> 1 and 11 -> 2 change; the third already says 3.
        assert_eq!(every, "balloons=3 renumbered=2");
        assert_eq!(s.leader(balloons[0]).text, "1");
        assert_eq!(s.leader(third).text, "3");
    }

    // ---- the names a note reads follow a rename (L3) -----------------------------------------------

    #[test]
    fn renaming_a_surface_rewrites_the_notes_that_read_it_in_the_same_undo_step() {
        let Ground { mut s, pit } = Ground::new();
        let above = made(
            &mut s,
            &format!("LEADER #{pit} 40,50 preset=surface surface=ground"),
        );
        // A name in another case is the same surface (the store finds it so).
        let level = made(
            &mut s,
            &(format!("LEADER #{pit} 41,51 ") + "template=\"{surface.GROUND:.2f} / {{x}}\""),
        );
        assert_eq!(says(&s, above), "+4.000");
        assert_eq!(says(&s, level), "21.00 / {x}");
        let steps = s.steps();

        let reply = s.run("SURFACE RENAME ground eg");
        assert!(
            reply.contains("renamed surface=ground name=eg corridors=0 roadobjects=0 leaders=2"),
            "{reply}"
        );
        assert_eq!(s.leader(above).text, "{dsurface.eg:+.3f}");
        assert_eq!(
            s.leader(level).text,
            "{surface.eg:.2f} / {{x}}",
            "the steps, the literal text and the doubled braces are kept"
        );
        assert_eq!(
            says(&s, above),
            "+4.000",
            "the note reads the renamed surface"
        );
        assert_eq!(says(&s, level), "21.00 / {x}");
        assert_eq!(
            s.steps(),
            steps + 1,
            "the surface and its readers, one step"
        );

        s.run("UNDO");
        assert_eq!(s.leader(above).text, "{dsurface.ground:+.3f}");
        assert!(s.document.surfaces().find("ground").is_some());
        assert_eq!(says(&s, above), "+4.000");
    }

    #[test]
    fn a_name_a_note_cannot_write_leaves_the_readers_and_says_so_in_a_warning() {
        let Ground { mut s, pit } = Ground::new();
        let above = made(
            &mut s,
            &format!("LEADER #{pit} 40,50 preset=surface surface=ground"),
        );
        // ':' starts a step in a field, so "a:b" cannot be a surface a note names.
        let reply = s.run("SURFACE RENAME ground a:b");
        assert!(reply.contains("leaders=0"), "{reply}");
        assert!(
            reply.contains("warning leaders=1 still read surface ground"),
            "{reply}"
        );
        assert_eq!(
            s.leader(above).text,
            "{dsurface.ground:+.3f}",
            "left, not half-rewritten"
        );
        assert!(
            s.document.surfaces().find("a:b").is_some(),
            "the rename itself was done"
        );
    }

    #[test]
    fn a_leader_on_a_locked_layer_is_left_reading_the_old_name_and_the_rename_still_runs() {
        let Ground { mut s, pit } = Ground::new();
        s.run("LAYER SET notes");
        let locked = made(
            &mut s,
            &format!("LEADER #{pit} 40,50 preset=surface surface=ground"),
        );
        s.run("LAYER SET 0");
        let free = made(
            &mut s,
            &format!("LEADER #{pit} 41,51 preset=surface surface=ground"),
        );
        s.run("LAYER LOCK notes");
        let reply = s.run("SURFACE RENAME ground eg");
        assert!(reply.contains("leaders=1"), "{reply}");
        assert!(
            reply.contains("warning leaders=1 still read surface ground"),
            "{reply}"
        );
        assert_eq!(s.leader(locked).text, "{dsurface.ground:+.3f}");
        assert_eq!(s.leader(free).text, "{dsurface.eg:+.3f}");
    }

    #[test]
    fn an_alignments_name_is_exact_and_counted_when_it_is_deleted() {
        let mut s = Session::default();
        s.run("ALIGN NEW MC01 0,0 100,0");
        let pit = point(&mut s, pt(30.0, 5.0), "0", PropertyMap::new());
        s.run(&format!(
            "LEADER #{pit} 40,10 preset=station alignment=MC01"
        ));
        s.run(&(format!("LEADER #{pit} 41,11 ") + "template=\"{offset.MC01:+.3f}\""));
        // A note that names neither is not counted.
        s.run(&(format!("LEADER #{pit} 42,12 ") + "template=\"{id}\""));
        assert_eq!(
            leaders_reading(s.document.model(), ReadKind::Alignment, "MC01"),
            2
        );
        assert_eq!(
            leaders_reading(s.document.model(), ReadKind::Alignment, "mc01"),
            0,
            "an alignment is found exactly"
        );
        assert_eq!(
            leaders_reading(s.document.model(), ReadKind::Surface, "MC01"),
            0
        );

        let reply = s.run("ALIGN DELETE MC01");
        assert!(reply.contains("alignment MC01 deleted"), "{reply}");
        assert!(
            reply.contains("warning leaders=2 read alignment MC01 and will read nothing now"),
            "{reply}"
        );
        // None read it any more: no warning for an alignment nobody reads.
        s.run("ALIGN NEW MC02 0,0 100,0");
        assert!(!s.run("ALIGN DELETE MC02").contains("warning"));
    }

    #[test]
    fn rename_leader_reads_follows_an_alignment_by_the_function_the_rename_will_use() {
        let mut s = Session::default();
        s.run("ALIGN NEW MC01 0,0 100,0");
        let pit = point(&mut s, pt(30.0, 5.0), "0", PropertyMap::new());
        let station = made(
            &mut s,
            &format!("LEADER #{pit} 40,10 preset=station alignment=MC01"),
        );
        assert_eq!(
            s.leader(station).text,
            "CH {station.MC01:ch}\nOS {offset.MC01:+.3f}"
        );
        let renamed =
            rename_leader_reads(s.document.model(), ReadKind::Alignment, "MC01", "Main Rd");
        assert_eq!(renamed.followed, 1);
        assert_eq!(renamed.unfollowed, 0);
        let Some(command) = renamed.command else {
            panic!("no command follows MC01");
        };
        assert!(s.document.execute(command).is_ok());
        // Both fields, with a blank in the name as a note writes one; the line
        // break and the steps are kept.
        assert_eq!(
            s.leader(station).text,
            "CH {station.Main Rd:ch}\nOS {offset.Main Rd:+.3f}"
        );
        // A rename to its own spelling changes nothing and makes no command.
        assert!(
            rename_leader_reads(
                s.document.model(),
                ReadKind::Alignment,
                "Main Rd",
                "Main Rd"
            )
            .command
            .is_none()
        );
        // Nothing reads it, so nothing is made.
        assert!(
            rename_leader_reads(s.document.model(), ReadKind::Alignment, "none", "x")
                .command
                .is_none()
        );
    }

    #[test]
    fn template_fields_are_renamed_and_everything_else_of_the_template_is_kept() {
        let rename = |name: &str| -> Option<String> {
            if name == "dsurface.Ground" {
                return Some(String::from("dsurface.eg"));
            }
            None
        };
        // The braces doubled to be literal, the line break, the other fields
        // and the steps stay as they were.
        assert_eq!(
            rename_template_fields(
                "{{x}} {dsurface.Ground:+.3f}\nCUT {cut.ground:.3f} {{",
                &rename
            ),
            "{{x}} {dsurface.eg:+.3f}\nCUT {cut.ground:.3f} {{"
        );
        // A field with no step.
        assert_eq!(
            rename_template_fields("{dsurface.Ground}", &rename),
            "{dsurface.eg}"
        );
        // Braces that do not balance name nothing: the template comes back as
        // it was.
        assert_eq!(
            rename_template_fields("{dsurface.Ground", &rename),
            "{dsurface.Ground"
        );
        assert_eq!(
            rename_template_fields("dsurface.Ground}", &rename),
            "dsurface.Ground}"
        );
        assert_eq!(rename_template_fields("", &rename), "");
    }

    // ---- the JSON's guard against a byte that is not UTF-8 (L4) -----------------------------------
    //
    // AByteThatIsNotUtf8InAnyNameIsWrittenAsTheReplacementCharacterNotThrown
    // is not twinned: see the file's comment.

    #[test]
    fn a_leaders_report_round_trips_through_the_documents_json_and_an_empty_one_is_an_empty_list() {
        let Pits { s, p1, .. } = Pits::new();
        let json = leaders_json(&s.document, &[], None);
        assert!(
            json.contains("\"template\": \"IL {prop.invert:.2f}\""),
            "{json}"
        );
        assert!(!json.contains("\"scope\""));
        // `only` empty is every leader, as it always was; an explicit empty
        // list in the report is none.
        assert!(leaders_json(&s.document, &[], None).contains("\"id\": "));
        let nothing: &[EntityId] = &[];
        let none = leaders_report(&s.document, Some(nothing), None);
        assert!(none.leaders.is_empty());
        assert_eq!(leaders_report(&s.document, None, None).leaders.len(), 5);
        let on_p1 = leaders_report(&s.document, None, Some(p1));
        assert_eq!(on_p1.leaders.len(), 3, "a1, a3 and a4 are on p1");
    }

    // ---- the words the Leaders manager hands to the verbs ----------------------------------------------

    #[test]
    fn an_anchor_word_reads_back_as_the_same_place_the_dialog_showed() {
        let mut s = Session::default();
        let pit = point(&mut s, pt(0.0, 0.0), "0", PropertyMap::new());
        let line = made(&mut s, "LINE 0,0 40,0");
        let circle = made(&mut s, "CIRCLE 100,0 5");
        let open = made(&mut s, "PLINE 0,50 10,50 10,60 20,60");
        let lot = made(&mut s, "PLINE 0,100 10,100 10,110 0,110 C");
        let leader = made(&mut s, "LEADER 0,0 5,5 text=A");
        // Every kind of place the dialog or a tool can name, on an entity that
        // has it: the verb must read the word back as exactly this reference,
        // the fraction included.
        let places = [
            anchor(pit, AnchorPoint::Position, 0, 0.0),
            anchor(line, AnchorPoint::Start, 0, 0.0),
            anchor(line, AnchorPoint::End, 0, 0.0),
            anchor(line, AnchorPoint::Mid, 0, 0.0),
            anchor(line, AnchorPoint::Along, 0, 0.123456),
            anchor(circle, AnchorPoint::Centre, 0, 0.0),
            anchor(circle, AnchorPoint::Along, 0, 0.5),
            anchor(open, AnchorPoint::Vertex, 2, 0.0),
            anchor(open, AnchorPoint::SegmentMid, 1, 0.0),
            anchor(open, AnchorPoint::Along, 1, 0.25),
            anchor(lot, AnchorPoint::Inside, 0, 0.0),
        ];
        for place in places {
            let word = anchor_word(&place);
            s.run(&format!("LEADER SET {leader} tip={word}"));
            assert_eq!(s.leader(leader).tip_ref, place, "{word}");
        }
        assert_eq!(
            anchor_word(&anchor(12, AnchorPoint::Along, 3, 0.25)),
            "#12.along3:0.25"
        );
        assert_eq!(
            anchor_word(&anchor(12, AnchorPoint::Vertex, 4, 0.0)),
            "#12.v4"
        );
    }
}
