#!/usr/bin/env python3
# tools/rust_window_gate.py --build <C++ build tree> [options] [-- ctest arguments]
#
# The gate of the Rust port's step 7 (docs/rust.md; the step 7 plan, section
# 4.6): every test of the C++ build that runs the window headless runs against
# the Rust katana, through a rewritten copy of the build's own CTest files, as
# the cli gate (tools/rust_cli_gate.py) runs the cli tests. Both programs'
# standard output and error are captured by a stream wrapper
# (tools/window_streams.py) and compared stream by stream; both screenshots
# are kept side by side for a person to look at, never compared pixel for
# pixel. The CTest reader is tools/ctest_tree.py, the cli gate's own.
#
# RUN BOOK (the integrator's)
#
#   After the C++ suite has run in the same build tree - never beside it: the
#   window tests write into folders of that tree - in WSL, as build_visible.sh's
#   `wsl` phase lays the trees out, after the cli gate:
#
#     QT_QPA_PLATFORM=offscreen xvfb-run -a python3 tools/rust_window_gate.py \
#         --build ~/katana-build/rust-port/linux-release \
#         --programs ~/katana-build/rust-port/cargo/release --rerun-cpp
#
#   under `xvfb-run -a`, as the suite runs (a fixture the gate carries runs as
#   the suite runs it; the window tests themselves set QT_QPA_PLATFORM=offscreen
#   in their own properties, and the Rust window opens no window headless). It
#     1. reads the build's CTest files from <build>/CTestTestfile.cmake down
#        through their subdirs() (ctest_tree.read_tree), and takes every test
#        that runs the window (below, "Which tests run the window") - 267 on
#        2026-11-11, all named qt_*_headless, counted at each run - with the
#        tests it carries (below);
#     2. replaces <cpp-bin>/katana, wherever it appears in a window test's
#        command or the properties a program runs from (inside -DAPP= too),
#        with the run's stream wrapper: <output>/rust/katana for the Rust run,
#        <output>/cpp/katana for the C++ one - small launchers it writes, which
#        run tools/window_streams.py over cargo's <programs>/katana and over
#        <cpp-bin>/katana - and adds KATANA_GATE_TEST=<test> to each window
#        test's ENVIRONMENT_MODIFICATION, by which the wrapper files what it
#        keeps (the wrapper takes it out of the program's environment);
#     3. with --rerun-cpp, runs the C++ window tests again first, through the
#        wrapper, in <output>/cpp, so that both programs' streams are captured
#        alike; then writes <output>/rust/CTestTestfile.cmake and runs `ctest
#        --test-dir <output>/rust --parallel 14 --output-on-failure` on it, with
#        any arguments given after `--` (`-- -R survey` runs those alone, and
#        compares those alone); each run's streams folder is emptied first;
#     4. reads both runs' Testing/Temporary/LastTest.log for ctest's verdicts,
#        compares each window test's streams (below), and writes the looks page.
#   QT_QPA_PLATFORM=offscreen and QT_SCALE_FACTOR reach both programs through
#   the tests' own properties: the Rust program reads QT_SCALE_FACTOR for its
#   pixels-per-point in a headless run, as the C++ program does through Qt, so
#   that a test's environment means the same to both (the name goes when the
#   C++ does, at step 9).
#   It exits 0 when ctest passed every test and every window test's streams
#   are the C++'s; 1 when a test failed, its streams differ or could not be
#   compared, each listed by test; 2 when it refuses to run (below) or is
#   called wrongly.
#
#   Without --rerun-cpp it compares with the C++ streams an earlier
#   --rerun-cpp kept in <output>/cpp/streams; the C++ suite's own run keeps
#   none (check_screenshot.cmake prints a run's output only when it fails), so
#   a test the C++ rerun did not run is "not compared".
#
#   Other options: --check reads and checks the rewrite, prints what it would
#   run, and writes and runs nothing (the self-check's mode, below);
#   --compare-only compares the streams kept as they are and writes the looks
#   page, running nothing; --cpp-bin names the C++ programs' folder as the
#   CTest files spell it (default CMAKE_CACHEFILE_DIR of <build>/CMakeCache.txt,
#   then "/bin"); --programs the folder of the Rust katana (default
#   $CARGO_TARGET_DIR/release, else <build>/../cargo/release); --output the
#   gate's folder (default <build>/rust_window_gate); --python the interpreter
#   the wrapper runs under (default this one's); --ctest the ctest to run;
#   --parallel ctest's -j.
#
# WHICH TESTS RUN THE WINDOW
#
#   A test whose command, or a property a program runs from (ENVIRONMENT,
#   ENVIRONMENT_MODIFICATION), names <cpp-bin>/katana anywhere, or names
#   katana (katana.exe) as a whole part of a word: the word, a -D<NAME>= value,
#   an item of a ;-list, the value of a NAME=VALUE or NAME=OP:VALUE item - as
#   a path's last part or alone (found on PATH). A sentence that has the word
#   katana in it names no program; a path whose last part is katana does.
#   Today 260 give it to a check script as -DAPP= and 7 run it directly.
#
# WHAT IT REFUSES (exit 2, every refusal listed, nothing written or run)
#
#   1. A window test that runs the window where the gate replaces nothing: it
#      names katana as above more often than <cpp-bin>/katana was replaced -
#      found on PATH, or at another path. Such a test would run the C++ window,
#      or none, under the Rust's name.
#   2. A window test that names any other program of the C++ build,
#      <cpp-bin>/<name> (none on 2026-11-11): the gate swaps one program, and a
#      second would run the C++ under the gate's verdict.
#   3. A count of window tests written that differs from the count read: the
#      rewritten file is read back by the same reader and its window tests
#      counted, so a test lost by the rewrite, or a name defined twice (which
#      one file cannot hold), stops the gate rather than shrinking it.
#
# WHAT IT CARRIES AS THE C++ SUITE RUNS IT (unchanged, named in its output)
#
#   1. Fixture providers: a test that is not a window test and sets up or
#      cleans up a fixture a window test requires (FIXTURES_REQUIRED), and the
#      providers of what those require in turn - on 2026-11-11 eighteen: the
#      `cmake -E rm` and make_directory steps, the update state folder's check,
#      and cli.surface_project_for_the_window_edit_check_is_made (with the
#      step that clears its folder first), which makes with the C++ katana_cli
#      the project a window test then edits. Left out, ctest would run the
#      window test without its fixture.
#   2. Fixture users: a test that is not a window test and requires a fixture
#      a window test sets up (FIXTURES_SETUP), with its own providers - on
#      2026-11-11 update_health_marker_is_there_after_the_health_run, which
#      reads the marker the health run wrote, and cli.a_saved_plan_views_paper_
#      scale_is_editable_and_undoable, which opens with the C++ katana_cli the
#      project the window saved. They are how the suite checks what the window
#      made: without them a health run that exits 0 and writes no marker would
#      pass. (The plan named the providers; the users are this tool's
#      addition, for that reason.)
#   Each test the rewrite moves into one file keeps the directory ctest ran it
#   in: a test with no WORKING_DIRECTORY is given its file's folder.
#
# THE STREAMS
#
#   For the n-th run of the window in a test, the wrapper keeps in
#   <output>/<run>/streams/<test>/ the files <n>.args, <n>.out, <n>.err,
#   <n>.exit and, when the run wrote its --screenshot, <n>.png (the wrapper's
#   header says how). For each window test that ran, the gate compares the
#   C++ run's with the Rust run's: how many times it ran the window; each
#   run's exit code (the code is what the scripts judge - REFUSED needs 1,
#   EXPECT 0 - and a WILL_FAIL run passes on any code but 0, so a Rust 2 where
#   the C++ says 1 passes its test and is still another program); and each
#   run's standard output and standard error, each whole and in its order,
#   after the normalisations below and no others.
#
# THE NORMALISATIONS (anything else that differs fails the gate; one added
# later is added here and in the lists below, with its reason)
#
#   (a) Paths. The two programs' paths - <cpp-bin>/katana, <programs>/katana,
#       each as given and with its links resolved, as a program reports its
#       own path, and the two wrappers - read <katana>. The test's own paths read
#       <path>, the most specific first, as check_screenshot.cmake's FORBID
#       replaces them: the copy, the output, the customisation folder, the
#       import, the project and the work folder of a check script's -D
#       variables; for a test that runs the window directly, each absolute
#       path among its arguments and its folder; the programs' folders; the
#       checkout. Why: the two programs are found at two folders, and paths
#       are where a checkout and a build happen to live, not words either
#       program chose.
#   (b) Qt's words. Whole lines that Qt or its platform plugin prints read as
#       nothing: check_screenshot.cmake's own list ("This plugin does not
#       support propagateSizeHints()" and the rest), and the lines that begin
#       "QStandardPaths:", "Fontconfig", "qt.qpa." and "QFont::" (QT_LINES).
#       Why: they are the toolkit's words, not Katana's, and the Rust window
#       has no Qt.
#   (c) Times and scratch: seconds=<d>.<ddd> reads seconds=<s>, and
#       katana-scratch/<pid>-<ns> reads katana-scratch/<process>, as in the
#       cli gate (ctest_tree.SECONDS and SCRATCH say why).
#   (d) Pixel measures that two toolkits' antialiasing and text metrics make
#       differ even with the geometry the same (PIXEL_MEASURES). The list
#       starts empty: a field is added only when a run shows it differing,
#       with its reason. The plan's coverage, covered=N, is the expected
#       candidate; painted=N should never need it, since the 3D view's
#       software rasteriser is bit for bit the C++'s.
#
# THE LOOKS
#
#   <output>/looks/<test>/cpp.png and rust.png - cpp-<n>.png and rust-<n>.png
#   for a test's n-th run of the window, n > 1 - copied from the streams, with
#   <output>/looks/index.html, the pairs side by side for a person (a test
#   that failed or whose streams differ first, then the rest, each in the
#   order read; each pair with its sizes in pixels and bytes), and
#   <output>/looks/index.txt, a line a pair, for a model that reads the PNGs
#   by path. From Windows: \\wsl.localhost\Ubuntu-24.04\root\katana-build\
#   rust-port\linux-release\rust_window_gate\looks\index.html. The gate never
#   compares the pictures: two toolkits draw two pictures, in every glyph and
#   border. Whether the window LOOKS right is a person's question.
#
# LIMITS
#
#   The launchers are POSIX sh scripts, which check_screenshot.cmake's
#   execute_process runs as it ran the program: the gate runs in WSL, as the
#   cli gate does. On Windows (step 8) a launcher needs another form (a .cmd,
#   or a small program), since a CTest file can name only a program there.
#   For the 7 tests that run the window directly, ctest hands both streams one
#   pipe; the wrapper's header says how far their order through it can move.
#
# ITS OWN CHECK
#
#   tests/rust_check_fixtures/rust_window_gate/ holds small synthetic build
#   trees of CTest files, one per refusal (unreplaced, other_program,
#   count_differs) and one the gate must accept (accepted: a screenshot test
#   given -DAPP=, a direct run, a fixture provider and a fixture user carried,
#   a test of another directory that is not a window test). Each is run with
#   --check --cpp-bin /katana-build/bin --programs /katana-build/cargo/release
#   --output /katana-build/rust_window_gate and its output matched, as the cli
#   gate's are; a check that refused everything would pass the three
#   refusals, which is why the fourth must pass.

