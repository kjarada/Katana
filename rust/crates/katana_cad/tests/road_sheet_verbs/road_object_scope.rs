// The scope and filter of the ROADOBJECT verbs (tests/cad/road/
// test_road_object_scope.cpp; road/road_object_scope.rs, docs/road_objects.md,
// "Scope and filter"): LIST, QUANTITIES and DELETE take the shared words - the
// selection, a view, an AREA, named layers, the whole drawing, then WHERE -
// and the answer is what each means for a road object, which is a definition
// on a layer with a plan extent and not an entity. Driven through
// CommandInterpreter::run on a Document, as the window's command line,
// katana_cli and katana_mcp drive it.
//
// The site: alignment Main, straight from (0,0) to (100,0) with a flat design
// profile at level 10, so station s, offset o is the point (s, o). On it, in
// this order (the ids are the kind and the set's next number):
//
//   barrier-1   road/objects/barrier   W-beam, stations 0 to 20, offset 4
//   sign-2      road/objects/sign      at station 50, offset 4
//   light-3     lighting/poles        poles at stations 0, 30 and 60, offset 6
//   barrier-4   road/objects/barrier   thrie-beam, stations 70 to 90, offset -4
//
// A barrier's rail runs its two stations and its end posts are 100 mm wide, so
// barrier-1 lies within x from -0.05 to 20.05 (and barrier-4 from 69.95 to
// 90.05), and a pole, an arm of 1.5 m across the road and a lantern are all
// within a couple of metres of their station: the boxes below leave metres to
// spare on either side, which is why they are the numbers they are.

use katana_cad::road::road_object_scope::{check_filter_fits, filter_properties, plan_extent};
use katana_cad::road::road_objects::{colour_of, placement_context, road_objects_from_json};
use katana_cad::{LayerOverrides, ModifyFilter, ScopeView};
use katana_core::ErrorCode;
use katana_entity::entity::property_to_string;
use katana_entity::{EntityType, PropertyMap};
use katana_geometry::{Box2, Point2};

use crate::{Session, contains, core_lines, road_site};

// Main, and the four objects on it.
#[track_caller]
fn scoped_site() -> Session {
    let mut site = road_site(true, 0.0);
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    site.ok(
        "ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6 \
         layer=lighting/poles",
    );
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=70 to=90 offset=-4 type=thriebeam");
    site
}

// The window's view: id 3, hiding the sign layer of its own, seeing x from -5
// to 65 and y from -10 to 10.
fn show_view(site: &mut Session) {
    site.interpreter.set_scope_context(Some(Box::new(
        |_id: Option<u32>| -> katana_core::Result<ScopeView> {
            let mut layers = LayerOverrides::new();
            layers.hide("road/objects/sign");
            Ok(ScopeView {
                id: 3,
                layers,
                area: Some(Box2::new(Point2::new(-5.0, -10.0), Point2::new(65.0, 10.0))),
                ..ScopeView::default()
            })
        },
    )));
}

fn objects(site: &Session) -> usize {
    site.document.road_objects().objects.len()
}

// The ids of the `object id=` records of a LIST reply, in order, joined by ",".
fn ids_of(reply: &str) -> String {
    let mut out = String::new();
    for line in core_lines(reply) {
        if let Some(rest) = line.strip_prefix("object id=") {
            let id = rest.split(' ').next().unwrap_or_default();
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(id);
        }
    }
    out
}

// The ids a LIST of the drawing gives under `condition`.
#[track_caller]
fn ids_for(site: &mut Session, condition: &str) -> String {
    ids_of(&site.ok(&format!("ROADOBJECT LIST DRAWING WHERE {condition}")))
}

// A filter key's value as the property's text, or "<absent>".
fn text(map: &PropertyMap, key: &str) -> String {
    map.get(key)
        .map_or_else(|| String::from("<absent>"), property_to_string)
}

mod road_object_scope {
    use super::*;

    // ---- the scope -----------------------------------------------------------------------

