// A whole corridor and the road model's objects made from it
// (tests/cad/road/test_corridor_objects.cpp; include/katana/cad/road/
// road_section.hpp, corridor_solids): volumes against width x thickness x
// length, closed solids, stable ids, the earthworks against the end areas the
// old assembly measured, strings and surfaces.

use std::collections::{BTreeMap, BTreeSet};

use crate::road_fixtures::{batters_alone, lane_template, level, plane_ground, straight};
use crate::{must, near};
use katana_cad::bim::bim_object::{BimModel, BimObject};
use katana_cad::road::corridor_model::{Corridor, DEFAULT_CURVE_TOLERANCE, TemplateRegion};
use katana_cad::road::road_section::{
    BuiltCorridor, CorridorString, SolidColumn, build_corridor, corridor_sections, corridor_solids,
    corridor_strings, loft_solid, sections_surface,
};
use katana_cad::road::road_template::{
    ComponentKind, KerbShape, RoadTemplate, RoadTemplateSet, Side, default_component, make_course,
    standard_templates,
};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::mesh::mesh_volume;
use katana_geometry::profile::solve_profile;
use katana_geometry::{
    AlignmentPi, HorizontalAlignment, Point2, Point3, ProfilePvi, SolvedAlignment, TriangleMesh,
    VerticalAlignment,
};
use katana_math::cpp;
use katana_math::numerics::{PI, tolerance};
use katana_terrain::{TinBuildOptions, TinInput, TinSurface, build_tin};

// The C++ defaults `interval` to 10.
fn corridor_of(template_name: &str, interval: f64) -> Corridor {
    Corridor {
        name: String::from("Main"),
        alignment: String::from("road"),
        template_name: String::from(template_name),
        interval,
        ..Corridor::default()
    }
}

// A 3.5 m lane at -3 % each side, AC14 50 mm over a 200 mm base.
fn lanes() -> RoadTemplate {
    let mut made = RoadTemplate {
        name: String::from("lanes"),
        ..RoadTemplate::default()
    };
    for side in [Side::Left, Side::Right] {
        let mut lane = default_component(ComponentKind::Lane, side);
        lane.width = 3.5;
        lane.crossfall = -3.0;
        lane.courses = vec![
            make_course("AC14", 0.05, "asphalt", "PAVEMENT"),
            make_course("base", 0.2, "rock", "PAVEMENT"),
        ];
        made.components.push(lane);
    }
    made
}

fn set_of(templates: Vec<RoadTemplate>) -> RoadTemplateSet {
    RoadTemplateSet { templates }
}

fn standard_set() -> RoadTemplateSet {
    set_of(standard_templates())
}

fn object_with_id<'a>(model: &'a BimModel, id: &str) -> Option<&'a BimObject> {
    model.objects.iter().find(|object| object.id == id)
}

#[track_caller]
fn quantity_of(object: &BimObject, name: &str) -> f64 {
    match object
        .quantities
        .iter()
        .find(|quantity| quantity.name == name)
    {
        Some(quantity) => quantity.value,
        None => panic!("{} has no {name}", object.id),
    }
}

#[track_caller]
fn property_pair(object: &BimObject, key: &str, value: &str) -> bool {
    object
        .properties
        .contains(&(String::from(key), String::from(value)))
}

fn alignment_of(pis: Vec<AlignmentPi>) -> SolvedAlignment {
    must(solve_alignment(&HorizontalAlignment {
        pis,
        ..HorizontalAlignment::default()
    }))
}

fn pi(x: f64, y: f64, radius: f64) -> AlignmentPi {
    AlignmentPi {
        point: Point2::new(x, y),
        radius,
        ..AlignmentPi::default()
    }
}

// A curve gentle enough to stay on the test ground: (0,0) to (100,0) to
// (200,40), rounded at 100 m radius.
fn curved() -> SolvedAlignment {
    alignment_of(vec![
        pi(0.0, 0.0, 0.0),
        pi(100.0, 0.0, 100.0),
        pi(200.0, 40.0, 0.0),
    ])
}

fn built_of(input: &TinInput) -> TinSurface {
    must(build_tin(input, &TinBuildOptions::default())).surface
}

// Ground that depends on x alone, on a grid so that every triangle lies in one
// cell of it: 5 m at x = 0, 20 and 40, 15 m at x = 10 and 30, and 5 m beyond.
// Under a design level of 10 the sections at 0, 20 and 40 are in fill and the
// ones at 10 and 30 in cut.
fn saw_ground() -> TinSurface {
    let mut input = TinInput::default();
    for i in -10..=40 {
        let x = 10.0 * f64::from(i);
        let z = if (0..=4).contains(&i) && i % 2 == 1 {
            15.0
        } else {
            5.0
        };
        for y in [-100.0, -50.0, 0.0, 50.0, 100.0] {
            input.points.push(Point3::new(x, y, z));
        }
    }
    built_of(&input)
}

// (0,0) to (100,0) to (100,100), the corner rounded at 50 m: a quarter turn to
// the left, 100 m of tangent and the arc from station 50 to 50 + 25 pi =
// 128.5398 between them, 178.5398 in all.
fn quarter_turn() -> SolvedAlignment {
    alignment_of(vec![
        pi(0.0, 0.0, 0.0),
        pi(100.0, 0.0, 50.0),
        pi(100.0, 100.0, 0.0),
    ])
}

// The AC14 course (50 mm) of the 3.5 m lanes round the quarter turn, with the
// corridor's curve tolerance as given: the left lane's volume, the right's, and
// how many sections the corridor made.
fn lane_volumes_on_the_quarter_turn(curve_tolerance: f64) -> (f64, f64, usize) {
    let mut corridor = corridor_of("lanes", 10.0);
    corridor.curve_tolerance = curve_tolerance;
    let built = build_corridor(
        &corridor,
        &set_of(vec![lanes()]),
        &quarter_turn(),
        &level(10.0, 0.0, 300.0),
        None,
    );
    assert!(built.problems.is_empty(), "{:?}", built.problems);
    let model = corridor_solids(&built);
    let left = object_with_id(&model, "corridor/Main/left/lane/AC14/1")
        .expect("the left lane's AC14 is missing");
    let right = object_with_id(&model, "corridor/Main/right/lane/AC14/1")
        .expect("the right lane's AC14 is missing");
    // The solid's own volume and the plan outline's area say the same: the
    // volume is that area at a thickness of 0.05.
    near(
        quantity_of(left, "GrossVolume"),
        quantity_of(left, "GrossArea") * 0.05,
        1e-9,
    );
    near(
        quantity_of(right, "GrossVolume"),
        quantity_of(right, "GrossArea") * 0.05,
        1e-9,
    );
    (
        quantity_of(left, "GrossVolume"),
        quantity_of(right, "GrossVolume"),
        built.sections,
    )
}

// How many faces use each edge, either way round (smaller index first), and
// each way round.
type EdgeCounts = BTreeMap<(u32, u32), i32>;

fn edges_of(mesh: &TriangleMesh) -> (EdgeCounts, EdgeCounts) {
    let mut undirected = EdgeCounts::new();
    let mut directed = EdgeCounts::new();
    for face in &mesh.faces {
        for (e, &a) in face.iter().enumerate() {
            let b = face[(e + 1) % 3];
            *undirected.entry((a.min(b), a.max(b))).or_default() += 1;
            *directed.entry((a, b)).or_default() += 1;
        }
    }
    (undirected, directed)
}

// Every edge of `mesh` used by exactly two faces, once in each direction: the
// test the IFC writer applies to its indices for "Closed" (docs/ifc.md).
fn closed_on_its_indices(mesh: &TriangleMesh) -> bool {
    let (undirected, directed) = edges_of(mesh);
    !mesh.faces.is_empty()
        && undirected.values().all(|&count| count == 2)
        && directed.values().all(|&count| count == 1)
}

