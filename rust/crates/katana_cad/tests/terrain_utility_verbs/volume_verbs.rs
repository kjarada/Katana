// The volume verbs (terrain/volume_verbs.rs, docs/volumes.md;
// tests/cad/terrain/test_volume_verbs.cpp) driven through
// CommandInterpreter::run on a Document whose surfaces are set as a front end
// sets them (Document::set_surfaces). Every expected number is a solid worked
// by hand in the comment beside it: slabs between parallel planes, a pyramid,
// a wedge whose area grows with the square of the station.

mod volume_verbs {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use katana_cad::terrain::surface_verbs::surface_record;
    use katana_cad::{CommandInterpreter, Document};
    use katana_commands::{EntityAttributes, create_entities, create_polyline};
    use katana_core::ErrorCode;
    use katana_core::path_text::path_to_utf8;
    use katana_core::text::{ReplyRecord, parse_finite_double, read_reply_record};
    use katana_entity::{Entity, EntityId, Geometry, PropertyValue};
    use katana_geometry::{Ellipse2, Point2, Point3, Polyline2, Spline2, Vec2};
    use katana_math::numerics::PI;
    use katana_terrain::{
        Breakline, NamedSurface, TinBuildOptions, TinInput, TinSurface, build_tin,
    };

    use crate::circle_spline::circle_spline;
    use crate::{Session, must, ok};

    // A surface of the points of a square grid over [x0, x0 + size]^2, every
    // `step`, at the heights `z` gives: any triangulation of points on a plane
    // is that plane, so a plane's grid is the plane exactly.
    fn grid_surface(
        x0: f64,
        y0: f64,
        size: f64,
        step: f64,
        z: impl Fn(f64, f64) -> f64,
    ) -> TinSurface {
        let mut input = TinInput::default();
        // std::lround: to the nearest, a half away from zero.
        let count = (size / step).round() as i32;
        for j in 0..=count {
            for i in 0..=count {
                let x = x0 + step * f64::from(i);
                let y = y0 + step * f64::from(j);
                input.points.push(Point3::new(x, y, z(x, y)));
            }
        }
        must(build_tin(&input, &TinBuildOptions::default())).surface
    }

    // The surface buildTin makes of `input` with the default options.
    fn built(input: &TinInput) -> TinSurface {
        must(build_tin(input, &TinBuildOptions::default())).surface
    }

