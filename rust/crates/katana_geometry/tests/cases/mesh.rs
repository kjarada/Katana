use crate::support::double_eq;
use katana_core::ErrorCode;
use katana_geometry::mesh::validate;
use katana_geometry::{Point2, Point3, TriangleMesh};
fn unit_cube() -> TriangleMesh {
    TriangleMesh {
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(1.0, 0.0, 1.0),
            Point3::new(1.0, 1.0, 1.0),
            Point3::new(0.0, 1.0, 1.0),
        ],
        faces: vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
    }
}
mod triangle_mesh {
    use super::*;
    #[test]
    fn a_cube_knows_its_triangles_its_area_and_its_box() {
        let cube = unit_cube();
        assert_eq!(cube.triangle_count(), 12);
        assert!(!cube.empty());
        double_eq(cube.area(), 6.0);
        let bounds = cube.bounds();
        assert_eq!(bounds.min, Point3::new(0.0, 0.0, 0.0));
        assert_eq!(bounds.max, Point3::new(1.0, 1.0, 1.0));
        let first = cube.triangle(0).unwrap();
        assert_eq!(first.a, Point3::new(0.0, 0.0, 0.0));
        double_eq(first.area(), 0.5);
    }
    #[test]
    fn the_plan_footprint_is_the_hull_of_the_vertices_and_says_so() {
        let hull = unit_cube().plan_hull();
        assert_eq!(hull.len(), 4);
        for corner in hull {
            assert!(
                [
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(1.0, 1.0),
                    Point2::new(0.0, 1.0)
                ]
                .contains(&corner)
            );
        }
        let wall = TriangleMesh {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 3.0),
                Point3::new(0.0, 0.0, 3.0),
            ],
            faces: vec![[0, 1, 2], [0, 2, 3]],
        };
        let stands_on = wall.plan_hull();
        assert_eq!(stands_on.len(), 2);
        assert_eq!(stands_on[0], Point2::new(0.0, 0.0));
        assert_eq!(stands_on[1], Point2::new(10.0, 0.0));
        double_eq(wall.area(), 30.0);
    }
    #[test]
    fn a_face_naming_a_vertex_that_does_not_exist_is_refused_and_not_read() {
        let mut mesh = unit_cube();
        mesh.faces.push([0, 1, 99]);
        let error = validate(&mesh).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.describe().contains("99"));
        assert!(mesh.triangle(12).is_none());
        assert!(mesh.triangle(13).is_none());
        double_eq(mesh.area(), 6.0);
    }
    #[test]
    fn a_non_finite_vertex_is_refused_naming_it() {
        let mut mesh = unit_cube();
        mesh.vertices[3].z = f64::NAN;
        let error = validate(&mesh).unwrap_err();
        assert!(error.describe().contains("vertex=3"));
    }
    #[test]
    fn an_empty_mesh_is_valid_and_answers_everything_with_nothing() {
        let mesh = TriangleMesh::default();
        assert!(validate(&mesh).is_ok());
        assert!(mesh.empty());
        assert_eq!(mesh.triangle_count(), 0);
        double_eq(mesh.area(), 0.0);
        assert!(mesh.plan_hull().is_empty());
        assert!(mesh.triangle(0).is_none());
        assert!(mesh.bounds().empty());
    }
}
