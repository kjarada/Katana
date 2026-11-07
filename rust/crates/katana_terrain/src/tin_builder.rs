//! TIN construction:
//!
//!   Survey Points -> Breaklines -> Boundaries -> Triangulation -> TIN -> Surface
//!
//! build_tin() computes a 2D constrained Delaunay triangulation of the plan
//! positions with exact predicates (cdt_backend, isolated inside this crate) and
//! returns a TinSurface made of Katana types only. See docs/terrain.md for the
//! algorithms and numerical assumptions.
//!
//! Input rules
//!   * Points and breakline vertices carry elevations. Breakline segments become
//!     constrained edges: the surface interpolates linearly along them.
//!   * Plan positions closer than tolerance::GEOMETRIC are one vertex. What
//!     happens to their elevations is decided by DuplicatePointPolicy; every
//!     merge is counted in the report.
//!   * A survey point lying exactly on a breakline segment splits the segment and
//!     keeps its own elevation.
//!   * Breaklines that cross in plan view create a vertex at the crossing. Its
//!     elevation is interpolated along each breakline; what happens when the
//!     breaklines disagree is decided by CrossingBreaklinePolicy.
//!   * The boundary is a hard clip: its edges are constrained edges and only
//!     triangles inside it are kept. Holes remove the triangles inside them.
//!     Boundary and hole vertices are plan positions. One that coincides with a
//!     survey point or breakline vertex is that vertex; any other takes the
//!     elevation of the unclipped surface at its position, and must therefore lie
//!     on it (inside the convex hull of the data).
//!   * Rings must be simple polygons (a ring that touches or crosses itself is
//!     rejected); different rings may overlap or cross each other.
//!
//! Output is canonical and deterministic: vertices in order of first appearance
//! (points, breakline vertices, boundary vertices, hole vertices, then crossing
//! vertices sorted by x, y), every triangle rotated to start at its smallest
//! vertex index, triangles sorted lexicographically. Identical input gives
//! bitwise-identical output.

use katana_core::text::format_general;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::polygon::{Orientation, orientation};
use katana_geometry::{Containment, Polyline2, Segment2, Triangle2};
use katana_math::numerics::{lerp, tolerance};

use crate::cdt_backend::{self, CdtConstraint, CdtInput, CdtOutput, NO_INDEX};
use crate::point_merge::PointMerger;
use crate::tin_surface::{NO_TRIANGLE, NO_VERTEX, Point2, Point3, TinSurface, std_max, std_min};

/// A 3D polyline whose segments the triangulation must honour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakline {
    pub vertices: Vec<Point3>,
    /// adds the segment from the last vertex back to the first
    pub closed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TinInput {
    pub points: Vec<Point3>,
    pub breaklines: Vec<Breakline>,
    /// Outer boundary; no vertices = none (the convex hull bounds the surface).
    /// Always treated as a closed ring, either orientation.
    pub boundary: Polyline2,
    /// Voids. Always treated as closed rings, either orientation.
    pub holes: Vec<Polyline2>,
}

/// Elevations of vertices that share a plan position (within GEOMETRIC).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DuplicatePointPolicy {
    /// Merge when the elevations agree within `elevation_tolerance` (the first is
    /// kept); fail with InvalidGeometry otherwise. A TIN cannot represent two
    /// elevations at one position, so this never alters data silently.
    #[default]
    ErrorOnConflict,
    /// the first occurrence in input order wins
    KeepFirst,
    /// arithmetic mean of all occurrences
    Average,
}

/// Elevation of the vertex created where two breaklines cross in plan view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossingBreaklinePolicy {
    /// Use the mean when the breaklines agree within `elevation_tolerance`; fail
    /// with InvalidGeometry otherwise.
    #[default]
    ErrorOnConflict,
    /// always use the mean of the crossing breaklines
    Average,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TinBuildOptions {
    pub duplicate_points: DuplicatePointPolicy,
    pub crossing_breaklines: CrossingBreaklinePolicy,
    /// Metres. Elevations at one plan position that differ by more than this are
    /// in conflict. Default: survey coordinates closer than COORDINATE are the
    /// same ground mark.
    pub elevation_tolerance: f64,
}