    // The pyramid on [0, 100]^2, apex 30 high at the centre, its four ridges
    // breaklines: exactly four faces, (1/3) 100^2 30 = 100 000 m^3 above
    // z = 0.
    fn pyramid() -> TinSurface {
        let points = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(100.0, 0.0, 0.0),
            Point3::new(100.0, 100.0, 0.0),
            Point3::new(0.0, 100.0, 0.0),
            Point3::new(50.0, 50.0, 30.0),
        ];
        let breaklines: Vec<Breakline> = points[..4]
            .iter()
            .map(|&corner| Breakline {
                vertices: vec![corner, points[4]],
                closed: false,
            })
            .collect();
        built(&TinInput {
            points,
            breaklines,
            ..TinInput::default()
        })
    }

    #[track_caller]
    fn add_surface(t: &mut Session, name: &str, surface: TinSurface) {
        let mut store = t.document.surfaces().clone();
        let named = NamedSurface {
            name: String::from(name),
            surface: Some(Arc::new(surface)),
            source: String::from("test"),
            ..NamedSurface::default()
        };
        ok(store.add(named));
        ok(t.document.set_surfaces(store, "TEST"));
    }

    // The ground z = 10 + 0.05 x and a design 1 m above it, both over
    // [0, 100]^2.
    fn add_parallel_planes(t: &mut Session) {
        add_surface(
            t,
            "ground",
            grid_surface(0.0, 0.0, 100.0, 10.0, |x, _| 10.0 + 0.05 * x),
        );
        add_surface(
            t,
            "design",
            grid_surface(0.0, 0.0, 100.0, 20.0, |x, _| 11.0 + 0.05 * x),
        );
    }

    fn records(reply: &str, kind: &str) -> Vec<ReplyRecord> {
        reply
            .split('\n')
            .filter_map(read_reply_record)
            .filter(|record| record.words.first().is_some_and(|word| word == kind))
            .collect()
    }

    fn record(reply: &str, kind: &str) -> Option<ReplyRecord> {
        records(reply, kind).into_iter().next()
    }

    fn value(record: &Option<ReplyRecord>, key: &str) -> String {
        match record {
            Some(record) => String::from(record.value(key).unwrap_or("<none>")),
            None => String::from("<no record>"),
        }
    }

    // A record's value, or "" when it has none (the C++ `value_or("")`).
    fn field(record: &ReplyRecord, key: &str) -> String {
        String::from(record.value(key).unwrap_or(""))
    }

    // The entity id the last drawn thing got: the drawing's largest.
    fn last_id(t: &Session) -> EntityId {
        let mut last: EntityId = 0;
        t.document
            .model()
            .entities
            .for_each(|entity| last = last.max(entity.id));
        last
    }

    fn fixture() -> Session {
        Session::default()
    }

    // A scratch file of the test's own: the C++ names it by the fixture's
    // address, which is unique among the cases one process runs; here the
    // cases share one process, so it carries the case's stem and the
    // process.
    fn scratch_file(stem: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "katana_volume_verbs_{stem}_{}.csv",
            std::process::id()
        ))
    }

    // The path as a line writes it (core::pathToUtf8).
    fn utf8(file: &Path) -> String {
        String::from_utf8_lossy(&path_to_utf8(file)).into_owned()
    }

    // A file's lines (CRLF removed) or none, as std::getline reads them: a
    // final line break ends the last line rather than starting an empty one.
    fn lines_of(file: &Path) -> Vec<String> {
        let Ok(bytes) = std::fs::read(file) else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut lines = Vec::new();
        let mut rest: &str = &text;
        while !rest.is_empty() {
            let (line, next) = match rest.find('\n') {
                Some(at) => (&rest[..at], &rest[at + 1..]),
                None => (rest, ""),
            };
            lines.push(String::from(line.strip_suffix('\r').unwrap_or(line)));
            rest = next;
        }
        lines
    }

    fn remove(file: &Path) {
        let _ = std::fs::remove_file(file);
    }

    // ---- VOLUME SURFACES -----------------------------------------------------------------------

    #[test]
    fn two_parallel_planes_one_metre_apart_hold_the_slab_over_the_whole_and_inside_a_typed_square()
    {
        let mut t = fixture();
        add_parallel_planes(&mut t);
        // The whole: 1 m over 100 x 100.
        let whole = t.ok("VOLUME SURFACES ground design");
        let total = record(&whole, "volume");
        assert_eq!(value(&total, "method"), "exact");
        assert_eq!(value(&total, "fill"), "10000.000");
        assert_eq!(value(&total, "cut"), "0.000");
        assert_eq!(value(&total, "net"), "10000.000");
        assert_eq!(value(&total, "area"), "10000.000");
        // Inside the 10 x 10 square typed corner by corner: 100.
        let inside = t.ok("VOLUME SURFACES ground design INSIDE 23,27 33,27 33,37 23,37");
        let square = record(&inside, "volume");
        assert_eq!(value(&square, "fill"), "100.000");
        assert_eq!(value(&square, "area"), "100.000");
        assert_eq!(value(&square, "polygons"), "1");
        assert_eq!(value(&record(&inside, "polygons"), "used"), "1");
    }

    #[test]
    fn each_polygon_a_scope_takes_has_its_own_record_and_an_open_line_is_counted_not_used() {
        let mut t = fixture();
        add_parallel_planes(&mut t);
        t.ok("LAYER NEW lots");
        t.ok("LAYER SET lots");
        t.ok("PLINE 10,10 20,10 20,20 10,20 CLOSE"); // 100 m2
        t.ok("PLINE 50,50 70,50 70,60 50,60 CLOSE"); // 200 m2
        t.ok("LINE 0,90 90,90"); // bounds nothing
        let reply = t.ok("VOLUME SURFACES ground design INSIDE FROM LAYERS lots EACH");
        let boundary = record(&reply, "boundary");
        assert_eq!(value(&boundary, "scope"), "layers");
        assert_eq!(value(&boundary, "matched"), "3");
        let used = record(&reply, "polygons");
        assert_eq!(value(&used, "used"), "2");
        assert_eq!(value(&used, "skipped.open"), "1");
        let each = records(&reply, "polygon");
        assert_eq!(each.len(), 2, "{reply}");
        assert_eq!(field(&each[0], "fill"), "100.000");
        assert_eq!(field(&each[0], "polygon.area"), "100.000");
        assert!(each[0].value("entity").is_some());
        assert_eq!(field(&each[1], "fill"), "200.000");
        assert_eq!(value(&record(&reply, "volume"), "fill"), "300.000");
    }

    #[test]
    fn a_full_ellipse_and_a_polyline_drawn_back_to_its_start_are_volume_polygons() {
        // The slab is 1 m thick, so the fill inside a polygon is its area. The
        // square [10, 20]^2 drawn back to its first corner but not closed:
        // 100. The ellipse of semi-axes 20 and 10 about (50, 50): pi 200 =
        // 628.319, less what its chords cut off - each within 1 mm of the
        // curve, so at most its perimeter (Ramanujan: pi (90 - sqrt 3500) =
        // 96.9) x 1 mm, 0.097; 0.15 allows a chord's farthest point to be off
        // its middle.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let drawn_back = Polyline2::new(
            vec![
                Point2::new(10.0, 10.0),
                Point2::new(20.0, 10.0),
                Point2::new(20.0, 20.0),
                Point2::new(10.0, 20.0),
                Point2::new(10.0, 10.0),
            ],
            false,
        );
        ok(t.document
            .execute(create_polyline(drawn_back, EntityAttributes::default())));
        let square = last_id(&t);
        let whole = Ellipse2 {
            center: Point2::new(50.0, 50.0),
            major_axis: Vec2::new(20.0, 0.0),
            ratio: 0.5,
            ..Ellipse2::default()
        };
        let ellipse = Entity {
            geometry: Geometry::Ellipse(whole),
            ..Entity::default()
        };
        ok(t.document.execute(create_entities(vec![ellipse])));
        let basin = last_id(&t);

        let reply = t.ok(&format!(
            "VOLUME SURFACES ground design INSIDE #{square} #{basin} EACH"
        ));
        let each = records(&reply, "polygon");
        assert_eq!(each.len(), 2, "{reply}");
        assert_eq!(field(&each[0], "fill"), "100.000");
        assert!(each[0].value("chorded").is_none());
        let Some(fill) = parse_finite_double(field(&each[1], "fill")) else {
            panic!("{reply}");
        };
        let ellipse_area = 200.0 * PI;
        assert!(fill <= ellipse_area + 0.0005); // the reply rounds to the millimetre
        assert!(fill >= ellipse_area - 0.15);
        assert_eq!(field(&each[1], "chorded"), "yes");
    }

    #[test]
    fn overlapping_polygons_are_refused_for_one_total_and_warned_of_with_each() {
        let mut t = fixture();
        add_parallel_planes(&mut t);
        t.ok("PLINE 10,10 30,10 30,30 10,30 CLOSE");
        let first = last_id(&t);
        t.ok("PLINE 20,20 40,20 40,40 20,40 CLOSE"); // shares a 10 x 10 corner
        let second = last_id(&t);
        let ids = format!("#{first} #{second}");
        assert_eq!(
            t.fails(&format!("VOLUME SURFACES ground design INSIDE {ids}")),
            ErrorCode::InvalidGeometry
        );
        let reply = t.ok(&format!("VOLUME SURFACES ground design INSIDE {ids} EACH"));
        assert!(record(&reply, "warning").is_some(), "{reply}");
        // Each is 400; the total counts the shared 100 twice, as it says.
        assert_eq!(value(&record(&reply, "volume"), "fill"), "800.000");
    }

    #[test]
    fn factors_are_applied_in_the_reply_and_never_to_the_volumes() {
        // The ground at 10 and a design z = 10 + 0.1 (x - 50) over [0, 100]^2:
        // cut and fill are each 100 x 0.1 x 50^2 / 2 = 12 500. bulk 1.25 makes
        // the cut 15 625, shrink 1.1 the fill 13 750: net -1875.
        let mut t = fixture();
        add_surface(
            &mut t,
            "ground",
            grid_surface(0.0, 0.0, 100.0, 25.0, |_, _| 10.0),
        );
        add_surface(
            &mut t,
            "design",
            grid_surface(0.0, 0.0, 100.0, 25.0, |x, _| 10.0 + 0.1 * (x - 50.0)),
        );
        let total = record(
            &t.ok("VOLUME SURFACES ground design bulk=1.25 shrink=1.1"),
            "volume",
        );
        assert_eq!(value(&total, "cut"), "12500.000");
        assert_eq!(value(&total, "fill"), "12500.000");
        assert_eq!(value(&total, "cut.adjusted"), "15625.000");
        assert_eq!(value(&total, "fill.adjusted"), "13750.000");
        assert_eq!(value(&total, "net.adjusted"), "-1875.000");
        assert_eq!(value(&total, "bulk"), "1.25");
    }

    #[test]
    fn a_scope_that_takes_no_polygon_is_said_and_not_refused() {
        let mut t = fixture();
        add_parallel_planes(&mut t);
        t.ok("LINE 0,0 10,10");
        let reply = t.ok("VOLUME SURFACES ground design INSIDE FROM DRAWING");
        assert_eq!(value(&record(&reply, "polygons"), "used"), "0");
        assert_eq!(value(&record(&reply, "volume"), "ran"), "no");
    }

    #[test]
    fn a_csv_is_written_and_a_file_already_there_is_replaced_only_when_asked() {
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let file = scratch_file("written");
        remove(&file);
        let path = utf8(&file);
        let reply = t.ok(&format!("VOLUME SURFACES ground design csv=\"{path}\""));
        assert_eq!(value(&record(&reply, "csv"), "rows"), "1");
        let text = match std::fs::read(&file) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => panic!("{path}: {error}"),
        };
        let mut lines = text.split('\n');
        // The eight columns where they always were, then what the reply also
        // says: the areas (the whole 10 000 m2 is fill, the design 1 m above
        // the ground everywhere) and the method and the surfaces that made the
        // table.
        assert_eq!(
            lines.next().unwrap_or(""),
            "record,polygon,entity,polygon_area,area,cut,fill,net,cut_area,fill_area,method,\
             base,top\r"
        );
        assert_eq!(
            lines.next().unwrap_or(""),
            "total,,,,10000.000,0.000,10000.000,10000.000,0.000,10000.000,exact,ground,design\r"
        );
        assert_eq!(
            t.fails(&format!("VOLUME SURFACES ground design csv=\"{path}\"")),
            ErrorCode::AlreadyExists
        );
        t.ok(&format!(
            "VOLUME SURFACES ground design csv=\"{path}\" OVERWRITE"
        ));
        remove(&file);
    }

    #[test]
    fn what_cannot_be_measured_is_refused_saying_why() {
        let mut t = fixture();
        add_parallel_planes(&mut t);
        add_surface(
            &mut t,
            "far",
            grid_surface(500.0, 500.0, 10.0, 10.0, |_, _| 1.0),
        );
        assert_eq!(
            t.fails("VOLUME SURFACES ground nothing"),
            ErrorCode::NotFound
        );
        assert_eq!(
            t.fails("VOLUME SURFACES ground far"),
            ErrorCode::InvalidGeometry
        );
        t.ok("LINE 0,0 10,10");
        assert_eq!(
            t.fails(&format!(
                "VOLUME SURFACES ground design INSIDE #{}",
                last_id(&t)
            )),
            ErrorCode::InvalidGeometry
        ); // a line is no polygon
        assert_eq!(
            t.fails("VOLUME SURFACES ground design INSIDE #99999"),
            ErrorCode::NotFound
        );
        assert_eq!(
            t.fails("VOLUME SURFACES ground design INSIDE 0,0 10,0"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("VOLUME SURFACES ground design EACH"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("VOLUME SURFACES ground design interval=5"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("VOLUME SURFACES ground design bulk=0"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(t.fails("VOLUME SURFACES ground"), ErrorCode::ParseFailure);
        assert_eq!(
            t.fails("VOLUME SIDEWAYS ground design"),
            ErrorCode::ParseFailure
        );
        // A polygon wholly off the surfaces measures nothing, which is said.
        assert_eq!(
            t.fails("VOLUME SURFACES ground design INSIDE 200,200 210,200 210,210"),
            ErrorCode::InvalidGeometry
        );
    }

    // The lines of the table `line` writes to `file` (named `path` on the
    // line), the file removed first.
    fn csv(t: &mut Session, file: &Path, path: &str, line: &str) -> Vec<String> {
        remove(file);
        t.ok(&format!("{line} csv=\"{path}\""));
        lines_of(file)
    }

    #[test]
    fn a_table_names_its_method_and_inputs_and_carries_the_areas_levels_and_cells_of_the_reply() {
        // Parallel planes 1 m apart over [0, 100]^2, ground z = 10 + 0.05 x.
        // The columns after the eighth say what the reply says: for the exact
        // surfaces 0 of cut area and the whole 10 000 m2 of fill area, which
        // design 1 m above the ground everywhere gives; for a level the above
        // and below, and the level.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let file = scratch_file("columns");
        remove(&file);
        let path = utf8(&file);

        // The exact surfaces.
        let surfaces = csv(&mut t, &file, &path, "VOLUME SURFACES ground design");
        assert_eq!(surfaces.len(), 2);
        assert_eq!(
            surfaces[0],
            "record,polygon,entity,polygon_area,area,cut,fill,net,cut_area,fill_area,method,\
             base,top"
        );
        assert_eq!(
            surfaces[1],
            "total,,,,10000.000,0.000,10000.000,10000.000,0.000,10000.000,exact,ground,design"
        );

        // The grid: 10 m cells, a hundred of them, each 1 m deep: 10 000 m3 -
        // the same numbers as the exact file, and the method and the cell are
        // what tell the two apart.
        let grid = csv(&mut t, &file, &path, "VOLUME GRID ground design cell=10");
        assert_eq!(grid.len(), 2);
        assert_eq!(
            grid[0],
            "record,polygon,entity,polygon_area,area,cut,fill,net,cells,method,base,top,cell"
        );
        assert_eq!(
            grid[1],
            "total,,,,10000.000,0.000,10000.000,10000.000,100,grid,ground,design,10"
        );

        // A level: z = 10 + 0.05 x crosses 11 at x = 20. Above it, x from 20
        // to 100: 100 x (0.025 (100^2 - 20^2) - 80) = 100 x 160 = 16 000 m3
        // over 80 x 100 = 8000 m2; below it, 100 x (20 - 0.025 x 20^2) = 100 x
        // 10 = 1000 m3 over 2000 m2; net 1000 - 16000 = -15 000.
        let level = csv(&mut t, &file, &path, "VOLUME DATUM ground 11");
        assert_eq!(level.len(), 2);
        assert_eq!(
            level[0],
            "record,polygon,entity,polygon_area,area,cut,fill,net,cut_area,fill_area,above,\
             below,method,surface,level"
        );
        assert_eq!(
            level[1],
            "total,,,,10000.000,16000.000,1000.000,-15000.000,8000.000,2000.000,16000.000,\
             1000.000,exact,ground,11"
        );

        // The factors are columns too, after the quantities they adjust.
        let factored = csv(
            &mut t,
            &file,
            &path,
            "VOLUME SURFACES ground design bulk=1.25 shrink=1.1",
        );
        assert_eq!(factored.len(), 2);
        assert_eq!(
            factored[0],
            "record,polygon,entity,polygon_area,area,cut,fill,net,cut_adjusted,fill_adjusted,\
             net_adjusted,cut_area,fill_area,method,base,top,bulk,shrink"
        );
        assert_eq!(
            factored[1],
            "total,,,,10000.000,0.000,10000.000,10000.000,0.000,11000.000,11000.000,0.000,\
             10000.000,exact,ground,design,1.25,1.1"
        );
        remove(&file);
    }

    #[test]
    fn a_section_table_says_whether_its_volumes_are_end_area_or_prismoidal_and_what_made_them() {
        // Sections every 50 m along y = 50, 10 m each side of the parallel
        // planes 1 m apart: 20 m2 on each of the three, so 50 x 20 = 1000 m3 a
        // panel by either formula (Simpson's rule on a constant is the
        // constant). With method=prismoidal the plain cut and fill columns
        // hold the prismoidal volumes - which the method column is the only
        // thing to say - and the mass ordinate is the cut x bulk less the fill
        // x shrink: 0 - 1000 x 1.5, then -3000.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        t.ok("ALIGN NEW cl 0,50 100,50");
        let file = scratch_file("sections");
        remove(&file);
        let path = utf8(&file);
        t.ok(&format!(
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=50 left=10 right=10 \
             method=prismoidal bulk=1.2 shrink=1.5 csv=\"{path}\""
        ));
        let lines = lines_of(&file);
        assert_eq!(lines.len(), 4);
        // The curvature column says the figures were made with the correction
        // on (the default); on a straight it changes none of them.
        assert_eq!(
            lines[0],
            "station,key,cut_area,fill_area,width,cut,fill,cut_total,fill_total,mass,method,\
             curvature,base,top,alignment,interval,left,right,bulk,shrink"
        );
        assert_eq!(
            lines[1],
            "0.000,yes,0.000,20.000,20.000,,,,,,prismoidal,on,ground,design,cl,50,10.000,\
             10.000,1.2,1.5"
        );
        assert_eq!(
            lines[2],
            "50.000,no,0.000,20.000,20.000,0.000,1000.000,0.000,1000.000,-1500.000,prismoidal,\
             on,ground,design,cl,50,10.000,10.000,1.2,1.5"
        );
        assert_eq!(
            lines[3],
            "100.000,yes,0.000,20.000,20.000,0.000,1000.000,0.000,2000.000,-3000.000,\
             prismoidal,on,ground,design,cl,50,10.000,10.000,1.2,1.5"
        );
        remove(&file);
    }

    #[test]
    fn a_csv_with_a_scope_that_takes_no_polygon_writes_nothing_and_says_so() {
        // The file a run before left is as it was - even with OVERWRITE, as
        // nothing ran - and the reply names the file and says it was not
        // written, where it used to say nothing of it.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let file = scratch_file("norun");
        if let Err(error) = std::fs::write(&file, b"OLD TABLE FROM AN EARLIER RUN\r\n") {
            panic!("{}: {error}", file.display());
        }
        let path = utf8(&file);
        let reply = t.ok(&format!(
            "VOLUME SURFACES ground design INSIDE FROM AREA 500,500,600,600 csv=\"{path}\" \
             OVERWRITE"
        ));
        assert_eq!(value(&record(&reply, "volume"), "ran"), "no");
        let written = record(&reply, "csv");
        assert!(written.is_some(), "{reply}");
        assert_eq!(value(&written, "file"), path);
        assert_eq!(value(&written, "written"), "no");
        assert!(
            written
                .as_ref()
                .is_some_and(|csv| csv.value("rows").is_none())
        );
        let kept = lines_of(&file);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], "OLD TABLE FROM AN EARLIER RUN");
        // A file that was not there is not made.
        remove(&file);
        let again = t.ok(&format!(
            "VOLUME SURFACES ground design INSIDE FROM AREA 500,500,600,600 csv=\"{path}\""
        ));
        assert_eq!(value(&record(&again, "csv"), "written"), "no");
        assert!(!file.exists());
        // Asked of nothing, a line with no csv= says nothing of a file.
        assert!(
            record(
                &t.ok("VOLUME SURFACES ground design INSIDE FROM AREA 500,500,600,600"),
                "csv"
            )
            .is_none()
        );
    }

    #[test]
    fn a_total_says_which_base_a_stockpile_was_measured_against_and_which_heights_its_toe_used() {
        // The pyramid [0, 100]^2, apex 30 high at the centre: its height is
        // 0.6 min(x, y) on the lower-left quarter and 30 - 0.6 (max(x, y) -
        // 50) on the upper right. Against base=lowest the level is the lowest
        // point on the boundary: the square [10, 40]^2 has it at the corner
        // (10, 10), 0.6 x 10 = 6, and [55, 80]^2 at (80, 80), 30 - 0.6 x 30 =
        // 12. A toe typed with every height is its own; one with none uses the
        // ground's. The total of ONE polygon has what its EACH record has; of
        // two that differ it has the fields they share and EACH has the rest.
        let mut t = fixture();
        add_surface(&mut t, "pile", pyramid());
        let lower_left = "10,10 40,10 40,40 10,40";
        let upper_right = "55,55 80,55 80,80 55,80";
        assert_eq!(
            value(
                &record(
                    &t.ok(&format!(
                        "VOLUME STOCKPILE pile INSIDE {lower_left} base=lowest"
                    )),
                    "volume"
                ),
                "lowest"
            ),
            "6.000"
        );
        assert_eq!(
            value(
                &record(
                    &t.ok(&format!(
                        "VOLUME STOCKPILE pile INSIDE {upper_right} base=lowest"
                    )),
                    "volume"
                ),
                "lowest"
            ),
            "12.000"
        );
        assert_eq!(
            value(
                &record(
                    &t.ok(&format!("VOLUME STOCKPILE pile INSIDE {lower_left}")),
                    "volume"
                ),
                "heights"
            ),
            "surface"
        );
        assert_eq!(
            value(
                &record(
                    &t.ok("VOLUME STOCKPILE pile INSIDE 10,10,6 40,10,6 40,40,24 10,40,6"),
                    "volume"
                ),
                "heights"
            ),
            "own"
        );
        // Two polygons that differ in the level they found: no one value for
        // the total, and each polygon's own with EACH.
        t.ok(&format!("PLINE {lower_left} CLOSE"));
        let first = last_id(&t);
        t.ok(&format!("PLINE {upper_right} CLOSE"));
        let second = last_id(&t);
        let pair = format!(" INSIDE #{first} #{second}");
        let differing = t.ok(&format!("VOLUME STOCKPILE pile{pair} base=lowest"));
        let Some(total) = record(&differing, "volume") else {
            panic!("{differing}");
        };
        assert!(total.value("lowest").is_none(), "{differing}");
        let each = t.ok(&format!("VOLUME STOCKPILE pile{pair} base=lowest EACH"));
        let polygons = records(&each, "polygon");
        assert_eq!(polygons.len(), 2, "{each}");
        assert_eq!(field(&polygons[0], "lowest"), "6.000");
        assert_eq!(field(&polygons[1], "lowest"), "12.000");
        // Two that agree: the shared field is on the total.
        assert_eq!(
            value(
                &record(&t.ok(&format!("VOLUME STOCKPILE pile{pair}")), "volume"),
                "heights"
            ),
            "surface"
        );
    }

    #[test]
    fn a_refusal_that_two_surfaces_share_no_ground_names_what_each_covers() {
        // a is the triangle (0,0) (60,0) (0,60), b the triangle (100,100)
        // (40,100) (100,40): the boxes meet on [40, 60]^2 over triangles that
        // do not. The refusal names each surface and its extent, for every
        // form, so a person who has drawn one in the wrong coordinates sees
        // it.
        let mut t = fixture();
        let first = TinInput {
            points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(60.0, 0.0, 0.0),
                Point3::new(0.0, 60.0, 0.0),
            ],
            ..TinInput::default()
        };
        add_surface(&mut t, "a", built(&first));
        let second = TinInput {
            points: vec![
                Point3::new(100.0, 100.0, 1.0),
                Point3::new(40.0, 100.0, 1.0),
                Point3::new(100.0, 40.0, 1.0),
            ],
            ..TinInput::default()
        };
        add_surface(&mut t, "b", built(&second));
        t.ok("ALIGN NEW cl 0,50 100,50");
        let a = "a covers 0.000,0.000,60.000,60.000";
        let b = "b covers 40.000,40.000,100.000,100.000";
        for line in [
            "VOLUME SURFACES a b",
            "VOLUME GRID a b cell=1",
            "VOLUME SECTIONS a b ALIGNMENT cl",
            "SURFACE DIFFERENCE a b",
            "SURFACE ENVELOPE a b MAX",
        ] {
            let error = t.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidGeometry, "{line}");
            assert!(error.message.contains(a), "{line}: {}", error.message);
            assert!(error.message.contains(b), "{line}: {}", error.message);
        }
        // A polygon off a surface names that surface's extent.
        for line in [
            "VOLUME DATUM a 1 INSIDE 200,200 210,200 210,210",
            "VOLUME STOCKPILE a INSIDE 200,200 210,200 210,210 level=1",
        ] {
            let error = t.refused(line);
            assert!(error.message.contains(a), "{line}: {}", error.message);
        }
        // Boxes that do not meet name them as before.
        add_surface(
            &mut t,
            "far",
            grid_surface(500.0, 500.0, 10.0, 10.0, |_, _| 1.0),
        );
        let apart = t.refused("VOLUME SURFACES a far");
        assert!(
            apart
                .message
                .contains("far covers 500.000,500.000,510.000,510.000"),
            "{}",
            apart.message
        );
    }

    // ---- a lot with a hole, and a spline that returns to its start --------------------------

    // A square ring as IMPORT leaves one: its tag, `exterior` or `hole`, in
    // the entity's metadata - or untagged, when `tag` is None.
    fn ring_entity(x: f64, y: f64, side: f64, tag: Option<&str>) -> Entity {
        let mut entity = Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(x, y),
                    Point2::new(x + side, y),
                    Point2::new(x + side, y + side),
                    Point2::new(x, y + side),
                ],
                true,
            )),
            ..Entity::default()
        };
        if let Some(tag) = tag {
            entity
                .metadata
                .insert(String::from("source.ring"), PropertyValue::from(tag));
        }
        entity
    }

    #[test]
    fn a_lot_with_a_tagged_hole_is_measured_less_the_hole_by_every_method_that_takes_a_polygon() {
        // Parallel planes 1 m apart over [0, 100]^2, the ground z = 10 + 0.05
        // x. A lot [10, 60]^2 (50 x 50 = 2500) with a tagged hole [20, 30]^2
        // (10 x 10 = 100) in it.
        //   exact, 1 m deep:      2500 - 100 = 2400 m3 over 2400 m2;
        //   grid, 1 m cells:      centres at k + 0.5, 50 x 50 in the lot less
        //                         10 x 10 in the hole: 2400 cells, none on an
        //                         edge;
        //   to the level 10:      the ground is above it by 0.05 x, the
        //                         integral of x over the lot is 50 x (60^2 -
        //                         10^2) / 2 = 87 500, over the hole 10 x (30^2
        //                         - 20^2) / 2 = 2500: 0.05 x 85 000 = 4250 m3
        //                         above;
        //   stockpile, lowest:    the lowest point on the lot's edge is x =
        //                         10, 10.5, so the pile is 0.05 (x - 10) over
        //                         the lot and the hole, 50 x (50^2 / 2) = 62
        //                         500 less 10 x ((20^2 - 10^2) / 2) = 1500:
        //                         0.05 x 61 000 = 3050;
        //   stockpile, boundary:  a base made of the corners at the ground's
        //                         heights is the ground itself, plane for
        //                         plane: nothing above or below it, 2400 m2.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        ok(t.document.execute(create_entities(vec![
            ring_entity(10.0, 10.0, 50.0, Some("exterior")),
            ring_entity(20.0, 20.0, 10.0, Some("hole")),
        ])));
        let lot = t.document.last_created_entities()[0];

        let each = t.ok("VOLUME SURFACES ground design INSIDE FROM DRAWING EACH");
        let used = record(&each, "polygons");
        assert_eq!(value(&used, "used"), "1");
        assert_eq!(value(&used, "holes"), "1");
        assert_eq!(value(&used, "skipped.other"), "0");
        let polygon = record(&each, "polygon");
        assert_eq!(value(&polygon, "entity"), format!("{lot}"));
        assert_eq!(value(&polygon, "polygon.area"), "2400.000");
        assert_eq!(value(&polygon, "holes"), "1");
        assert_eq!(value(&polygon, "fill"), "2400.000");
        let total = record(&each, "volume");
        assert_eq!(value(&total, "polygon.area"), "2400.000");
        assert_eq!(value(&total, "fill"), "2400.000");
        assert_eq!(value(&total, "area"), "2400.000");
        assert!(record(&each, "warning").is_none());

        let grid = record(
            &t.ok("VOLUME GRID ground design cell=1 INSIDE FROM DRAWING"),
            "volume",
        );
        assert_eq!(value(&grid, "cells"), "2400");
        assert_eq!(value(&grid, "fill"), "2400.000");

        let level = record(
            &t.ok("VOLUME DATUM ground 10 INSIDE FROM DRAWING"),
            "volume",
        );
        assert_eq!(value(&level, "above"), "4250.000");
        assert_eq!(value(&level, "area"), "2400.000");

        let lowest = record(
            &t.ok("VOLUME STOCKPILE ground INSIDE FROM DRAWING base=lowest"),
            "volume",
        );
        assert_eq!(value(&lowest, "above"), "3050.000");
        assert_eq!(value(&lowest, "lowest"), "10.500");
        assert_eq!(value(&lowest, "area"), "2400.000");

        let boundary = record(
            &t.ok("VOLUME STOCKPILE ground INSIDE FROM DRAWING"),
            "volume",
        );
        assert_eq!(value(&boundary, "above"), "0.000");
        assert_eq!(value(&boundary, "below"), "0.000");
        assert_eq!(value(&boundary, "area"), "2400.000");
    }

    #[test]
    fn a_ring_not_tagged_as_a_hole_is_a_polygon_of_its_own_and_overlaps_the_lot() {
        // The same lot with the inner ring untagged is a building in the lot:
        // two polygons that overlap by 100 m2, which one total would count
        // twice.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        ok(t.document.execute(create_entities(vec![
            ring_entity(10.0, 10.0, 50.0, Some("exterior")),
            ring_entity(20.0, 20.0, 10.0, None),
        ])));
        assert_eq!(
            t.fails("VOLUME SURFACES ground design INSIDE FROM DRAWING"),
            ErrorCode::InvalidGeometry
        );
        let each = t.ok("VOLUME SURFACES ground design INSIDE FROM DRAWING EACH");
        assert_eq!(value(&record(&each, "polygons"), "used"), "2");
        assert_eq!(value(&record(&each, "polygons"), "holes"), "0");
    }

    #[test]
    fn a_tagged_hole_in_no_lot_of_the_scope_is_measured_as_a_polygon_and_the_reply_warns_of_it() {
        // A hole [20, 30]^2 whose lot is not in the scope (only the hole is on
        // the layer asked for, or is named): it is the polygon asked for, 100
        // m2, and the reply says one ring tagged as a hole was left as a
        // polygon of its own.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        ok(t.document.execute(create_entities(vec![ring_entity(
            20.0,
            20.0,
            10.0,
            Some("hole"),
        )])));
        let reply = t.ok("VOLUME SURFACES ground design INSIDE FROM DRAWING");
        assert_eq!(value(&record(&reply, "polygons"), "used"), "1");
        assert_eq!(value(&record(&reply, "polygons"), "holes"), "0");
        assert_eq!(value(&record(&reply, "warning"), "holes.alone"), "1");
        assert_eq!(value(&record(&reply, "volume"), "fill"), "100.000");
    }

    #[test]
    fn a_spline_drawn_back_to_its_start_is_a_volume_polygon_and_an_open_one_is_counted_as_other() {
        // A circle of radius 10 about (50, 50) as a closed spline: 100 pi =
        // 314.159 m2 of the 1 m slab less what its chords cut off, at most the
        // perimeter (62.83) times the 1 mm they may stand off it, 0.063 - 0.1
        // allows a chord's farthest point to be off its middle. An open spline
        // in the same scope bounds nothing: skipped.other, as before.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let closed = Entity {
            geometry: Geometry::Spline(circle_spline(Point2::new(50.0, 50.0), 10.0)),
            ..Entity::default()
        };
        let open_spline = must(Spline2::from_control_points(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0),
            ],
            2,
        ));
        let open = Entity {
            geometry: Geometry::Spline(open_spline),
            ..Entity::default()
        };
        ok(t.document.execute(create_entities(vec![closed, open])));

        let reply = t.ok("VOLUME SURFACES ground design INSIDE FROM DRAWING EACH");
        let used = record(&reply, "polygons");
        assert_eq!(value(&used, "used"), "1");
        assert_eq!(value(&used, "skipped.other"), "1");
        assert_eq!(value(&used, "skipped.open"), "0");
        let polygon = record(&reply, "polygon");
        assert_eq!(value(&polygon, "chorded"), "yes");
        let Some(area) = parse_finite_double(value(&polygon, "polygon.area")) else {
            panic!("{reply}");
        };
        assert!(area <= 100.0 * PI);
        assert!(area >= 100.0 * PI - 0.1);
        assert_eq!(value(&polygon, "fill"), value(&polygon, "polygon.area")); // 1 m deep
        // Named by id, the open one is refused as what it is.
        let open_id = last_id(&t);
        let refusal = t.refused(&format!("VOLUME SURFACES ground design INSIDE #{open_id}"));
        assert_eq!(refusal.code, ErrorCode::InvalidGeometry);
        assert!(
            refusal.message.contains("an open spline"),
            "{}",
            refusal.message
        );
    }

    // ---- VOLUME DATUM and STOCKPILE ---------------------------------------------------------

    #[test]
    fn a_tilted_plane_against_a_level_inside_a_rectangle_is_cut_east_and_fill_west() {
        // z = 10 + 0.05 x against the level 11 inside [0, 40] x [0, 10]: the
        // plane crosses 11 at x = 20. Above: 10 x the integral of 0.05 x - 1
        // from 20 to 40 = 10 x 10 = 100; below the same, from 0 to 20.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let total = record(
            &t.ok("VOLUME DATUM ground 11 INSIDE 0,0 40,0 40,10 0,10"),
            "volume",
        );
        assert_eq!(value(&total, "cut"), "100.000");
        assert_eq!(value(&total, "above"), "100.000");
        assert_eq!(value(&total, "fill"), "100.000");
        assert_eq!(value(&total, "below"), "100.000");
        assert_eq!(value(&total, "net"), "0.000");
        assert_eq!(value(&total, "area"), "400.000");
        assert_eq!(value(&total, "level"), "11");
        assert_eq!(t.fails("VOLUME DATUM ground high"), ErrorCode::ParseFailure);
    }

    #[test]
    fn a_pyramid_stockpile_is_a_third_of_its_base_times_its_height_against_each_kind_of_base() {
        // The pyramid's toe is the square [0, 100]^2 at 0: 100 000 m^3 above a
        // base of the toe's heights, and above its lowest point (0) the same;
        // above level -1, 10 000 m^2 more of 1 m: 110 000.
        let mut t = fixture();
        add_surface(&mut t, "pile", pyramid());
        let toe = " INSIDE 0,0 100,0 100,100 0,100";
        let boundary = record(&t.ok(&format!("VOLUME STOCKPILE pile{toe}")), "volume");
        assert_eq!(value(&boundary, "above"), "100000.000");
        assert_eq!(value(&boundary, "cut"), "100000.000");
        assert_eq!(value(&boundary, "below"), "0.000");
        assert_eq!(value(&boundary, "base"), "boundary");
        let lowest = record(
            &t.ok(&format!("VOLUME STOCKPILE pile{toe} base=lowest")),
            "volume",
        );
        assert_eq!(value(&lowest, "above"), "100000.000");
        let level = record(
            &t.ok(&format!("VOLUME STOCKPILE pile{toe} level=-1")),
            "volume",
        );
        assert_eq!(value(&level, "above"), "110000.000");
        assert_eq!(value(&level, "base"), "level");
        assert_eq!(t.fails("VOLUME STOCKPILE pile"), ErrorCode::ParseFailure);
        assert_eq!(
            t.fails(&format!("VOLUME STOCKPILE pile{toe} base=middle")),
            ErrorCode::InvalidArgument
        );
    }

    // ---- VOLUME SECTIONS and GRID -----------------------------------------------------------

    #[test]
    fn sections_along_an_alignment_give_the_end_area_the_prismoidal_and_the_mass_haul() {
        // The wedge: the ground z = 0 and over it the triangle (0,0),
        // (100,-10), (100,10) of z = 0.1 x, so a section at s is s / 5 wide
        // and 0.1 s deep, s^2 / 50. Every 10 m: end areas 6700, prismoidal 20
        // 000 / 3 = 6666.667, the correction 33.333. All fill, so the mass
        // ordinate falls to -6700.
        let mut t = fixture();
        add_surface(
            &mut t,
            "ground",
            grid_surface(-20.0, -60.0, 140.0, 140.0, |_, _| 0.0),
        );
        let wedge = TinInput {
            points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(100.0, -10.0, 10.0),
                Point3::new(100.0, 10.0, 10.0),
            ],
            ..TinInput::default()
        };
        add_surface(&mut t, "wedge", built(&wedge));
        t.ok("ALIGN NEW cl 0,0 100,0");
        let reply = t.ok(
            "VOLUME SECTIONS ground wedge ALIGNMENT cl interval=10 left=20 right=20 method=both",
        );
        let sections = records(&reply, "section");
        assert_eq!(sections.len(), 11, "{reply}");
        assert_eq!(field(&sections[0], "station"), "0.000");
        assert_eq!(field(&sections[0], "key"), "yes");
        assert_eq!(field(&sections[5], "fill.area"), "50.000"); // 50^2 / 50
        assert_eq!(field(&sections[10], "mass"), "-6700.000");
        assert_eq!(field(&sections[10], "fill.total"), "6700.000");
        let totals = records(&reply, "volume");
        assert_eq!(totals.len(), 2);
        assert_eq!(field(&totals[0], "method"), "endarea");
        assert_eq!(field(&totals[0], "fill"), "6700.000");
        assert_eq!(field(&totals[1], "method"), "prismoidal");
        assert_eq!(field(&totals[1], "fill"), "6666.667");
        assert_eq!(field(&totals[1], "correction.fill"), "33.333");
        // The exact overlay agrees with the prismoidal: 0.1 x 1000 x 200 / 3.
        assert_eq!(
            value(
                &record(&t.ok("VOLUME SURFACES ground wedge"), "volume"),
                "fill"
            ),
            "6666.667"
        );

        assert_eq!(
            t.fails("VOLUME SECTIONS ground wedge ALIGNMENT nowhere"),
            ErrorCode::NotFound
        );
        assert_eq!(
            t.fails("VOLUME SECTIONS ground wedge ALIGNMENT cl method=simpson"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("VOLUME SECTIONS ground wedge ALIGNMENT cl interval=0"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_slab_measured_northwards_is_as_whole_as_eastwards_where_its_end_sections_lie_on_its_edges()
    {
        // 1 m over [0, 100]^2 is 10 000 m3. The sections at stations 0 and 100
        // of a north-going alignment lie on the slab's south and north edges,
        // and the tilt of cos(pi/2) in their ends used to halve them: 9500.
        let mut t = fixture();
        add_surface(
            &mut t,
            "lo",
            grid_surface(0.0, 0.0, 100.0, 100.0, |_, _| 10.0),
        );
        add_surface(
            &mut t,
            "hi",
            grid_surface(0.0, 0.0, 100.0, 100.0, |_, _| 11.0),
        );
        t.ok("ALIGN NEW north 50,0 50,100");
        t.ok("ALIGN NEW east 0,50 100,50");
        for alignment in ["north", "east"] {
            let reply = t.ok(&format!(
                "VOLUME SECTIONS lo hi ALIGNMENT {alignment} interval=10"
            ));
            let sections = records(&reply, "section");
            assert_eq!(sections.len(), 11, "{alignment}");
            assert_eq!(field(&sections[0], "width"), "100.000", "{alignment}");
            assert_eq!(field(&sections[10], "width"), "100.000", "{alignment}");
            assert_eq!(
                value(&record(&reply, "volume"), "fill"),
                "10000.000",
                "{alignment}"
            );
            assert_eq!(
                value(&record(&reply, "volume"), "sections.empty"),
                "0",
                "{alignment}"
            );
        }
    }

    #[test]
    fn sections_round_a_curve_are_refused_until_their_reach_is_short_of_its_radius() {
        // A 1 m slab over [0, 200]^2 is 40 000 m3 (VOLUME SURFACES). The
        // alignment 20,20 to 180,20 to 180,180 with a curve of radius 40 is
        // 240 + 20 pi long; with sections across all the common ground (the
        // default, 283 m) the sections cross one another and the sum was 62
        // 101, which is no volume there is. A band 15 m either side is 30 x
        // (240 + 20 pi) = 9084.956, whatever the interval.
        let mut t = fixture();
        add_surface(
            &mut t,
            "lo",
            grid_surface(0.0, 0.0, 200.0, 200.0, |_, _| 10.0),
        );
        add_surface(
            &mut t,
            "hi",
            grid_surface(0.0, 0.0, 200.0, 200.0, |_, _| 11.0),
        );
        t.ok("ALIGN NEW cl 20,20 180,20 180,180");
        t.ok("ALIGN SET cl 1 40");
        assert_eq!(
            value(&record(&t.ok("VOLUME SURFACES lo hi"), "volume"), "fill"),
            "40000.000"
        );
        assert_eq!(
            t.fails("VOLUME SECTIONS lo hi ALIGNMENT cl"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("VOLUME SECTIONS lo hi ALIGNMENT cl interval=1 left=100 right=100"),
            ErrorCode::InvalidArgument
        );
        for interval in ["10", "1", "0.1"] {
            let band = record(
                &t.ok(&format!(
                    "VOLUME SECTIONS lo hi ALIGNMENT cl interval={interval} left=15 right=15"
                )),
                "volume",
            );
            assert_eq!(value(&band, "fill"), "9084.956", "{interval}");
        }
    }

    #[test]
    fn sections_round_a_curve_are_corrected_for_the_centroid_say_by_so_much_and_off_gives_the_plain_figure()
     {
        // A 1 m slab over [-50, 150]^2 and the alignment 0,0 - 100,0 - 100,100
        // with a curve of radius 50 at the PI: 50 m of tangent, a quarter
        // circle about (50, 50) on the left, 50 m of tangent. A strip 10 wide
        // on the inside (left=10 right=0) holds the tangents' 2 x 500 and the
        // annulus between the radii 40 and 50, (pi/4)(2500 - 1600) = 225 pi:
        // 1000 + 706.858 = 1706.858. The plain end area, 10 m2 over the
        // centreline's length 100 + 25 pi, is 1000 + 250 pi = 1785.398, 78.540
        // = 25 pi too much, which is what the record says it moved. On the
        // outside (left=0 right=10) the annulus is (pi/4)(3600 - 2500) = 275
        // pi: 1863.938, and the correction adds 25 pi. A strip 10 each side is
        // symmetric about the centreline, 2000 + 500 pi = 3570.796 either way.
        let mut t = fixture();
        add_surface(
            &mut t,
            "lo",
            grid_surface(-50.0, -50.0, 200.0, 50.0, |_, _| 0.0),
        );
        add_surface(
            &mut t,
            "hi",
            grid_surface(-50.0, -50.0, 200.0, 50.0, |_, _| 1.0),
        );
        t.ok("ALIGN NEW cl 0,0 100,0 100,100");
        t.ok("ALIGN SET cl 1 50");
        let head = "VOLUME SECTIONS lo hi ALIGNMENT cl interval=10 ";

        let inside = t.ok(&format!("{head}left=10 right=0"));
        let total = record(&inside, "volume");
        assert_eq!(value(&total, "fill"), "1706.858");
        assert_eq!(value(&total, "cut"), "0.000");
        assert_eq!(value(&total, "curvature"), "on");
        assert_eq!(value(&total, "curvature.fill"), "-78.540");
        assert_eq!(value(&total, "curvature.cut"), "0.000");
        // A panel on the arc says its radius and which way it turns and what
        // it was moved by: the one that ends at station 70 is 10 long, A = 10,
        // M = 50, so 10 x (20 - 0.02 x 100) / 2 = 90 where the plain figure is
        // 100.
        let sections = records(&inside, "section");
        let mut curved = 0;
        for section in &sections {
            if let Some(radius) = section.value("radius") {
                curved += 1;
                assert_eq!(radius, "50.000");
                assert_eq!(field(section, "turn"), "left");
            }
            if field(section, "station") == "70.000" {
                assert_eq!(field(section, "fill"), "90.000");
                assert_eq!(field(section, "curvature.fill"), "-10.000");
                assert_eq!(field(section, "curvature.cut"), "0.000");
            }
            if field(section, "station") == "40.000" {
                // On a tangent: no radius, no turn, no correction to name.
                assert!(section.value("radius").is_none());
                assert!(section.value("curvature.fill").is_none());
            }
        }
        assert!(curved > 5);

        let off = record(
            &t.ok(&format!("{head}left=10 right=0 curvature=off")),
            "volume",
        );
        assert_eq!(value(&off, "fill"), "1785.398");
        assert_eq!(value(&off, "curvature"), "off");
        assert_eq!(value(&off, "curvature.fill"), "<none>"); // nothing was moved
        // Any case, and the default is on.
        assert_eq!(
            value(
                &record(
                    &t.ok(&format!("{head}left=10 right=0 curvature=ON")),
                    "volume"
                ),
                "fill"
            ),
            "1706.858"
        );

        let outside = record(&t.ok(&format!("{head}left=0 right=10")), "volume");
        assert_eq!(value(&outside, "fill"), "1863.938");
        assert_eq!(value(&outside, "curvature.fill"), "78.540");
        let symmetric = record(&t.ok(&format!("{head}left=10 right=10")), "volume");
        assert_eq!(value(&symmetric, "fill"), "3570.796");
        assert_eq!(value(&symmetric, "curvature.fill"), "0.000");
        assert_eq!(
            value(
                &record(
                    &t.ok(&format!("{head}left=10 right=10 curvature=off")),
                    "volume"
                ),
                "fill"
            ),
            "3570.796"
        );

        // Prismoidal takes the same correction on the same constant area, and
        // its record says by how much too.
        let prismoidal = record(
            &t.ok(&format!("{head}left=10 right=0 method=prismoidal")),
            "volume",
        );
        assert_eq!(value(&prismoidal, "fill"), "1706.858");
        assert_eq!(value(&prismoidal, "curvature.fill"), "-78.540");

        // A word that is neither on nor off is refused by name, and the other
        // forms take no curvature=.
        assert_eq!(
            t.fails(&format!("{head}left=10 right=0 curvature=maybe")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            t.fails("VOLUME SURFACES lo hi curvature=off"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn on_a_straight_alignment_the_curvature_word_changes_nothing_and_the_table_says_which_it_was()
    {
        // Parallel planes 1 m apart, sections 13 m across (left=10 right=3)
        // every 50 m along y = 50: 13 m2 on each of three, 50 x 13 = 650 a
        // panel, 1300; the reply says on and a correction of nothing, off says
        // off, and the section table has a column that tells a file made one
        // way from the other.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        t.ok("ALIGN NEW cl 0,50 100,50");
        let line = "VOLUME SECTIONS ground design ALIGNMENT cl interval=50 left=10 right=3";
        let on = record(&t.ok(line), "volume");
        assert_eq!(value(&on, "fill"), "1300.000");
        assert_eq!(value(&on, "curvature"), "on");
        assert_eq!(value(&on, "curvature.fill"), "0.000");
        let file = scratch_file("curvature");
        remove(&file);
        let path = utf8(&file);
        let off = record(
            &t.ok(&format!("{line} curvature=off csv=\"{path}\"")),
            "volume",
        );
        assert_eq!(value(&off, "fill"), "1300.000");
        assert_eq!(value(&off, "curvature"), "off");
        let lines = lines_of(&file);
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains(",method,curvature,base,"), "{}", lines[0]);
        assert!(
            lines[3].contains(",endarea,off,ground,design,cl,50,10.000,3.000"),
            "{}",
            lines[3]
        );
        remove(&file);
    }

    #[test]
    fn sections_of_two_legs_that_run_side_by_side_are_refused_until_their_reach_clears_one_another()
    {
        // East 100 m, north 15 m, west 100 m: the legs are 15 apart, so a reach
        // of 10 each way sweeps y = 5 to 10 along both, and no section of one
        // crosses a section of the other. Refused, naming the two stretches by
        // station; 7 each way (under half the 15) is measured.
        let mut t = fixture();
        add_surface(
            &mut t,
            "lo",
            grid_surface(-50.0, -50.0, 200.0, 50.0, |_, _| 0.0),
        );
        add_surface(
            &mut t,
            "hi",
            grid_surface(-50.0, -50.0, 200.0, 50.0, |_, _| 1.0),
        );
        t.ok("ALIGN NEW uturn 0,0 100,0 100,15 0,15");
        let refused =
            t.refused("VOLUME SECTIONS lo hi ALIGNMENT uturn interval=20 left=10 right=10");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        let message = &refused.message;
        assert!(message.contains("side by side"), "{message}");
        assert!(message.contains("from station 0.000"), "{message}");
        assert!(message.contains("to 215.000"), "{message}");
        assert!(message.contains("left= and right="), "{message}");
        let measured = record(
            &t.ok("VOLUME SECTIONS lo hi ALIGNMENT uturn interval=20 left=7 right=7"),
            "volume",
        );
        assert_eq!(value(&measured, "sections.empty"), "0");
    }

    #[test]
    fn sections_of_an_alignment_that_runs_clear_of_the_surfaces_are_refused_naming_everything() {
        let mut t = fixture();
        add_parallel_planes(&mut t); // [0, 100]^2
        t.ok("ALIGN NEW away 500,500 600,500");
        let error = t.refused("VOLUME SECTIONS ground design ALIGNMENT away");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        // Which alignment, which surfaces and what each covers.
        for named in [
            "alignment away",
            "ground covers 0.000,0.000,100.000,100.000",
            "design covers 0.000,0.000,100.000,100.000",
        ] {
            assert!(
                error.message.contains(named),
                "{named} in: {}",
                error.message
            );
        }
    }

    #[test]
    fn the_grid_overstates_the_pyramid_by_twenty_cells_squared() {
        // terrain's GridVolume test works it: on 5 m cells the midpoint rule
        // is 20 x 5^2 = 500 over the exact 100 000, on 400 cells.
        let mut t = fixture();
        add_surface(
            &mut t,
            "flat",
            grid_surface(0.0, 0.0, 100.0, 100.0, |_, _| 0.0),
        );
        add_surface(&mut t, "pile", pyramid());
        let grid = record(&t.ok("VOLUME GRID flat pile cell=5"), "volume");
        assert_eq!(value(&grid, "method"), "grid");
        assert_eq!(value(&grid, "fill"), "100500.000");
        assert_eq!(value(&grid, "cells"), "400");
        assert_eq!(t.fails("VOLUME GRID flat pile"), ErrorCode::InvalidArgument); // no cell=
    }

    #[test]
    fn lots_that_share_an_edge_on_a_grid_line_are_counted_once_by_the_grid_as_by_the_exact_method()
    {
        // 1 m over [0, 50]^2, 1 m cells with centres at k + 0.5. Lots [5,
        // 25.5] x [5, 45] and [25.5, 45] x [5, 45] share the edge x = 25.5, a
        // line of centres: 840 + 800 = 1640 cells when each is measured on its
        // own and summed, the 40 on the edge twice. Their union [5, 45]^2
        // holds 40 x 40 = 1600 cells, and 1600 m2 is also what the exact
        // method gives.
        let mut t = fixture();
        add_surface(
            &mut t,
            "lo",
            grid_surface(0.0, 0.0, 50.0, 50.0, |_, _| 10.0),
        );
        add_surface(
            &mut t,
            "hi",
            grid_surface(0.0, 0.0, 50.0, 50.0, |_, _| 11.0),
        );
        t.ok("PLINE 5,5 25.5,5 25.5,45 5,45 CLOSE");
        let lot_a = last_id(&t);
        t.ok("PLINE 25.5,5 45,5 45,45 25.5,45 CLOSE");
        let lot_b = last_id(&t);
        t.ok("PLINE 5,5 45,5 45,45 5,45 CLOSE");
        let lots = last_id(&t);
        let pair = format!(" INSIDE #{lot_a} #{lot_b}");
        let grid = record(&t.ok(&format!("VOLUME GRID lo hi cell=1{pair}")), "volume");
        assert_eq!(value(&grid, "cells"), "1600");
        assert_eq!(value(&grid, "fill"), "1600.000");
        assert_eq!(value(&grid, "area"), "1600.000");
        assert_eq!(value(&grid, "polygon.area"), "1600.000");
        let whole = record(
            &t.ok(&format!("VOLUME GRID lo hi cell=1 INSIDE #{lots}")),
            "volume",
        );
        assert_eq!(value(&whole, "cells"), "1600");
        assert_eq!(
            value(
                &record(&t.ok(&format!("VOLUME SURFACES lo hi{pair}")), "volume"),
                "fill"
            ),
            "1600.000"
        );
        // Other cells on the same lots: the two lots together always hold the
        // cells of their union, none twice.
        for cell in ["2", "0.5", "5"] {
            let paired_lots = record(
                &t.ok(&format!("VOLUME GRID lo hi cell={cell}{pair}")),
                "volume",
            );
            let whole_lots = record(
                &t.ok(&format!("VOLUME GRID lo hi cell={cell} INSIDE #{lots}")),
                "volume",
            );
            assert_eq!(
                value(&paired_lots, "cells"),
                value(&whole_lots, "cells"),
                "{cell}"
            );
            assert_eq!(
                value(&paired_lots, "fill"),
                value(&whole_lots, "fill"),
                "{cell}"
            );
        }
    }

    #[test]
    fn overlapping_polygons_on_the_grid_are_one_union_and_said_so_and_each_is_refused_by_name() {
        // [5, 30] x [5, 45] and [20, 45] x [5, 45] overlap on [20, 30]. The
        // grid counts each cell once, 1600 of them; the exact method refuses
        // the pair without EACH. polygon.area= is the two polygons' own, 1000
        // + 1000 cells of 1 m2 - the overlap twice, which the warning says.
        let mut t = fixture();
        add_surface(
            &mut t,
            "lo",
            grid_surface(0.0, 0.0, 50.0, 50.0, |_, _| 10.0),
        );
        add_surface(
            &mut t,
            "hi",
            grid_surface(0.0, 0.0, 50.0, 50.0, |_, _| 11.0),
        );
        t.ok("PLINE 5,5 30,5 30,45 5,45 CLOSE");
        let first = last_id(&t);
        t.ok("PLINE 20,5 45,5 45,45 20,45 CLOSE");
        let second = last_id(&t);
        let pair = format!(" INSIDE #{first} #{second}");
        let reply = t.ok(&format!("VOLUME GRID lo hi cell=1{pair}"));
        let grid = record(&reply, "volume");
        assert_eq!(value(&grid, "cells"), "1600");
        assert_eq!(value(&grid, "fill"), "1600.000");
        assert_eq!(value(&grid, "polygon.area"), "2000.000");
        assert!(record(&reply, "warning").is_some(), "{reply}");
        assert_eq!(
            t.fails(&format!("VOLUME SURFACES lo hi{pair}")),
            ErrorCode::InvalidGeometry
        );

        // EACH is refused by GRID, and the refusal does not send a person to a
        // form that refuses it: it names the forms that take it.
        let each = t.refused(&format!("VOLUME GRID lo hi cell=1{pair} EACH"));
        assert_eq!(each.code, ErrorCode::ParseFailure);
        assert!(
            each.message.contains("DATUM and STOCKPILE take it"),
            "{}",
            each.message
        );
    }

    // ---- SURFACE DIFFERENCE and ENVELOPE ----------------------------------------------------

    #[test]
    fn the_difference_is_a_new_cut_fill_surface_in_one_undo_step() {
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let preview = t.ok("SURFACE DIFFERENCE ground design PREVIEW");
        assert_eq!(value(&record(&preview, "surface"), "preview"), "yes");
        assert!(t.document.surfaces().find("design-ground").is_none());

        let reply = t.ok("SURFACE DIFFERENCE ground design");
        let made = record(&reply, "surface");
        assert_eq!(value(&made, "name"), "design-ground");
        assert_eq!(value(&made, "zmin"), "1.000");
        assert_eq!(value(&made, "zmax"), "1.000");
        let Some(difference) = t.document.surfaces().find("design-ground") else {
            panic!("no surface design-ground");
        };
        assert_eq!(difference.style, "Cut Fill");
        // Integrated at 0 it is the comparison's fill.
        assert_eq!(
            value(
                &record(&t.ok("VOLUME DATUM design-ground 0"), "volume"),
                "above"
            ),
            "10000.000"
        );

        assert_eq!(
            t.fails("SURFACE DIFFERENCE ground design NAME design-ground"),
            ErrorCode::AlreadyExists
        );
        t.ok("UNDO");
        assert!(t.document.surfaces().find("design-ground").is_none());
    }

    #[test]
    fn the_upper_envelope_of_crossing_planes_is_the_higher_of_them() {
        // max(10, 10 + 0.1 (x - 50)) over [0, 100]^2 stands 12 500 above 10:
        // the triangle of 0.1 x 50^2 / 2 along x, 100 along y.
        let mut t = fixture();
        add_surface(
            &mut t,
            "ground",
            grid_surface(0.0, 0.0, 100.0, 25.0, |_, _| 10.0),
        );
        add_surface(
            &mut t,
            "design",
            grid_surface(0.0, 0.0, 100.0, 25.0, |x, _| 10.0 + 0.1 * (x - 50.0)),
        );
        let reply = t.ok("SURFACE ENVELOPE ground design MAX NAME top");
        assert_eq!(value(&record(&reply, "envelope"), "side"), "max");
        let volume = record(&t.ok("VOLUME DATUM top 10"), "volume");
        assert_eq!(value(&volume, "above"), "12500.000");
        assert_eq!(value(&volume, "below"), "0.000");
        assert_eq!(
            t.fails("SURFACE ENVELOPE ground design HIGHEST"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            t.fails("SURFACE ENVELOPE ground design"),
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn the_envelope_is_drawn_in_the_default_style_not_the_cut_fill_map_of_a_difference() {
        // An envelope holds real levels (here 10 to 15), not a difference about
        // 0: in the Cut Fill style, whose ramp ends at +2, every face past the
        // top stop would be drawn in the one colour of fill. Only the
        // difference is a map.
        let mut t = fixture();
        add_surface(
            &mut t,
            "ground",
            grid_surface(0.0, 0.0, 100.0, 25.0, |_, _| 10.0),
        );
        add_surface(
            &mut t,
            "design",
            grid_surface(0.0, 0.0, 100.0, 25.0, |x, _| 10.0 + 0.1 * (x - 50.0)),
        );
        let upper = t.ok("SURFACE ENVELOPE ground design MAX NAME upper");
        let Some(envelope) = t.document.surfaces().find("upper") else {
            panic!("no surface upper");
        };
        assert!(envelope.style.is_empty(), "{}", envelope.style);
        assert_eq!(
            value(&record(&upper, "surface"), "style"),
            t.document.surface_styles().default_style
        );
        assert_eq!(value(&record(&upper, "surface"), "zmax"), "15.000");

        let lower = t.ok("SURFACE ENVELOPE ground design MIN NAME lower");
        assert_ne!(value(&record(&lower, "surface"), "style"), "Cut Fill");

        // The difference of the same two is the map.
        let map = t.ok("SURFACE DIFFERENCE ground design NAME map");
        assert_eq!(value(&record(&map, "surface"), "style"), "Cut Fill");
        let Some(stored) = t.document.surfaces().find("map") else {
            panic!("no surface map");
        };
        assert_eq!(stored.style, "Cut Fill");
    }

    // SURFACE INFO is a geoprocessing verb of katana_app (it lives with LIST
    // and REMOVE beside the GDAL ones), so this layer cannot run it: it
    // answers with cad's surface_record of the stored surface, and so is that
    // applied to the surface the verb stored.
    #[track_caller]
    fn info_of(document: &Document, name: &str) -> String {
        match document.surfaces().find(name) {
            Some(stored) => surface_record(stored, document.surface_styles()),
            None => panic!("no surface {name}"),
        }
    }

    #[track_caller]
    fn same_as_info(document: &Document, reply: &str, name: &str) {
        let made = record(reply, "surface");
        let info = record(&info_of(document, name), "surface");
        assert!(made.is_some(), "{reply}");
        assert!(info.is_some());
        for key in [
            "name",
            "triangles",
            "points",
            "bounds",
            "zmin",
            "zmax",
            "source",
            "style",
            "visible",
        ] {
            assert_eq!(value(&made, key), value(&info, key), "{key} of {name}");
        }
        assert_eq!(value(&made, "visible"), "yes");
    }

    #[test]
    fn a_surface_these_verbs_make_is_reported_as_surface_info_reports_it_with_or_without_cut_fill()
    {
        // One surface record for every reply that names a surface
        // (surface_record): the same keys and values the INFO of the new
        // surface gives, visible= among them, and a style that names a style -
        // the drawing's default once Cut Fill is gone, not a word that names
        // none.
        let mut t = fixture();
        add_parallel_planes(&mut t);
        let upper = t.ok("SURFACE ENVELOPE ground design MAX NAME upper");
        same_as_info(&t.document, &upper, "upper");
        let with_cut_fill = t.ok("SURFACE DIFFERENCE ground design NAME withCutFill");
        same_as_info(&t.document, &with_cut_fill, "withCutFill");

        // A style a surface is drawn in cannot be deleted: the map goes first.
        t.ok("UNDO");
        assert!(t.document.surfaces().find("withCutFill").is_none());
        t.ok("SURFACESTYLE DELETE \"Cut Fill\"");
        let preview = t.ok("SURFACE DIFFERENCE ground design NAME without PREVIEW");
        assert_eq!(value(&record(&preview, "surface"), "preview"), "yes");
        assert_eq!(
            value(&record(&preview, "surface"), "style"),
            t.document.surface_styles().default_style
        );
        assert_eq!(value(&record(&preview, "surface"), "visible"), "yes");
        let without = t.ok("SURFACE DIFFERENCE ground design NAME without");
        same_as_info(&t.document, &without, "without");
        assert_ne!(
            value(
                &record(&info_of(&t.document, "without"), "surface"),
                "style"
            ),
            "default"
        );
    }

    #[test]
    fn help_volume_gives_every_form() {
        let mut t = fixture();
        let help = t.ok("HELP VOLUME");
        for form in [
            "VOLUME SURFACES",
            "VOLUME DATUM",
            "VOLUME STOCKPILE",
            "VOLUME SECTIONS",
            "VOLUME GRID",
            "SURFACE DIFFERENCE",
            "SURFACE ENVELOPE",
        ] {
            assert!(help.contains(form), "{form}");
        }
        assert!(CommandInterpreter::help_text().contains("HELP VOLUME"));
    }
}
