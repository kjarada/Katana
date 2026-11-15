// Profiles and cross sections (PLAN.MD Phases 14 and 21; tests/cad/test_section.cpp).
//
// The surfaces here are built by hand with elevations that are exact in
// binary, so an expected profile elevation can be written down rather than
// read off the program. That is what makes an interpolation error visible
// instead of self-confirming.

use crate::near;
use katana_cad::section::{
    append_design_profile, cross_section_line, extract_section, section_stations,
};
use katana_cad::spatial_query::query_extents;
use katana_cad::{
    SampleReason, Section, SectionOptions, SectionSample, SectionSurface, SectionSurfaceInput,
};
use katana_core::ErrorCode;
use katana_entity::{Entity, EntityId, Geometry, Model};
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    Circle2, Point2, Point3, Polyline2, ProfilePvi, Segment2, SolvedProfile, SpatialEntry,
    SpatialIndex, VerticalAlignment,
};
use katana_math::cpp;
use katana_terrain::TinSurface;

fn line_from(points: &[Point2]) -> Polyline2 {
    Polyline2::new(points.to_vec(), false)
}

fn surface_of(vertices: Vec<Point3>, triangles: Vec<[u32; 3]>) -> TinSurface {
    match TinSurface::create(vertices, triangles, Vec::new()) {
        Ok(surface) => surface,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// A 100 x 100 square sloping 1:10 to the east: z = x / 10 exactly, which is
/// exact in binary at every tenth so the expected values below are exact too.
fn ramp() -> TinSurface {
    surface_of(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(100.0, 0.0, 10.0),
            Point3::new(100.0, 100.0, 10.0),
            Point3::new(0.0, 100.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

/// A roof: flat at z = 0 along the west edge, rising to a ridge at x = 50 and
/// falling again. The ridge is a slope break that interval sampling alone will
/// cut the top off unless the crossing is found.
fn roof() -> TinSurface {
    surface_of(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(50.0, 0.0, 10.0),
            Point3::new(100.0, 0.0, 0.0),
            Point3::new(0.0, 100.0, 0.0),
            Point3::new(50.0, 100.0, 10.0),
            Point3::new(100.0, 100.0, 0.0),
        ],
        vec![[0, 1, 4], [0, 4, 3], [1, 2, 5], [1, 5, 4]],
    )
}

fn input<'a>(name: &'a str, surface: &'a TinSurface) -> SectionSurfaceInput<'a> {
    SectionSurfaceInput {
        name,
        surface: Some(surface),
    }
}

fn without_crossings(interval: f64) -> SectionOptions<'static> {
    SectionOptions {
        interval,
        include_crossings: false,
        ..SectionOptions::default()
    }
}

fn extract(
    alignment: &Polyline2,
    inputs: &[SectionSurfaceInput<'_>],
    model: Option<&Model>,
    options: &SectionOptions<'_>,
) -> Section {
    match extract_section(alignment, inputs, model, options) {
        Ok(section) => section,
        Err(error) => panic!("{}", error.describe()),
    }
}

// ---- the design profile as a series of the section --------------------------------

/// A straight 300 m section along +x with one ground series sampled every
/// 50 m at a constant 10.0, built by hand so the test depends on nothing but
/// the Section struct.
fn straight_section() -> Section {
    let mut ground = SectionSurface {
        name: String::from("ground"),
        ..SectionSurface::default()
    };
    let mut s = 0.0;
    while s <= 300.0 {
        ground.samples.push(SectionSample {
            station: s,
            plan: Point2::new(s, 0.0),
            elevation: Some(10.0),
        });
        ground.reasons.push(SampleReason::Interval);
        s += 50.0;
    }
    ground.min_elevation = Some(10.0);
    ground.max_elevation = Some(10.0);
    Section {
        alignment: line_from(&[Point2::new(0.0, 0.0), Point2::new(300.0, 0.0)]),
        length: 300.0,
        surfaces: vec![ground],
        crossings: Vec::new(),
    }
}

fn pvi(station: f64, elevation: f64, curve_length: f64) -> ProfilePvi {
    ProfilePvi {
        station,
        elevation,
        curve_length,
    }
}

/// The textbook sag from the profile tests, shifted to start at 0, kept
/// inside the 300 m section: PVIs (0, 16), (100, 13, L = 100), (300, 17):
/// -3% into +2%, PVC 50 @ 14.5, PVT 150 @ 14.0, low point at
/// x = 0.03 * 100 / 0.05 = 60 -> station 110 @ 14.5 - 1.8 + 0.9 = 13.6.
fn sag_profile() -> SolvedProfile {
    let definition = VerticalAlignment {
        pvis: vec![
            pvi(0.0, 16.0, 0.0),
            pvi(100.0, 13.0, 100.0),
            pvi(300.0, 17.0, 0.0),
        ],
    };
    match solve_profile(&definition) {
        Ok(profile) => profile,
        Err(error) => panic!("{}", error.describe()),
    }
}

// ---- the crossing search: box rejection and the document's index -----------------------

/// Every entity's broad-phase box, as the Document's rebuild stores it.
fn index_of(model: &Model) -> SpatialIndex {
    let mut entries = Vec::new();
    model.entities.for_each(|entity| {
        entries.push(SpatialEntry {
            id: entity.id,
            box_value: query_extents(model, entity),
        });
    });
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);
    index
}

fn add_on(model: &mut Model, geometry: Geometry) -> EntityId {
    let entity = Entity {
        geometry,
        layer: String::from("0"),
        ..Entity::default()
    };
    match model.entities.add(entity) {
        Ok(id) => id,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
    Geometry::Line(Segment2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

mod cad_section {
    use super::*;

    #[test]
    fn samples_the_surface_at_every_station() {
        let surface = ramp();
        let inputs = [input("Existing", &surface)];

        let section = extract(
            &line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]),
            &inputs,
            None,
            &without_crossings(10.0),
        );
        assert_eq!(section.surfaces.len(), 1);
        assert_eq!(section.surfaces[0].name, "Existing");
        near(section.length, 100.0, 1e-12);

        let samples = &section.surfaces[0].samples;
        assert!(samples.len() >= 11);
        near(samples[0].station, 0.0, 1e-12);
        near(samples[samples.len() - 1].station, 100.0, 1e-12);

        // z = x / 10, and the alignment runs along y = 50, so elevation =
        // station / 10.
        for sample in samples {
            let elevation = sample
                .elevation
                .unwrap_or_else(|| panic!("station {}", sample.station));
            near(elevation, sample.station / 10.0, 1e-9);
        }
        near(
            section.surfaces[0].min_elevation.expect("a minimum"),
            0.0,
            1e-9,
        );
        near(
            section.surfaces[0].max_elevation.expect("a maximum"),
            10.0,
            1e-9,
        );
    }

    #[test]
    fn stations_are_strictly_ascending_with_no_duplicates() {
        let surface = roof();
        let inputs = [input("Design", &surface)];
        // 7 does not divide the length, so the end is ragged.
        let section = extract(
            &line_from(&[
                Point2::new(0.0, 20.0),
                Point2::new(60.0, 20.0),
                Point2::new(100.0, 80.0),
            ]),
            &inputs,
            None,
            &without_crossings(7.0),
        );

        let samples = &section.surfaces[0].samples;
        assert!(samples.len() >= 2);
        assert_eq!(samples.len(), section.surfaces[0].reasons.len());
        for i in 1..samples.len() {
            assert!(samples[i].station > samples[i - 1].station, "at index {i}");
        }
        assert_eq!(samples[0].station, 0.0);
        near(samples[samples.len() - 1].station, section.length, 1e-9);
    }

    #[test]
    fn the_ridge_is_sampled_exactly_rather_than_cut_off() {
        // THE point of finding surface breaks. The ridge is at x = 50, z = 10;
        // an interval of 30 puts samples at 0, 30, 60, 90, 100 and would
        // report a peak of 8 instead of 10 - a 20% error in the highest point
        // on the profile.
        let surface = roof();
        let inputs = [input("Design", &surface)];
        let alignment = line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]);

        let mut options = without_crossings(30.0);
        options.include_surface_breaks = true;
        let section = extract(&alignment, &inputs, None, &options);

        // Along y = 50 the alignment crosses three triangle edges: the two
        // mesh diagonals at x = 25 and x = 75, and the ridge itself at x = 50.
        // All three are genuine triangle boundaries and all three are
        // expected - only the middle one is a change of SLOPE, but the sampler
        // does not and should not try to tell them apart, because a diagonal
        // is where interpolation hands over from one plane to another whether
        // or not the planes agree.
        let mut peak: f64 = 0.0;
        let mut breaks = Vec::new();
        let surface_section = &section.surfaces[0];
        for (sample, reason) in surface_section.samples.iter().zip(&surface_section.reasons) {
            if let Some(elevation) = sample.elevation {
                peak = cpp::max(peak, elevation);
            }
            if *reason == SampleReason::SurfaceBreak {
                breaks.push(sample.station);
            }
        }
        assert_eq!(breaks.len(), 3);
        near(breaks[0], 25.0, 1e-3);
        // The ridge must be found exactly.
        near(breaks[1], 50.0, 1e-3);
        near(breaks[2], 75.0, 1e-3);
        // Bisection returns the station just PAST the change, so every break
        // lands in the new triangle rather than ambiguously on the shared
        // edge.
        for &station in &breaks {
            assert!(station >= (station / 25.0).round() * 25.0);
        }
        // The ridge must reach its true height.
        near(peak, 10.0, 1e-3);

        // And with break detection off it genuinely does cut the corner,
        // which is what makes the assertion above meaningful rather than
        // vacuous.
        options.include_surface_breaks = false;
        let coarse = extract(&alignment, &inputs, None, &options);
        let mut coarse_peak: f64 = 0.0;
        for sample in &coarse.surfaces[0].samples {
            if let Some(elevation) = sample.elevation {
                coarse_peak = cpp::max(coarse_peak, elevation);
            }
        }
        assert!(
            coarse_peak < 9.0,
            "without break detection the ridge is missed"
        );
    }

    #[test]
    fn stations_off_the_surface_report_no_elevation_rather_than_zero() {
        let surface = ramp(); // covers x in [0, 100]
        let inputs = [input("Existing", &surface)];

        // Runs well past the eastern edge of the surface.
        let section = extract(
            &line_from(&[Point2::new(50.0, 50.0), Point2::new(250.0, 50.0)]),
            &inputs,
            None,
            &without_crossings(10.0),
        );

        let mut saw_covered = false;
        let mut saw_gap = false;
        for sample in &section.surfaces[0].samples {
            if sample.elevation.is_some() {
                saw_covered = true;
                assert!(sample.station <= 51.0);
            } else {
                saw_gap = true;
            }
        }
        assert!(saw_covered);
        assert!(
            saw_gap,
            "past the rim the profile must be a gap, not a zero"
        );
    }

    #[test]
    fn two_surfaces_are_cut_on_the_same_stations() {
        // Existing against design is the whole reason a profile has more than
        // one surface: the stations must line up or the comparison is
        // meaningless.
        let existing = ramp();
        let design = roof();
        let inputs = [input("Existing", &existing), input("Design", &design)];

        let mut options = without_crossings(20.0);
        options.include_surface_breaks = false; // breaks are per surface by design

        let section = extract(
            &line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]),
            &inputs,
            None,
            &options,
        );
        assert_eq!(section.surfaces.len(), 2);
        assert_eq!(
            section.surfaces[0].samples.len(),
            section.surfaces[1].samples.len()
        );
        for (first, second) in section.surfaces[0]
            .samples
            .iter()
            .zip(&section.surfaces[1].samples)
        {
            assert_eq!(first.station, second.station);
        }
    }

    #[test]
    fn rejects_an_alignment_that_is_not_one() {
        let surface = ramp();
        let inputs = [input("Existing", &surface)];
        let options = SectionOptions::default();

        assert!(extract_section(&Polyline2::default(), &inputs, None, &options).is_err());
        assert!(
            extract_section(
                &line_from(&[Point2::new(1.0, 1.0)]),
                &inputs,
                None,
                &options
            )
            .is_err()
        );
        // Two coincident vertices collapse to one distinct vertex.
        assert!(
            extract_section(
                &line_from(&[Point2::new(1.0, 1.0), Point2::new(1.0, 1.0)]),
                &inputs,
                None,
                &options
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_an_interval_that_would_exhaust_memory() {
        let surface = ramp();
        let inputs = [input("Existing", &surface)];
        let alignment = line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]);

        let mut options = SectionOptions {
            interval: 1e-9, // 100 m at a nanometre: 1e11 samples
            ..SectionOptions::default()
        };
        let error = extract_section(&alignment, &inputs, None, &options)
            .expect_err("a typo must fail cleanly, not swap the machine out");
        assert_eq!(error.code, ErrorCode::InvalidArgument);

        options.interval = 0.0;
        assert!(extract_section(&alignment, &inputs, None, &options).is_err());
        options.interval = f64::NAN;
        assert!(extract_section(&alignment, &inputs, None, &options).is_err());
    }

    #[test]
    fn rejects_a_null_surface_instead_of_dereferencing_it() {
        let inputs = [SectionSurfaceInput {
            name: "Broken",
            surface: None,
        }];
        let error = extract_section(
            &line_from(&[Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)]),
            &inputs,
            None,
            &SectionOptions::default(),
        )
        .expect_err("no surface to cut");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn entities_crossing_the_alignment_are_reported_at_their_station() {
        let surface = ramp();
        let mut model = Model::default();
        // The layers the entities name must exist: a section shows what the
        // plan shows, and an entity on a layer that is not in the table is
        // not drawn (LayerDatabase::resolve). This fixture put them on layers
        // it never created, which the model's own commands would have refused,
        // and passed only while sections ignored layers altogether (audit
        // REN-04).
        assert!(model.layers.ensure("FENCE").is_ok());
        assert!(model.layers.ensure("DRAINAGE").is_ok());

        // A line crossing the alignment at x = 30, and a circle centred on the
        // alignment crossing it twice.
        let fence = Entity {
            geometry: segment(30.0, 20.0, 30.0, 80.0),
            layer: String::from("FENCE"),
            ..Entity::default()
        };
        assert!(model.entities.add(fence).is_ok());

        let manhole = Entity {
            geometry: Geometry::Circle(Circle2::new(Point2::new(70.0, 50.0), 5.0)),
            layer: String::from("DRAINAGE"),
            ..Entity::default()
        };
        assert!(model.entities.add(manhole).is_ok());

        let options = SectionOptions {
            interval: 25.0,
            include_crossings: true,
            ..SectionOptions::default()
        };

        let section = extract(
            &line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]),
            &[input("Existing", &surface)],
            Some(&model),
            &options,
        );
        assert_eq!(
            section.crossings.len(),
            3,
            "one fence crossing and two circle crossings"
        );

        // Sorted by station: 30 (fence), 65 and 75 (the circle's two sides).
        near(section.crossings[0].station, 30.0, 1e-9);
        assert_eq!(section.crossings[0].layer, "FENCE");
        near(section.crossings[1].station, 65.0, 1e-9);
        near(section.crossings[2].station, 75.0, 1e-9);
        // The elevation comes from the surface at the crossing: z = x / 10.
        near(
            section.crossings[0].elevation.expect("an elevation"),
            3.0,
            1e-9,
        );
    }

    #[test]
    fn hidden_entities_do_not_appear_on_the_section() {
        let surface = ramp();
        let mut model = Model::default();
        let hidden = Entity {
            geometry: segment(30.0, 20.0, 30.0, 80.0),
            visible: false,
            ..Entity::default()
        };
        assert!(model.entities.add(hidden).is_ok());

        let section = extract(
            &line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]),
            &[input("Existing", &surface)],
            Some(&model),
            &SectionOptions::default(),
        );
        assert!(section.crossings.is_empty());
    }

    #[test]
    fn a_cross_section_of_a_ramp_is_flat_across_the_fall_line() {
        // End to end: cut the ramp perpendicular to its fall and every
        // elevation on the section must be the same, because the ramp only
        // falls east.
        let surface = ramp();
        let alignment = line_from(&[Point2::new(10.0, 50.0), Point2::new(90.0, 50.0)]);

        let line = cross_section_line(&alignment, 40.0, 20.0)
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        let section = extract(
            &line,
            &[input("Existing", &surface)],
            None,
            &without_crossings(5.0),
        );

        // Station 40 along an alignment starting at x = 10 is x = 50, so
        // z = 5.
        for sample in &section.surfaces[0].samples {
            let elevation = sample.elevation.expect("an elevation");
            near(elevation, 5.0, 1e-6);
        }
    }

    #[test]
    fn a_design_profile_is_sampled_where_the_ground_is_and_at_its_own_key_stations() {
        let mut section = straight_section();
        let profile = sag_profile();
        assert!(append_design_profile(&mut section, &profile, String::from("design")).is_ok());
        assert_eq!(section.surfaces.len(), 2);
        let design = &section.surfaces[1];
        assert_eq!(design.name, "design");

        // Every ground station is present, so the two can be read against
        // each other; the PVC (50), PVT (150) and low point (110) are present
        // too and marked as the design's own.
        let stations: Vec<f64> = design.samples.iter().map(|sample| sample.station).collect();
        let vertices = design
            .reasons
            .iter()
            .filter(|reason| **reason == SampleReason::ProfileVertex)
            .count();
        for ground in [0.0, 50.0, 100.0, 150.0, 200.0, 250.0, 300.0] {
            assert!(stations.contains(&ground), "{ground}");
        }
        assert!(stations.contains(&110.0), "low point");
        assert!(vertices >= 4, "start, PVC, low point, PVT, end at least");
        assert!(stations.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(
            stations.windows(2).all(|pair| pair[0] != pair[1]),
            "a station appeared twice"
        );

        // Elevations are the profile's, exactly, and the plan positions are on
        // the alignment: station s of a straight along +x is (s, 0).
        for sample in &design.samples {
            let elevation = sample
                .elevation
                .unwrap_or_else(|| panic!("{}", sample.station));
            near(
                elevation,
                profile
                    .elevation_at(sample.station)
                    .expect("on the profile"),
                1e-12,
            );
            near(sample.plan.x, sample.station, 1e-9);
            near(sample.plan.y, 0.0, 1e-9);
        }
        // The low point.
        near(design.min_elevation.expect("a minimum"), 13.6, 1e-12);
        near(design.max_elevation.expect("a maximum"), 17.0, 1e-12);

        // The section's extent now spans ground and design together.
        let extent = section.extent();
        near(extent.min.y, 10.0, 1e-12);
        near(extent.max.y, 17.0, 1e-12);
    }

    #[test]
    fn a_design_shorter_than_the_section_leaves_a_gap_rather_than_inventing_grades() {
        let mut section = straight_section();
        let definition = VerticalAlignment {
            pvis: vec![pvi(0.0, 12.0, 0.0), pvi(200.0, 14.0, 0.0)],
        };
        let profile = solve_profile(&definition).expect("a profile");
        assert!(append_design_profile(&mut section, &profile, String::from("design")).is_ok());
        let design = section.surfaces.last().expect("the design");
        let mut gaps = 0;
        for sample in &design.samples {
            if sample.station > 200.0 {
                assert!(sample.elevation.is_none(), "{}", sample.station);
                gaps += 1;
            } else {
                assert!(sample.elevation.is_some(), "{}", sample.station);
            }
        }
        assert!(gaps >= 2, "250 and 300 lie beyond the design");
    }

    #[test]
    fn a_design_profile_rejects_an_empty_name_and_an_empty_section() {
        let mut section = straight_section();
        let profile = sag_profile();
        assert_eq!(
            append_design_profile(&mut section, &profile, String::new())
                .expect_err("no name")
                .code,
            ErrorCode::InvalidArgument
        );
        let mut empty = Section::default();
        assert_eq!(
            append_design_profile(&mut empty, &profile, String::from("design"))
                .expect_err("no length")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            section.surfaces.len(),
            1,
            "a refused append changes nothing"
        );
    }

    #[test]
    fn crossings_are_the_same_with_the_documents_index_as_without_it() {
        // An L-shaped alignment: (0, 50) east to (100, 50), then north to
        // (100, 100). Station s on the first leg is at x = s; on the second leg
        // station 100 + d is at y = 50 + d.
        //
        // Expected crossings, worked by hand:
        //   fence   x = 30 from y 20 to 80        -> station 30
        //   post    x = 40 from y 50 (touching) to 90 -> station 40
        //   string  polyline (60,0) (60,40) (60,60) -> its second edge, station 60
        //   manhole circle centre (70,50) r 5      -> stations 65 and 75
        //   kerb    y = 80 from x 90 to 110        -> second leg at y 80: 100 + 30 = 130
        // plus 2000 short strings scattered over y in [200, 900], none of which
        // reaches the alignment (it never goes above y = 100).
        let mut model = Model::default();
        assert!(model.layers.ensure("0").is_ok());
        let fence = add_on(&mut model, segment(30.0, 20.0, 30.0, 80.0));
        let post = add_on(&mut model, segment(40.0, 50.0, 40.0, 90.0));
        let string = add_on(
            &mut model,
            Geometry::Polyline(line_from(&[
                Point2::new(60.0, 0.0),
                Point2::new(60.0, 40.0),
                Point2::new(60.0, 60.0),
            ])),
        );
        let manhole = add_on(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(70.0, 50.0), 5.0)),
        );
        let kerb = add_on(&mut model, segment(90.0, 80.0, 110.0, 80.0));
        for i in 0..2000 {
            // A fixed, spread-out pattern rather than random numbers: the
            // answer must not depend on a generator.
            let x = f64::from((i * 37) % 1000);
            let y = 200.0 + f64::from((i * 53) % 700);
            add_on(
                &mut model,
                Geometry::Polyline(line_from(&[
                    Point2::new(x, y),
                    Point2::new(x + 3.0, y + 2.0),
                    Point2::new(x + 5.0, y),
                ])),
            );
        }
        let index = index_of(&model);

        let alignment = line_from(&[
            Point2::new(0.0, 50.0),
            Point2::new(100.0, 50.0),
            Point2::new(100.0, 100.0),
        ]);
        let mut options = SectionOptions {
            interval: 10.0,
            ..SectionOptions::default()
        };
        let scanned = extract(&alignment, &[], Some(&model), &options);
        options.spatial_index = Some(&index);
        let indexed = extract(&alignment, &[], Some(&model), &options);

        let expected: [(f64, EntityId); 6] = [
            (30.0, fence),
            (40.0, post),
            (60.0, string),
            (65.0, manhole),
            (75.0, manhole),
            (130.0, kerb),
        ];
        for section in [&scanned, &indexed] {
            assert_eq!(section.crossings.len(), expected.len());
            for (i, (station, entity)) in expected.iter().enumerate() {
                near(section.crossings[i].station, *station, 1e-9);
                assert_eq!(section.crossings[i].entity, *entity, "{i}");
            }
        }
        // Not merely close: the same crossings, bit for bit, in the same order.
        for (i, (first, second)) in scanned.crossings.iter().zip(&indexed.crossings).enumerate() {
            assert_eq!(first.station, second.station, "{i}");
            assert_eq!(first.plan, second.plan, "{i}");
        }
    }

    #[test]
    fn a_crossing_within_tolerance_of_the_alignment_survives_the_box_test() {
        // A fence that stops 5e-8 short of the alignment y = 50. intersect()
        // accepts a crossing within GEOMETRIC (1e-7) of both segments, so this
        // one is reported at x = 20 - but the fence's exact box ends below the
        // alignment's, and a box test without a margin would have thrown it
        // away before intersect() was ever asked.
        let mut model = Model::default();
        assert!(model.layers.ensure("0").is_ok());
        let fence = add_on(&mut model, segment(20.0, 10.0, 20.0, 50.0 - 5e-8));
        let index = index_of(&model);

        for with_index in [None, Some(&index)] {
            let options = SectionOptions {
                interval: 10.0,
                spatial_index: with_index,
                ..SectionOptions::default()
            };
            let section = extract(
                &line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]),
                &[],
                Some(&model),
                &options,
            );
            assert_eq!(section.crossings.len(), 1);
            assert_eq!(section.crossings[0].entity, fence);
            near(section.crossings[0].station, 20.0, 1e-9);
        }
    }

    #[test]
    fn a_polyline_edge_within_tolerance_of_the_alignment_survives_the_side_test() {
        // The polyline form of the test above, which is the path with the
        // second reject: both ends of the edge (20, 10) - (20, 50 - 5e-8) lie
        // below the alignment y = 50, one of them by only 5e-8, inside
        // GEOMETRIC. The side test must keep it, and intersect() then reports
        // the crossing at x = 20. A diagonal alignment is added so the side
        // test runs on a line that is not axis-aligned too: (0, 0) - (100,
        // 100) against an edge ending 5e-8 (perpendicular) short of it at
        // (70, 70).
        let mut model = Model::default();
        assert!(model.layers.ensure("0").is_ok());
        let fence = add_on(
            &mut model,
            Geometry::Polyline(line_from(&[
                Point2::new(20.0, 10.0),
                Point2::new(20.0, 50.0 - 5e-8),
            ])),
        );
        let offset = 5e-8 / 2.0_f64.sqrt();
        let spur = add_on(
            &mut model,
            Geometry::Polyline(line_from(&[
                Point2::new(90.0, 50.0),
                Point2::new(70.0 + offset, 70.0 - offset),
            ])),
        );

        let options = SectionOptions {
            interval: 10.0,
            ..SectionOptions::default()
        };
        let across = extract(
            &line_from(&[Point2::new(0.0, 50.0), Point2::new(100.0, 50.0)]),
            &[],
            Some(&model),
            &options,
        );
        // The spur, from (90, 50) on the line itself, crosses it there too:
        // stations 20 (the fence) and 90 (the spur's start).
        assert_eq!(across.crossings.len(), 2);
        assert_eq!(across.crossings[0].entity, fence);
        near(across.crossings[0].station, 20.0, 1e-9);
        assert_eq!(across.crossings[1].entity, spur);
        near(across.crossings[1].station, 90.0, 1e-9);

        // Along the diagonal y = x: the fence (x = 20, y 10 to 50) crosses it
        // at (20, 20), station 20 * sqrt(2); the spur's end at (70, 70), 5e-8
        // off it, is at station 70 * sqrt(2).
        let diagonal = extract(
            &line_from(&[Point2::new(0.0, 0.0), Point2::new(100.0, 100.0)]),
            &[],
            Some(&model),
            &options,
        );
        assert_eq!(diagonal.crossings.len(), 2);
        assert_eq!(diagonal.crossings[0].entity, fence);
        near(diagonal.crossings[0].station, 20.0 * 2.0_f64.sqrt(), 1e-9);
        assert_eq!(diagonal.crossings[1].entity, spur);
        near(diagonal.crossings[1].station, 70.0 * 2.0_f64.sqrt(), 1e-6);
    }
}

