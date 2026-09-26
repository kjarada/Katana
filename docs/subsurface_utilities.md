# Subsurface utility investigations (AS 5488)

Tools for surveying buried services and grading what is known about them by
the quality levels of AS 5488.1-2019, "Classification of subsurface utility
information". The code is `katana::survey::subsurface`
(`include/katana/survey/subsurface/`, `src/katana_survey/subsurface_*.cpp`);
the command line reaches it through `UTILITY`
(`include/katana/cad/utilities/utility_verbs.hpp`), a verb of the shared
`CommandInterpreter`, so the window's command line, `katana_cli`,
`katana_mcp` and an agent all run the same code. The calculation sits in
`katana_survey` because it is survey calculation - positions, tolerances,
lengths, levels - and needs nothing above `core` and `math`; the verbs and the
drawing they make sit in `katana_cad`, which owns the document.

The short version of the standard, as this code reads it:

| Level | How the information was obtained | Positional tolerance (default) |
|---|---|---|
| QL-D | existing records, or anecdote | none stated |
| QL-C | a surveyed surface feature (pit, valve, marker) correlated with the records | none stated |
| QL-B | detected from the surface (EML, GPR, other geophysics) and surveyed | +/-300 mm horizontal, +/-500 mm vertical where a depth is given |
| QL-A | exposed and seen (pothole, non-destructive excavation, open trench) and surveyed in 3D | +/-50 mm horizontal and vertical |

**The tolerances are data, not constants** (`QualityLevelTolerances`). The
defaults - QL-A +/-50 mm horizontal and vertical, QL-B +/-300 mm horizontal
and +/-500 mm vertical - were confirmed against AS 5488 by the project owner
on 2026-09-26. A client specification may still be tighter and a later
edition may differ, and a program that hard-coded them would certify against
the wrong numbers without anyone noticing, so they stay settable: set them
where the project specification differs.

## What each tool answers

| Question | Function | `UTILITY` |
|---|---|---|
| What quality level does this located point support? | `subsurface::classify` | `REPORT` |
| What level is each stretch of a service, and how long is each? | `subsurface::gradeLine` | `REPORT` |
| How deep is it? | `subsurface::depthOfCover`, `subsurface::topLevel` | `REPORT ... MINCOVER` |
| Does the deliverable claim more than its evidence supports? | `GradedVertex::overClaim` | `REPORT` findings |
| Can we build here? | `subsurface::checkClearance` | `CLEARANCE` |
| Were the QL-B detections as good as the locator said? | `subsurface::verifyDetections` | `VERIFY` |
| Does the deliverable meet the client's schema? | `subsurface::checkDelivery` | `CHECK ... SCHEMA` |
| What should the schema's Clash attribute say? | `subsurface::clashOf` | `CLEARANCE` |
| Where is each stretch, at what level, on the plan? | `utilities::drawUtilities` | `DRAW` |
| Which services are the survey's, or an import's, lines and points? | `utilities::readGeometryServices` | `DRAW <scope>` |
| What does it grade as now that it has been edited in CAD? | `utilities::planUtilityRegrade` | `REGRADE` |
| What does the drawing deliver, as a schedule? | `subsurface::writeUtilityCsv` | `SCHEDULE` |

Every question is asked of a schedule file or of what is drawn, chosen by
the scope and filter words every verb shares ("Drawing data", below) - the
drawing of the services too, which takes the survey's or an import's own
lines and points ("Services from surveyed and imported geometry", below).

## Decisions, and where this is stricter than the standard

**A level is decided by the method first and the tolerance second.**
`maximumQualityLevel` caps every record by how it was obtained: no stated
accuracy makes a radar pick QL-A, because QL-A means the service was seen.
Only then are the uncertainties tested (`subsurface::classify`).

**An uncertainty nobody assessed is not assumed good.** A detection without a
horizontal uncertainty is QL-C, not QL-B. The alternative - trusting the
method's nominal accuracy - is exactly the over-claim the classification
exists to prevent. A negative or non-finite uncertainty is a broken record
and is treated the same way.

**QL-A is three-dimensional.** An exposure with no level, or a level outside
tolerance, is QL-B in plan (`Classification::reasons` says why).

**A plan position and a level are graded separately.** A QL-B detection
whose depth estimate is outside +/-500 mm keeps its QL-B plan position and
has `Classification::levelQualified` false: its depth of cover is reported
with a note not to rely on it, and clearance never uses it vertically.

**A segment is never better than the worse of its two ends, nor than what
was observed between them.** This is where most over-claiming happens: a line
drawn through two potholes is not QL-A between them, because nobody saw the
service there. So `PathEvidence` records what is known of each segment:

- `Exposed` (an open trench) keeps the level of its ends, and is the only way
  a segment is QL-A;
- `Detected` is capped at QL-B, and at QL-C when the segment is longer than
  `GradingSettings::maximumDetectedSpacing` (default 10 m) - a line
  interpolated across a longer gap was not traced, it was assumed;
- `Assumed` is capped at QL-C.

The maximum detected spacing is the project's to set, not the standard's;
infinity switches the rule off.

**Cover is to the top of the service, and errs towards less cover.** A level
may be recorded on the top, the centre or the invert (`LevelReference`); a
level with no reference is read as the top. From an invert the whole
diameter is added, ignoring the wall, which puts the top slightly high and
so under-states cover rather than over-stating it.

**Clearance has four answers, not two** (`ClearanceStatus`). The service is
widened by its level's horizontal tolerance: clear across the whole
tolerance is `Clear`, clear only at the drawn position is `WithinTolerance`,
not clear even there is `Conflict`. A QL-C or QL-D position is not a
measurement, so near the works (within the required clearance plus
`ClearanceRequirement::unverifiedMargin`, default 2 m) it is `Unconfirmed`
rather than either, and the report lists where to pothole next. Where the
plan clearance fails and both the works and the service have qualified
levels, vertical separation - also widened by its tolerance - can clear a
crossing. The required clearances themselves come from the asset owners,
not from AS 5488, which classifies information and sets no separations.

Results are per segment rather than per service. The first version kept only
each service's worst segment, and on the sample schedule that hid a water
main's actual crossing of the design (QL-B, within tolerance) behind a QL-C
tail two metres away. The designer needs both.

