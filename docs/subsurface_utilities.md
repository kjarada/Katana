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
defaults are the published figures as they were available to the author; a
client specification may be tighter, a later edition may differ, and a
program that hard-coded them would certify against the wrong numbers without
anyone noticing. Check them against your copy of the standard and the
project specification, and set them where they differ.

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
UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>]
UTILITY VERIFY <schedule.csv>
UTILITY CLEARANCE <schedule.csv> <design.csv> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>
UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
```

`REPORT`, `VERIFY`, `CLEARANCE` and `CHECK` read the files, grade and reply
with the report; they never touch the drawing, so they are safe against any
open project. `SPACING` is `GradingSettings::maximumDetectedSpacing` (default
10 m) for both `REPORT` and `DRAW`, so a drawing and a report of one schedule
agree. `CHECK` with errors against the schema is refused, and the refusal
carries the whole check - a script that stops must still say why. `katana_cli`
prints the refusal's first line on stderr, as every refusal, and the check
after it on stdout, where a report goes, so `katana_cli -c "UTILITY CHECK
..." > check.txt` keeps the check exactly when it failed; and exits 1. An option
that is not the verb's, one given twice, or a negative or missing number is
refused by name; so is a file that cannot be read or parsed, with its path.

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
a service level, the part of the service the level is on - `top` for a level
given with no reference, as the grading and the cover read it, so it does
not say whether the reference was recorded.

| On | Properties |
|---|---|
| each polyline | `utility.line`, `utility.type`, `utility.quality_level` (the run's), `utility.limited_by` (why the run is below its ends, each reason once), `utility.length` (the run's plan length), `utility.from` and `utility.to` (its end vertices), `utility.owner`, `utility.material`, `utility.diameter` (metres) and `utility.diameter_inside`, `utility.configuration`, `utility.description`, `utility.status` |
| each point | `utility.line`, `utility.type`, `utility.vertex`, `utility.method`, `utility.quality_level` (the vertex's graded level), `utility.claimed` and `utility.over_claim`, `utility.service_level` with `utility.level_ref`, `utility.level_qualified`, `utility.surface_level`, `utility.cover` with `utility.cover_note`, `utility.cover_below_minimum` (only with `MINCOVER`), `utility.verifies` |
| both | a delivery schema's own attributes, uninterpreted, as `utility.field.<name>` |

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

A drawn plan exported to IFC (`EXPORT <file.ifc>`, File > Export IFC) goes
out by service, not by layer: each run the class the schedule's own export
gives it, each point a survey annotation, one system a service, each
classified by its quality level (`docs/ifc.md`, "Subsurface utilities").

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
Exposures, Clearance of Proposed Works and Check Against a Delivery Schema.
Each opens the same dialog on its own tab. The dialog has one field for each
option the verb takes - the detected spacing and minimum cover shared by Draw
and Report, as `SPACING` and `MINCOVER` are - shows the exact `UTILITY` line
it will run, and runs
that line through the window's command line. The line is logged and undoable
like a typed one, and the reply appears in the dialog, where it can be copied
or saved. Typing the same line on the command line does the same, which is how
an agent drives it. After a draw, typed or from the dialog, every plan view is
framed on what was drawn. `docs/desktop.md` ("Survey > Subsurface Utilities")
describes how it is built. File > Export IFC takes a schedule and its delivery
schema too, and its preview shows each service's graded segments by the class
they are written as (`docs/ifc.md`, "In the window").

## Not done

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
