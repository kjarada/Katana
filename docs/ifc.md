# IFC 4.3 exchange

`katana_ifc` writes the drawing, its alignments, its surfaces and an AS 5488
subsurface utility investigation as an IFC 4.3 file (IFC4X3_ADD2), and reads
IFC2X3, IFC4 and IFC4X3 files back into the drawing. It needs no third-party
library, like `katana_dxf` and `katana_archive12d`: IFC is ISO 10303-21 text,
and what matters is the mapping, not a model library
(`include/katana/ifc/export.hpp`, "Why a writer of our own"). It sits beside
the other exchange modules in the layering (`tools/check_layering.cmake`): it
may see `core`, `math`, `geometry`, `terrain`, `entity`, `commands` and
`survey`, and `katana_app` and `katana_qt` may see it.

The one decision the export exists to get right is **which IFC class each
thing becomes**. A file whose every object is an `IfcBuildingElementProxy`
says nothing a DXF did not: a consumer cannot ask it for the water mains, the
kerbs or the pits. So nothing here falls back to a proxy - what is known to be
an element becomes the element class the schema has for it, and what is only
known to be a line someone surveyed or drew becomes an `IfcAnnotation`, which
IFC 4.3 made for survey elements and which claims no more than is known.

## Using it

In the window, Import IFC and Export IFC, in File's Import and Export
submenus (below, "In the window"); a .ifc also goes through Import Any File,
a path given to the window and Export Drawing's IFC filter. On either command line - `katana_cli`, and so
`katana_mcp`, and the window's own - the verbs, which are also all the
dialogs do (each writes its line and runs it):

```
EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>] [RULES <rules.csv>]
                  [SPACING <m>] [NODRAWING] [NOENTITIES] [SELECTED] [NOALIGNMENTS]
                  [NOSURFACES] [PREVIEW]
IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES] [TOLERANCE <m>]
                  [TAKECRS | KEEPCRS]
INFO <file.ifc>
IFC RULES <file.csv>
```

```sh
katana_cli samples/ifc/scenario.txt -c "EXPORT site.ifc UTILITIES samples/utilities/schedule.csv"
katana_cli samples/ifc/scenario.txt -c "EXPORT site.ifc PREVIEW"
katana_cli -c "EXPORT services.ifc UTILITIES samples/utilities/schedule_tfnsw.csv SCHEMA tests/ifc/data/delivery_schema.csv NODRAWING"
katana_cli -c "INFO site.ifc" -c "IMPORT site.ifc TAKECRS" -c "ALIGN LIST"
```

The grammar is one function both front ends call
(`ifc::parseExportArguments`, `ifc::parseImportArguments` in
`include/katana/ifc/front_end.hpp`), as are reading the files an export names
(`ifc::readExportFiles`), the GlobalId namespace and the replies, so the same
line writes the same file and answers alike in either. A path is quoted, or
read up to the first ".ifc" that ends a word, so that one with blanks may be
typed as it is; a path that holds ".ifc " before its end is quoted, and a
quote opened before the .ifc and never closed is refused rather than read as
part of the path. Words are matched in any case, each once: a word given twice
is refused, since the second would silently repeat or undo the first.

