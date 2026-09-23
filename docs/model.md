# Domain model, commands and storage

`katana_entity`, `katana_commands` and `katana_storage` — the authoritative
drawing data, the only way to change it, and how it is persisted.

## Purpose

The domain model is the single source of truth (PLAN.MD Rule 3). The renderer,
the property panel and the command line all read from it and none of them keeps
a private copy that could drift. Every modification goes through a command, so
every modification is validated, atomic, undoable and observable — including,
later, modifications requested by the AI layer (Rule 2).

```
Command → validation → change set → model → change events → views
```

## Entity system

An `Entity` is a value: an id, a geometry, presentation attributes and two
open-ended property maps.

```cpp
struct Entity {
    EntityId id;                  // uint64, 1-based
    Geometry geometry;            // variant of the seven entity geometries
    std::string layer;            // "0" by default
    std::string style;            // empty means ByLayer
    std::optional<Color> color;   // empty means ByLayer
    bool visible;
    PropertyMap properties;       // user/application attributes
    PropertyMap metadata;         // provenance: source file, import time, author
};
```

`Geometry` is a `std::variant` over `PointGeometry`, `Segment2`, `Arc2`,
`Polyline2`, `Circle2`, `TextGeometry` and `DimensionGeometry`, ordered to match
`EntityType` so `typeOf()` is a cast of the variant index. Using a variant rather
than an inheritance hierarchy keeps entities copyable values, makes exhaustive
handling a compile-time property, and means the database owns its entities
outright. **The order is load-bearing** - see "Adding a geometry kind" below.

`PropertyMap` is an ordered `std::map` of `variant<bool, int64_t, double,
string>`. Ordered, because serialisation, diffs and iteration must be
deterministic.

Three geometry-generic operations live in `entity_geometry.hpp`:

* `validate` — rejects what must never enter the model: non-finite coordinates,
  non-positive radii or text heights, arcs sweeping more than a full turn,
  polylines with fewer than two distinct vertices, zero-length lines.
* `boundingBox` — used for culling and picking. Text extents are approximated at
  0.6 × height per character because the entity layer has no font metrics; this
  is documented at the call site and refined by the renderer.
* `transformed` — applies a similarity transform. Non-similarity matrices
  (shear, non-uniform scale) are **rejected** with `InvalidArgument`, because
  they would turn a circle into an ellipse, which is not a representable entity.
  Mirroring reverses arc sweep and dimension side so the result is geometrically
  faithful rather than merely mirrored coordinate-wise.

### Databases

`EntityDatabase` owns entities keyed by id in a `std::map`.

* **Ids are monotonic and never reused**, even after deletion. An id recorded by
  an undo step or an external reference can therefore never come to mean a
  different entity — the property that makes undo/redo safe across long
  sessions.
* Iteration is by ascending id.
* Every mutation reports a `ChangeEvent` to a single observer, which the command
  stack uses to attach entity-level detail to command events.
* `remove` returns the removed entity so the caller can restore it; `insert`
  puts an entity back under the id it already carries.

`LayerDatabase`, `StyleDatabase` and `PropertyDatabase` are ordered name-keyed
tables. Layer `"0"` always exists and cannot be removed or renamed.
`PropertyDatabase` holds an optional schema: a property that has been defined is
type-checked on assignment, and one that has not is free-form.

`Model` aggregates all four.

## Command system

```cpp
class Command {
    virtual std::string_view name() const = 0;       // "CREATE_LINE"
    virtual bool isDestructive() const;              // needs confirmation
    virtual Status validate(const CommandContext&) const = 0;
    virtual Status execute(CommandContext&) = 0;
    virtual Status undo(CommandContext&) = 0;
    virtual Status redo(CommandContext&) = 0;
};
```

Most commands are a `ChangeSetCommand`: a named function from the current model
to a `ChangeSet` of entities to add, modify and remove. That single shape gives
every command the same guarantees:

