# Updates

How an installed Katana learns that a newer release exists, downloads the
files of it that differ from its own, and has them swapped in by a helper
program once the window has closed - and how every file is put back when the
new version does not start. This document is the system as a whole: what a
person sees, what the owner does once and at each release, the feed, the
manifest and its signature, the download, the swap and its recovery, the
three surfaces, the platforms, and what is decided, unverified and not done.

Four other documents hold a part each and are not repeated here:
`docs/release.md` (the workflow that builds the packages and the update
assets, and the Windows installer), `docs/desktop.md`, "Help > Check for
Updates" (the window's dialog, state by state), `docs/mcp.md`, "Updates" (the
agent's tool), and `docs/headless.md`, "Updates in a headless run".

**Where it stands (2026-10-14).** No release carries the updater yet, and no
installed copy has been updated by it. The public keys the tree pins are
placeholders, so every build made from it today refuses every release
(`reason=placeholder_keys`) until the owner has run the key steps below. The
tests drive the whole path - producer, feed, engine, helper - on releases
they sign themselves, served from the loopback address
(`docs/testing.md`, "The update system's tests"); what only GitHub can show is
listed under "To verify on first release".

## What a person sees

1. **The check.** The window of an installed copy asks the release feed once
   a day whether a newer Katana exists. It sends two small requests and
   nothing about the person or the drawing; when it learns nothing - offline,
   an error from the server, a signature that does not verify - it says
   nothing at all. Help > "Check for Updates..." asks at once and answers in
   a dialog.
2. **"Update Available".** When there is a newer release, a button appears in
   the status bar, "Update 0.3.0", and the dialog it opens says "Update
   Available" and "Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71
   MB to download." with Update, Download only, Skip this version, Release
   notes and Later. Nothing opens over the drawing unasked.
3. **One click.** Update downloads the files that changed while the person
   keeps working ("Downloading 31 of 71 MB (12 of 23 files)."), then says
   "Katana 0.3.0 is ready." and "Katana will close, update and reopen in a few
   seconds.", and closes the window the way File > Quit does - with its
   question about unsaved changes. Cancel there, and the window stays and the
   release stays downloaded.
4. **Close, swap, reopen.** Once the window has gone, the helper replaces the
   files, starts the new version once without a window to see that it starts,
   and then opens Katana again. No administrator prompt appears at any point.
5. **If it goes wrong,** the old version is put back and opened again, and
   the dialog says so at that start: "Katana 0.3.0 could not be installed, so
   0.2.0 was restored." with the reason. A release that was installed and did
   not start is not offered again unasked: the dialog adds "A later release
   will be offered, and Settings > Updates offers this one again.", and the
   button there, "Offer the version that was put back again", is the way
   back for a sound release whose first start was only slow. If another
   program of the install is still running - `katana_mcp` held open by an
   agent's client, say - nothing is touched and the dialog names it: "Close
   these first: katana_mcp.exe (4412)." and "Then press Retry."

"A few seconds" is the swap, not the download. Neither has been timed on a
real install ("To verify on first release"); the estimates made when the
system was designed (2026-10-08, from a Release tree: the three programs
compress to about 36 MB, and about 60 of the 189 DLLs of a rolling toolchain
change in a month, another 35 MB) were a download of about 71 MB for a
monthly Windows update and a swap of 3 to 6 seconds.

## The pieces

| Piece | Where | What it is |
|---|---|---|
| the engine | `include/katana/update/`, `src/katana_update/` | layer `update`, which sees `core` and nothing else: versions, the manifest and its path rules, the signature and the trust gate, the plan, the staged download, the journalled swap, rollback and recovery, the per-user records. It links no HTTP library, no Qt and no GDAL |
| the helper | `src/katana_updater/` | `katana_updater`, the fourth program: the swap as a process of its own, static, linked to the engine and `katana_core` alone |
| the verbs | `src/katana_app/update_verbs.hpp`, `update_verbs.cpp` beside it | the `UPDATE` family, one function that runs a line for the context it is handed: `katana_cli`, the window and the agent's tool all run it |
| the transport | `src/katana_app/update_transport.cpp` | the engine's `Fetch`, made of the engine's own rules (`update::makeFetch`) over one request of the project's HTTP stack (`gis::httpExchange`) |
| the window | `src/katana_qt/update_dialog.hpp`, `src/katana_qt/update_workbench.hpp`, the Updates page of `src/katana_qt/customisation/settings_dialog.hpp` | the menu item, the status-bar button, the dialog, the automatic check, the close that lets the helper run |
| the release tools | `tools/update_make_manifest.py`, `tools/update_verify_release.py`, `tools/update_release_sign.py`, `tools/update_keytool.py` | the producer of a release's update assets, an independent recheck of them, the offline signer, and the owner's key step |
| the pinned keys | `cmake/KatanaUpdateKeys.cmake` | the two public keys every program verifies with |
| the installer | `cmake/NSIS.template.in` | per user, so that an update needs no administrator (`docs/release.md`, "The Windows installer") |

Everything the engine does to the machine goes through an interface, so that
the swap, its rollback and its recovery are driven in-process by tests with
fakes: `FsOps` (every change to the disk), `ProcessOps` (who is running),
`Launcher` (starting a program), `RegistryOps` (three per-user registry
values), `CredentialStore` (the one secret) and `Fetch` (the network). The
tests, their fake feed (`tools/update_fake_feed.py`) and the generator of
their signed fixtures (`tools/update_make_fixtures.py`) are `docs/testing.md`,
"The update system's tests".

## What is an install

The unit of everything below is the **install root R**, the folder that
holds `bin/` and `share/` - on macOS `Katana.app/Contents/Resources`, which
holds `share/` (`update::installRootOfExecutable` knows both layouts). Four
records beside the programs say what R is (`include/katana/update/install.hpp`):

| Record | Written by | Says |
|---|---|---|
| `R/share/katana/install.json` | packaging only: CPack's pre-build script (`cmake/KatanaPackaging.cmake`) | `{"schema":1,"target":"win64"}`: this copy is a RELEASE, and which target to ask the feed for. The target comes from here and never from the operating system: Windows on ARM running the x64 zip stays `win64` |
| `R/share/katana/installed.json` | the helper, at commit | the version, sequence, tag and target of the release an update put here. Absent on a fresh install, whose version is then the one the program was built as. Never believed below the program that reads it ("The installed version", below) |
| `R/share/katana/installed.manifest.json`, `.sig` | the helper, at commit | the signed manifest of the installed version: what a later update, and the uninstaller, may remove |
| `R/.update/` | the helper | the update lock, one folder per transaction (`journal`, `new/`, `old/`) and `recovery.log`; inside R so that every rename stays on one volume |

Those four file names and the folder `.update` are reserved: no manifest may
carry them.

**The installed version** is the HIGHER of the version `installed.json` names
and the version of the program that reads it (`update::readInstall`). The
record is the helper's, written when an update commits, and nothing else
keeps it true: a newer zip unpacked by hand over a copy that had updated
itself leaves it naming the older version. Believed alone, that record would
lower the floor against going back, and an EARLIER signed release - a later
build of the version the record names, say - would pass for a newer one and
be installed over the programs that are there. A program is never older than
itself, so a record can raise the installed version and never lower it. Only
the version is raised: the sequence, and that there is a record at all, stay
the record's. With the versions equal the sequences decide, so a copy
overwritten by hand is offered the release of its own version once more,
which changes no program and writes the records that were stale. The rule is
shown for a program below, at and above a record without a build of each
(`detail::readInstallAs`, the same reading with the program's version handed
in: `OfARecordAndAProgramTheHigherVersionIsTheInstalledOneInBothOrders`), for
this build's own version (`ARecordBelowTheProgramsOwnVersionDoesNotLowerTheInstalledVersion`),
against the order itself (`AStaleRecordUnderNewerProgramsLetsNoReleaseBetweenTheTwoBackIn`)
and at the verbs (`TheContextOfAnInstallWhoseRecordIsBelowThisProgramSaysThisProgramsVersion`).

### The three kinds

`UPDATE STATUS` says which a copy is, as `kind=`:

- **`installer`** - the per-user Windows installer put it there: the folder
  the installer recorded under `HKCU\Software\Jarada\Katana` is R. It installs
  to `%LOCALAPPDATA%\Programs\Katana` unless another folder is chosen, asks
  for no administrator rights and writes the registry under `HKCU` only
  (decision D1), so the helper replaces its files as the same user. After an
  update that proved healthy the helper rewrites `DisplayVersion` and
  `DisplayName` of its entry in Settings > Apps.
- **`archive`** - a zip or a tarball unpacked into a folder: it has
  `install.json` and the registry does not name its folder (or there is no
  registry). It is updated exactly as an installed copy is, wherever the
  person may write; it gains no entry in Settings > Apps.
- **`development`** - no `install.json`: a build tree, a plain
  `cmake --install`, the `bundle` target. It is NEVER updated: `UPDATE CHECK`
  answers `update=unsupported reason=development_tree` without asking any
  server, no automatic check runs, and the helper refuses it. Otherwise a
  developer's build would be offered a release that replaced it.
  `tools/check_install_rules.cmake` fails the suite if anything but CPack
  writes `install.json`, or if a build tree holds one.

A copy whose `install.json` is there and does not read is none of the three:
`UPDATE CHECK` answers `update=unsupported reason=install_record`, nothing is
downloaded or applied to it, and `UPDATE STATUS`, which has no word for it,
says `target=none kind=development` with a `warning` record that the install
record could not be read, and why.

**No kind is updated from a Katana that was started as administrator.** An
installed copy and an archive copy are both updated by the helper as the
person who runs Katana, from a copy of the helper in that person's update
folder (below), which every ordinary program of theirs can write. Started
from a process that holds the ELEVATED HALF OF A SPLIT TOKEN - Katana run
"as administrator" on a machine where an administrator's programs
ordinarily run with the filtered half - the helper would run with
administrator rights from that folder. So `UPDATE APPLY CONFIRM` is refused
there, `reason=elevated`, before the copy is made; the download stays, and
Katana started the ordinary way applies it ("The verbs"). A machine with
that protection turned off, and the built-in administrator account, have
one token and no unelevated half that could interfere, and are not refused
(`update::processIsElevated`, `include/katana/update/process.hpp`: Windows'
`TokenElevationType` is `TokenElevationTypeFull`; a process that cannot ask,
and every system but Windows, is "not elevated").

