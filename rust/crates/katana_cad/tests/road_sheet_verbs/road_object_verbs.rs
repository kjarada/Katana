// ROADOBJECT on the command line (tests/cad/road/test_road_object_verbs.cpp;
// road/road_object_verbs.rs), driven through CommandInterpreter::run on a
// Document as the window's command line, katana_cli and katana_mcp drive it:
// each word, its reply, its refusals, and that every edit is one undo step.
//
// The site: alignment Main, straight from (0,0) to (100,0) with a flat
// design profile at level 10, so station s, offset o is the point (s, o).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use katana_cad::road::road_object_verbs::road_objects_report;
use katana_cad::road::road_objects::{
    ROAD_OBJECT_KIND_COUNT, build_road_object, placement_context, road_objects_from_json,
};
use katana_cad::{CommandInterpreter, Document, DocumentChange};
use katana_core::ErrorCode;
use katana_geometry::{Point2, Point3};
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

use crate::{ScratchDirectory, Session, contains, core_lines, must, near, road_site};

fn site() -> Session {
    road_site(true, 0.0)
}

fn objects(site: &Session) -> usize {
    site.document.road_objects().objects.len()
}

fn has_layer(site: &Session, name: &str) -> bool {
    site.document.model().layers.contains(name)
}

// The ground z = 100 + 0.01 x over x -10..110, y -10..10 (two triangles).
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

// `name` on `surface`, from "test", in the drawing's default style.
fn named(name: &str, surface: &Arc<TinSurface>) -> NamedSurface {
    NamedSurface {
        name: String::from(name),
        surface: Some(Arc::clone(surface)),
        source: String::from("test"),
        ..NamedSurface::default()
    }
}

mod road_object_verbs {
    use super::*;

