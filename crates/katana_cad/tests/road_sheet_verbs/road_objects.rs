// Road objects (tests/cad/road/test_road_objects.cpp; road/road_objects.rs):
// each kind made at the place and the size worked out by hand, its solids
// closed, its ids stable, its definition kept as JSON, and what cannot be
// placed refused or listed - never drawn at a guessed level.
//
// The site most tests stand on: alignment Main, straight from (0,0) to
// (100,0) - so station s, offset o is the point (s, o), left of the
// centreline being +y - with a flat design profile at level 10 (or rising
// 2 m over its length, 2 percent, where a test says so).

use std::f64::consts::PI;
use std::sync::Arc;

use katana_cad::Document;
use katana_cad::bim::bim_object::{BimModel, BimObject, BimProblem, Quantity, placed_objects};
use katana_cad::road::road_object_mesh::{add_box, is_closed_mesh};
use katana_cad::road::road_objects::{
    HeightSource, ParameterValue, Parameters, PlacedVertex, PlacementKind, RoadObject,
    RoadObjectKind, RoadObjectPlacement, RoadObjectSet, apply_key, build_road_object, check_built,
    culvert_ends, effective_parameters, parse_parameter, placement_context, road_objects_from_json,
    road_objects_to_json, shape_problem, total_quantities,
};
use katana_core::ErrorCode;
use katana_entity::Color;
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_geometry::{Point2, Point3, TriangleMesh, Vec2};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

use crate::{Session, contains, head, must, near, road_site};

// The object's 3D objects as the drawing makes them now.
#[track_caller]
fn built(site: &Session, id: &str) -> BimModel {
    let set = site.document.road_objects();
    let Some(object) = set.find(id) else {
        panic!("{id}");
    };
    build_road_object(&placement_context(&site.document), object)
}

fn quantity<'a>(object: &'a BimObject, name: &str) -> Option<&'a Quantity> {
    object
        .quantities
        .iter()
        .find(|quantity| quantity.name == name)
}

#[track_caller]
fn quantity_value(object: &BimObject, name: &str) -> f64 {
    match quantity(object, name) {
        Some(found) => found.value,
        None => panic!("{} has no {name}", object.id),
    }
}

// A plane z = 100 + 0.01 x over x -10..110, y -10..10: two triangles, wound
// counter-clockwise.
fn sloping_ground() -> Arc<TinSurface> {
    let z = |x: f64| 100.0 + 0.01 * x;
    let tin = TinSurface::create(
        vec![
            Point3::new(-10.0, -10.0, z(-10.0)),
            Point3::new(110.0, -10.0, z(110.0)),
            Point3::new(110.0, 10.0, z(110.0)),
            Point3::new(-10.0, 10.0, z(-10.0)),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    );
    Arc::new(must(tin))
}

// The sloping ground in `site`'s drawing as "ground".
#[track_caller]
fn put_ground(site: &mut Session) {
    let mut store = SurfaceStore::new();
    must(store.add(NamedSurface {
        name: String::from("ground"),
        surface: Some(sloping_ground()),
        source: String::from("test"),
        ..NamedSurface::default()
    }));
    must(site.document.set_surfaces(store, Document::SET_SURFACES));
}

// A parameter that is a number.
#[track_caller]
fn number_of(parameters: &Parameters, key: &str) -> f64 {
    match parameters.get(key) {
        Some(ParameterValue::Number(value)) => *value,
        other => panic!("{key} is {other:?}"),
    }
}

fn total_of<'a>(totals: &'a [Quantity], name: &str) -> Option<&'a Quantity> {
    totals.iter().find(|total| total.name == name)
}

#[track_caller]
fn total_value(totals: &[Quantity], name: &str) -> f64 {
    match total_of(totals, name) {
        Some(found) => found.value,
        None => panic!("no total {name}"),
    }
}

// The centre of a mesh's plan bounds.
fn centre_of(object: &BimObject) -> Point2 {
    let bounds = object.mesh.bounds();
    Point2::new(
        (bounds.min.x + bounds.max.x) / 2.0,
        (bounds.min.y + bounds.max.y) / 2.0,
    )
}

// A 10 m square at level 5.5, drawn as a closed polyline: it is stored with its
// first corner again at the end, five vertices for four corners.
#[track_caller]
fn draw_closed_square(site: &mut Session) {
    site.ok("PLINE 0,0 10,0 10,10 0,10 CLOSE");
    site.ok("SELECT ALL");
    site.ok("PROP SET elevation 5.5");
}

fn vertex(x: f64, y: f64, z: f64) -> PlacedVertex {
    PlacedVertex {
        position: Point2::new(x, y),
        z: Some(z),
    }
}

mod road_objects {
    use super::*;

    #[test]
    fn a_sign_at_station50_offset4_stands_on_the_profile_facing_the_traffic() {
        // By hand: the spot is (50, 4) at level 10. One 75 mm post centred on
        // it; the 900 x 600 plate on its front, facing back along the
        // alignment (-x) at the traffic coming up it, its back 37.5 mm in
        // front of the spot (x = 49.9625) and 3 mm thick (to 49.9595), across
        // y 4 -+ 0.45, from 1.5 m above the ground to 2.1 m.
        let mut site = road_site(true, 0.0);
        let reply = site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        assert!(
            contains(&reply, "added id=sign-1 kind=sign placement=alignment"),
            "{reply}"
        );
        let model = built(&site, "sign-1");
        assert!(model.problems.is_empty());
        assert_eq!(model.objects.len(), 1);
        let sign = &model.objects[0];
        assert_eq!(sign.id, "object/sign-1/1");
        assert_eq!(sign.ifc_class, "IfcSign");
        assert_eq!(sign.predefined_type, "PICTORAL");
        assert_eq!(sign.layer, "road/objects/sign");
        assert!(is_closed_mesh(&sign.mesh));
        let bounds = sign.mesh.bounds();
        near(bounds.min.x, 49.9595, 1e-9);
        near(bounds.max.x, 50.0375, 1e-9);
        near(bounds.min.y, 3.55, 1e-9);
        near(bounds.max.y, 4.45, 1e-9);
        near(bounds.min.z, 10.0, 1e-9);
        near(bounds.max.z, 12.1, 1e-9);
        near(quantity_value(sign, "Height"), 2.1, 1e-12);
        near(quantity_value(sign, "GrossArea"), 0.9 * 0.6, 1e-12);
        assert_eq!(quantity_value(sign, "Count"), 1.0);
    }

    #[test]
    fn an_object_on_a_graded_profile_takes_its_stations_level_and_the_crossfall() {
        // 10 rising 2 over 100: 11 at station 50; 4 m out at -3 percent falls
        // 0.12 to 10.88.
        let mut site = road_site(true, 2.0);
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 crossfall=-3");
        let model = built(&site, "sign-1");
        assert_eq!(model.objects.len(), 1);
        near(model.objects[0].mesh.bounds().min.z, 10.88, 1e-9);
    }

