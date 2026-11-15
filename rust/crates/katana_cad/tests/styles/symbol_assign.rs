// The symbol library's logic (tests/cad/customisation/test_symbol_assign.cpp):
// putting a symbol on points through a style (one undo step, reusing a style
// that already says exactly that), swapping one symbol for another in the
// styles, who uses a symbol, what the library lists and how big a symbol
// prints. Every expectation is worked out by hand in the comment beside it.

use katana_cad::symbol_assign::{
    TextExtent, assign_symbol_to_points, estimated_text_extent, find_symbol_style,
    replace_symbol_in_styles, symbol_codes, symbol_library, symbol_print_size, symbol_users,
};
use katana_cad::{
    DefinitionSource, Document, StyleTextMark, SymbolAssignment, SymbolCode, SymbolLibraryEntry,
    SymbolUsers,
};
use katana_commands::{
    CommandPtr, EntityAttributes, create_layer, create_line, create_point, create_style,
    set_entity_style, update_layer,
};
use katana_core::ErrorCode;
use katana_entity::display::{BY_LAYER_LINETYPE, is_by_layer};
use katana_entity::tables::built_in_symbol_for;
use katana_entity::{
    Color, EntityId, Layer, LineStyle, Stroke, StrokeOp, StrokeText, Style, StyleLibrary,
    StyleUnits, SurveyMap, SurveyRule, SurveySection, SurveySymbol,
};
use katana_geometry::{Box2, Point2};
use katana_math::numerics::PI;

use crate::{double_eq, near};

fn move_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Move,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn draw(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Draw,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

// The fixture's "TEST Survey Mark" (tests/archive12d/data/customisation/
// test_symbols.4d), written out: a cross of two 1 m strokes through the
// origin and a 0.3 m circle round it - so it covers exactly -0.5..0.5 each
// way, the circle lying inside.
fn survey_mark() -> LineStyle {
    LineStyle {
        name: String::from("TEST Survey Mark"),
        group: String::from("Test/Marks"),
        at_vertices: true,
        source: String::from("test_symbols.4d"),
        strokes: vec![
            move_to(-0.5, 0.0),
            draw(0.5, 0.0),
            move_to(0.0, -0.5),
            draw(0.0, 0.5),
            move_to(0.0, 0.0),
            Stroke {
                op: StrokeOp::Circle,
                radius: 0.3,
                ..Stroke::default()
            },
        ],
        ..LineStyle::default()
    }
}

// A linestyle (not `mode vertex`, from a file not named for symbols, named by
// nothing): D3 does not offer it as a symbol.
fn kerb_linestyle() -> LineStyle {
    LineStyle {
        name: String::from("TEST Dashed Kerb"),
        units: StyleUnits::Paper,
        source: String::from("test_linestyles.4d"),
        strokes: vec![move_to(0.0, 0.0), draw(1.5, 0.0)],
        ..LineStyle::default()
    }
}

fn find_entry<'a>(entries: &'a [SymbolLibraryEntry], name: &str) -> Option<&'a SymbolLibraryEntry> {
    entries.iter().find(|entry| entry.entry.name == name)
}

struct Fixture {
    document: Document,
    p1: EntityId,
    p2: EntityId,
    p3: EntityId,
    line: EntityId,
}

fn create(document: &mut Document, command: CommandPtr) -> EntityId {
    assert!(document.execute(command).is_ok());
    document
        .last_created_entities()
        .first()
        .copied()
        .unwrap_or_default()
}

impl Fixture {
    fn new() -> Self {
        let mut document = Document::default();
        let mut library = StyleLibrary::default();
        assert!(library.add(survey_mark()).is_ok());
        assert!(library.add(kerb_linestyle()).is_ok());
        document.set_style_library(library);
        let p1 = create(
            &mut document,
            create_point(Point2::new(0.0, 0.0), EntityAttributes::default()),
        );
        let p2 = create(
            &mut document,
            create_point(Point2::new(10.0, 0.0), EntityAttributes::default()),
        );
        let p3 = create(
            &mut document,
            create_point(Point2::new(20.0, 0.0), EntityAttributes::default()),
        );
        let line = create(
            &mut document,
            create_line(
                Point2::new(0.0, 5.0),
                Point2::new(10.0, 5.0),
                EntityAttributes::default(),
            ),
        );
        Self {
            document,
            p1,
            p2,
            p3,
            line,
        }
    }

