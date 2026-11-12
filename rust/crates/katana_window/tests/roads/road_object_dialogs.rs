//! The twins of tests/qt_widgets/bim/test_road_object_dialogs.cpp: BIM > Road Objects, driven by
//! object names as a person drives it by clicking and typing.
//!
//! The dialogs change nothing themselves: they write the ROADOBJECT line a person would type and
//! hand it to the window's executor. So what is tested is each line - exactly the one a person
//! would type - that the drawing then holds what the line says, and that what the dialogs show is
//! what the drawing holds. The executor here runs the line through the real CommandInterpreter on
//! the same drawing, which is where the window's command line sends a ROADOBJECT line.
//!
//! The site: alignment Main, straight from (0,0) to (100,0), its design profile level at 10.

use std::collections::BTreeSet;

use katana_cad::Document;
use katana_cad::road::road_object_mesh::is_closed_mesh;
use katana_cad::road::road_object_verbs::road_object_verb_help;
use katana_cad::road::road_objects::{RoadObjectKind, road_object_kinds};
use katana_geometry::Point2;
use katana_window::bim::road_dialog_support::mnemonic;
use katana_window::bim::road_object_dialog::{
    Place, RoadObjectDialog, RoadObjectForm, road_object_add_line, road_object_preview,
};
use katana_window::bim::road_object_manager::{
    RoadObjectManagerDialog, road_objects_frame, scope_control_tip,
};
use katana_window::bim::road_object_workbench::{
    KIND_ACTIONS, MANAGER_ACTION, RoadObjectWorkbench,
};
use katana_window::canvas::{
    Brush, BrushStyle, Canvas, Color, Elide, Font, FontMetrics, FontSize, Image, IntRect, Path,
    PathElement, Pen, PointF, RectF, RenderHint, SizeF, TextFlags, Transform,
};
use katana_window::drive::{ControlKind, Harness, MenuItem, Part, Snapshot, WorkbenchHost};
use katana_window::host::RecordingHost;

use crate::driver::{Driven, ok};
use crate::window_support::interpreter_host;

/// The site's host: the interpreter on a drawing holding alignment Main.
fn site() -> RecordingHost {
    let mut host = interpreter_host(Document::new(None));
    for line in ["ALIGN NEW Main 0,0 100,0", "ALIGN DESIGN Main 0,10 100,10"] {
        ok(&mut host, line);
    }
    host
}

fn placing(host: RecordingHost) -> Driven<RoadObjectDialog> {
    let dialog = RoadObjectDialog::new(host.document());
    Driven::new(host, dialog, "roadObjectDialog")
}

fn on_kind(kind: RoadObjectKind) -> Driven<RoadObjectDialog> {
    let mut d = placing(site());
    d.dialog.show_kind(kind);
    d.settle();
    d
}

fn manager(mut host: RecordingHost) -> Driven<RoadObjectManagerDialog> {
    let dialog = RoadObjectManagerDialog::new(host.document_mut());
    Driven::new(host, dialog, "roadObjectManagerDialog")
}

/// Chooses the manager's row of `id` alone, as manager.selectObject did, and lets the frame run.
#[track_caller]
fn select_object(d: &mut Driven<RoadObjectManagerDialog>, id: &str) {
    let Driven { dialog, host, .. } = &mut *d;
    assert!(dialog.select_object(host.document_mut(), id), "no {id}");
    d.settle();
}

/// The mnemonics of every button, check box and radio button of `target`, each its own: Alt and
/// a letter two widgets share goes to neither, cycling between them.
#[track_caller]
fn mnemonics(snapshot: &Snapshot, target: &str) -> BTreeSet<char> {
    let mut letters = BTreeSet::new();
    for control in snapshot.in_target(target) {
        if !matches!(
            control.kind,
            ControlKind::Button | ControlKind::Toggle | ControlKind::Check | ControlKind::Radio
        ) {
            continue;
        }
        if let Some(letter) = mnemonic(&control.text) {
            assert!(
                letters.insert(letter),
                "two buttons use Alt+{letter}: {}",
                control.text
            );
        }
    }
    letters
}

// ---- a canvas that keeps what was filled, for the preview's twin ----------------------------------

/// Metrics of a font whose every character is half its pixel size wide.
struct FixedMetrics;

impl FixedMetrics {
    fn pixels(font: &Font) -> f64 {
        match font.size {
            FontSize::Pixels(pixels) => f64::from(pixels),
            FontSize::Points(points) => points * 96.0 / 72.0,
        }
    }
}

impl FontMetrics for FixedMetrics {
    fn ascent(&self, font: &Font) -> f64 {
        Self::pixels(font) * 0.8
    }
    fn descent(&self, font: &Font) -> f64 {
        Self::pixels(font) * 0.2
    }
    fn height(&self, font: &Font) -> f64 {
        Self::pixels(font)
    }
    fn line_spacing(&self, font: &Font) -> f64 {
        Self::pixels(font)
    }
    fn horizontal_advance(&self, font: &Font, text: &str) -> f64 {
        Self::pixels(font) / 2.0 * text.chars().count() as f64
    }
    fn bounding_rect(&self, font: &Font, text: &str) -> RectF {
        RectF::new(
            0.0,
            -self.ascent(font),
            self.horizontal_advance(font, text),
            self.height(font),
        )
    }
    fn elided_text(&self, _font: &Font, text: &str, _mode: Elide, _width: f64) -> String {
        String::from(text)
    }
}

