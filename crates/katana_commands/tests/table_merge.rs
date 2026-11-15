//! The twin of tests/commands/customisation/test_table_merge.cpp: the table
//! commands the style and linetype managers need beyond create, update,
//! rename and delete - merge, duplicate, purge, and an update that is no edit
//! at all - plus the delete guards, which read entity::table_usage and say how
//! many hold an item rather than only the first.

pub mod support;

use katana_commands::*;
use katana_core::error::{Error, ErrorCode, Status};
use katana_entity::table_usage::table_usage;
use katana_entity::{
    Entity, EntityId, Geometry, HatchLineFamily, HatchPattern, Layer, Linetype, LinetypeElement,
    Model, PointGeometry, Style, TableUsage, UsageOptions,
};
use katana_geometry::Point2;
use support::double_eq;

struct TableFixture {
    model: Model,
    stack: CommandStack,
}

impl TableFixture {
    fn new() -> Self {
        let mut model = Model::new();
        let stack = CommandStack::new(&mut model, None);
        Self { model, stack }
    }

    fn must(&mut self, command: CommandPtr) {
        if let Err(error) = self.stack.execute(&mut self.model, command) {
            panic!("{}", error.describe());
        }
    }

    fn refused(&mut self, command: CommandPtr) -> ErrorCode {
        self.refusal(command).code
    }

    fn refusal(&mut self, command: CommandPtr) -> Error {
        self.stack
            .execute(&mut self.model, command)
            .expect_err("unexpectedly succeeded")
    }

    fn undo(&mut self) -> Status {
        self.stack.undo(&mut self.model)
    }

    fn redo(&mut self) -> Status {
        self.stack.redo(&mut self.model)
    }

    /// The C++ `point(style, layer = "0")`.
    fn point(&mut self, style: &str, layer: &str) -> EntityId {
        let attributes = EntityAttributes {
            layer: String::from(layer),
            style: String::from(style),
            color: None,
        };
        let status = self.stack.execute(
            &mut self.model,
            create_point(Point2::new(0.0, 0.0), attributes),
        );
        assert!(status.is_ok());
        let created = self.stack.last_created_entities();
        created.first().copied().unwrap_or(0)
    }

    fn entity(&self, id: EntityId) -> &Entity {
        self.model
            .entities
            .find(id)
            .unwrap_or_else(|| panic!("no entity {id}"))
    }

    fn linetype_of_layer(&self, name: &str) -> &str {
        &self
            .model
            .layers
            .find(name)
            .expect("the layer exists")
            .linetype
    }

    fn linetype_of_style(&self, name: &str) -> &str {
        &self
            .model
            .styles
            .find(name)
            .expect("the style exists")
            .linetype
    }
}

/// The C++ `dashed(name, dash = 1.0)`.
fn dashed(name: &str, dash: f64) -> Linetype {
    Linetype {
        name: String::from(name),
        pattern: vec![
            LinetypeElement { length: dash },
            LinetypeElement { length: -0.5 },
        ],
        ..Linetype::default()
    }
}

/// The C++ `style(name, linetype = "continuous")`.
fn style(name: &str, linetype: &str) -> Style {
    Style {
        name: String::from(name),
        linetype: String::from(linetype),
        ..Style::default()
    }
}

fn layer_with_linetype(name: &str, linetype: &str) -> Layer {
    Layer {
        name: String::from(name),
        linetype: String::from(linetype),
        ..Layer::default()
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| String::from(*name)).collect()
}

/// The C++ `TableItems{styles, linetypes, hatchPatterns}`.
fn items(styles: &[&str], linetypes: &[&str], hatch_patterns: &[&str]) -> TableItems {
    TableItems {
        styles: names(styles),
        linetypes: names(linetypes),
        hatch_patterns: names(hatch_patterns),
    }
}

// ---- merge ---------------------------------------------------------------------------

mod table_merge {
    use super::*;

