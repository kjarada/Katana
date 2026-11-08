// What a section says besides its lines
// (include/katana/cad/plotting/section_annotation.hpp): the design and ground
// series, cut and fill between them, crossings' own levels and depths, and
// label placement; tests/cad/plotting/test_section_annotation.cpp. Sections
// are built by hand, sample by sample, so every expected number is worked on
// paper.

use std::f64::consts::PI;

use crate::near;
use katana_cad::plotting::section_annotation::{
    LABEL_GAP_MM, crossing_note, cut_fill_at, cut_fill_text, design_series, earthwork_regions,
    ground_series, is_design_series, level_at, level_range, outline_area, own_level, place_labels,
};
use katana_cad::plotting::{Earthwork, LabelCandidate};
use katana_cad::{LayerOverrides, Section, SectionCrossing, SectionSample, SectionSurface};
use katana_entity::entity::set_heights;
use katana_entity::{Entity, EntityId, Geometry, Model};
use katana_geometry::{Arc2, Box2, Circle2, Point2, Polyline2, Segment2};

fn series(name: &str, at: &[(f64, Option<f64>)]) -> SectionSurface {
    SectionSurface {
        name: String::from(name),
        samples: at
            .iter()
            .map(|&(station, elevation)| SectionSample {
                station,
                elevation,
                ..SectionSample::default()
            })
            .collect(),
        ..SectionSurface::default()
    }
}

