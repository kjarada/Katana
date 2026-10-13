#!/usr/bin/env python3
"""Regenerate tests/update/fixtures: the install trees, releases and bad variants the update tests consume.

    python tools/update_make_fixtures.py [--out tests/update/fixtures] [--keys <folder of *.seed.hex>]

Everything is made by the real producer (tools/update_make_manifest.py) from trees defined below, signed
with the PUBLISHED RFC 8032 section 7.1 test keys whose seeds are in fixtures/keys/ (TEST 1 active, TEST 2
spare, TEST 3 the key nobody trusts; the same keys as tests/update/test_key.hpp). No real key is created or
read. The private-key PEM openssl needs is built in a temporary folder from the 32-byte seed and removed
again: *.pem is git-ignored, by design.

What it writes under --out (it replaces install/ and release/, and leaves keys/ alone):

  install/v1, install/v2          small install trees of Katana 0.9.0 and 0.10.0 (0.10 > 0.9 as numbers
                                  and not as text, which the version comparison must get right). Between
                                  them: katana.exe, readme.txt and settings.ini change; libcore-1.dll,
                                  katana_cli.exe, empty.txt and random.bin do not; libold-1.dll and
                                  legacy/old.txt go (the second is the last file of share/katana/legacy,
                                  so that folder must go with it); libnew-2.dll, news.txt and help/index.txt
                                  arrive (the third is the first file of share/katana/help, a folder 0.9.0
                                  does not have, so the update must create it). So v1 lists 9 files and v2
                                  10. Each holds share/katana/install.json, which the producer leaves out
                                  of the update.
  release/v1, release/v2          Katana-update-win64.json, .json.sig, Katana-<ver>-win64.update.zip
  release/v2-spare-signed         v2 signed by the spare key (TEST 2): a good release
  release/v2-macos-notify         a signed notify-only manifest for macos-arm64 (no zip)
  release/bad/<name>              each one wrong in one way, each named by what it is:
    flipped-manifest-bit            v2 with one bit of one hash digit flipped; the v2 signature
    wrong-key                       v2 signed by TEST 3, whose key id no build trusts
    wrong-key-claims-active         v2 signed by TEST 3 but naming the active key's id
    swapped-sig                     v2's manifest with v1's (valid, but for another manifest) signature
    tampered-member                 v2 whose zip has one byte of share/katana/random.bin changed
    path-traversal                  a validly signed v2 that lists bin/../../evil.dll
    downgrade-sequence              a validly signed 0.10.0 with a sequence BELOW release/v2's
    downgrade-version               a validly signed 0.9.0 with a sequence ABOVE release/v2's

The sequences are fixed (a real build's UTC seconds, chosen here), so the files do not change from run to
run on one machine. They also fix `published` and `expires` (180 days later): release/v1 expires on
2027-03-20 and release/v2 on 2027-04-16. `expires` is advisory ("the information is old", never a
refusal, manifest.hpp), but a test that reads the REAL clock and treats it as a state would start to
fail on those dates: a test of anything that looks at `expires` injects its own clock. Python's zlib decides the compressed bytes, so another zlib (zlib-ng or zlib) makes a
different zip and so different offsets: the committed files are the reference and the tests compare a
regeneration with them by meaning, not by byte.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Optional

import update_make_manifest as producer

FIXTURES = producer.REPO_ROOT / "tests" / "update" / "fixtures"
TARGET = "win64"
V1_VERSION, V1_SEQUENCE = "0.9.0", 1790000000
V2_VERSION, V2_SEQUENCE = "0.10.0", 1792312800
SEED_GLOBS = ["etc/katana/*.ini"]
PKCS8_ED25519_PREFIX = bytes.fromhex("302e020100300506032b657004220420")


def pem_from_seed(seed_hex: str) -> str:
    """The PKCS#8 PEM of an Ed25519 private key with this 32-byte seed (RFC 8410)."""
    seed = bytes.fromhex(seed_hex)
    if len(seed) != 32:
        raise ValueError("an Ed25519 seed is 32 bytes")
    body = base64.encodebytes(PKCS8_ED25519_PREFIX + seed).decode("ascii")
    return "-----BEGIN PRIVATE KEY-----\n" + body + "-----END PRIVATE KEY-----\n"


def text(*lines: str) -> bytes:
    return ("\n".join(lines) + "\n").encode("ascii")


def noise(label: str, size: int) -> bytes:
    """Incompressible, reproducible bytes: SHA-256 of label and a counter, concatenated."""
    out = bytearray()
    counter = 0
    while len(out) < size:
        out += hashlib.sha256(f"{label}:{counter}".encode("ascii")).digest()
        counter += 1
    return bytes(out[:size])


