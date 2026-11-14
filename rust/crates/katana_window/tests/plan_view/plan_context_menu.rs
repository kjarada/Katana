//! The twin of the PlanContextMenu and PlanViewShortcutMenu cases of
//! tests/qt_widgets/test_plan_context_menu.cpp (its SelectById cases are twinned with the dialog):
//! what the plan view's shortcut menu offers with and without a selection, that its own items run
//! the verb line a person would type through the runner - one undo step each - and that the view
//! raises the menu and the double click with the right selection.
//!
//! The runner here is the window's in miniature: a recording host whose lines are run by a
//! CommandInterpreter on the same Document, and what it answered comes back as the window's
//! runner returns it. The window's actions the menu finds by object name are a list of action
//! rows; one triggered through the menu is kept in `triggered`.
//!
//! Where the C++ item calls the colour chooser and then the runner, the port's item answers a
//! ChooseColour and the answer is run by `PlanContextMenu::colour_chosen`, so the twin gives the
//! colour there. `ChoosingAColourRunsColorEvenWhenTheMenuIsDestroyedWhileItAsks` drops the menu
//! between the two: the C++'s count of calls through a destroyed runner has nothing to count
//! here, where a choice owns what it runs.
//!
//! `PlanViewShortcutMenu.TheWorkspaceGivesTheMenuFromAViewMadeBeforeItsHookWasSet` is about the
//! workspace's hooks and waits for view_workspace (wave 3).
//!
//! The plan view is 400 x 300 pixels, centred on the origin at 10 pixels a unit, so pixel
//! (250, 150) is model (5, 0).

use katana_cad::Document;
use katana_cad::drawing::grips::{Grip, GripKind};
use katana_cad::drawing::vertex_editing::read_polyline;
use katana_commands::{create_entities, create_layer};
use katana_entity::{CalloutShape, Entity, EntityId, Geometry, Layer};
use katana_geometry::{CurvePolyline2, Point2};
use katana_window::canvas::Color;
use katana_window::drive::action::{Action, Menu, MenuItem};
use katana_window::host::RecordingHost;
use katana_window::plan_context_menu::{
    PlanContextMenu, PlanContextMenuContext, PlanMenuChoice, named_line,
};
use katana_window::viewport_widget::{KeyModifiers, MouseButton};

use crate::support::Plan;
use crate::window_support::interpreter_host;

/// A drawing, the interpreter the runner runs lines with, and a window's actions for the menu to
/// find by object name, as it finds the main window's.
struct MenuFixture {
    host: RecordingHost,
    actions: Vec<Action>,
    /// The window's actions triggered through the menu, by object name.
    triggered: Vec<String>,
    last_tool_id: String,
    grip: Option<Grip>,
    tools_on_grip: bool,
}

impl MenuFixture {
    fn new() -> Self {
        Self {
            host: interpreter_host(Document::new(None)),
            actions: Vec::new(),
            triggered: Vec::new(),
            last_tool_id: String::new(),
            grip: None,
            tools_on_grip: false,
        }
    }

    /// An action of the window's, its text its name.
    fn action(&mut self, name: &str) {
        self.actions.push(Action::new(name, name));
    }

    fn action_called(&mut self, name: &str, text: &str) {
        self.actions.push(Action::new(name, text));
    }

    /// A line run on the drawing for the test's own set-up, not through the runner.
    #[track_caller]
    fn run(&mut self, line: &str) {
        let parts = &mut self.host.parts;
        if let Err(error) = parts.interpreter.run(&mut parts.document, line) {
            panic!("{line}: {}", error.describe());
        }
    }

    /// The menu built for the drawing's selection as it is now.
    fn menu(&self) -> PlanContextMenu {
        PlanContextMenu::new(PlanContextMenuContext {
            document: self.host.document(),
            actions: &self.actions,
            last_tool_id: self.last_tool_id.clone(),
            grip: self.grip,
            tools_on_grip: self.tools_on_grip,
        })
    }

