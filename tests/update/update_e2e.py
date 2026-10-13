#!/usr/bin/env python3
"""The end-to-end tests of Katana's update system: a real feed, the real engine, the real helper program.

    python tests/update/update_e2e.py --case <name> --engine <katana_update_e2e>
           --helper <katana_updater_test> --stub <katana_updater_stub> [--shard I/N] [--keep]
    python tests/update/update_e2e.py --list
    python tests/update/update_e2e.py --registered <case>... [--portable-only]

One run is one case (tests/update/e2e.cmake registers each as a ctest). Nothing here is a fake of the code
under test:

  - the releases are made by the REAL producer, tools/update_make_manifest.py, run as a program the way the
    release workflow runs it, and signed by openssl with the published RFC 8032 test key
    (tests/update/fixtures/keys, tests/update/test_key.hpp);
  - they are served over HTTP by tools/update_fake_feed.py on 127.0.0.1, port 0;
  - katana_update_e2e (tests/update/e2e_main.cpp) is the engine - check, plan, stage, preflight, launchApply,
    recover - with the test key as its trust and a socket client that can reach the loopback address only;
  - katana_updater_test is the helper's own main.cpp built with the test key and --die-at N; it is the
    `bin/katana_updater.exe` of every install here, so the engine's preflight copies and self-tests IT;
  - the install's `katana` is tests/update/updater_stub.cpp, a real program (a test never starts a file that
    is not one: Windows answers that with a message box), tagged v1 or v2 after its last byte so that the two
    releases differ and each start says which it was.

WHERE THE EXPECTATIONS COME FROM (CLAUDE.md, section 4). None is copied from what the programs print:

  - the tree an update must leave is built here from the two packages: v2's files, the seed as v1 had it,
    the files only v1 listed gone with their folder, and the three records the helper writes (install.hpp;
    installed.json is `{product, schema, sequence, tag, target, version}`, keys sorted, one line feed);
  - which files a plan must name: hashlib over the installed files and the release's (plan.hpp: absent or
    another hash; a seed only when absent);
  - which bytes may be asked of the zip: where Python's zipfile says each member's data is, merged by the
    rule ranges.hpp states (neighbours at most 1 MiB apart, 32 MiB at most), written again here;
  - reason tokens, outcomes and exit codes are the tables of check.hpp, stage.hpp, apply.hpp and
    src/katana_updater/main.cpp, which are the design and not a log;
  - what went over the wire is read from the feed's own log, never from the client.

EVERY ROOT IS TEMPORARY: one folder under the system's temporary folder for the run, removed at its end
(--keep leaves it and prints where). The install's folder has a blank and a letter outside ASCII in its
name, as a folder under a person's profile has. No registry key is read or written: katana_update_e2e has none
and the test helper is given none (src/katana_updater/main.cpp: without a base it uses a registry that has
nothing). No proxy: the engine's client speaks to a socket, and this process's own requests do not exist.

The cases marked portable ask the feed and start no program of the install, so they also run where the swap
is not written yet (Linux, macOS: T18) and without --helper and --stub. The others are Windows only.

Exit status: 0 the case passed; 1 it failed (the reason, then everything that was run and what it printed);
2 a usage error.
"""

from __future__ import annotations

import argparse
import base64
import fnmatch
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import traceback
import zipfile
import zlib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Iterable, Optional

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
TOOLS = REPO / "tools"
KEYS = HERE / "fixtures" / "keys"

sys.dont_write_bytecode = True
sys.path.insert(0, str(TOOLS))

import update_fake_feed as fake  # noqa: E402  (tools/ is put on the path above)

TARGET = "win64"
SEED_GLOB = "etc/katana/*.ini"
# The fixtures' own pair: 0.10 is above 0.9 as numbers and not as text, and the sequences are UTC seconds
# of two builds a month apart.
V1_VERSION, V1_SEQUENCE = "0.9.0", 1790000000
V2_VERSION, V2_SEQUENCE = "0.10.0", 1792312800
INSTALL_RECORD = b'{"schema":1,"target":"win64"}'  # install.hpp: what the CPack install writes
# ranges.hpp: two members merge when the hole between them is at most this, into at most that.
RANGE_MERGE_GAP = 1024 * 1024
RANGE_MAX_LENGTH = 32 * 1024 * 1024
# A file that does not change and does not compress, longer than the gap: a request that asked for it would
# be asking for bytes nobody needs, so no range may reach across it.
BIG = 3 * 512 * 1024
# The test helper's --timings (src/katana_updater/main.cpp): wait for the window, wait for the programs
# under R, poll interval, wait for the health check, how long a refused rename is retried - milliseconds.
TIMINGS = "500,1500,50,20000,30000"
TIMINGS_SHORT_RETRY = "500,1500,50,20000,600"
# Exit codes of the helper (the table at the top of src/katana_updater/main.cpp).
EXIT_OK, EXIT_BLOCKED, EXIT_ROLLED_BACK, EXIT_VERIFICATION, EXIT_SIMULATED_CRASH = 0, 10, 11, 21, 99
# Not a token of anything: a text to look for on the wire and in what the programs print.
TOKEN = "e2e-token-3f9c2a71d5b84e60"
GITHUB_REPO = "katana-e2e/private"
STORAGE_HOST = "storage.katana-e2e.invalid"
PKCS8_ED25519_PREFIX = bytes.fromhex("302e020100300506032b657004220420")  # RFC 8410, before the 32-byte seed
PLACEHOLDER_PROGRAM = b"KATANA E2E PLACEHOLDER: not a program, and never started (a portable case)\n"


class Failure(Exception):
    """The case failed: an expectation did not hold."""


def expect(condition: object, message: str) -> None:
    if not condition:
        raise Failure(message)


def expect_equal(actual: object, expected: object, what: str) -> None:
    if actual != expected:
        raise Failure(f"{what}:\n  expected {expected!r}\n  got      {actual!r}")


# ---- bytes, trees, hashes -----------------------------------------------------------------------


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def text(*lines: str) -> bytes:
    return ("\n".join(lines) + "\n").encode("ascii")


def noise(label: str, size: int) -> bytes:
    """Incompressible, reproducible bytes: SHA-256 of a label and a counter, concatenated."""
    out = bytearray()
    counter = 0
    while len(out) < size:
        out += hashlib.sha256(f"{label}:{counter}".encode("ascii")).digest()
        counter += 1
    return bytes(out[:size])


def tagged(program: bytes, tag: str) -> bytes:
    """The stub with a tag after its last byte: it still runs (the loader ignores what follows the last
    section) and reads the tag from its own file (updater_stub.cpp)."""
    return program + b"\nKATANA-STUB-TAG=" + tag.encode("ascii") + b"\n"


def make_package(tag: str, stub: bytes, helper: bytes, big: bool) -> dict[str, bytes]:
    """What a release installs, path to bytes, as the CPack install lays it out (install.json included).

    Between v1 and v2: both programs and one library, three data files and the seed differ; the helper, a
    data file and (with `big`) a library and a data file of 1.5 MiB each do not; a library and the only
    file of share/katana/legacy go; a library and the first file of share/katana/help arrive. In the
    update zip (program, lib, data, seed, each by path) the changed members then lie in three groups with
    an unchanged 1.5 MiB between them."""
    second = tag == "v2"
    files = {
        "bin/katana.exe": tagged(stub, tag),
        "bin/katana_cli.exe": tagged(stub, tag),
        "bin/katana_updater.exe": helper,
        "bin/libcore-1.dll": text(f"the core library of {tag}", *[f"symbol {n}" for n in range(20)]),
        "etc/katana/settings.ini": text("; a seed: written only when absent", "theme=dark" if second else "theme=light"),
        "share/katana/a-first.txt": text(f"the first of the data files, as {tag} has it"),
        "share/katana/install.json": INSTALL_RECORD,
        "share/katana/random.bin": noise(f"katana-e2e-random-{tag}", 2048),
        "share/katana/readme.txt": text(f"the readme of {tag}"),
        "share/katana/same.txt": text("the same in both versions"),
    }
    if big:
        files["bin/libbig-1.dll"] = noise("katana-e2e-big-library", BIG)
        files["share/katana/big.bin"] = noise("katana-e2e-big-data", BIG)
    if second:
        files["bin/libnew-2.dll"] = text("a library only v2 has")
        files["share/katana/help/index.txt"] = text("only in v2: the first file of a folder v1 does not have")
    else:
        files["bin/libold-1.dll"] = text("a library only v1 has")
        files["share/katana/legacy/old.txt"] = text("only in v1: the last file of its folder")
    return files


def write_tree(root: Path, files: dict[str, bytes]) -> None:
    for relative, data in files.items():
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


def snapshot(root: Path) -> dict[str, str]:
    """Every entry under `root` but the helper's own working folder .update: a folder is "dir", a file
    its SHA-256. Two trees are the same when this is - a folder left behind empty is a difference."""
    entries: dict[str, str] = {}
    for folder, folders, files in os.walk(root):
        relative = Path(folder).relative_to(root)
        if relative == Path("."):
            folders[:] = [name for name in folders if name != ".update"]
        for name in folders:
            entries[(relative / name).as_posix()] = "dir"
        for name in files:
            entries[(relative / name).as_posix()] = sha256_hex((Path(folder) / name).read_bytes())
    return entries