    #[test]
    fn merging_a_style_moves_every_wearer_onto_the_target_and_deletes_it_in_one_undo_step() {
        let mut f = TableFixture::new();
        f.must(create_style(style("Kerb", "continuous")));
        f.must(create_style(style("Kerb 2", "continuous")));
        let a = f.point("Kerb 2", "0");
        let b = f.point("Kerb 2", "0");
        let c = f.point("Kerb", "0"); // already on the target
        let steps = f.stack.undo_count();

        f.must(merge_style("Kerb 2", "Kerb"));
        assert_eq!(f.entity(a).style, "Kerb");
        assert_eq!(f.entity(b).style, "Kerb");
        assert_eq!(f.entity(c).style, "Kerb");
        assert!(!f.model.styles.contains("Kerb 2"));
        assert_eq!(f.stack.undo_count(), steps + 1, "one step");
        assert_eq!(f.stack.undo_name(), "MergeStyle");

        // Undo puts back exactly the wearers it moved - not c, which named the
        // target before the merge ran.
        assert!(f.undo().is_ok());
        assert!(f.model.styles.contains("Kerb 2"));
        assert_eq!(f.entity(a).style, "Kerb 2");
        assert_eq!(f.entity(b).style, "Kerb 2");
        assert_eq!(f.entity(c).style, "Kerb");

        assert!(f.redo().is_ok());
        assert!(!f.model.styles.contains("Kerb 2"));
        assert_eq!(f.entity(a).style, "Kerb");
    }

    #[test]
    fn merging_a_linetype_repoints_every_layer_and_style_naming_it() {
        let mut f = TableFixture::new();
        f.must(create_linetype(dashed("fence", 1.0)));
        f.must(create_linetype(dashed("fence old", 2.0)));
        f.must(create_layer(layer_with_linetype("survey", "fence old")));
        f.must(create_style(style("Post", "fence old")));
        f.must(create_style(style("Rail", "fence"))); // already on the target

        f.must(merge_linetype("fence old", "fence"));
        assert_eq!(f.linetype_of_layer("survey"), "fence");
        assert_eq!(f.linetype_of_style("Post"), "fence");
        assert!(!f.model.linetypes.contains("fence old"));

        assert!(f.undo().is_ok());
        assert_eq!(f.linetype_of_layer("survey"), "fence old");
        assert_eq!(f.linetype_of_style("Post"), "fence old");
        assert_eq!(f.linetype_of_style("Rail"), "fence", "it was never moved");
        let restored = f
            .model
            .linetypes
            .find("fence old")
            .expect("the definition comes back");
        // The definition comes back as it was, not as a copy of the target.
        double_eq(restored.pattern[0].length, 2.0);
    }

    #[test]
    fn a_merge_refuses_a_protected_source_a_missing_target_and_itself() {
        let mut f = TableFixture::new();
        f.must(create_linetype(dashed("fence", 1.0)));
        f.must(create_style(style("Kerb", "continuous")));
        assert_eq!(
            f.refused(merge_linetype("continuous", "fence")),
            ErrorCode::CommandRejected,
            "everything that resolves to continuous would change what it draws"
        );
        assert_eq!(
            f.refused(merge_linetype("fence", "nosuch")),
            ErrorCode::NotFound
        );
        assert_eq!(
            f.refused(merge_linetype("nosuch", "fence")),
            ErrorCode::NotFound
        );
        assert_eq!(
            f.refused(merge_style("Kerb", "nosuch")),
            ErrorCode::NotFound
        );
        assert_eq!(
            f.refused(merge_style("Kerb", "Kerb")),
            ErrorCode::InvalidArgument
        );
        // A refused merge changes nothing.
        assert!(f.model.linetypes.contains("fence"));
        assert!(f.model.styles.contains("Kerb"));
    }

    #[test]
    fn merge_delete_and_purge_are_destructive_and_duplicate_is_not() {
        // The flag is the confirmation gate for commands an AI proposes
        // (command.rs): discarding a definition must ask (audit MOD-08).
        assert!(merge_style("a", "b").is_destructive());
        assert!(merge_linetype("a", "b").is_destructive());
        assert!(delete_style("a").is_destructive());
        assert!(delete_linetype("a").is_destructive());
        assert!(delete_hatch_pattern("a").is_destructive());
        assert!(delete_dimension_style("a").is_destructive());
        assert!(delete_alignment("a").is_destructive());
        assert!(purge_table_items(items(&["a"], &[], &[])).is_destructive());
        assert!(!duplicate_style("a", "b").is_destructive());
        assert!(!update_style(style("a", "continuous")).is_destructive());
    }
}

