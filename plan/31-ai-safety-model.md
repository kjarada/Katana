<!-- Katana plan, section 31 of 47. Index: ../PLAN.MD. Previous: 30-phase-25-ai-agent.md. Next: 32-performance-targets.md -->

# 31. AI Safety Model

The AI must never receive unrestricted access to the C++ process.

Use a controlled command interface.

Every AI command must pass through:

```text
Schema validation
 ↓
Permission validation
 ↓
Domain validation
 ↓
Numerical validation
 ↓
Transaction
 ↓
Execution
```

For destructive operations:

```text
AI
 ↓
Proposed command
 ↓
User confirmation
 ↓
C++ execution
```

Example:

```text
DELETE 1,842 ENTITIES
```

must not execute automatically unless the application policy explicitly permits it.

---