* **Validation before mutation.** `validateChangeSet` checks that modified and
  removed entities exist and sit on unlocked layers, that nothing is listed
  twice, and that added and modified entities have valid geometry, an existing
  unlocked layer, an existing style and well-typed properties. A command that
  fails validation leaves the model untouched and never enters the history.
* **Atomicity.** Multi-entity commands apply completely or not at all. Scaling
  a selection where one member would collapse below `kGeometric` fails as a
  whole; the entities that could have scaled do not move.
* **Exact undo.** Undo restores recorded before-images, not computed inverses.
  Fifty random rotate-and-scale operations undo to bit-identical coordinates —
  a test asserts exactly this with `operator==`, no tolerance. Redo replays the
  recorded after-images and reinstates the *same* entity ids, so later history
  that refers to them stays valid.

`CommandStack` owns the history, publishes `CommandEvent`s carrying the
entity-level changes, and tracks a save point: `isModified()` is true whenever
the model differs from the last `markSaved()`, including after undoing past it,
and stays true once the saved state becomes unreachable because new work
replaced the redo branch.

`Transaction` groups commands into one undoable step. Only its first member can
be validated up front, since later members may depend on earlier effects (create
a layer, then draw on it); if a later step fails, the ones already executed are
rolled back in reverse order and the error names the failing step.

Available commands: `CREATE_POINT`/`LINE`/`CIRCLE`/`ARC`/`POLYLINE`/`TEXT`/
`DIMENSION`/`ENTITIES`, `DELETE`, `MOVE`, `ROTATE`, `SCALE`, `MIRROR`, `COPY`,
`ARRAY`, `TRIM`, `EXTEND`, `OFFSET`, `FILLET`, `CHAMFER`, `SET_LAYER`,
`SET_COLOR`, `SET_STYLE`, `SET_VISIBLE`, `SET_PROPERTY`, `REMOVE_PROPERTY`,
`SET_GEOMETRY`, `CREATE_LAYER`, `UPDATE_LAYER`, `DELETE_LAYER`. These names are
stable identifiers and will become the vocabulary of the structured API in
Phase 23.

## Storage

A project is a directory:

```
site.katana/
├── project.db     entities, layers, styles, property definitions,
│                  relationships, metadata
├── backups/       timestamped snapshots, pruned to the newest N
├── terrain/       large datasets live in files, not in SQLite rows
├── pointcloud/
├── assets/
└── cache/         disposable
```

`SqliteDatabase` is a thin RAII wrapper — `sqlite3.h` appears in no header, and
every call converts SQLite's return codes into `DatabaseFailure` carrying
SQLite's own message. The connection opens with `synchronous = FULL` and a
rollback journal (rather than WAL), so a committed save survives power loss and
a project remains a single file at rest, which matters on network shares.

`ProjectStore` provides:

* **Atomic save.** One transaction rewrites the whole model. A crash mid-save
  leaves the previous state intact. Rewriting everything is simple and correct;
  incremental saves driven by change events are a later optimisation, to be made
  only with profiling data.
* **Exact values.** Doubles round-trip bit for bit — asserted over 500 random
  values at survey magnitudes.
* **Migrations.** `PRAGMA user_version` holds the schema version. Older projects
  migrate step by step on open, after an automatic backup. Released migrations
  are append-only and are never edited. Projects written by a newer Katana are
  refused with `Unsupported` rather than guessed at.
* **Recovery.** `open` runs an integrity check. `recover` restores the newest
  backup that passes its own integrity check, moves the damaged file aside with
  a timestamp rather than deleting it, and moves its rollback journal with it so
  it cannot be replayed onto the restored database.

`captureModel` and `applyToModel` convert between a live `Model` and the plain
`ProjectContents` value. `applyToModel` validates the whole of the contents —
ids unique and non-zero, layers and styles resolvable, geometry valid,
`nextEntityId` above every stored id, relationships resolvable — *before*
touching the model, so opening a damaged project cannot leave a half-populated
drawing on screen.

