#!/usr/bin/env python3
# tools/ctest_tree.py - the CTest files of a build tree, read as ctest reads
# them and written back for a gate; what the gates of the Rust port share.
#
# Its users: the cli gate (tools/rust_cli_gate.py, step 6) and the window gate
# (tools/rust_window_gate.py, step 7), which run tests of the C++ build against
# the Rust programs through a rewritten copy of the build's own CTest files,
# and the window's geometry probe (tools/window_geometry.py), which runs the
# C++ window in the environment the build gives its headless tests. It was the
# cli gate's own reader until the window gate needed the same one; one reader,
# so that the two gates cannot come to disagree about what a CTest file says.
#
# THE API
#
#   The CMake language, as far as a generated CTestTestfile.cmake uses it:
#     read_commands(text) -> [Command]: each command's lower-case name, its
#       Arguments (raw: the text as written; value: as CMake reads it; quoted)
#       and its line. Raises CMakeError.
#     quoted(value): a value as a quoted argument, escaped as CMake's own
#       generator escapes it.
#   The tests of a build tree:
#     read_tree(build) -> Tree: <build>/CTestTestfile.cmake followed down
#       through subdirs(), as ctest follows it. Tree.tests is every test by
#       name, its first definition, in the order read; Tree.definitions every
#       add_test read, in order, a name defined again a Test of its own;
#       Tree.twice the names defined again. Tree.take(text, directory) reads
#       one more file's text into it (a gate reads its rewrite back so).
#     Test: name, directory (the folder of the file that defined it), add (its
#       add_test Command), properties ([(set_tests_properties Command, the
#       name's Argument)]), values ({property: the last value set});
#       property_arguments(), fixtures(key), run_words().
#   The rewrite:
#     RUN_PROPERTIES; mention_pattern(names); replace_programs(raw, cpp_bin,
#     names, target); rebuilt(test, replace); mentions(test, pattern);
#     other_programs(test, cpp_bin, names); fixture_providers(tests, chosen).
#   ctest's log: read_log(path), passed(verdict); and what differs between
#     the C++'s output and the Rust's, shown alike: difference(label, cpp, rust).
#   Words of a run that differ from run to run: SECONDS, SCRATCH.
#   A test's environment: environment_of(test, base).
#   The run: SAFE_PATH, default_cpp_bin(build), default_programs(build),
#     source_dir(build), run_ctest(ctest, folder, text, parallel, extra, label).

import difflib
import os
import re
import subprocess

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

# The properties a program can be run from, besides the command itself.
RUN_PROPERTIES = ('ENVIRONMENT', 'ENVIRONMENT_MODIFICATION')


class Test:
    def __init__(self, name, directory, add):
        self.name = name
        self.directory = directory
        self.add = add  # the add_test Command
        self.properties = []  # (the set_tests_properties Command, the name's Argument)
        self.values = {}  # property -> value, the last one set

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

    def run_words(self):
        """The values a program can be run from: the command's words after
        the test's name, then the values of the RUN_PROPERTIES, in order."""
        words = [argument.value for argument in self.add.arguments[1:]]
        words += [value.value for key, value in self.property_arguments() if key in RUN_PROPERTIES]
        return words


class Tree:
    """The tests of CTest files read so far (the top of this file)."""

    def __init__(self):
        self.tests = {}
        self.definitions = []
        self.twice = []

    def take(self, text, directory):
        """One CTest file's text, read as the file of `directory`: its
        add_test and set_tests_properties; the subdirs() it names, as folders."""
        commands = read_commands(text)
        here = {}  # this file's own tests: its properties are theirs first
        folders = []
        for command in commands:
            if command.name == 'add_test' and command.arguments:
                name = command.arguments[0].value
                test = Test(name, directory, command)
                self.definitions.append(test)
                here[name] = test
                if name in self.tests:
                    self.twice.append(name)
                    continue
                self.tests[name] = test
            elif command.name == 'set_tests_properties':
                words = command.arguments
                ends = [i for i, word in enumerate(words) if word.value == 'PROPERTIES' and not word.quoted]
                if not ends:
                    continue
                for word in words[:ends[0]]:
                    test = here.get(word.value) or self.tests.get(word.value)
                    if test is None:
                        continue
                    test.properties.append((command, word))
                    pairs = words[ends[0] + 1:]
                    for i in range(0, len(pairs) - 1, 2):
                        test.values[pairs[i].value] = pairs[i + 1].value
            elif command.name == 'subdirs':
                folders += [os.path.join(directory, word.value) for word in command.arguments]
        return folders


