// The Document's spatial index and what a box is drawn from besides its entity
// (docs/performance.md, "The index and the tables a box is drawn from").
//
// The index stores a box an entity, when the entity changes. A dimension's
// box is what it DRAWS - label, arrows, extension lines - by the dimension
// style its layer names (spatial_query's query_extents), so an edit of that
// style, or of the layer's choice of one, moves the box and changes no
// entity. And a command that fails publishes no change at all, whatever it
// did before it failed. These tests hold the index to the model through both:
// after every step, a query of the index is what a scan of every entity's box
// as the model now stands gives.
//
// The windows of the first tests are worked out by hand. An aligned dimension
// from (0,0) to (10,0) offset 2 has its dimension line at y = 2 and its label
// sat DIMGAP above it, reading "10.000" and centred on x = 5: with the
// built-in style (text height 2.5, gap 0.625) the label's top is at
// 2 + 0.625 + 2.5 = 5.125 and nothing drawn is higher (the extension lines
// stop at 2 + 1.25); with a text height of 25 it is at 2 + 0.625 + 25 =
// 27.625. So a window over x 4..6 and y 6..27 meets the dimension restyled to
// 25 and does not meet it as first drawn.

use std::collections::{BTreeMap, BTreeSet};

use katana_cad::Document;
use katana_cad::spatial_query::{for_each_candidate, query_extents, worth_indexing};
use katana_commands::{
    Command, CommandContext, CommandPtr, EntityAttributes, Transaction, create_dimension,
    create_dimension_style, create_entities, create_layer, create_line, delete_dimension_style,
    delete_entities, move_entities, update_dimension_style, update_layer,
};
use katana_core::{Error, ErrorCode, Status};
use katana_entity::{DimensionGeometry, DimensionStyle, Entity, EntityId, Geometry, Layer};
use katana_geometry::{Box2, Point2, Segment2, Vec2};

use crate::Mt19937;

