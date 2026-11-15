# Building and running

How Katana is built, tested and run, what each setting of the build does, and
why the build and the bundle are laid out as they are. How the tests are
written is `docs/testing.md`; how a release is packaged and published is
`docs/release.md`.

Katana is Rust (`docs/rust.md`): one Cargo workspace, `rust/Cargo.toml`, with
one crate per layer of `tools/check_layering.cmake`. The C++ program it was
ported from was removed on 2026-11-15, on the owner's word ("if we migrated
all files to rust, remove c++ i dont want to see it"); this document describes
the build that is left. CMake stays, as a TEST RUNNER only: the root
`CMakeLists.txt` is `project(Katana VERSION ... LANGUAGES NONE)`, compiles
nothing, and registers the process-level tests - the `cli.*` tests, the
headless window tests, the update and repository checks - against the
programs cargo built.

## Where it is built

**In WSL, until the port works on Windows** (the owner's ruling of
2026-11-05, `docs/rust.md`, "The owner's decisions"): the distribution
Ubuntu-24.04, with the conda-forge prefix of `tools/setup_linux_toolchain.py`
in `/opt/katana-toolchain`. Its setup, as root, is `docs/rust.md`, "WSL, where
the port is built". Nothing is built or run on Windows meanwhile; the native
Windows build below is written for when the owner says so.

| Where | Rust | The native shims' compilers | GDAL, PDAL, PROJ, SQLite, libcurl | CMake, Ninja |
|---|---|---|---|---|
| WSL, Linux | the prefix's (1.98.1 at the snapshot) | the prefix's GCC 16.2, glibc 2.28 sysroot | the prefix's | the prefix's |
| macOS (Apple silicon) | the prefix's | the prefix's clang 23 and libc++ | the prefix's | the prefix's |
| Windows x86-64 | MSYS2 UCRT64's `rust` (1.99.0) | UCRT64's GCC | UCRT64's | UCRT64's |
| Windows ARM64 | MSYS2 CLANGARM64's `rust` | CLANGARM64's clang | CLANGARM64's | CLANGARM64's |

Three files in the crates are C++, and the build compiles them: the shims of
GDAL's algorithm framework and HTTP in `rust/crates/katana_gis/native` and of
PDAL in `rust/crates/katana_pointcloud/native`, because those libraries have
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
bash tools/build_visible.sh close               # close this checkout's finished windows
```

A WSL run's window, log and status file are on the Windows side, in
`build/wsl/` (`visible_build.log`, `visible_build.status`); the build itself is
on WSL's own disk, in `~/katana-build/<worktree>/`: cargo's target in
`cargo/`, the runner in `tests/`. A build through `/mnt/d` is slow. Git is
never run inside WSL: a worktree made by Windows git names its repository by a
`D:/` path that Linux cannot open, and the build needs no git.

What `all` runs, in order, stopping after a build step that fails:

1. `cargo clippy --locked --workspace --all-targets --release --keep-going -- -D warnings`,
   with `rust/clippy.toml` (warnings are errors, as `-Werror` made them for
   the C++).
2. The float-to-text guard: clippy on `tests/rust_check_fixtures/float_to_string`,
   a crate whose one function writes a real with `to_string`, which
   `rust/clippy.toml` forbids (`KATANA_REAL_FORMATTER_REQUIRED`). The run fails
   unless the lint fires, because a lint that stopped firing would pass every
   build and find nothing.
3. `cargo build --locked --workspace --release`.
4. `cargo test --locked --workspace --release --no-fail-fast`; the log sums the
   counts of every test binary into one line,
   `== cargo test: N passed, M failed, K ignored`.
5. The runner: `cmake --preset linux-release -B ~/katana-build/<worktree>/tests
   -DKATANA_PROGRAMS=<cargo>/release`, then `ctest --parallel 14
   --output-on-failure` under `xvfb-run -a`, where the window's tests draw
   through Vulkan (Mesa's lavapipe in WSL). It runs after a failed cargo test
   too, so one run gives both answers, and the run fails if either failed.

Every cargo is `--locked` (the committed `rust/Cargo.lock` is what is built)
and given the same `RUSTFLAGS`, or each step would rebuild what the one before
it built. In WSL the script sets cargo's linker to the prefix's GCC and
`RUSTFLAGS="-C link-arg=-Wl,-rpath,<prefix>/lib"`, so a program finds GDAL
without `LD_LIBRARY_PATH`, and gives the build a Linux `PATH` of its own
(`docs/rust.md`, "WSL"). `KATANA_BUILD_JOBS` caps cargo's jobs when agents
build in several worktrees at once. A second run in the same place while one
is going is refused.

Without `wsl` the same phases run natively on Windows in MSYS2 UCRT64
(`/c/msys64/ucrt64/bin` first on `PATH`, or `KATANA_TOOLCHAIN_BIN` before it),
cargo building into `rust/target` and the runner into `build/release` with the
`release` preset; the log and status are in `build/windows/`. That mode is
kept for step 8 and not run now.

**Gone with the C++** (2026-11-15): the `build` phase with its CMake targets,
the C++ configure and ninja, the clang build directory of `KATANA_BUILD_DIR`,
`KATANA_CONFIGURE_ARGS` and `KATANA_BUILD_KEEP_GOING`, and the two gates `wsl
gate` and `wsl wgate`, which ran the C++ suite's cli and window tests against
the Rust programs and compared the two programs' replies (step 6's gate, 379
of 379; step 7's, 266 of 267: `docs/rust.md`). Their oracle was the C++ build;
what they held is now the runner's suite run against the Rust alone. Two cargo
tests that ran the C++ went with them for the same reason: the round-trip
gate, which had every file of the test data read and written by both
programs, and the number-format oracle, which compared the reply's number
texts with the C++'s on every exponent.

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
| `KATANA_PROGRAMS` | `$CARGO_TARGET_DIR/release` when that is set; else `~/katana-build/<checkout>/cargo/release` off Windows and `rust/target/release` on it | the folder of the programs cargo built |
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

**What the prefix no longer holds** (2026-11-15). The C++ build needed Qt,
CGAL with GMP and MPFR, Eigen, Boost's headers, nlohmann_json, GoogleTest,
Google Benchmark, clang-format, the GL and Vulkan headers Qt's build asked for,
and the headers of OpenSSL and zlib that the C++ update library linked; the
Rust needs none of them (its update engine verifies Ed25519 and inflates in
Rust). They were taken out of the script's specs, so its next run installs the
smaller set; a prefix made before then still has them and builds the same.
The solve was not read again for the smaller set, so which versions it gives
GDAL and the rest is to be seen in the first prefix made from it; the snapshot
date is unchanged.

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
beside the loaded `libproj` (`rust/crates/katana_core/src/library_data.rs`),
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
cargo build --manifest-path rust/Cargo.toml --locked --workspace --release
```