    fn style_of(&self, id: EntityId) -> String {
        self.document
            .model()
            .entities
            .find(id)
            .map(|entity| entity.style.clone())
            .unwrap_or_default()
    }

    // Assigns and executes, returning what the assignment said.
    fn assign(&mut self, ids: &[EntityId], symbol: &str, size: f64) -> SymbolAssignment {
        let assignment = assign_symbol_to_points(&self.document, ids, symbol, size);
        assert!(assignment.is_ok());
        let Ok(mut result) = assignment else {
            return SymbolAssignment::default();
        };
        if let Some(command) = result.command.take() {
            assert!(self.document.execute(command).is_ok());
        }
        result
    }

    fn add_style(&mut self, style: Style) {
        assert!(self.document.execute(create_style(style)).is_ok());
    }

    fn style_named(&self, name: &str) -> Style {
        self.document
            .model()
            .styles
            .find(name)
            .cloned()
            .unwrap_or_default()
    }
}

mod symbol_assign {
    use super::*;

    #[test]
    fn the_first_assign_makes_a_style_named_after_the_symbol_and_moves_only_the_points() {
        // Selected: two points and a line. The line is counted, not changed;
        // the style is new (the table has none), named after the symbol, and
        // says nothing but the symbol: ByLayer colour, linetype and hatch.
        let mut fixture = Fixture::new();
        let (p1, p2, p3, line) = (fixture.p1, fixture.p2, fixture.p3, fixture.line);
        let done = fixture.assign(&[p1, p2, line], "TEST Survey Mark", 0.0);

        assert!(done.creates_style);
        assert_eq!(done.style, "TEST Survey Mark");
        assert_eq!(done.points, 2);
        assert_eq!(done.not_points, 1);
        assert_eq!(done.not_found, 0);
        assert_eq!(fixture.style_of(p1), "TEST Survey Mark");
        assert_eq!(fixture.style_of(p2), "TEST Survey Mark");
        assert_eq!(fixture.style_of(p3), "");
        assert_eq!(fixture.style_of(line), "");

        let made = fixture
            .document
            .model()
            .styles
            .find("TEST Survey Mark")
            .expect("the style is made");
        assert_eq!(made.symbol, "TEST Survey Mark");
        assert_eq!(made.symbol_size, 0.0);
        assert!(made.color.is_none());
        assert!(is_by_layer(&made.linetype));
        assert!(made.hatch_pattern.is_empty());
    }

    #[test]
    fn one_undo_takes_back_both_the_new_style_and_the_points_move() {
        // Four creations in SetUp, then the assignment: five steps, and one
        // undo goes back to four with no style and the points ByLayer again.
        let mut fixture = Fixture::new();
        let (p1, p2) = (fixture.p1, fixture.p2);
        assert_eq!(fixture.document.history().undo_count(), 4);
        fixture.assign(&[p1, p2], "TEST Survey Mark", 0.0);
        assert_eq!(fixture.document.history().undo_count(), 5);

        assert!(fixture.document.undo().is_ok());
        assert_eq!(fixture.document.history().undo_count(), 4);
        assert!(!fixture.document.model().styles.contains("TEST Survey Mark"));
        assert_eq!(fixture.style_of(p1), "");
        assert_eq!(fixture.style_of(p2), "");

        assert!(fixture.document.redo().is_ok());
        assert!(fixture.document.model().styles.contains("TEST Survey Mark"));
        assert_eq!(fixture.style_of(p1), "TEST Survey Mark");
    }