**Verification compares like with like.** An exposure names the detection it
checks (`UtilityVertex::verifies`); both levels are brought to the top of the
service first, so a detection recorded to the centre and an exposure recorded
to the crown do not differ by half a diameter that is not error. An exposure
that is not itself QL-A checks nothing and is reported as unused.

## The schedule format

`subsurface::parseUtilityCsv` reads one row per located vertex; rows with the
same `line` form one service in file order. Columns are found by name from a
header row, in any order and letter case, with aliases
(`subsurface::utilityCsvColumns`). Required: `line`, `point`, `easting`,
`northing`, `method`. Optional: `level`, `level_ref`, `surface`, `h_unc`,
`v_unc`, `ql` (the level the deliverable claims), `path` (evidence to the
next vertex), `verifies`, and the service's attributes `type`, `owner`,
`material`, `diameter_mm`, `status`, `config`, `description`.

It refuses rather than guesses: an unknown column (a misspelt `survace` read
past would leave every cover uncomputed), `x` and `y` for the coordinates
(which is which differs between conventions - the same rule as
`include/katana/surveyio/delimited_points.hpp`), an unparseable value, and
two rows of one service that give different attributes. Diameter is the one
column with its unit in its name, because a diameter in a utility schedule is
millimetres by habit and metres everywhere else in this program. An empty
cell is "not recorded", never zero.

What is interpreted is also kept as it was written, under the header it was
written under (`UtilityAttributes::written`, `UtilityVertex::written`):
"In service" is `UtilityStatus::InService` to the grading, and still "In
service" to a deliverable written back out - the IFC export's delivery
property set (`docs/ifc.md`) carries the schedule's own values, so it cannot
quietly correct what `CHECK` reports as wrong.

`samples/utilities/schedule.csv` and `samples/utilities/design.csv` are a
worked example: a water main verified by a pothole, an electricity duct bank
with one weak radar pick, a telecommunications run between pits and a gas
main from the records, crossed by a proposed stormwater pipe.

```
katana_cli -c "UTILITY REPORT samples/utilities/schedule.csv MINCOVER 0.6"
katana_cli -c "UTILITY VERIFY samples/utilities/schedule.csv"
katana_cli -c "UTILITY CLEARANCE samples/utilities/schedule.csv samples/utilities/design.csv WIDTH 0.375"
katana_cli -c "UTILITY DRAW samples/utilities/schedule.csv" -c "LAYER LIST"
```

`samples/utilities/located_services.geojson` is the same investigation as a
GIS delivers it: plan strings and the points shot on the ground, with the
services' and the points' attributes under the GIS's own names. Imported and
drawn, it draws what the schedule draws, figure for figure
(`cli.utility_draw_takes_the_services_an_import_left_in_the_drawing`):

```
katana_cli -c "IMPORT samples/utilities/located_services.geojson" -c "UTILITY DRAW DRAWING FIELDS line=ASSET_ID,point=PT_ID,type=ASSET_TYPE,owner=OWNER,material=MATERIAL,diameter_mm=DIA_MM,status=STATUS,config=CONFIG,description=DESC,method=METHOD,h_unc=H_UNC,v_unc=V_UNC,ql=QL,level=LEVEL,level_ref=LEVEL_REF,path=PATH,verifies=VERIFIES" -c "UTILITY REPORT LAYERS utilities MINCOVER 0.6"
```

An investigation is delivered as IFC 4.3 with `EXPORT <file.ifc> UTILITIES
<schedule.csv> [SCHEMA <schema.csv>]`: each service an
`IfcDistributionSystem`, each graded segment the pipe, cable or conduit
element its type and feature make it, carrying its quality level, and each
located point an `IfcAnnotation` carrying its evidence (`docs/ifc.md`,
"Subsurface utilities").

## UTILITY on the command line

`UTILITY` is the interpreter's (`utilities::runUtilityVerb`), so it is typed
the same at the window's command line, in a `katana_cli` script and in a
`katana_mcp` batch. Words are case-insensitive and a path with blanks is
quoted. `HELP UTILITY` lists every option.

```
UTILITY REPORT    <schedule.csv> | <scope> [MINCOVER <m>] [SPACING <m>]
UTILITY VERIFY    <schedule.csv> | <scope>
UTILITY CLEARANCE <schedule.csv> | <scope> [DESIGN] <design.csv> | #<id> [LEVEL <z>] | ALIGNMENT <name>
                  [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
UTILITY CHECK     <schedule.csv> | <scope> SCHEMA <schema.csv>
UTILITY DRAW      <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
UTILITY DRAW      <scope> [TYPE <type>] [METHOD <method>] [H_UNC <m>] [V_UNC <m>]
                  [HEIGHTS surface|service|none] [LEVEL_REF top|centre|invert]
                  [PATH detected|exposed|assumed] [OWNER <text>] [MATERIAL <text>]
                  [DIAMETER_MM <mm>] [STATUS <status>] [FIELDS column=property,...]
                  [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
UTILITY REGRADE   <scope> [SPACING <m>] [MINCOVER <m>]
UTILITY SCHEDULE  <out.csv> <scope> [SCHEMA <schema.csv>]
```

`<scope>` is the scope and filter `MODIFY` takes, read by the one shared
parser (`docs/cad.md`, "Scope and filter") that every new verb on drawing
data is to take: `SELECTION`, `DRAWING`, `VIEW [id] [EXTENTS]`, `AREA
x0,y0,x1,y1` or `LAYERS a,b [ONLY]`, then `[WHERE key=value ...]`. Unlike
`MODIFY`, which takes the selection when no scope word is given, a `UTILITY`
verb needs a scope word or `WHERE` to read the drawing at all. `VIEW` is the window's plan view as it is on screen,
`EXTENTS` its layers anywhere; `katana_cli` and `katana_mcp` have no view and
take `AREA` instead. A first word that is one of those, or `WHERE`, takes the services
drawn in the document ("Drawing data", below) - for `DRAW`, the lines,
polylines and points a survey or an import left there, to draw as services
("Services from surveyed and imported geometry", below); anything else is
the path of a schedule, as before - a file named like a scope word is given
with its directory (`./drawing`).

