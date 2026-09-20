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
outright.

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
