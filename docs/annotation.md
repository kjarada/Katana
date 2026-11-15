# Annotation: paper-sized text, labels, dimensions and leaders

Everything a drafter writes ON a drawing rather than draws IN it: text sized
for the sheet, text styles, labels that read their words off the geometry,
rules that label a whole drawing, the placer that keeps labels apart, the
dimension kinds, leaders and callouts, and smart leaders that read their note
off what they point at. The model's side is `katana_entity`
(`rust/crates/katana_entity/src/annotation.rs`, `label_text.rs`, `label_values.rs`,
`leader_values.rs`, `anchor.rs`, `text_block.rs`), the layout and the edits are `katana_cad`
(`rust/crates/katana_cad/src/annotation/`), and the
window's part is a thin front end (`rust/crates/katana_window/src/annotation/`). Built on
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
(`PlanPaintOptions::annotation_scale`, `docs/plan_view.md`, "Annotation"): the
plan view uses the DOCUMENT's annotation scale (`Document::annotation_scale`,
set by `ANNOSCALE` or the Format toolbar's scale box, one undo step), a plot
of the drawing uses the plot's scale, and a sheet viewport its own. So the
same label is the same size on every sheet it appears on, whatever each
viewport's scale.

**Backward compatible by construction.** The default scale is 1:1000, the
one scale at which 2.5 mm is 2.5 model units - the height a `TextGeometry`
has always defaulted to. A text with no style and no paper height is drawn
at its model height, exactly as before, at every scale
(`text_layout::a_text_from_before_styles_keeps_its_model_height_at_every_scale`); a dimension style
is model units unless it says `PAPER on`; a drawing that never set a scale
draws paper-sized text the size of the text beside it. The document keeps
its scale in the project's metadata under `annotation_scale`, a key the
storage layer carries without reading it, so it needed no schema change.

## Text styles

A text style is a row of the `text_styles` table (`TextStyle`,
`rust/crates/katana_entity/src/annotation.rs`), changed only by its table commands
(`create_text_style`, `update_text_style`, `delete_text_style`), like the dimension
styles beside it. The built-in `Standard` style is seeded by the model and
cannot be deleted; deleting a style still used names the first user.

| Field | Meaning |
|---|---|
| `font_family` | the face; empty is the painter's plain-text face |
| `paperHeight` | millimetres on paper; 0 leaves a text its model height |
| `widthFactor` | horizontal stretch, 1 as designed |
| `oblique` | slant of the verticals, radians |
| `bold`, `italic` | |
| `color` | explicit, or empty for the layer's (ByLayer) |
| `mask`, `mask_margin` | a background mask behind the text, reaching `mask_margin` mm past it, painted in the paper on paper and the view's ground on screen |
| `readable` | turn a text that would read upside down half a turn |
| `line_spacing` | a multiple of the standard pitch between baselines, 5/3 of the height - the spacing multi-line text always had - at 1 |

A `TextGeometry` gained three members: `style`, `paperHeight` and `justify`.
Its height on paper is its own `paperHeight`, else its style's, else it is a
model-unit text of `height`. `justify` is the nine points, `TL` to `BR`
(`TextJustify`); a text holds several lines separated by `\n`, each justified
on its own (a right-justified note has a ragged left edge, as on a drawing);
`rotation` turns the block about its justification point. **Readable
auto-flip** (`readable_rotation`): a text whose reading direction points
between straight up and straight down on the left is turned half a turn, and
its justification mirrored so it still covers the same place
(`text_layout::a_readable_text_is_turned_and_covers_the_same_place`) - a label along a line running
west reads left to right.

`layout_text` (`rust/crates/katana_cad/src/annotation/text_layout.rs`) lays a text
out once, into a `Drawing` - text runs, masks, strokes - that every painter
paints the same way. Widths come from a MEASURE: the window's painter passes its
font's (`AnnotationFonts::measure`), the tests and a file export the
estimated one (0.6 of the height a character), so layout is exact on screen
and deterministic headless.

### Text in the window: the Text tools and Edit Text

The Annotate menu's **Text** tool (`TEXT`, `DTEXT`, `DT`) takes the verb's
options at its first prompt - `S` for a text style (`.` for none), `J` for
the justification (`TL` to `BR`), `P` for a height on paper in millimetres
(`0` for a model height) - and **Multiline Text** (`MTEXT`, `MT`; the
`annotate.mtext` tool) takes the same, then a line at a time until an empty
line, and makes ONE text of them, broken by `\n`, as `MTEXT p "a\nb"` does
(`rust/crates/katana_cad/src/tools/annotate_text.rs`). A bare `MT` or `MTEXT` starts the
tool; with arguments the line is the interpreter's verb, as `TEXT` is. Both
make a text by the verb's own rule, `fit_model_height`
(`rust/crates/katana_cad/src/annotation/text_layout.rs`): a paper-sized one - a
paper height, or a style with one - gets its model height for the drawing's
annotation scale and is never asked a height, so a drawn text and a typed
one of the same request are the same entity
(`annotate_text_options::a_style_and_a_justification_make_what_the_text_verb_makes`,
`annotate_multiline_text::every_line_is_one_entity_as_mtext_makes_it`). The choices are
offered from the newest text in the drawing, as its height and rotation
always were, so they carry from one text to the next and through an undo;
Multiline Text starts in `Standard` - paper-sized, as `MTEXT` is - where the
newest text would leave it in model units. A separate Multiline Text tool
was considered and rejected: the two tools differ only in how they commit, so
they are one tool with two modes rather than two copies of the option
prompts.

