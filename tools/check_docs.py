#!/usr/bin/env python3
# tools/check_docs.py [--all] [checkout-root]
#
# The `docs` test: a document in docs/ that cites something must cite
# something that exists. Run from the checkout root, or give the root; it
# prints every broken reference as "file:line: what" and exits 1 when there
# is one. --all checks the NOT_YET_CHECKED documents too, to list what
# stands between each and the check. See docs/testing.md, "The docs test".
#
# Why a test and not a habit: documents trail the code. Of 150 code commits
# measured on 2026-09-24, 12 touched a document, and the newest commit then
# had left three passages describing a dialog it had just changed. A path, a
# Class::member or a test name that no longer exists is the cheapest sign of
# such rot, and it can be checked mechanically.
#
# What is checked, in every top-level docs/*.md not in NOT_YET_CHECKED:
#   * repository paths (src/..., include/..., tests/..., tools/..., docs/...,
#     cmake/..., benchmarks/..., resources/..., samples/..., and the root
#     build files) in backticks, in links and in fenced code: the file or
#     folder exists, or a glob matches when the path has * or <placeholder>;
#     a path under a git-ignored folder is skipped, since a clean checkout
#     does not have it;
#   * backticked qualified names (`Document::setCurrentStyle`): the last two
#     names both occur in one file under include/, src/, tests/ or
#     benchmarks/ - a renamed or deleted member leaves no such file;
#   * test names: `qt_..._headless`, `qt_widgets.Suite.Case` and `cli....`
#     are registered in a CMakeLists.txt or a TEST(), and a long backticked
#     CamelCase name (a test case, by this project's naming rule) occurs in
#     the code;
#   * no citation of the removed root documents (the plan, the contributor
#     instructions, the readme);
# and, over all of docs/*.md, that docs/index.md lists every document and
# names none that does not exist (EXPECTED ones excepted); and that
# docs/headless.md names every switch src/katana_qt/main.cpp parses and every
# variable tools/check_screenshot.cmake reads.
#
# Deliberately NOT checked: bare file names (`numerics.hpp` - ambiguous by
# design, and the path beside them usually says which), prose, and whether
# a claim is TRUE. The last is a person's job; this only finds the claims
# that can no longer be true.

import fnmatch
import glob
import io
import os
import re
import sys

# Documents still being brought up to the standard this checks. Each name is
# removed from here in the commit that makes its document pass; the list only
# shrinks. The last four are being written as this list is made.
NOT_YET_CHECKED = {
    'render.md',
    'performance.md',
    'terrain.md',
    'survey.md',
    'survey_coding.md',
    'plan_view.md',
    'plotting.md',
    'gpu.md',
    'dxf.md',
}

# Documents docs/index.md may name before they exist.
EXPECTED = {'plan_view.md', 'plotting.md', 'gpu.md', 'dxf.md'}

ROOTS = ('src', 'include', 'tests', 'tools', 'docs', 'cmake', 'benchmarks', 'resources',
         'samples', 'third_party')
ROOT_FILES = ('CMakeLists.txt', 'CMakePresets.json', '.gitignore', '.clang-format',
              '.clang-tidy')
CODE_ROOTS = ('include', 'src', 'tests', 'benchmarks')
CODE_SUFFIXES = ('.hpp', '.cpp', '.h', '.inl')

# The removed root documents, as a citation would name them. Split so that
# this file does not itself read as one.
FORBIDDEN = re.compile(r'\bPLAN' + r'\.MD\b|\bPLAN' + r'\.md\b|\bCLAUDE' + r'\.md\b|\bREAD'
                       + r'ME(\.md)?\b|\bplan/[0-9]')

# Not preceded by a word character, a dot, a slash or a hyphen, so that the
# src/ inside build/release/src/... is not read as a path of its own.
PATH_TOKEN = re.compile(r'(?<![\w./-])(?:\.\./)*(?:' + '|'.join(ROOTS)
                        + r')/[^\s`\'"),;|\]]*')
ROOT_FILE_TOKEN = re.compile(r'(?<![\w/.])(?:' + '|'.join(re.escape(f) for f in ROOT_FILES)
                             + r')(?![\w/])')
QUALIFIED = re.compile(r'^~?[A-Za-z_]\w*(?:::~?[A-Za-z_]\w*)+(?:\(.*\))?$')
CAMEL_TEST = re.compile(r'^[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]*){4,}$')
QT_TEST = re.compile(r'^qt_[a-z0-9_]+$')
QT_WIDGETS = re.compile(r'^qt_widgets\.([A-Za-z0-9_]+)\.(\*|[A-Za-z0-9_]+)$')
CLI_TEST = re.compile(r'^cli\.[a-z0-9_]+$')
LINK = re.compile(r'\]\(([^)\s]+)\)')
BACKTICK = re.compile(r'`([^`\n]+)`')
WORD = re.compile(r'[A-Za-z_]\w*')


