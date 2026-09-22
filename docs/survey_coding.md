# Survey coding: the mapfile, the linestyles and the symbols

`PLAN.MD` 20.3. How a surveyor's field code becomes a drawing.

A surveyor shoots a point and types a code: `WM01`. That code is not a label -
it is an instruction. A 12d customisation turns it into a water main: in model
`SURVEY SERVICES`, coloured `sui water potable`, joined into a line rather than
left as a point, drawn with the `WATR Main` linestyle. Other codes get a
symbol instead: `AC*` puts a `CULT Bollard` at each point.

Three files do that, and they are useless apart:

| File | What it says |
|---|---|
| a **mapfile** (XML) | what each code becomes: model, colour, linestyle, weight, whether it is a line or a point, which symbol, what attributes |
| a **linestyle library** (`.4d`) | what each linestyle is drawn with |
| a **symbol library** (`.4d`) | what each symbol is drawn with |

The work was built against a real production customisation from a road
authority - 728 mapfile rules, 322 linestyles, 474 symbols, and a second
mapfile of 900 more rules. **It is third-party material under its own licence
and is NOT part of this repository.** Drop it in `docs/12d Refrence Files` to
use it; every test that touches it skips when it is absent, so the suite
stays green without it, and nothing in the source names the files.

## A symbol is a linestyle

This is the fact the whole design rests on, and it is not a simplification:
the 12d manual says it where it describes vertex symbols - "There can be the
same symbol (**defined as a linestyle**) for every vertex". The two `.4d`
files have one grammar, one reader and one type. What separates a symbol from
a linestyle is a single line in the definition, `mode vertex`: put the strokes
at each vertex of a string rather than running them along it.

So `entity::LineStyle` is both, and `atVertices` is the flag. Katana already
half-knew this - `Style::symbol` and `Style::linetype` have always been two
names on one record, with the comment "the style of a point and the style of a
line are one thing".

## The grammar, measured rather than assumed

```
<kind> "NAME" {                 kind = paperstyle | worldstyle | twoptstyle
  group "Survey/WATR"           a folder path, the tree a browser shows
  mode vertex                   a symbol: strokes go at each vertex
  length 2.5                    the period of the pattern along a line
  factor 5                      a scale on every coordinate
  xorigin 0   yorigin 0
  xorigin1/yorigin1             twoptstyle: the first anchor
  xorigin2/yorigin2             twoptstyle: the second
  stretch_mode 2  cycle_mode 2  twoptstyle: kept, not yet acted on
  colour "pen 035"              the pen for what follows; "view_colour"
                                means the entity's own colour
  move X Y                      pen up
  draw X Y                      pen down
  arc RADIUS START END          degrees, about the CURRENT point
  circle RADIUS                 about the current point
  dot RADIUS
  text "T" ANGLE HEIGHT "justify" "font" WIDTH a b c
}
```

`arc`, `circle`, `dot` and `text` carry no point of their own - they are drawn
about wherever the pen is. Forgetting that put every circle at the origin in
the first version of `LineStyle::bounds`, which is why it is said twice here
and in the code.

The three kinds differ in what a coordinate MEANS:

- `worldstyle` - model units. The mark stays the same size on the ground and
  grows on the page as you zoom in. A 6 m road marking is 6 m wide.
- `paperstyle` - millimetres on the plot. The mark stays the same size on the
  page whatever the scale. A 1.5 mm tick prints 1.5 mm.
- `twoptstyle` - stretched between two anchors, so the definition fits the
  span it is drawn across: a gate, a doorway.

**The last three numbers of a `text` command are not interpreted.** All 907
uses have 0 in the first; the other two take values like -0.3 and 0.035, and
no documentation available here names them. They are kept so that what was
read can be written back. Guessing that they are an offset would move the
text, and text that is subtly in the wrong place is worse than text that is
plainly unstyled.

## Where the pieces live, and why

| Piece | Layer | Why there |
|---|---|---|
| `entity::LineStyle`, `entity::StyleLibrary` | `entity` | three layers need them and cannot all see each other: `commands` applies a code, `cad` draws a definition, `archive12d` reads the file |
| `archive12d::readStyleLibrary` | `archive12d` | it already owns 12d's encoding detection, its lexer and its colour names |

**A library is not part of a Model and is not saved inside a project.** That
is how 12d works - the libraries are a site-wide customisation shared by every
project, named by a project rather than copied into it. The alternative, a
table in the Model beside Layer and Linetype, was rejected for two reasons:
one production customisation alone is 792 definitions and 35,000 strokes,
which would be written into every project file that used one of them; and two
projects would then be able to disagree about what "WATR Main" looks like.

`StyleLibrary` is `NamedTable`, the same container as every other table of
named things, rather than a new one.

## Names compare with regard to case, and that is a decision

