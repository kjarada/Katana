<!-- Katana plan, section 25 of 47, subsection 20.3. Index: ../../PLAN.MD. Previous: 02-20-2-12d-model-programme.md. Next: ../26-phase-21-civil-and-survey-engineering.md -->

## 20.3 The survey coding programme — TAKES PRECEDENCE OVER 20.2's REMAINDER AND EVERY OTHER PHASE

**The user's instruction of 2026-09-22, and the reason this section exists:**

> look at the files i put here `C:\GitHubProjects\Katana\docs\12d Refrence Files`,
> they are all related, as survey code WM is a water main and gets a specific
> linestyle in linestyles file, as well as some codes get a symbole, i want you
> to implement all this, mapfile, symples and linestyles, this is more
> important than other steps on the plan, so dont stop till it finishes and
> fully implmented

**Nothing else is started or resumed until every slice here is DELIVERED**,
including the property definitions left over from 20.2 slice 5. The only
exception is a defect that blocks this work.

### What the three files are

They are a real production customisation from a road authority, and they are
how a survey becomes a drawing. The files are third-party material under
their own licence and are NOT part of this repository; everything that uses
them skips when they are absent.

**CORRECTED (2026-09-23): until this date they WERE in the repository.** Commit
b77386e said it took them out and that `.gitignore` kept them out; neither was
true - all four were still tracked at HEAD and `.gitignore` had no rule for
them. They are now untracked with a rule added. They remain in the history
from commit 4e070d8 onwards, which has been pushed to `origin`; removing them
from the history needs a history rewrite and a force-push, which is the
owner's decision and has not been done.

| File | What it is |
|---|---|
| a detail mapfile (`.mapfile`) | XML (UTF-16LE). 728 rules keyed by survey code: what MODEL, colour, linestyle, weight and breakline a code becomes, which symbol it gets, what attributes it carries. |
| a linestyle library (`.4d`) | 322 linestyle definitions - the strokes a line is drawn with. ASCII. |
| a symbol library (`.4d`) | 474 symbol definitions, same grammar. UTF-16LE. |
| a second mapfile (`.4d`) | 900 more rules, sharing its extension with the libraries. |

A surveyor codes a point `WM01`. The mapfile's `WM*` rule puts it in model
`SURVEY SERVICES`, colour `sui water potable`, breakline `Line`, linestyle
`WATR Main`; the linestyle library says what `WATR Main` is drawn with. A
code like `AC*` additionally names a symbol (`CULT Bollard`) from the symbol
library. That chain is what this section builds.

### The grammar, as measured rather than assumed

Both `.4d` files are ONE grammar - which the 12d manual confirms: "There can
be the same symbol (**defined as a linestyle**) for every vertex"
(`10-super-string.md` 510). A symbol is a linestyle whose `mode` is `vertex`.

```
<kind> "NAME" {                 kind = paperstyle | worldstyle | twoptstyle
  group "Survey/WATR"           a folder path for the library tree
  mode vertex                   drawn at each vertex rather than along the line
  length 2.5                    the period of the pattern along the line
  factor 5                      a scale applied to every coordinate
  xorigin 0   yorigin 0         worldstyle/paperstyle origin
  xorigin1/yorigin1             twoptstyle: the first anchor
  xorigin2/yorigin2             twoptstyle: the second anchor
  stretch_mode 2  cycle_mode 2  twoptstyle: how it fills the span
  colour "pen 035"              the pen for the strokes that follow;
                                "view_colour" means the entity's own
  move X Y                      pen up
  draw X Y                      pen down
  arc RADIUS START END          degrees, centred on the current point
  circle RADIUS                 centred on the current point
  dot 0
  text "T" ANGLE HEIGHT "justify" "font" WIDTHFACTOR SLANT ? ?
}
```

Counted over the two files: paperstyle 238, worldstyle 510, twoptstyle 48;
`move` 17045, `draw` 17317, `circle` 312, `arc` 104, `dot` 179, `text` 514.
`paperstyle` sizes are plot millimetres and so scale with the plot;
`worldstyle` sizes are model units and stay on the ground.

The mapfile's `<map_data>` is a list of `<item>`, each keyed by a survey code
which is either exact (209 of the 465 distinct keys) or a prefix wildcard
(`WM*`, 256 of them) - there is not one mid-string wildcard in the file. An
item carries exactly ONE aspect, and 213 keys carry more than one item, so a
code's treatment is the UNION of the aspects that match it:

| Aspect | Fields | Count |
|---|---|---|
| geometry | `model` `colour` `breakline` `linestyle` `weight` `group` | 457 |
| symbol | `symbol_data{style colour size rotation offset raise}` `hide` | 200 |
| attributes | `attributes` / `vertex_attributes` / `segment_attributes`, with `justify` `shape` `size1` `size2` `active` | 48 |
| map_attributes | `map_attributes{integer\|text{name value}}` | 13 |
| textstyle | `textstyle_data{textstyle colour type size justify_x justify_y}` | 7 |

### The slices

1. **The style library reader. DELIVERED.** `entity::LineStyle` and
   `entity::StyleLibrary` (`include/katana/entity/style_library.hpp`) are the
   definitions - one type for linestyles and symbols alike, because in 12d a
   symbol IS a linestyle and `mode vertex` is the only difference.
   `archive12d::readStyleLibrary` (`include/katana/archive12d/style_library.hpp`)
   reads a `.4d` into one, keeping every command and naming every command it
   does not know. `readStyleLibraryInto` loads a customisation made of several
   files, later winning. See `docs/survey_coding.md` for the grammar, where
   each piece lives and why, and the measurements.

   Evidence: 12 tests. The real reference customisation reads with
   NO warnings - 796 definitions, 792 after the four defined twice, 157 of
   them symbols, in 71 groups - and every count is checked against a script
   that uses none of Katana's code.

   The types live in `entity`, not in `cad` as this plan first said, because
   `commands` has to see them to apply a survey code and `commands` cannot see
   `cad`. A library is deliberately NOT part of a Model: it is a site-wide
   customisation a project NAMES rather than copies, which is how 12d works
   and avoids writing 35,000 strokes into every project file.
2. **Strokes on demand. DELIVERED for the geometry.**
   `cad::styleDrawing` (`include/katana/cad/style_drawing.hpp`) turns a
   definition into polylines and texts in model coordinates - a symbol at each
   vertex, a two-point style across the span, or a pattern repeated along the
   line, with paper and world scaling. 14 tests, every expected value worked
   out by hand. See docs/survey_coding.md for the three decisions in it.

   OUTSTANDING: the sixteen built-in symbols are still a closed set that
   `Style::symbol` is validated against, so a style naming a library
   definition is refused by the model. That is slice 5, where it belongs.
3. **The mapfile reader. DELIVERED.** `entity::SurveyMap` and `SurveyRule`
   (`include/katana/entity/survey_map.hpp`) are the table;
   `archive12d::readMapFile` (`include/katana/archive12d/map_file.hpp`) reads
   it, on a small strict XML reader (`katana::core::readXml`,
   `include/katana/core/xml.hpp`) rather than a linked one, so this module
   keeps its no-third-party-dependency property and stays under the sanitizer
   job. `readMapFileInto` loads several mapfiles as one customisation.

   The plan above said the aspects are told apart by which fields a rule
   carries. THAT IS WRONG, and the test against the real file is what caught
   it: a mapfile is written in up to ten SECTIONS and the section is the
   meaning - `<map_attributes>` is attributes on the string inside
   `string_attribute_data` and attributes on each vertex inside
   `vertex_attribute_data`. Reading only `<map_data>` takes 457 of the 725
   rules and loses every symbol. See docs/survey_coding.md.

   Evidence: 15 tests. The reference detail mapfile reads as 725
   rules over 465 keys with no warnings; `names.4d` - a second mapfile,
   despite the extension a linestyle library also uses - reads as 899 rules
   with one warning, which is correct.
4. **Applying a code. DELIVERED.** `cad::applySurveyCodes`
   (`include/katana/cad/survey_coding.hpp`) turns the mapfile's answer into
   the layer, the style and the properties an entity should have, as ONE
   undoable transaction - twenty thousand coded points are one undo, not
   twenty thousand. `CODE [<property>]` in `katana_cli`. It reports what it
   did AND every code the mapfile had no rule for, because a code silently
   left alone looks exactly like a code that was handled. 7 tests; see
   docs/survey_coding.md for the six decisions in it.

   On a menu since b15eaa8: File > Apply Survey Codes (this slice said it was
   CLI-only, and so did the status below, until the audit of 2026-09-23 found
   the menu item).
5. **The model and the project. PARTIALLY DELIVERED.** `Style::symbol` is no
   longer a closed set: it is a name resolved when it is drawn, exactly as
   `Style::linetype` already was. `cad::Document` carries the loaded
   `StyleLibrary` and `SurveyMap` - not in the Model, because a customisation
   is named by a project rather than copied into it - and `definitionFor` is
   the one place that resolves a name.

   The model accepts any name because a project may be opened before its
   library is loaded; the COMMAND still refuses one nothing defines, so a
   typo is caught where a person typed it. `STYLE SET s symbol CULT Bollard`
   also now takes the rest of the line, since a library name has spaces in it.

   OUTSTANDING: a project does not yet remember which library and mapfile it
   was drawn with, so they must be loaded again each session.
