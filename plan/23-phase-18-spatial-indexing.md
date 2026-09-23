<!-- Katana plan, section 23 of 47. Index: ../PLAN.MD. Previous: 22-phase-17-point-cloud-engine.md. Next: 24-phase-19-performance-architecture.md -->

# 23. Phase 18 — Spatial Indexing

**STATUS: PARTIALLY DELIVERED — the 2D broad phase.**

`katana::geometry::SpatialIndex` (`include/katana/geometry/spatial_index.hpp`):
a sparse spatial hash grid over 2D bounding boxes with an overflow list for
boxes too large to bucket, giving O(1) insert, remove and update. It is a BROAD
phase: a query returns every id whose box overlaps, and the caller still runs
the exact geometric test, so swapping the scan for the index cannot change an
answer.

**Why a hash grid rather than the R-tree the phase names.** An R-tree handles
pathological size distributions better and is the textbook answer, but its
insertion, splitting and rebalancing are substantially more code, and every one
of those paths has to stay correct under the constant incremental edits a CAD
document makes. The grid is O(1) on all three, which is what keeps it honest
while the user is drawing. `oversizedCount()` is exposed so that the case where
an R-tree would win can be SEEN rather than guessed at; that is the recorded
upgrade trigger. A KD-tree or BVH was rejected for the same reason in reverse:
both are excellent for a static set and both want rebuilding after every edit.

**The subtlety worth knowing about.** The indexed box is NOT always the
geometric bounding box. Snapping an arc offers its CENTRE, which lies outside
the arc's own bounding box - far outside, for a shallow arc. Indexing plain
bounding boxes would have made the broad phase reject such an arc and centre
snap on arcs would have silently stopped working the moment the index was
enabled. The indexed box is therefore `detail::queryExtents`: the full circle
for an arc, the bounding box otherwise. Picking still tests the true bounding
box, so clicking an arc's centre does not select it. Both behaviours are
asserted.

`Document` owns an index and keeps it in step from the per-entity changes every
command reports, so a single click on a 250 000-entity drawing does not pay for
a rebuild; the whole index is rebuilt only when the model is replaced (new
document, project open), which is also what chooses the cell size from the data.

**Measured in Release** (16 x 2496 MHz), 4-vertex strings over a 1 km square:

| Entities | Snap scan | Snap indexed | Pick scan | Pick indexed | Box scan | Box indexed |
|---|---|---|---|---|---|---|
| 10 000 | 201 us | 2.3 us | 154 us | 0.97 us | 153 us | 0.042 us |
| 100 000 | 4404 us | 88 us | 3910 us | 12.6 us | 4442 us | 0.040 us |
| 250 000 | 11 277 us | 498 us | - | - | - | - |
| 500 000 | 24 252 us | 1886 us | - | - | - | - |

Box selection is now constant in drawing size. Snapping still grows with
DENSITY rather than with count, because intersection snapping is quadratic in
the candidates inside the aperture and a denser drawing puts more of them
there; that is inherent to the mode, not an index failure. The headline is that
a half-million-entity drawing went from 24 ms per mouse move - over the whole
16 ms frame budget before anything is drawn - to 1.9 ms.

Rebuilding costs 7.8 ms at 100 000 entities and 44.6 ms at 500 000, paid once
on open (about 5% of the 891 ms load as it was then; opening has since become
several times faster - section 24 - so the share is larger now).

The 2D viewport's repaint goes through the same narrowing, which matters more
than the click paths because it runs on every pan and zoom rather than on an
event: at 500 000 entities zoomed to 1% of the extent, collecting what to draw
fell from 22.3 ms to 0.090 ms (248x).

**A measured non-obvious result**, recorded because it is the kind of thing an
index is assumed not to do: asking the index for EVERYTHING is slower than
scanning. A query covering the whole drawing gathers every id, sorts them and
looks each one up again, against one ordered walk - 8.1 ms against 4.0 ms at
100 000 entities. A zoom-extents repaint is exactly that case. `forEachCandidate`
therefore picks: index below ~35% of the indexed area, scan above it, the
threshold taken from the measured crossover. Tests compare the two paths on
both sides of it.

**OUTSTANDING:** the rest of the structures the phase names, each for the
workload it suits - KD-tree for nearest-point queries over survey marks, BVH
for mesh intersection, octree for point clouds, quadtree for terrain tiles. The
terrain module already has its own uniform bucket grid inside `TinSurface`;
unifying the two is worth considering but has not been measured. The 3D scene
builder still walks every entity, which matters less because it rebuilds only
when the document changes rather than per frame.

---