`REPORT`, `VERIFY`, `CLEARANCE`, `CHECK` and `SCHEDULE` read and reply; they
never touch the drawing, so they are safe against any open project.
`SPACING` is `GradingSettings::maximumDetectedSpacing` (default 10 m) for
`REPORT`, `DRAW` and `REGRADE`, so a drawing and a report of one schedule
agree. `CHECK` with errors against the schema is refused, and the refusal
carries the whole check - a script that stops must still say why. `katana_cli`
prints the refusal's first line on stderr, as every refusal, and the check
after it on stdout, where a report goes, so `katana_cli -c "UTILITY CHECK
..." > check.txt` keeps the check exactly when it failed; and exits 1. An option
that is not the verb's, one given twice, or a negative or missing number is
refused by name (`LEVEL` alone may be negative: a level below the datum); so
is a file that cannot be read or parsed, with its path.

The proposed works `CLEARANCE` measures against are a design centre line
from one of three places. `DESIGN` may be left out, since the design file's
second place is what the verb always took:

- **A design file** (`parseDesignCsv`), as before.
- **An entity drawn as the centre line**, `#<id>`: a line or a polyline (a
  closed one returns to its start; anything else is refused by its kind).
  Its levels are `LEVEL`'s at every vertex when given, else the entity's own
  heights (`entity::heightsOf`, the `elevation`/`elevations` a 3D string
  carries), else none - and a design with no levels is cleared in plan only,
  as a design file without a level column is. `LEVEL` with a file or an
  alignment is refused: they carry their own.
- **A document alignment**, `ALIGNMENT <name>` - one made with the `ALIGN`
  verb (`docs/cad.md`); the word is `ALIGNMENT`, not `ALIGN`: its
  horizontal geometry chorded to 1 mm and its design profile's levels, where
  it has one (`utilities::designFromAlignment`). The stations are each
  element's ends, as many along a curve as the one sagitta rule
  (`geometry::sagittaChordCount`) asks for 1 mm, every key station of the
  profile, and enough inside each vertical curve that the straight line
  between two samples is within 1 mm of the parabola: a chord of length l on
  a curve whose grade changes by A over L departs from it by at most
  A l^2 / (8 L), so l = sqrt(8 L x 0.001 / A). A millimetre because the
  clearance report prints millimetres. Stations the profile does not reach
  have no level. `UtilityData.AnAlignmentsDesignIsWithinAMillimetreOfItsCurvesInPlanAndInLevel`
  holds every chord to it, on an alignment with a curve and a crest. The
  design always runs from the alignment's start to its end: a station a
  rounding outside them is held to the nearer end, not dropped. The last
  element's end is the elements' lengths summed from the start chainage,
  which can come out one unit in the last place past `endStation()`; dropping
  it once dropped the whole last tangent, and a hard conflict on it was
  reported clear (in about 2 % of start chainages, swept by the reviewer).
  `UtilityData.AnAlignmentsDesignEndsAtItsEndWhenItsLengthsSumARoundingPastIt`
  reproduces it at chainage 43.45.

## Drawing the services

`UTILITY DRAW` grades the schedule as `REPORT` does and adds it to the drawing
as ONE undo step named `UTILITY DRAW` (`utilities::utilityDrawCommand`). It is
all or nothing: a line that cannot be graded - one located vertex, a
coordinate that is not a number - refuses the whole draw, naming the line,
and nothing is added. `utilities::drawUtilities` builds the drawing as values
with no document, which is what `tests/cad/utilities/test_utility_drawing.cpp`
tests; the command then creates what the drawing lacks, in this order:

- **Linetypes** by quality level, named `utility-ql-b`, `utility-ql-c` and
  `utility-ql-d`, when a layer about to be made names one. Model metres, for
  plans at 1:200 to 1:500, so the level survives a monochrome plot:

  | Level | Linetype | Pattern (m) | At 1:500 on paper |
  |---|---|---|---|
  | QL-A | `continuous` | solid | solid |
  | QL-B | `utility-ql-b` | 1.5 dash, 0.75 gap | 3 mm dashes |
  | QL-C | `utility-ql-c` | 1.5 dash, 0.5 gap, dot, 0.5 gap | dash-dot |
  | QL-D | `utility-ql-d` | dot, 0.6 gap | a dot every 1.2 mm |

- **Layers** `<prefix>/<type>/QL-A` .. `QL-D` (those used) and
  `<prefix>/<type>/points`, and their parents, parents first - so undo takes
  every one back. The prefix is `utilities` unless `LAYER` names another
  (`LAYER "Site Services/Located"`); one too deep or too long for the two
  levels drawn under it is refused as the prefix, before anything is made.
  `<type>` is one word per kind of service:
  `water`, `electricity`, `telecommunications`, `gas`, `recycled-water`,
  `fire-service`, `sewer`, `stormwater`, `fuel`, `its`, `other`, `unknown`
  (`utilities::utilityTypeWord`). A QL layer takes its level's linetype; every
  layer of a type, and the `<prefix>/<type>` group, takes the type's colour. A
  layer that already exists is used as it is: its colour, linetype and lock
  are the person's. A locked one refuses the draw when it runs, as any edit
  on a locked layer is refused.
- **Entities**, ByLayer: one polyline per maximal run of consecutive segments
  graded at the same level, on that level's layer - so a polyline ends
  exactly where the level changes (on the sample, W1 is three: QL-B to the
  pothole, the QL-A trench, then QL-C past the detected spacing) - and one
  point per located vertex on the points layer. A run of no plan length to
  the model's tolerance (two records at one place, or a rounding error apart,
  graded unlike their neighbours) has nothing to draw; its points are still
  drawn.

**The colours are Katana's defaults, not the standard's.** AS 5488
classifies information and sets no colours. The defaults follow the colours
services are commonly marked in, and their pipes and conduits made in, in
Australia, adjusted where the marking colour would vanish on white paper;
each is at least 3:1 against the plan view's ground and, as it prints,
against white paper
(`UtilityDrawing.EveryTypeColourReadsOnTheScreenAndOnPaper`). They are layer
colours, so a project with its own convention changes the layers after a
draw - `MODIFY LAYERS utilities/water/QL-B SET LAYER.COLOUR=#RRGGBB`, or the
layer panel - or makes them first, `LAYER NEW utilities/water/QL-B #RRGGBB`,
since a draw uses a layer that exists as it is. (`LAYER` itself has no action
that recolours a layer, and `LAYER NEW` refuses one that exists.)

