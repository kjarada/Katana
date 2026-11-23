//! What a scope and a filter take, layer by layer (docs/performance.md, "Global Modify's
//! layers"; no C++ twin).
//!
//! match_entities asks the layer's half of a match once for a run of entities on one layer and
//! remembers the answer. A remembered answer is only right if it is the answer the rule gives
//! every entity, so the rule is written out again here, entity by entity and with nothing
//! remembered - from the layers and entities as this file generates them, not from the
//! program's own layer table - and every scope and filter that asks anything of a layer must
//! take exactly what it takes: the same ids, ascending.
//!
//! The drawing has both shapes the memory meets: its first half in runs of twenty on a layer
//! (every answer but one in twenty remembered), its second half with no two neighbours on one
//! layer (none remembered). The layers nest and share prefixes of TEXT ("survey", "survey 2",
//! "surveyor"), so that a sublayer rule that compared text would be caught; one branch is
//! switched off in the drawing, and one entity in eleven is switched off itself.

use std::sync::Arc;

use katana_cad::global_modify::{match_entities, matches_pattern};
use katana_cad::{Document, LayerOverrides, ModifyFilter, ModifyScope, ScopeKind};
use katana_commands::{create_entities, create_layer, update_layer};
use katana_entity::{Entity, EntityId, EntityType, Geometry, Layer, PointGeometry};
use katana_geometry::{Box2, Point2, Segment2};

use crate::must;

const LAYERS: [&str; 9] = [
    "0",
    "survey",
    "survey/kerb",
    "survey/kerb/top",
    "survey 2",
    "surveyor",
    "roads",
    "roads/kerb",
    "design/a",
];
const GENERATED: usize = 1200;

/// One generated entity, as this file made it.
struct Made {
    id: EntityId,
    layer: &'static str,
    visible: bool,
    point: bool,
    x: f64,
    y: f64,
}

fn made(index: usize) -> Made {
    Made {
        id: index as EntityId + 1,
        layer: if index < GENERATED / 2 {
            LAYERS[(index / 20) % LAYERS.len()]
        } else {
            // Four and nine share no factor: each entity is on another layer
            // than the one before it.
            LAYERS[(index * 4) % LAYERS.len()]
        },
        visible: !index.is_multiple_of(11),
        point: index.is_multiple_of(2),
        x: (index % 40) as f64 * 2.0,
        y: (index / 40) as f64 * 2.0,
    }
}

fn drawing() -> (Document, Vec<Made>) {
    let mut document = Document::default();
    for name in &LAYERS[1..] {
        must(document.execute(create_layer(Layer {
            name: String::from(*name),
            ..Layer::default()
        })));
    }
    // "roads" switched off, and "roads/kerb" with it.
    let mut roads = document
        .model()
        .layers
        .find("roads")
        .cloned()
        .expect("the layer roads");
    roads.visible = false;
    must(document.execute(update_layer(roads)));

    let all: Vec<Made> = (0..GENERATED).map(made).collect();
    let entities = all
        .iter()
        .map(|made| Entity {
            geometry: if made.point {
                Geometry::Point(PointGeometry {
                    position: Point2::new(made.x, made.y),
                })
            } else {
                Geometry::Line(Segment2::new(
                    Point2::new(made.x, made.y),
                    Point2::new(made.x + 1.0, made.y + 1.0),
                ))
            },
            layer: String::from(made.layer),
            visible: made.visible,
            ..Entity::default()
        })
        .collect();
    must(document.execute(create_entities(entities)));
    (document, all)
}

// ---- the rule, written out again ---------------------------------------------------------

