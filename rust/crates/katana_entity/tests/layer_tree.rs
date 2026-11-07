//! The twin of tests/entity/test_layer_tree.cpp: nested layers.
//!
//! The properties worth pinning are the ones a tree gets wrong quietly: a
//! "starts with" test that treats "designs/x" as a child of "design", an
//! inheritance rule that stops at the first parent instead of walking to the
//! root, and a rename that half-applies and leaves entities pointing at layers
//! that are no longer there.

use katana_core::error::ErrorCode;
use katana_entity::layer_path::{
    is_layer_under, layer_ancestors, layer_depth, layer_leaf, layer_parent, rewrite_layer_prefix,
    validate_layer_path,
};
use katana_entity::{Layer, LayerDatabase};

fn named(name: &str) -> Layer {
    Layer {
        name: String::from(name),
        ..Layer::default()
    }
}

fn job_tree() -> LayerDatabase {
    let mut layers = LayerDatabase::new();
    for name in [
        "design/surface/tin1",
        "design/surface/tin2",
        "design/road/kerb",
        "asbuilt/road/kerb",
        "asbuilt/services/water",
    ] {
        assert!(layers.add(named(name)).is_ok(), "{name}");
    }
    layers
}

fn position(names: &[String], name: &str) -> usize {
    names
        .iter()
        .position(|candidate| candidate == name)
        .unwrap_or(names.len())
}

// ---- path arithmetic -------------------------------------------------------------

mod layer_path {
    use super::*;

    #[test]
    fn splits_into_leaf_parent_and_ancestors() {
        assert_eq!(layer_leaf("design/surface/tin1"), "tin1");
        assert_eq!(layer_leaf("0"), "0");
        assert_eq!(layer_parent("design/surface/tin1"), "design/surface");
        assert_eq!(layer_parent("0"), "");
        assert_eq!(layer_depth("design/surface/tin1"), 3);
        assert_eq!(layer_depth("0"), 1);

        let ancestors = layer_ancestors("design/surface/tin1");
        assert_eq!(ancestors.len(), 2);
        assert_eq!(ancestors[0], "design");
        assert_eq!(ancestors[1], "design/surface");
        assert!(
            layer_ancestors("0").is_empty(),
            "a layer is not its own ancestor"
        );
    }

    #[test]
    fn descendant_test_compares_whole_segments_not_text_prefixes() {
        // THE bug this exists to prevent: hiding "design" must not hide "designs".
        assert!(is_layer_under("design/surface/tin1", "design"));
        assert!(is_layer_under("design/surface", "design"));
        assert!(
            is_layer_under("design", "design"),
            "a layer is under itself"
        );
        assert!(!is_layer_under("designs/surface", "design"));
        assert!(!is_layer_under("design", "design/surface"));
        assert!(!is_layer_under("asbuilt/road", "design"));
        assert!(
            is_layer_under("anything", ""),
            "everything is under the root"
        );
    }

    // The C++ also refuses "Caf\xE9" and "design/\x80", which a &str cannot
    // hold (audit MOD-12); the UTF-8 e-acute is still accepted.
    #[test]
    fn rejects_names_that_would_break_the_tree_or_a_file_name() {
        assert!(validate_layer_path("0").is_ok());
        assert!(validate_layer_path("design/surface/tin1").is_ok());
        assert!(
            validate_layer_path("As-Built 2024/Road (E)").is_ok(),
            "ordinary names still work"
        );

        assert!(validate_layer_path("").is_err());
        assert!(validate_layer_path("/design").is_err());
        assert!(validate_layer_path("design/").is_err());
        assert!(validate_layer_path("design//surface").is_err());
        assert!(validate_layer_path("design/ /surface").is_err());
        assert!(
            validate_layer_path("design/ surface").is_err(),
            "leading space"
        );
        assert!(
            validate_layer_path("design/surface ").is_err(),
            "trailing space"
        );
        // A layer name reaches a file name whenever something is exported per
        // layer, and ".." there is a path traversal.
        assert!(validate_layer_path("design/../etc").is_err());
        assert!(validate_layer_path(".").is_err());
        assert!(validate_layer_path("a\nb").is_err(), "control characters");
        assert!(
            validate_layer_path("design/Caf\u{e9}").is_ok(),
            "UTF-8 e-acute"
        );

        let mut too_deep = String::from("a");
        for _ in 0..20 {
            too_deep.push_str("/a");
        }
        assert!(validate_layer_path(&too_deep).is_err());
        assert!(validate_layer_path(&"x".repeat(600)).is_err());
    }

