<!-- Katana plan, section 47 of 47. Index: ../PLAN.MD. Previous: 46-audit-defect-register.md. Next: none -->

# 47. The workspace: dockable panels and views, per-view layers

**STATUS: PARTIALLY DELIVERED.**

The user's request of 2026-09-23: make the Layers, Properties, Reference Data
and Command Line panels dockable, movable and minimisable, closable and
brought back from the menus; make the drawing views the same, and able to go
out of the main window onto another screen; give each view its own control of
which layers it shows; and improve the desktop experience as a whole. It was
taken ahead of section 20.3 because the owner asked for it directly. The
decisions and the alternatives rejected are in `docs/cad.md`, "The workspace".

| # | Slice | Status |
|---|---|---|
| 1 | Per-view layers in THE visibility rule: `cad::LayerOverrides`, `isDrawn`/`isSelectable` with no default view, carried by `SnapRequest`, `SelectionFilter` and `SceneOptions` | **DELIVERED** (tests in slice 2) |
| 2 | `cad::ViewSet` - the open views and their state, replacing the tiled cells; `cad::dockSplits` - the presets as dock arrangements; their tests | partly: code delivered, tests pending |
| 3 | Views as docks in a nested window (`ViewWorkspace`), replacing `ViewportContainer`: open, close, change kind, arrange, float onto any screen; showing a view never replaces one | partly: the workspace, without its chrome |
| 4 | Dock chrome for panels and views: a title bar with minimise, float, maximise and close; a minimised tray; a Window menu listing every panel, toolbar and view; the layout saved between sessions and never in a headless run; Reset Layout | pending |
| 5 | Per-view layer controls: a Layers button on each view's title bar | pending |
| 6 | The view widgets on their state: plan zoom and section kept across a change of kind, the 3D view following edits (audit QT-05), a section keeping its zoom on a resize, the plan view's minimum size | pending |
| 7 | The wider improvements from the UX audit of 2026-09-23 | pending |

**OUTSTANDING:** slices 2 (tests) to 7.
