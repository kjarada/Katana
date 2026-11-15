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
# qualified name or a test name that no longer exists is the cheapest sign of
# such rot, and it can be checked mechanically.
#
# Katana is a Rust project (docs/rust.md): the C++ tree - src/, include/,
# benchmarks/ and the gtest sources under tests/ - was removed on the owner's
# word of 2026-11-15. The forms a document may cite, each checked as said:
#
#   * A repository path, in backticks, in a link or in fenced code:
#     `crates/katana_cad/src/global_modify.rs`, `tests/data/...`,
#     `tools/build_visible.sh`, `docs/...`, `cmake/...`, `resources/...`,
#     `samples/...`, and the root files (CMakeLists.txt, CMakePresets.json,
#     .gitignore). The file or folder exists, or a glob matches when the path
#     has * or <placeholder>; a path under a git-ignored folder is skipped,
#     since a clean checkout does not have it. A path into the removed C++
#     tree (src/, include/, benchmarks/, a .cpp/.hpp/.h under tests/) is
#     reported as such: cite the Rust file it became (the layout mirrors it:
#     src/katana_qt/x.cpp is crates/katana_window/src/x.rs,
#     src/katana_cad/y.cpp is crates/katana_cad/src/y.rs), or write the
#     history as a sentence without the path.
#   * A Rust path rooted at a crate: `katana_cad::global_modify::match_entities`,
#     `katana_entity::Document::set_current_style`. The crate is a folder of
#     crates/; the modules are followed through the files (x.rs or
#     x/mod.rs, or an inline `mod x {`) as far as they go; the first name after
#     them is declared (fn, struct, enum, trait, type, const, static, mod,
#     macro_rules!) in that module or below it - a re-export at the crate root
#     is found that way - and any name after that (a method, a variant, an
#     associated constant) occurs in the crate's code in a file that names the
#     one before it.
#   * A shorter qualified name, `Type::method`, `Enum::Variant` or
#     `module::item` (`Document::set_current_style`,
#     `global_modify::match_entities`): its last two names both occur in the
#     code of one .rs file under crates/, a file's own module name (its stem, or
#     its folder's name for mod.rs, lib.rs and main.rs) counting as a word in
#     it. `std::...`, the crates the workspace depends on (`egui::...`,
#     `wgpu::...`) and GDAL's own C++ classes (`OGRLayer::SetAttributeFilter`,
#     `GDALDataset::ExecuteSQL`: OGR*, GDAL*, CPL*, OSR*), which a document
#     cites for what the library does and the Rust reaches through its C API
#     or the shims, are not checked.
#   * A Rust test, `suite::case_name`: the twin of the gtest case
#     Suite.CaseName (the rule of the retired parity check, kept here): a
#     #[test] function `case_name` whose innermost module is `suite` - a
#     `mod suite { ... }`, or the file suite.rs when the function is at its
#     top. A qualified name whose last name is a #[test] function anywhere,
#     and of three words or more, is held to this form. A long snake_case
#     name on its own (`five_or_more_words_like_this`) is checked as a long
#     CamelCase name is.
#   * A process test - `cli.<name>`, `qt_<name>_headless`, or any long
#     snake_case name - registered by add_test(NAME ...) in a CMakeLists.txt or
#     .cmake file of the CMake test runner (tests/, cmake/, tools/, the root
#     CMakeLists.txt); a function those files define counts as such a name.
#   * A long CamelCase name (five humps or more: a Rust type, or a name the
#     code writes, such as an IFC class) occurs in the code, its string
#     literals counting here, or is a registered process test. A gtest name,
#     `Suite.CaseName` or `qt_widgets.Suite.Case`, is reported: the C++
#     tests are gone, so cite the twin `suite::case_name`.
#
# "Occurs in the code" means outside comments, and outside string literals
# but for the long names above: the port's comments cite the C++ names they
# replaced ("main.cpp's argument loop"), and a C++ name kept alive only by
# such a comment is exactly the rot this finds.
#
# And: no citation of the removed root documents (the plan, the contributor
# instructions, the readme); over all of docs/*.md, that docs/index.md lists
# every document and names none that does not exist (EXPECTED ones excepted);
# that docs/headless.md names every switch the window's headless runner
# parses (crates/katana_window/src/drive/run.rs and steps.rs) and every
# variable tools/check_screenshot.cmake reads; and that the worked example
# docs/customisation.md prints is, character for character, the text the
# format's own test (crates/katana_entity/tests/customisation_format.rs)
# reads and writes back.
#
# Deliberately NOT checked: bare file names (`numerics.rs` - ambiguous by
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
# shrinks.
NOT_YET_CHECKED = {
    'terrain.md',
    'plotting.md',
}

