# Road design: road templates, corridors and the road model

Horizontal BIM - roads, not buildings. The owner's request of 2026-10-15:
"work on building 3d objects BIM toolbars and menu, im only interested in
horizontal bim like roads etc". This document is the record of the road
templates, the corridors swept along alignments with them, the 3D objects
made from those - pavement courses, kerbs, footpaths, verges, medians,
drains and earthworks - their quantities, and how all of it is reached from
the window, the command line and `katana_mcp`. The objects placed along a
road (barriers, signs, culverts, walls) are the road objects document's
(docs/road_objects.md, written beside this one); the IFC 4.3 export of the
whole road model is `docs/ifc.md`'s.

Where it lives:

| Part | Files |
|---|---|
| templates | `include/katana/cad/road/road_template.hpp`, `src/katana_cad/road/road_template.cpp` |
| corridors | `include/katana/cad/road/corridor_model.hpp`, `src/katana_cad/road/corridor_model.cpp` |
| the engine | `include/katana/cad/road/road_section.hpp`, `src/katana_cad/road/road_section.cpp` |
| kept in the project | `src/katana_cad/road/road_document.cpp` (`Document::roadTemplates`, `Document::corridors`) |
| the objects | `include/katana/cad/bim/bim_object.hpp`, `src/katana_cad/road/corridor_objects.cpp`, `src/katana_cad/bim/bim_object.cpp`, `include/katana/cad/bim/bim_view.hpp` |
| the verbs | `include/katana/cad/road/road_verbs.hpp`, `src/katana_cad/road/road_verbs.cpp` |
| the window | `src/katana_qt/bim/` (the workbench, three dialogs), `src/katana_qt/plan_painter.cpp` (`PlanPainter::drawRoadModel`) |
| the old corridor | `include/katana/cad/corridor.hpp` - now the same engine |

## Road templates

A template is a list of COMPONENTS out from the centreline on each side, in
order. Each has a width, a crossfall, the layer and colour its objects are
shown on, and - where it is paved or covered - its PAVEMENT COURSES, top
down, each a name, a thickness, a material, the IfcCourse type the export
writes and a colour.

| Kind | Its section | Courses | Object kind, IFC part |
|---|---|---|---|
| lane | width at crossfall | yes | pavement, TRAFFICLANE |
| shoulder | width at crossfall | yes | pavement, SHOULDER |
| kerb | its shape from the lip | no (it is one solid) | kerb, ROADSIDE |
| footpath | width at crossfall | yes | footpath, SIDEWALK |
| verge | width at crossfall | yes | verge, ROADSIDE |
| median | width at crossfall | yes | median, CENTRALRESERVE |
| drain | a V: its invert `depth` below its inner edge at mid width | yes | drain, ROADSIDE |
| batter | cut and fill slopes to the ground, benched or not | no | earthworks |

**Signs.** An offset is positive to the LEFT looking along increasing
station, as `SolvedAlignment::pointAtStationOffset` has it. A crossfall is a
PERCENT and is the change of level going OUTWARD from the centreline: -3 falls
3 % away from it (a crowned lane), +2.5 rises (a footpath rising to the
boundary). A batter slope is run per unit rise, as `cad::Assembly` had it: 2
is 1V:2H. *Rejected:* `cad::Assembly`'s positive-falls-away ratio - every
template value would then need its own sign rule, a footpath rising and a
superelevated lane being the common cases; outward percent is the one sign
for all of them.

**A kerb is a shape, not a width** (`road::kerbShape`). Its columns run out
from the lip - the pavement's edge - at each change of its top, with a level
base `depth` under the lip:

| Shape | Profile | Defaults |
|---|---|---|
| barrier | the face `height` high, battered 1 in 6, then `width` of top | 150 mm face, 150 mm top, 150 mm deep |
| mountable | the face rising `height` over 1.5 x height, then `width` | 100 mm face in the divided road |
| kerbgutter | the gutter `gutter` wide falling `gutterfall` %, then a barrier face | 300 mm gutter at 6 % |
| edgestrip | flush: `width` at the component's crossfall, `depth` thick | 300 mm by 150 mm |

The face is never vertical: a vertical face gives the finished surface two
levels at one point, which a TIN cannot hold, and the 1 in 6 batter reads as
upright at any scale a road is drawn at.