| Type | Default | Common marking colour |
|---|---|---|
| water | `#2F80ED` blue | blue |
| electricity | `#E07000` orange | orange |
| telecommunications | `#FFFFFF` white, which prints black (`PlotSettings::whiteToBlack`) | white |
| gas | `#B8860B` dark gold | yellow, which vanishes on paper |
| recycled-water | `#9B59D0` lilac | lilac |
| fire-service | `#E53935` red | red |
| sewer | `#A0896B` dark cream | cream, which vanishes on paper |
| stormwater | `#43A047` green | none in common use: Katana's choice |
| fuel | `#B06030` brown | Katana's choice |
| its | `#009EB0` cyan | Katana's choice |
| other | `#949494` grey | Katana's choice |
| unknown | `#D63AD6` magenta | Katana's choice, to stand out: nobody established what it is |

**What the grading found is on the entities**, as `utility.*` properties
(`utilities::keys`), so the property panel, `PROP LIST` or an agent can ask
why a stretch is QL-C without running the report again. A property that was
not recorded, or cannot be computed, is absent - absent is not zero - and
that includes `utility.status`, which a schedule without one leaves off, as
the report lists it missing. Two are always there: `utility.type`, which
names the layer (`unknown` when not recorded), and `utility.level_ref` beside
a level or a depth, the part of the service it is on - `top` for a level
given with no reference, as the grading and the cover read it, so it does
not say whether the reference was recorded. A reference recorded as
`unknown` is kept even with no level, since it is what the schedule said.

**Each point carries its whole row of the schedule** - its place along the
line, its recorded level and depth as well as the service level made of
them, its uncertainties, its method, claim, what it verifies, the path to
the next point and its schema fields - and its line's attributes too. So the
points alone are the schedule: "Drawing data", below, reads them back, and
the runs are derived output that `UTILITY REGRADE` draws again.

