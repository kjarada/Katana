// What every tool's preview can say and where a tool picks (tool_feedback.rs,
// interactive_tool.rs; tests/cad/tools/test_tool_feedback.cpp): the roles'
// names, the pick through the view or the model restricted to the kinds a
// tool asks for, and the reaches that stand in for the view's apertures in a
// test.

use std::cell::Cell;
use std::collections::BTreeSet;

use katana_cad::interactive_tool::{
    Aperture, ToolContext, ViewPick, pick_reach, pick_under, vertex_reach,
};
use katana_cad::{Document, FeedbackRole};
use katana_commands::{EntityAttributes, create_line, create_polyline};
use katana_entity::entity::to_string;
use katana_entity::{EntityId, EntityType};
use katana_geometry::{Point2, Polyline2};

use crate::{double_eq, ok};

mod tool_feedback {
    use super::*;

    #[test]
    fn feedback_roles_have_stable_names() {
        // The headless pointer record's keys are these (the viewport's
        // pointer record; docs/headless.md): a test matching "target=1
        // added=1" must not break on a rename.
        assert_eq!(to_string(FeedbackRole::Target), "target");
        assert_eq!(to_string(FeedbackRole::Added), "added");
        assert_eq!(to_string(FeedbackRole::Removed), "removed");
        assert_eq!(to_string(FeedbackRole::Enter), "enter");
    }

    #[test]
    fn pick_under_takes_only_the_types_asked() {
        let mut document = Document::default();
        ok(document.execute(create_polyline(
            Polyline2::new(vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)], false),
            EntityAttributes::default(),
        )));
        let polyline = document.last_created_entities()[0];
        let attributes = document.current_attributes();
        ok(document.execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            attributes,
        )));
        let line = document.last_created_entities()[0];
        assert!(line > polyline);

        let cx = ToolContext {
            document: Some(&document),
            pick_tolerance: 0.5,
            ..ToolContext::default()
        };
        let any = BTreeSet::new();
        let polylines = BTreeSet::from([EntityType::Polyline, EntityType::CurvePolyline]);
        // Both lie under (5,0) at distance 0; a tie goes to the higher id,
        // the one drawn on top (selection.rs) - unless only polylines are
        // asked for.
        assert_eq!(
            pick_under(&cx, Point2::new(5.0, 0.0), &any, None),
            Some(line)
        );
        assert_eq!(
            pick_under(&cx, Point2::new(5.0, 0.0), &polylines, None),
            Some(polyline)
        );
        // Out of reach: 0.6 from both, with a reach of 0.5.
        assert!(pick_under(&cx, Point2::new(5.0, 0.6), &any, None).is_none());
        // A reach given explicitly is used instead of the tool's.
        assert_eq!(
            pick_under(&cx, Point2::new(5.0, 0.6), &polylines, Some(0.75)),
            Some(polyline)
        );
    }

    #[test]
    fn a_views_pick_is_used_when_the_context_has_one() {
        let document = Document::default();
        let reach_asked = Cell::new(0.0);
        let pick: &ViewPick<'_> =
            &|_at: Point2, reach: f64, _types: &BTreeSet<EntityType>| -> Option<EntityId> {
                reach_asked.set(reach);
                Some(42)
            };
        let cx = ToolContext {
            document: Some(&document),
            pick_tolerance: 0.5,
            pick: Some(pick),
            ..ToolContext::default()
        };
        // The view's answer, even with nothing in the model: it is the view
        // that knows its hidden layers.
        assert_eq!(
            pick_under(&cx, Point2::new(0.0, 0.0), &BTreeSet::new(), None),
            Some(42)
        );
        double_eq(reach_asked.get(), 0.5);
    }

    #[test]
    fn reach_falls_back_to_the_tools_aperture() {
        let mut cx = ToolContext {
            pick_tolerance: 0.5,
            ..ToolContext::default()
        };
        double_eq(pick_reach(&cx), 0.5);
        // 12 px over 8 px, the view's snap aperture over its pick aperture.
        double_eq(vertex_reach(&cx), 0.75);
        // The view's own, read now, win over the tolerance the tool was made
        // with.
        let pick_aperture: &Aperture<'_> = &|| 2.0;
        let vertex_aperture: &Aperture<'_> = &|| 3.0;
        cx.pick_aperture = Some(pick_aperture);
        cx.vertex_aperture = Some(vertex_aperture);
        double_eq(pick_reach(&cx), 2.0);
        double_eq(vertex_reach(&cx), 3.0);
    }
}