    #[test]
    fn rewriting_a_prefix_moves_only_real_descendants() {
        assert_eq!(
            rewrite_layer_prefix("design/surface/tin1", "design", "asbuilt"),
            "asbuilt/surface/tin1"
        );
        assert_eq!(
            rewrite_layer_prefix("design", "design", "asbuilt"),
            "asbuilt"
        );
        assert_eq!(
            rewrite_layer_prefix("designs/x", "design", "asbuilt"),
            "designs/x",
            "a text prefix that is not a segment must be left alone"
        );
    }
}

// ---- the database ----------------------------------------------------------------

mod layer_tree {
    use super::*;

    #[test]
    fn adding_a_nested_layer_creates_its_ancestors() {
        let mut layers = LayerDatabase::new();
        assert!(layers.add(named("design/surface/tin1")).is_ok());

        // Real layers, not placeholders: each must be findable and settable.
        assert!(layers.find("design").is_some());
        assert!(layers.find("design/surface").is_some());
        assert!(layers.find("design/surface/tin1").is_some());
        assert_eq!(layers.find("design").unwrap().name, "design");
        assert_eq!(layers.size(), 4, "three levels plus the default layer 0");
    }

    #[test]
    fn adding_a_sibling_reuses_the_existing_ancestors() {
        let layers = job_tree();
        // 0, design, design/surface, .../tin1, .../tin2, design/road, .../kerb,
        // asbuilt, asbuilt/road, .../kerb, asbuilt/services, .../water.
        assert_eq!(layers.size(), 12);
    }

    #[test]
    fn adding_a_layer_that_already_exists_is_refused_but_ensure_is_not() {
        let mut layers = LayerDatabase::new();
        assert!(layers.add(named("design/surface")).is_ok());

        let again = layers.add(named("design/surface"));
        assert_eq!(again.unwrap_err().code, ErrorCode::AlreadyExists);

        // ensure() is what an importer wants: make the path exist, do not
        // complain.
        assert!(layers.ensure("design/surface").is_ok());
        assert!(layers.ensure("design/surface/tin9").is_ok());
        assert!(layers.contains("design/surface/tin9"));
        assert!(
            layers.ensure("design//bad").is_err(),
            "ensure still validates"
        );
    }

    #[test]
    fn children_and_roots_list_only_the_immediate_level() {
        let layers = job_tree();

        let roots = layers.roots();
        assert_eq!(roots, vec!["0", "asbuilt", "design"]);

        let design_children = layers.children("design");
        assert_eq!(design_children, vec!["design/road", "design/surface"]);

        let surface_children = layers.children("design/surface");
        assert_eq!(surface_children.len(), 2);
        assert_eq!(surface_children[0], "design/surface/tin1");

        assert!(layers.children("design/surface/tin1").is_empty());
        assert!(layers.children("nope").is_empty());
        assert!(layers.has_children("design"));
        assert!(!layers.has_children("design/surface/tin1"));
        assert!(
            !layers.has_children("0"),
            "the default layer has no children"
        );
    }

    #[test]
    fn subtree_is_pre_order_and_stops_at_the_branch() {
        let layers = job_tree();
        let under = layers.subtree("design");
        assert_eq!(under.len(), 6);
        assert_eq!(under[0], "design", "the branch includes itself, first");
        assert_eq!(under[1], "design/road");
        assert_eq!(under[2], "design/road/kerb");
        assert_eq!(under[3], "design/surface");
        assert_eq!(under[4], "design/surface/tin1");
        assert_eq!(under[5], "design/surface/tin2");

        assert!(layers.subtree("missing").is_empty());
        // Nothing from the other branch leaks in even though both have
        // road/kerb.
        for name in &under {
            assert!(!name.starts_with("asbuilt"));
        }
    }

    #[test]
    fn visibility_inherits_all_the_way_to_the_root() {
        let mut layers = job_tree();
        assert!(layers.effectively_visible("design/surface/tin1"));

        // Switch off the GRANDparent: a rule that only checked the immediate
        // parent would still call the leaf visible.
        let mut design = layers.find("design").unwrap().clone();
        design.visible = false;
        assert!(layers.update(&design).is_ok());

        assert!(!layers.effectively_visible("design"));
        assert!(!layers.effectively_visible("design/surface"));
        assert!(!layers.effectively_visible("design/surface/tin1"));
        // And the other branch is untouched.
        assert!(layers.effectively_visible("asbuilt/road/kerb"));
        // The layer's OWN flag is unchanged; only the effective answer differs.
        assert!(layers.find("design/surface/tin1").unwrap().visible);
    }

