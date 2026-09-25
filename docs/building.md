# Building and running

How to configure, build and run Katana, what each build option does, and why
the build tree and the bundle are laid out as they are. How to run the tests
is `docs/testing.md`.

## Toolchain

C++26 (`KATANA_CXX_STANDARD`, default 26; 23 also builds), CMake 3.24 or later,
Ninja, and the libraries Qt 6 (Widgets), Eigen, PROJ, CGAL, SQLite,
nlohmann-json, GDAL and PDAL. On Windows every one of them comes from MSYS2
UCRT64 (`C:/msys64/ucrt64`), with GCC 16.2. MSYS2 is a rolling toolchain - it
moved from GCC 15.2 to 16.2 on 2026-09-21 - so check `g++ --version` before
quoting it in a measurement. Python 3 is optional and used only at build time
(Rule 1): without it the embedded customisation table is empty
("The customisation folder", below).

GoogleTest and Google Benchmark are fetched once and cached under
`third_party/_cache` (`cmake/KatanaThirdParty.cmake`), so later build
directories configure offline.

The programs need the MSYS2 runtime on `PATH` only while the build tree is
not self-contained (`-DKATANA_DEPLOY_RUNTIME=OFF`); every command below
assumes

```sh
export PATH="/c/msys64/ucrt64/bin:$PATH"
```

## Configure, build, test

With the presets (`CMakePresets.json`), each into `build/<preset>`:

```sh
cmake --preset debug            # configure
cmake --build --preset debug    # build
ctest --preset debug            # every test, output shown on failure
```

| Preset | Build type | Differs from the defaults by |
|---|---|---|
| `debug` | Debug | - |
| `release` | Release | - |
| `relwithdebinfo` | RelWithDebInfo | - |
| `sanitize` | Debug | `KATANA_ENABLE_SANITIZERS=ON`, `KATANA_BUILD_QT_APP=OFF`, `KATANA_BUILD_BENCHMARKS=OFF` |
| `tidy` | Debug | `KATANA_ENABLE_CLANG_TIDY=ON`, `KATANA_ENABLE_CPPCHECK=ON` |
| `linux-debug` | Debug | the Linux toolchain prefix instead of MSYS2 ("Linux", below) |
| `linux-release` | Release | as `linux-debug` |
| `linux-relwithdebinfo` | RelWithDebInfo | as `linux-debug` |
| `linux-sanitize` | Debug | as `linux-debug`, with `KATANA_ENABLE_SANITIZERS=ON` |

The Windows presets set the compiler to `C:/msys64/ucrt64/bin/g++.exe` and
`CMAKE_PREFIX_PATH` to `C:/msys64/ucrt64`; the Linux ones name the toolchain
file `cmake/toolchains/katana-linux.cmake`. Every preset uses Ninja and exports
`compile_commands.json`. Each has a build and a test preset of the same name
(the test preset for `tidy` excepted).

Without presets, as a worktree does it:

```sh
cmake -S . -B build/wt -G Ninja -DCMAKE_BUILD_TYPE=Debug \
      -DKATANA_DEPLOY_RUNTIME=OFF -DKATANA_BUILD_BENCHMARKS=OFF
cmake --build build/wt -j 3
ctest --test-dir build/wt -j 8
```

