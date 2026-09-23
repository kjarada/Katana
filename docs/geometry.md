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
* **Lengths** of `Vec2` and `Vec3` use `std::hypot`, which does not overflow or
  underflow for extreme components; `Vec2(3e200, 4e200).length()` is exactly
  `5e200`. `Vec4` and `Quaternion` use the square root of the squared length
  (this said every length used `std::hypot`).
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
inside PLAN.MD §32's 16 ms selection budget for interactive use. (This said it
was the call a spatial index should accelerate first; the index of Phase 18 is a
broad phase for picking, snapping, box selection and repaint and does not touch
`Polyline2::classify`, which is still the O(n) pair.)

`isSimple`, `clipPolygon` and `Rectangle2::closestPoint` were optimised on
2026-09-23, measured, below; nothing else here has been, per PLAN.MD Rules 5
and 6 — tests first, profile before optimising.

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

## The clothoid (`Spiral2`)

`include/katana/geometry/spiral2.hpp` is the transition curve every road and
railway standard uses: curvature changes linearly with distance, which is the
path a vehicle follows when the wheel is turned at a steady rate. It is the
first slice of Phase 21, and the only primitive that phase needs which did not
already exist.

### Decisions

**The general form is kept, not the textbook one.** The design texts give the
spiral from a straight into a curve of radius R. A real alignment also needs
the transition between two curves of different radius, and the reverse spiral
that leaves a curve. All three are one object with a start and an end
curvature; `fromStraightToRadius` is a named constructor for the common case,
not a separate type.

**Quadrature, not the power series.** The texts give x and y as series in the
deflection angle (`x = L(1 - t^2/10 + t^4/216 - ...)`). Those are exact only
for a spiral starting from zero curvature - the general case has no such closed
form. Composite ten-point Gauss-Legendre over panels that each turn at most
half a radian handles every case with the same code and is exact to well
beyond double precision: on a half-radian panel the trigonometric integrand is
so nearly a polynomial of degree 19 that the estimated error is below 1e-20
relative. The series was still derived by hand from the Fresnel integrand,
because it is an independent check on the quadrature (see below).

**Panel count from the larger absolute curvature, not the signed turn.** A
reverse spiral from +1/100 to -1/100 turns left, straightens, and turns right;
its signed total turn is exactly zero. A panel count taken from that would be
one panel over a curve that bends both ways. The bound uses
`max(|k0|, |k(s)|) * s`, which over-divides slightly when the curvature
changes sign and is never too coarse.

**A right-hand spiral is the EXACT reflection of the left-hand one**, not one
to within rounding, and the test asserts equality, not nearness. What makes it
so is that every angle of the mirror is the exact negation of the original's and
libm's `cos` is even and `sin` odd - so each term is exactly mirrored. This used
to credit evaluating symmetric Gauss-Legendre node pairs together; a sequential
sum gives the same result, so the pairing is not what the test protects, and a
libm whose `sin(-t)` is not exactly `-sin(t)` is what would break it.

**`asArc()` exists so the constant-curvature case is not integrated.** A
spiral with `k0 == k1` is a circular arc, and the section and stationing code
already has closed-form arc arithmetic. Reporting it as an `Arc2` lets that
code use it. The quadrature would get the same answer; it would just be doing
work for nothing.

**Chords by the sagitta rule at the tightest radius.** `chordCountFor` applies
the rule `cad::chordArc` uses, at `max(|k0|, |k1|)`, so every chord along the
spiral is within tolerance and not only the average one.

### How it was tested

Every reference value comes from outside the code, by at least one of three
routes that share nothing with it, and where two were available they agree to
every digit quoted:

* **Published constants.** With `A * sqrt(pi) = 1` the spiral's coordinates
  are the Fresnel integrals `C(s)` and `S(s)` in the pi/2 convention of
  Abramowitz & Stegun Table 7.7: `C(1) = 0.7798934003768228`,
  `S(1) = 0.4382591473903548`. Asserted to 1e-14. The `s = 2` case has turned
  through a full circle, so it also exercises the multi-panel path.
