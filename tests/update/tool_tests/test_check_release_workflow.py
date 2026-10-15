"""tools/check_release_workflow.py: the release workflow's shape is held, and every way of undoing it is seen.

The oracle is docs/release.md, not the tool: each case below is a change a later edit could make to the real
workflow (a second trigger, an action named by a tag, the signing key read by another job, a configure without
the update keys required ...), made on the file's own text, and the check has to refuse it by naming what is
wrong. A check that cannot see a violation passes as quietly as one that finds none, so a case first proves its
change reached something (the text differs from the real file), and the real file itself must pass.

Standard library only; no openssl, no cmake.
"""

from __future__ import annotations

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


def replaced(old: str, new: str, count: int = 1) -> str:
    """The real workflow with `old` (found exactly `count` times) replaced everywhere by `new`."""
    found = WORKFLOW.count(old)
    assert found == count, "the case's change would reach %d places, not %d: %r" % (found, count, old)
    return WORKFLOW.replace(old, new)


def after_first(anchor: str, old: str, new: str) -> str:
    """The real workflow with the first `old` after the first `anchor` replaced by `new`."""
    start = WORKFLOW.index(anchor)
    at = WORKFLOW.index(old, start)
    return WORKFLOW[:at] + new + WORKFLOW[at + len(old):]


def in_job(job: str, anchor_after_header: str, addition: str) -> str:
    """The real workflow with `addition` (lines) put before the first `anchor_after_header` of job `job`."""
    header = WORKFLOW.index("\n  %s:\n" % job)
    at = WORKFLOW.index(anchor_after_header, header)
    return WORKFLOW[:at] + addition + WORKFLOW[at:]


def first_uses(action: str) -> re.Match:
    match = re.search(r"uses: (%s)@([0-9a-f]{40})( # \S+)" % re.escape(action), WORKFLOW)
    assert match, "the workflow has no `uses: %s@<commit> # <tag>` line" % action
    return match