    #[test]
    fn a_second_assign_of_the_same_symbol_and_size_reuses_the_style() {
        let mut fixture = Fixture::new();
        let (p1, p3) = (fixture.p1, fixture.p3);
        fixture.assign(&[p1], "TEST Survey Mark", 0.0);
        let styles = fixture.document.model().styles.size();

        let second = fixture.assign(&[p3], "TEST Survey Mark", 0.0);

        assert!(!second.creates_style);
        assert_eq!(second.style, "TEST Survey Mark");
        assert_eq!(fixture.document.model().styles.size(), styles);
        assert_eq!(fixture.style_of(p3), "TEST Survey Mark");
    }

    #[test]
    fn another_size_is_another_style_suffixed_two_and_then_it_is_reused() {
        // 0 (own size) takes the plain name; 1.5 finds that name taken by a
        // style of a different size, so it is "... 2"; a second 1.5 reuses it.
        let mut fixture = Fixture::new();
        let (p1, p2, p3) = (fixture.p1, fixture.p2, fixture.p3);
        fixture.assign(&[p1], "TEST Survey Mark", 0.0);
        let bigger = fixture.assign(&[p2], "TEST Survey Mark", 1.5);
        assert!(bigger.creates_style);
        assert_eq!(bigger.style, "TEST Survey Mark 2");
        assert_eq!(fixture.style_named("TEST Survey Mark 2").symbol_size, 1.5);

        let again = fixture.assign(&[p3], "TEST Survey Mark", 1.5);
        assert!(!again.creates_style);
        assert_eq!(again.style, "TEST Survey Mark 2");
    }

    #[test]
    fn a_style_that_also_sets_a_colour_or_a_linetype_is_not_reused() {
        // "TEST Survey Mark" red, and "TEST Survey Mark 2" with the Style
        // default linetype "continuous" (not ByLayer): both draw the symbol at
        // size 0, neither says ONLY that. The first free name is "... 3".
        let mut fixture = Fixture::new();
        let p1 = fixture.p1;
        fixture.add_style(Style {
            name: String::from("TEST Survey Mark"),
            symbol: String::from("TEST Survey Mark"),
            linetype: String::from(BY_LAYER_LINETYPE),
            color: Some(Color::new(255, 0, 0, 255)),
            ..Style::default()
        });
        fixture.add_style(Style {
            name: String::from("TEST Survey Mark 2"),
            symbol: String::from("TEST Survey Mark"),
            ..Style::default()
        });

        let done = fixture.assign(&[p1], "TEST Survey Mark", 0.0);

        assert!(done.creates_style);
        assert_eq!(done.style, "TEST Survey Mark 3");
    }

    #[test]
    fn a_matching_style_under_another_name_is_reused_and_its_description_is_no_bar() {
        let mut fixture = Fixture::new();
        let p1 = fixture.p1;
        fixture.add_style(Style {
            name: String::from("Marks"),
            symbol: String::from("TEST Survey Mark"),
            linetype: String::from("BYLAYER"), // ByLayer in another spelling is still ByLayer
            description: String::from("control marks"),
            ..Style::default()
        });

        let done = fixture.assign(&[p1], "TEST Survey Mark", 0.0);

        assert!(!done.creates_style);
        assert_eq!(done.style, "Marks");
        assert!(!fixture.document.model().styles.contains("TEST Survey Mark"));
    }

    #[test]
    fn symbol_names_are_case_sensitive_when_a_style_is_reused() {
        // D3: "test survey mark" is another name, so its style is not reused.
        let mut fixture = Fixture::new();
        fixture.add_style(Style {
            name: String::from("lower"),
            symbol: String::from("test survey mark"),
            linetype: String::from(BY_LAYER_LINETYPE),
            ..Style::default()
        });

        let model = fixture.document.model();
        assert!(find_symbol_style(model, "TEST Survey Mark", 0.0).is_none());
        let found = find_symbol_style(model, "test survey mark", 0.0).expect("lower");
        assert!(std::ptr::eq(
            found,
            model.styles.find("lower").expect("lower exists")
        ));
    }

