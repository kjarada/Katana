//! A test implementation of each interface the foundation freezes for the later waves - the
//! program's view of the window, the views a dialog reaches, a workbench, the canvas, the jobs,
//! the clipboard and the settings - so each is shown implementable and usable as the plan's
//! section 7 (risk 2) asks, before the agents that implement them for real.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use egui::Ui;
use katana_app::console::{Captured, Console};
use katana_cad::view_set::{ViewId, ViewSet};
use katana_cad::viewport_layout::ViewKind;
use katana_cad::{Document, PlotSettings};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_geometry::{Box2, Point2};
use katana_update::ApplyRequest;
use katana_window::canvas::{
    Brush, Canvas, Color, Elide, FillRule, Font, FontMetrics, Image, IntRect, Path as CanvasPath,
    Pen, PointF, RectF, RenderHint, SizeF, TextFlags, Transform, read_png, write_png,
};
use katana_window::customisation::scope_filter_views;
use katana_window::drive::{
    Action, DrivenWindow, Harness, Menu, MenuSigns, SheetsPlot, ToolBar, WorkbenchHost,
};
use katana_window::host::{
    Host, JobControl, JobOutcome, PickId, RecordingHost, SettingValue, UserSettings, VerbOutcome,
    ViewAccess, job_apply, job_finished, job_work, with_geo,
};
use katana_window::shell::{DialogFrame, DockSpec, Workbench};

use crate::window_support::Scratch;

mod interfaces {
    use super::*;

    // ---- the program's view of the window -------------------------------------------------------

    /// A window that says what it was asked.
    #[derive(Default)]
    struct FakeWindow {
        document: Document,
        calls: Vec<String>,
        headless: bool,
        shown: bool,
    }

    impl FakeWindow {
        fn called(&mut self, what: &str) {
            self.calls.push(String::from(what));
        }
    }

    impl DrivenWindow for FakeWindow {
        fn ui(&mut self, ui: &mut Ui) {
            ui.label("the window");
        }
        fn busy(&self) -> bool {
            false
        }
        fn application_icon(&self) -> Image {
            Image::new(16, 16)
        }
        fn set_headless(&mut self, headless: bool) {
            self.headless = headless;
        }
        fn load_default_customisation(&mut self) {
            self.called("loadDefaultCustomisation");
        }
        fn restore_session(&mut self) {
            self.called("restoreSession");
        }
        fn show(&mut self) {
            self.shown = true;
        }
        fn is_visible(&self) -> bool {
            self.shown
        }
        fn start_updates(&mut self) {
            self.called("startUpdates");
        }
        fn show_start_screen_if_wanted(&mut self) {
            self.called("showStartScreenIfWanted");
        }
        fn open_project(&mut self, _directory: &Path) {
            self.called("openProject");
        }
        fn import_path(&mut self, _path: &Path) {
            self.called("importPath");
        }
        fn run_command(&mut self, line: &str) -> bool {
            self.called(line);
            true
        }
        fn run_verb_line(&mut self, line: &str) -> VerbOutcome {
            self.called(line);
            VerbOutcome::new(true, "done", "")
        }
        fn script_command_line(&self, path: &str) -> String {
            format!("SCRIPT \"{path}\"")
        }
        fn actions(&self) -> Vec<Action> {
            vec![Action::new("formatStyles", "&Styles...").opens("styleManagerDialog")]
        }
        fn menus(&self) -> Vec<Menu> {
            vec![
                Menu::new("formatMenu", "F&ormat")
                    .action(Action::new("formatStyles", "&Styles...")),
            ]
        }
        fn trigger_action(&mut self, name: &str) -> Status {
            if self.actions().iter().any(|action| action.name == name) {
                self.called(name);
                return Ok(());
            }
            Err(make_error(
                ErrorCode::NotFound,
                "no menu item or tool has that name",
                name,
            ))
        }
        fn select_all(&mut self) {
            self.called("selectAll");
        }
        fn select_only(&mut self, _id: EntityId) {
            self.called("selectOnly");
        }
        fn toggle_layer_through_panel(&mut self, _layer: &str) -> Status {
            Ok(())
        }
        fn open_style_manager(&mut self) -> String {
            String::from("styleManagerDialog")
        }
        fn open_attribute_manager(&mut self) -> String {
            String::from("attributeManagerDialog")
        }
        fn open_layer_manager(&mut self) -> String {
            String::from("layerManagerDialog")
        }
        fn open_dataset_info(&mut self, _path: &Path) -> Option<String> {
            None
        }
        fn open_import_options(&mut self, _path: &Path) -> Option<String> {
            None
        }
        fn open_export_options(&mut self, _path: &Path) -> Option<String> {
            Some(String::from("vectorExportDialog"))
        }
        fn open_menu(&mut self, name: &str) -> bool {
            name == "formatMenu"
        }
        fn pointer_at(&mut self, spec: &str, _click: bool) -> Result<String> {
            Err(make_error(
                ErrorCode::ParseFailure,
                "the pointer needs x,y",
                spec,
            ))
        }
        fn report_view(&mut self, _name: &str) -> Option<String> {
            None
        }
        fn shortcut_clashes(&self) -> (Vec<String>, usize) {
            (Vec::new(), 1)
        }
        fn menu_gaps(&self) -> (Vec<String>, usize) {
            (Vec::new(), 1)
        }
        fn menu_sign_clashes(&mut self) -> MenuSigns {
            MenuSigns::default()
        }
        fn check_tool_bar_fit(&mut self, sizes: &[(u32, u32)], err: &mut dyn Console) -> bool {
            for (width, height) in sizes {
                err.err(&format!(
                    "toolbar fit {width}x{height}: 0 toolbars along the top\n"
                ));
            }
            true
        }
        fn check_session_restore(&mut self, err: &mut dyn Console) -> bool {
            err.err("session restore: nothing to restore\n");
            true
        }
        fn document(&self) -> &Document {
            &self.document
        }
        fn plot_drawing_to_pdf(
            &mut self,
            _path: &Path,
            _settings: PlotSettings,
            _fit: bool,
        ) -> Status {
            Ok(())
        }
        fn plot_sheets(&mut self, plot: &SheetsPlot) -> Result<Vec<PathBuf>> {
            Ok(vec![plot.path.clone()])
        }
        fn plot_format_known(format: &str) -> bool {
            ["pdf", "pdfs", "png", "tiff"].contains(&format)
        }
        fn take_pending_update(&mut self) -> Option<ApplyRequest> {
            None
        }
        fn update_failure_is_boxed(&self, step_kinds: &[String]) -> bool {
            step_kinds.is_empty()
        }
    }

