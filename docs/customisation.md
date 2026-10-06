# Customisation

A customisation is what turns a surveyor's field codes into a drawing: the
linestyle and symbol definitions, the survey code rules, the colours its names
mean and how linework is processed. `docs/survey_coding.md` says what each of
those IS and how a code is applied. This document is about the customisation
as one thing: the value that holds one and the file that keeps it.

Its first chapter, below, is the file format. The chapters on the
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
byte (`TheWorkedExampleOfTheDocumentIsWrittenAsItIsPrinted`).

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

The object of a text stroke: `text` (empty), `angle` (0), `height` (0),
`justify` (empty; kept as written, such as `middle-centre`), `font` (empty),
`widthFactor` (1), and `extra`, three numbers `[a, b, c]` (all 0) that are
kept and not interpreted (`StrokeText::unnamed`; `docs/survey_coding.md`
says why).

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
  string's own), `size` (0: the definition's own size), `rotation`, `offset`,
  `raise` (0).
- **`text`**: `style`, `colour`, `units`, `justifyX`, `justifyY`, `weight`
  (text, empty); `size`, `offset`, `raise`, `angle`, `slant` (0);
  `widthFactor` (1); `underline`, `strikeout`, `italic` (false).
- **a pipe**: `justify`, `shape`, `size1`, `size2` (TEXT, empty); `active`
  (false).
- **an attribute**: `type`, which is `text` or `integer` (**required**);
  `name` (**required**, not empty); `value` (TEXT, empty).

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
| a name a project could not record | `InvalidArgument` | `top level: "name": a customisation name cannot hold ...` |
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

**The order of the checks**: the text is JSON; its `format` is this one's; its
`version` is not newer (so a newer file is told it is newer, not that its new
members are unknown); no member is given twice; then the members, the
top-level ones before the lists are gone through.

**Encoding.** The bytes are decoded by `core::decodeText` first, so a UTF-8
byte order mark and UTF-16, which an editor on Windows saves readily, are
read. Bytes that are neither are NOT decoded as Windows-1252, as that function
does for the formats that never said what they are: this format does say - it
is JSON, and JSON is UTF-8 - and a guess at a code page would put a wrong
character into a name silently, where a name is an identity. Such a file is
`not a Katana customisation file`, with "the bytes are not UTF-8 text" beside
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
   line.
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
module, whose three functions of the same names now forward to it, because
`cad` resolves a customisation's colours and may not see that module.

**A customisation's own table**, `colours`: `"sui electricity": "#FF7F00"`. A
colour is `#RRGGBB`, or `#RRGGBBAA` with an opacity, in either case of digit;
it is written in upper case. `entity::ColourTable` refuses

- a name that is empty, or nothing but blanks and separators, or not UTF-8;
- two names with one fold: a lookup could not say which was meant;
- a name whose fold is a STANDARD name. An archive import colours its strings
  from the standard names alone, having no customisation to ask, so a table
  that redefined `red` would draw two reds in one drawing.

**One fold.** A colour name is compared after `entity::foldColourName` and in
no other way: ASCII letters in lower case, blanks at the ends removed, `_` and
`-` read as a blank, and `gray` as `grey` where it is the whole name or its
last word (`Dark_Gray` is `dark grey`). Blanks inside a name are kept, so two
are not one.

**One resolver.** `entity::resolveColour` asks the table and then the standard
names, and gives nothing for a name neither knows - which leaves a colour
alone rather than guessing (`pen 035` is such a name, and a pen that does not
resolve draws in the entity's own colour).

### Linework codes

`include/katana/entity/linework_codes.hpp`. The spellings of the seven
linework controls are part of a customisation - a team writes `ST` or `S` as
its field book does - so `LineworkCodes` and its `validate` moved from `cad`
to this layer, where the file is read. `cad::LineworkCodes` is now an alias
and `cad::validate(codes)` forwards, so nothing that named them changed.
`entity::lineworkCodeMembers` lists the seven by the names the file, a refusal
and a command use, so that the three cannot come to disagree about one.

### The name

`validateCustomisationName`: not empty, valid UTF-8, and with no line break,
`/` or `\`. The name is written into a project as the record of what the
drawing was drawn with, one name a line and never a path, and the project
store refuses exactly these; a name that passed here and failed there would
make every SAVE of the session fail. The same rule holds for a source's name,
for `basedOn`'s and for a definition's `from` when it is not empty, since each
of them reaches that record.

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

- **Nothing reads or writes a file yet.** This is the value, the format and
  its tests; the commands, the built-in customisation and Settings come with
  the work that follows, and are recorded in this document then.
- **`-0` reads as 0.** Negative zero written without a fraction is an integer
  to the JSON library. The writer never writes it so; a person might.
- **Explicit zero against "not said"** is not kept for a rule's symbol and
  text numbers, because the model does not keep it (above, "Absent is not a
  default").
- **A source may be named twice** in `sources`; the format keeps the list as
  it is and leaves what that means to the session that made it.
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

  So what is left to take is the number conversion, about 21 ms of it, and
  the JSON library offers no way to replace it: only a scanner of this
  format's own would. *Not done*: that is a second JSON reader in the
  program, and it should be decided on a benchmark that can be repeated, on
  the converted reference customisation - which comes with the conversion.