    #[test]
    fn the_layers_a_scope_names_take_the_objects_on_them_with_or_without_sublayers() {
        let mut site = scoped_site();
        let sign = core_lines(&site.ok("ROADOBJECT LIST LAYERS road/objects/sign"));
        assert_eq!(sign.len(), 3);
        assert_eq!(
            sign[0],
            "scope=layers layers=road/objects/sign sublayers=yes matched=1"
        );
        assert_eq!(sign[1], "roadobjects count=1 problems=0");
        assert!(
            sign[2].starts_with("object id=sign-2 kind=sign "),
            "{}",
            sign[2]
        );

        // "road" and everything beneath it: the barriers and the sign, not the poles.
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST LAYERS road")),
            "barrier-1,sign-2,barrier-4"
        );
        // ONLY the layer itself, which holds none: reported, not refused.
        assert_eq!(
            site.ok("ROADOBJECT LIST LAYERS road ONLY"),
            "scope=layers layers=road sublayers=no matched=0\nroadobjects count=0 problems=0"
        );
        // Two layers, taken in the order the drawing keeps the objects.
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST LAYERS lighting,road/objects/sign")),
            "sign-2,light-3"
        );
        // The layer pattern of WHERE is the object's layer path, '*' and '?' wild
        // (and '*' runs over a '/'): "light*" is "lighting/poles".
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE LAYER=road/objects/s*")),
            "sign-2"
        );
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE LAYER=road/objects/s*,light*")),
            "sign-2,light-3"
        );
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE LAYER=lighting/?oles")),
            "light-3"
        );
        assert_eq!(
            site.refused("ROADOBJECT LIST LAYERS nosuch").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn an_area_takes_the_objects_whose_plan_extent_meets_it() {
        let mut site = scoped_site();
        // x from 10 to 25 meets the 20 m barrier (-0.05 to 20.05) and the row of
        // poles (light-3 is ONE object, its extent from the pole at 0 to the pole
        // at 60, so 10 to 25 lies along it though no pole stands there), not the
        // sign (50) nor the other barrier (70 to 90).
        let barrier = core_lines(&site.ok("ROADOBJECT LIST AREA 10,-10,25,10"));
        assert!(barrier.len() >= 3);
        assert_eq!(barrier[0], "scope=area area=10,-10,25,10 matched=2");
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST AREA 10,-10,25,10")),
            "barrier-1,light-3"
        );
        // The corners in either order are the same box, and the record says it sorted.
        assert_eq!(
            core_lines(&site.ok("ROADOBJECT LIST AREA 25,10,10,-10"))[0],
            "scope=area area=10,-10,25,10 matched=2"
        );
        // 45 to 55 meets the sign at 50 and, again, the row of poles along 0 to 60.
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST AREA 45,0,55,10")),
            "sign-2,light-3"
        );
        // The poles at 0, 30 and 60 are all in x from -5 to 65.
        let poles = site.ok("ROADOBJECT LIST AREA -5,-10,65,10 WHERE PROP=kind:light");
        assert_eq!(
            core_lines(&poles)[0],
            "scope=area area=-5,-10,65,10 where=\"PROP=kind:light\" matched=1"
        );
        assert_eq!(ids_of(&poles), "light-3");
        // Nothing there: reported.
        assert_eq!(
            site.ok("ROADOBJECT LIST AREA 200,200,300,300"),
            "scope=area area=200,200,300,300 matched=0\nroadobjects count=0 problems=0"
        );
        // Not four numbers is the parser's refusal.
        assert!(site.run("ROADOBJECT LIST AREA 1,2,3").is_err());
    }

    #[test]
    fn a_view_takes_the_objects_on_layers_it_shows_inside_its_visible_area() {
        let mut site = scoped_site();
        show_view(&mut site);
        // Hiding the sign layer of its own, seeing x from -5 to 65: barrier-1 and
        // the three poles (0, 30, 60); not the sign (hidden), not barrier-4 (from 70).
        let seen = site.ok("ROADOBJECT LIST VIEW");
        assert_eq!(
            core_lines(&seen)[0],
            "scope=view view=3 area=-5,-10,65,10 matched=2"
        );
        assert_eq!(ids_of(&seen), "barrier-1,light-3");
        // EXTENTS drops the area and keeps the layers: all but the sign, and the
        // record says what it was.
        let everywhere = site.ok("ROADOBJECT LIST VIEW 3 EXTENTS");
        assert_eq!(
            core_lines(&everywhere)[0],
            "scope=view view=3 extents=yes matched=3"
        );
        assert_eq!(ids_of(&everywhere), "barrier-1,light-3,barrier-4");
        // A layer the DRAWING hides is hidden in every view.
        site.ok("LAYER HIDE road/objects/barrier");
        assert_eq!(ids_of(&site.ok("ROADOBJECT LIST VIEW EXTENTS")), "light-3");
        // The quantities of that view are the poles' alone.
        let quantities = site.ok("ROADOBJECT QUANTITIES VIEW EXTENTS");
        assert!(
            contains(&quantities, "total kind=light objects=1"),
            "{quantities}"
        );
        assert!(!contains(&quantities, "kind=barrier"), "{quantities}");
    }

    #[test]
    fn a_view_with_no_window_is_refused_naming_the_area_that_says_the_same_without_one() {
        let mut site = scoped_site();
        let error = site.refused("ROADOBJECT LIST VIEW");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(contains(&error.message, "AREA"), "{}", error.describe());
        let steps = site.steps();
        assert_eq!(
            site.refused("ROADOBJECT DELETE VIEW").code,
            ErrorCode::InvalidState
        );
        assert_eq!(objects(&site), 4);
        assert_eq!(site.steps(), steps);
    }

    #[test]
    fn the_selection_takes_the_objects_placed_along_a_selected_line_and_no_others() {
        let mut site = scoped_site();
        site.ok("LINE 0,20 10,20");
        site.ok("LINE 0,30 10,30");
        site.ok("SELECT 1");
        site.ok("PROP SET elevation 2.5");
        site.ok("ROADOBJECT ADD fence ALONG #1"); // fence-5, source=#1
        site.ok("SELECT NONE");
        assert_eq!(
            site.ok("ROADOBJECT LIST SELECTION"),
            "scope=selection matched=0\nroadobjects count=0 problems=0"
        );
        site.ok("SELECT 1");
        let along = site.ok("ROADOBJECT LIST SELECTION");
        assert_eq!(core_lines(&along)[0], "scope=selection matched=1");
        assert_eq!(ids_of(&along), "fence-5");
        // Another line selected: no object was placed along it.
        site.ok("SELECT 2");
        assert_eq!(
            core_lines(&site.ok("ROADOBJECT LIST SELECTION"))[0],
            "scope=selection matched=0"
        );
        // Both: the objects along an alignment or at points have no entity to
        // select, so the four on Main are never taken by a selection.
        site.ok("SELECT ALL");
        assert_eq!(ids_of(&site.ok("ROADOBJECT LIST SELECTION")), "fence-5");
        // WHERE with no scope word is the selection, as it is everywhere.
        site.ok("SELECT 1");
        let filtered = site.ok("ROADOBJECT LIST WHERE PROP=kind:fence");
        assert_eq!(
            core_lines(&filtered)[0],
            "scope=selection where=\"PROP=kind:fence\" matched=1"
        );
        assert_eq!(ids_of(&filtered), "fence-5");
        assert_eq!(
            core_lines(&site.ok("ROADOBJECT LIST WHERE PROP=kind:barrier"))[0],
            "scope=selection where=\"PROP=kind:barrier\" matched=0"
        );
        // The line is the fence's source in its record.
        assert!(
            contains(&site.ok("ROADOBJECT LIST SELECTION"), "source=#1"),
            "the fence"
        );
    }

    // ---- the filter ----------------------------------------------------------------------

    #[test]
    fn where_reads_the_keys_set_writes_kind_type_id_and_placement() {
        let mut site = scoped_site();
        assert_eq!(
            ids_for(&mut site, "PROP=kind:barrier"),
            "barrier-1,barrier-4"
        );
        // The key in any case; a value is a pattern.
        assert_eq!(ids_for(&mut site, "PROP=Kind:SIGN"), "sign-2");
        assert_eq!(
            ids_for(&mut site, "PROP=id:barrier-*"),
            "barrier-1,barrier-4"
        );
        assert_eq!(ids_for(&mut site, "PROP=id:???ht-3"), "light-3");
        // The barrier's type: barrier-4 was given thriebeam, barrier-1 takes the
        // kind's default, which is a W-beam.
        assert_eq!(ids_for(&mut site, "PROP=type:thriebeam"), "barrier-4");
        assert_eq!(ids_for(&mut site, "PROP=type:wbeam"), "barrier-1");
        // A number as the reply prints it: offset 4 twice, -4, 6.
        assert_eq!(ids_for(&mut site, "PROP=offset:4"), "barrier-1,sign-2");
        assert_eq!(ids_for(&mut site, "PROP=offset:-4"), "barrier-4");
        assert_eq!(ids_for(&mut site, "PROP=offset:6"), "light-3");
        assert_eq!(ids_for(&mut site, "PROP=spacing:30"), "light-3");
        assert_eq!(ids_for(&mut site, "PROP=spacing"), "light-3"); // the key alone: any value
        assert_eq!(
            ids_for(&mut site, "PROP=heights:profile"),
            "barrier-1,sign-2,light-3,barrier-4"
        );
        assert_eq!(ids_for(&mut site, "PROP=placement:points"), "");
        // A value of any property: only barrier-4 says "thriebeam" anywhere.
        assert_eq!(ids_for(&mut site, "PROP=:thriebeam"), "barrier-4");
        // Both a layer and a property.
        assert_eq!(
            ids_for(&mut site, "LAYER=road/objects/barrier PROP=offset:4"),
            "barrier-1"
        );
        // The record says the filter as it was typed.
        assert_eq!(
            core_lines(&site.ok("ROADOBJECT LIST DRAWING WHERE PROP=Kind:SIGN"))[0],
            "scope=drawing where=\"PROP=Kind:SIGN\" matched=1"
        );
    }

    #[test]
    fn a_colour_filter_reads_the_colour_an_object_is_drawn_in_and_by_layer_means_none_of_its_own() {
        let mut site = scoped_site();
        site.ok("ROADOBJECT SET sign-2 colour=#FF0000");
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE COLOUR=#FF0000")),
            "sign-2"
        );
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE COLOUR=#00FF01")),
            ""
        );
        // The other three were never given a colour of their own.
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE COLOUR=ByLayer")),
            "barrier-1,light-3,barrier-4"
        );
        // The colour a kind is drawn in is a colour too.
        let colour = colour_of(
            site.document
                .road_objects()
                .find("barrier-1")
                .expect("barrier-1"),
        );
        assert_eq!(
            ids_of(&site.ok(&format!(
                "ROADOBJECT LIST DRAWING WHERE COLOUR={} PROP=kind:barrier",
                colour.to_hex()
            ))),
            "barrier-1,barrier-4"
        );
    }

    #[test]
    fn drawn_leaves_out_objects_on_a_layer_the_drawing_hides() {
        let mut site = scoped_site();
        site.ok("LAYER HIDE road/objects/sign");
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING")),
            "barrier-1,sign-2,light-3,barrier-4"
        );
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE DRAWN")),
            "barrier-1,light-3,barrier-4"
        );
        // A parent hidden hides what is beneath it.
        site.ok("LAYER HIDE road");
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST DRAWING WHERE DRAWN")),
            "light-3"
        );
    }

    #[test]
    fn type_style_and_text_are_about_entities_and_are_refused_for_a_road_object_by_name() {
        let mut site = scoped_site();
        for word in ["TYPE=point", "STYLE=thin", "TEXT=STOP"] {
            let error = site.refused(&format!("ROADOBJECT LIST DRAWING WHERE {word}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{word}");
            assert_eq!(
                error.context,
                word.split('=').next().unwrap_or_default(),
                "{}",
                error.describe()
            );
            assert!(!error.message.is_empty());
        }
        // Refused by every verb that takes a scope, with nothing changed.
        assert!(
            site.run("ROADOBJECT DELETE DRAWING WHERE TYPE=point")
                .is_err()
        );
        assert!(
            site.run("ROADOBJECT QUANTITIES DRAWING WHERE TEXT=a")
                .is_err()
        );
        assert_eq!(objects(&site), 4);
    }

    // ---- what the verbs do with what the scope took ----------------------------------------

    #[test]
    fn a_scope_that_takes_nothing_is_reported_by_list_quantities_and_delete_alike() {
        let mut site = scoped_site();
        site.ok("LAYER NEW empty");
        let steps = site.steps();
        let revision = site.document.model_revision();
        assert_eq!(
            site.ok("ROADOBJECT LIST LAYERS empty"),
            "scope=layers layers=empty sublayers=yes matched=0\nroadobjects count=0 problems=0"
        );
        assert_eq!(
            site.ok("ROADOBJECT QUANTITIES LAYERS empty"),
            "scope=layers layers=empty sublayers=yes matched=0\nroadobjects count=0"
        );
        assert_eq!(
            site.ok("ROADOBJECT DELETE LAYERS empty"),
            "scope=layers layers=empty sublayers=yes matched=0\ndeleted count=0"
        );
        // Nothing deleted is no undo step, and no change to the drawing.
        assert_eq!(site.steps(), steps);
        assert_eq!(site.document.model_revision(), revision);
        assert_eq!(objects(&site), 4);
    }

    #[test]
    fn a_delete_by_scope_is_one_undo_step_and_a_preview_changes_nothing() {
        let mut site = scoped_site();
        let steps = site.steps();
        let revision = site.document.model_revision();
        assert_eq!(
            site.ok("ROADOBJECT DELETE LAYERS road/objects/barrier PREVIEW"),
            "scope=layers layers=road/objects/barrier sublayers=yes matched=2\n\
             preview delete count=2 ids=barrier-1,barrier-4"
        );
        assert_eq!(site.document.model_revision(), revision);
        assert_eq!(objects(&site), 4);

        assert_eq!(
            site.ok("ROADOBJECT DELETE LAYERS road/objects/barrier"),
            "scope=layers layers=road/objects/barrier sublayers=yes matched=2\n\
             deleted count=2 ids=barrier-1,barrier-4"
        );
        assert_eq!(objects(&site), 2);
        assert_eq!(site.steps(), steps + 1);
        // One undo gives both back.
        site.ok("UNDO");
        assert_eq!(objects(&site), 4);
        assert!(site.document.road_objects().find("barrier-1").is_some());
        assert!(site.document.road_objects().find("barrier-4").is_some());

        // By a filter on the whole drawing; ALL then WHERE is the drawing's scope.
        assert_eq!(
            site.ok("ROADOBJECT DELETE ALL WHERE PROP=offset:4"),
            "scope=drawing where=\"PROP=offset:4\" matched=2\n\
             deleted count=2 ids=barrier-1,sign-2"
        );
        assert_eq!(objects(&site), 2);
        // ALL on its own is every object, with the plain reply it always had.
        assert_eq!(
            site.ok("ROADOBJECT DELETE ALL"),
            "deleted count=2 ids=light-3,barrier-4"
        );
        assert_eq!(objects(&site), 0);
    }

    #[test]
    fn quantities_of_a_scope_are_the_solids_of_the_objects_it_took_and_their_totals() {
        let mut site = scoped_site();
        // The two barriers are 20 m each: 20 m of W-beam and 20 m of thrie-beam.
        let quantities = core_lines(&site.ok("ROADOBJECT QUANTITIES LAYERS road/objects/barrier"));
        assert!(quantities.len() >= 4);
        assert_eq!(
            quantities[0],
            "scope=layers layers=road/objects/barrier sublayers=yes matched=2"
        );
        assert!(
            quantities[1].starts_with("quantity object=barrier-1 "),
            "{}",
            quantities[1]
        );
        let all = site.ok("ROADOBJECT QUANTITIES LAYERS road/objects/barrier");
        assert!(
            contains(&all, "total kind=barrier objects=2 length=40"),
            "{all}"
        );
        assert!(!contains(&all, "kind=sign"), "{all}");
        assert!(!contains(&all, "kind=light"), "{all}");
        // One kind by its key, over the drawing: only the lights.
        let lights = site.ok("ROADOBJECT QUANTITIES DRAWING WHERE PROP=kind:light");
        assert!(
            contains(
                &lights,
                "scope=drawing where=\"PROP=kind:light\" matched=1\n"
            ),
            "{lights}"
        );
        assert!(
            contains(&lights, "total kind=light objects=1 count=3"),
            "{lights}"
        );
    }

    #[test]
    fn objects_that_are_not_made_have_no_extent_and_are_counted_not_taken() {
        let mut site = scoped_site();
        site.ok("LINE 0,20 10,20");
        site.ok("SELECT 1");
        site.ok("PROP SET elevation 2.5");
        site.ok("ROADOBJECT ADD fence ALONG #1"); // fence-5: its line carries its heights
        site.ok("ALIGN DELETE Main"); // the four on it can no longer be made
        // The whole drawing still has all five; four of them are problems.
        assert_eq!(
            core_lines(&site.ok("ROADOBJECT LIST DRAWING"))[1],
            "roadobjects count=5 problems=4"
        );
        // A box round everything takes the one that is made, and counts the four
        // that would be taken if they were: they draw nothing, so they are nowhere.
        let area = site.ok("ROADOBJECT LIST AREA -1000,-1000,1000,1000");
        assert_eq!(
            core_lines(&area)[0],
            "scope=area area=-1000,-1000,1000,1000 matched=1 unmade=4"
        );
        assert_eq!(ids_of(&area), "fence-5");
        // The layers do not ask for an extent: all five are on road/objects/... or
        // lighting/poles still.
        assert_eq!(
            ids_of(&site.ok("ROADOBJECT LIST LAYERS road,lighting")),
            "barrier-1,sign-2,light-3,barrier-4,fence-5"
        );
        // DRAWN is about being drawn, so it leaves them out and counts them.
        let drawn = site.ok("ROADOBJECT LIST DRAWING WHERE DRAWN");
        assert_eq!(
            core_lines(&drawn)[0],
            "scope=drawing where=DRAWN matched=1 unmade=4"
        );
    }

    // ---- what is refused -------------------------------------------------------------------

    #[test]
    fn ids_and_a_scope_are_not_mixed_and_a_word_after_a_scope_is_not_ignored() {
        let mut site = scoped_site();
        let steps = site.steps();
        for line in [
            "ROADOBJECT DELETE sign-2 DRAWING",
            "ROADOBJECT DELETE sign-2 WHERE LAYER=x",
            "ROADOBJECT QUANTITIES sign-2 LAYERS road",
        ] {
            let error = site.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(
                contains(&error.message, "not both"),
                "{line}: {}",
                error.describe()
            );
        }
        // A word after a scope that is not the verb's own: refused, naming it.
        let after_list = site.refused("ROADOBJECT LIST DRAWING bogus");
        assert_eq!(after_list.code, ErrorCode::InvalidArgument);
        assert_eq!(after_list.context, "bogus");
        let after_quantities = site.refused("ROADOBJECT QUANTITIES DRAWING sign-2");
        assert_eq!(after_quantities.code, ErrorCode::ParseFailure);
        assert_eq!(after_quantities.context, "sign-2");
        let after_delete = site.refused("ROADOBJECT DELETE DRAWING PREVIEW now");
        assert_eq!(after_delete.code, ErrorCode::ParseFailure);
        assert_eq!(after_delete.context, "now");
        // PREVIEW before a scope would be read as a layer or an id: refused too.
        assert_eq!(
            site.refused("ROADOBJECT DELETE PREVIEW DRAWING").code,
            ErrorCode::InvalidArgument
        );
        // Plain ids after LIST are not ids: LIST takes none.
        assert_eq!(
            site.refused("ROADOBJECT LIST sign-2").code,
            ErrorCode::InvalidArgument
        );
        // Two scopes: the parser's refusal.
        assert!(site.run("ROADOBJECT LIST DRAWING SELECTION").is_err());
        assert_eq!(objects(&site), 4);
        assert_eq!(site.steps(), steps);
    }

    #[test]
    fn list_json_is_the_whole_stored_set_and_takes_no_scope() {
        let mut site = scoped_site();
        let error = site.refused("ROADOBJECT LIST JSON LAYERS road/objects/sign");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            contains(&error.message, "whole stored set"),
            "{}",
            error.describe()
        );
        // Plain, it is the JSON it was.
        let read = road_objects_from_json(&site.ok("ROADOBJECT LIST JSON"));
        let read = match read {
            Ok(read) => read,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(read, *site.document.road_objects());
    }

    #[test]
    fn a_record_the_drawing_cannot_read_is_said_so_and_not_reported_as_empty() {
        let mut site = scoped_site();
        let mut metadata = site.document.metadata().clone();
        metadata.unknown_keys.insert(
            String::from("road_objects"),
            String::from(
                r#"{"format": "katana-road-objects", "version": 2, "next": 2, "objects": []}"#,
            ),
        );
        site.document.set_metadata(metadata);
        assert_eq!(
            site.refused("ROADOBJECT LIST DRAWING").code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            site.refused("ROADOBJECT QUANTITIES LAYERS road").code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            site.refused("ROADOBJECT DELETE DRAWING").code,
            ErrorCode::CommandRejected
        );
    }

    // ---- the pieces ------------------------------------------------------------------------

    #[test]
    fn the_plan_extent_is_the_box_round_the_solids_and_nothing_for_an_object_that_is_not_made() {
        let mut site = scoped_site();
        {
            let context = placement_context(&site.document);
            let set = site.document.road_objects();
            // The 20 m barrier at offset 4: its length, and a metre of width at most.
            let barrier = plan_extent(&context, set.find("barrier-1").expect("barrier-1"))
                .expect("barrier-1 has an extent");
            assert!(barrier.width() > 19.9);
            assert!(barrier.width() < 20.5);
            assert!(barrier.height() < 1.0);
            assert!(barrier.min.y > 3.0);
            assert!(barrier.max.y < 5.0);
            // The sign: within a metre and a half of (50, 4).
            let sign = plan_extent(&context, set.find("sign-2").expect("sign-2"))
                .expect("sign-2 has an extent");
            assert!(sign.inflated(1.5).contains(Point2::new(50.0, 4.0)));
            assert!(sign.width() < 3.0);
            assert!(sign.height() < 3.0);
        }
        // An object whose alignment is gone is not made, and has no extent.
        site.ok("ALIGN DELETE Main");
        let context = placement_context(&site.document);
        let set = site.document.road_objects();
        assert!(plan_extent(&context, set.find("barrier-1").expect("barrier-1")).is_none());
    }

    #[test]
    fn the_filter_keys_are_the_keys_set_writes_with_the_values_the_object_is_made_with() {
        let mut site = scoped_site();
        let set = site.document.road_objects();

        let barrier = filter_properties(set.find("barrier-4").expect("barrier-4"));
        assert_eq!(text(&barrier, "id"), "barrier-4");
        assert_eq!(text(&barrier, "kind"), "barrier");
        assert_eq!(text(&barrier, "layer"), "road/objects/barrier");
        assert_eq!(text(&barrier, "placement"), "alignment");
        assert_eq!(text(&barrier, "alignment"), "Main");
        assert_eq!(text(&barrier, "station"), "70");
        assert_eq!(text(&barrier, "to"), "90");
        assert_eq!(text(&barrier, "offset"), "-4");
        assert_eq!(text(&barrier, "heights"), "profile");
        assert_eq!(text(&barrier, "crossfall"), "0");
        assert_eq!(text(&barrier, "dz"), "0");
        assert_eq!(text(&barrier, "type"), "thriebeam");
        // Not given, so absent: absent is not zero.
        assert_eq!(text(&barrier, "spacing"), "<absent>");
        assert_eq!(text(&barrier, "surface"), "<absent>");
        assert_eq!(text(&barrier, "name"), "<absent>");
        assert_eq!(text(&barrier, "facility"), "<absent>");
        assert_eq!(text(&barrier, "source"), "<absent>");
        // The default a barrier is made with, which no one typed.
        assert_eq!(
            text(
                &filter_properties(set.find("barrier-1").expect("barrier-1")),
                "type"
            ),
            "wbeam"
        );

        let lights = filter_properties(set.find("light-3").expect("light-3"));
        assert_eq!(text(&lights, "spacing"), "30");
        assert_eq!(text(&lights, "layer"), "lighting/poles");

        // An object placed along a line has its points and its source line.
        site.ok("LINE 0,20 10,20");
        site.ok("SELECT 1");
        site.ok("PROP SET elevation 2.5");
        site.ok("ROADOBJECT ADD fence ALONG #1 name=\"North fence\" facility=Main");
        let fence = filter_properties(
            site.document
                .road_objects()
                .find("fence-5")
                .expect("fence-5"),
        );
        assert_eq!(text(&fence, "placement"), "line");
        assert_eq!(text(&fence, "points"), "2");
        assert_eq!(text(&fence, "source"), "1");
        assert_eq!(text(&fence, "name"), "North fence");
        assert_eq!(text(&fence, "facility"), "Main");
        assert_eq!(text(&fence, "heights"), "line");
        assert_eq!(text(&fence, "alignment"), "<absent>");
    }

    #[test]
    fn an_unreachable_filter_is_checked_before_anything_is_looked_up() {
        assert!(check_filter_fits(&ModifyFilter::default()).is_ok());
        let mut filter = ModifyFilter {
            layers: vec![String::from("road/*")],
            property: Some(String::from("kind")),
            drawn_only: true,
            ..ModifyFilter::default()
        };
        assert!(check_filter_fits(&filter).is_ok());
        filter.types.insert(EntityType::Point);
        let types = check_filter_fits(&filter).expect_err("TYPE= is refused");
        assert_eq!(types.context, "TYPE");
        filter.types.clear();
        filter.style = Some(String::new());
        assert_eq!(
            check_filter_fits(&filter)
                .expect_err("STYLE= is refused")
                .context,
            "STYLE"
        );
        filter.style = None;
        filter.text = Some(String::new());
        assert_eq!(
            check_filter_fits(&filter)
                .expect_err("TEXT= is refused")
                .context,
            "TEXT"
        );
    }
}
