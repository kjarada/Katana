# Annotation: paper-sized text, labels, dimensions and leaders

Everything a drafter writes ON a drawing rather than draws IN it: text sized
for the sheet, text styles, labels that read their words off the geometry,
rules that label a whole drawing, the placer that keeps labels apart, the
dimension kinds, leaders and callouts. The model's side is `katana_entity`
(`include/katana/entity/annotation.hpp`, `label_text.hpp`, `label_values.hpp`,
`anchor.hpp`, `text_block.hpp`), the layout and the edits are `katana_cad`
(`include/katana/cad/annotation/`, `src/katana_cad/annotation/`), and the
window's part is a thin front end (`src/katana_qt/annotation/`). Built on
2026-09-25 at the owner's request for "a professional annotation system",
drivable by an agent as much as by a person.

The rules it keeps, from `docs/architecture.md`: every edit is ONE command
through `Document::execute` (Rule 2), so the command line, the window and an
agent get the same result and one undo takes it back; the layout is
deterministic (Rule 7) and headless, so a test or an agent can ask where a
label goes without a window; and absent is not zero - a label never prints
"RL 0.000" for a point with no level.

## Paper size and the annotation scale

An annotation is written for the sheet: a 2.5 mm label is 2.5 mm on a
1:200 plan and on a 1:2000 one. Every annotation size is therefore PAPER
millimetres, and there is ONE conversion to the model, `annotationModelSize`:
`paperMm x scale / 1000` model units at 1 : scale (`annotationPaperSize` is
its inverse).

| 2.5 mm on paper at | is, in the model |
|---|---|
| 1:200 | 0.5 m |
| 1:500 | 1.25 m |
| 1:1000 | 2.5 m |
| 1:2000 | 5 m |

The scale comes from where the drawing is looked at, never from a widget
(`PlanPaintOptions::annotationScale`, `docs/plan_view.md`, "Annotation"): the
plan view uses the DOCUMENT's annotation scale (`Document::annotationScale`,
set by `ANNOSCALE` or the Format toolbar's scale box, one undo step), a plot
of the drawing uses the plot's scale, and a sheet viewport its own. So the
same label is the same size on every sheet it appears on, whatever each
viewport's scale.

**Backward compatible by construction.** The default scale is 1:1000, the
one scale at which 2.5 mm is 2.5 model units - the height a `TextGeometry`
has always defaulted to. A text with no style and no paper height is drawn
at its model height, exactly as before, at every scale
(`ATextFromBeforeStylesKeepsItsModelHeightAtEveryScale`); a dimension style
is model units unless it says `PAPER on`; a drawing that never set a scale
draws paper-sized text the size of the text beside it. The document keeps
its scale in the project's metadata under `annotation_scale`, a key the
storage layer carries without reading it, so it needed no schema change.

## Text styles

A text style is a row of the `textStyles` table (`TextStyle`,
`include/katana/entity/annotation.hpp`), changed only by its table commands
(`createTextStyle`, `updateTextStyle`, `deleteTextStyle`), like the dimension
styles beside it. The built-in `Standard` style is seeded by the model and
cannot be deleted; deleting a style still used names the first user.

| Field | Meaning |
|---|---|
| `fontFamily` | the face; empty is the painter's plain-text face |
| `paperHeight` | millimetres on paper; 0 leaves a text its model height |
| `widthFactor` | horizontal stretch, 1 as designed |
| `oblique` | slant of the verticals, radians |
| `bold`, `italic` | |
| `color` | explicit, or empty for the layer's (ByLayer) |
| `mask`, `maskMargin` | a background mask behind the text, reaching `maskMargin` mm past it, painted in the paper on paper and the view's ground on screen |
| `readable` | turn a text that would read upside down half a turn |
| `lineSpacing` | a multiple of the standard pitch between baselines, 5/3 of the height - the spacing multi-line text always had - at 1 |

A `TextGeometry` gained three members: `style`, `paperHeight` and `justify`.
Its height on paper is its own `paperHeight`, else its style's, else it is a
model-unit text of `height`. `justify` is the nine points, `TL` to `BR`
(`TextJustify`); a text holds several lines separated by `\n`, each justified
on its own (a right-justified note has a ragged left edge, as on a drawing);
`rotation` turns the block about its justification point. **Readable
auto-flip** (`readableRotation`): a text whose reading direction points
between straight up and straight down on the left is turned half a turn, and
its justification mirrored so it still covers the same place
(`AReadableTextIsTurnedAndCoversTheSamePlace`) - a label along a line running
west reads left to right.

