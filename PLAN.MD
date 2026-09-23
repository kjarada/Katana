# Katana plan — index

The plan says *what* Katana is building; `CLAUDE.md` says how to work on it.
Since 2026-09-23 the plan's text lives in [`plan/`](plan/), one file per
top-level section, so that a section can be read - or given to a model - on
its own. This file holds none of that text: it is the index.

**Section numbers and headings are exactly what they were when the plan was
one 3 332-line file, and they must stay so.** Some 240 lines of source,
CMake, test and benchmark comments and some 150 lines of documentation cite
the plan by number - `PLAN.MD section 32`, `PLAN.MD §36`, `PLAN.MD 45.2`,
`PLAN.MD Phase 15`, `PLAN.MD Rule 4` - and they are deliberately NOT
rewritten: the table below resolves every one of them to its file in a single
lookup, so the citations stay true without editing, and stay true if a file
is ever renamed.

## Resolving a citation

* **Section N** (`section N`, `§N`, or a bare `PLAN.MD N`) is the bold row
  whose Section column is N: the file `plan/NN-*.md`, or the folder
  `plan/NN-*/` for a section long enough to have been split.
* **Phase P** is section P + 5: Phase 01 is section 6, Phase 15 is section 20,
  Phase 20 is section 25, Phase 25 is section 30. The heading carries both
  ("20. Phase 15 — 3D Rendering").
* **A subsection number** (`20.2`, `45.3b`) is the number printed in its
  heading, and inside a phase that is the PHASE number, not the section's:
  `20.1` to `20.3` are Phase 20's, in section 25. `5.1`, `5.2` and `5.3`
  exist twice - section 5's standing process (with `5.2a` and `5.2b`), and
  Phase 05's layers, appearance and linetypes in section 10 (with `5.4`,
  dimension styles). Search the Heading column for the number and read the
  Section column; the citing comment's subject tells the two apart.
* **Rule N** is one of section 4's seven rules.
* **A line number** (`PLAN.MD:1374`, used in `docs/audit/`) refers to the
  single-file plan at the commit it was written against, which
  `git show <commit>:PLAN.MD` recovers. The last single-file plan is
  `git show e39e047:PLAN.MD`.

## Editing the plan

* Edit the section's own file, and keep its number and heading. Its first
  line is a navigation comment, invisible when rendered; leave it alone.
* The roadmap table - which phase is done, partial or not started - is
  section 5's, in
  [plan/05-development-strategy.md](plan/05-development-strategy.md). It is
  still the authoritative record of status; keep it in step there.
* When a `#` or `##` heading is added, removed or retitled, change its row
  below in the same change.
* A new section takes the next number (48 at the split), a new file
  `plan/48-<slug>.md` and a bold row at the end of the table. Then run
  `python tools/check_plan_split.py --write-headers`, which writes the new
  file's navigation comment and updates the others' (each names the section
  count and the previous and next files).
* A section that grows past about 400 lines is split as section 25 is: a
  folder `plan/NN-<slug>/` holding the lead text in `00-overview.md` and one
  file per `##` subsection, each verbatim, with the table's rows pointing at
  them.
* `python tools/check_plan_split.py` checks, with nothing but the working
  tree, that the files still make one whole plan: every file indexed and
  every indexed file present, sections 1 to N once each and in order, each
  at the top of its own file, no heading duplicated, missing or out of step
  with this table, and no code fence left open across a file boundary.
  `tools/audit_register.py` edits section 46's counts and section 5's
  roadmap row in their own files.

## Why it is split this way

The single file had reached 3 332 lines (153 KB), too much to hand a model in
order to work on one part of it. A section is the unit every citation names,
so splitting at sections gives each citation exactly one home. Rejected:
fixed-size chunks, which cut sections in half and move with every edit;
rewriting the citations to file paths, which is some 390 edits and breaks
again at the first renamed file; and keeping the whole file beside the split,
which gives two copies to drift apart. The threshold for a folder is about
400 lines: only section 25 exceeded it (731 lines, 44 KB), and the largest
file is now its subsection 20.2 at 314 lines, then section 26 at 225. The
split is verbatim: `python tools/check_plan_split.py --against-git
e39e047:PLAN.MD` shows the files, navigation comments removed, concatenate
to the single-file plan byte for byte.

## Every heading and the file it is in

