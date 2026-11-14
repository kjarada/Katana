//! The twins of tests/qt_widgets/customisation/test_symbol_library.cpp: the symbol library over
//! the committed hand-written customisation fixture (tests/data/customisation), loaded into the
//! Document as a person's would be. Every count and every printed size below is worked out by
//! hand from those three files, in the comment beside it.
//!
//! The fixture, as far as these tests need it:
//!
//! ```text
//!   test_symbols     listed as SYMBOLS: TEST Survey Mark (at vertices, Test/Marks), TEST Tree
//!                    (NOT at vertices, Test/Vegetation, one pen), TEST Valve (at vertices,
//!                    Test/Marks, one text), TEST U Turn (at vertices, Test/Marks)
//!   test_linestyles  listed as LINESTYLES: TEST Dashed Kerb, TEST Water Main, TEST Gate, none at
//!                    vertices
//!   test_survey      symbol rules AC* -> TEST Survey Mark (1.5, white), TR* -> TEST Tree (3,
//!                    green), PX* -> "TEST Missing Symbol", which no library defines
//! ```
//!
//! What the C++ did to the grid's selection model is done by the dialog's own calls: a symbol
//! added to the selection (add_to_selection) and a click with the command key (ctrl_click). The
//! picker beside Replace in Styles is the seam's (codes_seams), set by set_replacement and
//! type_replacement.
//!
//! Left for the modules they wait for: ThePicturesAndThePlanDrawAPenInTheColourTheSessions-
//! CustomisationGivesIt (the style preview's picture, CU1's style_preview),
//! ALegendSampleDrawsAPenInTheColourItsContextsColoursGiveIt (SH1's legend_painter) and
//! ASheetsLegendDrawsAPenInTheColourThePlansSourceGivesIt (SH1's sheet_painter).

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use katana_cad::view_transform::ViewTransform;
use katana_cad::{CustomisationOrigin, Document};
use katana_commands::{
    EntityAttributes, create_line, create_point, create_style, set_entity_style,
};
use katana_entity::customisation::customisation_from_json;
use katana_entity::tables::symbol_names;
use katana_entity::{
    Color, ColourTable, CustomisationSourceNote, EntityId, LineStyle, Model, Stroke, StrokeOp,
    StrokeText, Style,
};
use katana_geometry::Point2;
use katana_storage::{ProjectMetadata, ProjectStore, capture_model};
use katana_window::canvas::{Canvas, Color as CanvasColor, Image, Pen, PenCap, PenJoin, PenStyle};
use katana_window::customisation::codes_seams::{
    BeginCommit, CodesContext, Committed, SelectAndShow,
};
use katana_window::customisation::style_painter::{
    CanvasRasterBackend, RasterBackend, style_pen_for,
};
use katana_window::customisation::symbol_library::SymbolLibraryDialog;
use katana_window::customisation::symbol_library_model::SymbolGridModel;
use katana_window::host::{Host, RecordingHost};
use katana_window::plan_painter::{
    PlanFrame, PlanPaintCache, PlanPaintOptions, PlanPaintStats, paint_plan, plan_source_of,
};

use crate::fixture_customisation::{
    customisation_fixture_directory, customisation_fixture_file, install_customisation_fixtures,
    install_noticed_session, quoted_file,
};
use crate::support::{Bench, thumbnails};
use crate::window_support::{Scratch, interpreter_host};

type Library = Bench<SymbolLibraryDialog>;

/// The fixtures by name, in the order they are loaded: the session is named after the first,
/// "test_symbols" unless a test says otherwise.
const FIXTURES: [&str; 3] = ["test_symbols", "test_linestyles", "test_survey"];

fn fixture_host(fixtures: &[&str]) -> RecordingHost {
    let mut document = Document::new(None);
    install_customisation_fixtures(&mut document, fixtures);
    interpreter_host(document)
}

fn context() -> CodesContext {
    CodesContext {
        thumbnails: Some(thumbnails()),
        ..CodesContext::default()
    }
}

fn library_with(host: RecordingHost, context: CodesContext) -> Library {
    Bench::new(host, |document| SymbolLibraryDialog::new(document, context))
}

fn library(host: RecordingHost) -> Library {
    library_with(host, context())
}

fn point(host: &mut RecordingHost, x: f64, y: f64) -> EntityId {
    host.document_mut()
        .execute(create_point(Point2::new(x, y), EntityAttributes::default()))
        .expect("the point is made");
    host.document().last_created_entities()[0]
}

fn select(bench: &mut Library, ids: Vec<EntityId>) {
    bench.document_mut().selection_mut().set(ids);
    bench.document().notify_selection_changed();
    bench.draw();
}

fn select_symbol(bench: &mut Library, name: &str) -> bool {
    let selected = bench.dialog.select_symbol(&mut bench.host, name);
    bench.draw();
    selected
}

fn style(host: &mut RecordingHost, name: &str, symbol: &str) {
    host.document_mut()
        .execute(create_style(Style {
            name: String::from(name),
            symbol: String::from(symbol),
            ..Style::default()
        }))
        .expect("the style is made");
}

fn add_to_library(document: &mut Document, definition: LineStyle) {
    let mut library = document.style_library().clone();
    library.add(definition).expect("the definition is added");
    document.set_style_library(library);
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| String::from(*line)).collect()
}

fn read_bytes(path: &Path) -> String {
    String::from_utf8(fs::read(path).expect("the file is read")).expect("the file is UTF-8")
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

fn source(name: &str, definitions: bool, rules: bool, notice: &[&str]) -> CustomisationSourceNote {
    CustomisationSourceNote {
        name: String::from(name),
        definitions,
        rules,
        notice: strings(notice),
    }
}

/// The group tree's items as "label" paths, depth-first, for one comparison.
fn tree_paths(bench: &Library) -> Vec<String> {
    let mut above: Vec<String> = Vec::new();
    let mut paths = Vec::new();
    for row in bench.dialog.group_rows() {
        above.truncate(row.depth);
        above.push(row.label.clone());
        paths.push(above.join(" > "));
    }
    paths
}

/// "a × b mm at 1:500, c × d m on the ground", as its four numbers' texts.
fn printed_at_500(text: &str) -> Option<[String; 4]> {
    let (paper, rest) = text.split_once(" mm at 1:500, ")?;
    let ground = rest.strip_suffix(" m on the ground")?;
    let (paper_width, paper_height) = paper.split_once(" \u{d7} ")?;
    let (ground_width, ground_height) = ground.split_once(" \u{d7} ")?;
    Some([
        String::from(paper_width),
        String::from(paper_height),
        String::from(ground_width),
        String::from(ground_height),
    ])
}

fn number(text: &str) -> f64 {
    text.parse()
        .unwrap_or_else(|_| panic!("{text} is a number"))
}

/// Ink of the colour 128 0 128, or of that colour faded into a dark ground as an antialiased edge
/// is: red and blue there and near equal, green under a quarter of them. An entity's own pen is a
/// grey or a white - red, green and blue equal - so none of its pixels is one of these, on any
/// ground.
fn holds_purple(image: &Image) -> bool {
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.pixel(x, y);
            if pixel.r >= 60
                && 4 * u32::from(pixel.g) <= u32::from(pixel.r)
                && (i32::from(pixel.r) - i32::from(pixel.b)).abs() <= 12
            {
                return true;
            }
        }
    }
    false
}

