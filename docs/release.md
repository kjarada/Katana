# Releases

How Katana is built for Windows, Linux and macOS and published as a GitHub
release, and why the packages are made the way they are. How to build on one
machine is `docs/building.md`; this is the three together.

## Publishing a release

The workflow is `.github/workflows/release.yml`. It builds the three
packages, starts each one from its unpacked archive, and publishes them.

1. Set the version in the root `CMakeLists.txt`, `project(Katana VERSION X.Y.Z)`,
   and merge it to `main`.
2. Tag that commit and push the tag:

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

   Or, on GitHub, Actions > Release > Run workflow, with the tag `vX.Y.Z`: the
   release is made on the commit the run was started from, and the tag is
   created with it.
3. The run takes the longest of the three builds (Windows, with the NSIS
   installer, is the slowest). When all three pass, release `vX.Y.Z` appears
   with:

| File | What it is |
|---|---|
| `Katana-X.Y.Z-win64.zip` | Windows x86-64: unzip anywhere, run `bin/katana.exe` |
| `Katana-X.Y.Z-win64.exe` | the same tree as an installer (NSIS), with an uninstaller |
| `Katana-X.Y.Z-linux-x86_64.tar.gz` | Linux x86-64, glibc 2.28 or later: unpack, run `bin/katana` |
| `Katana-X.Y.Z-macos-arm64.tar.gz` | macOS 13.3 or later on Apple silicon: unpack, run `bin/katana` |
| `SHA256SUMS.txt` | the SHA-256 of each file above |

Each archive holds `bin/katana`, `bin/katana_cli` and `bin/katana_mcp`, the
libraries they load, and `share/katana/samples`.

A tag that does not match `project(Katana VERSION ...)` fails the run in its
first job, before any building: the packages are named from the CMake
version, so `v0.3.0` over a 0.2.0 build would publish files that say 0.2.0.
Re-running a run whose release already exists replaces its files, so a
failed publish is fixed by re-running it.

The same workflow runs, without publishing, on every pull request that
changes the build: `CMakeLists.txt`, `CMakePresets.json`, `cmake/`,
`tools/setup_linux_toolchain.py` or the workflow itself. A run started from
the Actions tab with no tag also builds without publishing.

## What each job does

Each job runs the commands `docs/building.md` gives for its platform, with
`KATANA_BUILD_TESTS` and `KATANA_BUILD_BENCHMARKS` off, then
`--target package`. Nothing in the workflow is a second way of building.

| Job | Runner | Toolchain | Configure |
|---|---|---|---|
| Windows x86-64 | `windows-2025` | MSYS2 UCRT64 through `msys2/setup-msys2`, GCC 16 | `cmake --preset release`, with the compiler and prefix where the action installed MSYS2 |
| Linux x86-64 | `ubuntu-24.04` | `tools/setup_linux_toolchain.py`, GCC 16.2 | `cmake --preset linux-release` |
| macOS arm64 | `macos-15` | `tools/setup_linux_toolchain.py`, clang 23 | `cmake --preset macos-release` |

The Linux and macOS toolchain prefixes are cached under a key that is the
hash of `tools/setup_linux_toolchain.py`: its `SNAPSHOT` and `SPECS` fix every
version, so the script is the whole of what the prefix depends on. MSYS2 is
not frozen: it installs whatever the rolling distribution has that day, so a
Windows release records `g++ --version` in its log (the "Toolchain" step).

### The smoke test

Each package is unpacked into a fresh directory and run from there, as a
user would run it:

* `katana_cli -c "RECT 0,0 30,20" -c "SAVE ..."` must write a project;
* `INFO samples/gis/terrain.asc` must name its coordinate system as
  `EPSG:32630`: GDAL reads the `.prj` and PROJ names it from `proj.db`, so this
  fails unless the packaged PROJ data is found;
