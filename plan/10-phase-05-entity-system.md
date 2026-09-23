<!-- Katana plan, section 10 of 47. Index: ../PLAN.MD. Previous: 09-phase-04-geometry-algorithms.md. Next: 11-phase-06-command-system.md -->

# 10. Phase 05 — Entity System

**STATUS: DELIVERED.** `include/katana/entity`, 22 tests at delivery.

* `entity.hpp` - `Geometry` variant ordered to match `EntityType`; `PropertyMap`
  of typed values.
* `entity_geometry` - validate, boundingBox, distanceTo, transformed. A
  non-similarity transform is rejected rather than silently distorting an arc.
* `entity_database` - ids are monotonic and **never reused**, so a stale
  reference can never resolve to a different entity. `ChangeEvent` observer.
* `tables` - layer, style and property databases; layer "0" cannot be deleted.
* `serialization` - JSON, with nlohmann confined to a single translation unit.

---


## 5.2 Resolved appearance (delivered)

`katana::entity::resolveDisplay` (`include/katana/entity/display.hpp`) is the
ONE answer to "what colour is this entity, how thick is it, and what linetype
does it use". The chain is the DXF ByLayer model: entity colour overrides a
named style's, which overrides the layer's; line weight and linetype come from
the style when one is named, otherwise from the layer.

It replaced three different answers, two of which were visibly wrong:

* the 3D scene builder read the entity's own colour or a fixed default and
  ignored the layer entirely - and ByLayer is the DEFAULT for a new entity, so
  everything on a coloured layer drew grey in 3D while the 2D view showed it
  correctly;
* the 2D viewport read the entity's or the layer's and ignored the style;
* nothing at all read `Style::color`, `Style::lineWeight` or `Style::linetype`.
  The Style table was validated on write and persisted faithfully, and was
  inert - a style could not change how anything looked.

**Styles are created and edited** (this paragraph used to say no command
created one): `commands::createStyle / updateStyle / deleteStyle / renameStyle`
in `table_commands.cpp`, the `STYLE` verb, and Edit > Styles and Linetypes...,
all delivered with 20.2 slices 1 and 3.

## 5.4 Dimension styles (delivered)

`DimensionStyle`, `ArrowHead` and `DimensionStyleDatabase` in
`include/katana/entity/tables.hpp`, with `formatMeasurement` in
`src/katana_entity/dimension_text.cpp`. Lengths are MODEL units, unlike
`Layer::lineWeight` which is paper millimetres: a dimension is part of the
drawing, so its text must keep its size relative to the geometry it annotates.

The formatter is the part that has to be exactly right, because the number ends
up on a drawing somebody signs:

* **Order is scale, then round, then fix the decimals** (DIMLFAC, DIMRND,
  DIMDEC). Rounding before scaling rounds in the wrong units: 1.24 m rounded to
  0.05 is 1.25 m = 1250 mm, but scaled to millimetres FIRST it is 1240 and the
  same rounding does nothing, giving 1240 mm. A centimetre of difference from
  the order alone.
* **Ties go away from zero**, through `std::llround`, which C17 7.12.9.6-7
  specifies to round halfway cases away from zero regardless of the current
  rounding mode. This is not pedantry: measured on this toolchain,
  `printf("%.0f", 2.5)` gives 2 and `llround(2.5)` gives 3, and
  `printf("%.2f", 0.125)` gives 0.12 where the correct surveying answer is 0.13.
  `printf`, `std::to_chars` with a fixed precision and `QString::number` all
  round ties to EVEN, so none of them may be used here. A test also changes the
  rounding mode and requires the answer not to move.
* **Locale independent**: `std::to_chars` on the integer parts and hand-emitted
  digits for the fraction, so no global locale can turn 1.5 into "1,5" and make
  the drawing read as a different number.
* A text override wins VERBATIM - no prefix, no suffix, no rounding, no unit
  scale - because DXF does not apply DIMPOST to overridden text and a surveyor
  who typed a value means that value.
* Trailing-zero suppression never produces a bare ".", an empty label or "-0".

