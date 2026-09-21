# Katana — instructions for AI contributors

This file is loaded automatically at the start of every session. It is the
standing contract for how work is done here. `PLAN.MD` says *what* is being
built; this file says *how to work on it*.

**Read `PLAN.MD` sections 4 (Absolute Architectural Rules), 32 (Performance
Targets), 35 (Numerical Correctness) and 36 (Error Handling) before your first
change in a session.** Everything below assumes them.

---

## 1. The working loop

Every unit of work follows the same loop. Do not skip steps because a change
looks small — the steps are what keep the codebase able to absorb the next
change.

1. **Read before writing.** Find the existing abstraction. This codebase has
   one way to report failure (`Result<T>` / `Status`), one tolerance policy
   (`math::tolerance`), one undo mechanism (`Command`), one layering rule
   (`tools/check_layering.cmake`). A second one is a defect, not a shortcut.
2. **Write the test first where the behaviour is checkable.** See §3.
3. **Implement.**
4. **Build clean.** `-Werror` is on. A warning is a failure. Judge a build by
   ITS EXIT CODE, never by grepping its output for "error": a link failure
   prints no line that a source-path filter matches, and a filtered build once
   reported "built" over a missing binary.
5. **Run the whole suite**, not just your module: `ctest --test-dir build/debug -j 8`.
   The layering check is one of those tests.
6. **Update `PLAN.MD` and the affected document in `docs/`.** See §2. This is
   not optional and not a separate task to do later.
7. **Commit.** See §5.

## 2. Keeping the plan and documentation true — MANDATORY

The plan and the docs are part of the deliverable. A change that leaves them
stale has not been finished.

**Every time you complete a phase, a sub-feature, or anything that changes what
the software can do, in the same change:**

- **`PLAN.MD`** — update the status of the phase you touched. The vocabulary is:
  - `**STATUS: DELIVERED.**` followed by a short record of *what was actually
    built and where it lives*, replacing the original instructions.
  - `**STATUS: PARTIALLY DELIVERED.**` followed by what exists **and an explicit
    `OUTSTANDING:` paragraph** naming what does not. Never quietly narrow a
    phase to what you managed.
  - Untouched phases keep their original text.
  - Keep the roadmap table in §5 in step.
  - **Do not renumber sections.** Source comments cite them (`PLAN.MD §32`,
    `Rule 4`), and renumbering silently invalidates every one of those.
- **`docs/`** — one document per area (`docs/cad.md`, `docs/interop.md`, ...).
  Record the *decisions and their reasons*, not a restatement of the code.
  A performance claim goes in with its before/after numbers and the machine.
- **`README.md`** — the test count and the feature list.
- **When you make a non-obvious engineering decision, write down the
  alternative you rejected and why.** That is the single most valuable thing
  for whoever picks this up next, human or model. Prefer recording it next to
  the code it governs; record it in `docs/` when it spans modules.

If you discover the plan is **wrong** — it asks for something that turned out
to be a bad idea, or reality has moved — say so in the plan, with the reason.
Do not silently ignore it and do not silently obey it.

## 3. Testing — the cardinal rule

**Never change an expected value to match what the program produced.**

When a test fails, exactly one of these is true, and you must establish which
before touching anything:

- The implementation is wrong → fix the implementation.
- The test's expectation is wrong → fix the test, **and justify it from an
  authoritative source outside this program**: a standard, a published
  constant, an independent calculation, a hand-worked example. "The program
  says 3.7 so the expected value should be 3.7" is circular and is never a
  reason.
- A tolerance is genuinely too tight for the floating-point arithmetic
  involved → widen it, and state the error analysis that justifies the new
  number.

Never delete or disable a failing test to make a suite green.

Other standing rules:

- Test names are sentences that state the property, not the method
  (`StationsOffTheSurfaceReportNoElevationRatherThanZero`).
- Assert on **values worked out independently**, not on values captured from a
  previous run. Prefer inputs whose answers are exact in binary.
- A property test must be shown to reach the cases it claims to cover — count
  the interesting hits and assert the count is non-trivial. A generator that
  can never produce the case it protects is worse than no test, because it
  reads as coverage.
