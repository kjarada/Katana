# Building and running

How Katana is built, tested and run, what each setting of the build does, and
why the build and the bundle are laid out as they are. How the tests are
written is `docs/testing.md`; how a release is packaged and published is
`docs/release.md`.

Katana is Rust (`docs/rust.md`): one Cargo workspace, `Cargo.toml`, with
one crate per layer of `tools/check_layering.cmake`. The C++ program it was
ported from was removed on 2026-11-15, on the owner's word ("if we migrated
all files to rust, remove c++ i dont want to see it"); this document describes
the build that is left. CMake stays, as a TEST RUNNER only: the root
`CMakeLists.txt` is `project(Katana VERSION ... LANGUAGES NONE)`, compiles
nothing, and registers the process-level tests - the `cli.*` tests, the
headless window tests, the update and repository checks - against the
programs cargo built.

## Where it is built

**In WSL and natively on Windows.** The port was built in WSL alone from
2026-11-05, by the owner's ruling that nothing run on Windows until the port
was a working version (`docs/rust.md`, "The owner's decisions"). On
2026-11-16 the owner asked for the Windows build, which lifts that ruling.
The native build in MSYS2 UCRT64 ("Windows", below) is up: clippy and the
whole release build pass there (rustc 1.99, 2026-11-16); its test results
are not yet in, so this document does not yet say that the suite passes on
Windows. The WSL build is the
distribution Ubuntu-24.04 with the conda-forge prefix of
`tools/setup_linux_toolchain.py` in `/opt/katana-toolchain`; its setup, as
root, is `docs/rust.md`, "WSL, where the port is built".

| Where | Rust | The native shims' compilers | GDAL, PDAL, PROJ, SQLite, libcurl | CMake, Ninja |
|---|---|---|---|---|
| WSL, Linux | the prefix's (1.98.1 at the snapshot) | the prefix's GCC 16.2, glibc 2.28 sysroot | the prefix's | the prefix's |
| macOS (Apple silicon) | the prefix's | the prefix's clang 23 and libc++ | the prefix's | the prefix's |
| Windows x86-64 | MSYS2 UCRT64's `rust` (1.100.0) | UCRT64's GCC | UCRT64's | UCRT64's |
| Windows ARM64 | MSYS2 CLANGARM64's `rust` | CLANGARM64's clang | CLANGARM64's | CLANGARM64's |

Three files in the crates are C++, and the build compiles them: the shims of
GDAL's algorithm framework and HTTP in `crates/katana_gis/native` and of
PDAL in `crates/katana_pointcloud/native`, because those libraries have
only C++ interfaces there (`docs/rust.md`, "Third-party libraries"). They are
compiled by each crate's `build.rs` with the `cc` crate and the toolchain's
own compilers, so that the C++ ABI is that of the GDAL and PDAL linked. That
is the only reason a C++ compiler is in the toolchain.

Python 3 is used by checks and developer tools only: without it the runner
registers fewer tests and the build is the same.

## Build and test: `tools/build_visible.sh`

Every build and test run goes through `tools/build_visible.sh` (the owner's
instruction of 2026-11-03, to see every build "including the test phase"). It
opens a console window of its own on the owner's desktop and returns at once;
each line goes to the window and, through `tee`, to a log. When the run ends
it writes a status file (`exit=<code>`), which is what an agent waits for and
judges the run by, and the window stays open until it is closed.

```sh
bash tools/build_visible.sh wsl                 # all: clippy, build, cargo test, the runner's suite
bash tools/build_visible.sh wsl cargo           # the Rust workspace alone: clippy, build, cargo test
bash tools/build_visible.sh wsl cargo -p katana_cad --test interpreter   # only those cargo tests
bash tools/build_visible.sh wsl test -R Docs    # the runner's suite only, ctest's arguments after it
bash tools/build_visible.sh                     # the same phases natively on Windows: all
bash tools/build_visible.sh cargo               #   ... the Rust workspace alone
bash tools/build_visible.sh test -R '^cli\.'    #   ... the runner's suite only
bash tools/build_visible.sh close               # close this checkout's finished windows
```