* **The hand-derived series** on an ordinary R = 300 m, L = 90 m rural-road
  transition: X = 89.797710828008, Y = 4.492773032666.
* **Adaptive Simpson** written in Python for the purpose, on the raw
  integrand with a 1e-15 tolerance. It reproduces both of the above to every
  printed digit, and is the only reference for the curve-to-curve and
  reverse-through-zero cases, which have no series.

Plus the closed forms: constant curvature lands on the circular arc's own
points to 1e-12, zero curvature is a straight line, and a central difference
of `pointAt` matches `tangentAt` to 1e-7 at four stations - which would fail
for a quadrature integrating the wrong function whatever the tables said.

One expectation in the first draft of the tests was wrong: the chord count for
a 1 mm tolerance on the 300 m spiral was written as 38 from a slip in the
sagitta arithmetic. The code gave 60. Before the test was re-run the number
was re-derived two ways - directly, and by the small-angle identity
`acos(1 - e) ~ sqrt(2e)` - and both gave 60. The test now says 60 because of
those derivations, not because the program did (CLAUDE.md section 3).

### Where it is used

`SolvedAlignment` stations along spirals and chords them, the viewport draws
alignments that contain them, and `ALIGN` prints setting-out tables through
them. (This section said nothing drew or stationed along a spiral, from before
the alignment slice of Phase 21.)

## Chording curves (`chording.hpp`)

The sagitta rule - chords sized so none departs from the curve by more than a
stated tolerance - lived in `cad::scene` as `chordArc` and `chordCircle`.
When `Spiral2::chordCountFor` was written it grew its own copy of the same
arithmetic, which is the second way of doing something that CLAUDE.md section
1 calls a defect. The rule now lives in `geometry::chording`, the lowest layer
that can see `Arc2`, and the scene builder, the spiral and the horizontal
alignment all call it.

**Why the `cad` names were deleted rather than kept as forwarders.** The first
attempt kept `cad::chordArc` forwarding to `geometry::chordArc`. That does not
compile: the argument is an `Arc2` in `katana::geometry`, so argument-dependent
lookup finds the geometry function from any call site in `katana::cad`, and an
unqualified call becomes ambiguous between the two identical signatures. Two
functions with the same name and signature in two namespaces of the same
program are a trap for every future caller, not a convenience for the current
ones, so the `cad` pair went and their three tests moved to `tests/geometry`
with the code. `sagittaChordCount` is exposed alongside, so that a caller which
only needs the number - the spiral - uses the same clamps (a 1e-12 floor on
tolerance / radius, a 0.5 cap, at most 8192 chords) as one that needs the
points.

A fourth test pins the two callers to each other: `chordArc` on a 300 m arc
sweeping 0.3 rad at 1 mm yields exactly `sagittaChordCount(300, 0.3, 0.001) + 1`
points, and that count is 59 - the same 59 the spiral test derives for 90 m
on the same radius, because 90 m on r = 300 *is* a 0.3 rad sweep.

## The horizontal alignment (`alignment.hpp`)

The plan centreline of a road, railway, pipe or channel: the second slice of
Phase 21, built on the clothoid and the shared chording rule.

### Decisions

**Defined by PIs, with the elements derived.** An alignment is a sequence of
points of intersection - the corners of the tangent polygon - each carrying
the radius that rounds it and the lengths of the spirals into and out of the
curve. `solveAlignment` derives the tangents, spirals and arcs. The
alternative, storing the element list directly, was rejected because a list
can be edited into one with a gap or a kink between two elements, after which
every stationing and section query has to decide what that means. A PI cannot
be edited into a discontinuity: move it and the three elements at that corner
are recomputed to meet again. It is also what a designer actually edits. The
price is that a definition can be infeasible, and the solver's job is then to
say exactly which PI, not to draw something plausible.

