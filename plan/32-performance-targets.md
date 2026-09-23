<!-- Katana plan, section 32 of 47. Index: ../PLAN.MD. Previous: 31-ai-safety-model.md. Next: 33-memory-architecture.md -->

# 32. Performance Targets

Establish benchmarks progressively.

Initial targets:

```text
Viewport interaction:
60+ FPS

Simple selection:
< 16 ms

Simple geometry command:
< 100 ms

Undo/redo:
< 50 ms

Small project opening:
< 2 seconds
```

For large datasets:

```text
10M+ survey points:
interactive navigation

100M+ point cloud:
streamed visualization

1B+ points:
out-of-core / tiled / LOD architecture
```

These are engineering targets, not assumptions.

Benchmark on real datasets.

---