def read(path):
    with io.open(path, encoding='utf-8', errors='replace') as handle:
        return handle.read()


def ignored_patterns(root):
    """The .gitignore entries as fnmatch patterns over repository paths."""
    patterns = []
    path = os.path.join(root, '.gitignore')
    if not os.path.exists(path):
        return patterns
    for line in read(path).splitlines():
        line = line.strip()
        if not line or line.startswith('#') or line.startswith('!'):
            continue
        line = line.lstrip('/')
        if line.endswith('/'):
            patterns.append(line + '*')
            patterns.append(line.rstrip('/'))
        else:
            patterns.append(line)
            patterns.append('*/' + line)
    return patterns


def is_ignored(path, patterns):
    return any(fnmatch.fnmatch(path, pattern) or fnmatch.fnmatch(path + '/', pattern)
               for pattern in patterns)


def code_index(root):
    """word -> set of files, and every file's words, over the code roots."""
    files_by_word = {}
    for top in CODE_ROOTS:
        for directory, _, names in os.walk(os.path.join(root, top)):
            for name in names:
                if not name.endswith(CODE_SUFFIXES):
                    continue
                path = os.path.join(directory, name)
                for word in set(WORD.findall(read(path))):
                    files_by_word.setdefault(word, set()).add(path)
    return files_by_word


def registered_tests(root):
    """Every name given to add_test(NAME ...) and every TEST() suite and case."""
    names = set()
    suites = {}
    for top in ('tests', 'src', 'benchmarks'):
        for directory, _, files in os.walk(os.path.join(root, top)):
            for name in files:
                path = os.path.join(directory, name)
                if name == 'CMakeLists.txt' or name.endswith('.cmake'):
                    names.update(re.findall(r'NAME\s+"?([A-Za-z0-9_.]+)', read(path)))
                elif name.endswith('.cpp'):
                    for suite, case in re.findall(
                            r'\bTEST(?:_F|_P)?\s*\(\s*([A-Za-z0-9_]+)\s*,\s*([A-Za-z0-9_]+)',
                            read(path)):
                        suites.setdefault(suite, set()).add(case)
    return names, suites


def path_exists(root, docs_dir, token, doc_path):
    """None when the path is fine (exists, or is skipped), else a reason."""
    token = token.rstrip('.:,')
    if '...' in token or token.endswith('/..'):
        return None  # a placeholder such as src/...
    if token.startswith('../'):
        resolved = os.path.normpath(os.path.join(os.path.dirname(doc_path), token))
        relative = os.path.relpath(resolved, root).replace(os.sep, '/')
    else:
        relative = token
    relative = relative.split('#', 1)[0].rstrip('/')
    if not relative:
        return None
    if any(c in relative for c in '*<>{}'):
        pattern = re.sub(r'<[^>]*>', '*', relative)
        if '{' in pattern:
            pattern = re.sub(r'\{[^}]*\}', '*', pattern)
        if glob.glob(os.path.join(root, pattern)):
            return None
        return 'nothing matches ' + token
    if os.path.exists(os.path.join(root, relative)):
        return None
    return 'no such path ' + token


def fenced_blocks(lines):
    """For each line, whether it is inside a ``` block."""
    inside = False
    flags = []
    for line in lines:
        if line.lstrip().startswith('```'):
            flags.append(True)
            inside = not inside
            continue
        flags.append(inside)
    return flags


