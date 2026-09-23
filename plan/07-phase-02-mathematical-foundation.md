<!-- Katana plan, section 7 of 47. Index: ../PLAN.MD. Previous: 06-phase-01-build-system.md. Next: 08-phase-03-basic-geometry.md -->

# 7. Phase 02 — Mathematical Foundation

**STATUS: DELIVERED.** `include/katana/math`, header-only, 50 tests at delivery.

* `numerics.hpp` - the single tolerance policy (section 35). `kAbsolute` 1e-12,
  `kRelative` 1e-9, `kAngular` 1e-10 rad, `kGeometric` 1e-7 model units,
  `kCoordinate` 1e-4 m. Every constant carries its justification; 1 ulp at a UTM
  northing of 1e7 is about 1.9e-9 m, which is what sets the floor.
* `vec2/3/4`, `mat2/3/4` (row-major, acting on column vectors), `quaternion`
  (Hamilton, shortest-arc `slerp`), `transform` (TRS with an analytic inverse),
  `primitives` (Plane, Ray, AABB).

`operator==` is exact and `nearlyEqual` is separate: fuzzy equality is not
transitive and must never hide inside an operator. `nearlyEqual` guards
non-finite inputs explicitly, because `inf <= eps * inf` would otherwise make
infinity compare equal to any large number.

---

