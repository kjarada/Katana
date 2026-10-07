#!/usr/bin/env python3
"""Producer of Katana's update assets: the manifest, the update zip and, with a key, the signature.

Design: docs/update.md; the contract is include/katana/update/manifest.hpp, paths.hpp and
signature.hpp. For one release and one target token (win64 win-arm64 linux-x86_64 linux-aarch64
macos-arm64) it reads the unpacked install tree (the folder that holds bin/ and share/) and writes

    <out>/Katana-update-<token>.json            the manifest: UTF-8, LF, sorted keys, files sorted
    <out>/Katana-<ver>-<token>.update.zip       every install file at its root-relative path, each member
                                                deflated ALONE with fixed timestamps (a standard zip, so
                                                Python's zipfile reads it), ordered program, lib, data, seed
    <out>/Katana-update-<token>.json.sig        with --key: `ed25519 <keyid> <base64>`, an Ed25519
                                                signature over the manifest's exact bytes, made with the
                                                openssl command line (pkeyutl -sign -rawin)

The class of a file, first rule that fits: seed, if it matches a --seed pattern (written only if absent,
so user edits survive); lib, if it is a .dll .so .so.N .dylib or .pyd file or lies under lib/; program,
if it lies under bin/ and is an .exe, or has an execute bit (POSIX), or is named like a --program (the
extension, if .exe, left off; case does not matter); data, everything else. Programs are committed
last by the helper. The execute bit is read only on a POSIX file system, and "exec": true is written for
every program and every file that has the bit.

`offset` and `csize` of each file locate its raw deflate data inside the zip, so the engine can fetch one
file by an HTTP Range request without reading the zip's directory. `--notify-only` (required for
macos-arm64, whose bundle cannot be patched file by file) writes a manifest with no files and no
payload, and no zip.

The path rules of the design (paths.hpp) are enforced HERE as well as in the engine and the helper, so
that a release which no install would accept cannot be made. Every violation is listed and nothing is
written. The rules: relative, '/', first component one of bin share etc lib; printable ASCII only; no
'..', '.', empty component, ':', backslash or any of * ? " < > |; no component ending in '.' or ' '; no
Windows device name (CON PRN AUX NUL COM1-9 LPT1-9, with or without an extension); no 8.3 form
('~' and a digit); no updater record (install.json, installed.json, installed.manifest.json(.sig)) and
no '.update' component; no two paths equal after case folding; no file that is also the folder of
another (bin/a and bin/A/b, case folded); at most 10,000 files and 2 GiB. The one file the producer
leaves out by itself is share/katana/install.json, which the CPack install writes and the updater
owns. A symbolic link, a file that is not a regular file, and a file or folder that cannot be read are
refused: the manifest has no way to say them, and leaving them out would make every installed copy
delete them.

The sequence is UTC epoch seconds: --sequence, else SOURCE_DATE_EPOCH, else now. `published` is that
instant and `expires` is 180 days later, so the same inputs make the same bytes (the zip too, for one
zlib). Give every target of a release the same sequence (tools/update_verify_release.py fails a release
whose manifests differ in sequence, version or tag): one --sequence, or one SOURCE_DATE_EPOCH, for the
whole loop over the targets.

--key signs, and only with a key the tree pins: --keys-cmake (default cmake/KatanaUpdateKeys.cmake) must
exist and hold real keys, never the placeholders, and never a published test key (tests pass --test-key).

Exit status: 0 made, 1 refused or failed (nothing written), 2 a usage error.
"""

from __future__ import annotations

import argparse
import base64
import datetime
import fnmatch
import hashlib
import json
import os
import re
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import time
import zipfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Mapping, Optional, Union

# ---- the contract's constants (manifest.hpp, paths.hpp, signature.hpp) --------------------------

SCHEMA = 1
DEFAULT_APPLIER = 1
PRODUCT = "katana"
DEFAULT_CHANNEL = "stable"
DEFAULT_REPO = "kjarada/Katana"
TARGETS = ("win64", "win-arm64", "linux-x86_64", "linux-aarch64", "macos-arm64")
NOTIFY_ONLY_TARGETS = ("macos-arm64",)
INSTALL_ROOTS = ("bin", "share", "etc", "lib")
MAX_FILES = 10_000
MAX_INSTALL_BYTES = 2 * 1024 * 1024 * 1024
MAX_MANIFEST_BYTES = 8 * 1024 * 1024  # kMaxManifestBytes (feed.hpp): the engine refuses a larger one
EXPIRES_AFTER_DAYS = 180
CLASS_ORDER = ("program", "lib", "data", "seed")
DEFAULT_PROGRAMS = ("katana", "katana_cli", "katana_mcp", "katana_updater")
# The updater's own records: never in a manifest. share/katana/install.json is the one the CPack
# install writes into a package, so it is skipped rather than refused.
RESERVED_NAMES = ("install.json", "installed.json", "installed.manifest.json", "installed.manifest.json.sig")
CPACK_INSTALL_RECORD = "share/katana/install.json"
FIXED_ZIP_TIME = (1980, 1, 1, 0, 0, 0)  # the zip format's earliest date
ZIP_LEVEL = 9
CHUNK = 1 << 20

