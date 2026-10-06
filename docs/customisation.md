# Customisation

A customisation is what turns a surveyor's field codes into a drawing: the
linestyle and symbol definitions, the survey code rules, the colours its names
mean and how linework is processed. `docs/survey_coding.md` says what each of
those IS and how a code is applied. This document is about the customisation
as one thing: the value that holds one and the file that keeps it.

Its first chapter, below, is the file format, and its last is the converter
that makes such a file from the older formats. The chapters on the
customisation built into the program, on the commands that load and edit one
and on Settings are added by the work that builds them; until then
`docs/survey_coding.md` describes what the program does today.

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
surface). These are the format's own words, kept in a table private to
`src/katana_entity/customisation.cpp`.

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

- **Nothing in the program reads or writes a file yet.** This is the value,
  the format and its tests; the commands, the built-in customisation and
  Settings come with the work that follows, and are recorded in this document
  then. The one thing that writes a file today is the converter of the older
  formats, a developer's tool ("Converting a customisation from the legacy
  formats", below).
- **`-0` reads as 0.** Negative zero written without a fraction is an integer
  to the JSON library. The writer never writes it so; a person might.
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
- **The reader is slower than the readers it replaces.** The measurement
  that can be repeated is `benchmarks/bench_customisation.cpp`, and its
  figures are under "What reading and writing cost" at the end of this
  document. What follows is the first measurement, kept for what it found:
  made once, on 2026-10-06, with a throwaway program (Release, GCC 16, best
  of 15 runs) on a GENERATED customisation of the reference one's size - 792
  definitions, 35,684 strokes in the reference mix with coordinates of three
  decimals, 1,624 rules, 1.7 MB of text:

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
  the converted reference customisation. Both now exist, and say the same
  ("What reading and writing cost").

## Converting a customisation from the legacy formats

The Katana customisation format is to be the ONE format a customisation is
read in. At this point the program itself still reads only the older ones
("Not done", above); the work that follows moves it to this format and takes
their readers out. A customisation kept in the older formats - style
libraries (`.4d`) and survey code files (`.mapfile`) - is turned into the
Katana format once, by a developer, with `katana_customisation_convert`:

```
katana_customisation_convert --name <name> [--description <text>]
    [--notice-from <file>]... [--colours <table>]
    [--strip-leading-word <word>]... [--remove-word <word>]...
    [-o <output>] <style library or survey code file>...
```

It is a tool of the build tree and not of the product. It is built with
everything else, so that it cannot rot unseen, but it is never installed
(`cmake/KatanaPackaging.cmake` installs three programs by name) and none of
`katana`, `katana_cli` and `katana_mcp` links it: what it does is a static
library of its own, `katana_legacy_customisation`
(`src/katana_archive12d/legacy/convert.hpp`, `convert.cpp`), which only the
program (`convert_main.cpp`, beside them) and the archive module's tests
link. The library publishes that one directory, so the two include
`convert.hpp` and the module's private headers stay private. The program reads
its command line and nothing more, so everything below is tested as functions
(`tests/archive12d/customisation/test_convert.cpp`):

```
core::Result<Conversion> convertLegacyCustomisation(const std::vector<LegacyFile>& files, const ConvertOptions& options);
core::Result<Conversion> convertLegacyFiles(const ConvertRequest& request);
std::string              toText(const ConvertReport& report);
```

The first takes files as bytes, the second by path - it reads them, converts
and writes `-o` - and a `Conversion` is the `entity::Customisation`, its text
as `entity::customisationToJson` writes it, and a report. The readers of the
older formats are still in `katana_archive12d`, which the library links; they
are to move into that directory when they leave the product. One of them
gained one thing to say for the converter: `StyleLibraryRead::
replacedDefinitions`, each definition a library replaced AS IT WAS, where the
reader used to give a count and nothing else.

### What a conversion does

**The order of the files is their meaning**, so they are given in load order
and never sorted. A later library wins a definition both give; an earlier
survey code file wins a field both set, because its rules come first and
order is precedence. What each file IS is decided by looking inside it, as
the loader of those files always did: the extension says nothing, `.4d` being
the extension of both kinds.

