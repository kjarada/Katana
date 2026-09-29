#!/usr/bin/env python3
# tools/check_float_to_string.py [build-dir] [jobs]
#
# Lists every std::to_string of a floating-point value in the tree, as
# file:line, and exits 1 when there is one - or when a file could not be
# checked, since a check that did not run is not a pass. Run from the
# checkout root after a configure (it reads the build's
# compile_commands.json and runs its compiler, with -fsyntax-only);
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
# checked is listed as such, with the compiler's first error.
import json
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
HEADER = os.path.join(HERE, "float_to_string_check.hpp").replace("\\", "/")
MARK = "KATANA_TO_STRING_OF_FLOATING_POINT"


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


def main():
    build = sys.argv[1] if len(sys.argv) > 1 else "build/release"
    jobs = int(sys.argv[2]) if len(sys.argv) > 2 else os.cpu_count() or 4
    with open(os.path.join(build, "compile_commands.json"), encoding="utf-8") as f:
        entries = json.load(f)
    work, seen = [], set()
    for entry in entries:
        source = entry["file"].replace("\\", "/")
        # Only Katana's own code: not a fetched or cached third-party library.
        if source in seen or "/_deps/" in source or "/third_party/" in source:
            continue
        seen.add(source)
        with open(entry["file"], encoding="utf-8", errors="replace") as f:
            if "to_string" in f.read():
                work.append(entry)
    with ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(check, work))
    found = sorted({hit.replace("\\", "/") for hits, _ in results for hit in hits})
    unchecked = [note for _, note in results if note]
    for hit in found:
        print(hit)
    for note in unchecked:
        print(note)
    print(f"check_float_to_string: {len(work) - len(unchecked)} of {len(work)} files checked, "
          f"{len(found)} to_string of a floating-point value, {len(unchecked)} not checked")
    return 1 if found or unchecked else 0


if __name__ == "__main__":
    sys.exit(main())
