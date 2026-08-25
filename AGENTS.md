# Agent Instructions

## Scope

This repository is a private smeltery port focused on the editor runtime, assets, extensions, Nix packaging, and developer tooling. Remove public-project process surface when it is unrelated to private development.

## Development

- Prefer existing workspace patterns over new abstractions.
- Use `cargo metadata` or `cargo tree` before pruning crates.
- Keep `crates/`, `assets/`, `extensions/`, `tooling/`, `script/`, and Nix files coherent.
- Use Flox for normal local checks and Nix for package validation.

## Validation

Run targeted checks for touched areas. For broad or manifest changes, run:

```bash
flox activate
cargo fmt --all -- --check
cargo check -p mav --bin mav --locked
nix flake check
pre-commit run --all-files
```

## Git

Use conventional commits. Do not add AI attribution, generated-by footers, or co-author trailers.
