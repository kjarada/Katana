<!-- Katana plan, section 11 of 47. Index: ../PLAN.MD. Previous: 10-phase-05-entity-system.md. Next: 12-phase-07-project-storage.md -->

# 11. Phase 06 — Command System

**STATUS: DELIVERED.** 31 tests at delivery.

`ChangeSet` (add / modify / remove) validated before anything is touched, then
applied atomically and reported through change events. `CommandStack` carries
undo and redo stacks plus save-point tracking, and `Transaction` groups several
commands into one undo step. About 28 entity and layer commands.

Undo restores **recorded before-images**, never a computed inverse: a computed
inverse drifts in floating point and cannot represent a failed partial apply.
CORRECTED 2026-09-23: not quite never - creating and renaming a layer are
undone by computed inverses, which leave behind the ancestor layers the command
created implicitly, and so does a Transaction's rollback (audit MOD-07, open).

---

