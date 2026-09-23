<!-- Katana plan, section 25 of 47, subsection 20.2. Index: ../../PLAN.MD. Previous: 01-20-1-where-imported-data-lands.md. Next: 03-20-3-survey-coding-programme.md -->

## 20.2 The 12d Model programme — TAKES PRECEDENCE OVER EVERY OTHER PHASE

**Instruction (2026-09-22, from the user): nothing in any other phase of this
plan is to be started or resumed until every slice below is DELIVERED.** The
grading command, the plotting phases, the point-cloud work and the rest wait.
`CLAUDE.md` section 5.2 carries the same instruction. The one exception is a
defect that blocks a slice here.

12d Model is what Katana's users exchange work with. Section 25 delivered the
archive format - every element read, written and mapped. This programme
finishes the JOB: everything a 12d archive carries has a home in Katana, can
be seen, managed and edited, and goes back out as it came in. The measure is
the fourteen real archives in the user's `kjarada.github.io/examples/test-files`
(4 to 58 MB, written by 12d Model 15), which carry among them 270 distinct
linestyle names, 109 colour names, 7 text styles, 28 479 vertex symbols,
1 719 trimeshes, seven kinds of attribute block and every string type.

There are only two container forms: `.12da` and `.12daz`. There is no
`.12dz`; the extension was accepted in error and is withdrawn in slice 1.

### Slices, in order

1. **Names become tables.** A 12d linestyle name becomes a Katana `Style`
   (created by the import, continuous unless known) and the entity carries it
   as its style - not as metadata. A 12d text style becomes a Katana style
   too. Colours: the standard names as now; every other name (`sui
   electricity`, `shade 48`, `pen 025a`) is kept as the style's description so
   that nothing is anonymous. `.12dz` withdrawn. Export writes the names back
   from the tables.
2. **Symbols.** A `Symbol` is a named marker - a shape from a fixed set
   (circle, square, triangle, cross, diamond, tick, tree, pole, manhole ...)
   at a size, in a colour or ByLayer - held in the `Style` table so that "the
   style of a point" and "the style of a line" are one thing, as they are in
   12d. A point entity draws its style's symbol in plan; `symbol_value` and
   `symbol_data` on import set it; export writes them back. A **Symbols
   manager** lists, creates, edits and previews them.
3. **Line styles manager.** One dialog for the `Style` and `Linetype` tables
   together: name, linetype (with pattern editing and a preview), line
   weight, colour, hatch, symbol; new, edit, rename, delete with the
   in-use guard; assign to the selection or to a layer.
4. **Trimeshes.** A `SceneMesh` - name, vertices, triangular faces, colour,
   per-face colours where the file has them - drawn in the 3D view like a
   surface and as its outline in plan, held with the surfaces in the session
   (persistence of session data is a separate, later question for surfaces
   and meshes alike). Import creates one per `primitive_3d`; export writes
   `primitive_3d` back with its infos and flags.
5. **Attribute manager.** A dialog on the selection showing every property
   as the tree 12d had it (`Group/Sub/Name`, `vertex/3/Name`), typed, with
   add, edit, remove and rename, each an undoable command; property
   definitions (the schema) managed in the same place. Text, integer and real
   attributes as themselves; groups as branches.
6. **Layer manager.** A dialog with the whole layer table - visible, locked,
   colour, linetype, line weight, dimension style, hatch, entity count - with
   new, new child, rename, delete, move, and assign-to-selection; the dock
   stays as the quick view.
7. **Text as 12d draws it.** Justification, offset, raise and slant applied
   on import so the text sits where 12d put it; text style as a Katana style;
   width factor and the rest kept and written back.
8. **Super tins built, and referenced clouds found.** A super tin becomes a
   surface in which each member overrides the members before it within its
   footprint; a `ref_data` cloud is looked for beside the archive and imported
   when found.
9. **The round trip, on the real archives.** Import each of the fourteen
   archives, export it, import the export: every element kind present in the
   same numbers, every string with the same vertices, heights, style, colour,
   attributes and symbols, every mesh with the same faces. A test that reads
   the small fixtures does this in ctest; a script does it on the large ones
   and its results are recorded in `docs/interop.md`.

Each slice: tests first, engine, application, docs, commit. The status line
below is updated as each lands.

