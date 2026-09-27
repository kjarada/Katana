# Releases

How Katana is built for Windows, Linux and macOS - on x86-64 and on ARM64 -
tested, and published as a GitHub release, how the packages are signed, and why they
are made the way they are. How to build on one machine is
`docs/building.md`; this is all of them together.

## Publishing a release

The workflow is `.github/workflows/release.yml`. It runs ONLY when started by
hand - a commit, a tag or a pull request starts nothing (the owner's
decision, 2026-09-26: a build of five platforms is hours of runner time,
spent when a release is wanted and not on every push). It builds the five
packages, runs the whole test suite on each platform, starts each package
from what a user downloads and each installer silently, signs what it has
keys for, and publishes them.

1. Set the version in the root `CMakeLists.txt`, `project(Katana VERSION X.Y.Z)`,
   and merge it to `main`.
2. On GitHub: Actions > Release > Run workflow, on `main`, with the tag
   `vX.Y.Z`. The release, and its tag, are made on the commit the run started
   from. With the tag left empty the run builds and tests the packages and
   publishes nothing - the way to try a change to the build.
3. The run takes as long as its slowest job (Windows x86-64: build, the
   whole suite, the installer). When all five pass, release `vX.Y.Z` appears
   with:

| File | What it is |
|---|---|
| `Katana-X.Y.Z-win64.exe` | Windows x86-64 installer (NSIS), with an uninstaller |
| `Katana-X.Y.Z-win64.zip` | the same tree to unzip anywhere; run `bin/katana.exe` |
| `Katana-X.Y.Z-win-arm64.exe` | Windows 11 on ARM64 installer (NSIS), with an uninstaller |
| `Katana-X.Y.Z-win-arm64.zip` | the same tree to unzip anywhere |
| `Katana-X.Y.Z-linux-x86_64.tar.gz` | Linux x86-64, glibc 2.28 or later; run `bin/katana` |
| `Katana-X.Y.Z-linux-aarch64.tar.gz` | Linux on 64-bit ARM, glibc 2.28 or later |
| `Katana-X.Y.Z-macos-arm64.dmg` | macOS 13 or later on Apple silicon: `Katana.app` to drag into Applications |
| `SHA256SUMS.txt` | the SHA-256 of each file above |
| `*.asc`, `katana-signing-key.asc` | OpenPGP signatures and the public key, when the repository holds the key ("Signing") |

Each Windows and Linux archive holds `bin/katana`, `bin/katana_cli` and
`bin/katana_mcp`, the libraries they load, and `share/katana/samples`; each
disk image holds `Katana.app`, with the same programs in `Contents/MacOS`
("macOS"). How a user installs each is the root readme's job, not this
document's.

A tag that does not match `project(Katana VERSION ...)` fails the run in its
first job, before any building: the packages are named from the CMake
version, so `v0.3.0` over a 0.2.0 build would publish files that say 0.2.0.
Re-running a run whose release already exists replaces its files, so a
failed publish is fixed by re-running it.

## What each job does

Each job runs the commands `docs/building.md` gives for its platform, with
`KATANA_BUILD_TESTS` on and `KATANA_BUILD_BENCHMARKS` off, then the whole
suite (`ctest`), then `--target package`. Nothing in the workflow is a second
way of building.

| Job | Runner | Toolchain | Configure |
|---|---|---|---|
| Windows x86-64 | `windows-2025` | MSYS2 UCRT64, GCC 16 | `cmake --preset release`, with the compiler and prefix where the action installed MSYS2 |
| Windows ARM64 | `windows-11-arm` | MSYS2 CLANGARM64, clang and libc++; NSIS from Chocolatey | the same, with `clang++` and `-DKATANA_MAKENSIS` |
| Linux x86-64 | `ubuntu-24.04` | `tools/setup_linux_toolchain.py`, GCC 16.2 | `cmake --preset linux-release` |
| Linux ARM64 | `ubuntu-24.04-arm` | the same script, which installs conda-forge's `linux-aarch64` GCC 16.2 on an ARM machine | `cmake --preset linux-release` |
| macOS arm64 | `macos-15` | the same script, conda-forge's `osx-arm64` clang 23 | `cmake --preset macos-release` |

The ARM64 jobs build natively on GitHub's ARM runners, which private
repositories have had since 2026-01-29, rather than cross-compiling: the
package is then smoke-tested on the machine kind it is for, with nothing
emulated. MSYS2 has no GCC for ARM64 Windows; its CLANGARM64 environment is
the only one with Qt, GDAL and PDAL for it, so that build is clang's, which
the macOS build already requires the code to accept.

The Linux and macOS toolchain prefixes are cached under a key that is the
hash of `tools/setup_linux_toolchain.py` and the runner's system and
architecture: its `SNAPSHOT` and `SPECS` fix every version. MSYS2 is not
frozen: it installs whatever the rolling distribution has that day, so a
Windows release records the compiler's version in its log (the "Toolchain"
step).

### The whole suite, on the machine the package is for