# Documents docs/index.md may name before they exist (none, since all four
# that were being written landed).
EXPECTED = set()

# src, include and benchmarks are the removed C++ tree: still read as paths so
# that a citation of one is reported, never as somewhere a name may live.
ROOTS = ('src', 'include', 'tests', 'tools', 'docs', 'cmake', 'benchmarks', 'resources',
         'samples', 'third_party', 'crates', 'packaging')
REMOVED_TREE = re.compile(r'^(?:src|include|benchmarks)(?:/|$)|^tests/.*\.(?:cpp|hpp|h)$')
ROOT_FILES = ('CMakeLists.txt', 'CMakePresets.json', '.gitignore', '.clang-format',
              '.clang-tidy', 'Cargo.toml', 'Cargo.lock', 'clippy.toml')
CRATES = 'crates'
# The C++ shims under crates/*/native (GDAL's algorithm framework and
# HTTP, PDAL) are code of the Rust build too.
CODE_SUFFIXES = ('.rs', '.cpp', '.h')
# The CMake test runner: where a process test is registered.
TEST_RUNNER_ROOTS = ('tests', 'cmake', 'tools')

# The removed root documents, as a citation would name them. Split so that
# this file does not itself read as one.
FORBIDDEN = re.compile(r'\bPLAN' + r'\.MD\b|\bPLAN' + r'\.md\b|\bCLAUDE' + r'\.md\b|\bREAD'
                       + r'ME(\.md)?\b|\bplan/[0-9]')

# Not preceded by a word character, a dot, a slash or a hyphen, so that the
# src/ inside crates/katana_cad/src/... is not read as a path of its own.
PATH_TOKEN = re.compile(r'(?<![\w./-])(?:\.\./)*(?:' + '|'.join(ROOTS)
                        + r')/[^\s`\'"),;|\]]*')
ROOT_FILE_TOKEN = re.compile(r'(?<![\w/.])(?:' + '|'.join(re.escape(f) for f in ROOT_FILES)
                             + r')(?![\w/])')
QUALIFIED = re.compile(r'^[A-Za-z_]\w*(?:::[A-Za-z_]\w*)+(?:\(.*\))?(?:!)?$')
CAMEL_TEST = re.compile(r'^[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]*){4,}$')
SNAKE_TEST = re.compile(r'^[a-z][a-z0-9]*(?:_[a-z0-9]+){4,}$')
GTEST = re.compile(r'^(?:qt_widgets\.)?[A-Z][A-Za-z0-9]*\.(?:\*|[A-Z][A-Za-z0-9_]{8,})$')
THIRD_PARTY_CLASS = re.compile(r'^(?:OGR|GDAL|CPL|OSR)[A-Z]\w*$')
QT_TEST = re.compile(r'^qt_[a-z0-9_]+_headless$')
CLI_TEST = re.compile(r'^cli\.[a-z0-9_]+$')
LINK = re.compile(r'\]\(([^)\s]+)\)')
BACKTICK = re.compile(r'`([^`\n]+)`')
WORD = re.compile(r'[A-Za-z_]\w*')
DECLARATION = r'\b(?:fn|struct|enum|trait|type|const|static|mod|union|macro_rules!)\s+(?:r#)?%s\b'

