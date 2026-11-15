//! The twins of tests/qt_widgets/test_view_scale.cpp: a plan view's paper-style scale. Paper marks
//! keep their screen size at Auto and take the denominator at a fixed scale, whatever the zoom;
//! ground marks and plots ignore it; the kept drawing is painted again when only the scale moves;
//! and the Paper Style Scale dialog only builds the VIEWS SCALE line, which the view's bar opens
//! and the shared command carries out, undoably.

mod view_scale {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use katana_cad::view_set::{ViewSet, set_view_paper_scales};
    use katana_cad::viewport_layout::ViewKind;
    use katana_cad::{Document, PlotSettings};
    use katana_commands::{EntityAttributes, create_point};
    use katana_entity::{
        Entity, Geometry, LineStyle, Model, PointGeometry, Stroke, StrokeOp, Style, StyleLibrary,
        StyleUnits,
    };
    use katana_geometry::{Point2, Segment2};
    use katana_window::canvas::{Canvas, Color, Image, RasterCanvas};
    use katana_window::drive::Harness;
    use katana_window::host::{Answer, RecordingHost, VerbOutcome};
    use katana_window::plan_painter::{
        PlanFrame, PlanMedium, PlanPaintCache, PlanPaintOptions, PlanPaintStats, PlanSource,
        paint_plan,
    };
    use katana_window::view_scale_dialog::ViewScaleDialog;
    use katana_window::view_workspace::ViewWorkspace;
    use katana_window::viewport_widget::PlanView;

    use crate::support::Site;

    /// One mark, a 2 mm square symbol or a 4 mm linestyle with a 2 mm tick, painted on black into
    /// a 200 x 200 view at 20 px a metre, 4 px a paper millimetre, thin lines.
    struct Mark {
        model: Model,
        library: StyleLibrary,
        scale: f64,
        paper_style_scale: Option<f64>,
        on_paper: bool,
        symbol_sprites: bool,
        cache: PlanPaintCache,
        stats: PlanPaintStats,
    }

    impl Mark {
        /// `built_in` names one of the built-in shapes for the style's symbol in place of the
        /// library's "test mark"; `symbol_size` is Style::symbol_size, a width in model units.
        fn new(units: StyleUnits, linestyle: bool, built_in: &str, symbol_size: f64) -> Self {
            let mut definition = LineStyle {
                name: String::from("test mark"),
                units,
                ..LineStyle::default()
            };
            let stroke = |op: StrokeOp, x: f64, y: f64| Stroke {
                op,
                point: Point2::new(x, y),
                ..Stroke::default()
            };
            if linestyle {
                definition.length = 4.0;
                definition.strokes = vec![
                    stroke(StrokeOp::Move, 0.0, 0.0),
                    stroke(StrokeOp::Draw, 0.0, 2.0),
                ];
            } else {
                definition.symbol = true;
                definition.strokes = vec![
                    stroke(StrokeOp::Move, -1.0, -1.0),
                    stroke(StrokeOp::Draw, 1.0, -1.0),
                    stroke(StrokeOp::Draw, 1.0, 1.0),
                    stroke(StrokeOp::Draw, -1.0, 1.0),
                    stroke(StrokeOp::Draw, -1.0, -1.0),
                ];
            }
            let mut library = StyleLibrary::default();
            assert!(library.add(definition).is_ok());
            let mut style = Style {
                name: String::from("marks"),
                ..Style::default()
            };
            if linestyle {
                style.linetype = String::from("test mark");
            } else {
                style.symbol = if built_in.is_empty() {
                    String::from("test mark")
                } else {
                    String::from(built_in)
                };
                style.symbol_size = symbol_size;
            }
            let mut model = Model::default();
            assert!(model.styles.add(style).is_ok());
            let geometry = if linestyle {
                Geometry::Line(Segment2::new(Point2::new(-2.0, 0.0), Point2::new(2.0, 0.0)))
            } else {
                Geometry::Point(PointGeometry {
                    position: Point2::new(0.0, 0.0),
                })
            };
            let entity = Entity {
                style: String::from("marks"),
                geometry,
                ..Entity::default()
            };
            assert!(model.entities.add(entity).is_ok());
            Self {
                model,
                library,
                scale: 20.0,
                paper_style_scale: None,
                on_paper: false,
                symbol_sprites: false,
                cache: PlanPaintCache::new(),
                stats: PlanPaintStats::default(),
            }
        }