    #[test]
    fn a_window_answers_the_programs_calls_through_the_trait() {
        assert!(<FakeWindow as DrivenWindow>::plot_format_known("tiff"));
        assert!(!<FakeWindow as DrivenWindow>::plot_format_known("svg"));
        let mut fake = FakeWindow::default();
        let window: &mut dyn DrivenWindow = &mut fake;
        window.set_headless(true);
        window.load_default_customisation();
        assert!(window.run_verb_line("GRID ON").ok);
        assert_eq!(window.script_command_line("a.kcs"), "SCRIPT \"a.kcs\"");
        assert!(window.trigger_action("formatStyles").is_ok());
        assert_eq!(
            window
                .trigger_action("nothing")
                .expect_err("an unknown action")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(window.menus()[0].report(), "Format: Styles...");
        let mut err = Captured::default();
        assert!(window.check_tool_bar_fit(&[(1366, 768)], &mut err));
        assert_eq!(err.err, "toolbar fit 1366x768: 0 toolbars along the top\n");
        let mut harness = Harness::new();
        harness
            .run(|ui| window.ui(ui))
            .expect("the window is drawn");
        assert!(fake.headless);
        assert_eq!(
            fake.calls,
            ["loadDefaultCustomisation", "GRID ON", "formatStyles"]
        );
    }

    // ---- the views a dialog reaches -------------------------------------------------------------

    struct FakeViews {
        views: Rc<RefCell<ViewSet>>,
        zoomed: Rc<RefCell<Vec<Box2>>>,
        picks: Vec<Option<Option<Point2>>>,
    }

    impl ViewAccess for FakeViews {
        fn view_set(&self) -> Rc<RefCell<ViewSet>> {
            Rc::clone(&self.views)
        }
        fn zoom_to(&mut self, area: &Box2) {
            self.zoomed.borrow_mut().push(*area);
        }
        fn zoom_extents_all(&mut self) {}
        fn zoom_to_selection(&mut self, _view: ViewId) {}
        fn active_plan_view(&self) -> Option<ViewId> {
            self.views
                .borrow()
                .most_recent(ViewKind::Plan)
                .map(|state| state.id)
        }
        fn plan_views(&self) -> Vec<ViewId> {
            self.views
                .borrow()
                .views()
                .iter()
                .filter(|state| state.kind == ViewKind::Plan)
                .map(|state| state.id)
                .collect()
        }
        fn refresh_all(&mut self) {}
        fn pick_point(&mut self) -> PickId {
            // Asking again ends a pick still waiting with no point.
            if let Some(waiting) = self.picks.last_mut()
                && waiting.is_none()
            {
                *waiting = Some(None);
            }
            self.picks.push(None);
            self.picks.len() as PickId
        }
        fn picked_point(&mut self, pick: PickId) -> Option<Option<Point2>> {
            self.picks
                .get_mut(pick as usize - 1)
                .and_then(|answer| answer.take())
        }
    }

    #[test]
    fn a_dialog_reaches_the_views_through_its_host() {
        let views = Rc::new(RefCell::new(ViewSet::new()));
        let plan = views.borrow_mut().add(ViewKind::Plan).id;
        let zoomed = Rc::new(RefCell::new(Vec::new()));
        let fake = FakeViews {
            views: Rc::clone(&views),
            zoomed: Rc::clone(&zoomed),
            picks: Vec::new(),
        };
        let mut host = RecordingHost::new().with_views(Box::new(fake));
        let access = host.views().expect("the host has views");
        assert_eq!(access.active_plan_view(), Some(plan));
        assert_eq!(access.plan_views(), [plan]);
        let offered = scope_filter_views(&access.view_set().borrow());
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].title, "Plan 1");
        let area = Box2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 5.0));
        access.zoom_to(&area);
        assert_eq!(*zoomed.borrow(), [area]);
        let first = access.pick_point();
        assert_eq!(access.picked_point(first), None, "waiting");
        let second = access.pick_point();
        assert_eq!(
            access.picked_point(first),
            Some(None),
            "ended by the second"
        );
        assert_eq!(access.picked_point(second), None);
    }

    // ---- a workbench ----------------------------------------------------------------------------

    #[derive(Default)]
    struct FakeWorkbench {
        open: bool,
        frame: Option<DialogFrame>,
        ran: Vec<String>,
    }

    impl Workbench for FakeWorkbench {
        fn menus(&self) -> Vec<Menu> {
            vec![
                Menu::new("surveyMenu", "&Survey")
                    .action(
                        Action::new("surveyInverse", "&Inverse...")
                            .tip("The bearing and distance between two points")
                            .opens("surveyInverseDialog"),
                    )
                    .action(Action::new("surveyTraverse", "&Traverse").enabled(false)),
            ]
        }
        fn tool_bars(&self) -> Vec<ToolBar> {
            Vec::new()
        }
        fn docks(&self) -> Vec<DockSpec> {
            vec![DockSpec::new("surveyPointManagerDock", "Points")]
        }
        fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
            if !line.starts_with("INVERSE") {
                return false;
            }
            self.ran.push(String::from(line));
            host.log("inverse bearing=45", false);
            true
        }
        fn trigger(&mut self, _host: &mut dyn Host, name: &str) -> bool {
            if name != "surveyInverse" {
                return false;
            }
            self.open = true;
            true
        }
        fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
            if !self.open {
                return;
            }
            let ctx = ui.ctx().clone();
            let frame = self
                .frame
                .get_or_insert_with(|| DialogFrame::new("surveyInverseDialog", "Inverse"));
            let shown = frame.show_body(&ctx, host, |ui, host| {
                if katana_window::drive::button(ui, "surveyInverseRun", "Run").clicked() {
                    host.run("INVERSE 1 2");
                }
            });
            if shown.closed {
                self.open = false;
            }
        }
        fn dock_ui(&mut self, ui: &mut Ui, _host: &mut dyn Host, dock: &str) {
            katana_window::drive::label(ui, "status", &format!("{dock}: 0 points"));
        }
    }

    #[test]
    fn a_workbench_is_installed_in_the_stand_in_window_and_driven_by_its_names() {
        let mut host = RecordingHost::new();
        let mut window = WorkbenchHost::new(FakeWorkbench::default());
        assert_eq!(
            window
                .action("surveyInverse")
                .and_then(|action| action.dialog),
            Some(String::from("surveyInverseDialog"))
        );
        window
            .trigger(&mut host, "surveyInverse")
            .expect("the action is triggered");
        assert_eq!(
            window
                .trigger(&mut host, "surveyTraverse")
                .expect_err("a disabled item")
                .code,
            ErrorCode::InvalidState
        );
        assert_eq!(
            window
                .trigger(&mut host, "nothing")
                .expect_err("no such item")
                .code,
            ErrorCode::NotFound
        );
        let mut harness = Harness::new();
        harness
            .run(|ui| window.ui(ui, &mut host))
            .expect("the window is drawn");
        let dialog = harness
            .target_record("surveyInverseDialog")
            .expect("the dialog the action opened is a target");
        assert_eq!(dialog.title, "Inverse");
        assert_eq!(
            harness
                .snapshot()
                .control("surveyPointManagerDock", "status")
                .map(|status| status.text.clone()),
            Some(String::from("surveyPointManagerDock: 0 points"))
        );
        harness
            .target("surveyInverseDialog")
            .press("surveyInverseRun")
            .run(|ui| window.ui(ui, &mut host))
            .expect("Run is pressed");
        assert_eq!(host.lines(), ["INVERSE 1 2"]);
        assert!(window.run_line(&mut host, "INVERSE 3 4"));
        assert!(!window.run_line(&mut host, "LINE 0,0 1,1"));
        assert_eq!(window.workbench.ran, ["INVERSE 3 4"]);
    }

    // ---- jobs, the geo phase, the clipboard and the settings ------------------------------------

    #[test]
    fn a_job_applies_with_its_host_and_its_end_is_reported_as_jobs_cpp_reports_it() {
        let mut host = RecordingHost::new();
        let finished = job_finished(|report, host: &mut dyn Host| {
            host.log(&format!("{} {:?}", report.title, report.outcome), false);
        });
        let id = host.jobs().start(
            "Triangulating",
            job_work(|control: &JobControl| {
                control.set_progress(0.5);
                control.set_stage("Triangulating");
                Ok(Some(job_apply(|host: &mut dyn Host| {
                    host.log("applied", false);
                })))
            }),
            Some(finished),
        );
        let progress = host.jobs().progress();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].fraction, 0.5);
        assert_eq!(progress[0].stage, "Triangulating");
        assert!(host.jobs().is_active(id));
        host.wait_for(id);
        assert!(!host.jobs().is_active(id));
        let logged: Vec<&str> = host
            .logged()
            .iter()
            .map(|(text, _)| text.as_str())
            .collect();
        assert_eq!(logged, ["applied", "Triangulating Finished"]);

        // A failed work, a cancelled one, and an apply that panics.
        let failed = host.jobs().start(
            "Failing",
            job_work(|_| {
                Err(katana_core::Error::new(
                    ErrorCode::InvalidState,
                    "no points",
                ))
            }),
            Some(job_finished(|report, host: &mut dyn Host| {
                host.log(&report.error, true);
            })),
        );
        host.wait_for(failed);
        assert_eq!(
            host.logged().last().map(|(text, _)| text.as_str()),
            Some("InvalidState: no points")
        );
        let cancelled = host.jobs().start(
            "Cancelled",
            job_work(|_| {
                Ok(Some(job_apply(|host: &mut dyn Host| {
                    host.log("must not apply", false);
                })))
            }),
            Some(job_finished(|report, host: &mut dyn Host| {
                host.log(&format!("{:?}", report.outcome), false);
            })),
        );
        assert!(host.jobs().cancel(cancelled));
        host.wait_for(cancelled);
        assert_eq!(
            host.logged().last().map(|(text, _)| text.as_str()),
            Some("Cancelled")
        );
        let panicking = host.jobs().start(
            "Panicking",
            job_work(|_| {
                Ok(Some(job_apply(|_: &mut dyn Host| {
                    panic!("boom");
                })))
            }),
            Some(job_finished(|report, host: &mut dyn Host| {
                assert_eq!(report.outcome, JobOutcome::Failed);
                host.log(&report.error, true);
            })),
        );
        host.wait_for(panicking);
        assert_eq!(
            host.logged().last().map(|(text, _)| text.as_str()),
            Some("applying the result failed: boom")
        );
    }

    #[test]
    fn a_geo_phase_is_lent_the_hosts_drawing_for_the_call() {
        let mut host = RecordingHost::new();
        let revision = with_geo(&mut host, |context| context.document.model_revision());
        assert_eq!(revision, host.document().model_revision());
    }

    #[test]
    fn the_clipboard_and_the_settings_are_the_hosts_own() {
        let mut host = RecordingHost::new();
        host.clipboard()
            .set_text("12.5, 7.25")
            .expect("text is copied");
        assert_eq!(host.clipboard().text().as_deref(), Some("12.5, 7.25"));
        let mut image = Image::new(2, 2);
        image.fill(Color::rgb(1, 2, 3));
        host.clipboard()
            .set_image(&image)
            .expect("an image is copied");
        assert!(
            host.clipboard().text().is_none(),
            "a clipboard holds one thing"
        );
        assert_eq!(host.clipboard().image(), Some(image));

        let settings = host.settings();
        settings.set_text("online/keys/nearmap", "secret");
        settings.set_text("online/keys/mapbox", "token");
        settings.set_flag("start/showAtStart", false);
        settings.set_number("textSize", 2.0);
        settings.set_bytes("window/layout", vec![0, 1, 254, 255]);
        settings.set_list("scripts/recent", vec![String::from("a.kcs")]);
        assert_eq!(settings.child_keys("online/keys"), ["mapbox", "nearmap"]);
        let read = UserSettings::from_json(settings.to_json().as_bytes()).expect("they read back");
        assert_eq!(read.text("online/keys/nearmap"), Some("secret"));
        assert_eq!(read.flag("start/showAtStart"), Some(false));
        assert_eq!(read.number("textSize"), Some(2.0));
        assert_eq!(read.bytes("window/layout"), Some(&[0, 1, 254, 255][..]));
        assert_eq!(
            read.value("scripts/recent"),
            Some(&SettingValue::List(vec![String::from("a.kcs")]))
        );

        let scratch = Scratch::new("user-settings");
        let file = scratch.path("katana").join("settings.json");
        let mut kept = UserSettings::open(file.clone()).expect("a missing file is no settings yet");
        assert!(kept.text("start/coordinateSystem").is_none());
        kept.set_text("start/coordinateSystem", "EPSG:7856");
        kept.save().expect("the settings are written");
        let again = UserSettings::open(file).expect("the settings read back");
        assert_eq!(again.text("start/coordinateSystem"), Some("EPSG:7856"));
        assert!(
            UserSettings::in_memory().save().is_ok(),
            "in memory, nothing is written"
        );
    }

    #[test]
    fn an_image_written_as_png_reads_back_as_it_was() {
        let scratch = Scratch::new("png-round-trip");
        let mut image = Image::new(4, 3);
        image.set_pixel(3, 2, Color::rgb(0x4c, 0x9f, 0xfe));
        let file = scratch.path("shot.png");
        write_png(&image, &file).expect("the PNG is written");
        let read = read_png(&file).expect("the PNG reads back");
        assert_eq!(read.pixel(3, 2), Color::rgb(0x4c, 0x9f, 0xfe));
        assert_eq!(read.pixel(0, 0), Color::TRANSPARENT);
    }

    // ---- the canvas -----------------------------------------------------------------------------

    /// Metrics of a font whose every character is half its pixel size wide.
    struct FixedMetrics;

    impl FixedMetrics {
        fn pixels(font: &Font) -> f64 {
            match font.size {
                katana_window::canvas::FontSize::Pixels(pixels) => f64::from(pixels),
                katana_window::canvas::FontSize::Points(points) => points * 96.0 / 72.0,
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

    /// A canvas that records what it was asked to draw, with QPainter's state and its stack.
    struct RecordingCanvas {
        state: (Pen, Brush, Font, Transform, f64),
        saved: Vec<(Pen, Brush, Font, Transform, f64)>,
        drawn: Vec<String>,
        metrics: FixedMetrics,
    }

    impl RecordingCanvas {
        fn new() -> Self {
            Self {
                state: (
                    Pen::default(),
                    Brush::none(),
                    Font::default(),
                    Transform::IDENTITY,
                    1.0,
                ),
                saved: Vec::new(),
                drawn: Vec::new(),
                metrics: FixedMetrics,
            }
        }
    }

    impl Canvas for RecordingCanvas {
        fn device_size(&self) -> SizeF {
            SizeF::new(100.0, 100.0)
        }
        fn device_pixel_ratio(&self) -> f64 {
            1.0
        }
        fn save(&mut self) {
            self.saved.push(self.state.clone());
        }
        fn restore(&mut self) {
            if let Some(state) = self.saved.pop() {
                self.state = state;
            }
        }
        fn pen(&self) -> Pen {
            self.state.0.clone()
        }
        fn set_pen(&mut self, pen: Pen) {
            self.state.0 = pen;
        }
        fn brush(&self) -> Brush {
            self.state.1.clone()
        }
        fn set_brush(&mut self, brush: Brush) {
            self.state.1 = brush;
        }
        fn font(&self) -> Font {
            self.state.2.clone()
        }
        fn set_font(&mut self, font: Font) {
            self.state.2 = font;
        }
        fn render_hint(&self, _hint: RenderHint) -> bool {
            false
        }
        fn set_render_hint(&mut self, _hint: RenderHint, _on: bool) {}
        fn opacity(&self) -> f64 {
            self.state.4
        }
        fn set_opacity(&mut self, opacity: f64) {
            self.state.4 = opacity;
        }
        fn world_transform(&self) -> Transform {
            self.state.3
        }
        fn set_world_transform(&mut self, transform: Transform, combine: bool) {
            self.state.3 = if combine {
                transform * self.state.3
            } else {
                transform
            };
        }
        fn device_transform(&self) -> Transform {
            self.state.3
        }
        fn set_clip_rect(&mut self, _rect: RectF, _intersect: bool) {}
        fn set_clip_path(&mut self, _path: &CanvasPath, _intersect: bool) {}
        fn set_clipping(&mut self, _on: bool) {}
        fn has_clipping(&self) -> bool {
            false
        }
        fn draw_path(&mut self, path: &CanvasPath) {
            self.drawn.push(format!("path {}", path.elements.len()));
        }
        fn fill_path(&mut self, path: &CanvasPath, _brush: &Brush) {
            self.drawn.push(format!("fill {}", path.elements.len()));
        }
        fn stroke_path(&mut self, path: &CanvasPath, _pen: &Pen) {
            self.drawn.push(format!("stroke {}", path.elements.len()));
        }
        fn draw_text(&mut self, _origin: PointF, text: &str) {
            self.drawn.push(format!("text {text}"));
        }
        fn draw_text_in(&mut self, rect: RectF, _flags: TextFlags, text: &str) -> RectF {
            self.drawn.push(format!("text {text}"));
            rect
        }
        fn draw_image(&mut self, _target: RectF, image: &Image, _source: IntRect) {
            self.drawn
                .push(format!("image {}x{}", image.width(), image.height()));
        }
        fn metrics(&self) -> &dyn FontMetrics {
            &self.metrics
        }
    }

    #[test]
    fn a_canvas_draws_every_shape_through_its_paths_and_keeps_qpainters_state() {
        let mut canvas = RecordingCanvas::new();
        let painter: &mut dyn Canvas = &mut canvas;
        painter.save();
        painter.translate(10.0, 20.0);
        painter.scale(2.0, 2.0);
        assert_eq!(
            painter.world_transform().map(PointF::new(1.0, 1.0)),
            PointF::new(12.0, 22.0)
        );
        painter.draw_line(PointF::new(0.0, 0.0), PointF::new(1.0, 1.0));
        painter.draw_rect(RectF::new(0.0, 0.0, 2.0, 1.0));
        painter.draw_ellipse(RectF::new(0.0, 0.0, 4.0, 4.0));
        painter.draw_polygon(
            &[
                PointF::new(0.0, 0.0),
                PointF::new(1.0, 0.0),
                PointF::new(0.0, 1.0),
            ],
            FillRule::OddEven,
        );
        painter.fill_rect(RectF::new(0.0, 0.0, 1.0, 1.0), &Brush::solid(Color::WHITE));
        painter.draw_text(PointF::new(0.0, 0.0), "CH 100");
        painter.draw_image_at(PointF::new(0.0, 0.0), &Image::new(3, 2));
        painter.restore();
        assert!(painter.world_transform().is_identity());
        assert_eq!(
            painter
                .metrics()
                .horizontal_advance(&Font::default(), "abc"),
            18.0
        );
        assert_eq!(
            canvas.drawn,
            [
                // A line: a move and a line, stroked.
                "stroke 2",
                // A rectangle: a move, four lines and its close.
                "path 6",
                // An ellipse: a move, four quarter curves and its close.
                "path 6",
                // A polygon: a move, two lines, the line back and its close.
                "path 5",
                "fill 6",
                "text CH 100",
                "image 3x2",
            ]
        );
    }
}
