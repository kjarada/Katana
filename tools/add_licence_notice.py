#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (c) 2026 Jarada. Additional terms under GNU GPL version 3, section 7,
# apply: see ADDITIONAL_TERMS.md in https://github.com/kjarada/Katana
# tools/add_licence_notice.py [--apply | --check] [--self-test] [checkout-root]
#
# Puts the licence notice of ADDITIONAL_TERMS.md at the top of every tracked
# source file that lacks it. The GPL's section 7 asks anyone who adds terms to
# place, "in the relevant source files, a statement of the additional terms
# that apply to those files, or a notice indicating where to find the
# applicable terms"; this is how that is done for a thousand and more files
# without editing each by hand.
#
#   no flag     dry run: lists what it would change, changes nothing
#   --apply     writes the notice
#   --check     changes nothing, exits 1 when a file lacks the notice (for CI,
#               once the notices are in)
#   --self-test runs the script's own checks on temporary files
#
# What it does and does not do:
#   * C, C++ and shader sources get the long form, in "//" comments; CMake,
#     Python and similar scripts get the short form, in "#" comments
#     (ADDITIONAL_TERMS.md says when each is meant).
#   * It is idempotent: a file whose first 40 lines hold the notice is left
#     alone, so a second run changes nothing.
#   * A file that already carries somebody else's copyright line is NOT
#     touched and is listed under "foreign": it may be a third party's work,
#     and only a person can say what notice it needs.
#   * A shebang line, a coding line and a byte-order mark stay first; the
#     file's own line endings (LF or CRLF) are kept.
#   * Data files, documents, samples, resources and anything not tracked by
#     git are never touched.
# Run it once, on the merged tree, so that it does not conflict with work in
# progress, then read `git diff --stat` before committing.

import os
import re
import subprocess
import sys
import tempfile

NOTICE_MARK = 'Additional terms under GNU GPL version 3, section 7'
SHORT_SECOND = 'apply: see ADDITIONAL_TERMS.md in https://github.com/kjarada/Katana'

LONG_BODY = [
    'Copyright (c) 2026 Jarada',
    '',
    'This program is free software: you can redistribute it and/or modify',
    'it under the terms of the GNU General Public License as published by',
    'the Free Software Foundation, either version 3 of the License, or',
    '(at your option) any later version.',
    '',
    'This program is distributed in the hope that it will be useful,',
    'but WITHOUT ANY WARRANTY; without even the implied warranty of',
    'MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the',
    'GNU General Public License for more details.',
    '',
    'You should have received a copy of the GNU General Public License',
    'along with this program.  If not, see <https://www.gnu.org/licenses/>.',
    '',
    NOTICE_MARK + ', apply: see',
    'ADDITIONAL_TERMS.md in https://github.com/kjarada/Katana',
]
SHORT_BODY = [
    'SPDX-License-Identifier: GPL-3.0-or-later',
    'Copyright (c) 2026 Jarada. ' + NOTICE_MARK + ',',
    SHORT_SECOND,
]

C_EXT = {'.cpp', '.hpp', '.h', '.cc', '.c', '.vert', '.frag', '.geom', '.glsl'}
HASH_EXT = {'.py', '.cmake'}
SKIP_TOP = {'samples', 'docs', 'resources', 'third_party', '.claude', '.github'}


def style_of(path):
    """('//' or '#', long or short form), or None when the file is not source."""
    name = os.path.basename(path)
    if name == 'CMakeLists.txt':
        return '#', False
    for suffix, prefix, long_form in (('.cmake.in', '#', False), ('.rc.in', '//', True),
                                      ('.cpp.in', '//', True), ('.hpp.in', '//', True)):
        if name.endswith(suffix):
            return prefix, long_form
    ext = os.path.splitext(name)[1].lower()
    if ext in C_EXT:
        return '//', True
    if ext in HASH_EXT:
        return '#', False
    return None


def tracked_files(root):
    out = subprocess.run(['git', '-C', root, 'ls-files', '-z'], capture_output=True, check=True).stdout
    return [p for p in out.decode('utf-8').split('\0') if p]


def header_text(rel, prefix, long_form, eol):
    body = ([rel] + LONG_BODY) if long_form else SHORT_BODY
    lines = [(prefix + ' ' + l).rstrip() for l in body]
    return (eol.join(lines) + eol + eol).encode('utf-8')


def split_front(data):
    """(front, rest): a byte-order mark, a shebang and a coding line stay first."""
    front = b''
    if data.startswith(b'\xef\xbb\xbf'):
        front, data = data[:3], data[3:]
    for _ in range(2):
        m = re.match(rb'(#![^\r\n]*|#[^\r\n]*coding[:=][^\r\n]*)(\r\n|\n)', data)
        if not m:
            break
        front += data[:m.end()]
        data = data[m.end():]
    return front, data


def classify(root, rel):
    """'missing', 'has', 'foreign' or 'skip' for one tracked file."""
    if rel.split('/')[0] in SKIP_TOP or style_of(rel) is None:
        return 'skip'
    with open(os.path.join(root, rel), 'rb') as f:
        data = f.read()
    if not data.strip():
        return 'skip'
    head = data.decode('latin-1').splitlines()[:40]
    text = '\n'.join(head)
    if NOTICE_MARK in text:
        return 'has'
    for line in head:
        if re.search(r'copyright|\(c\)\s*\d{4}', line, re.I) and 'Jarada' not in line:
            return 'foreign'
    return 'missing'


