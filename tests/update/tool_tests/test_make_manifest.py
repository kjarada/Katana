"""tools/update_make_manifest.py: the manifest, the update zip, the path rules, the signature.

Expectations come from outside the tool: hashlib over the bytes written here, Python's zipfile and a local
header parsed with struct, hand-worked dates, the RFC 8032 vectors, and the openssl command line.
"""

from __future__ import annotations

import base64
import datetime
import json
import os
import re
import time
import unittest
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
    SEQUENCE_1790000000,
    THROWAWAY_SEED,
    ToolTest,
    inflate_raw,
    make_tree,
    needs_openssl,
    noise,
    openssl_verify,
    raw_member,
    run_tool,
    sha256_hex,
)

import update_make_manifest as producer

# A tree with every class in it, as a real package has. The program stub is a .exe; the seed is chosen
# with --seed; share/katana/install.json is what the CPack install writes and the producer must leave out.
TREE = {
    "bin/katana.exe": b"program one\n" * 40,
    "bin/katana_cli.exe": b"program two\n" * 30,
    "bin/libx.dll": b"library x\n" * 50,
    "bin/platforms/qwindows.dll": b"a plugin\n" * 20,
    "lib/liby.so.1": b"library y\n" * 25,
    "share/katana/readme.txt": b"hello\n",
    "share/katana/empty.txt": b"",
    "share/katana/install.json": b'{"schema":1,"target":"win64"}',
    "etc/katana/user.ini": b"; seed\ntheme=light\n",
}
# By hand, from the design: programs first, then libs, then data, then seeds, each by path.
ZIP_ORDER = [
    "bin/katana.exe", "bin/katana_cli.exe",
    "bin/libx.dll", "bin/platforms/qwindows.dll", "lib/liby.so.1",
    "share/katana/empty.txt", "share/katana/readme.txt",
    "etc/katana/user.ini",
]
CLASS_OF = {
    "bin/katana.exe": "program", "bin/katana_cli.exe": "program",
    "bin/libx.dll": "lib", "bin/platforms/qwindows.dll": "lib", "lib/liby.so.1": "lib",
    "share/katana/readme.txt": "data", "share/katana/empty.txt": "data",
    "etc/katana/user.ini": "seed",
}
EMPTY_SHA256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"  # FIPS 180-4, the empty message


class Produced(ToolTest):
    """Base: a tree on disk and a way to run the producer on it."""

    def build(self, files: dict[str, bytes] = TREE, name: str = "tree") -> Path:
        return make_tree(self.tmp / name, files)

    def produce(self, tree: Path, out: Path, *extra: object, version: str = "0.9.0", target: str = "win64",
                sequence: str = "1790000000", **kw):
        arguments = ["--tree", tree, "--out", out, "--version", version, "--target", target,
                     "--seed", "etc/katana/*.ini"]
        if sequence:
            arguments += ["--sequence", sequence]
        return run_tool("update_make_manifest.py", *arguments, *extra, **kw)


