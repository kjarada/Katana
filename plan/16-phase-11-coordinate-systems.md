<!-- Katana plan, section 16 of 47. Index: ../PLAN.MD. Previous: 15-phase-10-survey-data-model.md. Next: 17-phase-12-survey-calculations.md -->

# 16. Phase 11 — Coordinate Systems

**STATUS: DELIVERED.** `src/katana_geodesy`, 66 tests at delivery.

PROJ confined behind `proj_internal.hpp`: `CoordinateReferenceSystem`,
`CoordinateTransformer` (reporting the operation actually selected, its accuracy
and whether it is a ballpark), `geodesic`, `grid_factors`, `units`,
`similarity_transform2d`. No PROJ type appears in a public header.

A conversion between CRSs that share a datum reports accuracy 0 rather than
"unknown", so exact is distinguishable from a ballpark operation.

---

