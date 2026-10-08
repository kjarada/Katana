"""tests/update/fixtures: the committed trees, releases and bad variants the C++ tests consume.

Every claim about a fixture is checked against an oracle outside the tools that made it: hashlib over the tree
files, Python's zipfile and a local header parsed with struct, the openssl command line under the RFC 8032
public keys, and (for what is wrong with a bad variant) a byte-for-byte comparison with the good one.
"""

from __future__ import annotations

import base64
import hashlib
import json
import re
import shutil
import unittest
import urllib.parse
import zipfile
from pathlib import Path

from support import (
    FIXTURES,
    OPENSSL,
    PATH_CASES,
    REPO,
    RFC_PUBLIC,
    RFC_SEED,
    RFC_VECTOR,
    ToolTest,
    inflate_raw,
    needs_openssl,
    openssl_verify,
    raw_member,
    run_tool,
)

import update_make_fixtures as generator  # noqa: E402  (support puts tools/ on sys.path)
import update_make_manifest as producer  # noqa: E402

RELEASE = FIXTURES / "release"
INSTALL = FIXTURES / "install"
MANIFEST = "Katana-update-win64.json"
SIG = MANIFEST + ".sig"
INSTALL_RECORD = "share/katana/install.json"
V1_ZIP, V2_ZIP = "Katana-0.9.0-win64.update.zip", "Katana-0.10.0-win64.update.zip"

# What differs between the two trees: worked out by hand from the design of the fixtures.
ADDED = ["bin/libnew-2.dll", "share/katana/help/index.txt", "share/katana/news.txt"]
REMOVED = ["bin/libold-1.dll", "share/katana/legacy/old.txt"]
CHANGED = ["bin/katana.exe", "etc/katana/settings.ini", "share/katana/readme.txt"]
UNCHANGED = ["bin/katana_cli.exe", "bin/libcore-1.dll", "share/katana/empty.txt", "share/katana/random.bin"]