EXPORT writes the drawing's entities and alignments and the window's
surfaces, georeferenced by the project's coordinate system (`CRS SET`).
UTILITIES adds an investigation in the schedule format of
`docs/subsurface_utilities.md`; SCHEMA names the delivery schema it was
written to (`UTILITY CHECK`'s schema file); RULES a project's classification
rules ("Classification rules", below); SPACING is the longest detected
spacing that keeps QL-B (10 m by default). NOENTITIES, NOALIGNMENTS and
NOSURFACES leave those out - NODRAWING is all three, the investigation alone -
and SELECTED writes only the selected entities, refusing when nothing is
selected rather than writing them all (an empty list is every entity to the
writer, `ExportOptions::entities`). PREVIEW writes nothing: it answers what
the file would hold.

IMPORT brings a file's alignments, elements and annotations in as one
undoable step; NOALIGNMENTS, NOELEMENTS and NOSURFACES leave those out, and
all three together are refused. LOCAL moves them to sit at the origin, as the
other importers' LOCAL does - by the corner of everything the file brings,
alignments and surfaces included (`IfcImport::bounds`). TOLERANCE is how far
a chorded curve may stand from the true one (`ImportOptions::curveTolerance`,
1 mm by default). TAKECRS gives a project with no coordinate system the
file's, as its own undoable step; KEEPCRS does not; with neither, the line
says how (`CRS SET`), and File > Import asks. TAKECRS with LOCAL is refused:
data moved to the origin is in no system. `katana_cli` holds no surfaces, so
a terrain in the file is counted and not kept there; the window keeps it.
INFO describes a file without importing it; IFC RULES writes the default
classification rules, for a project to edit into its own.
`src/katana_app/ifc_verbs.cpp` is the whole of `katana_cli`'s side.

### Replies

Every reply is `key=value` records, one a line, as the interpreter's verbs
answer (`docs/cad.md`), so that a script or an agent reads what happened
without parsing sentences. Text values are quoted (`core::replyQuoted`) and
read back by `core::readReplyRecord`; the formats are written in one place
(`ifc::formatExportReply`, `ifc::formatImportReply`,
`ifc::formatDescription`) and File > Export IFC's table is read from them by
`ifc::readExportObjects`, so the writer and its one reader are kept in step
(`IfcReplies.AnExportsObjectRecordsReadBackAsItsTally`):

```
ifc exported file="site.ifc" schema=IFC4X3_ADD2 instances=1097 bytes=82906
counts alignments=1 services=4 segments=10 segments_3d=6 located_points=14 entities_written=5 entities_skipped=0 surfaces=0
class name=IfcKerb count=1
object from="layer Survey/Kerb" count=1 class="IfcKerb" predefined="NOTDEFINED" object_type="" system="" why="rule kerb"
object from="service E1" count=3 class="IfcCableCarrierSegment" predefined="CONDUITSEGMENT" object_type="" system="ELECTRICAL" why="a graded segment: configuration \"4 x 100 mm conduits\""
warning text="..."

ifc imported file="site.ifc" schema=IFC4X3_ADD2 crs="EPSG:7856" entities=29 alignments=1 surfaces=0 objects=29 drawn=29 as_points=0 alignments_as_polylines=0
class name=IfcKerb count=1
extent min_x=333900.000 min_y=6249950.000 max_x=334200.000 max_y=6250060.000
note text="the project is now in EPSG:7856, the file's coordinate system"

ifc described file="site.ifc" ... layers=15
alignment name="MC01" pis=4 pvis=4
```

PREVIEW's head is `ifc previewed`, with no `bytes`. A front end adds `note`
records of its own after the reply - the coordinate system taken or how to
take it, a shift - and an import's reply is written after the undoable
command, whose renames of an alignment the file shares a name with are among
its warnings. `IfcFrontEnd.EveryChoiceOfTheDialogsIsAWordOfTheLine`,
`IfcFrontEnd.ContradictoryOrRepeatedWordsAreRefused`,
`IfcFrontEnd.TheLineADialogWritesReadsBackAsItsArguments` and
`cli.ifc_every_choice_of_the_dialogs_is_a_word_of_the_line` pin the grammar;
`McpServer.AnAgentPreviewsExportsDescribesAndImportsIfcByTheDialogsLines` is
an agent doing all of it through `katana_mcp`.

### In the window

File > Export IFC is a dialog (`src/katana_qt/ifc_dialogs.hpp`, `docs/desktop.md`)
of the verb's choices, with what the window knows beside them: the file; the
drawing's entities (or only the selected ones), its alignments and the
session's surfaces, each with its count; the utility schedule, its delivery
schema and the detected spacing; a rules file, which Save Default Rules starts
from the defaults (`IFC RULES`); and whether the file will be georeferenced,
from the project's coordinate system. It writes the EXPORT line its fields
mean, shows it (`ifcExportCommand`), and runs it on the window's command line,
as a person or an agent could type it. Its table previews the file: for each
layer, service, alignment and surface, how many objects become which class,
in which system, and why. It is read from the reply of the same line with
PREVIEW added - the writer's own account (`ifc::IfcExport::tally`, filled as
each object is written, by writing the file in memory) - so what a person
checks before writing is exactly what is written, and what an agent asking
PREVIEW reads. Four of its rows for the scenario with the sample schedule
(`qt_ifc_export_dialog_previews_each_class_and_writes_the_file_headless`):

| From | Objects | IFC class | System | Why |
|---|---|---|---|---|
| layer Survey/Kerb | 1 | IfcKerb NOTDEFINED | | rule kerb |
| layer Stormwater/Pits | 1 | IfcDistributionChamberElement INSPECTIONPIT | STORMWATER | rule pit |
| layer Survey/Points | 1 | IfcAnnotation TEXT | | no rule: a text |
| service E1 | 3 | IfcCableCarrierSegment CONDUITSEGMENT | ELECTRICAL | a graded segment: configuration "4 x 100 mm conduits" |

What is not written is accounted for too: a label is "not written", "a
label: labels are not exported". Every product the report counts is in the
table once (`IfcExportTally.EveryProductTheReportCountsIsAccountedForOnce`).