def read_tree(build):
    """Every test of the tree whose top is <build>/CTestTestfile.cmake, followed
    through subdirs() as ctest follows it."""
    tree = Tree()

    def visit(directory):
        path = os.path.join(directory, 'CTestTestfile.cmake')
        if not os.path.isfile(path):
            return
        with open(path, encoding='utf-8', errors='surrogateescape') as stream:
            text = stream.read()
        try:
            folders = tree.take(text, directory)
        except CMakeError as error:
            raise CMakeError(f'{path}: {error}') from None
        for folder in folders:
            visit(folder)

    visit(build)
    return tree


# ---- the rewrite ----------------------------------------------------------------------

def mention_pattern(names):
    """A program of `names` named as a path's last part, or alone (found on
    PATH), in a word of a command: after the start, a separator, '=' (-DCLI=)
    or ';' (a list), and before the end, a list's ';' or a blank."""
    return re.compile(r'(?:^|[/\\=;:])(' + '|'.join(re.escape(name) for name in names)
                      + r')(?:\.exe)?(?=$|[;\s"])')


def replace_programs(raw, cpp_bin, names, target):
    """`raw` with each <cpp-bin>/<name> of `names` made target(name): (text, count)."""
    pattern = re.compile(re.escape(cpp_bin.rstrip('/')) + r'/(' + '|'.join(re.escape(name) for name in names)
                         + r')(?:\.exe)?(?![A-Za-z0-9_.\-])')
    return pattern.subn(lambda match: target(match.group(1)), raw)


def rebuilt(test, replace):
    """The test's commands as they will be written: add_test, then each
    set_tests_properties for it alone, then the folder ctest ran it in. With
    `replace` (raw -> (text, count)), the programs are replaced in every word;
    (text, how many were replaced in the places a program runs from)."""
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


def mentions(test, pattern):
    """The words a program runs from in which `pattern` (mention_pattern) finds
    a program, once for each time it does."""
    found = []
    for argument in test.add.arguments[1:]:
        found += [argument.value for _ in pattern.finditer(argument.value)]
    for key, value in test.property_arguments():
        if key in RUN_PROPERTIES:
            found += [value.value for _ in pattern.finditer(value.value)]
    return found


def other_programs(test, cpp_bin, names):
    """Every <cpp-bin>/<name> the test's commands name that is none of `names`."""
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
            if name not in names:
                others.append(cpp_bin.rstrip('/') + '/' + match.group(1))
    return others


def fixture_providers(tests, chosen):
    """The tests not among `chosen` that set up or clean up a fixture a chosen
    test requires (FIXTURES_REQUIRED), and the providers of what those require
    in turn, in the order found."""
    providers = []
    included = {test.name for test in chosen}
    wanted = [fixture for test in chosen for fixture in test.fixtures('FIXTURES_REQUIRED')]
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
                providers.append(test)
                wanted += test.fixtures('FIXTURES_REQUIRED')
    return providers


# ---- ctest's log ----------------------------------------------------------------------

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


def difference(label, cpp, rust):
    """The C++'s and the Rust's lines as a unified diff, one line of context,
    cut at 40 lines: how both gates show what differs."""
    diff = list(difflib.unified_diff(cpp, rust, f'{label} (C++)', f'{label} (Rust)', n=1, lineterm=''))
    if len(diff) > 40:
        diff = diff[:40] + [f'... and {len(diff) - 40} lines more']
    return diff


# ---- words of a run that differ from run to run ---------------------------------------
#
# Both gates read them alike (each gate's header says why it reads them so).
# seconds=<d>.<ddd> is the wall-clock time of a GDAL step (the geo and GIS
# records); katana-scratch/<pid>-<ns> is the geo executor's scratch folder,
# named by the process's id and its start time (geo_verbs, own_scratch) - the
# /<n> below it is compared.

