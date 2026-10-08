"""tools/update_verify_release.py: the independent recheck of a release directory.

A release is made once by the producer, then each test copies it, breaks it in ONE way and expects the
check to name that way. The breakages are made in the test with json, zipfile and struct, never with the
producer's own helpers except where a file has to be written back in the manifest's canonical form.
"""

from __future__ import annotations

import ast
import base64
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import warnings
import zipfile
import zlib
from pathlib import Path
from typing import Callable
from unittest import mock

from support import (
    OPENSSL,
    PATH_CASES,
    REPO,
    RFC_PUBLIC,
    RFC_SEED,
    RFC_VECTOR,
    SEQUENCE_1790000000,
    THROWAWAY_SEED,
    TOOLS,
    ToolTest,
    make_tree,
    needs_openssl,
    noise,
    private_pem,
    run_tool,
)

import update_make_manifest as producer  # noqa: E402  (support puts tools/ on sys.path)
import update_verify_release as verifier  # noqa: E402

TREE = {
    "bin/katana.exe": b"program one\n" * 40,
    "bin/katana_cli.exe": b"program two\n" * 30,
    "bin/libx.dll": b"library x\n" * 50,
    "lib/liby.so.1": b"library y\n" * 25,
    "share/katana/readme.txt": b"hello\n",
    "share/katana/empty.txt": b"",
    "share/katana/noise.bin": noise("verify-test", 600),
    "etc/katana/user.ini": b"; seed\ntheme=light\n",
}
MANIFEST = "Katana-update-win64.json"
ZIP = "Katana-0.9.0-win64.update.zip"
SIBLINGS = {"update_make_manifest", "update_verify_release", "update_release_sign", "update_keytool",
            "update_make_fixtures"}
PRODUCER_SIDE = SIBLINGS - {"update_verify_release"}


@needs_openssl
class Checked(ToolTest):
    """A signed baseline release (RFC TEST 1), copied fresh for each test."""

    baseline: Path

    @classmethod
    def setUpClass(cls) -> None:
        cls._scratch = tempfile.TemporaryDirectory(prefix="katana-verify-baseline-")
        root = Path(cls._scratch.name)
        key = root / "rfc1.pem"
        key.write_text(private_pem(RFC_SEED[1]), newline="\n")
        tree = make_tree(root / "tree", TREE)
        cls.baseline = root / "release"
        done = run_tool("update_make_manifest.py", "--tree", tree, "--out", cls.baseline, "--version", "0.9.0",
                        "--target", "win64", "--sequence", "1790000000", "--seed", "etc/katana/*.ini",
                        "--key", key, "--test-key")
        assert done.returncode == 0, done.stderr

    @classmethod
    def tearDownClass(cls) -> None:
        cls._scratch.cleanup()

    def setUp(self) -> None:
        super().setUp()
        self.release = self.tmp / "release"
        shutil.copytree(self.baseline, self.release)

    def verify(self, *extra: object, directory: Path = None, trust: tuple = ("--test-keys",), env: dict = None):
        return run_tool("update_verify_release.py", directory or self.release, *trust, *extra, env=env)

    def manifest(self, token: str = "win64") -> dict:
        return json.loads((self.release / f"Katana-update-{token}.json").read_bytes())

    def rewrite(self, change: Callable[[dict], None], token: str = "win64") -> None:
        """Change the parsed manifest and write it back canonical (the signature is then stale)."""
        path = self.release / f"Katana-update-{token}.json"
        manifest = json.loads(path.read_bytes())
        change(manifest)
        path.write_bytes(producer.encode_manifest(manifest))

    def broken(self, change: Callable[[dict], None], *phrases: str) -> subprocess.CompletedProcess:
        self.rewrite(change)
        done = self.verify("--allow-unsigned", trust=())
        self.assert_tool_failed(done, *phrases)
        return done


class TheBaseline(Checked):
    def test_a_release_the_producer_just_made_verifies_and_the_report_says_what_was_checked(self) -> None:
        done = self.verify()
        self.assert_tool_ok(done)
        self.assertIn("signature verified by key d75a980182b10ab7", done.stdout)
        self.assertIn("win64: Katana 0.9.0, sequence 1790000000, 8 files", done.stdout)
        self.assertIn("verified 1 manifest(s)", done.stdout)

    def test_the_report_says_when_the_set_of_targets_was_not_checked(self) -> None:
        self.assertIn("the set of targets is not checked", self.verify().stdout)
        done = self.verify("--targets", "win64")
        self.assert_tool_ok(done)
        self.assertNotIn("not checked", done.stdout.replace("not checked (no trusted keys given)", ""))

    def test_a_keys_file_that_does_not_exist_is_a_usage_error(self) -> None:
        done = self.verify(trust=("--keys-cmake", self.tmp / "no-such-keys.cmake"))
        self.assert_tool_failed(done, "does not exist", code=2)

    def test_with_no_trusted_key_and_no_allowance_for_unsigned_it_refuses_to_start(self) -> None:
        done = run_tool("update_verify_release.py", self.release)
        self.assert_tool_failed(done, "trusted keys", code=2)

    def test_the_trust_can_be_given_as_a_public_key(self) -> None:
        self.assert_tool_ok(self.verify(trust=("--pubkey", RFC_PUBLIC[1])))
        self.assert_tool_failed(self.verify(trust=("--pubkey", RFC_PUBLIC[2])), "not a trusted key")
        self.assert_tool_failed(self.verify(trust=("--pubkey", "abc")), "64 lower-case hex", code=2)

    def test_an_empty_directory_is_a_failure_not_a_pass(self) -> None:
        empty = self.tmp / "empty"
        empty.mkdir()
        self.assert_tool_failed(self.verify(directory=empty), "holds no Katana-update")