**Annotate > Edit Text...** (`annotateEditText`, the dialog `textEditDialog`,
`rust/crates/katana_window/src/annotation/text_edit_dialog.rs`) edits the one text
selected: its words over several lines, style, height on paper, model
height, justification, rotation and position. It follows the selection and,
with no single text selected, says so and waits for one - it asks nothing in
a box of its own, so a headless run is never stopped by it. Apply builds ONE
`TEXTEDIT id key=value ...` of the fields the form changed and runs it
through the window's one executor: one undo step, the verb's refusal in the
dialog's words. A text or a name holding a double quote cannot be written on
a command line at all (the interpreter's quotes have no escape), and is
refused before anything runs rather than sent cut short
(`rust/crates/katana_cad/src/annotation/command_words.rs`). A double click on a
text in the plan view opens it on that text (`MainWindow::edit_double_clicked`
triggers `annotateEditText`).

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
use (`LABELSTYLE VALUES kind` lists them; `label_value_names`). Every kind also
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

`rust/crates/katana_entity/src/label_text.rs` is the reference. A template is
literal text with fields in braces, `{value:step:step}`, the steps applied
left to right: unit conversions (`m`, `mm`, `km`, `ft`; `m2`, `ha`, `km2`,
`ac`; `deg`, `rad`, `gon`), a number format (`.3f`, or `+.3f`, which writes a
plus before a value above nothing - `{dz:+.3f}` is `+0.250` or `-0.250`, and
`0.000` for what rounds to nothing, neither above nor below), an angle format
(`dms`, `dms.1`, `dm`, `qb` for a quadrant bearing), the chainage format (`ch`:
`1+234.500`) and `upper` / `lower`. With no format step a value prints in its
quantity's own - a length to 3 decimals, an area to 1, a bearing in whole
seconds - so `{distance}` is already a surveyor's distance. `+.Nf` came with
the leaders that read a surface (2026-10-15): a height difference written
without its sign reads the same above as below.

    {bearing:dms} {distance:.3f}          90°00'00" 40.000
    {area:m2:.1f} m²\n{area:ha:.4f} ha    1000.0 m² / 0.1000 ha
    RL {z:.3f}                            RL 10.250, or nothing without a level
    {chainage:ch}                         0+020.000

A LINE of the template with a value the target does not have is dropped, and
a label whose every line is dropped is not drawn (and `LABEL` refuses to make
one). DMS is rounded ONCE, on the seconds, and carried, so 59.9996" never
prints as 60". A template is checked when the style is stored
(`check_label_template`): an unknown value or step is refused with its name,
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
is usually labelled (`default_label_styles`): Point Number, Spot Level (a
cross and the level), Point Details (number, code, level), Bearing Distance
(whole seconds and millimetres, segments of 5 mm or more on paper), Arc
Data, Lot Area (square metres and hectares, at the centroid, priority 10)
and Chainage (every 20 m, ticks every 10 m).

## The placer

`rust/crates/katana_cad/src/annotation/label_layout.rs` is the reference; in short,
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
   (`label_layout::the_result_does_not_depend_on_the_order_labels_arrive_in`).
3. Each piece takes its first candidate whose text box overlaps no text
   placed before it and crosses nothing it keeps out of. A piece with no free
   candidate is SUPPRESSED and counted; its marker or tick is still drawn.

**What a label keeps out of** is the linework and the other annotation drawn
with it: lines, polylines, arcs and circles, and the strokes and text boxes
of notes, leaders and dimensions (`append_keep_out`), each box by its edges AND
its diagonals, so a label that would fit wholly inside a note's box still
meets it (`label_layout::a_note_is_kept_out_of_even_when_the_label_would_fit_inside_it`). Its own
segment does not push a label away - the offset already clears it. The first
screenshot of the placer showed why the annotation belongs in the keep-out:
a bearing sat on an angular dimension's arrowhead and a lot's area on a
circle inside the lot. An AREA label tries the clear places first and only
then the label point over a mask, because a mask hides what it covers
(`label_layout::an_area_label_moves_off_a_line_before_it_takes_a_mask`).

Overlap is exact for the rotated boxes (separating axes), found through a
uniform grid over their bounding boxes (10 mm cells on paper), so a thousand
labels cost what their neighbours cost. The painter lays out what is in view
and gathers the keep-out as it draws (`docs/plan_view.md`); `LABEL LAYOUT`
gathers the same over the whole drawing (`label_keep_out`), so the command
line's reply is where the plan view puts each label.

**Measured**, `LABEL LAYOUT` over 10 000 point labels at random in a
kilometre square, in the linux-debug build (`-O0` with library assertions;
no optimised build was made for this): 0.40 s a layout including its
10 000-line reply, of which avoiding collisions is about 0.10 s (the same
with `collisions=off` is 0.30 s). At 1:1000, 633 of the 10 000 were
displaced and none suppressed; at 1:200, 12 displaced.

## Auto-labelling

A label rule (`LabelRule`, the `label_rules` table) says which entities get a
label and in which style: a layer glob, a code glob (case folded; `*` and
`?`), an entity type, the layer the labels go on (created if missing), and
whether it is enabled. `AUTOLABEL RUN` (`auto_label`) applies every enabled
rule, or the ones named, as ONE command:

* each matching entity - or alignment, for a chainage style - gets the rule's
  label unless it already has it;
* a label the rule made earlier and no longer asks for is removed;
* a label placed by hand, or another rule's, is never touched, and a dragged
  label keeps its place (`auto_label::a_rerun_keeps_a_dragged_label_and_drops_ones_no_longer_asked_for`);
* a target whose label would say nothing, or whose label layer is locked, is
  counted `skipped`.

Re-running with nothing changed makes no command and so no undo step
(`auto_label::a_run_is_one_step_and_a_rerun_changes_nothing`). `AUTOLABEL PREVIEW` replies what a
run would do without doing it; `AUTOLABEL CLEAR` removes what the rules made
and nothing else. Matching is in entity id order (Rule 7).

## Associativity

A dimension's points, a leader's tip and a label's target can NAME the
entity they belong to (`AnchorRef`: an entity and which of its points -
position, start, end, mid, centre, vertex N, the middle of segment N, a place
ALONG it, INSIDE it). On the command line a point written `#id`, `#id.end`,
`#id.v3`, `#id.s2`, `#id.inside` or `#id.along2:0.25` names one, and
`#id@x,y` names the place on the entity nearest x,y (`nearest_anchor`); the
interactive dimension tools name the lines, arcs and circles they are given
by picking, the Leader tool the entity its tip is clicked on, and - since
2026-09-26 - any point the plan view SNAPPED to an entity's end, middle,
centre or vertex. The view hands the tool what its
snap found (`katana_cad::snapping::snap_anchor` turns an Endpoint, Midpoint or Center snap into
the reference; `katana_cad::interactive_tool::route_snapped_point` gives it to
`InteractiveTool::anchored_point`, whose default is a plain point), and the
Linear, Aligned, Ordinate (feature and datum), Angular-by-points, Baseline
and Continue Dimension tools and the Leader's and Balloon's tip keep it, so
what they make follows the entity exactly as the verb's `#id` points do
(`annotate_leader_options.rs`, `associative_points`). An Intersection,
Perpendicular, Tangent or Nearest snap is not a point one entity keeps, and
stays a plain point.

**Along** (added with smart leaders) is a fraction of the way along: a line's
or an arc's from its start, a polyline's segment N and the fraction along it,
a circle's fraction of a turn counter-clockwise from east. `#id.alongN:T`
types it exactly - segment N (0 on a line, an arc or a circle), fraction T
from 0 to 1 - as `LIST` prints it ("along 2 0.25"), and so as the window's
Along sets it; `#id@x,y` names only the place nearest a point, which at a
polyline's corner is the first of the two segments meeting there
(`smart_leaders::an_exact_place_along_is_typed_as_long_as_the_entity_has_it`). A circle's turn is
[0, 1): a whole turn is typed as, and stored as, its start. A stored fraction
that is not a number reads as 0, the start
(`smart_anchors::a_fraction_that_is_no_number_reads_as_the_start`). It is what keeps a
tip at a quarter of the way along a pipe when the pipe is stretched to twice
the length (`smart_leaders::a_tip_on_a_line_stays_at_its_place_along_it`). A circle has no start, so a
tip on a circle keeps its compass direction from the centre as the circle
moves or grows; rotating a circle turns nothing a tip could follow.
**Inside** is a closed polyline's inside point - the centroid, or the middle
of the widest run through it when the centroid is in a notch (`inside_point`,
the point an area label goes at) - or a circle's centre.

A copy follows its original's references only where it should: an
annotation COPIED, MIRRORED or ARRAYED together with what it refers to
refers to the copy, so a pit copied with its callout gets a callout of its own
(`smart_leaders::a_copy_of_a_pit_with_its_callout_gets_a_callout_of_its_own`,
`annotation_tables::copies_of_annotation_refer_to_copies_of_what_they_refer_to`), and so it does from the
window's Copy tool, Rotate and Scale with their Copy option and the polar
Array, which make their copies with the same command,
`katana_commands::entity_commands::duplicate_entities` (`copying_tools::a_callout_copied_with_its_pit_reads_the_pits_copy`).
Copied alone, it is another annotation of the original. Before smart leaders
a copied pair snapped the copy's tip back onto the original pit.

Rather than teach every command about annotation, `Document::execute` wraps
EVERY command in `with_associative_update`
(`rust/crates/katana_cad/src/annotation/associative.rs`): after the command runs,
the annotations that follow what it changed are brought up to date as a
second change set INSIDE the same undo step, so one undo puts back the
geometry and everything that followed it. A radius follows its circle's
centre and radius and keeps its direction; an angle between two lines
follows their intersection; a label follows its target and is removed with
it (and comes back with it on undo); a SMART leader goes with its target the
same way, and so does a smart leader reading a leader that went, however
long the chain; a reference to an erased entity is
dropped, leaving an ordinary dimension or a plain leader where it was, as a
CAD user expects. An annotation on a locked layer is left alone.

## Dimensions

`DimensionGeometry` gained a `kind` (`entity.rs`, `DimensionKind`); Aligned
is the default, so every dimension written before is an aligned one and is
stored byte for byte as it was.

| Kind | Measures | Made by |
|---|---|---|
| Aligned | the true distance, its line parallel to the points | `DIM ALIGNED`, `DIM p p offset`, the Aligned Dimension tool |
| Linear | along a direction: horizontal, vertical or rotated | `DIM LINEAR` / `HORIZONTAL` / `VERTICAL`; the Linear Dimension tool for Rotated, for a line between the origins' levels and for snapped origins (otherwise it stores the aligned projection, `docs/tools.md`) |
| Angular | the angle at a vertex, the side the arc is dragged to | `DIM ANGULAR`, the Angular Dimension tool |
| Radius, Diameter | an arc's or a circle's, from its centre | `DIM RADIUS` / `DIAMETER`, the Radius and Diameter Dimension tools |
| OrdinateX, OrdinateY | a feature's X or Y from a datum | `DIM ORDINATE`, the Ordinate Dimension tool |

`DIM BASELINE dim p...` adds dimensions measured from the same first point,
each a spacing further out; `DIM CONTINUE dim p...` chains them end to end
(`dimension_build::baseline_and_continued_chains`). The Baseline and Continue Dimension tools
make the same chains through the same door, `katana_cad::annotation::dimension_build::dimension_chain`
(one command, on the base's layer and in its style), going on from the
drawing's newest linear or aligned dimension until Select picks another. A dimension style's sizes are model units,
or with `paper_sized` (`DIMSTYLE SET name PAPER on`) paper millimetres drawn
at the annotation scale (`dimension_draw::a_paper_sized_style_is_the_same_on_paper_at_every_scale`). The
arrowheads are closed filled, open, tick, dot or none (`ArrowHead`, which
moved from the dimension style to the entity header so a leader shares it).
An angle is written in DMS, a radius with `R` and a diameter with `Ø`.
`build_dimension` draws every kind for the painters; the aligned drawing is
exactly what it was before the kinds (`dimension_draw::the_aligned_drawing_is_what_it_was_before_the_kinds`).

### Dimension styles

A dimension is drawn in its LAYER's dimension style (`LAYER DIMSTYLE layer
style`), else `Standard`. `DIMSTYLE` names thirteen fields - `TEXT GAP EXTOFF
EXTBEYOND ARROW HEAD SCALE DECIMALS ROUND PREFIX SUFFIX TRIM PAPER` - and
`rust/crates/katana_cad/src/annotation/dimension_style_verbs.rs` is the one
statement of each: its name, how its value is read and how it is written
back. The verb reads its lines with it, `DIMSTYLE INFO` writes its record with
it, and the window's manager builds its lines with it, so the dialog cannot
write a line the verb would read differently.

* `DIMSTYLE SET name field value [field value ...]` sets every pair as ONE
  undo step, all or nothing: one pair that does not read, or a style that
  fails `validate`, refuses the whole line and changes nothing
  (`dimstyle_set_many::one_bad_pair_refuses_the_whole_line_and_changes_nothing`). It took
  one pair until 2026-09-26, so a dialog's Apply of five fields was five
  undos.
* `DIMSTYLE NEW name [field value ...]` takes the same pairs, so a copy of a
  style is one step. Duplicate running NEW and then SET was considered and
  rejected: that is two undos for one act.
* `DIMSTYLE INFO name` is one record: `name=`, every field in lower case,
  `layers=` (how many layers name the style) and `reads=` (what a dimension
  of ten units reads as). `DIMSTYLE LIST` keeps its short line a style.
* `TRIM` and `PAPER` take on/off (yes/no, true/false, 1/0) and nothing else,
  and `DECIMALS` a whole number: "PAPER of" once turned paper sizing off
  without a word, and "DECIMALS 2.7" gave two places.

A value that holds a blank is quoted (`PREFIX "L= "`, `SUFFIX ""` for none).
The interpreter's quotes have no escape, so a prefix or suffix holding a
double quote cannot be written on a line; the dialog says so rather than send
a line that sets something else (`rust/crates/katana_cad/src/annotation/command_words.rs`).

## Leaders and callouts

A leader (`LeaderGeometry`) is ONE entity from its arrow to its note: the
vertices (tip first, any number of bends), the note (`\n` between lines), the
arrowhead kind and size, a landing length, the text style and height, a
callout frame - `box` or `circle` - and optionally the entity its tip
follows. Sizes are paper millimetres. The landing runs away from the last
segment, level on the sheet, and the note sits half a text height beyond it,
left-justified to the right and right-justified to the left
(`leader_draw::the_landing_runs_away_from_the_line_and_the_note_sits_beyond_it`); a circle callout is
the smallest circle clearing its note. `BALLOON` is a circled leader whose
number counts on from the highest balloon in the drawing (or `n=`;
`katana_cad::annotation::leader_edit::next_balloon_number`, which the Balloon tool numbers by too). The
Leader tool makes the same entity (`docs/tools.md`), with `LEADER`'s arrow,
callout, text style, paper height, arrowhead size and landing as options for
the leader being drawn, and the Balloon tool `BALLOON`'s number, text style
and paper height;
it used to draw a polyline, an arrowhead and a text per line, which drifted
apart when one was moved.

Imported drafting leaders may specify `LeaderGeometry::landing_angle`, the
outward direction in radians, rather than asking it to be inferred from the
last leg. The builder honours it for the landing and readable text, including
a rotated callout; rotating or mirroring the entity transforms the direction
as well as the vertices. Absent, existing leaders behave exactly as before.
The optional geometry JSON member is `landingAngle`; `LEADER LIST` and the
MCP leader report give it as `landing_angle` when present. Geometry blob version 4 appends
one double to the version-3 leader payload and is written only when this
direction is present; old plain and smart leaders retain their existing
versions and byte layouts. Tests pin the new bytes and reject a truncated
angle (`docs/model.md`, "The kinds appended since").

### Moving and framing existing leaders

Select a leader in the plan view to expose square grips on its free arrow
tip, every bend and its note end. Drag a grip, or click it, type a coordinate
and press Enter, to move that point as one undo step. The text, frame, sizes
and landing direction stay with the leader. An attached arrow tip has no
free-move grip: use Attach or Along to change its attachment, or Detach
first. The same rule is enforced by the shared point-edit operation, not
just by hiding a handle.

Double-click the leader, or right-click **Edit Leader...**, to open its form
in **Annotate > Leaders...**. The **Position** group chooses Arrow tip, a
bend or Note end and accepts exact Easting and Northing values in model
units. Choosing another point only shows its coordinates; it is not an edit.
**Apply** combines changed coordinates and appearance into one `LEADER SET`
line and one undo step. Several selected leaders may share an appearance
edit, but an absolute point edit requires one leader so their notes cannot
accidentally be piled onto one coordinate.
Apply re-reads the stored leader before the coalesced change notification:
an immediate Undo then restores the coordinate fields instead of mistaking
the applied values for edits that were never submitted.

In **Look**, **Note frame** offers none, box and circle. **Landing direction**
accepts Auto or degrees: 0 points right, 180 left, and Auto derives the side
from the final segment. This also edits a direction preserved from an archive.
The existing arrow, text style, text height, arrow size and landing length
controls remain beside it. The right-click **Leader Note Frame** submenu
applies the same frame options to the selected leaders through the shared
scope parser; other selected entity kinds are unchanged.

```text
LEADER SET 12 at=25,18 callout=box
LEADER SET 12 vertex=1 at=15,20 landingangle=180
LEADER SET 12 landingangle=auto
LEADER SET SELECTION callout=box
LEADER SET DRAWING callout=none WHERE LAYER=notes
LEADER LIST 12 JSON
```

Vertex indices are zero-based: 0 is the arrow tip and the last point is the
note end. `vertex=` requires `at=`; an out-of-range index, non-finite
coordinates or direction, ambiguous tip assignments and degenerate results
are refused without changing the leader. `move_leader_vertices` is the one
headless operation behind both the command and the grip editor. Point grips
move existing points; they do not delete bends, and Delete on a hot leader
point does not erase the whole leader.

The manager's new controls are `leaderPositionPoint`, `leaderPositionX`,
`leaderPositionY`, `leaderPositionHint` and `leaderLandingAngle`. Invalid
text is an explicit form error and no command runs. Its form is scrollable
while Apply, Freeze and Detach remain outside the scroll area. `LEADER LIST
JSON` and MCP's `katana_leaders` expose the full `points` array, `text_style`,
`paper_height`, `arrow_size`, `landing`, and the optional `landing_angle`
in radians, so an agent can inspect the same positions and appearance.

`rust/crates/katana_cad/tests/annotation_drawing_verbs/leader_positions.rs` checks point edits, scoped
frames, refusals, atomic multi-grip moves and attached tips.
`rust/crates/katana_window/tests/annotation/leader_manager_positions.rs` drives the
exact-coordinate fields and their errors; the plan-view grip and context-menu
tests exercise the mouse gesture and frame commands. CLI, MCP and the
headless window all edit the same bent leader and check its points and box.

## Smart leaders

Built on 2026-09-25 at the owner's request for "more functionality for
leaders to interact with geometry and attributes, make them smart". A SMART
leader's tip is ON an entity and its note is READ OFF it: the pit's invert,
the pipe's size and length, the chainage along the main to the tip, the lot's
area - worked out every time the leader is drawn, as a label's words are, so
the note cannot disagree with the geometry or the attributes however either
was edited, by hand, by `PROP`, by Global Modify or by an agent. Two members
of `LeaderGeometry` say so, both appended with their defaults being a plain
leader:

* `fields`: the leader's `text` is a TEMPLATE in the label template language
  (`rust/crates/katana_entity/src/label_text.rs`, "Templates" above) -
  `LEADER #12 25,15 template="IL {prop.invert:.3f}\n{length:.1f} m"`;
* `labelStyle`: the note is that label style's template, read live - one
  `LABELSTYLE SET` and every leader in the style says the new thing. Only the
  template: the look is the leader's own, and `LEADER` lends it the label
  style's text style and paper height when it makes it (unless `style=` or
  `paper=` says otherwise), so it looks as the style's labels do. A leader's
  note is its own template or its style's, never both, and a style a leader
  reads cannot be deleted (`annotation_tables::a_label_style_a_leader_reads_is_not_deleted_and_must_exist`).

A plain leader's note is still exactly what was typed, braces and all.

### What a leader can say

`rust/crates/katana_entity/src/leader_values.rs` is the reference, and `LEADER VALUES
#id@x,y` lists what a note at a place could say, each value in its own format.
The values are those of the entity the tip names AT THE TIP
(`anchor_values`): the whole set of names is `leader_value_names`, and a
template naming anything else is refused as it is typed, with the name
(`check_leader_template`).

| Target | Values |
|---|---|
| every one | `id`, `layer`, `type` (Point, Line, ...), `code`, `point`, `description`, `prop.NAME`, and `x`, `y`, `easting`, `northing` of the tip |
| Point, Text | `z`, `rl` - its level; a text's `text` |
| Line | `bearing`, `distance`, `length`, `dx`, `dy`, `segment`, `chainage` (from its start to the tip), and `z`, `rl` at the tip, `dz`, `grade` from its heights |
| Arc | `radius`, `diameter`, `length`, `chord`, `delta`, `bearing` (of the chord), `tangent`, `chainage`, `z`, `rl` |
| Circle | `radius`, `diameter`, `length`, `area`, `perimeter`, `z`, `rl` |
| Polyline | the segment the tip is on - `bearing`, `distance`, `dx`, `dy`, `dz`, `grade`, `segment` - and the whole line's `length`, `vertices`, `chainage` to the tip and `z` at it; a closed one's `area` and `perimeter` |
| Dimension | `measurement`, or an angular one's `angle` |

Three rules decide what is there, each the model's policy already:

* **`length` is the whole entity's.** A callout on a pipe says the pipe's
  length; the segment the tip landed on is `distance`. A segment label's
  `length` is its segment's, since a label is of that segment.
* **Absent is not zero.** A level at the tip is the height of the vertex it
  is at, or interpolated between two vertices that both have one - never made
  up from one end (`leader_values::a_line_says_its_bearing_and_the_level_and_chainage_at_the_tip`); a
  tip inside a lot has a level only when every vertex is at one. A template
  line naming a value the target lacks is dropped, as a label's is.
* **The place, else the nearest.** When a reference names a place the target
  no longer has - a vertex since deleted - the place on the target nearest the
  tip stands for it, rather than the leader going blank.

A leader that would say NOTHING - every line dropped - is refused on the way
in by `LEADER` and the Leader tool, naming what the target lacks ("the leader
would say nothing: its target has no prop.invrt"), since a property the
target does not carry is far more often a typo than an intent
(`check_leader_says_something`). So is a smart leader whose tip is on nothing.

### What a leader reads of the drawing

Built on 2026-10-15 at the owner's request for "a leader to display a point's
height, a leader to get a point's difference to a TIN, a leader to display
names". A note may also read the drawing's SURFACES (`Document::surfaces()`,
`docs/terrain.md`) and ALIGNMENTS at its tip:

| Value | Is |
|---|---|
| `surface.NAME` | the level of surface NAME at the tip |
| `dsurface.NAME` | the target's level (`z`) minus that: positive ABOVE the surface |
| `cut.NAME` | how far the target is below the surface - the cut down to it - present only there |
| `fill.NAME` | how far it is above (or exactly on: a fill of nothing), present only there |
| `station.NAME` | the tip's station along alignment NAME, a chainage (`{station.MC01:ch}` is `0+030.000`) |
| `offset.NAME` | its offset from the centreline, positive LEFT looking along increasing station |
| `profile.NAME` | the alignment's design profile level at that station |
| `dprofile.NAME` | the target's level minus that: positive above the design |

So "CUT {cut.ground:.2f}\nFILL {fill.ground:.2f}" says one line, whichever
holds, and `{dsurface.ground:+.3f}` writes `+0.250` above and `-0.250` below.
Decisions:

* **The entity layer reads them through a context, not a reference.** The
  values are worked out in `katana_entity::leader_values::leader_note`, which every painter, the
  command line and the DXF export share - and `entity` may see neither
  `terrain` nor `cad` (its `Cargo.toml` depends on neither). So a plain
  `katana_entity::leader_values::LeaderContext` - a function giving a surface's level by name and
  point, one giving an alignment's station, offset and profile level, and the
  two lists of names - goes through `anchor_values`, `leaderValues`,
  `leaderNote` and `check_leader_says_something`, and `katana_cad::annotation::leader_presets::leader_context`
  makes it from a Document (`rust/crates/katana_cad/src/annotation/leader_presets.rs`).
  It holds COPIES - of the surface store, a pointer a surface, and of the
  alignments' definitions, each solved once when a leader first reads it -
  so it reads the drawing as it was when it was made and is safe on another
  thread (the background DXF export holds one). Rejected: a `Document`
  pointer in the context (dangles under a sheet painted after the drawing is
  replaced) and computing a station in the entity layer itself from the
  model's alignment table (it would solve every alignment again on every
  leader of every paint; the cad context solves each once).
