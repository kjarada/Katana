// SURFACE LEGEND and SURFACE PLAN through the interpreter
// (terrain/surface_display_verbs.rs, docs/surface_display.md "On the command
// line"; tests/cad/terrain/test_surface_display_verbs.cpp), on a drawing whose
// surface is the 10 x 10 square z = x and whose style is made here, so every
// number in a reply is worked out below.

mod surface_display_verbs {
    use std::sync::Arc;

    use katana_cad::terrain::surface_display_verbs::surface_legend_json;
    use katana_cad::terrain::surface_style::built_in_surface_styles;
    use katana_cad::{
        CommandInterpreter, Document, ScopeView, ScopeViewFn, SurfaceDisplayStyle, SurfaceFaces,
        SurfaceLegend, SurfaceLegendEntry, SurfaceLegendReply, ViewTransform,
    };
    use katana_core::{ErrorCode, Result};
    use katana_entity::Color;
    use katana_geometry::{Point2, Point3};
    use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

    use crate::{Session, contains as has, must, ok};

    fn square() -> Arc<TinSurface> {
        let tin = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 10.0),
                Point3::new(10.0, 10.0, 10.0),
                Point3::new(0.0, 10.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        );
        Arc::new(must(tin))
    }

    // The drawing: the built-in styles and "Test Bands" - plan faces by
    // elevation in five bands, triangles drawn, no boundary; in 3D faces by
    // slope (banded too: the banding is the plan's and the 3D view's alike).
    fn drawing(visible: bool) -> Session {
        let mut styles = built_in_surface_styles();
        styles.styles.push(SurfaceDisplayStyle {
            name: String::from("Test Bands"),
            plan_faces: SurfaceFaces::Elevation,
            banded: true,
            bands: 5,
            show_triangles: true,
            show_boundary: false,
            faces3d: SurfaceFaces::Slope,
            ..SurfaceDisplayStyle::default()
        });
        let mut session = Session::default();
        ok(session
            .document
            .set_surface_styles(styles, Document::SET_SURFACE_STYLES));
        let mut store = SurfaceStore::default();
        ok(store.add(NamedSurface {
            name: String::from("ground"),
            surface: Some(square()),
            source: String::from("test"),
            style: String::from("Test Bands"),
            visible,
        }));
        ok(session.document.set_surfaces(store, Document::SET_SURFACES));
        session
    }

    #[test]
    fn legend_lists_the_bands_of_the_surfaces_plan_style() {
        let mut drawing = drawing(true);
        let reply = drawing.ok("SURFACE LEGEND ground");
        // Five bands of 2 over the surface's own 0 to 10, coloured the terrain
        // ramp at 0, 1/4, 1/2, 3/4 and 1: #28468C (40, 70, 140) first, #F5F5F5
        // (245, 245, 245) last.
        assert!(
            has(
                &reply,
                "legend surface=\"ground\" style=\"Test Bands\" view=plan faces=elevation \
                 kind=bands entries=5"
            ),
            "{reply}"
        );
        assert!(
            has(
                &reply,
                "entry index=1 colour=#28468C from=0 to=2 label=\"0 - 2\""
            ),
            "{reply}"
        );
        assert!(
            has(
                &reply,
                "entry index=5 colour=#F5F5F5 from=8 to=10 label=\"8 - 10\""
            ),
            "{reply}"
        );
    }

    #[test]
    fn legend_in3_d_reads_faces3d_over_the_surfaces_own_range() {
        let mut drawing = drawing(true);
        // Slope: z = x rises 1 in 1, 100 % on both faces, so the range is 0 to
        // 100 and five bands are 20 wide.
        let reply = drawing.ok("SURFACE LEGEND ground 3D");
        assert!(
            has(&reply, "view=3d faces=slope kind=bands entries=5"),
            "{reply}"
        );
        assert!(
            has(&reply, "entry index=1 colour=#28468C from=0 to=20"),
            "{reply}"
        );
    }

    #[test]
    fn legend_as_json_is_one_object_of_the_same_entries() {
        let mut drawing = drawing(true);
        let reply = drawing.ok("SURFACE LEGEND ground JSON");
        assert_eq!(reply.chars().next(), Some('{'));
        assert!(has(&reply, "\"surface\":\"ground\""), "{reply}");
        assert!(has(&reply, "\"kind\":\"bands\""), "{reply}");
        assert!(has(&reply, "\"colour\":\"#28468C\""), "{reply}");
        assert!(has(&reply, "\"label\":\"0 - 2\""), "{reply}");
    }

    #[test]
    fn plan_says_what_the_plan_view_draws_of_the_surface() {
        let mut drawing = drawing(true);
        // The whole surface through 80 x 80 pixels: an eighth of a unit a
        // pixel. Its two triangles' five edges (4 sides, 1 diagonal) are 80
        // and 113 px long, all stroked; the image is 80 x 80 and every pixel
        // covered once.
        let reply = drawing.ok("SURFACE PLAN ground pixels=80,80");
        assert!(
            has(
                &reply,
                "plan surface=\"ground\" style=\"Test Bands\" visible=yes pixels=80,80 \
                 unitsPerPixel=0.125 triangles=2 inView=2 edges=5 tinted=0 faces=elevation \
                 image=80x80 covered=6400"
            ),
            "{reply}"
        );
        assert!(has(&reply, "minor=off major=off"), "{reply}");
        assert!(has(&reply, "\nscope=surface"), "{reply}");
    }

    #[test]
    fn plan_looks_through_an_area_and_says_which_one() {
        let mut drawing = drawing(true);
        let reply = drawing.ok("SURFACE PLAN ground AREA 0,0,5,5 pixels=50,50");
        // A tenth of a unit a pixel; both triangles reach the corner square.
        assert!(
            has(&reply, "unitsPerPixel=0.1 triangles=2 inView=2"),
            "{reply}"
        );
        assert!(has(&reply, "scope=area area=0,0,5,5"), "{reply}");
    }

    #[test]
    fn a_hidden_surface_is_still_described_and_says_it_is_hidden() {
        let mut drawing = drawing(false);
        assert!(has(
            &drawing.ok("SURFACE PLAN ground pixels=80,80"),
            "visible=no"
        ));
    }

    #[test]
    fn the_verbs_change_nothing_and_leave_no_undo_step() {
        let mut drawing = drawing(true);
        let steps = drawing.document.history().undo_count();
        let revision = drawing.document.model_revision();
        drawing.ok("SURFACE LEGEND ground");
        drawing.ok("SURFACE PLAN ground");
        assert_eq!(drawing.document.history().undo_count(), steps);
        assert_eq!(drawing.document.model_revision(), revision);
    }

    // `line` is refused with `code`, and the refusal names `part`.
    #[track_caller]
    fn refused(drawing: &mut Session, line: &str, code: ErrorCode, part: &str) {
        let error = drawing.refused(line);
        assert_eq!(error.code, code, "{line}: {}", error.describe());
        assert!(has(&error.describe(), part), "{line}: {}", error.describe());
    }

    #[test]
    fn every_refusal_says_what_was_wrong() {
        let mut drawing = drawing(true);
        refused(
            &mut drawing,
            "SURFACE LEGEND nothing",
            ErrorCode::NotFound,
            "nothing",
        );
        refused(
            &mut drawing,
            "SURFACE PLAN nothing",
            ErrorCode::NotFound,
            "nothing",
        );
        refused(
            &mut drawing,
            "SURFACE LEGEND",
            ErrorCode::ParseFailure,
            "LEGEND",
        );
        refused(
            &mut drawing,
            "SURFACE LEGEND ground SIDEWAYS",
            ErrorCode::InvalidArgument,
            "SIDEWAYS",
        );
        refused(
            &mut drawing,
            "SURFACE PLAN ground WHERE TYPE=point",
            ErrorCode::InvalidArgument,
            "WHERE",
        );
        refused(
            &mut drawing,
            "SURFACE PLAN ground SELECTION",
            ErrorCode::InvalidArgument,
            "AREA",
        );
        refused(
            &mut drawing,
            "SURFACE PLAN ground pixels=0,10",
            ErrorCode::InvalidArgument,
            "pixels=",
        );
        refused(
            &mut drawing,
            "SURFACE PLAN ground pixels=wide",
            ErrorCode::InvalidArgument,
            "pixels=",
        );
        refused(
            &mut drawing,
            "SURFACE PLAN ground AREA 0,0,5",
            ErrorCode::ParseFailure,
            "AREA",
        );
        // Headless there is no plan view: VIEW is refused naming AREA.
        let view = drawing.refused("SURFACE PLAN ground VIEW");
        assert!(has(&view.describe(), "AREA"), "{}", view.describe());
    }

    #[test]
    fn help_names_both() {
        let help = CommandInterpreter::help_text();
        assert!(has(&help, "SURFACE LEGEND"));
        assert!(has(&help, "SURFACE PLAN"));
    }

    // The window's answer for VIEW: a plan view of `width` x `height` pixels
    // at 8 pixels a unit about (5, 5), which has the id 7 - or one with no
    // plan extent at all, a 3D view's.
    fn plan_view_of(width: f64, height: f64, with_frame: bool) -> Box<ScopeViewFn<'static>> {
        Box::new(move |_: Option<u32>| -> Result<ScopeView> {
            // resize() sets only the pixels, so it may follow the scale and
            // centre the C++ sets after it.
            let mut transform = ViewTransform {
                scale: 8.0,
                center: Point2::new(5.0, 5.0),
                ..ViewTransform::default()
            };
            transform.resize(width, height);
            Ok(ScopeView {
                id: 7,
                area: Some(transform.visible_world_bounds()),
                frame: with_frame.then_some(transform),
                ..ScopeView::default()
            })
        })
    }

    #[test]
    fn plan_of_the_view_is_what_the_view_draws_at_its_own_size_and_scale() {
        let mut drawing = drawing(true);
        // 200 x 80 pixels at 8 a unit about (5, 5) shows x from -7.5 to 17.5
        // and y from 0 to 10: the square, whole, with 7.5 empty units either
        // side. The surface is judged at the view's 8 pixels a unit -
        // unitsPerPixel 1/8 - as the view draws it: 80 x 80 image pixels, all
        // covered (6 400), and the five edges, 80 and 113 px long, stroked.
        // Reframed in the default 1600 x 1000 it was judged at 25 / 1600 =
        // 0.015625 units a pixel, a 640 x 640 image of 409 600 pixels - a
        // different picture.
        drawing
            .interpreter
            .set_scope_context(Some(plan_view_of(200.0, 80.0, true)));
        let reply = drawing.ok("SURFACE PLAN ground VIEW");
        assert!(
            has(
                &reply,
                "pixels=200,80 unitsPerPixel=0.125 triangles=2 inView=2 edges=5 tinted=0 \
                 faces=elevation image=80x80 covered=6400"
            ),
            "{reply}"
        );
        assert!(
            has(&reply, "scope=view view=7 area=-7.5,0,17.5,10"),
            "{reply}"
        );
        // The same view in the plan verb's own default frame reads
        // differently, which is what the old reply was.
        let fitted = drawing.ok("SURFACE PLAN ground VIEW pixels=1600,1000");
        assert!(
            has(&fitted, "pixels=1600,1000 unitsPerPixel=0.015625 "),
            "{fitted}"
        );
        // pixels= still asks for another frame of the view's area: 25 x 10
        // units in 80 x 80 pixels is 25 / 80 = 0.3125 units a pixel.
        let asked = drawing.ok("SURFACE PLAN ground VIEW pixels=80,80");
        assert!(has(&asked, "pixels=80,80 unitsPerPixel=0.3125 "), "{asked}");
    }

    #[test]
    fn plan_of_a_view_with_no_transform_fits_its_area_as_before_and_extents_is_refused_by_name() {
        let mut drawing = drawing(true);
        // A provider that gives the area and no transform: fitted to the
        // default 1600 x 1000 as an AREA is - the 10 units over 1000 px.
        drawing
            .interpreter
            .set_scope_context(Some(plan_view_of(80.0, 80.0, false)));
        assert!(has(
            &drawing.ok("SURFACE PLAN ground VIEW"),
            "pixels=1600,1000 unitsPerPixel=0.01 "
        ));
        // VIEW EXTENTS has no area to look at: said so, not that the view has
        // no plan area.
        let extents = drawing.refused("SURFACE PLAN ground VIEW EXTENTS");
        assert_eq!(extents.code, ErrorCode::InvalidArgument);
        assert!(
            has(&extents.describe(), "EXTENTS"),
            "{}",
            extents.describe()
        );
        assert!(
            !has(&extents.describe(), "no plan area"),
            "{}",
            extents.describe()
        );
        // A view with no plan extent (a 3D view's) is refused as it was.
        drawing.interpreter.set_scope_context(Some(Box::new(
            |_: Option<u32>| -> Result<ScopeView> {
                Ok(ScopeView {
                    id: 9,
                    ..ScopeView::default()
                })
            },
        )));
        let three_d = drawing.refused("SURFACE PLAN ground VIEW 9");
        assert!(
            has(&three_d.describe(), "no plan area"),
            "{}",
            three_d.describe()
        );
    }

    #[test]
    fn plan_of_an_area_that_is_a_point_is_refused_not_answered_with_zero_counts() {
        let mut drawing = drawing(true);
        // The square covers (5, 5). A plan of a point is never worked out, and
        // its reply - every count 0 - was that of a point the surface does not
        // cover.
        for line in [
            "SURFACE PLAN ground AREA 5,5,5,5",
            "SURFACE PLAN ground AREA 500,500,500,500",
        ] {
            let error = drawing.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(
                has(&error.describe(), "AREA has no width and no height"),
                "{}",
                error.describe()
            );
        }
        // A line (no width, some height) has a frame.
        assert!(has(
            &drawing.ok("SURFACE PLAN ground AREA 5,5,5,6"),
            "inView="
        ));
    }

    #[test]
    fn legend_of_a_wireframe_in3_d_is_no_colours_for_its_mesh_is_one_grey() {
        let mut drawing = drawing(true);
        drawing.ok("SURFACE STYLE ground Outline");
        // Outline is faces3d=elevation drawn as a grey mesh: it colours no
        // face, so it has no gradient to explain (it said nine entries).
        let reply = drawing.ok("SURFACE LEGEND ground 3D");
        assert!(
            has(&reply, "view=3d faces=none kind=none entries=0"),
            "{reply}"
        );
    }

    #[test]
    fn legend_of_a_range_the_surface_runs_past_opens_its_end_bands() {
        let mut drawing = drawing(true);
        // z = x from 0 to 10; five bands over 3 to 7 are 0.8 wide: below 3.8
        // the first colour covers 0 to 3.8, above 6.2 the last covers 6.2 to
        // 10.
        drawing.ok("SURFACESTYLE NEW Focus FROM \"Test Bands\" min=3 max=7");
        drawing.ok("SURFACE STYLE ground Focus");
        let reply = drawing.ok("SURFACE LEGEND ground");
        let first = reply.find("entry index=1");
        let second = reply.find("entry index=2");
        let last = reply.find("entry index=5");
        let (Some(first), Some(second), Some(last)) = (first, second, last) else {
            panic!("{reply}");
        };
        let first_line = &reply[first..second];
        assert!(has(first_line, "label=\"< 3.8\""), "{first_line}");
        assert!(!has(first_line, " from="), "{first_line}");
        let last_line = &reply[last..];
        assert!(has(last_line, "label=\"6.2 +\""), "{last_line}");
        assert!(!has(last_line, " to="), "{last_line}");
        let json = drawing.ok("SURFACE LEGEND ground JSON");
        assert!(has(&json, "\"from\":null"), "{json}");
        assert!(has(&json, "\"to\":null"), "{json}");
    }

    // ---- the JSON reply, from plain data ------------------------------------------------------
    //
    // No verb on a Document can reach the guard behind these: the surface
    // store refuses a name that is not UTF-8 (SurfaceStore::check_name), and a
    // project that holds one is repaired on opening. So the reply is built
    // from the plain data it is made of.

    #[test]
    fn legend_json_writes_a_bad_byte_of_a_name_or_label_as_u_fffd_and_does_not_throw() {
        // The C++ hands the writer the three ways a byte string fails to be
        // UTF-8: 0xFF is never a byte of UTF-8; 0xC3 is the first of a
        // two-byte character with nothing after it (the string ends); 0x80 is
        // a continuation byte with no character to continue. Each is replaced
        // by ONE U+FFFD and the rest of the string is kept. No String holds
        // such bytes (docs/rust.md, step 2: the model holds validated UTF-8):
        // here they are replaced where Rust text is made of them,
        // String::from_utf8_lossy, one U+FFFD for each, which is the UTF-8
        // half of the case - the writer writes the replacement as it is, with
        // the members in the order the reply writes them.
        let reply = SurfaceLegendReply {
            surface: String::from_utf8_lossy(b"gro\xFFund").into_owned(),
            style: String::from_utf8_lossy(b"Bands\xC3").into_owned(),
            faces: SurfaceFaces::Elevation,
            legend: SurfaceLegend {
                faces: SurfaceFaces::Elevation,
                title: String::from("Elevation"),
                entries: vec![SurfaceLegendEntry {
                    colour: Color::new(40, 70, 140, 255),
                    label: String::from_utf8_lossy(b"a\x80z").into_owned(),
                    ..SurfaceLegendEntry::default()
                }],
                ..SurfaceLegend::default()
            },
            ..SurfaceLegendReply::default()
        };
        let json = surface_legend_json(&reply);
        // U+FFFD in UTF-8 is EF BF BD. The colour is #28468C (40, 70, 140 in
        // hex: 28, 46, 8C); an entry with neither end is open both ways.
        let replacement = "\u{FFFD}";
        let expected = format!(
            "{{\"surface\":\"gro{replacement}und\",\"style\":\"Bands{replacement}\",\
             \"view\":\"plan\",\"faces\":\"elevation\",\"kind\":\"bands\",\"title\":\"Elevation\",\
             \"entries\":[{{\"colour\":\"#28468C\",\"from\":null,\"to\":null,\"label\":\"a{replacement}z\"}}]}}"
        );
        assert_eq!(json, expected);
    }

    #[test]
    fn legend_json_of_a_clean_name_is_unchanged_and_of_no_colours_has_no_entries() {
        // The guard changes nothing it need not: a clean name is as given (a
        // multi-byte character that IS UTF-8, e + acute accent, passes whole),
        // and a surface whose faces are not coloured (a wireframe in 3D) has
        // the faces word "none", the kind "none" and no entries - not a
        // missing member.
        let reply = SurfaceLegendReply {
            surface: String::from("caf\u{E9}"),
            style: String::from("Outline"),
            three_d: true,
            ..SurfaceLegendReply::default()
        };
        let json = surface_legend_json(&reply);
        assert_eq!(
            json,
            "{\"surface\":\"caf\u{E9}\",\"style\":\"Outline\",\"view\":\"3d\",\"faces\":\"none\",\
             \"kind\":\"none\",\"title\":\"\",\"entries\":[]}"
        );
    }
}
