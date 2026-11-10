#!/usr/bin/env python3
# tools/rust_cli_gate.py --build <C++ build tree> [options] [-- ctest arguments]
#
# The gate of the Rust port's step 6 (docs/rust.md; the step 6 plan, section
# 4.6): every cli.* test of the C++ build runs against the Rust katana_cli and
# katana_mcp, through a rewritten copy of the C++ build's own CTest files, and
# each test's output is compared with what the C++ programs printed.
#
# RUN BOOK (the integrator's)
#
#   After the C++ suite has run in the same build tree - never beside it: the
#   cli tests write into folders of that tree - in WSL, as build_visible.sh's
#   `wsl` phase lays the trees out ($HOME/katana-build/<worktree>/linux-release
#   for the C++, $HOME/katana-build/<worktree>/cargo for cargo):
#
#     python3 tools/rust_cli_gate.py \
#         --build ~/katana-build/rust-port/linux-release \
#         --programs ~/katana-build/rust-port/cargo/release
#
#   under `xvfb-run -a`, as the suite runs, since a fixture the gate carries
#   may run the C++ window (below). It
#     1. reads the build's CTest files from <build>/CTestTestfile.cmake down
#        through their subdirs(), as ctest does - today the cli tests are in
#        <build>/src/katana_app/CTestTestfile.cmake and
#        <build>/tests/CTestTestfile.cmake, and one added elsewhere is found
#        too - and takes every add_test and set_tests_properties of a cli.*
#        test, quoted and bracket arguments alike, with its fixtures,
#        environment, working directory and pass expressions;
#     2. replaces the two programs' paths, <cpp-bin>/katana_cli and
#        <cpp-bin>/katana_mcp, wherever they appear in those commands (inside
#        -DCLI= and -DMCP= too), with <programs>/katana_cli and katana_mcp;
#     3. writes <build>/rust_cli_gate/CTestTestfile.cmake and runs
#        `ctest --test-dir <build>/rust_cli_gate --parallel 14
#        --output-on-failure` on it, with any arguments given after `--`
#        (`-- -R survey` runs those alone, and compares those alone);
#     4. reads that run's Testing/Temporary/LastTest.log and the C++ suite's
#        (<build>/Testing/Temporary/LastTest.log, or --cpp-log), and compares
#        each cli test's output with only the normalisations below.
#   It exits 0 when ctest passed every test and every output is the C++'s;
#   1 when a test failed or an output differs, each listed by test; 2 when it
#   refuses to run (below) or is called wrongly.
#
#   The C++ suite's LastTest.log holds what its LAST ctest ran: a later `test
#   -R ...` leaves the cli tests out of it, and a cli test with no C++ output
#   to compare is a failure ("not compared"). --rerun-cpp runs the C++ cli
#   tests again first, unchanged, in <build>/rust_cli_gate/cpp, and compares
#   with that run; it costs one more run of the cli tests and needs no full
#   suite before it.
#
#   Other options: --check reads and checks the rewrite, prints what it would
#   run, and writes and runs nothing (the self-check's mode, below);
#   --compare-only compares the two logs as they are, running nothing;
#   --cpp-bin names the C++ programs' folder as the CTest files spell it
#   (default CMAKE_CACHEFILE_DIR of <build>/CMakeCache.txt, then "/bin");
#   --programs the Rust programs' folder (default $CARGO_TARGET_DIR/release,
#   else <build>/../cargo/release); --output the gate's folder (default
#   <build>/rust_cli_gate); --ctest the ctest to run; --parallel ctest's -j.
#
# WHAT IT REFUSES (exit 2, every refusal listed, nothing written or run)
#
#   1. A cli test that runs a program the gate did not replace: a word of its
#      command, or of its ENVIRONMENT or ENVIRONMENT_MODIFICATION, that names
#      katana_cli or katana_mcp as a path's last part (or alone, found on PATH)
#      where no <cpp-bin>/ path was replaced - "a test that ran a program had
#      nothing replaced". Such a test would run a C++ program, or none, under
#      the Rust's name.
#   2. A cli test that names any other program of the C++ build,
#      <cpp-bin>/<name>: the gate swaps two programs, and a third would run
#      the C++ under the gate's verdict.
#   3. A count of cli tests written that differs from the count read: the
#      rewritten file is read back by the same reader and its cli tests
#      counted, so a test lost by the rewrite, or a name defined twice (which
#      one file cannot hold), stops the gate rather than shrinking it.
#   A cli test that names no program at all (a fixture step: `cmake -E rm`,
#   make_directory, a state file written) is taken as it is and counted.
#
# WHAT IT CARRIES AS THE C++ SUITE RUNS IT
#
#   A fixture a cli test requires (FIXTURES_REQUIRED) whose setup or cleanup
#   is not a cli test - on 2026-11-10 one: view_scale_saved, made by the C++
#   window's headless check (qt_paper_style_scale_dialog_uses_the_shared_
#   command_headless) and removed by qt_view_scale_project_cleanup - is
#   written with its providers, unchanged and named in the run's output. The
#   cli test then reads what the C++ window made in both runs, as the C++
#   suite has it; left out, ctest would run the test without its fixture.
#   Each test the rewrite moves into one file keeps the directory ctest ran
#   it in: a test with no WORKING_DIRECTORY is given its file's folder.
#
# THE NORMALISATIONS (anything else that differs fails the gate; one added
# later is added here with its reason)
#
#   1. The programs' own paths: <cpp-bin>/katana_cli and <programs>/katana_cli
#      (and katana_mcp) read as <katana_cli> (<katana_mcp>) in both outputs.
#      Why: the two programs are found at two paths, and a script that names
#      the program it ran (text_cli_check.cmake on a failure) prints it.
#   2. seconds=<d>.<ddd> reads as seconds=<s>. Why: it is the wall-clock time
#      of a GDAL step (the geo and GIS records), different on every run.
#   3. katana-scratch/<pid>-<ns> reads as katana-scratch/<process>. Why: the
#      geo executor's scratch folder is named by the process's id and its start
#      time (geo_verbs, own_scratch); the /<n> below it is compared.
#   4. The two streams are compared apart: the lines that begin "error: ",
#      "warning: " or "note: " - what the programs write to their standard
#      error, the console's `err` - in their order, and every other line in
#      its order. Why: ctest hands a test one pipe for both streams; the C++
#      program's std::cout is block-buffered on a pipe and its std::cerr is
#      not, so its error lines come out wherever its buffer happened to be
#      flushed, while the Rust console flushes every write. Each stream's text
#      is compared whole; only where one stream's lines fall between the
#      other's is not. Limit: a C++ standard output of more than a buffer
#      (4 KiB) before an error can be cut mid-line by the error line, which
#      then shows as a difference.
#
# ITS OWN CHECK
#
#   tests/rust_check_fixtures/rust_cli_gate/ holds small synthetic build trees
#   of CTest files, one per refusal (unreplaced, other_program, count_differs)
#   and one the gate must accept (accepted: quoted and bracket arguments, a
#   script given -DCLI= and -DMCP=, a step that runs no program, a fixture
#   whose setup is not a cli test, a test that is not a cli test naming another
#   program). Each is run with --check --cpp-bin /katana-build/bin --programs
#   /katana-build/cargo/release and its output matched, as rust_layering's
#   fixtures are; a check that refused everything would pass the three
#   refusals, which is why the fourth must pass.