import argparse
import html
import os
import re
import shlex
import shutil
import struct
import sys

sys.dont_write_bytecode = True  # a check leaves nothing behind in the tools folder
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ctest_tree  # noqa: E402 - the shared reader, beside this file
import window_streams  # noqa: E402 - the stream wrapper: its variable and its folders' names
from ctest_tree import CMakeError  # noqa: E402
from window_streams import TEST_VARIABLE, folder_name  # noqa: E402

WINDOW = 'katana'
RUNS = ('cpp', 'rust')
STREAMS_TOOL = os.path.abspath(window_streams.__file__)

# ---- which tests run the window -------------------------------------------------------

D_VARIABLE = re.compile(r'-D([A-Za-z_][A-Za-z0-9_]*)=(.*)', re.S)
IDENTIFIER = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
OPERATION = re.compile(r'[a-z_]+:')  # ENVIRONMENT_MODIFICATION's OP:
AFTER_PATH = r'(?![A-Za-z0-9_.\-])'


def leaves(text):
    """The parts of a word a program can be named by, each once: a -D value,
    the items of a ;-list, the value of a NAME=VALUE or NAME=OP:VALUE item,
    else the word."""
    match = D_VARIABLE.match(text)
    if match:
        return leaves(match.group(2))
    if ';' in text:
        return [leaf for item in ctest_tree.split_list(text) for leaf in leaves(item)]
    name, equals, value = text.partition('=')
    if equals and IDENTIFIER.fullmatch(name):
        operation = OPERATION.match(value)
        return leaves(value[operation.end():] if operation else value)
    return [text]


