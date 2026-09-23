"""Marks audit register entries FIXED and reports how many remain open.

usage: python tools/audit_register.py DATE ID [ID ...]
  e.g. python tools/audit_register.py 2026-09-24 QT-07 QT-08

The status becomes "FIXED <DATE>" - a commit cannot name its own hash, so the
fixing commit cites the ID instead and `git log --grep <ID>` finds it. The open
count is recomputed from the register itself every time, so it cannot drift.
The register (docs/audit/2026-09-23-defects.md) is the only record it edits:
the plan whose section 46 once repeated its counts was removed on 2026-09-23.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REG = os.path.join(ROOT, 'docs', 'audit', '2026-09-23-defects.md')


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

    entries = re.findall(
        r'### ([A-Z0-9]+)-\d+ · (\w+) · [^\n]*\n\n`[^`]*` · status: \*\*(OPEN|FIXED[^*]*)\*\*', reg)
    still_open = [severity for _, severity, entry_status in entries if entry_status == 'OPEN']
    open(REG, 'w', encoding='utf-8', newline='\n').write(reg)
    print('%d of %d open, %d high' % (len(still_open), len(entries), still_open.count('high')))


if __name__ == '__main__':
    main()
