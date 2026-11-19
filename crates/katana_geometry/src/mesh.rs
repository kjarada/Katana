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
    /// The edges exactly one face uses: the rim of an open mesh, and nothing of a closed one.
    /// Edges are matched by their two vertex indices whichever way round the faces wind them, so
    /// a mesh whose faces disagree on their winding still finds its rim. Each comes once, lower
    /// index first, with the face that uses it, in ascending order of (lower, higher). A face
    /// naming a vertex that does not exist makes no edge, and neither does the side of a face
    /// that repeats a vertex. Topology only: two faces that meet at one POSITION through
    /// different vertex indices are not joined, as a file that repeats its vertices per face
    /// does not join them.
    pub fn boundary_edges(&self) -> Vec<MeshEdge> {
        let count = self.vertices.len();
        let mut edges: Vec<MeshEdge> = Vec::with_capacity(self.faces.len() * 3);
        for (face, corners) in self.faces.iter().enumerate() {
            if corners.iter().any(|&corner| corner as usize >= count) {
                continue;
            }
            let [p, q, r] = *corners;
            for (a, b) in [(p, q), (q, r), (r, p)] {
                if a != b {
                    edges.push(MeshEdge {
                        a: a.min(b),
                        b: a.max(b),
                        face: face as u32,
                    });
                }
            }
        }
        edges.sort_unstable_by_key(|edge| (edge.a, edge.b, edge.face));
        let mut rim = Vec::new();
        let mut at = 0;
        while at < edges.len() {
            let mut end = at + 1;
            while end < edges.len() && edges[end].a == edges[at].a && edges[end].b == edges[at].b {
                end += 1;
            }
            if end - at == 1 {
                rim.push(edges[at]);
            }
            at = end;
        }
        rim
    }
}
/// An edge of a mesh: its two vertex indices, lower first, and a face that uses it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MeshEdge {
    pub a: u32,
    pub b: u32,
    pub face: u32,
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
