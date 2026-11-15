//! The twin of tests/qt_widgets/tools/test_vertex_tool_hover.cpp: the Draw > Vertices tools in a
//! real plan view, driven with the view's mouse and key events - what the view draws while the
//! cursor hovers before a click (the owner's "no visual clue where the vertex is going"), that
//! drawing it never repaints the drawing, and that a vertex clicked BEFORE the tool is the one the
//! tool works beside ("what about the already selected vertex").
//!
//! The fixture is `Plan::over_polyline`: 400 x 300 px at 10 px a unit centred on (10, 5), so model
//! (0, 0) is pixel (100, 200); snapping is off, so the cursor is where the mouse is. Fonts differ
//! by platform, so ink is compared with a baseline, never counted absolutely.

use katana_cad::drawing::vertex_editing::read_polyline;
use katana_cad::tool_feedback::FeedbackRole;
use katana_core::text::read_reply_record;
use katana_entity::entity::to_string;
use katana_geometry::Point2;
use katana_window::canvas::PointF;
use katana_window::drawing::feedback_painter::overlay;
use katana_window::viewport_widget::{KeyModifiers, ViewKey};

use crate::support::{Plan, ink_near};

const NONE: KeyModifiers = KeyModifiers::NONE;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

/// The fixture's polyline's vertices, whichever polyline kind it is stored as.
fn positions(f: &Plan) -> Vec<Point2> {
    read_polyline(f.entity(f.polyline).expect("the polyline is there"))
        .expect("it is a polyline")
        .positions()
}

/// The prompt the view last sent out.
fn prompt(f: &Plan) -> String {
    f.prompts().last().cloned().unwrap_or_default()
}

mod vertex_tool_hover {
    use super::*;

    #[test]
    fn hovering_insert_vertex_draws_its_marks_and_never_repaints_the_drawing() {
        let mut f = Plan::over_polyline();
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        f.hover(p(15.0, 0.3));
        f.paint();
        let counts = f.view.last_preview_counts().clone();
        assert_eq!(counts.target, 1, "the segment that splits");
        assert_eq!(counts.added, 1, "the vertex, where it will go");
        assert_eq!(
            counts.focus, 3,
            "the polyline's vertices, while the grips are hidden"
        );
        assert!(!counts.refused);
        assert_eq!(counts.caption, "new vertex between 1 and 2 · 5.000 from 1");
        // The marks are the view's furniture: moving over the drawing never paints the drawing
        // again.
        let drawn = f.view.drawing_paint_count();
        for x in [12.0, 14.0, 16.0] {
            f.hover(p(x, 0.2));
            f.paint();
        }
        assert_eq!(f.view.drawing_paint_count(), drawn);
        assert_eq!(f.view.last_preview_counts().added, 1);
        assert_eq!(f.undo_steps(), 1, "hovering changes nothing");
    }

    #[test]
    fn the_new_vertex_is_inked_where_it_will_go() {
        let mut f = Plan::over_polyline();
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        // Out of reach: 5 units, 50 px, from the polyline.
        f.hover(p(15.0, 5.0));
        let away = f.grab();
        assert_eq!(f.view.last_preview_counts().added, 0);
        f.hover(p(15.0, 0.3));
        let hovered = f.grab();
        assert_eq!(f.view.last_preview_counts().added, 1);
        // On the line at (15, 0), not under the cursor 3 px above it.
        let place = f.pixel(p(15.0, 0.0));
        let cyan = overlay::preview();
        assert!(ink_near(&hovered, place, 6.0, cyan) > ink_near(&away, place, 6.0, cyan));
        let green = overlay::target();
        let on_segment = f.pixel(p(12.0, 0.0));
        assert!(
            ink_near(&hovered, on_segment, 4.0, green) > ink_near(&away, on_segment, 4.0, green),
            "the segment that splits, in the target green"
        );
    }