fn add_entity(
    model: &mut Model,
    geometry: Geometry,
    heights: &[Option<f64>],
    layer: &str,
) -> EntityId {
    let mut entity = Entity {
        geometry,
        layer: String::from(layer),
        ..Entity::default()
    };
    set_heights(&mut entity.properties, heights);
    model
        .entities
        .add(entity)
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn crossing_of(
    id: EntityId,
    plan: Point2,
    station: f64,
    layer: &str,
    draped: Option<f64>,
) -> SectionCrossing {
    SectionCrossing {
        entity: id,
        layer: String::from(layer),
        plan,
        station,
        elevation: draped,
    }
}

fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
    Geometry::Line(Segment2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

fn level(surface: &SectionSurface, station: f64) -> f64 {
    level_at(surface, station).expect("a level there")
}

fn candidate(boxes: &[Box2], priority: f64) -> LabelCandidate {
    LabelCandidate {
        boxes: boxes.to_vec(),
        priority,
    }
}

fn label_box(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

// ---- the series ------------------------------------------------------------------------

mod section_annotation {
    use super::*;

    #[test]
    fn the_design_is_the_series_named_design_in_any_case() {
        assert!(is_design_series("design ROAD"));
        assert!(is_design_series("DESIGN"));
        assert!(is_design_series("Design finished surface"));
        assert!(!is_design_series("GROUND"));
        assert!(!is_design_series("desi"));
        assert!(!is_design_series(""));

        let mut section = Section {
            surfaces: vec![
                series("GROUND", &[]),
                series("ROCK", &[]),
                series("design ROAD", &[]),
            ],
            ..Section::default()
        };
        assert_eq!(
            design_series(&section).expect("a design").name,
            "design ROAD"
        );
        assert_eq!(ground_series(&section).expect("a ground").name, "GROUND");
        section.surfaces = vec![series("design ROAD", &[])];
        assert!(ground_series(&section).is_none());
    }

    #[test]
    fn a_level_is_straight_between_samples_and_nothing_in_a_gap() {
        let ground = series(
            "GROUND",
            &[
                (0.0, Some(10.0)),
                (10.0, Some(12.0)),
                (20.0, None),
                (30.0, Some(15.0)),
            ],
        );
        near(level(&ground, 5.0), 11.0, 1e-12);
        assert_eq!(level(&ground, 0.0), 10.0);
        assert_eq!(level(&ground, 30.0), 15.0);
        assert!(level_at(&ground, 15.0).is_none()); // into the gap
        assert!(level_at(&ground, 20.0).is_none());
        assert!(level_at(&ground, -0.1).is_none()); // before the start
        assert!(level_at(&ground, 30.1).is_none()); // past the end
        assert!(level_at(&series("EMPTY", &[]), 0.0).is_none());
        // A hair past either end - a range's end summed from a section's
        // length - reads the end; a millimetre past does not.
        assert_eq!(level(&ground, 30.0 + 1e-9), 15.0);
        assert_eq!(level(&ground, -1e-9), 10.0);
        assert!(level_at(&ground, 30.001).is_none());
        assert!(level_at(&ground, -0.001).is_none());
    }

    // ---- cut and fill ------------------------------------------------------------------------

    #[test]
    fn cut_and_fill_are_design_less_ground() {
        let design = series("design", &[(0.0, Some(10.0)), (10.0, Some(10.0))]);
        let ground = series("GROUND", &[(0.0, Some(9.0)), (10.0, Some(11.0))]);
        near(
            cut_fill_at(&design, &ground, 0.0).expect("fill"),
            1.0,
            1e-12,
        );
        near(
            cut_fill_at(&design, &ground, 10.0).expect("cut"),
            -1.0,
            1e-12,
        );
        near(
            cut_fill_at(&design, &ground, 5.0).expect("level"),
            0.0,
            1e-12,
        );
        assert!(cut_fill_at(&design, &ground, 11.0).is_none());

        assert_eq!(cut_fill_text(0.25), "+0.250");
        assert_eq!(cut_fill_text(-1.2), "-1.200");
        assert_eq!(cut_fill_text(0.0), "0.000");
        assert_eq!(cut_fill_text(-0.0004), "0.000"); // never "-0.000"
    }

    #[test]
    fn where_the_design_crosses_the_ground_the_region_splits() {
        // Design flat at 10 over 0..10; ground rising from 9 to 11. They cross
        // at 5: fill before (design above), cut after, each a triangle of
        // 5 x 1 / 2 = 2.5 square metres.
        let design = series("design", &[(0.0, Some(10.0)), (10.0, Some(10.0))]);
        let ground = series("GROUND", &[(0.0, Some(9.0)), (10.0, Some(11.0))]);
        let regions = earthwork_regions(&design, &ground, f64::NEG_INFINITY, f64::INFINITY);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].kind, Earthwork::Fill);
        assert_eq!(regions[1].kind, Earthwork::Cut);
        // The meeting point is in each outline once.
        assert_eq!(
            regions[0].outline,
            vec![
                Point2::new(0.0, 10.0),
                Point2::new(5.0, 10.0),
                Point2::new(0.0, 9.0)
            ]
        );
        assert_eq!(
            regions[1].outline,
            vec![
                Point2::new(5.0, 10.0),
                Point2::new(10.0, 10.0),
                Point2::new(10.0, 11.0)
            ]
        );
        near(outline_area(&regions[0].outline), 2.5, 1e-12);
        near(outline_area(&regions[1].outline), 2.5, 1e-12);

        // Only 2..8: 3 x 0.6 / 2 = 0.9 each side of the crossing.
        let within = earthwork_regions(&design, &ground, 2.0, 8.0);
        assert_eq!(within.len(), 2);
        near(outline_area(&within[0].outline), 0.9, 1e-12);
        near(outline_area(&within[1].outline), 0.9, 1e-12);
        near(within[0].outline[0].x, 2.0, 1e-12);
        near(within[1].outline[1].x, 8.0, 1e-12);
    }

    #[test]
    fn pieces_of_one_kind_are_one_region_and_a_gap_ends_it() {
        // Design above the ground all the way, over samples of both at
        // different stations: one fill region.
        let design = series(
            "design",
            &[(0.0, Some(11.0)), (10.0, Some(12.0)), (20.0, Some(11.0))],
        );
        let ground = series(
            "GROUND",
            &[(0.0, Some(10.0)), (5.0, Some(10.5)), (20.0, Some(10.0))],
        );
        let one = earthwork_regions(&design, &ground, f64::NEG_INFINITY, f64::INFINITY);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].kind, Earthwork::Fill);
        // Design at 0, 5, 10, 20 then ground back at 20, 10, 5, 0.
        assert_eq!(one[0].outline.len(), 8);
        // The area is the design's less the ground's, piece by piece: design
        // 0..10 = 115 and 10..20 = 115; ground 0..5 = 51.25 and 5..20 =
        // 153.75.
        near(outline_area(&one[0].outline), 230.0 - 205.0, 1e-9);

        // A gap in the ground at 10 splits it into two.
        let gappy = series(
            "GROUND",
            &[
                (0.0, Some(10.0)),
                (5.0, Some(10.5)),
                (10.0, None),
                (15.0, Some(10.0)),
                (20.0, Some(10.0)),
            ],
        );
        let two = earthwork_regions(&design, &gappy, f64::NEG_INFINITY, f64::INFINITY);
        assert_eq!(two.len(), 2);
        near(two[0].outline[two[0].outline.len() - 1].x, 0.0, 1e-12);
        near(two[1].outline[0].x, 15.0, 1e-12);
    }

    #[test]
    fn design_on_the_ground_has_nothing_to_shade() {
        let design = series("design", &[(0.0, Some(10.0)), (10.0, Some(12.0))]);
        let ground = series(
            "GROUND",
            &[(0.0, Some(10.0)), (5.0, Some(11.0)), (10.0, Some(12.0))],
        );
        let everywhere = |design: &SectionSurface, ground: &SectionSurface| {
            earthwork_regions(design, ground, f64::NEG_INFINITY, f64::INFINITY)
        };
        assert!(everywhere(&design, &ground).is_empty());
        // Nor where they do not overlap.
        assert!(
            everywhere(
                &design,
                &series("GROUND", &[(20.0, Some(1.0)), (30.0, Some(1.0))])
            )
            .is_empty()
        );
        assert!(everywhere(&design, &series("GROUND", &[])).is_empty());
    }

    // ---- crossings ------------------------------------------------------------------------------

    #[test]
    fn a_crossing_takes_its_entitys_own_level_where_it_crosses() {
        let mut model = Model::default();
        // A pipe from (0, 0) at 20 to (10, 0) at 22, crossed at x 2.5: 20.5.
        let pipe = add_entity(
            &mut model,
            segment(0.0, 0.0, 10.0, 0.0),
            &[Some(20.0), Some(22.0)],
            "WATER",
        );
        near(
            own_level(
                &model,
                &crossing_of(pipe, Point2::new(2.5, 0.0), 0.0, "WATER", None),
            )
            .expect("a level"),
            20.5,
            1e-12,
        );

        // A string with a level at each vertex, crossed on its second piece.
        let string = Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0),
            ],
            false,
        );
        let main = add_entity(
            &mut model,
            Geometry::Polyline(string),
            &[Some(30.0), Some(31.0), Some(35.0)],
            "WATER",
        );
        // A pit: one level for the whole circle.
        let pit = add_entity(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(50.0, 50.0), 1.0)),
            &[Some(18.25)],
            "WATER",
        );
        // An arc at 10 at its start and 20 at its end, crossed halfway round.
        let bend = Arc2::new(Point2::new(0.0, 0.0), 5.0, 0.0, PI / 2.0);
        let curve = add_entity(
            &mut model,
            Geometry::Arc(bend),
            &[Some(10.0), Some(20.0)],
            "WATER",
        );
        // A line drawn flat has no level of its own; nor has one missing a
        // level at either end of the piece crossed, nor an entity that is
        // gone.
        let kerb = add_entity(&mut model, segment(0.0, 5.0, 10.0, 5.0), &[], "WATER");
        let half = add_entity(
            &mut model,
            segment(0.0, 7.0, 10.0, 7.0),
            &[Some(20.0), None],
            "WATER",
        );
        let own = |id: EntityId, plan: Point2| {
            own_level(&model, &crossing_of(id, plan, 0.0, "WATER", None))
        };
        near(
            own(main, Point2::new(10.0, 4.0)).expect("a level"),
            32.6,
            1e-12,
        );
        assert_eq!(own(pit, Point2::new(51.0, 50.0)), Some(18.25));
        near(own(curve, bend.midpoint()).expect("a level"), 15.0, 1e-9);
        assert!(own(kerb, Point2::new(5.0, 5.0)).is_none());
        assert!(own(half, Point2::new(5.0, 7.0)).is_none());
        assert!(own(987654, Point2::default()).is_none());
    }

    #[test]
    fn a_service_is_noted_with_its_level_and_depth() {
        let mut model = Model::default();
        let water = add_entity(
            &mut model,
            segment(0.0, -5.0, 0.0, 5.0),
            &[Some(24.1)],
            "WATER",
        );
        let power = add_entity(
            &mut model,
            segment(4.0, -5.0, 4.0, 5.0),
            &[Some(36.0)],
            "power",
        );
        let kerb = add_entity(&mut model, segment(8.0, -5.0, 8.0, 5.0), &[], "KERB");
        let section = Section {
            surfaces: vec![
                series("design ROAD", &[(0.0, Some(26.0)), (10.0, Some(26.0))]),
                series("GROUND", &[(0.0, Some(25.3)), (10.0, Some(29.3))]),
            ],
            ..Section::default()
        };

        // Buried 25.30 - 24.10 = 1.20 under the ground.
        let service = crossing_note(
            &section,
            &crossing_of(water, Point2::new(0.0, 0.0), 0.0, "WATER", Some(25.3)),
            Some(&model),
        );
        assert!(service.own_level);
        near(service.level.expect("a level"), 24.1, 1e-12);
        near(service.depth.expect("a depth"), 1.2, 1e-12);
        assert_eq!(service.text, "WATER RL 24.10 D 1.20");
        assert_eq!(service.short_text, "WATER");

        // Overhead at 36.00 where the ground is 26.90: 9.10 above it.
        let overhead = crossing_note(
            &section,
            &crossing_of(power, Point2::new(4.0, 0.0), 4.0, "power", Some(26.9)),
            Some(&model),
        );
        assert_eq!(overhead.text, "POWER RL 36.00 H 9.10");

        // Draped on the ground: its level is the ground's, and it has no
        // depth.
        let draped = crossing_note(
            &section,
            &crossing_of(kerb, Point2::new(8.0, 0.0), 8.0, "KERB", Some(28.5)),
            Some(&model),
        );
        assert!(!draped.own_level);
        assert!(draped.depth.is_none());
        assert_eq!(draped.text, "KERB RL 28.50");

        // No surface under it and no level of its own: its layer alone.
        let bare = crossing_note(
            &section,
            &crossing_of(kerb, Point2::new(8.0, 0.0), 8.0, "FENCE", None),
            Some(&model),
        );
        assert_eq!(bare.text, "FENCE");
        assert!(bare.level.is_none());

        // Without the model every level is the one the section found.
        assert_eq!(
            crossing_note(
                &section,
                &crossing_of(water, Point2::new(0.0, 0.0), 0.0, "WATER", Some(25.3)),
                None
            )
            .text,
            "WATER RL 25.30"
        );
    }

    #[test]
    fn the_level_range_takes_in_buried_services_and_leaves_out_hidden_ones() {
        let mut model = Model::default();
        let sewer = add_entity(
            &mut model,
            segment(5.0, -5.0, 5.0, 5.0),
            &[Some(17.5)],
            "SEWER",
        );
        let section = Section {
            surfaces: vec![series(
                "GROUND",
                &[(0.0, Some(20.0)), (10.0, Some(22.0)), (20.0, Some(21.0))],
            )],
            crossings: vec![crossing_of(
                sewer,
                Point2::new(5.0, 0.0),
                5.0,
                "SEWER",
                Some(21.0),
            )],
            ..Section::default()
        };

        let all = level_range(&section, -1e9, 1e9, Some(&model), None).expect("a range");
        assert_eq!(all.0, 17.5);
        assert_eq!(all.1, 22.0);
        // Between 12 and 20: the ground only, from 21.8 at 12 down to 21.
        let part = level_range(&section, 12.0, 20.0, Some(&model), None).expect("a range");
        near(part.0, 21.0, 1e-12);
        near(part.1, 21.8, 1e-12);
        // The sewer hidden, it does not count.
        let mut hidden = LayerOverrides::default();
        hidden.hide("SEWER");
        assert_eq!(
            level_range(&section, -1e9, 1e9, Some(&model), Some(&hidden))
                .expect("a range")
                .0,
            20.0
        );
        // Nothing there: nothing.
        assert!(level_range(&section, 30.0, 40.0, Some(&model), None).is_none());
    }
}