# The tokens of Rust source that matter for finding code words, test
# functions and the modules around them. Order matters: a raw or byte string
# before an identifier, a character literal before a lifetime.
TOKEN = re.compile(r'''
      (?P<line_comment>//[^\n]*)
    | (?P<block_comment>/\*)
    | (?P<raw_string>(?<![\w])[bc]?r(?P<hashes>\#*)")
    | (?P<string>(?<![\w])[bc]"|")
    | (?P<char>'(?:\\(?:u\{[0-9A-Fa-f_]*\}|x[0-9A-Fa-f]{2}|.)|[^\\'\n])')
    | (?P<lifetime>'[A-Za-z_]\w*)
    | (?P<test>\#\[test\])
    | (?P<ident>(?:r\#)?[A-Za-z_]\w*)
    | (?P<open>\{)
    | (?P<close>\})
    | (?P<semicolon>;)
''', re.VERBOSE)
STRING_BODY = re.compile(r'(?:[^"\\]|\\.)*"', re.DOTALL)
COMMENT_EDGE = re.compile(r'/\*|\*/')


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


def module_name(path):
    """The module a .rs file is: its stem, or its folder's for mod/lib/main.rs."""
    stem = os.path.splitext(os.path.basename(path))[0]
    if stem in ('mod', 'lib', 'main'):
        return os.path.basename(os.path.dirname(path))
    return stem


def scan_rust(text, file_module):
    """The code of Rust source `text`: (its words outside comments and string
    literals, the words in its string literals, the (module, function) of
    every #[test] function, the code itself with comments and literals
    blanked)."""
    words = []
    literal_words = []
    tests = []
    code = []
    stack = []          # (module name, brace depth at which it closes)
    depth = 0
    pending_mod = None  # a `mod name` waiting for its `{`
    expect = None
    after_test = False  # a #[test] attribute was seen; the next fn is a test
    pos = 0
    while True:
        match = TOKEN.search(text, pos)
        if not match:
            break
        code.append(text[pos:match.start()])
        pos = match.end()
        kind = match.lastgroup
        if kind == 'hashes':
            kind = 'raw_string'
        if kind == 'line_comment' or kind == 'char' or kind == 'lifetime':
            code.append(' ')
            continue
        if kind == 'block_comment':
            level = 1
            while level:
                edge = COMMENT_EDGE.search(text, pos)
                if not edge:
                    pos = len(text)
                    break
                level += 1 if edge.group(0) == '/*' else -1
                pos = edge.end()
            code.append(' ')
            continue
        if kind == 'raw_string':
            closing = '"' + match.group('hashes')
            end = text.find(closing, pos)
            stop = len(text) if end < 0 else end
            literal_words += WORD.findall(text[pos:stop])
            pos = len(text) if end < 0 else end + len(closing)
            code.append('""')
            continue
        if kind == 'string':
            body = STRING_BODY.match(text, pos)
            stop = len(text) if not body else body.end()
            literal_words += WORD.findall(text[pos:stop])
            pos = stop
            code.append('""')
            continue
        code.append(match.group(0))
        if kind == 'test':
            after_test = True
        elif kind == 'open':
            depth += 1
            if pending_mod is not None:
                stack.append((pending_mod, depth))
                pending_mod = None
        elif kind == 'close':
            if stack and stack[-1][1] == depth:
                stack.pop()
            depth -= 1
        elif kind == 'semicolon':
            pending_mod = None  # `mod name;`: a module in a file of its own
        elif kind == 'ident':
            token = match.group(0).removeprefix('r#')
            words.append(token)
            if expect == 'mod name':
                pending_mod = token
                expect = None
            elif expect == 'fn name':
                tests.append((stack[-1][0] if stack else file_module, token))
                expect = None
            elif token == 'mod':
                expect = 'mod name'
            elif token == 'fn' and after_test:
                expect = 'fn name'
                after_test = False
    code.append(text[pos:])
    return words, literal_words, tests, ''.join(code)


