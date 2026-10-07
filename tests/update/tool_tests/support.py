"""What the release-tool tests share: where things are, a few oracles, a runner for the tools.

The oracles are deliberately NOT the tools' own code. Hashes come from hashlib over the bytes the test wrote;
a zip is read through zipfile and a local header is parsed here with struct; signatures are made and checked
with the openssl command line against the RFC 8032 section 7.1 keys and vectors (values copied from the RFC,
not from any program's output). The tools are run as programs, exactly as the owner and CI run them.
"""

from __future__ import annotations

import base64
import hashlib
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile
import zlib
from pathlib import Path
from typing import Optional

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
TOOLS = REPO / "tools"
FIXTURES = HERE.parent / "fixtures"

sys.path.insert(0, str(TOOLS))
sys.dont_write_bytecode = True

OPENSSL = os.environ.get("KATANA_OPENSSL") or shutil.which("openssl")
CMAKE = os.environ.get("KATANA_CMAKE") or shutil.which("cmake")
needs_openssl = unittest.skipUnless(OPENSSL, "openssl is not on PATH (set KATANA_OPENSSL)")
needs_cmake = unittest.skipUnless(CMAKE, "cmake is not on PATH (set KATANA_CMAKE)")

# RFC 8032, section 7.1. Seeds ("SECRET KEY") and public keys of TEST 1, 2, 3, and the signatures of the
# vectors openssl can sign (it refuses an empty message, so TEST 1's is not used).
RFC_SEED = {
    1: "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
    2: "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
    3: "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
}
RFC_PUBLIC = {
    1: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
    2: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
    3: "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
}
RFC_VECTOR = {
    2: ("72", "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da"
              "085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"),
    3: ("af82", "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac"
                "18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a"),
}
# A key that is NOT an RFC key and belongs to nobody: for the tests of the non-test-key paths. Its seed is a
# hash of a sentence, so it is reproducible and carries no secret.
THROWAWAY_SEED = hashlib.sha256(b"katana update tool tests: a throwaway key that signs nothing real").hexdigest()

# The sequences and times of the fixtures, worked out by hand: 1790000000 s is 20717 days (to 2026-09-21)
# and 51200 s, which is 14:13:20; 1792312800 s is 20744 days (2026-10-18) and 31200 s, 08:40:00.
SEQUENCE_1790000000 = ("1790000000", "2026-09-21T14:13:20Z", "2027-03-20T14:13:20Z")
SEQUENCE_1792312800 = ("1792312800", "2026-10-18T08:40:00Z", "2027-04-16T08:40:00Z")

# The path rules of include/katana/update/paths.hpp, as cases: (path, acceptable). Both Python
# implementations (the producer and the independent verifier) must agree with every line.
PATH_CASES = [
    ("bin/katana.exe", True),
    ("bin/platforms/qwindows.dll", True),
    ("share/katana/readme.txt", True),
    ("etc/ssl/certs/ca-bundle.crt", True),
    ("lib/libx.so.1.2", True),
    ("share/proj/proj.db", True),
    ("bin/console.exe", True),            # CON followed by letters is not CON
    ("bin/com10.dll", True),              # only COM1 to COM9
    ("bin/com.txt", True),
    ("bin/a~b.dll", True),                # a tilde without a digit is not 8.3
    ("share/katana/installed.json.bak", True),
    ("share/katana/install.json.txt", True),
    ("share/.updated/x", True),
    ("bin/.hidden", True),
    ("bin/a b.txt", True),
    ("share/katana/news-0.10.0.txt", True),
    ("", False),
    ("/bin/a", False),                    # absolute
    ("C:/bin/a", False),                  # drive
    ("C:bin/a", False),
    ("bin\\a.dll", False),
    ("bin/a:stream", False),
    ("bin/../etc/x", False),
    ("bin/./a", False),
    ("bin//a", False),
    ("bin/", False),
    ("other/a.txt", False),               # first component
    ("LICENSE", False),
    ("bin", False),
    ("share/caf\u00e9.txt", False),       # non-ASCII
    ("bin/a\tb", False),                  # control character
    ("bin/a\x7fb", False),                # DEL
    ("bin/a*b", False),
    ("bin/a?b", False),
    ('bin/a"b', False),
    ("bin/a<b", False),
    ("bin/a>b", False),
    ("bin/a|b", False),
    ("bin/a.", False),                    # Windows drops a trailing dot
    ("bin/a ", False),
    ("share/dir./x", False),
    ("bin/con", False),                   # device names
    ("bin/NUL.txt", False),
    ("bin/Com1.dll", False),
    ("bin/lpt9", False),
    ("bin/PRN", False),
    ("bin/PROGRA~1.DLL", False),          # 8.3
    ("share/KATANA~12/x", False),
    ("share/katana/install.json", False), # the updater's own records, in any case
    ("share/katana/INSTALL.JSON", False),
    ("share/katana/installed.json", False),
    ("share/katana/installed.manifest.json", False),
    ("share/katana/installed.manifest.json.sig", False),
    ("share/.update/x", False),
    ("share/.UPDATE/x", False),
    ("bin/.update", False),
]


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def noise(label: str, size: int) -> bytes:
    """Incompressible, reproducible bytes: SHA-256 of a label and a counter, concatenated."""
    out = bytearray()
    counter = 0
    while len(out) < size:
        out += hashlib.sha256(f"{label}:{counter}".encode("ascii")).digest()
        counter += 1
    return bytes(out[:size])


