"""tools/update_keytool.py: it pins public keys in a SCRATCH COPY of cmake/KatanaUpdateKeys.cmake.

The real file is never written by a test. The keys are the published RFC 8032 test keys (and a throwaway
key that belongs to nobody), as PEM files built in a scratch folder. The result is judged by cmake itself:
the file's own validator, run as a script, must accept what the tool wrote.
"""

from __future__ import annotations

import contextlib
import io
import os
import re
import subprocess
import unittest
from pathlib import Path
from unittest import mock

from support import (
    CMAKE,
    OPENSSL,
    REPO,
    RFC_PUBLIC,
    THROWAWAY_SEED,
    ToolTest,
    needs_cmake,
    needs_openssl,
    public_pem,
    run_tool,
)

import update_keytool as keytool  # noqa: E402  (support puts tools/ on sys.path)

REAL_FILE = REPO / "cmake" / "KatanaUpdateKeys.cmake"
PLACEHOLDER_FIXTURE = REPO / "tests" / "update" / "key_fixtures" / "placeholders.cmake"
START = "# >>> keys"
END = "# <<< keys"
_REAL_FILE_AT_START = REAL_FILE.read_bytes()


# What the file held when it was made: 32 bytes each that spell "PLACEHOLDER-..." and no key.
PLACEHOLDER_ACTIVE = "504c414345484f4c4445522d4143544956452d4e4f542d412d4b45592d30302d"
PLACEHOLDER_SPARE = "504c414345484f4c4445522d53504152452d4e4f542d412d4b45592d2d30302d"

# The command that gives CI the signing key, as docs/release.md ("Update signing", step 3) gives it to the
# owner: a secret of the GitHub environment `release`, which only the Publish job runs in. Without
# `--env release` the same command makes a secret of the repository, which every workflow there can read.
SECRET_COMMAND = "gh secret set UPDATE_SIGNING_KEY --env release"


def secret_commands(text: str) -> list[str]:
    """Every `gh secret set UPDATE_SIGNING_KEY ...` in `text`, up to the `<` that feeds it the key (or the line's end)."""
    return [found.strip() for found in re.findall(r"gh secret set UPDATE_SIGNING_KEY[^<\r\n]*", text)]


def with_placeholders(data: bytes) -> bytes:
    """`data` with only the block between the marker lines replaced by the placeholder settings."""
    text = data.decode("utf-8")
    newline = "\r\n" if "\r\n" in text else "\n"
    block = newline.join([
        "set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS ON)",
        f'set(KATANA_UPDATE_KEY_ACTIVE "{PLACEHOLDER_ACTIVE}")',
        f'set(KATANA_UPDATE_KEY_SPARE "{PLACEHOLDER_SPARE}")',
    ])
    start = re.search(r"^# >>> keys[^\r\n]*", text, re.MULTILINE)
    end = re.search(r"^# <<< keys", text, re.MULTILINE)
    assert start and end
    return (text[: start.end()] + newline + block + newline + text[end.start():]).encode("utf-8")


def tearDownModule() -> None:
    # No test here writes the real keys file: whatever it held, it holds.
    assert REAL_FILE.read_bytes() == _REAL_FILE_AT_START, "a test wrote cmake/KatanaUpdateKeys.cmake"