def _tree_v1() -> dict[str, bytes]:
    return {
        "bin/katana.exe": text("KATANA TEST PROGRAM", "release 0.9.0", *[f"code line {n}" for n in range(12)]),
        "bin/katana_cli.exe": text("KATANA TEST CLI PROGRAM", *[f"code line {n}" for n in range(10)]),
        "bin/libcore-1.dll": text("TEST LIBRARY core", *[f"symbol {n}" for n in range(20)]),
        "bin/libold-1.dll": text("TEST LIBRARY old (gone in 0.10.0)", *[f"symbol {n}" for n in range(8)]),
        "etc/katana/settings.ini": text("; a seed: written only when absent", "units=metric", "theme=light"),
        "share/katana/empty.txt": b"",
        "share/katana/install.json": b'{"schema":1,"target":"win64"}',
        "share/katana/legacy/old.txt": text("The only file of share/katana/legacy.", "0.10.0 removes it, and with it the folder."),
        "share/katana/random.bin": noise("katana-fixture-random", 2048),
        "share/katana/readme.txt": text("Katana test tree 0.9.0", "This file changes in 0.10.0."),
    }


def _tree_v2() -> dict[str, bytes]:
    tree = _tree_v1()
    tree["bin/katana.exe"] = text("KATANA TEST PROGRAM", "release 0.10.0", *[f"code line {n}" for n in range(14)])
    del tree["bin/libold-1.dll"]
    del tree["share/katana/legacy/old.txt"]   # the folder holds nothing else: it goes with its last file
    tree["share/katana/help/index.txt"] = text("The first file of share/katana/help.", "0.9.0 had no such folder: 0.10.0 creates it.")
    tree["bin/libnew-2.dll"] = text("TEST LIBRARY new (arrives in 0.10.0)", *[f"symbol {n}" for n in range(9)])
    tree["etc/katana/settings.ini"] = text("; a seed: written only when absent", "units=metric", "theme=dark")
    tree["share/katana/news.txt"] = text("What is new in 0.10.0", "A file that 0.9.0 did not have.")
    tree["share/katana/readme.txt"] = text("Katana test tree 0.10.0", "This file changed since 0.9.0.")
    return tree


TREES = {"v1": _tree_v1, "v2": _tree_v2}


def write_tree(root: Path, files: dict[str, bytes]) -> None:
    for relative, data in files.items():
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


class Keys:
    """The three test private keys, as PEM files in a folder that is removed when the object is closed."""

    def __init__(self, seeds_folder: Path):
        self._scratch = tempfile.TemporaryDirectory(prefix="katana-test-keys-")
        self.pem: dict[str, Path] = {}
        for name in ("active", "spare", "other"):
            seed = (seeds_folder / f"test-{name}.seed.hex").read_text(encoding="ascii").strip()
            path = Path(self._scratch.name) / f"test-{name}.pem"
            path.write_bytes(pem_from_seed(seed).encode("ascii"))
            self.pem[name] = path

    def close(self) -> None:
        self._scratch.cleanup()


def _make(keys: Keys, tree: Path, out: Path, *, version: str, sequence: int, key: str) -> producer.Produced:
    return producer.produce(
        tree=tree, out=out, version=version, target=TARGET, sequence=sequence, seeds=SEED_GLOBS,
        key=keys.pem[key], test_key=True,
    )


def _resign(keys: Keys, openssl: str, manifest: Path, key: str, claimed_key: Optional[str] = None) -> None:
    """Replace manifest.sig with a signature by `key`, naming `claimed_key`'s id (default: the signer's)."""
    signature = producer.sign_file(openssl, keys.pem[key], manifest)
    named = producer.public_key_of(openssl, keys.pem[claimed_key or key])
    manifest.with_name(manifest.name + ".sig").write_bytes(producer.signature_line(named, signature).encode("ascii"))


def _one_bit_off_a_hex_digit(digit: int) -> int:
    """The same character with one bit flipped that is still a lower-case hex digit."""
    for bit in (0x01, 0x02, 0x04, 0x08):
        flipped = digit ^ bit
        if chr(flipped) in "0123456789abcdef":
            return flipped
    raise ValueError(f"no single-bit change of {chr(digit)!r} is a hex digit")