**A kerb faces one way** (`Component::faces`, `faces=inward|outward`, inward
by default). The shapes above are a kerb on the outer edge of a carriageway:
the lip is on the carriageway's side and the kerb rises going outward, so its
face looks in, at the traffic. A median's or an island's kerb is the other
way round - highest against the median, falling going outward to its lip at
the carriageway - and is the same shape seen from its other end
(`faces=outward`): its columns run from the back, its levels are measured
from the back's top, and its base stays `depth` under the lip. The divided
road's median kerb faces outward, so its lanes start 100 mm under the median's
top and the profile is the top of the median. (Facing the way a roadside kerb
does, the lanes started 100 mm ABOVE the profile and the kerb's face looked
into a depressed median; the solid, the IfcKerb and every level outside the
median were wrong with it. The offsets and levels, worked by hand, are
`RoadSection.TheDividedRoadsLanesSitUnderTheMedianAndItsKerbFacesTheTraffic`.)
An outward kerb's points are `<side>/<kerb>/face` (the crest of the face), `invert` for a
kerb and gutter, and `lip`, where an inward kerb's are `face`, `invert` and
`back`. An edge strip has no face and is refused with `faces=outward`.
*Rejected:* lifting the median by the kerb's height so the lanes sit on the
profile - a component cannot start above the design level its side starts at;
and a negative `height` for a falling kerb - one number for two things, and
the base would need its own sign rule.

