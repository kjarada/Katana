// SURFACE EDIT, MERGE, PASTE, CLIP and OFFSET through the command interpreter
// (terrain/surface_edit_verbs.rs, docs/surface_editing.md;
// tests/cad/terrain/test_surface_edit_verbs.cpp): the drawing's surfaces
// edited as one undo step each, the replies, and every refusal.

mod surface_edit_verbs {
    use katana_cad::terrain::surface_edit_verbs::{
        active_edit_surface, set_surface_edit_target, surface_name_word,
    };
    use katana_cad::{CommandInterpreter, Document};
    use katana_commands::{EntityAttributes, create_line, create_point, create_polyline};
    use katana_core::ErrorCode;
    use katana_core::text::format_exact_real;
    use katana_entity::EntityId;
    use katana_geometry::{Point2, Point3, Polyline2};

    use crate::surface_edit_support::{
        flat_grid_surface, grid_surface, make_surface, put_surface, pyramid_surface,
        square_surface, surface_of,
    };
    use crate::surface_geometry::same_surface_geometry;
    use crate::{Session, contains, double_eq, level, must, near, ok};

    fn p(x: f64, y: f64) -> Point2 {
        Point2::new(x, y)
    }

    // A polyline on `layer`, made by its command; its id (0 when none was
    // made).
    fn polyline(
        session: &mut Session,
        vertices: Vec<Point2>,
        closed: bool,
        layer: &str,
    ) -> EntityId {
        ok(session.document.execute(create_polyline(
            Polyline2::new(vertices, closed),
            EntityAttributes {
                layer: String::from(layer),
                style: String::new(),
                color: None,
            },
        )));
        session
            .document
            .last_created_entities()
            .first()
            .copied()
            .unwrap_or(0)
    }

    // The whole number a reply gives for `key`: "points_added=4" is 4.
    #[track_caller]
    fn field(text: &str, key: &str) -> usize {
        let Some(at) = text.find(&format!(" {key}=")) else {
            panic!("{key} in {text}");
        };
        let rest = &text[at + key.len() + 2..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        match rest[..end].parse() {
            Ok(value) => value,
            Err(_) => panic!("{key} in {text} is not a whole number"),
        }
    }

    #[test]
    fn adding_a_point_is_one_undo_step_and_says_what_changed() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        let before = surface_of(&t.document, "ground").clone();
        let text = t.ok("SURFACE EDIT ground ADD POINT 5,5,10");
        assert!(
            contains(
                &text,
                "surface name=ground operation=add_point triangles_before=2 triangles_after=4 \
                 vertices_before=4 vertices_after=5 points_added=1"
            ),
            "{text}"
        );
        assert!(contains(&text, "changed=yes"), "{text}");
        assert_eq!(surface_of(&t.document, "ground").triangle_count(), 4);
        double_eq(level(surface_of(&t.document, "ground"), 5.0, 5.0), 10.0);

        t.ok("UNDO");
        assert!(*surface_of(&t.document, "ground") == before);
        t.ok("REDO");
        assert_eq!(surface_of(&t.document, "ground").triangle_count(), 4);
    }

    #[test]
    fn a_preview_works_the_edit_out_and_changes_nothing() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        let generation = t.document.surfaces_generation();
        let text = t.ok("SURFACE EDIT ground ADD POINT 5,5,10 PREVIEW");
        assert!(contains(&text, "triangles_after=4"), "{text}");
        assert!(contains(&text, "preview=yes"), "{text}");
        assert_eq!(surface_of(&t.document, "ground").triangle_count(), 2);
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn an_edit_that_changes_nothing_is_no_step() {
        // Unit cells have no side longer than sqrt 2 < 1.5: nothing to delete.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        let generation = t.document.surfaces_generation();
        let text = t.ok("SURFACE EDIT ground DELETE LONG 1.5");
        assert!(contains(&text, "changed=no"), "{text}");
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn flip_takes_the_edge_nearest_the_point() {
        // Corners 1 and 3 at 1: after the flip the centre lies on the diagonal
        // between them, at 1.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 1.0, 0.0, 1.0),
            true,
        );
        t.ok("SURFACE EDIT ground FLIP 5,5");
        near(
            level(surface_of(&t.document, "ground"), 5.0, 5.0),
            1.0,
            1e-12,
        );
    }

