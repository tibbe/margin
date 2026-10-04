# Rust crates

`tools/check.sh` runs the crates' checks: `cargo fmt`, clippy, the tests and
the docs, with warnings denied. While working, run the tests at hand, e.g.
`cargo test -p margin-core anchor`, and `cargo fmt --all` to format.
