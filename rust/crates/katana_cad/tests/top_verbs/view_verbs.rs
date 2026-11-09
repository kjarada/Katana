// VIEWS and ZOOM (view_verbs.rs): the window's views on the command line, run
// through the interpreter as the window runs them, against a real ViewSet held
// by a host that does what the window's workspace does with a request and
// records it (tests/cad/test_view_verbs.cpp). Every expected number is worked
// by hand from ViewTransform's definitions - a view W by H pixels centred on c
// at s pixels a unit shows x from c.x - W/2s to c.x + W/2s and y likewise -
// never read back from a run.

use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use katana_cad::scope_verbs::scope_view_of;
use katana_cad::selection::extent_of;
use katana_cad::view_set::set_view_paper_scales;
use katana_cad::view_verbs::{apply_plan_zoom, zoom_takes};
use katana_cad::{
    CommandInterpreter, Document, ViewId, ViewKind, ViewSet, ViewState, ViewTransform,
    ViewVerbHost, ZoomRequest, ZoomRequestKind,
};
use katana_commands::{create_entities, create_label_style, create_layer};
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::{Entity, EntityId, Geometry, LabelGeometry, LabelStyle, Layer};
use katana_geometry::{Box2, Point2, Point3};
use katana_math::numerics::PI;
use katana_render::Projection;

use crate::{Session, done, lines_of, must, near};

// What the window's workspace does with a request, minus the widgets: a plan
// view's IN, OUT, factor and CENTRE through apply_plan_zoom, WINDOW fitted with
// the 0.08 margin Zoom Extents leaves, EXTENTS left where it is (it frames
// what a widget draws, which there is none of here); then the move is noted
// and the link follows, as ViewWorkspace::viewMoved does. Every method takes
// &self (view_verbs.rs, "The Rust shape of ViewVerbHost"), so the set and the
// records are in cells.
#[derive(Default)]
struct RecordingHost {
    set: RefCell<ViewSet>,
    zooms: RefCell<Vec<ZoomRequest>>,
    changes: RefCell<Vec<Vec<ViewId>>>,
    setting_changes: RefCell<Vec<ViewId>>,
    // The reference layers the window has, by id (VIEWS HIDE ... REFERENCE).
    references: RefCell<BTreeSet<u64>>,
}

impl ViewVerbHost for RecordingHost {
    fn views(&self) -> &RefCell<ViewSet> {
        &self.set
    }

    fn open(&self, kind: ViewKind) -> Result<ViewId> {
        let mut set = self.set.borrow_mut();
        let id = set.add(kind).id;
        let _ = set.activate(id);
        Ok(id)
    }

    fn activate(&self, id: ViewId) -> Status {
        self.set.borrow_mut().activate(id)
    }

    fn zoom(&self, request: &ZoomRequest) -> Result<Vec<ViewId>> {
        self.zooms.borrow_mut().push(request.clone());
        let mut set = self.set.borrow_mut();
        let Some(view) = set.find_mut(request.view) else {
            panic!(
                "the verbs asked to zoom view {}, which is not open",
                request.view
            );
        };
        if view.kind == ViewKind::Plan {
            if !apply_plan_zoom(&mut view.plan, request)
                && (request.kind == ZoomRequestKind::Window
                    || request.kind == ZoomRequestKind::Scope)
            {
                view.plan.fit(&request.window, 0.08);
            }
            view.plan_framed = true;
        }
        let id = view.id;
        set.note_moved(id);
        let mut moved = vec![id];
        moved.extend(set.follow(id));
        Ok(moved)
    }

    fn changed(&self, ids: &[ViewId]) {
        self.changes.borrow_mut().push(ids.to_vec());
    }

    fn settings_changed(&self, id: ViewId) {
        self.setting_changes.borrow_mut().push(id);
    }

    fn has_reference(&self, id: u64) -> bool {
        self.references.borrow().contains(&id)
    }
}

// The C++ fixture holds `ViewState&` across lines; a Rust test cannot hold a
// borrow of the host's set while the interpreter borrows it, so the views are
// named by id and read afresh.
struct ViewVerbsTest {
    document: Document,
    interpreter: CommandInterpreter,
    host: Rc<RecordingHost>,
}

impl ViewVerbsTest {
    fn new() -> Self {
        let host = Rc::new(RecordingHost::default());
        let mut interpreter = CommandInterpreter::new();
        let view_host: Rc<dyn ViewVerbHost> = host.clone();
        interpreter.set_view_host(Some(view_host));
        // The scope word VIEW answered as the window answers it.
        let scope_host = Rc::clone(&host);
        interpreter.set_scope_context(Some(Box::new(move |id: Option<u32>| {
            scope_view_of(&scope_host.set.borrow(), id)
        })));
        Self {
            document: Document::default(),
            interpreter,
            host,
        }
    }

    // A plan view 300 x 200 px centred on (x, y) at `scale` px a unit, framed.
    fn plan_at(&self, x: f64, y: f64, scale: f64) -> ViewId {
        let mut set = self.host.set.borrow_mut();
        let view = set.add(ViewKind::Plan);
        view.plan.resize(300.0, 200.0);
        view.plan.center = Point2::new(x, y);
        view.plan.scale = scale;
        view.plan_framed = true;
        view.id
    }

    // The fixture's default plan view: centred on (10, 20) at 4 px a unit.
    fn plan(&self) -> ViewId {
        self.plan_at(10.0, 20.0, 4.0)
    }

    fn add(&self, kind: ViewKind) -> ViewId {
        self.host.set.borrow_mut().add(kind).id
    }

    // The open view `id`, read now; dropped before the next line runs.
    fn view(&self, id: ViewId) -> Ref<'_, ViewState> {
        Ref::map(self.host.set.borrow(), |set| match set.find(id) {
            Some(view) => view,
            None => panic!("no view {id} is open"),
        })
    }

    fn zooms(&self) -> Ref<'_, Vec<ZoomRequest>> {
        self.host.zooms.borrow()
    }

    fn last_zoom(&self) -> ZoomRequest {
        match self.host.zooms.borrow().last() {
            Some(request) => request.clone(),
            None => panic!("nothing was asked of the window"),
        }
    }

    fn setting_changes(&self) -> Vec<ViewId> {
        self.host.setting_changes.borrow().clone()
    }

    // The views the last link change named to the window.
    fn last_change(&self) -> Vec<ViewId> {
        match self.host.changes.borrow().last() {
            Some(ids) => ids.clone(),
            None => panic!("no link change reached the window"),
        }
    }

    fn run(&mut self, line: &str) -> Result<String> {
        self.interpreter.run(&mut self.document, line)
    }

    #[track_caller]
    fn ok(&mut self, line: &str) -> String {
        match self.run(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line}: {}", error.describe()),
        }
    }

    #[track_caller]
    fn refused(&mut self, line: &str) -> Error {
        match self.run(line) {
            Ok(reply) => panic!("{line} was not refused: {reply}"),
            Err(error) => error,
        }
    }
}

// The value of `key=` in a record line, up to the next blank.
fn field(record: &str, key: &str) -> String {
    let Some(at) = record.find(&format!(" {key}=")) else {
        return String::new();
    };
    let start = at + key.len() + 2;
    String::from(record[start..].split(' ').next().unwrap_or(""))
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| String::from(*name)).collect()
}

// ---- a view's own layers: HIDE, SHOW, ISOLATE -------------------------------------------
//
// The drawing: the default layer 0, design with design/road beneath it, and
// asbuilt. What each line leaves hidden is worked out from LayerOverrides'
// contract (layer_overrides.rs), not read back.

fn add_layers(document: &mut Document, list: &[&str]) {
    for name in list {
        let layer = Layer {
            name: String::from(*name),
            ..Layer::default()
        };
        must(document, create_layer(layer));
    }
}

mod view_verbs_test {
    use super::*;

    #[test]
    fn views_lists_every_open_view_as_one_record_in_creation_order() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        let model = {
            let mut set = t.host.set.borrow_mut();
            let model = set.add(ViewKind::Model3D);
            model.camera.set_target(Point3::new(1.0, 2.0, 3.0));
            model.camera.set_distance(50.0);
            model.camera.set_projection(Projection::Perspective);
            model.id
        };
        t.add(ViewKind::Section);

