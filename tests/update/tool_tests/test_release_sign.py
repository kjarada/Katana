"""tools/update_release_sign.py: the offline signing flow, on drafts made from the committed fixtures.

A draft is a release directory without .sig files. The expectations about WHAT CHANGED are worked out here
by hashing the fixtures' install trees with hashlib; the signature is judged by the openssl command line under
the RFC 8032 public key; and the printed `gh` commands are parsed back into arguments. A fake `gh` is first on
PATH, and must never run.
"""

from __future__ import annotations

import ast
import base64
import hashlib
import os
import shlex
import shutil
import stat
import unittest
import zipfile
from pathlib import Path

from support import (
    FIXTURES,
    OPENSSL,
    RFC_PUBLIC,
    THROWAWAY_SEED,
    TOOLS,
    ToolTest,
    make_tree,
    needs_openssl,
    openssl_verify,
    run_tool,
)

import update_make_manifest as producer  # noqa: E402  (support puts tools/ on sys.path)

MANIFEST = "Katana-update-win64.json"
SIG = MANIFEST + ".sig"
INSTALL_RECORD = "share/katana/install.json"


def hash_tree(root: Path) -> dict[str, str]:
    """relative path -> SHA-256, over an install tree, leaving out the record the producer leaves out."""
    return {
        path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(root.rglob("*")) if path.is_file()
        and path.relative_to(root).as_posix() != INSTALL_RECORD
    }