Before packaging, every job builds the tests and runs all of them, in
parallel, with `QT_QPA_PLATFORM=offscreen`; a single failure stops the job,
so nothing is packaged from a build with a failing test. On Linux the suite
runs under Xvfb with Mesa's software Vulkan driver installed, so the `gpu.`
suite exercises the Vulkan renderer rather than skipping.

Until 2026-09-27 the jobs ran only the part of the suite that meets hardware
nowhere else (`simd_` on the ARM machines, `gpu.` on macOS), and the release's
question was only whether each package was complete. The whole suite is the
stronger gate at the cost of runner time: a release is rare (it is started by
hand), and the five machines are the only place four of the platforms'
suites run at all. Those runners remain the only place some code meets real
hardware:

| Job | What only it proves |
|---|---|
| macOS arm64 | the Metal renderer against the software one on a real GPU |
| Linux ARM64, Windows ARM64, macOS arm64 | the NEON kernels at both levels, natively (built on x86-64 they run only under qemu, `docs/building.md`) |

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

Each run keeps the window's picture as an artifact, `screenshot-<platform>`, to
look at; it is not published. On Linux and macOS the toolchain prefix is
MOVED AWAY for the test, so that a
library the package failed to carry fails to load there rather than on a
user's machine. On Windows `PATH` is cut to the Windows directories.

On macOS the disk image is mounted, its link to `/Applications` checked,
and `Katana.app` copied out of it as a user drags it; `Info.plist` must parse,
the icon must be there, and `codesign --verify --deep --strict` must accept
the bundle's seal before the programs in `Contents/MacOS` run.

**The installers.** Each Windows installer is then run as an administrator's
deployment runs it: `/S /D=<folder>`, silently, into a folder of its own. The
installed `katana_cli` must name `EPSG:32630` as above, and the uninstaller
(`Uninstall.exe /S`, kept in place with `_?=` so the step waits for it) must
remove `bin\katana_cli.exe`.

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

On macOS the same tree is `Katana.app` ("macOS", below): the layout inside
`Contents` keeps the same relative paths under Apple's names.

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
  GPU renderer reads them (a build with `KATANA_GPU=OFF` has none).
* Two missing standard includes (`<span>`, `<numbers>`) that libstdc++ had
  supplied through other headers.
* Four thresholds and helpers read only by the AVX2 kernels' call sites
  (`kValidateMinimum`, `kProjectMinimum`, `kTransformBatchMinimum`,
  `kernelParams`) went unused on arm64, which has no kernels; they are
  `[[maybe_unused]]`. A GCC build with `KATANA_SIMD_KERNELS=OFF` failed on
  `kernelParams` the same way.

`katana` is not a `MACOSX_BUNDLE` in the build tree: there it is
`bin/katana`, as on Linux, so the headless checks and the tests find it where
they find it everywhere else. The bundle is made when installing.

**Katana.app.** `cmake --install` (and so `package` and `bundle`) lays the
tree out as an application bundle, because code signing seals a bundle only
when code and data are where it expects them, and Finder shows a bundle as
one application with its icon:

| In the bundle | What | Stands for, in the Linux tree |
|---|---|---|
| `Contents/Info.plist` | the executable (`katana`), the icon, the identifier `com.jarada.katana`, the minimum macOS (`cmake/KatanaInfo.plist.in`) | - |
| `Contents/MacOS/` | `katana`, `katana_cli`, `katana_mcp`; RPATH `@loader_path/../Frameworks` | `bin/` |
| `Contents/Frameworks/` | the libraries, and Qt's plugins in `Frameworks/qt6/plugins` | `lib/` |
| `Contents/Resources/qt.conf` | `Plugins=Frameworks/qt6/plugins`; in a bundle Qt reads `qt.conf` here and resolves it against `Contents` | `bin/qt.conf` |
| `Contents/Resources/share/`, `ssl/` | GDAL, PROJ and certificate data; the samples | `share/`, `ssl/` |
| `Contents/Resources/katana.icns` | the icon, made from `resources/katana.png` by `sips` and `iconutil` at install time | - |

`Frameworks` mirrors `lib/`, so every conda-forge library and plugin still
finds the others through its own relative RPATH; only the data moved, and
`core::dataBesideLibraryFile` looks in `Resources/` for a library in
`Frameworks/`. Putting the data in `Contents/share` beside `Frameworks`
would have needed no code change, but codesign expects everything that is
not code under `Resources`, and notarisation checks the seal. Moving Qt's
plugins to the conventional `Contents/PlugIns` was rejected: each plugin's
RPATH (`@loader_path/../../..`) would then miss `Frameworks`, and rewriting
them breaks their signatures.

After installing, every library, then `katana_cli` and `katana_mcp`, then the
bundle (which signs `katana` with `Info.plist` bound to it) are signed ad
hoc, so `codesign --verify --deep --strict` accepts it. CPack's DragNDrop
generator makes the disk image, with a link to `/Applications` beside the
app.

**Apple silicon only.** Intel Macs are not a target (the owner's decision,
2026-09-27). An Intel build was tried the same day - conda-forge has the whole
library set for `osx-64`, and GitHub's `macos-15-intel` runner could build it
natively - and removed before it shipped. Adding it back is a matrix entry in
the workflow and a host check in `cmake/toolchains/katana-macos.cmake` and the
setup script choosing the `osx-64` compiler.

