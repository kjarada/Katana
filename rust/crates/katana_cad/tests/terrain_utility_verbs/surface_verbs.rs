// The drawing's surfaces and display styles on the command line
// (terrain/surface_verbs.rs; tests/cad/terrain/test_surface_verbs.cpp):
// SURFACE RENAME, STYLE, SHOW, HIDE, COPY and SURFACESTYLE, through the
// interpreter every front end runs - each change one undo step, each refusal
// leaving the drawing as it was.

mod drawing_surface_verbs {
    use std::path::Path;
    use std::sync::Arc;

    use katana_cad::terrain::surface_style::{built_in_surface_styles, surface_styles_to_json};
    use katana_cad::terrain::surface_verbs::{
        import_surfaces, is_surface_verb, set_surface_style_option, surface_style_options,
    };
    use katana_cad::{CommandInterpreter, Document, SurfaceDisplayStyle, SurfaceFaces};
    use katana_commands::{EntityAttributes, create_point};
    use katana_core::ErrorCode;
    use katana_entity::{Color, ColourTable};
    use katana_geometry::Point2;
    use katana_storage::{OpenMode, SqliteDatabase};
    use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

    use crate::surface_test_support::{
        ScratchDirectory, flat_square, tilted_square, with, with_details,
    };
    use crate::{Session, contains, must, ok, strings};

    // Two surfaces, as SURFACE FROM would have made them.
    fn fixture() -> Session {
        let store = with_details(
            SurfaceStore::default(),
            "ground",
            tilted_square(100.0, 1.0, 10.0, 0.0, 0.0),
            "",
            true,
            "raster dem.tif",
        );
        let store = with_details(
            store,
            "design",
            flat_square(99.0),
            "",
            true,
            "drawing (4 entities)",
        );
        let mut session = Session::default();
        ok(session.document.set_surfaces(store, "SURFACE FROM"));
        session
    }

    // `line` must be refused with `code` and change nothing.
    #[track_caller]
    fn refused(session: &mut Session, line: &str, code: ErrorCode) {
        let steps = session.document.history().undo_count();
        let surfaces = session.document.surfaces().all().clone();
        let styles = session.document.surface_styles().clone();
        let error = session.refused(line);
        assert_eq!(error.code, code, "{line}\n{}", error.describe());
        assert_eq!(session.document.history().undo_count(), steps, "{line}");
        assert!(*session.document.surfaces().all() == surfaces, "{line}");
        assert!(*session.document.surface_styles() == styles, "{line}");
    }

    // Whether two surfaces hold the one triangulation, as the C++ compares
    // its shared pointers.
    fn same_triangulation(
        first: &Option<Arc<TinSurface>>,
        second: &Option<Arc<TinSurface>>,
    ) -> bool {
        match (first, second) {
            (Some(first), Some(second)) => Arc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        }
    }