// ---- rename ------------------------------------------------------------------------------

mod table_rename {
    use super::*;

    #[test]
    fn undoing_a_linetype_rename_onto_a_name_already_held_puts_back_only_what_the_rename_moved() {
        // The commands layer does not see the style library, so a style or
        // layer may name a linetype the model lacks - a library linestyle,
        // which STYLE SET accepts. The rename's validate asks only the model
        // table, so a model linetype can be renamed onto that name. Its undo
        // used to repoint EVERYTHING naming the new name back to the old one,
        // rewriting s and survey, which the rename never touched, to fence.
        let mut f = TableFixture::new();
        f.must(create_linetype(dashed("fence", 1.0)));
        f.must(create_style(style("s", "COMM Telephone Pole")));
        f.must(create_style(style("t", "fence")));
        f.must(create_layer(layer_with_linetype(
            "survey",
            "COMM Telephone Pole",
        )));
        f.must(create_layer(layer_with_linetype("kerbs", "fence")));

        f.must(rename_linetype("fence", "COMM Telephone Pole"));
        assert_eq!(f.linetype_of_style("t"), "COMM Telephone Pole");
        assert_eq!(f.linetype_of_layer("kerbs"), "COMM Telephone Pole");

        assert!(f.undo().is_ok());
        assert!(f.model.linetypes.contains("fence"));
        assert!(!f.model.linetypes.contains("COMM Telephone Pole"));
        // Moved by the rename, so moved back:
        assert_eq!(f.linetype_of_style("t"), "fence");
        assert_eq!(f.linetype_of_layer("kerbs"), "fence");
        // Named the new name before the rename ran, so left alone:
        assert_eq!(f.linetype_of_style("s"), "COMM Telephone Pole");
        assert_eq!(f.linetype_of_layer("survey"), "COMM Telephone Pole");

        // Redo moves the same holders again; a second undo is as exact.
        assert!(f.redo().is_ok());
        assert_eq!(f.linetype_of_style("t"), "COMM Telephone Pole");
        assert!(f.undo().is_ok());
        assert_eq!(f.linetype_of_style("t"), "fence");
        assert_eq!(f.linetype_of_style("s"), "COMM Telephone Pole");
    }

    #[test]
    fn undoing_a_style_rename_onto_a_name_entities_already_wore_puts_back_only_its_own_wearers() {
        // An entity can wear a style the table lacks - a project saved before
        // its style was removed outside Katana; STYLE USAGE lists it as "not
        // in the style table". Added straight to the database, since every
        // command refuses to create one.
        let mut f = TableFixture::new();
        f.must(create_style(style("Kerb", "continuous")));
        let moved = f.point("Kerb", "0");
        let orphan = Entity {
            layer: String::from("0"),
            style: String::from("Ghost"),
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(1.0, 1.0),
            }),
            ..Entity::default()
        };
        let stranded = f
            .model
            .entities
            .add(orphan)
            .expect("added straight to the database");

        f.must(rename_style("Kerb", "Ghost"));
        assert_eq!(f.entity(moved).style, "Ghost");

        assert!(f.undo().is_ok());
        assert!(f.model.styles.contains("Kerb"));
        assert!(!f.model.styles.contains("Ghost"));
        assert_eq!(f.entity(moved).style, "Kerb");
        assert_eq!(
            f.entity(stranded).style,
            "Ghost",
            "it wore Ghost before the rename ran"
        );
    }
}

// ---- duplicate ---------------------------------------------------------------------------

mod table_duplicate {
    use super::*;

