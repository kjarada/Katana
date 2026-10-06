# Customisation

A customisation is what turns a surveyor's field codes into a drawing: the
linestyle and symbol definitions, the survey code rules, the colours its names
mean and how linework is processed. `docs/survey_coding.md` says what each of
those IS and how a code is applied. This document is about the customisation
as one thing: the value that holds one and the file that keeps it.

Its first chapter, below, is the file format; its second, "The verbs", the
commands that load, write, keep and edit a customisation. The chapters on the
customisation built into the program and on Settings are added by the work
that builds them; until then `docs/survey_coding.md` describes what the
program does today.

## The format

A customisation used to be three files in another program's formats: a survey
code file (`.mapfile`, XML) and two style libraries (`.4d`). The Katana
customisation format replaces them with ONE file of Katana's own, in JSON,
holding one value: `entity::Customisation`
(`include/katana/entity/customisation.hpp`, `src/katana_entity/customisation.cpp`;
tests in `tests/entity/customisation/`).

```
core::Result<Customisation> customisationFromJson(std::string_view text);
core::Result<std::string>   customisationToJson(const Customisation&, const CustomisationWriteOptions& = {});
```

Three properties decide almost everything below:

- **Strict.** What the reader does not know, it refuses, naming the entry.
  Nothing is skipped.
- **Lossless.** A customisation the writer accepts reads back equal - every
  member, the order of the rules, and whether an optional part is present.
  What it could not write back as itself, it refuses to write.
- **Diffable.** The writer's bytes are a function of the value alone, one
  stroke and one rule a line, so two versions of a customisation compare as
  text.

**It lives in the entity layer** because two layers that may not see each
other must both reach it (`tools/check_layering.cmake`): `cad`, which loads,
merges and edits a customisation, and the archive module, where the readers
of the older files and their converter are. The JSON library is named only in
the `.cpp`; the header is text and `core::Result`, as
`include/katana/entity/serialization.hpp` is.

### A worked example

A water main drawn as a dashed line with a "W", a bollard drawn as a symbol,
and an attribute every code gets:

```json
{
  "format": "katana-customisation",
  "version": 1,
  "name": "Site",
  "colours": {
    "sui water potable": "#0070FF"
  },
  "linestyles": [
    {"name": "WATR Main", "group": "Survey/WATR", "units": "paper", "length": 12, "strokes": [
      ["move", 0, 0],
      ["draw", 8, 0],
      ["move", 10, -0.75],
      ["text", {"text": "W", "height": 1.5, "justify": "middle-centre"}]
    ]}
  ],
  "symbols": [
    {"name": "CULT Bollard", "group": "Survey/CULT", "units": "paper", "strokes": [
      ["circle", 0.75],
      ["dot", 0]
    ]}
  ],
  "codes": [
    {"key": "WM*", "sets": "feature", "layer": "SURVEY SERVICES", "colour": "sui water potable", "draw": "line", "linestyle": "WATR Main"},
    {"key": "AC*", "sets": "symbol", "symbol": {"name": "CULT Bollard"}},
    {"key": "*", "sets": "attributes", "attributes": [{"type": "text", "name": "Surveyed by"}]}
  ]
}
```

That is exactly what the writer produces for this customisation, byte for
byte (`TheWorkedExampleOfTheDocumentIsWrittenAsItIsPrinted`). The test holds
its own copy of the text, and the `docs` test (`tools/check_docs.py`) fails
when the block above and that copy differ by a character, so neither can be
changed without the other.

### The members

Every member not marked **required** may be left out, and a member left out
has the value in the "left out" column. Member names and words are matched as
written: case is not folded.

**The file.**

| Member | Value | Left out | Holds |
|---|---|---|---|
| `format` | the text `katana-customisation` | **required** | what the file is |
| `version` | a whole number, `1` | **required** | the version of this format |
| `name` | text | **required** | the customisation's name (see "The name") |
| `description` | text | empty | a line or two for whoever picks it from a list |
| `notice` | a list of lines of text | none | the author's notice - a licence, a disclaimer - carried with the data and shown to whoever uses it |
| `sources` | a list of sources | none | the customisations that went into this one |
| `basedOn` | `{"name", "digest"}` | absent | the customisation an edited copy started from |
| `colours` | an object, name to colour | none | the colour names of its own (see "Colour names") |
| `linework` | an object of seven spellings | **absent: says nothing** | the linework control codes |
| `automation` | an object of two switches | **absent: says nothing** | what is applied to survey data unasked |
| `linestyles` | a list of definitions | none | the definitions not listed as symbols |
| `symbols` | a list of definitions | none | the definitions listed as symbols |
| `codes` | a list of rules, IN ORDER | none | the survey code rules |

`linework` and `automation` left out do not mean their defaults. They mean the
customisation says nothing about them, and merged into a session it leaves
the session's own alone: a file of symbols passed to a colleague must not
reset their control codes. So `Customisation::linework` and
`Customisation::automation` are optionals, and an empty object (`"linework":
{}`) is PRESENT, with every member at its default.

**A source** - one entry of `sources`: `name` (**required**), `definitions`
(true when it brought linestyle or symbol definitions; left out: false),
`rules` (true when it brought survey code rules; left out: false), `notice`
(its own notice, a list of lines). A customisation merged from two keeps both
names here, which is what a project's record of what it was drawn with is
made from.

**`basedOn`**: `name` and `digest`, both **required** when it is there. The
digest is `customisationDigest` of the bytes of the file the copy started
from: FNV-1a, 64-bit, as 16 hexadecimal digits in lower case. It tells two
editions of one name apart and guards against nothing deliberate.

**`linework`**: `start`, `end`, `close`, `arcStart`, `arcEnd`, `join`,
`rectangle`, each the spelling of that control (left out: `ST`, `END`, `CL`,
`BC`, `EC`, `JPN`, `RECT`; empty: that control is off). **`automation`**:
`codesOnSurveyImport` and `lineworkOnSurveyImport`, true or false (left out:
true).

**A definition** - one entry of `linestyles` or `symbols`. The list it sits in
is what sets `LineStyle::symbol`; nothing inside it says so.

| Member | Value | Left out | `LineStyle` |
|---|---|---|---|
| `name` | text | **required** | `name`, once across BOTH lists |
| `group` | text, a `/`-separated path | empty | `group` |
| `units` | `world`, `paper` or `twoPoint` | `world` | `units` |
| `atVertices` | true or false | false | `atVertices` |
| `length` | a number, not negative | 0, "not said" | `length` |
| `factor` | a number above 0 | 1 | `factor` |
| `origin` | `[x, y]` | `[0, 0]` | `origin` |
| `anchors` | `[[x, y], [x, y]]` | both `[0, 0]` | `anchor1`, `anchor2` |
| `stretchMode` | a whole number | 0 | `stretchMode` |
| `cycleMode` | a whole number | 0 | `cycleMode` |
| `from` | text | the file's `name` | `source` |
| `strokes` | a list of strokes, in order | none | `strokes` and `texts` |

`from` is the name of the customisation a definition came from, written only
when it is not the file's own name. Empty text is allowed and means "made in a
session, of no customisation", which is `LineStyle::source` left empty.

**A stroke** is a list: its kind, then that kind's values. Coordinates and
radii are in the definition's units and angles in degrees, counter-clockwise.

| Stroke | Values | Meaning |
|---|---|---|
| `["move", x, y]` | two numbers | pen up to the point |
| `["draw", x, y]` | two numbers | pen down to the point |
| `["arc", r, start, end]` | three numbers | an arc about the CURRENT point; the radius keeps its sign |
| `["circle", r]` | one number | a circle about the current point |
| `["dot", r]` | one number | a dot at the current point; 0 is the smallest drawable |
| `["pen", "name"]` | a colour name | the colour of what follows; `view_colour` is the entity's own |
| `["text", {...}]` | one object | characters at the current point |

The object of a text stroke: `text` (empty), `angle` (0), `height` (0; not
negative), `justify` (empty; kept as written, such as `middle-centre`), `font`
(empty), `widthFactor` (1), and `extra`, three numbers `[a, b, c]` (all 0)
that are kept and not interpreted (`StrokeText::unnamed`;
`docs/survey_coding.md` says why).

**A rule** - one entry of `codes`. Any member may sit on a rule of any `sets`,
as the model allows; which members a rule of each kind is expected to carry is
a matter for the survey code lint, not for the format.