With no `CMAKE_BUILD_TYPE` the root `CMakeLists.txt` chooses RelWithDebInfo.
Judge a build by its exit code, never by filtering its output ("Working
rules" in `docs/architecture.md`).

## Linux

Katana builds, and its whole suite passes, on Linux x86-64. What makes that
take one command is a toolchain of its own, because distributions lag the
compiler this code needs: Ubuntu 24.04 ships GCC 14 (no `#embed`), CGAL 5.6
(no `Constraint_id::index`) and PROJ 9.4, and each fails the build somewhere.

```sh
python3 tools/setup_linux_toolchain.py      # once: into /opt/katana-toolchain, or $KATANA_TOOLCHAIN
cmake --preset linux-release
cmake --build --preset linux-release
ctest --preset linux-release -j 4
```

**The toolchain** (`tools/setup_linux_toolchain.py`) is one prefix of
conda-forge packages: GCC 16.2 (the version MSYS2 gives the Windows build),
CMake, Ninja, clang-format, Qt 6, CGAL, Eigen, PROJ, GDAL, PDAL, SQLite,
nlohmann-json, GoogleTest and Google Benchmark. It is solved and installed by
py-rattler, from PyPI, so it needs neither conda nor root beyond write access
to the prefix, and it reaches only `conda.anaconda.org` and `pypi.org`. The
solve is frozen at a date (`SNAPSHOT` in the script), so every machine gets
the same versions until that date is moved; a prefix whose stamp matches is
left alone, so running it again takes a second. On 2026-09-26 the snapshot
resolved to GCC 16.2.0, Qt 6.11.2, CGAL 6.1.1, PROJ 9.8.1, GDAL 3.13.2,
PDAL 2.10.1, Eigen 3.4.0, GoogleTest 1.18.0 and Google Benchmark 1.9.5.

**The toolchain file** `cmake/toolchains/katana-linux.cmake`, which the
`linux-*` presets name, points CMake at that prefix: its compilers, its
packages first on `CMAKE_PREFIX_PATH`, and its `lib/` as the programs'
run-time path, so they load its libstdc++, Qt, GDAL and PROJ without an
`LD_LIBRARY_PATH`. A missing prefix fails the configure, saying how to make
it, rather than falling back to the system compiler, which configures cleanly
and then fails in half a dozen files. Without a preset:
`-DCMAKE_TOOLCHAIN_FILE=cmake/toolchains/katana-linux.cmake`.

**GoogleTest and Google Benchmark** come from the prefix too: the toolchain
file sets `KATANA_FIND_TEST_FRAMEWORKS`, which makes
`cmake/KatanaThirdParty.cmake` find them with `find_package` instead of
downloading them, so a Linux configure needs no network. The Windows build
leaves it off and keeps its pinned downloads.

**PROJ's data.** In a relocated prefix PROJ's compiled-in search path is
rewritten at install time, and with PDAL loaded - in the application and in
every `katana_io` test - PROJ then opened its data directory as `proj.db` and
as each grid: GDAL identified no coordinate system, and PROJ called a grid
that is not installed available, so a transformation chose it and failed.
Katana therefore looks for PROJ's data beside the loaded `libproj`
(`include/katana/core/library_data.hpp`), as it already looks for GDAL's
beside the GDAL DLL on Windows, and hands it to each PROJ user's own search
path: its own contexts, and GDAL's (so PDAL's). An explicit `PROJ_DATA` still
wins, and a distribution's PROJ, whose compiled-in path is right, is left to
it.

What is Windows-only: `KATANA_DEPLOY_RUNTIME` and the `bundle` and `package`
targets (the Linux build tree runs from its run-time path), and the
customisation compiled in from `resources/customisation/`, which is
third-party material kept out of the repository - the tests that need it
skip without it, on every platform.

**Claude Code cloud sessions** run `.claude/hooks/session-start.sh` when they
start: it runs the setup script into `/opt/katana-toolchain`, puts the
prefix's `cmake`, `ctest`, `ninja` and `clang-format` on `PATH`, configures
`build/linux-release`, and tells the session how to build and test. The
container is kept after the hook, so the toolchain is downloaded once and
every later session finds it current. The hook does nothing in a local
session.

## Options

Every `KATANA_*` cache variable, with its default:

| Option | Default | Effect |
|---|---|---|
| `KATANA_CXX_STANDARD` | `26` | the C++ standard, 23 or 26; a variable so that a claim about the standard can be measured (`docs/performance.md`) |
| `KATANA_BUILD_TESTS` | `ON` | the test suites, the headless checks and the `layering` and `docs` tests |
| `KATANA_BUILD_BENCHMARKS` | `ON` | `katana_benchmarks` and `run-benchmarks` |
| `KATANA_BUILD_QT_APP` | `ON` | the desktop application `katana` and `katana_qt_widget_tests` |
| `KATANA_BUILD_IO` | `ON` | `katana_io` and `katana_interop`, the GDAL and PDAL modules; OFF also needs `KATANA_BUILD_QT_APP=OFF`, since the application links interop, and gives a `katana_cli` without IMPORT and EXPORT |
| `KATANA_WARNINGS_AS_ERRORS` | `ON` | `-Werror` |
| `KATANA_ENABLE_SANITIZERS` | `OFF` | ASan and UBSan where the toolchain has them; on MinGW, which ships no libsanitizer, UBSan in trap mode, so undefined behaviour aborts the test that caused it |
| `KATANA_ENABLE_CLANG_TIDY` | `OFF` | clang-tidy during compilation |
| `KATANA_ENABLE_CPPCHECK` | `OFF` | cppcheck during compilation |
| `KATANA_MODULE_FILTER` | empty | configure only the listed modules and their suites, e.g. `"katana_core;katana_math;katana_geometry"`; the configure fails, naming the module, when a listed module needs one that is not listed |
| `KATANA_DEPLOY_RUNTIME` | `ON` | copy the runtime DLLs and the GDAL and PROJ data beside the programs ("The build tree runs on its own too") |
| `KATANA_CUSTOMISATION_DIR` | `resources/customisation` | the folder whose customisation is compiled in ("The customisation folder") |

`KATANA_MODULE_FILTER` makes a small build quick: the numerics and survey
modules alone are a few minutes, the whole application most of an hour of
CPU. The filter check exists because CMake turns a link against a target that
does not exist into a bare `-lname`: a stale filter once configured cleanly and
failed at link time with no explanation.

## Targets

| Target | In `all` | What it builds or does |
|---|---|---|
| `katana` | yes | the desktop application (`src/katana_qt`) |
| `katana_cli` | yes | the command-line front end (`src/katana_app`) |
| `katana_mcp` | yes | the same session served to Claude over the Model Context Protocol (`docs/mcp.md`) |
| `katana_<module>_tests` | yes | one test executable per module (`katana_add_test_suite`, `docs/testing.md`) |
| `katana_qt_widget_tests` | yes | the widget tests, run offscreen |
| `katana_benchmarks` | yes | every `benchmarks/bench_*.cpp`, globbed |
| `katana_runtime` | yes | deploys the runtime beside the programs (Windows, `KATANA_DEPLOY_RUNTIME`) |
| `katana_12da_probe` | yes | what an archive file holds and what Katana would take from it (`src/katana_archive12d/probe_main.cpp`) |
| `run-benchmarks` | no | runs `katana_benchmarks` with `--benchmark_min_time=0.2s` |
| `format`, `format-check` | no | clang-format over the first-party sources; `format-check` fails if a file would change |
| `katana_make_icons` | no | writes `resources/katana.ico`, a 256 px PNG and `resources/icon_sheet.png` from the icon painters; the output is committed (`docs/desktop.md`) |
| `katana_tool_icon_sheet` | no | every catalogue tool's icon with its name and aliases (`docs/tools.md`) |
| `bundle` | no | a self-contained `<build>/dist/Katana` ("Bundling") |
| `package` | no | that tree as `Katana-<version>-win64.zip`, and an NSIS installer when `makensis` is found |

### Where things are built

```
<build>/bin/katana.exe          the application (target `katana`)
<build>/bin/katana_cli.exe      the command-line front end
<build>/bin/tests/              one test executable per module
<build>/bin/benchmarks/         katana_benchmarks.exe
<build>/lib/                    static libraries
<build>/share/                  proj and gdal data, when the runtime is deployed
```