    #[test]
    fn a_duplicate_is_a_copy_under_the_new_name_and_undo_removes_it() {
        let mut f = TableFixture::new();
        let kerb = Style {
            line_weight: 0.7,
            symbol: String::from("cross"),
            ..style("Kerb", "continuous")
        };
        f.must(create_style(kerb.clone()));
        f.must(duplicate_style("Kerb", "Kerb red"));
        let expected = Style {
            name: String::from("Kerb red"),
            ..kerb
        };
        assert_eq!(f.model.styles.find("Kerb red"), Some(&expected));
        assert_eq!(
            f.refused(duplicate_style("Kerb", "Kerb red")),
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            f.refused(duplicate_style("nosuch", "x")),
            ErrorCode::NotFound
        );
        assert!(f.undo().is_ok());
        assert!(!f.model.styles.contains("Kerb red"));

        f.must(create_linetype(dashed("fence", 1.0)));
        f.must(duplicate_linetype("fence", "fence 2"));
        assert_eq!(
            f.model
                .linetypes
                .find("fence 2")
                .map(|linetype| &linetype.pattern),
            f.model
                .linetypes
                .find("fence")
                .map(|linetype| &linetype.pattern)
        );
    }
}

// ---- the guards ------------------------------------------------------------------------------

mod table_guards {
    use super::*;

    #[test]
    fn deleting_a_style_in_use_gives_the_count_and_the_first_wearer() {
        let mut f = TableFixture::new();
        f.must(create_style(style("Kerb", "continuous")));
        let first = f.point("Kerb", "0");
        f.point("Kerb", "0");
        f.point("Kerb", "0");
        let error = f.refusal(delete_style("Kerb"));
        assert_eq!(error.code, ErrorCode::CommandRejected);
        // Three wearers; the first is the lowest id.
        assert_eq!(
            error.context,
            format!("used by 3 entities, e.g. id={first}")
        );
    }

    #[test]
    fn deleting_a_linetype_in_use_counts_the_layers_styles_and_entities_reaching_it() {
        let mut f = TableFixture::new();
        f.must(create_linetype(dashed("fence", 1.0)));
        f.must(create_layer(layer_with_linetype("survey", "fence")));
        f.must(create_style(style("Post", "fence")));
        // Two points on survey with no style reach fence through the layer; a
        // third wears Post and reaches it through the style.
        f.point("", "survey");
        f.point("", "survey");
        f.point("Post", "0");
        let error = f.refusal(delete_linetype("fence"));
        assert_eq!(error.code, ErrorCode::CommandRejected);
        assert_eq!(
            error.context,
            "used by 1 layer, 1 style and 3 entities, e.g. layer=survey"
        );
    }

    #[test]
    fn updating_continuous_to_a_pattern_is_refused_by_validate_not_only_by_execute() {
        // audit MOD-09: validate() used to say yes, and execute() then said no
        // - a caller that pre-validates (the AI safety model) was misled.
        let mut f = TableFixture::new();
        let command = update_linetype(dashed("continuous", 1.0));
        let validated = command.validate(&CommandContext::new(&mut f.model));
        assert_eq!(
            validated.expect_err("refused by validate").code,
            ErrorCode::InvalidArgument
        );
    }
}

// ---- no-op updates ---------------------------------------------------------------------------

mod table_no_op_update {
    use super::*;

    #[test]
    fn saving_an_unedited_style_pushes_no_undo_step_and_changes_nothing() {
        // The natural QT-02 regression base: the style manager's Save on a
        // form nobody touched must leave the style byte-identical and the
        // history as it was - which is also what stops it marking the project
        // modified.
        let mut f = TableFixture::new();
        let imported = Style {
            symbol: String::from("CULT Bollard"),
            description: String::from("library linestyle"),
            ..style("WATR Main", "WATR Main")
        };
        f.must(create_style(imported.clone()));
        let steps = f.stack.undo_count();

        assert!(update_style_if_changed(&f.model, imported.clone()).is_none());
        let continuous = f
            .model
            .linetypes
            .find("continuous")
            .expect("built in")
            .clone();
        assert!(update_linetype_if_changed(&f.model, continuous).is_none());
        assert_eq!(f.stack.undo_count(), steps);
        assert_eq!(f.model.styles.find("WATR Main"), Some(&imported));

        // A real change still gets a command, and it is undoable.
        let heavier = Style {
            line_weight: 0.5,
            ..imported
        };
        let command =
            update_style_if_changed(&f.model, heavier).expect("a real change gets a command");
        f.must(command);
        assert_eq!(f.stack.undo_count(), steps + 1);
        // A missing name still gets a command, whose validate says NotFound.
        let missing = update_style_if_changed(&f.model, style("nosuch", "continuous"))
            .expect("a missing name gets a command");
        assert_eq!(f.refused(missing), ErrorCode::NotFound);
    }
}

