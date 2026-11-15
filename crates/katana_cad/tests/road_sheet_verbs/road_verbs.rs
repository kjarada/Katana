// The road verbs (tests/cad/road/test_road_verbs.cpp; road/road_verbs.rs)
// through the interpreter on a Document, as every front end runs them: each
// edit one undo step, the replies key=value, the refusals specific, and the
// road model made again exactly when what it is made from changes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use katana_cad::bim::bim_object::{BimBuildCounts, BimModel, bim_build_counts, bim_model};
use katana_cad::bim::bim_view::scene_meshes;
use katana_cad::road::corridor_model::{
    Corridor, CorridorSet, DEFAULT_CURVE_TOLERANCE, corridors_from_json, corridors_to_json,
};
use katana_cad::road::road_template::{
    Component, ComponentKind, KerbShape, RoadTemplate, Side, road_templates_from_json,
};
use katana_cad::{CommandInterpreter, DocumentChange, NO_LAYER_OVERRIDES};
use katana_core::{Error, ErrorCode};
use katana_entity::entity::heights_of;
use katana_entity::{Entity, Geometry, PropertyValue};
use katana_geometry::Point3;
use katana_terrain::{NamedSurface, TinSurface};

use crate::road_fixtures::plane_ground;
use crate::{
    ScratchDirectory, Session, contains, head, must, near, quoted, read_file, read_text, write_file,
};

// A 100 m straight along +x with a level design at 10.
#[track_caller]
fn road(session: &mut Session) {
    session.ok("ALIGN NEW road 0,0 100,0");
    session.ok("ALIGN DESIGN road 0,10 100,10");
}

// A flat surface named `name` at `level` over the test road, put in the
// drawing as SURFACE puts one.
#[track_caller]
fn add_ground(session: &mut Session, name: &str, level: f64) {
    let mut store = session.document.surfaces().clone();
    let shared = Arc::new(plane_ground(0.0, 0.0, level, 100.0));
    must(store.add(NamedSurface {
        name: String::from(name),
        surface: Some(shared),
        source: String::from("test"),
        style: String::new(),
        visible: true,
    }));
    must(session.document.set_surfaces(store, "TEST_SURFACE"));
}

// The template `name` as the drawing holds it now.
#[track_caller]
fn template(session: &Session, name: &str) -> RoadTemplate {
    match session.document.road_templates().find(name) {
        Some(found) => found.clone(),
        None => panic!("no road template {name}"),
    }
}

// The component `name` on `side` of template `template_name`, as it is now.
#[track_caller]
fn component(session: &Session, template_name: &str, side: Side, name: &str) -> Component {
    let found = template(session, template_name);
    match found.find(side, name) {
        Some(index) => found.components[index].clone(),
        None => panic!("{template_name} has no {name} on {side:?}"),
    }
}

// The corridor `name` as the drawing holds it now.
#[track_caller]
fn corridor(session: &Session, name: &str) -> Corridor {
    match session.document.corridors().find(name) {
        Some(found) => found.clone(),
        None => panic!("no corridor {name}"),
    }
}

// Every entity of the drawing, in the model's order.
fn entities(session: &Session) -> Vec<Entity> {
    let mut out = Vec::new();
    session
        .document
        .model()
        .entities
        .for_each(|entity| out.push(entity.clone()));
    out
}

// The entities tagged as the strings of corridor `name` (road.corridor).
fn tagged(session: &Session, name: &str) -> usize {
    entities(session)
        .iter()
        .filter(|entity| {
            matches!(
                entity.properties.get("road.corridor"),
                Some(PropertyValue::Text(owner)) if owner == name
            )
        })
        .count()
}

// The surface `name` replaced by a different one, as an edit of a surface
// replaces it (SurfaceStore::replace): the same name, another object.
#[track_caller]
fn replace_ground(session: &mut Session, name: &str, level: f64) {
    let mut store = session.document.surfaces().clone();
    let shared = Arc::new(plane_ground(0.0, 0.0, level, 100.0));
    must(store.replace(name, Some(shared)));
    must(session.document.set_surfaces(store, "TEST_SURFACE_EDIT"));
}

// How many times each half of the road model was made since `before`, after
// asking for the model as a view does.
fn made_since(session: &Session, before: &BimBuildCounts) -> BimBuildCounts {
    let _ = bim_model(&session.document);
    let now = bim_build_counts(&session.document);
    BimBuildCounts {
        corridors: now.corridors - before.corridors,
        placed: now.placed - before.placed,
    }
}

fn counts(corridors: usize, placed: usize) -> BimBuildCounts {
    BimBuildCounts { corridors, placed }
}

// A road with a corridor targeting "ground" and a sign naming "dem", with
// "other" in the drawing for no one; the model asked for once, so that its two
// halves are made.
struct CachedRoad {
    session: Session,
    first: Arc<BimModel>,
    counts: BimBuildCounts,
}

impl CachedRoad {
    #[track_caller]
    fn new() -> Self {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        for name in ["ground", "dem", "other"] {
            add_ground(&mut session, name, 9.0);
        }
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" surface=ground");
        session.ok("ROADOBJECT ADD sign ALIGNMENT road station=50 offset=4 surface=dem");
        let first = bim_model(&session.document);
        let counts = bim_build_counts(&session.document);
        Self {
            session,
            first,
            counts,
        }
    }
}

// A file of the case's own scratch directory.
fn scratch_file(scratch: &ScratchDirectory, name: &str) -> PathBuf {
    scratch.path.join(format!("katana_road_{name}"))
}

// ROADTEMPLATE IMPORT of `bytes` is refused with `code`, saying `word`, and
// changes nothing.
#[track_caller]
fn refuse_import(session: &mut Session, file: &Path, bytes: &[u8], code: ErrorCode, word: &str) {
    let steps = session.steps();
    write_file(file, bytes);
    let shown = String::from_utf8_lossy(bytes).into_owned();
    let error = session.refused(&format!("ROADTEMPLATE IMPORT {}", quoted(file)));
    assert_eq!(error.code, code, "{shown}");
    assert!(
        contains(&error.message, word),
        "{shown}\n  -> {}",
        error.message
    );
    assert_eq!(session.steps(), steps, "{shown}");
    assert!(
        session.document.road_templates().templates.is_empty(),
        "{shown}"
    );
}

// A corridors record with one corridor carrying `member` as well.
fn corridor_with(member: &str) -> katana_core::Result<CorridorSet> {
    corridors_from_json(&format!(
        r#"{{"format":"katana-corridors","version":1,"corridors":[{{"name":"Bad","alignment":"road","template":"t","interval":10,{member}}}]}}"#
    ))
}

mod road_verbs {
    use super::*;

    #[test]
    fn defaults_add_the_three_standard_templates_as_one_step_and_then_nothing() {
        let mut session = Session::default();
        assert!(contains(&session.ok("ROADTEMPLATE"), "templates=0"));
        let added = session.ok("ROADTEMPLATE DEFAULTS");
        assert!(contains(&added, "added=3 skipped=0"), "{added}");
        assert_eq!(session.steps(), 1);
        assert_eq!(session.document.road_templates().templates.len(), 3);
        let again = session.ok("ROADTEMPLATE DEFAULTS");
        assert!(contains(&again, "added=0 skipped=3"), "{again}");
        assert_eq!(session.steps(), 1, "adding nothing is no step");
        let list = session.ok("ROADTEMPLATE LIST");
        assert!(
            contains(&list, "template name=\"rural two-lane\" left=4 right=4"),
            "{list}"
        );
    }

    #[test]
    fn a_template_is_made_and_edited_component_by_component_each_edit_one_step() {
        let mut session = Session::default();
        session.ok("ROADTEMPLATE NEW street description=\"a test street\"");
        let lane = session.ok("ROADTEMPLATE ADD street both lane width=3.25 crossfall=-2.5");
        assert!(
            contains(
                &lane,
                "side=left name=lane kind=lane width=3.25 crossfall=-2.5"
            ),
            "{lane}"
        );
        assert!(contains(&lane, "side=right name=lane"), "{lane}");
        session.ok("ROADTEMPLATE ADD street both batter cut=1.5 fill=3");
        // A kerb goes before the batter, as the side's last but one.
        session.ok("ROADTEMPLATE ADD street left kerb shape=mountable height=0.1");
        // A second lane is named after its place.
        let second = session.ok("ROADTEMPLATE ADD street right lane");
        assert!(contains(&second, "name=lane2"), "{second}");
        assert_eq!(session.steps(), 5);

        let street = template(&session, "STREET");
        assert_eq!(street.description, "a test street");
        let left = street.side(Side::Left);
        assert_eq!(left.len(), 3);
        assert_eq!(left[0].name, "lane");
        assert_eq!(left[1].kind, ComponentKind::Kerb);
        assert_eq!(left[1].shape, KerbShape::Mountable);
        assert_eq!(left[2].kind, ComponentKind::Batter);
        assert_eq!(left[2].cut_slope, 1.5);

        // SET by a bare name edits both sides; side:name one.
        session.ok("ROADTEMPLATE SET street lane width=3.5");
        session.ok("ROADTEMPLATE SET street left:lane crossfall=-3");
        let left_lane = component(&session, "street", Side::Left, "lane");
        let right_lane = component(&session, "street", Side::Right, "lane");
        assert_eq!(left_lane.width, 3.5);
        assert_eq!(right_lane.width, 3.5);
        assert_eq!(left_lane.crossfall, -3.0);
        assert_eq!(right_lane.crossfall, -2.5);

        // Courses, top down. A lane comes with a flexible pavement's three
        // (AC14, base, subbase); a seal goes on top of them.
        session.ok("ROADTEMPLATE COURSE street lane ADD seal thickness=0.01 at=1 ifc=protection");
        let info = session.ok("ROADTEMPLATE INFO street");
        assert!(
            contains(
                &info,
                "course template=street component=left:lane name=seal thickness=0.01 \
                 material=\"\" ifc=PROTECTION"
            ),
            "{info}"
        );
        session.ok("ROADTEMPLATE COURSE street right:lane SET base thickness=0.25");
        session.ok("ROADTEMPLATE COURSE street lane REMOVE subbase");
        let left_lane = component(&session, "street", Side::Left, "lane");
        let right_lane = component(&session, "street", Side::Right, "lane");
        assert_eq!(left_lane.courses.len(), 3);
        assert_eq!(left_lane.courses[0].name, "seal");
        assert_eq!(left_lane.courses[1].name, "AC14");
        assert_eq!(left_lane.courses[2].material, "crushed rock");
        assert_eq!(left_lane.courses[2].thickness, 0.2);
        assert_eq!(right_lane.courses[2].thickness, 0.25);
        session.ok("ROADTEMPLATE REMOVE street lane2");
        assert_eq!(template(&session, "street").side(Side::Right).len(), 2);

        // Every one of those was a step, and undoing them all leaves no key.
        let steps = session.steps();
        assert_eq!(steps, 11);
        session.ok(&format!("UNDO {steps}"));
        assert!(session.document.road_templates().templates.is_empty());
        assert!(
            !session
                .document
                .metadata()
                .unknown_keys
                .contains_key("road_templates")
        );
    }