CMake's default mirrors the source tree inside the build tree, which had put
the application at `build/release/src/katana_qt/katana_qt_app.exe` - a path
that reads as though the executable were in the sources. The root
`CMakeLists.txt` sets the output directories before any `add_subdirectory`,
because those variables initialise each target's property when the target is
created. Tests and benchmarks go one level down so that `bin` holds only what
ships. The directory `src/katana_qt` keeps its name: it names a LAYER, which
is a statement about dependencies, not about what the user double-clicks.

`build/debug`, `build/release` and `build/relwithdebinfo` are what the presets
make; any other name works. A worktree keeps its build trees inside itself
(`build/wt`, `build/wtr`).

## Running

### The desktop application

```sh
./build/release/bin/katana.exe [project-dir] [file-to-import ...] [switches]
```

Each argument that is a directory is opened as a project; any other is
imported (a GIS file, a point cloud, an archive), in the order given. The
switches (`--screenshot`, `--plot`, `--dialog` and the rest) run it without a
display for review and tests, and are `docs/headless.md`. A copy of
`samples/site_plan` is a project to open; `samples/site_plan.kcs` is the
script that draws it and `samples/gis` holds small GIS files.

Opening a project may migrate or back it up IN PLACE, so open a copy of any
project that must not change - a sample in the repository above all.

Run it in the foreground. A GUI program started as a detached background job
reports exit 139 when its parent shell is reaped, which looks exactly like a
segmentation fault and is not one. When a script must check that it starts,
run it headless under a timeout: a GUI-subsystem program that cannot find a
DLL or a Qt platform plugin does not exit, it opens a modal box and waits.

```sh
QT_QPA_PLATFORM=offscreen timeout 120 ./build/release/bin/katana.exe \
    samples-copy --screenshot out.png
```

### The command line

`katana_cli` is the same engine with no GUI:

```sh
katana_cli                                   # interactive; HELP lists commands, QUIT leaves
katana_cli drawing.kcs                       # run a script; '#' starts a comment
katana_cli -c "RECT 0,0 30,20" -c "SAVE site.katana"
katana_cli --help                            # the interpreter's verbs and the CLI's own
```

A script or a `-c` batch stops at the first failing command and exits 1, so it
composes with shell pipelines; a bad argument exits 2.