def public_pem(public_hex: str) -> str:
    der = bytes.fromhex("302a300506032b6570032100") + bytes.fromhex(public_hex)
    return "-----BEGIN PUBLIC KEY-----\n" + base64.encodebytes(der).decode("ascii") + "-----END PUBLIC KEY-----\n"


def private_pem(seed_hex: str) -> str:
    der = bytes.fromhex("302e020100300506032b657004220420") + bytes.fromhex(seed_hex)
    return "-----BEGIN PRIVATE KEY-----\n" + base64.encodebytes(der).decode("ascii") + "-----END PRIVATE KEY-----\n"


def openssl_verify(public_hex: str, message: bytes, signature: bytes) -> bool:
    """Ed25519 verification by the openssl command line, with a public key built here from its hex."""
    assert OPENSSL
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        (root / "k.pem").write_text(public_pem(public_hex), newline="\n")
        (root / "m").write_bytes(message)
        (root / "s").write_bytes(signature)
        done = subprocess.run(
            [OPENSSL, "pkeyutl", "-verify", "-pubin", "-inkey", str(root / "k.pem"), "-rawin",
             "-in", str(root / "m"), "-sigfile", str(root / "s")],
            capture_output=True, check=False,
        )
    return done.returncode == 0


def raw_member(zip_path: Path, name: str) -> tuple[int, int, bytes]:
    """(offset, csize, the raw deflate bytes) of a member: zipfile's directory, then the local header parsed here."""
    with zipfile.ZipFile(zip_path) as archive:
        info = archive.getinfo(name)
    data = zip_path.read_bytes()
    assert data[info.header_offset:info.header_offset + 4] == b"PK\x03\x04"
    name_length, extra_length = struct.unpack_from("<HH", data, info.header_offset + 26)
    start = info.header_offset + 30 + name_length + extra_length
    return start, info.compress_size, data[start:start + info.compress_size]


def inflate_raw(blob: bytes) -> bytes:
    inflater = zlib.decompressobj(-15)
    out = inflater.decompress(blob) + inflater.flush()
    assert inflater.eof and not inflater.unused_data, "not exactly one deflate stream"
    return out


def make_tree(root: Path, files: dict[str, bytes]) -> Path:
    for relative, data in files.items():
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    return root


def run_tool(script: str, *args: object, stdin: Optional[str] = None, env: Optional[dict] = None,
             cwd: Optional[Path] = None) -> subprocess.CompletedProcess:
    """Run tools/<script> as a program, the way the owner and CI do."""
    environment = {k: v for k, v in os.environ.items() if k != "SOURCE_DATE_EPOCH"}
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    environment["PYTHONIOENCODING"] = "utf-8"
    environment.update(env or {})
    return subprocess.run(
        [sys.executable, "-B", str(TOOLS / script), *map(str, args)],
        input=stdin, capture_output=True, text=True, encoding="utf-8", env=environment,
        cwd=str(cwd) if cwd else None, check=False,
    )


class ToolTest(unittest.TestCase):
    """A test with a scratch folder of its own."""

    def setUp(self) -> None:
        holder = tempfile.TemporaryDirectory(prefix="katana-tool-test-")
        self.addCleanup(holder.cleanup)
        self.tmp = Path(holder.name)

    def write_pem(self, name: str, seed_hex: str) -> Path:
        path = self.tmp / name
        path.write_text(private_pem(seed_hex), newline="\n")
        return path

    def rfc_pem(self, number: int) -> Path:
        return self.write_pem(f"rfc-test-{number}.pem", RFC_SEED[number])

    def assert_tool_failed(self, done: subprocess.CompletedProcess, *phrases: str, code: int = 1) -> None:
        shown = f"\nstdout: {done.stdout}\nstderr: {done.stderr}"
        self.assertEqual(done.returncode, code, shown)
        for phrase in phrases:
            self.assertIn(phrase, done.stdout + done.stderr, shown)

    def assert_tool_ok(self, done: subprocess.CompletedProcess) -> None:
        self.assertEqual(done.returncode, 0, f"\nstdout: {done.stdout}\nstderr: {done.stderr}")