# ---- keys ---------------------------------------------------------------------------------------

# The DER prefix of an Ed25519 SubjectPublicKeyInfo (RFC 8410): the 32 key bytes follow.
SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")
SIGNATURE_BYTES = 64

# The published RFC 8032 section 7.1 test keys (TEST 1, 2, 3), public halves. Anyone can sign with
# the private halves, so a release is never signed with one (refused below) and a test never signs
# with anything else (--test-key).
TEST_PUBLIC_KEYS = {
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a": "RFC 8032 7.1 TEST 1",
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c": "RFC 8032 7.1 TEST 2",
    "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025": "RFC 8032 7.1 TEST 3",
}

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_KEYS_CMAKE = REPO_ROOT / "cmake" / "KatanaUpdateKeys.cmake"


class ToolError(Exception):
    """A failure with a message for the person running the tool."""


class Refusal(ToolError):
    """The tree or the arguments break a rule; `problems` lists every violation."""

    def __init__(self, problems: list[str]):
        super().__init__("; ".join(problems))
        self.problems = problems


# ---- the path rules (paths.hpp) -----------------------------------------------------------------

_DEVICE_NAME = re.compile(r"^(con|prn|aux|nul|com[1-9]|lpt[1-9])$", re.IGNORECASE)
_SHORT_NAME = re.compile(r"~[0-9]")
_FORBIDDEN_CHARACTERS = frozenset('\\:*?"<>|')


def path_violations(path: str) -> list[str]:
    """Every rule `path` breaks, as sentences; empty when it is acceptable in a manifest."""
    if not path:
        return ["the path is empty"]
    found: list[str] = []
    if any(ord(c) < 0x20 or ord(c) > 0x7E for c in path):
        found.append("a character outside printable ASCII (0x20 to 0x7E): names are ASCII only")
    bad = sorted({c for c in path if c in _FORBIDDEN_CHARACTERS})
    if bad:
        found.append("the character(s) " + " ".join(repr(c) for c in bad) + " (a drive, a stream, a separator or a wildcard)")
    if path.startswith("/"):
        found.append("an absolute path")
        return found
    components = path.split("/")
    if len(components) < 2 and components[0] in INSTALL_ROOTS:
        found.append("a file named like an install folder (there must be a folder level below it)")
    if "" in components:
        found.append("an empty component")
    if ".." in components:
        found.append("a '..' component")
    if "." in components:
        found.append("a '.' component")
    first = components[0]
    if first not in INSTALL_ROOTS:
        found.append(f"first component '{first}' is not one of {' '.join(INSTALL_ROOTS)}")
    for component in components:
        if component in ("", ".", ".."):
            continue
        if component[-1] in ". ":
            found.append(f"component '{component}' ends in '.' or ' ' (Windows drops it)")
        if _DEVICE_NAME.match(component.split(".", 1)[0]):
            found.append(f"component '{component}' is a Windows device name")
        if _SHORT_NAME.search(component):
            found.append(f"component '{component}' looks like an 8.3 short name ('~' and a digit)")
        if component.lower() == ".update":
            found.append("a '.update' component (the updater's working folder)")
    if components[-1].lower() in RESERVED_NAMES:
        found.append(f"'{components[-1]}' is one of the updater's own records")
    return found


def set_violations(paths: list[str], sizes: list[int]) -> list[str]:
    """What is wrong with the paths taken together: case-folded duplicates and the two caps."""
    found: list[str] = []
    seen: dict[str, str] = {}
    for path in paths:
        folded = path.lower()
        if folded in seen:
            found.append(f"'{path}' and '{seen[folded]}' are the same name once case is folded")
        else:
            seen[folded] = path
    # A name cannot be a file and also the folder of another file (bin/a and bin/A/b): no Windows
    # volume can hold both, so an install that applies this set fails half way.
    for path in paths:
        parts = path.lower().split("/")
        for depth in range(1, len(parts)):
            parent = "/".join(parts[:depth])
            if parent in seen:
                found.append(f"'{path}' is inside '{seen[parent]}', which is also listed as a file (case folded)")
                break
    if len(paths) > MAX_FILES:
        found.append(f"{len(paths)} files; at most {MAX_FILES} are allowed")
    if sum(sizes) > MAX_INSTALL_BYTES:
        found.append(f"{sum(sizes)} bytes in all; at most {MAX_INSTALL_BYTES} (2 GiB) are allowed")
    return found


