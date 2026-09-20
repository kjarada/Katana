# Mathematics and geometry

`katana_math` (header-only) and `katana_geometry` (static library). These are
the foundation every other module computes on, and they depend on nothing but
the standard library.

## Purpose

`katana_math` provides the numerical policy and the small value types —
vectors, matrices, quaternions, transforms and the primitives used for picking.
`katana_geometry` provides 2D drafting geometry: primitives, intersections,
polygon algorithms and the curve-editing operations behind the CAD commands.

## Data model

All types are plain values with public data, default construction and
`constexpr` operations where possible. They are small enough to pass by value,
and nothing in either module allocates except the containers that obviously do
(`Polyline2::vertices`, triangulation output).

| Type | Notes |
|---|---|
| `Vec2`, `Vec3`, `Vec4` | `operator==` is exact; use `nearlyEqual` for tolerance |
| `Mat2`, `Mat3`, `Mat4` | row-major, acting on column vectors: `v' = M * v` |
| `Quaternion` | `(x, y, z, w)`, Hamilton convention, `w` scalar |
| `Transform` | translation × rotation × scale, applied as `T(R(S(p)))` |
| `Plane`, `Ray`, `AABB` | `AABB` default-constructs empty (inverted infinite bounds) so it can be grown |
| `Point2`, `Line2`, `Segment2`, `Circle2`, `Arc2`, `Polyline2`, `Rectangle2`, `Triangle2`, `Box2` | 2D drafting primitives |
| `Segment3`, `Triangle3` | 3D, used by terrain and picking |
| `Curve2` | `variant<Segment2, Arc2, Circle2>`, the editable curve types |

Conventions, applied everywhere:

* Angles are radians, counter-clockwise from +x; `Arc2` stores a start angle and
  a signed `sweep`, so direction is explicit and reversal is exact.
* "Left" of a directed line is the side its counter-clockwise normal points to.
* A closed `Polyline2` bounds a polygon; positive signed area is
  counter-clockwise.
* Degenerate values are representable (zero-length segment, zero radius). Every
  operation documents how it treats them rather than assuming they cannot occur.

## Numerical assumptions

All tolerances come from `math::tolerance` — see
[architecture.md](architecture.md) for the table and its justification. Within
these two modules specifically:

* **Parallelism** is decided on the sine of the angle between directions
  (`|u × v| ≤ kAngular · |u| · |v|`), which is scale-invariant. Lines meeting at
  less than `kAngular` are reported parallel rather than producing an
  intersection point millions of units away that no user could have intended.
* **Singularity** of a matrix is decided by comparing `|det|` against
  `kAbsolute` times the product of the row norms (a Hadamard-style bound), so
  the test is invariant to overall scale: a well-conditioned matrix scaled by
  1e-9 is still invertible.
* **Lengths** use `std::hypot`, which does not overflow or underflow for
  extreme components; `Vec2(3e200, 4e200).length()` is exactly `5e200`.
* **Chord accuracy** near tangency uses the factored form
  `sqrt((r - d)(r + d))` rather than `sqrt(r² - d²)`, which loses precision when
  `d ≈ r` — exactly the case that matters for a near-tangent line.
* **Area, centroid and circumcircle** are computed relative to a local origin
  (the first vertex, or vertex `a`), preserving precision at projected
  coordinate magnitudes.

## API

### Intersections

Every pair of primitives returns the same `IntersectionResult`:

| Kind | Meaning |
|---|---|
| `None` | no common point |
| `Points` | one or two isolated points; `tangent` is set when the curves touch without crossing |
| `Overlap` | infinitely many common points; for bounded overlaps the two ends are reported |

Points come back in a deterministic order (increasing parameter along the first
operand), which makes results reproducible and testable. Supported pairs:
line/line, line/segment, segment/segment, line/circle, segment/circle,
circle/circle, line/arc, segment/arc, circle/arc, arc/arc, plus symmetric
overloads so argument order never matters.

The cases that make this code non-trivial are all covered by tests: parallel,
collinear-overlapping, collinear-touching, collinear-with-a-gap, zero-length
operands, shared endpoints, T-junctions, external and internal tangency,
concentric and coincident circles, arcs that share a supporting circle, and
nearly parallel lines at the `kAngular` boundary.

### Polygons

