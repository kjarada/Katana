// Title-block fields and their automatic values (sheet_set.hpp);
// tests/cad/plotting/test_sheet_fields.cpp.
//
// The expected texts are what a drafter writes on the sheet, worked out from
// the rule each field states; none is copied from the code's output.

use std::collections::BTreeMap;

use katana_cad::PaperSize;
use katana_cad::plotting::frame::built_in_frame;
use katana_cad::plotting::sheet_set::{
    automatic_title, drawing_area, expand_template, format_sheet_number, mark_label,
    resolve_fields, sheet_scale_text,
};
use katana_cad::plotting::{
    FieldContext, Sheet, SheetSet, SignOff, Viewport, ViewportKind, WorldMark, WorldMarkKind,
};

fn viewport_of(kind: ViewportKind, scale: f64, exaggeration: f64) -> Viewport {
    Viewport {
        kind,
        scale,
        vertical_exaggeration: exaggeration,
        ..Viewport::default()
    }
}

fn three_sheets() -> SheetSet {
    let mut set = SheetSet::default();
    for name in ["KEY PLAN", "PLAN 1", "PLAN 2"] {
        let mut sheet = Sheet {
            name: String::from(name),
            ..Sheet::default()
        };
        sheet
            .viewports
            .push(viewport_of(ViewportKind::Plan, 500.0, 1.0));
        set.sheets.push(sheet);
    }
    set
}

fn context() -> FieldContext {
    FieldContext {
        project_name: String::from("Main Road Upgrade"),
        project_description: String::from("Detail survey"),
        coordinate_system: String::from("EPSG:7856"),
        file_name: String::from("main_road.katana"),
        plot_date: String::from("24/09/26"),
    }
}

