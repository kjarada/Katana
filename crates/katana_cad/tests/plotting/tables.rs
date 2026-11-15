// The drawing register and the revision table (include/katana/cad/plotting/
// tables.hpp): their rows, the table layout that fits them into a viewport,
// the register cover sheet and the one undoable step that adds it;
// tests/cad/plotting/test_tables.cpp.
//
// Most layouts are measured with a fixed-width "font" - every character half
// its cap height wide, condensed to 0.9 of that (TableStyle::x_factor), so a
// character is 0.45 of the cap height - and the expected positions are worked
// by hand from it.

use katana_cad::plotting::frame::{built_in_frame, parse_frame};
use katana_cad::plotting::generators::grid_sheets;
use katana_cad::plotting::layout::{
    TILING_GUTTER_MM, is_tileable, minimum_size, preset_cells, tiling_area, tiling_rank,
};
use katana_cad::plotting::sheet_commands::{ADD_SHEETS, add_sheets};
use katana_cad::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use katana_cad::plotting::sheet_set::{
    automatic_title, expand_template, mark_label, resolve_fields, sheet_scale_text,
    viewport_kind_from,
};
use katana_cad::plotting::tables::{
    add_register_sheet, drawing_register, estimate_text_width, layout_table, layout_viewport_table,
    register_sheet, revision_rows, table_spec_for,
};
use katana_cad::plotting::{
    FieldContext, FrameRole, FrameText, GridRequest, HorizontalJustify, RegisterRow, Revision,
    Sheet, SheetSet, SheetTemplate, TableColumn, TableLayout, TableSpec, TableStyle, TableText,
    TextWidth, TilingPreset, Viewport, ViewportKind,
};
use katana_cad::{Document, PaperSize};
use katana_core::ErrorCode;
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2};

use crate::{double_eq, near, rect};

fn ok<T>(result: katana_core::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

// Every character half its cap height wide, bold or not.
fn monospace(text: &str, cap_mm: f64, _bold: bool) -> f64 {
    0.5 * cap_mm * text.len() as f64
}

fn layout(spec: &TableSpec, area: Box2) -> TableLayout {
    layout_table(spec, &area, Some(&monospace), &TableStyle::default())
}

fn column(header: &str, justify: HorizontalJustify, stretch: bool, wrap: bool) -> TableColumn {
    TableColumn {
        header: String::from(header),
        justify,
        stretch,
        wrap,
    }
}

fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|cell| String::from(*cell)).collect()
}

fn revision(code: &str, date: &str, description: &str, by: &str) -> Revision {
    Revision {
        code: String::from(code),
        date: String::from(date),
        description: String::from(description),
        by: String::from(by),
    }
}

// A two-column table - a centred number and a stretching title - of `count`
// rows "1".."count" titled "SHEET 1"...
fn numbered(count: usize, heading: &str) -> TableSpec {
    TableSpec {
        heading: String::from(heading),
        columns: vec![
            column("No.", HorizontalJustify::Centre, false, false),
            column("TITLE", HorizontalJustify::Left, true, false),
        ],
        rows: (1..=count)
            .map(|i| vec![format!("{i}"), format!("SHEET {i}")])
            .collect(),
        ..TableSpec::default()
    }
}

fn find_text<'a>(layout: &'a TableLayout, text: &str) -> Option<&'a TableText> {
    layout.texts.iter().find(|item| item.text == text)
}

fn count_texts(layout: &TableLayout, text: &str) -> usize {
    layout.texts.iter().filter(|item| item.text == text).count()
}

// Where a text lies on the paper when set with `measure`: from its baseline
// up by its cap height, as wide as it is drawn (condensed and squeezed).
fn extent_of(item: &TableText, measure: &TextWidth<'_>) -> Box2 {
    let width = measure(&item.text, item.cap_mm, item.bold) * item.x_factor * item.squeeze;
    let mut left = item.anchor.x;
    if item.horizontal == HorizontalJustify::Centre {
        left -= width / 2.0;
    } else if item.horizontal == HorizontalJustify::Right {
        left -= width;
    }
    rect(
        left,
        item.anchor.y,
        left + width,
        item.anchor.y + item.cap_mm,
    )
}

// Every text and rule of `layout` inside `area`.
#[track_caller]
fn expect_inside(layout: &TableLayout, area: &Box2, measure: &TextWidth<'_>) {
    let slack = area.inflated(1e-9);
    for item in &layout.texts {
        assert!(
            slack.contains_box(extent_of(item, measure)),
            "{}",
            item.text
        );
    }
    for rule in &layout.rules {
        assert!(slack.contains(rule.from) && slack.contains(rule.to));
    }
}

fn viewport_of(kind: ViewportKind, scale: f64, exaggeration: f64) -> Viewport {
    Viewport {
        kind,
        scale,
        vertical_exaggeration: exaggeration,
        ..Viewport::default()
    }
}

fn sheet_with(id: &str, name: &str, mut viewport: Viewport) -> Sheet {
    viewport.id = format!("vp-{id}");
    Sheet {
        id: String::from(id),
        name: String::from(name),
        viewports: vec![viewport],
        ..Sheet::default()
    }
}

// A key plan, a plan, a long section and a sheet of notes on A1, with two
// revisions issued, numbered "DS-7-01"...
fn four_sheets() -> SheetSet {
    let mut set = SheetSet {
        numbering: String::from("{set}-{n:02}"),
        revisions: vec![
            revision("A", "01/02/26", "FIRST ISSUE", "JC"),
            revision("B", "03/02/26", "KERBS ADDED", "RE"),
        ],
        ..SheetSet::default()
    };
    set.defaults.set_number = String::from("DS-7");
    set.sheets.push(sheet_with(
        "s1",
        "KEY PLAN",
        viewport_of(ViewportKind::KeyPlan, 2000.0, 1.0),
    ));
    set.sheets.push(sheet_with(
        "s2",
        "PLAN 1",
        viewport_of(ViewportKind::Plan, 500.0, 1.0),
    ));
    set.sheets.push(sheet_with(
        "s3",
        "LONG SECTION",
        viewport_of(ViewportKind::LongSection, 500.0, 10.0),
    ));
    set.sheets.push(sheet_with(
        "s4",
        "NOTES",
        viewport_of(ViewportKind::Notes, 500.0, 1.0),
    ));
    set.sheets[3].paper = PaperSize::A1;
    set
}

