//! The twins of tests/qt_widgets/plotting/test_sheet_tables_painter.cpp: the drawing register and
//! the revision table as the sheet painter draws them (docs/plotting.md, "The drawing register and
//! the revision table"), and the scale the register reports for an automatic plan
//! (katana_window::plotting::sheet_tables). The rows' places are the layout's, so a row is found
//! by its box and never by a font's size.

use katana_cad::plot::PaperSize;
use katana_cad::plotting::generators::{
    GridRequest, SheetTemplate, grid_sheets, prepare_for_append,
};
use katana_cad::plotting::sheet_set::{Revision, Sheet, SheetSet, Viewport, ViewportKind};
use katana_cad::plotting::tables::{
    TableLayout, drawing_register, layout_viewport_table, register_sheet,
};
use katana_entity::{Entity, Geometry, Model};
use katana_geometry::{Box2, Point2, Segment2};
use katana_window::canvas::Image;
use katana_window::plan_painter::PlanSource;
use katana_window::plotting::sheet_tables::{drawn_register, resolved_sheet_set};
use katana_window::sheet_painter::{
    SheetPaintStats, SheetSource, resolve_plan_viewport, resolve_plan_viewport_on_sheet,
    sheet_frame,
};

use crate::support::{PPMM, Painting, WHITE, bx, double_eq, near, painted_with, pixels_in};

fn table_at(kind: ViewportKind, rect: Box2) -> Viewport {
    Viewport {
        id: String::from("vp1"),
        kind,
        rect,
        ..Viewport::default()
    }
}

fn sheet_named(id: &str, name: &str) -> Sheet {
    Sheet {
        id: String::from(id),
        name: String::from(name),
        ..Sheet::default()
    }
}

fn source_of(model: &Model) -> SheetSource<'_> {
    SheetSource {
        plan: PlanSource {
            model: Some(model),
            ..PlanSource::default()
        },
        ..SheetSource::default()
    }
}

/// The set's first sheet painted; `editing` with the editor's furniture (the construction guide
/// and the slot hints).
fn painted(
    set: &SheetSet,
    source: &SheetSource<'_>,
    editing: bool,
    name: &str,
) -> (Image, SheetPaintStats) {
    painted_with(
        set,
        source,
        &Painting {
            construction: editing,
            slot_hints: editing,
            ..Painting::named(name)
        },
    )
}

/// Pixels in the paper box that are not white (alpha aside).
fn ink_in(image: &Image, paper: &Box2) -> usize {
    pixels_in(image, paper, |c| {
        (c.r, c.g, c.b) != (WHITE.r, WHITE.g, WHITE.b)
    })
}

/// Pixels of the light shade a highlighted row is filled with.
fn shade_in(image: &Image, paper: &Box2) -> usize {
    pixels_in(image, paper, |c| {
        c.r > 215 && c.r < 245 && c.r == c.g && c.g == c.b
    })
}

fn pixels_of(paper: &Box2) -> usize {
    (paper.width() * PPMM).round() as usize * (paper.height() * PPMM).round() as usize
}

/// The layout of a table viewport, measured by the model's own estimate (the C++'s default).
fn layout_of(set: &SheetSet, viewport: &Viewport) -> TableLayout {
    layout_viewport_table(set, 0, viewport, None)
}

/// A cover with a register, and two plans after it.
fn register_set(rect: Box2) -> SheetSet {
    let mut set = SheetSet::default();
    let mut cover = sheet_named("s1", "COVER");
    cover
        .viewports
        .push(table_at(ViewportKind::SheetIndex, rect));
    set.sheets.push(cover);
    for name in ["PLAN 1", "PLAN 2"] {
        let mut sheet = sheet_named(&format!("s{}", set.sheets.len() + 1), name);
        sheet.viewports.push(Viewport {
            id: format!("vp{}", set.sheets.len() + 1),
            kind: ViewportKind::Plan,
            rect: bx(30.0, 40.0, 400.0, 280.0),
            scale: 500.0,
            ..Viewport::default()
        });
        set.sheets.push(sheet);
    }
    set
}

