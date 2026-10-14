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
the road model's cache (`bim::bimModel`, `road_design.md`) makes it once
per change of what it is made from.

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
| `light` - pole, arm and lantern | at points of it | `IfcMember` POST, and `IfcLightFixture` POINTSOURCE for the lantern | Count, Height |
| `culvert` - pipe or box, cells, headwalls | across it | `IfcPipeSegment` CULVERT; headwalls `IfcWall` RETAININGWALL | Length, Count (cells), GrossVolume; headwalls Width, GrossVolume |
| `wall` - retaining or noise | along it | `IfcWall` RETAININGWALL, or USERDEFINED with ObjectType NOISEBARRIER; a footing `IfcFooting` STRIP_FOOTING | Length, Height, GrossArea (the face), GrossVolume; footing Width, Depth, GrossVolume |
| `fence` - posts and wires | along it | `IfcRailing` FENCE | Length, Height, Count (posts) |
| `marking` - edge, centre, lane, continuity, solid | along it | `IfcSurfaceFeature` LINEMARKING | Length (marked), GrossArea, Count (marks) |

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
  that takes nothing is reported, not refused.
- `AT x,y[,z] ...` or `AT FROM <scope>` - a point kind at each point; a
  linear kind runs through the points in order; a culvert from the first
  (its inlet) to the second (its outlet).

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
  height of zero would be a guess - and the reply says to give `surface=`.

Along an alignment a linear object is sampled every metre between its key
stations and at each of them: a section swept along a 50 m radius curve so is
within 1^2 / (8 x 50) = 2.5 mm of the true offset curve.

A **culvert** is the exception to "stands on": its barrel is set by its
inverts. `invert=` and `invert2=` give them; without them each end's invert
is its ground less `cover=` less the barrel's height above its invert. Its
ends along an alignment are `length=` apart, centred on the station, square
to the alignment turned `skew=` degrees counter-clockwise, the inlet on the
right; on a straight their stations and offsets are exact, on a curve the end
is taken in the station's frame. Along a line or between points with
`heights=line`, the line's heights ARE the inverts, as a drainage line is
drawn at its invert.

## How an object is made

The solids (`include/katana/cad/road/road_object_mesh.hpp`) are closed and
outward-oriented - every edge used by two triangles, once each way
(`cad::road::isClosedMesh`) - so the 3D view culls back faces, the IFC export
writes each as a closed face set, and a volume is the divergence theorem over
the triangles (`cad::road::meshVolume`, summed about the first vertex so a
solid at map coordinates keeps its digits).