The minimum is macOS 13 (`CMAKE_OSX_DEPLOYMENT_TARGET` in
`cmake/toolchains/katana-macos.cmake`), the oldest Qt 6.11 supports. libc++'s
availability markup is switched off (`_LIBCPP_DISABLE_AVAILABILITY`): it
refuses what the deployment target's SYSTEM libc++ lacks - the first CI build
stopped at floating-point `std::from_chars`, "introduced in macOS 26.0" - but
the programs load the libc++ the package carries, never the system's. This is
conda-forge's documented remedy for the same situation.

The GPU renderer draws on Metal there, from the same GLSL baked to Metal
Shading Language (`docs/gpu.md`, "Shaders"); the macOS job runs the `gpu.`
test suite on the runner before packaging.

**Gatekeeper.** Without a Developer ID ("Signing") the app is signed ad hoc
(`codesign --sign -`, which Apple silicon requires of any code, and which
`cmake --install` must redo after rewriting the programs' RPATH) and is not
notarised. A copy downloaded through a browser is quarantined, and macOS
refuses to open it until the quarantine is cleared once after copying it to
Applications:

```sh
xattr -dr com.apple.quarantine /Applications/Katana.app
```

## Signing

Every package names its publisher, `KATANA_PUBLISHER` in the root
`CMakeLists.txt` (default "Jarada", the owner's choice): the company and
copyright in `katana.exe`'s version resource, and the installer's publisher
in Settings > Apps. That costs nothing and needs no key.

A SIGNATURE under that name is another matter. It is worth something only
when the operating system already trusts the key: Windows trusts a
code-signing certificate bought from a certificate authority, macOS an Apple
Developer ID (with the program notarised), and for OpenPGP a user trusts the
key they imported. A self-signed certificate reading "Jarada" would change
nothing a user sees - SmartScreen and Gatekeeper treat it as unsigned - so the
build never makes one. Instead it signs with the keys the repository holds,
and with none it publishes unsigned, as before.

`cmake/KatanaSign.cmake` does the signing, run by CPack between staging the
tree and archiving it (so the programs inside the ZIP, the tarball and the
installer are signed) and again after (so the installer is). It reads the key
from the environment, so a person signs a local package the same way:

| Platform | Environment | Signs |
|---|---|---|
| Windows | `KATANA_SIGN_PFX` (a .pfx), `KATANA_SIGN_PASSWORD` | `katana.exe`, `katana_cli.exe`, `katana_mcp.exe` and the installer, Authenticode SHA-256, time-stamped, with `osslsigncode` (or `signtool`) |
| macOS | `KATANA_SIGN_IDENTITY` (a keychain identity) | every library and plugin, then the programs, then `Katana.app`, with the hardened runtime, time-stamped; afterwards the `.dmg` |

The workflow fills those from repository secrets (Settings > Secrets and
variables > Actions), and adds what CPack cannot do:

| Secret | What it is | Used for |
|---|---|---|
| `WINDOWS_CERTIFICATE` | the code-signing certificate's `.pfx`, base64 (`base64 -w0 cert.pfx`) | Authenticode |
| `WINDOWS_CERTIFICATE_PASSWORD` | its password | |
| `MACOS_CERTIFICATE` | the "Developer ID Application" certificate and key exported from Keychain Access as `.p12`, base64 | codesign |
| `MACOS_CERTIFICATE_PASSWORD` | the `.p12`'s password | |
| `MACOS_SIGN_IDENTITY` | its name, `Developer ID Application: Jarada (TEAMID)` | |
| `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID` | the Apple account, an app-specific password for it, and the team | notarisation (`xcrun notarytool`) |
| `GPG_PRIVATE_KEY`, `GPG_PASSPHRASE` | an armoured OpenPGP secret key and its passphrase | a `.asc` beside every published file, and `katana-signing-key.asc` |

An OpenPGP key under the publisher's name is the one of these anyone can make
for nothing:

```sh
gpg --quick-generate-key "Jarada <address@example.com>" ed25519 sign 3y
gpg --armor --export-secret-keys "Jarada" > jarada-signing.asc   # the GPG_PRIVATE_KEY secret
```

The disk image is notarised as it is, and the ticket stapled to it
(`xcrun stapler staple`), so Gatekeeper accepts `Katana.app` copied out of it
without asking Apple's service, even offline.

## Not done

* **No CI on every change.** The whole suite runs in the release workflow,
  which is started by hand; a change is still gated by running the suite
  locally (`docs/testing.md`, audit BLD-01).
* **Signing needs keys the repository does not yet hold.** Until the
  secrets in "Signing" are added, releases carry checksums only: SmartScreen
  warns on Windows and macOS quarantines the download.
* **The Windows ARM64 installer's own code is x86**, run emulated: NSIS has no
  ARM64 build. The programs it installs are ARM64.
* **No AppImage, `.deb` or Flatpak.** The Linux package is a tarball.
* **The Windows toolchain is not frozen.** MSYS2 installs what it has on the
  day; two runs a month apart may build with different GCC and Qt versions.