/// One shape filled with a solid brush: its outline's points and its colour.
struct Filled {
    points: Vec<PointF>,
    colour: Color,
}

/// A canvas that keeps every shape drawn with a solid brush (drawPolygon, drawRect, drawPath),
/// with no transform: what a painter's twin reads where the C++ read pixels of an image. The
/// background (fillRect) is not kept: what is not covered by a kept shape is the background.
struct FillCanvas {
    pen: Pen,
    brush: Brush,
    font: Font,
    transform: Transform,
    opacity: f64,
    saved: Vec<(Pen, Brush, Font, Transform, f64)>,
    filled: Vec<Filled>,
    metrics: FixedMetrics,
}

impl FillCanvas {
    fn new() -> Self {
        Self {
            pen: Pen::default(),
            brush: Brush::none(),
            font: Font::default(),
            transform: Transform::IDENTITY,
            opacity: 1.0,
            saved: Vec::new(),
            filled: Vec::new(),
            metrics: FixedMetrics,
        }
    }

    /// Whether `point` is inside a kept shape (the even-odd rule over its outline).
    fn covers(&self, point: PointF) -> bool {
        self.filled.iter().any(|shape| {
            let points = &shape.points;
            let mut inside = false;
            for (at, a) in points.iter().enumerate() {
                let b = points[(at + 1) % points.len()];
                if (a.y > point.y) != (b.y > point.y)
                    && point.x < a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x)
                {
                    inside = !inside;
                }
            }
            inside
        })
    }
}

impl Canvas for FillCanvas {
    fn device_size(&self) -> SizeF {
        SizeF::new(400.0, 300.0)
    }
    fn device_pixel_ratio(&self) -> f64 {
        1.0
    }
    fn save(&mut self) {
        self.saved.push((
            self.pen.clone(),
            self.brush.clone(),
            self.font.clone(),
            self.transform,
            self.opacity,
        ));
    }
    fn restore(&mut self) {
        if let Some((pen, brush, font, transform, opacity)) = self.saved.pop() {
            self.pen = pen;
            self.brush = brush;
            self.font = font;
            self.transform = transform;
            self.opacity = opacity;
        }
    }
    fn pen(&self) -> Pen {
        self.pen.clone()
    }
    fn set_pen(&mut self, pen: Pen) {
        self.pen = pen;
    }
    fn brush(&self) -> Brush {
        self.brush.clone()
    }
    fn set_brush(&mut self, brush: Brush) {
        self.brush = brush;
    }
    fn font(&self) -> Font {
        self.font.clone()
    }
    fn set_font(&mut self, font: Font) {
        self.font = font;
    }
    fn render_hint(&self, _hint: RenderHint) -> bool {
        false
    }
    fn set_render_hint(&mut self, _hint: RenderHint, _on: bool) {}
    fn opacity(&self) -> f64 {
        self.opacity
    }
    fn set_opacity(&mut self, opacity: f64) {
        self.opacity = opacity;
    }
    fn world_transform(&self) -> Transform {
        self.transform
    }
    fn set_world_transform(&mut self, transform: Transform, combine: bool) {
        self.transform = if combine {
            transform * self.transform
        } else {
            transform
        };
    }
    fn device_transform(&self) -> Transform {
        self.transform
    }
    fn set_clip_rect(&mut self, _rect: RectF, _intersect: bool) {}
    fn set_clip_path(&mut self, _path: &Path, _intersect: bool) {}
    fn set_clipping(&mut self, _on: bool) {}
    fn has_clipping(&self) -> bool {
        false
    }
    fn draw_path(&mut self, path: &Path) {
        if let BrushStyle::Solid(colour) = &self.brush.style {
            let points: Vec<PointF> = path
                .elements
                .iter()
                .filter_map(|element| match element {
                    PathElement::MoveTo(point) | PathElement::LineTo(point) => Some(*point),
                    _ => None,
                })
                .collect();
            if points.len() >= 3 {
                self.filled.push(Filled {
                    points,
                    colour: *colour,
                });
            }
        }
    }
    fn fill_path(&mut self, _path: &Path, _brush: &Brush) {}
    fn stroke_path(&mut self, _path: &Path, _pen: &Pen) {}
    fn draw_text(&mut self, _origin: PointF, _text: &str) {}
    fn draw_text_in(&mut self, rect: RectF, _flags: TextFlags, _text: &str) -> RectF {
        rect
    }
    fn draw_image(&mut self, _target: RectF, _image: &Image, _source: IntRect) {}
    fn metrics(&self) -> &dyn FontMetrics {
        &self.metrics
    }
}

mod road_object_dialog {
    use super::*;

    // ---- the line the placing dialog writes ---------------------------------------------------

