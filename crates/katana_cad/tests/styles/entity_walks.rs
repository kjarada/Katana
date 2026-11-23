// The survey verbs' walks of the whole drawing (docs/performance.md, "The
// verbs' walks"; no C++ twin).
//
// Linework, survey coding and the code census each take either every entity
// of the drawing or the entities named. "Every entity" used to be a list of
// every id handed to the same lookups the named path makes; it is now one
// pass over the entities. The named path is unchanged, so it is the oracle:
// on a generated drawing with gaps in its ids, what the verb makes of the
// whole drawing must be, record for record and in the same order, what it
// makes of every id named - and the drawings the two commands leave must be
// the same drawing. The counts each test also asserts are tallied here from
// the entities as they were generated, not read back from a report.

use std::collections::{BTreeMap, BTreeSet};

use katana_cad::code_table::{code_census, code_census_of};
use katana_cad::linework::process_linework;
use katana_cad::survey_coding::apply_survey_codes;
use katana_cad::{Document, LineworkOptions, SurveyCodingOptions, SurveyCodingReport};
use katana_commands::{create_entities, create_layer, delete_entities, set_entity_layer};
use katana_core::Status;
use katana_entity::{
    Entity, EntityId, Geometry, Layer, PointGeometry, PropertyValue, SurveyBreakline, SurveyMap,
    SurveyRule,
};
use katana_geometry::{Point2, Polyline2, Segment2};

const GENERATED: usize = 2000;

#[track_caller]
fn must(status: Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

// A kerb is a line and a tree is a point, and each rule names a layer: coding
// the drawing moves every coded entity, so the two drawings compared below
// are compared after a change of every coded entity, not of none.
fn kerbs_and_trees() -> SurveyMap {
    let mut map = SurveyMap::default();
    for (key, model, breakline) in [
        ("KB*", "SURVEY KERB", SurveyBreakline::Line),
        ("TR*", "SURVEY DETAIL", SurveyBreakline::Point),
    ] {
        assert!(
            map.add(SurveyRule {
                key: String::from(key),
                model: String::from(model),
                breakline: Some(breakline),
                ..SurveyRule::default()
            })
            .is_ok()
        );
    }
    map
}

fn surveyed(at: Point2, code: &str, number: usize) -> Entity {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    if !code.is_empty() {
        entity
            .properties
            .insert(String::from("code"), PropertyValue::from(code));
    }
    entity.properties.insert(
        String::from("point"),
        PropertyValue::from(format!("{number}")),
    );
    entity
}

/// The code generated entity `index` carries; empty for none. A fifth are
/// kerb points in strings of ten ("KB00" to "KB39"), a fifth trees, and the
/// rest - uncoded points, lines and polylines - carry none. The first kerb
/// point is joined to point 3, a tree.
fn code_of(index: usize) -> String {
    match index % 5 {
        1 if index == 1 => String::from("KB00 JPN 3"),
        1 => format!("KB{:02}", index / 50),
        2 => String::from("TR1"),
        _ => String::new(),
    }
}

fn is_point(index: usize) -> bool {
    matches!(index % 5, 1..=3)
}

/// Generated entity `index`, which is given the id `index + 1`, numbered by
/// its id when it is a point.
fn generated(index: usize) -> Entity {
    let x = (index % 50) as f64 * 3.0;
    let y = (index / 50) as f64 * 3.0;
    let at = Point2::new(x, y);
    match index % 5 {
        0 => Entity {
            geometry: Geometry::Line(Segment2::new(at, Point2::new(x + 1.0, y + 2.0))),
            ..Entity::default()
        },
        4 => Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                vec![at, Point2::new(x + 1.0, y), Point2::new(x + 1.0, y + 1.0)],
                false,
            )),
            ..Entity::default()
        },
        _ => surveyed(at, &code_of(index), index + 1),
    }
}

/// The ids deleted after the drawing is made, so that the ids have gaps: 20
/// of them, every 97th.
fn deleted() -> BTreeSet<EntityId> {
    (1..=GENERATED as EntityId)
        .filter(|id| id.is_multiple_of(97))
        .collect()
}

