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

The work was built against a real Transport for NSW customisation prepared by
Extra Dimension Solutions - 728 mapfile rules, 322 linestyles, 474 symbols -
which is in `docs/12d Refrence Files`. **It carries its author's licence
notice.** Every test that uses it skips when it is absent, so the suite stays
green in a checkout without it.

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
  group "TfNSW Survey/WATR"     a folder path, the tree a browser shows
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
the Transport for NSW libraries alone are 792 definitions and 35,000 strokes,
which would be written into every project file that used one of them; and two
projects would then be able to disagree about what "WATR Main" looks like.

`StyleLibrary` is `NamedTable`, the same container as every other table of
named things, rather than a new one.

## Names compare with regard to case, and that is a decision

12d's own comparisons are not case-sensitive - the file format overview says
"Bypass" and "BYPASS" are the same linestyle. Katana's tables are
case-sensitive, and the library is one of them.

That difference is measured rather than overlooked. Of the **372 references**
the Transport for NSW mapfile makes into the two libraries, every single one
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
| `user_linestyl_TfNSWv15.4d` | 322 blocks (238 paperstyle, 47 twoptstyle, 37 worldstyle), 321 kept - "BDYS Parish" is defined twice | 10,969 `move`, 10,770 `draw` |
| `user_symbols_TfNSWv15.4d` | 474 blocks (473 worldstyle, 1 twoptstyle) | |
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
| `TfNSW_Survey_Detail.mapfile` | 725 over 465 distinct keys | none |
| `names.4d` | 899 | one, and it is right: an `<item>` holding only a `<group>`, which names no code and so could never apply |

An unknown section is named with how many rules went unread; an unknown field
inside a rule is named and the rest of the rule is kept.
