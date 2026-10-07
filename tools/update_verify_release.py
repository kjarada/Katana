#!/usr/bin/env python3
"""Independent recheck of a release directory's update assets.

The producer (tools/update_make_manifest.py) makes the manifest, the update zip and the signature. This tool
asks whether what it made is what the engine and the helper will accept, using nothing of the producer:
it does NOT import it, it re-derives the path rules from include/katana/update/paths.hpp, reads the zip
with Python's zipfile and a raw inflate of the signed byte range, and checks the signature with the
openssl command line. A bug in the producer therefore cannot also be in the check (a test holds the
import list to that).

For every Katana-update-<token>.json in the directory:
  - the signature file is one line `ed25519 <keyid> <base64>`, the key id names a trusted key, and the
    signature is a valid Ed25519 signature of the manifest's exact bytes (openssl pkeyutl -verify -rawin),
    the very bytes that are then parsed: the file is read once;
  - the manifest is UTF-8 with LF only, keys sorted at every level, no duplicate key; it has the members of
    manifest.hpp with the right types; product katana, schema 1, channel stable, a MAJOR.MINOR[.PATCH[.BUILD]]
    version, tag v<version>, a known target equal to the file name's, expires 180 days after published;
  - every path passes the rules of paths.hpp, files are sorted by path, unique, no two equal once case is
    folded, no file that is also the folder of another (case folded), at most 10,000 files and 2 GiB;
  - the update zip named by `payload` exists, has the size the manifest says, passes zipfile's CRC test, holds
    exactly the manifest's files (each deflated, fixed timestamps; their data laid out, by offset, in the
    order program, lib, data, seed);
  - for each file: the member read through zipfile hashes to `sha256` and has `size`; `csize` is the zip's
    compressed size and at least 2; `offset` is where the raw deflate data starts, and inflating exactly
    those bytes alone gives the same hash; no two ranges overlap and none leaves the zip;
  - the manifests of one release agree on product, channel, version, tag and sequence, and each target
    appears once; with --previous, (version, sequence) is above the previous release's.
A notify-only manifest (macos-arm64) has no files and no payload and is checked as such.

Trust is explicit: --keys-cmake cmake/KatanaUpdateKeys.cmake (refused while the keys are placeholders, or if a
pinned key is a published test key), --pubkey HEX (repeatable), or --test-keys (RFC 8032 TEST 1 and 2, as the
engine's tests trust). --allow-unsigned lets a directory with no signatures through, for the offline
signer's check before it signs.

Exit status: 0 verified, 1 a problem was found, 2 a usage error.
"""

from __future__ import annotations

import argparse
import base64
import datetime
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile
import zlib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

TOKENS = ("win64", "win-arm64", "linux-x86_64", "linux-aarch64", "macos-arm64")
CLASSES = ("program", "lib", "data", "seed")
ROOTS = ("bin", "share", "etc", "lib")
RESERVED = ("install.json", "installed.json", "installed.manifest.json", "installed.manifest.json.sig")
MAX_FILES = 10_000
MAX_BYTES = 2 * 1024 ** 3
MAX_MANIFEST_BYTES = 8 * 1024 ** 2
EXPIRY_DAYS = 180
SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")
TEST_TRUST = {
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a": "RFC 8032 TEST 1",
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c": "RFC 8032 TEST 2",
}
PUBLISHED_TEST_KEYS = set(TEST_TRUST) | {"fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025"}
KNOWN_MEMBERS = {
    "applier", "channel", "expires", "files", "notes_url", "notify_only", "payload", "product", "published",
    "schema", "sequence", "tag", "target", "version",
}
KNOWN_FILE_MEMBERS = {"class", "csize", "exec", "offset", "path", "sha256", "size"}

_DEVICE = re.compile(r"^(con|prn|aux|nul|com[1-9]|lpt[1-9])$", re.IGNORECASE)
# [0-9], never \d: in a str pattern \d also matches the digits of other scripts, which version.hpp refuses.
_TIME = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z")
_VERSION = re.compile(r"[0-9]{1,9}(\.[0-9]{1,9}){1,3}")


class UsageError(Exception):
    pass