SECONDS = re.compile(r'seconds=[0-9]+\.[0-9]+')
SCRATCH = re.compile(r'katana-scratch([/\\])[0-9]+-[0-9]+')


# ---- a test's environment -------------------------------------------------------------

def split_list(value):
    """A CMake list's items; a \\; inside an item is kept, as a ';'."""
    items = re.split(r'(?<!\\);', value)
    return [item.replace('\\;', ';') for item in items if item]


def environment_of(test, base):
    """The environment ctest gives the test's command: `base` (a dict, left
    as it is) with the test's ENVIRONMENT, then its ENVIRONMENT_MODIFICATION,
    applied as CMake documents them. Raises CMakeError for an operation CMake
    does not have."""
    environment = dict(base)
    for item in split_list(test.values.get('ENVIRONMENT', '')):
        name, _, value = item.partition('=')
        environment[name] = value
    before = dict(environment)  # what `reset:` goes back to
    separator = os.pathsep
    for item in split_list(test.values.get('ENVIRONMENT_MODIFICATION', '')):
        name, equals, rest = item.partition('=')
        operation, colon, value = rest.partition(':')
        if not equals or not colon:
            raise CMakeError(f'{test.name}: ENVIRONMENT_MODIFICATION item {item!r} is not NAME=OP:VALUE')
        current = environment.get(name)
        if operation == 'reset':
            if name in before:
                environment[name] = before[name]
            else:
                environment.pop(name, None)
        elif operation == 'set':
            environment[name] = value
        elif operation == 'unset':
            environment.pop(name, None)
        elif operation == 'string_append':
            environment[name] = (current or '') + value
        elif operation == 'string_prepend':
            environment[name] = value + (current or '')
        elif operation == 'path_list_append':
            environment[name] = current + separator + value if current else value
        elif operation == 'path_list_prepend':
            environment[name] = value + separator + current if current else value
        elif operation == 'cmake_list_append':
            environment[name] = current + ';' + value if current else value
        elif operation == 'cmake_list_prepend':
            environment[name] = value + ';' + current if current else value
        else:
            raise CMakeError(f'{test.name}: ENVIRONMENT_MODIFICATION has no operation {operation!r}')
    return environment


# ---- the run ----------------------------------------------------------------------------

# A folder a gate writes into a CTest file, unescaped: letters, digits and
# these, none of which a CMake quoted or bracket argument would have to escape.
SAFE_PATH = re.compile(r'^[A-Za-z0-9_./+,:~%=-]+$')


def cache_value(build, key):
    """<key>'s value in <build>/CMakeCache.txt, or None."""
    cache = os.path.join(build, 'CMakeCache.txt')
    if os.path.isfile(cache):
        with open(cache, encoding='utf-8', errors='replace') as stream:
            for line in stream:
                if line.startswith(key + ':'):
                    return line.split('=', 1)[1].strip()
    return None


def default_cpp_bin(build):
    """The C++ programs' folder as the build's CTest files spell it."""
    folder = cache_value(build, 'CMAKE_CACHEFILE_DIR')
    if folder is not None:
        return folder + '/bin'
    return build.rstrip('/') + '/bin'


def default_programs(build):
    """The Rust programs' folder: cargo's release folder."""
    target = os.environ.get('CARGO_TARGET_DIR')
    if target:
        return os.path.join(target, 'release')
    return os.path.normpath(os.path.join(build, '..', 'cargo', 'release'))


def source_dir(build):
    """The checkout the build was configured from, or None."""
    return cache_value(build, 'CMAKE_HOME_DIRECTORY')


def run_ctest(ctest, folder, text, parallel, extra, label):
    """`text` written as <folder>/CTestTestfile.cmake, and ctest run on it:
    ctest's exit code."""
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, 'CTestTestfile.cmake'), 'w', encoding='utf-8', newline='\n',
              errors='surrogateescape') as stream:
        stream.write(text)
    # A log a failed start would leave is not this run's.
    stale = os.path.join(folder, 'Testing', 'Temporary', 'LastTest.log')
    if os.path.isfile(stale):
        os.remove(stale)
    command = [ctest, '--test-dir', folder, '--parallel', str(parallel), '--output-on-failure'] + extra
    print(f'== {label}: ' + ' '.join(command), flush=True)
    return subprocess.call(command)