    #[test]
    fn a_vertex_clicked_before_the_tool_is_the_one_insert_works_beside() {
        let mut f = Plan::over_polyline();
        f.select();
        // A plain click on the middle vertex's grip: it is hot, and picked up.
        f.click_at(p(10.0, 0.0), NONE);
        assert_eq!(f.view.grip_controller().hot().len(), 1);
        let before = f.undo_steps();
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        let said = prompt(&f);
        assert!(said.contains("beside vertex 1 of polyline"), "{said}");
        assert!(
            said.starts_with("Insert Vertex: "),
            "the tool, not its family: {said}"
        );
        // Enter: the middle of the segment after it, (10,0)-(20,0).
        f.plain_key(ViewKey::Enter);
        assert_eq!(
            positions(&f),
            [p(0.0, 0.0), p(10.0, 0.0), p(15.0, 0.0), p(20.0, 0.0)]
        );
        // The grip went with the tool: a later click puts nothing down.
        f.plain_key(ViewKey::Escape);
        f.hover(p(25.0, 5.0));
        f.click_at(p(25.0, 5.0), NONE);
        assert_eq!(
            f.undo_steps(),
            before + 1,
            "the insert, and no GRIP_EDIT"
        );
        assert_eq!(f.document.history().undo_name(), "VERTEX_INSERT");
    }

    #[test]
    fn an_insert_is_answered_with_what_it_did_not_a_refusal_until_the_pointer_moves() {
        // The restarted tool's preview where the vertex now is would refuse a second click
        // there, and the view drew that in red at once.
        let mut f = Plan::over_polyline();
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        f.hover(p(15.0, 0.3));
        f.click_at(p(15.0, 0.3), NONE);
        f.paint();
        assert_eq!(positions(&f).len(), 4);
        assert!(!f.view.last_preview_counts().refused);
        assert_eq!(
            f.view.last_preview_counts().caption,
            format!(
                "vertex 2 added to polyline {} on segment 1 (4 vertices)",
                f.polyline
            )
        );
        // Away (2 units, 20 px) and back: now a click there would be refused, and the preview
        // says so.
        f.hover(p(17.0, 0.3));
        f.paint();
        assert!(!f.view.last_preview_counts().refused);
        f.hover(p(15.0, 0.3));
        f.paint();
        assert!(f.view.last_preview_counts().refused);
    }

    #[test]
    fn the_snap_marker_lies_under_the_new_vertex_not_over_it() {
        // With the snaps on, the cursor by the middle of segment 0 snaps to its Midpoint, (5,0),
        // where the new vertex goes: the triangle was drawn over the disc and hid its "+".
        let mut f = Plan::over_polyline();
        f.set_snap_enabled(true);
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        f.hover(p(5.2, 0.3));
        let image = f.grab();
        assert_eq!(f.view.last_preview_counts().added, 1);
        let disc = f.pixel(p(5.0, 0.0));
        let snap = overlay::snap();
        // The Midpoint marker is a triangle 6 px each way (the view's snap marker): its right
        // side runs from (0,-6) to (6,6) about the point, through (3.6,1.2) - inside the 6 px
        // disc and off its "+". There: the disc's cyan, and no yellow over it.
        let on_the_side = PointF::new(disc.x + 3.6, disc.y + 1.2);
        assert_eq!(ink_near(&image, on_the_side, 0.8, snap), 0);
        assert!(ink_near(&image, on_the_side, 0.8, overlay::preview()) > 0);
        // The marker is there, beyond the disc: the triangle's lower corners.
        assert!(ink_near(&image, disc, 10.0, snap) > 0);
    }

    #[test]
    fn a_polyline_on_a_layer_this_view_hides_is_not_edited_through_the_selection() {
        // Selected, then its layer hidden in this view alone: the view's pick passes it over,
        // and the tool's own look through the selection must too.
        let mut f = Plan::over_polyline();
        f.select();
        let layer = f
            .entity(f.polyline)
            .expect("the polyline is there")
            .layer
            .clone();
        f.state.layers.hide(&layer);
        f.start_tool("draw.vertex.delete")
            .expect("the tool starts");
        f.hover(p(10.0, 0.0));
        f.paint();
        assert_eq!(f.view.last_preview_counts().removed, 0);
        f.click_at(p(10.0, 0.0), NONE);
        assert_eq!(positions(&f).len(), 3);
    }