// ---- placing labels ---------------------------------------------------------------------------

mod section_label_placement {
    use super::*;

    #[test]
    fn the_nearest_the_middle_is_kept_and_the_other_dropped() {
        let bounds = label_box(0.0, 0.0, 100.0, 50.0);
        // Two labels in one place: the one with the lower priority wins,
        // whichever comes first.
        let labels = [
            candidate(&[label_box(10.0, 10.0, 12.0, 30.0)], 5.0),
            candidate(&[label_box(11.0, 10.0, 13.0, 30.0)], 1.0),
        ];
        let placed = place_labels(&labels, &bounds, &[], LABEL_GAP_MM);
        assert!(placed[0].is_none());
        assert_eq!(placed[1], Some(0));
    }

    #[test]
    fn a_label_takes_its_first_free_place() {
        let bounds = label_box(0.0, 0.0, 100.0, 50.0);
        let labels = [
            candidate(&[label_box(10.0, 10.0, 12.0, 30.0)], 0.0),
            // Its first place crowds the first label; its second - the other
            // side of its line - does not; its third is never tried.
            candidate(
                &[
                    label_box(12.1, 10.0, 14.0, 30.0),
                    label_box(20.0, 10.0, 22.0, 30.0),
                    label_box(40.0, 10.0, 42.0, 30.0),
                ],
                1.0,
            ),
            // Every place outside the bounds or crowded: dropped.
            candidate(
                &[
                    label_box(95.0, 10.0, 105.0, 30.0),
                    label_box(9.0, 29.0, 11.0, 45.0),
                ],
                2.0,
            ),
        ];
        let placed = place_labels(&labels, &bounds, &[], 0.3);
        assert!(placed[0].is_some());
        assert_eq!(placed[1], Some(1));
        assert!(placed[2].is_none());
    }

