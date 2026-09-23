"""Marks audit register entries FIXED and keeps the plan's section 46 counts true.

usage: python tools/audit_register.py DATE ID [ID ...]
  e.g. python tools/audit_register.py 2026-09-24 QT-07 QT-08

The status becomes "FIXED <DATE>" - a commit cannot name its own hash, so the
fixing commit cites the ID instead and `git log --grep <ID>` finds it. The
counts table, the status line and the roadmap row are recomputed from the
register itself every time, so they cannot drift from it.

The plan is split one file per section under plan/ (PLAN.MD is its index):
the counts table, the status line and the high-severity list are in section
46's file, and the roadmap row is in section 5's roadmap table.
"""
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REG = os.path.join(ROOT, 'docs', 'audit', '2026-09-23-defects.md')


def section_file(number):
    """plan/NN-*.md for a top-level section, found by its number rather than
    named, so that retitling the file does not break this tool."""
    found = glob.glob(os.path.join(ROOT, 'plan', '%02d-*.md' % number))
    if len(found) != 1:
        sys.exit('plan/: expected one file for section %d, found %s' % (number, found or 'none'))
    return found[0]


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    status = 'FIXED ' + sys.argv[1]
    ids = sys.argv[2:]

    reg = open(REG, encoding='utf-8').read()
    for ident in ids:
        pat = re.compile(r'(### %s · [^\n]*\n\n`[^`]*` · status: )\*\*OPEN\*\*' % re.escape(ident))
        reg, n = pat.subn(lambda m: m.group(1) + '**%s**' % status, reg)
        if n != 1:
            sys.exit('entry not found or not open: ' + ident)
    # The register is written with the plan files at the end, not here: a
    # plan table that is not found must leave all three untouched, or the
    # entry is FIXED with the counts unchanged and a rerun refuses the ID.

    # Recount the open defects per area and severity from the register.
    entries = re.findall(
        r'### ([A-Z0-9]+)-\d+ · (\w+) · [^\n]*\n\n`[^`]*` · status: \*\*(OPEN|FIXED[^*]*)\*\*', reg)
    counts = {}
    for area, severity, entry_status in entries:
        c = counts.setdefault(area, {'high': 0, 'medium': 0, 'low': 0})
        if entry_status == 'OPEN':
            c[severity] = c.get(severity, 0) + 1
    open_total = sum(sum(c.values()) for c in counts.values())
    totals = {s: sum(c[s] for c in counts.values()) for s in ('high', 'medium', 'low')}

    register_path, roadmap_path = section_file(46), section_file(5)
    plan = open(register_path, encoding='utf-8').read()
    roadmap = open(roadmap_path, encoding='utf-8').read()

    def sub(pattern, replacement, what, in_roadmap=False):
        nonlocal plan, roadmap
        text, n = re.subn(pattern, replacement, roadmap if in_roadmap else plan)
        if n != 1:
            sys.exit('%s: %s not found' % (os.path.relpath(
                roadmap_path if in_roadmap else register_path, ROOT), what))
        if in_roadmap:
            roadmap = text
        else:
            plan = text

    for area, c in counts.items():
        sub(r'(\| %s — [^|]*\| )\d+ \| \d+ \| \d+ \| \d+ \|' % area,
            lambda m, c=c: m.group(1) + '%d | %d | %d | %d |' % (
                c['high'], c['medium'], c['low'], sum(c.values())),
            'the row for ' + area)
    sub(r'\| \*\*total\*\* \| \*\*\d+\*\* \| \*\*\d+\*\* \| \*\*\d+\*\* \| \*\*\d+\*\* \|',
        '| **total** | **%d** | **%d** | **%d** | **%d** |' % (
            totals['high'], totals['medium'], totals['low'], open_total),
        'the total row')
    sub(r'\*\*STATUS: OPEN — \d+ of \d+ confirmed defects outstanding\.\*\*',
        '**STATUS: OPEN — %d of %d confirmed defects outstanding.**' % (open_total, len(entries)),
        'the status line')
    sub(r'\| 46 \| The audit of 2026-09-23 — defect register \| open — \d+ of \d+ confirmed '
        r'defects outstanding, \d+ of them high \|',
        '| 46 | The audit of 2026-09-23 — defect register | open — %d of %d confirmed defects '
        'outstanding, %d of them high |' % (open_total, len(entries), totals['high']),
        'the roadmap row', in_roadmap=True)
    # Strike a fixed entry in the high-severity list, if it is one.
    for ident in ids:
        plan = re.sub(r'\* \*\*%s\*\* — ' % re.escape(ident),
                      '* ~~**%s**~~ %s — ' % (ident, status), plan)
    open(REG, 'w', encoding='utf-8', newline='\n').write(reg)
    open(register_path, 'w', encoding='utf-8', newline='\n').write(plan)
    open(roadmap_path, 'w', encoding='utf-8', newline='\n').write(roadmap)
    print('%d of %d open, %d high' % (open_total, len(entries), totals['high']))


if __name__ == '__main__':
    main()
