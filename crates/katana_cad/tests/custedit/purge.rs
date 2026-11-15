// plan_purge and purge_command: what nothing uses, found to a fixpoint,
// deleted as one undo step, and never an empty one (purge.rs;
// tests/cad/customisation/test_purge.cpp).

use katana_cad::Document;
use katana_cad::purge::{PurgeOptions, plan_purge, purge_command};
use katana_commands::{
    CommandPtr, EntityAttributes, create_hatch_pattern, create_layer, create_linetype,
    create_point, create_style, update_layer,
};
use katana_core::ErrorCode;
use katana_entity::{HatchLineFamily, HatchPattern, Layer, Linetype, LinetypeElement, Style};
use katana_geometry::Point2;

fn must(document: &mut Document, command: CommandPtr) {
    if let Err(error) = document.execute(command) {
        panic!("{}", error.describe());
    }
}

fn style(name: &str, linetype: &str, hatch: &str) -> Style {
    Style {
        name: String::from(name),
        linetype: String::from(linetype),
        hatch_pattern: String::from(hatch),
        ..Style::default()
    }
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

/// The drawing every test reads (the C++ fixture `Purge`):
///
/// ```text
///   linetypes  fence  named only by style Post, which nothing wears
///              dash   named by layer kerbs
///              rail   named by style Kerb, which a point wears
///              spare  named by nothing
///   hatches    grass  named only by Post
///              stone  named by layer kerbs
///              brick  named by nothing
///   styles     Post (fence, grass) and Spare: worn by nothing
///              Kerb (rail): worn by the one point
/// ```
///
/// So a purge takes Post and Spare, and - because Post goes - fence and grass
/// with them; spare and brick because nothing names them. It keeps dash and
/// stone (a layer names them), rail (Kerb is worn), and the protected
/// continuous and none.
fn fixture() -> Document {
    let mut document = Document::default();
    for name in ["fence", "dash", "rail", "spare"] {
        let linetype = Linetype {
            name: String::from(name),
            pattern: vec![
                LinetypeElement { length: 1.0 },
                LinetypeElement { length: -0.5 },
            ],
            ..Linetype::default()
        };
        must(&mut document, create_linetype(linetype));
    }
    for name in ["grass", "stone", "brick"] {
        let pattern = HatchPattern {
            name: String::from(name),
            families: vec![HatchLineFamily {
                angle: 0.0,
                spacing: 1.0,
                offset: 0.0,
            }],
            ..HatchPattern::default()
        };
        must(&mut document, create_hatch_pattern(pattern));
    }
    let kerbs = Layer {
        name: String::from("kerbs"),
        linetype: String::from("dash"),
        hatch_pattern: String::from("stone"),
        ..Layer::default()
    };
    must(&mut document, create_layer(kerbs));
    must(&mut document, create_style(style("Post", "fence", "grass")));
    must(
        &mut document,
        create_style(style("Spare", "continuous", "")),
    );
    must(&mut document, create_style(style("Kerb", "rail", "")));
    let attributes = EntityAttributes {
        style: String::from("Kerb"),
        ..EntityAttributes::default()
    };
    must(
        &mut document,
        create_point(Point2::new(0.0, 0.0), attributes),
    );
    document
}

mod purge {
    use super::*;

    #[test]
    fn the_plan_is_what_nothing_uses_including_what_only_a_purged_style_named() {
        let document = fixture();
        let plan = plan_purge(document.model(), &PurgeOptions::default());
        assert_eq!(plan.styles, strings(&["Post", "Spare"]));
        assert_eq!(
            plan.linetypes,
            strings(&["fence", "spare"]),
            "fence is freed by purging Post, in the same purge"
        );
        assert_eq!(plan.hatch_patterns, strings(&["brick", "grass"]));
    }

    #[test]
    fn protected_items_are_never_planned_even_in_an_empty_drawing() {
        let _document = fixture();
        let empty = Document::default();
        // A new drawing has continuous, none, layer 0 and nothing using them.
        assert!(plan_purge(empty.model(), &PurgeOptions::default()).empty());
        let command = match purge_command(empty.model(), &PurgeOptions::default()) {
            Ok(command) => command,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(
            command.is_none(),
            "nothing to purge is no command, not an empty one"
        );
    }

    #[test]
    fn continuous_and_none_are_never_planned_even_when_nothing_names_them() {
        let mut document = fixture();
        // In the drawings above layer 0 names continuous and none, so they are
        // "used" and the protection is never what keeps them out. Point layer
        // 0 at dash and stone: now continuous is named only by Spare, which is
        // purged, and none by nothing (Kerb's hatch is empty, so the point
        // reaches layer 0's stone). Only the protection can exclude them.
        let mut zero = document.model().layers.find("0").expect("layer 0").clone();
        zero.linetype = String::from("dash");
        zero.hatch_pattern = String::from("stone");
        must(&mut document, update_layer(zero));

        // Without the protection these would be {continuous, fence, spare}
        // and {brick, grass, none}.
        let plan = plan_purge(document.model(), &PurgeOptions::default());
        assert_eq!(plan.styles, strings(&["Post", "Spare"]));
        assert_eq!(plan.linetypes, strings(&["fence", "spare"]));
        assert_eq!(plan.hatch_patterns, strings(&["brick", "grass"]));

        // And so the purge goes through: a plan holding continuous would be
        // refused as a whole, and remove nothing - not even spare.
        let command = match purge_command(document.model(), &PurgeOptions::default()) {
            Ok(Some(command)) => command,
            Ok(None) => panic!("there is something to purge"),
            Err(error) => panic!("{}", error.describe()),
        };
        must(&mut document, command);
        assert!(!document.model().linetypes.contains("spare"));
        assert!(document.model().linetypes.contains("continuous"));
        assert!(document.model().hatch_patterns.contains("none"));
    }

    #[test]
    fn a_linetype_a_staying_style_names_is_kept() {
        let document = fixture();
        // Styles not purged: Post stays, so fence and grass stay with it.
        let linetypes_only = plan_purge(
            document.model(),
            &PurgeOptions {
                styles: false,
                linetypes: true,
                hatches: true,
                ..PurgeOptions::default()
            },
        );
        assert!(linetypes_only.styles.is_empty());
        assert_eq!(linetypes_only.linetypes, strings(&["spare"]));
        assert_eq!(linetypes_only.hatch_patterns, strings(&["brick"]));

        // The current style is kept although no entity wears it, and what it
        // names is kept with it.
        let keeping_post = plan_purge(
            document.model(),
            &PurgeOptions {
                keep_styles: strings(&["Post"]),
                ..PurgeOptions::default()
            },
        );
        assert_eq!(keeping_post.styles, strings(&["Spare"]));
        assert_eq!(keeping_post.linetypes, strings(&["spare"]));
        assert_eq!(keeping_post.hatch_patterns, strings(&["brick"]));
    }

    #[test]
    fn one_purge_is_one_undo_step_and_one_undo_restores_everything() {
        let mut document = fixture();
        let styles_before = document.model().styles.all();
        let linetypes_before = document.model().linetypes.all();
        let hatches_before = document.model().hatch_patterns.all();
        let steps = document.history().undo_count();

        let command = match purge_command(document.model(), &PurgeOptions::default()) {
            Ok(Some(command)) => command,
            Ok(None) => panic!("there is something to purge"),
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(command.is_destructive());
        must(&mut document, command);
        assert_eq!(document.history().undo_count(), steps + 1);
        // What is left: styles Kerb; linetypes continuous, dash, rail;
        // hatches none, stone.
        assert_eq!(document.model().styles.names(), strings(&["Kerb"]));
        assert_eq!(
            document.model().linetypes.names(),
            strings(&["continuous", "dash", "rail"])
        );
        assert_eq!(
            document.model().hatch_patterns.names(),
            strings(&["none", "stone"])
        );

        // A second purge finds nothing: the plan was already a fixpoint.
        let again = match purge_command(document.model(), &PurgeOptions::default()) {
            Ok(again) => again,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(again.is_none());

        if let Err(error) = document.undo() {
            panic!("{}", error.describe());
        }
        assert_eq!(document.model().styles.all(), styles_before);
        assert_eq!(document.model().linetypes.all(), linetypes_before);
        assert_eq!(document.model().hatch_patterns.all(), hatches_before);
    }

    #[test]
    fn asking_for_nothing_is_a_mistake_not_an_empty_purge() {
        let document = fixture();
        let command = purge_command(
            document.model(),
            &PurgeOptions {
                styles: false,
                linetypes: false,
                hatches: false,
                ..PurgeOptions::default()
            },
        );
        let error = match command {
            Ok(_) => panic!("asking for nothing was not refused"),
            Err(error) => error,
        };
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}