    #[test]
    fn an_alignment_placement_writes_its_keys_before_its_placement() {
        let mut form = RoadObjectForm {
            kind: RoadObjectKind::Barrier,
            alignment: String::from("Main"),
            station: String::from("10"),
            to: String::from("60"),
            offset: String::from(" 3 "),
            parameters: vec![(String::from("type"), String::from("fshape"))],
            name: String::from("Median east"),
            ..RoadObjectForm::default()
        };
        let line =
            road_object_add_line(&form).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            line,
            "ROADOBJECT ADD barrier name=\"Median east\" type=fshape ALIGNMENT Main station=10 \
             to=60 offset=3"
        );
        form.preview = true;
        assert!(
            road_object_add_line(&form)
                .expect("the line")
                .ends_with(" PREVIEW")
        );
    }

    #[test]
    fn a_scope_ends_the_line_so_its_where_takes_nothing_of_the_objects() {
        let form = RoadObjectForm {
            kind: RoadObjectKind::Fence,
            place: Place::Lines,
            scope: String::from("LAYERS boundary WHERE TYPE=polyline"),
            parameters: vec![(String::from("height"), String::from("1.5"))],
            layer: String::from("fences"),
            ..RoadObjectForm::default()
        };
        let line =
            road_object_add_line(&form).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            line,
            "ROADOBJECT ADD fence layer=fences height=1.5 ALONG FROM LAYERS boundary WHERE \
             TYPE=polyline"
        );
    }

    #[test]
    fn points_are_written_as_typed_and_refused_when_they_do_not_read() {
        let mut form = RoadObjectForm {
            kind: RoadObjectKind::Sign,
            place: Place::Points,
            points: String::from("5,5,1  6,7"),
            parameters: vec![(String::from("code"), String::from("R1-1"))],
            ..RoadObjectForm::default()
        };
        assert_eq!(
            road_object_add_line(&form).expect("the line"),
            "ROADOBJECT ADD sign code=R1-1 AT 5,5,1 6,7"
        );
        form.points = String::from("5,5 six,7");
        assert!(road_object_add_line(&form).is_err());
        form.points.clear();
        assert!(road_object_add_line(&form).is_err());
    }

    #[test]
    fn what_a_line_cannot_say_refuses_the_whole_line() {
        let mut form = RoadObjectForm {
            kind: RoadObjectKind::Sign,
            alignment: String::from("Main"),
            ..RoadObjectForm::default()
        };
        let message = |form: &RoadObjectForm| {
            road_object_add_line(form)
                .expect_err("a line that cannot be said")
                .message
        };
        // No station.
        assert!(message(&form).contains("station"));
        form.station = String::from("fifty");
        assert!(message(&form).contains("station"));
        form.station = String::from("50");
        form.name = String::from("the \"big\" one");
        assert!(message(&form).contains("double quote"));
        form.name.clear();
        form.parameters = vec![(String::from("width"), String::from("wide"))];
        assert!(message(&form).contains("width"));
        form.parameters.clear();
        form.alignment.clear();
        assert!(road_object_add_line(&form).is_err());
        form.place = Place::Lines;
        assert!(road_object_add_line(&form).is_err(), "no scope");
    }

    #[test]
    fn the_preview_makes_every_kind_with_its_defaults_as_closed_solids() {
        for &kind in road_object_kinds() {
            let model = road_object_preview(kind, &[]);
            assert!(
                model.problems.is_empty(),
                "{kind:?}: {}",
                model
                    .problems
                    .first()
                    .map_or("", |problem| problem.message.as_str())
            );
            assert!(!model.objects.is_empty(), "{kind:?}");
            for object in &model.objects {
                assert!(is_closed_mesh(&object.mesh), "{}", object.id);
            }
        }
        // A parameter that does not read is left at its default; one that does is used: a wall
        // 3 m tall.
        let wall = road_object_preview(
            RoadObjectKind::Wall,
            &[(String::from("height"), String::from("3"))],
        );
        assert!(!wall.objects.is_empty());
        assert!((wall.objects[0].mesh.bounds().max.z - 3.0).abs() <= 1e-9);
    }

    // ---- the placing dialog, by object name ---------------------------------------------------

    #[test]
    fn a_sign_placed_from_its_page_is_the_line_a_person_would_type() {
        let mut d = on_kind(RoadObjectKind::Sign);
        assert_eq!(d.dialog.current_kind(), RoadObjectKind::Sign);
        assert_eq!(d.text("roadObjectKinds"), "Sign");
        assert_eq!(d.text("roadObjectAlignment"), "Main");
        d.fill("roadObjectStation", "50");
        d.fill("roadObjectOffset", "4");
        d.fill("roadObject_sign_code", "R1-1");
        assert_eq!(
            d.text("roadObjectLine"),
            "ROADOBJECT ADD sign code=R1-1 ALIGNMENT Main station=50 offset=4"
        );

        // Preview changes nothing.
        d.press("roadObjectPreviewRun");
        assert_eq!(d.lines().len(), 1);
        assert!(d.last().ends_with(" PREVIEW"));
        assert!(d.document().road_objects().objects.is_empty());
        let reply = d.text("roadObjectReply");
        assert!(reply.starts_with("preview id=sign-1"), "{reply}");

        d.press("roadObjectAdd");
        assert_eq!(d.lines().len(), 2);
        assert_eq!(
            d.last(),
            "ROADOBJECT ADD sign code=R1-1 ALIGNMENT Main station=50 offset=4"
        );
        let objects = d.document().road_objects();
        assert_eq!(objects.objects.len(), 1);
        assert_eq!(objects.objects[0].id, "sign-1");
        assert!(d.text("roadObjectStatus").contains("one undo step"));
    }

    #[test]
    fn a_choice_left_at_its_default_is_left_out_and_a_chosen_one_is_written() {
        let mut d = on_kind(RoadObjectKind::Barrier);
        d.fill("roadObjectStation", "0");
        d.fill("roadObjectTo", "20");
        assert!(!d.text("roadObjectLine").contains("type="));
        let choice = d.control("roadObject_barrier_type");
        assert_eq!(
            choice.value, choice.items[0],
            "the first item is the default"
        );
        assert!(
            !d.dialog
                .form()
                .parameters
                .iter()
                .any(|(key, _)| key == "type"),
            "the default carries no word"
        );
        d.fill("roadObject_barrier_type", "singleslope");
        let line = d.text("roadObjectLine");
        assert!(line.contains(" type=singleslope ALIGNMENT"), "{line}");
        // What cannot be said is not run: Add is off and the status says why.
        d.fill("roadObjectTo", "twenty");
        assert!(!d.enabled("roadObjectAdd"));
        let status = d.text("roadObjectStatus");
        assert!(status.contains("to must be a number"), "{status}");
    }

    #[test]
    fn a_refused_line_changes_nothing_and_says_why() {
        let mut d = on_kind(RoadObjectKind::Sign);
        d.fill("roadObjectStation", "150");
        d.press("roadObjectAdd");
        assert!(d.document().road_objects().objects.is_empty());
        let status = d.text("roadObjectStatus");
        assert!(status.contains("off alignment Main"), "{status}");
    }

    // ---- the findings of the review of 2026-10-15 ----------------------------------------------

    #[test]
    fn adding_from_a_scope_that_takes_nothing_says_nothing_was_added_and_makes_no_undo_step() {
        // The scope is the selection and nothing is selected: the verb reports added=0 and
        // records no step. The status said "Added - one undo step."
        let mut d = on_kind(RoadObjectKind::Fence);
        d.press("roadObjectPlaceLines");
        let line = d.text("roadObjectLine");
        assert!(line.contains("ALONG FROM"), "{line}");
        d.press("roadObjectAdd");
        let reply = d.text("roadObjectReply");
        assert!(reply.contains("added=0"), "{reply}");
        let status = d.text("roadObjectStatus");
        assert!(status.starts_with("Nothing added"), "{status}");
        assert!(!status.contains("undo step"), "{status}");
        assert!(d.document().road_objects().objects.is_empty());
        d.press("roadObjectPreviewRun");
        let status = d.text("roadObjectStatus");
        assert!(
            status.starts_with("Previewed: the scope took nothing"),
            "{status}"
        );
    }

    #[test]
    fn adding_from_a_scope_that_takes_two_lines_says_how_many_were_added_in_one_undo_step() {
        let mut host = site();
        for line in [
            "LINE 0,20 10,20",
            "LINE 0,30 10,30",
            "SELECT ALL",
            "PROP SET elevation 2.5",
        ] {
            ok(&mut host, line);
        }
        let mut d = placing(host);
        d.dialog.show_kind(RoadObjectKind::Fence);
        d.settle();
        d.press("roadObjectPlaceLines");
        d.press("roadObjectSourceScopeDrawing");
        d.press("roadObjectAdd");
        assert_eq!(d.document().road_objects().objects.len(), 2);
        assert_eq!(d.text("roadObjectStatus"), "Added 2 - one undo step.");
    }

    #[test]
    fn the_scopes_own_refusal_is_shown_in_place_of_a_generic_one() {
        // A filter colour that does not read: the scope widget says why, and the dialog said
        // "say which part of the drawing" to a person who had.
        let mut d = on_kind(RoadObjectKind::Fence);
        d.press("roadObjectPlaceLines");
        d.fill("roadObjectSourceFilterColour", "red");
        let status = d.text("roadObjectStatus");
        assert!(status.contains("filter's colour"), "{status}");
        assert!(!status.contains("say which part"), "{status}");
        assert!(!d.enabled("roadObjectAdd"));
        let refused = road_object_add_line(&d.dialog.form()).expect_err("no scope's words");
        assert!(refused.message.contains("filter's colour"));
        // With nothing the scope objects to and nothing said, the old words stand.
        let form = RoadObjectForm {
            kind: RoadObjectKind::Fence,
            place: Place::Lines,
            ..RoadObjectForm::default()
        };
        assert!(
            road_object_add_line(&form)
                .expect_err("no scope")
                .message
                .contains("say which part")
        );
    }

    #[test]
    fn every_button_of_the_dialog_has_its_own_mnemonic() {
        // Alt+D was both "At points in the drawing" and "Add": two widgets with one key make it
        // cycle between them and choose neither.
        let d = placing(site());
        let letters = mnemonics(&d.harness.snapshot(), "roadObjectDialog");
        assert!(letters.contains(&'d'), "Add keeps Alt+D");
    }

    #[test]
    fn the_crossfall_is_only_offered_where_there_is_a_profile_to_carry_out() {
        let mut d = on_kind(RoadObjectKind::Sign);
        d.fill("roadObjectStation", "50");
        d.fill("roadObjectCrossfall", "-3");
        assert!(d.enabled("roadObjectCrossfall"));
        assert!(d.text("roadObjectLine").contains("crossfall=-3"));
        // Along lines, or at points, there is no profile: not offered, not written.
        d.press("roadObjectPlacePoints");
        d.fill("roadObjectPoints", "5,5,1");
        assert!(!d.enabled("roadObjectCrossfall"));
        let line = d.text("roadObjectLine");
        assert!(!line.contains("crossfall"), "{line}");
        d.press("roadObjectAdd");
        assert_eq!(d.document().road_objects().objects.len(), 1);
        // On the heights of a line, or a surface: the same.
        d.press("roadObjectPlaceAlignment");
        d.fill("roadObjectHeights", "surface");
        assert!(!d.enabled("roadObjectCrossfall"));
        d.fill("roadObjectHeights", "profile");
        assert!(d.enabled("roadObjectCrossfall"));
    }

    #[test]
    fn the_culvert_preview_shows_the_barrel_through_its_headwalls() {
        // Seen along the barrel each headwall is a slab as large as the barrel and more, in the
        // barrel's colour; painted solid over it they hid it, and the preview was one plain
        // rectangle whatever the shape and the cells. The headwalls are outlines now and the
        // barrel is drawn over them: the pipe's hole is the background. Found by the culvert
        // colour (158, 154, 146): its bounding box is the barrel's, and the middle of that is the
        // hole - which no shape filled covers, so it is the preview's background there.
        let mut d = on_kind(RoadObjectKind::Culvert);
        let barrel = Color::rgb(158, 154, 146);
        for shape in ["circular", "box"] {
            d.fill("roadObject_culvert_shape", shape);
            let mut canvas = FillCanvas::new();
            d.dialog
                .paint_preview(&mut canvas, RectF::new(0.0, 0.0, 400.0, 300.0));
            let mut x0 = f64::INFINITY;
            let mut x1 = f64::NEG_INFINITY;
            let mut y0 = f64::INFINITY;
            let mut y1 = f64::NEG_INFINITY;
            for filled in canvas
                .filled
                .iter()
                .filter(|filled| filled.colour == barrel)
            {
                for point in &filled.points {
                    x0 = x0.min(point.x);
                    x1 = x1.max(point.x);
                    y0 = y0.min(point.y);
                    y1 = y1.max(point.y);
                }
            }
            assert!(x1 > x0, "{shape}: no culvert colour painted");
            assert!(y1 > y0, "{shape}");
            let middle = PointF::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            assert!(
                !canvas.covers(middle),
                "{shape}: the middle of the barrel is not its hole"
            );
        }
    }

    #[test]
    fn the_pages_scroll_so_preview_add_and_close_stay_in_reach_on_a_laptop_screen() {
        // Laid out in one piece the dialog was some 1,500 px tall and Add was off the screen of a
        // 1366 x 768 laptop (728 px under its task bar). The pages, placement and heights are the
        // frame's scrolling body; the buttons are its footer. A screen whose room is 700 points
        // (its height less the frame's margin each side) holds the dialog.
        let host = site();
        let dialog = RoadObjectDialog::new(host.document());
        let d = Driven::on(
            Harness::with_screen(1366.0, 716.0),
            host,
            dialog,
            "roadObjectDialog",
        );
        assert_eq!(d.control("roadObjectBody").part, Part::Body);
        for name in [
            "roadObjectAdd",
            "roadObjectPreviewRun",
            "roadObjectClose",
            "roadObjectLine",
            "roadObjectReply",
            "roadObjectStatus",
        ] {
            assert_eq!(
                d.control(name).part,
                Part::Footer,
                "{name} scrolls with the pages"
            );
        }
        for name in [
            "roadObjectKinds",
            "roadObjectPlacement",
            "roadObjectHeightsGroup",
            "roadObjectPreview",
        ] {
            assert_eq!(d.control(name).part, Part::Body, "{name}");
        }
        let shown = d
            .harness
            .target_record("roadObjectDialog")
            .expect("the dialog");
        assert!(shown.rect.height() <= 700.0, "{}", shown.rect.height());
    }
}