`-D_LIBCPP_DISABLE_AVAILABILITY` because libc++'s availability markup refuses
functions the SYSTEM libc++ of the deployment target lacks, and the programs
load conda-forge's, which the package carries (conda-forge's documented
remedy). This recipe is the release job's and has not been run (step 8).

**Claude Code cloud sessions** run `.claude/hooks/session-start.sh` when they
start, which runs the setup script into `/opt/katana-toolchain`.

## Windows

MSYS2 UCRT64 has every library and a Rust for it, `mingw-w64-ucrt-x86_64-rust`
(target `x86_64-pc-windows-gnu`, against UCRT): a program it builds imports
only `api-ms-win-crt-*` and system DLLs, never msvcrt. Why that and not
rustup's or the MSVC Rust is `docs/rust.md`, "Windows, once the port works".
With `/c/msys64/ucrt64/bin` first on `PATH`:

```sh
cargo build --manifest-path rust/Cargo.toml --locked --workspace --release
cmake --preset release -DKATANA_PROGRAMS=$(cygpath -m "$PWD/rust/target/release")
ctest --preset release --parallel 14
```

On ARM64, MSYS2's CLANGARM64 environment has the same libraries with clang
and libc++ and a Rust built for them (`mingw-w64-clang-aarch64-rust`); the
shims are then compiled with `CC=clang CXX=clang++`, as the release job does.
Nothing of this has been run since the owner's ruling (step 8).

## What cargo builds

| Program | Crate | What it is |
|---|---|---|
| `katana` | `katana_window` | the desktop window (`docs/desktop.md`), and its headless switches (`docs/headless.md`) |
| `katana_cli` | `katana_app` | the command-line front end |
| `katana_mcp` | `katana_app` | the same session served to an agent over the Model Context Protocol (`docs/mcp.md`) |
| `katana_updater` | `katana_updater` | the fourth program: the helper that swaps an install's files for an update's while Katana is not running, and puts them back if the new version does not start (`docs/update.md`); it depends on the update layer and core alone |
| `katana_updater_test`, `katana_updater_stub`, `katana_update_e2e` | `katana_updater` | what the update tests run and never ship: the helper and the engine with a TEST key in place of the pinned ones, and a stand-in for Katana the helper starts |
| `katana_make_icons`, `katana_tool_icon_sheet` | `katana_window` | developer tools: the icon files the window and the installer carry, committed (`docs/desktop.md`), and every catalogue tool's icon with its name (`docs/tools.md`) |