Both front ends run the one `CommandInterpreter` (`docs/cad.md`, "Command
interpreter"), and each adds verbs the engine cannot have, because `katana_cad`
may not see GDAL, PDAL or the archive readers:

| Verb | `katana_cli` | the window's command line |
|---|---|---|
| the interpreter's verbs (`HELP`) | yes | yes |
| `IMPORT`, `EXPORT`, `REFS`, `INFO <file>` (the interpreter's `INFO id` describes an entity) | yes (with `KATANA_BUILD_IO`) | yes |
| `COPC` | yes | no |
| `CUSTOMISE [REPLACE] <file>...` | yes | yes |
| `CODE`, `CODE EXPLAIN`, `CODE CENSUS`, `MAPFILE LIST`, `MAPFILE CHECK` | yes | no: the Survey Code Manager's tabs |
| `ZOOM`, `GRID`, `SNAP`, `QUIT` | `QUIT` only | yes |
| a catalogue tool's alias alone (`L`, `TRIM`, `STRETCH`) | no: "unknown command" where the interpreter has no verb of that name | starts the tool (`docs/tools.md`) |

Points are absolute (`12.5,40`), relative (`@3,4`) or polar (`@5<30`).

## The customisation folder

The linestyles, symbols and survey codes Katana draws with by default are
compiled INTO the program. `tools/embed_customisation.py` reads the folder
`KATANA_CUSTOMISATION_DIR` (by default `resources/customisation/`) at build
time - the style libraries `linestyles.4d` and `symbols.4d`, then the survey
code files `survey_codes.mapfile` and `survey_codes_names.mapfile`, the second
read after the first, whose rules win a field both give - and generates a
source file in the build tree. Nothing is installed beside the program, and
nothing is read from that folder at run time.

The folder is git-ignored: its files are third-party material under their own
licence. A checkout without it builds an empty table - Katana then draws plain
lines - and the tests that read it skip or prove only that loading nothing does
no harm (`qt_customisation_headless`). Point `KATANA_CUSTOMISATION_DIR` at a
customisation kept elsewhere to build with it without moving it.
`docs/survey_coding.md` has what the files hold and how to swap them.

A new worktree has none of the git-ignored folders. Copy them in before
configuring, and never add them to git: `third_party/_cache` (without it the
configure tries to download GoogleTest), `resources/customisation`, and the
licensed reference files kept under `docs/` that the customisation was made
from.

## Bundling

`cmake --build <build> --target bundle` installs into `<build>/dist/Katana`;
`--target package` makes `Katana-<version>-win64.zip` (and an NSIS installer
when `makensis` is on the machine). The result runs with no MSYS2, Qt or GDAL
installed. Three things make it so, and each was learned by the bundle
failing:

* **`windeployqt` for Qt, then a dependency scan over everything it deployed.**
  A scan of `katana.exe` alone never sees `qwindows.dll`, which Qt loads by
  name at start-up; and a scan that stops at the executables misses what only
  a PLUGIN needs (`qjpeg.dll` wants libjpeg). So the scan runs after
  `windeployqt`, over the executables and the plugins.
* **Conflicts are collected, not fatal.** `katana.exe` finds `Qt6Core.dll`
  beside itself while `bin/platforms/qwindows.dll` finds the toolchain's copy -
  one DLL, two paths, which `file(GET_RUNTIME_DEPENDENCIES)` reports as an
  error unless given `CONFLICTING_DEPENDENCIES_PREFIX`.
* **`qoffscreen.dll` is copied by hand.** `windeployqt` ships only the desktop
  platform plugin, but `katana --plot` is a headless feature. Without the
  offscreen plugin Qt does not exit with an error: it opens a modal "no Qt
  platform plugin" box and waits for a click, which is how the bundle's own
  smoke test came to hang for ten minutes.

`bin/` sits beside `share/` because that is where both data-hungry libraries
look: PROJ finds `proj.db` at `<its DLL>/../share/proj` by itself, and
`locateGdalData` (in `gdal_adapter.cpp`) does the same for GDAL. See
`docs/interop.md` for why GDAL needed telling.

The bundle is 388 MB. Almost all of it is the dependency chain of GDAL and PDAL
as MSYS2 builds them; Katana's own code is a few megabytes. The install rules
are `cmake/KatanaPackaging.cmake`; the licensed reference files under `docs/`
must never be installed.

**Known problem.** `cmake/KatanaPackaging.cmake` still installs, with no
condition, a file from the root of the repository that was removed on
2026-09-23 (the `install(FILES ...)` rule before the LICENSE rule), so
`cmake --install`, `bundle` and `package` fail at that step until the rule is
removed or made conditional. Nothing runs the bundle automatically, which is
why it went unnoticed.

## The build tree runs on its own too

Until 2026-09-23 only the bundle was self-contained: `<build>/bin/katana.exe`
found GDAL, PDAL, PROJ, Qt and the C++ runtime through `PATH`, so it ran only
where MSYS2 was installed and on `PATH` - the libraries were borrowed from the
toolchain at run time rather than being part of the program. Now the build
deploys them itself. The `katana_runtime` target (in `ALL`, after `katana` and
`katana_cli`) runs **the same `KatanaDeploy.cmake`** the bundle runs, with
`-DKATANA_DEPLOY_PREFIX=<build>`, so `<build>/bin` holds every DLL the programs
load and `<build>/share` holds `proj` and `gdal`. One script for both, so the
bundle and the build cannot disagree about what a self-contained Katana needs.
Switch it off with `-DKATANA_DEPLOY_RUNTIME=OFF`.

A full deploy is too slow for every build - measured on the Release tree, 36 s
on an idle machine and 62-85 s beside another build, most of it
`file(GET_RUNTIME_DEPENDENCIES)` running objdump over ~200 DLLs - so the build
run is incremental. It is skipped (0-1 s, measured) while three things hold:

* **A key is unchanged**: an MD5, taken at configure time, of every module's
  `LINK_LIBRARIES`, the toolchain and Qt plugin locations, and the deploy
  script's own text. Which DLLs are needed changes only when a link line does,
  and a link line changes only through CMake. The programs' import tables
  would be the direct answer, but `objdump -p` on the 143 MB Debug `katana.exe`
  takes 2.6 s - on every build.
* **Every DLL the last deploy recorded is still there.**
* **Every one is the toolchain's current copy**, by timestamp (`file(COPY)`
  keeps the source's). MSYS2 is a rolling toolchain; an update replaces
  `libstdc++-6.dll` under the same name, and a program relinked against the
  new one that finds the old one beside itself does not start.

Anything else is a full deploy, which first re-copies every DLL already in
`bin/` from the toolchain - the scan cannot, because it resolves a dependency
BESIDE the program first and so finds, and keeps, a stale copy - and writes its
record last, so a deploy that fails part-way is retried rather than trusted.
Verified on the Release tree: a back-dated `libgdal-39.dll` was noticed and
replaced with the toolchain's, a deleted `libproj-25.dll` was restored, and the
next run took 0 s. With `PATH` reduced to `C:\Windows\System32;C:\Windows`,
`build/release/bin/katana_cli.exe` imported the sample LAS (PDAL), ASCII grid
and GeoJSON (GDAL) and exported a DXF (GDAL's `header.dxf`, from
`build/release/share/gdal`), and `katana.exe --screenshot` built the window.

It is `katana_runtime` and not a POST_BUILD step on each program because
`katana` and `katana_cli` link in parallel under Ninja, and two deploys writing
one `bin/` at once would race.

**PDAL needs a home directory.** Run with an EMPTY environment (`env -i`),
PDAL refused every file: "No home directory found" - it reads `USERPROFILE`
(or `HOME`) for its plugin configuration. Every Windows session has
`USERPROFILE`, so this does not reach a user, but a service account or a
container that clears the environment would meet it.

## Why DLLs beside the program, and not static linking

Linking GDAL and PDAL INTO the executables was considered and is not done, for
reasons that are facts about the toolchain rather than preferences:

* **MSYS2 ships PDAL and PROJ as DLLs only.** `libgdal.a` exists; there is no
  `libpdalcpp.a` and no `libproj.a` (checked in `C:/msys64/ucrt64/lib`,
  2026-09-23).
* **Linking GDAL statically alone would be a defect.** `libpdalcpp-20.dll`
  imports `libgdal-39.dll` itself, so a Katana with GDAL linked in would load
  a SECOND GDAL through PDAL: two driver registries, two error states, two
  `GDAL_DATA` settings in one process, and a dataset opened by one unusable by
  the other.
* **A static build is a build of the whole stack from source**: GDAL, PROJ
  (with `proj.db` embedded - PROJ 9.6 can), PDAL, GEOS, SQLite, libtiff,
  libgeotiff, curl and their dependencies, as static libraries, with a driver
  set trimmed to what Katana reads (which would also cut most of the 388 MB).
  PDAL's CMake is built around shared libraries and plugins, so it is the
  hard part.
* **Licences.** GEOS is LGPL-2.1: linking it statically obliges whoever
  distributes Katana to let a recipient relink it against a modified GEOS
  (object files, or the source); as a DLL it is replaceable as it stands. Qt
  is LGPL-3 for the same reason and stays a DLL either way.

So "part of the program" is delivered as: every library in `bin/` beside the
executable, found first by Windows' DLL search order, and never the
toolchain's at run time. A single statically linked executable remains
possible as a packaging project of its own.

## Continuous integration

There is none: `.github/` holds no workflow (audit BLD-01, OPEN). Everything
above is run by hand, and `ctest` is the whole gate.