**Tangent lengths, derived rather than recalled.** The design texts give the
spiral-curve-spiral tangent length only for equal spirals,
`T = k + (R + p) tan(D / 2)`. The general case was derived by placing the
shifted circle: its centre sits `R + p_in` off the back tangent and
`R + p_out` off the forward tangent, and each tangent point is `k` short of
the foot of that perpendicular, which gives
`T_in = k_in + ((R + p_out) - (R + p_in) cos D) / sin D` and the mirror for
`T_out`. Before any test was written around it the derivation was checked
independently: the chain was laid out in Python with adaptive-Simpson spirals
and its end measured against the forward tangent line - 3e-15 m, 2e-14 m and
2e-14 m off for a simple curve, equal spirals and unequal spirals on a right
turn. With equal spirals it reduces to the published formula to twelve
decimals, and with none to `R tan(D / 2)` exactly.

**The chain end is not snapped to the computed ST.** After the spiral-out is
laid, the cursor is wherever the chain actually ended, and the next tangent
starts there. If the tangent-length derivation were wrong the chain would end
off the forward tangent and the next tangent would visibly kink. Snapping
would hide precisely that, so the test that checks every joint for continuity
in position and direction would be testing the snap, not the solver.

**Zero-length tangents are not stored.** Two curves back to back - a reverse
curve whose PIs are exactly a tangent length apart - leave nothing between
them. An element of zero length has no direction to report and would make
every station query at its joint ambiguous, so it is skipped, and a test
asserts that a reverse curve solves with no such element.

**The central arc is built as a constant-curvature `Spiral2` and converted
with `asArc()`.** The arc's centre and start angle then follow exactly the
conventions the spirals on either side use, instead of a second piece of
centre arithmetic that could disagree with them by a sign.

**A radius of zero is a kink, not an error.** An alignment traced from a
surveyed polyline has no curves at all; it must solve and station.

### What is refused, and why each names its PI

Fewer than two PIs; consecutive PIs closer than `kCoordinate` (no tangent
direction); a reversal through 180 degrees at a PI with a radius (nothing can
round it); spirals whose angles exceed the deflection (no central arc left);
neighbouring curves whose tangent lengths overlap (the tangent between them
would be negative - reported with the shortfall in metres). Every one names
the PI or pair of PIs, because the useful reply to "the curve does not fit"
is which corner to move.

### In the application

`entity::Alignment` is the third `NamedTable` (`docs/model.md`), holding the
PI definition and nothing else; the elements are derived on demand, so a
stored alignment cannot be discontinuous, and `add()` solves the definition
so one that cannot be built is refused naming its PI - including one arriving
from a file, which a test proves by hand-editing a saved row. Schema 7 stores
`alignments` and `alignment_pis`. The `ALIGN` verb defines and edits them and
prints a setting-out table that always includes the key stations; the
viewport draws every alignment as an amber overlay with a tick and chainage
label at each key station; and Terrain > Cut Section Along Alignment cuts a
section down one by name, sharing the section code with the selection path
rather than duplicating it. Details in `docs/cad.md`.

### Built on it since

Profiles, corridor quantities and the corridor surface, and parcels are built
(PLAN.MD section 26); grading has an engine without a GUI route. Nothing
references an alignment BY NAME in the model, so `deleteAlignment` still has no
in-use guard - but `ViewportCell::sectionAlignment` is declared as such a
reference and is dead, which is either to be removed or to become the first
thing the guard protects. (This section listed profiles, corridors and parcels
as not done.)

## The vertical alignment (`profile.hpp`)

Elevation against station along a horizontal alignment: the design grade line.
Built the same way as the horizontal alignment and for the same reasons.

### Decisions

**Defined by PVIs; the tangents and curves are derived.** A PVI is a station,
an elevation and the length of the curve that rounds the grade change there.
A PVI cannot be edited into a discontinuity, and a definition that cannot be
built is refused naming the PVI - the same argument as for the horizontal
alignment, and the same shape of code, so that anyone who has read one has
read the other.

**Symmetric parabolas, and only those.** Every road and rail standard
specifies the parabola for a vertical curve because its rate of change of
grade is constant: the vertical acceleration a vehicle feels is constant
through the curve, and sight distance has a closed form. Asymmetric
(unequal-tangent) curves exist in the texts and are not provided; they are
rare and nothing in the plan asks for one. Recorded so that the omission
reads as a decision rather than an oversight.

