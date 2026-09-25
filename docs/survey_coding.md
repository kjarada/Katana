# Survey coding: survey code files, linestyles and symbols

`PLAN.MD` 20.3 (the plan has since been removed; its section numbers survive
here as history). How a surveyor's field code becomes a drawing, and the
decisions of 2026-09-23 (D1-D9, tabled in `docs/cad.md`) that govern it.

A surveyor shoots a point and types a code: `WM01`. That code is not a label -
it is an instruction. A customisation turns it into a water main: on layer
`SURVEY SERVICES` (the rule's `model` field), coloured `sui water potable`,
joined into a line rather than left as a point, drawn with the `WATR Main`
linestyle. Other codes get a symbol instead: `AC*` puts a `CULT Bollard` at each point.

Three files do that, and they are useless apart:

| File | What it says |
|---|---|
| a **survey code file** (`.mapfile`, XML; "mapfile" below where the format is meant) | what each code becomes: layer (its `model`), colour, linestyle, weight, whether it is a line or a point, which symbol, what attributes |
| a **linestyle library** (`.4d`) | what each linestyle is drawn with |
| a **symbol library** (`.4d`) | what each symbol is drawn with |

The work was built against a real production customisation from a road
authority - 728 survey code rules, 322 linestyles, 474 symbols, and a second
survey code file of 900 more rules. **It is third-party material under its own
licence and is NOT part of this repository.** It is kept, under general file
names, in the git-ignored `resources/customisation/`, which the build compiles
in ("Where the built-in customisation comes from" below); every test that
touches it skips when it is absent, so the suite stays green without it.

## A symbol is a linestyle

This is the fact the whole design rests on, and it is not a simplification:
the file format's manual says it where it describes vertex symbols - "There can be the
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
474; and of the 193 symbols `survey_codes.mapfile` names that a library defines,
only **48** are `mode vertex` - every one of the 193 comes from the symbol
library. A symbol browser built on `atVertices` would have hidden three
quarters of the symbols the mapfile actually uses. So the rule (the lead's
decision D3, `cad::classifyDefinition` in `include/katana/cad/style_catalogue.hpp`)
is that a definition is offered as a SYMBOL when any of these holds:

- it is `mode vertex`;
- a `vertex_symbol_data` rule of the loaded mapfile names it;
- some `Style::symbol` names it;
- the file it was read from has "symbol" in its name, in any case - the way
  symbol libraries are named (the built-in one is `symbols.4d`), which is why
  every definition now carries
  `LineStyle::source`, the NAME of its file (never a path).

It is offered as a LINESTYLE when it is not `mode vertex`, so a definition
can be both, and `DefinitionKind` keeps each reason so a browser can say why
a definition is listed where it is. The counts both front ends print are D3's
too: CUSTOMISE says "792 linestyle and symbol definitions in 71 groups: 475
offered as symbols, 635 as linestyles (one definition can be both)", and the
window's log "792 definitions (475 symbols)", counting the library entries
`cad::symbolChoices` offers. They used to count `mode vertex` alone - "157 of
them symbols" - which is not what a symbol is.

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
| `archive12d::readStyleLibrary` | `archive12d` | it already owns the archive reader's encoding detection, its lexer and its colour names |

**A library is not part of a Model and is not saved inside a project.** That
is how a survey customisation works - the libraries are shared site-wide by every
project, named by a project rather than copied into it. The alternative, a
table in the Model beside Layer and Linetype, was rejected for two reasons:
one production customisation alone is 792 definitions and 35,000 strokes,
which would be written into every project file that used one of them; and two
projects would then be able to disagree about what "WATR Main" looks like.

`StyleLibrary` is `NamedTable`, the same container as every other table of
named things, rather than a new one.

## Names compare with regard to case, and that is a decision

The file format's own comparisons are not case-sensitive - its overview says
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
| 177 distinct `<linestyle>` values | 175 | `0` and `1`, the names of the plain continuous line |
| 195 distinct symbol `<style>` values | 193 | `Circle Single` and `SBEND`, from a standard library that is not one of these files |

So a customisation is not necessarily self-contained, and an unresolved name
has to be reported rather than treated as a fault in the file.

## The mapfile is written in sections, and the section is the meaning

This is the thing that is easy to get wrong, and the first version of the
reader did get it wrong. `<map_file>` does not hold one list of rules. It
holds up to ten SECTIONS, and which section a rule is in is what says what the
rule is about:

| Section | What its rules say | `survey_codes.mapfile` | `survey_codes_names.mapfile` |
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

Reading only `<map_data>` takes 457 of `survey_codes.mapfile`'s 725 rules and
quietly loses every symbol - which is exactly what the first version did, and
exactly what the test against the real file caught. The element name inside a
rule is not enough either: `<map_attributes>` means attributes on the STRING
inside `string_attribute_data` and attributes on each VERTEX inside
`vertex_attribute_data`. That is why `SurveyRule` records the section it came
from.

`survey_codes_names.mapfile` is a second survey code file, and it came with
the `.4d` extension a linestyle library also uses; it is kept under a name
that says what it is, but that is not what makes it one. What makes a file a
survey code file is that it contains a `<map_file>` element, not what it is
called, so a `.4d` file holding one is still read as one.

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
| `survey_codes.mapfile` | 725 over 465 distinct keys | none |
| `survey_codes_names.mapfile` | 899 | one, and it is right: an `<item>` holding only a `<group>`, which names no code and so could never apply |

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

**A two-point style is scaled equally in both axes.** The file's `stretch_mode`
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
"Writing it back" below - not by saving the project. The Survey Code Manager
is that editor for the map ("The Survey Code Manager" below). The library has
no editing buffer: the symbol library loads into it (merged) and exports from
it, and the style manager edits the drawing's styles and linetypes, which are
commands like any other.

## Loading a customisation

`archive12d::readCustomisation` takes a list of files and works out what each
one is BY LOOKING INSIDE IT. That is not fastidiousness: of the four files
this was built against, two are survey code files and two are style
libraries - and as delivered THREE of the four ended in `.4d`, so that
extension was two different formats in one folder and a loader that went by
the name would read half the customisation as the wrong thing. Kept in
`resources/customisation/`, the second survey code file ends in `.mapfile`;
the loader still asks each file what it is.

It lives beside the readers rather than in a front end because both front ends
need it, and it needs no third-party library, so it costs `archive12d` nothing
of the property that lets it build with `-DKATANA_BUILD_IO=OFF`.

Loading the real customisation, from the command line (each long list of
names cut short here):

```
R=resources/customisation
katana_cli -c "CUSTOMISE REPLACE $R/linestyles.4d $R/survey_codes.mapfile $R/survey_codes_names.mapfile $R/symbols.4d" -c CUSTOMISE
warning: the built-in customisation: survey_codes_names.mapfile: an <item> of <map_data> has no <key> and was skipped
Customisation: 792 linestyles and symbols and 1624 survey code rules, built in
  warning: survey_codes_names.mapfile: an <item> of <map_data> has no <key> and was skipped
  linestyles.4d: style library, 0 added, 318 replaced: "BARR Bollard", ... and 310 more
  survey_codes.mapfile: survey code file, 0 added, 680 replaced (codes, once for each section): "1*", ... and 672 more
  survey_codes_names.mapfile: survey code file, 0 added, 899 replaced (codes, once for each section): "1*", ... and 891 more
  symbols.4d: style library, 0 added, 474 replaced: "Accepted For Construction", ... and 466 more
Loaded now: 792 definitions, 1624 survey code rules
  3 names the survey codes ask for that no loaded library defines: "Circle Single", "LNMK Dividing - Separation Line S2 Multi Lane", "SBEND"
This drawing has no styles yet; import a drawing or survey that carries styles, or make one in Format > Styles and Linetypes or with STYLE NEW, to see the customisation take effect.
792 linestyle and symbol definitions in 71 groups: 475 offered as symbols, 635 as linestyles (one definition can be both)
1624 survey code rules over 632 distinct codes
This drawing has no styles yet; import a drawing or survey that carries styles, or make one in Format > Styles and Linetypes or with STYLE NEW, to see the customisation take effect.
```

(Run on 2026-09-24 by `katana_cli` built from the merged branches, with the
reference customisation compiled in from `resources/customisation/` - which is
why every file only REPLACES what the built-in already holds, and why the
built-in's one warning is said at start-up. The files load in the order they
are named, here their name order, as the built-in's do. A file's line counts
the definitions it brought, or for a survey code file its codes once for each
section they appear in, and names the first eight.
The plain lines `0` and `1` and the built-in symbol names are not listed as
missing: they draw without a library.)

In the application it is **Format > Load Customisation...**, and **Format >
Replace Loaded Customisation...** for a replace - the same two actions are on
Survey > Survey Coding - or `CUSTOMISE [REPLACE] <file>...` typed on its
command line, or `katana --customise <file>...` at start-up. Each takes
several files at once and writes the same report into the command log, ending
with what the loaded customisation means for the drawing open
("Saying whether it is working" below).

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

**Both front ends load through it** (audit QT-21, fixed). The window's
`MainWindow::applyCustomisation` - behind Format > Load and Replace, the typed
`CUSTOMISE` and `--customise` - and `katana_cli`'s `CUSTOMISE [REPLACE]
<file>...` (`runCustomise`, `src/katana_app/session.cpp`) read the files, call
`mergeCustomisation` with the library and map loaded now and what the files
brought, install both results
(a kind the load did not bring comes back as it was, so a symbol file alone
keeps the map) and print its `files`, one line each: the definitions or codes
it added and replaced. A Replace also says what went (`removedDefinitions`,
`removedKeys`), and a definition or rule that could not be installed is an
error line of its own. The load then names what the mapfile asks for that no
loaded library defines, judged against everything loaded, since a mapfile may
name what an earlier load defined. REPLACE is a replace only as the unquoted
first word, in any case - a quoted path that begins with "replace" is a path.
In the window a Replace is an action of its own, never a question, so a
headless run never meets a box.

**A file named twice in one load is read once**, in both front ends
(`cad::distinctCustomisationFiles`): read twice, every rule of it would sit in
the map twice, since both copies are the load's and the merge keeps them all.
Each later naming is said - "... is named twice in this load; it is read
once". Two namings are one file when their absolute, lexically normal paths
are equal or `std::filesystem::equivalent` says so; a pair it cannot tell
apart is read twice, which doubles rules but never drops a file.

**The built-in's faults are said** (audit A12-06, fixed). At start-up each
front end logs `builtinCustomisation()`'s errors and warnings once - the
second survey code file, `survey_codes_names.mapfile`, has one `map_data` item
with no key, so every start says so, the CLI on stderr - and then what it installed: "Customisation: 792
definitions (475 symbols) and 1,624 survey code rules, built in." in the
window's log, the numbers grouped in the user's locale.

### What the project records

`storage::ProjectMetadata::customisation` holds the names of the files a
drawing was drawn with, in load order - a record, not a reference: nothing is
loaded from it, and a name need not exist where the project is opened. It is
stored as one `customisation` metadata key, the names separated by line feeds
(a Windows file name may hold `;` but not a line break), and `save()` refuses
a name that is empty or holds a line break or a path separator. See
`docs/model.md` for the metadata keys an older build keeps for a newer one.

Both front ends fill it through `include/katana/cad/customisation_record.hpp`,
since `cad` cannot see `archive12d` and a file reaches it as a name and a kind
(`CustomisationSource`):

- **Every load is recorded** (`recordCustomisationLoad`), the built-in's files
  first. A file loaded again moves to the end; a Replace of a kind drops the
  earlier files of that kind, which no longer contribute.
- **A save writes the record** (`customisationRecordToSave`): the loaded files
  in load order - a library file only while some definition still comes from
  it - then any definition's `LineStyle::source` no loaded file accounts for,
  then the names the project already recorded that its OPEN found missing and
  no load has brought since (`noteCustomisationLoaded`). This session cannot
  judge those, so saving must not forget them, or every later open anywhere
  would draw plain lines without a word. File > Save and Save As write it
  (`MainWindow::recordCustomisation`); a typed `SAVE` writes it only when it
  can save (`typedSaveHasDestination`: a directory, or none when the drawing
  already has a project), because writing the metadata marks the drawing
  modified and a `SAVE` that could not go ahead would leave a drawing nobody
  touched asking to be saved.
- **An open warns** (`customisationNotLoaded`): the recorded names that are
  not loaded now, in the project's order, compared exactly. The window logs
  "Warning: this project was drawn with customisation files that are not
  loaded: ... Load them with Format > Load Customisation..." and the CLI
  prints the same warning on stderr. Not an error: the drawing opens and
  draws, and what a missing file defined draws as a plain line until it is
  loaded.

**The built-in's files, and 29 of its definitions, have new names**
(2026-09-24). The files were given the general names they have now, and 29
definitions - 4 linestyles and 25 symbols, scale bars and north points among
them - lost the publisher's word from the front of their names. A project
saved before records the earlier file names, and its styles can name an
earlier definition. Both are known to `include/katana/cad/customisation_record.hpp`
only by hash (`sourceNameHash`, FNV-1a), so no earlier name is spelt in the
repository:

- **An earlier file name is answered by the file that took its place**
  (`builtinRenames`, `RenamedSource`): `customisationNotLoaded` reports it
  missing only when that file is, so such a project opens without a warning
  nobody could clear, and `noteCustomisationLoaded` - through the same helper,
  so the two cannot disagree - clears it when a load brings the file under its
  new name; the save then records the new names. One earlier name was a plain
  one a person's own file could have (`RenamedSource::distinctive` false): it
  is answered only in a record that also holds a distinctive earlier name of
  the same set, so recorded alone, or beside the new names, it is that
  person's file - reported missing and kept by the save.
- **An earlier definition name** is given its current one by
  `definitionNameNow` (`builtinDefinitionRenames`), asked only after an exact
  lookup missed, so a person's own definition that still has such a name is
  the one found. Every drawing lookup asks it through `findDefinition`
  (`style_resolver.cpp`): `resolveLinetype`, `resolveSymbol` and
  `DefinitionCache::find`, so a style saved naming an earlier definition
  draws that definition, on screen, on the plot and in every preview.

An older build opening a project this one saved warns about the four general
names; nothing on this side can change that.

Not done: a save that has somewhere to go but still fails (an I/O error, a
directory `ProjectStore::create` refuses) leaves the record written and the
drawing marked modified, because `Document::setMetadata` cannot be taken
back; `Document::save` taking the metadata and committing it only on success
would fix it. A library file loaded whose every definition a later file
replaced counts as not loaded, so opening names it - a warning in excess, not
one missed.

## Writing it back

Under D1 an edited library or map is kept by writing it to a file, so the two
readers now have inverses in `archive12d`, each tested as one: read what was
written and get back what was given (`tests/archive12d/customisation/`).

**`writeStyleLibrary(library, StyleLibraryWriteOptions)`** gives `.4d` text,
UTF-8, in name order, one command per line as the reference libraries are written. Every field is
written - the three kinds, `mode vertex`, group, length, factor, origins,
anchors, stretch and cycle mode, pens, every stroke, and a text's three
numbers kept but not understood - so `readStyleLibrary(writeStyleLibrary(l))`
is `l`. The one thing a file cannot hold is `LineStyle::source`, which is the
file's own name and is stamped again when it is read. `names` picks the
definitions to write, and a name the library lacks fails the write rather than
leaving a file the person believes holds it; `comments` become the `//` head
where a published library carries its licence.

**`writeMapFile(map, MapFileWriteOptions)`** gives the mapfile XML as UTF-8
text; the reference files are UTF-16LE with a byte order mark, which
`core::encodeUtf16LittleEndian` makes of it, and the reader takes either.
Sections go in the reference files' order (`map_data`, `vertex_symbol_data`, `tinable_data`,
`vertex_textstyle_data`, the three pipe sections, the two attribute
sections), and within a section the rules keep their order. The subtle part
is the rule this file's "How a code resolves" rests on: among rules of ONE
key, the earlier wins every field more than one section fills. A map made by
loading two mapfiles, or by merging a load into the current map, can have a
key whose rules are not in that section order, and regrouping them would
change what the code resolves to. So a rule is never written ahead of an
earlier rule of its own key: where that order would put it there, the
sections are written again, in the same order, for the rules that must
follow, and the reader reads a repeated section in turn. Rules of different
keys may be regrouped, which no code can tell. A map in that order comes back
rule for rule with each section written once; exporting the compiled-in pair
of mapfiles gives one extra `map_data` and one extra `vertex_symbol_data`.
Whether other programs read a section that appears twice is not known here.
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