    /// The item chosen, as QAction::trigger: a line runs through the runner, a window's action
    /// is counted as triggered, and the choice is answered for the rest.
    fn trigger(&mut self, menu: &PlanContextMenu, name: &str) -> Option<PlanMenuChoice> {
        let choice = menu.trigger(name, &mut self.host);
        if let Some(PlanMenuChoice::Trigger(action)) = &choice {
            self.triggered.push(action.clone());
        }
        choice
    }

    /// The lines the runner was handed, in order.
    fn lines(&self) -> Vec<String> {
        self.host.lines().to_vec()
    }

    fn document(&self) -> &Document {
        self.host.document()
    }

    fn entity(&self, id: EntityId) -> &Entity {
        self.document()
            .model()
            .entities
            .find(id)
            .expect("the entity is there")
    }

    fn undo_steps(&self) -> usize {
        self.document().history().undo_count()
    }
}

/// The menu's items in order, a submenu by its own name, separators and section titles left out.
fn items_of(menu: &Menu) -> Vec<String> {
    menu.items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action(action) => Some(action.name.clone()),
            MenuItem::Menu(under) => Some(under.name.clone()),
            MenuItem::Section(_) | MenuItem::Separator => None,
        })
        .collect()
}

/// The menu's own item of that name.
#[track_caller]
fn item<'m>(menu: &'m PlanContextMenu, name: &str) -> &'m Action {
    menu.item(name)
        .unwrap_or_else(|| panic!("the menu has no {name}"))
}

fn callout(f: &MenuFixture, id: EntityId) -> CalloutShape {
    match &f.entity(id).geometry {
        Geometry::Leader(leader) => leader.callout,
        other => panic!("not a leader: {other:?}"),
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|&name| String::from(name)).collect()
}

mod plan_context_menu {
    use super::*;

    #[test]
    fn leader_editing_and_frames_use_the_existing_manager_and_shared_scoped_command() {
        let mut f = MenuFixture::new();
        f.action("annotateLeaders");
        f.run("LEADER 0,0 5,5 text=A");
        f.run("LEADER 0,10 5,15 text=B");
        f.run("LINE 0,0 10,0");
        f.run("SELECT ALL");
        let menu = f.menu();
        assert!(menu.item("planContextEditLeader").is_some());
        assert!(menu.item("planContextLeaderFrame.box").is_some());
        let _ = f.trigger(&menu, "planContextEditLeader");
        assert_eq!(f.triggered, ["annotateLeaders"]);
        let before = f.undo_steps();
        let _ = f.trigger(&menu, "planContextLeaderFrame.box");
        assert_eq!(f.lines(), ["LEADER SET SELECTION callout=box"]);
        assert_eq!(f.undo_steps(), before + 1);
        for id in [1, 2] {
            assert_eq!(callout(&f, id), CalloutShape::Box);
        }
        assert!(matches!(f.entity(3).geometry, Geometry::Line(_)));
        let refreshed = f.menu();
        assert_eq!(
            item(&refreshed, "planContextLeaderFrame.box").checked,
            Some(true)
        );
        f.host
            .document_mut()
            .undo()
            .expect("the frame is undone");
        assert_eq!(callout(&f, 1), CalloutShape::None);
    }