import argparse
import difflib
import os
import re
import subprocess
import sys

# ---- the CMake language, as far as a generated CTestTestfile.cmake uses it ------------

IDENTIFIER = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
BRACKET_OPEN = re.compile(r'\[(=*)\[')


class CMakeError(Exception):
    pass


class Argument:
    """One argument of a command: its text as written and its value."""

    def __init__(self, raw, value, quoted):
        self.raw = raw
        self.value = value
        self.quoted = quoted


class Command:
    def __init__(self, name, arguments, line):
        self.name = name
        self.arguments = arguments
        self.line = line


def unescape(character):
    # CMake's escape sequences: \t \r \n, \; which encodes itself (a list's
    # separator kept whole), and \ before any other character that is not a
    # letter or digit, which is that character. A letter or digit after \ is
    # no escape CMake has; it is kept as written.
    if character == 't':
        return '\t'
    if character == 'r':
        return '\r'
    if character == 'n':
        return '\n'
    if character == ';' or character.isalnum():
        return '\\' + character
    return character


def line_of(text, offset):
    return text.count('\n', 0, offset) + 1


def bracket_end(text, at, equals, what):
    close = ']' + equals + ']'
    end = text.find(close, at)
    if end < 0:
        raise CMakeError(f'line {line_of(text, at)}: {what} is never closed')
    return end, end + len(close)