impl Default for TinBuildOptions {
    fn default() -> Self {
        Self {
            duplicate_points: DuplicatePointPolicy::ErrorOnConflict,
            crossing_breaklines: CrossingBreaklinePolicy::ErrorOnConflict,
            elevation_tolerance: tolerance::COORDINATE,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TinBuildReport {
    /// Input points merged into an earlier vertex.
    pub duplicate_point_count: usize,
    /// Breakline vertices that coincide with an earlier vertex (normal when
    /// breaklines are drawn through survey points).
    pub shared_breakline_vertex_count: usize,
    /// Merges whose elevations differed by more than elevation_tolerance and were
    /// resolved by the policy (always 0 with ErrorOnConflict).
    pub elevation_conflict_count: usize,
    /// Vertices created where breaklines cross, and how many of those had
    /// breakline elevations differing by more than elevation_tolerance.
    pub breakline_crossing_count: usize,
    pub crossing_conflict_count: usize,
    /// Triangles removed by the boundary and the holes; vertices left without a
    /// triangle by that removal (they are not part of the surface).
    pub clipped_triangle_count: usize,
    pub dropped_vertex_count: usize,
    /// For every input point: its vertex in the surface, or NO_VERTEX when the
    /// point was clipped away.
    pub point_vertex: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TinBuildResult {
    pub surface: TinSurface,
    pub report: TinBuildReport,
}

// Constraint tags handed to the triangulation and reported back per edge.
const BREAKLINE_TAG: u8 = 1;
const BOUNDARY_TAG: u8 = 2;
const HOLE_TAG: u8 = 4;
const RING_TAGS: u8 = BOUNDARY_TAG | HOLE_TAG;

// The triangulation's adjacency array becomes the surface's without a pass.
const _: () = assert!(NO_INDEX == NO_TRIANGLE && NO_INDEX == NO_VERTEX);

/// A position as the C++ stream with precision 17 writes it.
fn describe_point(p: Point2) -> String {
    format!("({}, {})", format_general(p.x, 17), format_general(p.y, 17))
}

fn describe_value(value: f64) -> String {
    format_general(value, 17)
}

/// A boundary or hole after its vertices were merged into the vertex table.
struct Ring {
    /// "boundary" or "hole <i>", for messages
    name: String,
    tag: u8,
    /// vertex ids, no consecutive repeats
    ids: Vec<u32>,
    /// the positions of `ids`, closed
    polygon: Polyline2,
    /// its edges in the constraint list
    first_constraint: usize,
}

/// All vertices known before triangulation: positions merged within GEOMETRIC,
/// one elevation each (NaN while a ring vertex still waits for its elevation).
struct VertexTable {
    merger: PointMerger,
    options: TinBuildOptions,
    elevations: Vec<f64>,
    /// Average policy only
    sums: Vec<f64>,
    counts: Vec<u32>,
}

impl VertexTable {
    fn new(capacity: usize, options: TinBuildOptions) -> Self {
        Self {
            merger: PointMerger::new(capacity, tolerance::GEOMETRIC),
            options,
            elevations: Vec::with_capacity(capacity),
            sums: Vec::new(),
            counts: Vec::new(),
        }
    }

    /// Adds a survey point or breakline vertex. `what` names it for messages.
    fn add_elevated(
        &mut self,
        p: Point3,
        is_breakline_vertex: bool,
        what: &str,
        report: &mut TinBuildReport,
    ) -> Result<u32> {
        let entry = self.merger.add(Point2::new(p.x, p.y));
        if entry.inserted {
            self.elevations.push(p.z);
            if self.options.duplicate_points == DuplicatePointPolicy::Average {
                self.sums.push(p.z);
                self.counts.push(1);
            }
            return Ok(entry.index);
        }

        if is_breakline_vertex {
            report.shared_breakline_vertex_count += 1;
        } else {
            report.duplicate_point_count += 1;
        }
        let kept = self.elevations[entry.index as usize];
        if (p.z - kept).abs() > self.options.elevation_tolerance {
            if self.options.duplicate_points == DuplicatePointPolicy::ErrorOnConflict {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "two elevations at one plan position (DuplicatePointPolicy::ErrorOnConflict)",
                )
                .with_context(format!(
                    "{what} at {} has z={}, the earlier vertex has z={}",
                    describe_point(Point2::new(p.x, p.y)),
                    describe_value(p.z),
                    describe_value(kept)
                )));
            }
            report.elevation_conflict_count += 1;
        }
        if self.options.duplicate_points == DuplicatePointPolicy::Average {
            self.sums[entry.index as usize] += p.z;
            self.counts[entry.index as usize] += 1;
        }
        Ok(entry.index)
    }

    /// Adds a boundary/hole vertex, which has no elevation of its own.
    fn add_plan_only(&mut self, p: Point2) -> u32 {
        let entry = self.merger.add(p);
        if entry.inserted {
            self.elevations.push(f64::NAN);
        }
        entry.index
    }

    /// Call once after the last add_elevated(): settles the Average policy.
    fn finish_elevated(&mut self) {
        for i in 0..self.sums.len() {
            if self.counts[i] > 1 {
                self.elevations[i] = self.sums[i] / f64::from(self.counts[i]);
            }
        }
    }

    fn size(&self) -> usize {
        self.elevations.len()
    }
}

#[derive(Clone, Copy)]
struct Accumulator {
    sum: f64,
    lowest: f64,
    highest: f64,
    count: u32,
}

/// Elevations of the vertices the triangulation created on breaklines.
///
/// The vertices along each breakline segment are known from the triangulation. A
/// created vertex takes the elevation the breakline has at its position (linear
/// between the segment's end vertices); where several breaklines meet in it their
/// mean, subject to the crossing policy. Created vertices that lie on no breakline
/// (rings crossing each other) are returned in `unresolved`.
fn resolve_crossing_elevations(
    cdt: &CdtOutput,
    constraints: &[CdtConstraint],
    positions: &[Point2],
    options: &TinBuildOptions,
    elevations: &mut Vec<f64>,
    report: &mut TinBuildReport,
    unresolved: &mut Vec<u32>,
) -> Status {
    let input_count = positions.len();
    let created_count = cdt.generated_points.len();
    if created_count == 0 {
        return Ok(());
    }
    let mut created = vec![
        Accumulator {
            sum: 0.0,
            lowest: f64::INFINITY,
            highest: f64::NEG_INFINITY,
            count: 0,
        };
        created_count
    ];

    for (c, constraint) in constraints.iter().enumerate() {
        if constraint.tag != BREAKLINE_TAG {
            continue;
        }
        let begin = cdt.constraint_vertex_offsets[c] as usize;
        let end = cdt.constraint_vertex_offsets[c + 1] as usize;
        let segment = Segment2::new(
            positions[constraint.from as usize],
            positions[constraint.to as usize],
        );
        let z_from = elevations[constraint.from as usize];
        let z_to = elevations[constraint.to as usize];
        for &vertex in &cdt.constraint_vertices[begin..end] {
            if (vertex as usize) < input_count {
                continue; // a survey point on the breakline keeps its own elevation
            }
            let position = cdt.generated_points[vertex as usize - input_count];
            let z = lerp(z_from, z_to, segment.parameter_of(position));
            let accumulator = &mut created[vertex as usize - input_count];
            accumulator.sum += z;
            accumulator.lowest = std_min(accumulator.lowest, z);
            accumulator.highest = std_max(accumulator.highest, z);
            accumulator.count += 1;
        }
    }

    elevations.resize(input_count + created_count, f64::NAN);
    for (i, accumulator) in created.iter().enumerate() {
        if accumulator.count == 0 {
            unresolved.push((input_count + i) as u32);
            continue;
        }
        if accumulator.count > 1 {
            report.breakline_crossing_count += 1;
            if accumulator.highest - accumulator.lowest > options.elevation_tolerance {
                if options.crossing_breaklines == CrossingBreaklinePolicy::ErrorOnConflict {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "breaklines cross at different elevations \
                         (CrossingBreaklinePolicy::ErrorOnConflict)",
                    )
                    .with_context(format!(
                        "crossing at {}, elevations from {} to {}",
                        describe_point(cdt.generated_points[i]),
                        describe_value(accumulator.lowest),
                        describe_value(accumulator.highest)
                    )));
                }
                report.crossing_conflict_count += 1;
            }
        }
        elevations[input_count + i] = accumulator.sum / f64::from(accumulator.count);
    }
    Ok(())
}