    #[test]
    fn with_a_selection_it_offers_the_windows_actions_and_the_selections_verbs() {
        let mut f = MenuFixture::new();
        for name in [
            "editErase",
            "modify.move",
            "modify.rotate",
            "editAttributes",
            "inquiry.list",
            "select.similar",
            "formatGlobalModify",
            "editDeselect",
            "editSelectAll",
            "viewZoomExtents",
        ] {
            f.action(name);
        }
        f.run("LINE 0,0 10,0");
        f.run("SELECT 1");

        let menu = f.menu();
        assert_eq!(menu.menu().name, "planContextMenu");
        // The window's own actions, found by name - the same action as the menu bar's, not a
        // copy with another name - and the menu's items between them. Selecting all and zooming
        // are for a menu with nothing selected.
        assert_eq!(
            items_of(menu.menu()),
            names(&[
                "editErase",
                "modify.move",
                "modify.rotate",
                "planContextLayer",
                "planContextStyle",
                "planContextColour",
                "editAttributes",
                "planContextInfo",
                "inquiry.list",
                "select.similar",
                "formatGlobalModify",
                "editDeselect",
            ])
        );
        assert!(
            menu.item("editErase").is_none(),
            "the window's action is shown, never made again as the menu's own"
        );
        assert!(menu.menu().actions().contains(&&f.actions[0]));
    }

    #[test]
    fn an_editor_the_window_does_not_have_is_left_out_and_one_it_has_is_offered_for_its_kind() {
        let mut f = MenuFixture::new();
        f.run("TEXT 0,0 2.5 \"kerb\"");
        f.run("LINE 0,0 10,0");
        f.run("SELECT 1");
        let offered = |f: &MenuFixture| {
            items_of(f.menu().menu())
                .iter()
                .filter(|name| *name == "annotateEditText")
                .count()
        };
        assert_eq!(offered(&f), 0);
        f.action("annotateEditText");
        assert_eq!(offered(&f), 1);
        // A line is no text to edit.
        f.run("SELECT 2");
        assert_eq!(offered(&f), 0);
    }

    #[test]
    fn with_nothing_selected_it_offers_selecting_zooming_and_the_last_tool_again() {
        let mut f = MenuFixture::new();
        for name in [
            "editErase",
            "editSelectAll",
            "editSelectById",
            "viewZoomExtents",
        ] {
            f.action(name);
        }
        f.action_called("draw.circle", "Circle");
        f.last_tool_id = String::from("draw.circle");
        let menu = f.menu();
        assert_eq!(
            items_of(menu.menu()),
            names(&[
                "planContextRepeat",
                "editSelectAll",
                "editSelectById",
                "viewZoomExtents",
            ])
        );
        assert_eq!(item(&menu, "planContextRepeat").text, "Repeat Circle");
        let _ = f.trigger(&menu, "planContextRepeat");
        assert_eq!(
            f.triggered,
            ["draw.circle"],
            "Repeat is the tool's own action, so it starts as a click on Circle does"
        );

        // No tool run yet: nothing to repeat.
        f.last_tool_id.clear();
        let fresh = f.menu();
        assert!(fresh.item("planContextRepeat").is_none());
    }

    #[test]
    fn put_on_layer_runs_chlayer_through_the_runner_as_one_undo_step() {
        let mut f = MenuFixture::new();
        f.run("LAYER NEW walls");
        f.run("LINE 0,0 10,0");
        f.run("SELECT 1");
        let steps = f.undo_steps();

        let menu = f.menu();
        let _ = item(&menu, "planContextLayer.walls");
        let _ = f.trigger(&menu, "planContextLayer.walls");

        assert_eq!(f.lines(), ["CHLAYER walls"]);
        assert_eq!(f.entity(1).layer, "walls");
        assert_eq!(f.undo_steps(), steps + 1);
        f.host
            .document_mut()
            .undo()
            .expect("the change of layer is undone");
        assert_eq!(f.entity(1).layer, "0");
    }