#[track_caller]
fn must(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

fn on_layer(layer: &str) -> EntityAttributes {
    EntityAttributes {
        layer: String::from(layer),
        ..EntityAttributes::default()
    }
}

#[track_caller]
fn add_layer(document: &mut Document, name: &str) {
    must(document.execute(create_layer(Layer {
        name: String::from(name),
        ..Layer::default()
    })));
}

/// The layer `name` as it is, naming `style` for its dimensions.
#[track_caller]
fn layer_styled(document: &Document, name: &str, style: &str) -> Layer {
    let mut layer = document
        .model()
        .layers
        .find(name)
        .cloned()
        .expect("the layer exists");
    layer.dimension_style = String::from(style);
    layer
}

/// The built-in dimension style with another text height.
fn standard_with_text_height(text_height: f64) -> DimensionStyle {
    DimensionStyle {
        text_height,
        ..DimensionStyle::default()
    }
}

#[track_caller]
fn created(document: &mut Document, command: CommandPtr) -> EntityId {
    must(document.execute(command));
    *document
        .last_created_entities()
        .first()
        .expect("the command created an entity")
}

/// The dimension the comment at the top works out, on `layer`.
#[track_caller]
fn worked_dimension(document: &mut Document, layer: &str) -> EntityId {
    let dimension = DimensionGeometry {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(10.0, 0.0),
        offset: 2.0,
        ..DimensionGeometry::default()
    };
    created(document, create_dimension(dimension, on_layer(layer)))
}

/// An aligned dimension 2 to 20 long, anywhere in a square 400 across and at
/// any bearing, its dimension line 0.5 to 4 to either side.
#[track_caller]
fn draw_dimension(document: &mut Document, random: &mut Mt19937, layer: &str) {
    let start = Point2::new(random.real(-200.0, 200.0), random.real(-200.0, 200.0));
    let length = random.real(2.0, 20.0);
    let bearing = random.real(0.0, std::f64::consts::TAU);
    let side = if random.below(2) == 0 { 1.0 } else { -1.0 };
    let dimension = DimensionGeometry {
        start,
        end: Point2::new(
            start.x + length * bearing.cos(),
            start.y + length * bearing.sin(),
        ),
        offset: random.real(0.5, 4.0) * side,
        ..DimensionGeometry::default()
    };
    must(document.execute(create_dimension(dimension, on_layer(layer))));
}

/// A line a long way from the origin, so that the drawing is large beside the
/// windows asked about and the broad phase goes through the index for them.
#[track_caller]
fn far_line(document: &mut Document) -> EntityId {
    created(
        document,
        create_line(
            Point2::new(1000.0, 1000.0),
            Point2::new(1010.0, 1000.0),
            EntityAttributes::default(),
        ),
    )
}

/// Above the label of the worked dimension as first drawn, and within it once
/// its text is 25 high.
fn above_the_first_label() -> Box2 {
    Box2::new(Point2::new(4.0, 6.0), Point2::new(6.0, 27.0))
}

/// What the broad phase hands out for `reach`, through the document's index
/// or by its scan.
fn candidates(document: &Document, reach: Box2, through_the_index: bool) -> Vec<EntityId> {
    let index = through_the_index.then(|| document.spatial_index());
    let mut scratch = Vec::new();
    let mut visited = Vec::new();
    for_each_candidate(document.model(), index, reach, &mut scratch, |entity| {
        visited.push(entity.id);
    });
    visited
}

/// The exhaustive answer, worked out here from the model as it stands: every
/// entity whose box meets `reach`, ascending.
fn scanned(document: &Document, reach: Box2) -> Vec<EntityId> {
    let model = document.model();
    let mut found = Vec::new();
    model.entities.for_each(|entity| {
        if query_extents(model, entity).intersects(reach) {
            found.push(entity.id);
        }
    });
    found
}

/// The index, the broad phase through it and the broad phase's scan all give
/// the exhaustive answer for `reach`.
#[track_caller]
fn assert_answers_as_a_scan(document: &Document, reach: Box2, what: &str) {
    let expected = scanned(document, reach);
    assert_eq!(
        document.spatial_index().query(reach),
        expected,
        "the index, {what}"
    );
    assert_eq!(
        candidates(document, reach, true),
        expected,
        "the broad phase through the index, {what}"
    );
    assert_eq!(
        candidates(document, reach, false),
        expected,
        "the broad phase's scan, {what}"
    );
}

/// Every dimension's box as the model stands.
fn dimension_boxes(document: &Document) -> BTreeMap<EntityId, Box2> {
    let model = document.model();
    let mut boxes = BTreeMap::new();
    model.entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Dimension(_)) {
            boxes.insert(entity.id, query_extents(model, entity));
        }
    });
    boxes
}

/// A window that meets exactly one of two boxes that differ on a side.
struct Probe {
    window: Box2,
    /// The box it meets is `new`: the index must have gained the entity there.
    /// Otherwise it is `old`: the index must have let go of it.
    meets_new: bool,
}

