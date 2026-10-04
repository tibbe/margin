---
name: software-design
description: Design the code before changing it. Use before implementing a feature or fixing a bug.
---

# Software design

Before changing code, whether for a new feature or a bug fix, decide how the
code should look, so that it reads as if it had been designed for what it now
does from the start rather than changed to fit it afterwards.

Steps:

1. **Fit.** Check whether the current architecture takes the change cleanly.
   If it doesn't, restructure it first and build the change on the new
   structure, rather than piling special cases onto a design that no longer
   fits.
2. **Prior art.** Assume the problem has been solved before, and search the
   web to find how. For a question of code structure, look for the language's
   idiomatic patterns; for a library or framework, its own recommended
   approach, in its documentation; for application behavior, how other
   applications handle it. Many problems have more than one principled
   design, each a different trade-off; find every one the sources take, state
   each with its trade-offs, then choose the one that fits here and say why.
   Cite the sources you base the design on.
3. **Make illegal states unrepresentable.** Model the data so the wrong state
   can't be expressed: precise types and data structures, and where the type
   system can't express an invariant, a type whose only constructor enforces
   it. Bugs are then ruled out rather than patched. For a bug fix, this means
   finding the design that makes the bug impossible, not just the line that
   causes it.
