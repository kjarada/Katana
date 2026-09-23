<!-- Katana plan, section 8 of 47. Index: ../PLAN.MD. Previous: 07-phase-02-mathematical-foundation.md. Next: 09-phase-04-geometry-algorithms.md -->

# 8. Phase 03 — Basic Geometry

**STATUS: DELIVERED.** `include/katana/geometry`, 2D and 3D primitives.

`Box2`, `Line2`, `Segment2`, `Circle2`, `Arc2` (start angle plus signed sweep,
so reversal is exact), `Polyline2`, `Rectangle2`, `Triangle2`; `Segment3`,
`Triangle3` with Moller-Trumbore ray intersection. `Containment` is a tri-state
(`Outside`, `OnBoundary`, `Inside`) rather than a bool.

Area, centroid and circumcircle are computed relative to a local origin, because
a naive shoelace sum at projected coordinate magnitudes loses about six
significant digits.

---

