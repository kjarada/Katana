<!-- Katana plan, section 6 of 47. Index: ../PLAN.MD. Previous: 05-development-strategy.md. Next: 07-phase-02-mathematical-foundation.md -->

# 6. Phase 01 — Build System

**STATUS: DELIVERED.**

CMake 3.30+ / Ninja / C++26, one target per module. (This line said 3.24 and
C++20. The project has been C++26 since 6095a19, and `cxx_std_26` is a compile
feature CMake first knows in 3.30; TEST_LAUNCHER honoured by
gtest_discover_tests needs 3.29 and preset schema 6 needs 3.25.) `-Wall -Wextra -Wpedantic
-Wshadow -Werror`, and `-ffp-contract=off -fno-fast-math` for Rule 7.

* `CMakePresets.json` - debug, release, relwithdebinfo, sanitize, tidy, and the
  two Linux presets.
* `cmake/KatanaSanitizers.cmake` - ASan + UBSan on GCC/Clang and MSVC. MinGW
  ships no libsanitizer, so it falls back to UBSan trap mode; CI runs the real
  sanitizers on Linux.
* `cmake/KatanaStaticAnalysis.cmake` - clang-tidy and cppcheck as compiler
  launchers, so a finding fails the build.
* `cmake/KatanaThirdParty.cmake` - GoogleTest and Google Benchmark cached under
  `third_party/_cache`, so extra build directories configure offline.
* **Where things are built.** Everything a person runs lands in `<build>/bin`:
  `katana.exe` (the application - the target is `katana`, not the old
  `katana_qt_app`) and `katana_cli.exe`, with tests in `bin/tests` and
  benchmarks in `bin/benchmarks` so that `bin` itself holds only what ships.
  CMake's default had put the application at
  `build/release/src/katana_qt/katana_qt_app.exe`, a path that read as though
  the executable were in the sources.
* `cmake/KatanaPackaging.cmake` - `cmake --install`, a `bundle` target
  (`<build>/dist/Katana`) and CPack (`package`: a ZIP, plus an NSIS installer
  when `makensis` is installed). The installed tree is SELF-CONTAINED:
  `windeployqt` brings Qt and its plugins, a dependency scan over the
  executables AND the deployed plugins brings the other 184 DLLs, and
  `share/proj` and `share/gdal` carry the data files. Verified by running both
  bundled programs with `PATH` reduced to the Windows directories: the GUI
  executable plots a PDF headlessly and the CLI exports a DXF. It carries
  `qoffscreen.dll`, which windeployqt omits, because `--plot` is a headless
  feature and without that plugin Qt does not fail - it opens a modal box and
  waits for ever. Record in `docs/architecture.md`.
* **The build tree is self-contained too (2026-09-23, at the owner's
  request that GDAL and PDAL be part of the program rather than borrowed from
  MSYS2).** The `katana_runtime` target runs the bundle's own deploy script
  into `<build>/bin` and `<build>/share` after `katana` and `katana_cli` link,
  so `build/release/bin/katana.exe` runs with no MSYS2 on `PATH` - verified
  with `PATH` reduced to the Windows directories: the CLI imported LAS,
  ASCII-grid and GeoJSON data and exported DXF, and the GUI built its window.
  Incremental: a full deploy (36 s idle, 62-85 s under load, Release) runs
  once per configuration or toolchain update, and every other build checks
  timestamps in 0-1 s. `-DKATANA_DEPLOY_RUNTIME=OFF` turns it off.
  **Static linking was examined and NOT done**: MSYS2 has no static PDAL or
  PROJ, and `libpdalcpp` itself imports the GDAL DLL, so GDAL linked in
  statically would put two GDALs in one process. A fully static Katana means
  building GDAL, PROJ, PDAL and their dependencies from source - with a
  trimmed driver set, which would also answer the 388 MB below - and GEOS's
  LGPL then asks for relinkable objects. That is OUTSTANDING as a packaging
  project of its own; the reasoning is in `docs/architecture.md`.

* **The executable is branded.** `katana.rc.in` embeds the application icon
  and a `VERSIONINFO` block (version from `project(... VERSION ...)`, written
  once). The icon is painted by the application's own icon code and written to
  `resources/katana.ico` by the `katana_make_icons` developer tool; it is
  committed rather than generated during the build, for the reason recorded in
  `src/katana_qt/make_icons.cpp`. Warning flags in `KatanaTargetDefaults` are
  now guarded to C++, because a target's options reach every language on it
  and `windres` rejects `-Wall`.

**OUTSTANDING:** the bundle is 388 MB, nearly all of it the dependency chain of
GDAL and PDAL as MSYS2 builds them (HDF5, netCDF, Poppler, ...); trimming it
means building GDAL with fewer drivers, which is a packaging project of its
own. There is no installer unless NSIS is installed, no code signing, and no
Linux or macOS bundle.
* `tools/check_layering.cmake` - machine-enforces section 2 and Rule 4: a layer
  may only include from its allow-list, and no public header may include a
  third-party header. Runs as the `layering` CTest case.
* `.github/workflows/ci.yml` - Debug and Release tests, sanitizers, static
  analysis, layering, and a Windows MSYS2 job. **CORRECTED (2026-09-23): this
  file was DELETED** in 759c115, a Qt bug fix whose message does not mention it,
  and no CI had run since; `.github/lsan.supp` was left orphaned and later
  commits still justified designs by "the sanitizer job". Being restored.

Test discovery runs through a `TEST_LAUNCHER`, so the suite does not depend on
the ambient PATH.

---