Units are a scale factor plus a suffix rather than a unit enum, because the
entity layer may only see core, math and geometry (`tools/check_layering.cmake`)
so `katana/geodesy/units.hpp` is unreachable - and duplicating it would be a
second definition of a foot.

`katana::cad::buildDimension` turns a dimension and its style into plain
geometry - extension lines, the dimension line, arrow strokes or fills, and the
placed text - and BOTH renderers draw from it, so the two views cannot disagree
about what a dimension looks like. It also returns the drawn EXTENT, which the
broad phase now culls against: `entity::boundingBox` for a Dimension covers only
the measured points and the dimension line, so culling against it dropped
dimensions whose label was still on screen.

Everything is in model units. The viewport previously drew a fixed 5-pixel tick
and a 12-pixel label, which looks right on screen and plots at whatever size the
paper happens to give it, and formatted the number with `QString::number`, which
is locale dependent.

`Layer::dimensionStyle` names a style, resolved through the same ByLayer shape
as colour: the layer's named style, else the document default. A layer naming a
style that has gone still draws, in the default - leaving a dimension off a plan
because its styling is missing would remove information from it.

Schema 5 stores the styles and the layer column. A project from before it gets
the default, which is exactly how it already behaved.

Reachable from the application: `DIMSTYLE LIST | NEW name | SET name field value
| DELETE name` with fields TEXT, GAP, EXTOFF, EXTBEYOND, ARROW, HEAD, SCALE,
DECIMALS, ROUND, PREFIX, SUFFIX and TRIM, plus `LAYER DIMSTYLE layer style`.
LIST shows what a ten-unit dimension would READ as, because that is the question
anyone setting a style is actually asking.

**OUTSTANDING:** no GUI panel - styles are set from the command line only. The
3D view draws a dimension's lines and arrows but not its TEXT, because a glyph
outline needs a font the 3D path does not have. Arrow fills are outlined rather
than filled there, for the same reason `DrawList` fills only triangles. The text
width used for centring and for the extent assumes 0.6 of the height per
character, which matches the stroke font the 2D viewport draws with and would
need revisiting for a real font.

## 5.3 Linetypes (delivered)

`Linetype`, `LinetypeElement` and `LinetypeDatabase` in
`include/katana/entity/tables.hpp`, with the DXF sign convention (AutoCAD DXF
Reference, LTYPE group code 49): a positive length is a dash, a negative one a
gap, and exactly zero a dot. `patternLength()` is the sum of the ABSOLUTE
lengths (group 40), computed rather than stored so it cannot disagree with the
elements. Patterns must begin with a dash or a dot, end with a gap and strictly
alternate; each rule has its own error.

**Dash lengths are MODEL lengths.** A 0.5 m dash is half a metre of ground at
every zoom, so a 10 km boundary carries a thousand times the dashes of a 10 m
fence. The opposite - a fixed pattern in pixels - is the easy implementation and
renders a perfectly convincing dashed line while being exactly backwards; two
property tests exist to catch it.

`katana::cad::forEachDash` walks a path in model space and emits its pen-down
spans, carrying the pattern across vertices so it runs continuously along a
polyline rather than restarting at each corner (DXF `$PLINEGEN`). It is
all-or-nothing: where the span budget would be exceeded it emits NOTHING and
the caller draws the path solid, because a truncated dashed line is a shorter
line with nothing to say so.

Below about one device pixel per element the pattern is drawn solid instead -
under that, every dash and gap falls inside one pixel and antialiasing turns the
line a uniformly paler colour, so the user sees the wrong COLOUR rather than a
pattern. That is a display decision and is deliberately NOT in `math::tolerance`.

Both renderers use it. The 2D viewport converts to a `QPen` dash array, which is
in units of PEN WIDTH rather than pixels - at the 1.5 px pen the viewport draws
with, forgetting the division makes every pattern half again too long. Every 3D
path goes through one emitter INCLUDING a plain `Segment2`, because a segment
adding its line directly would have left lines solid while polylines, arcs and
circles dashed.