fn register_row(
    sheet_id: &str,
    number: &str,
    title: &str,
    scale: &str,
    paper: &str,
    revision: &str,
) -> RegisterRow {
    RegisterRow {
        sheet_id: String::from(sheet_id),
        number: String::from(number),
        title: String::from(title),
        scale: String::from(scale),
        paper: String::from(paper),
        revision: String::from(revision),
    }
}

mod sheet_tables {
    use super::*;

    // ---- the rows ---------------------------------------------------------------------

    #[test]
    fn the_register_lists_every_sheet_as_its_title_block_prints_it() {
        let rows = drawing_register(&four_sheets());
        let expected = vec![
            register_row("s1", "DS-7-01", "KEY PLAN", "1:2000", "A3", "B"),
            register_row("s2", "DS-7-02", "PLAN 1", "1:500", "A3", "B"),
            register_row("s3", "DS-7-03", "LONG SECTION", "H 1:500 V 1:50", "A3", "B"),
            register_row("s4", "DS-7-04", "NOTES", "N.T.S.", "A1", "B"),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn what_is_typed_on_a_sheet_wins_in_the_register_as_on_the_sheet() {
        let mut set = four_sheets();
        // This sheet was revised again.
        set.sheets[1]
            .fields
            .insert(String::from("revision"), String::from("C"));
        set.sheets[2]
            .fields
            .insert(String::from("sheet_number"), String::from("LS-1"));
        set.sheets[3]
            .fields
            .insert(String::from("sheet_name"), String::from("GENERAL NOTES"));
        set.sheets[0]
            .fields
            .insert(String::from("scale"), String::from("1:2000 @ A3"));
        let rows = drawing_register(&set);
        assert_eq!(rows[1].revision, "C");
        assert_eq!(rows[0].revision, "B");
        assert_eq!(rows[2].number, "LS-1");
        assert_eq!(rows[3].title, "GENERAL NOTES");
        assert_eq!(rows[0].scale, "1:2000 @ A3");
    }

    #[test]
    fn after_a_reorder_each_row_carries_the_number_its_sheet_has_now() {
        let mut set = four_sheets();
        // Typed: it goes where its sheet goes.
        set.sheets[2]
            .fields
            .insert(String::from("sheet_number"), String::from("LS-1"));
        // The notes to the front, as move_sheet(3, 0) does.
        set.sheets.rotate_left(3);
        let rows = drawing_register(&set);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].sheet_id, "s4");
        assert_eq!(rows[0].number, "DS-7-01");
        assert_eq!(rows[1].sheet_id, "s1");
        assert_eq!(rows[1].number, "DS-7-02");
        assert_eq!(rows[3].sheet_id, "s3");
        assert_eq!(rows[3].number, "LS-1");
    }

    #[test]
    fn revisions_are_listed_newest_first_and_can_be_limited_to_the_newest() {
        let mut set = four_sheets();
        set.revisions
            .push(revision("C", "07/02/26", "LEVELS CHECKED", "JC"));
        let codes = |limit: usize| -> String {
            revision_rows(&set, limit)
                .iter()
                .map(|revision| revision.code.as_str())
                .collect()
        };
        assert_eq!(codes(0), "CBA");
        assert_eq!(codes(2), "CB");
        assert_eq!(codes(1), "C");
        assert_eq!(codes(10), "CBA");
        assert_eq!(
            revision_rows(&set, 1)[0],
            revision("C", "07/02/26", "LEVELS CHECKED", "JC")
        );
        assert!(revision_rows(&SheetSet::default(), 0).is_empty());
    }

    // ---- the layout -------------------------------------------------------------------

    #[test]
    fn a_short_table_is_set_in_the_largest_text_ruled_heavy_outside_and_light_inside() {
        // 100 x 80 mm, three rows. At 2.5 mm text: the heading 1.25 x 2.5 =
        // 3.125 mm in a 6.25 mm band, the header row 5 mm, rows 5 mm - 26.25
        // mm in all, so the largest text fits. The number column is its header
        // "No." (3 x 0.45 x 2.5 = 3.375) plus 1.25 each side: 5.875 mm; the
        // title column the other 94.125.
        let spec = numbered(3, "REGISTER");
        let table = layout(&spec, rect(0.0, 0.0, 100.0, 80.0));
        double_eq(table.cap_mm, 2.5);
        double_eq(table.row_height_mm, 5.0);
        assert_eq!(table.blocks, 1);
        assert_eq!(table.rows_shown, 3);
        assert_eq!(table.rows_hidden, 0);

        // The heading's band is 80 down to 73.75, the header's to 68.75, then
        // a row every 5 mm, in order.
        assert_eq!(table.row_boxes.len(), 3);
        assert_eq!(table.row_boxes[0], rect(0.0, 63.75, 100.0, 68.75));
        assert_eq!(table.row_boxes[1], rect(0.0, 58.75, 100.0, 63.75));
        assert_eq!(table.row_boxes[2], rect(0.0, 53.75, 100.0, 58.75));

        // Row 1's texts on the baseline that centres a 2.5 mm capital in the
        // 5 mm row (68.75 - 3.75 = 65): the number centred in its column, the
        // title 1.25 mm into its own.
        let number = find_text(&table, "1").expect("the number 1");
        double_eq(number.anchor.x, 5.875 / 2.0);
        double_eq(number.anchor.y, 65.0);
        assert_eq!(number.horizontal, HorizontalJustify::Centre);
        assert!(!number.bold);
        let title = find_text(&table, "SHEET 1").expect("SHEET 1");
        double_eq(title.anchor.x, 5.875 + 1.25);
        double_eq(title.x_factor, 0.9);
        double_eq(title.squeeze, 1.0);
        // The heading in bold at 1.25 times the text; the headers in bold.
        let heading = find_text(&table, "REGISTER").expect("the heading");
        assert!(heading.bold);
        double_eq(heading.cap_mm, 3.125);
        double_eq(heading.anchor.y, 80.0 - (6.25 + 3.125) / 2.0);
        assert!(find_text(&table, "TITLE").expect("the header TITLE").bold);

        // Rules: the box, under the heading and under the header at 0.25; the
        // column rule and one under each row at 0.13. The column rule runs
        // down to the foot of the box, a ruled form with room for more.
        let heavy = table
            .rules
            .iter()
            .filter(|rule| rule.weight_mm == 0.25)
            .count();
        let light = table
            .rules
            .iter()
            .filter(|rule| rule.weight_mm == 0.13)
            .count();
        assert_eq!(heavy, 6);
        assert_eq!(light, 4);
        assert!(table.rules.iter().any(|rule| {
            rule.weight_mm == 0.13
                && (rule.from.x - 5.875).abs() < 1e-9
                && (rule.to.x - 5.875).abs() < 1e-9
                && rule.from.y == 73.75
                && rule.to.y == 0.0
        }));
        expect_inside(&table, &rect(0.0, 0.0, 100.0, 80.0), &monospace);
    }

