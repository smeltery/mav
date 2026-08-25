# Contributing to mav

This is a private smeltery repository. Contributions should focus on the editor runtime, extension support, assets, build tooling, Nix packaging, and documentation required to operate the private workspace.

## Local Setup

```bash
flox activate
pre-commit install
cargo check -p mav --bin mav --locked
```

## Pull Requests

Keep pull requests narrow. Explain why the change is needed, what user-visible behavior changes, and which commands validated it.

## Checks

```bash
cargo fmt --all -- --check
cargo check -p mav --bin mav --locked
cargo test -p extension_api
nix flake check
pre-commit run --all-files
```

## Repository Hygiene

Do not reintroduce public website deployment, community issue automation, sponsorship content, or hosted-service legal pages unless smeltery explicitly needs them.

Before removing crates, run:

```bash
cargo metadata --format-version=1 --no-deps
cargo tree -p mav --edges normal,build
```

The editor binary depends on many internal crates through workspace-level composition. A crate name alone is not enough evidence that it is unused.