fn field<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> &'a str {
    fields
        .get(name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn sign_off(name: &str, date: &str) -> SignOff {
    SignOff {
        name: String::from(name),
        date: String::from(date),
    }
}

mod sheet_fields {
    use super::*;

    #[test]
    fn sheet_number_and_count_are_both_automatic() {
        let set = three_sheets();
        let second = resolve_fields(&set, 1, &context());
        assert_eq!(field(&second, "sheet_number"), "2");
        // The count follows the set: the owner's app left it as typed.
        assert_eq!(field(&second, "sheet_count"), "3");
        assert_eq!(field(&second, "sheet_name"), "PLAN 1");
        assert_eq!(field(&second, "paper"), "A3");
        assert!(resolve_fields(&set, 3, &context()).is_empty()); // no fourth sheet
    }

    #[test]
    fn a_numbering_pattern_pads_and_prefixes() {
        let mut set = three_sheets();
        set.numbering = String::from("{set}-{n:02} of {N}");
        set.defaults.set_number = String::from("DS-7");
        assert_eq!(
            field(&resolve_fields(&set, 2, &context()), "sheet_number"),
            "DS-7-03 of 3"
        );
        assert_eq!(format_sheet_number("{n:03}", 7, 12, ""), "007");
        assert_eq!(format_sheet_number("C{n}", 12, 12, ""), "C12");
        // An unknown token is kept as written rather than silently dropped.
        assert_eq!(format_sheet_number("{x}{n}", 1, 1, ""), "{x}1");
    }

    #[test]
    fn each_sign_off_keeps_its_own_date_and_a_missing_one_is_the_plot_date() {
        let mut set = three_sheets();
        set.defaults.surveyor = sign_off("J. CITIZEN", "01/02/26");
        set.defaults.reviewer = sign_off("A. N. OTHER", "");
        let fields = resolve_fields(&set, 0, &context());
        assert_eq!(field(&fields, "surveyor_name"), "J. CITIZEN");
        assert_eq!(field(&fields, "surveyor_date"), "01/02/26");
        assert_eq!(field(&fields, "reviewer_name"), "A. N. OTHER");
        assert_eq!(field(&fields, "reviewer_date"), "24/09/26");
        assert_eq!(field(&fields, "plot_date"), "24/09/26");
    }

    #[test]
    fn project_values_come_from_the_project_unless_the_set_says_otherwise() {
        let mut set = three_sheets();
        let fields = resolve_fields(&set, 0, &context());
        assert_eq!(field(&fields, "project_line_1"), "Main Road Upgrade");
        assert_eq!(field(&fields, "project_line_2"), "Detail survey");
        assert_eq!(field(&fields, "project_line_3"), "");
        assert_eq!(field(&fields, "coordinate_system"), "EPSG:7856");
        assert_eq!(field(&fields, "file_name"), "main_road.katana");
        set.defaults.project_lines = vec![
            String::from("Stage 2"),
            String::new(),
            String::from("Lot 7"),
        ];
        set.defaults.coordinate_system = String::from("GRID 56");
        let fields = resolve_fields(&set, 0, &context());
        assert_eq!(field(&fields, "project_line_1"), "Stage 2");
        assert_eq!(field(&fields, "project_line_2"), "Detail survey"); // empty falls back
        assert_eq!(field(&fields, "project_line_3"), "Lot 7");
        assert_eq!(field(&fields, "coordinate_system"), "GRID 56");
    }

    #[test]
    fn a_value_typed_on_the_sheet_always_wins() {
        let mut set = three_sheets();
        let typed = &mut set.sheets[1].fields;
        typed.insert(String::from("sheet_count"), String::from("12"));
        typed.insert(String::from("sheet_number"), String::from("C-2"));
        typed.insert(String::from("scale"), String::from("1:250 @ A1"));
        let fields = resolve_fields(&set, 1, &context());
        assert_eq!(field(&fields, "sheet_count"), "12");
        assert_eq!(field(&fields, "sheet_number"), "C-2");
        assert_eq!(field(&fields, "scale"), "1:250 @ A1");
        // The other sheets are untouched.
        assert_eq!(
            field(&resolve_fields(&set, 2, &context()), "sheet_count"),
            "3"
        );
    }

    #[test]
    fn the_scale_is_the_main_viewports_and_a_section_states_both() {
        let mut plan = Sheet::default();
        plan.viewports
            .push(viewport_of(ViewportKind::Plan, 500.0, 1.0));
        assert_eq!(sheet_scale_text(&plan), "1:500");
        // A long section at 1:500 exaggerated 10 times: V 1 : 500 / 10 = 50.
        let mut section = Sheet::default();
        section
            .viewports
            .push(viewport_of(ViewportKind::LongSection, 500.0, 10.0));
        assert_eq!(sheet_scale_text(&section), "H 1:500 V 1:50");
        // Equal scales still say both.
        section.viewports[0].vertical_exaggeration = 1.0;
        assert_eq!(sheet_scale_text(&section), "H 1:500 V 1:500");
        // 1:250 at 8 times is 1:31.25, which a scale rule reads as 1:31.
        section.viewports[0] = viewport_of(ViewportKind::CrossSections, 250.0, 8.0);
        assert_eq!(sheet_scale_text(&section), "H 1:250 V 1:31");
        // A plan and a section at different scales: AS SHOWN.
        let mut both = plan.clone();
        both.viewports
            .push(viewport_of(ViewportKind::LongSection, 500.0, 10.0));
        assert_eq!(sheet_scale_text(&both), "AS SHOWN");
        // Two plans at one scale, and a 3D view and legend (not to scale): 1:500.
        let mut two = plan.clone();
        two.viewports
            .push(viewport_of(ViewportKind::Plan, 500.0, 1.0));
        two.viewports
            .push(viewport_of(ViewportKind::Model3D, 123.0, 1.0));
        two.viewports
            .push(viewport_of(ViewportKind::Legend, 1.0, 1.0));
        assert_eq!(sheet_scale_text(&two), "1:500");
        // A key plan inset beside the plan is at its own, smaller scale by
        // design: the sheet is still 1:500, not AS SHOWN.
        let mut inset = plan.clone();
        inset
            .viewports
            .push(viewport_of(ViewportKind::KeyPlan, 5000.0, 1.0));
        assert_eq!(sheet_scale_text(&inset), "1:500");
        // A key-plan sheet, with nothing else drawn to scale, is at the key
        // plan's scale.
        let mut key = Sheet::default();
        key.viewports
            .push(viewport_of(ViewportKind::KeyPlan, 2000.0, 1.0));
        key.viewports
            .push(viewport_of(ViewportKind::Legend, 1.0, 1.0));
        assert_eq!(sheet_scale_text(&key), "1:2000");
        // Nothing drawn to scale.
        let mut legend = Sheet::default();
        legend
            .viewports
            .push(viewport_of(ViewportKind::Legend, 1.0, 1.0));
        assert_eq!(sheet_scale_text(&legend), "N.T.S.");
    }

    #[test]
    fn automatic_titles_name_the_view_and_its_scale() {
        assert_eq!(
            automatic_title(&viewport_of(ViewportKind::Plan, 500.0, 1.0)),
            "PLAN 1:500"
        );
        assert_eq!(
            automatic_title(&viewport_of(ViewportKind::LongSection, 1000.0, 10.0)),
            "LONG SECTION H 1:1000 V 1:100"
        );
        let mut cross = viewport_of(ViewportKind::CrossSections, 200.0, 2.0);
        cross.source.stations = vec![120.0];
        assert_eq!(automatic_title(&cross), "CROSS SECTION CH 120.000");
        assert_eq!(
            automatic_title(&viewport_of(ViewportKind::KeyPlan, 5000.0, 1.0)),
            "KEY PLAN"
        );
    }

    #[test]
    fn frame_texts_fill_from_the_resolved_fields() {
        let frame = built_in_frame()
            .as_ref()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut set = three_sheets();
        set.defaults.set_number = String::from("DS-7");
        let fields = resolve_fields(&set, 0, &context());
        let project = frame.texts_showing("set_number");
        assert_eq!(project.len(), 1);
        assert_eq!(
            expand_template(&frame.texts[project[0]].content, &fields),
            "Main Road Upgrade\nDetail survey\n\n\nDrawing Set Number: DS-7"
        );
        let stamp = frame.texts_showing("paper");
        assert_eq!(stamp.len(), 1);
        assert_eq!(
            expand_template(&frame.texts[stamp[0]].content, &fields),
            "A3 Border version: 2.0    24/09/26"
        );
    }

    #[test]
    fn a_template_leaves_unknown_fields_empty_and_keeps_doubled_braces() {
        let fields = BTreeMap::from([(String::from("a"), String::from("1"))]);
        assert_eq!(expand_template("x{a}y{b}z", &fields), "x1yz");
        assert_eq!(expand_template("{{a}", &fields), "{a}");
        assert_eq!(expand_template("open {a", &fields), "open {a");
    }

    #[test]
    fn marks_name_the_sheet_they_lead_to_by_its_current_number() {
        let mut set = three_sheets();
        set.sheets[0].id = String::from("s1");
        set.sheets[1].id = String::from("s2");
        set.sheets[2].id = String::from("s3");
        let mut matching = WorldMark {
            kind: WorldMarkKind::MatchLine,
            points: Vec::new(),
            label: String::from("MATCH LINE CH 250.000"),
            sheet: String::from("s3"),
        };
        assert_eq!(
            mark_label(&set, &matching),
            "MATCH LINE CH 250.000 - SEE SHEET 3"
        );
        // Reordered, the same mark follows its sheet.
        set.sheets.swap(1, 2);
        assert_eq!(
            mark_label(&set, &matching),
            "MATCH LINE CH 250.000 - SEE SHEET 2"
        );
        let outline = WorldMark {
            kind: WorldMarkKind::SheetOutline,
            points: Vec::new(),
            label: String::new(),
            sheet: String::from("s2"),
        };
        assert_eq!(mark_label(&set, &outline), "3");
        // A mark whose sheet has gone keeps its label alone.
        matching.sheet = String::from("s9");
        assert_eq!(mark_label(&set, &matching), "MATCH LINE CH 250.000");
    }

    #[test]
    fn the_drawing_area_is_the_frames_or_the_paper_less_ten_millimetres() {
        let a3 = Sheet::default();
        let framed = drawing_area(&a3);
        assert_eq!(framed.min.x, 23.0);
        assert_eq!(framed.max.y, 287.0);
        let a1 = Sheet {
            paper: PaperSize::A1,
            ..Sheet::default()
        };
        assert_eq!(drawing_area(&a1).max.x, 820.0); // 410 x 2
        // Portrait A4, no frame: 10..200 x 10..287.
        let portrait = Sheet {
            paper: PaperSize::A4,
            landscape: false,
            ..Sheet::default()
        };
        let bare = drawing_area(&portrait);
        assert_eq!(bare.min.x, 10.0);
        assert_eq!(bare.max.x, 200.0);
        assert_eq!(bare.max.y, 287.0);
    }
}