The table is the file the choices made when Preview was pressed: changing any
of them but the file's name clears it, and a problem found since replaces the
last result. The window's counts - entities, the selection, alignments,
surfaces, the coordinate system - are read again as the drawing changes and
before each preview and export, so "Selected entities only" with nothing
selected is refused ("select what to export, or untick it") rather than
writing the whole drawing or none of it.

File > Import IFC describes a file first (Describe runs `INFO`: schema,
coordinate system, classes, alignments, surfaces) and imports what is ticked -
alignments, elements, surfaces - moved to the origin or not, with the curve
tolerance: its IMPORT line (`ifcImportCommand`) always says TAKECRS or
KEEPCRS, from its "Take the file's coordinate system". A file far from the
drawing - its entities, alignments and surfaces - is shifted alongside or
kept, as every import asks; declining is a cancel, which the dialog says as
one. File > Import and a path given to the window, whose line says neither,
ask about the coordinate system; a typed IMPORT or a headless session asks no
one and says how (`CRS SET`). Terrain comes in as surfaces of the session,
which Export IFC writes out again.


## Export: what goes where

### The project, its units and its georeferencing

One `IfcProject` (units metre, square and cubic metre, radian) with one 3D
model context and the subcontexts `Axis`, `Body`, `FootPrint` and
`Annotation`; one `IfcSite` holding every element and annotation; the
alignments aggregated into the project beside the site. The header names the
view the file keeps to: `ViewDefinition [Alignment-basedView]`, IFC 4.3's
view for alignments and what is positioned along them.

When the project's coordinate system has an EPSG code, it is written as an
`IfcProjectedCRS` named by that code with an `IfcMapConversion`, and the
coordinates are written relative to a **local origin**: the south-west corner
of everything exported, rounded down to 100 m in plan, at height 0. On the
scenario MC01's first PI is furthest west and south, so the origin is
333900, 6249900 (`IfcExport.IsGeoreferencedByAMapConversionFromALocalOriginNearTheWork`).
The file is then both georeferenced and drawable: a viewer holding MGA
coordinates in single precision would resolve them to about a metre.

Without an EPSG code the file is not georeferenced, its coordinates are the
project's own, and the report says so. A name that is not an EPSG code (a
WKT definition, a local grid) is not written at all: IFC4X3_ADD2 names a
system by its code and has no attribute for a definition, and a name no
reader can resolve is georeferencing in appearance only. A local origin
given without a system to record it in is refused.

### Alignments

Each alignment is an `IfcAlignment` with its **business logic** - an
`IfcAlignmentHorizontal` and, with a profile, an `IfcAlignmentVertical`, each
nesting one `IfcAlignmentSegment` per element with the design parameters a
designer states - and its **geometry**: an `IfcCompositeCurve` of
`IfcCurveSegment` (the `FootPrint`, or the `Axis` without a profile) and over
it an `IfcGradientCurve` (the `Axis`). Katana holds PIs and PVIs; both
descriptions are written from the solved elements, one segment per element,
so they agree with each other and with what Katana draws.

| Katana element | Business logic | Geometry |
|---|---|---|
| tangent | `LINE` | `IfcLine` along +x, from 0 for L |
| circular curve | `CIRCULARARC`, radius signed by the turn (negative: right) | `IfcCircle` of \|R\|, from 0 for L signed by the turn |
| transition | `CLOTHOID`, start and end radius (0 on the tangent side) | `IfcClothoid`, A = sign(Δk)·√(L/\|Δk\|), from k0·L/Δk |
| grade | `CONSTANTGRADIENT` | `IfcLine` |
| vertical curve | `PARABOLICARC`, radius L/(g2 − g1) | `IfcPolynomialCurve` h + g1·x + (g2 − g1)/2L·x² |

The encodings are IfcOpenShell's (`ifcopenshell.api.alignment`, 0.8), the
implementation buildingSMART's validation service evaluates geometry with.
Each layout and each curve ends with a zero-length segment, as the schema's
rules require. Stations are `IfcReferent` STATION with `Pset_Stationing`, at
the start, at every key station (TS, SC, CS, ST, PC, PT, the PVCs and PVTs)
and at the end, named "SC 1093.218" and so on; `Katana_Alignment` carries the
start and end stations and the PI and PVI counts. A profile that runs beyond
its alignment is not written, with a warning: the gradient curve cannot
extend past the curve it rests on.

On MC01 of `samples/ifc/scenario.txt` - R 80 with 20 m transitions turning
right, R 60 turning left - the tests work each parameter out from the PIs by
hand: the tangent bearings are atan2(70, 100) and −atan2(30, 100), the
deflection 0.9021827588 rad, the first arc 80 × (0.9021827588 − 40/160) =
52.1746 m, the second 60 × 0.9021827588 = 54.1310 m
(`tests/ifc/test_export.cpp`).

