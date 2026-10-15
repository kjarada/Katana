"""tools/check_release_workflow.py --scripts: every script of the release workflow is read by its shell's own
parser, and a script that does not parse is refused by name.

The oracle is the shell: bash -n and PowerShell's parser decide what parses, not the tool. The case that gave the
check its reason is here as it was: "exited with $LASTEXITCODE: proj.db ..." in the installer smoke test, a parse
error in PowerShell ("Variable reference is not valid") that sat in the workflow for a day, since only a run of
five builds would have shown it.

These tests need a working bash and a working PowerShell and fail without them: tests/update/CMakeLists.txt
registers them only where both answer, so a pass is never a check that could not be made.

Standard library only.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(REPO / "tools"))
sys.dont_write_bytecode = True

import check_release_workflow as check  # noqa: E402

WORKFLOW_PATH = REPO / ".github" / "workflows" / "release.yml"
WORKFLOW = WORKFLOW_PATH.read_bytes().decode("utf-8").replace("\r\n", "\n")
BASH = check.interpreter_for("bash")
POWERSHELL = check.interpreter_for("pwsh")


def replaced(old: str, new: str) -> str:
    found = WORKFLOW.count(old)
    assert found == 1, "the case's change would reach %d places, not 1: %r" % (found, old)
    return WORKFLOW.replace(old, new)


class Interpreters(unittest.TestCase):
    def test_this_machine_has_the_shells_these_tests_need(self) -> None:
        self.assertIsNotNone(BASH, "no bash answers on PATH; CMake registers these tests only where one does")
        self.assertIsNotNone(POWERSHELL, "no pwsh or powershell answers on PATH")


class RealWorkflow(unittest.TestCase):
    def test_the_scripts_of_the_workflow_are_found_by_shell(self) -> None:
        # Not vacuous: the workflow has two PowerShell steps (the NSIS one and the installer smoke test) and
        # many bash steps, and the Windows job's own default (msys2's bash) is bash, not what `shell: pwsh`
        # of the two steps beside them says.
        kinds = [s.shell for s in check.scripts(WORKFLOW)]
        self.assertEqual(kinds.count("pwsh"), 2)
        self.assertGreaterEqual(kinds.count("bash"), 20)
        self.assertEqual(set(kinds), {"bash", "pwsh"})
        windows_bash = [s for s in check.scripts(WORKFLOW) if s.job == "windows" and s.shell == "bash"]
        self.assertGreaterEqual(len(windows_bash), 5)

    def test_every_bash_step_parses(self) -> None:
        found, count = check.script_problems(WORKFLOW, "bash", BASH)
        self.assertEqual(found, [])
        self.assertGreaterEqual(count, 20)

    def test_every_powershell_step_parses(self) -> None:
        found, count = check.script_problems(WORKFLOW, "pwsh", POWERSHELL)
        self.assertEqual(found, [])
        self.assertEqual(count, 2)

    def test_the_command_line_says_how_many_it_read(self) -> None:
        for kind in ("bash", "pwsh"):
            done = subprocess.run([sys.executable, "-I", "-B", str(REPO / "tools" / "check_release_workflow.py"),
                                   "--scripts", kind], capture_output=True, text=True, check=False)
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertRegex(done.stdout, r"\d+ %s step\(s\) of release.yml parse" % kind)


class Refused(unittest.TestCase):
    def test_a_colon_after_a_variable_in_a_powershell_string_the_defect_that_started_this(self) -> None:
        text = replaced("exited with $($LASTEXITCODE): proj.db", "exited with $LASTEXITCODE: proj.db")
        found, _ = check.script_problems(text, "pwsh", POWERSHELL)
        self.assertEqual(len(found), 1, found)
        self.assertIn("does not parse under PowerShell", found[0])
        self.assertIn("Variable reference is not valid", found[0])
        self.assertIn("(job windows)", found[0])

    def test_an_unclosed_quote_in_a_bash_step(self) -> None:
        text = replaced("sha256sum Katana-* | tee SHA256SUMS.txt", "sha256sum Katana-* | tee 'SHA256SUMS.txt")
        found, _ = check.script_problems(text, "bash", BASH)
        self.assertEqual(len(found), 1, found)
        self.assertIn("does not parse under bash", found[0])
        self.assertIn("(job publish)", found[0])

    def test_a_dangling_if_in_a_bash_step(self) -> None:
        text = replaced("sha256sum Katana-* | tee SHA256SUMS.txt", "if [ -z \"$X\" ]; then\n  sha256sum Katana-*")
        found, _ = check.script_problems(text, "bash", BASH)
        self.assertEqual(len(found), 1, found)

    def test_a_github_expression_is_no_syntax_error(self) -> None:
        # ${{ ... }} is the runner's, replaced before the shell sees the script: it must not be what fails.
        self.assertIn("${{", WORKFLOW)
        found, _ = check.script_problems(WORKFLOW, "bash", BASH)
        self.assertEqual(found, [])

    def test_a_broken_step_in_a_shell_that_is_not_checked_is_not_this_checks_to_find(self) -> None:
        text = replaced("shell: pwsh\n        run: |\n          $makensis", "shell: python\n        run: |\n          $makensis")
        found, count = check.script_problems(text, "pwsh", POWERSHELL)
        self.assertEqual(count, 1)  # only the smoke test is PowerShell now
        self.assertEqual(found, [])


class Degenerate(unittest.TestCase):
    def test_a_workflow_with_no_powershell_step_is_a_problem_not_a_pass(self) -> None:
        found, count = check.script_problems(WORKFLOW.replace("shell: pwsh", "shell: bash"), "pwsh", POWERSHELL)
        self.assertEqual(count, 0)
        self.assertTrue(any("found nothing to parse" in what for what in found), found)

    def test_an_empty_file_is_a_problem_not_a_pass(self) -> None:
        found, count = check.script_problems("", "bash", BASH)
        self.assertEqual(count, 0)
        self.assertTrue(found)

    def test_a_run_key_that_is_a_mapping_is_not_a_script(self) -> None:
        self.assertIn("    defaults:\n      run:\n        shell: msys2 {0}\n", WORKFLOW)
        lines = [s.line for s in check.scripts(WORKFLOW)]
        defaults_run = WORKFLOW.split("\n").index("      run:")
        self.assertNotIn(defaults_run, lines)

    def test_crlf_line_ends_give_the_same_scripts(self) -> None:
        a = [(s.line, s.shell, s.body) for s in check.scripts(WORKFLOW)]
        b = [(s.line, s.shell, s.body) for s in check.scripts(WORKFLOW.replace("\n", "\r\n"))]
        self.assertEqual(a, b)

    def test_without_the_shell_the_exit_status_is_two_and_not_a_pass(self) -> None:
        env = {k: v for k, v in os.environ.items() if k.upper() != "PATH"}
        env["PATH"] = ""
        with tempfile.TemporaryDirectory() as folder:
            for kind in ("bash", "pwsh"):
                done = subprocess.run([sys.executable, "-I", "-B", str(REPO / "tools" / "check_release_workflow.py"),
                                       "--scripts", kind, str(WORKFLOW_PATH)],
                                      capture_output=True, text=True, check=False, env=env, cwd=folder)
                self.assertEqual(done.returncode, 2, done.stdout + done.stderr)
                self.assertIn("so its steps were not checked", done.stderr)

    def test_an_unknown_kind_and_a_missing_file_are_exit_two(self) -> None:
        for arguments in (["--scripts", "zsh"], ["--scripts"], ["--scripts", "bash", str(HERE / "no_such.yml")]):
            done = subprocess.run([sys.executable, "-I", "-B", str(REPO / "tools" / "check_release_workflow.py"),
                                   *arguments], capture_output=True, text=True, check=False)
            self.assertEqual(done.returncode, 2, arguments)

    def test_a_refusal_is_exit_one_and_names_the_step(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            broken = Path(folder) / "release.yml"
            broken.write_bytes(WORKFLOW.replace("sha256sum Katana-* | tee SHA256SUMS.txt",
                                                "sha256sum Katana-* | tee 'SHA256SUMS.txt").encode())
            done = subprocess.run([sys.executable, "-I", "-B", str(REPO / "tools" / "check_release_workflow.py"),
                                   "--scripts", "bash", str(broken)], capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 1)
        self.assertRegex(done.stderr, r"the step at line \d+ \(job publish\) does not parse under bash")


if __name__ == "__main__":
    unittest.main()
