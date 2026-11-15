// The styles and linetypes manager below the window: its rows, its chips and
// search, the fields a multi-selection agrees on, the one command a bulk edit
// makes, its diagnostics and "Select Users". Hand-built library and drawing
// only (tests/cad/customisation/test_style_manager_rows.cpp).

use katana_cad::Document;
use katana_cad::style_catalogue::free_style_name;
use katana_cad::style_manager_rows::{
    LinetypeOrigin, LinetypeRow, LinetypeRowKind, StyleDiagnosticKind, StyleFields, StyleFilter,
    StyleRow, UsageTable, apply_edit, common_fields, edit_styles_command, entities_using,
    free_linetype_name, linetype_rows, matches_filter, matches_search, style_diagnostics,
    style_rows,
};
use katana_commands::{
    CommandPtr, EntityAttributes, create_layer, create_linetype, create_point, create_style,
};
use katana_core::ErrorCode;
use katana_entity::{
    Color, EntityId, Layer, LineStyle, Linetype, LinetypeElement, Stroke, StrokeOp, Style,
    StyleLibrary, StyleUnits,
};
use katana_geometry::Point2;

use crate::{double_eq, strings};

fn stroke(op: StrokeOp, x: f64, y: f64) -> Stroke {
    Stroke {
        op,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

// The C++ fixture's helper; its last two arguments default to "" there.
fn definition(
    name: &str,
    units: StyleUnits,
    at_vertices: bool,
    length: f64,
    strokes: Vec<Stroke>,
    group: &str,
    source: &str,
) -> LineStyle {
    LineStyle {
        name: String::from(name),
        units,
        at_vertices,
        length,
        strokes,
        group: String::from(group),
        source: String::from(source),
        ..LineStyle::default()
    }
}

fn must(document: &mut Document, command: CommandPtr) {
    if let Err(error) = document.execute(command) {
        panic!("{}", error.describe());
    }
}

fn style(name: &str, linetype: &str, symbol: &str, weight: f64) -> Style {
    Style {
        name: String::from(name),
        linetype: String::from(linetype),
        symbol: String::from(symbol),
        line_weight: weight,
        ..Style::default()
    }
}

/// The drawing every test reads (the C++ fixture `StyleManagerRows`):
///
/// ```text
///   library   LS Paper Kerb   paperstyle, `length 4`                 period 4 (mm)
///             LS World Fence  worldstyle, no length; move 0, draw 3,
///                             move 5 - the pen travels 0..5          period 5
///             LS Gate         twoptstyle                             no period
///             dash            worldstyle - ALSO a drawing linetype   a collision (D2)
///             SYM Pit         `mode vertex`                          a symbol, not listed
///   linetypes continuous; dash {1, -0.5} (period 1.5); Dotted {0, -0.25} (0.25)
///   layer     kerbs, linetype dash
///   styles    Kerb   LS Paper Kerb, 0.35      worn by points A and B
///             Fence  LS World Fence, 0.35     worn by nothing
///             Ghost  NOPE Line / NOPE Manhole worn by point C; both names undefined
///             Pit    continuous / cross       worn by nothing; cross is built in
/// ```
struct Fixture {
    document: Document,
    kerb_points: Vec<EntityId>,
    ghost_point: EntityId,
}

fn fixture() -> Fixture {
    let mut library = StyleLibrary::default();
    let move0 = stroke(StrokeOp::Move, 0.0, 0.0);
    let draw3 = stroke(StrokeOp::Draw, 3.0, 0.0);
    let move5 = stroke(StrokeOp::Move, 5.0, 0.0);
    for line_style in [
        definition(
            "LS Paper Kerb",
            StyleUnits::Paper,
            false,
            4.0,
            vec![move0.clone(), draw3.clone()],
            "Roads/Kerbs",
            "linestyles_test.4d",
        ),
        definition(
            "LS World Fence",
            StyleUnits::World,
            false,
            0.0,
            vec![move0.clone(), draw3.clone(), move5.clone()],
            "",
            "",
        ),
        definition(
            "LS Gate",
            StyleUnits::TwoPoint,
            false,
            0.0,
            vec![move0.clone(), draw3.clone()],
            "",
            "",
        ),
        definition(
            "dash",
            StyleUnits::World,
            false,
            0.0,
            vec![move0.clone(), draw3.clone(), move5],
            "",
            "",
        ),
        definition(
            "SYM Pit",
            StyleUnits::World,
            true,
            0.0,
            vec![move0, draw3],
            "",
            "",
        ),
    ] {
        if let Err(error) = library.add(line_style) {
            panic!("{}", error.describe());
        }
    }
    let mut document = Document::default();
    document.set_style_library(library);

    let dash = Linetype {
        name: String::from("dash"),
        pattern: vec![
            LinetypeElement { length: 1.0 },
            LinetypeElement { length: -0.5 },
        ],
        ..Linetype::default()
    };
    must(&mut document, create_linetype(dash));
    let dotted = Linetype {
        name: String::from("Dotted"),
        description: String::from("a dot every quarter metre"),
        pattern: vec![
            LinetypeElement { length: 0.0 },
            LinetypeElement { length: -0.25 },
        ],
    };
    must(&mut document, create_linetype(dotted));
    let kerbs = Layer {
        name: String::from("kerbs"),
        linetype: String::from("dash"),
        ..Layer::default()
    };
    must(&mut document, create_layer(kerbs));

    must(
        &mut document,
        create_style(style("Kerb", "LS Paper Kerb", "", 0.35)),
    );
    must(
        &mut document,
        create_style(style("Fence", "LS World Fence", "", 0.35)),
    );
    must(
        &mut document,
        create_style(style("Ghost", "NOPE Line", "NOPE Manhole", 0.25)),
    );
    must(
        &mut document,
        create_style(style("Pit", "continuous", "cross", 0.25)),
    );

    let on = |style: &str| EntityAttributes {
        layer: String::from("0"),
        style: String::from(style),
        color: None,
    };
    let mut kerb_points = Vec::new();
    for x in [0.0, 1.0] {
        must(&mut document, create_point(Point2::new(x, 0.0), on("Kerb")));
        kerb_points.push(document.last_created_entities()[0]);
    }
    must(
        &mut document,
        create_point(Point2::new(2.0, 0.0), on("Ghost")),
    );
    let ghost_point = document.last_created_entities()[0];
    Fixture {
        document,
        kerb_points,
        ghost_point,
    }
}

#[track_caller]
fn row<'a>(rows: &'a [StyleRow], name: &str) -> &'a StyleRow {
    rows.iter()
        .find(|candidate| candidate.style.name == name)
        .unwrap_or_else(|| panic!("no row {name}"))
}

