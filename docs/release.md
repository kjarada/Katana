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
keys for, makes the update assets an installed Katana reads to find and
fetch the next version ("Update assets"), and publishes them.

Once, before the first release, the owner makes the update keys and pins
their public halves ("Update signing"). Every job configures with
`-DKATANA_REQUIRE_UPDATE_KEYS=ON` and the tree holds placeholder keys until
that is done, so until then every run stops in its first job, a run without
a tag included.

1. Set the version in the root `CMakeLists.txt`, `project(Katana VERSION X.Y.Z)`,
   and merge it to `main`.
2. On GitHub: Actions > Release > Run workflow, on `main`, with the tag
   `vX.Y.Z`. The release, and its tag, are made on the commit the run started
   from. With the tag left empty the run builds and tests the packages and
   publishes nothing - the way to try a change to the build. The other two
   inputs are usually left as they are:

   | Input | Default | What it does |
   |---|---|---|
   | `tag` | empty | the release to publish; empty builds, tests and publishes nothing |
   | `sign` | `ci` | who signs the update manifests. `ci`: this run, with the secret `UPDATE_SIGNING_KEY`. `offline`: nobody yet - the release is left a DRAFT until the owner has signed it with `tools/update_release_sign.py` ("Update signing") |
   | `publish_repo` | empty | `owner/name` of another repository to publish the release in, with the repository secret `RELEASES_REPO_TOKEN`; empty is this repository ("Update signing", at its end) |

3. The Publish job runs in the GitHub environment `release`, which holds the
   update signing key. When that environment names required reviewers, the
   job waits until one of them approves it on the run's page.
4. The run takes as long as its slowest job (Windows x86-64: build, the
   whole suite, the installer). When all five pass and the Publish job has
   run, release `vX.Y.Z` appears, marked as the latest, with:

| File | What it is |
|---|---|
| `Katana-X.Y.Z-win64.exe` | Windows x86-64 installer (NSIS), with an uninstaller |
| `Katana-X.Y.Z-win64.zip` | the same tree to unzip anywhere; run `bin/katana.exe` |
| `Katana-X.Y.Z-win-arm64.exe` | Windows 11 on ARM64 installer (NSIS), with an uninstaller |
| `Katana-X.Y.Z-win-arm64.zip` | the same tree to unzip anywhere |
| `Katana-X.Y.Z-linux-x86_64.tar.gz` | Linux x86-64, glibc 2.28 or later; run `bin/katana` |
| `Katana-X.Y.Z-linux-aarch64.tar.gz` | Linux on 64-bit ARM, glibc 2.28 or later |
| `Katana-X.Y.Z-macos-arm64.dmg` | macOS 13 or later on Apple silicon: `Katana.app` to drag into Applications |
| `Katana-update-<token>.json` | the update manifest of one target (`win64`, `win-arm64`, `linux-x86_64`, `linux-aarch64`, `macos-arm64`), under a name that is the same in every release: the version, and every file of that package with its SHA-256 ("Update assets"). The one for `macos-arm64` only announces the release |
| `Katana-update-<token>.json.sig` | that manifest's Ed25519 signature, one line `ed25519 <key id> <base64>` ("Update signing") |
| `Katana-X.Y.Z-<token>.update.zip` | the files of one of the first four targets, each compressed on its own, out of which an installed copy fetches the ones that changed; nobody downloads it whole. None for macOS |
| `SHA256SUMS.txt` | the SHA-256 of each file above |
| `*.asc`, `katana-signing-key.asc` | OpenPGP signatures and the public key, when the repository holds the key ("Signing") |

