# Architecture

Cross-cutting decisions that every Katana module obeys. Module-specific
documents live beside this one.

## Purpose

Katana is a deterministic C++ engineering platform for CAD and surveying. The
engine must be able to perform every important engineering operation on its own:
the user interface, the storage format and (later) the AI layer are all
consumers of it, never participants in it.

## Layering

Dependencies run one way, lowest first:

```
core → math → geometry → { geodesy, survey, terrain, entity } → commands
     → storage → cad → { qt, app }
io  → core
```

A module may include headers from the layers it is listed as depending on in
[`tools/check_layering.cmake`](../tools/check_layering.cmake), and from nowhere
else. Two rules are machine-checked there and run as the `layering` test:

1. **One-way dependencies.** `geometry` cannot include `survey`, `cad` or `qt`.
   The check reads every `#include "katana/<layer>/..."` and compares it against
   the declared allow-list for the file's own layer.
2. **No third-party types in public headers.** Eigen, CGAL, PROJ, SQLite, GDAL,
   PDAL, Qt, Vulkan and nlohmann may appear only in `src/`, never under
   `include/`. Each is wrapped by a Katana-owned interface — `Result<T>`-based,
   using Katana's own value types — so that replacing a library is a change to
   one module rather than to the whole codebase (PLAN.MD Rule 4).

The practical consequence: `katana_math` and `katana_geometry` have no
dependencies beyond the standard library, and their tests link nothing else.

## Error handling

There are no silent failures and no error codes that a caller can ignore by
accident. Every fallible operation returns
[`katana::core::Result<T>`](../include/katana/core/error.hpp) (or `Status`, its
void form), which holds either a value or an `Error` carrying:

* a typed `ErrorCode` — `InvalidGeometry`, `InvalidCRS`,
  `InvalidSurveyObservation`, `AdjustmentFailure`, `TriangulationFailure`,
  `DatabaseFailure`, `CommandRejected`, and so on;
* a human-readable message;
* a context string holding the offending input — the entity id, the file path,
  the layer name — so a failure can be diagnosed from a log alone.

`Result` is `[[nodiscard]]`. Exceptions are reserved for programmer error:
reading `value()` from a failed `Result` throws `BadResultAccess`, because that
is a bug in the caller, not a runtime condition. Third-party libraries that
throw (CGAL, nlohmann) or that use return codes (SQLite, PROJ) are converted to
`Result` at the module boundary; no third-party exception escapes a module.

## Numerical policy

Every tolerance in the codebase is a named constant in
[`numerics.hpp`](../include/katana/math/numerics.hpp), documented with its unit
and rationale. Ad-hoc epsilons are forbidden — they are how CAD kernels acquire
inconsistent behaviour between subsystems.

| Constant | Unit | Meaning |
|---|---|---|
| `kAbsolute` = 1e-12 | dimensionless | a normalised quantity (a sine, a unit determinant) is zero |
| `kRelative` = 1e-9 | dimensionless | general-purpose scaled equality |
| `kAngular` = 1e-10 | radians | directions are parallel |
| `kGeometric` = 1e-7 | model units | features coincide: duplicate vertices, point-on-curve, tangency |
| `kCoordinate` = 1e-4 | model units | survey coordinates are the same ground mark |

The scale that sets these is the projected coordinate: at a UTM northing of
1e7 m, one ulp of a `double` is about 1.9e-9 m, so no linear tolerance below
about 1e-8 can carry meaning. `kGeometric` sits about 50 ulp above that, leaving
room for a few chained operations to round.

Two further consequences run through the code:

* **Comparisons are explicit.** `operator==` on `Vec2`/`Vec3` is exact; fuzzy
  equality is `nearlyEqual(a, b, tolerance)`. Approximate equality is not
  transitive and must never hide inside an operator.