6. **Loading and drawing. PARTIALLY DELIVERED.**
   `archive12d::readCustomisation` loads any mix of libraries and mapfiles,
   deciding what each file is BY LOOKING INSIDE IT - `.4d` is the extension of
   both formats in the reference customisation, so the name cannot be
   trusted. `File > Load 12d Customisation...` in the application, `CUSTOMISE
   <file>...` in `katana_cli`, and `--customise <file>...` headlessly. They
   share `readCustomisation`, but not one report: the CLI writes its own, and
   `--customise` is ignored outside the `--screenshot` path (audit QT-04, open)
   - this used to say all three take the same path and write the same report.

   The viewport draws them: a point whose style names a definition gets that
   definition's strokes, a line whose linetype names one has it laid along
   the line, and a `colour` command inside a definition changes the pen.

   OUTSTANDING: a library BROWSER - the groups tree with a preview of each
   definition - is not built; the style manager still lists only the
   document's own styles.
7. **The CLI and the documentation. DELIVERED** for what exists so far:
   `CUSTOMISE` with its report, and `docs/survey_coding.md`.

### Two defects a production archive found, both now FIXED

Found with a real 15 MB archive and fixed with its numbers in hand.

**1. A `breakline point` string of several vertices was drawn as a polyline.**
That flag means the vertices ARE points. Measured in that archive: every
string states it - 12,280 `line`, 13,379 `point` - and 53 of the point ones
have several vertices, including a 61-vertex string of drill holes in a
`SURVEY INFRASTRUCTURE V2` model. Each vertex is now its own point entity,
and the import says how many it made. The trap, recorded in
docs/survey_coding.md: the CURRENT breakline type defaults to `point`, so 21
hand-written fixtures that meant lines became points; they now say
`breakline line` once at file level.

**2. A string's symbol was taken onto its style only when the string had ONE
vertex**, so that drill-hole string drew no symbols where 12d draws 61. A
string with one symbol block now takes it whatever its length, and the
exporter's symbol block moved out of its `PointGeometry` branch so a line is
written back with it.

**3. And a diagnostic, because neither was visible as a fault.** Loading a
customisation now reports how many of the drawing's styles it answers for -
`cad::customisationCoverage` - since "no linestyles showing" looks the same
whether nothing is loaded, the library does not define what the drawing
names, or the drawing's styles are 12d's plain lines.

### The customisation is compiled in (the user's instruction of 2026-09-22)

> i dont want it be loded at everytime softwares opens i want it as solid part
> of the software not as a customization

Done: `tools/embed_customisation.py` turns the customisation files into a
generated source at build time and `archive12d::builtinCustomisation` parses
them once, so the linestyles, symbols and survey codes are part of the binary
and nothing is found, loaded or configured. Verified by moving the files away
entirely and running: 792 definitions and 1,624 rules, still there.

The bytes are embedded rather than generated as C++ structures - one reader
for a built-in and a loaded customisation, and 21 ms in Release once per
session against a multi-megabyte translation unit. The generated source is
never committed, since the customisation is third-party material under its own
licence; a checkout without it builds an empty table and draws plain lines.

**STATUS: DELIVERED, bar the one thing below that is still true.** The
chain the user asked for works end to end: a mapfile, a linestyle library and
a symbol library load together; a survey code resolves through the mapfile to
a model, a colour, a linestyle and a symbol; the entity gets them; and the
viewport draws the definition's strokes.

OUTSTANDING, not blocking:
- a library BROWSER - the groups tree with a preview of each definition - is
  not built, so the 792 definitions can be used but not looked through (in
  progress 2026-09-23).
- a mapfile symbol's `rotation`, `offset` (across the string) and `raise` are
  parsed into `entity::SurveySymbol` and then dropped: `applySurveyCodes`
  carries only the style and size, `Style` has no rotation, and the viewport
  draws every symbol at 0. Found by the audit of 2026-09-23 (the docs said the
  rotation was applied). Not blocking because the reference customisation
  sets all three to nothing: counted over both of its mapfiles (decoded from
  UTF-16), 401 `<symbol_data>` rules, and every `rotation`, `offset` and
  `raise` in them is zero or empty - so no drawing made from it is affected.

Struck from this list, with the reason: "a project does not remember which
customisation it was drawn with" stopped being true of the ordinary case when
the customisation was compiled in (above) - there is nothing to remember. A
customisation loaded ON TOP of the built-in one is still per-session, and that
is recorded as a limitation rather than as outstanding work, since nothing
asks for it. "Applying a code is a CLI verb" was never true after b15eaa8:
File > Apply Survey Codes exists.

---