@dataclass
class Report:
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    manifests: dict[str, dict] = field(default_factory=dict)  # token -> parsed manifest

    def error(self, where: str, message: str) -> None:
        self.errors.append(f"{where}: {message}")

    @property
    def ok(self) -> bool:
        return not self.errors


# ---- the path rules, from paths.hpp -------------------------------------------------------------


def path_problems(path: object) -> list[str]:
    if not isinstance(path, str) or path == "":
        return ["not a non-empty string"]
    problems = []
    if any(not 0x20 <= ord(c) <= 0x7E for c in path):
        problems.append("not printable ASCII")
    if any(c in '\\:*?"<>|' for c in path):
        problems.append("has a backslash, colon, or one of * ? \" < > |")
    if path.startswith("/"):
        return problems + ["is absolute"]
    parts = path.split("/")
    if len(parts) < 2 or parts[0] not in ROOTS:
        problems.append("does not start with a folder of bin, share, etc, lib followed by a name")
    if any(p == "" for p in parts):
        problems.append("has an empty component")
    if any(p in (".", "..") for p in parts):
        problems.append("has a '.' or '..' component")
    for p in parts:
        if p not in ("", ".", "..") and p[-1] in ". ":
            problems.append(f"component '{p}' ends in a dot or space")
        if _DEVICE.match(p.split(".", 1)[0]):
            problems.append(f"component '{p}' is a device name")
        if re.search(r"~[0-9]", p):
            problems.append(f"component '{p}' is in 8.3 form")
        if p.lower() == ".update":
            problems.append("has a .update component")
    if parts[-1].lower() in RESERVED:
        problems.append(f"'{parts[-1]}' is a record the updater owns")
    return problems


# ---- parsing ------------------------------------------------------------------------------------


def _checked_object(pairs: list) -> dict:
    keys = [k for k, _ in pairs]
    if len(set(keys)) != len(keys):
        raise ValueError(f"a duplicate key among {keys}")
    if keys != sorted(keys):
        raise ValueError(f"keys are not sorted: {keys}")
    return dict(pairs)


def parse_manifest_bytes(data: bytes) -> dict:
    """The manifest as the contract writes it: UTF-8, LF, sorted keys. ValueError names the first fault."""
    if len(data) > MAX_MANIFEST_BYTES:
        raise ValueError(f"{len(data)} bytes; the engine reads at most {MAX_MANIFEST_BYTES}")
    if b"\r" in data:
        raise ValueError("has a carriage return: line ends must be LF")
    if data.startswith(b"\xef\xbb\xbf"):
        raise ValueError("starts with a byte-order mark")
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"is not UTF-8: {error}") from error

    def refuse(name: str):
        raise ValueError(f"has the constant {name}, which is not JSON")

    value = json.loads(text, object_pairs_hook=_checked_object, parse_constant=refuse)
    if not isinstance(value, dict):
        raise ValueError("is not a JSON object")
    return value