def names_window(leaf):
    return re.split(r'[/\\]', leaf)[-1] in (WINDOW, WINDOW + '.exe')


def window_mentions(test):
    """The words a program runs from that name the window, once for each part
    that does."""
    return [word for word in test.run_words() for leaf in leaves(word) if names_window(leaf)]


def path_pattern(path):
    """A program's path named in a word, as a path, not as the start of a longer name."""
    return re.compile(re.escape(path) + r'(?:\.exe)?' + AFTER_PATH)


def runs_window(test, program):
    """Whether the test runs the window: names `program` (a path_pattern)
    anywhere a program runs from, or the window as a whole part of a word."""
    words = test.run_words()
    return any(program.search(word) for word in words) or bool(window_mentions(test))


def how_it_runs(test):
    """'direct' when the command is the window, 'app' when a word of the
    command is -DAPP=<the window>, else 'other' (cmake -E env, a property)."""
    words = [argument.value for argument in test.add.arguments[1:]]
    if words and names_window(words[0]):
        return 'direct'
    for word in words:
        match = D_VARIABLE.match(word)
        if match and match.group(1) == 'APP' and names_window(match.group(2)):
            return 'app'
    return 'other'


# ---- the rewrite ----------------------------------------------------------------------

class Plan:
    def __init__(self):
        self.window = []  # the window Tests, in the order read
        self.providers = []  # carried: they set up or clean up what the window tests require
        self.users = []  # carried: they require what a window test sets up
        self.refusals = []
        self.how = {'app': 0, 'direct': 0, 'other': 0}
        self.texts = {}  # run -> the CTestTestfile.cmake to write
        self.read = 0
        self.written = 0