/// For each side on which `old` and `new` differ by more than a rounding, the
/// strip between the two positions of that side, from half way out: it meets
/// the box that reaches further on that side and stops short of the other.
fn probes_between(old: Box2, new: Box2) -> Vec<Probe> {
    // Far more than the rounding of a label's corner, far less than any
    // change of text height made below.
    const MOVED: f64 = 1.0e-6;
    let mut probes = Vec::new();
    // The top and the right: the further box is the one with the greater
    // maximum.
    if (new.max.y - old.max.y).abs() > MOVED {
        let (further, nearer, meets_new) = if new.max.y > old.max.y {
            (new, old, true)
        } else {
            (old, new, false)
        };
        probes.push(Probe {
            window: Box2::new(
                Point2::new(further.min.x, 0.5 * (nearer.max.y + further.max.y)),
                Point2::new(further.max.x, further.max.y),
            ),
            meets_new,
        });
    }
    if (new.max.x - old.max.x).abs() > MOVED {
        let (further, nearer, meets_new) = if new.max.x > old.max.x {
            (new, old, true)
        } else {
            (old, new, false)
        };
        probes.push(Probe {
            window: Box2::new(
                Point2::new(0.5 * (nearer.max.x + further.max.x), further.min.y),
                Point2::new(further.max.x, further.max.y),
            ),
            meets_new,
        });
    }
    // The bottom and the left: the further box is the one with the lesser
    // minimum.
    if (new.min.y - old.min.y).abs() > MOVED {
        let (further, nearer, meets_new) = if new.min.y < old.min.y {
            (new, old, true)
        } else {
            (old, new, false)
        };
        probes.push(Probe {
            window: Box2::new(
                Point2::new(further.min.x, further.min.y),
                Point2::new(further.max.x, 0.5 * (nearer.min.y + further.min.y)),
            ),
            meets_new,
        });
    }
    if (new.min.x - old.min.x).abs() > MOVED {
        let (further, nearer, meets_new) = if new.min.x < old.min.x {
            (new, old, true)
        } else {
            (old, new, false)
        };
        probes.push(Probe {
            window: Box2::new(
                Point2::new(further.min.x, further.min.y),
                Point2::new(0.5 * (nearer.min.x + further.min.x), further.max.y),
            ),
            meets_new,
        });
    }
    probes
}

/// A command that breaks the contract of one: it moves a line and then
/// reports a failure, leaving the line where it put it.
struct MovesALineAndFails {
    id: EntityId,
    to: Point2,
}

impl Command for MovesALineAndFails {
    fn name(&self) -> &str {
        "MOVES_A_LINE_AND_FAILS"
    }

    fn validate(&self, _context: &CommandContext<'_>) -> Status {
        Ok(())
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let Some(mut entity) = context.model.entities.find(self.id).cloned() else {
            return Err(Error::new(ErrorCode::NotFound, "the line is not there"));
        };
        entity.geometry = Geometry::Line(Segment2::new(
            self.to,
            Point2::new(self.to.x + 10.0, self.to.y),
        ));
        context.model.entities.replace(entity)?;
        Err(Error::new(
            ErrorCode::Internal,
            "failed after the line was moved",
        ))
    }

    fn undo(&mut self, _context: &mut CommandContext<'_>) -> Status {
        Ok(())
    }

    fn redo(&mut self, _context: &mut CommandContext<'_>) -> Status {
        Ok(())
    }
}

mod index_tables {
    use super::*;

    #[test]
    fn a_dimension_restyled_larger_is_found_where_its_label_now_reaches() {
        let mut document = Document::default();
        far_line(&mut document);
        let dimension = worked_dimension(&mut document, "0");
        let window = above_the_first_label();

        // As first drawn the label stops at y = 5.125, below the window.
        assert_eq!(scanned(&document, window), Vec::<EntityId>::new());
        assert_answers_as_a_scan(&document, window, "as first drawn");

        // The style edited, and no entity: the label now reaches y = 27.625.
        must(document.execute(update_dimension_style(standard_with_text_height(25.0))));
        assert_eq!(scanned(&document, window), vec![dimension]);
        assert!(
            worth_indexing(document.spatial_index(), window),
            "the window must be one the broad phase asks the index about, or its scan is all \
             this tests"
        );
        assert_answers_as_a_scan(&document, window, "restyled larger");
    }

    #[test]
    fn undoing_and_redoing_a_dimension_style_edit_moves_the_boxes_with_it() {
        let mut document = Document::default();
        far_line(&mut document);
        let dimension = worked_dimension(&mut document, "0");
        let window = above_the_first_label();
        must(document.execute(update_dimension_style(standard_with_text_height(25.0))));
        assert_eq!(document.spatial_index().query(window), vec![dimension]);

        // Undone, the label is 2.5 high again: an index still holding the
        // large box would go on answering with the dimension.
        must(document.undo());
        assert_eq!(scanned(&document, window), Vec::<EntityId>::new());
        assert_answers_as_a_scan(&document, window, "after the undo");

        must(document.redo());
        assert_eq!(scanned(&document, window), vec![dimension]);
        assert_answers_as_a_scan(&document, window, "after the redo");
    }