        let reply = t.ok("VIEWS");
        let lines = lines_of(&reply);
        assert_eq!(lines.len(), 4, "{reply}");
        // 300 x 200 at 4 px a unit about (10, 20): x 10 -+ 37.5, y 20 -+ 25. And
        // every kind of view ghosts the selection until told not to
        // (ViewState::selection_ghosts).
        assert_eq!(
            lines[0],
            "view=1 kind=plan title=\"Plan 1\" active=yes linked=no centre=10,20 \
             scale=4 area=-27.5,-5,47.5,45 ghosts=on"
        );
        assert!(
            lines[1].starts_with(
                "view=2 kind=3d title=\"3D 1\" active=no target=1,2,3 distance=50 azimuth="
            ),
            "{}",
            lines[1]
        );
        // The angles in degrees: the camera's radians times 180 / pi (the C++
        // writes pi out to 21 digits, which is this same double).
        let azimuth = match parse_finite_double(field(&lines[1], "azimuth")) {
            Some(azimuth) => azimuth,
            None => panic!("no azimuth in {}", lines[1]),
        };
        near(azimuth, t.view(model).camera.azimuth() * 180.0 / PI, 1e-12);
        assert_eq!(field(&lines[1], "projection"), "perspective");
        assert_eq!(
            lines[2],
            "view=3 kind=section title=\"Section 1\" active=no ghosts=on"
        );
        assert_eq!(lines[3], "views=3 linked=0");
        // LIST is the same.
        assert_eq!(t.ok("VIEWS LIST"), reply);
    }

    #[test]
    fn paper_scale_changes_only_the_named_plan_views_paper_marks_and_is_undoable() {
        let mut t = ViewVerbsTest::new();
        let first = t.plan();
        let second = t.plan();
        let centre = t.view(first).plan.center;
        let zoom = t.view(first).plan.scale;
        let annotation = t.document.annotation_scale();
        assert_eq!(t.ok("VIEWS SCALE 1"), "view=1 kind=plan paper_scale=auto");
        assert_eq!(
            t.ok("VIEWS SCALE 1 1:500"),
            "view=1 kind=plan paper_scale=500 changed=yes"
        );
        assert_eq!(t.view(first).paper_style_scale, Some(500.0));
        assert!(t.view(second).paper_style_scale.is_none());
        assert_eq!(t.view(first).plan.center, centre);
        assert_eq!(t.view(first).plan.scale, zoom);
        assert_eq!(t.document.annotation_scale(), annotation);
        assert_eq!(t.document.history().undo_count(), 1);
        assert_eq!(
            t.ok("VIEWS SCALE 1 500"),
            "view=1 kind=plan paper_scale=500 changed=no"
        );
        assert_eq!(t.document.history().undo_count(), 1);
        assert!(t.ok("VIEWS").contains(" paper_scale=500"));
        t.ok("UNDO");
        assert_eq!(t.ok("VIEWS SCALE 1"), "view=1 kind=plan paper_scale=auto");
        assert!(t.view(first).paper_style_scale.is_none());
        t.ok("REDO");
        assert_eq!(t.ok("VIEWS SCALE 1"), "view=1 kind=plan paper_scale=500");
        assert_eq!(
            t.ok("views scale 1 auto"),
            "view=1 kind=plan paper_scale=auto changed=yes"
        );
        assert!(t.view(first).paper_style_scale.is_none());
    }

    #[test]
    fn paper_scale_refuses_invalid_values_unknown_views_and_non_plan_views_without_an_edit() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan();
        t.add(ViewKind::Model3D);
        for word in [
            "0",
            "-5",
            "nan",
            "inf",
            "1e300",
            "1e-323",
            "1:0",
            "2:500",
            "1:500:2",
            "500 extra",
        ] {
            assert!(t.run(&format!("VIEWS SCALE 1 {word}")).is_err(), "{word}");
            assert!(t.view(view).paper_style_scale.is_none());
            assert_eq!(t.document.history().undo_count(), 0);
        }
        assert_eq!(t.refused("VIEWS SCALE 99 500").code, ErrorCode::NotFound);
        assert!(t.refused("VIEWS SCALE 2 500").message.contains("plan view"));
        assert!(t.refused("VIEWS SCALE").message.contains("view id"));
        assert!(t.refused("VIEWS SCALE 0 500").message.contains("view id"));
        assert!(t.setting_changes().is_empty());
        assert_eq!(
            t.ok("VIEWS SCALE LIST"),
            "view=1 kind=plan paper_scale=auto\nviews=1"
        );
    }

    #[test]
    fn linked_views_keep_independent_paper_scales_when_their_zoom_follows() {
        let mut t = ViewVerbsTest::new();
        let first = t.plan();
        let second = t.plan();
        t.ok("VIEWS LINK 1,2");
        t.ok("VIEWS SCALE 1 250");
        t.ok("VIEWS SCALE 2 500");
        t.ok("ZOOM IN view=1");
        assert_eq!(t.view(first).plan.scale, t.view(second).plan.scale);
        assert_eq!(t.view(first).paper_style_scale, Some(250.0));
        assert_eq!(t.view(second).paper_style_scale, Some(500.0));
    }

    #[test]
    fn undoing_a_scale_after_its_view_closes_never_dereferences_the_closed_view() {
        let mut t = ViewVerbsTest::new();
        let id = t.plan();
        t.ok("VIEWS SCALE 1 500");
        done(t.host.set.borrow_mut().remove(id));
        t.ok("UNDO");
        t.ok("REDO");
        assert_eq!(t.ok("VIEWS SCALE LIST"), "views=0");
    }

    #[test]
    fn views_open_and_activate_reply_the_views_record() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        let opened = t.ok("VIEWS OPEN elevation");
        assert!(
            opened.starts_with("view=2 kind=elevation title=\"Elevation 1\" active=yes target="),
            "{opened}"
        );
        assert_eq!(t.view(2).kind, ViewKind::Elevation);
        let activated = t.ok("views activate 1");
        assert!(
            activated.starts_with("view=1 kind=plan title=\"Plan 1\" active=yes linked=no centre="),
            "{activated}"
        );
        assert_eq!(t.host.set.borrow().active_id(), 1);

        assert!(t.refused("VIEWS OPEN map").message.contains("not 'map'"));
        assert_eq!(t.refused("VIEWS ACTIVATE 9").code, ErrorCode::NotFound);
        let title = t.refused("VIEWS ACTIVATE Plan");
        assert!(
            title.message.contains("not the number in a view's title"),
            "{}",
            title.message
        );
        assert!(
            t.refused("VIEWS SHUFFLE")
                .message
                .contains("VIEWS does not take 'SHUFFLE'")
        );
    }

    #[test]
    fn views_link_replies_with_the_leader_the_members_and_who_moved() {
        let mut t = ViewVerbsTest::new();
        t.plan_at(10.0, 20.0, 4.0);
        let two = t.plan_at(-5.0, 7.0, 1.0);

        assert_eq!(t.ok("VIEWS LINK 1,2"), "leader=1 linked=1,2 moved=2");
        assert_eq!(t.view(two).plan.center, Point2::new(10.0, 20.0));
        assert_eq!(t.view(two).plan.scale, 4.0);
        assert_eq!(t.host.changes.borrow().len(), 1);
        assert_eq!(t.last_change(), [1, 2], "the window repaints both");

        // TO, ids in two words, and the link's record in VIEWS.
        assert_eq!(t.ok("VIEWS LINK 2 1 TO 2"), "leader=2 linked=1,2 moved=1");
        assert!(t.ok("VIEWS").contains("views=2 linked=2"));

        // The unlink that leaves one member dissolves the link: both left.
        assert_eq!(t.ok("VIEWS UNLINK 2"), "unlinked=1,2 linked=none");
        assert_eq!(t.last_change(), [1, 2]);
        assert_eq!(t.ok("VIEWS UNLINK ALL"), "unlinked=none linked=none");
    }

    #[test]
    fn views_link_of_a3_d_view_is_refused_naming_its_kind_and_changes_nothing() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        t.add(ViewKind::Model3D);
        let refusal = t.refused("VIEWS LINK 1,2");
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);
        assert_eq!(refusal.message, "only plan views link: view 2 is 3D");
        assert!(t.host.set.borrow().linked_views().is_empty());
        assert!(t.host.changes.borrow().is_empty());
        assert!(
            t.refused("VIEWS LINK 1 TO")
                .message
                .contains("TO takes the one view")
        );
        assert_eq!(t.refused("VIEWS LINK 1,x").code, ErrorCode::ParseFailure);
    }

    #[test]
    fn zoom_alone_is_the_active_views_extents() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        t.plan();
        done(t.host.set.borrow_mut().activate(2));
        for line in ["ZOOM", "Z", "zoom extents", "ZOOM E"] {
            let before = t.zooms().len();
            t.ok(line);
            assert_eq!(t.zooms().len(), before + 1, "{line}");
            assert_eq!(t.last_zoom().kind, ZoomRequestKind::Extents, "{line}");
            assert_eq!(t.last_zoom().view, 2, "{line}");
        }
    }

    #[test]
    fn zoom_all_and_the_apostrophe_forms_are_the_extents_as_in_every_cad_program() {
        // A script's commonest zoom lines. The old window zoomed the extents for
        // any ZOOM line; ZOOM A was then refused here, and a script that ran to
        // the end there stopped at it.
        let mut t = ViewVerbsTest::new();
        t.plan();
        for line in [
            "ZOOM A", "Z A", "zoom all", "Z ALL", "'ZOOM", "'Z", "'zoom a", "'Z E",
        ] {
            let before = t.zooms().len();
            t.ok(line);
            assert_eq!(t.zooms().len(), before + 1, "{line}");
            assert_eq!(t.last_zoom().kind, ZoomRequestKind::Extents, "{line}");
        }
        // ALL with a filter after it is the scope word for the drawing: what it
        // matches, framed.
        t.ok("LINE 10,10 40,30");
        let filtered = t.ok("ZOOM ALL WHERE TYPE=line");
        assert!(
            filtered.starts_with("scope=drawing where=\"TYPE=line\" matched=1\n"),
            "{filtered}"
        );
        assert_eq!(t.last_zoom().kind, ZoomRequestKind::Scope);
        // Only the view verbs have an apostrophe form.
        let other = t.refused("'LINE 0,0 1,1");
        assert_eq!(other.message, "unknown command; type HELP");
    }

    #[test]
    fn a_factor_with_auto_cads_x_is_relative_to_the_view() {
        // AutoCAD's "2X" is twice the current view's scale: what a bare factor
        // is here.
        let mut t = ViewVerbsTest::new();
        let view = t.plan_at(10.0, 20.0, 4.0);
        t.ok("ZOOM 2X");
        assert_eq!(t.view(view).plan.scale, 8.0);
        t.ok("Z 0.5x");
        assert_eq!(t.view(view).plan.scale, 4.0);
        assert_eq!(t.view(view).plan.center, Point2::new(10.0, 20.0));
        // Relative to paper space means nothing in a plan view; no number, or a
        // factor not above 0, is refused as a bare one is.
        for line in ["ZOOM 2XP", "ZOOM X", "ZOOM 0X", "ZOOM -2X"] {
            assert_eq!(t.refused(line).code, ErrorCode::ParseFailure, "{line}");
        }
        assert_eq!(t.view(view).plan.scale, 4.0);
    }

    #[test]
    fn degenerate_zoom_and_views_lines_are_refused_naming_what_is_missing() {
        let mut t = ViewVerbsTest::new();
        // No view open at all.
        let none = t.refused("ZOOM");
        assert_eq!(none.code, ErrorCode::InvalidState);
        assert_eq!(
            none.message,
            "no view is open to zoom: VIEWS OPEN plan opens one"
        );

        t.plan();
        let centre = t.refused("ZOOM CENTRE");
        assert_eq!(
            centre.message,
            "ZOOM CENTRE takes the point to centre on: x,y"
        );
        assert_eq!(
            t.refused("ZOOM CENTRE 5").message,
            "ZOOM CENTRE takes the point to centre on: x,y"
        );
        assert_eq!(
            t.refused("VIEWS UNLINK").message,
            "name the views to unlink: VIEWS UNLINK <id>[,<id>...] | ALL"
        );
        assert_eq!(
            t.refused("VIEWS OPEN").message,
            "VIEWS OPEN takes what the view shows: plan, 3d, section or elevation"
        );
        assert_eq!(
            t.refused("VIEWS OPEN plan 3d").message,
            "VIEWS OPEN takes what the view shows: plan, 3d, section or elevation"
        );
        assert_eq!(
            t.refused("VIEWS OPEN map").message,
            "VIEWS OPEN takes plan, 3d, section or elevation, not 'map'"
        );
        assert_eq!(
            t.refused("VIEWS ACTIVATE").message,
            "VIEWS ACTIVATE takes one view id"
        );
        assert_eq!(
            t.refused("VIEWS ACTIVATE 1 2").message,
            "VIEWS ACTIVATE takes one view id"
        );
        assert!(
            t.zooms().is_empty(),
            "nothing refused was asked of the window"
        );
        assert_eq!(t.host.set.borrow().size(), 1, "nothing opened");
    }

    #[test]
    fn zoom_selection_of_one_point_centres_on_it_and_keeps_the_scale() {
        // A point has no extent to fit (ViewTransform::fit): the view centres on
        // it at the scale it had, and the scope says it took one.
        let mut t = ViewVerbsTest::new();
        let view = t.plan_at(10.0, 20.0, 4.0);
        t.ok("POINT 70,80");
        t.ok("SELECT ALL");
        let reply = t.ok("ZOOM SELECTION");
        assert!(
            reply.starts_with("scope=selection matched=1\nview=1 kind=plan centre=70,80 "),
            "{reply}"
        );
        assert_eq!(t.view(view).plan.center, Point2::new(70.0, 80.0));
        assert_eq!(t.view(view).plan.scale, 4.0);
    }

    #[test]
    fn zoom_centre_scale_sets_the_plan_view_exactly() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        // 300 x 200 at 8 px a unit about (50, 40): x 50 -+ 18.75, y 40 -+ 12.5.
        assert_eq!(
            t.ok("ZOOM CENTRE 50,40 SCALE 8 view=1"),
            "view=1 kind=plan centre=50,40 scale=8 area=31.25,27.5,68.75,52.5"
        );
        assert_eq!(t.view(1).plan.center, Point2::new(50.0, 40.0));
        assert_eq!(t.view(1).plan.scale, 8.0);
        // Without SCALE only the centre moves; view= may stand first.
        t.ok("ZOOM view=1 C -1.5,2.25");
        assert_eq!(t.view(1).plan.center, Point2::new(-1.5, 2.25));
        assert_eq!(t.view(1).plan.scale, 8.0);
    }

    #[test]
    fn zoom_in_and_out_are_by_two_about_the_centre_unless_a_factor_is_given() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan_at(10.0, 20.0, 4.0);
        t.ok("ZOOM IN");
        assert_eq!(t.view(view).plan.scale, 8.0);
        t.ok("ZOOM OUT 4");
        assert_eq!(t.view(view).plan.scale, 2.0);
        t.ok("ZOOM 0.5");
        assert_eq!(t.view(view).plan.scale, 1.0);
        // About the centre pixel, which is the centre exactly: it never moves.
        assert_eq!(t.view(view).plan.center, Point2::new(10.0, 20.0));

        for line in ["ZOOM IN 0", "ZOOM OUT -2", "ZOOM 0"] {
            assert_eq!(t.refused(line).code, ErrorCode::ParseFailure, "{line}");
        }
        assert_eq!(t.view(view).plan.scale, 1.0);
    }

    #[test]
    fn zoom_window_frames_the_box_as_zoom_extents_frames_the_drawing() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan_at(10.0, 20.0, 4.0);
        // The box 0,0 to 50,40 in 300 x 200 with 8% each side: 84% of 300 / 50 =
        // 5.04 and 84% of 200 / 40 = 4.2; the smaller fits, about (25, 20).
        t.ok("ZOOM WINDOW 50,40,0,0");
        let request = t.last_zoom();
        assert_eq!(request.kind, ZoomRequestKind::Window);
        assert_eq!(request.window.min, Point2::new(0.0, 0.0));
        assert_eq!(request.window.max, Point2::new(50.0, 40.0));
        assert_eq!(t.view(view).plan.center, Point2::new(25.0, 20.0));
        near(t.view(view).plan.scale, 4.2, 1e-12);
        // Two words are the same two corners.
        t.ok("ZOOM W 0,0 50,40");
        assert_eq!(t.last_zoom().window.max, Point2::new(50.0, 40.0));

        let same = t.refused("ZOOM WINDOW 5,5,5,5");
        assert_eq!(same.message, "a window needs two different corners");
        assert_eq!(t.refused("ZOOM WINDOW 1,2,3").code, ErrorCode::ParseFailure);
    }

    #[test]
    fn zoom_refuses_a_word_it_does_not_know_naming_it() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        // It once zoomed to the extents whatever followed ZOOM.
        let refusal = t.refused("ZOOM FOO BAR");
        assert_eq!(refusal.code, ErrorCode::ParseFailure);
        assert!(
            refusal
                .message
                .contains("ZOOM does not take 'FOO': ZOOM [EXTENTS | IN [f]"),
            "{}",
            refusal.message
        );
        assert!(t.refused("ZOOM IN 2 BAR").message.contains("'BAR'"));
        assert!(
            t.refused("ZOOM view=1 view=1")
                .message
                .contains("view= once")
        );
        assert!(
            t.zooms().is_empty(),
            "nothing refused was asked of the window"
        );
    }

    #[test]
    fn zoom_window_on_a3_d_view_is_refused_naming_the_kind() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        t.add(ViewKind::Model3D);
        let refusal = t.refused("ZOOM WINDOW 0,0,10,10 view=2");
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);
        assert!(
            refusal
                .message
                .contains("ZOOM WINDOW frames a plan view: view 2 is 3D"),
            "{}",
            refusal.message
        );
        let centre = t.refused("ZOOM CENTRE 1,2 view=2");
        assert!(
            centre
                .message
                .contains("ZOOM CENTRE centres a plan view: view 2 is 3D"),
            "{}",
            centre.message
        );
        // EXTENTS frames any view; IN, OUT and a factor zoom any view about its
        // middle - a 3D view as its wheel zooms there - and reach it as asked.
        t.ok("ZOOM EXTENTS view=2");
        t.ok("ZOOM IN view=2");
        t.ok("ZOOM 0.5 view=2");
        let zooms = t.zooms();
        assert_eq!(zooms.len(), 3);
        assert_eq!(zooms[1].kind, ZoomRequestKind::In);
        assert_eq!(zooms[1].factor, 2.0);
        assert_eq!(zooms[2].kind, ZoomRequestKind::Factor);
        assert_eq!(zooms[2].factor, 0.5);
        assert_eq!(zooms[2].view, 2);
    }

    #[test]
    fn zoom_of_a_view_that_is_not_open_is_refused_naming_it() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        let refusal = t.refused("ZOOM view=9");
        assert_eq!(refusal.code, ErrorCode::NotFound);
        assert_eq!(refusal.message, "no view 9 is open (VIEWS lists them)");
        assert_eq!(t.refused("ZOOM view=0").code, ErrorCode::ParseFailure);
        assert_eq!(t.refused("ZOOM view=").code, ErrorCode::ParseFailure);
    }

    #[test]
    fn zoom_scale_outside_the_limits_is_refused() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan();
        for line in [
            "ZOOM CENTRE 0,0 SCALE 1e8",
            "ZOOM CENTRE 0,0 SCALE 0",
            "ZOOM CENTRE 0,0 SCALE -1",
            "ZOOM CENTRE 0,0 SCALE",
        ] {
            let refusal = t.refused(line);
            assert_eq!(
                refusal.message, "SCALE is pixels per unit from 1e-07 to 1e+07",
                "{line}"
            );
        }
        assert_eq!(t.view(view).plan.center, Point2::new(10.0, 20.0));
        // At the limits themselves it is taken.
        t.ok("ZOOM CENTRE 0,0 SCALE 1e7");
        assert_eq!(t.view(view).plan.scale, 1e7);
    }

    #[test]
    fn views_and_zoom_are_refused_by_name_without_a_window() {
        // What katana_cli and katana_mcp have: an interpreter and no views.
        let mut headless = Session::default();
        for (line, verb) in [
            ("VIEWS", "VIEWS"),
            ("VIEWS LINK 1,2", "VIEWS"),
            ("ZOOM", "ZOOM"),
            ("Z", "ZOOM"),
            ("ZOOM IN view=1", "ZOOM"),
        ] {
            let refusal = headless.refused(line);
            assert_eq!(refusal.code, ErrorCode::Unsupported, "{line}");
            assert!(
                refusal
                    .message
                    .contains(&format!("{verb} is the desktop window's")),
                "{}",
                refusal.message
            );
            assert!(
                refusal.message.contains("katana --command"),
                "{}",
                refusal.message
            );
        }
        // A provider that answers null is no window either: here, no host.
        headless.interpreter.set_view_host(None);
        assert_eq!(headless.fails("VIEWS"), ErrorCode::Unsupported);
    }

    #[test]
    fn zoom_replies_with_the_view_then_every_view_that_followed() {
        let mut t = ViewVerbsTest::new();
        t.plan_at(10.0, 20.0, 4.0);
        t.plan_at(0.0, 0.0, 1.0);
        t.plan_at(3.0, 3.0, 3.0);
        t.ok("VIEWS LINK 1,2");
        // View 2 twice as close about its centre: 8 px a unit at (10, 20); view
        // 1 follows. View 3 is not linked and is not mentioned.
        let reply = t.ok("ZOOM IN view=2");
        let lines = lines_of(&reply);
        assert_eq!(lines.len(), 2, "{reply}");
        assert_eq!(
            lines[0],
            "view=2 kind=plan centre=10,20 scale=8 area=-8.75,7.5,28.75,32.5"
        );
        assert_eq!(
            lines[1],
            "view=1 kind=plan followed=2 centre=10,20 scale=8 area=-8.75,7.5,28.75,32.5"
        );
        assert_eq!(t.view(3).plan.scale, 3.0);
    }

    #[test]
    fn views_hide_show_isolate_change_only_that_view() {
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["design", "design/road", "asbuilt"]);
        let design = t.plan();
        let as_built = t.plan();

        // The design view hides the as-built layer, the as-built view the design
        // one - and with it design/road, beneath it.
        assert!(t.ok("VIEWS HIDE 1 asbuilt").ends_with(" hidden=asbuilt"));
        assert!(t.ok("VIEWS HIDE 2 design").ends_with(" hidden=design"));
        assert!(t.view(design).layers.hides("asbuilt"));
        assert!(!t.view(design).layers.hides("design/road"));
        assert!(t.view(as_built).layers.hides("design/road"));
        assert!(!t.view(as_built).layers.hides("asbuilt"));
        assert_eq!(t.setting_changes(), [1, 2], "each view redrawn, alone");

        // SHOW takes back exactly the entries named; a record with nothing
        // hidden says nothing about it.
        let shown = t.ok("VIEWS SHOW 1 asbuilt");
        assert!(!shown.contains("hidden="), "{shown}");
        assert!(t.view(design).layers.empty());

        // ISOLATE design/road: at each level of the path, the siblings of the
        // next segment - 0 and asbuilt at the root; design has no child but road.
        assert!(
            t.ok("VIEWS ISOLATE 1 design/road")
                .ends_with(" hidden=0,asbuilt")
        );
        assert!(
            t.ok("VIEWS SHOW 1 ALL")
                .ends_with("area=-27.5,-5,47.5,45 ghosts=on")
        );
        assert!(t.view(design).layers.empty());
        // The as-built view was never touched by any of that.
        assert_eq!(t.view(as_built).layers.hidden().len(), 1);

        // Two layers in one list, and in two words.
        t.ok("VIEWS HIDE 1 design,asbuilt 0");
        assert_eq!(t.view(design).layers.size(), 3);
        // VIEWS lists the hidden layers in every record.
        let listed = t.ok("VIEWS");
        assert!(listed.contains("hidden=0,asbuilt,design\n"), "{listed}");
    }

    #[test]
    fn hiding_a_layer_the_drawing_lacks_is_refused_naming_it() {
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["design", "design/road"]);
        let view = t.plan();
        let missing = t.refused("VIEWS HIDE 1 design,roads");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(
            missing.message,
            "no layer 'roads' in the drawing (LAYER LIST lists them)"
        );
        // Refused whole: design, which exists, was not hidden either.
        assert!(t.view(view).layers.empty());
        assert!(t.setting_changes().is_empty());
        assert_eq!(t.refused("VIEWS ISOLATE 1 roads").code, ErrorCode::NotFound);
        assert_eq!(t.refused("VIEWS SHOW 1 roads").code, ErrorCode::NotFound);
        assert!(
            t.refused("VIEWS ISOLATE 1 design design/road")
                .message
                .contains("ISOLATE takes one layer")
        );
        assert_eq!(t.refused("VIEWS HIDE 9 design").code, ErrorCode::NotFound);
        assert!(
            t.refused("VIEWS HIDE 1")
                .message
                .contains("takes a view id and the layers")
        );
        // A node of the tree with no layer of its own is a layer to hide: what
        // the Layers popup offers as "design" when only design/road existed.
        add_layers(&mut t.document, &["survey/points"]);
        assert!(t.ok("VIEWS HIDE 1 survey").ends_with(" hidden=survey"));
    }

    #[test]
    fn showing_a_layer_beneath_one_the_view_hides_is_refused_naming_what_holds_it() {
        // design hidden hides design/road with it (LayerOverrides): SHOW of the
        // road alone would leave it hidden and answer with the same record, as
        // if it had done something.
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["design", "design/road", "asbuilt"]);
        let view = t.plan();
        t.ok("VIEWS HIDE 1 design,asbuilt");
        t.host.setting_changes.borrow_mut().clear();

        let held = t.refused("VIEWS SHOW 1 asbuilt,design/road");
        assert_eq!(held.code, ErrorCode::CommandRejected);
        assert_eq!(
            held.message,
            "view 1 hides 'design', which holds 'design/road': show 'design' (VIEWS SHOW 1 \
             design)"
        );
        // Read whole before any of it is shown: asbuilt, before it, is hidden still.
        assert!(t.view(view).layers.hides_directly("asbuilt"));
        assert!(t.setting_changes().is_empty());

        // With what holds it on the same line, both are shown.
        assert!(
            t.ok("VIEWS SHOW 1 design/road,design")
                .ends_with(" hidden=asbuilt")
        );
        assert!(!t.view(view).layers.hides("design/road"));
    }

    #[test]
    fn views_set_ghosts_off_turns_that_views_ghosts_off_and_on_again() {
        let mut t = ViewVerbsTest::new();
        let design = t.plan();
        let as_built = t.plan();
        assert!(
            t.view(design).selection_ghosts,
            "a view ghosts the selection when it opens"
        );

        let off = t.ok("VIEWS SET 2 ghosts=off");
        assert!(off.starts_with("view=2 kind=plan "), "{off}");
        assert!(off.contains(" ghosts=off"), "{off}");
        assert!(!t.view(as_built).selection_ghosts);
        assert!(t.view(design).selection_ghosts, "only the view named");
        assert_eq!(t.setting_changes(), [2], "that view redrawn, alone");

        // The words as a person types them.
        assert!(t.ok("views set 2 GHOSTS=On").contains(" ghosts=on"));
        assert!(t.view(as_built).selection_ghosts);
        // VIEWS says so of every view.
        assert!(t.ok("VIEWS SET 1 ghosts=off").contains(" ghosts=off"));
        let listing = t.ok("VIEWS");
        let lines = lines_of(&listing);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with(" ghosts=off"), "{}", lines[0]);
        assert!(lines[1].ends_with(" ghosts=on"), "{}", lines[1]);
    }

    #[test]
    fn views_set_refuses_what_it_does_not_take_and_changes_nothing() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan();
        assert_eq!(
            t.refused("VIEWS SET 9 ghosts=off").code,
            ErrorCode::NotFound
        );
        let bare = t.refused("VIEWS SET 1");
        assert!(
            bare.message.contains("VIEWS SET <id> ghosts=on|off"),
            "{}",
            bare.message
        );
        let value = t.refused("VIEWS SET 1 ghosts=maybe");
        assert_eq!(value.message, "ghosts= is on or off, not 'maybe'");
        let key = t.refused("VIEWS SET 1 grid=on");
        assert!(
            key.message.contains("does not take 'grid=on'"),
            "{}",
            key.message
        );
        assert!(
            t.refused("VIEWS SET 1 ghosts")
                .message
                .contains("does not take 'ghosts'")
        );
        // Read whole before any of it is applied: the good word before the bad
        // one changed nothing.
        assert_eq!(
            t.refused("VIEWS SET 1 ghosts=off ghosts=maybe").code,
            ErrorCode::ParseFailure
        );
        assert!(t.view(view).selection_ghosts);
        assert!(t.setting_changes().is_empty());
    }

    // ---- the Layers popup's lines -------------------------------------------------------
    //
    // The popup does nothing itself: each box and button hands one of these lines to the
    // command runner (docs/desktop.md, "Per-view layers"). A layer's name may hold a comma,
    // which a comma list reads as the break between two layers, so LAYER takes every word
    // after it whole; and the reference layers - an imported raster or point cloud, which
    // cad can only hold the id of - have their own word.

    #[test]
    fn layer_takes_every_word_after_it_whole_so_a_layer_whose_name_holds_a_comma_can_be_named() {
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["a,b", "a", "b", "x y"]);
        let view = t.plan();

        // Without LAYER, "a,b" is two layers: a and b, never the layer called a,b.
        t.ok("VIEWS HIDE 1 a,b");
        assert_eq!(*t.view(view).layers.hidden(), names(&["a", "b"]));
        t.ok("VIEWS SHOW 1 ALL");

        // With it, the one layer: neither of its parts.
        t.ok("VIEWS HIDE 1 LAYER a,b");
        assert_eq!(*t.view(view).layers.hidden(), names(&["a,b"]));
        assert!(!t.view(view).layers.hides_directly("a"));
        t.ok("VIEWS SHOW 1 LAYER a,b");
        assert!(t.view(view).layers.empty());

        // Several words, each one layer, a quoted one with a blank in it as well.
        t.ok("VIEWS HIDE 1 LAYER a,b \"x y\" a");
        assert_eq!(*t.view(view).layers.hidden(), names(&["a", "a,b", "x y"]));
        t.ok("VIEWS SHOW 1 ALL");

        // ISOLATE takes it too: the root's other layers are what it hides - the default
        // layer 0, a, b and "x y".
        t.ok("VIEWS ISOLATE 1 LAYER a,b");
        assert_eq!(
            *t.view(view).layers.hidden(),
            names(&["0", "a", "b", "x y"])
        );
    }

    #[test]
    fn layer_that_names_no_layer_or_two_to_isolate_is_refused_and_changes_nothing() {
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["a,b", "a"]);
        let view = t.plan();
        // A name the drawing lacks, whole: the comma is not split to find a and b.
        let missing = t.refused("VIEWS HIDE 1 LAYER a,c");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(
            missing.message,
            "no layer 'a,c' in the drawing (LAYER LIST lists them)"
        );
        assert_eq!(
            t.refused("VIEWS ISOLATE 1 LAYER a,b a").code,
            ErrorCode::ParseFailure
        );
        assert!(t.view(view).layers.empty());
        assert!(t.setting_changes().is_empty());
    }

    #[test]
    fn a_line_whose_only_word_is_layer_or_reference_names_the_layer_called_that() {
        // The keywords need a word after them: alone, each is a layer's name as it was.
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["LAYER", "REFERENCE"]);
        let view = t.plan();
        t.ok("VIEWS HIDE 1 LAYER");
        assert!(t.view(view).layers.hides_directly("LAYER"));
        t.ok("VIEWS HIDE 1 REFERENCE");
        assert!(t.view(view).layers.hides_directly("REFERENCE"));
        assert!(t.view(view).hidden_references.is_empty());
    }

    #[test]
    fn reference_hides_and_shows_the_reference_layers_a_view_hides_of_its_own() {
        let mut t = ViewVerbsTest::new();
        *t.host.references.borrow_mut() = BTreeSet::from([7, 9, 12]);
        add_layers(&mut t.document, &["asbuilt"]);
        let view = t.plan();
        let other = t.plan();

        assert!(
            t.ok("VIEWS HIDE 1 REFERENCE 7")
                .ends_with(" ghosts=on hiddenrefs=7")
        );
        assert_eq!(t.view(view).hidden_references, BTreeSet::from([7]));
        // In one word or several, each once, as the layers are.
        assert!(
            t.ok("VIEWS HIDE 1 REFERENCE 12,9 9 7")
                .ends_with(" hiddenrefs=7,9,12")
        );
        assert_eq!(t.view(view).hidden_references, BTreeSet::from([7, 9, 12]));
        assert!(
            t.view(other).hidden_references.is_empty(),
            "that view alone"
        );
        assert_eq!(t.setting_changes(), [1, 1]);

        // Beside the layers it hides, in the same record.
        t.ok("VIEWS HIDE 1 asbuilt");
        assert!(
            t.ok("VIEWS SHOW 1 REFERENCE 9")
                .ends_with(" hidden=asbuilt hiddenrefs=7,12")
        );
        assert_eq!(t.view(view).hidden_references, BTreeSet::from([7, 12]));

        // SHOW ALL shows everything the document shows: the layers and the references.
        let everything = t.ok("VIEWS SHOW 1 ALL");
        assert!(!everything.contains("hidden"), "{everything}");
        assert!(t.view(view).layers.empty());
        assert!(t.view(view).hidden_references.is_empty());
    }

    #[test]
    fn a_reference_the_window_lacks_or_a_number_that_is_none_is_refused_whole() {
        let mut t = ViewVerbsTest::new();
        *t.host.references.borrow_mut() = BTreeSet::from([7]);
        let view = t.plan();
        // Read whole before any of it is applied: 7, which exists, is not hidden either.
        let missing = t.refused("VIEWS HIDE 1 REFERENCE 7 8");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(
            missing.message.contains("no reference layer 8"),
            "{}",
            missing.message
        );
        assert!(t.view(view).hidden_references.is_empty());
        assert!(t.setting_changes().is_empty());

        assert_eq!(
            t.refused("VIEWS HIDE 1 REFERENCE seven").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.refused("VIEWS HIDE 1 REFERENCE 0").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.refused("VIEWS HIDE 1 REFERENCE -7").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.refused("VIEWS HIDE 1 REFERENCE ,").code,
            ErrorCode::ParseFailure,
            "commas alone name no reference"
        );
        assert_eq!(
            t.refused("VIEWS ISOLATE 1 REFERENCE 7").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.refused("VIEWS HIDE 9 REFERENCE 7").code,
            ErrorCode::NotFound,
            "no such view"
        );
        assert!(t.view(view).hidden_references.is_empty());
    }

    #[test]
    fn a_driver_that_has_no_reference_data_refuses_every_reference() {
        // katana_cli and a plain test host answer no by default: nothing there to hide.
        let mut t = ViewVerbsTest::new();
        let view = t.plan();
        let none = t.refused("VIEWS HIDE 1 REFERENCE 1");
        assert_eq!(none.code, ErrorCode::NotFound);
        assert!(t.view(view).hidden_references.is_empty());
    }

    // ---- ZOOM on the shared scope -------------------------------------------------------
    //
    // The line (10,10)-(40,30) fills a 300 x 200 view at 84% of either side:
    // 0.84 x 300 / 30 = 8.4 = 0.84 x 200 / 20, about its middle, (25, 20).

    #[test]
    fn zoom_selection_frames_the_selected_entities_and_says_how_many_matched() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan();
        t.ok("LINE 10,10 40,30");
        t.ok("LINE 100,100 110,120");
        t.ok("SELECT 1");
        let reply = t.ok("ZOOM SELECTION view=1");
        assert!(
            reply.starts_with("scope=selection matched=1\nview=1 kind=plan centre=25,20 "),
            "{reply}"
        );
        assert_eq!(t.view(view).plan.center, Point2::new(25.0, 20.0));
        near(t.view(view).plan.scale, 8.4, 1e-12);
        assert!(!t.zooms().is_empty());
        let request = t.last_zoom();
        assert_eq!(request.kind, ZoomRequestKind::Scope);
        assert_eq!(request.window.min, Point2::new(10.0, 10.0));
        assert_eq!(request.window.max, Point2::new(40.0, 30.0));
    }

    #[test]
    fn a_scope_that_matches_nothing_moves_no_view_and_says_so() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan();
        t.ok("LINE 10,10 40,30");
        // Nothing selected: an answer, not a refusal, and no view moves.
        assert_eq!(t.ok("ZOOM SELECTION"), "scope=selection matched=0");
        assert_eq!(
            t.ok("ZOOM LAYERS 0 WHERE TYPE=circle"),
            "scope=layers layers=0 sublayers=yes where=\"TYPE=circle\" matched=0"
        );
        assert!(t.zooms().is_empty());
        assert_eq!(t.view(view).plan.center, Point2::new(10.0, 20.0));
        // A scope the drawing cannot answer is refused as every verb refuses it.
        assert_eq!(t.refused("ZOOM LAYERS nosuch").code, ErrorCode::NotFound);
    }

    // ---- what a view shows of what a scope took -----------------------------------------
    //
    // The design line (10,10)-(40,30) on "design" and the as-built line
    // (100,100)-(110,120) on "asbuilt". A view frames what it shows of what a
    // scope took (view_verbs.rs): the design line alone is framed about its
    // middle (25, 20) - at 8.4 px a unit in the 300 x 200 view, as above - and
    // the two lines together are the box (10,10)-(110,120).

    #[test]
    fn a_plan_view_frames_what_it_shows_of_the_selection_and_says_how_many_that_is() {
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["design", "asbuilt"]);
        let view = t.plan();
        t.ok("LAYER SET design");
        t.ok("LINE 10,10 40,30");
        t.ok("LAYER SET asbuilt");
        t.ok("LINE 100,100 110,120");
        let ids: Vec<EntityId> = t.document.model().entities.ids();
        assert_eq!(ids.len(), 2);
        t.ok("SELECT ALL");
        assert_eq!(t.document.selection().size(), 2);

        // The view hides the as-built layer with its ghosts off: it shows the
        // design line alone, and frames that.
        t.ok("VIEWS HIDE 1 asbuilt");
        t.ok("VIEWS SET 1 ghosts=off");
        let one = t.ok("ZOOM SELECTION view=1");
        assert!(
            one.starts_with("scope=selection matched=2\nview=1 kind=plan centre=25,20 "),
            "{one}"
        );
        assert!(one.ends_with(" shown=1"), "{one}");
        assert_eq!(t.zooms().len(), 1);
        let request = t.last_zoom();
        assert_eq!(request.ids, [ids[0]]);
        assert_eq!(request.window.min, Point2::new(10.0, 10.0));
        assert_eq!(request.window.max, Point2::new(40.0, 30.0));
        assert_eq!(t.view(view).plan.center, Point2::new(25.0, 20.0));

        // Its ghosts on, it shows the as-built line too, faintly, and frames both.
        t.ok("VIEWS SET 1 ghosts=on");
        let both = t.ok("ZOOM SELECTION view=1");
        assert!(both.ends_with(" shown=2"), "{both}");
        let request = t.last_zoom();
        assert_eq!(request.ids, ids);
        assert_eq!(request.window.min, Point2::new(10.0, 10.0));
        assert_eq!(request.window.max, Point2::new(110.0, 120.0));
        assert_eq!(t.view(view).plan.center, Point2::new(60.0, 65.0));
    }

    #[test]
    fn a_view_that_shows_none_of_what_the_scope_took_moves_nothing_and_says_so() {
        // It framed the box of everything the scope took and jumped - and took
        // its link - to empty ground, where nothing selected was drawn or
        // ghosted, while its tip promised nothing would move.
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["design"]);
        let view = t.plan();
        t.ok("LAYER SET design");
        t.ok("LINE 10,10 40,30");
        t.ok("LAYER SET 0");
        t.ok("SELECT ALL");
        assert_eq!(t.document.selection().size(), 1);
        let none = "scope=selection matched=1\nview=1 kind=plan shown=0 moved=no";

        // The view hides the layer, its ghosts off.
        t.ok("VIEWS HIDE 1 design");
        t.ok("VIEWS SET 1 ghosts=off");
        assert_eq!(t.ok("ZOOM SELECTION view=1"), none);

        // The drawing hides it: hidden in every view, a ghost or not.
        t.ok("VIEWS SHOW 1 ALL");
        t.ok("VIEWS SET 1 ghosts=on");
        t.ok("LAYER HIDE design");
        assert_eq!(t.ok("ZOOM SELECTION view=1"), none);

        // A ghost is of the selection alone: the design layer's line, taken by
        // LAYERS and not selected, is shown nowhere in a view that hides it.
        t.ok("LAYER SHOW design");
        t.ok("VIEWS HIDE 1 design");
        t.ok("SELECT NONE");
        assert_eq!(
            t.ok("ZOOM LAYERS design view=1"),
            "scope=layers layers=design sublayers=yes matched=1\n\
             view=1 kind=plan shown=0 moved=no"
        );

        assert!(t.zooms().is_empty(), "nothing was asked of the window");
        assert_eq!(t.view(view).plan.center, Point2::new(10.0, 20.0));
        assert_eq!(t.view(view).plan.scale, 4.0);
    }

    #[test]
    fn a_three_d_view_and_a_plan_view_frame_the_selection_by_one_rule() {
        // The same rule for a 3D view, whose widget frames where its scene puts
        // what it is handed: nothing it does not show is handed to it. And the
        // one difference of kind: a plan view ghosts no label (the plan painter
        // places labels among the others), where a 3D view marks where a
        // ghosted one attaches.
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["design"]);
        t.plan();
        t.add(ViewKind::Model3D);
        t.ok("LAYER SET design");
        t.ok("LINE 10,10 40,30");
        let line = last_id(&t.document);
        let style = LabelStyle {
            name: String::from("any"),
            ..LabelStyle::default()
        };
        must(&mut t.document, create_label_style(style));
        let label = Entity {
            layer: String::from("design"),
            geometry: Geometry::Label(LabelGeometry {
                target: line,
                style: String::from("any"),
                anchor: Point2::new(25.0, 20.0),
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        };
        must(&mut t.document, create_entities(vec![label]));
        let label_id = last_id(&t.document);
        assert_ne!(label_id, line);
        t.ok("VIEWS HIDE 1 design");
        t.ok("VIEWS HIDE 2 design");

        // Ghosts off: neither view shows the selected line, and neither moves.
        t.ok("VIEWS SET 2 ghosts=off");
        t.ok(&format!("SELECT {line}"));
        assert_eq!(
            t.ok("ZOOM SELECTION view=2"),
            "scope=selection matched=1\nview=2 kind=3d shown=0 moved=no"
        );
        assert!(t.zooms().is_empty());

        // Ghosts on, the 3D view is handed the line it ghosts.
        t.ok("VIEWS SET 2 ghosts=on");
        t.ok("ZOOM SELECTION view=2");
        assert_eq!(t.zooms().len(), 1);
        assert_eq!(t.last_zoom().ids, [line]);

        // The label alone, selected, on the layer both views hide with their
        // ghosts on: the 3D view shows it, the plan view does not.
        t.ok(&format!("SELECT {label_id}"));
        t.ok("ZOOM SELECTION view=2");
        assert_eq!(t.zooms().len(), 2);
        assert_eq!(t.last_zoom().ids, [label_id]);
        assert_eq!(
            t.ok("ZOOM SELECTION view=1"),
            "scope=selection matched=1\nview=1 kind=plan shown=0 moved=no"
        );
        assert_eq!(t.zooms().len(), 2);
    }

    #[test]
    fn zoom_with_no_words_is_still_extents_not_the_selection() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        t.ok("LINE 10,10 40,30");
        t.ok("SELECT ALL");
        t.ok("ZOOM");
        assert_eq!(t.zooms().len(), 1);
        assert_eq!(t.last_zoom().kind, ZoomRequestKind::Extents);
    }

    #[test]
    fn zoom_area_frames_what_is_in_the_box_not_the_box() {
        let mut t = ViewVerbsTest::new();
        let view = t.plan();
        t.ok("LINE 10,10 40,30");
        t.ok("LINE 100,100 110,120");
        // AREA takes the entities in the box: the first line, framed about its
        // own middle (25, 20), not the box's (25, 25). WINDOW frames the box.
        let reply = t.ok("ZOOM AREA 0,0,50,50");
        assert!(
            reply.starts_with("scope=area area=0,0,50,50 matched=1\n"),
            "{reply}"
        );
        assert_eq!(t.view(view).plan.center, Point2::new(25.0, 20.0));
        t.ok("ZOOM WINDOW 0,0,50,50");
        assert_eq!(t.view(view).plan.center, Point2::new(25.0, 25.0));
    }

    #[test]
    fn view_equals_after_where_is_not_taken_for_a_condition() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        t.plan();
        done(t.host.set.borrow_mut().activate(2));
        t.ok("LINE 10,10 40,30");
        t.ok("CIRCLE 5,5 1");
        // view= stands after the filter, where a word with '=' would be a
        // condition, and is taken off before the scope is read.
        let reply = t.ok("ZOOM DRAWING WHERE TYPE=line view=1");
        assert!(
            reply.starts_with("scope=drawing where=\"TYPE=line\" matched=1\nview=1 "),
            "{reply}"
        );
        assert_eq!(t.last_zoom().view, 1);
        assert_eq!(t.view(1).plan.center, Point2::new(25.0, 20.0));
    }

    #[test]
    fn zoom_on_a_section_takes_extents_in_and_out_only() {
        let mut t = ViewVerbsTest::new();
        t.plan();
        t.add(ViewKind::Section);
        t.ok("ZOOM IN view=2");
        assert_eq!(t.last_zoom().kind, ZoomRequestKind::In);
        t.ok("ZOOM OUT 3 view=2");
        assert_eq!(t.last_zoom().factor, 3.0);
        t.ok("ZOOM EXTENTS view=2");
        // Each refusal names the kinds that take it (zoom_takes): WINDOW and
        // CENTRE a plan view's, a scope a plan, 3D or elevation view's.
        for (line, takers) in [
            (
                "ZOOM WINDOW 0,0,1,1 view=2",
                "a plan view: view 2 is Section",
            ),
            ("ZOOM CENTRE 0,0 view=2", "a plan view: view 2 is Section"),
            (
                "ZOOM SELECTION view=2",
                "a plan, 3D or elevation view: view 2 is Section",
            ),
        ] {
            let refusal = t.refused(line);
            assert_eq!(refusal.code, ErrorCode::InvalidArgument, "{line}");
            assert!(refusal.message.contains(takers), "{}", refusal.message);
        }
        assert_eq!(t.zooms().len(), 3);
    }

    #[test]
    fn a_scope_zoom_on_a3_d_view_carries_what_the_scope_took_and_a_section_refuses_it() {
        // A 3D view frames the entities a scope took, where its scene draws
        // them, so the request carries them - ascending, as match_scope gives
        // them - beside their plan extent, which a plan view frames.
        let mut t = ViewVerbsTest::new();
        t.plan();
        t.add(ViewKind::Model3D);
        t.add(ViewKind::Section);
        t.ok("LINE 0,0 10,0");
        t.ok("LINE 50,50 60,60");
        t.ok("LINE 100,0 110,0");
        let ids: Vec<EntityId> = t.document.model().entities.ids();
        assert_eq!(ids.len(), 3);
        t.document.selection_mut().set(vec![ids[2], ids[0]]);

        t.ok("ZOOM SELECTION view=2");
        assert_eq!(t.zooms().len(), 1);
        let request = t.last_zoom();
        assert_eq!(request.kind, ZoomRequestKind::Scope);
        assert_eq!(request.ids, [ids[0], ids[2]]);
        // Their plan extent by hand: x from 0 to 110, both on y = 0.
        assert_eq!(request.window.min, Point2::new(0.0, 0.0));
        assert_eq!(request.window.max, Point2::new(110.0, 0.0));

        // A section frames no scope, and says which kinds do.
        let refusal = t.refused("ZOOM SELECTION view=3");
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);
        assert!(
            refusal
                .message
                .contains("ZOOM on a scope frames a plan, 3D or elevation view: view 3 is Section"),
            "{}",
            refusal.message
        );
        assert_eq!(
            t.zooms().len(),
            1,
            "nothing refused was asked of the window"
        );
    }

    #[test]
    fn the_record_of_a_view_that_has_framed_nothing_yet_says_so() {
        // A view frames what it draws at its first paint; until then its place
        // is the one every new view starts at, which VIEWS OPEN's record gave as
        // though the view looked there (view_verbs.rs, framed=no).
        let mut t = ViewVerbsTest::new();
        let opened = t.ok("VIEWS OPEN plan");
        assert!(opened.contains(" framed=no ghosts=on"), "{opened}");
        let model = t.ok("VIEWS OPEN 3d");
        assert!(model.contains(" framed=no ghosts=on"), "{model}");

        // Framed, a record says nothing of it.
        {
            let mut set = t.host.set.borrow_mut();
            match set.find_mut(1) {
                Some(view) => view.plan_framed = true,
                None => panic!("view 1 is open"),
            }
            match set.find_mut(2) {
                Some(view) => view.camera_framed = true,
                None => panic!("view 2 is open"),
            }
        }
        let listed = t.ok("VIEWS");
        assert!(!listed.contains("framed="), "{listed}");
        // A section's record has no place to be the placeholder of.
        assert!(!t.ok("VIEWS OPEN section").contains("framed="));
    }

    #[test]
    fn hide_or_show_of_commas_alone_is_refused_and_changes_nothing() {
        // "VIEWS HIDE 1 ," names no layer. It replied the view's record as
        // though it had worked, and changed nothing: a silent failure.
        let mut t = ViewVerbsTest::new();
        add_layers(&mut t.document, &["design"]);
        t.plan();
        t.ok("VIEWS HIDE 1 design");
        let heard = t.setting_changes().len();
        for line in [
            "VIEWS HIDE 1 ,",
            "VIEWS SHOW 1 ,,",
            "VIEWS HIDE 1 , ,",
            "VIEWS ISOLATE 1 ,",
        ] {
            let refusal = t.refused(line);
            assert_eq!(refusal.code, ErrorCode::ParseFailure, "{line}");
            assert!(
                refusal.message.contains("names no layer in ','")
                    || refusal.message.contains("names no layer in ',,'"),
                "{line}: {}",
                refusal.message
            );
        }
        assert_eq!(
            t.setting_changes().len(),
            heard,
            "nothing changed, so no view was redrawn"
        );
        assert!(
            t.view(1).layers.hides("design"),
            "and design is hidden still"
        );
    }
}