/// The manager on a site of three objects, in this order:
///
/// ```text
/// barrier-1  road/objects/barrier  stations 0 to 20, offset 4
/// sign-2     road/objects/sign     station 50, offset 4
/// light-3    lighting/poles        poles at stations 0, 30 and 60, offset 6
/// ```
fn scoped_manager() -> Driven<RoadObjectManagerDialog> {
    let mut host = site();
    for line in [
        "ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4",
        "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4",
        "ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6 \
         layer=lighting/poles",
    ] {
        ok(&mut host, line);
    }
    manager(host)
}

mod road_object_manager {
    use super::*;

    #[test]
    fn it_lists_the_objects_and_each_edit_is_the_line_a_person_would_type() {
        let mut host = site();
        ok(
            &mut host,
            "ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4",
        );
        ok(
            &mut host,
            "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4",
        );
        let mut d = manager(host);
        let table = d.dialog.table();
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.text(0, 0), "barrier-1");
        assert_eq!(table.text(1, 0), "sign-2");
        assert_eq!(d.text("roadObjectCount"), "2 road objects, 0 problems");

        select_object(&mut d, "barrier-1");
        let info = d.text("roadObjectInfo");
        assert!(info.starts_with("object id=barrier-1"), "{info}");
        d.fill("roadObjectKeys", "offset=5");
        d.press("roadObjectApply");
        assert_eq!(d.last(), "ROADOBJECT SET barrier-1 offset=5");
        assert_eq!(
            d.document()
                .road_objects()
                .find("barrier-1")
                .expect("barrier-1")
                .placement
                .offset,
            5.0
        );
        // The table followed the drawing, and kept the row chosen.
        d.settle();
        assert_eq!(d.dialog.selected_ids(), ["barrier-1"]);

