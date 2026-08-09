# Repository Guidance

## Design principles

- This is a lightweight OSS project. Simplicity is the primary design constraint.
- Apply YAGNI at all times. Implement only what the current requirement needs.
- Do not add functionality, conditional branches, fallbacks, configuration, or abstractions that the current requirement does not need.
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

## Scope discipline

- The current task defines the scope. A valid problem does not imply that it should be fixed in the current change.
- Do not perform opportunistic refactoring, cleanup, renaming, modernization, or consistency fixes unless required to satisfy the current requirement. Report out-of-scope problems separately.
- Before changing code outside the directly affected area, establish why the current requirement cannot be satisfied without it. Do not justify a broader change after deciding to make it.
- Evaluate review feedback against the current requirement and scope. A technically valid suggestion may still belong in a separate change.
- When existing unnecessary functionality, branches, or overdesign are found, do not fold their removal into the current task unless required. Address them in a separate focused PR with its own rationale and validation.
- Stop once the current requirement is satisfied and relevant checks pass. Do not continue improving nearby code.

## Review discipline

- The primary objective of review is to protect the simplest design that satisfies the current requirement. The number of findings or incorporated suggestions is not a quality signal.
- Review is not the primary mechanism for finding defects. Use it to evaluate whether failures will be observable, whether the owning boundary is clear enough to fix them promptly, and whether focused regression tests can detect recurrence without adding unnecessary complexity.
- Treat each automated review as an independent full review, not as verification of only the last fix.
- Do not repeatedly request review after each feedback-driven commit. Address the feedback, add or update the focused regression test when appropriate, run the relevant checks, reply with the decision and evidence, and resolve the thread.
- When automatic review is configured, rely on it by default. Request another review manually only when a material, previously unreviewed change to behavior, contracts, security boundaries, or correctness logic makes the prior review stale, or when a human explicitly asks for it.
- A review comment is not sufficient reason to broaden the change. Accept it only when it identifies a current, in-scope risk; otherwise explain why it is declined.
- Stop the review loop when the current requirement is satisfied, relevant CI and regression tests pass, and all actionable feedback has been addressed or explicitly declined with rationale. Do not seek additional review merely to obtain more certainty or more findings.

## Prohibited changes

- Do not overengineer.
- Do not add speculative considerations, capabilities, extension points, or features that are not needed now.
- Do not add dependencies.
