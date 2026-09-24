# Architecture

Cross-cutting decisions that every Katana module obeys. Module-specific
documents live beside this one.

## Purpose

Katana is a deterministic C++ engineering platform for CAD and surveying. The
engine must be able to perform every important engineering operation on its own:
the user interface, the storage format and (later) the AI layer are all
consumers of it, never participants in it.

## Layering

Dependencies run one way, lowest first:

```
core → math → geometry → { terrain, render, entity } → commands → storage → cad → app → qt
              geodesy, survey              beside geometry (they see core and math)
              surveyio                     survey + geometry; seen only by app and qt
              archive12d                   terrain + entity; seen by interop, app, qt
              gis, pointcloud → interop    the GDAL/PDAL adapters (katana_io) and import/export
```

`cad` may see neither `interop` nor `surveyio` nor `archive12d`: the
application core builds without GDAL and PDAL (`-DKATANA_BUILD_IO=OFF`), and no
instrument format's or archive reader's own types can reach the drawing. This diagram
used to show `io` and nothing above `cad` but `qt` and `app`; the exact lists
are in the layering check and nowhere else.

A module may include headers from the layers it is listed as depending on in
[`tools/check_layering.cmake`](../tools/check_layering.cmake), and from nowhere
else. Two rules are machine-checked there and run as the `layering` test:

1. **One-way dependencies.** `geometry` cannot include `survey`, `cad` or `qt`.
   The check reads every `#include "katana/<layer>/..."` and compares it against
   the declared allow-list for the file's own layer.
2. **No third-party types in public headers.** Eigen, CGAL, PROJ, SQLite, GDAL,
   PDAL, Qt, Vulkan and nlohmann may appear only in `src/`, never under
   `include/`. Each is wrapped by a Katana-owned interface — `Result<T>`-based,
   using Katana's own value types — so that replacing a library is a change to
   one module rather than to the whole codebase (PLAN.MD Rule 4).

The practical consequence: `katana_math` and `katana_geometry` have no
dependencies beyond the standard library, and their tests link nothing else.

## Error handling

There are no silent failures and no error codes that a caller can ignore by
accident. Every fallible operation returns
[`katana::core::Result<T>`](../include/katana/core/error.hpp) (or `Status`, its
void form), which holds either a value or an `Error` carrying:

* a typed `ErrorCode` — `InvalidGeometry`, `InvalidCRS`,
  `InvalidSurveyObservation`, `AdjustmentFailure`, `TriangulationFailure`,
  `DatabaseFailure`, `CommandRejected`, and so on;
* a human-readable message;
* a context string holding the offending input — the entity id, the file path,
  the layer name — so a failure can be diagnosed from a log alone.

`Result` is `[[nodiscard]]`. Exceptions are reserved for programmer error:
reading `value()` from a failed `Result` throws `BadResultAccess`, because that
is a bug in the caller, not a runtime condition. Third-party libraries that
throw (CGAL, nlohmann) or that use return codes (SQLite, PROJ) are converted to
`Result` at the module boundary; no third-party exception escapes a module.

## Numerical policy

Every tolerance in the codebase is a named constant in
[`numerics.hpp`](../include/katana/math/numerics.hpp), documented with its unit
and rationale. Ad-hoc epsilons are forbidden — they are how CAD kernels acquire
inconsistent behaviour between subsystems.

| Constant | Unit | Meaning |
|---|---|---|
| `kAbsolute` = 1e-12 | dimensionless | a normalised quantity (a sine, a unit determinant) is zero |
| `kRelative` = 1e-9 | dimensionless | general-purpose scaled equality |
| `kAngular` = 1e-10 | radians | directions are parallel |
| `kGeometric` = 1e-7 | model units | features coincide: duplicate vertices, point-on-curve, tangency |
| `kCoordinate` = 1e-4 | model units | survey coordinates are the same ground mark |

The scale that sets these is the projected coordinate: at a UTM northing of
1e7 m, one ulp of a `double` is about 1.9e-9 m, so no linear tolerance below
about 1e-8 can carry meaning. `kGeometric` sits about 50 ulp above that, leaving
room for a few chained operations to round.

Two further consequences run through the code:

* **Comparisons are explicit.** `operator==` on `Vec2`/`Vec3` is exact; fuzzy
  equality is `nearlyEqual(a, b, tolerance)`. Approximate equality is not
  transitive and must never hide inside an operator.
* **Large coordinates are handled deliberately.** Area, centroid and
  triangulation routines work relative to a local origin. A naive shoelace sum
  over UTM-magnitude coordinates forms products near 2.5e12 and loses roughly
  six significant digits of a 100 m² area; evaluating relative to the first
  vertex keeps full precision. Tests assert this by computing a figure locally
  and again shifted by (500000, 5000000).