**Continuity by identity, not by adjustment.** A parabola from the PVC with
entry grade g1 ends at the PVT at `z_PVI + g2 L / 2`, which is exactly where
the outgoing tangent starts. The solver relies on that identity and does not
snap; the joint-continuity test checks elevation and grade from both sides at
every joint of a three-curve profile, and would show a step if the identity
had not survived the arithmetic.

**One formula for both element kinds.** `z = z0 + g1 x + (g2 - g1) x^2 / 2L`
is the parabola; on a tangent `g2 == g1` and the quadratic term vanishes. One
evaluator rather than two that could disagree at a joint.

**High and low points are closed-form.** The grade is linear across a curve,
so it passes through zero inside the curve exactly when the entry and exit
grades differ in sign, at `x = g1 L / (g1 - g2)`. A low point on a sag is
where water collects and a high point on a crest is where sight distance is
tightest, so both are reported with the PVI they belong to.

**Zero-length tangents between back-to-back curves are not stored**, as in
the horizontal alignment, and a curve on the first or last PVI is refused
because there is no grade beyond it to blend into.

### How it was tested

The two standard textbook curves, worked by hand and then confirmed by
evaluating the parabola directly and finding the extremum by a 200 000-step
scan rather than by the closed form the code uses:

* sag, -3% into +2% over 200 m at PVI 1000 / 100.0: PVC 900 @ 103.0, PVT
  1100 @ 102.0, low point 1020 @ 101.2, grade there exactly 0;
* crest, +4% into -2% over 300 m at PVI 500 / 150.0: PVC 350 @ 144.0, PVT
  650 @ 147.0, high point 550 @ 148.0.

Every number is exact in decimal, so the tolerances are rounding only.

### In the application

`entity::Alignment::vertical` holds the profile as an `optional`, because an
alignment traced only to cut a section has no design and should not pretend
to an empty one; `validate` solves it, so an unbuildable profile is refused
naming its PVI, including one arriving from a file. Schema 8 stores
`alignment_pvis`, and the reader creates the profile on its first row so an
alignment with none comes back without one - a test proves that, because an
empty profile would fail to solve and refuse the whole load. `ALIGN DESIGN`
defines a profile, `PVI` appends, `PROFILE` prints it with its high and low
points, and `cad::appendDesignProfile` adds it to a section as a series
sampled at every ground station plus the profile's own key stations, so the
section view draws design against ground and the low point of a sag lands
where the water will. Details in `docs/cad.md`.

### Built on it since

Corridor cut and fill quantities between design and ground are integrated along
the alignment (PLAN.MD section 26), which is what this section said was
missing.

## Rejecting early without changing the answer (`polygon.hpp`, `primitives2d.hpp`)

`isSimple` is O(n^2) and runs on validation, so its constant matters: a
512-vertex boundary took 3.2 ms and takes 0.80 ms. The saving is a bounding-box
reject before the segment-segment intersection, with a margin of `4 *
kGeometric` - **derived, not chosen**: `intersect()` reports a hit when the
segments come within `2 * kGeometric` of each other, so two boxes further apart
than twice that cannot contain a reported intersection. Removing the margin
fails two polygon tests, which was checked by removing it.

`clipPolygon` swaps its two vertex buffers rather than move-assigning them, so
Sutherland-Hodgman's per-clip-edge lists are allocated once instead of once per
edge. `Rectangle2::closestPoint` walks a stack `std::array<Point2, 4>` instead
of building a `std::vector` of corners on every call, and is 2.8x faster for
it. This said it is called once per candidate entity on every pick and snap; it
is not - `Rectangle2` is not an entity geometry, and in production it is only
built by `fromCorners()` and turned into a polyline - so the saving is real but
lands nowhere hot.

The benchmarks for all three are in `benchmarks/bench_geometry.cpp`; they did
not exist when the changes were made, which is recorded in
`docs/performance.md` as a process failure rather than a footnote.