fn make_vertices(positions: &[Point2], created: &[Point2], elevations: &[f64]) -> Vec<Point3> {
    let mut vertices = Vec::with_capacity(positions.len() + created.len());
    for (i, p) in positions.iter().enumerate() {
        vertices.push(Point3::new(p.x, p.y, elevations[i]));
    }
    for (i, p) in created.iter().enumerate() {
        vertices.push(Point3::new(p.x, p.y, elevations[positions.len() + i]));
    }
    vertices
}

fn constraint_masks(edge_tags: &[[u8; 3]]) -> Vec<u8> {
    edge_tags
        .iter()
        .map(|tags| {
            let mut mask = 0u8;
            for (k, &tag) in tags.iter().enumerate() {
                if tag != 0 {
                    mask |= 1 << k;
                }
            }
            mask
        })
        .collect()
}

/// A ring is simple exactly when the closed walk along its edges - including the
/// vertices that split them - visits no vertex twice: crossings, touching
/// vertices and overlapping edges all show up as a repeated vertex. Exact, and
/// linear in the size of the ring (geometry's is_simple is quadratic).
fn require_simple_ring(
    ring: &Ring,
    cdt: &CdtOutput,
    positions: &[Point2],
    visited: &mut [u8],
) -> Status {
    let mut walk: Vec<u32> = Vec::new();
    for edge in 0..ring.ids.len() {
        let c = ring.first_constraint + edge;
        let begin = cdt.constraint_vertex_offsets[c] as usize;
        let end = cdt.constraint_vertex_offsets[c + 1] as usize;
        // the last vertex opens the next edge
        let mut i = begin;
        while i + 1 < end {
            walk.push(cdt.constraint_vertices[i]);
            i += 1;
        }
    }
    let mut repeated: Option<u32> = None;
    for &vertex in &walk {
        if visited[vertex as usize] != 0 && repeated.is_none() {
            repeated = Some(vertex);
        }
        visited[vertex as usize] = 1;
    }
    for &vertex in &walk {
        visited[vertex as usize] = 0;
    }
    if let Some(repeated) = repeated {
        let where_ = if (repeated as usize) < positions.len() {
            positions[repeated as usize]
        } else {
            cdt.generated_points[repeated as usize - positions.len()]
        };
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            format!("{} touches or crosses itself", ring.name),
        )
        .with_context(format!("at {}", describe_point(where_))));
    }
    Ok(())
}

