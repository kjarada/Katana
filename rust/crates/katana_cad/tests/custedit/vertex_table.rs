// The Vertices panel's table, headless (drawing/vertex_table.rs;
// tests/cad/drawing/test_vertex_table.cpp): the rows a polyline gives and what
// typing into each column does.

use katana_cad::AngleConvention;
use katana_cad::drawing::vertex_table::{
    VertexColumn, cell_text, edit_vertex_cell, vertex_column_from_string, vertex_rows,
};
use katana_geometry::{CurvePolyline2, Point2, PolylineResult};

use crate::near;

fn traverse() -> CurvePolyline2 {
    // East 10, then north 10, heights 100 and 101 and none.
    let mut p = CurvePolyline2::from_points(
        &[
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 10.0),
        ],
        false,
    );
    p.vertices[0].height = Some(100.0);
    p.vertices[1].height = Some(101.0);
    p
}

/// edit_vertex_cell as the C++ calls it, with its default angle convention
/// (AngleConvention::Bearing).
fn edit(
    polyline: &CurvePolyline2,
    index: usize,
    column: VertexColumn,
    text: &str,
) -> PolylineResult {
    edit_vertex_cell(polyline, index, column, text, AngleConvention::Bearing)
}

#[track_caller]
fn edited(result: PolylineResult) -> CurvePolyline2 {
    match result {
        Ok(polyline) => polyline,
        Err(error) => panic!("{}", error.describe()),
    }
}

mod vertex_table {
    use super::*;

    #[test]
    fn rows_give_coordinates_heights_and_each_segments_bearing_and_distance() {
        let rows = vertex_rows(&traverse());
        assert_eq!(rows.len(), 3);
        assert_eq!(cell_text(&rows[0], VertexColumn::Easting), "0.000");
        assert_eq!(cell_text(&rows[0], VertexColumn::Height), "100.000");
        assert_eq!(
            cell_text(&rows[2], VertexColumn::Height),
            "",
            "no height is not zero"
        );
        // East is a bearing of 90, north of 0.
        assert_eq!(
            cell_text(&rows[0], VertexColumn::Bearing),
            "90\u{B0}00'00\""
        );
        assert_eq!(cell_text(&rows[1], VertexColumn::Bearing), "0\u{B0}00'00\"");
        assert_eq!(cell_text(&rows[0], VertexColumn::Distance), "10.000");
        assert_eq!(
            cell_text(&rows[2], VertexColumn::Bearing),
            "",
            "the last vertex starts no segment"
        );
        assert_eq!(vertex_column_from_string("Z"), Some(VertexColumn::Height));
        assert_eq!(
            vertex_column_from_string("brg"),
            Some(VertexColumn::Bearing)
        );
        assert!(vertex_column_from_string("colour").is_none());
    }

    #[test]
    fn typing_into_a_cell_edits_the_vertex() {
        let p = traverse();
        assert_eq!(
            edited(edit(&p, 1, VertexColumn::Easting, "12.5")).vertices[1].position,
            Point2::new(12.5, 0.0)
        );
        assert_eq!(
            edited(edit(&p, 2, VertexColumn::Height, "99.25")).vertices[2].height,
            Some(99.25)
        );
        assert!(
            edited(edit(&p, 0, VertexColumn::Height, "")).vertices[0]
                .height
                .is_none()
        );
        assert_eq!(
            edited(edit(&p, 0, VertexColumn::Bulge, "0.5")).vertices[0].bulge,
            0.5
        );
        assert!(edit(&p, 0, VertexColumn::Index, "3").is_err());
        assert!(edit(&p, 0, VertexColumn::Easting, "east").is_err());
    }

    #[test]
    fn a_bearing_or_a_distance_moves_the_next_vertex() {
        let p = traverse();
        // The first segment turned to a bearing of 45 degrees, its 10 m kept.
        let turned = edited(edit(&p, 0, VertexColumn::Bearing, "45d00'00\""));
        near(turned.vertices[1].position.x, 10.0 * 0.5_f64.sqrt(), 1e-9);
        near(turned.vertices[1].position.y, 10.0 * 0.5_f64.sqrt(), 1e-9);
        // A quadrant bearing reads the same way.
        let quadrant = edited(edit(&p, 0, VertexColumn::Bearing, "N45E"));
        near(
            quadrant.vertices[1].position.x,
            turned.vertices[1].position.x,
            1e-9,
        );
        // The second segment stretched to 25 m, north still.
        let longer = edited(edit(&p, 1, VertexColumn::Distance, "25"));
        near(longer.vertices[2].position.y, 25.0, 1e-9);
        assert!(edit(&p, 2, VertexColumn::Distance, "5").is_err());
    }
}