class TheManifestAndTheZip(Produced):
    def setUp(self) -> None:
        super().setUp()
        self.tree = self.build()
        self.out = self.tmp / "out"
        self.assert_tool_ok(self.produce(self.tree, self.out))
        self.manifest_bytes = (self.out / "Katana-update-win64.json").read_bytes()
        self.manifest = json.loads(self.manifest_bytes)
        self.zip_path = self.out / "Katana-0.9.0-win64.update.zip"

    def test_it_writes_the_manifest_and_the_zip_and_nothing_else(self) -> None:
        self.assertEqual(sorted(p.name for p in self.out.iterdir()),
                         ["Katana-0.9.0-win64.update.zip", "Katana-update-win64.json"])

    def test_the_members_are_the_install_files_in_program_lib_data_seed_order_and_leave_out_the_install_record(self) -> None:
        with zipfile.ZipFile(self.zip_path) as archive:
            self.assertEqual(archive.namelist(), ZIP_ORDER)
            self.assertIsNone(archive.testzip())
        self.assertEqual([f["path"] for f in self.manifest["files"]], sorted(ZIP_ORDER))

    def test_every_member_is_deflated_alone_with_the_fixed_timestamp(self) -> None:
        with zipfile.ZipFile(self.zip_path) as archive:
            for info in archive.infolist():
                self.assertEqual(info.compress_type, zipfile.ZIP_DEFLATED, info.filename)
                self.assertEqual(info.date_time, (1980, 1, 1, 0, 0, 0), info.filename)

    def test_each_hash_and_size_is_that_of_the_file_that_was_written(self) -> None:
        for entry in self.manifest["files"]:
            content = TREE[entry["path"]]
            self.assertEqual(entry["sha256"], sha256_hex(content), entry["path"])
            self.assertEqual(entry["size"], len(content), entry["path"])

    def test_offset_and_csize_locate_exactly_the_raw_deflate_data_of_the_member(self) -> None:
        for entry in self.manifest["files"]:
            offset, csize, blob = raw_member(self.zip_path, entry["path"])
            self.assertEqual((entry["offset"], entry["csize"]), (offset, csize), entry["path"])
            self.assertGreaterEqual(entry["csize"], 2, entry["path"])
            # The bytes at that range, inflated on their own, are the file.
            raw = self.zip_path.read_bytes()[entry["offset"]:entry["offset"] + entry["csize"]]
            self.assertEqual(raw, blob)
            self.assertEqual(inflate_raw(raw), TREE[entry["path"]], entry["path"])

    def test_an_empty_file_is_deflated_to_a_stream_of_at_least_two_bytes(self) -> None:
        entry = next(f for f in self.manifest["files"] if f["path"] == "share/katana/empty.txt")
        self.assertEqual(entry["size"], 0)
        self.assertEqual(entry["sha256"], EMPTY_SHA256)
        self.assertGreaterEqual(entry["csize"], 2)
        self.assertEqual(inflate_raw(raw_member(self.zip_path, entry["path"])[2]), b"")

    def test_classes_follow_the_name_the_folder_and_the_seed_pattern(self) -> None:
        self.assertEqual({f["path"]: f["class"] for f in self.manifest["files"]}, CLASS_OF)

    def test_only_programs_are_marked_executable_when_no_file_has_the_execute_bit(self) -> None:
        marked = sorted(f["path"] for f in self.manifest["files"] if f.get("exec"))
        self.assertEqual(marked, ["bin/katana.exe", "bin/katana_cli.exe"])
        self.assertTrue(all(f["exec"] is True for f in self.manifest["files"] if "exec" in f))

    def test_the_header_members_are_those_of_the_design(self) -> None:
        m = self.manifest
        sequence, published, expires = SEQUENCE_1790000000
        self.assertEqual(m["applier"], 1)
        self.assertEqual(m["channel"], "stable")
        self.assertEqual(m["product"], "katana")
        self.assertEqual(m["schema"], 1)
        self.assertEqual(m["target"], "win64")
        self.assertEqual(m["version"], "0.9.0")
        self.assertEqual(m["tag"], "v0.9.0")
        self.assertEqual(m["sequence"], int(sequence))
        self.assertEqual(m["published"], published)
        self.assertEqual(m["expires"], expires)  # 180 days later, worked out by hand
        self.assertEqual(m["notes_url"], "https://github.com/kjarada/Katana/releases/tag/v0.9.0")
        self.assertEqual(m["payload"], {"codec": "deflate", "name": "Katana-0.9.0-win64.update.zip",
                                        "size": self.zip_path.stat().st_size})
        self.assertNotIn("notify_only", m)

    def test_expires_is_one_hundred_and_eighty_days_after_published(self) -> None:
        fmt = "%Y-%m-%dT%H:%M:%SZ"
        published = datetime.datetime.strptime(self.manifest["published"], fmt)
        expires = datetime.datetime.strptime(self.manifest["expires"], fmt)
        self.assertEqual(expires - published, datetime.timedelta(days=180))

    def test_the_bytes_are_utf8_lf_with_sorted_keys_and_sorted_files(self) -> None:
        self.assertNotIn(b"\r", self.manifest_bytes)
        self.assertTrue(self.manifest_bytes.endswith(b"\n"))
        self.manifest_bytes.decode("utf-8")
        orders = []

        def note(pairs):
            orders.append([k for k, _ in pairs])
            return dict(pairs)

        json.loads(self.manifest_bytes, object_pairs_hook=note)
        for keys in orders:
            self.assertEqual(keys, sorted(keys))
        paths = [f["path"] for f in self.manifest["files"]]
        self.assertEqual(paths, sorted(paths))
        self.assertEqual(len(paths), len(set(paths)))

    def test_a_compressible_file_is_compressed_and_noise_is_not_made_smaller(self) -> None:
        out = self.tmp / "out2"
        files = {"share/katana/text.txt": b"abcdefgh" * 20000, "share/katana/noise.bin": noise("make-manifest-test", 4096)}
        self.assert_tool_ok(self.produce(self.build(files, "tree2"), out))
        entries = {f["path"]: f for f in json.loads((out / "Katana-update-win64.json").read_bytes())["files"]}
        self.assertLess(entries["share/katana/text.txt"]["csize"], 2000)  # 160 000 bytes of 8-byte period
        self.assertGreaterEqual(entries["share/katana/noise.bin"]["csize"], 4096)

    def test_a_few_hundred_files_have_disjoint_increasing_ranges(self) -> None:
        files = {f"share/katana/many/f{n:03d}.txt": f"file {n}\n".encode() * (n % 7 + 1) for n in range(300)}
        out = self.tmp / "many"
        self.assert_tool_ok(self.produce(self.build(files, "many-tree"), out))
        manifest = json.loads((out / "Katana-update-win64.json").read_bytes())
        by_offset = sorted(manifest["files"], key=lambda f: f["offset"])
        for before, after in zip(by_offset, by_offset[1:]):
            self.assertLessEqual(before["offset"] + before["csize"], after["offset"])
        self.assertEqual(len(manifest["files"]), 300)
        self.assertEqual(by_offset[-1]["offset"] + by_offset[-1]["csize"] <= manifest["payload"]["size"], True)


