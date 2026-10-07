"""tools/update_keytool.py: it pins public keys in a SCRATCH COPY of cmake/KatanaUpdateKeys.cmake.

The real file is never written by a test. The keys are the published RFC 8032 test keys (and a throwaway
key that belongs to nobody), as PEM files built in a scratch folder. The result is judged by cmake itself:
the file's own validator, run as a script, must accept what the tool wrote.
"""

from __future__ import annotations

import subprocess
import unittest
from pathlib import Path

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

REAL_FILE = REPO / "cmake" / "KatanaUpdateKeys.cmake"
PLACEHOLDER_FIXTURE = REPO / "tests" / "update" / "key_fixtures" / "placeholders.cmake"
START = "# >>> keys"
END = "# <<< keys"
_REAL_FILE_AT_START = REAL_FILE.read_bytes()


def tearDownModule() -> None:
    # No test here writes the real keys file: whatever it held, it holds.
    assert REAL_FILE.read_bytes() == _REAL_FILE_AT_START, "a test wrote cmake/KatanaUpdateKeys.cmake"


@needs_openssl
class Pinning(ToolTest):
    def setUp(self) -> None:
        super().setUp()
        self.original = REAL_FILE.read_bytes()
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

    @needs_cmake
    def test_the_files_own_validator_refuses_placeholders_and_accepts_what_the_tool_wrote(self) -> None:
        def validate(*extra: str) -> subprocess.CompletedProcess:
            return subprocess.run([CMAKE, "-DKATANA_REQUIRE_UPDATE_KEYS=ON", *extra, "-P", str(self.scratch)],
                                  capture_output=True, text=True, check=False)

        # The control: with the placeholders in force the validator can say no, so its yes means something.
        control = validate(f"-DKATANA_UPDATE_KEYS_FIXTURE={PLACEHOLDER_FIXTURE}")
        self.assertNotEqual(control.returncode, 0, control.stderr)
        self.assertIn("PLACEHOLDER", control.stderr)
        self.assert_tool_ok(self.pub("--allow-test-keys"))
        done = validate()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn(f"Update keys: pinned, active {RFC_PUBLIC[1][:16]}, spare {RFC_PUBLIC[2][:16]}", done.stderr + done.stdout)

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

    def test_it_writes_nothing_but_the_keys_file_and_never_a_private_key(self) -> None:
        folder = self.scratch.parent
        before = sorted(p.name for p in folder.iterdir())
        done = self.pub("--allow-test-keys")
        self.assert_tool_ok(done)
        self.assertEqual(sorted(p.name for p in folder.iterdir()), before)
        self.assertNotIn(b"PRIVATE", self.scratch.read_bytes())
        self.assertNotIn("PRIVATE", done.stdout + done.stderr)


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
            "gh secret set UPDATE_SIGNING_KEY < update-signing.pem",
        ):
            self.assertIn(command, text)
        self.assertIn("NO `gen`", text)


if __name__ == "__main__":
    unittest.main()
