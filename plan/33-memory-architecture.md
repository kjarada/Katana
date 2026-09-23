<!-- Katana plan, section 33 of 47. Index: ../PLAN.MD. Previous: 32-performance-targets.md. Next: 34-testing-strategy.md -->

# 33. Memory Architecture

Optimize memory layout only after profiling.

Consider:

```text
SoA
AoS
memory pools
arena allocators
object pools
memory mapping
compressed storage
```

For massive point data, consider structures such as:

```text
X X X X X X
Y Y Y Y Y Y
Z Z Z Z Z Z
```

where appropriate instead of object-heavy layouts.

Avoid unnecessary heap allocations.

Measure:

```text
allocations/second
peak memory
resident memory
cache misses
memory bandwidth
```

---

