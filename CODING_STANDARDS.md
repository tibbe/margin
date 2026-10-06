# Coding standards

## Correct by construction

- **Make illegal states unrepresentable**: shape your types so that only valid
  combinations of data can be constructed.
- **Parse, don't validate**: a check that returns nothing forces every later
  caller to re-handle a case that's already been ruled out, so have it return a
  narrower type carrying what it learned, and do it once at the system boundary.

## Functional core, imperative shell

Decisions live in pure functions; I/O, time, randomness and UI live in a thin
layer that calls them.

## Documentation

- Link to the primary source, such as a spec, RFC or API doc, instead of
  copying its content into comments.

## Testing

### Anti-patterns

- **Tautological**: the assertion recomputes the expected value the way the
  code does (`expect(add(a, b)).toBe(a + b)`, a snapshot derived by hand the
  same way, a constant asserted equal to itself), so it passes by construction
  and can never disagree with the code. Expected values must come from an
  independent source of truth: a known-good literal, a worked example, the
  spec.
