# Subsurface utility investigations (AS 5488)

Tools for surveying buried services and grading what is known about them by
the quality levels of AS 5488.1-2019, "Classification of subsurface utility
information". The code is `katana::survey::subsurface`
(`include/katana/survey/subsurface/`, `src/katana_survey/subsurface_*.cpp`);
the command line reaches it through `UTILITY` (`src/katana_app/utility_verbs.hpp`).
It sits in `katana_survey` because it is survey calculation - positions,
tolerances, lengths, levels - and needs nothing above `core` and `math`.

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

`samples/utilities/schedule.csv` and `samples/utilities/design.csv` are a
worked example: a water main verified by a pothole, an electricity duct bank
with one weak radar pick, a telecommunications run between pits and a gas
main from the records, crossed by a proposed stormwater pipe.

```
katana_cli -c "UTILITY REPORT samples/utilities/schedule.csv MINCOVER 0.6"
katana_cli -c "UTILITY VERIFY samples/utilities/schedule.csv"
katana_cli -c "UTILITY CLEARANCE samples/utilities/schedule.csv samples/utilities/design.csv WIDTH 0.375"
```

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
there are errors, so a script can gate a delivery on it.

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
"docs/TfNSW Reference Files", registers the `cli.utility_check_*` tests, which extract it
and check the sample. The format of the schema file is in
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

## Not done

- The utilities are reported, not drawn: no layers, linetypes by quality
  level, or symbols in the drawing yet, and no Survey menu entry.
- Attribute quality levels (grading the type, owner or material of a service
  separately from its position) are not modelled.
- `CHECK` does not evaluate the schema's conditional attributes (it cannot
  know the condition) and does not check the `EPSG Code` beside the
  coordinate system, which the schema lists with no attribute of its own.
- Clearance is between centre lines widened by radius and tolerance, not
  between solids; a rectangular duct bank is treated as round.