    #[test]
    fn points_already_in_the_style_make_no_undo_step() {
        let mut fixture = Fixture::new();
        let p1 = fixture.p1;
        fixture.assign(&[p1], "TEST Survey Mark", 0.0);
        let steps = fixture.document.history().undo_count();

        let again = assign_symbol_to_points(&fixture.document, &[p1], "TEST Survey Mark", 0.0)
            .expect("an assignment");
        assert!(again.command.is_none());
        assert_eq!(again.already_in_style, 1);
        assert_eq!(again.points, 0);
        assert_eq!(fixture.document.history().undo_count(), steps);
    }

    #[test]
    fn no_point_among_the_ids_an_empty_name_or_a_bad_size_is_refused_and_says_why() {
        let fixture = Fixture::new();
        let (p1, line) = (fixture.p1, fixture.line);
        let only_line =
            assign_symbol_to_points(&fixture.document, &[line, 9999], "TEST Survey Mark", 0.0)
                .expect_err("no point among them");
        assert_eq!(only_line.code, ErrorCode::InvalidArgument);
        // One line and one id nothing has.
        assert_eq!(
            only_line.context,
            "1 entity that is not a point, 1 id no entity has"
        );

        assert!(assign_symbol_to_points(&fixture.document, &[p1], "", 0.0).is_err());
        assert!(
            assign_symbol_to_points(&fixture.document, &[p1], "TEST Survey Mark", -1.0).is_err()
        );
        assert!(
            assign_symbol_to_points(&fixture.document, &[p1], "TEST Survey Mark", f64::NAN)
                .is_err()
        );
        assert!(assign_symbol_to_points(&fixture.document, &[], "TEST Survey Mark", 0.0).is_err());
    }

    #[test]
    fn a_point_on_a_locked_layer_refuses_the_whole_step_and_leaves_no_style() {
        let mut fixture = Fixture::new();
        let p1 = fixture.p1;
        let mut locked = Layer {
            name: String::from("locked"),
            ..Layer::default()
        };
        assert!(
            fixture
                .document
                .execute(create_layer(locked.clone()))
                .is_ok()
        );
        let held = create(
            &mut fixture.document,
            create_point(
                Point2::new(30.0, 0.0),
                EntityAttributes {
                    layer: String::from("locked"),
                    ..EntityAttributes::default()
                },
            ),
        );
        locked.locked = true;
        assert!(fixture.document.execute(update_layer(locked)).is_ok());
        let steps = fixture.document.history().undo_count();

        let mut assignment =
            assign_symbol_to_points(&fixture.document, &[p1, held], "TEST Survey Mark", 0.0)
                .expect("an assignment");
        assert!(assignment.creates_style);

        let command = assignment.command.take().expect("a command");
        assert!(fixture.document.execute(command).is_err());
        // The Transaction undid the style it had made before the move failed.
        assert!(!fixture.document.model().styles.contains("TEST Survey Mark"));
        assert_eq!(fixture.style_of(p1), "");
        assert_eq!(fixture.document.history().undo_count(), steps);
    }

    #[test]
    fn replacing_a_symbol_changes_every_style_naming_it_in_one_undo_step() {
        // A and B name the mark, C names another: two styles change, C does not.
        let mut fixture = Fixture::new();
        for (name, symbol) in [
            ("A", "TEST Survey Mark"),
            ("B", "TEST Survey Mark"),
            ("C", "cross"),
        ] {
            fixture.add_style(Style {
                name: String::from(name),
                symbol: String::from(symbol),
                symbol_size: 2.0,
                ..Style::default()
            });
        }
        let steps = fixture.document.history().undo_count();

        let replace =
            replace_symbol_in_styles(fixture.document.model(), "TEST Survey Mark", "circle")
                .expect("two styles name it");
        assert!(fixture.document.execute(replace).is_ok());
        assert_eq!(fixture.document.history().undo_count(), steps + 1);
        assert_eq!(fixture.style_named("A").symbol, "circle");
        assert_eq!(fixture.style_named("B").symbol, "circle");
        assert_eq!(fixture.style_named("C").symbol, "cross");
        // Only the symbol: the size each style gave is kept.
        assert_eq!(fixture.style_named("A").symbol_size, 2.0);

        assert!(fixture.document.undo().is_ok());
        assert_eq!(fixture.style_named("A").symbol, "TEST Survey Mark");
        assert_eq!(fixture.style_named("B").symbol, "TEST Survey Mark");
    }