12d's own comparisons are not case-sensitive - the file format overview says
"Bypass" and "BYPASS" are the same linestyle. Katana's tables are
case-sensitive, and the library is one of them.

That difference is measured rather than overlooked. Of the **372 references**
the reference mapfile makes into its two libraries, every single one
matches exactly and not one needs case folding. Making the table
case-insensitive would have meant either a second container or a change to
`NamedTable` that the evidence did not ask for. A name that does not resolve
is reported by the caller instead of being guessed at, so if another
customisation ever does spell one differently it will say so rather than draw
the wrong mark.

## What the reader takes

Read against the real customisation, with the counts checked by a script that
uses none of Katana's code:

| | definitions | commands |
|---|---|---|
| the linestyle library | 322 blocks (238 paperstyle, 47 twoptstyle, 37 worldstyle), 321 kept - one name is defined twice | 10,969 `move`, 10,770 `draw` |
| the symbol library | 474 blocks (473 worldstyle, 1 twoptstyle) | 6,076 `move`, 6,547 `draw` |
| both, loaded as one customisation | 792 definitions, 157 of them symbols, in 71 groups | |

**No warnings.** Every keyword in 1.2 MB of production data is one the reader
knows. A keyword it did not know would be counted and named rather than
skipped quietly, which is the same promise `readArchive` makes.

Four definitions appear in both files, so "already exists" is the normal case
and the later file wins - a customisation is loaded in layers.

## The three files link up

Checked with a script, outside Katana:

| Reference | Resolves exactly | Missing |
|---|---|---|
| 177 distinct `<linestyle>` values | 175 | `0` and `1`, which are 12d's built-in plain lines |
| 195 distinct symbol `<style>` values | 193 | `Circle Single` and `SBEND`, from 12d's own standard library, which is not one of these files |

So a customisation is not necessarily self-contained, and an unresolved name
has to be reported rather than treated as a fault in the file.

## The mapfile is written in sections, and the section is the meaning

This is the thing that is easy to get wrong, and the first version of the
reader did get it wrong. `<map_file>` does not hold one list of rules. It
holds up to ten SECTIONS, and which section a rule is in is what says what the
rule is about:

| Section | What its rules say | Detail mapfile | `names.4d` |
|---|---|---|---|
| `map_data` | model, colour, breakline, linestyle, weight, group | 457 | 573 |
| `vertex_symbol_data` (and `_v9`) | the symbol at each vertex, and `hide` | 200 | 201 |
| `vertex_textstyle_data` | how the code's text is drawn | 7 | 7 |
| `pipe_data` | the string drawn as a pipe, and its attributes | 16 | - |
| `vertex_pipe_data` | the same per vertex | 16 | - |
| `segment_pipe_data` | the same per segment | 16 | - |
| `string_attribute_data` | attributes on the string | 12 | - |
| `vertex_attribute_data` | attributes on each vertex | 1 | - |
| `tinable_data` | whether the code is used in a surface | - | 119 |

Reading only `<map_data>` takes 457 of the detail mapfile's 725 rules and
quietly loses every symbol - which is exactly what the first version did, and
exactly what the test against the real file caught. The element name inside a
rule is not enough either: `<map_attributes>` means attributes on the STRING
inside `string_attribute_data` and attributes on each VERTEX inside
`vertex_attribute_data`. That is why `SurveyRule` records the section it came
from.

`names.4d` is a second mapfile, despite carrying the extension a linestyle
library also uses. What makes a file a mapfile is that it contains a
`<map_file>` element, not what it is called.

## How a code resolves

A key is either exact (`PABB`) or a prefix (`WM*`). Measured over both
mapfiles: 1,364 distinct keys, and **not one** uses a wildcard anywhere but at
the end. A key of any other shape is refused rather than matched
approximately - putting a code in the wrong model is worse than reporting that
a rule could not be used.

`lookup` collects every rule whose key matches and combines them, most
specific first:

1. an exact key beats every prefix;
2. a longer prefix beats a shorter one;
3. `*` is last;
4. rules of equal specificity keep the order they were read, so an earlier
   mapfile wins over a later one.

For each field, the first rule that says anything about it wins. **Attributes
are the exception: they accumulate.** A `*` rule saying every pipe has a
`DepthLocation` and a `SW*` rule giving the material are both meant - that is
the whole reason the mapfiles carry a `*` rule beside the coded ones - so they
are merged by name, with the more specific rule's value kept where both name
the same attribute.

This combination rule is Katana's reading. No documentation of the mapfile
format was available here, so the evidence is the files: rules in different
sections never contradict each other (they are about different things), and
where two rules in one section match a code the wildcard one is plainly the
fallback. Where the rule is load-bearing it is stated in a test, so a
different reading would have to change a test to take effect rather than
slipping in.

## What the reader takes

