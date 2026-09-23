# Survey coding: the mapfile, the linestyles and the symbols

`PLAN.MD` 20.3 (the plan has since been removed; its section numbers survive
here as history). How a surveyor's field code becomes a drawing, and the
decisions of 2026-09-23 (D1-D9, tabled in `docs/cad.md`) that govern it.

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

So `entity::LineStyle` is both. Katana already half-knew this -
`Style::symbol` and `Style::linetype` have always been two names on one
record, with the comment "the style of a point and the style of a line are
one thing".

**`mode vertex` is not what makes a symbol, and this file used to say it
was.** It is one signal among four. Measured with a script outside Katana:
157 of the 792 definitions say `mode vertex` (156 of them in the symbol
library, one in the linestyle library), while the symbol library alone holds
474; and of the 193 symbols the detail mapfile names that a library defines,
only **48** are `mode vertex` - every one of the 193 comes from the symbol
library. A symbol browser built on `atVertices` would have hidden three
quarters of the symbols the mapfile actually uses. So the rule (the lead's
decision D3, `cad::classifyDefinition` in `include/katana/cad/style_catalogue.hpp`)
is that a definition is offered as a SYMBOL when any of these holds:

- it is `mode vertex`;
- a `vertex_symbol_data` rule of the loaded mapfile names it;
- some `Style::symbol` names it;
- the file it was read from has "symbol" in its name, in any case - 12d's own
  `user_symbols_*.4d` convention, which is why every definition now carries
  `LineStyle::source`, the NAME of its file (never a path).

It is offered as a LINESTYLE when it is not `mode vertex`, so a definition
can be both, and `DefinitionKind` keeps each reason so a browser can say why
a definition is listed where it is. The CUSTOMISE report's "157 of them
symbols" and the window's "(157 symbols)" still count `mode vertex` alone
(`entity::vertexStyleNames`); read them as "157 of them `mode vertex`".

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

**A radius is a length.** The reference libraries write 45 arcs with a
NEGATIVE radius (U-turn and speed-zone markings). The sign does not choose a
side: drawn with the signed radius every one of them came out turned half a
turn about its centre, and `flattenDefinition` now takes `|r|` for arcs and
circles alike. The regression test, on a U-turn, puts the arc's end at
y = -0.4735 by hand where the signed radius gave +0.4925.

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

