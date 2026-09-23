"""Checks that plan/ still holds the whole Katana plan, split without loss.

The plan was one file, PLAN.MD, until 2026-09-23. Its text now lives in plan/,
one file per top-level section; a section longer than about 400 lines is a
folder instead, with its lead text in 00-overview.md and one file per "## "
subsection. PLAN.MD is the index: a table naming every "#" and "##" heading
of the plan and the file it is in, so that a citation such as "PLAN.MD 45.2"
resolves in one lookup. Every file under plan/ begins with a two-line
navigation header - an HTML comment naming its place, then a blank line -
above the plan's own text, which is otherwise untouched.

usage:
  python tools/check_plan_split.py
      Structural check of the working tree, needing nothing else. Asserts
      that every file the index names exists and every .md under plan/ is
      indexed; that each file begins with the navigation header its place in
      the index implies; that the "#" and "##" headings in the files are
      exactly the index's rows, in order, each with the right Section column;
      that the top-level sections are numbered 1..N, each once, in order, at
      the top of its own file whose name starts with its zero-padded number;
      that no heading appears twice; and that no code fence is left open at
      the end of a file (a fence split across two files renders both wrong).

  python tools/check_plan_split.py --against FILE
  python tools/check_plan_split.py --against-git REV:PATH
      The structural check, then the files concatenated in index order with
      their headers removed must equal that single-file plan byte for byte.
      e39e047 is the last commit with the plan in one file, so
          python tools/check_plan_split.py --against-git e39e047:PLAN.MD
      proves the split itself lost, added and reordered nothing. It stops
      holding as soon as a section is edited, by design; the structural check
      is the one to keep running.

  python tools/check_plan_split.py --write-headers
      Rewrites every file's navigation header from the index, then checks.
      Run it after adding, removing or renaming a file: each header names the
      section count and its neighbours, so one new section changes them all.

Line endings: the repository stores LF, and a checkout with
core.autocrlf=true (the owner's) writes CRLF into the working tree, so every
input is read with CRLF taken as LF. Nothing else is normalised.

Exit status 0 when every check holds; 1 with each failure printed otherwise.
"""
import argparse
import hashlib
import os
import posixpath
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INDEX = 'PLAN.MD'
PLAN_DIR = 'plan'

# An index row: | Section | Heading | [plan/file.md](plan/file.md) |
ROW = re.compile(r'^\|\s*(?P<section>[^|]*?)\s*\|\s*(?P<heading>[^|]*?)\s*\|'
                 r'\s*\[(?P<text>[^\]]+)\]\((?P<link>plan/[^)]+)\)\s*\|\s*$')
HEADING = re.compile(r'^(#{1,2}) (.*)$')
# A top-level section heading is "# N. Title". Section 1's is "## 1. Mission":
# it was written at level 2 under the plan's title block and is kept verbatim.
TOP = re.compile(r'^#{1,2} (\d+)\. ')
SUBSECTION_NUMBER = re.compile(r'^(\d+\.\d+[a-z]?) ')
FENCE = re.compile(r'^\s*(```|~~~)')
HEADER = re.compile(r'^<!-- Katana plan, .* -->$')
NO_SECTION = '—'   # the Section column of the title block's rows


def top_section(level, text):
    """The section number if this heading opens a top-level section, else None."""
    m = TOP.match('#' * level + ' ' + text)
    return m.group(1) if m else None


def read_text(path):
    with open(path, 'rb') as f:
        return f.read().decode('utf-8').replace('\r\n', '\n')


def split_header(text):
    """(header line or None, body) - the body is the plan's own text."""
    first, _, rest = text.partition('\n')
    if HEADER.match(first) and rest.startswith('\n'):
        return first, rest[1:]
    return None, text


def headings(body):
    """The "#" and "##" headings of a body, outside code fences, with the
    count of fence lines so an unclosed fence can be reported."""
    found, fences, inside = [], 0, False
    for line in body.split('\n'):
        if FENCE.match(line):
            fences += 1
            inside = not inside
            continue
        m = None if inside else HEADING.match(line)
        if m:
            found.append((len(m.group(1)), m.group(2).rstrip()))
    return found, fences