## Determinism

Identical inputs and configuration must give identical results (PLAN.MD Rule 7).
This is a correctness requirement for an engineering tool, not a convenience.

* Containers that feed output are ordered (`std::map`, sorted vectors). No
  result depends on `unordered_map` iteration order or on pointer addresses.
* Compilation uses `-ffp-contract=off -fno-fast-math`, so the compiler may not
  fuse or reassociate floating-point operations. Results match across
  optimisation levels, which is why every suite is run in Release as well as
  Debug.
* Property tests use a fixed-seed generator
  ([`tests/support/property.hpp`](../tests/support/property.hpp)), so a failure
  reproduces exactly.
* Entity ids increase monotonically and are never reused, so a recorded id
  cannot come to mean a different entity after an undo.

## Logging

[`katana::core::Logger`](../include/katana/core/log.hpp) emits structured
records: timestamp, level, category, message and key/value fields, rendered as
`2026-09-19T10:15:30.123Z INFO [command] executed name=CREATE_LINE entities=1`.

A logger is an ordinary object passed to the subsystems that need one; there is
no global logger. `log()` is safe to call concurrently and serialises sink
invocation. Values containing whitespace or `=` are quoted, so records stay
parseable. Commands log their name, change count and execution time; the
convention is to log identifiers and counts rather than coordinate payloads, so
logs do not accumulate project data.

## Threading

The current engine is single-threaded by construction. The document, its model,
the command stack and the project store all belong to one thread, and that is
stated in each header rather than left implicit. The pieces designed to be
parallelised later — terrain tiles, spatial queries, import and export — are
written as independent units of work, so that Phase 19 can add a task system
without restructuring them. `Logger` is the one type that is explicitly
thread-safe today.

## Testing strategy

`ctest` runs unit, integration, property, regression and end-to-end tests plus
the layering check. The rules that matter:

* **Expected values are derived independently of the implementation** — from a
  closed form, a textbook identity, an exact synthetic construction, or a hand
  calculation recorded in a comment. A value copied from the code under test
  proves only that the code is self-consistent. This project has seen the
  failure mode first-hand: an early least-squares "solver" hardcoded its own
  test's expected answer and passed.
* **Edge cases are named, not assumed** — parallel and coincident lines,
  zero-length segments, tangent circles, nearly parallel geometry, duplicate
  points, degenerate polygons, empty containers, non-finite input.
* **Failures are diagnosed before they are fixed.** When a test disagrees with
  the implementation, the question is which one is wrong on the merits. A
  tolerance may only be loosened with a floating-point justification.

## Build

CMake 3.24+, one target per module, warnings as errors. `CMakePresets.json`
provides Debug, Release, RelWithDebInfo, sanitizer and static-analysis
configurations. Sanitizers use ASan+UBSan where available; on MinGW, where
libsanitizer is not shipped, UBSan runs in trap mode so undefined behaviour
aborts the offending test. GoogleTest and Google Benchmark are fetched once and
cached under `third_party/_cache`, so additional build directories configure
offline.

### Where things are built

```
<build>/bin/katana.exe          the application (target `katana`)
<build>/bin/katana_cli.exe      the command-line front end
<build>/bin/tests/              one test executable per module
<build>/bin/benchmarks/
<build>/lib/                    static libraries
```

CMake's default mirrors the source tree inside the build tree, which had put
the application at `build/release/src/katana_qt/katana_qt_app.exe` - a path
that reads as though the executable were in the sources. The root
`CMakeLists.txt` sets the output directories before any `add_subdirectory`,
because those variables initialise each target's property when the target is
created. Tests and benchmarks go one level down so that `bin` holds only what
ships. The directory `src/katana_qt` keeps its name: it names a LAYER, which
is a statement about dependencies, not about what the user double-clicks.

### Bundling

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
as MSYS2 builds them; Katana's own code is a few megabytes.

### The build tree runs on its own too

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

### Why DLLs beside the program, and not static linking

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
possible as a packaging project of its own - see PLAN.MD section 6.

## Failure modes

| Condition | Behaviour |
|---|---|
| Invalid geometry submitted | rejected by `validate()` before entering the model |
| Command fails mid-way | change set rolls back; model unchanged; command not added to history |
| Transaction step fails | earlier steps are undone in reverse order |
| Locked layer edited | `CommandRejected`, nothing modified |
| Project database corrupt | detected on open by integrity check; `recover()` restores the newest sound backup and keeps the damaged file |
| Project from a newer Katana | refused with `Unsupported` rather than guessed at |
| Third-party library throws | converted to a `Result` at the module boundary |
