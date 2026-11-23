//! The cad layer's passes over a drawing, measured (CLAUDE.md section 5; the practice of
//! docs/testing.md, "Benchmarks"; docs/performance.md, "The broad phase's second extent", "The
//! verbs' walks" and "Global Modify's layers"): what a repaint, a pick and a snap pay to narrow a
//! query to its candidates, what the survey and utility verbs pay to walk the drawing, and what
//! a scope and a filter pay to say what they take.
//!
//! The drawing is generated here, in a Document, as an import makes one: 100 000 entities on a
//! grid 2.5 units apart over 200 layers named as a survey's are ("survey/code017"), in runs of 25
//! on a layer. One in a hundred is a coded survey point (1 000 points, 250 strings of four, one
//! of them joined to another by number), one in fifty a dimension, and the rest are lines and
//! polylines of 12 vertices by turns. Only public members that the base build has are used, so
//! this source measures the base build and the changed one alike. Cells, alternated round by
//! round with the order reversed every round, each round taking `REPEATS` samples of each:
//!
//! * `candidates_view` and `candidates_view_again`: `for_each_candidate` through the document's
//!   index for 200 windows of a hundredth of the drawing's area (a zoomed-in repaint), under two
//!   names, the A/A control;
//! * `candidates_cursor`: the same for 20 000 windows 2 units square (a pick or a snap);
//! * `linework`: `process_linework` of the whole drawing with joins and
//!   `skip_lines_already_drawn`, which walks the drawing three times to plan 250 lines;
//! * `survey_codes`: `apply_survey_codes` of the whole drawing, planned and not executed;
//! * `code_census`: `code_census` of the whole drawing;
//! * `utility_read`: `read_utility_data` of a drawing that holds no utility data: the walk alone;
//! * `match_layers`: `match_entities` for the layers under "survey" that two patterns take,
//!   drawn only;
//! * `match_view`: `match_entities` for a view that hides two layers of its own;
//! * `match_drawing`: `match_entities` for the whole drawing with no filter, where no layer is
//!   asked anything: the control, which must not move.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=... count=...` line a cell; `count` is
//! what the cell found (candidates visited, lines planned, entities coded or matched) and must
//! be the same in both builds. Run in Release:
//! `cargo bench --locked -p katana_cad --bench drawing_passes` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7).

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use katana_cad::code_table::code_census;
use katana_cad::document::Document;
use katana_cad::global_modify::{ModifyFilter, ModifyScope, ScopeKind, match_entities};
use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::linework::{LineworkOptions, process_linework};
use katana_cad::spatial_query::for_each_candidate;
use katana_cad::survey_coding::{SurveyCodingOptions, SurveyCodingReport, apply_survey_codes};
use katana_cad::utilities::utility_data::read_utility_data;
use katana_commands::{Transaction, create_entities, create_layer};
use katana_entity::{
    DimensionGeometry, Entity, Geometry, Layer, PointGeometry, PropertyValue, SurveyBreakline,
    SurveyMap, SurveyRule,
};
use katana_geometry::{Box2, Point2, Polyline2, Segment2, SpatialId};

const ENTITIES: usize = 100_000;
const LAYERS: usize = 200;
const COLUMNS: usize = 400;
const SPACING: f64 = 2.5;
const SITE_WIDTH: f64 = COLUMNS as f64 * SPACING;
const SITE_HEIGHT: f64 = (ENTITIES / COLUMNS) as f64 * SPACING;
const VIEW_QUERIES: usize = 200;
const CURSOR_QUERIES: usize = 20_000;
const REPEATS: usize = 3;

/// Knuth's MMIX linear congruential generator (The Art of Computer Programming, volume 2,
/// section 3.3.4, table 1, line 26), as a real in [0, 1): the same windows on every machine.
struct Sequence(u64);