| On | Properties |
|---|---|
| each polyline | `utility.line`, `utility.type`, `utility.quality_level` (the run's), `utility.limited_by` (why the run is below its ends, each reason once), `utility.length` (the run's plan length), `utility.from` and `utility.to` (its end vertices), `utility.owner`, `utility.material`, `utility.diameter` (metres) and `utility.diameter_inside`, `utility.configuration`, `utility.description`, `utility.status` |
| each point: the row | `utility.line`, `utility.vertex`, `utility.order` (its place along the line, from 1), `utility.method`, `utility.level` and `utility.depth` (as recorded), `utility.level_ref`, `utility.surface_level`, `utility.h_unc` and `utility.v_unc`, `utility.claimed`, `utility.verifies`, `utility.path` (to the next point: on every point but the last when the schedule gave a path for any stretch of the line, else on none), and the line's `utility.type`, `utility.owner`, `utility.material`, `utility.diameter`, `utility.diameter_inside`, `utility.configuration`, `utility.description`, `utility.status` |
| each point: the grading | `utility.quality_level` (the vertex's graded level), `utility.over_claim`, `utility.service_level`, `utility.level_qualified`, `utility.cover` with `utility.cover_note`, `utility.cover_below_minimum` (only with `MINCOVER`) |
| each point: the settings | `utility.spacing`, the `SPACING` it was graded with (metres; absent only when the rule was switched off through the API), and `utility.min_cover`, the `MINCOVER` (absent when none was given): what `REGRADE` grades it with again unless told otherwise |
| both | a delivery schema's own attributes, uninterpreted, as `utility.field.<name>`: a polyline its line's, a point its line's and its own. Which is which is the schedule format's to say (`subsurface::utilityCsvColumns`, `CarriedOn`), as it said when the schedule was read |
| both | a cell the reader reads as "not recorded", or in part, as the schedule wrote it, as `utility.recorded.<column>`: the line's `type` ("Not Specified", "N"), `status` ("Unknown") and `size` ("Not Applicable", "1200 x 900", a size beside `diameter_mm`) on its polylines and points; a vertex's `ql` ("Unknown") and `level_ref` (given with nothing measured on it) on its point. See `UTILITY CHECK <scope>`, below |

The reply is one record a line: the drawing, then each service.

```
utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=334000.000,6250000.000,334040.000,6250007.200
line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 ql_d=0.000
line id=E1 type=electricity length=27.001 ql_a=0.000 ql_b=9.001 ql_c=18.001 ql_d=0.000
line id=T1 type=telecommunications length=33.000 ql_a=0.000 ql_b=0.000 ql_c=33.000 ql_d=0.000
line id=G1 type=gas length=40.000 ql_a=0.000 ql_b=0.000 ql_c=0.000 ql_d=40.000
```

`layers=` counts the layers that hold what was drawn, made or reused, not the
groups above them; `bounds=` is the box of every located vertex, for a front
end to frame - survey data in a real coordinate system usually lands far
from what is on the screen. `utilities::drawReplyBounds` reads it back from
the reply, so every front end reads it the same way. `cli.utility_draw_adds_the_graded_sample_to_the_drawing`
and `tests/cad/utilities/test_utility_verbs.cpp` run it on the sample.

## Drawing data: the verbs on what is drawn

The owner asked on 2026-09-26 for "the utility tools to act on data on view,
layer/s, elements, filtered elements, like global change". `REPORT`,
`VERIFY`, `CLEARANCE` and `CHECK` take the services drawn in the document, by
the scope and filter words Global Modify takes (`docs/cad.md`, "Scope and
filter"), as well as a schedule file; `REGRADE` and `SCHEDULE` take only
what is drawn, and `DRAW` a file or the survey's own geometry (the next
section). The file stays a source beside the drawing: a delivered
schedule is still checked before anyone draws it.

`utilities::readUtilityData` (`include/katana/cad/utilities/utility_data.hpp`)
reads the scope's services from the drawn points:

- **A scope that takes part of a line takes the whole line**, because the
  grading is per line: a run of it, one of its points, or anything else
  carrying its `utility.line` brings in every point of it in the drawing.
  The reply's first record says so, after the scope's own keys:
  `scope=layers layers=utilities/water/QL-B sublayers=yes matched=1 lines=1
  completed=1 ignored=0` - `completed` counts the lines read whole from
  beyond the scope, `ignored` what the scope took that carries no utility
  data. A scope that takes nothing is reported, not refused ("no utility
  lines in the scope: nothing to report").
- **The services are in the order drawn**, by their first point's id -
  schedule order for services one `DRAW` made; the points of each by
  `utility.order`. That is what makes the first proof below hold.
- **A line whose points do not agree is refused by name**, and nothing is
  guessed: two points at one `utility.order` (a copy of a point, or a
  schedule drawn twice), a point missing its order, vertex id or method, a
  method, level reference, claim or path that does not read, a number that
  is not one, a depth above the surface, two points of one line that give its
  owner (or any line attribute) differently, or a line with its runs left and
  no points. A guess at which point is right would be a schedule nobody
  wrote.
- **A point that lost its line is refused, not left out.** A point with a
  `utility.vertex` or a `utility.order` but no `utility.line` (deleted or
  emptied by hand) is a vertex of some line; read without it, that line
  would be graded, redrawn and written a vertex short, with no word said. It
  is refused by its point id and entity id, naming the lines whose attributes
  it carries - when the scope takes it, and when the scope takes a line it
  may be a point of, wherever it is
  (`UtilityData.APointWhoseLineWasDeletedRefusesItsLineRatherThanLeavingItShort`).
  To make it a point of no line, delete its `utility.vertex` and
  `utility.order` too. Values a person types in the property panel read as the schedule
  reads them: `EML` is a method, `19` is a level, `2.5` a place between the
  second and third points, `trench` an exposed path - through the schedule
  reader's own `subsurface::parsePathEvidence`; until 2026-09-26 the drawing
  had a second reading of a path that knew only the three words it writes,
  and refused `trench`
  (`UtilityData.APathTypedOnAPointReadsAsTheSchedulesPathColumnReadsIt`).

Three proofs hold the drawing to the schedule, each a test in
`tests/cad/utilities/test_utility_data.cpp`:

1. **A schedule drawn and reported from the drawing is the schedule's
   report**, word for word after the scope record, for both samples and with
   `MINCOVER` and `SPACING` (`UtilityData.TheReportOfTheDrawingIsTheReportOfTheScheduleItWasDrawnFrom`);
   `VERIFY` and `CLEARANCE` likewise.
2. **The schedule written from the drawing reads back as the lines drawn**,
   field for field and doubles to the bit - `UtilityLine`'s `==`
   (`UtilityData.AScheduleWrittenFromTheDrawingReadsBackAsTheLinesDrawn`).
3. **A point moved or a method edited, then `REGRADE`, draws exactly what a
   `DRAW` of the schedule so edited draws**, entity for entity, and one undo
   puts every entity back, ids and all
   (`UtilityData.RegradeAfterAMovedPointDrawsWhatADrawOfTheScheduleSoEditedDraws`).

**`UTILITY REGRADE <scope> [SPACING <m>] [MINCOVER <m>]`** grades the lines in
scope again from their points as they are now and replaces their runs and
their points' graded properties, as ONE undo step named `UTILITY REGRADE`
(`utilities::planUtilityRegrade`). A point keeps its id and everything that
is a person's - their own properties (another `utility.` one too), style,
colour; its `utility.` properties are those a `DRAW` of its row writes. A
line whose runs come out the same is left alone, and a regrade that changes
nothing pushes no undo step. Each line is drawn under the prefix it was
drawn under, read from its points' layer `<prefix>/<type>/points`, else its
runs'; a point on its type's points layer follows an edited type to the new
type's layer, one a person put elsewhere stays there. It is all or nothing:
a line that cannot be graded, or a run on a locked layer, refuses the whole
regrade and nothing changes. The reply is `DRAW`'s records, the first
`utilities regraded changed=<lines changed> ... bounds=`, which
`utilities::drawReplyBounds` reads as it reads a draw's, then the scope
record.

`SPACING` and `MINCOVER` left out are **each line's own, as it was drawn**
(`utility.spacing`, `utility.min_cover` on its points), not the defaults: a
regrade after one point is moved changes that line only, not every other
line in scope drawn at another spacing, and does not strip the cover flags
of a drawing made with `MINCOVER`. Given, they grade every line in scope and
are stored on its points for the next regrade. The points of one line must
agree on them, as on the line's attributes. To drop a `MINCOVER` a drawing
was made with, take `utility.min_cover` off its points (`MODIFY ... SET
UNPROP=utility.min_cover`) and regrade
(`UtilityData.RegradeGradesEachLineWithTheSettingsItWasDrawnWithUnlessToldOtherwise`).

**`UTILITY SCHEDULE <out.csv> <scope> [SCHEMA <schema.csv>]`** writes the lines
in scope as a schedule (`subsurface::writeUtilityCsv`), so a drawing edited
in CAD becomes a deliverable again: one row per point, the line's attributes
on every row, each number in the shortest text that reads back exactly
(`core::formatExactReal`), a diameter in the millimetres that divide back to
it, and a column only when some row has a value for it. It overwrites the
file, as every export does headless. With `SCHEMA`, it writes in the
client's words (`subsurface::utilityCsvDialect`): each column under the
schema attribute that is one of its names (`line` as `AssetIdentifier`,
`method` as `LocateMethod`), each word value in the first spelling the
schema's domain lists for it (`in service` as `In Service`, QL-B as
`Quality Level B`). A value that cannot be written - a line break in a text,
a kept field no column carries - is refused by name, so a written schedule
always reads back. The reply is `utilities scheduled path=<out.csv>
lines=<n> vertices=<n>`, then the scope record.

**`UTILITY CHECK <scope> SCHEMA <schema.csv>`** checks the drawing as the
schedule `SCHEDULE ... SCHEMA` would write from it: what the drawing would
deliver. So a spelling the file got wrong and the drawing holds as a value -
`In service` for `In Service` - is not found in the drawing, and a value the
schema has no spelling for is written in Katana's own, for the check to
find. The line numbers in its findings are those of that written schedule;
`SCHEDULE` with the same schema writes it for reading beside the check.
(Rejected: keeping each cell's original text on the points. It would go
stale at the first edit in CAD, and the check would then pass a value the
drawing no longer holds.)

What the reader reads as **not recorded**, though, is kept as the schedule
wrote it (`UtilityAttributes::recorded`, `UtilityVertex::recorded`,
`utility.recorded.<column>` on the drawing): a claim of `Unknown`, a
`DepthLocation` with no level or depth on it, a type `Not Specified` or `N`,
a status `Unknown`, a size `Not Applicable` or `Unknown`, a `W x H` size (the
larger side is the diameter), and a size given beside `diameter_mm` (which
the reader takes over it). The model has no value for these to hold, so
without them a schedule that met a schema drew into a drawing whose
`CHECK` found mandatory columns missing, and `SCHEDULE ... SCHEMA` wrote a
deliverable without them. A spelling is not kept - `live` is `in service`,
written as the schema spells it - only what would otherwise be said as
nothing, or as less. And each is written back only while it still reads as
the value held (`Unknown` while there is no claim, `1200 x 900` while the
diameter is 1.2 m inside): an edit made since wins, which is what keeps it
from the staleness above
(`UtilityData.WhatTheScheduleSaidItDidNotKnowIsKeptSoTheDrawingMeetsTheSameSchema`,
`SubsurfaceScheduleWriter.CellsTheReaderReadsAsNotRecordedAreWrittenAsTheScheduleWroteThem`).

A drawn plan exported to IFC (`EXPORT <file.ifc>`, File > Export IFC) goes
out by service, not by layer: each run the class the schedule's own export
gives it, each point a survey annotation, one system a service, each
classified by its quality level (`docs/ifc.md`, "Subsurface utilities").

## Services from surveyed and imported geometry

The owner, on 2026-09-26: "the utility tools only act on a csv schedule, i want
it to act on the data on the program, normally we survey the data by gps,
total station, etc, we import it into the software then we start the post
processing"; and the data also comes by an ordinary import - a `.12da`
archive, IFC, a shapefile, DXF. Until then a service reached the drawing only through a schedule file,
so a survey had to be typed into a `.csv` before any of this could grade it.

**`UTILITY DRAW <scope>` draws the geometry in scope as services**
(`utilities::readGeometryServices`, `utilities::drawGeometryServices`,
`include/katana/cad/utilities/utility_data.hpp`), exactly as a `DRAW` of the
schedule that says the same thing draws it: the same layers, runs and points,
the points carrying the same rows. So everything after it - `REPORT`,
`VERIFY`, `CLEARANCE`, `CHECK`, `REGRADE`, `SCHEDULE`, the IFC export - works on
a survey as on a schedule, with no second path through any of them. It takes
what any import leaves, whoever made it: a coded survey's strings (a polyline
carrying its string name in `code`, `docs/survey_coding.md`) and points (a
point carrying its number in `point`, `docs/survey.md`), a `.12da` archive's
strings (named in `12d.name`), a DXF's or a shapefile's lines, an IFC file's
pipes - heights and all, since every import writes them one way
(`entity::setHeights`).

