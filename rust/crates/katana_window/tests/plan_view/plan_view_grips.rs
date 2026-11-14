//! The twin of tests/qt_widgets/drawing/test_plan_view_grips.cpp: grips in the plan view
//! (drawing/grip_controller), driven with the view's mouse and key events. The view draws the
//! selection's grips, a press on one and a drag moves it, every drag is one undo step, several hot
//! grips move together, Delete removes hot vertices and Esc drops a grip. What each grip does to
//! the geometry is katana_cad's and tested there; these test the gesture.
//!
//! The fixture is `Plan::over_polyline`: a 400 x 300 view at 10 px a unit centred on (10, 5), so
//! model (0, 0) is at pixel (100, 200) and a unit is ten pixels.

use katana_cad::drawing::vertex_editing::{edit_polyline, height_text};
use katana_commands::{EntityAttributes, create_entities, create_point};
use katana_entity::{CalloutShape, Entity, Geometry, LeaderGeometry};
use katana_geometry::polyline_vertices::{
    delete_vertex, insert_vertex, move_vertex, segment_to_arc, set_vertex_height,
};
use katana_geometry::{CurvePolyline2, Point2};
use katana_window::canvas::{Color, Image};
use katana_window::drawing::feedback_painter::overlay;
use katana_window::viewport_widget::{KeyModifiers, MouseButton, ViewKey};

use crate::support::Plan;

const NONE: KeyModifiers = KeyModifiers::NONE;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const CTRL: KeyModifiers = KeyModifiers::CTRL;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// One undoable edit of the fixture's polyline, as another tool, the panel or an agent makes it.
fn edit(
    f: &mut Plan,
    name: &str,
    change: impl Fn(&CurvePolyline2) -> katana_geometry::PolylineResult + Send + 'static,
) {
    f.document
        .execute(edit_polyline(f.polyline, name, change))
        .expect("the polyline is edited");
}

/// Whether the view claims Delete from the window's Erase (a ShortcutOverride offered and
/// accepted).
fn claimed(f: &mut Plan) -> bool {
    f.claims(ViewKey::Delete, NONE)
}

fn leader_of(f: &Plan, id: u64) -> LeaderGeometry {
    match &f.entity(id).expect("the leader is there").geometry {
        Geometry::Leader(leader) => leader.clone(),
        other => panic!("not a leader: {other:?}"),
    }
}

mod plan_view_grips {
    use super::*;

    #[test]
    fn the_selection_shows_its_grips_and_nothing_else_does() {
        let mut f = Plan::over_polyline();
        f.view
            .grip_controller_mut()
            .refresh(&f.document, Some(&f.state.layers), 0);
        assert!(
            f.view.grip_controller().grips().is_empty(),
            "nothing selected, no grips"
        );
        f.select();
        // Three vertices and two segment middles.
        assert_eq!(f.view.grip_controller().grips().len(), 5);
    }

    #[test]
    fn dragging_a_vertex_grip_moves_it_as_one_undo_step() {
        let mut f = Plan::over_polyline();
        f.select();
        let before = f.undo_steps();
        f.drag(p(10.0, 0.0), p(10.0, 6.0), NONE);
        assert_eq!(f.vertices()[1], p(10.0, 6.0));
        assert_eq!(f.undo_steps(), before + 1);
        assert_eq!(f.document.history().undo_name(), "GRIP_EDIT");
        f.document.undo().expect("the drag is undone");
        assert_eq!(f.vertices()[1], p(10.0, 0.0));
    }