impl Sequence {
    fn real(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn layer_name(layer: usize) -> String {
    format!("survey/code{layer:03}")
}

/// The layer of entity `index`: runs of 25 on a layer, as a survey's strings come, the runs
/// spread over every layer.
fn layer_of(index: usize) -> usize {
    (index / 25 * 7) % LAYERS
}

fn entity(index: usize) -> Entity {
    let x = (index % COLUMNS) as f64 * SPACING;
    let y = (index / COLUMNS) as f64 * SPACING;
    let mut entity = Entity {
        layer: layer_name(layer_of(index)),
        ..Entity::default()
    };
    if index.is_multiple_of(100) {
        // A surveyed point as a survey import writes one: strings of four, numbered in order.
        let number = index / 100;
        let mut code = format!("KB{:03}", number / 4);
        if number == 0 {
            // One join by point number, so the run looks every point up by its number.
            code += " JPN 3";
        }
        entity.geometry = Geometry::Point(PointGeometry {
            position: Point2::new(x, y),
        });
        entity
            .properties
            .insert(String::from("code"), PropertyValue::from(code));
        entity.properties.insert(
            String::from("point"),
            PropertyValue::from(format!("{}", number + 1)),
        );
    } else if index % 50 == 7 {
        entity.geometry = Geometry::Dimension(DimensionGeometry::new(
            Point2::new(x, y),
            Point2::new(x + 2.0, y),
            0.5,
            "",
        ));
    } else if index.is_multiple_of(2) {
        entity.geometry = Geometry::Line(Segment2::new(
            Point2::new(x, y),
            Point2::new(x + 2.0, y + 1.0),
        ));
    } else {
        let vertices = (0..12)
            .map(|vertex| Point2::new(x + f64::from(vertex) * 0.2, y + f64::from(vertex % 2) * 0.8))
            .collect();
        entity.geometry = Geometry::Polyline(Polyline2::new(vertices, false));
    }
    entity
}

fn drawing() -> Document {
    let mut document = Document::default();
    let mut map = SurveyMap::default();
    map.add(SurveyRule {
        key: String::from("KB*"),
        model: String::from("SURVEY KERB"),
        breakline: Some(SurveyBreakline::Line),
        ..SurveyRule::default()
    })
    .expect("the rule is added");
    document.set_survey_map(map);

    let mut layers = Transaction::new("LAYERS");
    for layer in 0..LAYERS {
        layers.add(create_layer(Layer {
            name: layer_name(layer),
            ..Layer::default()
        }));
    }
    document
        .execute(Box::new(layers))
        .expect("the layers are created");

    // What a file import executes: one transaction holding one bulk create.
    let mut import = Transaction::new("IMPORT");
    import.add(create_entities((0..ENTITIES).map(entity).collect()));
    document
        .execute(Box::new(import))
        .expect("the entities are created");
    document
}

fn windows(count: usize, width: f64, height: f64, seed: u64) -> Vec<Box2> {
    let mut sequence = Sequence(seed);
    (0..count)
        .map(|_| {
            let east = sequence.real() * (SITE_WIDTH - width);
            let north = sequence.real() * (SITE_HEIGHT - height);
            Box2::new(
                Point2::new(east, north),
                Point2::new(east + width, north + height),
            )
        })
        .collect()
}

struct Inputs {
    document: Document,
    views: Vec<Box2>,
    cursors: Vec<Box2>,
    layers_scope: ModifyScope,
    layers_filter: ModifyFilter,
    view_scope: ModifyScope,
    drawing_scope: ModifyScope,
    no_filter: ModifyFilter,
}

fn inputs() -> Inputs {
    let mut hidden = LayerOverrides::default();
    hidden.hide(&layer_name(3));
    hidden.hide(&layer_name(150));
    Inputs {
        document: drawing(),
        // A tenth of the drawing each way: a hundredth of its area.
        views: windows(VIEW_QUERIES, SITE_WIDTH / 10.0, SITE_HEIGHT / 10.0, 7),
        cursors: windows(CURSOR_QUERIES, 2.0, 2.0, 11),
        layers_scope: ModifyScope {
            kind: ScopeKind::Layers,
            layers: vec![String::from("survey")],
            sublayers: true,
            ..ModifyScope::default()
        },
        layers_filter: ModifyFilter {
            layers: vec![String::from("survey/code0*"), String::from("*/code1?5")],
            drawn_only: true,
            ..ModifyFilter::default()
        },
        view_scope: ModifyScope {
            kind: ScopeKind::View,
            view: Some(Arc::new(hidden)),
            ..ModifyScope::default()
        },
        drawing_scope: ModifyScope {
            kind: ScopeKind::Drawing,
            ..ModifyScope::default()
        },
        no_filter: ModifyFilter::default(),
    }
}

/// One sample of `cell`: its milliseconds, and what it found.
fn sample(cell: &str, inputs: &Inputs) -> (f64, usize) {
    let document = &inputs.document;
    let model = document.model();
    let started;
    let found = match cell {
        "linework" => {
            let options = LineworkOptions {
                skip_lines_already_drawn: true,
                ..LineworkOptions::default()
            };
            started = Instant::now();
            let planned =
                process_linework(black_box(document), &options).expect("the lines are planned");
            planned.report.strings.len()
        }
        "survey_codes" => {
            let options = SurveyCodingOptions::default();
            let mut report = SurveyCodingReport::default();
            started = Instant::now();
            let planned = apply_survey_codes(black_box(document), &options, Some(&mut report))
                .expect("the codes are planned");
            black_box(planned.is_some());
            report.coded
        }
        "code_census" => {
            started = Instant::now();
            code_census(black_box(document), "").coded
        }
        "utility_read" => {
            started = Instant::now();
            let read = read_utility_data(black_box(model), &[]).expect("nothing is read");
            read.services.len()
        }
        "match_layers" => {
            started = Instant::now();
            match_entities(document, &inputs.layers_scope, &inputs.layers_filter)
                .expect("the scope is resolved")
                .len()
        }
        "match_view" => {
            started = Instant::now();
            match_entities(document, &inputs.view_scope, &inputs.no_filter)
                .expect("the scope is resolved")
                .len()
        }
        "match_drawing" => {
            started = Instant::now();
            match_entities(document, &inputs.drawing_scope, &inputs.no_filter)
                .expect("the scope is resolved")
                .len()
        }
        _ => {
            let queries = if cell == "candidates_cursor" {
                &inputs.cursors
            } else {
                &inputs.views
            };
            let index = Some(document.spatial_index());
            // The caller's buffer, kept between queries as a mouse-move path keeps its own.
            let mut scratch: Vec<SpatialId> = Vec::new();
            let mut visited = 0;
            started = Instant::now();
            for query in queries {
                for_each_candidate(model, index, black_box(*query), &mut scratch, |entity| {
                    visited += 1;
                    black_box(entity.id);
                });
            }
            visited
        }
    };
    let taken = started.elapsed().as_secs_f64() * 1000.0;
    (taken, black_box(found))
}

fn main() {
    let rounds: usize = std::env::var("KATANA_BENCH_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(7);
    // `cargo test --release` runs a bench target as a test with `--bench` absent: once, quickly.
    let quick = !std::env::args().any(|argument| argument == "--bench");
    let rounds = if quick { 1 } else { rounds.max(1) };
    let inputs = inputs();
    let mut cells = [
        "candidates_view",
        "candidates_cursor",
        "linework",
        "survey_codes",
        "code_census",
        "utility_read",
        "match_layers",
        "match_view",
        "match_drawing",
        "candidates_view_again",
    ];
    let mut samples: Vec<(&str, Vec<f64>, usize)> =
        cells.iter().map(|cell| (*cell, Vec::new(), 0)).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let (taken, found) = sample(cell, &inputs);
                if let Some((_, kept, count)) =
                    samples.iter_mut().find(|(name, _, _)| *name == cell)
                {
                    kept.push(taken);
                    *count = found;
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    println!(
        "entities={} layers={LAYERS} indexed={}",
        inputs.document.model().entities.size(),
        inputs.document.spatial_index().size()
    );
    for (cell, mut kept, count) in samples {
        kept.sort_by(f64::total_cmp);
        let median = kept[kept.len() / 2];
        println!(
            "cell={cell} min_ms={:.3} median_ms={:.3} samples={} count={count}",
            kept[0],
            median,
            kept.len()
        );
    }
}