    #[test]
    fn a_layer_given_another_dimension_style_moves_the_boxes_of_its_dimensions() {
        let mut document = Document::default();
        far_line(&mut document);
        add_layer(&mut document, "dims");
        add_layer(&mut document, "other");
        let on_dims = worked_dimension(&mut document, "dims");
        // The same dimension on a layer that keeps the built-in style: it
        // must stay out of the window throughout.
        let on_other = worked_dimension(&mut document, "other");
        let window = above_the_first_label();

        // A style nothing names yet moves nothing.
        must(document.execute(create_dimension_style(DimensionStyle {
            name: String::from("Tall"),
            text_height: 25.0,
            ..DimensionStyle::default()
        })));
        assert_eq!(scanned(&document, window), Vec::<EntityId>::new());
        assert_answers_as_a_scan(&document, window, "with the style made and not named");

        // The layer names it: a change of the layer table alone.
        let tall = layer_styled(&document, "dims", "Tall");
        must(document.execute(update_layer(tall)));
        assert_eq!(scanned(&document, window), vec![on_dims]);
        assert_answers_as_a_scan(&document, window, "with the layer naming the style");
        assert!(!scanned(&document, window).contains(&on_other));

        // A style a layer names is not deleted, and a refusal moves nothing.
        assert!(document.execute(delete_dimension_style("Tall")).is_err());
        assert_answers_as_a_scan(&document, window, "after the refused delete");

        // The layer back on the built-in style: the box is the small one again.
        let plain = layer_styled(&document, "dims", "");
        must(document.execute(update_layer(plain)));
        assert_eq!(scanned(&document, window), Vec::<EntityId>::new());
        assert_answers_as_a_scan(&document, window, "with the layer on the built-in style");
    }