    #[test]
    fn a_bare_name_is_removed_from_both_sides_whichever_side_was_made_first() {
        // A side's first component is appended at the end of the vector, so a
        // template whose right side was made first is stored
        //   [right:lane 0, right:shoulder 1, left:lane 2, left:shoulder 3]
        // and the left lane's index (2) is the larger one: REMOVE lane must erase
        // 2 and then 0, and never the 0 first (which slid left:shoulder into 2).
        let mut session = Session::default();
        session.ok("ROADTEMPLATE NEW four");
        session.ok("ROADTEMPLATE ADD four right lane");
        session.ok("ROADTEMPLATE ADD four right shoulder");
        session.ok("ROADTEMPLATE ADD four left lane");
        session.ok("ROADTEMPLATE ADD four left shoulder");
        let four = template(&session, "four");
        assert_eq!(four.components.len(), 4);
        assert_eq!(
            four.components[0].side,
            Side::Right,
            "the premise: right is stored first"
        );
        assert_eq!(four.components[2].side, Side::Left);
        let before = session.steps();
        let reply = session.ok("ROADTEMPLATE REMOVE four lane");
        assert!(contains(&reply, "side=left name=lane removed"), "{reply}");
        assert!(contains(&reply, "side=right name=lane removed"), "{reply}");
        assert_eq!(session.steps(), before + 1, "both sides, one undo step");
        let four = template(&session, "four");
        assert_eq!(four.components.len(), 2);
        assert!(
            four.find(Side::Left, "shoulder").is_some(),
            "the left shoulder stays"
        );
        assert!(
            four.find(Side::Right, "shoulder").is_some(),
            "the right shoulder stays"
        );
        assert!(four.find(Side::Left, "lane").is_none());
        assert!(four.find(Side::Right, "lane").is_none());
        session.ok("UNDO");
        assert_eq!(template(&session, "four").components.len(), 4);

        // Two components only, right made before left: the indices are [1, 0],
        // and walking them with the 0 erased first read past the end.
        session.ok("ROADTEMPLATE NEW two");
        session.ok("ROADTEMPLATE ADD two right lane");
        session.ok("ROADTEMPLATE ADD two left lane");
        session.ok("ROADTEMPLATE REMOVE two lane");
        assert!(template(&session, "two").components.is_empty());

        // ADD both after a right-only component stores [right 0, right 1, left 2].
        session.ok("ROADTEMPLATE NEW mixed");
        session.ok("ROADTEMPLATE ADD mixed right lane");
        session.ok("ROADTEMPLATE ADD mixed both shoulder");
        session.ok("ROADTEMPLATE REMOVE mixed shoulder");
        let mixed = template(&session, "mixed");
        assert_eq!(mixed.components.len(), 1);
        assert_eq!(mixed.components[0].name, "lane");

        // A name that is on no side is refused and changes nothing.
        let steps = session.steps();
        assert_eq!(
            session.refused("ROADTEMPLATE REMOVE mixed nothing").code,
            ErrorCode::NotFound
        );
        assert_eq!(session.steps(), steps);
        assert_eq!(template(&session, "mixed").components.len(), 1);
    }

    #[test]
    fn a_kerb_faces_inward_or_outward_and_the_divided_roads_median_kerb_falls_outward() {
        let mut session = Session::default();
        session.ok("ROADTEMPLATE DEFAULTS");
        // The divided road's lanes sit 100 mm under the median's top (the profile):
        // hand-worked in RoadSection.TheDividedRoadsLanesSit...
        let section = session.ok("ROADTEMPLATE SECTION \"divided road\"");
        assert!(
            contains(
                &section,
                "point code=left/kerb/lip offset=2.300 level=-0.100"
            ),
            "{section}"
        );
        assert!(
            contains(
                &section,
                "point code=left/lane1/outer offset=5.800 level=-0.205"
            ),
            "{section}"
        );
        assert!(
            contains(
                &section,
                "point code=right/shoulder/outer offset=-11.800 level=-0.385"
            ),
            "{section}"
        );
        let info = session.ok("ROADTEMPLATE INFO \"divided road\"");
        assert!(
            contains(&info, "name=kerb kind=kerb shape=mountable faces=outward"),
            "{info}"
        );

        // The facing is a key, a word, and refused when it is neither.
        let steps = session.steps();
        let sideways = session.refused("ROADTEMPLATE SET \"divided road\" kerb faces=sideways");
        assert_eq!(sideways.code, ErrorCode::ParseFailure);
        assert!(
            contains(&sideways.message, "inward"),
            "{}",
            sideways.message
        );
        assert_eq!(session.steps(), steps);
        let turned = session.ok("ROADTEMPLATE SET \"divided road\" kerb faces=inward");
        assert!(contains(&turned, "faces=inward"), "{turned}");
        assert_eq!(session.steps(), steps + 1);
        assert!(contains(
            &session.ok("ROADTEMPLATE SECTION \"divided road\""),
            "point code=left/kerb/back offset=2.300 level=0.100"
        ));
        // A flush strip takes no facing; turning a kerb to one needs it inward.
        session.ok("ROADTEMPLATE SET \"divided road\" kerb faces=outward");
        let strip = session.refused("ROADTEMPLATE SET \"divided road\" kerb shape=edgestrip");
        assert_eq!(strip.code, ErrorCode::InvalidArgument);
        assert!(contains(&strip.message, "no face"), "{}", strip.message);
        session.ok("ROADTEMPLATE SET \"divided road\" kerb faces=inward shape=edgestrip");
        assert!(contains(
            &session.ok("ROADTEMPLATE INFO \"divided road\""),
            "name=kerb kind=kerb shape=edgestrip width="
        ));
    }

