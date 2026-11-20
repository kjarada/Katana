//! The twins of tests/qt_widgets/plotting/test_sheet_checks.cpp: that the checker is given what the
//! painter knows, so the two agree about what prints empty; the Checks dock and its toolbar
//! action; a double-click going to the finding; the re-check after a burst of edits, once; and a
//! plot with errors that warns and still plots.
//!
//! The A3 numbers are the frame's: drawing area 23..410 x 35..287 mm, the title block's top at
//! 34.625 mm (docs/plotting.md).

mod sheet_checks {
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use katana_cad::Document;
    use katana_cad::plotting::preflight::{Finding, Severity, finding_line, plan_window};
    use katana_cad::plotting::sheet_commands::{add_sheet, edit_viewport};
    use katana_cad::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind};
    use katana_cad::scene::{SceneMesh, SceneSurface};
    use katana_entity::{Alignment, Entity, Geometry, Model};
    use katana_geometry::mesh::TriangleMesh;
    use katana_geometry::{
        AlignmentPi, Box2, HorizontalAlignment, Point2, Point3, Segment2,
    };
    use katana_terrain::tin_surface::TinSurface;
    use katana_window::canvas::{Canvas, Color, Image, RasterCanvas};
    use katana_window::plan_painter::{PlanSource, plan_source_of};
    use katana_window::plotting::sheet_checks::{
        FIX_HEADER, check_sheets_for, check_sheets_icon, plan_resolver, preflight_log,
        preflight_options_for, severity_icon,
    };
    use katana_window::sheet_editor::SourceProvider;
    use katana_window::sheet_painter::{
        SheetPaintCache, SheetPaintOptions, SheetSource, paint_sheet, resolve_plan_viewport,
    };

    use crate::support::{Shown, bx, near, sheet_of};
    use crate::window_support::Scratch;

    fn inside() -> Box2 {
        bx(100.0, 100.0, 200.0, 180.0)
    }

    /// Down to 20 mm: into the title block.
    fn under_the_title_block() -> Box2 {
        bx(100.0, 20.0, 200.0, 100.0)
    }

    fn notes_at(id: &str, rect: Box2) -> Viewport {
        Viewport {
            id: String::from(id),
            kind: ViewportKind::Notes,
            rect,
            text: String::from("ALL LEVELS IN METRES"),
            ..Viewport::default()
        }
    }

    fn plan_at(id: &str, rect: Box2, scale: f64, centre: Point2) -> Viewport {
        Viewport {
            id: String::from(id),
            kind: ViewportKind::Plan,
            rect,
            scale,
            centre,
            ..Viewport::default()
        }
    }

    fn with_code(findings: &[Finding], code: &str) -> Vec<Finding> {
        findings
            .iter()
            .filter(|finding| finding.code == code)
            .cloned()
            .collect()
    }

    fn source_of() -> SourceProvider {
        Rc::new(|document: &Document| {
            let mut source = SheetSource {
                plan: plan_source_of(document),
                revision: document.model_revision(),
                ..SheetSource::default()
            };
            source.fields.project_name = String::from("Main Road");
            source.fields.plot_date = String::from("25/09/26");
            source
        })
    }

    fn shown(document: Document) -> Shown {
        Shown::with_source(document, source_of())
    }

    fn mesh(vertices: [Point3; 3]) -> TriangleMesh {
        TriangleMesh {
            vertices: vertices.to_vec(),
            faces: vec![[0, 1, 2]],
        }
    }

    fn source_over<'a>(model: Option<&'a Model>, meshes: Option<&'a [SceneMesh<'a>]>) -> SheetSource<'a> {
        SheetSource {
            plan: PlanSource {
                model,
                meshes,
                ..PlanSource::default()
            },
            ..SheetSource::default()
        }
    }

    /// Runs `poll` until `done` or two seconds pass (the C++'s waitFor over the event loop).
    fn wait_for(shown: &mut Shown, done: impl Fn(&Shown) -> bool) {
        let start = Instant::now();
        while !done(shown) && start.elapsed() < Duration::from_secs(2) {
            shown.editor.poll(&mut shown.host);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // ---- the checker, given what the painter knows -----------------------------------------------

    #[test]
    fn the_options_carry_what_the_painter_draws_from() {
        let model = Model::new();
        {
            let source = source_over(Some(&model), None);
            let resolver = plan_resolver(&source);
            let options = preflight_options_for(&source, &resolver);
            // Nothing to cut sections from, no logo decoded, nothing but the drawing.
            assert_eq!(options.section_surfaces, Some(0));
            assert_eq!(options.logo_readable, Some(false));
            assert!(options.other_content.is_empty());
            assert!(options.resolve_plan.is_some());
        }

        // A mesh is drawn in plan as its footprint, so it is content; a hidden one is not.
        let footprint = mesh([
            Point3::new(10.0, 20.0, 0.0),
            Point3::new(30.0, 20.0, 1.0),
            Point3::new(30.0, 50.0, 2.0),
        ]);
        let mut meshes = vec![SceneMesh {
            mesh: Some(&footprint),
            ..SceneMesh::default()
        }];
        {
            let mut source = source_over(Some(&model), Some(&meshes));
            source.logo = Image::new(40, 10);
            source.assets = PathBuf::from("assets");
            let resolver = plan_resolver(&source);
            let options = preflight_options_for(&source, &resolver);
            assert_eq!(options.other_content.len(), 1);
            assert_eq!(options.other_content[0], bx(10.0, 20.0, 30.0, 50.0));
            assert_eq!(options.logo_readable, Some(true));
            assert_eq!(options.assets, PathBuf::from("assets"));
        }
        meshes[0].visible = false;
        let source = source_over(Some(&model), Some(&meshes));
        let resolver = plan_resolver(&source);
        assert!(
            preflight_options_for(&source, &resolver)
                .other_content
                .is_empty()
        );
    }

    #[test]
    fn an_automatic_plan_is_checked_where_the_painter_draws_it() {
        // The headless rule (cad's plan_window, given the window's imagery and meshes as
        // other_content) must decide an automatic plan's scale and centre as the painter does, or
        // the checks would test a window that never prints. Compared case by case: square,
        // turned, along a stretch of an alignment, over a mesh alone, and with the drawing's layer
        // hidden.
        let mut model = Model::new();
        model
            .entities
            .add(Entity {
                geometry: Geometry::Line(Segment2::new(
                    Point2::new(0.0, 0.0),
                    Point2::new(300.0, 40.0),
                )),
                layer: String::from("WALLS"),
                ..Entity::default()
            })
            .expect("the line is added");
        let pi = |x: f64, y: f64| AlignmentPi {
            point: Point2::new(x, y),
            ..AlignmentPi::default()
        };
        model
            .alignments
            .add(Alignment {
                name: String::from("MC01"),
                horizontal: HorizontalAlignment {
                    pis: vec![pi(0.0, 100.0), pi(400.0, 300.0)],
                    ..HorizontalAlignment::default()
                },
                ..Alignment::default()
            })
            .expect("the road is added");
        let far = mesh([
            Point3::new(-900.0, -700.0, 0.0),
            Point3::new(-850.0, -700.0, 1.0),
            Point3::new(-850.0, -640.0, 2.0),
        ]);
        let meshes = vec![SceneMesh {
            mesh: Some(&far),
            ..SceneMesh::default()
        }];
        let source = source_over(Some(&model), Some(&meshes));
        let resolver = plan_resolver(&source);
        let options = preflight_options_for(&source, &resolver);

        let mut plan = plan_at("vp1", bx(23.0, 35.0, 410.0, 287.0), 1.0, Point2::default());
        plan.auto_scale = true;
        plan.auto_centre = true;
        let agree = |viewport: &Viewport, what: &str| {
            let painted = resolve_plan_viewport(viewport, &source);
            let headless = plan_window(viewport, &model, &options.other_content);
            assert_eq!(headless.scale, painted.scale, "{what}");
            assert!(near(headless.centre.x, painted.centre.x, 1e-9), "{what}");
            assert!(near(headless.centre.y, painted.centre.y, 1e-9), "{what}");
            let checked = (options.resolve_plan.expect("a resolver"))(viewport);
            assert_eq!(checked.scale, painted.scale, "{what}");
        };
        agree(&plan, "square");
        let mut turned = plan.clone();
        // Radians.
        turned.rotation = 0.5;
        agree(&turned, "turned");
        let mut along = plan.clone();
        along.source.alignment = String::from("MC01");
        along.source.chainage_from = 50.0;
        along.source.chainage_to = 250.0;
        agree(&along, "along a stretch of MC01");
        let mut hidden = plan.clone();
        let _ = hidden.hidden_layers.hide("WALLS");
        agree(&hidden, "WALLS hidden");
        // The mesh alone: the drawing and the alignment are far from it, so a scale fitted to the
        // mesh differs from one fitted to everything.
        let empty = Model::new();
        let alone = source_over(Some(&empty), Some(&meshes));
        let mesh_only = resolve_plan_viewport(&plan, &alone);
        let headless = plan_window(&plan, &empty, &options.other_content);
        assert_eq!(headless.scale, mesh_only.scale);
        assert!(near(headless.centre.x, mesh_only.centre.x, 1e-9));
        assert!(near(headless.centre.y, mesh_only.centre.y, 1e-9));
        assert!(near(mesh_only.centre.x, -875.0, 1e-9));
    }

    #[test]
    fn a_plan_over_only_a_mesh_is_not_empty() {
        let model = Model::new();
        let set = SheetSet {
            sheets: vec![sheet_of(
                "s1",
                "PLAN",
                vec![plan_at("vp1", inside(), 500.0, Point2::new(50.0, 0.0))],
            )],
            ..SheetSet::default()
        };
        let bare = source_over(Some(&model), None);
        assert_eq!(
            with_code(&check_sheets_for(&set, &bare, &[]), "plan.empty").len(),
            1
        );

        let ground = mesh([
            Point3::new(40.0, -5.0, 0.0),
            Point3::new(60.0, -5.0, 0.0),
            Point3::new(60.0, 5.0, 0.0),
        ]);
        let meshes = vec![SceneMesh {
            mesh: Some(&ground),
            ..SceneMesh::default()
        }];
        let source = source_over(Some(&model), Some(&meshes));
        assert!(with_code(&check_sheets_for(&set, &source, &[]), "plan.empty").is_empty());
    }

    #[test]
    fn the_checker_and_the_painter_agree_about_a_section_with_no_surface() {
        // A road north up x = 50 over ground z = x / 2; a cross section at CH 40.
        let ground = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(100.0, 0.0, 50.0),
                Point3::new(100.0, 100.0, 50.0),
                Point3::new(0.0, 100.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        )
        .expect("the ground is a surface");
        let mut model = Model::new();
        let pi = |y: f64| AlignmentPi {
            point: Point2::new(50.0, y),
            ..AlignmentPi::default()
        };
        model
            .alignments
            .add(Alignment {
                name: String::from("ROAD"),
                horizontal: HorizontalAlignment {
                    pis: vec![pi(10.0), pi(90.0)],
                    ..HorizontalAlignment::default()
                },
                ..Alignment::default()
            })
            .expect("the road is added");
        let mut across = Viewport {
            id: String::from("vp1"),
            kind: ViewportKind::CrossSections,
            rect: bx(30.0, 40.0, 210.0, 155.0),
            scale: 250.0,
            auto_centre: true,
            ..Viewport::default()
        };
        across.source.alignment = String::from("ROAD");
        across.source.stations = vec![40.0];
        across.source.section_half_width = 20.0;
        let set = SheetSet {
            sheets: vec![sheet_of("s1", "SECTIONS", vec![across])],
            ..SheetSet::default()
        };

        let painter_problems = |source: &SheetSource<'_>| {
            let mut paper = Image::new(420, 297);
            let options = SheetPaintOptions {
                pixels_per_millimetre: 1.0,
                ..SheetPaintOptions::default()
            };
            let mut cache = SheetPaintCache::new();
            let mut problems = 0;
            RasterCanvas::paint_image(&mut paper, &mut |canvas: &mut dyn Canvas| {
                problems = paint_sheet(canvas, &set, 0, source, &options, &mut cache)
                    .problems
                    .len();
            });
            problems
        };
        // No surface: the painter leaves it empty, and the checker says so.
        let mut source = source_over(Some(&model), None);
        assert!(painter_problems(&source) > 0);
        let without = with_code(&check_sheets_for(&set, &source, &[]), "section.no-surface");
        assert_eq!(without.len(), 1);
        assert_eq!(without[0].severity, Severity::Error);
        // A hidden surface is no surface either.
        source.surfaces.push(SceneSurface {
            name: String::from("GROUND"),
            surface: Some(&ground),
            visible: false,
            ..SceneSurface::default()
        });
        assert_eq!(
            with_code(&check_sheets_for(&set, &source, &[]), "section.no-surface").len(),
            1
        );
        // A visible one: both are content.
        source.surfaces[0].visible = true;
        assert_eq!(painter_problems(&source), 0);
        assert!(
            with_code(&check_sheets_for(&set, &source, &[]), "section.no-surface").is_empty()
        );
    }

    #[test]
    fn only_the_sheets_asked_for_are_checked() {
        let model = Model::new();
        let set = SheetSet {
            sheets: vec![sheet_of("s1", "ONE", Vec::new()), sheet_of("s2", "TWO", Vec::new())],
            ..SheetSet::default()
        };
        let source = source_over(Some(&model), None);
        let findings = with_code(&check_sheets_for(&set, &source, &[1]), "sheet.empty");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].sheet_id, "s2");
        // With no drawing at all the sheets are still checked, as the painter would draw them:
        // empty.
        let none = source_over(None, None);
        assert_eq!(
            with_code(&check_sheets_for(&set, &none, &[]), "sheet.empty").len(),
            2
        );
    }

    fn finding(
        severity: Severity,
        code: &str,
        sheet: Option<(usize, &str)>,
        viewport: &str,
        subject: &str,
        message: &str,
        fix: &str,
    ) -> Finding {
        Finding {
            severity,
            code: String::from(code),
            sheet_index: sheet.map(|(index, _)| index),
            sheet_id: sheet.map(|(_, id)| String::from(id)).unwrap_or_default(),
            viewport_id: String::from(viewport),
            subject: String::from(subject),
            message: String::from(message),
            fix: String::from(fix),
        }
    }

    #[test]
    fn the_log_before_a_plot_summarises_and_lists_the_errors() {
        let findings = vec![
            finding(
                Severity::Warning,
                "field.empty",
                None,
                "",
                "organisation",
                "Organisation is blank",
                "Fill it in",
            ),
            finding(
                Severity::Error,
                "viewport.outside",
                Some((1, "s2")),
                "vp3",
                "",
                "NOTES (vp3) runs under the title block",
                "Drag it back",
            ),
            finding(Severity::Info, "logo.missing", None, "", "", "No logo", ""),
        ];
        let lines = preflight_log(&findings);
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("Sheet checks: 1 error, 1 warning, 1 note."),
            "{}",
            lines[0]
        );
        assert_eq!(lines[1], finding_line(&findings[1]));
        // Notes alone are not worth a word before a plot.
        assert!(preflight_log(&findings[2..]).is_empty());
    }

    #[test]
    fn the_severity_marks_are_painted() {
        assert_eq!(
            severity_icon(Severity::Error, 16).pixel(3, 8),
            Color::rgb(211, 47, 47)
        );
        assert_eq!(
            severity_icon(Severity::Warning, 16).pixel(4, 13),
            Color::rgb(245, 166, 35)
        );
        assert_eq!(
            severity_icon(Severity::Info, 16).pixel(3, 8),
            Color::rgb(25, 118, 210)
        );
        assert!(!check_sheets_icon(16).is_null());
    }

    // ---- the editor ------------------------------------------------------------------------------

    fn add(document: &mut Document, sheet: Sheet) {
        add_sheet(document, sheet, None).expect("the sheet is added");
    }

    #[test]
    fn the_editor_has_a_checks_dock_and_a_check_sheets_action() {
        let mut document = Document::new(None);
        add(
            &mut document,
            sheet_of("s1", "NOTES", vec![notes_at("vp1", under_the_title_block())]),
        );
        let mut shown = shown(document);
        assert!(shown.editor.action(&shown.host, "sheetCheck").is_some());
        assert!(shown.has_control("sheetChecksList"));
        assert!(shown.has_control("sheetChecksSummary"));

        shown.editor.checks_mut().close();
        let before = shown.editor.checks().runs();
        shown.trigger("sheetCheck");
        assert_eq!(shown.editor.checks().runs(), before + 1);
        assert!(shown.editor.checks().is_visible());
        // Its rows are the findings, each marked: errors first, then warnings, then notes, each
        // in the checker's order.
        let dock = shown.editor.checks();
        let findings = dock.findings().to_vec();
        assert_eq!(dock.rows().len(), findings.len());
        assert_eq!(with_code(&findings, "viewport.outside").len(), 1);
        let mut last_row: Option<usize> = None;
        for severity in [Severity::Error, Severity::Warning, Severity::Info] {
            for (index, finding) in findings.iter().enumerate() {
                if finding.severity == severity {
                    let row = dock.row_of(index);
                    assert_eq!(row, Some(last_row.map_or(0, |last| last + 1)), "{}", finding.code);
                    last_row = row;
                }
            }
        }
        for (index, finding) in findings.iter().enumerate() {
            let row = &dock.rows()[dock.row_of(index).expect("a row")];
            assert!(!severity_icon(row.severity, 16).is_null());
            assert_eq!(row.cells[3], finding.message);
            assert_eq!(row.cells[4], finding.fix);
            if finding.code == "viewport.outside" {
                assert_eq!(row.cells[0], "Error");
                assert_eq!(row.cells[1], "1  NOTES");
                assert_eq!(row.cells[2], "vp1");
            } else if finding.sheet_index.is_none() {
                assert_eq!(row.cells[1], "All sheets");
            }
        }
        assert!(dock.summary().starts_with("1 error"), "{}", dock.summary());
        assert!(
            dock.window_title().starts_with("Checks (1 error"),
            "{}",
            dock.window_title()
        );
    }

    #[test]
    fn double_clicking_a_finding_goes_to_its_sheet_and_view() {
        let mut document = Document::new(None);
        add(&mut document, sheet_of("s1", "FIRST", vec![notes_at("vp1", inside())]));
        add(
            &mut document,
            sheet_of(
                "s2",
                "SECOND",
                vec![
                    notes_at("vp2", inside()),
                    notes_at("vp3", under_the_title_block()),
                ],
            ),
        );
        let mut shown = shown(document);
        shown.editor.set_current_sheet(&shown.host, 0);
        let _ = shown.editor.check_sheets(&mut shown.host);
        shown.draw();
        let findings = shown.editor.checks().findings().to_vec();
        let index = findings
            .iter()
            .rposition(|finding| finding.code == "viewport.outside")
            .expect("the view under the title block");
        assert_eq!(findings[index].viewport_id, "vp3");
        // The one error heads the list.
        assert_eq!(shown.editor.checks().row_of(index), Some(0));
        assert!(shown.has_control("sheetChecksList"));

        // A double-click on its row.
        shown.editor.activate_finding(&mut shown.host, index);
        assert_eq!(shown.editor.current_sheet(), 1);
        assert_eq!(shown.selected(), "vp3");
        assert!(
            shown.editor.status_message().contains("title block"),
            "{}",
            shown.editor.status_message()
        );

        // A finding about the set has no sheet to go to: the sheet stays.
        shown.editor.set_current_sheet(&shown.host, 0);
        shown.editor.show_finding(
            &mut shown.host,
            &finding(
                Severity::Info,
                "logo.missing",
                None,
                "",
                "",
                "The title block's logo slot is empty",
                "Choose one",
            ),
        );
        assert_eq!(shown.editor.current_sheet(), 0);
        // One whose sheet was removed since, neither.
        shown.editor.show_finding(
            &mut shown.host,
            &finding(
                Severity::Error,
                "viewport.outside",
                Some((5, "s9")),
                "vp9",
                "",
                "Gone",
                "",
            ),
        );
        assert_eq!(shown.editor.current_sheet(), 0);
    }

    /// Where the Fix column's title is drawn across the pane: its left and right edges. Not its
    /// height: the dock is as tall as what it lists, so the title's row moves down the window as
    /// the findings come, by design.
    fn fix_title_across(shown: &Shown) -> (f32, f32) {
        let title = shown.control(FIX_HEADER).expect("the Fix column's title");
        assert!(title.visible);
        assert_eq!(title.text, "Fix");
        (title.rect.left(), title.rect.right())
    }

    #[test]
    fn the_fix_column_stands_in_one_place_whether_the_table_is_empty_or_full() {
        // The columns were sized from their cells, so Fix's title stood under the titles' own
        // widths while nothing was listed and jumped right by the longest finding's length when
        // a check ran (the ninth look audit). The property: its place across the pane is the
        // same with nothing listed, with a check's findings, and with a finding far longer than
        // the pane.
        let mut document = Document::new(None);
        add(
            &mut document,
            sheet_of("s1", "NOTES", vec![notes_at("vp1", under_the_title_block())]),
        );
        let mut shown = shown(document);
        // Nothing listed, said by the test: a shown editor has a check of its own due in a
        // moment (RECHECK_DELAY_MS), which a run handed over drops, so no frame here lists
        // anything the test did not.
        let set = shown.sheet_set();
        let _ = shown.editor.checks_mut().check_now_with(Vec::new(), &set);
        shown.draw();
        assert!(shown.editor.checks().rows().is_empty());
        let empty = fix_title_across(&shown);
        // The title is in the pane, right of the four columns before it.
        let list = shown.control("sheetChecksList").expect("the list");
        assert!(empty.0 > list.rect.left() && empty.0 < list.rect.right(), "{empty:?}");

        // A check: the view under the title block, and what else the checker finds.
        let _ = shown.editor.check_sheets(&mut shown.host);
        shown.draw();
        assert!(!shown.editor.checks().rows().is_empty());
        assert_eq!(fix_title_across(&shown), empty);

        // A finding whose every cell is longer than its column, the message some 2,000 points
        // of words on a 1,400 point screen.
        let long = "the view runs under the title block and off the paper ".repeat(6);
        let _ = shown.editor.checks_mut().check_now_with(
            vec![finding(
                Severity::Error,
                "viewport.outside",
                Some((0, "s1")),
                "a-viewport-with-a-very-long-id-of-its-own",
                "",
                &long,
                &long,
            )],
            &set,
        );
        shown.draw();
        assert_eq!(shown.editor.checks().rows().len(), 1);
        assert_eq!(fix_title_across(&shown), empty);

        // And back to nothing listed.
        let _ = shown.editor.checks_mut().check_now_with(Vec::new(), &set);
        shown.draw();
        assert!(shown.editor.checks().rows().is_empty());
        assert_eq!(fix_title_across(&shown), empty);
    }

    fn move_bottom(shown: &mut Shown, bottom: f64) {
        edit_viewport(
            shown.document_mut(),
            "vp1",
            |viewport| {
                viewport.rect.min.y = bottom;
                Ok(())
            },
            "EDIT_VIEWPORT",
        )
        .expect("edited");
        // The editor hears of the edit, as its listener did.
        shown.observe();
    }

    #[test]
    fn a_burst_of_edits_is_checked_once_when_it_stops() {
        let mut document = Document::new(None);
        add(&mut document, sheet_of("s1", "NOTES", vec![notes_at("vp1", inside())]));
        let mut shown = shown(document);
        shown.editor.checks_mut().set_delay(30);
        let _ = shown.editor.check_now(&shown.host);
        assert!(with_code(shown.editor.checks().findings(), "viewport.outside").is_empty());
        let before = shown.editor.checks().runs();

        // Three edits, the last moving the view under the title block.
        for bottom in [90.0, 60.0, 20.0] {
            move_bottom(&mut shown, bottom);
        }
        assert!(shown.editor.checks().pending());
        assert_eq!(shown.editor.checks().runs(), before);
        wait_for(&mut shown, |shown| !shown.editor.checks().pending());
        assert_eq!(shown.editor.checks().runs(), before + 1);
        assert_eq!(
            with_code(shown.editor.checks().findings(), "viewport.outside").len(),
            1
        );

        // An undo is an edit too.
        shown.undo();
        shown.undo();
        shown.undo();
        wait_for(&mut shown, |shown| !shown.editor.checks().pending());
        assert_eq!(shown.editor.checks().runs(), before + 2);
        assert!(with_code(shown.editor.checks().findings(), "viewport.outside").is_empty());
    }

    #[test]
    fn a_closed_editor_checks_again_only_when_it_is_shown() {
        let mut document = Document::new(None);
        add(&mut document, sheet_of("s1", "NOTES", vec![notes_at("vp1", inside())]));
        let mut shown = shown(document);
        shown.editor.checks_mut().set_delay(30);
        let _ = shown.editor.check_now(&shown.host);
        let before = shown.editor.checks().runs();

        // Closed, an edit schedules nothing: the findings are only out of date.
        shown.editor.hide();
        move_bottom(&mut shown, 20.0);
        assert!(!shown.editor.checks().pending());
        shown.editor.poll(&mut shown.host);
        assert_eq!(shown.editor.checks().runs(), before);

        // Shown again, it checks once, and finds what the edit did.
        shown.editor.show(&shown.host);
        wait_for(&mut shown, |shown| !shown.editor.checks().pending());
        assert_eq!(shown.editor.checks().runs(), before + 1);
        assert_eq!(
            with_code(shown.editor.checks().findings(), "viewport.outside").len(),
            1
        );
    }

    #[test]
    fn a_plot_with_errors_warns_and_still_plots() {
        let mut document = Document::new(None);
        add(&mut document, sheet_of("s1", "CLEAN", vec![notes_at("vp1", inside())]));
        add(
            &mut document,
            sheet_of("s2", "WRONG", vec![notes_at("vp2", under_the_title_block())]),
        );
        let mut shown = shown(document);
        let scratch = Scratch::new("plot-with-errors");

        // The clean sheet alone: nothing to warn about, and the dock stays shut.
        shown.editor.checks_mut().close();
        shown.editor.set_current_sheet(&shown.host, 0);
        let one = scratch.path("one.pdf");
        shown
            .editor
            .plot_to_pdf(&mut shown.host, &one.to_string_lossy(), false)
            .expect("plotted");
        assert!(one.exists());
        let messages = shown.messages();
        assert!(!messages.is_empty());
        let last = messages.last().cloned().unwrap_or_default();
        assert!(!last.contains("checks found"), "{last}");
        assert!(!shown.editor.checks().is_visible());

        // Every sheet: the error on the second is reported and the dock comes up, but the PDF is
        // written.
        let all = scratch.path("all.pdf");
        shown
            .editor
            .plot_to_pdf(&mut shown.host, &all.to_string_lossy(), true)
            .expect("plotted");
        assert!(all.exists());
        let last = shown.messages().last().cloned().unwrap_or_default();
        assert!(last.contains("The checks found 1 error on the sheets"), "{last}");
        assert!(shown.editor.checks().is_visible());
        assert_eq!(
            with_code(shown.editor.checks().findings(), "viewport.outside").len(),
            1
        );
    }
}