def _is_int(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def version_key(text: str) -> tuple:
    if not _VERSION.fullmatch(text):
        raise ValueError(f"version {text!r} is not MAJOR.MINOR[.PATCH[.BUILD]] of ASCII digits")
    parts = [int(p) for p in text.split(".")]
    return tuple(parts + [0] * (4 - len(parts)))


def parse_signature_line(text: str) -> tuple[str, bytes]:
    match = re.fullmatch(r"ed25519 ([0-9a-f]{16}) ([A-Za-z0-9+/]{86}==)\r?\n?", text)
    if not match:
        raise ValueError("is not exactly one line `ed25519 <16 hex digits> <base64 of 64 bytes>`")
    signature = base64.b64decode(match.group(2), validate=True)
    if len(signature) != 64:
        raise ValueError(f"holds {len(signature)} signature bytes, not 64")
    return match.group(1), signature


# ---- trust and openssl --------------------------------------------------------------------------


def trust_from_cmake(path: Path) -> dict[str, str]:
    if not path.is_file():
        raise UsageError(f"{path} does not exist: the keys to trust cannot be read (--keys-cmake)")
    text = path.read_text(encoding="utf-8")
    flag = re.search(r"set\(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS\s+(ON|OFF)\)", text)
    keys = {
        which: re.search(rf'set\(KATANA_UPDATE_KEY_{which}\s+"([0-9a-f]{{64}})"\)', text)
        for which in ("ACTIVE", "SPARE")
    }
    if not flag or not all(keys.values()):
        raise UsageError(f"{path} does not hold the settings of cmake/KatanaUpdateKeys.cmake")
    if flag.group(1) == "ON":
        raise UsageError(f"{path} holds placeholder keys: nothing can verify against them (tools/update_keytool.py pub)")
    trust = {}
    for which, match in keys.items():
        key = match.group(1)  # type: ignore[union-attr]
        if key in PUBLISHED_TEST_KEYS:
            raise UsageError(f"{path} pins a published test key as {which.lower()}: anyone can sign with it")
        trust[key] = which.lower()
    return trust


def find_openssl(explicit: Optional[str]) -> str:
    found = explicit or os.environ.get("KATANA_OPENSSL") or shutil.which("openssl")
    if not found:
        raise UsageError("openssl was not found: put it on PATH, or pass --openssl or set KATANA_OPENSSL")
    return found


def openssl_verifies(openssl: str, public_key_hex: str, message: bytes, signature: bytes) -> bool:
    """Whether `signature` is a valid Ed25519 signature of exactly `message`, by the openssl command line.

    The message is the bytes the caller read and goes to openssl as a scratch copy of those bytes, so
    that what is checked, what is parsed and what is judged are one and the same, whatever happens to
    the file on disk meanwhile."""
    der = SPKI_PREFIX + bytes.fromhex(public_key_hex)
    pem = "-----BEGIN PUBLIC KEY-----\n" + base64.encodebytes(der).decode("ascii") + "-----END PUBLIC KEY-----\n"
    with tempfile.TemporaryDirectory() as scratch:
        key_file = Path(scratch, "key.pem")
        key_file.write_bytes(pem.encode("ascii"))
        sig_file = Path(scratch, "sig.bin")
        sig_file.write_bytes(signature)
        message_file = Path(scratch, "message")
        message_file.write_bytes(message)
        done = subprocess.run(
            [openssl, "pkeyutl", "-verify", "-pubin", "-inkey", str(key_file), "-rawin",
             "-in", str(message_file), "-sigfile", str(sig_file)],
            capture_output=True, check=False,
        )
    return done.returncode == 0


# ---- one manifest -------------------------------------------------------------------------------


def _check_time(report: Report, where: str, name: str, value: object) -> Optional[datetime.datetime]:
    if not isinstance(value, str) or not _TIME.fullmatch(value):
        report.error(where, f"{name} is not a UTC time like 2026-10-20T08:00:00Z")
        return None
    try:
        return datetime.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=datetime.timezone.utc)
    except ValueError:
        report.error(where, f"{name} is not a real date")
        return None