- **A service is each line or open polyline in scope**, its vertices the
  located vertices in order. A closed polyline is an outline - a pit, a
  building, a parcel - not a run, and is counted `ignored`, as are arcs,
  circles, texts and the rest.
- **Its id is its name**: the first code or name survey coding finds on it
  (`cad::codePropertyCandidates`), else `#<entity id>`. A name two sources in
  the draw share, or a service drawn already has, is told apart as
  `<name>#<entity id>`, since a line id is what holds a service's points
  together.
- **A point in scope on a vertex** - within `tolerance::kCoordinate`, the one
  ground mark - gives it its point number as the vertex id (else
  `<line>-<n>`) and whatever the point says of itself. Two points on one
  vertex are refused by name: which is the survey's is not guessed. A point
  on no vertex of a line taken is not a service on its own, and is counted
  `loose`.
- **Each value is the most particular said**: the point's own `utility.*`
  property (the keys a drawn point carries, "What the grading found is on
  the entities" above), then the line's, then the verb's option. The line's
  attributes - type, owner, material, diameter, status, configuration,
  description - are the line's and the options' alone, never a point's. A
  line may say a method, uncertainties, level reference, claim, path and
  depth for all its vertices; a depth on a record of a main "laid at 0.9 m"
  is the depth all along it. The utility properties are set the one way any
  property is: `MODIFY <scope> SET PROP=utility.method:pothole`, Format >
  Global Modify, or the property panel - so the potholes of a survey are
  selected and marked, and the rest of the string takes `METHOD EML`.
- **A height is the surface unless something says otherwise.** Most located
  services are marked on the ground and shot there; `HEIGHTS service` says
  the heights are the service's own, on its level reference (a pothole shot
  on the pipe, an as-constructed or design model), and `HEIGHTS none` that
  they are not to be used. `utility.heights` on a line or a point says it
  for that one - a string shot on the ground whose potholes were shot on the
  pipe. The default errs the safe way: a service level read as the surface
  leaves the vertex with no level - QL-B at best, no cover computed - where
  a surface read as the service would certify a measured level at the
  ground and a cover of nothing. A height is the line's at the vertex, else
  its point's; a level the point gives itself (`utility.level`,
  `utility.surface_level`, `utility.depth`) wins over either reading.
- **Nothing is assumed.** No method at a vertex - none on its point, its
  line or the verb - refuses the draw, naming the vertex: how a service was
  located is the whole of its quality level, so no default could be right.
  No uncertainty is "not assessed", which grades a detection QL-C, as a
  schedule's empty cell does. No type is `unknown`, drawn magenta. A
  GNSS or total station precision on an imported point is not taken as the
  service's uncertainty: it is the mark's, and the locator's error is most
  of the service's.
