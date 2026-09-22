# Storage — which database, and why

*Decision record. PLAN.MD §32 (performance targets), §33 (memory), Rule 6
(profile before optimising).*

## The question

The project database is SQLite. Should it be DuckDB, something else, or
something we write ourselves?

## What was measured

`benchmarks/bench_storage.cpp`, Release, GCC 15.2 UCRT64, 16 × 2496 MHz,
L3 18 MiB. The model is shaped like real survey data: 8-vertex polylines on
four nested layers, two attributes each.

| Operation (50 000 entities) | Time |
|---|---|
| `ProjectStore::save` — whole project | **445 ms** |
| `ProjectStore::open` + `load` + `applyToModel` | **891 ms** |
| Geometry → JSON only (no database) | **172 ms** |
| JSON → geometry only (no database) | **602 ms** |

Scaling is linear across 1 000 / 10 000 / 50 000 in every case.

## What that says

Subtracting the encoding from the storage figures:

| Opening a 50 000-entity project | Time | Share |
|---|---|---|
| **JSON decoding of geometry** | **602 ms** | **68 %** |
| SQLite read + rebuilding the model | 289 ms | 32 % |

**The database is not the bottleneck. The geometry encoding is, by more than
two to one.** Saving splits 172 ms of encoding against 273 ms of everything
else, so writing is more balanced, but reading — which is what a user waits for
— is dominated by parsing JSON text into `Geometry` values.

That single measurement decides the question, because it says any database
change is competing for at most a third of the time, while a format change is
competing for two thirds.

## Decision

**Keep SQLite for the project database. Do not adopt DuckDB for it. Do not
write our own.**

### Why not DuckDB

DuckDB is a genuinely excellent engine, and it is the wrong one for this table.

- **It is built for the opposite workload.** DuckDB is a columnar, vectorised
  OLAP engine: it is outstanding at scanning millions of rows and aggregating a
  few columns. The project store does the reverse — it reads a few thousand
  rows *whole*, each carrying a variable-length geometry blob, and it writes
  them one transaction at a time. Column-at-a-time execution has nothing to get
  hold of here, and the blob is opaque to it either way.
- **It would attack 32 % of the cost, optimistically.** Even a database that
  took zero time would leave 68 % of the open untouched.
- **Durability is the requirement, and SQLite's is the one that is proven.**
  This project has already been bitten by crash recovery: a hot rollback
  journal misdiagnosed as corruption, a journal renamed to a relative path, a
  backup chosen by filename sort. Those are fixed and tested *against SQLite's
  journal and WAL semantics*, which are specified, ubiquitous and unchanged for
  two decades. DuckDB's single-writer model and its checkpointing have moved
  considerably more over the same period. Trading a proven recovery story for a
  third of a cost we are not paying is a bad trade.
- **It is not packaged for this toolchain.** DuckDB is absent from MSYS2
  UCRT64, so it would have to be vendored and built from the amalgamation —
  a multi-minute addition to every clean build, for the above.
- **Weight.** SQLite is ~250 KB of object code and is already a dependency of
  GDAL and PROJ, so it costs nothing extra. DuckDB is tens of megabytes.

### Why not write our own

A storage engine's value is entirely in the part that is hard to test: what
happens when power is lost between the write and the fsync. SQLite has decades
of adversarial testing on exactly that, on filesystems we will never see. We
would be trading a solved problem for an unbounded one, to save 289 ms on a
50 000-entity project. No.

### What to do instead — in priority order

1. ~~**Replace the JSON geometry encoding with a binary one.**~~ **DONE** — see
   "The result" below. Predicted "most of 602 ms on load, most of 172 ms on
   save"; actual was 696 ms and 239 ms.
2. **Keep the row-per-entity shape.** It is what makes partial load, per-layer
   queries and incremental save possible later.
3. **Revisit only with a new measurement.** If binary encoding lands and SQLite
   is then the largest remaining term, measure again and reopen this.

### Where DuckDB *would* earn its place

Not the project database — the **analytical** store, which does not exist yet:

