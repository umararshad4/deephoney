# Contributing

DeepHoney is an early macOS research application. Small changes with a clear user-visible result are easiest to review.

## Run locally

Use macOS 15+, current stable Rust, and Apple Command Line Tools. Rust 1.99.1 was used for local validation. Clone the repository and run:

```sh
cargo run -p app --locked
```

Use your own provider credentials in Settings. Keep credentials, session databases, and local reports out of commits. Never include private research evidence in an issue or screenshot.

## Check a change

For engine behavior, start with the focused test that exercises the changed behavior. Before opening a pull request, run:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Build the desktop app for a broad UI or dependency change. Drive any changed UI flow in the actual app. Live provider tests are opt-in and may fail when external services are unavailable.

Match the surrounding code style and keep documentation current. The [development reference](docs/development.md) describes the engine interface, fixtures, and live checks.

## Report a bug

Include your macOS version, DeepHoney version or commit, selected mode and provider, reproduction steps, expected result, and the error message. Redact keys, personal data, and private topics. A local fixture that reproduces a provider parsing failure is useful.

## Versions

The Cargo workspace owns the version shared by both crates. Preview tags use `vMAJOR.MINOR.PATCH-alpha.N`; stable tags use `vMAJOR.MINOR.PATCH`. A GitHub prerelease must match the Cargo version and changelog entry. Releases currently distribute source code.
