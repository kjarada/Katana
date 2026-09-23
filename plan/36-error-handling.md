<!-- Katana plan, section 36 of 47. Index: ../PLAN.MD. Previous: 35-numerical-correctness.md. Next: 37-logging.md -->

# 36. Error Handling

Do not use silent failures.

Every subsystem should have structured error handling.

Examples:

```text
InvalidGeometry
InvalidCRS
InvalidSurveyObservation
AdjustmentFailure
TriangulationFailure
FileImportFailure
DatabaseFailure
RenderingFailure
```

Errors should contain enough information for diagnostics.

---