- When you fix a bug, **prove the regression test catches it**: remove the fix,
  watch the test fail, restore the fix. A test that passes either way is not a
  regression test.
- New public behaviour needs: the happy path, every documented failure, and the
  degenerate input (empty, zero, coincident, non-finite, enormous).

## 4. Performance

`PLAN.MD` §32 sets the targets; Rule 6 says profile before optimising. In
practice:

- **Measure before and after, and put both numbers in the commit message and
  the docs.** `benchmarks/bench_*.cpp` is where a measurement lives so it can
  be repeated.
- Measure in **Release**. A Debug timing is not evidence.
- An optimisation that changes results is a bug. Where an optimisation culls
  work (a spatial filter, a tile bin), add a test proving the culled run finds
  exactly what the exhaustive one did.
- Parallel code must produce **identical** output whatever the thread count —
  see `katana::core::TaskPool`. Assert it.

## 5. Commits — commit at every milestone

**Commit whenever a milestone is reached**, where a milestone is any of:

- a phase or sub-feature is complete and its tests pass;
- a bug is fixed with its regression test;
- a refactor lands with the suite still green;
- a measurable performance change, with its numbers.

Before committing, the whole suite must pass and the plan and docs must already
be updated in the same change. Do not batch a day of unrelated work into one
commit, and do not commit a half-finished feature to "save progress".

Message format — what changed and **why**, the evidence, and what is still open:

```
<area>: <what changed, imperative>

<why it was needed; the alternative rejected, if the choice was not obvious>

Evidence: <test counts, before/after measurements, external source consulted>
Outstanding: <what this deliberately does not do>
```

## 5.1 Hand over something runnable — MANDATORY

**Before you stop working, leave a Release build the user can run, and say how
to run it.** A session that ends with passing tests and no runnable application
has not delivered anything the user can check.

At the end of a work session, or whenever you are about to hand back:

1. Build Release and run the whole suite there, not only in Debug:
   ```sh
   cmake --build build/release
   ctest --test-dir build/release -j 8
   ```
   A Debug-only result is not evidence that the shipped configuration works.
2. Build the desktop application and confirm the binary is newer than the
   sources you changed. A stale executable that still launches is the easiest
   way to report success on work that did not build.
3. **Tell the user the exact command to launch it**, including the runtime PATH
   the MSYS2 toolchain needs, and sample data where that makes the feature
   visible:
   ```sh
   PATH="/c/msys64/ucrt64/bin:$PATH"      ./build/release/bin/katana.exe samples/site_plan
   ```
   Everything runnable is in `<build>/bin`: `katana.exe`, `katana_cli.exe`,
   and the test executables under `bin/tests`. For something that runs with
   NO toolchain on PATH - what a user would actually receive - make the bundle
   and run that instead:
   ```sh
   cmake --build build/release --target bundle
   ./build/release/dist/Katana/bin/katana.exe
   ```
4. **Say what to look at.** Name the menu item, the command or the sample file
   that exercises what you just built, and what correct looks like. "It builds"
   is not a test the user can perform.
5. **Do not launch the GUI through the background task runner.** A detached
   `katana.exe` reports exit 139 when its parent shell is reaped, which
   looks exactly like a segmentation fault and is not one. Measured: the same
   binary survives 150 s in the foreground and 30 minutes under gdb, and
   reports 139 every time it is launched detached. Hand the user the command
   instead; run it yourself only in the foreground under `timeout` when you
   need to check that it starts. ALWAYS under `timeout`: a GUI-subsystem
   program that cannot find a DLL or a Qt platform plugin does not exit, it
   opens a modal box and waits - which cost ten minutes the first time the
   bundle was tested with `QT_QPA_PLATFORM=offscreen` and no `qoffscreen.dll`.
6. Say plainly what is NOT yet visible in the GUI. A feature that exists only
   in the library and has no way to reach it from the application is finished
   work in the tests and unfinished work to the user; say which it is rather
   than letting the demo imply otherwise.

Never leave the application mid-refactor at the end of a session. If the work
is genuinely incomplete, finish to the nearest point where it builds, passes
and runs, and say what remains.

## 5.2 Do not stop until the plan is finished — MANDATORY

