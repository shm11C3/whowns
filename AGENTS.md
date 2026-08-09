# Repository Guidance

## Design principles

- Apply YAGNI at all times. Implement only what the current requirement needs.
- Before changing the code, be able to explain:
  - **Why** the change is needed.
  - **What** must change.
  - **How** the smallest coherent solution works.
  - **Why Not** plausible alternatives are unnecessary or worse for the current requirement.
- Low implementation cost does not justify a change. Verify its present necessity; cheap branches and exceptions still accumulate into complexity.
- Treat review feedback as evidence of a problem, not as a prescribed patch. Identify the essential issue, its root cause, and the boundary that should own it before changing code.
- Prefer simple, direct code with clear responsibilities.
- Simplicity is not an excuse for brittle, duplicated, or hard-to-maintain code. Keep the design easy to understand and able to accommodate reasonable changes to current requirements.
- Introduce an abstraction only when the present code or requirement demonstrates the need for it.

## Prohibited changes

- Do not overengineer.
- Do not add speculative considerations, capabilities, extension points, or features that are not needed now.
- Do not add dependencies.