    #[test]
    fn more_rows_are_set_in_smaller_text_until_they_fit() {
        // 20 rows take 2.5 + 2 + 40 = 44.5 cap heights (heading, header,
        // rows). In 89.2 mm that is 2.004 mm at most: 2.0 mm text, the largest
        // step.
        let table = layout(&numbered(20, "REGISTER"), rect(0.0, 0.0, 100.0, 89.2));
        near(table.cap_mm, 2.0, 1e-12);
        assert_eq!(table.blocks, 1);
        assert_eq!(table.rows_shown, 20);
        near(table.row_height_mm, 4.0, 1e-12);
        expect_inside(&table, &rect(0.0, 0.0, 100.0, 89.2), &monospace);
    }

    #[test]
    fn rows_that_do_not_fit_the_smallest_text_continue_in_a_second_block() {
        // 30 rows need 64.5 cap heights in one block - 116.1 mm at the 1.8 mm
        // minimum, more than 89.2. In two blocks of 15 they need 34.5: 2.5 mm
        // text fits (86.25 mm), so the rows continue side by side at full
        // size.
        let area = rect(0.0, 0.0, 200.0, 89.2);
        let table = layout(&numbered(30, "REGISTER"), area);
        double_eq(table.cap_mm, 2.5);
        assert_eq!(table.blocks, 2);
        assert_eq!(table.rows_shown, 30);
        assert_eq!(table.row_boxes.len(), 30);
        // Rows 1 to 15 down the first block, 16 to 30 down the second, in
        // order.
        double_eq(table.row_boxes[14].min.x, 0.0);
        double_eq(table.row_boxes[15].min.x, 100.0);
        double_eq(table.row_boxes[15].max.y, table.row_boxes[0].max.y);
        for i in 1..15 {
            double_eq(table.row_boxes[i].max.y, table.row_boxes[i - 1].min.y);
        }
        // Each block has its own header; the heading is once, across both.
        assert_eq!(count_texts(&table, "TITLE"), 2);
        assert_eq!(count_texts(&table, "REGISTER"), 1);
        // The blocks are parted by a heavy rule from the header down.
        assert!(table.rules.iter().any(|rule| {
            rule.weight_mm == 0.25
                && (rule.from.x - 100.0).abs() < 1e-9
                && (rule.from.y - (89.2 - 6.25)).abs() < 1e-9
                && rule.to.y == 0.0
        }));
        expect_inside(&table, &area, &monospace);
    }

    #[test]
    fn what_two_blocks_of_the_smallest_text_cannot_hold_is_counted_on_the_last_line() {
        // 100 rows in 200 x 89.2 at 1.8 mm: 89.2 - 1.8 x 4.5 = 81.1 mm under
        // the headers, 22 rows of 3.6 mm to a block, 44 lines in two. The last
        // line counts the rest: 43 rows shown, "+57 more" right-aligned in the
        // last line of the second block, grey.
        let area = rect(0.0, 0.0, 200.0, 89.2);
        let table = layout(&numbered(100, "REGISTER"), area);
        double_eq(table.cap_mm, 1.8);
        assert_eq!(table.blocks, 2);
        assert_eq!(table.rows_shown, 43);
        assert_eq!(table.rows_hidden, 57);
        let more = find_text(&table, "+57 more").expect("the count");
        assert!(more.muted);
        assert_eq!(more.horizontal, HorizontalJustify::Right);
        near(more.anchor.x, 200.0 - 0.9, 1e-9);
        // Its line's top is 81.1 - 21 x 3.6 = 5.5; its baseline 2.7 under
        // that.
        near(more.anchor.y, 2.8, 1e-9);
        // The rows shown are the first 43, in order.
        assert!(find_text(&table, "SHEET 43").is_some());
        assert!(find_text(&table, "SHEET 44").is_none());
        // The count is one cell across its block: the second block's column
        // rule (after its "100"-wide number column, 3 x 0.45 x 1.8 + 1.8 =
        // 4.23 mm) stops at the count's line, 5.5 mm up; the first block's
        // runs to the foot of the box.
        let column_rule_foot = |x: f64| {
            table
                .rules
                .iter()
                .find(|rule| {
                    rule.weight_mm == 0.13
                        && (rule.from.x - x).abs() < 1e-9
                        && (rule.to.x - x).abs() < 1e-9
                })
                .map_or(-1.0, |rule| rule.to.y)
        };
        near(column_rule_foot(4.23), 0.0, 1e-9);
        near(column_rule_foot(104.23), 5.5, 1e-9);
        expect_inside(&table, &area, &monospace);
    }