- **A sweep**: a cross-section in (u across, left positive; v up) swept along
  the path. The section stays VERTICAL - a barrier stands plumb on a grade,
  it does not lean with it - and is mitred at a bend, so it keeps its width
  round a corner; a mitre is cut at four times the half-width (SVG's and
  PostScript's miter limit), where a hairpin's would run to infinity. Rails,
  concrete barriers, walls, footings, wires and line marks are sweeps.
- **A hollow sweep**: the ring between two loops of the same corners - a pipe
  of 24 facets (a facet of a 900 mm pipe is 3.9 mm inside the circle at
  most), a box's barrel.
- **An extrusion**: a polygon in a plane pushed along its normal - a sign's
  plate - and its special case, a vertical prism: posts, poles, guide posts,
  headwalls, lanterns.

Several closed shells may make one object (a barrier's posts and its rail):
the mesh is still closed. The triangulation of a section is ear clipping,
which keeps a corner with no turn in it (so a cap always meets its sides).

**Which way.** A barrier's rail is on the face towards its traffic:
`side=left|right`, or by default towards the centreline along an alignment
and the right of a line. A sign faces back along its placement, at the
traffic coming up it (south at a point, where nothing says which way is
along), unless `facing=` gives the direction in degrees counter-clockwise
from east. A light pole's arm reaches towards the centreline, to the left of
a line, south at a point, unless `facing=` says.

**Ids.** An object's id is its kind's word and the set's next number
(`barrier-3`), never given twice: the set keeps its next number when every
object is deleted. Its solids are `object/<id>` for one, `object/<id>/<n>`
for the posts, signs or poles of a run, and `object/<id>/barrel`,
`/headwall/inlet`, `/headwall/outlet`, `/footing`, `/<n>/pole`,
`/<n>/luminaire` for the parts - the same on every run and machine, since the
IFC export derives its GlobalIds from them.

**IFC.** Each solid says its IFC 4.3 class and predefined type as the
export expects them (the table above): a noise wall is `IfcWall` USERDEFINED
with the property ObjectType NOISEBARRIER, IFC 4.3 having no noise wall type;
a wall's footing is an `IfcFooting` of its own, as IFC keeps a foundation
apart from what it carries. `facility=` names the road an object is part of
(the IfcRoad it is written in); an object with none is the site's and is part
of no road part.

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
kind's own. A key given empty goes back to its default. After a scope's
`WHERE` every `key=value` is a condition (`cad::isWhereKey`), so an object's
own keys stand before `ALONG` or `AT` there - as the dialog writes them.

Every edit is ONE undo step through `Document::setRoadObjects`, which in the
same step makes the layer an object names when the drawing lacks it - one
undo takes both away. An `ADD` of several lines is all or nothing. `PREVIEW`
replies what would be done and does nothing. `COPY ... station=` moves an
alignment placement's start and keeps its length; `dx=`/`dy=` move a placement
along a line or at points.

The replies are records, the keys in the form the options take:

```
added id=barrier-1 kind=barrier placement=alignment alignment=Main station=10 to=60 offset=3 heights=profile layer=road/objects/barrier objects=1 length=50 height=0.787 count=28
```

`LIST` leads with `roadobjects count= problems=` and gives an `object` record
each; `INFO` gives the object's record (a culvert's with its `inlet=` and
`outlet=`), a `parameter` record per key (`value=`, `given=yes|no`, `unit=`),
a `part` record per solid (`class=`, `type=`, `triangles=`, `closed=` and its
quantities) and its problems; `QUANTITIES` a `quantity` record per solid,
then a `total` per kind (lengths, areas, volumes and counts summed; a height,
width or depth the largest). A scope's record (`cad::scopeRecord`) leads the
reply of a line that took one.

## The drawing's record

The definitions are versioned JSON under the project metadata key
`road_objects` - the sheets' pattern (`docs/plotting.md`): the storage layer
carries a key it does not read, so a project gains road objects with no
schema change, they are saved and opened with it, and a newer version is
refused with Unsupported rather than read by guesswork (and is not
overwritten). The Document parses it once per change and shares the parsed
set with its undo steps (`Document::roadObjects`).

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
pole from the front; a culvert across its barrel.

**The manager** (`src/katana_qt/bim/road_object_manager.hpp`) lists the
objects with their placement, layer, quantities and first problem; shows the
chosen one's `INFO`; and Apply (`ROADOBJECT SET`), Copy, Delete and Zoom To
(`ZOOM WINDOW` round the chosen objects' solids, 10 m across at the least)
are each the line a person would type, run through the executor.

The 3D and plan views draw the objects through `bim::bimModel`, the road
model's code (`road_design.md`), each in its colour on its layer.

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
  an inverted T's triangulation.
- `tests/cad/road/test_road_objects.cpp` - each kind at its hand-worked place
  and size: the sign at station 50 offset 4
  (`RoadObjects.ASignAtStation50Offset4StandsOnTheProfileFacingTheTraffic`),
  a barrier's length and posts, the F-shape's area from its published angles,
  the skewed culvert's ends
  (`RoadObjects.ACulvertSkewedThirtyDegreesHasItsEndsWhereTheSkewPutsThem`),
  the lane line's marks; the refusals and the problems; the JSON's round
  trip, a newer version refused; ids never given twice.
- `tests/cad/road/test_road_object_verbs.cpp` - the verbs through the
  interpreter: one undo step with the layer, the RoadModel notification, a
  scope of lines all or nothing, preview, save and open.
- `tests/qt_widgets/bim/test_road_object_dialogs.cpp` - the line the dialog
  writes, the dialogs by object name, the menu and toolbar, the icons.
- `tests/app/test_mcp_server.cpp` - `katana_road_objects`
  (`McpServer.TheRoadObjectsToolGivesEachDefinitionWhatWasMadeOfItAndItsProblems`).
- `src/katana_app/road_objects_cli.cmake` - `cli.roadobject_*` through
  `katana_cli`, a save and an open in a second process among them.

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
- **No window screenshot test**: the dialogs are tested as widgets
  (`tests/qt_widgets/bim/`), not through the running window's `--dialog`.
- **Built and run**: written by a parallel task that could not build; the
  integration build is the first to compile and run any of it.