class TheKeysFile(Checked):
    def keys_file(self, flag: str, active: str, spare: str) -> Path:
        path = self.tmp / "keys.cmake"
        lines = [f"set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS {flag})",
                 f'set(KATANA_UPDATE_KEY_ACTIVE "{active}")', f'set(KATANA_UPDATE_KEY_SPARE "{spare}")']
        path.write_text("\n".join(lines) + "\n", newline="\n")
        return path

    def test_placeholder_keys_verify_nothing_and_say_so(self) -> None:
        done = self.verify(trust=("--keys-cmake", REPO / "cmake" / "KatanaUpdateKeys.cmake"))
        if done.returncode == 2:  # the tree still holds the skeleton's placeholders
            self.assertIn("placeholder", done.stderr)
        else:                      # the owner has pinned keys: this release is signed by neither
            self.assert_tool_failed(done, "not a trusted key")

    def test_a_pinned_published_test_key_is_refused_as_a_trust_root(self) -> None:
        keys = self.keys_file("OFF", RFC_PUBLIC[1], "ab" * 32)
        self.assert_tool_failed(self.verify(trust=("--keys-cmake", keys)), "published test key", code=2)

    def test_the_pinned_keys_are_the_trust_when_the_file_holds_real_ones(self) -> None:
        # A release signed by a key that is not an RFC key, pinned as the active key.
        throwaway = self.tmp / "throwaway.pem"
        throwaway.write_text(private_pem(THROWAWAY_SEED), newline="\n")
        public = producer.public_key_of(OPENSSL, throwaway).hex()
        keys = self.keys_file("OFF", public, "ab" * 32)
        tree = make_tree(self.tmp / "tree", TREE)
        done = run_tool("update_make_manifest.py", "--tree", tree, "--out", self.tmp / "signed", "--version", "0.9.0",
                        "--target", "win64", "--sequence", "1790000000", "--seed", "etc/katana/*.ini",
                        "--key", throwaway, "--keys-cmake", keys)
        self.assert_tool_ok(done)
        self.assert_tool_ok(self.verify(directory=self.tmp / "signed", trust=("--keys-cmake", keys)))
        self.assert_tool_failed(self.verify(trust=("--keys-cmake", keys)), "not a trusted key")