Without `wsl` the phases are the same and run natively in MSYS2 UCRT64
("Windows", below); with it they run in the WSL distribution. A WSL run's
window, log and status file are on the Windows side, in `build/wsl/`
(`visible_build.log`, `visible_build.status`); the build itself is on WSL's
own disk, in `~/katana-build/<worktree>/`: cargo's target in
`cargo/`, the runner in `tests/`. A build through `/mnt/d` is slow. Git is
never run inside WSL: a worktree made by Windows git names its repository by a
`D:/` path that Linux cannot open, and the build needs no git.

A worktree's first WSL build starts from a copy of the main checkout's
`cargo/` folder (`seed_from_main_checkout`): the worktree's `.git` file names
the main checkout, and the copy keeps its timestamps, so cargo finds every
third-party crate fresh and compiles only the workspace's own. Before it, each
new worktree compiled all of them again (the 515 third-party crates of `Cargo.lock`, several minutes of
every core) - the cost of an agent's first build. Measured on 2026-11-16: in a
checkout that has built once, a whole `wsl cargo` run compiles no third-party
crate at all (cargo's fingerprint log, `CARGO_LOG=cargo::core::compiler::fingerprint=info`,
which the script passes into WSL); the copy is skipped while a run is going in
the main checkout. Rejected: one cargo folder for every checkout, whose lock
would queue the agents' builds one behind another; sccache, a tool more to
install for what one copy does. `bash tools/build_visible.sh prune` deletes
the build folders of worktrees that are gone (each about 3 GB).

What `all` runs, in order, stopping after a build step that fails:

1. `cargo clippy --locked --workspace --all-targets --release --keep-going -- -D warnings`,
   with `clippy.toml` (warnings are errors, as `-Werror` made them for
   the C++).
2. The float-to-text guard: clippy on `tests/rust_check_fixtures/float_to_string`,
   a crate whose one function writes a real with `to_string`, which
   `clippy.toml` forbids (`KATANA_REAL_FORMATTER_REQUIRED`). The run fails
   unless the lint fires, because a lint that stopped firing would pass every
   build and find nothing.
3. `cargo build --locked --workspace --release`.
4. `cargo test --locked --workspace --release --no-fail-fast`; the log sums the
   counts of every test binary into one line,
   `== cargo test: N passed, M failed, K ignored`.