* **Every caller passes the document's** - the plan painter (`PlanSource::leaders`,
  made by `plan_source_of`, so plots and sheet viewports too), `LIST` and `INFO`,
  every `LEADER` and `BALLOON` verb, the label keep-out of `LABEL LAYOUT`, the
  property panel, the Leaders manager, the Leader and preset tools, the
  DXF export (`EXPORT` and the window's, both through `write_dxf_export`) and
  the IFC export (`EXPORT x.ifc`, katana_cli's and the window's, through
  `katana_app::ifc_verbs::set_leader_notes`: an IFC leader is its note, not its template). A
  caller that passes none reads no surface: such a line is dropped, never
  written as nothing. The DXF writer's own fallback - a caller that hands it
  no drawn annotation - cannot see `cad` and passes none.
* **Only what the template names is read.** A note is made on every paint,
  and a drawing may hold many surfaces: `leaderNote` reads the context values
  its template names; `LEADER VALUES`, the Leaders manager and `anchor_values`
  read every one of every surface and alignment, to offer them.
* **NAME is written as the drawing has it, blanks and all**:
  `{dsurface.Existing Ground:+.3f}`. Everything between the dot and the first
  `:` is the name, which is how the template language already read a field.
  A name holding `:`, `{` or `}` cannot be written; a preset refuses it with
  "rename it". Rejected: quoting a name (`{surface."Existing Ground"}`), a
  second syntax for one case, and the quotes cannot be typed on the command
  line inside a quoted template anyway ("Not yet", the double quote). A
  surface is found ignoring case, as `SURFACE` finds one; an alignment
  exactly, as `ALIGN` does.
* **Offset is positive LEFT**, looking along increasing station: the sign
  `SolvedAlignment::point_at_station_offset` and a cross section
  (`rust/crates/katana_cad/src/corridor.rs`) already use. Rejected: right positive, as some road
  texts write it - a second convention in one program is a sign error waiting.
  The station of a point is `SolvedAlignment::station_offset`, the inverse of
  `point_at_station_offset` (`docs/geometry.md`).
* **On the surface is a fill of nothing**: `fill` holds when the target is at
  or above it, `cut` when below. Rejected: neither at exactly nothing, which
  would make a cut/fill note vanish - and the leader be refused - for a point
  that is exactly on grade. "On" is within the geometric tolerance
  (`katana_math::numerics::GEOMETRIC`, 1e-7), not exactly equal: a surface's level
  is interpolated, so a point whose RL is the plane's to the last digit comes
  out a few bits either side of it, and below it was "a cut of 4e-15" - `CUT
  0.000` for one on-grade point and `FILL 0.000` for its neighbour (61 of 342
  on a plane TIN), and a fill-only leader refused as "below". `dsurface` is 0
  there too. A micrometre under the surface is a cut
  (`leader_context_values::a_point_on_the_surface_within_the_tolerance_is_a_fill_of_nothing_not_a_cut_of_the_rounding_error`,
  `leader_presets::every_point_on_the_surface_inside_a_triangle_is_a_fill_of_nothing_never_a_cut`).
  Rejected: a looser tolerance of its own - the project has one.
* **Absent stays absent**: a tip off the surface (outside it, in a hole),
  beyond an alignment's ends (no perpendicular reaches it: a station is never
  clamped to the end), at a station the profile does not reach, or a target
  with no level leaves the value out and its line is dropped. A refusal says
  WHY, which a missing value alone did not: "its target has no
  dsurface.ground (the tip is off surface ground)", "(the drawing has no
  surface grnd)", "(the target has no level)", "(the target is not below
  surface ground)", "(alignment MC01 has no design profile at station
  90.000)" (`katana_entity::leader_values::why_absent`).
* **A smart leader's tip is still on an entity.** A surface's level at a
  bare place is not a leader's: a note reads what it points at, and the
  surface beneath it. "Not yet" has the free tip.