class Code:
    """What the Rust tree declares, read and not built."""

    def __init__(self, root):
        self.root = root
        self.words = {}        # word -> set of files whose code has it
        self.literal_words = set()  # every word in a string literal
        self.code = {}         # file -> its code, comments and literals blanked
        self.tests = set()     # 'module::function' of every #[test] function
        self.test_names = set()
        self.crates = set()
        crates = os.path.join(root, CRATES)
        if os.path.isdir(crates):
            self.crates = {name for name in os.listdir(crates)
                           if os.path.isdir(os.path.join(crates, name))}
        # core and alloc are std's too, but katana's own `core` was a C++
        # namespace a stale citation would name: only std is skipped outright.
        self.external = {'std'} | self._dependencies()
        for directory, folders, names in os.walk(os.path.join(root, 'crates')):
            # cargo's output, generated sources among it, is not the code.
            folders[:] = [f for f in folders if f != 'target']
            for name in names:
                if not name.endswith(CODE_SUFFIXES):
                    continue
                path = os.path.join(directory, name)
                text = read(path)
                if name.endswith('.rs'):
                    module = module_name(path)
                    words, literals, tests, code = scan_rust(text, module)
                    words.append(module)
                    self.literal_words.update(literals)
                    for test in tests:
                        self.tests.add('%s::%s' % test)
                        self.test_names.add(test[1])
                else:
                    words, code = WORD.findall(text), text
                self.code[path] = code
                for word in set(words):
                    self.words.setdefault(word, set()).add(path)

    def _dependencies(self):
        """The crates the workspace depends on, by the name code uses."""
        names = set()
        for path in [os.path.join(self.root, 'Cargo.toml')] + glob.glob(
                os.path.join(self.root, 'crates', '**', 'Cargo.toml'), recursive=True):
            section = None
            for line in read(path).splitlines():
                line = line.strip()
                if line.startswith('['):
                    section = line.strip('[]')
                    continue
                if section and 'dependencies' in section:
                    name = re.match(r'([A-Za-z0-9_-]+)\s*=', line)
                    if name:
                        names.add(name.group(1).replace('-', '_'))
        return names - self.crates

    def in_one_file(self, first, second, within=None):
        files = self.words.get(first, set()) & self.words.get(second, set())
        if within:
            files = {f for f in files if f.startswith(within)}
        return files

    def declared_in(self, name, files):
        pattern = re.compile(DECLARATION % re.escape(name))
        return [f for f in files if pattern.search(self.code.get(f, ''))]

    def resolve(self, parts):
        """None when the crate-rooted path `parts` names something, else why."""
        crate = parts[0]
        source = os.path.join(self.root, CRATES, crate, 'src')
        if not os.path.isdir(source):
            return 'no crate %s in crates/' % crate
        folder = source
        candidates = [os.path.join(source, 'lib.rs'), os.path.join(source, 'main.rs')]
        current = next((c for c in candidates if os.path.exists(c)), None)
        rest = parts[1:]
        while rest:
            name = rest[0]
            as_file = os.path.join(folder, name + '.rs')
            as_folder = os.path.join(folder, name, 'mod.rs')
            if os.path.exists(as_file):
                current, folder = as_file, os.path.join(folder, name)
            elif os.path.exists(as_folder):
                current, folder = as_folder, os.path.join(folder, name)
            elif current and re.search(r'\bmod\s+%s\s*\{' % re.escape(name),
                                       self.code.get(current, '')):
                pass  # an inline module: its items are in the same file
            else:
                break
            rest = rest[1:]
        if not rest:
            return None  # the path names a module
        subtree = {f for f in self.code
                   if f == current or f.startswith(folder + os.sep)}
        if not self.declared_in(rest[0], subtree):
            # A re-export: declared anywhere in the crate, and the module the
            # path stopped at names it in its code.
            crate_files = {f for f in self.code if f.startswith(
                os.path.join(self.root, CRATES, crate) + os.sep)}
            reexported = current and re.search(r'\b%s\b' % re.escape(rest[0]),
                                               self.code.get(current, ''))
            if not (reexported and self.declared_in(rest[0], crate_files)):
                return 'nothing declares %s in %s' % (
                    rest[0], os.path.relpath(current or folder, self.root).replace(os.sep, '/'))
        within = os.path.join(self.root, CRATES, crate) + os.sep
        for owner, member in zip(rest, rest[1:]):
            if not self.in_one_file(owner, member, within):
                return 'no file of %s names both %s and %s' % (crate, owner, member)
        return None