`layoutText` (`include/katana/cad/annotation/text_layout.hpp`) lays a text
out once, into a `Drawing` - text runs, masks, strokes - that every painter
paints the same way. Widths come from a MEASURE: the Qt painter passes its
font's (`AnnotationFonts::measure`), the tests and a file export the
estimated one (0.6 of the height a character), so layout is exact on screen
and deterministic headless.

## Labels

A label (`LabelGeometry`) is an entity that says WHAT it labels and in which
label style - never the text. Its words are worked out from the target every
time it is drawn, so a label cannot disagree with the geometry it describes
however that geometry was edited. It lives on a layer (labels off by layer,
as a surveyor expects), is erased, undone and saved like any entity, and
carries: the target entity (or an alignment, for chainages), the `part` of a
polyline it labels (-1 for every segment), the style, its `anchor` (kept up
to date by the associative update), an optional dragged `position`, an
optional `textOverride`, and the `rule` that made it (empty for one placed
by hand).

### What a label can say

A label style has a KIND, and the kind says what values its template may
use (`LABELSTYLE VALUES kind` lists them; `labelValueNames`). Every kind also
has `id`, `layer`, `code` (the survey code, found where survey coding finds
one), `point` (the point number), `description` and `prop.NAME` for any
property.

| Kind | Labels | Values |
|---|---|---|
| `point` | a point | `x`, `y`, `easting`, `northing`, `z`, `rl` |
| `segment` | a line, each segment of a polyline | `bearing`, `distance`, `length`, `dx`, `dy`, `dz`, `grade`, `segment` |
| `arc` | an arc, a circle | `radius`, `length`, `chord`, `delta`, `bearing` (of the chord), `tangent` |
| `area` | a closed polyline, a circle | `area`, `perimeter`, `x`, `y` (the label point) |
| `chainage` | an alignment, every `interval` with ticks every `tickInterval` | `chainage`, `x`, `y`, `alignment` |

### Templates

`include/katana/entity/label_text.hpp` is the reference. A template is
literal text with fields in braces, `{value:step:step}`, the steps applied
left to right: unit conversions (`m`, `mm`, `km`, `ft`; `m2`, `ha`, `km2`,
`ac`; `deg`, `rad`, `gon`), a number format (`.3f`), an angle format (`dms`,
`dms.1`, `dm`, `qb` for a quadrant bearing), the chainage format (`ch`:
`1+234.500`) and `upper` / `lower`. With no format step a value prints in its
quantity's own - a length to 3 decimals, an area to 1, a bearing in whole
seconds - so `{distance}` is already a surveyor's distance.

    {bearing:dms} {distance:.3f}          90°00'00" 40.000
    {area:m2:.1f} m²\n{area:ha:.4f} ha    1000.0 m² / 0.1000 ha
    RL {z:.3f}                            RL 10.250, or nothing without a level
    {chainage:ch}                         0+020.000

A LINE of the template with a value the target does not have is dropped, and
a label whose every line is dropped is not drawn (and `LABEL` refuses to make
one). DMS is rounded ONCE, on the seconds, and carried, so 59.9996" never
prints as 60". A template is checked when the style is stored
(`checkLabelTemplate`): an unknown value or step is refused with its name,
never drawn with a hole in it.

### Label styles