    #[test]
    fn replacing_a_name_no_style_uses_gives_no_command() {
        let mut fixture = Fixture::new();
        fixture.add_style(Style {
            name: String::from("A"),
            symbol: String::from("cross"),
            ..Style::default()
        });

        let model = fixture.document.model();
        assert!(replace_symbol_in_styles(model, "TEST Survey Mark", "circle").is_none());
        assert!(replace_symbol_in_styles(model, "cross", "cross").is_none());
        // "" is "no symbol", not a name: styles without one are not rewritten.
        assert!(replace_symbol_in_styles(model, "", "cross").is_none());
    }

    #[test]
    fn the_users_of_a_symbol_are_its_styles_and_the_points_wearing_them_lines_counted_apart() {
        // Style "Marks" draws the mark; p1, p3 and the line wear it; p2 wears
        // nothing. Points ascending: p1 then p3; the line is the one other.
        let mut fixture = Fixture::new();
        let (p1, p3, line) = (fixture.p1, fixture.p3, fixture.line);
        fixture.add_style(Style {
            name: String::from("Marks"),
            symbol: String::from("TEST Survey Mark"),
            ..Style::default()
        });
        assert!(
            fixture
                .document
                .execute(set_entity_style(vec![p3, p1, line], "Marks"))
                .is_ok()
        );

        let users = symbol_users(&fixture.document, "TEST Survey Mark");

        assert_eq!(users.styles, vec![String::from("Marks")]);
        assert_eq!(users.points, vec![p1, p3]);
        assert_eq!(users.other_entities, 1);
        assert_eq!(
            symbol_users(&fixture.document, "cross"),
            SymbolUsers::default()
        );
        assert_eq!(symbol_users(&fixture.document, ""), SymbolUsers::default());
    }

    #[test]
    fn the_codes_naming_a_symbol_are_its_vertex_symbol_rules_with_size_and_colour() {
        // Two rules draw the mark (AC* at 1.5 white, SM* at its own size in the
        // string's colour); a map_data rule with that linestyle is not a symbol
        // rule and is not listed.
        let mut map = SurveyMap::default();
        let ac = SurveyRule {
            key: String::from("AC*"),
            section: SurveySection::VertexSymbol,
            symbol: Some(SurveySymbol {
                style: String::from("TEST Survey Mark"),
                colour: String::from("white"),
                size: 1.5,
                ..SurveySymbol::default()
            }),
            ..SurveyRule::default()
        };
        assert!(map.add(ac).is_ok());
        let linework = SurveyRule {
            key: String::from("LN*"),
            linestyle: String::from("TEST Survey Mark"),
            ..SurveyRule::default()
        };
        assert!(map.add(linework).is_ok());
        let sm = SurveyRule {
            key: String::from("SM*"),
            section: SurveySection::VertexSymbol,
            symbol: Some(SurveySymbol {
                style: String::from("TEST Survey Mark"),
                ..SurveySymbol::default()
            }),
            ..SurveyRule::default()
        };
        assert!(map.add(sm).is_ok());

        let codes = symbol_codes(&map, "TEST Survey Mark");

        assert_eq!(codes.len(), 2);
        assert_eq!(
            codes[0],
            SymbolCode {
                key: String::from("AC*"),
                size: 1.5,
                colour: String::from("white"),
                comment: String::new(),
            }
        );
        assert_eq!(
            codes[1],
            SymbolCode {
                key: String::from("SM*"),
                size: 0.0,
                colour: String::new(),
                comment: String::new(),
            }
        );
        assert!(symbol_codes(&map, "test survey mark").is_empty());
    }