    #[test]
    fn a_leader_note_and_bend_can_be_dragged_without_moving_its_tip_or_losing_its_frame() {
        let mut f = Plan::over_polyline();
        let entity = Entity {
            geometry: Geometry::Leader(LeaderGeometry {
                vertices: vec![p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)],
                text: String::from("Note"),
                callout: CalloutShape::Box,
                ..LeaderGeometry::default()
            }),
            ..Entity::default()
        };
        f.document
            .execute(create_entities(vec![entity]))
            .expect("the leader is made");
        let id = f.document.last_created_entities()[0];
        f.document.selection_mut().set(vec![id]);
        f.document.notify_selection_changed();
        f.paint();
        let before = f.undo_steps();
        f.drag(p(20.0, 0.0), p(20.0, 6.0), NONE);
        let moved = leader_of(&f, id);
        assert_eq!(moved.vertices.last(), Some(&p(20.0, 6.0)));
        assert_eq!(moved.vertices.first(), Some(&p(0.0, 0.0)));
        assert_eq!(moved.callout, CalloutShape::Box);
        assert_eq!(f.undo_steps(), before + 1);
        f.document.undo().expect("the drag is undone");
        f.paint();
        f.drag(p(10.0, 0.0), p(10.0, 4.0), NONE);
        assert_eq!(leader_of(&f, id).vertices[1], p(10.0, 4.0));
        f.paint();
        f.click_at(p(10.0, 4.0), SHIFT);
        f.plain_key(ViewKey::Delete);
        assert!(
            f.entity(id).is_some(),
            "Delete on a leader point must not fall through to erasing the entire leader"
        );
    }

    #[test]
    fn a_click_picks_a_grip_up_and_the_next_click_puts_it_down() {
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(20.0, 0.0), NONE);
        assert!(f.view.grip_controller().active(), "picked up");
        f.hover(p(22.0, 3.0));
        f.click_at(p(22.0, 3.0), NONE);
        assert_eq!(f.vertices()[2], p(22.0, 3.0));
        assert!(!f.view.grip_controller().active());
    }

    #[test]
    fn a_grip_picked_up_takes_a_typed_point() {
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(0.0, 0.0), NONE);
        f.type_keys("@3,4");
        f.plain_key(ViewKey::Enter);
        assert_eq!(f.vertices()[0], p(3.0, 4.0));
    }

    #[test]
    fn shift_clicked_grips_move_together() {
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(10.0, 0.0), SHIFT);
        f.click_at(p(20.0, 0.0), SHIFT);
        assert_eq!(f.view.grip_controller().hot().len(), 2);
        // Grabbing one of the hot grips moves both.
        f.drag(p(20.0, 0.0), p(20.0, 5.0), NONE);
        assert_eq!(f.vertices()[1], p(10.0, 5.0));
        assert_eq!(f.vertices()[2], p(20.0, 5.0));
        assert_eq!(f.vertices()[0], p(0.0, 0.0));
    }

    #[test]
    fn ctrl_dragging_a_segment_middle_inserts_a_vertex() {
        let mut f = Plan::over_polyline();
        f.select();
        f.drag(p(5.0, 0.0), p(5.0, -3.0), CTRL);
        assert_eq!(f.vertices().len(), 4);
        assert_eq!(f.vertices()[1], p(5.0, -3.0));
    }

    #[test]
    fn delete_removes_the_hot_vertex_and_esc_drops_a_grip() {
        let mut f = Plan::over_polyline();
        f.select();
        // A click makes the middle vertex hot (and picks it up); Delete removes it.
        f.click_at(p(10.0, 0.0), NONE);
        f.plain_key(ViewKey::Delete);
        assert_eq!(f.vertices().len(), 2);
        assert_eq!(
            f.document.model().entities.size(),
            1,
            "the vertex, not the polyline"
        );

        // Esc drops a picked-up grip without changing anything.
        let before = f.undo_steps();
        f.click_at(p(20.0, 0.0), NONE);
        f.hover(p(25.0, 5.0));
        f.plain_key(ViewKey::Escape);
        assert!(!f.view.grip_controller().active());
        assert_eq!(f.undo_steps(), before);
        assert_eq!(f.vertices()[1], p(20.0, 0.0));
    }

    #[test]
    fn a_grip_drag_snaps_to_an_endpoint() {
        let mut f = Plan::over_polyline();
        f.document
            .execute(create_point(p(15.2, 7.1), EntityAttributes::default()))
            .expect("the point is made");
        f.set_snap_enabled(true);
        f.select();
        // Dropped half a unit (5 px) from the point: the endpoint snap takes it.
        f.drag(p(10.0, 0.0), p(15.6, 7.4), NONE);
        assert_eq!(f.vertices()[1], p(15.2, 7.1));
    }

    #[test]
    fn ortho_holds_a_grip_drag_square_to_its_base() {
        let mut f = Plan::over_polyline();
        f.document.drafting_mut().ortho = true;
        f.select();
        f.drag(p(10.0, 0.0), p(11.0, 8.0), NONE);
        assert_eq!(f.vertices()[1], p(10.0, 8.0));
    }

    #[test]
    fn a_grip_picked_up_before_a_tool_is_dropped_not_committed_later() {
        // Click a vertex, then choose a tool - the owner's way in. The grip the click picked up
        // used to stay attached under the tool, and the first click after the tool ended put it
        // down: a GRIP_EDIT nobody asked for.
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(20.0, 0.0), NONE);
        assert!(f.view.grip_controller().active(), "picked up");
        let before = f.undo_steps();
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        assert!(
            !f.view.grip_controller().active(),
            "the tool took the grips"
        );
        f.plain_key(ViewKey::Escape);
        assert!(!f.view.tool_active());
        f.hover(p(25.0, 5.0));
        f.click_at(p(25.0, 5.0), NONE);
        assert_eq!(f.undo_steps(), before);
        assert_eq!(f.vertices()[2], p(20.0, 0.0));
    }

    #[test]
    fn delete_with_a_hot_vertex_is_the_views_key_not_the_windows_erase() {
        // The window's Erase holds Delete as a shortcut. The key is offered to the view first
        // (a shortcut override, ignored until someone claims it); a view that let it go had the
        // whole polyline erased instead of the vertex.
        let mut f = Plan::over_polyline();
        f.select();
        assert!(!claimed(&mut f), "nothing hot: Delete is the window's Erase");
        f.click_at(p(10.0, 0.0), SHIFT);
        assert_eq!(f.view.grip_controller().hot().len(), 1);
        assert!(claimed(&mut f), "a hot vertex: Delete is the view's");
        f.plain_key(ViewKey::Delete);
        assert_eq!(f.vertices(), [p(0.0, 0.0), p(20.0, 0.0)]);
        assert!(f.entity(f.polyline).is_some(), "the polyline stays");
    }

    #[test]
    fn inside_a_vertex_tool_delete_never_erases_the_polyline_it_is_editing() {
        // A vertex tool works on a SELECTED polyline - a vertex's grips are there only while it
        // is, and the tool selects what it has just edited - and the window's Erase holds
        // Delete, so Delete inside the tool erased the whole string: "Press Enter to delete
        // vertex 1", Delete pressed instead, and the polyline was gone. The view claims Delete
        // while a vertex tool runs: Delete Vertex takes it as its Enter, and the others do
        // nothing with it and say so.
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(10.0, 0.0), SHIFT);
        assert_eq!(f.view.grip_controller().hot().len(), 1);
        f.start_tool("draw.vertex.delete")
            .expect("the tool starts");
        assert!(claimed(&mut f), "Delete Vertex's, not the window's Erase");
        f.plain_key(ViewKey::Delete);
        assert_eq!(
            f.vertices(),
            [p(0.0, 0.0), p(20.0, 0.0)],
            "the chosen vertex, as Enter deletes it"
        );
        assert!(f.entity(f.polyline).is_some(), "the polyline stays");
        assert!(f.view.tool_active(), "and the tool runs on");
        assert!(claimed(&mut f), "with nothing chosen now, still the tool's");
        let undos = f.undo_steps();
        f.plain_key(ViewKey::Delete);
        assert_eq!(f.undo_steps(), undos, "nothing chosen, nothing deleted");
        f.plain_key(ViewKey::Escape);

        // Insert Vertex on the selected polyline: claimed, and refused.
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        assert!(claimed(&mut f));
        f.plain_key(ViewKey::Delete);
        assert!(f.entity(f.polyline).is_some());
        assert_eq!(f.undo_steps(), undos);
        assert!(f.view.tool_active());
        f.plain_key(ViewKey::Escape);
        assert!(!f.view.tool_active());
        // No tool and nothing hot: Delete is the window's Erase again.
        assert!(!claimed(&mut f));
    }

    #[test]
    fn a_hot_vertex_whose_vertex_went_in_two_edits_between_paints_goes_too() {
        // Chosen, then two edits before the view paints again - a SCRIPT, lines pasted, an agent:
        // the chosen vertex deleted, and another inserted ahead of it, leaving as many vertices
        // as before. Its index then named (10,0), a vertex nobody chose, and Delete removed it.
        // Its point is gone and the vertices before it have moved, so it goes too.
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(20.0, 0.0), SHIFT);
        assert_eq!(f.view.grip_controller().hot().len(), 1);
        edit(&mut f, "VERTEX_DELETE", |shape| delete_vertex(shape, 2));
        edit(&mut f, "VERTEX_INSERT", |shape| {
            insert_vertex(shape, 0, p(5.0, 0.0), None)
        });
        f.paint();
        assert!(f.view.grip_controller().hot().is_empty());
        // Delete Vertex chosen next, as the review ran it: nothing is offered to Enter, and
        // Enter deletes nothing. (With no vertex hot, the Delete key itself is Erase's, of the
        // whole selection, as it always was.)
        f.start_tool("draw.vertex.delete")
            .expect("the tool starts");
        f.plain_key(ViewKey::Enter);
        assert_eq!(
            f.vertices(),
            [p(0.0, 0.0), p(5.0, 0.0), p(10.0, 0.0)],
            "nothing nobody chose is deleted"
        );
    }

    #[test]
    fn a_double_click_on_a_grip_leaves_nothing_picked_up() {
        let mut f = Plan::over_polyline();
        f.select();
        f.log.clear();
        let before = f.undo_steps();
        // A double click: press, release, DOUBLE CLICK, release. The first click picks the grip
        // up.
        let at = f.pixel(p(10.0, 0.0));
        f.click_at(p(10.0, 0.0), NONE);
        f.mouse_double_click(at, MouseButton::Left);
        f.mouse_release(at, MouseButton::Left, NONE);
        assert_eq!(
            f.double_clicked().len(),
            1,
            "the double click still opens the polyline's editor"
        );
        assert!(
            !f.view.grip_controller().active(),
            "and leaves no grip following the cursor"
        );
        f.hover(p(14.0, 6.0));
        f.click_at(p(14.0, 6.0), NONE);
        assert_eq!(f.undo_steps(), before, "no GRIP_EDIT");
        assert_eq!(f.vertices()[1], p(10.0, 0.0));
    }

    #[test]
    fn a_hot_vertex_follows_its_vertex_through_an_insert_before_it() {
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(20.0, 0.0), SHIFT);
        assert_eq!(f.view.grip_controller().hot().len(), 1);
        // Another edit - a tool, the panel, an agent - puts a vertex before it: (20,0) is vertex
        // 3 now, and vertex 2 is (10,0).
        edit(&mut f, "VERTEX_INSERT", |shape| {
            insert_vertex(shape, 0, p(5.0, 0.0), None)
        });
        f.paint();
        f.plain_key(ViewKey::Delete);
        assert_eq!(
            f.vertices(),
            [p(0.0, 0.0), p(5.0, 0.0), p(10.0, 0.0)],
            "the vertex made hot goes, not the one now at its old index"
        );
    }

    #[test]
    fn a_hovered_grip_says_what_it_is_and_what_can_be_done() {
        let mut f = Plan::over_polyline();
        f.select();
        let id = f.polyline;
        f.hover(p(10.0, 0.1));
        assert_eq!(
            f.view.grip_controller().hover_hint(&f.document),
            format!(
                "Vertex 1 of polyline {id}: drag to move · click to pick up · Shift+click to \
                 choose · Delete removes the chosen · right-click for vertex tools"
            )
        );
        f.hover(p(5.0, 0.1));
        assert_eq!(
            f.view.grip_controller().hover_hint(&f.document),
            format!(
                "Segment 0 of polyline {id}: drag to stretch · Ctrl+drag to add a vertex · \
                 right-click for segment tools"
            )
        );
        f.hover(p(12.0, 6.0));
        assert!(
            f.view.grip_controller().hover_hint(&f.document).is_empty(),
            "no grip under the cursor, nothing to say"
        );
        // A grip picked up says what to do with it instead (its prompt).
        f.click_at(p(20.0, 0.0), NONE);
        assert!(f.view.grip_controller().hover_hint(&f.document).is_empty());
        assert!(!f.view.grip_controller().prompt().is_empty());
    }

    #[test]
    fn a_vertex_with_a_height_says_its_height() {
        let mut f = Plan::over_polyline();
        // Heights 100, none, 101.25: vertex 2's hint gives its own.
        edit(&mut f, "VERTEX_Z", |shape| {
            set_vertex_height(shape, 2, Some(101.25))
        });
        f.select();
        let id = f.polyline;
        f.hover(p(20.0, 0.1));
        let hint = f.view.grip_controller().hover_hint(&f.document);
        assert!(
            hint.starts_with(&format!("Vertex 2 of polyline {id}, z 101.250: ")),
            "{hint}"
        );
        // A height a hair under zero reads as the vertex tools' labels read it (cad's
        // height_text): "z 0.000". The band wrote it with its own formatting, "z -0.000", beside
        // a tool's "z 0.000" for the same vertex.
        edit(&mut f, "VERTEX_Z", |shape| {
            set_vertex_height(shape, 1, Some(-0.0004))
        });
        f.paint();
        f.hover(p(10.0, 0.1));
        let hint = f.view.grip_controller().hover_hint(&f.document);
        assert!(
            hint.starts_with(&format!("Vertex 1 of polyline {id}, z 0.000: ")),
            "{hint}"
        );
        assert_eq!(height_text(Some(-0.0004)), "z 0.000");
    }

    #[test]
    fn ctrl_over_a_segment_middle_shows_the_vertex_a_drag_would_add() {
        let mut f = Plan::over_polyline();
        f.select();
        // Without Ctrl the middle is the hovered grip's green diamond; with it, the new vertex's
        // cyan disc: more cyan round the middle's pixel.
        let middle = f.pixel(p(5.0, 0.0));
        let cyan = overlay::preview();
        let near = |pixel: Color| {
            (i32::from(pixel.r) - i32::from(cyan.r)).abs() <= 24
                && (i32::from(pixel.g) - i32::from(cyan.g)).abs() <= 24
                && (i32::from(pixel.b) - i32::from(cyan.b)).abs() <= 24
        };
        let cyan_near = |image: &Image| {
            let mut count = 0;
            for y in middle.y as i32 - 6..=middle.y as i32 + 6 {
                for x in middle.x as i32 - 6..=middle.x as i32 + 6 {
                    count += i32::from(near(image.pixel(x as u32, y as u32)));
                }
            }
            count
        };
        f.hover(p(5.0, 0.1));
        let plain = f.grab();
        f.hover_with(p(5.0, 0.1), CTRL);
        assert!(f.view.grip_controller().ctrl());
        let held = f.grab();
        assert!(cyan_near(&held) > cyan_near(&plain));
        // Let go of Ctrl: the diamond again.
        f.view.key_release(ViewKey::Control);
        assert!(!f.view.grip_controller().ctrl());
    }

    #[test]
    fn a_hot_vertex_stays_chosen_when_it_is_itself_moved() {
        // Chosen, then moved by another edit (VERTEX MOVE, a Vertices panel cell, another view):
        // matched again by its position alone, the chosen vertex was silently unchosen. As many
        // vertices as before means none was inserted or deleted ahead of it, so its index still
        // names it.
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(20.0, 0.0), SHIFT);
        assert_eq!(f.view.grip_controller().hot().len(), 1);
        edit(&mut f, "VERTEX_MOVE", |shape| {
            move_vertex(shape, 2, p(22.0, 3.0))
        });
        f.paint();
        assert_eq!(f.view.grip_controller().hot().len(), 1, "still chosen");
        assert_eq!(f.view.grip_controller().hot()[0].index, 2);
        assert_eq!(f.view.grip_controller().hot()[0].position, p(22.0, 3.0));
        f.plain_key(ViewKey::Delete);
        assert_eq!(f.vertices(), [p(0.0, 0.0), p(10.0, 0.0)]);
    }

    #[test]
    fn a_word_typed_after_a_click_on_a_grip_goes_to_the_command_line_and_the_grip_stays_chosen() {
        // A plain click on a vertex is how it is chosen, and it also picks the grip up to take a
        // typed point. The tool's name typed next - "click the vertex, then type INSERTVERTEX" -
        // went into the grip's point and was refused there. Nothing a grip takes starts with a
        // letter: the grip is put down, still chosen, and the word goes to the command line.
        let mut f = Plan::over_polyline();
        f.view.hooks.text_typed = true;
        f.select();
        f.click_at(p(10.0, 0.0), NONE);
        assert!(f.view.grip_controller().active(), "picked up");
        let _ = f.key_press(ViewKey::Other, NONE, "I");
        let _ = f.key_press(ViewKey::Other, NONE, "N");
        assert_eq!(f.typed_out(), ["I", "N"]);
        assert!(!f.view.grip_controller().active(), "put down");
        assert_eq!(
            f.view.grip_controller().hot().len(),
            1,
            "and still chosen"
        );
        assert_eq!(f.view.grip_controller().hot()[0].index, 1);
        assert_eq!(f.vertices()[1], p(10.0, 0.0), "nothing moved");
        // A point typed after the click still goes to the grip.
        f.click_at(p(0.0, 0.0), NONE);
        f.type_keys("@3,4");
        f.plain_key(ViewKey::Enter);
        assert_eq!(f.vertices()[0], p(3.0, 4.0));
    }

    #[test]
    fn an_arcs_middle_says_a_drag_bends_it() {
        // An arc segment's middle keeps both ends and bends the arc; its hint said "drag to
        // stretch", a straight segment's gesture.
        let mut f = Plan::over_polyline();
        edit(&mut f, "SEGMENT_ARC", |shape| {
            segment_to_arc(shape, 0, p(5.0, 3.0))
        });
        f.select();
        let id = f.polyline;
        // The arc through (5,3) from (0,0) to (10,0) has its middle there.
        f.hover(p(5.0, 3.1));
        assert_eq!(
            f.view.grip_controller().hover_hint(&f.document),
            format!(
                "Segment 0 of polyline {id}: drag to bend the arc · Ctrl+drag to add a vertex · \
                 right-click for segment tools"
            )
        );
        f.hover(p(15.0, 0.1));
        let hint = f.view.grip_controller().hover_hint(&f.document);
        assert!(hint.contains("drag to stretch"), "{hint}");
    }

    #[test]
    fn a_ctrl_press_on_a_segment_middle_says_it_adds_a_vertex() {
        // A Ctrl-press on a middle adds a vertex where it is put down; the band said what a
        // stretch says, word for word.
        let mut f = Plan::over_polyline();
        f.select();
        f.click_at(p(15.0, 0.0), CTRL);
        assert!(f.view.grip_controller().active());
        let prompt = f.view.grip_controller().prompt();
        assert!(
            prompt.starts_with("Grip segment middle, adding a vertex: "),
            "{prompt}"
        );
        f.plain_key(ViewKey::Escape);
        f.click_at(p(15.0, 0.0), NONE);
        assert!(f.view.grip_controller().active());
        let prompt = f.view.grip_controller().prompt();
        assert!(!prompt.contains("adding a vertex"), "{prompt}");
    }
}