**Where the writers are reached.** The Survey Code Manager's Export Code
File... writes its BUFFER with `writeMapFile`, as UTF-16LE with a byte order
mark, as the reference files are written, and Export Code List CSV... writes
`cad::codeListCsv` (RFC 4180, UTF-8 with no byte order mark, so Excel may
misread a name that is not ASCII). The symbol library's Export Selected to
.4d... writes the selected library definitions with `writeStyleLibrary`.
`katana_cli` has no export verb for either.

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
  them, because a library linestyle is the line, gaps and all (the next section
  says why; this list used to say the definition was laid "in addition to
  the line itself", which is the bug that section records). A model
  `Linetype` of that name is used only when the library holds no non-vertex
  definition of it, and a name neither holds is a solid line (decision D2);
- a line whose style names a symbol has that symbol at EVERY vertex (decision
  D8, reversing an earlier "not drawn, by decision"), as a survey drawing
  shows fence posts; the symbol's name is never also laid as a pattern along the line, so
  a style whose linetype names its own symbol - what the 12da import writes
  for a symbol string - is a plain line with the symbols on it;
- a `colour` command inside a definition changes the pen for the strokes that
  follow it, `view_colour` means the entity's own colour, and on paper a white
  library pen prints black (decision D7, `cad::paperColour`).

## A library linestyle IS the line

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
| set `Style::name` to the archive's linestyle name but left `Style::linetype` as `"continuous"` | the name reached nowhere a renderer looks |
| stored `builtInSymbolFor(name)` instead of the name - "SEWR Manhole Cover" became "manhole" | the 473 symbols in a loaded library could never be matched |
| put a survey code nowhere called "code" - a 12da carries it as the string NAME | `CODE` found nothing to apply |

All three are fixed: the import keeps the REAL names the archive gives, the guess at a
built-in shape happens at draw time only when nothing defines the name, and
the code property is found rather than assumed.

Measured before assuming this would help: of 82 distinct linestyle names in
one real archive, 79 are in the reference library; 214 of 217 in another. The
misses are `0`, `1` and the empty name - the plain continuous line's names.

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

- **A rule's model becomes Katana's layer.** Both are `/`-separated paths naming
  where something lives, and it is the mapping the 12da import already makes.
- **A code's style is chosen by its APPEARANCE (decision D4).** What a code
  looks like is four things: its linestyle (or a plain line), its symbol, the
  symbol's size and its colour. This used to be "the style named after the
  library linestyle", and in the reference mapfile every symbol code also says
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
  is what the survey code file says the thing is; it draws as a plain line or mark until a
  library defines it, which is better than dropping the information.
- **`$PipeDiameter` is left alone.** An attribute whose value names another
  attribute cannot be resolved without the survey data the drawing was made
  from. It is counted and reported rather than written literally.
- **Only text is a code.** A number in the code property is a measurement
  someone named badly; treating `1.5` as a field code would file it under
  whatever the rule for `1*` says.
- **The colour comes through a callback.** The standard colour names are known to
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

The 233 are the reference files' `sui ...`, `pen NNN` and `off yellow` names, which
`archive12d::standardColour` does not know; such a code leaves the colour
unset and, since D4, reuses any colourless style that draws alike. Each
survey code file checked alone has NO duplicates, and
`survey_codes_names.mapfile` no shadowed rule; `survey_codes.mapfile` alone
has 44 shadowed rules, which are its `*` pipe rows -
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
is a set of separate survey shots that the archive keeps in one string for
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

An archive's `symbol_value` block on a string puts that symbol on EVERY vertex -
that is what `mode vertex` means in the library. Katana used to take the
symbol onto the style only for a string of ONE vertex, so that drill-hole
string drew no symbols at all where it should show 61.

A string with one symbol block now takes it whatever its length. That needed
the exporter to change too: the block built from a style was written only
inside its `PointGeometry` branch, so a line's symbol was written back from
`12d.symbol.*` metadata instead - and once the symbol lived on the style
there was no metadata to write. `setSymbolFromStyle` is now a member both use,
and it writes the style's `symbol` - the library name - rather than the style's
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

**Control codes are data, not survey code file data.** A survey code file has
no start, end or close codes - it strings by name alone - so what `ST` means
is in no file Katana reads. `LineworkCodes` holds the spellings, and the defaults are common
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
spelled alike. In the application the spellings are SESSION data kept by the
Format workbench (`CustomisationWorkbench::lineworkCodes`), which the Survey
Code Manager's Linework tab shows and edits and every run reads; they start as
the defaults each session and are not remembered between sessions.

**Curves are chorded.** `Polyline2` has no arc segment, and giving it one is a
schema change deferred under D6. So each consecutive three curve points
define an arc - (p0,p1,p2), (p2,p3,p4), ... - densified until the gap between
chord and arc is at most `chordTolerance`, 5 mm by default: below the
accuracy of a detail survey, and about 25 chords a quarter turn at a 10 m
radius. Every surveyed point stays a vertex; heights between them are
interpolated along the arc and set through `entity::setHeights`, the writer
the archive and survey imports share, and a point with no height gives its vertex
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

Process Linework is the Survey Code Manager's Linework tab, previewed before
it runs, against the drawing's map. Draw Survey Features is not reachable from
either front end yet - no command, menu item or dialog - and the survey
import wizard does not call it; nor has `katana_cli` a linework verb.

**A point is coded by its string name.** `applySurveyCodes` looks a POINT up
by the string name of its field code, its linework controls left out
(`parseFieldCode`): `PABB ST` is coded as `PABB`, and reported under `PABB`,
so the styling a point gets and the line it joins agree. It used to read the
whole field, which left an exact-key point with a control token unmatched.
Any other entity's code is still looked up whole.

## The Survey Code Manager

Format > Survey Code Manager... (also on Survey > Survey Coding;
`src/katana_qt/customisation/code_manager*.cpp`) is the editor D1 asked for: the
survey code library a surveyor codes against - Civil 3D's description keys, TBC's feature definitions, Carlson's field-to-finish -
in one non-modal dialog of five tabs, each over one of the cad foundations
above, so the dialog decides nothing the CLI would say differently:

| Tab | Over | Shows and does |
|---|---|---|
| Code Table (`codeTableTab`) | `cad::codeTable`, `explainCode` | a key per row with what it resolves to (layer, colour, line or point, linestyle, symbol, tinable, attributes), filtered; a code typed in `testCode` or a selected key explained field by field - the value, the rule that set it, the rules that lost - with previews of its linestyle and symbol; double-click an explained field for the rule that set it; a rule form by section, with Add, Update, Duplicate, Delete, Up and Down (earlier wins more ties) |
| Codes in Drawing (`codesInDrawingTab`) | `cad::codeCensus` | every distinct code the drawing carries, classed matched, fallback only or unmatched against the BUFFER, so a rule being written shows against the drawing's codes before Apply; select the entities carrying one; start a new rule for an unmatched code keyed by `cad::suggestedKey` |
| Issues (`codeIssuesTab`) | `cad::lintSurveyMap` | the lint of the buffer |
| Apply Codes (`applyCodesTab`) | `cad::applySurveyCodes` | its report as a preview, for the selection or everything, then Execute as one undo step |
| Linework (`lineworkTab`) | `cad::processLinework` | the session's control codes (above) and the options, previewed, then executed as one undo step |

**Edits go to a BUFFER** (D1). The map is session data, not undoable, so the
form's buttons change a copy of the drawing's map through `SurveyMap`'s own
mutators, each refused exactly as `SurveyMap` refuses it, and nothing reaches
the drawing until **Apply** (`setSurveyMap`). **Revert** takes the drawing's
map back. The indicator (`dirtyIndicator`) says which: "No unapplied edits:
these are the drawing's 1624 rules." or "Unapplied edits: Apply puts them on
the drawing, Revert discards them. 1625 rules." A map changed from outside -
a load, a Replace - is taken up at once while the buffer is unedited; with
edits pending the buffer is kept, the log says so, and the edits are measured
against the map the drawing has NOW, so undoing them all makes the buffer
clean again. Apply Codes and Linework run against the DRAWING's map, because
`applySurveyCodes` reads the Document and cannot be handed a buffer; while the
buffer has unapplied edits both tabs say so (`applyDirtyNote`,
`lineworkDirtyNote`), and Execute plans again when the drawing, the library or
the map moved since the preview, rather than apply yesterday's answer.

**Import Code File...** reads a survey code file - or any customisation
file, as the loader decides by content - and merges its rules into the BUFFER for review
before Apply, Merge by default (D1) and Replace with `importReplace` ticked;
library definitions in the file are reported but not loaded, since this
dialog edits the map. **Export Code File...** and **Export Code List CSV...**
write the buffer ("Writing it back").

**Closing never loses an edit unasked.** The manager is kept, hidden, between
uses, so its buffer outlives closing it. Closing it with unapplied edits
(`SurveyCodeManagerDialog::reject`) asks an interactive session Apply /
Discard / Cancel; a headless one closes, keeps the buffer and says so in the
log. Quitting asks the same question over the manager,
shown first (`CustomisationWorkbench::confirmClose`, run by
`MainWindow::closeEvent` before the unsaved-drawing question, because Apply
changes the drawing that question is about); Cancel keeps the window and the
edits. A headless `QUIT` with unapplied edits is refused and said - "Unapplied
Edits: the Survey Code Manager has rule edits that are not on the drawing ...
Apply or Revert them first." - never discarded, and a script presses `applyMap`
or `revertMap` first. `qt_quitting_with_unapplied_code_manager_edits_is_refused_and_said_headless`
duplicates a rule and quits.

Every editable combo in it (colours, text size type, pipe justify and shape,
property names) has a case-sensitive completer, so a typed name keeps its case
(D3), and no button is a default, so Enter in a field presses nothing. The
colour field lists `archive12d::standardColourNames()` - the names
`standardColour` draws, from its own table, so the dialog keeps no copy of
the table. Not done: the dialog takes about 2.4 s to build in a Debug build
on the reference map, most of it the two pickers' pictures; its linestyle
preview draws a linestyle small in the middle of its pane and its symbol
preview is a blank white pane; its own `linestyleState`
(`code_manager_support.cpp`) is a plain / defined / wrong-kind rule that does
not read `cad::linetypeStatus` ("Saying whether it is working"); and the
linework summary is not pluralised ("1 lines").

## Saying whether it is working

"The linestyles are not showing" has several causes that look identical: no
customisation loaded, a customisation that does not define what this drawing
names, a style whose linetype names a symbol, or a drawing whose styles are
the plain continuous line's names `0` and `1`. Loading one now reports which. A drawing with
one style whose linetype is `CULT Bollard`, a `mode vertex` symbol, gives
(`katana_cli`, `IMPORT` of such a 12da and then `CUSTOMISE`, 2026-09-24):

```
0 of this drawing's 1 styles are drawn with a loaded definition (1 name one; the rest are plain lines)
  1 name is loaded as a `mode vertex` symbol, not a linestyle, so a linetype naming it draws solid: "CULT Bollard"
```

`cad::customisationCoverage` is the one place that counts it, and it counts by
the one rule every "missing" list reads, `cad::linetypeStatus` and
`cad::symbolStatus` (`style_catalogue.hpp`; `docs/cad.md`, "What the managers
stand on"). So a name is missing exactly when what is drawn is a fallback,
never: a symbol Katana draws itself (`cross`, `manhole`), counted in `builtIn`
rather than listed (audit CAD-17); a Style linetype of `ByLayer`, which names
no definition at all - it takes the layer's (decision D2) - and which the
coverage once listed as "in no loaded library" until the integration commit
taught it otherwise; a plain line; or a style's linetype that is its own
symbol's name, which the 12da import writes for every symbol string (D8). The
missing names are kept apart by REASON, because each asks for a different
fix:

- `unresolved` - defined nowhere, in no loaded library: load one that defines
  it. CUSTOMISE says "N names are in no loaded library", and so does the
  window's log.
- `notLinestyles` - a linetype naming only a `mode vertex` definition, which
  the viewport draws solid (D2). The library IS loaded and defines the name,
  as a symbol, so the fix is to choose a linestyle; telling that person the
  name is "in no loaded library" sends them looking for a library they have.
  `cad::formatCoverage`, which CUSTOMISE prints, gives these their own line,
  as above; the window's log (`MainWindow::reportCustomisationCoverage`) does
  not mention them yet, and prints only the `unresolved` line.

The coverage looks at the styles' names only; a layer's missing linetype is
in `cad::missingNames`, the managers' list of the same thing for every Style
and Layer name, with who uses each and `MissingName::status`. The style
manager's Diagnostics and the symbol library show it ("The Format menu" in
`docs/cad.md`).

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
Both front ends LOG `builtinCustomisation().errors` and `.warnings` once at
start-up, so A12-06 is fixed ("Loading a customisation" above).

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
without it generates an EMPTY table and Katana draws every line as the plain
continuous line.

### Where the built-in customisation comes from

`tools/embed_customisation.py` reads the directory `KATANA_CUSTOMISATION_DIR`
names - a CMake cache variable, `resources/customisation/` in the source tree
by default (`src/katana_archive12d/CMakeLists.txt`). That folder is
**git-ignored**: its files stay on the disk they were put on and are never
committed. The reference customisation is kept there under general names:

| File | What it is |
|---|---|
| `linestyles.4d` | the linestyle library |
| `survey_codes.mapfile` | the survey code file |
| `survey_codes_names.mapfile` | the second survey code file, read after the first |
| `symbols.4d` | the symbol library |

**The load order is the files' name order**, which is the order the script
embeds them and `builtinCustomisation` reads them, and it is what the table
above lists. It matters twice. Between the two libraries the LATER file wins
a definition both give, so `symbols.4d` wins the four names it shares with
`linestyles.4d`. Between the two survey code files the EARLIER rule wins a
field both give (rules of equal specificity keep the order they were read,
"How a code resolves"), so `survey_codes.mapfile` has to sort ahead of
`survey_codes_names.mapfile`, and it does, because `.` sorts before `_`. A
file the script does not recognise as a style library or a survey code file,
by looking inside it, is left out.

The tests read the same folder (`KATANA_CUSTOMISATION_FILES` in
`tests/archive12d`; `-DCUSTOMISE_DIR` of `qt_customisation_headless`), and every
test of it skips when it is empty or absent.
`Customisation.TheBuiltInCustomisationIsFourGenerallyNamedFilesInLoadOrder`
holds the four names and their order, and
`Customisation.NoWordBeginsTheGroupPathOfMostBuiltInDefinitions` keeps a
word that says only whose the customisation was from the front of the group
paths.

**Swapping in another customisation.** Either put its files in
`resources/customisation/` in place of these, or point the build at a
directory kept elsewhere without moving it:

```
cmake -S . -B build/release -DKATANA_CUSTOMISATION_DIR=D:/Survey/Customisation
cmake --build build/release
```

Re-run CMake either way before rebuilding: the file list is a configure-time
glob, so a plain rebuild does not see a new or renamed file. Name the files
so that their name order is the load order you want. The tests that read the
folder were measured against the reference customisation - its counts, its
four names, the 29 renames above - so with another one they report the
differences rather than skip; `KATANA_CUSTOMISATION_DIR` set to an empty
directory builds none, and those tests skip.

Without rebuilding, Format > Load Customisation... (or `CUSTOMISE <file>...`)
goes on top of what is built in - its definitions and codes take the place of
the same ones, everything else is kept - and Format > Replace Loaded
Customisation... (`CUSTOMISE REPLACE <file>...`) takes the place of each kind
it brings. That is how a site tries a new library without reissuing the
application.
