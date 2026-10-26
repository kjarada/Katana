# Building and running

How to configure, build and run Katana, what each build option does, and why
the build tree and the bundle are laid out as they are. How to run the tests
is `docs/testing.md`.

## Toolchain

C++26 (`KATANA_CXX_STANDARD`, default 26; 23 also builds), CMake 3.24 or later
(with one older than 3.28 the tests still configure, and say that the
program tests of other directories are not given their starting
customisation: `docs/headless.md`, "The customisation a run starts with"),
Ninja, and the libraries Qt 6 (Widgets), Eigen, PROJ, CGAL, SQLite,
nlohmann-json, GDAL and PDAL. On Windows every one of them comes from MSYS2
UCRT64 (`C:/msys64/ucrt64`), with GCC 16.2. MSYS2 is a rolling toolchain - it
moved from GCC 15.2 to 16.2 on 2026-09-21 - so check `g++ --version` before
quoting it in a measurement. Python 3 is optional and used only for checks and
developer tools (Rule 1): without it the docs test and the censuses are not
registered, and the build is the same - the built-in customisation is compiled
in with `#embed`, which needs none ("The built-in customisation and the
reference folder", below).

The update layer adds two libraries the toolchain already carries, OpenSSL's
libcrypto and zlib, and on Windows it needs them as STATIC libraries
(`libcrypto.a`, `libz.a`; MSYS2's `openssl` and `zlib` packages have both):
the configure stops, naming what it found, without them ("The update layer",
below). `katana_io` links libcurl by name as well, the one GDAL loads.

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
| `macos-debug` | Debug | the macOS toolchain prefix ("macOS", below) |
| `macos-release` | Release | as `macos-debug` |

The Windows presets set the compiler to `C:/msys64/ucrt64/bin/g++.exe` and
`CMAKE_PREFIX_PATH` to `C:/msys64/ucrt64`; the Linux ones name the toolchain
file `cmake/toolchains/katana-linux.cmake`, the macOS ones
`cmake/toolchains/katana-macos.cmake`. Every preset uses Ninja and exports
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
nlohmann-json, OpenSSL, zlib, GoogleTest and Google Benchmark. It is solved
and installed by py-rattler, from PyPI, so it needs neither conda nor root
beyond write access to the prefix, and it reaches only `conda.anaconda.org`
and `pypi.org`. The solve is frozen at a date (`SNAPSHOT` in the script), so
every machine gets the same versions until that date is moved; a prefix
whose stamp matches is left alone, so running it again takes a second. On
2026-09-26 the snapshot resolved to GCC 16.2.0, Qt 6.11.2, CGAL 6.1.1,
PROJ 9.8.1, GDAL 3.13.2, PDAL 2.10.1, Eigen 3.4.0, GoogleTest 1.18.0 and
Google Benchmark 1.9.5.

**OpenSSL and zlib are named**, since 2026-10-14, because the update layer
asks for both by name (`src/katana_update/CMakeLists.txt`: Ed25519 from
libcrypto, raw inflate from zlib; "The update layer", below) and until then
had them only as other packages' dependencies. The solve was read that day,
without installing anything, for the three platforms the script serves -
`linux-64`, `linux-aarch64` and `osx-arm64` - at the script's own `SNAPSHOT`,
once as the list stood and once with the two names added. Naming them added
no package and removed none: 280, 280 and 239 packages before and after,
with OpenSSL 3.6.4 and zlib 1.3.2 in each. What the names change is what
holds them there. OpenSSL was asked for by 34 packages in each solve, Qt,
GDAL and libcurl among them. `zlib` - the package with `zlib.h`, `libz.a`
and the `libz.so` a linker asks for - came through three alone: `geotiff`,
`libspatialite` and `prometheus-cpp`. What everything else that inflates
names is `libzlib` (58 packages on Linux, 63 on macOS), which is the
shared library under its versioned names and nothing else: two files, no
`zlib.h` and no `libz.so` (on macOS no `libz.dylib`). So a moved `SNAPSHOT` in which those
three had stopped naming `zlib` would have left a prefix without the
header, and the configure to whatever copy a distribution keeps under
`/usr`. The two names are part of the prefix's stamp, like every other, so
a prefix made before them is installed again by the next run.

**The toolchain file** `cmake/toolchains/katana-linux.cmake`, which the
`linux-*` presets name, points CMake at that prefix: its compilers, its
packages first on `CMAKE_PREFIX_PATH`, and its `lib/` as the programs'
run-time path, so they load its libstdc++, Qt, GDAL and PROJ without an
`LD_LIBRARY_PATH`; an INSTALLED program's run-time path is `$ORIGIN/../lib`
instead, where the install copies them ("Bundling"). The sysroot is pinned to
glibc 2.28, so what is built here runs on Debian 10, Ubuntu 20.04, RHEL 8 and
later (`docs/release.md`). A missing prefix fails the configure, saying how to make
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
wins when it works, and a distribution's PROJ, whose compiled-in path is
right, is left to it. **When it does not work** - a machine-wide `PROJ_LIB`
left by PostGIS, QGIS or OSGeo4W, naming a folder with no `proj.db` or one of
another PROJ's layout, which PROJ rejects - both fall back on the data Katana
carries beside its `libproj` (`bundledProjDataDirectory`): the geodesy contexts
retry on a fresh context (`ProjContext::create`) and GDAL is asked once at
start-up, only when the variable is set, whether it can read EPSG:4326
(`pointGdalAtProjData`). Before, such a variable made every CRS verb fail with
"not a coordinate system: give an EPSG code" for a valid code, and GDAL lose a
file's EPSG code without a word. If neither opens, the error names the
variable and its value and says to unset it, not to install PROJ
(`cli.proj_lib_naming_no_database_falls_back_on_the_proj_data_katana_carries`
and its `PROJ_DATA` twin, Windows).

**The GPU renderer** is built on Linux too, on Vulkan, its shaders baked at
build time by the prefix's `qsb` (`docs/gpu.md`). The 3D view uses it on the
`xcb` and `wayland` platforms. Its tests need a display and a Vulkan device:
ctest runs them under `xvfb-run` when it is installed, on Mesa's lavapipe
where there is no GPU (Debian and Ubuntu: `xvfb`, `mesa-vulkan-drivers`), and
they skip without them.

What is Windows-only: `KATANA_DEPLOY_RUNTIME` (the Linux build tree runs
from its run-time path). The `bundle` and `package` targets work on Linux
too, since 2026-09-26 ("Bundling"). The customisation compiled in from
`resources/customisation/` is a tracked file, so a build on any platform has
it; the tests that need it skip only where a build's path names no file.

## Windows on ARM64

MSYS2's CLANGARM64 environment has the same libraries for ARM64 Windows,
with clang and libc++ (it has no GCC for ARM64). The `release` preset works
with its compiler named on the command line, as the release workflow does:

```sh
cmake --preset release -DCMAKE_CXX_COMPILER=C:/msys64/clangarm64/bin/clang++.exe \
      -DCMAKE_PREFIX_PATH=C:/msys64/clangarm64
```

MSYS2 has no NSIS for ARM64, so `package` makes only the zip unless
`-DKATANA_MAKENSIS="C:/Program Files (x86)/NSIS/makensis.exe"` names NSIS's
own x86 build (`choco install nsis`), which Windows on ARM runs emulated and
which makes the same installer.

## Linux on ARM64

`tools/setup_linux_toolchain.py` run on an ARM64 machine installs
conda-forge's `linux-aarch64` GCC 16.2, sysroot and libraries, and
`cmake/toolchains/katana-linux.cmake` picks the `aarch64-conda-linux-gnu`
compilers by the host's processor; the `linux-*` presets are the same.

### Cross-building for Linux aarch64 (the NEON kernels)

There is no preset for it: it exists to run the NEON kernels' tests on an
x86-64 machine, under qemu (`docs/performance.md`, "SIMD: NEON on 64-bit
ARM"). The recipe used on 2026-09-26, with py-rattler as
`tools/setup_linux_toolchain.py` uses it and the same `SNAPSHOT`:

1. **The compiler**, which runs on x86-64: solve `gxx_linux-aarch64 16.2.*`,
   `gcc_linux-aarch64 16.2.*` and `sysroot_linux-aarch64 2.28.*` for the
   platforms `linux-64` and `noarch` (they are cross compilers, so they live
   in `linux-64`) into a prefix of their own, say `/opt/katana-cross`.
2. **The libraries**, for aarch64: solve the script's `COMMON_SPECS` without
   the tools, plus `libstdcxx 16.*`, `libgcc 16.*`, `sysroot_linux-aarch64
   2.28.*`, `libgl-devel` and `libvulkan-headers`, for `linux-aarch64` and
   `noarch`, with virtual packages given by hand (`__unix`, `__linux`,
   `__glibc` 2.28, `__archspec` aarch64 - `VirtualPackage.detect()` would
   describe the x86-64 host), into `/opt/katana-aarch64`, installed with
   `platform=Platform("linux-aarch64")`.
3. **qemu**: `apt-get install qemu-user`.
4. **A toolchain file** setting `CMAKE_SYSTEM_NAME Linux`,
   `CMAKE_SYSTEM_PROCESSOR aarch64`, the compilers
   `/opt/katana-cross/bin/aarch64-conda-linux-gnu-gcc` and `-g++`,
   `CMAKE_SYSROOT /opt/katana-cross/aarch64-conda-linux-gnu/sysroot`,
   `CMAKE_FIND_ROOT_PATH /opt/katana-aarch64` (programs NEVER, the rest
   ONLY), that prefix first in `CMAKE_PREFIX_PATH`, `-L` and
   `-Wl,-rpath-link` to its `lib/` in the exe and shared linker flags, its
   `lib/` as `CMAKE_BUILD_RPATH`, `KATANA_FIND_TEST_FRAMEWORKS ON`, and
   `CMAKE_CROSSCOMPILING_EMULATOR` `qemu-aarch64;-L;<the sysroot>`, which
   ctest and the listing of each suite's cases (`katana_discover_tests`,
   `cmake/KatanaTargetDefaults.cmake`) then run every test executable through.
5. `cmake -S . -B build/aarch64 -G Ninja -DCMAKE_BUILD_TYPE=Release
   -DCMAKE_TOOLCHAIN_FILE=<that file> -DKATANA_BUILD_QT_APP=OFF
   -DKATANA_BUILD_IO=OFF -DKATANA_BUILD_BENCHMARKS=OFF`, build, and
   `ctest --test-dir build/aarch64 -R "^simd_"`.

The `cli.*` tests fail there: they start `katana_cli` through
`cmake -E env`, which the emulator does not wrap. Qt and GDAL were not
cross-built; the aarch64 Qt is in the prefix, but `katana_gpu` would also
need the host's `qsb` (`QT_HOST_PATH`), which was not tried.

## macOS

Katana builds on macOS on Apple silicon with the same script and the same
libraries, from conda-forge's `osx-arm64` channel, with clang 23 and libc++
in place of GCC (Intel Macs are not a target; `docs/release.md`, "macOS"):

```sh
python3 tools/setup_linux_toolchain.py      # once: into /opt/katana-toolchain, or $KATANA_TOOLCHAIN
cmake --preset macos-release
cmake --build --preset macos-release --parallel
cmake --build build/macos-release --target bundle
```

GCC cannot be used: every conda-forge C++ library for macOS (Qt, GDAL, PDAL)
is built against libc++, and GCC's libstdc++ does not link with them.
`cmake/toolchains/katana-macos.cmake` sets the compiler
(`arm64-apple-darwin20.0.0`), arm64, macOS 13 as the oldest target, and the run-time
paths (`@loader_path/../Frameworks`, installed in `Katana.app`). `bundle`
makes `<build>/dist/Katana/Katana.app` and `package` a `.dmg` holding it.
What it took to make the code build with clang, the app's layout, the minimum
version and Gatekeeper are `docs/release.md`, "macOS". The GPU renderer draws
on Metal there (`docs/gpu.md`). The whole suite runs on Apple silicon in the
release workflow (`docs/release.md`).

**Claude Code cloud sessions** run `.claude/hooks/session-start.sh` when they
start: it runs the setup script into `/opt/katana-toolchain`, installs Xvfb
and Mesa's Vulkan drivers for the GPU tests when it can, puts the
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
| `KATANA_BUILD_IO` | `ON` | `katana_io` and `katana_interop`, the GDAL and PDAL modules; OFF also needs `KATANA_BUILD_QT_APP=OFF`, since the application links interop, and gives a `katana_cli` whose IMPORT and EXPORT take only the native formats, .dxf and .ifc |
| `KATANA_WARNINGS_AS_ERRORS` | `ON` | `-Werror` |
| `KATANA_ENABLE_SANITIZERS` | `OFF` | ASan and UBSan where the toolchain has them; on MinGW, which ships no libsanitizer, UBSan in trap mode, so undefined behaviour aborts the test that caused it |
| `KATANA_ENABLE_CLANG_TIDY` | `OFF` | clang-tidy during compilation |
| `KATANA_ENABLE_CPPCHECK` | `OFF` | cppcheck during compilation |
| `KATANA_MODULE_FILTER` | empty | configure only the listed modules and their suites, e.g. `"katana_core;katana_math;katana_geometry"`; the configure fails, naming the module, when a listed module needs one that is not listed |
| `KATANA_DEPLOY_RUNTIME` | `ON` | copy the runtime DLLs and the GDAL and PROJ data beside the programs ("The build tree runs on its own too") |
| `KATANA_BUILTIN_CUSTOMISATION` | `resources/customisation/katana-standard.customisation.json`, tracked | the Katana customisation file compiled into `katana_cad` as THE built-in, the default; a build where the path names no file has none of its own ("The built-in customisation and the reference folder"; `docs/customisation.md`, "The built-in") |
| `KATANA_ALTERNATE_CUSTOMISATIONS` | `resources/customisation/nsw.customisation.json`, tracked | a `;`-separated list of further Katana customisation files compiled in, offered by name (Settings, `CUSTOMISE BUILTIN`, the seam); one that is no file is not offered, and the list may be empty |
| `KATANA_REQUIRE_BUILTIN_CUSTOMISATION` | `OFF` | fail the configure when the default file is absent: a guard for a release job against the tracked file having been deleted or a path being wrong |
| `KATANA_REFERENCE_CUSTOMISATION_DIR` | the reference folder under `docs/`, git-ignored | where the reference customisation is kept in the legacy formats; read only by the converter and by the legacy readers' own tests, which skip when the folder is absent or holds no such file; the program reads none of it ("The built-in customisation and the reference folder") |
| `KATANA_CCACHE` | `ON` | compile through ccache when it is installed and no `CMAKE_CXX_COMPILER_LAUNCHER` is given ("Build and test speed") |
| `KATANA_UPDATE_REPO` | `kjarada/Katana` | `owner/name` of the GitHub repository whose latest release an installed copy asks for updates; compiled into the update library ("The update layer") |
| `KATANA_UPDATE_FEED` | empty | the base URL of a release host of the owner's own, in the place of GitHub; it must honour `Range`. With this and `KATANA_UPDATE_REPO` both empty the build has no update feed |
| `KATANA_REQUIRE_UPDATE_KEYS` | `OFF` | fail the configure while `cmake/KatanaUpdateKeys.cmake` holds the placeholder keys: ON in every job of the release workflow, so that no release ships with keys nobody holds |
| `KATANA_UPDATE_SEQUENCE` | empty | the sequence of the release this build is packaged for - the number that release's update manifests carry, UTC seconds - which `package` writes beside the version into the copy's install record; the release workflow gives every job its run's. Empty: the package names its version alone. Digits only, from 1 to 4102444800, or the configure fails. A cache entry, so a build directory that packaged one release must be given the next one's before it packages again ("The update layer") |

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
| `katana_updater` | yes | the fourth program: the helper that swaps an install's files for an update's while Katana is not running, and puts them back if the new version does not start (`src/katana_updater`, `docs/update.md`). Static on Windows, linked to `katana_update` and `katana_core` alone |
| `katana_updater_test`, `katana_update_e2e`, `katana_updater_stub`, `katana_update_link_probe` | yes, with the tests | what the update system's tests run and never ship: the helper's own source and the engine, each with a TEST key in the place of the pinned ones, a stand-in for Katana that the helper starts, and a probe of the static link (`tests/update/CMakeLists.txt`, `docs/testing.md`). They land in `<build>/bin/tests`, and the install check refuses the first two by name |
| `katana_<module>_tests` | yes | one test executable per module (`katana_add_test_suite`, `docs/testing.md`) |
| `katana_qt_widget_tests` | yes | the widget tests, run offscreen |
| `katana_benchmarks` | yes | every `benchmarks/bench_*.cpp`, globbed |
| `katana_runtime` | yes | deploys the runtime beside the programs (Windows, `KATANA_DEPLOY_RUNTIME`) |
| `katana_12da_probe` | yes | what an archive file holds and what Katana would take from it (`src/katana_archive12d/probe_main.cpp`) |
| `katana_customisation_convert` | yes | style libraries and survey code files in, one Katana customisation out (`src/katana_archive12d/legacy/convert_main.cpp`); with its library `katana_legacy_customisation` it is a developer's tool, never installed and linked by none of the programs that are ("The built-in customisation and the reference folder") |
| `run-benchmarks` | no | runs `katana_benchmarks` with `--benchmark_min_time=0.2s` |
| `format`, `format-check` | no | clang-format over the first-party sources; `format-check` fails if a file would change |
| `katana_make_icons` | no | writes `resources/katana.ico`, a 256 px PNG and `resources/icon_sheet.png` from the icon painters; the output is committed (`docs/desktop.md`) |
| `katana_tool_icon_sheet` | no | every catalogue tool's icon with its name and aliases (`docs/tools.md`) |
| `bundle` | no | a self-contained `<build>/dist/Katana` ("Bundling") |
| `package` | no | that tree as `Katana-<version>-win64.zip` (`-win-arm64.zip`), and an NSIS installer when `makensis` is found; on Linux `Katana-<version>-linux-x86_64.tar.gz` (`-linux-aarch64`); on macOS `Katana-<version>-macos-arm64.dmg` holding `Katana.app`. Only a package holds `share/katana/install.json`, the record that makes a copy a release the updater will touch ("The update layer") |

### Where things are built

```
<build>/bin/katana.exe          the application (target `katana`)
<build>/bin/katana_cli.exe      the command-line front end
<build>/bin/katana_mcp.exe      the MCP server
<build>/bin/katana_updater.exe  the update helper
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
| `IMPORT <file> [LOCAL \| ALONGSIDE \| OFFSET=dE,dN]`, `EXPORT`, `REFS`, `INFO <file>` | yes (with `KATANA_BUILD_IO`; a `.dxf` without it) | yes |
| `INFO <id>` or `INFO #<id>`: an entity, the interpreter's - taken for a file only when a file of that name exists (both took every `INFO` for a file until 2026-09-26) | yes | yes |
| `COPC <source> <destination.copc.laz>` | yes | yes (since 2026-09-26) |
| `GDAL VERSION`, `LIST`, `HELP`, and `GDAL <algorithm> ... [FROM ...] [TO ...]`: any of GDAL's algorithms, run by the one geoprocessing executor (`docs/geoprocessing.md`) | yes (with `KATANA_BUILD_IO`) | yes, as a background job |
| `CUSTOMISE`: the interpreter's family - `[REPLACE] <file>...` loads Katana customisation files, `EXPORT`, `RESET`, `KEEP`, `REVERT`, `REMOVE`, `SET`, and alone what is loaded, as records (`docs/customisation.md`, "The verbs") | yes (since 2026-10-06; a survey code file or a style library of another program is refused) | yes (since 2026-10-07, and `--customise` runs this line: it goes to the interpreter as `CODE` does, and the window's own tokenizer and loader for it, which read those older files, are gone) |
| `CODE`, `CODE EXPLAIN`, `CODE CENSUS`, `CODE LIST`, `CODE CHECK` (the interpreter's since 2026-09-26; the last two were `MAPFILE LIST` and `MAPFILE CHECK` until 2026-10-06; `docs/customisation.md`, "The verbs") | yes | yes |
| `IMPORT <file.ifc>`, `EXPORT <file.ifc>`, `INFO <file.ifc>`, `IFC RULES` (`docs/ifc.md`) | yes, with or without `KATANA_BUILD_IO` | yes, with the same options, and File > Import IFC / Export IFC, which run these lines |
| `UTILITY REPORT`, `VERIFY`, `CLEARANCE`, `CHECK`, `DRAW` (the interpreter's since 2026-09-25; `docs/subsurface_utilities.md`) | yes | yes |
| `UPDATE CHECK`, `STATUS`, `DOWNLOAD CONFIRM`, `APPLY CONFIRM`, `SKIP`, `BLOCKED CLEAR`, `SET AUTO`, `TOKEN STATUS \| CLEAR`: the program's own update (`src/katana_app/update_verbs.hpp`, `docs/update.md`). In a build tree `STATUS` says `kind=development` and `CHECK` says `update=unsupported` without asking any server | yes; `APPLY CONFIRM` must be the last line of a run | yes, `CHECK`, `DOWNLOAD` and `APPLY` as background jobs; a headless window answers `CHECK` and `STATUS` only |
| `ZOOM`, `GRID`, `SNAP`, `QUIT` | `QUIT` only | yes |
| a catalogue tool's alias alone (`L`, `TRIM`, `STRETCH`) | no: "unknown command" where the interpreter has no verb of that name | starts the tool (`docs/tools.md`) |

Points are absolute (`12.5,40`), relative (`@3,4`) or polar (`@5<30`).

## The built-in customisation and the reference folder

The linestyles, symbols and survey codes a program starts with are compiled
INTO it, in the window as in `katana_cli` and `katana_mcp`: the Katana
customisation file the CMake variable `KATANA_BUILTIN_CUSTOMISATION` names,
`resources/customisation/katana-standard.customisation.json` by default, is THE
built-in; the files `KATANA_ALTERNATE_CUSTOMISATIONS` lists, the NSW file by
default, are offered beside it by name. All are embedded in `katana_cad` with
`#embed`, into one generated source in the build tree
(`docs/customisation.md`, "The built-in", has where they come from, why they
are in the repository and the environment variable that overrides the default
for one run). Where a file is absent - a path that names none; the files are
tracked, so a checkout has them - it is not offered, and with none at all the
program has no built-in and draws plain lines until a customisation is loaded.
Nothing is installed beside the program, and nothing is read from that folder
at run time.

Four things configure it, and none needs Python or a script:

| | Says | Default |
|---|---|---|
| `KATANA_BUILTIN_CUSTOMISATION` | the default file compiled in; it is picked up when it is put there after the configure (a `CONFIGURE_DEPENDS` glob over the path), and whether it is a file is asked of the file itself, so a checkout under a directory with `[` in its name still has it | `resources/customisation/katana-standard.customisation.json` |
| `KATANA_ALTERNATE_CUSTOMISATIONS` | the further files, a `;`-separated list, offered by name; each is checked and picked up the same way, a path that is no file is not offered (the configure says so), and a file already offered is offered once | `resources/customisation/nsw.customisation.json` |
| `KATANA_REQUIRE_BUILTIN_CUSTOMISATION` | make the absence of the default a configure error | `OFF`; `ON` for a release job, to catch the tracked file having been deleted or a path being wrong before a program that draws plain lines ships |
| `KATANA_REFERENCE_CUSTOMISATION_DIR` | where the reference customisation is kept in the legacy formats | the git-ignored folder under `docs/` |

Katana Standard (`docs/katana_standard.md`) is the default and the NSW file the
alternate. To change which is the default, point `KATANA_BUILTIN_CUSTOMISATION`
at the other and list the first in `KATANA_ALTERNATE_CUSTOMISATIONS`; to build
without the NSW file, which is third-party material (`LICENSING.md`), give
`-DKATANA_ALTERNATE_CUSTOMISATIONS=` an empty list and the program has Katana
Standard alone.

**A build directory keeps what it was given, and is moved once.** A cache
variable that was set keeps its value, so a directory configured when NSW was
the default still had `KATANA_BUILTIN_CUSTOMISATION` set to the NSW file: it
offered NSW as the default and Katana Standard nowhere, and the tests of the new
default skipped while the suite stayed green. So the first configure that finds
that value MOVES it to Katana Standard, once, and says so in a status line
(`cmake/KatanaBuiltInDefault.cmake`). Nothing else is moved:

- a new build directory is given the default as it is;
- a `-DKATANA_BUILTIN_CUSTOMISATION=<file>` with no type is a choice and is kept,
  on a directory that was configured before as on a new one;
- once moved, or once configured by this version, the entry is settled and a
  value given later (typed or not) is kept;
- a stale directory that holds a file of your own is not the earlier default
  and keeps it.

The one case it cannot tell apart is `-DKATANA_BUILTIN_CUSTOMISATION:FILEPATH=<the
NSW file>`, typed, on the very first configure of a directory that already had
that value; the status line says how to put NSW back as the default. To have NSW
as the default, give the file with `-D` once. The move is held by the test
`builtin_default_moves_the_earlier_nsw_default_once_and_keeps_a_choice`
(`tools/check_builtin_default.cmake`), which configures small projects with the
real CMake.

The built-in is tracked and the reference folder is not: its files are
third-party material under their own licence, kept on disk and never committed
(why the converted file may be, `docs/customisation.md`, "Why the built-in is
in the repository"). A checkout has the built-in and not the reference files,
so the tests that read the reference files - the legacy readers' reference
tests and the converter's - say so and skip, and no test of the WINDOW or of
`katana_cli` depends on either (each says what customisation the program
starts with, `docs/headless.md`, "The customisation a run starts with"). The
tests that need a compiled-in file, `BuiltInCensus.*`, `BuiltInRenames.*` and
`BuiltInLibrary.*` against it, find the built-in by NAME in the list of what the
build compiled in, run in a checkout and skip where the build has none of
that name. A build
WITHOUT the file configures, builds and passes: it was built that way on
2026-10-07, when the file was not tracked and was the only built-in, with
`-DKATANA_BUILTIN_CUSTOMISATION` naming a file that does not exist, and the cad
suite gave 2,325 passed and 5 skipped (the five that need the reference
customisation compiled in) and the app suite 124 passed
(`docs/testing.md`, "The suites that read third-party data").

A new worktree has the tracked built-in from its checkout and none of the
git-ignored folders. Copy those in before configuring, and never add them to
git: `third_party/_cache` (without it the configure tries to download
GoogleTest) and the licensed reference files kept under `docs/` that the
customisation was made from.

### The converter

`katana_customisation_convert` turns the reference files into ONE file in the
Katana customisation format, which is the file the built-in is compiled in
from (`docs/customisation.md`, "Converting a customisation from the legacy
formats", has its command line, what it does to names and colours, and the
figures of the reference conversion). It is built in `all` beside
`katana_12da_probe`, into `<build>/bin`, and like the probe it has no install
rule: `cmake --install`, the bundle and the packages hold `katana`,
`katana_cli`, `katana_mcp` and the update helper `katana_updater` only, and a
test fails the suite if another program is ever installed. The readers of the older formats it is made of are a
library of their own, `katana_legacy_customisation`
(`src/katana_archive12d/legacy/`), which no program links - a second test
walks the link closure of `katana`, `katana_cli`, `katana_mcp` and
`katana_updater` and fails on
it (`tests/CMakeLists.txt`; each check is proved on a fixture, `docs/testing.md`).

The reference folder is that of `KATANA_REFERENCE_CUSTOMISATION_DIR` (the root
`CMakeLists.txt` defines it, above the modules and the tests, so that both see
one value): two style libraries and two survey code files in the legacy
formats, with their colour table in a `support` folder beside them. It is
third-party material under its author's own licence: on the owner's machine
and in no clone, and nothing committed needs it. Two kinds of test read it:
the legacy readers' and writers' own reference tests, which skip where the
folder is absent or holds fewer than four files, and
`TheReferenceCustomisationConvertsToTheFiguresOfItsCensus`, which skips where
the folder holds no file in the legacy formats and FAILS where it holds some
that are not the four - a file missing, or one too many, is a mistake to be
told of. Point the variable elsewhere to convert a customisation kept
elsewhere:

```sh
cmake --preset release -DKATANA_REFERENCE_CUSTOMISATION_DIR=D:/Survey/Reference
```

The reference customisation is converted with the four files in this order -
the words are arguments so that no committed test, document or source spells
them (the notice the converter copies in does, being the authors' words; the
committed file has had it removed), and the file is found again at the next
build:

```sh
build/release/bin/katana_customisation_convert --name NSW \
    --description "Linestyles, symbols and survey codes of the NSW customisation." \
    --notice-from "<linestyle library>" --notice-from "<symbol library>" \
    --colours "<folder>/support/colours.4d" \
    --strip-leading-word <publisher's word> --strip-leading-word <vendor's word> \
    --remove-word <publisher's word> \
    -o resources/customisation/nsw.customisation.json \
    "<symbol library>" "<survey code file>" "<names file>" "<linestyle library>"
```

The name and the description are given word for word, unlike the rest: both
are written into the file, so its bytes - and the digest of them, by which a
customisation a user kept is told from another edition of the built-in - are
this command's and no other's. They are Katana's own words and no part of the
reference files, so a committed test or document may spell them.

**The symbol library first and the linestyle library last, by decision**
(2026-10-06). The order of the files is the load order, a customisation holds
one definition a name, and the later library's is the one kept; three names
are defined by both libraries, and the survey code rules give two of them as
their linestyle and none as a symbol. *Rejected: the order these files were
always loaded in*, the symbol library last, which kept the symbol's strokes
under four rules that draw a line. *Not done: two definitions a name*, which
a library that is one table by name cannot hold; the symbol library's three
are not in the built-in. `docs/customisation.md`, "The reference
customisation", has the whole of it, and the figures each order gives.
`tools/reference_census.py` counts the four files in the order IT is given
them, so give it the same one.

The converted file sits in `resources/customisation/`, tracked, and
`.gitattributes` keeps its bytes as they are in every checkout (they are
embedded, and their digest is recorded in the customisations users keep). The
legacy files are NOT kept there: they stay in the reference folder, which
nothing globs beside the converted file. (Until the legacy readers' reference tests read the
reference folder instead of the one the old embedding compiled from, two of
them handed every file of `resources/customisation/` to the legacy loader and
would have failed on the converted one; they read the reference folder now.)
`benchmarks/bench_customisation.cpp` measures the DEFAULT built-in when there is
one (the file `KATANA_BUILTIN_CUSTOMISATION` names, found again at each build:
Katana Standard now, and not this converted file, which is an alternate), and
`benchmarks/qt/bench_plan_paint.cpp` draws with the default's library when there
is one. Both therefore measure the smaller file since the default moved:
`bench_plan_paint` draws with 221 definitions where it drew with 792. Its
`libraryDefinitions` counter says which, and a run is compared only with one that
has the same figure, so the figures taken before the default moved are not
baselines for it.

## The update layer

An installed Katana updates itself (`docs/update.md`). What that asks of a
build:

- **Two libraries, static on Windows.** `katana_update`
  (`src/katana_update/CMakeLists.txt`) verifies an Ed25519 signature with
  OpenSSL's libcrypto and inflates with zlib. With MinGW it looks for
  `libcrypto.a`, `libz.a` and `openssl/evp.h` BY NAME (the cache variables
  `KATANA_UPDATE_CRYPTO_STATIC`, `KATANA_UPDATE_ZLIB_STATIC` and
  `KATANA_UPDATE_OPENSSL_INCLUDE`) and the configure fails without them:
  the helper runs from a copy in a folder with no `bin/` beside it, so it
  must import nothing but the operating system's DLLs. They are not asked
  for through `find_package(OpenSSL)`, whose cached answer GDAL's and
  libcurl's own configuration files would otherwise decide. Elsewhere the
  toolchain's OpenSSL and zlib are used as they are.
- **The fourth program.** `katana_updater` (`src/katana_updater`) links
  `katana_update` and `katana_core` and nothing else, with `-static` under
  MinGW, which takes libstdc++, libgcc and libwinpthread in. The test
  `katana_updater_imports_only_os_dlls` reads its import table. That
  listing speaks for what the helper CARRIES, and of the two libraries that
  is libcrypto alone: the helper checks every signature itself and never
  inflates - a release is inflated when it is staged, by the program that
  downloads it - so the linker takes nothing of zlib into it, and its check
  would pass on a link line that named zlib's DLL. For zlib the evidence is
  `katana_update_link_probe` (`tests/update/link_probe.cpp`), which uses
  Ed25519, raw inflate and crc32 through the same third-party target and
  the same link options, and whose own import table is read
  (`katana_update_link_probe_imports_only_os_dlls`). The helper is
  installed on Windows and Linux and not on macOS, where nothing is updated
  in place.
- **Where a copy asks.** `KATANA_UPDATE_REPO` and `KATANA_UPDATE_FEED`
  ("Options") are compiled into the library, with the version from
  `project(Katana VERSION ...)`: `katana_cli` has no version of its own, so
  the library carries one for every program. The default repository,
  `kjarada/Katana`, is public, so the default feed is read with no token; a
  build that names a PRIVATE repository can still be updated by a person
  who holds a read-only token for it (`docs/update.md`, "The access
  token"). That version is also what a package says it is, in its install
  record (below), and so the floor against going back: an installed copy's
  build is the higher of the one an update recorded and the one its
  package names (`docs/update.md`, "The installed version").
- **The keys.** `cmake/KatanaUpdateKeys.cmake`, included by the root
  `CMakeLists.txt`, holds the two public keys every program verifies a
  release with; they reach one source file, `src/katana_update/trust.cpp`.
  Until the owner has pinned real keys the configure says `Update keys:
  PLACEHOLDERS - this build can neither verify nor receive an update`, and
  such a build refuses every release; with `-DKATANA_REQUIRE_UPDATE_KEYS=ON`
  the configure fails instead. A build with real keys says `Update keys:
  pinned, active <key id>, spare <key id>`. The file is rewritten by
  `python tools/update_keytool.py pub`, never by hand.
- **What makes a copy a release.** `share/katana/install.json` says what
  the package is: its target (`win64`, `win-arm64`, `linux-x86_64` ...), the
  version it was built as - `project(Katana VERSION ...)`, always - and,
  where the configure was given `-DKATANA_UPDATE_SEQUENCE`, the sequence of
  the release it is packaged for; a package built by hand belongs to no
  release and names none. Never a sequence without a version: the engine
  does not read such a record, and a copy whose record does not read cannot
  be updated. The `packaging_install_record_*` tests run the script on a
  staged folder and read the record back, for this build, for a package of
  a release and for one of none. The record is written
  by a script CPack runs on the tree it stages and by nothing else. That
  script is `<build>/KatanaInstallRecord.cmake`, which is no file of the
  repository: `cmake/KatanaPackaging.cmake` ("install.json") writes it into
  the build directory at configure time and lists it first among CPack's
  pre-build scripts, before the signing script, so that what is signed
  already holds the record. A macOS bundle was sealed ad hoc by the install
  before either ran, and a file added afterwards is one that seal does not
  list; so `cmake/KatanaSign.cmake` seals the bundle again, ad hoc, when no
  Developer ID is named - the one thing it does with no certificate (since
  2026-10-14, reasoned from the files and not yet run on a Mac). So a
  build tree, `cmake --install` and the `bundle` target are DEVELOPMENT
  copies, which never check for an update and are never updated; only what
  `package` makes is a release. `tools/check_install_rules.cmake` holds
  that, and that the programs installed are the four and no other.
- **The installer.** `cmake/NSIS.template.in` replaces CPack's own, which
  asks for administrator rights: the installer is per user, so that an
  update replaces files as the same user (`docs/release.md`, "The Windows
  installer").

## Bundling

`cmake --build <build> --parallel --target bundle` installs into `<build>/dist/Katana`;
`--target package` makes `Katana-<version>-win64.zip` (and an NSIS installer
when `makensis` is on the machine), on Linux a `.tar.gz`, and on macOS a
`.dmg` holding `Katana.app`. The
result runs with no MSYS2, Qt, GDAL or toolchain prefix installed. It holds
`katana`, `katana_cli`, `katana_mcp` and, except on macOS, the update helper
`katana_updater` ("The update layer").

On Windows, copy **the whole `dist/Katana` directory**, then start
`Katana/bin/katana.exe`. Copying only the executable is not a portable
installation: its DLLs and Qt plugins are beside it, and the geospatial data
and certificates are in the neighbouring `share` and `etc` directories.
The bundle target builds all four programs first, including `katana_mcp`;
it does not rely on a previous build of `ALL`.

On Linux and macOS `cmake/KatanaDeployUnix.cmake.in` does the copying, when
the build uses a toolchain prefix: the prefix's libraries into `lib/`, Qt's
plugins into `lib/qt6/plugins` with a `bin/qt.conf`, and `share/proj`,
`share/gdal` and `ssl/cacert.pem`, in the prefix's own layout so that no
library's RPATH needs rewriting. Why that layout, and which libraries are left
to the system, is `docs/release.md`. On macOS the same tree is laid out as
`Katana.app` (`docs/release.md`, "macOS"). A build against a distribution's
own libraries installs only Katana's programs.

On Windows, three things make it so, and each was learned by the bundle
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

An unresolved non-system DLL is now a **build or install error**, naming the
missing file and the runtime directory searched. A warning was not enough:
it allowed a successful-looking build whose executable could not start on
another machine. Missing Qt deployment tools or platform plugins likewise
fail rather than leaving a partial runtime. Windows' own DLLs and API sets
remain the operating system's responsibility; they are not redistributed.

`bin/` sits beside `share/` because that is where both data-hungry libraries
look: PROJ finds `proj.db` at `<its DLL>/../share/proj` by itself, and
`locateGdalData` (in `gdal_adapter.cpp`) does the same for GDAL. See
`docs/interop.md` for why GDAL needed telling. libcurl, which GDAL and GIS >
Online Data fetch through, is the third: it checks every `https://` answer
against `<its DLL>/../etc/ssl/certs/ca-bundle.crt`, so the deploy copies the
toolchain's bundle there too. Until it did, every online request from
`bin/katana.exe` failed with "error adding trust anchors from file" while the
tests passed - they load libcurl from the toolchain, beside its own bundle.
`runtime_has_the_certificates_https_is_checked_against` checks it is there.
libcurl finds that path through the ANSI code page, which a folder named
outside it (a profile "Łukasz") breaks, so `locateCertificates` also hands
GDAL the real path in UTF-8 on Windows (`docs/release.md`, the table of
libraries' data).

The bundle is about 504 MB without the update helper. Measured on 2026-10-13
in a Release build tree, whose `bin/` and `share/` are what the bundle is
copied from (no bundle was made for the measurement): the three programs
98.8 MB (`katana.exe` 40.4, `katana_cli.exe` 28.8, `katana_mcp.exe` 29.5:
every Katana module is a static library linked into each), 189 DLLs 381.3 MB,
Qt's 11 plugins 2.8 MB, `share/proj` and `share/gdal` 13.3 MB, the
certificate bundle 0.2 MB and the samples 8.0 MB. A megabyte here is a
million bytes of the files as they lie on the disk; the compressed packages
are smaller (`docs/release.md`). The helper is not in the figure: the tree
measured held it as a placeholder program, before the swap was written into
it. Three quarters of the bundle is the DLLs: the dependency chain of GDAL
and PDAL as MSYS2 builds them, and Qt. (This paragraph said 388 MB, with
Katana's own code "a few megabytes", until that measurement.) The install
rules are `cmake/KatanaPackaging.cmake`; the licensed reference files under
`docs/` must never be installed.

The rule that once installed a removed root file unconditionally, and so
failed every `cmake --install`, is conditional now
(`packaging_installs_only_present_first_party_files`), and the release
workflow runs `package` on every platform.

## The build tree runs on its own too

Until 2026-09-23 only the bundle was self-contained: `<build>/bin/katana.exe`
found GDAL, PDAL, PROJ, Qt and the C++ runtime through `PATH`, so it ran only
where MSYS2 was installed and on `PATH` - the libraries were borrowed from the
toolchain at run time rather than being part of the program. Now the build
deploys them itself. Each shipped executable runs **the same
`KatanaDeploy.cmake`** as the bundle after linking. This includes an IDE build
of just `katana`, or `cmake --build <build> --parallel --target katana_cli`:
neither builds `ALL`, which was the gap in the earlier arrangement. The
`katana_runtime` target still runs in `ALL`, after all four programs, to repair
missing or outdated runtime files even when nothing needed linking.
`<build>/bin` holds the DLLs and Qt plugins, and the adjacent `share` and `etc`
directories hold the data and certificates. The actual target directory is
used for multi-configuration generators too. One script serves all these
paths. Switch off build-tree deployment with `-DKATANA_DEPLOY_RUNTIME=OFF`;
installation and bundling still deploy their own runtime.

A full deploy is too slow for every build - measured on the Release tree, 36 s
on an idle machine and 62-85 s beside another build, most of it
`file(GET_RUNTIME_DEPENDENCIES)` running objdump over ~200 DLLs - so the build
run is incremental. A no-op check previously measured 0-1 s. A scan is skipped
only while the runtime data is present and these things hold:

* **A key is unchanged**: an MD5, taken at configure time, of every module's
  `LINK_LIBRARIES`, the toolchain and Qt plugin locations, and the deploy
  script's own text.
* **Every requested executable has been scanned since linking**, recorded by
  name, modification time and size. A CLI-only deployment cannot satisfy a
  later first build of the desktop or MCP server. A relink is rescanned even
  if the CMake link libraries did not change: new code can start using an
  import library already named on the link line.
* **Every DLL the last deploy recorded is still there.**
* **Every one is the toolchain's current copy**, by timestamp (`file(COPY)`
  keeps the source's). MSYS2 is a rolling toolchain; an update replaces
  `libstdc++-6.dll` under the same name, and a program relinked against the
  new one that finds the old one beside itself does not start.

Anything else is a full deploy, which first re-copies every DLL already in
`bin/` from the toolchain - the scan cannot, because it resolves a dependency
BESIDE the program first and so finds, and keeps, a stale copy - and writes its
record last, so a deploy that fails part-way is retried rather than trusted.
The old stamp is removed before a full deployment starts.
Verified on the Release tree: a back-dated `libgdal-39.dll` was noticed and
replaced with the toolchain's, a deleted `libproj-25.dll` was restored, and the
next run took 0 s. With `PATH` reduced to `C:\Windows\System32;C:\Windows`,
`build/release/bin/katana_cli.exe` imported the sample LAS (PDAL), ASCII grid
and GeoJSON (GDAL) and exported a DXF (GDAL's `header.dxf`, from
`build/release/share/gdal`), and `katana.exe --screenshot` built the window.

Post-link deployments take a process-scoped file lock on the destination
before inspecting or copying anything. They scan only their own completed
executable, not another program that Ninja may still be linking. This
replaces the earlier choice to deploy only through an `ALL` target: that
avoided concurrent copies, but missed individual-target builds.

The `runtime_deployment_*` tests build small DLL chains in a path containing
spaces, build the CLI and MCP targets separately and together, relink with a
newly used import, restore a deleted transitive DLL, and require missing DLLs
and Qt tools to fail without a success stamp.
`runtime_programs_start_without_the_toolchain_on_path`
starts the real CLI and MCP server and takes a desktop screenshot with only
Windows on `PATH` and no inherited Qt, GDAL or PROJ search paths. These checks
cannot pass by borrowing the developer's installed libraries.

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
  set trimmed to what Katana reads (which would also cut most of the DLLs'
  381 MB, "Bundling").
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

## Build and test speed

Measured on 2026-09-29 on the owner's Windows 11 machine (24 threads, GCC
16.2, CMake 4.4, Ninja), with Microsoft Defender Antivirus and Defender for
Endpoint running, as they always do there. Every build at `-j4` and every
suite run at `--parallel 4`, one at a time, each build into a fresh tree; a
sampler counted the machine's `ninja.exe` processes every 30 s and found no
other build running during the builds and suite runs in the table. The baseline is commit 09121dd in a
worktree of its own; the change is the same tree with what this section
describes.

| | Baseline | Now |
|---|---|---|
| configure, fresh tree (twice each, alternating) | 40.6 s, 41.3 s | 28.5 s, 28.1 s |
| clean build, first time (ccache empty) | 1377 s, 1136 steps | 1135 s, 958 steps |
| clean build, ccache warm | - | 70 s |
| compile time summed over every step | 5457 s | 4489 s |
| whole suite, 5916 tests (two runs each) | 1064 s, 924 s | 375 s, 524 s |
| median time of one test | 0.40 s, 0.39 s | 0.07 s, 0.10 s |
| `ctest -N`, first after the build, then again | 50.7 s, 11.2 s | 0.8 s, 0.6 s |

Every run passed every test (28 skipped in each, the same 28), exit 0. Two
runs of the same tree differ by up to 40% with nothing else running; the
scanner is the likely cause, not an established one.

**Why the suite was slow.** Defender inspects a program as it starts, and an
executable it has not seen before much longer: listing the cases of a freshly
copied `katana_qt_widget_tests.exe` (48 MB) took 8.96 s the first time and
0.24 s the next. The suite starts over 5 700 processes for its GoogleTest cases
alone. With `gtest_discover_tests` in PRE_TEST mode, CMake 4.4 registers each
case as `cmake -P GoogleTest/LaunchTest.cmake`, which starts the test; the
Windows toolchain PATH was a TEST_LAUNCHER, `cmake -E env --modify`, which
made it three processes per case (`ctest --show-only=json-v1` shows the
chain). One `cmake.exe` start measured 0.09 s against 0.02 s for a test
executable listing nothing (0.46 s on an earlier, busier day). And every
`ctest` - `ctest -N` and `ctest -R` included - listed all 21 executables
again before running anything.

**What changed.**

- `katana_discover_tests` (`cmake/KatanaTargetDefaults.cmake`, with
  `cmake/KatanaGoogleTests.cmake.in`) registers each case as the executable
  itself with `--gtest_filter`: one process per case. It still lists the
  cases at test time, with CMake's own lister, and keeps the list until the
  executable is rebuilt. The toolchain is put on PATH for the listing only;
  each case keeps its `ENVIRONMENT_MODIFICATION`, which `ctest --show-only`
  reports. The function's comment says why neither of `gtest_discover_tests`'
  modes was kept. A listing that fails fails `ctest` (exit 8, `ctest -N`
  included), as it did before: checked by putting a program that exits 1 in
  place of `katana_math_tests.exe`. The three places that registered cases (the module suites,
  `tests/gpu`, `tests/qt_widgets`) call the one function.
- `katana_qt_ui` (`src/katana_qt/CMakeLists.txt`) is an OBJECT library of
  everything the window is made of except `MainWindow` and the files only it
  reaches. `katana`, `katana_qt_widget_tests` and `katana_qt_benchmarks` link
  it, where the widget tests and the benchmark used to compile 178 of the same
  sources again, with the same flags: 897 s of compile time. OBJECT, not
  STATIC, so that a file whose only effect is a static initialiser is linked
  into every program as it was when each listed the sources. The widget tests
  now also carry the window files they did not list before, and all pass.
- Google Benchmark's two feature checks that ask what C++11 guarantees
  (`HAVE_STD_REGEX`, `HAVE_STEADY_CLOCK`) are answered instead of compiled,
  linked and run (`cmake/KatanaThirdParty.cmake`): the 12.7 s of the
  configure above.
- ccache, when installed (`KATANA_CCACHE`, on by default; MSYS2:
  `pacman -S mingw-w64-ucrt-x86_64-ccache`). A cold cache cost nothing
  measurable: the 776 compile steps both builds have took 3885 s without it
  and 3741 s through it. A fresh tree of the same checkout, or a branch
  switched away from and back, then builds in about a minute.

**Rejected, with the measurement.**

- **ccache's `base_dir`**, which would let a second worktree reuse the first
  one's entries. It makes ccache give the compiler paths relative to the
  build tree, and `__FILE__` with them; the survey-format, survey-job-storage
  and customisation tests that find their fixtures beside `__FILE__` then
  looked in the wrong place, and 160 cases failed. Those tests would first
  have to take their fixture folders from a compile definition, as
  `KATANA_SURVEYIO_DATA` is given to the widget tests.
- **`gtest_discover_tests` in POST_BUILD mode**, which also registers the
  executable directly. It lists the cases while building, when nothing puts
  the toolchain on PATH; where the PATH has an older MinGW first (Git for
  Windows' bash) the listing dies with STATUS_ENTRYPOINT_NOT_FOUND and fails
  the build.
- **Precompiled headers.** A header of the 17 Qt headers and 10
  standard headers the window's sources include most, on `katana_qt_ui`:
  its 127 objects compiled in 95 s at `-j4` against 131 s without, about 3%
  of a clean build, for a 257 MB `.gch`. A precompiled header is included
  into every file of the target, so a file missing an `#include` compiles
  here and fails where the header is not precompiled, and ccache caches such
  a build only with `sloppiness` relaxed. A warm ccache does far better.
- **lld.** The 52 link and archive steps took 28 s of the build's 4489 s,
  the longest link 1.8 s (`katana_qt_widget_tests`).
- **Splitting the slowest files.** The slowest compile took 17 s
  (`src/katana_terrain/cdt_backend.cpp`), in a build that kept 3.65 of
  its 4 compilers busy on average: the build is bound by the total compile
  time, which splitting does not reduce.

**Not done.**

- **Fewer test processes.** Run whole, one process per executable and one
  after another, the 21 test executables took 34 s, against 1170 s of
  per-case time in the suite above: nearly all of what is left is starting
  processes. Registering one test per GoogleTest suite (825 of them) instead
  of per case would claw much of that back, but a case would then share its
  process with the rest of its suite, and `ctest -R Suite.Case` would no
  longer select one case. Not measured as a change.
- **The virus scanner.** What IT could exclude, for the reasons above:
  - folders: the checkout and the worktrees beside it, with their build trees
    (here `D:\01_PROGRAMMING\01_Katana` and `D:\01_PROGRAMMING\Katana-wt`),
    the toolchain (`C:\msys64`), and the ccache folder
    (`%LOCALAPPDATA%\ccache`);
  - processes: `ninja.exe`, `cmake.exe`, `ctest.exe`, `ccache.exe`,
    `g++.exe`, `cc1plus.exe`, `as.exe`, `ld.exe` (all under
    `C:\msys64\ucrt64`), so that the files they read and write are not
    scanned as they are opened;
  - or instead a Dev Drive for the checkouts, where Defender's performance
    mode scans asynchronously - Microsoft's arrangement for exactly this.
  A folder exclusion is what spares the first start of a newly linked test
  executable, which a process exclusion does not: a process exclusion covers
  what the process opens, not the process's own image. On a Hyper-V guest
  with 12 virtual CPUs and no scanner the suite before this change took 168 s
  at `--parallel 12`, against 750-1030 s here at `--parallel 14` (measured
  before this work); that guest was not measured with this change.

## Continuous integration

`.github/workflows/release.yml`, started by hand, builds and packages Windows
and Linux on x86-64 and ARM64 and macOS on Apple silicon, and publishes a
GitHub release when given a tag (`docs/release.md`). It does not run the tests: there is no CI for
the suite (audit BLD-01, OPEN), and `ctest`, run by hand, is the whole gate.
