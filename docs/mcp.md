# Claude and Katana: the MCP server

`katana_mcp` serves the Katana engine over the Model Context Protocol (MCP), so
that Claude - in Claude Desktop, Claude Code or any other MCP client - can run
an engineering project in Katana: open a project, draw and edit it, run the
survey calculations, lay out alignments and parcels, apply survey codes,
grade, check and draw located buried services by AS 5488 quality level, and
grade again, report, check and write back as a schedule what is drawn
(`UTILITY REPORT`, `VERIFY`, `CLEARANCE`, `CHECK`, `DRAW`, `REGRADE`,
`SCHEDULE`, `docs/subsurface_utilities.md`), change what a scope and a filter
take (`MODIFY`, `docs/cad.md`, "Scope and filter"), import and export, and
save it. The tool that runs commands names each `UTILITY` action and the
scope words in its description, since that is what a client reads first.
The window can then open the same project.

It is the session `katana_cli` runs (`src/katana_app/session.hpp`), served to a
client instead of typed. Every tool is a shape over the command line's verbs, so
there is one implementation of each, pinned by the `cli.*` tests, and Claude can
do nothing that a person at `katana_cli` could not. Every change is one
undoable command, as it is when typed.

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
| `katana_import` | DXF always; GIS vector, raster and point-cloud files with `KATANA_BUILD_IO`; `placement` moves what a DXF, vector file or .12da archive holds as one piece: `local` (its lower-left corner to 0,0; also `local: true`), `alongside` (onto the drawing's lower-left corner) or `offset` (by `offset_east`, `offset_north`); `keep` by default; anything but keep is refused for rasters and clouds (`docs/interop.md`, "Placing an import"). With `KATANA_BUILD_IO`, `structuredContent.records` holds the reply's records as objects - `imported`, `placed`, `reference`, `surface`, `tally`, `warning` - numbers as numbers and `bounds` as `[x0, y0, x1, y1]` | `IMPORT "<path>" [LOCAL \| ALONGSIDE \| OFFSET=dE,dN]` |
| `katana_export` | DXF always; GIS vector formats and .12da archives (with the session's surfaces) with `KATANA_BUILD_IO`; `structuredContent.records` holds the `exported` record and any `warning` | `EXPORT "<path>"` |
| `katana_undo` | undo, or with `redo` redo, `steps` steps | `UNDO n` / `REDO n` |

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

The help (`katana://help`) and the status (`katana://status`) are also
resources, for a client that attaches context rather than calling tools. The
status - the tool's text and structured content, and the resource - is the
interpreter's `STATUS` and `STATUS JSON` (`docs/cad.md`, "STATUS"), so an
agent driving the window or `katana_cli` reads the same record.

Annotation needs no tool of its own: its verbs reply in `key=value` records
(`docs/annotation.md`, "The verbs"), which come back in each command's
`output`. `LABEL LAYOUT` names every label piece that found no room on a
line of its own (`label=4 piece=0 suppressed=yes`), so an agent can select it
or pin it elsewhere with `LABEL SET id at=x,y`, as the window's Label Layout
Report does (`McpServer.LabelLayoutNamesTheLabelsWithNoRoomSoAnAgentCanMoveThem`).

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
| `katana_gdal_run` | `{algorithm, arguments?, tokens?, inputs?, output?, confirm?, preview?}` | `{ok, line, algorithm, inputs, scope: [{arg, matched, used, points, lines, polygons, skipped: {reason: n}}], outputs: [{arg, kind, target, layer?, created?, id?, name?, raster?: {width, height}, file?}], text?, return_code?, warnings, cancelled, seconds}` |

- **Read-only tools read the bridge.** `katana_gdal_catalogue` and
  `katana_gdal_describe` change nothing, so they read the bridge directly;
  a line would add nothing. Their schemas are generated from the argument
  specs GDAL declares (`src/katana_app/geo/schema.hpp`). An algorithm's
  `arguments_schema` is the JSON Schema of `katana_gdal_run`'s `arguments`,
  and its `inputs_schema` that of its `inputs`.
- **`katana_gdal_run` builds a line.** It builds the GDAL line a person would
  type and runs it through the Session:
  - `arguments` become GDAL's `--name=value` words; a dataset argument there
    is refused, since datasets are `inputs`;
  - `tokens` are GDAL's own words, as they are;
  - each input becomes a FROM clause; a source is `{scope: "selection" |
    "drawing" | "area" | "layers", area?, layers?, only?, where?}`, written
    by the shared `cad::formatScopeWords`, or `{raster}`, `{surface, cell?}`,
    `{file, layer?}`;
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

`McpServer.GdalCatalogueListsHillshade`,
`McpServer.GdalDescribeGivesASchemaWithBounds`,
`McpServer.GdalRunBuildsTheLineAndReturnsOutputs`,
`McpServer.GdalRunRefusesAConfirmAlgorithmWithoutConfirm` and
`McpServer.GdalRunRefusesAnArgumentTheAlgorithmHasNot` pin them.

### T0: katana_terrain_list

Not started.

### V5: katana_gis_query

Not started.

### I2: katana_formats

Not started.

### D1: katana_dataset_info

| In | Out |
|---|---|
| `{path, layer?, stats?, check?, json?}`: a file, a folder, a `/vsi` path or a URL | `{ok, line, records: [{record: "dataset" \| "raster" \| "band" \| "overview" \| "subdataset" \| "layer" \| "field" \| "pointcloud" \| "found" \| "check" \| "problem" \| "warning", ...fields}], gdal?}` |

- **It is the INFO line.** The tool builds `INFO "<path>" [LAYER <name>]
  [STATS] [CHECK]`, runs it through the Session and hands back its records as
  objects (`geo::recordsJson`): numbers as numbers, `yes`/`no` as booleans,
  `bounds` as `[x0, y0, x1, y1]`, an empty value as null - absent, not zero.
- **`json: true` adds GDAL's own description** as `gdal`, from `INFO ...
  JSON`: `raster info`, `vector info` and, for a multidimensional format,
  `mdim info`, verbatim.
- **It reads, and writes nothing**: `stats` computes the bands' statistics
  without leaving an `.aux.xml` beside the file. Its hints are read-only and
  open-world, since a path may be a URL.

`McpServer.DatasetInfoReturnsRecordsAndGdalsJson` pins it; the records are
`docs/interop.md`'s ("Dataset information").

### D2: katana_references

Not started.

### I3 and I4: katana_import and katana_export options

Not started.

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
  `SNAPSHOT`, `ONLINE` and `SCRIPT` run only in the desktop window (its
  painter is Qt's; its online workbench keeps the provider keys; a script is
  `katana_run_script`'s here). The session answers each with `Unsupported:
  <VERB> is the desktop window's` and where to run it instead - in
  `katana_cli` too - where it once said "unknown command", which a model
  cannot tell from a typo; `katana_run_commands`' description says so as well
  (`McpServer.TheWindowsOwnVerbsAreRefusedSayingWhereTheyRun`, a `cli.` test
  for each verb in `src/katana_app/CMakeLists.txt`, and
  `cli.a_window_verb_in_a_batch_fails_it`).

Nothing is written to disk until a `SAVE`. Opening a project can back it up or
migrate it in place (`docs/headless.md`), so point Claude at a copy of a
project that matters.

## How it is built

* `src/katana_app/session.cpp` - the session both front ends share: the
  Document, its `CommandInterpreter`, and the verbs above `katana_cad`
  (`CUSTOMISE`; `IMPORT`, `EXPORT`, `INFO <file>`, `REFS`, `COPC` and the
  geoprocessing verbs through the executor in `src/katana_app/geo/`, the one
  the window runs; a build without GDAL has the DXF `IMPORT` and `EXPORT` of
  `dxf_verbs.cpp`).
  `CODE` and `MAPFILE` were here until 2026-09-26 and are the interpreter's
  now, so the window has them too (`docs/cad.md`). It was
  `katana_cli`'s `main.cpp`; `src/katana_app/main.cpp` is now only the command
  line's argument handling.
* `src/katana_app/mcp_server.cpp` - `Server::handle`: one JSON-RPC message in,
  its reply out, and the tools. It has no I/O of its own, which is what lets
  `tests/app/test_mcp_server.cpp` drive it message by message.
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