class Reproducibility(Produced):
    def test_the_same_input_gives_the_same_bytes_wherever_it_is_and_whenever_it_runs(self) -> None:
        tree = self.build()
        first, second = self.tmp / "first", self.tmp / "second"
        self.assert_tool_ok(self.produce(tree, first))
        copy = self.build(TREE, "elsewhere")
        self.assert_tool_ok(self.produce(copy, second))
        for name in ("Katana-update-win64.json", "Katana-0.9.0-win64.update.zip"):
            self.assertEqual((first / name).read_bytes(), (second / name).read_bytes(), name)

    def test_the_sequence_is_the_argument_else_source_date_epoch_else_now(self) -> None:
        tree = self.build()
        explicit = self.tmp / "a"
        self.assert_tool_ok(self.produce(tree, explicit, sequence="1790000000", env={"SOURCE_DATE_EPOCH": "5"}))
        self.assertEqual(json.loads((explicit / "Katana-update-win64.json").read_bytes())["sequence"], 1790000000)
        from_env = self.tmp / "b"
        self.assert_tool_ok(self.produce(tree, from_env, sequence="", env={"SOURCE_DATE_EPOCH": "1792312800"}))
        manifest = json.loads((from_env / "Katana-update-win64.json").read_bytes())
        self.assertEqual((manifest["sequence"], manifest["published"]), (1792312800, "2026-10-18T08:40:00Z"))
        before = int(time.time())
        now = self.tmp / "c"
        self.assert_tool_ok(self.produce(tree, now, sequence=""))
        after = int(time.time())
        got = json.loads((now / "Katana-update-win64.json").read_bytes())["sequence"]
        self.assertTrue(before <= got <= after, (before, got, after))

    def test_a_source_date_epoch_that_is_not_a_number_is_refused(self) -> None:
        done = self.produce(self.build(), self.tmp / "o", sequence="", env={"SOURCE_DATE_EPOCH": "yesterday"})
        self.assert_tool_failed(done, "SOURCE_DATE_EPOCH")