Each Windows and Linux archive holds `bin/katana`, `bin/katana_cli`,
`bin/katana_mcp` and `bin/katana_updater` (the helper that swaps an install's
files for an update's), the libraries they load, `share/katana/samples`, and
`share/katana/install.json`, which names the package's target and is what
makes a copy a release the updater will touch; each disk image holds
`Katana.app`, with the first three programs in `Contents/MacOS` ("macOS").
How a user installs each is the root readme's job, not this document's.

A tag that does not match `project(Katana VERSION ...)` fails the run in its
first job, before any building: the packages are named from the CMake
version, so `v0.3.0` over a 0.2.0 build would publish files that say 0.2.0.

Running the Publish job again for a release that already exists replaces
its files, removes any that the new run did not make, and marks the release
latest again, so a failed publish is fixed by re-running it ("Update assets"
says what an installed copy meets meanwhile). Run again by itself, from the
run's page, the job publishes the SAME packages under the same sequence, so
a copy that already has them is not offered them a second time; a whole new
run for the same tag builds new packages, with a later sequence, and those
are offered to the copies an update had already brought to that version (a
copy installed by hand keeps what it has until the next version; "Update
assets", `--sequence`). So a published build is replaced for those copies
by a NEW run, not by running one of its build jobs again: a job run again
keeps its run's sequence, and its new package would stand in the release
without being offered to a copy that took the old one. To take a bad
release out of
circulation, mark the one before it as the latest,
`gh release edit vPREVIOUS --latest`: installed copies read whichever release
is the latest, and are never offered a version below their own.

## Update assets

An installed Katana finds a newer version by reading two small files of the
latest release, and then fetches only the files that differ from its own
(`docs/update.md`; the addresses are built in
`include/katana/update/feed.hpp`):

```
https://github.com/<repo>/releases/latest/download/Katana-update-<token>.json.sig
https://github.com/<repo>/releases/latest/download/Katana-update-<token>.json
https://github.com/<repo>/releases/download/vX.Y.Z/Katana-X.Y.Z-<token>.update.zip
```

So the latest release IS the update feed, and the Publish job makes what it
serves, from the packages themselves, in its step "Update assets":

1. **Unpack** the four packages an update can be applied to file by file:
   `win64`, `win-arm64`, `linux-x86_64`, `linux-aarch64`. Each must hold a
   `share/katana/install.json` that names its own target, or the step stops:
   a manifest is offered to the installs whose record names its target.
2. **Produce.** `tools/update_make_manifest.py` runs once for each tree and
   writes its manifest and its update zip; once more, with `--notify-only`
   and no tree, for `macos-arm64`, whose manifest lists no file: a file
   changed inside `Katana.app` breaks the bundle's seal, so a Mac is told
   about a release and not updated.
3. **Sign.** With `sign` left at `ci` the same run of the producer writes
   each manifest's `.sig` ("Update signing").
4. **Recheck.** `tools/update_verify_release.py` reads the folder again with
   none of the producer's code: every hash recomputed from the zips, every
   file's byte range inflated on its own, every signature verified against
   the keys `cmake/KatanaUpdateKeys.cmake` pins - the keys the programs just
   built will verify with - and exactly the five targets present, agreeing
   on version, tag and sequence.
5. **Publish.** The names start `Katana-`, so the checksums and the OpenPGP
   signatures take the update assets as they take the packages, and ONE
   `gh release create ... --latest` call carries every file. `gh` creates the
   release as a draft, uploads, and only then publishes, so the release an
   installed copy finds as the latest is never one whose manifests are still
   on their way; and `--latest` is said rather than left to GitHub's guess
   from dates, because the latest release is the feed.

What the producer is told about a tree:

| Option | Value | Why |
|---|---|---|
| `--sequence` | the UTC time in seconds at which the run began, read once in the Version job: ONE value for all five manifests | it orders two BUILDS of one version, so it is taken when the building starts and not when the publishing does. A whole new run outranks an earlier one. The Publish job run again by itself gets the first job's value again, and so publishes the same files under the same sequence (for the same input the producer writes the same bytes, its zlib being the same). That matters because an update is offered when `(version, sequence)` is above what a copy recorded when it was last updated (`isNewerThan`, `include/katana/update/trust.hpp`): a sequence read at publishing would offer every such copy the files it already has, and have it restart to install none. The recheck refuses a release whose manifests disagree on it |
| `--seed` | `share/katana/samples/*` | a seed is written only where it is absent. Opening a sample project changes it in place, so as ordinary data every update would fetch the samples again and write over what the user had done in them |
| `--exclude` | `LICENSE`, and for Linux `ssl/*` | a manifest names files under `bin`, `share`, `etc` and `lib` and nowhere else, and the producer refuses a tree that holds anything more rather than leave it out unasked. The licence text lies beside those folders; the Linux tree keeps its certificate bundle in `ssl/`, where the bundled libcurl looks ("How the Linux and macOS packages are made self-contained"). Neither is ever updated |
| `--version`, `--tag`, `--repo` | what is being published, and where | what a manifest offers, and its link to the release notes |
| `--key` | the signing key, when the run signs | "Update signing" |

The seed and the excluded names are written once, at the top of the workflow
(`UPDATE_SEED`, `UPDATE_EXCLUDE`, `UPDATE_EXCLUDE_LINUX`), for the Publish job
and the rehearsal below alike. `share/katana/install.json` needs no option:
the producer leaves the install record out by itself.

**The Windows packages are unpacked on Linux,** where the execute bits a zip
made on Windows carries (its archiver's guess from the file names) become
real, and the producer would read them: as `exec` in the manifest, and as a
reason to class a file in `bin/` as a program. The step clears them, so a
Windows file is classed by its name alone (`.exe`, `.dll`), as the producer
classes it when it runs on Windows, where there is no such bit. This is the
one part of the step that could not be tried on the machine it was written
on, which has no such bits either ("Not done").

**No package holds a symbolic link,** which the producer refuses (a manifest
has no way to say one, and the updater does not follow links,
`include/katana/update/plan.hpp`). The Linux tree has none by construction:
`cmake/KatanaDeployUnix.cmake.in` copies each library as a FILE, under the
name the loader asks for and with the contents its chain of links ends at,
not the chain. Were one ever to appear - in a Qt plugin folder or a data
folder, which that script copies from the toolchain prefix as they are - the
place to resolve it is that script, so that the tarball and its manifest
still describe the same tree.

**The rehearsal in the build jobs.** A tree the producer refuses would
otherwise be found by the Publish job, after every build, and never in a
run without a tag. So each Windows and Linux job, in its smoke test and
before any program has run in the unpacked tree, runs the producer and the
recheck on it as the Publish job will - unsigned, with sequence 1, into a
scratch folder that is thrown away. It is also the only place the producer
meets a tree on the system it was built for.

**A manifest describes the zip or the tarball.** The Windows installer
installs the same tree, with one difference when a code-signing certificate
is present: `cmake/KatanaSign.cmake` signs the two staged copies separately,
so the installer's programs differ from the zip's in the signature's time
stamp. That costs nothing: an installed copy compares its files with the
NEXT release's manifest, in which the programs differ anyway.

**While a release is being replaced** (the Publish job run again, or a new
run for the same tag), its files are uploaded one after another - the
packages and the update zips first, then the signatures, the manifests last.
The Publish job run again by itself normally uploads what is already there. A new
run uploads other files, and for a moment an installed copy may
read a manifest and a signature of two different runs, or a manifest whose
zip has already been replaced. It then verifies nothing, or a fetched
file fails its hash; either way it changes nothing and finds the release
whole at its next check. Should the repository use GitHub's immutable
releases, a published release cannot be replaced at all: make the next
version instead.

### The first release that carries the updater

An installed copy is updated by the helper it already has, which believes
only manifests signed by a key it already holds. So the first release built
with real update keys is installed by hand, as every package was before; the
release after it is the first to arrive as an update. No release had been
published when the updater was added (2026-10-13), so no installed copy is
left without it.

### To verify on the first release

Nothing in the workflow has run since the update assets were added ("Not
done"), and several things can only be seen on GitHub. In the run:

* The Version job's step "Update keys" ends `Update keys: pinned, active
  <key id>, spare <key id>`, with no warning: the key check ran under the
  runner's own CMake. The job's first step prints the run's sequence.
* Each Configure step prints `KATANA_REQUIRE_UPDATE_KEYS:BOOL=ON` and
  `KATANA_UPDATE_REPO:STRING=<repo>` after CMake's own output, and goes on:
  both settings reached the project ("What each job does").
* The Linux, macOS and Windows ARM64 jobs configure and build at all: they
  are the first places the update library, and the helper with it (not on
  macOS), meets those systems - on Linux and macOS OpenSSL and zlib as the
  toolchain prefix has them, on Windows ARM64 their static libraries.
* Each Windows and Linux job's smoke test prints `selftest=ok` and
  `keys=pinned`, and all five print `target=<token>` and `kind=archive`
  ("The smoke test").
* The rehearsal in each of the four jobs ends `verified 1 manifest(s)`, and
  the number of files it reports is the package's. A refusal names every
  file it is about.
* The Publish job waits for approval, if the environment `release` has
  required reviewers; its step "Update assets" then ends
  `verified 5 manifest(s) in dist`, after a line `signature verified by key
  <key id> (active)` for each manifest.

On the release, from any machine (with `<repo>` and the version filled in):

```sh
gh release list --repo <repo> --limit 3          # vX.Y.Z is marked Latest
curl -sL https://github.com/<repo>/releases/latest/download/Katana-update-win64.json.sig
                                                 # one line: ed25519 <key id> <base64>
curl -sL -r 0-99 -o part.bin -w '%{http_code} %{size_download}\n' \
  https://github.com/<repo>/releases/download/vX.Y.Z/Katana-X.Y.Z-win64.update.zip
                                                 # 206 100: a byte range, through GitHub's redirect
gh release download vX.Y.Z --repo <repo> --dir published \
  --pattern 'Katana-update-*' --pattern 'Katana-*.update.zip'
python tools/update_verify_release.py published --keys-cmake cmake/KatanaUpdateKeys.cmake \
  --targets win64,win-arm64,linux-x86_64,linux-aarch64,macos-arm64
                                                 # what GitHub serves is what was signed
```

Then, in a copy installed by hand from the release, `katana_cli -c "UPDATE
STATUS"` says `kind=installer` (the installer) or `kind=archive` (the zip or
the tarball) and the package's target, and `katana_cli -c "UPDATE CHECK"`
says `update=current`: a fresh install is not offered the release it came
from. With the next release published, the same line says
`update=available`, and Help > Check for Updates installs it: the first live
update. What the updater itself still has to be seen doing against the real
GitHub is listed in `docs/update.md`.

Only when they are first used: with `sign` set to `offline`, that
`gh release view vX.Y.Z` and `gh release download vX.Y.Z` find the DRAFT by
its tag (the workflow's re-run and the offline tool's printed command both
rely on it); with `publish_repo`, that the release appears in the other
repository, on its default branch; and when the Publish job is first run
again by itself, that its step "Update assets" prints the sequence the first
attempt printed (GitHub gives a job that is run again the outputs its
earlier jobs had).

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

Two more settings make each of them a RELEASE and not a developer's build.
`-DKATANA_REQUIRE_UPDATE_KEYS=ON` fails the configure while
`cmake/KatanaUpdateKeys.cmake` holds the placeholder update keys, since a
program built with them can never be updated. `-DKATANA_UPDATE_REPO=<owner/name>`
is the repository an installed copy asks for the next version: the one the
run publishes in, which is this repository unless the input `publish_repo`
names another.

CMake takes a `-D` the project has no use for with a warning and builds
without it, and no program says which repository it asks, so a renamed
option would give green jobs and programs that ask the default repository
for ever. Each Configure step therefore ends by checking that both settings
arrived. In `CMakeCache.txt` each must stand as the option the project
declares, `KATANA_REQUIRE_UPDATE_KEYS:BOOL=ON` and
`KATANA_UPDATE_REPO:STRING=<owner/name>`: a `-D` nothing declares stays
there as `UNINITIALIZED`. And in `build.ninja` the update library's compile
line must define `KATANA_UPDATE_REPO` as that repository and as nothing
else. The check stops at the compile line on purpose. Looking for the name
in the built programs was considered and not done: a short text assigned to
a string may be compiled into the instructions that store it, piece by
piece, so its absence from the file's bytes would prove nothing and could
stop a good release ("Not done").

The first job, before any toolchain is installed, asks the keys file the
same question the configures will: it runs `cmake/KatanaUpdateKeys.cmake`
as a script with `KATANA_REQUIRE_UPDATE_KEYS` on, so a tree with placeholder
keys fails in the first minute and says what to do. The file is not run
bare. It is written for a configure, where the root `CMakeLists.txt` has
set the policies of its minimum CMake before including it; a script starts
with none set, and the hosted runner's own CMake (3.31.6 on the image of
2026-10-04; the build jobs install 4.x) then refuses the file's
`if(... IN_LIST ...)`. So the step writes a script of two lines, the same
`cmake_minimum_required` and an `include` of the file, and runs that. Only
a refusal the keys file itself states stops the run. If CMake stops for
another reason, the step says so in a warning and the run goes on: each
job's configure makes the same check, and a check that could not run is not
a reason to refuse a release.

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
  picture - Qt, its platform plugin and the whole start-up;
* `katana_updater --selftest` must succeed and print `selftest=ok` and
  `keys=pinned`: the helper starts with nothing beside it but what the
  package carries, verifies a known signature and refuses an altered one,
  and holds real update keys, not the placeholders. Windows and Linux; the
  macOS bundle carries no helper;
* `katana_cli -c "UPDATE STATUS"` must say `installed=X.Y.Z`, `target=<token>`
  (the token in the package's name, read back from
  `share/katana/install.json`) and `kind=archive`. In the copy the installer
  smoke test installs it must say `kind=installer` instead: the installer
  records its folder under `HKCU`, which is how the updater tells an
  installed copy from an unpacked zip.

Before any of these runs in the unpacked tree, the Windows and Linux jobs
make the tree's update assets as the Publish job will and recheck them
("Update assets", the rehearsal).

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

### Update signing

The update manifests carry a signature of another kind. Those above are for
an operating system, or for the person downloading; this one is what an
installed Katana checks before it believes a word of a manifest: Ed25519 over
the manifest's exact bytes, in `Katana-update-<token>.json.sig`. Every program
verifies it with two public keys compiled into it from
`cmake/KatanaUpdateKeys.cmake`: the ACTIVE key, which releases are signed with,
and a SPARE, which stays offline for the day the active one is lost or
leaked. A release without these signatures still installs by hand; it is
only never offered as an update. How the keys are trusted and replaced is
`docs/update.md`; what follows is what a release needs.

| Secret | What it is | Used for |
|---|---|---|
| `UPDATE_SIGNING_KEY` | the ACTIVE update key's private half, the file as `openssl genpkey` wrote it; a secret of the environment `release` | the `.sig` beside each update manifest |
| `RELEASES_REPO_TOKEN` | a token that may write the releases of the repository the input `publish_repo` names; a secret of the REPOSITORY, not of the environment, so that the first job can see that it is there | publishing there instead of here (the end of this section) |

**Once: the owner's key steps.** No script and no agent makes or sees a
private key: `tools/update_keytool.py` reads the two files only to ask
`openssl` for their public halves (`python tools/update_keytool.py --help`
gives the same steps).

1. Make the two keys, and back both up offline:

   ```sh
   openssl genpkey -algorithm ed25519 -out update-signing.pem
   openssl genpkey -algorithm ed25519 -out update-spare.pem
   ```

   `*.pem` and `update-signing*` are git-ignored. With both lost, no installed
   copy can be updated again: each has to be replaced by hand with a build
   that pins new keys.
2. Pin their public halves, and commit `cmake/KatanaUpdateKeys.cmake`, which
   the tool rewrites (public data):

   ```sh
   python tools/update_keytool.py pub update-signing.pem update-spare.pem
   ```

   From that commit on, the key check in the release workflow's first job
   passes, and prints the two key ids.
3. On GitHub, Settings > Environments > New environment, named `release`.
   Under "Required reviewers" add yourself, so that nothing is signed before
   you have approved the run. Then give that environment the ACTIVE key, and
   only that one; the spare never goes to GitHub:

   ```sh
   gh secret set UPDATE_SIGNING_KEY --env release < update-signing.pem
   ```

   (The Publish job names the environment, and GitHub makes one of that
   name, with no reviewers, the first time a run asks for it. A secret set
   on the repository - the same command without `--env release` - reaches
   the job too, but then every workflow of the repository can read it.)
4. Publish a release and install it by hand ("The first release that carries
   the updater").

**Signing in the run: `sign` left at `ci`.** The step "Update assets" of the
Publish job is given the secret, and no other step or job is. It writes the
key to a file only it can read, in the runner's temporary folder, gives the
producer that file (`--key`), and removes it when the step ends, however it
ends. The producer signs with the `openssl` command line; before that it
refuses a key that is not one of the two the tree pins, whose signature
every installed copy would reject, and any published test key. The recheck
then verifies all five signatures against the pinned keys.

Without the secret there is nothing to sign with. The run says so in a
warning, makes no update asset, and publishes the release as it would have
been before there was an updater: it installs by hand, installed copies are
not told about it, and while it is the latest release they find no update
information at all. To put that right, set the secret and run the Publish
job again from the run's page, while the run still has its artifacts (90
days unless the repository says otherwise): it adds the update assets to the
release. A run without the key over a release that already carries update
assets is refused instead: it would replace the packages and leave manifests
that describe the earlier build.

**Signing offline: `sign` set to `offline`.** For a key that never leaves
the owner's machine. The run makes the manifests and the update zips, signs
nothing, and leaves the release a DRAFT, which no installed copy can see. On
the machine that holds the key, in a checkout that pins the same keys:

```sh
python tools/update_release_sign.py --repo kjarada/Katana --tag vX.Y.Z --key update-signing.pem
```

The tool never runs `gh`. Run first, it finds no downloaded draft and prints
the `gh release download` command that fetches one into `release-vX.Y.Z`;
run that, then the same tool command again. It recomputes every hash from
the zips, shows what the release changes when given the one before it
(`--previous release-vPREVIOUS`, fetched the same way), checks that the key
is one the tree pins, and has the version typed before it signs anything.
Last, it prints the two commands that finish the release, for the owner to
read and run: `gh release upload` with the five `.sig` files, and
`gh release edit vX.Y.Z --draft=false --latest`.

The `.sig` files of a release signed this way are not in `SHA256SUMS.txt`
and have no `.asc`: they did not exist when those were written. The workflow
refuses `offline` for a release that is already published, where it would
put unsigned manifests in the place of the signed ones installed copies are
reading.

**When the active key is lost or leaked,** the spare is the way out: every
installed copy accepts a release signed with it (offline, with
`--key update-spare.pem`), and that release is built from a tree that pins
the old spare as its active key beside a new spare.
`tools/update_keytool.py` refuses to replace both pinned keys at once, since
a copy that trusts only the old pair could then never be updated.

**Publishing in another repository: the input `publish_repo`.** Not needed
while this repository is public, as it is: releases are published in it and
installed copies read them without a token. The input is for a source
repository that is private, whose packages are published from a public
repository of releases only. That repository must exist and hold a commit (a
release tags a branch), and the secret `RELEASES_REPO_TOKEN` must hold a
fine-grained token with "Contents: read and write" on it alone. Set it on
the repository (`gh secret set RELEASES_REPO_TOKEN`, without `--env`): a run
with a tag and a `publish_repo` asks for it in its first job, which does not
run in the environment `release` and sees none of that environment's
secrets, and stops there when it is missing, before anything is built for a
release that could not be created. (A run without a tag publishes nothing
and needs no token.) The run then
builds every program with `-DKATANA_UPDATE_REPO` naming it, writes it into
the manifests' links to the release notes, and creates the release there:
on that repository's default branch, since the commit being released does
not exist in it, and with the plain note "Katana X.Y.Z".

## Not done

* **The workflow has not run since the update assets were added**
  (2026-10-13), and no run of it has yet published a release. What was done
  instead: the file was parsed by an independent YAML parser, every script
  in it passed `bash -n` or PowerShell's parser, every option it gives `gh`
  and the update tools was held to their own lists, and the scripts of the
  first job and of the Publish job (all but the OpenPGP step) were run as
  they stand on made-up packages, with a throwaway key and a stand-in for
  `gh`. The first job's key check was run under CMake 3.31.6, the version
  the hosted image carries, and under 4.4; the check that ends each
  Configure step was run on cache files CMake itself wrote, for projects
  that do and do not declare the two options. None of that is a run: nothing
  was tried on a runner, against the
  real `gh` and GitHub, or on a file system that has execute bits (the
  Windows packages' part of "Update assets"); and the smoke tests' new lines
  need the programs. A rehearsal - two versions published in a throwaway
  repository, the second installed as an update of the first - is the
  owner's acceptance step and the gate for the first real release ("To
  verify on the first release").
* **Nothing in the repository holds the workflow.** The checks above were
  made with scripts of the session that wrote the workflow; they are not in
  the repository and not in ctest. So no test fails if a later edit gives
  the workflow a second trigger, takes the Publish job out of the
  environment `release`, or drops `-DKATANA_REQUIRE_UPDATE_KEYS=ON` from a
  configure, which is what keeps a release from being built with the
  placeholder keys. A check of the file's shape that needs only Python's
  standard library, with a list of breaks it must notice, was written for
  this and is not yet in the tree.
* **The update keys are placeholders** until the owner's key steps ("Update
  signing"); until then the workflow stops in its first job.
* **No program says which repository it asks for updates,** so the jobs
  prove `-DKATANA_UPDATE_REPO` only as far as the update library's compile
  line ("What each job does"). Were `UPDATE STATUS` to name the feed, the
  smoke test could prove it in the program a user runs.
* **The keys file cannot be run bare by a CMake older than 4.0.**
  `cmake -P cmake/KatanaUpdateKeys.cmake`, the use the file's own header
  gives, stops there at `if(... IN_LIST ...)`, because a script has no
  policy set (seen with CMake 3.31.6, 2026-10-13). The workflow goes round
  it ("What each job does"); the file should state the policy itself.
* **On macOS without a Developer ID the install record is added after the
  bundle is sealed.** Found by reading, 2026-10-13, and not yet seen on a
  Mac: `cmake --install` seals `Katana.app` ad hoc
  (`cmake/KatanaDeployUnix.cmake.in`, its last step), and CPack then writes
  `Contents/Resources/share/katana/install.json` into the staged bundle
  (`cmake/KatanaPackaging.cmake`, "install.json"). With a Developer ID
  `cmake/KatanaSign.cmake` signs everything again afterwards; without one
  nothing does, so `codesign --verify --deep --strict` in the macOS smoke
  test should refuse the bundle for a file added to it. The bundle has to be
  sealed again after the record is written, in the packaging and not in the
  workflow.
* **macOS is told about a release, not updated,** and the Linux packages are
  not yet updated in place either (`docs/update.md`); their manifests and
  update zips are published all the same.
* **The Linux certificate bundle is outside an update.** `ssl/` is not one
  of the four folders a manifest may name, so `ssl/cacert.pem` is excluded
  ("Update assets"). It will matter when a Linux copy can be updated in
  place: then either `ssl` joins the folders
  (`include/katana/update/paths.hpp` and both tools) or the bundle moves
  under one of them.
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
  An update between two such releases therefore fetches the libraries that
  changed as well as the programs (the owner's decision, 2026-10-08: not
  frozen).