| Member | Value | Left out | `SurveyRule` |
|---|---|---|---|
| `key` | text: a code, or a prefix ending in `*` | **required** | `key` |
| `sets` | one of the nine words below | **required** | `section` |
| `layer` | text, a layer path | empty | `model` |
| `colour` | text, a colour name | empty | `colour` |
| `draw` | `line` or `point` | absent | `breakline` |
| `linestyle` | text, a definition's name | empty | `linestyle` |
| `weight` | TEXT (`0`, `Normal`) | empty | `weight` |
| `group` | text | empty | `group` |
| `comment` | text | empty | `comment` |
| `surface` | true or false | absent | `tinable` |
| `hide` | true or false | absent | `hide` |
| `symbol` | an object | absent | `symbol` |
| `text` | an object | absent | `textStyle` |
| `pipe`, `vertexPipe`, `segmentPipe` | an object each | absent | the three pipes |
| `attributes`, `vertexAttributes`, `segmentAttributes` | a list each | none | the three attribute lists |

`sets` is what the rule is about: `feature` (where the code goes and how it is
drawn), `symbol`, `text`, `pipe`, `vertexPipe`, `segmentPipe`, `attributes`
(on the string), `vertexAttributes` and `surface` (whether it goes into a
surface). They are the words the survey code tools show for the same
sections, from one table ("The words a rule is shown by", below).

- **`symbol`**: `name` (a definition's name; empty), `colour` (empty: the
  string's own), `size` (0: the definition's own size; not negative),
  `rotation`, `offset`, `raise` (0).
- **`text`**: `style`, `colour`, `units` (text, empty), `size` (0; not
  negative), `justifyX`, `justifyY` (text, empty), `offset`, `raise`, `angle`,
  `slant` (0), `widthFactor` (1), `underline`, `strikeout`, `italic` (false),
  `weight` (TEXT, empty).
- **a pipe**: `justify`, `shape`, `size1`, `size2` (TEXT, empty); `active`
  (false).
- **an attribute**: `type`, which is `text` or `integer` (**required**);
  `name` (**required**, not empty); `value` (TEXT, empty).

Each of these four lists, like the tables above and the members of a source,
of `basedOn`, of `linework`, of `automation` and of a text stroke's object, is
in the order the members are WRITTEN in ("Layout", rule 6).

**What the numbers may be.** Any number a double holds, except that a
`length`, a text stroke's `height` and the `size` of a symbol or of a text
are not negative and a `factor` is above 0 - each refused by
`entity::validate`, in its own words - and that `stretchMode` and `cycleMode`
are whole numbers a 32-bit integer holds. A number being *finite* is not a
rule of the file at all - JSON has no way to write one that is not - but see
"Numbers a double cannot hold" below.

**Text that stays text.** `weight`, a pipe's sizes and an attribute's value
are text because they are not always numbers: a weight is `0` in one rule and
`Normal` in another, and a size or a value of `$PipeDiameter` names another
attribute, to be resolved against the entity the rule is applied to. Written
as numbers they are refused, not converted.

**Absent is not a default.** Where the table says "absent" the model holds an
optional, and the two states differ: `SurveyMap::lookup` takes a rule's
`symbol`, `text` or pipe WHOLE from the first rule that has one, so an empty
`"text": {}` still stops a less specific rule's from showing through. The
writer therefore writes a present optional even when everything in it is at
its default, and the reader keeps `{}` present. Inside those objects the
model has no such distinction - a `size` of 0 and no size are one state in
memory, as they were in the files this replaces - so a number left out there
means "not said" and is the 0 the table gives.

### The words a rule is shown by

The words in the table below are the same in a file and in what the program
shows: the nine `sets` words, `layer`, `surface`, and `at vertices` for a
definition's `atVertices`. `rule #2 KT* (feature)` in a reply, `feature` in
the Survey Code Manager's Section list and `"sets": "feature"` in a file are
one thing by one name, so a person told that a `feature` rule has no `layer`
knows which member of which entry to write. That is true of these words and
not yet of every member: some are still shown by another word ("Not done").

They used to differ: the survey code tools showed the element names of the
survey code file (`.mapfile`) the rules were once read from, and a symbol's
placement by the style library's (`.4d`) keyword. The words changed on
2026-10-06 by this table, which is where every test expectation that changed
with them was taken from:

| Shown until then | The word now, in the file and in the window | What it names |
|---|---|---|
| `map_data` | `feature` | a rule about where the code goes and how it is drawn |
| `vertex_symbol_data` | `symbol` | a rule about the symbol at each point |
| `vertex_textstyle_data` | `text` | a rule about how the code's text is drawn |
| `pipe_data` | `pipe` | a rule that draws the string as a pipe |
| `vertex_pipe_data` | `vertexPipe` | the same at each vertex |
| `segment_pipe_data` | `segmentPipe` | the same on each segment |
| `string_attribute_data` | `attributes` | a rule that puts attributes on the string |
| `vertex_attribute_data` | `vertexAttributes` | the same on each vertex |
| `tinable_data` | `surface` | a rule about whether the code goes into a surface |
| `model`, a rule's field | `layer` | the layer the code's entities go on |
| `tinable`, a rule's field | `surface` | whether they go into a surface |
| `mode vertex`, of a definition | `at vertices` | drawn at each vertex of a string, not along it: `atVertices` |
| `a vertex symbol`, where a text says what a name is | an `at vertices` symbol | the same definition, named as a thing |

