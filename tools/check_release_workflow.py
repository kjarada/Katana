#!/usr/bin/env python3
"""Holds .github/workflows/release.yml to the SHAPE that docs/release.md says it has.

    python tools/check_release_workflow.py [path/to/release.yml]
    python tools/check_release_workflow.py --scripts bash|pwsh [path/to/release.yml]

The release workflow is the one place where a secret that matters (the update signing key) meets code
that can be edited by a commit, so what keeps that safe is written down in docs/release.md, "Publishing a
release", "Update signing" and "The actions the workflow runs", and until this check nothing in the
repository noticed if a later edit undid it. What it holds, each refusal naming the line:

  1. The workflow starts on workflow_dispatch and on nothing else: a commit, a tag, a pull request, a
     schedule or another workflow starts nothing (the owner's decision of 2026-09-26).
  2. The token it is given reads the repository (`permissions: contents: read`), and only the Publish job
     may write, which it must say itself.
  3. Every `uses:` names its action by the 40 digits of a commit, with the tag it was read from as a
     comment on the line, and an action is one commit wherever it is used. A tag is whatever its
     publisher points it at on the day of the run.
  4. The environment `release` is named once, by the job `publish`; the secret UPDATE_SIGNING_KEY is
     read once, in that job, and no job inherits secrets.
  5. The Publish job waits for the Version job and both build jobs, and runs only for a tag.
  6. Every configure step passes -DKATANA_REQUIRE_UPDATE_KEYS=ON (which keeps a release from being built
     with keys nobody holds), -DKATANA_UPDATE_REPO and -DKATANA_UPDATE_SEQUENCE.
  7. No script is built from text a person typed when starting the run: a `run:` holds no
     ${{ inputs.* }}, ${{ github.event.* }} or ${{ github.head_ref }}; they go in through `env:`.
  8. What an update leaves alone (UPDATE_SEED, UPDATE_EXCLUDE, UPDATE_EXCLUDE_LINUX) is what
     docs/release.md, "Update assets", says.

It reads lines and indentation, not YAML: Python's standard library has no YAML parser, and the file is
regular. Whatever it does not understand it reports as a problem rather than passing over.

With --scripts it also hands every `run:` of that shell to the shell's own parser and runs nothing: `bash -n`
for the steps that run under bash (the default of the Linux and macOS jobs, and msys2's bash in the Windows
one), PowerShell's parser for the steps that say `shell: pwsh` (the parser of `pwsh`, or of Windows
PowerShell when that is all there is, which does not read PowerShell 7's `&&`). A `${{ ... }}` is replaced by
a word first. It exists because a string such as "exited with $LASTEXITCODE: ..." is a parse error in
PowerShell, sat in the installer smoke test for a day, and no run could have shown it before an hour of
builds. A shell it cannot find or start is an exit status of 2, never a pass.

What it does not hold: that the scripts do what they should (the workflow's own run does), and that the
settings GitHub keeps outside the repository are as they should be (docs/release.md, "Update signing", step
3, has the reads for those).

Exit status 0 with one line when the file is as it must be; 1 with one line per problem; 2 when it could not
check (a file it cannot read, a shell it cannot start).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
DEFAULT_WORKFLOW = REPO / ".github" / "workflows" / "release.yml"

# The jobs the Publish job waits for, by the ids the workflow gives them.
PUBLISH_NEEDS = ("version", "windows", "unix")
# What a configure of the release must be given (docs/release.md, "What each job does").
CONFIGURE_SETTINGS = ("-DKATANA_REQUIRE_UPDATE_KEYS=ON", "-DKATANA_UPDATE_REPO=", "-DKATANA_UPDATE_SEQUENCE=")
# What the producer is told about an unpacked package (docs/release.md, "Update assets").
UPDATE_ENV = {
    "UPDATE_SEED": "share/katana/samples/*",
    "UPDATE_EXCLUDE": "LICENSE",
    "UPDATE_EXCLUDE_LINUX": "ssl/*",
}
# Expressions a person types when starting a run; none belongs in a script (script injection).
TYPED_EXPRESSION = re.compile(r"\$\{\{\s*(inputs\.|github\.event|github\.head_ref)")

_TOP_KEY = re.compile(r"^([A-Za-z_][A-Za-z0-9_-]*):(.*)$")
_JOB_KEY = re.compile(r"^  ([A-Za-z_][A-Za-z0-9_-]*):\s*$")
_USES = re.compile(r"^(\s*)(?:-\s+)?uses:\s*(\S+)(.*)$")
_PINNED = re.compile(r"^([A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+)@([0-9a-f]{40})$")


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _blocks(lines: list[str]) -> dict[str, tuple[int, int]]:
    """The top-level keys of the file, each with the half-open range of lines it owns."""
    starts = [(m.group(1), i) for i, line in enumerate(lines) if (m := _TOP_KEY.match(line))]
    blocks: dict[str, tuple[int, int]] = {}
    for n, (key, start) in enumerate(starts):
        end = starts[n + 1][1] if n + 1 < len(starts) else len(lines)
        blocks.setdefault(key, (start, end))
    return blocks


def _jobs(lines: list[str], blocks: dict[str, tuple[int, int]]) -> dict[str, tuple[int, int]]:
    """The jobs of the `jobs:` block, each with its half-open range of lines."""
    if "jobs" not in blocks:
        return {}
    start, end = blocks["jobs"]
    heads = [(m.group(1), i) for i in range(start + 1, end) if (m := _JOB_KEY.match(lines[i]))]
    jobs: dict[str, tuple[int, int]] = {}
    for n, (name, first) in enumerate(heads):
        jobs[name] = (first, heads[n + 1][1] if n + 1 < len(heads) else end)
    return jobs


def _job_of(jobs: dict[str, tuple[int, int]], line: int) -> str | None:
    for name, (start, end) in jobs.items():
        if start <= line < end:
            return name
    return None


def _code(line: str) -> str:
    """The line without a comment that begins at a space and a hash (YAML's rule outside quotes)."""
    stripped = line.lstrip()
    if stripped.startswith("#"):
        return ""
    return re.split(r"\s#", line, maxsplit=1)[0]


def problems(text: str) -> list[str]:
    """Every way `text`, the content of a release workflow, is not the shape described above. Empty: it is."""
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    found: list[str] = []

    def problem(line: int | None, what: str) -> None:
        found.append(("line %d: " % (line + 1) if line is not None else "") + what)

    blocks = _blocks(lines)
    jobs = _jobs(lines, blocks)

    # 1. The trigger.
    if "on" not in blocks:
        problem(None, "the workflow has no `on:` block, so nothing says what starts it; it must start on "
                      "workflow_dispatch alone")
    else:
        start, end = blocks["on"]
        if lines[start].strip() != "on:":
            problem(start, "`on:` carries its triggers on the same line (%r); write them as a block, and "
                           "only workflow_dispatch" % lines[start].strip())
        triggers = []
        for i in range(start + 1, end):
            code = _code(lines[i])
            if not code.strip():
                continue
            if _indent(code) == 2:
                key = re.match(r"^  ([A-Za-z_][A-Za-z0-9_-]*):", code)
                if key:
                    triggers.append(key.group(1))
                else:
                    problem(i, "a trigger written in a form this check does not read (%r); a trigger is a "
                               "key of the `on:` block" % code.strip())
        for trigger in triggers:
            if trigger != "workflow_dispatch":
                problem(start, "the workflow starts on %r too; it starts by hand (workflow_dispatch) and on "
                               "nothing else: a commit, a tag or a pull request starts no build of five "
                               "platforms" % trigger)
        if "workflow_dispatch" not in triggers:
            problem(start, "the workflow does not start on workflow_dispatch, the one trigger it has")

    # 2. Permissions.
    if "permissions" not in blocks:
        problem(None, "the workflow sets no top-level `permissions:`; the token it is given would be the "
                      "repository's default, which may write")
    else:
        start, end = blocks["permissions"]
        granted = [_code(lines[i]).strip() for i in range(start + 1, end) if _code(lines[i]).strip()]
        if lines[start].strip() != "permissions:" or granted != ["contents: read"]:
            problem(start, "the top-level `permissions:` must be exactly `contents: read`; it is %r"
                    % (" ".join([lines[start].strip()] + granted)))
    for i, line in enumerate(lines):
        code = _code(line).strip()
        if re.match(r"^(?:[A-Za-z-]+:\s*write|permissions:\s*write-all)$", code):
            owner = _job_of(jobs, i)
            if owner != "publish":
                problem(i, "%r outside the Publish job (%s): only that job may write, and it says so itself"
                        % (code, "in job %r" % owner if owner else "at the top level"))

    # 3. The actions.
    commits: dict[str, set[str]] = {}
    for i, line in enumerate(lines):
        if _code(line).strip() == "" or not (m := _USES.match(_code(line))):
            continue
        reference = m.group(2).strip("\"'")
        if reference.startswith("./"):
            continue
        pinned = _PINNED.match(reference)
        if not pinned:
            problem(i, "`uses: %s` is not pinned to a commit; it must end in @ and the 40 hexadecimal digits "
                       "of the commit (a tag, a branch or a short digest is whatever its publisher moves it "
                       "to)" % reference)
            continue
        if not re.search(r"\s#\s*\S", line[line.index(reference) + len(reference):]):
            problem(i, "`uses: %s` has no comment naming the tag its commit was read from" % reference)
        commits.setdefault(pinned.group(1), set()).add(pinned.group(2))
    for action, digests in sorted(commits.items()):
        if len(digests) > 1:
            problem(None, "%s is pinned to %d different commits (%s); one action, one commit, in every job"
                    % (action, len(digests), ", ".join(sorted(d[:12] for d in digests))))

    # 4. The environment and the secret.
    environments = [(i, _code(line).strip()) for i, line in enumerate(lines)
                    if re.match(r"^\s*environment:", _code(line))]
    if not environments:
        problem(None, "no job names an `environment:`, so the update signing key is not behind the "
                      "environment `release` and its reviewers")
    for i, code in environments:
        owner = _job_of(jobs, i)
        if owner != "publish":
            problem(i, "%r in %s: only the Publish job names the environment `release`, which holds the "
                       "signing key" % (code, "job %r" % owner if owner else "no job"))
        elif code != "environment: release":
            problem(i, "the Publish job names the environment as %r; it must be `environment: release`" % code)
    if len(environments) > 1:
        problem(environments[1][0], "the environment is named %d times; once, by the Publish job"
                % len(environments))
    secret_lines = [i for i, line in enumerate(lines) if "secrets.UPDATE_SIGNING_KEY" in _code(line)]
    for i in secret_lines:
        owner = _job_of(jobs, i)
        if owner != "publish":
            problem(i, "secrets.UPDATE_SIGNING_KEY is read in %s; only the Publish job, behind the "
                       "environment, may read it" % ("job %r" % owner if owner else "no job"))
    if len(secret_lines) > 1:
        problem(secret_lines[1], "secrets.UPDATE_SIGNING_KEY is read %d times; once, by the one step that "
                                 "signs" % len(secret_lines))
    if not secret_lines:
        problem(None, "nothing reads secrets.UPDATE_SIGNING_KEY: the Publish job cannot sign the update "
                      "manifests")
    for i, line in enumerate(lines):
        if re.match(r"^\s*secrets:\s*inherit\b", _code(line)):
            problem(i, "`secrets: inherit` hands every secret to what it calls")

    # 5. The Publish job.
    if "publish" not in jobs:
        problem(None, "the workflow has no job `publish`")
    else:
        start, end = jobs["publish"]
        body = [_code(lines[i]) for i in range(start, end)]
        needs = next((line for line in body if re.match(r"^    needs:", line)), None)
        if needs is None:
            problem(start, "the Publish job has no `needs:`; it must wait for %s" % ", ".join(PUBLISH_NEEDS))
        else:
            for name in PUBLISH_NEEDS:
                if not re.search(r"\b%s\b" % re.escape(name), needs):
                    problem(start, "the Publish job does not wait for the job %r (%s): it could publish "
                                   "packages that were never built" % (name, needs.strip()))
        if not any(line.strip() == "if: needs.version.outputs.tag != ''" for line in body):
            problem(start, "the Publish job does not run only for a tag (`if: needs.version.outputs.tag != ''`): "
                           "a run without a tag publishes nothing")
        if not any(re.match(r"^    permissions:\s*$", line) for line in body):
            problem(start, "the Publish job does not state its own `permissions:`; it is the one job that "
                           "writes (the release)")

    # 6. The configure steps.
    configure_count = 0
    i = 0
    while i < len(lines):
        code = _code(lines[i])
        if re.search(r"\bcmake\s+--preset\b", code):
            configure_count += 1
            command = code
            last = i
            while command.rstrip().endswith("\\") and last + 1 < len(lines):
                last += 1
                command += "\n" + _code(lines[last])
            for setting in CONFIGURE_SETTINGS:
                if setting not in command:
                    problem(i, "this configure does not pass %s: a release must be built with the update keys "
                               "checked, for the repository it publishes in and under the run's sequence"
                            % setting)
            i = last
        i += 1
    if configure_count < 2:
        problem(None, "the workflow has %d configure step(s) (`cmake --preset`); the Windows and the Linux and "
                      "macOS jobs have one each" % configure_count)

    # 7. Typed text in a script.
    run_indent: int | None = None
    for i, line in enumerate(lines):
        if run_indent is not None:
            if line.strip() and _indent(line) <= run_indent:
                run_indent = None
            else:
                if TYPED_EXPRESSION.search(line):
                    problem(i, "an expression of what a person typed when starting the run is in a script "
                               "(%s); pass it through `env:` and read the variable" % line.strip()[:80])
                continue
        m = re.match(r"^(\s*)(?:-\s+)?run:\s*(.*)$", line)
        if m:
            key_indent = line.index("run:")
            rest = m.group(2).strip()
            if rest[:1] in ("|", ">"):
                run_indent = key_indent
            elif TYPED_EXPRESSION.search(rest):
                problem(i, "an expression of what a person typed when starting the run is in a script (%s); "
                           "pass it through `env:` and read the variable" % rest[:80])

    # 8. What an update leaves alone.
    if "env" not in blocks:
        problem(None, "the workflow has no top-level `env:`, which holds %s" % ", ".join(UPDATE_ENV))
    else:
        start, end = blocks["env"]
        top_env = {}
        for i in range(start + 1, end):
            m = re.match(r"^  ([A-Za-z_][A-Za-z0-9_]*):\s*\"?([^\"#]*?)\"?\s*(?:#.*)?$", lines[i])
            if m:
                top_env[m.group(1)] = m.group(2)
        for name, value in UPDATE_ENV.items():
            if top_env.get(name) != value:
                problem(start, "the top-level env %s is %r; it must be %r (docs/release.md, \"Update assets\"): "
                               "an update's manifest names nothing outside bin, share, etc and lib"
                        % (name, top_env.get(name), value))
    return found


class Script:
    """One `run:` of the workflow: where it is, the shell it runs under and its text."""

    def __init__(self, job: str | None, line: int, shell: str, body: str) -> None:
        self.job = job
        self.line = line  # 0-based line of the `run:` key
        self.shell = shell  # "bash", "pwsh", or the word the workflow gave, which is not checked
        self.body = body


def scripts(text: str) -> list[Script]:
    """Every `run:` of the workflow, with the shell its step runs under (the step's `shell:`, else the job's
    `defaults: run: shell:`, else bash) and its text with each ${{ ... }} replaced by a word."""
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    jobs = _jobs(lines, _blocks(lines))
    found: list[Script] = []
    i = 0
    while i < len(lines):
        m = re.match(r"^(\s*)(?:-\s+)?run:\s*(.*)$", lines[i])
        if not m or lines[i].lstrip().startswith("#"):
            i += 1
            continue
        key_indent = lines[i].index("run:")
        rest = m.group(2).strip()
        if rest == "":  # `defaults: run:` is a mapping, and a script is never empty
            i += 1
            continue
        first, last = i, i
        if re.fullmatch(r"[|>][+-]?", rest):
            body_lines = []
            j = i + 1
            while j < len(lines) and (not lines[j].strip() or _indent(lines[j]) > key_indent):
                body_lines.append(lines[j])
                j += 1
            last = j - 1
            margin = min((_indent(line) for line in body_lines if line.strip()), default=0)
            body = "\n".join(line[margin:] for line in body_lines)
        else:
            body = rest
        # The step: from its "- " (the line itself when the step begins with `run:`) to the next step.
        start = i
        if not re.match(r"^\s*-\s+run:", lines[i]):
            while start > 0 and not (_indent(lines[start]) == key_indent - 2 and lines[start].lstrip().startswith("- ")):
                start -= 1
        end = last + 1
        while end < len(lines) and (not lines[end].strip() or _indent(lines[end]) >= key_indent):
            end += 1
        shell = None
        for k in range(start, end):
            sm = re.match(r"^\s*(?:-\s+)?shell:\s*(\S.*?)\s*$", lines[k])
            if sm and (_indent(lines[k]) == key_indent or lines[k].lstrip().startswith("- shell:")):
                shell = sm.group(1)
        job = _job_of(jobs, i)
        if shell is None and job is not None:
            job_start, job_end = jobs[job]
            defaults = next((k for k in range(job_start, job_end)
                             if lines[k].strip() == "defaults:" and _indent(lines[k]) == 4), None)
            k = defaults + 1 if defaults is not None else job_end
            while k < job_end and (not lines[k].strip() or _indent(lines[k]) > 4):
                sm = re.match(r"^\s+shell:\s*(\S.*?)\s*$", lines[k])
                if sm:
                    shell = sm.group(1)
                k += 1
        if shell is None or re.match(r"^(bash|sh|msys2)\b", shell):
            kind = "bash"
        elif re.match(r"^(pwsh|powershell)\b", shell):
            kind = "pwsh"
        else:
            kind = shell
        found.append(Script(job, first, kind, re.sub(r"\$\{\{.*?\}\}", "X", body)))
        i = last + 1
    return found


def script_problems(text: str, kind: str, interpreter: str) -> tuple[list[str], int]:
    """Hands every script of `kind` ("bash" or "pwsh") in `text` to `interpreter`'s parser, which runs nothing.
    Returns the problems and how many scripts it read; none read is itself a problem, so a workflow whose
    scripts this failed to find cannot pass."""
    import os
    import subprocess
    import tempfile

    chosen = [s for s in scripts(text) if s.shell == kind]
    found: list[str] = []
    if not chosen:
        found.append("no `run:` of the workflow runs under %s, which is not how this workflow is written; "
                     "the check found nothing to parse" % kind)
    for script in chosen:
        where = "the step at line %d (job %s)" % (script.line + 1, script.job)
        if kind == "bash":
            # The script on standard input and -n: bash reads it and runs none of it. Standard input and
            # not a file, so that a Windows path is no matter to the bash of MSYS2 or of Git.
            done = subprocess.run([interpreter, "-n"], input=script.body.encode("utf-8"),
                                  capture_output=True, check=False)
            if done.returncode != 0:
                found.append("%s does not parse under bash: %s"
                             % (where, done.stderr.decode("utf-8", "replace").strip()))
        else:
            with tempfile.TemporaryDirectory() as folder:
                path = Path(folder) / "step.ps1"
                path.write_text(script.body + "\n", encoding="utf-8")
                parse = ("$errors = $null; $tokens = $null; "
                         "[void][System.Management.Automation.Language.Parser]::ParseFile("
                         "$env:KATANA_PS_STEP, [ref]$tokens, [ref]$errors); "
                         "foreach ($e in $errors) { 'line ' + $e.Extent.StartLineNumber + ': ' + $e.Message }; "
                         "if ($errors.Count -gt 0) { exit 1 }")
                done = subprocess.run([interpreter, "-NoProfile", "-NonInteractive", "-Command", parse],
                                      capture_output=True, check=False, env={**os.environ, "KATANA_PS_STEP": str(path)})
            if done.returncode != 0:
                said = " / ".join(done.stdout.decode("utf-8", "replace").strip().split("\n")[:3])
                found.append("%s does not parse under PowerShell: %s"
                             % (where, said or done.stderr.decode("utf-8", "replace").strip()))
    return found, len(chosen)


def interpreter_for(kind: str) -> str | None:
    """The program that parses scripts of `kind`, or None when there is none that answers."""
    import shutil
    import subprocess

    for name in (("bash",) if kind == "bash" else ("pwsh", "powershell")):
        path = shutil.which(name)
        if not path:
            continue
        if kind == "bash":
            command = [path, "-c", "echo katana-ok"]
        else:
            command = [path, "-NoProfile", "-NonInteractive", "-Command", "Write-Output katana-ok"]
        try:
            done = subprocess.run(command, capture_output=True, check=False, timeout=60)
        except (OSError, subprocess.SubprocessError):
            continue
        # The bash.exe in Windows' own folder starts a Linux distribution, which may not be installed,
        # and says so instead of echoing: only a shell that answers is used.
        if done.returncode == 0 and b"katana-ok" in done.stdout:
            return path
    return None


def main(argv: list[str]) -> int:
    args = argv[1:]
    kind = None
    if args[:1] == ["--scripts"]:
        if len(args) < 2 or args[1] not in ("bash", "pwsh"):
            print("check_release_workflow: --scripts takes bash or pwsh", file=sys.stderr)
            return 2
        kind = args[1]
        args = args[2:]
    path = Path(args[0]) if args else DEFAULT_WORKFLOW
    try:
        text = path.read_bytes().decode("utf-8")
    except (OSError, UnicodeDecodeError) as error:
        print("check_release_workflow: cannot read %s: %s" % (path, error), file=sys.stderr)
        return 2
    if kind is not None:
        interpreter = interpreter_for(kind)
        if interpreter is None:
            print("check_release_workflow: no working %s on PATH, so its steps were not checked"
                  % ("bash" if kind == "bash" else "pwsh or powershell"), file=sys.stderr)
            return 2
        found, count = script_problems(text, kind, interpreter)
        if found:
            print("check_release_workflow: %s has steps that do not parse:" % path, file=sys.stderr)
            for what in found:
                print("  - " + what, file=sys.stderr)
            return 1
        print("check_release_workflow: %d %s step(s) of %s parse under %s"
              % (count, kind, path.name, Path(interpreter).name))
        return 0
    found = problems(text)
    if found:
        print("check_release_workflow: %s is not the shape docs/release.md describes:" % path, file=sys.stderr)
        for what in found:
            print("  - " + what, file=sys.stderr)
        return 1
    print("check_release_workflow: %s starts by hand only, reads with a token that cannot write except in "
          "Publish, names every action by its commit, keeps the update signing key in the environment "
          "release, and configures every build with the update keys required" % path.name)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
