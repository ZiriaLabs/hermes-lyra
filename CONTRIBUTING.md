# Contributing to hermes-lyra

Thank you for your interest in contributing. This document explains how to contribute and where different types of feedback belong.

## Types of Contributions

### Documentation Improvements

We welcome improvements to the [specification](docs/specification.md) — typo fixes, clarity improvements, better examples, and new guides. Documentation lives in the `docs/` directory.

### Bug Reports

Found a bug in the spec, documentation, or reference library? Open an issue on the project's GitHub repo.

### Proposals, Questions, and Feedback

Feature requests, spec design questions, and general feedback go in GitHub Discussions. Concrete bugs go in Issues. We maintain a high bar for additions to the spec — it is much easier to add things to a specification than to remove them. Every new field, effect, or shape adds complexity that all implementers must understand and support. When in doubt, leave it out.

### Reference Library

Bug fixes, additional tests, and clarification of public API docs are welcome. Architectural changes belong in a Discussion first.

Before opening a PR:

- Run `cargo test --release`. All tests must pass.
- Run `cargo fmt --all` and `cargo clippy --workspace -- -D warnings`.
- Any change to canonical bytes, the protocol identifier, or field semantics requires a protocol version bump.
- New public API must be documented inline. One short line per item.

### What We're Not Accepting (Yet)

To keep the project focused during this early stage, we are currently not accepting:

- **New optional fields.** The spec's appeal is that there is exactly one valid descriptor shape.
- **Effect tags beyond the closed v0.2 vocabulary.** Extension requires a spec revision.
- **Convenience wrappers around sealed types** that could let callers construct invalid state.

If you're unsure whether your contribution fits, open a Discussion before investing significant effort.

## Reporting Limitations

If you find a way to forge a receipt, produce non-deterministic canonical bytes, or otherwise violate a documented protocol guarantee, please open a security advisory before a public issue.

## License

By contributing, you agree that your contributions will be licensed under [Apache 2.0](LICENSE), matching the rest of the repository.
