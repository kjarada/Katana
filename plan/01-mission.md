<!-- Katana plan, section 1 of 47. Index: ../PLAN.MD. Previous: 00-introduction.md. Next: 02-core-architecture.md -->

## 1. Mission

Build a professional, high-performance CAD and surveying application from the ground up.

The system must prioritize:

1. Numerical correctness
2. Deterministic behavior
3. High performance
4. Modular architecture
5. Testability
6. Large-dataset support
7. Professional CAD workflows
8. Professional surveying workflows
9. Long-term extensibility
10. AI-assisted operation

The application must be implemented primarily in **C++**.

Use **C** only where low-level C implementations provide a clear benefit.

Use **Python exclusively for the AI layer and AI-related tooling**.

Do **not** use:

* Blender
* Rust
* Python for the core application
* Python for geometry
* Python for surveying calculations
* Python for rendering
* Python for the main UI

The C++ application must remain fully functional without the AI layer.

---

