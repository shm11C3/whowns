# Repository Guidance

## Design principles

- Apply YAGNI at all times. Implement only what the current requirement needs.
- Before changing the code, be able to explain:
  - **Why** the change is needed.
  - **What** must change.
  - **How** the smallest coherent solution works.
  - **Why Not** plausible alternatives are unnecessary or worse for the current requirement.
- Prefer simple, direct code with clear responsibilities.
- Simplicity is not an excuse for brittle, duplicated, or hard-to-maintain code. Keep the design easy to understand and able to accommodate reasonable changes to current requirements.
- Introduce an abstraction only when the present code or requirement demonstrates the need for it.

## Prohibited changes

- Do not overengineer.
- Do not add speculative considerations, capabilities, extension points, or features that are not needed now.
- Do not add dependencies.
