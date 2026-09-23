<!-- Katana plan, section 39 of 47. Index: ../PLAN.MD. Previous: 38-documentation.md. Next: 40-ai-coding-workflow.md -->

# 39. Code Quality Requirements

The AI coding agent must:

* Keep functions focused
* Avoid unnecessary abstractions
* Avoid premature optimization
* Avoid global mutable state
* Use RAII
* Prefer value semantics where appropriate
* Clearly define ownership
* Minimize raw pointers
* Use smart pointers where ownership requires them
* Avoid unnecessary inheritance
* Prefer composition
* Keep dependencies directional
* Keep public APIs small

Do not generate large speculative frameworks before they are needed.

---