class TheSignature(Checked):
    def sig_path(self) -> Path:
        return self.release / (MANIFEST + ".sig")

    def test_a_missing_signature_fails_unless_unsigned_is_allowed(self) -> None:
        self.sig_path().unlink()
        self.assert_tool_failed(self.verify(), "has no .sig")
        self.assert_tool_ok(self.verify("--allow-unsigned", trust=()))

    def test_a_signature_line_with_crlf_or_no_line_end_is_accepted(self) -> None:
        line = self.sig_path().read_text(encoding="ascii").rstrip("\n")
        for ending in ("\r\n", ""):
            with self.subTest(ending=repr(ending)):
                self.sig_path().write_bytes((line + ending).encode("ascii"))
                self.assert_tool_ok(self.verify())

    def test_malformed_signature_lines_are_refused(self) -> None:
        good = self.sig_path().read_text(encoding="ascii").rstrip("\n")
        algorithm, key_id, body = good.split(" ")
        cases = {
            "another algorithm": f"rsa {key_id} {body}\n",
            "upper-case key id": f"{algorithm} {key_id.upper()} {body}\n",
            "short key id": f"{algorithm} {key_id[:8]} {body}\n",
            "short signature": f"{algorithm} {key_id} {body[:40]}==\n",
            "two lines": good + "\n" + good + "\n",
            "empty": "",
            "extra field": good + " extra\n",
        }
        for name, content in cases.items():
            with self.subTest(case=name):
                self.sig_path().write_bytes(content.encode("ascii"))
                self.assert_tool_failed(self.verify(), MANIFEST + ".sig")

    def test_a_signature_by_another_key_is_named_as_untrusted_or_invalid(self) -> None:
        # TEST 2 is trusted by --test-keys; a signature made by it but claiming TEST 1's id is invalid.
        other = self.tmp / "rfc2.pem"
        other.write_text(private_pem(RFC_SEED[2]), newline="\n")
        signature = producer.sign_file(OPENSSL, other, self.release / MANIFEST)
        claimed = producer.public_key_of(OPENSSL, other)
        self.sig_path().write_bytes(producer.signature_line(claimed, signature).encode("ascii"))
        self.assert_tool_ok(self.verify())  # TEST 2 is trusted too
        self.assert_tool_failed(self.verify(trust=("--pubkey", RFC_PUBLIC[1])), "not a trusted key")
        line = "ed25519 " + RFC_PUBLIC[1][:16] + " " + base64.b64encode(signature).decode()
        self.sig_path().write_bytes((line + "\n").encode("ascii"))
        self.assert_tool_failed(self.verify(), "not a valid Ed25519 signature")

    def test_the_signature_is_checked_over_the_bytes_that_were_read_and_not_over_the_path(self) -> None:
        path = self.release / MANIFEST
        original = path.read_bytes()
        seen, parsed = [], []
        real_verify, real_parse = verifier.openssl_verifies, verifier.parse_manifest_bytes

        def swap_the_file_then_check(openssl, key, message, signature):
            seen.append(message)
            path.write_bytes(b"something else, put there after the read")  # a swap between read and check
            return real_verify(openssl, key, message, signature)

        def note_what_is_parsed(data):
            parsed.append(data)
            return real_parse(data)

        with mock.patch.object(verifier, "openssl_verifies", swap_the_file_then_check), \
                mock.patch.object(verifier, "parse_manifest_bytes", note_what_is_parsed):
            report = verifier.verify_release(self.release, trust={RFC_PUBLIC[1]: "test"}, openssl=OPENSSL)
        self.assertEqual(seen, [original])
        self.assertEqual(len(parsed), 1)
        self.assertIs(seen[0], parsed[0], "the signature is checked over the very bytes that are parsed: one read")
        self.assertTrue(report.ok, report.errors)

    def test_openssl_verifies_a_message_given_as_bytes_against_the_rfc_vector(self) -> None:
        # RFC 8032 section 7.1, TEST 2: the message 0x72 and its signature, public key of TEST 2.
        message = bytes.fromhex("72")
        signature = bytes.fromhex("92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da"
                                  "085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00")
        self.assertTrue(verifier.openssl_verifies(OPENSSL, RFC_PUBLIC[2], message, signature))
        self.assertFalse(verifier.openssl_verifies(OPENSSL, RFC_PUBLIC[2], b"s", signature))
        self.assertFalse(verifier.openssl_verifies(OPENSSL, RFC_PUBLIC[1], message, signature))

    def test_one_flipped_bit_of_the_manifest_breaks_the_signature(self) -> None:
        data = bytearray((self.release / MANIFEST).read_bytes())
        data[10] ^= 0x01
        (self.release / MANIFEST).write_bytes(bytes(data))
        self.assert_tool_failed(self.verify(), "not a valid Ed25519 signature")

    def test_manifests_handed_in_as_bytes_are_judged_as_given_and_not_as_the_file_stands(self) -> None:
        good = (self.release / MANIFEST).read_bytes()
        (self.release / MANIFEST).write_bytes(b"what the file holds a moment later")
        report = verifier.verify_release(self.release, trust={RFC_PUBLIC[1]: "test"}, openssl=OPENSSL,
                                         manifest_bytes={MANIFEST: good})
        self.assertTrue(report.ok, report.errors)
        self.assertEqual(sorted(report.manifests), ["win64"])
        # A manifest the caller did not hand in is read from the directory, as before.
        report = verifier.verify_release(self.release, allow_unsigned=True,
                                         manifest_bytes={"Katana-update-linux-x86_64.json": good})
        self.assertFalse(report.ok)
        self.assertTrue(any("win64.json" in error for error in report.errors), report.errors)

    def base_only_configuration(self) -> Path:
        """An openssl configuration that activates only the base provider: openssl run with it can neither sign
        nor verify, because no signature algorithm is loaded."""
        conf = self.tmp / "base-only.cnf"
        conf.write_text("openssl_conf = openssl_init\n[openssl_init]\nproviders = provider_sect\n"
                        "[provider_sect]\nbase = base_sect\n[base_sect]\nactivate = 1\n", newline="\n")
        return conf

    def test_a_configuration_of_the_machine_cannot_make_a_good_signature_fail_to_verify(self) -> None:
        conf = self.base_only_configuration()
        message = bytes.fromhex("72")                      # RFC 8032 section 7.1, TEST 2
        signature = bytes.fromhex(RFC_VECTOR[2][1])
        with mock.patch.dict(os.environ, {"OPENSSL_CONF": str(conf)}):
            plain = verifier.subprocess.run(
                [OPENSSL, "pkeyutl", "-verify", "-pubin", "-inkey", str(self.write_public(RFC_PUBLIC[2])), "-rawin",
                 "-in", str(self.write_bytes("m", message)), "-sigfile", str(self.write_bytes("s", signature))],
                capture_output=True, check=False)
            self.assertNotEqual(plain.returncode, 0, "the configuration is meant to break a plain openssl run")
            self.assertTrue(verifier.openssl_verifies(OPENSSL, RFC_PUBLIC[2], message, signature))
            self.assertFalse(verifier.openssl_verifies(OPENSSL, RFC_PUBLIC[2], b"s", signature))   # still a real check
        done = self.verify(env={"OPENSSL_CONF": str(conf)})
        self.assert_tool_ok(done)
        self.assertIn("signature verified", done.stdout)

    def write_public(self, public_hex: str) -> Path:
        path = self.tmp / "public.pem"
        der = bytes.fromhex("302a300506032b6570032100") + bytes.fromhex(public_hex)
        path.write_text("-----BEGIN PUBLIC KEY-----\n" + base64.encodebytes(der).decode() + "-----END PUBLIC KEY-----\n",
                        newline="\n")
        return path

    def write_bytes(self, name: str, data: bytes) -> Path:
        path = self.tmp / name
        path.write_bytes(data)
        return path

    def test_the_variables_that_change_what_openssl_loads_are_removed_whatever_their_case(self) -> None:
        with mock.patch.dict(os.environ, {"OPENSSL_CONF": "a", "openssl_modules": "b", "OpenSSL_Engines": "c",
                                          "KATANA_TEST_MARK": "m"}):
            kept = verifier.openssl_environment()
        for name in kept:
            self.assertNotIn(name.upper(), ("OPENSSL_CONF", "OPENSSL_MODULES", "OPENSSL_ENGINES"))
        self.assertEqual(kept["KATANA_TEST_MARK"], "m")


class TheManifestBytes(Checked):
    def rewrite_bytes(self, change: Callable[[bytes], bytes]) -> None:
        path = self.release / MANIFEST
        path.write_bytes(change(path.read_bytes()))

    def check(self, *phrases: str) -> None:
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), *phrases)

    def test_a_carriage_return_is_refused(self) -> None:
        self.rewrite_bytes(lambda b: b.replace(b"\n", b"\r\n"))
        self.check("carriage return")

    def test_a_byte_order_mark_and_bytes_that_are_not_utf8_are_refused(self) -> None:
        self.rewrite_bytes(lambda b: b"\xef\xbb\xbf" + b)
        self.check("byte-order mark")
        self.rewrite_bytes(lambda b: b"\xff\xfe" + b[3:])
        self.check("not UTF-8")

    def test_keys_out_of_order_are_refused(self) -> None:
        self.rewrite_bytes(lambda b: json.dumps(dict(reversed(list(json.loads(b).items())))).encode())
        self.check("not sorted")

    def test_a_repeated_key_is_refused(self) -> None:
        self.rewrite_bytes(lambda b: b.replace(b'"schema":1,', b'"schema":1,\n"schema":1,'))
        self.check("duplicate key")

    def test_a_file_that_is_not_json_or_not_an_object_is_refused(self) -> None:
        self.rewrite_bytes(lambda b: b"{ nope")
        self.check(MANIFEST)
        self.rewrite_bytes(lambda b: b"[1]")
        self.check("not a JSON object")
        self.rewrite_bytes(lambda b: b'{"a":NaN}')
        self.check("not JSON")