# ---- the install tree ---------------------------------------------------------------------------


@dataclass
class TreeFile:
    path: str          # root-relative, '/'
    source: Union[Path, bytes]
    size: int
    executable: bool   # the execute bit on a POSIX file; the manifest's "exec"
    file_class: str = "data"


@dataclass
class Scan:
    files: list[TreeFile] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)    # left out by rule (the CPack install record)
    excluded: list[str] = field(default_factory=list)   # left out by --exclude
    problems: list[str] = field(default_factory=list)


def execute_bit(mode: int, posix: bool = os.name != "nt") -> bool:
    """Whether a file's mode has an execute bit that means something: only on a POSIX file system."""
    return posix and bool(mode & 0o111)


def scan_tree(root: Path, excludes: Iterable[str] = (), posix: bool = os.name != "nt") -> Scan:
    """Walk `root`, collecting the regular files and every problem found on the way.

    A folder or file that cannot be read is a problem, never a silent omission: a manifest that left
    its files out would make every installed copy delete them on the next update."""
    if not root.is_dir():
        raise ToolError(f"{root} is not a directory")
    excludes = list(excludes)
    result = Scan()

    def unreadable(error: OSError) -> None:
        name = getattr(error, "filename", None)
        try:
            shown = os.path.relpath(name, root).replace(os.sep, "/") if name else str(root)
        except ValueError:
            shown = str(name)
        result.problems.append(f"{shown}: cannot be read ({error.strerror or error})")

    for dirpath, dirnames, filenames in os.walk(root, followlinks=False, onerror=unreadable):
        dirnames.sort()
        for name in dirnames:
            full = os.path.join(dirpath, name)
            if os.path.islink(full):
                rel = os.path.relpath(full, root).replace(os.sep, "/")
                result.problems.append(f"{rel}: a symbolic link (a manifest cannot say one)")
        for name in sorted(filenames):
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, root).replace(os.sep, "/")
            try:
                info = os.lstat(full)
            except OSError as error:
                unreadable(error)
                continue
            if stat.S_ISLNK(info.st_mode):
                result.problems.append(f"{rel}: a symbolic link (a manifest cannot say one)")
            elif not stat.S_ISREG(info.st_mode):
                result.problems.append(f"{rel}: not a regular file")
            elif rel.lower() == CPACK_INSTALL_RECORD:
                result.skipped.append(rel)
            elif any(fnmatch.fnmatchcase(rel, pattern) for pattern in excludes):
                result.excluded.append(rel)
            else:
                result.files.append(TreeFile(rel, Path(full), info.st_size, execute_bit(info.st_mode, posix)))
    return result


def classify(path: str, executable: bool, programs: Iterable[str], seeds: Iterable[str]) -> str:
    """program, lib, data or seed (see the module docstring; --seed and --program refine it)."""
    name = path.rsplit("/", 1)[-1]
    lowered = name.lower()
    if any(fnmatch.fnmatchcase(path, pattern) for pattern in seeds):
        return "seed"
    is_library = (
        lowered.endswith((".dll", ".so", ".dylib", ".pyd"))
        or ".so." in lowered
        or path.split("/", 1)[0] == "lib"
    )
    if is_library:
        return "lib"
    if path.split("/", 1)[0] == "bin":
        stem = lowered[:-4] if lowered.endswith(".exe") else lowered
        if lowered.endswith(".exe") or executable or stem in {p.lower() for p in programs}:
            return "program"
    return "data"


def prepare_files(scan: Scan, programs: Iterable[str], seeds: Iterable[str]) -> list[TreeFile]:
    """Check every rule over `scan` and give each file its class; raise Refusal listing everything."""
    problems = list(scan.problems)
    for file in scan.files:
        for violation in path_violations(file.path):
            hint = ""
            if violation.startswith("first component"):
                hint = " (pass --exclude to leave a file out of the update)"
            problems.append(f"{file.path}: {violation}{hint}")
    problems += set_violations([f.path for f in scan.files], [f.size for f in scan.files])
    if problems:
        raise Refusal(problems)
    programs = list(programs)
    seeds = list(seeds)
    for file in scan.files:
        file.file_class = classify(file.path, file.executable, programs, seeds)
        # A program is executable by definition; the bit is what the helper sets where there is one.
        file.executable = file.executable or file.file_class == "program"
    return sorted(scan.files, key=lambda f: f.path)


