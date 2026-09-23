<!-- Katana plan, section 45 of 47. Index: ../PLAN.MD. Previous: 44-core-principle.md. Next: 46-audit-defect-register.md -->

# 45. Survey module and instrument interoperability

**STATUS: IN PROGRESS.** Requested by the user on 2026-09-23, and taking
precedence over the remainder of section 20.2 in `CLAUDE.md` section 5.2's
ordering until it is delivered.

A Survey menu, a Point Manager, an import wizard, and a Survey Data Exchange
subsystem that reads field data from Leica, Trimble and Topcon instruments and
data collectors into the drawing.

## 45.1 What already existed, and what this programme is therefore NOT

The brief asked for fifteen phases. Four of them were already built, and this is
recorded here so that nobody builds them a second time:

* **The survey calculation engine exists.** `katana::survey` has `cogo.hpp`
  (inverse, forward, polygon area and perimeter, slope-to-horizontal reduction,
  trigonometric height difference), `traverse.hpp` (computation, misclosure and
  adjustment), `angles.hpp`, `leveling.hpp`, `least_squares.hpp`,
  `network_adjustment.hpp`, `error_propagation.hpp` and `statistics.hpp`. The
  brief's "Phase 3 - survey calculation engine" is a wiring job, not a build.
* **A normalized survey model exists** in `survey/data_model.hpp`: `SurveyPoint`
  and four observation kinds, with the conventions stated at the top of the file
  (metres, **radians**, azimuths clockwise in [0, 2*pi), ordered containers so no
  result depends on hashing). What it lacked was provenance, stations, a project
  aggregate, and the calculated-versus-observed distinction. Those are 45.2.
* **Coordinate reference systems exist.** `katana::geodesy` has
  `coordinate_reference_system.hpp`, `coordinate_transformer.hpp`, `units.hpp`,
  `ellipsoid.hpp`, `geodesic.hpp` and `grid_factors.hpp`, with PROJ kept behind
  `src/` (Rule 4). The brief's "Phase 11" is wiring, and the wizard's job is to
  refuse to transform rather than to implement a transformation.
* **A text decoder, text helpers and exact unit ratios exist** in the layers
  below surveyio - `core/text_encoding.hpp`, `core/text.hpp`,
  `math/unit_ratio.hpp` - and a parser uses them rather than its own (45.3b).
* **A strict XML reader exists.** It was private to `archive12d`; LandXML does
  not get a second one, so it was promoted to `core` as `katana::core::readXml`
  (`include/katana/core/xml.hpp`). It refuses a doctype, an undefined entity and
  nesting past `kXmlMaxDepth`, and those three refusals are security properties
  rather than simplifications: a doctype is the entity-expansion and
  external-entity entry point, and the reader recurses, so past the cap the
  answer must be an error and not a blown stack.

## 45.2 Architecture, decided before any of it was written

**A new `surveyio` layer**, registered in `tools/check_layering.cmake` as
allowed `core;math;geometry;survey`. It parses manufacturer formats into
`katana::survey` values and nothing else.

* **`cad` may NOT see `surveyio`**, exactly as it may not see `interop` or
  `archive12d`. This is the structural answer to the brief's section 7 - "do not
  make the CAD application internally dependent on Leica, Trimble or Topcon
  structures". `app`/`qt` own the parsers; `cad` only ever sees
  `katana::survey`, which it is already allowed to see. A Leica type cannot
  reach the drawing even by accident, because the layering test fails the build.
* **`geodesy` is deliberately absent from `surveyio`'s allowed list.** A parser
  records the coordinate system a file *declares* - a name, an EPSG code, or
  explicitly "unknown" - and transforms nothing. Transformation is an act the
  user authorises in the wizard with both systems named. Making it structurally
  impossible in the parser is stronger than remembering not to do it.
* **Only the file NAME of a source is stored, never a path the file supplied.**
  The same rule the 12da reader follows for `ref_data`: a file must not be able
  to choose what gets read.

## 45.3 Slices

| # | Slice | Status |
|---|---|---|
| 1 | The XML reader promoted from `archive12d` to `core`, with the tests it never had | **DELIVERED** |
| 2 | Model: provenance, `SurveyStation`, `SurveyProject`, calculated-vs-observed | **DELIVERED** |
| 3 | `surveyio` scaffolding: format descriptors, a self-registering format registry, and detection that cannot silently guess | **DELIVERED** |
| 4 | Generic ASCII/CSV import and export, with column mapping and saved templates | pending |
| 5 | LandXML import and export | pending |
| 6 | Leica: GSI-8 and GSI-16, and ASCII/CSV | pending |
| 7 | Trimble: JobXML, CSV/ASCII, LandXML | pending |
| 8 | Topcon: documented observation formats, CSV/ASCII | pending |
| 8b | The `cad` bridge: a `SurveyProject` becomes ONE undoable command | **DELIVERED** |
| 9 | The Survey menu and its commands | partly: the Survey menu and toolbar exist (`katana_qt/survey/SurveyWorkbench`) with survey coding in them; the tools are in progress |
| 10 | The import wizard, six steps, with validation and a warning/error report | pending |
| 11 | Point Manager with source column and filtering | pending |
| 12 | Instrument profiles, and import history undoable as one transaction | pending |
| 13 | Reports: point, observation, import, traverse | pending |
| 14 | The compatibility matrix, stating format AND version actually supported | pending |

