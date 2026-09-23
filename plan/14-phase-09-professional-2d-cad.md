<!-- Katana plan, section 14 of 47. Index: ../PLAN.MD. Previous: 13-phase-08-basic-cad-application.md. Next: 15-phase-10-survey-data-model.md -->

# 14. Phase 09 — Professional 2D CAD

**STATUS: PARTIALLY DELIVERED.**

Done: `CommandInterpreter` turns one line of text into one validated, undoable
command - about 40 verbs with the usual CAD aliases, absolute / relative /
polar points, and locale-independent parsing via `std::from_chars`. Layers,
styles and entity properties are editable in the GUI, each edit a command.

Linetypes, dimension styles and hatch patterns are delivered, each as a named
table resolved through the same ByLayer chain as colour. They share one
container, `entity::NamedTable` - see `docs/model.md` for what adding another
costs and, more importantly, for which steps around it fail SILENTLY.

Hatching: `geometry::hatchLines` clips a family of parallel lines to a closed
boundary by the even-odd rule, so a concave notch is left unfilled rather than
bridged. Spacing and angle are model units and the family is anchored to the
world origin, so two adjacent parcels hatched alike line up across their shared
edge. `cad::hatchDrawing` decides when a pattern has become finer than the
screen can show and draws it as a solid tone instead - the same rule
`cad::shouldDash` applies to linetypes, and what keeps a 0.2 m pattern over a
site plan from costing thousands of clipped segments at zoom-out.

**OUTSTANDING: splines, blocks.** Neither is lost on import - the OGR DXF
driver tessellates a SPLINE to a LINESTRING and expands an INSERT inline, and a
HATCH arrives as a POLYGON, verified on a file written for the purpose. What is
missing is AUTHORING them, and the two differ in cost:

* a spline needs an eighth `entity::Geometry` alternative, which is the
  expensive kind of change - `docs/model.md` lists the places it must be
  threaded through and separates the ones the compiler catches from the ones
  that do not;
* a block needs a definition table and an instance entity, so that editing the
  definition updates every instance. Expanded-on-import geometry, which is what
  arrives today, cannot do that.

**FIXED since it was recorded here:** the create / update / delete command
classes that were written out three times are one template set over a
`TablePolicy<T>` in `table_commands.cpp`, done in 20.2 slice 1 and used for
linetypes, dimension styles, hatch patterns, alignments and styles.

---