    #[test]
    fn beside_a_chosen_vertex_enters_place_is_drawn() {
        let mut f = Plan::over_polyline();
        f.select();
        // The middle vertex's grip, now hot.
        f.click_at(p(10.0, 0.0), NONE);
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        // By segment 0: the click's place there, and Enter's at (15,0), the middle of segment 1.
        f.hover(p(6.0, 0.3));
        let image = f.grab();
        assert_eq!(f.view.last_preview_counts().enter, 1);
        assert_eq!(f.view.last_preview_counts().added, 1);
        assert!(ink_near(&image, f.pixel(p(15.0, 0.0)), 7.5, overlay::preview()) > 0);
    }

    #[test]
    fn the_pointer_record_reads_back_as_one_record_with_a_bearing_in_its_caption() {
        // Move Vertex's caption ends in a bearing, whose seconds mark is a double quote. The
        // record quoted the caption by hand, the quote closed it early, and the line read back
        // as no record at all. It is written as every reply is (core's reply_quoted), and read
        // back whole.
        let mut f = Plan::over_polyline();
        f.select();
        f.start_tool("draw.vertex.move")
            .expect("the tool starts");
        // Vertex 1.
        let _ = f
            .view
            .pointer_at(&mut f.document, &mut f.state, p(10.0, 0.0), true, NONE)
            .expect("the click is in the view");
        let hovered = f
            .view
            .pointer_at(&mut f.document, &mut f.state, p(10.0, 3.0), false, NONE)
            .expect("the hover is in the view");
        let caption = f.view.last_preview_counts().caption.clone();
        assert!(
            caption.contains('"'),
            "a bearing's seconds mark: {caption}"
        );
        let record = read_reply_record(&hovered).unwrap_or_else(|| panic!("{hovered}"));
        assert_eq!(record.words, ["pointer:"]);
        assert_eq!(record.value("caption"), Some(caption.as_str()));
        assert_eq!(
            record.value("prompt"),
            Some("Move Vertex: Specify its new position")
        );
        // Its keys for the roles are the roles' own names.
        for role in [
            FeedbackRole::Target,
            FeedbackRole::Added,
            FeedbackRole::Removed,
            FeedbackRole::Enter,
        ] {
            assert!(
                record.value(to_string(role)).is_some(),
                "{}",
                to_string(role)
            );
        }
        assert_eq!(
            record.value("added"),
            Some("1"),
            "the vertex where it goes"
        );
    }

    #[test]
    fn a_long_prompt_is_cut_in_the_middle_so_the_tools_name_stays() {
        // Cut at the left, a prompt longer than the view lost the tool's name - at the larger
        // text sizes, or in a narrow view - and the band read "...Vertex: Click beside". With
        // nothing typed it is cut in the middle; with something typed, at the left, where the
        // typing is.
        let mut f = Plan::over_polyline();
        f.resize(260, 300);
        f.select();
        // The middle vertex's grip, now chosen.
        f.click_at(p(10.0, 0.0), NONE);
        f.start_tool("draw.vertex.insert")
            .expect("the tool starts");
        f.paint();
        let band = String::from(f.view.prompt_band_text());
        assert!(band.starts_with("Insert Vertex: "), "{band}");
        assert!(band.contains('\u{2026}'), "cut: {band}");
        assert!(band.ends_with('_'), "{band}");
        let _ = f.key_press(ViewKey::Other, NONE, "4");
        f.paint();
        let band = String::from(f.view.prompt_band_text());
        assert!(band.ends_with("4_"), "{band}");
        assert!(!band.starts_with("Insert Vertex: "), "{band}");
    }
}