def parse_index(errors):
    rows = []
    for number, line in enumerate(read_text(os.path.join(ROOT, INDEX)).split('\n'), 1):
        m = ROW.match(line)
        if not m:
            continue
        heading = m['heading']
        if heading.startswith('**') and heading.endswith('**'):
            heading = heading[2:-2]
        if m['text'] != m['link']:
            errors.append('%s:%d: link text %s differs from its target %s'
                          % (INDEX, number, m['text'], m['link']))
        rows.append({'section': m['section'], 'heading': heading,
                     'file': m['link'], 'line': number})
    if not rows:
        errors.append('%s: no index rows found' % INDEX)
    return rows


def ordered_files(rows, errors):
    files = []
    for row in rows:
        if not files or files[-1] != row['file']:
            if row['file'] in files:
                errors.append('%s:%d: rows for %s are not contiguous'
                              % (INDEX, row['line'], row['file']))
            files.append(row['file'])
    return files


def describe(path, body, section, total):
    """What a file's header says it is: the title block, a section, a folder's
    lead text, or a subsection."""
    found, _ = headings(body)
    level, first = found[0] if found else (1, '')
    top = top_section(level, first)
    if top:
        if posixpath.dirname(path) != PLAN_DIR:
            return 'section %s of %d, lead text before its subsections' % (top, total)
        return 'section %s of %d' % (top, total)
    if section is None:
        return 'title block before section 1 of %d' % total
    number = SUBSECTION_NUMBER.match(first)
    return 'section %s of %d, subsection %s' % (
        section, total, number.group(1) if number else first)


def expected_headers(files, bodies):
    # The highest number rather than a count, so that a duplicated section is
    # reported once as a duplicate and not again as every header being stale.
    total = max((int(n) for f in files for n in (top_section(*h) for h in headings(bodies[f])[0])
                 if n), default=0)
    result, section = {}, None
    for i, path in enumerate(files):
        here = posixpath.dirname(path)
        top = [n for n in (top_section(*h) for h in headings(bodies[path])[0]) if n]
        if top:
            section = top[0]
        prev = posixpath.relpath(files[i - 1], here) if i > 0 else 'none'
        nxt = posixpath.relpath(files[i + 1], here) if i + 1 < len(files) else 'none'
        result[path] = '<!-- Katana plan, %s. Index: %s. Previous: %s. Next: %s -->' % (
            describe(path, bodies[path], section, total),
            posixpath.relpath(INDEX, here), prev, nxt)
    return result


def load_original(args):
    if args.against:
        return read_text(args.against), args.against
    run = subprocess.run(['git', '-C', ROOT, 'show', args.against_git],
                         capture_output=True, check=False)
    if run.returncode != 0:
        sys.exit('git show %s failed: %s' % (args.against_git,
                                            run.stderr.decode('utf-8', 'replace').strip()))
    return run.stdout.decode('utf-8').replace('\r\n', '\n'), args.against_git