    #[test]
    fn a_revision_table_that_does_not_fit_counts_the_earlier_ones_and_stays_one_block() {
        let mut set = SheetSet::default();
        for i in 0..40 {
            set.revisions
                .push(revision(&format!("{i}"), "01/01/26", "ISSUE", "JC"));
        }
        let table_view = Viewport {
            kind: ViewportKind::Revisions,
            rect: rect(0.0, 0.0, 120.0, 40.0),
            ..Viewport::default()
        };
        let table = layout_viewport_table(&set, 0, &table_view, Some(&monospace));
        assert_eq!(table.blocks, 1);
        assert!(table.rows_hidden > 0);
        // Newest first: the last issued is at the top.
        assert!(!table.row_boxes.is_empty());
        assert!(find_text(&table, "39").is_some());
        assert!(find_text(&table, "0").is_none());
        assert!(find_text(&table, &format!("+{} earlier", table.rows_hidden)).is_some());
        assert_eq!(table.rows_shown + table.rows_hidden, 40);
        expect_inside(&table, &table_view.rect, &monospace);
    }

    #[test]
    fn the_highlighted_row_is_shaded_across_its_block() {
        let mut spec = numbered(30, "REGISTER");
        spec.highlight = Some(17);
        let table = layout(&spec, rect(0.0, 0.0, 200.0, 89.2));
        assert_eq!(table.shaded.len(), 1);
        assert_eq!(table.shaded[0], table.row_boxes[17]);
        double_eq(table.shaded[0].width(), 100.0);
    }

    #[test]
    fn a_long_description_wraps_and_makes_its_row_taller() {
        // REV | DATE | DESCRIPTION | BY in 60 mm. At 2.5 mm the fixed columns
        // are "REV" 5.875, "01/02/26" 11.5 and "BY" 4.75 wide, leaving the
        // description 37.875 mm: 35.375 for text, 31.4 characters of 1.125 mm.
        let description =
            "KERB LINES ON THE NORTH SIDE MOVED AFTER THE SECOND SURVEY OF THE CORRIDOR";
        let spec = TableSpec {
            columns: vec![
                column("REV", HorizontalJustify::Centre, false, false),
                column("DATE", HorizontalJustify::Centre, false, false),
                column("DESCRIPTION", HorizontalJustify::Left, true, true),
                column("BY", HorizontalJustify::Centre, false, false),
            ],
            rows: vec![row(&["A", "01/02/26", description, "JC"])],
            ..TableSpec::default()
        };
        let area = rect(0.0, 0.0, 60.0, 60.0);
        let table = layout(&spec, area);
        double_eq(table.cap_mm, 2.5);
        let room = 37.875 - 2.5;

        let lines: Vec<&TableText> = table
            .texts
            .iter()
            .filter(|item| !item.bold && (item.anchor.x - (17.375 + 1.25)).abs() < 1e-9)
            .collect();
        assert!(lines.len() >= 2);
        let mut joined = String::new();
        for (i, line) in lines.iter().enumerate() {
            // Each line fits its column without a squeeze, and the next word
            // would not have: the words break as late as they can.
            let width = monospace(&line.text, 2.5, false) * 0.9;
            assert!(width <= room + 1e-9);
            double_eq(line.squeeze, 1.0);
            if let Some(next_line) = lines.get(i + 1) {
                let next = next_line.text.split(' ').next().unwrap_or_default();
                assert!(monospace(&format!("{} {next}", line.text), 2.5, false) * 0.9 > room);
                // Lines step down 1.5 cap heights.
                double_eq(line.anchor.y - next_line.anchor.y, 3.75);
            }
            if i > 0 {
                joined.push(' ');
            }
            joined.push_str(&line.text);
        }
        assert_eq!(joined, description);
        // The row is as tall as its lines: 2 cap heights, and 1.5 more a line.
        assert_eq!(table.row_boxes.len(), 1);
        double_eq(
            table.row_boxes[0].height(),
            2.5 * (2.0 + 1.5 * (lines.len() - 1) as f64),
        );
        // The other cells stay on the row's first line.
        double_eq(
            find_text(&table, "JC").expect("JC").anchor.y,
            lines[0].anchor.y,
        );
        expect_inside(&table, &area, &monospace);
    }

    #[test]
    fn a_line_break_starts_a_line_only_in_a_wrapping_cell() {
        // A sheet name typed over two lines is one line in the register; a
        // description's line break starts a new line, and a "\r\n" leaves no
        // stray '\r' in a word.
        let spec = TableSpec {
            heading: String::from("SHEET\nLIST"),
            columns: vec![
                column("TITLE", HorizontalJustify::Left, true, false),
                column("DESCRIPTION", HorizontalJustify::Left, false, true),
            ],
            rows: vec![row(&["PLAN\nNORTH", "FIRST\r\nSECOND"])],
            ..TableSpec::default()
        };
        let table = layout(&spec, rect(0.0, 0.0, 120.0, 60.0));
        assert!(find_text(&table, "SHEET LIST").is_some());
        assert!(find_text(&table, "PLAN NORTH").is_some());
        let first = find_text(&table, "FIRST").expect("FIRST");
        let second = find_text(&table, "SECOND").expect("SECOND");
        double_eq(first.anchor.y - second.anchor.y, 1.5 * table.cap_mm);
        for item in &table.texts {
            assert!(!item.text.contains(['\r', '\n']), "{}", item.text);
        }
        // The row is two lines high: 2 cap heights and 1.5 more.
        assert_eq!(table.row_boxes.len(), 1);
        double_eq(table.row_boxes[0].height(), 3.5 * table.cap_mm);
    }