def skip_comment(text, at):
    """`at` is a '#': the offset after the comment."""
    bracket = BRACKET_OPEN.match(text, at + 1)
    if bracket:
        return bracket_end(text, bracket.end(), bracket.group(1), 'a bracket comment')[1]
    end = text.find('\n', at)
    return len(text) if end < 0 else end + 1


def read_arguments(text, at):
    """The arguments from just after a command's '(' to its ')': (arguments, offset after)."""
    arguments = []
    depth = 1
    while True:
        if at >= len(text):
            raise CMakeError('a command is never closed')
        character = text[at]
        if character in ' \t\r\n':
            at += 1
        elif character == '#':
            at = skip_comment(text, at)
        elif character == '(':
            depth += 1
            arguments.append(Argument('(', '(', False))
            at += 1
        elif character == ')':
            depth -= 1
            at += 1
            if depth == 0:
                return arguments, at
            arguments.append(Argument(')', ')', False))
        elif character == '"':
            start = at
            at += 1
            value = []
            while True:
                if at >= len(text):
                    raise CMakeError(f'line {line_of(text, start)}: a quoted argument is never closed')
                character = text[at]
                if character == '"':
                    at += 1
                    break
                if character == '\\' and at + 1 < len(text):
                    following = text[at + 1]
                    if following == '\n':
                        at += 2  # a line continuation
                    elif following == '\r' and text.startswith('\n', at + 2):
                        at += 3
                    else:
                        value.append(unescape(following))
                        at += 2
                    continue
                value.append(character)
                at += 1
            arguments.append(Argument(text[start:at], ''.join(value), True))
        elif BRACKET_OPEN.match(text, at):
            opening = BRACKET_OPEN.match(text, at)
            content, end = bracket_end(text, opening.end(), opening.group(1), 'a bracket argument')
            value = text[opening.end():content]
            # A line end straight after the opening bracket is not part of
            # the value.
            if value.startswith('\r\n'):
                value = value[2:]
            elif value.startswith('\n'):
                value = value[1:]
            arguments.append(Argument(text[at:end], value, True))
            at = end
        else:
            start = at
            value = []
            while at < len(text) and text[at] not in ' \t\r\n()#"':
                if text[at] == '\\' and at + 1 < len(text):
                    value.append(unescape(text[at + 1]))
                    at += 2
                else:
                    value.append(text[at])
                    at += 1
            arguments.append(Argument(text[start:at], ''.join(value), False))


def read_commands(text):
    commands = []
    at = 0
    while at < len(text):
        character = text[at]
        if character in ' \t\r\n':
            at += 1
            continue
        if character == '#':
            at = skip_comment(text, at)
            continue
        name = IDENTIFIER.match(text, at)
        if not name:
            raise CMakeError(f'line {line_of(text, at)}: {character!r} begins no command')
        line = line_of(text, at)
        at = name.end()
        while at < len(text) and text[at] in ' \t':
            at += 1
        if at >= len(text) or text[at] != '(':
            raise CMakeError(f'line {line}: {name.group(0)} is not followed by (')
        arguments, at = read_arguments(text, at + 1)
        commands.append(Command(name.group(0).lower(), arguments, line))
    return commands