    #[test]
    fn the_index_answers_as_a_scan_through_generated_edits_of_entities_and_tables() {
        // Dimensions on four layers and lines among them, then 96 steps: every
        // eighth an edit of the built-in dimension style (large and small by
        // turns), of the layer table (one layer's choice of style, on and off),
        // of a second style, a transaction that edits a style and draws a line,
        // its undo (and every other time its redo), a move, a new dimension
        // and a delete. After each, random windows; and after each that
        // moved a dimension's box without touching the dimension, the windows
        // that tell the box before from the box after.
        const LAYERS: [&str; 4] = ["0", "dims/a", "dims/b", "lines"];
        let mut document = Document::default();
        for name in &LAYERS[1..] {
            add_layer(&mut document, name);
        }
        must(document.execute(create_dimension_style(DimensionStyle {
            name: String::from("Wide"),
            text_height: 6.0,
            ..DimensionStyle::default()
        })));

        let mut random = Mt19937::new(20261121);
        for i in 0..60 {
            draw_dimension(&mut document, &mut random, LAYERS[i % LAYERS.len()]);
        }
        let lines: Vec<Entity> = (0..120)
            .map(|_| {
                let start = Point2::new(random.real(-200.0, 200.0), random.real(-200.0, 200.0));
                Entity {
                    geometry: Geometry::Line(Segment2::new(
                        start,
                        Point2::new(
                            start.x + random.real(-15.0, 15.0),
                            start.y + random.real(0.5, 15.0),
                        ),
                    )),
                    layer: String::from("lines"),
                    ..Entity::default()
                }
            })
            .collect();
        must(document.execute(create_entities(lines)));

        let mut gained = 0; // windows only the box AFTER a table edit meets
        let mut let_go = 0; // windows only the box BEFORE it meets
        let mut through_the_index = 0; // random windows the index answered, with something in them
        for round in 0..96 {
            let cycle = round / 8;
            let before = dimension_boxes(&document);
            // Whether this step can move a dimension's box without changing
            // the dimension.
            let mut a_table_step = true;
            match round % 8 {
                0 => {
                    // Large and small by turns, so that every such edit moves
                    // the label of every dimension drawn in the built-in
                    // style: a label 10 or more high is wider than any
                    // dimension here is long, and one 1.5 or less high is not.
                    let text_height = if cycle % 2 == 0 {
                        random.real(10.0, 12.0)
                    } else {
                        random.real(0.8, 1.5)
                    };
                    must(document.execute(update_dimension_style(
                        standard_with_text_height(text_height),
                    )));
                }
                1 => {
                    a_table_step = false;
                    let all = document.model().entities.ids();
                    let chosen: BTreeSet<EntityId> = (0..5)
                        .map(|_| all[random.below(all.len() as u32) as usize])
                        .collect();
                    let delta = Vec2::new(random.real(-20.0, 20.0), random.real(-20.0, 20.0));
                    must(document.execute(move_entities(chosen.into_iter().collect(), delta)));
                }
                2 => {
                    let style = if cycle % 2 == 0 { "Wide" } else { "" };
                    let layer = layer_styled(&document, "dims/a", style);
                    must(document.execute(update_layer(layer)));
                }
                3 => {
                    a_table_step = false;
                    draw_dimension(&mut document, &mut random, LAYERS[cycle % LAYERS.len()]);
                }
                4 => {
                    must(document.execute(update_dimension_style(DimensionStyle {
                        name: String::from("Wide"),
                        text_height: random.real(3.0, 9.0),
                        ..DimensionStyle::default()
                    })));
                }
                5 => {
                    // One step that changes a table AND an entity.
                    let mut both = Transaction::new("RESTYLE_AND_DRAW");
                    both.add(update_dimension_style(standard_with_text_height(
                        random.real(3.0, 6.0),
                    )));
                    both.add(create_line(
                        Point2::new(random.real(-200.0, 200.0), random.real(-200.0, 200.0)),
                        Point2::new(random.real(-200.0, 200.0), random.real(-200.0, 200.0)),
                        on_layer("lines"),
                    ));
                    must(document.execute(Box::new(both)));
                }
                6 => {
                    must(document.undo());
                    if cycle % 2 == 1 {
                        must(document.redo());
                    }
                }
                _ => {
                    a_table_step = false;
                    let model = document.model();
                    let mut first_line = None;
                    model.entities.for_each(|entity| {
                        if first_line.is_none() && matches!(entity.geometry, Geometry::Line(_)) {
                            first_line = Some(entity.id);
                        }
                    });
                    let doomed = first_line.expect("a line is left to delete");
                    must(document.execute(delete_entities(vec![doomed])));
                }
            }

            if a_table_step {
                let after = dimension_boxes(&document);
                // Forty of them a step: enough to cover every dimension of a
                // layer, and the scan each is compared with walks the drawing.
                let mut asked = 0;
                for (id, new) in &after {
                    let Some(old) = before.get(id) else {
                        continue;
                    };
                    for probe in probes_between(*old, *new) {
                        if asked == 40 {
                            break;
                        }
                        asked += 1;
                        let found = scanned(&document, probe.window);
                        assert_eq!(
                            found.contains(id),
                            probe.meets_new,
                            "round {round}: the window was made to meet only one of the boxes"
                        );
                        assert_answers_as_a_scan(
                            &document,
                            probe.window,
                            &format!("round {round}, a window beside dimension {id}"),
                        );
                        if probe.meets_new {
                            gained += 1;
                        } else {
                            let_go += 1;
                        }
                    }
                }
            }

            for trial in 0..10 {
                let x = random.real(-230.0, 230.0);
                let y = random.real(-230.0, 230.0);
                let width = random.real(1.0, 60.0);
                let height = random.real(1.0, 60.0);
                let window = Box2::new(Point2::new(x, y), Point2::new(x + width, y + height));
                assert_answers_as_a_scan(
                    &document,
                    window,
                    &format!("round {round} trial {trial}"),
                );
                if worth_indexing(document.spatial_index(), window)
                    && !scanned(&document, window).is_empty()
                {
                    through_the_index += 1;
                }
            }
            assert_eq!(
                document.spatial_index().size(),
                document.model().entities.size(),
                "round {round}"
            );
        }

        // Floors, not expectations: a run that stopped reaching the case would
        // pass with nothing compared. Fifteen dimensions are drawn on layer
        // "0", which keeps the built-in style throughout, and each of the
        // twelve edits of that style moves the label of every one of them -
        // six times outwards and six inwards - so each direction has at least
        // 6 x 15 = 90 windows before the cap of forty a step, and 6 x 40 =
        // 240 at most from those steps; a floor of 60 leaves room for both.
        assert!(
            gained >= 60,
            "windows only a restyled box meets: {gained}"
        );
        assert!(
            let_go >= 60,
            "windows only the box before the restyle meets: {let_go}"
        );
        // 960 random windows over a drawing 400 across, each up to 60: most
        // hold something and all are far under the share of the drawing at
        // which the broad phase scans instead.
        assert!(
            through_the_index >= 300,
            "random windows answered through the index: {through_the_index}"
        );
    }