    #[test]
    fn a_long_title_is_set_smaller_rather_than_squeezed_hard() {
        // A 40-character title in 40 mm. At 2.5 mm it would be pressed to
        // 0.70 of its width; at 2.2 mm it keeps 0.82, above the 0.8 preferred,
        // and 2.3 mm would give 0.78. So 2.2 mm, though 2.5 mm fits the
        // height.
        let mut spec = numbered(1, "REGISTER");
        spec.rows[0][1] = "X".repeat(40);
        let table = layout(&spec, rect(0.0, 0.0, 40.0, 80.0));
        near(table.cap_mm, 2.2, 1e-12);
        let title = find_text(&table, &"X".repeat(40)).expect("the title");
        assert!(title.squeeze >= 0.8);
        assert!(title.squeeze < 1.0);
        expect_inside(&table, &rect(0.0, 0.0, 40.0, 80.0), &monospace);
    }

    #[test]
    fn a_table_with_no_rows_is_its_heading_and_header_and_a_tiny_one_is_nothing() {
        let empty = layout(&numbered(0, "REGISTER"), rect(0.0, 0.0, 100.0, 40.0));
        double_eq(empty.cap_mm, 2.5);
        assert_eq!(empty.rows_shown, 0);
        assert_eq!(empty.rows_hidden, 0);
        assert!(find_text(&empty, "TITLE").is_some());

        // 20 x 5 mm cannot hold the heading and the header even at 1.8 mm.
        let tiny = layout(&numbered(3, "REGISTER"), rect(0.0, 0.0, 20.0, 5.0));
        assert_eq!(tiny.rows_shown, 0);
        assert_eq!(tiny.rows_hidden, 3);
        assert!(tiny.texts.is_empty());
        assert!(tiny.rules.is_empty());
        // No columns, or no rectangle: nothing, every row counted.
        assert_eq!(
            layout_table(
                &TableSpec::default(),
                &rect(0.0, 0.0, 100.0, 100.0),
                None,
                &TableStyle::default()
            )
            .rows_hidden,
            0
        );
        assert_eq!(
            layout_table(
                &numbered(2, "REGISTER"),
                &Box2::default(),
                None,
                &TableStyle::default()
            )
            .rows_hidden,
            2
        );
    }

    #[test]
    fn the_same_table_gives_the_same_layout() {
        let mut spec = numbered(57, "REGISTER");
        spec.highlight = Some(3);
        let area = rect(10.0, 20.0, 190.0, 120.0);
        let style = TableStyle::default();
        assert_eq!(
            layout_table(&spec, &area, None, &style),
            layout_table(&spec, &area, None, &style)
        );
        assert_eq!(layout(&spec, area), layout(&spec, area));
    }

    #[test]
    fn the_estimate_uses_arials_advance_widths() {
        // Arial's cap height is 1467 of its 2048 units, so a 1 mm capital is
        // set on a 2048 / 1467 mm em. A digit is 0.556 em; "W" 0.944 and "I"
        // 0.278.
        let em = 2048.0 / 1467.0;
        near(estimate_text_width("0", 1.0, false), 0.556 * em, 1e-12);
        near(
            estimate_text_width("00", 2.0, false),
            4.0 * 0.556 * em,
            1e-12,
        );
        near(
            estimate_text_width("WI", 1.0, false),
            (0.944 + 0.278) * em,
            1e-12,
        );
        // Bold is wider where Arial Bold is: b is 0.611 against 0.556.
        near(estimate_text_width("b", 1.0, true), 0.611 * em, 1e-12);
        assert!(
            estimate_text_width("SHEET No.", 2.5, true)
                > estimate_text_width("SHEET No.", 2.5, false)
        );
        // A character outside ASCII counts once, whatever its bytes: an
        // accented capital as a digit, a wide ideograph as a full em.
        near(estimate_text_width("\u{c9}", 1.0, false), 0.556 * em, 1e-12);
        near(estimate_text_width("\u{5317}", 1.0, false), em, 1e-12);
        assert_eq!(estimate_text_width("", 2.5, false), 0.0);
    }

    // ---- the viewports --------------------------------------------------------------------

    #[test]
    fn a_register_viewport_lists_the_set_and_highlights_the_sheet_it_is_on() {
        let set = four_sheets();
        let mut view = Viewport {
            kind: ViewportKind::SheetIndex,
            rect: rect(30.0, 40.0, 250.0, 280.0),
            ..Viewport::default()
        };
        let spec = table_spec_for(&set, 2, &view);
        assert_eq!(spec.heading, "DRAWING REGISTER");
        assert_eq!(spec.columns.len(), 5);
        let headers = ["SHEET No.", "TITLE", "SCALE", "PAPER", "REV"];
        for (column, header) in spec.columns.iter().zip(headers) {
            assert_eq!(column.header, header);
        }
        assert!(spec.columns[1].stretch);
        assert_eq!(spec.highlight, Some(2));
        assert_eq!(spec.rows.len(), 4);
        assert_eq!(
            spec.rows[2],
            row(&["DS-7-03", "LONG SECTION", "H 1:500 V 1:50", "A3", "B"])
        );
        // A title of its own replaces the heading.
        view.title = String::from("SHEET LIST");
        assert_eq!(table_spec_for(&set, 2, &view).heading, "SHEET LIST");

        // Laid out with the estimate: every row at the largest text, the
        // highlight on row 3, everything inside the viewport.
        let table = layout_viewport_table(&set, 2, &view, None);
        double_eq(table.cap_mm, 2.5);
        assert_eq!(table.rows_shown, 4);
        assert_eq!(table.shaded.len(), 1);
        assert_eq!(table.shaded[0], table.row_boxes[2]);
        expect_inside(&table, &view.rect, &estimate_text_width);
    }