    #[test]
    fn lock_inherits_and_a_missing_layer_is_neither_visible_nor_editable() {
        let mut layers = job_tree();
        assert!(!layers.effectively_locked("design/surface/tin1"));

        let mut surface = layers.find("design/surface").unwrap().clone();
        surface.locked = true;
        assert!(layers.update(&surface).is_ok());
        assert!(layers.effectively_locked("design/surface/tin1"));
        assert!(!layers.effectively_locked("design/road/kerb"));

        // A layer that is gone: not drawn, not editable.
        assert!(!layers.effectively_visible("design/gone"));
        assert!(layers.effectively_locked("design/gone"));
    }

    #[test]
    fn removing_a_branch_node_is_refused_rather_than_orphaning_it() {
        let mut layers = job_tree();
        let refused = layers.remove("design/surface");
        assert_eq!(refused.unwrap_err().code, ErrorCode::InvalidArgument);
        assert!(
            layers.contains("design/surface/tin1"),
            "nothing may have been deleted"
        );

        // A leaf is fine.
        assert!(layers.remove("design/surface/tin2").is_ok());
        assert!(!layers.contains("design/surface/tin2"));
    }

    #[test]
    fn removing_a_subtree_returns_it_deepest_first_so_undo_can_replay_it() {
        let mut layers = job_tree();
        let removed = layers
            .remove_subtree("design")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(removed.len(), 6);
        // Deepest first: replaying the list backwards recreates parents before
        // children, which is exactly what an undo has to do.
        assert_eq!(
            removed[0].name, "design/surface/tin2",
            "deepest and last, first"
        );
        assert_eq!(removed[removed.len() - 1].name, "design");

        assert!(!layers.contains("design"));
        assert!(!layers.contains("design/surface/tin1"));
        assert!(
            layers.contains("asbuilt/road/kerb"),
            "the other branch survives"
        );

        // Replaying it backwards restores the branch exactly.
        for layer in removed.iter().rev() {
            assert!(layers.add(layer.clone()).is_ok(), "{}", layer.name);
        }
        assert!(layers.contains("design/surface/tin1"));
    }

    #[test]
    fn the_default_layer_cannot_be_removed_or_renamed() {
        let mut layers = job_tree();
        assert!(layers.remove("0").is_err());
        assert!(layers.remove_subtree("0").is_err());
        assert!(layers.rename_subtree("0", "zero").is_err());
        assert!(layers.contains("0"));
    }

    #[test]
    fn renaming_a_branch_moves_every_descendant_and_reports_the_mapping() {
        let mut layers = job_tree();
        let mapping = layers
            .rename_subtree("design", "proposed")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(mapping.len(), 6);

        for (before, after) in &mapping {
            assert_eq!(*after, format!("proposed{}", &before["design".len()..]));
            assert!(!layers.contains(before), "{before} should have moved");
            assert!(layers.contains(after), "{after} should exist");
        }
        assert!(layers.contains("proposed/surface/tin1"));
        assert!(
            layers.contains("asbuilt/road/kerb"),
            "the other branch is untouched"
        );
    }

    #[test]
    fn renaming_into_another_branch_creates_the_new_parents() {
        let mut layers = job_tree();
        let mapping = layers.rename_subtree("design/surface", "archive/2024/surface");
        assert!(mapping.is_ok());

        assert!(layers.contains("archive"));
        assert!(layers.contains("archive/2024"));
        assert!(layers.contains("archive/2024/surface/tin1"));
        assert!(!layers.contains("design/surface"));
        assert!(
            layers.contains("design/road/kerb"),
            "the rest of design stays"
        );
    }

    #[test]
    fn a_colliding_rename_changes_nothing_at_all() {
        let mut layers = job_tree();
        // asbuilt/road already exists, so moving design/road onto it collides.
        let refused = layers.rename_subtree("design/road", "asbuilt/road");
        assert_eq!(refused.unwrap_err().code, ErrorCode::AlreadyExists);

        // The whole point: a failure must leave the tree exactly as it was,
        // not half moved.
        assert!(layers.contains("design/road"));
        assert!(layers.contains("design/road/kerb"));
        assert!(layers.contains("asbuilt/road"));
        assert!(layers.contains("asbuilt/road/kerb"));
        assert_eq!(layers.size(), 12);
    }

