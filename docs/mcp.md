# Claude and Katana: the MCP server

`katana_mcp` serves the Katana engine over the Model Context Protocol (MCP), so
that Claude - in Claude Desktop, Claude Code or any other MCP client - can run
an engineering project in Katana: open a project, draw and edit it, run the
survey calculations, lay out alignments and parcels, apply survey codes,
grade, check and draw located buried services by AS 5488 quality level
(`UTILITY`, `docs/subsurface_utilities.md`), import and export, and save it.
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
| `katana_status` | project, unsaved changes, entity, layer and alignment counts, current layer and style, selection, undo depth | nothing: read from the Document |
| `katana_new_project` | a new, empty drawing | `NEW` |
| `katana_open_project` | open a project directory | `OPEN "<path>"` |
| `katana_save_project` | save in place, or as a project at `path` | `SAVE` / `SAVE "<path>"` |
| `katana_list_entities` | every entity with its id, layer and measurements | `LIST` |
| `katana_describe_entity` | one entity in full | `INFO <id>` |
| `katana_import` | DXF always; GIS vector, raster and point-cloud files with `KATANA_BUILD_IO`; `local: true` moves a DXF, vector file or .12da archive as one piece so its lower-left corner sits at 0,0 (refused for rasters and clouds) | `IMPORT "<path>" [LOCAL]` |
| `katana_export` | DXF always; GIS vector formats with `KATANA_BUILD_IO` | `EXPORT "<path>"` |
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
The tool's description of `local` said it imported "in the drawing's own
coordinates rather than reprojecting"; it moves the data, and says so.

Each tool that runs commands answers with a transcript (`> command`, then what
it printed and any error or warning) and, to clients of MCP 2025-06-18 or later,
the same as structured content: each command's `ok`, `output` and `messages`,
and `status`, the drawing's state after the call. `isError` is true when a
command failed, so the model sees the failure and the reason.

The help (`katana://help`) and the status (`katana://status`) are also
resources, for a client that attaches context rather than calling tools.

Annotation needs no tool of its own: its verbs reply in `key=value` records
(`docs/annotation.md`, "The verbs"), which come back in each command's
`output`. `LABEL LAYOUT` names every label piece that found no room on a
line of its own (`label=4 piece=0 suppressed=yes`), so an agent can select it
or pin it elsewhere with `LABEL SET id at=x,y`, as the window's Label Layout
Report does (`McpServer.LabelLayoutNamesTheLabelsWithNoRoomSoAnAgentCanMoveThem`).

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

Nothing is written to disk until a `SAVE`. Opening a project can back it up or
migrate it in place (`docs/headless.md`), so point Claude at a copy of a
project that matters.

## How it is built

* `src/katana_app/session.cpp` - the session both front ends share: the
  Document, its `CommandInterpreter`, and the verbs above `katana_cad`
  (`CUSTOMISE`, the DXF and GIS `IMPORT`/`EXPORT`, `INFO <file>`, `COPC`).
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
* **No pictures.** A tool that plots the drawing with the window's headless
  `--plot` or `--screenshot` (`docs/headless.md`) and returns the image would
  let Claude check its own work by eye.
* **Only what the command line has.** Terrain surfaces, corridors, grading,
  sections and sheets exist in `katana_cad` but have no command-line verbs
  yet, so no tools either; each verb added to the interpreter or the session
  reaches Claude with no change here.