    #[test]
    fn a_revision_viewport_shows_its_newest_revisions_only() {
        let mut set = four_sheets();
        set.revisions
            .push(revision("C", "07/02/26", "LEVELS CHECKED", "JC"));
        let view = Viewport {
            kind: ViewportKind::Revisions,
            rect: rect(260.0, 40.0, 400.0, 120.0),
            revision_limit: 2,
            ..Viewport::default()
        };
        let spec = table_spec_for(&set, 0, &view);
        assert_eq!(spec.heading, "REVISIONS");
        assert_eq!(spec.columns.len(), 4);
        assert_eq!(spec.columns[2].header, "DESCRIPTION");
        assert!(spec.columns[2].wrap);
        assert_eq!(spec.max_blocks, 1);
        assert!(spec.highlight.is_none());
        assert_eq!(spec.rows.len(), 2);
        assert_eq!(
            spec.rows[0],
            row(&["C", "07/02/26", "LEVELS CHECKED", "JC"])
        );
        assert_eq!(spec.rows[1][0], "B");

        // Any other kind is no table.
        assert!(
            table_spec_for(&set, 0, &viewport_of(ViewportKind::Plan, 500.0, 1.0))
                .columns
                .is_empty()
        );
        let plan = Viewport {
            rect: view.rect,
            ..viewport_of(ViewportKind::Plan, 500.0, 1.0)
        };
        assert!(layout_viewport_table(&set, 0, &plan, None).texts.is_empty());
    }

    #[test]
    fn the_new_kinds_have_names_titles_ranks_and_sizes() {
        assert_eq!(ViewportKind::SheetIndex.name(), "sheet_index");
        assert_eq!(ViewportKind::Revisions.name(), "revisions");
        assert_eq!(
            viewport_kind_from("sheet_index"),
            Some(ViewportKind::SheetIndex)
        );
        assert_eq!(
            viewport_kind_from("revisions"),
            Some(ViewportKind::Revisions)
        );
        assert_eq!(
            automatic_title(&viewport_of(ViewportKind::SheetIndex, 500.0, 1.0)),
            "DRAWING REGISTER"
        );
        assert_eq!(
            automatic_title(&viewport_of(ViewportKind::Revisions, 500.0, 1.0)),
            "REVISIONS"
        );
        assert!(is_tileable(ViewportKind::SheetIndex));
        assert!(is_tileable(ViewportKind::Revisions));
        assert!(tiling_rank(ViewportKind::SheetIndex) < tiling_rank(ViewportKind::Revisions));
        assert_eq!(minimum_size(ViewportKind::SheetIndex).width, 70.0);
        assert_eq!(minimum_size(ViewportKind::Revisions).height, 20.0);
        // Tables are not drawn to scale: a register sheet is N.T.S.
        let sheet = Sheet {
            viewports: vec![
                viewport_of(ViewportKind::SheetIndex, 500.0, 1.0),
                viewport_of(ViewportKind::Revisions, 500.0, 1.0),
            ],
            ..Sheet::default()
        };
        assert_eq!(sheet_scale_text(&sheet), "N.T.S.");
    }

    // ---- the revision in the title block ------------------------------------------------

    #[test]
    fn the_title_block_revision_is_the_current_one_and_the_built_in_frame_is_left_alone() {
        // The built-in frame has no text for the revision, so it is drawn as
        // it was measured; the revision table is how its sheets show
        // revisions.
        let frame = built_in_frame()
            .as_ref()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(frame.texts_showing("revision").is_empty());

        // A frame text that shows it prints the current revision: the latest
        // issued, or the one typed on the sheet.
        let mut set = four_sheets();
        let rev = FrameText {
            content: String::from("REV {revision}"),
            fields: vec![String::from("revision")],
            ..FrameText::default()
        };
        let context = FieldContext::default();
        assert_eq!(
            expand_template(&rev.content, &resolve_fields(&set, 0, &context)),
            "REV B"
        );
        set.revisions
            .push(revision("C", "07/02/26", "LEVELS CHECKED", "JC"));
        assert_eq!(
            expand_template(&rev.content, &resolve_fields(&set, 0, &context)),
            "REV C"
        );
        set.sheets[0]
            .fields
            .insert(String::from("revision"), String::from("B1"));
        assert_eq!(
            expand_template(&rev.content, &resolve_fields(&set, 0, &context)),
            "REV B1"
        );
    }

    #[test]
    fn a_frame_that_names_the_revision_field_prints_the_current_revision() {
        // A frame written for Katana names its fields as a sheet does. Its
        // "REV {revision}" is a field text, filled like the measured frame's.
        let json = r##"{
            "sheet": {"width_mm": 420, "height_mm": 297,
                      "margins_mm": {"left": 23, "right": 10, "top": 10, "bottom": 35},
                      "viewport_tbf": {"x0": 23, "y0": 35, "x1": 410, "y1": 287},
                      "drawing_border_tbf": {"x0": 22, "y0": 34, "x1": 411, "y1": 289},
                      "title_block_tbf": {"x0": 22, "y0": 9, "x1": 411, "y1": 34},
                      "sheet_edge_tbf": {"x0": 0, "y0": 0, "x1": 420, "y1": 297}},
            "cells": [],
            "entities": [
                {"index": 0, "kind": "text", "role": "label", "anchor_tbf": [300, 20],
                 "cap_height_mm": 2.5, "x_factor": 0.82, "justify": "bottom-left", "angle_deg": 0,
                 "font_face": "Arial", "bold": false, "colour_hex": "#000000",
                 "raw_text": "REV {revision}"},
                {"index": 1, "kind": "text", "role": "label", "anchor_tbf": [300, 15],
                 "cap_height_mm": 2.5, "x_factor": 0.82, "justify": "bottom-left", "angle_deg": 0,
                 "font_face": "Arial", "bold": false, "colour_hex": "#000000",
                 "raw_text": "{Not a field} and {{braces}}"}]}"##;
        let frame = ok(parse_frame(json));
        assert_eq!(frame.texts_showing("revision"), vec![0]);
        let rev = &frame.texts[0];
        assert_eq!(rev.role, FrameRole::Field);
        assert_eq!(rev.content, "REV {revision}");
        // Braces around anything that is not a field name are text.
        assert!(frame.texts[1].fields.is_empty());
        assert_eq!(frame.texts[1].content, "{Not a field} and {{braces}}");