def tree_files(root: Path) -> dict[str, bytes]:
    return {p.relative_to(root).as_posix(): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def manifest_of(directory: Path) -> dict:
    return json.loads((directory / MANIFEST).read_bytes())


def signature_of(directory: Path) -> tuple[str, bytes]:
    _, key_id, body = (directory / SIG).read_text(encoding="ascii").split()
    return key_id, base64.b64decode(body, validate=True)


class TheTestKeys(ToolTest):
    def seed_file(self, name: str) -> str:
        return (FIXTURES / "keys" / f"test-{name}.seed.hex").read_text(encoding="ascii").strip()

    def test_the_three_seeds_are_the_published_rfc_8032_ones(self) -> None:
        self.assertEqual([self.seed_file(n) for n in ("active", "spare", "other")],
                         [RFC_SEED[1], RFC_SEED[2], RFC_SEED[3]])

    @needs_openssl
    def test_the_pem_built_from_a_seed_has_the_rfc_public_key_and_makes_the_rfc_signatures(self) -> None:
        for number in (1, 2, 3):
            pem = self.tmp / f"k{number}.pem"
            pem.write_text(generator.pem_from_seed(RFC_SEED[number]), newline="\n")
            self.assertEqual(producer.public_key_of(OPENSSL, pem).hex(), RFC_PUBLIC[number])
        for number, (message_hex, signature_hex) in RFC_VECTOR.items():
            message = self.tmp / f"m{number}"
            message.write_bytes(bytes.fromhex(message_hex))
            pem = self.tmp / f"k{number}.pem"
            self.assertEqual(producer.sign_file(OPENSSL, pem, message).hex(), signature_hex)

    def test_the_python_and_the_cpp_side_use_the_same_keys(self) -> None:
        header = (REPO / "tests" / "update" / "test_key.hpp").read_text(encoding="utf-8")
        found = dict(re.findall(r'(kTest\w+Hex)\s*=\s*"([0-9a-f]{64})"', header))
        self.assertEqual(found["kTestActiveSeedHex"], self.seed_file("active"))
        self.assertEqual(found["kTestSpareSeedHex"], self.seed_file("spare"))
        self.assertEqual(found["kTestActivePublicHex"], RFC_PUBLIC[1])
        self.assertEqual(found["kTestSparePublicHex"], RFC_PUBLIC[2])

    def test_no_private_key_is_committed_in_pem_form_or_any_form(self) -> None:
        self.assertEqual(list(FIXTURES.rglob("*.pem")), [])
        for path in FIXTURES.rglob("*"):
            if path.is_file():
                self.assertNotIn(b"PRIVATE KEY", path.read_bytes(), path.as_posix())

    def test_the_fixtures_are_kept_as_exact_bytes(self) -> None:
        attributes = (FIXTURES / ".gitattributes").read_text(encoding="utf-8")
        self.assertRegex(attributes, r"(?m)^\* -text$")
        for path in list(RELEASE.rglob("*.json")) + list(RELEASE.rglob("*.sig")) + list(FIXTURES.glob("keys/*")):
            self.assertNotIn(b"\r", path.read_bytes(), path.as_posix())
        for relative, data in {**tree_files(INSTALL / "v1"), **tree_files(INSTALL / "v2")}.items():
            if not relative.endswith("random.bin"):
                self.assertNotIn(b"\r", data, relative)


class ThePathRulesTable(ToolTest):
    """fixtures/path_rules.txt: the one table of path cases that every implementation of paths.hpp is held to."""

    TABLE = FIXTURES / "path_rules.txt"

    def lines(self) -> list[list[str]]:
        raw = self.TABLE.read_bytes()
        self.assertNotIn(b"\r", raw)
        self.assertTrue(all(b < 0x80 for b in raw), "the table is plain ASCII")
        return [line.split("\t") for line in raw.decode("ascii").split("\n") if line and not line.startswith("#")]

    @staticmethod
    def escaped(path: str) -> str:
        """The spelling the header of the file defines: %XX per UTF-8 byte outside 0x21..0x7E, and for '%'."""
        return "".join(chr(b) if 0x21 <= b <= 0x7E and b != 0x25 else "%%%02X" % b for b in path.encode("utf-8"))

    def test_every_line_is_a_verdict_a_path_and_a_reason_with_the_path_spelled_the_one_way(self) -> None:
        rows = self.lines()
        self.assertGreaterEqual(len(rows), 50)
        for row in rows:
            with self.subTest(row=row):
                self.assertEqual(len(row), 3)
                verdict, field, reason = row
                self.assertIn(verdict, ("ok", "no"))
                self.assertNotEqual(reason.strip(), "")
                decoded = urllib.parse.unquote(field, encoding="utf-8", errors="strict")
                self.assertEqual(self.escaped(decoded), field)

    def test_the_cases_that_need_an_escape_decode_to_what_they_say(self) -> None:
        cases = dict(PATH_CASES)
        self.assertEqual(len(cases), len(PATH_CASES), "a path is listed once")
        self.assertIs(cases["share/caf" + chr(0xE9) + ".txt"], False)    # a letter outside ASCII
        self.assertIs(cases["bin/a\tb"], False)                           # a control character
        self.assertIs(cases["bin/a" + chr(0x7F) + "b"], False)           # DEL
        self.assertIs(cases["bin/a b.txt"], True)                         # a space inside is allowed
        self.assertIs(cases["bin/a "], False)                             # at the end it is not
        self.assertIs(cases[""], False)                                   # the empty path
        self.assertIs(cases["bin" + chr(0x5C) + "a.dll"], False)          # a backslash
        self.assertIs(cases["bin/katana.exe"], True)
        self.assertEqual({True, False}, set(cases.values()))

    def test_the_header_tells_the_cpp_test_to_read_the_same_file(self) -> None:
        header = [line for line in self.TABLE.read_text(encoding="ascii").split("\n") if line.startswith("#")]
        self.assertTrue(any("test_paths.cpp" in line for line in header))


class TheTrees(ToolTest):
    def test_between_the_trees_three_files_arrive_two_go_three_change_four_stay(self) -> None:
        v1, v2 = tree_files(INSTALL / "v1"), tree_files(INSTALL / "v2")
        for tree in (v1, v2):
            tree.pop(INSTALL_RECORD)
        self.assertEqual(sorted(set(v2) - set(v1)), ADDED)
        self.assertEqual(sorted(set(v1) - set(v2)), REMOVED)
        self.assertEqual(sorted(p for p in set(v1) & set(v2) if v1[p] != v2[p]), CHANGED)
        self.assertEqual(sorted(p for p in set(v1) & set(v2) if v1[p] == v2[p]), UNCHANGED)
        # Counted by hand from the lists above: v1 = 2 removed + 3 changed + 4 unchanged = 9 files, and
        # v2 = 3 added + 3 changed + 4 unchanged = 10 (the install record is not counted).
        self.assertEqual((len(v1), len(v2)), (9, 10))

    @staticmethod
    def folders(files: dict[str, bytes]) -> set[str]:
        """Every folder that holds a file, directly or below: the parents of each path, up to but not including the root."""
        found = set()
        for path in files:
            parts = path.split("/")
            found.update("/".join(parts[:depth]) for depth in range(1, len(parts)))
        return found

    def test_an_update_has_a_folder_to_create_and_a_folder_that_empties(self) -> None:
        v1, v2 = tree_files(INSTALL / "v1"), tree_files(INSTALL / "v2")
        for tree in (v1, v2):
            tree.pop(INSTALL_RECORD)
        # share/katana/help exists only in v2: its first file arrives, so the update must create the folder.
        self.assertEqual(self.folders(v2) - self.folders(v1), {"share/katana/help"})
        # share/katana/legacy exists only in v1 and its only file goes: the folder must go with it. Its parent,
        # share/katana, still holds files and stays.
        self.assertEqual(self.folders(v1) - self.folders(v2), {"share/katana/legacy"})
        self.assertEqual([p for p in v1 if p.startswith("share/katana/legacy/")], ["share/katana/legacy/old.txt"])
        self.assertIn("share/katana", self.folders(v2))

    def test_each_tree_has_the_install_record_the_cpack_install_writes(self) -> None:
        for name in ("v1", "v2"):
            self.assertEqual(json.loads((INSTALL / name / INSTALL_RECORD).read_bytes()), {"schema": 1, "target": "win64"})

    def test_there_is_an_empty_file_and_an_incompressible_one_among_them(self) -> None:
        self.assertEqual((INSTALL / "v2" / "share/katana/empty.txt").read_bytes(), b"")
        self.assertEqual(len((INSTALL / "v2" / "share/katana/random.bin").read_bytes()), 2048)

    def test_ten_is_above_nine_as_a_number_and_below_it_as_text(self) -> None:
        old, new = manifest_of(RELEASE / "v1")["version"], manifest_of(RELEASE / "v2")["version"]
        self.assertEqual((old, new), ("0.9.0", "0.10.0"))
        self.assertGreater(tuple(map(int, new.split("."))), tuple(map(int, old.split("."))))
        self.assertLess(new, old)


class TheGoodReleases(ToolTest):
    def release_checks(self, name: str, version: str, sequence: int) -> None:
        directory = RELEASE / name
        manifest = manifest_of(directory)
        tree = tree_files(INSTALL / ("v1" if version == "0.9.0" else "v2"))
        tree.pop(INSTALL_RECORD)
        self.assertEqual((manifest["version"], manifest["sequence"], manifest["target"]), (version, sequence, "win64"))
        self.assertEqual([f["path"] for f in manifest["files"]], sorted(tree))
        zip_path = directory / f"Katana-{version}-win64.update.zip"
        self.assertEqual(manifest["payload"]["size"], zip_path.stat().st_size)
        with zipfile.ZipFile(zip_path) as archive:
            self.assertIsNone(archive.testzip())
            self.assertEqual(sorted(archive.namelist()), sorted(tree))
        for entry in manifest["files"]:
            content = tree[entry["path"]]
            self.assertEqual(entry["sha256"], hashlib.sha256(content).hexdigest(), entry["path"])
            self.assertEqual(entry["size"], len(content), entry["path"])
            offset, csize, raw = raw_member(zip_path, entry["path"])
            self.assertEqual((entry["offset"], entry["csize"]), (offset, csize), entry["path"])
            self.assertEqual(inflate_raw(raw), content, entry["path"])

    def test_v1_is_what_its_tree_and_its_zip_say(self) -> None:
        self.release_checks("v1", "0.9.0", 1790000000)

    def test_v2_is_what_its_tree_and_its_zip_say(self) -> None:
        self.release_checks("v2", "0.10.0", 1792312800)

    def test_the_classes_and_the_exec_flags_are_those_of_the_stub_tree(self) -> None:
        classes = {f["path"]: (f["class"], f.get("exec", False)) for f in manifest_of(RELEASE / "v2")["files"]}
        self.assertEqual(classes, {
            "bin/katana.exe": ("program", True), "bin/katana_cli.exe": ("program", True),
            "bin/libcore-1.dll": ("lib", False), "bin/libnew-2.dll": ("lib", False),
            "etc/katana/settings.ini": ("seed", False),
            "share/katana/empty.txt": ("data", False), "share/katana/help/index.txt": ("data", False),
            "share/katana/news.txt": ("data", False),
            "share/katana/random.bin": ("data", False), "share/katana/readme.txt": ("data", False),
        })

    def test_the_hashes_of_unchanged_files_are_equal_between_v1_and_v2_and_changed_ones_differ(self) -> None:
        v1 = {f["path"]: f["sha256"] for f in manifest_of(RELEASE / "v1")["files"]}
        v2 = {f["path"]: f["sha256"] for f in manifest_of(RELEASE / "v2")["files"]}
        self.assertEqual(sorted(p for p in v1 if p in v2 and v1[p] == v2[p]), UNCHANGED)
        self.assertEqual(sorted(p for p in v1 if p in v2 and v1[p] != v2[p]), CHANGED)

    @needs_openssl
    def test_v1_and_v2_are_signed_by_the_active_key_and_the_spare_release_by_the_spare(self) -> None:
        for name in ("v1", "v2"):
            key_id, signature = signature_of(RELEASE / name)
            manifest = (RELEASE / name / MANIFEST).read_bytes()
            self.assertEqual(key_id, RFC_PUBLIC[1][:16])
            self.assertTrue(openssl_verify(RFC_PUBLIC[1], manifest, signature), name)
            self.assertFalse(openssl_verify(RFC_PUBLIC[2], manifest, signature), name)
        key_id, signature = signature_of(RELEASE / "v2-spare-signed")
        self.assertEqual((RELEASE / "v2-spare-signed" / MANIFEST).read_bytes(), (RELEASE / "v2" / MANIFEST).read_bytes())
        self.assertEqual(key_id, RFC_PUBLIC[2][:16])
        self.assertTrue(openssl_verify(RFC_PUBLIC[2], (RELEASE / "v2" / MANIFEST).read_bytes(), signature))
        self.assertFalse(openssl_verify(RFC_PUBLIC[1], (RELEASE / "v2" / MANIFEST).read_bytes(), signature))

    @needs_openssl
    def test_the_macos_release_is_a_signed_notify_only_manifest_with_no_zip(self) -> None:
        directory = RELEASE / "v2-macos-notify"
        self.assertEqual(sorted(p.name for p in directory.iterdir()),
                         ["Katana-update-macos-arm64.json", "Katana-update-macos-arm64.json.sig"])
        manifest = json.loads((directory / "Katana-update-macos-arm64.json").read_bytes())
        self.assertEqual((manifest["files"], manifest["notify_only"], manifest["target"]), ([], True, "macos-arm64"))
        self.assertNotIn("payload", manifest)
        _, body = (directory / "Katana-update-macos-arm64.json.sig").read_text().split()[1:]
        self.assertTrue(openssl_verify(RFC_PUBLIC[1], (directory / "Katana-update-macos-arm64.json").read_bytes(),
                                       base64.b64decode(body)))


@needs_openssl
class TheBadVariants(ToolTest):
    """Each one differs from the good v2 in the one way its name says, and the independent check says so."""

    def bad(self, name: str) -> Path:
        return RELEASE / "bad" / name

    def verify(self, name: str, *extra: object):
        return run_tool("update_verify_release.py", self.bad(name), "--test-keys", *extra)

    def test_the_good_ones_pass_the_independent_check(self) -> None:
        for name in ("v1", "v2", "v2-spare-signed", "v2-macos-notify"):
            with self.subTest(release=name):
                self.assert_tool_ok(run_tool("update_verify_release.py", RELEASE / name, "--test-keys"))

    def test_a_flipped_manifest_bit_differs_from_v2_in_exactly_one_bit(self) -> None:
        good = (RELEASE / "v2" / MANIFEST).read_bytes()
        bad = (self.bad("flipped-manifest-bit") / MANIFEST).read_bytes()
        self.assertEqual(len(good), len(bad))
        differing = [(a ^ b) for a, b in zip(good, bad) if a != b]
        self.assertEqual(len(differing), 1)
        self.assertEqual(bin(differing[0]).count("1"), 1)
        json.loads(bad)  # still a manifest: only the signature can tell
        self.assertEqual((self.bad("flipped-manifest-bit") / SIG).read_bytes(), (RELEASE / "v2" / SIG).read_bytes())
        self.assert_tool_failed(self.verify("flipped-manifest-bit"), "not a valid Ed25519 signature")

    def test_the_wrong_key_signs_validly_but_is_not_trusted(self) -> None:
        directory = self.bad("wrong-key")
        key_id, signature = signature_of(directory)
        manifest = (directory / MANIFEST).read_bytes()
        self.assertEqual(manifest, (RELEASE / "v2" / MANIFEST).read_bytes())
        self.assertEqual(key_id, RFC_PUBLIC[3][:16])
        self.assertNotIn(key_id, (RFC_PUBLIC[1][:16], RFC_PUBLIC[2][:16]))
        self.assertTrue(openssl_verify(RFC_PUBLIC[3], manifest, signature))
        self.assert_tool_failed(self.verify("wrong-key"), "not a trusted key")

    def test_the_wrong_key_that_claims_to_be_the_active_one_does_not_verify_under_it(self) -> None:
        directory = self.bad("wrong-key-claims-active")
        key_id, signature = signature_of(directory)
        manifest = (directory / MANIFEST).read_bytes()
        self.assertEqual(key_id, RFC_PUBLIC[1][:16])
        self.assertTrue(openssl_verify(RFC_PUBLIC[3], manifest, signature))
        self.assertFalse(openssl_verify(RFC_PUBLIC[1], manifest, signature))
        self.assert_tool_failed(self.verify("wrong-key-claims-active"), "not a valid Ed25519 signature")

    def test_the_swapped_signature_is_v1s_own_and_valid_for_v1_only(self) -> None:
        directory = self.bad("swapped-sig")
        self.assertEqual((directory / SIG).read_bytes(), (RELEASE / "v1" / SIG).read_bytes())
        self.assertEqual((directory / MANIFEST).read_bytes(), (RELEASE / "v2" / MANIFEST).read_bytes())
        _, signature = signature_of(directory)
        self.assertTrue(openssl_verify(RFC_PUBLIC[1], (RELEASE / "v1" / MANIFEST).read_bytes(), signature))
        self.assertFalse(openssl_verify(RFC_PUBLIC[1], (directory / MANIFEST).read_bytes(), signature))
        self.assert_tool_failed(self.verify("swapped-sig"), "not a valid Ed25519 signature")

    def test_the_tampered_member_is_one_byte_inside_random_bin_with_manifest_and_signature_untouched(self) -> None:
        directory = self.bad("tampered-member")
        self.assertEqual((directory / MANIFEST).read_bytes(), (RELEASE / "v2" / MANIFEST).read_bytes())
        self.assertEqual((directory / SIG).read_bytes(), (RELEASE / "v2" / SIG).read_bytes())
        good, bad = (RELEASE / "v2" / V2_ZIP).read_bytes(), (directory / V2_ZIP).read_bytes()
        self.assertEqual(len(good), len(bad))
        positions = [i for i, (a, b) in enumerate(zip(good, bad)) if a != b]
        self.assertEqual(len(positions), 1)
        entry = next(f for f in manifest_of(directory)["files"] if f["path"] == "share/katana/random.bin")
        self.assertTrue(entry["offset"] <= positions[0] < entry["offset"] + entry["csize"])
        self.assertEqual(bin(good[positions[0]] ^ bad[positions[0]]).count("1"), 1)
        done = self.verify("tampered-member")
        self.assert_tool_failed(done, "random.bin", "fails its CRC")

    def test_the_path_traversal_manifest_is_validly_signed_and_only_its_path_is_refused(self) -> None:
        directory = self.bad("path-traversal")
        manifest = manifest_of(directory)
        self.assertIn("bin/../../evil.dll", [f["path"] for f in manifest["files"]])
        with zipfile.ZipFile(directory / V2_ZIP) as archive:
            self.assertIn("bin/../../evil.dll", archive.namelist())
        key_id, signature = signature_of(directory)
        self.assertEqual(key_id, RFC_PUBLIC[1][:16])
        self.assertTrue(openssl_verify(RFC_PUBLIC[1], (directory / MANIFEST).read_bytes(), signature))
        done = self.verify("path-traversal")
        self.assert_tool_failed(done, "bin/../../evil.dll", "'..'")
        self.assertIn("signature verified", done.stdout)

    def test_the_downgrades_are_validly_signed_and_only_ordering_refuses_them(self) -> None:
        v2 = manifest_of(RELEASE / "v2")
        same_version = manifest_of(self.bad("downgrade-sequence"))
        self.assertEqual((same_version["version"], same_version["sequence"]), (v2["version"], v2["sequence"] - 1))
        older = manifest_of(self.bad("downgrade-version"))
        self.assertEqual(older["version"], "0.9.0")
        self.assertGreater(older["sequence"], v2["sequence"])
        for name in ("downgrade-sequence", "downgrade-version"):
            with self.subTest(release=name):
                self.assert_tool_ok(self.verify(name))
                key_id, signature = signature_of(self.bad(name))
                self.assertTrue(openssl_verify(RFC_PUBLIC[1], (self.bad(name) / MANIFEST).read_bytes(), signature))
                self.assert_tool_failed(self.verify(name, "--previous", RELEASE / "v2"),
                                        "is not above the previous release")


class Regeneration(ToolTest):
    """The generator makes the committed files again, by meaning (a different zlib makes a different zip)."""

    def test_regenerating_gives_the_same_trees_manifests_and_zip_contents(self) -> None:
        if not producer.shutil.which("openssl"):
            self.skipTest("openssl is not on PATH")
        out = self.tmp / "regenerated"
        shutil.copytree(FIXTURES / "keys", out / "keys")
        done = run_tool("update_make_fixtures.py", "--out", out)
        self.assert_tool_ok(done)
        for part in ("install", "release"):
            committed = sorted(p.relative_to(FIXTURES / part).as_posix() for p in (FIXTURES / part).rglob("*") if p.is_file())
            made = sorted(p.relative_to(out / part).as_posix() for p in (out / part).rglob("*") if p.is_file())
            self.assertEqual(made, committed, part)
        self.assertEqual(tree_files(out / "install"), tree_files(FIXTURES / "install"))
        for path in sorted((FIXTURES / "release").rglob("*.json")):
            relative = path.relative_to(FIXTURES / "release")
            with self.subTest(manifest=relative.as_posix()):
                regenerated = json.loads((out / "release" / relative).read_bytes())
                committed = json.loads(path.read_bytes())
                for manifest in (regenerated, committed):
                    if "payload" in manifest:
                        manifest["payload"].pop("size")
                    for entry in manifest["files"]:
                        entry.pop("csize")
                        entry.pop("offset")
                self.assertEqual(regenerated, committed)
        for path in sorted((FIXTURES / "release").rglob("*.zip")):
            relative = path.relative_to(FIXTURES / "release")
            with self.subTest(zip=relative.as_posix()):
                self.assertEqual(self.contents(out / "release" / relative), self.contents(path))
                if (out / "release" / relative).read_bytes() == path.read_bytes():
                    # The very same zlib: then every file must be byte for byte the committed one.
                    for sibling in path.parent.iterdir():
                        self.assertEqual((out / "release" / relative.parent / sibling.name).read_bytes(),
                                         sibling.read_bytes(), sibling.name)

    @staticmethod
    def contents(zip_path: Path) -> list[tuple[str, bytes]]:
        """Every member as (name, inflated bytes), in zip order. The bytes are inflated from the raw deflate
        data and not through zipfile's read, because the tampered variant fails its CRC on purpose."""
        with zipfile.ZipFile(zip_path) as archive:
            names = archive.namelist()
        return [(name, inflate_raw(raw_member(zip_path, name)[2])) for name in names]


if __name__ == "__main__":
    unittest.main()