# ---- the update zip -----------------------------------------------------------------------------


@dataclass
class Placement:
    offset: int
    csize: int
    size: int
    sha256: str


def _set_compression_level(info: zipfile.ZipInfo, level: int) -> None:
    try:
        info.compress_level = level  # Python 3.14 and later
    except AttributeError:
        info._compresslevel = level  # type: ignore[attr-defined]  # 3.7 to 3.13


def _chunks(source: Union[Path, bytes]) -> Iterable[bytes]:
    if isinstance(source, bytes):
        yield source
        return
    with open(source, "rb") as handle:
        while chunk := handle.read(CHUNK):
            yield chunk


def zip_order(files: Iterable[TreeFile]) -> list[TreeFile]:
    """program, lib, data, seed, each by path: the executables sit side by side in the zip."""
    return sorted(files, key=lambda f: (CLASS_ORDER.index(f.file_class), f.path))


def write_update_zip(zip_path: Path, members: Iterable[TreeFile]) -> dict[str, Placement]:
    """Write every member deflated alone, with fixed timestamps; return where each one's bytes are.

    The members are written in the order given. No rule is checked here: that is prepare_files's
    job, and a test of the consumers needs a zip a rule would have refused."""
    placements: dict[str, Placement] = {}
    infos: list[tuple[TreeFile, zipfile.ZipInfo]] = []
    with open(zip_path, "wb") as raw:
        with zipfile.ZipFile(raw, "w", zipfile.ZIP_DEFLATED, allowZip64=True) as archive:
            for member in members:
                info = zipfile.ZipInfo(member.path, date_time=FIXED_ZIP_TIME)
                info.compress_type = zipfile.ZIP_DEFLATED
                info.create_system = 3  # Unix, whatever machine this runs on
                info.external_attr = (0o100755 if member.executable else 0o100644) << 16
                info.file_size = member.size
                _set_compression_level(info, ZIP_LEVEL)
                digest = hashlib.sha256()
                written = 0
                with archive.open(info, "w", force_zip64=member.size >= (1 << 30)) as sink:
                    for chunk in _chunks(member.source):
                        digest.update(chunk)
                        written += len(chunk)
                        sink.write(chunk)
                if written != member.size:
                    raise ToolError(f"{member.path} changed while it was being read ({member.size} bytes, then {written})")
                placements[member.path] = Placement(0, info.compress_size, written, digest.hexdigest())
                infos.append((member, info))
    with open(zip_path, "rb") as raw:
        for member, info in infos:
            raw.seek(info.header_offset)
            header = raw.read(30)
            signature, name_length, extra_length = struct.unpack("<4s22xHH", header)
            if signature != b"PK\x03\x04":
                raise ToolError(f"{member.path}: no local file header where the zip says it is")
            placement = placements[member.path]
            placement.offset = info.header_offset + 30 + name_length + extra_length
            placement.csize = info.compress_size
            if placement.csize < 2:
                raise ToolError(f"{member.path}: the deflate stream is {placement.csize} bytes; a stream is never shorter than 2")
    return placements


# ---- the manifest -------------------------------------------------------------------------------


