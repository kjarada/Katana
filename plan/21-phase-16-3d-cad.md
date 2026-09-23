<!-- Katana plan, section 21 of 47. Index: ../PLAN.MD. Previous: 20-phase-15-3d-rendering.md. Next: 22-phase-17-point-cloud-engine.md -->

# 21. Phase 16 — 3D CAD

**STATUS: THE PLAN IS WRONG HERE. RESHAPED, NOT STARTED.**

This phase said "integrate Open CASCADE". Section 5 of this plan licenses
saying so when the plan asks for something that turns out to be a bad idea.
This is one. The verdict below was checked against the machine, not argued
from taste.

**Why not Open CASCADE:**

* **It is not a dependency, it is a second application.** Not installed here
  (`pacman -Q mingw-w64-ucrt-x86_64-opencascade` -> not found). `pacman -Si`:
  36.84 MiB to download, **261.70 MiB installed**, and it depends on **VTK,
  Tcl, Tk, FFmpeg, OpenVR, FreeImage and TBB**. Katana's entire `build/release`
  is a fraction of that. Rule 4 keeps third-party types out of public headers,
  and this would satisfy Rule 4 while still making the shipped product an
  Open CASCADE distribution with a CAD program attached.
* **It brings a second tolerance system.** OCCT carries per-shape tolerances
  that propagate through boolean operations. Section 35 and `math::tolerance`
  are the single tolerance policy. Two is a defect (CLAUDE.md section 1).
* **It brings a second parallelism system.** OCCT parallel booleans are TBB.
  `katana::core::TaskPool` exists to make parallelism *deterministic* - Rule 7.
  TBB's work stealing gives no such guarantee, so an OCCT boolean could return
  a different shape on a different machine.
* **`TopoDS_Shape` cannot live in the entity model.** It is a handle into a
  shared `TShape` graph, not a copyable value. `entity::Geometry` is a
  `std::variant` of plain values that the before-image undo copies wholesale
  and `geometryToBlob` serialises field by field. A B-rep would need its own
  ownership, its own undo and its own serialisation - see `docs/model.md`.
* **The civil work does not need it.** `katana::geometry::volume` already
  computes cut and fill between surfaces *exactly*: "Nothing is sampled: the
  results are exact up to floating-point rounding (sums are compensated)"
  (`volume.hpp:9`). A B-rep boolean would replace an exact answer with a
  tolerance-dependent one. That is a regression dressed as a feature.

**What this phase should be instead: 3D solids for civil, built on the TIN.**
The precedent is already set - `TinSurface` appears nowhere in
`entity/entity.hpp` (`grep -c` -> 0); a surface is a referenced dataset the
model names, not an eighth geometry kind. A solid follows the same shape: a
`TriangleMesh` dataset, produced by extruding a closed plan polygon between two
surfaces or two elevations, with volume from the existing exact code.

NURBS, fillet, chamfer, revolve and sweep are **removed from this plan**. They
are mechanical-CAD features. Nothing in sections 1-20 asks for them, and no
survey or civil workflow described anywhere in this document uses one. If a
real requirement for machined solids ever arrives, it arrives with a user, and
Open CASCADE can be reconsidered then - behind an optional `KATANA_BUILD_BREP`
module, the way GDAL and PDAL are already optional.

---

