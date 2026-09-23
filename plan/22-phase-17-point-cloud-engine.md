<!-- Katana plan, section 22 of 47. Index: ../PLAN.MD. Previous: 21-phase-16-3d-cad.md. Next: 23-phase-18-spatial-indexing.md -->

# 22. Phase 17 — Point Cloud Engine

**STATUS: PARTIALLY DELIVERED.**

PDAL behind `katana::pointcloud` (`src/katana_io/point_cloud_engine.cpp`); no
PDAL type appears in a public header. LAS and LAZ read and write, with crop,
classification filtering and decimation applied INSIDE the pipeline so the
points are discarded before they are ever materialised.

`readHeader()` reads only the header, so the importer sizes its decimation from
the real point count before committing to a read: opening a 400-million-point
file costs what opening a small one costs. `decimationForBudget()` rounds up, so
the result is at or under the stated budget rather than just over it.

Displayed in the 2D viewport, coloured by elevation, intensity, ASPRS
classification or the file's own RGB. Points are splatted into an image buffer
rather than drawn one QPainter call at a time, which is what keeps two million
points interactive; the per-point colours are cached and only the projection is
redone per frame.

**OUTSTANDING: the out-of-core half of this phase - but NOT as written.**
There is no level of detail and no streaming: the decimated sample is held in
memory, so a billion-point dataset is opened as a sample of it rather than
worked on in full. Sections 32 and 33 govern what that will need.

**The plan is wrong about how.** It asks Katana to build a spatial hierarchy.
It should not build one. **COPC** (Cloud Optimised Point Cloud) is a LAZ file
whose chunks are already arranged as an octree, and the PDAL already linked
into `katana_io` reads and writes it natively - verified on this machine:
`pdal --drivers` lists **`readers.copc`** and **`writers.copc`**. `readers.copc`
takes a `resolution` argument and returns only the octree levels coarse enough
to matter, so level of detail becomes a *query parameter* rather than a data
structure Katana owns, maintains and has to keep correct under edits.

**Engine half DELIVERED.** `PointCloudReadOptions::resolution`,
`PointCloudEngine::convertToCopc` and `isCopc`. A resolution asked of a file
that is not COPC is refused rather than ignored, because `readers.las` would
silently return the whole file; the destination must be named `.copc.laz`
because the extension is what makes later reads infer `readers.copc`. Tested
on a 160 000-point cloud converted inside the test, asserting the properties
of level of detail rather than one PDAL version's counts. Record in
`docs/interop.md`.

**Asked by a person (2026-09-23):** GIS > Convert Point Cloud to COPC and the
CLI's `COPC` verb convert a file, keeping its coordinate precision (the
source's scale and offset are forwarded - audit IO-17 had them quantised to
1 cm), and GIS > Import Point Cloud offers a point spacing for a COPC file
(`PointCloudImportOptions::resolution`).
**OUTSTANDING:** nothing converts on import by itself, and no viewport
re-queries at a resolution derived from the current `worldPerPixel` - the
same quantity the renderer already computes; until then the display is still
the one sample it was given. Building a
bespoke octree would duplicate, in Katana, a structure that is already in the
file format, already in the linked library, and already an OGC standard.

---

