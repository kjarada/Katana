<!-- Katana plan, section 5 of 47. Index: ../PLAN.MD. Previous: 04-absolute-architectural-rules.md. Next: 06-phase-01-build-system.md -->

# 5. Development Strategy

Development must proceed from simplest to most complex.

Do not implement advanced systems prematurely.

The required progression is:

| Phase | | Status |
|---|---|---|
| 01 | Build System | **done** |
| 02 | Mathematics | **done** |
| 03 | Basic Geometry | **done** |
| 04 | Geometry Algorithms | **done** |
| 05 | Entity System | **done** |
| 06 | Command System | **done** |
| 07 | Project Storage | **done** |
| 08 | Basic CAD | **done** |
| 09 | Professional 2D CAD | partial — nested layers, resolved appearance, linetypes, dimension styles and hatching delivered; authoring splines and blocks outstanding |
| 10 | Survey Data | **done** |
| 11 | Coordinate Systems | **done** |
| 12 | Survey Calculations | **done** |
| 13 | Least Squares | **done** |
| 14 | Terrain | **done** |
| 15 | 3D Rendering | partial — tiled multithreaded SOFTWARE renderer, camera, tiled viewports, 3D and section views; Vulkan backend outstanding |
| 16 | 3D CAD | reshaped — Open CASCADE rejected with reasons; civil solids on the TIN instead |
| 17 | Point Clouds | partial — PDAL read/write, budgeted decimation, 2D display; COPC conversion and resolution reads, from the GIS menu and the CLI; automatic conversion and view-driven re-query outstanding |
| 18 | Spatial Indexing | partial — sparse hash grid behind snapping, picking and box selection (50x-310x measured); no KD-tree, BVH, octree or terrain quadtree |
| 19 | Performance Architecture | partial — `katana::core::TaskPool` drives the renderer; task graph and work stealing rejected with reasons; `compareSurfaces` 12.1 s -> 2.1 s by pairwise triangle clipping |
| 20 | File Interoperability | partial — raster, vector, point cloud and 12d Archive import/export in GUI and CLI, every GDAL/PDAL capability on the GIS menu; no DWG/LandXML/IFC. 20.2 (12d programme) delivered bar property definitions; 20.3 (survey coding) delivered bar the library browser |
| 21 | Civil Engineering | partial — sections; clothoid; alignments and profiles; corridor quantities and surface, cross-checked; parcels with bearings, area, deed wording and labels; grading engine without a GUI route; richer assemblies outstanding |
| 22 | Drawing/Plotting | partial — Plot to PDF through the screen's own drawing code at ISO sizes and standard scales, `Layer::lineWeight` finally honoured; layouts, title blocks, annotation scaling, printing outstanding |
| 23 | C++ Application API | reshaped — expose the existing interpreter, do not write a second one |
| 24 | Python AI | not started |
| 25 | AI Agent | not started |
| 26 | Production Hardening | not started — no section of its own yet; section 43 lists it last |
| 45 | Survey module and instrument interoperability | in progress — model, detection and the cad bridge delivered; parsers, reduction, Survey menu, wizard and Point Manager in progress |
| 46 | The audit of 2026-09-23 — defect register | open — 124 of 142 confirmed defects outstanding, 4 of them high |

The delivered phases below are recorded rather than specified: their
requirements have been met, so only what was built and where it lives is kept.
Section numbers are deliberately NOT renumbered — source comments cite them
(`PLAN.MD section 32`, `Rule 4`), and renumbering would silently invalidate
every such reference. A test count in a phase's record is the count when
that phase was delivered, not now: the suite's current total is in README.md,
and the counts here drifted from it for months before the audit of 2026-09-23
said so.

Do not skip phases without explicit architectural justification.

## 5.1 Standing process — the plan and the documentation are deliverables

`CLAUDE.md` at the repository root is the working contract for every
contributor, human or model, and is loaded automatically at the start of a
session. It is normative; this section states the parts that bind the plan
itself.

**Update this file as part of the change, not afterwards.** Whenever a phase or
sub-feature is completed, or anything changes what the software can do:

* Set the phase's status here using the vocabulary
  `**STATUS: DELIVERED.**` / `**STATUS: PARTIALLY DELIVERED.**`, and for a
  partial one write an explicit `OUTSTANDING:` paragraph. Never quietly narrow
  a phase to whatever was managed.
* Keep the roadmap table above in step.
* Update the document in `docs/` for the area, and `README.md`.
* Record the alternative that was rejected and why, wherever the decision is
  not obvious. `docs/storage.md` is the worked example: a measurement, a
  decision, and the reasons the other options lost.

**Do not renumber sections.** Source comments cite them (`PLAN.MD section 32`,
`Rule 4`); renumbering silently invalidates every such reference.

**If the plan turns out to be wrong** — it asks for something that proved to be
a bad idea, or the ground has moved — say so here, in place, with the reason.
Do not silently obey it and do not silently ignore it.

## 5.2 Standing process — commit at every milestone

A milestone is: a phase or sub-feature complete with its tests passing; a bug
fixed with a regression test that is proven to catch it; a refactor landed with
the suite green; or a measured performance change. At each one, commit — with
the plan and documentation already updated in the same change, and with the
evidence (test counts, before/after numbers, the external source consulted) in
the message.

Do not batch unrelated work into one commit, and do not commit a half-finished
feature to save progress.

## 5.2b Standing process — do not stop until the plan is finished

Work continuously through this plan until every phase is delivered. Finish a
milestone, commit it, and begin the next in the same session; do not stop at a
natural pause point and do not stop to ask whether to continue.

Stop only for a decision that is genuinely the user's (destructive,
outward-facing, or a fork where the options lead to materially different
software), for a hard block naming exactly what is missing, or because the plan
is complete. Running out of obvious next steps is not a stopping condition: the
table in this section lists what remains, and `CLAUDE.md` section 6 lists the
standing improvement work.

**The order of work is the one `CLAUDE.md` section 5.2 states, and it is
stated there only.** This paragraph used to say that 20.2 was the only work
until delivered; the user's later instruction put 20.3 before it, and section
45 (requested 2026-09-23) after 20.3 and before 20.2's remainder, so three
documents had come to give three orderings. As of 2026-09-23: 20.3's
remainder, then section 45, then 20.2's remainder, then the earliest
undelivered phase, with real defects found on the way fixed when small and
adjacent.

This does not license committing a half-finished feature to keep moving. Every
commit still lands green, with the plan and documentation updated in the same
change.

## 5.2a Standing process — hand over something runnable

Before stopping work, leave a Release build that runs and say how to run it,
what to look at, and what correct looks like. A session that ends with a green
test suite and no runnable application has delivered nothing the user can
check. `CLAUDE.md` section 5.1 is normative on this.

Say plainly which of the work is reachable from the GUI and which exists only
in the library with no way to invoke it yet - a feature with no route to it from
the application is finished work in the tests and unfinished work to the user.

## 5.3 Standing process — self-improvement

Every contributor is expected to leave the codebase easier to work on, not only
larger. On every session, look for and act on: a second way of doing something
that already has a first way; a comment that says what instead of why; a test
that would pass if the code were wrong; a silently ignored failure; a constant
with no stated source; anything that took three files to work out and is
written down in none of them.

A real defect found outside the current task is either fixed, if it is small
and adjacent, or recorded here under the phase it belongs to. Never leave it
only in a conversation: the conversation is lost, the repository is not.

---