// The last entity of the drawing, by id: the C++ `entities.ids().back()`.
fn last_id(document: &Document) -> EntityId {
    match document.model().entities.ids().last() {
        Some(&id) => id,
        None => panic!("the drawing is empty"),
    }
}

mod view_verbs {
    use super::*;

    #[test]
    fn headless_paper_scale_names_an_existing_saved_view_rather_than_inventing_one() {
        let mut session = Session::default();
        let missing = session.refused("VIEWS SCALE 1 500");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(missing.message.contains("saved plan view"));
        assert_eq!(session.ok("VIEWS SCALE LIST"), "views=0");
        done(set_view_paper_scales(
            &mut session.document,
            &BTreeMap::from([(7, Some(250.0))]),
        ));
        assert_eq!(
            session.ok("VIEWS SCALE LIST"),
            "view=7 kind=plan paper_scale=250\nviews=1"
        );
        assert_eq!(
            session.ok("VIEWS SCALE 7 1:500"),
            "view=7 kind=plan paper_scale=500 changed=yes"
        );
        assert_eq!(
            session.ok("VIEWS SCALE 7 AUTO"),
            "view=7 kind=plan paper_scale=auto changed=yes"
        );
        session.ok("UNDO");
        assert_eq!(
            session.ok("VIEWS SCALE 7"),
            "view=7 kind=plan paper_scale=500"
        );
        assert!(session.run("VIEWS SCALE 8 500").is_err());
        assert!(session.run("VIEWS SCALE").is_err());
    }
}

