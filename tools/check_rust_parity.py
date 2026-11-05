#!/usr/bin/env python3
# tools/check_rust_parity.py [--list] [checkout-root]
#
# The `rust_parity` test: every C++ test case of a ported layer has its Rust
# twin (docs/rust.md, "Tests are the specification").
#
# The C++ cases are the TEST, TEST_F and TEST_P macros under tests/<dir>/; a
# case `Suite.CaseName` has its twin in a Rust test function `case_name` whose
# innermost enclosing module is `suite` - the gtest names in snake case, so
# `CoreText.TrimNeverCutsAUtf8SequenceWhateverTheLocale` is
# `core_text::trim_never_cuts_a_utf8_sequence_whatever_the_locale`. A test
# function at the top of a file under tests/ has the file's stem as its module.
# Twins are looked for in every .rs file of rust/crates, so a case may be
# ported into whichever crate holds what it tests.
#
# rust/parity.toml says which test directories are ported - only those are
# held to it - and lists the exemptions: a C++ case with no Rust twin and the
# reason (a misuse the type system now refuses to compile, a reader the owner
# chose not to port). An exemption must name a case that exists, so that one
# left behind by a deleted test is found.
#
# The Rust is read, not built: comments, string and character literals are
# stepped over, so a brace inside a test's text does not move a function into
# another module. Prints what is missing as "<Suite.Case> has no Rust twin
# (<module::function>)" and exits 1 when anything is. Needs Python 3.11 or
# later (tomllib).

import argparse
import os
import re
import sys
import tomllib

TEST_MACRO = re.compile(r'\bTEST(?:_F|_P)?\s*\(\s*([A-Za-z_]\w*)\s*,\s*([A-Za-z_]\w*)\s*\)')
# Directories under tests/ that hold no gtest cases of a layer.
NOT_LAYERS = {'support', 'data', 'layering_fixtures', 'rust_check_fixtures', 'simd_kernel_fixtures',
              'link_closure_fixtures', 'program_test_fixtures', 'runtime_deployment'}


def snake(name):
    """CamelCase to snake_case: CLocale -> c_locale, Utf8Sequence -> utf8_sequence."""
    name = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1_\2', name)
    name = re.sub(r'([a-z0-9])([A-Z])', r'\1_\2', name)
    return name.lower()


def cpp_cases(root):
    """test directory -> sorted list of 'Suite.Case'."""
    cases = {}
    tests = os.path.join(root, 'tests')
    for directory in sorted(os.listdir(tests)):
        top = os.path.join(tests, directory)
        if directory in NOT_LAYERS or not os.path.isdir(top):
            continue
        found = set()
        for folder, _, names in os.walk(top):
            for name in names:
                if name.endswith('.cpp'):
                    with open(os.path.join(folder, name), encoding='utf-8', errors='replace') as handle:
                        found.update(f'{s}.{c}' for s, c in TEST_MACRO.findall(handle.read()))
        if found:
            cases[directory] = sorted(found)
    return cases


# The tokens that matter for finding test functions and the modules around
# them; everything between them is skipped by the search. Order matters: a raw
# or byte string before an identifier, a character literal before a lifetime.
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


def rust_tests(text, file_module):
    """The (module, function) of every #[test] function in Rust source `text`."""
    found = []
    stack = []          # (module name, brace depth at which it closes)
    depth = 0
    pending_mod = None  # a `mod name` waiting for its `{`
    expect = None       # 'mod name' or 'fn name' after `mod`, or `fn` of a test
    after_test = False  # a #[test] attribute was seen; the next fn is a test
    pos = 0
    while True:
        match = TOKEN.search(text, pos)
        if not match:
            break
        pos = match.end()
        kind = match.lastgroup
        if kind == 'hashes':
            kind = 'raw_string'
        if kind == 'line_comment' or kind == 'char' or kind == 'lifetime':
            continue
        if kind == 'block_comment':
            level = 1
            while level:
                edge = COMMENT_EDGE.search(text, pos)
                if not edge:
                    return found
                level += 1 if edge.group(0) == '/*' else -1
                pos = edge.end()
            continue
        if kind == 'raw_string':
            closing = '"' + match.group('hashes')
            end = text.find(closing, pos)
            pos = len(text) if end < 0 else end + len(closing)
            continue
        if kind == 'string':
            body = STRING_BODY.match(text, pos)
            pos = len(text) if not body else body.end()
            continue
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
            if expect == 'mod name':
                pending_mod = token
                expect = None
            elif expect == 'fn name':
                found.append((stack[-1][0] if stack else file_module, token))
                expect = None
            elif token == 'mod':
                expect = 'mod name'
            elif token == 'fn' and after_test:
                expect = 'fn name'
                after_test = False
    return found


def rust_twins(root):
    twins = set()
    crates = os.path.join(root, 'rust', 'crates')
    for folder, _, names in os.walk(crates):
        for name in names:
            if name.endswith('.rs'):
                path = os.path.join(folder, name)
                with open(path, encoding='utf-8', errors='replace') as handle:
                    stem = os.path.splitext(name)[0]
                    twins.update(f'{m}::{f}' for m, f in rust_tests(handle.read(), stem))
    return twins


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', nargs='?', default='.')
    parser.add_argument('--list', action='store_true', help='print the count of each test directory')
    args = parser.parse_args()
    root = os.path.abspath(args.root)

    with open(os.path.join(root, 'rust', 'parity.toml'), 'rb') as handle:
        settings = tomllib.load(handle)
    ported = settings.get('ported', [])
    exempt = settings.get('exempt', {})

    cases = cpp_cases(root)
    twins = rust_twins(root)
    every_case = {case for listed in cases.values() for case in listed}
    problems = []
    for directory in ported:
        if directory not in cases:
            problems.append(f"rust/parity.toml: '{directory}' is not a test directory with cases")
    for case in sorted(exempt):
        if case not in every_case:
            problems.append(f'rust/parity.toml: the exemption names no C++ test: {case}')

    total = len(every_case)
    twinned = 0
    for directory, listed in sorted(cases.items()):
        have = 0
        for case in listed:
            suite, name = case.split('.')
            twin = f'{snake(suite)}::{snake(name)}'
            if twin in twins or case in exempt:
                have += 1
            elif directory in ported:
                problems.append(f'{case} has no Rust twin ({twin}) [tests/{directory}]')
        twinned += have
        if args.list:
            mark = 'ported' if directory in ported else '      '
            print(f'  {mark} tests/{directory}: {have} of {len(listed)}')

    print(f'Rust parity: {twinned} of {total} C++ test cases have a Rust twin or an exemption; '
          f'ported directories: {", ".join(ported) if ported else "none"}.')
    if problems:
        for problem in problems:
            print('  ' + problem)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
