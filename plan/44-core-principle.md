<!-- Katana plan, section 44 of 47. Index: ../PLAN.MD. Previous: 43-required-development-order.md. Next: 45-survey-module-and-instruments.md -->

# 44. Core Principle

The final product should **not** be an AI wrapped around a CAD program.

It should be:

```text
A high-performance deterministic C++ engineering platform
                    +
             A controlled AI interface
```

The C++ engine must be capable of performing every important engineering operation independently.

AI should provide:

```text
natural-language interaction
automation
workflow generation
data querying
command composition
documentation assistance
```

The AI must never replace the deterministic geometry, surveying, numerical, or engineering systems.

The long-term objective is therefore:

```text
                 AI
                  │
                  ▼
        ┌───────────────────┐
        │ C++ Command Layer │
        └─────────┬─────────┘
                  │
                  ▼
        ┌───────────────────┐
        │ Engineering Core  │
        └─────────┬─────────┘
                  │
        ┌─────────┼──────────┐
        ▼         ▼          ▼
       CAD      Survey     Terrain
        │         │          │
        └─────────┼──────────┘
                  ▼
             High-Speed
             C++ Engine
                  │
                  ▼
              GPU / UI
```

**Build the deterministic C++ engine first. Add intelligence only after the engine exposes reliable, structured commands.**

