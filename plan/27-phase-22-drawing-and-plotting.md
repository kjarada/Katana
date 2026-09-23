<!-- Katana plan, section 27 of 47. Index: ../PLAN.MD. Previous: 26-phase-21-civil-and-survey-engineering.md. Next: 28-phase-23-cpp-application-api.md -->

# 27. Phase 22 — Drawing and Plotting

**STATUS: PARTIALLY DELIVERED. Reshaped - this is the phase that makes three
already-built things honest, and the first of them now is.**

Delivered: File > Plot to PDF... The plot paints the drawing with the SAME
drawing code the screen uses, through a sheet transform in place of the
screen's (`cad/plot.hpp`: ISO 216 paper sizes, a scale ladder, the sheet as
a `ViewTransform`, a fit rule that picks the first standard scale the drawing
fits at), with every line as wide as its layer's `lineWeight` says in
millimetres of paper - which that field has claimed to mean since Phase 09
and has never meant until now. Entities, hatches (with their model-unit
patterns) and alignments are plotted; rasters and point clouds are not, by
decision. The arithmetic is tested against ISO 216 and 25.4 mm to the inch,
and the PDF itself by the `qt_plot_headless` test, which runs the
application's `--plot` switch offscreen on a copy of the sample project and
checks the file that comes out - the dialog and the switch share one code
path, so the test exercises what the menu does. Record in `docs/cad.md`.

**OUTSTANDING:** layouts and paper space (a plot is the current view on one
sheet); viewports on a sheet; title blocks; annotation scaling (text and
dimension sizes are still model units, so a 2.5 m label is 2.5 mm at 1:1000
and 0.25 mm at 1:10 000); plot styles beyond the line weight (colour-to-pen
mapping, screening); printing to a device (needs Qt PrintSupport, not yet
linked); rasters and point clouds on the sheet.

Three capabilities exist, are validated, are persisted, and are read by nothing
that draws. Each is a silent lie to the user, who sets a value and sees no
effect:

* **`Layer::lineWeight`** is documented as *millimetres on paper*. There is no
  paper. `grep lineWeight src/katana_qt/` finds nothing: the viewport draws
  every line at a hardcoded width. "Millimetres on paper" is meaningless until
  this phase defines paper.
* **`DimensionStyle::textHeight`** is in model units, which is correct, but
  annotation scaling - the thing that makes one dimension readable at 1:200 and
  at 1:2000 - has nowhere to live without a sheet scale.
* **Dimension styles themselves** (delivered above) are named "plot styles"
  in this phase's own list, which double-counts work already done.

So this phase is not "add plotting". It is: **define paper, and connect the
three settings that already claim to describe it.** A `lineWeight` that changes
nothing is worse than an absent one, because it reads as a working feature.

`dimension styles` is struck from the list below - delivered in Phase 09.

Separate the engineering model from the drawing model.

Architecture:

```text
Engineering Model
       ↓
Drawing Model
       ↓
Sheet
       ↓
Viewport
       ↓
Annotations
       ↓
Plot
```

Implement:

```text
layouts
paper space
viewports
title blocks
annotation scaling
dimension styles
plot styles
PDF output
printing
```

---