* **A surface or an alignment is read by name, and the name follows a
  rename.** `SURFACE RENAME` rewrites the leaders whose own template names the
  surface - `{surface.NAME}`, `{dsurface.NAME}`, `{cut.NAME}`, `{fill.NAME}`,
  found ignoring case as the store finds a surface - to the new name in the
  SAME undo step as the rename and the corridors and road objects it already
  carries (`katana_cad::annotation::leader_edit::rename_leader_reads`; the steps, the literal text, the
  line breaks and the doubled braces of a template are kept,
  `katana_entity::label_text::rename_template_fields`). A preset leader so still reads the surface
  and `recognise_preset` still knows it. The reply says `leaders=N`. Two kinds
  of leader are left reading the old name, counted in a `warning` record and
  not hidden: one whose new name no template can write (empty, or holding
  `:`, `{` or `}`, as `preset_template` refuses the same), and one on a locked
  layer, because a single leader in the way would have refused the whole
  rename, which is worth more than they are
  (`leader_scope::a_name_a_note_cannot_write_leaves_the_readers_and_says_so_in_a_warning`,
  `leader_scope::a_leader_on_a_locked_layer_is_left_reading_the_old_name_and_the_rename_still_runs`).
  A label style's template cannot name a surface (`check_label_template`), so
  none is rewritten. `SURFACE REMOVE` and `ALIGN DELETE` do not refuse while a
  leader reads the thing: they say, in a `warning` record, how many leaders
  will read nothing (`katana_cad::annotation::leader_edit::leaders_reading`) and go on, and UNDO brings
  the thing back with its readers. Rejected: refusing the removal while a
  leader reads it - the choice the road objects made too (`docs/road_objects.md`:
  a removal that is refused makes a surface that was meant to go stay) - and
  a quiet removal, which leaves a leader drawn bare with nobody told. There
  is no alignment rename (no `ALIGN RENAME`, no command), so there is nothing
  for the readers of an alignment to follow yet: `rename_leader_reads` takes
  `ReadKind::Alignment` and is tested, for the verb that will call it
  (`leader_scope::rename_leader_reads_follows_an_alignment_by_the_function_the_rename_will_use`).

### Presets

A preset is a template written once, made by every front end through
`katana_cad::annotation::leader_presets::resolve_leader_preset`:

| `preset=` | Tool | Template | Says |
|---|---|---|---|
| `level` | Level Leader | `RL {rl:.3f}` | the point's level |
| `surface` | Surface Difference Leader | `{dsurface.NAME:+.3f}` | how far above (+) or below (-) the surface |
| `cutfill` | Cut/Fill Leader | `CUT {cut.NAME:.3f}` / `FILL {fill.NAME:.3f}` | the cut to a point below, the fill to one above |
| `name` | Point Name Leader | `{point}` | the point's name (its point number) |
| `code` | Code Leader | `{code}` | its survey code |
| `details` | (none) | `{point}` / `{code}` / `RL {rl:.3f}` | each line that holds |
| `station` | Station Leader | `CH {station.NAME:ch}` / `OS {offset.NAME:+.3f}` | the station and the offset, left positive |

The level to the millimetre as a survey plan writes one; a difference with
its sign always written; the chainage form a long section uses. NAME is the
`surface=` or `alignment=` given - the drawing's spelling of it goes into the
template - or, with none, the FIRST VISIBLE surface (the first alignment by
name), and the reply says so: `chosen="first visible surface"`. Rejected:
refusing a preset with no name (one surface is the common drawing, and the
person would be asked what there is one of), and choosing without saying
(a leader made against a surface nobody chose, in silence). The leader is a
smart leader like any other - its template is the preset's, read live - so
`LEADER LIST`, the property panel and the Leaders manager say which preset a
template is (`recognise_preset`), and a template edited away from it is no
preset. Refused: an unknown preset, `surface=` or `alignment=` without one
or for a preset that reads neither, a name the drawing has not, a drawing
with none to take, every surface hidden, and - as any leader - one that would
say nothing.

### Pointing at the right place

`#id@x,y` puts the tip on the entity nearest x,y and names the place ALONG it
("Associativity" above), so the tip stays at the same place of a line that
moves or stretches, and the chainage and level it reads are the new ones.
`#id.inside` points into a lot; a leader ending inside an outline ends in a
DOT unless `arrow=` says otherwise, one ending on it in an arrowhead
(ISO 128-22, leader lines; `smart_leaders::inside_a_lot_the_arrow_is_a_dot_and_the_note_its_area`). `LEADER
ATTACH id #id@x,y` moves a leader's tip onto an entity, `LEADER SET id
tip=...` too, and the Leader tool puts a tip clicked within the pick aperture
of a point, line, arc, circle, polyline or text on it (`docs/tools.md`).

### Changing what an entity says, through its leader

`LEADER PROP id SET invert 10.5 [real]` sets the attribute on the entity the
leader's tip is on - the same command `PROP SET` is, one undo step - and
replies with the note as it now reads; `LEADER PROP id DELETE key` removes it
(`smart_leaders::a_template_is_read_off_the_pit_and_follows_its_attributes`).

### Many at once

`LEADER FOR id... | <scope> [template= | labelstyle= | text=]` makes a
leader to each entity as ONE step: its tip at a point's or a text's position,
the middle of a line or an arc, halfway along an open polyline, inside a
closed one, and on a circle on the side the note goes; the note `length=` mm
(10: four of a leader's default 2.5 mm text heights, clear of the tip and a
point's symbol) from the tip at `angle=` degrees (45: up and to the right,
off the horizontal and vertical most drawn lines follow) - chosen, not a
standard's, and kept in `LeadersForOptions` alone - at the document's
annotation scale. An entity that offers no place (a dimension, a label, a leader) or
about which the note would say nothing is skipped and counted, and the reply
says the first reason when nothing was made
(`smart_leaders::for_makes_one_leader_per_entity_in_one_step`). `BALLOON FOR` numbers a balloon to
each on from the highest; a smart balloon (`template=`) is no number in the
run. A numbered balloon stays a circle: only a circle's number is counted,
so `BALLOON` or `BALLOON FOR` with no note and `callout=box` or `none` is
refused rather than make a number that would be given again
(`leader_edit::a_numbered_balloon_is_a_circle_so_its_number_is_counted`).

### Arranging them

`LEADER ALIGN id id... | <scope> [x=] [spacing=mm]` lines the notes up in a
column, as AutoCAD's MLEADERALIGN does: each leader's last vertex - where its
note hangs - goes to one x (the topmost note's, or `x=`), and with `spacing=`
the notes are stacked that many paper millimetres apart from the top down, in
the order they were; nothing else of a leader moves
(`smart_leaders::align_puts_the_notes_in_a_column_top_down`). `BALLOON RENUMBER [start=1]
[order=id|x|y]` numbers the numbered balloons again from `start`, in the
order they were made, across the sheet by their tips, or down it; a smart
balloon is left alone (`smart_leaders::renumbering_numbers_the_balloons_in_the_order_asked`). Each
is one step, and none when nothing moves.

### Scope and filter

Every verb that reads or changes a SET of drawing data takes the one grammar
Global Modify, `MODIFY`, `EXPORT` and the UTILITY verbs take (the contributors'
contract, "Every tool acts on a scope and a filter"; `docs/cad.md`), in place of ids:

    [SELECTION | VIEW [id] [EXTENTS] | DRAWING | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY]]
    [WHERE TYPE=.. LAYER=.. STYLE=.. COLOUR=.. PROP=.. TEXT=.. DRAWN]

It is read by `katana_cad::scope_verbs::parse_scope_words` and resolved by `katana_cad::scope_verbs::match_scope` -
`katana_cad::annotation::leader_presets::match_leaders` narrows what that matched to the leaders, so the
`matched=` it reports counts leaders - and there is no second reader of a
scope here. `LEADER LIST [JSON]`, `VALUES`, `SET`, `FREEZE`, `DETACH`,
`PROP`, `ALIGN` and `BALLOON RENUMBER` act on the leaders it takes; `LEADER
FOR` and `BALLOON FOR` on the entities it takes - of any kind, those with no
place for a leader counted in `skipped=` as before. `ATTACH` puts one tip on
one place and stays an id. Decisions:

* **The reply leads with the scope's record**, as the other verbs' do:
  `scope=layers layers=notes sublayers=yes matched=4`, then the verb's own
  reply (`frozen leaders=3`). `VALUES` adds `skipped=` (leaders whose tip is on
  nothing have no values), `PROP` adds `entities=` and `skipped=` (two leaders
  on one pit set it once, in one step). Ids carry no record.
* **A scope that takes nothing is said, not refused, and never means
  "all"**: `matched=0`, `updated leaders=0`, no step; `LEADER LIST DRAWING
  WHERE TYPE=point JSON` is `"leaders": []`, not every leader
  (`leader_scope::a_scope_that_takes_no_leader_lists_none_and_never_all`: an empty id list
  means "all" to `leaders_json`, so the scope's goes through `leaders_report`,
  which tells none from all). The one exception is MODIFY's: an EDIT of
  `SELECTION` with no leader selected (nothing, for `FOR`) is refused as it was -
  "no leader is selected; use SELECT first" - since forgetting to select is a
  mistake a script should stop at. Rejected: `matched=0` there, which lets a
  script that selected nothing go on as if it had worked.
* **Ids or a scope, never both**: "name the entities by id or by a scope".
  Rejected: their intersection, which makes `LEADER FREEZE 5 DRAWING` mean
  something nobody typed on purpose. With neither, a verb is as it was: `LEADER
  LIST` lists every leader, `BALLOON RENUMBER` numbers every numbered balloon.
  Any scope or `WHERE` word makes the grammar's rule apply, `WHERE` alone
  being the selection's filter. A scoped `RENUMBER` numbers the balloons it
  takes from `start=` and leaves the others, so it can give a number another
  balloon has; no scope numbers them all again.
* **The verb's own options go before `WHERE`**: after it a key that is a WHERE
  key (`TYPE LAYER STYLE COLOUR PROP TEXT`) is the condition and not the
  option, which matters for `text=` and `style=` (the note, the text style),
  so `LEADER FOR DRAWING text=NOTE WHERE TYPE=point LAYER=plan`
  (`leader_scope::a_where_key_after_where_is_the_condition_and_not_the_verbs_own_option`).
  Any other option (`preset=`, `arrow=`) may stand after it. Rejected: reading
  `style=` after `WHERE` as the option, which would make the same word two
  things in one line. `PROP` takes the scope before `SET` or `DELETE`, which
  end it (the word after `LAYERS` is a layer, even one called `set`).
* **`VIEW` is the window's plan view**, with its own hidden layers and its area
  (`VIEW <id>` another view, `VIEW EXTENTS` without the area); headless it is
  refused in favour of `AREA`, as everywhere (`ScopeViewProvider`).
* **MCP**: `katana_leaders` takes `scope`, `area`, `scope_layers`, `only` and
  `scope_where` (the names `katana_import` uses, read by the same
  `scope_words_of`), reads the leaders the scope took from the Document - still no
  line in the history - and adds `scope: {record, matched}` to its object; with
  `ids` too it is refused. `LEADER LIST ... JSON` prints the same object.

### One set of edits

`rust/crates/katana_cad/src/annotation/leader_edit.rs` is every leader edit - the
note and look (`change_leaders`, `apply_leader_change`), attaching, freezing
and detaching, an attribute through the leader (`set_leaders_target_property`,
for many at once), one per entity (`leaders_for`), aligning, renumbering (of
the balloons a scope took, `renumber_balloons`), and the notes that read a
renamed surface (`rename_leader_reads`) - each checked against the model and
returned as ONE command or refused - a template that does not check, a
style not in the drawing, a size below 0 mm (`paper=-3` included; 0 is the
text style's height, or no landing) - never half made. The verbs parse their
words into it, and the window's Leaders manager ("In the window" below)
writes the line a person would type and hands it to the same verbs, so a verb
typed and a button pressed are one code path.

### Freezing and letting go

`LEADER FREEZE id...` turns a smart note into the words it says now and
keeps the tip following; `LEADER DETACH id...` lets the tip go, freezing a
smart note first, since a note read off nothing would say nothing. Both are
one step. `INFO` and `LIST` print what a leader says, its template and what
it is on; the property panel shows the same (`Text`, `Template`, `Label
style`, `On`).

## The verbs

`CommandInterpreter::annotation_help_text` is the reference, printed by `HELP`.
Options are `key=value`, `\n` in a text or template is a line break and
`\\` a backslash - so a note holding `\n` itself (`C:\\new`) can be typed,
as a reply writes it (`katana_core::text::unescape_typed`, the sheet verbs' reader too;
`smart_leaders::a_backslash_is_typed_as_two_so_a_note_may_hold_backslash_n`) - and every reply is
`key=value` records an agent can read without guessing:

| Verb | Replies, for example |
|---|---|
| `ANNOSCALE [N \| 1:N]` | `annoscale=500` |
| `TEXTSTYLE LIST \| NEW \| SET \| DELETE \| INFO` | `created text style name=Notes font="" paper=3.5 width=1 ... spacing=1` |
| `TEXT p [height] "text" style= paper= justify= rotation=`, `MTEXT`, `TEXTEDIT id` | `created text id=7 style=Standard paper=3.5 justify=MC height=0.7` |
| `LABELSTYLE LIST \| NEW \| SET \| DELETE \| INFO \| DEFAULTS \| VALUES kind \| CHECK kind template` | `added=7 styles=...`, `kind=point values=id,layer,...` |
| `LABEL id... style=`, `LABEL SELECTION`, `LABEL ALIGN name`, `LIST`, `SET`, `DELETE`, `LAYOUT [scale= collisions=]` | `scale=600 considered=5 placed=4 displaced=0 suppressed=1 orphaned=0`, a `label=... x= y= candidate= text=` line for each piece placed and a `label=... piece=... suppressed=yes` line for each that found no room |
| `AUTOLABEL RULE ADD \| SET \| DELETE \| LIST`, `RUN`, `PREVIEW`, `CLEAR` | `autolabel created=2 kept=0 removed=0 skipped=0` and `rule=... labels=N` |
| `DIM LINEAR \| HORIZONTAL \| VERTICAL \| ALIGNED \| ANGULAR \| RADIUS \| DIAMETER \| ORDINATE \| BASELINE \| CONTINUE` | `created dimension id=5 kind=diameter measures=10 text=Ø10.000 associative=yes`, `created dimensions=2 ids=7,8` |
| `LEADER p p... text= \| template= \| labelstyle= arrow= callout= style= paper= arrowsize= landing=`, `BALLOON` | `created leader id=26 text="IL 10.500" associative=yes target=12 anchor=position template="IL {prop.invert:.3f}"` |
| `LEADER p p... preset=level\|surface\|cutfill\|name\|code\|details\|station [surface=\|alignment=]`, also on `LEADER FOR` and `LEADER SET` | `created leader id=27 text=+4.000 associative=yes target=12 anchor=position template={dsurface.ground:+.3f} preset=surface surface=ground chosen="first visible surface"` |
| `LEADER PRESETS` | `preset=surface title="Surface Difference" reads=surface template={dsurface.NAME:+.3f} tool=annotate.leader.surface`, one a preset |
| `LEADER LIST [id... \| <scope>] [target=id] JSON` | `{"leaders": [{"id": 27, ..., "preset": "surface", "reads": "ground", "text": "+4.000", "lines": ["+4.000"]}], "presets": [...], "surfaces": ["ground"], "alignments": []}`, and `"scope": {"record": ..., "matched": N}` when a scope chose the leaders - MCP's `katana_leaders`. Written with nlohmann's `replace` handler (U+FFFD for a byte that is not UTF-8, never an exception, as `docs/cad.md` says of the other replies): a surface name typed at katana_cli in the ANSI code page ended it here, and the window with it (`docs/headless.md`, "Text and names") |
| `DIMSTYLE LIST \| INFO name \| NEW name [field value ...] \| SET name field value [field value ...] \| DELETE name` | `dimension style site updated`; `INFO`: `name=site text=3.5 gap=0.625 ... paper=on layers=1 reads=10.000` |
| `LEADER VALUES id \| #id[.point\|@x,y] \| <scope>` | `target=12 type=Point` and a `value=rl text=12.500` line each; over a scope `scope=layers layers=notes sublayers=yes matched=4 skipped=1`, then a `leader=26 target=12 type=Point` block for each leader on an entity |
| `LEADER LIST [id... \| <scope>] [target=id]`, `SET id... \| <scope> tip= at= ...`, `ATTACH id p`, `DETACH`, `FREEZE` (ids or a scope) | `id=26 layer=0 vertices=2 tip=10,10 target=12 anchor=position smart=yes template=... arrow=ClosedFilled callout=none text=...` |
| `LEADER PROP id \| <scope> SET key value [type] \| DELETE key` | `leader=26 target=12 property=invert text="IL 10.500"`; over a scope its record adds `entities=2 skipped=1` |
| `LEADER FOR id... \| <scope>`, `BALLOON FOR` | `created leaders=4 ids=7,8,9,10 skipped=1` (led by `scope=... matched=N` over a scope) |
| `LEADER ALIGN id... \| <scope> [x= spacing=mm]`, `BALLOON RENUMBER [<scope>] [start= order=id\|x\|y]` | `aligned leaders=3 x=22 spacing=4`, `balloons=3 renumbered=3` |

