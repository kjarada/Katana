#!/usr/bin/env python3
# tools/window_streams.py --program <katana> --streams <folder> -- <arguments...>
#
# The window gate's stream wrapper (tools/rust_window_gate.py, "The streams"):
# it stands where a headless test runs the window, runs the real program - the
# C++ build's bin/katana or cargo's release/katana - with the same arguments,
# passes its standard output and error through unchanged, and keeps a copy of
# each apart. Not for a person; the gate writes a small launcher per run that
# calls it, and puts the launcher's path in the rewritten tests in place of
# <cpp-bin>/katana.
#
# Why it exists: tools/check_screenshot.cmake keeps the program's output in
# variables and prints it only when the check fails, so ctest's log holds a
# PNG's size where the window's own words were; and a test that runs the
# window directly gets both streams in one pipe. Kept apart here, in both
# runs alike, the two programs' streams can be compared stream by stream.
#
# WHAT IT WRITES, in <folder>/<test>/, <test> being the KATANA_GATE_TEST the
# rewrite adds to each window test's environment ("unnamed" without one; a
# character outside letters, digits, '_', '.' and '-' becomes '_'), for the
# <n>th run of the window in that test (the sheets checks run it several
# times; n counts from 1, claimed with an exclusive create, so two runs
# cannot take one number):
#   <n>.args  JSON: the test, the program, the arguments, the working folder;
#   <n>.out   the program's standard output, byte for byte;
#   <n>.err   its standard error, byte for byte;
#   <n>.exit  its exit code, "signal <s>" when a signal ended it, or
#             "not started" when it could not be started;
#   <n>.png   a copy of the PNG its --screenshot wrote, when this run wrote one
#             (new, or changed since the run began): the looks page's picture.
#
# HOW IT ENDS: with the program's exit code; when a signal ended the program,
# the wrapper ends itself with the same signal, so the check script sees what
# it would have seen (a crash is still a crash). A program that cannot be
# started is said on standard error and ends the wrapper with 127, as a shell
# does. KATANA_GATE_TEST is taken out of the program's environment: the
# program sees the test's environment and nothing of the gate's.
#
# On Linux the program is made to die with the wrapper (PR_SET_PDEATHSIG):
# when a timeout kills the wrapper - check_screenshot.cmake's 120 seconds,
# ctest's TIMEOUT - the window must not run on without it.
#
# LIMIT: when the two streams reach one pipe (ctest runs the program
# directly), the wrapper writes each piece as it arrives, from two threads;
# two pieces the program wrote to the two streams within a moment of each
# other can reach the pipe in the other order than it wrote them. A check
# script's execute_process keeps the streams apart, where order between them
# does not exist.

import json
import os
import re
import shutil
import signal
import subprocess
import sys
import threading

TEST_VARIABLE = 'KATANA_GATE_TEST'
CHUNK = 65536


def folder_name(name):
    return re.sub(r'[^A-Za-z0-9_.-]', '_', name) if name else 'unnamed'


def claim(folder):
    """The first run number free in the folder, claimed by creating its .args."""
    number = 1
    while True:
        try:
            descriptor = os.open(os.path.join(folder, f'{number}.args'), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
            return number, descriptor
        except FileExistsError:
            number += 1


def screenshot_of(arguments):
    """The path given to --screenshot, made absolute, or None."""
    for index, argument in enumerate(arguments[:-1]):
        if argument == '--screenshot':
            return os.path.abspath(arguments[index + 1])
    return None


def stamp(path):
    try:
        status = os.stat(path)
    except OSError:
        return None
    return (status.st_ino, status.st_size, status.st_mtime_ns)


def die_with_parent():
    """A preexec_fn for Linux: the child gets SIGKILL when the wrapper dies."""
    if not sys.platform.startswith('linux'):
        return None
    try:
        import ctypes
        prctl = ctypes.CDLL(None, use_errno=True).prctl
    except (OSError, AttributeError):
        return None
    parent = os.getpid()

    def before_exec():
        prctl(1, signal.SIGKILL)  # PR_SET_PDEATHSIG is 1 (linux/prctl.h)
        # The wrapper may have died between the fork and the call above.
        if os.getppid() != parent:
            os._exit(1)

    return before_exec


def write_all(descriptor, data):
    while data:
        written = os.write(descriptor, data)
        data = data[written:]


def pump(source, descriptor, kept):
    """Everything from the program's pipe to our own stream and to the copy."""
    forwarding = True
    while True:
        chunk = os.read(source.fileno(), CHUNK)
        if not chunk:
            break
        kept.write(chunk)
        kept.flush()
        if forwarding:
            try:
                write_all(descriptor, chunk)
            except OSError:
                # Whoever read our stream has gone; the copy is still kept,
                # and the program is not stopped by it.
                forwarding = False


def main(argv):
    if len(argv) < 5 or argv[0] != '--program' or argv[2] != '--streams' or argv[4] != '--':
        sys.stderr.write('window_streams: called wrongly: --program <katana> --streams <folder> -- <arguments>\n')
        return 2
    program, streams, arguments = argv[1], argv[3], argv[5:]
    name = os.environ.get(TEST_VARIABLE, '')
    environment = dict(os.environ)
    environment.pop(TEST_VARIABLE, None)
    folder = os.path.join(streams, folder_name(name))
    os.makedirs(folder, exist_ok=True)
    number, descriptor = claim(folder)
    with os.fdopen(descriptor, 'w', encoding='utf-8') as stream:
        json.dump({'test': name, 'program': program, 'arguments': arguments, 'directory': os.getcwd()},
                  stream, indent=1)
        stream.write('\n')
    base = os.path.join(folder, str(number))
    screenshot = screenshot_of(arguments)
    before = stamp(screenshot) if screenshot else None

    with open(base + '.out', 'wb') as out_kept, open(base + '.err', 'wb') as err_kept:
        try:
            child = subprocess.Popen([program] + arguments, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     env=environment, preexec_fn=die_with_parent())
        except OSError as error:
            message = f'window_streams: cannot run {program}: {error.strerror or error}\n'.encode()
            err_kept.write(message)
            with open(base + '.exit', 'w', encoding='utf-8') as stream:
                stream.write('not started\n')
            write_all(2, message)
            return 127
        threads = [threading.Thread(target=pump, args=(child.stdout, 1, out_kept)),
                   threading.Thread(target=pump, args=(child.stderr, 2, err_kept))]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        code = child.wait()

    with open(base + '.exit', 'w', encoding='utf-8') as stream:
        stream.write(f'signal {-code}\n' if code < 0 else f'{code}\n')
    if screenshot:
        after = stamp(screenshot)
        if after is not None and after != before:
            try:
                shutil.copyfile(screenshot, base + '.png')
            except OSError:
                pass  # the picture is for looking; the run's verdict is the test's
    if code < 0:
        # Ended by a signal: end the same way, so the caller sees it.
        try:
            signal.signal(-code, signal.SIG_DFL)
        except (OSError, ValueError):
            pass
        os.kill(os.getpid(), -code)
        return 128 - code
    return code


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
