# Katana - instructions for AI contributors

This file is loaded at the start of every session, and every subagent and
workflow agent gets it too. It is the standing contract for how work is done
here.

There is no plan document. The owner removed it on 2026-09-23 ("it
complicates the development cycle"), so do not bring it back. `docs/` is the
record: one document per area, indexed by `docs/index.md`. The root
`README.md` (added at the owner's request on 2026-09-26, and made a
developer-facing front page at the owner's request on 2026-10-08) is the
project's front page: what Katana is, how to build and try it, how to
contribute, and the install and verify reference for each platform. Detailed
development notes belong in `docs/`, and `docs/` does not cite it
(`tools/check_docs.py`).

**Katana is Rust.** The owner had the whole product ported from C++ (decided
2026-11-05; the C++ removed on the owner's word on 2026-11-15: "remove c++ i
dont want to see it"). One Cargo workspace, at the checkout's root (`Cargo.toml`), holds a crate
per layer (`crates/katana_<layer>`) and the four programs: the window `katana`
(egui and wgpu), `katana_cli`, `katana_mcp` and `katana_updater`. The only
C++ left is three small shims inside the crates that GDAL's algorithm
framework and PDAL need (`crates/katana_gis/native`,
`crates/katana_pointcloud/native`); write no other. CMake remains only as
the runner of the process-level tests (`project(Katana LANGUAGES NONE)`; the
`cli.*` and headless window tests against the cargo-built programs).
`docs/rust.md` is the port's record.

**Start every session in the checkout root:** the folder that holds `.git`,
`Cargo.toml` and this file, and nothing above it. A session started in a folder
that merely contains the checkout is not in a git repository, does not load
this file or `.claude/settings.json` at the start, and puts its own
`.claude/` in that outer folder instead of this one. Where things live:

- **Tracked, shared:** `CLAUDE.md`, `.claude/settings.json`, `.claude/hooks/`.
- **Ignored, per developer:** `.claude/settings.local.json`, `CLAUDE.local.md`,
  `.claude/worktrees/`, `.vscode/`, `build/`, `target/`.
- **Outside the repository, by Claude Code's design:** the auto-memory and the
  session transcripts, in `%USERPROFILE%\.claude\projects\<folder>\` (on
  Windows). They belong to one person's account, are never committed, and are
  keyed on the git repository, so they follow the checkout root. Nothing a
  future session must know belongs only there: put it in this file or in
  `docs/`.

---

## 1. Every feature ships on all three surfaces - MANDATORY

Katana is agentic CAD. A person must be able to do everything in the window
with the mouse, and an AI agent must be able to do everything through a
command. **Every feature ships, in the same change, in:**

- **The desktop window (`katana`, `crates/katana_window`).**
  - A menu item in the right menu and section, plus a toolbar or context-menu
    entry where apt.
  - A dialog or panel with a stable control name on every action and control
    (the drive harness's names, which the headless runner and the tests use).
  - The dialog does not do the work itself. It builds the verb line and hands
    it to the window's command executor, exactly as if it had been typed.
    That makes it one code path, logged and undoable like a typed command.
    Models: `crates/katana_window/src/gis_online_dialog.rs` (GIS > Online
    Data) and the Survey menu's utilities dialog
    (`crates/katana_window/src/survey/utility_dialog.rs`).
- **`katana_cli`.** Prefer a verb in the shared command interpreter
  (`crates/katana_cad/src/command_interpreter.rs`). There, the window's
  command line, `katana_cli` and `katana_mcp` all get it at once;
  `crates/katana_cad/src/plotting/sheet_verbs.rs` is the model for a verb
  family. A verb that needs GDAL or PDAL lives in
  `crates/katana_app/src/session.rs`, and the window's command line must
  accept it too. Add `cli.*` tests in `tests/cli/CMakeLists.txt`.
- **`katana_mcp`.** It runs session lines, so a verb reaches it. When an agent
  needs structured data, add a tool or resource in
  `crates/katana_app/src/mcp/` and `docs/mcp.md`, and test it in
  `crates/katana_app/tests/mcp/`.

Underneath all three:

- **One door for edits.** The real operation is a headless, testable function
  or an undoable command in `katana_cad` / `katana_commands`, never code in a
  widget. One user edit is one undo step.
- **Machine-readable results.**
  - Replies are `key=value` records, one per line.
  - Failures are `katana_core::Status` / `katana_core::Result` with a specific
    message, never only a message box.
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
  - `ModifyScope` and `ModifyFilter`, resolved by `match_entities`
    (`crates/katana_cad/src/global_modify.rs`).
  - The verb grammar
    `[SELECTION|VIEW|DRAWING|AREA x0,y0,x1,y1|LAYERS a,b [ONLY]] [WHERE k=v ...]`,
    read by the one shared parser (`crates/katana_cad/src/scope_verbs.rs`).
    `VIEW` is the window's active plan view, supplied to the interpreter by the
    window; headless it is refused in favour of `AREA`.
  - In the window, the same "Apply to" and "Only those that match" controls
    as the Global Modify dialog, from one shared widget
    (`crates/katana_window/src/customisation/scope_filter_widget.rs`).
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
   (`katana_math`'s tolerances), one undo mechanism (the `Command` trait), and
   one layering rule (`tools/check_layering.cmake`'s table, the
   `rust_layering` test). A second one is a defect, not a shortcut.
2. **Write the test first** where the behaviour is checkable (see section 4).
3. **Implement.**
4. **Build clean.** Clippy runs with `-D warnings` and `RUSTFLAGS` with
   `-D warnings`, so a warning is a failure. Judge a build by its EXIT CODE,
   never by grepping its output.
5. **Run the whole suite**, not just your crate.
6. **Update the affected `docs/*.md` in the same change.**
   - Record decisions and the reasons for them, not a restatement of the code.
   - When you reject an alternative, write down which one and why.
   - The `docs` test (`tools/check_docs.py`) must pass.
7. **Commit** (see section 6).

## 3. Building - MANDATORY, the owner's standing instructions

**Rust only, in WSL and on Windows.** The owner's instructions (2026-11-15: "i
just want rust"; 2026-11-16: "build for windows so i can test", which lifted the
WSL-only rule of 2026-11-05). Two hosts build the same workspace:

- **WSL**, the distribution `Ubuntu-24.04`, with the toolchain prefix
  `/opt/katana-toolchain` (rustc, cargo, the GCC that builds the shims, PROJ,
  GDAL, PDAL; `tools/setup_linux_toolchain.py`); output in
  `~/katana-build/<checkout>/`.
- **Windows**, natively in MSYS2 UCRT64 (its rust, gcc, cmake, gdal, pdal,
  proj, sqlite3 and curl packages; `docs/building.md`); output in `target/`.
  The programs need UCRT64's DLLs beside them: `bash tools/stage_windows.sh`
  makes a runnable copy in `build/windows/Katana` (`--beside` puts the same
  runtime beside `target/release`).

A build and a test run are, in WSL, with `CARGO_TARGET_DIR` in
`~/katana-build/<checkout>/cargo` (on Windows the same commands, without the
`xvfb-run`, with `target/` and the `release` preset):

```sh
cargo clippy --locked --workspace --all-targets --release -- -D warnings
cargo build  --locked --workspace --release
cargo test   --locked --workspace --release
cmake --preset linux-release -B ~/katana-build/<checkout>/tests -DKATANA_PROGRAMS=$CARGO_TARGET_DIR/release
xvfb-run -a ctest --test-dir ~/katana-build/<checkout>/tests --parallel 14 --output-on-failure
```

They are run by `tools/build_visible.sh`, in a console window the owner
watches (below, "Show the build"); never type them into Claude's shell.

- **Build once, at the end - the owner's standing instruction (2026-10-13).** A
  build and a suite run take a long time, and a session that keeps running them
  spends its time waiting.
  - While code is being written - by you, or by several agents editing in
    parallel worktrees - do NOT run cargo or ctest. Write, read and review the
    code; check what can be checked by reading it.
  - The build and the whole-suite run happen ONCE, as the final step, after
    every agent has finished editing: one integration step builds everything,
    runs everything, and fixes what the merge or the first compile broke. Its
    totals are the evidence (section 6, section 7).
  - A task that was only asked to edit says "not built" in its report as a
    plain fact, not as a failure, and lists the tests it wrote for the
    integration step to run.
  - Not covered by this: the owner asking you to build, run a test or drive the
    program, and the hand-off build (section 7).
  - Everything else in this section says how to build when it is time to.
- **Subagents only write code - the owner's standing instruction (2026-11-19):
  "when developing i dont want the subagents to build the app only to code".**
  A subagent (an Agent the session starts, a workflow agent, a worktree
  agent) NEVER builds, tests or runs the program:
  - no `tools/build_visible.sh` in any form (no `wsl`, `cargo`, `test`), no
    `cargo`, `ctest`, `cmake`, and no `katana`, `katana_cli` or `katana_mcp`
    run, headless or not - and so no screenshots of its own;
  - it reads, writes and reviews code, tests and docs, checks by reading
    what can be checked so, and reports "not built" with the tests it wrote
    and what each should show;
  - the main session - the integrator - merges the agents' work and does
    the one build, the whole suite, the screenshots and the fixes the
    first compile needs (section 7's hand-off);
  - this holds for every task, the look rounds' screenshot audits included:
    an audit agent reads the integrator's screenshots, it does not make them;
  - a brief for a subagent says this in so many words, and never tells it to
    build, test or run anything.
- **Show the build, in its own window, every time - the owner's standing
  instruction (2026-11-03): "show the build process everytime on cmd line so
  i can see it including the test phase".** Claude's shell moves a command
  still running after 10 minutes to the background, and the IDE does not show
  the owner a command's output, so a build run there is one the owner never
  sees. Every build and test run - a one-test run too - goes through
  `tools/build_visible.sh`:
  - Start it from the checkout or worktree root. It opens the window and
    returns at once:
    - `bash tools/build_visible.sh wsl` - Clippy, the build, cargo test and the
      process tests (the whole suite); `bash tools/build_visible.sh` without
      `wsl` runs the same natively on Windows (logs in `build/windows/`);
    - `bash tools/build_visible.sh wsl cargo [cargo test arguments]` - Clippy,
      the build and cargo test, only the tests named if any (e.g.
      `wsl cargo -p katana_window --test workspace maximis`);
    - `bash tools/build_visible.sh wsl test [ctest arguments]` - the process
      tests only, e.g. `wsl test -R docs`.
  - Then wait with ONE Bash command run with `run_in_background`, which
    notifies you when the run ends:

    ```sh
    until [ -f build/wsl/visible_build.status ]; do sleep 15; done; cat build/wsl/visible_build.status
    ```

  - Judge the run by the `exit=` in the status file. Read
    `build/wsl/visible_build.log` - every line the window showed - for the
    failures and the test counts.
  - Show everything: no `> log` redirection, and no pipe through `grep`,
    `tail`, `head` or `Select-String` in front of the owner's view. The
    script's `tee` is the copy.
  - Before starting, say in one line what is being built. Afterwards, state
    the exit code and the test counts.
  - The window stays open after the run so the owner can read it. Once you
    have read its status file and log, close it - the owner's instruction
    (2026-11-05): "make sure to close any finished cmd". `bash
    tools/build_visible.sh close` closes this checkout's finished windows and
    never one whose run is still going.
  - Allowed in Claude's shell still: reading the log, and running a built
    program headless under `timeout` (section 7).
  - On Linux (a cloud session) there is no owner's desktop: run the commands
    above in the shell as they are.
- **Never run two builds in one target directory at once.**
  `tools/build_visible.sh` refuses a second run while one is going in the same
  checkout; wait for the status file before starting another.
- **Every crate is pinned in `Cargo.lock`**, at releases at least two
  weeks old, and every cargo run is `--locked`; a new crate is added with the
  owner's knowledge and its notice in `THIRD_PARTY_NOTICES.md`.

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
  (`stations_off_the_surface_report_no_elevation_rather_than_zero`).
- Assert on values worked out independently, never on a previous run's output.
- A property test must be shown to reach the cases it claims to cover.
- When you fix a bug, prove the regression test catches it: remove the fix,
  watch it fail, restore the fix.
- New public behaviour needs the happy path, every documented failure, and
  the degenerate input (empty, zero, coincident, non-finite, enormous).
- No test opens a window. The window's tests drive egui headless through the
  drive harness (`crates/katana_window/src/drive/`); the real program is
  driven headlessly through `tools/check_screenshot.cmake` (`-DDRIVE` steps;
  see `docs/headless.md`).
- Fonts differ between machines, so assert ink against a baseline, never an
  absolute pixel count.
- Tests that reach any host other than the loopback address run only with
  `KATANA_ONLINE_TESTS=1`. A test may start its own server on `127.0.0.1`
  (port 0), with proxies disabled.

## 5. Performance

- Profile before optimising. Measure before and after in **Release**, and put
  both numbers in the commit message and the docs. A measurement that must be
  repeatable lives as a benchmark beside its crate.
- An optimisation that changes results is a bug. Where one culls work, add a
  test that the culled run finds exactly what the exhaustive one did.
- Parallel code produces **identical** output whatever the thread count
  (`katana_core::task_pool`). Assert it.

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

1. Build Release and run the whole suite (`bash tools/build_visible.sh wsl`,
   and `bash tools/build_visible.sh` on Windows when the change can differ
   there).
2. Confirm the programs are newer than the sources you changed. A stale
   program still launches.
3. Tell the owner how to run it, with sample data where that makes the
   feature visible:
   - the window on Windows: `bash tools/stage_windows.sh`, then
     `build/windows/Katana/bin/katana.exe <copy of samples/site_plan>`;
   - in WSL, shown on the desktop through WSLg:
     `~/katana-build/<checkout>/cargo/release/katana <copy of samples/site_plan>`;
   - the command line: `katana_cli -c "..."` beside it.
   - Open a COPY of a sample: a session writes into the project it opens.
   - Samples are `samples/site_plan`, `samples/gis` and `samples/utilities`.
4. **Say what to look at:** the menu item, the command, and what correct looks
   like.
5. **Never launch the window detached in Claude's shell.** Show it to the
   owner in a console window of its own (`cmd.exe /c start ...`),
   which the owner closes. Run it yourself only headless, under `timeout`:
   `--screenshot`, `--command`, `--dialog` and `--report` (`docs/headless.md`).
   Then read the PNG and check it.
6. **Say plainly what is not yet reachable** in the window, the CLI or MCP
   (section 1).

## 8. Agents and worktrees

**Run at most four agents at a time - the owner's standing instruction
(2026-11-15): "spawn 4 agents max".** It was two from 2026-11-14 and one from
2026-11-12, after six agents running at once spent the account's limit in
minutes and all six of a wave stopped mid-file. So:

- At most FOUR agents are running at any moment, a read-only sweep included.
  Start another only after one of the four has reported (or been stopped).
- Work that splits into parts whose file sets do not overlap is still split,
  each part with its own worktree and brief: the parts are run four at a time,
  not all side by side.
- An agent that stopped part way is resumed from where it stopped, not
  started again.

What follows says how each agent's part is set up, whichever order they run
in.

- **Put worktrees beside the checkout, in `Katana-wt/<name>`**, created from
  the main checkout with `git worktree add -b <branch> ../Katana-wt/<name> <base>`.
  The path is relative to the checkout so that it holds on any machine, and
  the worktree folder is a sibling of the checkout, never inside it. Never put
  them under `.claude/`, which got wiped. When one is finished and merged,
  remove it with `git worktree remove ../Katana-wt/<name>` and then
  `git branch -d <branch>`; `git worktree prune` clears the record of one whose
  folder is already gone; `bash tools/build_visible.sh prune` then deletes the
  WSL build folders (about 3 GB each) of worktrees that are gone.
  - The harness's `isolation: "worktree"` branches from `origin/main`, not
    local HEAD, so it misses unpushed commits.
  - Each worktree builds in its own `~/katana-build/<worktree>/` in WSL. Its
    first build starts from a copy of the main checkout's cargo folder, so
    only Katana's own crates are compiled, not every third-party crate again
    (`seed_from_main_checkout` in `tools/build_visible.sh`); build the main
    checkout once before starting agents, and do not start one while a run
    is going there (the copy is skipped then, and the worktree compiles
    everything).
- **Give each agent a disjoint, named file set.** Any shared hotspot needs an
  exact, small edit: a crate's `lib.rs` (each agent adds its modules in a
  commented block of its own, merged with `git merge-file --union`), the main
  window's modules, `Cargo.toml` files (only the integrator adds a crate).
- **Agents do not build** (section 3, "Subagents only write code"): only the
  integrator builds a worktree, with `KATANA_BUILD_JOBS` capped when it runs
  more than one at once. Each worktree's run opens its own window, titled
  with the worktree's folder.
- **Agents read egui's and egui_dock's API from the fetched sources**
  (`\\wsl.localhost\Ubuntu-24.04\root\.cargo\registry\src\...`), never from
  memory: they change at every release.
- **Merge finished trees into main, then build once, on main - the owner's
  standing instruction (2026-11-21): "merge into main then compile once if
  anything need to change we change on main".** When the agents have
  reported, their branches are merged into the main checkout's working tree
  (`git merge --squash`, uncommitted), and the one build and suite run happen
  THERE. What the first compile breaks is fixed on main, and the commit is
  made when the suite is green (section 6). Do not build an integration
  worktree first: it compiles everything twice and the owner tests main's
  copy.
- **Ask agents for evidence**: what changed, the tests written and what each
  should show, what they could not check by reading. Then verify centrally,
  by building the merged result and running the WHOLE suite yourself.
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

- the `.12da` archive format name, the `katana_archive12d` crate, persisted
  `12d.*` property keys and format tokens such as the xml12d namespace;
- the public **TfNSW Utility Schema**, which may be named in the subsurface
  utility tools (`docs/subsurface_utilities.md`, `samples/utilities/`, the
  `UTILITY CHECK` tests, `tools/utility_schema_domains.py`).
- the **NSW customisation's A3 utility cross-section title frame**, carried
  exactly as the owner made it - its "Transport for New South Wales" heading,
  its "Transport / For NSW" logo text and its disclaimer naming TfNSW - on the
  owner's decision of 2026-11-19 ("i want the exact title frame to be
  available within the NSW customisation"). It belongs to the NSW built-in
  only: it is offered when the NSW customisation is the active one, and
  Katana Standard's frames stay unbranded. The rule above holds everywhere
  else - no other frame, style, text or doc takes those names, and the frame's
  code and tests use general words ("the customisation's title frame").

The original third-party reference files live gitignored on the owner's
machine and are never installed or committed. The one file derived from them
that is tracked is `resources/customisation/nsw.customisation.json`, the
converted NSW built-in, committed on the owner's decision of 2026-10-08. The
converter copied the libraries' notice into its `notice` member; the owner
removed that text from the committed file by hand (two rows of dashes are all
that is left of it), so the file is not what the converter wrote. On
2026-11-03 the owner stripped the notices from the reference copies too and
had the test that converted them removed: the NSW built-in is the committed
JSON file, and the legacy `.4d` files are no longer needed. Tracked
tests, docs and fixtures still use invented names only, and quote counts, never
the names or the notice, so the rule above holds for them. Why the file is
tracked, and what that does and does not settle: `docs/customisation.md`, "Why
the built-in is in the repository".

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

Each layer is a crate, `crates/katana_<layer>`, and may depend only on the
layers `tools/check_layering.cmake`'s table allows it. The `rust_layering`
test enforces this on the crates' `Cargo.toml` files.

```
core -> math -> geometry -> {terrain, render, entity} -> commands -> storage -> cad -> app -> window
        geodesy, survey (on core + math)          archive12d, dxf (beside commands, no GDAL)
        surveyio (survey + geometry; seen only by app and window)
        gis, pointcloud (GDAL/PDAL) -> interop (seen by app and window)
        gpu (katana_gpu, with render's rule: never a Document)
```

(The window crate is the table's `qt` row, its name from before the port.)

- **`cad` may not see `interop` or `surveyio`.** Keeping GDAL and PDAL out of
  the core is what lets it build without them.
- **`render` may not see `entity`.** The renderer consumes a `DrawList`.
- **No third-party type in a public API.** A third-party crate is used only in
  the crate that adapts it, and never re-exported (the `rust_layering` test
  checks it).
- **`katana_app` is the headless session** that `katana_cli` and `katana_mcp`
  share. The desktop window drives the same command interpreter.
- **Auto-update** is the `update` layer (core only, seen by `app` and the
  window) and a fourth program, `katana_updater`; see `docs/update.md`.
- **Text and units have one home each:** `katana_core::text_encoding`,
  `katana_core::text` (locale-independent) and `katana_math::unit_ratio`. A
  real becomes text through `katana_core::text::format_exact_real` and its
  siblings, never `to_string`: Clippy refuses `ToString::to_string`
  (`clippy.toml`), and an integer is formatted with `format!`.
- **Absent is not zero.** A missing height is an `Option`, never a placeholder
  number.
- **The entity geometry enum is append-only.** Its index is the on-disk kind
  byte. A new kind goes at the end, with a storage schema bump.

## 12. Style

- Follow the file you are editing: its comment density, naming and idiom.
- Comments say **why**. The code already says what.
- `Status` success is `Ok(())`.
- Entity ids are monotonic and never reused.
- Layer names are `/`-separated paths (`design/surface/tin1`;
  `crates/katana_entity/src/layer_path.rs`).
- Use British spelling in prose. In identifiers, use American spelling where
  the surrounding code does (`color`, `normalize`).
- `#![deny(unsafe_code)]` everywhere but the named FFI, OS-call and SIMD
  modules.

## 13. The port's rules that still hold

`docs/rust.md` records the port: the decisions, the waves, the toolchain, what
differs from the C++ that was, and "Not done". The work is on branch
`rust-port` (`../Katana-wt/rust-port`; agents in `../Katana-wt/rust-<slice>`)
and reaches `main` only at a cut-over, with the owner.

- **A test's expectation came from the C++'s test, copied, never
  recomputed.** The twins keep them; section 4 holds for every change.
- **A defect found goes under "Not done" in `docs/rust.md`** or the area's
  document, with what it would take.