/// Marks the triangles to keep. Ring edges partition the triangulation into
/// regions; a region is inside or outside as a whole, so one point per region is
/// classified against the polygons: the centroid of the region's largest
/// triangle, which is as far from every ring edge as the region allows.
fn select_triangles(cdt: &CdtOutput, vertices: &[Point3], rings: &[Ring]) -> Vec<u8> {
    let triangle_count = cdt.triangles.len();
    let mut keep = vec![0u8; triangle_count];
    let mut assigned = vec![0u8; triangle_count];
    let mut region: Vec<u32> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();

    let plan_triangle = |t: u32| {
        let tri = cdt.triangles[t as usize];
        let corner = |k: usize| {
            let v = vertices[tri[k] as usize];
            Point2::new(v.x, v.y)
        };
        Triangle2::new(corner(0), corner(1), corner(2))
    };

    for seed in 0..triangle_count as u32 {
        if assigned[seed as usize] != 0 {
            continue;
        }
        region.clear();
        stack.clear();
        stack.push(seed);
        assigned[seed as usize] = 1;
        let mut largest = seed;
        let mut largest_area = -1.0;
        while let Some(t) = stack.pop() {
            region.push(t);
            let area = plan_triangle(t).signed_area();
            if area > largest_area || (area == largest_area && t < largest) {
                largest_area = area;
                largest = t;
            }
            for k in 0..3 {
                let next = cdt.neighbors[t as usize][k];
                if next != NO_INDEX
                    && (cdt.edge_tags[t as usize][k] & RING_TAGS) == 0
                    && assigned[next as usize] == 0
                {
                    assigned[next as usize] = 1;
                    stack.push(next);
                }
            }
        }

        let probe = plan_triangle(largest).centroid();
        let mut inside = true;
        for ring in rings {
            let where_ = ring.polygon.classify(probe);
            // OnBoundary means the whole region is thinner than GEOMETRIC.
            inside = if ring.tag == BOUNDARY_TAG {
                where_ == Containment::Inside
            } else {
                where_ == Containment::Outside
            };
            if !inside {
                break;
            }
        }
        if inside {
            for &t in &region {
                keep[t as usize] = 1;
            }
        }
    }
    keep
}