        d.press("roadObjectCopy");
        assert_eq!(d.last(), "ROADOBJECT COPY barrier-1");
        assert_eq!(d.document().road_objects().objects.len(), 3);

        select_object(&mut d, "sign-2");
        d.press("roadObjectDelete");
        assert_eq!(d.last(), "ROADOBJECT DELETE sign-2");
        assert!(d.document().road_objects().find("sign-2").is_none());
    }

    #[test]
    fn zoom_frames_the_chosen_objects_with_room_round_them() {
        // The sign at (50, 4): its solids span x 49.9595 to 50.0375 and y 3.55 to 4.45
        // (test_road_objects.cpp), so it is framed 10 m across - the least a frame is - about its
        // middle, (49.9985, 4).
        let mut host = site();
        ok(
            &mut host,
            "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4",
        );
        let frame = road_objects_frame(host.document(), &[String::from("sign-1")])
            .expect("the sign is framed");
        assert!((frame.min.x - (49.9985 - 5.0)).abs() <= 1e-9);
        assert!((frame.max.x - (49.9985 + 5.0)).abs() <= 1e-9);
        assert!((frame.min.y - -1.0).abs() <= 1e-9);
        assert!((frame.max.y - 9.0).abs() <= 1e-9);
        assert!(road_objects_frame(host.document(), &[String::from("sign-9")]).is_none());

        let mut d = manager(host);
        select_object(&mut d, "sign-1");
        d.press("roadObjectZoom");
        let last = d.last();
        assert!(last.starts_with("ZOOM WINDOW "), "{last}");
    }

    #[test]
    fn a_set_it_cannot_read_is_said_and_not_shown_as_zero_objects() {
        let mut host = site();
        let mut metadata = host.document().metadata().clone();
        metadata.unknown_keys.insert(
            String::from("road_objects"),
            String::from(
                r#"{"format": "katana-road-objects", "version": 2, "next": 2, "objects": []}"#,
            ),
        );
        host.document_mut().set_metadata(metadata);
        let d = manager(host);
        let count = d.text("roadObjectCount");
        assert!(!count.starts_with("0 road objects, 0 problems"), "{count}");
        assert!(count.starts_with("0 road objects, 1 problems"), "{count}");
        assert!(count.contains("cannot be read"), "{count}");
        assert!(count.contains("newer version"), "{count}");
    }

    #[test]
    fn copy_takes_the_keys_field_so_a_copy_at_points_can_be_moved_from_the_window() {
        // station=, dx= and dy= are what move a copy and nothing else in the window says them:
        // Copy sent only the id, and the copy stood on its original.
        let mut host = site();
        ok(&mut host, "ROADOBJECT ADD sign AT 5,5,1");
        let mut d = manager(host);
        select_object(&mut d, "sign-1");
        d.fill("roadObjectKeys", "dx=10 dy=-2");
        d.press("roadObjectCopy");
        assert_eq!(d.last(), "ROADOBJECT COPY sign-1 dx=10 dy=-2");
        {
            let objects = d.document().road_objects();
            let copy = objects.find("sign-2").expect("the copy");
            assert_eq!(copy.placement.vertices.len(), 1);
            assert_eq!(copy.placement.vertices[0].position, Point2::new(15.0, 3.0));
        }
        // The keys were for that copy: spent, not left to move the next.
        assert_eq!(d.text("roadObjectKeys"), "");
        d.press("roadObjectCopy");
        assert_eq!(d.last(), "ROADOBJECT COPY sign-1");
        // A refused Apply keeps its keys, to be put right.
        d.fill("roadObjectKeys", "nosuchkey=1");
        d.press("roadObjectApply");
        assert_eq!(d.text("roadObjectKeys"), "nosuchkey=1");
        let status = d.text("roadObjectManagerStatus");
        assert!(status.starts_with("Refused"), "{status}");
    }

    // ---- the manager by scope -------------------------------------------------------------------
    //
    // The shared "Apply to" and "Only those that match" controls beside the table, and the three
    // buttons that put them into ROADOBJECT LIST, QUANTITIES and DELETE - each the line a person
    // would type, run by the window's executor.

    #[test]
    fn the_shared_scope_controls_start_on_the_whole_drawing_and_grey_what_a_road_object_has_none_of()
     {
        let d = scoped_manager();
        d.control("roadObjectManagerByScope");
        assert_eq!(
            d.control("roadObjectManagerScopeDrawing").checked,
            Some(true)
        );
        // An entity's type, style and text: a road object has none, so they are greyed with the
        // reason, where the verb would refuse them by name.
        for name in [
            "roadObjectManagerTypePoint",
            "roadObjectManagerTypeLine",
            "roadObjectManagerTypeSpline",
            "roadObjectManagerFilterStyle",
            "roadObjectManagerFilterText",
        ] {
            assert!(!d.enabled(name), "{name}");
            assert!(
                scope_control_tip(name).is_some_and(|tip| tip.contains("not an entity")),
                "{name}"
            );
        }
        // What means something for one stays live: a layer pattern, a colour, a property and its
        // value, and what is drawn.
        for name in [
            "roadObjectManagerFilterLayer",
            "roadObjectManagerFilterColour",
            "roadObjectManagerFilterProperty",
            "roadObjectManagerFilterValue",
            "roadObjectManagerDrawnOnly",
        ] {
            assert!(d.enabled(name), "{name}");
        }
        // The three buttons, each with a mnemonic of its own beside the others'.
        mnemonics(&d.harness.snapshot(), "roadObjectManagerDialog");
        for name in [
            "roadObjectSelectMatching",
            "roadObjectQuantities",
            "roadObjectDeleteMatching",
        ] {
            assert_eq!(d.control(name).kind, ControlKind::Button, "{name}");
        }
    }

    #[test]
    fn select_matching_runs_list_with_the_scope_and_chooses_the_rows_it_took() {
        let mut d = scoped_manager();

        // A property and its value on the whole drawing: the kind.
        d.fill("roadObjectManagerFilterProperty", "kind");
        d.fill("roadObjectManagerFilterValue", "barrier");
        d.press("roadObjectSelectMatching");
        assert_eq!(d.last(), "ROADOBJECT LIST DRAWING WHERE PROP=kind:barrier");
        assert_eq!(d.dialog.selected_ids(), ["barrier-1"]);
        let status = d.text("roadObjectManagerStatus");
        assert!(
            status.starts_with("1 chosen - scope=drawing where=\"PROP=kind:barrier\" matched=1"),
            "{status}"
        );

        // A layer pattern of the filter: the lights' layer.
        d.fill("roadObjectManagerFilterProperty", "");
        d.fill("roadObjectManagerFilterValue", "");
        d.fill("roadObjectManagerFilterLayer", "lighting/*");
        d.press("roadObjectSelectMatching");
        assert_eq!(d.last(), "ROADOBJECT LIST DRAWING WHERE LAYER=lighting/*");
        assert_eq!(d.dialog.selected_ids(), ["light-3"]);

        // The checked layers: the sign's, with the layers beneath it.
        d.fill("roadObjectManagerFilterLayer", "");
        d.press("roadObjectManagerScopeLayers");
        d.control("roadObjectManagerLayers");
        let ticked = d
            .dialog
            .scope_mut()
            .layer_names()
            .iter()
            .filter(|layer| *layer == "road/objects/sign")
            .count();
        assert_eq!(ticked, 1);
        d.dialog.scope_mut().tick_layer("road/objects/sign", true);
        d.settle();
        d.press("roadObjectSelectMatching");
        assert_eq!(d.last(), "ROADOBJECT LIST LAYERS road/objects/sign");
        assert_eq!(d.dialog.selected_ids(), ["sign-2"]);

        // A scope that takes nothing is said, not refused, and chooses nothing.
        d.press("roadObjectManagerScopeDrawing");
        d.fill("roadObjectManagerFilterProperty", "kind");
        d.fill("roadObjectManagerFilterValue", "culvert");
        d.press("roadObjectSelectMatching");
        assert!(d.dialog.selected_ids().is_empty());
        let status = d.text("roadObjectManagerStatus");
        assert!(
            status.starts_with("0 chosen - scope=drawing where=\"PROP=kind:culvert\" matched=0"),
            "{status}"
        );
    }

    #[test]
    fn quantities_runs_quantities_with_the_scope_and_shows_them_in_the_details() {
        let mut d = scoped_manager();
        d.fill("roadObjectManagerFilterProperty", "kind");
        d.fill("roadObjectManagerFilterValue", "barrier");
        d.press("roadObjectQuantities");
        assert_eq!(
            d.last(),
            "ROADOBJECT QUANTITIES DRAWING WHERE PROP=kind:barrier"
        );
        // The one barrier is 20 m long.
        let details = d.text("roadObjectInfo");
        assert!(
            details.starts_with("scope=drawing where=\"PROP=kind:barrier\" matched=1"),
            "{details}"
        );
        assert!(
            details.contains("total kind=barrier objects=1 length=20"),
            "{details}"
        );
        assert!(!details.contains("kind=sign"), "{details}");
    }

    #[test]
    fn delete_matching_runs_delete_with_the_scope_as_one_step_and_the_table_follows() {
        let mut d = scoped_manager();
        d.fill("roadObjectManagerFilterLayer", "road/objects/s*");
        d.press("roadObjectDeleteMatching");
        assert_eq!(
            d.last(),
            "ROADOBJECT DELETE DRAWING WHERE LAYER=road/objects/s*"
        );
        assert!(d.document().road_objects().find("sign-2").is_none());
        assert!(d.document().road_objects().find("barrier-1").is_some());
        assert!(d.document().road_objects().find("light-3").is_some());
        d.settle();
        assert_eq!(d.rows("roadObjectTable"), 2);
        let status = d.text("roadObjectManagerStatus");
        assert!(status.contains("deleted count=1 ids=sign-2"), "{status}");
        // One undo brings it back.
        d.ok("UNDO");
        d.settle();
        assert!(d.document().road_objects().find("sign-2").is_some());
        assert_eq!(d.rows("roadObjectTable"), 3);
    }

    #[test]
    fn controls_that_do_not_read_refuse_with_their_own_reason_and_run_nothing() {
        let mut d = scoped_manager();
        let before = d.lines().len();
        // The layers scope with no layer ticked, and a colour that is not a colour.
        d.press("roadObjectManagerScopeLayers");
        d.press("roadObjectDeleteMatching");
        let status = d.text("roadObjectManagerStatus");
        assert!(status.starts_with("Refused: "), "{status}");
        d.press("roadObjectManagerScopeDrawing");
        d.fill("roadObjectManagerFilterColour", "red");
        d.press("roadObjectSelectMatching");
        let status = d.text("roadObjectManagerStatus");
        assert!(status.starts_with("Refused: "), "{status}");
        // Nothing was handed to the executor, and nothing was deleted.
        assert_eq!(d.lines().len(), before);
        assert_eq!(d.document().road_objects().objects.len(), 3);
    }
}