| | rules | warnings |
|---|---|---|
| the detail mapfile | 725 over 465 distinct keys | none |
| `names.4d` | 899 | one, and it is right: an `<item>` holding only a `<group>`, which names no code and so could never apply |

An unknown section is named with how many rules went unread; an unknown field
inside a rule is named and the rest of the rule is kept.

## From a definition to geometry

`cad::styleDrawing` turns a definition into polylines and texts in model
coordinates, so the viewport, the plotter and a preview all paint the same
thing - the arrangement `cad::symbolStrokes` already had, for the same reason.

The definition decides how it is placed, not the caller:

- `atVertices` - a symbol at each vertex, scaled and rotated.
- `twoptstyle` - stretched across the whole line, its two anchors mapped onto
  the ends.
- otherwise - repeated along the line, the definition's +x running along and
  its +y to the LEFT.

Three decisions worth recording:

**The pattern bends with the line.** Each point is placed by its own distance
along the line rather than by rigidly transforming a whole instance, so a
pattern that spans a corner follows it instead of flying off. On a straight
run the two are identical. The cost is that a long pattern on a tight curve
is distorted rather than detached, which is the right way round for survey
linework.

**On a corner the outgoing leg wins.** An instance landing exactly on a
vertex has two directions to choose from. It takes the one it is about to be
drawn along. This is arbitrary but has to be decided, so it is decided in one
place and stated in a test.

**A two-point style is scaled equally in both axes.** 12d's `stretch_mode`
and `cycle_mode` are kept but not acted on, because no documentation
available here says what their values mean. A similarity rather than an
independent scale per axis means a doorway stretched across a wider opening
is still a doorway; shearing it would be a mistake that looks like a feature.

**A pattern cannot run away.** A definition with a 1 mm period laid along a
30 km traverse would ask for thirty million instances. It is capped at 20,000
and what comes back is still a drawing.

## A symbol name stopped being a closed set

`Style::symbol` used to be validated against sixteen built-in names. The
reference symbol library alone names 473 symbols and **not one** of them is
among the sixteen, so that check made the mapfile unusable: a style saying
`CULT Bollard` was refused by the model.

It is now a name resolved when it is drawn, exactly as `Style::linetype`
already was - which also makes the two consistent, where before one was a
closed set and its twin was not.

The check was not simply deleted, because catching a typo is worth something:

| Where | What it does | Why |
|---|---|---|
| `entity::validate(Style)` | accepts any name that is valid UTF-8 | a project can be opened before its library is loaded, and a 12da import brings names of its own |
| the `STYLE SET ... symbol` command | refuses a name that is neither built in nor in the loaded library | a person typing a name should be told about a typo, which is the courtesy the `hatch` field already paid |

`cad::Document` carries the loaded library and mapfile, and `definitionFor`
is the single place a name is resolved.

## Loading a customisation

`archive12d::readCustomisation` takes a list of files and works out what each
one is BY LOOKING INSIDE IT. That is not fastidiousness: of the four files
this was built against, two are mapfiles and two are style libraries - and
THREE of the four end in `.4d`, so that extension is two different formats in
one folder and a loader that went by the name would read half the
customisation as the wrong thing.

It lives beside the readers rather than in a front end because both front ends
need it, and it needs no third-party library, so it costs `archive12d` nothing
of the property that lets it build with `-DKATANA_BUILD_IO=OFF`.

Loading the real customisation, from the command line:

```
katana_cli -c 'CUSTOMISE "docs/12d Refrence Files/<linestyles>.4d" ...'
  <linestyles>.4d: style library, 322 definitions (1 replacing one already loaded)
  <symbols>.4d:    style library, 474 definitions (3 replacing one already loaded)
  <detail>.mapfile: mapfile, 725 rules
  names.4d: mapfile, 899 rules
  warning: names.4d: an <item> of <map_data> has no <key> and was skipped
  5 names the mapfile asks for that no loaded library defines: "0", "1", ...
792 linestyle and symbol definitions in 71 groups, 157 of them symbols
1624 survey code rules over 632 distinct codes
```

In the application it is **File > Load 12d Customisation...**, which takes
several files at once and writes the same report into the command log.

## Drawing it

The viewport resolves a name against the loaded library first and the sixteen
built-in shapes after:

- a point whose style names a definition is drawn with that definition's
  strokes, at the style's size and the symbol's rotation;
- a line whose style's `linetype` names a definition has that definition laid
  ALONG it, in addition to the line itself - a 12d linestyle is strokes on the
  line, not a dash pattern cut out of it, so the ticks of a fence style sit on
  the fence;
- a `colour` command inside a definition changes the pen for the strokes that
  follow it, and `view_colour` means the entity's own colour.

## A 12d linestyle IS the line

Two bugs came out of looking at a real customisation on screen, and both made
every linestyle come out as a plain continuous line. They are worth writing
down because the same mistake is easy to make twice.