def quoted(value):
    """A value as a CMake quoted argument, escaped as CMake's own generator escapes it."""
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"').replace('$', '\\$') + '"'


# ---- the tests of a build tree --------------------------------------------------------

class Test:
    def __init__(self, name, directory, add):
        self.name = name
        self.directory = directory
        self.add = add  # the add_test Command
        self.properties = []  # (the set_tests_properties Command, the name's Argument)
        self.values = {}  # property -> value, the last one set

    def is_cli(self):
        return self.name.startswith('cli.')

    def property_arguments(self):
        """Each (key, value Argument) of its set_tests_properties, in order."""
        for command, _ in self.properties:
            words = command.arguments
            first = next(i for i, word in enumerate(words) if word.value == 'PROPERTIES' and not word.quoted)
            pairs = words[first + 1:]
            for i in range(0, len(pairs) - 1, 2):
                yield pairs[i].value, pairs[i + 1]

    def fixtures(self, key):
        return [word for word in self.values.get(key, '').split(';') if word]


def read_tree(build):
    """Every test of the tree whose top is <build>/CTestTestfile.cmake, followed
    through subdirs() as ctest follows it: (tests by name in the order read,
    cli add_test commands read, names defined twice)."""
    tests = {}
    cli_read = 0
    twice = []

    def visit(directory):
        nonlocal cli_read
        path = os.path.join(directory, 'CTestTestfile.cmake')
        if not os.path.isfile(path):
            return
        with open(path, encoding='utf-8', errors='surrogateescape') as stream:
            text = stream.read()
        try:
            commands = read_commands(text)
        except CMakeError as error:
            raise CMakeError(f'{path}: {error}') from None
        for command in commands:
            if command.name == 'add_test' and command.arguments:
                name = command.arguments[0].value
                if name.startswith('cli.'):
                    cli_read += 1
                if name in tests:
                    twice.append(name)
                    continue
                tests[name] = Test(name, directory, command)
            elif command.name == 'set_tests_properties':
                words = command.arguments
                ends = [i for i, word in enumerate(words) if word.value == 'PROPERTIES' and not word.quoted]
                if not ends:
                    continue
                for word in words[:ends[0]]:
                    test = tests.get(word.value)
                    if test is None:
                        continue
                    test.properties.append((command, word))
                    pairs = words[ends[0] + 1:]
                    for i in range(0, len(pairs) - 1, 2):
                        test.values[pairs[i].value] = pairs[i + 1].value
            elif command.name == 'subdirs':
                for word in command.arguments:
                    visit(os.path.join(directory, word.value))

    visit(build)
    return tests, cli_read, twice


# ---- the rewrite ----------------------------------------------------------------------

PROGRAMS = ('katana_cli', 'katana_mcp')
# The properties a program can be run from, besides the command itself.
RUN_PROPERTIES = ('ENVIRONMENT', 'ENVIRONMENT_MODIFICATION')
# A program named as a path's last part, or alone (found on PATH), in a word
# of a command: after the start, a separator, '=' (-DCLI=) or ';' (a list),
# and before the end, a list's ';' or a blank.
MENTION = re.compile(r'(?:^|[/\\=;:])(katana_(?:cli|mcp))(?:\.exe)?(?=$|[;\s"])')


class Plan:
    def __init__(self):
        self.cli = []  # cli Tests, in the order read
        self.providers = []  # non-cli fixture providers carried as they are
        self.refusals = []
        self.no_program = []
        self.text = ''
        self.cpp_text = ''
        self.written = 0