def generate(out: Path, keys_folder: Path) -> None:
    openssl = producer.find_openssl()
    keys = Keys(keys_folder)
    try:
        for stale in ("install", "release"):
            shutil.rmtree(out / stale, ignore_errors=True)
        for name, build in TREES.items():
            write_tree(out / "install" / name, build())
        install, release = out / "install", out / "release"
        manifest_file = producer.manifest_name(TARGET)
        v2_zip = producer.update_zip_name(V2_VERSION, TARGET)

        _make(keys, install / "v1", release / "v1", version=V1_VERSION, sequence=V1_SEQUENCE, key="active")
        v2 = _make(keys, install / "v2", release / "v2", version=V2_VERSION, sequence=V2_SEQUENCE, key="active")

        _make(keys, install / "v2", release / "v2-spare-signed", version=V2_VERSION, sequence=V2_SEQUENCE, key="spare")
        producer.produce(
            tree=None, out=release / "v2-macos-notify", version=V2_VERSION, target="macos-arm64",
            sequence=V2_SEQUENCE, notify_only=True, key=keys.pem["active"], test_key=True,
        )

        bad = release / "bad"

        def variant(name: str) -> Path:
            folder = bad / name
            shutil.copytree(release / "v2", folder)
            return folder

        # One bit of one hash digit: still a manifest, still the right length, no longer the signed bytes.
        folder = variant("flipped-manifest-bit")
        data = bytearray((folder / manifest_file).read_bytes())
        at = bytes(data).index(b'"sha256":"') + len(b'"sha256":"')
        data[at] = _one_bit_off_a_hex_digit(data[at])
        (folder / manifest_file).write_bytes(bytes(data))

        folder = variant("wrong-key")
        _resign(keys, openssl, folder / manifest_file, "other")

        folder = variant("wrong-key-claims-active")
        _resign(keys, openssl, folder / manifest_file, "other", claimed_key="active")

        folder = variant("swapped-sig")
        shutil.copyfile(release / "v1" / (manifest_file + ".sig"), folder / (manifest_file + ".sig"))

        # One byte inside random.bin's stored data (it does not compress, so the stream stays a valid
        # deflate stream of the same length): the zip's CRC and the signed hash both catch it.
        folder = variant("tampered-member")
        entry = next(f for f in v2.manifest["files"] if f["path"] == "share/katana/random.bin")
        zip_bytes = bytearray((folder / v2_zip).read_bytes())
        zip_bytes[entry["offset"] + entry["csize"] // 2] ^= 0x01
        (folder / v2_zip).write_bytes(bytes(zip_bytes))

        _make_path_traversal(keys, install / "v2", bad / "path-traversal")

        _make(keys, install / "v2", bad / "downgrade-sequence", version=V2_VERSION, sequence=V2_SEQUENCE - 1, key="active")
        _make(keys, install / "v1", bad / "downgrade-version", version=V1_VERSION, sequence=V2_SEQUENCE + 3600, key="active")
    finally:
        keys.close()


def _make_path_traversal(keys: Keys, tree: Path, out: Path) -> None:
    """A release whose signed manifest lists bin/../../evil.dll, made below the producer's rules."""
    scan = producer.scan_tree(tree)
    files = producer.prepare_files(scan, producer.DEFAULT_PROGRAMS, SEED_GLOBS)
    victim = next(f for f in files if f.path == "bin/libnew-2.dll")
    victim.path = "bin/../../evil.dll"
    files.sort(key=lambda f: f.path)
    out.mkdir(parents=True, exist_ok=True)
    zip_path = out / producer.update_zip_name(V2_VERSION, TARGET)
    placements = producer.write_update_zip(zip_path, producer.zip_order(files))
    manifest = producer.build_manifest(
        version=V2_VERSION, tag=f"v{V2_VERSION}", target=TARGET, repo=producer.DEFAULT_REPO, channel="stable",
        sequence=V2_SEQUENCE, applier=1, files=[producer.file_entry(f, placements[f.path]) for f in files],
        payload={"codec": "deflate", "name": zip_path.name, "size": zip_path.stat().st_size}, notify_only=False,
    )
    manifest_path = out / producer.manifest_name(TARGET)
    manifest_path.write_bytes(producer.encode_manifest(manifest))
    _resign(keys, producer.find_openssl(), manifest_path, "active")


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        prog="update_make_fixtures.py",
        description="Regenerate tests/update/fixtures.",
        epilog=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--out", type=Path, default=FIXTURES, help="the fixtures folder (default tests/update/fixtures)")
    parser.add_argument("--keys", type=Path, help="the folder of test-*.seed.hex (default <out>/keys)")
    args = parser.parse_args(argv)
    try:
        generate(args.out, args.keys or args.out / "keys")
    except (producer.ToolError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"wrote the install trees and releases under {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