HEADER = ('# Written by tools/rust_window_gate.py from the CTest files of {build}: its window\n'
          '# tests with the window {what}. Rewritten at every run; do not edit.\n')


def wrapper(output, run):
    return output.rstrip('/') + '/' + run + '/' + WINDOW


def window_text(test, replace):
    """The window test as it will be written for a run: its commands with the
    window replaced, then its ENVIRONMENT_MODIFICATION again with the test's
    name for the wrapper added - the last one set is the one ctest takes, so
    the test's own variables are all kept: (text, how many were replaced
    where a program runs from)."""
    text, replaced = ctest_tree.rebuilt(test, replace)
    old = test.values.get('ENVIRONMENT_MODIFICATION', '')
    value = (old + ';' if old else '') + f'{TEST_VARIABLE}=set:{test.name}'
    line, _ = replace(ctest_tree.quoted(value))
    text += f'set_tests_properties({test.add.arguments[0].raw} PROPERTIES ENVIRONMENT_MODIFICATION {line})\n'
    return text, replaced


def plan_rewrite(build, tree, cpp_bin, output):
    plan = Plan()
    tests = tree.tests
    cpp_window = path_pattern(cpp_bin.rstrip('/') + '/' + WINDOW)
    plan.window = [test for test in tests.values() if runs_window(test, cpp_window)]
    plan.read = sum(1 for test in tree.definitions if runs_window(test, cpp_window))
    window_names = {test.name for test in plan.window}
    made = set()
    for test in plan.window:
        made.update(test.fixtures('FIXTURES_SETUP'))
    plan.users = [test for test in tests.values()
                  if test.name not in window_names and made & set(test.fixtures('FIXTURES_REQUIRED'))]
    plan.providers = ctest_tree.fixture_providers(tests, plan.window + plan.users)
    carried = {test.name for test in plan.users} | {test.name for test in plan.providers}

    for run in RUNS:
        target = wrapper(output, run)

        def replace(raw, target=target):
            return ctest_tree.replace_programs(raw, cpp_bin, (WINDOW,), lambda name: target)

        what = ('replaced by the stream wrapper over the Rust katana, ' + target if run == 'rust'
                else 'the C++\'s, run through the stream wrapper ' + target + ', for --rerun-cpp')
        parts = [HEADER.format(build=build, what=what)]
        for test in tests.values():
            if test.name in carried:
                # Carried as the C++ suite runs it (the top of this file says why).
                parts.append(ctest_tree.rebuilt(test, None)[0])
                continue
            if test.name not in window_names:
                continue
            text, replaced = window_text(test, replace)
            parts.append(text)
            if run != 'rust':
                continue
            mentioned = window_mentions(test)
            if len(mentioned) > replaced:
                plan.refusals.append(
                    f'{test.name} runs {WINDOW} where the gate replaces nothing ({"; ".join(mentioned)}): a test '
                    f'that runs the window must name it as {cpp_bin.rstrip("/")}/{WINDOW}')
            for other in ctest_tree.other_programs(test, cpp_bin, (WINDOW,)):
                plan.refusals.append(
                    f'{test.name} names {other}, a program of the C++ build the gate does not replace')
        plan.texts[run] = ''.join(parts)
    for test in plan.window:
        plan.how[how_it_runs(test)] += 1

    # The rewrite read back: every window test read must be there, once.
    written = ctest_tree.Tree()
    written.take(plan.texts['rust'], output)
    rust_window = path_pattern(wrapper(output, 'rust'))
    plan.written = len({test.name for test in written.definitions if runs_window(test, rust_window)})
    if plan.written != plan.read:
        twice = set(tree.twice)
        doubled = sorted({test.name for test in tree.definitions
                          if test.name in twice and runs_window(test, cpp_window)})
        detail = ''
        if doubled:
            detail = ': ' + ', '.join(doubled) + (' is' if len(doubled) == 1 else ' are') + ' defined more than once'
        plan.refusals.append(
            f'the count of window tests written ({plan.written}) differs from the count read ({plan.read}){detail}')
    return plan