def check_manifest_fields(report: Report, where: str, token: str, m: dict) -> bool:
    """The members and their types. False when the rest cannot be checked."""
    start = len(report.errors)
    for name in sorted(set(m) - KNOWN_MEMBERS):
        report.warnings.append(f"{where}: unknown member '{name}' (the engine ignores it)")
    for name in sorted(KNOWN_MEMBERS - {"notify_only", "payload"}):
        if name not in m:
            report.error(where, f"the member '{name}' is missing")
    if len(report.errors) > start:
        return False
    if m["schema"] != 1 or not _is_int(m["schema"]):
        report.error(where, f"schema is {m['schema']!r}, not 1")
    if m["product"] != "katana":
        report.error(where, f"product is {m['product']!r}, not 'katana'")
    if m["channel"] != "stable":
        report.error(where, f"channel is {m['channel']!r}, not 'stable'")
    if not _is_int(m["applier"]) or m["applier"] < 1:
        report.error(where, "applier is not an integer of 1 or more")
    if m["target"] != token:
        report.error(where, f"target is {m['target']!r} but the file name says {token}")
    if not isinstance(m["version"], str) or not _VERSION.fullmatch(m["version"]):
        report.error(where, f"version is {m['version']!r}, not MAJOR.MINOR[.PATCH[.BUILD]] of digits")
    elif m["tag"] != "v" + m["version"]:
        report.error(where, f"tag is {m['tag']!r}, not 'v{m['version']}'")
    if not isinstance(m["notes_url"], str) or not m["notes_url"].startswith("https://"):
        report.error(where, "notes_url is not an https URL")
    elif not m["notes_url"].endswith("/releases/tag/" + str(m["tag"])):
        report.error(where, f"notes_url does not end in /releases/tag/{m['tag']}")
    if not _is_int(m["sequence"]) or m["sequence"] <= 0:
        report.error(where, "sequence is not a positive integer")
    published = _check_time(report, where, "published", m["published"])
    expires = _check_time(report, where, "expires", m["expires"])
    if published and expires and expires - published != datetime.timedelta(days=EXPIRY_DAYS):
        report.error(where, f"expires is not published plus {EXPIRY_DAYS} days")
    if published and _is_int(m["sequence"]) and m["sequence"] > 0:
        if int(published.timestamp()) != m["sequence"]:
            report.warnings.append(f"{where}: published and sequence are not the same instant")
    if not isinstance(m["files"], list):
        report.error(where, "files is not a list")
    notify = m.get("notify_only", False)
    if "notify_only" in m and notify is not True:
        report.error(where, "notify_only, when present, is true")
    if token == "macos-arm64" and notify is not True:
        report.error(where, "macos-arm64 must be notify-only: its bundle cannot be patched file by file")
    if notify is True:
        if m["files"] != []:
            report.error(where, "a notify-only manifest has files")
        if "payload" in m:
            report.error(where, "a notify-only manifest has a payload")
    else:
        payload = m.get("payload")
        if not isinstance(payload, dict):
            report.error(where, "there is no payload object")
        else:
            expected = f"Katana-{m['version']}-{token}.update.zip"
            if payload.get("codec") != "deflate":
                report.error(where, f"payload codec is {payload.get('codec')!r}, not 'deflate'")
            if payload.get("name") != expected:
                report.error(where, f"payload name is {payload.get('name')!r}, not {expected}")
            if not _is_int(payload.get("size")) or payload["size"] <= 0:
                report.error(where, "payload size is not a positive integer")
        if isinstance(m["files"], list) and not m["files"]:
            report.error(where, "the manifest lists no files")
    return len(report.errors) == start