`orientation`, `isConvex`, `isSimple`, `convexHull` (Andrew's monotone chain),
`simplify` (Douglas–Peucker), `clip` of a segment to a box (Liang–Barsky),
`clipPolygon` against a convex region (Sutherland–Hodgman), and `triangulate`
(ear clipping).

`triangulate` returns counter-clockwise index triples regardless of input
winding, skips vertices collinear with their neighbours, and fails with
`TriangulationFailure` — naming the number of remaining vertices — when no ear
can be found, which means the polygon is not simple. It is O(n²); that is
adequate for drafting polygons and is documented rather than hidden. Polygons
with holes and boolean set operations are **not** implemented; `clipPolygon`
requires a convex clip region and says so.

### Editing

`offset`, `trim`, `extend`, `fillet` and `chamfer`, all pure functions from
geometry to geometry.

* `offset` of a polyline uses mitre joins and is valid only while the distance
  stays below the local feature size; beyond that the result self-intersects.
  This is stated in the header rather than silently produced. A path that
  doubles back on itself is detected and returns `InvalidGeometry`.
* `trim` removes the portion containing the pick point, bounded by
  intersections with the cutters. A trimmed circle becomes an arc. A circle
  needs two crossings to be trimmed; one (a tangent) is an error.
* `extend` lengthens the end nearest the pick point to the first boundary ahead
  of it. Circles have no end to extend and are rejected.
* `fillet` and `chamfer` operate on the corner where the two supporting lines
  meet, keeping the far end of each segment, so they work whether the segments
  currently reach the corner, stop short of it, or overshoot it. The tangent
  distance is `r / tan(θ/2)` and the arc centre sits at `r / sin(θ/2)` along the
  bisector; both are verified in tests by checking the centre is exactly one
  radius from each supporting line.

## Threading and ownership

Both modules are free functions and value types: no shared mutable state, no
globals, nothing to synchronise. Any function may be called concurrently on
distinct arguments. Callers own all inputs and outputs.

## Performance

Operations are O(1) except where noted: `triangulate` and `isSimple` are O(n²),
`convexHull` is O(n log n), `simplify` is O(n log n) typical. Arc bounding boxes
check the four cardinal directions rather than sampling.

Measured on a 16-thread 2.5 GHz x86-64, Release, single-threaded
(`benchmarks/bench_math.cpp` and `bench_geometry.cpp`; run them with
`cmake --build <dir> --target run-benchmarks`):

| Operation | Throughput / time |
|---|---|
| `transformPoint` through a `Mat4` | 600–730 M points/s |
| `Quaternion::rotate` | 437 M points/s |
| `Mat4::inverse` | 17 ns |
| segment/segment intersection | 26–29 M pairs/s |
| circle/circle intersection | 73 M pairs/s |
| `Polyline2::area` | 390–410 M vertices/s |
| point-in-polygon, 1024-vertex ring | 11.6 µs per probe |
| `triangulate`, 256 / 1024 vertices | 133 µs / 1.76 ms |
| `convexHull`, 65 536 points | 5.6 ms |
| `simplify`, 4096 vertices | 565 µs |
| polyline `offset`, 4096 vertices | 283 µs |
| `fillet` | 92 ns |

Two numbers set expectations for later phases. `triangulate` grows quadratically
— 13× the time for 4× the vertices — so terrain-scale triangulation uses CGAL's
constrained Delaunay rather than this routine, which exists for drafting-scale
polygons. And point-in-polygon is O(n) twice over (a boundary-distance pass and
a crossing-number pass); at 11.6 µs against a 1024-vertex ring it is comfortably
inside PLAN.MD §32's 16 ms selection budget for interactive use, but it is the
call that a spatial index (Phase 18) should accelerate first.

No further optimisation has been applied, per PLAN.MD Rules 5 and 6 — tests
first, profile before optimising.

## Failure modes

| Condition | Result |
|---|---|
| Singular matrix | `inverse()` returns `nullopt` |
| Zero-length direction | `normalized()` returns the zero vector; `Line2::through` returns `nullopt` |
| Collinear points | `Plane::fromPoints`, `Triangle2::circumcircle`, `Arc2::throughPoints` return `nullopt` |
| Degenerate triangle | `barycentric` returns `nullopt` |
| Non-simple polygon | `triangulate` returns `TriangulationFailure` |
| Non-convex clip region | `clipPolygon` returns `InvalidGeometry` |
| Offset distance too large for a circle/arc | `nullopt` or `InvalidGeometry` |
| Fillet radius too large for the segments | `InvalidGeometry` |
| Parallel segments filleted | `InvalidGeometry` — no corner exists |
