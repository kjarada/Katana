# Katana - instructions for AI contributors

This file is loaded at the start of every session, and every subagent and
workflow agent gets it too. It is the standing contract for how work is done
here.

There is no plan document. The owner removed it on 2026-09-23 ("it
complicates the development cycle"), so do not bring it back. `docs/` is the
record: one document per area, indexed by `docs/index.md`. The root
`README.md` (added at the owner's request on 2026-09-26) is for people
installing Katana: what it is, the downloads, and how to install, verify and
run them on each platform. Keep it to that; development detail belongs in
`docs/`, and `docs/` does not cite it (`tools/check_docs.py`).

---

## 1. Every feature ships on all three surfaces - MANDATORY

Katana is agentic CAD. A person must be able to do everything in the window
with the mouse, and an AI agent must be able to do everything through a
command. **Every feature ships, in the same change, in:**

- **The desktop window (`katana`, `src/katana_qt`).**
  - A menu item in the right menu and section, plus a toolbar or context-menu
    entry where apt.
  - A dialog or panel with a stable `objectName` on every action and widget.
  - The dialog does not do the work itself. It builds the verb line and hands
    it to the window's command executor, exactly as if it had been typed.
    That makes it one code path, logged and undoable like a typed command.
    Models: `gis_online_dialog.cpp` (GIS > Online Data) and the Survey menu's
    utilities dialog.
- **`katana_cli`.** Prefer a verb in the shared `CommandInterpreter`
  (`katana_cad`). There, the window's command line, `katana_cli` and
  `katana_mcp` all get it at once; `plotting/sheet_verbs.cpp` is the model for
  a verb family. A verb that needs GDAL or PDAL lives in
  `src/katana_app/session.cpp`, and the window's command line must accept it
  too. Add `cli.*` tests in `src/katana_app/CMakeLists.txt`.
- **`katana_mcp`.** It runs session lines, so a verb reaches it. When an agent
  needs structured data, add a tool or resource in `src/katana_app/mcp_server.cpp`
  and `docs/mcp.md`, and test it in `tests/app/test_mcp_server.cpp`.

Underneath all three:

- **One door for edits.** The real operation is a headless, testable function
  or an undoable command in `katana_cad` / `katana_commands`, never code in a
  widget. One user edit is one undo step.
- **Machine-readable results.**
  - Replies are `key=value` records, one per line.
  - Failures are `core::Status` / `core::Result` with a specific message,
    never only a message box.
  - State has a plain-data, versioned JSON form where it is edited (as
    `SheetSet` does).
  - Problems go into a list an agent can read.
- **No hidden modality.** Every dialog has a non-interactive path, a headless
  run never blocks on a question, and generators are deterministic.
- **Tests and docs cover each surface.** A feature missing a surface is not
  done. At hand-off, say plainly which surface is missing, if any.

### 1.1 Every tool acts on a scope and a filter - MANDATORY

Any tool, verb or dialog that reads or changes drawing data must be able to
act on the same choices as Global Modify. That includes reports, checks,
labelling, coding, export, modify, measure, and drawing from data.

- **Scope, where it acts:**
  - the selection;
  - what a view shows (its visible area and its own hidden layers);
  - named layers, with or without their sublayers;
  - the whole drawing;
  - headless, an `AREA x0,y0,x1,y1` window.
- **Filter, "only those that match":** entity types, layer patterns, style,
  colour, property and value, text, drawn only.
- **One mechanism.**
  - `cad::ModifyScope` and `cad::ModifyFilter`, resolved by `cad::matchEntities`
    (`include/katana/cad/global_modify.hpp`).
  - The verb grammar
    `[SELECTION|VIEW|DRAWING|AREA x0,y0,x1,y1|LAYERS a,b [ONLY]] [WHERE k=v ...]`,
    read by the one shared parser (`include/katana/cad/scope_verbs.hpp`).
    `VIEW` is the window's active plan view, supplied to the interpreter by the
    window; headless it is refused in favour of `AREA`.
  - In the window, the same "Apply to" and "Only those that match" controls
    as the Global Modify dialog, from one shared widget
    (`src/katana_qt/customisation/scope_filter_widget.*`).
  - Never write a second scope or filter implementation. If a tool needs more,
    extend the shared one.
- **Files stay an alternative source, not the only one.** A tool that also
  reads files (a schedule, a CSV) keeps the file as one source beside the
  drawing.
- **Say what the scope took** ("12 matched ...") with the result, as Global
  Modify's preview does. A scope that takes nothing is reported, not
  treated as an error.

## 2. The working loop

1. **Read before writing.** Find the existing abstraction. There is one way to
   report failure (`Result<T>` / `Status`), one tolerance policy
   (`math::tolerance`), one undo mechanism (`Command`), and one layering rule
   (`tools/check_layering.cmake`, the `layering` test). A second one is a
   defect, not a shortcut.
2. **Write the test first** where the behaviour is checkable (see section 4).
3. **Implement.**
4. **Build clean.** `-Werror -Wshadow` are on, so a warning is a failure.
   Judge a build by its EXIT CODE, never by grepping its output.
5. **Run the whole suite**, not just your module.
6. **Update the affected `docs/*.md` in the same change.**
   - Record decisions and the reasons for them, not a restatement of the code.
   - When you reject an alternative, write down which one and why.
   - The `docs` test (`tools/check_docs.py`) must pass.
7. **Commit** (see section 6).

## 3. Building - MANDATORY, the owner's standing instructions

Build and test like this:

```sh
cmake --preset release                       # configure: build/release
cmake --build build/release --parallel       # ALWAYS --parallel
QT_QPA_PLATFORM=offscreen ctest --test-dir build/release --parallel 14 --output-on-failure
```

- **Always `--parallel`.**
- **Show the build.** Run cmake, ninja and ctest in the foreground, with
  their output reaching the terminal:
  - no `> log` redirection and no backgrounding;
  - no pipe through `grep`, `tail`, `head` or `Select-String` that hides the
    progress. If a copy is needed, use `tee`.
  - Before a long build, say in one line what is being built. Afterwards,
    state the exit code and the test counts.
- **Never pipe a build into PowerShell's `Select-Object -First N`.** It stops
  the pipeline and can kill ninja mid-write.
- **Never run two builds in one build directory at once.** A build that passes
  the tool's 10-minute limit is moved to the background and KEEPS RUNNING.
  Wait for its notification before starting another. Two ninjas in one
  directory corrupt `.ninja_deps`.
- **If a running Katana stops the build, kill it and build again.** This is
  the owner's standing instruction (2026-09-30), so do not ask first.
  - The sign is a link that fails with `cannot open output file
    bin\katana.exe: Permission denied`, or the same for `katana_cli.exe` or
    `katana_mcp.exe`. Windows locks a running program's file; it is usually
    the owner's window, opened from this `build/release`.
  - Stop only the programs started from the build directory being built,
    from the checkout or worktree root, then run the same build again:

    ```sh
    bin="$(cygpath -w "$PWD/build/release/bin")"
    powershell -NoProfile -Command "Get-Process katana,katana_cli,katana_mcp -ErrorAction SilentlyContinue | Where-Object { \$_.Path -like '$bin\*' } | Stop-Process -Force"
    ```

  - Never while that directory's own suite is running: its headless tests
    run `bin/katana.exe` too, and a build there is already forbidden above.
  - Unsaved work in the window is lost; the owner accepts that. Say in the
    hand-off that you closed it.
- **After editing CMake files, run `cmake --preset release` on its own first.**
  Don't let the build regenerate itself mid-run.
- **If ninja prints "premature end of file; recovering",** ninja's own recovery
  does not heal the log. Every later build then recompiles the same files
  again. Fix it with:
  - `ninja -C build/release -t recompact`, then build once;
  - afterwards `ninja -C build/release -t deps | grep -c "premature\|STALE"`
    must be 0.
- **The build copies the runtime into `build/release`** (`katana_runtime`,
  `cmake/KatanaDeploy.cmake.in`), so `bin/katana.exe` runs with no MSYS2 on
  PATH:
  - the DLLs;
  - `share/proj` and `share/gdal`;
  - `etc/ssl/certs/ca-bundle.crt`, the certificate bundle libcurl checks
    `https://` against. Without it every web request fails.
- Presets: `debug`, `release`, `relwithdebinfo`, `sanitize`, `tidy`, and
  `linux-debug` / `linux-sanitize` for Linux.
- Toolchain: MSYS2 UCRT64, GCC 16 (a rolling install; check `g++ --version`
  before quoting it), C++26, Qt 6, Ninja.

## 4. Testing - the cardinal rule

**Never change an expected value to match what the program produced.** When a
test fails, establish which of these is true before touching anything:

- The implementation is wrong: fix the implementation.
- The expectation is wrong: fix the test, **justified from a source outside
  this program**, such as a standard, a published constant, an independent
  calculation or a hand-worked example.
- A tolerance is too tight for the arithmetic: widen it, and state the error
  analysis behind the new number.

Never delete or disable a failing test to make a suite green.

- Test names are sentences stating the property
  (`StationsOffTheSurfaceReportNoElevationRatherThanZero`).
- Assert on values worked out independently, never on a previous run's output.
- A property test must be shown to reach the cases it claims to cover.
- When you fix a bug, prove the regression test catches it: remove the fix,
  watch it fail, restore the fix.
- New public behaviour needs the happy path, every documented failure, and
  the degenerate input (empty, zero, coincident, non-finite, enormous).
- Qt tests run with `QT_QPA_PLATFORM=offscreen`.
- The real window is driven headlessly through `tools/check_screenshot.cmake`
  (`-DDRIVE` steps; see `docs/headless.md`).
- Fonts differ between Windows and Linux, so assert ink against a baseline,
  never an absolute pixel count.
- Tests that reach the network run only with `KATANA_ONLINE_TESTS=1`.

## 5. Performance

- Profile before optimising. Measure before and after in **Release**, and put
  both numbers in the commit message and the docs. `benchmarks/bench_*.cpp`
  is where a measurement lives so it can be repeated.
- An optimisation that changes results is a bug. Where one culls work, add a
  test that the culled run finds exactly what the exhaustive one did.
- Parallel code produces **identical** output whatever the thread count
  (`katana::core::TaskPool`). Assert it.

## 6. Commits

**When to commit:** commit at every milestone, and only when the whole suite is
green and the docs are updated in the same change. A milestone is:

- a feature slice complete with its tests;
- a bug fixed with its regression test;
- a refactor with the suite still green;
- a measured performance change.

Do not commit a half-finished feature.

The message says what changed and why, with the evidence, and what is still
open:

```
<area>: <what changed>

<why it was needed; the alternative rejected, if the choice was not obvious>

Evidence: <test counts, before/after measurements, source consulted>
Outstanding: <what this deliberately does not do>
```

**Staying safe with the owner's edits:**

- **Check `git status` and `git log` before and after.** The owner edits in
  the IDE while work is in progress, and the IDE's Sync pushes and pulls
  `main` on its own.
- **`git diff --cached` every staged file before committing.** Never
  `git add -A` blindly.
- **Never push, merge a pull request on GitHub, or delete a remote branch**
  unless the owner asks in that conversation.
- **Python writes CRLF on Windows** unless the file is opened with
  `newline='\n'`. A bash heredoc can mangle backslashes, so write scripts
  with a file tool instead.

## 7. Hand over something runnable - MANDATORY

Before handing back:

1. Build Release and run the whole suite there.
2. Confirm `build/release/bin/katana.exe` is newer than the sources you
   changed. A stale executable still launches.
3. Tell the owner how to run it, with sample data where that makes the
   feature visible:
   - the window: `build/release/bin/katana.exe samples/site_plan`;
   - the command line: `build/release/bin/katana_cli.exe -c "..."`.
   - For a self-contained copy, run `cmake --build build/release --target bundle`,
     then `build/release/dist/Katana/bin/katana.exe`.
   - Samples are `samples/site_plan`, `samples/gis` and `samples/utilities`.
4. **Say what to look at:** the menu item, the command, and what correct looks
   like.
5. **Never launch the GUI detached or in the background.** A detached
   `katana.exe` reports exit 139 when its parent shell is reaped, which looks
   like a crash and is not one. Run it yourself only headless, under
   `timeout`, with `QT_QPA_PLATFORM=offscreen`: `--screenshot`, `--command`,
   `--dialog` and `--report` (`docs/headless.md`). Then read the PNG and check
   it.
6. **Say plainly what is not yet reachable** in the window, the CLI or MCP
   (section 1).

## 8. Parallel agents and worktrees

Use parallel agents when a task splits into parts whose file sets do not
overlap, each part can be judged on its own, and together they are more than
a few minutes of work. Read-only sweeps (audits, reviews) are the clearest
win.

- **Put worktrees under `C:/GitHubProjects/Katana-wt/<name>`**, created from
  the main checkout with `git worktree add -b <branch> ../Katana-wt/<name> main`.
  Never put them under `.claude/`, which got wiped.
  - The harness's `isolation: "worktree"` branches from `origin/main`, not
    local HEAD, so it misses unpushed commits.
  - Each worktree builds in its own `build/release`.
- **Give each agent a disjoint, named file set.** Any shared hotspot needs an
  exact, small edit: `main_window.cpp`, the Survey menu, CMake source lists.
  Test sources compiled into `tests/qt_widgets` and `benchmarks/qt` are
  listed explicitly there, and a missing one is a link error.
- **A new worktree lacks everything gitignored.** Copy `third_party/_cache` in
  before configuring if the configure tries to download GoogleTest. Copy the
  reference customisation in only when the work needs it, and never add those
  files to git.
- **Cap the parallelism** when several agents build at once (`-j 4` each), or
  build only the targets the part needs.
- **Ask agents for evidence**: what changed, which tests ran, the output.
  Then verify centrally, by building the merged result and running the WHOLE
  suite yourself.
- **Workflow agents inherit `/effort` live**; there is no need to relaunch
  them.

## 9. Names - MANDATORY

Katana is a general product, so these never appear in new identifiers, UI
text, docs, styles, codes, the plot frame or commit messages:

- "12d", "Transport for NSW" or "TfNSW".

Users bring their own logo. Use general words: "customisation", "library
linestyle", "survey code file (.mapfile)", "style library (.4d)", "title block
file".

These are kept on purpose:

- the `.12da` archive format name, the `katana_archive12d` module, persisted
  `12d.*` property keys and format tokens such as the xml12d namespace;
- the public **TfNSW Utility Schema**, which may be named in the subsurface
  utility tools (`docs/subsurface_utilities.md`, `samples/utilities/`, the
  `UTILITY CHECK` tests, `tools/utility_schema_domains.py`).

Third-party reference files live gitignored on the owner's machine and are
never installed or committed.

## 10. Honesty and leaving it better

**Report honestly.**

- If tests fail, say so and show the output.
- If part of the scope was skipped, say which part and why.
- A green summary over a red build destroys the only thing that makes the
  rest of this useful.

**Leave the codebase easier to work on.** In every session, look for these:

- a second way of doing something that already has one;
- a comment that says what rather than why;
- a test that would pass if the code were wrong;
- a silent failure;
- a number with no source (every constant names where it comes from);
- something you had to work out by reading three files.

When you find a defect outside the task, fix it if it is small and adjacent.
Otherwise record it in the area's doc under "Not done". Never leave it only in
the conversation.

## 11. Architecture quick reference

A layer may include only its own headers and those of the layers
`tools/check_layering.cmake` allows it. The `layering` test enforces this.

```
core -> math -> geometry -> {terrain, render, entity} -> commands -> storage -> cad -> app -> qt
        geodesy, survey (on core + math)          archive12d, dxf (beside commands, no GDAL)
        surveyio (survey + geometry; seen only by app and qt)
        gis, pointcloud (GDAL/PDAL) -> interop (seen by app and qt)
        gpu (under src/katana_qt, but with render's rule: never a Document)
```

- **`cad` may not see `interop` or `surveyio`.** Keeping GDAL and PDAL out of
  the core is what lets it build with `-DKATANA_BUILD_IO=OFF`.
- **`render` may not see `entity`.** The renderer consumes a `DrawList`.
- **No third-party type in a public header.** GDAL, PDAL, PROJ, SQLite, curl
  and Qt stay behind `src/`.
- **`katana_app` is the headless session** that `katana_cli` and `katana_mcp`
  share. The desktop window drives the same `CommandInterpreter`.
- **Text and units have one home each:** `core/text_encoding.hpp`,
  `core/text.hpp` (locale-independent; never `std::isspace` or `strtod` on
  file text) and `math/unit_ratio.hpp`. A real becomes text through
  `core::formatExactReal`, never `std::to_string`: libc++ still pads that to
  six decimals, so it passes on GCC and fails the clang builds
  (`tools/check_float_to_string.py` finds one).
- **Absent is not zero.** A missing height is `std::optional`, never a
  placeholder number.
- **The entity geometry variant is append-only.** Its index is the on-disk
  kind byte. A new kind goes at the end, with a storage schema bump.

## 12. Style

- Follow the file you are editing: its comment density, naming and idiom.
- Comments say **why**. The code already says what.
- `Status` success is `return {};`.
- Entity ids are monotonic and never reused.
- Layer names are `/`-separated paths (`design/surface/tin1`;
  `include/katana/entity/layer_path.hpp`).
- Use British spelling in prose. In identifiers, use American spelling where
  the surrounding code does (`color`, `normalize`).