### Subsurface utilities (AS 5488, and the delivery schema they were written to)

Each **service** of the schedule is an `IfcDistributionSystem`, and each of
its **graded segments** - the standard grades segments, not services - one
element, grouped by the system. Every **located point** is an `IfcAnnotation`
SURVEY carrying its evidence, a member of its service's system. The class comes from the service's type and the
words of its feature, subtype, configuration, material and description
(`ifc::classifyUtilityRun`, `include/katana/ifc/classification.hpp`):

| The service | Segment class | System |
|---|---|---|
| water, recycled water, fire service, sewer, stormwater, gas, fuel | `IfcPipeSegment` RIGIDSEGMENT | WATERSUPPLY, USERDEFINED "RECYCLEDWATER", FIREPROTECTION, SEWAGE, STORMWATER, GAS, FUEL |
| electricity, communications, ITS | `IfcCableSegment` CABLESEGMENT | ELECTRICAL, COMMUNICATION, CONTROL |
| ... said to be a culvert | `IfcPipeSegment` CULVERT | the type's |
| ... a conduit or duct (not a fluid service) | `IfcCableCarrierSegment` CONDUITSEGMENT | the type's |
| ... a trough or trunking | `IfcCableCarrierSegment` CABLETRUNKINGSEGMENT | the type's |
| ... optical fibre | `IfcCableSegment` OPTICALCABLESEGMENT | the type's |
| unknown, unless its words say pipe or main | `IfcDistributionFlowElement` "UNKNOWN SERVICE" | NOTDEFINED |

What a service is laid in decides before what it carries: the sample's E1 is
"4 x 100 mm conduits", and its segments are cable carriers. A service
recorded at a single point is the feature its words name
(`ifc::classifyUtilityPoint`): a manhole, valve or meter pit, sump, chamber or
pit (`IfcDistributionChamberElement`), a hydrant (`IfcFireSuppressionTerminal`
FIREHYDRANT), a valve (`IfcValve`: STOPCOCK, AIRRELEASE, FLUSHING, ISOLATING), a
marker post (`IfcSign` MARKER), a pillar or cabinet (`IfcJunctionBox`); a point
whose words name none is the survey record it is and no more.

What each object carries:

| Property set | On | Holds |
|---|---|---|
| `AS5488_QualityLevel` | each segment | the graded level, the level claimed, whether the claim holds, what limits it, the path evidence, the plan length, the level references and centre levels at each end, whether it is drawn in 3D |
| `AS5488_LocatedPoint` | each point | the method, attained and claimed levels, why it is below its method's ceiling, levels, depth and cover |
| `AS5488_Service` | the system and its elements | type, owner, material, size, configuration, status, the length at each level |
| `Pset_Uncertainty` | segments and points | IFC's own statement of positional uncertainty: MEASUREMENT for QL-A and QL-B, INTERPRETATION for QL-C, ESTIMATE for QL-D, with the tolerance or the assessed uncertainty |
| `Pset_<class>TypeCommon`, `Pset_PipeSegmentOccurrence`, `Pset_ConstructionOccurence`, `Pset_DistributionSystemCommon` | as the schema defines them | the asset identifier as Reference, status, diameters, length, gradient and invert where known, installation date |
| the schema's own title, its words joined by '_' (`Utility_Delivery_Schema`); `Delivery_Attributes` when the schema was not given | the system, its elements and points | the delivery schema's attributes **exactly as the schedule wrote them** |

and each segment and point is classified by an `IfcClassificationReference`
"QL-A" .. "QL-D" in AS 5488.1-2019; a delivery schedule's services by their
asset type code, which is AS 5488.2-2019 Table A.4's whichever schema carried
it, so the reference is into that standard and the schema is named by the
property set its values are in. The export names no client: a schema's name
comes from its own file (`DeliverySchema::title`).

**The delivery property set carries the schedule's values, not Katana's
reading of them.** Reading a schedule turns "In service" into a status; a
deliverable written back must say "In service", or it would quietly correct
what `UTILITY CHECK` reports as wrong. So the schedule reader keeps each
interpreted cell as written, under its header (`UtilityAttributes::written`,
`UtilityVertex::written`), and that is what is written. With the schema
file, the set lists the schema's attributes in its order, each described by
the schema's label; a value from one of its lists is an
`IfcPropertyEnumeratedValue` referring to an `IfcPropertyEnumeration` of that
list, a number a number, and a value the list does not hold - E-0001's "In
service" - plain text, since an enumerated value outside its enumeration is a
schema error and the value must not be lost
(`IfcExportUtilities.TheDeliverySchemasAttributesAreWrittenAsTheScheduleWroteThem`).

