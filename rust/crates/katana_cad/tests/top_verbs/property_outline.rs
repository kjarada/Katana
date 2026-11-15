// The property outline (property_outline.rs): flat '/' names read as a tree one
// level at a time, several entities read as one, and PROP TREE's reply
// (tests/cad/test_property_outline.cpp). Every expectation is worked out by
// hand from the maps the tests build.

use katana_cad::property_outline::{
    has_properties_under, natural_less, property_outline, property_outline_of,
};
use katana_cad::{CommandInterpreter, Document, PropertyOutlineEntry};
use katana_commands::{EntityAttributes, create_point, set_entity_property};
use katana_core::ErrorCode;
use katana_entity::{EntityId, PropertyMap, PropertyValue};
use katana_geometry::Point2;

use crate::{create, err, must};

fn map_of(entries: &[(&str, PropertyValue)]) -> PropertyMap {
    entries
        .iter()
        .map(|(key, value)| (String::from(*key), value.clone()))
        .collect()
}

// A surveyed string as an archive import leaves it: a group two deep, a value
// of its own, and per-vertex attributes on vertices 1, 2 and 10.
fn surveyed_string() -> PropertyMap {
    map_of(&[
        ("Asset/Dimensions/Size", PropertyValue::Integer(300)),
        ("Asset/Dimensions/Depth", PropertyValue::Real(1.5)),
        ("Asset/Owner", PropertyValue::from("Council")),
        ("Name", PropertyValue::from("KERB")),
        ("vertex/1/QL", PropertyValue::from("B")),
        ("vertex/1/Depth", PropertyValue::Real(0.6)),
        ("vertex/2/QL", PropertyValue::from("A")),
        ("vertex/10/QL", PropertyValue::from("C")),
    ])
}

fn names(entries: &[PropertyOutlineEntry]) -> Vec<String> {
    entries.iter().map(|entry| entry.name.clone()).collect()
}

// ---- the order --------------------------------------------------------------------------

mod property_outline {
    use super::*;

    #[test]
    fn natural_order_puts_vertex_two_before_vertex_ten() {
        assert!(natural_less("vertex/2", "vertex/10"));
        assert!(!natural_less("vertex/10", "vertex/2"));
        assert!(natural_less("9", "10"));
        // A run longer than any integer type still compares as a number.
        assert!(natural_less(
            "vertex/99",
            "vertex/123456789012345678901234567890"
        ));
        // Letters without regard to case: "asset" before "Name" before "vertex",
        // where the map's byte order puts every capital first.
        assert!(natural_less("asset", "Name"));
        assert!(natural_less("Name", "vertex"));
    }

    #[test]
    fn natural_order_is_total_where_case_or_leading_zeros_alone_differ() {
        // Equal when folded, so the plain byte order decides: 'N' (78) before
        // 'n' (110), and "007" before "7" ('0' before '7').
        assert!(natural_less("Name", "name"));
        assert!(!natural_less("name", "Name"));
        assert!(natural_less("007", "7"));
        assert!(!natural_less("7", "007"));
        // Irreflexive, and a prefix comes first.
        assert!(!natural_less("QL", "QL"));
        assert!(natural_less("QL", "QL2"));
        assert!(!natural_less("", ""));
        assert!(natural_less("", "a"));
    }

    // ---- one map ----------------------------------------------------------------------------

    #[test]
    fn the_top_of_an_archive_string_is_its_groups_its_values_and_its_vertices() {
        let top = property_outline(&surveyed_string(), "");
        assert_eq!(names(&top), ["Asset", "Name", "vertex"]);

        // Asset: Dimensions and Owner beneath it, three values in all.
        assert_eq!(top[0].path, "Asset");
        assert_eq!(top[0].holders, 0);
        assert!(top[0].value.is_none());
        assert!(!top[0].varies);
        assert_eq!(top[0].children, 2);
        assert_eq!(top[0].values, 3);

        assert_eq!(top[1].path, "Name");
        assert_eq!(top[1].holders, 1);
        assert_eq!(top[1].value, Some(PropertyValue::from("KERB")));
        assert_eq!(top[1].children, 0);

        // vertex: vertices 1, 2 and 10, four values.
        assert_eq!(top[2].children, 3);
        assert_eq!(top[2].values, 4);
    }

