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
  `LineStyle::source`, the NAME of its file (never a path). The style library
  reader also puts that answer on the definition itself, as
  `LineStyle::symbol`: the flag a Katana customisation file sets by the list a
  definition sits in (`docs/customisation.md`), where one name covers both
  kinds and so cannot say which. The catalogue and the lint still ask the
  name; they move to the flag with the work that brings that format into use.

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
(the linetype and symbol pickers) and `cad::codeTableRowMatches` (CODE
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
is that editor for the map ("The Survey Code Manager" below). The definition
editor is that editor for the library, one definition at a time
(`docs/desktop.md`, "The definition editor"): its form is the buffer, and Save
installs a copy of the library with that one definition added or replaced.
Otherwise the symbol library loads into the library (merged) and exports from
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

Loading the real customisation from the command line, as `katana_cli` did it
until 2026-10-06. (Its `CUSTOMISE` is the interpreter's now and reads a Katana
customisation file - `docs/customisation.md`, "The verbs" - so these four
files are refused there as not one; the window's own `CUSTOMISE` still loads
them, through this reader, with the same per-file lines.) Each long list of
names is cut short here:

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
and filters by it. It was also one of D3's four signals that a definition is a
symbol - "symbol" anywhere in the file's name - until each definition came to
say so itself (`LineStyle::symbol`): one customisation holds both kinds under
one name, so where a definition came from no longer says which it is. This
reader, which does read one kind a file, sets the flag by that file-name rule.

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

**The window loads through it** (audit QT-21, fixed), and so did `katana_cli`
until 2026-10-06, when its `CUSTOMISE` became the interpreter's, which merges
Katana customisation files by the same rule on the one customisation type
(`cad::mergeCustomisation`, "The merge" below) and replies in records. The
window's `MainWindow::applyCustomisation` - behind Format > Load and Replace,
the typed `CUSTOMISE` and `--customise` - reads the files, calls
`mergeCustomisation` with the library and map loaded now and what the files
brought, installs both results
(a kind the load did not bring comes back as it was, so a symbol file alone
keeps the map) and prints its `files`, one line each: the definitions or codes
it added and replaced. A Replace also says what went (`removedDefinitions`,
`removedKeys`), and a definition or rule that could not be installed is an
error line of its own. The load then names what the mapfile asks for that no
loaded library defines, judged against everything loaded, since a mapfile may
name what an earlier load defined. In the window REPLACE is a replace only as
the unquoted first word, in any case - a quoted path that begins with
"replace" is a path; in the interpreter's family, which sees a line's words
with their quotes already removed, a keyword is the whole first word, so a
quoted path with a blank in it is still a path and a file called exactly
`replace` is written `./replace`. In the window a Replace is an action of its
own, never a question, so a headless run never meets a box.

**A file named twice in one load is read once**, in both front ends
(`cad::distinctCustomisationFiles`): read twice, every rule of it would sit in
the map twice, since both copies are the load's and the merge keeps them all.
Each later naming is said - in the window "... is named twice in this load;
it is read once", in `katana_cli` and `katana_mcp` a `repeated file=` record.
Two namings are one file when their absolute, lexically normal paths
are equal or `std::filesystem::equivalent` says so; a pair it cannot tell
apart is read twice, which doubles rules but never drops a file.

**The built-in's faults are said** (audit A12-06, fixed). At start-up the
window logs `builtinCustomisation()`'s errors and warnings once - the
second survey code file, `survey_codes_names.mapfile`, has one `map_data` item
with no key, so every start says so - and then what it installed:
"Customisation: 792 definitions (475 symbols) and 1,624 survey code rules,
built in." in its log, the numbers grouped in the user's locale. `katana_cli`
and `katana_mcp` no longer start from those four files: their session starts
through `startCustomisation` and says what it installed and what went wrong
in its own two lines (`docs/customisation.md`, "What katana_cli and katana_mcp
start with").

### What the project records

`storage::ProjectMetadata::customisation` holds the names of the files a
drawing was drawn with, in load order - a record, not a reference: nothing is
loaded from it, and a name need not exist where the project is opened. It is
stored as one `customisation` metadata key, the names separated by line feeds
(a Windows file name may hold `;` but not a line break), and `save()` refuses
a name that is empty or holds a line break or a path separator. See
`docs/model.md` for the metadata keys an older build keeps for a newer one.

The rules are in `include/katana/cad/customisation_record.hpp`, and the
Document applies them itself ("The customisation on the Document" below);
each front end did, before, through lists of its own. A source reaches the
record as a name and what it brought (`CustomisationSource`):

- **Every load is recorded** (`recordCustomisationLoad`), the built-in's files
  first. A source loaded again moves to the end; a Replace of a kind takes
  what the earlier sources brought of that kind, and a source left bringing
  neither kind goes.
- **A save writes the record** (`customisationRecordToSave`): the loaded
  sources in load order - one that brought definitions alone only while some
  definition still comes from it - then any definition's `LineStyle::source`
  no loaded source accounts for, then the names the project already recorded
  that its OPEN found missing and no load has brought since
  (`noteCustomisationLoaded`). This session cannot judge those, so saving
  must not forget them, or every later open anywhere would draw plain lines
  without a word. The save itself writes it, into what it saves
  (`Document::save`), so no front end does and a `SAVE` that cannot go ahead
  marks nothing modified.
- **An open warns** (`customisationNotLoaded`): the recorded names that are
  not loaded now, in the project's order, compared exactly. The window logs
  "Warning: this project was drawn with customisation files that are not
  loaded: ... Load them with Format > Load Customisation..." and the CLI
  prints the same names on stderr - "warning: this project was drawn with
  customisations that are not loaded: ...", since what a project records of a
  Katana customisation is the name it declares and not a file's. Not an
  error: the drawing opens and draws, and what a missing file defined draws
  as a plain line until it is loaded.

**The built-in's files, and 29 of its definitions, have new names**
(2026-09-24). The files were given the general names they have now, and 29
definitions - 4 linestyles and 25 symbols, scale bars and north points among
them - lost the publisher's word from the front of their names. A project
saved before records the earlier file names, and its styles can name an
earlier definition. Both are known to `include/katana/cad/customisation_record.hpp`
only by hash (`sourceNameHash`, FNV-1a), so no earlier name is spelt in the
repository:

- **An earlier file name is answered by what took its place**
  (`builtinRenames`, `RenamedSource`): `customisationNotLoaded` reports it
  missing only when that is, so such a project opens without a warning
  nobody could clear, and `noteCustomisationLoaded` - through the same helper,
  so the two cannot disagree - clears it when a load brings it under its new
  name; the save then records the new name. One earlier name was a plain one
  a person's own file could have (`RenamedSource::distinctive` false): it is
  answered only in a record that also holds another earlier name of the same
  set, so recorded alone, or beside names that are not of its set, it is that
  person's file - reported missing and kept by the save. The table has grown
  a second set, the four general names, and answers with the built-in's own
  name ("What a project records, now that the Document keeps it" below).
- **An earlier definition name** is given its current one by
  `definitionNameNow` (`builtinDefinitionRenames`), asked only after an exact
  lookup missed, so a person's own definition that still has such a name is
  the one found. Every drawing lookup asks it through `findDefinition`
  (`style_resolver.cpp`): `resolveLinetype`, `resolveSymbol` and
  `DefinitionCache::find`, so a style saved naming an earlier definition
  draws that definition, on screen, on the plot and in every preview.

An older build opening a project this one saved warns about the four general
names; nothing on this side can change that.

Fixed (2026-10-06): a save that has somewhere to go but still fails (an I/O
error, a directory `ProjectStore::create` refuses) used to leave the record
written and the drawing marked modified, because `Document::setMetadata`
cannot be taken back. The save now writes the record into what it saves and
commits it to the metadata only on success
(`CustomisationState.ASaveThatFailsPartWayLeavesTheRecordAndTheDrawingAsTheyWere`
makes the store refuse a save and finds the metadata, the modified flag and
the project untouched).

Not done: a source that brought definitions alone, every one of which a later
load replaced, counts as not loaded, so opening names it - a warning in
excess, not one missed. And any later install of a customisation whose
sources still list it takes it off the missing names, so the next save lets
it go: the session loaded it and replaced it, which is a judgement, but it is
made at the install rather than at the load.

Not done, and the reason this change is committed only together with the one
that moves the front ends onto `startCustomisation`: **until a front end
hands over a host, the built-in's earlier names are answered by nothing.**
The Document answers them with the name its host's built-in declares
(`CustomisationState::builtIn`). `katana_cli` and `katana_mcp` tell it one
since 2026-10-06 (their session starts through `startCustomisation`); the
window does not yet, and the table it used before answered each first name
with one of the four general file names. So in the window a project recording
the four first names opens with a warning of four missing files, and every
save keeps them - wherever the four files are loaded under their general
names: compiled in or named to its `CUSTOMISE`. In a build that has the four files
compiled in, `tests/archive12d/test_builtin_renames.cpp` also fails (it
expects four renames naming the four files; there are eight, naming the
built-in), and is retired with the reader it tests.

Two more things the front ends do differently since the Document took the
record over, both meant: `NEW` clears the names the last open found missing
(they were the old project's), and EVERY `Document::save` and `saveAs` writes
the record - a script, a test or a tool that saves a Document, where only a
front end's own Save and typed `SAVE` did.

## The customisation on the Document: state, merge, start-up

(2026-10-06.) The Katana customisation format (`docs/customisation.md`) made a
customisation ONE value, `entity::Customisation`. This is what `cad` does with
one: where a session's customisation lives, how another is merged into it, and
what a session starts with. **The shared `CUSTOMISE` verb stands on it, and
`katana_cli` and `katana_mcp` start through it** (`docs/customisation.md`,
"The verbs" and "What katana_cli and katana_mcp start with"). **The window
does not use it yet**: it still seeds from the older built-in and loads style
libraries and survey code files as the sections above describe, until it is
moved onto `startCustomisation` and the shared verb.

### The state

The library and the survey map were all a Document held; each front end kept
the rest in lists of its own, and a verb in the shared interpreter can reach
neither. `Document::customisationState()`
(`include/katana/cad/customisation_state.hpp`) now holds everything else the
session knows of its customisation:

| Member | What it is |
|---|---|
| `name`, `description`, `notice` | the customisation's own; a session whose library or map was set directly keeps the name it had |
| `origin` | `none`, `builtIn`, `kept`, `loaded` or `edited` (`CustomisationOrigin`) |
| `sources` | what went into the session, in load order: name, whether it brought definitions, whether it brought rules, its notice |
| `basedOn` | the customisation a kept or edited copy started from: a name and the digest of its file |
| `colours` | the colour table |
| `linework` | the linework control codes - the defaults until a customisation or a setter says otherwise |
| `automation` | what is applied to survey data without being asked for |
| `kept` | the session is what the next start would give |
| `missingAtOpen` | the names the open project recorded that this session lacks |
| `builtIn` | the name the host's built-in declares, installed or not |
| `startProblems`, `keptFromAnotherBuiltIn` | what the session's start found: what went wrong, a sentence each, and whether the kept customisation was made from another built-in than the host has ("What a session starts with") |

It is session data as the library and the map are: not undoable, not in the
project, kept across NEW and OPEN. **A default Document has none of it and
installs none**; about thirty tests and `Session(nullptr)` rely on a Document
that starts empty.

- `installCustomisation(customisation, origin, kept)` installs a whole one in
  the session's place; `customisation()` gives the session back as one - what
  an export or a KEEP writes, and what a merge starts from. The two are
  inverses, also through the file (`CustomisationState.TheSessionAsOneCustomisationInstallsBackAsTheSameSession`).
  A customisation that lists no sources is its own one source. **One that says
  nothing of the linework codes or the switches installs the DEFAULTS**, not
  what the session had: an install takes the place of the whole session, and
  installed `kept` it must be the session the next start gives - a start that
  has no earlier session to have left its spellings behind. Rejected: leaving
  the session's alone, which is what this did at first. It is the MERGE's
  rule, and a load gets it there (the session always says its own, and the
  merge carries them forward); on an install it meant a reset to the built-in
  kept the control codes and the switches it was asked to reset, while the
  state read `builtIn` and `kept`.
- An install is refused, and nothing changes, for whatever would make the
  session unfit to be kept or recorded (`customisationFaults`): its name, a
  source's name or a definition's `source` that a project's record could not
  hold - the store refuses a name with a line break or a path separator, and
  every save of the session would then fail - linework codes `validate`
  refuses, and a `basedOn` the kept file's writer refuses. One list, which a
  merge reports whole for each customisation of a load, so what a load is
  told and what an install refuses cannot come to differ. A customisation
  read from a file has passed all of it already; one built in code has not.
- The raw `setStyleLibrary` / `setSurveyMap` are what an editor calls, and an
  edit they are: origin `edited`, `kept` false. `setColourTable`,
  `setLineworkCodes` and `setAutomation` are the same for their parts.
- `DocumentChange::Customisation` is bit 18. It is outside `kDrawing` (a name
  or a switch draws nothing) and no part of a Replaced drawing (the
  customisation is kept). What does change the drawing is reported with it: an
  install also names `StyleLibrary` and `SurveyMap`, a colour table
  `StyleLibrary`. **A colour table bumps `libraryGeneration` too**, because a
  sprite or a thumbnail of a definition bakes in the colours its pens resolved
  to and is dropped only on that counter.
- The raw setters still notify EXACTLY what they did
  (`DocumentChanges.MetadataLibraryAndSurveyMapEachReportTheirOwnPart` pins it),
  although they now also mark the origin. A panel showing the origin compares
  `customisationGeneration()`, which every change of the state moves. Rejected:
  a second notification from the setter - every plan view repaints on each.

**Rejected: deriving `kept` from the origin.** A reset to the built-in while a
kept file exists is `builtIn` and NOT what the next start gives. It is a flag
of its own, set by whoever knows.

### What a project records, now that the Document keeps it

Five call sites in two front ends wrote the record before a save and worked
out what was missing after an open. That logic is the Document's:

- **`Document::open` works out `missingAtOpen`** (`customisationNotLoaded`),
  and reports `Customisation` when it changed. The front end says the warning
  in its own words from that list. **`newDocument` clears it** - left standing,
  a bare `CUSTOMISE` after OPEN then NEW went on naming the old project's
  files.
- **A save writes the record INTO WHAT IT SAVES** (`customisationRecordToSave`
  in `Document::saveContents`) and `metadata()` reads it back only once the
  save has succeeded. It no longer goes through `setMetadata`, which marks the
  drawing modified - so the old trouble is gone at the root: a `SAVE` that
  cannot go ahead leaves an untouched drawing unmodified, and a save that
  fails part way leaves the metadata as it was. `typedSaveHasDestination`
  stays for the reference layers, which a front end still writes through
  `setMetadata`.
- **A source carries two flags** (`CustomisationSource`: `definitions`,
  `rules`) where it had one `library` bool. A style library brought
  definitions and a survey code file rules, but one Katana customisation
  brings both, and a Replace takes the place of one kind at a time: a source
  that brought both then keeps the other kind, and one left with neither goes.
  A source that brought definitions ALONE is recorded only while some
  definition still comes from it; one that brought rules is taken at its word.
  `CustomisationSource` is the very entry a file keeps in its `"sources"`, so
  the session's load record is written and read back without a second type.
- **The built-in's earlier names are answered by the built-in's own name.**
  `builtinRenames(builtIn)` gives eight entries in two sets: the four first
  file names, known by hash, and the four general names the files had next
  (`linestyles.4d`, `survey_codes.mapfile`, `survey_codes_names.mapfile`,
  `symbols.4d`). The general names are all PLAIN - anyone's own files might
  have them - and a plain name is answered only beside ANOTHER earlier name of
  its own set: the record of the built-in holds all four, and a person's own
  `symbols.4d` recorded alone is still that person's file, missing when it is.
  The name is not written in the source: the Document passes the name its
  host's built-in declares (`CustomisationState::builtIn`), and
  `builtinRenames()` with none takes the compiled-in customisation's, which is
  empty - and answers nothing - in a build that has none. That form is for a
  caller with no Document to ask. `customisationNotLoaded`,
  `noteCustomisationLoaded` and `customisationRecordToSave` take the table
  they are given and no longer have a form that takes none: it answered with
  whatever this program had compiled in, which is not the built-in of a run
  the seam gave another, and it made a test's answer a matter of which
  machine built it.

**Rejected: answering with the session's own name.** After `CUSTOMISE REPLACE`
with a council's customisation the session is that council's, and the four
names a project recorded for the built-in's files would be "answered" by a
customisation that has none of what they brought.

**Rejected: marking the general names distinctive.** Then a person's own
`symbols.4d`, missing, passes as loaded - the one mistake the record may not
make. Marked plain under the old rule (answered only beside a DISTINCTIVE
name) they answered nothing, since a record of the four holds no distinctive
name; hence the set.

### The merge

`cad::mergeCustomisation(current, loaded, LoadMode)`
(`include/katana/cad/customisation_merge.hpp`) is the rule of "A load goes ON
TOP of what is loaded" above, unchanged, over `entity::Customisation`: the
session and one or more loaded customisations in, what to install and a
report out. The twelve tests it came with were moved with it and keep their
expectations (`tests/cad/customisation/test_customisation_merge.cpp`); the
older merge stays where it is until the front ends leave it.

What one customisation holds beyond definitions and rules is merged by what
it is:

- **Colours by name**, compared as colour names are (`entity::foldColourName`):
  a loaded name takes the place of the session's, in both modes. A colour is
  no kind a Replace takes the place of.
- **Linework codes and automation only when the loaded customisation SAYS
  them.** A file of symbols for a colleague must not reset their control
  codes; absent is not the defaults.
- **Sources are recorded**, each loaded customisation by name with what it
  brought and its notice. One that LISTS its sources - every customisation a
  session writes does - brings those instead. **Its own notice is never
  dropped**: a session writes it at the top level and leaves the entry of its
  own name without one, so merged into another session it goes onto that
  entry (a line said in both places is said once). One that lists no source
  of its name - written under another name than the session had - gives it to
  every source it lists, since nothing says which of them it came with.
  Rejected: an entry of its own name that brought neither kind, which would
  put a name into every project's record only for customisations that happen
  to have a notice.
- **The session keeps its name.** A load is added to it. It takes the first
  loaded customisation's name, description and notice when it has no name
  yet, and when a Replace brought BOTH kinds - nothing of the session's
  definitions or rules is then left for its name to be about.
- **A load is all or nothing.** Every problem is listed, each naming its
  customisation, and with any problem `merged` is the session as it was. The
  two front ends had come to differ on exactly this: one failed a load with a
  problem, the other installed the rest.

### The built-in, and the seam

One Katana customisation file is compiled into `katana_cad` with `#embed`
(`src/katana_cad/customisation/builtin_customisation.cpp`), as the plot frame
is. The file is NOT in the repository - it is third-party material,
git-ignored - so a clean checkout and every CI build have none, and that is a
state the program runs in rather than a fault:

- `KATANA_BUILTIN_CUSTOMISATION` (a CMake FILEPATH,
  `resources/customisation/nsw.customisation.json` by default) names the file.
  Present, it is embedded; absent, the program has no built-in. Whether it is
  there is asked of the file (`if(EXISTS ... AND NOT IS_DIRECTORY ...)`), not
  of a glob: to a glob the path is a pattern, so a checkout under a directory
  with a `[` in its name had no built-in though the file was there, and a
  directory of that name was taken for the file. A `CONFIGURE_DEPENDS` glob is
  still there, over the path with its pattern characters bracketed, for the
  one thing it is for: a file put there after the configure is picked up by
  the next build.
- `KATANA_REQUIRE_BUILTIN_CUSTOMISATION=ON` makes its absence a configure
  error, for a release job that must not ship drawing plain lines. The
  configure checks that the file is THERE; that it READS is a test's to say
  (`CustomisationHost.TheCompiledInCustomisationIsReadOrThereIsNoneAndNeverOneThatDidNotRead`),
  which fails a build whose compiled-in file is not a customisation.
- `compiledInCustomisation()` parses it once, at the first call. One that does
  not parse is REPORTED, never thrown: the program starts and says so.

**The seam.** The environment variable `KATANA_BUILTIN_CUSTOMISATION` says
what "the built-in" is for one run, whatever was compiled in: unset, the
compiled-in one; `none`, no built-in; anything else, that Katana customisation
FILE is the built-in. It is read in one place, `builtInCustomisation()` -
which is what a front end asks for its host, not `compiledInCustomisation()`.
It exists so that a test of a program is the same test on a machine whose
build has the built-in and on one whose build has not. A file the seam names
that does not read gives NO built-in and the reason - never the compiled-in
one in its place, which would pass a test that named a fixture and was given
something else.

What the variable's value MEANS is a function of its own,
`builtInCustomisationFor(seam, compiledIn)`, so that the rule is tested
against a stand-in for the compiled-in customisation: tested only through
the environment, "whatever is compiled in" could not fail in a build that
compiled nothing in, which is every clone and CI. The variable is read
through `core::environmentVariable`, never `getenv`: on Windows that gives
the bytes of the ANSI code page, in which a file named outside the code page
is a file named with `?` and is not found (`docs/customisation.md`, "What
katana_cli and katana_mcp start with"). The file is then named as text,
through `core::pathFromUtf8`: text handed to `builtInCustomisationFor` may
still be narrow bytes that are not UTF-8, and a `std::filesystem::path` built
straight from those throws at the first of them - out of a function that
promises to report and never throw.

The older embedding (`tools/embed_customisation.py`, four files) and
`archive12d::builtinCustomisation()` are still what the window calls, and
stay until it is moved; the session of `katana_cli` and `katana_mcp` asks
`builtInCustomisation()` and no longer reads the four files.

### What a session starts with

A front end hands over a `CustomisationHost`
(`include/katana/cad/customisation_host.hpp`): the built-in, which a build may
not have, and the path of the file the user keeps their own in, which a front
end may not have. `startCustomisation(document, host)` installs the kept file
when it exists and reads (origin `kept`), else the built-in (origin
`builtIn`), else nothing, and what it installs is `kept`. Its report says
what was installed, with counts, and every problem in a sentence.

- A kept file that does not read is REPORTED and the built-in stands in for
  it, rather than the session starting with nothing.
- The built-in is installed based on ITSELF (`basedOn` its own name and the
  digest of its own bytes), so a copy kept from it later says what it was made
  from - whatever the built-in's file says IT was made from. A built-in that
  was itself exported from a session names an earlier customisation; kept as
  that, every copy made from it carried a digest that is not this built-in's
  and was reported as made from another, at every start. A kept file whose
  `basedOn` is not this built-in's name and digest is installed all the same
  and reported (`keptFromAnotherBuiltIn`): it is the user's, and they may want
  to know the built-in has moved on.
- What a start installs is the session the NEXT start would give, the control
  codes and the switches included: starting over a changed session - which is
  what a reset to the built-in is - leaves the session a new Document started
  with the same host has
  (`CustomisationStart.StartingOverAChangedSessionGivesTheSessionAFreshStartGives`).
- What a start found is left on the Document as well as handed back: the
  report's problems and `keptFromAnotherBuiltIn`
  (`CustomisationState::startProblems`, `Document::setCustomisationStart`),
  which `CUSTOMISE JSON` gives as `start` for as long as the session runs
  (`docs/customisation.md`, "The replies"). A front end says them once, where
  its errors go, and a client of `katana_mcp` is never shown that.
- A Document given no host installs nothing.

Reading a file's bytes, turning typed text into a path and reading an
environment variable that names a file are core's
(`include/katana/core/path_text.hpp`): the UTF-8 path helper was private to
the sheet verbs, with a second copy in the utility verbs. `ifc::pathFromUtf8`
is core's now too, under the name its callers use; its own conversion threw
on a name that is not UTF-8. What the standard library does with narrow bytes
was probed rather than assumed (GCC 16.2, 2026-10-06): `std::filesystem::path`
converts them as UTF-8 and throws at the first byte that is not. Not done:
`src/katana_surveyio/reader.cpp` keeps a private copy of the old conversion
for a file named inside a field file. Whether a name that is not UTF-8 can
reach it was not looked into, and it was left as it is.

### One resolver for a colour name

`cad::resolveColour(document, name)` (`include/katana/cad/colour_lookup.hpp`)
is the Document's customisation table, then the standard names;
`colourLookup(document)` is the same as a function, in the shape
`SurveyCodingOptions::colourOf` and `ColourLookup` take. It asks the Document
at each call, so it follows a table installed later. **A caller that passes no
lookup still gets no colours** - it is offered, not applied behind anyone's
back, and `ColourLookup.SurveyCodingGivenNoLookupStillLeavesColoursAloneWhateverTheDocumentKnows`
holds that.

## Writing it back

Under D1 an edited library or map is kept by writing it to a file, so the two
readers now have inverses in `archive12d`, each tested as one: read what was
written and get back what was given (`tests/archive12d/customisation/`).

**`writeStyleLibrary(library, StyleLibraryWriteOptions)`** gives `.4d` text,
UTF-8, in name order, one command per line as the reference libraries are written. Every field is
written - the three kinds, `mode vertex`, group, length, factor, origins,
anchors, stretch and cycle mode, pens, every stroke, and a text's three
numbers kept but not understood - so `readStyleLibrary(writeStyleLibrary(l),
name)` is `l` when `l` came from a file of that name. Two things a file
cannot hold, because the file's own NAME says them and the reader sets both
again from the name it is given: `LineStyle::source`, and `LineStyle::symbol`
(whether that name holds "symbol"). Read back under no name, a definition
differs from what was written in those two and in nothing else. `names` picks the
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

**Where the writers are reached.** No longer from the window: since
2026-10-06 the Survey Code Manager's Export Codes... and the symbol library's
Export Selected... write Katana customisation files
(`entity::customisationToJson`; "The Survey Code Manager" below and
`docs/desktop.md`, "Symbol Library"), and these two writers are reached only
by their own tests. Export Code List CSV... still writes `cad::codeListCsv`
(RFC 4180, UTF-8 with no byte order mark, so Excel may misread a name that is
not ASCII). `katana_cli` writes neither of those files: its `CUSTOMISE EXPORT`
writes the session as one Katana customisation file (`docs/customisation.md`,
"The verbs").

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
- **A colour name is resolved by whoever is asked to code.** `applySurveyCodes`
  takes the lookup as a callback (`SurveyCodingOptions::colourOf`), because
  the standard colour names were once known only to `archive12d`, which `cad`
  may not see; without one, colours are left alone rather than guessed at.
  The `CODE` verb no longer depends on a front end for it: it resolves a name
  through the Document (`cad::resolveColour`: the customisation's own table,
  then the standard names, which are the entity layer's since 2026-10-06), and
  what a front end passed with `CommandInterpreter::setColourLookup` is asked
  only for a name neither knows (`docs/customisation.md`, "Colour names" and
  "The verbs"). A name nothing knows - a plot pen, `pen 025` - still leaves
  the colour alone.

From the command line, the whole chain:

```
katana_cli
  -c 'POINT 0,0' -c 'SELECT ALL' -c 'PROP SET code WM01 text' -c 'CODE' -c 'LIST'

scope=drawing matched=1
1 entity carries a code in "code": 1 matched, 0 fallback-only (only the bare * rule answers), 0 with no rule
1 entity changed
Layers created: SURVEY SERVICES
Styles created: WATR Main
By code:
  WM01: 1 entity, prefix, matched; layer 0 -> SURVEY SERVICES; style WATR Main (created); set DepthLocation, Depth Location; 1 changed
Applied as one command. UNDO puts it all back.
1  Point  layer=SURVEY SERVICES  at 0,0
```

(With the compiled-in customisation, so no CUSTOMISE is needed. The first
line is what the scope took: `CODE` with no scope word is the whole drawing,
which holds the one point. The `DepthLocation` and `Depth Location`
attributes come from the bare `*` pipe rules every code meets; see "Asking
the map why".)

## Asking the map why: explain, list, census, lint

A person looking at a point drawn wrongly has one question - why did this
code get that? - and until now nothing could answer it. `include/katana/cad/code_table.hpp`
holds the answers, below both front ends, each a structured result plus ONE
formatter, so the application and `katana_cli` print the same words (the two
used to disagree about what applying codes had done; `formatCodingReport` is
now the one report of an application, with a row per code):

The verbs are the `CommandInterpreter`'s (`include/katana/cad/survey_code_verbs.hpp`)
since 2026-09-26, so the window's command line, `katana_cli` and `katana_mcp`
all have them; until then they were `katana_cli`'s own, and the window
answered "unknown command". `CODE` itself - apply the loaded codes, one undo
step - is one of them too. Since 2026-10-06 `CODE` and `CODE CENSUS` take the
shared scope and filter (`CODE LAYERS survey WHERE TYPE=point PREVIEW`), each
reply beginning with what the scope took, and the two verbs that were
`MAPFILE LIST` and `MAPFILE CHECK` are `CODE LIST` and `CODE CHECK`:
`docs/customisation.md`, "The verbs", has the grammar, why `CODE` with no
scope word is the whole drawing, and how a line's first word is read.

| Question | Function | Verb (every command line) |
|---|---|---|
| Why does this code get what it gets? | `explainCode` → `formatCodeExplanation` | `CODE EXPLAIN <code>` |
| What does the map say, one code per line? | `codeTable`, `codeTableRowMatches` → `formatCodeTable` | `CODE LIST [<filter>]` |
| Which codes do these entities carry? | `codeCensus` → `formatCodeCensus` | `CODE CENSUS [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW]` |
| What is wrong with the map before it is applied? | `lintSurveyMap`, `lintSurveyRule` → `formatLint` | `CODE CHECK` |

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
group, layer, colour, linestyle or symbol, case folded (D3).

**The census** counts every distinct code the drawing carries, found as
`applySurveyCodes` finds it, and classes each against the loaded map; each
distinct code is looked up once.

**The lint** has two severities: an ERROR is a rule that cannot be applied as
written, a WARNING one that applies but not as its author meant.

| Kind | Severity | Meaning |
|---|---|---|
| `InvalidLayerPath` | error | a layer that is not a layer path |
| `KeyWhitespace` | error | a key with surrounding blanks, which no typed code matches |
| `UnresolvedLinestyle` | warning | a linestyle no loaded library defines |
| `UnresolvedSymbol` | warning | a symbol no library defines and Katana cannot draw |
| `SymbolNotSymbolCapable` | warning | a symbol rule naming a definition that is not a symbol: neither `at vertices` nor listed as one by its customisation (D3, `LineStyle::symbol`). A definition read from no known file once got the benefit of the doubt, when the sign was the file's name; every definition says which it is now |
| `LinestyleIsVertex` | warning | a linestyle naming an `at vertices` definition, which is a symbol |
| `UnknownColour` | warning | a colour name the colour table does not know |
| `NoModel` | warning | a `feature` rule with no layer, which leaves its code where it is |
| `DuplicateRule` | warning | the same as an earlier rule, field for field |
| `ShadowedRule` | warning | earlier rules of its key already say all it says |
| `FieldOutsideSection` | warning | a field its section does not use - a symbol rule that also names a layer. A lookup applies it, so it is no error; but a rule is known by its key IN ITS SECTION and that is what a load replaces, so the field comes and goes with rules it has nothing to do with. The table of which section uses which field is the one the survey code file writer enforced by refusing to write such a rule |

The words in those texts - `feature`, `layer`, `at vertices` - are the Katana
customisation format's, not the survey code file's (`map_data`, `model`,
`mode vertex`); `docs/customisation.md`, "The words a rule is shown by",
has the table.

`SurveyMap::add` refuses a key with blanks and an invalid layer path, so those
two can be met only by `lintSurveyRule` on a rule not yet in a map - an
editor's form before it commits. `CODE CHECK` fails the command when any
error is found, so a script stops: the refusal's first line counts the errors
and the lint follows it, as `UTILITY CHECK` carries its check
(`codeCheckReply`, tested on hand-made issues because no map can hold such
a rule today). On the compiled-in pair of mapfiles, run
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
  string attribute DepthLocation = Top of Pipe  <- rule #664 * (pipe)
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
  breakline, because the file has said the points are one line - unless
  `onlyRuledLines` is set, which is how an import uses it (below).

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
spelled alike. The spellings are part of a customisation and the session's
are the Document's (`Document::customisationState().linework`): the
`LINEWORK` verb and a survey import read them there. The Survey Code
Manager's Linework tab still shows, edits and runs with a copy the Format
workbench keeps (`CustomisationWorkbench::lineworkCodes`), which starts as
the defaults each session - so until the tab runs the verb's line, a
spelling changed in one is not the other's.

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

**`skipLinesAlreadyDrawn`.** Off by default. On, a line planned now that the
drawing already holds is not drawn a second time: a polyline carrying the
same code and string number that runs through the same vertices at the same
heights, closed alike, whatever drew it and whatever layer and style it has
since been given. It is listed in `LineworkReport::alreadyDrawn` instead of
`strings`, its points count as placed, and none of them is removed on its
account whatever `keepPoints` says - the line that would stand in for them
is not this run's. The comparison is exact, with no tolerance: a line is
built from its points' own coordinates each time, so one that differs in the
last place is the line of a point that moved. It is an option, and not what
`processLinework` always does, because an import's own run must not have it:
two imports of one file each draw, and own, their lines. The `LINEWORK` verb
turns it on (below).

**`drawSurveyFeatures` and the code property.** Pass it the options the
import was given: the line goes on its code's layer, or with its points when
the code names none. When the import wrote no code (`codeProperty` empty),
each line still gets its code, under `codePropertyCandidates().front()`
(`code`), and `LineworkReport::property` names it - a line is styled by the
code it carries. `coding.createLayers` governs every layer here, as in
`processLinework`; `import.createLayers` is not read.

Process Linework is the `LINEWORK` verb ("`LINEWORK` on the command line",
below) and the Survey Code Manager's Linework tab, previewed before it runs,
against the drawing's map. Draw Survey Features has no command, menu item or
dialog of its own: it runs inside an import that asks for linework (next
section).

**A point is coded by its string name.** `applySurveyCodes` looks a POINT up
by the string name of its field code, its linework controls left out
(`parseFieldCode`): `PABB ST` is coded as `PABB`, and reported under `PABB`,
so the styling a point gets and the line it joins agree. It used to read the
whole field, which left an exact-key point with a control token unmatched.
Any other entity's code is still looked up whole.

**A string name is the code followed by the string number.** A delimited list
writes the name whole in its code column (`KJ01`). A field file whose records
keep a code column and a string column gives a point the code alone (`KJ`),
and the readers that share one project builder put the number only in the
name of the `survey::SurveyFeature` that strings the point. Looked up by the
code alone, strings 01 and 02 of `KJ` are one string, and a one-letter code
`B` in string 12 never meets the key written for it, `B1*`: a key with a
digit before its `*` answers only a name that includes the string number. So
the number comes into the drawing beside the code, in the property `string`
(`kSurveyStringProperty`): `nameSurveyStrings` gives each point the name of
the first named feature OF ITS OWN CODE as a metadata key, and the import
bridge writes it like any other. A feature of another code does not name the
point (a point shot again under a second code is still of its first), and
neither does a feature with no name.

A survey job writes it only when the import is to be coded or strung AND the
drawing has survey codes. Rejected: on every import, as the brief first had
it. A job imported with both options off must be drawn as it always was, and
so must one finished into a drawing with no customisation - a finish that is
on by default may not change a drawing it does nothing to, not by one
property. The cost is under "Not done" below.

Three readers then agree, through one function, `surveyLookupName(map, code,
string)`:

- `applySurveyCodes` looks an entity up by its code followed by its string
  number - a point's first token, another entity's whole code - and reports it
  under that name (`B12`);
- `processLinework` groups by the same name, so `KJ` in string 01 and `KJ` in
  string 02 are two lines, and a point written `KJ01` whole joins the first;
  the line carries what its points carry (the code, and `string` when they
  have one), so it finds the rule they found;
- `drawSurveyFeatures`, under `onlyRuledLines`, reads a feature's name as its
  string number.

The name is tried first and THE CODE ALONE SECOND, when no rule more specific
than the bare `*` answers the name. Rejected: the name only. An exact key
(`PABB`) does not match `PABB3`, so numbering a string would lose a rule the
code had the day before, and a code nobody wrote a rule for would be reported
once per string instead of once. The code is also what is looked up when the
name's best rule is a prefix shorter than the code - `PA*`, a rule for a
family of codes - and the code has an exact key: the family rule answers
`PABB3`, and would otherwise have coded a numbered `PABB` string in place of
`PABB`'s own rule. A prefix as long as the code or longer (`KJ*` for `KJ`,
`B1*` for `B`) was written for the numbered names, and the name is looked up.
An entity with no `string` property is read exactly as it was before the
property existed.

### One step from field file to finished drawing: `withSurveyFinish`

Importing a coded field file took three steps a person had to know about -
import, Apply Survey Codes, Process Linework - each its own undo step, the
second replacing the selection and the third reachable from one dialog.
`cad::withSurveyFinish` (`include/katana/cad/survey_finish.hpp`) makes them
ONE command. It owns the command that draws the points, runs it, and then, as
`SurveyFinishOptions` say (`codes`, `linework`; both OFF by default, so a
caller that says nothing imports exactly as before):

1. codes the points that command created (`applySurveyCodes`);
2. draws the strings the file NUMBERED itself (`drawSurveyFeatures`): the
   features that have a name and name a point of this import;
3. strings the other created points by their codes (`processLinework`), so a
   file with no such features - a delimited list coded `KB01 ST`, a controller
   file with no string numbers - is strung by its codes.

The decisions, and why:

- **Each step is planned when it runs, and replayed by undo and redo** - what
  the styling of linework's lines always did. `applySurveyCodes` and
  `processLinework` read the drawing, and the points are not in it until the
  import has run. `drawSurveyFeatures` and `processLinework` fix the layers
  they create when they are planned, and creating a layer that exists is
  refused: so the features are planned after the coding step has RUN, and the
  code-strung lines after the features have.
- **Inside the job command, not around it.** `ImportSurveyJobCommand` takes
  the options as `SurveyJobImport::finish` and wraps its own points command.
  Rejected: an outer `Transaction` of the job command, a coding step and a
  linework step. The reduced coordinates and the features are a local of the
  job command's `execute` (the project it draws drops the features, and the
  raw project is let go), so a step added afterwards has nothing to draw
  from; the job records `createdEntities` from its own points command, so
  lines drawn by a later step would be nobody's and Remove Job would leave
  them; and a `Transaction` validates its first part twice, each validation of
  the job command being a reduction.
- **It acts on what the import created, and nothing else.** An empty id list
  means every entity to `applySurveyCodes` and every point to
  `processLinework`, so neither is ever handed one: an import that drew
  nothing codes nothing. A string of the file none of whose points the import
  created is not drawn (`SurveyFeatureOptions::consider`): a longer copy of a
  file topped up with Skip draws the strings that run through a new point,
  not every string of the first copy a second time. It never deletes a point
  (`keepPoints` stays on): a removed point would fail the job's pairing of
  entities with points, or read later as deleted by hand.
- **Only what a rule makes a line is drawn** (`onlyRuledLines` on both
  functions): a string is a line when a rule MORE SPECIFIC than the bare `*`
  says so. Some readers make a feature of every coded shot, so unfiltered
  every survey mark of one code is joined to the next; and `lookup` inherits a
  breakline from `*`, which answers a typo too. A control code alone draws
  nothing here either - a line no rule describes is a surprise in a step
  nobody asked for by name, and Process Linework still draws it when asked.
  The others are reported: `UnplacedFeatureReason::NoRule` / `PointCode`,
  `UnplacedReason::NoRule` / `PointCode`.
- **Only a NAMED feature is a string of the file's own.** A field file that
  keeps a string number beside the code gives each (code, number) a feature
  named by the number: there the file has said which points are one line. A
  feature with no name says only what its points' codes say - and a reader
  whose format has no string numbers makes one per RUN of consecutive shots
  of a code, so two kerbs shot in sections across a road are runs of one
  point each, and a kerb with a tree shot in the middle ends at the tree.
  Such a feature is not drawn; its points go to step 3, where their codes
  group them as Process Linework would. Rejected: drawing every feature a
  rule makes a line and leaving every point any feature names out of step 3
  (what the brief asked for, and what this did at first). It is right only
  for a reader whose feature IS the whole string: with the run-per-feature
  reader it drew no kerb at all and reported that the step ran.
- **A point is in at most one automatic line of its own string.** A point a
  named feature OF ITS OWN CODE holds is that string's - the test by which
  `nameSurveyStrings` numbers it - and the feature has answered for it, with
  a line or with the reason there is none, so it is left out of step 3
  (`SurveyFinishReport::pointsInFeatures`): its code would say the same of
  it a second time. A named feature of ANOTHER code - a line keyed on a
  controller between two kerb points and coded as a boundary - is drawn or
  not on its own and leaves its ends in their own code's string. A feature
  line carries the code and `string`, and is reported as the two (`KB 32`),
  not by the number.
- **A line runs through its points where they stand.** A vertex of a file's
  string is taken from the point's entity when this import, or the job being
  re-adjusted, has one in the drawing, so a point the person moved (and a
  re-adjustment keeps where they put it) is still on its line, as it is on a
  line strung by code. A point with no such entity is where the file puts
  it: the drawing's control, a point a policy skipped, one deleted by hand.
- **A step with nothing to do is not an error, and says why**
  (`SurveyFinishSkip`: `no-survey-codes`, `no-codes-in-file`,
  `no-rule-matches`, `no-points`). With no rule for any code the coding plan
  is dropped whole - also the fallback rule's attributes that a `CODE` run
  would attach - and an empty map skips the linework too: with no map nothing
  says which codes are lines. With no survey codes loaded the drawing is
  EXACTLY the one the import makes without the finish; with codes loaded and
  no rule for the file's codes it differs by the `string` property alone. A
  real failure (control codes spelled alike, a feature naming a point the
  project lacks) fails the WHOLE import, points included. Rejected: drawing
  the points and reporting the failed step, because an import that asked for
  codes and silently got none looks like one whose codes matched nothing.

`SurveyFinishReport` says, per step, what ran or why not: the points coded,
matched and with no rule (`codesWithNoRule`), the layers and styles created,
the lines drawn (`lines`: the reply's point count is `points`, never the
created list, which now holds the lines too), and the features and points in
no line with their reasons. `describe` gives all of it as sentences, for a
log. `finishWarnings` gives only the sentences a person may have to act on -
a step that was asked for and had nothing to go on, codes with no rule, a
string or a point left out of every line for a reason other than being a
point code or uncoded, a job's lines deleted or left stale - and those are
what a job adds to its own report's warnings. Rejected: adding every
sentence there. The report's warnings are what `SURVEY IMPORT` counts and
lists as `reduction_warnings`, so an import that coded and strung everything
would have answered with two warnings.

**The job owns its lines.** `SurveyJob::createdEntities` holds the points and
then the lines; `placedPoints` stays the points alone. Remove Job deletes
both. The job's option text (`katana-survey-import-options=1`,
`writeSurveyJobOptions` / `readSurveyJobOptions`) gains `apply-codes` and
`draw-linework`, written only when on, and ABSENT IS OFF with the version
unchanged: a job imported before the keys existed was not coded, and
re-adjusting it must not start to. What is stored is what was asked for,
whether or not a step then had anything to do.

**Re-adjusting a finished job** finishes it again in its one undo step, as its
stored text says: the points the run draws for the first time are coded (the
others keep what they have), and - when the run changes the drawing at all -
the job's lines are strung again from the new reduction, the file's numbered
strings first and then the job's other points by code.

- A line the run still makes is REDRAWN IN PLACE: the earlier line that
  carries the same code and string number is the same entity afterwards - its
  id, its layer, its style, its other properties - with the new vertices and
  heights (`LineworkOptions::earlierLines`, `LineworkString::redrawn`). A line
  none of whose points moved is not touched. Rejected: deleting the job's
  lines and drawing them again, which this did at first. Every command
  carries the associative update, which removes a label whose target is gone
  and a smart leader with it and detaches a dimension: one point moved by a
  millimetre took every label off every line of the job, with the place it
  had been dragged to and the text typed over it, and the report said only
  that lines "were deleted". It also undid a layer or a style the person had
  given a line, which a re-adjustment never does to a point.
- A line the run no longer makes is deleted, and what follows it goes with
  it, as with any deleted entity. The report says how many, as a warning.
- The earlier lines are touched only when the linework step runs: with no
  survey codes loaded, or no rule for any code, they are left, and the
  report says they are as the earlier adjustment drew them and how many.

Whether is the job's; HOW is the caller's (`SurveyJobReadjustment::finish`:
the colour lookup, the control codes, the order), because a function and the
session's control codes are not job data.

Not done:

- `SURVEY IMPORT` sets the options (`docs/survey.md`, "SURVEY IMPORT codes
  and strings what it draws"); the import wizard does not yet, and still
  draws points only. The Survey Jobs dialog does not pass
  `SurveyJobReadjustment::finish`, so a new point of a finished job is styled
  without a colour lookup (and gets a second style when its rule names a
  colour); `cad::surveyImportFinish(document).options` is what it should
  pass.
- **A job keeps no string numbers unless it was finished with survey codes
  loaded.** Imported with both options off, or into a drawing with no
  customisation, its points carry the code alone, and nothing adds the number
  later: the features are the reduction's and are not kept with the job, and
  a re-adjustment numbers only a job stored as finished. `CODE` by hand then
  looks `B` up, not `B12`, and never finds `B1*`; `LINEWORK` by hand joins
  `KJ` 01 to `KJ` 02. Removing the job and importing it again, finished, is
  the way to get them.
- Where a job's line RUNS is the job's: a line the person reshaped is redrawn
  by a re-adjustment like any other, lines having no record of how the job
  left them as points have.
- A point deleted by hand that `Keep` leaves deleted is still a vertex of a
  string the file numbered, where the new run puts it (the report counts
  those vertices), and is passed by on a string strung by code, which has
  only the entities to read. The two kinds of line differ there.
- A named line record whose code IS its end points' code numbers those ends
  as its own string, so the rest of that code's points are strung without
  them.
- The Survey Code Manager's Linework tab, which calls `processLinework`
  itself, draws a finished job's lines a second time over its points. The
  `LINEWORK` verb leaves those points out (next section); the tab does until
  it runs the verb's line.

### `LINEWORK` on the command line

```
LINEWORK [<scope>] [WHERE k=v ...] [ORDER number|entity] [PREVIEW]
```

`processLinework` as a verb of the shared interpreter
(`include/katana/cad/linework_verbs.hpp`, `runLineworkVerb`), so the window's
command line, `katana_cli`, `katana_mcp` and an agent all have it; `HELP
LINEWORK` is its reference. Until 2026-10-06 the one way to string points was
a tab of a dialog. What it decides:

- **The scope is the shared one, and comes first** (`docs/cad.md`, "Scope and
  filter"). With no scope word it is the selection when anything is selected
  and the whole drawing when nothing is, also under a bare `WHERE`, and the
  reply's `scope=` says which. Rejected: the selection always, as `MODIFY`
  has it, refusing when nothing is selected. Stringing a survey is nearly
  always done to all of it, and a typed `LINEWORK` in a drawing with nothing
  selected would be refused for want of a word. A scope word after `ORDER`
  or `PREVIEW` is refused by name rather than read as one: guessing it was
  meant is how a line strings the whole drawing where it was to string one
  layer.
- **The control codes and the colours are the Document's**
  (`customisationState().linework`, `cad::colourLookup`), as a survey
  import's are, so a line strung by hand looks like one an import strung.
- **A control code alone still makes a line**, and so does the fallback rule
  (`onlyRuledLines` stays off): the run was asked for by name. An import's
  own run draws only what a rule more specific than `*` makes a line.
  `processLinework` decides it point by point, so where no rule makes the
  code a line ONLY the points that carry a control code are joined: of `KB
  ST`, `KB`, `KB CL` with no rule for `KB`, the first and third make the
  line and the second is unplaced, with the reason. The help said "or a
  control code asks for one" of the whole string until a review ran it; it
  now says what happens
  (`LineworkVerb.WithNoRuleForACodeOnlyItsPointsThatCarryAControlCodeAreJoined`).
- **It never removes a point.** `keepPoints` is not offered: a point taken
  out from under a survey job reads to its next re-adjustment as deleted by
  hand. The tab keeps its box.
- **`ORDER number|entity`**: by point number (the default; a point with none
  has no place and is reported) or as drawn, which for an import is the
  order the file listed the shots.
- **`PREVIEW`** plans and reports. A run that draws nothing is no undo step;
  one that draws is exactly one.
- **Points their survey job has already strung are left out, and counted**
  (`left_out=<n> reason=strung-by-their-job`). A job imported with linework
  on drew its lines itself - the file's numbered strings among them, which
  only the file knows - and owns them: a re-adjustment redraws them in place
  and Remove Job deletes them. A second line through the same points would
  lie on top of the job's and be nobody's. How it is known: the job's stored
  options say `draw-linework`, the point is among the job's
  `createdEntities`, and a polyline among those same `createdEntities`, still
  in the drawing, carries the point's string name (its code's first word
  followed by its string number - what a line drawn by linework carries
  too). So a point of such a job whose string the job has NO line of - a rule
  written since, a line deleted by hand - is strung by the verb like any
  other. Three alternatives were rejected:
  - every point of a job stored with `draw-linework`, whatever it drew. A job
    imported when no rule made its codes lines would then never be strung by
    anything, and the reply would say its job had strung points that are in
    no line;
  - handing the job's lines to `processLinework` as `earlierLines`, to be
    redrawn rather than drawn again. A string by code is not the file's
    string: `KB` 1 closed and begun again is two strings of the file and one
    name, and the redrawn line would run through both - the verb would
    reshape lines the job owns;
  - leaving out any point whose string name some line in the drawing
    carries, job or no job. Two surveys of one site both have a `KB1`.
  A job whose option text cannot be read (damaged, or a newer Katana's)
  refuses a run whose scope takes one of its points, naming the job: whether
  it strung them is not known, and a guess either way draws twice or not at
  all. A scope that takes nothing of it does not read it.
- **A line the drawing already holds is not drawn again, and is counted**
  (`left_out=<n> reason=already-drawn lines=<m>`: `m` lines, and the `n`
  points they run through, each once). Added 2026-10-07, after a review ran
  `LINEWORK` twice and got the same line twice: the job rule knows only what
  a job's own record names, so a second run over loose points, over a job
  imported with linework off, or over a string whose job line had been
  erased and strung again, laid every line on top of the last and said
  nothing of it. The verb now plans with
  `LineworkOptions::skipLinesAlreadyDrawn`: a line is the line already drawn
  when a polyline carries its code and string number and runs through the
  same vertices at the same heights, closed alike. It is the VERY line that
  is known, never its name. A string that has gained a point, or whose point
  was moved or re-levelled, is another line: it is drawn, and the line that
  was there stays until someone erases it. Two alternatives were rejected:
  - redrawing the earlier line in place, as a job redraws its own
    (`earlierLines`). The verb has no record of which line is whose, so it
    would take whichever polyline carries the name - and a `KB1` of another
    survey, anywhere in the drawing, would be dragged onto these points;
  - comparing the vertices and not the heights. A string re-levelled and
    strung again would then answer "already drawn" over a line that still
    carries the old heights, which is a stale surface nobody was told of.

The reply is records: `linework scope=... matched=<n> considered=<n>
lines=<n> unplaced=<n> notes=<n>` (`matched` is what the scope took,
`considered` the points among it that were looked at - those left to their
job are not among them, those of a line already drawn are - and `lines` the
lines drawn; `preview=yes` after them for a preview), the two `left_out`
records when there is something to say, a `string`
record for each line - its name, key and number, how many points and
vertices, whether it closed, its layer, `join=yes` on a join and, once
drawn, its entity - for the
first 50 (`kLineworkStringsListed`), then `strings_more=<n>`, an `unplaced
reason="..." points=<n>` record for each reason a point is in no line, and a
`note kind="..." count=<n>` record for each kind of note. The reasons and
kinds are `toString(UnplacedReason)` and `toString(LineworkNoteKind)`, the
words the tab shows, quoted; a second set of one-word names for them would
be a second vocabulary to keep in step. A scope that takes nothing is
answered with zeros.

Tests: `tests/cad/customisation/test_linework_verbs.cpp` - the grammar's
refusals by name, the default scope and each scope word, the preview, a run
as one undo step with the Document's colour and control codes, both orders,
a rectangle, a closed string and a join told apart in their records, every
reason a point is in no line in one reply, the left-out rule (a finished
job; part of one, where only what the scope took is counted; a job imported
with linework off, which is strung; a string whose line was deleted, which is
strung again - once), a job with unreadable options and one a newer Katana
wrote, a line already drawn (the second run, a string that gained a point,
and what makes a line the same line, case by case), a drawing with no survey
codes, a control code with and without a rule, points with no code, the cap
of 50, and the help. `cli.linework_strings_points_by_their_control_codes_as_one_undo_step`
runs it twice through `katana_cli`.

Not done:

- A line is known as drawn only when it is exactly the line this run plans.
  After a point is added to a string, moved or re-levelled, or when the
  other `ORDER` runs the string through its points another way, the string's
  line is drawn anew and the earlier one is left where it was: the person
  erases it. Nothing ties a line to the points it was strung through.
- A job imported with `LINEWORK off`, strung afterwards by the verb, is
  strung BY CODE and not as its file strung it: a string the file closed and
  began again under one number (the fixture `gnss.fld`: `KB` 1 closed through
  R001 to R003, then R004) comes out as one open line through all four
  (`SurveyVerbs.AJobImportedWithLineworkOffIsStrungByTheVerbByCodeAndOnlyOnce`).
  The file's own strings are drawn only by an import that asks for linework;
  re-importing the job with it on is the way to get them.
- A code no rule makes a line is strung only through the points that carry
  a control code (above): a shot of that code between them with no control
  code is left out, not joined.
- The note that a string has no rule to style it is counted also when its
  line was found already drawn.
- The Survey menu has no Process Linework item and the Linework tab does not
  run this line yet; nor does `katana_mcp`'s command tool name the verb in
  its description (the line itself reaches it, and `katana_help` lists it).
- The reply counts the notes by kind and does not list them; the tab's
  preview still does. It does not say which layers and styles its lines
  created, as `SURVEY IMPORT`'s `linework` record now does.

## The Survey Code Manager

Format > Survey Code Manager... (also on Survey > Survey Coding;
`src/katana_qt/customisation/code_manager*.cpp`) is the editor D1 asked for: the
survey code library a surveyor codes against - Civil 3D's description keys, TBC's feature definitions, Carlson's field-to-finish -
in one non-modal dialog of five tabs, each over one of the cad foundations
above, so the dialog decides nothing the CLI would say differently:

| Tab | Over | Shows and does |
|---|---|---|
| Code Table (`codeTableTab`) | `cad::codeTable`, `explainCode` | a key per row with what it resolves to (layer, colour, line or point, linestyle, symbol, surface, attributes), filtered; a code typed in `testCode` or a selected key explained field by field - the value, the rule that set it, the rules that lost - with previews of its linestyle and symbol; double-click an explained field for the rule that set it; a rule form by section, with Add, Update, Duplicate, Delete, Up and Down (earlier wins more ties) |
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

**Import Codes...** (`importCodes`) reads a Katana customisation file
(`docs/customisation.md`; `cad::readCustomisationFile`) and merges ITS RULES
into the BUFFER for review before Apply, Merge by default (D1) and Replace
with `importReplace` ticked. The merge is `cad::mergeCustomisation`, the one
every load goes through, handed the buffer as the session and the file's
rules as the load - so Merge and Replace mean here what they mean to
`CUSTOMISE`: a key's rules in a section take the place of that key's in that
section, or the file's rules become the whole buffer. The log names the load
by its customisation and counts a key once for each section it has rules in
("test_survey (merge): 11 added, 0 replaced ..."). The definitions, colours,
linework codes and automation switches the file also holds are NOT taken:
they are counted in the log ("definitions (7), colours (2), the linework
codes"), which says where they are loaded - the Symbol Library's Import
Definitions, or the `CUSTOMISE` line for a whole customisation. A file with
no rules, and a file that is not a Katana customisation (one of the older
formats is told "not a Katana customisation file"), are refused and the
buffer is as it was.

**Export Codes...** (`exportCodes`) writes the buffer as a Katana
customisation of survey codes alone, under the session's name - or, while the
session has none, the file's own name without `.customisation.json`. The file
is the session's customisation with the buffer's rules in the place of its
own, cut down to its codes by the ONE rule a part of a customisation is
written by - both managers' exports and `CUSTOMISE EXPORT <file> CODES`
(`cad::customisationPart`, `include/katana/cad/customisation_part.hpp`;
`docs/desktop.md`, "Symbol Library", has the table, and
`docs/customisation.md`, "One rule for a part, whoever writes it", the
reasons):

- the rules, in the buffer's order - order is precedence, so a rule moved
  between two of another section is written between them
  (`ExportedCodesKeepTheirOrderWhereSectionsInterleave`);
- the session's description and its author's NOTICE, and the sources that
  brought the session rules, each with its own notice. A notice is "carried
  with the data and shown to whoever uses it" (`docs/customisation.md`), and
  the export first wrote a name and the rules and nothing else, so every rule
  of a customisation went out under its name without its author's terms. The
  notice of a source the file leaves out - one that brought definitions
  alone - is written after the session's own: until 2026-10-07 it was
  dropped with its source, by a rule of the managers' own;
- the colours of the session's customisation that the rules NAME - a rule's
  colour, its symbol's and its text's, the three places the lint looks - so
  that a code coloured "sui water potable" is that colour where the file
  goes; the table of colours is then a source of the file too;
- NOT the session's definitions, and nothing of its linework codes or its
  automation switches: a file of codes for a colleague must not reset their
  control codes. Nor what the session is based on: a file of codes is not an
  edition of the built-in.

It reads back as the buffer, rule for rule
(`ExportedCodesAreAKatanaCustomisationOfTheRulesAloneThatReadsBackAsTheBuffer`),
and a session of four customisations is written byte for byte as worked out
by hand
(`ExportedCodesCarryTheSessionsNoticeItsRuleSourcesAndTheColoursTheRulesName`).
*Rejected: writing the session's whole customisation with the buffer's rules
in it.* What an edited buffer is, is rules; the session's definitions are
not being edited here and belong to `CUSTOMISE EXPORT`. **Export Code List
CSV...** writes `cad::codeListCsv` of the buffer.

These two were "Import Code File..." and "Export Code File..." (`importMapfile`,
`exportMapfile`), reading and writing another program's survey code file,
until 2026-10-06. Not done:

- Apply does not KEEP. It calls the commit hook round its commit, before
  `setSurveyMap` and what the hook hands back after
  (`CustomisationContext::beginCommit`), but the workbench leaves the hook
  empty, so the session is left "not kept" (`docs/desktop.md`, "The
  definition editor", "Not done").
- **The buffer does not know where its rules came from.** Import Codes takes
  a file's rules and nothing else, so the file's notice and its sources stop
  at the buffer: after Apply (`Document::setSurveyMap`, an edit) the session
  holds the imported rules and lists no source for them, and an export then
  names the sources of the session's OWN rules. An author's notice that came
  with imported rules is therefore not carried on. It needs the buffer to
  keep the sources its imports brought and Apply to record them
  (`Document::recordCustomisationLoad`), which belongs with the work that
  wires Apply to `CUSTOMISE KEEP`. Until then a customisation whose notice
  must travel is loaded whole, with the `CUSTOMISE` line.
- An export is the manager's own work, not a `CUSTOMISE EXPORT ... CODES`
  line, which writes the SESSION's rules and cannot be handed a buffer.

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
(D3), and no button is a default, so Enter in a field presses nothing. A
colour field lists the names the session's customisation defines and then
`entity::standardColourNames()` - each from the table that resolves it
(`cad::resolveColour`), so the dialog offers exactly what is drawn and keeps
no copy of either - and is filled again when the customisation's colours
change, keeping what it shows. Not done: the dialog takes about 2.4 s to build in a Debug build
on the reference map, most of it the two pickers' pictures; its linestyle
preview draws a linestyle small in the middle of its pane and its symbol
preview is a blank white pane; its own `linestyleState`
(`code_manager_support.cpp`) is a plain / defined / wrong-kind rule that does
not read `cad::linetypeStatus` ("Saying whether it is working"); the
linework summary is not pluralised ("1 lines"); and the Code Table's Text
chip has the objectName of the search field beside it, `filterText` (a chip
is named after its first label), so the two are told apart only by their
type.

## Saying whether it is working

"The linestyles are not showing" has several causes that look identical: no
customisation loaded, a customisation that does not define what this drawing
names, a style whose linetype names a symbol, or a drawing whose styles are
the plain continuous line's names `0` and `1`. Loading one now reports which. A drawing with
one style whose linetype is `CULT Bollard`, a `mode vertex` symbol, gives
(`katana_cli`, `IMPORT` of such a 12da and then `CUSTOMISE`, 2026-09-24):

```
0 of this drawing's 1 styles are drawn with a loaded definition (1 name one; the rest are plain lines)
  1 name is loaded as an `at vertices` symbol, not a linestyle, so a linetype naming it draws solid: "CULT Bollard"
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