| Section | Heading | File |
|---|---|---|
| — | AI Engineering Plan | [plan/00-introduction.md](plan/00-introduction.md) |
| — | High-Performance C/C++ CAD & Survey Platform | [plan/00-introduction.md](plan/00-introduction.md) |
| 1 | **1. Mission** | [plan/01-mission.md](plan/01-mission.md) |
| 2 | **2. Core Architecture** | [plan/02-core-architecture.md](plan/02-core-architecture.md) |
| 3 | **3. Primary Technology Stack** | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Core | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | GUI | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Rendering | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Numerical Mathematics | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Computational Geometry | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Solid CAD Geometry | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Coordinate Reference Systems | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | GIS | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Point Clouds | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Database | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Compression | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Testing | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | Profiling | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 3 | AI | [plan/03-primary-technology-stack.md](plan/03-primary-technology-stack.md) |
| 4 | **4. Absolute Architectural Rules** | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 4 | Rule 1 — C++ owns the application | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 4 | Rule 2 — AI never directly modifies the database | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 4 | Rule 3 — The renderer is not the source of truth | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 4 | Rule 4 — External libraries must be isolated | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 4 | Rule 5 — Test before optimization | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 4 | Rule 6 — Profile before optimizing | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 4 | Rule 7 — Preserve deterministic computation | [plan/04-absolute-architectural-rules.md](plan/04-absolute-architectural-rules.md) |
| 5 | **5. Development Strategy** | [plan/05-development-strategy.md](plan/05-development-strategy.md) |
| 5 | 5.1 Standing process — the plan and the documentation are deliverables | [plan/05-development-strategy.md](plan/05-development-strategy.md) |
| 5 | 5.2 Standing process — commit at every milestone | [plan/05-development-strategy.md](plan/05-development-strategy.md) |
| 5 | 5.2b Standing process — do not stop until the plan is finished | [plan/05-development-strategy.md](plan/05-development-strategy.md) |
| 5 | 5.2a Standing process — hand over something runnable | [plan/05-development-strategy.md](plan/05-development-strategy.md) |
| 5 | 5.3 Standing process — self-improvement | [plan/05-development-strategy.md](plan/05-development-strategy.md) |
| 6 | **6. Phase 01 — Build System** | [plan/06-phase-01-build-system.md](plan/06-phase-01-build-system.md) |
| 7 | **7. Phase 02 — Mathematical Foundation** | [plan/07-phase-02-mathematical-foundation.md](plan/07-phase-02-mathematical-foundation.md) |
| 8 | **8. Phase 03 — Basic Geometry** | [plan/08-phase-03-basic-geometry.md](plan/08-phase-03-basic-geometry.md) |
| 9 | **9. Phase 04 — Geometry Algorithms** | [plan/09-phase-04-geometry-algorithms.md](plan/09-phase-04-geometry-algorithms.md) |
| 10 | **10. Phase 05 — Entity System** | [plan/10-phase-05-entity-system.md](plan/10-phase-05-entity-system.md) |
| 10 | 5.2 Resolved appearance (delivered) | [plan/10-phase-05-entity-system.md](plan/10-phase-05-entity-system.md) |
| 10 | 5.4 Dimension styles (delivered) | [plan/10-phase-05-entity-system.md](plan/10-phase-05-entity-system.md) |
| 10 | 5.3 Linetypes (delivered) | [plan/10-phase-05-entity-system.md](plan/10-phase-05-entity-system.md) |
| 10 | 5.1 Nested layers (delivered) | [plan/10-phase-05-entity-system.md](plan/10-phase-05-entity-system.md) |
| 11 | **11. Phase 06 — Command System** | [plan/11-phase-06-command-system.md](plan/11-phase-06-command-system.md) |
| 12 | **12. Phase 07 — Project Storage** | [plan/12-phase-07-project-storage.md](plan/12-phase-07-project-storage.md) |
| 13 | **13. Phase 08 — Basic CAD Application** | [plan/13-phase-08-basic-cad-application.md](plan/13-phase-08-basic-cad-application.md) |
| 14 | **14. Phase 09 — Professional 2D CAD** | [plan/14-phase-09-professional-2d-cad.md](plan/14-phase-09-professional-2d-cad.md) |
| 15 | **15. Phase 10 — Survey Data Model** | [plan/15-phase-10-survey-data-model.md](plan/15-phase-10-survey-data-model.md) |
| 16 | **16. Phase 11 — Coordinate Systems** | [plan/16-phase-11-coordinate-systems.md](plan/16-phase-11-coordinate-systems.md) |
| 17 | **17. Phase 12 — Survey Calculations** | [plan/17-phase-12-survey-calculations.md](plan/17-phase-12-survey-calculations.md) |
| 18 | **18. Phase 13 — Least-Squares Adjustment** | [plan/18-phase-13-least-squares-adjustment.md](plan/18-phase-13-least-squares-adjustment.md) |
| 19 | **19. Phase 14 — Terrain Engine** | [plan/19-phase-14-terrain-engine.md](plan/19-phase-14-terrain-engine.md) |
| 20 | **20. Phase 15 — 3D Rendering** | [plan/20-phase-15-3d-rendering.md](plan/20-phase-15-3d-rendering.md) |
| 21 | **21. Phase 16 — 3D CAD** | [plan/21-phase-16-3d-cad.md](plan/21-phase-16-3d-cad.md) |
| 22 | **22. Phase 17 — Point Cloud Engine** | [plan/22-phase-17-point-cloud-engine.md](plan/22-phase-17-point-cloud-engine.md) |
| 23 | **23. Phase 18 — Spatial Indexing** | [plan/23-phase-18-spatial-indexing.md](plan/23-phase-18-spatial-indexing.md) |
| 24 | **24. Phase 19 — Performance Architecture** | [plan/24-phase-19-performance-architecture.md](plan/24-phase-19-performance-architecture.md) |
| 24 | The optimisation pass of 2026-09-23 — DELIVERED | [plan/24-phase-19-performance-architecture.md](plan/24-phase-19-performance-architecture.md) |
| 24 | The language standard is a build variable | [plan/24-phase-19-performance-architecture.md](plan/24-phase-19-performance-architecture.md) |
| 25 | **25. Phase 20 — File Interoperability** | [plan/25-phase-20-file-interoperability/00-overview.md](plan/25-phase-20-file-interoperability/00-overview.md) |
| 25 | 20.1 Where imported data lands (delivered) | [plan/25-phase-20-file-interoperability/01-20-1-where-imported-data-lands.md](plan/25-phase-20-file-interoperability/01-20-1-where-imported-data-lands.md) |
| 25 | 20.2 The 12d Model programme — TAKES PRECEDENCE OVER EVERY OTHER PHASE | [plan/25-phase-20-file-interoperability/02-20-2-12d-model-programme.md](plan/25-phase-20-file-interoperability/02-20-2-12d-model-programme.md) |
| 25 | 20.3 The survey coding programme — TAKES PRECEDENCE OVER 20.2's REMAINDER AND EVERY OTHER PHASE | [plan/25-phase-20-file-interoperability/03-20-3-survey-coding-programme.md](plan/25-phase-20-file-interoperability/03-20-3-survey-coding-programme.md) |
| 26 | **26. Phase 21 — Civil and Survey Engineering** | [plan/26-phase-21-civil-and-survey-engineering.md](plan/26-phase-21-civil-and-survey-engineering.md) |
| 26 | Alignments | [plan/26-phase-21-civil-and-survey-engineering.md](plan/26-phase-21-civil-and-survey-engineering.md) |
| 26 | Profiles | [plan/26-phase-21-civil-and-survey-engineering.md](plan/26-phase-21-civil-and-survey-engineering.md) |
| 26 | Corridors | [plan/26-phase-21-civil-and-survey-engineering.md](plan/26-phase-21-civil-and-survey-engineering.md) |
| 26 | Parcels | [plan/26-phase-21-civil-and-survey-engineering.md](plan/26-phase-21-civil-and-survey-engineering.md) |
| 26 | Grading | [plan/26-phase-21-civil-and-survey-engineering.md](plan/26-phase-21-civil-and-survey-engineering.md) |
| 27 | **27. Phase 22 — Drawing and Plotting** | [plan/27-phase-22-drawing-and-plotting.md](plan/27-phase-22-drawing-and-plotting.md) |
| 28 | **28. Phase 23 — C++ Application API** | [plan/28-phase-23-cpp-application-api.md](plan/28-phase-23-cpp-application-api.md) |
| 29 | **29. Phase 24 — Python AI Layer** | [plan/29-phase-24-python-ai-layer.md](plan/29-phase-24-python-ai-layer.md) |
| 30 | **30. Phase 25 — AI Agent** | [plan/30-phase-25-ai-agent.md](plan/30-phase-25-ai-agent.md) |
| 30 | Level 1 — Simple commands | [plan/30-phase-25-ai-agent.md](plan/30-phase-25-ai-agent.md) |
| 30 | Level 2 — Queries | [plan/30-phase-25-ai-agent.md](plan/30-phase-25-ai-agent.md) |
| 30 | Level 3 — Multi-step workflows | [plan/30-phase-25-ai-agent.md](plan/30-phase-25-ai-agent.md) |
| 30 | Level 4 — Engineering workflows | [plan/30-phase-25-ai-agent.md](plan/30-phase-25-ai-agent.md) |
| 31 | **31. AI Safety Model** | [plan/31-ai-safety-model.md](plan/31-ai-safety-model.md) |
| 32 | **32. Performance Targets** | [plan/32-performance-targets.md](plan/32-performance-targets.md) |
| 33 | **33. Memory Architecture** | [plan/33-memory-architecture.md](plan/33-memory-architecture.md) |
| 34 | **34. Testing Strategy** | [plan/34-testing-strategy.md](plan/34-testing-strategy.md) |
| 34 | Unit tests | [plan/34-testing-strategy.md](plan/34-testing-strategy.md) |
| 34 | Integration tests | [plan/34-testing-strategy.md](plan/34-testing-strategy.md) |
| 34 | Regression tests | [plan/34-testing-strategy.md](plan/34-testing-strategy.md) |
| 34 | Property tests | [plan/34-testing-strategy.md](plan/34-testing-strategy.md) |
| 34 | Performance tests | [plan/34-testing-strategy.md](plan/34-testing-strategy.md) |
| 34 | Real-world datasets | [plan/34-testing-strategy.md](plan/34-testing-strategy.md) |
| 35 | **35. Numerical Correctness** | [plan/35-numerical-correctness.md](plan/35-numerical-correctness.md) |
| 36 | **36. Error Handling** | [plan/36-error-handling.md](plan/36-error-handling.md) |
| 37 | **37. Logging** | [plan/37-logging.md](plan/37-logging.md) |
| 38 | **38. Documentation** | [plan/38-documentation.md](plan/38-documentation.md) |
| 39 | **39. Code Quality Requirements** | [plan/39-code-quality-requirements.md](plan/39-code-quality-requirements.md) |
| 40 | **40. AI Coding Workflow** | [plan/40-ai-coding-workflow.md](plan/40-ai-coding-workflow.md) |
| 41 | **41. Definition of Done** | [plan/41-definition-of-done.md](plan/41-definition-of-done.md) |
| 42 | **42. Final Project Architecture** | [plan/42-final-project-architecture.md](plan/42-final-project-architecture.md) |
| 43 | **43. Required Development Order** | [plan/43-required-development-order.md](plan/43-required-development-order.md) |
| 44 | **44. Core Principle** | [plan/44-core-principle.md](plan/44-core-principle.md) |
| 45 | **45. Survey module and instrument interoperability** | [plan/45-survey-module-and-instruments.md](plan/45-survey-module-and-instruments.md) |
| 45 | 45.1 What already existed, and what this programme is therefore NOT | [plan/45-survey-module-and-instruments.md](plan/45-survey-module-and-instruments.md) |
| 45 | 45.2 Architecture, decided before any of it was written | [plan/45-survey-module-and-instruments.md](plan/45-survey-module-and-instruments.md) |
| 45 | 45.3 Slices | [plan/45-survey-module-and-instruments.md](plan/45-survey-module-and-instruments.md) |
| 45 | 45.3b The bridge, and a gap it exposed | [plan/45-survey-module-and-instruments.md](plan/45-survey-module-and-instruments.md) |
| 45 | 45.4 The proprietary-format rule, as it will be applied | [plan/45-survey-module-and-instruments.md](plan/45-survey-module-and-instruments.md) |
| 45 | 45.5 Numbers that must not be guessed | [plan/45-survey-module-and-instruments.md](plan/45-survey-module-and-instruments.md) |
| 46 | **46. The audit of 2026-09-23 — the defect register** | [plan/46-audit-defect-register.md](plan/46-audit-defect-register.md) |
| 47 | **47. The workspace: dockable panels and views, per-view layers** | [plan/47-workspace-docks-and-views.md](plan/47-workspace-docks-and-views.md) |