        let mut set = four_sheets();
        assert_eq!(
            expand_template(
                &rev.content,
                &resolve_fields(&set, 1, &FieldContext::default())
            ),
            "REV B"
        );
        set.revisions
            .push(revision("C", "07/02/26", "LEVELS CHECKED", "JC"));
        assert_eq!(
            expand_template(
                &rev.content,
                &resolve_fields(&set, 1, &FieldContext::default())
            ),
            "REV C"
        );
    }

    // ---- storage -----------------------------------------------------------------------

    #[test]
    fn the_new_kinds_and_the_revision_limit_survive_a_json_round_trip() {
        let mut set = four_sheets();
        let cover = Sheet {
            id: String::from("s9"),
            viewports: vec![
                Viewport {
                    id: String::from("vp8"),
                    kind: ViewportKind::SheetIndex,
                    rect: rect(24.0, 36.0, 270.0, 286.0),
                    ..Viewport::default()
                },
                Viewport {
                    id: String::from("vp9"),
                    kind: ViewportKind::Revisions,
                    rect: rect(273.0, 36.0, 409.0, 286.0),
                    revision_limit: 3,
                    ..Viewport::default()
                },
            ],
            ..Sheet::default()
        };
        set.sheets.insert(0, cover);

        let text = ok(sheet_set_to_json(&set));
        assert!(text.contains("\"kind\":\"sheet_index\""));
        assert!(text.contains("\"kind\":\"revisions\""));
        assert!(text.contains("\"revision_limit\":3"));
        // Only the one viewport that limits its revisions says so.
        assert_eq!(
            text.find("\"revision_limit\""),
            text.rfind("\"revision_limit\"")
        );
        assert_eq!(ok(sheet_set_from_json(&text)), set);
    }

    #[test]
    fn a_revision_limit_that_is_not_a_count_is_refused() {
        let with_limit = |limit: &str| {
            format!(
                r#"{{"format": "katana-sheets", "version": 1, "sheets": [{{"id": "s1", "viewports": [{{"id": "vp1", "kind": "revisions", "revision_limit": {limit}}}]}}]}}"#
            )
        };
        let good = ok(sheet_set_from_json(&with_limit("4")));
        assert_eq!(good.sheets[0].viewports[0].revision_limit, 4);
        for bad in ["-1", "2.5", "\"3\"", "true"] {
            let read = sheet_set_from_json(&with_limit(bad));
            assert_eq!(
                read.expect_err("not a count").code,
                ErrorCode::ParseFailure,
                "{bad}"
            );
        }
    }

    // ---- the register cover sheet ------------------------------------------------------------

    #[test]
    fn the_register_sheet_has_the_register_beside_the_revisions() {
        let sheet = ok(register_sheet(&four_sheets(), &SheetTemplate::default()));
        assert_eq!(sheet.name, "DRAWING REGISTER");
        assert_eq!(sheet.id, "s1");
        assert_eq!(sheet.paper, PaperSize::A3);
        assert!(!sheet.frame_legend);
        assert_eq!(sheet.viewports.len(), 2);
        let index = &sheet.viewports[0];
        let revisions = &sheet.viewports[1];
        assert_eq!(index.kind, ViewportKind::SheetIndex);
        assert_eq!(revisions.kind, ViewportKind::Revisions);
        assert_eq!(index.id, "vp1");
        assert_eq!(revisions.id, "vp2");
        // "Main and panel right": the register in the 66% cell, the revisions
        // in the panel beside it.
        let cells = preset_cells(
            TilingPreset::MainRight,
            &tiling_area(&sheet),
            TILING_GUTTER_MM,
        );
        assert_eq!(index.rect, cells[0]);
        assert_eq!(revisions.rect, cells[1]);
        // The same set gives the same sheet.
        assert_eq!(
            ok(register_sheet(&four_sheets(), &SheetTemplate::default())),
            sheet
        );

        // On the paper asked for.
        let a1 = SheetTemplate {
            paper: PaperSize::A1,
            ..SheetTemplate::default()
        };
        let big = ok(register_sheet(&four_sheets(), &a1));
        assert_eq!(big.paper, PaperSize::A1);
        assert_eq!(
            big.viewports[0].rect,
            preset_cells(
                TilingPreset::MainRight,
                &tiling_area(&big),
                TILING_GUTTER_MM
            )[0]
        );
    }

    #[test]
    fn a_set_with_a_register_is_not_given_a_second() {
        let mut set = four_sheets();
        set.sheets[2]
            .viewports
            .push(viewport_of(ViewportKind::SheetIndex, 500.0, 1.0));
        let error = register_sheet(&set, &SheetTemplate::default()).expect_err("a second register");
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        assert_eq!(error.context, "sheet DS-7-03 (LONG SECTION)");
    }

    #[test]
    fn a_register_of_a_many_sheet_set_fills_its_cell_and_counts_the_rest() {
        // 150 sheets and the cover: at the smallest text the A3 cell holds two
        // blocks of rows; the rest are counted, and the count is every row not
        // shown.
        let mut set = SheetSet::default();
        for i in 1..=150 {
            set.sheets.push(sheet_with(
                &format!("s{}", i + 1),
                &format!("PLAN TILE {i}"),
                viewport_of(ViewportKind::Plan, 500.0, 1.0),
            ));
        }
        let cover = ok(register_sheet(&set, &SheetTemplate::default()));
        set.sheets.insert(0, cover);
        let view = &set.sheets[0].viewports[0];
        let table = layout_viewport_table(&set, 0, view, None);
        double_eq(table.cap_mm, 1.8);
        assert_eq!(table.blocks, 2);
        assert!(table.rows_shown > 100);
        assert_eq!(table.rows_shown + table.rows_hidden, 151);
        assert!(table.rows_hidden > 0);
        assert!(find_text(&table, &format!("+{} more", table.rows_hidden)).is_some());
        // The cover is the sheet the register is on: its own row is shaded.
        assert_eq!(table.shaded.len(), 1);
        assert_eq!(table.shaded[0], table.row_boxes[0]);
        expect_inside(&table, &view.rect, &estimate_text_width);
    }

    #[test]
    fn adding_the_register_puts_it_first_in_one_step_with_fresh_ids() {
        let mut document = Document::default();
        let grid = GridRequest {
            area: Box2::new(Point2::new(0.0, 0.0), Point2::new(400.0, 200.0)),
            scale: 500.0,
            ..GridRequest::default()
        };
        let tiles = ok(grid_sheets(&grid, None));
        ok(add_sheets(&mut document, tiles, ADD_SHEETS));
        let before = SheetSet::clone(&document.sheet_set());
        assert_eq!(before.sheets.len(), 7); // a key plan and six tiles, s1 to s7
        let steps = document.history().undo_count();

        let id = ok(add_register_sheet(&mut document, &SheetTemplate::default()));
        let set = document.sheet_set();
        assert_eq!(set.sheets.len(), 8);
        assert_eq!(document.history().undo_count(), steps + 1);
        assert_eq!(set.sheets[0].name, "DRAWING REGISTER");
        assert_eq!(set.sheets[0].id, id);
        // Fresh ids: after every sheet and viewport already in the set.
        assert_eq!(id, "s8");
        assert_eq!(set.sheets[0].viewports[0].id, "vp8");
        assert_eq!(set.sheets[0].viewports[1].id, "vp9");
        // The others follow, numbered one on; the key plan's outline of the
        // first tile follows it by id to its new number.
        assert_eq!(set.sheets[1].id, "s1");
        assert_eq!(mark_label(&set, &set.sheets[1].viewports[0].marks[0]), "3");
        let rows = drawing_register(&set);
        assert_eq!(rows[0].title, "DRAWING REGISTER");
        assert_eq!(rows[0].scale, "N.T.S.");
        assert_eq!(rows[1].number, "2");
        assert_eq!(rows[1].title, "KEY PLAN");

        // A second is refused and changes nothing.
        let again = add_register_sheet(&mut document, &SheetTemplate::default())
            .expect_err("a second register");
        assert_eq!(again.code, ErrorCode::AlreadyExists);
        assert_eq!(document.history().undo_count(), steps + 1);

        // One undo takes it away, exactly.
        ok(document.undo());
        assert_eq!(*document.sheet_set(), before);
    }

    #[test]
    fn the_register_can_start_an_empty_set() {
        let mut document = Document::default();
        let id = ok(add_register_sheet(&mut document, &SheetTemplate::default()));
        assert_eq!(id, "s1");
        assert_eq!(document.sheet_set().sheets.len(), 1);
        let rows = drawing_register(&document.sheet_set());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].number, "1");
    }

    #[test]
    fn blanks_at_the_ends_of_a_cell_are_dropped() {
        // A description typed with a trailing line break is one line, not two,
        // and a code with a trailing space is centred on the code.
        let spec = TableSpec {
            columns: vec![
                column("REV", HorizontalJustify::Centre, false, false),
                column("DESCRIPTION", HorizontalJustify::Left, true, true),
            ],
            rows: vec![row(&["B ", "FIRST ISSUE\n"]), row(&["A", "  \r\n"])],
            ..TableSpec::default()
        };
        let table = layout(&spec, rect(0.0, 0.0, 80.0, 60.0));
        assert_eq!(table.row_boxes.len(), 2);
        double_eq(table.row_boxes[0].height(), 2.0 * table.cap_mm);
        double_eq(table.row_boxes[1].height(), 2.0 * table.cap_mm);
        assert!(find_text(&table, "B").is_some());
        assert!(find_text(&table, "FIRST ISSUE").is_some());
        for item in &table.texts {
            assert!(!item.text.is_empty());
            assert!(!item.text.ends_with([' ', '\r', '\n']), "{}", item.text);
        }
    }

    #[test]
    fn a_table_too_narrow_for_one_block_is_squeezed_into_it_at_the_smallest_text() {
        // 12 mm cannot hold the register's columns even at 1.8 mm: one block,
        // every column in proportion, every text squeezed inside the box.
        let set = four_sheets();
        let view = Viewport {
            rect: rect(10.0, 10.0, 22.0, 100.0),
            ..viewport_of(ViewportKind::SheetIndex, 500.0, 1.0)
        };
        let table = layout_viewport_table(&set, 0, &view, None);
        double_eq(table.cap_mm, 1.8);
        assert_eq!(table.blocks, 1);
        assert_eq!(table.rows_shown, 4);
        for item in &table.texts {
            assert!(item.cap_mm >= 1.8, "{}", item.text);
        }
        expect_inside(&table, &view.rect, &estimate_text_width);
    }

    #[test]
    fn a_single_word_wider_than_its_column_has_a_line_of_its_own_squeezed() {
        let word = "W".repeat(200);
        let spec = TableSpec {
            columns: vec![
                column("REV", HorizontalJustify::Centre, false, false),
                column("DESCRIPTION", HorizontalJustify::Left, true, true),
            ],
            rows: vec![vec![String::from("A"), format!("SEE {word} BELOW")]],
            ..TableSpec::default()
        };
        let area = rect(0.0, 0.0, 60.0, 40.0);
        let table = layout(&spec, area);
        assert_eq!(table.rows_shown, 1);
        let own = find_text(&table, &word).expect("the long word");
        assert!(own.squeeze < 1.0);
        assert!(find_text(&table, "SEE").is_some());
        assert!(find_text(&table, "BELOW").is_some());
        double_eq(table.row_boxes[0].height(), 5.0 * table.cap_mm);
        expect_inside(&table, &area, &monospace);
    }
}