    #[track_caller]
    fn named<'a>(document: &'a Document, name: &str) -> &'a NamedSurface {
        match document.surfaces().find(name) {
            Some(surface) => surface,
            None => panic!("no surface {name}"),
        }
    }

    #[test]
    fn rename_keeps_the_surface_and_is_one_undo_step() {
        let mut t = fixture();
        let ground = named(&t.document, "ground").surface.clone();
        let reply = t.ok("SURFACE RENAME GROUND \"Existing Ground\"");
        assert!(
            contains(&reply, "renamed surface=ground name=\"Existing Ground\""),
            "{reply}"
        );
        assert!(
            contains(
                &reply,
                "surface name=\"Existing Ground\" triangles=2 points=4"
            ),
            "{reply}"
        );
        assert!(t.document.surfaces().find("existing ground").is_some());
        assert!(same_triangulation(
            &named(&t.document, "existing ground").surface,
            &ground
        ));
        assert_eq!(t.document.history().undo_name(), "SURFACE RENAME");
        ok(t.document.undo());
        assert!(t.document.surfaces().find("ground").is_some());
        assert!(t.document.surfaces().find("Existing Ground").is_none());
    }

    #[test]
    fn a_level_of_sixty_one_digits_is_printed_in_full_and_not_as_nan() {
        // An edit refuses a level past 1e9, but a project, an archive or a
        // LandXML file may hold one: the record that names the surface said
        // zmin=nan for a FINITE level whose digits and decimals filled more
        // than the 64 characters its formatter had. 1e61 is the double
        // 9999999999999999493871352970740188669636450110134100730839040
        // (Python's '%.3f' % 1e61): 61 digits, so 65 characters with the
        // decimals.
        let mut t = fixture();
        let store = with(t.document.surfaces().clone(), "far", flat_square(1e61), "");
        ok(t.document.set_surfaces(store, "TEST"));
        let reply = t.ok("SURFACE COPY far twin");
        let level = "9999999999999999493871352970740188669636450110134100730839040.000";
        assert!(
            contains(&reply, &format!(" zmin={level} zmax={level} ")),
            "{reply}"
        );
        assert!(!contains(&reply, "nan"), "{reply}");
    }

    #[test]
    fn a_rename_the_drawing_cannot_take_is_refused() {
        let mut t = fixture();
        refused(&mut t, "SURFACE RENAME nothing other", ErrorCode::NotFound);
        refused(
            &mut t,
            "SURFACE RENAME ground DESIGN",
            ErrorCode::AlreadyExists,
        );
        refused(&mut t, "SURFACE RENAME ground", ErrorCode::InvalidArgument);
        refused(
            &mut t,
            "SURFACE RENAME ground \"\"",
            ErrorCode::InvalidArgument,
        );
    }

    #[test]
    fn a_style_is_given_by_its_name_in_any_case_and_empty_is_the_default() {
        let mut t = fixture();
        let reply = t.ok("SURFACE STYLE ground \"cut fill\"");
        assert_eq!(
            named(&t.document, "ground").style,
            "Cut Fill",
            "as the style spells it"
        );
        assert!(
            contains(&reply, "style=\"Cut Fill\" visible=yes"),
            "{reply}"
        );
        let reply = t.ok("SURFACE STYLE ground \"\"");
        assert_eq!(named(&t.document, "ground").style, "");
        assert!(
            contains(&reply, "style=Standard"),
            "drawn in the default: {reply}"
        );
        refused(
            &mut t,
            "SURFACE STYLE ground \"No Such Style\"",
            ErrorCode::NotFound,
        );
        refused(
            &mut t,
            "SURFACE STYLE nothing Contours",
            ErrorCode::NotFound,
        );
    }

    #[test]
    fn hide_and_show_change_only_what_the_views_draw_in_one_step() {
        let mut t = fixture();
        let steps = t.document.history().undo_count();
        let reply = t.ok("SURFACE HIDE ALL");
        assert!(contains(&reply, "hidden surfaces=2 changed=2"), "{reply}");
        assert_eq!(t.document.history().undo_count(), steps + 1);
        assert!(!named(&t.document, "ground").visible);
        assert!(!named(&t.document, "design").visible);
        // Asked again, nothing changes and no step is made.
        let reply = t.ok("SURFACE HIDE ALL");
        assert!(contains(&reply, "hidden surfaces=2 changed=0"), "{reply}");
        assert_eq!(t.document.history().undo_count(), steps + 1);
        let reply = t.ok("surface show Ground");
        assert!(contains(&reply, "surface name=ground"), "{reply}");
        assert!(contains(&reply, "visible=yes"), "{reply}");
        assert!(named(&t.document, "ground").visible);
        assert!(!named(&t.document, "design").visible);
        // One unknown name refuses the whole line.
        refused(&mut t, "SURFACE SHOW design nothing", ErrorCode::NotFound);
        refused(&mut t, "SURFACE SHOW", ErrorCode::InvalidArgument);
    }

    #[test]
    fn show_all_of_no_surfaces_is_not_an_error() {
        let mut t = fixture();
        t.document.new_document();
        let reply = t.ok("SURFACE SHOW ALL");
        assert!(contains(&reply, "shown surfaces=0 changed=0"), "{reply}");
        assert!(!t.document.history().can_undo());
    }

    #[test]
    fn a_copy_shares_the_triangulation_and_keeps_the_style() {
        let mut t = fixture();
        t.ok("SURFACE STYLE ground Contours");
        let reply = t.ok("SURFACE COPY ground \"ground before\"");
        assert!(
            contains(&reply, "copied surface=ground name=\"ground before\""),
            "{reply}"
        );
        assert!(t.document.surfaces().find("ground before").is_some());
        let copy = named(&t.document, "ground before");
        assert!(same_triangulation(
            &copy.surface,
            &named(&t.document, "ground").surface
        ));
        assert_eq!(copy.style, "Contours");
        assert_eq!(copy.source, "copy of ground");
        assert_eq!(t.document.history().undo_name(), "SURFACE COPY");
        refused(
            &mut t,
            "SURFACE COPY ground design",
            ErrorCode::AlreadyExists,
        );
        refused(&mut t, "SURFACE COPY nothing other", ErrorCode::NotFound);
    }

    #[test]
    fn only_the_drawings_own_surface_words_are_claimed() {
        let _t = fixture();
        assert!(is_surface_verb(&strings(&["surfacestyle"])));
        assert!(is_surface_verb(&strings(&["SURFACE", "rename", "a", "b"])));
        for word in [
            "LIST",
            "INFO",
            "REMOVE",
            "FROM",
            "EXPORT",
            "EDIT",
            "MERGE",
            "DIFFERENCE",
            "ENVELOPE",
        ] {
            assert!(!is_surface_verb(&strings(&["SURFACE", word])), "{word}");
        }
        assert!(!is_surface_verb(&strings(&["SURFACE"])));
    }

    #[test]
    fn the_styles_are_listed_and_their_json_is_the_documents_form() {
        let mut t = fixture();
        let reply = t.ok("SURFACESTYLE LIST");
        assert!(
            contains(&reply, "style name=Standard default=yes surfaces=2"),
            "{reply}"
        );
        assert!(
            contains(&reply, "style name=\"Cut Fill\" default=no surfaces=0"),
            "{reply}"
        );
        assert!(
            contains(&reply, "listed styles=7 default=Standard"),
            "{reply}"
        );
        let json = t.ok("SURFACESTYLE LIST JSON");
        assert_eq!(json, surface_styles_to_json(t.document.surface_styles()));
        assert_eq!(
            t.ok("SURFACESTYLE"),
            reply,
            "LIST is what the verb alone says"
        );
        refused(&mut t, "SURFACESTYLE LIST XML", ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_style_is_made_from_another_set_renamed_made_the_default_and_deleted() {
        let mut t = fixture();
        let reply = t.ok("SURFACESTYLE NEW Fine FROM contours interval=0.5 major=4");
        assert!(
            contains(&reply, "created style=Fine from=Contours"),
            "{reply}"
        );
        let Some(fine) = t.document.surface_styles().find("Fine") else {
            panic!("no style Fine");
        };
        assert!(fine.contours.show, "from Contours");
        assert_eq!(fine.contours.interval, 0.5);
        assert_eq!(fine.contours.major_every, 4);
        assert_eq!(t.document.history().undo_name(), "SURFACESTYLE NEW");

        // A colour by its standard name, and stops as value:colour pairs.
        let reply = t
            .ok("SURFACESTYLE SET fine majorcolour=red stops=-1:#0000FF;1:#FF0000 faces=elevation");
        assert!(contains(&reply, "set style=Fine changed=yes"), "{reply}");
        let Some(fine) = t.document.surface_styles().find("Fine") else {
            panic!("no style Fine");
        };
        assert_eq!(fine.contours.major_colour, Color::new(255, 0, 0, 255));
        assert_eq!(fine.stops.len(), 2);
        assert_eq!(fine.stops[0].value, -1.0);
        assert_eq!(fine.plan_faces, SurfaceFaces::Elevation);

        // Renamed with a surface drawn in it: the surface follows, in one
        // step.
        t.ok("SURFACE STYLE ground Fine");
        let steps = t.document.history().undo_count();
        let reply = t.ok("SURFACESTYLE RENAME Fine \"Fine Contours\"");
        assert!(
            contains(
                &reply,
                "renamed style=Fine name=\"Fine Contours\" surfaces=1"
            ),
            "{reply}"
        );
        assert_eq!(t.document.history().undo_count(), steps + 1);
        assert_eq!(named(&t.document, "ground").style, "Fine Contours");

        let reply = t.ok("SURFACESTYLE DEFAULT \"fine contours\"");
        assert!(
            contains(&reply, "default style=\"Fine Contours\""),
            "{reply}"
        );
        refused(
            &mut t,
            "SURFACESTYLE DELETE \"Fine Contours\"",
            ErrorCode::InvalidArgument,
        ); // the default
        t.ok("SURFACESTYLE DEFAULT Standard");
        refused(
            &mut t,
            "SURFACESTYLE DELETE \"Fine Contours\"",
            ErrorCode::InvalidArgument,
        ); // drawn in
        t.ok("SURFACE STYLE ground \"\"");
        let reply = t.ok("SURFACESTYLE DELETE \"Fine Contours\"");
        assert!(
            contains(&reply, "deleted style=\"Fine Contours\" styles=7"),
            "{reply}"
        );
        assert!(t.document.surface_styles().find("Fine Contours").is_none());
    }

    #[test]
    fn a_style_change_that_cannot_be_made_changes_nothing() {
        let mut t = fixture();
        refused(
            &mut t,
            "SURFACESTYLE NEW Standard",
            ErrorCode::AlreadyExists,
        );
        refused(&mut t, "SURFACESTYLE NEW", ErrorCode::InvalidArgument);
        refused(
            &mut t,
            "SURFACESTYLE NEW Fine FROM nothing",
            ErrorCode::NotFound,
        );
        refused(
            &mut t,
            "SURFACESTYLE NEW Fine colour=red",
            ErrorCode::InvalidArgument,
        ); // no such key
        refused(
            &mut t,
            "SURFACESTYLE NEW Fine interval=0",
            ErrorCode::InvalidArgument,
        ); // cannot be drawn
        refused(
            &mut t,
            "SURFACESTYLE SET Standard opacity=half",
            ErrorCode::InvalidArgument,
        );
        refused(
            &mut t,
            "SURFACESTYLE SET Standard opacity=1.5",
            ErrorCode::InvalidArgument,
        );
        refused(
            &mut t,
            "SURFACESTYLE SET Standard boundarycolour=nocolour",
            ErrorCode::InvalidArgument,
        );
        refused(
            &mut t,
            "SURFACESTYLE SET Standard stops=1:#FF0000;0:#0000FF",
            ErrorCode::InvalidArgument,
        );
        refused(
            &mut t,
            "SURFACESTYLE SET Standard ramp=plaid",
            ErrorCode::InvalidArgument,
        );
        refused(
            &mut t,
            "SURFACESTYLE SET Standard faces3d=none",
            ErrorCode::InvalidArgument,
        );
        refused(
            &mut t,
            "SURFACESTYLE SET Standard",
            ErrorCode::InvalidArgument,
        );
        refused(
            &mut t,
            "SURFACESTYLE SET nothing triangles=on",
            ErrorCode::NotFound,
        );
        refused(
            &mut t,
            "SURFACESTYLE RENAME Standard Contours",
            ErrorCode::AlreadyExists,
        );
        refused(&mut t, "SURFACESTYLE DELETE nothing", ErrorCode::NotFound);
        refused(&mut t, "SURFACESTYLE DEFAULT nothing", ErrorCode::NotFound);
        refused(
            &mut t,
            "SURFACESTYLE PAINT Standard",
            ErrorCode::InvalidArgument,
        );
    }

    // The words after "options " in what SURFACESTYLE INFO printed.
    #[track_caller]
    fn options_of(info: &str) -> &str {
        let Some(options) = info.find("\noptions ") else {
            panic!("{info}");
        };
        &info[options + "\noptions ".len()..]
    }

    #[test]
    fn what_info_prints_typed_back_makes_the_same_style() {
        let mut t = fixture();
        let info = t.ok("SURFACESTYLE INFO \"Slope Bands\"");
        let words = options_of(&info);
        t.ok(&format!("SURFACESTYLE NEW Again {words}"));
        let Some(again) = t.document.surface_styles().find("Again") else {
            panic!("no style Again");
        };
        let mut again = again.clone();
        again.name = String::from("Slope Bands");
        let built_in = built_in_surface_styles();
        let Some(slope_bands) = built_in.find("Slope Bands") else {
            panic!("no built-in style Slope Bands");
        };
        assert!(again == *slope_bands);
    }

    #[test]
    fn one_option_word_at_a_time_leaves_the_style_as_it_was_when_refused() {
        let _t = fixture();
        // The C++ default colour table is an empty one.
        let colours = ColourTable::default();
        let mut style = SurfaceDisplayStyle {
            name: String::from("Probe"),
            ..SurfaceDisplayStyle::default()
        };
        let before = style.clone();
        assert!(set_surface_style_option(&mut style, "bands", "many", &colours).is_err());
        assert!(
            set_surface_style_option(&mut style, "stops", "0:#00FF00;x:#FF0000", &colours).is_err()
        );
        assert!(style == before);
        assert!(set_surface_style_option(&mut style, "min", "-2.5", &colours).is_ok());
        assert_eq!(style.range_min, Some(-2.5));
        assert!(set_surface_style_option(&mut style, "min", "", &colours).is_ok());
        assert!(style.range_min.is_none(), "empty clears");
        assert!(set_surface_style_option(&mut style, "stops", "", &colours).is_ok());
        assert!(style.stops.is_empty());
    }

    #[test]
    fn an_importers_surfaces_join_the_drawing_in_the_imports_one_step_under_free_names() {
        let mut t = fixture();
        let steps = t.document.history().undo_count();
        let brought = vec![
            NamedSurface {
                name: String::from("ground"), // taken: becomes "ground (2)"
                surface: Some(flat_square(50.0)),
                source: String::from("IFC site.ifc"),
                ..NamedSurface::default()
            },
            NamedSurface {
                name: String::from("pad"),
                surface: Some(flat_square(51.0)),
                ..NamedSurface::default()
            },
        ];
        let records = must(import_surfaces(
            &mut t.document,
            Some(create_point(
                Point2::new(1.0, 1.0),
                EntityAttributes::default(),
            )),
            brought,
            String::from("IMPORT site.ifc"),
        ));
        assert_eq!(records.len(), 2);
        assert!(
            contains(&records[0], "surface name=\"ground (2)\""),
            "{}",
            records[0]
        );
        assert!(contains(&records[1], "surface name=pad"), "{}", records[1]);
        assert_eq!(
            t.document.history().undo_count(),
            steps + 1,
            "the point and both surfaces"
        );
        assert_eq!(t.document.model().entities.size(), 1);
        ok(t.document.undo());
        assert_eq!(
            t.document.surfaces().all().len(),
            2,
            "the two the fixture made"
        );
        assert_eq!(t.document.model().entities.size(), 0);

        // None brought: the command alone, as it always ran.
        let none = must(import_surfaces(
            &mut t.document,
            Some(create_point(
                Point2::new(2.0, 2.0),
                EntityAttributes::default(),
            )),
            Vec::new(),
            String::from("IMPORT"),
        ));
        assert!(none.is_empty());
        assert_eq!(t.document.model().entities.size(), 1);
    }

    #[test]
    fn the_help_names_the_verbs_and_every_key() {
        let mut t = fixture();
        assert!(contains(
            &CommandInterpreter::help_text(),
            "SURFACE RENAME <name> <new>"
        ));
        assert!(contains(
            &CommandInterpreter::help_text(),
            "SURFACESTYLE LIST [JSON]"
        ));
        let help = t.ok("HELP SURFACESTYLE");
        for (key, _) in surface_style_options(&SurfaceDisplayStyle::default()) {
            assert!(contains(&help, &format!("{key}=")), "{key}");
        }
    }

    // ---- text the verbs refuse, and the reply that says what stands in ----------------------

    #[test]
    fn names_and_descriptions_typed_in_the_ansi_code_page_are_refused_and_nothing_changes() {
        // katana_cli on Windows arrives with 0xB0 for the degree sign and 0xE9
        // for e-acute: not UTF-8, and the project stores a style set as JSON,
        // whose writer threw out of SAVE. The C++ types six lines with those
        // bytes and finds each refused; a line is a &str here, which holds no
        // such byte (docs/rust.md, step 2), so those refusals cannot be typed.
        // The UTF-8 half: nothing bad reached the model, so a save has
        // nothing to fail on; and the same text as UTF-8 is accepted.
        let mut t = fixture();
        let scratch = ScratchDirectory::new("ansi-verbs");
        ok(t.document.save_as(&scratch.path.join("p.katana")));
        // The same text as UTF-8 is accepted: nothing is wrong with the
        // letters.
        t.ok("SURFACESTYLE NEW Pente\u{B0} description=caf\u{E9}");
        let Some(made) = t.document.surface_styles().find("Pente\u{B0}") else {
            panic!("no style Pente\u{B0}");
        };
        assert_eq!(made.description, "caf\u{E9}");
    }

    #[test]
    fn an_imported_name_with_a_quote_is_made_nameable_so_the_surface_can_be_addressed() {
        // A TIN named PAD "A" in an archive, or an IFC string: the tokenizer
        // has no escape inside quotes, so no line could ever name it - listed,
        // never removed, renamed or styled.
        let mut t = fixture();
        let brought = vec![
            NamedSurface {
                name: String::from("PAD \"A\""),
                surface: Some(flat_square(50.0)),
                ..NamedSurface::default()
            },
            NamedSurface {
                name: String::from("PAD \"A\""),
                surface: Some(flat_square(51.0)),
                ..NamedSurface::default()
            },
        ];
        let records = must(import_surfaces(
            &mut t.document,
            None,
            brought,
            String::from("IMPORT"),
        ));
        assert_eq!(records.len(), 2);
        assert!(
            contains(&records[0], "surface name=\"PAD 'A'\""),
            "{}",
            records[0]
        );
        assert!(
            contains(&records[1], "surface name=\"PAD 'A' (2)\""),
            "{}",
            records[1]
        );
        // And a line can now name it.
        assert!(contains(
            &t.ok("SURFACE HIDE \"PAD 'A'\""),
            "hidden surfaces=1 changed=1"
        ));
        t.ok("SURFACE RENAME \"PAD 'A'\" pad");
        assert!(t.document.surfaces().find("pad").is_some());
    }

    #[test]
    fn hide_of_a_surface_named_all_hides_only_that_surface() {
        // The tokenizer drops quotes, so SURFACE HIDE "ALL" - what the
        // Surfaces panel sends for the check box of a surface called ALL -
        // reached the verb as the keyword and hid every surface.
        let mut t = fixture();
        t.ok("SURFACE COPY ground ALL");
        let hidden = t.ok("SURFACE HIDE \"ALL\"");
        assert!(contains(&hidden, "hidden surfaces=1 changed=1"), "{hidden}");
        assert!(named(&t.document, "ground").visible);
        assert!(named(&t.document, "design").visible);
        assert!(!named(&t.document, "ALL").visible);
        let shown = t.ok("SURFACE SHOW all");
        assert!(contains(&shown, "shown surfaces=1 changed=1"), "{shown}");
        assert!(named(&t.document, "ALL").visible);
        // Several names still work, ALL among them as a name.
        t.ok("SURFACE HIDE ground ALL");
        assert!(!named(&t.document, "ground").visible);
        assert!(!named(&t.document, "ALL").visible);
        assert!(named(&t.document, "design").visible);
        // With no surface of that name the keyword means all, as it always
        // did.
        assert!(t.document.surfaces().find("ALL").is_some());
        t.ok("SURFACE RENAME ALL everything");
        assert!(contains(
            &t.ok("SURFACE SHOW ALL"),
            "shown surfaces=3 changed=2"
        ));
    }

    #[test]
    fn a_descriptions_backslashes_survive_what_info_prints_typed_back() {
        // INFO prints the description through record_value, which doubles each
        // backslash; the verb read it plain, so each trip doubled them again.
        // A description is read as a typed text.
        let mut t = fixture();
        t.ok("SURFACESTYLE NEW Dir description=C:\\dir");
        let Some(dir) = t.document.surface_styles().find("Dir") else {
            panic!("no style Dir");
        };
        assert_eq!(dir.description, "C:\\dir", "one backslash");
        for description in ["C:\\dir", "a\\\\nb", "x\\", "plain", ""] {
            let mut styles = t.document.surface_styles().clone();
            let Some(each) = styles.styles.iter_mut().find(|each| each.name == "Dir") else {
                panic!("no style Dir");
            };
            each.description = String::from(description);
            ok(t.document
                .set_surface_styles(styles, Document::SET_SURFACE_STYLES));
            let info = t.ok("SURFACESTYLE INFO Dir");
            t.ok(&format!("SURFACESTYLE NEW Back {}", options_of(&info)));
            let Some(back) = t.document.surface_styles().find("Back") else {
                panic!("no style Back");
            };
            assert_eq!(back.description, description, "{description}");
            t.ok("SURFACESTYLE DELETE Back");
        }
        // A backslash and an n typed on a line is a line break, which a
        // description may not hold; the refusal says what to type instead.
        let broken = t.refused("SURFACESTYLE NEW Line description=a\\nb");
        assert_eq!(broken.code, ErrorCode::InvalidArgument);
        assert!(
            contains(&broken.message, "two backslashes"),
            "{}",
            broken.message
        );
    }

    // A project whose `surface_styles` metadata the test chose, opened in
    // `document`. The C++ names its scratch directory for the test, as the
    // scratch directory's rule asks: ctest runs the two tests that use this at
    // once, and one directory of a fixed name was removed by the second while
    // the first saved into it - on Linux, which unlinks a folder whose files
    // are open (Linux ARM64, run 41581201803); Windows refuses, which hid it.
    // Here the test hands over a directory of its own.
    #[track_caller]
    fn open_with_stored_styles(document: &mut Document, stored: &str, name: &str, scratch: &Path) {
        let project = scratch.join(format!("{name}.katana"));
        {
            let mut made = Document::default();
            ok(made.save_as(&project));
        }
        {
            let raw = must(SqliteDatabase::open(
                &project.join("project.db"),
                OpenMode::ReadWriteCreate,
            ));
            ok(raw.execute(&format!(
                "UPDATE metadata SET value = '{stored}' WHERE key = 'surface_styles'"
            )));
        }
        ok(document.open(&project));
    }

    #[test]
    fn the_json_of_styles_that_only_stand_in_says_so_and_a_healthy_set_does_not() {
        let mut t = fixture();
        assert!(
            !contains(&t.ok("SURFACESTYLE LIST JSON"), "standing_in"),
            "a read set carries no note"
        );

        let scratch = ScratchDirectory::new(
            "stored-styles-the_json_of_styles_that_only_stand_in_says_so_and_a_healthy_set_does_not",
        );
        let mut newer = Session::default();
        open_with_stored_styles(
            &mut newer.document,
            r#"{"format":"katana-surface-styles","version":99,"styles":[]}"#,
            "newer",
            &scratch.path,
        );
        let reply = newer.ok("SURFACESTYLE LIST JSON");
        assert!(
            contains(&reply, "\"standing_in\":{\"code\":\"Unsupported\""),
            "{reply}"
        );
        assert!(contains(&reply, "\"editable\":false"), "{reply}");
        // The set itself is still there to read, in the form a save writes.
        assert!(
            contains(&reply, "\"format\":\"katana-surface-styles\""),
            "{reply}"
        );

        let mut damaged = Session::default();
        open_with_stored_styles(&mut damaged.document, "{not json", "damaged", &scratch.path);
        let reply = damaged.ok("SURFACESTYLE LIST JSON");
        assert!(
            contains(&reply, "\"standing_in\":{\"code\":\"ParseFailure\""),
            "{reply}"
        );
        assert!(contains(&reply, "\"editable\":true"), "{reply}");
        // What a save writes into the project never carries the note.
        assert!(!contains(
            &surface_styles_to_json(damaged.document.surface_styles()),
            "standing_in"
        ));
    }

    #[test]
    fn a_set_that_changes_no_field_on_unreadable_styles_says_it_replaced_them() {
        let _t = fixture();
        let scratch = ScratchDirectory::new(
            "stored-styles-a_set_that_changes_no_field_on_unreadable_styles_says_it_replaced_them",
        );
        let mut damaged = Session::default();
        open_with_stored_styles(
            &mut damaged.document,
            "{not json",
            "damaged-set",
            &scratch.path,
        );
        let steps = damaged.document.history().undo_count();
        // Standard already shows its boundary, so no field of the style moves.
        let reply = damaged.ok("SURFACESTYLE SET Standard boundary=on");
        assert!(
            contains(
                &reply,
                "set style=Standard changed=yes replaced=unreadable-styles"
            ),
            "{reply}"
        );
        assert_eq!(
            damaged.document.history().undo_count(),
            steps + 1,
            "and the step it recorded is real"
        );
        assert!(
            damaged.document.surface_styles_status().is_ok(),
            "the built-ins are now the project's own"
        );
        // A second one, on a set that reads, is changed=no as it always was.
        let reply = damaged.ok("SURFACESTYLE SET Standard boundary=on");
        assert!(
            contains(&reply, "set style=Standard changed=no\n"),
            "{reply}"
        );
        assert!(!contains(&reply, "replaced="), "{reply}");
    }
}
