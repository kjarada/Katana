<!-- Katana plan, section 34 of 47. Index: ../PLAN.MD. Previous: 33-memory-architecture.md. Next: 35-numerical-correctness.md -->

# 34. Testing Strategy

Every subsystem requires tests.

## Unit tests

Test individual algorithms.

## Integration tests

Test subsystem interactions.

## Regression tests

Protect known survey/CAD results.

## Property tests

Test mathematical invariants.

Examples:

```text
distance(A,A) == 0

area(reversed polygon) == area(polygon)

transform(inverse_transform(x)) ≈ x

length(reverse(polyline)) == length(polyline)
```

## Performance tests

Track benchmark regressions.

## Real-world datasets

Maintain a test corpus containing:

```text
small surveys
large surveys
terrain
point clouds
CAD drawings
complex parcels
large civil projects
```

---