/// A ring 1 m across whose one pen is a colour only a customisation defines.
fn purple_ring() -> LineStyle {
    LineStyle {
        name: String::from("TEST Purple Ring"),
        at_vertices: true,
        symbol: true,
        strokes: vec![
            Stroke {
                op: StrokeOp::Pen,
                pen: String::from("sui test purple"),
                ..Stroke::default()
            },
            Stroke {
                op: StrokeOp::Move,
                point: Point2::new(0.0, 0.0),
                ..Stroke::default()
            },
            Stroke {
                op: StrokeOp::Circle,
                radius: 0.5,
                ..Stroke::default()
            },
        ],
        ..LineStyle::default()
    }
}

fn purple() -> ColourTable {
    let mut colours = ColourTable::new();
    colours
        .add("sui test purple", rgb(128, 0, 128))
        .expect("the colour is added");
    colours
}

/// The file a session of four customisations, each with its author's notice, exports two of its
/// symbols to (fixture_customisation, install_noticed_session) - written out by hand from
/// docs/customisation.md, "Layout". The session's own name, description and notice, and after it
/// the notice of the one source the file leaves out; of its four sources the two the symbols came
/// from, each with its notice, and the table of colours, because the file carries one of its
/// colours; the colour a pen names and not the other; and the two symbols, in name order, the one
/// that is not the session's own saying where it is from. No rules, nothing of the linework codes,
/// and not what the session is based on.
const NOTICED_SYMBOLS: &str = concat!(
    "{\n",
    "  \"format\": \"katana-customisation\",\n",
    "  \"version\": 1,\n",
    "  \"name\": \"base\",\n",
    "  \"description\": \"The base set.\",\n",
    "  \"notice\": [\n",
    "    \"Base: all rights reserved.\",\n",
    "    \"Client codes, for this job only.\"\n",
    "  ],\n",
    "  \"sources\": [\n",
    "    {\"name\": \"base\", \"definitions\": true},\n",
    "    {\"name\": \"marks\", \"definitions\": true, \"notice\": [\"Marks drawn by hand.\"]},\n",
    "    {\"name\": \"tints\", \"notice\": [\"Tints: free to use.\"]}\n",
    "  ],\n",
    "  \"colours\": {\n",
    "    \"tint teal\": \"#008080\"\n",
    "  },\n",
    "  \"symbols\": [\n",
    "    {\"name\": \"BASE Peg\", \"atVertices\": true, \"strokes\": [\n",
    "      [\"pen\", \"tint teal\"],\n",
    "      [\"circle\", 0.5]\n",
    "    ]},\n",
    "    {\"name\": \"MARK Cross\", \"atVertices\": true, \"from\": \"marks\", \"strokes\": [\n",
    "      [\"move\", -0.5, 0],\n",
    "      [\"draw\", 0.5, 0]\n",
    "    ]}\n",
    "  ]\n",
    "}\n"
);

mod symbol_library {
    use super::*;

    #[test]
    fn the_grid_lists_the_fixtures_symbols_and_the_one_not_at_vertices_that_a_code_draws_as_one()
    {
        let mut bench = library(fixture_host(&FIXTURES));

        // 4 library symbols (D3: three at vertices, and TEST Tree, which its customisation lists
        // as a symbol and TR* draws as one), the 16 built-in shapes (none hidden: the library
        // defines none of their names), and PX*'s undefined name: 21, sorted with case folded. No
        // linestyle of test_linestyles.
        assert_eq!(symbol_names().len(), 16);
        let expected = strings(&[
            "arrow",
            "circle",
            "cross",
            "diamond",
            "dot",
            "flag",
            "manhole",
            "plus",
            "pole",
            "ring",
            "square",
            "star",
            "target",
            "TEST Missing Symbol",
            "TEST Survey Mark",
            "TEST Tree",
            "TEST U Turn",
            "TEST Valve",
            "tick",
            "tree",
            "triangle",
        ]);
        assert_eq!(bench.dialog.shown_names(), expected);

        // The grid: pictures 64 pixels across, a row a symbol.
        assert_eq!(SymbolGridModel::PICTURE_SIZE, 64);
        assert_eq!(bench.rows("symbolGrid"), 21);
        assert_eq!(bench.dialog.grid_model().row_count(), 21);
        assert_eq!(bench.text("symbolCount"), "21 of 21 symbols");

        // The tree: Test/Marks holds Survey Mark, Valve and U Turn (3), Test/Vegetation the tree
        // (1), so Test 4; nothing is ungrouped.
        assert_eq!(
            tree_paths(&bench),
            strings(&[
                "All symbols (21)",
                "Built-in (16)",
                "Test (4)",
                "Test (4) > Marks (3)",
                "Test (4) > Vegetation (1)",
                "Not defined (1)",
            ])
        );

        // Choosing Vegetation leaves the symbol that is not at vertices alone, and its details
        // say why it is a symbol at all.
        bench.fill("symbolGroups", "Vegetation (1)");
        assert_eq!(bench.dialog.shown_names(), strings(&["TEST Tree"]));
        assert!(select_symbol(&mut bench, "TEST Tree"));
        assert_eq!(
            bench.text("detailMode"),
            "along a line; a symbol because a survey code draws it as one, its customisation \
             lists it as a symbol"
        );
        // move, circle, move, draw and one pen.
        assert_eq!(
            bench.text("detailContent"),
            "4 strokes, 0 texts, 1 pen change"
        );
        assert_eq!(bench.text("detailCodes"), "TR* (size 3, green)");
        assert_eq!(bench.text("detailGroup"), "Test/Vegetation");
        // The customisation it came from, by name.
        assert_eq!(bench.text("detailSource"), "test_symbols");

        // The search reaches a survey code: only the mark is drawn by AC*.
        bench.fill("symbolGroups", "All symbols (21)");
        bench.fill("filterText", "ac*");
        assert_eq!(bench.dialog.shown_names(), strings(&["TEST Survey Mark"]));

        // The At vertices chip: the three definitions drawn at vertices.
        bench.fill("filterText", "");
        bench.press("filterVertexmode");
        assert_eq!(
            bench.dialog.shown_names(),
            strings(&["TEST Survey Mark", "TEST U Turn", "TEST Valve"])
        );
        // The chip says what it lists, with the count of those three.
        assert_eq!(bench.text("filterVertexmode"), "At vertices (3)");
    }

    #[test]
    fn the_missing_chip_lists_a_styles_undefined_symbol_with_its_fallback() {
        let mut host = fixture_host(&FIXTURES);
        style(&mut host, "Old Pits", "OLD Pit Lid");
        let mut bench = library(host);

        // Missing: the style's "OLD Pit Lid" and PX*'s "TEST Missing Symbol", folded order
        // ("old..." < "test...").
        bench.press("filterMissing");
        assert_eq!(
            bench.dialog.shown_names(),
            strings(&["OLD Pit Lid", "TEST Missing Symbol"])
        );
        assert_eq!(bench.text("filterMissing"), "Missing (2)");

        // Its stand-in, by built_in_symbol_for's words: "old pit lid" has "pit", so a manhole;
        // "test missing symbol" has none of them, so a circle.
        let pit_tip = bench
            .dialog
            .grid_model()
            .tool_tip(0)
            .expect("the first row has a tip");
        assert!(
            pit_tip.contains("Not defined - drawn as the built-in \"manhole\""),
            "{pit_tip}"
        );
        assert_eq!(bench.dialog.grid_model().is_missing(0), Some(true));

        assert!(select_symbol(&mut bench, "OLD Pit Lid"));
        assert_eq!(
            bench.text("detailMissing"),
            "Not defined - drawn as the built-in \"manhole\" until a library defines it"
        );
        assert_eq!(
            bench.text("detailStyles"),
            "Old Pits - 0 entities in the drawing"
        );
        assert!(select_symbol(&mut bench, "TEST Missing Symbol"));
        assert!(bench.text("detailMissing").contains("\"circle\""));
        assert_eq!(bench.text("detailCodes"), "PX* (size 1, yellow)");
    }

