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
   job waits until one of them approves it on the run's page. The
   environment takes runs of `main` only, once it is restricted as "Update
   signing" says (step 3 there): a run with a tag that was started on
   another branch builds and tests everything and is then refused at this
   job.
4. The run takes as long as its slowest job (Windows x86-64: build, the
   whole suite, the installer). When all five pass and the Publish job has
   run, release `vX.Y.Z` appears, marked as the latest, with:

| File | What it is |
|---|---|
| `Katana-X.Y.Z-win64.exe` | Windows x86-64 installer (NSIS), per user, with no administrator prompt (see "The Windows installer"), with an uninstaller |
| `Katana-X.Y.Z-win64.zip` | the same tree to unzip anywhere; run `bin/katana.exe` |
| `Katana-X.Y.Z-win-arm64.exe` | Windows 11 on ARM64 installer (NSIS), per user, with an uninstaller |
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
files for an update's), the libraries they load, `share/katana/samples`,
`LICENSE` and, under `share/katana/licences`, the additional terms, the
plain-words guide and the third-party notices, and
`share/katana/install.json`, which names the package's target, its version
and the sequence of the run that built it ("Update assets", "The package
names its own build") and is what makes a copy a release the updater will
touch; each disk image holds
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
   `share/katana/install.json` that names its own target, the version being
   published and the run's sequence, or the step stops before it makes a
   manifest: a manifest is offered to the installs whose record names its
   target, and it must describe the build the package says it is ("The
   package names its own build", below).
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
| `--sequence` | the UTC time in seconds at which the run began, read once in the Version job: ONE value for all five manifests, and the one the five packages were configured with | it orders two BUILDS of one version, so it is taken when the building starts and not when the publishing does. A whole new run outranks an earlier one. The Publish job run again by itself gets the first job's value again, and so publishes the same files under the same sequence (for the same input the producer writes the same bytes, its zlib being the same). That matters because an update is offered when `(version, sequence)` is above what a copy recorded when it was last updated (`isNewerThan`, `include/katana/update/trust.hpp`): a sequence read at publishing would offer every such copy the files it already has, and have it restart to install none. The recheck refuses a release whose manifests disagree on it |
| `--seed` | `share/katana/samples/*` | a seed is written only where it is absent. Opening a sample project changes it in place, so as ordinary data every update would fetch the samples again and write over what the user had done in them |
| `--exclude` | `LICENSE`, and for Linux `ssl/*` | a manifest names files under `bin`, `share`, `etc` and `lib` and nowhere else, and the producer refuses a tree that holds anything more rather than leave it out unasked. The licence text lies beside those folders; the Linux tree keeps its certificate bundle in `ssl/`, where the bundled libcurl looks ("How the Linux and macOS packages are made self-contained"). Neither is ever updated |
| `--version`, `--tag`, `--repo` | what is being published, and where | what a manifest offers, and its link to the release notes |
| `--key` | the signing key, when the run signs | "Update signing" |

The seed and the excluded names are written once, at the top of the workflow
(`UPDATE_SEED`, `UPDATE_EXCLUDE`, `UPDATE_EXCLUDE_LINUX`), for the Publish job
and the rehearsal below alike. `share/katana/install.json` needs no option:
the producer leaves the install record out by itself.

**The package names its own build.** The run's sequence goes to the builds
as well as to the producer. Every job configures with
`-DKATANA_UPDATE_SEQUENCE=<sequence>`, and CPack writes it beside the
version into the package's install record (`cmake/KatanaPackaging.cmake`,
"install.json"):

```
{"schema":1,"target":"win64","version":"X.Y.Z","sequence":1792312800}
```

A package built outside the workflow is given no sequence and names its
version alone. The record is the copy's own account of which build it is,
and the updater goes by it where a zip was unpacked by hand over a copy that
had updated itself: that copy is placed by the higher of the helper's record
and the package's, by version and within a version by sequence
(`docs/update.md`, "The installed version"). With the release's own
sequence in the package, such a copy is not offered the release it already
is, and is offered a later build of its version. So the number in a package
and the number in its manifest must be ONE number, and the workflow holds
them together in three places:

* each Configure step refuses a sequence that is not a number before CMake
  runs - an empty one would configure, and package a Katana that names its
  version alone - and then finds it in the cache as the option the project
  declares ("What each job does");
* each job's smoke test reads the record out of the package it unpacked:
  the target, the version and the sequence, each a member of its own ("The
  smoke test");
* the Publish job reads the record of each of the four packages it unpacks
  against the version and the sequence it is about to write into their
  manifests, and stops before it makes one. It does not open the disk
  image, which a Linux runner cannot mount: that package's record is read
  in its own job only.

A freshly installed copy is not placed by its sequence at all: with no
record of an update beside it, a release of its own version is not an
update whatever the two numbers are (`isNewerThan`).

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
done"), and several things can only be seen on GitHub. Before the run, the
reads that end step 3 of "Update signing" must say that the environment
`release` has its reviewers and takes the branch `main` and no other. In
the run:

* Every job's first step, "Set up job", lists the actions it fetched, each
  by the 40 digits the workflow names it with ("The actions the workflow
  runs"), and no job stops there for an action GitHub could not find.
* The annotations on the run's summary page, read for Node 20. Five of
  the six actions ask for it ("Not done"). A warning that names them says
  either that they were run on Node 24 instead, or that they ran on Node
  20 and the day it leaves the runner: either is the reason to move those
  actions before the next release, and a step that fails inside one of
  them is the reason to move it now. No such warning means only that
  GitHub asked the runner for none.
* The Version job's step "Update keys" ends `Update keys: pinned, active
  <key id>, spare <key id>`, with no warning: the key check ran under the
  runner's own CMake. The job's first step prints the run's sequence.
* Each Configure step prints `KATANA_REQUIRE_UPDATE_KEYS:BOOL=ON`,
  `KATANA_UPDATE_REPO:STRING=<repo>` and
  `KATANA_UPDATE_SEQUENCE:STRING=<the sequence the first job printed>`
  after CMake's own output, and goes on: the three settings reached the
  project ("What each job does").
* The Linux, macOS and Windows ARM64 jobs configure and build at all: they
  are the first places the update library, and the helper with it (not on
  macOS), meets those systems - on Linux and macOS OpenSSL and zlib as the
  toolchain prefix has them, on Windows ARM64 their static libraries.
* Each job's smoke test prints, before any program runs, the three members
  it found in the package's install record: `"target":"<token>"`,
  `"version":"X.Y.Z"` and `"sequence":<that same sequence>` ("Update
  assets", "The package names its own build"). It is the first time the
  packaging writes a sequence on a runner; and where the record lies in the
  macOS bundle, `Contents/Resources/share/katana/install.json`, was read
  from the packaging and not seen on a Mac.
* Each Windows and Linux job's smoke test prints `selftest=ok` and
  `keys=pinned`, and all five print `target=<token>` and `kind=archive`
  ("The smoke test").
* The rehearsal in each of the four jobs ends `verified 1 manifest(s)`, and
  the number of files it reports is the package's. A refusal names every
  file it is about.
* The Publish job waits for approval, if the environment `release` has
  required reviewers; its step "Update assets" then prints the same three
  members for each of the four packages it unpacks and ends
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

Three more settings make each of them a RELEASE and not a developer's build.
`-DKATANA_REQUIRE_UPDATE_KEYS=ON` fails the configure while
`cmake/KatanaUpdateKeys.cmake` holds the placeholder update keys, since a
program built with them can never be updated. `-DKATANA_UPDATE_REPO=<owner/name>`
is the repository an installed copy asks for the next version: the one the
run publishes in, which is this repository unless the input `publish_repo`
names another. `-DKATANA_UPDATE_SEQUENCE=<sequence>` is the run's sequence,
the number the Version job read before anything was built: CPack writes it
beside the version into the package's install record, and the Publish job
writes it into the release's update manifests ("Update assets", "The
package names its own build"). The step refuses a sequence that is not a
number before it starts CMake.

CMake takes a `-D` the project has no use for with a warning and builds
without it, and no program says which repository it asks, so a renamed
option would give green jobs and programs that ask the default repository
for ever. Each Configure step therefore ends by checking that the three
settings arrived. In `CMakeCache.txt` each must stand as the option the
project declares, `KATANA_REQUIRE_UPDATE_KEYS:BOOL=ON`,
`KATANA_UPDATE_REPO:STRING=<owner/name>` and
`KATANA_UPDATE_SEQUENCE:STRING=<sequence>`: a `-D` nothing declares stays
there as `UNINITIALIZED`. And in `build.ninja` the update library's compile
line must define `KATANA_UPDATE_REPO` as that repository and as nothing
else. The sequence is compiled into no program, so there is no compile
line to look for it on: the cache says the project took it, before anything
is built, and the smoke test reads it where it ends up, in the package's
install record. The check of the repository stops at the compile line on
purpose. Looking for the name
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

### The actions the workflow runs

The jobs run six actions: code of other repositories, which the runner
fetches when a job starts. Each `uses:` line names its action by the 40
digits of a commit, with the tag that commit was read from as a comment on
the same line:

```yaml
- uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
```

Until 2026-10-14 every line named a tag (`@v4`). A tag is whatever its
publisher points it at on the day of the run: one that is moved - by a new
release, or by whoever has taken over the publisher's account - changes
the code every job runs without one line changing here, and two of the
actions run in the Publish job, which is where the update signing key is.
A commit cannot be moved: what runs is what was read. The comment is for
the reader; GitHub runs the commit.

| Action | Tag | The release at the commit | Read | Jobs |
|---|---|---|---|---|
| `actions/checkout` | `v5` | 5.1.0 | 2026-10-15 | every job |
| `msys2/setup-msys2` | `v2` | 2.33.0 | 2026-10-14 | the Windows jobs |
| `actions/upload-artifact` | `v6` | 6.0.0 | 2026-10-15 | the Windows, Linux and macOS jobs |
| `actions/setup-python` | `v6` | 6.3.0 | 2026-10-15 | the Linux and macOS jobs |
| `actions/cache` | `v5` | 5.1.0 | 2026-10-15 | the Linux and macOS jobs |
| `actions/download-artifact` | `v7` | 7.0.0 | 2026-10-15 | Publish |

**Moved off Node.js 20 on 2026-10-15.** GitHub deprecated Node.js 20 on its
runners, and every job of run 38580778852 warned that `checkout`,
`upload-artifact`, `download-artifact`, `cache` and `setup-python`, at the
commits above, target it and were being forced onto Node.js 24. Each was moved
to the FIRST major version whose `action.yml` says `using: node24` - checkout
v5, upload-artifact v6, download-artifact v7, cache v5, setup-python v6 - by
the steps below. Between the old commits and these, the releases' one
breaking change besides the runtime and a runner of at least 2.327.1 (which
GitHub's own runners are) is download-artifact v5's extraction path for a
single artifact downloaded by id; the Publish job downloads by `pattern`
with `merge-multiple`, which it leaves alone. *Rejected:* the newest majors
(checkout v7, upload-artifact v7, download-artifact v8, cache v6,
setup-python v7): nothing here needs what they add, and the two artifact
actions' were eight days old when read. `msys2/setup-msys2` was not in the
warning and stays where it was.

The commits themselves are in the workflow and not repeated here. A row
says which release its action's commit is, and the day that was read; it
is changed together with the action's lines (step 3 below), and where
the two disagree the workflow is what runs.

**Moving an action to a newer version.** Nothing moves by itself any more,
so an action stays at its commit until somebody moves it: when a runner
image retires what an action needs, when an action's publisher reports a
fault, or before a release, as part of getting ready for it. One action at
a time:

1. Read the commit its tag names now. These are reads of GitHub and change
   nothing there:

   ```sh
   gh api repos/actions/checkout/git/ref/tags/v4 --jq '.object.type + " " + .object.sha'
   ```

   `commit` and 40 digits: those digits are the commit. `tag` and 40 digits:
   the tag is an annotated one and the digits are the tag's own, not a
   commit's; follow it, and take what this prints:

   ```sh
   gh api repos/actions/checkout/git/tags/<those digits> --jq '.object.type + " " + .object.sha'
   ```

   (All six were plain tags, `commit`, when they were read.) A newer major
   version is the same read with the other tag, `v5` for `v4`. Which
   release the commit is, for the table above, is among the tags on it,
   `v4.4.0` beside `v4`:

   ```sh
   gh api repos/actions/checkout/tags --paginate --jq '.[] | select(.commit.sha == "<the commit>") | .name'
   ```
2. Read what changed between the commit the workflow names and the new one,
   `https://github.com/actions/checkout/compare/<old>...<new>`, and the
   action's release notes: a major version may rename an input or need a
   newer runner. The Node it asks the runner for is the `using:` line
   under `runs:` in its `action.yml` as the new commit has it:

   ```sh
   gh api -H "Accept: application/vnd.github.raw+json" "repos/actions/checkout/contents/action.yml?ref=<new>"
   ```
3. Put the new digits on EVERY line that names the action - one action,
   one commit, in all jobs - change the comment if the tag changed, and
   put the release and the day in the action's row of the table above.
4. Run the workflow with the tag left empty. It builds and tests the
   packages and publishes nothing, which tries the new commit before a
   release depends on it. `actions/download-artifact` is the exception:
   it runs only in the Publish job, which such a run skips, so a new
   commit of it is first tried by a release itself. Should it fail there,
   nothing has been published; the line is put right on `main` and the
   release is started again as a new run, because a job run again from
   the run's page runs the workflow as that run's commit has it.

A `uses:` line added later is written the same way, by the same read.
GitHub can be told to refuse any other: the repository's Actions
permissions (Settings > Actions > General) hold a requirement that actions
be pinned to a full-length commit SHA, which
`gh api repos/kjarada/Katana/actions/permissions --jq .sha_pinning_required`
shows (`false` on 2026-10-14). It has not been turned on, and what it
does to a run was not tried ("Not done").

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
user would run it. First, before any program runs, what the package says
of itself: `share/katana/install.json` (in the bundle, under
`Contents/Resources`) must hold `"target":"<token>"`, `"version":"X.Y.Z"`
and `"sequence":<the run's sequence>`, each as a whole member, so a sequence
that only begins with the run's is refused ("Update assets", "The package
names its own build"). The record is read through a pipe and nothing is
written beside it: a file left in the tree would be taken for one of the
package's by the rehearsal that follows. Then:

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

**The installers.** Each Windows installer is then run as a deployment script
runs it, as the user the job runs as (the installer asks for no administrator
rights): `/S /D=<folder>`, silently, into a folder of its own. The installed
`katana_cli` must name `EPSG:32630` as above, and the uninstaller
(`Uninstall.exe /S`, kept in place with `_?=` so the step waits for it) must
remove `bin\katana_cli.exe`.

## The Windows installer

The installer is PER USER (decision D1 of the update system).
It asks for no administrator rights and shows no UAC prompt, installs for the
account that runs it, and writes the registry under `HKCU` only. Why: the
updater replaces files inside the install as that same user; a per-machine
install in `C:\Program Files` would need an elevation on every update, and
every elevated step is one more way for an update to go wrong. No release
had been made when this was decided, so nothing was migrated from the older
per-machine installer.

* **Where.** `%LOCALAPPDATA%\Programs\Katana` unless the person chooses another
  folder they can write to; a folder that cannot be written to is refused with
  a message (a silent run exits with code 2 and writes nothing). A copy that is
  already installed is offered again as the folder, and `/D=` beats both.
* **What it records.** `HKCU\Software\Jarada\Katana`, default value: the install
  folder, which is how Katana and the updater tell an installed copy from an
  unpacked archive. `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Katana`:
  the entry in **Settings > Apps** (`DisplayName`, `DisplayVersion`, `Publisher`
  Jarada, `UninstallString`, `InstallLocation`, `DisplayIcon`, `EstimatedSize`,
  `NoModify`, `NoRepair`), which the updater rewrites after an update. The
  Start-menu folder and its shortcuts are the current user's.
* **What the uninstaller removes.** The files the installer put down; every file
  the installed manifest lists (`share/katana/installed.manifest.json`: an update
  adds files that the uninstaller, built before them, has never heard of); the
  updater's records and its working folder `.update`; what the updater staged in
  `%LOCALAPPDATA%\Katana\update` (not `state.json`, the person's choices about
  updating); the registry keys and shortcuts above, and the command the update
  helper registers under `HKCU\...\RunOnce` to recover at the next logon. A path
  read from the manifest is held to the updater's rules first (one of `bin`,
  `share`, `etc`, `lib` first, no `.` or `..` component, no wildcard, printable
  ASCII, at most 240 characters), so a manifest that has been tampered with
  cannot send a delete outside the folder.
* **What it leaves.** Anything else in the install folder, and the folder with it
  (the folder is removed only when empty and nothing in it is removed
  recursively but `.update`); the settings under `HKCU\Software\Katana` and
  everything else under `%LOCALAPPDATA%\Katana`.
* **Silent.** `/S` and `/D=<folder>` (last, unquoted) work as in any NSIS
  installer, and no question is asked without a default answer. Installing over
  an installed copy asks whether to uninstall the old one first, and a silent
  run does so without asking; that is also the way to repair a copy. An entry
  in **Settings > Apps** whose uninstaller is no longer there, because the
  folder was deleted by hand, is no installed copy: nothing is asked, nothing
  is run, and the install goes ahead and writes the entry anew.
* **Over a copy that was not uninstalled first** (the answer to that question
  was No, or the registry does not know the copy in the chosen folder) the
  installer removes `share/katana/installed.json` and `.update` before it puts
  its files down. The first is the updater's record of the version the folder
  holds, which would go on naming the old one; the second can hold the journal
  of an update that was cut short, whose recovery at the next start would put
  files of the old version back over the new ones. The installed manifest is
  kept: it is how the next update and the uninstaller find the files an earlier
  update added. A folder without `share/katana/install.json`, which is what
  says a packaged Katana is in it, is not touched.
* **A file in use** (a program of Katana's that is still running) stops the
  install: the installer offers Retry and Cancel, never the skip that NSIS
  offers by default, which would leave the old program among the new libraries
  (`AllowSkipFiles off`).
* **The licence page.** Between Welcome and the folder page the installer shows
  the licence. Until 2026-10-15 it showed none: CPack builds the page only when
  `CPACK_RESOURCE_FILE_LICENSE` names a file, nothing named one, and the
  template's `@CPACK_NSIS_LICENSE_PAGE@` was empty, so an installer went from
  Welcome to the folder page with no licence in it. `cmake/KatanaPackaging.cmake`
  now makes the page's text at configure time, `KatanaInstallerLicence.txt` in
  the build folder, from the two texts that are binding: a note saying whose
  Katana it is and that nobody has to accept the licence to run it, then
  `LICENSE` (the GNU General Public License) whole, then `ADDITIONAL_TERMS.md`
  whole. One file because the installer has one page for it. It is made again
  when either text changes (`CMAKE_CONFIGURE_DEPENDS`), has Windows line ends
  (the page's text box breaks a line on CRLF, and `LICENSE` is LF in the
  repository) and is ASCII, which a Unicode installer reads as it is: a
  character outside ASCII would need a byte order mark that CMake cannot write,
  so the configure refuses one by name. The template says, before the page, that
  the licence need not be accepted and puts **Next**, not **I Agree**, on its
  button (the page the NSIS manual shows for a GPL program): the GPL asks for no
  acceptance, and a button that asked would say it did. A silent install
  (`/S`) shows no page. The packages carry the same notices, `LICENSE` at the
  top and `ADDITIONAL_TERMS.md`, `LICENSING.md` and `THIRD_PARTY_NOTICES.md`
  under `share/katana/licences`, and the page says where. *Under `share`, not
  beside `LICENSE`:* the update manifest has to leave `LICENSE` out
  (`UPDATE_EXCLUDE`: it refuses any file outside `bin`, `share`, `etc` and
  `lib`), so a notice beside it would never be updated, and the third-party
  notices change with the libraries an update brings. *Rejected:* a page of the
  GPL alone, since the additional terms are as binding as it is; showing the
  third-party audit on the page, a long table that is read in the folder it is
  installed to. `packaging_installer_shows_the_licence` holds this build's
  own configure to all of it (`tools/check_installer_licence.cmake`), and
  `packaging_installer_licence_check_*` hand the check a configure wrong in one
  way each - no licence file, a page of Unix line ends with no licence, a
  template with no page, notices no install rule names - and read the refusal.
  The release smoke test checks that each package holds the notices. *Seen,
  2026-10-15:* the interactive installer, run on a machine that already had
  Katana installed (so it first asked about uninstalling the old one, answered
  No), showed Welcome, then **License Agreement** with the header "Please review
  the licence of Katana before installing it.", the licence text in its scroll
  box beginning with the note and the copyright line, the line that nobody has to
  accept it, and **Next >** between **< Back** and **Cancel**; it was closed
  there, before the Install step, so nothing was written. (Run an installer from a
  shell with `MSYS2_ARG_CONV_EXCL='*'`, or `/S` is rewritten as a path and the
  wizard opens instead of a silent install.)
* **NSIS.** The template, `cmake/NSIS.template.in`, replaces CPack's: CPack's asks
  for administrator rights, writes `HKLM` and installs to Program Files. It has no
  components, no "add to PATH" and no Install Options page (Katana has never
  changed `PATH`), and it is a Unicode installer, because the default folder is
  under the user's profile, whose name can hold any character. MSYS2's NSIS
  (3.13) is built for x86-64 only and keeps its plugins where the default target
  does not look, so for it the template says `Target amd64-unicode`; CPack's own
  template stops there with "Plugin not found". The official and Chocolatey builds
  (the ARM64 job) are x86 and need nothing said. A missing template is not an
  error to CPack, only another installer, so the configure fails without it.
* **Tests.** `packaging_nsis_template_is_per_user` holds the template's text,
  on any host, to the rules listed at the top of
  `tests/update/check_nsis_script.cmake`: no elevation and nothing machine-wide,
  the folder and the registry values above, the folder tried before anything is
  cleared or written, what an install over a copy clears, an entry whose
  uninstaller is gone, what the uninstaller removes, in which order, and what it
  keeps, and a default answer to every question. It reads text. What only a run
  shows (the exit code of a refused folder, the size in **Settings > Apps**, the
  shortcuts) it does not hold; a block comment and a condition it cannot
  evaluate it refuses, and what is inside a false condition it leaves out, as
  NSIS does; an instruction that is compiled and never reached it cannot see.
  Each `packaging_nsis_check_catches_*` test breaks one rule in that text and
  requires the check to name it. Where NSIS is
  installed, `packaging_nsis_installer_builds_from_the_real_configuration` runs
  the build's own CPack configuration over a small tree, compiles the installer
  with `makensis` and holds the script CPack generated to the same rules, and
  `packaging_nsis_uninstaller_removes_what_is_listed_and_nothing_else` compiles
  the uninstaller's own file-removal functions into a throwaway installer and
  runs them over small trees whose manifests have hostile entries, inside the
  build tree (no registry, no Start menu, a stand-in for `%LOCALAPPDATA%`); the
  `packaging_nsis_uninstaller_test_catches_*` tests break one of those functions'
  rules each, and run only where the unbroken run passes (it is their fixture).
  Those programs are new to the machine each time, and a scanner that looks at
  a new program before it runs takes some seconds over each: the script gives
  each a minute and stops at the first that does not come back. (A build folder
  with a space in its path has none of the uninstaller tests: NSIS takes the
  folder as the unquoted end of a command line.) None of them runs Katana's
  installer: that writes the user's registry and Start menu. **An install, an
  update and an uninstall on a clean Windows machine, by hand, is the owner's
  acceptance step**, and with it an install after the folder was deleted by
  hand, which needs the registry and so has no test that runs it; the release
  workflow's installer smoke test (above) covers the silent install and
  uninstall only.

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

**The window names the plugin folder itself.** The first macOS package to
reach its smoke test (2026-10-15) ran `katana_cli` whole - GDAL, PROJ, PDAL,
the update record - and then the window stopped: "Could not find the Qt
platform plugin", with `Frameworks/qt6/plugins/platforms/` in the bundle,
signed and sealed. So Qt had not taken that folder from `qt.conf`; why was
not settled from Qt's source, where a relative path resolves against
`Contents` only once the application object exists. `katana` on macOS
therefore adds `Contents/Frameworks/qt6/plugins` to Qt's library paths
itself, from its own path, before the application object loads the
platform plugin (`addBundledQtPlugins`, `src/katana_qt/main.cpp`), and the
smoke test runs the window with `QT_DEBUG_PLUGINS=1` and prints the bundle's
`qt.conf`, so that the log says where Qt looked.

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

### Windows: why a download is warned about, and what stops it

The owner downloaded `Katana-0.2.0-win64.exe` from the release on 2026-10-15
and Edge said "isn't commonly downloaded. Make sure you trust it before you
open it", offering only Keep. That is Microsoft Defender SmartScreen's
reputation check on a file it has not seen before, and it shows for any file
that is **new and unsigned**: v0.2.0 was published with no code-signing
certificate (the repository holds no `WINDOWS_CERTIFICATE`; `gh secret list`
and the `release` environment hold only `UPDATE_SIGNING_KEY`, which is the
update key and has nothing to do with Windows). No change to the program or
the installer makes Windows trust an unsigned file; what does:

* **A signature from a publisher Windows trusts** - Authenticode over every
  program and over the installer, time-stamped - so that the warning names
  "Jarada" as a verified publisher and reputation follows the publisher, not
  each new file's hash. Reputation is Microsoft's and is not published: a
  certificate that is new may still show a milder prompt until enough people
  have installed signed files from it, and the update helper's downloads do
  not carry a mark of the web at all, so only the first download is warned
  about.
* **How the key is held.** Since 2023 a certificate authority issues a code
  signing key only on a hardware token or in its cloud signing service, so a
  certificate bought today cannot be exported as the `.pfx` that
  `cmake/KatanaSign.cmake` and the workflow expect (`WINDOWS_CERTIFICATE`,
  `osslsigncode`). The signing has to be done by a service from the job:
  **Azure Trusted Signing** (Microsoft's own service, about US$10 a month, an
  Azure account and an identity check that takes days; eligibility by country
  and by individual or organisation is to be read on Azure's page when it is
  applied for), or a certificate authority's cloud signer (DigiCert KeyLocker,
  SSL.com eSigner, an Azure Key Vault HSM with AzureSignTool).
* **Where it would go.** Sign the four programs in the build's `bin` before
  CPack stages them, and the installer after it is made: CPack's pre-build
  script already runs `KatanaSign.cmake` between the two, and a
  provider-neutral `KATANA_SIGN_COMMAND` (a command with the file's place in
  it, run for each file in place of `osslsigncode`) would let any of the
  services above be named by the workflow without the CMake knowing which.
  **Not written:** the choice of service is the owner's - money, an account and
  an identity - and a workflow step for a service that has no account behind
  it could not be run.
* **Other ways round the browser's warning,** each a decision of its own: a
  winget manifest (a pull request to microsoft/winget-pkgs; `winget install`
  does not go through the browser's reputation check) and a Microsoft Store
  listing (an MSIX, signed by Microsoft). Until then a download is kept
  through the browser's `...` menu > Keep > Show more > Keep anyway, or
  Properties > Unblock on the file.

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
   Two settings on its page, before it is given anything:

   * **Required reviewers:** add yourself, so that nothing is signed before
     you have approved the run.
   * **Deployment branches and tags** (Settings > Environments > release >
     Deployment branches): in place of "No restriction" choose "Selected
     branches and tags" and add one rule, the branch `main`, which is the
     default branch. No rule for a tag: a tag can be put on any commit.

   Why the restriction. An environment gives its secrets to any job that
   names it, and what a job does is what the workflow file says ON THE
   BRANCH THE RUN IS OF: a run started by hand takes `release.yml` as the
   chosen branch has it, and a new workflow file on a branch runs when the
   branch is pushed. Unrestricted, the environment would therefore hand the
   key to a branch whose Publish job had been edited to print it, or to
   sign other files, as soon as that branch was run - after one approval,
   where there are reviewers, asked on a page that names the branch and
   shows nothing of what it changed. Restricted, GitHub refuses such a job
   before it starts, so the key reaches only what `main` holds: its
   `release.yml`, and the tools of that tree which the Publish job runs.

   That is as strong as what guards `main`, and no stronger. Nothing
   guards it yet but the right to push: `main` is not a protected branch
   and the repository has no ruleset ("Not done"), so an edited
   `release.yml` pushed straight to `main` and then run is asked for
   approval on a page that names `main`. What the restriction buys is
   that such an edit has to be a commit of `main`, in the history every
   checkout pulls, and cannot sit on a branch nobody reads. The approval
   is still the owner's look at the commit the run is of, which the
   run's page names. The cost is that a release is started on `main`, as
   "Publishing a release" already says. A run without a tag works on any
   branch as before: its Publish job is skipped and never asks for the
   environment.

   Then give the environment the ACTIVE key, and only that one; the spare
   never goes to GitHub:

   ```sh
   gh secret set UPDATE_SIGNING_KEY --env release < update-signing.pem
   ```

   (Make the environment first. The Publish job names it, and GitHub makes
   one of that name, with no reviewers and no restriction, the first time a
   run asks for it. A secret set on the repository - the same command
   without `--env release` - reaches the job too, but then every workflow
   of the repository can read it. `tools/update_keytool.py` prints the
   command with `--env release`, in its help and after `pub`.)

   Both settings are GitHub's, where nothing in the repository can see
   them, so read them back. Each read prints plain text: `gh` lays a
   list or an object out over several lines at a terminal and on one
   line into a pipe (seen with `gh` 2.98.0), so one of those would not
   look the same in both.

   ```sh
   gh api repos/kjarada/Katana/environments/release \
     --jq '[.protection_rules[].type] | sort | join(" ")'
       # branch_policy required_reviewers
   gh api repos/kjarada/Katana/environments/release \
     --jq '.deployment_branch_policy | "custom=\(.custom_branch_policies) protected=\(.protected_branches)"'
       # custom=true protected=false   (custom=null protected=null: no restriction)
   gh api repos/kjarada/Katana/environments/release/deployment-branch-policies \
     --jq '.branch_policies[] | .type + " " + .name'
       # branch main, and no other line
   ```

   None of the three could be run here when this was written: the
   environment did not exist ("Not done"). They were run as they stand
   on an environment of another public repository that has both settings
   (`pypa/pip`, `PyPI`), where the first two printed the lines above and
   the third one line for that environment's one rule.
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

* **The Windows installer and programs are unsigned**, so a browser warns about
  the download ("Windows: why a download is warned about", above): a
  code-signing service has to be chosen and its secrets put in the workflow
  (the owner's), and the signing through a service written.
* **The workflow has run once, and has not yet published a release.** The
  first run, on 2026-10-14 for `v0.2.0`, passed the Version job (with the
  update keys pinned that day) and the whole Windows x86-64 job: build, suite,
  package, smoke test and installer test. The other four stopped before their
  packages, and none on the workflow itself:
  * Windows ARM64: MSYS2's package download timed out at a mirror, before any
    of the project's code ran.
  * macOS arm64: a test captured two constants in lambdas, which clang
    rejects as unneeded (`-Wunused-lambda-capture` under `-Werror`).
  * Linux x86-64 and ARM64: four headless window tests expected a count
    grouped with a comma, which the runner's C locale does not group; and a
    fake-feed tool test expected its records in the order the requests were
    sent, which a threaded server does not keep.

  The same day a local build with clang and libc++ (MSYS2 CLANG64, the
  library of the Windows ARM64 build) found what that job would have met
  next, all under libc++ on Windows only: `sha256FileHex` hashed the null
  device as an empty file and returned the digest of a file's first part
  when a read failed part way (libc++'s `std::filesystem::status` cannot
  classify NUL, and its `ifstream` reports a failed read as the end of the
  file), and the update library's path helper turned an empty path into the
  current folder. All of these are fixed. What was done before the run,
  for the record: the file was parsed by an independent YAML parser, every script
  in it passed `bash -n` or PowerShell's parser, every option it gives `gh`
  and the update tools was held to their own lists, and the scripts of the
  first job and of the Publish job (all but the OpenPGP step) were run as
  they stand on made-up packages, with a throwaway key and a stand-in for
  `gh`. The first job's key check was run under CMake 3.31.6, the version
  the hosted image carries, and under 4.4; the check that ends each
  Configure step was run on cache files CMake itself wrote, for projects
  that do and do not declare the two options. Still not tried on a runner:
  the Publish job - against the real `gh` and GitHub, and on a file system
  that has execute bits (the Windows packages' part of "Update assets") -
  and the smoke tests of the four packages other than Windows x86-64's. A
  rehearsal - two versions published in a throwaway
  repository, the second installed as an update of the first - is the
  owner's acceptance step and the gate for the first real release ("To
  verify on the first release").
* **Nor has it run since its actions were named by commit** (2026-10-14,
  "The actions the workflow runs"). The six commits were read from GitHub,
  each two ways that agree, and every `uses:` line was held to them by a
  script of that session; the edit is those lines and a comment. The first
  run (2026-10-14) fetched five of the six by these digits, every job past
  its "Set up job" step; `actions/download-artifact`, which only the Publish
  job runs, has not been fetched yet.
* **Nor since the packages were given the run's sequence** (2026-10-14,
  "Update assets", "The package names its own build"). What was done
  instead: the workflow's bash scripts passed `bash -n` again (the two
  PowerShell steps did not change); the reading of the install record was
  run on made-up records - one as the packaging writes it, one spread over
  lines with white space, one with its members in another order, one
  without a sequence, one whose sequence only begins with the run's, one of
  another target and version, and none at all; and the cache check was run
  on the cache CMake wrote for a project that declares
  `KATANA_UPDATE_SEQUENCE` and for one that does not. The first run
  (2026-10-14) built the Windows x86-64 package with the option, and that
  job's smoke test found the run's target, version and sequence in the
  package's install record; the other four packages, and the Publish job's
  reading of the records, have not got that far yet ("To verify on the
  first release").
* **Nothing in the repository holds the workflow.** The checks above were
  made with scripts of the sessions that wrote and changed the workflow;
  they are not in the repository and not in ctest. So no test fails if a
  later edit gives the workflow a second trigger, takes the Publish job out
  of the environment `release`, names an action by a tag again, or drops
  `-DKATANA_REQUIRE_UPDATE_KEYS=ON` from a configure, which is what keeps a
  release from being built with the placeholder keys. A check of the file's
  shape that needs only Python's standard library, with a list of breaks it
  must notice, was written for this and is not yet in the tree; when it is,
  a `uses:` that does not end in 40 hexadecimal digits belongs among what
  it refuses. For that one break GitHub has a setting of its own, which is
  off ("The actions the workflow runs", at its end).
* **Nothing says when a pinned action has a newer version.** An action
  stays at its commit until it is moved by hand ("The actions the workflow
  runs"), and the repository has no `.github/dependabot.yml`, which would
  propose each move as a pull request.
* **Five of the six actions are old major versions that ask for Node 20.**
  At the commits the workflow names, the `action.yml` of
  `actions/checkout`, `actions/upload-artifact`, `actions/setup-python`,
  `actions/cache` and `actions/download-artifact` says `using: node20`
  (`msys2/setup-msys2` says `node24`), and Node 20 reached its end of
  life in April 2026. On 2026-10-14 their publishers' latest releases
  were checkout 7.0.1, upload-artifact 7.0.2, setup-python 7.0.0, cache
  6.1.0 and download-artifact 8.0.2, each `node24`. Naming commits did
  not cause this: the tags `v4` and `v5` name these same commits, and
  that change moved no version. The runner of that day (2.338.0;
  `HandlerFactory.cs` and `JobExtension.cs` of its worker, in the
  repository `actions/runner`) still carries Node 20 beside Node 24 and,
  on the machines these jobs run on, refuses no action for asking for
  it: it runs such an action on one or the other, as GitHub tells it for
  the run, and can name it in a warning on the run's page. So the first run says where this stands
  ("To verify on the first release"). Each of the five is moved as "The
  actions the workflow runs" says, with the release notes of every major
  version between read; none has been tried.
* **`main` itself is not guarded.** The environment `release` takes runs
  of `main` only ("Update signing", step 3), and that is worth what
  decides what gets into `main`. On 2026-10-14 the right to push was all
  that did:

  ```sh
  gh api repos/kjarada/Katana/branches/main --jq .protected
      # false
  gh api repos/kjarada/Katana/rules/branches/main --jq '[.[].type] | sort | join(" ")'
      # an empty line: no rule of any ruleset applies to main
  ```

  A ruleset on the default branch (Settings > Rules > Rulesets) that
  requires a pull request and blocks force pushes and deletion makes the
  second read print `deletion non_fast_forward pull_request` (the names
  were seen on another repository that has those rules; no ruleset was
  made here). Every change to what the Publish job runs would then have
  been a pull request. It is the owner's to decide: it ends pushing to
  `main` directly, which is how changes reach `main` today, and while
  one account has the right to push
  (`gh api repos/kjarada/Katana/collaborators --jq '.[].login'`) the pull
  request it asks for is that account's own to open and to merge: a
  recorded step, not a second person. Until then the approval on the
  run's page is the guard there is.
* **The update keys are placeholders** until the owner's key steps ("Update
  signing"); until then the workflow stops in its first job. The
  environment `release` of those steps, with its reviewers and its
  restriction to `main`, did not exist on 2026-10-14 either: GitHub would
  make one with neither the first time a run reached the Publish job.
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
* **The per-user installer has not been through its acceptance step**: an
  install, an update and an uninstall on a clean Windows machine, by hand
  ("The Windows installer").
* **Neither the installer nor its uninstaller looks for a running Katana.** The
  installer stops at a file that is in use (Retry or Cancel; a silent run
  fails) and never skips it, but by then it has already replaced other files;
  the uninstaller leaves such a file behind and still removes its entry in
  **Settings > Apps**. Close Katana first.
* **No AppImage, `.deb` or Flatpak.** The Linux package is a tarball.
* **The Windows toolchain is not frozen.** MSYS2 installs what it has on the
  day; two runs a month apart may build with different GCC and Qt versions.
  An update between two such releases therefore fetches the libraries that
  changed as well as the programs (the owner's decision, 2026-10-08: not
  frozen).