It converts from what the readers MADE of the files - a `StyleLibrary` and a
`SurveyMap` - and never from their text. *Rejected: a translation of the text
itself.* The readers apply defaults and pass over what they cannot read (an
`<item>` with no key, a rotation written as an empty element), and a second
reading of the same text would be a second opinion of what a file means.

- **`linestyles` or `symbols`.** A definition goes to `symbols` when
  `LineStyle::symbol` is set, which the style library reader does for a file
  whose NAME holds "symbol": the older format keeps one kind a file and says
  which nowhere else.
- **Every definition's `from` is the customisation.** Its source becomes
  `--name`; the files it was read from are no longer what it came from, and
  their names are not carried.
- **What a conversion cannot know, it leaves unsaid.** No `sources` (a
  customisation that lists none is its own one source), no `linework` and no
  `automation`: the older files say nothing of control codes or of what is
  applied unasked, and "says nothing" keeps a session's own when the file is
  merged into it.
- **The rules are kept as they are**, in the order read, duplicates and
  shadowed rules included ("Decisions, and what was rejected").
- **`--notice-from <file>`** (repeatable) takes that file's LEADING `//`
  comment block as the notice: the run of comment lines it opens with, each
  as written with the marker and one blank after it taken off, ending at the
  first line that is not a comment. A block the same as the one before it is
  taken once. A file asked for a notice and holding none is refused: carrying
  the author's words with the data is why one asks.
- **`--colours <table>`** gives a colour to every name a rule uses - its own,
  its symbol's, its text's - that the standard names lack. The table is text,
  a colour a line: `R G B <index> "name"`, whatever follows the name ignored.
  A name is matched by `entity::foldColourName`, so `sui_gas` in the table is
  the `sui gas` of a rule, and is written into `colours` as the rules first
  spell it. A name the table gives two colours is refused, but only when a
  rule uses it: a table of nine hundred colours need not be sound where
  nothing reads it.
- **A plot pen is never given a colour**: a name that is `pen`, digits and at
  most one letter (`pen 035`, `pen 12a`), though the table has one. Such a
  name is a pen of the plotter the files were written for, not a colour of
  the drawing. Unresolved, it leaves the entity's own colour alone
  (`entity::resolveColour`), which is how these codes have always been drawn;
  resolved, every one of them would change colour on the day of the
  conversion.
- **`--strip-leading-word <word>`** (repeatable) takes a word, with the
  blanks after it, off the FRONT of every definition name, every group path,
  every rule's `group` and every `linestyle` and symbol name a rule gives -
  so that a rule still names the definition it named. It is matched as
  written (case is not folded, as names are not), at most one word a text,
  the first given that begins it; a text that is nothing but the word keeps
  it.
- **`--remove-word <word>`** (repeatable) takes a word out of rule
  `comment`s wherever it stands as a whole word, with the blanks around it;
  one blank is left where it had blanks on both sides.

**What went is named.** The older formats keep linestyles and symbols in
separate files, where one name may be both; a customisation has ONE
definition a name, so of two that share a name the later file's is kept and
the other goes. That is the order doing what it means - and it is never done
silently, because the definition that went is one somebody drew. Each is a
line of the report: its name as the files give it, the file kept and the file
dropped, the kind each file made it, whether the two differ (leaving out
their file and kind), and how many rules name it as their linestyle and as
their symbol. Where a name is kept as one kind and rules use it as the OTHER
kind, the one that went, that is a warning besides. Those rules are left
naming a definition they were not written for (a line drawn with what is now
a symbol), and giving the libraries in the other order keeps the one the
rules mean. A definition replaced by one of its own kind is listed and not
warned of: no order of the files would keep anything else.

**What a conversion cannot carry, it says.**

- *A standard colour name the table colours otherwise.* A customisation's
  table may not redefine a standard name ("Colour names"), so `brown` is the
  standard brown whatever the table has for it. Where the table has another
  colour, the name is reported with both (`colour_kept_standard`): it is a
  colour the table's author meant and the drawing will not have.
