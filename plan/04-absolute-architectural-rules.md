<!-- Katana plan, section 4 of 47. Index: ../PLAN.MD. Previous: 03-primary-technology-stack.md. Next: 05-development-strategy.md -->

# 4. Absolute Architectural Rules

The AI coding agent must follow these rules throughout development.

## Rule 1 — C++ owns the application

All core functionality must be implemented in C++.

Python cannot become a hidden dependency of the core application.

## Rule 2 — AI never directly modifies the database

The AI must issue structured commands.

Correct:

```text
User
 ↓
AI
 ↓
Structured Command
 ↓
Validation
 ↓
C++ Command Engine
 ↓
Transaction
 ↓
Database
```

Incorrect:

```text
User
 ↓
AI
 ↓
Direct SQL / arbitrary memory manipulation
```

## Rule 3 — The renderer is not the source of truth

The domain model is authoritative.

The viewport is a representation of the domain model.

## Rule 4 — External libraries must be isolated

Do not allow external library types to contaminate the entire codebase.

Create internal interfaces.

Example:

```cpp
class CoordinateTransformer;
class GeometryKernel;
class TerrainEngine;
class PointCloudEngine;
```

rather than exposing third-party implementation types everywhere.

## Rule 5 — Test before optimization

Every important subsystem must have automated tests before aggressive optimization.

## Rule 6 — Profile before optimizing

Do not optimize based solely on assumptions.

Measure:

* CPU
* memory
* cache behavior
* allocation
* GPU
* I/O
* latency
* throughput

## Rule 7 — Preserve deterministic computation

Given identical inputs and configuration, computational results should be reproducible within defined numerical tolerances.

---

