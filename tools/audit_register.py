"""Marks audit register entries FIXED and keeps PLAN.MD section 46's counts true.

usage: python tools/audit_register.py DATE ID [ID ...]
  e.g. python tools/audit_register.py 2026-09-24 QT-07 QT-08

The status becomes "FIXED <DATE>" - a commit cannot name its own hash, so the
fixing commit cites the ID instead and `git log --grep <ID>` finds it. The
counts table, the status line and the roadmap row are recomputed from the
register itself every time, so they cannot drift from it.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REG = os.path.join(ROOT, 'docs', 'audit', '2026-09-23-defects.md')
PLAN = os.path.join(ROOT, 'PLAN.MD')


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
    open(REG, 'w', encoding='utf-8', newline='\n').write(reg)

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

    plan = open(PLAN, encoding='utf-8').read()

    def sub(pattern, replacement, what):
        nonlocal plan
        plan, n = re.subn(pattern, replacement, plan)
        if n != 1:
            sys.exit('PLAN.MD: %s not found' % what)

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
        'the roadmap row')
    # Strike a fixed entry in the high-severity list, if it is one.
    for ident in ids:
        plan = re.sub(r'\* \*\*%s\*\* — ' % re.escape(ident),
                      '* ~~**%s**~~ %s — ' % (ident, status), plan)
    open(PLAN, 'w', encoding='utf-8', newline='\n').write(plan)
    print('%d of %d open, %d high' % (open_total, len(entries), totals['high']))


if __name__ == '__main__':
    main()
