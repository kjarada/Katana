<!-- Katana plan, section 13 of 47. Index: ../PLAN.MD. Previous: 12-phase-07-project-storage.md. Next: 14-phase-09-professional-2d-cad.md -->

# 13. Phase 08 — Basic CAD Application

**STATUS: DELIVERED.** 27 unit tests plus 4 end-to-end CTest cases at delivery.

`katana_cad` holds everything an interactive session needs that is not a widget,
so all of it is tested without a display:

* `Document` - model, command stack, project, selection, current layer.
* `selection` - `isDrawn` vs `isSelectable`; `pickEntity` against drawn geometry
  (clicking inside a circle selects nothing); `pickInBox` with real window and
  crossing tests, not bounding-box overlap.
* `view_transform` - `zoomAt` keeps the point under the cursor fixed; scale
  clamped to [1e-7, 1e7]; `gridSpacing` on a 1-2-5 sequence.
* `snapping` - 8 modes with a documented resolution order.

`katana_qt` is the desktop shell and `katana_app` the headless `katana_cli`;
both drive the same objects.

---