    #[test]
    fn obstacles_are_kept_clear_and_ties_go_in_order() {
        let bounds = label_box(0.0, 0.0, 100.0, 50.0);
        let obstacles = [label_box(0.0, 40.0, 30.0, 50.0)];
        let labels = [
            candidate(
                &[
                    label_box(5.0, 35.0, 7.0, 45.0),
                    label_box(5.0, 5.0, 7.0, 15.0),
                ],
                0.0,
            ),
            candidate(&[label_box(6.0, 5.0, 8.0, 15.0)], 0.0),
            candidate(&[label_box(50.0, 5.0, 52.0, 15.0)], 0.0),
        ];
        let placed = place_labels(&labels, &bounds, &obstacles, LABEL_GAP_MM);
        assert_eq!(placed[0], Some(1)); // the key is in the way of its first place
        assert!(placed[1].is_none()); // the first, equal and earlier, took its place
        assert!(placed[2].is_some());
        // Boxes touching within the gap crowd each other; farther apart they
        // do not.
        let pair = [
            candidate(&[label_box(0.0, 0.0, 2.0, 2.0)], 0.0),
            candidate(&[label_box(2.2, 0.0, 4.0, 2.0)], 1.0),
        ];
        assert!(place_labels(&pair, &bounds, &[], 0.3)[1].is_none());
        assert!(place_labels(&pair, &bounds, &[], 0.1)[1].is_some());
    }
}