    #[test]
    fn the_library_lists_d3s_symbols_the_built_ins_and_every_undefined_name_with_its_fallback() {
        // A style names "Old Pit" (defined nowhere) and a survey code names
        // "Old Valve" (defined nowhere, worn by no style). Both are listed,
        // marked missing, each with built_in_symbol_for's shape; the kerb
        // linestyle is not a symbol under D3 and is not listed.
        let mut fixture = Fixture::new();
        fixture.add_style(Style {
            name: String::from("Old"),
            symbol: String::from("Old Pit"),
            ..Style::default()
        });
        let mut map = SurveyMap::default();
        let valve = SurveyRule {
            key: String::from("VV*"),
            section: SurveySection::VertexSymbol,
            symbol: Some(SurveySymbol {
                style: String::from("Old Valve"),
                size: 0.8,
                ..SurveySymbol::default()
            }),
            ..SurveyRule::default()
        };
        assert!(map.add(valve).is_ok());
        fixture.document.set_survey_map(map);

        let entries = symbol_library(&fixture.document);
        let find = |name: &str| find_entry(&entries, name);

        let mark = find("TEST Survey Mark").expect("the mark is listed");
        assert_eq!(mark.entry.source, DefinitionSource::Library);
        assert!(find("TEST Dashed Kerb").is_none());
        let cross = find("cross").expect("cross is listed");
        assert_eq!(cross.entry.source, DefinitionSource::BuiltIn);

        let pit = find("Old Pit").expect("Old Pit is listed");
        assert!(pit.entry.missing);
        assert_eq!(pit.fallback, built_in_symbol_for("Old Pit"));
        assert_eq!(pit.entry.users.styles, vec![String::from("Old")]);

        let valve_entry = find("Old Valve").expect("Old Valve is listed");
        assert!(valve_entry.entry.missing);
        assert_eq!(valve_entry.fallback, built_in_symbol_for("Old Valve"));
        assert_eq!(valve_entry.codes.len(), 1);
        assert_eq!(valve_entry.codes[0].key, "VV*");
        assert_eq!(valve_entry.codes[0].size, 0.8);

        // Sorted with case folded: every neighbour pair is in order.
        for (i, pair) in entries.windows(2).enumerate() {
            assert!(
                pair[0].entry.name.to_ascii_lowercase() <= pair[1].entry.name.to_ascii_lowercase(),
                "at {}",
                i + 1
            );
        }
    }
}

// The fixture's "TEST Valve" written out: a 0.8 m box about the insertion
// point, then `move 0 0.6` and a 0.5 m "V", bottom-centre, 0.8 wide factor.
fn valve() -> LineStyle {
    LineStyle {
        name: String::from("TEST Valve"),
        at_vertices: true,
        strokes: vec![
            move_to(-0.4, -0.4),
            draw(0.4, -0.4),
            draw(0.4, 0.4),
            draw(-0.4, 0.4),
            draw(-0.4, -0.4),
            move_to(0.0, 0.6),
            Stroke {
                op: StrokeOp::Text,
                text: 0,
                ..Stroke::default()
            },
        ],
        texts: vec![StrokeText {
            text: String::from("V"),
            height: 0.5,
            justify: String::from("bottom-centre"),
            font: String::from("Arial"),
            width_factor: 0.8,
            ..StrokeText::default()
        }],
        ..LineStyle::default()
    }
}

// `mode vertex; move 0 0; text "H" 0 1`: a symbol that is only a letter,
// 1 m high, with no justification.
fn letter_only() -> LineStyle {
    LineStyle {
        name: String::from("letter"),
        at_vertices: true,
        strokes: vec![
            move_to(0.0, 0.0),
            Stroke {
                op: StrokeOp::Text,
                text: 0,
                ..Stroke::default()
            },
        ],
        texts: vec![StrokeText {
            text: String::from("H"),
            height: 1.0,
            ..StrokeText::default()
        }],
        ..LineStyle::default()
    }
}

mod symbol_print_size {
    use super::*;

