<!-- Katana plan, section 12 of 47. Index: ../PLAN.MD. Previous: 11-phase-06-command-system.md. Next: 13-phase-08-basic-cad-application.md -->

# 12. Phase 07 — Project Storage

**STATUS: DELIVERED.** 13 tests at delivery.

SQLite behind `sqlite_database.hpp` (RAII pimpl; `synchronous=FULL`,
`journal_mode=DELETE`, `foreign_keys=ON`; the online backup API for snapshots).
`project_store` carries the application id `0x4B544E41` ("KTNA"), a schema
version, an append-only migration table, backup rotation, and `recover()`, which
restores the newest sound backup and moves the damaged file aside rather than
deleting it.

---