def snapshot_of(files: dict[str, bytes]) -> dict[str, str]:
    """The snapshot a tree holding exactly `files` has."""
    entries: dict[str, str] = {}
    for relative, data in files.items():
        entries[relative] = sha256_hex(data)
        parts = relative.split("/")
        for depth in range(1, len(parts)):
            entries["/".join(parts[:depth])] = "dir"
    return entries


def tree_hash(entries: dict[str, str]) -> str:
    """One hash for a whole tree: its snapshot, a line an entry, sorted."""
    return sha256_hex("".join(f"{name}\t{what}\n" for name, what in sorted(entries.items())).encode("utf-8"))


def tree_difference(expected: dict[str, str], actual: dict[str, str]) -> str:
    lines = []
    for name in sorted(expected):
        if name not in actual:
            lines.append(f"  missing:    {name}")
        elif actual[name] != expected[name]:
            lines.append(f"  different:  {name}")
    lines += [f"  unexpected: {name}" for name in sorted(actual) if name not in expected]
    return "\n".join(lines) or "  (no difference)"


def is_seed(path: str) -> bool:
    return fnmatch.fnmatchcase(path, SEED_GLOB)


def changed_paths(installed: dict[str, bytes], release: dict[str, bytes]) -> list[str]:
    """The files of a release an install must fetch (plan.hpp): absent, or other bytes; a seed only when
    absent. install.json is no file of a release (the producer leaves it to the updater)."""
    changed = []
    for path in sorted(release):
        if path == "share/katana/install.json":
            continue
        if path not in installed or (not is_seed(path) and installed[path] != release[path]):
            changed.append(path)
    return changed


def member_span(zip_path: Path, name: str) -> tuple[int, int]:
    """(where the member's deflate data starts, how long it is), from zipfile's directory and the local
    header read here - not from the manifest, which is what the engine goes by."""
    with zipfile.ZipFile(zip_path) as archive:
        info = archive.getinfo(name)
    with open(zip_path, "rb") as raw:
        raw.seek(info.header_offset)
        signature, name_length, extra_length = struct.unpack("<4s22xHH", raw.read(30))
    expect(signature == b"PK\x03\x04", f"{name}: no local file header where the zip says it is")
    return info.header_offset + 30 + name_length + extra_length, info.compress_size


def merged_ranges(spans: Iterable[tuple[int, int]]) -> list[tuple[int, int]]:
    """The requests that fetch these (offset, length) members, by the rule ranges.hpp states: in offset
    order, a member joins the range before it when the hole between them is at most RANGE_MERGE_GAP and the
    range stays within RANGE_MAX_LENGTH."""
    ranges: list[list[int]] = []
    for offset, length in sorted(spans):
        if ranges:
            start, so_far = ranges[-1]
            if offset - (start + so_far) <= RANGE_MERGE_GAP and offset + length - start <= RANGE_MAX_LENGTH:
                ranges[-1][1] = max(so_far, offset + length - start)
                continue
        ranges.append([offset, length])
    return [(offset, length) for offset, length in ranges]