mod road_object_workbench {
    use super::*;

    #[test]
    fn the_bim_menu_gains_a_road_objects_section_with_an_icon_and_a_tip_on_every_item() {
        let mut host = site();
        let workbench = RoadObjectWorkbench::new();
        let menu = workbench.menu();
        let mut names = Vec::new();
        let mut letters = BTreeSet::new();
        for item in &menu.items {
            match item {
                MenuItem::Section(title) => {
                    assert_eq!(title, "Road Objects", "the section's title")
                }
                MenuItem::Action(action) => {
                    names.push(action.name.clone());
                    assert!(!action.icon.is_empty(), "{}", action.name);
                    assert!(!action.tip.is_empty(), "{}", action.name);
                    // Each underlined letter its own within the section.
                    if let Some(letter) = mnemonic(&action.text) {
                        assert!(letters.insert(letter), "{}", action.text);
                    }
                }
                MenuItem::Separator | MenuItem::Menu(_) => {}
            }
        }
        assert_eq!(
            names,
            [
                "bimRoadBarrier",
                "bimRoadGuidePosts",
                "bimRoadSign",
                "bimRoadLightPoles",
                "bimRoadCulvert",
                "bimRoadWall",
                "bimRoadFence",
                "bimRoadLineMarking",
                "bimRoadObjects",
            ]
        );
        // The ones placed most on the toolbar, after a separator.
        let bar = workbench.tool_bar();
        let on_bar: Vec<&str> = bar
            .actions()
            .iter()
            .map(|action| action.name.as_str())
            .collect();
        assert_eq!(
            on_bar,
            [
                "bimRoadBarrier",
                "bimRoadSign",
                "bimRoadLightPoles",
                "bimRoadLineMarking",
                "bimRoadObjects",
            ]
        );

        // A menu item opens the one dialog, on its own page, as the window's.
        let mut window = WorkbenchHost::new(workbench);
        window
            .trigger(&mut host, "bimRoadCulvert")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut d = Driven::new(host, window, "");
        // By its object name, as --dialog finds it.
        assert!(d.harness.target_record("roadObjectDialog").is_some());
        assert_eq!(
            d.dialog
                .workbench
                .placing_dialog()
                .expect("the placing dialog")
                .current_kind(),
            RoadObjectKind::Culvert
        );
        {
            let Driven { dialog, host, .. } = &mut d;
            dialog
                .trigger(host, MANAGER_ACTION)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        }
        d.settle();
        assert!(d.harness.target_record("roadObjectManagerDialog").is_some());
        assert!(d.dialog.workbench.manager_dialog().is_some());
    }