    #[test]
    fn assign_to_selected_points_is_one_undo_step_and_a_second_assign_reuses_the_style() {
        let mut host = fixture_host(&FIXTURES);
        let p1 = point(&mut host, 0.0, 0.0);
        let p2 = point(&mut host, 10.0, 0.0);
        let p3 = point(&mut host, 20.0, 0.0);
        assert!(
            host.document_mut()
                .execute(create_line(
                    Point2::new(0.0, 5.0),
                    Point2::new(9.0, 5.0),
                    EntityAttributes::default()
                ))
                .is_ok()
        );
        let line = host.document().last_created_entities()[0];
        let mut bench = library(host);
        assert!(select_symbol(&mut bench, "TEST Survey Mark"));
        select(&mut bench, vec![p1, p2, line]);

        assert!(bench.enabled("assignToPoints"));
        let steps = bench.document().history().undo_count();
        bench.press("assignToPoints");

        // One step: the style made and the two points moved; the line left.
        assert_eq!(bench.document().history().undo_count(), steps + 1);
        let style_of = |bench: &Library, id: EntityId| -> String {
            bench
                .document()
                .model()
                .entities
                .find(id)
                .map(|entity| entity.style.clone())
                .unwrap_or_default()
        };
        assert_eq!(style_of(&bench, p1), "TEST Survey Mark");
        assert_eq!(style_of(&bench, p2), "TEST Survey Mark");
        assert_eq!(style_of(&bench, line), "");
        assert!(bench.logged_exactly(
            "2 points now in style \"TEST Survey Mark\" (new), drawing \"TEST Survey Mark\". 1 \
             selected entity is not a point and left as it was."
        ));

        assert!(bench.document_mut().undo().is_ok());
        assert!(
            !bench
                .document()
                .model()
                .styles
                .contains("TEST Survey Mark")
        );
        assert_eq!(style_of(&bench, p1), "");
        assert!(bench.document_mut().redo().is_ok());

        // The second assign, to p3, finds the style and makes none.
        bench.draw();
        select(&mut bench, vec![p3]);
        let styles = bench.document().model().styles.size();
        let before = bench.document().history().undo_count();
        assert!(bench.enabled("assignToPoints"));
        bench.press("assignToPoints");
        assert_eq!(bench.document().history().undo_count(), before + 1);
        assert_eq!(bench.document().model().styles.size(), styles);
        assert_eq!(style_of(&bench, p3), "TEST Survey Mark");
        assert!(bench.logged_exactly(
            "1 point now in style \"TEST Survey Mark\", drawing \"TEST Survey Mark\"."
        ));

        // And the grid's badge counts the three points drawing it.
        bench.draw();
        let row = bench
            .dialog
            .grid_model()
            .row_of("TEST Survey Mark")
            .expect("the mark is shown");
        assert_eq!(bench.dialog.grid_model().uses(row), Some(3));
    }

    #[test]
    fn the_details_pane_states_the_survey_marks_print_size_worked_by_hand_at1_to500() {
        let mut bench = library(fixture_host(&FIXTURES));
        assert!(select_symbol(&mut bench, "TEST Survey Mark"));
        bench.fill("plotScale", "1:500");

        // A world-units cross of 1 m strokes (-0.5..0.5 each way) with a 0.3 m circle inside: 1 x
        // 1 m on the ground. At 1:500 a plot millimetre is 0.5 m, so it prints 1 / 0.5 = 2 mm
        // each way.
        assert_eq!(
            bench.text("detailPrint"),
            "2 \u{d7} 2 mm at 1:500, 1 \u{d7} 1 m on the ground"
        );
        assert_eq!(bench.text("detailExtent"), "1 \u{d7} 1 m");
        assert_eq!(bench.text("detailUnits"), "World: metres on the ground");
        assert_eq!(
            bench.text("detailMode"),
            "at vertices: drawn at each vertex of a string"
        );
        assert_eq!(bench.text("detailOrigin"), "(0, 0)");
        // move, draw, move, draw, move, circle.
        assert_eq!(
            bench.text("detailContent"),
            "6 strokes, 0 texts, 0 pen changes"
        );
        assert_eq!(bench.text("detailCodes"), "AC* (size 1.5, white)");

        // At AC*'s size, 1.5 m: 1.5 / 0.5 = 3 mm at 1:500.
        bench.fill("assignSize", "1.5");
        assert_eq!(
            bench.text("detailPrint"),
            "3 \u{d7} 3 mm at 1:500, 1.5 \u{d7} 1.5 m on the ground"
        );

        // At its own size and 1:100 (0.1 m per plot millimetre): 10 mm.
        bench.fill("assignSize", "0");
        bench.fill("plotScale", "1:100");
        assert_eq!(
            bench.text("detailPrint"),
            "10 \u{d7} 10 mm at 1:100, 1 \u{d7} 1 m on the ground"
        );
        assert_eq!(bench.dialog.preview().scale_denominator(), 100);
    }