5. The runner: in WSL `cmake --preset linux-release -B ~/katana-build/<worktree>/tests
   -DKATANA_PROGRAMS=<cargo>/release`, then `ctest --parallel 14
   --output-on-failure` under `xvfb-run -a`, where the window's tests draw
   through Vulkan (Mesa's lavapipe in WSL); on Windows the `release` preset
   into `build/release`, with `KATANA_PROGRAMS` the checkout's
   `target/release`, and no display server. It runs after a failed cargo
   test too, so one run gives both answers, and the run fails if either
   failed.

Every cargo is `--locked` (the committed `Cargo.lock` is what is built)
and given the same `RUSTFLAGS`, or each step would rebuild what the one before
it built. In WSL the script sets cargo's linker to the prefix's GCC and
`RUSTFLAGS="-C link-arg=-Wl,-rpath,<prefix>/lib"`, so a program finds GDAL
without `LD_LIBRARY_PATH`, and gives the build a Linux `PATH` of its own
(`docs/rust.md`, "WSL"). `KATANA_BUILD_JOBS` caps cargo's jobs when agents
build in several worktrees at once. A second run in the same place while one
is going is refused.

Without `wsl` the same phases run natively on Windows in MSYS2 UCRT64
(`/c/msys64/ucrt64/bin` first on `PATH`, or `KATANA_TOOLCHAIN_BIN` before it),
cargo building into `target` and the runner into `build/release` with the
`release` preset; the log and status are in `build/windows/`
(`visible_build.log`, `visible_build.status`), and the wait is the same loop
on that folder.

The script has no phase that builds anything but cargo's: the C++ build's
phases and the two gates that compared the C++ and Rust programs' replies
went with the C++ on 2026-11-15, since their oracle was the C++ build
(`docs/rust.md`, "The C++ is removed"). What the gates held is the runner's
suite, now run against the Rust alone.

Why a window rather than the agent's own shell: that shell moves a command
still running after ten minutes to the background, and the IDE does not show
the owner a command's output, so the long runs were the ones nobody saw. A log
file alone was rejected for the same reason: the owner would have to know to
open it. `.gitattributes` keeps every `*.sh` file LF, since Git Bash reads a
carriage return as part of each line.

## The test runner

The root `CMakeLists.txt` compiles nothing and builds no target. It is
configured with the folder of the programs and registers the tests that run
them: each `cli.*` test runs `katana_cli` or `katana_mcp`, each headless
window test runs `katana` through `tools/check_screenshot.cmake`, and the
update tests run `katana_updater` and its test programs. Rejected: porting
those six hundred tests to a runner of cargo's - their definitions and the
scripts that drive them already do the job, and every one passed against the
Rust programs at the gates.

| Setting | Default | What it is |
|---|---|---|
| `KATANA_PROGRAMS` | `$CARGO_TARGET_DIR/release` when that is set; else `~/katana-build/<checkout>/cargo/release` off Windows and `target/release` on it | the folder of the programs cargo built |
| `KATANA_PROGRAM_<name>` | `<KATANA_PROGRAMS>/<name>` | one program, for `katana`, `katana_cli`, `katana_mcp`, `katana_updater` and the updater's three test programs |
| `KATANA_RUNTIME_BIN` | MSYS2 UCRT64's `bin` on Windows, else `KATANA_PROGRAMS` | the folder first on every program test's `PATH`, where the libraries are |
| `KATANA_TEST_CLI`, `KATANA_TEST_WINDOW` | `ON` | the two families of process tests, so that a runner with no display can leave the window's out |
| `KATANA_REQUIRE_UPDATE_KEYS` | `OFF` | fail the configure while `cmake/KatanaUpdateKeys.cmake` holds placeholders |

Its presets (`CMakePresets.json`) are `release` (Windows), `linux-release` and
`macos-release`, each into `build/<preset>` with Ninja, with a test preset of
the same name; `tools/build_visible.sh` gives the WSL run its own build
directory on WSL's disk. By hand, after a cargo build:

```sh
cmake --preset linux-release -DKATANA_PROGRAMS=$CARGO_TARGET_DIR/release
ctest --preset linux-release --parallel 14
```

A program that is not there is not a reason to register fewer tests: its
tests fail, which is what they should do. Judge a run by its exit code, never
by filtering its output.

## Linux

The toolchain is one prefix of conda-forge packages
(`tools/setup_linux_toolchain.py`), because distributions lag what the crates
need: Ubuntu 24.04 ships PROJ 9.4 and a Rust older than the workspace's
`rust-version`. It holds Rust (with cargo, clippy and rustfmt), GCC 16.2 and
its glibc 2.28 sysroot for the shims and the link, CMake and Ninja for the
runner, and PROJ, GDAL, PDAL and SQLite (libcurl comes with GDAL). It is
solved and installed by py-rattler, from PyPI, so it needs neither conda nor
root beyond write access to the prefix, and it reaches only
`conda.anaconda.org` and `pypi.org` - all a Claude Code cloud session's egress
allows. The solve is frozen at a date (`SNAPSHOT` in the script), so every
machine gets the same versions until that date is moved; a prefix whose stamp
matches is left alone.

```sh
python3 tools/setup_linux_toolchain.py      # once: into /opt/katana-toolchain, or $KATANA_TOOLCHAIN
cd /opt/katana-toolchain && mkdir -p bin-tools && for t in cmake ctest cpack ninja \
    cargo rustc rustdoc cargo-clippy clippy-driver rustfmt cargo-fmt; do
    ln -sf ../bin/$t bin-tools/$t; done
```

Only those tools go on `PATH`: the prefix's `bin/` also holds a python3 that
would shadow the system's. On 2026-11-05 the snapshot gave rustc 1.98.1,
GCC 16.2.0, GDAL 3.13.2, PROJ 9.8.1 and PDAL 2.10.1.

**The specs are the Rust's alone** since 2026-11-15: what only the C++ build
needed (Qt, CGAL, Boost, GoogleTest and the rest) was taken out, so a new
prefix is smaller, and one made before then still has them and builds the
same. The solve was not read again for the smaller set, so which versions it
gives GDAL and the rest is to be seen in the first prefix made from it; the
snapshot date is unchanged.

**Run-time paths.** cargo links each program with `-Wl,-rpath,<prefix>/lib`
(`RUSTFLAGS`, set by `tools/build_visible.sh` and the release jobs), so the
programs and the tests load the prefix's libraries without
`LD_LIBRARY_PATH`. A packaged program finds the `lib/` beside its `bin/`
instead: the packaging rewrites that one entry to `$ORIGIN/../lib`
(`docs/release.md`, "Packaging"). The sysroot is pinned to glibc 2.28, so
what is built here runs on Debian 10, Ubuntu 20.04, RHEL 8 and later.

**PROJ's data.** In a relocated prefix PROJ's compiled-in search path is
rewritten at install time, and with PDAL loaded PROJ then opened its data
directory as `proj.db` and as each grid. So Katana looks for PROJ's data
beside the loaded `libproj` (`crates/katana_core/src/library_data.rs`),
as it looks for GDAL's beside the GDAL library on Windows, and hands it to
each PROJ user's own search path: its own contexts, and GDAL's (so PDAL's).
An explicit `PROJ_DATA` still wins when it works. When it does not - a
machine-wide `PROJ_LIB` left by PostGIS, QGIS or OSGeo4W naming a folder with
no `proj.db` or another PROJ's - both fall back on the data Katana carries
beside its `libproj`, and if neither opens the error names the variable and
its value and says to unset it.

**The window and the GPU.** The window draws with egui and wgpu, through
Vulkan on Linux (lavapipe where there is no GPU), and loads only the system's
graphics and windowing libraries: what the Linux release job installs for its
tests is the list in `docs/rust.md`, "WSL" (Xvfb, Mesa's Vulkan drivers, the
xcb libraries). The headless window (`--screenshot` and the switches of
`docs/headless.md`) draws with no display and no GPU.

## macOS

The same script installs conda-forge's `osx-arm64` packages, with clang 23
and libc++ in place of GCC (Intel Macs are not a target; `docs/release.md`,
"macOS"). GCC cannot be used: every conda-forge C++ library for macOS (PDAL,
GDAL) is built against libc++, so the PDAL shim must be compiled by that clang.
What cargo is given there, as the release job gives it:

```sh
export CC=$KATANA_TOOLCHAIN/bin/arm64-apple-darwin20.0.0-clang
export CXX=$KATANA_TOOLCHAIN/bin/arm64-apple-darwin20.0.0-clang++
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=$CC
export CXXFLAGS=-D_LIBCPP_DISABLE_AVAILABILITY MACOSX_DEPLOYMENT_TARGET=13.0
export SDKROOT=$(xcrun --sdk macosx --show-sdk-path)
export RUSTFLAGS="-D warnings -C link-arg=-Wl,-rpath,$KATANA_TOOLCHAIN/lib"
cargo build --locked --workspace --release
```

`-D_LIBCPP_DISABLE_AVAILABILITY` because libc++'s availability markup refuses
functions the SYSTEM libc++ of the deployment target lacks, and the programs
load conda-forge's, which the package carries (conda-forge's documented
remedy). This recipe is the release job's and has not been run.

**Claude Code cloud sessions** run `.claude/hooks/session-start.sh` when they
start, which runs the setup script into `/opt/katana-toolchain`.

## Windows

MSYS2 UCRT64 has every library and a Rust for it, `mingw-w64-ucrt-x86_64-rust`
(target `x86_64-pc-windows-gnu`, against UCRT): a program it builds imports
only `api-ms-win-crt-*` and system DLLs, never msvcrt. Why that and not
rustup's or the MSVC Rust is `docs/rust.md`, "Windows, once the port works".

**The packages.** MSYS2 installed in `C:\msys64` (the path the script and the
`release` preset name), then, in its UCRT64 shell, the set the Windows
release job installs (`.github/workflows/release.yml`, its `pacboy` line):

```sh
pacman -S --needed mingw-w64-ucrt-x86_64-rust mingw-w64-ucrt-x86_64-gcc \
    mingw-w64-ucrt-x86_64-cmake mingw-w64-ucrt-x86_64-ninja mingw-w64-ucrt-x86_64-python \
    mingw-w64-ucrt-x86_64-gdal mingw-w64-ucrt-x86_64-pdal mingw-w64-ucrt-x86_64-proj \
    mingw-w64-ucrt-x86_64-sqlite3 mingw-w64-ucrt-x86_64-curl
pacman -S --needed mingw-w64-ucrt-x86_64-nsis      # only to make the installer
```

GCC is there for the three shims, which must be compiled by the compiler
GDAL and PDAL were built with; the linker cargo uses is the same GCC. Python
is for the checks only. Install `rust` from the same package list as the
rest, never after a lone `pacman -Sy`: a newer build of it can need an
`llvm-libs` that the rest of the toolchain was not built against
(`docs/rust.md`, "Windows, once the port works"). `--needed` leaves an
installed package as it is, but upgrading `rust` alone would move it past
that toolchain, so update MSYS2 whole (`pacman -Syu`) or not at all. The
owner's machine had 1.99.1-1, installed on 2026-11-25 from the package list
of 2026-10-24 that the rest came from; by that afternoon's builds it had
1.100.0 (Rev2), and they ran clean on it. 1.100 brought lints 1.99 did not
give, each mended at its source the same day rather than allowed: Clippy's
`needless_range_loop` (two loops in `crates/katana_terrain/src/tin_surface.rs`
and one in `crates/katana_cad/src/plotting/sheet_verbs.rs`, each now over the
slice it read by index, the same items in the same order), `needless_bool` (in
`crates/katana_window/src/drawing/feedback_painter.rs`) and
`doc_lazy_continuation`; and Cargo's own `unused_dependencies` and
`unused_workspace_dependencies`, which found `egui_extras` in the window (used
by nothing, and gone with the two crates only it brought in), `katana_core` in
`katana_math`, `katana_geometry` in `katana_surveyio` (its tests' only, so a
dev-dependency now), and `katana_updater` and `katana_window` in the
workspace's table (no crate takes either from it). The WSL toolchain, 1.98.1,
gives none of these, so a change can be clean there and not here: build both
before a hand-off.

**The build.** `bash tools/build_visible.sh` from Git Bash (or any bash),
with no `wsl`: it puts `/c/msys64/ucrt64/bin` first on `PATH` itself. By
hand, with that folder first on `PATH`:

```sh
cargo build --locked --workspace --release
cmake --preset release -DKATANA_PROGRAMS=$(cygpath -m "$PWD/target/release")
ctest --preset release --parallel 14
```

Clippy and the release build pass this way (2026-11-16, rustc 1.99); the
results of cargo's tests and the runner's suite on Windows are not yet in.

**Running what cargo built.** The programs in `target/release` load GDAL,
PDAL, PROJ, SQLite, libcurl and the GCC runtime as DLLs of
`C:\msys64\ucrt64\bin`, which is on no `PATH` a program started from
Explorer has. The runner puts that folder first on every test's `PATH`
(`KATANA_RUNTIME_BIN`). To run the programs, `bash tools/stage_windows.sh`
from the checkout's root makes a self-contained copy in
`build/windows/Katana`, laid out as an install is: `bin/` with the four
programs and every DLL they import from UCRT64, followed import by import
with `objdump`, `share/gdal` and `share/proj`, and
`etc/ssl/certs/ca-bundle.crt`, without which every `https://` request fails.
`bash tools/stage_windows.sh --beside` puts the same runtime beside cargo's
own output instead (the DLLs into `target/release`, the data one folder up),
so that `target/release/katana.exe` itself starts from Explorer. It copies
only, and refreshes a stage made before; it is a developer's copy, not a
package ("Bundling"). `tools/build_visible.sh` runs `--beside` at the end of
every Windows build, so both copies follow each build without a second step:
a build whose program cannot be opened is not a build the owner can test
(the owner's instruction, restated on 2026-11-25), and the runner's suite
then runs the programs with the very DLLs the owner's copy has.

**A program that will not start with 0xC0000139** ("entry point not found")
has found every DLL by name, but one of them lacks a function another
imports: two UCRT64 packages that were not built against each other. On
2026-11-25 that was `mingw-w64-ucrt-x86_64-hdf5` 2.2.0-4, built without the
ROS3 driver, under `netcdf` 4.10.1-2, which imports `H5Pset_fapl_ros3` and
was built against 2.2.0-2; GDAL loads NetCDF, so every program failed to
start and 609 of the runner's 825 tests failed. Putting HDF5 2.2.0-2 back
from pacman's cache (`pacman -U
/var/cache/pacman/pkg/mingw-w64-ucrt-x86_64-hdf5-2.2.0-2-any.pkg.tar.zst`)
mended it; GDAL 3.13.3-3 was built before 2.2.0-4 existed, so it matches the
older HDF5 too. A whole `pacman -Syu` brings 2.2.0-4 back, and should mend it
only once MSYS2 has rebuilt NetCDF against it. To find such a pair, list each
staged file's imports and each DLL's exports with `objdump -p` and look for
an imported name that the DLL does not export.

The Windows-only code - the updater's swap and its registry, the Credential
Manager, the wide-path file operations - is compiled only here, and tested
only by the native suite run (`docs/rust.md`, "Not done").

On ARM64, MSYS2's CLANGARM64 environment has the same libraries with clang
and libc++ and a Rust built for them (`mingw-w64-clang-aarch64-rust`); the
shims are then compiled with `CC=clang CXX=clang++`, as the release job does.
That has not been run on a machine of the owner's; the release job is where
it is first tried.

## What cargo builds

| Program | Crate | What it is |
|---|---|---|
| `katana` | `katana_window` | the desktop window (`docs/desktop.md`), and its headless switches (`docs/headless.md`) |
| `katana_cli` | `katana_app` | the command-line front end |
| `katana_mcp` | `katana_app` | the same session served to an agent over the Model Context Protocol (`docs/mcp.md`) |
| `katana_updater` | `katana_updater` | the fourth program: the helper that swaps an install's files for an update's while Katana is not running, and puts them back if the new version does not start (`docs/update.md`); it depends on the update layer and core alone |
| `katana_updater_test`, `katana_updater_stub`, `katana_update_e2e` | `katana_updater` | what the update tests run and never ship: the helper and the engine with a TEST key in place of the pinned ones, and a stand-in for Katana the helper starts |
| `katana_make_icons`, `katana_tool_icon_sheet` | `katana_window` | developer tools: the icon files the window and the installer carry, committed (`docs/desktop.md`), and every catalogue tool's icon with its name (`docs/tools.md`) |

They land in `<cargo target>/release`: `target/release` in a Windows
checkout, `~/katana-build/<worktree>/cargo/release` in WSL. Only the first
four are packaged, by name (`docs/release.md`, "Packaging").

## What the build reads from its environment

The crates' build scripts take a few values from cargo's environment, where
the C++ build took them as CMake options:

| Variable | Read by | Default | Effect |
|---|---|---|---|
| `KATANA_UPDATE_REPO` | `crates/katana_update/build.rs` | `kjarada/Katana` | `owner/name` of the GitHub repository whose latest release an installed copy asks for updates |
| `KATANA_UPDATE_FEED` | the same | empty | the base URL of a release host of the owner's own, in place of GitHub; it must honour `Range`. With both empty the build has no update feed |
| `KATANA_REQUIRE_UPDATE_KEYS` | the same | unset | `ON` (CMake's truth: ON, YES, TRUE, Y, a non-zero number) fails the build while `cmake/KatanaUpdateKeys.cmake` holds the placeholder keys; set in every release job |
| `KATANA_ALTERNATE_CUSTOMISATIONS` | `crates/katana_cad/build.rs` | `resources/customisation/nsw.customisation.json` | a `;`-separated list of further customisation files compiled in, offered by name; one that is no file is not offered |
| `KATANA_REQUIRE_BUILTIN_CUSTOMISATION` | the same | unset | `ON` fails the build when the default built-in is absent: a guard for a release job against the tracked file being deleted |
| `KATANA_TOOLCHAIN` | the build scripts of geodesy, gis, pointcloud and storage | unset | the conda-forge prefix: its `lib/` as a link search path and, on Linux, its compilers for the shims |
| `CC`, `CXX`, `AR` | `cc`, in gis and pointcloud | the toolchain's | the shims' compilers, where another is wanted |

A value cargo is given reaches the program; a value cargo is not given is
not an error - the program is built without it, and no program says which
repository it asks. That is why a release's packaging reads back what the
build script was given (`docs/release.md`, "Packaging").

The version is `project(Katana VERSION ...)` of the root `CMakeLists.txt`,
read by `crates/katana_update/build.rs`: the one place a version is
written, which the release workflow and the packaging read too. The crates'
own `version` in `Cargo.toml` is `0.0.0` and is not the product's.

## Running

### The desktop application

```sh
~/katana-build/<worktree>/cargo/release/katana [project-dir] [file-to-import ...] [switches]   # WSL
build/windows/Katana/bin/katana.exe [project-dir] [file-to-import ...] [switches]               # Windows, after tools/stage_windows.sh
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
run it headless under a timeout:

```sh
timeout 120 katana samples-copy --screenshot out.png
```

### The command line

`katana_cli` is the same engine with no window:

```sh
katana_cli                                   # interactive; HELP lists commands, QUIT leaves
katana_cli drawing.kcs                       # run a script; '#' starts a comment
katana_cli -c "RECT 0,0 30,20" -c "SAVE site.katana"
katana_cli --help                            # the interpreter's verbs and the CLI's own
```

A script or a `-c` batch stops at the first failing command and exits 1, so it
composes with shell pipelines; a bad argument exits 2. Both front ends run
the one command interpreter (`docs/cad.md`, "Command interpreter"), and each
adds verbs the engine cannot have, because the cad layer may not see GDAL,
PDAL or the archive readers (`tools/check_layering.cmake`): `IMPORT`,
`EXPORT`, `REFS`, `INFO <file>`, `COPC`, `GDAL ...` and the update verbs are
the session's (`katana_app`), and the window's command line accepts them
too. Points are absolute (`12.5,40`), relative (`@3,4`) or polar (`@5<30`).

## The built-in customisation

The linestyles, symbols and survey codes a program starts with are compiled
INTO it, in the window as in `katana_cli` and `katana_mcp`:
`resources/customisation/katana-standard.customisation.json` is THE built-in,
and the files `KATANA_ALTERNATE_CUSTOMISATIONS` lists, the NSW file by
default, are offered beside it by name. `crates/katana_cad/build.rs`
embeds each with `include_bytes!`, the default first; a path that names no
file is not offered, and with none at all the program has no built-in and
draws plain lines until a customisation is loaded. Nothing is installed
beside the program, and nothing is read from that folder at run time.
`docs/customisation.md`, "The built-in", has where the files come from, why
they are in the repository and the environment variable that overrides the
default for one run.

The default is not read from cargo's environment: `KATANA_BUILTIN_CUSTOMISATION`
is the RUN-time seam, which the suite and a person set for one run, and a
`KATANA_BUILTIN_CUSTOMISATION=none cargo test` must not build a program with
no built-in. To change the default, change the constant in the build script.

Cargo keeps no cache of the choice, so a changed default takes effect at the
next build with nothing stale to move (the C++ build needed a script for
that).

The reference customisation in the legacy formats, which the NSW file was
converted from, is third-party material kept on the owner's machine and never
committed; since 2026-11-03 nothing needs it (`docs/customisation.md`). The
converter of those formats was a C++ program and has no Rust port: the NSW
built-in is the committed JSON file.

## The update layer

An installed Katana updates itself (`docs/update.md`). What that asks of a
build:

- **The keys.** `cmake/KatanaUpdateKeys.cmake` holds the two public keys every
  program verifies a release with, between two marker lines that
  `python tools/update_keytool.py pub` rewrites, never a hand. A CMake file
  still, because the release workflow's first job and the runner check it
  with CMake, and the update tools read it (`--keys-cmake`);
  `crates/katana_update/build.rs` reads the same block, holds it to the
  same rules (64 hexadecimal digits, no published RFC 8032 test key, the
  spare not the active, the placeholder flag agreeing with the bytes) and
  compiles the keys in. With `KATANA_REQUIRE_UPDATE_KEYS=ON` the build fails
  while they are placeholders.
- **Where a copy asks.** `KATANA_UPDATE_REPO` and `KATANA_UPDATE_FEED`, with
  the version, are compiled into the update crate: every program carries the
  same three. The default repository, `kjarada/Katana`, is public, so the
  default feed is read with no token; a build that names a PRIVATE repository
  can be updated by a person who holds a read-only token for it
  (`docs/update.md`, "The access token").
- **The fourth program.** `katana_updater` depends on the update layer and
  core alone (`tools/check_layering.cmake`), so that it can run from a copy in
  a folder with no `bin/` beside it. It verifies Ed25519 and inflates with
  pure-Rust crates (ed25519-dalek, miniz_oxide), so it needs neither OpenSSL
  nor zlib, which the C++ helper linked statically. Whether the Windows build
  of it imports only the system's DLLs is what the runner's
  `katana_updater_imports_only_os_dlls` reads, on Windows where `objdump` is
  found; the first native run is the first to read it of the Rust helper.
- **What makes a copy a release.** `share/katana/install.json`, written only
  into a package's staged tree by CPack (`docs/release.md`, "Packaging"):
  cargo's target folder, `cmake --install` and the `bundle` target are
  DEVELOPMENT copies, which never check for an update and are never updated.

## Bundling

`cmake/KatanaPackaging.cmake`, through the packaging project
`packaging/CMakeLists.txt`, installs the four programs from cargo's release
folder with the libraries they load, as a self-contained tree, and CPack
makes the packages from it:

```sh
cd packaging
cmake --preset package -DKATANA_PROGRAMS=<cargo target>/release   # + -DKATANA_TOOLCHAIN=<prefix> on Linux, macOS
cmake --build ../build/package --target bundle                    # build/package/dist/Katana
cmake --build ../build/package --target package                   # the zip and installer, tarball or disk image
```

`docs/release.md`, "Packaging", is the whole of it: the layout, the
dependency scan, the run-time paths, the install record and what a release
configure checks. On Windows, copy **the whole `dist/Katana` directory**, then
start `Katana/bin/katana.exe`; its DLLs are beside it, and the geospatial data
and certificates in the neighbouring `share` and `etc` directories.

Cargo's target folder is not made self-contained by the build, as the C++
build tree was after every link: a developer runs the programs where the
toolchain is - in WSL through their run-time path, on Windows with MSYS2 on
`PATH`, which the runner puts first on every test's (`KATANA_RUNTIME_BIN`) -
or, on Windows, stages a runnable copy by hand with `tools/stage_windows.sh`
("Windows"). The bundle is the tree a release ships.

`bin/` sits beside `share/` because that is where the data-hungry libraries
look: PROJ finds `proj.db` at `<its library>/../share/proj` by itself, and
`crates/katana_core/src/library_data.rs` finds GDAL's data and libcurl's
certificates the same way, beside the library that reads them.

**PDAL needs a home directory.** Run with an EMPTY environment (`env -i`),
PDAL refused every file: "No home directory found" - it reads `USERPROFILE`
(or `HOME`) for its plugin configuration. Every Windows session has
`USERPROFILE`, so this does not reach a user, but a service account or a
container that clears the environment would meet it.

## Why DLLs beside the program, and not static linking

Linking GDAL and PDAL INTO the programs was considered and is not done, for
reasons that are facts about the toolchain rather than preferences:

* **MSYS2 ships PDAL and PROJ as DLLs only.** `libgdal.a` exists; there is no
  `libpdalcpp.a` and no `libproj.a` (checked in `C:/msys64/ucrt64/lib`,
  2026-09-23).
* **Linking GDAL statically alone would be a defect.** `libpdalcpp` imports
  the GDAL DLL itself, so a Katana with GDAL linked in would load a SECOND
  GDAL through PDAL: two driver registries, two error states, two `GDAL_DATA`
  settings in one process, and a dataset opened by one unusable by the other.
* **A static build is a build of the whole stack from source**: GDAL, PROJ
  (with `proj.db` embedded), PDAL, GEOS, SQLite, libtiff, libgeotiff, curl and
  their dependencies, as static libraries, with a driver set trimmed to what
  Katana reads. PDAL's CMake is built around shared libraries and plugins, so
  it is the hard part.
* **Licences.** GEOS is LGPL-2.1: linking it statically obliges whoever
  distributes Katana to let a recipient relink it against a modified GEOS; as
  a DLL it is replaceable as it stands.

So "part of the program" is delivered as: every library in `bin/` beside the
executable, found first by Windows' DLL search order, and never the
toolchain's at run time.

## Build and test speed

The measurements of the C++ build - its configure, its clean and ccache
builds, the five thousand GoogleTest processes the Windows virus scanner
slowed - described a build that no longer exists and were removed with it;
`git log` keeps them. The Rust's are `docs/rust.md`, "Measurements". What
carries over: Microsoft Defender inspects a program as it starts, and a newly
built one much longer, so on Windows - where every cargo test binary and every
`cli.*` process is a new program - the checkout, its worktrees, the toolchain
(`C:\msys64`) and cargo's folders are what IT could exclude, or a Dev Drive
used for the checkouts. How long the native build and suite take is to be
measured on its first passing run.

## Continuous integration

`.github/workflows/release.yml`, started by hand, builds Windows and Linux on
x86-64 and ARM64 and macOS on Apple silicon with cargo, runs cargo's tests
and the runner's whole suite on each, packages and smoke-tests the packages,
and publishes a GitHub release when given a tag (`docs/release.md`). There is
no CI on a push: the release workflow and the runs of
`tools/build_visible.sh` are the whole gate. The workflow in its Rust form has
not been run (`docs/release.md`, "Not done").
