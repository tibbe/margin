# Coding Standards

## Design

- Write code to be correct by construction, relying on principles such as
  [Make Illegal States Unrepresentable](https://blog.janestreet.com/effective-ml-revisited/)
  and [Parse, don't validate](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/).

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