So `CODE EXPLAIN` answers `layer: SURVEY SERVICES  <- rule #0 WM* (feature)`,
the lint's kind is `no layer` and its message `a feature rule with no layer`,
the code list's last column is headed `surface`, and a symbol is `defined, at
vertices` - in the reply and in the Survey Code Manager's explanation alike.
In the window the Code Table's chips are Feature and Surface and the Symbol
Library's is At vertices.

The last row is one state that had two names once `mode vertex` had gone: a
linetype naming such a definition was "an `at vertices` symbol, not a
linestyle" in the Style Manager's "Drawn as", and "a vertex symbol, not a
linestyle" in `STYLE SET`'s refusal, the Survey Code Manager and the preview's
notice. They all say the first now.

Three texts were reworded past the table, because the word-for-word change
read badly:

- the lint's `model "A//B" cannot become a layer` is `layer "A//B" is not a
  layer path`. Word for word it is "layer ... cannot become a layer"; and
  "not a valid layer" is what the Survey Code Manager's Layer field says of
  its two GOOD states, so the refusal shown under that field may not hold it;
- its `a symbol (mode vertex), drawn at vertices rather than along the line`
  is `a symbol (at vertices), drawn at each vertex rather than along the
  line`, so the phrase is not said twice;
- the rule form's `Goes into a TIN` is `Goes into a surface`.

**One table for the nine section words**: `entity::kSurveySectionWords`
(`include/katana/entity/survey_map.hpp`). `entity::toString(SurveySection)`
reads it for every reply and label, and this format's reader and writer read
it for `sets`, so the two cannot come to differ; a word changed in it is a new
version of the format ("What may never change without a new version"). The
Code Table's chips are built from it too - a chip says the word of the first
section it stands for, with a capital - and not typed beside it, where a copy
would go on showing the old word after the table changed.
*Rejected: a second table of words for display* - "Layer and colour" for
`feature`, say. Two vocabularies were the defect: what the window calls a
thing is then not what a person may type.

**The older files keep their own words, with their own code.** A survey code
file is still read and written with `<map_data>` and a style library with
`worldstyle`; those words are in a table beside that reader and writer
(`src/katana_archive12d/customisation_words.hpp`), in both directions. The
three functions that held them in the entity layer for the files' sake - the
parse of a section's element, and a definition's kind word both ways - are
gone from it: their only callers were those readers and writers.

**What did not change**: C++ names (`SurveyRule::model`,
`SurveySection::Map`, `LintKind::NoModel`), property keys, and the objectNames
tests and headless scripts find widgets by - `ruleModel`, `ruleTinable`, and
the chips `filterMap`, `filterTinable` and `filterVertexmode`, which keep the
name their first label gave them while showing the new one
(`FilterBar::setChipLabel`).

### The reader is strict

`customisationFromJson` refuses, and says which entry and which member:

| What | Error | Example of the message |
|---|---|---|
| text that is not JSON, or whose `format` is not this one's | `ParseFailure` | `not a Katana customisation file` |
| a `version` newer than this build reads | `Unsupported` | `the customisation was written by a newer version of Katana` |
| a member the format does not have | `ParseFailure` | `codes[57] "WM*": unknown member "linesytle"` |
| a member given twice in one object | `ParseFailure` | `top level: member "codes" is given twice, ...` |
| a value of the wrong kind | `ParseFailure` | `linestyles[3] "T Gate": "stretchMode" must be a whole number` |
| a word outside its list | `ParseFailure` | `codes[2] "AC*": "sets" is "symbols", which is not one of ...` |
| a required member missing | `ParseFailure` | `codes[4]: has no "key", which it must` |
| a stroke of the wrong length or kind | `ParseFailure` | `symbols[0] "TEST Valve" strokes[9]: "arc" takes ...` |
| a number a double cannot hold | `ParseFailure` | `linestyles[0] "A": "length" has a number too large to hold` |
| a name a project could not record, or one that could not be told from another | `InvalidArgument` | `top level: "name": a customisation name cannot hold ...` |
| two definitions of one name | `InvalidArgument` | `symbols[1] "A": the file holds two definitions of this name; ...` |
| two colour names with one fold, or one that is a standard name | `InvalidArgument` | `colours "Red": a standard colour name cannot be given another colour ...` |
| whatever `entity::validate` refuses of a definition, a rule or the linework codes | `InvalidArgument` | `codes[1] "W*M": a survey code key may only use ...` |

`ParseFailure` is for text that is not in the format's shape, `InvalidArgument`
for text that is and still does not describe a customisation.

**How an entry is named.** The list it is in, its place counted from 0 as a
JSON path counts, and its own name in quotes: `codes[57] "WM*"`,
`symbols[0] "TEST Valve"`, `sources[1] "Site"`. A part of an entry follows it:
`codes[3] "AC*" symbol`, `codes[3] "AC*" attributes[1]`,
`symbols[0] "TEST Valve" strokes[9]`. The members of the file itself are
`top level`, and `basedOn`, `linework`, `automation` and `colours "<name>"`
are named as such. The place and the name are both given because a rule's key
alone is ambiguous - several rules share one - and a place alone says nothing
to the person who wrote the file. An unknown member's refusal carries, as the
error's context, the members that entry does have.

A stroke is as far as a place goes. Whatever is wrong inside one - an unknown
member of a text stroke's object, a member given twice there, a number in its
`extra` - is refused under `... strokes[9]`, by the member; nothing is named
`strokes[9][1]`.

`colours` is an object, and the members of a JSON object have no order: the
reader goes through its names in the order of their BYTES, whatever order the
file has them in. So of two names with one fold it is the later in that order
which is the entry refused - `colours "sui_gas"` for `SUI Gas` and `sui_gas`,
either way round - with both spellings beside it, the earlier first. The same
holds for which of two unknown members is the one named.

**Why strict.** A customisation is edited by hand, and the two mistakes a
lenient reader hides are the two a person makes:

- A misspelt member. Skipped, `"linesytle": "WATR Main"` is a rule that
  silently draws no linestyle. Worse, the program writes a customisation back
  - a copy kept for the user, an export - and the member it skipped is not in
  what it writes: the typo, and with it the intent, is gone.
- A member given twice. The JSON library keeps the LAST of two equal keys and
  reports nothing, so a second `"codes"` list pasted at the end of a file
  replaces every rule before it. The reader therefore builds the tree itself,
  from the library's parse events, and notices the second as it is put in -
  afterwards the first can no longer be seen.

A whole number is refused when given `2.7` or `true` for the same reason: the
library's own conversion cuts the first to 2 and reads the second as 1, each
another value than the file gave.

**Numbers a double cannot hold.** Two more values are JSON and are not read
as another value:

- *Too small.* `1e-400` is below the smallest double, and the JSON library
  reads it as 0 with nothing said. Here 0 has meanings of its own - a `length`
  "not said", a symbol's `size` "the definition's own" - and a `factor` of 0
  is refused in words that blame a zero nobody wrote. So a number that is not
  zero as written and would be read as zero is refused by its member:
  `linestyles[0] "A": "length" has a number too small to hold: it is not zero,
  and would be read as 0`, with the number as written beside it. A zero
  written at length (`0e-400`, `-0.000`) is zero, and the smallest numbers a
  double does hold (`5e-324`) are read as the nearest double, like any other
  decimal.
- *Too large.* `1e400` is past the largest double, and at such a number the
  JSON library STOPS: nothing after it is read. The refusal names the entry
  and the member as for any value of the wrong kind - `linestyles[0] "A":
  "length" has a number too large to hold`, or `... strokes[9]: this stroke
  has ...` - with the number and the line and column of its last character
  beside it (`1e400 at line 2, column 45`). But the order of the checks below
  can be kept only as far as the text had been read. A file that had said its
  `format` by then is refused so (and one that had said a newer `version` is
  told it is newer); a file that had not is `not a Katana customisation file`,
  with the number and its place beside it, because that is all that is known
  of it. The writer puts `format` and `version` first, so a file it wrote, or
  one a person edited from such a file, is always the first case.

**A value nested however deep is refused like any other.** A value may nest
as deep as its text is long - 100,000 `[` are a file of 200 KB - and the
reader goes down no level of the program's stack for a level of the value:
the tree is built from parse events, the JSON library takes one apart without
recursion, and what a refusal shows of a value is written out by the reader
itself, which stops after 60 characters, where the library's `dump()` would
write the whole value first, one stack frame a level. (That was a stack
overflow at about 11,000 levels until 2026-10-06; the tests now read 100,000
in every place a value can sit.) *Rejected: a limit on depth*, refusing the
text at the eighth level, which is one more than the format has. It would
have to stop the reading there, and a file of a newer version that does nest
deeper would then be told it is not a customisation, where it should be told
it is newer.

**The order of the checks**: the text is JSON; its `format` is this one's; its
`version` is not newer (so a newer file is told it is newer, not that its new
members are unknown); no member is given twice; then the members, the
top-level ones before the lists are gone through. A number too large to hold
is the one refusal that comes before the whole text has been read, as said
above.

**Encoding.** The bytes are decoded by `core::decodeText` first, so a UTF-8
byte order mark and UTF-16, which an editor on Windows saves readily, are
read. Bytes that are neither are NOT decoded as Windows-1252, as that function
does for the formats that never said what they are: this format does say - it
is JSON, and JSON is UTF-8 - and a guess at a code page would put a wrong
character into a name silently, where a name is an identity. Such a file is
`not a Katana customisation file`, with "the bytes are not UTF-8 text" beside
it. So is one whose bytes break the promise of their own byte order mark - a
UTF-8 mark over bytes that are not UTF-8, UTF-16 with an odd number of bytes
or half a surrogate pair - with the decoder's account of what is wrong beside
it. Malformed JSON gets the same message with the line and column beside it.

A survey code file (`.mapfile`) and a style library (`.4d`) are simply not
JSON, and get `not a Katana customisation file`.

**Strokes are read as the text is parsed**, not from a tree of it. The reader
takes the JSON library's parse events (its SAX interface) and builds a tree
for everything except a definition's `strokes`, which it turns into
`entity::Stroke` values as they go by. A library is 35,000 strokes, and a tree
holds each as a list of its own with its word as text of its own - most of
what a tree of the whole file weighs, built to be read once and thrown away.
On a generated customisation of the reference one's size this took the reader
from 55 ms to 43 ("Not done" has the measurement and what is left). What a
refusal says of a stroke, and the order refusals come in, are as they would be
from a tree: the strokes' first problem is kept with its place and reported
when its definition is read.

### What the writer writes

`customisationToJson` is a hand-written emitter, not the JSON library's
`dump()`. That function gives either one line for the whole file or one line
for every number; a library of 35,000 strokes is read, compared and merged by
people as text, and one stroke a line is the unit a diff shows.

#### Layout

1. UTF-8, no byte order mark, lines ended by a line feed; the file ends
   `}` and a line feed.
2. Two blanks a level. The first line is `{` and the last `}`.
3. Each top-level member starts a line, in this order: `format`, `version`,
   `name`, `description`, `notice`, `sources`, `basedOn`, `colours`,
   `linework`, `automation`, `linestyles`, `symbols`, `codes`.
4. The entries of `notice`, `sources`, `colours`, `linestyles`, `symbols` and
   `codes` are one a line, four blanks in, with the closing bracket on a line
   of its own two blanks in. `basedOn`, `linework` and `automation` are one
   line each.
5. A definition is one line holding its members, in the order of the table
   above, up to `"strokes": [`; then its strokes one a line, six blanks in;
   then `]}`, four blanks in. A definition with no strokes has no `strokes`
   member and is one line.
6. A rule is one line, its members in the order of the table above; so is a
   source. Whatever sits inside an entry - a rule's `symbol`, its attribute
   lists, a text stroke's object, a source's `notice` - is on that entry's
   line. The members of EVERY object are written in the order "The members"
   lists them in, an object inside an entry included: a rule's `text` is
   `style`, `colour`, `units`, `size`, `justifyX`, `justifyY`, `offset`,
   `raise`, `angle`, `slant`, `widthFactor`, `underline`, `strikeout`,
   `italic`, `weight`.
7. A member is `"name": value`, one blank after the colon. On one line,
   members and values are separated by a comma and a blank, with no blank
   inside the brackets: `{"a": 1, "b": [0, 1]}`. Entries on lines of their own
   are separated by a comma at the end of the line.
8. **Order.** Definitions are in name order (by byte, so upper case first),
   each in its own list. Rules are in map order - their order is their
   precedence. Colours are in the order of their FOLDED names (`site_grey`
   before `Site Orange`).
9. **Numbers** are the shortest text that reads back as the same double
   (`core::formatExactReal`): `0`, `2.5`, `-0.25`, `0.3333333333333333`, and
   for some the form with an exponent, `1e-05`, `1e+21`. Negative zero is
   written `-0.0`: its shortest text is `-0`, which a JSON reader takes for
   the INTEGER 0, losing the sign. Whole-number members are written as
   integers.
10. **Text** is a JSON string as the JSON library escapes it: `"` and `\`
    take a backslash; a control character below U+0020 is `\n`, `\t`, `\r`,
    `\b`, `\f` or `\u00XX` with lower-case digits; `/` and everything outside
    ASCII are written as they are.