    #[test]
    fn a_command_that_fails_after_changing_the_model_leaves_an_index_that_answers_as_a_scan() {
        let mut document = Document::default();
        // 1 000 lines 10 long, one above the other: the cell is chosen as 10
        // (twice the mean box side, 1000 * 10 / (2 * 1000) = 5).
        let lines: Vec<Entity> = (0..1000)
            .map(|i| {
                let y = f64::from(i);
                Entity {
                    geometry: Geometry::Line(Segment2::new(
                        Point2::new(0.0, y),
                        Point2::new(10.0, y),
                    )),
                    ..Entity::default()
                }
            })
            .collect();
        must(document.execute(create_entities(lines)));
        // One line 1 000 long, inserted without a rebuild: the cell stays 10.
        must(document.execute(create_line(
            Point2::new(0.0, 5000.0),
            Point2::new(1000.0, 5000.0),
            EntityAttributes::default(),
        )));
        assert_eq!(document.spatial_index().cell_size(), 10.0);
        let moved = *document
            .model()
            .entities
            .ids()
            .first()
            .expect("the first line");
        let was = Box2::new(Point2::new(-1.0, -0.5), Point2::new(11.0, 0.5));
        let now = Box2::new(Point2::new(499.0, 8999.0), Point2::new(511.0, 9001.0));
        assert_eq!(scanned(&document, was), vec![moved]);

        // Refused before it ran: the model cannot have changed, and the index
        // is left alone - not built again, which would have chosen the cell
        // again.
        assert!(document.execute(delete_entities(Vec::new())).is_err());
        assert_eq!(document.spatial_index().cell_size(), 10.0);

        // Failed after it ran, the line left at its new place.
        let broken: CommandPtr = Box::new(MovesALineAndFails {
            id: moved,
            to: Point2::new(500.0, 9000.0),
        });
        assert!(document.execute(broken).is_err());
        assert_eq!(scanned(&document, now), vec![moved]);
        assert_eq!(scanned(&document, was), Vec::<EntityId>::new());
        assert!(worth_indexing(document.spatial_index(), now));
        assert!(worth_indexing(document.spatial_index(), was));
        assert_answers_as_a_scan(&document, now, "where the failed command left the line");
        assert_answers_as_a_scan(&document, was, "where the line was");
        // Built again whole: the cell is what a rebuild chooses for these
        // 1 001 boxes, twice the mean side (1000 * 10 + 1000) / (2 * 1001).
        assert_eq!(
            document.spatial_index().cell_size(),
            11000.0 / (2.0 * 1001.0) * 2.0
        );
        // Nothing was pushed: there is still nothing but the two creates to undo.
        assert_eq!(document.history().undo_count(), 2);
    }
}