    #[test]
    fn refusals_name_what_is_wrong_and_change_nothing() {
        let mut session = Session::default();
        session.ok("ROADTEMPLATE DEFAULTS");
        let steps = session.steps();
        assert_eq!(
            session.refused("ROADTEMPLATE INFO nowhere").code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session.refused("ROADTEMPLATE NEW \"urban street\"").code,
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE ADD \"urban street\" up lane")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE ADD \"urban street\" left tunnel")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE SET \"urban street\" lane width=wide")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE SET \"urban street\" lane colour=#zz")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE SET \"urban street\" lane flavour=sweet")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE SET \"urban street\" lane width=0")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE SET \"urban street\" nothing width=1")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE COURSE \"urban street\" kerb ADD x thickness=0.1")
                .code,
            ErrorCode::InvalidArgument
        ); // a kerb has no courses
        assert_eq!(
            session
                .refused("ROADTEMPLATE COURSE \"urban street\" lane ADD AC14 thickness=0.1")
                .code,
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE COURSE \"urban street\" lane SET x ifc=BRICK")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session
                .refused("ROADTEMPLATE COURSE \"urban street\" lane SET AC14 ifc=BRICK")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(session.steps(), steps);
    }

    #[test]
    fn a_corridor_is_made_with_its_layers_the_cut_hidden_and_built_at_once() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        let before = session.steps();
        let made =
            session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" interval=20");
        assert_eq!(session.steps(), before + 1);
        assert!(
            contains(
                &made,
                "corridor name=Main alignment=road template=\"urban street\" surface=\"\" \
                 interval=20"
            ),
            "{made}"
        );
        // No surface: the pavement alone, and a problem saying why.
        assert!(
            contains(
                &made,
                "built corridor=Main from=0.000 to=100.000 runs=1 sections=6"
            ),
            "{made}"
        );
        assert!(contains(&made, "no target surface"), "{made}");
        {
            let layers = &session.document.model().layers;
            assert!(layers.contains("road/earthworks/cut"));
            assert!(!layers.find("road/earthworks/cut").expect("cut").visible);
            assert!(layers.find("road/earthworks/fill").expect("fill").visible);
            assert!(layers.contains("road/pavement"));
            assert!(layers.contains("road/kerb"));
        }
        // One undo takes the corridor and its layers.
        session.ok("UNDO");
        assert!(session.document.corridors().corridors.is_empty());
        assert!(
            !session
                .document
                .model()
                .layers
                .contains("road/earthworks/cut")
        );
    }

    #[test]
    fn a_corridor_needs_what_it_names() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("ALIGN NEW plain 0,0 50,0");
        assert_eq!(
            session
                .refused("CORRIDOR NEW A ALIGNMENT nowhere TEMPLATE \"urban street\"")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session
                .refused("CORRIDOR NEW A ALIGNMENT plain TEMPLATE \"urban street\"")
                .code,
            ErrorCode::InvalidArgument
        ); // no design profile
        assert_eq!(
            session
                .refused("CORRIDOR NEW A ALIGNMENT road TEMPLATE nowhere")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session
                .refused("CORRIDOR NEW A ALIGNMENT road TEMPLATE \"urban street\" surface=none")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session
                .refused("CORRIDOR NEW A ALIGNMENT road TEMPLATE \"urban street\" interval=0")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.refused("CORRIDOR NEW A ALIGNMENT road").code,
            ErrorCode::ParseFailure
        );
        session.ok("CORRIDOR NEW A ALIGNMENT road TEMPLATE \"urban street\"");
        // A template a corridor uses is not deleted from under it.
        let used = session.refused("ROADTEMPLATE DELETE \"urban street\"");
        assert!(contains(&used.message, "corridor A"), "{}", used.message);
        assert_eq!(
            session
                .refused("CORRIDOR REGION A ADD 60 40 \"divided road\"")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn renaming_a_template_renames_it_in_its_corridors_in_the_same_step() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        session.ok("CORRIDOR REGION Main ADD 40 60 \"urban street\"");
        let before = session.steps();
        let renamed = session.ok("ROADTEMPLATE RENAME \"urban street\" town");
        assert!(contains(&renamed, "corridors=1"), "{renamed}");
        assert_eq!(session.steps(), before + 1);
        let main = corridor(&session, "main");
        assert_eq!(main.template_name, "town");
        assert_eq!(main.regions[0].template_name, "town");
        session.ok("UNDO");
        assert_eq!(corridor(&session, "Main").template_name, "urban street");
        assert!(
            session
                .document
                .road_templates()
                .find("urban street")
                .is_some()
        );
    }

    #[test]
    fn regions_and_superelevation_are_kept_in_station_order() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"rural two-lane\"");
        session.ok("CORRIDOR REGION Main ADD 60 80 \"divided road\"");
        session.ok("CORRIDOR REGION Main ADD 10 20 \"urban street\"");
        assert_eq!(
            session
                .refused("CORRIDOR REGION Main ADD 15 30 \"urban street\"")
                .code,
            ErrorCode::InvalidArgument
        ); // overlaps 10 to 20
        session.ok("CORRIDOR SUPERELEVATION Main ADD 70 6 -6");
        session.ok("CORRIDOR SUPERELEVATION Main ADD 30 -3 -3");
        // The same station again replaces its entry.
        session.ok("CORRIDOR SUPERELEVATION Main ADD 70 5 -5");
        let main = corridor(&session, "Main");
        assert_eq!(main.regions.len(), 2);
        assert_eq!(main.regions[0].start, 10.0);
        assert_eq!(main.regions[1].template_name, "divided road");
        assert_eq!(main.superelevation.len(), 2);
        assert_eq!(main.superelevation[0].station, 30.0);
        assert_eq!(main.superelevation[1].left, 5.0);
        let info = session.ok("CORRIDOR INFO Main");
        assert!(
            contains(&info, "region from=10 to=20 template=\"urban street\""),
            "{info}"
        );
        assert!(
            contains(&info, "superelevation station=70 left=5 right=-5"),
            "{info}"
        );
        session.ok("CORRIDOR REGION Main REMOVE 10");
        assert_eq!(
            session.refused("CORRIDOR REGION Main REMOVE 10").code,
            ErrorCode::NotFound
        );
        session.ok("CORRIDOR SUPERELEVATION Main CLEAR");
        assert!(corridor(&session, "Main").superelevation.is_empty());
    }

    #[test]
    fn quantities_list_each_object_with_totals_and_write_the_csv() {
        let scratch = ScratchDirectory::new("road-quantities");
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE NEW lanes");
        session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
        session.ok("ROADTEMPLATE COURSE lanes lane REMOVE subbase");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
        let quantities = session.ok("CORRIDOR QUANTITIES Main");
        // 100 m of AC14 50 mm and base 200 mm, 3.5 m wide, each side: by hand
        // 17.5 and 70 m3 a side; totals 35 and 140 m3 over 700 m2 each.
        // No target surface: no earthworks were measured, so cut, fill and net are
        // empty - not the 0.000 of a road with none (CLAUDE.md: absent is not zero).
        assert!(
            contains(
                &quantities,
                "quantities corridor=Main objects=4 cut= fill= net=\n"
            ),
            "{quantities}"
        );
        assert!(
            contains(
                &quantities,
                "object id=corridor/Main/left/lane/AC14/1 kind=pavement name=\"Main left lane \
                 AC14\" Length=100.000 Width=3.500 Depth=0.050 GrossArea=350.000 \
                 GrossVolume=17.500"
            ),
            "{quantities}"
        );
        assert!(
            contains(
                &quantities,
                "total kind=pavement material=asphalt count=2 length=200.000 area=700.000 \
                 volume=35.000"
            ),
            "{quantities}"
        );
        assert!(
            contains(
                &quantities,
                "material=\"crushed rock\" count=2 length=200.000 area=700.000 volume=140.000"
            ),
            "{quantities}"
        );

        let file = scratch.path.join("katana_road_quantities.csv");
        let _ = std::fs::remove_file(&file);
        let written = session.ok(&format!("CORRIDOR QUANTITIES ALL csv={}", quoted(&file)));
        assert!(contains(&written, "rows=4"), "{written}");
        // RFC 4180 records end in CRLF, as the VOLUME verbs' do: read as bytes.
        let text = read_text(&file);
        assert!(
            text.starts_with(
                "id,name,kind,facility,part,material,Length,Width,Depth,Height,GrossArea,\
                 GrossVolume\r\n"
            ),
            "{text}"
        );
        assert!(
            contains(
                &text,
                "corridor/Main/right/lane/base/1,Main right lane base,pavement,Main,TRAFFICLANE,\
                 crushed rock,100.000,3.500,0.200,,350.000,70.000"
            ),
            "{text}"
        );
        // A file already there is not replaced without OVERWRITE, as VOLUME's is
        // not; with it, it is, and the same rows are written.
        write_file(&file, b"keep me");
        let refused = session.refused(&format!("CORRIDOR QUANTITIES Main csv={}", quoted(&file)));
        assert_eq!(refused.code, ErrorCode::AlreadyExists);
        assert!(
            contains(&refused.message, "OVERWRITE"),
            "{}",
            refused.message
        );
        assert_eq!(read_text(&file), "keep me");
        let replaced = session.ok(&format!(
            "CORRIDOR QUANTITIES Main csv={} OVERWRITE",
            quoted(&file)
        ));
        assert!(contains(&replaced, "rows=4"), "{replaced}");
        let rewritten = read_text(&file);
        assert!(rewritten.starts_with("id,name,kind,"), "{rewritten}");
        // OVERWRITE belongs to a file.
        assert_eq!(
            session.refused("CORRIDOR QUANTITIES Main OVERWRITE").code,
            ErrorCode::ParseFailure
        );
        let _ = std::fs::remove_file(file);
        assert_eq!(
            session.refused("CORRIDOR QUANTITIES nowhere").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn an_object_is_listed_and_described_by_its_id() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        let list = session.ok("ROADMODEL");
        assert!(
            contains(
                &list,
                "object id=corridor/Main/left/kerb/1 kind=kerb facility=Main part=ROADSIDE \
                 ifc=IfcKerb layer=road/kerb"
            ),
            "{list}"
        );
        assert!(
            contains(&list, "problem source=\"corridor Main\""),
            "{list}"
        );
        let info = session.ok("ROADMODEL INFO corridor/Main/left/kerb/1");
        assert!(
            contains(&info, "property name=shape value=kerbgutter"),
            "{info}"
        );
        assert!(
            contains(&info, "property name=mountable value=false"),
            "{info}"
        );
        assert_eq!(
            session
                .refused("ROADMODEL INFO corridor/Main/left/nothing/1")
                .code,
            ErrorCode::NotFound
        );
        let json = session.ok("ROADMODEL JSON");
        assert!(
            contains(&json, "\"id\": \"corridor/Main/left/kerb/1\""),
            "{}",
            head(&json, 400)
        );
        assert!(contains(&json, "\"ifcClass\": \"IfcKerb\""));
    }

    #[test]
    fn strings_are_three_dimensional_polylines_that_a_rerun_replaces() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE NEW lanes");
        session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
        let preview = session.ok("CORRIDOR STRINGS Main PREVIEW");
        assert!(
            contains(&preview, "strings created=3 replaced=0"),
            "{preview}"
        );
        assert_eq!(session.document.model().entities.size(), 0);
        let before = session.steps();
        let first = session.ok("CORRIDOR STRINGS Main");
        assert!(
            contains(
                &first,
                "strings created=3 replaced=0 corridor=Main layer=road/strings points=33"
            ),
            "{first}"
        );
        assert_eq!(session.steps(), before + 1);
        assert_eq!(session.document.model().entities.size(), 3);
        // Each a polyline along x with its heights: the lane edge 0.105 under
        // the design level of 10.
        let mut saw_edge = false;
        for entity in entities(&session) {
            assert_eq!(entity.layer, "road/strings");
            let Geometry::Polyline(polyline) = &entity.geometry else {
                panic!("entity {} is not a polyline", entity.id);
            };
            let heights = heights_of(&entity.properties, polyline.vertices.len());
            assert_eq!(heights.len(), 11);
            let Some(PropertyValue::Text(code)) = entity.properties.get("road.string") else {
                panic!("entity {} has no road.string", entity.id);
            };
            if code == "left/lane/outer" {
                saw_edge = true;
                let Some(first) = heights[0] else {
                    panic!("the edge's first vertex has no height");
                };
                near(first, 9.895, 1e-12);
            }
        }
        assert!(saw_edge);
        let second = session.ok("CORRIDOR STRINGS Main");
        assert!(contains(&second, "created=3 replaced=3"), "{second}");
        assert_eq!(session.document.model().entities.size(), 3);
    }

    #[test]
    fn a_renamed_corridor_keeps_its_strings_and_its_next_strings_replaces_them_whatever_the_case() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE NEW lanes");
        session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
        assert!(contains(
            &session.ok("CORRIDOR STRINGS Main"),
            "strings created=3 replaced=0"
        ));
        assert_eq!(tagged(&session, "Main"), 3);

        // Renaming carries the strings with it, in the same undo step.
        let before = session.steps();
        let renamed = session.ok("CORRIDOR RENAME Main Road1");
        assert!(
            contains(&renamed, "corridor renamed from=Main to=Road1 strings=3"),
            "{renamed}"
        );
        assert_eq!(session.steps(), before + 1);
        assert_eq!(tagged(&session, "Main"), 0);
        assert_eq!(tagged(&session, "Road1"), 3);
        // One undo takes the name and the tags back together.
        session.ok("UNDO");
        assert_eq!(tagged(&session, "Main"), 3);
        assert_eq!(tagged(&session, "Road1"), 0);
        assert!(session.document.corridors().find("Main").is_some());
        assert!(session.document.corridors().find("Road1").is_none());
        session.ok("CORRIDOR RENAME Main Road1");
        assert_eq!(tagged(&session, "Road1"), 3);
        // So the next STRINGS replaces them: it does not add a second set.
        let again = session.ok("CORRIDOR STRINGS Road1");
        assert!(contains(&again, "strings created=3 replaced=3"), "{again}");
        assert_eq!(session.document.model().entities.size(), 3);

        // A case-only rename the same; and the name read ignoring case, as
        // corridors are found.
        session.ok("CORRIDOR RENAME Road1 ROAD1");
        assert_eq!(tagged(&session, "ROAD1"), 3);
        assert!(contains(
            &session.ok("CORRIDOR STRINGS road1"),
            "created=3 replaced=3"
        ));
        assert_eq!(session.document.model().entities.size(), 3);

        // A corridor later given the old name, on another alignment, does not take
        // them over: its first STRINGS replaces nothing and the renamed corridor's
        // are still there.
        session.ok("ALIGN NEW other 0,200 60,200");
        session.ok("ALIGN DESIGN other 0,12 60,12");
        session.ok("CORRIDOR NEW Main ALIGNMENT other TEMPLATE lanes");
        assert!(contains(
            &session.ok("CORRIDOR STRINGS Main"),
            "created=3 replaced=0"
        ));
        assert_eq!(session.document.model().entities.size(), 6);
        assert_eq!(tagged(&session, "ROAD1"), 3);
    }

    #[test]
    fn what_was_not_built_or_measured_is_empty_and_a_measured_zero_is_zero() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        // No target surface: the earthworks are not measured. The range was built.
        let made = session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        assert!(
            contains(
                &made,
                "built corridor=Main from=0.000 to=100.000 runs=1 sections=11"
            ),
            "{made}"
        );
        assert!(contains(&made, " cut= fill= net= problems=1"), "{made}");
        // A kerb has no area: its total says nothing of one rather than 0.000, and
        // the courses, which do have one, say it.
        let quantities = session.ok("CORRIDOR QUANTITIES Main");
        assert!(
            contains(
                &quantities,
                "total kind=kerb material=concrete count=2 length=200.000 area= volume=19.125"
            ),
            "{quantities}"
        );
        // 3.3 m of lane and 2.3 m of parking, 100 m, both sides: 1120 m2, 56 m3.
        assert!(
            contains(
                &quantities,
                "total kind=pavement material=asphalt count=4 length=400.000 area=1120.000 \
                 volume=56.000"
            ),
            "{quantities}"
        );

        // A corridor that could not be built has no range: from= and to= empty.
        session.ok("ALIGN DELETE road");
        let gone = session.ok("CORRIDOR BUILD Main");
        assert!(
            contains(&gone, "built corridor=Main from= to= runs=0 sections=0"),
            "{gone}"
        );
        assert!(contains(&gone, " cut= fill= net= problems=1"), "{gone}");
        session.ok("CORRIDOR DELETE Main");

        // A range off the alignment is not built either.
        road(&mut session);
        session.ok("CORRIDOR NEW Far ALIGNMENT road TEMPLATE \"urban street\" from=200 to=300");
        let off = session.ok("CORRIDOR BUILD Far");
        assert!(
            contains(&off, "built corridor=Far from= to= runs=0"),
            "{off}"
        );

        // Measured against a surface, a corridor with no cut says 0.000 of it: the
        // two are different. Ground at 9 under a design of 10, a 4 m lane with no
        // courses and 1 in 4 fill batters: the lane's 4 x 1 and each batter's
        // 0.5 x 4 x 1 = 2 make 6 m2 a side, 1200 m3 in 100 m.
        session.ok("ROADTEMPLATE NEW fillonly");
        session.ok("ROADTEMPLATE ADD fillonly both lane width=4 crossfall=0");
        session.ok("ROADTEMPLATE COURSE fillonly lane REMOVE AC14");
        session.ok("ROADTEMPLATE COURSE fillonly lane REMOVE base");
        session.ok("ROADTEMPLATE COURSE fillonly lane REMOVE subbase");
        session.ok("ROADTEMPLATE ADD fillonly both batter");
        add_ground(&mut session, "ground", 9.0);
        let measured =
            session.ok("CORRIDOR NEW Fill ALIGNMENT road TEMPLATE fillonly surface=ground");
        assert!(
            contains(
                &measured,
                " cut=0.000 fill=1200.000 net=1200.000 problems=0"
            ),
            "{measured}"
        );
        assert!(contains(
            &session.ok("CORRIDOR QUANTITIES Fill"),
            "quantities corridor=Fill objects=2 cut=0.000 fill=1200.000 net=1200.000"
        ));
        // Another corridor with a surface does not make the unmeasured one measured.
        assert!(contains(
            &session.ok("CORRIDOR QUANTITIES Far"),
            " cut= fill= net="
        ));
    }

    #[test]
    fn a_key_a_components_kind_does_not_keep_is_refused_and_not_answered_updated() {
        let mut session = Session::default();
        session.ok("ROADTEMPLATE NEW t");
        session.ok("ROADTEMPLATE ADD t both lane");
        session.ok("ROADTEMPLATE ADD t both batter");
        let steps = session.steps();
        // A lane keeps no material or batter slope, a batter no width or crossfall;
        // each was answered "updated" and made no step.
        for line in [
            "ROADTEMPLATE SET t left:lane material=concrete",
            "ROADTEMPLATE SET t left:lane cut=3",
            "ROADTEMPLATE SET t left:lane depth=0.4",
            "ROADTEMPLATE SET t left:lane shape=barrier",
            "ROADTEMPLATE SET t left:batter width=5",
            "ROADTEMPLATE SET t left:batter crossfall=-10",
            "ROADTEMPLATE SET t left:batter superelevated=on",
            "ROADTEMPLATE ADD t left lane height=0.3",
            "ROADTEMPLATE ADD t left kerb benchheight=1",
            "ROADTEMPLATE SET t lane flavour=sweet",
        ] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{line}");
            assert_eq!(session.steps(), steps, "{line} made a step");
        }
        let lane = session.refused("ROADTEMPLATE SET t left:lane material=concrete");
        assert!(
            contains(
                &lane.message,
                "a lane has no material=; its keys are name layer colour width crossfall \
                 superelevated"
            ),
            "{}",
            lane.message
        );
        assert!(contains(
            &session
                .refused("ROADTEMPLATE SET t left:batter width=5")
                .message,
            "its keys are name layer colour cut fill maxwidth benchheight benchwidth"
        ));
        // What a kind keeps is taken, `color` and `thickness` as `colour` and `depth`.
        session.ok("ROADTEMPLATE SET t left:lane color=#00FF00 width=3.6");
        session.ok("ROADTEMPLATE SET t left:batter cut=3 benchheight=2 benchwidth=3");
        let drain = session.ok("ROADTEMPLATE ADD t left drain thickness=0.4");
        assert!(contains(&drain, "kind=drain"), "{drain}");
        assert!(contains(&drain, "depth=0.4"), "{drain}");
        assert_eq!(session.steps(), steps + 3);
    }

    #[test]
    fn an_edge_strip_is_reported_without_a_face_and_keeps_its_superelevation() {
        let mut session = Session::default();
        session.ok("ROADTEMPLATE NEW s");
        session.ok("ROADTEMPLATE ADD s both lane");
        let strip = session.ok("ROADTEMPLATE ADD s left kerb shape=edgestrip");
        // A flush strip has no face to give a height to or to turn; it has a
        // crossfall, and says whether a superelevation table sets it.
        assert!(!contains(&strip, "height="), "{strip}");
        assert!(!contains(&strip, "faces="), "{strip}");
        assert!(
            contains(
                &strip,
                "shape=edgestrip width=0.15 depth=0.15 crossfall=0 superelevated=no"
            ),
            "{strip}"
        );
        // Another kerb's superelevation means nothing and is refused.
        let refused = session.refused("ROADTEMPLATE ADD s right kerb superelevated=on");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(
            contains(&refused.message, "only an edge strip has"),
            "{}",
            refused.message
        );
        let drain = session.ok("ROADTEMPLATE ADD s right drain superelevated=on");
        assert!(contains(&drain, "superelevated=yes"), "{drain}");
    }

    #[test]
    fn a_corridor_off_its_alignment_says_so_and_a_section_outside_its_range_is_refused() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" from=20 to=60");
        // Sections only over the corridor's stations, the ends included.
        for station in ["20", "40", "60"] {
            assert!(
                contains(
                    &session.ok(&format!("CORRIDOR SECTION Main {station}")),
                    "section corridor=Main station="
                ),
                "{station}"
            );
        }
        for station in ["19.9", "60.1", "90", "-5"] {
            let error = session.refused(&format!("CORRIDOR SECTION Main {station}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{station}");
            assert!(
                contains(
                    &error.message,
                    "is outside corridor Main's stations 20 to 60"
                ),
                "{}",
                error.message
            );
        }

        // A corridor whose whole range is off the alignment has no stations at all.
        session.ok("CORRIDOR SET Main from=200 to=300");
        let none = session.refused("CORRIDOR SECTION Main 250");
        assert_eq!(none.code, ErrorCode::InvalidState);
        assert!(
            contains(&none.message, "after its alignment and profile end"),
            "{}",
            none.message
        );
        // SURFACE names that, as STRINGS does, and not "the ground".
        let surface = session.refused("CORRIDOR SURFACE Main");
        assert_eq!(surface.code, ErrorCode::InvalidState);
        assert!(
            contains(&surface.message, "after its alignment and profile end"),
            "{}",
            surface.message
        );
        assert!(!contains(&surface.message, "ground"), "{}", surface.message);
        assert!(contains(
            &session.refused("CORRIDOR STRINGS Main").message,
            "after its alignment and profile end"
        ));
    }

    #[test]
    fn a_corridor_sets_empty_only_what_it_can_do_without() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        add_ground(&mut session, "ground", 9.0);
        session.ok(
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" surface=ground from=10 \
             to=90",
        );
        let steps = session.steps();
        // Empty clears a surface and each end of the range.
        let cleared = session.ok("CORRIDOR SET Main surface= from= to=");
        assert!(
            contains(&cleared, "surface=\"\" interval=10 from= to="),
            "{cleared}"
        );
        // A corridor always has an alignment and a template: said so, naming the
        // key, not "no road template : ..." with the name left blank.
        for line in [
            "CORRIDOR SET Main alignment=",
            "CORRIDOR SET Main template=",
        ] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{line}");
            assert!(
                contains(&error.message, "needs a name"),
                "{}",
                error.message
            );
        }
        assert_eq!(
            session.refused("CORRIDOR SET Main interval=").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(session.steps(), steps + 1);
        // The help says which keys clear.
        let help = session.ok("HELP CORRIDOR");
        assert!(contains(&help, "surface= from= to="), "{help}");
        assert!(!contains(&help, "(empty clears)"), "{help}");
    }

    #[test]
    fn a_template_is_not_deleted_while_the_corridors_that_may_use_it_cannot_be_read() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        for damage in [
            "{not json",
            r#"{"format":"katana-corridors","version":9,"corridors":[]}"#,
        ] {
            let mut metadata = session.document.metadata().clone();
            metadata
                .unknown_keys
                .insert(String::from("corridors"), String::from(damage));
            session.document.set_metadata(metadata);
            // The corridors are empty to the guard, which is why it must ask.
            let deleted = session.refused("ROADTEMPLATE DELETE \"urban street\"");
            assert_eq!(deleted.code, ErrorCode::InvalidState, "{damage}");
            assert!(
                contains(&deleted.message, "corridors cannot be read"),
                "{}",
                deleted.message
            );
            assert!(
                session
                    .document
                    .road_templates()
                    .find("urban street")
                    .is_some()
            );
            // And BUILD ALL does not say there are none to build.
            assert_eq!(
                session.refused("CORRIDOR BUILD ALL").code,
                ErrorCode::InvalidState,
                "{damage}"
            );
        }
    }

    #[test]
    fn corridor_problems_and_quantities_all_list_the_corridors_and_not_the_placed_objects() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ALIGN NEW other 0,20 100,20");
        session.ok("ALIGN DESIGN other 0,10 100,10");
        session.ok("ROADOBJECT ADD barrier ALIGNMENT other station=10 to=50");
        session.ok("ALIGN DELETE other");
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        // The placed barrier's alignment is gone: that is a problem of the road
        // model, and ROADMODEL PROBLEMS lists it among the corridor's.
        let model = session.ok("ROADMODEL PROBLEMS");
        assert!(
            contains(&model, "source=\"object "),
            "the premise: the barrier has a problem\n{model}"
        );
        let problems = session.ok("CORRIDOR PROBLEMS");
        assert!(contains(&problems, "problems=1\n"), "{problems}");
        assert!(
            contains(&problems, "source=\"corridor Main\""),
            "{problems}"
        );
        assert!(!contains(&problems, "source=\"object "), "{problems}");
        assert!(contains(
            &session.ok("CORRIDOR PROBLEMS Main"),
            "problems=1\n"
        ));
        let all = session.ok("CORRIDOR QUANTITIES ALL");
        assert!(contains(&all, "source=\"corridor Main\""), "{all}");
        assert!(!contains(&all, "source=\"object "), "{all}");
    }

    #[test]
    fn a_corridors_earthworks_name_the_surface_as_the_drawing_spells_it_whatever_was_typed() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE NEW fillonly");
        session.ok("ROADTEMPLATE ADD fillonly both lane width=4 crossfall=0");
        session.ok("ROADTEMPLATE ADD fillonly both batter");
        add_ground(&mut session, "Ground", 11.0);
        // Typed as GROUND, built against "Ground" (the lookup ignores case): the cut
        // carries the drawing's spelling, which the IFC export finds the ground by,
        // exactly.
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE fillonly surface=GROUND");
        let info = session.ok("ROADMODEL INFO corridor/Main/left/earthworks/cut/1");
        assert!(
            contains(&info, "property name=\"Target Surface\" value=Ground"),
            "{info}"
        );
        assert!(!contains(&info, "value=GROUND"), "{info}");
    }

    #[test]
    fn a_component_cannot_take_the_name_of_the_earthworks_objects() {
        let mut session = Session::default();
        session.ok("ROADTEMPLATE NEW t");
        session.ok("ROADTEMPLATE ADD t both lane");
        let steps = session.steps();
        // corridor/<c>/<side>/earthworks/cut/<run> is the cut's id: a component of
        // that name with a course called cut would have made the same id.
        for line in [
            "ROADTEMPLATE ADD t both verge name=earthworks",
            "ROADTEMPLATE ADD t left verge name=Earthworks",
            "ROADTEMPLATE SET t lane name=EARTHWORKS",
        ] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(contains(&error.message, "earthworks"), "{}", error.message);
        }
        assert_eq!(session.steps(), steps);
        session.ok("ROADTEMPLATE SET t lane name=earthwork");
    }

    #[test]
    fn the_finished_and_subgrade_surfaces_go_into_the_drawing_as_one_step() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE NEW lanes");
        session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
        let before = session.steps();
        let reply = session.ok("CORRIDOR SURFACE Main");
        assert!(
            contains(&reply, "surface name=\"Main finished\" kind=finished"),
            "{reply}"
        );
        assert!(
            contains(&reply, "surface name=\"Main subgrade\" kind=subgrade"),
            "{reply}"
        );
        assert_eq!(session.steps(), before + 1);
        let level_at = |name: &str| -> f64 {
            let surfaces = session.document.surfaces();
            let Some(found) = surfaces.find(name) else {
                panic!("no surface {name}");
            };
            let Some(surface) = found.surface.as_ref() else {
                panic!("{name} has no triangles");
            };
            match surface.elevation_at_xy(50.0, 0.0) {
                Some(level) => level,
                None => panic!("(50, 0) is off {name}"),
            }
        };
        near(level_at("Main finished"), 10.0, 1e-9);
        // The lane's courses as they come: 50 + 200 + 250 mm.
        near(level_at("Main subgrade"), 9.5, 1e-9);
        // Again: replaced in place, not added.
        session.ok("CORRIDOR SURFACE Main");
        assert_eq!(session.document.surfaces().all().len(), 2);
    }

    #[test]
    fn a_region_keeps_its_own_strings_and_sections_even_when_it_is_one_interval_long() {
        // The rural two-lane road (the centreline and four codes a side: 9
        // strings) with the urban street (the centreline and seven a side: 15)
        // in a region. Each run has its own strings from its first section to its
        // last, so there are 9 + 15 + 9 whatever the region's length, and the
        // surfaces take every section built - 13 in both cases (5 + 3 + 5, and
        // 5 + 2 + 6) - not the 11 the join's second sections were dropped to.
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"rural two-lane\"");
        for region in ["40 60", "40 50"] {
            session.ok("CORRIDOR REGION Main CLEAR");
            session.ok(&format!(
                "CORRIDOR REGION Main ADD {region} \"urban street\""
            ));
            let strings = session.ok("CORRIDOR STRINGS Main PREVIEW");
            assert!(
                contains(&strings, "strings created=33 replaced=0"),
                "{region}\n{strings}"
            );
            let surface = session.ok("CORRIDOR SURFACE Main PREVIEW");
            assert!(contains(&surface, "kind=finished"), "{surface}");
            let thirteen = surface.matches("sections=13").count();
            assert_eq!(thirteen, 2, "{region} finished and subgrade: {surface}");
        }
    }

    #[test]
    fn the_road_model_is_made_again_when_a_template_changes_and_not_otherwise() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        let first = bim_model(&session.document);
        assert!(
            Arc::ptr_eq(&bim_model(&session.document), &first),
            "asked twice"
        );
        session.ok("LINE 0,50 10,50");
        assert!(
            Arc::ptr_eq(&bim_model(&session.document), &first),
            "a line is not what the road model is made from"
        );
        session.ok("ROADTEMPLATE SET \"urban street\" lane width=3.6");
        let second = bim_model(&session.document);
        assert!(!Arc::ptr_eq(&second, &first));
        assert_ne!(*second, *first);
        session.ok("UNDO");
        assert_eq!(
            *bim_model(&session.document),
            *first,
            "undo makes the model it took back"
        );
        session.ok("ALIGN DESIGN road 0,11 100,11");
        assert_ne!(*bim_model(&session.document), *first, "the profile moved");
    }

    // The corridors are made again when the alignments' revision or the surfaces'
    // generation moves - a new alignment, a hidden surface - and the model is the
    // same object if it came out the same, so that a view holding it, and the 3D
    // views the road workbench hands it to, have nothing to do.
    #[test]
    fn the_road_model_is_the_same_object_after_an_edit_that_left_it_as_it_was() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        let first = bim_model(&session.document);
        assert!(
            !first.objects.is_empty(),
            "an empty model would be the same object anyway"
        );

        // A new alignment no corridor uses: the alignments' revision moves.
        let alignments = session.document.model().alignments.revision();
        session.ok("ALIGN NEW other 0,50 100,50");
        assert_ne!(
            session.document.model().alignments.revision(),
            alignments,
            "the key the corridors are made again on did not move"
        );
        assert!(Arc::ptr_eq(&bim_model(&session.document), &first));

        // A surface no corridor uses, added and then hidden: the surfaces'
        // generation moves each time.
        let tin = must(TinSurface::create(
            vec![
                Point3::new(0.0, 100.0, 1.0),
                Point3::new(10.0, 100.0, 2.0),
                Point3::new(10.0, 110.0, 3.0),
            ],
            vec![[0, 1, 2]],
            Vec::new(),
        ));
        let mut store = session.document.surfaces().clone();
        must(store.add(NamedSurface {
            name: String::from("unused"),
            surface: Some(Arc::new(tin)),
            source: String::from("test"),
            ..NamedSurface::default()
        }));
        let generation = session.document.surfaces_generation();
        must(session.document.set_surfaces(store.clone(), "TEST"));
        assert_ne!(session.document.surfaces_generation(), generation);
        assert!(
            Arc::ptr_eq(&bim_model(&session.document), &first),
            "a surface was added"
        );
        must(store.set_visible("unused", false));
        let shown = session.document.surfaces_generation();
        must(session.document.set_surfaces(store, "TEST"));
        assert_ne!(session.document.surfaces_generation(), shown);
        assert!(
            Arc::ptr_eq(&bim_model(&session.document), &first),
            "a surface was hidden"
        );

        // What the model is made from still makes a new one.
        session.ok("ROADTEMPLATE SET \"urban street\" lane width=3.6");
        assert!(!Arc::ptr_eq(&bim_model(&session.document), &first));
    }

    // A 3D view applies the layers it hides itself to the meshes it is handed
    // (RenderViewWidget::shownMeshes), so each mesh says which layer it is on.
    #[test]
    fn each_scene_mesh_of_the_road_model_names_the_layer_its_object_is_on() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        let model = bim_model(&session.document);
        let meshes = scene_meshes(&model, session.document.model(), &NO_LAYER_OVERRIDES);
        assert!(!meshes.is_empty());
        let mut at = 0;
        for object in &model.objects {
            if object.mesh.empty() {
                continue;
            }
            assert!(at < meshes.len());
            assert_eq!(meshes[at].layer, object.layer, "{}", object.id);
            assert!(!object.layer.is_empty(), "{} is on no layer", object.id);
            at += 1;
        }
        assert_eq!(at, meshes.len());
        // The layer hidden for the whole drawing takes its objects out of the list,
        // which is the drawing's rule; a view's own is the view's to apply.
        session.ok("LAYER HIDE road/kerb");
        let without = scene_meshes(&model, session.document.model(), &NO_LAYER_OVERRIDES);
        assert!(without.len() < meshes.len());
        for mesh in &without {
            assert_ne!(mesh.layer, "road/kerb");
        }
    }

    #[test]
    fn several_components_are_set_by_one_line_as_one_step_and_a_refused_value_changes_none() {
        fn width_of(session: &Session, side: Side, name: &str) -> f64 {
            component(session, "rural two-lane", side, name).width
        }
        let mut session = Session::default();
        session.ok("ROADTEMPLATE DEFAULTS");
        // The standard rural two-lane: a 3.5 m lane (Austroads) and a 2 m shoulder
        // each side (road_template.rs, default_component and the template itself).
        assert_eq!(width_of(&session, Side::Left, "lane"), 3.5);
        assert_eq!(width_of(&session, Side::Right, "lane"), 3.5);
        assert_eq!(width_of(&session, Side::Left, "shoulder"), 2.0);
        assert_eq!(width_of(&session, Side::Right, "shoulder"), 2.0);
        let before = session.steps();

        let reply = session.ok(
            "ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane width=3.75 \
             left:shoulder width=2.5 crossfall=-4",
        );
        assert_eq!(session.steps(), before + 1, "three components, one step");
        assert_eq!(width_of(&session, Side::Left, "lane"), 3.25);
        assert_eq!(width_of(&session, Side::Right, "lane"), 3.75);
        assert_eq!(width_of(&session, Side::Left, "shoulder"), 2.5);
        assert_eq!(
            width_of(&session, Side::Right, "shoulder"),
            2.0,
            "the right shoulder was not named"
        );
        assert!(contains(&reply, "side=left name=lane"), "{reply}");
        assert!(contains(&reply, "side=right name=lane"), "{reply}");
        assert!(contains(&reply, "side=left name=shoulder"), "{reply}");
        session.ok("UNDO");
        assert_eq!(width_of(&session, Side::Left, "lane"), 3.5);
        assert_eq!(width_of(&session, Side::Right, "lane"), 3.5);
        assert_eq!(width_of(&session, Side::Left, "shoulder"), 2.0);
        assert_eq!(session.steps(), before);

        // A value the template refuses in the LAST group leaves the first ones
        // undone: nothing is committed, so there is no step to take back.
        let zero = session
            .refused("ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane width=0");
        assert!(
            contains(&zero.describe(), "positive"),
            "{}",
            zero.describe()
        );
        assert_eq!(width_of(&session, Side::Left, "lane"), 3.5);
        assert_eq!(session.steps(), before);
        // An unknown component and a group with no key are refused as a whole too.
        session.refused(
            "ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 left:nothing width=1",
        );
        assert_eq!(width_of(&session, Side::Left, "lane"), 3.5);
        let bare =
            session.refused("ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane");
        assert!(
            contains(&bare.describe(), "needs key=value"),
            "{}",
            bare.describe()
        );
        assert_eq!(width_of(&session, Side::Left, "lane"), 3.5);
        assert_eq!(session.steps(), before);

        // One group is the line it always was.
        session.ok("ROADTEMPLATE SET \"rural two-lane\" lane width=3.4");
        assert_eq!(width_of(&session, Side::Left, "lane"), 3.4);
        assert_eq!(width_of(&session, Side::Right, "lane"), 3.4);
        assert_eq!(session.steps(), before + 1);

        // A group names its component as the groups before it left it, as the
        // same two lines typed one after the other would: the second finds the
        // lane the first renamed.
        session.ok(
            "ROADTEMPLATE SET \"rural two-lane\" left:lane name=carriageway \
             left:carriageway width=3.1",
        );
        let renamed = template(&session, "rural two-lane");
        assert!(renamed.find(Side::Left, "carriageway").is_some());
        assert_eq!(width_of(&session, Side::Left, "carriageway"), 3.1);
        assert!(renamed.find(Side::Left, "lane").is_none());
        assert_eq!(session.steps(), before + 2);
    }

    #[test]
    fn several_courses_are_set_by_one_line_as_one_step_and_a_refused_value_changes_none() {
        fn thickness_of(session: &Session, course: &str) -> f64 {
            let lane = component(session, "rural two-lane", Side::Left, "lane");
            lane.courses
                .iter()
                .find(|each| each.name == course)
                .map_or(-1.0, |each| each.thickness)
        }
        let mut session = Session::default();
        session.ok("ROADTEMPLATE DEFAULTS");
        // A lane's flexible pavement: 50 mm of AC14, 200 mm of base, 250 mm of
        // subbase (road_template.rs, default_component).
        assert_eq!(thickness_of(&session, "AC14"), 0.05);
        assert_eq!(thickness_of(&session, "base"), 0.2);
        assert_eq!(thickness_of(&session, "subbase"), 0.25);
        let before = session.steps();

        let reply = session.ok(
            "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 base \
             thickness=0.3 material=gravel",
        );
        assert_eq!(session.steps(), before + 1, "two courses, one step");
        assert_eq!(thickness_of(&session, "AC14"), 0.06);
        assert_eq!(thickness_of(&session, "base"), 0.3);
        assert_eq!(thickness_of(&session, "subbase"), 0.25, "not named");
        assert!(contains(&reply, "name=AC14"), "{reply}");
        assert!(contains(&reply, "name=base"), "{reply}");
        session.ok("UNDO");
        assert_eq!(thickness_of(&session, "AC14"), 0.05);
        assert_eq!(thickness_of(&session, "base"), 0.2);
        assert_eq!(session.steps(), before);

        let zero = session.refused(
            "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 base \
             thickness=0",
        );
        assert!(
            contains(&zero.describe(), "positive"),
            "{}",
            zero.describe()
        );
        assert_eq!(
            thickness_of(&session, "AC14"),
            0.05,
            "the first course was left as it was"
        );
        session.refused(
            "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 nothing \
             thickness=1",
        );
        assert_eq!(thickness_of(&session, "AC14"), 0.05);
        let bare = session.refused(
            "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 base",
        );
        assert!(
            contains(&bare.describe(), "needs key=value"),
            "{}",
            bare.describe()
        );
        assert_eq!(thickness_of(&session, "AC14"), 0.05);
        assert_eq!(session.steps(), before);

        // One course is the line it always was.
        session.ok("ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET subbase thickness=0.3");
        assert_eq!(thickness_of(&session, "subbase"), 0.3);
        assert_eq!(session.steps(), before + 1);
    }

    #[test]
    fn a_road_model_edit_is_reported_as_a_road_model_change_and_a_line_is_not() {
        let mut session = Session::default();
        let parts = Arc::new(Mutex::new(Vec::<u32>::new()));
        let heard = Arc::clone(&parts);
        let _handle = session
            .document
            .add_change_listener(move |change: &DocumentChange<'_>| {
                if let Ok(mut parts) = heard.lock() {
                    parts.push(change.parts);
                }
            });
        let last = || -> u32 {
            match parts.lock() {
                Ok(parts) => *parts.last().expect("a change was reported"),
                Err(error) => panic!("{error}"),
            }
        };
        session.ok("ROADTEMPLATE NEW t");
        assert!((last() & DocumentChange::ROAD_MODEL) != 0);
        assert!((last() & DocumentChange::HISTORY) != 0);
        session.ok("LINE 0,0 1,1");
        assert!((last() & DocumentChange::ROAD_MODEL) == 0);
        session.ok("UNDO");
        assert!((last() & DocumentChange::ROAD_MODEL) == 0);
        session.ok("UNDO");
        assert!(
            (last() & DocumentChange::ROAD_MODEL) != 0,
            "undoing the template"
        );
    }

    #[test]
    fn templates_and_corridors_survive_the_json_the_project_keeps() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"divided road\" from=10 to=90");
        {
            let keys = &session.document.metadata().unknown_keys;
            assert!(keys.contains_key("road_templates"));
            assert!(keys.contains_key("corridors"));
            let templates = must(road_templates_from_json(&keys["road_templates"]));
            assert_eq!(templates, *session.document.road_templates());
            let corridors = must(corridors_from_json(&keys["corridors"]));
            assert_eq!(corridors, *session.document.corridors());
            assert_eq!(corridors.corridors[0].start, Some(10.0));
        }
        // A project whose corridors a newer Katana wrote: kept, not overwritten.
        let mut metadata = session.document.metadata().clone();
        metadata.unknown_keys.insert(
            String::from("corridors"),
            String::from(r#"{"format":"katana-corridors","version":9,"corridors":[]}"#),
        );
        session.document.set_metadata(metadata);
        assert_eq!(
            session
                .document
                .corridors_status()
                .expect_err("a newer record is not read")
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            session.refused("CORRIDOR DELETE Main").code,
            ErrorCode::InvalidState
        );
    }

    #[test]
    fn help_lists_every_road_verb() {
        let mut session = Session::default();
        let help = session.ok("HELP CORRIDOR");
        for word in [
            "ROADTEMPLATE ADD",
            "CORRIDOR NEW",
            "CORRIDOR SUPERELEVATION",
            "CORRIDOR STRINGS",
            "CORRIDOR QUANTITIES",
            "ROADMODEL",
        ] {
            assert!(contains(&help, word), "{word}");
        }
        assert!(contains(&CommandInterpreter::help_text(), "ROADTEMPLATE"));
    }

    #[test]
    fn a_corridors_curve_tolerance_is_kept_set_and_refused_outside_its_range() {
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        let made =
            session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" curvetol=0.002");
        assert!(
            contains(
                &made,
                "interval=10 from= to= regions=0 superelevation=0 curvetol=0.002"
            ),
            "{made}"
        );
        assert_eq!(
            session.document.corridors().corridors[0].curve_tolerance,
            0.002
        );
        // A corridor that does not say has the default, written as it is.
        session.ok("CORRIDOR NEW Plain ALIGNMENT road TEMPLATE \"urban street\"");
        assert!(contains(
            &session.ok("CORRIDOR INFO Plain"),
            "curvetol=0.001"
        ));
        assert_eq!(DEFAULT_CURVE_TOLERANCE, 0.001);
        // 0 and off are the same: no section is added in a curve.
        let steps = session.steps();
        assert!(
            contains(&session.ok("CORRIDOR SET Main curvetol=off"), "curvetol=0 "),
            "off is written as the 0 it is"
        );
        assert!(contains(
            &session.ok("CORRIDOR SET Main curvetol=0.05"),
            "curvetol=0.05"
        ));
        assert!(contains(
            &session.ok("CORRIDOR SET Main curvetol=0"),
            "curvetol=0"
        ));
        assert_eq!(session.steps(), steps + 3, "each a step of its own");
        session.ok("CORRIDOR SET Main curvetol=0.1");
        assert_eq!(
            session.document.corridors().corridors[0].curve_tolerance,
            0.1
        );

        // The range is 0 to a tenth; a word that is not a number is a parse error.
        let before = session.steps();
        for line in [
            "CORRIDOR SET Main curvetol=-0.001",
            "CORRIDOR SET Main curvetol=0.1001",
            "CORRIDOR SET Main curvetol=1",
            "CORRIDOR NEW Bad ALIGNMENT road TEMPLATE \"urban street\" curvetol=2",
        ] {
            assert_eq!(
                session.refused(line).code,
                ErrorCode::InvalidArgument,
                "{line}"
            );
        }
        for line in [
            "CORRIDOR SET Main curvetol=abc",
            "CORRIDOR SET Main curvetol=",
            "CORRIDOR SET Main curvetol=nan",
            "CORRIDOR SET Main curvetol=inf",
        ] {
            assert_eq!(
                session.refused(line).code,
                ErrorCode::ParseFailure,
                "{line}"
            );
        }
        assert_eq!(session.steps(), before, "a refused value changes nothing");
        assert_eq!(
            session.document.corridors().corridors[0].curve_tolerance,
            0.1
        );
        assert!(contains(&session.ok("HELP CORRIDOR"), "curvetol="));
    }

    #[test]
    fn a_corridors_curve_tolerance_survives_the_projects_json_and_an_old_corridor_reads_as_the_default()
     {
        let mut corridor = Corridor {
            name: String::from("Main"),
            alignment: String::from("road"),
            template_name: String::from("t"),
            curve_tolerance: 0.0023,
            ..Corridor::default()
        };
        let mut set = CorridorSet {
            corridors: vec![corridor.clone()],
        };
        corridor.name = String::from("Other");
        corridor.curve_tolerance = 0.0;
        set.corridors.push(corridor);
        let text = must(corridors_to_json(&set));
        let back = must(corridors_from_json(&text));
        assert_eq!(back, set);
        assert_eq!(back.corridors[0].curve_tolerance, 0.0023);
        assert_eq!(
            back.corridors[1].curve_tolerance, 0.0,
            "0 is kept, not read as absent"
        );

        // A corridor saved before the tolerance existed has no member: the default.
        let old = must(corridors_from_json(concat!(
            r#"{"format":"katana-corridors","version":1,"corridors":["#,
            r#"{"name":"Old","alignment":"road","template":"t","interval":10}]}"#
        )));
        assert_eq!(old.corridors[0].curve_tolerance, DEFAULT_CURVE_TOLERANCE);
        // A member that is not a sensible tolerance is refused as a corridor is.
        let refused = |member: &str| -> Error {
            corridor_with(member).expect_err("the tolerance is refused")
        };
        assert_eq!(
            refused(r#""curveTolerance":5"#).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(r#""curveTolerance":-1"#).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(r#""curveTolerance":"fine""#).code,
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn a_curved_corridor_has_the_sections_its_tolerance_needs_and_a_straight_one_has_no_more() {
        // The quarter turn at 50 m (alignment A, 178.5398 m long): 20 stations of
        // the interval and the keys, and two more in each of the arc's eight gaps
        // (10 m is 0.2 radians, over the 0.0775 a tolerance of 0.001 allows, and
        // three parts do), 36. Off, the 20 alone. The straight road is 11 either way.
        let mut session = Session::default();
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("ALIGN NEW A 0,0 100,0 100,100");
        session.ok("ALIGN SET A 1 50");
        session.ok("ALIGN DESIGN A 0,10 200,10");
        let curved = session.ok("CORRIDOR NEW Bend ALIGNMENT A TEMPLATE \"urban street\"");
        assert!(contains(&curved, "runs=1 sections=36 "), "{curved}");
        assert!(contains(
            &session.ok("CORRIDOR BUILD Bend"),
            "runs=1 sections=36 "
        ));
        assert!(contains(
            &session.ok("CORRIDOR SET Bend curvetol=off"),
            "curvetol=0"
        ));
        assert!(contains(
            &session.ok("CORRIDOR BUILD Bend"),
            "runs=1 sections=20 "
        ));
        road(&mut session);
        for (which, tolerance) in (1..).zip(["curvetol=0.001", "curvetol=0"]) {
            let line = format!(
                "CORRIDOR NEW Road{which} ALIGNMENT road TEMPLATE \"urban street\" {tolerance}"
            );
            assert!(
                contains(&session.ok(&line), "runs=1 sections=11 "),
                "{tolerance}"
            );
        }
    }

    // ---- the road model's cache keys (docs/road_design.md, "The cache") ---------------------

    #[test]
    fn an_edit_of_a_surface_neither_a_corridor_targets_nor_an_object_names_makes_neither_half_again()
     {
        let mut s = CachedRoad::new();
        assert!(!s.first.objects.is_empty(), "an empty model would not tell");
        // "other" replaced by another surface (an edit of it), then hidden and
        // removed, and a new one added: the surfaces' generation moves each time, and
        // neither half reads any of them.
        replace_ground(&mut s.session, "other", 8.0);
        assert_eq!(
            made_since(&s.session, &s.counts),
            counts(0, 0),
            "an edit of a surface nothing names"
        );
        let mut store = s.session.document.surfaces().clone();
        must(store.set_visible("other", false));
        must(s.session.document.set_surfaces(store.clone(), "TEST"));
        assert!(store.remove("other"));
        must(s.session.document.set_surfaces(store, "TEST"));
        add_ground(&mut s.session, "another", 7.0);
        assert_eq!(
            made_since(&s.session, &s.counts),
            counts(0, 0),
            "hidden, removed and added"
        );
        // And nothing else in the drawing: a line, a layer, an undo of one.
        s.session.ok("LINE 0,50 10,50");
        s.session.ok("LAYER HIDE road/kerb");
        s.session.ok("UNDO");
        assert_eq!(
            made_since(&s.session, &s.counts),
            counts(0, 0),
            "a line, a layer, an undo"
        );
        assert!(Arc::ptr_eq(&bim_model(&s.session.document), &s.first));
    }

    #[test]
    fn an_edit_of_the_surface_a_corridor_targets_makes_the_corridors_again_and_not_the_placed_objects()
     {
        let mut s = CachedRoad::new();
        replace_ground(&mut s.session, "ground", 8.0);
        assert_eq!(made_since(&s.session, &s.counts), counts(1, 0));
        // The earthworks are the new ground's: the model is not the one it was.
        assert_ne!(*bim_model(&s.session.document), *s.first);
        // An undo puts the old surface object back, which is another change.
        let before_undo = bim_build_counts(&s.session.document);
        s.session.ok("UNDO");
        assert_eq!(made_since(&s.session, &before_undo), counts(1, 0));
        assert_eq!(*bim_model(&s.session.document), *s.first);
        // Removed, the corridor loses its ground (and says so): the corridors again.
        let before_removal = bim_build_counts(&s.session.document);
        let mut store = s.session.document.surfaces().clone();
        assert!(store.remove("ground"));
        must(s.session.document.set_surfaces(store, "TEST"));
        assert_eq!(made_since(&s.session, &before_removal), counts(1, 0));
    }

    #[test]
    fn an_edit_of_the_surface_a_placed_object_names_makes_the_placed_objects_again_and_not_the_corridors()
     {
        let mut s = CachedRoad::new();
        replace_ground(&mut s.session, "dem", 8.0);
        assert_eq!(made_since(&s.session, &s.counts), counts(0, 1));
        // The sign stands on the new surface: its model is not the one it was.
        assert_ne!(*bim_model(&s.session.document), *s.first);
    }

    #[test]
    fn each_definition_makes_only_the_half_it_is_part_of_and_an_alignment_both() {
        let mut s = CachedRoad::new();
        // A template: the corridors', not the placed objects'.
        s.session
            .ok("ROADTEMPLATE SET \"urban street\" lane width=3.6");
        assert_eq!(made_since(&s.session, &s.counts), counts(1, 0));
        // A placed object: the placed objects', not the corridors'.
        let before = bim_build_counts(&s.session.document);
        s.session
            .ok("ROADOBJECT ADD sign ALIGNMENT road station=60 offset=4");
        assert_eq!(made_since(&s.session, &before), counts(0, 1));
        // An alignment: both read the alignments, and it moves their revision.
        let before = bim_build_counts(&s.session.document);
        s.session.ok("ALIGN NEW elsewhere 0,500 100,500");
        assert_eq!(made_since(&s.session, &before), counts(1, 1));
        // The corridor's own definition: the corridors'.
        let before = bim_build_counts(&s.session.document);
        s.session.ok("CORRIDOR SET Main interval=20");
        assert_eq!(made_since(&s.session, &before), counts(1, 0));
        // Asking again when nothing moved makes nothing.
        let before = bim_build_counts(&s.session.document);
        assert_eq!(made_since(&s.session, &before), counts(0, 0));
    }

    // ---- template files: ROADTEMPLATE EXPORT and IMPORT ---------------------------------------

    #[test]
    fn templates_are_exported_to_a_file_the_form_roadtemplatejson_prints_and_imported_back_as_one_step()
     {
        let scratch = ScratchDirectory::new("road-templates-export");
        let one = scratch_file(&scratch, "export_one.json");
        let all = scratch_file(&scratch, "export_all.json");

        let mut source = Session::default();
        source.ok("ROADTEMPLATE DEFAULTS");
        source.ok("ROADTEMPLATE SET \"urban street\" lane width=3.6");
        let exported_one = source.ok(&format!(
            "ROADTEMPLATE EXPORT \"urban street\" {}",
            quoted(&one)
        ));
        assert!(contains(&exported_one, "exported file="), "{exported_one}");
        assert!(
            contains(
                &exported_one,
                " templates=1\ntemplate name=\"urban street\" left="
            ),
            "{exported_one}"
        );
        let exported_all = source.ok(&format!("ROADTEMPLATE EXPORT all {}", quoted(&all)));
        assert!(contains(&exported_all, " templates=3\n"), "{exported_all}");
        assert_eq!(
            source.steps(),
            2,
            "writing a file changes nothing in the drawing"
        );

        // The file is the plain-data, versioned JSON ROADTEMPLATE JSON prints,
        // and a newline.
        assert_eq!(
            read_text(&all),
            format!("{}\n", source.ok("ROADTEMPLATE JSON"))
        );
        assert!(read_file(&one).starts_with(br#"{"format":"katana-road-templates","#));
        assert!(contains(&read_text(&one), r#""version":1"#));

        // One template into another drawing: one step, exactly as it was (every
        // double written back as it was read), and an undo takes it away.
        let mut target = Session::default();
        let imported = target.ok(&format!("ROADTEMPLATE IMPORT {}", quoted(&one)));
        assert!(
            contains(
                &imported,
                " templates=1 added=1 replaced=0\ntemplate name=\"urban street\""
            ),
            "{imported}"
        );
        assert_eq!(target.steps(), 1);
        assert_eq!(target.document.road_templates().templates.len(), 1);
        assert_eq!(
            target.document.road_templates().templates[0],
            template(&source, "urban street")
        );
        target.ok("UNDO");
        assert!(target.document.road_templates().templates.is_empty());
        assert!(
            !target
                .document
                .metadata()
                .unknown_keys
                .contains_key("road_templates"),
            "the first template's undo leaves the metadata as it was"
        );

        // All three, in the file's order, into a third.
        let mut third = Session::default();
        let everything = third.ok(&format!("ROADTEMPLATE IMPORT {}", quoted(&all)));
        assert!(
            contains(&everything, " templates=3 added=3 replaced=0\n"),
            "{everything}"
        );
        assert_eq!(
            *third.document.road_templates(),
            *source.document.road_templates()
        );
        assert_eq!(third.steps(), 1);

        // And into a drawing that has the standard ones, as an update of them.
        third.ok("ROADTEMPLATE SET \"urban street\" lane width=3.0");
        assert_eq!(
            third
                .refused(&format!("ROADTEMPLATE IMPORT {}", quoted(&all)))
                .code,
            ErrorCode::AlreadyExists
        );
    }

    #[test]
    fn an_import_refuses_a_name_the_drawing_has_unless_overwrite_replaces_it_where_it_stands() {
        let scratch = ScratchDirectory::new("road-templates-overwrite");
        let file = scratch_file(&scratch, "overwrite.json");
        let mut session = Session::default();
        session.ok("ROADTEMPLATE DEFAULTS");
        // A second drawing with a new template and the urban street spelt in
        // capitals - the same name, as every verb finds a template ignoring case.
        let mut other = Session::default();
        other.ok("ROADTEMPLATE DEFAULTS");
        other.ok("ROADTEMPLATE RENAME \"urban street\" \"URBAN STREET\"");
        other.ok("ROADTEMPLATE SET \"URBAN STREET\" lane width=3.0");
        other.ok("ROADTEMPLATE NEW extra");
        other.ok(&format!("ROADTEMPLATE EXPORT ALL {}", quoted(&file)));

        let steps = session.steps();
        let before = (*session.document.road_templates()).clone();
        let refused = session.refused(&format!("ROADTEMPLATE IMPORT {}", quoted(&file)));
        assert_eq!(refused.code, ErrorCode::AlreadyExists);
        // All three names the file shares are said, and "extra", which does not
        // clash, is not imported either: nothing is.
        for name in ["rural two-lane", "URBAN STREET", "divided road"] {
            assert!(contains(&refused.message, name), "{}", refused.message);
        }
        assert!(
            contains(&refused.message, "OVERWRITE"),
            "{}",
            refused.message
        );
        assert_eq!(session.steps(), steps);
        assert_eq!(*session.document.road_templates(), before);

        let replaced = session.ok(&format!("ROADTEMPLATE IMPORT {} overwrite", quoted(&file)));
        assert!(
            contains(&replaced, " templates=4 added=1 replaced=3\n"),
            "{replaced}"
        );
        assert!(
            contains(
                &replaced,
                "template name=extra left=0 right=0 courses=0 description=\"\" added"
            ),
            "{replaced}"
        );
        assert!(
            contains(&replaced, "template name=\"URBAN STREET\""),
            "{replaced}"
        );
        assert_eq!(session.steps(), steps + 1, "one step for the whole file");
        // The three stand where they were, the new one last, and the replaced one
        // has the file's spelling and the file's lane.
        {
            let templates = session.document.road_templates();
            let set = &templates.templates;
            assert_eq!(set.len(), 4);
            assert_eq!(set[0].name, "rural two-lane");
            assert_eq!(set[1].name, "URBAN STREET");
            assert_eq!(set[2].name, "divided road");
            assert_eq!(set[3].name, "extra");
            let lane = set[1].find(Side::Left, "lane").expect("a left lane");
            assert_eq!(set[1].components[lane].width, 3.0);
        }
        session.ok("UNDO");
        assert_eq!(
            *session.document.road_templates(),
            before,
            "an undo puts all four back as they were"
        );
    }

    #[test]
    fn a_template_file_is_read_and_checked_as_the_project_reads_its_own_and_anything_else_is_refused()
     {
        let scratch = ScratchDirectory::new("road-templates-read");
        let file = scratch_file(&scratch, "read.json");
        let mut session = Session::default();
        let steps = session.steps();
        let head_text = r#"{"format":"katana-road-templates","version":1,"templates":["#;
        let lane = r##"{"name":"lane","kind":"lane","side":"left","layer":"road/pavement","colour":"#808080","width":"##;
        // A file that is not there, a folder, nothing, text, another sort of JSON.
        assert_eq!(
            session
                .refused(&format!(
                    "ROADTEMPLATE IMPORT {}",
                    quoted(&scratch_file(&scratch, "nothing.json"))
                ))
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session
                .refused(&format!("ROADTEMPLATE IMPORT {}", quoted(&scratch.path)))
                .code,
            ErrorCode::NotFound
        );
        refuse_import(
            &mut session,
            &file,
            b"",
            ErrorCode::ParseFailure,
            "road template file",
        );
        refuse_import(
            &mut session,
            &file,
            b"not json at all",
            ErrorCode::ParseFailure,
            "road template file",
        );
        refuse_import(
            &mut session,
            &file,
            br#"{"format":"katana-corridors","version":1,"corridors":[]}"#,
            ErrorCode::ParseFailure,
            "not a set of road templates",
        );
        // A newer Katana's file is not read by guesswork; a set of nothing says so.
        refuse_import(
            &mut session,
            &file,
            br#"{"format":"katana-road-templates","version":9,"templates":[]}"#,
            ErrorCode::Unsupported,
            "newer version",
        );
        refuse_import(
            &mut session,
            &file,
            format!("{head_text}]}}").as_bytes(),
            ErrorCode::InvalidArgument,
            "holds no road templates",
        );
        // The checks of ROADTEMPLATE SET, on every template: a lane of no width,
        // a name twice (ignoring case).
        refuse_import(
            &mut session,
            &file,
            format!(r#"{head_text}{{"name":"bad","description":"","components":[{lane}0}}]}}]}}"#)
                .as_bytes(),
            ErrorCode::InvalidArgument,
            "left:lane",
        );
        let good = format!(r#"{{"name":"a","description":"","components":[{lane}3.5}}]}}"#);
        let again = format!(r#"{{"name":"A","description":"","components":[{lane}3.5}}]}}"#);
        refuse_import(
            &mut session,
            &file,
            format!("{head_text}{good},{again}]}}").as_bytes(),
            ErrorCode::ParseFailure,
            "two road templates have one name",
        );

        // A good one is read, with a byte order mark in front as an editor writes
        // it; and a template of the same shape written by hand is the same as the
        // one a verb makes.
        let marked = [
            b"\xEF\xBB\xBF".as_slice(),
            format!("{head_text}{good}]}}").as_bytes(),
        ]
        .concat();
        write_file(&file, &marked);
        let read = session.ok(&format!("ROADTEMPLATE IMPORT {}", quoted(&file)));
        assert!(
            contains(
                &read,
                " templates=1 added=1 replaced=0\ntemplate name=a left=1 right=0 "
            ),
            "{read}"
        );
        assert_eq!(session.steps(), steps + 1);
    }

    #[test]
    fn an_export_writes_a_file_only_where_there_is_none_or_overwrite_says_so_and_refuses_what_it_cannot_write()
     {
        let scratch = ScratchDirectory::new("road-templates-write");
        let file = scratch_file(&scratch, "write.json");
        let mut session = Session::default();
        // Nothing to export from a drawing with no templates; nothing of a name it lacks.
        assert_eq!(
            session
                .refused(&format!("ROADTEMPLATE EXPORT ALL {}", quoted(&file)))
                .code,
            ErrorCode::InvalidState
        );
        session.ok("ROADTEMPLATE DEFAULTS");
        assert_eq!(
            session
                .refused(&format!("ROADTEMPLATE EXPORT nowhere {}", quoted(&file)))
                .code,
            ErrorCode::NotFound
        );
        assert!(!file.exists(), "a refusal writes nothing");
        // The words: a template and a file, then OVERWRITE and nothing else.
        for line in ["ROADTEMPLATE EXPORT", "ROADTEMPLATE EXPORT ALL"] {
            assert_eq!(
                session.refused(line).code,
                ErrorCode::ParseFailure,
                "{line}"
            );
        }
        assert_eq!(
            session
                .refused(&format!(
                    "ROADTEMPLATE EXPORT ALL {} REPLACE",
                    quoted(&file)
                ))
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session
                .refused(&format!(
                    "ROADTEMPLATE EXPORT ALL {} OVERWRITE now",
                    quoted(&file)
                ))
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session.refused("ROADTEMPLATE IMPORT").code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session
                .refused(&format!("ROADTEMPLATE IMPORT {} REPLACE", quoted(&file)))
                .code,
            ErrorCode::ParseFailure
        );

        session.ok(&format!(
            "ROADTEMPLATE EXPORT \"divided road\" {}",
            quoted(&file)
        ));
        let first = read_file(&file);
        // A file already there is not replaced without OVERWRITE, and is untouched.
        let refused = session.refused(&format!("ROADTEMPLATE EXPORT ALL {}", quoted(&file)));
        assert_eq!(refused.code, ErrorCode::AlreadyExists);
        assert!(
            contains(&refused.message, "OVERWRITE"),
            "{}",
            refused.message
        );
        assert_eq!(read_file(&file), first);
        session.ok(&format!(
            "ROADTEMPLATE EXPORT ALL {} OVERWRITE",
            quoted(&file)
        ));
        assert_ne!(read_file(&file), first);
        assert_eq!(
            read_text(&file),
            format!("{}\n", session.ok("ROADTEMPLATE JSON"))
        );
        // A place that cannot be written is a failure to export, not a crash.
        assert_eq!(
            session
                .refused(&format!(
                    "ROADTEMPLATE EXPORT ALL {}",
                    quoted(&scratch.path.join("katana_no_folder").join("templates.json"))
                ))
                .code,
            ErrorCode::FileExportFailure
        );
        assert_eq!(
            session
                .refused(&format!(
                    "ROADTEMPLATE EXPORT ALL {} OVERWRITE",
                    quoted(&scratch.path)
                ))
                .code,
            ErrorCode::FileExportFailure
        );
        assert!(contains(
            &session.ok("HELP CORRIDOR"),
            "ROADTEMPLATE IMPORT <file.json> [OVERWRITE]"
        ));
    }

    #[test]
    fn roadmodelshow_is_the_windows_alone_and_a_session_with_no_views_refuses_it_by_name() {
        // Show Road Model's line is taken by the window before the interpreter
        // (RoadWorkbench::runLine); here, with no views, it says whose it is and
        // where to hide the objects instead - not "unknown word", which would read as
        // a typing mistake.
        let mut session = Session::default();
        road(&mut session);
        session.ok("ROADTEMPLATE DEFAULTS");
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
        let steps = session.steps();
        for line in ["ROADMODEL SHOW", "ROADMODEL SHOW OFF", "roadmodel show on"] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::Unsupported, "{line}");
            assert!(
                contains(&error.message, "ROADMODEL SHOW is the desktop window's"),
                "{}",
                error.message
            );
            assert!(
                contains(&error.message, "LAYER HIDE road"),
                "{}",
                error.message
            );
        }
        assert_eq!(session.steps(), steps);
        // The rest of the verb is as it was, and a word it does not know is still a
        // mistake.
        assert!(contains(&session.ok("ROADMODEL"), "road model objects="));
        assert_eq!(
            session.refused("ROADMODEL HIDE").code,
            ErrorCode::ParseFailure
        );
        assert!(contains(
            &session.ok("HELP CORRIDOR"),
            "ROADMODEL SHOW [ON|OFF]"
        ));
    }
}