/// `layer` is `name` or lies beneath it: whole path segments, not text.
fn within(layer: &str, name: &str) -> bool {
    layer == name
        || layer
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The drawing shows the layer: the one branch switched off is "roads".
fn shown(layer: &str) -> bool {
    !within(layer, "roads")
}

/// The entity is drawn in a view that hides `hidden` of its own.
fn drawn(made: &Made, hidden: &[&str]) -> bool {
    made.visible && shown(made.layer) && !hidden.iter().any(|name| within(made.layer, name))
}

/// The entity's box meets `area`: a point is its place, a line the square it
/// crosses.
fn meets(made: &Made, area: Box2) -> bool {
    let reach = if made.point { 0.0 } else { 1.0 };
    made.x <= area.max.x
        && made.x + reach >= area.min.x
        && made.y <= area.max.y
        && made.y + reach >= area.min.y
}

fn any_pattern(layer: &str, patterns: &[&str]) -> bool {
    patterns
        .iter()
        .any(|pattern| matches_pattern(layer, pattern))
}

fn patterns(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| String::from(*name)).collect()
}

fn layers_scope(names: &[&str], sublayers: bool) -> ModifyScope {
    ModifyScope {
        kind: ScopeKind::Layers,
        layers: patterns(names),
        sublayers,
        ..ModifyScope::default()
    }
}

fn scope_of(kind: ScopeKind) -> ModifyScope {
    ModifyScope {
        kind,
        ..ModifyScope::default()
    }
}

/// What the rule takes of `all`, ascending - they were generated so.
fn taken(all: &[Made], rule: impl Fn(&Made) -> bool) -> Vec<EntityId> {
    all.iter()
        .filter(|made| rule(made))
        .map(|made| made.id)
        .collect()
}

/// match_entities takes exactly `expected`, which must be some of the drawing
/// and not all of it: a case that took nothing, or everything, would hold
/// whatever the layers' answers were.
#[track_caller]
fn assert_takes(
    document: &Document,
    scope: &ModifyScope,
    filter: &ModifyFilter,
    expected: &[EntityId],
    what: &str,
) {
    assert!(!expected.is_empty(), "{what}: the case takes nothing");
    assert!(expected.len() < GENERATED, "{what}: the case takes everything");
    let matched = match_entities(document, scope, filter)
        .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
    assert_eq!(matched, expected, "{what}");
}

mod global_modify_layers {
    use super::*;

    #[test]
    fn a_layers_scope_takes_what_the_rule_takes_entity_by_entity() {
        let (document, all) = drawing();
        let none = ModifyFilter::default();

        // With sublayers: "survey" takes "survey/kerb" and "survey/kerb/top",
        // and neither "survey 2" nor "surveyor".
        assert_takes(
            &document,
            &layers_scope(&["survey"], true),
            &none,
            &taken(&all, |made| within(made.layer, "survey")),
            "survey and beneath",
        );
        // Without: the layer alone.
        assert_takes(
            &document,
            &layers_scope(&["survey"], false),
            &none,
            &taken(&all, |made| made.layer == "survey"),
            "survey alone",
        );
        // Two layers, and a pattern over what they hold.
        let kerbs = ModifyFilter {
            layers: patterns(&["*kerb*"]),
            ..ModifyFilter::default()
        };
        assert_takes(
            &document,
            &layers_scope(&["survey/kerb", "roads"], true),
            &kerbs,
            &taken(&all, |made| {
                (within(made.layer, "survey/kerb") || within(made.layer, "roads"))
                    && any_pattern(made.layer, &["*kerb*"])
            }),
            "two branches, kerbs only",
        );
        // The same without sublayers: "roads" itself matches no kerb.
        assert_takes(
            &document,
            &layers_scope(&["survey/kerb", "roads"], false),
            &kerbs,
            &taken(&all, |made| made.layer == "survey/kerb"),
            "two layers alone, kerbs only",
        );
    }