    #[test]
    fn the_details_pane_counts_a_symbols_letters_in_what_it_prints() {
        let mut host = fixture_host(&FIXTURES);
        // A symbol that is nothing but a 1 m "H" standing on its insertion point.
        add_to_library(
            host.document_mut(),
            LineStyle {
                name: String::from("TEST Letter"),
                at_vertices: true,
                strokes: vec![
                    Stroke {
                        op: StrokeOp::Move,
                        ..Stroke::default()
                    },
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
            },
        );

        let mut bench = library(host);
        bench.fill("plotScale", "1:500");
        assert!(select_symbol(&mut bench, "TEST Valve"));

        // TEST Valve: a 0.8 m box (-0.4..0.4 each way), then a 0.5 m "V" standing bottom-centre on
        // (0, 0.6), in the preview's own face. Its width is the box's: a V is narrower than the
        // size it is set at, so at 0.8 wide it is under 0.5 x 0.8 = 0.4 m, within -0.2..0.2. Its
        // top is 0.6 + 0.5 x the face's ascent (as a share of that size), and every face's ascent
        // lies between 0.7 and 1.25 of it (Arial 0.905, Segoe UI 1.079), so the valve is 1 + 0.5 x
        // 0.7 = 1.35 to 1 + 0.5 x 1.25 = 1.625 m high: at 1:500 (0.5 m a plot millimetre), 2.7 to
        // 3.25 mm. Its anchor alone made it 1 m, 2 mm.
        let printed = bench.text("detailPrint");
        let valve = printed_at_500(&printed).unwrap_or_else(|| panic!("{printed}"));
        assert_eq!(valve[0], "1.6", "{printed}");
        assert_eq!(valve[2], "0.8", "{printed}");
        assert!(number(&valve[1]) >= 2.7, "{printed}");
        assert!(number(&valve[1]) <= 3.25, "{printed}");
        assert!(
            (number(&valve[3]) - number(&valve[1]) * 0.5).abs() <= 0.001,
            "{printed}"
        );

        // The letter alone, set 1 m high: it covers its face's ascent above the baseline and
        // descent below it, together 1 to 1.4 of the size in every face (Arial 1.117, Segoe UI
        // 1.33), so 1 to 1.4 m high and 2 to 2.8 mm at 1:500 - not "0 x 0".
        assert!(select_symbol(&mut bench, "TEST Letter"));
        let letter_printed = bench.text("detailPrint");
        let letter =
            printed_at_500(&letter_printed).unwrap_or_else(|| panic!("{letter_printed}"));
        assert!(number(&letter[0]) > 0.0, "{letter_printed}");
        assert!(number(&letter[1]) >= 2.0, "{letter_printed}");
        assert!(number(&letter[1]) <= 2.8, "{letter_printed}");
    }

    #[test]
    fn importing_merges_the_files_definitions_into_the_library_and_logs_what_it_added_and_replaced()
     {
        // The session starts with the linestyles and its own TEST Tree; importing the symbols
        // adds three and replaces the tree - and keeps the three linestyles, because a load from
        // here never replaces (D1): 3 + 1 + 3.
        let mut host = fixture_host(&["test_linestyles"]);
        add_to_library(
            host.document_mut(),
            LineStyle {
                name: String::from("TEST Tree"),
                strokes: vec![Stroke {
                    op: StrokeOp::Draw,
                    point: Point2::new(1.0, 0.0),
                    ..Stroke::default()
                }],
                ..LineStyle::default()
            },
        );
        let mut bench = library(host);

        let file = customisation_fixture_file("test_symbols");
        assert!(
            bench
                .dialog
                .import_definitions_file(&mut bench.host, &file)
        );

        // The dialog handed the executor the line CUSTOMISE DEFINITIONS and the file, and the verb
        // said what it did: 3 added (Survey Mark, U Turn and Valve) and 1 replaced (Tree), the
        // load named by its customisation.
        assert_eq!(
            bench.lines(),
            vec![format!("CUSTOMISE DEFINITIONS {}", quoted_file(&file))]
        );
        let reply = bench.host.last_reply();
        assert!(reply.contains("loaded file="), "{reply}");
        assert!(
            reply.contains("name=test_symbols definitions_added=3 definitions_replaced=1 "),
            "{reply}"
        );
        let library = bench.document().style_library();
        assert_eq!(library.size(), 7);
        assert!(library.contains("TEST Dashed Kerb"));
        // What came in says where it came from, the tree that was replaced included, and the
        // session records that customisation as one that brought definitions and no rules.
        assert_eq!(
            library.find("TEST Valve").expect("the valve").source,
            "test_symbols"
        );
        let tree = library.find("TEST Tree").expect("the tree");
        assert_eq!(tree.source, "test_symbols");
        assert!(tree.symbol);
        let brought = bench
            .document()
            .customisation_state()
            .sources
            .iter()
            .find(|each| each.name == "test_symbols")
            .cloned()
            .expect("the source is recorded");
        assert!(brought.definitions);
        assert!(!brought.rules);
        bench.draw();
        assert!(
            bench
                .dialog
                .shown_names()
                .iter()
                .any(|name| name == "TEST Valve")
        );

        // A file that is not there fails, says so, and changes nothing.
        bench.host.parts.logged.clear();
        assert!(!bench.dialog.import_definitions_file(
            &mut bench.host,
            &customisation_fixture_directory().join("no_such.customisation.json")
        ));
        assert_eq!(bench.host.logged().len(), 1);
        assert!(bench.host.logged()[0].1);
        assert_eq!(bench.document().style_library().size(), 7);
    }

    #[test]
    fn import_takes_a_files_definitions_and_colours_and_leaves_its_rules_and_settings_alone() {
        // A customisation written for this test, holding one of everything: a symbol whose pen is
        // a colour of its own, that colour, a rule and the linework codes.
        let scratch = Scratch::new("symbols-extra");
        let path = scratch.file(
            "extra.customisation.json",
            r##"{
  "format": "katana-customisation",
  "version": 1,
  "name": "extra",
  "colours": {
    "sui test purple": "#800080"
  },
  "linework": {"start": "BEG"},
  "symbols": [
    {"name": "TEST Extra Mark", "group": "Test/Marks", "atVertices": true, "strokes": [
      ["pen", "sui test purple"],
      ["circle", 1]
    ]}
  ],
  "codes": [
    {"key": "ZZ*", "sets": "feature", "layer": "TEST MISC"}
  ]
}
"##,
        );
        // 7 definitions, 11 rules, no colours.
        let mut bench = library(fixture_host(&FIXTURES));

        assert!(
            bench
                .dialog
                .import_definitions_file(&mut bench.host, &path)
        );

        // Taken: the definition, listed as a symbol under its customisation's name, and the
        // colour.
        let library = bench.document().style_library();
        assert_eq!(library.size(), 8);
        let mark = library.find("TEST Extra Mark").expect("the mark came in");
        assert!(mark.symbol);
        assert_eq!(mark.source, "extra");
        assert_eq!(
            bench
                .document()
                .customisation_state()
                .colours
                .find("sui test purple"),
            Some(rgb(128, 0, 128))
        );
        // One definition and one colour, each added and none replaced.
        let reply = bench.host.last_reply();
        assert!(
            reply.contains(
                "name=extra definitions_added=1 definitions_replaced=0 codes_added=0 \
                 codes_replaced=0 colours_added=1 colours_replaced=0 "
            ),
            "{reply}"
        );

        // Left alone: its rule, and its spelling of Start. The reply says so, as a record: one
        // rule, linework codes, no automation.
        assert_eq!(bench.document().survey_map().size(), 11);
        assert_eq!(bench.document().customisation_state().linework.start, "ST");
        assert!(
            reply.contains("left rules=1 linework=yes automation=no"),
            "{reply}"
        );
        // The session's record of what went into it: "extra" brought definitions, and no rules.
        let extra = bench
            .document()
            .customisation_state()
            .sources
            .iter()
            .find(|each| each.name == "extra")
            .cloned()
            .expect("the source is recorded");
        assert!(extra.definitions);
        assert!(!extra.rules);
    }