fn revision(code: &str, date: &str, description: &str, by: &str) -> Revision {
    Revision {
        code: String::from(code),
        date: String::from(date),
        description: String::from(description),
        by: String::from(by),
    }
}

fn add_segment(model: &mut Model, a: Point2, b: Point2) {
    model
        .entities
        .add(Entity {
            geometry: Geometry::Line(Segment2::new(a, b)),
            layer: String::from("0"),
            ..Entity::default()
        })
        .expect("the line is added");
}

mod sheet_table_painter {
    use super::*;

    #[test]
    fn the_register_is_ruled_where_its_layout_says_and_its_own_sheets_row_is_shaded() {
        let rect = bx(30.0, 50.0, 250.0, 270.0);
        let set = register_set(rect);
        let model = Model::new();
        let source = source_of(&model);
        let (paper, stats) = painted(&set, &source, false, "the_register_is_ruled");
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.viewports_drawn, 1);

        // Three rows at the largest text whatever the font: the rows' places are the layout's,
        // and so are the painter's.
        let layout = layout_of(&set, &set.sheets[0].viewports[0]);
        assert_eq!(layout.rows_shown, 3);
        assert!(double_eq(layout.cap_mm, 2.5), "{}", layout.cap_mm);
        let first = layout.row_boxes[0];
        let second = layout.row_boxes[1];

