// One answer to what a linetype or symbol NAME draws
// (tests/cad/customisation/test_style_resolver.cpp): the viewport, the plot
// and every preview ask here, so these are the rules all of them draw by.
//
// Every definition is built inline; nothing depends on the gitignored
// reference files. Expected values are worked by hand in the comments.

use std::sync::Arc;

use katana_cad::customisation_record::source_name_hash;
use katana_cad::style_resolver::{
    below_symbol_detail, drawn_extent, find_definition_in, point_symbol_drawing,
    point_symbol_drawing_cached, resolve_line_pattern, resolve_linetype, resolve_symbol,
    style_sample_drawing, style_sample_path, symbol_vertices,
};
use katana_cad::{
    DashOptions, DefinitionCache, Document, LinetypeKind, RenamedDefinition, StyleDrawing,
    StyleSamplePath, StyleStroke, StyleTextMark, SymbolKind,
};
use katana_entity::{
    Geometry, LineStyle, Linetype, LinetypeElement, Model, Stroke, StrokeOp, Style, StyleLibrary,
};
use katana_geometry::{Circle2, Point2, Polyline2, Segment2};

use crate::{library_of, near};

fn move_to(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Move,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn draw(x: f64, y: f64) -> Stroke {
    Stroke {
        op: StrokeOp::Draw,
        point: Point2::new(x, y),
        ..Stroke::default()
    }
}

fn definition(name: &str, strokes: Vec<Stroke>, at_vertices: bool) -> LineStyle {
    LineStyle {
        name: String::from(name),
        strokes,
        at_vertices,
        ..LineStyle::default()
    }
}

fn dashed(name: &str, lengths: &[f64]) -> Linetype {
    Linetype {
        name: String::from(name),
        pattern: lengths
            .iter()
            .map(|&length| LinetypeElement { length })
            .collect(),
        ..Linetype::default()
    }
}

fn stroke_length(stroke: &StyleStroke) -> f64 {
    let mut total = 0.0;
    for pair in stroke.path.vertices.windows(2) {
        total += pair[0].distance_to(pair[1]);
    }
    total
}

mod style_resolver {
    use super::*;

    #[test]
    fn a_library_definition_beats_the_built_in_shape_of_the_same_name() {
        // A library that defines "circle" as a single dash: the library is what
        // the drawing was customised with, so it answers first.
        let library = library_of(vec![definition(
            "circle",
            vec![move_to(0.0, 0.0), draw(2.0, 0.0)],
            false,
        )]);
        let resolved = resolve_symbol(&library, "circle");
        assert_eq!(resolved.kind, SymbolKind::LibraryDefinition);
        let found = resolved.definition.expect("a definition");
        assert_eq!(found.name, "circle");

        // Drawn at (10, 20) at its own scale: the dash from (10, 20) to (12, 20),
        // not the built-in's 24-chord ring.
        let drawing = point_symbol_drawing(
            &library,
            "circle",
            Point2::new(10.0, 20.0),
            0.0,
            0.0,
            1.0,
            5.0,
        );
        assert_eq!(drawing.strokes.len(), 1);
        assert_eq!(
            drawing.strokes[0].path.vertices,
            vec![Point2::new(10.0, 20.0), Point2::new(12.0, 20.0)]
        );
    }

    #[test]
    fn an_unknown_symbol_falls_back_to_a_built_in_shape_and_says_so() {
        let empty = StyleLibrary::default();
        // "SEWR Manhole Cover" is a real library name no library here defines;
        // its words suggest the manhole, and the caller is told it is a guess.
        let guessed = resolve_symbol(&empty, "SEWR Manhole Cover");
        assert_eq!(guessed.kind, SymbolKind::BuiltInFallback);
        assert_eq!(guessed.built_in, "manhole");
        assert!(guessed.definition.is_none());
        // A built-in name is itself, and not a guess.
        let exact = resolve_symbol(&empty, "cross");
        assert_eq!(exact.kind, SymbolKind::BuiltIn);
        assert_eq!(exact.built_in, "cross");
        // No name is no symbol: the plain point mark.
        assert_eq!(resolve_symbol(&empty, "").kind, SymbolKind::None);
        assert!(
            point_symbol_drawing(&empty, "", Point2::new(0.0, 0.0), 0.0, 0.0, 1.0, 1.0).empty()
        );
    }

    #[test]
    fn a_built_in_shape_with_no_size_takes_the_callers_fallback_half_width() {
        let empty = StyleLibrary::default();
        // A cross is the square's two diagonals. With size 0 the half-width is
        // the fallback, 3: about (10, 20) the first diagonal runs (7, 17) to
        // (13, 23).
        let fallback =
            point_symbol_drawing(&empty, "cross", Point2::new(10.0, 20.0), 0.0, 0.0, 1.0, 3.0);
        assert_eq!(fallback.strokes.len(), 2);
        let first = &fallback.strokes[0].path.vertices;
        assert_eq!(first[0], Point2::new(7.0, 17.0));
        assert_eq!(first[first.len() - 1], Point2::new(13.0, 23.0));
        assert_eq!(
            fallback.strokes[0].pen, "",
            "a built-in shape is in the entity's own pen"
        );
        // A size is a WIDTH: 4 wide is 2 either side, (8, 18) to (12, 22).
        let sized =
            point_symbol_drawing(&empty, "cross", Point2::new(10.0, 20.0), 4.0, 0.0, 1.0, 3.0);
        let first = &sized.strokes[0].path.vertices;
        assert_eq!(first[0], Point2::new(8.0, 18.0));
        assert_eq!(first[first.len() - 1], Point2::new(12.0, 22.0));
    }

    #[test]
    fn a_library_linestyle_wins_over_a_model_linetype_of_its_name_and_the_collision_is_reported() {
        let mut model = Model::default();
        assert!(
            model
                .linetypes
                .add(dashed("WATR Main", &[1.0, -0.5]))
                .is_ok()
        );
        let library = library_of(vec![definition(
            "WATR Main",
            vec![move_to(0.0, 0.0), draw(3.0, 0.0), move_to(5.0, 0.0)],
            false,
        )]);

        let both = resolve_linetype(&model, &library, "WATR Main");
        assert_eq!(both.kind, LinetypeKind::LibraryDefinition);
        assert!(both.definition.is_some());
        assert!(
            both.linetype.is_none(),
            "the library's strokes, with no dash pattern on them"
        );
        assert!(both.collision);

        let none = StyleLibrary::default();
        let model_only = resolve_linetype(&model, &none, "WATR Main");
        assert_eq!(model_only.kind, LinetypeKind::ModelLinetype);
        assert!(model_only.linetype.is_some());
        assert!(!model_only.collision);

        let nobody = resolve_linetype(&model, &library, "Nothing Holds This");
        assert_eq!(nobody.kind, LinetypeKind::Solid);
        assert!(!nobody.collision);
    }

    #[test]
    fn a_vertex_symbol_is_never_a_line_pattern() {
        // D8: a `mode vertex` definition named as a linetype does not become a
        // pattern. The line is the model linetype of that name if there is one -
        // a collision worth reporting - and solid otherwise.
        let library = library_of(vec![definition(
            "Post",
            vec![move_to(-1.0, 0.0), draw(1.0, 0.0)],
            true,
        )]);
        let mut model = Model::default();
        assert_eq!(
            resolve_linetype(&model, &library, "Post").kind,
            LinetypeKind::Solid
        );
        assert!(model.linetypes.add(dashed("Post", &[1.0, -1.0])).is_ok());
        let resolved = resolve_linetype(&model, &library, "Post");
        assert_eq!(resolved.kind, LinetypeKind::ModelLinetype);
        assert!(resolved.collision);
    }

    #[test]
    fn a_line_whose_linetype_names_its_own_symbol_is_a_plain_line() {
        // What an archive import writes for a symbol string: name, linetype and
        // symbol all the symbol's name. The definition is NOT `mode vertex` - as
        // 147 of the symbols the reference survey code files use are not - and
        // it used to be laid along the line as a pattern.
        let library = library_of(vec![definition(
            "CULT Fence Post",
            vec![move_to(0.0, 0.0), draw(0.0, 1.0)],
            false,
        )]);
        let model = Model::default();
        assert_eq!(
            resolve_line_pattern(&model, &library, "CULT Fence Post", "CULT Fence Post").kind,
            LinetypeKind::Solid
        );
        // The same name as a linetype with no symbol is a linestyle, as D2 says.
        assert_eq!(
            resolve_line_pattern(&model, &library, "CULT Fence Post", "").kind,
            LinetypeKind::LibraryDefinition
        );
    }

    #[test]
    fn the_sample_path_is_a_run_a_corner_and_a_half_circle_inside_its_box() {
        // Width 8: the run ends at 0.45 * 8 = 3.6, the corner is (0.6 * 8,
        // 0.25 * 8) = (4.8, 2), and the arc of radius 0.2 * 8 = 1.6 about
        // (6.4, 2) dips to (6.4, 0.4) and ends at (8, 2).
        let sample = style_sample_path(8.0);
        assert_eq!(sample.vertices.len(), 4);
        let expected = [
            Point2::new(0.0, 0.0),
            Point2::new(3.6, 0.0),
            Point2::new(4.8, 2.0),
            Point2::new(8.0, 2.0),
        ];
        for (vertex, expected) in sample.vertices.iter().zip(expected) {
            near(vertex.x, expected.x, 1e-12);
            near(vertex.y, expected.y, 1e-12);
        }
        assert!(sample.path.vertices.len() > 4, "the arc is chorded");
        let mut lowest: f64 = 10.0;
        for p in &sample.path.vertices[3..] {
            near(p.distance_to(Point2::new(6.4, 2.0)), 1.6, 1e-12);
            lowest = lowest.min(p.y);
        }
        near(lowest, 0.4, 1e-12);
        for p in &sample.path.vertices {
            assert!(p.x >= -1e-12);
            assert!(p.x <= 8.0 + 1e-12);
            assert!(p.y >= -1e-12);
            assert!(p.y <= 2.0 + 1e-12);
        }
    }

    #[test]
    fn a_model_linetype_sample_is_its_dash_spans_counted_by_hand() {
        // Dash 2, gap 1 along (0,0) -> (4,0) -> (4,3), seven long. The pattern
        // runs on across the corner: dash 0..2; gap 2..3; dash 3..5, which the
        // corner at 4 splits into (3,0)-(4,0) and (4,0)-(4,1); gap 5..6; dash
        // 6..7, cut by the end at (4,3). Four spans.
        let mut model = Model::default();
        assert!(model.linetypes.add(dashed("DASH", &[2.0, -1.0])).is_ok());
        let style = Style {
            name: String::from("S"),
            linetype: String::from("DASH"),
            ..Style::default()
        };
        let corner = vec![
            Point2::new(0.0, 0.0),
            Point2::new(4.0, 0.0),
            Point2::new(4.0, 3.0),
        ];
        let sample = StyleSamplePath {
            path: Polyline2::new(corner.clone(), false),
            vertices: corner,
        };
        let options = DashOptions {
            view_scale: 1.0, // the shortest element, 1, is one pixel: resolvable
            ..DashOptions::default()
        };
        let drawing = style_sample_drawing(
            &model,
            &StyleLibrary::default(),
            &style,
            &sample,
            1.0,
            &options,
            0.0,
        );
        let spans = [
            vec![Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)],
            vec![Point2::new(3.0, 0.0), Point2::new(4.0, 0.0)],
            vec![Point2::new(4.0, 0.0), Point2::new(4.0, 1.0)],
            vec![Point2::new(4.0, 2.0), Point2::new(4.0, 3.0)],
        ];
        assert_eq!(drawing.strokes.len(), spans.len());
        for (i, (stroke, span)) in drawing.strokes.iter().zip(&spans).enumerate() {
            assert_eq!(stroke.path.vertices, *span, "{i}");
        }
    }

    #[test]
    fn a_library_linestyle_sample_is_its_strokes_with_no_solid_line_and_no_dashes_leaked() {
        // "L" is a 1-long dash and a 1-long gap (period 2). The model holds a
        // quarter-unit dash of the SAME name; D2 says the library wins and none
        // of that pattern reaches its strokes. Along a straight 9, the repeats
        // at 0, 2, 4, 6 and 8 give five 1-long dashes - no 9-long solid line,
        // and no quarter-unit pieces.
        let mut model = Model::default();
        assert!(model.linetypes.add(dashed("L", &[0.25, -0.25])).is_ok());
        let library = library_of(vec![definition(
            "L",
            vec![move_to(0.0, 0.0), draw(1.0, 0.0), move_to(2.0, 0.0)],
            false,
        )]);
        let style = Style {
            name: String::from("S"),
            linetype: String::from("L"),
            ..Style::default()
        };
        let straight = vec![Point2::new(0.0, 0.0), Point2::new(9.0, 0.0)];
        let sample = StyleSamplePath {
            path: Polyline2::new(straight.clone(), false),
            vertices: straight,
        };
        let options = DashOptions {
            view_scale: 1.0,
            ..DashOptions::default()
        };
        let drawing = style_sample_drawing(&model, &library, &style, &sample, 1.0, &options, 0.0);
        assert_eq!(drawing.strokes.len(), 5);
        for stroke in &drawing.strokes {
            near(stroke_length(stroke), 1.0, 1e-12);
        }
        assert_eq!(drawing.strokes[0].path.vertices[0], Point2::new(0.0, 0.0));
        let last = &drawing.strokes[drawing.strokes.len() - 1].path.vertices;
        assert_eq!(last[last.len() - 1], Point2::new(9.0, 0.0));
    }

    #[test]
    fn a_sample_carries_its_styles_symbol_at_every_vertex() {
        // D8. A continuous line with a cross 2 wide (1 either side) at each of
        // the sample's four vertices: the line, then two diagonals per vertex -
        // nine strokes, the first diagonal of each running from (v - 1) to
        // (v + 1).
        let style = Style {
            name: String::from("Fence"),
            symbol: String::from("cross"),
            symbol_size: 2.0,
            ..Style::default()
        };
        let sample = style_sample_path(8.0);
        let drawing = style_sample_drawing(
            &Model::default(),
            &StyleLibrary::default(),
            &style,
            &sample,
            1.0,
            &DashOptions::default(),
            0.0,
        );
        assert_eq!(drawing.strokes.len(), 1 + 4 * 2);
        assert_eq!(
            drawing.strokes[0].path.vertices, sample.path.vertices,
            "the line itself"
        );
        for (v, at) in sample.vertices.iter().enumerate() {
            let diagonal = &drawing.strokes[1 + 2 * v].path.vertices;
            near(diagonal[0].x, at.x - 1.0, 1e-12);
            near(diagonal[0].y, at.y - 1.0, 1e-12);
            near(diagonal[diagonal.len() - 1].x, at.x + 1.0, 1e-12);
            near(diagonal[diagonal.len() - 1].y, at.y + 1.0, 1e-12);
        }
    }

    #[test]
    fn a_cache_returns_fresh_geometry_once_the_library_generation_moves() {
        // Through a real Document, whose set_style_library bumps the generation.
        let mut document = Document::default();
        document.set_style_library(library_of(vec![definition(
            "S",
            vec![move_to(0.0, 0.0), draw(1.0, 0.0)],
            false,
        )]));
        let mut cache = DefinitionCache::default();
        let before = cache
            .find(document.style_library(), document.library_generation(), "S")
            .expect("S is defined");
        assert_eq!(
            before.runs[0].points[before.runs[0].points.len() - 1],
            Point2::new(1.0, 0.0)
        );
        let again = cache
            .find(document.style_library(), document.library_generation(), "S")
            .expect("S is defined");
        assert!(
            Arc::ptr_eq(&again, &before),
            "the same generation is answered from the cache"
        );

        // The library is replaced: "S" now runs to (5, 0).
        document.set_style_library(library_of(vec![definition(
            "S",
            vec![move_to(0.0, 0.0), draw(5.0, 0.0)],
            false,
        )]));
        let after = cache
            .find(document.style_library(), document.library_generation(), "S")
            .expect("S is defined");
        assert_eq!(
            after.runs[0].points[after.runs[0].points.len() - 1],
            Point2::new(5.0, 0.0)
        );
        assert_eq!(cache.generation(), Some(document.library_generation()));
        // What was handed out before is owned, not a view into the replaced
        // library, so it still reads what it read.
        assert_eq!(
            before.runs[0].points[before.runs[0].points.len() - 1],
            Point2::new(1.0, 0.0)
        );

        // A name the library lacks is remembered as absent, and the cached and
        // uncached drawings agree.
        assert!(
            cache
                .find(
                    document.style_library(),
                    document.library_generation(),
                    "none"
                )
                .is_none()
        );
        let generation = document.library_generation();
        let cached = point_symbol_drawing_cached(
            &mut cache,
            document.style_library(),
            generation,
            "S",
            Point2::new(1.0, 1.0),
            0.0,
            0.0,
            1.0,
            1.0,
        );
        let uncached = point_symbol_drawing(
            document.style_library(),
            "S",
            Point2::new(1.0, 1.0),
            0.0,
            0.0,
            1.0,
            1.0,
        );
        assert_eq!(
            cached.strokes[0].path.vertices,
            uncached.strokes[0].path.vertices
        );
    }

    #[test]
    fn a_symbol_under_three_pixels_is_drawn_as_a_dot() {
        // A symbol 2 units across: 2 px at 1 px a unit (a dot), 4 px at 2.
        let library = library_of(vec![definition(
            "Box",
            vec![move_to(-1.0, -1.0), draw(1.0, -1.0), draw(1.0, 1.0)],
            true,
        )]);
        let drawing =
            point_symbol_drawing(&library, "Box", Point2::new(0.0, 0.0), 0.0, 0.0, 1.0, 1.0);
        let minimum = katana_cad::style_resolver::MINIMUM_SYMBOL_PIXELS;
        assert!(below_symbol_detail(&drawing, 1.0, minimum));
        assert!(!below_symbol_detail(&drawing, 2.0, minimum));
        assert!(
            !below_symbol_detail(&StyleDrawing::default(), 1.0, minimum),
            "nothing to replace"
        );
    }

    #[test]
    fn the_drawn_extent_counts_a_texts_height_times_its_length() {
        // "AB", 1 high, anchored at (10, 0): 1 x 2 either side of the anchor,
        // x 8..12 and y -2..2 - more than any font draws, which is the safe side
        // for culling a label that reaches into the view.
        let mut drawing = StyleDrawing::default();
        drawing.texts.push(StyleTextMark {
            at: Point2::new(10.0, 0.0),
            text: String::from("AB"),
            height: 1.0,
            ..StyleTextMark::default()
        });
        let bounds = drawn_extent(&drawing);
        assert_eq!(bounds.min, Point2::new(8.0, -2.0));
        assert_eq!(bounds.max, Point2::new(12.0, 2.0));
    }

    #[test]
    fn a_vertex_symbol_goes_on_every_vertex_of_a_line_and_nowhere_on_a_circle() {
        assert_eq!(
            symbol_vertices(&Geometry::Line(Segment2::new(
                Point2::new(0.0, 0.0),
                Point2::new(3.0, 4.0)
            ))),
            vec![Point2::new(0.0, 0.0), Point2::new(3.0, 4.0)]
        );
        let fence = Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
            ],
            false,
        );
        assert_eq!(
            symbol_vertices(&Geometry::Polyline(fence.clone())),
            fence.vertices
        );
        assert!(
            symbol_vertices(&Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), 1.0))).is_empty()
        );
    }

    // A drawing saved before the built-in customisation was given general names
    // still names the definitions that were renamed. The lookup every drawing
    // path shares tries the exact name first and asks the rename table only
    // when that misses: "Brand X" (invented; the table holds its hash) finds
    // "X", while a library that itself holds "Brand X" - a person's own
    // definition - keeps it.
    #[test]
    fn a_definition_named_as_it_was_before_the_rename_is_found_under_its_name_now() {
        let renamed = [RenamedDefinition {
            former_name: source_name_hash("Brand X"),
            now: "X",
        }];

        let mut renamed_only = StyleLibrary::default();
        assert!(
            renamed_only
                .add(definition(
                    "X",
                    vec![move_to(0.0, 0.0), draw(1.0, 0.0)],
                    false
                ))
                .is_ok()
        );
        let found = find_definition_in(&renamed_only, "Brand X", &renamed).expect("found as X");
        assert_eq!(found.name, "X");
        let exact = find_definition_in(&renamed_only, "X", &renamed).expect("X itself");
        assert!(std::ptr::eq(exact, found));

        let mut both = renamed_only.clone();
        assert!(
            both.add(definition(
                "Brand X",
                vec![move_to(0.0, 0.0), draw(0.0, 1.0)],
                false
            ))
            .is_ok()
        );
        let own = find_definition_in(&both, "Brand X", &renamed).expect("its own");
        assert_eq!(own.name, "Brand X");

        assert!(find_definition_in(&renamed_only, "Other", &renamed).is_none());
        assert!(find_definition_in(&renamed_only, "", &renamed).is_none());
    }
}