mod view_zoom {
    use super::*;

    #[test]
    fn the_kinds_each_zoom_takes_are_the_verbs_one_rule() {
        // The table the bars, the View menu and the Zoom To dialog read
        // (zoom_takes), as view_verbs.rs states it: EXTENTS, IN, OUT and a
        // factor every kind, about its middle; a scope every kind but a section,
        // which shows where entities cross it and has no box of them to frame;
        // WINDOW and CENTRE a plan view, whose plan position they are.
        for kind in [
            ViewKind::Plan,
            ViewKind::Model3D,
            ViewKind::Section,
            ViewKind::Elevation,
        ] {
            let plan = kind == ViewKind::Plan;
            assert!(zoom_takes(kind, ZoomRequestKind::Extents));
            assert!(zoom_takes(kind, ZoomRequestKind::In));
            assert!(zoom_takes(kind, ZoomRequestKind::Out));
            assert!(zoom_takes(kind, ZoomRequestKind::Factor));
            assert_eq!(
                zoom_takes(kind, ZoomRequestKind::Scope),
                kind != ViewKind::Section
            );
            assert_eq!(zoom_takes(kind, ZoomRequestKind::Window), plan);
            assert_eq!(zoom_takes(kind, ZoomRequestKind::Centre), plan);
        }
    }

