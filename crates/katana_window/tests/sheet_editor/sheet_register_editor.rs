//! The twins of tests/qt_widgets/plotting/test_sheet_register_editor.cpp: the drawing register and
//! the revision table in the sheet editor - Generate Sheets' cover, Add View's two tables, and the
//! newest-revisions setting, each one step.

mod sheet_register_editor {
    use katana_cad::Document;
    use katana_cad::plotting::generators::SheetTemplate;
    use katana_cad::plotting::sheet_commands::add_sheet;
    use katana_cad::plotting::sheet_set::{Sheet, SheetSet, ViewportKind, drawing_area};
    use katana_cad::plotting::tables::add_register_sheet;

    use crate::support::Shown;

    fn add_named_sheet(document: &mut Document, name: &str) {
        let sheet = Sheet {
            name: String::from(name),
            ..Sheet::default()
        };
        add_sheet(document, sheet, None).expect("the sheet is added");
    }

    /// Generate Sheets answered with the drawing register: whether its Replace box was offered,
    /// then OK.
    fn generate_register(shown: &mut Shown) -> bool {
        let _ = shown.editor.generate_sheets(&shown.host);
        shown.draw();
        shown.fill_in(
            "sheetGenerateDialog",
            "sheetGenerateLayout",
            "Drawing register (cover sheet)",
        );
        // The register goes in front of the sheets there are: "replace" makes no sense for it and
        // is turned off.
        let replace_enabled = shown
            .control("sheetGenerateReplace")
            .expect("the Replace box")
            .enabled;
        shown.press_in("sheetGenerateDialog", "sheetGenerateOk");
        replace_enabled
    }

    #[test]
    fn generate_sheets_puts_the_register_cover_first_in_one_step() {
        let mut document = Document::new(None);
        add_named_sheet(&mut document, "PLAN 1");
        add_named_sheet(&mut document, "PLAN 2");
        let before = SheetSet::clone(&document.sheet_set());
        let mut shown = Shown::new(document);
        shown.editor.set_current_sheet(&shown.host, 1);
        assert!(!generate_register(&mut shown));
        let set = shown.sheet_set();
        assert_eq!(set.sheets.len(), 3);
        assert_eq!(set.sheets[0].name, "DRAWING REGISTER");
        assert_eq!(set.sheets[0].viewports[0].kind, ViewportKind::SheetIndex);
        assert_eq!(set.sheets[1].name, "PLAN 1");
        // The editor shows the new cover.
        assert_eq!(shown.editor.current_sheet(), 0);
        let logged = shown.host.logged().to_vec();
        assert!(!logged.is_empty());
        assert!(!logged.last().expect("a message").1);

        // A second register is refused, said so, and changes nothing.
        let _ = generate_register(&mut shown);
        assert_eq!(shown.sheet_set().sheets.len(), 3);
        let last = shown.host.logged().last().cloned().expect("a message");
        assert!(last.1);
        assert!(last.0.contains("already has a drawing register"), "{}", last.0);

        shown.undo();
        assert_eq!(shown.sheet_set(), before);
    }

    #[test]
    fn add_view_offers_the_register_and_the_revision_table_each_in_one_step() {
        let mut document = Document::new(None);
        add_named_sheet(&mut document, "COVER");
        let mut shown = Shown::new(document);
        let add_register = shown.action("sheetAddView_sheet_index");
        let add_revisions = shown.action("sheetAddView_revisions");
        assert_eq!(add_register.text, "Drawing register");
        assert_eq!(add_revisions.text, "Revision table");

        let steps = shown.steps();
        shown.trigger("sheetAddView_sheet_index");
        shown.trigger("sheetAddView_revisions");
        assert_eq!(shown.steps(), steps + 2);
        let set = shown.sheet_set();
        let sheet = &set.sheets[0];
        assert_eq!(sheet.viewports.len(), 2);
        assert_eq!(sheet.viewports[0].kind, ViewportKind::SheetIndex);
        assert_eq!(sheet.viewports[1].kind, ViewportKind::Revisions);
        // Placed on the paper, inside the drawing area, not over each other.
        let area = drawing_area(sheet);
        for view in &sheet.viewports {
            assert!(!view.rect.empty());
            assert!(area.inflated(1e-9).contains_box(view.rect));
        }
        // The canvas paints both without a complaint.
        shown.paint();
        assert!(shown.canvas().last_stats().problems.is_empty());
        assert_eq!(shown.canvas().last_stats().viewports_drawn, 2);
    }

    #[test]
    fn the_newest_revisions_setting_is_one_step() {
        let mut document = Document::new(None);
        add_register_sheet(&mut document, &SheetTemplate::default()).expect("a register");
        let mut shown = Shown::new(document);
        let id = shown.sheet_set().sheets[0].viewports[1].id.clone();
        shown.canvas_mut().select(&id);
        shown.draw();
        let limit = shown.control("sheetRevisionLimit").expect("the limit");
        // 0, which the box shows as All.
        assert_eq!(limit.value, "All");

        let steps = shown.steps();
        shown.fill("sheetRevisionLimit", "3");
        assert_eq!(shown.sheet_set().sheets[0].viewports[1].revision_limit, 3);
        assert_eq!(shown.steps(), steps + 1);
        // The panel is rebuilt showing the stored value.
        assert_eq!(
            shown.control("sheetRevisionLimit").expect("the limit").value,
            "3"
        );

        shown.undo();
        assert_eq!(shown.sheet_set().sheets[0].viewports[1].revision_limit, 0);
    }
}
