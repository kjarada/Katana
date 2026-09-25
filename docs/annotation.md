# Annotation: paper-sized text, labels, dimensions and leaders

Everything a drafter writes ON a drawing rather than draws IN it: text sized
for the sheet, text styles, labels that read their words off the geometry,
rules that label a whole drawing, the placer that keeps labels apart, the
dimension kinds, leaders and callouts, and smart leaders that read their note
off what they point at. The model's side is `katana_entity`
(`include/katana/entity/annotation.hpp`, `label_text.hpp`, `label_values.hpp`,
`leader_values.hpp`, `anchor.hpp`, `text_block.hpp`), the layout and the edits are `katana_cad`
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
position, start, end, mid, centre, vertex N, the middle of segment N, a place
ALONG it, INSIDE it). On the command line a point written `#id`, `#id.end`,
`#id.v3`, `#id.s2` or `#id.inside` names one, and `#id@x,y` names the place on
the entity nearest x,y (`nearestAnchor`); the interactive dimension tools
name the lines, arcs and circles they are given by picking, and the Leader
tool the entity its tip is clicked on.

**Along** (added with smart leaders) is a fraction of the way along: a line's
or an arc's from its start, a polyline's segment N and the fraction along it,
a circle's fraction of a turn counter-clockwise from east. It is what keeps a
tip at a quarter of the way along a pipe when the pipe is stretched to twice
the length (`ATipOnALineStaysAtItsPlaceAlongIt`). A circle has no start, so a
tip on a circle keeps its compass direction from the centre as the circle
moves or grows; rotating a circle turns nothing a tip could follow.
**Inside** is a closed polyline's inside point - the centroid, or the middle
of the widest run through it when the centroid is in a notch (`insidePoint`,
the point an area label goes at) - or a circle's centre.

A copy follows its original's references only where it should: an
annotation COPIED, MIRRORED or ARRAYED together with what it refers to
refers to the copy, so a pit copied with its callout gets a callout of its own
(`ACopyOfAPitWithItsCalloutGetsACalloutOfItsOwn`,
`CopiesOfAnnotationReferToCopiesOfWhatTheyReferTo`). Copied alone, it is
another annotation of the original. Before smart leaders a copied pair
snapped the copy's tip back onto the original pit.

Rather than teach every command about annotation, `Document::execute` wraps
EVERY command in `withAssociativeUpdate`
(`include/katana/cad/annotation/associative.hpp`): after the command runs,
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
  (`include/katana/entity/label_text.hpp`, "Templates" above) -
  `LEADER #12 25,15 template="IL {prop.invert:.3f}\n{length:.1f} m"`;
* `labelStyle`: the note is that label style's template, read live - one
  `LABELSTYLE SET` and every leader in the style says the new thing. Only the
  template: the look is the leader's own, and `LEADER` lends it the label
  style's text style and paper height when it makes it (unless `style=` or
  `paper=` says otherwise), so it looks as the style's labels do. A leader's
  note is its own template or its style's, never both, and a style a leader
  reads cannot be deleted (`ALabelStyleALeaderReadsIsNotDeletedAndMustExist`).

A plain leader's note is still exactly what was typed, braces and all.

### What a leader can say

`include/katana/entity/leader_values.hpp` is the reference, and `LEADER VALUES
#id@x,y` lists what a note at a place could say, each value in its own format.
The values are those of the entity the tip names AT THE TIP
(`anchorValues`): the whole set of names is `leaderValueNames`, and a
template naming anything else is refused as it is typed, with the name
(`checkLeaderTemplate`).

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
  up from one end (`ALineSaysItsBearingAndTheLevelAndChainageAtTheTip`); a
  tip inside a lot has a level only when every vertex is at one. A template
  line naming a value the target lacks is dropped, as a label's is.
* **The place, else the nearest.** When a reference names a place the target
  no longer has - a vertex since deleted - the place on the target nearest the
  tip stands for it, rather than the leader going blank.