    #[test]
    fn import_calls_the_commit_hook_before_the_install_and_what_it_hands_back_after_one_that_was_taken()
     {
        // The commit hook is how a session whose customisation is the kept one stays kept across
        // an editor's own commit: its maker is asked BEFORE - the install leaves the session "not
        // kept", so only before it can "was it kept?" be answered - and what it hands back is
        // called AFTER an install that was taken. The library knows neither; it calls, in that
        // order, as the definition editor's Save does. A counting hook that notes at each call the
        // library's generation, and whether the session says it is kept, shows which side of the
        // commit a call came on.
        #[derive(Default)]
        struct Seen {
            begun: Cell<i32>,
            finished: Cell<i32>,
            library_at_begin: Cell<u64>,
            library_at_finish: Cell<u64>,
            kept_at_begin: Cell<bool>,
            kept_at_finish: Cell<bool>,
        }
        let scratch = Scratch::new("symbols-hook");
        let mut host = fixture_host(&["test_linestyles"]);
        // The session as a start leaves one: kept.
        let session = host.document().customisation();
        assert!(
            host.document_mut()
                .install_customisation(session, CustomisationOrigin::Kept, true)
                .is_ok()
        );
        assert!(host.document().customisation_state().kept);
        let seen = Rc::new(Seen {
            kept_at_finish: Cell::new(true),
            ..Seen::default()
        });
        let begin: BeginCommit = {
            let seen = Rc::clone(&seen);
            Rc::new(move |host: &mut dyn Host| -> Option<Committed> {
                seen.begun.set(seen.begun.get() + 1);
                seen.library_at_begin
                    .set(host.document().library_generation());
                seen.kept_at_begin
                    .set(host.document().customisation_state().kept);
                let seen = Rc::clone(&seen);
                Some(Box::new(move |host: &mut dyn Host| {
                    seen.finished.set(seen.finished.get() + 1);
                    seen.library_at_finish
                        .set(host.document().library_generation());
                    seen.kept_at_finish
                        .set(host.document().customisation_state().kept);
                }))
            })
        };
        let mut bench = library_with(
            host,
            CodesContext {
                begin_commit: Some(begin),
                ..context()
            },
        );

        // What is not a commit is asked of the hook all the same (the line is run between the two
        // calls, as the definition editor's Delete does) but is never followed by what it hands
        // back: a file that is not there, and one that reads and holds nothing a symbol library
        // takes - the fixture of survey codes, which has rules and no definition or colour.
        assert!(!bench.dialog.import_definitions_file(
            &mut bench.host,
            &scratch.path("no-such.customisation.json")
        ));
        assert!(!bench.dialog.import_definitions_file(
            &mut bench.host,
            &customisation_fixture_file("test_survey")
        ));
        assert_eq!(seen.begun.get(), 2);
        assert_eq!(seen.finished.get(), 0);
        assert!(bench.document().customisation_state().kept);

        let before = bench.document().library_generation();
        assert!(bench.dialog.import_definitions_file(
            &mut bench.host,
            &customisation_fixture_file("test_symbols")
        ));
        assert_eq!(seen.begun.get(), 3);
        assert_eq!(seen.finished.get(), 1);
        assert_eq!(
            seen.library_at_begin.get(),
            before,
            "asked before the library was touched"
        );
        assert!(
            seen.kept_at_begin.get(),
            "and so while the session still said it was kept"
        );
        assert!(
            seen.library_at_finish.get() > before,
            "answered once it had been"
        );
        assert!(
            !seen.kept_at_finish.get(),
            "which is what the answer is there to put right"
        );
        assert!(bench.document().style_library().contains("TEST Valve"));
    }

    #[test]
    fn importing_definitions_leaves_out_a_source_that_brought_only_rules() {
        // A project drawn with a customisation "B", opened where B is not loaded: B is missing,
        // and the session says so.
        let scratch = Scratch::new("symbols-sources");
        let project = scratch.path("recorded.katana");
        {
            let metadata = ProjectMetadata {
                customisation: strings(&["B"]),
                ..ProjectMetadata::default()
            };
            let mut store = ProjectStore::create(&project, &metadata)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            store
                .save(&capture_model(&Model::default(), metadata, Vec::new()))
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        }
        let mut document = Document::new(None);
        assert!(document.open(&project).is_ok());
        assert_eq!(
            document.customisation_state().missing_at_open,
            strings(&["B"])
        );

        // A customisation that was itself merged from two, and lists them: A brought its one
        // definition and B its one rule.
        let file = scratch.file(
            "both.customisation.json",
            r#"{
  "format": "katana-customisation",
  "version": 1,
  "name": "both",
  "sources": [
    {"name": "A", "definitions": true, "notice": ["A's notice."]},
    {"name": "B", "rules": true, "notice": ["B's notice."]}
  ],
  "symbols": [
    {"name": "TEST A Mark", "atVertices": true, "from": "A", "strokes": [
      ["circle", 1]
    ]}
  ],
  "codes": [
    {"key": "BB*", "sets": "feature", "layer": "TEST B"}
  ]
}
"#,
        );
        let mut host = interpreter_host(document);
        {
            let mut dialog = SymbolLibraryDialog::new(host.document(), context());
            assert!(dialog.import_definitions_file(&mut host, &file));
        }

        // The definition came in, and A with it: what A brought HERE is its definitions, and its
        // notice travels with them.
        let library = host.document().style_library();
        assert_eq!(
            library.find("TEST A Mark").expect("the mark came in").source,
            "A"
        );
        assert!(host.document().survey_map().empty());
        // B brought the file its rules, and none of them was taken: B is no source of this
        // session, and it is as missing as it was. Listed, it would have passed for loaded - the
        // warning gone and the record of the project saying B is here - with not one of its rules
        // in the session.
        assert_eq!(
            host.document().customisation_state().sources,
            vec![source("A", true, false, &["A's notice."])]
        );
        assert_eq!(
            host.document().customisation_state().missing_at_open,
            strings(&["B"])
        );