Geometry and properties are stored as JSON text via
`katana_entity/serialization.cpp`, which keeps nlohmann confined to one
translation unit and keeps the schema readable for debugging and diffing.

## Numerical assumptions

The model stores `double` coordinates in model units (metres unless a project
says otherwise) and neither rounds nor normalises them. Validation uses
`kGeometric` for degeneracy. Storage preserves values exactly; no tolerance is
applied on save or load.

## Threading and ownership

Single-threaded. `EntityDatabase`, `Model`, `CommandStack` and `ProjectStore`
each belong to one thread — the document thread — and none is internally
synchronised. `CommandStack` holds the model by reference and the model must
outlive it; the stack installs itself as the model's entity observer for its
lifetime and clears it on destruction. `Document` owns the model, the stack and
the store, and rebuilds the stack when the document is replaced.

## Performance

Entity lookup, insertion and removal are O(log n); `bounds()` and layer queries
are O(n). `std::map` was chosen for ordering and simplicity, not for speed; the
plan requires profiling before optimisation, and the structure-of-arrays layout
contemplated in PLAN.MD §33 is a Phase 19 decision to be taken with measurements
in hand. Save and load are O(n) in entities with one prepared statement reused
across rows.

Against PLAN.MD §32's targets, the operations in this layer — command execution,
undo/redo — are far below the 100 ms and 50 ms budgets at drafting scale; they
have not yet been measured on a 10⁶-entity drawing.

## Failure modes

| Condition | Result |
|---|---|
| Invalid geometry | `InvalidGeometry`, model untouched, no history entry |
| Entity or layer not found | `NotFound` |
| Duplicate id or name | `AlreadyExists` |
| Entity on a locked layer edited | `CommandRejected` |
| Layer still in use deleted | `CommandRejected` naming the entity count |
| Default layer removed | `CommandRejected` |
| Command would change nothing | `CommandRejected` |
| Transaction step fails | earlier steps undone, error names the step |
| Undo/redo with empty history | `InvalidState` |
| SQLite error | `DatabaseFailure` with SQLite's message |
| Corrupt project | detected on open; `recover()` restores a backup |
| Project schema too new | `Unsupported` |

## Adding a geometry kind

`static_assert(std::variant_size_v<Geometry> == 7)` in `entity.hpp` points here.
The count is deliberately awkward to change, because the fan-out is wide and
some of it does not fail at compile time.

### Two rules that are not negotiable

**Append only. Never insert in the middle, never reorder.** The variant *index*
is the on-disk kind byte — `geometry_blob.cpp` writes `geometry.index()` — so a
reorder reinterprets every project ever saved. Most shifts are caught by the
payload-length checks and surface as "truncated" or "trailing bytes", but **two
kinds of equal payload size swap with no complaint at all** and the drawing
reloads as the wrong shapes. The JSON path is unaffected because it keys on the
type *name*, which is exactly why this would be missed: it looks
encoding-specific. `GeometryBlobWireFormat` pins the mapping, including against
a hand-written byte sequence.

**Add the `EntityType` enumerator at the same ordinal.** `typeOf()` casts the
variant index straight to it, so the two orders are one thing, not two.

### The compiler will stop you here

These are exhaustive visitors: `std::visit` refuses to compile until they are
complete. Getting them green fixes picking, box selection, the spatial index,
the viewport cull and validation for free.

