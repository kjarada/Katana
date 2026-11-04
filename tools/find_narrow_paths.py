#!/usr/bin/env python3
# Finds every std::filesystem::path Katana makes from narrow text, or turns
# into it, by asking clang (docs/headless.md, "Text and names").
#
#   python tools/find_narrow_paths.py <build dir with compile_commands.json> [source-substring...]
#
# e.g. python tools/find_narrow_paths.py build/clang64 katana_cad/ katana_app/
# with CLANG64 first on PATH. The build directory must be a clang one (the
# local check that mirrors the Windows ARM64 job, docs/building.md): the AST
# is clang's, dumped with -fsyntax-only, so nothing is compiled or written.
#
# Why: Katana's text is UTF-8, and what narrow bytes mean to std::filesystem
# is the library's to say. libstdc++ reads and writes them as UTF-8, so the
# GCC suite never shows a slip; libc++ on Windows (the Windows ARM64 release
# job) uses the ANSI code page, so a path made from "José" names "JosÃ©", and
# path::string() gives a non-ASCII name back in that page, which GDAL and
# every other reader of UTF-8 takes for another file. core::pathFromUtf8 and
# core::pathToUtf8 are the doors (include/katana/core/path_text.hpp). The
# conversion is implicit, so no grep finds it and no compiler warns of it.
#
# What is reported, one line each, file:line:
#   NARROW   a path constructed from a std::string, string_view or char pointer
#   APPEND   path /= or += a narrow string
#   STRING   path.string() or generic_string(): narrow text out of a path
# A path made from a string literal is not reported: Katana's literals are
# ASCII, which every code page reads alike. Only declarations whose name holds
# "katana" (and, in a file with a main(), main itself) are dumped, so the
# standard library's and Qt's own headers are not searched. The exit code is 1
# when anything is reported.
import json
import re
import shlex
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

NARROW_TYPE = (
    r"(?:const )?(?:std::(?:__1::)?(?:string|basic_string<char[^,()]*>|string_view"
    r"|basic_string_view<char[^,()]*>)|char \*const|const char \*const|const char \*|char \*)"
)
PATH_TYPE = r"'std::(?:__1::)?(?:__fs::)?filesystem::path'"
CONSTRUCT = re.compile(r"CXXConstructExpr .*" + PATH_TYPE + r" 'void \(" + NARROW_TYPE +
                       r" ?&?, format\)'")
APPEND = re.compile(r"DeclRefExpr .*'operator(?:/=|\+=)' '(?:std::(?:__1::)?(?:__fs::)?"
                    r"filesystem::)?path &\(" + NARROW_TYPE + r" ?&?\)'")
STRING_CALL = re.compile(r"MemberExpr .*'<bound member function type>' \.(?:string|generic_string) ")
PATH_OBJECT = re.compile(r"'(?:const )?(?:std::(?:__1::)?(?:__fs::)?filesystem::)?path'")
# Clang prints a location's file only when it changes and its line only when
# that changes; the scan follows it the same way.
FILE_LOC = re.compile(r"<([A-Za-z]:[\\/][^:<>]+|/[^:<>]+):(\d+):\d+")
LINE_LOC = re.compile(r"<line:(\d+):\d+")
MAIN = re.compile(rb"\bint\s+w?main\s*\(")


def dump_command(entry, name):
    words = shlex.split(entry["command"], posix=False)
    kept = []
    skip = False
    for word in words:
        if skip:
            skip = False
            continue
        if word in ("-o", "-MF", "-MT", "-MQ"):
            skip = True
            continue
        if word.startswith(("-MD", "-MMD")) or word == "-c":
            continue
        kept.append(word.strip('"'))
    return kept + ["-fsyntax-only", "-Wno-everything", "-fno-color-diagnostics",
                   "-Xclang", "-ast-dump", "-Xclang", "-ast-dump-filter=" + name]


def dumps(entry):
    names = ["katana"]
    try:
        if MAIN.search(Path(entry["file"]).read_bytes()):
            names.append("main")
    except OSError:
        pass
    out = []
    for name in names:
        run = subprocess.run(dump_command(entry, name), cwd=entry["directory"], capture_output=True)
        out.append(run.stdout.decode("utf-8", "replace"))
    return out


def scan(dump, root):
    findings = set()
    lines = dump.splitlines()
    current_file = None
    current_line = None
    for i, line in enumerate(lines):
        located = FILE_LOC.search(line)
        if located:
            current_file = located.group(1).replace("\\", "/")
            current_line = int(located.group(2))
        else:
            relative = LINE_LOC.search(line)
            if relative:
                current_line = int(relative.group(1))
        kind = None
        if CONSTRUCT.search(line):
            kind = "NARROW"
        elif APPEND.search(line):
            kind = "APPEND"
        elif STRING_CALL.search(line) and i + 1 < len(lines) and PATH_OBJECT.search(lines[i + 1]):
            kind = "STRING"
        if kind is None or current_file is None:
            continue
        if not current_file.lower().startswith(root.lower()):
            continue  # a library's header
        findings.add((kind, current_file[len(root):].lstrip("/"), current_line))
    return findings


def main(argv):
    if len(argv) < 2:
        print("usage: find_narrow_paths.py <build dir with compile_commands.json> [source-substring...]",
              file=sys.stderr)
        return 2
    build = Path(argv[1]).resolve()
    filters = argv[2:]
    root = Path(__file__).resolve().parent.parent.as_posix()
    with open(build / "compile_commands.json", encoding="utf-8") as f:
        entries = [e for e in json.load(f)
                   if not filters or any(s in e["file"].replace("\\", "/") for s in filters)]
    found = set()
    with ThreadPoolExecutor(max_workers=12) as pool:
        for outputs in pool.map(dumps, entries):
            for dump in outputs:
                found |= scan(dump, root)
    for kind, file, line in sorted(found, key=lambda r: (r[1], r[2], r[0])):
        print(f"{kind:7} {file}:{line}")
    counts = {kind: sum(1 for k, _, _ in found if k == kind) for kind in ("NARROW", "APPEND", "STRING")}
    print(f"{len(entries)} sources; " + ", ".join(f"{k} {v}" for k, v in counts.items()), file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