def replace_programs(raw, cpp_bin, programs):
    """`raw` with each <cpp-bin>/katana_cli|mcp made the Rust's: (text, count)."""
    pattern = re.compile(re.escape(cpp_bin.rstrip('/')) + r'/(katana_(?:cli|mcp))(?:\.exe)?(?![A-Za-z0-9_.\-])')
    return pattern.subn(lambda match: programs.rstrip('/') + '/' + match.group(1), raw)


def rebuilt(test, replace):
    """The test's commands as they will be written: add_test, then each
    set_tests_properties for it alone, then the folder ctest ran it in. With
    `replace`, the programs are the Rust's in every word; (text, how many were
    replaced in the places a program runs from)."""
    run_count = 0

    def word(argument, runs):
        nonlocal run_count
        if not replace:
            return argument.raw
        text, count = replace(argument.raw)
        if runs:
            run_count += count
        return text

    lines = ['add_test(' + ' '.join(word(argument, True) for argument in test.add.arguments) + ')']
    for command, name in test.properties:
        words = command.arguments
        first = next(i for i, argument in enumerate(words) if argument.value == 'PROPERTIES' and not argument.quoted)
        pairs = words[first + 1:]
        texts = []
        for i, argument in enumerate(pairs):
            # A value (odd place) of a property a program runs from.
            runs = i % 2 == 1 and pairs[i - 1].value in RUN_PROPERTIES
            texts.append(word(argument, runs))
        lines.append(f'set_tests_properties({name.raw} PROPERTIES ' + ' '.join(texts) + ')')
    if 'WORKING_DIRECTORY' not in test.values:
        lines.append(f'set_tests_properties({test.add.arguments[0].raw} PROPERTIES WORKING_DIRECTORY '
                     f'{quoted(os.path.abspath(test.directory))})')
    return '\n'.join(lines) + '\n', run_count


def mentions(test):
    """The words a program runs from that name katana_cli or katana_mcp."""
    found = []
    for argument in test.add.arguments[1:]:
        found += [argument.value for _ in MENTION.finditer(argument.value)]
    for key, value in test.property_arguments():
        if key in RUN_PROPERTIES:
            found += [value.value for _ in MENTION.finditer(value.value)]
    return found


def other_programs(test, cpp_bin):
    """Every <cpp-bin>/<name> the test's commands name that is neither program."""
    pattern = re.compile(re.escape(cpp_bin.rstrip('/')) + r'/([^/\s"\';()\[\]]+)')
    others = []
    texts = [argument.raw for argument in test.add.arguments]
    for command, _ in test.properties:
        texts += [argument.raw for argument in command.arguments]
    for text in texts:
        for match in pattern.finditer(text):
            name = match.group(1)
            if name.endswith('.exe'):
                name = name[:-len('.exe')]
            if name not in PROGRAMS:
                others.append(cpp_bin.rstrip('/') + '/' + match.group(1))
    return others


HEADER = ('# Written by tools/rust_cli_gate.py from the CTest files of {build}: its cli tests\n'
          '# with {what}. Rewritten at every run; do not edit.\n')


