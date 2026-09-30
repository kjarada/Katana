#!/usr/bin/env python3
# tools/check_float_to_string.py [build-dir] [jobs]
#
# Lists every std::to_string of a floating-point value in the tree, as
# file:line, and exits 1 when there is one - or when a file could not be
# checked, or there was nothing to check, since a check that did not run is
# not a pass. Run from the checkout root after a configure (it reads the
# build's compile_commands.json and runs its compiler, with -fsyntax-only);
# build/release by default. The compiler must run from this shell: on
# Windows, MSYS2's bin first on PATH. Without it g++ cannot load its DLLs,
# fails printing nothing, and every file is "not checked".
#
# Why: C++26 makes std::to_string of a double the shortest text that reads
# back, as std::format("{}") does (P2587). GCC 16's library already does;
# libc++ (the clang builds: Windows ARM64, macOS) still prints "%f", six
# decimals. So such a call passes every test on GCC and says "1.650000" or
# rounds a coordinate to the micrometre on clang; on 2026-09-29 86 of them
# failed five tests there. katana::core::formatExactReal (core/text.hpp) is
# the one way to write a real as text: the same shortest form everywhere.
#
# How: a text search cannot tell a double from an int, so the compiler does.
# tools/float_to_string_check.hpp is force-included; it routes to_string
# through a template that fails to compile for a floating-point argument, a
# new instantiation per call so each call is reported, not only a file's
# first. Only files that mention to_string are checked. A file that cannot be
# checked is listed as such, with the compiler's first error. Before any of
# them, a canary - one std::to_string(1.0) - is compiled with the first
# file's own command and must be reported: a "compiler" that exits 0 and
# prints nothing, or a header that no longer trips, would otherwise check
# every file and find nothing, a pass with nothing behind it.
import json
import os
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
HEADER = os.path.join(HERE, "float_to_string_check.hpp").replace("\\", "/")
MARK = "KATANA_TO_STRING_OF_FLOATING_POINT"
# The call is on line 2, which is where the check must say it is.
CANARY = "#include <string>\nstd::string katanaCanary() { return std::to_string(1.0); }\n"


def run_check(entry, extra=""):
    # The command line as ninja gives it to the system, with its own
    # quoting; only the output and dependency-file options are taken out.
    command = entry["command"]
    command = re.sub(r" -o \S+", " ", command)
    command = re.sub(r" -M[DTF]?(?: \S+)?(?= )", " ", command)
    command = command.replace(" -c ", " ") + f' -fsyntax-only{extra} -include "{HEADER}"'
    return subprocess.run(command, cwd=entry["directory"], capture_output=True, text=True,
                          errors="replace", shell=os.name != "nt")


def check(entry):
    run = run_check(entry)
    if MARK not in run.stderr and run.returncode != 0:
        # The libraries the header reads first bring std::quoted with them,
        # which a file's own quoted() then meets; without them it may pass.
        run = run_check(entry, " -DKATANA_CHECK_NO_PREINCLUDE")
    if MARK in run.stderr:
        found = []
        for line in run.stderr.splitlines():
            # The call is the "required from here" note (GCC) or the
            # "in instantiation of" note (clang).
            m = re.search(r"^(.+?):(\d+):\d+:.*(required from here|in instantiation of function)",
                          line)
            if m and "float_to_string_check" not in m.group(1):
                found.append(f"{m.group(1)}:{m.group(2)}")
        return found, None
    if run.returncode != 0:
        errors = [line for line in run.stderr.splitlines() if "error" in line]
        reason = (errors or [run.stderr.strip()])[0][:300]
        # A compiler that could not start says nothing at all.
        reason = reason or f"the compiler exited {run.returncode} and printed nothing"
        return [], f"{entry['file']}: not checked: {reason}"
    return [], None


def canary_fires(entry):
    """None when the canary, compiled with `entry`'s command, is reported at
    its call; otherwise why the check cannot fire."""
    with tempfile.TemporaryDirectory() as folder:
        path = os.path.join(folder, "canary.cpp").replace("\\", "/")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(CANARY)
        command, count = re.subn(r' -c (?:"[^"]*"|\S+)', lambda _: f' -c "{path}"',
                                 entry["command"])
        if count != 1:
            return f"{entry['file']}'s command does not name one source after -c"
        hits, note = check(dict(entry, command=command, file=path))
        if any(hit.replace("\\", "/").endswith("canary.cpp:2") for hit in hits):
            return None
        return note or (f"a std::to_string(1.0) compiled with {entry['file']}'s command was not "
                        "reported")


def main():
    build = sys.argv[1] if len(sys.argv) > 1 else "build/release"
    jobs = int(sys.argv[2]) if len(sys.argv) > 2 else os.cpu_count() or 4
    with open(os.path.join(build, "compile_commands.json"), encoding="utf-8") as f:
        entries = json.load(f)
    work, seen, unreadable = [], set(), []
    for entry in entries:
        source = entry["file"].replace("\\", "/")
        # Only Katana's own code: not a fetched or cached third-party library.
        if source in seen or "/_deps/" in source or "/third_party/" in source:
            continue
        seen.add(source)
        try:
            with open(entry["file"], encoding="utf-8", errors="replace") as f:
                text = f.read()
        except OSError as error:
            # A source the database names and this tree lacks: a build
            # directory configured from another checkout. Not checked, said
            # as such, rather than a traceback.
            unreadable.append(f"{entry['file']}: not checked: {error.strerror}")
            continue
        if "to_string" in text:
            work.append(entry)
    if not work and not unreadable:
        # Some 320 of the tree's own sources mention to_string, so a database
        # that gives none - empty, or not this tree's - checked nothing, and a
        # check that did not run is not a pass.
        print(f"check_float_to_string: nothing to check: "
              f"{os.path.join(build, 'compile_commands.json')} lists {len(seen)} of Katana's own "
              f"sources and none mentions to_string; a check that checked nothing is not a pass")
        return 1
    if work:
        cannot = canary_fires(work[0])
        if cannot:
            print(f"check_float_to_string: the check cannot fire: {cannot}; a check that cannot "
                  f"find a to_string of a double finds none anywhere, and that is not a pass")
            return 1
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(check, work))
    found = sorted({hit.replace("\\", "/") for hits, _ in results for hit in hits})
    unchecked = unreadable + [note for _, note in results if note]
    total = len(work) + len(unreadable)
    for hit in found:
        print(hit)
    for note in unchecked:
        print(note)
    print(f"check_float_to_string: {total - len(unchecked)} of {total} files checked, "
          f"{len(found)} to_string of a floating-point value, {len(unchecked)} not checked"
          f"{'; the canary to_string(1.0) was reported' if work else ''}")
    return 1 if found or unchecked else 0


if __name__ == "__main__":
    sys.exit(main())