The rule has two halves since the managers' foundations were built (decision
D3): **storage never folds case, a search always does.** `cad::filterChoices`
(the linetype and symbol pickers) and `cad::codeTableRowMatches` (MAPFILE
LIST's filter) match a substring with case ignored, and `cad::explainCode`
reports a key the code would have met but for its case or surrounding blanks
(`NearMissKind`) - the only way "wm01" getting nothing from the rule for
`WM*` is ever explained, since matching itself is byte for byte.

## What the reader takes

Read against the real customisation, with the counts checked by a script that
uses none of Katana's code:

| | definitions | commands |
|---|---|---|
| the linestyle library | 322 blocks (238 paperstyle, 47 twoptstyle, 37 worldstyle), 321 kept - one name is defined twice | 10,969 `move`, 10,770 `draw` |
| the symbol library | 474 blocks (473 worldstyle, 1 twoptstyle) | 6,076 `move`, 6,547 `draw` |
| both, loaded as one customisation | 792 definitions in 71 groups, 157 of them `mode vertex` (not the number of symbols - see "A symbol is a linestyle") | |

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
mapfiles: 632 distinct keys in 1,624 rules (the CLI's own count below; this
said 1,364 until the audit of 2026-09-23), and **not one** uses a wildcard
anywhere but at the end. A key of any other shape is refused rather than matched
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

**Matching is not the same as having a rule (decision D5, audit CAD-05).**
The reference mapfiles carry bare `*` rules - sixteen in each pipe section -
that every code meets, so "some rule matched" was true of a typo too, and no
unknown code was ever reported. `SurveyMatch::kind` now says how a code met
the map by its most specific key (`SurveyMatchKind`: `Exact`, `Prefix`,
`FallbackOnly` for nothing but `*`, `None`), and `SurveyMatch::matched()` is
true only when a key more specific than `*` matched, or the combination names
a model, a linestyle or a symbol. A code only `*` answers is FALLBACK-ONLY: its
`*` attributes are still applied - that rule does say every code gets them -
but it is counted in `SurveyCodingReport::fallbackOnly` and listed in
`fallbackOnlyCodes`, apart from both the matched and the unmatched codes.
Linework uses the same word: a string name only `*` answers is its own key
with no number (`cad::splitStringName`).

**A rule's identity is its index**, and its index is also its precedence
(among rules of equal specificity the earlier wins). So the map is edited by
index - `SurveyMap::at`, `replace`, `remove`, `insert`, `move` - each refusing
a rule `validate` refuses (a key with surrounding blanks, which can match
nothing a surveyor types; a model that is not a valid layer path) and then
leaving the map as it was. Every edit, `add` included, invalidates the rule
pointers `match` handed out and the indices `matchIndices` did; anything
cached keys on `cad::Document::surveyMapGeneration()`, never on a
`SurveyRule*`. Lookup is indexed by key - one hash probe for the exact key and
one per prefix length of the code - where it used to test every rule: 1,624
rules against 20,000 points was 32 million key comparisons.

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

**A pattern cannot run away, and is never cut short.** A definition with a
1 mm period laid along a 30 km traverse would ask for thirty million
instances. This paragraph used to say the pattern was capped at 20,000 "and
what comes back is still a drawing": it was a drawing of the first 20,000
repeats, the rest of the line drew nothing - the viewport had already skipped
the plain line - and nothing said so (audit CAD-04). The budget is now
all-or-nothing, as `forEachDash`'s is: over it, `layLinestyle` lays nothing
and says `OverBudget`, and the caller draws the plain line. A caller that can
see only part of the line passes the visible box and gets only the repeats
that can reach it, each where it falls on the WHOLE line; `docs/cad.md`
("What a style draws") has the details.

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
is the single place a name is looked up. What a name then DRAWS - which of the
library and the model's Linetype table wins, and what a symbol falls back to -
is `cad/style_resolver.hpp`'s question, described in `docs/cad.md` ("What a
style draws").

**Both are session data, by decision (D1).** The library and the map live on
`cad::Document`: not in the `Model`, not undoable, not saved in the project.
An editor of either works on a copy - a buffer the dialog owns - and commits
it whole with `setStyleLibrary` or `setSurveyMap` (Apply; Revert throws the
buffer away). Each bumps `libraryGeneration()` or `surveyMapGeneration()` and
notifies the listeners, and an edit is kept by EXPORTING it to a file - see
"Writing it back" below - not by saving the project. No such editor exists
yet; the foundations below are what one will stand on.

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
katana_cli -c 'CUSTOMISE REPLACE "docs/12d Refrence Files/<linestyles>.4d" ...' -c CUSTOMISE
  <linestyles>.4d: style library, 322 definitions (1 replacing one already loaded)
  <symbols>.4d: style library, 474 definitions (3 replacing one already loaded)
  <detail>.mapfile: mapfile, 725 rules
  names.4d: mapfile, 899 rules
  warning: names.4d: an <item> of <map_data> has no <key> and was skipped
Loaded now: 792 definitions, 1624 survey code rules
  3 names the mapfile asks for that no loaded library defines: "Circle Single",
  "LNMK Dividing - Separation Line S2 Multi Lane", "SBEND"
792 linestyle and symbol definitions in 71 groups, 157 of them symbols
1624 survey code rules over 632 distinct codes
```

(Run on 2026-09-24 against a build with the reference customisation compiled
in, which is why REPLACE: loaded on top of the same files, the merge adds
nothing and says "1624 rules were loaded already and are not added again".
The plain lines `0` and `1` and the built-in symbol names are no longer
listed as missing - they draw without a library. "157 of them symbols" is the
`mode vertex` count; see "A symbol is a linestyle".)

In the application it is **File > Load 12d Customisation...**, which takes
several files at once and writes the same report into the command log.

Every definition read is stamped with the NAME of the file it came from
(`LineStyle::source`, from `archive12d::sourceFileName`) - never the path, so
a library carries no trace of whose disk it was loaded from. A browser groups
and filters by it, and it is one of D3's four signals that a definition is a
symbol.

### A load goes ON TOP of what is loaded (decision D1)

Katana starts with the customisation compiled into it, so a person loading
their own symbol file wants their symbols ADDED, not the other 792 definitions
and 1,624 rules thrown away - which is what every load used to do, by
installing the load's library and map over the current ones, empty halves
included (audit QT-21). `archive12d::mergeCustomisation(currentLibrary,
currentMap, loaded, LoadMode)` is the rule:

- **`LoadMode::Merge`, the default.** What the load brings takes precedence,
  and nothing else is lost. A definition replaces the current one of its name
  (later wins, as between the files of one load). The rules a load gives a key
  in one section replace the current rules of that key in THAT section, every
  other rule is kept, and all of the key's loaded rules go in at the key's
  first current rule - ahead of every current rule of that key that stays. The
  reason is the fields two sections can fill (a comment; an attribute of one
  name from `pipe_data` and `string_attribute_data`, or from
  `vertex_pipe_data` and `vertex_attribute_data`): earlier wins a field, so a
  loaded group appended at the end lost to the current rule of the sibling
  section, and the load did not win after all.
- **`LoadMode::Replace`.** What the load brings is ALL there is - of each kind
  it brought. `removedDefinitions` and `removedKeys` say what went with it.
- **Either way**, a load that brought no definitions leaves the library as it
  was, and one that brought no rules leaves the map: an empty table is never
  installed by a load that did not bring one. `FileMerge` reports, per file,
  the definitions or keys it added and those it replaced.

**Where the front ends stand.** Neither calls `mergeCustomisation` yet.
`katana_cli`'s `CUSTOMISE [REPLACE] <file>...` (`runCustomise`,
`src/katana_app/main.cpp`) merges by default and replaces only when REPLACE is
the unquoted first word, in any case - a quoted path that begins with
"replace" is a path. REPLACE installs only the halves the load brought. But
its merge is its own, written before the rule above: definitions go through
`entity::addOrReplace` (the same result), while rules are APPENDED after the
current ones, skipping a rule the map already holds field for field - so there
the CURRENT rule of a key keeps precedence, the opposite of
`LoadMode::Merge`. The application's File > Load 12d Customisation still
replaces library and map wholesale (`MainWindow::applyCustomisation`; QT-21
stays open). Both should install `mergeCustomisation`'s result and show its
`files`; until they do, the two front ends and the library disagree about who
wins a field.

### What the project records

`storage::ProjectMetadata::customisation` holds the names of the files a
drawing was drawn with, in load order - a record, not a reference: nothing is
loaded from it, and a name need not exist where the project is opened. It is
stored as one `customisation` metadata key, the names separated by line feeds
(a Windows file name may hold `;` but not a line break), and `save()` refuses
a name that is empty or holds a line break or a path separator. Nothing fills
it yet: a front end should set it from `LineStyle::source` and the loaded file
names before saving. See `docs/model.md` for the metadata keys an older build
keeps for a newer one.

## Writing it back

Under D1 an edited library or map is kept by writing it to a file, so the two
readers now have inverses in `archive12d`, each tested as one: read what was
written and get back what was given (`tests/archive12d/customisation/`).

**`writeStyleLibrary(library, StyleLibraryWriteOptions)`** gives `.4d` text,
UTF-8, in name order, one command per line as 12d writes it. Every field is
written - the three kinds, `mode vertex`, group, length, factor, origins,
anchors, stretch and cycle mode, pens, every stroke, and a text's three
numbers kept but not understood - so `readStyleLibrary(writeStyleLibrary(l))`
is `l`. The one thing a file cannot hold is `LineStyle::source`, which is the
file's own name and is stamped again when it is read. `names` picks the
definitions to write, and a name the library lacks fails the write rather than
leaving a file the person believes holds it; `comments` become the `//` head
where 12d's own libraries carry their licence.

**`writeMapFile(map, MapFileWriteOptions)`** gives the mapfile XML as UTF-8
text; 12d's own are UTF-16LE with a byte order mark, which
`core::encodeUtf16LittleEndian` makes of it, and the reader takes either.
Sections go in 12d's order (`map_data`, `vertex_symbol_data`, `tinable_data`,
`vertex_textstyle_data`, the three pipe sections, the two attribute
sections), and within a section the rules keep their order. The subtle part
is the rule this file's "How a code resolves" rests on: among rules of ONE
key, the earlier wins every field more than one section fills. A map made by
loading two mapfiles, or by merging a load into the current map, can have a
key whose rules are not in 12d's section order, and regrouping them would
change what the code resolves to. So a rule is never written ahead of an
earlier rule of its own key: where 12d's order would put it there, the
sections are written again, in the same order, for the rules that must
follow, and the reader reads a repeated section in turn. Rules of different
keys may be regrouped, which no code can tell. A map in 12d's order comes back
rule for rule with each section written once; exporting the compiled-in pair
of mapfiles gives one extra `map_data` and one extra `vertex_symbol_data`.
Whether 12d itself reads a section that appears twice is not known here.
**Rejected:** refusing to write such a map and naming the key - merged maps
are normal, and under D1 export is the only way map edits are kept.

