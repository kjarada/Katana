//! The twins of tests/qt_widgets/plotting/test_section_editor.cpp: a section view's scale in the
//! sheet editor's properties - Auto shown with what it comes to now, chosen as one step, and a
//! fixed scale chosen from Auto keeping the exaggeration Auto chose.

mod sheet_section_editor {
    use katana_cad::Document;
    use katana_cad::plotting::sheet_commands::{add_sheet, edit_viewport};
    use katana_cad::plotting::sheet_set::{Sheet, ViewportKind};
    use katana_commands::create_alignment;
    use katana_entity::Alignment;
    use katana_geometry::{AlignmentPi, HorizontalAlignment, Point2, ProfilePvi, VerticalAlignment};

    use crate::support::{Shown, bx, source_with_document, viewport};

    /// A road east from (10, 50) to (190, 50), its design rising from RL 20 to RL 29: 180 m of
    /// chainage and 9 m of level, with no surface to cut - the long section is its design alone.
    fn road(document: &mut Document) {
        let pi = |x: f64| AlignmentPi {
            point: Point2::new(x, 50.0),
            ..AlignmentPi::default()
        };
        let pvi = |station: f64, elevation: f64| ProfilePvi {
            station,
            elevation,
            curve_length: 0.0,
        };
        let alignment = Alignment {
            name: String::from("ROAD"),
            horizontal: HorizontalAlignment {
                pis: vec![pi(10.0), pi(190.0)],
                ..HorizontalAlignment::default()
            },
            vertical: Some(VerticalAlignment {
                pvis: vec![pvi(0.0, 20.0), pvi(180.0, 29.0)],
            }),
            ..Alignment::default()
        };
        document
            .execute(create_alignment(alignment))
            .expect("the road is added");
    }

    /// A sheet whose one viewport is a long section of the road over the whole A3 drawing area,
    /// at a fixed H 1:500 V 1:50.
    fn sheet_with_long_section(document: &mut Document, automatic: bool) {
        let mut section = viewport("vp1", ViewportKind::LongSection, bx(23.0, 35.0, 410.0, 287.0));
        section.scale = 500.0;
        section.vertical_exaggeration = 10.0;
        section.auto_scale = automatic;
        section.auto_centre = true;
        section.source.alignment = String::from("ROAD");
        let sheet = Sheet {
            id: String::from("s1"),
            name: String::from("LONG SECTION"),
            viewports: vec![section],
            ..Sheet::default()
        };
        add_sheet(document, sheet, None).expect("the sheet is added");
    }

    fn shown(document: Document) -> Shown {
        let mut shown = Shown::bare(document, source_with_document());
        shown.canvas_mut().select("vp1");
        shown.draw();
        shown
    }

    fn only(shown: &Shown) -> katana_cad::plotting::sheet_set::Viewport {
        shown.document().sheet_set().sheets[0].viewports[0].clone()
    }

    #[test]
    fn an_automatic_section_shows_what_it_is_now_and_disables_the_exaggeration() {
        let mut document = Document::new(None);
        road(&mut document);
        sheet_with_long_section(&mut document, true);
        let shown = shown(document);

        let scale = shown.control("sheetViewportScale").expect("the scale");
        assert_eq!(scale.value, "Auto");
        // 180 m across the 363 mm plot needs 1:551, so 1:750; 9 m up its 210 mm then fits ten
        // times (docs/plotting.md's example).
        assert_eq!(
            shown.editor.tooltip("sheetViewportScale"),
            Some("Automatic: now H 1:750 V 1:75")
        );
        let exaggeration = shown
            .control("sheetViewportExaggeration")
            .expect("the exaggeration");
        assert!(!exaggeration.enabled);
    }

    #[test]
    fn choosing_auto_for_a_section_is_one_step() {
        let mut document = Document::new(None);
        road(&mut document);
        sheet_with_long_section(&mut document, false);
        let mut shown = shown(document);

        assert_ne!(
            shown.control("sheetViewportScale").expect("the scale").value,
            "Auto"
        );
        assert!(
            shown
                .control("sheetViewportExaggeration")
                .expect("the exaggeration")
                .enabled
        );

        let before = shown.steps();
        shown.fill("sheetViewportScale", "Auto");
        assert!(only(&shown).auto_scale);
        assert_eq!(shown.steps(), before + 1);
        // The properties were built again, from the edited viewport.
        assert!(
            !shown
                .control("sheetViewportExaggeration")
                .expect("the exaggeration")
                .enabled
        );

        shown.undo();
        assert!(!only(&shown).auto_scale);
    }

    #[test]
    fn a_fixed_scale_chosen_from_auto_keeps_the_exaggeration_auto_chose() {
        // Stored at two times before Auto chose H 1:750 V 1:75: picking 1:750 keeps the ten times
        // it is drawn at, not the two stored.
        let mut document = Document::new(None);
        road(&mut document);
        sheet_with_long_section(&mut document, true);
        edit_viewport(
            &mut document,
            "vp1",
            |v| {
                v.vertical_exaggeration = 2.0;
                Ok(())
            },
            "EDIT_VIEWPORT",
        )
        .expect("edited");
        let mut shown = shown(document);
        assert_eq!(
            shown.editor.tooltip("sheetViewportScale"),
            Some("Automatic: now H 1:750 V 1:75")
        );
        assert!(
            shown
                .control("sheetViewportScale")
                .expect("the scale")
                .items
                .iter()
                .any(|item| item == "1:750")
        );
        shown.fill("sheetViewportScale", "1:750");
        assert!(!only(&shown).auto_scale);
        assert_eq!(only(&shown).scale, 750.0);
        assert_eq!(only(&shown).vertical_exaggeration, 10.0);
    }
}