struct RingInput<'a> {
    polygon: &'a Polyline2,
    name: String,
    tag: u8,
}

/// The unclipped surface of the points and breaklines, built on first use. It
/// supplies the elevation of ring vertices that are not data vertices, and of
/// vertices where rings cross each other.
struct Reference<'a> {
    surface: Option<TinSurface>,
    data_positions: &'a [Point2],
    breakline_constraints: &'a [CdtConstraint],
    options: TinBuildOptions,
}

impl Reference<'_> {
    fn elevation(&mut self, data_elevations: &[f64], position: Point2, what: &str) -> Result<f64> {
        if self.surface.is_none() {
            let cdt = cdt_backend::triangulate(&CdtInput {
                points: self.data_positions,
                constraints: self.breakline_constraints,
                export_constraint_vertices: true,
            })?;
            let mut elevations = data_elevations.to_vec();
            let mut scratch = TinBuildReport::default(); // crossings are reported by the final pass
            let mut unresolved = Vec::new();
            resolve_crossing_elevations(
                &cdt,
                self.breakline_constraints,
                self.data_positions,
                &self.options,
                &mut elevations,
                &mut scratch,
                &mut unresolved,
            )?;
            let masks = constraint_masks(&cdt.edge_tags);
            let vertices = make_vertices(self.data_positions, &cdt.generated_points, &elevations);
            self.surface = Some(TinSurface::assemble(
                vertices,
                cdt.triangles,
                cdt.neighbors,
                masks,
            )?);
        }
        let z = self
            .surface
            .as_ref()
            .and_then(|surface| surface.elevation_at(position));
        match z {
            Some(z) => Ok(z),
            None => Err(Error::new(
                ErrorCode::InvalidGeometry,
                format!(
                    "{what} lies off the triangulated data and is not a data vertex; \
                     it cannot be given an elevation"
                ),
            )
            .with_context(format!("at {}", describe_point(position)))),
        }
    }
}