What a rule does not say is written as NO element, never as `0`: an empty
`<rotation/>` and a rotation of 0 read the same, but a size of 0 and no size
do not mean the same thing to a person reading the file. The writer fails,
naming the rule, rather than write a file that would not read back as the
map: a field the rule's section cannot carry, a pipe with nothing but
`active`, an attribute with no name or of a type other than text and integer,
a value with surrounding blanks (XML text is trimmed on reading) or holding a
character XML 1.0 cannot carry. One loss the writer cannot help:
`SurveySymbol` and `SurveyTextStyle` keep size, rotation and offset as plain
doubles where 0 means absent, so an explicit `<rotation>0</rotation>` goes
out as no element; telling them apart needs `std::optional` in the model.

**No front end calls either writer yet.** There is no export verb or menu
item; the decision has its writer but not its button.

## Drawing it

The viewport, the plot and every preview ask one resolver
(`cad/style_resolver.hpp`; `docs/cad.md`, "What a style draws"):

- a point whose style names a symbol is drawn with the library definition of
  that name, else with the built-in shape of that name, else with the shape
  the name suggests (`entity::builtInSymbolFor`), at the style's size and
  UNROTATED: a mapfile symbol's `rotation`, `offset` and `raise` are read into
  `SurveySymbol` and not yet applied, and `Style` has no rotation field to
  carry one (a schema change, deferred under D6);