| Field | Meaning |
|---|---|
| `kind`, `text` | above |
| `textStyle`, `paperHeight` | the face; the height on paper (0: the text style's, else 2.5 mm) |
| `placement` | `auto`, `above`, `below`, `along` (on the line, over a mask), `centroid`, `right`, `left` |
| `orientation` | `aligned` (along its segment or arc, readable) or `horizontal` (level on the sheet) |
| `offset` | paper mm between the feature and the text |
| `leader` | a line back to the feature when the text had to move away |
| `displace` | whether the placer may move the text away at all |
| `priority` | higher places first |
| `color` | overrides the text style's |
| `marker`, `markerSize` | `none`, `cross`, `dot`, `circle` at a point, drawn whatever the text does |
| `minimumLength` | paper mm: a shorter segment is not labelled at this scale |
| `interval`, `tickInterval`, `tickLength` | a chainage label's labelled marks, its ticks, and their length on paper |

`LABELSTYLE DEFAULTS` adds seven styles that label a survey plan the way one
is usually labelled (`defaultLabelStyles`): Point Number, Spot Level (a
cross and the level), Point Details (number, code, level), Bearing Distance
(whole seconds and millimetres, segments of 5 mm or more on paper), Arc
Data, Lot Area (square metres and hectares, at the centroid, priority 10)
and Chainage (every 20 m, ticks every 10 m).

## The placer

`include/katana/cad/annotation/label_layout.hpp` is the reference; in short,
a deterministic greedy CANDIDATE placer, the classical cartographic method:

1. Each piece of each label (a point, each segment or arc, an area, each
   chainage mark) gets an ordered list of candidate places: its style's
   preferred place first, then the others the style allows (Imhof's order
   around a point; slid along a line to 35, 65, 20 and 80 per cent; stepped
   off an area's label point), then two rings further out - DISPLACED, with
   a leader back when the style asks for one.
2. Pieces are placed in a fixed order: a dragged label first, then by
   priority, then by label id and piece. No container's order or pointer
   takes part, so the same drawing places the same way every time
   (`TheResultDoesNotDependOnTheOrderLabelsArriveIn`).
3. Each piece takes its first candidate whose text box overlaps no text
   placed before it and crosses nothing it keeps out of. A piece with no free
   candidate is SUPPRESSED and counted; its marker or tick is still drawn.

**What a label keeps out of** is the linework and the other annotation drawn
with it: lines, polylines, arcs and circles, and the strokes and text boxes
of notes, leaders and dimensions (`appendKeepOut`), each box by its edges AND
its diagonals, so a label that would fit wholly inside a note's box still
meets it (`ANoteIsKeptOutOfEvenWhenTheLabelWouldFitInsideIt`). Its own
segment does not push a label away - the offset already clears it. The first
screenshot of the placer showed why the annotation belongs in the keep-out:
a bearing sat on an angular dimension's arrowhead and a lot's area on a
circle inside the lot. An AREA label tries the clear places first and only
then the label point over a mask, because a mask hides what it covers
(`AnAreaLabelMovesOffALineBeforeItTakesAMask`).

Overlap is exact for the rotated boxes (separating axes), found through a
uniform grid over their bounding boxes (10 mm cells on paper), so a thousand
labels cost what their neighbours cost. The painter lays out what is in view
and gathers the keep-out as it draws (`docs/plan_view.md`); `LABEL LAYOUT`
gathers the same over the whole drawing (`labelKeepOut`), so the command
line's reply is where the plan view puts each label.

**Measured**, `LABEL LAYOUT` over 10 000 point labels at random in a
kilometre square, in the linux-debug build (`-O0` with library assertions;
no optimised build was made for this): 0.40 s a layout including its
10 000-line reply, of which avoiding collisions is about 0.10 s (the same
with `collisions=off` is 0.30 s). At 1:1000, 633 of the 10 000 were
displaced and none suppressed; at 1:200, 12 displaced.

## Auto-labelling

A label rule (`LabelRule`, the `labelRules` table) says which entities get a
label and in which style: a layer glob, a code glob (case folded; `*` and
`?`), an entity type, the layer the labels go on (created if missing), and
whether it is enabled. `AUTOLABEL RUN` (`autoLabel`) applies every enabled
rule, or the ones named, as ONE command:

* each matching entity - or alignment, for a chainage style - gets the rule's
  label unless it already has it;
* a label the rule made earlier and no longer asks for is removed;
* a label placed by hand, or another rule's, is never touched, and a dragged
  label keeps its place (`ARerunKeepsADraggedLabelAndDropsOnesNoLongerAskedFor`);
* a target whose label would say nothing, or whose label layer is locked, is
  counted `skipped`.

Re-running with nothing changed makes no command and so no undo step
(`ARunIsOneStepAndARerunChangesNothing`). `AUTOLABEL PREVIEW` replies what a
run would do without doing it; `AUTOLABEL CLEAR` removes what the rules made
and nothing else. Matching is in entity id order (Rule 7).

## Associativity

A dimension's points, a leader's tip and a label's target can NAME the
entity they belong to (`AnchorRef`: an entity and which of its points -
position, start, end, mid, centre, vertex N, the middle of segment N). On the
command line a point written `#id`, `#id.end`, `#id.v3` or `#id.s2` names one;
the interactive dimension tools name the lines, arcs and circles they are
given by picking.

Rather than teach every command about annotation, `Document::execute` wraps
EVERY command in `withAssociativeUpdate`
(`include/katana/cad/annotation/associative.hpp`): after the command runs,
the annotations that follow what it changed are brought up to date as a
second change set INSIDE the same undo step, so one undo puts back the
geometry and everything that followed it. A radius follows its circle's
centre and radius and keeps its direction; an angle between two lines
follows their intersection; a label follows its target and is removed with
it (and comes back with it on undo); a reference to an erased entity is
dropped, leaving an ordinary dimension where it was, as a CAD user expects.
An annotation on a locked layer is left alone.

## Dimensions

`DimensionGeometry` gained a `kind` (`entity.hpp`, `DimensionKind`); Aligned
is the default, so every dimension written before is an aligned one and is
stored byte for byte as it was.

| Kind | Measures | Made by |
|---|---|---|
| Aligned | the true distance, its line parallel to the points | `DIM ALIGNED`, `DIM p p offset`, the Aligned Dimension tool |
| Linear | along a direction: horizontal, vertical or rotated | `DIM LINEAR` / `HORIZONTAL` / `VERTICAL` (the Linear Dimension tool stores the aligned projection, `docs/tools.md`) |
| Angular | the angle at a vertex, the side the arc is dragged to | `DIM ANGULAR`, the Angular Dimension tool |
| Radius, Diameter | an arc's or a circle's, from its centre | `DIM RADIUS` / `DIAMETER`, the Radius and Diameter Dimension tools |
| OrdinateX, OrdinateY | a feature's X or Y from a datum | `DIM ORDINATE`, the Ordinate Dimension tool |

`DIM BASELINE dim p...` adds dimensions measured from the same first point,
each a spacing further out; `DIM CONTINUE dim p...` chains them end to end
(`BaselineAndContinuedChains`). A dimension style's sizes are model units,
or with `paperSized` (`DIMSTYLE SET name PAPER on`) paper millimetres drawn
at the annotation scale (`APaperSizedStyleIsTheSameOnPaperAtEveryScale`). The
arrowheads are closed filled, open, tick, dot or none (`ArrowHead`, which
moved from the dimension style to the entity header so a leader shares it).
An angle is written in DMS, a radius with `R` and a diameter with `Ø`.
`buildDimension` draws every kind for the painters; the aligned drawing is
exactly what it was before the kinds (`TheAlignedDrawingIsWhatItWasBeforeTheKinds`).

## Leaders and callouts

A leader (`LeaderGeometry`) is ONE entity from its arrow to its note: the
vertices (tip first, any number of bends), the note (`\n` between lines), the
arrowhead kind and size, a landing length, the text style and height, a
callout frame - `box` or `circle` - and optionally the entity its tip
follows. Sizes are paper millimetres. The landing runs away from the last
segment, level on the sheet, and the note sits half a text height beyond it,
left-justified to the right and right-justified to the left
(`TheLandingRunsAwayFromTheLineAndTheNoteSitsBeyondIt`); a circle callout is
the smallest circle clearing its note. `BALLOON` is a circled leader whose
number counts on from the highest balloon in the drawing (or `n=`). The
Leader tool makes the same entity (`docs/tools.md`); it used to draw a
polyline, an arrowhead and a text per line, which drifted apart when one was
moved.

## The verbs

`CommandInterpreter::annotationHelpText` is the reference, printed by `HELP`.
Options are `key=value`, `\n` in a text or template is a line break, and
every reply is `key=value` records an agent can read without guessing:

| Verb | Replies, for example |
|---|---|
| `ANNOSCALE [N \| 1:N]` | `annoscale=500` |
| `TEXTSTYLE LIST \| NEW \| SET \| DELETE \| INFO` | `created text style name=Notes font="" paper=3.5 width=1 ... spacing=1` |
| `TEXT p [height] "text" style= paper= justify= rotation=`, `MTEXT`, `TEXTEDIT id` | `created text id=7 style=Standard paper=3.5 justify=MC height=0.7` |
| `LABELSTYLE LIST \| NEW \| SET \| DELETE \| INFO \| DEFAULTS \| VALUES kind \| CHECK kind template` | `added=7 styles=...`, `kind=point values=id,layer,...` |
| `LABEL id... style=`, `LABEL SELECTION`, `LABEL ALIGN name`, `LIST`, `SET`, `DELETE`, `LAYOUT [scale= collisions=]` | `scale=600 considered=5 placed=4 displaced=0 suppressed=1 orphaned=0`, a `label=... x= y= candidate= text=` line for each piece placed and a `label=... piece=... suppressed=yes` line for each that found no room |
| `AUTOLABEL RULE ADD \| SET \| DELETE \| LIST`, `RUN`, `PREVIEW`, `CLEAR` | `autolabel created=2 kept=0 removed=0 skipped=0` and `rule=... labels=N` |
| `DIM LINEAR \| HORIZONTAL \| VERTICAL \| ALIGNED \| ANGULAR \| RADIUS \| DIAMETER \| ORDINATE \| BASELINE \| CONTINUE` | `created dimension id=5 kind=diameter measures=10 text=Ø10.000 associative=yes`, `created dimensions=2 ids=7,8` |
| `LEADER p p... text= arrow= callout= style= paper= arrowsize= landing=`, `BALLOON` | `created leader id=26 text="PIT 12\nIL 10.50" associative=no` |
| `DIMSTYLE SET name PAPER on` | the style, `paper-sized` in `DIMSTYLE LIST` |

`TS`, `LS`, `MT` and `LE` are aliases. `LABEL` of many entities, `AUTOLABEL
RUN` and `DIM BASELINE` are each ONE undo step (`ManyLabelsAreOneStep`). A
session an agent might type:

    LABELSTYLE DEFAULTS
    AUTOLABEL RULE ADD lots style="Lot Area" layer=BOUNDARY type=Polyline labellayer=LABELS
    AUTOLABEL RULE ADD bearings style="Bearing Distance" layer=BOUNDARY type=Polyline labellayer=LABELS
    AUTOLABEL RUN
    ANNOSCALE 500
    LABEL LAYOUT

## In the window

Format > **Text Styles...** (`formatTextStyles`, the dialog
`textStyleManagerDialog`) and Format > **Label Styles and Rules...**
(`formatLabelStyles`, `labelStyleManagerDialog`), and the annotation scale
box on the Format toolbar (`annotationScaleCombo`), all built by
`AnnotationWorkbench` (`src/katana_qt/annotation/annotation_workbench.hpp`)
so `MainWindow` gains one member. The managers
(`src/katana_qt/annotation/annotation_managers.hpp`, which lists every
field's object name) are a list and a form: Apply runs the table command -
one undo step, nothing when the form is unchanged - and a refusal is shown
in the dialog's problem line in the command's own words. The label manager's
template field is checked as it is typed, and its second tab holds the rules
with Run and Clear, which report what `AUTOLABEL` replies. The dialogs keep
no copy of the tables: an undo or a verb typed while one is open shows at
once. The Annotate menu's tools are in `docs/tools.md`; the painting is in
`docs/plan_view.md`, "Annotation".

**Labels by hand** (2026-09-26). Until then a label could come only from an
auto-label rule or a typed `LABEL`; nothing in the window made, edited or
reported one. Now:

* **Annotate > Label Objects** (`annotate.label`; `LABELOBJECTS`, `LBL`, and
  a bare `LABEL`, as a bare `LEADER` starts the Leader tool) labels what is
  selected, or what is picked then Enter, with Style, Part and Text options,
  then a click for the text's place or Enter to leave it to the placer. It
  makes its labels through `annotation::createLabels`, the door the `LABEL`
  verb now goes through too, so the tool and `LABEL id... style= part= at=
  text=` make the same entities as one undo step (`test_annotate_label_tool.cpp`
  compares the two). The style offered is the one the drawing's newest
  hand-placed label wears when it fits, else the first by name that can
  label the object: remembered by the drawing, as the Text tool's height is,
  not by the program. Several objects share no one place, so with more than
  one only Enter places them. The selection is cleared once they are made, so
  the tool, starting again, asks for the next objects instead of labelling
  the same ones twice.
* **Annotate > Edit Label...** (`annotateEditLabel`, the dialog
  `labelEditDialog`; `src/katana_qt/annotation/label_edit_dialog.hpp` lists
  every field) opens on the selected label: its style, its own text (ticked,
  or `text=none`), where its text is pinned (ticked, or `at=none` for the
  placer) and its layer. OK and Apply build the `LABEL SET` line naming only
  what changed - shown as it will run - and run it through the window's one
  executor (`docs/desktop.md`), so it is echoed, kept and one undo step. With
  no label selected it says so and runs nothing; it asks nothing in a box.
  A text the command line cannot write (a double quote, or the bare word
  `none`, which `LABEL SET` reads as "no own text") is refused in the dialog,
  naming the field.
* **The Properties panel** shows a label's resolved **Text** and its
  **Position** (`pinned at E, N` or `automatic`) beside its style, target,
  part and rule. The text is `annotation::shownLabelText`, which `LABEL LIST`
  now replies with too, so the panel and the verb cannot disagree.
* **Annotate > Label Layout Report...** (`annotateLabelLayout`,
  `labelLayoutDialog`; `label_layout_report.hpp`) runs `LABEL LAYOUT` - with
  `collisions=off` when its box is unticked - through the same executor when
  it opens and on Run, and shows the placed, displaced, suppressed and
  orphaned counts read back from the reply. **Select Suppressed** runs
  `SELECT id...` for the labels with a piece that found no room. For that
  the verb now names each such piece in a record of its own,
  `label=7 piece=0 suppressed=yes`, after the placed ones
  (`LabelLayout::suppressedPieces`, `LabelLayoutNamesEachPieceThatFoundNoRoom`).
  The dialog works nothing out itself: an agent reads the same records.

Opening Edit Label by double-clicking a label is left to the plan view's
double-click work, which reaches the dialog through the action's name,
`annotateEditLabel`.

## Stored with the project

Migration 11 (`src/katana_storage/project_store.cpp`, `docs/model.md`) adds
`text_styles` (a column a field, a fixed set a DXF STYLE row also has),
`label_styles` (the name and kind as columns and the rest as versioned JSON,
`labelStyleDefinitionToJson`, so a later placement option needs no
migration; a definition from a newer build is refused as `Unsupported`
rather than half read), `label_rules`, and a dimension style's
`paper_sized`, defaulting to model units. `Standard` is seeded by the model
and not stored until changed. A project from before opens exactly as it was
(`AProjectFromBeforeSchema11OpensWithNoneAndModelDimensions`), and every
table and kind survives saving twice and reopening
(`EveryTableAndKindSurvivesSavingTwiceAndReopening`).

The geometry blob (`include/katana/entity/geometry_blob.hpp`) has a second
version, `kBlobVersionAnnotation`, written ONLY for what needs it - a label,
a leader, a text with a style, paper height or justification, a dimension of
another kind or with references (`fitsVersionOne`). Everything else is
written in version 1, byte for byte as before, so the pinned wire-format
test still holds and an older build still reads every drawing that uses
none of this; one that does is refused per entity as an unknown version,
loudly.

## Exchange

DXF (`include/katana/dxf/writer.hpp`) has no label entity, and the writer
draws only an aligned dimension and a bare leader itself; it may not see
`katana_cad`, where the placer and the builders are
(`tools/check_layering.cmake`). So a front end draws the rest -
`drawAnnotationForExport` (`include/katana/cad/annotation/export_annotation.hpp`):
labels placed as the plan view places them at the drawing's annotation
scale, every dimension kind but aligned, and leaders with their arrowheads
and callout frames - as lines, closed polylines and single-line texts, and
hands them to the writer (`ExportOptions::drawn`), which writes each on its
entity's layer and in its colour. The window's DXF export and `katana_cli`'s
`EXPORT` both do it, with the document's annotation scale, so both write the
same file; paper-sized text is written at that scale's height. Written
without them (a caller that passes nothing), labels and the other dimension
kinds are skipped with a warning and a leader is its line, landing and note.
A label with no room at the scale is left out and said so. A mask, a width
factor and a slant do not reach the file.

DXF import is unchanged: a file's texts and dimensions come in as texts and
aligned dimensions. The GIS vector export skips annotation with a count, as
it skipped dimensions, and so does the archive exporter.

## Tests

| File | What |
|---|---|
| `tests/entity/annotation/test_label_text.cpp` | the template language: every step, DMS carry, chainage, absent lines dropped, errors named |
| `tests/entity/annotation/test_annotation_model.cpp` | the tables' policies, text blocks, label values of every kind, anchors, the new kinds' validation, bounds and transforms |
| `tests/commands/test_annotation_tables.cpp` | the table commands, a style in use refused with its user, the change-set guards |
| `tests/storage/test_annotation_storage.cpp` | migration 11 and saving twice |
| `tests/cad/annotation/test_text_layout.cpp` | paper height at several scales, justification, readability, masks |
| `tests/cad/annotation/test_label_layout.cpp` | the placer: collisions, scale, suppression, displacement with leaders, order independence, priority, keep-out of lines and of annotation, area labels, dragged labels, chainage ticks |
| `tests/cad/annotation/test_associative.cpp` | following moves, stretches, radii and intersections, erasing, locked layers; one undo step each |
| `tests/cad/annotation/test_auto_label.cpp` | rule matching, one step, re-runs, clearing, chainage rules, the default styles |
| `tests/cad/annotation/test_dimensions_and_leaders.cpp` | every kind built and drawn, paper-sized styles, chains, leaders and callouts |
| `tests/cad/annotation/test_annotation_verbs.cpp` | every verb and its reply, undo as one step |
| `tests/cad/annotation/test_export_annotation.cpp` | what a file is handed |
| `tests/cad/tools/test_annotate.cpp` | the Leader tool's one entity, the Angular, Radius, Diameter and Ordinate Dimension tools |
| `tests/cad/tools/test_annotate_label_tool.cpp` | the Label Objects tool against the `LABEL` line, its style offered, options, undo and refusals |
| `tests/qt_widgets/annotation/test_label_edit_dialog.cpp` | the `LABEL SET` line Edit Label writes, the dialog and the Label Layout Report driven by object name, each line run as the window runs it |
| `qt_labels_are_made_edited_and_reported_in_the_window_headless` | the window end to end: `LBL` answered on the command line, the Properties rows, Edit Label's line through the one executor, the layout report, the Annotate menu |
| `cli.label_layout_names_the_label_that_found_no_room` | `katana_cli`: the suppressed record, and a pinned label placed |
| `tests/dxf/test_writer.cpp` | paper-sized text at the scale, drawn annotation, labels with no room |
| `tests/qt_widgets/annotation/test_annotation_ui.cpp` | painting (a white style prints black, masks, paper height at every scale, the painter's label counts) and the managers and scale box driven by object name |

## Not yet

* **Feet.** A model unit is taken to be a metre (`annotationModelSize`); a
  drawing in feet would need the conversion to read the project's linear
  unit.
* **Placement is worked out on every paint.** It costs what the measurement
  above says for the labels in view; a cache keyed on the model revision,
  the scale and the view is the obvious next step for drawings with tens of
  thousands of labels.
* **Zoom Extents** frames an annotation by its entity's box, which for a
  paper-sized note or leader does not include its text: a note beyond the
  drawing can be cut off at the edge.
* **The Text tool** asks for a model height as before; a style, a paper
  height and a justification are the verb's (`TEXT ... style= paper=
  justify=`, `TEXTEDIT`). The Linear Dimension tool stores the aligned
  projection (`docs/tools.md`).
* **Labels are not picked by their text**: a label is selected at its
  anchor, and moved off its placed position with Annotate > Edit Label
  (`LABEL SET id at=x,y`) rather than by dragging its text.
* **PURGE** does not purge unused text or label styles.
* **DXF**: no MTEXT, masks, width factors or slants on export, and nothing
  on import becomes a label, a leader or a dimension kind.
* **The 3D view** draws a label as a marker at its anchor and a leader as
  its line: its words are worked out for a scale the 3D view does not have.