`TS`, `LS`, `MT` and `LE` are aliases. `LABEL` of many entities, `AUTOLABEL
RUN` and `DIM BASELINE` are each ONE undo step (`annotation_verbs::many_labels_are_one_step`). A
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
(`formatLabelStyles`, `labelStyleManagerDialog`), Format > **Dimension
Styles...** (`formatDimensionStyles`, `dimensionStyleManagerDialog`), and the
annotation scale box on the Format toolbar (`annotationScaleCombo`), all built by
`AnnotationWorkbench` (`rust/crates/katana_window/src/annotation/annotation_workbench.rs`)
so `MainWindow` gains one member. The managers
(`rust/crates/katana_window/src/annotation/annotation_managers.rs`, which lists every
field's control name) are a list and a form: Apply stores the form - one undo
step, nothing when the form is unchanged - and a refusal is shown in the
dialog's problem line in the verb's own words. The label manager's template
field is checked as it is typed, and its second tab holds the rules. The
dialogs keep no copy of the tables: an undo or a verb typed while one is
open shows at once. The Annotate menu's tools are in `docs/tools.md`; the
painting is in `docs/plan_view.md`, "Annotation".

The Dimension Styles manager
(`rust/crates/katana_window/src/annotation/dimension_style_manager.rs`, which lists every
control name) was the first of them to change nothing itself: each button
builds the `DIMSTYLE` line a person would type and runs it through the
window's one executor (`docs/desktop.md`, "One executor: the command
runner"), so it is echoed in the command log, undone as a typed line is, and
refused in the verb's own words in the dialog's problem line. The list shows
each style and how many layers name it; New and Duplicate take their name
from a box beside them (blank: the fresh name its placeholder shows), so no
question is asked in a box of its own and a headless run can name one. The
form is grouped Text, Lines and arrows, Units, and Paper-sized; a size is in
model units or, paper-sized, in millimetres on paper, as the line under the
form says. Apply sends only the fields the form changed, as one `DIMSTYLE
SET`, and nothing when none did; Duplicate sends one `DIMSTYLE NEW name
field value ...` of what the form shows; Delete is refused while a layer
names the style, naming the layer; Revert drops the edits. Under the form, a
dimension of ten reads as the formatter `DIMSTYLE LIST` uses says, and a
sample is drawn by the plan painter (`paint_plan_geometry`) at the drawing's
annotation scale - the arrowhead choices' pictures too - so what is shown is
what the plan view draws. The numbers are typed, not spun: a spin box of two
places showed `Standard`'s 0.625 gap as 0.63, and Apply would have stored it.
A form's unapplied edits survive a change that leaves their style alone (a
layer made elsewhere) and give way to one that changes it.

Since 2026-09-26 the Text Style and Label Style managers change nothing
themselves either: every button builds the verb line and runs it through
the one executor. **New** takes its name from the box beside it
(`textStyleNewName`, `labelStyleNewName`; blank gives the fresh name its
placeholder shows) and, for a label style, its kind from `labelStyleNewKind`
- `TEXTSTYLE NEW "name"`, `LABELSTYLE NEW "name" kind=area` - where it used
to make "Text Style 2" and leave the renaming to nobody, since no verb
renames a style. **Apply** sends `TEXTSTYLE SET` or `LABELSTYLE SET` with the
keys the form changed, one step, and nothing when none did; **Add Standard
Set** is `LABELSTYLE DEFAULTS`. Beside the template, **Insert Value**
(`labelTemplateInsert`) lists what a template of the style's kind may say -
`LABELSTYLE VALUES` - each with the steps that apply to it, and puts
`{value:step}` at the cursor (`{prop.NAME}` with `NAME` selected to type
over). What it offers is what `check_label_template` accepts, asked value by
value and step by step, so it cannot offer a field the template would
refuse; each choice is an action named `labelValue:VALUE[:STEP]`, which a
headless run can trigger.

The rules tab's form has the rule's **Type** (`labelRuleType`: Any, an
entity type or Alignment, or any glob) and **Enabled** (`labelRuleEnabled`);
a row chosen in the table fills the form, **Update Rule** stores the form
over the rule of its name with `AUTOLABEL RULE SET` (every field, so a
filter emptied is cleared), and the table's Enabled box switches a rule with
a line of its own, `AUTOLABEL RULE SET name enabled=off`. The table takes
several rows: **Run Rules**, **Preview** and **Remove Rule Labels** act on
the rules chosen - `AUTOLABEL RUN lots sides`, `PREVIEW lots`, `CLEAR
sides` - and on every enabled rule when none is chosen, which was all they
could do before; a rule switched off labels nothing even when chosen
(`rule_matches`). Preview changes nothing and says what a run would. The
report under the table is the verb's first reply line word for word, and
the table's **Labels** column the `rule=NAME labels=N` lines after it.

**Labels by hand** (2026-09-26). Until then a label could come only from an
auto-label rule or a typed `LABEL`; nothing in the window made, edited or
reported one. Now:

* **Annotate > Label Objects** (`annotate.label`; `LABELOBJECTS`, `LBL`, and
  a bare `LABEL`, as a bare `LEADER` starts the Leader tool) labels what is
  selected, or what is picked then Enter, with Style, Part and Text options,
  then a click for the text's place or Enter to leave it to the placer. It
  makes its labels through `katana_cad::annotation::auto_label::create_labels`, the door the `LABEL`
  verb now goes through too, so the tool and `LABEL id... style= part= at=
  text=` make the same entities as one undo step (`annotate_label_tool.rs`
  compares the two). The style offered is the one the drawing's newest
  hand-placed label wears when it fits, else the first by name that can
  label the object: remembered by the drawing, as the Text tool's height is,
  not by the program. Several objects share no one place, so with more than
  one only Enter places them. The selection is cleared once they are made, so
  the tool, starting again, asks for the next objects instead of labelling
  the same ones twice.