    #[test]
    fn a_barrier_runs_its_stations_with_a_post_every_spacing_and_an_end_post() {
        // 10 to 60 is 50 m; posts every 1.905 m: 26 whole spacings make 27
        // posts to 49.53, and the end post at 50 makes 28. Left of the
        // centreline at 3, its traffic is on its right, so the 83 mm rail is on
        // the face of the 150 mm posts towards y = 0: y 2.842 to 2.925, the
        // posts 2.925 to 3.075; the top of rail 787 mm above level 10.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=10 to=60 offset=3");
        let model = built(&site, "barrier-1");
        assert_eq!(model.objects.len(), 1);
        let barrier = &model.objects[0];
        assert_eq!(barrier.id, "object/barrier-1");
        assert_eq!(barrier.ifc_class, "IfcRailing");
        assert_eq!(barrier.predefined_type, "GUARDRAIL");
        assert!(is_closed_mesh(&barrier.mesh));
        near(quantity_value(barrier, "Length"), 50.0, 1e-9);
        assert_eq!(quantity_value(barrier, "Count"), 28.0);
        near(quantity_value(barrier, "Height"), 0.787, 1e-12);
        let bounds = barrier.mesh.bounds();
        near(bounds.min.y, 3.0 - 0.075 - 0.083, 1e-9);
        near(bounds.max.y, 3.0 + 0.075, 1e-9);
        near(bounds.max.z, 10.787, 1e-9);
        assert_eq!(barrier.plan.len(), 1);
        assert_eq!(
            barrier.plan[0].vertices.first(),
            Some(&Point2::new(10.0, 3.0))
        );
        assert_eq!(
            barrier.plan[0].vertices.last(),
            Some(&Point2::new(60.0, 3.0))
        );
    }

    #[test]
    fn a_single_slope_barriers_volume_is_its_section_times_its_length() {
        // A trapezoid 0.610 at the base, 0.203 at the top, 1.067 tall:
        // (0.610 + 0.203) / 2 x 1.067 = 0.4337355 m2, over 20 m 8.67471 m3.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD barrier type=singleslope ALIGNMENT Main station=0 to=20 offset=-2");
        let model = built(&site, "barrier-1");
        assert_eq!(model.objects.len(), 1);
        let barrier = &model.objects[0];
        assert!(is_closed_mesh(&barrier.mesh));
        assert_eq!(barrier.material, "concrete");
        near(quantity_value(barrier, "GrossVolume"), 8.67471, 1e-9);
        assert!(
            quantity(barrier, "Count").is_none(),
            "a concrete barrier has no posts"
        );
    }

    #[test]
    fn an_f_shape_barrier_has_the_published_face_angles() {
        // Worked from the shape's definition, not the code's: a 76 mm reveal,
        // 55 degrees from the horizontal to 254 mm, 84 degrees to the top at
        // 813 mm, on a 610 mm base. Half-widths: 0.305 at the foot, at the
        // knee 0.305 - 0.178 / tan 55, at the top that less 0.559 / tan 84.
        let base: f64 = 0.305;
        let knee = base - (0.254 - 0.076) / (55.0 * PI / 180.0).tan();
        let top = knee - (0.813 - 0.254) / (84.0 * PI / 180.0).tan();
        let area = 2.0
            * (base * 0.076
                + (base + knee) / 2.0 * (0.254 - 0.076)
                + (knee + top) / 2.0 * (0.813 - 0.254));
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD barrier type=fshape ALIGNMENT Main station=30 to=40");
        let model = built(&site, "barrier-1");
        assert_eq!(model.objects.len(), 1);
        let barrier = &model.objects[0];
        assert!(is_closed_mesh(&barrier.mesh));
        near(quantity_value(barrier, "GrossVolume"), area * 10.0, 1e-9);
        // Centred on the alignment: its foot is 0.305 either side.
        let bounds = barrier.mesh.bounds();
        near(bounds.max.y, base, 1e-9);
        near(bounds.max.z, 10.813, 1e-9);
    }

    #[test]
    fn a_retaining_wall_stands_on_its_strip_footing_and_its_face_is_length_times_height() {
        // 30 m of the default: a 2.0 m stem 0.25 thick, 0.25 x 2.0 x 30 = 15 m3,
        // its face 30 x 2 = 60 m2; and below the ground its 1.2 x 0.4 footing,
        // an IfcFooting of its own, 1.2 x 0.4 x 30 = 14.4 m3.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD wall ALIGNMENT Main station=0 to=30 offset=-5");
        let model = built(&site, "wall-1");
        assert_eq!(model.objects.len(), 2);
        let wall = &model.objects[0];
        assert_eq!(wall.id, "object/wall-1");
        assert_eq!(wall.ifc_class, "IfcWall");
        assert_eq!(wall.predefined_type, "RETAININGWALL");
        assert!(is_closed_mesh(&wall.mesh));
        near(quantity_value(wall, "Length"), 30.0, 1e-9);
        near(quantity_value(wall, "GrossArea"), 60.0, 1e-9);
        near(quantity_value(wall, "GrossVolume"), 15.0, 1e-9);
        near(wall.mesh.bounds().min.z, 10.0, 1e-9);
        let footing = &model.objects[1];
        assert_eq!(footing.id, "object/wall-1/footing");
        assert_eq!(footing.ifc_class, "IfcFooting");
        assert_eq!(footing.predefined_type, "STRIP_FOOTING");
        assert!(is_closed_mesh(&footing.mesh));
        near(quantity_value(footing, "GrossVolume"), 14.4, 1e-9);
        near(footing.mesh.bounds().min.z, 9.6, 1e-9);
        near(footing.mesh.bounds().max.z, 10.0, 1e-9);
        // Of the site, as no road was named: no part of one.
        assert!(wall.facility.is_empty());
        assert!(wall.part.is_empty());
    }

    #[test]
    fn a_noise_wall_is_a_user_defined_wall_said_to_be_a_noise_barrier_with_no_footing() {
        let mut site = road_site(true, 0.0);
        site.ok(
            "ROADOBJECT ADD wall type=noise facility=\"Main Road\" ALIGNMENT Main station=0 \
             to=10 offset=8",
        );
        let model = built(&site, "wall-1");
        assert_eq!(model.objects.len(), 1);
        let wall = &model.objects[0];
        assert_eq!(wall.predefined_type, "USERDEFINED");
        let noise = (String::from("ObjectType"), String::from("NOISEBARRIER"));
        assert!(wall.properties.contains(&noise));
        // A part of the road named.
        assert_eq!(wall.facility, "Main Road");
        assert_eq!(wall.part, "ROADSIDE");
        // 4.0 x 0.2 x 10 = 8 m3, standing on the ground.
        near(quantity_value(wall, "GrossVolume"), 8.0, 1e-9);
        near(wall.mesh.bounds().min.z, 10.0, 1e-9);
    }