- point clouds at 10⁸–10⁹ points (PLAN.MD Phase 17's out-of-core half),
- survey observations and adjustment residuals queried across jobs,
- anything answering "every point in this polygon below this elevation,
  grouped by classification".

That is columnar scanning over immutable bulk data — exactly what DuckDB is
for, and exactly what SQLite is bad at. **Apache Arrow and Parquet are already
installed** in this toolchain as GDAL dependencies, and PDAL can write Parquet,
so the cheap first step is to store bulk point data as Parquet and query it
with Arrow, adding DuckDB only if the queries outgrow that.

Whatever is chosen there, it goes **behind a Katana interface in `src/`**, with
no third-party type in a public header (Rule 4) — the same isolation GDAL and
PDAL already have.

## The result

Schema version 3 stores geometry as a versioned little-endian blob
(`include/katana/entity/geometry_blob.hpp`). Same machine, same benchmark:

| 50 000 entities | Before | After | |
|---|---|---|---|
| `open` + `load` + `applyToModel` | 891 ms | **195 ms** | 4.6× |
| `save` | 445 ms | **206 ms** | 2.2× |

Measuring the two encodings head to head, on the same 50 000 geometries with
no database involved:

| | JSON | Blob | |
|---|---|---|---|
| Encode | 205 ms | **19.8 ms** | 10.4× |
| Decode | 587 ms | **3.61 ms** | **163×** |

Decoding is the one that mattered, and it is now effectively free: 3.6 ms
against 587 ms. Reading a length and memcpy-ing doubles is not comparable work
to lexing text and converting decimal to binary.

**The load saved 696 ms, of which decoding accounts for 583 ms.** The remaining
~113 ms is most likely the string handling that went with the JSON: that path
pulled each geometry out of `columnText` as a `std::string` and then allocated
again while parsing, where the blob is read into one buffer and decoded in
place. The arithmetic is consistent with that, but the 113 ms has not been
measured directly — treat it as an explanation, not a result.

Doubles are stored by bit pattern, so a round trip is exact — negative zero,
denormals and infinities included. A saved drawing reloads as the same drawing,
not one that agrees to fifteen digits, which is what Rule 7 requires.

### Migration

The blob column was **added**, not swapped in. Rows written before schema 3
keep their JSON and are read from it; they convert the next time the project is
saved. Where both are present the blob wins, because it is the newer of the
two — asserted by a test that plants a deliberately *different* circle in the
JSON column and requires the blob's geometry to come back.

An older build refuses a schema-3 project outright (`Unsupported`, "project was
written by a newer version of Katana") rather than reading the now-empty JSON
column and producing an empty drawing.

### One trap found on the way

`std::string_view{}` has a **null** `data()`, and SQLite binds a null pointer as
SQL NULL rather than as empty text. Writing the legacy column as
`std::string_view{}` therefore violated its `NOT NULL` constraint. The fix is in
the wrapper rather than at the call site: `SqliteStatement::bind` now
substitutes a pointer to an empty string, so no future caller can mean "empty"
and silently get NULL.

## What is still open

SQLite now accounts for the majority of what remains (195 ms for 50 000
entities). Before reopening the engine question, note that the remaining time
includes `applyToModel` — validating and inserting every entity into the model —
which no database change would touch. Measure that split first.

## Reproducing

```sh
cmake --build build/release --target katana_benchmarks
./build/release/benchmarks/katana_benchmarks.exe \
    --benchmark_filter="ProjectSave|ProjectOpen|GeometryJson" --benchmark_min_time=0.3s
```

Re-run this before changing anything in this document. A decision recorded
without its measurement is an opinion.

## Loading a project without copying it twice

Opening a 50 000-entity project allocated 1.38 million times, and 287 563 of
those - 22.6% - were `applyToModel` duplicating every `Entity` on its way from
the loaded `ProjectContents` into the model. The contents are thrown away on the
next line.

`applyToModel` therefore has two overloads, a copying one and a consuming one,
sharing a single templated body so that the insertion order and the built-in
name rules cannot drift apart between them. `Document::open` uses the consuming
one, taking the metadata out **before** the call and installing it only after
the apply succeeds - a failed open must leave the document exactly as it was,
and that promise is what the staging in `applyToModel` exists to keep.

`ConsumingTheContentsBuildsExactlyTheModelCopyingThemDoes` builds a project with
something in every table `applyToModel` fills, applies one copy of it each way
and compares the two models value for value - not by id, because the failure a
move introduces is a field left behind, not a missing record. Rewriting the
layer loop as the plausible "add, and fall back to update if it is already
there" - which reads a layer after moving from it - fails that test; that was
checked by writing it.

`SqliteStatement::columnTextView` and `columnBlobSpan` borrow SQLite's own row
buffer for values parsed and discarded within the row. `columnText` and
`columnBlob` are now those plus a copy, so one place knows how SQLite hands a
value over. The lifetime rule is on the declarations, and it is sharp: a view
dies at the next `step()`, `reset()` or `run()`, and at a second read of the
same column through a different accessor, because SQLite converts in place.

The entity vector is sized from `SELECT COUNT(*)` rather than from
`nextEntityId`. The counter is free but is only an upper bound, and a drawing
that has had most of its entities deleted would reserve for its ids instead of
its rows.
