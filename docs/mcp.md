# Claude and Katana: the MCP server

`katana_mcp` serves the Katana engine over the Model Context Protocol (MCP), so
that Claude - in Claude Desktop, Claude Code or any other MCP client - can run
an engineering project in Katana: open a project, draw and edit it, run the
survey calculations, lay out alignments and parcels, apply survey codes,
grade, check and draw located buried services by AS 5488 quality level - from
a schedule, or from the lines and points a survey or an import left in the
drawing (`UTILITY DRAW <scope> METHOD ...`) - and grade again, report, check
and write back as a schedule what is drawn (`UTILITY REPORT`, `VERIFY`,
`CLEARANCE`, `CHECK`, `DRAW`, `REGRADE`, `SCHEDULE`,
`docs/subsurface_utilities.md`), change what a scope and a filter
take (`MODIFY`, `docs/cad.md`, "Scope and filter"), import and export, and
save it. The tool that runs commands names each `UTILITY` action and the
scope words in its description, since that is what a client reads first.
The window can then open the same project.

It is the session `katana_cli` runs (`src/katana_app/session.hpp`), served to a
client instead of typed. Every tool that changes anything is a shape over the
command line's verbs, so there is one implementation of each, pinned by the
`cli.*` tests, and Claude can do nothing that a person at `katana_cli` could
not. Every change is one undoable command, as it is when typed. (Two tools run
no line and change nothing: `katana_status` and `katana_customisation` read
the Document for what no reply holds as structure.)

The session starts with the customisation `katana_cli` starts with - the
kept file the environment variable `KATANA_CUSTOMISATION` names, else the
program's built-in one - and says which in the client's log
(`docs/customisation.md`, "What katana_cli and katana_mcp start with").

## Connecting a client

A client starts `katana_mcp` itself and talks to it over its standard streams;
nobody runs it by hand. `--project <directory>` opens that project when the
session starts.

**Claude Code**, from a terminal:

```sh
claude mcp add katana -- C:/path/to/Katana/bin/katana_mcp.exe
claude mcp add katana -- C:/path/to/Katana/bin/katana_mcp.exe --project C:/work/site-copy
```

**Claude Desktop**, in `claude_desktop_config.json` (Settings > Developer >
Edit Config), then restart Claude Desktop:

```json
{
  "mcpServers": {
    "katana": {
      "command": "C:/path/to/Katana/bin/katana_mcp.exe",
      "args": ["--project", "C:/work/site-copy"]
    }
  }
}
```

The path is `<build>/bin/katana_mcp.exe` in a build tree, or `bin/` of the bundle;
both find their runtime libraries beside themselves (`docs/building.md`). The
build tree's copy is fine for trying it out.

Then ask for the work in plain words - "open C:/work/site-copy, put the kerb
lines on a layer of their own and give me the parcel areas", "draw a 30 by 20
lot at 1000,2000 and label its bearings" - and Claude chooses the commands.

## Tools