def plan_rewrite(build, tests, cli_read, twice, cpp_bin, programs):
    plan = Plan()
    plan.cli = [test for test in tests.values() if test.is_cli()]
    # The fixtures the cli tests require, and the tests that are not cli
    # tests among their providers, to their own fixtures' providers.
    included = {test.name for test in plan.cli}
    wanted = [fixture for test in plan.cli for fixture in test.fixtures('FIXTURES_REQUIRED')]
    seen = set()
    while wanted:
        fixture = wanted.pop()
        if fixture in seen:
            continue
        seen.add(fixture)
        for test in tests.values():
            if test.name in included:
                continue
            if fixture in test.fixtures('FIXTURES_SETUP') or fixture in test.fixtures('FIXTURES_CLEANUP'):
                included.add(test.name)
                plan.providers.append(test)
                wanted += test.fixtures('FIXTURES_REQUIRED')

    def replace(raw):
        return replace_programs(raw, cpp_bin, programs)

    rust = [HEADER.format(build=build, what='katana_cli and katana_mcp the Rust\'s, in ' + programs)]
    cpp = [HEADER.format(build=build, what='the C++ programs as they are, for --rerun-cpp')]
    for test in tests.values():
        if test.name not in included:
            continue
        if not test.is_cli():
            # Carried as the C++ suite runs it (the top of this file says why).
            text, _ = rebuilt(test, None)
            rust.append(text)
            cpp.append(text)
            continue
        text, replaced = rebuilt(test, replace)
        named = mentions(test)
        if len(named) > replaced:
            plan.refusals.append(
                f'{test.name} runs {MENTION.search(named[0]).group(1)} where the gate replaces nothing '
                f'({"; ".join(named)}): a test that runs a program must name it as '
                f'{cpp_bin.rstrip("/")}/katana_cli or katana_mcp')
        for other in other_programs(test, cpp_bin):
            plan.refusals.append(
                f'{test.name} names {other}, a program of the C++ build the gate does not replace')
        if not named:
            plan.no_program.append(test.name)
        rust.append(text)
        cpp.append(rebuilt(test, None)[0])
    plan.text = ''.join(rust)
    plan.cpp_text = ''.join(cpp)
    # The rewrite read back: every cli test read must be there, once.
    written = [command.arguments[0].value for command in read_commands(plan.text)
               if command.name == 'add_test' and command.arguments
               and command.arguments[0].value.startswith('cli.')]
    plan.written = len(set(written))
    if plan.written != cli_read:
        detail = ''
        if twice:
            detail = ': ' + ', '.join(sorted(set(twice))) + ' is defined more than once'
        plan.refusals.append(
            f'the count of cli tests written ({plan.written}) differs from the count read ({cli_read}){detail}')
    return plan


# ---- the logs ---------------------------------------------------------------------------

TEST_LINE = re.compile(r'^\d+/\d+ Test: (.*)$', re.M)
OUTPUT_START = re.compile(r'\nOutput:\n-+\n')
STATUS = re.compile(r'\nTest time =[^\n]*\n-+\n([^\n]*)')
END_OF_OUTPUT = '<end of output>\nTest time ='


def read_log(path):
    """{test name: (output or None, ctest's verdict line)} of a LastTest.log."""
    if not os.path.isfile(path):
        return {}
    with open(path, 'rb') as stream:
        text = stream.read().decode('utf-8', 'surrogateescape')
    starts = [(match.start(), match.group(1)) for match in TEST_LINE.finditer(text)]
    results = {}
    for index, (start, name) in enumerate(starts):
        end = starts[index + 1][0] if index + 1 < len(starts) else len(text)
        block = text[start:end]
        head = OUTPUT_START.search(block)
        tail = block.rfind(END_OF_OUTPUT)
        output = block[head.end():tail] if head and tail >= head.end() else None
        # ctest's verdict follows the output: searched from there, so that a
        # program's own text cannot be taken for it.
        status = STATUS.search(block, tail if tail >= 0 else 0)
        results[name] = (output, status.group(1).strip() if status else '')
    return results


def passed(verdict):
    # "Test Passed." - or "Test Pass Reason:" where a pass expression or a
    # WILL_FAIL decided; a failure is "Test Failed." or "Test Fail Reason:".
    return verdict.startswith('Test Pass')


SECONDS = re.compile(r'seconds=[0-9]+\.[0-9]+')
SCRATCH = re.compile(r'katana-scratch([/\\])[0-9]+-[0-9]+')
ERROR_STREAM = ('error: ', 'warning: ', 'note: ')


def normalised(output, paths):
    """The output's standard-output lines and standard-error lines, each in
    order, after the normalisations at the top of this file."""
    for path, placeholder in paths:
        output = output.replace(path, placeholder)
    output = SECONDS.sub('seconds=<s>', output)
    output = SCRATCH.sub(r'katana-scratch\1<process>', output)
    lines = output.split('\n')
    return ([line for line in lines if not line.startswith(ERROR_STREAM)],
            [line for line in lines if line.startswith(ERROR_STREAM)])


