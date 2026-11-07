//! Twin of tests/ifc/test_road.cpp. The road model in IFC 4.3 (road.rs,
//! docs/ifc.md "Road model"): a synthetic model - two pavement courses, a
//! kerb, a fill and a cut batter, a line marking, a sign and a noise wall,
//! each a box whose volume is worked out by hand - exported, and the file
//! read back instance by instance. Nothing here waits on the corridor and
//! road-object builders: the input is the plain data katana_app makes of
//! them (RoadModelInput).

pub mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use katana_entity::entity::{Color, Entity, Geometry, property_to_string};
use katana_geometry::mesh::TriangleMesh;
use katana_geometry::{Point2, Polyline2};
use katana_ifc::{
    ClassificationRule, ExportInput, ExportOptions, IfcClass, IfcExport, ImportOptions,
    RoadElement, RoadModelInput, RoadQuantity, SurfaceInput, format_export_reply, guid_for,
    parse_classification_rules, read_ifc, write_ifc, write_ifc_file,
};
use katana_math::Vec3;
use katana_terrain::TinSurface;
use support::*;

// ---- the input ------------------------------------------------------------------------

/// The box [x0, x1] x [y0, y1] x [z0, z1] as twelve triangles wound outwards:
/// each face's (b - a) x (c - a) points away from the box (bottom -z, top +z,
/// front -y, back +y, left -x, right +x - each worked through by hand), and
/// each edge is used once in each direction.
fn box_mesh(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> TriangleMesh {
    TriangleMesh {
        vertices: vec![
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x0, y1, z0),
            Vec3::new(x0, y0, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x0, y1, z1),
        ],
        faces: vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [3, 7, 6],
            [3, 6, 2],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ],
    }
}

fn unit_box() -> TriangleMesh {
    box_mesh(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
}

fn class(entity: &str, predefined_type: &str, object_type: &str) -> IfcClass {
    IfcClass::new(entity, predefined_type, object_type)
}

fn element(
    id: &str,
    kind: &str,
    ifc_class: IfcClass,
    facility: &str,
    part: &str,
    mesh: TriangleMesh,
) -> RoadElement {
    RoadElement {
        id: String::from(id),
        name: String::from(id),
        kind: String::from(kind),
        ifc_class,
        facility: String::from(facility),
        part: String::from(part),
        mesh,
        layer: format!("Road/{kind}"),
        ..RoadElement::default()
    }
}

fn quantity(name: &str, value: f64, unit: &str) -> RoadQuantity {
    RoadQuantity::new(name, value, unit)
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

const WEARING: &str = "corridor/Main/course/AC14/left/1";
const BASE: &str = "corridor/Main/course/BASE/left/1";
const KERB: &str = "corridor/Main/kerb/left/1";
const FILL: &str = "corridor/Main/batter/fill/left/1";
const CUT: &str = "corridor/Main/batter/cut/right/1";
const MARKING: &str = "object/marking-1";
const SIGN: &str = "object/sign-1";
const WALL: &str = "object/wall-1";

/// One corridor, Main, 100 m long from x = 1000: a 3.5 m lane of a 50 mm
/// wearing course (100 x 3.5 x 0.05 = 17.5 m3) on a 200 mm base (70 m3), a
/// kerb in the road itself, a fill batter and a cut batter at the roadside, a
/// line marking on the lane, a noise wall; and a sign of the site.
fn road_model() -> RoadModelInput {
    let mut road = RoadModelInput::default();
    let mut wearing = element(
        WEARING,
        "pavement",
        class("IfcCourse", "PAVEMENT", ""),
        "Main",
        "CARRIAGEWAY",
        box_mesh(1000.0, 2000.0, 9.95, 1100.0, 2003.5, 10.0),
    );
    wearing.material = String::from("Asphalt AC14");
    wearing.colour = Color::new(40, 40, 40, 255);
    wearing.quantities = vec![
        quantity("Length", 100.0, "m"),
        quantity("Width", 3.5, "m"),
        quantity("Thickness", 0.05, "m"),
        quantity("GrossVolume", 17.5, "m3"),
        quantity("Area", 350.0, "m2"),
    ];
    wearing.properties = vec![
        (String::from("Component"), String::from("lane")),
        (String::from("Stations"), String::from("0 to 100")),
    ];
    road.elements.push(wearing);

    let mut base = element(
        BASE,
        "pavement",
        class("IfcCourse", "PAVEMENT", ""),
        "Main",
        "CARRIAGEWAY",
        box_mesh(1000.0, 2000.0, 9.75, 1100.0, 2003.5, 9.95),
    );
    base.material = String::from("Crushed rock");
    base.quantities = vec![quantity("GrossVolume", 70.0, "m3")];
    road.elements.push(base);

    let mut kerb = element(
        KERB,
        "kerb",
        class("IfcKerb", "NOTDEFINED", ""),
        "Main",
        "",
        box_mesh(1000.0, 2003.5, 9.85, 1100.0, 2003.65, 10.15),
    );
    kerb.material = String::from("Concrete");
    kerb.quantities = vec![quantity("Length", 100.0, "m"), quantity("Weight", 1.5, "t")];
    road.elements.push(kerb);

    let mut fill = element(
        FILL,
        "batter",
        class("IfcEarthworksFill", "SLOPEFILL", ""),
        "Main",
        "ROADSIDE",
        box_mesh(1000.0, 2003.65, 9.0, 1100.0, 2006.0, 9.75),
    );
    fill.quantities = vec![quantity("CompactedVolume", 12.5, "m3")];
    road.elements.push(fill);

    let mut cut = element(
        CUT,
        "batter",
        class("IfcEarthworksCut", "CUT", ""),
        "Main",
        "ROADSIDE",
        box_mesh(1000.0, 1997.0, 9.75, 1100.0, 2000.0, 10.5),
    );
    cut.ground = String::from("ground");
    cut.material = String::from("Clay"); // what it removes: a void takes no material
    cut.quantities = vec![quantity("UndisturbedVolume", 20.0, "m3")];
    road.elements.push(cut);

    road.elements.push(element(
        MARKING,
        "marking",
        class("IfcSurfaceFeature", "LINEMARKING", ""),
        "Main",
        "CARRIAGEWAY",
        box_mesh(1000.0, 2001.7, 10.0, 1100.0, 2001.8, 10.003),
    ));

    let mut sign = element(
        SIGN,
        "sign",
        class("IfcSign", "PICTORAL", ""),
        "",
        "",
        box_mesh(1050.0, 2008.0, 10.0, 1050.6, 2008.05, 12.1),
    );
    sign.quantities = vec![
        quantity("Height", 0.9, "m"),
        quantity("Width", 0.6, "m"),
        quantity("Count", 1.0, ""),
    ];
    road.elements.push(sign);

    road.elements.push(element(
        WALL,
        "wall",
        class("IfcWall", "USERDEFINED", "NOISEBARRIER"),
        "Main",
        "ROADSIDE",
        box_mesh(1000.0, 2009.0, 10.0, 1100.0, 2009.2, 13.0),
    ));

    road.problems = vec![String::from("corridor Main: the profile ends at 100")];
    road
}

fn ground() -> TinSurface {
    must(TinSurface::create(
        vec![
            Vec3::new(990.0, 1990.0, 10.0),
            Vec3::new(1110.0, 1990.0, 10.0),
            Vec3::new(990.0, 2020.0, 10.0),
            Vec3::new(1110.0, 2020.0, 10.0),
        ],
        vec![[0, 1, 3], [0, 3, 2]],
        Vec::new(),
    ))
}

fn options() -> ExportOptions {
    ExportOptions {
        project_name: String::from("Test"),
        ..ExportOptions::default()
    }
}

fn exported_with(input: &ExportInput<'_>, with: &ExportOptions) -> IfcExport {
    must(write_ifc(input, with))
}

fn exported(input: &ExportInput<'_>) -> IfcExport {
    exported_with(input, &options())
}

fn road_input(surface: Option<&TinSurface>) -> ExportInput<'_> {
    let mut input = ExportInput {
        road: road_model(),
        ..ExportInput::default()
    };
    if let Some(surface) = surface {
        input.surfaces.push(SurfaceInput {
            name: String::from("ground"),
            surface: Some(surface),
        });
    }
    input
}

/// An input of the road model's elements alone.
fn elements_input(elements: Vec<RoadElement>) -> ExportInput<'static> {
    ExportInput {
        road: RoadModelInput {
            elements,
            problems: Vec::new(),
        },
        ..ExportInput::default()
    }
}

// ---- the file, read back ------------------------------------------------------------

/// The top-level arguments of an instance's attribute list, as written.
fn arguments_of(list: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut quoted = false;
    for c in list.chars() {
        if c == '\'' {
            quoted = !quoted;
        } else if !quoted && c == '(' {
            depth += 1;
        } else if !quoted && c == ')' {
            depth -= 1;
        } else if !quoted && depth == 0 && c == ',' {
            out.push(std::mem::take(&mut current));
            continue;
        }
        current.push(c);
    }
    out.push(current);
    out
}