| Tool | What it does | Runs |
|---|---|---|
| `katana_run_commands` | a list of commands, in order; stops at the first failure unless `stop_on_error` is false | each line |
| `katana_run_script` | a `.kcs` script file, as `katana_cli script.kcs` runs it | each line of the file |
| `katana_help` | the command reference | `HELP` and the session's own verbs |
| `katana_status` | project, unsaved changes, entity, layer and alignment counts, current layer and style, selection, undo depth | nothing: read from the Document by `cad::documentStatus`, as `STATUS` and `STATUS JSON` read it |
| `katana_new_project` | a new, empty drawing | `NEW` |
| `katana_open_project` | open a project directory | `OPEN "<path>"` |
| `katana_save_project` | save in place, or as a project at `path` | `SAVE` / `SAVE "<path>"` |
| `katana_list_entities` | every entity with its id, layer and measurements | `LIST` |
| `katana_describe_entity` | one entity in full | `INFO #<id>` |
| `katana_import` | DXF always; GIS vector, raster and point-cloud files with `KATANA_BUILD_IO`; the options of "I3 and I4" below (layers, where, sql, a scope, clip, fields, crs, a raster's band, a cloud's budget, preview ...); `placement` moves what a DXF, vector file or .12da archive holds as one piece: `local` (its lower-left corner to 0,0; also `local: true`), `alongside` (onto the drawing's lower-left corner) or `offset` (by `offset_east`, `offset_north`); `keep` by default; anything but keep is refused for rasters and clouds (`docs/interop.md`, "Placing an import"). With `KATANA_BUILD_IO`, `structuredContent.records` holds the reply's records as objects - `imported`, `placed`, `reference`, `surface`, `tally`, `warning` - numbers as numbers and `bounds` as `[x0, y0, x1, y1]` | `IMPORT "<path>" [LOCAL \| ALONGSIDE \| OFFSET=dE,dN]` |
| `katana_export` | DXF always; GIS vector formats and .12da archives (with the session's surfaces) with `KATANA_BUILD_IO`; the shared scope and EXPORT's options ("I3 and I4" below); with a .las or .laz path and `cloud` (an id or name), a reference point cloud as held (`docs/interop.md`, "The GIS menu"); `structuredContent.records` holds the `exported` record, then the `scope` record and any `warning` | `EXPORT "<path>" [<scope>] [<options>]`, `EXPORT "<path>" CLOUD <id\|name>` |
| `katana_undo` | undo, or with `redo` redo, `steps` steps | `UNDO n` / `REDO n` |
| `katana_customisation` | the session's customisation as structured data: `part` is `summary`, `codes`, `definitions` or `problems`, with `filter`, `name`, `offset` and `limit` ("The customisation", below) | nothing: read from the Document, through `cad` and the customisation format's own writer |

Two tools were broken in every build with the GIS module until 2026-09-26,
and are pinned now by `McpServer.DescribeEntityDescribesTheEntityItNames` and
`McpServer.ImportLocalMovesAQuotedPathsDataToTheOrigin`:
`katana_describe_entity` sends `INFO <id>`, which the session read as
`INFO <file>` before the interpreter could see it; and `katana_import` with
`local: true` sends `IMPORT "<path>" LOCAL`, from which the session took the
`LOCAL` and left the quotes on the path. Both front ends now read an `INFO`
id and an `IMPORT` argument with the interpreter's own functions
(`CommandInterpreter::isEntityId`, `CommandInterpreter::importArgument`).
`INFO 12` is still a file when one is called 12, so the tool sends `INFO #12`,
which is always the entity: with a file called `1` or `#1` in the server's
working directory (a mistyped shell `2>1` makes the first), it once answered
"no importer reads files named ''"
(`McpServer.DescribeEntityNamesTheEntityWhateverFilesTheWorkingDirectoryHolds`).
The tool's description of `local` said it imported "in the drawing's own
coordinates rather than reprojecting"; it moves the data, and says so.

An entity's attributes are read a level at a time through
`katana_run_commands` with `PROP TREE` (`docs/cad.md`, "Properties as a tree:
PROP TREE"): `katana_describe_entity` sends `INFO`, which prints every
property, and a surveyed string with thirty attributes on each of a
thousand vertices is thirty thousand lines of it. `PROP TREE`, on the
selection or any scope, answers the top of the tree - `Asset`,
`vertex` with how many names and values are beneath each - and `PROP TREE
UNDER vertex/3` one vertex's, in pages (`FROM`, `LIMIT`), as the window's
Properties panel reads them. No tool of its own was added: the verb's
records are already the structured answer, and a tool would be a second
spelling of it.

A survey field file - any format the survey import reads, the opcode field
file (`.fld`) and the Sokkia SDR file (`.sdr`,
`McpServer.AnAgentReadsAndImportsASokkiaSdrFile`) among them - is read and
imported the same way:
`SURVEY READ <file>` answers what the reader made of it in records (`survey`,
`content`, `declared`, each `warning` with its record), and `SURVEY IMPORT
<file>` imports it as a survey job, one undo step, answering with the job and
the reduction's warnings (`docs/survey.md`, "The opcode field file (.fld)"),
and with a `resection` record for each setup the reduction positioned by
resection: the setup, its point, the points it was computed `from`, its
northing, easting and height, their one-sigma precision (a priori, scaled by
the variance factor only where the resection's global test fails above), its
redundancy (plan + heights), how many of its residuals were `flagged`, its
geometry's `dilution` and whether that is `weak`, how many `checks` after the
file's resection block checked it, the `file_offset` from coordinates the file
gave for the station (or `none`), the setup it was `radiated_from` before its
own block resected it and the `radiation_offset` from there (or `none`), and
an `unapplied_scale_factor` where it fitted ground distances to grid
coordinates (or `none`). A setup that was not resected - its observations fit
two positions alike, or do not fix it - is in the warnings, with why. After
it, one
`resection_residual` record per residual the outlier test flagged or
rejected - the observation, its residual and `unit` (`rad` or `m`), its
a-priori `sigma`, `standardised` value and `state` - so an agent sees which,
however many other warnings come first
(`cli.survey_import_free_station_field_file`,
`cli.survey_import_resection_field_file`; `docs/survey.md`, "The reduction's
resection").
The import takes every reduction option of the wizard's Reduction step
through `SETTINGS <file>` and `SET <key>=<value> ...`, the settings' own text
form, and answers which settings it ran with (`settings`, and a `setting`
record for each that is not a default). A file that gives no coordinates is
imported by holding a point of the drawing: `FORWARD` puts a named point
there, and `SET control=<id>;drawing;fixed;0;fixed;0;fixed;0` holds it
(`McpServer.AnAgentImportsAFileWithNoCoordinatesHoldingAPointOfTheDrawing`;
`docs/survey.md`, "The reduction settings of SURVEY IMPORT"). The reply then
says where each point was held (`held`, with the drawing's entity) and what
the adjustment made of the data (`reduction`, and an `adjustment` record of
each run: its observations, unknowns and redundancy, variance factor, global
test, and the outliers flagged and rejected), so an agent is told of a
network whose global test failed as a person is; an id the drawing has at
two places is refused, naming both. The command tool's description names
these verbs, so an agent finds them from the tool list
(`McpServer.TheCommandToolNamesTheSurveyFieldFileVerbs`). No tool of its
own: the lines are the whole of it, and the reply's records are the
structured answer.
`CRS SET` takes a WKT as the agent sends it, quotes and all
(`McpServer.AnAgentImportsAFieldFileAndSetsTheSystemByItsWkt`); it once lost
them and refused every WKT whose names hold a blank.

`SURVEY IMPORT` also codes and strings what it draws, in that one undo step,
wherever survey codes are loaded: the points go to the layers and styles
their codes give them and are joined into lines, unless `CODES off` or
`LINEWORK off` says not to for the one line, or the customisation's two
switches do for every import. The reply says what was done in two more
records, after the reduction's warnings and before the `resection` records:
`coded points= matched= unmatched_codes= layers= styles=`, then an
`unmatched_code` record for each code no rule answers (the first ten, then
`unmatched_codes_more=`), and `linework lines= unplaced= layers= styles=`,
each counting the layers and styles its own step made. A step that did not
run answers `coded none reason=<word>` or `linework none reason=<word>` -
`off`, `no-survey-codes`, `no-codes-in-file`, `no-rule-matches` or
`no-points` - and none of them fails the line (`docs/survey.md`, "SURVEY
IMPORT codes and strings what it draws"). Points already in the drawing are
strung by `LINEWORK [<scope>] [WHERE k=v ...] [ORDER number|entity]
[PREVIEW]`, a verb of the shared interpreter that answers in records as well
and leaves out, counted, the points their survey job has strung and the lines
the drawing already holds (`docs/survey_coding.md`, "`LINEWORK` on the
command line"; `HELP LINEWORK`). Not done: the command tool's description
names neither the two words nor `LINEWORK`, so an agent finds them through
`katana_help` and not from the tool list alone.

The annotation styles are read and changed through `katana_run_commands`
with the verbs the window's managers send (`docs/annotation.md`): `DIMSTYLE
INFO name` answers one record of every field, the layers that name the style
and what a dimension of ten reads as, and `DIMSTYLE SET name field value
[field value ...]` changes several fields as one undo step
(`McpServer.DimensionStylesAreSetInOneStepAndReadBackAsARecord`). A text is
edited as the window's Edit Text edits it, several keys in one `TEXTEDIT`
with `\n` for a line break, and `katana_describe_entity` reads it back with
the break written the same way
(`McpServer.ATextIsEditedInOneStepAndDescribedWithItsLineBreaks`). The label
rules tab's Preview is `AUTOLABEL PREVIEW` of the rules named, which answers
the counts and a `rule=NAME labels=N` record a rule, and `LABELSTYLE VALUES
kind` lists what a template may say
(`McpServer.ChosenLabelRulesArePreviewedByNameWithACountForEach`).

Each tool that runs commands answers with a transcript (`> command`, then what
it printed and any error or warning) and, to clients of MCP 2025-06-18 or later,
the same as structured content: each command's `ok`, `output` and `messages`,
and `status`, the drawing's state after the call. `isError` is true when a
command failed, so the model sees the failure and the reason.

An argument a tool does not declare is refused before anything runs, naming
it and the arguments the tool takes. Every schema says
`additionalProperties: false`, but a client need not check it, and the
server used to ignore such an argument: `{"split": true}` given to
`katana_export` (whose word is `split_by_layer`) wrote one layer where the
agent had asked for one per drawing layer, and said nothing
(`McpServer.AnArgumentTheToolDoesNotDeclareIsRefusedByNameAndNothingRuns`).

The help (`katana://help`), the status (`katana://status`) and the
customisation (`katana://customisation`, "The customisation" below) are also
resources, for a client that attaches context rather than calling tools. The
status - the tool's text and structured content, and the resource - is the
interpreter's `STATUS` and `STATUS JSON` (`docs/cad.md`, "STATUS"), so an
agent driving the window or `katana_cli` reads the same record. With the GIS
module, `katana://formats` is the fourth: `FORMATS JSON`, every format this
GDAL reads and writes (`katana_formats`, below).

Annotation needs no tool of its own: its verbs reply in `key=value` records
(`docs/annotation.md`, "The verbs"), which come back in each command's
`output`. `LABEL LAYOUT` names every label piece that found no room on a
line of its own (`label=4 piece=0 suppressed=yes`), so an agent can select it
or pin it elsewhere with `LABEL SET id at=x,y`, as the window's Label Layout
Report does (`McpServer.LabelLayoutNamesTheLabelsWithNoRoomSoAnAgentCanMoveThem`).

## The customisation

A session's customisation is what turns field codes into a drawing: the
linestyle and symbol definitions, the survey code rules, the colours and the
settings (`docs/customisation.md`). An agent reads it as data with one tool
and one resource, and changes it with the `CUSTOMISE` lines every front end
runs.

### Reading: katana_customisation and katana://customisation

| In | Out |
|---|---|
| `{part: "summary"}` | cad's report as one object, with no `part` or `total` beside its members: `name`, `origin`, `kept`, `description`, `notice`, `basedOn`, `builtIn`, `sources` (each with its notice), `counts`, `automation`, `linework`, `colours`, `problems` (counted), `coverage`, `missing`, `start` - what `CUSTOMISE JSON` prints (`docs/customisation.md`, "The replies") |
| `{part: "codes", filter?, offset?, limit?}` | `{part, total, offset, limit, codes: [{index, key, sets, ...}]}`: one object a survey code rule |
| `{part: "definitions", filter?, offset?, limit?}` | `{part, total, offset, limit, definitions: [{name, kind, group, units, atVertices, from}]}` |
| `{part: "definitions", name}` | the same with the ONE definition of that name, whole: those six members and everything else its file holds, `strokes` included |
| `{part: "problems", filter?, offset?, limit?}` | `{part, total, offset, limit, rules, cannotApply, warnings, problems: [{index, key, sets, severity, kind, message}]}`: the lint `CODE CHECK` prints |

- **The summary is one object, and the only part with no envelope.** It is
  `cad::customisationJson` whole: no `part`, `total`, `offset` or `limit`
  stands beside cad's members, and the three lists alone are `{part, total,
  offset, limit, <part>: [...]}`. The tool's description says so, since a
  client that looked for `part` in every reply found none in this one.
  *Rejected: adding `part: "summary"` beside cad's members, or putting the
  object inside the lists' envelope.* The tool, the resource and `CUSTOMISE
  JSON` would then give three different objects for one report, and a member
  cad adds later could meet one of the tool's own under the same name. A
  caller knows which part it asked for.
- **`start` is what went wrong when the session started**:
  `{problems: [sentence], keptFromAnotherBuiltIn}`. A kept file that did not
  read - and what started in its place - and a built-in that did not read are
  `problems`; a kept customisation made from another built-in than the program
  has is the flag. Both are also printed once at the start, on standard
  error, which is the client's log and never reaches the agent: one whose
  kept file had been refused read `origin: builtIn` in the summary and
  nothing of why. It is always there, `[]` and `false` when the start had
  nothing to say, and it is of the START - a load or an edit since does not
  change it (`docs/customisation.md`, "What katana_cli and katana_mcp start
  with").
- **A rule and a definition are the objects a file holds, and one member
  more.** Each is written by the Katana customisation format's own writer
  (`entity::customisationToJson`, `entity::definitionToJson`) and read back as
  JSON, so a member is spelt here exactly as `docs/customisation.md`, "The
  members", spells it. The tool has no words of its own for what a
  customisation holds. A member at its default is left out, as in a file.
  *Rejected: building the objects from the model by hand*, which is a second
  writer of every member the format has one for, and would go on saying an
  old word after the format's table changed.
  **`index` (a rule) and `kind` (a definition) are the tool's own and no
  file's.** A file says a rule's place by its order in `codes` and a
  definition's kind by the list it is in, `linestyles` or `symbols`, and its
  reader is strict: an entry copied into a file whole is refused for that
  member (`codes[0] "WM*": unknown member "index"`). With it taken off, a
  rule, or a definition read whole by `name`, is an entry of a file and loads.
  An entry of the definitions LIST is not: it has no strokes, and merged in
  it would put a definition that draws nothing in the place of the one of
  its name.
- **`index` is a rule's identity.** It is its place in the map, which is its
  precedence and what `CODE EXPLAIN` and `CODE CHECK` cite it by ("rule #57");
  an issue of `problems` carries the `index` of its rule, so the two lists
  join.
- **A definition's list entry says six things for every definition** - `kind`
  is `linestyle` or `symbol`, by the list of its file it sits in; `units` and
  `atVertices` are said even where a file leaves them at their default;
  `from` is the customisation it came from - and never its strokes. A
  library is tens of thousands of strokes; they come only for the one
  definition `name` picks, matched as it is written.
- **The lists are paged.** `total` is how many entries `filter` took,
  `offset` and `limit` choose the page (100 unless said, at most 1,000), and
  an offset past the end is an empty page. `filter` is a substring with
  letter case ignored: of a rule's key, comment or layer; of a definition's
  name or group; of an issue's key, kind or message. `rules`, `cannotApply`
  and `warnings` count the whole lint, whatever the filter - the three
  numbers the summary's `problems` has, by the same names.
- **An argument that says nothing for the part is refused**, as an
  undeclared one is: a `filter` on the summary, a `name` with `codes`, a
  `filter` beside a `name`. Ignored, the call would answer something other
  than what was asked.
- **The text is the same object**, for a client older than structured
  content.
- **It reads the Document, not a line.** The verbs that list a customisation
  reply in text made for a person - a line a code, not an object a rule -
  and a read sent through a line would sit in the command history of a
  session it never changed. `katana://customisation` is the summary, the same
  function (`cad::customisationJson`), as `katana://status` is the status.

`McpServer.TheCustomisationSummaryIsCadsReportAsAToolAndAsAResource`,
`McpServer.TheCodesPartGivesEachRuleAsItsFileHoldsItFilteredAndPaged`,
`McpServer.TheDefinitionsPartListsEachDefinitionAndGivesStrokesOnlyForTheOneNamed`
and `McpServer.TheProblemsPartIsTheRulesLintAnObjectAnIssue` pin them, on the
three files of `tests/data/customisation` and on what their text holds.
`McpServer.ARuleOrADefinitionTheToolGaveLoadsBackOnceTheToolsOwnMemberIsTakenOff`
loads a rule and a definition back both ways, refused with the tool's member
and read back the same without it. The two tests of `start` run a PROGRAM's
session, as `katana_mcp` has one, with the two start-up variables set:
`McpServer.WhatWentWrongWhenTheSessionStartedIsInTheSummaryAndTheResource`
(the kept file holds a rule copied from the tool, `index` and all) and
`McpServer.AKeptCustomisationMadeFromAnotherBuiltInIsSaidInTheSummary`.

### Changing: export, edit the JSON, load it back

There is no tool that edits a rule or a definition, and none is wanted
(`docs/customisation.md`, "The verbs: decisions, and what was rejected": the
file is the one door for an edit). An agent edits a customisation as a person
with an editor does, through `katana_run_commands`:

1. `CUSTOMISE EXPORT "<file>"` writes the session as ONE Katana customisation
   file, in JSON, a rule and a stroke a line. With `CODES`, `LINESTYLES`,
   `SYMBOLS` or `ONLY <definition>...` it writes that part alone.
2. The agent changes the JSON: a rule's `layer`, a new entry in `codes`, a
   definition's strokes. The format is strict - a member it does not know, or
   a value of the wrong kind, is refused naming the entry - so a mistake is a
   refusal that says where, not a rule silently dropped. An entry taken from
   `katana_customisation` goes in without the tool's own member, `index` or
   `kind` ("Reading", above).
3. `CUSTOMISE REPLACE "<file>"` loads it in the place of what is loaded, or
   `CUSTOMISE "<file>"` MERGES it: a definition takes the place of the one of
   its name, and the rules a file gives a key in a section take the place of
   that key's rules there. So a file holding only what changes - two rules,
   one symbol - is an edit, and nothing else in the session moves.

`CUSTOMISE REMOVE` deletes definitions and `CUSTOMISE REMOVE CODE` a key's
rules (a merge cannot delete), `CUSTOMISE SET` sets the two automation
switches and the linework codes, and `CUSTOMISE` alone, or the tool above,
says what the session holds afterwards. A load is all or nothing: one file
that does not read, or one thing refused in one, loads none of them, and the
reply lists every reason. A survey code file (`.mapfile`) or a style library
(`.4d`) of another program is not a Katana customisation file and is refused
as that.

A change lasts the session. `CUSTOMISE KEEP` makes it what the next start
gives, and is refused unless the server was started with
`KATANA_CUSTOMISATION` naming the file to keep it in: an agent trying a
customisation must not be rewriting its user's by accident. A kept file that
does not read at the next start is not what starts - the built-in is - and
the summary's `start.problems` says so and why.

`McpServer.ACustomisationIsEditedByExportingItChangingTheJsonAndLoadingItBack`
runs the round trip: it exports, moves one rule to another layer in the JSON,
loads the file back and reads the rule with the tool. `katana_run_commands`'
description names the lines, and `katana_customisation`'s says that it only
reads and where an edit goes
(`McpServer.TheToolListSaysHowACustomisationIsLoadedWrittenAndRead`).

## Geoprocessing tools

GDAL's algorithm framework (`docs/geoprocessing.md`) as tools, in
`src/katana_app/geo/mcp_geo_tools.cpp`, with the GIS module only. They are
built from the same helpers as the tools above (`src/katana_app/mcp_tools.hpp`:
`Tool`, `ToolReply`, `ToolRefusal`, `objectSchema`, `hints`, `oneLine`),
which moved there from `mcp_server.cpp` unchanged so that a tool in another
file is the same kind of thing. One subsection per geoprocessing package;
each adds its tools in its own block of `mcp_geo_tools.cpp`.

### F0: katana_gdal_catalogue, katana_gdal_describe, katana_gdal_run

| Tool | In | Out |
|---|---|---|
| `katana_gdal_catalogue` | `{filter?, schemas?}`: a group (`raster`, `vector grid`) or words to look for | `{gdal_version, algorithms: [{path, name, description, aliases, policy, url, container, arguments_schema?, inputs_schema?}]}`; the schemas only when 20 or fewer are listed |
| `katana_gdal_describe` | `{algorithm}`: `"raster hillshade"`, aliases taken | `{algorithm, arguments: [{name, short_name, aliases, type, required, positional, category, default, choices, min: {value, inclusive}, max, count: {min, max}, dataset: {kinds, accepts, update, sources}, depends_on, exclusion_group, dependency_group, input, output, description}], arguments_schema, inputs_schema, gdal_usage}` |
| `katana_gdal_run` | `{algorithm, arguments?, tokens?, inputs?, output?, confirm?, overwrite?, preview?}` | `{ok, line, algorithm, inputs, scope: [{arg, matched, used, points, lines, polygons, skipped: {reason: n}}], outputs: [{arg, kind, target, layer?, created?, id?, name?, raster?: {width, height}, file?}], text?, return_code?, warnings, cancelled, seconds}` |

- **Read-only tools read the bridge.** `katana_gdal_catalogue` and
  `katana_gdal_describe` change nothing, so they read the bridge directly;
  a line would add nothing. Their schemas are generated from the argument
  specs GDAL declares (`src/katana_app/geo/schema.hpp`). An algorithm's
  `arguments_schema` is the JSON Schema of `katana_gdal_run`'s `arguments`,
  and its `inputs_schema` that of its `inputs`. Each input is offered only
  the sources its kinds read (`dataset.sources`): a raster input no drawing
  scope, a vector input no raster or surface, a file to every input
  (`GeoExecutor.TheInputsSchemaOffersOnlyTheSourcesAnArgumentReads`). A
  source of another kind given anyway is refused by the GDAL verb's binder
  before anything runs, as it is on the command line and in the window
  (`McpServer.GdalRunRefusesASourceTheInputDoesNotRead`).
- **`katana_gdal_run` builds a line.** It builds the GDAL line a person would
  type and runs it through the Session:
  - `arguments` become GDAL's `--name=value` words; a dataset argument there
    is refused, since datasets are `inputs`;
  - `tokens` are GDAL's own words, as they are;
  - each input becomes a FROM clause; a source is `{scope: "selection" |
    "drawing" | "area" | "layers", area?, layers?, only?, where?}`, written
    by the shared `cad::formatScopeWords`, or `{raster}`, `{surface, cell?}`,
    `{file, layer?}`; `{where}` without `scope` is the selection, filtered,
    as `WHERE` alone reads on the command line, and is written `SELECTION
    WHERE ...` (so is `katana_export`'s `where` without `scope`);
  - `output` becomes the TO clause: `{layer}`, `{reference}`,
    `{file, format?, overwrite?}` or `{surface}`.

  The text echoes the line (`> GDAL ...`) with the records the command line
  prints; the structured content is those records, read back by
  `geo::parseRecords`, so an agent reads what the scope matched and what was
  made without parsing text.
- **Confirm algorithms are gated.** An algorithm whose policy is confirm - it
  changes or removes existing data: vsi delete, dataset rename, raster edit
  ... - is refused unless `confirm: true`. The verb refuses it again without
  CONFIRM. The tool's hints are destructive and open-world, since a FILE
  source may be a URL.
- **So are a pipeline's steps and words.** A pipeline is a Safe leaf, but a
  pipeline with an `update` step writes into a dataset already there: it
  needs `confirm: true` too, however `tokens` quote it. GDAL's own words
  that change an existing dataset (`--overwrite`, `--append`, `--update`,
  `--upsert`, `--overwrite-layer`, `--add`, `--resume`), in a pipeline
  or not, need `overwrite: true`, which adds OVERWRITE to the line; the verb
  refuses both again (`docs/geoprocessing.md`, "Safety").

`McpServer.GdalRunRefusesAPipelineThatChangesExistingDataUnlessToldTo`,
`McpServer.GdalCatalogueListsHillshade`,
`McpServer.GdalDescribeGivesASchemaWithBounds`,
`McpServer.GdalRunBuildsTheLineAndReturnsOutputs`,
`McpServer.GdalRunRefusesAConfirmAlgorithmWithoutConfirm` and
`McpServer.GdalRunRefusesAnArgumentTheAlgorithmHasNot` pin them.

### T0: katana_terrain_list

| Tool | Arguments | Structured content |
|---|---|---|
| `katana_terrain_list` | `{}` | `{surfaces: [{name, triangles, points, bounds: [x0, y0, x1, y1], zmin, zmax, plan_area, source}], rasters: [{id, name, role, width, height, cell, crs, nodata, source, derived_from}]}` |

It runs `SURFACE LIST JSON` through the Session, as the command line would,
and hands its JSON back: the surfaces a terrain verb reads as `SURFACE
<name>` and the rasters it reads as `RASTER <id|name>` (`docs/terrain.md`,
"Surfaces on every front end"). A raster's `cell` is null unless its cells
are square and unrotated; `nodata` is null until the reference-layer work
reads it. Read-only and idempotent. Surfaces are made with `SURFACE FROM`
through `katana_run_commands`, or by `katana_gdal_run` with `output:
{surface}`. `McpServer.TerrainListGivesTheSessionsSurfacesAndRasters` pins
it.

### T1: CONTOUR through katana_run_commands

No tool of its own: `CONTOUR` is a session line, so `katana_run_commands`
runs it exactly as the window's Terrain > Analysis > Contours does, and its
reply is the verb's records (`contours method= cell= levels= count= major=
minor= layer= smoothed=`, the scope's `areas used= skipped.open=`). What an
agent needs to choose a source is `katana_terrain_list`'s. A tool would only
restate the verb's options as a schema, and the plan gives CONTOUR none
(`docs/terrain.md`, "Contours").
`McpServer.ContoursOfASurfaceAreDrawnThroughTheCommandTool` pins it.

### T2: RASTER SHADE through katana_run_commands

The same: `RASTER SHADE` is a session line, and its reply carries the
`ramp` and `legend value= r= g= b=` records an agent needs to describe the
picture. The picture is a derived reference raster, which
`katana_terrain_list` then lists with `role: "derived"` and the line in
`derived_from`.
`McpServer.AShadingMadeThroughTheCommandToolIsListedAsADerivedRaster` pins
it.

### T3: RASTER SLOPE and RASTER ASPECT through katana_run_commands

The same again: the lines run through `katana_run_commands`, and the reply's
`class name= from= to= unit= area= polygons=` records are the per-class area
report an agent reads; the areas are one undo step (`UNDO` takes them all).
The slope raster is listed by `katana_terrain_list` like any derived raster.
`McpServer.SlopeClassesMadeThroughTheCommandToolAreOneUndoStep` pins it.

### T4: RASTER ZONAL, RASTER SAMPLE and DRAPE through katana_run_commands

No tool of their own, as the plan gives them none: the lines run through
`katana_run_commands`, and their replies are the records an agent reads -
`zone entity=<id> mean= count= ...` per zone, `sample at=x,y z=` (or
`ground=no`, never 0) per point, and the drape's `drape method= entities=
vertices= off=`. ZONAL and DRAPE change the drawing in place, one undo step
each. `McpServer.StatisticsByAreaThroughTheCommandToolAreWrittenOnTheLot`
pins it.

### T5: RASTER VIEWSHED and LOS through katana_run_commands

The same: the lines run through `katana_run_commands`. The viewshed's
reply carries `observer at= visible_cells=` per observer and the summary's
`visible_cells=` and `area=`, and the viewshed itself is a derived raster
`katana_terrain_list` lists; LOS answers with one `sight visible= ...
clearance= blocked_at=` record.
`McpServer.ALineOfSightThroughTheCommandToolSaysWhetherTheTargetIsSeen`
pins it.

### V5: katana_gis_query

| Tool | In | Out |
|---|---|---|
| `katana_gis_query` | `{sql, scope?, area?, layers?, only?, where?, dialect?}`: one SELECT; the scope `drawing` (the default), `selection`, `area` or `layers`, with the shared filter; `sqlite` (the default, Spatialite's `ST_` functions) or `ogrsql` | `{ok, line, columns, column_types, rows: [{column: value}], matched, used}` |

An agent's question of the drawing in one call - "the easement area per
owner" - answered as typed JSON (`src/katana_app/geo/sql_mcp.cpp`,
`docs/geoprocessing.md` "V5"). It builds the line a person would type,
`GIS SQL "<sql>" <scope> dialect=<d>`, runs it through the Session, and reads
the `column` and `row` records back: each cell is typed by its column's type
(`integer` and `real` as numbers, `boolean` as true or false), and a column a
row has no cell for is null. The tables are `points`, `lines` and `polygons`
with `katana_id`, `layer`, `style`, `colour`, `type` and every property; the
geometry column is `geometry`. Only a SELECT runs, and it changes nothing
(read-only hint). A double-quoted identifier is written as a
`[bracketed]` one, which SQLite reads alike and a command line can carry.
`GisQueryMcp.GisQueryReturnsRowsAsJson` and
`GisQueryMcp.AStatementThatIsNotASelectIsRefused` (in `tests/geo`) pin it.

### I2: katana_formats

- **`katana_formats`** lists the formats this build of GDAL reads and
  writes, from its own registry (`docs/interop.md`, "Formats"): each
  driver's name - what EXPORT's and GDAL's format arguments take - its
  description, the kinds of data it holds, reads and writes, its extensions,
  whether it opens `/vsi` paths, its connection prefix and GDAL's page for
  it. `kind` (`raster` | `vector`), `capability` (`read` | `write`) and
  `filter` (words each of which the name, description or an extension
  holds) narrow the list. `driver` gives that one driver's open, creation
  and layer-creation options instead: each option's name, type, default,
  choices, bounds and description. Read-only, as `katana_gdal_catalogue` is.
- **The resource `katana://formats`** is `FORMATS JSON`: every format, the
  same objects the tool returns.

The tool and the resource are built from the verb's own chooser and records
(`src/katana_app/geo/formats_verbs.hpp`), so `FORMATS`, the tool and the
resource cannot come to differ. The text is the verb's records; the
structured content is `{gdal_version, formats: [...]}`, or the driver's
`{format, open_options, creation_options, layer_creation_options}`. An
unknown driver, or a `kind` or `capability` that is neither word, is
refused. `McpServer.FormatsReturnsStructuredDrivers` pins it, and
`McpServer.TheHelpAndStatusAreResources` counts it among the resources: four
with the GIS module and three without, each one more than before
`katana://customisation` was added.

### D1: katana_dataset_info

| In | Out |
|---|---|
| `{path, layer?, stats?, check?, json?}`: a file, a folder, a `/vsi` path or a URL | `{ok, line, records: [{record: "dataset" \| "raster" \| "band" \| "overview" \| "subdataset" \| "layer" \| "field" \| "pointcloud" \| "found" \| "check" \| "problem" \| "warning", ...fields}], gdal?}` |

- **It is the INFO line.** The tool builds `INFO "<path>" [LAYER <name>]
  [STATS] [CHECK]`, runs it through the Session and hands back its records as
  objects (`geo::recordsJson`): numbers as numbers, `yes`/`no` as booleans,
  `bounds` as `[x0, y0, x1, y1]` and any other comma list of numbers as an
  array (a scope's `area`, a grid's `cell` of `4,3`), an empty value as null
  - absent, not zero. A record's `record` is every word before its first
  `key=`: IFC's `ifc exported file=...` is `{"record": "ifc exported",
  "file": ...}`, where one word made it `ifc` with a field `exported file`
  (`GisRecords.AKindIsEveryWordBeforeTheFirstKeyAndANumberListIsAnArray`,
  `McpServer.AnIfcExportsRecordIsItsWholeKindWithItsFields`). A list that is
  not all numbers (`stats=mean,min`) stays a string: its words are the
  verb's, and splitting some lists and not others on the kind of item would
  be a second rule.
- **`json: true` adds GDAL's own description** as `gdal`, from `INFO ...
  JSON`: `raster info`, `vector info` and, for a multidimensional format,
  `mdim info`, verbatim.
- **It reads, and writes nothing**: `stats` computes the bands' statistics
  without leaving an `.aux.xml` beside the file. Its hints are read-only and
  open-world, since a path may be a URL.

`McpServer.DatasetInfoReturnsRecordsAndGdalsJson` pins it; the records are
`docs/interop.md`'s ("Dataset information").

### D2: katana_references

| In | Out |
|---|---|
| `{action?: "list" \| "show" \| "hide" \| "remove" \| "info" \| "opacity" \| "color" \| "rename" \| "overviews" \| "restore", id?: integer \| string, value?: number \| string, confirm?}` | `{ok, line, records: [...], references: [{record: "reference", id, kind, name, visible, opacity?, color?, ...}], missing: [...]}` |

- **It is the REFS line** (`docs/interop.md`, "Reference layers"): built
  from the action, the layer's id or name and the value, run through the
  Session, its records handed back as objects - then `REFS JSON`, so every
  reply carries the layers as they are after it, each with the id the next
  call names it by.
- **Overviews need `confirm: true`**: they are written beside the raster's
  file. The tool refuses without it, and the verb refuses a line without
  CONFIRM.
- **`restore`** reads again the layers the project records, as opening it
  does; `missing` lists those whose files could not be read.

`McpServer.ReferencesActsOnALayerByIdOrNameAndListsThemAfter` pins it.

### I3 and I4: katana_import and katana_export options

`katana_import` takes IMPORT's options (`docs/interop.md`, "Import
options") as arguments and builds the line a person would type, run through
the Session like any other; the structured reply's `commands` shows it.

| Argument | Word |
|---|---|
| `layers` [names] | `layers=a,b` |
| `where` | `where="..."` (OGR SQL WHERE, applied by the driver) |
| `sql`, `dialect` | `sql="SELECT ..."`, `dialect=ogrsql\|sqlite` |
| `scope` (selection, drawing, area, layers), `area` [x0, y0, x1, y1], `scope_layers`, `only`, `scope_where` [conditions] | the shared scope words; `area` alone is `AREA` |
| `clip` | `clip` |
| `fields` [names], `attributes: false` | `fields=a,b`, `attributes=no` |
| `target_layer`, `max_features` | `target=`, `max=` |
| `open_options` ["KEY=VALUE"] | `oo=KEY=VALUE` each, checked against the driver's list |
| `crs` (project, adopt), `source_crs` | `crs=`, `srs=` |
| `band`, `subdataset`, `max_pixels`, `name` | a raster's `band=`, `subdataset=`, `maxpixels=`, `name=` |
| `budget`, `classes`, `resolution`, `name` | a cloud's `budget=`, `class=`, `resolution=`, `name=` |
| `preview` | `PREVIEW`: the `import ... preview=yes features= of=` record, nothing imported |

- The scope's own conditions are `scope_where`, not `where`: `where` is the
  file's attribute filter, which the plan's contract named first.
- A value holding a double quote or a line break is refused: no line can
  carry one (in SQLite, write an identifier as `[name]`).
- The scope in JSON is read by the one reader, `mcp::scopeWordsOf`, which
  `katana_gdal_run` and `katana_gis_query` now share.

`McpServer.ImportTakesItsFilterScopeAndPreviewArgumentsAsTheWordsAPersonTypes`
pins the lines and records.

`katana_export` takes EXPORT's scope and options (`docs/interop.md`,
"Export options") likewise:

| Argument | Word |
|---|---|
| `scope` (selection, drawing, area, layers), `area`, `layers`, `only`, `where` [conditions] | the shared scope words; none is the whole drawing |
| `layer_name` | `layername=` |
| `split_by_layer` | `split=layer` |
| `append` | `append` |
| `crs` (project, native, a code) | `crs=` |
| `creation_options`, `layer_creation_options` ["KEY=VALUE"] | `co=`, `lco=` each, checked against the driver's lists |
| `text` (points, skip) | `text=` |
| `curve`, `properties` | `curve=`, `properties=yes\|no` |
| `preview` | `PREVIEW`: the `export ... preview=yes entities=` record, nothing written |

Here `layers` and `where` are the scope's (katana_export has no file to
filter); `structuredContent.records[0]` is still the `exported` record, the
`scope` record after it.
`McpServer.ExportTakesTheSharedScopeAndItsOptionsAsTheWordsAPersonTypes` pins
them.

## What the server adds to the command line

Three rules that a typed command does not have, because a model is not a person
watching the window:

* **Unsaved work is not discarded silently.** `NEW` and `OPEN` - through their
  tools or typed in a batch - are refused while the drawing has unsaved changes,
  unless the call says `discard_unsaved_changes: true`. The refusal names the
  way out: save first, or say so.
* **One line is one command.** A command with a line break in it is refused
  rather than run as two, and a path with a double quote in it is refused
  rather than cut short at the quote (the interpreter's quoted words have no
  escape).
* **`QUIT` is not a command here.** The client ends the session by closing it;
  a `QUIT` in a batch is reported and skipped.
* **The window's own verbs are refused by name.** `PLOT`, `PLOTSHEETS`,
  `SNAPSHOT`, `ONLINE`, `SCRIPT`, `GRID` and `EXAGGERATION` run only in the
  desktop window (its painter is Qt's; its online workbench keeps the
  provider keys; a script is `katana_run_script`'s here; the grid and the
  exaggeration are how its views draw), and so do `VIEWS` and `ZOOM`, which
  act on its views (`docs/cad.md`, "The window's views: VIEWS and ZOOM") -
  the interpreter refuses those two itself, having no views here. The
  session answers each with `Unsupported: <VERB> is the desktop window's` and
  where to run it instead - in `katana_cli` too - where it once said "unknown
  command", which a model cannot tell from a typo (`GRID` and `EXAGGERATION`
  did until 2026-09-30); `katana_run_commands`' description says so as well
  (`McpServer.TheWindowsOwnVerbsAreRefusedSayingWhereTheyRun`, a `cli.` test
  for each verb in `src/katana_app/CMakeLists.txt`, and
  `cli.a_window_verb_in_a_batch_fails_it`). An agent that needs linked views
  drives the window with `katana --command "VIEWS LINK 1,2"`
  (`docs/headless.md`).

IFC 4.3 is reached as the window's IFC dialogs reach it, by lines: `EXPORT
<file.ifc> ... PREVIEW` for the class each object would become, `EXPORT`,
`INFO` and `IMPORT` with every choice the dialogs offer, and `IFC RULES` for a
project's rules to start from (`docs/ifc.md`, "Using it"). Their replies are
`key=value` records ("Replies" there), which an agent reads as it reads any
other verb's (`McpServer.AnAgentPreviewsExportsDescribesAndImportsIfcByTheDialogsLines`).

Nothing is written to disk until a `SAVE`. Opening a project can back it up or
migrate it in place (`docs/headless.md`), so point Claude at a copy of a
project that matters.

## How it is built

* `src/katana_app/session.cpp` - the session both front ends share: the
  Document, its `CommandInterpreter`, and the verbs above `katana_cad`
  (`IMPORT`, `EXPORT`, `INFO <file>`, `REFS`, `COPC` and the
  geoprocessing verbs through the executor in `src/katana_app/geo/`, the one
  the window runs; `SURVEY`; a build without GDAL has the DXF `IMPORT` and
  `EXPORT` of `dxf_verbs.cpp`). When it starts it builds the customisation's
  host and starts the Document with it.
  `CODE` was here until 2026-09-26 and is the interpreter's now, with `CODE
  LIST` and `CODE CHECK` (once a verb of their own, `MAPFILE`), so the window
  has them too (`docs/cad.md`); `CUSTOMISE` was here, with a parser and a
  loader of its own, until 2026-10-06, and is the interpreter's family now
  (`docs/customisation.md`, "The verbs"). It was
  `katana_cli`'s `main.cpp`; `src/katana_app/main.cpp` is now only the command
  line's argument handling.
* `src/katana_app/mcp_server.cpp` - `Server::handle`: one JSON-RPC message in,
  its reply out, and the tools. It has no I/O of its own, which is what lets
  `tests/app/test_mcp_server.cpp` drive it message by message.
* `src/katana_app/mcp_customisation.cpp` - `katana_customisation`, built from
  the helpers of `src/katana_app/mcp_tools.hpp` as the geoprocessing tools
  are.
* `src/katana_app/mcp_main.cpp` - the process: the stdio transport
  (newline-delimited JSON-RPC 2.0) and `--project`.

**stdout belongs to the protocol.** A single stray line on it - a warning a
verb prints, a library's `printf` - and the client reads a message that is not
JSON and drops the connection. So each command runs with `std::cout` and
`std::cerr` captured into the reply, and, below that, `mcp_main.cpp` duplicates
file descriptor 1 for the protocol and points descriptor 1 itself at stderr
before the session is created: whatever else writes to "stdout" lands in the
client's log of the server. The test
`McpServer.NothingACommandPrintsLeaksOntoTheRealStreams` pins the first half.

**Protocol.** The server speaks MCP 2025-06-18 and agrees to 2025-03-26 or
2024-11-05 when a client asks for one (`kLatestProtocolVersion`); structured
content is sent only from 2025-06-18, where it was introduced. It answers
`initialize` and `ping`, the tool methods (list and call), the resource
methods (list, read and the empty templates list) and the empty prompt list,
answers a
JSON-RPC batch member by member, never answers a notification, and replies to
anything else with the JSON-RPC error the specification names.

## Not done yet

* **The window is not driven.** The server runs its own session on project
  files; it does not reach into a running `katana` window. To see the result,
  save and open the project in the window. A bridge into the window (the same
  server behind a local socket, acting on `MainWindow`'s document) is the
  natural next step.
* **No pictures, no plots, no online data.** A tool that plots the drawing
  with the window's headless `--plot` or `--screenshot` (`docs/headless.md`)
  and returns the image would let Claude check its own work by eye; `PLOT`,
  `PLOTSHEETS` and `SNAPSHOT` need a painter the session has not got (Qt's),
  and `ONLINE` the window's key store and job runner. Moving the online verbs
  behind a service the session can call, and giving the server a plot and a
  picture tool that run the window headless, is the work that closes this.
  `SNAPSHOT` covers the plan and 3D views; the whole window, which
  `--screenshot` grabs, has no verb.
* **Only what the command line has.** Terrain surfaces, corridors, grading
  and sections exist in `katana_cad` but have no command-line verbs yet, so
  no tools either; each verb added to the interpreter or the session
  reaches Claude with no change here. The sheets have their verbs
  (`docs/plotting.md`, "Sheets on the command line"), so an agent lays out,
  saves, loads and appends sheet sets through `katana_run_commands` with the
  lines the Sheets editor's dialogs and Sheet Set menu run.