## 45.3b The bridge, and a gap it exposed

`cad/survey_import.hpp` turns a `survey::SurveyProject` into one
`commands::Transaction` - the layers it needs, then its points - so a single
undo removes an import of ten thousand points. It calls
`survey::validateProject()` rather than re-checking anything, so a parser and
the bridge cannot disagree about what a valid project is, and it returns
`nullptr` with no error for an empty project so that "nothing to do" and
"something went wrong" stay distinguishable. It lives in `cad` because `cad` may
not see `surveyio`; a field code is sanitised before it becomes a layer name,
because a layer name is a PATH and a code containing `/` would otherwise build a
subtree nobody asked for.

**GAP, found while building it - FIXED (2026-09-23).**
`survey::SurveyPoint::elevation` was a plain `double` with no "unset" state, so a
file with no height column yielded 0.0 and the bridge wrote it as a real height
at the datum; a surface built from such a drawing was flattened to zero under
every one of those points, with nothing reported. It is now
`std::optional<double>`. The bridge writes no elevation property for a point
without one (the surface builder already leaves such a point out), counts them
in `SurveyImportReport::pointsWithoutElevation` and warns; the level adjustment
refuses a held or weighted benchmark with no published height, naming it,
rather than levelling the run onto a datum of zero. The optional was chosen
over a companion flag because a flag and a value can disagree and a bare double
can be read without looking at the flag. Proven: with the refusal removed,
`SurveyLevelNetwork.AHeldBenchmarkWithNoHeightIsRefusedRatherThanTakenAsZero`
fails. The first round of parsers had each worked round the gap with a private
metadata key (`elevation-absent`, `landxml.elevationAbsent`, "none in source");
those go when the parsers are finished against the fixed model.

**A second gap of the same shape, FIXED the same day: a point with no
coordinates at all.** A raw observation file names its targets without
positioning them, and `SurveyPoint` has no such state. The unfinished Leica
parser created such a point AT THE ORIGIN with a metadata marker and the Topcon
one at placeholder ordinates - either would be drawn as a real mark at (0, 0) by
any consumer that forgot the marker. `survey::UnpositionedPoint` and
`SurveyProject::unpositionedPoints` now carry them: same id space as `points`,
referable by observations, stations and features, never drawn by the bridge,
which counts them in `pointsWithoutPosition` and warns - including when that
leaves nothing to import. Reduction of raw observations to coordinates (the
brief's "survey calculation engine" wiring) is what will turn them into
`CoordinateSource::Calculated` points, and is not yet built.

**Three shared pieces the parsers had each written for themselves, collapsed
(2026-09-23).** Found by reading the unfinished parsers side by side: a second
text decoder, a third private trim/number parser, and the foot and link ratios
repeated as doubles. The decoder moved from `archive12d` to
`katana::core::decodeText` (with `decodeTextAs` for an encoding a person
states), the helpers became `core/text.hpp`, and the exact length ratios moved
to `math/unit_ratio.hpp` with `geodesy/units.cpp` built on them and
`survey::metresPer(LinearUnit)` mapping a declared unit onto them. The 12d
reader's and the XML reader's own helpers now delegate to core, and in doing so
stopped consulting the C locale. Record in `docs/survey.md`.

## 45.4 The proprietary-format rule, as it will be applied

The brief forbids guessing undocumented binary structures and forbids claiming
support because of a file extension. This programme applies that as follows, and
the UI carries it:

* A format is implemented only when its structure is publicly documented or can
  be validated against a specification. Every format the UI offers names its
  parser variant - "Leica GSI-16" - not its manufacturer.
* A `FormatDescriptor` states what a format can carry (points, observations,
  stations), whether import and export are supported, and the parser version.
  The UI shows that record. There is no "all Leica files supported" anywhere.
* Detection returns a RANKED list with an explicit uncertain outcome that a
  caller cannot ignore. An unknown file is never handed to an incompatible
  parser.
* Test fixtures are constructed FROM specifications. No proprietary sample data.

## 45.5 Numbers that must not be guessed

Two failure modes in this area silently corrupt survey data rather than failing,
and both are called out here because a test that does not target them will pass
over the bug:

* **GSI encodes the decimal position in the word, not as a decimal point.** A
  parser that reads the data block as a plain integer produces coordinates wrong
  by orders of magnitude, and they look plausible. Every GSI test asserts a
  value worked out by hand from the specification.
* **Coordinate ORDER differs between families.** LandXML `CgPoint` content and
  Trimble exports are northing-first; much else is easting-first. Where the
  order is genuinely ambiguous in a file, the importer asks. It never guesses,
  because a transposed survey is a survey that looks fine and is wrong.

---