def plural(count, word):
    return f'{count} {word}{"" if count == 1 else "s"}'


def listed(tests):
    return ' (' + ', '.join(test.name for test in tests) + ')' if tests else ''


# ---- the normalisations (the top of this file says why each) --------------------------

# (b) Whole lines of Qt's own words. The first is check_screenshot.cmake's
# list, as it drops them; the rest are the toolkit's notes on its paths,
# fonts and platform.
QT_LINES = (
    re.compile(r'This plugin does not support (propagateSizeHints\(\)|raise\(\)|lower\(\)|'
               r'setting window (opacity|masks)|grabbing the (keyboard|mouse))\r?$'),
    re.compile(r'QStandardPaths:'),
    re.compile(r'Fontconfig'),
    re.compile(r'qt\.qpa\.'),
    re.compile(r'QFont::'),
)

# (d) (pattern, replacement, reason) for a pixel measure that the two
# toolkits make differ. Empty until a run shows one; covered=N is the
# expected first, added with the run that shows it.
PIXEL_MEASURES = []


def is_absolute(path):
    return path.startswith('/') or re.match(r'[A-Za-z]:[/\\]', path) is not None


def test_paths(test, cpp_bin, programs, output, checkout):
    """(path, placeholder) pairs for normalisation (a), in the order replaced."""
    # Each program's folder as given and as the program itself would report
    # it (/proc/self/exe, its links resolved).
    folders = []
    for folder in (cpp_bin, programs):
        folders += [folder.rstrip('/'), os.path.realpath(folder)]
    folders += [output.rstrip('/') + '/cpp', output.rstrip('/') + '/rust']
    programs_paths = []
    for folder in folders:
        for suffix in ('.exe', ''):
            programs_paths.append(folder + '/' + WINDOW + suffix)
    own = []
    words = [argument.value for argument in test.add.arguments[1:]]
    variables = {}
    for word in words:
        match = D_VARIABLE.match(word)
        if match:
            variables[match.group(1)] = match.group(2)
    if how_it_runs(test) == 'direct':
        for word in words[1:]:
            if is_absolute(word):
                own += [word, os.path.dirname(word)]
    else:
        result = variables.get('OUTPUT')
        if result:
            work = os.path.dirname(result)
            stem = os.path.basename(result).split('.', 1)[0]  # CMake's NAME_WE
            own += [work + '/' + stem + '_project', result, work]
        own += [variables[key] for key in ('CUSTOMISE_DIR', 'IMPORT', 'PROJECT') if variables.get(key)]
    own += folders
    if checkout:
        own.append(checkout)
    own = sorted({path.rstrip('/') for path in own if is_absolute(path) and len(path.rstrip('/')) > 1},
                 key=lambda path: (-len(path), path))
    pairs = []
    for paths, placeholder in ((programs_paths, '<katana>'), (own, '<path>')):
        for path in paths:
            pairs.append((path, placeholder))
            native = path.replace('/', '\\')
            if native != path:
                pairs.append((native, placeholder))
    return pairs


def normalised(data, paths):
    """A stream's lines after the normalisations (a) to (d)."""
    text = data.decode('utf-8', 'surrogateescape')
    text = '\n'.join(line for line in text.split('\n') if not any(qt.match(line) for qt in QT_LINES))
    for path, placeholder in paths:
        text = text.replace(path, placeholder)
    text = ctest_tree.SECONDS.sub('seconds=<s>', text)
    text = ctest_tree.SCRATCH.sub(r'katana-scratch\1<process>', text)
    for pattern, replacement, _ in PIXEL_MEASURES:
        text = pattern.sub(replacement, text)
    return text.split('\n')


# ---- the streams kept -----------------------------------------------------------------

def kept_runs(folder):
    """{n: {'out', 'err', 'exit', 'png'}} the wrapper kept for a test, or None
    when it kept nothing (the window never ran in it)."""
    if not os.path.isdir(folder):
        return None
    runs = {}
    for entry in os.listdir(folder):
        match = re.fullmatch(r'([0-9]+)\.args', entry)
        if not match:
            continue
        number = int(match.group(1))
        base = os.path.join(folder, match.group(1))
        run = {}
        for key in ('out', 'err'):
            try:
                with open(f'{base}.{key}', 'rb') as stream:
                    run[key] = stream.read()
            except OSError:
                run[key] = b''
        try:
            with open(base + '.exit', encoding='utf-8') as stream:
                run['exit'] = stream.read().strip()
        except OSError:
            run['exit'] = 'unknown (the wrapper did not finish)'
        run['png'] = base + '.png' if os.path.isfile(base + '.png') else None
        runs[number] = run
    return runs