**Geometry is never guessed.** A segment has an `Axis` in 3D - and a `Body`,
a swept disk of its size - only when both of its ends give the level of the
service's centre: a level recorded on the centre, or on the top or invert
with a size to bring it there. Otherwise it has a 2D `FootPrint` and nothing
else: a pipe drawn at a depth nobody measured would be taken for one
somebody had. On the sample schedule that is 6 of 10 segments - W1-2 to W1-4
and all of E1 (`IfcExportUtilities.EveryGradedSegmentIsOneElementAndOnlyThoseWithLevelsAreIn3d`).

**A services plan drawn from a schedule** (`UTILITY DRAW`,
`docs/subsurface_utilities.md`, "Drawing the services") is in the drawing,
not the schedule, and is laid out for a plan: a layer per type and quality
level, each run a polyline carrying what the grading found as `utility.*`
properties. Exported by its layers' words it would be one system per level,
and E1's conduits a cable; so an entity with `utility.line` and
`utility.type` is written as the service it belongs to. Each run is the
class the schedule's own export gives the service, from the attributes the
run carries (`ifc::classifyUtilityRun`: E1's "4 x 100 mm conduits" an
`IfcCableCarrierSegment` CONDUITSEGMENT); a line an EXPLODE made of a run is
still a run of it. A service is the runs of one line id, drawn under one
`<prefix>/<type>`, with the same attributes: two schedules drawn under one
prefix that both have an "E1" are two services, each classed by its own
attributes, and the clash of ids is said. Each located point is an
`IfcAnnotation` SURVEY of the service whose run it stands on, whatever layer
it has been moved to, at the level the grading took it at where the drawing
kept one, as the schedule's export places its points. A project's own rules
(a RULES file) still name the class of what it drew; only the defaults give
way to the service's class. Each service is one `IfcDistributionSystem`
named by its line, with the schedule export's description and LongName
(`serviceLongName`); each run and point is classified by its level in
AS 5488.1 - the classification the schedule's services use, written once for
both - and carries what the drawing kept of its grade in the same sets, a
run's `AS5488_QualityLevel` and a point's `AS5488_LocatedPoint`. What the
drawing does not keep is absent: a run's claimed level and path evidence, a
point's recorded level and depth; the level the grading took a point at is
`ServiceLevel`, since the schedule's `Level` is only a recorded one. A level
that is not one of QL-A to QL-D is said and the object written unclassified.
The drawn plan together with its schedule (UTILITIES) is each service twice,
drawn and graded: written, as asked, and said. A drawn run is one per stretch
of one level, not one per segment, so the sample drawn gives W1 three pipes
where the schedule gives five segments
(`IfcExportDrawing.ADrawnServicesPlanGoesOutByServiceAsTheScheduleWouldClassIt`,
`IfcExportDrawing.TwoServicesDrawnWithOneLineIdStayTwoServices`,
`IfcExportDrawing.AnExplodedRunStaysInItsService`,
`IfcExportDrawing.APointMovedToAnotherLayerStaysWithTheRunItStandsOn`,
`IfcExportDrawing.AProjectsRuleStillClassesADrawnRun`,
`cli.ifc_a_drawn_services_plan_goes_out_by_service`). The drawing is plan
only, so the runs have their `FootPrint`; the schedule export is the one that
draws a service in 3D where its levels allow.

### Drawing entities

An entity becomes the class of the first rule whose words its layer path,
survey code or string name (from a .12da archive) holds - whole words, any case, a trailing `*`
a prefix - among the rules that apply to its kind (`ifc::classifyEntity`,
`ifc::defaultClassificationRules`):