**Limits.** `road::checkTemplate` refuses what would make a section cost the
session: a width, depth, thickness or maximum batter width over 1 000 m
(`road::kMaximumComponentLength`), and a batter that would make more than 100
benches across its maximum width (`road::kMaximumBenches`: the reach divided
by the bench period, a bench height of the flatter slope's run plus the flat).
The first is a sanity bound with no standard behind it: a section is sampled
every half metre across its whole span, so a width typed in the wrong unit
cost time linear in it and, past 4.5e15 m where adding 0.5 no longer moves a
double, never ended. The second keeps a bench every millimetre (a typo) from
hanging the build: each bench is two named points a section and the cut and
fill areas cost more than linearly in a section's points. *Rejected:* a floor
on the bench's size (say 10 mm) - the cost is the number of benches, which
depends on the reach too, and a 0.1 m reach with a 50 mm bench is a few
points; and a linear merge-walk in `areaBetween` instead - the bound already
keeps a section to a few hundred points. The refusal reaches every route a
template takes into the drawing: the verbs, the JSON read back with a project,
and a section or corridor being built.

### Defaults, and where they come from

`road::defaultComponent` gives each kind its starting values; a designer
edits them. Each is of the order the guide named gives, not a design:

- a 3.5 m lane at 3 % crossfall - the common rural two-lane cross-section in
  Austroads' Guide to Road Design Part 3 (Geometric Design);
- footpaths and verges rising 2.5 % - no steeper across than the 1 in 40 AS
  1428.1 allows a continuous accessible path of travel;
- kerbs as the AASHTO Green Book (A Policy on Geometric Design of Highways and
  Streets, 2018, section 4.7) describes curbs: a vertical curb 150 to 200 mm
  high, a sloping one 100 mm or less, a gutter 0.3 to 1.8 m wide falling 5 to
  8 % - hence 150 mm, 100 mm, 300 mm and 6 %;
- a fill batter of 1V:4H, the slope the AASHTO Roadside Design Guide (2011,
  chapter 3) calls recoverable, and a 1V:2H cut, a common slope in stable
  ground; batters looked for out to 50 m, as the old assembly did;
- the courses of a typical flexible pavement: 50 mm of AC14 over 200 mm of
  base and 250 mm of subbase, both crushed rock; a sprayed seal on the
  shoulders; 100 mm concrete paths on 50 mm of sand; 100 mm of topsoil on
  verges and medians. A course's colour follows its material
  (`road::makeCourse`).

`ROADTEMPLATE DEFAULTS` adds three standard templates (`road::standardTemplates`),
each side the mirror of the other: **rural two-lane** (3.5 m lane, 2 m sealed
shoulder, table drain, batter), **urban street** (3.3 m lane, 2.3 m parking
lane, kerb and gutter, 1.5 m footpath, 1 m verge, batter) and **divided road**
(2 m median each side, mountable kerb facing outward, two 3.5 m lanes, 2.5 m
shoulder, batter; its profile is the median's top).

## Corridors

A corridor names its alignment (with a design profile), its road template
and its TARGET SURFACE, and holds what is its own: a station range (each end
absent is the alignment and profile's overlap), the interval it is sectioned
at (10 m by default), REGIONS where another template applies, and the
SUPERELEVATION table.

**Superelevation** is a table of stations, each with the crossfall of the
superelevated components (lanes and shoulders by default) on each side,
outward percent: (+6, -6) is full superelevation falling to the right, for a
curve to the right. Between two entries it is linear; outside the table each
component keeps its template crossfall. A table is entered as a
superelevation diagram is drawn, with normal crown at both ends of each
transition (`road::superelevationAt`). *Rejected:* holding the nearest
entry outside the table - one entry at the middle of a curve would then
superelevate the whole road. *Rejected:* rules from a design speed and a
radius - those are a design standard's, differ between authorities, and the
table is what such a calculation produces anyway.

**Regions** run the corridor with another template between two stations,
start before end, not overlapping, kept in station order. The corridor's own
template runs before, between and after them, so a corridor is a series of
RUNS, numbered from 1 in station order; the number is in each object's id.

## Building a corridor

`road::buildCorridor` sections each run at the interval (multiplied from the
corridor's start, as the setting-out table is, so the hundredth is not off by
a hundred roundings) and at every key station - the alignment's TS, SC, CS,
ST, the profile's PVC and PVT, each superelevation station and each region's
ends - because the road changes character there and a schedule that stepped
over them could not be checked (`road::sectionStations`). More than 100 000
sections is refused before one is made, as ALIGN STATIONS refuses as many
rows.

**A section** (`road::roadSection`) sets each side's components out in turn
from the centreline at the design level; a lane or shoulder at its crossfall
(or the superelevation's), a kerb by its shape, a drain as its V, the rest at
their crossfalls. Each point is named - `CL`, `left/lane/outer`,
`left/kerb/face`, `right/batter/daylight` - which is what strings are joined
by. Each course of a component is a solid of columns at the component's
vertices, `thickness` apart vertically; a kerb is one solid of its shape's
columns.

**The batter** starts at the hinge, the last finished point, and is marched
out to the target surface exactly as the old corridor's daylight was:
half-metre steps until the batter and the ground change order, then forty
bisections (`docs/cad.md`, "Corridor quantities"). The last step is the
maximum width itself, so a limit that is not a multiple of half a metre - or
is under half a metre - still finds a daylight inside it
(`RoadSection.ABatterLimitThatIsNotAMultipleOfAStepStillFindsADaylightInsideIt`).
Benches are flats of their width every bench height of rise or fall. A batter
that meets no ground within its maximum width - or meets no surface at all -
leaves its side NOT REACHED and the section incomplete; its pavement is still
built.

**The formation** - the subgrade - is the bottom of each component's courses,
a kerb's base, the top of what has no courses, and then the batter. Where two
neighbours' bottoms differ, or the courses end at the hinge, the line steps,
and the shallower corner is set `road::kFaceRun` (1 mm) into its own
component, so the formation stays a function of offset. A TIN of it needs
that (two levels at one point cannot be triangulated), and so do the end
areas. 1 mm is more than the TIN builder's duplicate tolerance (0.1 mm,
`math::tolerance::kCoordinate`) and less than any construction tolerance on
a subgrade; the area it moves is under a thousandth of a square metre for a
metre-deep pavement, and the tests work it in by hand
(`RoadSection.ThePavementBoxIsCutAndTheBatterStartsAtTheFinishedEdge`).
*Rejected:* taking the deeper of two neighbours across the shallower one -
the subgrade surface then lies below the shallower courses for the whole of
its width.

A side that is nothing but its batter (a ramp's open edge, an embankment of
two slopes) has no component to put a point on the line, so its formation
starts at the hinge - the centreline at the design level, coded
`<side>/batter/hinge` - and meets the other side's first point at the
centreline as two components meet: one point when the levels agree, else the
shallower corner stepped 1 mm. Before this the line ran straight from that
side's daylight to the other side's first point and the triangle under the
batter was left out of the cut and fill, the earthworks solids and the
subgrade
(`RoadSection.ASideOfABatterAloneStartsItsFormationAtTheHingeAndMeasuresTheTriangleUnderIt`,
`CorridorObjects.AnEmbankmentOfBattersAloneIsTheTrianglesUnderTheirSlopesAndItsSubgradeRises`).

**Earthworks** are between the formation and the ground across the section:
CUT where the ground is above it - the pavement box is cut - and FILL where
it is below, each side's split at the centreline. Along the run they are
measured by the AVERAGE END AREA between two complete sections, as
`corridorQuantities` always measured them, and for the reason it gives: it is
the method specifications and measurement standards name. An interval
touching an incomplete section adds nothing, and the count is a problem.

A side has cut (or fill) at a section only where the ground and the formation
are more than 0.1 mm apart (`math::tolerance::kCoordinate`, the TIN's
duplicate tolerance) at some point of the stretch: `RoadSection::leftCutSpans`
and its three kin hold those stretches, out from the centreline, found
exactly where the two lines cross between the ground samples. A side with none
has an area of exactly zero. The daylight is bisected to about 1e-13 m, so it
lands on either side of the ground by that much, and the other kind of
earthworks was a sliver of 1e-26 m2 that "area > 0" took for a solid: a
full-length cut object beside a road in fill. *Rejected:* a volume
threshold on the object (1e-6 m3 or so) - the area is the thing wrong, and
`built.cut` and `CORRIDOR SECTION` would still carry it. The price: a sheet
of earthworks thinner than 0.1 mm over a wide road is left out (0.05 mm over
a 30 m width and 100 m is 0.15 m3).

**Where two runs meet** - a region's start or end - each run has a section of
its own at the station, built with its own template (`RoadSection::run` tells
them apart). The solids are lofted per run, so they step there. The strings
and the surfaces do the same: `road::corridorSections` lists both sections,
every string breaks at the join and runs over its own run's stations (a code
only the region has is one string from the region's start to its end, not
from one interval after), and a surface lays the later run's section 1 mm on
(`kFaceRun`, the formation's device across an offset) so that the TIN, which
holds one level at a point, has a near-vertical face there and each template's
own surface either side of it. Before, the later run's section at the join was
dropped for the earlier run's: a region's own codes lost their first interval,
a region one interval long lost its strings altogether, a code both templates
have was joined across the change, and the surface blended the two templates
over the interval beside each join, so it disagreed with the solids and the
region (0.17 m low on a kerb inside a region, 0.21 m high on a drain just after
it). *Rejected:* one section standing for the station, taken from the run
that is longer or the later one - either way the other template's last
interval is drawn with the wrong section.

**One engine.** `cad::corridorSection`, `corridorQuantities` and
`corridorSurface` (`include/katana/cad/corridor.hpp`) are this engine run on
`road::templateFromAssembly` - a lane each side at the assembly's half width
and crossfall, then a batter of its slopes - with their old refusals checked
first in their old words. Their tests (`tests/cad/test_corridor.cpp`) are
unchanged and are the check that the engine is the old one where the two
meet. The only difference: their stations now include the alignment's key
stations as well as the profile's.

**What it cannot build it says.** `road::BuiltCorridor::problems` lists,
a sentence each: an alignment, a design profile, a template or a surface the
drawing lacks; a template that cannot be built; a range off the alignment;
regions outside the corridor; no target surface ("the batters and the
earthworks are not built"); sections that do not reach the ground. The rest
is built.

## The road model's objects

`bim::corridorObjects` builds every corridor and `road::corridorSolids` makes
its objects (`include/katana/cad/bim/bim_object.hpp`):

- a closed solid per COURSE of each component, per side, per run;
- one per KERB per side per run;
- the earthworks CUT and FILL of each side of each run: a closed solid for
  each PIECE - a stretch of stations where that side has any (see above),
  inside a stretch of complete sections, with the one section beyond each end
  where its end area tapers to nothing. None where there is none.

**A closed solid** (`road::loftSolid`): the solid's columns at each station
give a top and a bottom face, the first and last column an inner and an
outer face, and the first and last station a cap each - each quad walked so
that every edge it shares is walked the other way by its neighbour, so each
edge is shared by exactly two triangles and the solid is closed and
consistently turned. Which way that turns depends on the side (offsets grow
to the left, so the right side's solids are mirror images): it is turned
outward by the sign of the volume it encloses. An earthworks solid has
`road::kEarthworksColumns` (41) columns a side - its look, not its quantity -
spread from where its earthworks start across each section to where they end,
not from the centreline to the daylight: a fill beside a road in cut is no
sheet laid over the whole half-section. Where a section's earthworks have a
gap in them (cut, fill, cut across one section) the columns span the gap with
no thickness, and its faces there have no area, which leaves the solid
closed.

**Object ids** are stable - the IFC export derives each GlobalId from one,
and a model handed over twice must name the same things the same way:

    corridor/<corridor>/<side>/<component>/<course>/<run>
    corridor/<corridor>/<side>/<component>/<run>                  a kerb
    corridor/<corridor>/<side>/earthworks/cut|fill/<run>[-<piece>]

`<piece>` only from the second piece of that kind and side in a run, counted
in station order over the pieces made: a fill that ends along the road and
begins again is `fill/1` and `fill/1-2`. A piece is a stretch of stations
over which that side has cut (or fill), inside a stretch of complete sections
(`CorridorObjects.EarthworksSplitWhereTheirThicknessEndsAndTheSecondPieceCarriesItsNumber`).

**IFC classes**, as the IFC export reads them (`docs/ifc.md`): a pavement,
verge, median or drain course is an IfcCourse of its course's type (PAVEMENT
by default; a verge's and a median's topsoil NOTDEFINED); a footpath's top
course is the path itself, an IfcSlab SIDEWALK, the bedding under it an
IfcCourse; a kerb an IfcKerb with no predefined type (the schema has none to
name), its `mountable` property IfcKerb's attribute; earthworks an
IfcEarthworksCut CUT or an IfcEarthworksFill EMBANKMENT, the cut carrying the
`Target Surface` property the export reads. The facility is the corridor; the
part is an IfcRoadPartTypeEnum word.

**Quantities**, named as IFC's base quantities are, units m, m2, m3:

| Object | Length | Width | Depth / Height | GrossArea | GrossVolume |
|---|---|---|---|---|---|
| course | the run's stations | the component's | Depth: its thickness | its plan outline | the solid's own volume |
| kerb | its lip line in plan | the shape's | Height: its face | - | the solid's own volume |
| earthworks | the piece's stations | - | - | its footprint's plan area | end area |

An earthworks object's GrossArea is the plan area of its footprint: its plan
outline runs from where the earthworks start to where they end across each
section, narrowing to a point over the last interval where the end area
tapers to nothing (as the average-end-area volume does), so on a straight a
side's cut and fill add up to the side's footprint
(`CorridorObjects.ASideWithCutAndFillGivesEachItsOwnFootprintAndTheTwoAddUpToTheSide`).
A section with the earthworks in two separate stretches draws and measures
their hull.

A course's volume is the SOLID'S, so the model and its quantity agree and a
course on the outside of a curve - longer than the centreline - is measured
as the longer thing it is. The earthworks' is the END AREA, the measurement
standard, as above; the solid drawn for them differs from it by the corners
its 41 columns cut, which the tests work out by hand
(`CorridorObjects.TheEarthworksAreTheEndAreasTheOldAssemblyMeasured`).

**Layers and colours.** An object is on its component's layer
(`road/pavement`, `road/shoulder`, `road/kerb`, `road/footpath`,
`road/verge`, `road/median`, `road/drain`), the earthworks on
`road/earthworks/cut` and `road/earthworks/fill`; a course in its course's
colour, a kerb in its component's, earthworks blue for cut and red for fill,
as the Cut Fill surface style draws them.

**An object follows its layer** (`bim::isShown`): the deepest part of its
path the drawing has decides, so "road" switched off hides everything under
it even before "road/pavement" is made, and a path the drawing has none of
is drawn - an object is not an entity and is drawn in its own colour. A view
that hides a layer of its own hides its objects there. `CORRIDOR NEW` makes
the layers its objects are on in the same undo step, `road/earthworks/cut`
switched OFF: a cut is the ground taken away, and drawn solid it would bury
the road it was taken away for.

## Kept with the project

The templates and corridors are versioned JSON under the project metadata
keys `road_templates` and `corridors` (`road::roadTemplatesToJson`,
`road::corridorsToJson`), the sheets' pattern (`docs/plotting.md`): the storage
layer carries a key it does not read, so no schema change; parsed once per
change and shared by an undo step; every edit ONE undo step
(`Document::setRoadTemplates`, `Document::setCorridors`, and the command forms
for a step that does more - CORRIDOR NEW with its layers, a template renamed
with the corridors that name it); an empty set stored as no key, so undoing
the first template leaves the metadata as it was; a set equal to the current
one no step; what a newer Katana wrote kept, every edit refused
(`Unsupported` read, `CommandRejected` on a write).

**The RoadModel change.** `Document::roadModelChanged` compares the road
model's metadata keys - these two and the placed objects' `road_objects` -
after every command, undo and redo, and reports `DocumentChange::RoadModel`
with the step's other parts: one notification, and the placed objects need no
mechanism of their own. *Rejected:* each store calling the notifier from its
command - a second notification per step, and one the placed objects would
have had to repeat. The first command after a project is opened may report
it once more than it changed; what listens rebuilds from a cache, so that
costs nothing (below).

**The cache** (`bim::bimModel`, `src/katana_cad/bim/bim_object.cpp`). Made
once per change of what it is made from, in two halves: the corridors' on the
two keys' text, the alignments' revision and the surfaces' generation - so a
line drawn does not rebuild them - and the placed objects' on their key and
every step, since an object may stand on a drawn line. A half made again that
equals what it replaced keeps the old one, so the model a view holds is the
same object until something in it changes
(`RoadVerbs.TheRoadModelIsMadeAgainWhenATemplateChangesAndNotOtherwise`).
The views call it every paint; the comparison is a few kilobytes of text.

## The verbs

The interpreter's (`road::runRoadVerb`), so the window's command line,
`katana_cli` and `katana_mcp` all run them; `HELP CORRIDOR` lists every word.

    ROADTEMPLATE [LIST] [JSON] | INFO <t> | DEFAULTS | SECTION <t>
    ROADTEMPLATE NEW <t> [FROM <t>] [description=] | DELETE <t> | RENAME <t> <new>
    ROADTEMPLATE ADD <t> left|right|both <kind> [name=] [at=<n>] [key=value ...]
    ROADTEMPLATE SET <t> <component> key=value ... | REMOVE <t> <component>
    ROADTEMPLATE COURSE <t> <component> ADD <course> thickness= [material=] [ifc=] [colour=] [at=]
    ROADTEMPLATE COURSE <t> <component> SET <course> key=value ... | REMOVE <course>
    CORRIDOR [LIST] [JSON] | INFO <c> | PROBLEMS [<c>]
    CORRIDOR NEW <c> ALIGNMENT <a> TEMPLATE <t> [surface=] [interval=] [from=] [to=]
    CORRIDOR SET <c> key=value ... | DELETE <c> | RENAME <c> <new>
    CORRIDOR REGION <c> ADD <from> <to> <t> | REMOVE <from> | CLEAR
    CORRIDOR SUPERELEVATION <c> ADD <station> <left%> <right%> | REMOVE <station> | CLEAR
    CORRIDOR BUILD <c>|ALL | SECTION <c> <station>
    CORRIDOR STRINGS <c> [layer=] [PREVIEW] | SURFACE <c> [finished=] [subgrade=] [PREVIEW]
    CORRIDOR QUANTITIES <c>|ALL [csv=<file>]
    ROADMODEL [LIST] | JSON | PROBLEMS | INFO <id>

A `<component>` is `side:name` or `name`, every side that has one. A
component goes before its side's batter, or at `at=`; a second of a name is
`name2`. Words are case-insensitive, names found ignoring case.

**Replies** are records, key=value, one a line. A definition's numbers are
written exactly (`core::formatExactReal`), so a value a reply prints can be
typed back; a computed length, level, area or volume to three places - a
millimetre, a thousandth of a square or cubic metre - which is what a
schedule is read to (`std::format`, locale-free). A refusal names what is
wrong: `NotFound` for a template, component, course, corridor, alignment or
surface the drawing lacks, `ParseFailure` for a word or value that does not
read, `InvalidArgument` for a template or corridor that cannot be built
(`road::checkTemplate`, `road::checkCorridor`), `AlreadyExists` for a name in
use, `InvalidState` for definitions that cannot be read. A template a
corridor uses is not deleted from under it.

**STRINGS** draws each code's points along the corridor as a 3D polyline
(`entity::setHeights`) on `road/strings`, marked `road.corridor` and
`road.string`, replacing in the same step the strings that corridor made
before and nothing else - found by its name read ignoring case, as corridors
are found. `CORRIDOR RENAME` retags the strings with the new name in its own
undo step (`strings=<n>` in the reply), or the next STRINGS would leave them
beside a second set and a corridor later given the old name would delete
them. `CORRIDOR DELETE` leaves its strings in the drawing: they are
entities a person may have kept; a corridor later made with the same name
replaces them on its next STRINGS. **SURFACE** puts the finished surface and the
formation into the drawing's surfaces (`Document::setSurfaces`), as `<c>
finished` and `<c> subgrade` unless named, replacing surfaces of those names
in place: every section's points (the reply's `sections=` counts them, both of
a join's), each code's string as a breakline, the outermost points as the
boundary. Both have PREVIEW.

**Scope.** The corridor verbs act on named definitions, not on drawing
entities, so they take no scope or filter (the scope and filter words are for
verbs on drawing data: `docs/cad.md`, "Scope and filter"): what a corridor is made from is named in it. What they make
that is drawing data - STRINGS - goes on a layer of its own and replaces only
what the same corridor made before. QUANTITIES takes a corridor or ALL.

## In the window

The BIM menu (`bimMenu`), after Terrain, and the BIM toolbar beside Terrain's
on the second row (`src/katana_qt/bim/road_workbench.hpp`), each item building
its line and running it through the window's one executor:

| Section | Item | Object name | Dialog |
|---|---|---|---|
| Templates | Road Template Manager... | `bimRoadTemplates` | `roadTemplateManagerDialog` |
| Corridors | Corridors... | `bimCorridors` | `corridorDialog` |
| Model | Show Road Model | `bimShowRoadModel` | - |
| Quantities | Corridor Quantities... (Ctrl+Shift+Q) | `bimCorridorQuantities` | `corridorQuantitiesDialog` |

The road objects' items follow, in a section of their own (the road
objects document, docs/road_objects.md). The dialogs' widgets and what each runs are listed
in their headers: `src/katana_qt/bim/road_template_manager.hpp` (the templates;
the components and courses in two grids that are buffers, applied by one SET
line per edited row, as the Alignment Manager's are; a drawing of the
template across a level road - `road::templateSection` - its courses
coloured and its vertical exaggerated so a 50 mm course can be seen),
`src/katana_qt/bim/corridor_dialog.hpp` (the corridors, their regions and
superelevation, Build, Build Strings, Build Surfaces, and the problems) and
`src/katana_qt/bim/corridor_quantities_dialog.hpp` (the records of CORRIDOR
QUANTITIES as a table, Copy and Save CSV; no file dialog in a headless run).

**Where the old corridor commands went.** Terrain > Corridor Quantities... and
Corridor Surface... asked for an alignment, a surface and an assembly in a
modal box (`askCorridor`) and called `cad::` directly - no verb, nothing
kept, nothing to undo, nothing an agent could reach. They are gone; Terrain's
Corridors section now shows the BIM menu's Corridors... and Corridor
Quantities... - the same QAction objects, as File shows the other menus'
imports - and both still say where an alignment with a design profile is made
when there is none
(`qt_terrain_commands_point_to_the_alignment_manager_headless`). A
one-carriageway corridor is a template like any other now. The Terrain
toolbar lost the two buttons, which are the BIM toolbar's.

**The road model in the views.** Each 3D view draws `bim::bimModel`'s objects
as meshes in their colours (`bim::sceneMeshes`), put in the window's list of
meshes beside the imported ones (`MainWindow::setRoadMeshes`), marked
`SceneMesh::planFootprint` off so that the plan view does not also draw
their hulls - a curved course's hull would cover the inside of the curve.
Each plan view, plot and sheet draws their plan outlines through the one
painter (`PlanSource::roadModel`, `PlanPainter::drawRoadModel`), beneath the
drawing as a mesh's footprint is, in the object's colour with a light fill,
earthworks dashed; Zoom Extents counts them. The meshes are made again from
the event loop when the road model, the surfaces, the alignments or the
layers change, and handed over only when what is shown changed. Show Road
Model (`setRoadModelShown`) takes them out of all of it for the session, as
View > Thin screen lines is set; an agent hides them by their layers.

**The layout.** The new toolbar does not bump `kLayoutVersion`: a layout saved
before it has no BIM toolbar, which Qt then places at the end of the second
row, after GIS - shown, not misplaced. Raising the version would have put
every saved layout back to the default for that.

## Through MCP

`katana://road-model` is `ROADMODEL JSON` (`road::roadModelJson`): every
object without its mesh - its triangle count and box instead - with its IFC
class, layer, quantities and properties, and every problem
(`docs/mcp.md`). Every verb above reaches `katana_mcp` through
`katana_run_commands`.

## Tests

- `tests/cad/road/test_road_template.cpp`: the standard templates valid and
  mirrored, a kerb's shapes worked by hand, the checks, the JSON round trip
  exact.
- `tests/cad/road/test_road_section.cpp`: a street set out at hand-worked
  offsets and levels, its solids, the formation's steps, superelevation at
  the middle of its transition, batters daylighting on a plane in cut, fill
  and on sidelong ground, the pavement box cut, benches, refusals.
- `tests/cad/road/test_corridor_objects.cpp`: course volumes on a straight as
  width x thickness x length, every solid closed and turned outward, stable
  ids and IFC classes, a kerb's length and volume, the earthworks against the
  old assembly's end areas, regions, problems, strings and surfaces.
- `tests/cad/road/test_road_verbs.cpp`: each verb as one undo step, refusals,
  layers, renames, quantities and the CSV, strings, surfaces, the cache, the
  RoadModel change, the JSON the project keeps.
- `tests/cad/test_corridor.cpp`: the old functions, unchanged, on the new engine.
- `tests/qt_widgets/bim/test_road_dialogs.cpp`: each dialog's lines and the
  workbench's menu and meshes.
- `qt_the_bim_menu_makes_a_corridor_through_its_dialogs_headless` and
  `qt_terrain_commands_point_to_the_alignment_manager_headless`: the real
  window.
- `src/katana_app/road_cli.cmake`: `katana_cli` end to end, and a road model
  saved and read back by another process
  (`cli.a_saved_road_model_is_read_back_by_another_process`).
- `McpServer.TheRoadModelIsAResourceAsRoadModelJsonGivesIt`.

## Not done

- **An earthworks solid still has faces of no area where it tapers to
  nothing.** The solids are split along the road where a side has none and
  drawn over the stretch across the section where it has some, but the end
  section of a piece (the one with none) and a gap inside one section have
  no thickness: the mesh is closed, but the IFC writer drops a face of no
  area (docs/ifc.md, "Road model"), which leaves its neighbours' edges used
  once, and writes the solid as an open face set with a warning. Every course
  is closed. The piece should end in a real closed taper (a wedge welded to a
  line), and a section with a gap should be two stretches across.
- Superelevation by design speed and radius: the table is entered by hand.
- Shoulder rollover on the high side of a superelevated section: a
  superelevated shoulder takes the lane's crossfall.
- A template that changes width along a run (lane widening, tapers) other
  than by a region of another template, and transitions between regions: a
  region's ends are steps (the solids step, the strings break, the surface
  has a face 1 mm wide).
- Daylight to a surface other than the corridor's target per side, and cut
  and fill batters with different benches.
- Courses under a kerb (its bedding) and a lined table drain's lining as a
  solid apart from its courses.
- The earthworks' quantity by the prismoidal formula: end area only, as
  corridorQuantities. The volumes work has a prismoidal method along an
  alignment between two surfaces (the volumes document, docs/volumes.md).
- Mass haul for a corridor.
- Sections of a corridor in the section view, and sheets of cross sections
  from a corridor (GENERATE sections draws the ground and the profile only).
- A verb for Show Road Model (session display state, as the grid's toggle is
  in a window verb): an agent hides the objects by their layers.
- The 3D view draws every object in one colour per object, shaded; no
  material textures, no transparency for the cut.
- `ROADTEMPLATE` has no import or export to a file of its own; a project's
  templates are copied by its JSON (`ROADTEMPLATE JSON`) only by hand.
- The window's Command Reference shows the road verbs in its Commands
  section from the general help, not a section of their own.