/// Fails with
///   InvalidArgument       non-finite coordinate, invalid option value, too many
///                         vertices for 32-bit indices;
///   InvalidGeometry       breakline with fewer than 2 vertices, degenerate or
///                         self-intersecting ring, ring vertex off the data,
///                         elevation conflict under an ErrorOnConflict policy;
///   TriangulationFailure  fewer than 3 distinct positions, all positions
///                         collinear, nothing left after clipping, or a failure
///                         reported by the triangulation kernel.
pub fn build_tin(input: &TinInput, options: &TinBuildOptions) -> Result<TinBuildResult> {
    if !(options.elevation_tolerance >= 0.0) || !options.elevation_tolerance.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "elevationTolerance must be finite and not negative",
        )
        .with_context(describe_value(options.elevation_tolerance)));
    }

    // ---- validate coordinates ----------------------------------------------------
    let mut capacity = input.points.len() + input.boundary.vertices.len();
    for (i, point) in input.points.iter().enumerate() {
        if !point.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "point has a non-finite coordinate",
            )
            .with_context(format!("point={i}")));
        }
    }
    for (b, breakline) in input.breaklines.iter().enumerate() {
        if breakline.vertices.len() < 2 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "breakline has fewer than 2 vertices",
            )
            .with_context(format!("breakline={b}")));
        }
        for vertex in &breakline.vertices {
            if !vertex.is_finite() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "breakline vertex has a non-finite coordinate",
                )
                .with_context(format!("breakline={b}")));
            }
        }
        capacity += breakline.vertices.len();
    }
    let mut ring_inputs: Vec<RingInput> = Vec::new();
    if !input.boundary.vertices.is_empty() {
        ring_inputs.push(RingInput {
            polygon: &input.boundary,
            name: String::from("boundary"),
            tag: BOUNDARY_TAG,
        });
    }
    for (h, hole) in input.holes.iter().enumerate() {
        ring_inputs.push(RingInput {
            polygon: hole,
            name: format!("hole {h}"),
            tag: HOLE_TAG,
        });
        capacity += hole.vertices.len();
    }
    for ring_input in &ring_inputs {
        for vertex in &ring_input.polygon.vertices {
            if !vertex.is_finite() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{} vertex has a non-finite coordinate", ring_input.name),
                ));
            }
        }
    }
    if capacity >= (NO_VERTEX - 1) as usize {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "too many vertices for 32-bit indices",
        )
        .with_context(format!("vertices={capacity}")));
    }

    // ---- vertex table: points, then breakline vertices, then ring vertices -------
    let mut report = TinBuildReport::default();
    let mut table = VertexTable::new(capacity, *options);

    let mut point_ids: Vec<u32> = Vec::with_capacity(input.points.len());
    for (i, &point) in input.points.iter().enumerate() {
        point_ids.push(table.add_elevated(point, false, &format!("point {i}"), &mut report)?);
    }

    let mut constraints: Vec<CdtConstraint> = Vec::new();
    for (b, breakline) in input.breaklines.iter().enumerate() {
        let mut ids: Vec<u32> = Vec::with_capacity(breakline.vertices.len());
        for (v, &vertex) in breakline.vertices.iter().enumerate() {
            ids.push(table.add_elevated(
                vertex,
                true,
                &format!("breakline {b} vertex {v}"),
                &mut report,
            )?);
        }
        let before = constraints.len();
        let segments = if breakline.closed {
            ids.len()
        } else {
            ids.len() - 1
        };
        for s in 0..segments {
            let from = ids[s];
            let to = ids[(s + 1) % ids.len()];
            if from != to {
                constraints.push(CdtConstraint {
                    from,
                    to,
                    tag: BREAKLINE_TAG,
                });
            }
        }
        if constraints.len() == before {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "breakline has no length (all vertices coincide)",
            )
            .with_context(format!("breakline={b}")));
        }
    }
    table.finish_elevated();
    let data_vertex_count = table.size();
    let breakline_constraint_count = constraints.len();

    let mut rings: Vec<Ring> = Vec::new();
    for ring_input in &ring_inputs {
        let mut ids: Vec<u32> = Vec::new();
        for &vertex in &ring_input.polygon.vertices {
            let id = table.add_plan_only(vertex);
            if ids.last() != Some(&id) {
                ids.push(id);
            }
        }
        if ids.len() > 1 && ids.first() == ids.last() {
            ids.pop(); // the first vertex repeated to close the ring
        }
        let polygon = Polyline2::new(
            ids.iter()
                .map(|&id| table.merger.positions()[id as usize])
                .collect(),
            true,
        );
        if orientation(&polygon) == Orientation::Degenerate {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                format!(
                    "{} must be a polygon with at least 3 distinct vertices and a non-zero area",
                    ring_input.name
                ),
            )
            .with_context(format!("distinct vertices={}", ids.len())));
        }
        let first_constraint = constraints.len();
        for e in 0..ids.len() {
            constraints.push(CdtConstraint {
                from: ids[e],
                to: ids[(e + 1) % ids.len()],
                tag: ring_input.tag,
            });
        }
        rings.push(Ring {
            name: ring_input.name.clone(),
            tag: ring_input.tag,
            ids,
            polygon,
            first_constraint,
        });
    }

    let positions: Vec<Point2> = table.merger.positions().clone();
    let mut elevations: Vec<f64> = std::mem::take(&mut table.elevations);

    // ---- reference surface ---------------------------------------------------------
    // Points and breaklines only, not clipped. Built only when a vertex needs it.
    let mut reference = Reference {
        surface: None,
        data_positions: &positions[..data_vertex_count],
        breakline_constraints: &constraints[..breakline_constraint_count],
        options: *options,
    };

    for id in data_vertex_count..positions.len() {
        let z = reference.elevation(
            &elevations[..data_vertex_count],
            positions[id],
            "boundary or hole vertex",
        )?;
        elevations[id] = z;
    }

    // ---- triangulation -------------------------------------------------------------
    let mut cdt = cdt_backend::triangulate(&CdtInput {
        points: &positions,
        constraints: &constraints,
        export_constraint_vertices: true,
    })?;

    let mut unresolved: Vec<u32> = Vec::new();
    resolve_crossing_elevations(
        &cdt,
        &constraints,
        &positions,
        options,
        &mut elevations,
        &mut report,
        &mut unresolved,
    )?;
    for &vertex in &unresolved {
        let z = reference.elevation(
            &elevations[..data_vertex_count],
            cdt.generated_points[vertex as usize - positions.len()],
            "crossing of boundary/hole edges",
        )?;
        elevations[vertex as usize] = z;
    }

    let mut visited = vec![0u8; positions.len() + cdt.generated_points.len()];
    for ring in &rings {
        require_simple_ring(ring, &cdt, &positions, &mut visited)?;
    }

    let mut vertices = make_vertices(&positions, &cdt.generated_points, &elevations);
    let mut masks = constraint_masks(&cdt.edge_tags);

    // ---- clip and compact ----------------------------------------------------------
    if !rings.is_empty() {
        let keep = select_triangles(&cdt, &vertices, &rings);

        let mut new_triangle = vec![NO_TRIANGLE; cdt.triangles.len()];
        let mut new_vertex = vec![NO_VERTEX; vertices.len()];
        let mut kept_triangles: u32 = 0;
        for (t, &kept) in keep.iter().enumerate() {
            if kept != 0 {
                new_triangle[t] = kept_triangles;
                kept_triangles += 1;
                for &v in &cdt.triangles[t] {
                    new_vertex[v as usize] = 0; // mark as used
                }
            }
        }
        if kept_triangles == 0 {
            return Err(Error::new(
                ErrorCode::TriangulationFailure,
                "no triangle is left inside the boundary and outside the holes",
            ));
        }
        let mut kept_vertices: u32 = 0;
        for v in 0..vertices.len() {
            if new_vertex[v] != NO_VERTEX {
                new_vertex[v] = kept_vertices;
                vertices[kept_vertices as usize] = vertices[v]; // monotone: never overwrites unread data
                kept_vertices += 1;
            }
        }
        report.clipped_triangle_count = keep.len() - kept_triangles as usize;
        report.dropped_vertex_count = vertices.len() - kept_vertices as usize;
        vertices.truncate(kept_vertices as usize);

        // A monotone renumbering keeps every triangle's rotation and the
        // lexicographic order, so the compacted arrays are still canonical.
        for t in 0..keep.len() {
            if keep[t] == 0 {
                continue;
            }
            let target = new_triangle[t] as usize;
            for k in 0..3 {
                let neighbor = cdt.neighbors[t][k];
                cdt.triangles[target][k] = new_vertex[cdt.triangles[t][k] as usize];
                cdt.neighbors[target][k] = if neighbor == NO_INDEX {
                    NO_TRIANGLE
                } else {
                    new_triangle[neighbor as usize]
                };
            }
            masks[target] = masks[t];
        }
        cdt.triangles.truncate(kept_triangles as usize);
        cdt.neighbors.truncate(kept_triangles as usize);
        masks.truncate(kept_triangles as usize);

        report.point_vertex = point_ids
            .iter()
            .map(|&id| new_vertex[id as usize])
            .collect();
    } else {
        report.point_vertex = point_ids;
    }

    let surface = TinSurface::assemble(vertices, cdt.triangles, cdt.neighbors, masks)?;
    Ok(TinBuildResult { surface, report })
}