    #[test]
    fn layers_under_a_layer_are_a_submenu_headed_by_that_layer_and_blanks_are_quoted() {
        let mut f = MenuFixture::new();
        f.run("LAYER NEW site/kerb/top");
        f.run("LAYER NEW \"old work\"");
        f.run("LINE 0,0 10,0");
        f.run("SELECT 1");
        let menu = f.menu();

        // LAYER NEW made site and site/kerb as well; each is a submenu of what is under it,
        // headed by the layer itself.
        let site = menu
            .submenu("planContextLayerTree.site")
            .expect("site is a submenu");
        assert_eq!(
            items_of(site),
            names(&["planContextLayer.site", "planContextLayerTree.site/kerb"])
        );
        let kerb = menu
            .submenu("planContextLayerTree.site/kerb")
            .expect("site/kerb is a submenu");
        assert_eq!(
            items_of(kerb),
            names(&[
                "planContextLayer.site/kerb",
                "planContextLayer.site/kerb/top"
            ])
        );
        let top = menu
            .submenu("planContextLayer")
            .expect("Put on Layer is a submenu");
        assert_eq!(
            items_of(top),
            names(&[
                "planContextLayer.0",
                "planContextLayer.old work",
                "planContextLayerTree.site",
            ])
        );

        let _ = f.trigger(&menu, "planContextLayer.site/kerb/top");
        let _ = f.trigger(&menu, "planContextLayer.old work");
        assert_eq!(
            f.lines(),
            ["CHLAYER site/kerb/top", "CHLAYER \"old work\""]
        );
        assert_eq!(f.entity(1).layer, "old work");
    }

    // A DXF can bring a layer whose name holds a double quote, which no typed line can carry. Its
    // item is still listed - the layer is there - but disabled, saying why, rather than running
    // CHLAYER on the name cut short at the quote.
    #[test]
    fn a_layer_no_line_can_name_is_offered_disabled_and_runs_nothing() {
        let mut f = MenuFixture::new();
        let pipes = Layer {
            name: String::from("6\" pipes"),
            ..Layer::default()
        };
        f.host
            .document_mut()
            .execute(create_layer(pipes))
            .expect("the layer is made");
        f.run("LINE 0,0 10,0");
        f.run("SELECT 1");
        let menu = f.menu();

        let quoted = item(&menu, "planContextLayer.6\" pipes");
        assert!(!quoted.enabled);
        assert_eq!(
            quoted.tip,
            "the layer: a double quote cannot be written on a command line"
        );
        let _ = f.trigger(&menu, "planContextLayer.6\" pipes");
        assert!(f.lines().is_empty());
        assert_eq!(f.entity(1).layer, "0");
    }

    #[test]
    fn the_layer_the_whole_selection_is_on_is_ticked_and_none_when_it_is_on_several() {
        let mut f = MenuFixture::new();
        f.run("LAYER NEW walls");
        f.run("LINE 0,0 10,0");
        f.run("LINE 0,5 10,5");
        f.run("SELECT 1 2");
        f.run("CHLAYER walls");
        {
            let menu = f.menu();
            assert_eq!(item(&menu, "planContextLayer.walls").checked, Some(true));
            assert_eq!(item(&menu, "planContextLayer.0").checked, Some(false));
        }
        f.run("SELECT 1");
        f.run("CHLAYER 0");
        f.run("SELECT 1 2");
        let menu = f.menu();
        assert_eq!(item(&menu, "planContextLayer.walls").checked, Some(false));
        assert_eq!(item(&menu, "planContextLayer.0").checked, Some(false));
    }