    #[test]
    fn layer_patterns_and_drawn_only_take_what_the_rule_takes_entity_by_entity() {
        let (document, all) = drawing();
        let names = ["survey*", "roads/?erb", "DESIGN/A"];

        let filter = ModifyFilter {
            layers: patterns(&names),
            ..ModifyFilter::default()
        };
        assert_takes(
            &document,
            &scope_of(ScopeKind::Drawing),
            &filter,
            &taken(&all, |made| any_pattern(made.layer, &names)),
            "patterns over the drawing",
        );

        // Drawn only: the entity's own switch, and the layer's with its
        // parents'.
        let drawn_filter = ModifyFilter {
            layers: patterns(&names),
            drawn_only: true,
            ..ModifyFilter::default()
        };
        let expected = taken(&all, |made| {
            any_pattern(made.layer, &names) && drawn(made, &[])
        });
        assert!(
            !expected
                .iter()
                .any(|id| all[*id as usize - 1].layer == "roads/kerb"),
            "a layer under one switched off is not drawn"
        );
        assert_takes(
            &document,
            &scope_of(ScopeKind::Drawing),
            &drawn_filter,
            &expected,
            "patterns, drawn only",
        );

        // Drawn only and nothing else: no pattern, so the layer is asked only
        // whether it is shown.
        let only_drawn = ModifyFilter {
            drawn_only: true,
            ..ModifyFilter::default()
        };
        assert_takes(
            &document,
            &scope_of(ScopeKind::Drawing),
            &only_drawn,
            &taken(&all, |made| drawn(made, &[])),
            "drawn only",
        );
    }

    #[test]
    fn a_view_takes_what_the_rule_takes_entity_by_entity() {
        let (document, all) = drawing();
        // The view hides a branch and a parent that is not itself a layer
        // anything is on.
        let hides = ["survey/kerb", "design"];
        let mut hidden = LayerOverrides::default();
        for name in hides {
            hidden.hide(name);
        }
        let area = Box2::new(Point2::new(10.0, 10.0), Point2::new(40.0, 30.0));
        let view = ModifyScope {
            kind: ScopeKind::View,
            view: Some(Arc::new(hidden)),
            ..ModifyScope::default()
        };
        assert_takes(
            &document,
            &view,
            &ModifyFilter::default(),
            &taken(&all, |made| drawn(made, &hides)),
            "a view that hides two branches",
        );

        // Only what lies in its visible area, and only the points.
        let in_area = ModifyScope {
            area: Some(area),
            ..view.clone()
        };
        let mut points = ModifyFilter::default();
        points.types.insert(EntityType::Point);
        assert_takes(
            &document,
            &in_area,
            &points,
            &taken(&all, |made| {
                drawn(made, &hides) && meets(made, area) && made.point
            }),
            "the points in the view's area",
        );

        // A view that hides nothing of its own draws by the drawing's rule.
        assert_takes(
            &document,
            &scope_of(ScopeKind::View),
            &ModifyFilter::default(),
            &taken(&all, |made| drawn(made, &[])),
            "a view that hides nothing",
        );
    }

    #[test]
    fn a_selection_is_filtered_by_layer_as_the_rule_filters_it() {
        let (mut document, all) = drawing();
        // Every third entity, given from the last to the first.
        let mut chosen: Vec<EntityId> = all.iter().map(|made| made.id).step_by(3).collect();
        chosen.reverse();
        document.selection_mut().set(chosen);

        // A pattern in another case than the layers are named in.
        let filter = ModifyFilter {
            layers: patterns(&["SURVEY/*"]),
            drawn_only: true,
            ..ModifyFilter::default()
        };
        assert_takes(
            &document,
            &scope_of(ScopeKind::Selection),
            &filter,
            &taken(&all, |made| {
                (made.id - 1).is_multiple_of(3)
                    && (made.layer == "survey/kerb" || made.layer == "survey/kerb/top")
                    && made.visible
            }),
            "the selection on the layers beneath survey",
        );
    }

    #[test]
    fn a_scope_that_asks_nothing_of_a_layer_takes_every_entity() {
        // The one request for which the layer's half is not asked at all.
        let (document, all) = drawing();
        let matched = match_entities(
            &document,
            &scope_of(ScopeKind::Drawing),
            &ModifyFilter::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(matched, taken(&all, |_| true));
        assert_eq!(matched.len(), GENERATED);
    }
}
