<!-- Katana plan, section 9 of 47. Index: ../PLAN.MD. Previous: 08-phase-03-basic-geometry.md. Next: 10-phase-05-entity-system.md -->

# 9. Phase 04 — Geometry Algorithms

**STATUS: DELIVERED.** 76 tests at delivery, benchmarked in `benchmarks/bench_geometry.cpp`.

* `intersection.hpp` - one `IntersectionResult` for every pair: line/line,
  line/segment, segment/segment, line/circle, segment/circle, circle/circle,
  line/arc, segment/arc, circle/arc, arc/arc, plus symmetric overloads. Near
  tangency uses the factored `sqrt((r-d)(r+d))`.
* `polygon.hpp` - orientation, isConvex, isSimple, convexHull (monotone chain),
  simplify (Douglas-Peucker), clip (Liang-Barsky), clipPolygon
  (Sutherland-Hodgman), triangulate (ear clipping, O(n^2), documented).
* `editing.hpp` - offset, trim, extend, fillet, chamfer over
  `Curve2 = variant<Segment2, Arc2, Circle2>`.

Measured figures are recorded in `docs/geometry.md`.

---