    #[test]
    fn style_and_colour_run_style_apply_and_color_and_a_cancelled_colour_runs_nothing() {
        let mut f = MenuFixture::new();
        f.run("STYLE NEW fence");
        f.run("LINE 0,0 10,0");
        f.run("SELECT 1");
        {
            let menu = f.menu();
            assert_eq!(item(&menu, "planContextStyle.ByLayer").checked, Some(true));
            let _ = f.trigger(&menu, "planContextStyle.fence");
            let asked = f.trigger(&menu, "planContextColour.Choose");
            assert!(matches!(asked, Some(PlanMenuChoice::ChooseColour { .. })));
            // The colour dialog answers red.
            let _ = PlanContextMenu::colour_chosen(&mut f.host, Some(Color::rgb(255, 0, 0)));
        }
        assert_eq!(f.entity(1).style, "fence");
        assert_eq!(
            f.entity(1).color.map(|colour| colour.to_hex()),
            Some(String::from("#FF0000"))
        );
        {
            let menu = f.menu();
            assert_eq!(item(&menu, "planContextStyle.fence").checked, Some(true));
            assert_eq!(item(&menu, "planContextStyle.ByLayer").checked, Some(false));
            let asked = f.trigger(&menu, "planContextColour.Choose");
            assert_eq!(
                asked,
                Some(PlanMenuChoice::ChooseColour {
                    initial: Color::rgb(255, 0, 0)
                }),
                "the dialog starts from the selection's colour"
            );
            // The colour dialog cancelled.
            assert!(PlanContextMenu::colour_chosen(&mut f.host, None).is_none());
            let _ = f.trigger(&menu, "planContextStyle.ByLayer");
            let _ = f.trigger(&menu, "planContextColour.ByLayer");
        }
        assert_eq!(
            f.lines(),
            [
                "STYLE APPLY fence",
                "COLOR #FF0000",
                "STYLE APPLY -",
                "COLOR BYLAYER"
            ]
        );
        assert!(f.entity(1).style.is_empty());
        assert!(f.entity(1).color.is_none());
    }

    // The C++ window deletes its menu as it hides, and since Qt 6.7 the colour dialog's own event
    // loop can run that delete while Choose is still asking; Choose then reached its runner
    // through the destroyed menu (the crash recolouring 774 selected leaders, 2026-11-03). Here
    // the menu is dropped between the asking and the answer, and the line must still run.
    #[test]
    fn choosing_a_colour_runs_color_even_when_the_menu_is_destroyed_while_it_asks() {
        let mut f = MenuFixture::new();
        f.run("LEADER 0,0 5,5 text=A");
        f.run("LEADER 0,10 5,15 text=B");
        f.run("SELECT ALL");
        let steps = f.undo_steps();

        let menu = f.menu();
        let asked = f.trigger(&menu, "planContextColour.Choose");
        assert!(matches!(asked, Some(PlanMenuChoice::ChooseColour { .. })));
        // The menu goes while the colour is being asked for, as the colour dialog's loop
        // destroys it.
        drop(menu);
        let outcome = PlanContextMenu::colour_chosen(&mut f.host, Some(Color::rgb(0, 0, 255)));

        assert!(outcome.is_some_and(|outcome| outcome.ok));
        assert_eq!(f.lines(), ["COLOR #0000FF"]);
        for id in [1, 2] {
            assert_eq!(
                f.entity(id).color.map(|colour| colour.to_hex()),
                Some(String::from("#0000FF"))
            );
        }
        assert_eq!(
            f.undo_steps(),
            steps + 1,
            "one recolour, one undo step"
        );
    }

    #[test]
    fn entity_information_is_for_one_entity_and_runs_info() {
        let mut f = MenuFixture::new();
        f.run("RECT 0,0 10,5");
        f.run("POINT 20,20");
        f.run("SELECT 1");
        {
            let menu = f.menu();
            let _ = f.trigger(&menu, "planContextInfo");
        }
        // #1: the entity, even where a file is called 1 (the session's INFO).
        assert_eq!(f.lines(), ["INFO #1"]);
        let outcome = f
            .host
            .outcomes()
            .last()
            .cloned()
            .expect("the line was answered");
        assert!(outcome.ok);
        // A 10 x 5 rectangle: perimeter 30, area 50.
        assert_eq!(
            outcome.reply.trim(),
            "1  Polyline  layer=0  vertices=4  closed  length=30  area=50"
        );

        f.run("SELECT 1 2");
        let menu = f.menu();
        assert!(menu.item("planContextInfo").is_none());
    }