        // A save keeps B in the record as a name still wanted, so the project opened again -
        // here, in this very session - still says B is missing.
        assert!(host.document_mut().save().is_ok());
        assert_eq!(host.document().metadata().customisation, strings(&["A", "B"]));
        assert!(host.document_mut().open(&project).is_ok());
        assert_eq!(
            host.document().customisation_state().missing_at_open,
            strings(&["B"])
        );
    }

    #[test]
    fn import_refuses_a_file_that_is_not_a_katana_customisation_and_one_with_nothing_for_it() {
        let scratch = Scratch::new("symbols-refused");
        let mut bench = library(fixture_host(&FIXTURES));
        let generation = bench.document().library_generation();

        // A definition as the older style libraries wrote one: not JSON.
        let other = scratch.file(
            "older_library.txt",
            "worldstyle \"TEST Old\" {\n  move 0 0\n  draw 1 0\n}\n",
        );
        assert!(
            !bench
                .dialog
                .import_definitions_file(&mut bench.host, &other)
        );
        assert_eq!(bench.host.logged().len(), 1);
        let (message, is_error) = bench.host.logged()[0].clone();
        assert!(is_error);
        assert!(
            message.contains("not a Katana customisation file"),
            "{message}"
        );

        // A customisation of survey codes alone: nothing a symbol library takes, and the refusal
        // says where its rules are loaded instead.
        bench.host.parts.logged.clear();
        assert!(!bench.dialog.import_definitions_file(
            &mut bench.host,
            &customisation_fixture_file("test_survey")
        ));
        assert_eq!(bench.host.logged().len(), 1);
        let (message, is_error) = bench.host.logged()[0].clone();
        assert!(is_error);
        assert!(
            message.contains(
                "the file holds no definitions or colours to load, so nothing was loaded; its \
                 survey code rules are loaded by CUSTOMISE <file>"
            ),
            "{message}"
        );

        assert_eq!(bench.document().style_library().size(), 7);
        assert_eq!(bench.document().survey_map().size(), 11);
        assert_eq!(bench.document().library_generation(), generation);
    }

    #[test]
    fn a_headless_session_opens_no_file_dialog_and_a_chooser_stands_in_for_one() {
        let mut bench = library(fixture_host(&["test_linestyles"]));
        assert!(bench.dialog.headless(&bench.host));

        bench.press("importDefinitions");
        assert_eq!(bench.host.logged().len(), 1);
        assert!(bench.host.logged()[0].1);
        assert!(bench.host.logged()[0].0.contains("importDefinitionsFile"));

        bench.dialog.choose_import_file =
            Some(Box::new(|| -> PathBuf { customisation_fixture_file("test_symbols") }));
        bench.press("importDefinitions");
        assert!(
            bench
                .document()
                .style_library()
                .contains("TEST Survey Mark")
        );
    }

    #[test]
    fn export_writes_only_the_selected_library_definitions_and_names_the_rest() {
        let scratch = Scratch::new("symbols-export");
        let mut bench = library(fixture_host(&FIXTURES));
        for name in ["TEST Valve", "cross", "TEST Survey Mark"] {
            bench.dialog.add_to_selection(&mut bench.host, name);
        }
        let path = scratch.path("export.customisation.json");

        assert!(bench.dialog.export_selected_to(&mut bench.host, &path));

        let written = read_bytes(&path);
        // A Katana customisation file, under the session's name - the first fixture loaded. Of the
        // three customisations the session was loaded from, the ONE both selected symbols came
        // from is this file's source, one line (docs/customisation.md, "Layout", rules 4, 6 and
        // 11); then the lists at once: the session has no colours, and no source it leaves out
        // has a notice to carry.
        let head = concat!(
            "{\n",
            "  \"format\": \"katana-customisation\",\n",
            "  \"version\": 1,\n",
            "  \"name\": \"test_symbols\",\n",
            "  \"sources\": [\n",
            "    {\"name\": \"test_symbols\", \"definitions\": true}\n",
            "  ],\n",
            "  \"symbols\": [\n"
        );
        assert!(written.starts_with(head), "{written}");
        let read = customisation_from_json(written.as_bytes()).expect("the file reads");
        // Neither the source that brought the session's rules nor the one that brought its
        // linestyles is a source of this file.
        assert_eq!(read.sources, vec![source("test_symbols", true, false, &[])]);
        assert!(read.notice.is_empty());
        assert_eq!(
            read.library.names(),
            strings(&["TEST Survey Mark", "TEST Valve"])
        );
        // The format is lossless: the valve reads back as the session holds it, where it came
        // from and that it is listed as a symbol included - the two things a style library file
        // could not hold.
        let valve = bench
            .document()
            .style_library()
            .find("TEST Valve")
            .cloned()
            .expect("the valve");
        assert_eq!(valve.source, "test_symbols");
        assert!(valve.symbol);
        assert!(read.library.find("TEST Valve") == Some(&valve));
        // The definitions alone: none of the session's 11 rules, and nothing said of its settings
        // - a file of two symbols for a colleague must not reset their control codes.
        assert!(read.map.empty());
        assert!(read.linework.is_none());
        assert!(read.automation.is_none());
        assert!(bench.logged_exactly("Not exported, as no library defines them: cross"));
    }

    #[test]
    fn an_export_carries_the_sessions_notice_and_the_sources_that_brought_definitions() {
        let scratch = Scratch::new("symbols-noticed");
        let mut document = Document::new(None);
        install_noticed_session(&mut document);
        assert_eq!(document.customisation_state().name, "base");
        assert_eq!(document.customisation_state().sources.len(), 4);
        assert!(document.customisation().based_on.is_some());
        let mut bench = library(interpreter_host(document));
        assert!(bench.dialog.grid_model().row_of("MARK Cross").is_some());
        bench.dialog.add_to_selection(&mut bench.host, "MARK Cross");
        let path = scratch.path("noticed.customisation.json");

        // The cross alone, which came from marks and has no pen. The file carries no colour, and
        // so nothing of the table of colours: it is no source of this file. Its sources are
        // `marks` alone - nothing of base's is written, and listed it would pass for loaded
        // wherever the file went - and the session's notice is followed by the notice of each
        // source left out that has one, in their order: the client's and the table of colours'.
        assert!(bench.dialog.export_selected_to(&mut bench.host, &path));
        let read = customisation_from_json(read_bytes(&path).as_bytes()).expect("the file reads");
        assert_eq!(read.library.names(), strings(&["MARK Cross"]));
        assert!(read.colours.empty());
        assert_eq!(
            read.notice,
            strings(&[
                "Base: all rights reserved.",
                "Client codes, for this job only.",
                "Tints: free to use."
            ])
        );
        assert_eq!(
            read.sources,
            vec![source("marks", true, false, &["Marks drawn by hand."])]
        );
        assert!(read.based_on.is_none());

        // With the peg, whose pen is one of the table's colours: the whole file.
        assert!(bench.dialog.grid_model().row_of("BASE Peg").is_some());
        bench.dialog.add_to_selection(&mut bench.host, "BASE Peg");
        assert!(bench.dialog.export_selected_to(&mut bench.host, &path));
        assert_eq!(read_bytes(&path), NOTICED_SYMBOLS);

        // The colleague the file is for imports its definitions into a session of their own,
        // which has no name yet: it takes the file's, with its description and notice - the two
        // lines the file carries - and each source the file lists keeps its own notice (cad's
        // customisation_merge). They are shown whose data it is.
        {
            let mut theirs = interpreter_host(Document::new(None));
            let mut library = SymbolLibraryDialog::new(theirs.document(), context());
            assert!(library.import_definitions_file(&mut theirs, &path));
            let colleague = theirs.document();
            let state = colleague.customisation_state();
            assert_eq!(state.name, "base");
            assert_eq!(state.description, "The base set.");
            assert_eq!(
                state.notice,
                strings(&[
                    "Base: all rights reserved.",
                    "Client codes, for this job only."
                ])
            );
            assert_eq!(
                state.sources,
                vec![
                    source("base", true, false, &[]),
                    source("marks", true, false, &["Marks drawn by hand."]),
                    source("tints", false, false, &["Tints: free to use."]),
                ]
            );
            assert_eq!(
                colleague.style_library().names(),
                strings(&["BASE Peg", "MARK Cross"])
            );
            assert_eq!(
                colleague
                    .style_library()
                    .find("BASE Peg")
                    .expect("the peg")
                    .source,
                "base"
            );
            assert_eq!(
                colleague
                    .style_library()
                    .find("MARK Cross")
                    .expect("the cross")
                    .source,
                "marks"
            );
            assert_eq!(state.colours.find("tint teal"), Some(rgb(0, 128, 128)));
            assert!(colleague.survey_map().empty());
        }
    }

    #[test]
    fn an_export_carries_the_colours_its_pens_name_and_no_others() {
        // Two colours of the session's own, and a symbol whose pen names one.
        let scratch = Scratch::new("symbols-coloured");
        let mut host = fixture_host(&FIXTURES);
        let mut colours = ColourTable::new();
        assert!(colours.add("sui test purple", rgb(128, 0, 128)).is_ok());
        assert!(colours.add("sui test teal", rgb(0, 128, 128)).is_ok());
        host.document_mut().set_colour_table(colours);
        add_to_library(
            host.document_mut(),
            LineStyle {
                name: String::from("TEST Purple Mark"),
                at_vertices: true,
                symbol: true,
                // The pen spelled another way than the table spells it: one name, by the fold a
                // colour name is compared in.
                strokes: vec![
                    Stroke {
                        op: StrokeOp::Pen,
                        pen: String::from("SUI_Test_Purple"),
                        ..Stroke::default()
                    },
                    Stroke {
                        op: StrokeOp::Circle,
                        radius: 1.0,
                        ..Stroke::default()
                    },
                ],
                ..LineStyle::default()
            },
        );
        let mut bench = library(host);
        for name in ["TEST Purple Mark", "TEST Tree"] {
            bench.dialog.add_to_selection(&mut bench.host, name);
        }
        let path = scratch.path("coloured.customisation.json");

        assert!(bench.dialog.export_selected_to(&mut bench.host, &path));

        let read = customisation_from_json(read_bytes(&path).as_bytes()).expect("the file reads");
        assert_eq!(
            read.library.names(),
            strings(&["TEST Purple Mark", "TEST Tree"])
        );
        // The purple the mark's pen names, as the table spells it; not the teal no selected pen
        // names, and not the tree's "green", which is a standard name and no customisation's to
        // define.
        let carried = read.colours.entries();
        assert_eq!(carried.len(), 1);
        assert_eq!(carried[0].name, "sui test purple");
        assert_eq!(carried[0].colour, rgb(128, 0, 128));
    }

    #[test]
    fn the_grids_selection_and_current_symbol_survive_a_reload_and_a_filter() {
        // Two selected for an export, the mark current; then a command reloads the grid and a
        // search narrows it. What is still shown stays selected.
        let mut bench = library(fixture_host(&FIXTURES));
        assert!(select_symbol(&mut bench, "TEST Survey Mark"));
        bench.dialog.add_to_selection(&mut bench.host, "TEST Valve");
        assert_eq!(
            bench.dialog.selected_names(),
            strings(&["TEST Survey Mark", "TEST Valve"])
        );

        point(&mut bench.host, 0.0, 0.0);
        bench.draw();
        assert_eq!(
            bench.dialog.selected_names(),
            strings(&["TEST Survey Mark", "TEST Valve"])
        );
        assert_eq!(bench.dialog.current_symbol(), "TEST Survey Mark");

        // "survey" keeps only the mark: the valve is hidden, the mark stays.
        bench.fill("filterText", "survey");
        assert_eq!(
            bench.dialog.selected_names(),
            strings(&["TEST Survey Mark"])
        );
        assert_eq!(bench.dialog.current_symbol(), "TEST Survey Mark");
        // And a search that hides it leaves nothing current for an action to reach.
        bench.fill("filterText", "valve");
        assert_eq!(bench.dialog.current_symbol(), "");
        assert!(!bench.enabled("assignSize"));
    }

    #[test]
    fn a_symbol_deselected_with_ctrl_stays_deselected_across_a_filter_and_a_reload() {
        // The mark, then the valve and the tree added with Ctrl, then the tree taken out again
        // with Ctrl: the tree is left current but not selected.
        let scratch = Scratch::new("symbols-ctrl");
        let mut bench = library(fixture_host(&FIXTURES));
        assert!(select_symbol(&mut bench, "TEST Survey Mark"));
        bench.dialog.ctrl_click(&mut bench.host, "TEST Valve");
        bench.dialog.ctrl_click(&mut bench.host, "TEST Tree");
        // Rows in the grid's order: names with case folded.
        assert_eq!(
            bench.dialog.selected_names(),
            strings(&["TEST Survey Mark", "TEST Tree", "TEST Valve"])
        );
        bench.dialog.ctrl_click(&mut bench.host, "TEST Tree");
        let kept = strings(&["TEST Survey Mark", "TEST Valve"]);
        assert_eq!(bench.dialog.selected_names(), kept);
        assert_eq!(bench.dialog.current_symbol(), "TEST Tree");

        // "test" still shows all three: the selection is what the person left, not the tree they
        // took out of it.
        bench.fill("filterText", "test");
        assert_eq!(bench.dialog.selected_names(), kept);
        assert_eq!(bench.dialog.current_symbol(), "TEST Tree");

        // A command elsewhere reloads the grid: the same again.
        point(&mut bench.host, 0.0, 0.0);
        bench.draw();
        assert_eq!(bench.dialog.selected_names(), kept);
        assert_eq!(bench.dialog.current_symbol(), "TEST Tree");

        // So an export writes the two kept, and not the tree.
        let out = scratch.path("ctrl_export.customisation.json");
        assert!(bench.dialog.export_selected_to(&mut bench.host, &out));
        let written =
            customisation_from_json(read_bytes(&out).as_bytes()).expect("the file reads");
        assert_eq!(written.library.names(), kept);
    }

    #[test]
    fn replace_in_styles_swaps_a_missing_symbol_for_a_library_one_in_one_undo_step() {
        let mut host = fixture_host(&FIXTURES);
        for name in ["Old Pits", "Old Pits Large"] {
            style(&mut host, name, "OLD Pit Lid");
        }
        let mut bench = library(host);
        assert!(select_symbol(&mut bench, "OLD Pit Lid"));
        bench
            .dialog
            .set_replacement(&mut bench.host, "TEST Survey Mark");
        let steps = bench.document().history().undo_count();

        bench.press("replaceInStyles");

        assert_eq!(bench.document().history().undo_count(), steps + 1);
        let symbol_of = |bench: &Library, name: &str| -> String {
            bench
                .document()
                .model()
                .styles
                .find(name)
                .map(|style| style.symbol.clone())
                .unwrap_or_default()
        };
        assert_eq!(symbol_of(&bench, "Old Pits"), "TEST Survey Mark");
        assert_eq!(symbol_of(&bench, "Old Pits Large"), "TEST Survey Mark");
        // Only PX*'s name is left missing once the dialog has caught up.
        bench.draw();
        assert_eq!(bench.text("filterMissing"), "Missing (1)");

        assert!(bench.document_mut().undo().is_ok());
        assert_eq!(symbol_of(&bench, "Old Pits"), "OLD Pit Lid");
    }

    #[test]
    fn replace_in_styles_waits_for_a_replacement_and_never_takes_the_symbol_off() {
        let mut host = fixture_host(&FIXTURES);
        style(&mut host, "Marks", "TEST Valve");
        let mut bench = library(host);
        assert!(select_symbol(&mut bench, "TEST Valve"));

        // The picker starts with nothing chosen: nothing to replace it with.
        assert_eq!(bench.dialog.replace_with().current_name(), "");
        assert!(!bench.enabled("replaceInStyles"));
        let steps = bench.document().history().undo_count();
        assert!(!bench.dialog.replace_in_styles(&mut bench.host, ""));
        assert_eq!(
            bench
                .document()
                .model()
                .styles
                .find("Marks")
                .expect("the style")
                .symbol,
            "TEST Valve"
        );
        assert_eq!(bench.document().history().undo_count(), steps);
        assert!(bench.logged_exactly(
            "Replace \"TEST Valve\": choose the symbol to put in its place first"
        ));

        // Chosen from the list, or typed: the button can be used.
        bench
            .dialog
            .set_replacement(&mut bench.host, "TEST Survey Mark");
        bench.draw();
        assert!(bench.enabled("replaceInStyles"));
        // The symbol itself is no replacement for itself.
        bench.dialog.set_replacement(&mut bench.host, "TEST Valve");
        bench.draw();
        assert!(!bench.enabled("replaceInStyles"));
        bench.dialog.type_replacement(&mut bench.host, "TEST Tree");
        bench.draw();
        assert_eq!(bench.dialog.replace_with().current_name(), "TEST Tree");
        assert!(bench.enabled("replaceInStyles"));
        // Cleared again: not usable again.
        bench.dialog.set_replacement(&mut bench.host, "");
        bench.draw();
        assert!(!bench.enabled("replaceInStyles"));
    }

    #[test]
    fn set_on_style_and_select_points_using_act_on_the_current_symbol() {
        let mut host = fixture_host(&FIXTURES);
        style(&mut host, "Marks", "");
        let p1 = point(&mut host, 0.0, 0.0);
        let p2 = point(&mut host, 5.0, 0.0);
        point(&mut host, 9.0, 0.0);
        assert!(
            host.document_mut()
                .execute(set_entity_style(vec![p2, p1], "Marks"))
                .is_ok()
        );
        let shown: Rc<RefCell<Vec<EntityId>>> = Rc::new(RefCell::new(Vec::new()));
        let select_and_show: SelectAndShow = {
            let shown = Rc::clone(&shown);
            Rc::new(move |_host: &mut dyn Host, ids: &[EntityId]| {
                *shown.borrow_mut() = ids.to_vec();
            })
        };
        let mut bench = library_with(
            host,
            CodesContext {
                select_and_show: Some(select_and_show),
                ..context()
            },
        );
        assert!(select_symbol(&mut bench, "TEST Valve"));

        bench.fill("targetStyle", "Marks");
        bench.press("setOnStyle");
        assert_eq!(
            bench
                .document()
                .model()
                .styles
                .find("Marks")
                .expect("the style")
                .symbol,
            "TEST Valve"
        );

        bench.draw();
        assert!(select_symbol(&mut bench, "TEST Valve"));
        assert!(bench.enabled("selectPointsUsing"));
        bench.press("selectPointsUsing");
        assert_eq!(*shown.borrow(), vec![p1, p2]);
    }

    #[test]
    fn the_dialog_outlives_its_document_and_does_nothing_after_it() {
        let mut bench = library(fixture_host(&FIXTURES));
        assert!(select_symbol(&mut bench, "TEST Survey Mark"));

        bench.close_drawing();

        assert!(!bench.dialog.assign_to_selected_points(&mut bench.host));
        assert!(!bench.dialog.import_definitions_file(
            &mut bench.host,
            &customisation_fixture_file("test_symbols")
        ));
        assert!(!bench.dialog.select_points_using(&mut bench.host));
        bench.dialog.reload(&mut bench.host);
        bench.draw();
    }

    // ---- the colour a pen inside a definition names ---------------------------------------------
    //
    // A definition's pen NAMES its colour, and the one painter every picture and the plan go
    // through resolves the name as the drawing does: the session's customisation first, then the
    // standard names. Purple is 128 0 128 here, and the standard blue is the CSS one, 0 0 255
    // (docs/customisation.md, "Colour names").

    #[test]
    fn a_pen_is_the_colour_the_customisation_gives_its_name_and_the_entitys_pen_when_nothing_does()
    {
        let entity = Pen::with(
            CanvasColor::rgb(10, 20, 30),
            2.0,
            PenStyle::Solid,
            PenCap::Flat,
            PenJoin::Bevel,
        );
        let colours = purple();

        // The customisation's name, however it is spelled within the one fold: letter case, and
        // `_` or `-` for a blank.
        for spelled in ["sui test purple", "SUI_Test-Purple"] {
            let pen = style_pen_for(&entity, spelled, None, Some(&colours));
            assert_eq!(pen.color, CanvasColor::rgb(128, 0, 128), "{spelled}");
            // Only the colour is the definition's: the width and the cap are the entity's.
            assert_eq!(pen.width, 2.0, "{spelled}");
            assert_eq!(pen.cap, PenCap::Flat, "{spelled}");
        }
        // A standard name is its standard colour, with a table and without one.
        assert_eq!(
            style_pen_for(&entity, "blue", None, Some(&colours)).color,
            CanvasColor::rgb(0, 0, 255)
        );
        assert_eq!(
            style_pen_for(&entity, "blue", None, None).color,
            CanvasColor::rgb(0, 0, 255)
        );
        // What an unresolved pen draws is what it always drew: the entity's pen, whole. A plot
        // pen's number is such a name, and so is the customisation's own where no table is given
        // or the table is empty.
        let none = ColourTable::new();
        assert!(style_pen_for(&entity, "pen 035", None, Some(&colours)) == entity);
        assert!(style_pen_for(&entity, "sui test purple", None, None) == entity);
        assert!(style_pen_for(&entity, "sui test purple", None, Some(&none)) == entity);
        assert!(style_pen_for(&entity, "", None, Some(&colours)) == entity);
    }

    #[test]
    fn a_symbol_stamped_on_the_plan_is_drawn_again_when_the_customisations_colours_change() {
        // The plan as a VIEW paints it: symbols stamped from images kept between frames
        // (PlanPaintOptions::symbol_sprites), in ONE cache the view keeps. A stamp bakes in the
        // colour its pen resolved to, and its key holds the symbol and the entity's pen but not
        // that colour - so a stamp made before the customisation's colours changed must not be
        // the one drawn after.
        let mut host = fixture_host(&FIXTURES);
        add_to_library(host.document_mut(), purple_ring());
        style(&mut host, "Purple Rings", "TEST Purple Ring");
        assert!(
            host.document_mut()
                .execute(create_point(
                    Point2::new(0.0, 0.0),
                    EntityAttributes {
                        layer: String::from("0"),
                        style: String::from("Purple Rings"),
                        ..EntityAttributes::default()
                    }
                ))
                .is_ok()
        );

        let raster: Arc<dyn RasterBackend> = Arc::new(CanvasRasterBackend);
        let options = PlanPaintOptions {
            symbol_sprites: true,
            raster: Some(Arc::clone(&raster)),
            ..PlanPaintOptions::default()
        };
        // The view's, kept across both paints.
        let mut cache = PlanPaintCache::default();
        let plan = |document: &Document, cache: &mut PlanPaintCache| -> (Image, PlanPaintStats) {
            // 64 pixels square about the point at 32 pixels a metre: the ring is 32 pixels across,
            // well above the size a symbol is drawn as a dot.
            let mut image = Image::new(64, 64);
            image.fill(CanvasColor::rgb(0, 0, 0));
            let mut transform = ViewTransform::default();
            transform.resize(64.0, 64.0);
            transform.scale = 32.0;
            transform.center = Point2::new(0.0, 0.0);
            let frame = PlanFrame {
                transform,
                ..PlanFrame::default()
            };
            let source = plan_source_of(document);
            let mut stats = PlanPaintStats::default();
            raster.paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
                stats = paint_plan(canvas, &source, &frame, &options, cache);
            });
            (image, stats)
        };

        // Nothing defines the pen: the ring is stamped, in the entity's pen.
        let (before, stats) = plan(host.document(), &mut cache);
        assert_eq!(stats.sprites_drawn, 1, "the ring was not drawn from a stamp");
        assert_eq!(cache.sprite_count(), 1);
        assert!(!holds_purple(&before));
        // Painted again with nothing changed, it is the same stamp.
        let (again, _) = plan(host.document(), &mut cache);
        assert!(again == before);
        assert_eq!(cache.sprite_count(), 1);

        // The customisation defines it: the next frame, from the same cache, is purple - and still
        // from a stamp, a new one.
        host.document_mut().set_colour_table(purple());
        let (after, stats) = plan(host.document(), &mut cache);
        assert_eq!(stats.sprites_drawn, 1);
        assert_eq!(cache.sprite_count(), 1);
        assert!(holds_purple(&after));
    }
}
