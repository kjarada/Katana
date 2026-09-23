<!-- Katana plan, section 2 of 47. Index: ../PLAN.MD. Previous: 01-mission.md. Next: 03-primary-technology-stack.md -->

# 2. Core Architecture

Use this dependency hierarchy:

```text
AI Layer
    │
    ▼
C++ Application API
    │
    ▼
Command System
    │
    ▼
Domain Model
    │
    ├── CAD
    ├── Survey
    ├── Terrain
    ├── Civil
    └── GIS
    │
    ▼
Computational Core
    │
    ├── Geometry
    ├── Mathematics
    ├── Geodesy
    ├── Spatial Algorithms
    └── Numerical Computation
    │
    ▼
Infrastructure
    │
    ├── Storage
    ├── Memory
    ├── Concurrency
    └── File I/O
    │
    ▼
Rendering
    │
    ▼
Qt UI
```

The dependency direction must remain one-way.

Lower layers must never depend on higher-level application features.

For example:

```text
Geometry
```

must not depend on:

```text
Qt
Survey
AI
CAD UI
SQLite
```

---