    #[test]
    fn an_argument_is_quoted_only_when_it_must_be() {
        let line = |name: &str| named_line("CHLAYER", name, "the layer");
        assert_eq!(line("walls").expect("a plain word"), "CHLAYER walls");
        assert_eq!(
            line("site/kerb").expect("a path is a plain word"),
            "CHLAYER site/kerb"
        );
        assert_eq!(
            line("old work").expect("a blank is quoted"),
            "CHLAYER \"old work\""
        );
        assert_eq!(line("").expect("an empty name is quoted"), "CHLAYER \"\"");
        // No line can carry a double quote - the tokenizer has no escape - so quoting one would
        // act on the name cut short at it.
        let refused = line("6\" pipes").expect_err("a double quote cannot be written");
        assert_eq!(
            refused.message,
            "the layer: a double quote cannot be written on a command line"
        );
    }

    #[test]
    fn a_vertex_grips_items_run_the_vertex_verbs_and_start_the_tools_on_it() {
        let mut f = MenuFixture::new();
        f.action("editErase");
        f.run("PLINE 0,0 10,0 20,0");
        f.run("SELECT 1");
        f.grip = Some(Grip {
            entity: 1,
            kind: GripKind::Vertex,
            index: 1,
            position: Point2::new(10.0, 0.0),
        });
        f.tools_on_grip = true;
        let menu = f.menu();
        // The vertex's own items first, before the selection's; an open polyline has no start to
        // change.
        let items = items_of(menu.menu());
        assert!(items.len() >= 8);
        assert_eq!(
            items[..8],
            names(&[
                "planContextVertex.Delete",
                "planContextVertex.InsertAfter",
                "planContextVertex.Move",
                "planContextVertex.Height",
                "planContextVertex.Fillet",
                "planContextVertex.Chamfer",
                "planContextVertex.Straighten",
                "editErase",
            ])
        );
        assert_eq!(
            item(&menu, "planContextVertex.InsertAfter").tip,
            "VERTEX INSERT 1 #1.s1 after=1"
        );
        match f.trigger(&menu, "planContextVertex.Move") {
            Some(PlanMenuChoice::StartToolOn { tool, grip }) => {
                assert_eq!((tool.as_str(), grip.index), ("draw.vertex.move", 1));
            }
            other => panic!("Move Vertex starts its tool on the grip: {other:?}"),
        }
        let _ = f.trigger(&menu, "planContextVertex.Delete");
        assert_eq!(f.lines(), ["VERTEX DELETE 1 1"]);
        let after = read_polyline(f.entity(1)).expect("entity 1 is a polyline");
        assert_eq!(
            after.vertices.len(),
            2,
            "one undoable edit, run as its line"
        );
    }

    #[test]
    fn an_end_vertex_offers_no_corner_and_no_segment_after_it() {
        let mut f = MenuFixture::new();
        f.run("PLINE 0,0 10,0 20,0");
        f.run("SELECT 1");
        f.grip = Some(Grip {
            entity: 1,
            kind: GripKind::Vertex,
            index: 2,
            position: Point2::new(20.0, 0.0),
        });
        f.tools_on_grip = true;
        let menu = f.menu();
        assert!(!item(&menu, "planContextVertex.InsertAfter").enabled);
        assert!(!item(&menu, "planContextVertex.Fillet").enabled);
        assert_eq!(
            item(&menu, "planContextVertex.Fillet").tip,
            "vertex 2 is an end of the polyline; a corner is where two segments meet"
        );
        assert!(item(&menu, "planContextVertex.Delete").enabled);
    }