@needs_openssl
class Pinning(ToolTest):
    def setUp(self) -> None:
        super().setUp()
        # The real file with its keys block put back to the placeholders it started with, whatever it holds now:
        # once the owner has pinned real keys these tests must still start from "nothing pinned".
        self.original = with_placeholders(REAL_FILE.read_bytes())
        self.scratch = self.tmp / "KatanaUpdateKeys.cmake"
        self.scratch.write_bytes(self.original)
        self.active = self.rfc_pem(1)
        self.spare = self.rfc_pem(2)

    def pub(self, *extra: object, active: Path = None, spare: Path = None, cmake: Path = None):
        return run_tool("update_keytool.py", "pub", active or self.active, spare or self.spare,
                        "--cmake", cmake or self.scratch, *extra)

    def split(self, data: bytes) -> tuple[bytes, bytes, bytes]:
        """(everything up to and including the start marker line, the block, the end marker line and after)."""
        # Line ends are compared as LF: a Windows checkout of this file is CRLF, and the CRLF test below
        # holds the tool to keeping whichever the file has.
        text = data.decode("utf-8").replace("\r\n", "\n")
        start = text.index(START)
        start_end = text.index("\n", start)
        end = text.index(END)
        return text[:start_end].encode(), text[start_end:end].encode(), text[end:].encode()

    def test_it_rewrites_only_the_block_between_the_markers(self) -> None:
        done = self.pub("--allow-test-keys")
        self.assert_tool_ok(done)
        before_head, _, before_tail = self.split(self.original)
        after_head, block, after_tail = self.split(self.scratch.read_bytes())
        self.assertEqual(after_head, before_head)
        self.assertEqual(after_tail, before_tail)
        self.assertEqual(block.decode(), "\n".join([
            "",
            "set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)",
            f'set(KATANA_UPDATE_KEY_ACTIVE "{RFC_PUBLIC[1]}")',
            f'set(KATANA_UPDATE_KEY_SPARE "{RFC_PUBLIC[2]}")',
            "",
        ]))
        self.assertIn(RFC_PUBLIC[1][:16], done.stdout)
        self.assertIn(RFC_PUBLIC[2][:16], done.stdout)

    def validate(self, *extra: str, require: str = "ON") -> subprocess.CompletedProcess:
        """The scratch file run through its own validator, as a script: the check a configure makes."""
        return subprocess.run([CMAKE, f"-DKATANA_REQUIRE_UPDATE_KEYS={require}", *extra, "-P", str(self.scratch)],
                              capture_output=True, text=True, check=False)

    def public_of(self, pem: Path) -> str:
        return subprocess.run([OPENSSL, "pkey", "-in", str(pem), "-pubout", "-outform", "DER"],
                              capture_output=True, check=True).stdout[-32:].hex()

    @needs_cmake
    def test_the_files_own_validator_refuses_placeholders_and_accepts_what_the_tool_wrote(self) -> None:
        # The control: with the placeholders in force the validator can say no, so its yes means something.
        control = self.validate(f"-DKATANA_UPDATE_KEYS_FIXTURE={PLACEHOLDER_FIXTURE}")
        self.assertNotEqual(control.returncode, 0, control.stderr)
        self.assertIn("PLACEHOLDER", control.stderr)
        # Two keys that are not published ones, pinned by the tool without --allow-test-keys.
        first = self.write_pem("first.pem", THROWAWAY_SEED)
        second = self.write_pem("second.pem", "07" * 32)
        self.assert_tool_ok(self.pub(active=first, spare=second))
        done = self.validate()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn(f"Update keys: pinned, active {self.public_of(first)[:16]}, "
                      f"spare {self.public_of(second)[:16]}", done.stderr + done.stdout)

    @needs_cmake
    def test_the_files_own_validator_refuses_a_published_test_key_in_either_slot_whatever_the_flag(self) -> None:
        # What `update_keytool.py pub --allow-test-keys` writes, or a hand edit of the tree's own file: the
        # tool refuses these keys without the flag, and the configure must refuse them whoever wrote them.
        mine = self.public_of(self.write_pem("other.pem", THROWAWAY_SEED))
        # Two keys that are not published ones: the key the test key leaves in the other slot.
        real_looking = "97fcb0a317920f5d988416cb98ba6937886975cc6b4a35d65a4b244c82d76428"
        for number in (1, 2, 3):
            for slot in ("ACTIVE", "SPARE"):
                for require in ("ON", "OFF"):
                    with self.subTest(test_key=number, slot=slot, require=require):
                        if slot == "ACTIVE":
                            self.pin_by_hand(RFC_PUBLIC[number], real_looking)
                        else:
                            self.pin_by_hand(real_looking, RFC_PUBLIC[number])
                        done = self.validate(require=require)
                        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
                        self.assertRegex(done.stderr, rf"KATANA_UPDATE_KEY_{slot}\s+is\s+a\s+published\s+RFC\s+8032")
        # The control: the same file with no test key in it passes, so the refusals above are about the key.
        self.pin_by_hand(mine, real_looking)
        for require in ("ON", "OFF"):
            done = self.validate(require=require)
            self.assertEqual(done.returncode, 0, done.stderr)

    def pin_by_hand(self, active: str, spare: str) -> None:
        """The scratch file with exactly these two keys and the flag off: a hand edit, not the tool."""
        text = self.original.decode("utf-8")
        newline = "\r\n" if "\r\n" in text else "\n"
        block = newline.join([
            "set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)",
            f'set(KATANA_UPDATE_KEY_ACTIVE "{active}")',
            f'set(KATANA_UPDATE_KEY_SPARE "{spare}")',
        ])
        start = re.search(r"^# >>> keys[^\r\n]*", text, re.MULTILINE)
        end = re.search(r"^# <<< keys", text, re.MULTILINE)
        self.scratch.write_bytes((text[: start.end()] + newline + block + newline + text[end.start():]).encode("utf-8"))

    def test_the_validators_list_of_published_keys_is_the_rfcs_and_the_tools(self) -> None:
        text = REAL_FILE.read_text(encoding="utf-8")
        listed = re.search(r"set\(_katana_published_test_keys\s+([^)]*)\)", text)
        self.assertIsNotNone(listed, "cmake/KatanaUpdateKeys.cmake must list the published test keys")
        in_cmake = re.findall(r'"([0-9a-f]{64})"', listed.group(1))
        # RFC_PUBLIC is copied from RFC 8032 section 7.1; the tool's own list must be the same three.
        self.assertCountEqual(in_cmake, RFC_PUBLIC.values())
        self.assertCountEqual(keytool.TEST_PUBLIC_KEYS, RFC_PUBLIC.values())

    def test_a_published_test_key_is_refused_without_the_flag_and_the_file_is_untouched(self) -> None:
        self.assert_tool_failed(self.pub(), "published test key")
        self.assertEqual(self.scratch.read_bytes(), self.original)

    def test_a_key_pair_that_is_not_a_test_key_is_pinned_without_the_flag(self) -> None:
        other = self.write_pem("other.pem", THROWAWAY_SEED)
        # TEST 3 is a published key too, so a pair holding it is refused.
        self.assert_tool_failed(self.pub(active=other, spare=self.rfc_pem(3)), "published test key")
        second = self.write_pem("second.pem", "07" * 32)
        self.assert_tool_ok(self.pub(active=other, spare=second))
        public = subprocess.run([OPENSSL, "pkey", "-in", str(other), "-pubout", "-outform", "DER"],
                                capture_output=True, check=True).stdout[-32:].hex()
        text = self.scratch.read_text(encoding="utf-8")
        self.assertIn(f'set(KATANA_UPDATE_KEY_ACTIVE "{public}")', text)
        self.assertIn("set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)", text)

    def test_the_same_key_twice_is_refused(self) -> None:
        self.assert_tool_failed(self.pub("--allow-test-keys", spare=self.active), "same key")
        self.assertEqual(self.scratch.read_bytes(), self.original)

    def test_something_that_is_not_a_private_key_is_refused(self) -> None:
        garbage = self.tmp / "garbage.pem"
        garbage.write_text("not a key at all\n")
        self.assert_tool_failed(self.pub("--allow-test-keys", active=garbage), "not a private key")
        public_only = self.tmp / "public-only.pem"
        public_only.write_text(public_pem(RFC_PUBLIC[1]), newline="\n")
        self.assert_tool_failed(self.pub("--allow-test-keys", active=public_only), "not a private key")
        self.assertEqual(self.scratch.read_bytes(), self.original)

    def test_a_target_that_is_not_the_keys_file_is_refused(self) -> None:
        as_pem = self.tmp / "keys.pem"
        as_pem.write_bytes(self.original)
        self.assert_tool_failed(self.pub("--allow-test-keys", cmake=as_pem), ".pem")
        with_secret = self.tmp / "with-secret.cmake"
        with_secret.write_bytes(self.original + b"# -----BEGIN PRIVATE KEY-----\n")
        self.assert_tool_failed(self.pub("--allow-test-keys", cmake=with_secret), "private key")

    def test_a_file_without_exactly_one_pair_of_markers_in_order_is_refused(self) -> None:
        text = self.original.decode("utf-8")
        cases = {
            "none": text.replace(START, "# something else").replace(END, "# else"),
            "twice": text + "\n" + START + "\n" + END + "\n",
            "reversed": text.replace(START, "@@S@@").replace(END, START).replace("@@S@@", END),
        }
        for name, content in cases.items():
            with self.subTest(case=name):
                path = self.tmp / f"{name}.cmake"
                path.write_bytes(content.encode("utf-8"))
                self.assert_tool_failed(self.pub("--allow-test-keys", cmake=path), "# >>> keys")
                self.assertEqual(path.read_bytes(), content.encode("utf-8"))

    def test_crlf_line_ends_are_kept(self) -> None:
        crlf = self.original.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
        self.scratch.write_bytes(crlf)
        self.assert_tool_ok(self.pub("--allow-test-keys"))
        result = self.scratch.read_bytes()
        self.assertNotIn(b"\n", result.replace(b"\r\n", b""), "every line end must still be CRLF")
        self.assertIn(f'set(KATANA_UPDATE_KEY_ACTIVE "{RFC_PUBLIC[1]}")\r\n'.encode(), result)

    def test_published_test_keys_are_never_written_to_the_trees_own_keys_file(self) -> None:
        # Without --cmake the target is the tree's own file; with it spelled out, the same. The scratch copy
        # stands in for that file here, so that a regression in the guard cannot damage the real one.
        for target in ([], ["--cmake", str(self.scratch)]):
            with self.subTest(target=target):
                out, err = io.StringIO(), io.StringIO()
                with mock.patch.object(keytool, "DEFAULT_KEYS_CMAKE", self.scratch), \
                        contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                    code = keytool.main(["pub", str(self.active), str(self.spare), "--allow-test-keys", *target])
                self.assertEqual(code, 1, out.getvalue())
                self.assertIn("never writes", err.getvalue())
                self.assertIn("scratch copy", err.getvalue())
                self.assertEqual(self.scratch.read_bytes(), self.original)

    def pinned(self) -> tuple[str, str]:
        """(active, spare) as the scratch file holds them now, read with a regular expression of the test's own."""
        text = self.scratch.read_text(encoding="utf-8")
        return (re.search(r'KATANA_UPDATE_KEY_ACTIVE "([0-9a-f]{64})"', text).group(1),
                re.search(r'KATANA_UPDATE_KEY_SPARE "([0-9a-f]{64})"', text).group(1))

    def test_replacing_both_pinned_keys_strands_every_installed_copy_and_is_refused_unless_forced(self) -> None:
        self.assert_tool_ok(self.pub("--allow-test-keys"))             # RFC 1 active, RFC 2 spare: pinned for real
        self.assertEqual(self.pinned(), (RFC_PUBLIC[1], RFC_PUBLIC[2]))
        first = self.write_pem("first.pem", THROWAWAY_SEED)
        second = self.write_pem("second.pem", "07" * 32)
        before = self.scratch.read_bytes()
        done = self.pub("--allow-test-keys", active=first, spare=second)
        self.assert_tool_failed(done, "neither is among the new keys", "--replace-both")
        self.assertEqual(self.scratch.read_bytes(), before)
        self.assert_tool_ok(self.pub("--allow-test-keys", "--replace-both", active=first, spare=second))
        # The pinned keys are the new pair. (Not "the RFC key is nowhere in the file": the file lists the
        # published test keys it refuses, so those digits are in it whatever is pinned.)
        self.assertEqual(self.pinned(), (self.public_of(first), self.public_of(second)))

    def test_a_rotation_that_keeps_one_pinned_key_is_allowed(self) -> None:
        self.assert_tool_ok(self.pub("--allow-test-keys"))
        new = self.write_pem("new.pem", THROWAWAY_SEED)
        new_public = subprocess.run([OPENSSL, "pkey", "-in", str(new), "-pubout", "-outform", "DER"],
                                    capture_output=True, check=True).stdout[-32:].hex()
        # The same pair again.
        self.assert_tool_ok(self.pub("--allow-test-keys"))
        self.assertEqual(self.pinned(), (RFC_PUBLIC[1], RFC_PUBLIC[2]))
        # A new spare beside the active key.
        self.assert_tool_ok(self.pub("--allow-test-keys", spare=new))
        self.assertEqual(self.pinned(), (RFC_PUBLIC[1], new_public))
        # The active key leaked: the old spare is the active one and the new key is the spare.
        self.assert_tool_ok(self.pub("--allow-test-keys", active=self.write_pem("kept.pem", THROWAWAY_SEED), spare=self.rfc_pem(1)))
        self.assertEqual(self.pinned(), (new_public, RFC_PUBLIC[1]))

    def test_the_placeholders_pin_nothing_so_the_first_pair_is_never_a_replacement(self) -> None:
        self.assertIn("set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS ON)", self.scratch.read_text(encoding="utf-8"))
        self.assert_tool_ok(self.pub("--allow-test-keys"))

    def test_the_file_is_flushed_to_disk_before_it_replaces_the_old_one(self) -> None:
        calls = []
        real_fsync = os.fsync
        real_replace = os.replace

        def fsync(descriptor):
            calls.append("fsync")
            return real_fsync(descriptor)

        def replace(source, target, *args, **kwargs):
            calls.append("replace")
            return real_replace(source, target, *args, **kwargs)

        with mock.patch.object(os, "fsync", fsync), mock.patch.object(os, "replace", replace):
            keytool.pin_keys(self.active, self.spare, self.scratch, allow_test_keys=True, openssl=OPENSSL)
        self.assertEqual(calls, ["fsync", "replace"])

    def test_it_writes_nothing_but_the_keys_file_and_never_a_private_key(self) -> None:
        folder = self.scratch.parent
        before = sorted(p.name for p in folder.iterdir())
        done = self.pub("--allow-test-keys")
        self.assert_tool_ok(done)
        self.assertEqual(sorted(p.name for p in folder.iterdir()), before)
        self.assertNotIn(b"PRIVATE", self.scratch.read_bytes())
        self.assertNotIn("PRIVATE", done.stdout + done.stderr)

    def test_after_pinning_it_gives_the_command_that_sets_the_secret_of_the_environment_release(self) -> None:
        done = self.pub("--allow-test-keys")
        self.assert_tool_ok(done)
        # The whole command, with the active key's own file after the `<` and never the spare's.
        self.assertIn(f"{SECRET_COMMAND} < {self.active}", done.stdout)
        self.assertNotIn(str(self.spare), done.stdout.split("gh secret set", 1)[1])
        # And in no other form: a command without --env release would make a secret every workflow can read.
        self.assertEqual(secret_commands(done.stdout), [SECRET_COMMAND])

    def test_a_refused_pair_is_given_no_command_for_the_secret(self) -> None:
        done = self.pub()                                              # published test keys, no flag: refused
        self.assert_tool_failed(done, "published test key")
        self.assertEqual(secret_commands(done.stdout + done.stderr), [])


