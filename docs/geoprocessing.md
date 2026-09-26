# Geoprocessing: GDAL's algorithms on every front end

GDAL 3.11 onwards has an algorithm framework: the `gdal raster ...`,
`gdal vector ...`, `gdal pipeline` command line, and the C++ registry
behind it. GDAL 3.13.2 (MSYS2's, the one Katana builds against) holds 121
algorithms in 17 groups: terrain analysis (hillshade, slope, aspect,
viewshed, contour), gridding, vector overlay and buffering, SQL, format
conversion, dataset and file management.

This document is the record of how Katana puts them on all three surfaces
every feature ships on: the window, `katana_cli` and `katana_mcp`, each
running the same code. It is also the base the geoprocessing packages build
on: one section per package under "Packages" below, each package editing only
its own.

The foundation (package F0) is five pieces, one per layer:

| Piece | Where | What |
|---|---|---|
| The bridge | `include/katana/gis/processing.hpp`, `src/katana_io/geo/` | GDAL's catalogue, argument specs and runs behind plain types; the only code that includes `gdalalgorithm.h` |
| The bindings | `include/katana/interop/geo/`, `src/katana_interop/geo/` | drawing data to feature tables and back as one undo step; a surface as a grid; derived rasters |
| The executor | `src/katana_app/geo/geo_verbs.hpp` | prepare, work, apply: the one path every geoprocessing verb runs on |
| The GDAL verb | `src/katana_app/geo/gdal_verbs.cpp` | `GDAL VERSION`, `LIST`, `HELP`, and a run of any algorithm in GDAL's own words with Katana's `FROM` and `TO` |
| The MCP tools | `src/katana_app/geo/mcp_geo_tools.cpp` | `katana_gdal_catalogue`, `katana_gdal_describe`, `katana_gdal_run` |

In the window the verb runs as a background job (`src/katana_qt/geo/`).

Every rule in "The bridge" was measured, not assumed. The measurements were
made with small programs against GDAL 3.13.2 on 2026-09-26, while the
design was drawn up. Where a number is quoted ("63 of 120"), it is from
those runs.

## The bridge

`katana::gis::processing` (`include/katana/gis/processing.hpp`) wraps GDAL's
C++ algorithm API behind plain types: `ArgSpec`, `AlgorithmInfo`,
`RunRequest`, `RunOutputs`, `RasterGrid`, `FeatureSet`. No GDAL type crosses
it, so everything above is written against this header. GDAL itself calls its
algorithm API provisional: `gdal --help` "reserves the right to modify,
rename, reorganize". The contract tests (below) are what notice when it does.

### The catalogue

- **Walked from the root.** `catalogue()` instantiates the root algorithm
  `gdal` and walks its sub-algorithms. The registry's own top-level list
  (`GetNames`, the C API's `GDALAlgorithmRegistryGetAlgNames`) stops at
  `convert dataset info mdim pipeline raster vector vsi`; it misses the
  `driver` family (`driver gpkg validate`, `driver cog validate`, ...).
  `GdalCatalogue.HoldsEveryLeafFromTheRootIncludingTheDriverFamily`.
- **Walked once.** It is cached behind a `std::call_once`, and read-only
  after that, so it is safe from any thread.
- **Aliases.** GDAL lists an algorithm's public and hidden aliases in one
  list, split at `GDALAlgorithmRegistry::HIDDEN_ALIAS_SEPARATOR`. `resolve()`
  accepts both, case-insensitively, so `raster warp` is `raster reproject` and
  `raster neighbours` is `raster neighbors`. It takes the longest run of words
  that names an algorithm, and a group (`vector grid`) is refused listing
  its members (`GdalCatalogue.AnAliasResolvesToItsAlgorithm`,
  `GdalCatalogue.AContainerIsRefusedListingItsChildren`).
- **Instances.** Every use takes a fresh instance, walked down from the root.
  `Run()` is single-shot: a second call fails with a misleading ": No such
  file or directory".

### Arguments as data

`describe()` reads each `GDALAlgorithmArg` into an `ArgSpec`:

- **Hidden arguments are dropped.** An argument hidden, or hidden from the
  API (`help`, `json-usage`, `config`, `progress`), is left out.
- **Bounds.** GDAL returns a bound as a pair: the value, and whether it is
  inclusive. A NaN value means there is no bound. The inclusive flag does not
  say that: 19 of GDAL's 82 minimums are exclusive (`contour --interval` must
  be above 0). An absent bound is `std::nullopt`, never 0
  (`GdalDescribe.ReportsBoundsWithTheirInclusivity`,
  `GdalDescribe.AMissingBoundIsAbsentNotZero`).
- **Dataset arguments.** Each says its kinds (raster, vector,
  multidimensional), whether it is opened for update, and whether it takes a
  name, an object or both (GDAL's `GADV_NAME`, `GADV_OBJECT`).
- **GDAL's own usage.** `usageJson` is GDAL's `--json-usage`, verbatim, for
  agents. It leaves out short names, aliases and the positional flag, so it is
  a supplement to the specs and never their source.

### A run

`run(request, stop, progress)`:

- **Values first.** Each `RunRequest::values` entry is bound by long name,
  short name or alias. Scalars go through GDAL's `Set`, which refuses a bad
  value at once with GDAL's own message ("Should be one among ...",
  "altitude is 100, but should be <= 90").
- **An in-memory dataset per run.** Each `RasterGrid` or `FeatureSet` becomes
  its own MEM dataset, built inside the run, on the run's thread. One MEM
  input shared between concurrent runs gave 63 wrong results in 120, with
  "Only one feature iterator can be active at a time"
  (`GdalRun.ConcurrentRunsOnTheirOwnDatasetsMatchTheSingleThreadedResult`).
  A list is bound with a moved `std::vector<GDALArgDatasetValue>`: writing
  into the argument's own list does not mark it set.
- **GDAL's words last.** The request's `tokens`, GDAL's own command-line
  tail, go to `ParseCommandLineArguments` after the datasets are bound. So
  list, CRS and choice parsing is exactly GDAL's. GDAL gives a positional
  word to the next positional argument not already set, so with the input
  bound, `hillshade shade.tif` names the output.
- **The output.** It goes to memory (`--output-format MEM`, an empty name),
  unless the tail names it or the request asks for a file. An info-like
  algorithm whose output is optional and which prints (`raster pixel-info`)
  is left to print; a pipeline prints only when its last step does (`info`,
  `compare`, `export-schema`: "Pipelines" below). The output's reference is
  taken before `Finalize()`,
  which drops the algorithm's own; taken after, it is null.
- **Reading an output back.** A raster becomes a `RasterGrid` (Float64
  values, GDAL's data type named), and a vector a `FeatureSet` of typed
  fields, with multi-geometries as their members. What GDAL printed is
  `text` (the `output-string` argument), and what it returned is
  `returnCode` (`dataset check`, `raster compare`).
- **Validated apart.** `ValidateArguments()` runs before `Run()`, so a
  refusal of the request is `InvalidArgument`, not a failure of the
  algorithm.

### Big rasters spill to files

A raster output of more cells (width x height x bands) than
`maxMemoryCells` (64 Mi by default) never enters a `std::vector`. It is
copied to a tiled GeoTIFF, `COMPRESS=DEFLATE`, with `PREDICTOR=3` for
floating-point data, in `spillDirectory`, and `RunOutputs::file` names it.
`maxMemoryCells = 0` spills every raster. That is how a derived reference
raster is written: without passing through memory, in the form it is kept in
(`GdalRun.ARasterLargerThanTheMemoryLimitSpillsToATiledGeoTiff`).

A `raster color-map` output made in memory has its bands named red, green,
blue and, with `--add-alpha`, alpha before it is spilled or read: GDAL 3.13
leaves them Undefined in MEM, and a picture copied on so drew its clear
cells opaque (docs/terrain.md, "Viewshed and line of sight", Decided).

### Names only: staging

A few arguments take a dataset by name only: `raster calc`'s inputs, the
index inputs, `mdim mosaic`, `driver parquet create-metadata-file`
(`GdalContract.NameOnlyInputsAreTheOnesTheBridgeStages`). A grid or feature
set given to one is written under `/vsimem/katana/<run>/`, as a GeoTIFF or a
GeoPackage. The folder is removed when the run ends, whatever happened
(`GdalRun.ANameOnlyInputIsStagedAndRemoved`, which also checks
`stagedDatasetCount()` is 0 afterwards). Name-only OUTPUTS (`vector grid *`,
`convert`) need nothing: a MEM output's name is the empty string, which is
a name.

This MSYS2 GDAL has neither muparser nor ExprTk. `raster calc` therefore
runs only its builtin dialect (`--dialect=builtin`, with sum, mean, min, max
...); free-form expressions are refused by GDAL, with GDAL's message.

### Cancel

- **The stop is the adapter's to report.** The progress adapter calls
  `progress`, then returns FALSE to GDAL once the stop is requested. It
  records that it did.
- **Run()'s answer is not trusted.** A run is cancelled whenever the adapter
  refused or the stop was requested, whatever `Run()` returned. `raster
  hillshade` returns true after a refused progress call, with a partial
  output. slope, aspect, roughness, tpi and contour return false with no
  message. The vector algorithms report "Failed to write layer".
  `GdalRun.ACancelledRunIsCancelledEvenWhenGdalReportsSuccess` requests the
  stop from inside the progress callback, after two calls.
- **What a cancel leaves.** The outputs are discarded, and so is a partial
  file the run began, when the file did not exist before. The result is
  `InvalidState` "cancelled".
- **A stop requested before the run** runs nothing
  (`GdalRun.AStopBeforeTheRunStartsRunsNothing`).
- **Pipelines stop late.** Pipelines, and some vector algorithms, report
  progress once, at the end. Cancelling them is best-effort: they stop there.

### Errors

- **One collector per run.** A run collects its errors with a thread-local
  handler (`CPLPushErrorHandlerEx`). 8 threads x 50 runs saw 200 errors of
  their own and none of each other's.
- **The first failure is the message.** The `Result`'s message is GDAL's
  first failure, which already names the algorithm ("buffer: Required
  argument 'distance' has not been specified."). Warnings come back as
  `Diagnostic`s.
- **GDAL's own threads.** Errors raised on threads GDAL starts itself (the
  viewshed's) skip the run's handler. They reach the process-wide
  `CPLQuietErrorHandler` that `gdal_adapter.cpp` installs, which therefore
  stays. A per-run global handler would be wrong with runs in parallel. The
  failure such an error causes still reaches the run through `Run()`
  (`GdalRun.AViewshedCancelledOnGdalsOwnThreadsStillSaysCancelled`).
- **The codes.** A failure where an input is opened is `FileImportFailure`,
  and where the output is written `FileExportFailure`. A refusal of the
  request is `InvalidArgument`. Anything else GDAL ran and failed at is
  `CommandRejected`.

### Configuration

- **Two options only.** A run may set `GDAL_NUM_THREADS` and `GDAL_CACHEMAX`,
  thread-locally. Anything else is refused by name
  (`GdalRun.OnlyAllowListedConfigurationIsSetForARun`).
- **Nothing that matters may depend on them.** Thread-local options do not
  reach threads GDAL starts (measured).
- **`--config` is refused as a word.** It could set `GDAL_ENABLE_EXTERNAL`,
  which lets a pipeline's `external` step run programs, or
  `SPATIALITE_SECURITY=relaxed`, which opens Spatialite's file functions.
  Katana never sets either.

### Sidecars

No `.aux.xml` may be left beside a person's file by a run that only read it.
`raster info --stats` writes one when the dataset closes. The design offered
two switches, and both were measured:

| Switch | Stops a new sidecar | Still reads one already there |
|---|---|---|
| thread-local `GDAL_PAM_ENABLED=NO` | yes | **no** - its metadata, no-data, georeferencing are lost |
| process-wide `GDAL_PAM_PROXY_DIR` | **no** - used only where the file's folder is not writable | yes |

Neither will do. So the bridge (`SidecarGuard`) notes the sidecar beside
every input it reads by name, before the run. When every dataset has closed,
it puts it back as it was: removed when the run made it, restored byte for
byte when the run changed it. An input opened for update is left alone:
what such an algorithm writes is the change asked for.
`GdalRun.ARunOnAUserFileLeavesNoSidecarBesideIt` and
`GdalRun.ASidecarAlreadyBesideAFileIsReadAndLeftAsItWas` pin both halves.

A pipeline names its inputs in its text, not in a dataset argument, so they
were not watched: `raster pipeline "read a.asc ! info --stats"` left
`a.asc.aux.xml` behind (review finding). Every word of a pipeline that is
not an option, and every option's value, is now watched, outside the steps
that write (`write`, `update`, `materialize`, `tile`, `partition`); a word
that names no plain file is passed over
(`GdalRun.APipelinesReadStepLeavesNoSidecarBesideItsFile`). The same test
found that a pipeline ending in `info`, given word by word rather than
quoted, was bound a MEM output and refused: whether a pipeline prints is now
read from its words when it is not one quoted argument.

## Safety

Every leaf is Safe or Confirm (`src/katana_io/geo/policy.cpp`).

- **What Confirm means.** An algorithm that changes or removes EXISTING data
  in place is Confirm: vsi delete, move, sync, copy, sozip create and
  optimize; dataset delete, rename and copy; driver gpkg and openfilegdb
  repack; parquet create-metadata-file; raster edit; raster overview add,
  delete and refresh; raster update; vector update.
- **Two sources, the stricter wins.** A table judges each of GDAL 3.13's 121
  leaves by hand. And a rule: an input GDAL opens for update makes an
  algorithm Confirm whatever the table says, so `raster edit` is caught by
  the rule before the table (`policyReason` says which).
- **An unjudged leaf is Confirm.** A leaf a GDAL upgrade adds is
  "unclassified" until someone judges it, and
  `GdalPolicy.UnclassifiedLeavesNeedConfirmAndAreNamed` names every one.
- **Running one.** A Confirm algorithm runs only when the line says
  `CONFIRM`, or an MCP call passes `confirm: true`. In the window it is the
  toolbox's Confirm box, without which it writes no line.
- **Refused words.** `--config`, `--help`, `-h`, `--help-doc`,
  `--json-usage`, `--progress`, `--quiet` and `-q` are refused
  (`checkTokens`), and so is a pipeline step called `external`, in the words
  or in a pipeline string, nested in `[ ]` too
  (`GdalPolicy.APipelineStepNamedExternalIsRefusedWhereverItStands`).
- **Replacing a file.** `TO FILE` never replaces a file without `OVERWRITE`,
  and the tail's `--overwrite`, `--overwrite-layer`, `--append`, `--update`,
  `--upsert`, `--add` and `--resume` need `OVERWRITE` too: each changes a
  dataset already there. `--add` (vector rasterize) burns into the raster
  the output names with no `--update` - measured with `gdal vector
  rasterize`, the file's checksum changed - and `--resume` (raster tile)
  writes into a tile set already there. The list came from every boolean
  argument of every GDAL 3.13 algorithm and pipeline step (`gdal
  --json-usage`), each whose name suggested it read by its description
  (`GdalPolicy.RasterizeAddAndTileResumeChangeAnExistingDataset`).
- **A replaced file survives a cancel.** A file `TO FILE` names is written
  into a staging folder beside it and put in place by the apply
  (`StagedFiles`, as EXPORT's is). With `OVERWRITE`, GDAL wrote over the
  file from the start, so a run cancelled part way left an empty or partial
  file where the original had been (review finding;
  `GeoExecutor.ACancelledRunLeavesTheFileItWouldHaveReplacedAsItWas`: the
  original's bytes are there after the cancel). A file GDAL's own words
  name is GDAL's to write where they say ("Not done").
- **A pipeline is judged by its steps and its words, however it is quoted.**
  The three pipeline algorithms are Safe as leaves, but a pipeline's steps
  can do what a Confirm leaf does: `read a ! update b` writes into `b`, and
  `"read a ! write --overwrite b"`, quoted as one word, replaced `b` while
  the tail check saw only the one word (review finding, reproduced with
  katana_cli: the file's checksum changed). `tailEffects` reads the pipeline
  word by word - words given one by one, one quoted text, `--pipeline=...`,
  nested `[ ]` steps - and gives the step that needs `CONFIRM` and the word
  that needs `OVERWRITE`; the verb and `katana_gdal_run` both ask it
  (`GeoExecutor.APipelineUpdateStepNeedsConfirmQuotedOrNot`,
  `GeoExecutor.AQuotedPipelineThatOverwritesAFileNeedsOverwrite`,
  `McpServer.GdalRunRefusesAPipelineThatChangesExistingDataUnlessToldTo`).
- **Steps are judged as leaves are.** Of the steps GDAL 3.13's pipelines
  offer, only `update` writes into a dataset already there, so it is Confirm
  as `raster update` and `vector update` are. `edit` and `overview` are
  Confirm as leaves but not as steps: a step changes the piped dataset,
  which the pipeline then writes where it is told. A step in neither list is
  Confirm until judged, and `GdalPolicy.EveryStepGdalsPipelinesOfferIsJudged`
  names any a GDAL upgrade adds. The text's first word is the exception: it
  may be the value of an option before the pipeline (`--output-format GTiff
  read ...`), which GDAL refuses by itself if it is no step.
  - *Rejected:* asking GDAL. The pipeline algorithm parses its steps only
    when it runs, and its usage JSON does not say which step opens its
    output for update, so the table is the judgement, as it is for leaves.
  - *Rejected:* mapping a step to the leaf of the same name. That would make
    `edit` Confirm, wrongly, and say nothing of a step with no leaf.
- **A pipeline that ends in `update` names its output there,** as one ending
  in `write` does. Bound a second output by the run, GDAL refused every such
  pipeline ("update: Positional values starting at 'b.tif' are not
  expected"), so a confirmed update could not run at all.
- **MCP.** `katana_gdal_run` takes `confirm` for a Confirm leaf or an
  `update` step, and `overwrite` for GDAL's own words above; `output`'s
  `overwrite` still covers the file `TO` names.

## Bindings

### Drawing data to features

`interop::geo::drawingDataset` (`include/katana/interop/geo/drawing_dataset.hpp`)
is the ONE conversion of entities to feature tables, and EXPORT writes files
through it too (I1; `docs/interop.md`, "Fidelity"). For EXPORT it writes
one table of every kind (`oneTable`), with `katana_id` and `layer` but not
`style`, `colour` and `type` (`styleFields`), and the properties only when
asked (`properties`).

- **Tables.** `points`, `lines` and `polygons`; an empty one is omitted.
- **Fields.** `katana_id` first (Integer64: the key a result is joined back
  by), then `layer`, `style`, `colour`, `type`. Then the properties, typed
  and sorted by key. A key whose type differs between entities becomes a
  String and says so
  (`DrawingDataset.KatanaIdAndTypedPropertiesBecomeTypedFields`,
  `DrawingDataset.AKeyOfSeveralTypesBecomesTextAndSaysSo`).
- **Areas.** A closed polyline, or a circle, is an area. Nested closed
  polylines are NOT holes: a building inside a lot is not a hole in the lot
  (`DrawingDataset.ABuildingInsideALotIsNotAHole`).
- **Holes.** Only a ring tagged `source.ring=hole` (what IMPORT writes, in
  an entity's metadata) or `gis.ring=hole` (what a result writes, in its
  properties) joins an area. It joins the one it lies wholly inside - the
  one whose part it shares when there is such (`gis.part`, or IMPORT's
  `source.part`), else the smallest, and of equals the one made nearest
  before it, so a file imported twice joins each hole to its own exterior
  (`DrawingDataset.ATaggedHoleJoinsItsExterior`: 10000 - 400 = 9600 m2). A
  tagged hole inside no area stays an area of its own, and says so. GEOS's
  organisePolygons was the design's choice for this; a point-in-polygon test
  of every vertex does it without GDAL in the interop layer, and it is what
  the part hint is checked against.
- **Curves.** Arcs and circles are chords within `curveTolerance` (1 mm by
  default), by the same `chordCount` EXPORT uses (`src/katana_interop/curve_chords.hpp`).
- **The draw system's curves** (`docs/drawing.md`), each read as main's
  vector export read it before the one conversion took EXPORT over:
  - a **curve polyline** (`CurvePolyline2`, a polyline with arcs by bulge)
    is chords within `curveTolerance` (`tessellateWithHeights`), and closed
    it is an area (`DrawingCurves.AClosedCurvePolylineIsAnAreaShortOfItsArcsSegmentByAtMostTheChordError`:
    100 + 25 (pi/2 - 1) m2, less at most the arc's length times the
    tolerance). Its heights are its vertices' own, not the elevation
    properties (it holds them itself); a chord point's height is the
    geometry's rule, linear by length along its segment, and none where an
    end has none - so a string with one vertex not surveyed goes in plan
    and is counted, as a polyline does
    (`DrawingCurves.ASlopingArcsChordPointsTakeTheHeightLinearByLengthAlongIt`,
    `DrawingCurves.AnArcWithAnEndNotSurveyedGoesInPlanNotAtZero`).
  - an **ellipse** or **spline** is chords within `curveTolerance`; a whole
    ellipse, and a spline that ends where it began, is an area; both go in
    plan, since neither holds a height
    (`DrawingCurves.AWholeEllipseIsAnAreaAndAnArcOfOneALineBothInPlan`,
    `DrawingCurves.AClosedSplineIsAnAreaAndAnOpenOneALine`).
  - Everything that reads the drawing through this conversion takes them
    with no code of its own: EXPORT, every GDAL algorithm, the closed
    shapes used as zones (RASTER ZONAL), clip boundaries (GIS CLIP, RASTER
    CLIP, IMPORT's clip, CONTOUR's and slope's boundaries), dissolve and
    overlay areas, and the points HULL and RASTER GRID read. GIS DISSOLVE
    REPLACE deletes the entities of the areas table rather than testing
    each entity's kind, since that test was a second rule of what an area
    is and did not know the new kinds
    (`DrawingCurves.DissolveReplaceDeletesTheEllipsesItMerged`).
  - An in-place result on one of them (GIS CLIP REPLACE cutting it, a
    repair that moved it) writes back the chords of what GDAL returned, as
    a polyline with its heights in its properties - the same as an arc
    cut in place. A result that leaves it as it was is no edit, so the
    curve stays a curve.
  - *Rejected:* taking an ellipse's or spline's height from an `elevation`
    property. Neither kind has a height in the draw system, and main's
    export wrote both in plan; a height the geometry does not hold would
    be one invented for every point between.
  - *Rejected:* a curve type in the tables (GDAL's CircularString /
    CompoundCurve) for a curve polyline's arcs. The algorithms work on
    linear geometry and GEOS linearises curves itself at its own
    tolerance; chording here keeps the one tolerance the reply can state.
- **Heights.** Z is written only when every vertex has a height: absent is
  not zero (`DrawingDataset.AVertexWithoutAHeightKeepsTheLineTwoDimensional`).
  An entity heighted at only some vertices, or an arc whose ends differ,
  goes in plan with its heights in the elevation fields, and a warning
  counts them. With `requireHeights`, a heightless entity is left out and
  counted.
- **What reads Z is given heights only.** A table with one heighted feature
  is three-dimensional, and GDAL reads a feature in it with no Z as z = 0.
  So the GDAL verb sets `requireHeights` when its run reads Z
  (`processing::readsHeights`): a vector grid that makes values - every
  method but `count`, `average-distance` and `average-distance-points` -
  unless `--zfield` names a field; `vector rasterize --3d`; a pipeline with
  such a step. Before, a heightless point was a 0 in the grid
  (`GridVerb.TheGdalVerbLeavesAHeightlessPointOutOfAGridThatReadsZ`: every
  surveyed height 10, the cell by the heightless point 0). Other algorithms
  keep a heightless entity, because they read positions only.
  - *Rejected:* leaving heightless entities out of every GDAL run. A buffer
    of a line with no heights is a buffer all the same.
  - *Not judged:* `vector sql` can read Z in its SQL; the verb cannot see
    that, so a heightless feature reaches it as GDAL gives it.
- **What has no feature.** Text, dimensions, labels and leaders are left
  out and counted by kind; the counts are the scope record's `skipped.*`.
- **CRS.** The project's coordinate system (the document's
  `coordinateSystem`) is set on every table - unless an entity in it was
  imported moved from its file's coordinates (LOCAL, ALONGSIDE, OFFSET=;
  metadata `source.shift`), which is in no known system: then no table
  claims one, and a warning says why (`docs/interop.md`, "Placing an
  import").
- **Coordinate systems are compared by GDAL, not by their text**
  (`gis::sameCrs`, `OGRSpatialReference::IsSame`, a geographic 3D system
  demoted to its 2D one): a .prj's naming and EPSG's definition of one
  system are the same system, and "unknown" (either empty or unreadable)
  is never "different". The plan's risk list asked for this; nothing
  compared systems before the review.
- **A result in another system than the project's is said.** A result
  drawn TO LAYER, or kept as a reference raster, whose system is known to
  differ from the project's is drawn at its own coordinates, and the reply
  says so in a warning naming both: `GDAL vector reproject
  --output-crs=EPSG:4326 ... TO LAYER` drew degrees among MGA metres with
  nothing said (review finding;
  `GeoExecutor.AResultInAnotherCrsThanTheProjectsIsSaidToBe`). Drawing it
  is still what was asked; the curated raster tools that hand drawing
  points to GDAL as the raster's coordinates (RASTER VIEWSHED, SAMPLE,
  DRAPE, LOS) refuse instead (`docs/terrain.md`).

### Features to the drawing

`resultCommand` builds ONE `commands::Transaction`, named with the verb line,
and changes nothing itself: the executor executes it, so a result is one
undo step (`DrawingDataset.AllResultsAreOneUndoStep`). A feature becomes
entities by `featurePieces`, which IMPORT uses as well: a point, a line or
polyline, an arc or a circle where the feature's curve is one
(`VectorGeometry::arcs`), an area's rings as closed polylines.

- **Create.** Missing layers are made; a result of several tables goes to
  `<layer>/<table>`. Typed fields become typed properties. `gis.op` names
  the algorithm, and `gis.source` the entity a feature came from. A
  polygon's rings are closed polylines tagged `gis.ring` and `gis.part`; the
  part is the exterior's own id, so the rings join again when they are read
  back (`DrawingDataset.AHoleWrittenBackIsTaggedAndJoinsItsAreaAgain`). No
  area or length is written, because a measure kept on an entity goes stale
  when the entity is edited; the replies carry measures instead.
- **UpdateGeometry.** It replaces the geometry of the entity `katana_id`
  names, heights included. The id is kept, and so are the entity's labels
  and associations (`DrawingDataset.UpdateGeometryKeepsTheEntitysId`).
- **SetProperties.** It writes the fields as properties, prefixed, on the
  entity each `katana_id` names.
- **REPLACE is the verb's own.** `resultCommand` deletes nothing. A verb
  that replaces adds the deletion to the same step: GIS DISSOLVE deletes
  the areas its scope took, GIS CLIP what fell outside. A feature carries
  only its area's `katana_id`, so the holes an area took in are listed in
  `DrawingDataset::holes`, and a verb that deletes an area deletes them
  with it. The option `deleteSources` of `ResultOptions` - delete what
  each feature's `katana_id` names - was removed: no verb used it, and it
  could not find
  the holes, which never reach the features as ids. A second REPLACE beside
  the verbs' own would have kept that defect alive.
- **Nothing to do.** A result of nothing is no command: the command stack
  refuses an empty transaction, and nothing changed.

`DrawingDataset.DrawingToDatasetToEntitiesIsTheIdentity` round-trips points,
lines, polylines and areas with their properties.

### Rasters and surfaces

- **`RASTER <id|name>`** binds the reference raster's own file, read at full
  precision by GDAL, never its 8-bit display copy.
- **`SURFACE <name> [CELL <m>]`** binds a surface from the session's
  `terrain::SurfaceStore`, sampled at cell centres by `geo::surfaceGrid`: the
  one sampler, which `exportSurfaceRaster` now uses too. The cell is CELL, or
  `suggestedCellSize` of the surface's extent; the grid is capped at 25M
  cells. The sampling is work, so it runs on the worker, from the shared
  immutable surface prepare copied a pointer to.
- **`FILE <path> [LAYER <name>]`** binds the path as it is: a /vsi path or a
  URL passes through. LAYER names the layer of the dataset the FROM binds:
  GDAL's `<arg>-layer` where the algorithm has one (vector clip's
  `--like-layer`, layer-algebra's `--method-layer`, zonal-stats'
  `--zones-layer`), else `--input-layer` for `input`, or for the one vector
  dataset (raster pixel-info's `position-dataset`). A dataset with no layer
  argument of its own refuses LAYER, naming it. Before, every LAYER set
  `--input-layer`, so `GDAL vector clip FROM input FILE pts.gpkg FROM like
  FILE areas.gpkg LAYER m` looked for `m` in the points and failed ("Cannot
  find source layer 'm'";
  `GdalRun.AFileSourcesLayerIsTheLayerOfItsOwnDataset`).

### Derived rasters

A raster result goes to `<project>/cache/gdal/<name>.tif`. When the drawing
has no project it goes to the front end's scratch folder
(`<temp>/katana-scratch/<process id>-<start>/<n>`, `geo::ownScratch`), and
the reply says `persisted=no`. It is read by `interop::importRaster` as a
reference raster with `role` Derived and `derivation` - the line that made
it. A name already taken becomes `<name>-2`, `-3` ...
(`GeoExecutor.ARasterResultBecomesADerivedReferenceRaster`). `RasterOverlay`
gained `role`, `facts`, `derivation` and `displayStyle`; F0 fills the role
and the derivation, T2 sets the display style of the pictures it renders
(the style is rendered into them), and D2 persists them.

The scratch folder is named by the process id and the time the process
first asks for it, not by the id alone. Windows hands an ended process's id
to a later one, and the folder the earlier one left behind still held its
files, so a later run's `NAME west` came back as `west-2` - a name not
asked for, and replies that changed from run to run (two tests failed so in
one whole-suite run, with 153 such folders in the temp folder;
`DemSession.ARasterAnotherSessionMadeDoesNotRenameTheResult`).
Clearing the folders left behind at start-up was rejected: a name does not
say whether its process has ended, and another window still running may be
using its folder.

Inside the process's folder each front end - each `app::Session`, the
window - has a numbered folder of its own, given once when it is made. While
the folder was one per process, a second Session in the same process found
the first one's `west` and its own came back as `west-2`; the test above
failed so whenever `katana_geo_tests` ran as one process, and passed under
ctest, which runs each test alone. Clearing the folder between sessions was
rejected for the reason above: another front end in the process may still
hold a reference raster there.

## The executor

`katana::app::geo` (`src/katana_app/geo/geo_verbs.hpp`) runs a line in three
phases, so a session runs it inline and the window as a job:

1. **`prepare`, on the calling thread** (the GUI thread in the window):
   - the line is split (`geo::tokenize`: the interpreter's rules, and which
     words were quoted, so a quoted `"FROM"` is a value). What writes a
     line - every GDAL dialog and the MCP tools - quotes a name by the one
     rule, `geo::lineWord` (`replies.hpp`): a blank, an empty word, or a
     word the line reads as its own (`geo::lineKeyword`: TO, PREVIEW,
     CONFIRM, REPLACE, NAME, WHERE, the source and scope words ...). A layer
     named `preview` was written bare: `TO LAYER preview` was refused, and
     `TO REFERENCE PREVIEW` previewed instead of making a raster
     (`GisBuffer.AnOutputNamedLikeAKeywordGoesWhereItSaysOnceQuoted`,
     `DemVerbs.AReferenceNamedPreviewIsMadeNotPreviewedOnceQuoted`,
     `GisBufferLine.ALayerNamedLikeAKeywordIsQuoted`,
     `McpServer.GdalRunQuotesAnOutputNamedLikeAKeyword`). The four helpers
     that each quoted by their own rule (`gisWord`, the two `lineWord`s of
     the dialogs' support files, `katana_gdal_run`'s `word`) now call it.
     Reading a keyword as a name where one is expected was rejected: `TO
     REFERENCE` takes an optional name, so `TO REFERENCE PREVIEW` is
     ambiguous to a reader, and a quoted word never is;
   - the verb is found in the table, the algorithm resolved, its policy
     checked;
   - FROM and TO are read by the ONE source and target parsers
     (`bindings.hpp`);
   - scopes are resolved by `cad::matchScope`, `VIEW` through the
     interpreter's scope context (`CommandInterpreter::scopeContext`);
   - everything the work needs is COPIED: a feature set, a surface's shared
     pointer, paths.

   `LIST`, `HELP`, `VERSION`, `PREVIEW`, a refusal and a scope that took
   nothing answer here.
2. **`work`, on a worker or inline.** It is pure: the bridge and the
   conversions, never the Document or the reference data. It returns an
   `Apply` (`GeoExecutor.TheWorkReadsOnlyWhatPrepareCopied`).
3. **`apply`, on the calling thread.** One command on the drawing,
   reference rasters added, and the reply records.

- **Nothing shows until it is applied.** A cancelled or failed run leaves no
  trace (`GeoExecutor.ACancelledRunChangesNothing`).
- **Create-only applies are safe** whatever the drawing did meanwhile.
- **In-place applies check first.** Those the lanes add (UpdateGeometry,
  SetProperties, REPLACE) compare their targets with the copies taken at
  prepare, by `geo::unchangedSince`. They refuse with `InvalidState` "the
  drawing changed while the job ran"
  (`GeoExecutor.AnInPlaceApplyRefusesWhenTheDrawingChangedWhileTheJobRan`).

**The front ends.**

- **`katana_cli` and `katana_mcp`.** `Session::run` asks `geo::handles`, then
  runs `geo::runNow` inline. Records go to stdout; `error: <Code>: <message>`
  goes to stderr.
- **The window.** It runs the line through `GeoWorkbench::runLine`
  ("The window" below).

**The verb table** (`src/katana_app/geo/verb_table.cpp`) maps a verb, and an
optional second word, to its prepare function and its usage, and - where a
verb is shared with the interpreter - to `takes`, which says which of its
lines are the executor's (`INFO <file>`, not `INFO <id>`). `handles`,
`prepare` and `helpText` read it alone. So HELP, the window's Command
Reference (its "Geoprocessing" section) and `katana_help` list the same
verbs, and HELP typed as a session line is `katana_help`'s text
(`cli.help_typed_as_a_line_names_the_geo_families`). It has one reserved
block per package.

## The GDAL verb

```
GDAL VERSION
GDAL LIST [raster|vector|mdim|dataset|vsi|pipeline|driver|convert|info|<text>] [JSON]
GDAL HELP <algorithm> [JSON]
GDAL [RUN] <algorithm> [<gdal word>...] [FROM [<arg>] <source>]... [TO [<arg>] <target>]
     [CONFIRM] [OVERWRITE] [PREVIEW]

<source> := <scope> | RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path> [LAYER <name>]
<scope>  := SELECTION | DRAWING | VIEW [<id>] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY],
            then [WHERE key=value ...] - cad::parseScopeWords, the one scope parser
<target> := LAYER <path> | REFERENCE [<name>] | FILE <path> [FORMAT <driver>]
            | SURFACE <name> (T0: a raster result triangulated and kept)
            | SELECTION | REPORT (V5: a query's katana_id selected, its rows reported)
```

- **The algorithm.** One to three words, by longest match, aliases taken.
- **GDAL's words** are anything GDAL's own command line takes for that
  algorithm: `--zfactor 2`, `--zfactor=2`, `-z 2`, positionals, `--co K=V`.
  `name=value`, when `name` is one of its arguments, is written
  `--name=value`: the form the other verbs' options take. The words end at
  the first unquoted FROM, TO, CONFIRM, OVERWRITE or PREVIEW, and GDAL words
  after a clause are refused (`GeoExecutor.GdalTokensAfterAClauseAreRefused`).
- **FROM** binds a dataset argument: the one it names (`FROM method LAYERS
  corridor`), or the first required input no FROM has taken, or - when no
  required one is left - the algorithm's one input, required or not. A
  pipeline's `input` is optional (its `read` step may name a file), so the
  example below with an unnamed FROM was refused as "no required dataset
  left" (`PipelineVerb.AnUnnamedFromBindsThePipelinesOneInput`). Fixing only
  the example was rejected: the rule surprised anyone who typed it, and an
  algorithm with one input leaves FROM nothing else to mean. Where two or
  more optional inputs are left, which is meant is not guessed and FROM must
  name it. A list argument takes several FROMs.
- **A source of a kind the argument does not read is refused** as the line
  is read, before anything runs: the drawing given to a raster input
  (`hillshade`'s), a raster or surface to a vector input
  (`GeoExecutor.ASourceOfAKindTheArgumentDoesNotReadIsRefusedBeforeItRuns`).
  GDAL answered the first "Unable to fetch band #1", from the worker. A
  file goes to any input; GDAL opens it for the kinds the input reads. A dataset GDAL's words also give is refused as given
  twice (`GeoExecutor.AnInputGivenTwiceIsRefused`). No dataset argument of
  GDAL's is named like a source or scope word
  (`GdalContract.NoDatasetArgumentIsNamedLikeASourceKeyword`), so the
  argument name is never mistaken for a source.
- **Without TO:**
  - features go to `LAYER gis/<the algorithm's last word>`;
  - a raster becomes `REFERENCE <the algorithm's last word>`;
  - printed text is printed;
  - an output GDAL's words name is written there, as `gdal` writes it
    (`GeoExecutor.AFileOnlyGdalLineWritesTheFile`).
- **Wrong targets are refused before anything runs.** A raster cannot go to
  a layer, nor features be a reference raster.
- **PREVIEW** resolves the sources, validates the arguments with GDAL, says
  what each scope took, and changes nothing
  (`GeoExecutor.PreviewChangesNothing`,
  `GeoExecutor.PreviewRefusesWhatARunWouldRefuse`).
- **A scope that takes nothing** is reported (`matched=0`, `ran=no`), and
  nothing runs (`GeoExecutor.AnEmptyScopeIsReportedNotAnError`).

Examples:

```
GDAL raster hillshade --zfactor=2 FROM FILE terrain.asc TO REFERENCE ground-shade
GDAL vector buffer --distance=5 --endcap-style=flat FROM LAYERS services WHERE TYPE=polyline TO LAYER gis/easement
GDAL vector layer-algebra intersection FROM input LAYERS lots FROM method LAYERS corridor TO LAYER gis/overlay
GDAL raster slope --unit=percent FROM SURFACE ground CELL 1
GDAL raster hillshade dem.tif shade.tif --zfactor 2
GDAL pipeline "read ! contour --interval 2 ! buffer 0.5 ! write" FROM SURFACE ground CELL 1 TO LAYER gis/bands
GDAL vsi delete "C:/tmp/x.tif" CONFIRM
```

A pipeline's text is one quoted word, set as the pipeline argument: GDAL's
own parser reads a pipeline from separate words. Its `read` and `write`
steps with no name use the bound input and the run's output.

### Replies

One record per line: a word saying what it is, then `key=value` fields in a
fixed order. A value holding a blank, a quote or an `=` is quoted
(`cad::recordValue`); an empty one is bare (`aliases=`). A measure to the
millimetre (`geo::fixed3`) that rounds to nothing has no sign: a surface of
a raster less its own surface said `zmin=-0.000`
(`GisRecords.AValueThatRoundsToZeroHasNoSign`). Errors go to stderr as
`error: <Code>: <message>`.

```
gdal algorithm="vector buffer" policy=safe seconds=0.003 cancelled=no
input arg=input source=drawing
scope arg=input scope=drawing matched=2 used=1 points=0 lines=1 polygons=0 skipped.text=1
output arg=output kind=vector target=layer layer=gis/buffer created=1 updated=0 deleted=0 skipped=0
output arg=output kind=raster target=reference id=1 name=hillshade raster=40x30 file="..." persisted=no
output arg=output kind=file target=file file=shade.tif
text lines=<n>           (then the n lines GDAL printed)
return code=<n>
warning text="..."
algorithm path="raster hillshade" policy=safe aliases= description="..."            (LIST)
group path="raster" algorithms=47 description="..."                                   (LIST)
arg name=zfactor short=z aliases= type=real required=no positional=no category=Base
    default= min=0 min_inclusive=no max= max_inclusive= choices= count=0..1 dataset=
    accepts= input=yes output=no description="..."                                    (HELP)
binding arg=input kinds=raster accepts=name,object sources=raster,surface,file list=yes required=yes
gdal version=3.13.2 release="Iowa City" proj=9.8.1 geos=3.15.0 raster_drivers=147 vector_drivers=82 algorithms=121
```

The scope record carries the shared `cad::scopeRecord` in its middle, so it
says what a scope took in the words `MODIFY` and `UTILITY` use.
`geo::parseRecords` reads the records back; it is how `katana_gdal_run` turns
the command line's own text into structured content. A record's kind is
every word before its first `key=` (`ifc exported`), and a bare word after
the fields is a field with no value, never glued to the next key.

### Schemas

`src/katana_app/geo/schema.hpp` turns the specs into JSON, generated at run
time and never written by hand:

- `argumentJson` is one argument;
- `argumentsSchema` is the JSON Schema of `katana_gdal_run`'s `arguments`:
  GDAL's choices as an enum, its bounds as `minimum` or `exclusiveMinimum`
  and `maximum` or `exclusiveMaximum`, list counts as `minItems` and
  `maxItems`, and its default;
- `inputsSchema` is the schema of its `inputs`, a Source object per input
  dataset.

`GDAL HELP ... JSON` and `katana_gdal_describe` read the same functions, and
the toolbox's forms the specs they are made from.

## The window

`GeoWorkbench` (`src/katana_qt/geo/geo_workbench.hpp`) is built as the online
and utility workbenches are. `MainWindow::buildGisActions` hands it the
window's document, interpreter, reference rasters and surfaces, and
`MainWindow::runWorkbenchLine` asks it first. So a typed line and a dialog's
line through `MainWindow::runVerbLine` reach it alike.

- **What answers at once is logged** (LIST, HELP, a refusal).
- **Anything that runs is a background job** (`src/katana_qt/jobs.hpp`),
  with progress and Cancel in the status bar. The job's Apply runs the
  executor's apply on the GUI thread, as one undo step.
- **Headless, the line waits for its job.** Under `--command`, `--run-line`
  or a script it calls `JobRunner::waitFor`, so its outcome is logged, and
  captured by `runVerbLine`, before the next line.
- **A dialog's status line** (`GisToolDialog::summary`) says what the step
  did: created, changed and deleted, on the layer the reply names, or "in
  place" when the reply is `target=in-place` and names none (it said "...
  deleted on ."; `GisClipDialog.ItsLineCutsAPipeInPlaceThroughTheExecutor`).
- **Interactively, it logs `job id=<n> title="..." state=started`.** When
  the job ends, `GeoServices::finished` and the listeners
  (`GeoWorkbench::addFinishedListener`) are told. That is how a dialog learns
  what its line did.

`qt_gdal_typed_on_the_command_line_runs_headless` and
`qt_gdal_line_through_the_windows_executor_is_one_undo_step_headless` run it
in the real window. `qt_widgets.GeoWorkbench.AnInteractiveRunSaysItStartedAndACancelAppliesNothing`
cancels a job before its result is delivered.

**Surfaces.** The window's surfaces are a `terrain::SurfaceStore`
(`include/katana/terrain/surface_store.hpp`), the one the session has too.
`MainWindow::syncSceneSurfaces` rebuilds the scene's list whenever the
store's revision moves. A second surface of a name is "name (2)": the store
keeps names unique so that `SURFACE <name>` finds one.

**Menus.** The packages add their items through `GeoMenus`
(`src/katana_qt/geo/menu_table.cpp`, a block each). It makes the GIS
menu's "Processing - GDAL", "Analysis - GDAL" and "Check - GDAL" sections,
and the Terrain menu's `terrainAnalysisMenu` and `terrainDemMenu`, the first
time an item is added, so an empty heading never shows. `Icon::Processing`
is the one icon the packages share. The two Terrain submenus are items of
the Terrain menu too, so each gets an icon (Processing, and ExportDem for
DEM) and a status tip when it is made: main's `--check-menus`
(`docs/desktop.md`) holds every item in every submenu to both, and the
first run of it on this branch listed these two as its only gaps.

**Dialogs.** The GIS menu's option dialogs are one file each now
(`gis_import_dialogs`, `gis_export_dialog`, `surface_raster_dialog`,
`dataset_info_dialog`, with `gis_dialog_support.hpp`), since four packages
each extend one of them.

## Decisions, and the alternatives rejected

- **The verb is GDAL, not PROCESS.** The argument vocabulary is GDAL's own,
  which agents already know from the `gdal` command line, and the GIS menu
  names its sections after GDAL already. The naming rule (no product names)
  is about other names, not this library's.
- **The C++ API, not the C API alone.** The same toolchain builds Katana and
  GDAL, and only the C++ API gives bounds with their inclusivity, which the
  forms and schemas need. The C API plus the usage JSON would do for a GDAL
  built by MSVC; that is recorded here, not built.
- **No lazy "stream" outputs for PREVIEW.** A streamed dataset computes when
  read, which suits a preview, but it is not thread-safe and keeps its
  inputs alive after the run. PREVIEW validates the arguments and says what
  the scopes took instead.
- **No GDALG (`.gdalg.json`) recipes as Katana's recipe format.**
  `Serialize()` cannot write an input bound as an object
  ("<input:unserialisable>"). A `.kcs` script replays any GDAL line with its
  drawing bindings. Opening someone else's `.gdalg.json` still works as a
  FILE source, because GDALG is a driver.
- **The logged line is Katana's.** For the same reason, the history, the
  undo list and a script keep the line a person typed, not GDAL's serialised
  command.
- **Sidecars are put back, not switched off** ("Sidecars" above).
- **WHERE alone is the selection, filtered.** The design said "WHERE alone
  means DRAWING". The one scope parser says no scope word is the selection,
  as MODIFY has always read it, and a second reading of the same words would
  be a second implementation of the one scope grammar (`docs/cad.md`). `FROM DRAWING WHERE
  ...` says the whole drawing.
- **A surface is sampled on the worker.** `bindRaster` hands back a
  `DeferredDataset`, made on the worker, rather than the grid itself: a 25M
  cell sampling is work, and the surface is shared and immutable.
- **A raster result is spilled, never kept in a `std::vector`.** For a
  reference target the run asks for `maxMemoryCells = 0`: the result is
  written once, where it is kept, as it is kept. It is still built whole in
  GDAL's MEM driver first and copied out ("Not done"): the review found the
  earlier wording, "never held", claimed more than the code does.
- **Which word names the output is read from GDAL's declarations**
  (`argumentsGiven`), in GDAL's order, without opening anything. Asking GDAL
  to parse the words would open the datasets they name - a URL, on the GUI
  thread - and fails anyway when the output is missing.

## What the lanes build against, where it differs from the plan's contract

The plan's contract (sections A to H) was written before the code; F0 kept
its names and shapes except here:

- `AlgorithmInfo::policyReason` is new: "listed", "update", "unclassified"
  or "container".
- The bridge has three more functions:
  - `argumentsGiven`, which arguments a tail gives, read from GDAL's
    declarations;
  - `checkTokens`, the refused words;
  - `stagedDatasetCount`, for the tests.
- `DrawingDatasetStats::warnings` is new.
- `geo::Tokens` is a struct: the words, and which were quoted.
- `bindRaster(Context&, const Source&)` returns a `DeferredDataset`.
- `inputRecord(arg, source)` and `scopeRecord(arg, bound)` make the input
  and scope records.
- `applyOutputs(Context&, const ApplyRequest&, RunOutputs&&)` takes an
  `ApplyRequest`: the target, the output's argument, the default name and
  the `ResultOptions`.
- `PrepareVerb` is `Result<Prepared>(Context&, const Tokens&, line)`.
- `geo::ownScratch` gives each front end - the window, each session - a
  scratch folder of its own (it was `defaultScratch`, one per process).
- I1 added, all source-compatible: `DrawingDatasetOptions::oneTable`,
  `tableName`, `styleFields` and `properties`; `interop::geo::featurePieces`;
  `VectorGeometry::arcs`; `GdalDataset::readTable` and `writeTables`, over
  the `processing::FeatureTable` of this contract.
- I2 added, all source-compatible: `include/katana/gis/formats.hpp`
  (`gis::formats`, `findFormat`, `formatOptions`, `readableExtensions`,
  `vectorSaveChoices`, `vectorWriterFor`, `isVirtualPath`, `isRemotePath`,
  `identifyContent`); `interop::archiveExtensions`; a `gis::writeZip` of
  several members. `GdalDataset::open` takes `/vsi` paths, URLs and
  archives; `vectorDriverForPath` is `vectorWriterFor`, so a `.kml` is
  LIBKML's. `interop::kindForPath` routes by content where it can look.
- `RasterOverlay::facts` is declared, not yet filled (D2).

## Contract tests

GDAL's algorithm framework is provisional, and MSYS2 upgrades GDAL on a
rolling basis. Each package therefore pins the paths and arguments it binds,
with `expectArgument(path, name, ArgType, required)`
(`tests/geo/contract_support.hpp`). F0 pins its own in
`tests/geo/test_processing_contract.cpp`: the inputs every raster algorithm
took as a dataset list in 3.13, `output-string`, `return-code`, `pipeline`,
and the name-only inputs.

The tests: `tests/geo/` (suite `katana_geo_tests`) holds the bridge, policy,
contract, bindings, surface store and executor tests. It also holds the
verb's, run through a Session (`GeoSession.AGdalLineRunsThroughTheSessionAndItsRasterIsAReference`).
`src/katana_app/geo/cli/gdal.cmake` holds the `cli.gdal_*` tests,
`tests/geo/headless/gdal.cmake` the window's, and
`tests/qt_widgets/geo/test_geo_workbench.cpp` the workbench's. Every expected
value is worked by hand and quoted beside it:

- hillshade of flat ground at altitude 45: 1 + 254 sin 45 deg = 180.6, so
  181;
- the plane's slope: 5 %, within 5e-4. GDAL reads the grid as Float32 (ulp
  7.6e-6 near 100), and Horn's gradient carries 3.8e-6 of that into dz/dx:
  3.8e-4 in percent;
- the plane's aspect: 270 deg;
- a 100 m line buffered 1 m either side, flat caps: 200 m2;
- a 4 m corridor over a 50 m lot: 200 m2;
- a linear grid of points on z = 100 + x/10 + y/20, at (52.5, 47.5):
  107.625.

## Packages

One section per package of the geoprocessing plan. A package writes only its
own, as it edits only its own block of `verb_table.cpp`,
`mcp_geo_tools.cpp`, `bindings.cpp` and `menu_table.cpp`.

The directories a package adds files to are globbed, so no package edits a
CMake list:

- `src/katana_io/geo/*.cpp` (katana_io);
- `src/katana_interop/geo/*.cpp`, with `include/katana/interop/geo/`;
- a geo folder beside katana_cad's sources, for domain edits that need no
  GDAL, globbed by `src/katana_cad/CMakeLists.txt` and empty so far;
- `src/katana_app/geo/*.cpp` and its `cli/*.cmake`;
- `src/katana_qt/geo/*.cpp`, compiled into the window and the widget tests;
- `tests/geo/test_*.cpp`, `tests/geo/headless/*.cmake`, and
  `tests/qt_widgets/geo/*.cpp`.

### F0: GDAL algorithm bridge, bindings, one geoprocessing executor, the GDAL verb, MCP tools

Built: everything above. Its window surface is the typed GDAL line, run as a
job. Its menu item is X1's toolbox, which is to reach main with it. The MCP
tools are in `docs/mcp.md`.

### X1: GIS > GDAL Toolbox window

Built: GIS > Processing - GDAL > GDAL Toolbox (`gdalToolbox`, opening
`gdalToolboxDialog`; `src/katana_qt/geo/gdal_toolbox_dialog.hpp`), the GDAL
verb's menu item. It runs nothing itself: it writes the GDAL line its fields
describe and hands it to the window's one executor, so its CLI and MCP
surfaces are the verb's and `katana_gdal_run`'s.

- **The catalogue** is a tree, group then algorithm, filtered by a search
  over paths, aliases and descriptions; the algorithm named exactly as typed,
  else the first shown, is chosen as the search is typed. A Confirm
  algorithm says "confirm" beside it.
- **The form is GDAL's declaration** (`argument_form.hpp`), read when the
  algorithm is chosen: a check box, a choice of GDAL's choices, a spin box
  carrying GDAL's bounds, a comma-separated line for a list. A number starts
  "not given" unless GDAL has a default; only what differs from GDAL's
  default is written. An exclusive bound is refused at its end value when the
  line is written (`contour --interval=0`), so nothing runs. Base arguments
  show and the rest fold away (`gdalToolboxAdvanced`). One given of an
  exclusion group disables the others; a dependent argument waits for what it
  depends on. What the executor refuses (`--quiet`) is not offered, nor what
  the output's target says (`--output-format`, `--overwrite`, `--append`,
  `--update`, `--overwrite-layer`, `--upsert`).
- **Datasets are bound by pickers** (`binding_picker.hpp`), each offering only
  what can be one: the drawing by Global Modify's own scope and filter
  controls (for a vector), a reference raster or a surface (for a raster), or
  a file. An optional input is "Not given" until it is; an argument that takes
  several datasets gets a list. The first required input is bound by a FROM
  without its name, as the verb binds it; every other names its argument.
- **The output** goes where its kinds allow: a layer (vector), a reference
  raster (raster), a file with its format and OVERWRITE; a surface is listed
  and not offered until the terrain session's store takes a raster result.
- **Confirm** shows only for a Confirm algorithm, and the line waits for it.
- **Help** opens GDAL's page for the algorithm; headless it is said, not
  opened.
- **The algorithm last chosen** is remembered per user (QSettings) and
  chosen again; a store that cannot be read is no algorithm.

`gdalToolboxLine` is the pure function the line comes from. Tests:
`tests/qt_widgets/geo/test_gdal_toolbox.cpp` (the search, the form's bounds
and choices, the exact line, the pickers' kinds, the exclusive minimum, the
exclusion group, Confirm, optional and listed inputs, a file output, Run and
Preview through a stub runner) and `qt_gdal_toolbox_buffers_a_drawn_line_headless`
(`tests/geo/headless/toolbox.cmake`: a 100 m line buffered 1 m either side
with flat caps from the toolbox, 200 m2 by hand).

Its menu letter is X (GDAL Toolbo&x): every other letter of the name is
another GIS item's once the lanes are merged, and
`qt_every_shortcut_and_menu_letter_reaches_one_thing_headless` refuses a
letter shared in one menu. Terrain's DEM submenu is D&EM for the same reason
(D is Surface From Drawing's).

Not done: a CRS argument (`raster reproject`'s `--output-crs`, `--input-crs`
...) is a plain line, typed as GDAL reads it. The plan offered GDAL's own
suggestions there, as the output format has (`processing::suggest`). GDAL
gives the authorities for an empty value and 7216 `<code> -- <name>` entries
after `EPSG:` (GDAL 3.13.2, measured with `gdal completion`), so it wants a
completer that asks again as the text changes and writes `EPSG:<code>` back.
That is not built.

### X2: Toolbox pipeline tab

Built: the GDAL Toolbox's Pipeline tab (`src/katana_qt/geo/pipeline_builder.hpp`).
Its CLI and MCP surfaces are the GDAL verb's: it writes
`GDAL pipeline "read ! <step> ... ! write" FROM input <source> TO <target>`.

#### Pipelines

- **The steps are GDAL's own.** The pipeline's usage lists its steps
  (`pipeline_algorithms`); each offered is the catalogue algorithm under
  raster or vector of that name, whose form (`ArgumentForm`, restricted to
  the arguments a step takes) edits it. `read` and `write` are the
  pipeline's ends, bound by FROM and TO; `external` runs a program and `tee`
  is a pipeline of its own, so neither is offered, and a text naming
  external is refused as the verb refuses it.
- **A step is offered only where it reads what the pipeline makes.** After a
  raster, raster steps; after `contour` or `polygonize`, vector steps; a
  file may be either until a step says. So a mixed raster-to-vector pipeline
  is built step by step, and the output offers a layer or a reference raster
  as the last step makes one.
- **The text is shown and may be edited.** An edit is read back into steps
  (`parsePipeline`): options as `--name=value`, `--name value`, `-n value`,
  flags, and values by position in GDAL's order, each checked by the step's
  form. A text that cannot be read into steps still runs as typed, and the
  status says why the steps did not follow. When the line itself is
  refused - an `external` step - the status says it cannot run and why,
  beside the disabled Run; it said "It runs as typed"
  (`PipelineBuilder.APipelineWithAnExternalStepCannotBeBuilt`).
- **A value with a blank cannot be carried.** The pipeline is one quoted
  word on the line, and a line has no quote inside a quoted word.
- **A recipe is the line in a script** (`gdalPipelineSave` adds it to a
  `.kcs`; File > Run Script replays it), not a `.gdalg.json`, which cannot
  hold a dataset bound from the drawing (see "Decisions").
- **GDAL's usage JSON is not JSON.** It writes a default of Infinity (vector
  grid's radius) bare; the step list is read after such numbers are made
  null.

A pipeline ending in `write` now binds its output as any run's (the bridge,
"The output"): before, a pipeline to a layer or a reference raster failed
with "write: Positional arguments starting at 'OUTPUT' have not been
specified", because a pipeline has an `output-string` - for the steps that
print - and the bridge took that to mean it prints. It prints only when its
last step is `info`, `compare` or `export-schema`, the steps GDAL declares
with an output-string (`PipelineContract.TheStepsThatPrintAreTheOnesGdalDeclaresSo`
names any it adds; `PipelineVerb.*` fail without the fix).

Tests: `tests/qt_widgets/geo/test_pipeline_builder.cpp`,
`tests/geo/test_pipeline_verb.cpp`,
`cli.gdal_pipeline_contours_then_buffer_from_a_raster_to_a_layer`
(`src/katana_app/geo/cli/pipeline.cmake`) and
`qt_gdal_toolbox_pipeline_tab_runs_its_steps_headless`
(`tests/geo/headless/pipeline.cmake`). By hand: plane.asc spans
100.025 .. 101.975, so contours every 0.5 m are 100.5, 101.0 and 101.5, each
buffered into one area: three.

### T0: Terrain session: one surface store and SURFACE verbs on every front end

Built: `SURFACE LIST | INFO | REMOVE | FROM | EXPORT`
(`src/katana_app/geo/surface_verbs.cpp`, its helpers in
`src/katana_app/geo/terrain_verbs.hpp`), `TO SURFACE` in `bindings.cpp`'s T0
block, `katana_terrain_list` (`docs/mcp.md`), and the window's Terrain >
Surface From and GIS > Export Surface as DEM, which build SURFACE lines
(`src/katana_qt/surface_raster_dialog.hpp`, with
`src/katana_qt/geo/terrain_dialog_support.hpp` for what the terrain dialogs
share). The rules that make survey points and breaklines of drawing data
moved to `cad::geo::surfaceInput`. `docs/terrain.md`, "Surfaces on every
front end", has the grammar, the records and the decisions.

Where it differs from the plan:

- `SURFACE FROM <scope>` takes the scope as FROM does
  (`FROM DRAWING WHERE DRAWN`, `FROM LAYERS ground`); the plan's `SURFACE FROM
  DRAWING [<scope>]` is also read. `FROM FILE <path>` reads a DEM that is not
  a reference raster.
- A heightless entity is left out, not put on the datum as the window did;
  the comparison with the old code is on levelled data, and on the site plan
  sample the test shows the difference.
- The DEM is written through GDAL's `raster convert`, not through
  `GdalDataset::writeRaster`, which is left as it was for its other callers.
- One Surface From dialog for the three items, named by the `<d>` rule
  (`surfaceFromScope`).
- `GeoServices` gained `views` (the scope controls' View choice) and
  `GeoWorkbench::window`, the parent the packages' dialogs are made under.
- TO SURFACE triangulates in its apply, since the GDAL verb's work ends with
  the run; SURFACE FROM triangulates in its work.

### T1: CONTOUR from a surface or an elevation raster

Built: `CONTOUR` (`src/katana_app/geo/contour_verbs.cpp`), its conversion,
clip and command in `include/katana/interop/geo/contour_entities.hpp`, and
Terrain > Analysis > Contours (`terrainContours`, `contoursDialog`,
`src/katana_qt/geo/contours_dialog.hpp`). `docs/terrain.md`, "Contours", has
the grammar, the records and the decisions. What T1 to T3 share - a raster
handed from one GDAL step to the next (`RasterChain`), the source reader
(`bindTerrainSource`) and the areas a scope's closed shapes make
(`bindAreas`) - is in `src/katana_app/geo/terrain_steps.cpp`, declared in
`terrain_verbs.hpp`; the dialogs' shared pieces are in
`terrain_dialog_support.hpp`. No MCP tool: `katana_run_commands` runs the
line (`docs/mcp.md`).

Where it differs from the plan:

- Both engines' lines are cut at the boundary by one clipper
  (`clipContours`); a raster is cut by GDAL to the areas' box first, not by
  the boundary itself, whose no-data edge would end its contours short.
- The dialog has a `contourClip` box that turns the scope on.
- The raster engine refuses more than the tracer's 100 000 levels too,
  judged on a strided sample before GDAL runs.

### T2: Terrain shading: hillshade, colour relief, slope shading

Built: `RASTER SHADE` (`src/katana_app/geo/shade_verbs.cpp`), the ramps in
`include/katana/interop/geo/colour_ramps.hpp`, and Terrain > Analysis >
Terrain Shading (`terrainShading`, `terrainShadingDialog`,
`src/katana_qt/geo/terrain_shading_dialog.hpp`). `docs/terrain.md`,
"Shading", has the grammar, the records and the decisions. The picture is a
derived reference raster through F0's `applyOutputs` (TO REFERENCE), its
`displayStyle` set after. No MCP tool: `katana_run_commands` runs the line,
and `katana_terrain_list` lists the picture with its derivation.

Where it differs from the plan:

- The styles are rendered into the picture, not drawn from the elevation
  raster at draw time; `displayStyle` names the picture.
- Every style ends as RGBA, a hillshade included, so the reader does not
  stretch it.
- Slope shading is in degrees; the diverging ramp is symmetric about zero
  unless `range=` says; a `grey` ramp colours `plain`.

### T3: Slope and aspect with slope-class areas

Built: `RASTER SLOPE` and `RASTER ASPECT` (`src/katana_app/geo/slope_verbs.cpp`)
and Terrain > Analysis > Slope and Aspect (`terrainSlope`,
`slopeAnalysisDialog`, `src/katana_qt/geo/slope_analysis_dialog.hpp`).
`docs/terrain.md`, "Slope and aspect", has the grammar, the records and the
decisions. The slope raster goes through F0's `applyOutputs` (TO
REFERENCE), the class areas through `resultCommand` (one undo step). No MCP
tool: `katana_run_commands` runs the line (`docs/mcp.md`).

Where it differs from the plan:

- The class breaks begin at 0 and end open, not at the data's range: a
  slope is never below 0, and neither end then needs a pass over the data.
- The slope raster keeps the values; its display copy is a coloured picture
  of them.
- With a scope, the class areas are cut to the shapes exactly (`vector
  clip`), since GDAL's raster clip keeps every cell a shape touches.
- The dialog has a `slopeClip` box that turns the scope on, and a
  `slopeName`.

### T4: Statistics by area, sampling and drape

Built: `RASTER ZONAL` (`src/katana_app/geo/zonal_verbs.cpp`), `RASTER SAMPLE`
and `DRAPE` (`src/katana_app/geo/drape_verbs.cpp`), recorded in
`docs/terrain.md`, "Statistics by area" and "Sampling and drape". Window:
Terrain > Analysis > Statistics by Area (`terrainZonal`, `zonalStatsDialog`)
and Drape and Sample Heights (`terrainDrape`, `drapeDialog`). katana_cli
and katana_mcp: the verbs, through the session (`katana_run_commands`); no
tool of their own. Pieces of their own:

- `include/katana/gis/raster_sampling.hpp`: a raster's value at a point,
  through `GDALRasterInterpolateAtPoint` - the C API, since no algorithm of
  the framework samples at arbitrary points without a vector round trip
  (`raster pixel-info` writes a GeoJSON per call).
- `include/katana/cad/geo/drape.hpp`: the drape as a domain edit, given the
  ground as a callback, so katana_cad never sees GDAL.
- `src/katana_app/geo/analysis_support.hpp`, shared with T5: the ground
  (a surface on its triangles, or a raster file), points written `x,y`, and
  keyword-and-value words (`AT x,y`) taken out of a line before the rest
  goes to the one scope parser through `vector::readVerbWords`.
- `GeoServices::pickPoint` (`src/katana_qt/geo/point_pick.hpp`): the next
  left click in any plan view, for a dialog's line.

Both in-place applies compare their targets with the copies taken at
prepare (`geo::unchangedSince`) and refuse when they differ. ZONAL writes
through `ResultMode::SetProperties`, and in the same step removes a
`<prefix>_<stat>` an earlier run left where this run has no number.
`ZonalContract.TheVerbStillFindsTheArgumentsItBinds` pins `raster
zonal-stats`'s `input`, `zones`, `stat`, `include-field` and `pixels`.

Where it differs from the plan:

- The zones table carries `katana_id` only (GDAL includes the fields it is
  asked for, and the zones' own properties are theirs already).
- The statistics offered are the single-number ones; `pixels=centre` is
  GDAL's `default`.
- The pick is an event filter on the plan views, not the tool host the
  "Not done" entry below once proposed: a tool ends in a command, and a
  pick changes nothing.
- The dialog adds `zonalOverwrite` and `zonalPreview`; the drape dialog's
  Sample tab adds `samplePoint`, `sampleAdd`, `sampleRemove` and
  `sampleMethod` to the plan's `sampleSource`, `samplePoints` and
  `samplePick`.

### T5: Viewshed and line of sight

Built: `RASTER VIEWSHED` and `LOS` (`src/katana_app/geo/viewshed_verbs.cpp`),
recorded in `docs/terrain.md`, "Viewshed and line of sight". Window:
Terrain > Analysis > Viewshed and Line of Sight (`terrainViewshed`,
`viewshedDialog`), whose Pick buttons use T4's `GeoServices::pickPoint`.
katana_cli and katana_mcp: the verbs, through the session
(`katana_run_commands`); no tool of their own.

- Each observer is one `raster viewshed` run, bound with its own copy of
  the input (never a dataset shared between runs), every option passed
  explicitly - `curvature-coefficient` and `visible-value` included - and
  the runs are unioned natively on the input's grid.
- An observer off the raster is refused by the work, which is where the
  raster is first opened.
- The line of sight is `terrain::lineOfSight`
  (`include/katana/terrain/line_of_sight.hpp`), fed from the analysis
  support's ground: a TIN, or a raster through `gis::RasterSampler`.
- `ViewshedContract.TheVerbStillFindsTheArgumentsItBinds` pins `raster
  viewshed`'s `input`, `position`, `height`, `target-height`,
  `max-distance`, `curvature-coefficient` and `visible-value`, and `raster
  polygonize`'s `attribute-name`.

Where it differs from the plan:

- The viewshed test's fixture is a 1 m wall under a 1.7 m eye, not a 5 m
  wall: with the eye below the wall top no shadow ends, so the plan's
  similar-triangles value is not a viewshed's (`docs/terrain.md`).
- The dialog adds `viewshedObserver`, `viewshedAdd`, `viewshedRemove`,
  `viewshedUseScope`, `viewshedCurvature` and `viewshedName` to the plan's
  fields, and the Line of Sight tab `losHeight`, `losTargetHeight` and
  `losCurvature`.

### T6: RASTER GRID: survey points to a DEM

Built: `RASTER GRID` (`src/katana_app/geo/grid_verbs.cpp`), recorded in
`docs/terrain.md`, "Gridding points to a DEM". It grids the points a scope
takes with `vector grid <method>`, heights from the geometry
(`DrawingDatasetOptions::requireHeights`) or a property (`--zfield`), and
keeps the DEM as a derived reference raster. Window: Terrain > DEM > Grid
Points to DEM (`gridDemDialog`). katana_cli and katana_mcp: the verb, through
the session (`katana_run_commands`). Its contract test pins `vector grid
linear`'s `input`, `output` (a name only, which the bridge stages),
`extent`, `resolution`, `size`, `zfield`, `nodata`, `input-layer` and
`radius`, and `invdist`'s `power`.

Where it differs from the plan: the extent is grown outwards to whole cells
(GDAL stretches the cells to fit an extent, measured), and `TO SURFACE`
waits for T0's block in the bindings, so the dialog's `gridToSurface` is
not offered yet. The shared pieces of the DEM verbs are
`src/katana_app/geo/dem_support.hpp`, and of their dialogs
`src/katana_qt/geo/geo_dialog_support.hpp`; `GeoServices::views` is new, so
a dialog's scope offers the window's views.

### T7: DEM tools: mosaic, clip, fill, footprint, reproject, difference

Built: `RASTER MOSAIC`, `CLIP`, `FILL`, `FOOTPRINT`, `REPROJECT` and
`DIFFERENCE` (`src/katana_app/geo/dem_verbs.cpp`), recorded in
`docs/terrain.md`, "The DEM tools". Window: Terrain > DEM > DEM Tools
(`demToolsDialog`, a tab each). katana_cli and katana_mcp: the verbs, through
the session. The contract test pins `raster mosaic`'s `input`, `output`,
`resolution` and `absolute-path`; `raster clip`'s `bbox` and `like`;
`raster fill-nodata`'s `max-distance`, `smoothing-iterations` and
`strategy`; `raster footprint`; `raster reproject`'s `output-crs`,
`input-crs`, `like`, `resampling` and `resolution`; and `raster calc`'s
`input`, `calc` and `dialect`, with `builtin` among its choices.

Where it differs from the plan:

- A mosaic is written with `SAVE <file>`, not `save=`: an option's value is
  one unquoted word, and a path may hold blanks.
- DIFFERENCE aligns with the first raster's extent, size and system said
  outright, not `--like`, which GDAL ignores silently for rasters with no
  CRS (measured); and it aligns only when the grids differ.
- CLIP by boundaries lets GDAL bring the boundaries into the raster's
  system rather than refusing two known systems that differ: that is the
  right answer, not a guess. REPROJECT refuses a raster with no system,
  which GDAL would "reproject" unchanged.
- The binding picker (`src/katana_qt/geo/binding_picker.hpp`, X1's) was
  built here, since the DEM tools pick rasters as the toolbox picks its
  datasets.

### V1: GIS BUFFER and GIS DISSOLVE

```
GIS BUFFER [<scope>] distance=<m>|distance=prop:<key> [side=both|left|right]
           [caps=round|flat|square] [joins=round|mitre|bevel] [dissolve[=k1,k2]]
           [TO LAYER <path>] [PREVIEW]
GIS DISSOLVE [<scope>] [by=k1,k2] [keep=identical] [TO LAYER <path>] [REPLACE] [PREVIEW]
```

Easements, setbacks, corridors and clearance zones around what the scope
takes, and areas merged by what their properties say
(`src/katana_app/geo/buffer_verbs.cpp`). The window's items are GIS >
Analysis - GDAL > Buffer... and Dissolve... (`gisBuffer`, `gisDissolve`;
`src/katana_qt/geo/buffer_dialog.hpp`, `dissolve_dialog.hpp`). An agent
types the same lines through `katana_run_commands`.

**BUFFER** runs `vector buffer` on the scope's feature set.

- **The distance** is one length for all, or `distance=prop:<key>`: each
  entity by the number its own property holds (a clearance by voltage). The
  entities are grouped by value and each group is one run. An entity whose
  property gives no number, or 0, is skipped and counted
  (`skipped.no_distance=`).
- **Negative** sets an area in: a 3 m mitred setback of a 20 x 40 lot is 14 x
  34 (`GisBuffer.AMinusThreeMetreMitreSetbackOfATwentyByFortyLotIs14By34`).
  GEOS buffers a line or a point inwards to nothing; that is counted
  (`empty=`), never drawn (`GisBuffer.ALineBufferedInwardsIsNothingAndSaysSo`).
- **One side** of a line is `side=left|right`, walking along it: left of a
  west-to-east line is north (`GisBuffer.LeftOfAWestToEastLineIsNorth`).
- **Arcs.** GDAL's default of 8 segments a quadrant strays 0.096 m from a 5 m
  arc. The quadrant's segments are the fewest whose chords keep within the
  1 mm curve tolerance the bindings chord with, by the one sagitta rule
  (`geometry::sagittaChordCount`); the reply says how many
  (`quadrant_segments=`).
- **A round buffer of a point is a circle.** Drawn as one - a `Circle2` of
  the distance's radius, exactly, with the point's properties - rather than
  as chords (`GisBuffer.ARoundPointBufferIsAnExactCircle`: pi x 25 m2).
- **dissolve** merges the results: `DISSOLVE` all of them, `dissolve=k1,k2`
  those whose properties agree. It is DISSOLVE's own chain below.
- **The result** is closed polylines on `gis/buffer` (or TO LAYER), holes
  tagged as every result's are, with the source's properties, `gis.op` and
  `gis.source`. `vector buffer` drops Z: a buffer is in plan.

**DISSOLVE** merges the areas in the scope.

- **`vector dissolve` alone does not group.** It unions only the parts within
  each feature (measured). So merging by a property is `vector combine
  --group-by` and then `vector dissolve`; the combine's geometry collection
  comes back as the feature's parts.
- **Areas only.** Closed polylines, closed curve polylines, circles, whole
  ellipses and closed splines (drawingDataset's areas, with their tagged
  holes); the points and lines a scope takes are left as they
  are, and a warning says how many.
- **`keep=identical`** keeps each property the whole group holds alike
  (`--add-extra-fields=always-identical`); the rest are left behind
  (`GisDissolve.KeepIdenticalKeepsOnlySharedValues`).
- **`REPLACE`** deletes every area that went in - the joined holes too - in
  the same undo step as the result
  (`GisDissolve.ReplaceDeletesTheSourcesInTheSameUndoStep`). It is an
  in-place apply, so it compares those areas with the copies taken at
  prepare and refuses when one changed while the job ran
  (`GisDissolve.AnInPlaceReplaceRefusesADrawingChangedWhileItRan`). A
  dissolve's result carries no `katana_id` of a source, which is why the
  verb deletes what its scope took.

**Replies.**

```
gis op=buffer seconds=0.004 cancelled=no
scope arg=input scope=drawing matched=1 used=1 points=0 lines=1 polygons=0
output arg=output kind=vector target=layer layer=gis/buffer created=1 updated=0 deleted=0 skipped=0
buffer distance=1 side=both caps=flat joins=round quadrant_segments=18 dissolve=no groups=1 features=1 circles=0 area=200.000 empty=0
dissolve by=owner keep=identical areas=4 replace=no groups=2 area=8000.000
```

The first three are the GDAL verb's own records (the `gis` record is the
`gdal` one of a curated verb); the last says what was made and measures it:
the replies carry the area, not the entities, which would go stale. A scope
that took nothing answers `ran=no` and runs nothing; PREVIEW answers `gis
op=buffer preview=yes`, the scope record and `preview valid=yes changed=no`.

**What the vector verbs share** (`src/katana_app/geo/vector_support.hpp`),
written here for BUFFER and DISSOLVE and used by every vector verb after them:

- **Their own words are read as MODIFY reads SET and PREVIEW**: taken out of
  the line wherever they stand, and the rest handed to the one scope parser.
  So `GIS BUFFER DRAWING WHERE TYPE=line distance=1` works: the filter ends
  where its conditions do
  (`GisBuffer.OptionsAfterAWhereFilterAreTheVerbsOwn`).
- **The result's layer is `TO LAYER <path>`**, the one target parser's word,
  not the plan's `layer=`. `LAYER=` is a WHERE key: after a filter,
  `layer=gis/easement` would silently be read as "on layers named
  gis/easement" and match nothing. An option key is never a WHERE key.
- One run of a vector algorithm on a feature set (`vector::runVector`), the
  measures (`vector::measure`), a result of several tables made one
  (`vector::mergedTable`), and one step executed and framed
  (`vector::executeStep`).

**The dialogs' frame** (`src/katana_qt/geo/gis_tool_dialog.hpp`) is shared
by every GIS analysis and check dialog: the scope controls
(`ScopeFilterWidget`), the dialog's fields, `<d>Command`, `<d>Preview`,
`<d>Run`, `<d>Status`, `<d>Reply`. Run hands the line to the window's one
executor. Interactively the runner answers `job id=<n> ... state=started`
and the dialog shows the job's reply when the workbench says it ended
(`qt_widgets.GisBufferDialog.AnInteractiveRunShowsItsJobsReplyWhenItEnds`);
headless it is there when Run returns (`qt_gis_buffer_dialog_headless`,
`qt_gis_dissolve_dialog_headless`). The scope's View choice lists the
window's views, from `GeoServices::views` - the workspace, as every other
workbench of the window is given it. (Built alone, this lane found the
workspace as the window's one `ViewWorkspace` child; the terrain lane and the
toolbox lane each added a views service, and when the lanes were merged the
workspace pointer, the form the window's other services structs already had,
became the one.)

**Tests.** `tests/geo/test_buffer_and_dissolve.cpp`, every value by hand: a
100 m line 1 m either side with flat ends, 200 m2; the 14 x 34 setback,
476 m2; clearances of 1 and 3 m on 100 m lines, 200 and 600 m2; a 5 m circle,
78.540 m2; two crossing strips dissolved, 400 - 4 = 396 m2; two 50 x 40 lots
merged, 4000 m2. `GisBufferContract.TheArgumentsTheVerbsBindAreGdals` pins
`vector buffer`, `combine` and `dissolve`. The dialogs:
`tests/qt_widgets/geo/test_buffer_dialog.cpp`; katana_cli:
`cli.gis_buffer_of_a_drawn_line_creates_two_hundred_square_metres`,
`cli.gis_dissolve_of_two_adjacent_lots_is_one_area_of_four_thousand_square_metres`.

**Not done.** Lines are not dissolved (merged into multi-lines): the verb
merges areas. The distance cannot come from an expression, only a property.

### V2: GIS OVERLAY: polygon booleans between two scopes

```
GIS OVERLAY intersection|difference|union|symdifference|identity|update|clip
    <scope> WITH (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
    [keep=a,b|all|none] [keepwith=a,b|all|none] [TO LAYER <path>]
    [csv=<file>] [OVERWRITE] [PREVIEW]
```

The union, intersection and difference `geometry/polygon.hpp` says Katana
does not provide, through GDAL's `vector layer-algebra`
(`src/katana_app/geo/overlay_verbs.cpp`): easement area per lot, net
developable area, lots split by a zone, pipe length per lot. The window's
item is GIS > Analysis - GDAL > Overlay... (`gisOverlay`,
`src/katana_qt/geo/overlay_dialog.hpp`).

- **Two scopes, one parser.** The subject's words end at WITH, which is no
  scope word, and the overlay's begin after it; both are read by the one
  scope parser. The overlay may instead be a file, read on the worker
  through GDAL's own `vector filter` (its LAYER, and `where=` as GDAL's SQL).
- **Operations.** Katana's words are GDAL's but for difference (`erase`) and
  symdifference (`sym-difference`); GDAL's own are taken too.
- **One layer at a time.** layer-algebra reads one input layer and one
  method layer ("Cannot get input layer ''" for a dataset of several;
  measured), so each of the subject's tables is a run of its own and the
  overlay is its areas. union, symdifference and update give back the
  overlay's pieces the subject misses, which a second run would repeat: they
  take the subject's areas only, and say how many lines and points they left
  out.
- **Lines against areas.** A line comes back cut at the areas' edges, which
  is how `GisOverlay.ALineAgainstLotsGivesItsLengthPerLot` measures a 100 m
  pipe as 50 + 50.
- **Properties.** GDAL names what it carries `input_<field>` and
  `method_<field>`. A name only one side has is given back as it was
  (`owner`); one both sides have keeps GDAL's prefix (`input_name`,
  `method_name`). The subject's `katana_id` becomes `gis.source` and the
  overlay's `gis.with`, so each piece says which two entities it came from.
  Dropped from either side, prefixed or not: the drawing's bookkeeping
  fields (`layer`, `style`, `colour`, `type`) and `gis.*` provenance.
  difference, update and clip give the subject's fields without GDAL's
  prefix, and the bookkeeping came back in their rows
  (`GisOverlay.ARowCarriesTheLotsPropertiesNotTheDrawingsBookkeeping`); an
  overlay made by another verb lent its `gis.op` and `gis.source`, which on
  a row read as the piece's own (`gis.source=<the pipe>` beside
  `entity=<the lot>`) though the piece drawn is given its own
  (`GisOverlay.AnOverlayMadeByAnotherVerbLendsItsIdNotItsProvenance`).
  `with=` names the overlay piece. `keep=` and `keepwith=`
  choose the fields (GDAL's `--input-field`, `--method-field`); `katana_id`
  is always carried.
- **Rows.** One `row` record per piece: `entity=`, `with=`, what it carries,
  and its `area=` or `length=`. `csv=<file>` writes them too; a file already
  there is replaced only with OVERWRITE.

```
row entity=1 input_kind=lot input_name=A owner=Smith with=3 method_kind=corridor method_name=C area=200.000
overlay operation=intersection features=2 area=400.000 length=0.000
```

A second scope that takes nothing is reported (`scope arg=method ...
matched=0`, `ran=no`; `GisOverlay.AnEmptySecondScopeIsReported`).

**The dialog** has two sets of the scope controls: the frame's for the
subject (`gisOverlayScope`, ...) and a second for the overlay
(`gisOverlayWithScope`, `gisOverlayWithScopeDrawing`, ...) - the plan named
them `gisOverlaySubject` and `gisOverlayWith`, but the shared widget names
itself `<prefix>Scope`, so the subject keeps the frame's name every dialog
has. `gisOverlayWithSourceFile` chooses a file instead.

**Tests.** `tests/geo/test_overlay_verb.cpp`, on `tests/geo/data/lots.geojson`
drawn, by hand: a 4 m corridor over each 50 m lot, 200 m2; each lot less it,
1800 m2; the union, 2000 + 2000 + 480 - 400 = 4080 m2; identity keeps the
lots' 4000 m2; the file's corridor cuts as the drawn one does.
`GisOverlayContract.TheArgumentsTheVerbBindsAreGdals` pins the operations and
fields. The dialog: `tests/qt_widgets/geo/test_overlay_dialog.cpp`;
katana_cli:
`cli.gis_overlay_of_a_corridor_on_two_lots_gives_two_hundred_square_metres_each`,
`cli.gis_overlay_with_a_file_reads_its_corridor`.

**Not done.** A file's coordinate system is not compared with the
project's: the bridge has no equivalence test yet (text comparison of WKT
is wrong), so a file in another CRS is overlaid as it is.

### V3: GIS HULL and GIS CLIP

```
GIS HULL [<scope>] [convex | concave=<0..1>] [holes] [TO LAYER <path>] [PREVIEW]
GIS CLIP [<scope>] BY (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
         [TO LAYER <path> | REPLACE] [PREVIEW]
```

The boundary around what the scope takes, and what it takes cut to a
boundary (`src/katana_app/geo/hull_clip_verbs.cpp`). The window's items are
GIS > Analysis - GDAL > Boundary Around Features... and Clip to Boundary...
(`gisHull`, `gisClip`; `src/katana_qt/geo/hull_dialog.hpp`, `clip_dialog.hpp`).

**HULL** takes every point and vertex of the scope (curves as their chords),
once each.

- **Convex** - the default - is Katana's own `geometry::convexHull`: GDAL
  would add nothing. The corners and centre of a square give the square,
  its centre inside (`GisHull.ConvexHullOfTheCornersAndCentreOfASquareIsTheSquare`).
- **`concave=<ratio>`** is GDAL's `vector concave-hull` of all the points
  as ONE multipoint - GDAL hulls each feature, and a hull per point is no
  boundary. GEOS's ratio runs from 0, the tightest, to 1, the convex hull;
  `holes` lets it leave a gap inside. On a 1 m grid over an L of 36 m2 the
  convex hull is 68 m2 and a ratio of 0.1 gives 36.5 m2: the L and the half
  square the hull cuts across its inner corner
  (`GisHull.ConcaveHullOfAnLShapedPointSetIsSmallerThanItsConvexHull`).
- Fewer than three points, or all in a line, bound no area: nothing is drawn,
  and a warning says why.

**CLIP** runs `vector clip --like`, the boundary bound as the like dataset:
the second scope's areas, or a file with GDAL's own `--like-layer` and
`--like-where`. It clips to the like dataset's geometries, not only their
bounds (measured), and gives each piece a feature of its own.

- **Drawn beside** (the default, or TO LAYER): the pieces on `gis/clip`, the
  entities left whole (`GisClip.ClipOfALineByABoxHasHandComputedLength`).
- **REPLACE, in place**: an entity cut keeps its id on its largest piece and
  the rest are made beside it, as copies, reported by `split` records
  (`GisClip.APartSplitIntoTwoKeepsTheIdOnTheLargerAndReportsTheOther`); one
  wholly outside is deleted; one wholly inside is not touched - no rewrite of
  its vertices (`GisClip.InPlaceWhatIsOutsideGoesAndWhatIsInsideStaysAsItWasInOneStep`).
  An area wholly outside goes with the holes it took in
  (`DrawingDataset::holes`); left behind, a hole would be read as an area of
  its own the next time its layer is read. `outside=` counts areas and
  lines, `deleted=` entities, holes included
  (`GisClip.AnAreaWhollyOutsideGoesWithItsHolesInOneStep`).
  All in one undo step, after the entities - the holes of the areas too -
  are compared with the copies taken at prepare, and refused when one
  changed while the job ran
  (`GisClip.AnInPlaceClipRefusesADrawingChangedWhileItRan`,
  `GisClip.AHoleMovedWhileItRanIsNotDeletedWithItsArea`). It is REPAIR's
  reshape (`vector::reshapeCommand`).
- A curve cut in place becomes the chords of what is left: GDAL clips
  lines, and an arc's remainder is drawn as a polyline.

```
hull kind=concave ratio=0.1 holes=no points=57 area=36.500
clip mode=replace features=3 whole=1 cut=1 outside=1 area=0.000 length=30.000
```

**Tests.** `tests/geo/test_hull_and_clip.cpp`, by hand as above, and a pipe
through boxes at 10..40 and 60..95 keeping its id on the 35 m piece;
`GisHullClipContract.TheArgumentsTheVerbsBindAreGdals` pins concave-hull and
clip. The dialogs: `tests/qt_widgets/geo/test_hull_and_clip_dialogs.cpp`;
katana_cli: `cli.gis_hull_of_a_squares_corners_and_centre_is_the_square`,
`cli.gis_clip_in_place_leaves_thirty_metres_of_the_line`.

**Not done.** The concave boundary as the edge of SURFACE FROM DRAWING is a
follow-up once this and T0 are both on main (it crosses the two lanes).

### V4: GIS CHECK, REPAIR and COVERAGE

```
GIS CHECK [<scope>] [markers=<layer>] [PREVIEW]
GIS REPAIR [<scope>] [method=linework|structure] [PREVIEW]
GIS COVERAGE CHECK [<scope>] [gap=<m>] [markers=<layer>] [PREVIEW]
GIS COVERAGE CLEAN [<scope>] [gap=<m>] [snap=<m>]
                   [merge=longest-border|max-area|min-area|min-index] REPLACE [PREVIEW]
```

Invalid geometry in imported data, and the gaps and overlaps of a
subdivision or a cadastral fabric (`src/katana_app/geo/check_verbs.cpp`).
The window's items are GIS > Check - GDAL > Check Geometry..., Repair
Geometry... and Gaps and Overlaps... (`gisCheck`, `gisRepair`, `gisCoverage`;
`src/katana_qt/geo/geometry_check_dialog.hpp`, `coverage_dialog.hpp`), whose
`<d>Problems` table lists the problem records of the last reply.

**CHECK** runs `vector check-geometry` on the scope's lines and areas (a
point cannot be invalid). It gives one problem record at each place GDAL
found a defect, with GDAL's reason and the entity, which it carries back as
`katana_id` (`--include-field`):

```
problem kind=self-intersection entity=7 at=35,5 reason=Self-intersection
check features=2 problems=1 entities=1
```

The bow-tie (30,0) (40,10) (40,0) (30,10) crosses itself where x - 30 = y
and 40 - x = y: at (35,5) (`GisCheck.ABowTieSelfIntersectsAt35Comma5`).
`at=` is said to the millimetre, as `length=` is, without trailing zeros:
GEOS's arithmetic put the middle of an edge from y = 0 to 40 at
20.000000000000007, which the reply printed
(`GisCheck.WhereAProblemIsIsSaidToTheMillimetre`). `fixed3`'s trailing
zeros were rejected there: a place is two numbers joined by a comma, and
`at=35,5` is what the records have always said. A
check changes nothing - unless `markers=<layer>` asks for a point at each
problem. Those replace the markers the last check left on that layer (tagged
`gis.marker=check`), in one step, the way AUTOLABEL replaces its rule's
labels: a rerun never piles markers up, and a clean rerun clears them
(`GisCheck.MarkersAreReplacedOnRerun`).

**REPAIR** runs `vector make-valid` and changes only what came back
different; a valid area is not even rewritten.

- **In place.** The entity's geometry is replaced (`setEntityGeometry`), so
  its id - and every label and association on it - is kept
  (`GisCheck.RepairKeepsTheId`). An area keeps the way round it ran.
- **Split.** A result of several parts - the bow-tie's two triangles of
  25 m2 - keeps the id on the largest and makes the rest as copies of the
  entity (layer, style, colour, properties) with `gis.source`; the reply
  says which (`split entity=7 parts=2 kept=7 created=12`;
  `GisCheck.RepairOfTheBowTieGivesTwoTrianglesOf25SquareMetresEach`).
- **Holes.** An area is one ring, and its holes are entities of their own.
  A result whose holes differ from the area's cannot be written on the
  area, so it is left as it was, and said.
- **Checked first.** It is an in-place apply: the entities are compared with
  the copies taken at prepare, and a drawing changed while the job ran is
  refused (`GisCheck.RepairRefusesADrawingChangedWhileItRan`).

**COVERAGE CHECK** runs `vector check-coverage` on the scope's areas.

- **Areas only.** check-coverage refuses mixed geometry (measured), so the
  scope's points and lines are left out, counted (`ignored=`) and said
  (`GisCheck.MixedGeometryInScopeIsFilteredAndCounted`).
- **Joined back by order.** check-coverage carries no field, but with
  `--include-valid` it gives one result per area, in order: that is how an
  edge is joined to its entity.
- **What an edge is.** GDAL says only that an edge is invalid. The verb says
  why, from where the middles of the edge's segments lie against the other
  areas: strictly inside one is an `overlap`; outside all and within `gap=`
  of one is a `gap`; otherwise the edge lies on a neighbour's without
  sharing its vertices, a `mismatch`
  (`GisCheck.OverlappingLotsReportTheirOverlapEdges`,
  `GisCheck.AnEnclosedGapNarrowerThanGapIsReported`).
- `markers=<layer>` draws each bad edge as a polyline, replacing the last
  run's (`gis.marker=coverage`).

```
problem kind=overlap entity=1 at=50,20 length=40.000 reason="the area overlaps a neighbour along this edge"
coverage mode=check gap=0.05 areas=4 ignored=0 problems=2 entities=2 overlaps=0 gaps=2 mismatches=0
```

**COVERAGE CLEAN** runs `vector clean-coverage`, which moves boundaries so
the areas meet. Legally surveyed boundaries must not be adjusted silently,
so:

- **it needs REPLACE**, and says why without it
  (`GisCheck.CleanWithoutReplaceIsRefused`);
- every clean that moves a boundary warns so, and one UNDO puts them all
  back (`GisCheck.CleanClosesAnEnclosedGapAndOneUndoRestoresIt`: 12000 -
  0.8 m2 becomes 12000);
- it closes **enclosed** gaps only. A sliver open to the outside of the
  fabric is not a gap to GEOS and stays (measured on GDAL 3.13.2).

It applies as REPAIR does - in place, split parts made, compared first
(`GisCheck.CleanRefusesADrawingChangedWhileItRan`) - and
changes only the areas whose ring came back different: clean-coverage
rewrites every ring's start and direction, so "changed" is judged by the
ring (`vector::sameRing`), not the vertex list.

**Shared** (`vector_support.hpp`): `reshapeCommand` is the in-place apply
REPAIR, CLEAN and the clip of V3 use - the largest part on the id, the rest
as copies - and `replaceMarkers` the markers' rerun rule.

**Tests.** `tests/geo/test_check_verbs.cpp` (the bow-tie, the lots
overlapping by a metre, four lots about a 0.02 m enclosed gap);
`GisCheckContract.TheArgumentsTheVerbsBindAreGdals` pins check-geometry,
make-valid, check-coverage and clean-coverage. The dialogs:
`tests/qt_widgets/geo/test_check_dialogs.cpp`; the window:
`qt_gis_check_dialog_headless`; katana_cli:
`cli.gis_check_finds_the_bow_ties_crossing_at_35_5`,
`cli.gis_repair_makes_the_bow_tie_two_triangles_of_25_square_metres`.

**Not done.** A problem is not yet selectable from the table (the entity id
is in its row); `simplify-coverage` stays in the GDAL verb and the toolbox.

### V5: GIS SQL and katana_gis_query

```
GIS SQL "<select>" [<scope>] [dialect=sqlite|ogrsql]
        [AS REPORT | AS SELECT | AS LAYER <layer>] [csv=<file>] [OVERWRITE] [PREVIEW]
```

The drawing queried with SQL through GDAL's `vector sql`
(`src/katana_app/geo/sql_verbs.cpp`). The window's item is GIS > Analysis -
GDAL > Query with SQL... (`gisSql`, `src/katana_qt/geo/sql_dialog.hpp`), with
the tables and their columns listed for the scope and the rows in a grid;
an agent's is the MCP tool `katana_gis_query` (`docs/mcp.md`).

- **The tables** are drawingDataset's: `points`, `lines`, `polygons`, each
  with `katana_id`, `layer`, `style`, `colour`, `type` and then the
  entities' properties as typed columns. The geometry column is `geometry`
  (`ST_Area(geometry)`). An identifier with a dot is written `[gis.source]`
  or `"gis.source"`.
- **SQLite by default**, with Spatialite's `ST_` functions; `dialect=ogrsql`
  is OGR's own SQL.
- **A query reads.** Only one SELECT runs: any other statement, and a `;`
  beginning a second one, is refused before GDAL sees it
  (`GisSql.ANonSelectStatementIsRefused`). `SPATIALITE_SECURITY` is never
  set, so Spatialite's file functions are not even there: `BlobToFile` is
  "no such function", and no file appears (`GisSql.BlobToFileIsRefused`).
- **AS REPORT** (the default) answers a `column` record per column - its
  name, its key in the rows and its type, so a reader knows a 7 from a "7" -
  and a `row` record per row. A null has no cell in its row: said as
  nothing, not as an empty value. A column name a record cannot hold (a
  blank, `=`) is keyed with `_` in its place (`count(*) n` is `count(*)_n`).
  `csv=<file>` writes the rows too.
- **AS SELECT** makes the entities the `katana_id` column names the
  selection (`GisSql.AsSelectSelectsTheReturnedIds`).
- **AS LAYER <layer>** draws the geometry the rows return, as one undo step
  (`GisSql.AsLayerCreatesOneUndoStep`).
- **AS is TO.** `AS SELECT` is the target parser's `TO SELECTION`, and
  `TO REPORT`, `TO SELECTION` are now the GDAL verb's too: the V5 block of
  `bindings.cpp` applies them through `vector::reportRecords` and
  `vector::selectFeatures`, so `GDAL vector sql ... FROM DRAWING TO REPORT`
  answers the same records (`GisSql.TheGdalVerbReportsAndSelectsToo`).

```
column name=owner key=owner type=string
column name=area key=area type=real
row owner=Jones area=600
row owner=Smith area=4000
output arg=output kind=vector target=report rows=2
sql dialect=sqlite tables=polygons rows=2
```

**One line, whoever writes it.** A line cannot carry a double quote inside a
quoted word, so the dialog and `katana_gis_query` write a statement through
`vector::sqlForLine`: line breaks become blanks, and SQLite's
`"identifiers"` become `[identifiers]`, which SQLite reads alike. A double
quote inside a `'...'` literal, or any in OGR SQL, cannot be written, and is
refused naming why.

**katana_gis_query** builds `GIS SQL "<sql>" <scope> dialect=<d>` - the
scope `drawing` unless it says otherwise, since a question of the drawing is
the usual one - runs it through the Session and reads the records back:
`{columns, column_types, rows, matched, used}`, each cell typed by its
column (`GisQueryMcp.GisQueryReturnsRowsAsJson`, driving the MCP server as a
client does). Its test is in `tests/geo`, beside the verb's, rather than
in `tests/app/test_mcp_server.cpp`, which every lane would otherwise append
to.

**Tests.** `tests/geo/test_sql_verb.cpp`, by hand: a 50 x 40 lot's
`ST_Area` is 2000 and its 1 m inward buffer's 48 x 38 = 1824; Smith's two
lots sum to 4000 and Jones's to 600. The dialog:
`tests/qt_widgets/geo/test_sql_dialog.cpp`; katana_cli:
`cli.gis_sql_counts_and_sums_the_area_of_two_lots`.

**Not done.** Spatialite over the MEM tables has no spatial index, so a
spatial join of many thousands of features is slow; materialising to a
GeoPackage first is the plan's answer when it is measured to matter.

### I0: One GIS executor: IMPORT, EXPORT, INFO, REFS, COPC

Built. The five verbs are the executor's, a file each
(`src/katana_app/geo/import_verb.cpp`, `export_verb.cpp`, `info_verb.cpp`,
`refs_verb.cpp`, `copc_verb.cpp`), in the table's I0 block, and they reply
in records; `docs/interop.md` ("IMPORT, EXPORT, INFO, REFS and COPC on every
front end") has the grammar, the records and the decisions. What the other
packages build on:

- **`VerbEntry::takes`**, optional: whether a line of the verb is the
  executor's at all. `INFO`'s (`geo::takesInfo`) leaves `INFO <id>` to the
  interpreter. A row of four fields, as every other package writes, leaves it
  null.
- **`Context::farApart` and `Context::imported`**, the window's: the
  far-apart question an `IMPORT` asks, and the meshes and 3D view only a
  window shows. A session sets neither.
- **`geo::StagedFiles`** (`staged_files.hpp`): a file a job writes, put in
  place by its apply, so a cancel after the write leaves nothing. I4's
  EXPORT options and any verb that writes a user's file use it. A writer's
  failure is given back through `StagedFiles::asTarget`, which puts the
  target's path where the staging folder's was: an EXPORT refused for its
  CRS named `.katana-staging-19705-1/f.kml`, a folder gone by the time
  anyone read it (review finding;
  `ExportOptions.AWritersFailureNamesTheFileAskedForNotItsStagingFolder`).
- **`gis_records.hpp`**: the `reference` and `surface` records, and
  `recordJson` / `recordsJson`, any reply's records as JSON objects -
  `katana_import` and `katana_export` hand them to an agent, and D1's and
  D2's tools can.
- **`import_records.hpp`** and **`dxf_verbs.hpp`** in `src/katana_app`: the
  records and the DXF steps every build shares, so a build without GDAL
  replies in the same records.
- The session's `SurfaceStore` now holds a 12d archive's surfaces.

The reply format changed from prose to records; the `cli.*`, `Session.*`,
`GeoSession.*` and window checks were rewritten for the records only
(F0's `GeoSession.AGdalLineRunsThroughTheSessionAndItsRasterIsAReference` and
`cli.gdal_hillshade_of_the_sample_terrain_to_a_reference` among them, for
`REFS`).

### I1: Vector fidelity

Built (`docs/interop.md`, "Fidelity"): EXPORT reads the drawing through
`drawingDataset` and IMPORT makes entities by `featurePieces`, so the
geoprocessing bindings and the file path are one conversion. The adapter
reads and writes typed tables, keeps a file's arcs for the import, counts
what it cannot carry, and returns GDAL's warnings. KML, KMZ and GPX are
converted to longitude and latitude from the project's coordinate system,
which EXPORT now passes on every front end. Tests:
`tests/geo/test_vector_fidelity.cpp`,
`src/katana_app/geo/cli/vector_fidelity.cmake`,
`tests/geo/headless/vector_fidelity.cmake`.

### I2: Every driver: FORMATS

Built (`docs/interop.md`, "Formats"): the formats come from GDAL's driver
manager with a small curated overlay (`gis/formats.hpp`), a file is routed
by what it holds, an archive by its inside, and `/vsi` paths and URLs open.
`FORMATS` is the verb table's I2 row (`src/katana_app/geo/formats_verbs.cpp`);
it answers at prepare, as `GDAL LIST` does. `katana_formats` and
`katana://formats` share its records; GIS > Processing - GDAL > Formats...
is the I2 block of `menu_table.cpp`. Tests: `tests/geo/test_formats.cpp`,
`tests/qt_widgets/geo/test_formats_dialog.cpp`,
`src/katana_app/geo/cli/formats.cmake`, `tests/geo/headless/formats.cmake`.

### I3: IMPORT options

Built (`docs/interop.md`, "Import options"). `IMPORT` keeps its I0 row; its
words are read by `prepareImport` in `src/katana_app/geo/import_verb.cpp`,
the option words by `vector::readVerbWords` - the one reader of a line's
own words and its scope - with `oo=` taken out first, since it may be given
more than once. The reader takes a verb's options wherever they stand, but
after WHERE a key that is also a WHERE key (`cad::isWhereKey`: CONTOUR's
`layer=`) is the filter's condition, since the scope widget writes the
filter at the end of a dialog's line (`docs/terrain.md`, "Contours";
CONTOUR and RASTER SLOPE / ASPECT moved onto the reader from readers of
their own that refused an option after the filter). What the others build on:

- **`gis::checkOptions`** (`include/katana/gis/formats.hpp`): KEY=VALUE
  options checked against a driver's open, creation or layer-creation list.
  I4's `co=` and `lco=` use it.
- **`gis::VectorReadOptions::attributeFilter` and `spatialFilter`, and
  `GdalDataset::readSql`**: filters and statements handed to the driver.
- **`Document::coordinateSystemCommand`**: the project's CRS as a command,
  for a step that changes it with other things (`crs=adopt`).
- **`GisDialogContext::await`**: a modal dialog's way to hear its job end
  (the import dialogs' Preview), beside the workbench listener a non-modal
  GIS dialog uses. It was added to the existing context rather than making
  a fourth (see "Not done", three dialog contexts).
- **`mcp::scopeWordsOf`** (`src/katana_app/mcp_tools.hpp`): a scope in a
  tool's JSON as the grammar's words. `katana_gdal_run`'s sources and
  `katana_gis_query` each read it in their own copy; both now call this,
  and `katana_import` does.

A deviation from the plan's grammar: `class=` takes one class, since the
point cloud reader filters by one, and a list is refused rather than read as
its first. Added beyond it: `attributes=no` (the dialog's "keep attributes"
box had no word) and `resolution=` (the cloud dialog's COPC level of
detail had none). Tests: `tests/geo/test_import_options.cpp`,
`tests/qt_widgets/geo/test_import_dialogs.cpp`,
`src/katana_app/geo/cli/import_options.cmake`, and
`McpServer.ImportTakesItsFilterScopeAndPreviewArgumentsAsTheWordsAPersonTypes`.

### I4: EXPORT on the shared scope and filter

Built (`docs/interop.md`, "Export options"). `EXPORT` keeps its I0 row; its
words are read by `prepareExport` in `src/katana_app/geo/export_verb.cpp`
through `vector::readVerbWords` and the one scope parser, with `co=` and
`lco=` taken out first (repeatable). What the others build on:

- **`interop::VectorExportOptions`** gained `targetCrs`, `splitByLayer`,
  `append`, `creationOptions`, `layerCreationOptions` and `textAsPoints`;
  **`gis::VectorExportOptions::append`** adds layers to an existing file.
- **A table's `OGR_STYLE` field** is also written as each feature's style
  string (`GdalDataset::writeTables`), for any verb that wants labels drawn.
- **`src/katana_interop/table_reprojection.hpp`**: the one move of a feature
  table between coordinate systems, for IMPORT's `crs=project` and EXPORT's
  `crs=<code>`.

Deviations from the plan: `layername=` for the plan's `layer=`, since an
option key may not be a WHERE key; the DXF scope goes through a pruned copy
of the drawing rather than the DXF writer's entity list (the copy is already
made for the worker, and the archive writer takes the same route); GeoJSON's
default is longitude and latitude (RFC 7946), `crs=native` keeping the
project's. The Export Vector dialog is the GIS frame's (`vectorExport`),
opened by File > Export Vector after its file dialog, so `--dialog` does not
reach it headless - the widget tests drive it by object name, and the
`EXPORT` line is the headless path. Tests:
`tests/geo/test_export_options.cpp`,
`tests/qt_widgets/geo/test_export_dialog.cpp`,
`src/katana_app/geo/cli/export_options.cmake`, and
`McpServer.ExportTakesTheSharedScopeAndItsOptionsAsTheWordsAPersonTypes`.

### D1: INFO as structured data, STATS, CHECK

Built (`docs/interop.md`, "Dataset information"). `INFO <file|folder|url>
[JSON] [STATS] [CHECK] [LAYER <name>]` replies with dataset, raster, band,
overview, subdataset, layer, field and point-cloud records, read from GDAL's
own `raster info` and `vector info` JSON through the bridge - which
`interop::describeSource` now uses in place of the adapter's reading - or
gives that JSON back; a folder is `dataset identify`, CHECK `dataset check`.
The Dataset Information dialog runs the INFO lines through the window's one
executor and hears a job's end through `MainWindow::awaitJob`; MCP
`katana_dataset_info`.

- **Deviations.** The records are built from `interop::SourceDescription`,
  which gained bands, fields, extents, subdatasets and GDAL's JSON
  (`DescribeOptions`: statistics, layer, multidim, anyFormat), rather than
  from a second parse of the JSON in the verb. `STATS` and a layer are
  options of `describeSource`, so the import dialogs could use them too.
  Statistics come from the band's `STATISTICS_` metadata (14 digits), not
  the JSON's rounded keys (3 decimals).
- **For the other packages.** `MainWindow::awaitJob(started, done)` is how a
  dialog hears what its line did once the job it started ends (the
  toolbox's reply, a lane's dialog); it reads the `job` record the line
  answered with.

### D2: Reference layers: manageable, persistent, with overviews

Built (`docs/interop.md`, "Reference layers"): the REFS family on every
front end, the project's record of its reference layers, REFS RESTORE on
opening, the Reference Data panel as a builder of REFS lines, and MCP
`katana_references`.

- **Deviations.** The records are a metadata key, `reference_layers`, not a
  table added by a storage migration: a schema change would make every
  project this build saves unopenable by the builds before it, and back up
  and migrate every older project on opening, for a few short records,
  where a key an older build keeps as it was (`docs/model.md`, decision
  D6). They are recorded at each save rather than whenever a layer changes,
  so an import does not by itself make the drawing ask to be saved.
  `REFS RESTORE` is added to the grammar: opening a project runs it, and an
  agent can too. `RasterOverlay::facts` is still not filled: INFO reads a
  band's facts from its file (D1), and nothing yet needs them held.
- **For the other packages.** `interop::ReferenceSource` and its record
  carry `displayStyle` (T2's) and `derivation`, so a shaded or derived
  raster comes back as it was; `geo::recordReferences(context)` is what a
  front end calls before a save.

## Not done

- **The toolbox's Pipeline tab has no Confirm or Overwrite box.** A pipeline
  with an `update` step, or with `--overwrite` in its text, is refused
  there with the verb's message naming CONFIRM or OVERWRITE; it runs typed
  on the command line, through `katana_cli`, or `katana_gdal_run` with
  `confirm` / `overwrite`. The Algorithm tab's boxes judge the leaf's policy
  only, which for a pipeline is Safe.
- **A reference raster is built in memory first.** Every raster result a
  run keeps as a reference is written by the algorithm into a MEM dataset
  and then copied to its tiled GeoTIFF (`spill`), so a result bigger than
  memory fails where writing straight to the file would not (review
  finding). Deferred: writing a raster-only output straight to the GeoTIFF
  is a change to how every algorithm's output is bound (its format, its
  creation options, the colour-map band roles stamped after the run), and
  it has no test that can tell the two apart short of a result larger than
  a test machine's memory or a peak-memory measure, which the benchmarks
  (`benchmarks/bench_*.cpp`) are the place for, in Release, before and
  after.
- **A file GDAL's own words name is not cancel-safe.** `TO FILE` is staged
  and survives a cancel; `GDAL raster hillshade a.tif b.tif --overwrite`
  (the output in GDAL's words) is written by GDAL where it says, and a
  cancel part way leaves `b.tif` partial. Katana knows which word is the
  output by name, not by value (`argumentsGiven`), so it cannot redirect
  it.
- **A tile set is rewritten in place.** `raster tile` into a folder that
  already holds tiles rewrites them with no `--overwrite` (GDAL checks
  nothing there; measured, the tile's time changed). `TO FILE <folder>`
  needs OVERWRITE as any existing target does, but a folder named in GDAL's
  own words is not checked, since which word is the output is known by
  name (`argumentsGiven`), not by value.
- **`VIEW` in a headless session** is refused naming `AREA`, as MODIFY
  refuses it.
- **A picked point does not snap.** `GeoServices::pickPoint` takes the
  click where it is; snapping belongs to a running tool, and a pick is none.
- **Stopping late.** Pipelines and vector algorithms report progress once,
  so a cancel stops them only at their end.
- **PREVIEW opens files on the calling thread.** Validation opens datasets
  given by name, so a PREVIEW of a `/vsicurl` source waits on the network.
  A run does its opening on the worker.
- **The scratch folder is left behind.** Derived rasters of a drawing with
  no project stay in `<temp>/katana-scratch/<process id>-<start>` when the
  process ends.
- **Measuring the apply.** The cost of creating entities in an apply is not
  yet measured for large results; the benchmark the plan names for it is not
  written. Nothing is promised for a result of millions of features.
- **Three dialog contexts where one would do.** The lanes built their
  dialogs side by side, and each gave its dialogs a context of what the
  window lends them: `GeoDialogContext` (`geo_dialog_support.hpp`, the
  toolbox and the DEM dialogs), `TerrainDialogContext`
  (`terrain_dialog_support.hpp`, Surface From and the terrain analysis
  dialogs) and `GisDialogContext` (`gis_tool_dialog.hpp`, the vector
  dialogs). They hold the same things - the executor, `headless`, the
  document, the views and a way to hear a job end - and differ in that way:
  `GeoDialogContext::listen` returns a token whose life bounds the listener,
  `TerrainDialogContext::listen` is never taken back (safe only because the
  window destroys the workbench before its child dialogs), and
  `GisDialogContext` has `await`, told of one job. When wave 1 was merged,
  the views they are given were made one `GeoServices::views` (the
  workspace). The review after it made the rest of what they duplicated
  one each: the quoting of a word (`geo::lineWord`, above), and the reading
  of the `job id=<n> title="..." state=started` record an interactive run
  logs - four patterns (the Geo and Terrain supports', the GIS dialog's and
  `MainWindow::awaitJob`'s) are now `startedJob`, beside the `startedRecord`
  that writes it (`geo_workbench.hpp`,
  `GeoWorkbench.TheStartedRecordReadsBackAsItsJob`), and the GIS dialog's own
  pattern for a record's field is `geo::parseRecords`. The three structs
  remain: folding them into one, with the token listener, changes the
  listener contract of the Terrain dialogs, the modal import dialogs'
  `await`, and the construction in 6, 17 and 24 files that name each
  context (the dialogs and their widget tests), which is a refactor of its
  own rather than part of a review's fixes.