**The definition replaces the line; it does not decorate it.** Look at what a
definition actually contains:

```
paperstyle "BDGE Abutment Bottom" {
  move 0 0
  draw 3 0     <- the line itself, for three units
  move 5 0     <- and then a two-unit GAP
}
```

That is a dash pattern written as strokes. A fence style carries the fence as
well as its ticks. So drawing the plain line underneath fills in every gap,
and everything looks continuous - which is exactly what it did. The plain line
is now drawn only when no definition applies, or when the entity carries a
hatch (which is painted inside the same call, and which a 12da never brings).

**A trailing `move` is the gap, and counts towards the period.** The period of
a pattern, when the file gives no `length`, is how far the PEN travels - not
the span of what was drawn. Measuring only the drawn part read
`BDGE Abutment Bottom` as a period of 3 rather than 5, so each dash butted
against the next and the pattern was solid even after the first fix. The pen
extent now includes bare moves, and `StyleDrawing.ATrailingMoveIsTheGap...`
is the test.

**A millimetre is not a pixel.** `paperScale` treated one plot millimetre as
one pixel, making every paper linestyle about four times too small: a fence
style's ticks came out a pixel tall and vanished into the line. It is now a
millimetre of screen, from the widget's own DPI.

## The import has to keep the real names

The readers, the drawing and the menu can all be right and nothing appears,
because the 12da import was never connected to them:

| What the import did | Why nothing drew |
|---|---|
| set `Style::name` to the 12d linestyle name but left `Style::linetype` as `"continuous"` | the name reached nowhere a renderer looks |
| stored `builtInSymbolFor(name)` instead of the name - "SEWR Manhole Cover" became "manhole" | the 473 symbols in a loaded library could never be matched |
| put a survey code nowhere called "code" - a 12da carries it as the string NAME | `CODE` found nothing to apply |

All three are fixed: the import keeps the REAL 12d names, the guess at a
built-in shape happens at draw time only when nothing defines the name, and
the code property is found rather than assumed.

Measured before assuming this would help: of 82 distinct linestyle names in
one real archive, 79 are in the reference library; 214 of 217 in another. The
misses are `0`, `1` and the empty name - 12d's own plain lines.

## A test that passed while doing nothing

Worth recording, because it nearly got through. The headless test that loads
the customisation into the real window first passed in 0.29 s having loaded
**nothing**: the list of paths was passed to `cmake -P` as
`-DCUSTOMISE=a\;b\;c`, the escaped separators arrived as part of the paths,
every `EXISTS` failed, and the helper dutifully ran the application without
the switch. The screenshot showed a window with no customisation in its log,
which is what gave it away.

It now takes a DIRECTORY and globs it, so there is no list to mangle, and the
helper prints how many files it found. A test that cannot report what it
actually exercised is a test that can pass for the wrong reason.

## Applying a code

`cad::applySurveyCodes` is what the three files are FOR. An entity carries a
field code in a property - `code` by default - and this turns the mapfile's
answer into the layer, the style and the properties it should have.

It PLANS rather than acts: everything comes back as one
`commands::Transaction`, so twenty thousand coded points are **one undo**, not
twenty thousand. A command per entity would make an undo stack nobody could
use.

Decisions worth recording:

- **12d's model becomes Katana's layer.** Both are `/`-separated paths naming
  where something lives, and it is the mapping the 12da import already makes.
- **The style is named after the 12d linestyle**, or the symbol where there is
  no linestyle. The same rule the 12da import follows, so a drawing coded here
  and a drawing imported from 12d share style names instead of growing two
  sets of them.
- **A name the library does not define is still recorded**, and reported. It
  is what 12d says the thing is; it draws as a plain line or mark until a
  library defines it, which is better than dropping the information.
- **`$PipeDiameter` is left alone.** An attribute whose value names another
  attribute cannot be resolved without the survey data the drawing was made
  from. It is counted and reported rather than written literally.
- **Only text is a code.** A number in the code property is a measurement
  someone named badly; treating `1.5` as a field code would file it under
  whatever the rule for `1*` says.
- **The colour comes through a callback.** 12d's colour names are known to
  `archive12d`, which `cad` may not see, so the front ends pass
  `archive12d::standardColour`. Without one, colours are left alone rather
  than guessed at.

From the command line, the whole chain:

```
katana_cli
  -c 'CUSTOMISE "…/<linestyles>.4d" "…/<symbols>.4d" "…/<detail>.mapfile"'
  -c 'POINT 0,0' -c 'SELECT ALL' -c 'PROP SET code WM01 text' -c 'CODE' -c 'LIST'

  2 entities carry a "code", 2 of them codes the mapfile has a rule for
  Applied: 1 layers and 1 styles created. UNDO puts it all back.
  1  Point  layer=SURVEY SERVICES  at 0,0
```
