<!-- Katana plan, section 40 of 47. Index: ../PLAN.MD. Previous: 39-code-quality-requirements.md. Next: 41-definition-of-done.md -->

# 40. AI Coding Workflow

For every implementation task, follow this sequence:

```text
1. Inspect existing architecture
2. Identify affected modules
3. Define API
4. Implement smallest correct version
5. Add unit tests
6. Add integration tests if necessary
7. Compile
8. Run tests
9. Run sanitizers
10. Benchmark if performance-sensitive
11. Review dependencies
12. Document the implementation
```

Never blindly rewrite large sections of the project.

---