| Where | What |
|---|---|
| `entity_geometry.cpp` | `toString`, `Validator`, `boundingBox`, `distanceTo`, `transformed` |
| `serialization.cpp` | `toJsonValue` visitor, `geometryFromJsonValue` switch |
| `geometry_blob.cpp` | writer chain (`static_assert` on the trailing `else`), reader switch |
| `snapping.cpp` | `EntitySnaps` |
| `selection.cpp` | `TouchesBox` |
| `scene.cpp` | `appendEntities` (`static_assert` on the trailing `else`) |
| `export.cpp` | the export visitor (`static_assert` on the trailing `else`) |
| `command_interpreter.cpp` | `describe`'s `Detail` visitor |
| `viewport_widget.cpp` | `drawGeometry`'s visitor |
| `main_window.cpp` | `describeGeometry`'s visitor |

### The compiler will NOT stop you here

These opted out of exhaustive dispatch, mostly for good reasons. Each is a
place where a new kind is silently absent rather than reported.

| Where | What is silently lost |
|---|---|
| `snapping.cpp` `appendCurves` | a `get_if` chain. A curve-like kind contributes no curves, so Intersection and Nearest snap stop working for it — no error, the cursor just will not snap. |
| `spatial_query.hpp` `queryExtents` | special-cases `Arc2` because an arc's centre lies outside its bounding box. A kind with any snap point outside its bbox is rejected by the broad phase — and because that is index-dependent, it can work on a small drawing and fail as the drawing grows, looking like a performance flake. |
| `section.cpp` `appendCrossings` | a curve-like kind produces no section crossings. **The section comes out missing features and looks complete** — a wrong answer in a signed deliverable. |
| `main_window.cpp` `buildSurfaceFromDrawing` | a kind carrying surveyed vertices contributes nothing, and the TIN comes out missing a breakline while looking plausible. |
| `editing.hpp` `Curve2` | a second variant. A curve-like kind almost certainly belongs here too; it has its own `static_assert`. |
| `entity_commands.cpp` | `asCurve`, `edgeCurves`, `asSegment` decide what is trimmable, offsettable and filletable. |
| `import.cpp` | maps `gis::GeometryKind` inwards; only Point/LineString/Polygon exist there. |
| `project_store.cpp` | `kCurrentSchemaVersion` does not change for a new kind, so an older build does not refuse the project up front — it opens it and fails per row with "unknown geometry kind in blob". Loud, but late, and phrased as corruption rather than version skew. Consider bumping it. |

### Tests with a hand-written "one of each" corpus

Each of these will otherwise keep passing while never touching the new kind,
under a name that claims full coverage:

- `test_entity.cpp` `oneOfEachGeometry()` — asserts its own size against
  `variant_size_v` so it fails loudly instead.
- `test_geometry_blob.cpp` — the `samples` lists, and `GeometryBlobWireFormat`,
  which asserts its pinned table covers every alternative.
- `test_commands.cpp`, `test_cad.cpp`, `test_storage.cpp` — per-type creation,
  interpreter round-trip and save/load corpora.

### Then

Update the `static_assert` count, this document, `PLAN.MD` and `README.md`.

## Layer visibility and lock: one rule, asked everywhere

A layer is shown when it and every ancestor is visible, and locked when it or
any ancestor is locked (PLAN.MD 5.1). The rule has one implementation,
`LayerDatabase::resolve`, and every consumer goes through it - `cad::isDrawn`
and `cad::isSelectable`, which the viewport, the plot, picking, box selection,
snapping, the 3D scene, sections and Surface From Drawing ask, and the
`requireUnlockedLayer` check every command runs. Until 2026-09-23 the layer
panel was the only place that honoured inheritance (audit MOD-01).

The inherited state is STORED, not walked: each map node holds its layer and
`shown`/`locked`, and every member that changes the table recomputes the
subtree it touched. Rejected: walking the ancestors on each query, which is what
`effectivelyVisible` did - it allocated a vector of ancestor names per call, and
the viewport asks for every entity on every frame; and a per-frame cache in each
caller, which is nine callers each able to forget it. The recomputation is
cheap because the state of a layer depends only on itself and its ancestors: an
update or rename moves only its own subtree, and adding a layer moves nothing
else, since every layer's ancestors already exist.

