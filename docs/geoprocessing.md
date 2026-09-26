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
  is left to print. The output's reference is taken before `Finalize()`,
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
  `CONFIRM`, or an MCP call passes `confirm: true`. The toolbox's Confirm box
  (X1) will be the window's way.
- **Refused words.** `--config`, `--help`, `-h`, `--help-doc`,
  `--json-usage`, `--progress`, `--quiet` and `-q` are refused
  (`checkTokens`), and so is a pipeline step called `external`, in the words
  or in a pipeline string, nested in `[ ]` too
  (`GdalPolicy.APipelineStepNamedExternalIsRefusedWhereverItStands`).
- **Replacing a file.** `TO FILE` never replaces a file without `OVERWRITE`,
  and the tail's `--overwrite`, `--overwrite-layer`, `--append`, `--update`
  and `--upsert` need `OVERWRITE` too: each changes a file already there.

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
- **Heights.** Z is written only when every vertex has a height: absent is
  not zero (`DrawingDataset.AVertexWithoutAHeightKeepsTheLineTwoDimensional`).
  An entity heighted at only some vertices, or an arc whose ends differ,
  goes in plan with its heights in the elevation fields, and a warning
  counts them. With `requireHeights`, a heightless entity is left out and
  counted.
- **What has no feature.** Text, dimensions, labels and leaders are left
  out and counted by kind; the counts are the scope record's `skipped.*`.
- **CRS.** The project's coordinate system (the document's
  `coordinateSystem`) is set on every table.

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
- **REPLACE (`deleteSources`).** It deletes the source entities, in the same
  step (`DrawingDataset.ReplaceDeletesTheSourcesInTheSameStep`).
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
  URL passes through.

### Derived rasters

A raster result goes to `<project>/cache/gdal/<name>.tif`. When the drawing
has no project it goes to the front end's scratch folder
(`<temp>/katana-scratch/<process id>`, `geo::defaultScratch`), and the reply
says `persisted=no`. It is read by `interop::importRaster` as a reference
raster with `role` Derived and `derivation` - the line that made it. A name
already taken becomes `<name>-2`, `-3` ...
(`GeoExecutor.ARasterResultBecomesADerivedReferenceRaster`). `RasterOverlay`
gained `role`, `facts`, `derivation` and `displayStyle`; F0 fills the role
and the derivation, T2 draws the display styles, and D2 persists them.

## The executor

`katana::app::geo` (`src/katana_app/geo/geo_verbs.hpp`) runs a line in three
phases, so a session runs it inline and the window as a job:

1. **`prepare`, on the calling thread** (the GUI thread in the window):
   - the line is split (`geo::tokenize`: the interpreter's rules, and which
     words were quoted, so a quoted `"FROM"` is a value);
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
verbs. It has one reserved block per package.

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
```

- **The algorithm.** One to three words, by longest match, aliases taken.
- **GDAL's words** are anything GDAL's own command line takes for that
  algorithm: `--zfactor 2`, `--zfactor=2`, `-z 2`, positionals, `--co K=V`.
  `name=value`, when `name` is one of its arguments, is written
  `--name=value`: the form the other verbs' options take. The words end at
  the first unquoted FROM, TO, CONFIRM, OVERWRITE or PREVIEW, and GDAL words
  after a clause are refused (`GeoExecutor.GdalTokensAfterAClauseAreRefused`).
- **FROM** binds a dataset argument: the one it names (`FROM method LAYERS
  corridor`), or the first required input no FROM has taken. A list argument
  takes several FROMs. A dataset GDAL's words also give is refused as given
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
(`cad::recordValue`); an empty one is bare (`aliases=`). Errors go to
stderr as `error: <Code>: <message>`.

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
the command line's own text into structured content.

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

`GDAL HELP ... JSON`, `katana_gdal_describe` and the toolbox (X1) read the
same functions.

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
is the one icon the packages share.

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
- **A raster result is spilled, never held.** For a reference target the run
  asks for `maxMemoryCells = 0`: the result is written once, where it is
  kept, as it is kept.
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
- `geo::defaultScratch` is the scratch folder of the window and the session
  alike.
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

Not started.

### X2: Toolbox pipeline tab

Not started.

### T0: Terrain session: one surface store and SURFACE verbs on every front end

Not started. The store is F0's (`terrain::SurfaceStore`); `TO SURFACE` has
its block in `bindings.cpp`.

### T1: CONTOUR from a surface or an elevation raster

Not started.

### T2: Terrain shading: hillshade, colour relief, slope shading

Not started. `RasterOverlay::displayStyle` is declared for it.

### T3: Slope and aspect with slope-class areas

Not started.

### T4: Statistics by area, sampling and drape

Not started. `ResultMode::SetProperties` and `UpdateGeometry` are its
applies, with `geo::unchangedSince`.

### T5: Viewshed and line of sight

Not started.

### T6: RASTER GRID: survey points to a DEM

Not started. `DrawingDatasetOptions::requireHeights` is its binding.

### T7: DEM tools: mosaic, clip, fill, footprint, reproject, difference

Not started.

### V1: GIS BUFFER and GIS DISSOLVE

Not started.

### V2: GIS OVERLAY: polygon booleans between two scopes

Not started.

### V3: GIS HULL and GIS CLIP

Not started.

### V4: GIS CHECK, REPAIR and COVERAGE

Not started.

### V5: GIS SQL and katana_gis_query

Not started. `TO SELECTION` and `TO REPORT` have their block in `bindings.cpp`.

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
  EXPORT options and any verb that writes a user's file use it.
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

Not started. Its dialog is `gis_import_dialogs`.

### I4: EXPORT on the shared scope and filter

Not started. Its dialog is `gis_export_dialog`.

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

- **No menu item of F0's own.** The GDAL verb is reachable on the command
  line, from a script and through `runVerbLine`; its menu item is X1's
  GDAL Toolbox.
- **`VIEW` in a headless session** is refused naming `AREA`, as MODIFY
  refuses it.
- **No point is picked in the plan view.** The plan gave `GeoServices` a
  point pick for a dialog (the viewshed's observer); the window has no such
  service yet, so it is not declared either. The package that first needs
  it (T5) adds it through the plan view's tool host.
- **Stopping late.** Pipelines and vector algorithms report progress once,
  so a cancel stops them only at their end.
- **PREVIEW opens files on the calling thread.** Validation opens datasets
  given by name, so a PREVIEW of a `/vsicurl` source waits on the network.
  A run does its opening on the worker.
- **The scratch folder is left behind.** Derived rasters of a drawing with
  no project stay in `<temp>/katana-scratch/<process id>` when the process
  ends.
- **Measuring the apply.** The cost of creating entities in an apply is not
  yet measured for large results; the benchmark the plan names for it is not
  written. Nothing is promised for a result of millions of features.