// ---- cross section lines --------------------------------------------------------

mod cad_cross_section {
    use super::*;

    #[test]
    fn the_line_is_perpendicular_and_centred_on_the_station() {
        let alignment = line_from(&[Point2::new(0.0, 0.0), Point2::new(100.0, 0.0)]);

        let line = cross_section_line(&alignment, 40.0, 15.0)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(line.vertices.len(), 2);

        // Alignment runs east; the section runs north-south through x = 40.
        near(line.vertices[0].x, 40.0, 1e-9);
        near(line.vertices[1].x, 40.0, 1e-9);
        // Left of travel comes first.
        near(line.vertices[0].y, 15.0, 1e-9);
        near(line.vertices[1].y, -15.0, 1e-9);
        near(line.length(), 30.0, 1e-9);
    }

    #[test]
    fn a_station_on_a_vertex_uses_the_bisector_rather_than_one_side() {
        // A right-angle bend. At the corner the "segment direction" is
        // ambiguous; rounding to either side would swing the section by 45
        // degrees.
        let alignment = line_from(&[
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 0.0),
            Point2::new(100.0, 100.0),
        ]);

        let line = cross_section_line(&alignment, 100.0, 10.0)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let direction = line.vertices[1] - line.vertices[0];
        // The bisector of east-then-north points north-east, so its
        // perpendicular points south-east: equal and opposite components.
        near(direction.x.abs(), direction.y.abs(), 1e-6);
    }

    #[test]
    fn rejects_a_station_outside_the_alignment() {
        let alignment = line_from(&[Point2::new(0.0, 0.0), Point2::new(100.0, 0.0)]);
        assert!(cross_section_line(&alignment, -5.0, 10.0).is_err());
        assert!(cross_section_line(&alignment, 150.0, 10.0).is_err());
        assert!(cross_section_line(&alignment, 50.0, 0.0).is_err());
        assert!(cross_section_line(&alignment, 50.0, -1.0).is_err());
        // The ends themselves are inside.
        assert!(cross_section_line(&alignment, 0.0, 10.0).is_ok());
        assert!(cross_section_line(&alignment, 100.0, 10.0).is_ok());
    }

    #[test]
    fn stations_always_include_both_ends_and_never_drift() {
        let alignment = line_from(&[Point2::new(0.0, 0.0), Point2::new(1000.0, 0.0)]);
        let stations = section_stations(&alignment, 0.1)
            .unwrap_or_else(|error| panic!("{}", error.describe()));

        assert_eq!(stations[0], 0.0);
        near(stations[stations.len() - 1], 1000.0, 1e-9);
        // Accumulating 0.1 ten thousand times drifts by about 1e-12;
        // multiplying does not. The last interior station must still be where
        // it belongs.
        near(stations[5000], 500.0, 1e-12);
        for pair in stations.windows(2) {
            assert!(pair[1] > pair[0]);
        }
    }

    #[test]
    fn stations_reject_nonsense_intervals() {
        let alignment = line_from(&[Point2::new(0.0, 0.0), Point2::new(100.0, 0.0)]);
        assert!(section_stations(&alignment, 0.0).is_err());
        assert!(section_stations(&alignment, -1.0).is_err());
        assert!(section_stations(&alignment, 1e-9).is_err());
        assert!(section_stations(&Polyline2::default(), 1.0).is_err());
    }
}
