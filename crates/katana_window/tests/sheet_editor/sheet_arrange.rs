//! The twins of tests/qt_widgets/plotting/test_sheet_arrange.cpp: the sheet editor's Arrange
//! commands, each one undoable step, and Choose Paper.

mod sheet_arrange_editor {
    use katana_cad::Document;
    use katana_cad::plot::{PaperSize, paper_dimensions};
    use katana_cad::plotting::layout::{TILING_GUTTER_MM, TilingPreset, preset_cells, tiling_area};
    use katana_cad::plotting::sheet_commands::{add_sheet, edit_sheet};
    use katana_cad::plotting::sheet_set::{Sheet, Viewport, ViewportKind};
    use katana_commands::{EntityAttributes, create_line};
    use katana_geometry::{Box2, Point2};
    use katana_window::canvas::{Canvas, Color, Image, RasterCanvas, write_png};
    use katana_window::drive::{Menu, MenuItem, ToolBarItem};
    use katana_window::sheet_painter::{SheetPaintCache, SheetPaintOptions, paint_sheet};

    use crate::support::{Shown, bx, near, viewport};

    const DEGREE: f64 = std::f64::consts::PI / 180.0;

    /// One sheet holding `viewports`, added as one step.
    fn add_sheet_with(document: &mut Document, viewports: Vec<Viewport>) {
        let sheet = Sheet {
            id: String::from("s1"),
            name: String::from("TEST"),
            viewports,
            ..Sheet::default()
        };
        add_sheet(document, sheet, None).expect("the sheet is added");
    }

    fn viewport_of(shown: &Shown, id: &str) -> Viewport {
        shown.by_id(0, id)
    }

    fn add_line(document: &mut Document, a: Point2, b: Point2) {
        document
            .execute(create_line(a, b, EntityAttributes::default()))
            .expect("a line");
    }

    /// A `length` x `width` strip along `turn`, centred on `centre`, drawn as its four sides; its
    /// corners.
    fn add_strip(
        document: &mut Document,
        centre: Point2,
        length: f64,
        width: f64,
        turn: f64,
    ) -> Vec<Point2> {
        let corners: Vec<Point2> = [
            Point2::new(-length / 2.0, -width / 2.0),
            Point2::new(length / 2.0, -width / 2.0),
            Point2::new(length / 2.0, width / 2.0),
            Point2::new(-length / 2.0, width / 2.0),
        ]
        .iter()
        .map(|corner| centre + corner.rotated(turn))
        .collect();
        for i in 0..corners.len() {
            add_line(document, corners[i], corners[(i + 1) % corners.len()]);
        }
        corners
    }

    fn overlap(a: &Box2, b: &Box2) -> bool {
        a.min.x < b.max.x - 1e-9
            && b.min.x < a.max.x - 1e-9
            && a.min.y < b.max.y - 1e-9
            && b.min.y < a.max.y - 1e-9
    }