    #[test]
    fn a_world_symbol_prints_its_ground_size_divided_by_the_scale() {
        // The mark covers 1 m x 1 m on the ground. At 1:500 a plot millimetre
        // is 0.5 m, so it prints 1 / 0.5 = 2 mm each way; its insertion point
        // (the crossing) is inside it.
        let printed = symbol_print_size(&survey_mark(), 0.0, 500.0, None).expect("a size");
        double_eq(printed.ground_width, 1.0);
        double_eq(printed.ground_height, 1.0);
        double_eq(printed.paper_width, 2.0);
        double_eq(printed.paper_height, 2.0);
        assert!(printed.insertion_inside);
    }

    #[test]
    fn a_size_makes_the_larger_side_span_it() {
        // Size 1.5 m over a 1 m definition: 1.5 m each way, 3 mm at 1:500 and
        // 15 mm at 1:100.
        let at500 = symbol_print_size(&survey_mark(), 1.5, 500.0, None).expect("a size");
        double_eq(at500.ground_width, 1.5);
        double_eq(at500.paper_width, 3.0);
        let at100 = symbol_print_size(&survey_mark(), 1.5, 100.0, None).expect("a size");
        double_eq(at100.paper_width, 15.0);
    }

    #[test]
    fn a_paper_symbol_prints_its_own_millimetres_at_every_scale() {
        // A paperstyle 2 mm wide and 1 mm high: 2 x 1 mm on the plot at any
        // scale; on the ground 2 x 0.25 = 0.5 m by 0.25 m at 1:250, and 2 x 1 =
        // 2 m by 1 m at 1:1000.
        let tick = LineStyle {
            name: String::from("tick"),
            units: StyleUnits::Paper,
            strokes: vec![move_to(0.0, 0.0), draw(2.0, 0.0), draw(2.0, 1.0)],
            ..LineStyle::default()
        };

        let at250 = symbol_print_size(&tick, 0.0, 250.0, None).expect("a size");
        double_eq(at250.paper_width, 2.0);
        double_eq(at250.paper_height, 1.0);
        double_eq(at250.ground_width, 0.5);
        double_eq(at250.ground_height, 0.25);
        let at1000 = symbol_print_size(&tick, 0.0, 1000.0, None).expect("a size");
        double_eq(at1000.paper_width, 2.0);
        double_eq(at1000.ground_width, 2.0);
    }

    #[test]
    fn a_symbol_drawn_away_from_its_insertion_point_says_so() {
        // Strokes from (5, 5) to (6, 6): the point it is put on, (0, 0), is
        // outside the 1 x 1 box it draws.
        let away = LineStyle {
            name: String::from("away"),
            strokes: vec![move_to(5.0, 5.0), draw(6.0, 5.0), draw(6.0, 6.0)],
            ..LineStyle::default()
        };

        let printed = symbol_print_size(&away, 0.0, 500.0, None).expect("a size");
        double_eq(printed.ground_width, 1.0);
        assert!(!printed.insertion_inside);
    }

    #[test]
    fn a_valves_letter_above_its_box_is_part_of_what_prints() {
        // The box spans -0.4..0.4 each way. The V stands on (0, 0.6), 0.5 m
        // high, so it reaches y = 1.1; it is 0.6 x 0.5 x 0.8 = 0.24 m wide,
        // centred on x = 0 (-0.12..0.12), inside the box's width. So the valve
        // covers 0.8 m by -0.4..1.1 = 1.5 m, which at 1:500 (0.5 m a plot
        // millimetre) prints 1.6 x 3 mm. Measuring the V's anchor alone gave
        // 0.8 x 1 m, 1.6 x 2 mm.
        let printed = symbol_print_size(&valve(), 0.0, 500.0, None).expect("a size");
        double_eq(printed.ground_width, 0.8);
        double_eq(printed.ground_height, 1.5);
        double_eq(printed.paper_width, 1.6);
        double_eq(printed.paper_height, 3.0);
        assert!(printed.insertion_inside);
    }