### The per-user folder

What belongs to the person and not to the install is in the **update state
folder U** (`include/katana/update/state.hpp`): `%LOCALAPPDATA%\Katana\update`
on Windows, `$XDG_STATE_HOME/katana/update` elsewhere
(`~/.local/state/katana/update` when that variable is not set). The
environment variable `KATANA_UPDATE_STATE_DIR` names another folder - how
every test keeps out of the folder of whoever runs the suite. It moves no
trust: nothing in U is believed without a signature check.

| In U | Holds |
|---|---|
| `state.json` | `last_check`, `auto`, `skipped` (a version) and `blocked` (the sequences of releases that were installed here and did not start: neither offered nor applied again until `UPDATE BLOCKED CLEAR` empties the list) |
| `last.json` | how the last swap ended: `status`, `reason`, `holder`, `version`, `tx`. The window shows it once at its next start and clears it |
| `stage-<sequence>/` | the downloaded files of one release at their root-relative paths, and last of all `manifest.json` and `manifest.json.sig`, whose presence means "complete"; `stage-<sequence>.lock` beside it |
| `run-<tx>/` | the copy of the helper that runs the swap. A swap that ends puts the folder on `cleanup.list` itself. The copy of a swap that was interrupted and recovered is listed there by the next start of an interactive window of that install, which learns of it from `R/.update/recovery.log` ("Recovery"). The copy of a helper that died during the health check is deleted by whoever settles that update with the right to write here - the window that confirms it, or the next swap - and listed like a recovered one after `katana_updater --recover` settled it |
| `healthy-<tx>` | the marker a new version writes to say it starts: by the health run the helper starts, or - when that helper died first - by a window of the new version at its first idle turn ("An update whose helper died during the health check") |
| `cleanup.list` | what is to be deleted and could not be at the time: what a held lock kept the helper from deleting, the helper copy of a request that nobody started (`app::abandonApply`, the one way a prepared apply is given up: `katana_cli`'s session, the window after Later or a close that was cancelled, and `main()` when the helper could not be started), and the helper copy of a recovered swap, which the window's start lists itself. An interactive window of an installed copy removes what it names at its next start; nothing else reads the list |

A headless run and `katana_mcp` are given no state folder: they read and
write none of this ("The three surfaces").

## The feed

The latest release of the repository IS the feed. No index, no mutable tag
and no API call: an installed copy reads two files of fixed names from
whichever release is marked latest, and then the update zip of the release
the manifest names (`include/katana/update/feed.hpp`):

```
https://github.com/<repo>/releases/latest/download/Katana-update-<target>.json.sig
https://github.com/<repo>/releases/latest/download/Katana-update-<target>.json
https://github.com/<repo>/releases/download/<tag>/Katana-<version>-<target>.update.zip
```

`<repo>` is compiled in: `KATANA_UPDATE_REPO`, `kjarada/Katana` by default.
`<target>` is one of `win64`, `win-arm64`, `linux-x86_64`, `linux-aarch64`
and `macos-arm64`. The release workflow marks each release it publishes with
`gh release create ... --latest`, so the flag and not a date decides which
release is the feed, and a bad release is taken out of circulation with
`gh release edit vPREVIOUS --latest`: a copy is never offered a version below
its own.

`KATANA_UPDATE_FEED` set to a base URL replaces GitHub with a host of the
owner's own: `<base>/<fixed name>` and `<base>/<payload name>`. It must
honour `Range`. With both variables empty a build has no update feed and
answers `update=unsupported reason=disabled_build`.

Each release carries, for each target, the manifest, its signature and
(except for `macos-arm64`) the update zip; their names start `Katana-`, so
the release's checksums and OpenPGP signatures take them as they take the
packages. How the Publish job makes them is `docs/release.md`, "Update
assets".

### The manifest

One signed manifest per release and target says what every file of the
install must hash to, and where each file's bytes are in the update zip
(`include/katana/update/manifest.hpp`). The unit of an update is the FILE,
decided by its SHA-256, never the version: a copy fetches the files whose
hash differs from what it has, whichever versions lie between.

This is the manifest of the tests' second release
(`tests/update/fixtures/release/v2/Katana-update-win64.json`), seven of its
ten files left out:

```
{
"applier":1,
"channel":"stable",
"expires":"2027-04-16T08:40:00Z",
"files":[
{"class":"program","csize":85,"exec":true,"offset":44,"path":"bin/katana.exe","sha256":"b70d132208d7dd410c0daba7f5fcb5a1594c844a73d499845bea1856fb6bf8da","size":207},
{"class":"lib","csize":84,"offset":288,"path":"bin/libcore-1.dll","sha256":"d03ea5d5f8851f1e01f0cdc3b24b9c514ae2808e25c0ec51ffb8a29f49d925e7","size":208},
{"class":"seed","csize":59,"offset":3046,"path":"etc/katana/settings.ini","sha256":"661fdedd66289258964b0c1e6f7eba28fff7954990b27b2b4bcb22b4d7f886ae","size":59}
],
"notes_url":"https://github.com/kjarada/Katana/releases/tag/v0.10.0",
"payload":{"codec":"deflate","name":"Katana-0.10.0-win64.update.zip","size":3791},
"product":"katana",
"published":"2026-10-18T08:40:00Z",
"schema":1,
"sequence":1792312800,
"tag":"v0.10.0",
"target":"win64",
"version":"0.10.0"
}
```

and its signature file is one line:

```
ed25519 d75a980182b10ab7 smwA9q7VhAlyA31jJzssOrk/xCfYn/QNPkzIQY7DW7FzwjKi7P2DXzjq77lR8X0IB6axvtJEIKxy16zN+N3XCQ==
```

(The key id there is the published test key's; no shipped program trusts it.)

| Member | Means |
|---|---|
| `files[].path` | root-relative, with `/`; held to the path rules below |
| `files[].class` | `program`, `lib`, `data` or `seed`: the order of the swap. A `seed` is written only where the file is absent, so what a person changed in it survives; the release workflow marks the sample projects so |
| `files[].sha256`, `size` | of the file as installed |
| `files[].offset`, `csize` | where the file's raw deflate data lies in the update zip, so that one file is fetched by one `Range` request without reading the zip's directory |
| `files[].exec` | written only when true: the file gets its execute bit where there is one |
| `payload` | the update zip: every install file at its root-relative path, each member deflated ALONE, with fixed timestamps. A standard zip, so Python's `zipfile` is an independent judge of it; it is never fetched whole |
| `version`, `sequence` | what is offered. A version is 2 to 4 numbers with dots, compared number by number (`0.10` is above `0.9`). `sequence` is the UTC time in seconds at which the release's build began, so that a second build of one version outranks the first |
| `product`, `target`, `channel` | signed, and must be `katana`, the install's own target and `stable`: a validly signed manifest of another platform is not an update for this one |
| `applier` | the least helper protocol that can apply the release (1 today). A release that needs a newer helper carries it, and the staged copy runs the swap |
| `expires` | 180 days after `published`, and ADVISORY: past it the dialog adds "The update information is old: a newer release may exist." and `UPDATE CHECK` says `information=old`. It never refuses anything, so the local clock decides no trust |
| `notify_only` | `true` for `macos-arm64`: the release is announced, with no files and no payload |

Unknown members are ignored, so a newer producer may add more. A manifest is
PARSED ONLY AFTER its signature has verified, and the parser then holds
every value to a bound: at most 10,000 files and 2 GiB installed; each
`offset + csize` inside the payload; `csize` at most the file's size plus
deflate's worst case; the whole manifest at most 8 MiB; no member twice, no
nesting deeper than 16 levels.

### The path rules

A path from a manifest is written under only if it passes one rule set
(`include/katana/update/paths.hpp`), used by the engine when it verifies a
manifest and again by the helper before it touches the install - and
enforced by the producer too, so that a release no install would accept
cannot be made. A path is relative, uses `/`, and begins with `bin`, `share`,
`etc` or `lib`. It is refused when it has an empty, `.` or `..` component; a
byte outside printable ASCII; a backslash, `:` or one of `* ? " < > |`; a
component ending in `.` or a space; a Windows device name (`CON`, `NUL`,
`COM1` ...); an 8.3 short form; or one of the reserved names. A whole
manifest is refused when two paths are equal once ASCII case is folded, or
when one path is also the folder of another.

ASCII only is a rule on purpose: what two Unicode spellings of one name mean
differs between NTFS, APFS and ext4, so a rule that said "equal after
folding" could be wrong on the machine that applies it. Every file Katana
ships has an ASCII name, and the producer refuses one that does not.

## Signing and trust

**The scheme.** Ed25519 (RFC 8032) over the manifest's exact bytes, verified
in the process by OpenSSL's EVP interface, never by running a program
(`include/katana/update/signature.hpp`). The signature file is one line,
`ed25519 <key id> <base64>`; the key id is the first 16 hexadecimal digits of
the public key.

**Two keys are pinned** at build time in `cmake/KatanaUpdateKeys.cmake`: the
ACTIVE key, whose private half signs releases, and a SPARE, which stays
offline. Either verifies. The keys reach one source file,
`src/katana_update/trust.cpp`, as compile definitions and nothing else can
read them.

**The gate** is one function, `update::verifyManifest`
(`include/katana/update/trust.hpp`), and nothing that reads a network answer
may parse a manifest any other way. In this order, stopping at the first
failure: the manifest is not empty and not over 8 MiB; the signature line
parses; this build's keys are not placeholders; the line's key id names a
pinned key; the signature verifies over the exact bytes; only now is the
manifest parsed; and its signed product, target and channel are this
install's. "Newer than what is installed" is decided after it, by
`update::isNewerThan`, which the check and the helper both call.

**The helper trusts nobody's answer.** It verifies the staged manifest again
with its own pinned keys before it touches a file, repeats the "newer" test
from the install's own record, and hashes every file it is about to put in
place. A stage folder that somebody altered is found there.

**The trust root is a parameter, never a global.** `update::productionTrust`
is what the four programs pass. Only test executables pass a test key, the
published RFC 8032 vectors, and none of them is installed:
`katana_update_tests`, `katana_update_e2e` and `katana_updater_test`, which
link the test trust root (`tests/update/test_key.cpp`), and
`katana_app_tests`, which builds the same trust from the two public keys for
the verbs' tests (`tests/app/test_update_verbs.cpp`). So no
variable, file or switch at run time changes whose signature counts, and the
test `update_test_keys_are_in_no_shipped_program` reads the bytes of the four
programs to see that no test key is in them.

**Nothing in the environment changes what "verified" means.** The helper's
first act is `update::hardenOpenSslEnvironment`: `OPENSSL_CONF`,
`OPENSSL_MODULES` and `OPENSSL_ENGINES` are removed from its process and
libcrypto is started without a configuration file
(`TheSelfTestPassesUnderAHostileOpenSslConfModulesOrEnginesAndSaysWhatItRemoved`).
It lends the person's own values back to the programs it starts.

**What the verifier refuses beyond OpenSSL.** Measured while the signature
code was written: OpenSSL 3.6.5 answered "valid" for an all-zero public key
and an all-zero signature on a one-byte message, because that key is a point
of small order. So a signature whose S is not below the group order, and a
public key that is not a canonical point of the prime-order subgroup, are
refused before OpenSSL is asked for its verdict.

**Placeholders.** Until the owner's key steps the pinned keys are 32 bytes
that spell `PLACEHOLDER-...` in ASCII and are the encoding of no curve
point, so no private key matches them. A build with them refuses every
release - it cannot be updated, and it cannot be forged for - and
`KATANA_REQUIRE_UPDATE_KEYS=ON`, which every job of the release workflow
passes, makes the configure fail while they are in place. A release can
never ship with them.

**Rotation.** Ship a build that pins a new spare beside the active key. If
the active key is lost or leaked, sign a release with the spare, built from
a tree that pins the old spare as its active key beside a new spare; every
installed copy accepts it. `tools/update_keytool.py` refuses to replace both
pinned keys at once. Lose both private keys and no installed copy can be
updated again: each has to be replaced by hand.

## What the owner does

### Once: the owner's steps

No script and no agent makes or sees a private key. These are the steps
`python tools/update_keytool.py --help` prints, and `docs/release.md`,
"Update signing", has them with the workflow around them.

1. Make the two keys, the active one that releases are signed with and the
   spare that stays offline, and back both up offline:

   ```sh
   openssl genpkey -algorithm ed25519 -out update-signing.pem
   openssl genpkey -algorithm ed25519 -out update-spare.pem
   ```

   `*.pem` and `update-signing*` are git-ignored.
2. Write their public halves into `cmake/KatanaUpdateKeys.cmake` with the key
   tool, and commit that file (public keys are public data):

   ```sh
   python tools/update_keytool.py pub update-signing.pem update-spare.pem
   ```

   The tool reads the two files only to ask `openssl` for their public
   halves, and rewrites only the block between the file's two marker lines.
3. Put the ACTIVE private key, and only that one, in the secret
   `UPDATE_SIGNING_KEY` of the GitHub environment `release` (Settings >
   Environments > New environment; add yourself under "Required reviewers"
   so that nothing is signed before you approve the run). The spare never
   goes to GitHub:

   ```sh
   gh secret set UPDATE_SIGNING_KEY --env release < update-signing.pem
   ```

   The key tool's own text gives this command without `--env release` and
   says to add it once the environment exists: make the environment first,
   so that the secret is the environment's.
4. **The first updater-bearing release is installed by hand; the next one is
   the first live update.** An installed copy is updated by the helper it
   already has, which believes only manifests signed by a key it already
   holds.

### At each release

1. Set the version in the root `CMakeLists.txt`, `project(Katana VERSION
   X.Y.Z)`, and merge it.
2. Actions > Release > Run workflow, with the tag `vX.Y.Z`. The Publish job
   runs in the environment `release`; approve it on the run's page when the
   environment names reviewers.
3. The job makes a manifest for each of the five targets and an update zip
   for each but macOS, signs each manifest with the secret, rechecks every
   hash and signature with `tools/update_verify_release.py` - which uses
   none of the producer's code - and creates the release marked latest.
   From that moment installed copies are offered it, each at its next check.

For a key that never leaves the owner's machine the workflow's input `sign`
is set to `offline`: the release is left a DRAFT, which no installed copy
can see, until `tools/update_release_sign.py` has signed it there
(`docs/release.md`, "Update signing"). Without the secret a run publishes
the packages and no update assets, with a warning: the release installs by
hand and installed copies are not told of it.

## Check and download

**The check** (`update::check`, `include/katana/update/check.hpp`) fetches
the signature, then the manifest, each bounded by the engine and not by the
server (1024 bytes and 8 MiB), and believes neither until
`update::verifyManifest` has passed. Every way of not learning the answer is
the STATUS `unreachable` with a reason token, never an error: a check that
nobody asked for then says nothing, and a typed one says one sentence. What a
check sends about the copy is its user agent, `Katana/<version> (<target>)`,
and nothing else; the server sees the computer's network address, as every
web server does, and Settings > Updates says so.

| `update=` | When | `reason=` |
|---|---|---|
| `available` | a verified release above the installed version (or the same version with a higher sequence, for a copy an update installed) | `notify_only` for a release that is only announced |
| `current` | the feed's release is not above it | `skipped` when it is, and the person skipped that version |
| `blocked` | this very release was installed here, did not start and was put back | `blocked_before`, with a `detail=` that says the way back: "this release was installed here before, did not start and was put back, so it is not offered; UPDATE BLOCKED CLEAR (Settings > Updates in the window) offers it again" |
| `unreachable` | the feed could not be read, or did not verify | `offline`, `http_404`, `http_403`, `http_429`, `http_<n>`, `html_page` (a sign-in page where the file should be), `bad_redirect` (a redirect to an address that cannot be used: none, or one that is not https), `too_many_redirects` (a sixth), `too_large`, `bad_signature`, `untrusted_key`, `placeholder_keys`, `malformed`, `wrong_product`, `wrong_target`, `wrong_channel`, `verify_unavailable`, `asset_missing`, `cancelled`, `no_transport`, and `error` for a failure the transport gave no token of its own |
| `unsupported` | this copy cannot update | `development_tree`, `disabled_build`, `install_record`, `no_network` (a build without the HTTP stack, `-DKATANA_BUILD_IO=OFF`) |

A fresh install is not offered the release it came from: it has no record
of its own sequence, so only a higher VERSION is newer for it.

**The plan** (`update::plan`) hashes every installed file the manifest
lists, FRESH each time - never from a cache, which would skip a file that
had changed and ship a mixed tree. A file is fetched when it is absent or
its hash differs; a `seed` only when it is absent. The reply's
`files=<changed>/<total>` and `download_bytes=` are this plan's.

**The download** (`update::stage`, `include/katana/update/stage.hpp`)
fetches only the changed files, by `Range` requests into the one update zip:

- The producer orders the zip's members `program`, `lib`, `data`, `seed`, so
  the changed programs lie together. Members at most 1 MiB apart are merged
  into one request, the bytes between them included - each request costs
  about 150 ms, which is what a megabyte costs at 6 MB/s (both measured
  against the release host on 2026-10-08) - and a merged range is at most
  32 MiB. So what is fetched can be more than `download_bytes=`, which
  counts the changed files alone.
- One range per request: the release's storage host answered multi-range
  requests with 501 when measured.
- The most bytes a request accepts is the range's own length, so a server
  that ignores `Range` and starts the whole zip is cut off there and
  reported as "this network does not allow partial downloads"
  (`partial_downloads`). A 206 that is another part of the file, or of a
  file of another length, is refused (`bad_range`).
- Each member is inflated - capped at its signed size - and hashed while it
  is written to a scratch name; only a member whose SHA-256 is the signed
  one is renamed to its place under `U/stage-<sequence>/`. So a name in the
  stage folder is a verified file at every instant, and a download that was
  cancelled or failed resumes file by file: what is already there and right
  is not asked for again.
- Free space is checked first: 2.6 times the changed bytes (the staged copy,
  the copy the helper makes inside the install, and the old files kept for
  rollback).
- One download at a time fills a folder; a second program of the same
  install is refused at once (`stage_busy`).

**The transport** follows redirects itself, at most five, to https
addresses only (plain http on the loopback only when the request itself
began there: a test), and rebuilds the headers for every hop: that is where
the access token's rule is kept ("The access token"). An answer whose content
type is HTML is a web page standing in for the file and is refused as such.
No address with a query string and no header is ever put in an error: the
address a release redirects to is a signed one, and all of its query is the
secret.

## The helper

`katana_updater` is the fourth program in `bin/` (not on macOS). It is
static: it links the engine and `katana_core` and imports nothing but the
operating system's DLLs, which a test reads from its import table
(`katana_updater_imports_only_os_dlls`). **It never runs from `bin/`.**
`UPDATE APPLY CONFIRM` copies it to `U/run-<tx>/` and runs `--selftest`
there (the "preflight"): the copy is what runs the swap, so at commit
nothing under R is loaded, and running it while the window is still open
proves it starts - and lets a virus scanner look at it - before anything is
closed. Nothing in it asks for administrator rights, and an install that
cannot be written as it stands is refused, never retried with more. Nor is
it started with more than it needs: a Katana that was started as
administrator is refused the apply before the copy is made
(`reason=elevated`, "The three kinds").

```
katana_updater --root R --stage S --state U --tx T --wait-pid P --relaunch <path|none>
katana_updater --recover R
katana_updater --selftest | --version | --help
```

The first form is how Katana starts it and is not meant to be typed.
`--recover R` puts back an update of the install folder R that was
interrupted, and then settles one that is installed and was never said to
start: confirms it, puts it back, or leaves it for a start that can decide
("An update whose helper died during the health check"). It is the one
command a person may have to run by hand, and every message that says a
rollback did not finish names it. The helper prints `key=value` lines, and
its exit code is its interface to a script. The swap prints `outcome=`,
`reason=`, `exit=`, a `holder=` for each blocking process and `detail=`;
`--recover` prints `recover=ok|nothing_to_do|failed`, `exit=` and `detail=`
- `ok` is a run that found a transaction folder and did not fail - and,
where this program swaps nothing ("Platform scope"), `recover=refused` with
a `reason=` line.

| Exit | Means |
|---|---|
| 0 | the update is installed and healthy; or `--recover` put everything right, had nothing to do, or met an update that was never said to start and confirmed it, put it back, or left it (its `detail=` begins with which: `confirmed:`, `put back:`, `left:`, or `busy:` when another helper holds the lock) |
| 10 | blocked: a program of the install is still running, or one was started while the files were being hashed and copied; nothing was touched |
| 11 | rolled back: the update failed and the previous version is back |
| 12 | rollback incomplete: the install may be half updated; run `katana_updater --recover R`. Also what `--recover` itself answers when it could not finish: a half-swapped tree it could not put back, or an update that was never said to start whose putting back did not finish, or could not begin (then nothing was moved, and the next run decides again) |
| 13 | refused: nothing was touched (the `reason=` line says why) |
| 20 | the command line is wrong |
| 21 | the staged release is not trusted by this program's keys, or `--selftest` failed |
| 30 | internal error |

The window does not read these: the helper writes `U/last.json`, and the
window shows that at its next start.

### The swap

`update::apply` (`include/katana/update/apply.hpp`). Every step is a test,
and the fault loops break it before each of its file operations
(`docs/testing.md`).

1. **Lock and verify.** Take `R/.update/lock`; a lock another helper holds
   ends this one in silence (`locked`). Under it, what an earlier swap left
   is settled FIRST. A transaction that a crash left unfinished is put back:
   planned over a half-swapped tree, a new update would succeed and a later
   recovery would then roll old files over it. An update that is in place
   and was never said to start - its helper died in step 7 - is confirmed
   when its marker `U/healthy-<tx>` is there, and is never put back from
   here: a swap is not the place to judge a release somebody may be running
   from. Where the first cannot be put back, the second has no marker, or
   `R/.update` cannot be read to know, nothing new is started
   (`interrupted_update`; the `detail` says which, and what settles it).
   Then verify the staged manifest with the helper's own keys -
   `staged_manifest`, `notify_only`, `applier_too_new`, `downgrade`.
2. **Wait** for the window's process to exit (60 s), then for every process
   that was started from a file under R (30 s). Past that the outcome is
   `blocked`, naming them; nothing was touched. The helper NEVER kills a
   process it did not start: a program somebody is using is waited for and
   then named. The one process it does stop is its own health run of step 7,
   when that outlasts its wait (`SpawnRequest::timeout`). The same question
   is asked once more in step 4.
3. **Plan, changing nothing under R.** Hash every file the manifest lists.
   Equal: keep. Otherwise the staged file must exist and is COPIED - not
   renamed: the stage folder is user-writable, and a file in R must take R's
   own access rights - into `R/.update/<tx>/new/`, and the copy is hashed.
   Every folder on the way must be a real folder that resolves under R: a
   junction or a symbolic link could send a write outside the install
   (`reparse_point`). A file is REMOVED only when the installed manifest -
   verified again here - lists it, the new one does not, and its bytes are
   still the ones that manifest signed. A file nobody listed is never
   touched, and a first update, which has no installed manifest, deletes
   nothing (`AFirstUpdateWithNoInstalledManifestDeletesNothing`). Any
   shortfall ends here with R untouched.
4. **Journal, and look again.** Append the PLAN group - every operation the
   transaction will make - with one flush to stable storage. Then step 2's
   SECOND SCAN. The first was over before the plan, and the plan hashes and
   copies every changed file of a release: seconds, in which a person opens
   Katana again or a script runs `katana_cli`, and a program started then
   would go on running old code over the new tree. So the processes under R
   are listed once more, with the PLAN on disk and nothing moved - one
   question and no second wait. A holder found now is the `blocked` of step
   2, exactly: the transaction's folder is deleted, the install is as it
   was, the download stays for Retry, the old version is started again and
   `last.json` names the holder. A scan that fails is no "nobody"
   (`scan_failed`). Only then `COMMITTING`, the first record after the
   answer, so that nothing is done between the two in which a program could
   start unseen
   (`AProgramStartedWhileThePlanWasMadeBlocksTheUpdateAndTheInstallAndTheDownloadAreAsBefore`,
   `TheSecondLookIsTheLastThingBeforeTheJournalSaysCommitting`,
   `AScanThatFailsAtTheSecondLookIsARefusalTooAndNothingIsTouched`). On
   Windows a `RunOnce` value, `KatanaUpdateRecover`, is then registered for
   `<run copy>\katana_updater --recover R`: a machine that loses power now
   rolls back at its next logon even if no program of the install can start.
5. **Commit, by renames only.** In the order data and seeds, libraries,
   programs, the helper's own file, the removals, and last the three records
   that say which version is installed. Each file: `R/p` to `old/p`, then
   `new/p` to `R/p`. A rename that Windows refuses with error 5, 32 or 33 -
   which a virus scanner holding a file for a moment causes as much as a
   program does - is tried again every 250 ms for 30 s. Never
   `MOVEFILE_DELAY_UNTIL_REBOOT`: a swap that completes at a reboot is a
   swap nobody can roll back.
6. **Rollback**, after any failure once the journal exists. It does NOT
   replay the journal backwards: it decides FROM THE DISK. Where `old/p`
   exists it goes back over `R/p`; a file the transaction moved in is moved
   out; a folder it created goes when empty; then `ROLLED_BACK`. So it is
   the same code after a crash at any point, the middle of an earlier
   rollback included. The old version is then started again and `last.json`
   says `rolled_back` with the reason. One state is guarded: from
   `COMMITTED_PENDING_HEALTH` NOTHING MOVES until the `ROLLING_BACK` record
   is on disk. That state alone tells recovery to leave the tree, so the
   journal must stop saying it before the tree stops being whole; where the
   record cannot be written the rollback has not begun, and says so
   (`AFailedHealthRunWhosePuttingBackCannotBeginLeavesTheWholeNewTreeAndNoMarkerToCallItHealthyBy`).
7. **Success, once the new version has shown that it starts.** Journal
   `COMMITTED_PENDING_HEALTH`, then start the new version as `R/bin/katana
   --update-health <tx>` and wait up to 60 s. That run builds its window
   without showing it, writes `U/healthy-<tx>` on the event loop's first
   idle turn - which a program that hangs while starting never reaches - and
   exits. Healthy is: the marker is there, and the run ended with 0 (or was
   still running when the wait ended, and was stopped). Anything else - it
   could not be started, it ended with an error, it never wrote its marker -
   is not a working release. A marker such a start wrote is deleted FIRST,
   so that nobody who meets the transaction later can read it as proof;
   then rollback as in 6 (`health_exit`, `health_timeout`), and its
   sequence is added to `blocked` in `state.json`, so that it is not
   offered again until a person asks (`UPDATE BLOCKED CLEAR`). When healthy:
   for an installer-kind install rewrite the version shown in Settings >
   Apps; journal `HEALTHY`; delete the transaction folder, the staged files
   and the marker; remove the `RunOnce` value; write `last.json`; and start
   Katana again as a person would. The folder and the marker go only once
   `HEALTHY` is on disk: until then they are what the update is settled by
   (`AnUpdateWhoseHealthyRecordCannotBeWrittenKeepsItsFolderAndItsMarkerAndIsSettledByThem`).
   What a lock keeps from being deleted - the helper's own running copy
   among it - goes on `U/cleanup.list` for the next start.

If the ROLLBACK ITSELF fails - a file that stays locked, a failing disk -
the outcome is `rollback_incomplete`, never "rolled back": nothing is
started, the `RunOnce` entry stays for the next logon, and `last.json` and
the dialog name the command to run, `katana_updater --recover <R>`.

If THE HELPER DIES in step 7 - between its `COMMITTED_PENDING_HEALTH`
record and its `HEALTHY` one: power, a shutdown, the process ended - the
tree is a whole new version that nobody has said starts. The `RunOnce`
entry is still registered, the helper's copy is still in `U/run-<tx>`, and
no `last.json` was written. No program's `update::recover` touches such an
update: it may be a live helper's, waiting for that very program. It is
settled under the update lock by `katana_updater --recover`, by a window of
the new version, or by step 1 of the next swap ("An update whose helper
died during the health check", under "Recovery").

The steps are numbered as `include/katana/update/apply.hpp` numbers them,
which is how the code and the other documents cite them.

### The journal

`R/.update/<tx>/journal` is append-only text, one record a line
(`include/katana/update/journal.hpp`):

```
<n> <OP> <arg>... <crc>
```

`n` counts from 1, an argument is percent-encoded, and `crc` is the CRC-32 of
the line before it. A record is complete only with its line end; reading
stops at the first line that is incomplete, has a wrong check or is out of
order, so a torn last write is ignored. The operations are `PLAN`, `ADD`,
`REPLACE`, `REMOVE`, `MKDIR`, `COMMITTING`, `MOVED_OLD`, `MOVED_NEW`,
`COMMITTED_PENDING_HEALTH`, `ROLLING_BACK`, `ROLLED_BACK` and `HEALTHY`.

**Who writes what follows `COMMITTED_PENDING_HEALTH`.** The helper that made
the swap, while it lives: `HEALTHY` once its health run has passed, or
`ROLLING_BACK` and `ROLLED_BACK` when it has not. A helper that dies in
between leaves the journal at `COMMITTED_PENDING_HEALTH`, and there is no
further kind of record for that: the SAME records are appended later, under
the update lock, by whoever settles the transaction - `HEALTHY` by
`katana_updater --recover`, by a window of the new version or by the next
swap, each on the marker; `ROLLING_BACK` and `ROLLED_BACK` by
`katana_updater --recover` alone. So a journal reads the same way whoever
finished it, and `update::needsRecovery` is as it was: `PLAN`, `COMMITTING`
and `ROLLING_BACK` are what a crash left; `COMMITTED_PENDING_HEALTH` is not.
And while a journal says `COMMITTED_PENDING_HEALTH` it is TRUE of the disk:
the tree is the whole new version, because a rollback from that state moves
no file before its `ROLLING_BACK` record is written; every old file is still
in the folder, and the marker is there exactly when the new version started
well, because the folder and the marker are deleted only after `HEALTHY` is
written, and the marker of a start that then failed before the rollback
begins.

The journal is NOT authenticated: it lies in a folder a process of the same
user can write, and its check finds a torn write, not a forged line. So a
path read back from it is acted on only when it passes the manifest's own
path rules, and a record that fails is skipped and written to
`R/.update/recovery.log`. A forged journal can at worst make a rollback do
less; it cannot reach outside R's `bin`, `share`, `etc` and `lib`
(`APathThatLeavesTheInstallIsSkippedAndNothingOutsideIsTouched`).

### Recovery

`update::recover(R)` (`include/katana/update/recover.hpp`) is the FIRST thing
`main()` does in `katana`, `katana_cli` and `katana_mcp`: a machine that lost
power, or a helper that was killed in the middle of a swap, is put right the
next time anything starts. With nothing interrupted - every start but that
one - it costs the listing of one folder, and nothing at all in a copy that
has never updated.

| The journal says | Recovery |
|---|---|
| nothing complete, `PLAN`, `COMMITTING`, `ROLLING_BACK` | a crash left it half done: rolled back, by running the helper copy `U/run-<tx>/katana_updater` (it is outside R, so it works when R's own `bin/` is half swapped), else `R/bin/katana_updater`, with `--recover R` |
| `COMMITTED_PENDING_HEALTH` | left exactly as it is, with no lock asked for and nothing started: the tree is a whole version, and the helper that owns the folder may be alive and waiting for the very program that asks. When that helper has died, it is settled by others ("An update whose helper died during the health check", below) |
| `ROLLED_BACK`, `HEALTHY` | over: what is left of the folder is deleted |

An update that is running now holds the lock; recovery waits up to 20 s for
it and then leaves it alone. A recovery that fails is said on stderr with the
command to run by hand, and the program goes on.

`katana_updater --recover` is started by headless programs too, so it writes
NOTHING per user - no `last.json`, no `state.json`, no `cleanup.list`, no
health marker - whoever started it. It READS U for one question, and only
when the install holds an update that was never said to start: whether that
update's marker `U/healthy-<tx>` is there, and its helper copy `U/run-<tx>`
(below). Its one record is a line in `R/.update/recovery.log` for each run
that found something to put right.

`update::recover` keeps the same rule in the program that called it. It
READS U, to find the helper copy `U/run-<tx>/katana_updater`, and that is
all it does there: it makes, changes and deletes nothing in U, whichever
program calls it. No `cleanup.list` is begun for what a lock kept in
`R/.update` - the next start of any program finds that folder again and
deletes what is left of it - and the helper copy it ran is left by recovery:
the window's next start removes it.
`RecoveryStartedTheWayAHeadlessProgramStartsItWritesTheRecoveryLogAndNoPerUserState`
compares U before and after, entry by entry and byte by byte.

**The copy a recovery ran from** is removed by the one program that sweeps
U, where it removes what `cleanup.list` names: an interactive window of that
install, at its next start (`UpdateWorkbench::start`, on Windows, where an
update can have run). `katana_cli` keeps per-user state too, and sweeps
nothing. So until that start the copy - one folder holding one program -
stays, which is for as long as only `katana_cli`, `katana_mcp` or a headless
window is started. Nothing in U names the copy - a swap that a crash ended
never listed its own, and one whose rollback failed keeps its copy off the
list on purpose, for the `RunOnce` entry to run - so the INSTALL names it.
The window reads `R/.update/recovery.log`, in which every `--recover` that
found something wrote a line

```
<UTC time> recover found=<tx,...> result=ok|failed left=<tx,...>
```

(a recovery that failed ends its line with `detail=` and the reason), and
takes each transaction of a `found=` list whose folder `R/.update/<tx>` is
gone: that update is over, rolled back or healthy, and nothing starts its
helper again. Its `U/run-<tx>` goes on `cleanup.list`, and the sweep of the
same start removes it. A transaction whose folder is still there keeps its
copy - a rollback that failed runs from it at the next try. A copy that is
gone already, which is every start but the first after a recovery, is not
listed again.

The log is no more trusted than the journal beside it. A line counts only
when it is whole, ended by its line feed, and has that form as far as
`result=`, after which nothing is read - the same file quotes the journal
records a rollback refused, whose text is anybody's - and an id counts only
when it is a transaction id
(`update::isValidTransactionId`), so a line names no place but `U/run-<id>`.
Which transactions a log names is `updateRecoveredTransactions`
(`src/katana_qt/update_dialog.hpp`), a function of the log's text and of
which folders are gone
(`TheHelperCopyOfARecoveredUpdateIsListedOnlyOnceItsTransactionIsOver`,
`OnlyAWholeLineThatARecoveryWroteNamesATransaction`,
`WhatIsNoTransactionIdInARecoveryLineNamesNoFolder`). Reading the file - not
through a link, and not one longer than 1 MiB - and listing the copies are
the workbench's (`src/katana_qt/update_workbench.cpp`), which no test builds
("Not done").

*Rejected: sweeping every `run-<tx>` in U that no transaction of this
install owns.* U is one folder for every copy of Katana a person runs, and
such a folder is also what `UPDATE APPLY CONFIRM` has just made, here or in
another install, for a helper that has not started yet. Only an id that this
install's own recovery named is known to be this install's, and begun.

What is left: a COPY of an install folder made while an update in it was
half done carries the transaction with it. Once the copy has been put right,
its window removes the helper copy the original would have run, and the
original's recovery then runs its own `R/bin/katana_updater` - as recovery
does whenever that copy is gone (the table above).

**The residual.** A program started during the commit - about a second -
may fail to load, half its libraries being new; it is started again. And a
program of the install that another Windows account is running cannot be
seen by the scan, so the update goes through under it: Windows lets the file
of a running program be renamed, and renames are all the commit does. That
program runs on as old code until it is closed. A per-user install is a
folder of one profile, so this takes an archive unpacked where several
accounts reach it (`src/katana_updater/process_scan_win.cpp` gives the
reasons the file's name alone is not held on).

### An update whose helper died during the health check

The helper installs a release and then waits, up to a minute, for the new
version to prove that it starts (step 7). A helper that dies in that wait -
power, a shutdown, the process ended - leaves an update that is INSTALLED AND
WAS NEVER SAID TO START:

- the journal at `COMMITTED_PENDING_HEALTH`, the tree a whole new version,
  and every file the commit moved aside still under `R/.update/<tx>/old/`;
- the `RunOnce` entry of step 4, still registered;
- the helper's copy in `U/run-<tx>/`, on no list, and the marker
  `U/healthy-<tx>` when the health run got as far as writing it;
- no `last.json`, so nothing for a window to show.

Recovery is right to leave that tree alone, and until 2026-10-14 nothing
else settled it but a later update that proved healthy: `--recover` and the
`RunOnce` entry did nothing with it, so a release that could not start
stayed installed until the person reinstalled. It is settled now, by
evidence and never by assumption.

**One rule** (`detail::settlePending`, `src/katana_update/recover_internal.hpp`)
for every transaction whose journal says `COMMITTED_PENDING_HEALTH`. Its
caller holds the update lock. A helper holds that lock from its first step
to its last and it goes with the process, so holding it is how a settler
knows that no helper is alive and waiting on the transaction.

| Settled as | When | What is done |
|---|---|---|
| `HEALTHY` | `U/healthy-<tx>` is a file: the proof the swap itself accepts, whoever wrote it | for an installer-kind install the version shown in Settings > Apps is rewritten (the swap writes it only once the proof is there, and may have died before); `HEALTHY` is appended, one flushed record; then the folder is deleted, its journal last |
| `ROLLED_BACK` | only for the one caller that may put back, and only when ALL of these hold: there is no marker; U holds `run-<tx>`, the dead helper's copy, so U is the folder the update was applied from and the one the marker would be in; every file the PLAN's `REPLACE` and `REMOVE` records moved aside is still a file under `old/`; the tree is still that release (`detail::treeIsRelease`: `installed.manifest.json` verifies with the settler's own keys, its sequence is the PLAN's, and every file it lists that is no seed hashes as listed); and the process scan - asked before the hashing, and again as the last thing - answers and names nobody but the settler itself | `update::rollback`, as step 6 does it |
| left as it is | every other case: a marker that cannot be looked for, a scan that fails, a tree somebody changed, old files that are not all kept, another per-user folder, a program of the install running | NOTHING is written: no journal record, no file, no registry value |

"No marker" and "put back" are claims too, which is why the rule looks
before it believes either. A marker missing from ANOTHER folder than the one
the new version writes to - `KATANA_UPDATE_STATE_DIR` set for Katana and not
for this run, another account - says nothing about whether the release
started (`AMarkerMissingFromAnotherPerUserFolderIsNotProofThatTheReleaseNeverStarted`).
A rollback puts back what is under `old/`, so with one of those files gone
it would leave a mix and call it the old version. And nothing is ever put
back under a running program, or over files that are no longer that release
(`NothingIsPutBackUnderARunningProgramOrOverFilesThatAreNoLongerThatRelease`,
`TheProgramsAreAskedForBeforeTheReleaseInTheTreeIsReadAndAgainAfterItAndNothingChangesUntilThen`).

**Three callers**, which differ only in what they may do
(`include/katana/update/recover.hpp`):

1. **`katana_updater --recover R`** - the `RunOnce` entry at the next logon,
   or a person. It first does what it always did, unchanged
   (`update::recoverInProcess`), and when that is OK calls
   `update::settleUnproved`. With no such update: nothing, and no lock is
   asked for. Otherwise the lock is polled as recovery polls it, up to 20 s;
   a lock that stays held is a live helper's health run, which is not this
   run's to judge: `busy`, with nothing written, the registry included. Then,
   FIRST, the `RunOnce` entry is registered again, for the helper that is
   running and R - the very text the swap registered, when it was that entry
   which started the run: Windows removes an entry before it runs it, so a
   run that lost power in the next lines would otherwise leave no logon to
   come back at
   (`TheLogonNetIsPutBackFirstAndTheVersionIsRewrittenBeforeTheRecordIsWritten`).
   Then the rule, putting back allowed. Then the entry is removed when no
   transaction folder is left under `R/.update`; an update that was left, or
   a putting back that did not finish, keeps it. (A lock that cannot be
   opened at all is no helper's: the update is left, with the entry
   registered again.) U is only READ - the marker stays where it is - so the
   headless rule above holds for this too
   (`AnUpdateWhoseMarkerIsThereIsSettledHealthyAndThePerUserFolderIsOnlyRead`).
   The run prints `recover=ok`, exit 0, and a `detail=` that begins with
   what became of the update - `confirmed:`, `put back:`, `left:` or `busy:`
   - or `recover=failed`, exit 12, when the putting back did not finish or
   could not begin.

   What such a run cannot record is NOT recorded: a putting back made here
   writes no `last.json` and does not add the release to `blocked`, so the
   next check offers that release again. No apply-and-put-back loop follows:
   nothing applies an update but a person asking for it, and an update a
   person applies runs under a live helper, whose own health check blocks a
   release that does not start.
2. **A window of the new version**, on its first idle turn and before the
   automatic check (`UpdateWorkbench::start`). A window that has come idle is
   exactly what the health run exists to show, so it IS the proof - when it
   is that version. The call is made only in an interactive window (it
   writes per-user state), on Windows, of an installed copy that has an
   install record and whose installed version is the program's own
   (`updateStartMayConfirm`, with the record read again at that moment).
   What that refuses is a window of the OLD files that was started during
   the commit: the record then names a version above its own, and its start
   proves nothing about the release. `update::confirmStarted` then: with no
   such update, nothing - a listing of one folder. One attempt at the lock,
   since a window does not wait; while it is held the window asks again
   every 5 s, 12 times - the minute a helper waits at most - and then leaves
   it to the next start or the next logon. Else the marker `U/healthy-<tx>`
   is written FIRST, so that a crash from there on is settled healthy by the
   next `--recover` too; the rule settles the update healthy, and never puts
   anything back; the marker and `U/run-<tx>` are deleted, what will not go
   being put on `cleanup.list`; and the `RunOnce` entry is removed. Nothing
   is shown and nothing logged: the update did work
   (`docs/desktop.md`, "An update that was never said to start").
3. **The next swap**, in its step 1, after it has put back what a crash
   left: the rule with the request's own U, and never a putting back.
   Settled: the marker and `U/run-<tx>` go, and the dead helper's `RunOnce`
   entry with them. Left: `interrupted_update`, and nothing is touched. So an
   install holds ONE such update at most
   (`AnEarlierUpdateWhoseMarkerIsThereIsSettledFirstWhateverBecomesOfTheNextOne`,
   `AnEarlierUpdateThatWasNeverSaidToStartStopsTheNextOneBeforeItTouchesAnything`).

`update::recover` itself - the first act of every program, the health run
included - is unchanged and asks for no lock over such an update
(`AnUpdateThatWasNeverSaidToStartIsLeftExactlyAsItIsWithNoLockAndNoHelper`):
that is what keeps the health run's own recovery from waiting on, or
starting again, the helper that started it. And none of the three settlers
starts a program.

**What comes of it, case by case.**

- *Power is lost again while it is settled.* Before `HEALTHY` is on disk the
  next settler decides again from the same evidence; after it any start
  sweeps what is left of the folder; and a putting back that was cut off is
  finished by every start, as any rollback is - `ROLLING_BACK` is a state
  recovery puts back. Three loops die before every change to the disk of a
  healthy settlement, of a putting back and of a window's confirming, and
  require the whole tree to be one version's afterwards ("The update
  system's tests" in `docs/testing.md`).
- *The window, `katana_cli` and `katana_mcp` are started together.* Each
  one's `update::recover` returns at once, and the window confirms.
- *A helper is alive, in its own health run.* It holds the lock: `--recover`
  answers `busy` and a window asks again; the helper settles its own update.
- *Katana is started at a logon before the `RunOnce` entry runs.* Its window
  confirms the update; or the scan names it as a program of the install, and
  `--recover` leaves the update for the next logon, the entry registered
  again.
- *A program's own recovery starts `--recover`* (it found a half-swapped
  tree). That program runs from R, so the scan names it and nothing of this
  kind is put back under it.

What this leaves open is under "Not done".

## The three surfaces

One grammar, one implementation (`app::runUpdateLine`): a dialog builds the
line a person would type, and `HELP UPDATE` prints the family.

### The verbs

| Line | Reply |
|---|---|
| `UPDATE CHECK` | `update=available\|current\|unreachable\|unsupported\|blocked installed=<v> latest=<v> files=<changed>/<total> download_bytes=<n> notes_url=<url> reason=<token> detail="<sentence>"`; `information=old` when the release's `expires` has passed |
| `UPDATE STATUS` | `installed=<v> target=<t\|none> kind=installer\|archive\|development staged=<v\|none> skipped=<v\|none> auto=on\|off last_check=<UTC\|never> token=present\|absent last=<status>[:<reason>]` |
| `UPDATE DOWNLOAD CONFIRM` | `progress bytes=<done>/<total> files=<done>/<total>` lines, then `staged=<v>`. With nothing to download: `UPDATE CHECK`'s record, which says why, then `staged=none`. In a session that keeps no update folder: `staged=none reason=no_state`, and nothing is asked of the feed. A download that FAILS is an error, not a reply (below) |
| `UPDATE APPLY CONFIRM` | `ready=<v> transaction=<tx>`; the caller then ends its session and starts the helper |
| `UPDATE SKIP <version>`, `UPDATE SKIP NONE` | `skipped=<v\|none>` |
| `UPDATE SET AUTO ON\|OFF` | `auto=on\|off` |
| `UPDATE TOKEN STATUS`, `UPDATE TOKEN CLEAR` | `token=present\|absent`, and never the token |

A key is written only when it has a value, so a reply without `reason=` is
one that was carried out. What a line could not do but did not stop for
follows its record as `warning text="..."`.

A v1 install asking about the tests' second release, and then downloading it
(the expectations of `tests/app/test_update_verbs.cpp`, worked out there from
the fixtures):

```
> UPDATE STATUS
installed=0.9.0 target=win64 kind=archive staged=none skipped=none auto=on last_check=never token=absent last=none
> UPDATE CHECK
update=available installed=0.9.0 latest=0.10.0 files=5/10 download_bytes=339 notes_url=https://github.com/kjarada/Katana/releases/tag/v0.10.0
> UPDATE DOWNLOAD CONFIRM
progress bytes=2949/2949 files=5/5
staged=0.10.0
> UPDATE APPLY CONFIRM
ready=0.10.0 transaction=<32 hexadecimal digits>
```

(The progress line shown is the last of them: a download says one each time
another tenth has arrived. Its bytes are what is FETCHED, 2949 here, and
`download_bytes=339` is what the five changed files weigh in the zip: they
lie within a megabyte of one another, so one request fetches them with the
unchanged files between them.)

- **`CONFIRM` is a word of the line, not a question asked back.**
  `UPDATE DOWNLOAD` or `UPDATE APPLY` without it is refused, saying what the
  line with it would do, so the word is typed knowing it
  (`cli.update_download_without_confirm_is_refused_saying_what_confirm_does`).
- **A download that fails is an error, with `reason=` first in its
  context**, as a refused `APPLY CONFIRM` is: `plan_failed` (the installed
  files could not be compared, so nothing was asked for), `low_disk_space`,
  `stage_busy`, `write_failed`, `staged_corrupt` and `staged_hash` (a member
  that does not unpack, or is not the signed file), `short_range`, the
  private route's `no_token`, `asset_missing`, `payload_mismatch` and
  `malformed`, `platform` where this version does not install, or the
  transport's own token: `offline`, `http_<n>`, `html_page`,
  `partial_downloads`, `bad_range`, `bad_redirect`, `too_many_redirects`,
  `cancelled` (`include/katana/update/stage.hpp` lists the engine's). What
  was staged before the failure stays, and the next download does not ask
  for it again.
- **`APPLY CONFIRM` changes nothing in the install.** It finds the newest
  complete download whose records verify, runs the preflight, and hands its
  caller the request to start the helper with. It is refused, with
  `reason=` first in the error's context, as `not_staged`, `blocked_before`,
  `development_tree`, `install_record`, `no_state`, `state_unreadable`,
  `platform`, or with the preflight's token (`helper_missing`,
  `helper_not_started`, `helper_timeout`, `helper_failed`, `write_failed`).
- **`last=`** is how the last swap ended: `committed`, `blocked`,
  `rolled_back`, `refused` or `rollback_incomplete`, with the helper's reason
  after a colon (`last=rolled_back:health_exit`).
- **`last_check`** is recorded when the feed ANSWERED - a manifest verified -
  and not for a check that could not read it, so that an automatic check
  that failed is due again.

### katana_cli

Runs every line. **`UPDATE APPLY CONFIRM` must be the last line of a run**:
the run ends, the session with it, and only then is the helper started,
which waits for the process to exit; nothing is started again afterwards. A
script or a `-c` batch with a line after it is refused before anything is
prepared
(`cli.update_apply_confirm_that_is_not_the_last_line_is_refused`); typed or
piped on standard input, where the next line cannot be looked at without
waiting for it, the session ends with the line and says so. How the swap
ended is `last=` of `UPDATE STATUS` in the next run.

### The window

Help > "Check for Updates..." (`helpCheckForUpdates`) and the status-bar
button `statusUpdateAvailable` open the dialog `updateDialog`
(`docs/desktop.md`, "Help > Check for Updates", has every state, sentence
and button). Its buttons build the lines above and hand them to the window's
one executor, so a `DOWNLOAD` or an `APPLY` typed on the command line goes
exactly the way the buttons go. The three lines that take time - `CHECK`,
`DOWNLOAD CONFIRM` and `APPLY CONFIRM`, whose preflight runs the helper's
self-test - run as background jobs.

- **The automatic check** runs only when ALL of these hold: the window is
  one a person uses, never headless; the copy was installed by packaging;
  the state's `auto` is on; a check is due - 24 hours after the last and up
  to 2 hours more, a jitter drawn once per process so that a release does not
  meet every copy at the same minute; and the environment variable
  `KATANA_UPDATE_DISABLE` is not set to anything. A window left open looks
  once an hour whether one has come due. It is quiet when it learns nothing.
- **One click.** Update is `UPDATE DOWNLOAD CONFIRM`, then `UPDATE APPLY
  CONFIRM`, then the window's ordinary `close()`. ONLY when that close
  succeeded does `MainWindow::takePendingUpdate` hand `main()` the request,
  after the event loop has ended, and `main()` starts the helper as its last
  act. A close that was cancelled, or a window the person closes themselves,
  installs nothing, and the release stays downloaded.
- **Settings > Updates** (`settingsUpdates`): "Check for updates
  automatically" (`settingsUpdateAuto`, which runs `UPDATE SET AUTO ON` or
  `OFF`), what a check sends (`settingsUpdateSends`), the access token
  (`settingsUpdateToken`, Save and Clear), and "Offer the skipped version
  again" (`settingsUpdateUnskip`, `UPDATE SKIP NONE`).
- **At a start** an interactive window lists in `cleanup.list` the helper
  copy of each update of its install that a recovery put right
  ("Recovery"), removes what the list names, shows and clears `last.json`,
  and offers again, as Ready, a release that was downloaded in an earlier
  session - unless it was skipped since, or rolled back here. A development
  copy does none of this: it shares the per-user folder with an installed
  copy on the same machine and must leave it alone.

### katana_mcp

One tool, `katana_check_update`, read-only and open-world
(`hints(true, false, true, true)`): it runs `UPDATE CHECK` and `UPDATE
STATUS` for the surface `UpdateSurface::Mcp` and returns the two records, as
text and as the objects `check` and `status` (`docs/mcp.md`, "Updates").
**An agent never downloads and never applies**: applying replaces the
programs of the install, `katana_mcp` among them, and the client that
started the server holds it open. Every `UPDATE` line sent to
`katana_run_commands` or `katana_run_script` is refused - `CHECK` and
`STATUS` included, so that the tool that runs lines can go on telling its
client that it reaches no network - naming `katana_check_update`. The tool
keeps nothing per user: no state folder, and no credential store but the
environment variable of its own run.

### A headless run

A headless window - a run with `--screenshot`, `--plot`, `--plot-sheets` or
`--sheets-json`, or a `--script` batch - takes no part unless told to
(`docs/headless.md`, "Updates in a headless run"). A run with `--command`
alone is NOT headless: it is a window a person may use, which is why the
automatic check also asks for an install record, and a build tree stays
offline whatever it is started with.

- no automatic check, and no dialog unless `--dialog` asks for it;
- no per-user state read or written: its verbs' context has no state folder
  and no credential store;
- `UPDATE CHECK` and `UPDATE STATUS` are answered when typed;
- `UPDATE DOWNLOAD CONFIRM` and `UPDATE APPLY CONFIRM` are answered
  `staged=none reason=headless` and `ready=none reason=headless`, with a
  sentence naming who does run them; `SKIP`, `SET AUTO` and `TOKEN` are
  refused as `Unsupported`, `reason=headless`.

Two things every run does, headless or not: `update::recover` first, and the
switch `--update-health <tx>`, which is the helper's question and no
person's.

## The access token

Not needed: `kjarada/Katana` is public, and the default feed is read without
one. The token exists for a release repository that is PRIVATE, where a
person who holds a read-only token can still update. It is the one secret
the update system can hold (`include/katana/update/credential.hpp`).

- **Where it is kept.** On Windows in the Credential Manager, as a generic
  credential named `Katana/update-token`. Everywhere, the environment
  variable `KATANA_UPDATE_TOKEN` is read FIRST and overrides it; on Linux and
  macOS, and for a headless window and `katana_mcp`, the variable is the only
  source. It is never compiled into a program.
- **How to set it.** In the window, File > Settings > Updates: type or paste
  it into the token field and press Save. Save writes the credential store
  DIRECTLY and runs no line; the field is emptied and nothing of the token is
  logged or shown. For a script, or on Linux and macOS, set the variable in
  the environment that starts Katana.
- **It is never a word of a line.** Lines are echoed in the command log, in
  a script's output and on a headless run's stderr, so there is no `UPDATE
  TOKEN SET`. `UPDATE TOKEN STATUS` says `token=present` or `token=absent`;
  `UPDATE TOKEN CLEAR` removes the stored one (and says
  `reason=environment` when the variable still gives one, which is not the
  program's to remove). A token typed on a line all the same is refused
  without being repeated, and the window echoes such a line with `***` in
  its place.
- **Where it is sent.** Nowhere while the public address answers. Only when
  the first request - the signature, at the public address - answers 404,
  which is what a private repository answers an anonymous read, the build's
  feed is GitHub and a token exists, does the check go ONCE through the REST
  API instead: `api.github.com/repos/<repo>/releases/latest`, then the two
  assets by their ids; a download later finds the zip through
  `releases/tags/<tag>`. The token is sent as `Authorization: Bearer` over
  https to exactly `api.github.com`, on the first hop of a request alone.
  The API answers an asset request with a redirect to a storage host; the
  transport follows it, re-sends `Range`, and sends NO token there - nor to
  any other address a server answered with, `api.github.com` itself
  included
  (`TheTokenReachesTheApiHostOnTheFirstHopAndNotTheStorageHostItRedirectsTo`,
  `ARedirectNeverCarriesTheTokenNotEvenBackToTheApiHost`).
- **It is in nothing that is said.** Every reply, error and progress line
  passes through `update::redactToken`, and an error names a token as
  "token" and nothing else.
- **What a token may be**: one word of at most 2560 bytes - what the
  Credential Manager holds - with no control character and no space inside.
  The white space a token pasted from a web page arrives with is taken off.

## Platform scope

| Target | Gets | Why |
|---|---|---|
| Windows x86-64 | everything: check, download, swap, rollback, recovery | the file system, the launcher, the process scan and the registry are written for Windows (`fsops_win.cpp`, `spawn_win.cpp` in `src/katana_update/`; `process_scan_win.cpp`, `registry_win.cpp` in `src/katana_updater/`) |
| Windows on ARM64 | the same code | it has not been built or run on ARM64: the machine this was written on has no ARM64 toolchain. The release workflow's ARM64 job is the first place it meets one ("To verify on first release") |
| Linux x86-64, aarch64 | check and notify: `UPDATE CHECK` and `UPDATE STATUS` work, and the dialog says "This copy cannot update itself: the release notes say where to download it." | the apply's file system and launcher are not written for Linux (`src/katana_update/fsops_posix.cpp`, `spawn_posix.cpp`: every operation is one refusal). `UPDATE DOWNLOAD` and `UPDATE APPLY` are refused, `reason=platform`, before a request is sent; the helper, which the package carries, refuses a swap and a recovery the same way (exit 13) and writes nothing. The manifests and update zips are published all the same |
| macOS on Apple silicon | check and notify, from a notify-only manifest: `update=available reason=notify_only` | a file changed inside `Katana.app` breaks the seal of the bundle, so nothing is ever replaced there: the package holds no helper, and the refusal stands for good, whatever is later written for Linux |

## To verify on first release

What could not be tested without GitHub, and so has only been tested
against a fake of it on the loopback address. The workflow's own list - the
key check, the jobs' output, the commands that fetch a byte range and
recheck what GitHub serves - is `docs/release.md`, "To verify on the first
release"; this is what the UPDATER has to be seen doing. A rehearsal - two
versions published in a throwaway repository, the second installed as an
update of the first - is the owner's acceptance step and the gate for the
first real release; no agent creates a repository, a release or a secret.

- **The real feed addresses, and the latest flag.** That
  `https://github.com/<repo>/releases/latest/download/Katana-update-<target>.json`
  and its `.sig` reach the files of the release `gh release create --latest`
  flagged - the flag, not the newest date - and that after
  `gh release edit vPREVIOUS --latest` they reach the earlier release's.
- **`Range` on the storage host.** That a ranged request for the update zip,
  through GitHub's redirect to its storage host, is answered 206 with the
  `Content-Range` asked for, by Katana's own transport and not only by
  `curl` (with which single ranges were measured on 2026-10-08, and
  multi-range requests answered 501).
- **The shipped transport itself.** The programs fetch through
  `app::makeUpdateFetch`: the engine's rules over libcurl
  (`gis::httpExchange`). The rules are tested to the letter and end to end,
  but the end-to-end tests fetch with a client of their own on plain
  sockets, which is what keeps them on the machine; the libcurl half meets
  a server only in two tests that need `KATANA_ONLINE_TESTS=1` (a redirect
  is handed back and not followed; no credential configured for GDAL is
  sent), and no test gives it a `Range` answer or a body to cut off at its
  bound. So the first release is the first time the transport a person's
  copy uses fetches a byte range (`docs/testing.md`, "The update system's
  tests").
- **The redirect that must not carry the token.** On the private route, that
  `api.github.com/repos/<repo>/releases/assets/<id>` answers 302 to a signed
  storage address, that the storage host serves it WITHOUT an `Authorization`
  header - the transport sends none there - and that `Range` passes through
  that redirect. Also unverified there: whether a fine-grained token can be
  issued for a private repository a person merely shares, and the API's
  quota in practice (60 requests an hour anonymously, 5000 with a token).
- **A renamed repository.** `api.github.com` answers a renamed repository
  with a 301 to its new address, and the hop that follows it carries no
  token, so a private repository then answers 404. Whether that redirect
  should carry the token is the owner's to rule; as written, the person
  corrects the repository's name.
- **A real signing run.** That the Publish job, in the environment
  `release`, signs each manifest with the secret through the runner's
  `openssl`, and that the recheck then prints `signature verified by key
  <key id> (active)` for each. No signature has been made with a real key.
- **The helper on a real install.** The static helper on Windows ARM64; the
  copy in `U/run-<tx>` under a virus scanner and SmartScreen, unsigned as it
  is until the repository holds a code-signing certificate; the `RunOnce`
  entry at a real logon; `katana --update-health` in an installed copy; and
  how long the swap and the health run take.
- **The installer's acceptance step**: an install, an update and an
  uninstall on a clean Windows machine, by hand (`docs/release.md`, "The
  Windows installer").

## Decisions

Lettered as the owner ruled them on 2026-10-08; tests and build files cite
them by letter.

- **D0, distribution.** `kjarada/Katana` is public and is the default feed
  (`KATANA_UPDATE_REPO`), so no token is needed. The private-repository
  route stays, optional, for a repository that is private. Every build
  carries the tracked built-in customisations compiled in, so an update
  distributes them as every package does; that they are distributed is the
  owner's decision (`docs/customisation.md`, "Why the built-in is in the
  repository").
- **D1, the Windows installer is per user**: `HKCU`,
  `%LOCALAPPDATA%\Programs\Katana`, no administrator prompt. *Rejected: an
  elevated instance of the helper for a per-machine install* - a prompt at
  every update, and every step that crosses the privilege boundary is one
  more way for an update to go wrong. No release existed, so nothing was
  migrated.
- **D2, the automatic check is on by default**, checks and does not
  install, is quiet on failure, runs in the window only and never headless.
- **D3, CI signs** with the secret `UPDATE_SIGNING_KEY` of the environment
  `release`. *Rejected as the default: signing by hand at every release*,
  for the friction; it stays as `sign=offline` for a key that never leaves
  the owner's machine. No agent generates or handles a private key.
- **D4, a test may start its own server on the loopback address** and runs
  without `KATANA_ONLINE_TESTS`; a test that reaches any other host does not
  (`docs/testing.md`).
- **D5, the Windows toolchain is not frozen.** An update between two
  releases built a month apart therefore fetches the libraries that changed
  as well as the programs.

And the engineering decisions, each with what was rejected:

- **The unit is the file, by SHA-256, never the version.** What a copy
  fetches is decided by comparing hashes, so it is one rule whichever
  version the copy comes from; and two builds of one version, which differ
  in bytes and not in version, are told apart by their sequence.
- **A standard zip read by `Range`.** *Rejected: a pack format of our own*
  (it would have no independent reader to judge it by) *and zstd packs*
  (their gain was not measured). *No binary deltas*: the saving was
  estimated from guesses stacked on guesses; they are worth writing only if
  a measurement shows a threefold gain.
- **`releases/latest/download` and an explicit `--latest`.** *Rejected: a
  mutable `feed` tag*, which can disagree with a release, *and the REST API*
  as the ordinary route: 60 requests an hour for each network address
  without a token.
- **OpenSSL's EVP for Ed25519**, which the toolchain already carries.
  *Rejected: Monocypher* (a new library to vendor) *and libsodium* (a new
  DLL).
- **The helper runs from a copy in U.** *Rejected: running it in place*,
  which would rest on renaming the image of a running program, for which
  the design found only informal evidence.
- **Rollback decides from the disk.** *Rejected: replaying the journal
  backwards*: a decision from the disk is the same after a crash at any
  point, a crash inside a rollback included, and the journal is not
  authenticated.
- **The trust root is a parameter**, and only the test executables pass a
  test key. *Rejected: a test key in a shipped program*, behind whatever
  switch: nothing in the environment may move the trust root.
- **The health run exits.** The design had the relaunched window write the
  marker and carry on. As built, the helper WAITS for the copy it started
  with `--update-health` and stops one that is still running when its wait
  ends, so a window that carried on would be closed under the person a
  minute later. The health run is therefore a window that is built, never
  shown and gone again, and the helper then starts Katana as a person would.
- **Settings > Apps is rewritten after the health check**, not before:
  written before, a release that is rolled back would still be the one the
  person is told they have.
- **The token goes to no address a server answered with**, `api.github.com`
  included: a host that is trusted with a download is not thereby trusted to
  aim an authenticated request at a path of its choosing.
- **Redirects are followed by the engine, one request at a time.**
  *Rejected: letting the HTTP library follow them*: it re-sends the
  request's headers wherever it is sent, and GDAL's fetch has no switch to
  stop it. So the project's HTTP layer gained one request that follows
  nothing (`gis::httpExchange`), on the same libcurl GDAL loads.
- **The reserved names are four literal names.** The design's pattern
  `installed*.json` was narrowed: the helper writes exactly those four, and
  `installed.json.bak` is a file like any other.
- **`expires` is advisory.** *Rejected: refusing an expired manifest*: the
  local clock would then decide trust, and a machine with a wrong date could
  not be updated.
- **An agent checks and reports.** *Rejected: letting `katana_run_commands`
  run the two read-only lines*: that tool tells its client that it reaches
  no network, and one line that asked a server would make that false for
  every call.
- **Rejected as small gains for more states:** ETag revalidation, parallel
  download workers, release channels, a fallback that fetches the whole
  payload, installing at exit, and a `ROLLBACK` verb.

GitHub's own limits bound what a release may hold: "Up to 1000 release
assets may be associated with a single release" and "Each file included in a
release must be under 2 GiB" (GitHub Docs, "About releases", "Storage and
bandwidth quotas", read 2026-10-13). The reader of a release's asset list
(`update::parseReleaseAssets`) takes at most twice the thousand. An update
zip is one file of a release, so on GitHub it is bounded by the 2 GiB,
whatever more the manifest's parser allows (4 GiB).

## Not done

- **Linux does not install an update** (the design's task T18): the apply's
  file system (`rename(2)`, `fsync` of the directory, `flock`), its launcher
  and the scan of `/proc` are not written. Open with it: a Linux helper
  cannot be static as the Windows one is, and it runs from a copy in the
  state folder, where it must still find the install's `lib/`; and `ssl/`,
  where the Linux package keeps its certificate bundle, is not one of the
  four folders a manifest may name.
- **macOS does not install an update**, and will not by this design:
  replacing the whole bundle is a design of its own.
- **No binary deltas, no stripping of the programs, no frozen Windows
  toolchain, no elevated path for a per-machine install** (the design's
  optional tasks T19 to T22), and no measurement of two real builds to say
  what an update weighs (its spike S4). The figures under "What a person
  sees" are estimates.
- **The live rehearsal has not been run** (T17), nor the spike that would
  have asked the real GitHub about redirects and `Range` (S2): "To verify on
  first release". The static helper was built on x86-64 only (S1), and
  nothing of the update system has been built for Windows on ARM64.
- **The per-user installer has not been installed, updated and uninstalled
  on a clean Windows machine** (S3): its script is held to its rules as
  text, and its file removal is run on small trees (`docs/release.md`, "The
  Windows installer"); the run by hand is the owner's acceptance step.
- **`UPDATE CHECK` does not say that the install folder cannot be written.**
  A copy unpacked where its user may not write is offered a release like
  any other; the helper refuses at the swap (`last=refused:root_not_writable`),
  after the download.
- **No resume inside a file.** A download resumes file by file; a large
  file that was half fetched is fetched again.
- **No beta channel**, no installing an older version, no installing at
  exit.
- **No keyring on Linux and no Keychain on macOS**: the token is the
  environment variable there.
- **No "Open Releases" button** in the dialog for a network that does not
  allow partial downloads; Release notes is the way to the page.
- **No program says which repository it asks**, so a wrong
  `KATANA_UPDATE_REPO` is seen only in the build's compile line.
- **No script uploads a release to a host of the owner's own**
  (`KATANA_UPDATE_FEED`): the zip, then the `.sig`, then the manifest, by
  hand.
- **The state file has no lock.** Two programs that change an update
  setting at the same instant can lose one change (`docs/mcp.md`, "Not done
  yet").
- **The helper is not code-signed** until the repository holds a
  certificate: `cmake/KatanaSign.cmake` signs every `katana*.exe` of a
  package when it does, the helper among them. Until then it is unsigned,
  like the other programs.
- **What only a window, or only an installed copy, can show has no test
  that runs it**: the workbench's wiring (`docs/desktop.md`) - what a start
  lists from `recovery.log` and sweeps among it, which a build tree and a
  headless run never reach - and the few lines of `katana_cli`'s `main()`
  that start the helper (`docs/mcp.md`).
  The shipped programs cannot be given a test key, which is the point of
  them.
- **No test in the ordinary run puts the shipped transport in front of a
  server** (`src/katana_app/update_transport.cpp` over libcurl): "To verify
  on first release". A test that ran it against the fake feed would have to
  link `katana_app` and be held to the loopback by something other than its
  own construction.
- **The Command Reference has no Updates section** (`docs/desktop.md`);
  `HELP UPDATE` prints the text.
- **Five things found while this document was read against the code, in
  files it does not own, and left as they are:**
  - `discardPrepared` in `src/katana_qt/update_workbench.cpp` is a second
    copy of `app::abandonApply` (`src/katana_app/update_verbs.hpp`): each
    puts `U/run-<tx>` on `cleanup.list` for a request nobody will start.
    The window should call the one the session and `katana_cli` call.
  - `include/katana/update/check.hpp` lists `root_not_writable` and
    `headless` among a check's reasons, and says of `Unsupported` that it
    covers an unwritable root. No check gives either token: the first is the
    helper's, at the swap, and the second is what a headless window answers
    `DOWNLOAD`, `APPLY` and the lines it refuses with.
  - `include/katana/update/state.hpp` says `processCleanupList` is called by
    the helper after a healthy update as well as by the window. Only the
    window calls it, at its start; the helper appends to the list and never
    reads it.
  - The repository's front page, under "Licence", says the repository "is
    not public yet", and the comment over `KATANA_UPDATE_REPO` in
    `src/katana_update/CMakeLists.txt` says the owner "will make" it
    public. It is public (decision D0; it answered a request without
    credentials on 2026-10-13).
  - `tools/update_keytool.py` prints the `gh secret set` command without
    `--env release`, in its help as a parenthesis to add and after `pub` not
    at all; this document and `docs/release.md` give it with.