class NotifyOnly(Produced):
    def test_a_notify_only_manifest_has_no_files_no_payload_and_no_zip(self) -> None:
        out = self.tmp / "out"
        done = run_tool("update_make_manifest.py", "--out", out, "--version", "0.9.0", "--target", "macos-arm64",
                        "--notify-only", "--sequence", "1790000000")
        self.assert_tool_ok(done)
        self.assertEqual([p.name for p in out.iterdir()], ["Katana-update-macos-arm64.json"])
        manifest = json.loads((out / "Katana-update-macos-arm64.json").read_bytes())
        self.assertEqual(manifest["files"], [])
        self.assertNotIn("payload", manifest)
        self.assertIs(manifest["notify_only"], True)
        self.assertEqual(manifest["target"], "macos-arm64")

    def test_macos_without_notify_only_is_refused(self) -> None:
        done = self.produce(self.build(), self.tmp / "out", target="macos-arm64")
        self.assert_tool_failed(done, "notify-only")
        self.assertFalse((self.tmp / "out").exists())

    def test_a_notify_only_manifest_takes_no_tree(self) -> None:
        done = self.produce(self.build(), self.tmp / "out", "--notify-only", target="macos-arm64")
        self.assert_tool_failed(done, "no --tree")


class ThePathRules(Produced):
    def test_the_rule_table_of_paths_hpp(self) -> None:
        for path, acceptable in PATH_CASES:
            with self.subTest(path=path):
                self.assertEqual(producer.path_violations(path) == [], acceptable, producer.path_violations(path))

    def test_two_names_equal_once_case_is_folded_are_refused_and_distinct_ones_are_not(self) -> None:
        self.assertEqual(producer.set_violations(["bin/a.dll", "bin/b.dll"], [1, 1]), [])
        found = producer.set_violations(["bin/Lib.dll", "bin/lib.DLL"], [1, 1])
        self.assertEqual(len(found), 1)
        self.assertIn("case", found[0])

    def test_the_caps_are_exactly_ten_thousand_files_and_two_gibibytes(self) -> None:
        paths = [f"share/f{n}" for n in range(10_000)]
        self.assertEqual(producer.set_violations(paths, [1] * 10_000), [])
        self.assertEqual(len(producer.set_violations(paths + ["share/one-more"], [1] * 10_001)), 1)
        two_gib = 2 * 1024 ** 3
        self.assertEqual(producer.set_violations(["share/a", "share/b"], [two_gib - 1, 1]), [])
        self.assertEqual(len(producer.set_violations(["share/a", "share/b"], [two_gib - 1, 2])), 1)

    def test_the_tool_refuses_a_tree_of_ten_thousand_and_one_files_and_writes_nothing(self) -> None:
        files = {f"share/katana/many/f{n:05d}.txt": b"x" for n in range(10_001)}
        out = self.tmp / "too-many-out"
        done = self.produce(self.build(files, "too-many-tree"), out)
        self.assert_tool_failed(done, "10001 files", "at most 10000", "nothing was written")
        self.assertFalse(out.exists())

    def refuses(self, files: dict[str, bytes], *phrases: str, extra: tuple = ()) -> None:
        out = self.tmp / "refused-out"
        done = self.produce(self.build(files, "refused-tree"), out, *extra)
        self.assert_tool_failed(done, "nothing was written", *phrases)
        self.assertFalse(out.exists(), "a refusal must write nothing")

    def test_a_non_ascii_name_is_refused(self) -> None:
        self.refuses({"bin/katana.exe": b"x", "share/café.txt": b"y"}, "caf", "ASCII")

    def test_an_8_3_short_name_is_refused(self) -> None:
        self.refuses({"bin/katana.exe": b"x", "bin/PROGRA~1.DLL": b"y"}, "PROGRA~1.DLL", "8.3")

    def test_a_file_outside_bin_share_etc_and_lib_is_refused_unless_it_is_excluded(self) -> None:
        files = {"bin/katana.exe": b"x", "LICENSE": b"licence text"}
        self.refuses(files, "LICENSE", "first component")
        out = self.tmp / "excluded-out"
        done = self.produce(self.build(files, "excl-tree"), out, "--exclude", "LICENSE")
        self.assert_tool_ok(done)
        self.assertIn("excluded LICENSE", done.stdout)
        paths = [f["path"] for f in json.loads((out / "Katana-update-win64.json").read_bytes())["files"]]
        self.assertEqual(paths, ["bin/katana.exe"])

    def test_the_updaters_own_records_are_refused_anywhere_but_the_cpack_install_record_is_skipped(self) -> None:
        for name in ("bin/install.json", "etc/installed.json", "share/katana/installed.manifest.json",
                     "share/katana/installed.manifest.json.sig", "share/.update/lock"):
            with self.subTest(name=name):
                self.refuses({"bin/katana.exe": b"x", name: b"y"}, name)
        out = self.tmp / "skipped"
        done = self.produce(self.build({"bin/katana.exe": b"x", "share/katana/install.json": b"{}"}, "skip-tree"), out)
        self.assert_tool_ok(done)
        self.assertIn("skipped share/katana/install.json", done.stdout)

    def test_every_violation_is_listed_not_only_the_first(self) -> None:
        out = self.tmp / "many-out"
        done = self.produce(self.build({"bin/katana.exe": b"x", "bin/A~1.DLL": b"y", "LICENSE": b"z", "share/café": b"w"},
                                       "many-bad"), out)
        self.assert_tool_failed(done, "A~1.DLL", "LICENSE", "caf")
        self.assertIn("3 problem", done.stderr + done.stdout)

    def test_a_symbolic_link_is_refused(self) -> None:
        tree = self.build({"bin/katana.exe": b"x", "share/real.txt": b"y"}, "link-tree")
        try:
            os.symlink(tree / "share" / "real.txt", tree / "share" / "link.txt")
        except (OSError, NotImplementedError):
            self.skipTest("this account cannot create symbolic links")
        done = self.produce(tree, self.tmp / "link-out")
        self.assert_tool_failed(done, "link.txt", "symbolic link")

    def test_an_empty_tree_is_refused(self) -> None:
        (self.tmp / "empty-tree").mkdir()
        done = self.produce(self.tmp / "empty-tree", self.tmp / "o")
        self.assert_tool_failed(done, "no files")