* `IMPORT` of `samples/gis/survey_scan.las` (PDAL) and `parcels.geojson`
  (GDAL's vector drivers) must succeed;
* `katana <project> --screenshot out.png`, offscreen, must write the window's
  picture - Qt, its platform plugin and the whole start-up.

On Linux and macOS the toolchain prefix is MOVED AWAY for the test, so that a
library the package failed to carry fails to load there rather than on a
user's machine. On Windows `PATH` is cut to the Windows directories.

The tests do not run in this workflow. They are the gate before a change is
merged (`docs/testing.md`); the release job's question is only whether the
package is complete. See "Not done".

## How the Linux and macOS packages are made self-contained

The Windows package has been self-contained since 2026-09-23
(`docs/building.md`, "Bundling"). Linux and macOS had no package at all: their
programs found the toolchain prefix's libraries through an RPATH naming that
prefix, so they ran only on a machine that had it. No distribution ships the
libraries Katana needs (GCC 15 or later, CGAL 6, PROJ 9.5), so "let the
package manager supply them" has nothing to supply them with.

`cmake/KatanaDeployUnix.cmake.in`, run by `cmake --install` when the build
uses a toolchain prefix, copies them in. The installed tree MIRRORS the
prefix - `bin/`, `lib/`, `lib/qt6/plugins/`, `share/`, `ssl/` - and that is
the whole trick: conda-forge builds every library with an RPATH relative to
itself (`$ORIGIN` on Linux, `@loader_path` on macOS) and every Qt plugin with
`$ORIGIN/../../..`, so in the same layout they find one another with nothing
rewritten. Only Katana's own programs change, from the prefix's `lib/` to
`$ORIGIN/../lib` (`CMAKE_INSTALL_RPATH` in `cmake/toolchains/`). Rewriting
each library's RPATH (what `patchelf`-based tools do) was the alternative; it
rewrites every bundled library though none needs it and, on macOS, breaks each
one's code signature.

Three kinds of file are found by a path compiled into a library for the
prefix it was built in, which the user's machine does not have. Each is now
looked for beside the library instead, as PROJ's data already was on Linux
(`include/katana/core/library_data.hpp`):

| Library | Reads | From, in the package |
|---|---|---|
| PROJ | `proj.db` and grids | `share/proj`, beside `lib/libproj*` (`projDataDirectory`) |
| GDAL | `header.dxf` and its other support files | `share/gdal`, beside `lib/libgdal*` (`locateGdalData`, `gdal_adapter.cpp`) |
| libcurl, through GDAL | the certificates `https://` is checked against | `ssl/cacert.pem`, beside `lib/libcurl*` (`locateCertificates`, `gdal_adapter.cpp`) |

Each yields to the user's own setting (`PROJ_DATA`, `GDAL_DATA`,
`CURL_CA_BUNDLE`, `SSL_CERT_FILE`), and does nothing for a distribution's
library, which has no such directory beside it.

Qt's plugins are copied from the prefix into `lib/qt6/plugins`, and
`bin/qt.conf` points Qt at them; without it Qt asks the prefix compiled into
it, the build machine's.

