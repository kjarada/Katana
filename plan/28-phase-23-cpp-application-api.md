<!-- Katana plan, section 28 of 47. Index: ../PLAN.MD. Previous: 27-phase-22-drawing-and-plotting.md. Next: 29-phase-24-python-ai-layer.md -->

# 28. Phase 23 — C++ Application API

**STATUS: NOT STARTED. Reshaped, and roughly half the list is already built.**

The JSON command shape below duplicates `katana::cad::CommandInterpreter`,
which already parses verbs, validates arguments, returns `Result<T>` and routes
every mutation through the `Command` undo stack. A second command parser beside
it is precisely the "second way of doing something that already has a first
way" that CLAUDE.md section 6 says to collapse.

What is genuinely missing is not a command language but a **stable, typed,
header-only-facing surface** that a caller outside `src/` can link against -
which is a new `katana_api` module at the top of the layering graph, not a new
interpreter. `query entities`, `query terrain`, `run spatial queries`,
`create surfaces` and `export data` in the list below are all already reachable
in-process; they need exposing, not implementing.

The JSON example is retained below as the *wire format for a future
out-of-process caller*, not as the in-process API.

Before implementing AI, expose a stable C++ application API.

The API should support:

```text
query entities
create entities
modify entities
delete entities
execute commands
query survey data
query terrain
run calculations
run spatial queries
create surfaces
export data
```

The API must use structured commands and typed data.

Example:

```json
{
  "command": "CREATE_LINE",
  "start": [100.0, 200.0],
  "end": [300.0, 500.0]
}
```

The C++ application must validate this command.

---

