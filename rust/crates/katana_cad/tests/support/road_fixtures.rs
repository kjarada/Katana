//! What the road model's tests are built on (tests/cad/road/road_fixtures.hpp):
//! a straight alignment along +x (station s is (s, 0), left is +y), a level
//! design profile, a planar ground that a TIN of four corners reproduces
//! exactly, and small templates whose sections are worked by hand in the
//! tests.
//!
//! Owned by the road model's tests (wave G4); a test target includes it with
//! `#[path = "../support/road_fixtures.rs"] pub mod road_fixtures;`. The C++
//! default arguments are said at every call.

use katana_cad::road::road_template::{
    ComponentKind, PavementCourse, RoadTemplate, Side, default_component,
};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentPi, HorizontalAlignment, Point2, Point3, ProfilePvi, SolvedAlignment, SolvedProfile,
    VerticalAlignment,
};
use katana_terrain::{TinBuildOptions, TinInput, TinSurface, build_tin};

/// z = a x + b y + c over [-100, 400] x [-y_half, y_half] (the C++ defaults
/// y_half to 100).
pub fn plane_ground(a: f64, b: f64, c: f64, y_half: f64) -> TinSurface {
    let mut input = TinInput::default();
    for (x, y) in [
        (-100.0, -y_half),
        (400.0, -y_half),
        (400.0, y_half),
        (-100.0, y_half),
    ] {
        input.points.push(Point3::new(x, y, a * x + b * y + c));
    }
    match build_tin(&input, &TinBuildOptions::default()) {
        Ok(built) => built.surface,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// `length` metres along +x from the origin (the C++ defaults it to 200).
pub fn straight(length: f64) -> SolvedAlignment {
    let definition = HorizontalAlignment {
        pis: vec![
            AlignmentPi {
                point: Point2::new(0.0, 0.0),
                ..AlignmentPi::default()
            },
            AlignmentPi {
                point: Point2::new(length, 0.0),
                ..AlignmentPi::default()
            },
        ],
        ..HorizontalAlignment::default()
    };
    match solve_alignment(&definition) {
        Ok(solved) => solved,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// Level at `elevation` from `from` to `to` (the C++ defaults them to 0 and
/// 200).
pub fn level(elevation: f64, from: f64, to: f64) -> SolvedProfile {
    let definition = VerticalAlignment {
        pvis: vec![
            ProfilePvi {
                station: from,
                elevation,
                ..ProfilePvi::default()
            },
            ProfilePvi {
                station: to,
                elevation,
                ..ProfilePvi::default()
            },
        ],
    };
    match solve_profile(&definition) {
        Ok(solved) => solved,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// A lane of `width` at `crossfall` percent each side with the given courses,
/// then a batter of `cut` and `fill` run per rise (the C++ defaults both to 2).
pub fn lane_template(
    width: f64,
    crossfall: f64,
    courses: Vec<PavementCourse>,
    cut: f64,
    fill: f64,
) -> RoadTemplate {
    let mut made = RoadTemplate {
        name: String::from("test"),
        ..RoadTemplate::default()
    };
    for side in [Side::Left, Side::Right] {
        let mut lane = default_component(ComponentKind::Lane, side);
        lane.width = width;
        lane.crossfall = crossfall;
        lane.courses = courses.clone();
        made.components.push(lane);
        let mut batter = default_component(ComponentKind::Batter, side);
        batter.cut_slope = cut;
        batter.fill_slope = fill;
        made.components.push(batter);
    }
    made
}

/// Nothing but a batter on each side: the hinge is the centreline.
pub fn batters_alone(cut: f64, fill: f64) -> RoadTemplate {
    let mut made = RoadTemplate {
        name: String::from("batters"),
        ..RoadTemplate::default()
    };
    for side in [Side::Left, Side::Right] {
        let mut batter = default_component(ComponentKind::Batter, side);
        batter.cut_slope = cut;
        batter.fill_slope = fill;
        made.components.push(batter);
    }
    made
}