    #[test]
    fn kinds_lists_every_kind_and_one_kinds_keys_with_their_defaults() {
        let mut site = site();
        let kinds = core_lines(&site.ok("ROADOBJECT KINDS"));
        assert_eq!(kinds.len(), ROAD_OBJECT_KIND_COUNT);
        for line in &kinds {
            assert!(line.starts_with("kind word="), "{line}");
        }
        assert!(contains(
            &kinds[0],
            "kind word=barrier title=\"Safety barrier\" shape=linear"
        ));
        let barrier = site.ok("ROADOBJECT KINDS barriers");
        assert!(
            contains(
                &barrier,
                "parameter key=height type=number unit=m default=\"by type\""
            ),
            "{barrier}"
        );
        assert!(contains(
            &barrier,
            "choices=wbeam,thriebeam,fshape,singleslope,wirerope"
        ));
        assert_eq!(
            site.refused("ROADOBJECT KINDS bench").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn an_add_is_one_undo_step_together_with_the_layer_it_made() {
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        assert_eq!(objects(&site), 1);
        assert!(has_layer(&site, "road/objects/barrier"));
        site.ok("UNDO");
        assert_eq!(objects(&site), 0);
        assert!(!has_layer(&site, "road/objects/barrier"));
        site.ok("REDO");
        assert_eq!(objects(&site), 1);
        assert!(has_layer(&site, "road/objects/barrier"));
        // A second object on the layer makes no layer again.
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=40 to=60 offset=4");
        site.ok("UNDO");
        assert!(has_layer(&site, "road/objects/barrier"));
    }

    #[test]
    fn road_model_changes_are_reported_with_the_step_its_undo_and_its_redo() {
        let mut site = site();
        let reported = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&reported);
        let _handle = site
            .document
            .add_change_listener(move |change: &DocumentChange<'_>| {
                if change.has(DocumentChange::ROAD_MODEL) {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            });
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        assert_eq!(reported.load(Ordering::SeqCst), 1);
        site.ok("UNDO");
        assert_eq!(reported.load(Ordering::SeqCst), 2);
        site.ok("REDO");
        assert_eq!(reported.load(Ordering::SeqCst), 3);
        // A line that changes nothing reports nothing.
        site.ok("ROADOBJECT LIST");
        assert_eq!(reported.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_preview_says_what_would_be_added_and_changes_nothing() {
        let mut site = site();
        let revision = site.document.model_revision();
        let reply =
            site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4 PREVIEW");
        assert!(
            reply.starts_with("preview id=barrier-1 kind=barrier"),
            "{reply}"
        );
        assert!(contains(&reply, "length=20"), "{reply}");
        assert_eq!(objects(&site), 0);
        assert_eq!(site.document.model_revision(), revision);
        assert!(!has_layer(&site, "road/objects/barrier"));
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_and_not_refused() {
        let mut site = site();
        let reply = site.ok("ROADOBJECT ADD fence ALONG FROM DRAWING");
        assert!(contains(&reply, "scope=drawing matched=0"), "{reply}");
        assert!(contains(&reply, "added=0"), "{reply}");
        assert_eq!(objects(&site), 0);
    }

    #[test]
    fn along_a_scope_makes_one_object_per_line_in_one_step_and_counts_what_it_skipped() {
        let mut site = site();
        site.ok("LINE 0,20 10,20");
        site.ok("LINE 0,30 10,30");
        site.ok("POINT 5,25");
        site.ok("SELECT ALL");
        site.ok("PROP SET elevation 2.5");
        let reply = core_lines(&site.ok("ROADOBJECT ADD fence height=1.5 ALONG FROM DRAWING"));
        assert_eq!(reply.len(), 3);
        assert!(
            contains(
                &reply[0],
                "scope=drawing matched=3 lines=2 skipped=1 skippedtypes=point"
            ),
            "{}",
            reply[0]
        );
        assert!(
            contains(
                &reply[1],
                "added id=fence-1 kind=fence placement=line points=2 source=#1"
            ),
            "{}",
            reply[1]
        );
        assert!(contains(&reply[2], "added id=fence-2"), "{}", reply[2]);
        assert!(contains(&reply[2], "source=#2"), "{}", reply[2]);
        assert!(contains(&reply[2], "height=1.5"), "{}", reply[2]);
        assert_eq!(objects(&site), 2);
        site.ok("UNDO");
        assert_eq!(objects(&site), 0);
    }

    #[test]
    fn an_add_of_several_lines_is_all_or_nothing() {
        let mut site = site();
        site.ok("LINE 0,20 10,20");
        site.ok("LINE 0,30 10,30");
        site.ok("SELECT 1");
        site.ok("PROP SET elevation 2.5");
        // The second line has no heights, so its fence cannot stand anywhere.
        let error = site.refused("ROADOBJECT ADD fence ALONG FROM DRAWING");
        assert!(
            contains(&error.message, "no height"),
            "{}",
            error.describe()
        );
        assert!(contains(&error.context, "#2"), "{}", error.describe());
        assert_eq!(objects(&site), 0);
        // Given a surface it could stand on, or a line's own heights, it goes.
        site.ok("ROADOBJECT ADD fence ALONG #1");
        assert_eq!(objects(&site), 1);
    }

    #[test]
    fn along_something_that_is_not_a_line_is_refused() {
        let mut site = site();
        site.ok("POINT 1,1");
        let error = site.refused("ROADOBJECT ADD barrier ALONG #1");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            contains(&error.message, "#1 is a point, not a line"),
            "{}",
            error.describe()
        );
        assert_eq!(
            site.refused("ROADOBJECT ADD barrier ALONG #99").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn an_add_needs_one_placement_and_a_kind() {
        let mut site = site();
        assert_eq!(
            site.refused("ROADOBJECT ADD barrier").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            site.refused("ROADOBJECT ADD bench ALIGNMENT Main station=1")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            site.refused("ROADOBJECT ADD sign ALIGNMENT Main AT 1,1,1")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            site.refused("ROADOBJECT ADD sign ALIGNMENT Main station=50 plinth")
                .code,
            ErrorCode::ParseFailure
        );
        // A run of point objects needs both its end and its spacing.
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD light ALIGNMENT Main station=0 spacing=30")
                .message,
            "spacing= needs to="
        ));
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD light ALIGNMENT Main station=0 to=60")
                .message,
            "to= needs spacing="
        ));
        // A linear object needs its end; a culvert crosses at one station.
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD barrier ALIGNMENT Main station=0")
                .message,
            "to="
        ));
        assert!(contains(
            &site
                .refused("ROADOBJECT ADD culvert ALIGNMENT Main station=0 to=5")
                .message,
            "one station"
        ));
        assert_eq!(objects(&site), 0);
    }

    #[test]
    fn set_changes_keys_as_one_step() {
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        let reply = site.ok("ROADOBJECT SET barrier-1 type=fshape name=\"Median east\"");
        assert!(
            reply.starts_with("set id=barrier-1 kind=barrier name=\"Median east\""),
            "{reply}"
        );
        assert!(contains(&reply, "changed=type,name"), "{reply}");
        let info = site.ok("ROADOBJECT INFO barrier-1");
        assert!(
            contains(&info, "parameter key=type value=fshape given=yes"),
            "{info}"
        );
        assert!(
            contains(&info, "parameter key=height value=0.813 given=no unit=m"),
            "{info}"
        );
        site.ok("UNDO");
        assert!(
            site.document
                .road_objects()
                .find("barrier-1")
                .expect("barrier-1")
                .parameters
                .is_empty()
        );
    }

    #[test]
    fn set_refuses_what_the_kind_does_not_take_or_cannot_be_built_and_changes_nothing() {
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        let revision = site.document.model_revision();
        assert_eq!(
            site.refused("ROADOBJECT SET barrier-1 diameter=1").code,
            ErrorCode::NotFound
        );
        assert!(contains(
            &site.refused("ROADOBJECT SET barrier-1 to=200").message,
            "off alignment Main"
        ));
        assert_eq!(
            site.refused("ROADOBJECT SET barrier-9 to=10").code,
            ErrorCode::NotFound
        );
        assert_eq!(site.document.model_revision(), revision);
        assert_eq!(
            site.document
                .road_objects()
                .find("barrier-1")
                .expect("barrier-1")
                .placement
                .to,
            Some(20.0)
        );
    }

    #[test]
    fn a_copy_at_a_station_keeps_its_length() {
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=10 to=30 offset=4");
        let reply = site.ok("ROADOBJECT COPY barrier-1 station=50 offset=-4");
        assert!(contains(&reply, "copied id=barrier-2"), "{reply}");
        assert!(contains(&reply, "station=50 to=70 offset=-4"), "{reply}");
        assert!(contains(&reply, "from=barrier-1"), "{reply}");
        assert_eq!(objects(&site), 2);
    }

    #[test]
    fn a_copy_of_an_object_at_points_moves_by_dx_and_dy() {
        let mut site = site();
        site.ok("ROADOBJECT ADD sign AT 5,5,1");
        site.ok("ROADOBJECT COPY sign-1 dx=10 dy=-2");
        {
            let set = site.document.road_objects();
            let copy = set.find("sign-2").expect("sign-2");
            assert_eq!(copy.placement.vertices.len(), 1);
            assert_eq!(copy.placement.vertices[0].position, Point2::new(15.0, 3.0));
            assert_eq!(copy.placement.vertices[0].z, Some(1.0));
        }
        // dx= has no meaning along an alignment.
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        assert_eq!(
            site.refused("ROADOBJECT COPY sign-3 dx=1").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn delete_refuses_an_unknown_id_and_then_deletes_nothing() {
        let mut site = site();
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        assert_eq!(
            site.refused("ROADOBJECT DELETE sign-1 sign-7").code,
            ErrorCode::NotFound
        );
        assert_eq!(objects(&site), 1);
        assert_eq!(
            site.ok("ROADOBJECT DELETE sign-1 PREVIEW"),
            "preview delete count=1 ids=sign-1"
        );
        assert_eq!(objects(&site), 1);
        assert_eq!(
            site.ok("ROADOBJECT DELETE SIGN-1"),
            "deleted count=1 ids=sign-1"
        );
        assert_eq!(objects(&site), 0);
    }

    #[test]
    fn info_says_a_culverts_ends_and_its_solids() {
        // The skewed culvert of test_road_objects.cpp: inlet (55, -8.660254)
        // and outlet (45, 8.660254), at invert 8.4.
        let mut site = site();
        site.ok("ROADOBJECT ADD culvert ALIGNMENT Main station=50 skew=30");
        let info = site.ok("ROADOBJECT INFO culvert-1");
        assert!(
            contains(&info, "inlet=55,-8.660254,8.4 outlet=45,8.660254,8.4"),
            "{info}"
        );
        assert!(
            contains(
                &info,
                "part id=object/culvert-1/barrel class=IfcPipeSegment type=CULVERT"
            ),
            "{info}"
        );
        assert!(contains(&info, "closed=yes"), "{info}");
        assert!(!contains(&info, "closed=no"), "{info}");
        assert_eq!(
            site.refused("ROADOBJECT INFO culvert-2").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn list_json_is_the_stored_set_and_list_is_a_record_an_object() {
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        site.ok("ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6");
        let read = must(road_objects_from_json(&site.ok("ROADOBJECT LIST JSON")));
        assert_eq!(read, *site.document.road_objects());
        let listing = core_lines(&site.ok("ROADOBJECT LIST"));
        assert_eq!(listing.len(), 3);
        assert_eq!(listing[0], "roadobjects count=2 problems=0");
        assert!(
            listing[1].starts_with("object id=barrier-1"),
            "{}",
            listing[1]
        );
        // Three poles, at stations 0, 30 and 60, each a pole and a lantern: six
        // solids, but a count of three - the lanterns hang from the poles and are
        // not poles (this said 6, which counted each pole's lantern as well).
        assert!(contains(&listing[2], "objects=6"), "{}", listing[2]);
        assert!(contains(&listing[2], " count=3 "), "{}", listing[2]);
    }

    #[test]
    fn a_walls_record_and_total_give_its_length_once_with_its_footings_concrete() {
        // 50 m of the default wall (stem 0.25 x 2.0, footing 1.2 x 0.4): 50 m long
        // - the footing runs the same 50 m - with a face of 50 x 2 = 100 m2 and
        // 25 + 24 = 49 m3 of concrete. Two walls together are 100 m, not 150.
        let mut site = site();
        let added = site.ok("ROADOBJECT ADD wall ALIGNMENT Main station=10 to=60 offset=5");
        assert!(
            contains(
                &added,
                " objects=2 length=50 height=2 grossarea=100 grossvolume=49 width=1.2 depth=0.4"
            ),
            "{added}"
        );
        site.ok("ROADOBJECT ADD wall ALIGNMENT Main station=10 to=60 offset=-5 footing=0");
        let quantities = site.ok("ROADOBJECT QUANTITIES");
        assert!(
            contains(
                &quantities,
                "total kind=wall objects=2 length=100 height=2 grossarea=200 grossvolume=74 \
                 width=1.2 depth=0.4"
            ),
            "{quantities}"
        );
        // Each solid keeps its own: the footing says its length to IFC.
        assert!(
            contains(
                &quantities,
                "part=object/wall-1/footing class=IfcFooting type=STRIP_FOOTING length=50 \
                 width=1.2 depth=0.4 grossvolume=24"
            ),
            "{quantities}"
        );
    }

    #[test]
    fn quantities_are_given_per_solid_and_totalled_by_kind() {
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=30 to=50 offset=4");
        let reply = site.ok("ROADOBJECT QUANTITIES");
        assert!(
            contains(
                &reply,
                "quantity object=barrier-1 part=object/barrier-1 class=IfcRailing"
            ),
            "{reply}"
        );
        assert!(
            contains(&reply, "total kind=barrier objects=2 length=40"),
            "{reply}"
        );
        assert_eq!(
            site.refused("ROADOBJECT QUANTITIES nosuch").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn help_road_object_lists_every_word() {
        let mut site = site();
        let help = site.ok("HELP ROADOBJECT");
        for word in [
            "KINDS",
            "LIST",
            "INFO",
            "ADD",
            "SET",
            "COPY",
            "DELETE",
            "QUANTITIES",
            "PROBLEMS",
            "ALIGNMENT",
            "ALONG",
            "AT FROM",
        ] {
            assert!(contains(&help, word), "{word}");
        }
        assert!(contains(&CommandInterpreter::help_text(), "ROADOBJECT"));
    }

    #[test]
    fn the_objects_are_saved_with_the_project_and_opened_again() {
        let scratch = ScratchDirectory::new("road-objects-saved");
        let directory = scratch.path.join("saved.katana");
        let saved = {
            let mut site = site();
            site.ok(
                "ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4 type=thriebeam",
            );
            site.ok("ROADOBJECT ADD sign AT 5,5,1 code=R1-1");
            let saved = (*site.document.road_objects()).clone();
            must(site.document.save_as(&directory));
            saved
        };
        let mut opened = Document::default();
        must(opened.open(&directory));
        assert!(opened.road_objects_status().is_ok());
        assert_eq!(*opened.road_objects(), saved);
        // A new drawing has none.
        opened.new_document();
        assert!(opened.road_objects().objects.is_empty());
    }

    #[test]
    fn the_report_for_an_agent_holds_each_definition_and_what_was_made_of_it() {
        let mut site = site();
        site.ok("ROADOBJECT ADD culvert ALIGNMENT Main station=50");
        let report = road_objects_report(&site.document);
        assert!(
            contains(&report, "\"format\": \"katana-road-objects-report\""),
            "{report}"
        );
        assert!(contains(&report, "\"id\": \"culvert-1\""), "{report}");
        assert!(
            contains(&report, "\"ifc_class\": \"IfcPipeSegment\""),
            "{report}"
        );
        assert!(contains(&report, "\"closed\": true"), "{report}");
        assert!(!contains(&report, "\"closed\": false"), "{report}");
    }

    // ---- the findings of the review of 2026-10-15 --------------------------------------------

    #[test]
    fn an_undone_add_takes_away_the_parent_layers_it_made_and_keeps_those_that_were_there() {
        // "road/objects/barrier" is made with "road" and "road/objects"; the layer
        // table makes the parents of a layer it adds, and undoing the leaf alone
        // left them behind - layers nobody asked for.
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        assert!(has_layer(&site, "road"));
        assert!(has_layer(&site, "road/objects"));
        assert!(has_layer(&site, "road/objects/barrier"));
        site.ok("UNDO");
        assert!(!has_layer(&site, "road"));
        assert!(!has_layer(&site, "road/objects"));
        assert!(!has_layer(&site, "road/objects/barrier"));
        site.ok("REDO");
        assert!(has_layer(&site, "road"));
        assert!(has_layer(&site, "road/objects"));
        assert!(has_layer(&site, "road/objects/barrier"));
        site.ok("UNDO");

        // A layer of the person's own, three deep.
        site.ok(
            "ROADOBJECT ADD sign layer=signs/regulatory/stop ALIGNMENT Main station=50 offset=4",
        );
        assert!(has_layer(&site, "signs"));
        assert!(has_layer(&site, "signs/regulatory"));
        assert!(has_layer(&site, "signs/regulatory/stop"));
        site.ok("UNDO");
        assert!(!has_layer(&site, "signs"));
        assert!(!has_layer(&site, "signs/regulatory"));
        assert!(!has_layer(&site, "signs/regulatory/stop"));

        // A parent that was there is not the ADD's to take away.
        site.ok("LAYER NEW road");
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        assert!(has_layer(&site, "road/objects/barrier"));
        site.ok("UNDO");
        assert!(has_layer(&site, "road"));
        assert!(!has_layer(&site, "road/objects"));
        assert!(!has_layer(&site, "road/objects/barrier"));
    }

    #[test]
    fn a_copy_keeps_an_end_it_was_given_whichever_order_the_keys_are_typed_in() {
        // barrier-1 runs 10 to 60. station= alone moves the copy and keeps its
        // 50 m; a to= given is the end, before or after the station.
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=10 to=60 offset=4");
        let station = site.ok("ROADOBJECT COPY barrier-1 station=30 PREVIEW");
        assert!(contains(&station, "station=30 to=80"), "{station}");
        let end_first = site.ok("ROADOBJECT COPY barrier-1 to=50 station=30 PREVIEW");
        assert!(contains(&end_first, "station=30 to=50"), "{end_first}");
        assert!(contains(&end_first, "length=20"), "{end_first}");
        let end_last = site.ok("ROADOBJECT COPY barrier-1 station=30 to=50 PREVIEW");
        assert!(contains(&end_last, "station=30 to=50"), "{end_last}");
        assert!(contains(&end_last, "length=20"), "{end_last}");
        // An end before the new station is refused, whichever order it is typed in.
        assert!(contains(
            &site
                .refused("ROADOBJECT COPY barrier-1 to=20 station=30")
                .message,
            "must be past station="
        ));
        assert!(contains(
            &site
                .refused("ROADOBJECT COPY barrier-1 station=30 to=20")
                .message,
            "must be past station="
        ));
        assert_eq!(objects(&site), 1);
    }

    #[test]
    fn an_id_named_twice_is_one_object_in_quantities_and_twice_on_an_add_is_refused() {
        let mut site = site();
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=10 to=60 offset=4");
        // The 50 m barrier and its 28 posts once, however it is named.
        let once = site.ok("ROADOBJECT QUANTITIES barrier-1 BARRIER-1 barrier-1");
        assert!(
            contains(&once, "total kind=barrier objects=1 length=50 "),
            "{once}"
        );
        assert!(contains(&once, " count=28"), "{once}");

        site.ok("LINE 0,20 10,20");
        site.ok("SELECT 1");
        site.ok("PROP SET elevation 2.5");
        let error = site.refused("ROADOBJECT ADD fence ALONG #1 #1");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(contains(&error.message, "twice"), "{}", error.describe());
        assert_eq!(objects(&site), 1);
        site.ok("ROADOBJECT ADD fence ALONG #1");
        assert_eq!(objects(&site), 2);
    }

    #[test]
    fn a_word_list_is_not_given_is_refused_and_never_ignored_or_read_as_an_id() {
        // LIST used to answer a LAYERS scope with every object - it ignored any word
        // after it - and QUANTITIES and DELETE read one as an id that does not exist.
        // They take the scope now (test_road_object_scope.cpp); a word that is neither
        // a scope nor the verb's own is refused, naming it.
        let mut site = site();
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        let after_list = site.refused("ROADOBJECT LIST sign-1");
        assert_eq!(after_list.code, ErrorCode::InvalidArgument);
        assert_eq!(after_list.context, "sign-1");
        assert_eq!(
            site.refused("ROADOBJECT LIST JSON sign-1").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            site.refused("ROADOBJECT LIST JSON JSON").code,
            ErrorCode::InvalidArgument
        );
        // The scope is read and answers: the sign alone, not both.
        let layers = site.ok("ROADOBJECT LIST LAYERS road/objects/sign");
        assert!(contains(&layers, "matched=1"), "{layers}");
        assert!(contains(&layers, "object id=sign-1"), "{layers}");
        assert!(!contains(&layers, "barrier-2"), "{layers}");
        // Nothing was deleted by the refused lines, and the plain words go on.
        assert_eq!(objects(&site), 2);
        assert!(contains(
            &site.ok("ROADOBJECT LIST"),
            "roadobjects count=2 problems=0"
        ));
        assert_eq!(
            site.ok("ROADOBJECT DELETE ALL"),
            "deleted count=2 ids=sign-1,barrier-2"
        );
    }

    #[test]
    fn info_on_objects_a_newer_katana_wrote_says_so_and_not_that_the_object_is_missing() {
        let mut site = site();
        let mut metadata = site.document.metadata().clone();
        metadata.unknown_keys.insert(
            String::from("road_objects"),
            String::from(
                r#"{"format": "katana-road-objects", "version": 2, "next": 2, "objects": [{"id": "sign-1",
           "kind": "sign", "placement": {"kind": "alignment", "alignment": "Main", "station": 50}}]}"#,
            ),
        );
        site.document.set_metadata(metadata);
        let error = site.refused("ROADOBJECT INFO sign-1");
        assert_eq!(error.code, ErrorCode::Unsupported, "{}", error.describe());
        assert_eq!(site.refused("ROADOBJECT LIST").code, ErrorCode::Unsupported);
        assert_eq!(
            site.refused("ROADOBJECT QUANTITIES").code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn a_renamed_surface_is_followed_by_the_objects_and_the_corridors_that_stand_on_it() {
        // The ground z = 100 + 0.01 x over x -10..110, y -10..10 (two triangles),
        // as "ground" and as "dem". A corridor and a sign stand on "ground", and
        // a second sign on "dem".
        let shared = sloping_ground();
        let mut site = site();
        let mut store = SurfaceStore::new();
        must(store.add(named("ground", &shared)));
        must(store.add(named("dem", &shared)));
        must(site.document.set_surfaces(store, Document::SET_SURFACES));
        site.ok("ROADTEMPLATE DEFAULTS");
        site.ok("CORRIDOR NEW c1 ALIGNMENT Main TEMPLATE \"rural two-lane\" surface=ground");
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4 surface=ground");
        site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=60 offset=4 surface=dem");

        let renamed = site.ok("SURFACE RENAME ground eg");
        assert!(
            contains(
                &renamed,
                "renamed surface=ground name=eg corridors=1 roadobjects=1"
            ),
            "{renamed}"
        );
        assert_eq!(
            site.document.corridors().find("c1").expect("c1").surface,
            "eg"
        );
        assert_eq!(
            site.document
                .road_objects()
                .find("sign-1")
                .expect("sign-1")
                .placement
                .surface,
            "eg"
        );
        assert_eq!(
            site.document
                .road_objects()
                .find("sign-2")
                .expect("sign-2")
                .placement
                .surface,
            "dem"
        );
        // The sign still stands on its ground, and nothing is listed as a problem.
        assert_eq!(site.ok("ROADOBJECT PROBLEMS"), "problems=0");
        let made = build_road_object(
            &placement_context(&site.document),
            site.document.road_objects().find("sign-1").expect("sign-1"),
        );
        assert!(made.problems.is_empty());
        assert_eq!(made.objects.len(), 1);
        // (50, 4) on z = 100 + 0.01 x is 100.5.
        near(made.objects[0].mesh.bounds().min.z, 100.5, 1e-9);

        // The rename, the corridor and the objects are ONE undo step.
        site.ok("UNDO");
        assert!(site.document.surfaces().find("ground").is_some());
        assert!(site.document.surfaces().find("eg").is_none());
        assert_eq!(
            site.document.corridors().find("c1").expect("c1").surface,
            "ground"
        );
        assert_eq!(
            site.document
                .road_objects()
                .find("sign-1")
                .expect("sign-1")
                .placement
                .surface,
            "ground"
        );
        site.ok("REDO");
        assert_eq!(
            site.document.corridors().find("c1").expect("c1").surface,
            "eg"
        );
        assert_eq!(
            site.document
                .road_objects()
                .find("sign-1")
                .expect("sign-1")
                .placement
                .surface,
            "eg"
        );
    }

    #[test]
    fn a_renamed_surface_with_nothing_standing_on_it_renames_only_the_surface() {
        let mut site = site();
        let mut store = SurfaceStore::new();
        must(store.add(named("ground", &sloping_ground())));
        must(site.document.set_surfaces(store, Document::SET_SURFACES));
        let renamed = site.ok("SURFACE RENAME ground eg");
        assert!(
            contains(
                &renamed,
                "renamed surface=ground name=eg corridors=0 roadobjects=0"
            ),
            "{renamed}"
        );
        site.ok("UNDO");
        assert!(site.document.surfaces().find("ground").is_some());
    }
}