// ---- purge ----------------------------------------------------------------------------------

mod table_purge {
    use super::*;

    #[test]
    fn purging_a_set_deletes_it_all_as_one_step_and_one_undo_restores_it() {
        let mut f = TableFixture::new();
        f.must(create_linetype(dashed("fence", 1.0)));
        f.must(create_style(style("Post", "fence")));
        f.must(create_style(style("Spare", "continuous")));
        let steps = f.stack.undo_count();

        // fence is named only by Post, which goes in the same set: it is free.
        f.must(purge_table_items(items(
            &["Post", "Spare"],
            &["fence"],
            &[],
        )));
        assert!(!f.model.styles.contains("Post"));
        assert!(!f.model.styles.contains("Spare"));
        assert!(!f.model.linetypes.contains("fence"));
        assert_eq!(f.stack.undo_count(), steps + 1);
        assert_eq!(f.stack.undo_name(), "PurgeTables");

        assert!(f.undo().is_ok());
        assert!(f.model.styles.contains("Post"));
        assert!(f.model.styles.contains("Spare"));
        assert_eq!(f.linetype_of_style("Post"), "fence");
        assert!(f.model.linetypes.contains("fence"));
    }

    #[test]
    fn a_purge_refuses_anything_still_used_once_the_set_is_gone_and_changes_nothing() {
        let mut f = TableFixture::new();
        f.must(create_linetype(dashed("fence", 1.0)));
        f.must(create_style(style("Post", "fence")));
        f.must(create_style(style("Spare", "continuous")));
        // Post stays, so fence is held.
        let held = f.refusal(purge_table_items(items(&["Spare"], &["fence"], &[])));
        assert_eq!(held.code, ErrorCode::CommandRejected);
        assert_eq!(held.context, "fence: used by 1 style, e.g. style=Post");
        assert!(
            f.model.styles.contains("Spare"),
            "a refused purge removes nothing"
        );

        f.point("Spare", "0");
        assert_eq!(
            f.refused(purge_table_items(items(&["Spare"], &[], &[]))),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            f.refused(purge_table_items(items(&["nosuch"], &[], &[]))),
            ErrorCode::NotFound
        );
        assert_eq!(
            f.refused(purge_table_items(TableItems::default())),
            ErrorCode::InvalidArgument,
            "never an empty undo step"
        );
    }

    #[test]
    fn continuous_and_none_are_refused_by_validate_even_when_nothing_names_them() {
        // Layer 0 names continuous and none by default, so a refusal of either
        // could come from "still used" alone and never reach the protection.
        // Layer 0 is pointed elsewhere first, so only the protection can
        // refuse them - and it has to in validate(), where a caller that
        // pre-validates looks; the table's own refusal comes only at
        // execute().
        let mut f = TableFixture::new();
        f.must(create_linetype(dashed("fence", 1.0)));
        let stone = HatchPattern {
            name: String::from("stone"),
            families: vec![HatchLineFamily {
                angle: 0.0,
                spacing: 1.0,
                offset: 0.0,
            }],
            ..HatchPattern::default()
        };
        f.must(create_hatch_pattern(stone));
        let mut zero = f.model.layers.find("0").expect("built in").clone();
        zero.linetype = String::from("fence");
        zero.hatch_pattern = String::from("stone");
        f.must(update_layer(zero));
        let usage = table_usage(&f.model, UsageOptions::default());
        assert!(!TableUsage::of(&usage.linetypes, "continuous").used());
        assert!(!TableUsage::of(&usage.hatch_patterns, "none").used());

        let continuous = purge_table_items(items(&[], &["continuous"], &[]))
            .validate(&CommandContext::new(&mut f.model));
        assert_eq!(
            continuous.expect_err("continuous is protected").code,
            ErrorCode::CommandRejected
        );
        let none = purge_table_items(items(&[], &[], &["none"]))
            .validate(&CommandContext::new(&mut f.model));
        assert_eq!(
            none.expect_err("none is protected").code,
            ErrorCode::CommandRejected
        );
    }
}