/// What is inside a list's parentheses: the C++'s
/// `substr(1, list.size() - 2)`, which for "$" (one character) is the empty
/// rest.
fn inner(list: &str) -> &str {
    if list.len() >= 2 {
        &list[1..list.len() - 1]
    } else {
        &list[list.len().min(1)..]
    }
}

struct Instance {
    class: String,
    args: Vec<String>,
    line: String,
}

type Instances = BTreeMap<i32, Instance>;

fn instances_in(text: &str) -> Instances {
    let mut out = Instances::new();
    let mut at = 0;
    while let Some(found) = text[at..].find("\n#") {
        let start = at + found;
        let end = start + 1 + text[start + 1..].find(";\n").expect("an instance ends");
        let line = &text[start + 1..end];
        at = end;
        let equals = line.find('=').expect("an instance is named");
        let open = equals + line[equals..].find('(').expect("an instance has arguments");
        let instance = Instance {
            class: String::from(&line[equals + 1..open]),
            args: arguments_of(&line[open + 1..line.len() - 1]),
            line: String::from(line),
        };
        let id: i32 = line[1..equals].parse().expect("an instance number");
        out.entry(id).or_insert(instance);
    }
    out
}

/// `std::stoi` of what follows '#': its leading digits.
fn ref_of(argument: &str) -> i32 {
    let Some(rest) = argument.strip_prefix('#') else {
        return 0;
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().expect("a reference is a number")
}

fn refs_of(list: &str) -> Vec<i32> {
    arguments_of(inner(list))
        .iter()
        .map(|item| ref_of(item))
        .collect()
}

fn of_type(all: &Instances, class: &str) -> Vec<i32> {
    all.iter()
        .filter(|(_, instance)| instance.class == class)
        .map(|(&id, _)| id)
        .collect()
}

/// The element whose Tag (its eighth attribute) is `tag`; 0 when none is.
fn by_tag(all: &Instances, tag: &str) -> i32 {
    let quoted = format!("'{tag}'");
    all.iter()
        .find(|(_, instance)| instance.args.len() >= 8 && instance.args[7] == quoted)
        .map_or(0, |(&id, _)| id)
}

/// The spatial element `element` is contained in; 0 when none.
fn container_of(all: &Instances, element: i32) -> i32 {
    for rel in of_type(all, "IFCRELCONTAINEDINSPATIALSTRUCTURE") {
        if refs_of(&all[&rel].args[4]).contains(&element) {
            return ref_of(&all[&rel].args[5]);
        }
    }
    0
}

/// What `relating` aggregates, in the relationship's order.
fn parts_of(all: &Instances, relating: i32) -> Vec<i32> {
    for rel in of_type(all, "IFCRELAGGREGATES") {
        if ref_of(&all[&rel].args[4]) == relating {
            return refs_of(&all[&rel].args[5]);
        }
    }
    Vec::new()
}

/// The ObjectPlacement of a product.
fn placement_of(all: &Instances, product: i32) -> i32 {
    ref_of(&all[&product].args[5])
}

/// A placement's PlacementRelTo.
fn relative_to(all: &Instances, placement: i32) -> i32 {
    ref_of(&all[&placement].args[0])
}

/// The property or quantity sets defining `object`, by name.
fn sets_of(all: &Instances, object: i32) -> BTreeMap<String, i32> {
    let mut out = BTreeMap::new();
    for rel in of_type(all, "IFCRELDEFINESBYPROPERTIES") {
        if refs_of(&all[&rel].args[4]).contains(&object) {
            let set = ref_of(&all[&rel].args[5]);
            out.entry(all[&set].args[2].clone()).or_insert(set);
        }
    }
    out
}

/// The quantities of an IfcElementQuantity as written:
/// "IFCQUANTITYLENGTH('Length',$,$,100.,$)".
fn quantities_of(all: &Instances, set: i32) -> BTreeSet<String> {
    refs_of(&all[&set].args[5])
        .into_iter()
        .map(|quantity| {
            let line = &all[&quantity].line;
            String::from(&line[line.find('=').unwrap() + 1..])
        })
        .collect()
}

fn set_of(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| String::from(*item)).collect()
}

/// The face set of a product's Body, and its points as written.
#[derive(Default)]
struct FaceSet {
    closed: bool,
    points: Vec<[f64; 3]>,
    triangles: Vec<[i32; 3]>,
}

fn reals_of(list: &str) -> Vec<f64> {
    arguments_of(inner(list))
        .iter()
        .map(|item| {
            item.parse()
                .unwrap_or_else(|error| panic!("{item}: {error}"))
        })
        .collect()
}

fn body_of(all: &Instances, product: i32) -> FaceSet {
    let mut out = FaceSet::default();
    let shape = ref_of(&all[&product].args[6]);
    for representation in refs_of(&all[&shape].args[2]) {
        if all[&representation].args[1] != "'Body'" {
            continue;
        }
        let faces = &all[&refs_of(&all[&representation].args[3])[0]];
        assert_eq!(faces.class, "IFCTRIANGULATEDFACESET");
        out.closed = faces.args[2] == ".T.";
        let points = &all[&ref_of(&faces.args[0])];
        for point in arguments_of(inner(&points.args[0])) {
            let xyz = reals_of(&point);
            out.points.push([xyz[0], xyz[1], xyz[2]]);
        }
        for triangle in arguments_of(inner(&faces.args[3])) {
            let abc = reals_of(&triangle);
            out.triangles
                .push([abc[0] as i32, abc[1] as i32, abc[2] as i32]);
        }
    }
    out
}