// The area of the smallest triangle: a face of no area is a sliver the mesh is
// closed with and a viewer may refuse.
fn thinnest_triangle(mesh: &TriangleMesh) -> f64 {
    let mut thinnest = f64::INFINITY;
    for i in 0..mesh.triangle_count() {
        let area = mesh.triangle(i).expect("a face of the mesh").area();
        thinnest = cpp::min(thinnest, area);
    }
    thinnest
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

// How many separate pieces the faces hold: faces that share a vertex are one.
fn shells_of(mesh: &TriangleMesh) -> usize {
    let mut parent: Vec<usize> = (0..mesh.vertices.len()).collect();
    let mut used = vec![false; mesh.vertices.len()];
    for face in &mesh.faces {
        for &index in face {
            used[index as usize] = true;
            // The C++ `parent[root(index)] = root(face[0])` evaluates its right
            // side first.
            let into = root(&mut parent, face[0] as usize);
            let from = root(&mut parent, index as usize);
            parent[from] = into;
        }
    }
    let mut roots: BTreeSet<usize> = BTreeSet::new();
    for (i, &is_used) in used.iter().enumerate() {
        if is_used {
            roots.insert(root(&mut parent, i));
        }
    }
    roots.len()
}

// A ground that depends on y alone (a TIN on a grid, as saw_ground is on x):
// 10.5 from the centreline to 0.5 out, falling to 9.5 at 1.5 out, level to
// 2.5, rising to 10.5 at 3.5 and level beyond, the same each side. Under a
// design level of 10, with a 4 m lane and 2:1 batters, each side has a CUT out
// to where the ground crosses 10 (1.0), a FILL to where it crosses back (3.0),
// and a cut again to where the cut batter, rising from the lane's edge at 4 m,
// meets the ground (10.5 at 5.0): two stretches of cut across every section,
// with a fill between them.
fn two_cuts_across_ground() -> TinSurface {
    let profile: [(f64, f64); 6] = [
        (0.0, 10.5),
        (0.5, 10.5),
        (1.5, 9.5),
        (2.5, 9.5),
        (3.5, 10.5),
        (100.0, 10.5),
    ];
    let mut input = TinInput::default();
    for x in [-10.0, 0.0, 50.0, 100.0, 110.0] {
        for (y, z) in profile {
            input.points.push(Point3::new(x, y, z));
            if y != 0.0 {
                input.points.push(Point3::new(x, -y, z));
            }
        }
    }
    built_of(&input)
}

fn column(offset: f64, top: f64, bottom: f64) -> SolidColumn {
    SolidColumn {
        offset,
        top,
        bottom,
    }
}

// The earthworks of a corridor of the lane template over `ground`, its target
// surface called "ground".
fn earthworks_over(ground: &TinSurface, length: f64) -> BuiltCorridor {
    let mut corridor = corridor_of("test", 10.0);
    corridor.surface = String::from("ground");
    build_corridor(
        &corridor,
        &set_of(vec![lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0)]),
        &straight(length),
        &level(10.0, 0.0, length),
        Some(ground),
    )
}

mod corridor_objects {
    use super::*;

    #[test]
    fn course_volumes_on_a_straight_are_width_times_thickness_times_length() {
        // 100 m of a 3.5 m lane: the AC14 is 3.5 x 0.05 x 100 = 17.5 m3 a side,
        // the base 3.5 x 0.2 x 100 = 70 m3; each covers 350 m2 in plan. The
        // crossfall tilts the section into a parallelogram of the same area.
        let built = build_corridor(
            &corridor_of("lanes", 10.0),
            &set_of(vec![lanes()]),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        );
        assert!(built.problems.is_empty(), "{:?}", built.problems);
        assert_eq!(built.runs.len(), 1);
        assert_eq!(built.runs[0].sections.len(), 11); // 0, 10, ..., 100
        let model = corridor_solids(&built);
        assert!(model.problems.is_empty());
        assert_eq!(model.objects.len(), 4);
        for side in ["left", "right"] {
            let wearing =
                object_with_id(&model, &format!("corridor/Main/{side}/lane/AC14/1")).expect(side);
            near(quantity_of(wearing, "GrossVolume"), 17.5, 1e-9);
            near(mesh_volume(&wearing.mesh), 17.5, 1e-9);
            near(quantity_of(wearing, "GrossArea"), 350.0, 1e-9);
            near(quantity_of(wearing, "Length"), 100.0, 1e-12);
            near(quantity_of(wearing, "Width"), 3.5, 1e-12);
            near(quantity_of(wearing, "Depth"), 0.05, 1e-15);
            assert_eq!(wearing.ifc_class, "IfcCourse");
            assert_eq!(wearing.predefined_type, "PAVEMENT");
            assert_eq!(wearing.kind, "pavement");
            assert_eq!(wearing.part, "TRAFFICLANE");
            assert_eq!(wearing.facility, "Main");
            assert_eq!(wearing.material, "asphalt");
            assert_eq!(wearing.layer, "road/pavement");
            let base =
                object_with_id(&model, &format!("corridor/Main/{side}/lane/base/1")).expect(side);
            near(quantity_of(base, "GrossVolume"), 70.0, 1e-9);
        }
    }

    #[test]
    fn every_solid_is_closed_with_each_edge_shared_by_exactly_two_faces_turned_outward() {
        // The urban street - lanes, parking, kerb and gutter, footpath, verge,
        // batter - round a curve, in fill above a plane: every course, kerb and
        // earthworks solid, both sides.
        let templates = standard_set();
        let ground = plane_ground(0.0, 0.0, 8.0, 100.0);
        let profile = must(solve_profile(&VerticalAlignment {
            pvis: vec![
                ProfilePvi {
                    station: 0.0,
                    elevation: 10.0,
                    ..ProfilePvi::default()
                },
                ProfilePvi {
                    station: 300.0,
                    elevation: 10.0,
                    ..ProfilePvi::default()
                },
            ],
        }));
        let built = build_corridor(
            &corridor_of("urban street", 10.0),
            &templates,
            &curved(),
            &profile,
            Some(&ground),
        );
        let model = corridor_solids(&built);
        assert!(model.objects.len() > 10);
        let mut saw_fill = false;
        for object in &model.objects {
            assert!(!object.mesh.empty(), "{}", object.id);
            let (undirected, directed) = edges_of(&object.mesh);
            for (edge, count) in &undirected {
                assert_eq!(*count, 2, "{} edge {}-{}", object.id, edge.0, edge.1);
            }
            // Turned consistently: each edge walked once each way.
            for count in directed.values() {
                assert_eq!(*count, 1, "{}", object.id);
            }
            assert!(
                mesh_volume(&object.mesh) > 0.0,
                "{} is turned inward",
                object.id
            );
            saw_fill = saw_fill || object.ifc_class == "IfcEarthworksFill";
            assert!(!object.plan.is_empty(), "{}", object.id);
        }
        assert!(saw_fill);
    }