def difference(name, stream, cpp, rust):
    diff = list(difflib.unified_diff(cpp, rust, f'{name} {stream} (C++)', f'{name} {stream} (Rust)',
                                     n=1, lineterm=''))
    if len(diff) > 40:
        diff = diff[:40] + [f'... and {len(diff) - 40} lines more']
    return diff


# ---- the run ----------------------------------------------------------------------------

SAFE_PATH = re.compile(r'^[A-Za-z0-9_./+,:~%=-]+$')


def default_cpp_bin(build):
    cache = os.path.join(build, 'CMakeCache.txt')
    if os.path.isfile(cache):
        with open(cache, encoding='utf-8', errors='replace') as stream:
            for line in stream:
                if line.startswith('CMAKE_CACHEFILE_DIR:'):
                    return line.split('=', 1)[1].strip() + '/bin'
    return build.rstrip('/') + '/bin'


def default_programs(build):
    target = os.environ.get('CARGO_TARGET_DIR')
    if target:
        return os.path.join(target, 'release')
    return os.path.normpath(os.path.join(build, '..', 'cargo', 'release'))


def run_ctest(ctest, folder, text, parallel, extra):
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, 'CTestTestfile.cmake'), 'w', encoding='utf-8', newline='\n',
              errors='surrogateescape') as stream:
        stream.write(text)
    # A log a failed start would leave is not this run's.
    stale = os.path.join(folder, 'Testing', 'Temporary', 'LastTest.log')
    if os.path.isfile(stale):
        os.remove(stale)
    command = [ctest, '--test-dir', folder, '--parallel', str(parallel), '--output-on-failure'] + extra
    print('== rust_cli_gate: ' + ' '.join(command), flush=True)
    return subprocess.call(command)