    #[test]
    fn a_symbol_that_is_only_a_letter_prints_the_letters_size() {
        // "H" stands on (0, 0) from its left: 0.6 x 1 x 1 = 0.6 m wide and 1 m
        // high, so 0.6 / 0.5 = 1.2 by 1 / 0.5 = 2 mm at 1:500 - not 0 x 0.
        let own = symbol_print_size(&letter_only(), 0.0, 500.0, None).expect("a size");
        double_eq(own.ground_width, 0.6);
        double_eq(own.ground_height, 1.0);
        double_eq(own.paper_width, 1.2);
        double_eq(own.paper_height, 2.0);
        // A size is a width of LineStyle::bounds(), which for a lone text is
        // its anchor: no width, so symbol_drawing keeps the definition's own
        // scale rather than dividing by zero - and it prints the same at size 2.
        let sized = symbol_print_size(&letter_only(), 2.0, 500.0, None).expect("a size");
        double_eq(sized.paper_width, 1.2);
        double_eq(sized.paper_height, 2.0);
    }

    #[test]
    fn an_estimated_text_follows_its_justification_width_factor_and_angle() {
        let mut mark = StyleTextMark {
            at: Point2::new(10.0, 20.0),
            text: String::from("12"),
            height: 0.5,
            ..StyleTextMark::default()
        };

        // Two characters 0.6 x 0.5 = 0.3 m each: 0.6 m, ending at the anchor
        // ("right"), and centred on it upright ("middle"): x 9.4..10, y
        // 19.75..20.25.
        mark.justify = String::from("middle-right");
        let found = estimated_text_extent(&mark);
        double_eq(found.min.x, 9.4);
        double_eq(found.max.x, 10.0);
        double_eq(found.min.y, 19.75);
        double_eq(found.max.y, 20.25);

        // Half as wide (0.3 m), centred ("centre"), hanging from it ("top"):
        // x 9.85..10.15, y 19.5..20.
        mark.justify = String::from("top-centre");
        mark.width_factor = 0.5;
        let found = estimated_text_extent(&mark);
        double_eq(found.min.x, 9.85);
        double_eq(found.max.x, 10.15);
        double_eq(found.min.y, 19.5);
        double_eq(found.max.y, 20.0);

        // Unjustified and turned a quarter anticlockwise: its 0.3 m run goes
        // up from the anchor (y 20..20.3) and its 0.5 m height leftwards (x
        // 9.5..10).
        mark.justify.clear();
        mark.angle = PI / 2.0;
        let found = estimated_text_extent(&mark);
        near(found.min.x, 9.5, 1e-12);
        near(found.max.x, 10.0, 1e-12);
        near(found.min.y, 20.0, 1e-12);
        near(found.max.y, 20.3, 1e-12);

        // Nothing to print: no box, not a point at the anchor.
        mark.text.clear();
        assert!(estimated_text_extent(&mark).empty());
        mark.text = String::from("12");
        mark.height = 0.0;
        assert!(estimated_text_extent(&mark).empty());
    }

    #[test]
    fn a_text_extent_given_by_the_caller_replaces_the_estimate() {
        // A font that sets the V 0.2 m wide and 0.6 m tall above (0, 0.6) - a
        // stand-in for a measured face - makes the valve -0.4..1.2: 1.6 m, or
        // 3.2 mm at 1:500.
        let measured = |text: &StyleTextMark| {
            Box2::new(
                Point2::new(text.at.x - 0.1, text.at.y),
                Point2::new(text.at.x + 0.1, text.at.y + 0.6),
            )
        };
        let extent: &TextExtent = &measured;
        let printed = symbol_print_size(&valve(), 0.0, 500.0, Some(extent)).expect("a size");
        double_eq(printed.ground_height, 1.6);
        double_eq(printed.paper_height, 3.2);
        double_eq(printed.paper_width, 1.6);
    }

    #[test]
    fn nothing_to_measure_or_no_scale_gives_no_size() {
        let empty = LineStyle {
            name: String::from("empty"),
            ..LineStyle::default()
        };
        assert!(symbol_print_size(&empty, 0.0, 500.0, None).is_none());
        assert!(symbol_print_size(&survey_mark(), 0.0, 0.0, None).is_none());
        assert!(symbol_print_size(&survey_mark(), -1.0, 500.0, None).is_none());
    }
}