    #[test]
    fn a_culvert_skewed_thirty_degrees_has_its_ends_where_the_skew_puts_them() {
        // Across Main at station 50, 20 m long, turned 30 degrees from square:
        // its axis is the left normal (0,1) turned 30 degrees counter-clockwise,
        // (-sin 30, cos 30) = (-0.5, 0.8660254), so the inlet is 10 m back along
        // it at (55, -8.660254) and the outlet at (45, 8.660254). The default
        // 900 mm pipe with a 100 mm wall under 600 mm of cover on level 10: the
        // invert is 10 - 0.6 - 0.1 - 0.9 = 8.4 at both ends.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD culvert ALIGNMENT Main station=50 skew=30");
        let set = site.document.road_objects();
        let culvert = set.find("culvert-1").expect("culvert-1");
        let ends = must(culvert_ends(&placement_context(&site.document), culvert));
        let half = 10.0 * (PI / 6.0).cos();
        near(ends.inlet.x, 55.0, 1e-9);
        near(ends.inlet.y, -half, 1e-9);
        near(ends.inlet.z, 8.4, 1e-9);
        near(ends.outlet.x, 45.0, 1e-9);
        near(ends.outlet.y, half, 1e-9);
        near(ends.outlet.z, 8.4, 1e-9);

        let model = built(&site, "culvert-1");
        assert_eq!(model.objects.len(), 3, "the barrel and two headwalls");
        let barrel = &model.objects[0];
        assert_eq!(barrel.id, "object/culvert-1/barrel");
        assert_eq!(barrel.ifc_class, "IfcPipeSegment");
        assert_eq!(barrel.predefined_type, "CULVERT");
        assert!(is_closed_mesh(&barrel.mesh));
        near(quantity_value(barrel, "Length"), 20.0, 1e-9);
        // The ring between two 24-gons of circumradius 0.55 and 0.45:
        // 24/2 x sin 15 deg x (0.55^2 - 0.45^2), over 20 m.
        let ring = 12.0 * (PI / 12.0).sin() * (0.55 * 0.55 - 0.45 * 0.45);
        near(quantity_value(barrel, "GrossVolume"), ring * 20.0, 1e-9);
        assert_eq!(model.objects[1].id, "object/culvert-1/headwall/inlet");
        assert_eq!(model.objects[2].id, "object/culvert-1/headwall/outlet");
        for headwall in [&model.objects[1], &model.objects[2]] {
            assert!(is_closed_mesh(&headwall.mesh), "{}", headwall.id);
            // 0.25 thick, the barrel's 1.1 and 0.3 either side wide, 1.1 and
            // 0.3 above tall: 0.25 x 1.7 x 1.4.
            near(
                quantity_value(headwall, "GrossVolume"),
                0.25 * 1.7 * 1.4,
                1e-9,
            );
            near(headwall.mesh.bounds().min.z, 8.3, 1e-9);
        }
    }

    #[test]
    fn a_culvert_at_two_points_runs_from_the_first_to_the_second_at_their_inverts() {
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD culvert AT 0,20,5 10,20,4.5 headwalls=off");
        let model = built(&site, "culvert-1");
        assert_eq!(model.objects.len(), 1);
        // The 3D length of a barrel falling 0.5 over 10: sqrt(100.25).
        near(
            quantity_value(&model.objects[0], "Length"),
            100.25_f64.sqrt(),
            1e-12,
        );
        let set = site.document.road_objects();
        let culvert = set.find("culvert-1").expect("culvert-1");
        let ends = must(culvert_ends(&placement_context(&site.document), culvert));
        assert_eq!(ends.inlet.z, 5.0);
        assert_eq!(ends.outlet.z, 4.5);
    }

    #[test]
    fn a_culvert_at_one_point_is_refused() {
        let mut site = road_site(true, 0.0);
        let error = site.refused("ROADOBJECT ADD culvert AT 0,20,5");
        assert!(contains(&error.message, "two"), "{}", error.describe());
        assert!(site.document.road_objects().objects.is_empty());
    }

    #[test]
    fn guide_posts_at_a_spacing_stand_only_at_whole_spacings() {
        // 0 to 25 every 10: at 0, 10 and 20, not at the end.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD guidepost ALIGNMENT Main station=0 to=25 spacing=10 offset=-4");
        let model = built(&site, "guidepost-1");
        assert_eq!(model.objects.len(), 3);
        for (i, post) in model.objects.iter().enumerate() {
            assert_eq!(post.id, format!("object/guidepost-1/{}", i + 1));
            assert!(is_closed_mesh(&post.mesh));
            let bounds = post.mesh.bounds();
            near((bounds.min.x + bounds.max.x) / 2.0, 10.0 * i as f64, 1e-9);
            near((bounds.min.y + bounds.max.y) / 2.0, -4.0, 1e-9);
            near(bounds.max.z - bounds.min.z, 1.0, 1e-12);
        }
    }

    #[test]
    fn a_light_poles_arm_reaches_towards_the_centreline() {
        // At (20, 6), left of the centreline: the 1.5 m arm reaches to (20, 4.5)
        // and the 600 x 300 x 150 lantern hangs under its 80 mm arm, from
        // 10 + 10 - 0.08 - 0.15 = 19.77 to 19.92.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD light ALIGNMENT Main station=20 offset=6");
        let model = built(&site, "light-1");
        assert_eq!(model.objects.len(), 2);
        assert_eq!(model.objects[0].id, "object/light-1/1/pole");
        assert_eq!(model.objects[1].id, "object/light-1/1/luminaire");
        assert_eq!(model.objects[1].ifc_class, "IfcLightFixture");
        assert!(is_closed_mesh(&model.objects[0].mesh));
        assert!(is_closed_mesh(&model.objects[1].mesh));
        near(model.objects[0].mesh.bounds().max.z, 20.0, 1e-9);
        let lantern = model.objects[1].mesh.bounds();
        near(lantern.min.x, 19.85, 1e-9);
        near(lantern.max.x, 20.15, 1e-9);
        near(lantern.min.y, 4.2, 1e-9);
        near(lantern.max.y, 4.8, 1e-9);
        near(lantern.min.z, 19.77, 1e-9);
        near(lantern.max.z, 19.92, 1e-9);
    }

    #[test]
    fn a_lane_lines_marks_follow_the_three_and_nine_pattern() {
        // 100 m from a mark at 0: marks start at 0, 12 ... 96, nine of them, the
        // last 96 to 99: 27 m marked, 100 mm wide, 2.7 m2.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD marking type=lane ALIGNMENT Main station=0 to=100 offset=1.75");
        let model = built(&site, "marking-1");
        assert_eq!(model.objects.len(), 1);
        let line = &model.objects[0];
        assert_eq!(line.ifc_class, "IfcSurfaceFeature");
        assert!(is_closed_mesh(&line.mesh));
        assert_eq!(quantity_value(line, "Count"), 9.0);
        near(quantity_value(line, "Length"), 27.0, 1e-9);
        near(quantity_value(line, "GrossArea"), 2.7, 1e-9);
        assert_eq!(line.plan.len(), 9);
    }