/// The volume a face set encloses, by the divergence theorem: the sum over
/// its triangles of a . (b x c) / 6, positive when they are wound outwards.
fn volume_of(faces: &FaceSet) -> f64 {
    let mut volume = 0.0;
    for t in &faces.triangles {
        let a = faces.points[(t[0] - 1) as usize];
        let b = faces.points[(t[1] - 1) as usize];
        let c = faces.points[(t[2] - 1) as usize];
        volume += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    volume / 6.0
}

fn warned(out: &IfcExport, words: &str) -> bool {
    out.warnings.iter().any(|warning| warning.contains(words))
}

fn metadata(entity: &Entity, key: &str) -> String {
    entity
        .metadata
        .get(key)
        .map_or_else(|| String::from("<absent>"), property_to_string)
}

mod ifc_export_road {
    use super::*;

    // ---- the spatial structure -----------------------------------------------------

    // IfcSite -> IfcRoad (ELEMENT) -> IfcRoadPart (PARTIAL), each by
    // IfcRelAggregates, the parts in the order the model first names them:
    // CARRIAGEWAY, then ROADSIDE.
    #[test]
    fn each_facility_is_an_ifc_road_in_the_site_and_each_part_an_ifc_road_part_in_its_road() {
        let out = exported(&road_input(None));
        let all = instances_in(&out.text);
        assert_eq!(out.roads, 1);
        assert_eq!(out.road_parts, 2);
        assert_eq!(out.road_elements, 8);
        assert_eq!(out.road_elements_skipped, 0);
        assert_eq!(of_type(&all, "IFCROAD").len(), 1);
        let road = of_type(&all, "IFCROAD")[0];
        assert_eq!(all[&road].args[2], "'Main'");
        assert_eq!(all[&road].args[8], ".ELEMENT.");
        assert_eq!(all[&road].args.len(), 10); // IfcRoad has ten attributes (7.9.3.3)

        let site = of_type(&all, "IFCSITE")[0];
        assert_eq!(parts_of(&all, site), [road]);
        let parts = parts_of(&all, road);
        assert_eq!(parts.len(), 2);
        for part in &parts {
            assert_eq!(all[part].class, "IFCROADPART");
            assert_eq!(all[part].args.len(), 11); // IfcRoadPart has eleven (7.9.3.4)
            assert_eq!(all[part].args[8], ".PARTIAL.");
            // Both are parts of the cross-section: "lateral" in the
            // enumeration's own definitions (7.9.2.2).
            assert_eq!(all[part].args[9], ".LATERAL.");
        }
        assert_eq!(all[&parts[0]].args[10], ".CARRIAGEWAY.");
        assert_eq!(all[&parts[0]].args[2], "'Main CARRIAGEWAY'");
        assert_eq!(all[&parts[1]].args[10], ".ROADSIDE.");
        assert_eq!(out.classes["IfcRoad"], 1);
        assert_eq!(out.classes["IfcRoadPart"], 2);
    }

    // A facility is a corridor's name and Katana finds a corridor ignoring
    // case (CorridorSet::find), so facility=main beside the corridor Main is
    // on Main: one IfcRoad, not two, and a line marking of it finds the
    // course of its road under it. The same for a part ("roadside" and
    // "ROADSIDE" are one part). The road and the parts are written under the
    // first spelling the model has: Main, and ROADSIDE from the fill, which
    // comes before the rail's "roadside". By hand: 4 elements (a course, a
    // fill, a marking, a rail) in 1 road and 2 parts (CARRIAGEWAY, ROADSIDE);
    // the marking, a feature, is contained nowhere and adheres to the course
    // (its first face's centre is in the course's plan: the marking box is
    // x 1..9, y 1..1.1, the course 0..10 x 0..3.5); the rail is in the
    // ROADSIDE part. A sign with a part but no facility is the site's and
    // must not throw.
    #[test]
    fn a_facility_and_part_match_ignoring_case_as_a_corridors_name_does() {
        let input = elements_input(vec![
            element(
                "course",
                "pavement",
                class("IfcCourse", "PAVEMENT", ""),
                "Main",
                "CARRIAGEWAY",
                box_mesh(0.0, 0.0, 0.0, 10.0, 3.5, 0.05),
            ),
            element(
                "fill",
                "batter",
                class("IfcEarthworksFill", "SLOPEFILL", ""),
                "Main",
                "ROADSIDE",
                box_mesh(0.0, 5.1, 0.0, 10.0, 6.0, 0.5),
            ),
            element(
                "mark",
                "marking",
                class("IfcSurfaceFeature", "LINEMARKING", ""),
                "main",
                "carriageway",
                box_mesh(1.0, 1.0, 0.05, 9.0, 1.1, 0.053),
            ),
            element(
                "rail",
                "barrier",
                class("IfcRailing", "GUARDRAIL", ""),
                "MAIN",
                "roadside",
                box_mesh(0.0, 5.0, 0.0, 10.0, 5.1, 1.0),
            ),
            element(
                "sign",
                "sign",
                class("IfcSign", "PICTORAL", ""),
                "",
                "ROADSIDE",
                box_mesh(20.0, 0.0, 0.0, 20.5, 0.05, 2.0),
            ),
        ]);
        let out = exported(&input);
        let all = instances_in(&out.text);
        assert_eq!(out.road_elements, 5);
        assert_eq!(
            out.road_elements_skipped, 0,
            "the marking has a course under it, ignoring case"
        );
        assert_eq!(out.roads, 1);
        assert_eq!(out.road_parts, 2);
        assert_eq!(of_type(&all, "IFCROAD").len(), 1);
        let road = of_type(&all, "IFCROAD")[0];
        assert_eq!(all[&road].args[2], "'Main'");
        let mut parts = BTreeMap::new();
        for part in of_type(&all, "IFCROADPART") {
            parts.entry(all[&part].args[2].clone()).or_insert(part);
        }
        assert_eq!(parts.len(), 2);
        assert!(parts.contains_key("'Main CARRIAGEWAY'") && parts.contains_key("'Main ROADSIDE'"));
        assert_eq!(
            container_of(&all, by_tag(&all, "rail")),
            parts["'Main ROADSIDE'"]
        );
        assert_eq!(
            container_of(&all, by_tag(&all, "fill")),
            parts["'Main ROADSIDE'"]
        );
        assert_eq!(
            container_of(&all, by_tag(&all, "sign")),
            of_type(&all, "IFCSITE")[0]
        );
        // The marking adheres to the course, and is contained in nothing.
        assert_eq!(of_type(&all, "IFCRELADHERESTOELEMENT").len(), 1);
        assert_eq!(
            ref_of(&all[&of_type(&all, "IFCRELADHERESTOELEMENT")[0]].args[4]),
            by_tag(&all, "course")
        );
        assert_eq!(container_of(&all, by_tag(&all, "mark")), 0);
        // Not a second road in the account either: one source, "road Main".
        for row in &out.tally {
            assert_ne!(row.source, "road main");
            assert_ne!(row.source, "road MAIN");
        }
    }

    // Two facilities that differ by more than case stay two roads.
    #[test]
    fn facilities_that_differ_by_more_than_case_are_two_roads() {
        let input = elements_input(
            ["Main", "MAINS", "main"]
                .into_iter()
                .map(|facility| {
                    element(
                        &format!("k-{facility}"),
                        "kerb",
                        class("IfcKerb", "NOTDEFINED", ""),
                        facility,
                        "",
                        unit_box(),
                    )
                })
                .collect(),
        );
        let out = exported(&input);
        assert_eq!(out.roads, 2); // Main (twice, as "main") and MAINS
        assert_eq!(out.classes["IfcKerb"], 3);
    }

    // Every product the report counts is in the account once (the same
    // property ifc_export_tally::every_product_the_report_counts_is_accounted_for_once
    // holds for a drawing): the road and each part of it too, so that
    // File > Export IFC's table, which is filled from the account, lists them.
    // By hand, road_input() is one corridor, Main, with two parts:
    // CARRIAGEWAY (the courses, the marking's host) and ROADSIDE (the fill and
    // the wall); the kerb is in the road itself.
    #[test]
    fn every_road_and_part_the_report_counts_is_in_the_account_once() {
        let out = exported(&road_input(None));
        let mut tallied = BTreeMap::new();
        for row in &out.tally {
            if !row.entity.is_empty() {
                *tallied.entry(row.entity.clone()).or_insert(0) += row.count;
            }
        }
        assert_eq!(tallied, out.classes);
        assert_eq!(tallied["IfcRoad"], 1);
        assert_eq!(tallied["IfcRoadPart"], 2);
        let mut parts = BTreeMap::new();
        for row in &out.tally {
            if row.entity == "IfcRoad" {
                assert_eq!(row.source, "road Main");
            }
            if row.entity == "IfcRoadPart" {
                assert_eq!(row.source, "road Main");
                assert_eq!(row.why, "a part of road Main");
                *parts.entry(row.predefined_type.clone()).or_insert(0) += row.count;
            }
        }
        assert_eq!(
            parts,
            BTreeMap::from([
                (String::from("CARRIAGEWAY"), 1),
                (String::from("ROADSIDE"), 1)
            ])
        );
    }

    // Each element in its part, in its road when it names no part, in the
    // site when it names no road; each placed relative to its container, the
    // part to its road and the road to the site (IfcLocalPlacement,
    // 8.7.3.14).
    #[test]
    fn every_element_is_contained_in_its_part_its_road_or_the_site_and_placed_relative_to_it() {
        let out = exported(&road_input(None));
        let all = instances_in(&out.text);
        let site = of_type(&all, "IFCSITE")[0];
        let road = of_type(&all, "IFCROAD")[0];
        let parts = parts_of(&all, road);
        assert_eq!(parts.len(), 2);
        let carriageway = parts[0];
        let roadside = parts[1];

        let wearing = by_tag(&all, WEARING);
        let kerb = by_tag(&all, KERB);
        let fill = by_tag(&all, FILL);
        let sign = by_tag(&all, SIGN);
        let wall = by_tag(&all, WALL);
        assert!(wearing != 0 && kerb != 0 && fill != 0 && sign != 0 && wall != 0);
        assert_eq!(all[&wearing].class, "IFCCOURSE");
        assert_eq!(all[&wearing].args[8], ".PAVEMENT.");
        assert_eq!(container_of(&all, wearing), carriageway);
        assert_eq!(container_of(&all, by_tag(&all, BASE)), carriageway);
        assert_eq!(container_of(&all, kerb), road);
        assert_eq!(container_of(&all, fill), roadside);
        assert_eq!(container_of(&all, wall), roadside);
        assert_eq!(container_of(&all, sign), site);
        // A USERDEFINED wall says what it is (IfcWall's CorrectPredefinedType).
        assert_eq!(all[&wall].args[4], "'NOISEBARRIER'");
        assert_eq!(all[&wall].args[8], ".USERDEFINED.");

        assert_eq!(
            relative_to(&all, placement_of(&all, road)),
            placement_of(&all, site)
        );
        assert_eq!(
            relative_to(&all, placement_of(&all, carriageway)),
            placement_of(&all, road)
        );
        assert_eq!(
            relative_to(&all, placement_of(&all, wearing)),
            placement_of(&all, carriageway)
        );
        assert_eq!(
            relative_to(&all, placement_of(&all, kerb)),
            placement_of(&all, road)
        );
        assert_eq!(
            relative_to(&all, placement_of(&all, fill)),
            placement_of(&all, roadside)
        );
    }

    // ---- the body ------------------------------------------------------------------

    // A box is 8 points and 12 triangles, closed, enclosing what its sides
    // make: the wearing course 100 x 3.5 x 0.05 = 17.5 m3, the kerb
    // 100 x 0.15 x 0.3 = 4.5 m3.
    #[test]
    fn a_body_is_a_closed_triangulated_face_set_enclosing_the_meshes_volume() {
        let out = exported(&road_input(None));
        let all = instances_in(&out.text);
        let wearing = body_of(&all, by_tag(&all, WEARING));
        assert!(wearing.closed);
        assert_eq!(wearing.points.len(), 8);
        assert_eq!(wearing.triangles.len(), 12);
        // The coordinates are written to the last bit (step_real), so the
        // only error is the sum's: its terms are products of order 1e3 x 2e3 x
        // 10 = 2e7, whose ulp is 4e-9, and twelve of them cancel to 6 x 17.5.
        near(volume_of(&wearing), 17.5, 1e-6);
        near(
            volume_of(&body_of(&all, by_tag(&all, KERB))),
            100.0 * 0.15 * 0.3,
            1e-6,
        );
        // Representation type and context: the Body of a tessellation.
        assert!(out.text.contains("'Body','Tessellation'"));
    }

    // The schema's closed face set has its normals pointing out of the
    // material (8.8.3.47): a mesh wound inwards is written wound outwards, so
    // its volume is still positive.
    #[test]
    fn a_closed_mesh_wound_inwards_is_written_wound_outwards() {
        let mut inward = element(
            "inward",
            "pavement",
            class("IfcCourse", "PAVEMENT", ""),
            "Main",
            "CARRIAGEWAY",
            box_mesh(0.0, 0.0, 0.0, 1.0, 2.0, 3.0),
        );
        for face in &mut inward.mesh.faces {
            face.swap(1, 2);
        }
        let out = exported(&elements_input(vec![inward]));
        let all = instances_in(&out.text);
        let body = body_of(&all, by_tag(&all, "inward"));
        assert!(body.closed);
        near(volume_of(&body), 6.0, 1e-12); // 1 x 2 x 3
    }

    // A face set is Closed only when every edge is used once each way; one
    // triangle is not, and is written open and said.
    #[test]
    fn an_open_mesh_is_written_open_and_said() {
        let sheet = TriangleMesh {
            vertices: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 2]],
        };
        let out = exported(&elements_input(vec![element(
            "sheet",
            "marking",
            class("IfcCourse", "PAVEMENT", ""),
            "Main",
            "",
            sheet,
        )]));
        let all = instances_in(&out.text);
        let body = body_of(&all, by_tag(&all, "sheet"));
        assert!(!body.closed);
        assert_eq!(body.triangles.len(), 1);
        assert!(warned(&out, "1 road elements are not closed solids"));
    }

    // A mesh that repeats each corner once a face (36 vertices for a box's 12
    // triangles) is still the box: points at exactly one place are written
    // once, so it is closed; a face naming one place twice is dropped.
    #[test]
    fn coincident_vertices_are_written_once_and_a_degenerate_face_is_dropped() {
        let shared = box_mesh(0.0, 0.0, 0.0, 2.0, 2.0, 2.0);
        let mut split = TriangleMesh::default();
        for face in &shared.faces {
            let first = u32::try_from(split.vertices.len()).unwrap();
            for &index in face {
                split.vertices.push(shared.vertices[index as usize]);
            }
            split.faces.push([first, first + 1, first + 2]);
        }
        split.faces.push([0, 0, 1]); // degenerate: names vertex 0 twice
        assert_eq!(split.vertices.len(), 36);
        let out = exported(&elements_input(vec![element(
            "split",
            "pavement",
            class("IfcCourse", "PAVEMENT", ""),
            "",
            "",
            split,
        )]));
        let all = instances_in(&out.text);
        let body = body_of(&all, by_tag(&all, "split"));
        assert_eq!(body.points.len(), 8);
        assert_eq!(body.triangles.len(), 12);
        assert!(body.closed);
        near(volume_of(&body), 8.0, 1e-12);
    }

    // The plan outline given is the FootPrint; without one, the mesh's plan
    // hull is - for a box, its rectangle, closed by repeating its first point.
    #[test]
    fn the_foot_print_is_the_plan_outline_or_the_meshes_hull() {
        let mut outlined = element(
            "outlined",
            "kerb",
            class("IfcKerb", "NOTDEFINED", ""),
            "",
            "",
            box_mesh(0.0, 0.0, 0.0, 10.0, 1.0, 1.0),
        );
        outlined.plan = vec![Polyline2::new(vec![p(0.0, 0.5), p(10.0, 0.5)], false)];
        let hulled = element(
            "hulled",
            "kerb",
            class("IfcKerb", "NOTDEFINED", ""),
            "",
            "",
            box_mesh(20.0, 0.0, 0.0, 30.0, 1.0, 1.0),
        );
        let out = exported(&elements_input(vec![outlined, hulled]));
        let all = instances_in(&out.text);
        let foot_print_of = |product: i32| {
            let mut point_counts = Vec::new();
            let shape = ref_of(&all[&product].args[6]);
            for representation in refs_of(&all[&shape].args[2]) {
                if all[&representation].args[1] == "'FootPrint'" {
                    assert_eq!(all[&representation].args[2], "'Curve2D'");
                    for curve in refs_of(&all[&representation].args[3]) {
                        point_counts.push(refs_of(&all[&curve].args[0]).len());
                    }
                }
            }
            point_counts
        };
        assert_eq!(foot_print_of(by_tag(&all, "outlined")), [2]);
        assert_eq!(foot_print_of(by_tag(&all, "hulled")), [5]); // 4 corners + 1
    }

    // ---- material, quantities, properties ------------------------------------------

    #[test]
    fn each_material_is_written_once_and_associated_with_every_element_of_it() {
        let mut road = road_model();
        road.elements[1].material = String::from("Asphalt AC14"); // the base, now of the wearing course's material
        let input = ExportInput {
            road,
            ..ExportInput::default()
        };
        let out = exported(&input);
        let all = instances_in(&out.text);
        let mut materials = BTreeMap::new();
        for material in of_type(&all, "IFCMATERIAL") {
            let name = all[&material].args[0].clone();
            assert!(!materials.contains_key(&name), "{name} is written twice");
            materials.insert(name, material);
        }
        assert_eq!(materials.len(), 2); // Asphalt AC14, Concrete
        assert!(materials.contains_key("'Asphalt AC14'"));
        let mut found = false;
        for rel in of_type(&all, "IFCRELASSOCIATESMATERIAL") {
            if ref_of(&all[&rel].args[5]) == materials["'Asphalt AC14'"] {
                found = true;
                assert_eq!(
                    refs_of(&all[&rel].args[4]),
                    [by_tag(&all, WEARING), by_tag(&all, BASE)]
                );
            }
        }
        assert!(found);
    }

    // What a class's base quantity set names, with the type it gives it, goes
    // in that set (Qto_CourseBaseQuantities, 6.6.5.2: Length, Width,
    // Thickness, Volume, GrossVolume, Weight), measured as 'BaseQuantities';
    // the rest - a course's Area, a sign's Count - in Katana_Quantities. A
    // weight in tonnes is written in kilograms, under its own unit, since the
    // project's units have none for mass.
    #[test]
    fn quantities_the_template_names_are_base_quantities_and_the_rest_katanas() {
        let out = exported(&road_input(None));
        let all = instances_in(&out.text);
        let wearing = sets_of(&all, by_tag(&all, WEARING));
        let base = *wearing
            .get("'Qto_CourseBaseQuantities'")
            .expect("the course's base quantities");
        assert_eq!(all[&base].class, "IFCELEMENTQUANTITY");
        assert_eq!(all[&base].args[4], "'BaseQuantities'");
        assert_eq!(
            quantities_of(&all, base),
            set_of(&[
                "IFCQUANTITYLENGTH('Length',$,$,100.,$)",
                "IFCQUANTITYLENGTH('Width',$,$,3.5,$)",
                "IFCQUANTITYLENGTH('Thickness',$,$,0.05,$)",
                "IFCQUANTITYVOLUME('GrossVolume',$,$,17.5,$)",
            ])
        );
        let own = *wearing
            .get("'Katana_Quantities'")
            .expect("the course's own quantities");
        assert_eq!(all[&own].args[4], "$");
        assert_eq!(
            quantities_of(&all, own),
            set_of(&["IFCQUANTITYAREA('Area',$,$,350.,$)"])
        );

        let kerb = sets_of(&all, by_tag(&all, KERB));
        assert!(kerb.contains_key("'Qto_KerbBaseQuantities'"));
        let kilogram = *of_type(&all, "IFCSIUNIT").last().unwrap();
        let unit_line = &all[&kilogram].line;
        assert_eq!(
            &unit_line[unit_line.find('=').unwrap() + 1..],
            "IFCSIUNIT(*,.MASSUNIT.,.KILO.,.GRAM.)"
        );
        assert_eq!(
            quantities_of(&all, kerb["'Qto_KerbBaseQuantities'"]),
            BTreeSet::from([
                String::from("IFCQUANTITYLENGTH('Length',$,$,100.,$)"),
                format!("IFCQUANTITYWEIGHT('Weight',$,#{kilogram},1500.,$)"),
            ])
        );

        let sign = sets_of(&all, by_tag(&all, SIGN));
        assert_eq!(
            quantities_of(&all, sign["'Qto_SignBaseQuantities'"]),
            set_of(&[
                "IFCQUANTITYLENGTH('Height',$,$,0.9,$)",
                "IFCQUANTITYLENGTH('Width',$,$,0.6,$)",
            ])
        );
        // A count is an INTEGER, "1" and not "1.": CountValue is an
        // IfcCountMeasure, and IFC 4.3.2.0 (IFC4X3_ADD2) declares `TYPE
        // IfcCountMeasure = INTEGER` (IFC4 declared a NUMBER, which is why the
        // REAL was written).
        assert_eq!(
            quantities_of(&all, sign["'Katana_Quantities'"]),
            set_of(&["IFCQUANTITYCOUNT('Count',$,$,1,$)"])
        );

        // The fill's compacted volume is its template's (6.6.5.4); the wall,
        // whose class has no template here, has no quantity set at all.
        assert!(
            sets_of(&all, by_tag(&all, FILL)).contains_key("'Qto_EarthworksFillBaseQuantities'")
        );
        assert!(!sets_of(&all, by_tag(&all, WALL)).contains_key("'Katana_Quantities'"));
    }

    // The road model names a solid's volume GrossVolume whatever the solid
    // is, and a course's thickness Depth (cad/road/road_section); each class's
    // base quantity set names its own. The model's name is written as the
    // set's where the set gives it that type: a cut's volume is its
    // UndisturbedVolume, a fill's its CompactedVolume, a kerb's its Volume, a
    // course's Depth its Thickness - and what the set has no name for stays
    // in Katana_Quantities. An element that already has the set's own name is
    // left as it is.
    #[test]
    fn the_road_models_quantity_names_are_written_as_their_classes_base_quantities_name_them() {
        let mut course = element(
            "course",
            "pavement",
            class("IfcCourse", "PAVEMENT", ""),
            "Main",
            "CARRIAGEWAY",
            box_mesh(0.0, 0.0, 0.0, 100.0, 3.5, 0.05),
        );
        course.quantities = vec![
            quantity("Length", 100.0, "m"),
            quantity("Width", 3.5, "m"),
            quantity("Depth", 0.05, "m"),
            quantity("GrossArea", 350.0, "m2"),
            quantity("GrossVolume", 17.5, "m3"),
        ];
        let mut kerb = element(
            "kerb",
            "kerb",
            class("IfcKerb", "NOTDEFINED", ""),
            "Main",
            "ROADSIDE",
            box_mesh(0.0, 3.5, 0.0, 100.0, 3.65, 0.15),
        );
        kerb.quantities = vec![
            quantity("Length", 100.0, "m"),
            quantity("Width", 0.15, "m"),
            quantity("Height", 0.15, "m"),
            quantity("GrossVolume", 2.25, "m3"),
        ];
        let mut cut = element(
            "cut",
            "earthworks",
            class("IfcEarthworksCut", "CUT", ""),
            "Main",
            "ROADSEGMENT",
            box_mesh(0.0, 3.65, -2.0, 100.0, 9.0, 0.0),
        );
        cut.quantities = vec![
            quantity("Length", 100.0, "m"),
            quantity("GrossArea", 600.0, "m2"),
            quantity("GrossVolume", 1000.0, "m3"),
        ];
        let mut fill = element(
            "fill",
            "earthworks",
            class("IfcEarthworksFill", "EMBANKMENT", ""),
            "Main",
            "ROADSEGMENT",
            box_mesh(0.0, -9.0, -2.0, 100.0, 0.0, 0.0),
        );
        fill.quantities = vec![
            quantity("Length", 100.0, "m"),
            quantity("GrossArea", 600.0, "m2"),
            quantity("GrossVolume", 850.0, "m3"),
        ];
        // A cut that already has the set's own name keeps both, as they are.
        let mut both = element(
            "both",
            "earthworks",
            class("IfcEarthworksCut", "CUT", ""),
            "Main",
            "ROADSEGMENT",
            box_mesh(0.0, 10.0, -2.0, 100.0, 12.0, 0.0),
        );
        both.quantities = vec![
            quantity("GrossVolume", 7.0, "m3"),
            quantity("UndisturbedVolume", 5.0, "m3"),
        ];
        let out = exported(&elements_input(vec![course, kerb, cut, fill, both]));
        let all = instances_in(&out.text);

        let course_sets = sets_of(&all, by_tag(&all, "course"));
        assert_eq!(
            quantities_of(&all, course_sets["'Qto_CourseBaseQuantities'"]),
            set_of(&[
                "IFCQUANTITYLENGTH('Length',$,$,100.,$)",
                "IFCQUANTITYLENGTH('Width',$,$,3.5,$)",
                "IFCQUANTITYLENGTH('Thickness',$,$,0.05,$)",
                "IFCQUANTITYVOLUME('GrossVolume',$,$,17.5,$)",
            ])
        );
        assert_eq!(
            quantities_of(&all, course_sets["'Katana_Quantities'"]),
            set_of(&["IFCQUANTITYAREA('GrossArea',$,$,350.,$)"])
        );

        let kerb_sets = sets_of(&all, by_tag(&all, "kerb"));
        assert_eq!(
            quantities_of(&all, kerb_sets["'Qto_KerbBaseQuantities'"]),
            set_of(&[
                "IFCQUANTITYLENGTH('Length',$,$,100.,$)",
                "IFCQUANTITYLENGTH('Width',$,$,0.15,$)",
                "IFCQUANTITYLENGTH('Height',$,$,0.15,$)",
                "IFCQUANTITYVOLUME('Volume',$,$,2.25,$)",
            ])
        );
        assert!(
            !kerb_sets.contains_key("'Katana_Quantities'"),
            "nothing of the kerb's is left over"
        );

        let cut_sets = sets_of(&all, by_tag(&all, "cut"));
        assert_eq!(
            quantities_of(&all, cut_sets["'Qto_EarthworksCutBaseQuantities'"]),
            set_of(&[
                "IFCQUANTITYLENGTH('Length',$,$,100.,$)",
                "IFCQUANTITYVOLUME('UndisturbedVolume',$,$,1000.,$)",
            ])
        );
        assert_eq!(
            quantities_of(&all, cut_sets["'Katana_Quantities'"]),
            set_of(&["IFCQUANTITYAREA('GrossArea',$,$,600.,$)"])
        );

        let fill_sets = sets_of(&all, by_tag(&all, "fill"));
        assert_eq!(
            quantities_of(&all, fill_sets["'Qto_EarthworksFillBaseQuantities'"]),
            set_of(&[
                "IFCQUANTITYLENGTH('Length',$,$,100.,$)",
                "IFCQUANTITYVOLUME('CompactedVolume',$,$,850.,$)",
            ])
        );

        let both_sets = sets_of(&all, by_tag(&all, "both"));
        assert_eq!(
            quantities_of(&all, both_sets["'Qto_EarthworksCutBaseQuantities'"]),
            set_of(&["IFCQUANTITYVOLUME('UndisturbedVolume',$,$,5.,$)"])
        );
        assert_eq!(
            quantities_of(&all, both_sets["'Katana_Quantities'"]),
            set_of(&["IFCQUANTITYVOLUME('GrossVolume',$,$,7.,$)"])
        );
    }

    // A quantity in a unit IFC's simple quantities do not take, a negative
    // one (each IfcQuantity*'s WR: the value is not negative), and a name
    // repeated on one element (IfcElementQuantity's UniqueQuantityNames) are
    // left out, each kind said once.
    #[test]
    fn a_quantity_the_schema_would_refuse_is_left_out_and_said() {
        let mut kerb = element(
            "k",
            "kerb",
            class("IfcKerb", "NOTDEFINED", ""),
            "",
            "",
            unit_box(),
        );
        kerb.quantities = vec![
            quantity("Length", 3.0, "ft"),
            quantity("Height", -0.1, "m"),
            quantity("Width", 0.15, "m"),
            quantity("Width", 0.2, "m"),
        ];
        let out = exported(&elements_input(vec![kerb]));
        let all = instances_in(&out.text);
        assert_eq!(
            quantities_of(
                &all,
                sets_of(&all, by_tag(&all, "k"))["'Qto_KerbBaseQuantities'"]
            ),
            set_of(&["IFCQUANTITYLENGTH('Width',$,$,0.15,$)"])
        );
        assert!(warned(&out, "quantities in \"ft\" are left out"));
        assert!(warned(&out, "1 road element quantities are negative"));
        assert!(warned(&out, "repeat a name on their element"));
    }

    // IfcQuantityCount's CountValue is an IfcCountMeasure: an INTEGER in
    // IFC4X3_ADD2 (`TYPE IfcCountMeasure = INTEGER`), so a count is written as
    // one. A count with no integer spelling is left out, said: a half (2.5),
    // and one past the largest 32-bit signed integer, 2147483647 -
    // IfcOpenShell 0.8.5 reads a larger STEP INTEGER as 2147483647, silently.
    // By hand, for each:
    //   Posts    28            -> 28
    //   None     0             -> 0 (a count may be none; not negative)
    //   Nearly   12 - 4e-10    -> 12 (4e-10 from a whole number, inside the
    //                             1e-9 a few roundings of a count stay within)
    //   Largest  2147483647    -> 2147483647 (the bound itself is written)
    //   Half     2.5, Past 2147483648, Huge 3000000000 -> left out, 3 said
    // and a left-out count does not take its name: a second "Half" that is
    // whole is written.
    #[test]
    fn a_count_is_an_integer_and_one_that_cannot_be_is_left_out_and_said() {
        let mut posts = element(
            "p",
            "sign",
            class("IfcSign", "PICTORAL", ""),
            "",
            "",
            unit_box(),
        );
        posts.quantities = vec![
            quantity("Posts", 28.0, ""),
            quantity("None", 0.0, ""),
            quantity("Nearly", 12.0 - 4e-10, ""),
            quantity("Largest", 2147483647.0, ""),
            quantity("Half", 2.5, ""),
            quantity("Past", 2147483648.0, ""),
            quantity("Huge", 3000000000.0, ""),
            quantity("Half", 3.0, ""),
        ];
        let out = exported(&elements_input(vec![posts]));
        let all = instances_in(&out.text);
        assert_eq!(
            quantities_of(
                &all,
                sets_of(&all, by_tag(&all, "p"))["'Katana_Quantities'"]
            ),
            set_of(&[
                "IFCQUANTITYCOUNT('Posts',$,$,28,$)",
                "IFCQUANTITYCOUNT('None',$,$,0,$)",
                "IFCQUANTITYCOUNT('Nearly',$,$,12,$)",
                "IFCQUANTITYCOUNT('Largest',$,$,2147483647,$)",
                "IFCQUANTITYCOUNT('Half',$,$,3,$)",
            ])
        );
        assert!(warned(&out, "3 road element counts are not whole numbers"));
        // No REAL is ever written for a count, whatever its value.
        assert!(!out.text.contains("IFCQUANTITYCOUNT('Posts',$,$,28.,"));
    }

    // A count that is not finite or is negative is the same refusal as any
    // other quantity's (the schema's WR), said with those, not as a
    // fractional count.
    #[test]
    fn a_count_that_is_negative_or_not_finite_is_left_out_with_the_other_invalid_quantities() {
        let mut posts = element(
            "p",
            "sign",
            class("IfcSign", "PICTORAL", ""),
            "",
            "",
            unit_box(),
        );
        posts.quantities = vec![
            quantity("Negative", -1.0, ""),
            quantity("Infinite", f64::INFINITY, ""),
            quantity("Missing", f64::NAN, ""),
        ];
        let out = exported(&elements_input(vec![posts]));
        let all = instances_in(&out.text);
        assert!(!sets_of(&all, by_tag(&all, "p")).contains_key("'Katana_Quantities'"));
        assert!(warned(
            &out,
            "3 road element quantities are negative, not finite or unnamed"
        ));
        assert!(!warned(&out, "counts are not whole numbers"));
    }

    // What Katana knows of the element (Katana_RoadElement) and the element's
    // own properties, as they are (Katana_RoadProperties).
    #[test]
    fn an_element_carries_what_it_is_and_its_own_properties() {
        let out = exported(&road_input(None));
        let all = instances_in(&out.text);
        let sets = sets_of(&all, by_tag(&all, WEARING));
        assert!(
            sets.contains_key("'Katana_RoadElement'")
                && sets.contains_key("'Katana_RoadProperties'")
        );
        let values_of = |set: i32| {
            let mut values = BTreeMap::new();
            for property in refs_of(&all[&set].args[4]) {
                values
                    .entry(all[&property].args[0].clone())
                    .or_insert_with(|| all[&property].args[2].clone());
            }
            values
        };
        let own = values_of(sets["'Katana_RoadElement'"]);
        assert_eq!(own["'Id'"], format!("IFCIDENTIFIER('{WEARING}')"));
        assert_eq!(own["'Kind'"], "IFCLABEL('pavement')");
        assert_eq!(own["'Facility'"], "IFCLABEL('Main')");
        assert_eq!(own["'Part'"], "IFCLABEL('CARRIAGEWAY')");
        assert_eq!(own["'ClosedSolid'"], "IFCBOOLEAN(.T.)");
        let properties = values_of(sets["'Katana_RoadProperties'"]);
        assert_eq!(properties["'Component'"], "IFCLABEL('lane')");
        assert_eq!(properties["'Stations'"], "IFCLABEL('0 to 100')");
    }

    // ---- features ------------------------------------------------------------------

    // A cut is the void "from modification of existing terrain" (6.6.3.4): it
    // voids the surface it was taken from, is contained in nothing
    // (5.4.3.30), and is placed relative to the ground it voids.
    #[test]
    fn a_cut_voids_the_ground_it_is_taken_from_and_is_contained_nowhere() {
        let surface = ground();
        let out = exported(&road_input(Some(&surface)));
        let all = instances_in(&out.text);
        let cut = by_tag(&all, CUT);
        assert_ne!(cut, 0);
        assert_eq!(all[&cut].class, "IFCEARTHWORKSCUT");
        assert_eq!(all[&cut].args[8], ".CUT.");
        assert_eq!(container_of(&all, cut), 0);
        let mut terrain = 0;
        for element in of_type(&all, "IFCGEOGRAPHICELEMENT") {
            if all[&element].args[2] == "'ground'" {
                terrain = element;
            }
        }
        assert_ne!(terrain, 0);
        assert_eq!(of_type(&all, "IFCRELVOIDSELEMENT").len(), 1);
        let voids = &all[&of_type(&all, "IFCRELVOIDSELEMENT")[0]];
        assert_eq!(ref_of(&voids.args[4]), terrain);
        assert_eq!(ref_of(&voids.args[5]), cut);
        assert_eq!(
            relative_to(&all, placement_of(&all, cut)),
            placement_of(&all, terrain)
        );
        assert_eq!(out.classes["IfcGeographicElement"], 1); // the surface; no stand-in

        // A void takes no material (IfcRelAssociatesMaterial's
        // NoVoidElement): the cut's is said in its Katana_RoadElement, and no
        // IfcMaterial 'Clay' is written for nothing to use.
        for rel in of_type(&all, "IFCRELASSOCIATESMATERIAL") {
            assert!(!refs_of(&all[&rel].args[4]).contains(&cut));
        }
        for material in of_type(&all, "IFCMATERIAL") {
            assert_ne!(all[&material].args[0], "'Clay'");
        }
        let own = sets_of(&all, cut)["'Katana_RoadElement'"];
        let said = refs_of(&all[&own].args[4]).into_iter().any(|property| {
            all[&property].args[0] == "'Material'" && all[&property].args[2] == "IFCLABEL('Clay')"
        });
        assert!(said);
    }

    // Without the ground's surface in the file, the cut voids an
    // IfcGeographicElement TERRAIN that stands for that ground with no shape,
    // so that the void the schema requires has an element to be in.
    #[test]
    fn without_its_surface_a_cut_voids_a_ground_that_stands_for_it() {
        let out = exported(&road_input(None));
        let all = instances_in(&out.text);
        let grounds = of_type(&all, "IFCGEOGRAPHICELEMENT");
        assert_eq!(grounds.len(), 1);
        let stand = &all[&grounds[0]];
        assert_eq!(stand.args[2], "'ground'");
        assert_eq!(stand.args[6], "$"); // no shape
        assert_eq!(stand.args[8], ".TERRAIN.");
        assert_eq!(
            ref_of(&all[&of_type(&all, "IFCRELVOIDSELEMENT")[0]].args[4]),
            grounds[0]
        );
    }

    // A marking adheres to the course it is painted on
    // (IfcRelAdheresToElement, 5.4.3.45): of the wearing course (top 10.00)
    // and the base (top 9.95) under it, the wearing course.
    #[test]
    fn a_marking_adheres_to_the_highest_course_under_it_and_is_contained_nowhere() {
        let out = exported(&road_input(None));
        let all = instances_in(&out.text);
        let marking = by_tag(&all, MARKING);
        assert_ne!(marking, 0);
        assert_eq!(all[&marking].class, "IFCSURFACEFEATURE");
        assert_eq!(all[&marking].args[8], ".LINEMARKING.");
        assert_eq!(container_of(&all, marking), 0);
        assert_eq!(of_type(&all, "IFCRELADHERESTOELEMENT").len(), 1);
        let adheres = &all[&of_type(&all, "IFCRELADHERESTOELEMENT")[0]];
        assert_eq!(ref_of(&adheres.args[4]), by_tag(&all, WEARING));
        assert_eq!(refs_of(&adheres.args[5]), [marking]);
        assert_eq!(
            relative_to(&all, placement_of(&all, marking)),
            placement_of(&all, by_tag(&all, WEARING))
        );
    }

    // A host named outright is taken; a marking with nothing under it has
    // nothing to adhere to, and an IfcSurfaceFeature adhering to nothing would
    // break the schema's mandatory inverse: it is not written, and said.
    #[test]
    fn a_marking_with_nothing_under_it_is_not_written_and_said() {
        let lonely = element(
            "lonely",
            "marking",
            class("IfcSurfaceFeature", "LINEMARKING", ""),
            "Main",
            "",
            box_mesh(0.0, 0.0, 0.0, 10.0, 0.1, 0.003),
        );
        let mut named = element(
            "named",
            "marking",
            class("IfcSurfaceFeature", "LINEMARKING", ""),
            "Other",
            "",
            box_mesh(500.0, 0.0, 0.0, 510.0, 0.1, 0.003),
        );
        named.host = String::from("slab");
        let slab = element(
            "slab",
            "pavement",
            class("IfcSlab", "PAVING", ""),
            "Main",
            "",
            box_mesh(100.0, 100.0, 0.0, 110.0, 110.0, 0.2),
        );
        let out = exported(&elements_input(vec![lonely, named, slab]));
        let all = instances_in(&out.text);
        assert_eq!(by_tag(&all, "lonely"), 0);
        assert_eq!(out.road_elements, 2);
        assert_eq!(out.road_elements_skipped, 1);
        assert!(warned(
            &out,
            "road element \"lonely\" is not written: a surface feature adheres"
        ));
        assert_ne!(by_tag(&all, "named"), 0);
        assert_eq!(
            ref_of(&all[&of_type(&all, "IFCRELADHERESTOELEMENT")[0]].args[4]),
            by_tag(&all, "slab")
        );
    }

    // A road object has a road only when one is given (facility=), so the
    // marking a person places with no road is the site's: it adheres to the
    // course of whichever road is under it, as a marking of a road adheres to
    // that road's, and is not dropped for want of a road it was never told.
    #[test]
    fn a_marking_that_names_no_road_adheres_to_the_course_under_it() {
        let mut input = road_input(None);
        for each in &mut input.road.elements {
            if each.id == MARKING {
                each.facility.clear();
                each.part.clear();
            }
        }
        let out = exported(&input);
        let all = instances_in(&out.text);
        let marking = by_tag(&all, MARKING);
        assert_ne!(marking, 0, "the marking was dropped");
        assert_eq!(out.road_elements_skipped, 0);
        assert_eq!(of_type(&all, "IFCRELADHERESTOELEMENT").len(), 1);
        let adheres = &all[&of_type(&all, "IFCRELADHERESTOELEMENT")[0]];
        assert_eq!(ref_of(&adheres.args[4]), by_tag(&all, WEARING));
        assert_eq!(refs_of(&adheres.args[5]), [marking]);
    }

    // With nothing under it the reason given is the true one: it names no
    // road AND no course of any road lies under it.
    #[test]
    fn a_marking_that_names_no_road_and_has_no_course_under_it_says_so() {
        let out = exported(&elements_input(vec![
            element(
                "lonely",
                "marking",
                class("IfcSurfaceFeature", "LINEMARKING", ""),
                "",
                "",
                box_mesh(0.0, 0.0, 0.0, 10.0, 0.1, 0.003),
            ),
            element(
                "slab",
                "pavement",
                class("IfcSlab", "PAVING", ""),
                "Main",
                "",
                box_mesh(100.0, 100.0, 0.0, 110.0, 110.0, 0.2),
            ),
        ]));
        assert_eq!(out.road_elements_skipped, 1);
        assert!(warned(
            &out,
            "road element \"lonely\" is not written: a surface feature adheres to an element, \
             and it names no road (facility=)"
        ));
    }

    // Two closed boxes that touch - the cells of a box culvert side by side,
    // with no gap between their walls - share positions without sharing an
    // edge. Each is closed by its own indices, so the face set is closed:
    // merging them by position would make four faces of the shared edges and
    // write a closed solid as an open face set with a warning.
    #[test]
    fn closed_shells_that_touch_are_one_closed_face_set_not_an_open_one() {
        let left = box_mesh(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let right = box_mesh(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
        let mut both = left;
        let offset = u32::try_from(both.vertices.len()).unwrap();
        both.vertices.extend_from_slice(&right.vertices);
        for face in &right.faces {
            both.faces
                .push([face[0] + offset, face[1] + offset, face[2] + offset]);
        }
        let out = exported(&elements_input(vec![element(
            "cells",
            "culvert",
            class("IfcPipeSegment", "CULVERT", ""),
            "",
            "",
            both,
        )]));
        let all = instances_in(&out.text);
        let body = body_of(&all, by_tag(&all, "cells"));
        assert!(body.closed);
        // The positions are not merged: 2 x 8 points and 2 x 12 triangles.
        assert_eq!(body.points.len(), 16);
        assert_eq!(body.triangles.len(), 24);
        // Two unit cubes.
        near(volume_of(&body), 2.0, 1e-12);
        assert!(!warned(&out, "not closed solids"));
    }

    // What is not closed by its indices is still judged as it always was:
    // merged by position, and open when an edge is left over. A pair of
    // touching boxes with a face missing is open.
    #[test]
    fn touching_shells_with_a_face_missing_are_still_written_open_and_said() {
        let left = box_mesh(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let right = box_mesh(1.0, 0.0, 0.0, 2.0, 1.0, 1.0);
        let mut both = left;
        let offset = u32::try_from(both.vertices.len()).unwrap();
        both.vertices.extend_from_slice(&right.vertices);
        // The second box without its last two triangles (its +x face).
        for face in &right.faces[..right.faces.len() - 2] {
            both.faces
                .push([face[0] + offset, face[1] + offset, face[2] + offset]);
        }
        let out = exported(&elements_input(vec![element(
            "cells",
            "culvert",
            class("IfcPipeSegment", "CULVERT", ""),
            "",
            "",
            both,
        )]));
        let all = instances_in(&out.text);
        assert!(!body_of(&all, by_tag(&all, "cells")).closed);
        assert!(warned(&out, "1 road elements are not closed solids"));
    }

    // ---- what is refused -----------------------------------------------------------

    // Everything that would make an invalid file is left out, each element
    // said by its id, and the rest is written.
    #[test]
    fn what_would_make_an_invalid_file_is_left_out_and_said() {
        let kerb = || class("IfcKerb", "NOTDEFINED", "");
        let mut broken = element(
            "broken",
            "kerb",
            class("IfcKerb", "", ""),
            "",
            "",
            unit_box(),
        );
        broken.mesh.vertices[3].z = f64::NAN;
        let mut bad_plan = element(
            "bad-plan",
            "kerb",
            class("IfcKerb", "", ""),
            "",
            "",
            unit_box(),
        );
        bad_plan.plan = vec![Polyline2::new(
            vec![p(0.0, 0.0), p(f64::INFINITY, 0.0)],
            false,
        )];
        let elements = vec![
            element("good", "kerb", kerb(), "", "", unit_box()),
            element("", "kerb", kerb(), "", "", unit_box()),
            element("good", "kerb", kerb(), "", "", unit_box()),
            element(
                "asphalt",
                "pavement",
                class("IfcCourse", "ASPHALT", ""),
                "",
                "",
                unit_box(),
            ),
            element(
                "made-up",
                "thing",
                class("IfcRoadThing", "", ""),
                "",
                "",
                unit_box(),
            ),
            element("nameless", "thing", class("", "", ""), "", "", unit_box()),
            element(
                "note",
                "thing",
                class("IfcAnnotation", "", ""),
                "",
                "",
                unit_box(),
            ),
            element(
                "vague",
                "wall",
                class("IfcWall", "USERDEFINED", ""),
                "",
                "",
                unit_box(),
            ),
            broken,
            element(
                "empty",
                "kerb",
                class("IfcKerb", "", ""),
                "",
                "",
                TriangleMesh::default(),
            ),
            bad_plan,
        ];

        let out = exported(&elements_input(elements));
        assert_eq!(out.road_elements, 1);
        assert_eq!(out.road_elements_skipped, 10);
        for words in [
            "road element \"\" is not written: it has no id",
            "road element \"good\" is not written: an earlier element has the same id",
            "\"ASPHALT\" is not a predefined type of IfcCourse",
            "IfcRoadThing is not a class this export writes",
            "road element \"nameless\" is not written: it names no IFC class",
            "IfcAnnotation is not a class this export writes",
            "road element \"vague\" is not written: it is USERDEFINED",
            "road element \"broken\" is not written: its mesh is not valid",
            "road element \"empty\" is not written: its mesh has no face",
            "road element \"bad-plan\" is not written: its plan outline",
        ] {
            assert!(warned(&out, words), "{words}");
        }
        // Each is in the account too, as not written.
        let not_written: usize = out
            .tally
            .iter()
            .filter(|row| row.entity.is_empty() && row.why.starts_with("not written: "))
            .map(|row| row.count)
            .sum();
        assert_eq!(not_written, 10);
    }

    // A part the enumeration lists is its own type; ROADSEGMENT is the one
    // longitudinal part ("Longitudinal, linear segment of a road"), an
    // INTERSECTION a region; a part it does not list is USERDEFINED of that
    // name, its usage NOTDEFINED.
    #[test]
    fn a_parts_usage_is_what_the_enumeration_says_of_it_and_an_unlisted_part_is_user_defined() {
        let out = exported(&elements_input(
            ["ROADSEGMENT", "INTERSECTION", "BUSWAY"]
                .into_iter()
                .map(|part| {
                    element(
                        &format!("in-{part}"),
                        "pavement",
                        class("IfcCourse", "PAVEMENT", ""),
                        "Main",
                        part,
                        unit_box(),
                    )
                })
                .collect(),
        ));
        let all = instances_in(&out.text);
        let mut parts = BTreeMap::new();
        for part in of_type(&all, "IFCROADPART") {
            let args = &all[&part].args;
            parts.insert(
                args[2].clone(),
                vec![args[4].clone(), args[9].clone(), args[10].clone()],
            );
        }
        assert_eq!(
            parts["'Main ROADSEGMENT'"],
            ["$", ".LONGITUDINAL.", ".ROADSEGMENT."]
        );
        assert_eq!(
            parts["'Main INTERSECTION'"],
            ["$", ".REGION.", ".INTERSECTION."]
        );
        assert_eq!(
            parts["'Main BUSWAY'"],
            ["'BUSWAY'", ".NOTDEFINED.", ".USERDEFINED."]
        );
    }

    // ---- the whole file ------------------------------------------------------------

    // GlobalIds are made from the ids ("road/element/<id>" in the project's
    // namespace): the same model exported twice is the same file, and an
    // element keeps its GlobalId when others are added before it.
    #[test]
    fn global_ids_come_from_the_ids_and_are_stable_across_exports() {
        let first = exported(&road_input(None));
        let second = exported(&road_input(None));
        assert_eq!(first.text, second.text);

        let all = instances_in(&first.text);
        assert_eq!(
            all[&by_tag(&all, WEARING)].args[0],
            format!("'{}'", guid_for("Test", &format!("road/element/{WEARING}")))
        );
        assert_eq!(
            all[&of_type(&all, "IFCROAD")[0]].args[0],
            format!("'{}'", guid_for("Test", "road/facility/Main"))
        );

        let mut grown = road_input(None);
        grown.road.elements.insert(
            0,
            element(
                "object/sign-0",
                "sign",
                class("IfcSign", "PICTORAL", ""),
                "",
                "",
                unit_box(),
            ),
        );
        let after = instances_in(&exported(&grown).text);
        assert_eq!(
            after[&by_tag(&after, WEARING)].args[0],
            all[&by_tag(&all, WEARING)].args[0]
        );
    }

    // The model's problems are the export's warnings, and its reply counts
    // the road model - only when one was given, so an export without one
    // answers as it always has.
    #[test]
    fn the_models_problems_are_warnings_and_the_reply_counts_the_road_model() {
        let out = exported(&road_input(None));
        assert!(warned(
            &out,
            "the road model: corridor Main: the profile ends at 100"
        ));
        let reply = format_export_reply(&out, "road.ifc", false);
        assert!(
            reply.contains("\nroad elements=8 skipped=0 roads=1 parts=2\n"),
            "{reply}"
        );
        assert!(
            reply.contains(
                "\nobject from=\"road Main\" count=2 class=\"IfcCourse\" \
                 predefined=\"PAVEMENT\" object_type=\"\" system=\"\" \
                 why=\"the road model's pavement\""
            ),
            "{reply}"
        );
        assert!(reply.contains("\nobject from=\"road model\" count=1 class=\"IfcSign\""));
        // The road and its parts are objects of the reply too: the window's
        // preview table is filled from these records.
        assert!(
            reply.contains(
                "\nobject from=\"road Main\" count=1 class=\"IfcRoad\" \
                 predefined=\"NOTDEFINED\" object_type=\"\" system=\"\" \
                 why=\"a corridor, the road its parts and elements are in\""
            ),
            "{reply}"
        );
        assert!(
            reply.contains(
                "\nobject from=\"road Main\" count=1 class=\"IfcRoadPart\" \
                 predefined=\"CARRIAGEWAY\""
            ),
            "{reply}"
        );

        let without = exported(&ExportInput::default());
        assert!(!format_export_reply(&without, "x.ifc", false).contains("road elements="));
    }

    // What the export writes of a cut, the import reads back: an
    // IfcEarthworksCut is an IfcFeatureElementSubtraction, and the generated
    // class table called every such feature "not drawn" (an opening is what a
    // wall is cut by, not an object), so the largest solids of a road model
    // vanished on a round trip, with no count and no warning. A cut is drawn
    // now, as its FootPrint outline like the fill beside it. By hand, the cut
    // batter of road_model() is the box [1000, 1100] x [1997, 2000] x
    // [9.75, 10.5], so its plan hull is the rectangle x 1000..1100,
    // y 1997..2000, and the file (not georeferenced, so written at the
    // project's own coordinates) gives that outline back. The fill is read
    // too, and an opening is still not: a feature of another kind is not an
    // object.
    #[test]
    fn a_cut_batter_is_read_back_from_its_file_as_its_foot_print_outline() {
        let surface = ground();
        let out = exported(&road_input(Some(&surface)));
        let back = must(read_ifc(&out.text, &ImportOptions::default()));
        assert_eq!(back.classes["IfcEarthworksCut"], 1);
        assert_eq!(back.classes["IfcEarthworksFill"], 1);
        let cut = back
            .entities
            .iter()
            .rev()
            .find(|entity| metadata(entity, "ifc.class") == "IfcEarthworksCut")
            .expect("the cut was not read back");
        assert_eq!(metadata(cut, "ifc.predefinedType"), "CUT");
        assert_eq!(metadata(cut, "ifc.tag"), CUT);
        assert_eq!(cut.layer, "Road/batter");
        let Geometry::Polyline(outline) = &cut.geometry else {
            panic!("the cut is not an outline: {:?}", cut.geometry);
        };
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (1e300, 1e300, -1e300, -1e300);
        for vertex in &outline.vertices {
            min_x = f64::min(min_x, vertex.x);
            max_x = f64::max(max_x, vertex.x);
            min_y = f64::min(min_y, vertex.y);
            max_y = f64::max(max_y, vertex.y);
        }
        near(min_x, 1000.0, 1e-6);
        near(max_x, 1100.0, 1e-6);
        near(min_y, 1997.0, 1e-6);
        near(max_y, 2000.0, 1e-6);
    }

    // A feature of another kind is still not an object of its own: an
    // opening in a file is read past, not drawn. And the stand-in ground a
    // cut voids when its surface is not in the file has no shape at all (no
    // Representation): it comes in as a point at its placement, counted, and
    // the warning says no shape rather than "solids", which it is not.
    #[test]
    fn the_stand_in_ground_has_no_shape_and_the_import_says_so_not_that_it_is_a_solid() {
        let out = exported(&road_input(None)); // the cut names "ground"; no surface is given
        let back = must(read_ifc(&out.text, &ImportOptions::default()));
        assert!(back.products_as_points >= 1); // the TERRAIN stand-in at least
        let mut said = false;
        for warning in &back.warnings {
            assert!(!warning.contains("are solids"), "{warning}");
            said = said || warning.contains("objects have no shape Katana can draw");
        }
        assert!(said);

        // An opening, read past: the same file with its cut renamed to an
        // IfcOpeningElement (also an IfcFeatureElementSubtraction) loses the
        // cut from the objects read, and the class table counts it nowhere.
        assert!(out.text.contains("IFCEARTHWORKSCUT("));
        let opening = out
            .text
            .replacen("IFCEARTHWORKSCUT(", "IFCOPENINGELEMENT(", 1);
        let read = must(read_ifc(&opening, &ImportOptions::default()));
        assert!(!read.classes.contains_key("IfcEarthworksCut"));
        assert!(!read.classes.contains_key("IfcOpeningElement"));
    }

    // The local origin is the south-west corner of everything written, the
    // road model included: rounded down to 100 m, 1000, 1900 for the cut at
    // y 1997.
    #[test]
    fn the_local_origin_takes_the_road_model_into_account() {
        let mut georeferenced = options();
        georeferenced.georeference.name = String::from("EPSG:7856");
        let out = exported_with(&road_input(None), &georeferenced);
        let all = instances_in(&out.text);
        let conversions = of_type(&all, "IFCMAPCONVERSION");
        assert_eq!(conversions.len(), 1);
        assert_eq!(all[&conversions[0]].args[2], "1000.");
        assert_eq!(all[&conversions[0]].args[3], "1900.");
    }

    // The file IfcOpenShell judges (tools/check_ifc.py; the
    // ifc.the_road_model_export_is_valid_to_ifcopenshell test, registered
    // where it is installed): the model with its ground, georeferenced,
    // written whole. KATANA_IFC_ROAD_FILE names where; without it, a
    // temporary file is written and removed.
    #[test]
    fn the_road_model_is_written_for_the_validator() {
        let scratch = Scratch::new("ifc_the_road_model_is_written_for_the_validator");
        let path = std::env::var_os("KATANA_IFC_ROAD_FILE")
            .map_or_else(|| scratch.root.join("katana_road_model.ifc"), PathBuf::from);
        let surface = ground();
        let mut georeferenced = options();
        georeferenced.georeference.name = String::from("EPSG:7856");
        let written = must(write_ifc_file(
            &road_input(Some(&surface)),
            &path,
            &georeferenced,
        ));
        assert!(path.exists());
        assert_eq!(written.road_elements, 8);
        assert!(!written.classes.contains_key("IfcBuildingElementProxy"));
    }
}

mod ifc_rules_file {
    use super::*;

    // A drawing entity cannot be a feature of anything: a rule naming one is
    // refused when its file is read and when it is handed to the writer.
    #[test]
    fn a_rule_may_not_name_a_feature_class() {
        for feature in ["IfcEarthworksCut", "IfcSurfaceFeature"] {
            let read = parse_classification_rules(&format!(
                "rule,words,class,predefined_type\nx,X,{feature},\n"
            ))
            .expect_err(feature);
            assert!(
                read.describe().contains("is a feature of another element"),
                "{}",
                read.describe()
            );

            let mut with = options();
            with.rules = vec![ClassificationRule {
                name: String::from("x"),
                words: vec![String::from("X")],
                kinds: Vec::new(),
                target: class(feature, "", ""),
                system: String::new(),
            }];
            let written = write_ifc(&ExportInput::default(), &with).expect_err(feature);
            assert!(written.describe().contains("a feature of another element"));
        }
        // The classes the road model writes as elements are a rule's to name.
        must(parse_classification_rules(
            "rule,words,class,predefined_type\nbase,BASE,IfcCourse,PAVEMENT\n",
        ));
    }
}