def installed_record(version: str, sequence: int) -> bytes:
    """installed.json as the helper records it (install.hpp; apply.hpp, step 5): keys sorted, one LF."""
    record = {"product": "katana", "schema": 1, "sequence": sequence, "tag": f"v{version}", "target": TARGET,
              "version": version}
    return (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode("ascii")


# ---- what a program printed ---------------------------------------------------------------------


@dataclass
class Done:
    """One program that ran: its exit code and what it printed."""

    command: list[str]
    code: int
    out: str
    err: str

    def values(self, key: str) -> list[str]:
        prefix = key + "="
        return [line[len(prefix):] for line in self.out.splitlines() if line.startswith(prefix)]

    def value(self, key: str) -> str:
        found = self.values(key)
        expect(found, f"no {key}= line in the output of {Path(self.command[0]).name}:\n{self.out}{self.err}")
        return found[0]

    def has(self, key: str) -> bool:
        return bool(self.values(key))


@dataclass
class Release:
    """One release as the producer wrote it: the folder a feed serves."""

    folder: Path
    version: str
    sequence: int
    package: dict[str, bytes]
    target: str = TARGET

    @property
    def manifest_path(self) -> Path:
        return self.folder / f"Katana-update-{self.target}.json"

    @property
    def signature_path(self) -> Path:
        return self.folder / f"Katana-update-{self.target}.json.sig"

    @property
    def zip_path(self) -> Path:
        return self.folder / f"Katana-{self.version}-{self.target}.update.zip"

    def copy(self, to: Path) -> "Release":
        shutil.copytree(self.folder, to)
        return Release(to, self.version, self.sequence, self.package, self.target)


# ---- the bench: the programs, the work folder, the releases ---------------------------------------


class Bench:
    def __init__(self, work: Path, engine: Optional[Path], helper: Optional[Path], stub: Optional[Path]):
        self.work = work
        self.engine = engine
        self.helper = helper
        self.stub = stub
        self.transcript: list[str] = []
        self.feeds: list[fake.FakeFeed] = []
        self.children: list[subprocess.Popen] = []
        self._packages: dict[tuple[str, bool], dict[str, bytes]] = {}
        self._releases: dict[str, Release] = {}
        self._keys: dict[str, Path] = {}

    # ---- running things ---------------------------------------------------------------------

    def environment(self, more: Optional[dict[str, str]] = None) -> dict[str, str]:
        """What every program started here runs in: this process's environment without what would send
        a request through a proxy or point a test helper at a registry key, and with `more`."""
        dropped = {"KATANA_UPDATER_TEST_REGISTRY_BASE", "KATANA_STUB_REGISTRY_BASE", "KATANA_STUB_MACHINE_RUNONCE",
                   "KATANA_STUB_PID_FILE", "KATANA_STUB_MODE", "KATANA_STUB_LOG", "KATANA_UPDATE_STATE_DIR",
                   "KATANA_UPDATE_TOKEN", "SOURCE_DATE_EPOCH", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"}
        env = {name: value for name, value in os.environ.items() if name.upper() not in dropped}
        env["NO_PROXY"] = "127.0.0.1"
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        env["PYTHONIOENCODING"] = "utf-8"
        env.update(more or {})
        return env

    def run(self, command: list[object], *, env: Optional[dict[str, str]] = None, cwd: Optional[Path] = None,
            timeout: float = 600.0) -> Done:
        words = [str(word) for word in command]
        try:
            ran = subprocess.run(words, capture_output=True, env=self.environment(env), cwd=str(cwd) if cwd else None,
                                 stdin=subprocess.DEVNULL, timeout=timeout, check=False)
        except subprocess.TimeoutExpired as late:
            self.transcript.append(f"$ {' '.join(words)}\n  DID NOT FINISH in {timeout} s\n"
                                   f"{(late.stdout or b'').decode('utf-8', 'replace')}")
            raise Failure(f"{Path(words[0]).name} did not finish in {timeout} s") from late
        done = Done(words, ran.returncode, ran.stdout.decode("utf-8", "replace").replace("\r\n", "\n"),
                    ran.stderr.decode("utf-8", "replace").replace("\r\n", "\n"))
        self.transcript.append(f"$ {' '.join(words)}\n  exit {done.code}\n{_indented(done.out)}{_indented(done.err)}")
        return done

    def note(self, line: str) -> None:
        self.transcript.append(f"# {line}")

    # ---- the packages and the releases ------------------------------------------------------------

    def needs_programs(self) -> None:
        expect(self.engine and self.helper and self.stub,
               "this case starts the helper and the stand-in for Katana: give --engine, --helper and --stub")
        expect(os.name == "nt", "this case swaps an install, which is written for Windows only so far")

    def package(self, tag: str, big: bool = True) -> dict[str, bytes]:
        key = (tag, big)
        if key not in self._packages:
            stub = self.stub.read_bytes() if self.stub else PLACEHOLDER_PROGRAM
            helper = self.helper.read_bytes() if self.helper else PLACEHOLDER_PROGRAM
            self._packages[key] = make_package(tag, stub, helper, big)
        return self._packages[key]

    def key(self, name: str) -> Path:
        """The private key PEM of a published RFC 8032 test key (active: TEST 1, spare: TEST 2, other:
        TEST 3, which nothing trusts), built from its seed in the work folder and removed with it."""
        if name not in self._keys:
            seed = bytes.fromhex((KEYS / f"test-{name}.seed.hex").read_text(encoding="ascii").strip())
            expect(len(seed) == 32, f"test-{name}.seed.hex is not a 32-byte seed")
            body = base64.encodebytes(PKCS8_ED25519_PREFIX + seed).decode("ascii")
            path = self.work / "keys" / f"test-{name}.pem"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("-----BEGIN PRIVATE KEY-----\n" + body + "-----END PRIVATE KEY-----\n", newline="\n")
            self._keys[name] = path
        return self._keys[name]

    def produce(self, name: str, package: dict[str, bytes], version: str, sequence: int, *, key: str = "active",
                target: str = TARGET) -> Release:
        """A release of `package`, made by the real producer run as a program, signed with a test key."""
        if name in self._releases:
            return self._releases[name]
        tree = self.work / "package" / name
        write_tree(tree, package)
        out = self.work / "release" / name
        made = self.run([sys.executable, "-B", TOOLS / "update_make_manifest.py", "--tree", tree, "--out", out,
                         "--version", version, "--target", target, "--sequence", sequence, "--seed", SEED_GLOB,
                         "--key", self.key(key), "--test-key"])
        expect_equal(made.code, 0, f"the producer's exit code for the release {name}")
        release = Release(out, version, sequence, package, target)
        for made_file in (release.manifest_path, release.signature_path, release.zip_path):
            expect(made_file.is_file(), f"the producer did not write {made_file.name}")
        self._releases[name] = release
        return release

    def release(self, tag: str, big: bool = True) -> Release:
        """v1 or v2 as published."""
        version, sequence = (V1_VERSION, V1_SEQUENCE) if tag == "v1" else (V2_VERSION, V2_SEQUENCE)
        return self.produce(tag if big else f"{tag}-small", self.package(tag, big), version, sequence)

    def variant(self, release: Release, name: str) -> Release:
        """A copy of a release's folder, for a test to damage."""
        return release.copy(self.work / "release" / name)

    # ---- the feed ---------------------------------------------------------------------------------

    def feed(self, folder: Path, *rules: fake.Rule, **options: object) -> fake.FakeFeed:
        served = fake.FakeFeed(folder, rules, **options)  # type: ignore[arg-type]
        served.start()
        self.feeds.append(served)
        return served

    def close(self) -> None:
        for child in self.children:
            if child.poll() is None:
                child.kill()
            try:
                child.wait(timeout=30)
            except subprocess.TimeoutExpired:
                pass
        for served in self.feeds:
            try:
                served.stop()
            except Exception:  # noqa: BLE001  (a feed that was stopped already)
                pass


def _indented(output: str) -> str:
    return "".join(f"  | {line}\n" for line in output.splitlines())


def wait_for(found: Callable[[], object], seconds: float, what: str) -> object:
    """What `found` returns once it is true. A Failure naming `what` when it never is."""
    end = time.monotonic() + seconds
    while True:
        value = found()
        if value:
            return value
        if time.monotonic() >= end:
            raise Failure(f"waited {seconds:.0f} s for {what}")
        time.sleep(0.05)


def read_json(path: Path) -> Optional[dict]:
    """The JSON object in a file, or None while it is not there or not whole."""
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return value if isinstance(value, dict) else None


# ---- a world: one install, its state folder, what its stub is told -------------------------------------


@dataclass
class Prepared:
    """What preflight left: the helper copy that will run the swap."""

    transaction: str
    helper: Path
    stage: Path


@dataclass
class World:
    bench: Bench
    name: str
    installed: str = "v1"
    big: bool = True
    top: Path = field(init=False)
    root: Path = field(init=False)
    state: Path = field(init=False)
    log: Path = field(init=False)
    stub_mode: str = "healthy"

    def __post_init__(self) -> None:
        self.top = self.bench.work / self.name
        # A blank and a letter outside ASCII (a with an acute accent), as a folder under a person's
        # profile has: the paths go through two command lines, an environment variable and a log.
        self.root = self.top / ("Katan" + chr(0xE1) + " 1")
        self.state = self.top / "state"
        self.log = self.top / "stub.log"
        self.state.mkdir(parents=True)
        self.files = self.installed_files()
        write_tree(self.root, self.files)

    def installed_files(self) -> dict[str, bytes]:
        """The install as the helper leaves a release it installed: the package, installed.json and the
        signed manifest beside it (install.hpp), so that a later update knows what that release put here."""
        release = self.bench.release(self.installed, self.big)
        files = dict(release.package)
        files["share/katana/installed.json"] = installed_record(release.version, release.sequence)
        files["share/katana/installed.manifest.json"] = release.manifest_path.read_bytes()
        files["share/katana/installed.manifest.json.sig"] = release.signature_path.read_bytes()
        return files

    def after_update_to(self, release: Release) -> dict[str, bytes]:
        """The files an update of this install to `release` must leave (design, section 5; apply.hpp):
        the release's own, a seed as the install already had it, nothing only the old release listed,
        install.json as it was, and the records of the new release."""
        files = dict(release.package)
        for path, data in self.files.items():
            if is_seed(path):
                files[path] = data
        files["share/katana/installed.json"] = installed_record(release.version, release.sequence)
        files["share/katana/installed.manifest.json"] = release.manifest_path.read_bytes()
        files["share/katana/installed.manifest.json.sig"] = release.signature_path.read_bytes()
        return files

    # ---- the programs ---------------------------------------------------------------------------

    def environment(self) -> dict[str, str]:
        """The stub's switches (updater_stub.cpp), and the folder its health marker goes into - which is
        also where update::recover looks for the helper copy."""
        return {"KATANA_STUB_LOG": str(self.log), "KATANA_STUB_MODE": self.stub_mode,
                "KATANA_UPDATE_STATE_DIR": str(self.state)}

    def engine(self, command: str, *args: object) -> Done:
        return self.bench.run([self.bench.engine, command, "--root", self.root, "--state", self.state, *args],
                              env=self.environment(), cwd=self.top)

    def ask(self, command: str, feed: fake.FakeFeed, *args: object) -> Done:
        """check, plan or stage against a feed of the owner's own: the fake's first port."""
        return self.engine(command, "--feed", feed.url, *args)

    def stage(self, feed: fake.FakeFeed, *args: object) -> Path:
        """Stage what the feed offers; the stage folder."""
        staged = self.ask("stage", feed, *args)
        expect_equal(staged.code, 0, "the exit code of stage")
        expect(staged.has("staged") and staged.value("staged") != "none",
               f"nothing was staged:\n{staged.out}{staged.err}")
        return Path(staged.value("stage"))

    def preflight(self, stage: Path) -> Prepared:
        ran = self.engine("preflight", "--stage", stage)
        expect_equal(ran.code, 0, "the exit code of preflight")
        prepared = Prepared(ran.value("transaction"), Path(ran.value("helper")), stage)
        expect(re.fullmatch(r"[0-9a-f]{1,64}", prepared.transaction), f"not a transaction id: {prepared.transaction}")
        expect(prepared.helper.is_file(), f"preflight named a helper copy that is not there: {prepared.helper}")
        expect_equal(prepared.helper.parent, self.state / f"run-{prepared.transaction}",
                     "the folder of the helper copy (state.hpp: U/run-<tx>)")
        return prepared

    def swap(self, prepared: Prepared, *more: object, timings: str = TIMINGS, relaunch: bool = False,
             timeout: float = 600.0) -> Done:
        """The helper copy, started the way the engine starts it (apply.hpp, helperArguments) with the test
        build's options after it, and waited for."""
        again = self.root / "bin" / "katana.exe" if relaunch else "none"
        return self.bench.run([prepared.helper, "--root", self.root, "--stage", prepared.stage, "--state", self.state,
                               "--tx", prepared.transaction, "--wait-pid", "0", "--relaunch", again,
                               "--timings", timings, *more],
                              env=self.environment(), cwd=prepared.helper.parent, timeout=timeout)

    # ---- what is on the disk ----------------------------------------------------------------------

    def expect_tree(self, expected: dict[str, bytes], what: str) -> None:
        wanted, actual = snapshot_of(expected), snapshot(self.root)
        if tree_hash(actual) != tree_hash(wanted):
            raise Failure(f"{what}: the install is not the tree it must be\n{tree_difference(wanted, actual)}")

    def expect_untouched(self, what: str) -> None:
        self.expect_tree(self.files, what)

    def leftovers(self) -> list[str]:
        """The names under R/.update other than the lock: what a finished transaction must not leave."""
        folder = self.root / ".update"
        return sorted(entry.name for entry in folder.iterdir() if entry.name != "lock") if folder.is_dir() else []

    def last(self) -> dict:
        """U/last.json (state.hpp): how the swap that was asked for ended."""
        found = wait_for(lambda: read_json(self.state / "last.json"), 30, "last.json")
        assert isinstance(found, dict)
        return found

    def stub_lines(self) -> list[str]:
        try:
            return self.log.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            return []

    def wait_for_stub(self, start: str, seconds: float = 60.0) -> str:
        """The first line of the stub's log that begins with `start`: the program the helper starts last is
        detached, so its line comes when it comes."""
        found = wait_for(lambda: next((line for line in self.stub_lines() if line.startswith(start)), None),
                         seconds, f"a line '{start}...' in the stub's log (it has: {self.stub_lines()})")
        assert isinstance(found, str)
        return found

    def blocked_sequences(self) -> list[int]:
        state = read_json(self.state / "state.json") or {}
        return list(state.get("blocked", []))


def staged_files(stage: Path) -> dict[str, str]:
    """The files in a stage folder, by path, with their hashes (the two records and the lock aside)."""
    entries = snapshot(stage) if stage.is_dir() else {}
    return {name: what for name, what in entries.items()
            if what != "dir" and name not in ("manifest.json", "manifest.json.sig") and not name.startswith(".partial")}


def expect_only_verified(stage: Path, release: Release, what: str) -> dict[str, str]:
    """Nothing unverified is ever in a stage folder (stage.hpp): every file there is a file of the release,
    byte for byte. Returns what is there."""
    there = staged_files(stage)
    for path, digest in there.items():
        expect(path in release.package, f"{what}: the stage holds {path}, which the release does not list")
        expect_equal(digest, sha256_hex(release.package[path]), f"{what}: the staged {path}")
    return there


def zip_requests(feed: fake.FakeFeed, release: Release, port: Optional[str] = None) -> list[dict]:
    return [r for r in feed.requests(port) if r["path"].split("?")[0] == "/" + release.zip_path.name]


def expect_ranges_served(requests: list[dict], expected: list[tuple[int, int]], what: str) -> None:
    """The feed served exactly these (offset, length) parts of the zip, each whole, and nothing else of it."""
    expect_equal([r["status"] for r in requests], [206] * len(expected), f"{what}: the statuses of the zip's requests")
    expect_equal([tuple(r["range"]) for r in requests], [(offset, offset + length - 1) for offset, length in expected],
                 f"{what}: the parts of the zip that were served (first and last byte)")
    expect_equal([r["served"] for r in requests], [length for _, length in expected], f"{what}: the bytes served")


# ---- the cases ------------------------------------------------------------------------------------

CASES: dict[str, tuple[Callable[["Bench", argparse.Namespace], None], bool]] = {}


def case(name: str, portable: bool = False) -> Callable:
    def register(function: Callable[["Bench", argparse.Namespace], None]) -> Callable:
        CASES[name] = (function, portable)
        return function
    return register


def expect_unreachable(asked: Done, reason: str, what: str) -> None:
    expect_equal(asked.code, 0, f"{what}: the exit code (a feed that cannot be believed is an answer, not an error)")
    expect_equal(asked.value("update"), "unreachable", f"{what}: update=")
    expect_equal(asked.value("reason"), reason, f"{what}: reason=")


def expect_nothing_fetched_or_staged(world: World, feed: fake.FakeFeed, release: Release, what: str) -> None:
    """A release that was not offered is not downloaded: stage says so, the zip is never asked for, and
    the per-user folder holds no stage."""
    staged = world.ask("stage", feed)
    expect_equal(staged.code, 0, f"{what}: the exit code of stage")
    expect_equal(staged.values("staged"), ["none"], f"{what}: staged=")
    expect_equal(zip_requests(feed, release), [], f"{what}: requests for the zip")
    expect_equal(sorted(entry.name for entry in world.state.iterdir()), [], f"{what}: what the state folder holds")
    world.expect_untouched(what)


@case("v1_to_v2")
def v1_to_v2(bench: Bench, args: argparse.Namespace) -> None:
    """The whole path, as the window walks it: check, plan, download, the helper started detached, the new
    version's health check, the relaunch. The tree is then exactly v2's and the feed served only what changed."""
    bench.needs_programs()
    v2 = bench.release("v2")
    world = World(bench, "v1_to_v2")
    feed = bench.feed(v2.folder)
    changed = changed_paths(world.files, v2.package)
    total = len(v2.package) - 1  # install.json is no file of the release
    spans = {path: member_span(v2.zip_path, path) for path in v2.package if path != "share/katana/install.json"}
    wanted = merged_ranges(spans[path] for path in changed)
    # The premise of "only what changed": the changed files lie in three groups, with a file nobody needs
    # between them that is longer than the gap a request may bridge.
    expect_equal(len(wanted), 3, "the number of requests the changed files need (the layout of make_package)")

    checked = world.ask("check", feed)
    expect_equal(checked.code, 0, "the exit code of check")
    expect_equal({key: checked.value(key) for key in ("target", "kind", "installed", "installed_sequence", "update",
                                                      "latest", "sequence", "reason", "route")},
                 {"target": TARGET, "kind": "archive", "installed": V1_VERSION, "installed_sequence": str(V1_SEQUENCE),
                  "update": "available", "latest": V2_VERSION, "sequence": str(V2_SEQUENCE), "reason": "",
                  "route": "download"}, "what check says")
    expect_equal(checked.value("notes_url"), f"https://github.com/kjarada/Katana/releases/tag/v{V2_VERSION}", "notes_url=")
    # The signature first, then the manifest, and nothing else (design, section 2).
    asked = feed.requests()
    expect_equal([r["path"] for r in asked], [f"/Katana-update-{TARGET}.json.sig", f"/Katana-update-{TARGET}.json"],
                 "what a check asks the feed for")
    for request in asked:
        expect(re.fullmatch(r"Katana/\S+ \(win64\)", request["headers"].get("user-agent", "")),
               f"the user agent is not 'Katana/<version> (<target>)': {request['headers']}")
        expect(not {"authorization", "cookie", "range"} & set(request["headers"]),
               f"a check sent a header it has no reason to: {request['headers']}")

    feed.clear_log()
    planned = world.ask("plan", feed)
    expect_equal(planned.code, 0, "the exit code of plan")
    expect_equal(planned.values("changed"), changed, "the files the plan names as changed")
    expect_equal(planned.value("files"), f"{len(changed)}/{total}", "files=")
    expect_equal(int(planned.value("download_bytes")), sum(spans[path][1] for path in changed), "download_bytes=")
    expect_equal(int(planned.value("changed_bytes")), sum(len(v2.package[path]) for path in changed), "changed_bytes=")
    expect_equal(planned.values("range"), [f"{offset}+{length}" for offset, length in wanted], "the plan's requests")
    expect_equal(zip_requests(feed, v2), [], "requests for the zip while only planning")
    world.expect_untouched("after plan")

    feed.clear_log()
    staged = world.ask("stage", feed)
    expect_equal((staged.code, staged.values("staged")), (0, [V2_VERSION]), "the exit code of stage, and staged=")
    stage = Path(staged.value("stage"))
    expect_equal(stage, world.state / f"stage-{V2_SEQUENCE}", "the stage folder (state.hpp: U/stage-<sequence>)")
    # Progress is bytes of the zip fetched over the bytes to fetch (stage.hpp), and ends at its total.
    to_fetch = sum(length for _, length in wanted)
    expect_equal(staged.value("progress"), f"{to_fetch}/{to_fetch}", "progress= at the end of the download")
    expect_equal(staged.value("progress_steady"), "true", "progress_steady= (never backwards, never past its total)")
    fetched = zip_requests(feed, v2)
    expect_ranges_served(fetched, wanted, "the download")
    for request in fetched:
        expect_equal(request["headers"].get("range"), f"bytes={request['range'][0]}-{request['range'][1]}",
                     "the Range header of a request for the zip")
    # Only what changed: the bytes served are the changed members and the zip's own headers between
    # neighbours (30 bytes and the name each), and no part of a file that did not change and is too long to
    # bridge was served.
    served = sum(r["served"] for r in fetched)
    needed = sum(spans[path][1] for path in changed)
    expect(needed <= served <= needed + 300 * len(changed),
           f"the feed served {served} bytes of the zip for {needed} bytes of changed files")
    expect(served < v2.zip_path.stat().st_size - 2 * BIG, "the feed served the files that did not change")
    for kept in ("bin/libbig-1.dll", "share/katana/big.bin"):
        offset, length = spans[kept]
        for request in fetched:
            expect(request["range"][1] < offset or request["range"][0] >= offset + length,
                   f"a request reached into {kept}, which did not change: {request['range']}")
    # Staged again: what is there and verified is not fetched a second time, and counts as fetched.
    again = world.ask("stage", feed)
    expect_equal((again.code, again.values("staged")), (0, [V2_VERSION]), "a second stage of the same release")
    expect_equal(zip_requests(feed, v2), fetched, "a stage that is complete asks for no byte of the zip again")
    expect_equal((again.value("progress"), again.value("progress_steady")), (f"{needed}/{needed}", "true"),
                 "progress= of a stage with nothing left to fetch (what is staged counts as fetched)")
    expect_equal(expect_only_verified(stage, v2, "the stage"), {path: sha256_hex(v2.package[path]) for path in changed},
                 "the files in the stage folder")
    expect_equal((stage / "manifest.json").read_bytes(), v2.manifest_path.read_bytes(), "the staged manifest")
    expect_equal((stage / "manifest.json.sig").read_bytes(), v2.signature_path.read_bytes(), "the staged signature")
    world.expect_untouched("after the download")

    # From here on there is no feed: the swap needs none.
    feed.stop()
    launched = world.engine("launch", "--stage", stage)
    expect_equal(launched.code, 0, "the exit code of launch")
    transaction = launched.value("launched")
    expect_equal(Path(launched.value("relaunch")), world.root / "bin" / "katana.exe", "what the helper is told to start")
    # The helper is detached and waits for the program that started it: last.json is its last word.
    last = wait_for(lambda: read_json(world.state / "last.json"), 300, "the helper to finish (last.json)")
    assert isinstance(last, dict)
    expect_equal({key: last.get(key) for key in ("status", "reason", "version", "tx")},
                 {"status": "committed", "reason": "", "version": V2_VERSION, "tx": transaction}, "last.json")
    world.expect_tree(world.after_update_to(v2), "after the update")
    expect_equal(world.leftovers(), [], "what a healthy update leaves in R/.update")
    expect(not stage.exists(), "the stage folder is still there after a healthy update")
    # The new version was started for its health check from the install, and again at the end.
    program = str(world.root / "bin" / "katana.exe")
    health = world.wait_for_stub("health ")
    expect(health.startswith("health tag=v2 ") and f"exe={program} " in health
           and health.endswith(f"tx={transaction} mode=healthy"), f"the health check's start: {health}")
    again = world.wait_for_stub("start ")
    expect(again.startswith("start tag=v2 ") and f"exe={program} " in again, f"the relaunch: {again}")
    expect_equal(len(world.stub_lines()), 2, f"the number of times the stub was started: {world.stub_lines()}")

    # And the install now IS that release: the same feed offers nothing, and no file of it differs.
    feed = bench.feed(v2.folder)
    current = world.ask("check", feed)
    expect_equal((current.value("update"), current.value("installed"), current.value("installed_sequence")),
                 ("current", V2_VERSION, str(V2_SEQUENCE)), "check after the update")
    nothing = world.ask("plan", feed, "--even-if-not-newer")
    expect_equal((nothing.value("files"), nothing.values("changed"), nothing.value("download_bytes")),
                 (f"0/{total}", [], "0"), "a plan of the installed release against its own install")


@case("bad_member_hash")
def bad_member_hash(bench: Bench, args: argparse.Namespace) -> None:
    """A release whose zip holds other bytes for one file than its signed manifest lists: the file is not
    staged, the stage is never complete, the install is not touched - and the download resumes once the
    feed serves the real zip."""
    bench.needs_programs()
    v2 = bench.release("v2")
    bad = bench.variant(v2, "v2-bad-member")
    victim = "share/katana/random.bin"
    offset, length = member_span(bad.zip_path, victim)
    damaged = bytearray(bad.zip_path.read_bytes())
    damaged[offset + length // 2] ^= 0x01
    bad.zip_path.write_bytes(bytes(damaged))
    # What the damage is, judged here: still one deflate stream of the signed length (noise is stored, not
    # compressed, so a changed byte is a changed byte of the file), whose hash is not the signed one.
    inflater = zlib.decompressobj(-15)
    inflated = inflater.decompress(bytes(damaged[offset:offset + length])) + inflater.flush()
    expect(inflater.eof and len(inflated) == len(v2.package[victim]) and inflated != v2.package[victim],
           "the damaged member is not a file of the same length with other bytes")

    world = World(bench, "bad_member_hash")
    feed = bench.feed(bad.folder)
    refused = world.ask("stage", feed)
    expect_equal(refused.code, 1, "the exit code of stage")
    expect_equal((refused.value("error"), refused.value("error_reason")), ("InvalidArgument", "staged_hash"),
                 "how stage fails (stage.hpp)")
    expect(f"path={victim}" in refused.value("error_context"),
           f"the failure does not name the file: {refused.value('error_context')}")
    expect(not refused.has("staged"), "stage said staged= after failing")
    stage = Path(refused.value("stage"))
    before = expect_only_verified(stage, v2, "after the failed download")
    expect(victim not in before, "the file that failed its hash was staged")
    expect(before, "the files fetched before the bad one were not kept (the premise of the resume below)")
    expect(not (stage / "manifest.json").exists() and not (stage / "manifest.json.sig").exists(),
           "a stage that failed says it is complete")
    world.expect_untouched("after the failed download")
    # A stage that is not complete is never applied.
    unready = world.engine("preflight", "--stage", stage)
    expect_equal((unready.code, unready.value("error_reason")), (1, "staged_manifest"), "preflight on an incomplete stage")
    expect_equal(sorted(entry.name for entry in world.state.iterdir() if entry.name.startswith("run-")), [],
                 "helper copies left by a preflight that refused")

    # The real zip: only what is still missing is asked for.
    feed.set_root(v2.folder)
    feed.clear_log()
    world.stage(feed)
    missing = [path for path in changed_paths(world.files, v2.package) if path not in before]
    expect(victim in missing, "the bad file is not among those still to fetch")
    expect_ranges_served(zip_requests(feed, v2), merged_ranges(member_span(v2.zip_path, path) for path in missing),
                         "the resumed download")
    done = world.swap(world.preflight(stage))
    expect_equal((done.code, done.value("outcome")), (EXIT_OK, "committed"), "the swap of the resumed stage")
    world.expect_tree(world.after_update_to(v2), "after the update")


@case("locked_file")
def locked_file(bench: Bench, args: argparse.Namespace) -> None:
    """A file of the install that another program holds open: the commit cannot move it, everything that
    was already moved is put back, and the old version is started again."""
    bench.needs_programs()
    v2 = bench.release("v2")
    world = World(bench, "locked_file")
    feed = bench.feed(v2.folder)
    stage = world.stage(feed)
    prepared = world.preflight(stage)
    # A library: the data files are committed before it (apply.hpp, step 5), so the rollback has files to
    # put back. Python opens without FILE_SHARE_DELETE, which is what keeps a file from being renamed.
    held = world.root / "bin" / "libcore-1.dll"
    with open(held, "rb"):
        rolled = world.swap(prepared, timings=TIMINGS_SHORT_RETRY, relaunch=True)
    expect_equal((rolled.code, rolled.value("outcome"), rolled.value("reason")),
                 (EXIT_ROLLED_BACK, "rolled_back", "rename_failed"), "the swap with a file held open")
    world.expect_untouched("after the rollback")
    last = world.last()
    expect_equal((last.get("status"), last.get("reason"), last.get("version"), last.get("tx")),
                 ("rolled_back", "rename_failed", V2_VERSION, prepared.transaction), "last.json")
    old = world.wait_for_stub("start ")
    expect(old.startswith("start tag=v1 "), f"the version started after the rollback: {old}")
    expect(not any(line.startswith("health ") for line in world.stub_lines()),
           f"the new version was started although the commit failed: {world.stub_lines()}")
    expect_equal(world.blocked_sequences(), [], "releases blocked after a failure that says nothing about the release")

    # Nothing but the open file was wrong: the same stage installs once it is let go.
    done = world.swap(world.preflight(stage))
    expect_equal((done.code, done.value("outcome")), (EXIT_OK, "committed"), "the swap once the file is free")
    world.expect_tree(world.after_update_to(v2), "after the update")


@case("crash_at_every_step")
def crash_at_every_step(bench: Bench, args: argparse.Namespace) -> None:
    """The helper is killed before its N-th change to the disk, for every N until a run is not killed
    (--die-at: every line of the journal, every copy, every rename), and recovery follows. The install is
    then v1 or v2, whole - never some of each."""
    bench.needs_programs()
    first, step = args.shard
    v2 = bench.release("v2", big=False)
    master = World(bench, "crash-master", big=False)
    feed = bench.feed(v2.folder)
    master_stage = master.stage(feed)
    feed.stop()
    v1_files = master.files
    v2_files = master.after_update_to(v2)
    v1_hash, v2_hash = tree_hash(snapshot_of(v1_files)), tree_hash(snapshot_of(v2_files))

    outcomes: list[str] = []      # per killed run, after recovery: "v1" or "v2"
    half_swapped = 0              # killed runs that left some of each BEFORE recovery
    through_the_engine = 0        # of those, recovered by update::recover, as the next start of a program would
    number = first
    while True:
        expect(number <= 5000, "no run got to the end within 5000 changes to the disk")
        world = World(bench, f"crash-{number}", big=False)
        shutil.copytree(master_stage, world.state / master_stage.name)
        stage = world.state / master_stage.name
        prepared = world.preflight(stage)
        ran = world.swap(prepared, "--die-at", number)
        if ran.code != EXIT_SIMULATED_CRASH:
            expect_equal((ran.code, ran.value("outcome")), (EXIT_OK, "committed"),
                         f"the run that was not killed (--die-at {number})")
            world.expect_tree(v2_files, f"after the run that was not killed (--die-at {number})")
            break
        left = tree_hash(snapshot(world.root))
        mixed = left not in (v1_hash, v2_hash)
        if mixed:
            half_swapped += 1
        if mixed and through_the_engine == 0:
            # The first half-swapped tree is put right the way the next start of any program does it.
            recovered = world.engine("recover")
            expect_equal((recovered.code, recovered.values("recover")), (0, ["ok"]),
                         f"update::recover after a crash before change {number}")
            through_the_engine += 1
        else:
            recovered = bench.run([prepared.helper, "--recover", world.root], env=world.environment(),
                                  cwd=prepared.helper.parent)
            expect_equal(recovered.code, 0, f"the exit code of --recover after a crash before change {number}")
        after = snapshot(world.root)
        if tree_hash(after) == v1_hash:
            outcomes.append("v1")
        elif tree_hash(after) == v2_hash:
            outcomes.append("v2")
        else:
            raise Failure(f"after a crash before change {number} and recovery the install is neither version:\n"
                          f"against v1:\n{tree_difference(snapshot_of(v1_files), after)}\n"
                          f"against v2:\n{tree_difference(snapshot_of(v2_files), after)}")
        if not args.keep:
            shutil.rmtree(world.top, ignore_errors=True)
            bench.transcript = bench.transcript[-40:]  # the last runs are what a failure needs
        number += step

    bench.note(f"killed at {len(outcomes)} changes from {first} in steps of {step}: {outcomes.count('v1')} left v1, "
               f"{outcomes.count('v2')} left v2; {half_swapped} were half swapped before recovery")
    # The loop is shown to reach what it claims: runs killed before anything moved, runs killed in the
    # middle of the commit, and the update's own end.
    expect(outcomes and outcomes[0] == "v1", f"the first crash (before change {first}) did not leave v1: {outcomes[:3]}")
    expect(half_swapped >= 1, "no run was killed in the middle of the commit")
    expect_equal(through_the_engine, 1, "half-swapped trees recovered through update::recover")
    # Once the commit is over a crash leaves v2; before that, v1. The order never turns back.
    expect_equal(outcomes, sorted(outcomes), "the versions left by recovery, in the order of the crashes")


@case("downgrade")
def downgrade(bench: Bench, args: argparse.Namespace) -> None:
    """A validly signed release that is not above what is installed is not offered - by version first, by
    sequence for the same version - and the helper refuses it by itself when it is handed one anyway."""
    bench.needs_programs()
    # 0.9.0 built AFTER the installed 0.10.0: a lower version is never newer, whatever its sequence.
    older_built_later = bench.produce("v1-built-later", bench.package("v1"), V1_VERSION, V2_SEQUENCE + 3600)
    # The installed version from an earlier build, and from a later one: the sequence decides.
    same_built_earlier = bench.produce("v2-built-earlier", bench.package("v2"), V2_VERSION, V2_SEQUENCE - 1)
    same_built_later = bench.produce("v2-built-later", bench.package("v2"), V2_VERSION, V2_SEQUENCE + 60)

    world = World(bench, "downgrade", installed="v2")
    for release, what in ((older_built_later, "a lower version built later"),
                          (same_built_earlier, "the installed version from an earlier build")):
        feed = bench.feed(release.folder)
        asked = world.ask("check", feed)
        expect_equal((asked.code, asked.value("update"), asked.value("latest"), asked.value("sequence")),
                     (0, "current", release.version, str(release.sequence)), f"check against {what}")
        expect_nothing_fetched_or_staged(world, feed, release, what)
        feed.stop()
    # The control: the same comparison does offer a build above the installed one.
    feed = bench.feed(same_built_later.folder)
    expect_equal(world.ask("check", feed).value("update"), "available",
                 "check against the installed version from a LATER build (a workflow run again)")
    feed.stop()

    # Handed to the helper all the same: it compares for itself (apply.hpp, step 1) and touches nothing.
    feed = bench.feed(older_built_later.folder)
    stage = world.stage(feed, "--even-if-not-newer")
    refused = world.swap(world.preflight(stage))
    expect_equal((refused.code, refused.value("outcome"), refused.value("reason")),
                 (EXIT_VERIFICATION, "refused", "downgrade"), "the helper, handed a downgrade")
    world.expect_untouched("after the refused downgrade")


@case("replayed_release")
def replayed_release(bench: Bench, args: argparse.Namespace) -> None:
    """A release that was once the newest, served again byte for byte after a newer one is installed: its
    signature is as good as it ever was, and it is still not offered, nor applied."""
    bench.needs_programs()
    v1, v2 = bench.release("v1"), bench.release("v2")
    world = World(bench, "replayed_release", installed="v2")
    for release, what in ((v1, "the old release replayed"), (v2, "the installed release itself")):
        feed = bench.feed(release.folder)
        asked = world.ask("check", feed)
        expect_equal((asked.code, asked.value("update"), asked.value("latest")), (0, "current", release.version),
                     f"check against {what}")
        expect_nothing_fetched_or_staged(world, feed, release, what)
        feed.stop()

    feed = bench.feed(v1.folder)
    stage = world.stage(feed, "--even-if-not-newer")
    refused = world.swap(world.preflight(stage))
    expect_equal((refused.code, refused.value("outcome"), refused.value("reason")),
                 (EXIT_VERIFICATION, "refused", "downgrade"), "the helper, handed the old release")
    world.expect_untouched("after the refused replay")


def flip_one_hash_digit(manifest: bytes) -> bytes:
    """`manifest` with one bit of one hash digit changed, so that it is still a manifest of the same length
    with a hash of the right form - and no longer the bytes that were signed."""
    at = manifest.index(b'"sha256":"') + len(b'"sha256":"')
    for bit in (0x01, 0x02, 0x04, 0x08):
        if chr(manifest[at] ^ bit) in "0123456789abcdef":
            return manifest[:at] + bytes([manifest[at] ^ bit]) + manifest[at + 1:]
    raise Failure("no single bit of the digit can change and leave a hex digit")


@case("tampered_byte", portable=True)
def tampered_byte(bench: Bench, args: argparse.Namespace) -> None:
    """One bit of the manifest changed after it was signed: nothing in it is believed."""
    v2 = bench.release("v2")
    bad = bench.variant(v2, "v2-tampered-manifest")
    original = v2.manifest_path.read_bytes()
    changed = flip_one_hash_digit(original)
    expect(len(changed) == len(original) and sum(a != b for a, b in zip(changed, original)) == 1,
           "the tampered manifest does not differ from the signed one in exactly one byte")
    expect(isinstance(json.loads(changed), dict), "the tampered manifest is no longer JSON")
    bad.manifest_path.write_bytes(changed)
    world = World(bench, "tampered_byte")
    feed = bench.feed(bad.folder)
    expect_unreachable(world.ask("check", feed), "bad_signature", "a manifest with one bit changed")
    expect_nothing_fetched_or_staged(world, feed, bad, "a manifest with one bit changed")


@case("swapped_signature", portable=True)
def swapped_signature(bench: Bench, args: argparse.Namespace) -> None:
    """A good signature of ANOTHER manifest, by the trusted key: it covers the bytes it was made for and
    no others."""
    v1, v2 = bench.release("v1"), bench.release("v2")
    bad = bench.variant(v2, "v2-swapped-signature")
    shutil.copyfile(v1.signature_path, bad.signature_path)
    world = World(bench, "swapped_signature")
    feed = bench.feed(bad.folder)
    expect_unreachable(world.ask("check", feed), "bad_signature", "v2's manifest under v1's signature")
    expect_nothing_fetched_or_staged(world, feed, bad, "v2's manifest under v1's signature")


@case("other_key", portable=True)
def other_key(bench: Bench, args: argparse.Namespace) -> None:
    """A release signed by a key this build does not pin - under its own name, and under the pinned key's."""
    v2 = bench.release("v2")
    foreign = bench.produce("v2-other-key", bench.package("v2"), V2_VERSION, V2_SEQUENCE, key="other")
    world = World(bench, "other_key")
    feed = bench.feed(foreign.folder)
    expect_unreachable(world.ask("check", feed), "untrusted_key", "a release signed by a key nobody pinned")
    expect_nothing_fetched_or_staged(world, feed, foreign, "a release signed by a key nobody pinned")
    feed.stop()

    # The same signature, said to be the active key's: the line `ed25519 <key id> <base64>` with the id of
    # the release that IS signed by the active key.
    kind, _, signature = foreign.signature_path.read_text(encoding="ascii").split()
    trusted_id = v2.signature_path.read_text(encoding="ascii").split()[1]
    claimed = bench.variant(foreign, "v2-other-key-claims-active")
    claimed.signature_path.write_bytes(f"{kind} {trusted_id} {signature}\n".encode("ascii"))
    feed = bench.feed(claimed.folder)
    expect_unreachable(world.ask("check", feed), "bad_signature", "another key's signature under the pinned key's id")
    expect_nothing_fetched_or_staged(world, feed, claimed, "another key's signature under the pinned key's id")


@case("wrong_target", portable=True)
def wrong_target(bench: Bench, args: argparse.Namespace) -> None:
    """A validly signed release of another platform, served under this platform's file names: the target
    that counts is the signed one."""
    arm = bench.produce("v2-win-arm64", bench.package("v2"), V2_VERSION, V2_SEQUENCE, target="win-arm64")
    renamed = bench.work / "release" / "v2-win-arm64-as-win64"
    renamed.mkdir(parents=True)
    shutil.copyfile(arm.manifest_path, renamed / f"Katana-update-{TARGET}.json")
    shutil.copyfile(arm.signature_path, renamed / f"Katana-update-{TARGET}.json.sig")
    shutil.copyfile(arm.zip_path, renamed / arm.zip_path.name)
    served = Release(renamed, V2_VERSION, V2_SEQUENCE, arm.package)
    world = World(bench, "wrong_target")
    feed = bench.feed(renamed)
    expect_unreachable(world.ask("check", feed), "wrong_target", "a win-arm64 release under the win64 names")
    expect_equal([r["path"].split("?")[0] for r in feed.requests() if r["path"].endswith(".zip")], [],
                 "requests for a zip of the other platform")
    expect_nothing_fetched_or_staged(world, feed, served, "a win-arm64 release under the win64 names")


@case("program_running")
def program_running(bench: Bench, args: argparse.Namespace) -> None:
    """A program of the install is still running: the helper waits, gives up, names it, and has touched
    nothing. It never kills. Once the program is gone the same stage installs."""
    bench.needs_programs()
    v2 = bench.release("v2")
    world = World(bench, "program_running")
    feed = bench.feed(v2.folder)
    stage = world.stage(feed)
    prepared = world.preflight(stage)
    # The install's own katana_cli, left running: the stub, started here with the box for a file that is
    # not a program turned off (main(), below) - it IS a program, and this process holds its handle.
    program = world.root / "bin" / "katana_cli.exe"
    sleeper = subprocess.Popen([str(program), "--sleep", "120000"], env=bench.environment(world.environment()),
                               cwd=str(world.top), stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL)
    bench.children.append(sleeper)
    world.wait_for_stub("sleep tag=v1 ")
    blocked = world.swap(prepared)
    expect_equal((blocked.code, blocked.value("outcome"), blocked.value("reason")), (EXIT_BLOCKED, "blocked", "blocked"),
                 "the swap while a program of the install runs")
    expect_equal(blocked.values("holder"), [f"katana_cli.exe ({sleeper.pid})"], "the programs the helper names")
    expect(sleeper.poll() is None, "the program that blocked the update is no longer running: the helper never kills")
    world.expect_untouched("after the blocked swap")
    last = world.last()
    expect_equal((last.get("status"), last.get("reason"), last.get("holder")),
                 ("blocked", "blocked", f"katana_cli.exe ({sleeper.pid})"), "last.json")
    expect((stage / "manifest.json").is_file(), "the stage is gone after a blocked swap")

    sleeper.kill()  # the test's own process; the person would close the program
    sleeper.wait(timeout=60)
    done = world.swap(world.preflight(stage))
    expect_equal((done.code, done.value("outcome")), (EXIT_OK, "committed"), "the swap once the program is closed")
    world.expect_tree(world.after_update_to(v2), "after the update")


@case("new_version_exits_1")
def new_version_exits_1(bench: Bench, args: argparse.Namespace) -> None:
    """The new version starts and exits with an error: the old one is put back, started again, and this
    release is not offered a second time."""
    bench.needs_programs()
    v2 = bench.release("v2")
    world = World(bench, "new_version_exits_1")
    feed = bench.feed(v2.folder)
    stage = world.stage(feed)
    prepared = world.preflight(stage)
    world.stub_mode = "exit1"
    rolled = world.swap(prepared, relaunch=True)
    expect_equal((rolled.code, rolled.value("outcome"), rolled.value("reason")),
                 (EXIT_ROLLED_BACK, "rolled_back", "health_exit"), "the swap to a version that exits with 1")
    world.expect_untouched("after the rollback")
    last = world.last()
    expect_equal((last.get("status"), last.get("reason"), last.get("version")),
                 ("rolled_back", "health_exit", V2_VERSION), "last.json")
    # It was the NEW version that was started for the check, and the OLD one afterwards.
    health = world.wait_for_stub("health ")
    expect(health.startswith("health tag=v2 ") and health.endswith(f"tx={prepared.transaction} mode=exit1"),
           f"the health check's start: {health}")
    old = world.wait_for_stub("start ")
    expect(old.startswith("start tag=v1 "), f"the version started after the rollback: {old}")
    expect_equal(world.blocked_sequences(), [V2_SEQUENCE], "the sequences state.json holds as blocked")

    world.stub_mode = "healthy"
    again = world.ask("check", feed)
    expect_equal((again.code, again.value("update"), again.value("reason")), (0, "blocked", "blocked_before"),
                 "check after the release failed here")
    feed.clear_log()
    staged = world.ask("stage", feed)
    expect_equal(staged.values("staged"), ["none"], "stage of a release that is blocked")
    expect_equal(zip_requests(feed, v2), [], "requests for the zip of a blocked release")


def header_values(requests: list[dict]) -> str:
    return "\n".join(f"{name}: {value}" for request in requests for name, value in request["headers"].items())


@case("token_first_hop_only")
def token_first_hop_only(bench: Bench, args: argparse.Namespace) -> None:
    """Where the access token goes: to api.github.com with the request the engine wrote, and to no address
    a server answered with - the storage host a download is redirected to gets none. And a feed of the
    owner's own gets none at all."""
    bench.needs_programs()
    v2 = bench.release("v2")

    # ---- a feed of the owner's own that redirects every file to another port ---------------------
    world = World(bench, "token-own-feed")
    changed = changed_paths(world.files, v2.package)
    wanted = merged_ranges(member_span(v2.zip_path, path) for path in changed)
    feed = bench.feed(v2.folder, fake.Rule("redirect", "*"))
    staged = world.ask("stage", feed, "--token", TOKEN)
    expect_equal((staged.code, staged.values("staged")), (0, [V2_VERSION]), "stage through a redirecting feed")
    first, second = feed.requests("first"), feed.requests("second")
    expect_equal({r["status"] for r in first}, {302}, "the statuses of the feed's port")
    expect_ranges_served(zip_requests(feed, v2, "second"), wanted, "the download through the redirect")
    for request in zip_requests(feed, v2, "second"):
        expect_equal(request["headers"].get("range"), f"bytes={request['range'][0]}-{request['range'][1]}",
                     "the Range header on the second hop")
    for request in first + second:
        expect("authorization" not in request["headers"], f"a feed of the owner's own was sent the token: {request}")
    expect(TOKEN not in header_values(first + second), "the token is in a header sent to a feed of the owner's own")

    # ---- GitHub's private route: the token to the API, nothing to the storage host ---------------
    world = World(bench, "token-private-route")
    feed = bench.feed(v2.folder, github=GITHUB_REPO, token=TOKEN, storage_base=f"https://{STORAGE_HOST}")
    routes = ["--connect", f"github.com={feed.port}", "--connect", f"api.github.com={feed.port}",
              "--connect", f"{STORAGE_HOST}={feed.second_port}"]
    private = ["--github", GITHUB_REPO, *routes]
    staged = world.engine("stage", *private, "--token", TOKEN)
    expect_equal((staged.code, staged.values("staged"), staged.value("update"), staged.value("route")),
                 (0, [V2_VERSION], "available", "api"), "stage by the private route")
    expect(TOKEN not in staged.out + staged.err, "the token is in what the program printed")
    first, second = feed.requests("first"), feed.requests("second")
    anonymous = [r for r in first if r["headers"].get("host") == "github.com"]
    api = [r for r in first if r["headers"].get("host") == "api.github.com"]
    expect_equal(len(anonymous) + len(api), len(first), "requests to a host that is neither github.com nor its API")
    # The public address is tried first, without the token; its 404 is what sends the engine to the API.
    expect_equal([(r["path"], r["status"]) for r in anonymous],
                 [(f"/{GITHUB_REPO}/releases/latest/download/Katana-update-{TARGET}.json.sig", 404)],
                 "what github.com was asked")
    expect("authorization" not in anonymous[0]["headers"], "github.com was sent the token")
    expect(len(api) >= 5, f"the API was asked {len(api)} times (two lookups and three assets at least)")
    for request in api:
        expect(request["path"].startswith(f"/repos/{GITHUB_REPO}/releases/"), f"not an API address: {request['path']}")
        expect_equal(request["headers"].get("authorization"), f"Bearer {TOKEN}", f"Authorization for {request['path']}")
        expect_equal(request["headers"].get("x-github-api-version"), "2022-11-28", "the API version header")
        expect(request["status"] in (200, 302), f"the API refused {request['path']}: {request['status']}")
    expect(second, "nothing was fetched from the storage host: the test would prove nothing")
    for request in second:
        expect("authorization" not in request["headers"], f"the storage host was sent Authorization: {request}")
        expect_equal(request["headers"].get("host"), STORAGE_HOST, "the host of a second hop")
        expect("sig=FAKE-SIGNED-ADDRESS-SECRET" in request["path"], f"not the address the API redirected to: {request['path']}")
    expect(TOKEN not in header_values(second), "the token is in a header sent to the storage host")
    expect_ranges_served(zip_requests(feed, v2, "second"), wanted, "the download by the private route")
    stage = Path(staged.value("stage"))
    expect_equal(expect_only_verified(stage, v2, "the stage"), {path: sha256_hex(v2.package[path]) for path in changed},
                 "the files staged by the private route")
    expect_equal((stage / "manifest.json").read_bytes(), v2.manifest_path.read_bytes(), "the staged manifest")

    # ---- no token, or another: the private repository says 404 and that is the answer -------------
    for extra, what in (([], "without a token"), (["--token", "not-" + TOKEN], "with a token the repository does not know")):
        asked = World(bench, f"token-{len(extra)}").engine("check", *private, *extra)
        expect_unreachable(asked, "http_404", f"check of a private repository {what}")

    # ---- the storage host refuses: neither the token nor the signed address is in what is said ----
    feed.set_rules([fake.Rule("status", "*.zip", 403, "second")])
    refused = World(bench, "token-storage-refuses").engine("stage", *private, "--token", TOKEN)
    expect_equal((refused.code, refused.value("error_reason")), (1, "http_403"), "stage when the storage host answers 403")
    said = refused.out + refused.err
    expect(TOKEN not in said, "the token is in the failure's text")
    expect("FAKE-SIGNED-ADDRESS-SECRET" not in said, "the signed address's query is in the failure's text")


@case("feed_unreachable", portable=True)
def feed_unreachable(bench: Bench, args: argparse.Namespace) -> None:
    """Every way of not learning the answer is the answer `unreachable` with its reason (check.hpp), never an
    error and never an offer."""
    v2 = bench.release("v2")
    world = World(bench, "feed_unreachable")
    feed = bench.feed(v2.folder)
    for rule, reason in ((fake.Rule("status", "*.sig", 403), "http_403"),
                         (fake.Rule("status", "*", 429), "http_429"),
                         (fake.Rule("status", "*", 404), "http_404"),
                         (fake.Rule("status", "*.json", 403), "http_403"),
                         (fake.Rule("html", "*"), "html_page")):
        feed.set_rules([rule])
        expect_unreachable(world.ask("check", feed), reason, f"check against a feed that answers {rule.kind} {rule.value}")
    feed.set_rules([])
    expect_equal(world.ask("check", feed).value("update"), "available", "check once the feed answers again")
    # Nothing listens: the port of a feed that has stopped.
    gone = f"http://127.0.0.1:{feed.port}"
    feed.stop()
    expect_unreachable(world.engine("check", "--feed", gone), "offline", "check against a port nobody listens on")
    world.expect_untouched("after the checks")
    expect_equal(sorted(entry.name for entry in world.state.iterdir()), [], "what the state folder holds after checks")


@case("download_fails")
def download_fails(bench: Bench, args: argparse.Namespace) -> None:
    """A network that will not give part of a file, and a connection that drops in the middle of one: the
    download fails with its reason, nothing unverified is kept, and it succeeds once the feed behaves."""
    bench.needs_programs()
    v2 = bench.release("v2")
    world = World(bench, "download_fails")
    feed = bench.feed(v2.folder, fake.Rule("ignore-range", "*.zip"))
    whole = world.ask("stage", feed)
    expect_equal((whole.code, whole.value("error"), whole.value("error_reason")), (1, "Unsupported", "partial_downloads"),
                 "stage against a feed that ignores Range")
    expect_equal(whole.value("error_message"), "this network does not allow partial downloads",
                 "the sentence for a network that ignores Range (ranges.hpp)")
    stage = Path(whole.value("stage"))
    expect_equal(staged_files(stage), {}, "what the stage holds after a refused download")
    expect(not (stage / "manifest.json").exists(), "a stage that failed says it is complete")

    feed.set_rules([fake.Rule("truncate", "*.zip")])
    cut = world.ask("stage", feed)
    expect_equal((cut.code, cut.value("error"), cut.value("error_reason")), (1, "FileImportFailure", "offline"),
                 "stage when the connection drops inside a range")
    expect_equal(staged_files(stage), {}, "what the stage holds after a dropped connection")
    expect(not (stage / "manifest.json").exists(), "a stage that failed says it is complete")
    world.expect_untouched("after the failed downloads")

    feed.set_rules([])
    expect_equal(world.stage(feed), stage, "the stage folder once the feed behaves")
    done = world.swap(world.preflight(stage))
    expect_equal((done.code, done.value("outcome")), (EXIT_OK, "committed"), "the swap of the stage")
    world.expect_tree(world.after_update_to(v2), "after the update")


# ---- the command line ---------------------------------------------------------------------------


def parse_shard(text: str) -> tuple[int, int]:
    """I/N: of the crashes, those before change I, I+N, I+2N ... (1/1: all of them)."""
    match = re.fullmatch(r"([0-9]+)/([0-9]+)", text)
    if not match or not 1 <= int(match.group(1)) <= int(match.group(2)):
        raise argparse.ArgumentTypeError(f"'{text}' is not I/N with 1 <= I <= N")
    return int(match.group(1)), int(match.group(2))


def turn_error_boxes_off() -> None:
    """No box of the system's may wait for a click under a test: for a program that cannot start, for one
    that crashes. The mode is inherited by what this process starts (update::Launcher does the same for
    what it starts: src/katana_update/spawn_win.cpp)."""
    if os.name != "nt":
        return
    import ctypes

    sem_failcriticalerrors, sem_nogpfaulterrorbox, sem_noopenfileerrorbox = 0x0001, 0x0002, 0x8000
    ctypes.windll.kernel32.SetErrorMode(sem_failcriticalerrors | sem_nogpfaulterrorbox | sem_noopenfileerrorbox)


def remove_work(work: Path) -> None:
    """The work folder goes; a program that is still on its way out (the one the helper starts last is
    detached) holds its own file for a moment."""
    for _ in range(100):
        shutil.rmtree(work, ignore_errors=True)
        if not work.exists():
            return
        time.sleep(0.1)
    print(f"note: {work} could not be removed entirely", file=sys.stderr)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="update_e2e.py",
        description="One end-to-end case of the update system: the real producer, a fake feed, the real engine and helper.",
        epilog=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--case", choices=sorted(CASES), help="the case to run")
    parser.add_argument("--list", action="store_true", help="print the cases, a line each (`portable` after those that start no program)")
    parser.add_argument("--engine", type=Path, help="katana_update_e2e")
    parser.add_argument("--helper", type=Path, help="katana_updater_test: the helper built with the test key")
    parser.add_argument("--stub", type=Path, help="katana_updater_stub: the stand-in for Katana")
    parser.add_argument("--shard", type=parse_shard, default=(1, 1), metavar="I/N",
                        help="crash_at_every_step only: the crashes before change I, I+N, I+2N ...")
    parser.add_argument("--keep", action="store_true", help="leave the work folder and print where it is")
    parser.add_argument("--registered", nargs="*", metavar="CASE",
                        help="instead of running a case: fail unless these are exactly the cases there are "
                             "(tests/update/e2e.cmake says which it registered)")
    parser.add_argument("--portable-only", action="store_true",
                        help="with --registered: only the portable cases are expected")
    return parser


def check_registered(registered: list[str], portable_only: bool) -> int:
    """0 when `registered` names every case that can run here, once each, and nothing else."""
    expected = sorted(name for name, (_, portable) in CASES.items() if portable or not portable_only)
    problems = [f"registered twice: {name}" for name in sorted(set(registered)) if registered.count(name) > 1]
    problems += [f"not registered: {name}" for name in expected if name not in registered]
    problems += [f"registered, but no case of the driver{' that runs here' if portable_only else ''}: {name}"
                 for name in sorted(set(registered)) if name not in expected]
    for problem in problems:
        print(problem)
    if not problems:
        print(f"all {len(expected)} cases are registered")
    return 1 if problems else 0


def main(argv: Optional[list[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="backslashreplace")  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            pass
    if args.list:
        for name in sorted(CASES):
            print(f"{name} portable" if CASES[name][1] else name)
        return 0
    if args.registered is not None:
        return check_registered(args.registered, args.portable_only)
    if not args.case:
        parser.error("give --case (or --list, or --registered)")
    if not args.engine:
        parser.error("give --engine: every case asks the feed through katana_update_e2e")
    for given in (args.engine, args.helper, args.stub):
        if given is not None and not given.is_file():
            parser.error(f"{given} is not a file")
    turn_error_boxes_off()

    # Its long spelling, without an 8.3 name in it: the paths go to the helper and into comparisons.
    work = Path(tempfile.mkdtemp(prefix="katana-e2e-")).resolve()
    bench = Bench(work, args.engine.resolve(), args.helper.resolve() if args.helper else None,
                  args.stub.resolve() if args.stub else None)
    function, _ = CASES[args.case]
    try:
        function(bench, args)
    except Exception as failure:  # noqa: BLE001  (whatever went wrong, what was run is what explains it)
        if isinstance(failure, Failure):
            print(f"FAILED {args.case}: {failure}\n")
        else:
            print(f"FAILED {args.case}: the test itself stopped on an error\n{traceback.format_exc()}")
        print("---- what was run ----")
        print("\n".join(bench.transcript))
        for number, served in enumerate(bench.feeds, 1):
            print(f"---- what feed {number} was asked ----")
            for record in served.requests(settle_seconds=1.0):
                print(json.dumps(record, sort_keys=True))
        return 1
    finally:
        bench.close()
        if args.keep:
            print(f"kept: {work}")
        else:
            remove_work(work)
    for line in bench.transcript:
        if line.startswith("# "):
            print(line[2:])
    print(f"passed {args.case}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