class TheManifestFields(Checked):
    def test_a_member_that_is_missing_is_named(self) -> None:
        self.broken(lambda m: m.pop("expires"), "'expires' is missing")

    def test_wrong_constants_are_named(self) -> None:
        self.broken(lambda m: m.update(product="other"), "product")
        self.broken(lambda m: m.update(channel="beta"), "channel")
        self.broken(lambda m: m.update(schema=2), "schema")
        self.broken(lambda m: m.update(applier=0), "applier")

    def test_a_target_that_the_file_name_does_not_say_is_named(self) -> None:
        shutil.move(self.release / MANIFEST, self.release / "Katana-update-linux-x86_64.json")
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "says linux-x86_64")

    def test_an_unknown_token_in_a_file_name_is_refused(self) -> None:
        shutil.copy(self.release / MANIFEST, self.release / "Katana-update-solaris.json")
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "solaris")

    def test_a_tag_that_is_not_v_and_the_version_is_named(self) -> None:
        self.broken(lambda m: m.update(tag="v9.9.9"), "tag is")

    def test_the_notes_url_must_be_an_https_url_of_the_releases_tag_page(self) -> None:
        good = "https://github.com/kjarada/Katana/releases/tag/v0.9.0"
        self.assertEqual(self.manifest()["notes_url"], good)
        self.broken(lambda m: m.update(notes_url=good.replace("https:", "http:")), "notes_url is not an https URL")
        self.rewrite(lambda m: m.update(notes_url=5))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "notes_url is not an https URL")
        self.rewrite(lambda m: m.update(notes_url=good.replace("v0.9.0", "v9.9.9")))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "notes_url does not end in /releases/tag/v0.9.0")
        self.rewrite(lambda m: m.update(notes_url=good + "/"))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "notes_url does not end in")

    def test_versions_that_the_workflow_cannot_write_are_refused(self) -> None:
        # The last is "1.2" in Arabic-Indic digits: a str pattern's \d matches it, version.hpp does not.
        for version in ("1", "1.2.3.4.5", "1.0-beta", "1..2", "x.y", "١.٢"):
            with self.subTest(version=version):
                self.rewrite(lambda m, v=version: m.update(version=v, tag="v" + v))
                self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "version is")

    def test_times_and_the_expiry_rule_are_checked(self) -> None:
        self.broken(lambda m: m.update(expires=m["published"]), "published plus 180 days")
        self.broken(lambda m: m.update(published="yesterday"), "published is not a UTC time")
        self.broken(lambda m: m.update(published="2026-13-45T00:00:00Z"), "not a real date")
        self.broken(lambda m: m.update(published="٢٠٢٦-09-21T14:13:20Z"), "published is not a UTC time")

    def test_a_sequence_that_is_not_a_positive_integer_is_refused(self) -> None:
        for value in ("5", 0, -3, 1.5, True):
            with self.subTest(value=value):
                self.rewrite(lambda m, v=value: m.update(sequence=v))
                self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "sequence is not a positive integer")

    def test_an_unknown_member_is_a_warning_not_a_failure(self) -> None:
        self.rewrite(lambda m: m.update(extra=1))
        done = self.verify("--allow-unsigned", trust=())
        self.assert_tool_ok(done)
        self.assertIn("unknown member 'extra'", done.stdout)

    def test_a_published_time_that_is_not_the_sequence_is_a_warning(self) -> None:
        self.rewrite(lambda m: m.update(published="2026-09-22T14:13:20Z", expires="2027-03-21T14:13:20Z"))
        done = self.verify("--allow-unsigned", trust=())
        self.assert_tool_ok(done)
        self.assertIn("not the same instant", done.stdout)

    def test_the_payload_must_be_the_zip_the_manifest_names(self) -> None:
        self.broken(lambda m: m["payload"].update(size=m["payload"]["size"] + 1), "payload size says")
        self.rewrite(lambda m: m["payload"].update(name="Katana-0.9.0-win64.zip"))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "payload name")
        self.rewrite(lambda m: m["payload"].update(codec="zstd"))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "codec")

    def test_a_notify_only_manifest_may_not_carry_files_or_a_payload(self) -> None:
        done = run_tool("update_make_manifest.py", "--out", self.tmp / "mac", "--version", "0.9.0",
                        "--target", "macos-arm64", "--notify-only", "--sequence", "1790000000")
        self.assert_tool_ok(done)
        mac = self.tmp / "mac"
        self.assert_tool_ok(run_tool("update_verify_release.py", mac, "--allow-unsigned"))
        path = mac / "Katana-update-macos-arm64.json"
        original = json.loads(path.read_bytes())
        for name, change, phrase in (
            ("not notify-only", lambda m: m.pop("notify_only"), "must be notify-only"),
            ("with files", lambda m: m.update(files=self.manifest()["files"][:1]), "has files"),
            ("with a payload", lambda m: m.update(payload=self.manifest()["payload"]), "has a payload"),
        ):
            with self.subTest(case=name):
                changed = json.loads(json.dumps(original))
                change(changed)
                path.write_bytes(producer.encode_manifest(changed))
                self.assert_tool_failed(run_tool("update_verify_release.py", mac, "--allow-unsigned"), phrase)

    def test_a_manifest_that_lists_nothing_to_install_is_refused(self) -> None:
        self.broken(lambda m: m.update(files=[]), "lists no files")