- *Characters read by a guess.* A file that is neither UTF-8 nor marked as
  UTF-16 is read as Windows-1252 (`core::decodeText`), and its characters
  outside ASCII - a diameter sign in a definition's name - are what that
  guess makes of them: a wrong one is a wrong character in the customisation
  and an error nowhere. A warning names the file, the encoding and how many
  such characters there are; once a file, and only when there are any.
- *A file that gave nothing.* What a file is, is asked of a keyword anywhere
  in its text, so a comment is enough to make one a style library of no
  definitions. A library no definition was read from, and a survey code file
  no rule was read from, are warnings.

The text is read back before it is handed over, and the conversion fails if
it does not come back as the value it was written from. The file is this
tool's whole product and is compiled into a build; a fault found there would
be found by a user. For the same reason it is written beside the output first
(`<output>.partial`) and then put in its place, as the DXF and IFC writers
do: a write that fails leaves the file that was there, never half of a new
one (`AWriteThatFailsLeavesTheFileThatWasThere`).

**What a rename may not do.** Taking a word off can make two things one, and
both are refused, naming them, rather than settled by a rule nobody chose:

- two definitions arriving at one name - `"ACME Kerb" and "Kerb" would both
  be "Kerb"`;
- a rule coming to name a definition it did not name: a reference that was
  the name of NO definition (`ACME Tree`, defined nowhere) which without its
  word is one (`Tree`), or one (`Post`) that a renamed definition (`ACME
  Post`) would come to be called. A reference that names nothing before and
  nothing after is renamed like any other and stays unresolved.

### The report

On success the program prints what it did as `key=value` lines, a text value
quoted as every reply's is (`core::replyQuoted`), and exits 0; on any failure
it prints the reason on standard error, writes nothing and exits 1 - an
exception included, which `main` catches so that the exit code is never
anything else. A list is its count and then a line an entry, and an entry of
several facts is a record of them on its line. The three fixtures below,
converted together:

```
name="Site"
files=3
definitions=7
symbols=4
linestyles=3
at_vertices=3
groups=4
strokes=36
definitions_replaced=0
rules=11
keys=8
colours_resolved=0
colours_unresolved=1
colour_unresolved="sui test purple"
colours_kept_standard=0
names_renamed=0
groups_renamed=0
references_renamed=0
rule_groups_renamed=0
comments_changed=0
unresolved_references=2
unresolved_reference="0"
unresolved_reference="TEST Missing Symbol"
notice_lines=0
warnings=0
```

and then `bytes`, the length of the text, and `output`, where it was written.
The two lists whose entries are records, when they have something to say:

```
definition_replaced="Gate" kept="site_symbols.4d" kept_as=symbol dropped="site_lines.4d" dropped_as=linestyle differs=yes linestyle_rules=2 symbol_rules=0
colour_kept_standard="brown" standard="#A52A2A" table="#964B00"
```

