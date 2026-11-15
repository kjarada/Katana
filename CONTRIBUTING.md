# Contributing to Katana

Humans and AI agents are both welcome. This page is the README's
[Contribute](README.md#contribute) section as a checklist. `CLAUDE.md` is the
full contract, and where the two differ, `CLAUDE.md` wins. `docs/index.md` says
which document covers which part of the code.

Licence: GPL-3.0-or-later with additional terms about credit (`LICENSE`,
`ADDITIONAL_TERMS.md`; see the [README](README.md#licence) and `LICENSING.md`).
You contribute under it and keep your copyright.

## Your first hour

1. **Build once.** [README, Quick start](README.md#quick-start) has the
   toolchain and the commands: Katana is one Cargo workspace at the
   checkout's root, built with `cargo build --release` on Linux or in WSL,
   or natively on Windows in MSYS2 UCRT64 (`docs/building.md`, "Windows";
   that build passes; its test results are not yet in). To work on one crate,
   `cargo build -p <crate>` and `cargo test -p <crate>` build and test only
   it and what it stands on.
2. **Run something.** [README, Try it](README.md#try-it) runs a command line, a
   screenshot and the tests. `cargo test -p <crate> <name>` runs the crate's
   tests whose names match; `bash tools/build_visible.sh wsl test -R <name>`
   (without `wsl` on Windows) the test runner's.
3. **Pick a task.** [README, Where to start](README.md#where-to-start) lists
   five that were real on 2026-10-08, from one line of code to a large gap. To
   find more, read the "Not done" list at the end of the document for the area
   you care about (`docs/index.md` says which document that is). No survey
   knowledge is needed for the build, packaging, window usability, tests or
   documents.
4. **Change it the way the checklists below say**, and open a pull request
   against `main`. A small change can go straight to a pull request; for
   anything large, open an issue first so the design is agreed.

Questions go in an issue. Issues are enabled on the repository; Discussions are
not.

## Your checkout and git

- [ ] Clone once, and work from the clone's root: the folder that holds `.git`.
      Open that folder in your editor, build there, and start an AI session
      there (`CLAUDE.md`, "Start every session in the checkout root"). Do not
      wrap the clone in another folder that you open instead.

  ```sh
  git clone https://github.com/kjarada/Katana.git
  cd Katana
  git switch -c <topic> main        # one branch per change
  ```

- [ ] Keep the branch current with `git fetch origin`, then
      `git rebase origin/main`. Publish it with `git push -u origin <topic>`
      and open the pull request from there (agents push only when the owner
      asks, see "Commits").
- [ ] `git status` should list only what you changed. Everything local is
      ignored: `build/`, `target/`, `.vscode/`,
      `.claude/settings.local.json`, `.claude/worktrees/`, and the third-party
      reference folders under `docs/`, which must never be added.
- [ ] A second line of work at the same time gets a worktree, not a second
      clone: `git worktree add -b <branch> ../Katana-wt/<name> main`. It
      shares the history, has its own build folders, and is removed with
      `git worktree remove ../Katana-wt/<name>` once merged, then
      `git branch -d <branch>`. A worktree is a folder beside the checkout,
      never inside it.

## Before you start

- [ ] Read the document for the area you will touch (`docs/index.md`), and its
      "Not done" list if it has one. The lists are a backlog.
- [ ] For anything large, open an issue first so the design is agreed before
      the work is done.
- [ ] Do not add third-party reference data to the repository: style libraries,
      survey code files or schemas that came under someone else's licence. Test
      fixtures use invented names. One tracked file is an exception, the
      built-in customisation `resources/customisation/nsw.customisation.json`,
      kept and shipped on the owner's decision of 2026-10-08, who states the
      information in it is public; it is outside the project licence
      (README, [Licence](README.md#licence)). It is not a precedent.
- [ ] Do not use the names that `CLAUDE.md` section 9 lists (the "Names" rule)
      in new identifiers, UI text, docs or commit messages.

## Build

- [ ] The toolchain is in the README's [Quick start](README.md#quick-start)
      (`python3 tools/setup_linux_toolchain.py` on Linux and in WSL; MSYS2
      UCRT64's packages on Windows); the build and the test runner are
      `docs/building.md`. In short, from the checkout's root:

  ```sh
  cargo clippy --locked --workspace --all-targets --release -- -D warnings
  cargo build --locked --workspace --release
  ```

  The maintainer runs both, and the tests, through `tools/build_visible.sh`
  (`bash tools/build_visible.sh wsl`, or without `wsl` natively on
  Windows), in a window of its own.
- [ ] Judge a build by its exit code, never by filtering its output. Clippy
      runs with `-D warnings`, so a warning is a failure.
- [ ] Never run two builds in one target folder at once.

## Test

- [ ] Write the test first where the behaviour can be checked. Name a test as a
      sentence that states the property.
- [ ] Run the whole suite before you commit, not only your crate: every
      crate's tests and the test runner's process-level tests.

  ```sh
  cargo test --release                         # from the checkout's root
  bash tools/build_visible.sh wsl              # all of it, the maintainer's way (no `wsl` on Windows)
  ```

  `docs/testing.md` explains each kind of test and how to register one: a
  `#[test]` in the crate, or an `add_test` of the CMake test runner for a
  program run as a process (`tests/cli/`, `tests/CMakeLists.txt`).
- [ ] Never change an expected value to match what the program produced. When a
      test fails, either the code is wrong (fix it), or the expectation is
      wrong (fix it, justified from a source outside the program: a standard, a
      published constant, a hand calculation), or a tolerance is too tight
      (widen it and state the error analysis).
- [ ] Never delete or disable a failing test to make a run green.
- [ ] A bug fix comes with a regression test. Remove the fix, watch the test
      fail, put the fix back.
- [ ] New public behaviour needs the happy path, every documented failure, and
      the degenerate inputs: empty, zero, coincident, non-finite, enormous.
- [ ] The window's tests run headless, with no display. Fonts differ between
      platforms, so assert ink against a baseline, never an absolute pixel
      count.
- [ ] Tests that reach the network run only with `KATANA_ONLINE_TESTS=1`.
- [ ] Before you change anything for speed, measure in Release. Put the before
      and after numbers in the commit message and the docs, and commit the
      benchmark beside the code so it can be repeated. The Rust workspace has
      no benchmarks yet (`docs/performance.md`, "Not done"): a speed change
      is the place to add the first, with the method of `docs/testing.md`,
      "Benchmarks".

## The contract

- [ ] A feature ships in the same change on all three surfaces, or you say
      plainly which one is missing:
  - the window: a menu item, and a dialog with a stable name on every
    control that builds the verb line and hands it to the window's command
    executor, as if it had been typed;
  - `katana_cli`: a verb in the shared `CommandInterpreter`, with `cli.*` tests;
  - `katana_mcp`: it runs session lines, so a verb reaches it; add a tool in
    `crates/katana_app/src/mcp/` and `docs/mcp.md` when an agent needs
    structured data.
- [ ] Any tool that reads or changes drawing data takes the shared scope
      (selection, view, layers, drawing, area) and filter. Do not write a second
      scope parser; extend `katana_cad::global_modify::match_entities`. This is the rule: `MODIFY`,
      `CODE`, `LINEWORK` and the `UTILITY` verbs take it today, and older verbs
      such as `ERASE` and `SELECT` do not yet (`docs/cad.md`, "Scope and
      filter").
- [ ] Edits go through one door: an undoable command, never code in a widget.
      One user edit is one undo step.
- [ ] Replies are `key=value` records, one per line. Failures are
      `katana_core::Status` or `katana_core::Result` with a specific message.
- [ ] No hidden modality: every dialog has a non-interactive path, and a
      headless run never waits on a question.
- [ ] Respect the layering (`tools/check_layering.cmake`, held by
      `tools/check_rust_layering.py`) and keep third-party types out of a
      crate's public interface.

## Style

- [ ] Follow the file you are editing: its comment density, naming and idiom.
- [ ] Comments say why. The code already says what.
- [ ] British spelling in prose; in identifiers, the surrounding code's spelling.
- [ ] A real becomes text through `katana_core::text::format_exact_real`, never
      `to_string` (`clippy.toml` refuses it). File text is read through
      `katana_core::text`, which takes no locale.
- [ ] A missing value is an `Option`, never a placeholder number.

## Docs

- [ ] Update the affected `docs/*.md` in the same change: record decisions and
      the reasons, and name any alternative you rejected and why.
- [ ] `python tools/check_docs.py .` passes. It is the `docs` test, and it
      checks that what the documents cite exists.
- [ ] If you find a defect outside your task, fix it when it is small and
      adjacent. Otherwise record it under "Not done" in the area's document.

## Commits

- [ ] Commit at a milestone, with the whole suite green and the docs updated:
      a feature slice with its tests, a bug with its regression test, a
      refactor, or a measured speed change. Do not commit half a feature.
- [ ] The message says what changed and why, with the evidence and what is
      still open:

  ```
  <area>: <what changed>

  <why it was needed; the alternative rejected, if the choice was not obvious>

  Evidence: <test counts, before and after measurements, source consulted>
  Outstanding: <what this deliberately does not do>
  ```

- [ ] Stage files by path and read `git diff --cached` before you commit. Never
      `git add -A` blindly.
- [ ] Sign off every commit (`git commit -s`). The `Signed-off-by:` line is
      the Developer Certificate of Origin, version 1.1
      (<https://developercertificate.org/>): you wrote the change, or have the
      right to submit it under the project's licence. Never submit code copied
      from an incompatible licence, or files that belong to someone else.
- [ ] A commit made with an agent's help carries a `Co-Authored-By` trailer.
      The person who submits it still signs it off.
- [ ] Agents never push, merge a pull request or delete a remote branch unless
      the owner asks.

## Reviews

- [ ] Open a pull request against `main` and describe what changed, why, the
      evidence, and what is outstanding. The maintainer decides what merges.
- [ ] Expect a reader who tries to refute your change. The project's review
      method is a second reader told to refute each finding before it is
      reported (`docs/audit/2026-09-23-defects.md`).
- [ ] Working in parallel? Give each agent a disjoint, named set of files and
      its own git worktree in a folder beside the checkout, never under
      `.claude/`. Each worktree builds in its own folders. Merge, then
      run the whole suite on the merged result yourself.
- [ ] Report honestly. If a test fails or part of the work was skipped, say so
      and show the output.