| Words (examples) | Kinds | Class |
|---|---|---|
| CULVERT | runs | `IfcPipeSegment` CULVERT |
| CONDUIT, DUCT | runs | `IfcCableCarrierSegment` CONDUITSEGMENT |
| STORMWATER, SW, SEWER, WATER, GAS, FUEL ... | runs | `IfcPipeSegment` RIGIDSEGMENT, in the system the words name |
| OPTIC, FIBRE | runs | `IfcCableSegment` OPTICALCABLESEGMENT |
| ELEC, POWER, HV, COMMS, TELSTRA, NBN, CABLE ... | runs | `IfcCableSegment` CABLESEGMENT |
| MANHOLE, MH; SUMP; CHAMBER; PIT, SIP, KIP, GULLY | points, circles | `IfcDistributionChamberElement` MANHOLE, SUMP, INSPECTIONCHAMBER, INSPECTIONPIT |
| HYDRANT, FH; VALVE, SV, WV, GV; MARKER | points | `IfcFireSuppressionTerminal` FIREHYDRANT; `IfcValve` ISOLATING; `IfcSign` MARKER |
| KERB, KB, LIP OF KERB ... | runs | `IfcKerb` |
| GUARDRAIL, BARRIER, W BEAM; HANDRAIL; FENCE | runs | `IfcRailing` GUARDRAIL, HANDRAIL, FENCE |
| RETAINING WALL; WALL | runs | `IfcWall` RETAININGWALL; NOTDEFINED |
| TREE, SHRUB, VEGETATION | any | `IfcGeographicElement` VEGETATION |
| BOREHOLE, BH, TEST PIT | points | `IfcBorehole` |
| SIGN | points | `IfcSign` |
| CONTOUR | runs | `IfcAnnotation` CONTOURLINE, with `Pset_AnnotationContourLine` |

Services come first - linework on a service's layer is the service - and a
pit on a "STORMWATER PITS" layer is in STORMWATER without a rule per service
(`ifc::serviceSystemFor`). What no rule names is an `IfcAnnotation` of its
kind: SURVEY for a point, or a line with heights or a survey code; TEXT,
DIMENSION, LEADER; NOTDEFINED for other linework. The rules are data: a
caller that sets `ExportOptions::rules` replaces the defaults with them, and
a project whose layers are named otherwise gives a RULES file ("Classification
rules" below), whose rules both front ends try before the defaults, which
still classify everything the file does not name
(`ifc::readExportFiles`). A rule naming a class the export does not write, or
USERDEFINED without saying what, is refused.
`IfcBuildingElementProxy` is written only when such a rule asks for it.

The geometry is what the class expects: a run's `Axis` in 3D where every
vertex has a height, else its `FootPrint`; a point feature's `FootPrint` point
(and circle); an annotation's `Annotation` point, curve or text. Arcs are
`IfcIndexedPolyCurve` arcs through three points of the true arc and circles
`IfcCircle`, never chords. The drawing system's kinds (`docs/drawing.md`)
keep their curves too: a curve polyline is one `IfcIndexedPolyCurve` of an
`IfcLineIndex` per straight segment and an `IfcArcIndex` through each arc's
true middle (its height the mean of its ends', and in 3D only when every
vertex has one); an ellipse is an `IfcEllipse` placed along its major axis,
and an elliptical arc that ellipse as an `IfcTrimmedCurve` by parameter - IFC's
conic parameter is the eccentric anomaly in radians, as Katana's is; a spline
is an `IfcBSplineCurveWithKnots` (`IfcRationalBSplineCurveWithKnots` with
weights), each distinct knot once with its multiplicity, its fit points left
out as how it was drawn rather than what it is. A curve polyline or spline is
a run to the default rules, so a kerb drawn with an arc is still an `IfcKerb`,
and a rules file's `kinds` may name `CurvePolyline`, `Ellipse` and `Spline`.
Text is `IfcTextLiteralWithExtent` (plain
`IfcTextLiteral` is deprecated in 4.3). Each entity keeps its layer as an
`IfcPresentationLayerAssignment`, its colour as a curve style, its properties
(`Katana_Attributes`) and its provenance (`Katana_Provenance`). Labels are not
written: what a label says and where it stands are the placer's, which this
layer cannot see; they are counted in the report.

**Drainage from a .12da archive** is taken as the network it is: pipe i of a drainage string
(its `pipe.<i>.*` properties) an `IfcPipeSegment` from vertex i to i + 1, its
centre at its inverts plus half its diameter where the upstream end is known -
from the string's own vertex levels, or a `flow_direction` of 1 - and in plan
where it is not; each pit an `IfcDistributionChamberElement` by its type (a
headwall USERDEFINED "HEADWALL"), from its lowest connected invert to its top;
a house connection an `IfcPipeFitting` JUNCTION; all in one
`IfcDistributionSystem` per string, STORMWATER unless its words say otherwise.

### Classification rules

A project whose layers are named otherwise than the defaults expect writes
its own rules - a CSV file, a rule a row, the columns found by name
(`ifc::parseClassificationRules`):

```
rule,words,kinds,class,predefined_type,object_type,system
light pole,POLE*;LP;LIGHTING,Point,IfcColumn,COLUMN,,
headwall,HEADWALL*;HW,Point;Circle,IfcDistributionChamberElement,USERDEFINED,HEADWALL,STORMWATER
```

Blank lines and lines that begin with '#' are skipped; a field holding ',',
'"' or '#', or blanks at either end, is double-quoted, with '"' doubled inside
it, and a quoted field ends at its closing quote - anything but blanks before
the next ',' is refused, naming the line.
`words` are separated by ';' and matched as the defaults' are; `kinds` limit
a rule to Point, Line, Arc, Polyline, Circle, Text, Dimension, Label or
Leader, and are empty for any; `system` names the distribution system an
element serves. A file's rules are tried BEFORE the defaults, so a project
names only its exceptions (`samples/ifc/classification_rules.csv`:
`cli.ifc_a_projects_rules_classify_what_the_defaults_would_not`). Everything
that would make an invalid file is refused when the file is read, naming
its line: a class the export does not write, a predefined type the class does
not have, USERDEFINED without an object type. The predefined types are the
schema's own, generated from IfcOpenShell's IFC4X3_ADD2
(`tools/ifc_product_classes.py --predefined`,
`src/katana_ifc/predefined_types.inc`), and every default rule and service
class is checked against them
(`IfcRulesFile.EveryClassTheExportChoosesIsOneTheSchemaHas`). Save Default
Rules in the export dialog, and `IFC RULES <file.csv>` on any command line,
write the defaults (`ifc::writeDefaultRules`, `ifc::formatClassificationRules`)
as a starting point.

### Surfaces

Each surface passed in is an `IfcGeographicElement` TERRAIN whose `Body` is an
`IfcTriangulatedFaceSet`, with `Katana_Surface`.

### GlobalIds, and the same file every time

Every GlobalId is made from a key that names the object - "entity/41",
"alignment/MC01/horizontal/3" - within a namespace that names the project
(`ExportOptions::guidNamespace`; both front ends pass the project's name and
creation time, `ifc::guidNamespaceFor`), by two 64-bit FNV-1a hashes mixed and encoded in IFC's base
64 (`ifc::guidFor`). The same project exported twice gives each object the
same GlobalId, so a consumer can track an object across issues; two projects
give different ones. The writer reads no clock and no random source: the same
input is the same file, byte for byte (Rule 7,
`IfcExport.TheSameInputIsTheSameFileAndGlobalIdsBelongToTheProject`); the
timestamp in the header is the caller's.

## Import: what comes in, and as what

`ifc::readIfc` parses any ISO 10303-21 file whose schema is an IFC one
(`src/katana_ifc/step_reader.cpp`: complex instances are counted and passed
over, lists deeper than 64 are refused, an error names its line), and
`ifc::importCommand` makes the result one undoable step. The product classes
of IFC2X3, IFC4 and IFC4X3_ADD2, and which are drawn, spatial or positioning,
are generated from IfcOpenShell's schemas (`tools/ifc_product_classes.py`,
`src/katana_ifc/product_classes.inc`).