* **Annotate > Edit Label...** (`annotateEditLabel`, the dialog
  `labelEditDialog`; `rust/crates/katana_window/src/annotation/label_edit_dialog.rs` lists
  every field) opens on the selected label: its style, its own text (ticked,
  or `text=none`), where its text is pinned (ticked, or `at=none` for the
  placer) and its layer. OK and Apply build the `LABEL SET` line naming only
  what changed - shown as it will run - and run it through the window's one
  executor (`docs/desktop.md`), so it is echoed, kept and one undo step. With
  no label selected it says so and runs nothing; it asks nothing in a box.
  A text the command line cannot write is refused in the dialog, naming the
  field. That is a double quote, or a backslash before an n, which
  `LABEL SET` would read back as a line break. So is the bare word `none`,
  which `LABEL SET` reads as "no own text". The words are written as every
  annotation dialog writes them (`command_word`, `annotation_text_word`).
  Non-modal and kept, it follows the drawing (`LabelEditDialog::follow`), as
  Edit Text does: the label changed from elsewhere - an undo, a typed
  `LABEL SET` - is shown afresh, since Apply names only what differs from the
  label as loaded; erased, or the drawing replaced, the form is emptied and
  disabled. It once kept what it loaded, and Apply across an `OPEN` wrote
  the last drawing's edits onto the label the opened project had at that id
  (`label_edit_dialog::a_new_drawing_empties_it_rather_than_editing_another_drawings_label`).
* **The Properties panel** shows a label's resolved **Text** and its
  **Position** (`pinned at E, N` or `automatic`) beside its style, target,
  part and rule. The text is `katana_cad::annotation::label_layout::shown_label_text`, which `LABEL LIST`
  now replies with too, so the panel and the verb cannot disagree.
* **Annotate > Label Layout Report...** (`annotateLabelLayout`,
  `labelLayoutDialog`; `label_layout_report.rs`) runs `LABEL LAYOUT` - with
  `collisions=off` when its box is unticked - through the same executor when
  it opens and on Run, and shows the placed, displaced, suppressed and
  orphaned counts read back from the reply. **Select Suppressed** runs
  `SELECT id...` for the labels with a piece that found no room. For that
  the verb now names each such piece in a record of its own,
  `label=7 piece=0 suppressed=yes`, after the placed ones
  (`LabelLayout::suppressed_pieces`, `annotation_verbs::label_layout_names_each_piece_that_found_no_room`).
  The dialog works nothing out itself: an agent reads the same records.