    #[test]
    fn vertices_are_listed_in_their_numbers_order() {
        let vertices = property_outline(&surveyed_string(), "vertex");
        assert_eq!(names(&vertices), ["1", "2", "10"]);
        assert_eq!(vertices[0].path, "vertex/1");
        assert_eq!(vertices[0].children, 2);
        assert_eq!(vertices[2].path, "vertex/10");
        assert_eq!(vertices[2].children, 1);

        let first = property_outline(&surveyed_string(), "vertex/1");
        assert_eq!(names(&first), ["Depth", "QL"]);
        assert_eq!(first[0].value, Some(PropertyValue::Real(0.6)));
        assert_eq!(first[1].path, "vertex/1/QL");
    }

    #[test]
    fn a_level_two_deep_holds_its_values_with_their_types() {
        let dimensions = property_outline(&surveyed_string(), "Asset/Dimensions");
        assert_eq!(names(&dimensions), ["Depth", "Size"]);
        assert_eq!(dimensions[0].value, Some(PropertyValue::Real(1.5)));
        assert_eq!(dimensions[1].value, Some(PropertyValue::Integer(300)));
        assert_eq!(dimensions[1].path, "Asset/Dimensions/Size");
    }

    #[test]
    fn a_name_that_is_a_value_and_a_branch_is_both() {
        let pipe = map_of(&[
            ("Pipe", PropertyValue::from("DN300")),
            ("Pipe/Material", PropertyValue::from("PVC")),
        ]);
        let top = property_outline(&pipe, "");
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].value, Some(PropertyValue::from("DN300")));
        assert_eq!(top[0].children, 1);
        assert_eq!(top[0].values, 1);
    }

    #[test]
    fn a_path_nothing_is_beneath_and_an_empty_map_give_no_entries() {
        assert!(property_outline(&surveyed_string(), "Nothing").is_empty());
        // "Name" holds a value but nothing is beneath it.
        assert!(property_outline(&surveyed_string(), "Name").is_empty());
        // "vert" is the start of a name, not a part of one.
        assert!(property_outline(&surveyed_string(), "vert").is_empty());
        assert!(property_outline(&PropertyMap::new(), "").is_empty());
        assert!(property_outline_of(&[], "").is_empty());
    }

    #[test]
    fn an_empty_part_of_a_name_is_an_entry_of_its_own_rather_than_lost() {
        let doubled = map_of(&[("a//b", PropertyValue::Boolean(true))]);
        let under = property_outline(&doubled, "a");
        assert_eq!(under.len(), 1);
        assert_eq!(under[0].name, "");
        assert_eq!(under[0].path, "a/");
        assert_eq!(under[0].children, 1);
    }

    #[test]
    fn whether_anything_is_under_a_path_needs_no_level_read() {
        let string = surveyed_string();
        assert!(has_properties_under(&string, "vertex"));
        assert!(has_properties_under(&string, "vertex/10"));
        assert!(has_properties_under(&string, "Name"));
        assert!(!has_properties_under(&string, "vertex/3"));
        assert!(!has_properties_under(&string, "vert"));
        assert!(has_properties_under(&string, ""));
        assert!(!has_properties_under(&PropertyMap::new(), ""));
    }

    // ---- several maps -----------------------------------------------------------------------

    #[test]
    fn several_entities_show_a_value_only_where_every_one_holds_the_same() {
        let a = map_of(&[
            ("code", PropertyValue::from("TREE")),
            ("height", PropertyValue::Real(1.0)),
            ("only", PropertyValue::Integer(1)),
            ("g/x", PropertyValue::Boolean(true)),
        ]);
        let b = map_of(&[
            ("code", PropertyValue::from("TREE")),
            ("height", PropertyValue::Real(2.0)),
            ("g/x", PropertyValue::Boolean(true)),
            ("g/y", PropertyValue::Boolean(false)),
        ]);
        let top = property_outline_of(&[&a, &b], "");
        assert_eq!(names(&top), ["code", "g", "height", "only"]);

        assert_eq!(top[0].value, Some(PropertyValue::from("TREE")));
        assert_eq!(top[0].holders, 2);
        assert!(!top[0].varies);

        // g: a branch, x in both and y in one; three values counted per entity.
        assert!(!top[1].varies);
        assert_eq!(top[1].holders, 0);
        assert_eq!(top[1].children, 2);
        assert_eq!(top[1].values, 3);

        // Held differently.
        assert!(top[2].varies);
        assert_eq!(top[2].holders, 2);
        assert!(top[2].value.is_none());

        // Held by one of the two: as much a disagreement.
        assert!(top[3].varies);
        assert_eq!(top[3].holders, 1);
        assert!(top[3].value.is_none());
    }
}

// ---- PROP TREE --------------------------------------------------------------------------

// Two points: the surveyed string's properties on the first, and code=TREE
// with a height on both.
struct PropTreeTest {
    document: Document,
    interpreter: CommandInterpreter,
    first: EntityId,
}