fn filtered(document: &Document, filter: StyleFilter) -> Vec<String> {
    style_rows(document)
        .into_iter()
        .filter(|candidate| matches_filter(candidate, filter))
        .map(|candidate| candidate.style.name)
        .collect()
}

fn searched(document: &Document, text: &str) -> Vec<String> {
    style_rows(document)
        .into_iter()
        .filter(|candidate| matches_search(candidate, text))
        .map(|candidate| candidate.style.name)
        .collect()
}

#[track_caller]
fn row_of<'a>(rows: &'a [LinetypeRow], name: &str, origin: LinetypeOrigin) -> &'a LinetypeRow {
    rows.iter()
        .find(|candidate| candidate.name == name && candidate.origin == origin)
        .unwrap_or_else(|| panic!("no row {name}"))
}

#[track_caller]
fn stored(document: &Document, name: &str) -> Style {
    document
        .model()
        .styles
        .find(name)
        .unwrap_or_else(|| panic!("no style {name}"))
        .clone()
}

mod style_manager_rows {
    use super::*;

    #[test]
    fn a_style_row_counts_the_entities_wearing_it_and_marks_the_names_nothing_defines() {
        let fixture = fixture();
        let rows = style_rows(&fixture.document);
        assert_eq!(rows.len(), 4);
        // The table's order: ascending by name.
        assert_eq!(rows[0].style.name, "Fence");
        assert_eq!(rows[3].style.name, "Pit");

        assert_eq!(row(&rows, "Kerb").entities, 2);
        assert!(
            !row(&rows, "Kerb").missing(),
            "a library linestyle is defined"
        );
        assert_eq!(row(&rows, "Ghost").entities, 1);
        assert!(row(&rows, "Ghost").missing_linetype);
        assert!(row(&rows, "Ghost").missing_symbol);
        assert_eq!(row(&rows, "Pit").entities, 0);
        assert!(
            !row(&rows, "Pit").missing(),
            "continuous and the built-in cross need nothing"
        );
        assert_eq!(
            row(&rows, "Kerb").style.linetype,
            "LS Paper Kerb",
            "the row carries the style"
        );
    }