| In the file | In the drawing |
|---|---|
| `IfcAlignment` | a named alignment, its PIs and PVIs **reconstructed** from the business logic and **checked** (below); else a 3D polyline of its exact geometry on "IFC/Alignments", and a warning saying why |
| an element or annotation | its shape - the `Axis`, else `FootPrint`, else `Annotation`, else a swept disk's directrix - as points, lines, arcs, circles, polylines and texts, heights kept and absent where the file has none; a shape Katana cannot draw (a solid) a point at its placement, counted |
| a terrain (`IfcGeographicElement` TERRAIN, a TIN) | a surface, for a caller that keeps surfaces |
| property sets and quantities | properties "Set/Property", lengths, areas, volumes and angles in Katana's units |
| classifications, the system served | "Classification/<system>", "System" |
| class, GlobalId, name, tag, predefined type, container | metadata `ifc.class`, `ifc.globalId` ... |
| presentation layer and colour | layer and colour; without a layer, "IFC/<container>/<class>" |
| `Katana_Attributes`, `Katana_Provenance` | where they came from, so an export read back is the drawing it was |

Lengths come in as metres from the file's unit (SI prefixes and
conversion-based units), angles as radians. A file with an `IfcMapConversion`
(or `IfcMapConversionScaled`) is read into its projected system - eastings,
northings, height, rotation and scale applied - and its EPSG code is reported
for `CRS SET`; an `IfcRigidOperation` in lengths is a shift, one in degrees
places the file in a geographic system, which only a projection could turn
into grid coordinates, so the file is read in its own coordinates and that is
said. Local, linear (`IfcLinearPlacement` along an alignment) and grid
placements are followed.