    #[test]
    fn a_hole_from_a_closed_polyline_by_id_removes_its_area() {
        // A 10 x 10 grid of unit cells less [3,6] x [3,7]: 100 - 12 = 88.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 4.0, 0.0, 0.0),
            true,
        );
        let id = polyline(
            &mut t,
            vec![p(3.0, 3.0), p(6.0, 3.0), p(6.0, 7.0), p(3.0, 7.0)],
            true,
            "0",
        );
        t.ok(&format!("SURFACE EDIT ground HOLE #{id}"));
        near(surface_of(&t.document, "ground").plan_area(), 88.0, 1e-9);
    }

    #[test]
    fn from_with_no_scope_word_is_the_selection_as_the_scope_grammar_has_it() {
        // The same hole, the polyline taken from the selection: 88.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 4.0, 0.0, 0.0),
            true,
        );
        let id = polyline(
            &mut t,
            vec![p(3.0, 3.0), p(6.0, 3.0), p(6.0, 7.0), p(3.0, 7.0)],
            true,
            "0",
        );
        t.document.selection_mut().set(vec![id]);
        let text = t.ok("SURFACE EDIT ground HOLE FROM");
        assert!(
            text.starts_with("scope=selection matched=1 polygons=1 skipped=0\n"),
            "{text}"
        );
        near(surface_of(&t.document, "ground").plan_area(), 88.0, 1e-9);
    }

    #[test]
    fn a_typed_polygon_closed_on_its_first_corner_is_that_polygon() {
        // [3,6] x [3,7] typed back to its first corner: the same 12 out, 88
        // left.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 4.0, 0.0, 0.0),
            true,
        );
        t.ok("SURFACE EDIT ground HOLE 3,3 6,3 6,7 3,7 3,3");
        near(surface_of(&t.document, "ground").plan_area(), 88.0, 1e-9);
    }

    #[test]
    fn holes_from_a_scope_of_many_footprints_make_what_one_line_per_footprint_makes() {
        // 12 footprints in the 10 m cells of a 4 x 3 lattice on a 40 x 40 m
        // ground (1600 m2), [10 i + 2.3, 10 i + 6.0] x [10 j + 3.1, 10 j + 6.0]:
        // 3.7 x 2.9 = 10.73 m2 each, 128.76 together, 1471.24 left. Cut
        // together they are one patch of the ground (HOLE FROM LAYERS ...);
        // typed one line at a time they are twelve - and the same surface, the
        // same counts, and one undo step.
        let level_of = |x: f64, y: f64| 0.5 * x + 0.25 * y;
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            grid_surface(40, 1.0, level_of, 0.0, 0.0),
            true,
        );
        let before = surface_of(&t.document, "ground").clone();
        let mut typed = Document::default();
        let mut typed_lines = CommandInterpreter::new();
        put_surface(
            &mut typed,
            "ground",
            grid_surface(40, 1.0, level_of, 0.0, 0.0),
            true,
        );
        t.ok("LAYER NEW foot");
        t.ok("LAYER SET foot");
        let mut added = 0;
        let mut removed = 0;
        let mut crossings = 0;
        let mut first_typed = String::new();
        let mut last_typed = String::new();
        for j in 0..3 {
            for i in 0..4 {
                // Tenths, divided once, so the number is the nearest double to
                // the decimal.
                let x0 = f64::from(100 * i + 23) / 10.0;
                let y0 = f64::from(100 * j + 31) / 10.0;
                let x1 = f64::from(100 * i + 60) / 10.0;
                let y1 = f64::from(100 * j + 60) / 10.0;
                polyline(
                    &mut t,
                    vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)],
                    true,
                    "foot",
                );
                let at = format_exact_real;
                let line = format!(
                    "SURFACE EDIT ground HOLE {},{} {},{} {},{} {},{}",
                    at(x0),
                    at(y0),
                    at(x1),
                    at(y0),
                    at(x1),
                    at(y1),
                    at(x0),
                    at(y1)
                );
                let one = must(typed_lines.run(&mut typed, &line));
                added += field(&one, "points_added");
                removed += field(&one, "points_removed");
                crossings += field(&one, "crossings");
                if first_typed.is_empty() {
                    first_typed = one;
                } else {
                    last_typed = one;
                }
            }
        }
        let text = t.ok("SURFACE EDIT ground HOLE FROM LAYERS foot ONLY");
        assert!(contains(&text, "polygons=12"), "{text}");
        near(
            surface_of(&t.document, "ground").plan_area(),
            1600.0 - 128.76,
            1e-8,
        );
        assert_eq!(
            same_surface_geometry(
                surface_of(&t.document, "ground"),
                surface_of(&typed, "ground")
            ),
            Ok(())
        );
        // The reply is the twelve replies summed: the first's "before", the
        // last's "after", the counts of all.
        assert_eq!(
            field(&text, "triangles_before"),
            field(&first_typed, "triangles_before")
        );
        assert_eq!(
            field(&text, "triangles_after"),
            field(&last_typed, "triangles_after")
        );
        assert_eq!(
            field(&text, "vertices_before"),
            field(&first_typed, "vertices_before")
        );
        assert_eq!(
            field(&text, "vertices_after"),
            field(&last_typed, "vertices_after")
        );
        assert_eq!(field(&text, "points_added"), added);
        assert_eq!(field(&text, "points_removed"), removed);
        assert_eq!(field(&text, "crossings"), crossings);
        assert!(added > 0);
        // One undo step for the lot.
        t.ok("UNDO");
        assert!(*surface_of(&t.document, "ground") == before);
    }

    #[test]
    fn a_boundary_from_a_scope_says_what_the_scope_took() {
        // The pyramid clipped to [2,8]^2: 36. The scope takes the polyline and
        // a line, which is no polygon and is counted as skipped.
        let mut t = Session::default();
        put_surface(&mut t.document, "ground", pyramid_surface(10.0), true);
        polyline(
            &mut t,
            vec![p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0), p(2.0, 8.0)],
            true,
            "0",
        );
        ok(t.document.execute(create_line(
            p(20.0, 20.0),
            p(30.0, 30.0),
            EntityAttributes::default(),
        )));
        let text = t.ok("SURFACE EDIT ground BOUNDARY FROM DRAWING");
        assert!(
            text.starts_with("scope=drawing matched=2 polygons=1 skipped=1\n"),
            "{text}"
        );
        near(surface_of(&t.document, "ground").plan_area(), 36.0, 1e-9);
    }

    #[test]
    fn a_draped_breakline_typed_in_counts_its_crossings() {
        // y = 2.5 crosses the spokes from (0,0) and (10,0) to the apex.
        let mut t = Session::default();
        put_surface(&mut t.document, "ground", pyramid_surface(10.0), true);
        let text = t.ok("SURFACE EDIT ground BREAKLINE 0,2.5 10,2.5");
        assert!(contains(&text, "crossings=2"), "{text}");
        near(
            level(surface_of(&t.document, "ground"), 2.5, 2.5),
            5.0,
            1e-12,
        );
    }

    #[test]
    fn raise_inside_a_typed_polygon_moves_only_the_vertices_inside() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        let text = t.ok("SURFACE EDIT ground RAISE 1.5 INSIDE 0.5,0.5 2.5,0.5 2.5,2.5 0.5,2.5");
        assert!(contains(&text, "moved=4"), "{text}");
        assert!(contains(&text, "largest_change=1.5"), "{text}");
        double_eq(level(surface_of(&t.document, "ground"), 1.0, 1.0), 1.5);
        double_eq(level(surface_of(&t.document, "ground"), 3.0, 3.0), 0.0);
    }

    #[test]
    fn lower_set_level_and_scale_change_levels_as_they_name_it() {
        let mut t = Session::default();
        put_surface(&mut t.document, "ground", pyramid_surface(10.0), true);
        t.ok("SURFACE EDIT ground LOWER 2");
        double_eq(level(surface_of(&t.document, "ground"), 5.0, 5.0), 8.0);
        t.ok("SURFACE EDIT ground SCALE 2 base=-2");
        double_eq(level(surface_of(&t.document, "ground"), 5.0, 5.0), 18.0); // -2 + 2 (8 + 2)
        t.ok("SURFACE EDIT ground SETLEVEL 3");
        double_eq(level(surface_of(&t.document, "ground"), 5.0, 5.0), 3.0);
        double_eq(level(surface_of(&t.document, "ground"), 0.0, 0.0), 3.0);
    }

    #[test]
    fn smooth_takes_its_passes_and_weight() {
        // The apex's neighbours are the four corners at 0, held on the rim.
        let mut t = Session::default();
        put_surface(&mut t.document, "ground", pyramid_surface(10.0), true);
        t.ok("SURFACE EDIT ground SMOOTH passes=1 weight=1");
        double_eq(level(surface_of(&t.document, "ground"), 5.0, 5.0), 0.0);
    }

    #[test]
    fn thin_with_tolerance_zero_removes_the_points_that_change_nothing() {
        // tin_edit's case: a plane with one vertex raised; (1,3) and (3,1) go.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            grid_surface(
                4,
                1.0,
                |x, y| 0.5 * x + 0.25 * y + if x == 2.0 && y == 2.0 { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ),
            true,
        );
        let text = t.ok("SURFACE EDIT ground THIN 0");
        assert!(contains(&text, "points_removed=2"), "{text}");
    }

    #[test]
    fn delete_point_and_triangles_take_their_places() {
        let mut t = Session::default();
        put_surface(&mut t.document, "ground", pyramid_surface(10.0), true);
        t.ok("SURFACE EDIT ground DELETE POINT 5.1,5.1 reach=0.5");
        assert_eq!(surface_of(&t.document, "ground").triangle_count(), 2);
        put_surface(
            &mut t.document,
            "cells",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        t.ok("SURFACE EDIT cells DELETE TRIANGLE 0.6,0.2");
        assert_eq!(surface_of(&t.document, "cells").triangle_count(), 31);
        t.ok("SURFACE EDIT cells DELETE TRIANGLES OUTSIDE 0,0 2,0 2,2 0,2");
        near(surface_of(&t.document, "cells").plan_area(), 3.5, 1e-12);
    }

    #[test]
    fn paste_merge_clip_and_offset_make_surfaces_by_name() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 10.0, 10.0, 0.0, 0.0),
            true,
        );
        put_surface(
            &mut t.document,
            "pad",
            flat_grid_surface(2, 10.0, 15.0, 40.0, 40.0),
            true,
        );

        t.ok("SURFACE PASTE pad INTO ground NAME combined");
        let Some(combined) = t.document.surfaces().find("combined") else {
            panic!("no surface combined");
        };
        assert_eq!(combined.source, "paste of pad into ground");
        let Some(combined_surface) = combined.surface.as_deref() else {
            panic!("combined has no triangulation");
        };
        double_eq(level(combined_surface, 50.0, 50.0), 15.0);
        double_eq(level(surface_of(&t.document, "ground"), 50.0, 50.0), 10.0);

        t.ok("SURFACE OFFSET ground 2 NAME raised");
        double_eq(level(surface_of(&t.document, "raised"), 50.0, 50.0), 12.0);

        t.ok("SURFACE CLIP ground 0,0 20,0 20,20 0,20 NAME corner");
        near(surface_of(&t.document, "corner").plan_area(), 400.0, 1e-9);

        t.ok("SURFACE MERGE ground pad NAME merged");
        double_eq(level(surface_of(&t.document, "merged"), 50.0, 50.0), 15.0);

        // In place, the base keeps its name and its place.
        let count = t.document.surfaces().all().len();
        t.ok("SURFACE PASTE pad INTO ground");
        assert_eq!(t.document.surfaces().all().len(), count);
        double_eq(level(surface_of(&t.document, "ground"), 50.0, 50.0), 15.0);
    }

    #[test]
    fn a_paste_of_a_top_with_two_rim_vertices_inside_the_tolerance_is_refused_saying_so() {
        // The rectangle [2.3, 12.3] x [3.7, 13.7] with two vertices 1e-9 m
        // apart on its upper side (tin_edit's case): the refusal names the rim
        // and the tolerance, not "triangle names a vertex twice".
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(20, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        put_surface(
            &mut t.document,
            "top",
            make_surface(
                vec![
                    Point3::new(2.3, 3.7, 1.0),
                    Point3::new(12.3, 3.7, 1.0),
                    Point3::new(12.3, 13.7, 1.0),
                    Point3::new(2.3, 13.7, 1.0),
                    Point3::new(8.6, 13.7, 1.0),
                    Point3::new(8.600000001, 13.7, 1.0),
                ],
                vec![[0, 1, 2], [0, 2, 5], [0, 5, 4], [0, 4, 3]],
                Vec::new(),
            ),
            true,
        );
        let generation = t.document.surfaces_generation();
        let error = t.refused("SURFACE PASTE top INTO ground NAME out");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(contains(&error.message, "rim"), "{}", error.message);
        assert!(
            contains(&error.message, "coincidence tolerance"),
            "{}",
            error.message
        );
        assert!(
            !contains(&error.message, "names a vertex twice"),
            "{}",
            error.message
        );
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn every_refusal_says_what_was_wrong_and_changes_nothing() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        put_surface(
            &mut t.document,
            "pad",
            flat_grid_surface(1, 4.0, 2.0, 3.0, 3.0),
            true,
        );
        let generation = t.document.surfaces_generation();
        assert_eq!(
            t.fails("SURFACE EDIT nowhere ADD POINT 1,1,1"),
            ErrorCode::NotFound
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground EXPLODE"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground ADD POINT 1,1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground ADD POINT 1,1,1 extra"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground HOLE #999"),
            ErrorCode::NotFound
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground DELETE LONG -1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground SMOOTH passes=0"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground SMOOTH sideways=1"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground FLIP 50,50"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground FLIP 5,0"),
            ErrorCode::InvalidGeometry
        ); // the rim
        assert_eq!(t.fails("SURFACE MERGE ground pad"), ErrorCode::ParseFailure);
        assert_eq!(
            t.fails("SURFACE MERGE ground pad NAME pad"),
            ErrorCode::AlreadyExists
        );
        assert_eq!(t.fails("SURFACE PASTE pad ground"), ErrorCode::ParseFailure);
        assert_eq!(t.fails("SURFACE OFFSET ground 1"), ErrorCode::ParseFailure);
        assert_eq!(
            t.fails("SURFACE CLIP ground 20,20 30,20 30,30 NAME away"),
            ErrorCode::InvalidState
        );
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn an_entity_of_the_wrong_kind_is_refused() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        ok(t.document
            .execute(create_point(p(3.0, 3.0), EntityAttributes::default())));
        let point = t.document.last_created_entities()[0];
        assert_eq!(
            t.fails(&format!("SURFACE EDIT ground HOLE #{point}")),
            ErrorCode::InvalidArgument
        );
        // Two polygons where BOUNDARY takes one.
        polyline(
            &mut t,
            vec![p(1.0, 1.0), p(4.0, 1.0), p(4.0, 4.0)],
            true,
            "0",
        );
        polyline(
            &mut t,
            vec![p(6.0, 6.0), p(9.0, 6.0), p(9.0, 9.0)],
            true,
            "0",
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground BOUNDARY FROM DRAWING"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_and_no_step_is_made() {
        // The drawing holds one POINT: no polygon, no line, no point with a
        // level. CLAUDE.md 1.1: a scope that takes nothing is reported, not an
        // error - the scope record says what it took, the surface record says
        // changed=no and why nothing ran, and the session goes on. (OUTSIDE of
        // no polygon is not the whole surface, and an INSIDE that took nothing
        // is not "no INSIDE".)
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        ok(t.document
            .execute(create_point(p(3.0, 3.0), EntityAttributes::default())));
        let generation = t.document.surfaces_generation();
        let before = surface_of(&t.document, "ground").clone();
        for (line, why) in [
            (
                "DELETE TRIANGLES INSIDE FROM DRAWING",
                "the scope took no polygon",
            ),
            (
                "DELETE TRIANGLES OUTSIDE FROM DRAWING",
                "the scope took no polygon",
            ),
            (
                "DELETE POINTS INSIDE FROM DRAWING",
                "the scope took no polygon",
            ),
            ("BREAKLINE FROM DRAWING", "the scope took no line"),
            ("DELETE BREAKLINE FROM DRAWING", "the scope took no line"),
            ("BOUNDARY FROM DRAWING", "the scope took no polygon"),
            ("HOLE FROM DRAWING", "the scope took no polygon"),
            ("RAISE 1 INSIDE FROM DRAWING", "the scope took no polygon"),
            ("SMOOTH INSIDE FROM DRAWING", "the scope took no polygon"),
            (
                "ADD POINTS FROM DRAWING",
                "the scope took no point with a level",
            ),
        ] {
            let text = t.ok(&format!("SURFACE EDIT ground {line}"));
            assert!(contains(&text, "scope=drawing matched=1"), "{line}: {text}");
            assert!(
                contains(&text, &format!("changed=no ran=no reason=\"{why}\"")),
                "{line}: {text}"
            );
            assert_eq!(t.document.surfaces_generation(), generation, "{line}");
            assert!(*surface_of(&t.document, "ground") == before, "{line}");
        }
    }

    #[test]
    fn a_clip_of_a_scope_that_takes_nothing_makes_no_surface_and_says_so() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        ok(t.document
            .execute(create_point(p(3.0, 3.0), EntityAttributes::default())));
        let text = t.ok("SURFACE CLIP ground FROM DRAWING NAME part");
        assert!(
            contains(
                &text,
                "surface name=part operation=clip changed=no ran=no reason=\"the scope took no \
                 polygon\""
            ),
            "{text}"
        );
        assert!(t.document.surfaces().find("part").is_none());
    }

    #[test]
    fn several_breaklines_crossing_at_different_levels_are_refused_in_either_order() {
        // (5,5) is 12 on the first and 8 on the second. Added one after the
        // other the first named won there and the second bent to it.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(10.0, 10.0, 10.0, 10.0),
            true,
        );
        t.ok("PLINE3D 1,5,12 9,5,12");
        let along = t.document.last_created_entities()[0];
        t.ok("PLINE3D 5,1,8 5,9,8");
        let across = t.document.last_created_entities()[0];
        let generation = t.document.surfaces_generation();
        let a = format!("#{along}");
        let c = format!("#{across}");
        assert_eq!(
            t.fails(&format!("SURFACE EDIT ground BREAKLINE {a} {c}")),
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            t.fails(&format!("SURFACE EDIT ground BREAKLINE {c} {a}")),
            ErrorCode::InvalidGeometry
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground BREAKLINE FROM DRAWING"),
            ErrorCode::InvalidGeometry
        );
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn several_breaklines_that_agree_where_they_cross_are_one_edit() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(10.0, 10.0, 10.0, 10.0),
            true,
        );
        t.ok("PLINE3D 1,5,12 9,5,12");
        t.ok("PLINE3D 5,1,12 5,9,12");
        let text = t.ok("SURFACE EDIT ground BREAKLINE FROM DRAWING");
        assert!(
            contains(&text, "scope=drawing matched=2 lines=2 skipped=0\n"),
            "{text}"
        );
        assert!(contains(&text, "crossings=1"), "{text}");
        near(
            level(surface_of(&t.document, "ground"), 5.0, 5.0),
            12.0,
            1e-9,
        );
        t.ok("UNDO");
        assert_eq!(
            surface_of(&t.document, "ground").triangle_count(),
            2,
            "one undo step for both lines"
        );
    }

    #[test]
    fn a_surface_or_a_layer_called_preview_is_a_name_not_the_preview_flag() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 4.0, 0.0, 0.0),
            true,
        );
        // A new surface called preview (surface names compare ignoring case,
        // so it is the one name for the rest): 4 + 3 = 7 over [0, 10]^2.
        t.ok("SURFACE OFFSET ground 3 NAME preview");
        assert!(t.document.surfaces().find("Preview").is_some());
        double_eq(level(surface_of(&t.document, "preview"), 5.0, 5.0), 7.0);
        // A surface called Preview as the source of an offset, a clip and a
        // paste's top, and as a member of a merge.
        t.ok("SURFACE OFFSET Preview 1 NAME up");
        double_eq(level(surface_of(&t.document, "up"), 5.0, 5.0), 8.0);
        t.ok("SURFACE CLIP Preview 1,1 5,1 5,5 1,5 NAME clipped");
        near(surface_of(&t.document, "clipped").plan_area(), 16.0, 1e-9);
        t.ok("SURFACE MERGE ground Preview NAME merged");
        assert!(t.document.surfaces().find("merged").is_some());
        // A layer called preview in a FROM scope.
        t.ok("LAYER NEW preview");
        t.ok("LAYER SET preview");
        polyline(
            &mut t,
            vec![p(3.0, 3.0), p(6.0, 3.0), p(6.0, 7.0), p(3.0, 7.0)],
            true,
            "preview",
        );
        t.ok("SURFACE EDIT ground HOLE FROM LAYERS preview ONLY");
        near(surface_of(&t.document, "ground").plan_area(), 88.0, 1e-9);
        // And a PREVIEW that is not a value still previews, wherever it
        // stands.
        let count = t.document.surfaces().all().len();
        let text = t.ok("SURFACE OFFSET ground 1 NAME later PREVIEW");
        assert!(contains(&text, "preview=yes"), "{text}");
        assert_eq!(t.document.surfaces().all().len(), count);
        let early = t.ok("SURFACE EDIT ground PREVIEW RAISE 1");
        assert!(contains(&early, "preview=yes"), "{early}");
    }

    #[test]
    fn delete_point_just_outside_the_rim_takes_the_corner_within_reach() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        // (-0.05,-0.05) is 0.07 from the corner (0,0) and off the surface.
        let text = t.ok("SURFACE EDIT ground DELETE POINT -0.05,-0.05 reach=0.2");
        assert!(
            contains(&text, "triangles_before=32 triangles_after=31"),
            "{text}"
        );
        assert!(contains(&text, "points_removed=1"), "{text}");
        // Far outside, nothing is within reach: off the surface, said as such.
        let far = t.refused("SURFACE EDIT ground DELETE POINT -5,-5 reach=0.2");
        assert_eq!(far.code, ErrorCode::InvalidArgument);
        assert!(contains(&far.message, "off the surface"), "{}", far.message);
        // On the surface with no vertex within reach: said so.
        let bare = t.refused("SURFACE EDIT ground DELETE POINT 1.5,0.2 reach=0.01");
        assert!(contains(&bare.message, "within reach"), "{}", bare.message);
        let off_no_reach = t.refused("SURFACE EDIT ground DELETE POINT -0.05,-0.05");
        assert_eq!(off_no_reach.message, "the point is off the surface");
    }

    #[test]
    fn a_typed_polygon_that_closes_to_too_few_corners_says_how_many_it_has() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        // 0,0 10,0 0,0 is two corners and the first again: "needs three
        // corners [corners=3]" contradicted itself.
        for line in [
            "SURFACE EDIT ground BOUNDARY 0,0 10,0 0,0",
            "SURFACE EDIT ground HOLE 0,0 10,0 0,0 0,0",
        ] {
            let error = t.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(
                contains(&error.message, "three distinct corners"),
                "{}",
                error.message
            );
            assert!(
                contains(&error.context, "corners=2 typed="),
                "{}",
                error.context
            );
        }
        let flat = t.refused("SURFACE EDIT ground DELETE TRIANGLES INSIDE 5,5 5,5 5,5");
        assert!(
            contains(&flat.context, "corners=1 typed=3"),
            "{}",
            flat.context
        );
    }

    #[test]
    fn a_thin_that_removes_nothing_from_a_surface_not_in_canonical_order_is_no_step() {
        // The square's triangles written rotated, as an archive may give them:
        // THIN rebuilt the arrays sorted, which differs, so "changed=yes" and
        // an undo step for an edit that did nothing.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            make_surface(
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(10.0, 10.0, 0.0),
                    Point3::new(0.0, 10.0, 0.0),
                ],
                vec![[1, 2, 0], [3, 0, 2]],
                Vec::new(),
            ),
            true,
        );
        let generation = t.document.surfaces_generation();
        let text = t.ok("SURFACE EDIT ground THIN 0");
        assert!(contains(&text, "points_removed=0"), "{text}");
        assert!(contains(&text, "changed=no"), "{text}");
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn a_level_beyond_the_most_an_edit_accepts_is_refused() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        let generation = t.document.surfaces_generation();
        assert_eq!(
            t.fails("SURFACE EDIT ground ADD POINT 1e308,1,1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground RAISE 1e308"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground SETLEVEL 1e70000"),
            ErrorCode::ParseFailure
        );
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn the_tools_act_on_the_chosen_surface_else_the_first_visible() {
        let mut t = Session::default();
        set_surface_edit_target(String::new());
        assert_eq!(
            active_edit_surface(&t.document)
                .expect_err("no surface")
                .code,
            ErrorCode::NotFound
        );
        put_surface(
            &mut t.document,
            "hidden",
            square_surface(0.0, 0.0, 0.0, 0.0),
            false,
        );
        assert_eq!(must(active_edit_surface(&t.document)), "hidden"); // the first, none being visible
        put_surface(
            &mut t.document,
            "ground",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        put_surface(
            &mut t.document,
            "pad",
            square_surface(0.0, 0.0, 0.0, 0.0),
            true,
        );
        assert_eq!(must(active_edit_surface(&t.document)), "ground");
        set_surface_edit_target(String::from("PAD"));
        assert_eq!(must(active_edit_surface(&t.document)), "pad");
        set_surface_edit_target(String::from("gone"));
        assert_eq!(must(active_edit_surface(&t.document)), "ground");
        set_surface_edit_target(String::new());
        assert_eq!(surface_name_word("my ground"), "\"my ground\"");
        assert_eq!(surface_name_word("ground"), "ground");
    }

    #[test]
    fn help_names_every_verb() {
        let mut t = Session::default();
        let help = t.ok("HELP SURFACE EDIT");
        for word in [
            "DELETE TRIANGLE",
            "DELETE BREAKLINE",
            "MOVE POINT",
            "FLIP",
            "ADD POINTS",
            "BREAKLINE",
            "BOUNDARY",
            "HOLE",
            "SETLEVEL",
            "SMOOTH",
            "THIN",
            "SURFACE MERGE",
            "SURFACE PASTE",
            "SURFACE CLIP",
            "SURFACE OFFSET",
        ] {
            assert!(contains(&help, word), "{word}");
        }
        assert!(contains(&t.ok("HELP"), "SURFACE EDIT"));
    }

    #[test]
    fn delete_breakline_clears_the_edges_along_the_line_and_is_one_undo_step() {
        // The row y = 4 from x = 1 to 7 of a 10 x 10 unit grid is a breakline
        // of six lattice edges; its vertex (4,4) cannot be deleted, until the
        // line is.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        t.ok("SURFACE EDIT ground BREAKLINE 1,4 7,4");
        let with_line = surface_of(&t.document, "ground").clone();
        assert_eq!(
            t.fails("SURFACE EDIT ground DELETE POINT 4,4"),
            ErrorCode::InvalidGeometry
        );

        let text = t.ok("SURFACE EDIT ground DELETE BREAKLINE 1,4 7,4");
        assert!(
            contains(
                &text,
                "surface name=ground operation=delete_breakline triangles_before=200 \
                 triangles_after=200 vertices_before=121 vertices_after=121 points_added=0 \
                 points_removed=0"
            ),
            "{text}"
        );
        assert!(
            contains(&text, " edges_cleared=6 edges_held=0 changed=yes"),
            "{text}"
        );
        // The triangles are as they were; only the flags went.
        assert_eq!(
            surface_of(&t.document, "ground").triangles(),
            with_line.triangles()
        );
        assert_eq!(
            surface_of(&t.document, "ground").vertices(),
            with_line.vertices()
        );
        assert!(
            *surface_of(&t.document, "ground") != with_line,
            "the constrained edges are part of a surface"
        );
        let deleted = t.ok("SURFACE EDIT ground DELETE POINT 4,4");
        assert!(contains(&deleted, "points_removed=1"), "{deleted}");

        // One undo step each: undoing the delete, then the clearing.
        t.ok("UNDO");
        t.ok("UNDO");
        assert!(*surface_of(&t.document, "ground") == with_line);
        assert_eq!(
            t.fails("SURFACE EDIT ground DELETE POINT 4,4"),
            ErrorCode::InvalidGeometry
        );
    }

    #[test]
    fn delete_breakline_along_no_breakline_is_no_step_and_says_none_was_cleared() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        t.ok("SURFACE EDIT ground BREAKLINE 1,4 7,4");
        let generation = t.document.surfaces_generation();
        let text = t.ok("SURFACE EDIT ground DELETE BREAKLINE 1,5 7,5");
        assert!(
            contains(&text, " edges_cleared=0 edges_held=0 changed=no"),
            "{text}"
        );
        assert_eq!(t.document.surfaces_generation(), generation);
        // PREVIEW works the clearing out and changes nothing.
        let preview = t.ok("SURFACE EDIT ground DELETE BREAKLINE 1,4 4,4 PREVIEW");
        assert!(
            contains(
                &preview,
                " edges_cleared=3 edges_held=0 changed=yes preview=yes"
            ),
            "{preview}"
        );
        assert_eq!(t.document.surfaces_generation(), generation);
        // A line that is too short, and a name that is no surface.
        assert_eq!(
            t.fails("SURFACE EDIT ground DELETE BREAKLINE 1,4"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground DELETE BREAKLINE"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE EDIT nowhere DELETE BREAKLINE 1,4 7,4"),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn delete_breakline_from_a_scope_takes_the_lines_and_says_what_the_scope_took() {
        // Two breaklines of the surface, a polyline over each drawn on a layer:
        // the scope record says it took two lines, and both are cleared. A
        // third polyline, on another layer, would clear a third if it were
        // named.
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        t.ok("SURFACE EDIT ground BREAKLINE 1,4 3,4");
        t.ok("SURFACE EDIT ground BREAKLINE 6,2 6,5");
        t.ok("LAYER NEW cleared");
        t.ok("LAYER SET cleared");
        polyline(&mut t, vec![p(0.0, 4.0), p(4.0, 4.0)], false, "cleared");
        polyline(&mut t, vec![p(6.0, 0.0), p(6.0, 9.0)], false, "cleared");
        let text = t.ok("SURFACE EDIT ground DELETE BREAKLINE FROM LAYERS cleared ONLY");
        assert!(contains(&text, "lines=2 skipped=0"), "{text}");
        assert!(
            contains(&text, " edges_cleared=5 edges_held=0 changed=yes"),
            "{text}"
        );
    }

    #[test]
    fn delete_breakline_along_the_constrained_rim_holds_it_and_says_so() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        t.ok("SURFACE EDIT ground BOUNDARY 2.5,2.5 7.5,2.5 7.5,7.5 2.5,7.5");
        let generation = t.document.surfaces_generation();
        let text = t.ok("SURFACE EDIT ground DELETE BREAKLINE 2.5,2.5 7.5,2.5");
        assert!(contains(&text, " edges_cleared=0 "), "{text}");
        assert!(
            !contains(&text, " edges_held=0 "),
            "the rim edges along the line are counted: {text}"
        );
        assert!(contains(&text, "changed=no"), "{text}");
        assert_eq!(t.document.surfaces_generation(), generation);
    }

    #[test]
    fn move_point_moves_the_vertex_keeping_its_level_as_one_undo_step() {
        // The plane z = 0.5 x + 0.25 y on a 4 x 4 grid: (2,2) is at 1.5, and
        // moved to (3.5, 2.2) - across the ring of its own triangles - it
        // stays at 1.5, where the plane there is 2.3. 25 vertices before and
        // after, a vertex made and a vertex taken out; 32 triangles either way
        // (a polygon of n corners round k points has n + 2k - 2).
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            grid_surface(4, 1.0, |x, y| 0.5 * x + 0.25 * y, 0.0, 0.0),
            true,
        );
        let before = surface_of(&t.document, "ground").clone();
        let text = t.ok("SURFACE EDIT ground MOVE POINT 2,2 TO 3.5,2.2");
        assert!(
            contains(
                &text,
                "surface name=ground operation=move_point triangles_before=32 \
                 triangles_after=32 vertices_before=25 vertices_after=25 points_added=1 \
                 points_removed=1"
            ),
            "{text}"
        );
        assert!(contains(&text, " moved=1 "), "{text}");
        assert!(contains(&text, "changed=yes"), "{text}");
        let after = surface_of(&t.document, "ground");
        near(after.plan_area(), 16.0, 1e-9);
        near(level(after, 3.5, 2.2), 1.5, 1e-12);
        assert_eq!(t.document.history().undo_name(), "SURFACE_EDIT_MOVE_POINT");

        t.ok("UNDO");
        assert!(*surface_of(&t.document, "ground") == before);
        t.ok("REDO");
        near(
            level(surface_of(&t.document, "ground"), 3.5, 2.2),
            1.5,
            1e-12,
        );
    }

    #[test]
    fn move_point_names_the_vertex_by_its_reach_and_previews_without_moving_it() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        let generation = t.document.surfaces_generation();
        // (2.1, 2.1) is 0.14 from the vertex (2,2) and is where a click would
        // land.
        let preview = t.ok("SURFACE EDIT ground MOVE POINT 2.1,2.1 TO 2.4,1.7 reach=0.5 PREVIEW");
        assert!(contains(&preview, " moved=1 "), "{preview}");
        assert!(contains(&preview, "changed=yes preview=yes"), "{preview}");
        assert_eq!(t.document.surfaces_generation(), generation);
        t.ok("SURFACE EDIT ground MOVE POINT 2.1,2.1 TO 2.4,1.7 reach=0.5");
        let moved = surface_of(&t.document, "ground")
            .vertices()
            .iter()
            .any(|v| v.x == 2.4 && v.y == 1.7);
        assert!(moved, "a vertex at the target");
        // A reach that finds no vertex, a negative one, a point off the
        // surface.
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 1.5,0.2 TO 1.6,0.3 reach=0.01"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 2,2 TO 2.5,2.5 reach=-1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 40,40 TO 2.5,2.5"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn move_point_is_refused_where_the_vertex_or_the_target_cannot_be_and_says_why() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(10, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        t.ok("SURFACE EDIT ground BREAKLINE 1,4 7,4");
        let generation = t.document.surfaces_generation();
        // A vertex of the breakline; a corner of the rim; a target off the
        // surface.
        let on_line = t.refused("SURFACE EDIT ground MOVE POINT 4,4 TO 4.2,4.3");
        assert_eq!(on_line.code, ErrorCode::InvalidGeometry);
        assert!(
            contains(&on_line.message, "breakline"),
            "{}",
            on_line.message
        );
        let on_rim = t.refused("SURFACE EDIT ground MOVE POINT 0,0 TO 0.3,0.2");
        assert_eq!(on_rim.code, ErrorCode::InvalidGeometry);
        assert!(contains(&on_rim.message, "rim"), "{}", on_rim.message);
        let off = t.refused("SURFACE EDIT ground MOVE POINT 5,6 TO 50,60");
        assert_eq!(off.code, ErrorCode::InvalidGeometry);
        assert!(contains(&off.message, "off the surface"), "{}", off.message);
        assert_eq!(t.document.surfaces_generation(), generation);
        // Clear the breakline and the vertex moves.
        t.ok("SURFACE EDIT ground DELETE BREAKLINE 1,4 7,4");
        t.ok("SURFACE EDIT ground MOVE POINT 4,4 TO 4.2,4.3");
        near(surface_of(&t.document, "ground").plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn move_point_needs_its_words_and_an_xy_target() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE 2,2 TO 2.5,2.5"),
            ErrorCode::ParseFailure
        );
        assert_eq!(t.fails("SURFACE EDIT ground MOVE"), ErrorCode::ParseFailure);
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 2,2"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 2,2 TO"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 2,2 TO 2.5"),
            ErrorCode::ParseFailure
        );
        // A level on the target would be a change of level, which is RAISE's
        // and ADD POINT's.
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 2,2 TO 2.5,2.5,9"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("SURFACE EDIT ground MOVE POINT 2,2 TO 2.5,2.5 extra"),
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn move_point_by_less_than_the_tolerance_is_no_move_and_no_step() {
        let mut t = Session::default();
        put_surface(
            &mut t.document,
            "ground",
            flat_grid_surface(4, 1.0, 0.0, 0.0, 0.0),
            true,
        );
        let generation = t.document.surfaces_generation();
        let text = t.ok("SURFACE EDIT ground MOVE POINT 2,2 TO 2.00000005,2");
        assert!(contains(&text, "changed=no"), "{text}");
        assert_eq!(t.document.surfaces_generation(), generation);
    }
}