    #[test]
    fn every_road_object_icon_paints_something() {
        // The nine icons of the section, each named for the icon set: the window's icons
        // (bim::road_object_icons, painted by code through the canvas) draw each by this name.
        // Counting the ink each paints is the icon set's own twin, with its painter, which this
        // wave builds beside the dialogs (see the wave's report).
        let workbench = RoadObjectWorkbench::new();
        let icons: Vec<&'static str> = workbench
            .menu()
            .actions()
            .iter()
            .map(|action| action.icon)
            .collect();
        assert_eq!(
            icons,
            [
                "RoadBarrier",
                "RoadGuidePosts",
                "RoadSign",
                "RoadLightPole",
                "RoadCulvert",
                "RoadWall",
                "RoadFence",
                "RoadLineMarking",
                "RoadObjects",
            ]
        );
        for entry in &KIND_ACTIONS {
            assert!(!entry.icon.is_empty(), "{}", entry.name);
        }
    }

    // Help > Command Reference reads HELP ROADOBJECT as a section of its own, so a word added to
    // the verb's help is there at once: the help it reads.
    #[test]
    fn the_command_reference_has_the_road_object_verbs() {
        let all = road_object_verb_help();
        for line in [
            "ROADOBJECT ADD kind",
            "ROADOBJECT KINDS",
            "ROADOBJECT PROBLEMS",
            "ALONG FROM <scope>",
        ] {
            assert!(all.contains(line), "{line}");
        }
        assert!(!all.is_empty());
    }
}
