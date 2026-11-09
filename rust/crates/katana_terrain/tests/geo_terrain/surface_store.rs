//! tests/geo/test_surface_store.cpp: the named surfaces a session keeps
//! (katana_terrain::surface_store).
//!
//! Two of the C++'s twelve cases are not twinned, being refusals of text that
//! is not UTF-8 which the Rust types cannot carry to the store (docs/rust.md,
//! "Step 2": the model holds validated UTF-8): NamedSurface's source and
//! style are Strings, and rename and set_style take &str, so
//! ASourceOrAStyleThatIsNotUtf8IsRefused and
//! ARenameToANameThatIsNotUtf8IsRefusedAndKeepsTheOldName have no input to
//! give (exemption candidates). The rule they test survives where text
//! still arrives as bytes: check_name, nameable and unique_name, all
//! twinned below.

mod surface_store {
    use std::sync::Arc;

    use katana_core::ErrorCode;
    use katana_core::text_encoding::is_valid_utf8;
    use katana_geometry::Point3;
    use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

    fn triangle() -> Arc<TinSurface> {
        let surface = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 10.0),
                Point3::new(10.0, 0.0, 11.0),
                Point3::new(0.0, 10.0, 12.0),
            ],
            vec![[0, 1, 2]],
            Vec::new(),
        );
        match surface {
            Ok(surface) => Arc::new(surface),
            Err(error) => panic!("{}", error.describe()),
        }
    }

    // The C++ aggregate {name, surface, source}.
    fn surface(name: &str, surface: Option<Arc<TinSurface>>, source: &str) -> NamedSurface {
        NamedSurface {
            name: String::from(name),
            surface,
            source: String::from(source),
            ..NamedSurface::default()
        }
    }

    #[test]
    fn a_surface_is_found_by_its_name_in_any_case() {
        let mut store = SurfaceStore::new();
        assert!(
            store
                .add(surface("Ground", Some(triangle()), "drawing"))
                .is_ok()
        );
        assert!(store.find("ground").is_some());
        let found = store.find("GROUND").expect("found in any case");
        assert_eq!(found.name, "Ground");
        assert_eq!(found.source, "drawing");
        assert!(store.find("design").is_none());
    }

    #[test]
    fn a_name_in_use_is_refused() {
        let mut store = SurfaceStore::new();
        assert!(store.add(surface("ground", Some(triangle()), "")).is_ok());
        let again = store.add(surface("GROUND", Some(triangle()), ""));
        let error = again.expect_err("a name in use is refused");
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        assert_eq!(store.all().len(), 1);
    }

    #[test]
    fn an_empty_name_or_no_surface_is_refused() {
        let mut store = SurfaceStore::new();
        assert_eq!(
            store
                .add(surface("", Some(triangle()), ""))
                .expect_err("an empty name is refused")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            store
                .add(surface("ground", None, ""))
                .expect_err("no surface is refused")
                .code,
            ErrorCode::InvalidArgument
        );
        assert!(store.empty());
    }

    #[test]
    fn every_change_moves_the_revision() {
        let mut store = SurfaceStore::new();
        let start = store.revision();
        assert!(store.add(surface("a", Some(triangle()), "")).is_ok());
        let added = store.revision();
        assert!(added > start);
        assert!(!store.remove("b"));
        assert_eq!(store.revision(), added); // nothing removed, nothing changed
        assert!(store.remove("A"));
        assert!(store.revision() > added);
        let removed = store.revision();
        store.clear();
        assert!(store.revision() > removed);
    }

    #[test]
    fn a_unique_name_counts_from_two() {
        let mut store = SurfaceStore::new();
        assert_eq!(store.unique_name(b"ground"), "ground");
        assert!(store.add(surface("ground", Some(triangle()), "")).is_ok());
        assert_eq!(store.unique_name(b"ground"), "ground (2)");
        assert!(
            store
                .add(surface("ground (2)", Some(triangle()), ""))
                .is_ok()
        );
        assert_eq!(store.unique_name(b"Ground"), "Ground (3)");
        assert_eq!(store.unique_name(b""), "surface");
    }

    // ---- the rule for the text a surface carries --------------------------------------
    //
    // A surface's name, source and style reach project storage and the JSON
    // replies, whose writer throws on a byte that is not UTF-8, and a name a
    // command line cannot carry (a double quote, a line break) is a surface
    // nobody can address again. So the store refuses both where they enter,
    // and makes an importer's name nameable.

    // The C++'s named(name, source = "test", style = {}); the cases twinned
    // give only the name.
    fn named(name: &str) -> NamedSurface {
        NamedSurface {
            name: String::from(name),
            surface: Some(triangle()),
            source: String::from("test"),
            ..NamedSurface::default()
        }
    }

    // "cafe" typed in the ANSI code page: Windows-1252 puts e-acute at 0xE9,
    // which is no UTF-8 (the lead byte of a three-byte sequence with nothing
    // after it).
    const CAFE_IN_ANSI: &[u8] = b"caf\xE9";

    #[test]
    fn a_name_that_is_not_utf8_is_refused_and_the_message_does_not_carry_its_bytes() {
        // A NamedSurface's name is a String, so a name that is not UTF-8
        // cannot reach add in Rust; what add applies to a name first is the
        // store's one rule, check_name, on its bytes - refused here. That
        // the refused add leaves the store empty at revision 0 has no twin:
        // no such add can be made.
        let refused = SurfaceStore::check_name(CAFE_IN_ANSI).expect_err("not UTF-8 is refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        // The point of the check is the bytes: a message holding them would
        // reach the same JSON writers the name was refused for.
        assert!(is_valid_utf8(refused.describe()), "{}", refused.describe());
        assert!(refused.message.contains("UTF-8"), "{}", refused.message);
    }

    #[test]
    fn a_valid_utf8_name_with_letters_beyond_ascii_is_kept() {
        let mut store = SurfaceStore::new();
        // "Grüße 测试" as UTF-8: U+00FC, U+00DF, U+6D4B, U+8BD5.
        let name = "Gr\u{fc}\u{df}e \u{6d4b}\u{8bd5}";
        assert!(store.add(named(name)).is_ok());
        let found = store.find(name).expect("found by its name");
        assert_eq!(found.name, name);
    }

    #[test]
    fn a_name_a_command_line_cannot_carry_is_refused_by_add_and_rename() {
        // The tokenizer has no escape inside quotes, so a double quote cannot
        // be typed in a name and a line break would end the line: such a
        // surface could be listed and never named again.
        let mut store = SurfaceStore::new();
        assert!(store.add(named("PAD \"A\"")).is_err());
        assert!(store.add(named("two\nlines")).is_err());
        assert!(store.add(named("carriage\rreturn")).is_err());
        assert!(store.add(named("")).is_err());
        assert!(store.empty());

        assert!(store.add(named("PAD A")).is_ok());
        assert!(store.rename("PAD A", "PAD \"A\"").is_err());
        assert!(store.rename("PAD A", "").is_err());
        assert!(store.find("PAD A").is_some());

        // A blank, an apostrophe and a backslash can all be typed (in quotes).
        assert!(store.rename("PAD A", "Tom's pad C:\\x").is_ok());
        assert!(store.find("Tom's pad C:\\x").is_some());
    }

    #[test]
    fn check_name_is_the_one_rule_and_names_what_is_wrong() {
        assert!(SurfaceStore::check_name(b"ground").is_ok());
        assert!(SurfaceStore::check_name(b" a b ").is_ok());
        let empty = SurfaceStore::check_name(b"").expect_err("an empty name is refused");
        assert!(empty.message.contains("needs a name"), "{}", empty.message);
        let quote = SurfaceStore::check_name(b"x\"y").expect_err("a quote is refused");
        assert!(quote.message.contains("double quote"), "{}", quote.message);
        let bytes = SurfaceStore::check_name(CAFE_IN_ANSI).expect_err("not UTF-8 is refused");
        assert!(bytes.message.contains("UTF-8"), "{}", bytes.message);
    }

    #[test]
    fn a_name_from_a_file_is_made_nameable_and_unique_by_unique_name() {
        let mut store = SurfaceStore::new();
        // A quote becomes an apostrophe and a line break a blank; text needing
        // neither is unchanged.
        assert_eq!(SurfaceStore::nameable(b"PAD \"A\""), "PAD 'A'");
        assert_eq!(SurfaceStore::nameable(b"two\r\nlines"), "two  lines");
        assert_eq!(SurfaceStore::nameable(b"ground"), "ground");
        // Bytes that are not UTF-8 are read as Windows-1252: 0xE9 is U+00E9,
        // UTF-8 C3 A9.
        assert_eq!(SurfaceStore::nameable(CAFE_IN_ANSI), "caf\u{e9}");

        let first = store.unique_name(b"PAD \"A\"");
        assert_eq!(first, "PAD 'A'");
        assert!(store.add(named(&first)).is_ok());
        let second = store.unique_name(b"PAD \"A\"");
        assert_eq!(second, "PAD 'A' (2)");
        assert!(store.add(named(&second)).is_ok());
        // Every name unique_name returns is one add accepts: an import never
        // fails for a name a file chose.
        assert!(SurfaceStore::check_name(store.unique_name(CAFE_IN_ANSI).as_bytes()).is_ok());
        assert_eq!(store.unique_name(b""), "surface");
    }
}