def registered_tests(root):
    """Every name given to add_test(NAME ...) by the CMake test runner, and
    the names of its functions."""
    names = set()
    files = [os.path.join(root, 'CMakeLists.txt')]
    for top in TEST_RUNNER_ROOTS:
        for directory, _, found in os.walk(os.path.join(root, top)):
            files += [os.path.join(directory, n) for n in found
                      if n == 'CMakeLists.txt' or n.endswith('.cmake')]
    for path in files:
        if os.path.exists(path):
            text = read(path)
            names.update(re.findall(r'NAME\s+"?([A-Za-z0-9_.]+)', text))
            # A function of the runner (katana_say_what_every_program_test_starts_with)
            # is cited as a test's mechanism is.
            names.update(re.findall(r'\bfunction\(\s*([A-Za-z0-9_]+)', text))
    return names


def path_exists(root, docs_dir, token, doc_path):
    """None when the path is fine (exists, or is skipped), else a reason."""
    token = token.rstrip('.:,')
    if '...' in token or token.endswith('/..'):
        return None  # a placeholder such as crates/...
    if token.startswith('../'):
        resolved = os.path.normpath(os.path.join(os.path.dirname(doc_path), token))
        relative = os.path.relpath(resolved, root).replace(os.sep, '/')
    else:
        relative = token
    relative = relative.split('#', 1)[0].rstrip('/')
    if not relative:
        return None
    if REMOVED_TREE.search(relative):
        return 'cites the removed C++ tree: ' + token
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


def check_name(span, code, counts):
    """None when the qualified name `span` is fine, else why not."""
    name = re.sub(r'(?:\(.*\))?!?$', '', span)
    parts = name.split('::')
    if parts[0] in code.external or THIRD_PARTY_CLASS.match(parts[0]):
        return None
    last, owner = parts[-1], parts[-2]
    # A test is named by a sentence; a short name (`Style::name`) that some
    # test also happens to bear is read as the item it is.
    if last in code.test_names and last.count('_') >= 2:
        counts['tests'] += 1
        if '%s::%s' % (owner, last) not in code.tests:
            return 'no #[test] fn %s in a module %s' % (last, owner)
        return None
    counts['names'] += 1
    if parts[0] in code.crates:
        return code.resolve(parts)
    if not code.in_one_file(owner, last):
        return 'no Rust file names both %s and %s (%s)' % (owner, last, span)
    return None