A double click on a label in the plan view opens Edit Label on it
(`MainWindow::edit_double_clicked` triggers `annotateEditLabel`, as it
triggers `annotateEditText` for a text; `docs/desktop.md`, "Double click to
edit").

**Leaders.** Under the Annotate menu's tools, three entries open the Leaders
manager (`LeaderManagerDialog`, `rust/crates/katana_window/src/annotation/leader_manager.rs`,
the dialog `leaderManagerDialog`) on one of its tabs: **Leaders...**
(`annotateLeaders`), **Leaders for Selection...**
(`annotateLeadersForSelection`) and **Arrange Leaders and Balloons...**
(`annotateArrangeLeaders`), each given a menu letter the tools have not taken
(`AnnotationWorkbench::add_leader_actions`). It is the smart leaders' front end
and changes NOTHING itself: every button builds the `LEADER` or `BALLOON` line a
person would type - shown as it ran in the read-only `leaderLine` - and hands
it to the window's one executor (`docs/desktop.md`, "One executor: the command
runner"), so the edit is echoed in the command log, kept in the history, ONE
undo step like a typed line, and a refusal is said in the dialog's problem
line in the verb's own words. It still calls the read-only helpers of
`leader_edit.rs` (what a note would say, the place a leader goes on an
entity, the value rows) to preview; none of them edits. The lines are built by
pure functions of plain data (`leader_option_words`, `leader_set_line`,
`leaders_for_line`), so a test reads the very line:

| Button | Line |
|---|---|
| Apply | `LEADER SET id [id...]` and only the options the form changed - `text=` / `template=` / `labelstyle=`, `arrow=`, `callout=`, `style=`, `paper=`, `arrowsize=`, `landing=`, `landingangle=`, `tip=#id.alongN:T`, `vertex=N at=x,y` - the numbers written exactly (`katana_core::text::format_exact_real`: a height of 2.3456 is not sent as 2.346). An untouched form runs no line |
| Freeze Note, Detach Tip | `LEADER FREEZE id...`, `LEADER DETACH id...` |
| Attach to Selected | `LEADER ATTACH id #id.along0:0.25` (`command_words.rs`, `anchor_word`, the place written so the verb reads it back exactly) |
| Set, Remove (attribute) | `LEADER PROP id SET key value [type]`, `LEADER PROP id DELETE key` |
| Make for Selection | `LEADER FOR <options> angle=45 length=10 <scope>`, `BALLOON FOR ...` |
| Align Selected Leaders | `LEADER ALIGN [x=] [spacing=] <scope>` |
| Renumber Balloons | `BALLOON RENUMBER start= order=` and, ticked, the scope |

A note the line cannot carry is refused in the problem line, naming the field,
and nothing runs: a double quote (`the note: a double quote cannot be written
on a command line`), in a note, a text style, a label style or an attribute.
A line break is written as the two characters the verbs read back as one, and
a backslash twice (`katana_core::text::unescape_typed`), so a note holding `C:
ew` goes
through whole - which the other annotation dialogs, using
`annotation_text_word`, refuse. Rejected: sending the single leader the form
shows as `LEADER SET ... ` with every field, which would rewrite a value the
widgets cannot hold exactly.

The **Which leaders...** button above the list, **For Selection** and
**Arrange** each carry the shared Apply to / Only those that match controls
(`rust/crates/katana_window/src/customisation/scope_filter_widget.rs`, the very widget of Global Modify and
the utilities dialog; control names `leaderList...`, `leaderFor...` and
`leaderArrange...`, `Scope`, `Layers`, `FilterLayer`, `TypePoint` and so on).
On the Leader tab pressing the button shows the controls and the list then
holds the leaders the scope takes (`katana_cad::global_modify::match_entities`, the matcher the verbs
resolve a scope with), the whole drawing and no filter to begin with, so
unpressed and pressed-with-defaults list the same; a scope that cannot be read
(no layer ticked) says why under the button and lists none. Apply, Freeze and
Detach act on the shown leader or the selected ones, as before: they name ids,
which stay the other way to name leaders. On For Selection the controls choose
the entities the leaders are made for - the selected ones to begin with, and
the tab then says "2 entities selected" as it did - and the line ends in the
scope's words; the scope's own record (`scope=layers layers=plan sublayers=yes
matched=2`) is shown beside the report (`leaderForScopeReport`). On Arrange
they choose the leaders to line up, and **Only the balloons Apply to takes**
(`balloonRenumberScoped`, unticked to begin with) the balloons to number
again; unticked, every numbered balloon is, as it always was. The tab is still
named For Selection and its menu item "Leaders for Selection..."
(`annotateLeadersForSelection`): the selection is what it begins with, and the
names are what the headless tests and shortcuts know it by.

* **Leader**: every leader in the drawing, by what it says, and a form for the
  chosen one, which follows the drawing's selection. The form says what the
  tip is on in words ("Polyline 1, 50.0% along segment 2") and, for a tip
  along a line, an arc, a circle or a polyline segment, how far along in per
  cent, which Apply moves it by; **Attach to Selected** puts the tip of a
  plain leader, or of one on something else, on the first entity selected with
  it that offers a place (not a dimension or a label), at the place of it
  nearest the tip, and is no step where the tip is already (`LEADER ATTACH`,
  `leader_manager::attach_to_selected_puts_the_tip_on_the_entity_and_along_moves_it`). The note is Text, a
  Template read off that entity, or a Label style's template, checked as it is
  typed and shown as it would read ("Says"); a style's template is shown to
  read, and read again when the style is edited
  (`leader_manager::a_styles_note_is_read_afresh_when_the_style_changes`). Beside it are the values the
  entity offers at the tip: double-clicking one puts `{name}` into the note
  where the cursor is - the end of the note until the user moves it - and
  makes the note a template. Below: the arrow, callout, text style and sizes;
  an attribute of the entity set or removed through the leader, of the type
  chosen or as typed (`LEADER PROP`, which reads a value as `PROP SET` does -
  `CommandInterpreter::property_value`); and Freeze Note, Detach Tip and Apply.
  Choosing a label style lends the form that style's text style and height, as
  `labelstyle=` lends a leader them; choosing another style puts back what the
  first lent before lending its own, and so does a style appearing in or going
  from the drawing under the form
  (`leader_manager::a_style_that_comes_or_goes_under_the_form_lends_its_look`); choosing Text or Template
  again puts back the words typed, and the look from before in each field
  still showing what the style lent - not in one the user has changed since
  (`leader_manager::a_note_made_a_styles_and_back_keeps_the_users_words_and_look`,
  `leader_manager::a_look_typed_after_a_lend_is_kept_and_a_styles_height_is_lent_exactly`).

  Apply sends only what the user changed: the form remembers what it showed
  and leaves out every field still showing that, so a value its widgets cannot
  hold exactly - a height finer than their 3 decimals, a landing past their
  1000 mm, a non-breaking space - is never rewritten, and an untouched form
  applied is no step
  (`leader_manager::an_untouched_form_is_no_step_and_what_its_widgets_cannot_hold_is_kept`). A note made a
  label style's is the one exception, since the style lends its look to
  whatever the edit leaves out: the text style and height the form shows are
  sent unless they are the style's own, which is then lent whole, finer than
  the form's decimals - so a look set back to the leader's own over the
  style's is kept
  (`leader_manager::a_look_set_back_over_a_styles_is_kept_and_another_style_lends_over_the_one_before`). With
  the shown leader one of several selected, Apply, Freeze and Detach change
  them all, in one step, and the form says so
  (`leader_manager::several_selected_leaders_are_changed_together_in_one_step`) - all but Along, which
  is where the shown leader's tip is on its own entity: changed with others
  selected, Apply refuses it rather than move every tip to that one place.
  Freeze, Detach and Attach act on the leader as drawn, so while the form has
  changes not applied they are greyed, their tips say why, and they refuse
  (`leader_manager::the_list_chooses_and_freeze_waits_for_the_form_to_be_applied`). The form is read
  again from the leader when the leader changes under it - an undo, a verb
  typed - and left alone, unapplied edits and all, when something else changes
  (`leader_manager::the_form_follows_the_selection_and_an_undo_but_keeps_unapplied_edits`); a drawing
  opened or started afresh starts the form afresh, even where the new drawing
  has the very same leader (`leader_manager::a_drawing_replaced_starts_the_form_afresh`). It watches
  the document through `DocumentWatcher`, as `docs/desktop.md` asks.
* **For Selection**: a leader, or with Balloons ticked a balloon, to each
  selected entity (`LEADER FOR`, `BALLOON FOR`): the note as text, a template
  or a label style's, checked and shown as it would read for the first
  selected entity that has a place for a leader, with the values that entity
  offers there to double-click into it; the look (arrow and callout, each
  Automatic by default - a dot for a tip inside an outline, a circle for a
  balloon - text style, height, arrow size, landing), a label style's shown as
  it lends it, as on the Leader tab, and what the boxes show being what the
  leaders get (`leader_manager::for_selection_shows_the_look_a_style_lends_and_makes_what_it_shows`); and
  the note's direction and distance on paper. A balloon with no note is
  numbered on from the highest there is, and must stay a circle
  (`leader_manager::a_numbered_balloon_must_stay_a_circle`). It says how many it made and why it
  skipped any, and what it made is selected, so the Leader tab shows it and
  Arrange can line it up
  (`leader_manager::for_selection_is_previewed_for_the_first_and_placed_as_told`).
* **Arrange**: the selected leaders' notes lined up at the topmost's x or a
  given one, stacked or not (`LEADER ALIGN`), and the balloons numbered again
  from a number, as made, across or down (`BALLOON RENUMBER`); numbering
  that changes nothing is no step
  (`leader_manager::arrange_aligns_the_selected_notes_and_renumbers_the_balloons`).

**Preset leaders** (2026-10-15). Six tools of the catalogue's Annotate
category, group Leaders (`rust/crates/katana_cad/src/tools/annotate_leader_presets.rs`):
Level Leader (`annotate.leader.level`, `LEADERLEVEL`), Surface Difference
Leader (`annotate.leader.surface`, `LEADERSURFACE`), Cut/Fill Leader
(`annotate.leader.cutfill`, `LEADERCUTFILL`), Point Name Leader
(`annotate.leader.name`, `LEADERNAME`), Code Leader (`annotate.leader.code`,
`LEADERCODE`) and Station Leader (`annotate.leader.station`,
`LEADERSTATION`) - so each is a menu item on Annotate and a button of its own
on the Annotate toolbar, with a painted icon (`rust/crates/katana_window/src/tools/icons_annotate.rs`:
the point a vertex handle, the leader the accent, and at its landing a level
mark, a ground line with the difference dashed and a plus-or-minus, cut and
fill arrows, a name tag, a code's bars, an alignment's chainage ticks). Click
the point (or snap to an entity's end, middle or vertex), then where the note
goes, or Enter for where `LEADER FOR` puts one; `S` (`A` for the station)
chooses the surface (the alignment) by name. Each makes, through the same
door, exactly the leader `LEADER #id@x,y x,y preset=...` makes, as one step,
and starts again for the next point: a row of spot levels is two clicks each
(`leader_preset_tools::the_level_leader_tool_makes_what_the_leader_line_makes`). A point
with nothing to say - no level, off the surface - is refused at the first
click with why, before the note's place is asked. The surface offered is the
one the drawing's newest leader of that preset reads, remembered by the
drawing as Label Objects remembers a style. Not a "Leader, ..." family: a
family is ONE toolbar button with the rest behind a drop-down, and the owner
asked for each in one click. Their names leave the Annotate menu's letters
room: each takes one of its own and Text keeps T - and Edit Text now takes X
only while no tool has it, since a tool short of letters is moved onto any
letter of its name and Text was one rename from being given X
(`AnnotationWorkbench::add_edit_text_action`). The tool's preview is painted with
the drawing it reads - the model and the surfaces and alignments, through
`paint_plan_geometry`'s `source` (the window gives it for a leader shape only) -
so while the person chooses where the note goes the preview already says "RL
30.250", and so does a smart leader's grip drag; painted with none it was a
bare arrow and landing, and the note appeared only once the leader was made
(`leader_preview::a_smart_leader_is_painted_with_its_note_read_off_the_drawing_its_tip_is_on`).

The Leaders manager's **Preset** boxes (`leaderPreset` and
`leaderPresetReads` on the Leader tab, `leaderForPreset` and
`leaderForPresetReads` on For Selection) put a preset's template into the
note - its surface or alignment chosen beside it, or the first visible one,
said in the problem line - which Apply or Make then stores, one step each,
the same template `LEADER preset=` makes; a leader shown says which preset it
is and what it reads, and a note typed away from the template is no preset
(`leader_manager_presets::*`). Its values list the drawing's surfaces' and
alignments' values at the tip, and the property panel shows a preset's
**Preset** row ("Surface Difference (ground)").

## Stored with the project

Migration 11 (`rust/crates/katana_storage/src/project_store.rs`, `docs/model.md`) adds
`text_styles` (a column a field, a fixed set a DXF STYLE row also has),
`label_styles` (the name and kind as columns and the rest as versioned JSON,
`label_style_definition_to_json`, so a later placement option needs no
migration; a definition from a newer build is refused as `Unsupported`
rather than half read), `label_rules`, and a dimension style's
`paper_sized`, defaulting to model units. `Standard` is seeded by the model
and not stored until changed. A project from before opens exactly as it was
(`annotation_storage::a_project_from_before_schema11_opens_with_none_and_model_dimensions`), and every
table and kind survives saving twice and reopening
(`annotation_storage::every_table_and_kind_survives_saving_twice_and_reopening`).

The geometry blob (`rust/crates/katana_entity/src/geometry_blob.rs`) has a second
version, `BLOB_VERSION_ANNOTATION`, written ONLY for what needs it - a label,
a leader, a text with a style, paper height or justification, a dimension of
another kind or with references (`fits_version_one`). Everything else is
written in version 1, byte for byte as before, so the pinned wire-format
test still holds and an older build still reads every drawing that uses
none of this; one that does is refused per entity as an unknown version,
loudly.

Smart leaders added a third, `BLOB_VERSION_SMART_LEADER`, on the same rule
(`fits_version_two`): written only for a dimension or a leader with an anchor
Along or Inside - every anchor then carries its fraction - and for a smart
leader, whose `fields` and `labelStyle` follow its version-2 payload. A plain
leader and every other annotation is still version 2, byte for byte
(`smart_leaders::a_version_three_blob_holds_them_and_nothing_else_needs_one`), so no schema migration
was needed, and a build from before refuses exactly the entities it could
not read correctly (`annotation_storage::smart_leaders_and_anchors_along_survive_saving_twice_and_reopening`).

## Exchange

DXF (`rust/crates/katana_dxf/src/writer.rs`) has no label entity, and the writer
draws only an aligned dimension and a bare leader itself; it may not see
`katana_cad`, where the placer and the builders are
(`katana_dxf`'s `Cargo.toml` does not depend on it). So a front end draws the rest -
`draw_annotation_for_export` (`rust/crates/katana_cad/src/annotation/export_annotation.rs`):
labels placed as the plan view places them at the drawing's annotation
scale, every dimension kind but aligned, and leaders with their arrowheads
and callout frames - as lines, closed polylines and single-line texts, and
hands them to the writer (`ExportOptions::drawn`), which writes each on its
entity's layer and in its colour. The window's DXF export and `katana_cli`'s
`EXPORT` both do it, with the document's annotation scale, so both write the
same file; paper-sized text is written at that scale's height. A smart
leader's note reads the drawing's surfaces and alignments there as on screen:
both hand `draw_annotation_for_export` the document's `LeaderContext` (the
background `EXPORT` job holds the copy it was made with). Written
without them (a caller that passes nothing), labels and the other dimension
kinds are skipped with a warning and a leader is its line, landing and note -
a smart leader's note as it reads (`leaderNote`), without `{code}`, which
survey coding decides and the writer cannot see, and without a line reading a
surface or an alignment, which the writer cannot see either: that line is
dropped, never written as nothing.
A label with no room at the scale is left out and said so. A mask, a width
factor and a slant do not reach the file.

A scoped export (`EXPORT file.dxf LAYERS notes`, a selection, an area, a
filter - the window's Export Drawing dialog builds the same line) draws the
annotation from the WHOLE drawing and only then writes the entities the scope
took (`write_dxf_export` with the scope, `rust/crates/katana_app/src/dxf_verbs.rs`): a
smart leader reads the entity its tip is on, a label the entity it labels, and
the writer was handed a copy of the drawing with everything the scope left out
already removed - a scope that took the leader and not its point wrote a bare
arrow, the note missing, `skipped=0` and no word, and every preset leader is
associative, so it was most of them
(`export_options::a_scoped_dxf_export_keeps_the_note_of_a_leader_whose_target_the_scope_left_out`).
A smart leader whose note comes out empty even so - the entity its tip is on
is not in the drawing at all, or has none of the values its template names -
is written as the arrow it is and the export says which, a warning record:
`leader #12 is written without its note: ...`
(`export_options::a_leader_whose_note_cannot_be_read_is_written_and_the_export_says_it_has_no_note`). A
12d archive has no drawn annotation (it counts leaders and labels as
annotation not exported), so its scope still cuts the copy first.

DXF import is unchanged: a file's texts and dimensions come in as texts and
aligned dimensions. The GIS vector export skips annotation with a count, as
it skipped dimensions, and so does the archive exporter.

## Tests

| File | What |
|---|---|
| `rust/crates/katana_entity/tests/label_text.rs` | the template language: every step, DMS carry, chainage, absent lines dropped, errors named |
| `rust/crates/katana_entity/tests/annotation_model.rs` | the tables' policies, text blocks, label values of every kind, anchors, the new kinds' validation, bounds and transforms |
| `rust/crates/katana_commands/tests/annotation_tables.rs` | the table commands, a style in use refused with its user, the change-set guards |
| `rust/crates/katana_storage/tests/annotation_storage.rs` | migration 11 and saving twice |
| `rust/crates/katana_cad/tests/leaf/text_layout.rs` | paper height at several scales, justification, readability, masks |
| `rust/crates/katana_cad/tests/annotation/label_layout.rs` | the placer: collisions, scale, suppression, displacement with leaders, order independence, priority, keep-out of lines and of annotation, area labels, dragged labels, chainage ticks |
| `rust/crates/katana_cad/tests/custedit/associative.rs` | following moves, stretches, radii and intersections, erasing, locked layers; one undo step each |
| `rust/crates/katana_cad/tests/annotation/auto_label.rs` | rule matching, one step, re-runs, clearing, chainage rules, the default styles |
| `rust/crates/katana_cad/tests/annotation/dimensions_and_leaders.rs` | every kind built and drawn, paper-sized styles, chains, leaders and callouts |
| `rust/crates/katana_cad/tests/annotation_drawing_verbs/annotation_verbs.rs` | every verb and its reply, undo as one step |
| `rust/crates/katana_cad/tests/annotation_drawing_verbs/dimstyle_set_many.rs` | `DIMSTYLE SET` and `NEW` with several pairs as one step, a bad pair refusing the line, `INFO`'s record, the changes between two styles reading back as the second, the words a dialog writes |
| `rust/crates/katana_cad/tests/annotation/export_annotation.rs` | what a file is handed |
| `rust/crates/katana_cad/tests/annotools/annotate.rs` | the Leader tool's one entity, the Angular, Radius, Diameter and Ordinate Dimension tools |
| `rust/crates/katana_cad/tests/tool_cases/annotate_text_options.rs` | the Text tool's Style, Justify and Paper options and Multiline Text, each checked against what `TEXT` and `MTEXT` make |
| `rust/crates/katana_cad/tests/tool_cases/annotate_label_tool.rs` | the Label Objects tool against the `LABEL` line, its style offered, options, undo and refusals |
| `rust/crates/katana_cad/tests/tool_cases/annotate_dimension_chain.rs` | the Baseline and Continue Dimension tools against `DIM BASELINE` and `DIM CONTINUE`: the newest base, Select, Spacing, Enter and Esc, a snapped origin followed |
| `rust/crates/katana_cad/tests/tool_cases/annotate_leader_options.rs` | `snap_anchor`; the Leader's options and the Balloon tool against `LEADER` and `BALLOON`; Linear's Rotated and a line between the levels against `DIM LINEAR`; the Ordinate's Datum against `DIM ORDINATE datum=`; snapped points followed |
| `qt_the_chain_balloon_and_leader_options_answer_on_the_command_line_headless` | the window: `DBA`, `BALLOON` and `LE`'s Callout answered on the command line |
| `rust/crates/katana_window/tests/annotation/label_edit_dialog.rs` | the `LABEL SET` line Edit Label writes, the dialog and the Label Layout Report driven by control name, each line run as the window runs it |
| `qt_labels_are_made_edited_and_reported_in_the_window_headless` | the window end to end: `LBL` answered on the command line, the Properties rows, Edit Label's line through the one executor, the layout report, the Annotate menu |
| `cli.label_layout_names_the_label_that_found_no_room` | `katana_cli`: the suppressed record, and a pinned label placed |
| `rust/crates/katana_entity/tests/leader_values.rs` | Along and Inside anchors, the nearest place, what a leader can say of every kind of target, the leader template check, validation, the version-3 blob and JSON; the degenerate cases - a fraction that is no number, a repeated vertex, a figure of no area, no place at all - and a national grid's magnitudes |
| `rust/crates/katana_cad/tests/annotation_drawing_verbs/smart_leaders.rs` | the LEADER verbs end to end: templates, label styles, `#id@x,y`, `.inside` and `.alongN:T`, a backslash typed, VALUES, LIST, SET, ATTACH, DETACH, FREEZE, PROP, FOR, ALIGN, BALLOON FOR and RENUMBER; following stretches and moves, going with the target, copies |
| `rust/crates/katana_cad/tests/drawtools/modify_transform.rs` | (`copying_tools::a_callout_copied_with_its_pit_reads_the_pits_copy`) the Copy, Rotate and Scale with Copy, and polar Array tools copy a callout as one of its pit's copy |
| the `cli.` tests' CMake list | `cli.a_smart_leader_is_saved_with_the_point_it_reads` and the tests after it: the verbs through `katana_cli` - saved, opened by another process and read afresh, an exact along, balloons numbered again, a refusal stopping and failing the script |
| `rust/crates/katana_cad/tests/annotools/smart_leader_tool.rs` | the Leader tool: a tip put on what it is clicked on, fields checked as typed, a note that would say nothing refused |
| `rust/crates/katana_cad/tests/annotation/leader_edit.rs` | the shared edits: an unchanged edit is no command, what cannot be made is refused rather than skipped (negative sizes too), a label style's look lent, attaching (and again, no command), the value rows, where a leader goes on an entity (and a tip put on one, inside a lot), LEADER FOR's and ALIGN's refusals, a numbered balloon kept a circle and the largest number refusing a next |
| `rust/crates/katana_entity/tests/leader_context_values.rs` | what a leader reads of the drawing through a fake context of planes and a straight alignment: a surface's level and the difference, cut and fill (one line, on the surface a fill of nothing), a name with a blank, station, offset, profile and its difference, every refusal's reason, no context, every value listed, the template check of each kind, `+.Nf` |
| `rust/crates/katana_cad/tests/annotation_drawing_verbs/leader_presets.rs` | the presets on a Document with a real TIN: level, surface (the first visible chosen and said, a name in any case, a hidden one named), the note following a surface edit and its undo, cut/fill for many in one step, station with its alignment and profile, every refusal and nothing made, `LEADER PRESETS`, `LIST JSON`, `VALUES`, recognising each preset, a freeze, `LEADER SET preset=`, the context a copy, LIST and the export |
| `rust/crates/katana_cad/tests/tool_cases/leader_preset_tools.rs` | the six tools in the catalogue; the Level Leader against `LEADER #1@30,40 40,50 preset=level`; the surface chosen with `S`, and offered from the newest leader; a point with nothing to say refused at the first click; Enter's place; a row of levels; name, code and station |
| `rust/crates/katana_window/tests/annotation/leader_manager_presets.rs` | the Leaders manager's Preset boxes by control name: a preset's template written and applied in one step, a named surface and refusals, a leader shown as its preset until typed away, For Selection, the surfaces' values |
| `rust/crates/katana_geometry/tests/cases/alignment.rs` | (`StationOffset...`) the station and offset of a point on a straight alignment, a curve, outside a kink, beyond the ends, and undoing `point_at_station_offset` on spirals |
| the cli tests' `leaders.cmake` | `cli.a_surface_leader_says_how_far_a_point_is_above_the_ground`: a surface from the sample terrain, a point at RL 30, surface, cut/fill and level leaders and LIST; one on no surface failing the script |
| `rust/crates/katana_app/tests/mcp/mcp_server.rs` | `mcp_server::the_leaders_tool_gives_what_each_leader_says_and_the_presets`, and `mcp_server::the_leaders_tool_takes_a_scope_and_filter_and_says_how_many_leaders_it_took`: a layer, an area given alone, a filter, a scope that takes none giving none, a filter alone on the selection, and the refusals (ids with a scope, a layer that is not there, VIEW, an area that is not four numbers) |
| `rust/crates/katana_cad/tests/annotation_drawing_verbs/leader_scope.rs` | the leader verbs over the shared scope and filter, each value worked out by hand: LIST (a layer, an area, a filter, the selection, the window's view with its hidden layers), VALUES, SET, FREEZE, DETACH, PROP (two leaders on one pit set it once), ALIGN, FOR and BALLOON FOR and RENUMBER; a scope that takes nothing is said and never means "all"; the refusals (ids with a scope, two scopes, a missing layer, VIEW headless, an edit of an empty selection); a WHERE key after WHERE is the condition; `SURFACE RENAME` rewriting the notes that read the surface in its one undo step (any case, steps and doubled braces kept, a locked layer or an unwritable name left and warned), `ALIGN DELETE`'s warning, `rename_leader_reads` for an alignment, `rename_template_fields`; the anchor words the dialog writes read back as the same place; and the JSON writer handed a byte that is not UTF-8 |
| the `cli.` tests' CMake list | `cli.leaders_take_a_scope_and_say_how_many_they_took`, `cli.leaders_named_by_id_and_by_scope_are_refused`, `cli.balloons_of_one_layer_are_numbered_again_by_scope`; the cli tests' `leaders.cmake` adds `cli.renaming_a_surface_moves_its_leaders_and_removing_it_warns` |
| `rust/crates/katana_window/tests/annotation/leader_manager.rs` | the Leaders manager driven by control name and by its widgets' own edits: the form, the template checked and previewed, only what changed applied (values the widgets cannot hold kept), the note kind switched and back, a style's template read afresh, values double-clicked in at the cursor, a label style's look lent, set back, kept when typed over and lent exactly, and lent anew when the style comes or goes, attributes set, attach (past a dimension, waiting for Apply, again no step) and along, several leaders at once, freeze and detach waiting for Apply, following the selection, undo and a replaced drawing, For Selection's preview for the first with a place, its look and a style's, and placing, numbered balloons kept circles, Arrange, each menu entry's tab |
| `rust/crates/katana_window/tests/annotation/leader_manager_lines.rs` | the Leaders manager hands lines to the executor and changes nothing itself: the options of a change written exactly (2.3456 not cut), a note's line break and backslash, a double quote refused with a sentence naming the field and nothing run, an untouched form running no line, no executor said, each button's line (SET of only what changed, FREEZE, DETACH, ATTACH along, PROP, FOR, BALLOON FOR, ALIGN, RENUMBER), the list and the For Selection and Arrange tabs taking a layer, a filter or the whole drawing from the shared controls, and the reply's report |
| `rust/crates/katana_dxf/tests/writer.rs` | paper-sized text at the scale, drawn annotation, labels with no room |
| `rust/crates/katana_window/tests/plan_painting/leader_preview.rs` | a smart leader painted as a preview or a grip drag with the drawing it reads says its note, a plain one and one whose target is gone are as they were |
| `rust/crates/katana_window/tests/annotation/annotation_ui.rs` | painting (a white style prints black, masks, paper height at every scale, the painter's label counts) and the managers and scale box driven by control name: named New, Apply of the changed keys, Insert Value, rules added, updated and switched, and Run, Preview and Clear of the chosen rules |
| `qt_the_label_style_manager_runs_chosen_rules_headless` (`tests/CMakeLists.txt`) | the real window: a label style named with its kind, a rule with its type previewed and run as the one chosen |
| `rust/crates/katana_window/tests/annotation/text_edit_dialog.rs` | Edit Text by control name: the selection followed, one `TEXTEDIT` of the changed keys, a value no line can carry refused |
| `qt_multiline_text_and_edit_text_run_in_the_window_headless` (`tests/CMakeLists.txt`) | the real window: `MT` typed, two lines made one text; Edit Text on a selected text |
| `rust/crates/katana_window/tests/annotation/dimension_style_manager.rs` | the Dimension Styles manager by control name: every field in one line and one step, New, Duplicate, a refused Delete, the preview and sample, Revert |
| `qt_the_dimension_styles_manager_runs_its_lines_headless` (`tests/CMakeLists.txt`) | the real window: Format > Dimension Styles..., a style made and applied, undone by one typed UNDO |

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
  drawing can be cut off at the edge. The fix is in the plan painter's
  `plan_drawn_bounds` (and the plot's fit), which would add each note's measured
  box at the annotation scale - the painter's font metrics, not anything of
  the annotation code - so it was left to the view code's owner.
* **The Linear Dimension tool** still stores the aligned projection for an
  unsnapped horizontal or vertical dimension outside the origins' levels
  (`docs/tools.md`).
* **Edit Text works on one text.** A selection of several texts is not
  edited together; `MODIFY` changes their layers and styles, and `TEXTEDIT`
  is one text a line.
* **Labels are not picked by their text**: a label is selected at its
  anchor, and moved off its placed position with Annotate > Edit Label
  (`LABEL SET id at=x,y`) rather than by dragging its text.
* **PURGE** does not purge unused text or label styles. It wants them in
  `katana_entity::table_usage::table_usage`, `katana_commands::entity_commands::TableItems` (with the undo of a deleted
  style) and `katana_cad::purge::plan_purge` - the table commands, not the annotation code.
* **No style is renamed.** No verb or command renames a text, label or
  dimension style, so the managers offer no Rename; New names a style as it
  is made instead.
* **DXF**: no MTEXT, masks, width factors or slants on export, and nothing
  on import becomes a label, a leader or a dimension kind.
* **The 3D view** draws a label as a marker at its anchor and a leader as
  its line: its words are worked out for a scale the 3D view does not have.
* **Smart leaders** have one arrow: a multi-leader - one note, several arrows
  to several pits, "3 x SMH" - is not built. A tip on a polyline names its
  segment by number, as a Vertex anchor does, so inserting a vertex before
  it moves the tip to the next segment along. The Leader tool attaches only
  to an entity within the pick aperture; clicking inside a lot does not
  point into it (`#id.inside`, `LEADER FOR` and the Leaders manager's Attach
  to Selected do). An arc's or a circle's centre is reached only by typing
  (`#id.centre`). DXF export writes the note as it reads, not a field a CAD
  reader could update.
* **A surface or an alignment is read by name.** A surface renamed is
  followed ("What a leader reads of the drawing"); one removed leaves the
  leaders that read it drawn bare, with a warning saying how many, and an
  alignment has no rename to follow (no `ALIGN RENAME` exists; `rename_leader_reads`
  is ready for it) and no in-use guard (`docs/geometry.md`).
* **A tip on nothing reads no surface.** The ground level at a bare click -
  a leader to a place, not a point - would need a smart leader with no
  target, which the model refuses; put a point there.
* **The alignment a station preset takes** with none named is the first by
  name, not the nearest the tip: a preset is resolved once for LEADER FOR's
  many targets, and the nearest of one is not the nearest of the next.
* **The context is made for every paint**, a copy of the surface list and of
  the alignments' definitions; not measured on a drawing of hundreds of
  alignments.
* **The Annotate menu has more items than letters.** With the six preset
  tools, Edit Label..., Leaders..., Leaders for Selection... and Arrange...
  have no menu letter of their own (none is shared:
  `qt_every_shortcut_and_menu_letter_reaches_one_thing_headless`), and the
  Annotate toolbar down the left edge is six buttons longer.
* **A double quote** cannot be typed in a note, a template, a style's name
  or an attribute: the command line has no escape for one inside quotes, so
  what the window and the Leader tool can put in a note, a script cannot.
  The project-wide limit, as for sheets (`docs/plotting.md`). The Leaders
  manager says so in its problem line, naming the field, rather than send a
  line that sets something else.