    #[test]
    fn object_ids_are_stable_and_name_their_definition() {
        let templates = standard_set();
        let ground = plane_ground(0.0, 0.0, 9.0, 100.0);
        let first = corridor_solids(&build_corridor(
            &corridor_of("urban street", 10.0),
            &templates,
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&ground),
        ));
        let second = corridor_solids(&build_corridor(
            &corridor_of("urban street", 10.0),
            &templates,
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&ground),
        ));
        assert_eq!(first, second);
        for id in [
            "corridor/Main/left/lane/AC14/1",
            "corridor/Main/right/parking/base/1",
            "corridor/Main/left/kerb/1",
            "corridor/Main/left/footpath/concrete/1",
            "corridor/Main/left/verge/topsoil/1",
            "corridor/Main/left/earthworks/fill/1",
            "corridor/Main/right/earthworks/fill/1",
        ] {
            assert!(object_with_id(&first, id).is_some(), "{id}");
        }
        // A footpath's top course is the path, a slab; what is under it a
        // course.
        let slab = object_with_id(&first, "corridor/Main/left/footpath/concrete/1").expect("slab");
        assert_eq!(slab.ifc_class, "IfcSlab");
        assert_eq!(slab.predefined_type, "SIDEWALK");
        let bedding =
            object_with_id(&first, "corridor/Main/left/footpath/bedding/1").expect("bedding");
        assert_eq!(bedding.ifc_class, "IfcCourse");
        assert_eq!(
            object_with_id(&first, "corridor/Main/left/kerb/1")
                .expect("kerb")
                .predefined_type,
            ""
        );
        assert_eq!(
            object_with_id(&first, "corridor/Main/left/earthworks/fill/1")
                .expect("fill")
                .predefined_type,
            "EMBANKMENT"
        );
        let mut ids: Vec<&str> = first
            .objects
            .iter()
            .map(|object| object.id.as_str())
            .collect();
        ids.sort_unstable();
        assert!(
            ids.windows(2).all(|pair| pair[0] != pair[1]),
            "two objects share an id"
        );
    }

    #[test]
    fn a_divided_roads_kerb_and_lanes_are_set_out_under_the_median_not_above_it() {
        // The median kerb's cross-section, by hand: the top 0.15 wide and 0.25
        // thick (level 0 on a base of -0.25) 0.0375 m2, and the face falling over
        // 0.15 m from 0.25 to 0.15 thick, 0.15 x 0.2 = 0.03 m2: 0.0675 m2, 6.75
        // m3 in 100 m, 0.3 m wide. Lane 1 starts under the profile (10): its top
        // reaches 10 - 0.1 = 9.9 at the inner edge, not 10.1.
        let model = corridor_solids(&build_corridor(
            &corridor_of("divided road", 10.0),
            &standard_set(),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        ));
        for side in ["left", "right"] {
            let kerb = object_with_id(&model, &format!("corridor/Main/{side}/kerb/1")).expect(side);
            near(quantity_of(kerb, "Width"), 0.3, 1e-12);
            near(quantity_of(kerb, "GrossVolume"), 6.75, 1e-9);
            assert!(mesh_volume(&kerb.mesh) > 0.0, "turned outward");
            assert!(property_pair(kerb, "mountable", "true"));
            let lane =
                object_with_id(&model, &format!("corridor/Main/{side}/lane1/AC14/1")).expect(side);
            near(lane.mesh.bounds().max.z, 9.9, 1e-12);
        }
    }

    #[test]
    fn a_flush_edge_strip_reports_how_thick_it_is_and_no_face_height() {
        // 0.15 m wide at 0 % and 0.15 m thick: 0.0225 m2, 2.25 m3 in 100 m. It
        // has no face: its Height was the kerb default 0.15 (or whatever was
        // typed, 5 m in the finding) and the IFC base quantity carried it.
        let mut made = RoadTemplate {
            name: String::from("strips"),
            ..RoadTemplate::default()
        };
        for side in [Side::Left, Side::Right] {
            let lane = default_component(ComponentKind::Lane, side);
            let mut strip = default_component(ComponentKind::Kerb, side);
            strip.shape = KerbShape::EdgeStrip;
            strip.crossfall = 0.0;
            strip.height = 5.0;
            made.components.extend([lane, strip]);
        }
        let model = corridor_solids(&build_corridor(
            &corridor_of("strips", 10.0),
            &set_of(vec![made]),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        ));
        let strip = object_with_id(&model, "corridor/Main/left/kerb/1").expect("the strip");
        near(quantity_of(strip, "Width"), 0.15, 1e-12);
        near(quantity_of(strip, "Depth"), 0.15, 1e-12);
        near(quantity_of(strip, "GrossVolume"), 2.25, 1e-9);
        assert!(strip.quantities.iter().all(|each| each.name != "Height"));
        // A kerb with a face keeps its Height, the face.
        let urban = corridor_solids(&build_corridor(
            &corridor_of("urban street", 10.0),
            &standard_set(),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        ));
        near(
            quantity_of(
                object_with_id(&urban, "corridor/Main/left/kerb/1").expect("the kerb"),
                "Height",
            ),
            0.15,
            1e-15,
        );
    }

    #[test]
    fn a_kerbs_length_is_its_lip_line_and_its_shape_is_its_width() {
        let model = corridor_solids(&build_corridor(
            &corridor_of("urban street", 10.0),
            &standard_set(),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        ));
        let kerb = object_with_id(&model, "corridor/Main/left/kerb/1").expect("the kerb");
        assert_eq!(kerb.kind, "kerb");
        assert_eq!(kerb.ifc_class, "IfcKerb");
        near(quantity_of(kerb, "Length"), 100.0, 1e-9);
        near(quantity_of(kerb, "Width"), 0.475, 1e-12); // kerb and gutter
        near(quantity_of(kerb, "Height"), 0.15, 1e-15);
        // Its cross-section, column to column: the channel 0.3 wide and 0.15
        // thick (0.045), the face 0.025 wide from 0.15 thick to 0.3 (0.005625),
        // the top 0.15 wide and 0.3 thick (0.045): 0.095625 m2, 9.5625 m3.
        near(quantity_of(kerb, "GrossVolume"), 9.5625, 1e-9);
    }

    #[test]
    fn the_earthworks_are_the_end_areas_the_old_assembly_measured() {
        // The textbook cut - 1 m under flat ground, 8 m wide, 2:1 batters, 10 m2
        // a section - over 200 m: 1000 m3 each side, 2000 in all, as
        // corridor_quantities has always given it.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let mut corridor = corridor_of("test", 10.0);
        corridor.surface = String::from("ground");
        let built = build_corridor(
            &corridor,
            &set_of(vec![lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0)]),
            &straight(200.0),
            &level(-1.0, 0.0, 200.0),
            Some(&ground),
        );
        near(built.cut, 2000.0, 1e-6);
        near(built.fill, 0.0, 1e-9);
        assert_eq!(built.incomplete_sections, 0);
        let model = corridor_solids(&built);
        let left = object_with_id(&model, "corridor/Main/left/earthworks/cut/1").expect("left");
        let right = object_with_id(&model, "corridor/Main/right/earthworks/cut/1").expect("right");
        near(quantity_of(left, "GrossVolume"), 1000.0, 1e-6);
        near(quantity_of(right, "GrossVolume"), 1000.0, 1e-6);
        assert_eq!(left.ifc_class, "IfcEarthworksCut");
        assert_eq!(left.predefined_type, "CUT");
        // The surface the cut is taken from, which the IFC export reads.
        assert!(property_pair(left, "Target Surface", "ground"));
        assert_eq!(left.layer, "road/earthworks/cut");
        // No fill anywhere: no solid of nothing.
        assert!(object_with_id(&model, "corridor/Main/left/earthworks/fill/1").is_none());
        // The solid is drawn through 41 columns a side, every 0.15 m of its 6,
        // so it holds the end area's volume less the corner the columns at 3.9
        // and 4.05 cut off the lane's edge at 4: a triangle of 0.5 x (0.1 x
        // 0.025) = 0.00125 m2 a section, 0.25 m3 over 200 m - 999.75.
        near(mesh_volume(&left.mesh), 999.75, 1e-6);
    }

    #[test]
    fn an_embankment_of_batters_alone_is_the_triangles_under_their_slopes_and_its_subgrade_rises() {
        // Ground at 9, design 10, a 1 in 4 fill batter each side and nothing
        // between: a triangle 0.5 x 4 x 1 = 2 m2 a side a section, over 100 m
        // 200 m3 a side and 400 in all. The subgrade is the batters' own line,
        // so it reaches the design level at the centreline and falls 1 over 4 m
        // either side: 9.5 two metres out.
        let low = plane_ground(0.0, 0.0, 9.0, 100.0);
        let mut corridor = corridor_of("batters", 10.0);
        corridor.surface = String::from("ground");
        let built = build_corridor(
            &corridor,
            &set_of(vec![batters_alone(2.0, 4.0)]),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            Some(&low),
        );
        assert!(built.problems.is_empty(), "{:?}", built.problems);
        near(built.fill, 400.0, 1e-6);
        near(built.cut, 0.0, 1e-9);
        let model = corridor_solids(&built);
        for side in ["left", "right"] {
            let fill = object_with_id(&model, &format!("corridor/Main/{side}/earthworks/fill/1"))
                .expect(side);
            near(quantity_of(fill, "GrossVolume"), 200.0, 1e-6);
            near(mesh_volume(&fill.mesh), 200.0, 1e-6); // a straight batter needs no columns
        }
        let alignment = built.alignment.as_ref().expect("the corridor's alignment");
        let subgrade = must(sections_surface(
            &corridor_sections(&built),
            alignment,
            true,
            false,
        ));
        let at = |x: f64, y: f64| {
            subgrade
                .surface
                .elevation_at_xy(x, y)
                .expect("on the surface")
        };
        near(at(50.0, 0.0), 10.0, 1e-9);
        near(at(50.0, 2.0), 9.5, 1e-9);
        near(at(50.0, -2.0), 9.5, 1e-9);

        // The same in cut: ground at 11, a 1 in 2 cut batter climbs 1 over 2 m,
        // 0.5 x 2 x 1 = 1 m2 a side, 100 m3 a side; the subgrade is 10 at the
        // centreline and 10.5 one metre out, and not the flat 11 it read when
        // the line ran from daylight to daylight.
        let high = plane_ground(0.0, 0.0, 11.0, 100.0);
        let dug = build_corridor(
            &corridor,
            &set_of(vec![batters_alone(2.0, 4.0)]),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            Some(&high),
        );
        near(dug.cut, 200.0, 1e-6);
        near(dug.fill, 0.0, 1e-9);
        let trench = must(sections_surface(
            &corridor_sections(&dug),
            dug.alignment.as_ref().expect("the corridor's alignment"),
            true,
            false,
        ));
        near(
            trench.surface.elevation_at_xy(50.0, 0.0).expect("on it"),
            10.0,
            1e-9,
        );
        near(
            trench.surface.elevation_at_xy(50.0, 1.0).expect("on it"),
            10.5,
            1e-9,
        );
    }

    #[test]
    fn a_side_with_cut_and_fill_gives_each_its_own_footprint_and_the_two_add_up_to_the_side() {
        // Ground z = 9.5 + 0.25 y, rising to the left; design 10; a 4 m lane at
        // 0 % with 2:1 batters. Left of the centreline the ground is below the
        // lane out to y = 2 (z 10) and above it beyond: fill thick 0.5 at the
        // centreline down to nothing at 2 m (0.5 x 2 x 0.5 = 0.5 m2); cut thick
        // nothing at 2 m, 0.5 at the hinge (4, 10, the ground 10.5), nothing
        // again where the batter (10 + d/2) meets the ground (10.5 + d/4) at d =
        // 2, out 6: two triangles of 0.5 x 2 x 0.5, 1.0 m2. Right of the
        // centreline the ground falls away: fill 0.5 at the centreline and 1.5
        // at the hinge (ground 8.5), (0.5 + 1.5) / 2 x 4 = 4 m2 under the lane;
        // the batter 10 - d/2 meets 8.5 - d/4 at d = 6, out 10: 0.5 x 6 x 1.5 =
        // 4.5 m2. Over 100 m: volumes 50, 100 and 850 m3; footprints 2, 4 and 10
        // m wide.
        let ground = plane_ground(0.0, 0.25, 9.5, 100.0);
        let built = earthworks_over(&ground, 100.0);
        assert!(built.problems.is_empty(), "{:?}", built.problems);
        near(built.cut, 100.0, 1e-6);
        near(built.fill, 900.0, 1e-6);
        let model = corridor_solids(&built);
        assert_eq!(
            model.objects.len(),
            3,
            "left cut, left fill, right fill; no right cut"
        );
        assert!(object_with_id(&model, "corridor/Main/right/earthworks/cut/1").is_none());

        let left_fill =
            object_with_id(&model, "corridor/Main/left/earthworks/fill/1").expect("left fill");
        let left_cut =
            object_with_id(&model, "corridor/Main/left/earthworks/cut/1").expect("left cut");
        let right_fill =
            object_with_id(&model, "corridor/Main/right/earthworks/fill/1").expect("right fill");
        near(quantity_of(left_fill, "GrossVolume"), 50.0, 1e-6);
        near(quantity_of(left_cut, "GrossVolume"), 100.0, 1e-6);
        near(quantity_of(right_fill, "GrossVolume"), 850.0, 1e-6);
        // The footprints: fill 0 to 2 m out, cut 2 to 6, so together the 6 m
        // from the centreline to the daylight and not 6 m twice; the right fill
        // 10 m. The solids are drawn over the same stretches, so a straight
        // thickness needs no columns to be exact.
        near(quantity_of(left_fill, "GrossArea"), 2.0 * 100.0, 1e-6);
        near(quantity_of(left_cut, "GrossArea"), 4.0 * 100.0, 1e-6);
        near(quantity_of(right_fill, "GrossArea"), 10.0 * 100.0, 1e-6);
        near(
            quantity_of(left_fill, "GrossArea") + quantity_of(left_cut, "GrossArea"),
            600.0,
            1e-6,
        );
        near(mesh_volume(&left_fill.mesh), 50.0, 1e-6);
        near(mesh_volume(&left_cut.mesh), 100.0, 1e-6);
        near(mesh_volume(&right_fill.mesh), 850.0, 1e-6);
        // The plan outline is the footprint too: the cut's is a strip 4 m wide
        // from 2 m out.
        let bounds = left_cut.plan[0].bounding_box();
        near(bounds.min.y, 2.0, 1e-9);
        near(bounds.max.y, 6.0, 1e-9);
    }

    #[test]
    fn a_fill_that_ends_along_the_road_is_a_solid_of_its_own_stations_and_the_cut_begins_where_it_ends()
     {
        // Ground z = 5 + 0.1 x under a design level of 10: it meets the design
        // at x = 50, in fill before it and in cut after. A 4 m lane, 2:1
        // batters. At 10 m sections the fill depth D = 5, 4, 3, 2, 1, 0 at x = 0
        // ... 50 gives, a side, 4 D + D2 = 45, 32, 21, 12, 5, 0 m2 (the lane,
        // and the batter's triangle 0.5 x 2D x D) and a footprint 4 + 2 D = 14,
        // 12, 10, 8, 6 m wide. End areas: (45 + 32) 5 + (32 + 21) 5 + (21 + 12)
        // 5 + (12 + 5) 5 + (5 + 0) 5 = 925 m3. Plan area: the trapezoids (14 +
        // 12) 5 + (12 + 10) 5 + (10 + 8) 5 + (8 + 6) 5 = 130 + 110 + 90 + 70 =
        // 400, and the last interval narrowing from 6 m to a point, 0.5 x 6 x 10
        // = 30: 430 m2. The cut mirrors it from x = 50 to 100.
        let ground = plane_ground(0.1, 0.0, 5.0, 100.0);
        let built = earthworks_over(&ground, 100.0);
        near(built.cut, 1850.0, 1e-5);
        near(built.fill, 1850.0, 1e-5);
        let model = corridor_solids(&built);
        assert_eq!(model.objects.len(), 4);
        let property = |object: &BimObject, name: &str| -> String {
            object
                .properties
                .iter()
                .find(|(key, _)| key == name)
                .map_or_else(|| String::from("(none)"), |(_, value)| value.clone())
        };
        for side in ["left", "right"] {
            let fill = object_with_id(&model, &format!("corridor/Main/{side}/earthworks/fill/1"))
                .expect(side);
            let cut = object_with_id(&model, &format!("corridor/Main/{side}/earthworks/cut/1"))
                .expect(side);
            assert_eq!(property(fill, "from"), "0");
            assert_eq!(property(fill, "to"), "50");
            assert_eq!(property(cut, "from"), "50");
            assert_eq!(property(cut, "to"), "100");
            near(quantity_of(fill, "Length"), 50.0, 1e-9);
            near(quantity_of(cut, "Length"), 50.0, 1e-9);
            near(quantity_of(fill, "GrossVolume"), 925.0, 1e-5);
            near(quantity_of(cut, "GrossVolume"), 925.0, 1e-5);
            near(quantity_of(fill, "GrossArea"), 430.0, 1e-5);
            near(quantity_of(cut, "GrossArea"), 430.0, 1e-5);
        }
    }

    #[test]
    fn earthworks_split_where_their_thickness_ends_and_the_second_piece_carries_its_number() {
        // The sawtooth ground, 40 m of road at 10 m sections: fill at 0, 20 and
        // 40 (D = 5: 45 m2 a side, 14 m wide), cut at 10 and 30 (45 m2, 14 m
        // wide), nothing at the sections between. Each piece runs over its
        // sections and one more at each end, where its end area tapers to
        // nothing:
        //
        //   fill 1    x 0-10    (45 + 0) 5 = 225 m3,  triangle 0.5 x 14 x 10 = 70 m2
        //   fill 1-2  x 10-30   450 m3, a diamond 20 long and 14 wide, 140 m2
        //   fill 1-3  x 30-40   225 m3, 70 m2
        //   cut 1     x 0-20    450 m3, 140 m2;  cut 1-2  x 20-40  450 m3, 140 m2.
        let ground = saw_ground();
        let built = earthworks_over(&ground, 40.0);
        assert_eq!(built.incomplete_sections, 0);
        let model = corridor_solids(&built);
        let expected: [(&str, f64, f64, f64); 5] = [
            ("corridor/Main/left/earthworks/fill/1", 10.0, 225.0, 70.0),
            ("corridor/Main/left/earthworks/fill/1-2", 20.0, 450.0, 140.0),
            ("corridor/Main/left/earthworks/fill/1-3", 10.0, 225.0, 70.0),
            ("corridor/Main/left/earthworks/cut/1", 20.0, 450.0, 140.0),
            ("corridor/Main/left/earthworks/cut/1-2", 20.0, 450.0, 140.0),
        ];
        for (id, length, volume, area) in expected {
            let object = object_with_id(&model, id).expect(id);
            near(quantity_of(object, "Length"), length, 1e-9);
            near(quantity_of(object, "GrossVolume"), volume, 1e-5);
            near(quantity_of(object, "GrossArea"), area, 1e-5);
        }
        assert!(object_with_id(&model, "corridor/Main/left/earthworks/fill/1-4").is_none());
        assert!(object_with_id(&model, "corridor/Main/left/earthworks/cut/1-3").is_none());
        // Five pieces a side, ten in all, and the pieces' volumes are the
        // corridor's: 900 m3 of fill and 900 of cut a side.
        assert_eq!(model.objects.len(), 10);
        near(built.fill, 1800.0, 1e-5);
        near(built.cut, 1800.0, 1e-5);
    }

    #[test]
    fn ground_on_the_other_side_of_the_design_by_nothing_makes_no_earthworks_at_all() {
        // The daylight is found by bisection, to about 1e-13 m, so it lands on
        // either side of the ground by that much and leaves a sliver of the
        // other kind of earthworks, about 1e-26 m2, that a test of "area > 0"
        // took for a solid: a full-length cut object with the whole
        // half-section's area beside a road in fill, and a fill beside one in
        // cut. Ground tilted across the road, so the daylight is never at a
        // round number.
        for tilt in [0.03, 0.05, 0.07, 0.11] {
            let below = plane_ground(0.0, tilt, 5.0, 100.0); // under the design of 10
            let fill = earthworks_over(&below, 100.0);
            assert_eq!(fill.incomplete_sections, 0, "{tilt}");
            assert_eq!(fill.cut, 0.0, "no cut at all, not 1e-26 of it, tilt {tilt}");
            assert!(fill.fill > 0.0);
            let fill_model = corridor_solids(&fill);
            assert_eq!(fill_model.objects.len(), 2, "{tilt}");
            for object in &fill_model.objects {
                assert_eq!(
                    object.ifc_class, "IfcEarthworksFill",
                    "{} tilt {tilt}",
                    object.id
                );
            }

            let above = plane_ground(0.0, tilt, 15.0, 100.0); // over it
            let cut = earthworks_over(&above, 100.0);
            assert_eq!(cut.fill, 0.0, "no fill at all, tilt {tilt}");
            assert!(cut.cut > 0.0);
            let cut_model = corridor_solids(&cut);
            assert_eq!(cut_model.objects.len(), 2, "{tilt}");
            for object in &cut_model.objects {
                assert_eq!(
                    object.ifc_class, "IfcEarthworksCut",
                    "{} tilt {tilt}",
                    object.id
                );
            }
        }

        // Ground at the design level: nothing to build, no object, no area.
        let flat = plane_ground(0.0, 0.0, 10.0, 100.0);
        let at_grade = earthworks_over(&flat, 100.0);
        assert_eq!(at_grade.cut, 0.0);
        assert_eq!(at_grade.fill, 0.0);
        assert!(corridor_solids(&at_grade).objects.is_empty());

        // The tolerance is the model's 0.1 mm (the TIN's duplicate tolerance): a
        // sheet of cut 0.05 mm thick is not earthworks, 0.2 mm is. A side of
        // it: 4 m of lane at 0.0002 m, 8e-4 m2, and the batter 2:1 up to the
        // ground 0.0004 m out, 0.5 x 0.0004 x 0.0002 = 4e-8 m2: 0.080004 m3 over
        // 100 m, 0.160008 m3 both sides.
        let thin = plane_ground(0.0, 0.0, 10.00005, 100.0);
        let sheet = earthworks_over(&thin, 100.0);
        assert_eq!(sheet.cut, 0.0);
        assert!(corridor_solids(&sheet).objects.is_empty());
        let thick = plane_ground(0.0, 0.0, 10.0002, 100.0);
        let layer = earthworks_over(&thick, 100.0);
        near(layer.cut, 0.160008, 1e-6);
        assert_eq!(corridor_solids(&layer).objects.len(), 2);
    }

    #[test]
    fn a_region_is_a_run_of_its_own_numbered_in_station_order() {
        // The lanes template, with a region of the textbook cut template from 40
        // to 60: three runs, 1 (0-40), 2 (40-60), 3 (60-100).
        let mut corridor = corridor_of("lanes", 10.0);
        corridor.regions = vec![TemplateRegion {
            start: 40.0,
            end: 60.0,
            template_name: String::from("test"),
        }];
        let built = build_corridor(
            &corridor,
            &set_of(vec![lanes(), lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0)]),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        );
        assert_eq!(built.runs.len(), 3);
        assert_eq!(built.runs[1].region, 2);
        assert_eq!(built.runs[1].road_template.name, "test");
        assert_eq!(built.runs[1].sections[0].station, 40.0);
        let sections = &built.runs[1].sections;
        assert_eq!(sections[sections.len() - 1].station, 60.0);
        let model = corridor_solids(&built);
        let first = object_with_id(&model, "corridor/Main/left/lane/AC14/1").expect("run 1");
        let last = object_with_id(&model, "corridor/Main/left/lane/AC14/3").expect("run 3");
        near(quantity_of(first, "GrossVolume"), 3.5 * 0.05 * 40.0, 1e-9);
        near(quantity_of(last, "GrossVolume"), 3.5 * 0.05 * 40.0, 1e-9);
        // The region's template has no courses: run 2 has no course solids.
        assert!(object_with_id(&model, "corridor/Main/left/lane/AC14/2").is_none());
    }

    #[test]
    fn a_regions_strings_run_over_its_own_stations_and_the_surface_steps_at_its_ends() {
        // The rural two-lane road with the urban street from 40 to 60, on a 100
        // m straight at 10 m: runs 0-40, 40-60 and 60-100, 5 + 3 + 5 = 13
        // sections.
        let mut corridor = corridor_of("rural two-lane", 10.0);
        corridor.regions = vec![TemplateRegion {
            start: 40.0,
            end: 60.0,
            template_name: String::from("urban street"),
        }];
        let built = build_corridor(
            &corridor,
            &standard_set(),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        );
        assert_eq!(built.runs.len(), 3);
        assert_eq!(built.sections, 13);

        // Strings, by code: the first and last x of each string made.
        let strings = corridor_strings(&built, false);
        let mut spans: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
        for string in &strings {
            spans
                .entry(string.code.clone())
                .or_default()
                .push((string.points[0].x, string.points[string.points.len() - 1].x));
        }
        let expect_spans = |code: &str, expected: &[(f64, f64)]| {
            let found = spans.get(code).cloned().unwrap_or_default();
            assert_eq!(found.len(), expected.len(), "{code}");
            for (i, (got, wanted)) in found.iter().zip(expected).enumerate() {
                assert!((got.0 - wanted.0).abs() <= 1e-9, "{code} string {i}");
                assert!((got.1 - wanted.1).abs() <= 1e-9, "{code} string {i}");
            }
        };
        // What only the region has runs its whole 40 to 60 (it began at 50: the
        // region's section at 40 was dropped for the rural run's).
        expect_spans("left/footpath/outer", &[(40.0, 60.0)]);
        expect_spans("right/kerb/back", &[(40.0, 60.0)]);
        // What only the rural template has stops at 40 and resumes at 60 (it
        // resumed at 70: the section at 60 was dropped for the urban run's).
        expect_spans("left/shoulder/outer", &[(0.0, 40.0), (60.0, 100.0)]);
        expect_spans("right/drain/invert", &[(0.0, 40.0), (60.0, 100.0)]);
        // What both have is broken at each join, not carried across one.
        expect_spans(
            "left/lane/outer",
            &[(0.0, 40.0), (40.0, 60.0), (60.0, 100.0)],
        );
        expect_spans("CL", &[(0.0, 40.0), (40.0, 60.0), (60.0, 100.0)]);
        // The centreline and 4 + 4 rural codes (the lane's and shoulder's edges
        // and the drain's invert and outer edge) a side before and after the
        // region, and the urban street's centreline and 7 + 7 for the region: 9
        // + 15 + 9.
        assert_eq!(strings.len(), 33);

        // The finished surface: the urban street's kerb top (9.964: 10 less the
        // lane's 3.3 x 3 % = 0.099 and the parking's 2.3 x 3 % = 0.069 to the lip
        // at 9.832, the gutter 0.3 m falling 6 % to 9.814, the face up 0.15 to
        // 9.964) at 6 m out in the region, the rural drain (5.5 m at 9.835, its
        // invert at 6.5 m 0.5 down, so 9.585 at 6 m) outside it - and a step
        // between, not a blend: 1 mm either side of the join the surface is each
        // template's.
        let alignment = built.alignment.as_ref().expect("the corridor's alignment");
        let finished = must(sections_surface(
            &corridor_sections(&built),
            alignment,
            false,
            false,
        ));
        assert_eq!(
            finished.sections, 13,
            "every section built, the join's both"
        );
        let level6 = |station: f64| {
            finished
                .surface
                .elevation_at_xy(station, 6.0)
                .unwrap_or_else(|| panic!("{station} is off the surface"))
        };
        near(level6(35.0), 9.585, 1e-9);
        near(level6(39.999), 9.585, 1e-9);
        near(level6(40.002), 9.964, 1e-9);
        near(level6(45.0), 9.964, 1e-9);
        near(level6(55.0), 9.964, 1e-9);
        near(level6(59.999), 9.964, 1e-9);
        near(level6(60.002), 9.585, 1e-9);
        near(level6(65.0), 9.585, 1e-9);
    }

    #[test]
    fn what_cannot_be_built_is_listed_and_the_rest_is_built() {
        // No surface for the batters; a region naming a template that is not
        // there; the rest of the corridor still has its pavement.
        let mut corridor = corridor_of("test", 10.0);
        corridor.regions = vec![TemplateRegion {
            start: 40.0,
            end: 60.0,
            template_name: String::from("nowhere"),
        }];
        let built = build_corridor(
            &corridor,
            &set_of(vec![lane_template(
                4.0,
                0.0,
                vec![make_course("AC", 0.05, "asphalt", "PAVEMENT")],
                2.0,
                2.0,
            )]),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        );
        let mentions = |words: &str| built.problems.iter().any(|problem| problem.contains(words));
        assert!(mentions("nowhere"), "the missing template");
        assert!(mentions("no target surface"), "the batters");
        assert_eq!(built.runs.len(), 2); // 0-40 and 60-100
        let model = corridor_solids(&built);
        assert!(object_with_id(&model, "corridor/Main/left/lane/AC/1").is_some());
        assert!(object_with_id(&model, "corridor/Main/left/lane/AC/3").is_some());
        assert_eq!(model.problems.len(), built.problems.len());
        assert_eq!(model.problems[0].source, "corridor Main");
    }

    #[test]
    fn an_alignment_and_profile_that_do_not_overlap_build_nothing_and_say_so() {
        let built = build_corridor(
            &corridor_of("lanes", 10.0),
            &set_of(vec![lanes()]),
            &straight(100.0),
            &level(10.0, 300.0, 400.0),
            None,
        );
        assert!(built.runs.is_empty());
        assert!(!built.problems.is_empty());
        assert!(built.problems[0].contains("do not overlap"));
        assert!(corridor_solids(&built).objects.is_empty());
    }

    #[test]
    fn strings_run_along_each_code_and_the_surfaces_lie_on_the_sections() {
        let built = build_corridor(
            &corridor_of("lanes", 10.0),
            &set_of(vec![lanes()]),
            &straight(100.0),
            &level(10.0, 0.0, 100.0),
            None,
        );
        let strings: Vec<CorridorString> = corridor_strings(&built, false);
        let mut points: BTreeMap<String, usize> = BTreeMap::new();
        for string in &strings {
            points.insert(string.code.clone(), string.points.len());
        }
        let expected: BTreeMap<String, usize> = [
            (String::from("CL"), 11),
            (String::from("left/lane/outer"), 11),
            (String::from("right/lane/outer"), 11),
        ]
        .into_iter()
        .collect();
        assert_eq!(points, expected);

        let alignment = built.alignment.as_ref().expect("the corridor's alignment");
        let finished = must(sections_surface(
            &corridor_sections(&built),
            alignment,
            false,
            false,
        ));
        // On the centreline the design level; halfway across the lane 1.75 x 3
        // % = 0.0525 below it.
        near(
            finished.surface.elevation_at_xy(50.0, 0.0).expect("on it"),
            10.0,
            1e-9,
        );
        near(
            finished.surface.elevation_at_xy(50.0, 1.75).expect("on it"),
            9.9475,
            1e-9,
        );
        let formation = must(sections_surface(
            &corridor_sections(&built),
            alignment,
            true,
            false,
        ));
        near(
            formation.surface.elevation_at_xy(50.0, 0.0).expect("on it"),
            9.75,
            1e-9,
        );
        assert_eq!(formation.sections, 11);
    }

    #[test]
    fn the_solids_loft_at_the_sections_and_refuse_too_few_of_them() {
        let alignment = straight(100.0);
        assert!(
            loft_solid(
                &alignment,
                &[0.0],
                &[vec![column(0.0, 1.0, 0.0), column(1.0, 1.0, 0.0)]],
                0.0
            )
            .is_err()
        );
        assert!(
            loft_solid(
                &alignment,
                &[0.0, 10.0],
                &[
                    vec![column(0.0, 1.0, 0.0), column(1.0, 1.0, 0.0)],
                    vec![column(0.0, 1.0, 0.0)]
                ],
                0.0
            )
            .is_err()
        );
        // A 1 x 1 x 10 box: 8 corners, 12 triangles, 10 m3.
        let section = vec![column(0.0, 1.0, 0.0), column(1.0, 1.0, 0.0)];
        let solid = must(loft_solid(
            &alignment,
            &[0.0, 10.0],
            &[section.clone(), section],
            0.0,
        ));
        assert_eq!(solid.vertices.len(), 8);
        assert_eq!(solid.faces.len(), 12);
        near(mesh_volume(&solid), 10.0, 1e-12);
    }

    // ---- courses and kerbs round a curve (the chord error, docs/road_design.md) ----------

    #[test]
    fn a_courses_volume_round_a_quarter_circle_is_the_annulus_sectors_to_the_curve_tolerance() {
        // The left lane is on the inside of a left turn: 3.5 m wide, from r =
        // 46.5 to r = 50; the right lane from 50 to 53.5. Each AC14 is 50 mm
        // thick and lies over 100 m of tangent (3.5 x 100 x 0.05 = 17.5 m3) and
        // a quarter of an annulus, pi/4 (R2^2 - R1^2) x 0.05:
        //   left   pi/4 (2500 - 2162.25) = pi/4 x 337.75 = 265.268 m2   13.2634 m3
        //   right  pi/4 (2862.25 - 2500) = pi/4 x 362.25 = 284.510 m2   14.2255 m3
        // 30.7634 and 31.7255 m3 in all.
        let left_arc = PI / 4.0 * (50.0 * 50.0 - 46.5 * 46.5) * 0.05;
        let right_arc = PI / 4.0 * (53.5 * 53.5 - 50.0 * 50.0) * 0.05;
        let left_exact = 17.5 + left_arc;
        let right_exact = 17.5 + right_arc;
        let (left, right, sections) = lane_volumes_on_the_quarter_turn(DEFAULT_CURVE_TOLERANCE);
        // A tolerance of 0.001 holds the arc's part to a thousandth - the chords
        // are short by theta^2 / 6 at most, theta = 0.0775 - and it is always
        // short, never over. (Without it the same arc is 0.7 % short: below.)
        assert!(left <= left_exact);
        assert!(right <= right_exact);
        assert!(left >= left_exact - 0.001 * left_arc);
        assert!(right >= right_exact - 0.001 * right_arc);
        // The sections: 20 the interval and the keys give (0, 10, ..., 120,
        // 128.5398, 130, ..., 170, 178.5398) and two more in each of the arc's
        // eight gaps (three parts each: 10 m turns 0.2 radians, over the 0.0775
        // allowed), 36.
        assert_eq!(sections, 36);
    }

    #[test]
    fn a_courses_volume_round_a_quarter_circle_with_no_tolerance_is_the_chords_of_the_interval_stations()
     {
        // curve_tolerance 0 adds no section: the 20 of the interval and the
        // keys, and the quads between them. Round the arc the stations are 50,
        // 60, ..., 120 (seven steps of 10 m, theta = 0.2) and 128.5398 (8.5398
        // m, theta = 0.17080). An annulus sector's chord quad is 0.5 sin(theta)
        // (R2^2 - R1^2), so the arc holds
        //   left   0.5 (7 sin 0.2 + sin 0.17080) 337.75 = 263.555 m2  13.1778 m3
        //   right  0.5 (7 sin 0.2 + sin 0.17080) 362.25 = 282.673 m2  14.1337 m3
        // against the exact 265.268 and 284.510: 0.65 % short, theta^2 / 6.
        let mut sines = 7.0 * (10.0_f64 / 50.0).sin();
        sines += ((25.0 * PI - 70.0) / 50.0).sin();
        let left_chords = 17.5 + 0.5 * sines * (50.0 * 50.0 - 46.5 * 46.5) * 0.05;
        let right_chords = 17.5 + 0.5 * sines * (53.5 * 53.5 - 50.0 * 50.0) * 0.05;
        let (left, right, sections) = lane_volumes_on_the_quarter_turn(0.0);
        near(left, left_chords, 1e-9);
        near(right, right_chords, 1e-9);
        assert_eq!(sections, 20);
        // And they are short of the curve by far more than the default allows.
        let left_arc = PI / 4.0 * (50.0 * 50.0 - 46.5 * 46.5) * 0.05;
        assert!(left < 17.5 + left_arc - 0.001 * left_arc);
    }

    #[test]
    fn a_kerbs_length_round_a_quarter_circle_is_its_arc_to_a_quarter_of_the_curve_tolerance() {
        // The urban street's left kerb has its lip 3.3 + 2.3 = 5.6 m from the
        // centreline, r = 44.4 m on the inside of the turn: 100 m of tangent and
        // pi/2 x 44.4 = 69.7434 m of arc, 169.7434 in all. A chord holds
        // sin(theta/2)/(theta/2) of its arc, theta^2 / 24 short, so with theta =
        // 0.0775 the length is within 0.001/4 of the arc's 69.74: 0.0174 m.
        // Without the added sections the same kerb is 69.6306 m on the arc (the
        // 7 x 10 m and 8.5398 m chords, 2 r sin(theta/2) each), 0.113 short.
        let templates = standard_set();
        let exact = 100.0 + PI / 2.0 * 44.4;
        let mut corridor = corridor_of("urban street", 10.0);
        let built = build_corridor(
            &corridor,
            &templates,
            &quarter_turn(),
            &level(10.0, 0.0, 300.0),
            None,
        );
        let model = corridor_solids(&built);
        let kerb = object_with_id(&model, "corridor/Main/left/kerb/1").expect("the kerb");
        let length = quantity_of(kerb, "Length");
        assert!(length <= exact);
        assert!(length >= exact - 0.001 / 4.0 * (PI / 2.0 * 44.4));
        corridor.curve_tolerance = 0.0;
        let chords = corridor_solids(&build_corridor(
            &corridor,
            &templates,
            &quarter_turn(),
            &level(10.0, 0.0, 300.0),
            None,
        ));
        let chorded = object_with_id(&chords, "corridor/Main/left/kerb/1").expect("the kerb");
        // 100 + the seven 10 m chords at theta = 0.2 and the 8.5398 m one.
        let mut arc = 7.0 * 2.0 * 44.4 * 0.1_f64.sin();
        arc += 2.0 * 44.4 * ((25.0 * PI - 70.0) / 100.0).sin();
        near(quantity_of(chorded, "Length"), 100.0 + arc, 1e-9);
    }

    // ---- solids that taper: no face of no area (docs/road_design.md, "A closed solid") ----

    #[test]
    fn a_solid_that_tapers_to_a_line_is_a_wedge_welded_to_it_with_no_face_of_no_area() {
        // Three columns 1 m apart, 1 m thick at station 0 and nothing at station
        // 10 (its top and bottom are the one line at level 0). Vertices: 6 at
        // station 0 and 3 at 10. Triangles: the top 4 and the bottom 4 over two
        // cells, the inner and the outer face one each (a quad whose far side is
        // a line is one triangle), the cap at 0 four, the cap at 10 none: 14. V -
        // E + F = 9 - 21 + 14 = 2, a sphere. The volume is half the end area, 2 x
        // 1 = 2 m2, over 10 m: 10 m3 (a wedge).
        let alignment = straight(100.0);
        let wedge = must(loft_solid(
            &alignment,
            &[0.0, 10.0],
            &[
                vec![
                    column(0.0, 1.0, 0.0),
                    column(1.0, 1.0, 0.0),
                    column(2.0, 1.0, 0.0),
                ],
                vec![
                    column(0.0, 0.0, 0.0),
                    column(1.0, 0.0, 0.0),
                    column(2.0, 0.0, 0.0),
                ],
            ],
            0.0,
        ));
        assert_eq!(wedge.vertices.len(), 9);
        assert_eq!(wedge.faces.len(), 14);
        assert!(closed_on_its_indices(&wedge));
        near(mesh_volume(&wedge), 10.0, 1e-12);
        assert!(
            thinnest_triangle(&wedge) > 1e-3,
            "a face of no area is left in"
        );
    }

    #[test]
    fn an_edge_column_of_no_thickness_is_a_welded_line_and_makes_no_face_of_no_area() {
        // Four columns, thick 1 m but the last, which is a line (ground meeting
        // the formation): two stations, 7 vertices each (the last column is
        // one), 14. Triangles: top 6, bottom 6, inner 2, outer 0 (all four
        // corners are the one line), the two caps 5 each (their last cell is one
        // triangle): 24. V - E + F = 14 - 36 + 24 = 2. The section is 1 + 1 + 0.5
        // = 2.5 m2, 25 m3.
        let alignment = straight(100.0);
        let section = vec![
            column(0.0, 1.0, 0.0),
            column(1.0, 1.0, 0.0),
            column(2.0, 1.0, 0.0),
            column(3.0, 0.0, 0.0),
        ];
        let solid = must(loft_solid(
            &alignment,
            &[0.0, 10.0],
            &[section.clone(), section],
            0.0,
        ));
        assert_eq!(solid.vertices.len(), 14);
        assert_eq!(solid.faces.len(), 24);
        assert!(closed_on_its_indices(&solid));
        near(mesh_volume(&solid), 25.0, 1e-12);
        assert!(thinnest_triangle(&solid) > 1e-3);

        // A thickness under the weld bound is a line too, at the top's level (5
        // vertices a station); over it, a face (6). 0.00005 m is under the
        // model's 0.1 mm, 0.0002 over it.
        for (thickness, welds) in [(0.00005, true), (0.0002, false)] {
            let thin = vec![
                column(0.0, 1.0, 0.0),
                column(1.0, 1.0, 0.0),
                column(2.0, 1.0, 1.0 - thickness),
            ];
            let made = must(loft_solid(
                &alignment,
                &[0.0, 10.0],
                &[thin.clone(), thin],
                1e-4,
            ));
            assert_eq!(
                made.vertices.len(),
                if welds { 10 } else { 12 },
                "{thickness}"
            );
            assert!(closed_on_its_indices(&made), "{thickness}");
        }
    }

    #[test]
    fn two_stretches_with_nothing_between_them_are_two_shells_and_the_gap_has_no_face() {
        // Six columns, 1 m thick but the middle two (a gap), on two stations
        // that are both ends of the solid: the cell across the gap has nothing
        // between its top and its bottom at its four corners, two faces on one
        // surface, and is left out. Vertices: 10 a station (the middle two are
        // one each), 20. Triangles: top 8, bottom 8, inner 4, outer 4, the caps
        // 6 each: 32. Two spheres: V - E + F = 20 - 48 + 32 = 4. Each shell is 1
        // + 0.5 = 1.5 m2: 30 m3 in all, over 10 m.
        let alignment = straight(100.0);
        let section = vec![
            column(0.0, 1.0, 0.0),
            column(1.0, 1.0, 0.0),
            column(2.0, 0.0, 0.0),
            column(3.0, 0.0, 0.0),
            column(4.0, 1.0, 0.0),
            column(5.0, 1.0, 0.0),
        ];
        let solid = must(loft_solid(
            &alignment,
            &[0.0, 10.0],
            &[section.clone(), section],
            0.0,
        ));
        assert_eq!(solid.vertices.len(), 20);
        assert_eq!(solid.faces.len(), 32);
        assert!(closed_on_its_indices(&solid));
        assert_eq!(shells_of(&solid), 2);
        near(mesh_volume(&solid), 30.0, 1e-12);
        assert!(thinnest_triangle(&solid) > 1e-3);
    }

    #[test]
    fn an_interior_column_of_no_thickness_is_not_welded_so_a_solid_never_pinches_to_a_line() {
        // Three stations, 1 m thick everywhere but the middle column of the
        // middle station, which is nothing. It is not welded: welded, the line
        // it makes with a body on each side would be an edge four faces use. So
        // the solid has 6 vertices a station, 18, and 32 triangles (top 8,
        // bottom 8, inner 4, outer 4, the two caps 4 each), V - E + F = 18 - 48
        // + 32 = 2. Its volume is the plan area of each of the four cells' two
        // triangles, 5 m2 a triangle, times the mean thickness at its corners:
        // the cells have 2/3, 2/3; 1, 2/3; 2/3, 1; 2/3, 2/3: 5 x (4/3 + 5/3 +
        // 5/3 + 4/3) = 30 m3.
        let alignment = straight(100.0);
        let thick = vec![
            column(0.0, 1.0, 0.0),
            column(1.0, 1.0, 0.0),
            column(2.0, 1.0, 0.0),
        ];
        let pinched = vec![
            column(0.0, 1.0, 0.0),
            column(1.0, 0.0, 0.0),
            column(2.0, 1.0, 0.0),
        ];
        let solid = must(loft_solid(
            &alignment,
            &[0.0, 10.0, 20.0],
            &[thick.clone(), pinched, thick],
            0.0,
        ));
        assert_eq!(solid.vertices.len(), 18);
        assert_eq!(solid.faces.len(), 32);
        assert!(closed_on_its_indices(&solid));
        assert_eq!(shells_of(&solid), 1);
        near(mesh_volume(&solid), 30.0, 1e-12);
        assert!(thinnest_triangle(&solid) > 1e-3);
    }

    #[test]
    fn a_tapering_earthworks_piece_ends_in_a_line_and_its_volume_is_that_of_the_solid_it_was() {
        // The sawtooth ground (as earthworks_split_where...): the fill beside the
        // lane is 5 m thick to 4 m out and falls to nothing at 14 (2:1), at
        // sections 0, 20 and 40, and there is none at 10 and 30. fill/1 runs
        // from 0 (the thick section) to 10 (a line along the formation),
        // fill/1-2 from 10 to 30 with its thick section at 20, fill/1-3 from 30
        // to 40.
        //
        // fill/1: 41 columns of 0.35 m, 41 + 40 vertices at station 0 (the last
        // is a line) and 41 at 10: 122; top 80 + bottom 80 + inner 1 + outer 0 +
        // cap at 0 79 (its last cell is one triangle) + cap at 10 none: 240
        // triangles. V - E + F = 122 - 360 + 240 = 2.
        //
        // fill/1-2: a line, the thick section, a line: 41 + 81 + 41 = 163
        // vertices; the cell between the line at 10 and the thick section's
        // last column has all three corners (10, 39), (10, 40) and (20, 40)
        // welded, so its top and bottom triangle (one surface twice) are left
        // out and (10, 40) is in no face: 162 vertices; 160 + 158 + the wall at
        // the first column 2 = 320 triangles: 162 - 480 + 320 = 2.
        //
        // The mesh volume is Sum of plan area x mean thickness of each
        // triangle's corners, the plan cells 0.35 x 10 split a to c: for a
        // section thick t_i at its columns and a line beyond it that is 1.75 (2
        // t_i + t_(i+1)) / 3 a cell, and the other way round 1.75 (2 t_(i+1) +
        // t_i) / 3. With t = 5 to 4 m out and 5 - (out - 4) / 2 beyond:
        // 226.42083 and 223.50417, 449.925 for the two ends together - the end
        // area's 225, 225 and 450 less and plus a diagonal's worth, as the old
        // solid (a face of no area at its end) was.
        let ground = saw_ground();
        let built = earthworks_over(&ground, 40.0);
        let model = corridor_solids(&built);
        let first = object_with_id(&model, "corridor/Main/left/earthworks/fill/1").expect("1");
        let middle = object_with_id(&model, "corridor/Main/left/earthworks/fill/1-2").expect("1-2");
        let last = object_with_id(&model, "corridor/Main/left/earthworks/fill/1-3").expect("1-3");
        assert_eq!(first.mesh.vertices.len(), 122);
        assert_eq!(first.mesh.faces.len(), 240);
        assert_eq!(middle.mesh.vertices.len(), 162);
        assert_eq!(middle.mesh.faces.len(), 320);
        near(mesh_volume(&first.mesh), 226.42083333333, 1e-6);
        near(mesh_volume(&middle.mesh), 449.925, 1e-6);
        near(mesh_volume(&last.mesh), 223.50416666667, 1e-6);
        // The quantities are the end areas', as they were.
        near(quantity_of(first, "GrossVolume"), 225.0, 1e-5);
        near(quantity_of(middle, "GrossVolume"), 450.0, 1e-5);
        near(quantity_of(first, "GrossArea"), 70.0, 1e-5);
        near(quantity_of(middle, "GrossArea"), 140.0, 1e-5);
        for object in [first, middle, last] {
            assert!(closed_on_its_indices(&object.mesh), "{}", object.id);
            assert!(
                mesh_volume(&object.mesh) > 0.0,
                "{} is turned inward",
                object.id
            );
            assert!(
                thinnest_triangle(&object.mesh) > tolerance::ABSOLUTE,
                "{} has a face of no area",
                object.id
            );
        }
    }

    #[test]
    fn a_section_with_cut_fill_and_cut_across_it_is_two_shells_of_cut_and_one_of_fill() {
        // two_cuts_across_ground: each side has, at every section, cut from the
        // centreline to 1.0 (0.5 thick to 0.5 out, then falling to nothing: 0.25
        // + 0.125 = 0.375 m2), fill from 1.0 to 3.0 (0.125 + 0.5 + 0.125 = 0.75
        // m2), and cut again from 3.0 to 5.0 (0.125 + 0.25 + 0.25 = 0.625 m2).
        // Over 100 m: cut 100 m3, fill 75 m3 a side. The ground's kinks and the
        // lane's edge are on columns of 0.025 and 0.05 m (the cut's stretches
        // are 1 and 2 m wide, the fill's 2, each in 40 steps), so the solids hold
        // exactly that. The cut was one solid whose columns spanned the 2 m gap
        // with no thickness; it is two shells now, and the fill one.
        let ground = two_cuts_across_ground();
        let built = earthworks_over(&ground, 100.0);
        assert_eq!(built.incomplete_sections, 0);
        near(built.cut, 200.0, 1e-5);
        near(built.fill, 150.0, 1e-5);
        let model = corridor_solids(&built);
        assert_eq!(
            model.objects.len(),
            4,
            "a cut and a fill a side, one piece each"
        );
        for side in ["left", "right"] {
            let cut = object_with_id(&model, &format!("corridor/Main/{side}/earthworks/cut/1"))
                .expect(side);
            let fill = object_with_id(&model, &format!("corridor/Main/{side}/earthworks/fill/1"))
                .expect(side);
            near(quantity_of(cut, "GrossVolume"), 100.0, 1e-5);
            near(quantity_of(fill, "GrossVolume"), 75.0, 1e-5);
            near(mesh_volume(&cut.mesh), 100.0, 1e-5);
            near(mesh_volume(&fill.mesh), 75.0, 1e-5);
            assert_eq!(shells_of(&cut.mesh), 2, "{side}");
            assert_eq!(shells_of(&fill.mesh), 1, "{side}");
            for object in [cut, fill] {
                assert!(closed_on_its_indices(&object.mesh), "{}", object.id);
                assert!(
                    thinnest_triangle(&object.mesh) > tolerance::ABSOLUTE,
                    "{}",
                    object.id
                );
            }
            // Drawn and measured as their hull (a plan outline is one polygon):
            // the cut from the centreline to 5 m out, the fill from 1 to 3.
            near(quantity_of(cut, "GrossArea"), 500.0, 1e-5);
            near(quantity_of(fill, "GrossArea"), 200.0, 1e-5);
        }
    }

    #[test]
    fn no_earthworks_solid_of_the_standard_templates_has_a_face_of_no_area_or_is_open() {
        // The three standard templates over ground that tilts across the road
        // (cut and fill on a side), over the sawtooth ground (pieces that taper
        // away along the road) and over the ground with two cuts across: every
        // earthworks solid is closed on its own indices, turned outward, and has
        // no triangle of less area than math::tolerance::ABSOLUTE (1e-12 m2): a
        // face of no area is 0, or a sliver at the daylight a few 1e-13 m thick.
        // The cases are reached: the solids are counted that have fewer vertices
        // than 41 columns top and bottom on every station would (2 x 41 a
        // station), which every solid whose edge or end is a line has, and each
        // ground must have some.
        let templates = standard_set();
        // (a tilt of 0.25 would run parallel to the standard fill batter, 1V:4H,
        // and never daylight)
        let cases: Vec<(&str, TinSurface, f64)> = vec![
            ("tilted", plane_ground(0.0, 0.1, 9.5, 100.0), 100.0),
            ("sawtooth", saw_ground(), 40.0),
            ("two cuts across", two_cuts_across_ground(), 100.0),
        ];
        for (name, ground, length) in &cases {
            let mut seen = 0;
            let mut welded = 0;
            for road_template in &templates.templates {
                let mut corridor = corridor_of(&road_template.name, 10.0);
                corridor.surface = String::from("ground");
                let built = build_corridor(
                    &corridor,
                    &templates,
                    &straight(*length),
                    &level(10.0, 0.0, *length),
                    Some(ground),
                );
                for object in &corridor_solids(&built).objects {
                    if object.kind != "earthworks" {
                        continue;
                    }
                    seen += 1;
                    assert!(
                        closed_on_its_indices(&object.mesh),
                        "{name} {} {}",
                        road_template.name,
                        object.id
                    );
                    assert!(
                        mesh_volume(&object.mesh) > 0.0,
                        "{} is turned inward",
                        object.id
                    );
                    assert!(
                        thinnest_triangle(&object.mesh) > tolerance::ABSOLUTE,
                        "{name} {} {}",
                        road_template.name,
                        object.id
                    );
                    // One shell of 41 columns would have 82 vertices a station,
                    // 41 at a line and 81 where one end is.
                    let stations = quantity_of(object, "Length") / 10.0 + 1.0;
                    if (object.mesh.vertices.len() as f64) < 82.0 * stations {
                        welded += 1;
                    }
                }
            }
            assert!(seen > 0, "{name} made no earthworks: nothing was tested");
            assert!(
                welded > 0,
                "{name} welded nothing: no taper or edge was reached"
            );
        }
    }
}