def check_document(root, doc_path, ignored, code, tests, problems, counts):
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
                reason = check_name(span, code, counts)
                if reason:
                    report(number, reason)
                continue
            if GTEST.match(span):
                counts['tests'] += 1
                report(number, 'a C++ test name (%s): cite its Rust twin, suite::case_name'
                       % span)
                continue
            if QT_TEST.match(span) or CLI_TEST.match(span):
                counts['tests'] += 1
                if span not in tests:
                    report(number, 'no registered test ' + span)
                continue
            if CAMEL_TEST.match(span) or SNAKE_TEST.match(span):
                counts['tests'] += 1
                if (span not in code.words and span not in code.literal_words
                        and span not in tests):
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
    """Every switch the window's headless runner parses and every variable
    check_screenshot.cmake reads is named in docs/headless.md: a switch nobody
    can find is one nobody uses, and the switches were once spread over four
    documents with three of them in none. The switches are the match arms of
    the runner's argument loop (read_switches) and of the steps it records
    (StepKind::from_switch)."""
    headless = os.path.join(root, 'docs', 'headless.md')
    drive = os.path.join(root, CRATES, 'katana_window', 'src', 'drive')
    script = os.path.join(root, 'tools', 'check_screenshot.cmake')
    if not os.path.exists(headless):
        return
    text = read(headless)
    for name in ('run.rs', 'steps.rs'):
        source = os.path.join(drive, name)
        if not os.path.exists(source):
            problems.append('docs/headless.md: cannot read the switches: no %s'
                            % os.path.relpath(source, root).replace(os.sep, '/'))
            continue
        arms = re.findall(r'"(--[a-z0-9-]+)"\s*(?=\||=>)', read(source))
        for switch in sorted(set(arms)):
            if not re.search(re.escape(switch) + r'(?![a-z0-9-])', text):
                problems.append('docs/headless.md: does not name the switch %s '
                                '(crates/katana_window/src/drive/%s)' % (switch, name))
    if os.path.exists(script):
        source = read(script)
        variables = set(re.findall(r'DEFINED\s+([A-Z][A-Z0-9_]+)', source))
        variables |= set(re.findall(r'\bif\(([A-Z][A-Z0-9_]+)\)', source))
        for variable in sorted(variables):
            if not re.search(r'-D' + variable + r'\b', text):
                problems.append('docs/headless.md: does not name -D%s '
                                '(tools/check_screenshot.cmake)' % variable)


def check_worked_example(root, problems):
    """The example docs/customisation.md prints under "A worked example" is
    the text crates/katana_entity/tests/customisation_format.rs reads and
    writes back byte for byte. A person may copy that example to start a file
    of their own, so it must be what the writer writes; the test
    (customisation::the_worked_example_of_the_document_is_written_as_it_is_printed)
    proves that of its own copy, held in a raw string, and this holds the
    document to that copy - a claim in a document that IS checked, where the
    rest of this script only checks that what is cited exists."""
    document = os.path.join(root, 'docs', 'customisation.md')
    test = os.path.join(root, CRATES, 'katana_entity', 'tests', 'customisation_format.rs')
    if not os.path.exists(document):
        return
    lines = read(document).split('\n')
    try:
        heading = lines.index('### A worked example')
        begin = lines.index('```json', heading) + 1
        end = lines.index('```', begin)
    except ValueError:
        problems.append('docs/customisation.md: no ```json block under '
                        '"### A worked example"')
        return
    literal = 'r##"' + '\n'.join(lines[begin:end]) + '\n"##'
    if not os.path.exists(test) or literal not in read(test):
        problems.append('docs/customisation.md:%d: the worked example is not the text '
                        'crates/katana_entity/tests/customisation_format.rs holds '
                        '(the_worked_example_of_the_document_is_written_as_it_is_printed)'
                        % (begin + 1))


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
    code = Code(root)
    tests = registered_tests(root)

    problems = []
    counts = {'paths': 0, 'names': 0, 'tests': 0}
    checked = 0
    for doc in docs:
        if os.path.basename(doc) in NOT_YET_CHECKED and not everything:
            continue
        checked += 1
        check_document(root, doc, ignored, code, tests, problems, counts)
    check_index(root, docs, problems)
    check_switches(root, problems)
    check_worked_example(root, problems)

    for problem in problems:
        print(problem)
    print('check_docs: %d documents checked (%d not yet), %d paths, %d qualified names and '
          '%d test names cited, %d problems'
          % (checked, len(docs) - checked, counts['paths'], counts['names'], counts['tests'],
             len(problems)))
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