def main():
    parser = argparse.ArgumentParser(description='The cli tests against the Rust programs, '
                                     'and both programs\' outputs compared (the top of this file).')
    parser.add_argument('--build', required=True, help='the C++ build tree whose CTest files are read')
    parser.add_argument('--programs', help='the folder of the Rust katana_cli and katana_mcp')
    parser.add_argument('--cpp-bin', help='the C++ programs\' folder as the CTest files name it')
    parser.add_argument('--output', help='the gate\'s folder (default <build>/rust_cli_gate)')
    parser.add_argument('--cpp-log', help='the C++ run\'s LastTest.log')
    parser.add_argument('--ctest', default='ctest')
    parser.add_argument('--parallel', type=int, default=14)
    parser.add_argument('--check', action='store_true', help='check the rewrite; write and run nothing')
    parser.add_argument('--rerun-cpp', action='store_true', help='run the C++ cli tests again first')
    parser.add_argument('--compare-only', action='store_true', help='compare the logs as they are')
    parser.add_argument('ctest_arguments', nargs='*', help='after --: passed to ctest')
    arguments = parser.parse_args()

    build = arguments.build
    cpp_bin = arguments.cpp_bin or default_cpp_bin(build)
    programs = arguments.programs or default_programs(build)
    output = arguments.output or os.path.join(build, 'rust_cli_gate')
    if not SAFE_PATH.match(programs):
        print(f'rust_cli_gate: refused: the Rust programs\' folder {programs!r} holds a character a '
              f'CTest file would have to escape; give one of letters, digits and / . _ - + , : ~ % =')
        return 2
    if not os.path.isfile(os.path.join(build, 'CTestTestfile.cmake')):
        print(f'rust_cli_gate: refused: {build} has no CTestTestfile.cmake; name a configured C++ build tree')
        return 2
    try:
        tests, cli_read, twice = read_tree(build)
        plan = plan_rewrite(build, tests, cli_read, twice, cpp_bin, programs)
    except CMakeError as error:
        print(f'rust_cli_gate: refused: a CTest file cannot be read: {error}')
        return 2

    for refusal in plan.refusals:
        print(f'rust_cli_gate: refused: {refusal}')
    providers = ', '.join(test.name for test in plan.providers) or 'none'
    print(f'rust_cli_gate: {cli_read} cli tests read, {plan.written} written '
          f'({len(plan.cli) - len(plan.no_program)} run a program, {len(plan.no_program)} '
          f'run{"s" if len(plan.no_program) == 1 else ""} none); {len(plan.providers)} fixture '
          f'provider{"" if len(plan.providers) == 1 else "s"} carried from the C++ suite as it is: {providers}')
    if plan.refusals:
        return 2
    for program in PROGRAMS:
        path = os.path.join(programs, program)
        if not os.path.isfile(path):
            print(f'rust_cli_gate: warning: {path} is not there: the tests that run it will fail')
    if arguments.check:
        return 0

    cpp_log = arguments.cpp_log or os.path.join(build, 'Testing', 'Temporary', 'LastTest.log')
    tested = 0
    if not arguments.compare_only:
        if arguments.rerun_cpp:
            run_ctest(arguments.ctest, os.path.join(output, 'cpp'), plan.cpp_text, arguments.parallel,
                      arguments.ctest_arguments)
        tested = run_ctest(arguments.ctest, output, plan.text, arguments.parallel, arguments.ctest_arguments)
    if arguments.rerun_cpp:
        cpp_log = os.path.join(output, 'cpp', 'Testing', 'Temporary', 'LastTest.log')

    cpp_results = read_log(cpp_log)
    rust_results = read_log(os.path.join(output, 'Testing', 'Temporary', 'LastTest.log'))
    paths = []
    for program in PROGRAMS:
        for folder in (cpp_bin, programs):
            for suffix in ('.exe', ''):
                paths.append((folder.rstrip('/') + '/' + program + suffix, f'<{program}>'))
    same = 0
    failed = []
    differing = []
    not_compared = []
    for test in plan.cli:
        if test.name not in rust_results:
            continue  # not in this run (ctest's selection); ctest says why
        output_text, verdict = rust_results[test.name]
        if not passed(verdict):
            cpp_verdict = cpp_results.get(test.name, (None, ''))[1]
            also = ' (the C++ run failed it too)' if cpp_verdict and not passed(cpp_verdict) else ''
            failed.append(f'{test.name}: {verdict or "no verdict"}{also}')
        cpp_output = cpp_results.get(test.name, (None, ''))[0]
        if cpp_output is None or output_text is None:
            not_compared.append(f'{test.name}: '
                                + ('the C++ run\'s log has no output for it (--rerun-cpp gives one)'
                                   if cpp_output is None else 'this run\'s log has no output for it'))
            continue
        cpp_out, cpp_err = normalised(cpp_output, paths)
        rust_out, rust_err = normalised(output_text, paths)
        if cpp_out == rust_out and cpp_err == rust_err:
            same += 1
            continue
        lines = []
        if cpp_out != rust_out:
            lines += difference(test.name, 'standard output', cpp_out, rust_out)
        if cpp_err != rust_err:
            lines += difference(test.name, 'standard error', cpp_err, rust_err)
        differing.append((test.name, lines))

    for line in failed:
        print(f'rust_cli_gate: failed: {line}')
    for name, lines in differing:
        print(f'rust_cli_gate: the outputs differ: {name}')
        for line in lines:
            print('    ' + line)
    for line in not_compared:
        print(f'rust_cli_gate: not compared: {line}')
    compared = same + len(differing)
    print(f'rust_cli_gate: {compared + len(not_compared)} cli tests ran: {len(failed)} failed; outputs '
          f'{same} the same as the C++\'s, {len(differing)} different, {len(not_compared)} not compared'
          f' (ctest exit {tested})')
    return 1 if tested != 0 or failed or differing or not_compared else 0


if __name__ == '__main__':
    sys.exit(main())