def apply_one(root, rel):
    prefix, long_form = style_of(rel)
    path = os.path.join(root, rel)
    with open(path, 'rb') as f:
        data = f.read()
    front, rest = split_front(data)
    eol = '\r\n' if b'\r\n' in data[:2000] else '\n'
    with open(path, 'wb') as f:
        f.write(front + header_text(rel, prefix, long_form, eol) + rest)


def run(root, mode):
    counts = {'missing': [], 'has': [], 'foreign': [], 'skip': []}
    for rel in tracked_files(root):
        counts[classify(root, rel)].append(rel)
    by_ext = {}
    for rel in counts['missing']:
        key = os.path.splitext(rel)[1] or os.path.basename(rel)
        by_ext[key] = by_ext.get(key, 0) + 1
    print('lacking the notice: %d (%s)' % (len(counts['missing']),
          ', '.join('%s %d' % kv for kv in sorted(by_ext.items())) or 'none'))
    print('already has it:     %d' % len(counts['has']))
    print('skipped (not source, or a folder this never touches): %d' % len(counts['skip']))
    print('foreign copyright line, left alone, for a person to read: %d' % len(counts['foreign']))
    for rel in counts['foreign']:
        print('  foreign: ' + rel)
    if mode == 'apply':
        for rel in counts['missing']:
            apply_one(root, rel)
        print('wrote the notice into %d files' % len(counts['missing']))
        return 0
    if mode == 'check':
        return 1 if counts['missing'] else 0
    for rel in counts['missing'][:10]:
        print('  would change: ' + rel)
    if len(counts['missing']) > 10:
        print('  ... and %d more (--apply writes them)' % (len(counts['missing']) - 10))
    return 0


def self_test():
    def git(root, *args):
        subprocess.run(['git', '-C', root] + list(args), check=True, capture_output=True)

    with tempfile.TemporaryDirectory() as root:
        git(root, 'init', '-q')
        files = {
            'src/a.cpp': b'#include <vector>\nint a() { return 1; }\n',
            'src/b.hpp': b'#pragma once\r\nint b();\r\n',
            'tools/c.py': b'#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\nprint(1)\n',
            'src/CMakeLists.txt': b'add_library(x a.cpp)\n',
            'src/bom.cpp': b'\xef\xbb\xbfint bom() { return 2; }\n',
            'src/foreign.cpp': b'// Copyright (c) 2001 Somebody Else\nint f();\n',
            'src/data.json': b'{}\n',
            'samples/s.cpp': b'int s;\n',
            'src/empty.cpp': b'',
        }
        for rel, data in files.items():
            os.makedirs(os.path.dirname(os.path.join(root, rel)), exist_ok=True)
            with open(os.path.join(root, rel), 'wb') as f:
                f.write(data)
        git(root, 'add', '.')
        assert run(root, 'check') == 1
        assert run(root, 'apply') == 0
        assert run(root, 'check') == 0, 'second run must find nothing to do'
        snap = {rel: open(os.path.join(root, rel), 'rb').read() for rel in files}
        run(root, 'apply')
        assert snap == {rel: open(os.path.join(root, rel), 'rb').read() for rel in files}, 'not idempotent'
        a = snap['src/a.cpp']
        assert a.startswith(b'// src/a.cpp\n// Copyright (c) 2026 Jarada\n')
        assert a.endswith(b'\n\n#include <vector>\nint a() { return 1; }\n')
        assert b'\r' not in a
        assert snap['src/b.hpp'].count(b'\r\n') == snap['src/b.hpp'].count(b'\n'), 'CRLF kept'
        assert snap['src/b.hpp'].endswith(b'#pragma once\r\nint b();\r\n')
        c = snap['tools/c.py']
        assert c.startswith(b'#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\n# SPDX-License-Identifier')
        assert c.endswith(b'\nprint(1)\n')
        assert snap['src/CMakeLists.txt'].startswith(b'# SPDX-License-Identifier')
        assert snap['src/bom.cpp'].startswith(b'\xef\xbb\xbf// src/bom.cpp\n')
        assert snap['src/foreign.cpp'] == files['src/foreign.cpp'], 'foreign file untouched'
        assert snap['src/data.json'] == files['src/data.json']
        assert snap['samples/s.cpp'] == files['samples/s.cpp']
        assert snap['src/empty.cpp'] == b''
    print('self-test passed')
    return 0


def main(argv):
    mode = 'dry'
    root = '.'
    for a in argv[1:]:
        if a == '--apply':
            mode = 'apply'
        elif a == '--check':
            mode = 'check'
        elif a == '--self-test':
            return self_test()
        elif a.startswith('-'):
            print('unknown option ' + a, file=sys.stderr)
            return 2
        else:
            root = a
    return run(os.path.abspath(root), mode)


if __name__ == '__main__':
    sys.exit(main(sys.argv))