    #[test]
    fn a_segment_grip_offers_make_straight_only_on_an_arc() {
        let mut f = MenuFixture::new();
        f.run("PLINE 0,0 10,0 20,0");
        let mut shape = CurvePolyline2::from_points(
            &[
                Point2::new(0.0, 20.0),
                Point2::new(10.0, 20.0),
                Point2::new(20.0, 20.0),
            ],
            false,
        );
        shape.vertices[0].bulge = 1.0;
        let curved = Entity {
            geometry: Geometry::CurvePolyline(shape),
            ..Entity::default()
        };
        f.host
            .document_mut()
            .execute(create_entities(vec![curved]))
            .expect("the curved polyline is made");
        f.run("SELECT 1 2");
        f.grip = Some(Grip {
            entity: 1,
            kind: GripKind::SegmentMid,
            index: 0,
            position: Point2::new(5.0, 0.0),
        });
        {
            let menu = f.menu();
            assert!(
                menu.item("planContextSegment.Line").is_none(),
                "a straight segment is straight already"
            );
            let _ = f.trigger(&menu, "planContextSegment.AddMiddle");
            assert_eq!(f.lines(), ["VERTEX INSERT 1 #1.s0 after=0"]);
            assert_eq!(
                read_polyline(f.entity(1))
                    .expect("entity 1 is a polyline")
                    .vertices[1]
                    .position,
                Point2::new(5.0, 0.0)
            );
        }
        f.grip = Some(Grip {
            entity: 2,
            kind: GripKind::SegmentMid,
            index: 0,
            position: Point2::new(5.0, 15.0),
        });
        let menu = f.menu();
        assert_eq!(
            item(&menu, "planContextSegment.Line").tip,
            "VERTEX SET 2 0 bulge=0"
        );
    }
}

// ---- the plan view raises it ---------------------------------------------------------------

/// A plan view, as test_plan_view_tools makes one, whose owner shows the shortcut menu.
fn plan() -> Plan {
    let mut plan = Plan::at_origin();
    plan.view.hooks.context_menu = true;
    plan
}

fn right_click(plan: &mut Plan, x: f64, y: f64) {
    plan.press_with(x, y, MouseButton::Right, KeyModifiers::NONE);
}

mod plan_view_shortcut_menu {
    use super::*;

    #[test]
    fn a_right_click_on_an_entity_with_nothing_selected_selects_it_for_the_menu() {
        let mut plan = plan();
        let id = plan.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        right_click(&mut plan, 250.0, 150.0); // (5, 0), on the line
        assert_eq!(plan.menus(), 1);
        assert_eq!(plan.document.selection().ids(), [id]);

        // On empty space with nothing selected: the menu, with nothing to act on.
        plan.document.selection_mut().clear();
        right_click(&mut plan, 250.0, 50.0); // (5, 10)
        assert_eq!(plan.menus(), 2);
        assert!(plan.document.selection().empty());
    }

    #[test]
    fn a_right_click_keeps_the_selection_it_finds_there() {
        let mut plan = plan();
        let first = plan.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let _second = plan.add_line(Point2::new(0.0, 10.0), Point2::new(10.0, 10.0));
        plan.document.selection_mut().set(vec![first]);
        right_click(&mut plan, 250.0, 50.0); // on the second line
        assert_eq!(plan.menus(), 1);
        assert_eq!(
            plan.document.selection().ids(),
            [first],
            "the menu is for the selection, which a click beside it must not change"
        );
    }

    #[test]
    fn a_double_click_on_an_entity_asks_to_edit_it_and_empty_space_asks_nothing() {
        let mut plan = plan();
        let id = plan.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        plan.double_click(250.0, 150.0);
        assert_eq!(plan.double_clicked(), [id]);
        assert_eq!(plan.document.selection().ids(), [id]);
        plan.double_click(250.0, 50.0);
        assert_eq!(plan.double_clicked().len(), 1);
    }

    #[test]
    fn a_double_click_while_a_tool_runs_is_the_tools_click_not_an_edit() {
        let mut plan = plan();
        let _ = plan.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        plan.start_tool("draw.line").expect("the tool starts");
        plan.double_click(250.0, 150.0);
        assert!(plan.double_clicked().is_empty());
        right_click(&mut plan, 250.0, 150.0); // Enter, not the menu
        assert_eq!(plan.menus(), 0);
    }
}