@needs_openssl
class Signing(ToolTest):
    def setUp(self) -> None:
        super().setUp()
        self.draft = self.tmp / "draft"
        self.previous = self.tmp / "previous"
        shutil.copytree(FIXTURES / "release" / "v2", self.draft)
        shutil.copytree(FIXTURES / "release" / "v1", self.previous)
        (self.draft / SIG).unlink()  # a draft has no signature
        self.key = self.rfc_pem(1)
        # A gh that records that it ran. The tool must never run it.
        self.fake = self.tmp / "fakebin"
        self.fake.mkdir()
        self.ran = self.tmp / "gh-ran.txt"
        (self.fake / "gh").write_text(f'#!/bin/sh\necho "$@" >> "{self.ran.as_posix()}"\n', newline="\n")
        (self.fake / "gh").chmod((self.fake / "gh").stat().st_mode | stat.S_IXUSR)
        (self.fake / "gh.cmd").write_text(f'@echo %* >> "{self.ran}"\r\n', newline="")
        self.environment = {"PATH": str(self.fake) + os.pathsep + os.environ["PATH"]}

    def sign(self, *extra: object, answer: str = "0.10.0\n", draft: Path = None, key: Path = None, tag: str = "v0.10.0"):
        return run_tool("update_release_sign.py", "--dir", draft or self.draft, "--tag", tag, "--key", key or self.key,
                        "--previous", self.previous, *extra, stdin=answer, env=self.environment)

    def assert_nothing_signed(self, directory: Path = None) -> None:
        self.assertEqual(sorted((directory or self.draft).glob("*.sig")), [])

    def test_the_report_names_what_changed_from_the_hashes_of_the_two_install_trees(self) -> None:
        v1 = hash_tree(FIXTURES / "install" / "v1")
        v2 = hash_tree(FIXTURES / "install" / "v2")
        added = sorted(set(v2) - set(v1))
        removed = sorted(set(v1) - set(v2))
        changed = sorted(p for p in set(v1) & set(v2) if v1[p] != v2[p])
        unchanged = sorted(p for p in set(v1) & set(v2) if v1[p] == v2[p])
        self.assertEqual((added, removed, changed),
                         (["bin/libnew-2.dll", "share/katana/news.txt"], ["bin/libold-1.dll"],
                          ["bin/katana.exe", "etc/katana/settings.ini", "share/katana/readme.txt"]))
        with zipfile.ZipFile(self.draft / "Katana-0.10.0-win64.update.zip") as archive:
            download = sum(archive.getinfo(p).compress_size for p in added + changed)
        done = self.sign("--test-key", "--report-only")
        self.assert_tool_ok(done)
        out = done.stdout
        self.assertIn("win64: Katana 0.10.0, sequence 1792312800  (was 0.9.0, sequence 1790000000)", out)
        self.assertIn(f"{len(v2)} files: {len(added)} added, {len(removed)} removed, "
                      f"{len(changed)} changed, {len(unchanged)} unchanged", out)
        self.assertIn(f"downloads {download} bytes", out)
        for path in added:
            self.assertIn(f"+ {path}", out)
        for path in removed:
            self.assertIn(f"- {path}", out)
        for path in changed:
            self.assertIn(f"~ {path}", out)
        for path in unchanged:
            self.assertNotIn(path, out)
        self.assertIn("PROGRAMS: bin/katana.exe", out)
        self.assertIn("Report only: nothing was signed", out)
        self.assert_nothing_signed()

    def test_without_a_previous_release_every_file_is_new(self) -> None:
        done = run_tool("update_release_sign.py", "--dir", self.draft, "--tag", "v0.10.0", "--key", self.key,
                        "--test-key", "--report-only", env=self.environment)
        self.assert_tool_ok(done)
        self.assertIn("all new to this comparison", done.stdout)

    def test_typing_the_version_signs_and_the_signature_is_the_one_openssl_verifies(self) -> None:
        done = self.sign("--test-key")
        self.assert_tool_ok(done)
        manifest = (self.draft / MANIFEST).read_bytes()
        line = (self.draft / SIG).read_text(encoding="ascii")
        algorithm, key_id, body = line.split()
        self.assertEqual((algorithm, key_id), ("ed25519", RFC_PUBLIC[1][:16]))
        signature = base64.b64decode(body, validate=True)
        self.assertTrue(openssl_verify(RFC_PUBLIC[1], manifest, signature))
        # Ed25519 is deterministic: it is the very signature the fixtures were made with.
        self.assertEqual(line, (FIXTURES / "release" / "v2" / SIG).read_text(encoding="ascii"))
        self.assertIn("every signature verifies with the openssl command line", done.stdout)

    def test_the_upload_commands_are_printed_exactly_and_never_run(self) -> None:
        done = self.sign("--test-key")
        self.assert_tool_ok(done)
        commands = [shlex.split(line.strip()) for line in done.stdout.splitlines() if line.strip().startswith("gh ")]
        signature = (self.draft / SIG).resolve().as_posix()
        self.assertEqual(commands, [
            ["gh", "release", "upload", "v0.10.0", signature, "--repo", "kjarada/Katana"],
            ["gh", "release", "edit", "v0.10.0", "--repo", "kjarada/Katana", "--draft=false", "--latest"],
        ])
        self.assertIn("NOT run by this tool", done.stdout)
        self.assertFalse(self.ran.exists(), "the fake gh ran: " + (self.ran.read_text() if self.ran.exists() else ""))

    def test_the_tool_itself_imports_no_way_to_start_a_program(self) -> None:
        tree = ast.parse((TOOLS / "update_release_sign.py").read_text(encoding="utf-8"))
        imported = {a.name.split(".")[0] for n in ast.walk(tree) if isinstance(n, ast.Import) for a in n.names}
        imported |= {n.module.split(".")[0] for n in ast.walk(tree) if isinstance(n, ast.ImportFrom) and n.module}
        self.assertNotIn("subprocess", imported)
        self.assertNotIn("os", imported)

    def test_every_target_of_the_draft_is_signed_and_listed_in_one_upload_command(self) -> None:
        draft = self.tmp / "two-targets"
        for target in ("win64", "linux-x86_64"):
            tree = make_tree(self.tmp / ("tree-" + target), {"bin/katana.exe": b"x" * 100, "share/a.txt": b"a\n"})
            done = run_tool("update_make_manifest.py", "--tree", tree, "--out", draft, "--version", "0.10.0",
                            "--target", target, "--sequence", "1792312800")
            self.assert_tool_ok(done)
        done = run_tool("update_release_sign.py", "--dir", draft, "--tag", "v0.10.0", "--key", self.key, "--test-key",
                        stdin="0.10.0\n", env=self.environment)
        self.assert_tool_ok(done)
        self.assertEqual(sorted(p.name for p in draft.glob("*.sig")),
                         ["Katana-update-linux-x86_64.json.sig", "Katana-update-win64.json.sig"])
        upload = next(shlex.split(l.strip()) for l in done.stdout.splitlines() if l.strip().startswith("gh release upload"))
        self.assertEqual(sorted(Path(a).name for a in upload if a.endswith(".sig")),
                         ["Katana-update-linux-x86_64.json.sig", "Katana-update-win64.json.sig"])

    def test_a_wrong_or_missing_confirmation_signs_nothing(self) -> None:
        for answer in ("0.10\n", "v0.10.0\n", "0.9.0\n", "\n", "", "yes\n", " \n"):
            with self.subTest(answer=answer):
                done = self.sign("--test-key", answer=answer)
                self.assert_tool_failed(done, "Refused", "nothing was signed")
                self.assert_nothing_signed()

    def test_the_answer_may_have_spaces_round_it_but_must_be_the_version(self) -> None:
        self.assert_tool_ok(self.sign("--test-key", answer="  0.10.0  \n"))

    def test_a_tampered_zip_or_a_wrong_hash_in_the_draft_is_refused_before_anything_is_asked(self) -> None:
        zip_path = self.draft / "Katana-0.10.0-win64.update.zip"
        shutil.copyfile(FIXTURES / "release" / "bad" / "tampered-member" / zip_path.name, zip_path)
        done = self.sign("--test-key")
        self.assert_tool_failed(done, "Step 1", "nothing was signed", "random.bin")
        self.assertNotIn("Type the version", done.stdout)
        self.assert_nothing_signed()

    def test_a_manifest_that_is_not_for_this_tag_is_refused(self) -> None:
        done = self.sign("--test-key", tag="v0.9.9")
        self.assert_tool_failed(done, "not v0.9.9", "nothing was signed")
        self.assert_nothing_signed()

    def test_a_higher_version_is_above_a_previous_release_with_a_later_sequence(self) -> None:
        # downgrade-version is 0.9.0 with a sequence AFTER this draft's: the version decides.
        done = run_tool("update_release_sign.py", "--dir", self.draft, "--tag", "v0.10.0", "--key", self.key,
                        "--test-key", "--previous", FIXTURES / "release" / "bad" / "downgrade-version",
                        stdin="0.10.0\n", env=self.environment)
        self.assert_tool_ok(done)

    def test_the_same_build_as_the_previous_release_is_refused(self) -> None:
        done = run_tool("update_release_sign.py", "--dir", self.draft, "--tag", "v0.10.0", "--key", self.key,
                        "--test-key", "--previous", FIXTURES / "release" / "v2", stdin="0.10.0\n", env=self.environment)
        self.assert_tool_failed(done, "is not above the previous release", "nothing was signed")
        self.assert_nothing_signed()

    def test_a_draft_that_is_already_signed_is_not_signed_again(self) -> None:
        shutil.copyfile(FIXTURES / "release" / "v2" / SIG, self.draft / SIG)
        before = (self.draft / SIG).read_bytes()
        done = self.sign("--test-key")
        self.assert_tool_failed(done, "already signed")
        self.assertEqual((self.draft / SIG).read_bytes(), before)

    def test_the_spare_key_signs_too_but_a_key_the_build_does_not_pin_does_not(self) -> None:
        self.assert_tool_ok(self.sign("--test-key", key=self.rfc_pem(2)))
        self.assertEqual((self.draft / SIG).read_text().split()[1], RFC_PUBLIC[2][:16])
        (self.draft / SIG).unlink()
        done = self.sign("--test-key", key=self.rfc_pem(3))
        self.assert_tool_failed(done, "not one of the keys this build pins")
        self.assert_nothing_signed()

    def test_a_published_test_key_is_refused_for_a_real_signing(self) -> None:
        done = self.sign()  # no --test-key
        self.assert_tool_failed(done, "published test key")
        self.assert_nothing_signed()

    def test_the_placeholders_in_the_tree_refuse_a_real_signing(self) -> None:
        throwaway = self.write_pem("throwaway.pem", THROWAWAY_SEED)
        placeholders = self.tmp / "placeholders.cmake"
        placeholders.write_text("set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS ON)\n"
                                f'set(KATANA_UPDATE_KEY_ACTIVE "{"50" * 32}")\n'
                                f'set(KATANA_UPDATE_KEY_SPARE "{"51" * 32}")\n', newline="\n")
        done = self.sign("--keys-cmake", placeholders, key=throwaway)
        self.assert_tool_failed(done, "placeholder")
        self.assert_nothing_signed()

    def test_a_key_the_tree_pins_signs_without_the_test_flag(self) -> None:
        throwaway = self.write_pem("throwaway.pem", THROWAWAY_SEED)
        public = producer.public_key_of(OPENSSL, throwaway).hex()
        pinned = self.tmp / "pinned.cmake"
        pinned.write_text("set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)\n"
                          f'set(KATANA_UPDATE_KEY_ACTIVE "{public}")\n'
                          f'set(KATANA_UPDATE_KEY_SPARE "{"ab" * 32}")\n', newline="\n")
        done = self.sign("--keys-cmake", pinned, key=throwaway)
        self.assert_tool_ok(done)
        signature = base64.b64decode((self.draft / SIG).read_text().split()[2])
        self.assertTrue(openssl_verify(public, (self.draft / MANIFEST).read_bytes(), signature))
        self.assertFalse(openssl_verify(RFC_PUBLIC[1], (self.draft / MANIFEST).read_bytes(), signature))


if __name__ == "__main__":
    unittest.main()