    #[test]
    fn the_extent_of_ids_is_their_box_passing_over_ids_that_name_nothing() {
        let mut session = Session::default();
        session.ok("LINE 10,10 40,30");
        session.ok("CIRCLE 100,50 5");
        // The line and the circle's own box (95,45)-(105,55); id 7 names nothing.
        let ids: [EntityId; 3] = [1, 2, 7];
        let model = session.document.model();
        let extent: Box2 = extent_of(model, &ids);
        assert_eq!(extent.min, Point2::new(10.0, 10.0));
        assert_eq!(extent.max, Point2::new(105.0, 55.0));
        assert!(extent_of(model, &[7]).empty());
        assert!(extent_of(model, &[]).empty());
    }

    #[test]
    fn apply_plan_zoom_leaves_window_and_extents_to_the_widget() {
        let mut view = ViewTransform::default();
        view.resize(300.0, 200.0);
        view.center = Point2::new(10.0, 20.0);
        view.scale = 4.0;
        let mut request = ZoomRequest {
            kind: ZoomRequestKind::Extents,
            ..ZoomRequest::default()
        };
        assert!(!apply_plan_zoom(&mut view, &request));
        request.kind = ZoomRequestKind::Window;
        request.window = Box2::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
        assert!(!apply_plan_zoom(&mut view, &request));
        assert_eq!(view.center, Point2::new(10.0, 20.0));
        assert_eq!(view.scale, 4.0);

        // IN clamps as the wheel does: at the largest scale it stays there.
        view.scale = ViewTransform::MAXIMUM_SCALE;
        request.kind = ZoomRequestKind::In;
        request.factor = 2.0;
        assert!(apply_plan_zoom(&mut view, &request));
        assert_eq!(view.scale, ViewTransform::MAXIMUM_SCALE);
    }
}

mod command_verb {
    use super::*;

    #[test]
    fn the_verb_of_a_line_is_the_interpreters_own_reading() {
        // What the window asks of a line typed inside a tool (verb_of): the
        // interpreter's alias table and its apostrophe rule, never a list of the
        // window's own.
        assert_eq!(CommandInterpreter::verb_of("z"), "ZOOM");
        assert_eq!(CommandInterpreter::verb_of("  Zoom in 2  "), "ZOOM");
        assert_eq!(CommandInterpreter::verb_of("'z e"), "ZOOM");
        assert_eq!(CommandInterpreter::verb_of("'ZOOM"), "ZOOM");
        assert_eq!(CommandInterpreter::verb_of("views open plan"), "VIEWS");
        assert_eq!(CommandInterpreter::verb_of("l 0,0 1,1"), "LINE");
        // An apostrophe makes no other word a verb; run() refuses it.
        assert_eq!(CommandInterpreter::verb_of("'LINE 0,0"), "'LINE");
        assert_eq!(CommandInterpreter::verb_of("'"), "'");
        assert_eq!(CommandInterpreter::verb_of(""), "");
        assert_eq!(CommandInterpreter::verb_of("   "), "");
    }
}