    #[test]
    fn a_layer_cannot_be_moved_inside_itself() {
        let mut layers = job_tree();
        let refused = layers.rename_subtree("design", "design/old");
        assert_eq!(refused.unwrap_err().code, ErrorCode::InvalidArgument);
        assert!(layers.contains("design/surface/tin1"));

        assert!(
            layers.rename_subtree("design", "design//bad").is_err(),
            "target is validated"
        );
        assert!(layers.rename_subtree("missing", "elsewhere").is_err());
    }

    #[test]
    fn renaming_to_the_same_name_is_a_no_op_rather_than_a_collision() {
        let mut layers = job_tree();
        let mapping = layers
            .rename_subtree("design", "design")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(mapping.is_empty());
        assert!(layers.contains("design/surface/tin1"));
    }

    #[test]
    fn names_are_ordered_so_that_listing_them_is_already_a_tree_walk() {
        let layers = job_tree();
        let names = layers.names();
        assert!(names.windows(2).all(|pair| pair[0] <= pair[1]));
        // A parent always precedes its children, which is what lets a tree be
        // built in one pass instead of by repeated lookup. (Not IMMEDIATELY:
        // see the test after this one.)
        for (i, name) in names.iter().enumerate().skip(1) {
            let parent = layer_parent(name);
            if !parent.is_empty() {
                let found = position(&names, parent);
                assert!(found < names.len(), "{name} has no parent row");
                assert!(found < i);
            }
        }
    }

    #[test]
    fn a_sibling_that_sorts_between_a_parent_and_its_children_is_not_mistaken_for_them() {
        // Space, '-' and '.' sort below '/', so "design 2" comes between
        // "design" and "design/surface". has_children used to look only at the
        // key straight after "design", find "design 2", and answer no - so
        // remove() took "design" out from under its children.
        let mut layers = LayerDatabase::new();
        assert!(layers.ensure("design/surface").is_ok());
        for sibling in ["design 2", "design-old", "design.bak"] {
            assert!(layers.ensure(sibling).is_ok());
        }
        let names = layers.names();
        assert!(position(&names, "design") < position(&names, "design 2"));
        assert!(
            position(&names, "design 2") < position(&names, "design/surface"),
            "the premise: the sibling sorts between"
        );

        assert!(layers.has_children("design"));
        assert!(!layers.has_children("design 2"));
        let removed = layers.remove("design");
        assert_eq!(removed.unwrap_err().code, ErrorCode::InvalidArgument);
        assert!(layers.contains("design"));

        // And the inherited state reaches the real children, past the siblings.
        let mut design = layers.find("design").unwrap().clone();
        design.visible = false;
        assert!(layers.update(&design).is_ok());
        assert!(!layers.effectively_visible("design/surface"));
        assert!(
            layers.effectively_visible("design 2"),
            "a sibling is not a child"
        );
    }

    #[test]
    fn inherited_state_is_kept_current_through_every_change() {
        let mut layers = LayerDatabase::new();
        assert!(layers.ensure("a/b/c").is_ok());
        let mut a = layers.find("a").unwrap().clone();
        a.locked = true;
        assert!(layers.update(&a).is_ok());
        assert!(layers.resolve("a/b/c").locked);

        // A layer added under a locked parent is locked from the start.
        assert!(layers.ensure("a/b/d").is_ok());
        assert!(layers.resolve("a/b/d").locked);

        // Moving the branch out from under it frees the whole branch.
        assert!(layers.rename_subtree("a/b", "free/b").is_ok());
        assert!(!layers.resolve("free/b").locked);
        assert!(!layers.resolve("free/b/c").locked);
        assert!(!layers.resolve("free/b/d").locked);
        assert!(layers.resolve("a").locked, "the parent keeps its own lock");

        // A copy carries the state with it; reset() starts clean.
        let copy = layers.clone();
        assert!(copy.resolve("a").locked);
        layers.reset();
        assert!(layers.resolve("0").shown);
        assert!(!layers.resolve("0").locked);

        // A layer that is not there is neither shown nor editable.
        let missing = layers.resolve("nowhere");
        assert!(missing.layer.is_none());
        assert!(!missing.shown);
        assert!(missing.locked);
    }
}