**Work continuously through `PLAN.MD` until every phase is delivered.** Do not
stop at a natural pause point, do not stop because a feature is complete, and
do not stop to ask whether to carry on. Finish a milestone, commit it, and
start the next one in the same session.

Take the next work item in this order:

1. The phase you are part-way through, to a point where it is DELIVERED rather
   than partially delivered.
2. A real defect found on the way — fix it if it is small and adjacent, record
   it in `PLAN.MD` otherwise.
3. The earliest phase in `PLAN.MD` §5's table that is not yet delivered, unless
   a measurement says a later one is worth more (record the measurement).

Stop only for these, and say which:

- **A decision that is genuinely the user's** — a destructive or irreversible
  action, an outward-facing one, or a fork in the plan where the options lead
  to materially different software. State the options and recommend one.
- **A hard block** — a missing dependency, a credential, a file only the user
  has. Say exactly what is needed.
- **The plan is complete.**

Running out of obvious next steps is not a stopping condition: §5 of `PLAN.MD`
lists the remaining phases and §6 of this file lists the standing improvement
work. Neither is ever empty.

Two things this does NOT license. It does not license committing a broken or
half-finished feature to keep moving — §5 still applies, and every commit lands
green. And it does not license silence: hand back a runnable build and an
honest account at each milestone (§5.1), then carry on.

## 6. Self-improvement — leave the codebase easier to work on

You are expected to improve the *conditions* for the next contributor, not only
to add features. On every session, actively look for and act on:

- **A second way of doing something that already has a first way.** Collapse it.
- **A comment that explains what the code does rather than why.** Replace it
  with the reason, or delete it.
- **A test that would pass if the code were wrong.** Strengthen it or say so.
- **A silent failure** — an ignored return, a swallowed error, a default that
  hides a mistake. `PLAN.MD` §36 forbids these.
- **A number with no source.** Every constant gets a comment naming where it
  comes from (a standard, a measurement, a stated policy).
- **Something you had to work out by reading three files.** Write it down where
  the second file would have told you.

When you find a real defect outside the task you were given: fix it if it is
small and adjacent, otherwise record it in `PLAN.MD` under the phase it belongs
to. Never leave it only in the conversation — the conversation is lost and the
repository is not.

**Report honestly.** If tests fail, say so and show the output. If you skipped
part of the scope, say which part and why. A green summary over a red build
destroys the only thing that makes the rest of this useful.

## 7. Architecture quick reference

Layers, lowest first. A layer may only include headers from itself and the
layers below it, as declared in `tools/check_layering.cmake` and enforced by
the `layering` test:

```
core -> math -> geometry -> {terrain, render, entity} -> commands -> storage -> cad -> app -> qt
                            geodesy, survey (beside geometry)
                            gis, pointcloud (GDAL/PDAL) -> interop
```

- **`cad` may not see `interop`.** Keeping GDAL and PDAL out of the core
  application is what lets it build with `-DKATANA_BUILD_IO=OFF`, which the
  sanitizer job needs. Reference data is owned by `app`/`qt`.
- **`render` may not see `entity`.** The renderer consumes a `DrawList` and has
  no idea a Document exists — Rule 3 made structural.
- **No third-party type in any public header** (Rule 4). GDAL, PDAL, CGAL,
  PROJ, SQLite and Qt stay behind `src/`.

Build:

```sh
cmake -S . -B build/debug -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build build/debug
ctest --test-dir build/debug -j 8
```

`build/debug`, `build/rel` and `build/release` are the primary build
directories. C++23, GCC 16.2 (MSYS2 UCRT64) on Windows - the toolchain is a
rolling MSYS2 install, so check `g++ --version` before quoting it in a
measurement; it moved from 15.2 to 16.2 on 2026-09-21.

## 8. Style

- Follow the file you are editing: its comment density, naming and idiom.
- Comments say **why**. The code already says what.
- `Status` success is `return {};`.
- Entity ids are monotonic and never reused.
- Layer names are `/`-separated paths (`design/surface/tin1`); see
  `include/katana/entity/layer_path.hpp` for why the tree is derived from the
  names rather than stored.
- British spelling in prose; American in identifiers where the surrounding code
  already uses it (`color`, `normalize`).