    #[test]
    fn the_current_style_is_marked_on_its_row_alone() {
        let mut fixture = fixture();
        for candidate in style_rows(&fixture.document) {
            assert!(!candidate.current, "a new drawing draws ByLayer (D9)");
        }
        if let Err(error) = fixture.document.set_current_style("Fence") {
            panic!("{}", error.describe());
        }
        let rows = style_rows(&fixture.document);
        assert!(row(&rows, "Fence").current);
        assert!(!row(&rows, "Kerb").current);
        assert!(!row(&rows, "Pit").current);
    }

    #[test]
    fn the_chips_split_used_from_unused_and_pick_out_the_missing() {
        let fixture = fixture();
        let document = &fixture.document;
        // Used = worn by an entity: Kerb (2) and Ghost (1); Fence and Pit by
        // none.
        assert_eq!(
            filtered(document, StyleFilter::All),
            strings(&["Fence", "Ghost", "Kerb", "Pit"])
        );
        assert_eq!(
            filtered(document, StyleFilter::Used),
            strings(&["Ghost", "Kerb"])
        );
        assert_eq!(
            filtered(document, StyleFilter::Unused),
            strings(&["Fence", "Pit"])
        );
        assert_eq!(
            filtered(document, StyleFilter::Missing),
            strings(&["Ghost"])
        );
    }

    #[test]
    fn the_search_folds_case_and_looks_in_the_name_linetype_symbol_and_description() {
        let fixture = fixture();
        let document = &fixture.document;
        assert_eq!(
            searched(document, ""),
            strings(&["Fence", "Ghost", "Kerb", "Pit"])
        );
        // "kerb" is Kerb's name and, in another case, in its linetype.
        assert_eq!(searched(document, "kerb"), strings(&["Kerb"]));
        // "PAPER" only in Kerb's linetype "LS Paper Kerb".
        assert_eq!(searched(document, "PAPER"), strings(&["Kerb"]));
        // "manhole" only in Ghost's symbol "NOPE Manhole".
        assert_eq!(searched(document, "manhole"), strings(&["Ghost"]));
        // "continuous": Pit's linetype.
        assert_eq!(searched(document, "Continuous"), strings(&["Pit"]));
        assert!(searched(document, "zzz").is_empty());
    }

    #[test]
    fn a_selection_agrees_only_on_the_fields_every_style_shares() {
        let fixture = fixture();
        let kerb = stored(&fixture.document, "Kerb");
        let fence = stored(&fixture.document, "Fence");
        let common = common_fields(&[kerb.clone(), fence]);
        // Both 0.35 wide, both ByLayer colour, no symbol, size 0, no hatch, no
        // description; their linetypes differ.
        assert!(
            common.linetype.is_none(),
            "LS Paper Kerb against LS World Fence: <varies>"
        );
        assert_eq!(common.line_weight, Some(0.35));
        assert_eq!(
            common.color,
            Some(None),
            "both ByLayer is an agreement, not <varies>"
        );
        assert_eq!(common.symbol.as_deref(), Some(""));
        assert_eq!(common.hatch_pattern.as_deref(), Some(""));

        // One style agrees with itself on everything; no styles on nothing.
        let one = common_fields(std::slice::from_ref(&kerb));
        assert_eq!(one.linetype.as_deref(), Some("LS Paper Kerb"));
        assert_eq!(one.description.as_deref(), Some(""));
        assert!(common_fields(&[]).empty());
    }

    #[test]
    fn an_edit_writes_only_the_fields_it_sets_and_never_the_name() {
        let fixture = fixture();
        let ghost = stored(&fixture.document, "Ghost");
        let edit = StyleFields {
            line_weight: Some(0.7),
            color: Some(Some(Color::new(255, 0, 0, 255))),
            ..StyleFields::default()
        };
        let edited = apply_edit(ghost.clone(), &edit);
        assert_eq!(edited.name, "Ghost");
        assert_eq!(edited.line_weight, 0.7);
        assert_eq!(edited.color, Some(Color::new(255, 0, 0, 255)));
        // Untouched: the undefined names stay exactly as they were (QT-02).
        assert_eq!(edited.linetype, "NOPE Line");
        assert_eq!(edited.symbol, "NOPE Manhole");
        assert_eq!(apply_edit(ghost.clone(), &StyleFields::default()), ghost);
    }