class TheArguments(Produced):
    def test_a_version_that_the_workflow_could_not_have_written_is_refused(self) -> None:
        for version in ("1", "1.2.3.4.5", "v1.0", "1.0-beta", "1..2", "1.2.x", ""):
            with self.subTest(version=version):
                done = self.produce(self.build(), self.tmp / "o", version=version)
                self.assertNotEqual(done.returncode, 0)
                self.assertFalse((self.tmp / "o").exists())

    def test_versions_of_two_to_four_numeric_components_are_accepted(self) -> None:
        for version in ("0.9", "0.9.0", "10.20.30.40"):
            with self.subTest(version=version):
                out = self.tmp / ("o" + version)
                self.assert_tool_ok(self.produce(self.build(), out, version=version))
                self.assertEqual(json.loads((out / "Katana-update-win64.json").read_bytes())["tag"], "v" + version)

    def test_an_unknown_target_or_a_zero_sequence_is_refused(self) -> None:
        self.assertEqual(self.produce(self.build(), self.tmp / "o", target="win32").returncode, 2)
        self.assert_tool_failed(self.produce(self.build(), self.tmp / "o2", sequence="0"), "sequence")

    def test_all_five_target_tokens_make_manifests_that_name_themselves(self) -> None:
        for target in ("win64", "win-arm64", "linux-x86_64", "linux-aarch64"):
            with self.subTest(target=target):
                out = self.tmp / target
                self.assert_tool_ok(self.produce(self.build(), out, target=target))
                self.assertEqual(json.loads((out / f"Katana-update-{target}.json").read_bytes())["target"], target)
                self.assertTrue((out / f"Katana-0.9.0-{target}.update.zip").exists())