        // The heavy rule under the header, across the table.
        assert!(
            ink_in(
                &paper,
                &bx(
                    rect.min.x + 5.0,
                    first.max.y - 0.2,
                    rect.max.x - 5.0,
                    first.max.y + 0.2
                )
            ) > (0.9 * (rect.width() - 10.0) * PPMM) as usize
        );
        // Row 1 is the cover, the sheet the register is drawn on: shaded where it has no text (the
        // middle of its title cell), row 2 not.
        let blank_of_first = bx(
            rect.min.x + 60.0,
            first.min.y + 0.8,
            rect.min.x + 120.0,
            first.max.y - 0.8,
        );
        assert!(shade_in(&paper, &blank_of_first) > pixels_of(&blank_of_first) * 9 / 10);
        let blank_of_second = bx(
            rect.min.x + 60.0,
            second.min.y + 0.8,
            rect.min.x + 120.0,
            second.max.y - 0.8,
        );
        assert_eq!(ink_in(&paper, &blank_of_second), 0);
        // The rows' texts: ink at the left of each row.
        assert!(
            ink_in(
                &paper,
                &bx(
                    rect.min.x + 1.0,
                    second.min.y + 0.5,
                    rect.min.x + 60.0,
                    second.max.y - 0.5
                )
            ) > 20
        );
        // A table's heading is its title: nothing written in the bottom-left corner, where the
        // other views put theirs.
        assert_eq!(
            ink_in(
                &paper,
                &bx(
                    rect.min.x + 1.0,
                    rect.min.y + 1.0,
                    rect.min.x + 12.0,
                    rect.min.y + 7.0
                )
            ),
            0
        );
        // The heading in the band at the top.
        assert!(
            ink_in(
                &paper,
                &bx(
                    rect.min.x + 1.0,
                    rect.max.y - 6.0,
                    rect.min.x + 60.0,
                    rect.max.y - 1.0
                )
            ) > 50
        );
    }

    #[test]
    fn a_revision_table_lists_its_rows_and_an_empty_one_explains_itself_only_on_screen() {
        let rect = bx(260.0, 150.0, 400.0, 270.0);
        let mut set = SheetSet::default();
        let mut cover = sheet_named("s1", "COVER");
        cover.viewports.push(table_at(ViewportKind::Revisions, rect));
        set.sheets.push(cover);
        let model = Model::new();
        let source = source_of(&model);

        // No revisions: on paper, nothing in the middle; in the editor, a hint.
        let middle = rect.center();
        let around = bx(
            middle.x - 20.0,
            middle.y - 1.0,
            middle.x + 20.0,
            middle.y + 1.0,
        );
        let (plotted, stats) = painted(&set, &source, false, "a_revision_table_empty");
        assert_eq!(ink_in(&plotted, &around), 0);
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        let (editing, _) = painted(&set, &source, true, "a_revision_table_editing");
        assert!(ink_in(&editing, &around) > 20);

        // Two revisions: the newest in the first row, both drawn.
        set.revisions = vec![
            revision("A", "01/02/26", "FIRST ISSUE", "JC"),
            revision("B", "03/02/26", "KERBS ADDED ALONG THE NORTH SIDE", "RE"),
        ];
        let (paper, stats) = painted(&set, &source, false, "a_revision_table_two");
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        let layout = layout_of(&set, &set.sheets[0].viewports[0]);
        assert_eq!(layout.rows_shown, 2);
        for row in &layout.row_boxes {
            assert!(
                ink_in(
                    &paper,
                    &bx(
                        row.min.x + 0.5,
                        row.min.y + 0.5,
                        row.max.x - 0.5,
                        row.max.y - 0.5
                    )
                ) > 40
            );
        }
        // No row shaded - a revision table has no current row: the blank end of the newest row's
        // description cell, past its text and short of the BY column, is paper white.
        let newest = layout.row_boxes[0];
        let blank = bx(
            newest.max.x - 40.0,
            newest.min.y + 0.8,
            newest.max.x - 12.0,
            newest.max.y - 0.8,
        );
        assert_eq!(ink_in(&paper, &blank), 0);
    }

    #[test]
    fn a_register_too_small_for_its_rows_says_so() {
        // 60 sheets in a 70 x 40 mm register: most are counted, not listed, and the painter says so
        // for the editor and the plot log.
        let mut set = register_set(bx(30.0, 50.0, 100.0, 90.0));
        for i in 3..60 {
            set.sheets
                .push(sheet_named(&format!("s{}", i + 1), &format!("PLAN {i}")));
        }
        let model = Model::new();
        let source = source_of(&model);
        let (_, stats) = painted(&set, &source, false, "a_register_too_small");
        assert_eq!(stats.problems.len(), 1);
        assert!(
            stats.problems[0].starts_with("vp1: "),
            "{}",
            stats.problems[0]
        );
        assert!(
            stats.problems[0].ends_with(" of 60 rows do not fit; make the view larger"),
            "{}",
            stats.problems[0]
        );
        // The view is still drawn: the rows that fit, and the count.
        assert_eq!(stats.viewports_drawn, 1);
    }

    #[test]
    fn the_register_reports_the_scale_an_automatic_plan_is_drawn_at() {
        // A 1000 m line in a 370 x 240 mm plan left on automatic: drawn at the first sheet scale
        // that holds it with 4% to spare, not at the 1:500 it was stored with - and the register
        // says what the title block says.
        let mut model = Model::new();
        add_segment(&mut model, Point2::new(0.0, 0.0), Point2::new(1000.0, 0.0));
        let mut set = register_set(bx(30.0, 50.0, 250.0, 270.0));
        set.sheets[1].viewports[0].auto_scale = true;
        set.sheets[1].viewports[0].auto_centre = true;
        let source = source_of(&model);

        let resolved = resolve_plan_viewport(&set.sheets[1].viewports[0], &source);
        assert_ne!(resolved.scale, 500.0);
        let drawn = drawn_register(&set, &source);
        assert_eq!(drawn.len(), 3);
        assert_eq!(drawn[1].scale, format!("1:{}", resolved.scale.round() as i64));
        // The model alone knows only the scale last chosen.
        assert_eq!(drawing_register(&set)[1].scale, "1:500");
        // A fixed scale is left as it is.
        assert_eq!(drawn[2].scale, "1:500");
        let resolved_set = resolved_sheet_set(&set, &source, None);
        assert!(!resolved_set.sheets[1].viewports[0].auto_scale);
        assert_eq!(resolved_set.sheets[2], set.sheets[2]);
    }

    // A sheet whose only scaled view is an automatic key plan: the register says the scale the key
    // plan is drawn at, fitted to the sheets' outlines as the painter fits it, not to the drawing
    // - here a stray line 20 km away that would make it a far smaller scale.
    #[test]
    fn the_register_reports_a_key_plans_scale_as_the_painter_fits_it() {
        let mut model = Model::new();
        add_segment(&mut model, Point2::new(0.0, 0.0), Point2::new(100.0, 0.0));
        add_segment(
            &mut model,
            Point2::new(20000.0, 20000.0),
            Point2::new(20010.0, 20000.0),
        );
        let mut set = register_set(bx(30.0, 50.0, 250.0, 270.0));
        let key = Viewport {
            id: String::from("vp9"),
            kind: ViewportKind::KeyPlan,
            rect: bx(30.0, 40.0, 130.0, 110.0),
            auto_scale: true,
            auto_centre: true,
            ..Viewport::default()
        };
        let mut key_sheet = sheet_named("s4", "KEY");
        key_sheet.viewports.push(key.clone());
        set.sheets.push(key_sheet);
        let source = source_of(&model);

        let painted_scale = resolve_plan_viewport_on_sheet(&key, &source, &set, 3).scale;
        let of_the_drawing = resolve_plan_viewport(&key, &source).scale;
        assert!(
            painted_scale < of_the_drawing,
            "the outlines are nearer than the stray line"
        );
        let drawn = drawn_register(&set, &source);
        assert_eq!(drawn.len(), 4);
        assert_eq!(drawn[3].scale, format!("1:{}", painted_scale.round() as i64));
    }

    #[test]
    fn the_register_cover_plots_both_tables_and_no_utility_legend() {
        // The cover register_sheet makes, put first in a set of six tiles and a key plan with two
        // revisions issued: the register in the big cell, the revisions beside it, the cover's own
        // row shaded, no utility legend.
        let model = Model::new();
        let source = source_of(&model);
        let grid = GridRequest {
            area: bx(0.0, 0.0, 400.0, 200.0),
            scale: 500.0,
            ..GridRequest::default()
        };
        let mut tiles = grid_sheets(&grid, None).expect("the area is tiled");
        let mut set = SheetSet {
            revisions: vec![
                revision("A", "01/02/26", "FIRST ISSUE", "JC"),
                revision("B", "03/02/26", "KERBS ADDED ALONG THE NORTH SIDE", "RE"),
            ],
            ..SheetSet::default()
        };
        prepare_for_append(&set, &mut tiles);
        set.sheets = tiles;
        let cover = register_sheet(&set, &SheetTemplate::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut one = vec![cover];
        prepare_for_append(&set, &mut one);
        set.sheets.insert(0, one.remove(0));
        assert_eq!(set.sheets.len(), 8);

        let (paper, stats) = painted(&set, &source, false, "the_register_cover");
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        assert_eq!(stats.viewports_drawn, 2);

        let index = &set.sheets[0].viewports[0];
        let revisions = &set.sheets[0].viewports[1];
        assert_eq!(index.kind, ViewportKind::SheetIndex);
        assert_eq!(revisions.kind, ViewportKind::Revisions);
        assert!(index.rect.max.x < revisions.rect.min.x);

        // Every sheet listed and both revisions, at the largest text.
        let rows = layout_of(&set, index);
        assert_eq!(rows.rows_shown, 8);
        assert_eq!(rows.rows_hidden, 0);
        let issued = layout_of(&set, revisions);
        assert_eq!(issued.rows_shown, 2);
        for row in &rows.row_boxes {
            assert!(
                ink_in(
                    &paper,
                    &bx(
                        row.min.x + 0.5,
                        row.min.y + 0.5,
                        row.min.x + 60.0,
                        row.max.y - 0.5
                    )
                ) > 20
            );
        }
        for row in &issued.row_boxes {
            assert!(
                ink_in(
                    &paper,
                    &bx(
                        row.min.x + 0.5,
                        row.min.y + 0.5,
                        row.max.x - 0.5,
                        row.max.y - 0.5
                    )
                ) > 40
            );
        }
        // The cover's row is shaded; the key plan's after it is not.
        let own = rows.row_boxes[0];
        let blank = bx(
            own.min.x + 80.0,
            own.min.y + 0.8,
            own.min.x + 140.0,
            own.max.y - 0.8,
        );
        assert!(shade_in(&paper, &blank) > pixels_of(&blank) * 9 / 10);
        assert_eq!(
            shade_in(
                &paper,
                &bx(
                    blank.min.x,
                    rows.row_boxes[1].min.y + 0.8,
                    blank.max.x,
                    rows.row_boxes[1].max.y - 0.8
                )
            ),
            0
        );
        // No utility legend: its block, bottom left of the title block, is bare.
        let frame = sheet_frame(&set.sheets[0]).expect("the cover has a frame");
        let legend = frame.cell("legend").expect("the frame has a legend block");
        let inside = legend.rect.inflated(-2.0);
        assert_eq!(ink_in(&paper, &inside), 0);
    }

    #[test]
    fn a_realistic_cover_is_legible() {
        // Eleven sheets of mixed scales and papers, four revisions, one with a long description:
        // every row and revision listed, nothing below 1.8 mm.
        let model = Model::new();
        let source = source_of(&model);
        let mut set = SheetSet {
            numbering: String::from("{set}-{n:02}"),
            revisions: vec![
                revision("A", "12/01/26", "PRELIMINARY ISSUE FOR COMMENT", "JC"),
                revision("B", "03/02/26", "KERBS ADDED ALONG THE NORTH SIDE", "RE"),
                revision(
                    "C",
                    "17/03/26",
                    "DRAINAGE REDESIGNED BETWEEN CH 120 AND CH 480 FOLLOWING THE GEOTECHNICAL \
                     REPORT; PIT LIDS, INVERT LEVELS AND PIPE CLASSES REVISED THROUGHOUT; SEE \
                     SHEETS 04 TO 07",
                    "JC",
                ),
                revision("D", "02/04/26", "ISSUED FOR CONSTRUCTION", "MK"),
            ],
            ..SheetSet::default()
        };
        set.defaults.set_number = String::from("C-104");
        let names = [
            "GENERAL ARRANGEMENT AND KEY PLAN",
            "SITE PLAN",
            "PLAN SHEET 1",
            "PLAN SHEET 2",
            "PLAN SHEET 3",
            "LONGITUDINAL SECTION MC01",
            "CROSS SECTIONS CH 0 TO CH 240",
            "CROSS SECTIONS CH 260 TO CH 500",
            "DRAINAGE DETAILS",
            "PAVEMENT DETAILS AND TYPICAL SECTIONS",
            "GENERAL NOTES",
        ];
        let scales = [
            2000.0, 1000.0, 500.0, 500.0, 500.0, 500.0, 200.0, 200.0, 50.0, 20.0, 0.0,
        ];
        let papers = [
            PaperSize::A1,
            PaperSize::A1,
            PaperSize::A1,
            PaperSize::A1,
            PaperSize::A1,
            PaperSize::A3,
            PaperSize::A3,
            PaperSize::A3,
            PaperSize::A3,
            PaperSize::A3,
            PaperSize::A4,
        ];
        for (i, name) in names.iter().enumerate() {
            let mut sheet = sheet_named(&format!("s{}", i + 1), name);
            sheet.paper = papers[i];
            sheet.viewports.push(Viewport {
                id: format!("vp{}", i + 1),
                kind: if scales[i] > 0.0 {
                    ViewportKind::Plan
                } else {
                    ViewportKind::Notes
                },
                scale: if scales[i] > 0.0 { scales[i] } else { 500.0 },
                rect: bx(30.0, 40.0, 200.0, 200.0),
                ..Viewport::default()
            });
            set.sheets.push(sheet);
        }
        set.sheets[9]
            .fields
            .insert(String::from("revision"), String::from("B"));
        let cover =
            register_sheet(&set, &SheetTemplate::default()).expect("the cover is made");
        let mut one = vec![cover];
        prepare_for_append(&set, &mut one);
        set.sheets.insert(0, one.remove(0));

        let (_, stats) = painted(&set, &source, false, "a_realistic_cover");
        assert!(stats.problems.is_empty(), "{:?}", stats.problems);
        let rows = layout_of(&set, &set.sheets[0].viewports[0]);
        let issued = layout_of(&set, &set.sheets[0].viewports[1]);
        assert_eq!(rows.rows_hidden, 0);
        assert_eq!(issued.rows_hidden, 0);
        for layout in [&rows, &issued] {
            assert!(layout.cap_mm >= 1.8, "{}", layout.cap_mm);
            for item in &layout.texts {
                assert!(item.squeeze >= 0.8, "{}", item.text);
            }
        }
    }

    #[test]
    fn a_long_register_continues_in_a_second_block_and_long_revisions_wrap() {
        // A cover over 150 sheets and four revisions, one described at length: the register fills
        // two blocks of the smallest text and counts the rest; the long description wraps inside
        // its cell and its row is taller.
        let model = Model::new();
        let source = source_of(&model);
        let mut set = SheetSet::default();
        for i in 1..=150 {
            set.sheets.push(sheet_named(
                &format!("s{}", i + 1),
                &format!("PLAN AND LONGITUDINAL SECTION CH {}", i * 100),
            ));
        }
        set.revisions = vec![
            revision("A", "01/02/26", "FIRST ISSUE", "JC"),
            revision("B", "03/02/26", "KERBS ADDED", "RE"),
            revision(
                "C",
                "05/02/26",
                "LEVELS CHECKED AGAINST THE NEW CONTROL, DRAINAGE PITS RENUMBERED AND THE \
                 EASTERN BOUNDARY REDRAWN FROM THE LATEST TITLE SEARCH",
                "JC",
            ),
            revision("D", "09/02/26", "ISSUED FOR CONSTRUCTION", "RE"),
        ];
        let cover =
            register_sheet(&set, &SheetTemplate::default()).expect("the cover is made");
        set.sheets.insert(0, cover);

        let (paper, stats) = painted(&set, &source, false, "a_long_register");
        let index = &set.sheets[0].viewports[0];
        let revisions = &set.sheets[0].viewports[1];
        let rows = layout_of(&set, index);
        assert_eq!(rows.blocks, 2);
        assert!(double_eq(rows.cap_mm, 1.8), "{}", rows.cap_mm);
        assert!(rows.rows_hidden > 0);
        assert_eq!(stats.problems.len(), 1);
        assert!(
            stats.problems[0].starts_with(&format!("{}: ", index.id)),
            "{}",
            stats.problems[0]
        );
        // The last row shown is in the second block, and it is drawn.
        let last = rows.row_boxes[rows.row_boxes.len() - 1];
        assert!(near(last.min.x, index.rect.center().x, 1e-9));
        assert!(
            ink_in(
                &paper,
                &bx(
                    last.min.x + 0.3,
                    last.min.y + 0.3,
                    last.min.x + 40.0,
                    last.max.y - 0.3
                )
            ) > 20
        );

        let issued = layout_of(&set, revisions);
        assert_eq!(issued.rows_shown, 4);
        assert_eq!(issued.rows_hidden, 0);
        // Newest first: D, then the long C, whose row holds several lines.
        let long_row = issued.row_boxes[1];
        assert!(long_row.height() > 2.0 * issued.row_height_mm);
        assert!(near(
            issued.row_boxes[0].height(),
            issued.row_height_mm,
            1e-9
        ));
        // Ink on the wrapped lines, low in the tall row as well as at its top.
        assert!(
            ink_in(
                &paper,
                &bx(
                    long_row.min.x + 20.0,
                    long_row.min.y + 0.3,
                    long_row.max.x - 10.0,
                    long_row.min.y + issued.row_height_mm
                )
            ) > 20
        );
    }
}