def _compact(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def encode_manifest(manifest: dict) -> bytes:
    """Sorted keys, one member per line and one file per line, LF, ASCII (so UTF-8), a final LF."""
    lines = []
    for key in sorted(manifest):
        value = manifest[key]
        if key == "files" and value:
            lines.append('"files":[\n' + ",\n".join(_compact(entry) for entry in value) + "\n]")
        else:
            lines.append(json.dumps(key) + ":" + _compact(value))
    encoded = ("{\n" + ",\n".join(lines) + "\n}\n").encode("ascii")
    if len(encoded) > MAX_MANIFEST_BYTES:
        raise ToolError(f"the manifest is {len(encoded)} bytes; the engine accepts at most {MAX_MANIFEST_BYTES}")
    return encoded


def utc_text(seconds: int) -> str:
    return datetime.datetime.fromtimestamp(seconds, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def build_manifest(
    *,
    version: str,
    tag: str,
    target: str,
    repo: str,
    channel: str,
    sequence: int,
    applier: int,
    files: list[dict],
    payload: Optional[dict],
    notify_only: bool,
) -> dict:
    published = datetime.datetime.fromtimestamp(sequence, datetime.timezone.utc)
    manifest = {
        "applier": applier,
        "channel": channel,
        "expires": (published + datetime.timedelta(days=EXPIRES_AFTER_DAYS)).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "files": files,
        "notes_url": f"https://github.com/{repo}/releases/tag/{tag}",
        "product": PRODUCT,
        "published": published.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "schema": SCHEMA,
        "sequence": sequence,
        "tag": tag,
        "target": target,
        "version": version,
    }
    if payload is not None:
        manifest["payload"] = payload
    if notify_only:
        manifest["notify_only"] = True
    return manifest


def file_entry(file: TreeFile, placement: Placement) -> dict:
    entry = {
        "class": file.file_class,
        "csize": placement.csize,
        "offset": placement.offset,
        "path": file.path,
        "sha256": placement.sha256,
        "size": placement.size,
    }
    if file.executable:
        entry["exec"] = True
    return entry


def update_zip_name(version: str, target: str) -> str:
    return f"Katana-{version}-{target}.update.zip"


def manifest_name(target: str) -> str:
    return f"Katana-update-{target}.json"


def resolve_sequence(explicit: Optional[int], environ: Mapping[str, str]) -> int:
    if explicit is not None:
        value = explicit
    elif environ.get("SOURCE_DATE_EPOCH", "") != "":
        text = environ["SOURCE_DATE_EPOCH"]
        if not re.fullmatch(r"[0-9]+", text):
            raise ToolError(f"SOURCE_DATE_EPOCH is '{text}', not a number of seconds")
        value = int(text)
    else:
        value = int(time.time())
    if not 0 < value < 1 << 40:
        raise ToolError(f"the sequence {value} is not a plausible UTC epoch-seconds value")
    return value


# ---- signing with the openssl command line ------------------------------------------------------


def find_openssl(explicit: Optional[str] = None) -> str:
    candidate = explicit or os.environ.get("KATANA_OPENSSL") or shutil.which("openssl")
    if not candidate:
        raise ToolError("openssl was not found: put it on PATH, or pass --openssl or set KATANA_OPENSSL")
    return candidate


def _openssl(openssl: str, *args: str) -> subprocess.CompletedProcess:
    try:
        return subprocess.run([openssl, *args], capture_output=True, check=False)
    except OSError as error:
        raise ToolError(f"could not run {openssl}: {error}") from error


def public_key_of(openssl: str, private_pem: Path) -> bytes:
    """The 32-byte Ed25519 public key of a private-key PEM, as openssl derives it."""
    with tempfile.TemporaryDirectory() as scratch:
        out = Path(scratch) / "public.der"
        done = _openssl(openssl, "pkey", "-in", str(private_pem), "-pubout", "-outform", "DER", "-out", str(out))
        if done.returncode != 0 or not out.exists():
            raise ToolError(f"{private_pem} is not a private key openssl can read: {done.stderr.decode(errors='replace').strip()}")
        der = out.read_bytes()
    if len(der) != len(SPKI_PREFIX) + 32 or not der.startswith(SPKI_PREFIX):
        raise ToolError(f"{private_pem} is not an Ed25519 private key")
    return der[len(SPKI_PREFIX):]


def key_id(public_key: bytes) -> str:
    """The first 16 lower-case hex digits of the key: keyIdOf in signature.hpp."""
    return public_key.hex()[:16]


def public_pem(public_key: bytes) -> str:
    body = base64.encodebytes(SPKI_PREFIX + public_key).decode("ascii")
    return "-----BEGIN PUBLIC KEY-----\n" + body + "-----END PUBLIC KEY-----\n"


def sign_file(openssl: str, private_pem: Path, message: Path) -> bytes:
    """Ed25519 over the exact bytes of `message`: openssl pkeyutl -sign -rawin."""
    with tempfile.TemporaryDirectory() as scratch:
        out = Path(scratch) / "signature.bin"
        done = _openssl(openssl, "pkeyutl", "-sign", "-rawin", "-inkey", str(private_pem), "-in", str(message), "-out", str(out))
        if done.returncode != 0 or not out.exists():
            raise ToolError(f"openssl could not sign: {done.stderr.decode(errors='replace').strip()}")
        signature = out.read_bytes()
    if len(signature) != SIGNATURE_BYTES:
        raise ToolError(f"openssl made a {len(signature)}-byte signature; Ed25519 makes {SIGNATURE_BYTES}")
    return signature


def sign_bytes(openssl: str, private_pem: Path, message: bytes) -> bytes:
    """Ed25519 over exactly `message`: the bytes a caller has read and judged, not a path that may change."""
    with tempfile.TemporaryDirectory() as scratch:
        copy = Path(scratch) / "message"
        copy.write_bytes(message)
        return sign_file(openssl, private_pem, copy)


def verify_file(openssl: str, public_key: bytes, message: Path, signature: bytes) -> bool:
    with tempfile.TemporaryDirectory() as scratch:
        pem = Path(scratch) / "public.pem"
        pem.write_bytes(public_pem(public_key).encode("ascii"))
        sig = Path(scratch) / "signature.bin"
        sig.write_bytes(signature)
        done = _openssl(openssl, "pkeyutl", "-verify", "-pubin", "-inkey", str(pem), "-rawin", "-in", str(message), "-sigfile", str(sig))
        return done.returncode == 0


def signature_line(public_key: bytes, signature: bytes) -> str:
    return f"ed25519 {key_id(public_key)} {base64.b64encode(signature).decode('ascii')}\n"


@dataclass
class PinnedKeys:
    placeholders: bool
    active: str
    spare: str


def read_pinned_keys(path: Path) -> PinnedKeys:
    """The keys cmake/KatanaUpdateKeys.cmake pins. A file that is not there is an error: a mistyped
    path must not switch every key guard off."""
    if not path.is_file():
        raise ToolError(f"{path} does not exist, so the keys the tree pins cannot be checked (--keys-cmake)")
    text = path.read_text(encoding="utf-8")
    flag = re.search(r"set\(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS\s+(ON|OFF)\)", text)
    active = re.search(r'set\(KATANA_UPDATE_KEY_ACTIVE\s+"([0-9a-f]{64})"\)', text)
    spare = re.search(r'set\(KATANA_UPDATE_KEY_SPARE\s+"([0-9a-f]{64})"\)', text)
    if not (flag and active and spare):
        raise ToolError(f"{path} does not hold the three settings of cmake/KatanaUpdateKeys.cmake")
    return PinnedKeys(flag.group(1) == "ON", active.group(1), spare.group(1))


def check_signing_key(public_key: bytes, *, test_key: bool, keys_cmake: Optional[Path]) -> None:
    """Which keys may sign. A release is never signed with a published test key; a test is never
    signed with anything but one, nor with a key the tree pins as its production key."""
    pub_hex = public_key.hex()
    pinned = read_pinned_keys(keys_cmake) if keys_cmake is not None else None
    if test_key:
        if pub_hex not in TEST_PUBLIC_KEYS:
            raise ToolError("--test-key signs only with a published RFC 8032 test key; this key is not one")
        if pinned is not None and not pinned.placeholders and pub_hex in (pinned.active, pinned.spare):
            raise ToolError("this test key equals a production key pinned in cmake/KatanaUpdateKeys.cmake")
        return
    if pub_hex in TEST_PUBLIC_KEYS:
        raise ToolError(
            f"this is the published test key {TEST_PUBLIC_KEYS[pub_hex]}: anyone can sign with it, so a "
            "release is never signed with it (tests pass --test-key)"
        )
    if pinned is None:  # only a caller that passes no file at all; the command line always has one
        return
    if pinned.placeholders:
        raise ToolError(
            "cmake/KatanaUpdateKeys.cmake still holds the placeholder keys, so no build could verify a "
            "signature made now: run tools/update_keytool.py pub first"
        )
    if pub_hex not in (pinned.active, pinned.spare):
        raise ToolError(
            f"key {key_id(public_key)} is not pinned in cmake/KatanaUpdateKeys.cmake (active "
            f"{pinned.active[:16]}, spare {pinned.spare[:16]}): an installed copy would reject the update"
        )


# ---- producing a release ------------------------------------------------------------------------


@dataclass
class Produced:
    manifest: dict
    manifest_path: Path
    zip_path: Optional[Path]
    signature_path: Optional[Path]
    skipped: list[str]
    excluded: list[str]


def produce(
    *,
    tree: Optional[Path],
    out: Path,
    version: str,
    target: str,
    tag: Optional[str] = None,
    repo: str = DEFAULT_REPO,
    channel: str = DEFAULT_CHANNEL,
    sequence: Optional[int] = None,
    applier: int = DEFAULT_APPLIER,
    seeds: Iterable[str] = (),
    programs: Iterable[str] = DEFAULT_PROGRAMS,
    excludes: Iterable[str] = (),
    notify_only: bool = False,
    key: Optional[Path] = None,
    test_key: bool = False,
    keys_cmake: Optional[Path] = DEFAULT_KEYS_CMAKE,
    openssl: Optional[str] = None,
    environ: Optional[Mapping[str, str]] = None,
) -> Produced:
    """Make the assets of one release and one target in `out`. All or nothing: a refusal writes nothing."""
    if target not in TARGETS:
        raise Refusal([f"target '{target}' is not one of {' '.join(TARGETS)}"])
    if not re.fullmatch(r"[0-9]{1,9}(\.[0-9]{1,9}){1,3}", version):
        raise Refusal([f"version '{version}' is not MAJOR.MINOR[.PATCH[.BUILD]] of digits"])
    if target in NOTIFY_ONLY_TARGETS and not notify_only:
        raise Refusal([f"{target} gets a notify-only manifest (its bundle cannot be patched file by file): pass --notify-only"])
    if notify_only and tree is not None:
        raise Refusal(["a notify-only manifest has no files: give no --tree"])
    if not notify_only and tree is None:
        raise Refusal(["--tree is required (the folder that holds bin/ and share/)"])
    tag = tag or f"v{version}"
    if not re.fullmatch(r"[A-Za-z0-9._-]+", tag):
        raise Refusal([f"tag '{tag}' has a character other than letters, digits, '.', '_' and '-'"])
    if not re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", repo):
        raise Refusal([f"repo '{repo}' is not owner/name"])
    if not channel or not re.fullmatch(r"[a-z0-9_-]+", channel):
        raise Refusal([f"channel '{channel}' is not a lower-case word"])
    if applier < 1:
        raise Refusal(["applier must be 1 or more"])
    seq = resolve_sequence(sequence, os.environ if environ is None else environ)

    openssl_path = None
    public_key = None
    if key is not None:
        openssl_path = find_openssl(openssl)
        public_key = public_key_of(openssl_path, key)
        check_signing_key(public_key, test_key=test_key, keys_cmake=keys_cmake)

    skipped: list[str] = []
    excluded: list[str] = []
    files: list[TreeFile] = []
    if tree is not None:
        scan = scan_tree(tree, excludes)
        files = prepare_files(scan, programs, seeds)
        skipped, excluded = scan.skipped, scan.excluded
        if not files:
            raise Refusal([f"{tree} holds no files to put in an update"])

    out.mkdir(parents=True, exist_ok=True)
    manifest_final = out / manifest_name(target)
    zip_final = out / update_zip_name(version, target)
    sig_final = out / (manifest_name(target) + ".sig")
    work = Path(tempfile.mkdtemp(prefix=".make-manifest-", dir=out))
    try:
        zip_path = None
        entries: list[dict] = []
        payload = None
        if not notify_only:
            zip_path = work / zip_final.name
            placements = write_update_zip(zip_path, zip_order(files))
            entries = [file_entry(f, placements[f.path]) for f in files]
            payload = {"codec": "deflate", "name": zip_final.name, "size": zip_path.stat().st_size}
        manifest = build_manifest(
            version=version, tag=tag, target=target, repo=repo, channel=channel, sequence=seq,
            applier=applier, files=entries, payload=payload, notify_only=notify_only,
        )
        manifest_path = work / manifest_final.name
        manifest_path.write_bytes(encode_manifest(manifest))
        sig_path = None
        if key is not None:
            assert openssl_path is not None and public_key is not None
            signature = sign_file(openssl_path, key, manifest_path)
            if not verify_file(openssl_path, public_key, manifest_path, signature):
                raise ToolError("openssl made a signature that does not verify under the key's own public half")
            sig_path = work / sig_final.name
            sig_path.write_bytes(signature_line(public_key, signature).encode("ascii"))
        # Everything is made; only now does anything appear in `out`. The order is the one in which a
        # failure half way (a lock on a file, a power cut) leaves something that fails closed: the zip
        # first (the old manifest then names a zip that does not match), the old signature next (a
        # manifest without a .sig is refused by every consumer), the manifest, and the signature last.
        if zip_path is not None:
            os.replace(zip_path, zip_final)
        if sig_final.exists():
            sig_final.unlink()  # a signature of the manifest this replaces must not outlive it
        os.replace(manifest_path, manifest_final)
        if sig_path is not None:
            os.replace(sig_path, sig_final)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return Produced(manifest, manifest_final, zip_final if not notify_only else None,
                    sig_final if key is not None else None, skipped, excluded)


# ---- the command line ---------------------------------------------------------------------------


def _humanise(count: int) -> str:
    return f"{count / (1 << 20):.1f} MiB"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="update_make_manifest.py",
        description="Make the update manifest, the update zip and (with --key) the signature of one release and one target.",
        epilog=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--tree", type=Path, help="the unpacked install tree: the folder that holds bin/ and share/")
    parser.add_argument("--out", type=Path, required=True, help="the folder the assets are written to")
    parser.add_argument("--version", required=True, help="MAJOR.MINOR[.PATCH[.BUILD]], as project(Katana VERSION)")
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--tag", help="the release tag (default: v<version>)")
    parser.add_argument("--repo", default=DEFAULT_REPO, help=f"owner/name, for the notes URL (default {DEFAULT_REPO})")
    parser.add_argument("--channel", default=DEFAULT_CHANNEL)
    parser.add_argument("--sequence", type=int, help="UTC epoch seconds; else SOURCE_DATE_EPOCH; else now. Give every target of a release the same")
    parser.add_argument("--applier", type=int, default=DEFAULT_APPLIER, help="the minimum helper protocol that can apply it")
    parser.add_argument("--notify-only", action="store_true", help="announce the release without offering to install it (macos-arm64)")
    parser.add_argument("--seed", action="append", default=[], metavar="GLOB", help="a root-relative path pattern whose files are seeds (written only if absent)")
    parser.add_argument("--program", action="append", default=None, metavar="NAME", help=f"a program name in bin/ (default {', '.join(DEFAULT_PROGRAMS)}); .exe files and POSIX executables are programs anyway")
    parser.add_argument("--exclude", action="append", default=[], metavar="GLOB", help="a root-relative path pattern to leave out of the update (a top-level LICENSE, say); listed in the output")
    parser.add_argument("--key", type=Path, help="an Ed25519 private key PEM: also write <manifest>.sig (never printed, copied or written elsewhere)")
    parser.add_argument("--test-key", action="store_true", help="the key is a published RFC 8032 test key (tests only); without it such a key is refused")
    parser.add_argument("--keys-cmake", type=Path, default=DEFAULT_KEYS_CMAKE, help="the file that pins the production keys (default cmake/KatanaUpdateKeys.cmake)")
    parser.add_argument("--openssl", help="the openssl executable (default: KATANA_OPENSSL, else PATH)")
    return parser


def _survive_any_console() -> None:
    """A file name the rules refuse is non-ASCII by definition, and the message that names it must reach
    a console whose code page cannot show it (cp1252 on Windows) instead of dying in the print."""
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(errors="backslashreplace")  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            pass


def main(argv: Optional[list[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    _survive_any_console()
    try:
        made = produce(
            tree=args.tree, out=args.out, version=args.version, target=args.target, tag=args.tag, repo=args.repo,
            channel=args.channel, sequence=args.sequence, applier=args.applier, seeds=args.seed,
            programs=args.program if args.program is not None else DEFAULT_PROGRAMS, excludes=args.exclude,
            notify_only=args.notify_only, key=args.key, test_key=args.test_key, keys_cmake=args.keys_cmake,
            openssl=args.openssl,
        )
    except Refusal as refusal:
        print(f"refused: {len(refusal.problems)} problem(s); nothing was written", file=sys.stderr)
        for problem in refusal.problems:
            print(f"  {problem}", file=sys.stderr)
        return 1
    except (ToolError, OSError) as error:  # OSError: a locked output, an unreadable file
        print(f"error: {error}", file=sys.stderr)
        return 1
    manifest = made.manifest
    counts = {c: sum(1 for f in manifest["files"] if f["class"] == c) for c in CLASS_ORDER}
    print(f"{manifest['target']}: Katana {manifest['version']} ({manifest['tag']}), sequence {manifest['sequence']} ({manifest['published']})")
    if manifest.get("notify_only"):
        print("  notify-only: no files, no payload")
    else:
        total = sum(f["size"] for f in manifest["files"])
        print(f"  {len(manifest['files'])} files ({', '.join(f'{n} {c}' for c, n in counts.items())}), "
              f"{_humanise(total)} installed, zip {_humanise(manifest['payload']['size'])}")
    for path in made.skipped:
        print(f"  skipped {path}: the install record belongs to the updater")
    for path in made.excluded:
        print(f"  excluded {path}: not part of the update")
    print(f"  wrote {made.manifest_path}")
    if made.zip_path:
        print(f"  wrote {made.zip_path}")
    if made.signature_path:
        print(f"  wrote {made.signature_path}")
    else:
        print("  not signed (no --key)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
