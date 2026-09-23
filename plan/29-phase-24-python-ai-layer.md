<!-- Katana plan, section 29 of 47. Index: ../PLAN.MD. Previous: 28-phase-23-cpp-application-api.md. Next: 30-phase-25-ai-agent.md -->

# 29. Phase 24 — Python AI Layer

Python is introduced only at this stage.

Python must not implement:

```text
geometry
survey calculations
rendering
database logic
terrain algorithms
CAD algorithms
```

Python is responsible for:

```text
LLM integration
prompt construction
intent recognition
agent orchestration
AI tool selection
conversation
AI context management
```

Architecture:

```text
User
 ↓
Python AI
 ↓
Structured command
 ↓
C++ API
 ↓
Validation
 ↓
Execution
 ↓
Result
 ↓
Python AI
 ↓
User
```

---