* **Large coordinates are handled deliberately.** Area, centroid and
  triangulation routines work relative to a local origin. A naive shoelace sum
  over UTM-magnitude coordinates forms products near 2.5e12 and loses roughly
  six significant digits of a 100 m² area; evaluating relative to the first
  vertex keeps full precision. Tests assert this by computing a figure locally
  and again shifted by (500000, 5000000).

## Determinism

Identical inputs and configuration must give identical results (PLAN.MD Rule 7).
This is a correctness requirement for an engineering tool, not a convenience.

* Containers that feed output are ordered (`std::map`, sorted vectors). No
  result depends on `unordered_map` iteration order or on pointer addresses.
* Compilation uses `-ffp-contract=off -fno-fast-math`, so the compiler may not
  fuse or reassociate floating-point operations. Results match across
  optimisation levels, which is why every suite is run in Release as well as
  Debug.
* Property tests use a fixed-seed generator
  ([`tests/support/property.hpp`](../tests/support/property.hpp)), so a failure
  reproduces exactly.
* Entity ids increase monotonically and are never reused, so a recorded id
  cannot come to mean a different entity after an undo.

## Logging

[`katana::core::Logger`](../include/katana/core/log.hpp) emits structured
records: timestamp, level, category, message and key/value fields, rendered as
`2026-09-19T10:15:30.123Z INFO [command] executed name=CREATE_LINE entities=1`.

A logger is an ordinary object passed to the subsystems that need one; there is
no global logger. `log()` is safe to call concurrently and serialises sink
invocation. Values containing whitespace or `=` are quoted, so records stay
parseable. Commands log their name, change count and execution time; the
convention is to log identifiers and counts rather than coordinate payloads, so
logs do not accumulate project data.

## Threading

The current engine is single-threaded by construction. The document, its model,
the command stack and the project store all belong to one thread, and that is
stated in each header rather than left implicit. The pieces designed to be
parallelised later — terrain tiles, spatial queries, import and export — are
written as independent units of work, so that Phase 19 can add a task system
without restructuring them. `Logger` is the one type that is explicitly
thread-safe today.

## Testing strategy

`ctest` runs unit, integration, property, regression and end-to-end tests plus
the layering check. The rules that matter:

* **Expected values are derived independently of the implementation** — from a
  closed form, a textbook identity, an exact synthetic construction, or a hand
  calculation recorded in a comment. A value copied from the code under test
  proves only that the code is self-consistent. This project has seen the
  failure mode first-hand: an early least-squares "solver" hardcoded its own
  test's expected answer and passed.
* **Edge cases are named, not assumed** — parallel and coincident lines,
  zero-length segments, tangent circles, nearly parallel geometry, duplicate
  points, degenerate polygons, empty containers, non-finite input.
* **Failures are diagnosed before they are fixed.** When a test disagrees with
  the implementation, the question is which one is wrong on the merits. A
  tolerance may only be loosened with a floating-point justification.

## Build

CMake 3.24+, one target per module, warnings as errors. `CMakePresets.json`
provides Debug, Release, RelWithDebInfo, sanitizer and static-analysis
configurations. Sanitizers use ASan+UBSan where available; on MinGW, where
libsanitizer is not shipped, UBSan runs in trap mode so undefined behaviour
aborts the offending test. GoogleTest and Google Benchmark are fetched once and
cached under `third_party/_cache`, so additional build directories configure
offline.

## Failure modes

| Condition | Behaviour |
|---|---|
| Invalid geometry submitted | rejected by `validate()` before entering the model |
| Command fails mid-way | change set rolls back; model unchanged; command not added to history |
| Transaction step fails | earlier steps are undone in reverse order |
| Locked layer edited | `CommandRejected`, nothing modified |
| Project database corrupt | detected on open by integrity check; `recover()` restores the newest sound backup and keeps the damaged file |
| Project from a newer Katana | refused with `Unsupported` rather than guessed at |
| Third-party library throws | converted to a `Result` at the module boundary |