        fn symbol() -> Self {
            Self::new(StyleUnits::Paper, false, "", 0.0)
        }

        fn paint(&mut self) -> Image {
            let mut image = Image::new(200, 200);
            image.fill(Color::BLACK);
            let source = PlanSource {
                model: Some(&self.model),
                library: Some(&self.library),
                library_generation: 1,
                ..PlanSource::default()
            };
            let mut frame = PlanFrame::default();
            frame.transform.resize(200.0, 200.0);
            frame.transform.scale = self.scale;
            let plot = PlotSettings::default();
            let options = PlanPaintOptions {
                pixels_per_millimetre: 4.0,
                thin_lines: true,
                paper_style_scale: self.paper_style_scale,
                symbol_sprites: self.symbol_sprites,
                medium: if self.on_paper {
                    PlanMedium::Paper
                } else {
                    PlanMedium::Screen
                },
                plot: self.on_paper.then_some(&plot),
                ..PlanPaintOptions::default()
            };
            let cache = &mut self.cache;
            let mut stats = PlanPaintStats::default();
            RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
                stats = paint_plan(canvas, &source, &frame, &options, cache);
            });
            self.stats = stats;
            image
        }
    }

    /// qGray: (r * 11 + g * 16 + b * 5) / 32.
    fn grey(colour: Color) -> u32 {
        (u32::from(colour.r) * 11 + u32::from(colour.g) * 16 + u32::from(colour.b) * 5) / 32
    }

    /// The pixels inked: their bounding width and height, None for none.
    fn ink_bounds(image: &Image) -> Option<(u32, u32)> {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for y in 0..image.height() {
            for x in 0..image.width() {
                if grey(image.pixel(x, y)) > 0 {
                    bounds = Some(match bounds {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
        }
        bounds.map(|(x0, y0, x1, y1)| (x1 - x0 + 1, y1 - y0 + 1))
    }

    /// A one-pixel pen plus antialiasing can reach the neighbouring pixel on either edge.
    fn expect_size(image: &Image, width: f64, height: f64) {
        let (inked_width, inked_height) = ink_bounds(image).expect("something is inked");
        assert!(
            (f64::from(inked_width) - width).abs() <= 2.0,
            "{inked_width} wide, not {width}"
        );
        assert!(
            (f64::from(inked_height) - height).abs() <= 2.0,
            "{inked_height} high, not {height}"
        );
    }

    fn height_of(image: &Image) -> f64 {
        f64::from(ink_bounds(image).expect("something is inked").1)
    }

    #[test]
    fn automatic_paper_symbols_keep_their_screen_size_when_zoom_changes() {
        let mut mark = Mark::symbol();
        // The square is 2 mm across: Auto at 4 px/mm is 8 px at either zoom.
        expect_size(&mark.paint(), 8.0, 8.0);
        mark.scale = 40.0;
        expect_size(&mark.paint(), 8.0, 8.0);
    }

    #[test]
    fn fixed_paper_symbols_use_the_denominator_and_zoom_independently() {
        let mut mark = Mark::symbol();
        // 2 mm at 1:500 = 1 m; at 20 px/m it is 20 px across.
        mark.paper_style_scale = Some(500.0);
        expect_size(&mark.paint(), 20.0, 20.0);
        mark.paper_style_scale = Some(1000.0);
        expect_size(&mark.paint(), 40.0, 40.0);
        mark.paper_style_scale = Some(500.0);
        mark.scale = 40.0;
        expect_size(&mark.paint(), 40.0, 40.0);
        mark.paper_style_scale = None;
        expect_size(&mark.paint(), 8.0, 8.0);
    }

    #[test]
    fn paper_linestyle_ticks_use_the_same_reference_scale_as_symbols() {
        let mut mark = Mark::new(StyleUnits::Paper, true, "", 0.0);
        mark.paper_style_scale = Some(500.0);
        // The tick is 2 mm high: 2 * 500 / 1000 m * 20 px/m = 20 px.
        assert!((height_of(&mark.paint()) - 20.0).abs() <= 2.0);
        mark.paper_style_scale = Some(1000.0);
        assert!((height_of(&mark.paint()) - 40.0).abs() <= 2.0);
        mark.paper_style_scale = None;
        assert!((height_of(&mark.paint()) - 8.0).abs() <= 2.0);
    }

    #[test]
    fn ground_sized_marks_and_plot_paper_marks_ignore_the_screen_reference_scale() {
        for linestyle in [false, true] {
            let mut ground = Mark::new(StyleUnits::World, linestyle, "", 0.0);
            let before = ground.paint();
            ground.paper_style_scale = Some(500.0);
            assert!(ground.paint() == before);
            ground.paper_style_scale = Some(2000.0);
            assert!(ground.paint() == before);

            let mut paper = Mark::new(StyleUnits::Paper, linestyle, "", 0.0);
            paper.on_paper = true;
            let plotted = paper.paint();
            paper.paper_style_scale = Some(500.0);
            assert!(paper.paint() == plotted);
            paper.paper_style_scale = Some(2000.0);
            assert!(paper.paint() == plotted);
        }
    }

    // A built-in shape with no size of its own plots as a paper mark, 1 mm either side, so a fixed
    // scale sizes it as the plot does.
    #[test]
    fn a_built_in_symbol_with_no_size_is_sized_by_the_fixed_scale_as_the_plot_sizes_it() {
        let mut mark = Mark::new(StyleUnits::Paper, false, "square", 0.0);
        // Auto: the plain mark's 4 px either side, 8 px across, at either zoom.
        expect_size(&mark.paint(), 8.0, 8.0);
        // 1:500: 1 mm is 0.5 m, so 1 m across, 20 px at 20 px/m.
        mark.paper_style_scale = Some(500.0);
        expect_size(&mark.paint(), 20.0, 20.0);
        // 1:1000: 2 m across, 40 px; zooming to 40 px/m makes it 80 px.
        mark.paper_style_scale = Some(1000.0);
        expect_size(&mark.paint(), 40.0, 40.0);
        mark.scale = 40.0;
        expect_size(&mark.paint(), 80.0, 80.0);
        mark.paper_style_scale = None;
        expect_size(&mark.paint(), 8.0, 8.0);
        // On paper the screen's scale changes nothing.
        let mut plotted = Mark::new(StyleUnits::Paper, false, "square", 0.0);
        plotted.on_paper = true;
        let before = plotted.paint();
        plotted.paper_style_scale = Some(500.0);
        assert!(plotted.paint() == before);
    }

    // A style with a symbol size, a width on the ground: the plot draws it at that width at any
    // scale, so the screen's paper scale leaves it as it is: 2 m across is 40 px at 20 px/m.
    #[test]
    fn a_symbol_with_a_ground_size_keeps_it_whatever_the_fixed_scale() {
        for built_in in ["", "square"] {
            let mut mark = Mark::new(StyleUnits::World, false, built_in, 2.0);
            let automatic = mark.paint();
            expect_size(&automatic, 40.0, 40.0);
            for denominator in [500.0, 2000.0] {
                mark.paper_style_scale = Some(denominator);
                assert!(mark.paint() == automatic, "1:{denominator} {built_in}");
            }
        }
    }

    #[test]
    fn a_symbol_sprite_is_rebuilt_when_only_the_reference_scale_changes() {
        let mut mark = Mark::symbol();
        mark.symbol_sprites = true;
        mark.paper_style_scale = Some(500.0);
        expect_size(&mark.paint(), 20.0, 20.0);
        assert!(mark.stats.sprites_drawn > 0);
        mark.paper_style_scale = Some(1000.0);
        let changed = mark.paint();
        expect_size(&changed, 40.0, 40.0);
        let mut fresh = Mark::symbol();
        fresh.symbol_sprites = true;
        fresh.paper_style_scale = Some(1000.0);
        assert!(changed == fresh.paint());
    }

    #[test]
    fn the_kept_plan_image_invalidates_when_only_its_paper_style_scale_changes() {
        let mut document = Document::new(None);
        assert!(
            document
                .execute(create_point(
                    Point2::new(0.0, 0.0),
                    EntityAttributes::default()
                ))
                .is_ok()
        );
        let mut views = ViewSet::new();
        let state = views.add(ViewKind::Plan);
        state.plan_framed = true;
        state.plan.scale = 20.0;
        let mut widget = PlanView::new(&document, state);
        widget.resize(state, 200, 200);
        let _ = widget.paint(&document, state);
        let paints = widget.drawing_paint_count();
        state.paper_style_scale = Some(500.0);
        let _ = widget.paint(&document, state);
        assert_eq!(widget.drawing_paint_count(), paints + 1);
        let _ = widget.paint(&document, state);
        assert_eq!(widget.drawing_paint_count(), paints + 1);
        state.paper_style_scale = None;
        let _ = widget.paint(&document, state);
        assert_eq!(widget.drawing_paint_count(), paints + 2);
        assert_eq!(state.plan.scale, 20.0);
    }

    /// The dialog over `views`, on view `view`, shown in a harness of its own, with a host that
    /// answers every line `answer`.
    struct Builder {
        dialog: ViewScaleDialog,
        host: RecordingHost,
        harness: Harness,
    }

    impl Builder {
        fn new(views: Rc<RefCell<ViewSet>>, view: u32, answer: VerbOutcome) -> Self {
            let mut dialog = ViewScaleDialog::new(views, view);
            dialog.show();
            let mut builder = Self {
                dialog,
                host: RecordingHost::new().answering(Answer::Fixed(answer)),
                harness: Harness::new(),
            };
            builder.harness.target("viewScaleDialog");
            builder.draw();
            builder
        }

        fn draw(&mut self) {
            let (dialog, host) = (&mut self.dialog, &mut self.host);
            self.harness
                .run(|ui| dialog.ui(ui, host))
                .unwrap_or_else(|problem| panic!("{problem}"));
        }
    }

    #[test]
    fn the_dialog_only_builds_an_explicit_views_scale_line_and_reports_its_result() {
        let views = Rc::new(RefCell::new(ViewSet::new()));
        views.borrow_mut().add(ViewKind::Plan);
        views.borrow_mut().add(ViewKind::Model3D);
        views.borrow_mut().add(ViewKind::Plan);
        let mut b = Builder::new(
            Rc::clone(&views),
            3,
            VerbOutcome::new(true, "view=3 kind=plan paper_scale=500 changed=yes", ""),
        );
        assert!(!b.dialog.is_modal());
        for name in [
            "viewScaleView",
            "viewScaleMode",
            "viewScaleDenominator",
            "viewScaleApply",
            "viewScaleStatus",
        ] {
            assert!(b.harness.control(name).is_some(), "{name}");
        }
        let target = b.harness.control("viewScaleView").expect("the view choice");
        assert_eq!(target.items.len(), 2, "a model view cannot choose paper-style scale");
        assert!(target.value.ends_with("(view 3)"), "{}", target.value);

        b.harness
            .fill("viewScaleMode", "Fixed 1:N")
            .fill("viewScaleDenominator", "500")
            .press("viewScaleApply");
        b.draw();
        assert_eq!(b.host.lines(), ["VIEWS SCALE 3 500"]);
        assert!(b.dialog.status().contains("paper_scale=500"));
        assert!(
            views.borrow().find(3).and_then(|state| state.paper_style_scale).is_none(),
            "the dialog does not edit"
        );
        b.harness.fill("viewScaleDenominator", "nan").press("viewScaleApply");
        b.draw();
        assert_eq!(b.host.lines().len(), 1);
        assert!(!b.dialog.status().is_empty());
        b.harness.fill("viewScaleMode", "Auto (fixed screen size)").press("viewScaleApply");
        b.draw();
        assert_eq!(b.host.lines().last().map(String::as_str), Some("VIEWS SCALE 3 AUTO"));
    }

    #[test]
    fn the_view_bar_runs_the_shared_scale_command_and_undo_restores_its_display_state() {
        let mut w = Site::new();
        assert!(w.bar(1).shown_tools().contains(&String::from("ViewScaleButton")));
        w.press(1, "ViewScaleButton");
        assert!(w.harness.target_record("viewScaleDialog").is_some());
        w.harness
            .target("viewScaleDialog")
            .fill("viewScaleMode", "Fixed 1:N")
            .fill("viewScaleDenominator", "500")
            .press("viewScaleApply");
        w.frame();
        assert_eq!(w.state(1).paper_style_scale, Some(500.0));
        assert_eq!(w.document().history().undo_count(), 1);
        assert!(w.tip(1, "ViewScaleButton").contains("1:500"));

        assert!(w.document_mut().undo().is_ok());
        w.frame();
        assert!(w.state(1).paper_style_scale.is_none());
        assert_eq!(
            w.harness.report("viewScaleMode").as_deref(),
            Ok("Auto (fixed screen size)")
        );
        assert!(w.tip(1, "ViewScaleButton").contains("Auto"));
        assert!(w.document_mut().redo().is_ok());
        w.frame();
        assert_eq!(w.state(1).paper_style_scale, Some(500.0));

        assert!(
            w.views
                .set_view_kind(w.host.document_mut(), 1, ViewKind::Model3D)
                .is_ok()
        );
        w.frame();
        assert!(!w.bar(1).shown_tools().contains(&String::from("ViewScaleButton")));
        assert!(
            w.views
                .set_view_kind(w.host.document_mut(), 1, ViewKind::Plan)
                .is_ok()
        );
        w.frame();
        assert_eq!(w.state(1).paper_style_scale, Some(500.0));
    }

    #[test]
    fn a_closed_target_is_refused_without_editing_another_view() {
        let views = Rc::new(RefCell::new(ViewSet::new()));
        views.borrow_mut().add(ViewKind::Plan);
        views.borrow_mut().add(ViewKind::Plan);
        let mut dialog = ViewScaleDialog::new(Rc::clone(&views), 1);
        let mut host = RecordingHost::new().answering(Answer::Fixed(VerbOutcome::new(true, "", "")));
        assert!(views.borrow_mut().remove(1).is_ok());
        assert!(!dialog.apply(&mut host));
        assert!(host.lines().is_empty());
        assert!(views.borrow().find(2).and_then(|state| state.paper_style_scale).is_none());
    }

    #[test]
    fn opening_and_replacing_a_drawing_restores_its_scale_without_moving_the_camera() {
        let mut document = Document::new(None);
        let scales = BTreeMap::from([(1, Some(500.0))]);
        assert!(set_view_paper_scales(&mut document, &scales).is_ok());
        let workspace = ViewWorkspace::new(&document);
        assert_eq!(
            workspace.state(1).and_then(|state| state.paper_style_scale),
            Some(500.0)
        );
        workspace.with_state(1, |view| {
            view.plan.center = Point2::new(4.0, 5.0);
            view.plan.scale = 8.0;
        });
        let mut metadata = document.metadata().clone();
        metadata.unknown_keys.remove("view_paper_scales");
        document.set_metadata(metadata);
        workspace.follow_drawing(&mut document);
        let view = workspace.state(1).expect("view 1 is open");
        assert!(view.paper_style_scale.is_none());
        assert_eq!(view.plan.center, Point2::new(4.0, 5.0));
        assert_eq!(view.plan.scale, 8.0);
    }
}