impl PropTreeTest {
    fn new() -> Self {
        let mut document = Document::default();
        let first = create(
            &mut document,
            create_point(Point2::new(0.0, 0.0), EntityAttributes::default()),
        );
        let second = create(
            &mut document,
            create_point(Point2::new(10.0, 0.0), EntityAttributes::default()),
        );
        for (key, value) in surveyed_string() {
            must(&mut document, set_entity_property(vec![first], key, value));
        }
        must(
            &mut document,
            set_entity_property(vec![first], "height", PropertyValue::Real(1.0)),
        );
        must(
            &mut document,
            set_entity_property(vec![second], "height", PropertyValue::Real(2.0)),
        );
        Self {
            document,
            interpreter: CommandInterpreter::new(),
            first,
        }
    }

    fn run(&mut self, line: &str) -> katana_core::Result<String> {
        self.interpreter.run(&mut self.document, line)
    }

    #[track_caller]
    fn reply(&mut self, line: &str) -> String {
        match self.run(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line}: {}", error.describe()),
        }
    }

    // SELECT <first>, which must be taken.
    #[track_caller]
    fn select_first(&mut self) {
        let line = format!("SELECT {}", self.first);
        self.reply(&line);
    }
}

mod prop_tree_test {
    use super::*;

    #[test]
    fn the_selections_top_level_is_a_record_for_the_level_then_one_an_entry() {
        let mut t = PropTreeTest::new();
        t.select_first();
        assert_eq!(
            t.reply("PROP TREE"),
            "scope=selection matched=1 under=\"\" entries=4 from=0 shown=4\n\
             path=Asset children=2 values=3\n\
             path=height value=1 type=real\n\
             path=Name value=KERB type=text\n\
             path=vertex children=3 values=4"
        );
    }

    #[test]
    fn under_names_the_level_and_from_and_limit_page_through_it() {
        let mut t = PropTreeTest::new();
        t.select_first();
        assert_eq!(
            t.reply("PROP TREE UNDER vertex FROM 1 LIMIT 1"),
            "scope=selection matched=1 under=vertex entries=3 from=1 shown=1\n\
             path=vertex/2 children=1 values=1"
        );
        // A trailing '/' names the same branch.
        assert_eq!(
            t.reply("prop tree under vertex/10/"),
            "scope=selection matched=1 under=vertex/10 entries=1 from=0 shown=1\n\
             path=vertex/10/QL value=C type=text"
        );
        // Past the end is an empty page, not a refusal.
        assert_eq!(
            t.reply("PROP TREE UNDER vertex FROM 7"),
            "scope=selection matched=1 under=vertex entries=3 from=3 shown=0"
        );
    }

    #[test]
    fn a_drawing_scope_reads_every_entity_and_says_where_they_differ() {
        let mut t = PropTreeTest::new();
        // Nothing selected: the scope is what is read, not the selection.
        let text = t.reply("PROP TREE DRAWING");
        assert!(
            text.contains("scope=drawing matched=2 under=\"\" entries=4"),
            "{text}"
        );
        assert!(
            text.contains("\npath=height varies=yes holders=2"),
            "{text}"
        );
        // Name is only the first point's: it varies, held by one of two.
        assert!(text.contains("\npath=Name varies=yes holders=1"), "{text}");

        let filtered = t.reply("PROP TREE DRAWING WHERE PROP=Name:KERB");
        assert!(filtered.contains("matched=1"), "{filtered}");
        assert!(
            filtered.contains("\npath=Name value=KERB type=text"),
            "{filtered}"
        );
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_not_refused() {
        let mut t = PropTreeTest::new();
        assert_eq!(
            t.reply("PROP TREE"),
            "scope=selection matched=0 under=\"\" entries=0 from=0 shown=0"
        );
    }

    #[test]
    fn words_it_cannot_read_are_refused_naming_the_word() {
        let mut t = PropTreeTest::new();
        for line in [
            "PROP TREE LIMIT 0",
            "PROP TREE FROM -1",
            "PROP TREE LIMIT many",
            "PROP TREE UNDER",
            "PROP TREE BOGUS",
        ] {
            let refusal = err(t.run(line));
            assert_eq!(refusal.code, ErrorCode::ParseFailure, "{line}");
        }
        // Two scope words are the shared parser's refusal, in its own terms.
        assert!(t.run("PROP TREE DRAWING SELECTION").is_err());
        let refusal = err(t.run("PROP TREE BOGUS"));
        assert!(
            refusal.describe().contains("BOGUS"),
            "{}",
            refusal.describe()
        );
    }
}
