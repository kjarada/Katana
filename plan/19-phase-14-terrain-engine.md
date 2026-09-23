<!-- Katana plan, section 19 of 47. Index: ../PLAN.MD. Previous: 18-phase-13-least-squares-adjustment.md. Next: 20-phase-15-3d-rendering.md -->

# 19. Phase 14 — Terrain Engine

**STATUS: DELIVERED.** `src/katana_terrain` (72 tests when this was written;
see the suite for the count now).

`tin_builder` (constrained Delaunay via CGAL, behind `cdt_backend.hpp`),
`tin_surface`, `contours`, `volume`, `tiled_terrain`, `point_merge`, and
`super_surface` (the combined surface of 20.2 slice 8). Breaklines, boundaries
and holes; duplicate-point policies; bitwise-deterministic rebuilds.

OUTSTANDING (stated in `tiled_terrain.hpp` and missing from this record until
the audit of 2026-09-23): the TILED terrain refuses breaklines, boundaries and
holes - `create()` fails with Unsupported rather than building a surface that
ignores them - and keeps every point in memory; out-of-core tiles are not
built.

---

