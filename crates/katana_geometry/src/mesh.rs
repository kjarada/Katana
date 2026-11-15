use crate::polygon::convex_hull;
use crate::{Point2, Point3, Triangle3};
use katana_core::error::{Error, ErrorCode, Status};
use katana_math::primitives::AABB as Aabb;
use katana_math::summation::CompensatedSum;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriangleMesh {
    pub vertices: Vec<Point3>,
    pub faces: Vec<[u32; 3]>,
}
impl TriangleMesh {
    pub fn triangle_count(&self) -> usize {
        self.faces.len()
    }
    pub fn empty(&self) -> bool {
        self.faces.is_empty()
    }
    pub fn triangle(&self, index: usize) -> Option<Triangle3> {
        let face = self.faces.get(index)?;
        Some(Triangle3::new(
            *self.vertices.get(face[0] as usize)?,
            *self.vertices.get(face[1] as usize)?,
            *self.vertices.get(face[2] as usize)?,
        ))
    }
    pub fn bounds(&self) -> Aabb {
        crate::point_batch::bounds_of(&self.vertices)
    }
    pub fn area(&self) -> f64 {
        let mut total = 0.0;
        for index in 0..self.faces.len() {
            if let Some(face) = self.triangle(index) {
                total += face.area();
            }
        }
        total
    }
    pub fn plan_hull(&self) -> Vec<Point2> {
        convex_hull(
            self.vertices
                .iter()
                .map(|vertex| Point2::new(vertex.x, vertex.y))
                .collect(),
        )
    }
}
pub fn validate(mesh: &TriangleMesh) -> Status {
    for (index, vertex) in mesh.vertices.iter().enumerate() {
        if !vertex.x.is_finite() || !vertex.y.is_finite() || !vertex.z.is_finite() {
            return Err(
                Error::new(ErrorCode::InvalidArgument, "a mesh vertex is not finite")
                    .with_context(format!("vertex={index}")),
            );
        }
    }
    for (face, indices) in mesh.faces.iter().enumerate() {
        for &index in indices {
            if index as usize >= mesh.vertices.len() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a mesh face names a vertex that does not exist",
                )
                .with_context(format!("face={face} vertex={index}")));
            }
        }
    }
    Ok(())
}
pub fn mesh_volume(mesh: &TriangleMesh) -> f64 {
    let Some(&origin) = mesh.vertices.first() else {
        return 0.0;
    };
    let mut volume = CompensatedSum::default();
    for face in &mesh.faces {
        if face
            .iter()
            .any(|&index| index as usize >= mesh.vertices.len())
        {
            continue;
        }
        let first = mesh.vertices[face[0] as usize] - origin;
        let second = mesh.vertices[face[1] as usize] - origin;
        let third = mesh.vertices[face[2] as usize] - origin;
        volume.add(first.dot(second.cross(third)) / 6.0);
    }
    volume.value()
}