class RealWorkflow(unittest.TestCase):
    def test_the_real_workflow_is_the_shape_the_documents_describe(self) -> None:
        self.assertEqual(check.problems(WORKFLOW), [])

    def test_the_command_line_says_so_with_exit_status_zero(self) -> None:
        done = subprocess.run([sys.executable, "-I", "-B", str(REPO / "tools" / "check_release_workflow.py"),
                               str(WORKFLOW_PATH)], capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn("starts by hand only", done.stdout)

    def test_the_cases_below_reach_what_the_real_file_has(self) -> None:
        # Not vacuous: the file has the things the cases change - four checkouts of one commit, an
        # environment, one read of the signing key, two configures that pass the required settings.
        self.assertEqual(len(re.findall(r"uses: actions/checkout@", WORKFLOW)), 4)
        self.assertEqual(WORKFLOW.count("    environment: release\n"), 1)
        reads = [line for line in WORKFLOW.split("\n") if "secrets.UPDATE_SIGNING_KEY" in check._code(line)]
        self.assertEqual(len(reads), 1)
        self.assertEqual(len(re.findall(r"cmake --preset", WORKFLOW)), 2)

    def test_the_verdict_does_not_depend_on_the_line_ends(self) -> None:
        self.assertEqual(check.problems(WORKFLOW.replace("\n", "\r\n")), [])
        broken = replaced("  workflow_dispatch:\n", "  push:\n  workflow_dispatch:\n")
        self.assertEqual(check.problems(broken.replace("\n", "\r\n")), check.problems(broken))
        self.assertNotEqual(check.problems(broken), [])


class Refused(unittest.TestCase):
    def refuses(self, text: str, phrase: str) -> None:
        self.assertNotEqual(text, WORKFLOW, "the case changed nothing")
        found = check.problems(text)
        self.assertTrue(any(phrase in what for what in found),
                        "expected a problem containing %r, got:\n  %s" % (phrase, "\n  ".join(found) or "(none)"))

    # 1. The trigger.
    def test_a_push_trigger(self) -> None:
        self.refuses(replaced("on:\n  workflow_dispatch:\n", "on:\n  push:\n    branches: [main]\n  workflow_dispatch:\n"),
                     "starts on 'push' too")

    def test_a_pull_request_target_trigger(self) -> None:
        self.refuses(replaced("on:\n  workflow_dispatch:\n", "on:\n  pull_request_target:\n  workflow_dispatch:\n"),
                     "starts on 'pull_request_target' too")

    def test_a_schedule_trigger(self) -> None:
        self.refuses(replaced("on:\n  workflow_dispatch:\n", "on:\n  schedule:\n    - cron: '0 3 * * *'\n  workflow_dispatch:\n"),
                     "starts on 'schedule' too")

    def test_triggers_written_on_the_same_line(self) -> None:
        self.refuses(replaced("on:\n  workflow_dispatch:\n", "on: [workflow_dispatch, push]\n  workflow_dispatch:\n"),
                     "carries its triggers on the same line")

    def test_no_workflow_dispatch(self) -> None:
        self.refuses(replaced("on:\n  workflow_dispatch:\n", "on:\n  release:\n"), "does not start on workflow_dispatch")

    # 2. Permissions.
    def test_a_token_that_can_write_at_the_top(self) -> None:
        self.refuses(replaced("permissions:\n  contents: read\n", "permissions:\n  contents: write\n"),
                     "must be exactly `contents: read`")

    def test_a_top_level_permissions_block_that_went_missing(self) -> None:
        self.refuses(replaced("permissions:\n  contents: read\n\n", ""), "sets no top-level `permissions:`")

    def test_write_in_a_job_other_than_publish(self) -> None:
        self.refuses(in_job("windows", "    runs-on:", "    permissions:\n      contents: write\n"),
                     "outside the Publish job")

    def test_write_all_in_a_job_other_than_publish(self) -> None:
        self.refuses(in_job("unix", "    runs-on:", "    permissions: write-all\n"), "outside the Publish job")

    def test_the_publish_job_that_stops_stating_its_permissions(self) -> None:
        self.refuses(replaced("    permissions:\n      contents: write\n", ""), "does not state its own `permissions:`")

    # 3. The actions.
    def test_an_action_named_by_its_tag(self) -> None:
        m = first_uses("actions/checkout")
        self.refuses(WORKFLOW.replace(m.group(0), "uses: actions/checkout@v5", 1), "is not pinned to a commit")

    def test_an_action_named_by_a_short_digest(self) -> None:
        m = first_uses("actions/upload-artifact")
        self.refuses(WORKFLOW.replace(m.group(0), "uses: actions/upload-artifact@%s%s" % (m.group(2)[:7], m.group(3)), 1),
                     "is not pinned to a commit")

    def test_an_action_named_by_a_branch(self) -> None:
        m = first_uses("actions/cache")
        self.refuses(WORKFLOW.replace(m.group(0), "uses: actions/cache@main", 1), "is not pinned to a commit")

    def test_an_action_by_a_digest_with_no_tag_comment(self) -> None:
        m = first_uses("actions/setup-python")
        self.refuses(WORKFLOW.replace(m.group(0), "uses: actions/setup-python@%s" % m.group(2), 1),
                     "has no comment naming the tag")

    def test_one_action_at_two_commits(self) -> None:
        m = first_uses("actions/checkout")
        other = "0123456789abcdef0123456789abcdef01234567"
        self.assertNotEqual(m.group(2), other)
        self.refuses(WORKFLOW.replace(m.group(0), "uses: actions/checkout@%s%s" % (other, m.group(3)), 1),
                     "actions/checkout is pinned to 2 different commits")

    def test_a_docker_image_in_place_of_an_action(self) -> None:
        self.refuses(WORKFLOW.replace("    steps:\n", "    steps:\n      - uses: docker://alpine:3\n", 1),
                     "is not pinned to a commit")

    # 4. The environment and the secret.
    def test_the_publish_job_outside_the_environment(self) -> None:
        self.refuses(replaced("    environment: release\n", ""), "no job names an `environment:`")

    def test_the_publish_job_in_another_environment(self) -> None:
        self.refuses(replaced("    environment: release\n", "    environment: production\n"),
                     "must be `environment: release`")

    def test_another_job_in_the_environment(self) -> None:
        self.refuses(in_job("windows", "    runs-on:", "    environment: release\n"), "only the Publish job names the environment")

    def test_the_signing_key_read_by_another_job(self) -> None:
        self.refuses(in_job("unix", "    runs-on:", "    env:\n      LEAK: ${{ secrets.UPDATE_SIGNING_KEY }}\n"),
                     "only the Publish job, behind the environment, may read it")

    def test_the_signing_key_read_twice_in_the_publish_job(self) -> None:
        self.refuses(replaced("      HAS_GPG_KEY:", "      ALSO: ${{ secrets.UPDATE_SIGNING_KEY }}\n      HAS_GPG_KEY:"),
                     "is read 2 times")

    def test_the_signing_key_read_by_nothing(self) -> None:
        self.refuses(replaced("secrets.UPDATE_SIGNING_KEY", "secrets.SOMETHING_ELSE"), "nothing reads secrets.UPDATE_SIGNING_KEY")

    def test_every_secret_inherited(self) -> None:
        self.refuses(in_job("windows", "    runs-on:", "    secrets: inherit\n"), "`secrets: inherit`")

    # 5. The Publish job.
    def test_publish_that_does_not_wait_for_the_build_jobs(self) -> None:
        self.refuses(replaced("    needs: [version, windows, unix]\n", "    needs: [version]\n"),
                     "does not wait for the job 'windows'")

    def test_publish_with_no_needs_at_all(self) -> None:
        self.refuses(replaced("    needs: [version, windows, unix]\n", ""), "has no `needs:`")

    def test_publish_that_runs_without_a_tag(self) -> None:
        self.refuses(replaced("    if: needs.version.outputs.tag != ''\n", ""), "does not run only for a tag")

    # 6. The configure steps.
    def test_a_configure_that_stops_requiring_the_update_keys(self) -> None:
        self.refuses(after_first("cmake --preset", "-DKATANA_REQUIRE_UPDATE_KEYS=ON", ""),
                     "does not pass -DKATANA_REQUIRE_UPDATE_KEYS=ON")

    def test_a_configure_that_stops_naming_the_repository(self) -> None:
        self.refuses(after_first("cmake --preset", "-DKATANA_UPDATE_REPO=", "-DKATANA_SOMETHING="),
                     "does not pass -DKATANA_UPDATE_REPO=")

    def test_a_configure_that_stops_passing_the_sequence(self) -> None:
        start = WORKFLOW.rindex("cmake --preset")
        at = WORKFLOW.index("-DKATANA_UPDATE_SEQUENCE=", start)
        self.refuses(WORKFLOW[:at] + "-DKATANA_NOT_THE_SEQUENCE=" + WORKFLOW[at + len("-DKATANA_UPDATE_SEQUENCE="):],
                     "does not pass -DKATANA_UPDATE_SEQUENCE=")

    def test_a_workflow_with_no_configure_at_all(self) -> None:
        self.refuses(WORKFLOW.replace("cmake --preset", "cmake --version #"), "configure step(s)")

    # 7. Typed text in a script.
    def test_the_tag_typed_by_a_person_in_a_script(self) -> None:
        self.refuses(replaced("          echo \"::error::publish_repo names $PUBLISH_REPO,",
                              "          echo ${{ inputs.tag }}\n          echo \"::error::publish_repo names $PUBLISH_REPO,"),
                     "what a person typed when starting the run is in a script")

    def test_an_event_field_in_a_script(self) -> None:
        self.refuses(replaced("          echo \"::error::publish_repo names $PUBLISH_REPO,",
                              "          echo \"${{ github.event.inputs.publish_repo }}\"\n          echo \"::error::publish_repo names $PUBLISH_REPO,"),
                     "what a person typed when starting the run is in a script")

    def test_an_input_in_a_one_line_script(self) -> None:
        self.refuses(WORKFLOW.replace("    steps:\n", "    steps:\n      - run: echo ${{ inputs.sign }}\n", 1),
                     "what a person typed when starting the run is in a script")

    def test_an_input_in_env_is_how_it_goes_in(self) -> None:
        # The way the workflow passes them is not refused: INPUT_TAG: ${{ inputs.tag }} under env.
        self.assertIn("INPUT_TAG: ${{ inputs.tag }}", WORKFLOW)
        self.assertEqual(check.problems(WORKFLOW), [])

    # 8. What an update leaves alone.
    def test_the_exclude_list_widened(self) -> None:
        self.refuses(replaced('  UPDATE_EXCLUDE: "LICENSE"\n', '  UPDATE_EXCLUDE: "LICENSE bin/*"\n'),
                     "the top-level env UPDATE_EXCLUDE is 'LICENSE bin/*'")

    def test_the_linux_exclude_gone(self) -> None:
        self.refuses(replaced('  UPDATE_EXCLUDE_LINUX: "ssl/*"\n', ""), "the top-level env UPDATE_EXCLUDE_LINUX is None")

    def test_the_seed_changed(self) -> None:
        self.refuses(replaced('  UPDATE_SEED: "share/katana/samples/*"\n', '  UPDATE_SEED: "share/*"\n'),
                     "the top-level env UPDATE_SEED is 'share/*'")


class Degenerate(unittest.TestCase):
    def test_an_empty_file_is_refused_not_passed_and_not_a_crash(self) -> None:
        found = check.problems("")
        self.assertTrue(found)
        self.assertTrue(any("no `on:` block" in what for what in found))
        self.assertTrue(any("no job `publish`" in what for what in found))

    def test_a_file_of_comments_only_is_refused(self) -> None:
        self.assertTrue(check.problems("# nothing\n# here\n"))

    def test_text_that_is_not_yaml_is_refused(self) -> None:
        self.assertTrue(check.problems("\x00\x01 not a workflow \ufffd"))

    def test_a_missing_file_is_exit_two_with_a_reason(self) -> None:
        done = subprocess.run([sys.executable, "-I", "-B", str(REPO / "tools" / "check_release_workflow.py"),
                               str(HERE / "no_such_workflow.yml")], capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 2)  # could not check: not the same as a refusal (1)
        self.assertIn("cannot read", done.stderr)

    def test_a_refusal_goes_to_the_exit_status_and_names_the_line(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            broken = Path(folder) / "release.yml"
            broken.write_bytes(WORKFLOW.replace("on:\n  workflow_dispatch:\n", "on:\n  push:\n  workflow_dispatch:\n").encode())
            done = subprocess.run([sys.executable, "-I", "-B", str(REPO / "tools" / "check_release_workflow.py"),
                                   str(broken)], capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 1)
        self.assertIn("starts on 'push' too", done.stderr)
        self.assertRegex(done.stderr, r"line \d+: ")


if __name__ == "__main__":
    unittest.main()