class TheCommandLine(ToolTest):
    def test_there_is_no_gen_command(self) -> None:
        done = run_tool("update_keytool.py", "gen", "x.pem")
        self.assertEqual(done.returncode, 2, done.stderr)
        self.assertIn("invalid choice", done.stderr)

    def test_help_gives_the_owners_exact_commands(self) -> None:
        text = run_tool("update_keytool.py", "--help").stdout
        for command in (
            "openssl genpkey -algorithm ed25519 -out update-signing.pem",
            "openssl genpkey -algorithm ed25519 -out update-spare.pem",
            "python tools/update_keytool.py pub update-signing.pem update-spare.pem",
            f"{SECRET_COMMAND} < update-signing.pem",
        ):
            self.assertIn(command, text)
        self.assertIn("NO `gen`", text)

    def test_help_gives_the_secret_command_only_for_the_environment_release(self) -> None:
        # Once, and with --env release: the help must not also offer the repository-wide form as a first step
        # "until the environment exists", which is the form that would then be the one run.
        text = run_tool("update_keytool.py", "--help").stdout
        self.assertEqual(secret_commands(text), [SECRET_COMMAND])
        self.assertNotIn("add --env release", text)

    def test_the_reader_of_secret_commands_tells_the_two_forms_apart(self) -> None:
        # The helper the tests above lean on, held to texts written here.
        self.assertEqual(secret_commands("x\n   gh secret set UPDATE_SIGNING_KEY < update-signing.pem\n"),
                         ["gh secret set UPDATE_SIGNING_KEY"])
        self.assertEqual(secret_commands("one:  gh secret set UPDATE_SIGNING_KEY --env release < a.pem\r\n"
                                         "two: gh secret set UPDATE_SIGNING_KEY\r\n"),
                         [SECRET_COMMAND, "gh secret set UPDATE_SIGNING_KEY"])
        self.assertEqual(secret_commands("gh secret set RELEASES_REPO_TOKEN"), [])


if __name__ == "__main__":
    unittest.main()
