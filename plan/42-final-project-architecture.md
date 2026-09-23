<!-- Katana plan, section 42 of 47. Index: ../PLAN.MD. Previous: 41-definition-of-done.md. Next: 43-required-development-order.md -->

# 42. Final Project Architecture

The intended final system is:

```text
┌─────────────────────────────────────────────┐
│              PYTHON AI LAYER                │
│                                             │
│ LLM │ Agents │ Intent │ Context │ Planning  │
└──────────────────────┬──────────────────────┘
                       │
                 C++ Application API
                       │
┌──────────────────────▼──────────────────────┐
│              COMMAND ENGINE                 │
│                                             │
│ Commands │ Transactions │ Undo │ Redo       │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│               DOMAIN MODEL                  │
│                                             │
│ CAD │ Survey │ Terrain │ Civil │ GIS       │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│             COMPUTATIONAL CORE              │
│                                             │
│ Geometry │ Geodesy │ Numerical │ Spatial    │
└──────────┬───────────┬───────────┬──────────┘
           │           │           │
        Eigen        CGAL       PROJ
           │           │           │
      Open CASCADE   GDAL       PDAL
           │
┌──────────▼──────────────────────────────────┐
│          PERFORMANCE INFRASTRUCTURE         │
│                                             │
│ Threads │ SIMD │ Memory │ Cache │ GPU       │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│               VULKAN ENGINE                 │
└──────────────────────┬──────────────────────┘
                       │
┌──────────────────────▼──────────────────────┐
│                  QT UI                      │
└─────────────────────────────────────────────┘
```

---