def compare(name, cpp_runs, rust_runs, paths):
    """The lines that say how the two runs' streams differ; none when they do not."""
    lines = []
    if len(cpp_runs) != len(rust_runs):
        lines.append(f'ran the window {plural(len(rust_runs), "time")} where the C++ ran it '
                     f'{plural(len(cpp_runs), "time")}')
    for number in sorted(set(cpp_runs) & set(rust_runs)):
        cpp, rust = cpp_runs[number], rust_runs[number]
        if cpp['exit'] != rust['exit']:
            lines.append(f'run {number}: exit {rust["exit"]} where the C++\'s was {cpp["exit"]}')
        for key, stream in (('out', 'standard output'), ('err', 'standard error')):
            cpp_lines, rust_lines = normalised(cpp[key], paths), normalised(rust[key], paths)
            if cpp_lines != rust_lines:
                lines += ctest_tree.difference(f'{name} run {number} {stream}', cpp_lines, rust_lines)
    return lines


# ---- the looks ------------------------------------------------------------------------

def png_size(path):
    """(width, height) from a PNG's header, or None."""
    try:
        with open(path, 'rb') as stream:
            head = stream.read(24)
    except OSError:
        return None
    if len(head) < 24 or head[:8] != b'\x89PNG\r\n\x1a\n':
        return None
    return struct.unpack('>II', head[16:24])


PAGE_STYLE = '''
:root { color-scheme: light dark; --page: #ffffff; --ink: #1b1f24; --muted: #57606a; --rule: #d0d7de;
        --bad: #b42318; }
@media (prefers-color-scheme: dark) {
  :root { --page: #0d1117; --ink: #e6edf3; --muted: #8b949e; --rule: #30363d; --bad: #f97066; }
}
body { background: var(--page); color: var(--ink); font: 14px/1.45 system-ui, sans-serif; margin: 16px; }
h1 { font-size: 20px; margin: 0 0 4px; }
h2 { font-size: 15px; margin: 0 0 4px; font-family: ui-monospace, monospace; overflow-wrap: anywhere; }
section { border-top: 1px solid var(--rule); padding: 12px 0; }
.pair { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
figure { margin: 0; min-width: 0; }
img { display: block; max-width: 100%; height: auto; border: 1px solid var(--rule); }
figcaption, .verdict { color: var(--muted); font-size: 12px; }
.bad { color: var(--bad); }
.none { border: 1px dashed var(--rule); padding: 24px; text-align: center; color: var(--muted); }
'''


def write_looks(output, entries, summary):
    """The looks page: entries are (test name, C++ verdict, Rust verdict,
    streams word, attention, [(n, cpp png or None, rust png or None)])."""
    looks = os.path.join(output, 'looks')
    shutil.rmtree(looks, ignore_errors=True)
    os.makedirs(looks)
    sections = []
    lines = []
    ordered = [entry for entry in entries if entry[4]] + [entry for entry in entries if not entry[4]]
    for name, cpp_verdict, rust_verdict, streams, attention, pictures in ordered:
        folder = folder_name(name)
        figures = []
        for number, cpp_png, rust_png in pictures:
            pair = []
            texts = []
            for run, source in (('cpp', cpp_png), ('rust', rust_png)):
                label = 'C++' if run == 'cpp' else 'Rust'
                if source is None:
                    pair.append(f'<figure><div class="none">no picture from the {label} run</div></figure>')
                    texts.append(f'{run}=none')
                    continue
                file = f'{run}.png' if number == 1 else f'{run}-{number}.png'
                os.makedirs(os.path.join(looks, folder), exist_ok=True)
                shutil.copyfile(source, os.path.join(looks, folder, file))
                size = png_size(source)
                pixels = f'{size[0]} x {size[1]}' if size else 'not a PNG'
                count = os.path.getsize(source)
                link = html.escape(f'{folder}/{file}')
                pair.append(f'<figure><a href="{link}"><img src="{link}" loading="lazy" alt="{label}"></a>'
                            f'<figcaption>{label}, run {number}: {pixels} pixels, {count:,} bytes</figcaption></figure>')
                texts.append(f'{run}={folder}/{file} ({pixels}, {count} bytes)')
            figures.append('<div class="pair">' + ''.join(pair) + '</div>')
            lines.append(f'{name} run {number}: {" ".join(texts)}; C++ {cpp_verdict}; Rust {rust_verdict}; '
                         f'streams {streams}')
        mark = ' class="bad"' if attention else ''
        sections.append(f'<section id="{html.escape(folder)}"><h2{mark}>{html.escape(name)}</h2>'
                        f'<p class="verdict">C++: {html.escape(cpp_verdict)} &middot; Rust: '
                        f'{html.escape(rust_verdict)} &middot; streams: {html.escape(streams)}</p>'
                        + ''.join(figures) + '</section>')
    page = ('<!doctype html>\n<html lang="en"><head><meta charset="utf-8">'
            '<meta name="viewport" content="width=device-width, initial-scale=1">'
            '<title>Window gate looks</title><style>' + PAGE_STYLE + '</style></head><body>'
            '<h1>The window gate: the C++ and the Rust window, side by side</h1>'
            f'<p class="verdict">{html.escape(summary)}. Written by tools/rust_window_gate.py; the gate '
            'compares the streams, never these pictures. A test that failed, or whose streams differ, comes first.</p>'
            + ''.join(sections) + '</body></html>\n')
    with open(os.path.join(looks, 'index.html'), 'w', encoding='utf-8', newline='\n') as stream:
        stream.write(page)
    with open(os.path.join(looks, 'index.txt'), 'w', encoding='utf-8', newline='\n') as stream:
        stream.write(f'# {summary}\n# test run n: the C++ and the Rust picture (paths below this folder); '
                     'the verdicts; the streams\n')
        stream.write(''.join(line + '\n' for line in lines))
    return os.path.join(looks, 'index.html'), len(entries)


