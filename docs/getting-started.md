# Getting Started

`mav` is the public smeltery workspace for building and maintaining a Mav-based editor distribution. The repository keeps the editor source, extension host, bundled extension examples, assets, Nix packaging, and development tooling. Website content, community automation, hosted-service legal pages, and release machinery that is not needed for development have been removed.

## Prerequisites

- macOS or Linux for local development.
- GitHub access to `smeltery/mav`.
- Flox for the default development shell.
- Nix with flakes enabled when validating package builds.
- A working Rust toolchain. The Flox activation hook respects `rust-toolchain.toml`.

## First Run

```bash
git clone https://github.com/smeltery/mav.git
cd mav
flox activate
cargo run -p mav --bin mav
```

The first build downloads Rust dependencies, native libraries, grammars, and extension fixtures. Expect the first build to take significantly longer than later incremental builds.

## Repository Shape

The workspace is intentionally large because the editor binary links through many internal crates. Do not prune crates based only on names. Use `cargo metadata` before removing any crate so reverse dependencies are visible.

```bash
cargo metadata --format-version=1 --no-deps
cargo tree -p mav --edges normal,build
```

## Daily Loop

```bash
flox activate
cargo fmt --all
cargo check -p mav --bin mav
cargo test -p extension_api
```

Use `nix develop` when you specifically need to debug the Nix shell or package closure. Use `flox activate` for the normal contributor path.