    #[test]
    fn an_edge_line_is_one_continuous_mark() {
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD marking ALIGNMENT Main station=0 to=100 offset=-3.5");
        let model = built(&site, "marking-1");
        assert_eq!(model.objects.len(), 1);
        assert_eq!(quantity_value(&model.objects[0], "Count"), 1.0);
        near(
            quantity_value(&model.objects[0], "GrossArea"),
            100.0 * 0.15,
            1e-9,
        );
    }

    #[test]
    fn a_fence_round_a_closed_polyline_goes_all_the_way_round_with_one_post_at_its_start() {
        // A 10 m square at level 5.5: 40 m of fence; posts every 3 m at 0 ...
        // 39, and the end post would stand on the first, so 14.
        let mut site = road_site(true, 0.0);
        site.ok("PLINE 0,0 10,0 10,10 0,10 CLOSE");
        site.ok("SELECT ALL");
        site.ok("PROP SET elevation 5.5");
        let reply = site.ok("ROADOBJECT ADD fence ALONG #1");
        assert!(
            contains(&reply, "placement=line points=5 source=#1 heights=line"),
            "{reply}"
        );
        let model = built(&site, "fence-1");
        assert_eq!(model.objects.len(), 1);
        assert!(is_closed_mesh(&model.objects[0].mesh));
        near(quantity_value(&model.objects[0], "Length"), 40.0, 1e-9);
        assert_eq!(quantity_value(&model.objects[0], "Count"), 14.0);
        near(model.objects[0].mesh.bounds().min.z, 5.5, 1e-12);
    }

    #[test]
    fn a_sign_on_a_surface_stands_on_the_ground_under_it() {
        // On the plane z = 100 + 0.01 x, (50, 4) is at 100.5.
        let mut site = road_site(false, 0.0);
        put_ground(&mut site);
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 surface=ground");
        let model = built(&site, "sign-1");
        assert_eq!(model.objects.len(), 1);
        near(model.objects[0].mesh.bounds().min.z, 100.5, 1e-9);
    }