    fn find_menu<'m>(items: &'m [MenuItem], name: &str) -> Option<&'m Menu> {
        items.iter().find_map(|item| match item {
            MenuItem::Menu(menu) if menu.name == name => Some(menu),
            _ => None,
        })
    }

    fn has_action(menu: &Menu, name: &str) -> bool {
        menu.actions().iter().any(|action| action.name == name)
    }

    fn last_message(shown: &Shown) -> String {
        shown.messages().last().cloned().unwrap_or_default()
    }

    #[test]
    fn the_arrange_menu_offers_every_command_by_name() {
        let mut shown = Shown::new(Document::new(None));
        let bar = shown.editor.tool_bar(&shown.host);
        assert!(
            bar.items
                .iter()
                .any(|item| *item == ToolBarItem::Widget(String::from("sheetArrangeButton")))
        );
        let menu = shown.editor.arrange_menu(&shown.host);
        assert_eq!(menu.name, "sheetArrangeMenu");
        for name in [
            "sheetArrangeAuto",
            "sheetAlignLeft",
            "sheetAlignRight",
            "sheetAlignTop",
            "sheetAlignBottom",
            "sheetAlignHCentre",
            "sheetAlignVCentre",
            "sheetDistributeHorizontal",
            "sheetDistributeVertical",
            "sheetFitToContent",
            "sheetRotateToBestFit",
        ] {
            assert!(has_action(&menu, name), "{name}");
        }
        assert!(find_menu(&menu.items, "sheetMatchScaleMenu").is_some());

        // The canvas's context menu has the same commands in its Arrange submenu.
        let items = shown.editor.context_menu_items(&shown.host, "");
        let arrange = find_menu(&items, "sheetContextArrangeMenu").expect("the Arrange submenu");
        assert!(has_action(arrange, "sheetRotateToBestFit"));
        assert!(has_action(arrange, "sheetArrangeAuto"));
        assert!(find_menu(&arrange.items, "sheetMatchScaleMenu").is_some());

        // With no sheet, a command says so and changes nothing.
        shown.trigger("sheetArrangeAuto");
        assert!(!shown.messages().is_empty());
        assert_eq!(shown.errors(), 1);
        assert!(last_message(&shown).contains("no sheet"));
    }

    #[test]
    fn auto_arrange_moves_a_panel_off_the_plan_in_one_step() {
        let mut document = Document::new(None);
        let main = preset_cells(
            TilingPreset::MainRight,
            &tiling_area(&Sheet::default()),
            TILING_GUTTER_MM,
        )[0];
        add_sheet_with(
            &mut document,
            vec![
                viewport("vp1", ViewportKind::Plan, main),
                viewport("vp2", ViewportKind::Legend, bx(100.0, 100.0, 180.0, 200.0)),
            ],
        );
        let mut shown = Shown::new(document);
        let before = shown.steps();

        shown.trigger("sheetArrangeAuto");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(viewport_of(&shown, "vp1").rect, main);
        assert!(!overlap(
            &viewport_of(&shown, "vp1").rect,
            &viewport_of(&shown, "vp2").rect
        ));
        assert_eq!(shown.errors(), 0);
        assert!(last_message(&shown).contains("moved 1 view"));

        shown.undo();
        assert_eq!(viewport_of(&shown, "vp2").rect, bx(100.0, 100.0, 180.0, 200.0));
    }

    fn three_notes() -> Vec<Viewport> {
        vec![
            viewport("vp1", ViewportKind::Notes, bx(30.0, 40.0, 80.0, 80.0)),
            viewport("vp2", ViewportKind::Notes, bx(100.0, 50.0, 130.0, 120.0)),
            viewport("vp3", ViewportKind::Notes, bx(300.0, 45.0, 360.0, 60.0)),
        ]
    }

    #[test]
    fn aligning_acts_on_the_selection_or_on_every_view() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, three_notes());
        let mut shown = Shown::new(document);
        let before = shown.steps();

        // Nothing selected: every view, on the highest top.
        shown.trigger("sheetAlignTop");
        assert_eq!(shown.steps(), before + 1);
        for id in ["vp1", "vp2", "vp3"] {
            assert_eq!(viewport_of(&shown, id).rect.max.y, 120.0, "{id}");
        }

        // One selected: to the tiling area's edge, 24 mm from the left.
        shown.canvas_mut().select("vp3");
        shown.trigger("sheetAlignLeft");
        assert_eq!(shown.steps(), before + 2);
        assert_eq!(viewport_of(&shown, "vp3").rect.min.x, 24.0);
        assert_eq!(viewport_of(&shown, "vp1").rect.min.x, 30.0);

        // Spacing with one selected: nothing between to space, and the views not chosen are not
        // moved - refused, as a message.
        shown.trigger("sheetDistributeHorizontal");
        assert_eq!(shown.steps(), before + 2);
        assert_eq!(shown.errors(), 1);
        assert!(last_message(&shown).contains("three views"), "{}", last_message(&shown));
        assert_eq!(viewport_of(&shown, "vp1").rect.min.x, 30.0);

        // Back as they were - vp1 at 30..80, vp2 at 100..130, vp3 at 300..360 - and nothing
        // selected, vp2 goes half way along the 220 mm between the others less its width.
        shown.undo();
        shown.undo();
        shown.canvas_mut().set_selection(&[], "");
        shown.trigger("sheetDistributeHorizontal");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.errors(), 1);
        let gap = (300.0 - 80.0 - 30.0) / 2.0;
        assert!(near(viewport_of(&shown, "vp2").rect.min.x, 80.0 + gap, 1e-9));
    }

    /// A group chosen on the canvas is arranged as a group, whichever of it is the primary:
    /// aligned together, and the view not chosen left where it is.
    #[test]
    fn a_group_on_the_canvas_is_arranged_as_a_group() {
        let mut document = Document::new(None);
        add_sheet_with(&mut document, three_notes());
        let mut shown = Shown::new(document);
        let before = shown.steps();
        shown
            .canvas_mut()
            .set_selection(&[String::from("vp1"), String::from("vp2")], "vp2");
        shown.trigger("sheetAlignTop");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(viewport_of(&shown, "vp1").rect.max.y, 120.0);
        assert_eq!(viewport_of(&shown, "vp2").rect.max.y, 120.0);
        assert_eq!(viewport_of(&shown, "vp3").rect.max.y, 60.0, "not chosen");
    }

    #[test]
    fn match_scale_lists_the_other_scaled_views_and_matches_the_selection() {
        // A 300 x 100 m drawing in an automatic plan filling the sheet, centred on it, is drawn at
        // 1 : 1000 (it needs 1 : 810 with the painter's 4% to spare).
        let mut document = Document::new(None);
        add_line(&mut document, Point2::new(0.0, 0.0), Point2::new(300.0, 100.0));
        let mut plan = viewport("vp1", ViewportKind::Plan, tiling_area(&Sheet::default()));
        plan.auto_scale = true;
        plan.auto_centre = true;
        plan.scale = 250.0;
        let mut section = viewport("vp2", ViewportKind::LongSection, bx(30.0, 40.0, 300.0, 120.0));
        section.scale = 200.0;
        add_sheet_with(
            &mut document,
            vec![
                plan,
                section,
                viewport("vp3", ViewportKind::Legend, bx(320.0, 40.0, 400.0, 100.0)),
            ],
        );
        let mut shown = Shown::new(document);
        shown.canvas_mut().select("vp2");

        let menu = shown.editor.match_scale_menu(&shown.host);
        assert_eq!(menu.name, "sheetMatchScaleMenu");
        // The plan is listed; the selected section and the legend are not.
        let actions = menu.actions();
        let from_plan = actions
            .iter()
            .find(|action| action.name == "sheetMatchScale_vp1")
            .expect("the plan is listed");
        assert!(from_plan.text.contains("1:1000"), "{}", from_plan.text);
        assert!(!actions.iter().any(|action| action.name == "sheetMatchScale_vp2"));
        assert!(!actions.iter().any(|action| action.name == "sheetMatchScale_vp3"));

        let before = shown.steps();
        shown.trigger("sheetMatchScale_vp1");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(viewport_of(&shown, "vp2").scale, 1000.0);
        assert_eq!(
            viewport_of(&shown, "vp1").scale,
            250.0,
            "the plan it came from is untouched"
        );
        assert_eq!(shown.errors(), 0);
    }

    #[test]
    fn fit_view_to_content_sizes_the_selected_plan() {
        // 100 x 50 m at 1 : 500 is 200 x 100 mm, and 5% more, about the centre.
        let mut document = Document::new(None);
        add_line(&mut document, Point2::new(0.0, 0.0), Point2::new(100.0, 50.0));
        let mut plan = viewport("vp1", ViewportKind::Plan, bx(100.0, 100.0, 200.0, 200.0));
        plan.scale = 500.0;
        add_sheet_with(
            &mut document,
            vec![
                plan,
                viewport("vp2", ViewportKind::Legend, bx(320.0, 40.0, 400.0, 100.0)),
            ],
        );
        let mut shown = Shown::new(document);
        shown.canvas_mut().select("vp1");
        shown.trigger("sheetFitToContent");
        let rect = viewport_of(&shown, "vp1").rect;
        assert!(near(rect.min.x, 45.0, 1e-9));
        assert!(near(rect.max.x, 255.0, 1e-9));
        assert!(near(rect.min.y, 97.5, 1e-9));
        assert!(near(rect.max.y, 202.5, 1e-9));
        assert_eq!(shown.errors(), 0);

        // A legend has nothing to fit.
        shown.canvas_mut().select("vp2");
        shown.trigger("sheetFitToContent");
        assert_eq!(shown.errors(), 1);
    }

    #[test]
    fn a_turned_plan_is_drawn_where_the_model_says_it_is() {
        // A 450 x 20 m strip along 45 degrees in a plan filling the sheet. Square it needs
        // 1 : 2000; turned, 1 : 1250 - and no further than that takes.
        let mut document = Document::new(None);
        let middle = Point2::new(5000.0, 7000.0);
        let corners = add_strip(&mut document, middle, 450.0, 20.0, 45.0 * DEGREE);
        let mut plan = viewport("vp1", ViewportKind::Plan, tiling_area(&Sheet::default()));
        plan.auto_scale = true;
        plan.auto_centre = true;
        add_sheet_with(&mut document, vec![plan]);
        let mut shown = Shown::new(document);
        let before = shown.steps();

        // Nothing selected: the sheet's main plan.
        shown.trigger("sheetRotateToBestFit");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.errors(), 0);
        let turned = viewport_of(&shown, "vp1");
        assert_eq!(turned.scale, 1250.0);
        assert!(turned.rotation > 0.0);
        assert!(turned.rotation < 45.0 * DEGREE);
        assert!(near(turned.centre.x, middle.x, 1e-6));
        assert!(near(turned.centre.y, middle.y, 1e-6));

        // Painted: each corner of the strip where the view's rotation, centre and scale put it,
        // inked, and inside the view.
        let set = shown.sheet_set();
        let sheet = &set.sheets[0];
        const PPMM: f64 = 4.0;
        let paper = paper_dimensions(sheet.paper, sheet.landscape);
        let mut image = Image::new(
            (paper.width_mm * PPMM).round() as u32,
            (paper.height_mm * PPMM).round() as u32,
        );
        image.fill(Color::WHITE);
        let options = SheetPaintOptions {
            pixels_per_millimetre: PPMM,
            ..SheetPaintOptions::default()
        };
        let mut cache = SheetPaintCache::new();
        {
            let source = shown.editor.source(shown.document());
            RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
                let _ = paint_sheet(canvas, &set, 0, &source, &options, &mut cache);
            });
        }
        if let Some(folder) = std::env::var("KATANA_SHEET_PNG").ok().filter(|folder| !folder.is_empty()) {
            let _ = write_png(
                &image,
                &std::path::Path::new(&folder).join("SheetArrangeTurned.png"),
            );
        }
        let ink_near = |at: Point2| {
            let cx = (at.x * PPMM).floor() as i64;
            let cy = ((paper.height_mm - at.y) * PPMM).floor() as i64;
            let mut ink = 0;
            for y in cy - 3..=cy + 3 {
                for x in cx - 3..=cx + 3 {
                    if x >= 0
                        && y >= 0
                        && (x as u32) < image.width()
                        && (y as u32) < image.height()
                        && image.pixel(x as u32, y as u32) != Color::WHITE
                    {
                        ink += 1;
                    }
                }
            }
            ink
        };
        let paper_centre = turned.rect.center();
        for corner in &corners {
            let on_paper = (*corner - turned.centre).rotated(-turned.rotation) * (1000.0 / turned.scale);
            let at = paper_centre + on_paper;
            assert!(turned.rect.inflated(-1.0).contains(at), "{}, {}", at.x, at.y);
            assert!(ink_near(at) > 0, "no line at the corner {}, {}", at.x, at.y);
        }
        // The long sides run nearly across the paper: their middles are inked too, where the
        // strip's centre line crosses the view's middle.
        let side = Point2::new(0.0, 10.0).rotated(45.0 * DEGREE - turned.rotation) * (1000.0 / 1250.0);
        assert!(ink_near(paper_centre + side) > 0);
        assert!(ink_near(paper_centre - side) > 0);

        shown.undo();
        assert_eq!(viewport_of(&shown, "vp1").rotation, 0.0);
    }

    #[test]
    fn choose_paper_puts_the_sheet_on_paper_that_holds_its_plan() {
        // 500 x 300 m at 1 : 1000 filling the sheet: 520 x 312 mm with 4% to spare, which A3
        // cannot hold and A2 landscape can.
        let mut document = Document::new(None);
        let _ = add_strip(&mut document, Point2::new(250.0, 150.0), 500.0, 300.0, 0.0);
        let mut plan = viewport("vp1", ViewportKind::Plan, tiling_area(&Sheet::default()));
        plan.scale = 1000.0;
        add_sheet_with(&mut document, vec![plan]);
        let mut shown = Shown::new(document);
        shown.draw();
        let button = shown.control("sheetChoosePaper").expect("the button");
        assert!(button.enabled);
        let before = shown.steps();
        // The button opens Choose Paper at the scale the plan is drawn at; its Apply runs SHEET
        // SUGGESTPAPER, whose reply says the paper in its words.
        shown.press_button("sheetChoosePaper");
        assert!(shown.editor.suggest_paper_dialog().is_some());
        shown.press_in("sheetSuggestPaperDialog", "sheetSuggestPaperApply");
        assert_eq!(shown.steps(), before + 1);
        assert_eq!(shown.errors(), 0);
        let set = shown.sheet_set();
        let sheet = &set.sheets[0];
        assert_eq!(sheet.paper, PaperSize::A2);
        assert!(sheet.landscape);
        assert_eq!(viewport_of(&shown, "vp1").rect, tiling_area(sheet));
        assert!(last_message(&shown).contains("paper=A2 orientation=landscape"));

        // A sheet with no plan has nothing to choose paper for.
        edit_sheet(
            shown.document_mut(),
            0,
            |s| {
                s.viewports[0].kind = ViewportKind::Legend;
                Ok(())
            },
            "EDIT_SHEET",
        )
        .expect("edited");
        // The properties are built again on the change.
        shown.draw();
        let disabled = shown.control("sheetChoosePaper").expect("the button");
        assert!(!disabled.enabled);
    }

    #[test]
    fn the_fitted_drawing_is_generated_turned_when_asked() {
        // The strip along 45 degrees, on its own sheet (no legend beside it): in the 385 x 250 mm
        // tiling area it is 1 : 2000 square and 1 : 1250 turned by the least turn that reaches it.
        let mut document = Document::new(None);
        let _ = add_strip(&mut document, Point2::new(0.0, 0.0), 450.0, 20.0, 45.0 * DEGREE);
        let mut shown = Shown::new(document);
        let answer = |shown: &mut Shown, rotate: bool| {
            let _ = shown.editor.generate_sheets(&shown.host);
            shown.draw();
            let turn = shown.control("sheetGenerateRotate").expect("the rotate box");
            assert!(turn.enabled);
            shown.fill_in(
                "sheetGenerateDialog",
                "sheetGenerateRotate",
                if rotate { "on" } else { "off" },
            );
            shown.fill_in("sheetGenerateDialog", "sheetGenerateLegend", "off");
            shown.press_in("sheetGenerateDialog", "sheetGenerateOk");
        };

        answer(&mut shown, false);
        let set = shown.sheet_set();
        assert_eq!(set.sheets.len(), 1);
        assert_eq!(set.sheets[0].viewports[0].scale, 2000.0);
        assert_eq!(set.sheets[0].viewports[0].rotation, 0.0);

        answer(&mut shown, true);
        let set = shown.sheet_set();
        assert_eq!(set.sheets.len(), 2);
        let turned = &set.sheets[1].viewports[0];
        let t = 45.0 * DEGREE - 225.0_f64.atan2(10.0) + (156.25 / 225.0_f64.hypot(10.0)).acos();
        assert_eq!(turned.scale, 1250.0);
        assert!(near(turned.rotation, t, 1e-9));
    }
}