- **`FIELDS column=property,...` reads an import's own attributes as the
  schedule's columns**: `FIELDS type=ASSET_TYPE,line=ASSET_ID` reads a
  shapefile's `ASSET_TYPE` as each service's type and its `ASSET_ID` as its
  id. A column is named as a schedule's header names it, aliases and all
  (`subsurface::utilityCsvColumnNamed`, the schedule reader's own lookup), so
  `LocateMethod=LOC_METHOD` and a delivery schema's own attributes
  (`AssetFeature=FEAT`) read as they would in a schedule; `diameter_mm` is
  millimetres, as the schedule's is. A property a person set in Katana
  (`utility.owner`) wins over a field read so. `easting` and `northing` are
  refused (the geometry is where the service is), and so is `size`, whose
  `W x H` reading is the schedule reader's alone.
- **What is drawn is not drawn again.** Each point a draw from the drawing
  makes carries the entity its line was read from, `utility.source` =
  `#<id>`, which `REGRADE` keeps as it keeps a person's own properties. A
  source that a point names, a drawn service's runs and points, and a
  survey point on a vertex of a source drawn before are all counted `drawn`
  and left alone; so the same draw run again after more of the site is
  surveyed draws only what is new. A scope with nothing left to draw says
  so, and is no failure.
- **The survey is left as it is.** The services are drawn beside it, under
  `utilities/` (or `LAYER`), as a schedule's are: the survey is the record
  of what was measured, and a draw that rewrote it could not be undone by
  anyone who later deleted the drawn services.

The reply is the draw's, with what the scope took after its first record, as
a regrade's is - so `utilities::drawReplyBounds` frames it the same way:

```
utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=334000.000,6250000.000,334040.000,6250007.200
scope=drawing matched=18 lines=4 points=14 loose=0 drawn=0 ignored=0
line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 ql_d=0.000
```

`points` counts the points that gave a vertex its number and values, `loose`
the points on no line taken, `drawn` what is drawn already, `ignored` what
cannot be a service. A `REPORT`, `VERIFY`, `CLEARANCE`, `CHECK`, `REGRADE`
or `SCHEDULE` whose scope takes only such geometry - the survey, not yet
drawn as services - says that `UTILITY DRAW` with the same scope draws it.

Held by `tests/cad/utilities/test_utility_geometry.cpp`: a survey read as
services is, field for field, the schedule written by hand from the same
field book, and drawn it is that schedule's drawing, entity for entity
(`UtilityGeometry.ASurveyDrawnAsServicesDrawsWhatTheScheduleOfTheSameSurveyDraws`);
every other verb then reads it as that schedule's drawing, and a regrade of it
changes nothing
(`UtilityGeometry.EveryVerbReadsWhatWasDrawnFromTheSurveyAsTheSchedulesDrawing`).
Through a real import, `located_services.geojson` draws and reports what
`schedule.csv` does, word for word
(`cli.utility_draw_takes_the_services_an_import_left_in_the_drawing`).

Rejected:

- **Marking the survey's own entities as the services in place.** A DXF's or
  a shapefile's lines have no points to carry a vertex's evidence; the
  survey's strings would become runs that `REGRADE` deletes and draws again
  split by quality level; and the survey, the record of what was measured,
  would be rewritten by a grading.
- **Grading raw geometry in every verb, with the options on each.** The
  evidence of a pothole in the middle of a string has nowhere to live but a
  point, and the points-are-the-schedule model already gives every verb its
  data once the services are drawn; a second reading in six verbs is six
  places to disagree.
- **Reading every property named like a schedule column.** An import's
  `type`, `status` or `id` means what its author meant; `FIELDS` says which
  are the schedule's.
- **A new verb.** `DRAW` already draws services; that it now takes the
  drawing, as the other verbs do, is one form fewer to learn, and the dialog's
  Draw tab reads either source as the others do.

## The TfNSW Utility Schema and Specification

TfNSW's Utility Schema and Specification (DMS-FT-493, v1.2, December 2022) is
the delivery schema NSW transport projects require: 43 attributes per utility
asset - `AssetIdentifier`, `AssetTypeCode`, `AssetOwner`, `Size`,
`DepthLocation`, `Depth`, `QualityLevel`, `LocateMethod`, `Clash` and the rest -
most of them mandatory, most of them taking a value from a list. Two things
use it, and they are kept apart on purpose.

**Grading reads its attribute names.** `subsurface::parseUtilityCsv` takes a
schedule whose columns are the schema's attribute names, so that one file is
both a TfNSW deliverable and something `REPORT`, `VERIFY` and `CLEARANCE` can
grade. `samples/utilities/schedule_tfnsw.csv` is one. The schema has no
geometry, so the schedule adds `point`, `easting`, `northing` (and, as wanted,
`surface`, `h_unc`, `v_unc`). What each schema attribute becomes:

| Schema attribute | Read as |
|---|---|
| `AssetIdentifier` | the service (`line`) |
| `AssetTypeCode` | `UtilityType`: C D E F G I P S W N, AS 5488.2 Table A.4's letters |
| `AssetOwner`, `AssetStatus`, `Material`, `Configuration` | the attributes of those names; `Disused` is a status of its own, not `Abandoned` |
| `Size` | the diameter, as an INSIDE dimension (the schema measures pipes inside); `W x H` takes the larger side; `Not Applicable` and `Unknown` are no size |
| `DepthLocation` | `LevelReference`: Top of Pipe, Obvert, Top of Concrete Encasement, Plastic Cover Protection Encountered and Ground Level are the top (cover is to whatever is met first), Top Row Invert is an invert, Other and Unknown are `LevelReference::Unknown` |
| `Depth` | `UtilityVertex::depth`: below the surface, to the depth location |
| `QualityLevel` | the claimed level; "Quality Level A" .. "D", and "Unknown" claims nothing |
| `LocateMethod` | `LocationMethod`: Archive Drawings and Plans and Geographic Information System are records, Electronic Detection is EML, Ground Penetrating Radar is GPR, Potholing is non-destructive excavation, Survey (a surveyed feature) is a surface feature, Unknown caps the level at QL-D |
| everything else | kept by its name (`UtilityAttributes::fields`, or `UtilityVertex::fields` for the per-point `DepthDescription`, `DateInfoObtained`, `PotholeReport`, `PitReport`, `Notes`), never interpreted; two rows of one asset may not disagree about an asset attribute |

The schema's quality level is one per asset, so the claim is also tested
along the asset: a segment between two points that both claim a level is
claimed at the weaker, and the report says how many metres grade below it
(on the sample, 18 m of a duct bank claimed QL-B between radar picks further
apart than the detected spacing).

Because the schema's `Size` is an inside dimension, the top found from an
invert or a centre is the inside top: a cover from it is larger than the
real one by the wall, and says so. Where both are given, an outside
`diameter_mm` is used instead.

**Checking reads the schema itself, at run time.** `UTILITY CHECK <schedule>
SCHEMA <schema.csv>` tests every row for the mandatory attributes, each value
against its list, dates as YYYY/MM/DD, numbers, subtypes, features and
capacities against the row's asset type code, and that `AssetIdentifier` is
prefixed with the asset type code, as the schema asks. Values must be spelt
exactly - the schema says so - and one that matches only when case is
ignored is a warning naming the listed spelling. The exit status is 1 when
there are errors, so a script can gate a delivery on it
(`cli.utility_check_with_errors_fails_the_script`).

The schema file is made from the user's own copy of the workbook:

```
python tools/utility_schema_domains.py Utility-Schema-and-Specification-v1.2.xlsx tfnsw-utility-schema.csv
katana_cli -c "UTILITY CHECK samples/utilities/schedule_tfnsw.csv SCHEMA tfnsw-utility-schema.csv"
```

It is not in the repository, and neither is the workbook: TfNSW's cover page
says the document may be used only by those providing services to a NSW
Government agency with its authority, and is not under an open licence. So
the repository carries the means of reading it - the same arrangement as the
12d reference files - and a checkout that has a copy, as
Utility-Schema-and-Specification-v1.2.xlsx in the git-ignored folder
"docs/TfNSW Reference Files", registers
`cli.utility_check_schema_from_the_workbook`, which extracts it, and
`cli.utility_check_finds_what_the_tfnsw_sample_gets_wrong`, which checks the
sample against it. (The two `cli.utility_check_with_errors_*` tests use a
schema written by hand and run in every checkout.) The format of the schema file is in
`include/katana/survey/subsurface/delivery_schema.hpp`; nothing in the
checker is TfNSW's, so another client's schema can be written by hand.

Two things found in v1.2 while writing the extraction:

- its two organisation attributes are labelled the wrong way round -
  `TfNSW_ContractOrgCode` is "Originator Name" and `TfNSW_ContractOrgName` is
  "Originator Code". The extraction follows the attribute names, since those
  are what a deliverable's columns carry: codes to `...Code`, names to
  `...Name`.
- `Size` and `Configuration` are called domain lists but list examples ending
  "etc."; they are written as open domains, where any number or `N x M` is
  accepted besides the listed words.

## In the desktop app

Survey > Subsurface Utilities (AS 5488) has an item for each tool: Draw
Utility Schedule, Utility Investigation Report, Verify Detections Against
Exposures, Clearance of Proposed Works, Check Against a Delivery Schema,
Regrade Drawn Utilities and Export Drawn Utilities as a Schedule. Each opens
the same dialog on its own tab. The dialog has one field for each option the
verb takes - the detected spacing and minimum cover shared by Draw, Report and
Regrade, as `SPACING` and `MINCOVER` are, and on Draw of what is drawn the
group "Drawn geometry as services" for the options above - shows the exact `UTILITY` line it
will run, and runs that line through the window's command line. The line is
logged and undoable like a typed one, and the reply appears in the dialog,
where it can be copied or saved. Typing the same line on the command line
does the same, which is how an agent drives it. After a draw or a regrade,
typed or from the dialog, every plan view is framed on what was drawn.

Above the tabs, **Services from** chooses a schedule file or what is drawn.
What is drawn is taken by the same "Apply to" and "Only those that match"
controls as Format > Global Modify - the selection, what a view shows (only
what is on screen, or its layers anywhere), the checked layers with or
without their sublayers, or the whole drawing; then types, layer and style
patterns, colour, a property and its value, text, drawn only - and they write
the scope words into the line: `UTILITY REPORT VIEW 3 WHERE
PROP=utility.type:water`. Draw, Report, Verify, Clearance and Check read
either source - for Draw, what is drawn is the survey's or an import's
geometry; Regrade and Schedule read the drawing. Clearance's
works are a design file, a line or polyline in the drawing (its `#id`, or Use
Selected, at a level or its own heights) or one of the drawing's alignments.
`VIEW` typed in the window works too, since the window answers it; a script
run by `katana_cli` gives `AREA` instead. `docs/desktop.md` ("Survey >
Subsurface Utilities") describes how it is built. File > Export IFC takes a schedule and its delivery
schema too, and its preview shows each service's graded segments by the class
they are written as (`docs/ifc.md`, "In the window").

## Not done

- A service drawn from the drawing does not follow its survey: a string
  moved after the draw leaves the service where it was drawn. Move the
  service's points and `REGRADE`, or delete the service and draw again.
- An arc in the scope is not a service, and neither is a spline: chording one
  would invent located vertices nobody located. Draw the run as a polyline
  through its located points.
- Points are not strung into services by the draw: a coded survey's points
  are strung by its linework first (`docs/survey_coding.md`), and a point on
  no line is counted `loose`.
- `FIELDS` reads an entity's properties, not its metadata; a `size` of
  `W x H` is read from a schedule only; the surface level at a vertex is not
  taken from a surface (a TIN) - only from a height, a point's own, or a
  field.

- The dialog's Clearance design from an entity takes the id typed or the
  one entity selected; picking the works in the drawing while the dialog
  waits is not offered.
- `REGRADE` leaves a quality-level layer it has emptied (a line no longer
  QL-C anywhere) in the layer table, and draws new runs ByLayer, not in a
  style or colour a person gave the old ones: the runs are derived output,
  and removing a layer is `PURGE`'s decision, not a regrade's.
- A drawing made by a `UTILITY DRAW` before 2026-09-26 has points without
  `utility.order` and the rest of the row, and is refused by name; draw its
  schedule again.
- A design centre line drawn as an arc, a circle or a spline is refused;
  draw it as a polyline or make it an alignment.

- The drawing is plan only: levels, depths and cover are properties of the
  points, not a 3D string, and pits, valves and poles are points, not
  symbols.
- Attribute quality levels (grading the type, owner or material of a service
  separately from its position) are not modelled.
- `CHECK` does not evaluate the schema's conditional attributes (it cannot
  know the condition) and does not check the `EPSG Code` beside the
  coordinate system, which the schema lists with no attribute of its own.
- Clearance is between centre lines widened by radius and tolerance, not
  between solids; a rectangular duct bank is treated as round.