def first_difference(a, b):
    """(line number, line of a, line of b) at the first line that differs."""
    la = a.split('\n') + ['(end of text)']
    lb = b.split('\n') + ['(end of text)']
    for i, (x, y) in enumerate(zip(la, lb)):
        if x != y:
            return i + 1, x, y
    return len(la), '', ''


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    source = parser.add_mutually_exclusive_group()
    source.add_argument('--against', metavar='FILE')
    source.add_argument('--against-git', metavar='REV:PATH')
    parser.add_argument('--write-headers', action='store_true')
    args = parser.parse_args()

    errors = []
    rows = parse_index(errors)
    files = ordered_files(rows, errors)

    on_disk = set()
    for directory, _, names in os.walk(os.path.join(ROOT, PLAN_DIR)):
        for name in names:
            if name.endswith('.md'):
                rel = os.path.relpath(os.path.join(directory, name), ROOT)
                on_disk.add(rel.replace(os.sep, '/'))
    for orphan in sorted(on_disk - set(files)):
        errors.append('%s is not in the index' % orphan)
    missing = [f for f in files if f not in on_disk]
    for path in missing:
        errors.append('%s is in the index but does not exist' % path)
    if missing or not files:
        return report(errors)

    raw = {f: read_text(os.path.join(ROOT, f)) for f in files}
    split = {f: split_header(raw[f]) for f in files}
    bodies = {f: split[f][1] for f in files}
    wanted = expected_headers(files, bodies)

    if args.write_headers:
        for f in files:
            text = wanted[f] + '\n\n' + bodies[f]
            if text != raw[f]:
                with open(os.path.join(ROOT, f), 'w', encoding='utf-8', newline='\n') as out:
                    out.write(text)
                print('header written: ' + f)
            split[f] = (wanted[f], bodies[f])

    rows_of = {}
    for row in rows:
        rows_of.setdefault(row['file'], []).append(row)
    section, numbers, seen = NO_SECTION, [], {}
    for f in files:
        header, body = split[f]
        if header != wanted[f]:
            errors.append('%s: navigation header is\n    %s\n  expected\n    %s\n'
                          '  (python tools/check_plan_split.py --write-headers)'
                          % (f, header, wanted[f]))
        if not body.endswith('\n'):
            errors.append('%s: does not end with a newline' % f)
        found, fences = headings(body)
        if fences % 2:
            errors.append('%s: a code fence is left open at the end of the file' % f)
        if not found or not HEADING.match(body.split('\n', 1)[0]):
            errors.append('%s: the text does not begin with a heading' % f)
        indexed = [r['heading'] for r in rows_of[f]]
        if [t for _, t in found] != indexed:
            errors.append('%s: headings in the file\n    %s\n  differ from its index rows\n    %s'
                          % (f, [t for _, t in found], indexed))
        # Every heading in the file, indexed or not: one the index lacks may
        # still be a second copy of a section.
        for position, (level, text) in enumerate(found):
            row = rows_of[f][position] if position < len(rows_of[f]) else None
            top = top_section(level, text)
            if top:
                section = top
                numbers.append(int(section))
                if position != 0:
                    errors.append('%s: section %s does not begin its own file' % (f, section))
                name = posixpath.basename(f) if posixpath.dirname(f) == PLAN_DIR \
                    else posixpath.basename(posixpath.dirname(f))
                if not name.startswith('%02d-' % int(section)):
                    errors.append('%s: section %s lives in a file not named %02d-...'
                                  % (f, section, int(section)))
            if row and row['section'] != section:
                errors.append('%s:%d: Section column says %s, the heading is in section %s'
                              % (INDEX, row['line'], row['section'], section))
            key = '#' * level + ' ' + text
            if key in seen:
                errors.append('%s: heading "%s" duplicates one in %s' % (f, key, seen[key]))
            seen[key] = f
    highest = max(numbers, default=0)
    twice = sorted({n for n in numbers if numbers.count(n) > 1})
    absent = sorted(set(range(1, highest + 1)) - set(numbers))
    if twice:
        errors.append('top-level section(s) %s appear more than once' % twice)
    if absent:
        errors.append('top-level section(s) %s are missing' % absent)
    if not twice and not absent and numbers != sorted(numbers):
        errors.append('top-level sections are out of order: %s' % numbers)

    whole = ''.join(bodies[f] for f in files)
    print('plan/: %d files, %d top-level sections, %d headings indexed, %d lines of plan text'
          % (len(files), len(numbers), len(rows), whole.count('\n')))
    if args.against or args.against_git:
        original, name = load_original(args)
        if whole == original:
            print('identical to %s: %d lines, %d bytes, sha256 %s'
                  % (name, original.count('\n'), len(original.encode('utf-8')),
                     hashlib.sha256(original.encode('utf-8')).hexdigest()))
        else:
            line, ours, theirs = first_difference(whole, original)
            errors.append('the concatenation differs from %s (%d vs %d bytes); first at line %d:\n'
                          '    plan/:    %r\n    original: %r'
                          % (name, len(whole.encode('utf-8')), len(original.encode('utf-8')),
                             line, ours, theirs))
    return report(errors)


def report(errors):
    for e in errors:
        print('FAIL: ' + e)
    print('OK' if not errors else '%d failure(s)' % len(errors))
    return 1 if errors else 0


if __name__ == '__main__':
    sys.exit(main())