def check_document(root, doc_path, ignored, words, tests, suites, problems, counts):
    relative_doc = os.path.relpath(doc_path, root).replace(os.sep, '/')
    lines = read(doc_path).split('\n')
    in_code = fenced_blocks(lines)
    docs_dir = os.path.dirname(doc_path)

    def report(number, what):
        problems.append('%s:%d: %s' % (relative_doc, number, what))

    for number, line in enumerate(lines, 1):
        if FORBIDDEN.search(line):
            report(number, 'cites a removed root document: ' + FORBIDDEN.search(line).group(0))

        spans = BACKTICK.findall(line)
        candidates = list(spans)
        candidates += LINK.findall(line)
        if in_code[number - 1]:
            candidates.append(line)

        seen = set()
        for text in candidates:
            for token in PATH_TOKEN.findall(text) + ROOT_FILE_TOKEN.findall(text):
                clean = token.rstrip('.:,')
                if clean in seen:
                    continue
                seen.add(clean)
                bare = clean
                while bare.startswith('../'):
                    bare = bare[3:]
                if is_ignored(bare.rstrip('/'), ignored):
                    continue
                counts['paths'] += 1
                reason = path_exists(root, docs_dir, clean, doc_path)
                if reason:
                    report(number, reason)

        for span in spans:
            span = span.strip()
            if QUALIFIED.match(span):
                name = re.sub(r'\(.*\)$', '', span)
                parts = [p.lstrip('~') for p in name.split('::')]
                if parts[0] == 'std':
                    continue
                last, owner = parts[-1], parts[-2]
                counts['names'] += 1
                if not (words.get(last, set()) & words.get(owner, set())):
                    report(number, 'no file names both %s and %s (%s)' % (owner, last, span))
                continue
            widgets = QT_WIDGETS.match(span)
            if widgets:
                suite, case = widgets.groups()
                counts['tests'] += 1
                if suite not in suites:
                    report(number, 'no widget test suite ' + suite)
                elif case != '*' and case not in suites[suite]:
                    report(number, 'no widget test %s.%s' % (suite, case))
                continue
            if QT_TEST.match(span) and span.endswith('_headless') or CLI_TEST.match(span):
                counts['tests'] += 1
                if span not in tests:
                    report(number, 'no registered test ' + span)
                continue
            if CAMEL_TEST.match(span):
                counts['tests'] += 1
                if span not in words:
                    report(number, 'no test or name %s in the code' % span)


def check_index(root, docs, problems):
    index_path = os.path.join(root, 'docs', 'index.md')
    if not os.path.exists(index_path):
        problems.append('docs/index.md: missing')
        return
    text = read(index_path)
    named = set(re.findall(r'\]\(([A-Za-z0-9_./-]+\.md)\)', text))
    present = {os.path.basename(d) for d in docs}
    for name in sorted(present - {'index.md'}):
        if name not in named:
            problems.append('docs/index.md: does not list %s' % name)
    for name in sorted(named):
        if '/' in name:
            if not os.path.exists(os.path.join(root, 'docs', name)):
                problems.append('docs/index.md: lists %s, which does not exist' % name)
        elif name not in present and name not in EXPECTED:
            problems.append('docs/index.md: lists %s, which does not exist' % name)


def check_switches(root, problems):
    """Every switch katana parses and every variable check_screenshot.cmake
    reads is named in docs/headless.md: a switch nobody can find is one
    nobody uses, and the switches were once spread over four documents with
    three of them in none."""
    headless = os.path.join(root, 'docs', 'headless.md')
    main_cpp = os.path.join(root, 'src', 'katana_qt', 'main.cpp')
    script = os.path.join(root, 'tools', 'check_screenshot.cmake')
    if not os.path.exists(headless):
        return
    text = read(headless)
    if os.path.exists(main_cpp):
        for switch in sorted(set(re.findall(r'argument == "(--[a-z0-9-]+)"', read(main_cpp)))):
            if not re.search(re.escape(switch) + r'(?![a-z0-9-])', text):
                problems.append('docs/headless.md: does not name the switch %s '
                                '(src/katana_qt/main.cpp)' % switch)
    if os.path.exists(script):
        source = read(script)
        variables = set(re.findall(r'DEFINED\s+([A-Z][A-Z0-9_]+)', source))
        variables |= set(re.findall(r'\bif\(([A-Z][A-Z0-9_]+)\)', source))
        for variable in sorted(variables):
            if not re.search(r'-D' + variable + r'\b', text):
                problems.append('docs/headless.md: does not name -D%s '
                                '(tools/check_screenshot.cmake)' % variable)


def main():
    arguments = sys.argv[1:]
    everything = '--all' in arguments
    arguments = [a for a in arguments if a != '--all']
    root = os.path.abspath(arguments[0] if arguments else os.getcwd())
    docs = sorted(glob.glob(os.path.join(root, 'docs', '*.md')))
    if not docs:
        print('check_docs: no docs/*.md under %s' % root)
        return 1
    ignored = ignored_patterns(root)
    words = code_index(root)
    tests, suites = registered_tests(root)

    problems = []
    counts = {'paths': 0, 'names': 0, 'tests': 0}
    checked = 0
    for doc in docs:
        if os.path.basename(doc) in NOT_YET_CHECKED and not everything:
            continue
        checked += 1
        check_document(root, doc, ignored, words, tests, suites, problems, counts)
    check_index(root, docs, problems)
    check_switches(root, problems)

    for problem in problems:
        print(problem)
    print('check_docs: %d documents checked (%d not yet), %d paths, %d qualified names and '
          '%d test names cited, %d problems'
          % (checked, len(docs) - checked, counts['paths'], counts['names'], counts['tests'],
             len(problems)))
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
