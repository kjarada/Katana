# Road objects: what stands beside and crosses a road

The furniture and the structures of a road - safety barriers, guide posts,
signs, light poles, culverts, retaining and noise walls, fences and line
markings - placed along an alignment, along a line in the drawing or at
points, and made into closed 3D solids the views, the quantities and the IFC
export read. Horizontal BIM, as the owner asked on 2026-10-15 ("im only
interested in horizontal bim like roads etc, not intrested in buildings"):
nothing here is a building.

The code is `include/katana/cad/road/road_objects.hpp` (the kinds, the
definition, how each is made), `include/katana/cad/road/road_object_mesh.hpp`
(the solids), `include/katana/cad/road/road_object_verbs.hpp` (the `ROADOBJECT`
verbs) and `src/katana_cad/road/road_object_store.cpp` (the Document's
members); the window's side is `src/katana_qt/bim/`. The road templates and
corridors beside them are `road_design.md`'s; both are made into the one
`cad::bim::BimModel` (`include/katana/cad/bim/bim_object.hpp`), corridors first.

## A definition, made every time

An object is a DEFINITION: its kind, where it is placed, and the parameters a
person gave. The drawing keeps that, and nothing else: the solids are made
from it every time they are asked for (`cad::road::buildRoadObject`, and
`bim::placedObjects` for every object of the drawing), from the alignment, its
design profile and the drawing's surfaces as they are now. Move a PI and the
barrier along it moves; lower the profile and the sign comes down with it.

*Rejected: keeping the meshes.* A stored mesh disagrees with its alignment the
moment the alignment is edited, and a 10 km barrier is a few hundred
kilobytes of triangles in every save. The definition is a few hundred bytes,
and making it again costs well under a millisecond for an ordinary object;
the road model's cache (`bim::bimModel`, `road_design.md`) keeps the model it
made while what the objects are made from - their key, the alignments, the
surfaces they name - is unchanged: a line drawn, a template edit or a surface
no object names does not make them again.

What cannot be made is never drawn at a guessed level. `ROADOBJECT ADD`,
`SET` and `COPY` make the object before they commit it and refuse what does
not make, naming why; an object that stops making later - its alignment
shortened or deleted, its surface removed - is left out of the model and its
problem listed (`BimModel::problems`, `ROADOBJECT PROBLEMS`, `katana_road_objects`).

## The kinds

| Kind (`ROADOBJECT ADD`) | Takes its placement | IFC 4.3 | Quantities |
|---|---|---|---|
| `barrier` - W-beam, thrie-beam, F-shape, single slope, wire rope | along it | `IfcRailing` GUARDRAIL | Length, Height, Count (posts), GrossVolume (concrete) |
| `guidepost` | at points of it | `IfcSign` MARKER | Count, Height |
| `sign` - rectangle, diamond, circle, octagon, triangle; one post or two | at points of it | `IfcSign` PICTORAL | Count, Height, GrossArea (the plate) |
| `light` - pole, arm and lantern | at points of it | `IfcMember` POST, and `IfcLightFixture` POINTSOURCE for the lantern | Count (the pole's; the lantern says 1 of its own), Height |
| `culvert` - pipe or box, cells, headwalls | across it | `IfcPipeSegment` CULVERT; headwalls `IfcWall` RETAININGWALL | Length, Count (cells), GrossVolume; headwalls Width, GrossVolume |
| `wall` - retaining or noise | along it | `IfcWall` RETAININGWALL, or USERDEFINED with ObjectType NOISEBARRIER; a footing `IfcFooting` STRIP_FOOTING | Length, Height, GrossArea (the face), GrossVolume; footing Length, Width, Depth, GrossVolume |
| `fence` - posts and wires | along it | `IfcRailing` FENCE | Length, Height, Count (posts) |
| `marking` - edge, centre, lane, continuity, solid | along it | `IfcSurfaceFeature` LINEMARKING | Length (marked), GrossArea, Count (marks) |

**Totals** (an object's record, `QUANTITIES`' `total kind=`, the manager's
column: `cad::road::totalQuantities`) sum the volumes, a height, width or depth
the largest, and take a Length, a Count and a GrossArea from the object itself
and not from what stands under it or hangs from it - a wall's footing
(`IfcFooting`) and a light pole's luminaire (`IfcLightFixture`). Each of those
says its own Length or Count, as IFC reads them off the element, but they
measure the same run and the same pole again: a 50 m wall is 50 m long
whatever its footing, its concrete the stem's 25 m3 and the footing's 24
together, and three poles are three, not six.

`ROADOBJECT KINDS <kind>` lists every parameter of a kind with its unit, range
and default; the dialog's pages are built from the same table
(`cad::road::parametersOf`), so a parameter added there appears on every
surface at once.

### Where the defaults come from

Every default is a published or a typical value, named in the code beside it
(`src/katana_cad/road/road_objects.cpp`) and said by `ROADOBJECT KINDS
<kind>`'s `source=`:

- **Barriers.** The beam rails of AASHTO M 180: a W-beam 311 mm and a
  thrie-beam 508 mm edge to edge, both 83 mm deep, drawn as plain boxes of
  that size (the corrugation left out). Tops of rail 787 mm (31 in, the
  MASH-tested Midwest Guardrail System) and 864 mm; posts 150 x 100 mm
  (W6x9) at 1.905 m (6 ft 3 in). The F-shape (AASHTO Roadside Design Guide,
  FHWA): a 76 mm reveal, the face at 55 degrees from the horizontal to
  254 mm, then 84 degrees to the top at 813 mm, on a 610 mm base - its top
  width follows from those. The single slope: 1067 mm (42 in) on a 610 mm
  base with a 203 mm top, faces 10.8 degrees from the vertical. Wire rope:
  four 19 mm ropes 100 mm apart below a top rope at 0.80 m, posts every 3 m:
  typical of the proprietary systems, whose own drawings govern.
- **Guide posts.** As AS 1742.2 sets them out; 1.0 m above the ground and
  100 mm square is typical.
- **Signs.** The plate's lower edge 1.5 m above the ground: the rural minimum
  of the MUTCD (2009), section 2A.18 (5 ft; 7 ft, 2.1 m, where there are
  pedestrians). A 900 x 600 mm plate on a 75 mm post, 3 mm thick: typical.
- **Light poles.** A 10 m mounting height and a 1.5 m outreach, typical of
  road lighting to AS/NZS 1158; the arm 80 mm square, the lantern
  600 x 300 x 150 mm.
- **Culverts.** A 900 mm reinforced concrete pipe with a 100 mm wall,
  typical of the AS/NZS 4058 sizes, a box's wall 150 mm; 600 mm of cover, a
  typical minimum under a pavement (AS/NZS 3725 governs the loads);
  headwalls 250 mm thick, 300 mm beyond the barrels each side and above.
- **Walls.** A 2.0 m retaining wall with a 250 mm stem on a 1.2 x 0.4 m strip
  footing; a 4.0 m noise wall of 200 mm panels, with no footing: typical,
  the structural design governs.
- **Fences.** 1.2 m posts 100 mm square every 3 m with three wires - drawn
  20 mm thick so they show at the distance a road is looked at from.
- **Line marking.** AS 1742.2's longitudinal lines: an edge line 150 mm;
  dividing and lane lines 100 mm, broken 3 m line and 9 m gap; continuity
  lines 150 mm, 1 m and 3 m; 3 mm of thermoplastic.

The job's own standard drawings govern every one of these; each is a
parameter a person sets.

## Placing

- `ALIGNMENT <name> station=<s> [to=<s>] [offset=<o>] [spacing=<m>]` - a
  linear kind runs from `station` to `to`; a point kind stands at `station`,
  or every `spacing` from `station` to `to` (only at whole spacings: a run of
  guide posts gets no extra post at its end, where a barrier gets an end
  post); a culvert crosses at `station`. `offset` is left of the centreline
  positive, looking along increasing station.
- `ALONG #<id> ...` or `ALONG FROM <scope> [WHERE ...]` - one object along
  each line the ids or the shared scope (`docs/cad.md`, "Scope and filter")
  take: a line, a polyline (a closed one all the way round), a curve polyline
  or an arc, chorded within the drawing's tolerance. What the scope takes
  that is not a line is counted (`skipped=`, `skippedtypes=`), and a scope
  that takes nothing is reported, not refused. A line named twice
  (`ALONG #1 #1`) is refused: one object is made along each line.
- `AT x,y[,z] ...` or `AT FROM <scope>` - a point kind at each point; a
  linear kind runs through the points in order; a culvert from the first
  (its inlet) to the second (its outlet).

**A closed line has one object at each corner.** A closed polyline is stored
with its first vertex again at the end so that a fence goes all the way
round, and a point kind placed along it without `spacing=` stands once at
each corner - not a second time where it closes, on the first. With
`spacing=` the same: where the spacing divides the whole line, the last
distance is the start again and is not stood on (40 m round, every 20 m,
makes 2 poles, at 0 and 20). A vertex that repeats the one before it (a node
edited twice) is that corner again, not another, and a line drawn out and
back to its start is closed in this sense. A point kind along a line faces
along the segment it starts; between corners that differ, so never along
nothing.

**Keys that apply to nothing are refused, not echoed.** `spacing=` is for the
point kinds along an alignment or a line - a barrier's and a fence's posts
have `postspacing=`, and at points one object stands at each point;
`crossfall=` carries the design profile out to the offset, so it is for an
object that stands on the profile (an object on a surface, or on the heights
of a line, has none to carry). A run is at most 10,000 posts, signs, poles or
marks, and a broken line's dash at least a millimetre (the least a sweep
keeps): a spacing typed as 0.001 for 0.1 would otherwise ask for a hundred
times the memory, which a million posts already took (2.3 GB, 5.7 s) to
make. A longer run is placed in lengths.

### Heights

An object stands on the ground of its placement, raised by `dz=`:

- **profile** (the default along an alignment): the design profile's level at
  the station, carried out to the offset at `crossfall=` percent - the level
  at `|offset|` is the centreline's plus `|offset| x crossfall / 100`, so a
  negative crossfall falls away from the centreline;
- **surface** (the default once `surface=` names one): the named surface of
  the drawing under each point; a linear object's line is sampled every
  metre so it follows the ground;
- **line** (the default along lines and at points): the heights the line or
  the points carry (`entity::heightsOf`). A point with none is refused - a
  height of zero would be a guess - and the reply says to give `surface=`. An
  arc keeps a height at each end (two in `elevations`, or the one `elevation`
  both share) and rises evenly by its sweep between them, as section
  annotation reads it; each chord of it takes its fraction of the rise.

Along an alignment a linear object is sampled every metre between its key
stations and at each of them: a section swept along a 50 m radius curve so is
within 1^2 / (8 x 50) = 2.5 mm of the true offset curve.

A **culvert** is the exception to "stands on": its barrel is set by its
inverts. `invert=` and `invert2=` give them; without them each end's invert
is its ground less `cover=` less the barrel's height above its invert. Its
ends along an alignment are `length=` apart, centred on the station, square
to the alignment turned `skew=` degrees counter-clockwise, the inlet on the
right; on a straight their stations and offsets are exact, on a curve the end
is taken in the station's frame. An end's station matters to the profile
alone: on a surface a skewed end that falls before the alignment's start or
past its end stands on the surface under it, and on the profile it is refused
naming that station. Along a line or between points with
`heights=line`, the line's heights ARE the inverts, as a drainage line is
drawn at its invert.

## How an object is made

The solids (`include/katana/cad/road/road_object_mesh.hpp`) are closed and
outward-oriented - every edge used by two triangles, once each way
(`cad::road::isClosedMesh`) - so the IFC export writes each as a closed face
set and a volume is the divergence theorem over the triangles
(`geometry::meshVolume`, summed about the first vertex so a solid at map
coordinates keeps its digits). The winding is for the export and the volume,
not the display: the 3D view draws both faces of every triangle (it never
culls: `render::RenderOptions::backfaceCull` is off, because a surface is
looked at from below too, and the GPU pipelines set no cull mode). A solid
that comes out with no faces, or open, is refused by the verb that places it
and listed as a problem if it appears later.

- **A sweep**: a cross-section in (u across, left positive; v up) swept along
  the path. The section stays VERTICAL - a barrier stands plumb on a grade,
  it does not lean with it - and is mitred at a bend, so it keeps its width
  round a corner. Rails, concrete barriers, walls, footings, wires and line
  marks are sweeps.
  - **Past the mitre limit the joint is bevelled.** A mitre is cut at four
    times the half-width (SVG's and PostScript's miter limit), where a
    hairpin's would run to infinity, which is a turn of more than about 151
    degrees. Past it the joint is two rings, square to the leg that arrives
    and to the leg that leaves, joined side to side: each leg keeps its full
    width right up to the joint. *Rejected: shortening the mitre* (what the
    limit first did), which narrowed the section at the joint to 0.695 of its
    width at 160 degrees and let the leg taper to it - a wrong volume the
    whole length of a leg, not at the corner. The volume of a section
    symmetric about its line is still its area times the line's length, to
    the digit (the two lobes of the joint, one each side of the line, are
    equal and opposite); one that lies to a side differs from a mitre's by
    t² h (tan(θ/2) - sin θ / 2) at the joint and nowhere else, for a section
    reaching t to that side, h tall, at a turn of θ.
  - **A closed path is swept as a loop.** A path that returns to its first
    point, at the same level, has no ends: the last ring joins the first and
    the start corner is mitred like the others. Two square end faces there
    left a notch outside the corner and an overlap inside it, or - closed
    midway along a side - two coincident caps. A path that returns to its
    start at another level (a spiral's turn) has two ends, as it always had.
- **A hollow sweep**: the ring between two loops of the same corners - a pipe
  of 24 facets (a facet of a 900 mm pipe is 3.9 mm inside the circle at
  most), a box's barrel; joined as a sweep is.
- **An extrusion**: a polygon in a plane pushed along its normal - a sign's
  plate - and its special case, a vertical prism: posts, poles, guide posts,
  headwalls, lanterns.

Several closed shells may make one object (a barrier's posts and its rail;
the cells of a box culvert side by side, with no gap): the mesh is still
closed, and the IFC export judges closure on the mesh's own indices
(`docs/ifc.md`, "The body"), so shells that touch at a shared position stay one
closed face set. The triangulation of a section is ear clipping
(`cad::road::triangulatePolygon`), which keeps a corner with no turn in it, so
a cap always meets the sides that use that corner. *Rejected:
`geometry::triangulate`*, which drops such a corner: a cap would then no longer
meet its sides and the solid would not close. Extending it with a mode that
keeps them, and deleting this second ear clipper (which also decides a
straight corner by an exact zero where the shared one uses
`math::tolerance`'s angular tolerance), is open ("Not done").

**Which way.** A barrier's rail is on the face towards its traffic:
`side=left|right`, or by default towards the centreline along an alignment
and the right of a line. A sign faces back along its placement, at the
traffic coming up it (south at a point, where nothing says which way is
along), unless `facing=` gives the direction in degrees counter-clockwise
from east. A light pole's arm reaches towards the centreline, to the left of
a line, south at a point, unless `facing=` says.

**Ids.** An object's id is its kind's word and the set's next number
(`barrier-3`), never given twice: the set keeps its next number when every
object is deleted, and when an ADD is undone - a number the drawing has handed
out stays taken, undo and redo included, because the IFC GlobalId derives from
the id and a different object must not inherit one already handed over. (An
undo to the drawing before its first object leaves the `road_objects` key
there, with no objects and the next number.) Its solids are `object/<id>` for
one, `object/<id>/<n>` for the posts, signs or poles of a run, and
`object/<id>/barrel`,
`/headwall/inlet`, `/headwall/outlet`, `/footing`, `/<n>/pole`,
`/<n>/luminaire` for the parts - the same on every run and machine, since the
IFC export derives its GlobalIds from them.

**IFC.** Each solid says its IFC 4.3 class and predefined type as the
export expects them (the table above): a noise wall is `IfcWall` USERDEFINED
with the property ObjectType NOISEBARRIER, IFC 4.3 having no noise wall type;
a wall's footing is an `IfcFooting` of its own, as IFC keeps a foundation
apart from what it carries. `facility=` names the road an object is part of
(the IfcRoad it is written in); an object with none is the site's and is part
of no road part. A line marking of the site adheres to the surface course of
whichever road lies under it (`docs/ifc.md`, "Features").

## The verbs

```
ROADOBJECT KINDS [<kind>]
ROADOBJECT LIST [JSON]
ROADOBJECT INFO <id>
ROADOBJECT ADD <kind> [key=value ...] <placement> [key=value ...] [PREVIEW]
ROADOBJECT SET <id> key=value ... [PREVIEW]
ROADOBJECT COPY <id> [station=<s>] [dx=<m>] [dy=<m>] [key=value ...] [PREVIEW]
ROADOBJECT DELETE <id> [<id> ...] | ALL [PREVIEW]
ROADOBJECT QUANTITIES [<id> ...]
ROADOBJECT PROBLEMS
```

The keys every object takes: `name=`, `layer=` (default
`road/objects/<kind>`), `colour=` (#RRGGBB or a standard colour name),
`material=`, `facility=`, and the placement's `alignment=`, `station=`, `to=`,
`offset=`, `spacing=`, `heights=`, `surface=`, `crossfall=`, `dz=`; then the
kind's own. A key given empty goes back to its default: `offset=`,
`crossfall=` and `dz=` to 0, `to=`, `spacing=`, `heights=`, `surface=` and the
kind's own to not given; `name=`, `layer=`, `colour=`, `material=` and
`facility=` to the kind's. `station=` and `alignment=` have no default - where
along which alignment is the whole of the placement - and are refused empty.
After a scope's `WHERE` every `key=value` is a condition (`cad::isWhereKey`),
so an object's own keys stand before `ALONG` or `AT` there - as the dialog
writes them.

Every edit is ONE undo step through `Document::setRoadObjects`, which in the
same step makes the layer an object names when the drawing lacks it, and the
parents of that layer (`road`, `road/objects` for `road/objects/barrier`) -
one undo takes them all away, and takes none that were there already. An `ADD`
of several lines is all or nothing. `PREVIEW` replies what would be done and
does nothing. `COPY ... station=` moves an alignment placement's start and
keeps its length, unless the copy is given a `to=`, which is its end whichever
order the keys are typed in (and is refused when it is before the new station);
`dx=`/`dy=` move a placement along a line or at points.

**Ids, not a scope.** `LIST`, `QUANTITIES` and `DELETE` act on ids, or on all
(`DELETE ALL`; `QUANTITIES` with none): a road object is a definition kept by
id, not a drawn entity the shared scope (`ALONG FROM`'s grammar) can take, as
the corridor verbs act on names (`road_design.md`). A scope word after them is
refused by name - it used to be answered with every object by `LIST` and read
as an id that does not exist by the other two - and an id named twice is one
object (`QUANTITIES barrier-1 barrier-1` totals one barrier). Taking a scope
there - by layer, by a view's or an area's plan extent - is open ("Not done").

The replies are records, the keys in the form the options take:

```
added id=barrier-1 kind=barrier placement=alignment alignment=Main station=10 to=60 offset=3 heights=profile layer=road/objects/barrier objects=1 length=50 height=0.787 count=28
```

`LIST` leads with `roadobjects count= problems=` and gives an `object` record
each; `INFO` gives the object's record (a culvert's with its `inlet=` and
`outlet=`), a `parameter` record per key (`value=`, `given=yes|no`, `unit=`),
a `part` record per solid (`class=`, `type=`, `triangles=`, `closed=` and its
quantities) and its problems; `QUANTITIES` a `quantity` record per solid,
then a `total` per kind (see "Totals" above). A scope's record
(`cad::scopeRecord`) leads the reply of a line that took one. `INFO`, `LIST`
and `QUANTITIES` on a record they cannot read (a newer Katana's, or damaged)
say so - Unsupported, with the reason - and do not say an object is missing.

## The drawing's record

The definitions are versioned JSON under the project metadata key
`road_objects` - the sheets' pattern (`docs/plotting.md`): the storage layer
carries a key it does not read, so a project gains road objects with no
schema change, they are saved and opened with it, and a newer version is
refused with Unsupported rather than read by guesswork (and is not
overwritten). The Document parses it once per change and shares the parsed
set with its undo steps (`Document::roadObjects`).

**Surfaces and alignments they name.** A road object names the surface it
stands on and the alignment it follows, as a corridor does. `SURFACE RENAME`
carries both along in its one undo step (`corridors=` and `roadobjects=` in its
reply say how many followed), so the sign still stands on its ground and the
corridor still daylights on it. `SURFACE REMOVE` and `ALIGN DELETE` do not
refuse: an object whose surface or alignment is gone is left out of the model
and its problem listed (above), and putting the name back (or undoing)
restores it. *Rejected: a guard that refuses the removal*, as `ROADTEMPLATE
DELETE` refuses a template a corridor uses: a template is a library entry
that must stay for the corridor to be built at all, where a surface or an
alignment is a thing in the drawing that is removed on purpose, and the
problem list already says what depended on it.

```json
{
  "format": "katana-road-objects",
  "version": 1,
  "next": 4,
  "objects": [
    {
      "id": "barrier-1",
      "kind": "barrier",
      "placement": {"kind": "alignment", "alignment": "Main", "station": 10.0, "to": 60.0, "offset": 3.0},
      "parameters": {"type": "thriebeam"}
    }
  ]
}
```

**Only what was given is stored**; the rest are the kind's defaults when the
object is made. *Rejected: every default written in at `ADD`*, which would
leave a barrier whose `type=` is changed from W-beam to single slope with a
W-beam's height, and an object's numbers would not say which were chosen.
The cost: a default changed in a later version changes the objects that took
it - which is why each default names its source here.

**A line is copied, not linked.** `ALONG` copies the line's vertices and
heights into the definition and notes the entity it came from (`source=#12`).
*Rejected: following the entity*, so a barrier moves when its line is edited:
the lines an object is placed along are often working geometry, deleted or
reshaped afterwards, and a barrier that silently moved or vanished with them
would be worse than one that stays where it was placed. `ROADOBJECT SET` or a
new `ADD` moves it on purpose.

A change of the key is reported as `DocumentChange::RoadModel`, with the step,
its undo and its redo, by the road model's comparison of its metadata keys
after every command (the road task's road_document.cpp, `road_design.md`).
The store notified it from inside its command once; with the comparison it
went out twice, the first time before the step was on the history.

## In the window

The BIM menu (made beside Terrain, `road_design.md`) has a **Road
Objects** section from the road objects' workbench
(`src/katana_qt/bim/road_object_workbench.hpp`): Barrier, Guide Posts, Sign,
Light Poles, Culvert, Wall, Fence and Line Marking open one placing dialog
(`roadObjectDialog`) on the kind's page, and Road Objects... opens the
manager (`roadObjectManagerDialog`). The BIM toolbar has the ones placed
most: barrier, sign, light poles, line marking and the manager. The icons
are painted in `src/katana_qt/bim/road_object_icons.cpp` - each object as it
stands beside a road, what makes it that object in the accent - with a pen
of their own rather than the tool catalogue's, since the icon contact sheet
(`katana_make_icons`) compiles that file and has no catalogue.

**The placing dialog** (`src/katana_qt/bim/road_object_dialog.hpp`) writes
the `ROADOBJECT ADD` line its fields describe, shown as it will run; Add hands
it to the window's executor exactly as if typed, and Preview runs it with
`PREVIEW`. The placement is along an alignment (its stations), along the
lines the shared "Apply to" and "Only those that match" controls take
(`ALONG FROM`), at points typed, or at the points the controls take. Each
page's parameters come from the kind's table, each left empty at its
default; the preview beside them draws the object those parameters make, by
the same code the drawing is made by, on a straight level road - a linear
kind seen along its line, which is its cross-section; a post, a sign or a
pole from the front; a culvert across its barrel, its headwalls as outlines
round it (filled they would hide the barrel). The crossfall field is offered
only along an alignment standing on the profile, the one place it is read.

The pages, the placement and the heights are in a scroll area
(`roadObjectBody`) and the line, Preview, Add and Close and the reply stay
below it: laid out in one piece the dialog was some 1,500 px tall and Add was
off the screen of a 1366 x 768 laptop. (The window's screen fit,
`screen_fit.hpp`, scrolls any dialog that is still too tall for its screen,
buttons and all; this one asks for little.) The status says what the reply
said: "Nothing added: the scope took nothing to place on." for a scope that
takes nothing (the verb reports it and records no undo step), "Added 2 - one
undo step." for several lines, and the scope's own reason - a colour that does
not read - in place of a generic one. Each button has its own mnemonic.

**The manager** (`src/katana_qt/bim/road_object_manager.hpp`) lists the
objects with their placement, layer, quantities and first problem; shows the
chosen one's `INFO`; and Apply (`ROADOBJECT SET`), Copy, Delete and Zoom To
(`ZOOM WINDOW` round the chosen objects' solids, 10 m across at the least)
are each the line a person would type, run through the executor. The keys
field goes with Apply and with Copy - `station=`, `dx=` and `dy=` are what move
a copy, and no other control says them - and is cleared once its line is
accepted (a refused line keeps it, to be put right). A record the drawing
holds but cannot read is said in the count ("0 road objects, 1 problems - the
drawing's road objects cannot be read ..."), not shown as an empty set.

The 3D and plan views draw the objects through `bim::bimModel`, the road
model's code (`road_design.md`), each in its colour on its layer. Help >
Command Reference has a Road objects section: `HELP ROADOBJECT`'s text, read
as every family's is (`src/katana_qt/command_reference_dialog.cpp`).

## For an agent

`katana_mcp` runs every `ROADOBJECT` line through `katana_run_commands`, and
`katana_road_objects` returns the drawing's road objects as structured data:
each definition as the drawing keeps it, the solids made of it (id, IFC class
and predefined type, layer, material, triangles, closed, quantities) and the
problems (`cad::road::roadObjectsReport`; `docs/mcp.md`). It reads the
Document, so asking leaves no line in the history.

## Tests

- `tests/cad/road/test_road_object_mesh.cpp` - every solid closed, at the
  volume worked out by hand: a box, a mitred corner, a sheared hollow sweep,
  an inverted T's triangulation; a turn past the mitre limit keeping its full
  width (1 x 1 x 20) and just inside it staying one ring; a path doubling back;
  a hollow sweep round a hairpin; a closed square swept as a loop (80 m3,
  off-centre 36) and closed midway along a side, a triangle, and a path that
  returns at another level not a loop.
- `tests/cad/road/test_road_objects.cpp` - each kind at its hand-worked place
  and size: the sign at station 50 offset 4
  (`RoadObjects.ASignAtStation50Offset4StandsOnTheProfileFacingTheTraffic`),
  a barrier's length and posts, the F-shape's area from its published angles,
  the skewed culvert's ends
  (`RoadObjects.ACulvertSkewedThirtyDegreesHasItsEndsWhereTheSkewPutsThem`),
  the lane line's marks; the refusals and the problems; the JSON's round
  trip, a newer version refused; ids never given twice, undo included; totals
  (a wall's length once, a light's poles); point objects round a closed square
  and over a repeated vertex; a culvert on a surface with an end off the
  alignment; an arc's heights; the cap on a run and the dash under a
  millimetre; `spacing=` and `crossfall=` refused where nothing reads them.
- `tests/cad/road/test_road_object_verbs.cpp` - the verbs through the
  interpreter: one undo step with the layer and its parents, the RoadModel
  notification, a scope of lines all or nothing, preview, save and open; a
  wall's total, COPY's `to=` in either order, a repeated id, the scope words
  refused by LIST, QUANTITIES and DELETE, INFO on a record it cannot read, and
  `SURFACE RENAME` carrying a corridor and the objects with it.
- `tests/qt_widgets/bim/test_road_object_dialogs.cpp` - the line the dialog
  writes, the dialogs by object name, the menu and toolbar, the icons; the
  status for a scope that takes nothing and for several lines, the scope's own
  refusal, no two mnemonics alike, the crossfall offered only on the profile,
  the culvert preview's hole, the scroll area, the manager's unreadable record
  and Copy's keys.
- `tests/app/test_mcp_server.cpp` - `katana_road_objects`
  (`McpServer.TheRoadObjectsToolGivesEachDefinitionWhatWasMadeOfItAndItsProblems`).
- `src/katana_app/road_objects_cli.cmake` - `cli.roadobject_*` through
  `katana_cli`, a save and an open in a second process among them; the
  totals, a closed line's corners, the parent layers undone, ids after an
  undo, the refusals, and the renamed surface.

## Not done

- **Barrier ends.** No terminals, flares, transitions from one type to
  another, blockouts, or the W-beam's corrugation; a barrier's run stops
  square at its ends.
- **A culvert's ends are cut square** to its barrel, not parallel to the
  road as a skewed culvert's usually are; no wingwalls or aprons. On a curve
  a skewed end's level is taken in the station's frame.
- **Sign faces** are a flat colour: no legend or symbol is drawn on the
  plate; `code=` and `text=` are properties.
- **Line marking** is longitudinal lines only - no arrows, words or raised
  pavement markers - and says no `Host` (the pavement course it lies on is
  the corridor's, `road_design.md`).
- **No placing by clicking.** The dialog takes stations, the selection or a
  scope; a tool in the catalogue that places an object where a person clicks
  in the plan view is not written.
- **Scope and filter on the list, quantities and delete verbs.** They take ids
  (the project's rule is that every tool that reads drawing data acts on the
  shared scope and filter): a road object is a definition with a `layer=` and
  a plan extent, so LAYERS, AREA and VIEW mean something for it, and a
  resolver beside `cad::matchEntities` - not a second implementation of the
  grammar - would give the verbs and the manager the shared controls. A scope
  word is refused meanwhile.
- **A second ear clipper.** `cad::road::triangulatePolygon` beside
  `geometry::triangulate` (see "How an object is made" for why it was not
  called); the fix is a keep-collinear mode on the shared one, with the
  tolerance policy of `math::tolerance`.
- **No window screenshot test**: the dialogs are tested as widgets
  (`tests/qt_widgets/bim/`), not through the running window's `--dialog`.
- **Built and run**: written by a parallel task that could not build; the
  integration build is the first to compile and run any of it.