class TheFiles(Checked):
    def first(self, m: dict) -> dict:
        return m["files"][0]

    def test_a_wrong_hash_is_named_with_both_values(self) -> None:
        done = self.broken(lambda m: self.first(m).update(sha256="0" * 64), "the member hashes to")
        self.assertIn("0" * 64, done.stdout)

    def test_wrong_sizes_are_named(self) -> None:
        self.broken(lambda m: self.first(m).update(size=self.first(m)["size"] + 1), "the zip says")
        self.rewrite(lambda m: self.first(m).update(csize=self.first(m)["csize"] + 1))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "compressed size")

    def test_an_offset_that_is_off_by_one_is_named_and_one_past_the_zip_leaves_it(self) -> None:
        self.broken(lambda m: self.first(m).update(offset=self.first(m)["offset"] + 1), "the data starts at")
        self.rewrite(lambda m: self.first(m).update(offset=10 ** 9))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "leaves the zip")

    def test_a_deflate_stream_cannot_be_shorter_than_two_bytes(self) -> None:
        self.broken(lambda m: self.first(m).update(csize=1), "never under 2")

    def test_overlapping_ranges_are_named(self) -> None:
        def overlap(m: dict) -> None:
            a, b = m["files"][0], m["files"][1]
            b["offset"], b["csize"] = a["offset"], a["csize"]
        self.broken(overlap, "overlap")

    def test_a_class_label_that_breaks_the_zip_order_or_is_unknown_is_named(self) -> None:
        self.broken(lambda m: self.first(m).update(**{"class": "data"}), "not ordered program, lib, data, seed")
        self.rewrite(lambda m: self.first(m).update(**{"class": "weird"}))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "class 'weird'")

    def test_entries_that_are_unsorted_or_repeat_a_name_are_named(self) -> None:
        self.broken(lambda m: m["files"].reverse(), "not sorted by path")
        self.rewrite(lambda m: m["files"].insert(0, dict(m["files"][0])))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "case is folded")

    def test_two_names_equal_once_case_is_folded_are_named(self) -> None:
        def add(m: dict) -> None:
            clone = dict(m["files"][0])
            clone["path"] = "BIN/katana.exe"
            m["files"].insert(0, clone)
        self.broken(add, "case is folded")

    def test_a_name_that_is_a_file_and_the_folder_of_another_is_named_even_when_case_differs(self) -> None:
        for parent_spelling in ("bin/katana.exe", "bin/Katana.EXE"):
            with self.subTest(folder=parent_spelling):
                shutil.copyfile(self.baseline / MANIFEST, self.release / MANIFEST)

                def add(m: dict, spelling: str = parent_spelling) -> None:
                    inner = dict(m["files"][1])
                    inner["path"] = spelling + "/inner.dll"
                    m["files"].insert(1, inner)

                self.broken(add, "is also listed as a file", "inner.dll")

    def test_a_zip_whose_data_is_laid_out_out_of_class_order_is_named_by_offset(self) -> None:
        # Written here, from the producer's pieces, in the WRONG physical order: seed, data, lib, program.
        tree = make_tree(self.tmp / "tree", TREE)
        scan = producer.scan_tree(tree)
        files = producer.prepare_files(scan, producer.DEFAULT_PROGRAMS, ["etc/katana/*.ini"])
        backwards = sorted(files, key=lambda f: (-producer.CLASS_ORDER.index(f.file_class), f.path))
        directory = self.tmp / "backwards"
        directory.mkdir()
        zip_path = directory / ZIP
        placements = producer.write_update_zip(zip_path, backwards)
        manifest = producer.build_manifest(
            version="0.9.0", tag="v0.9.0", target="win64", repo=producer.DEFAULT_REPO, channel="stable",
            sequence=1790000000, applier=1, files=[producer.file_entry(f, placements[f.path]) for f in files],
            payload={"codec": "deflate", "name": ZIP, "size": zip_path.stat().st_size}, notify_only=False,
        )
        (directory / MANIFEST).write_bytes(producer.encode_manifest(manifest))
        done = self.verify("--allow-unsigned", directory=directory, trust=())
        self.assert_tool_failed(done, "not ordered program, lib, data, seed (by offset)")
        # Everything else about that release is right: only the order is named.
        self.assertEqual(len([line for line in done.stdout.splitlines() if line.startswith("ERROR")]), 1, done.stdout)

    def test_bad_digits_and_flags_are_named(self) -> None:
        self.broken(lambda m: self.first(m).update(sha256="A" * 64), "sha256 is not 64 lower-case hex")
        self.rewrite(lambda m: self.first(m).update(exec="yes"))
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "exec is not a boolean")

    def test_a_member_the_manifest_does_not_list_and_a_file_the_zip_lacks_are_named(self) -> None:
        with zipfile.ZipFile(self.release / ZIP, "a") as archive:
            archive.writestr("bin/extra.dll", b"surprise")
        done = self.verify("--allow-unsigned", trust=())
        self.assert_tool_failed(done, "'bin/extra.dll', which the manifest does not list")

    def test_a_listed_file_the_zip_lacks_is_named(self) -> None:
        def add(m: dict) -> None:
            ghost = dict(m["files"][0])
            ghost["path"] = "share/zzz-ghost.dll"
            m["files"].append(ghost)
        self.broken(add, "'share/zzz-ghost.dll', which")

    def test_a_missing_or_damaged_zip_is_named(self) -> None:
        data = (self.release / ZIP).read_bytes()
        (self.release / ZIP).write_bytes(data[: len(data) // 2])
        self.assert_tool_failed(self.verify(), "not a zip")
        (self.release / ZIP).unlink()
        self.assert_tool_failed(self.verify(), "is not in the directory")

    def test_a_flipped_bit_inside_a_member_is_caught_three_ways(self) -> None:
        manifest = self.manifest()
        entry = next(f for f in manifest["files"] if f["path"] == "share/katana/noise.bin")
        data = bytearray((self.release / ZIP).read_bytes())
        data[entry["offset"] + entry["csize"] // 2] ^= 0x01
        (self.release / ZIP).write_bytes(bytes(data))
        done = self.verify()
        self.assert_tool_failed(done, "fails its CRC", "zipfile cannot read the member")
        # Whether the damaged stream still inflates (stored blocks do) or no longer does depends on the
        # zlib that made it; either way the range check names the member.
        self.assertTrue("does not give the signed bytes" in done.stdout or "do not inflate" in done.stdout, done.stdout)

    def test_an_encrypted_member_is_named_and_does_not_stop_the_report(self) -> None:
        path = self.release / ZIP
        data = bytearray(path.read_bytes())
        with zipfile.ZipFile(path) as archive:
            first_entry_of_the_directory = archive.start_dir
        self.assertEqual(bytes(data[first_entry_of_the_directory:first_entry_of_the_directory + 4]), b"PK\x01\x02")
        data[first_entry_of_the_directory + 8] |= 0x01          # general purpose flag, bit 0: encrypted
        path.write_bytes(bytes(data))
        done = self.verify("--allow-unsigned", trust=())
        self.assert_tool_failed(done, ": encrypted")
        self.assertNotIn("Traceback", done.stderr)

    def test_a_directory_entry_in_the_zip_is_named(self) -> None:
        with zipfile.ZipFile(self.release / ZIP, "a") as archive:
            archive.writestr(zipfile.ZipInfo("bin/sub/", date_time=(1980, 1, 1, 0, 0, 0)), b"")
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "bin/sub/: a directory entry")

    def test_a_member_name_that_appears_twice_is_named(self) -> None:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")   # zipfile warns about the duplicate it is asked to write
            with zipfile.ZipFile(self.release / ZIP, "a") as archive:
                info = zipfile.ZipInfo("bin/katana.exe", date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, b"a second bin/katana.exe")
        self.assert_tool_failed(self.verify("--allow-unsigned", trust=()), "has a member name twice")

    def test_a_member_that_is_stored_or_timestamped_differently_is_named(self) -> None:
        path = self.release / ZIP
        entries = {}
        with zipfile.ZipFile(path) as archive:
            for info in archive.infolist():
                entries[info.filename] = archive.read(info)
        with zipfile.ZipFile(path, "w", zipfile.ZIP_STORED) as archive:
            for name, data in entries.items():
                info = zipfile.ZipInfo(name, date_time=(2026, 1, 2, 3, 4, 6))
                info.compress_type = zipfile.ZIP_STORED
                archive.writestr(info, data)
        done = self.verify("--allow-unsigned", trust=())
        self.assert_tool_failed(done, "not deflated", "not the fixed 1980-01-01 00:00:00")


class TheReleaseAsAWhole(ToolTest):
    def make(self, out: Path, target: str, sequence: str, version: str = "0.9.0") -> None:
        tree = make_tree(self.tmp / ("tree-" + target), TREE)
        done = run_tool("update_make_manifest.py", "--tree", tree, "--out", out, "--version", version,
                        "--target", target, "--sequence", sequence, "--seed", "etc/katana/*.ini")
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_manifests_of_one_release_must_agree_on_version_and_sequence(self) -> None:
        out = self.tmp / "rel"
        self.make(out, "win64", "1790000000")
        self.make(out, "linux-x86_64", "1790000001")
        done = run_tool("update_verify_release.py", out, "--allow-unsigned")
        self.assert_tool_failed(done, "disagree on sequence")
        self.make(out, "linux-x86_64", "1790000000", version="0.9.1")
        done = run_tool("update_verify_release.py", out, "--allow-unsigned")
        self.assert_tool_failed(done, "disagree on version", "disagree on tag")

    def test_two_targets_with_one_sequence_verify_and_the_target_list_is_checked(self) -> None:
        out = self.tmp / "rel"
        self.make(out, "win64", "1790000000")
        self.make(out, "linux-x86_64", "1790000000")
        self.assert_tool_ok(run_tool("update_verify_release.py", out, "--allow-unsigned",
                                     "--targets", "win64,linux-x86_64"))
        done = run_tool("update_verify_release.py", out, "--allow-unsigned",
                        "--targets", "win64,linux-x86_64,win-arm64")
        self.assert_tool_failed(done, "no manifest for win-arm64")
        done = run_tool("update_verify_release.py", out, "--allow-unsigned", "--targets", "win64")
        self.assert_tool_failed(done, "linux-x86_64, which were not asked for")

    def test_a_release_must_be_above_the_previous_one_in_version_then_sequence(self) -> None:
        old, new = self.tmp / "old", self.tmp / "new"
        self.make(old, "win64", "1790000000", version="0.9.0")
        for version, sequence, ok in (("0.10.0", "1700000000", True),   # a higher version wins whatever the sequence
                                      ("0.9.0", "1790000001", True),    # the same version, a re-run
                                      ("0.9.0", "1790000000", False),   # the same build
                                      ("0.9.0", "1789999999", False),   # an older build
                                      ("0.8.9", "1800000000", False)):  # an older version, however late
            with self.subTest(version=version, sequence=sequence):
                shutil.rmtree(new, ignore_errors=True)
                self.make(new, "win64", sequence, version=version)
                done = run_tool("update_verify_release.py", new, "--allow-unsigned", "--previous", old)
                if ok:
                    self.assert_tool_ok(done)
                else:
                    self.assert_tool_failed(done, "is not above the previous release")

    def rewrite_previous(self, directory: Path, change) -> None:
        path = directory / "Katana-update-win64.json"
        manifest = json.loads(path.read_bytes())
        change(manifest)
        path.write_bytes(producer.encode_manifest(manifest))

    def test_a_previous_manifest_that_cannot_be_ordered_is_named_and_is_not_a_traceback(self) -> None:
        old, new = self.tmp / "old", self.tmp / "new"
        self.make(old, "win64", "1790000000")
        self.make(new, "win64", "1790000001")
        for name, change, phrase in (
            ("a sequence that is text", lambda m: m.update(sequence="1790000000"), "sequence '1790000000' is not a positive integer"),
            ("a sequence of zero", lambda m: m.update(sequence=0), "sequence 0 is not a positive integer"),
            ("a version that is a number", lambda m: m.update(version=9), "version 9 is not a string"),
            ("a version that is not a version", lambda m: m.update(version="nine"), "version 'nine' is not MAJOR.MINOR"),
            ("no sequence at all", lambda m: m.pop("sequence"), "sequence None is not a positive integer"),
        ):
            with self.subTest(case=name):
                shutil.rmtree(old)
                self.make(old, "win64", "1790000000")
                self.rewrite_previous(old, change)
                done = run_tool("update_verify_release.py", new, "--allow-unsigned", "--previous", old)
                self.assert_tool_failed(done, "the previous manifest", "cannot be read", phrase)
                self.assertNotIn("Traceback", done.stderr)
        (old / "Katana-update-win64.json").write_bytes(b"not json")
        self.assert_tool_failed(run_tool("update_verify_release.py", new, "--allow-unsigned", "--previous", old),
                                "cannot be read")

    def test_one_manifest_given_as_previous_is_compared_with_its_own_target_only(self) -> None:
        # The previous win64 release is 0.10.0. The new release (0.9.0 here, so below it) holds win64 and linux:
        # only win64 can be below the previous win64; linux has no previous manifest and is only noted.
        old, new = self.tmp / "old", self.tmp / "new"
        self.make(old, "win64", "1790000000", version="0.10.0")
        self.make(new, "win64", "1790000001", version="0.9.0")
        self.make(new, "linux-x86_64", "1790000001", version="0.9.0")
        done = run_tool("update_verify_release.py", new, "--allow-unsigned", "--previous", old / "Katana-update-win64.json")
        self.assert_tool_failed(done, "win64: (version 0.9.0, sequence 1790000001) is not above the previous release's "
                                      "(version 0.10.0, sequence 1790000000)")
        self.assertEqual(len([line for line in done.stdout.splitlines() if line.startswith("ERROR")]), 1, done.stdout)
        self.assertIn("linux-x86_64: no previous manifest for this target", done.stdout)

    def test_one_manifest_given_as_previous_that_is_for_no_target_here_compares_nothing_and_says_so(self) -> None:
        old, new = self.tmp / "old", self.tmp / "new"
        self.make(old, "win64", "1790000000")
        self.make(new, "linux-x86_64", "1790000001")
        done = run_tool("update_verify_release.py", new, "--allow-unsigned", "--previous", old / "Katana-update-win64.json")
        self.assert_tool_failed(done, "is for another target than any here (linux-x86_64): nothing was compared")

    def test_the_report_says_that_the_previous_manifests_are_not_signature_checked(self) -> None:
        old, new = self.tmp / "old", self.tmp / "new"
        self.make(old, "win64", "1790000000")
        self.make(new, "win64", "1790000001")
        said = "signatures are not checked"
        done = run_tool("update_verify_release.py", new, "--allow-unsigned", "--previous", old)
        self.assert_tool_ok(done)
        self.assertIn(said, done.stdout)
        self.assertNotIn(said, run_tool("update_verify_release.py", new, "--allow-unsigned").stdout)

    def test_a_previous_directory_without_a_manifest_for_the_target_is_noted_and_compares_nothing(self) -> None:
        new, nothing = self.tmp / "new", self.tmp / "nothing"
        self.make(new, "win64", "1790000001")
        nothing.mkdir()
        done = run_tool("update_verify_release.py", new, "--allow-unsigned", "--previous", nothing)
        self.assert_tool_ok(done)
        self.assertIn(f"win64: no previous manifest for this target at {nothing / 'Katana-update-win64.json'}", done.stdout)
        self.assertNotIn("signatures are not checked", done.stdout)


def hand_zip(members: list[tuple[str, bytes, bytes]]) -> tuple[bytes, dict[str, tuple[int, int]]]:
    """A zip written byte by byte (struct), so that a test can store exactly the raw data it wants.

    Each member is (name, the bytes stored as its deflate data, the file's content); the central directory carries
    the CRC of the content and the stored length. Returns the zip and, per name, (offset, length) of the stored data."""
    body, directory, placed = bytearray(), bytearray(), {}
    for name, stored, content in members:
        encoded = name.encode("ascii")
        crc = zlib.crc32(content)
        header_offset = len(body)
        # signature, version 2.0, flags 0, deflate (8), time 0, date 1980-01-01 (0x21), crc, sizes, name length, extra 0
        body += struct.pack("<4sHHHHHIIIHH", b"PK\x03\x04", 20, 0, 8, 0, 0x21, crc, len(stored), len(content), len(encoded), 0)
        body += encoded
        placed[name] = (len(body), len(stored))
        body += stored
        directory += struct.pack("<4sHHHHHHIIIHHHHHII", b"PK\x01\x02", 0x031E, 20, 0, 8, 0, 0x21, crc, len(stored),
                                 len(content), len(encoded), 0, 0, 0, 0, 0o100644 << 16, header_offset)
        directory += encoded
    end = struct.pack("<4sHHHHIIH", b"PK\x05\x06", 0, 0, len(members), len(members), len(directory), len(body), 0)
    return bytes(body + directory + end), placed


class TheRangeOfAMember(ToolTest):
    """The signed range offset..offset+csize must be exactly one deflate stream, however well zipfile reads the member."""

    CONTENT = b"print('hello from a test program')\n" * 20

    def release_with(self, trailing: bytes) -> Path:
        compressor = zlib.compressobj(9, zlib.DEFLATED, -15)
        stream = compressor.compress(self.CONTENT) + compressor.flush()
        archive, placed = hand_zip([("bin/katana.exe", stream + trailing, self.CONTENT)])
        offset, length = placed["bin/katana.exe"]
        self.assertEqual(length, len(stream) + len(trailing))
        directory = self.tmp / ("release-" + str(len(trailing)))
        directory.mkdir()
        (directory / ZIP).write_bytes(archive)
        sequence, published, expires = SEQUENCE_1790000000
        manifest = {
            "applier": 1, "channel": "stable", "expires": expires,
            "files": [{"class": "program", "csize": length, "exec": True, "offset": offset, "path": "bin/katana.exe",
                       "sha256": hashlib.sha256(self.CONTENT).hexdigest(), "size": len(self.CONTENT)}],
            "notes_url": "https://github.com/kjarada/Katana/releases/tag/v0.9.0",
            "payload": {"codec": "deflate", "name": ZIP, "size": len(archive)},
            "product": "katana", "published": published, "schema": 1, "sequence": int(sequence), "tag": "v0.9.0",
            "target": "win64", "version": "0.9.0",
        }
        (directory / MANIFEST).write_bytes(producer.encode_manifest(manifest))
        return directory

    def test_a_zip_written_by_hand_with_exactly_the_stream_verifies(self) -> None:
        # The control: the same hand-made zip and manifest, with nothing after the stream, is a good release.
        self.assert_tool_ok(run_tool("update_verify_release.py", self.release_with(b""), "--allow-unsigned"))

    def test_bytes_after_the_end_of_the_stream_inside_the_signed_range_are_named(self) -> None:
        # zipfile reads this member happily (it stops at the end of the stream, and the CRC is of the content), and
        # the zip's compressed size equals the manifest's csize. Only the check that the range holds exactly one
        # deflate stream notices that two bytes of something else lie inside the signed range.
        directory = self.release_with(b"\x00\x00")
        with zipfile.ZipFile(directory / ZIP) as archive:
            self.assertEqual(archive.read("bin/katana.exe"), self.CONTENT)
            self.assertIsNone(archive.testzip())
        done = run_tool("update_verify_release.py", directory, "--allow-unsigned")
        self.assert_tool_failed(done, "are not exactly one deflate stream")
        self.assertEqual(len([line for line in done.stdout.splitlines() if line.startswith("ERROR")]), 1, done.stdout)


class TheManifestSizeCap(ToolTest):
    CAP = 8 * 1024 * 1024  # kMaxManifestBytes of include/katana/update/feed.hpp

    def test_a_manifest_of_exactly_eight_mebibytes_parses_and_one_byte_more_does_not(self) -> None:
        # '{"a":"' is 6 bytes and '"}' plus a line feed 3, so (CAP - 9) bytes of padding make exactly CAP.
        exactly = b'{"a":"' + b"x" * (self.CAP - 9) + b'"}\n'
        self.assertEqual(len(exactly), self.CAP)
        self.assertEqual(len(verifier.parse_manifest_bytes(exactly)["a"]), self.CAP - 9)
        with self.assertRaises(ValueError) as raised:
            verifier.parse_manifest_bytes(b"x" + exactly)
        self.assertIn("at most 8388608", str(raised.exception))


class TheConsole(Checked):
    def test_a_path_the_console_cannot_show_is_still_reported_and_the_exit_status_stays_one(self) -> None:
        name = "".join(chr(code) for code in (0x65E5, 0x672C))  # two Japanese letters
        self.rewrite(lambda m: m["files"][0].update(path="bin/" + name + ".dll"))
        # A console whose code page is ASCII: the report must not die in the print.
        done = run_tool("update_verify_release.py", self.release, "--allow-unsigned", env={"PYTHONIOENCODING": "ascii"})
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        shown = "".join("%su%04x" % (chr(92), ord(c)) for c in name)  # backslashreplace writes a backslash, u, 4 hex digits
        self.assertIn(shown, done.stdout)
        self.assertIn("FAILED", done.stdout)
        self.assertNotIn("Traceback", done.stderr)


class Independence(ToolTest):
    def imports_of(self, script: str) -> set[str]:
        tree = ast.parse((TOOLS / script).read_text(encoding="utf-8"))
        names: set[str] = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                names.update(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module:
                names.add(node.module.split(".")[0])
        return names

    def test_the_verifier_imports_nothing_of_the_producer_side_and_only_the_standard_library(self) -> None:
        imported = self.imports_of("update_verify_release.py")
        self.assertEqual(imported & SIBLINGS, set())
        self.assertLessEqual(imported - {"__future__"}, set(sys.stdlib_module_names))

    def test_loading_the_verifier_loads_none_of_the_producer_side(self) -> None:
        code = ("import sys; sys.path.insert(0, sys.argv[1]); import update_verify_release; "
                "print(sorted(m for m in sys.modules if m.startswith('update_')))")
        done = subprocess.run([sys.executable, "-B", "-c", code, str(TOOLS)], capture_output=True, text=True, check=True)
        self.assertEqual(done.stdout.strip(), "['update_verify_release']")

    def test_every_tool_uses_only_the_standard_library_and_its_siblings(self) -> None:
        for script in ("update_make_manifest.py", "update_keytool.py", "update_verify_release.py",
                       "update_release_sign.py", "update_make_fixtures.py"):
            with self.subTest(script=script):
                outside = {n for n in self.imports_of(script) if n != "__future__" and n not in sys.stdlib_module_names}
                self.assertLessEqual(outside, SIBLINGS)

    def test_both_implementations_of_the_path_rules_agree_with_the_table_of_paths_hpp(self) -> None:
        for path, acceptable in PATH_CASES:
            with self.subTest(path=path):
                self.assertEqual(verifier.path_problems(path) == [], acceptable, verifier.path_problems(path))
                self.assertEqual(producer.path_violations(path) == [], acceptable)


if __name__ == "__main__":
    unittest.main()