**What stays the system's, on Linux.** The C library, the GL, EGL and Vulkan
loaders (the graphics driver's), X11's `libX11` and `libxcb`, `fontconfig` and
`freetype`. The first two cannot be shipped; fontconfig reads its
configuration through a path compiled in for the prefix, so a shipped copy
finds no `fonts.conf` and Qt draws no text. The list follows AppImage's
excludelist, for the same reasons. So a Linux machine needs, beyond glibc
2.28: `libGL`, `libEGL`, `libfontconfig`, `libfreetype`, `libX11` and `libxcb`
(Debian and Ubuntu: `libgl1 libegl1 libfontconfig1 libfreetype6 libx11-6
libxcb1`), which every desktop has.

**Why glibc 2.28.** The programs are linked against the sysroot the
toolchain installs, and the solver used to take the newest (2.39, Ubuntu
24.04's), which would have made the Linux package refuse to start on
anything older. `tools/setup_linux_toolchain.py` pins `sysroot_linux-64 2.28`:
Debian 10, Ubuntu 20.04 and RHEL 8.

## macOS

macOS is built with conda-forge's clang 23 and libc++, not GCC, because every
C++ library conda-forge ships for macOS is built against libc++ and GCC's
libstdc++ cannot link with them: PDAL's API passes `std::string`. Homebrew was
the other candidate and fails the same way (its Qt, GDAL and PDAL are also
libc++), besides not being frozen at a date.

Making the code build with clang took small changes, found by compiling every
source with clang 21 and libc++ (`-fsyntax-only`, the project's own flags,
on Linux) before any Mac was involved: 21 of 872 translation units failed.

* `#embed`'s warning is `-Wc23-extensions` in clang and `-Wc++26-extensions`
  in GCC; the three files that embed pick by compiler.
* `core/simd.hpp` gained a third backend: libc++ has neither `<simd>` nor the
  Parallelism TS (it hides the TS behind `-fexperimental-library`), so there
  a `Vec<T>` is a plain array of 16 bytes' worth of lanes with the same names
  and lane-by-lane semantics; `test_simd_facade.cpp` holds all three to the
  same cases.
* The Style Manager's `connectGuarded` called a slot with no arguments in a
  form clang checks when the function template is instantiated, not when the
  call is; the call moved into a helper whose arguments make it dependent.
* clang warns on lambda captures and private fields that are never used where
  GCC does not; they were removed, or marked `[[maybe_unused]]` where only the
  GPU renderer, which the macOS build does not have, reads them.
* Two missing standard includes (`<span>`, `<numbers>`) that libstdc++ had
  supplied through other headers.

`katana` is no longer a `MACOSX_BUNDLE`: on macOS it is `bin/katana`, as on
Linux, so the headless checks, the tests and the package find it where they
find it everywhere else.

The minimum is macOS 13.3 (`CMAKE_OSX_DEPLOYMENT_TARGET` in
`cmake/toolchains/katana-macos.cmake`): the first whose libc++ availability
markup allows floating-point `std::to_chars` and `std::from_chars`. The markup
is checked when compiling even though the libc++ that runs is the packaged
one.

The GPU renderer is not built on macOS (`src/katana_qt/gpu/CMakeLists.txt`:
its shaders exist for Direct3D 11 and Vulkan only); the 3D view uses the
software rasteriser there.

**Gatekeeper.** The package is signed ad hoc (`codesign --sign -`, which
Apple silicon requires of any code, and which `cmake --install` must redo
after rewriting the programs' RPATH), not with a Developer ID, and it is not
notarised. A copy downloaded through a browser is quarantined, and macOS
refuses to open it. Clear the quarantine once after unpacking:

```sh
xattr -dr com.apple.quarantine Katana-X.Y.Z-macos-arm64
```

## Not done

* **The tests do not run in the release workflow.** It checks that each
  package is complete, not that the code is correct; the suite is still run by
  hand (`docs/testing.md`), and there is still no CI for it (audit BLD-01).
  Running `ctest` in the Linux and macOS jobs would add most of an hour to
  each, and the macOS suite has never been run at all.
* **No signed or notarised macOS build, and no `Katana.app`.** Both need an
  Apple Developer ID and its certificate as a repository secret. With them, a
  `.app` whose `Contents/` holds this same tree, signed and notarised, and a
  `.dmg` around it, is a packaging step of its own.
* **No Intel Mac build.** GitHub's Intel macOS runners are being retired; the
  toolchain script would need `clangxx_osx-64` and an `x86_64` runner.
* **No signed Windows installer.** The NSIS installer is unsigned, so
  SmartScreen warns on first run.
* **No AppImage, `.deb` or Flatpak.** The Linux package is a tarball.
* **The Windows toolchain is not frozen.** MSYS2 installs what it has on the
  day; two runs a month apart may build with different GCC and Qt versions.