Schema 4 stores definitions, with elements in their own table so their order is
the database's to keep. An older project naming a pattern that has no definition
keeps the name and draws solid, exactly as it already did; no empty definition is
invented for it, because that would shadow a real one later.

Reachable from the application: `LINETYPE LIST`, `LINETYPE NEW name dash gap
[...]` (lengths in model units, + dash, - gap, 0 dot), `LINETYPE DELETE name`,
and `LAYER LTYPE layer linetype` to attach one. Deleting a pattern a layer or
style still names is refused, and the error names the layer.

**OUTSTANDING:** no built-in patterns are shipped. `acad.lin` is in imperial
drawing units and the ISO set in millimetres, while a survey drawing here is in
metres, so any table shipped would be a conversion of numbers not to hand or an
invention - and a test would then pin the invention as though it were a standard.
Patterns are defined by the user until an importer reads real ones. `$LTSCALE`
and a per-entity scale are parameterised throughout but pinned at 1. (The GUI
editor this used to say was missing is Edit > Styles and Linetypes..., 20.2
slice 3.)

## 5.1 Nested layers (delivered)

Layer names are `/`-separated paths: `design/surface/tin1`, `asbuilt/road/kerb`.
The tree is DERIVED from the names, not stored - see
`include/katana/entity/layer_path.hpp` for the reasoning. In short: an entity
references its layer by full path, and that string is what the database, a DXF
export and every undo record carry. If the tree were primary, renaming `design`
would have to rewrite the node, every descendant and every entity, and any of
those failing would leave entities pointing at a layer that no longer exists.
Keeping the path as the identity means the tree is a view of the names, with one
source of truth - the one already persisted. `std::map` order over full paths
puts every parent before its children and makes a layer's descendants one
contiguous range (`name/` to `name0`), so a subtree is a range scan.
CORRECTED 2026-09-23: this said the order was a pre-order walk, and
`hasChildren` believed it - it tested only the key after the layer. But space,
`-` and `.` sort below `/`, so "design 2" comes between "design" and
"design/surface"; with both present "design" had no children and `remove`
orphaned the branch. Found while fixing the inheritance below; regression
tested.

* Adding `design/surface/tin1` creates `design` and `design/surface` as real
  layers, each with its own colour, visibility and lock. Implicit nodes that
  exist only in the tree widget would give the user a row they can see and
  cannot switch off.
* **Visibility and lock inherit down the tree; colour and linetype do not.**
  ByLayer already resolves colour, and a second inheritance rule on top would
  make the resolved colour impossible to predict. CORRECTED 2026-09-23 (audit
  MOD-01/02/03, REN-03/04): until then only the layer panel honoured the
  inheritance. The viewport, picking, box selection, snapping, the plot, the 3D
  scene, sections and Surface From Drawing looked at the entity's own layer, and
  the commands' lock check at its own lock, so a line on
  `design/surface/tin1` stayed drawn under a hidden `design` and SELECT ALL,
  ERASE erased it under a locked one. Now `LayerDatabase::resolve` answers
  shown/locked with inheritance in one lookup, from state the table keeps
  current on every change (only the changed subtree is recomputed), and
  `cad::isDrawn` / `isSelectable` and `requireUnlockedLayer` are built on it;
  a refusal names the ancestor holding the lock.
* Descendant tests compare whole segments, so `designs/x` is not under
  `design`. A plain string-prefix test would hide or lock the wrong layers.
* `remove` refuses a layer that still has children; `removeSubtree` takes the
  branch and returns it deepest-first so undo replays it parents-first.
  `renameSubtree` moves a branch, validates every target BEFORE moving anything
  so a collision cannot leave the tree half renamed, and returns the (old, new)
  mapping so the `RenameLayer` command can move the entities in the same undo
  step.
* `.` and `..` are refused as levels: layer names reach file names whenever
  something is exported per layer, and `..` there is a path traversal.
* Shown as a `QTreeWidget` in the left dock, with new/new-child/rename/delete,
  per-node visibility and lock ticks, a colour swatch, entity counts, the
  current layer in bold, and layers switched off by an ancestor greyed.