/// The generated indices the drawing still holds.
fn kept() -> Vec<usize> {
    let gone = deleted();
    (0..GENERATED)
        .filter(|index| !gone.contains(&(*index as EntityId + 1)))
        .collect()
}

fn drawing() -> Document {
    let mut document = Document::default();
    document.set_survey_map(kerbs_and_trees());
    must(document.execute(create_entities((0..GENERATED).map(generated).collect())));
    must(document.execute(delete_entities(deleted().into_iter().collect())));
    document
}

/// Every id of the drawing from the highest down: named in an order that is
/// not entity order.
fn every_id_descending(document: &Document) -> Vec<EntityId> {
    let mut ids = document.model().entities.ids();
    ids.reverse();
    ids
}

fn snapshot(document: &Document) -> BTreeMap<EntityId, Entity> {
    let mut entities = BTreeMap::new();
    document.model().entities.for_each(|entity| {
        entities.insert(entity.id, entity.clone());
    });
    entities
}

mod entity_walks {
    use super::*;

    #[test]
    fn linework_of_the_whole_drawing_is_the_linework_of_every_id_named() {
        let mut whole_drawing = drawing();
        let mut every_id = drawing();
        let kept = kept();
        assert_eq!(whole_drawing.model().entities.size(), GENERATED - 20);

        // Joins and the lines already drawn: the options under which the run
        // walks the drawing three times.
        let whole_options = LineworkOptions {
            skip_lines_already_drawn: true,
            ..LineworkOptions::default()
        };
        let named_options = LineworkOptions {
            ids: every_id_descending(&every_id),
            ..whole_options.clone()
        };
        let mut whole = process_linework(&whole_drawing, &whole_options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut named = process_linework(&every_id, &named_options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        // Tallied from what was generated.
        let points = kept.iter().filter(|index| is_point(**index)).count();
        assert_eq!(whole.report.considered, points);
        assert_eq!(whole.report.not_points, kept.len() - points);
        // Forty strings of kerb points, each still of several points, and
        // the one join.
        assert_eq!(whole.report.strings.len(), 41);
        assert!(whole.report.strings.iter().any(|string| string.join));

        assert_eq!(whole.report.property, named.report.property);
        assert_eq!(whole.report.considered, named.report.considered);
        assert_eq!(whole.report.not_points, named.report.not_points);
        assert_eq!(whole.report.strings, named.report.strings);
        assert_eq!(whole.report.already_drawn, named.report.already_drawn);
        assert_eq!(whole.report.unplaced, named.report.unplaced);
        assert_eq!(whole.report.notes, named.report.notes);
        assert_eq!(whole.report.layers_created, named.report.layers_created);
        assert_eq!(whole.report.points_removed, named.report.points_removed);
        // In entity order: the unplaced points ascend by id.
        assert!(
            whole
                .report
                .unplaced
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
        );

        // And the two commands leave the same drawing, id for id.
        must(whole_drawing.execute(whole.command.take().expect("lines to draw")));
        must(every_id.execute(named.command.take().expect("lines to draw")));
        assert_eq!(snapshot(&whole_drawing), snapshot(&every_id));
        assert_eq!(
            whole.report.styling_report(),
            named.report.styling_report()
        );

        // Run again over the lines just drawn, which the walk for the lines
        // already drawn now finds: the same by the walk of the whole drawing
        // as by the ids named, every line planned either drawn or found.
        let whole_again = process_linework(&whole_drawing, &whole_options)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let named_again = process_linework(
            &every_id,
            &LineworkOptions {
                ids: every_id_descending(&every_id),
                ..whole_options.clone()
            },
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(!whole_again.report.already_drawn.is_empty());
        assert_eq!(
            whole_again.report.already_drawn.len() + whole_again.report.strings.len(),
            41
        );
        assert_eq!(
            whole_again.report.already_drawn,
            named_again.report.already_drawn
        );
        assert_eq!(whole_again.report.strings, named_again.report.strings);
    }

    #[test]
    fn a_line_the_drawing_holds_twice_is_reported_as_the_one_of_the_lower_id() {
        let mut document = Document::default();
        document.set_survey_map(kerbs_and_trees());
        must(document.execute(create_entities(
            (0..3)
                .map(|i| surveyed(Point2::new(f64::from(i) * 5.0, 0.0), "KB1", i as usize + 1))
                .collect(),
        )));
        let mut first = process_linework(&document, &LineworkOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        must(document.execute(first.command.take().expect("a line to draw")));
        // Points 1 to 3, so the line is entity 4; a copy of it is entity 5.
        let line = document
            .model()
            .entities
            .find(4)
            .cloned()
            .expect("the line drawn");
        assert!(matches!(line.geometry, Geometry::Polyline(_)));
        must(document.execute(create_entities(vec![line])));
        assert!(document.model().entities.contains(5));
        for name in ["first", "second"] {
            must(document.execute(create_layer(Layer {
                name: String::from(name),
                ..Layer::default()
            })));
        }
        let asked = LineworkOptions {
            skip_lines_already_drawn: true,
            ..LineworkOptions::default()
        };

        // The drawing's coded lines are filed in entity order and the first
        // that matches is the one reported: entity 4, whatever layer it is on.
        must(document.execute(set_entity_layer(vec![4], "first")));
        must(document.execute(set_entity_layer(vec![5], "second")));
        let told = process_linework(&document, &asked)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(told.report.already_drawn.len(), 1);
        assert_eq!(told.report.already_drawn[0].layer, "first");

        must(document.execute(set_entity_layer(vec![4], "second")));
        must(document.execute(set_entity_layer(vec![5], "first")));
        let told = process_linework(&document, &asked)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(told.report.already_drawn.len(), 1);
        assert_eq!(told.report.already_drawn[0].layer, "second");
    }

    #[test]
    fn survey_codes_of_the_whole_drawing_are_those_of_every_id_named_in_entity_order() {
        let mut whole_drawing = drawing();
        let mut every_id = drawing();
        let kept = kept();

        let mut whole_report = SurveyCodingReport::default();
        let whole = apply_survey_codes(
            &whole_drawing,
            &SurveyCodingOptions::default(),
            Some(&mut whole_report),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        // Named in entity order: the named path keeps the order it is given.
        let mut named_report = SurveyCodingReport::default();
        let named = apply_survey_codes(
            &every_id,
            &SurveyCodingOptions {
                ids: every_id.model().entities.ids(),
                ..SurveyCodingOptions::default()
            },
            Some(&mut named_report),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));

        // Tallied from what was generated.
        let coded = kept
            .iter()
            .filter(|index| !code_of(**index).is_empty())
            .count();
        assert_eq!(whole_report.property, "code");
        assert_eq!(whole_report.coded, coded);
        assert_eq!(whole_report, named_report);

        // Every coded entity is moved to its rule's layer: the same entities,
        // to the same layers, in both drawings.
        assert_eq!(whole_report.changed, coded);
        must(whole_drawing.execute(whole.expect("entities to code")));
        must(every_id.execute(named.expect("entities to code")));
        assert_eq!(snapshot(&whole_drawing), snapshot(&every_id));
        let moved = whole_drawing
            .model()
            .entities
            .ids_on_layer("SURVEY KERB")
            .len()
            + whole_drawing
                .model()
                .entities
                .ids_on_layer("SURVEY DETAIL")
                .len();
        assert_eq!(moved, coded);
    }

    #[test]
    fn the_census_of_the_drawing_is_the_census_of_every_id() {
        let document = drawing();
        let kept = kept();
        let whole = code_census(&document, "");
        let named = code_census_of(&document, &document.model().entities.ids(), "");

        // Tallied from what was generated: how many carry a code, and how
        // many carry each.
        let mut carried: BTreeMap<String, usize> = BTreeMap::new();
        for index in &kept {
            let code = code_of(*index);
            if !code.is_empty() {
                *carried.entry(code).or_default() += 1;
            }
        }
        assert_eq!(whole.property, "code");
        assert_eq!(whole.coded, carried.values().sum::<usize>());
        let counted: BTreeMap<String, usize> = whole
            .codes
            .iter()
            .map(|row| (row.code.clone(), row.entities))
            .collect();
        assert_eq!(counted, carried);
        // One row a code, in code order.
        assert_eq!(whole.codes.len(), carried.len());
        assert!(whole.codes.windows(2).all(|pair| pair[0].code < pair[1].code));

        assert_eq!(whole, named);
    }
}
