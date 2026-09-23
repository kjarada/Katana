<!-- Katana plan, section 46 of 47. Index: ../PLAN.MD. Previous: 45-survey-module-and-instruments.md. Next: 47-workspace-docks-and-views.md -->

# 46. The audit of 2026-09-23 — the defect register

**STATUS: OPEN — 124 of 142 confirmed defects outstanding.**

A read-only audit of every subsystem: ten auditors, one per area, each told to
verify the code rather than trust this plan, and every defect they reported put
to a second reader told to refute it. The 142 that survived are recorded in
full — what goes wrong, a failure scenario, a suggested fix — in
`docs/audit/2026-09-23-defects.md`, each with an ID. The audit's other findings
(tests that would pass over a bug, duplication, numbers with no source, stale
documentation, unmeasured performance concerns, knowledge to write next to the
code) are in `docs/audit/2026-09-23-findings.md`; they were not challenged, so
each is a lead to check. The false claims it found in this plan were corrected
in f90a1db where no pending fix changes them.

A fix cites the ID (`fixes audit QT-07`) in its commit message and, in the same
commit, runs `python tools/audit_register.py <date> QT-07`, which marks the
entry FIXED in the register and recounts this table from the register, so the
two cannot disagree. (A commit cannot name its own hash, so the entry says
FIXED and a date, and `git log --grep QT-07` finds the commit.)

| Area | high | medium | low | open |
|---|---|---|---|---|
| MOD — entity model, commands, storage, selection | 0 | 0 | 12 | 12 |
| QT — the desktop application | 2 | 9 | 10 | 21 |
| IO — raster, vector and point-cloud interoperability | 1 | 6 | 6 | 13 |
| A12 — the 12d archive and customisation | 1 | 4 | 11 | 16 |
| CAD — the CAD engine | 0 | 5 | 13 | 18 |
| REN — rendering and scene building | 0 | 2 | 5 | 7 |
| GEO — core, maths and geometry | 0 | 2 | 10 | 12 |
| TER — terrain | 0 | 1 | 4 | 5 |
| SUR — survey, geodesy and survey import | 0 | 1 | 4 | 5 |
| NET — least-squares adjustment | 0 | 1 | 2 | 3 |
| BLD — build, tests and tooling | 0 | 3 | 9 | 12 |
| **total** | **4** | **34** | **86** | **124** |

The high-severity ones, taken first:

* ~~**MOD-01**~~ FIXED 2026-09-23 — hiding or locking a parent layer does not hide, protect or stop
  editing the entities on its children (`selection.cpp`, `change_set.cpp`).
* **QT-01** — a 12d archive holding only tins or trimeshes cannot be imported:
  the empty import transaction is rejected and the surfaces are thrown away.
* **QT-02** — Styles and Linetypes' Save Changes rewrites the linetype and
  symbol of every 12d-imported style.
* ~~**IO-01**~~ FIXED 2026-09-23 — vector import discards Z and vector export writes Z = 0.
* ~~**IO-02**~~ FIXED 2026-09-23 — placement advice calls small data inside a large drawing "far
  apart", and its default button then moves it.
* **IO-03** — a budgeted point-cloud read materialises the whole file.
* **A12-01** — a split breakline-point string exports every point with the
  first vertex's attributes and no point id.
* REN-01 (eye-plane clipping) — FIXED in ea6cd24.

Where this sits in the order of work: CLAUDE.md 5.2 item 2 says a real defect
is fixed when it is small and adjacent and recorded otherwise - this register
is that record. CLAUDE.md 5.3 lets a wave of defect fixes run BESIDE the work
the order puts first when their file sets are disjoint, which is how they are
being taken; where the two compete for the same files, the order in 5.2 decides.

