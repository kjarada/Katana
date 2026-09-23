<!-- Katana plan, section 30 of 47. Index: ../PLAN.MD. Previous: 29-phase-24-python-ai-layer.md. Next: 31-ai-safety-model.md -->

# 30. Phase 25 — AI Agent

Implement progressively.

## Level 1 — Simple commands

Example:

```text
"Create a line between points 100 and 120."
```

AI produces:

```json
{
  "command": "CREATE_LINE",
  "start_point": 100,
  "end_point": 120
}
```

## Level 2 — Queries

Example:

```text
"Find all survey points below elevation 100."
```

## Level 3 — Multi-step workflows

Example:

```text
"Create a surface from today's survey points and generate 1-foot contours."
```

AI produces a command sequence:

```text
IMPORT_POINTS
BUILD_TIN
CREATE_CONTOURS
DISPLAY
```

## Level 4 — Engineering workflows

Example:

```text
"Calculate the cut and fill between the existing and proposed surfaces."
```

AI orchestrates the appropriate C++ commands.

---