    #[test]
    fn an_alignment_with_no_profile_refuses_profile_heights() {
        let mut site = road_site(false, 0.0);
        let error = site.refused("ROADOBJECT ADD sign ALIGNMENT Main station=50");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            contains(&error.message, "no design profile"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn a_station_off_the_alignment_is_refused() {
        let mut site = road_site(true, 0.0);
        let error = site.refused("ROADOBJECT ADD sign ALIGNMENT Main station=150");
        assert!(
            contains(&error.message, "off alignment Main"),
            "{}",
            error.describe()
        );
        assert!(site.document.road_objects().objects.is_empty());
    }

    #[test]
    fn a_point_with_no_height_is_refused_rather_than_stood_at_zero() {
        let mut site = road_site(true, 0.0);
        let error = site.refused("ROADOBJECT ADD sign AT 5,5");
        assert!(
            contains(&error.message, "no height"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn an_object_whose_alignment_is_gone_is_listed_as_a_problem_and_not_drawn() {
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        site.ok("ALIGN DELETE Main");
        let model = placed_objects(&site.document);
        assert!(model.objects.is_empty());
        assert_eq!(model.problems.len(), 1);
        assert_eq!(model.problems[0].source, "object sign-1");
        assert!(contains(
            &model.problems[0].message,
            "alignment Main does not exist"
        ));
        // And the verb says so.
        assert!(contains(
            &site.ok("ROADOBJECT PROBLEMS"),
            "problem source=\"object sign-1\""
        ));
    }

    #[test]
    fn the_same_definition_makes_the_same_objects_with_the_same_ids() {
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=40 offset=4");
        site.ok("ROADOBJECT ADD guidepost ALIGNMENT Main station=0 to=40 spacing=20 offset=-4");
        let first = placed_objects(&site.document);
        let second = placed_objects(&site.document);
        assert_eq!(first.objects.len(), 4);
        assert_eq!(second.objects.len(), first.objects.len());
        for (one, other) in first.objects.iter().zip(&second.objects) {
            assert_eq!(one.id, other.id);
            assert_eq!(one.mesh.vertices, other.mesh.vertices);
            assert_eq!(one.mesh.faces, other.mesh.faces);
        }
    }

    #[test]
    fn ids_are_never_given_twice_even_after_a_delete() {
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        site.ok("ROADOBJECT DELETE sign-2");
        let reply = site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=60 offset=4");
        assert!(contains(&reply, "id=sign-3"), "{reply}");
        site.ok("ROADOBJECT DELETE ALL");
        assert!(site.document.road_objects().objects.is_empty());
        assert_eq!(site.document.road_objects().next_number, 4);
    }

    #[test]
    fn the_json_gives_back_the_same_set() {
        let mut barrier = RoadObject {
            id: String::from("barrier-1"),
            kind: RoadObjectKind::Barrier,
            name: String::from("Bridge approach, east"),
            layer: String::from("road/safety"),
            colour: Some(Color {
                r: 10,
                g: 20,
                b: 30,
                a: 255,
            }),
            material: String::from("galvanised steel"),
            facility: String::from("Main Road"),
            placement: RoadObjectPlacement {
                alignment: String::from("Main"),
                station: 12.345678901234567,
                to: Some(98.7),
                offset: -3.1,
                crossfall: -2.5,
                dz: 0.15,
                ..RoadObjectPlacement::default()
            },
            ..RoadObject::default()
        };
        barrier.parameters.insert(
            String::from("type"),
            ParameterValue::Text(String::from("thriebeam")),
        );
        barrier
            .parameters
            .insert(String::from("height"), ParameterValue::Number(0.9));
        let fence = RoadObject {
            id: String::from("fence-4"),
            kind: RoadObjectKind::Fence,
            placement: RoadObjectPlacement {
                kind: PlacementKind::Line,
                vertices: vec![
                    PlacedVertex {
                        position: Point2::new(1.5, 2.5),
                        z: Some(3.25),
                    },
                    PlacedVertex {
                        position: Point2::new(4.0, 5.0),
                        z: None,
                    },
                ],
                source: 17,
                heights: Some(HeightSource::Surface),
                surface: String::from("ground"),
                ..RoadObjectPlacement::default()
            },
            ..RoadObject::default()
        };
        let mut sign = RoadObject {
            id: String::from("sign-8"),
            kind: RoadObjectKind::Sign,
            placement: RoadObjectPlacement {
                kind: PlacementKind::Points,
                vertices: vec![vertex(0.0, 0.0, 1.0)],
                spacing: Some(5.0),
                ..RoadObjectPlacement::default()
            },
            ..RoadObject::default()
        };
        sign.parameters.insert(
            String::from("code"),
            ParameterValue::Text(String::from("R1-1 \"stop\"")),
        );
        let set = RoadObjectSet {
            objects: vec![barrier, fence, sign],
            next_number: 9,
        };

        let json = must(road_objects_to_json(&set));
        let back = match road_objects_from_json(&json) {
            Ok(back) => back,
            Err(error) => panic!("{}\n{json}", error.describe()),
        };
        assert_eq!(back, set, "{json}");
    }

    #[test]
    fn a_newer_version_is_refused_as_unsupported_and_another_format_as_not_ours() {
        let newer = road_objects_from_json(
            r#"{"format": "katana-road-objects", "version": 2, "objects": []}"#,
        )
        .expect_err("a newer version is refused");
        assert_eq!(newer.code, ErrorCode::Unsupported);
        let other = road_objects_from_json(r#"{"format": "katana-sheets"}"#)
            .expect_err("another format is refused");
        assert_eq!(other.code, ErrorCode::ParseFailure);
        // A value out of range in the file is refused as a typed one would be.
        let edited = road_objects_from_json(
            r#"{"format": "katana-road-objects", "version": 1, "objects": [{"id": "barrier-1",
           "kind": "barrier", "placement": {"kind": "alignment", "alignment": "Main",
           "station": 0, "to": 10}, "parameters": {"height": 9}}]}"#,
        )
        .expect_err("a value out of range is refused");
        assert_eq!(edited.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn a_parameter_is_read_by_its_kinds_rules() {
        assert!(parse_parameter(RoadObjectKind::Barrier, "height", "0.9").is_ok());
        let high = parse_parameter(RoadObjectKind::Barrier, "height", "5")
            .expect_err("a height of 5 is refused");
        assert_eq!(high.code, ErrorCode::InvalidArgument);
        let word = parse_parameter(RoadObjectKind::Barrier, "height", "tall")
            .expect_err("a word is refused");
        assert_eq!(word.code, ErrorCode::ParseFailure);
        let half = parse_parameter(RoadObjectKind::Barrier, "ropes", "2.5")
            .expect_err("half a rope is refused");
        assert!(contains(&half.message, "whole number"));
        let choice = must(parse_parameter(RoadObjectKind::Barrier, "type", "WBEAM"));
        assert_eq!(choice, ParameterValue::Text(String::from("wbeam")));
        let unknown = parse_parameter(RoadObjectKind::Barrier, "diameter", "1")
            .expect_err("a key the kind lacks is refused");
        assert_eq!(unknown.code, ErrorCode::NotFound);
    }

    #[test]
    fn changing_a_barriers_type_gives_the_new_types_dimensions() {
        let mut barrier = RoadObject {
            kind: RoadObjectKind::Barrier,
            ..RoadObject::default()
        };
        assert_eq!(number_of(&effective_parameters(&barrier), "height"), 0.787);
        assert!(
            !effective_parameters(&barrier).contains_key("width"),
            "a W-beam has no base width"
        );
        must(apply_key(&mut barrier, "type", "singleslope"));
        let made = effective_parameters(&barrier);
        assert_eq!(number_of(&made, "height"), 1.067);
        assert_eq!(number_of(&made, "width"), 0.610);
        assert_eq!(number_of(&made, "topwidth"), 0.203);
        // A key given again empty goes back to the default.
        must(apply_key(&mut barrier, "height", "1.2"));
        assert_eq!(number_of(&effective_parameters(&barrier), "height"), 1.2);
        must(apply_key(&mut barrier, "height", ""));
        assert_eq!(number_of(&effective_parameters(&barrier), "height"), 1.067);
    }

    #[test]
    fn an_alignment_key_on_an_object_placed_along_a_line_is_refused() {
        let mut fence = RoadObject {
            kind: RoadObjectKind::Fence,
            placement: RoadObjectPlacement {
                kind: PlacementKind::Line,
                ..RoadObjectPlacement::default()
            },
            ..RoadObject::default()
        };
        let refused =
            apply_key(&mut fence, "station", "10").expect_err("station= is an alignment's");
        assert!(contains(&refused.message, "along an alignment"));
        assert!(apply_key(&mut fence, "colour", "not a colour").is_err());
        assert!(apply_key(&mut fence, "colour", "red").is_ok());
        assert!(apply_key(&mut fence, "layer", "a//b").is_err());
    }

    // ---- totals, point objects round a closed line, caps, refusals -------------------------

    #[test]
    fn a_total_counts_a_walls_length_once_whatever_its_footing_and_a_lights_count_is_its_poles() {
        // 50 m of the default wall: the stem 50 m long, 0.25 x 2.0 x 50 = 25 m3 with
        // a face of 50 x 2 = 100 m2, and under it the footing, also 50 m long,
        // 1.2 x 0.4 x 50 = 24 m3. The wall is 50 m long, not 100, and holds
        // 25 + 24 = 49 m3 of concrete.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD wall ALIGNMENT Main station=10 to=60 offset=5");
        let wall = built(&site, "wall-1");
        assert_eq!(wall.objects.len(), 2);
        let totals = total_quantities(&wall.objects);
        near(total_value(&totals, "Length"), 50.0, 1e-9);
        near(total_value(&totals, "GrossArea"), 100.0, 1e-9);
        near(total_value(&totals, "GrossVolume"), 49.0, 1e-9);
        near(total_value(&totals, "Height"), 2.0, 1e-12);
        near(total_value(&totals, "Width"), 1.2, 1e-12);
        near(total_value(&totals, "Depth"), 0.4, 1e-12);
        // The footing says its own length to IFC all the same.
        near(quantity_value(&wall.objects[1], "Length"), 50.0, 1e-9);

        // The same wall with no footing is the same 50 m.
        site.ok("ROADOBJECT ADD wall ALIGNMENT Main station=10 to=60 offset=-5 footing=0");
        let bare = built(&site, "wall-2");
        assert_eq!(bare.objects.len(), 1);
        near(
            total_value(&total_quantities(&bare.objects), "Length"),
            50.0,
            1e-9,
        );

        // Poles at 0, 30 and 60 are three, with their three lanterns not counted
        // among them.
        site.ok("ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6");
        let lights = built(&site, "light-3");
        assert_eq!(lights.objects.len(), 6);
        let lit = total_quantities(&lights.objects);
        assert_eq!(total_value(&lit, "Count"), 3.0);
        near(total_value(&lit, "Height"), 10.0, 1e-12);
        assert_eq!(
            quantity_value(&lights.objects[1], "Count"),
            1.0,
            "a luminaire counts itself"
        );

        // Signs, guide posts and the rest are what they were: one Count each.
        site.ok("ROADOBJECT ADD guidepost ALIGNMENT Main station=0 to=40 spacing=20 offset=-4");
        assert_eq!(
            total_value(
                &total_quantities(&built(&site, "guidepost-4").objects),
                "Count"
            ),
            3.0
        );
    }

    #[test]
    fn point_objects_along_a_closed_line_stand_once_at_each_corner_and_not_again_where_it_closes() {
        // The square is 40 m round, stored with five vertices.
        let mut site = road_site(true, 0.0);
        draw_closed_square(&mut site);

        // One at each corner: four, not five.
        site.ok("ROADOBJECT ADD guidepost ALONG #1");
        let corners = built(&site, "guidepost-1");
        assert_eq!(corners.objects.len(), 4);
        let expected = [
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 10.0),
            Point2::new(0.0, 10.0),
        ];
        for (object, corner) in corners.objects.iter().zip(&expected) {
            near(centre_of(object).x, corner.x, 1e-9);
            near(centre_of(object).y, corner.y, 1e-9);
        }

        // Every 10 m: at 0, 10, 20 and 30; 40 m is the start again.
        site.ok("ROADOBJECT ADD guidepost ALONG #1 spacing=10");
        assert_eq!(built(&site, "guidepost-2").objects.len(), 4);

        // Every 15 m: 0, 15 and 30 m, and 45 is past the end - nothing to drop: at
        // (0,0), (10,5) and (0,10).
        site.ok("ROADOBJECT ADD guidepost ALONG #1 spacing=15");
        let fifteen = built(&site, "guidepost-3");
        assert_eq!(fifteen.objects.len(), 3);
        near(centre_of(&fifteen.objects[1]).x, 10.0, 1e-9);
        near(centre_of(&fifteen.objects[1]).y, 5.0, 1e-9);
        near(centre_of(&fifteen.objects[2]).x, 0.0, 1e-9);
        near(centre_of(&fifteen.objects[2]).y, 10.0, 1e-9);

        // Every 20 m: 0, 20 and 40 m, which is the start again - two poles, each a
        // pole and a lantern, so four solids and a Count of two.
        site.ok("ROADOBJECT ADD light ALONG #1 spacing=20");
        let lights = built(&site, "light-4");
        assert_eq!(lights.objects.len(), 4);
        assert_eq!(
            total_value(&total_quantities(&lights.objects), "Count"),
            2.0
        );
        for part in &lights.objects {
            assert!(is_closed_mesh(&part.mesh), "{}", part.id);
        }
    }

    #[test]
    fn a_repeated_vertex_or_a_closing_vertex_twice_makes_no_empty_solid() {
        // A node edited twice leaves two vertices in one place: the direction the
        // second stands along is the zero vector, and its post, its sign's post
        // or its pole's arm used to come out as nothing, counted and listed as
        // closed. By triangles: a guide post is a box, 12; a sign a box and a
        // plate, 12 + 12; a pole an 8-sided prism (6 + 6 cap triangles + 16
        // sides = 28) and its arm (12), 40, and a lantern 12.
        let site = road_site(true, 0.0);
        let stand = |kind: RoadObjectKind, vertices: Vec<PlacedVertex>| -> BimModel {
            let object = RoadObject {
                id: String::from("x-1"),
                kind,
                placement: RoadObjectPlacement {
                    kind: PlacementKind::Line,
                    vertices,
                    ..RoadObjectPlacement::default()
                },
                ..RoadObject::default()
            };
            build_road_object(&placement_context(&site.document), &object)
        };
        let repeated = vec![
            vertex(0.0, 0.0, 5.0),
            vertex(10.0, 0.0, 5.0),
            vertex(10.0, 0.0, 5.0),
            vertex(20.0, 0.0, 5.0),
        ];
        let posts = stand(RoadObjectKind::GuidePost, repeated.clone());
        assert!(posts.problems.is_empty());
        assert_eq!(posts.objects.len(), 3);
        for part in &posts.objects {
            assert_eq!(part.mesh.triangle_count(), 12, "{}", part.id);
            assert!(is_closed_mesh(&part.mesh), "{}", part.id);
        }
        let signs = stand(RoadObjectKind::Sign, repeated.clone());
        assert!(signs.problems.is_empty());
        assert_eq!(signs.objects.len(), 3);
        for part in &signs.objects {
            assert_eq!(part.mesh.triangle_count(), 24, "{}", part.id);
            assert!(is_closed_mesh(&part.mesh), "{}", part.id);
        }
        let poles = stand(RoadObjectKind::LightPole, repeated);
        assert!(poles.problems.is_empty());
        assert_eq!(poles.objects.len(), 6);
        for (i, part) in poles.objects.iter().enumerate() {
            assert_eq!(
                part.mesh.triangle_count(),
                if i.is_multiple_of(2) { 40 } else { 12 },
                "{}",
                part.id
            );
            assert!(is_closed_mesh(&part.mesh), "{}", part.id);
        }

        // A closed square that also repeats its first vertex at the end - a DXF
        // writer's habit on top of the closing flag - has four corners, not six.
        let doubled = vec![
            vertex(0.0, 0.0, 5.0),
            vertex(10.0, 0.0, 5.0),
            vertex(10.0, 10.0, 5.0),
            vertex(0.0, 10.0, 5.0),
            vertex(0.0, 0.0, 5.0),
            vertex(0.0, 0.0, 5.0),
        ];
        assert_eq!(stand(RoadObjectKind::GuidePost, doubled).objects.len(), 4);

        // A line of one place has one object, facing the way a point does.
        let alone = stand(
            RoadObjectKind::Sign,
            vec![vertex(3.0, 4.0, 5.0), vertex(3.0, 4.0, 5.0)],
        );
        assert_eq!(alone.objects.len(), 1);
        assert_eq!(alone.objects[0].mesh.triangle_count(), 24);
    }

    #[test]
    fn a_culvert_on_a_surface_stands_on_it_when_an_end_of_its_skew_falls_off_the_alignment() {
        // Skewed 60 degrees and 20 m long at station 5: the inlet is 10 m along
        // the axis (-sin 60, cos 60) back from (5, 0), at (13.660254, -5), and the
        // outlet 10 m forward of it, at (-3.660254, 5). That end's STATION is 5 -
        // 8.660254 = -3.66, before the alignment's start; the surface under it is
        // z = 100 + 0.01 x, which covers it. The profile has no say on a surface:
        // the ground there is 99.96339746, the inlet's 100.13660254, and each
        // invert is the ground less 0.6 of cover less 1.0 (the pipe's 0.9 and its
        // 0.1 wall).
        let mut site = road_site(false, 0.0);
        put_ground(&mut site);
        site.ok("ROADOBJECT ADD culvert ALIGNMENT Main station=5 skew=60 surface=ground");
        let set = site.document.road_objects();
        let culvert = set.find("culvert-1").expect("culvert-1");
        let ends = must(culvert_ends(&placement_context(&site.document), culvert));
        let along = (PI / 3.0).sin() * 10.0;
        near(ends.inlet.x, 5.0 + along, 1e-9);
        near(ends.inlet.y, -5.0, 1e-9);
        near(ends.inlet.z, 100.0 + 0.01 * (5.0 + along) - 1.6, 1e-9);
        near(ends.outlet.x, 5.0 - along, 1e-9);
        near(ends.outlet.y, 5.0, 1e-9);
        near(ends.outlet.z, 100.0 + 0.01 * (5.0 - along) - 1.6, 1e-9);
        assert!(built(&site, "culvert-1").problems.is_empty());
    }

    #[test]
    fn a_culvert_on_the_profile_still_refuses_an_end_off_the_alignment_naming_its_station() {
        let mut site = road_site(true, 0.0);
        let error = site.refused("ROADOBJECT ADD culvert ALIGNMENT Main station=5 skew=60");
        assert!(
            contains(&error.message, "station -3.66"),
            "{}",
            error.describe()
        );
        assert!(
            contains(&error.message, "off alignment Main"),
            "{}",
            error.describe()
        );
    }

    #[test]
    fn an_arc_carries_its_end_heights_along_its_chords() {
        // A semicircle of radius 10: ARC takes its start, a point on it and its end.
        let mut site = road_site(true, 0.0);
        site.ok("ARC 10,0 0,10 -10,0");
        site.ok("SELECT ALL");
        // No height: refused, as any line with none is.
        assert!(contains(
            &site.refused("ROADOBJECT ADD fence ALONG #1").message,
            "no height"
        ));
        // One elevation: both ends at 5, so a level fence: the posts stand on 5 and
        // are 1.2 tall.
        site.ok("PROP SET elevation 5");
        site.ok("ROADOBJECT ADD fence ALONG #1");
        let level = built(&site, "fence-1");
        assert_eq!(level.objects.len(), 1);
        near(level.objects[0].mesh.bounds().min.z, 5.0, 1e-9);
        near(level.objects[0].mesh.bounds().max.z, 6.2, 1e-9);
        // Two: 5 at the start and 7 at the end, rising evenly with the sweep. The
        // first post is on 5 and the last on 7, 1.2 above it, 8.2.
        site.ok("PROP SET elevations \"5 7\"");
        site.ok("ROADOBJECT ADD fence ALONG #1");
        let graded = built(&site, "fence-2");
        assert_eq!(graded.objects.len(), 1);
        near(graded.objects[0].mesh.bounds().min.z, 5.0, 1e-9);
        near(graded.objects[0].mesh.bounds().max.z, 8.2, 1e-9);
    }

    #[test]
    fn a_run_past_the_cap_is_refused_before_anything_is_made_and_one_at_it_is_made() {
        // 99.99 m every 0.01 m is 9999 spacings and 10000 posts: the most a
        // definition takes. The whole 100 m is 10001.
        let mut site = road_site(true, 0.0);
        let at_the_cap =
            site.ok("ROADOBJECT ADD guidepost ALIGNMENT Main station=0 to=99.99 spacing=0.01");
        assert!(
            contains(&at_the_cap, "objects=10000"),
            "{}",
            head(&at_the_cap, 300)
        );
        let past =
            site.refused("ROADOBJECT ADD guidepost ALIGNMENT Main station=0 to=100 spacing=0.01");
        assert!(
            contains(&past.message, "more than the 10000"),
            "{}",
            past.describe()
        );
        // A spacing that would be billions is refused as quickly.
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD sign ALIGNMENT Main station=0 to=100 spacing=0.0000001")
                .message,
            "more than the 10000"
        ));
        // Only the run at the cap was kept.
        assert_eq!(site.document.road_objects().objects.len(), 1);
    }

    #[test]
    fn a_dash_shorter_than_a_millimetre_or_more_marks_than_the_cap_are_refused() {
        let mut site = road_site(true, 0.0);
        let tiny = site.refused(
            "ROADOBJECT ADD marking type=lane dash=0.0005 gap=1 ALIGNMENT Main station=0 to=100 \
             offset=1",
        );
        assert!(
            contains(&tiny.message, "shorter than the 0.001 m"),
            "{}",
            tiny.describe()
        );
        // 1 mm dash and 1 mm gap over 100 m is 50000 marks.
        let many = site.refused(
            "ROADOBJECT ADD marking type=lane dash=0.001 gap=0.001 ALIGNMENT Main station=0 \
             to=100 offset=1",
        );
        assert!(
            contains(&many.message, "more than the 10000"),
            "{}",
            many.describe()
        );
        assert!(site.document.road_objects().objects.is_empty());
    }

    #[test]
    fn a_lines_last_mark_too_short_to_have_a_solid_is_not_counted_or_measured() {
        // 12.0004 m of the 3 m and 9 m pattern: a mark at 0 to 3, then the next at
        // 12 would run 0.4 mm - less than the millimetre a sweep keeps - and made
        // nothing while it was counted and measured. One mark, 3 m, 0.3 m2.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD marking type=lane ALIGNMENT Main station=0 to=12.0004 offset=1.75");
        let model = built(&site, "marking-1");
        assert_eq!(model.objects.len(), 1);
        let line = &model.objects[0];
        assert!(is_closed_mesh(&line.mesh));
        assert_eq!(quantity_value(line, "Count"), 1.0);
        near(quantity_value(line, "Length"), 3.0, 1e-9);
        near(quantity_value(line, "GrossArea"), 0.3, 1e-9);
        assert_eq!(line.plan.len(), 1);
    }

    #[test]
    fn spacing_and_crossfall_where_they_apply_to_nothing_are_refused_not_echoed() {
        let mut site = road_site(true, 0.0);
        // A barrier's and a fence's posts have postspacing=; a wall, nothing.
        let barrier =
            site.refused("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 spacing=5");
        assert!(
            contains(&barrier.message, "postspacing"),
            "{}",
            barrier.describe()
        );
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD wall ALIGNMENT Main station=0 to=20 spacing=5")
                .message,
            "nothing to space"
        ));
        // At points one stands at each point.
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD sign AT 5,5,1 spacing=5")
                .message,
            "one stands at each point"
        ));
        // No profile to carry a crossfall out along a line or at points.
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD sign AT 5,5,1 crossfall=-3")
                .message,
            "no profile to carry"
        ));
        // Nor a surface.
        put_ground(&mut site);
        assert!(contains(
            &site
                .refused(
                    "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 surface=ground \
                     crossfall=-3"
                )
                .message,
            "stands on a surface"
        ));
        assert!(site.document.road_objects().objects.is_empty());

        // On the profile it is carried, and SET cannot then put the object on a
        // surface without letting the crossfall go.
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 crossfall=-3");
        assert!(contains(
            &site.refused("ROADOBJECT SET sign-1 surface=ground").message,
            "stands on a surface"
        ));
        site.ok("ROADOBJECT SET sign-1 crossfall= surface=ground");
        assert_eq!(
            site.document
                .road_objects()
                .find("sign-1")
                .expect("sign-1")
                .placement
                .crossfall,
            0.0
        );
        // A zero crossfall is no crossfall: accepted anywhere.
        site.ok("ROADOBJECT ADD sign AT 5,5,1 crossfall=0");
    }

    #[test]
    fn offset_crossfall_and_dz_given_empty_go_back_to_zero_and_station_and_alignment_need_a_value()
    {
        let mut sign = RoadObject {
            kind: RoadObjectKind::Sign,
            placement: RoadObjectPlacement {
                alignment: String::from("Main"),
                station: 50.0,
                offset: 4.0,
                crossfall: -3.0,
                dz: 0.5,
                ..RoadObjectPlacement::default()
            },
            ..RoadObject::default()
        };
        must(apply_key(&mut sign, "offset", ""));
        must(apply_key(&mut sign, "crossfall", ""));
        must(apply_key(&mut sign, "dz", ""));
        assert_eq!(sign.placement.offset, 0.0);
        assert_eq!(sign.placement.crossfall, 0.0);
        assert_eq!(sign.placement.dz, 0.0);
        // Where along the alignment is the point of the placement: no default.
        let station = apply_key(&mut sign, "station", "").expect_err("station= needs a value");
        assert_eq!(station.code, ErrorCode::InvalidArgument);
        assert!(contains(&station.message, "needs a number"));
        assert_eq!(sign.placement.station, 50.0);
        assert!(apply_key(&mut sign, "alignment", "").is_err());
        assert_eq!(sign.placement.alignment, "Main");
    }

    #[test]
    fn ids_are_never_given_twice_even_after_an_undo_or_a_redo() {
        // barrier-1, barrier-2 (a copy), then the copy undone: the next number is
        // 3, because 2 was given, not 2 again.
        let mut site = road_site(true, 0.0);
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        site.ok("ROADOBJECT COPY barrier-1 station=30");
        assert_eq!(site.document.road_objects().next_number, 3);
        site.ok("UNDO");
        assert_eq!(site.document.road_objects().objects.len(), 1);
        assert_eq!(site.document.road_objects().next_number, 3);
        assert!(contains(
            &site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4"),
            "id=sign-3"
        ));
        // Everything undone: the first object ever placed too. 4 is next.
        site.ok("UNDO");
        site.ok("UNDO");
        assert!(site.document.road_objects().objects.is_empty());
        assert_eq!(site.document.road_objects().next_number, 4);
        assert!(contains(
            &site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=60 offset=4"),
            "id=sign-4"
        ));
        // Undone and redone, the sign is sign-4 again and 5 is next: a redo does
        // not take the counter back to what the step began with.
        site.ok("UNDO");
        site.ok("REDO");
        assert!(site.document.road_objects().find("sign-4").is_some());
        assert_eq!(site.document.road_objects().next_number, 5);
    }

    // ---- the refusals that nothing builds an input for any more ----------------------------
    //
    // build_road_object refuses a part with no faces, and an edit refuses a part
    // that is not a closed solid: two guards that every maker now satisfies, so no
    // object reaches them. A mutation check once disabled each of them and all the
    // road tests still passed; these hand each guard the input it exists for.

    #[test]
    fn objects_made_of_nothing_or_of_a_part_with_no_faces_are_no_object_and_say_why() {
        let bare = BimObject {
            id: String::from("object/barrier-1"),
            ..BimObject::default()
        };
        let mut post = BimObject {
            id: String::from("object/barrier-1/post"),
            ..BimObject::default()
        };
        add_box(
            &mut post.mesh,
            Point3::new(0.0, 0.0, 0.0),
            Vec2::new(1.0, 0.0),
            1.0,
            1.0,
            1.0,
        );
        assert!(!post.mesh.faces.is_empty());

        let none = || String::from("none");
        assert_eq!(
            shape_problem(&[]).unwrap_or_else(none),
            "its placement gives nowhere to stand it"
        );
        let shapeless = [post.clone(), bare.clone()];
        assert_eq!(
            shape_problem(&shapeless).unwrap_or_else(none),
            "object/barrier-1 could not be made into a solid: its placement or its dimensions \
             give it no shape"
        );
        // The first part with no faces is the one named, whichever place it is in.
        let shapeless_first = [bare, post.clone()];
        assert_eq!(
            shape_problem(&shapeless_first).unwrap_or_else(none),
            "object/barrier-1 could not be made into a solid: its placement or its dimensions \
             give it no shape"
        );
        // Solids with faces are an object; one is enough.
        assert!(shape_problem(std::slice::from_ref(&post)).is_none());
        assert!(shape_problem(&[post.clone(), post]).is_none());
    }

    #[test]
    fn a_built_object_with_an_open_part_or_a_problem_is_refused_before_it_is_stored() {
        let mut object = RoadObject {
            id: String::from("barrier-1"),
            kind: RoadObjectKind::Barrier,
            placement: RoadObjectPlacement {
                kind: PlacementKind::Line,
                source: 12,
                ..RoadObjectPlacement::default()
            },
            ..RoadObject::default()
        };

        // One triangle: every edge used once, with nothing to close it.
        let open = BimObject {
            id: String::from("object/barrier-1"),
            mesh: TriangleMesh {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                faces: vec![[0, 1, 2]],
            },
            ..BimObject::default()
        };
        assert!(!is_closed_mesh(&open.mesh));
        let built = BimModel {
            objects: vec![open.clone()],
            ..BimModel::default()
        };
        let refused = check_built(&object, &built).expect_err("an open part is refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert_eq!(
            refused.message,
            "object/barrier-1 is not a closed solid; its placement or its dimensions make a \
             shape that does not close"
        );
        assert_eq!(refused.context, "barrier-1");

        // A closed box beside it does not excuse it: every part is checked.
        let mut closed_box = BimObject {
            id: String::from("object/barrier-1/post"),
            ..BimObject::default()
        };
        add_box(
            &mut closed_box.mesh,
            Point3::new(0.0, 0.0, 0.0),
            Vec2::new(1.0, 0.0),
            1.0,
            1.0,
            1.0,
        );
        assert!(is_closed_mesh(&closed_box.mesh));
        let mixed = BimModel {
            objects: vec![closed_box.clone(), open],
            ..BimModel::default()
        };
        let still_refused = check_built(&object, &mixed).expect_err("the open part is refused");
        assert!(contains(
            &still_refused.message,
            "object/barrier-1 is not a closed solid"
        ));

        let closed = BimModel {
            objects: vec![closed_box],
            ..BimModel::default()
        };
        assert!(check_built(&object, &closed).is_ok());

        // A problem the model was left with is refused first, naming the line the
        // object was placed along.
        let failed = BimModel {
            problems: vec![BimProblem {
                source: String::from("object barrier-1"),
                message: String::from("station 150 is off alignment Main (0 to 100)"),
            }],
            ..BimModel::default()
        };
        let problem = check_built(&object, &failed).expect_err("the problem is refused");
        assert_eq!(
            problem.message,
            "station 150 is off alignment Main (0 to 100)"
        );
        assert_eq!(problem.context, "barrier-1 (#12)");
        object.placement.source = INVALID_ENTITY_ID;
        assert_eq!(
            check_built(&object, &failed)
                .expect_err("the problem is refused")
                .context,
            "barrier-1"
        );
    }
}