They land in `<cargo target>/release`: `rust/target/release` in a Windows
checkout, `~/katana-build/<worktree>/cargo/release` in WSL. Only the first
four are packaged, by name (`docs/release.md`, "Packaging").

## What the build reads from its environment

The crates' build scripts take a few values from cargo's environment, where
the C++ build took them as CMake options:

| Variable | Read by | Default | Effect |
|---|---|---|---|
| `KATANA_UPDATE_REPO` | `rust/crates/katana_update/build.rs` | `kjarada/Katana` | `owner/name` of the GitHub repository whose latest release an installed copy asks for updates |
| `KATANA_UPDATE_FEED` | the same | empty | the base URL of a release host of the owner's own, in place of GitHub; it must honour `Range`. With both empty the build has no update feed |
| `KATANA_REQUIRE_UPDATE_KEYS` | the same | unset | `ON` (CMake's truth: ON, YES, TRUE, Y, a non-zero number) fails the build while `cmake/KatanaUpdateKeys.cmake` holds the placeholder keys; set in every release job |
| `KATANA_ALTERNATE_CUSTOMISATIONS` | `rust/crates/katana_cad/build.rs` | `resources/customisation/nsw.customisation.json` | a `;`-separated list of further customisation files compiled in, offered by name; one that is no file is not offered |
| `KATANA_REQUIRE_BUILTIN_CUSTOMISATION` | the same | unset | `ON` fails the build when the default built-in is absent: a guard for a release job against the tracked file being deleted |
| `KATANA_TOOLCHAIN` | the build scripts of geodesy, gis, pointcloud and storage | unset | the conda-forge prefix: its `lib/` as a link search path and, on Linux, its compilers for the shims |
| `CC`, `CXX`, `AR` | `cc`, in gis and pointcloud | the toolchain's | the shims' compilers, where another is wanted |

A value cargo is given reaches the program; a value cargo is not given is
not an error - the program is built without it, and no program says which
repository it asks. That is why a release's packaging reads back what the
build script was given (`docs/release.md`, "Packaging").

The version is `project(Katana VERSION ...)` of the root `CMakeLists.txt`,
read by `rust/crates/katana_update/build.rs`: the one place a version is
written, which the release workflow and the packaging read too. The crates'
own `version` in `rust/Cargo.toml` is `0.0.0` and is not the product's.

## Running

### The desktop application

```sh
~/katana-build/<worktree>/cargo/release/katana [project-dir] [file-to-import ...] [switches]
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
default, are offered beside it by name. `rust/crates/katana_cad/build.rs`
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

The C++ build's cache logic that moved a stale default once
(`KatanaBuiltInDefault.cmake` and its test) went with the C++: cargo keeps no
cache of the choice, so there is nothing to move.

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
  `rust/crates/katana_update/build.rs` reads the same block, holds it to the
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
  of it imports only the system's DLLs is the release job's to show (step 8).
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

Cargo's target folder is not made self-contained, as the C++ build tree was
(its `katana_runtime` target deployed the DLLs beside the programs after
every link): a developer runs the programs where the toolchain is - in WSL
through their run-time path, on Windows with MSYS2 on `PATH`, which the
runner puts first on every test's (`KATANA_RUNTIME_BIN`) - and the bundle is
the tree that runs on its own.

`bin/` sits beside `share/` because that is where the data-hungry libraries
look: PROJ finds `proj.db` at `<its library>/../share/proj` by itself, and
`rust/crates/katana_core/src/library_data.rs` finds GDAL's data and libcurl's
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
built one much longer, so when the port is built on Windows the checkout, its
worktrees, the toolchain (`C:\msys64`) and cargo's folders are what IT could
exclude, or a Dev Drive used for the checkouts.

## Continuous integration

`.github/workflows/release.yml`, started by hand, builds Windows and Linux on
x86-64 and ARM64 and macOS on Apple silicon with cargo, runs cargo's tests
and the runner's whole suite on each, packages and smoke-tests the packages,
and publishes a GitHub release when given a tag (`docs/release.md`). There is
no CI on a push: the release workflow and the runs of
`tools/build_visible.sh` are the whole gate. The workflow in its Rust form has
not been run (`docs/release.md`, "Not done").