- a line whose style's `linetype` names a library linestyle is drawn BY that
  definition - its strokes replace the line, with no Katana dash applied to
  them, because a 12d linestyle is the line, gaps and all (the next section
  says why; this list used to say the definition was laid "in addition to
  the line itself", which is the bug that section records). A model
  `Linetype` of that name is used only when the library holds no non-vertex
  definition of it, and a name neither holds is a solid line (decision D2);
- a line whose style names a symbol has that symbol at EVERY vertex (decision
  D8, reversing an earlier "not drawn, by decision"), as 12d draws fence
  posts; the symbol's name is never also laid as a pattern along the line, so
  a style whose linetype names its own symbol - what the 12da import writes
  for a symbol string - is a plain line with the symbols on it;
- a `colour` command inside a definition changes the pen for the strokes that
  follow it, `view_colour` means the entity's own colour, and on paper a white
  12d pen prints black (decision D7, `cad::paperColour`).

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
is now drawn only when no definition applies, when the entity carries a
hatch (which is painted inside the same call, and which a 12da never brings),
or when the pattern is not laid: a period under two pixels on screen
(`TooFine` - the repeats would merge into a smudge the colour of the line
anyway) or more repeats than the budget (`OverBudget`).

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
field code, and this turns the mapfile's answer into the layer, the style and
the properties it should have.

Where the code is, is FOUND when nobody names it: the first of
`codePropertyCandidates()` - `code`, `12d.name`, `Code`, `CODE`,
`feature_code` - that any entity carries as text, and the report names the one
used. A survey file's points carry `code`; a 12da import records a string's
name, which in a coded survey IS its code, as provenance metadata
(`12d.name`), so `surveyCodeOf` asks an entity's properties first and its
metadata after. Asking the caller which would be asking them to know how the
drawing got here.

It PLANS rather than acts: everything comes back as one
`commands::Transaction`, so twenty thousand coded points are **one undo**, not
twenty thousand. A command per entity would make an undo stack nobody could
use.

Decisions worth recording:

- **12d's model becomes Katana's layer.** Both are `/`-separated paths naming
  where something lives, and it is the mapping the 12da import already makes.
- **A code's style is chosen by its APPEARANCE (decision D4).** What a code
  looks like is four things: its linestyle (or a plain line), its symbol, the
  symbol's size and its colour. This used to be "the style named after the
  12d linestyle", and in the reference mapfile every symbol code also says
  linestyle `0`, which comes with 18 different colours - so every one of
  those codes landed on ONE style `0`, and whichever entity came first
  decided its symbol and colour for all of them. Now (`survey_coding.cpp`,
  `Appearance`, `drawsAs`, `existingStyleFor`, `nameFor`):
  1. an existing style that DRAWS exactly that appearance is REUSED, whatever
     it is called - so renaming a coded style and applying codes again makes
     no second one. Of the styles that draw it, one whose description is the
     appearance's colour name comes first (the style an earlier pass made for
     it), then the first in name order. A colour name the colour table does
     not know has no RGB, so any style with no colour of its own draws it -
     the 12da import's `0` included - but one described by ANOTHER such name
     of the map is left to that name's codes: otherwise a code coded in a
     later pass took a neighbour's style (WR* onto WM*'s `WATR Main`) where a
     fresh drawing gives it its own. That set of names comes from the map's
     rules, not from the entities coded, so the choice does not depend on
     what was selected;
  2. otherwise a style is created and named after the linestyle, else the
     symbol, else `Plain`; a name another appearance already holds gets
     ` (<colour name>)` and then ` 2`, ` 3`. The appearances are named in a
     fixed order (linestyle, symbol, size, then the colour as RGB hex, or the
     name where the RGB is unknown), so the names follow from the rules and
     never from which entity came first: of two plain text codes, cyan
     (#00FFFF) takes `Plain` and yellow becomes `Plain (yellow)`.
  The colour a code gives goes on the STYLE, and its name into
  `Style::description`, which is how an unknown name is matched again. A
  12da import puts colour on the ENTITY instead, so an imported point's own
  colour hides a coded style's colour; and imported SYMBOL styles (linetype =
  the symbol's name, no colour) are not reused for a coded symbol appearance.
  Both are open.
- **Options that say "no" are honoured one at a time** (audit CAD-18).
  `createLayers` off: an entity whose rule names a layer the drawing lacks
  keeps its layer and still gets its style and attributes; it is counted in
  `skippedNoLayer` and in its code's row (`SurveyCodeRow::layerKept`), so the
  row never reads as a move that did not happen. `createStyles` off: an
  appearance no existing style has leaves the style alone (`skippedNoStyle`).
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
  -c 'POINT 0,0' -c 'SELECT ALL' -c 'PROP SET code WM01 text' -c 'CODE' -c 'LIST'

1 entity carries a code in "code": 1 matched, 0 fallback-only (only the bare * rule answers), 0 with no rule
1 entity changed
Layers created: SURVEY SERVICES
Styles created: WATR Main
By code:
  WM01: 1 entity, prefix, matched; layer 0 -> SURVEY SERVICES; style WATR Main (created); set DepthLocation, Depth Location; 1 changed
Applied as one command. UNDO puts it all back.
1  Point  layer=SURVEY SERVICES  at 0,0
```

(With the compiled-in customisation, so no CUSTOMISE is needed. The
`DepthLocation` and `Depth Location` attributes come from the bare `*` pipe
rules every code meets; see "Asking the map why".)

## Asking the map why: explain, list, census, lint

A person looking at a point drawn wrongly has one question - why did this
code get that? - and until now nothing could answer it. `include/katana/cad/code_table.hpp`
holds the answers, below both front ends, each a structured result plus ONE
formatter, so the application and `katana_cli` print the same words (the two
used to disagree about what applying codes had done; `formatCodingReport` is
now the one report of an application, with a row per code):

| Question | Function | CLI verb |
|---|---|---|
| Why does this code get what it gets? | `explainCode` → `formatCodeExplanation` | `CODE EXPLAIN <code>` |
| What does the map say, one code per line? | `codeTable`, `codeTableRowMatches` → `formatCodeTable` | `MAPFILE LIST [<filter>]` |
| Which codes does this drawing carry? | `codeCensus` → `formatCodeCensus` | `CODE CENSUS [<property>]` |
| What is wrong with the map before it is applied? | `lintSurveyMap`, `lintSurveyRule` → `formatLint` | `MAPFILE CHECK` |

**An explanation cites rules by index**, the only identity a rule has. For
each field it names the FIRST rule, most specific first, that says anything
about it - the rule `lookup` takes it from - and the later matching rules that
set it to something else and lost (`CodeFieldSource::overruled`): most often
the second of two loaded mapfiles disagreeing, and the built-in pair disagree
about `hide` for 190 keys. It says what each linestyle and symbol name
resolves to, what the colour name's RGB is (or that it has none, which leaves
an entity's colour alone), which attributes accumulate and which are
deferred (`$PipeDiameter`), how the code matched (D5) and any near miss by
case or blanks.

**The table** has one row per distinct key, and the row shows what a code that
key catches resolves to with the less specific keys included - for `WM*` that
is `WM*`, then `W*`, then `*`. The filter is a substring of key, comment,
group, model, colour, linestyle or symbol, case folded (D3).

**The census** counts every distinct code the drawing carries, found as
`applySurveyCodes` finds it, and classes each against the loaded map; each
distinct code is looked up once.

**The lint** has two severities: an ERROR is a rule that cannot be applied as
written, a WARNING one that applies but not as its author meant.

| Kind | Severity | Meaning |
|---|---|---|
| `InvalidLayerPath` | error | a model that cannot become a layer |
| `KeyWhitespace` | error | a key with surrounding blanks, which no typed code matches |
| `UnresolvedLinestyle` | warning | a linestyle no loaded library defines |
| `UnresolvedSymbol` | warning | a symbol no library defines and Katana cannot draw |
| `SymbolNotSymbolCapable` | warning | a symbol rule naming a definition known not to be a symbol (D3; one read from no known file gets the benefit of the doubt) |
| `LinestyleIsVertex` | warning | a linestyle naming a `mode vertex` definition |
| `UnknownColour` | warning | a colour name the colour table does not know |
| `NoModel` | warning | a `map_data` rule that puts its code nowhere |
| `DuplicateRule` | warning | the same as an earlier rule, field for field |
| `ShadowedRule` | warning | earlier rules of its key already say all it says |

`SurveyMap::add` refuses a key with blanks and an invalid layer path, so those
two can be met only by `lintSurveyRule` on a rule not yet in a map - an
editor's form before it commits. `MAPFILE CHECK` fails the command when any
error is found, so a script stops. On the compiled-in pair of mapfiles, run
on 2026-09-24:

```
1624 rules checked: 0 errors, 944 warnings
  by kind: 1 unresolved linestyle, 4 unresolved symbol, 3 linestyle is a symbol,
  233 unknown colour, 446 duplicate rule, 257 shadowed rule
```

The 233 are 12d's `sui ...`, `pen NNN` and `off yellow` names, which
`archive12d::standardColour` does not know; such a code leaves the colour
unset and, since D4, reuses any colourless style that draws alike. Each
mapfile checked alone has NO duplicates, and names.4d no shadowed rule; the
detail mapfile alone has 44 shadowed rules, which are its `*` pipe rows -
about 15 of each pipe section's 16 - because Katana does not model those rows
as conditions on an attribute, which is what they very likely are. So all 446
duplicates and the other 213 shadowed rules come from loading the second
mapfile on top of the first, which says much of it again. All of it is true
of how Katana will apply the map, which is what a lint is for.

The explanation of a code with a near miss, abridged:

```
katana_cli -c 'CODE EXPLAIN wm01'
Code "wm01": only the bare * rule answers it: fallback-only, not matched
  string attribute DepthLocation = Top of Pipe  <- rule #664 * (pipe_data)
  ...
  near miss: key "WM*" differs only in letter case
```

## `breakline point` means the vertices ARE points

Not a display hint and not only a TIN flag: a string marked `breakline point`
is a set of separate survey shots that 12d keeps in one string for
convenience. It is the rare case - in a production archive of 25,659 strings,
12,280 say `line` and 13,379 say `point`, and of those only **53** have more
than one vertex.

Those 53 were the ones that looked wrong: one is a 61-vertex string of drill
holes in a `SURVEY INFRASTRUCTURE V2` model, and Katana drew a polyline
through them. The import now gives each vertex its own point entity, carrying
the string's layer, style, colour, metadata and its own height, and says how
many it made.

**The trap, which cost a first attempt.** The format has a CURRENT breakline
type and its default is `point` (commands, 1.4.4). Every string in a real
archive states the flag - that production file has no file-level `breakline`
command at all and leaves nothing to the default - but a hand-written fixture
does not, so 21 test fixtures that plainly meant lines became points. The
fixtures now say `breakline line` once at file level, which is the format's
own way of saying it, and a fixture wanting points overrides it per string.

## A vertex symbol goes on every vertex

A 12d `symbol_value` block on a string puts that symbol on EVERY vertex -
that is what `mode vertex` means in the library. Katana used to take the
symbol onto the style only for a string of ONE vertex, so that drill-hole
string drew no symbols at all where 12d draws 61.

A string with one symbol block now takes it whatever its length. That needed
the exporter to change too: the block built from a style was written only
inside its `PointGeometry` branch, so a line's symbol was written back from
`12d.symbol.*` metadata instead - and once the symbol lived on the style
there was no metadata to write. `setSymbolFromStyle` is now a member both use,
and it writes the style's `symbol` - the 12d name - rather than the style's
own name, which a person may have changed since the import.

It is also DRAWN now: a line whose style names a symbol has it at every
vertex (decision D8, `cad::symbolVertices`), where the viewport used to skip
symbols on lines "by decision". Only a string carrying a DIFFERENT symbol per
vertex is still kept, written back and not drawn - there is no one symbol for
its style to carry - and the import's warning says so.

## Linework: joining coded points into lines

A surveyor walking a kerb shoots `KB1`, `KB1`, `KB1` and expects a line, not
three dots. The mapfile says WHICH codes are lines (`SurveyRule::breakline`)
and how they look; `include/katana/cad/linework.hpp` is what joins them - what
Civil 3D calls Process Linework, TBC Process Feature Codes and Carlson Field
to Finish. There are two entry points, and a set of points goes through ONE of
them, never both, or a kerb the file strung and whose points are also coded
`KB1` is drawn twice:

- **`processLinework(document, LineworkOptions)`** strings the drawing's
  coded points itself;
- **`drawSurveyFeatures(document, project, SurveyFeatureOptions)`** draws the
  lines a field file strung for itself (a LandXML `PlanFeature`, a
  controller's line record: `survey::SurveyFeature`), taking positions from the
  project's coordinates rather than from point entities, so it works whether
  or not the points were imported. It draws a feature whatever its code's
  breakline, because the file has said the points are one line.

Either returns ONE command - layers, lines, their styling and, when asked,
the removal of the points - and a report. The lines are styled by calling
`applySurveyCodes` on them inside the same transaction when the command runs,
not by a second copy of its naming, so a line coded `WM01` wears exactly the
style a point coded `WM01` would. Two consequences: `LineworkReport::styling`
is filled only once the command has run, and the command refuses to run on
any other Document.

**Which points form one string.** The first token of a point's code is its
STRING NAME: every point named `WM01` is one water main and `WM02` another,
and `WM01` and `WM1` are two strings, as they are in the field.
`splitStringName` reports how a name divides into the key of its most
specific rule and a string number (`WM01` under `WM*` is key `WM*`, number
`01`; an exact key is the whole name, with no number; a name only `*` answers
is fallback-only, D5) - but the split is reported, not used for grouping.
String names and keys keep their case. A point is a candidate when its code
resolves to breakline Line or it carries a control code. Points are ordered by
their point number, numerically where it is a number ("9" before "10"), or by
entity id (`LineworkOrder::EntityOrder`, the order the file listed them); a
point with no number is not placed when ordering by number, and is reported.

**Control codes are data, not 12d mapfile data.** A mapfile has no start, end
or close codes - 12d strings by name alone - so what `ST` means is in no file
Katana reads. `LineworkCodes` holds the spellings, and the defaults are common
field conventions, copied from no product's file:

| Default | Control | Meaning |
|---|---|---|
| `ST` | start | a new line of this string begins here |
| `END` | end | the line ends here |
| `CL` | close | the line ends here and closes |
| `BC` / `EC` | curve start / end | the points between are on arcs |
| `JPN <n>` | join | also draw a line from here to point number `n` |
| `RECT` | rectangle | three points make a rectangle |

Tokens after the name are matched ignoring ASCII case - they are keypad
keywords, and `st` means `ST` - and the first token is always the name, so a
code `CL` is a string called CL. An empty spelling switches a control off;
`validate(LineworkCodes)` refuses a spelling with a blank and two controls
spelled alike. The codes live only in `LineworkOptions` for now: a survey-code
manager will need somewhere to keep and edit them, and session data on the
Document, like the map (D1), would fit.

**Curves are chorded.** `Polyline2` has no arc segment, and giving it one is a
schema change deferred under D6. So each consecutive three curve points
define an arc - (p0,p1,p2), (p2,p3,p4), ... - densified until the gap between
chord and arc is at most `chordTolerance`, 5 mm by default: below the
accuracy of a detail survey, and about 25 chords a quarter turn at a 10 m
radius. Every surveyed point stays a vertex; heights between them are
interpolated along the arc and set through `entity::setHeights`, the writer
the 12d and survey imports share, and a point with no height gives its vertex
none. `RECT` on three points p0, p1, p2 makes the closed rectangle with side
p0-p1 reaching p2's side by p2's distance from it; its two constructed corners
have no height, since nobody surveyed them.

**Nothing is dropped silently.** Every point not in a line is in
`LineworkReport::unplaced` with a reason (no code, no rule, a point code, no
point number, alone in its string, all its string in one place), and every
token not understood is a note with the point it was on. The notes say what
was DRAWN, not only what was wrong: a `RECT` on other than three points says
"drawn closed through them" or "drawn open"; a curve never ended says it was
taken to the string's last point, or that a `BC` on the last point curved
nothing; a curve through collinear points is drawn straight and not counted in
`LineworkString::curves`.

**`keepPoints`.** On by default. Off, the points a run of their OWN string
went into are deleted in the same command - the line now stands for them -
and nothing else is: a point in no line always stays, and so does a point
only a `JPN` join reached (a tree or an uncoded control point joined to is
not replaced by the join line). `pointsRemoved` counts only what was deleted.

**`drawSurveyFeatures` and the code property.** Pass it the options the
import was given: the line goes on its code's layer, or with its points when
the code names none. When the import wrote no code (`codeProperty` empty),
each line still gets its code, under `codePropertyCandidates().front()`
(`code`), and `LineworkReport::property` names it - a line is styled by the
code it carries. `coding.createLayers` governs every layer here, as in
`processLinework`; `import.createLayers` is not read.

Not yet reachable from either front end: there is no command, menu item or
dialog for Process Linework or Draw Survey Features, and the survey import
wizard does not call `drawSurveyFeatures`. When `applySurveyCodes` styles
POINTS it still reads the whole code field, so an exact-key point with a
control token (`PABB ST`) is unmatched there; lines are unaffected, since they
carry only the string name.

## Saying whether it is working

"The linestyles are not showing" has several causes that look identical: no
customisation loaded, a customisation that does not define what this drawing
names, or a drawing whose styles are 12d's plain lines `0` and `1`. Loading
one now reports which:

```
4 of this drawing's 4 styles are drawn with a loaded definition
  (4 name one; the rest are 12d's plain lines).
```

`cad::customisationCoverage` is the one place that counts it. Two kinds of
name are not "missing" there, because they draw correctly with no library: a
symbol Katana draws itself (`cross`, `manhole`), counted in `builtIn` rather
than listed as unresolved (audit CAD-17), and a Style linetype of `ByLayer`,
which names no definition at all - it takes the layer's (decision D2). The
second was found only when the two branches met: the styles branch made
`ByLayer` legal and the coverage, in another branch, listed it as "in no
loaded library" until the integration commit taught it otherwise.
`cad::missingNames` (`style_catalogue.hpp`) is the managers' list of the same
thing for every Style and Layer name, with who uses each.

## The customisation is part of the program

Not a file to load, not a setting to point somewhere: the linestyles, symbols
and survey codes are **compiled into the binary**. Every drawing has them the
moment it is opened, on any machine, with nothing configured.

`tools/embed_customisation.py` runs at build time and turns each customisation
file into a byte array in a generated source; `archive12d::builtinCustomisation`
parses them once, lazily, and keeps the result. The front ends seed the
Document from it at startup.

A built-in file that cannot be read costs ONLY ITSELF:
`readEachCustomisationFile` names it in `Customisation::errors` and keeps what
the files before and after it brought, where one damaged file used to leave
every drawing on plain lines without a word (audit A12-06). A person loading
files still gets `readCustomisation`, which fails whole so they can be told.
A12-06 stays open until the front ends LOG `builtinCustomisation().errors`
and `.warnings`; today neither does.

Two decisions worth recording:

**The BYTES are embedded, not generated C++ structures.** Generating the 792
definitions and 35,000 strokes as brace-initialised structs would skip the
parse, but it is a multi-megabyte translation unit that costs more to compile
than the parse costs to run - measured at **21 ms in Release** for the whole
customisation, once per session, and that figure includes reading four files
from disk, which the built-in does not do. It also means one reader: a
built-in customisation and a loaded one go through exactly the same parser,
so there is no second way for the two to disagree.

**The generated source is never committed.** The customisation is third-party
material under its own licence and is not in this repository, so a checkout
without it generates an EMPTY table and Katana draws plain lines - exactly as
12d does without one. Drop a customisation into `docs/12d Refrence Files`,
then RE-RUN CMake (`cmake -S . -B build/release`) and rebuild to have it
compiled in: the file list is a configure-time glob, so a plain rebuild does not
see a new file.

`File > Load 12d Customisation...` still overrides what is built in, which is
how a site tries a new library without reissuing the application.