**Alignments are reconstructed, then checked.** Each run of [CLOTHOID]
CIRCULARARC [CLOTHOID] between two LINEs is one PI at the intersection of
those lines, with the arc's radius and the transitions' lengths; each
PARABOLICARC one PVI at the intersection of its grades. Katana then solves
the reconstruction, and every segment the file states must start within
`ifc::kAlignmentTolerance` (10 mm, the .12da import's tolerance) of
where the solution puts it. Where the check fails, or the geometry has no PI
form - it starts or ends on a curve, compounds two arcs, or uses a transition
other than the clothoid (Katana has the clothoid) - the alignment comes in as
a 3D polyline of its exact geometry, chorded to `ImportOptions::curveTolerance`
(transitions integrated from their curvature, which is what defines them),
and the warning says why. A vertical of circular arcs is not a Katana design
profile: its levels come in on a polyline beside the alignment. Cant is not
read; Katana's alignments have none. Stations are taken from the
`Pset_Stationing` referents.

## Validation

The files are judged by implementations that are not Katana's, so that a
mistake in the writer and the same mistake in the reader cannot agree:

- **IfcOpenShell 0.8.5** (`tools/check_ifc.py`, the `cli.ifc_the_scenario_export_is_valid_to_ifcopenshell`
  and `cli.ifc_the_delivery_export_is_valid_to_ifcopenshell` tests, registered
  where the configuring Python can import ifcopenshell): the schema - types,
  cardinalities, enumerations, inverses - and its EXPRESS rules: **0 findings**
  on the scenario with the sample services (1097 instances) and on the
  delivery-schema export (461 instances). The same tool evaluates each alignment's geometry
  with IfcOpenShell's own kernel and compares it with the business logic,
  segment by segment: on MC01 they agree to under 1 µm. Checked the other way,
  a clothoid's start direction moved by 0.01 rad and a vertical curve's
  start height by 62.5 mm are both reported.
- **buildingSMART's validation rules** (`ifc-gherkin-rules` at 893f827,
  2026-07-29, 95 rules evaluated, run by hand): no errors on either file. The
  scenario draws warnings from IFC431, "entities in scope for the
  Alignment-based view": its text is an `IfcTextLiteralWithExtent` with an
  `IfcPlanarExtent`, which that view does not list. The text is kept - a lot
  number is worth more than a clean report - and the delivery-schema export,
  services alone, draws none. GRP001 raises an exception of its own on any
  `IfcDistributionSystem` (it looks up the relationship by the exact name
  `IfcGroup`, not its subtypes); that is the rule's defect, not the file's.
- **The window's files**: the export dialog's, the typed EXPORT's and the
  surfaces round trip's (the `qt_ifc_*_headless` checks) give **0 findings**
  too - 1097, 1097 and 58 instances - and
  `qt_ifc_the_windows_export_is_valid_to_ifcopenshell` checks the first where
  IfcOpenShell is installed.
- **Import**: all 46 files of buildingSMART's IFC 4.3 sample models
  (`IFC4.3.x-sample-models` at 50e6c5c) import without error by hand; the
  railway sample's alignment is reconstructed to 4 PIs, and the UTM and
  Gauss-Krüger georeferencing samples land where IfcOpenShell's own
  conversion puts them.

## Not done

- **Scope and filter as Global Modify's** (the project's rule that every tool
  acts on the selection, a view, layers, the drawing or an area, and on the
  entities that match). EXPORT takes the selection (`SELECTED`) or the whole
  drawing, and leaves out entities, alignments or surfaces; it does not yet
  take `VIEW`, `AREA`, `LAYERS a,b [ONLY]` or `WHERE k=v`, nor does the
  export dialog carry the "Apply to" and "Only those that match" controls.
  The rule asks for one shared scope parser in katana_cad and one shared
  "Apply to" widget in the window, neither of which exists yet (2026-09-26):
  the scope and filter words are read inside
  `CommandInterpreter::modify`, and a second reading of them here is what the
  rule forbids. When the shared parser exists, both front ends resolve the
  scope with `cad::matchEntities` and pass the entities as
  `ExportOptions::entities`, which the writer already takes, and the reply
  says what the scope took.
- **Solids** are not read beyond swept disks and triangulated surfaces:
  Katana has no solid entity, so an extruded wall or a B-rep comes in as a
  point at its placement with its properties.
- **Export of 3D solids** other than swept disks (a pit as a box, a culvert as
  its section) is not attempted; a pit is its footprint and, from a .12da
  archive, a cylinder from invert to top.
- **Cant**, IFC4X1-form alignments, and transitions other than the clothoid
  have no Katana equivalent, so they come in as exact polylines.
- **Labels** are not exported (above), and the delivery schema's conditional
  attributes are not evaluated here any more than by `UTILITY CHECK`.
- **Attribute quality levels** (AS 5488's grading of the type or owner
  separately from the position) are not modelled, so none are written.