11. **What is left out.** A member at its default is not written. Always
    written: `format`, `version`, `name`; a definition's `name`; a rule's
    `key` and `sets`; a source's `name`; an attribute's `type` and `name`;
    `basedOn`'s two members; EVERY member of `linework` and `automation`
    (they are settings, and a file Katana wrote should not come to mean
    something else because a default changed under it); and an optional that
    is present, even when empty (`"text": {}`, `"hide": false`).

"At its default" is decided bit for bit for a number, so `"length": -0.0` is
written although `-0.0 == 0.0`.

`CustomisationFormat.ASmallCustomisationIsWrittenByteForByteAsTheLayoutSays`
holds a file written out by hand from these eleven rules.

#### What it refuses

`InvalidArgument`, naming the entry as the reader does, for anything that
would not read back as itself:

- a number that is not finite - JSON has no form for one (`entity::validate`
  does not look at a text's three `extra` numbers, so a library can hold one);
- text that is not valid UTF-8 (a rule's text and pipe members, a
  description, a notice and a definition's source are not checked by
  `entity::validate` either);
- a stroke carrying a member its kind does not use. The model has ONE flat
  stroke for all seven kinds and lets any member be set on any of them; a file
  has a place only for the members the kind uses, so a `move` with a radius
  would come back without it. Compared bit for bit: a stray `-0.0` is refused
  too;
- a definition whose `texts` are not exactly its text strokes in order. The
  model keeps a text beside the strokes, named by index; the file keeps it
  INSIDE its stroke, where a person looks for it. A text no stroke places, one
  placed twice, or two placed in the other order has no form in the file;
- an attribute whose type is neither `text` nor `integer`;
- and everything the READER would refuse: a bad name, a bad digest, linework
  codes that `entity::validate` refuses. The reader and the writer call one
  function for these, so the writer cannot write a file the reader will not
  open - which matters most for the copy the program keeps for a user.

So "lossless" is literally true of what is written, and what it is not true
of is refused rather than written approximately.

#### What a write holds

`CustomisationWriteOptions` chooses among the definitions and the rules:
`linestyles`, `symbols` and `codes` each on or off, and `only`, a list of
definition names. A name in `only` that the library lacks fails the write
(`NotFound`), and so does one whose kind is switched off (`InvalidArgument`):
a definition asked for by name is never dropped silently. The name, notice,
sources, colours, linework and automation are written as the value has them;
a caller that wants one left out clears it in a copy first - an export of two
symbols for a colleague should not carry the session's control codes.

### One definition as text

For an editor, one definition goes to and from the format's text alone:

```
core::Result<std::string> definitionToJson(const LineStyle&, std::string_view customisation = {});
core::Result<LineStyle>   definitionFromJson(std::string_view text, bool symbol, std::string_view customisation = {});
```

The text is the very object a file holds, written by the same code with the
file's indent taken off, and no line feed after it. What a file says of a
definition from outside it is given as arguments: `symbol` is the list it
would sit in, and `customisation` the name it would sit under - `from` is
written only when the source differs from it, and a text with no `from` reads
as it. Reading runs `entity::validate`, since no library is there to. The
entry is named `definition "<name>"`.

### Editing a definition

In the window one definition is made, changed, copied or deleted in the
definition editor (`docs/desktop.md`, "The definition editor";
`src/katana_qt/customisation/definition_editor.hpp`), reached from the Symbol
Library and from the Linetypes tab of Styles and Linetypes. What it shows of
a definition is this format and nothing beside it:

- **the members as fields**: `name`, `group`, the list it sits in (Kind),
  `units` by its three words, `atVertices`, `length`, `factor`, `origin`,
  `anchors`, `stretchMode` and `cycleMode`; and `from`, shown and not edited.
  A number is the text the writer would write for it (rule 9 of "Layout"),
  and an empty one is the member left out;
- **the `strokes` as the lines a file holds**, one stroke a line, read by
  `definitionFromJson` as they are typed. The table of strokes under "The
  members" is therefore the editor's reference as much as the file's, and a
  line can be carried from one to the other unchanged.

Two departures from a file, both in the strokes box and neither in what is
saved: the comma that ends a line may be left off (the lines are joined with
commas before the reader sees them), and a blank line is passed over. A line
that would END the list and give the definition a member - valid JSON, since
a definition is one object - is refused: members are set in the fields.

What the format decides, the editor does not decide again:

- **What a definition may be** is the reader's answer. The message under the
  form is its refusal, or `entity::validate`'s, in the words under "The
  reader is strict", with the line of the box in front: `Line 3: definition
  "TEST Valve" strokes[2]: "drow" is not a kind of stroke ...`. The line is
  found by reading each line alone, since the reader names a stroke by its
  place and not by a line; and the line and column the reader ends some
  refusals with are left off, being of the joined text and not of the box
  (`cad::readStrokeText`, `include/katana/cad/definition_edit.hpp`).
- **A name is fixed once the definition exists.** A name is a definition's
  identity - "once across BOTH lists" - and what a rule's `linestyle`, a
  rule's `symbol` and the drawing's styles and layers hold of it. Nothing
  follows a changed name, so the editor does not offer one; Duplicate copies
  a definition under a new name. The format takes any name; the editor does
  not MAKE one that no command line could name the definition by - a double
  quote in it, or the words `CODE` and `FORCE` alone - since such a
  definition could not be removed again (`cad::removeDefinitionLine`).
- **Which list holds it is a field**, Kind, and is what sets
  `LineStyle::symbol` - as the list does in a file.
- **Where it came from is kept.** `from` is not edited: a definition changed
  in a session still came from the customisation it came from, and a copy of
  it did too. One made in the editor has none ("Made in this session"), which
  a file writes as `"from": ""` under a named customisation.

**Removing a definition asks who names it.** `cad::definitionUsers`
(`include/katana/cad/definition_users.hpp`) lists the rules that name it as
their `linestyle` or as their `symbol` - each by its index, its key and its
`sets` word, `rule #1 AC* (symbol)`, since a rule has no identity but its
place - and the styles and layers of the drawing that name it. Any rule is
asked, whatever its `sets`: a member may sit on a rule of any kind. A name
counts as it is drawn: itself, and the earlier name of a definition that was
renamed, where the library has no definition of that earlier name. The
answer also says what ELSE answers to the name - the drawing's own linetype
of it, or the built-in shape of it - because that is what the users are
drawn with once the definition is gone; only where neither does is a line
then plain and a point a stand-in mark. The editor's Delete hands the line
`CUSTOMISE REMOVE "<name>"` to the window's executor and shows what it
answered; when it was refused and something names the definition it shows
this list and offers the same line with `FORCE`.

A commit is the whole library installed again with the one definition added
or replaced. There is no undo of it, and no command that makes one
definition: on a command line a definition is written in a file and the file
loaded.

### Colour names

`include/katana/entity/colour_names.hpp`. A rule, a symbol and a pen NAME
their colour; two things turn a name into a colour.

**The standard names**: 27 of them - 14 plain, 7 dark, 6 light - at the RGB
the same names have in the CSS list, with two departures stated beside the
table (`green` is the primary, and the greys sit either side of `grey`).
`entity::standardColour`, `entity::standardColourNames` and
`entity::nearestStandardColour` are that table; it was private to the archive
module, and moved because `cad` resolves a customisation's colours and may
not see that module. The archive module's three names for them
(`include/katana/archive12d/domain.hpp`) are using-declarations: the entity
functions themselves, not three that forward. *Rejected: forwarding
functions.* `nearestStandardColour` takes an `entity::Color`, so a plain call
inside the archive namespace finds the entity function through its argument
as well as the archive's own, and two functions of one signature make that
call ambiguous in any file that sees both headers; one function found twice
does not.

**A customisation's own table**, `colours`: `"sui electricity": "#FF7F00"`. A
colour is `#RRGGBB`, or `#RRGGBBAA` with an opacity, in either case of digit;
it is written in upper case. `entity::ColourTable` refuses

- a name that is empty, or nothing but blanks and separators, or not UTF-8;
- two names with one fold: a lookup could not say which was meant;
- a name whose fold is a STANDARD name. An archive import colours its strings
  from the standard names alone, having no customisation to ask, so a table
  that redefined `red` would draw two reds in one drawing.

**One fold.** A colour name is compared after `entity::foldColourName` and in
no other way. In this order: ASCII letters to lower case; `_` and `-` read as
a blank; blanks at the ends removed; and `gray` read as `grey` where it is
the whole name or its last word (`Dark_Gray` is `dark grey`). Blanks inside a
name are kept, so two are not one.

The order matters at the ends. The fold came from the archive import, which
trimmed FIRST and so left `red_` as `red ` - a "folded" name with a blank at
its end, which folded again was `red`. Two answers from one fold is not a
form to compare names in: a table could hold `red_` beside the standard `red`
it is another spelling of, and `sui gas_` beside `sui gas`. Since 2026-10-06
the separators become blanks before the ends are trimmed, so a separator at
an end goes as a blank there does, and folding a folded name changes nothing
(`FoldingAFoldedNameChangesNothing`). The one thing this changes for what
existed before: an archive whose colour is named `red_` is now drawn red,
where that name used to match no standard colour and leave the colour alone.

**One resolver.** `entity::resolveColour` asks the table and then the standard
names, and gives nothing for a name neither knows - which leaves a colour
alone rather than guessing (`pen 035` is such a name, and a pen that does not
resolve draws in the entity's own colour).

### Linework codes

`include/katana/entity/linework_codes.hpp`. The spellings of the seven
linework controls are part of a customisation - a team writes `ST` or `S` as
its field book does - so `LineworkCodes` and its `validate` moved from `cad`
to this layer, where the file is read. `cad::LineworkCodes` is now an alias
and `cad::validate` a using-declaration of `entity::validate`, so nothing
that named them changed - `cad::validate(codes)` from outside, and a plain
`validate(codes)` inside the `cad` namespace. *Rejected: a `cad::validate`
that forwards*, which is what it first was. The codes are an entity type, so
the plain call finds `entity::validate` through its argument as well, and
beside a second function of the same signature it does not compile
(`tests/cad/customisation/test_linework_code_names.cpp` holds both spellings
of the call to compiling). It is the trap `docs/geometry.md` records for the
chording functions, where the `cad` names were deleted; here the names are
wanted, so they are made to name the one function.

`entity::lineworkCodeMembers` lists the seven by the names the file, a refusal
and a command use, so that the three cannot come to disagree about one. It is
the only list of those words: `cad` had the same seven in a function from its
control enumeration to text, whose last caller was the `validate` that moved,
and that function went with it.

### The name

`validateCustomisationName`: not empty, valid UTF-8, with no line break, `/`
or `\`, no control character (U+0000 to U+001F and U+007F) and no blank at
either end.

The first part is what the project's store refuses. The name is written into
a project as the record of what the drawing was drawn with, one name a line
and never a path; a name that passed here and failed there would make every
SAVE of the session fail.

The second part - control characters, and a blank at an end - the store would
take, and it is refused here because the name is an IDENTITY. A project is
matched to its customisation by it, and a list shows it: `NSW ` and `NSW`
would be two customisations that look like one, and a tab, an escape or a NUL
cannot be seen where the name is shown (an escape sequence in a name would
also be acted on by a terminal a reply is printed to). It is the rule a
layer's name is held to (`validateLayerPath`), and it is in version 1 from
the start because a rule added later would refuse files that exist by then,
where one relaxed later refuses nothing. A blank INSIDE a name is part of it,
and only the plain blank is looked for at the ends: a no-break space there is
kept, as it is in a layer's name.

The same rule holds for a source's name, for `basedOn`'s and for a
definition's `from` when it is not empty, since each of them reaches that
record. One consequence: the style library reader stamps a file's name on its
definitions as it is, so a library read from a file whose name begins with a
blank holds definitions this format refuses to write, naming each and its
`from`, until they are given another source.

### What may never change without a new version

A version 1 file must mean in every later build what it means now, so none of
these changes while `version` stays 1:

- the value of a member left out - every "left out" above, the seven linework
  spellings included (which is one reason the writer writes them all);
- a stroke's values: how many, their order, their units (degrees, the
  definition's own units);
- a word: `world`, `feature`, `line`, `move`, `text`... A word ADDED is a new
  version too, because an older build would refuse the file for it;
- a new member, for the same reason;
- the fold of a colour name, and the set of standard names (a file that
  defines a name of its own is refused the day that name becomes standard);
- what a name, a key or a number may be: a narrower rule would turn away
  files that exist, and a wider one would let this build write, as version 1,
  what an older build refuses;
- what makes two rules the same rule in a merge: `sets` and `key` together.

A build reads its own version and every older one, and refuses a newer file
(`Unsupported`) rather than read it wrongly and write it back without what it
did not understand. Forward compatibility is the version number alone; there
is no "unknown members are skipped" to lean on.

### Decisions, and what was rejected

**One flat, ordered list of rules** - not one object per code. Order is
precedence and identity: `SurveyMap` resolves a field from the first rule that
sets it, a merge matches rules by `sets` and `key`, and a rule is cited by its
index. *Rejected: one object per code*, gathering everything said of `WM*`
under one key. Several rules share a key in practice - 213 keys of one of the
reference survey code files have more than one rule, and with the second file
merged in 446 rules repeat an earlier one field for field - and such an
object could hold only one of them; a JSON object's members have no order and
may not repeat.

**The rules are kept as they are, duplicates included.** The lint reports
those 446 duplicate rules and 257 shadowed ones in the reference
customisation (`docs/survey_coding.md`). *Rejected: removing them on
conversion.* It would change the count a conversion is checked against
(1,624, by a census that uses none of this program's code) and every later
rule's index; and whether a shadowed rule matters depends on what it is later
merged with - remove the rule that shadows it and it speaks. Reporting them is
the lint's work, and deciding is the owner's.

**Strokes as short lists with their numbers verbatim.** *Rejected: SVG path
strings* (`"M0 0 L8 0"`). A definition is not an SVG path: an arc here is a
radius and two angles about the current point, a pen and a text have no path
form, and a negative radius must survive as written. A path string would also
be a second grammar inside the JSON, with its own number parser, where every
real in a file must go through one exact conversion. A text stroke is the
exception that takes an object: it had nine values by position, of which
nobody could say which was which.

**The reader refuses what it does not know.** *Rejected: skipping unknown
members*, which is what the sheet set's JSON does and what makes a format
easy to extend. Here it would hide a typo and then lose it from the copy
written back ("Why strict" above).

**A customisation is data in this one format wherever it comes from.**
*Rejected: generating C++ tables* from a customisation to compile one in. That
is a second representation with a second writer, it makes a script a build
requirement, and a customisation built in could then not be exported, edited
and read back as the same thing.

**`key` and `sets` are required.** A rule's `sets` has no default because
`sets` and `key` together are its identity in a merge: a symbol rule whose
`sets` was forgotten would be read as a `feature` rule and replace that key's
layer and colour rule.

**A definition says where it came from; the list says what it is.** `symbol`
is a field of the definition (`LineStyle::symbol`) set by the list it sits
in, and is not worked out from `source` as it used to be (a file name holding
"symbol"): one customisation holds both kinds under one name. The style
library reader, which still reads one kind a file, sets the flag by that
file-name rule; the catalogue and the lint still ask the name itself and are
moved to the flag by the work that follows this.

**An attribute's type is two words.** The model keeps it as text; the format
holds `text` or `integer`, the two kinds an attribute can be set as.
Anything else is refused by the reader and by the writer.

**No comments.** A file is strict JSON, with no `//` lines; what an author has
to say goes in `description`, `notice` or a rule's `comment`, where it
survives being read and written.

### Not done

- **The two programs do not read a file of it from their command lines
  yet.** The verbs that read and write one are in the interpreter ("The
  verbs", below), and the Symbol Library and the Survey Code Manager read
  and write one from their own buttons (`docs/desktop.md`), but `katana_cli`
  and the window still take a `CUSTOMISE` line themselves, and neither
  starts a session from one (`docs/survey_coding.md`, "What a session starts
  with"); Settings comes with the work that follows.
- **`-0` reads as 0.** Negative zero written without a fraction is an integer
  to the JSON library. The writer never writes it so; a person might.
- **Some of a rule's members are still shown by another word.** `CODE
  EXPLAIN` and the Survey Code Manager's explanation say `breakline: Line` or
  `Point` for what a file holds as `"draw": "line"` or `"point"`, `text style`
  for `text`, `vertex pipe` and `segment pipe` for `vertexPipe` and
  `segmentPipe`, and call an entry of `attributes`, `vertexAttributes` or
  `segmentAttributes` a `string`, `vertex` or `segment` attribute
  (`fieldSpecs` and `attributeScopes` in
  `src/katana_cad/customisation/code_table.cpp`). They were not among the
  words that changed ("The words a rule is shown by"), so for these a person
  writing a file from an explanation needs the table of members above.
- **One refusal still says `model`.** Linework that meets a rule whose layer
  is not a layer name answers "the survey map gives a code a model that is
  not a valid layer name", with `model=` beside it
  (`src/katana_cad/customisation/linework.cpp`). No map can hold such a rule
  (`SurveyMap::add` refuses it), so nobody is shown it; it was left because
  that file was being rewritten by other work when the words changed.
- **Explicit zero against "not said"** is not kept for a rule's symbol and
  text numbers, because the model does not keep it (above, "Absent is not a
  default").
- **A source may be named twice** in `sources`; the format keeps the list as
  it is and leaves what that means to the session that made it.
- **A number too large, in a file that says what it is afterwards.** The
  reading stops at such a number ("Numbers a double cannot hold"), so a file
  whose `format` comes AFTER it is told it is not a customisation, with the
  number and its place beside it, where the same file with `format` first is
  told which member holds it. Reading on would take a JSON reader of this
  format's own.
- **The style manager keeps a second table of standard colours.** The colour
  menu of Styles and Linetypes (`src/katana_qt/style_manager.cpp`,
  `kStandardColours` there) lists twelve colours "by the standard colour
  names" from a table of its own, and two of them are not the standard
  colour: its `orange` is 255, 128, 0 where the standard one is 255, 165, 0,
  and its `brown` 150, 75, 0 where the standard one is 165, 42, 42. A style
  given "orange" from that menu is therefore not the orange a rule's
  `"colour": "orange"` draws. It should be filled from
  `entity::standardColourNames`; left because the window's behaviour belongs
  to the work that gives it Settings.
- **No benchmark that can be repeated yet**, and the reader is slower than the
  readers it replaces. Measured once, on 2026-10-06, with a throwaway program
  (Release, GCC 16, best of 15 runs) on a GENERATED customisation of the
  reference one's size - 792 definitions, 35,684 strokes in the reference
  mix with coordinates of three decimals, 1,624 rules, 1.7 MB of text:

  | | Before | With strokes read from the parse events |
  |---|---|---|
  | writing | 8 ms | 8 ms |
  | reading | 55 ms | 43 ms |

  Of the 43 ms the JSON library's own lexing is 28, and 27 of those are the
  definitions' text: about 71,000 numbers, each handed by the JSON library to
  the C library's `strtod`. Timed alone on numbers of that shape, `strtod`
  takes 305 ns a number here (22 ms for the 71,000) and `std::from_chars` 13
  ns (0.9 ms). The rest of the 43 is 6 ms for the strokes and 6 ms for the
  rules, which is allocation. The older readers were measured at 21 ms on the
  reference files (`docs/survey_coding.md`).

  What the reader gained later that day - a number too small kept as such, a
  number too large placed, and what a refusal shows written out by the reader
  itself - is one comparison a real number and otherwise off the path a sound
  file takes. Measured side by side with the same program, on a machine busy
  with other builds: 45.0 ms before, 44.8 after.

  So what is left to take is the number conversion, about 21 ms of it, and
  the JSON library offers no way to replace it: only a scanner of this
  format's own would. *Not done*: that is a second JSON reader in the
  program, and it should be decided on a benchmark that can be repeated, on
  the converted reference customisation - which comes with the conversion.

## The verbs

Two verbs of the shared `CommandInterpreter` work on a session's
customisation: `CUSTOMISE`, which loads, writes, keeps and edits it
(`include/katana/cad/customisation_verbs.hpp`), and `CODE`, which applies its
survey codes to a drawing and answers questions about them
(`include/katana/cad/survey_code_verbs.hpp`). Being the interpreter's, each is
ONE implementation for the window's command line, `katana_cli`, `katana_mcp`
and an agent. `HELP` lists both in brief and `HELP CUSTOMISE` prints the
family's whole reference (`cad::customisationVerbHelp`).

**Where this stands.** The family is in the interpreter and is tested there
(`tests/cad/customisation/test_customisation_verbs.cpp`,
`tests/cad/customisation/test_survey_code_verbs.cpp`). `CODE` reaches every
front end today. A `CUSTOMISE` line does not yet: `katana_cli` and the window
each take it before the interpreter sees it, with the code they had, which
still reads the older files (`src/katana_app/session.cpp`,
`src/katana_qt/main_window.cpp`). Each is moved to this family by the work
that ports it; until then what `docs/survey_coding.md` says of `CUSTOMISE
<file>` is what those two programs do, and neither hands the interpreter a
host.

### CUSTOMISE

| Line | What it does |
|---|---|
| `CUSTOMISE` | what is loaded: two count lines, then records, then what this drawing uses of it |
| `CUSTOMISE JSON` | the same as one JSON object (`cad::customisationJson`) |
| `CUSTOMISE <file>...` | merges Katana customisation files into the session |
| `CUSTOMISE REPLACE <file>...` | loads them in the place of each KIND they bring |
| `CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] [NAME <name>] [ONLY <definition>...]` | writes the session, or a part of it |
| `CUSTOMISE RESET` | the program's built-in customisation in the place of the session's |
| `CUSTOMISE KEEP` | writes the session to the kept file, which the next start reads |
| `CUSTOMISE REVERT` | reads the kept file again |
| `CUSTOMISE REMOVE <definition>... [FORCE]` | removes definitions; refused while one is in use, unless `FORCE` |
| `CUSTOMISE REMOVE CODE <key>...` | removes every rule of each key |
| `CUSTOMISE SET <key>=<value>...` | `auto.codes` and `auto.linework` (`on`, `off`); `linework.start`, `.end`, `.close`, `.arcstart`, `.arcend`, `.join`, `.rectangle` (a spelling; empty switches that control off) |

**A keyword is the whole first word**, in any case, and `CUSTOMIZE` is the
same verb: the interpreter reads the verb as it reads every verb, alias
included, and hands the family the words after it. A file that is literally
called as a keyword is given with its directory, `./json`, as a file called
as a scope word is.

**A load** reads every file with `entity::customisationFromJson`, merges them
with `cad::mergeCustomisation` and installs the result with
`Document::installCustomisation`. It is all or nothing: when one file does
not read, or the merge or the install refuses one thing, nothing is loaded
and every reason is listed. One file that does not read is refused in the
reader's own words, the file beside them - a survey code file or a style
library of another program is `not a Katana customisation file`. A file
named twice is read once, and the reply says so. What Merge and Replace each
do to definitions, rules, colours, the settings and the sources is the
merge's to say (`include/katana/cad/customisation_merge.hpp`).

**An export** writes the whole session, or a part. A kind word chooses among
the three kinds (`CODES`, `LINESTYLES`, `SYMBOLS`): those said, alone. `ONLY`
chooses among the DEFINITIONS: those named, of either kind unless
`LINESTYLES` or `SYMBOLS` says which, and no codes unless `CODES` is said
too - `ONLY "TEST Peg"` is what a window's "export the selected symbols"
means, and `CODES ONLY "TEST Peg"` is the codes with that one symbol.

| Words | Linestyles written | Symbols written | Codes written |
|---|---|---|---|
| none | all | all | all |
| `CODES` | none | none | all |
| `LINESTYLES` | all | none | none |
| `SYMBOLS CODES` | none | all | all |
| `ONLY a b` | those among a, b | those among a, b | none |
| `CODES ONLY a b` | those among a, b | those among a, b | all |
| `SYMBOLS ONLY a b` | none: a linestyle named is refused | those among a, b | none |

The file is the first word. One of `EXPORT`'s own words there (`CUSTOMISE
EXPORT CODES`) is a line whose file was left out and is refused; a file so
called is given with its directory (`./CODES`), as the family's keyword rule
has it. After `ONLY` every word is a definition: one called `NAME` is listed
by giving `NAME <name>` before `ONLY`, and a kind word put after it is
refused as the definition it is not, saying that it goes before. A file that
is there is written over, and the reply says it was (`replaced=yes`). A
session with no name - rules made in an editor with no customisation ever
installed - is refused until `NAME` gives one.

**A part** - anything a kind word or `ONLY` chose - is a fragment for
someone else's session, or for this one later:

- It is written without the linework codes, the automation and `basedOn`.
  Merged in, it must not reset anyone's control codes, and it is not a copy
  of the built-in to be told from another edition.
- It keeps the session's name unless `NAME` gives another: it is a part of
  that customisation.
- The colours go with it, whole.
- **It lists a source only for what it holds of it**: with `definitions`
  when one of the definitions written came from that source
  (`LineStyle::source`), with `rules` when the rules are written and that
  source brought some, with neither when the source brought neither kind (a
  table of colours) and there are colours to write. A source the part holds
  nothing of is left out.
- **Every notice goes with it.** The notice of a source that is left out is
  written with the part's own, after it: nothing says whose the colours
  are, and an author's notice is never dropped by an export.

Where a file is loaded its sources are taken at their word: each becomes a
source of that session by name, and a project's record of what it was drawn
with is by name too (`cad::customisationRecord`: a source said to have
brought rules is recorded for good). A part written with the session's list
as it stood said of one exported symbol that it was the whole customisation,
rules included.

**`REMOVE`** takes definitions out of the library and refuses, changing
nothing, when one of them is still named - by a survey code rule as its
linestyle or its symbol, by a style of the drawing as its linetype or its
symbol, or by a layer of the drawing as its linetype - listing every rule,
style and layer that names it. `FORCE` removes it all the same, and what
names it then draws plain. `REMOVE CODE` removes every rule of each key, in
every section; a key is matched as it is written.

**`SET`** changes the two switches and the seven control codes. Every item is
read before any is set, and the codes are judged by `entity::validate`, so
one refused item sets none of them. A key is given once in a line: given
twice it is refused, where the later value once won and the reply said both.

### The replies

Every reply is records, one a line: a leading word, then `key=value` fields
(`cad::recordValue` quotes a value that needs it; `core::readReplyRecord`
reads a line back). A file's path is written with `/` on every platform - a
backslash in a record is an escape. No reply to a line that succeeded holds
the word `error`: a script, and dozens of the command line's own tests, take
that word for a failure. (So the JSON report's count of rules that cannot be
applied is `cannotApply`. `CODE CHECK`'s reply does say "0 errors": it is the
reply that verb had as `MAPFILE CHECK`, kept word for word, and it counts as
`UTILITY CHECK` counts - see "The verbs: not done".)

```
CUSTOMISE                 the two count lines, then
                          customisation name=<n> origin=none|builtIn|kept|loaded|edited kept=yes|no definitions=<n> codes=<n> rules=<n> colours=<n>
                          source name=<n> definitions=yes|no rules=yes|no        one a source, in load order
                          automation auto.codes=on|off auto.linework=on|off
                          linework linework.start=ST linework.end=END ... linework.rectangle=RECT
                          missing name=<n>                                        one a name the open project recorded and the session lacks
                          then the coverage lines
CUSTOMISE [REPLACE] f...  repeated file=<f>                                       a file named twice
                          loaded file=<f> name=<n> definitions_added=<n> definitions_replaced=<n> codes_added=<n> codes_replaced=<n> colours_added=<n> colours_replaced=<n> linework=yes|no automation=yes|no
                          removed definitions=<n> codes=<n>                       REPLACE, when it removed any
                          removed definition=<name>   removed code=<key>          up to 20 of each
                          undefined names=<n>                                     names the rules ask for that nothing defines
                          undefined name=<name>                                   up to 20
                          customisation name=...                                  what is loaded now
CUSTOMISE EXPORT f ...    exported file=<f> name=<n> definitions=<n> codes=<n> rules=<n> replaced=yes|no
CUSTOMISE RESET           reset name=<n> definitions=<n> codes=<n> rules=<n> kept=yes|no
CUSTOMISE KEEP            kept file=<f> written=yes name=<n> definitions=<n> codes=<n> rules=<n> backup=<f>.bak|none
                          kept file=<f> written=no reason=the-session-is-the-built-in backup=<f>.bak|none
CUSTOMISE REVERT          reverted file=<f> name=<n> definitions=<n> codes=<n> rules=<n>
CUSTOMISE REMOVE a b      removed definition=<name> rules=<n> styles=<n> layers=<n>   one a name: the rules, styles and layers that still name it
CUSTOMISE REMOVE CODE k   removed code=<key> rules=<n>                            one a key
CUSTOMISE SET k=v ...     set <k>=<v> ...
```

The report's first lines are the two counts when the session holds a
definition or a rule. With neither it says so in one of two ways: `No
customisation is loaded.` when nothing has been installed (`origin=none`),
and `No linestyle or symbol definitions and no survey code rules are loaded.`
of a customisation that is there and brought neither kind - a table of
colours, a file of settings. The first once stood above the record naming
the customisation it said was not loaded.

`codes=` is distinct keys and `rules=` rules, as the count line has them
("1,624 survey code rules over 632 distinct codes"); `codes_added` and
`codes_replaced` count a key once for each section a file gives it rules in,
which is what the merge replaces by. The `automation` and `linework` records
carry the very keys `SET` takes, so either can be typed back after it.

The bare report has a second, older text, `cad::formatCustomisationSummary`,
which names a source by the kind of file it once was ("a style library"). The
front ends' own `CUSTOMISE` and File > Drawing Summary still print it, and it
goes with the last of them; the verb's text is `cad::formatCustomisationReply`,
and both are made from one `CustomisationSummary`.

`CUSTOMISE JSON` is `cad::customisationJson`: one object, keys in
alphabetical order, two blanks a level, with `name`, `origin`, `kept`,
`description`, `notice`, `basedOn`, `builtIn`, `sources` (each with its
notice), `counts`, `automation`, `linework`, `colours`, `problems`,
`coverage` and `missing`. `automation`, `linework` and `basedOn` use the
format's own member names, so the report reads against a file. `problems` is
the lint `CODE CHECK` prints, counted - `rules`, `cannotApply`, `warnings`,
`byKind` - and `undefined`, the names the rules ask for that nothing defines.
It does not hold the definitions or the rules; a file does. It is a function
so that a front end can hand a client the object without a line passing
through the command history.

### The host: RESET, KEEP and REVERT

Three of the words need what the Document does not hold: the program's
built-in customisation and the path of the file the user's own is kept in. A
front end hands both to the interpreter as a host
(`CommandInterpreter::setCustomisationHost`, `cad::CustomisationHost`), as it
hands over its views. A session given none - a test's bare Document, a tool -
has every other word, and these three refused by name; so is `RESET` in a
build with no built-in, and `KEEP` and `REVERT` where the front end keeps no
file.

`RESET` installs the built-in whole, the control codes and the switches
included, through the one function a start with no kept file uses
(`cad::installBuiltInCustomisation`). It writes nothing.

`KEEP` writes the session, as `entity::customisationToJson` writes it, to a
file beside the kept one and renames that into its place; the file that was
there stays beside it as `.bak`. The session says what it was made from
(`basedOn`: the built-in's name and the digest of its bytes, stamped when the
built-in was installed), so the kept file says it too, and a later start can
tell that the built-in has moved on.

It refuses to write over a kept file that

- **changed on disk since this session read it.** The host's kept file is
  noted, by the digest of its bytes, when the host is handed over, and again
  at each `KEEP` and `REVERT`. A second Katana that kept its own in between
  would otherwise lose it without a word. `REVERT` reads the file as it is
  now, after which it is the one this session saw; `EXPORT` first keeps what
  this session has;
- **cannot be read at all** - a folder of that name, a file that will not
  open: what is there is never written over unseen;
- **does not read** as a customisation: its owner may mean to mend it;
- **was written by a newer Katana**, which holds what this one cannot write
  back.

A session with no name is refused too, saying how it gets one (`EXPORT ...
NAME`, then `REPLACE`). A write that fails - the folder cannot be made, the
file that is there cannot be copied beside it as `.bak`, the new text cannot
be put in its place - is a `FileExportFailure` that leaves the kept file as
it was and nothing beside it; `EXPORT` writes the same way.

When the session IS the built-in, nothing is written and the kept file is set
aside as `.bak` (`written=no reason=the-session-is-the-built-in`). "Is the
built-in" is asked of the install itself - the session is compared with what
installing the built-in into an empty Document gives - so it cannot drift
from what `RESET` leaves.

`REVERT` reads the kept file again and installs it.

### CODE

```
CODE [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW]           apply the loaded codes, one undo step
CODE CENSUS [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW]    the codes those entities carry
CODE EXPLAIN <code>                                              why a code gets what it gets
CODE LIST [<filter>]                                             the loaded codes, one a line
CODE CHECK                                                       their lint; refused when a rule has an error
```

`CODE` and `CODE CENSUS` are one grammar. `PREVIEW` on `CODE` reports what
coding would do and changes nothing (`Preview: nothing was changed.`). A
census changes nothing to begin with, so the word asks nothing more of it:
it is taken, wherever it stands, and the reply is the census.

The scope and filter are the one grammar every verb on drawing data takes
(`docs/cad.md`, "Scope and filter"), read by the one parser, and the reply
begins with what the scope took (`cad::scopeRecord`): `scope=layers
layers=survey sublayers=yes matched=212`. A scope that takes nothing is
reported, and codes nothing.

**With no scope word the scope is the whole drawing - under a bare `WHERE`
too.** Every other verb reads no scope word as the selection. `CODE` was the
whole drawing before it took a scope, and it is typed in scripts and sent by
agents as that one word.

**The first word decides how the rest is read**: a subcommand (`EXPLAIN`,
`CENSUS`, `LIST`, `CHECK`); else a scope word, `WHERE`, `PROPERTY` or
`PREVIEW`, which begin the grammar above; else the property, as the whole
rest of the line, which is what `CODE feature_code` has always meant. A word
that needs the word after it and has none begins nothing: `LAYER`, `LAYERS`
and `AREA` need their list or their window, so `CODE Layer` - an attribute
many drawings from GIS data carry - is still that property, and `CODE LAYERS
a` is the layer `a`. A property called as a scope word that stands alone
(`ALL`, `VIEW`, `SEL`) or as a subcommand is given as `PROPERTY <name>`.

So is a property of several words whose first is one of those words. With a
word after it even `LAYER`, `LAYERS` and `AREA` begin the scope form, and
nothing can tell `CODE Layer a`, the layer, from a property called "Layer
a": the line is the layer, and `CODE Area m2` is refused as a window that is
not four numbers. That property is `CODE PROPERTY "Area m2"`. A property of
several words whose first is none of them is still the rest of the line.

`CODE LIST` and `CODE CHECK` were a verb of their own, `MAPFILE`, named after
the survey code file of another program. That file no longer loads; the verb
went with it and is an unknown command now. Their replies are unchanged.

**Colours.** The verbs resolve a colour name through the Document
(`cad::resolveColour`: the customisation's own table, then the standard
names). What a front end passed with `CommandInterpreter::setColourLookup` is
asked only for a name neither knows.

### The verbs: decisions, and what was rejected

**One family, in the interpreter.** `CUSTOMISE` had been written twice, by
hand, in the two front ends, because the readers of the older files lived
where `cad` could not see them. The two had come to differ: one refused a
load with a problem in it and the other installed what was left.

**A keyword is the whole first word.** *Rejected: "a quoted word is a file"*,
the rule the session's own parser kept. The command line removes quotes
before a verb sees its words, so the rule cannot move into the interpreter.
*Rejected: a `LOAD` keyword.* It would buy nothing - a word that is no
keyword is already a file - and `CUSTOMISE [REPLACE] <file>` is the form
every help text and script has.

**The fragment is the one door for an edit.** Nothing edits a rule or a
definition in place. A customisation file holding only what changes is merged
in: it replaces a definition by its name and a key's rules by their section,
which is exactly what an editor's Apply means, so a dialog writes the
fragment and runs the line. *Rejected: verbs that set one member of one
rule.* That is a second writer of every member the format already has one
writer for, and a rule has no name to be addressed by - only its place.
`REMOVE` exists because a merge cannot delete, and `SET` because the switches
and the control codes are settings, not data.

**A load is all or nothing, and every problem is listed.** *Rejected:
installing what read and reporting the rest.* Half a customisation draws a
survey with half its symbols, and nothing on screen says which half.

**A part lists only the sources it holds something of.** *Rejected: the
session's list as it stands*, which is what was first written. One symbol
exported under another name then listed its customisation with `definitions`
and `rules`; a colleague who loaded it was shown that customisation as a
source with rules it did not have, and every project they saved afterwards
recorded it for good, the symbol removed or not. *Rejected: no `sources` at
all, every notice folded into the part's own.* It is true of where the part
is loaded, but it flattens what the session knows: the codes of a session
merged from two customisations would come back as one source, so `EXPORT f
CODES`, an edit and `REPLACE f` - the round trip an agent makes - would leave
the session with other sources than it had. Narrowed, the same sources come
back, each with what it brought.
*Rejected: dropping the notice of a source that is left out.* The colours go
with every part and nothing says whose they are.

**`PREVIEW` is a word of a census too, and does nothing there.** *Rejected:
refusing it*, which is what was first written ("a census changes nothing, so
it has no preview to give"). True, and the wrong answer: a line built for
`CODE` with `CENSUS` put in became a usage error for the one word that
promises to change nothing, and `CODE CENSUS preview`, which had been a
property, became one too. *Rejected: reading a lone `preview` after `CENSUS`
as the property it was.* `PREVIEW` first would then mean one thing after
`CODE` and another after `CODE CENSUS`; a property so called is `PROPERTY
preview` in both.

**`CODE` with no scope word is the drawing.** *Rejected: the selection, as
the other verbs have it.* The same line would then code something else the
day something happened to be selected, with no word of it in a script. The
positional property is kept for the same reason. *Rejected: reading every
scope word as a scope* - `CODE Layer` would be refused for a missing layer
list where it had always worked.

**`KEEP` is said, not implied.** A typed, scripted or agent's line changes
the session; `KEEP` makes it what the next start gives. *Rejected: writing
every change at once.* One line would then last a session in `katana_cli`
and for ever in the window, and an agent trying a customisation would be
rewriting its user's.

**The host is handed over.** *Rejected: `cad` reading the built-in for
itself in `RESET`.* A session with no host - every test's - would then be
installed with whatever the machine's build happened to hold.

**A kept copy of the built-in is retired, not written.** *Rejected: writing
it.* It would be read at every later start in the place of the built-in,
and so hide every later edition of the built-in for ever.

**A kept file that changed is not written over.** *Rejected: the last writer
wins.* Two windows share one kept file, and the loser would not be told.

**The words of the new lint warning.** `CODE CHECK` prints the warning for a
field its section does not use, which named two fields by the survey code
file's words (`model`, `tinable`) where every other reply says `layer` and
`surface` ("The words a rule is shown by"). It says the format's two now.

### The verbs: not done

- **The front ends do not use the family for `CUSTOMISE` yet** ("Where this
  stands"). Until they do, `katana_cli --help` lists a Customise block twice,
  the interpreter's and the session's own, and the two disagree; and `CODE`
  with nothing loaded says `CUSTOMISE <file> loads a Katana customisation
  file`, which is true of this family and not yet of what those two programs
  run for that line.
- **A quoted verb slips past the front ends' own `CUSTOMISE`.** Each tests
  the line's first word as typed, and the interpreter removes quotes, so
  `"CUSTOMISE" JSON` reaches this family today, with no host, beside the
  older code that an unquoted line still gets. It closes when the two
  interceptions go.
- **An unnamed session has no verb that names it.** `EXPORT ... NAME <name>`
  writes it under one, and loading that file with `REPLACE` gives the session
  the name; `KEEP` refuses it until then, and says so.
- **A part still answers for its source by NAME.** A session that loads one
  symbol exported from a customisation lists that customisation as a source
  with `definitions`, and the open project's `missing` record for the name
  goes, as it would at the next open by the definition's own `from`. A
  project's record is names, and a name cannot say "a part of".
- **`FORCE` is read as the last word and `CODE` as the first** of a `REMOVE`.
  A definition called `FORCE` is removed by `REMOVE FORCE FORCE`, and one
  called `CODE` by listing it after another name.
- **Who uses a definition** is asked by a function private to the verb
  (`usesOfDefinition`, `src/katana_cad/customisation/customisation_verbs.cpp`),
  which counts the rules, the drawing's styles and the drawing's layers that
  name it. A shared function for the question is being written; this one is
  replaced by it, and what replaces it must count the layers too. An entity
  is not asked: it reaches a definition only through its style or its layer.
- **`CODE CHECK` says "0 errors" when it succeeds.** It is the reply the verb
  had under its old name, kept word for word, and `UTILITY CHECK` counts the
  same way ("0 errors, 0 warnings"); so a test of the command line that runs
  either cannot fail on the word `error`, as the others do. Rewording it is
  one line of `cad::formatLint` and every pin of that count.
- **A property of several words that begins with a scope word** is read as
  that scope ("CODE", above): it is named `PROPERTY "Area m2"`.
- **The refusal "nothing was loaded, for N reasons"** lists what the merge
  refuses of a load. A file that reads has already passed everything the
  merge looks for, so no file reaches it today and no test does.
- **Two Katanas keeping in the same instant** are not locked against each
  other: the check and the write are two steps.
- **`PREVIEW` needs the scope form.** `CODE feature_code` takes the whole
  rest of the line as the property; a preview of it is `CODE PROPERTY
  feature_code PREVIEW`.
- **`CommandInterpreter::setColourLookup` is still there**, for the front
  ends that call it; it answers nothing the Document does not.