# ---- the run ----------------------------------------------------------------------------

def write_wrapper(output, run, python, program):
    """The run's launcher at <output>/<run>/katana: window_streams.py over `program`."""
    folder = os.path.join(output, run)
    os.makedirs(folder, exist_ok=True)
    streams = os.path.join(folder, 'streams')
    shutil.rmtree(streams, ignore_errors=True)
    path = wrapper(output, run)
    with open(path, 'w', encoding='utf-8', newline='\n') as stream:
        stream.write('#!/bin/sh\n'
                     f'# Written by tools/rust_window_gate.py: the window of its {run} run, {program},\n'
                     '# its streams kept by tools/window_streams.py. Rewritten at every run; do not edit.\n'
                     f'exec {shlex.quote(python)} -I {shlex.quote(STREAMS_TOOL)} --program {shlex.quote(program)} '
                     f'--streams {shlex.quote(streams)} -- "$@"\n')
    os.chmod(path, 0o755)


def main():
    parser = argparse.ArgumentParser(description='The headless window tests against the Rust window, '
                                     'and both programs\' streams compared (the top of this file).')
    parser.add_argument('--build', required=True, help='the C++ build tree whose CTest files are read')
    parser.add_argument('--programs', help='the folder of the Rust katana')
    parser.add_argument('--cpp-bin', help='the C++ programs\' folder as the CTest files name it')
    parser.add_argument('--output', help='the gate\'s folder (default <build>/rust_window_gate)')
    parser.add_argument('--python', default=sys.executable, help='the interpreter the stream wrapper runs under')
    parser.add_argument('--ctest', default='ctest')
    parser.add_argument('--parallel', type=int, default=14)
    parser.add_argument('--check', action='store_true', help='check the rewrite; write and run nothing')
    parser.add_argument('--rerun-cpp', action='store_true', help='run the C++ window tests again first')
    parser.add_argument('--compare-only', action='store_true', help='compare the streams kept as they are')
    parser.add_argument('ctest_arguments', nargs='*', help='after --: passed to ctest')
    arguments = parser.parse_args()

    build = arguments.build
    cpp_bin = arguments.cpp_bin or ctest_tree.default_cpp_bin(build)
    programs = arguments.programs or ctest_tree.default_programs(build)
    output = arguments.output or os.path.join(build, 'rust_window_gate')
    if not ctest_tree.SAFE_PATH.match(output):
        print(f'rust_window_gate: refused: the gate\'s folder {output!r} holds a character a CTest file '
              f'would have to escape; give one of letters, digits and / . _ - + , : ~ % =')
        return 2
    if not os.path.isfile(os.path.join(build, 'CTestTestfile.cmake')):
        print(f'rust_window_gate: refused: {build} has no CTestTestfile.cmake; name a configured C++ build tree')
        return 2
    try:
        tree = ctest_tree.read_tree(build)
        plan = plan_rewrite(build, tree, cpp_bin, output)
    except CMakeError as error:
        print(f'rust_window_gate: refused: a CTest file cannot be read: {error}')
        return 2

    for refusal in plan.refusals:
        print(f'rust_window_gate: refused: {refusal}')
    print(f'rust_window_gate: {plan.read} window tests read, {plan.written} written '
          f'({plan.how["app"]} through -DAPP=, {plan.how["direct"]} run directly, {plan.how["other"]} otherwise); '
          f'carried from the C++ suite as it is: {plural(len(plan.providers), "fixture provider")}'
          f'{listed(plan.providers)} and {plural(len(plan.users), "fixture user")}{listed(plan.users)}')
    if plan.refusals:
        return 2
    programs_by_run = {'cpp': cpp_bin.rstrip('/') + '/' + WINDOW, 'rust': os.path.join(programs, WINDOW)}
    if not os.path.isfile(programs_by_run['rust']):
        print(f'rust_window_gate: warning: {programs_by_run["rust"]} is not there: the window tests will fail')
    if arguments.rerun_cpp and not os.path.isfile(programs_by_run['cpp']):
        print(f'rust_window_gate: warning: {programs_by_run["cpp"]} is not there: the C++ rerun will fail')
    if arguments.check:
        return 0

    exits = {}
    if not arguments.compare_only:
        order = ('cpp', 'rust') if arguments.rerun_cpp else ('rust',)
        for run in order:
            write_wrapper(output, run, arguments.python, programs_by_run[run])
            exits[run] = ctest_tree.run_ctest(arguments.ctest, os.path.join(output, run), plan.texts[run],
                                              arguments.parallel, arguments.ctest_arguments, 'rust_window_gate')

    results = {run: ctest_tree.read_log(os.path.join(output, run, 'Testing', 'Temporary', 'LastTest.log'))
               for run in RUNS}
    checkout = ctest_tree.source_dir(build)
    same = 0
    failed = []
    differing = []
    not_compared = []
    entries = []
    for test in plan.window:
        if test.name not in results['rust']:
            continue  # not in this run (ctest's selection); ctest says why
        verdict = results['rust'][test.name][1]
        cpp_verdict = results['cpp'].get(test.name, (None, ''))[1]
        if not ctest_tree.passed(verdict):
            also = ' (the C++ run failed it too)' if cpp_verdict and not ctest_tree.passed(cpp_verdict) else ''
            failed.append(f'{test.name}: {verdict or "no verdict"}{also}')
        folder = folder_name(test.name)
        rust_runs = kept_runs(os.path.join(output, 'rust', 'streams', folder)) or {}
        cpp_runs = kept_runs(os.path.join(output, 'cpp', 'streams', folder))
        if test.name not in results['cpp']:
            not_compared.append(f'{test.name}: the C++ run has no record of it (--rerun-cpp runs it)')
            streams = 'not compared'
            cpp_runs = cpp_runs or {}
        else:
            cpp_runs = cpp_runs or {}
            lines = compare(test.name, cpp_runs, rust_runs,
                            test_paths(test, cpp_bin, programs, output, checkout))
            if lines:
                differing.append((test.name, lines))
                streams = 'different'
            else:
                same += 1
                streams = 'the same'
        numbers = sorted(number for number in set(cpp_runs) | set(rust_runs)
                         if cpp_runs.get(number, {}).get('png') or rust_runs.get(number, {}).get('png'))
        if numbers:
            pictures = [(number, cpp_runs.get(number, {}).get('png'), rust_runs.get(number, {}).get('png'))
                        for number in numbers]
            attention = not ctest_tree.passed(verdict) or streams != 'the same'
            entries.append((test.name, cpp_verdict or 'no verdict', verdict or 'no verdict', streams, attention,
                            pictures))

    for line in failed:
        print(f'rust_window_gate: failed: {line}')
    for name, lines in differing:
        print(f'rust_window_gate: the streams differ: {name}')
        for line in lines:
            print('    ' + line)
    for line in not_compared:
        print(f'rust_window_gate: not compared: {line}')
    ran = same + len(differing) + len(not_compared)
    tested = exits.get('rust', 0)
    summary = (f'{ran} window tests ran: {len(failed)} failed; streams {same} the same as the C++\'s, '
               f'{len(differing)} different, {len(not_compared)} not compared')
    page, pairs = write_looks(output, entries, summary)
    ctest_exits = f'ctest exit {tested}' + (f', the C++ rerun\'s {exits["cpp"]}' if 'cpp' in exits else '')
    print(f'rust_window_gate: {summary} ({ctest_exits}); looks: {plural(pairs, "test")} with pictures, {page}')
    return 1 if tested != 0 or failed or differing or not_compared else 0


if __name__ == '__main__':
    sys.exit(main())