`unresolved_references` are the linestyle and symbol names the rules use that
no definition has. They are reported and are not an error: a customisation
need not be self-contained, and `0` is the plain continuous line. `warnings`
is what the readers of the older formats said of the files and what the
conversion found in them itself ("What went is named", "What a conversion
cannot carry, it says"), each under its file's name; none of them fails a
conversion, and each is for the person converting to read. Without `-o`
nothing is written and the report alone is printed, which is how to see what
a conversion would give.

The report is UTF-8, and so are the arguments: on Windows the program takes
its command line as the UTF-16 it is and never in the ANSI code page, in
which a file name outside it does not arrive, a name arrives as bytes the
format refuses, and a word to strip matches nothing.

### The fixtures, and what the converter is held to

`tests/data/customisation/` holds three small customisations in the Katana
format: `test_linestyles.customisation.json` (3 linestyles),
`test_symbols.customisation.json` (4 symbols) and
`test_survey.customisation.json` (11 rules over 8 keys). They are the three
fixtures of `tests/archive12d/data/customisation/` over again, and they were
WRITTEN BY HAND, from those files' text and the format chapter above, before
the converter was first run on them. That is the point of them: a converter
tested against its own output agrees with itself. Each legacy fixture,
converted under its twin's name, must equal the twin - the value whole, and
the text byte for byte, since the twins are written in the writer's layout
(`EachLegacyFixtureConvertsToItsHandWrittenTwin`,
`TheTwinsAreInTheWritersLayoutByteForByte`). They must never be replaced by
what the converter writes. And because a file cannot show how it was made,
`TheTwinsHoldWhatTheLegacyFixturesSay` states what each twin holds, value by
value, from the legacy fixtures' text - anchors, modes, a text's every
member, a weight, a `hide` that is said and one that is not: a twin
regenerated from a converter that had lost a field would agree with that
converter, and fail there.

`test_survey` has no `colours`: its one unknown colour name was invented for
the legacy fixture as a name no table knows, nothing gives it an RGB, and so
it stays unresolved here as there.

They are in a directory of no one suite because more than one loads them: the
converter's tests today, and the command line, the window and the widget
tests with the work that follows.

One thing a checkout does to them: git's `core.autocrlf` hands a text file to
a Windows working tree with CRLF line ends. The reader takes either, but a
comparison of bytes does not, so the tests take the carriage returns off a
twin's text before comparing it. *Not done*: a `.gitattributes` rule keeping
these files' line feeds, which would also keep a digest of one
(`customisationDigest`) the same on every platform - a test that pins such a
digest of a committed file will need it.

The other small inputs are written in the test, and what each becomes is
worked out beside it. The program itself has four tests, the cases of
`tests/archive12d/check_convert_program.cmake` (`customisation_convert.*`),
because the functions' tests pass with an option wired to the wrong member or
to none: a fixture converted to its twin with the exit code, the report and
the line ends checked, and a missing `--name` refused with exit code 1; every
option given at once, each seen in the report and in the file written; an
unknown option, an option without its value and `--help`; and a name, a word,
a file and an output outside ASCII.

### The reference customisation

The customisation the survey coding was built against is two style libraries
and two survey code files, with a colour table in a `support` folder beside
them. It is third-party material under its author's own licence and its two
libraries carry a notice against copying, so it is in no clone: the folder
is git-ignored, and the cache variable `KATANA_REFERENCE_CUSTOMISATION_DIR`
names it (`docs/building.md`). Nothing committed needs it, and the test that
converts it skips where the folder is absent or holds no file in the legacy
formats - and FAILS where it holds some that are not the four, which a skip
would hide
(`AReferenceFolderIsSkippedOnlyWhenItHoldsNoLegacyFileAndRefusedWhenItHoldsTheWrongOnes`).

It is converted - by the owner, on the machine that has it - with

```
katana_customisation_convert --name NSW --description "<a sentence>"
    --notice-from "<linestyle library>" --notice-from "<symbol library>"
    --colours "<folder>/support/colours.4d"
    --strip-leading-word <publisher's word> --strip-leading-word <vendor's word>
    --remove-word <publisher's word>
    -o resources/customisation/nsw.customisation.json
    "<linestyle library>" "<survey code file>" "<names file>" "<symbol library>"
```

The four files in that order is the documented load order, the one the
program has always loaded them in: the symbol library last, and the names
file after the survey code file, whose rules win a field both set.

**That order is an open decision, and the owner's.** Three names are given by
BOTH libraries, as three different definitions. Read last, the symbol
library's are the ones kept - and two of those three names are what four
rules give as their LINESTYLE, while no rule places any of the three as a
symbol. So four rules are left drawing a line with a symbol (the survey code
check reports two of them as such), and the conversion says so in its report:
three `definition_replaced` lines that change kind, two warnings. With the
symbol library given FIRST the three linestyles are kept instead and no rule
names a definition of the other kind - 471 symbols and 321 linestyles in
place of 474 and 318, which moves every figure below that counts them. *Not
done here:* those figures are what the rest of this work is pinned to, so the
order was kept and the choice recorded for the owner.

The two words are the publisher's, which begins most group paths, and the
vendor's, which begins a few; they are arguments so that no committed file
spells them
(`tests/archive12d/customisation/test_convert.cpp` finds them in the data, as
what stands before a group path's first blank and is no part of the path).
The output folder is git-ignored too, and is where the build looks for the
customisation to compile in.

What the conversion of 2026-10-06 gave, each figure also the census's:

| | |
|---|---|
| definitions | 792, from 796 blocks: four names are defined twice, three of them once in each library |
| definitions replaced | 4: one within the linestyle library, by a block of the same words; three of the linestyle library's by the symbol library's, each a different definition - two of those names the linestyle of four rules, none placed as a symbol |
| listed as symbols / not | 474 / 318 |
| drawn at vertices | 157 |
| group paths | 71 |
| strokes | 35,684: 17,014 move, 17,220 draw, 104 arc, 312 circle, 178 dot, 342 pen, 514 text |
| rules | 1,624 (725 and 899) over 632 keys |
| names the rules use | 426, of which 5 are defined by neither library |
| colour names the rules use | 22: 9 standard, 7 given a colour from the table, 6 left unresolved - every one a plot pen |
| standard names the table colours otherwise | 2: they draw in the standard colour |
| characters read by an inferred encoding | 82, all in the linestyle library (Windows-1252) |
| warnings | 4: an `<item>` that names no code, the inferred encoding, and the two names kept as symbols that rules give as their linestyle |
| leading words taken off | 29 definition names, 786 group paths, 1,029 rule groups, 8 rule references |
| comments changed | 1 |
| notice | 16 lines: the two libraries' blocks of 8, which differ |
| the file | 1,541,413 bytes |

**Where the figures come from.** Two scripts in `tools/`, neither using any of
Katana's code:

- `tools/reference_census.py`, given the four legacy files in that order and
  the words (`--strip WORD`, `--remove WORD`), counts them with a small
  tokenizer and the standard XML parser - and counts what a conversion
  REPORTS besides: what carried a word, the blocks replaced and how the rules
  name them, the characters an inferred encoding gave, the libraries' notice
  lines and, given the colour table (`--colours TABLE`), the colour names by
  where their colour comes from;
- `tools/customisation_census.py`, given a Katana customisation, counts it
  with the standard `json` module under the same names, and with
  `--against <census.json>` compares the two figure by figure and exits 1 if
  any differs.

The converted file gave the first script's figures in all 21 it can be asked
for. The others - eleven, with the words and the table given - describe the
legacy files and the conversion rather than the result: blocks read, a file
and replaced, strokes read, rules a file, items with no key, what carried a
word, what was replaced, the inferred encoding, the notice lines, the
colours. `TheReferenceCustomisationConvertsToTheFiguresOfItsCensus` holds
the converter and its REPORT to all of them, with the 29 names the earlier
clean-up gave the definitions that carried the publisher's word, every one of
which must be defined. No figure in that test is the converter's own.

### What reading and writing cost

`benchmarks/bench_customisation.cpp`, Release, GCC 16.2, on 2026-10-06 on a
machine busy with other builds; the median of five repetitions, in
milliseconds. The generated customisation is made in code to the reference
one's size and mix (792 definitions, 35,684 strokes with coordinates of three
decimals, 1,624 rules), so that the measurement can be repeated in any clone;
the built-in column is the converted reference file, where the build has one.

| | Generated, 1,558,760 bytes | Built-in, 1,541,413 bytes |
|---|---|---|
| writing, whole | 7.3 | 7.4 |
| writing, definitions alone | 4.7 | 4.9 |
| writing, rules alone | 2.2 | 2.1 |
| reading, whole | 42.6 | 38.8 |
| reading, definitions alone | 36.0 | 34.0 |
| reading, rules alone | 6.0 | 5.5 |
| the 69,270 numbers of the strokes, by `strtod` | 21.5 | |
| the same, by `std::from_chars` | 0.6 | |

**Reading is about twice the older readers**, which were measured at 21 ms on
the four reference files (`docs/survey_coding.md`): 39 ms for the reference
customisation itself, once, at the first use of the built-in. The benchmark
says where it goes. The definitions are 34 of the 39, and a definition is
almost nothing but numbers; converting the numbers of the strokes with the C
library's `strtod`, which is what the JSON library hands each one to, takes
21.5 ms with nothing else done, and `std::from_chars` does the same work in
0.6. So about half of a read is one function called 69,270 times, and the
rest is the JSON library's scanning and the building of the strokes and
rules.

The figures are the machine's as much as the code's. Run twice more the same
evening, with nothing changed in the reader or the writer: under two other
builds every row was about twice the table - reading the reference file
83.8 ms, writing it 16.7 - and so were the two rows that run none of Katana's
code (41.8 and 1.3 ms for the numbers alone); with the machine quiet again the
table came back (40.0 ms to read the reference file, 7.5 to write it, 24.1
and 0.6 for the numbers). The proportions are what to take from it: half of a
read is `strtod`.

*Not done, and nothing was changed to get these figures:* the only way to
take that half is a number scanner of the format's own in place of the JSON
library's, which is a second JSON reader in the program. It is the owner's
to decide against 20 ms at start-up; the benchmark is there to decide it on.

### Decisions about the converter, and what was rejected

**The words to strip are arguments, and matched as written.** *Rejected:
finding them* (the commonest first word of the group paths), which is what
the reference test does to avoid spelling them. A tool that decides by itself
what is a publisher's mark would one day take a real first word off every
path of somebody's library. *Rejected: folding case*, since a definition's
name is compared with regard to case everywhere else: `acme Kerb` and `ACME
Kerb` are two names, and a word stripped from one and not the other would be
a surprise in either direction.

**A definition that went is reported, and the conversion does not choose.**
Two libraries that give one name is the normal case of these files, so it is
not refused. *Rejected: keeping whichever kind the rules use*, which looks
kind and is a rule nobody chose - it would make the result depend on the
survey code files as well as on the order, and where rules use a name both
ways it has no answer. *Rejected: carrying both under two names*: the rules
name one. The order stays the whole of the meaning, and the report gives the
person converting what they need to choose it: what went, whether it
differed, and which kind the rules were written for.

**A standard colour stands, and the table's other colour for it is said.**
*Rejected: writing the table's colour into `colours`*, which the format
refuses, and for the reason given under "Colour names" - an archive import
has no customisation to ask, and would draw the same name in another colour.
*Rejected: renaming the colour in the rules* to carry it: a conversion that
rewrote what the rules say would no longer be one.

**A colour is written under the rules' spelling, not the table's.** The table
says `sui_gas` and the rules `sui gas`; they are one name by the fold, and
the file is read by the people who read its rules.

**Only the rules' colours are looked up, not the pens inside definitions.** A
definition's `pen` names its colour the same way, and the reference
libraries' pens are a plot pen and `view_colour`, which is the entity's own:
neither is to be given a colour. A library whose strokes name a colour of the
table's would draw in the entity's colour as it does today; *not done*,
because nothing here needs it and a pen that begins to resolve changes how a
definition is drawn.

**Text colours are looked up with the rest.** The survey code lint checks a
rule's own colour, its symbol's and its text's with one resolver, so the
converter fills the table for all three.

### Not done by the converter

- **The converter's readers are still the product's.** `katana_archive12d`
  holds them, and the three programs link that library, until the work that
  moves them into `src/katana_archive12d/legacy/` beside the converter.
- **The order of the reference libraries is not settled** ("The reference
  customisation"): as converted, four rules draw a line with a symbol.
- **A telephone number is in the reference customisation.** Ten of its symbol
  definitions draw a text that holds one - the same number in each - and five
  of the ten are placed as symbols by ten rules (five codes, two rules each),
  so a survey coded with those draws it. It is faithful to the source, and no
  option of the converter takes a stroke out of a definition; whether a
  general product ships it is the owner's to decide before the file is
  compiled in.
- **Two standard colour names draw in the standard colour, not the table's**
  (`colour_kept_standard`). Reported, and all that can be done here.
- **No colour for a definition's pen** (above), and **no `sources`**: a
  converted customisation does not say which files it was made from.
- **No warning fails a conversion** - a reader's or the conversion's own. It
  is printed with the report. The reference files give four; a definition a
  library reader passed over would be reported the same way, and is a loss
  the person converting has to notice.
- **A console may not show the report's characters outside ASCII.** The
  program writes UTF-8 and leaves the console's code page alone; redirected,
  the report is what it should be.