@needs_openssl
class Signing(Produced):
    def setUp(self) -> None:
        super().setUp()
        self.tree = self.build()

    def sign(self, key: Path, out: Path, *extra: object, **kw):
        return self.produce(self.tree, out, "--key", key, *extra, **kw)

    def test_the_signature_is_one_line_naming_the_key_and_openssl_verifies_it_under_the_rfc_public_key(self) -> None:
        out = self.tmp / "out"
        self.assert_tool_ok(self.sign(self.rfc_pem(1), out, "--test-key"))
        raw = (out / "Katana-update-win64.json.sig").read_bytes()
        line = raw.decode("ascii")
        self.assertTrue(line.endswith("\n") and line.count("\n") == 1 and "\r" not in line)
        match = re.fullmatch(r"ed25519 ([0-9a-f]{16}) ([A-Za-z0-9+/]{86}==)\n", line)
        self.assertIsNotNone(match, line)
        self.assertEqual(match.group(1), RFC_PUBLIC[1][:16])  # "the first 16 hex digits of the key"
        signature = base64.b64decode(match.group(2), validate=True)
        self.assertEqual(len(signature), 64)
        manifest = (out / "Katana-update-win64.json").read_bytes()
        self.assertTrue(openssl_verify(RFC_PUBLIC[1], manifest, signature))
        flipped = bytearray(manifest)
        flipped[len(flipped) // 2] ^= 0x01
        self.assertFalse(openssl_verify(RFC_PUBLIC[1], bytes(flipped), signature))
        self.assertFalse(openssl_verify(RFC_PUBLIC[2], manifest, signature))

    def test_signing_reproduces_the_rfc_8032_signatures_of_tests_2_and_3(self) -> None:
        for number, (message_hex, signature_hex) in RFC_VECTOR.items():
            with self.subTest(test=number):
                message = self.tmp / f"message{number}"
                message.write_bytes(bytes.fromhex(message_hex))
                signature = producer.sign_file(OPENSSL, self.rfc_pem(number), message)
                self.assertEqual(signature.hex(), signature_hex)

    def test_the_key_id_is_the_first_sixteen_hex_digits_of_the_public_key(self) -> None:
        for number in (1, 2, 3):
            public = producer.public_key_of(OPENSSL, self.rfc_pem(number))
            self.assertEqual(public.hex(), RFC_PUBLIC[number])
            self.assertEqual(producer.key_id(public), RFC_PUBLIC[number][:16])

    def test_a_published_test_key_is_refused_for_a_release_and_nothing_is_written(self) -> None:
        out = self.tmp / "out"
        done = self.sign(self.rfc_pem(1), out)
        self.assert_tool_failed(done, "published test key", "--test-key")
        self.assertFalse(out.exists())

    def test_a_test_run_refuses_any_key_that_is_not_a_published_test_key(self) -> None:
        out = self.tmp / "out"
        done = self.sign(self.write_pem("throwaway.pem", THROWAWAY_SEED), out, "--test-key")
        self.assert_tool_failed(done, "not one")
        self.assertFalse(out.exists())

    def test_a_test_run_refuses_a_test_key_that_the_tree_pins_as_a_production_key(self) -> None:
        pinned = self.pinning("pins-a-test-key.cmake", "OFF", RFC_PUBLIC[2], "ab" * 32)
        done = self.sign(self.rfc_pem(2), self.tmp / "out", "--test-key", "--keys-cmake", pinned)
        self.assert_tool_failed(done, "production key")

    def pinning(self, name: str, flag: str, active: str, spare: str) -> Path:
        path = self.tmp / name
        lines = [
            f"set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS {flag})",
            f'set(KATANA_UPDATE_KEY_ACTIVE "{active}")',
            f'set(KATANA_UPDATE_KEY_SPARE "{spare}")',
        ]
        path.write_text("\n".join(lines) + "\n", newline="\n")
        return path

    def test_a_release_key_must_be_pinned_and_placeholders_pin_nothing(self) -> None:
        throwaway = self.write_pem("throwaway.pem", THROWAWAY_SEED)
        public = producer.public_key_of(OPENSSL, throwaway).hex()
        placeholders = self.pinning("placeholders.cmake", "ON", "50" * 32, "51" * 32)
        self.assert_tool_failed(self.sign(throwaway, self.tmp / "o1", "--keys-cmake", placeholders), "placeholder")
        elsewhere = self.pinning("elsewhere.cmake", "OFF", RFC_PUBLIC[1], RFC_PUBLIC[2])
        self.assert_tool_failed(self.sign(throwaway, self.tmp / "o2", "--keys-cmake", elsewhere), "not pinned")
        # The tree's own file, whatever it holds today (placeholders now, the owner's keys later): this
        # key is not one it pins.
        self.assertNotEqual(self.sign(throwaway, self.tmp / "o4", "--keys-cmake", REPO / "cmake" / "KatanaUpdateKeys.cmake").returncode, 0)
        pinned = self.pinning("pinned.cmake", "OFF", public, "ab" * 32)
        out = self.tmp / "o3"
        self.assert_tool_ok(self.sign(throwaway, out, "--keys-cmake", pinned))
        signature = base64.b64decode((out / "Katana-update-win64.json.sig").read_text().split()[2])
        self.assertTrue(openssl_verify(public, (out / "Katana-update-win64.json").read_bytes(), signature))

    def test_a_key_that_openssl_cannot_read_is_refused_without_writing_anything(self) -> None:
        bad = self.tmp / "bad.pem"
        bad.write_text("this is not a key\n")
        out = self.tmp / "out"
        self.assert_tool_failed(self.sign(bad, out, "--test-key"), "not a private key")
        self.assertFalse(out.exists())

    def test_an_unsigned_run_removes_a_stale_signature_of_the_manifest_it_replaces(self) -> None:
        out = self.tmp / "out"
        self.assert_tool_ok(self.sign(self.rfc_pem(1), out, "--test-key"))
        self.assertTrue((out / "Katana-update-win64.json.sig").exists())
        self.assert_tool_ok(self.produce(self.tree, out))
        self.assertFalse((out / "Katana-update-win64.json.sig").exists())

    def test_the_private_key_is_never_copied_into_the_output(self) -> None:
        out = self.tmp / "out"
        key = self.rfc_pem(1)
        done = self.sign(key, out, "--test-key")
        self.assert_tool_ok(done)
        for path in out.iterdir():
            self.assertNotIn(b"PRIVATE", path.read_bytes())
            self.assertNotIn(bytes.fromhex(RFC_SEED[1]), path.read_bytes())
        self.assertNotIn("PRIVATE", done.stdout + done.stderr)


class FixtureTrees(Produced):
    def test_the_producer_reproduces_the_committed_fixture_manifests_by_meaning(self) -> None:
        out = self.tmp / "out"
        done = run_tool("update_make_manifest.py", "--tree", FIXTURES / "install" / "v2", "--out", out,
                        "--version", "0.10.0", "--target", "win64", "--sequence", "1792312800",
                        "--seed", "etc/katana/*.ini")
        self.assert_tool_ok(done)
        made = json.loads((out / "Katana-update-win64.json").read_bytes())
        committed = json.loads((FIXTURES / "release" / "v2" / "Katana-update-win64.json").read_bytes())
        for manifest in (made, committed):
            manifest["payload"].pop("size")
            for entry in manifest["files"]:
                entry.pop("csize")
                entry.pop("offset")
        self.assertEqual(made, committed)


if __name__ == "__main__":
    unittest.main()