A leader that would say NOTHING - every line dropped - is refused on the way
in by `LEADER` and the Leader tool, naming what the target lacks ("the leader
would say nothing: its target has no prop.invrt"), since a property the
target does not carry is far more often a typo than an intent
(`checkLeaderSaysSomething`). So is a smart leader whose tip is on nothing.

### Pointing at the right place

`#id@x,y` puts the tip on the entity nearest x,y and names the place ALONG it
("Associativity" above), so the tip stays at the same place of a line that
moves or stretches, and the chainage and level it reads are the new ones.
`#id.inside` points into a lot; a leader ending inside an outline ends in a
DOT unless `arrow=` says otherwise, one ending on it in an arrowhead
(ISO 128-22, leader lines; `InsideALotTheArrowIsADotAndTheNoteItsArea`). `LEADER
ATTACH id #id@x,y` moves a leader's tip onto an entity, `LEADER SET id
tip=...` too, and the Leader tool puts a tip clicked within the pick aperture
of a point, line, arc, circle, polyline or text on it (`docs/tools.md`).

### Changing what an entity says, through its leader

`LEADER PROP id SET invert 10.5 [real]` sets the attribute on the entity the
leader's tip is on - the same command `PROP SET` is, one undo step - and
replies with the note as it now reads; `LEADER PROP id DELETE key` removes it
(`ATemplateIsReadOffThePitAndFollowsItsAttributes`).

### Many at once

`LEADER FOR id... | SELECTION [template= | labelstyle= | text=]` makes a
leader to each entity as ONE step: its tip at a point's or a text's position,
the middle of a line or an arc, halfway along an open polyline, inside a
closed one, and on a circle on the side the note goes; the note `length=` mm
(10) from the tip at `angle=` degrees (45), at the document's annotation
scale. An entity that offers no place (a dimension, a label, a leader) or
about which the note would say nothing is skipped and counted, and the reply
says the first reason when nothing was made
(`ForMakesOneLeaderPerEntityInOneStep`). `BALLOON FOR` numbers a balloon to
each on from the highest; a smart balloon (`template=`) is no number in the
run. A numbered balloon stays a circle: only a circle's number is counted,
so `BALLOON` or `BALLOON FOR` with no note and `callout=box` or `none` is
refused rather than make a number that would be given again
(`ANumberedBalloonIsACircleSoItsNumberIsCounted`).

### Arranging them

`LEADER ALIGN id id... | SELECTION [x=] [spacing=mm]` lines the notes up in a
column, as AutoCAD's MLEADERALIGN does: each leader's last vertex - where its
note hangs - goes to one x (the topmost note's, or `x=`), and with `spacing=`
the notes are stacked that many paper millimetres apart from the top down, in
the order they were; nothing else of a leader moves
(`AlignPutsTheNotesInAColumnTopDown`). `BALLOON RENUMBER [start=1]
[order=id|x|y]` numbers the numbered balloons again from `start`, in the
order they were made, across the sheet by their tips, or down it; a smart
balloon is left alone (`RenumberingNumbersTheBalloonsInTheOrderAsked`). Each
is one step, and none when nothing moves.

### One set of edits

`include/katana/cad/annotation/leader_edit.hpp` is every leader edit - the
note and look (`changeLeaders`, `applyLeaderChange`), attaching, freezing
and detaching, an attribute through the leader, one per entity
(`leadersFor`), aligning, renumbering - each checked against the model and
returned as ONE command or refused - a template that does not check, a
style not in the drawing, a size below 0 mm (`paper=-3` included; 0 is the
text style's height, or no landing) - never half made. The verbs parse their
words into it, and the window's Leaders manager ("In the window" below)
fills it from its form, so a verb typed and a button pressed cannot differ.

### Freezing and letting go

`LEADER FREEZE id...` turns a smart note into the words it says now and
keeps the tip following; `LEADER DETACH id...` lets the tip go, freezing a
smart note first, since a note read off nothing would say nothing. Both are
one step. `INFO` and `LIST` print what a leader says, its template and what
it is on; the property panel shows the same (`Text`, `Template`, `Label
style`, `On`).

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
| `LABEL id... style=`, `LABEL SELECTION`, `LABEL ALIGN name`, `LIST`, `SET`, `DELETE`, `LAYOUT [scale= collisions=]` | `scale=600 considered=5 placed=5 displaced=0 suppressed=0 orphaned=0` and a `label=... x= y= candidate= text=` line each |
| `AUTOLABEL RULE ADD \| SET \| DELETE \| LIST`, `RUN`, `PREVIEW`, `CLEAR` | `autolabel created=2 kept=0 removed=0 skipped=0` and `rule=... labels=N` |
| `DIM LINEAR \| HORIZONTAL \| VERTICAL \| ALIGNED \| ANGULAR \| RADIUS \| DIAMETER \| ORDINATE \| BASELINE \| CONTINUE` | `created dimension id=5 kind=diameter measures=10 text=Ø10.000 associative=yes`, `created dimensions=2 ids=7,8` |
| `LEADER p p... text= \| template= \| labelstyle= arrow= callout= style= paper= arrowsize= landing=`, `BALLOON` | `created leader id=26 text="IL 10.500" associative=yes target=12 anchor=position template="IL {prop.invert:.3f}"` |
| `LEADER VALUES id \| #id[.point\|@x,y]` | `target=12 type=Point` and a `value=rl text=12.500` line each |
| `LEADER LIST [id...] [target=id]`, `SET id... tip= at= ...`, `ATTACH id p`, `DETACH`, `FREEZE` | `id=26 layer=0 vertices=2 tip=10,10 target=12 anchor=position smart=yes template=... arrow=ClosedFilled callout=none text=...` |
| `LEADER PROP id SET key value [type] \| DELETE key` | `leader=26 target=12 property=invert text="IL 10.500"` |
| `LEADER FOR id... \| SELECTION`, `BALLOON FOR` | `created leaders=4 ids=7,8,9,10 skipped=1` |
| `LEADER ALIGN id... [x= spacing=mm]`, `BALLOON RENUMBER [start= order=id\|x\|y]` | `aligned leaders=3 x=22 spacing=4`, `balloons=3 renumbered=3` |
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

**Leaders.** Under the Annotate menu's tools, three entries open the Leaders
manager (`LeaderManagerDialog`, `src/katana_qt/annotation/leader_manager.hpp`,
the dialog `leaderManagerDialog`) on one of its tabs: **Leaders...**
(`annotateLeaders`), **Leaders for Selection...**
(`annotateLeadersForSelection`) and **Arrange Leaders and Balloons...**
(`annotateArrangeLeaders`), each given a menu letter the tools have not taken
(`AnnotationWorkbench::addLeaderActions`). It is the smart leaders' front end
and decides nothing of its own: every button is an edit of
`include/katana/cad/annotation/leader_edit.hpp`, the same the LEADER and
BALLOON verbs make, one undo step each, and a refusal is said in the
dialog's problem line in the command's words.

* **Leader**: every leader in the drawing, by what it says, and a form for the
  chosen one, which follows the drawing's selection. The form says what the
  tip is on in words ("Polyline 1, 50.0% along segment 2") and, for a tip
  along a line, an arc, a circle or a polyline segment, how far along in per
  cent, which Apply moves it by; **Attach to Selected** puts the tip of a
  plain leader, or of one on something else, on the first entity selected with
  it that offers a place (not a dimension or a label), at the place of it
  nearest the tip, and is no step where the tip is already (`LEADER ATTACH`,
  `AttachToSelectedPutsTheTipOnTheEntityAndAlongMovesIt`). The note is Text, a
  Template read off that entity, or a Label style's template, checked as it is
  typed and shown as it would read ("Says"); a style's template is shown to
  read, and read again when the style is edited
  (`AStylesNoteIsReadAfreshWhenTheStyleChanges`). Beside it are the values the
  entity offers at the tip: double-clicking one puts `{name}` into the note
  where the cursor is - the end of the note until the user moves it - and
  makes the note a template. Below: the arrow, callout, text style and sizes;
  an attribute of the entity set or removed through the leader, of the type
  chosen or as typed (`LEADER PROP`, which reads a value as `PROP SET` does -
  `CommandInterpreter::propertyValue`); and Freeze Note, Detach Tip and Apply.
  Choosing a label style lends the form that style's text style and height, as
  `labelstyle=` lends a leader them; choosing another style puts back what the
  first lent before lending its own, and so does a style appearing in or going
  from the drawing under the form
  (`AStyleThatComesOrGoesUnderTheFormLendsItsLook`); choosing Text or Template
  again puts back the words typed, and the look from before in each field
  still showing what the style lent - not in one the user has changed since
  (`ANoteMadeAStylesAndBackKeepsTheUsersWordsAndLook`,
  `ALookTypedAfterALendIsKeptAndAStylesHeightIsLentExactly`).

  Apply sends only what the user changed: the form remembers what it showed
  and leaves out every field still showing that, so a value its widgets cannot
  hold exactly - a height finer than their 3 decimals, a landing past their
  1000 mm, a non-breaking space - is never rewritten, and an untouched form
  applied is no step
  (`AnUntouchedFormIsNoStepAndWhatItsWidgetsCannotHoldIsKept`). A note made a
  label style's is the one exception, since the style lends its look to
  whatever the edit leaves out: the text style and height the form shows are
  sent unless they are the style's own, which is then lent whole, finer than
  the form's decimals - so a look set back to the leader's own over the
  style's is kept
  (`ALookSetBackOverAStylesIsKeptAndAnotherStyleLendsOverTheOneBefore`). With
  the shown leader one of several selected, Apply, Freeze and Detach change
  them all, in one step, and the form says so
  (`SeveralSelectedLeadersAreChangedTogetherInOneStep`) - all but Along, which
  is where the shown leader's tip is on its own entity: changed with others
  selected, Apply refuses it rather than move every tip to that one place.
  Freeze, Detach and Attach act on the leader as drawn, so while the form has
  changes not applied they are greyed, their tips say why, and they refuse
  (`TheListChoosesAndFreezeWaitsForTheFormToBeApplied`). The form is read
  again from the leader when the leader changes under it - an undo, a verb
  typed - and left alone, unapplied edits and all, when something else changes
  (`TheFormFollowsTheSelectionAndAnUndoButKeepsUnappliedEdits`); a drawing
  opened or started afresh starts the form afresh, even where the new drawing
  has the very same leader (`ADrawingReplacedStartsTheFormAfresh`). It watches
  the document through `DocumentWatcher`, as `docs/desktop.md` asks.
* **For Selection**: a leader, or with Balloons ticked a balloon, to each
  selected entity (`LEADER FOR`, `BALLOON FOR`): the note as text, a template
  or a label style's, checked and shown as it would read for the first
  selected entity that has a place for a leader, with the values that entity
  offers there to double-click into it; the look (arrow and callout, each
  Automatic by default - a dot for a tip inside an outline, a circle for a
  balloon - text style, height, arrow size, landing), a label style's shown as
  it lends it, as on the Leader tab, and what the boxes show being what the
  leaders get (`ForSelectionShowsTheLookAStyleLendsAndMakesWhatItShows`); and
  the note's direction and distance on paper. A balloon with no note is
  numbered on from the highest there is, and must stay a circle
  (`ANumberedBalloonMustStayACircle`). It says how many it made and why it
  skipped any, and what it made is selected, so the Leader tab shows it and
  Arrange can line it up
  (`ForSelectionIsPreviewedForTheFirstAndPlacedAsTold`).
* **Arrange**: the selected leaders' notes lined up at the topmost's x or a
  given one, stacked or not (`LEADER ALIGN`), and the balloons numbered again
  from a number, as made, across or down (`BALLOON RENUMBER`); numbering
  that changes nothing is no step
  (`ArrangeAlignsTheSelectedNotesAndRenumbersTheBalloons`).

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

Smart leaders added a third, `kBlobVersionSmartLeader`, on the same rule
(`fitsVersionTwo`): written only for a dimension or a leader with an anchor
Along or Inside - every anchor then carries its fraction - and for a smart
leader, whose `fields` and `labelStyle` follow its version-2 payload. A plain
leader and every other annotation is still version 2, byte for byte
(`AVersionThreeBlobHoldsThemAndNothingElseNeedsOne`), so no schema migration
was needed, and a build from before refuses exactly the entities it could
not read correctly (`SmartLeadersAndAnchorsAlongSurviveSavingTwiceAndReopening`).

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
kinds are skipped with a warning and a leader is its line, landing and note -
a smart leader's note as it reads (`leaderNote`), without `{code}`, which
survey coding decides and the writer cannot see.
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
| `tests/entity/annotation/test_leader_values.cpp` | Along and Inside anchors, the nearest place, what a leader can say of every kind of target, the leader template check, validation, the version-3 blob and JSON |
| `tests/cad/annotation/test_smart_leaders.cpp` | the LEADER verbs end to end: templates, label styles, `#id@x,y` and `.inside`, VALUES, LIST, SET, ATTACH, DETACH, FREEZE, PROP, FOR, ALIGN, BALLOON FOR and RENUMBER; following stretches and moves, going with the target, copies |
| `tests/cad/tools/test_smart_leader_tool.cpp` | the Leader tool: a tip put on what it is clicked on, fields checked as typed, a note that would say nothing refused |
| `tests/cad/annotation/test_leader_edit.cpp` | the shared edits: an unchanged edit is no command, what cannot be made is refused rather than skipped (negative sizes too), a label style's look lent, attaching (and again, no command), the value rows, where a leader goes on an entity, a numbered balloon kept a circle |
| `tests/qt_widgets/annotation/test_leader_manager.cpp` | the Leaders manager driven by object name and its widgets' own signals: the form, the template checked and previewed, only what changed applied (values the widgets cannot hold kept), the note kind switched and back, a style's template read afresh, values double-clicked in at the cursor, a label style's look lent, set back, kept when typed over and lent exactly, and lent anew when the style comes or goes, attributes set, attach (past a dimension, waiting for Apply, again no step) and along, several leaders at once, freeze and detach waiting for Apply, following the selection, undo and a replaced drawing, For Selection's preview for the first with a place, its look and a style's, and placing, numbered balloons kept circles, Arrange, each menu entry's tab |
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
  anchor, and moved off its placed position with `LABEL SET id at=x,y`
  rather than by dragging.
* **PURGE** does not purge unused text or label styles.
* **DXF**: no MTEXT, masks, width factors or slants on export, and nothing
  on import becomes a label, a leader or a dimension kind.
* **The 3D view** draws a label as a marker at its anchor and a leader as
  its line: its words are worked out for a scale the 3D view does not have.
* **Smart leaders** have one arrow: a multi-leader - one note, several arrows
  to several pits, "3 x SMH" - is not built. A tip on a polyline names its
  segment by number, as a Vertex anchor does, so inserting a vertex before
  it moves the tip to the next segment along. The Leader tool attaches only
  to an entity within the pick aperture; clicking inside a lot does not
  point into it (`#id.inside`, or `LEADER FOR`, does). DXF export writes the
  note as it reads, not a field a CAD reader could update.