def check_file_entries(report: Report, where: str, m: dict) -> bool:
    start = len(report.errors)
    files = m["files"]
    paths = []
    for index, entry in enumerate(files):
        label = f"{where}: files[{index}]"
        if not isinstance(entry, dict):
            report.errors.append(f"{label} is not an object")
            continue
        for name in sorted(set(entry) - KNOWN_FILE_MEMBERS):
            report.warnings.append(f"{label} has unknown member '{name}'")
        for name in sorted(KNOWN_FILE_MEMBERS - {"exec"}):
            if name not in entry:
                report.errors.append(f"{label} lacks '{name}'")
        if any(name not in entry for name in KNOWN_FILE_MEMBERS - {"exec"}):
            continue
        path = entry["path"]
        paths.append(path)
        for problem in path_problems(path):
            report.errors.append(f"{where}: path {path!r} {problem}")
        if entry["class"] not in CLASSES:
            report.errors.append(f"{label} class {entry['class']!r} is not one of {' '.join(CLASSES)}")
        for name in ("size", "csize", "offset"):
            if not _is_int(entry[name]) or entry[name] < 0:
                report.errors.append(f"{label} {name} is not a non-negative integer")
        if _is_int(entry["csize"]) and entry["csize"] < 2:
            report.errors.append(f"{label} csize is {entry['csize']}: a deflate stream is never under 2 bytes")
        if not isinstance(entry["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
            report.errors.append(f"{label} sha256 is not 64 lower-case hex digits")
        if "exec" in entry and not isinstance(entry["exec"], bool):
            report.errors.append(f"{label} exec is not a boolean")
    string_paths = [p for p in paths if isinstance(p, str)]
    if string_paths != sorted(string_paths):
        report.error(where, "files are not sorted by path")
    folded: dict[str, str] = {}
    for path in string_paths:
        if path.lower() in folded:
            report.error(where, f"{path!r} and {folded[path.lower()]!r} are one name once case is folded")
        folded[path.lower()] = path
    for path in string_paths:
        parts = path.lower().split("/")
        for depth in range(1, len(parts)):
            parent = "/".join(parts[:depth])
            if parent in folded:
                report.error(where, f"{path!r} is inside {folded[parent]!r}, which is also listed as a file (case folded)")
                break
    if len(files) > MAX_FILES:
        report.error(where, f"{len(files)} files; at most {MAX_FILES}")
    total = sum(e["size"] for e in files if isinstance(e, dict) and _is_int(e.get("size")))
    if total > MAX_BYTES:
        report.error(where, f"{total} bytes in all; at most {MAX_BYTES}")
    return len(report.errors) == start


# ---- the zip ------------------------------------------------------------------------------------


def check_payload(report: Report, where: str, directory: Path, m: dict) -> None:
    payload = m["payload"]
    zip_path = directory / payload["name"]
    if not zip_path.is_file():
        report.error(where, f"the payload {payload['name']} is not in the directory")
        return
    actual_size = zip_path.stat().st_size
    if actual_size != payload["size"]:
        report.error(where, f"payload size says {payload['size']} but {payload['name']} is {actual_size} bytes")
    try:
        archive = zipfile.ZipFile(zip_path)
    except zipfile.BadZipFile as error:
        report.error(where, f"{payload['name']} is not a zip: {error}")
        return
    with archive, open(zip_path, "rb") as raw:
        bad = archive.testzip()
        if bad is not None:
            report.error(where, f"{payload['name']}: the member {bad} fails its CRC")
        infos = archive.infolist()
        names = [i.filename for i in infos]
        name_set = set(names)
        if len(name_set) != len(names):
            report.error(where, f"{payload['name']} has a member name twice")
        wanted = {e["path"]: e for e in m["files"]}
        for extra in sorted(name_set - set(wanted)):
            report.error(where, f"{payload['name']} holds {extra!r}, which the manifest does not list")
        for missing in sorted(set(wanted) - name_set):
            report.error(where, f"the manifest lists {missing!r}, which {payload['name']} lacks")
        for info in infos:
            if info.compress_type != zipfile.ZIP_DEFLATED:
                report.error(where, f"{info.filename}: not deflated (method {info.compress_type})")
            if info.flag_bits & 0x1:
                report.error(where, f"{info.filename}: encrypted")
            if info.date_time != (1980, 1, 1, 0, 0, 0):
                report.error(where, f"{info.filename}: timestamp {info.date_time} is not the fixed 1980-01-01 00:00:00")
            if info.filename.endswith("/"):
                report.error(where, f"{info.filename}: a directory entry")
        # The class order: program, lib, data, seed, so that the executables are adjacent. What matters is
        # where the data sits, which is the offset (a Range request merges neighbours by it), so the
        # order is read from the offsets, not from the order of the zip's directory.
        by_offset = sorted((e for e in m["files"] if e["class"] in CLASSES), key=lambda e: e["offset"])
        ranks = [CLASSES.index(e["class"]) for e in by_offset]
        if ranks != sorted(ranks):
            report.error(where, f"{payload['name']} is not ordered program, lib, data, seed (by offset)")
        ranges = []
        for entry in m["files"]:
            path = entry["path"]
            if path not in name_set:
                continue
            info = archive.getinfo(path)
            if info.file_size != entry["size"]:
                report.error(where, f"{path}: the zip says {info.file_size} bytes, the manifest {entry['size']}")
            if info.compress_size != entry["csize"]:
                report.error(where, f"{path}: the zip's compressed size is {info.compress_size}, the manifest's csize {entry['csize']}")
            try:
                content: Optional[bytes] = archive.read(info)
            except (zipfile.BadZipFile, zlib.error, EOFError, NotImplementedError, RuntimeError) as error:
                report.error(where, f"{path}: zipfile cannot read the member: {error}")
                content = None
            if content is not None:
                digest = hashlib.sha256(content).hexdigest()
                if digest != entry["sha256"]:
                    report.error(where, f"{path}: the member hashes to {digest}, the manifest says {entry['sha256']}")
            # Where the raw deflate data really starts, from the local header.
            raw.seek(info.header_offset)
            header = raw.read(30)
            if len(header) < 30 or header[:4] != b"PK\x03\x04":
                report.error(where, f"{path}: no local header at the zip's recorded position")
                continue
            name_length, extra_length = struct.unpack("<26xHH", header)
            start = info.header_offset + 30 + name_length + extra_length
            if start != entry["offset"]:
                report.error(where, f"{path}: the data starts at {start}, the manifest's offset is {entry['offset']}")
            if entry["offset"] + entry["csize"] > actual_size:
                report.error(where, f"{path}: offset + csize leaves the zip")
                continue
            ranges.append((entry["offset"], entry["offset"] + entry["csize"], path))
            raw.seek(entry["offset"])
            blob = raw.read(entry["csize"])
            inflater = zlib.decompressobj(-15)
            try:
                inflated = inflater.decompress(blob) + inflater.flush()
            except zlib.error as error:
                report.error(where, f"{path}: the bytes at offset..offset+csize do not inflate: {error}")
                continue
            if not inflater.eof or inflater.unused_data:
                report.error(where, f"{path}: the bytes at offset..offset+csize are not exactly one deflate stream")
            elif len(inflated) != entry["size"] or hashlib.sha256(inflated).hexdigest() != entry["sha256"]:
                report.error(where, f"{path}: inflating exactly offset..offset+csize does not give the signed bytes")
        ranges.sort()
        for (a_start, a_end, a_path), (b_start, _, b_path) in zip(ranges, ranges[1:]):
            if b_start < a_end:
                report.error(where, f"the ranges of {a_path} and {b_path} overlap")


# ---- the whole directory ------------------------------------------------------------------------


def verify_release(
    directory: Path,
    *,
    trust: Optional[dict[str, str]] = None,
    allow_unsigned: bool = False,
    previous: Optional[Path] = None,
    targets: Optional[list[str]] = None,
    openssl: Optional[str] = None,
) -> Report:
    report = Report()
    if trust is None and not allow_unsigned:
        raise UsageError("give trusted keys (--keys-cmake, --pubkey or --test-keys), or --allow-unsigned")
    files = sorted(directory.glob("Katana-update-*.json"))
    if not files:
        report.error(str(directory), "holds no Katana-update-<token>.json")
        return report
    checker = find_openssl(openssl) if trust else ""
    for manifest_path in files:
        match = re.fullmatch(r"Katana-update-(.+)\.json", manifest_path.name)
        token = match.group(1) if match else ""
        where = manifest_path.name
        if token not in TOKENS:
            report.error(where, f"'{token}' is not a target token ({' '.join(TOKENS)})")
            continue
        data = manifest_path.read_bytes()
        sig_path = manifest_path.with_name(manifest_path.name + ".sig")
        if sig_path.exists():
            try:
                key_id, signature = parse_signature_line(sig_path.read_text(encoding="ascii"))
            except (ValueError, UnicodeDecodeError) as error:
                report.error(sig_path.name, f"{error}")
            else:
                if trust is None:
                    report.notes.append(f"{sig_path.name}: well formed, not checked (no trusted keys given)")
                else:
                    candidates = [k for k in trust if k.startswith(key_id)]
                    if not candidates:
                        report.error(sig_path.name, f"signed by key {key_id}, which is not a trusted key")
                    elif openssl_verifies(checker, candidates[0], data, signature):
                        report.notes.append(f"{where}: signature verified by key {key_id} ({trust[candidates[0]]})")
                    else:
                        report.error(sig_path.name, f"is not a valid Ed25519 signature of {where} under key {key_id}")
        elif not allow_unsigned:
            report.error(where, "has no .sig")
        try:
            m = parse_manifest_bytes(data)
        except ValueError as error:
            report.error(where, f"{error}")
            continue
        if not check_manifest_fields(report, where, token, m):
            continue
        report.manifests[token] = m
        if not check_file_entries(report, where, m):
            continue
        if "payload" in m:
            check_payload(report, where, directory, m)
    check_release_consistency(report, targets)
    if previous is not None:
        check_against_previous(report, previous)
    return report


def check_release_consistency(report: Report, targets: Optional[list[str]]) -> None:
    manifests = report.manifests
    for field_name in ("product", "channel", "version", "tag", "sequence"):
        values = {str(m[field_name]) for m in manifests.values()}
        if len(values) > 1:
            report.error("release", f"the manifests disagree on {field_name}: {sorted(values)}")
    if targets is None:
        report.notes.append("the set of targets is not checked (--targets was not given)")
    else:
        missing = sorted(set(targets) - set(manifests))
        extra = sorted(set(manifests) - set(targets))
        if missing:
            report.error("release", f"no manifest for {', '.join(missing)}")
        if extra:
            report.error("release", f"manifests for {', '.join(extra)}, which were not asked for")


def check_against_previous(report: Report, previous: Path) -> None:
    for token, m in report.manifests.items():
        old_path = previous if previous.is_file() else previous / f"Katana-update-{token}.json"
        if not old_path.is_file():
            report.notes.append(f"{token}: no previous manifest at {old_path}")
            continue
        try:
            old = parse_manifest_bytes(old_path.read_bytes())
            old_key = (version_key(old["version"]), old["sequence"])
        except (ValueError, KeyError, TypeError) as error:
            report.error(token, f"the previous manifest {old_path} cannot be read: {error}")
            continue
        new_key = (version_key(m["version"]), m["sequence"])
        if new_key <= old_key:
            report.error(token, f"(version {m['version']}, sequence {m['sequence']}) is not above the previous release's "
                                f"(version {old['version']}, sequence {old['sequence']})")


# ---- the command line ---------------------------------------------------------------------------


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        prog="update_verify_release.py",
        description="Independent recheck of a release directory's update assets.",
        epilog=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("directory", type=Path, help="the folder holding Katana-update-<token>.json(.sig) and the update zips")
    parser.add_argument("--keys-cmake", type=Path, help="trust the keys pinned in this file (cmake/KatanaUpdateKeys.cmake)")
    parser.add_argument("--pubkey", action="append", default=[], metavar="HEX", help="trust this 64-digit Ed25519 public key (repeatable)")
    parser.add_argument("--test-keys", action="store_true", help="trust the RFC 8032 TEST 1 and TEST 2 keys (tests only)")
    parser.add_argument("--allow-unsigned", action="store_true", help="accept a directory with no .sig files (the check before signing)")
    parser.add_argument("--previous", type=Path, help="the previous release's directory (or one manifest): this release must be above it")
    parser.add_argument("--targets", help="comma-separated tokens the release must hold, exactly")
    parser.add_argument("--openssl", help="the openssl executable (default: KATANA_OPENSSL, else PATH)")
    args = parser.parse_args(argv)
    for stream in (sys.stdout, sys.stderr):  # a path with non-ASCII letters must not kill the report
        try:
            stream.reconfigure(errors="backslashreplace")  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            pass
    try:
        trust: Optional[dict[str, str]] = None
        if args.keys_cmake:
            trust = trust_from_cmake(args.keys_cmake)
        if args.test_keys:
            trust = {**(trust or {}), **TEST_TRUST}
        for key in args.pubkey:
            if not re.fullmatch(r"[0-9a-f]{64}", key):
                raise UsageError(f"--pubkey '{key}' is not 64 lower-case hex digits")
            trust = {**(trust or {}), key: "given"}
        report = verify_release(
            args.directory, trust=trust, allow_unsigned=args.allow_unsigned, previous=args.previous,
            targets=args.targets.split(",") if args.targets else None, openssl=args.openssl,
        )
    except UsageError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    for note in report.notes:
        print(f"ok    {note}")
    for token, m in sorted(report.manifests.items()):
        kind = "notify-only" if m.get("notify_only") else f"{len(m['files'])} files"
        print(f"read  {token}: Katana {m['version']}, sequence {m['sequence']}, {kind}")
    for warning in report.warnings:
        print(f"warn  {warning}")
    for error in report.errors:
        print(f"ERROR {error}")
    if report.ok:
        print(f"verified {len(report.manifests)} manifest(s) in {args.directory}")
        return 0
    print(f"FAILED: {len(report.errors)} problem(s) in {args.directory}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
