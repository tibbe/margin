---
name: software-design
description:
  Design the code before changing it. Use before implementing a feature or
  fixing a bug.
---

# Software design

Before changing code, whether for a new feature or a bug fix, decide how the
code should look, so that it reads as if it had been designed for what it now
does from the start rather than changed to fit it afterwards.

## Process

### 1. Prior art

Assume the problem has been solved before, and research how in parallel
sub-agents, so each search stays out of the others' context and yours.

1. Frame the problem: what the change must do.
2. Spawn one sub-agent per kind of prior art the problem has, all at once:
   - **Language**: the language's idiomatic patterns for this code structure.
   - **Library**: the library's or framework's own recommended approach, from
     its documentation.
   - **Applications**: how other applications handle this behavior.

   Give each the framing and this brief: "Search the web for how this problem is
   solved. Report every principled design the sources take, each with its
   trade-offs and the sources it comes from. Under 400 words."

3. Compare the designs the sub-agents report.

### 2. Choose

Choose the design that best solves this problem. Prefer the design that makes
illegal states unrepresentable: precise types and data structures, and where
the type system can't express an invariant, a type whose only constructor
enforces it. Bugs are then ruled out rather than patched. For a bug fix, this
means finding the design that makes the bug impossible, not just the line that
causes it.

### 3. Plan

Make the change easy, then make the easy change. List where the current code
departs from the chosen design: those parts are restructured first, so the
change itself only adds to the new structure.

## Output

End with the design, for the implementation that follows:

- the chosen design and why, with its sources;
- the designs not chosen, each with its trade-offs;
- the restructuring the change needs first, and then the change.
