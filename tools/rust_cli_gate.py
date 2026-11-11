#!/usr/bin/env python3
# tools/rust_cli_gate.py --build <C++ build tree> [options] [-- ctest arguments]
#
# The gate of the Rust port's step 6 (docs/rust.md; the step 6 plan, section
# 4.6): every cli.* test of the C++ build runs against the Rust katana_cli and
# katana_mcp, through a rewritten copy of the C++ build's own CTest files, and
# each test's output is compared with what the C++ programs printed. Its CTest
# reader is tools/ctest_tree.py, shared with step 7's window gate
# (tools/rust_window_gate.py), so that the two gates read a CTest file alike.
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
import os
import sys

sys.dont_write_bytecode = True  # a check leaves nothing behind in the tools folder
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ctest_tree  # noqa: E402 - the shared reader, beside this file
from ctest_tree import CMakeError, read_commands  # noqa: E402

# ---- the rewrite ----------------------------------------------------------------------

PROGRAMS = ('katana_cli', 'katana_mcp')
# A program named as a path's last part, or alone (found on PATH), in a word
# of a command: after the start, a separator, '=' (-DCLI=) or ';' (a list),
# and before the end, a list's ';' or a blank.
MENTION = ctest_tree.mention_pattern(PROGRAMS)


def is_cli(test):
    return test.name.startswith('cli.')


class Plan:
    def __init__(self):
        self.cli = []  # cli Tests, in the order read
        self.providers = []  # non-cli fixture providers carried as they are
        self.refusals = []
        self.no_program = []
        self.text = ''
        self.cpp_text = ''
        self.written = 0


HEADER = ('# Written by tools/rust_cli_gate.py from the CTest files of {build}: its cli tests\n'
          '# with {what}. Rewritten at every run; do not edit.\n')


def plan_rewrite(build, tests, cli_read, twice, cpp_bin, programs):
    plan = Plan()
    plan.cli = [test for test in tests.values() if is_cli(test)]
    # The fixtures the cli tests require, and the tests that are not cli
    # tests among their providers, to their own fixtures' providers.
    plan.providers = ctest_tree.fixture_providers(tests, plan.cli)
    included = {test.name for test in plan.cli} | {test.name for test in plan.providers}

    def replace(raw):
        return ctest_tree.replace_programs(raw, cpp_bin, PROGRAMS,
                                           lambda name: programs.rstrip('/') + '/' + name)

    rust = [HEADER.format(build=build, what='katana_cli and katana_mcp the Rust\'s, in ' + programs)]
    cpp = [HEADER.format(build=build, what='the C++ programs as they are, for --rerun-cpp')]
    for test in tests.values():
        if test.name not in included:
            continue
        if not is_cli(test):
            # Carried as the C++ suite runs it (the top of this file says why).
            text, _ = ctest_tree.rebuilt(test, None)
            rust.append(text)
            cpp.append(text)
            continue
        text, replaced = ctest_tree.rebuilt(test, replace)
        named = ctest_tree.mentions(test, MENTION)
        if len(named) > replaced:
            plan.refusals.append(
                f'{test.name} runs {MENTION.search(named[0]).group(1)} where the gate replaces nothing '
                f'({"; ".join(named)}): a test that runs a program must name it as '
                f'{cpp_bin.rstrip("/")}/katana_cli or katana_mcp')
        for other in ctest_tree.other_programs(test, cpp_bin, PROGRAMS):
            plan.refusals.append(
                f'{test.name} names {other}, a program of the C++ build the gate does not replace')
        if not named:
            plan.no_program.append(test.name)
        rust.append(text)
        cpp.append(ctest_tree.rebuilt(test, None)[0])
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

ERROR_STREAM = ('error: ', 'warning: ', 'note: ')


def normalised(output, paths):
    """The output's standard-output lines and standard-error lines, each in
    order, after the normalisations at the top of this file."""
    for path, placeholder in paths:
        output = output.replace(path, placeholder)
    output = ctest_tree.SECONDS.sub('seconds=<s>', output)
    output = ctest_tree.SCRATCH.sub(r'katana-scratch\1<process>', output)
    lines = output.split('\n')
    return ([line for line in lines if not line.startswith(ERROR_STREAM)],
            [line for line in lines if line.startswith(ERROR_STREAM)])


# ---- the run ----------------------------------------------------------------------------

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
    cpp_bin = arguments.cpp_bin or ctest_tree.default_cpp_bin(build)
    programs = arguments.programs or ctest_tree.default_programs(build)
    output = arguments.output or os.path.join(build, 'rust_cli_gate')
    if not ctest_tree.SAFE_PATH.match(programs):
        print(f'rust_cli_gate: refused: the Rust programs\' folder {programs!r} holds a character a '
              f'CTest file would have to escape; give one of letters, digits and / . _ - + , : ~ % =')
        return 2
    if not os.path.isfile(os.path.join(build, 'CTestTestfile.cmake')):
        print(f'rust_cli_gate: refused: {build} has no CTestTestfile.cmake; name a configured C++ build tree')
        return 2
    try:
        tree = ctest_tree.read_tree(build)
        cli_read = sum(1 for test in tree.definitions if is_cli(test))
        plan = plan_rewrite(build, tree.tests, cli_read, tree.twice, cpp_bin, programs)
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
            ctest_tree.run_ctest(arguments.ctest, os.path.join(output, 'cpp'), plan.cpp_text, arguments.parallel,
                                 arguments.ctest_arguments, 'rust_cli_gate')
        tested = ctest_tree.run_ctest(arguments.ctest, output, plan.text, arguments.parallel,
                                      arguments.ctest_arguments, 'rust_cli_gate')
    if arguments.rerun_cpp:
        cpp_log = os.path.join(output, 'cpp', 'Testing', 'Temporary', 'LastTest.log')

    cpp_results = ctest_tree.read_log(cpp_log)
    rust_results = ctest_tree.read_log(os.path.join(output, 'Testing', 'Temporary', 'LastTest.log'))
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
        if not ctest_tree.passed(verdict):
            cpp_verdict = cpp_results.get(test.name, (None, ''))[1]
            also = ' (the C++ run failed it too)' if cpp_verdict and not ctest_tree.passed(cpp_verdict) else ''
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
            lines += ctest_tree.difference(f'{test.name} standard output', cpp_out, rust_out)
        if cpp_err != rust_err:
            lines += ctest_tree.difference(f'{test.name} standard error', cpp_err, rust_err)
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