The subtree is the node plus the keys from `name/` to `name0` - NOT the keys
after the node, because space, `-` and `.` sort below `/` and put "design 2"
between "design" and "design/surface". `hasChildren` made that mistake until
the same change, and `remove` orphaned children because of it.

## Named tables

`Linetype`, `DimensionStyle`, `Style`, `HatchPattern` and `Alignment` are stored in `NamedTable<T, Policy>`
(`include/katana/entity/named_table.hpp`), one implementation of "an ordered map
from name to record" with the add / update / remove / find / all surface, the
`AlreadyExists` and `NotFound` shape, and the built-in entry that `remove`
refuses. They were three hand-written copies of that; the copies differed only
in their noun, their validation and which name was protected, and those are now
the policy.

**To add another kind of named table**: write a `Policy` beside the others in
`tables.hpp` giving `kNoun` and `validate`, plus `isProtected` and `seed` if it
has a built-in entry and `checkUpdate` if it has a rule that applies to updates
but not to adds; then `using XDatabase = NamedTable<X, XPolicy>;`. The
container needs nothing else.

Everything *around* the table is still by hand, and - exactly as for a geometry
kind - the list divides into the parts the compiler catches and the parts it
does not.

*The compiler will stop you here:*

* a field on `entity::Model`;
* `ProjectContents` in `storage/project_store.hpp`;
* the create / update / delete commands.

*The compiler will NOT stop you here. Each of these fails silently:*

* **`Model::adoptContents`** - a hand-written list of moves, and a table left
  out of it is dropped on every load with no error anywhere. This is not
  hypothetical: `hatchPatterns` was lost exactly this way, and only a
  round-trip test that saved a pattern and looked for it again caught it.
* **`Model::reset`** - a table left out keeps its contents across File > New.
* **`captureModel` and `applyToModel`** - omit either and the table saves or
  loads as empty.
* **The `DELETE FROM` list in `ProjectStore::save`** - `save` clears and
  rewrites every table it owns, so a missing `DELETE` fails on the *second*
  save with a primary-key conflict, not the first. Both the linetype and the
  dimension style tables shipped this bug and were caught by saving twice in a
  test. Do that.
* **The schema migration** - a new column needs a `DEFAULT` that reproduces the
  behaviour of projects written before it existed, or old drawings change
  appearance on open.

The five tables that ARE plain named tables - linetypes, dimension styles,
hatch patterns, alignments and styles - share one implementation of their
create, update and delete commands (`src/katana_commands/table_commands.cpp`),
parameterised by a `TablePolicy<T>` that holds the only things that differ:
the item that may not be deleted, an update that may not be made, and who
still uses an item so a deletion is refused naming the holder. There used to
be four hand-written copies; adding a fifth for styles is what made the
pattern visible, and a change to how these commands undo now lands in one
place.

`LayerDatabase` is deliberately not one of these. Layer names are `/`-separated
paths and the table derives a tree from them (`children`, `subtree`,
`removeSubtree`, `rename`), so it is a different structure that happens to be
keyed by name. `PropertyDatabase` is not one either: it validates *values*
against definitions, which no other table does.

## Reading a named table without copying it

`NamedTable::all()` returns a `std::vector<T>` **by value**, which is right for
a caller that wants to keep the answer and wasteful for one that only wants to
read it - the viewport called `all()` once a frame and deep-copied every
alignment, profile and PI vector in order to draw them. `forEach(visit)` walks
the records in the same name order without copying any of them. `all()` stays:
the copy is what makes a caller safe to go on and modify the table.

`ChangeSetCommand` no longer keeps an image of every entity it creates.
`EntityDatabase::remove()` returns the entity it removed, and `undo()` always
runs before `redo()`, so the image `redo()` needs is captured at undo time
instead of copied at execute time. See `docs/performance.md` for what that cost:
12.25 allocations per created entity, now 4.75.