    #[test]
    fn an_edit_that_changes_nothing_makes_no_command() {
        let fixture = fixture();
        let document = &fixture.document;
        let before = document.history().undo_count();
        let empty = edit_styles_command(
            document.model(),
            &strings(&["Kerb", "Fence"]),
            &StyleFields::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(empty.is_none());

        // Both are already 0.35 wide: setting 0.35 changes neither.
        let same = StyleFields {
            line_weight: Some(0.35),
            ..StyleFields::default()
        };
        let unchanged = edit_styles_command(document.model(), &strings(&["Kerb", "Fence"]), &same)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(unchanged.is_none());
        assert_eq!(document.history().undo_count(), before);
    }

    #[test]
    fn a_bulk_edit_is_one_undo_step_and_leaves_each_styles_other_fields_its_own() {
        let mut fixture = fixture();
        let document = &mut fixture.document;
        let before = document.history().undo_count();
        let edit = StyleFields {
            line_weight: Some(0.5),
            ..StyleFields::default()
        };
        let command =
            edit_styles_command(document.model(), &strings(&["Kerb", "Fence", "Pit"]), &edit)
                .unwrap_or_else(|error| panic!("{}", error.describe()))
                .expect("a command");
        must(document, command);
        assert_eq!(
            document.history().undo_count(),
            before + 1,
            "three styles, one step"
        );

        for name in ["Kerb", "Fence", "Pit"] {
            assert_eq!(stored(document, name).line_weight, 0.5, "{name}");
        }
        assert_eq!(stored(document, "Kerb").linetype, "LS Paper Kerb");
        assert_eq!(stored(document, "Fence").linetype, "LS World Fence");
        assert_eq!(stored(document, "Pit").symbol, "cross");

        if let Err(error) = document.undo() {
            panic!("{}", error.describe());
        }
        assert_eq!(stored(document, "Kerb").line_weight, 0.35);
        assert_eq!(stored(document, "Fence").line_weight, 0.35);
        assert_eq!(stored(document, "Pit").line_weight, 0.25);
    }

    #[test]
    fn an_edit_one_style_refuses_is_refused_whole_naming_the_style() {
        let fixture = fixture();
        let document = &fixture.document;
        let negative = StyleFields {
            line_weight: Some(-1.0),
            ..StyleFields::default()
        };
        let Err(refused) =
            edit_styles_command(document.model(), &strings(&["Kerb", "Fence"]), &negative)
        else {
            panic!("a negative weight was not refused");
        };
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(refused.context.contains("Kerb"), "{}", refused.describe());

        let Err(missing) = edit_styles_command(
            document.model(),
            &strings(&["Kerb", "Nobody"]),
            &StyleFields::default(),
        ) else {
            panic!("a style the model lacks was not refused");
        };
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(missing.context, "Nobody");
        assert_eq!(stored(document, "Kerb").line_weight, 0.35);
    }

    #[test]
    fn drawing_linetypes_and_library_linestyles_are_one_list_and_a_collision_is_two_rows() {
        let fixture = fixture();
        let rows = linetype_rows(&fixture.document);
        // Folded order: continuous, dash, dash, dotted, ls gate, ls paper kerb,
        // ls world fence; the drawing's dash before the library's. SYM Pit is
        // `mode vertex`: a symbol, never a linestyle (D2).
        let seen: Vec<(String, LinetypeOrigin)> = rows
            .iter()
            .map(|candidate| (candidate.name.clone(), candidate.origin))
            .collect();
        let expected: Vec<(String, LinetypeOrigin)> = [
            ("continuous", LinetypeOrigin::Drawing),
            ("dash", LinetypeOrigin::Drawing),
            ("dash", LinetypeOrigin::Library),
            ("Dotted", LinetypeOrigin::Drawing),
            ("LS Gate", LinetypeOrigin::Library),
            ("LS Paper Kerb", LinetypeOrigin::Library),
            ("LS World Fence", LinetypeOrigin::Library),
        ]
        .into_iter()
        .map(|(name, origin)| (String::from(name), origin))
        .collect();
        assert_eq!(seen, expected);
        assert!(rows[1].collision);
        assert!(rows[2].collision);
        assert!(!rows[0].collision);
        assert!(!rows[5].collision);
        // Both rows of the collision answer to one name: layer kerbs names it.
        assert_eq!(rows[1].users.layers, strings(&["kerbs"]));
        assert_eq!(rows[2].users.layers, strings(&["kerbs"]));
    }

    #[test]
    fn each_linetype_row_says_what_draws_it_and_one_repeat_of_it() {
        let fixture = fixture();
        let rows = linetype_rows(&fixture.document);
        assert_eq!(rows.len(), 7);
        let continuous = row_of(&rows, "continuous", LinetypeOrigin::Drawing);
        assert_eq!(continuous.kind, LinetypeRowKind::Dash);
        assert_eq!(continuous.period, 0.0);
        // |1| + |-0.5|
        double_eq(row_of(&rows, "dash", LinetypeOrigin::Drawing).period, 1.5);
        // |0| + |-0.25|: a dot has no length
        let dotted = row_of(&rows, "Dotted", LinetypeOrigin::Drawing);
        double_eq(dotted.period, 0.25);
        assert_eq!(dotted.description, "a dot every quarter metre");

        let kerb = row_of(&rows, "LS Paper Kerb", LinetypeOrigin::Library);
        assert_eq!(kerb.kind, LinetypeRowKind::Paper);
        // The file's length, in plot millimetres.
        double_eq(kerb.period, 4.0);
        assert_eq!(kerb.group, "Roads/Kerbs");
        assert_eq!(kerb.source_file, "linestyles_test.4d");
        assert_eq!(kerb.users.styles, strings(&["Kerb"]));
        assert_eq!(kerb.users.entities, 2);

        // No length: the pen travels from x = 0 to the last move at x = 5.
        let fence = row_of(&rows, "LS World Fence", LinetypeOrigin::Library);
        assert_eq!(fence.kind, LinetypeRowKind::World);
        double_eq(fence.period, 5.0);

        let gate = row_of(&rows, "LS Gate", LinetypeOrigin::Library);
        assert_eq!(gate.kind, LinetypeRowKind::TwoPoint);
        assert_eq!(gate.period, 0.0, "stretched between two points: no repeat");
        assert!(!gate.users.used());
    }

    #[test]
    fn diagnostics_list_the_missing_names_then_the_collisions_each_with_who_uses_them() {
        let fixture = fixture();
        let diagnostics = style_diagnostics(&fixture.document);
        assert_eq!(diagnostics.len(), 3);

        assert_eq!(diagnostics[0].kind, StyleDiagnosticKind::MissingLinetype);
        assert_eq!(diagnostics[0].name, "NOPE Line");
        assert_eq!(diagnostics[0].users.styles, strings(&["Ghost"]));
        assert_eq!(diagnostics[0].users.entities, 1);
        assert_eq!(diagnostics[0].drawn_as, "a solid line (continuous)");

        // built_in_symbol_for: "manhole" is in the name, so the manhole shape.
        assert_eq!(diagnostics[1].kind, StyleDiagnosticKind::MissingSymbol);
        assert_eq!(diagnostics[1].name, "NOPE Manhole");
        assert_eq!(diagnostics[1].drawn_as, "the built-in manhole");

        assert_eq!(diagnostics[2].kind, StyleDiagnosticKind::Collision);
        assert_eq!(diagnostics[2].name, "dash");
        assert_eq!(diagnostics[2].users.layers, strings(&["kerbs"]));
    }

    #[test]
    fn select_users_gathers_the_entities_of_every_name_ascending_and_once() {
        let fixture = fixture();
        let model = fixture.document.model();
        let mut expected = fixture.kerb_points.clone();
        expected.push(fixture.ghost_point);
        expected.sort_unstable();
        assert_eq!(
            entities_using(
                model,
                UsageTable::Style,
                &strings(&["Ghost", "Kerb", "Kerb"])
            ),
            expected
        );
        assert_eq!(
            entities_using(model, UsageTable::Linetype, &strings(&["LS Paper Kerb"])),
            fixture.kerb_points
        );
        assert_eq!(
            entities_using(model, UsageTable::Symbol, &strings(&["NOPE Manhole"])),
            [fixture.ghost_point]
        );
        assert!(entities_using(model, UsageTable::Style, &strings(&["Fence"])).is_empty());
    }

    #[test]
    fn a_free_name_counts_up_from_two_and_a_linetype_name_avoids_the_library_too() {
        let mut fixture = fixture();
        let document = &mut fixture.document;
        assert_eq!(free_style_name(document.model(), "New Style"), "New Style");
        assert_eq!(free_style_name(document.model(), "Kerb"), "Kerb 2");
        must(
            document,
            create_style(style("Kerb 2", "continuous", "", 0.25)),
        );
        assert_eq!(free_style_name(document.model(), "Kerb"), "Kerb 3");

        // "LS Gate" is no drawing linetype, but the library has it: a drawing
        // linetype of that name would start as a collision.
        assert_eq!(free_linetype_name(document, "LS Gate"), "LS Gate 2");
        assert_eq!(free_linetype_name(document, "Dotted"), "Dotted 2");
        assert_eq!(free_linetype_name(document, "Hidden"), "Hidden");
    }
}