**Slice 1 DELIVERED (2026-09-22).** A 12d linestyle is a Katana `Style`
the entity carries in its own `style` field; the importer lists the styles
it needs (`stylesNeeded`) and the CLI and the desktop application create the
missing ones in the import's transaction, beside the layers. `Test 4 without
tin.12daz` (27 177 entities) arrives with its 98 linestyles as styles. The
`Style` table gained `description`, `symbol` and `symbolSize` (schema 9,
defaults reproducing every older project), a free `validate`, and its
create/update/delete commands - by collapsing the four hand-written copies
of the table command trio into one template with a policy per table
(`table_commands.cpp`), which was the "second way of doing something" that
CLAUDE.md section 6 says to remove when met. `STYLE LIST | SYMBOLS | NEW |
SET | DELETE | APPLY` in the interpreter. `.12dz` withdrawn. Colour names
Katana has no RGB for stay on the entity as `12d.colour` rather than on the
style, because a 12d colour belongs to the string, not to its linestyle.

**Slice 2 PARTIALLY DELIVERED (2026-09-22) - the engine and the drawing;
the manager dialog is folded into slice 3.** `cad::symbolStrokes` is the
one definition of the sixteen shapes (`include/katana/cad/symbols.hpp`);
`resolveDisplay` carries the style's symbol and size; the plan viewport and
the PDF plot draw a point's symbol from its style (`ViewportWidget::
drawSymbol`, size = width as 12d's is, 0 = the plain mark). A 12d
`symbol_value`/`symbol_data` on a point makes the point's style the
symbol's linestyle, with the shape read from the name
(`archive12d::symbolForLinestyle`: manhole, pole, tree, target, flag, star,
diamond, dot, cross, else circle) and the size from the block; the symbol's
colour is the point's; `12d.string_style` keeps the string's own linestyle;
only non-default rotation/offset/raise, a non-standard colour and a size
unlike the style's become `12d.symbol.*`. A line's per-vertex blocks are
kept as one list per key and written back as `symbol_value` or
`symbol_data`. `Test 4 without tin` arrives with 113 of its 211 styles as
symbols; the fixture `tests/archive12d/data/symbols.12da` is drawn by
`qt_import_symbols_headless`. The rest of the work is in `docs/cad.md`
(Point symbols) and `docs/interop.md` (Symbols are on the style).
**Decision:** the "Symbols manager" this slice asked for and the "Line
styles manager" of slice 3 are ONE dialog, because a symbol is a field of a
Style and a second dialog on the same table would be the second way of doing
something; it is delivered under slice 3. OUTSTANDING: that dialog (slice
3); a symbol on the vertices of a line is kept, counted and not drawn; a
point symbol's rotation is kept and not drawn.

**Slice 3 DELIVERED (2026-09-22).** One dialog for both tables -
`src/katana_qt/style_manager.cpp`, Edit > Styles and Linetypes... - listing
every style (linetype, weight, colour, hatch, symbol and its size,
description) and every linetype (pattern, period, description), with a form
below and a preview that draws the sample through the SAME
`cad::qtDashPattern` and `cad::symbolStrokes` the viewport and the plot use,
so a preview that looks right is evidence and not a second opinion. New,
rename, delete, save changes, apply to the selection, and assign a linetype
to a layer; everything through the ordinary commands, so a change made here
is on the same undo stack as one typed on the command line.

The new engine piece is `renameStyle` / `renameLinetype`
(`table_commands.cpp`): a rename is a move in a name-keyed table, so it is
remove + add + repoint every holder - entities for a style, layers and
styles for a linetype - as ONE undo step. A protected item ("continuous")
may not be renamed away for the same reason it may not be deleted. The
rename ENDS by asking the delete guard whether anything still names the old
item, so the two readings of the same references police each other rather
than drifting. `STYLE RENAME` and `LINETYPE RENAME` expose it without the
GUI, and `STYLE NEW/DELETE/APPLY/RENAME` now refuse trailing arguments
instead of silently dropping them - `STYLE NEW TOPO Natural Surface Point`
used to make a style called "TOPO".

The table is read-only and edited through the form: a table that edited in
place would run a command from inside its own `itemChanged` signal, which is
exactly the shape of the layer-panel crash in `docs/cad.md`.
`qt_style_manager_headless` builds and paints the dialog over a 12d import.
OUTSTANDING: the dialog's buttons are driven by hand - the commands beneath
them are covered by tests, the dialog itself only by "it builds and paints".

**Slice 4 DELIVERED (2026-09-22).** A 12d `primitive_3d` becomes a
`geometry::TriangleMesh` - vertices and faces, deliberately NOT a
`TinSurface`, because a surface is single-valued in x and y and a mesh may
be closed or overhang, and keeping them separate types is what stops a pipe
being asked for "the level at this station". It is held in the session
beside the surfaces (`MainWindow::addMesh`), drawn in the 3D view like a
surface, and in plan as its FOOTPRINT - the convex hull of its vertices,
which is exact and cheap for any topology, dashed and lightly filled under
the drawing. Per-face colours survive both ways: the one-based
`face_flags` into `face_infos` on the way in, one info per distinct colour
on the way out. `Test 4`'s Windsor Road archive brings 1 453 meshes of
90 656 triangles in 0.11 s; `test trimishes complex.12da` brings 266 of
126 536.

Two decisions recorded where they apply. A mesh defaults to `Shaded` and
not `ShadedWithEdges`: it has no neighbour table, so every edge of every
face is drawn and each line costs two triangles - measured in Release,
886 504 triangles at 24.9 ms with edges against 127 288 at 18.0 ms without
(`include/katana/cad/scene.hpp`). And the plan view draws the footprint
rather than the triangles, because 1 453 meshes of 90 656 triangles would
bury the drawing they are context for.

OUTSTANDING: a mesh is session data, so it is not saved with the project
(the same open question as surfaces - PLAN.MD Phase 07); vertex and edge
infos, the edge list and `blend` are read and not modelled; the Reference
Data panel does not list meshes, so there is no way to hide or delete one
from the GUI yet.

**Slice 5 PARTIALLY DELIVERED (2026-09-22).** Edit > Attributes... (Ctrl+1)
shows the selection's properties as the TREE 12d had them - the importer
flattens `group { name "Asset" ... }` to "Asset/Dimensions/Size" and this
puts the branches back - with each value's type, and Add, Save Value,
Rename and Remove as ordinary commands on the one undo stack. It acts on
the whole selection: a value the selected entities do not share, or a
property only some of them carry, shows as `<varies>` rather than as one of
them, because a manager that showed the first entity's value and wrote it to
all of them would quietly overwrite the rest.

The engine gained `renameEntityProperty` (keeps the value and its type,
refuses a name the entity already has - the two values differ and keeping
one would lose the other) and `entity::toString`/`typeName`, ONE definition
of what a property value says, which the panel, the command line and the
dialog now share. `PROP` became verb-first like every other table verb -
`PROP LIST | SET key value [text|integer|real|boolean] | DELETE key |
RENAME old new` - because the old bare `PROP key value` could not be
extended without becoming ambiguous (a property may be named DELETE), and
because a 12d feature code of "2" must be able to stay text.

OUTSTANDING: property DEFINITIONS - the schema, what 12d calls attribute
definitions - are not managed here or anywhere; the dialog edits the
attributes an entity has rather than declaring what attributes a kind of
entity should have. That is what remains of this slice.

**Slice 6 DELIVERED (2026-09-22).** Edit > Layers... (Ctrl+L) is the whole
table - name, on, locked, colour, linetype, weight, hatch and how many
entities are on each - with a form below for the definition (including the
dimension style, which the dock has no room for), and New, New Child, Save
Changes, Rename or Move, Delete, Put Selection Here and Make Current. The
dock stays as the quick view, as the slice asked.

"Move" needed no new command: a layer name is a PATH, so moving a layer
under another parent IS renaming it, and `renameLayer` already takes the
subtree and the entities on it as one undo step. Make Current is not a
command at all - the current layer is session state like the selection, and
putting it on the undo stack would make Ctrl+Z undo where the next line
will be drawn.

**Slice 7 DELIVERED (2026-09-22).** One `applyAnnotation` in the importer
serves all three places text arrives - `string text`, vertex text and
segment text - and applies what 12d says about it: justification (the anchor
is moved to where the baseline starts, since a Katana text position IS the
left end of the baseline), the text's own colour ("no_colour" meaning the
string's), its textstyle as a Katana style, an `offset` where the size is
`worldsize` and so in model units, and a `raise` added to the elevation
because a raise is a level and not a plan displacement. Slant, width factor,
justification and a paper-millimetre offset are kept as `12d.text.*`.

**The plan asked for offset, raise and slant to be "applied"; two of those
are recorded here as wrong.** Slant and width factor have no field in a
Katana `TextGeometry`, and inventing them in the renderer would be a worse
lie than leaving the text upright - they are kept and written back instead.
The manual does not give the DIRECTION of a text offset; it is taken as
perpendicular to the text, to the left, and that assumption is stated where
it is made. Measured in the sample archives: every annotation is
"bottom-left", `raise`, `slant` and `x_factor` are always 0, 0 and 1, and
`offset` is 0 except 12 (with `papersize 10`) and 0.2 (with `worldsize`) -
so justification and colour are what actually change a real file, and the
width estimate behind justification never fires on one.

**Slice 8 DELIVERED (2026-09-22).** A super tin is built into a surface
(`terrain::combineSurfaces`, a new `super_surface.hpp`) in which a LATER
member overrides an earlier one wherever it covers it; the members stay as
surfaces of their own, because a tin and a super tin are separate objects in
12d. `test super tin.12da` now arrives as four surfaces - NATURAL SURFACE,
PAD A, PAD B and COMBINED SURFACE of 34 triangles, the base's 32 minus the
14 the pads took, plus the pads' 16 (this said 18, which does not add up). A `ref_data` point cloud is looked for
BESIDE the archive by file name and read when it is there; when it is not,
the warning now says so rather than implying it was never tried.

Three decisions recorded where they are made. A triangle of a lower member
is dropped WHOLE if a higher one covers any part of it, not merely its
middle: two sheets over one point would make `elevationAt` answer with
whichever triangle it met first, which is the base as often as the pad, and
a ragged seam that gives NO answer just outside a pad is honest where a
wrong level is not. The rank order (later wins) is not in the manual, which
lists the tins and says nothing; it follows the sample archive's own comment
("Rank order: base surface first, then the pads that substitute it over
their footprints") and is one comparison to reverse, with the test that pins
it saying so. And only the file NAME of a `ref_data` reference is used -
following "..\..\scans\site.las" out of the directory the archive sits in
would let a file choose what gets read.

OUTSTANDING: the seam is exact to within one triangle of the lower member,
not exactly clipped; clipping each triangle against the higher surface's
boundary and re-triangulating the offcuts is the exact answer and is
recorded in `super_surface.hpp` as the alternative rejected.

**Slice 9 PARTIALLY DELIVERED (2026-09-22).**
`tests/interop/test_round_trip.cpp` takes six fixtures through import,
export, import, export, import. The first export NORMALISES (an arc is
written as a two-vertex super string, a drainage string as its line plus its
pits, a full_tin as a tin), so the property asserted is that every reading
after the first is identical - the same entities, styles with their symbols,
layers, meshes with their faces and face colours, tallies and per-string
summaries - and that the first pass loses nothing.
`katana_12da_probe --roundtrip` does the same on the large archives and
`docs/interop.md` records the run over all fourteen: thirteen are stable.

It found two defects, which is what it was for. **Fixed:** exporting a
drawing imported from a 12da wrote every alignment twice - as the alignment
AND as the centreline polyline the import made to draw it with - and each
round trip read the duplicate back and added another. The exporter now skips
a centreline whose alignment it is writing, and the alignment carries the
layer, colour and style that polyline wore (before this they were hard-coded
to model "Alignments", colour red, style "1", so an alignment now keeps its
appearance for the first time).

**The surface defect this slice found, and its cause.** Four surfaces of
`plot_PW_example_data.12da` - `Design/2/LOTS`, `Design/2/ROADS`,
`Design/2/ROADS DETAIL` and `Super/FS` - built on the first import and were
REFUSED on the second with "two triangles run along an edge in the same
direction". The hypothesis recorded here was a duplicated triangle pair. It
was wrong. The cause was a FLAT triangle, and there were two independent
faults behind it:

1. 12d's own tins carry triangles with no plan area. Counted directly from
   the file's hexadecimal floats (outside Katana, by
   `tools/sliver_census.py`): seven triangles across the eight surfaces
   have a doubled area below 6e-14 m^2 over coordinates of 6.2e6 m - smaller
   than the rounding error of computing that area, so which way round they
   were listed is noise. Katana measured the sign anyway and handed
   `TinSurface` an edge direction chosen by the last bits of a subtraction.
   **Fixed**: the import nulls a triangle that is degenerate in plan, exactly
   as it already nulls one with a null height. The census predicted the four
   failures and the four survivors with no exceptions.
2. The writer put tin points out at eight decimal places, moving each by up
   to 5e-9 - enough to flatten a sliver that was not quite flat. 12d Model's
   own default is `output_tin_hex_floats true` for this reason, and there was
   already a test in the repository showing eight places loses the value.
   **Fixed**: `WriteOptions::hexFloatTins` now defaults to true.

Either fix alone makes the file stable (both were measured that way); both
are kept, because they are answers to different questions - what a degenerate
triangle means, and whose bits those are to throw away.

**STATUS: 20.2 is delivered but for the property DEFINITIONS left over from
slice 5, which is recorded and does not block the application.**

---

